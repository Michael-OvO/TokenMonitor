<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { currentMonitor } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { isLinux } from "../utils/platform.js";
  import { logger } from "../utils/logger.js";
  import type {
    FloatBallExpandDirection,
    RateLimitProviderId,
    StatusWidgetSummary,
    TrayConfig,
  } from "../types/index.js";
  import {
    detectScreenToPhysicalScale,
    getPhysicalWindowPositionFromPointer,
    shouldHandleFloatBallPointerButton,
  } from "./floatBallInteraction.js";

  const appWindow = getCurrentWebviewWindow();
  const IS_LINUX = isLinux();
  const DRAG_THRESHOLD_PX = 5;
  const BLUR_GUARD_MS = 180;

  const EMPTY_CONFIG: TrayConfig = {
    barDisplay: "both",
    barProvider: "claude",
    showPercentages: false,
    percentageFormat: "compact",
    showCost: true,
    costPrecision: "full",
  };

  let summary = $state<StatusWidgetSummary>({
    config: EMPTY_CONFIG,
    totalCost: 0,
    claudeUtil: null,
    codexUtil: null,
    title: "$0.00",
  });
  let expanded = $state(false);
  let nativeExpanded = false;
  let expandDirection = $state<FloatBallExpandDirection>("right");

  let dragging = $state(false);
  let ignoreBlurUntil = 0;
  let suppressShellClick = false;
  let expansionRequestId = 0;

  let dragState: {
    pointerId: number;
    startScreenX: number;
    startScreenY: number;
    startWindowX: number;
    startWindowY: number;
    screenToPhysicalScale: number;
    initiated: boolean;
  } | null = null;

  type WidgetBar = {
    provider: RateLimitProviderId;
    label: string;
    shortLabel: string;
    utilization: number | null;
    color: string;
  };

  // Float ball always shows both providers regardless of Settings.
  const FIXED_PROVIDERS: RateLimitProviderId[] = ["claude", "codex"];

  let bars = $derived.by((): WidgetBar[] =>
    FIXED_PROVIDERS.map((provider) => ({
      provider,
      label: provider === "claude" ? "Claude" : "Codex",
      shortLabel: provider === "claude" ? "CL" : "CX",
      utilization: provider === "claude" ? summary.claudeUtil : summary.codexUtil,
      color: provider === "claude" ? "#d79b64" : "#72aefc",
    })),
  );

  function percent(value: number | null): string {
    return value == null ? "N/A" : `${Math.round(value)}%`;
  }

  function fillWidth(value: number | null): string {
    const safe = value == null ? 0 : Math.max(0, Math.min(value, 100));
    return `${safe}%`;
  }

  function formatBallCost(cost: number): string {
    if (cost <= 0) return "$0";
    if (cost < 1) return `$${cost.toFixed(2)}`;
    if (cost < 10) return `$${cost.toFixed(1)}`;
    return `$${Math.round(cost)}`;
  }

  let refreshFailures = 0;
  async function refreshSummary() {
    try {
      summary = await invoke<StatusWidgetSummary>("get_status_widget_summary");
      refreshFailures = 0;
    } catch (e) {
      refreshFailures++;
      logger.warn("floatBall", `Refresh failed (${refreshFailures}x): ${e}`);
    }
  }

  async function setExpanded(next: boolean) {
    logger.info("floatBall", `${next ? "Expanded" : "Collapsed"}`);
    const requestId = ++expansionRequestId;

    if (next) {
      ignoreBlurUntil = Date.now() + BLUR_GUARD_MS;

      if (expanded) return;

      if (nativeExpanded) {
        expanded = true;
        return;
      }

      try {
        // Pre-determine direction to explicitly prevent visually jumping
        const pos = await invoke<{ x: number; y: number }>("get_float_ball_position");
        const monitor = await currentMonitor();
        if (monitor) {
          const middle = monitor.size.width / 2;
          expandDirection = pos.x > middle ? "left" : "right";
        }
        
        // Let Svelte update DOM with correct flex-direction BEFORE resizing OS window
        await new Promise((r) => requestAnimationFrame(r));

        const nextLayout = await invoke<{ expandDirection: FloatBallExpandDirection }>(
          "set_float_ball_expanded",
          { expanded: true },
        );
        if (requestId !== expansionRequestId) return;
        expandDirection = nextLayout.expandDirection;
        nativeExpanded = true;
        expanded = true;
      } catch (e) {
        if (requestId !== expansionRequestId) return;
        logger.error("floatBall", `Expansion failed: ${e}`);
        expanded = false;
        nativeExpanded = false;
      }
      return;
    }

    if (!expanded && !nativeExpanded) {
      return;
    }

    if (!nativeExpanded) return;

    // Trigger visual collapse before resizing the OS window
    expanded = false;

    // Wait for the CSS transition to play out before shrinking OS window
    await new Promise((r) => setTimeout(r, 260));

    if (requestId !== expansionRequestId) return;

    try {
      await invoke("set_float_ball_expanded", { expanded: false });
    } catch (e) {
      logger.error("floatBall", `Collapse native OS call failed: ${e}`);
    } finally {
      if (requestId === expansionRequestId) {
        nativeExpanded = false;
        expanded = false;
      }
    }
  }

  function releasePointerCapture(target: HTMLElement, pointerId: number) {
    if (target.hasPointerCapture(pointerId)) {
      target.releasePointerCapture(pointerId);
    }
  }

  function onPointerDown(event: PointerEvent) {
    if (!shouldHandleFloatBallPointerButton(event.button, IS_LINUX)) return;
    event.preventDefault();

    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);

    const screenX = event.screenX;
    const screenY = event.screenY;
    const clientX = event.clientX;
    const clientY = event.clientY;

    Promise.all([
      invoke<{ x: number; y: number }>("get_float_ball_position"),
      appWindow.scaleFactor(),
    ])
      .then(([pos, scale]) => {
        dragState = {
          pointerId: event.pointerId,
          startScreenX: screenX,
          startScreenY: screenY,
          startWindowX: pos.x,
          startWindowY: pos.y,
          screenToPhysicalScale: detectScreenToPhysicalScale({
            scale,
            windowX: pos.x,
            windowY: pos.y,
            clientX,
            clientY,
            screenX,
            screenY,
          }),
          initiated: false,
        };
      })
      .catch(() => {
        releasePointerCapture(target, event.pointerId);
      });
  }

  function onPointerMove(event: PointerEvent) {
    if (!dragState || event.pointerId !== dragState.pointerId) return;

    const dx = event.screenX - dragState.startScreenX;
    const dy = event.screenY - dragState.startScreenY;

    if (!dragState.initiated && Math.hypot(dx, dy) >= DRAG_THRESHOLD_PX) {
      dragState.initiated = true;
      dragging = true;
    }

    if (dragState.initiated) {
      const { x: newX, y: newY } = getPhysicalWindowPositionFromPointer({
        startScreenX: dragState.startScreenX,
        startScreenY: dragState.startScreenY,
        startWindowX: dragState.startWindowX,
        startWindowY: dragState.startWindowY,
        screenX: event.screenX,
        screenY: event.screenY,
        screenToPhysicalScale: dragState.screenToPhysicalScale,
      });
      invoke("move_float_ball_to", { x: newX, y: newY }).catch((e) => {
        logger.debug("floatBall", `move failed: ${e}`);
      });
    }
  }

  function onPointerUp(event: PointerEvent) {
    if (!dragState || event.pointerId !== dragState.pointerId) return;

    const target = event.currentTarget as HTMLElement;
    releasePointerCapture(target, event.pointerId);

    const wasDragging = dragState.initiated;
    dragState = null;
    dragging = false;
    suppressShellClick = true;

    if (wasDragging) {
      if (!expanded) {
        invoke("snap_float_ball").catch((e) => {
          logger.debug("floatBall", `snap failed: ${e}`);
        });
      }
    } else {
      void setExpanded(!expanded);
    }
  }

  function onShellClick(event: MouseEvent) {
    if (!expanded) return;
    if (dragging) return;
    if (suppressShellClick) {
      suppressShellClick = false;
      return;
    }

    const target = event.target;
    if (target instanceof HTMLElement && target.closest(".ball-handle")) {
      return;
    }

    void setExpanded(false);
  }

  function onShellKeyDown(event: KeyboardEvent) {
    if (!expanded) return;
    if (event.key !== "Enter" && event.key !== " ") return;

    const target = event.target;
    if (target instanceof HTMLElement && target.closest(".ball-handle")) {
      return;
    }

    event.preventDefault();
    void setExpanded(false);
  }

  onMount(() => {
    void refreshSummary();

    let destroyed = false;
    const cleanups: (() => void)[] = [];

    function registerListener(promise: Promise<() => void>) {
      promise.then((fn) => {
        if (destroyed) {
          fn(); // Already unmounted — clean up immediately.
        } else {
          cleanups.push(fn);
        }
      });
    }

    registerListener(listen("status-widget-updated", () => refreshSummary()));
    registerListener(listen("data-updated", () => refreshSummary()));
    registerListener(
      listen("tauri://blur", () => {
        if (expanded && !dragging && Date.now() >= ignoreBlurUntil) {
          void setExpanded(false);
        }
      }),
    );

    return () => {
      destroyed = true;
      for (const fn of cleanups) fn();
    };
  });
