<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { currentMonitor } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";
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
  const COLLAPSE_TRANSITION_MS = 260;
  const BALL_SIZE_CSS_PX = 56;
  const EXPANDED_WIDTH_CSS_PX = 152;
  const EXPAND_MARGIN_CSS_PX = 8;

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
  let interactionCounter = 0;
  let moveSequence = 0;
  let collapsedAnchor = $state<"top" | "left" | "right" | "bottom" | null>(null);
  let queuedMove:
    | {
        x: number;
        y: number;
        interactionId: string | null | undefined;
      }
    | null = null;
  let moveFlushRaf = 0;
  let moveInFlight = false;

  let activePointer: {
    pointerId: number;
    button: number;
    interactionId: string;
  } | null = null;
  let dragGeneration = 0;
  let dragState: {
    pointerId: number;
    button: number;
    interactionId: string;
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

  type FloatBallPositionPayload = {
    x: number;
    y: number;
    anchor?: "top" | "left" | "right" | "bottom" | null;
    expanded?: boolean;
    ballOffsetX?: number;
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

  function formatPoint(point: { x: number; y: number } | null): string {
    return point ? `(${point.x}, ${point.y})` : "n/a";
  }

  function formatMonitor(
    monitor: { position: { x: number; y: number }; size: { width: number; height: number } } | null,
  ): string {
    if (!monitor) return "none";
    return `pos=${formatPoint(monitor.position)} size=${monitor.size.width}x${monitor.size.height}`;
  }

  function formatError(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  function nextInteractionId(kind: string): string {
    interactionCounter += 1;
    return `${kind}-${Date.now().toString(36)}-${interactionCounter}`;
  }

  function formatInteraction(interactionId: string | null | undefined): string {
    return `interaction=${interactionId ?? "n/a"}`;
  }

  function resolveExpandDirection(
    position: FloatBallPositionPayload,
    monitor:
      | {
          position: { x: number; y: number };
          size: { width: number; height: number };
          workArea?: {
            position: { x: number; y: number };
            size: { width: number; height: number };
          };
          scaleFactor?: number;
        }
      | null,
    currentDirection: FloatBallExpandDirection,
  ): FloatBallExpandDirection {
    if (position.anchor === "left") return "right";
    if (position.anchor === "right") return "left";
    if (!monitor) return currentDirection;

    const workArea = monitor.workArea ?? {
      position: monitor.position,
      size: monitor.size,
    };
    const scale = monitor.scaleFactor ?? 1;
    const ballWidth = Math.round(BALL_SIZE_CSS_PX * scale);
    const expandedWidth = Math.round(EXPANDED_WIDTH_CSS_PX * scale);
    const expandMargin = Math.round(EXPAND_MARGIN_CSS_PX * scale);
    const innerLeft = workArea.position.x + expandMargin;
    const innerRight = workArea.position.x + workArea.size.width - expandMargin;
    const roomRight = innerRight - (position.x + expandedWidth);
    const roomLeft = position.x - (expandedWidth - ballWidth) - innerLeft;

    if (currentDirection === "right") {
      if (roomRight >= 0) return "right";
      if (roomLeft >= 0) return "left";
    } else {
      if (roomLeft >= 0) return "left";
      if (roomRight >= 0) return "right";
    }

    if (roomRight > roomLeft) return "right";
    if (roomLeft > roomRight) return "left";
    return currentDirection;
  }

  async function getFloatBallPosition(
    interactionId: string | null | undefined,
    reason: string,
  ): Promise<FloatBallPositionPayload> {
    logger.debug(
      "floatBall",
      `Position invoke: ${formatInteraction(interactionId)} reason=${reason}`,
    );
    return invoke<FloatBallPositionPayload>("get_float_ball_position", {
      interactionId,
    }).catch((error) => {
      logger.warn(
        "floatBall",
        `Position invoke failed: ${formatInteraction(interactionId)} reason=${reason} error=${formatError(error)}`,
      );
      throw error;
    });
  }

  function syncCollapsedAnchor(position: FloatBallPositionPayload | null | undefined) {
    collapsedAnchor = position && !position.expanded ? (position.anchor ?? null) : null;
  }

  async function refreshSummary() {
    try {
      summary = await invoke<StatusWidgetSummary>("get_status_widget_summary");
      logger.debug(
        "floatBall",
        `Summary refreshed: total=${summary.totalCost} claude=${summary.claudeUtil ?? "n/a"} codex=${summary.codexUtil ?? "n/a"}`,
      );
    } catch {
      // Keep the last good payload visible.
    }
  }

  async function setExpanded(
    next: boolean,
    source = "unknown",
    interactionId = nextInteractionId(next ? "expand" : "collapse"),
  ) {
    const requestId = ++expansionRequestId;
    logger.info(
      "floatBall",
      `setExpanded requested: ${formatInteraction(interactionId)} source=${source} next=${next} requestId=${requestId} expanded=${expanded} nativeExpanded=${nativeExpanded} direction=${expandDirection}`,
    );

    if (next) {
      collapsedAnchor = null;

      if (expanded) {
        logger.debug(
          "floatBall",
          `Expand skipped: ${formatInteraction(interactionId)} source=${source} already visual requestId=${requestId}`,
        );
        return;
      }

      // On Linux: window is always expanded-size, no native resize needed.
      // On other platforms: need to resize the native window.
      if (!IS_LINUX) {
        ignoreBlurUntil = Date.now() + BLUR_GUARD_MS;

        if (nativeExpanded) {
          expanded = true;
          collapsedAnchor = null;
          logger.debug(
            "floatBall",
            `Expand reused native state: ${formatInteraction(interactionId)} source=${source} requestId=${requestId}`,
          );
          return;
        }
      }

      try {
        const pos = await getFloatBallPosition(interactionId, "expand-preflight");
        const monitor = await currentMonitor();
        logger.debug(
          "floatBall",
          `Expand preflight: ${formatInteraction(interactionId)} source=${source} requestId=${requestId} pos=${formatPoint(pos)} monitor=${formatMonitor(monitor)}`,
        );
        expandDirection = resolveExpandDirection(pos, monitor, expandDirection);

        // Let Svelte update DOM with correct flex-direction BEFORE backend call
        await new Promise((r) => requestAnimationFrame(r));

        // Backend: on Linux this just repositions window + updates input shape.
        // On other platforms it resizes the native window.
        const nextLayout = await invoke<{ expandDirection: FloatBallExpandDirection }>(
          "set_float_ball_expanded",
          { expanded: true, interactionId, source },
        );
        if (requestId !== expansionRequestId) return;
        expandDirection = nextLayout.expandDirection;
        nativeExpanded = true;
        expanded = true;
        collapsedAnchor = null;

        if (!IS_LINUX) {
          ignoreBlurUntil = Date.now() + BLUR_GUARD_MS;
        }
        logger.info(
          "floatBall",
          `Expand applied: ${formatInteraction(interactionId)} source=${source} requestId=${requestId} direction=${nextLayout.expandDirection}`,
        );
      } catch (e) {
        if (requestId !== expansionRequestId) return;
        logger.error(
          "floatBall",
          `Expansion failed: ${formatInteraction(interactionId)} source=${source} requestId=${requestId} error=${formatError(e)}`,
        );
        expanded = false;
        nativeExpanded = false;
      }
      return;
    }

    if (!expanded) {
      return;
    }

    // On non-Linux, also check nativeExpanded
    if (!IS_LINUX && !nativeExpanded) return;

    // Trigger visual collapse first (ordering: CSS BEFORE shrinking input shape)
    expanded = false;
    logger.debug(
      "floatBall",
      `Collapse visual phase: ${formatInteraction(interactionId)} source=${source} requestId=${requestId} isLinux=${IS_LINUX} waitMs=${IS_LINUX ? 0 : COLLAPSE_TRANSITION_MS}`,
    );

    await tick();
    await new Promise((r) => requestAnimationFrame(r));

    // On non-Linux: wait for CSS width transition to complete before native resize
    if (!IS_LINUX) {
      await new Promise((r) => setTimeout(r, COLLAPSE_TRANSITION_MS));
    }

    if (requestId !== expansionRequestId) return;

    try {
      // Backend: on Linux this just updates input shape (no resize).
      const nextLayout = await invoke<{ expandDirection: FloatBallExpandDirection }>(
        "set_float_ball_expanded",
        { expanded: false, interactionId, source },
      );
      expandDirection = nextLayout.expandDirection;
      syncCollapsedAnchor(await getFloatBallPosition(interactionId, "collapse-post"));
      logger.info(
        "floatBall",
        `Collapse applied: ${formatInteraction(interactionId)} source=${source} requestId=${requestId} direction=${nextLayout.expandDirection}`,
      );
    } catch (e) {
      logger.error(
        "floatBall",
        `Collapse native OS call failed: ${formatInteraction(interactionId)} source=${source} requestId=${requestId} error=${formatError(e)}`,
      );
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

  function moveFloatBallTo(
    x: number,
    y: number,
    interactionId: string | null | undefined,
  ): Promise<void> {
    const sequence = ++moveSequence;
    logger.debug(
      "floatBall",
      `Move invoke: ${formatInteraction(interactionId)} sequence=${sequence} target=${formatPoint({ x, y })}`,
    );
    return invoke<void>("move_float_ball_to", { x, y, sequence, interactionId }).catch((error) => {
      logger.warn(
        "floatBall",
        `Move invoke failed: ${formatInteraction(interactionId)} sequence=${sequence} target=${formatPoint({ x, y })} error=${formatError(error)}`,
      );
      throw error;
    });
  }

  function cancelQueuedMove() {
    queuedMove = null;
    if (moveFlushRaf !== 0) {
      cancelAnimationFrame(moveFlushRaf);
      moveFlushRaf = 0;
    }
  }

  function requestMoveFlush() {
    if (moveFlushRaf !== 0) return;
    moveFlushRaf = requestAnimationFrame(() => {
      moveFlushRaf = 0;
      void flushQueuedMove();
    });
  }

  async function flushQueuedMove() {
    if (moveInFlight || !queuedMove) return;
    const nextMove = queuedMove;
    queuedMove = null;
    moveInFlight = true;

    try {
      await moveFloatBallTo(nextMove.x, nextMove.y, nextMove.interactionId);
    } catch {
      // moveFloatBallTo already logs failures.
    } finally {
      moveInFlight = false;
      if (queuedMove) {
        requestMoveFlush();
      }
    }
  }

  function queueMoveFloatBallTo(
    x: number,
    y: number,
    interactionId: string | null | undefined,
  ) {
    queuedMove = { x, y, interactionId };
    if (!moveInFlight) {
      requestMoveFlush();
    }
  }

  function snapFloatBall(interactionId: string | null | undefined): Promise<void> {
    logger.info("floatBall", `Snap invoke: ${formatInteraction(interactionId)}`);
    return invoke<void>("snap_float_ball", { interactionId })
      .then(async () => {
        syncCollapsedAnchor(await getFloatBallPosition(interactionId, "snap-post"));
      })
      .catch((error) => {
        logger.warn(
          "floatBall",
          `Snap invoke failed: ${formatInteraction(interactionId)} error=${formatError(error)}`,
        );
        throw error;
      });
  }

  function resetPointerInteraction(target: HTMLElement, pointerId: number) {
    releasePointerCapture(target, pointerId);
    cancelQueuedMove();
    // Linux: restore input shape if drag was active
    if (IS_LINUX && dragState?.initiated) {
      void invoke("set_float_ball_dragging", {
        dragging: false,
        interactionId: dragState.interactionId,
      }).catch(() => {});
    }
    dragGeneration += 1;
    activePointer = null;
    dragState = null;
    dragging = false;
  }

  function onPointerDown(event: PointerEvent) {
    if (!shouldHandleFloatBallPointerButton(event.button, IS_LINUX)) return;
    event.preventDefault();
    const interactionId = nextInteractionId(event.button === 2 ? "secondary" : "pointer");
    logger.info(
      "floatBall",
      `Pointer down: ${formatInteraction(interactionId)} pointerId=${event.pointerId} button=${event.button} screen=${formatPoint({ x: event.screenX, y: event.screenY })} client=${formatPoint({ x: event.clientX, y: event.clientY })} expanded=${expanded} nativeExpanded=${nativeExpanded}`,
    );

    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);

    const screenX = event.screenX;
    const screenY = event.screenY;
    const clientX = event.clientX;
    const clientY = event.clientY;

    const gen = ++dragGeneration;
    activePointer = {
      pointerId: event.pointerId,
      button: event.button,
      interactionId,
    };
    dragState = null;
    dragging = false;

    Promise.all([
      getFloatBallPosition(interactionId, "pointerdown"),
      appWindow.scaleFactor(),
    ])
      .then(([pos, scale]) => {
        if (gen !== dragGeneration || activePointer?.pointerId !== event.pointerId) return;
        const screenToPhysicalScale = detectScreenToPhysicalScale({
          scale,
          windowX: pos.x,
          windowY: pos.y,
          clientX,
          clientY,
          screenX,
          screenY,
        });
        dragState = {
          pointerId: event.pointerId,
          button: event.button,
          interactionId,
          startScreenX: screenX,
          startScreenY: screenY,
          startWindowX: pos.x,
          startWindowY: pos.y,
          screenToPhysicalScale,
          initiated: false,
        };
        logger.debug(
          "floatBall",
          `Pointer init ready: ${formatInteraction(interactionId)} pointerId=${event.pointerId} button=${event.button} startWindow=${formatPoint(pos)} scale=${scale} detectedScale=${screenToPhysicalScale}`,
        );
        syncCollapsedAnchor(pos);
      })
      .catch((error) => {
        if (gen !== dragGeneration || activePointer?.pointerId !== event.pointerId) return;
        logger.warn(
          "floatBall",
          `Pointer init failed: ${formatInteraction(interactionId)} pointerId=${event.pointerId} error=${formatError(error)}`,
        );
        resetPointerInteraction(target, event.pointerId);
      });
  }

  function onPointerMove(event: PointerEvent) {
    if (!dragState || event.pointerId !== dragState.pointerId) return;
    if (dragState.button !== 0) return; // Only allow drag with left click

    const dx = event.screenX - dragState.startScreenX;
    const dy = event.screenY - dragState.startScreenY;

    if (!dragState.initiated && Math.hypot(dx, dy) >= DRAG_THRESHOLD_PX) {
      dragState.initiated = true;
      dragging = true;
      collapsedAnchor = null;
      // Linux: expand input shape to full window so pointer events keep
      // arriving even when the cursor leaves the 56×56 ball region.
      if (IS_LINUX) {
        void invoke("set_float_ball_dragging", {
          dragging: true,
          interactionId: dragState.interactionId,
        }).catch(() => {});
      }
      logger.info(
        "floatBall",
        `Drag started: ${formatInteraction(dragState.interactionId)} pointerId=${event.pointerId} startWindow=${formatPoint({ x: dragState.startWindowX, y: dragState.startWindowY })} delta=${formatPoint({ x: dx, y: dy })} scale=${dragState.screenToPhysicalScale}`,
      );
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
      logger.debug(
        "floatBall",
        `Drag move: ${formatInteraction(dragState.interactionId)} pointerId=${event.pointerId} target=${formatPoint({ x: newX, y: newY })} screen=${formatPoint({ x: event.screenX, y: event.screenY })}`,
      );
      queueMoveFloatBallTo(newX, newY, dragState.interactionId);
    }
  }

  function onPointerUp(event: PointerEvent) {
    if (!activePointer || event.pointerId !== activePointer.pointerId) return;

    const target = event.currentTarget as HTMLElement;
    const pointerState = dragState?.pointerId === event.pointerId ? dragState : null;
    const wasDragging = pointerState?.initiated ?? false;
    const finalPosition =
      pointerState && pointerState.button === 0 && pointerState.initiated
        ? getPhysicalWindowPositionFromPointer({
            startScreenX: pointerState.startScreenX,
            startScreenY: pointerState.startScreenY,
            startWindowX: pointerState.startWindowX,
            startWindowY: pointerState.startWindowY,
            screenX: event.screenX,
            screenY: event.screenY,
            screenToPhysicalScale: pointerState.screenToPhysicalScale,
          })
        : null;
    const button = activePointer.button;
    const interactionId = pointerState?.interactionId ?? activePointer.interactionId;
    resetPointerInteraction(target, event.pointerId);
    suppressShellClick = button === 0 && !wasDragging;
    logger.info(
      "floatBall",
      `Pointer up: ${formatInteraction(interactionId)} pointerId=${event.pointerId} button=${button} wasDragging=${wasDragging} final=${formatPoint(finalPosition)} expanded=${expanded} suppressShellClick=${suppressShellClick}`,
    );

    if (button === 2) {
      // Right click: toggle expand
      if (!wasDragging) {
        logger.info(
          "floatBall",
          `Right-click toggle: ${formatInteraction(interactionId)} nextExpanded=${!expanded}`,
        );
        void setExpanded(!expanded, "right-click", interactionId);
      }
    } else if (button === 0) {
      // Left drag: apply the final move.
      if (finalPosition && !expanded) {
        logger.info(
          "floatBall",
          `Drag ended: ${formatInteraction(interactionId)} requesting final move at ${formatPoint(finalPosition)}`,
        );
        cancelQueuedMove();
        void moveFloatBallTo(finalPosition.x, finalPosition.y, interactionId)
          .then(() => {
            // Linux: restore input shape to ball-only after drag
            if (IS_LINUX) {
              return invoke("set_float_ball_dragging", {
                dragging: false,
                interactionId,
              }).catch(() => {});
            }
          })
          .catch(() => {});
      } else if (!wasDragging) {
        logger.debug(
          "floatBall",
          `Left click ended without drag action: ${formatInteraction(interactionId)}`,
        );
      }
    }
  }

  function onPointerCancel(event: PointerEvent) {
    if (!activePointer || event.pointerId !== activePointer.pointerId) return;
    const target = event.currentTarget as HTMLElement;
    logger.info(
      "floatBall",
      `Pointer cancel: ${formatInteraction(activePointer.interactionId)} pointerId=${event.pointerId}`,
    );
    resetPointerInteraction(target, event.pointerId);
  }

  function onShellClick(event: MouseEvent) {
    if (!expanded) return;
    if (dragging) return;
    if (suppressShellClick) {
      logger.debug("floatBall", "Shell click suppressed after ball click");
      suppressShellClick = false;
      return;
    }

    const target = event.target;
    if (target instanceof HTMLElement && target.closest(".ball-handle")) {
      return;
    }

    logger.info("floatBall", "Shell click collapse");
    void setExpanded(false, "shell-click");
  }

  function onShellKeyDown(event: KeyboardEvent) {
    if (!expanded) return;
    if (event.key !== "Enter" && event.key !== " ") return;

    const target = event.target;
    if (target instanceof HTMLElement && target.closest(".ball-handle")) {
      return;
    }

    event.preventDefault();
    logger.info("floatBall", `Shell key collapse: key=${event.key}`);
    void setExpanded(false, "shell-key");
  }

  onMount(() => {
    logger.info("floatBall", `Mounted: linux=${IS_LINUX}`);
    void refreshSummary();
    void getFloatBallPosition(nextInteractionId("mount"), "mount")
      .then((position) => syncCollapsedAnchor(position))
      .catch(() => {});

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
        if (!expanded) return;
        if (dragging) {
          logger.debug("floatBall", "Blur ignored while dragging");
          return;
        }
        if (Date.now() < ignoreBlurUntil) {
          logger.debug("floatBall", "Blur ignored during expand guard window");
          return;
        }

        logger.info("floatBall", "Blur collapse");
        void setExpanded(false, "blur");
      }),
    );

    return () => {
      destroyed = true;
      cancelQueuedMove();
      for (const fn of cleanups) fn();
    };
  });
</script>

<div
  class="shell"
  class:expanded
  class:linux={IS_LINUX}
  data-direction={expandDirection}
  data-collapsed-anchor={collapsedAnchor ?? ""}
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
      onpointercancel={onPointerCancel}
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
  :global(*), :global(*::before), :global(*::after) {
    box-sizing: border-box;
  }

  :global(html),
  :global(body) {
    margin: 0;
    width: 100%;
    height: 100%;
    max-width: 100%;
    max-height: 100%;
    overflow: hidden;
    overflow: clip;
    scrollbar-width: none;
    background: transparent;
    font-family: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }

  /* Suppress WebKitGTK scrollbars that appear when the GTK viewport is
     slightly smaller than content due to size negotiation timing. */
  :global(::-webkit-scrollbar) {
    display: none !important;
    width: 0 !important;
    height: 0 !important;
    background: transparent !important;
  }

  :global(#float-ball) {
    width: 100%;
    height: 100%;
    max-width: 100%;
    max-height: 100%;
    overflow: hidden;
    overflow: clip;
    scrollbar-width: none;
    background: transparent;
  }

  .shell {
    width: 100%;
    height: 100%;
    display: flex;
    justify-content: center;
    align-items: center;
    background: transparent;
    overflow: hidden;
    outline: none !important;
    -webkit-tap-highlight-color: transparent;
  }

  .shell.expanded {
    isolation: isolate;
  }

  /* Linux: fixed-size window — ball position depends on direction ALWAYS
     (not just when expanded), because the window is always 152×56. */
  .shell.linux[data-direction="left"] {
    justify-content: flex-end;
  }

  .shell.linux[data-direction="right"] {
    justify-content: flex-start;
  }

  /* Non-Linux: keep direction-based alignment active even during collapse
     transition so the ball doesn't drift to center while the capsule shrinks. */
  .shell:not(.linux)[data-direction="left"] {
    justify-content: flex-end;
  }

  .shell:not(.linux)[data-direction="right"] {
    justify-content: flex-start;
  }

  .capsule {
    width: 56px;
    height: 56px;
    max-width: 100%;
    max-height: 100%;
    min-width: 0;
    min-height: 0;
    border-radius: 28px;
    display: flex;
    flex-shrink: 0;
    align-items: center;
    background:
      radial-gradient(120% 120% at 30% 10%, rgba(255, 255, 255, 0.15) 0%, rgba(255, 255, 255, 0) 50%),
      linear-gradient(160deg, rgba(30, 34, 45, 0.9) 0%, rgba(10, 12, 16, 0.98) 100%);
    box-shadow:
      inset 0 0 0 1px rgba(255, 255, 255, 0.1),
      inset 0 2px 4px rgba(255, 255, 255, 0.15),
      inset 0 -4px 12px rgba(0, 0, 0, 0.6);
    transition:
      width 250ms cubic-bezier(0.175, 0.885, 0.32, 1.1),
      background 200ms ease,
      box-shadow 200ms ease;
    overflow: hidden;
    transform: translate(0, 0);
  }

  .capsule.expanded {
    width: 100%; /* expands fully to match OS window */
    background:
      radial-gradient(150% 150% at 20% 10%, rgba(255, 255, 255, 0.12) 0%, rgba(255, 255, 255, 0) 40%),
      linear-gradient(160deg, rgba(20, 24, 32, 0.95) 0%, rgba(8, 10, 14, 0.99) 100%);
    box-shadow:
      inset 0 0 0 1px rgba(255, 255, 255, 0.1),
      inset 0 2px 4px rgba(255, 255, 255, 0.12),
      inset 0 -4px 12px rgba(0, 0, 0, 0.8);
  }

  /* Linux: fixed-size window is always 152×56. The capsule fills the window
     when expanded; when collapsed, it shows just the 56×56 ball at one end.
     No width transition needed (no native resize to synchronize with). */
  .shell.linux .capsule {
    width: 56px;
    height: 56px;
    max-height: 100%;
    border-radius: 50%;
    transition:
      background 200ms ease,
      box-shadow 200ms ease;
  }

  .shell.linux .capsule.expanded {
    width: 100%;
    height: 100%;
    border-radius: 999px;
  }

  .shell[data-direction="left"] .capsule {
    /* Anchored right via parent flex-end */
    flex-direction: row-reverse;
  }

  .shell[data-direction="left"] .capsule.expanded {
    background:
      radial-gradient(150% 150% at 80% 10%, rgba(255, 255, 255, 0.12) 0%, rgba(255, 255, 255, 0) 40%),
      linear-gradient(200deg, rgba(20, 24, 32, 0.95) 0%, rgba(8, 10, 14, 0.99) 100%);
  }

  /* Removed clip-path overrides: to maintain physical positional consistency, the capsule must remain a full shape when collapsed. */

  /* The pseudo-handle just grabs and takes events without having its own background */
  .ball-handle {
    width: 56px;
    height: 56px;
    min-width: 56px;
    min-height: 56px;
    max-width: 100%;
    max-height: 100%;
    border: none;
    border-radius: 50%;
    padding: 0;
    flex: 0 0 56px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: grab;
    outline: none !important;
    background: transparent;
    color: rgba(255, 255, 255, 0.95);
    position: relative;
    z-index: 2;
    transition: transform 150ms cubic-bezier(0.175, 0.885, 0.32, 1.275);
  }

  .shell.linux .ball-handle {
    width: 56px;
    height: 56px;
    min-width: 56px;
    min-height: 56px;
    max-width: 100%;
    max-height: 100%;
    flex: 0 0 56px;
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
    min-width: 0;
    min-height: 0;
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
    min-width: 0;
    min-height: 0;
    gap: 8px; /* more spacing, sleeker look */
  }

  .usage-row {
    display: grid;
    grid-template-columns: 16px 1fr 24px;
    min-width: 0;
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
    
    text-shadow: 0 0 6px currentColor; /* neon glow trick */
    opacity: 0.9;
  }

  .bar-track {
    min-width: 0;
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
