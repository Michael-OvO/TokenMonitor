import type { AccordionToggleDetail } from "./types/index.js";
import {
  DEFAULT_MAX_WINDOW_HEIGHT,
  MIN_WINDOW_HEIGHT,
  RESIZE_HYSTERESIS_PX,
  RESIZE_SETTLE_DELAY_MS,
  WINDOW_WIDTH,
  clampWindowHeight,
  classifyResize,
  isWindowScrollLocked,
  measureTargetWindowHeight,
  resolveEffectiveWindowMaxHeight,
  resolveMonitorMaxWindowHeight,
  resolveScrollThresholdHeight,
} from "./windowSizing.js";

export interface ResizeOrchestratorDeps {
  getPopEl: () => HTMLDivElement | null;
  invoke: (cmd: string, args: Record<string, unknown>) => Promise<void>;
  onScrollLockChange: (locked: boolean) => void;
  currentMonitor: () => Promise<{
    workArea: { size: { height: number } };
    scaleFactor: number;
  } | null>;
  logDebug: (event: string, data: Record<string, unknown>) => void;
  captureDebugSnapshot: (reason: string) => Record<string, unknown>;
  formatDebugError: (error: unknown) => { message: string };
  isDebugEnabled: () => boolean;
}

export interface ResizeOrchestrator {
  syncSizeAndVerify: (source?: string) => void;
  animateWindowHeight: (
    targetHeight: number,
    durationMs: number,
    source?: string,
  ) => void;
  followContentDuringTransition: (durationMs: number, source: string) => void;
  resizeToContent: (source?: string) => void;
  handleBreakdownAccordionToggle: (detail: AccordionToggleDetail) => void;
  refreshWindowMetrics: () => Promise<void>;
  destroy: () => void;
  getMaxWindowH: () => number;
  getScrollThresholdH: () => number;
  getIsScrollLocked: () => boolean;
}