</script>

<div
  class="shell"
  class:expanded
  class:linux={IS_LINUX}
  data-direction={expandDirection}
  role="button"
  aria-label="Floating status widget"
  aria-expanded={expanded}
  tabindex="0"
  onclick={onShellClick}
  onkeydown={onShellKeyDown}
  oncontextmenu={(e) => e.preventDefault()}
>
  <div class="capsule" class:expanded>
    <button
      class="ball-handle"
      class:expanded={expanded}
      type="button"
      aria-label="Toggle floating status widget"
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
    >
      <span class="ball-label">{formatBallCost(summary.totalCost)}</span>
    </button>

    {#if expanded}
      <section class="panel">
        <div class="bars">
          {#each bars as bar}
            <div class="usage-row">
              <span class="provider-tag" style:color={bar.color}>{bar.shortLabel}</span>
              <div class="bar-track">
                <div
                  class="bar-fill"
                  style={`width:${fillWidth(bar.utilization)}; background:${bar.color}; box-shadow: 0 0 6px ${bar.color}60, inset 0 1px 1px rgba(255,255,255,0.4);`}
                ></div>
              </div>
              <span class="pct" style:color={bar.color}>{percent(bar.utilization)}</span>
            </div>
          {/each}
        </div>
      </section>
    {/if}
  </div>
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
    font-family: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }

  :global(#float-ball) {
    width: 100%;
    height: 100%;
    background: transparent;
  }

  .shell {
    width: 100%;
    height: 100%;
    /* We handle flex in the capsule now */
    background: transparent;
    overflow: visible;
    outline: none;
    -webkit-tap-highlight-color: transparent;
  }

  .shell.expanded {
    isolation: isolate;
  }

  .capsule {
    width: 56px;
    height: 56px;
    border-radius: 28px;
    display: flex;
    align-items: center;
    background:
      radial-gradient(circle at 28px 28px, rgba(255, 255, 255, 0.15) 0%, rgba(255, 255, 255, 0) 45%),
      linear-gradient(145deg, rgba(35, 40, 48, 0.85) 0%, rgba(15, 17, 22, 0.95) 100%);
    box-shadow:
      0 0 0 1px rgba(255, 255, 255, 0.08),
      inset 0 1px 2px rgba(255, 255, 255, 0.2),
      inset 0 -2px 8px rgba(0, 0, 0, 0.5);
    transition:
      width 250ms cubic-bezier(0.175, 0.885, 0.32, 1.1),
      background 200ms ease,
      box-shadow 200ms ease;
    overflow: hidden;
  }

  .capsule.expanded {
    width: 100%; /* expands fully to match OS window */
    background:
      radial-gradient(circle at 28px 28px, rgba(255, 255, 255, 0.12) 0%, rgba(255, 255, 255, 0) 45%),
      linear-gradient(145deg, rgba(30, 35, 42, 0.9) 0%, rgba(12, 14, 18, 0.98) 100%);
    box-shadow:
      0 0 0 1px rgba(255, 255, 255, 0.12),
      inset 0 1px 2px rgba(255, 255, 255, 0.15),
      inset 0 -2px 8px rgba(0, 0, 0, 0.6);
  }

  /* Linux: disable width transition to prevent ghost artifacts on transparent windows */
  .shell.linux .capsule {
    transition:
      background 200ms ease,
      box-shadow 200ms ease;
  }

  .shell[data-direction="left"] .capsule {
    margin-left: auto; /* keeps anchored right when stretching left */
    flex-direction: row-reverse;
  }

  /* The pseudo-handle just grabs and takes events without having its own background */
  .ball-handle {
    width: 56px;
    height: 56px;
    border: none;
    border-radius: 50%;
    padding: 0;
    flex: 0 0 56px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: grab;
    outline: none;
    background: transparent;
    color: rgba(255, 255, 255, 0.95);
    position: relative;
    z-index: 2;
    transition: transform 150ms cubic-bezier(0.175, 0.885, 0.32, 1.275);
  }

  .ball-handle:hover, .capsule.expanded .ball-handle:hover {
    background: radial-gradient(circle at 50% 50%, rgba(255, 255, 255, 0.06) 0%, transparent 60%);
  }

  .ball-handle:active {
    cursor: grabbing;
    transform: scale(0.92);
  }

  .ball-label {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: -0.02em;
    font-variant-numeric: tabular-nums;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.34);
    pointer-events: none;
    user-select: none;
  }

  .panel {
    flex: 1;
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 0 18px 0 0; /* padding right when expanded right */
    animation: panelReveal 250ms cubic-bezier(0.16, 1, 0.3, 1) forwards;
    opacity: 0;
  }

  .shell[data-direction="left"] .panel {
    padding: 0 0 0 18px; /* invert padding for left expansion */
    animation-name: panelRevealLeft;
  }

  @keyframes panelReveal {
    0% { opacity: 0; transform: translateX(8px); }
    100% { opacity: 1; transform: translateX(0); }
  }

  @keyframes panelRevealLeft {
    0% { opacity: 0; transform: translateX(-8px); }
    100% { opacity: 1; transform: translateX(0); }
  }

  .bars {
    display: flex;
    flex-direction: column;
    gap: 8px; /* more spacing, sleeker look */
  }

  .usage-row {
    display: grid;
    grid-template-columns: 16px 1fr 24px;
    align-items: center;
    gap: 8px;
  }

  .shell[data-direction="left"] .usage-row {
    direction: rtl; /* swap track and tag visually without changing DOM */
  }

  .shell[data-direction="left"] .usage-row > * {
    direction: ltr; /* keep inner text orientation natural */
  }

  .provider-tag {
    font-size: 10px;
    line-height: 1;
    font-weight: 800;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    text-shadow: 0 0 6px currentColor; /* neon glow trick */
    opacity: 0.9;
  }

  .bar-track {
    height: 4px; /* sleeker, thinner track */
    border-radius: 999px;
    overflow: hidden;
    background: rgba(0, 0, 0, 0.4);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.8), 0 1px 0 rgba(255, 255, 255, 0.05);
  }

  .bar-fill {
    height: 100%;
    border-radius: inherit;
    transition: width 400ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .pct {
    font-size: 11px;
    line-height: 1;
    font-weight: 700;
    color: rgba(226, 232, 240, 0.95);
    font-variant-numeric: tabular-nums;
    text-align: right; /* right aligned numbers */
  }

  .shell[data-direction="left"] .pct {
    text-align: left;
  }
</style>