export function createResizeOrchestrator(
  deps: ResizeOrchestratorDeps,
): ResizeOrchestrator {
  // ── Internal state (local to this closure) ──
  let resizeRaf = 0;
  let resizeTimer: ReturnType<typeof setTimeout> | null = null;
  let lastWindowH = typeof window === "undefined" ? 0 : window.innerHeight;
  let windowAnimationRaf = 0;
  let windowAnimationToken = 0;
  let isWindowHeightAnimating = false;

  // Resize throttle: max 3 operations per 500ms window
  const RESIZE_THROTTLE_WINDOW_MS = 500;
  const RESIZE_THROTTLE_MAX_OPS = 3;
  /** Smooth animated shrink duration — prevents jarring bottom-anchor jump. */
  const SHRINK_ANIMATE_MS = 150;
  /** After animation ends, suppress observer shrinks for this period to prevent jitter. */
  const POST_ANIMATION_COOLDOWN_MS = 250;
  let shrinkCooldownUntil = 0;
  let resizeThrottleTimestamps: number[] = [];
  let resizeThrottleDeferTimer: ReturnType<typeof setTimeout> | null = null;

  let maxWindowH = DEFAULT_MAX_WINDOW_HEIGHT;
  let scrollThresholdH = DEFAULT_MAX_WINDOW_HEIGHT;
  let isScrollLocked = false;

  // ── Internal helpers ──

  function captureDebugSnapshot(reason: string): Record<string, unknown> {
    return deps.isDebugEnabled()
      ? deps.captureDebugSnapshot(reason)
      : {};
  }

  function clearPendingResize(): void {
    deps.logDebug("resize:clear-pending", {
      hadTimer: Boolean(resizeTimer),
      hadRaf: resizeRaf !== 0,
      ...captureDebugSnapshot("clear-pending"),
    });
    if (resizeTimer) {
      clearTimeout(resizeTimer);
      resizeTimer = null;
    }
    cancelAnimationFrame(resizeRaf);
    resizeRaf = 0;
  }

  function clearWindowHeightAnimation(): void {
    if (windowAnimationRaf !== 0) {
      cancelAnimationFrame(windowAnimationRaf);
      windowAnimationRaf = 0;
    }
    windowAnimationToken += 1;
    isWindowHeightAnimating = false;
  }

  async function refreshWindowMetrics(): Promise<void> {
    try {
      const monitor = await deps.currentMonitor();
      if (!monitor) return;
      maxWindowH = resolveMonitorMaxWindowHeight(
        monitor.workArea.size.height,
        monitor.scaleFactor,
      );
      scrollThresholdH = resolveScrollThresholdHeight(
        monitor.workArea.size.height,
        monitor.scaleFactor,
      );
      deps.logDebug("resize:monitor-metrics", {
        workAreaHeight: monitor.workArea.size.height,
        scaleFactor: monitor.scaleFactor,
        maxWindowH,
        scrollThresholdH,
      });
    } catch {
      maxWindowH = DEFAULT_MAX_WINDOW_HEIGHT;
      scrollThresholdH = DEFAULT_MAX_WINDOW_HEIGHT;
      deps.logDebug("resize:monitor-metrics-fallback", { maxWindowH });
    }
  }

  function getEffectiveWindowMaxHeight(): number {
    return resolveEffectiveWindowMaxHeight(
      maxWindowH,
      scrollThresholdH,
      MIN_WINDOW_HEIGHT,
    );
  }

  function updateScrollLockState(
    nextLocked: boolean,
    source: string,
    rawMeasuredHeight: number | null = null,
    nextHeight: number | null = null,
  ): void {
    if (nextLocked === isScrollLocked) return;
    isScrollLocked = nextLocked;
    deps.onScrollLockChange(nextLocked);
    deps.logDebug(
      nextLocked ? "resize:scroll-lock-enter" : "resize:scroll-lock-exit",
      {
        source,
        rawMeasuredHeight,
        nextHeight,
        scrollThresholdH,
        effectiveMaxWindowH: getEffectiveWindowMaxHeight(),
        ...captureDebugSnapshot(`scroll-lock-${source}`),
      },
    );
  }

  function measureWindowHeight(
    source = "measure",
  ): {
    rawMeasuredHeight: number;
    nextHeight: number;
    scrollLocked: boolean;
    effectiveMaxWindowH: number;
  } | null {
    const popEl = deps.getPopEl();
    if (!popEl) return null;
    const rawMeasuredHeight = measureTargetWindowHeight(popEl.scrollHeight);
    const effectiveMaxWindowH = getEffectiveWindowMaxHeight();
    const scrollLocked = isWindowScrollLocked(
      rawMeasuredHeight,
      effectiveMaxWindowH,
    );
    const nextHeight = clampWindowHeight(
      rawMeasuredHeight,
      effectiveMaxWindowH,
      MIN_WINDOW_HEIGHT,
    );
    updateScrollLockState(scrollLocked, source, rawMeasuredHeight, nextHeight);
    return {
      rawMeasuredHeight,
      nextHeight,
      scrollLocked,
      effectiveMaxWindowH,
    };
  }

  function applyWindowHeight(targetHeight: number, source = "unknown"): void {
    const effectiveMaxWindowH = getEffectiveWindowMaxHeight();
    const nextHeight = clampWindowHeight(
      targetHeight,
      effectiveMaxWindowH,
      MIN_WINDOW_HEIGHT,
    );
    const disposition = classifyResize(
      nextHeight,
      lastWindowH,
      MIN_WINDOW_HEIGHT,
    );
    deps.logDebug("resize:apply-request", {
      source,
      targetHeight,
      nextHeight,
      effectiveMaxWindowH,
      scrollLocked: isScrollLocked,
      disposition,
      deltaFromLast: nextHeight - lastWindowH,
      deltaAbs: Math.abs(nextHeight - lastWindowH),
      hysteresisPx: RESIZE_HYSTERESIS_PX,
      ...captureDebugSnapshot(`apply-${source}`),
    });
    if (disposition === "skip") return;
    lastWindowH = nextHeight;
    deps
      .invoke("set_window_size_and_align", {
        width: WINDOW_WIDTH,
        height: nextHeight,
      })
      .then(() => {
        deps.logDebug("resize:set-size-resolved", {
          source,
          nextHeight,
          ...captureDebugSnapshot(`set-size-resolved-${source}`),
        });
      })
      .catch((error) => {
        deps.logDebug("resize:set-size-rejected", {
          source,
          nextHeight,
          error: deps.formatDebugError(error),
          ...captureDebugSnapshot(`set-size-rejected-${source}`),
        });
        if (typeof window !== "undefined") {
          lastWindowH = window.innerHeight;
        }
      });
  }

  function syncSize(
    source = "unknown",
  ): {
    rawMeasuredHeight: number;
    nextHeight: number;
    scrollLocked: boolean;
    effectiveMaxWindowH: number;
  } | null {
    const measurement = measureWindowHeight(`sync-${source}`);
    deps.logDebug("resize:sync-size", {
      source,
      rawMeasuredHeight: measurement?.rawMeasuredHeight ?? null,
      measuredHeight: measurement?.nextHeight ?? null,
      effectiveMaxWindowH:
        measurement?.effectiveMaxWindowH ?? getEffectiveWindowMaxHeight(),
      scrollLocked: measurement?.scrollLocked ?? isScrollLocked,
      ...captureDebugSnapshot(`sync-${source}`),
    });
    if (!measurement) return null;
    applyWindowHeight(measurement.nextHeight, source);
    return measurement;
  }

  function easeWindowHeight(progress: number): number {
    const t = Math.max(0, Math.min(progress, 1));
    const inverse = 1 - t;
    return 1 - inverse * inverse * inverse;
  }

  function scheduleSettledResize(
    delay = RESIZE_SETTLE_DELAY_MS,
    source = "unknown",
  ): void {
    deps.logDebug("resize:schedule-settled", {
      source,
      delay,
      ...captureDebugSnapshot(`schedule-${source}`),
    });
    clearPendingResize();
    resizeTimer = setTimeout(() => {
      resizeTimer = null;
      resizeRaf = requestAnimationFrame(() => {
        resizeRaf = requestAnimationFrame(() => {
          resizeRaf = 0;
          deps.logDebug("resize:settled-fire", {
            source,
            ...captureDebugSnapshot(`settled-fire-${source}`),
          });
          syncSizeAndVerify(`${source}:settled`);
        });
      });
    }, delay);
  }

  // ── Public API ──

  function syncSizeAndVerify(source = "unknown"): void {
    deps.logDebug("resize:sync-and-verify", { source });
    const measurement = measureWindowHeight(`sync-${source}`);
    if (!measurement) return;

    if (measurement.scrollLocked) {
      deps.logDebug("resize:skipped-scroll-locked", {
        source: `${source}:verify`,
        reason: "skip-settled-remeasure-while-scroll-locked",
        rawMeasuredHeight: measurement.rawMeasuredHeight,
        nextHeight: measurement.nextHeight,
        effectiveMaxWindowH: measurement.effectiveMaxWindowH,
        ...captureDebugSnapshot(`scroll-locked-${source}`),
      });
      applyWindowHeight(measurement.nextHeight, `${source}:scroll-lock`);
      return;
    }

    const disposition = classifyResize(
      measurement.nextHeight,
      lastWindowH,
      MIN_WINDOW_HEIGHT,
    );

    if (disposition === "shrink") {
      // Follow content rather than independent animation — prevents bottom gap
      // on top-anchored windows (Linux) where window > content during JS animation.
      followContentDuringTransition(
        SHRINK_ANIMATE_MS,
        `${source}:shrink`,
      );
      return;
    }

    // Grow / skip: immediate (prevents clipping)
    applyWindowHeight(measurement.nextHeight, `${source}:initial`);
    scheduleSettledResize(100, `${source}:verify`);
  }

  function animateWindowHeight(
    targetHeight: number,
    durationMs: number,
    source = "unknown",
  ): void {
    const startHeight =
      typeof window === "undefined" ? lastWindowH : window.innerHeight;
    const effectiveMaxWindowH = getEffectiveWindowMaxHeight();
    const nextHeight = clampWindowHeight(
      targetHeight,
      effectiveMaxWindowH,
      MIN_WINDOW_HEIGHT,
    );
    const disposition = classifyResize(
      nextHeight,
      startHeight,
      MIN_WINDOW_HEIGHT,
    );
    deps.logDebug("resize:animate-request", {
      source,
      durationMs,
      startHeight,
      targetHeight,
      nextHeight,
      effectiveMaxWindowH,
      scrollLocked: isScrollLocked,
      disposition,
      ...captureDebugSnapshot(`animate-${source}`),
    });

    if (disposition === "skip" || durationMs <= 0) {
      clearWindowHeightAnimation();
      applyWindowHeight(nextHeight, `${source}:immediate`);
      syncSizeAndVerify(`${source}:verify`);
      return;
    }

    clearPendingResize();
    clearWindowHeightAnimation();

    const animationToken = windowAnimationToken;
    const startedAt = performance.now();
    isWindowHeightAnimating = true;
    lastWindowH = startHeight;

    const step = (now: number) => {
      if (animationToken !== windowAnimationToken) return;

      const progress = Math.min((now - startedAt) / durationMs, 1);
      const eased = easeWindowHeight(progress);
      const interpolatedHeight = Math.round(
        startHeight + (nextHeight - startHeight) * eased,
      );
      applyWindowHeight(interpolatedHeight, `${source}:frame`);

      if (progress >= 1) {
        windowAnimationRaf = 0;
        isWindowHeightAnimating = false;
        shrinkCooldownUntil = performance.now() + POST_ANIMATION_COOLDOWN_MS;
        // Final snap: measure and apply directly — no re-animation chain
        const finalMeasurement = measureWindowHeight(`${source}:complete`);
        if (finalMeasurement) {
          applyWindowHeight(
            finalMeasurement.nextHeight,
            `${source}:final`,
          );
        }
        return;
      }

      windowAnimationRaf = requestAnimationFrame(step);
    };

    windowAnimationRaf = requestAnimationFrame(step);
  }

  function followContentDuringTransition(
    durationMs: number,
    source: string,
  ): void {
    clearPendingResize();
    clearWindowHeightAnimation();

    const token = ++windowAnimationToken;
    isWindowHeightAnimating = true;
    const startedAt = performance.now();
    deps.logDebug("resize:follow-start", { source, durationMs });

    const step = () => {
      if (token !== windowAnimationToken) return;

      const measurement = measureWindowHeight(`${source}:follow`);
      if (measurement) {
        applyWindowHeight(measurement.nextHeight, `${source}:follow`);
      }

      const elapsed = performance.now() - startedAt;
      if (elapsed < durationMs + 50) {
        windowAnimationRaf = requestAnimationFrame(step);
      } else {
        windowAnimationRaf = 0;
        isWindowHeightAnimating = false;
        shrinkCooldownUntil = performance.now() + POST_ANIMATION_COOLDOWN_MS;
        deps.logDebug("resize:follow-end", {
          source,
          elapsed: Math.round(elapsed),
        });
        const finalMeasurement = measureWindowHeight(`${source}:complete`);
        if (finalMeasurement) {
          applyWindowHeight(
            finalMeasurement.nextHeight,
            `${source}:final`,
          );
        }
      }
    };

    windowAnimationRaf = requestAnimationFrame(step);
  }

  function resizeToContent(source = "observer"): void {
    if (isWindowHeightAnimating) {
      deps.logDebug("resize:observer-skipped-animation", {
        source,
        ...captureDebugSnapshot(`observer-skipped-${source}`),
      });
      return;
    }

    // Throttle: max N resize ops per window to break cascading loops
    const now = performance.now();
    resizeThrottleTimestamps = resizeThrottleTimestamps.filter(
      (t) => now - t < RESIZE_THROTTLE_WINDOW_MS,
    );
    if (resizeThrottleTimestamps.length >= RESIZE_THROTTLE_MAX_OPS) {
      deps.logDebug("resize:throttled", {
        source,
        opsInWindow: resizeThrottleTimestamps.length,
      });
      // Defer a single re-measure to the next throttle window
      if (!resizeThrottleDeferTimer) {
        resizeThrottleDeferTimer = setTimeout(() => {
          resizeThrottleDeferTimer = null;
          resizeToContent(`${source}:deferred`);
        }, RESIZE_THROTTLE_WINDOW_MS);
      }
      return;
    }
    resizeThrottleTimestamps.push(now);

    const measurement = measureWindowHeight(`resize-to-content-${source}`);
    const rawMeasuredHeight = measurement?.rawMeasuredHeight ?? null;
    const nextHeight = measurement?.nextHeight ?? null;
    const deltaFromLast =
      nextHeight == null ? null : nextHeight - lastWindowH;
    deps.logDebug("resize:observer-measure", {
      source,
      rawMeasuredHeight,
      measuredHeight: nextHeight,
      effectiveMaxWindowH:
        measurement?.effectiveMaxWindowH ?? getEffectiveWindowMaxHeight(),
      scrollLocked: measurement?.scrollLocked ?? isScrollLocked,
      nextHeight,
      lastWindowH,
      deltaFromLast,
      deltaAbs: deltaFromLast == null ? null : Math.abs(deltaFromLast),
      hysteresisPx: RESIZE_HYSTERESIS_PX,
      ...captureDebugSnapshot(`resize-to-content-${source}`),
    });
    if (!measurement || nextHeight == null) return;
    const disposition = classifyResize(
      nextHeight,
      lastWindowH,
      MIN_WINDOW_HEIGHT,
    );

    if (measurement.scrollLocked) {
      if (disposition === "skip") {
        deps.logDebug("resize:skipped-scroll-locked", {
          source,
          reason: "observer-noop-while-scroll-locked",
          rawMeasuredHeight,
          nextHeight,
          lastWindowH,
          ...captureDebugSnapshot(`scroll-locked-${source}`),
        });
        return;
      }

      clearPendingResize();
      applyWindowHeight(nextHeight, `${source}:scroll-lock`);
      deps.logDebug("resize:skipped-scroll-locked", {
        source,
        reason: "skip-animation-and-settled-resize-while-scroll-locked",
        rawMeasuredHeight,
        nextHeight,
        disposition,
        lastWindowH,
        ...captureDebugSnapshot(`scroll-locked-${source}`),
      });
      return;
    }

    // Post-animation cooldown: suppress observer shrinks to prevent jitter
    if (disposition === "shrink" && performance.now() < shrinkCooldownUntil) {
      deps.logDebug("resize:shrink-cooldown-skip", {
        source,
        remainingMs: Math.round(shrinkCooldownUntil - performance.now()),
      });
      return;
    }

    switch (disposition) {
      case "grow":
        clearPendingResize();
        applyWindowHeight(nextHeight, `${source}:grow`);
        // Re-measure after setSize settles
        scheduleSettledResize(100, `${source}:grow`);
        return;
      case "shrink":
        scheduleSettledResize(RESIZE_SETTLE_DELAY_MS, `${source}:shrink`);
        return;
      default:
        return;
    }
  }

  function handleBreakdownAccordionToggle(
    detail: AccordionToggleDetail,
  ): void {
    const direction = detail.expanding ? "expand" : "collapse";
    const source = `breakdown-${detail.scope}-${direction}`;
    if (detail.expanding) {
      const currentHeight =
        typeof window === "undefined" ? lastWindowH : window.innerHeight;
      animateWindowHeight(
        currentHeight + detail.height,
        detail.durationMs,
        source,
      );
    } else {
      followContentDuringTransition(detail.durationMs, source);
    }
  }

  function destroy(): void {
    if (resizeTimer) clearTimeout(resizeTimer);
    if (resizeThrottleDeferTimer) clearTimeout(resizeThrottleDeferTimer);
    cancelAnimationFrame(resizeRaf);
    clearWindowHeightAnimation();
    resizeTimer = null;
    resizeThrottleDeferTimer = null;
    resizeRaf = 0;
  }

  // Keep syncSize accessible internally for scheduleSettledResize;
  // it's not exposed publicly because syncSizeAndVerify is the intended API.
  void syncSize;

  return {
    syncSizeAndVerify,
    animateWindowHeight,
    followContentDuringTransition,
    resizeToContent,
    handleBreakdownAccordionToggle,
    refreshWindowMetrics,
    destroy,
    getMaxWindowH: () => maxWindowH,
    getScrollThresholdH: () => scrollThresholdH,
    getIsScrollLocked: () => isScrollLocked,
  };
}
