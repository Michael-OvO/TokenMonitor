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
  /** Reported whenever a setSize request succeeds; lets callers persist the last applied height. */
  onHeightApplied?: (height: number) => void;
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
  setChartHoverActive: (active: boolean) => void;
  /** Release the "no shrink before first data" gate; shrink requests were buffered until now. */
  markInitialContentReady: () => void;
  destroy: () => void;
  getMaxWindowH: () => number;
  getScrollThresholdH: () => number;
  getIsScrollLocked: () => boolean;
}

export function createResizeOrchestrator(
  deps: ResizeOrchestratorDeps,
): ResizeOrchestrator {
  type WindowHeightRequest = {
    height: number;
    source: string;
  };

  // ── Internal state (local to this closure) ──
  let resizeRaf = 0;
  let resizeTimer: ReturnType<typeof setTimeout> | null = null;
  let lastWindowH = typeof window === "undefined" ? 0 : window.innerHeight;
  let windowAnimationRaf = 0;
  let windowAnimationToken = 0;
  let isWindowHeightAnimating = false;
  let observerResizeRaf = 0;
  let pendingObserverSource = "observer";
  let chartHoverActive = false;

  // Resize throttle: max 3 operations per 500ms window
  const ANIMATED_RESIZE_FRAME_INTERVAL_MS = 32;
  let pendingWindowHeightRequest: WindowHeightRequest | null = null;
  let isWindowHeightRequestInFlight = false;

  let maxWindowH = DEFAULT_MAX_WINDOW_HEIGHT;
  let scrollThresholdH = DEFAULT_MAX_WINDOW_HEIGHT;
  let isScrollLocked = false;
  // Until the first data payload lands, the DOM is smaller than it will be
  // once the bar chart renders. Suppress shrink requests during that window
  // to avoid a visible pop-out after content arrives.
  let initialContentReady = false;

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
      hadObserverRaf: observerResizeRaf !== 0,
      ...captureDebugSnapshot("clear-pending"),
    });
    if (resizeTimer) {
      clearTimeout(resizeTimer);
      resizeTimer = null;
    }
    cancelAnimationFrame(resizeRaf);
    resizeRaf = 0;
    cancelAnimationFrame(observerResizeRaf);
    observerResizeRaf = 0;
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

  function flushWindowHeightRequest(): void {
    if (isWindowHeightRequestInFlight || !pendingWindowHeightRequest) return;

    const request = pendingWindowHeightRequest;
    pendingWindowHeightRequest = null;
    isWindowHeightRequestInFlight = true;

    deps
      .invoke("set_window_size_and_align", {
        width: WINDOW_WIDTH,
        height: request.height,
      })
      .then(() => {
        deps.logDebug("resize:set-size-resolved", {
          source: request.source,
          nextHeight: request.height,
          ...captureDebugSnapshot(`set-size-resolved-${request.source}`),
        });
        deps.onHeightApplied?.(request.height);
      })
      .catch((error) => {
        deps.logDebug("resize:set-size-rejected", {
          source: request.source,
          nextHeight: request.height,
          error: deps.formatDebugError(error),
          ...captureDebugSnapshot(`set-size-rejected-${request.source}`),
        });
        if (!pendingWindowHeightRequest && typeof window !== "undefined") {
          lastWindowH = window.innerHeight;
        }
      })
      .finally(() => {
        isWindowHeightRequestInFlight = false;
        flushWindowHeightRequest();
      });
  }

  function applyMeasuredHeight(
    measurement: {
      rawMeasuredHeight: number;
      nextHeight: number;
      scrollLocked: boolean;
      effectiveMaxWindowH: number;
    },
    source: string,
  ): void {
    if (measurement.scrollLocked) {
      deps.logDebug("resize:apply-scroll-locked", {
        source,
        rawMeasuredHeight: measurement.rawMeasuredHeight,
        nextHeight: measurement.nextHeight,
        effectiveMaxWindowH: measurement.effectiveMaxWindowH,
      });
    }
    applyWindowHeight(measurement.nextHeight, source);
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
    if (!initialContentReady && disposition === "shrink") {
      deps.logDebug("resize:shrink-blocked-initial", {
        source,
        nextHeight,
        lastWindowH,
      });
      return;
    }
    if (chartHoverActive && nextHeight < lastWindowH) {
      deps.logDebug("resize:shrink-blocked-chart-hover", {
        source,
        nextHeight,
        lastWindowH,
      });
      return;
    }
    lastWindowH = nextHeight;
    pendingWindowHeightRequest = {
      height: nextHeight,
      source,
    };
    flushWindowHeightRequest();
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

  function shouldApplyAnimatedResizeFrame(
    now: number,
    lastAppliedAt: number,
    isFinalFrame: boolean,
  ): boolean {
    if (isFinalFrame) return true;
    return now - lastAppliedAt >= ANIMATED_RESIZE_FRAME_INTERVAL_MS;
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
    applyMeasuredHeight(measurement, `${source}:sync`);
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
    let lastFrameAppliedAt = Number.NEGATIVE_INFINITY;
    isWindowHeightAnimating = true;
    lastWindowH = startHeight;

    const step = (now: number) => {
      if (animationToken !== windowAnimationToken) return;

      const progress = Math.min((now - startedAt) / durationMs, 1);
      const eased = easeWindowHeight(progress);
      const interpolatedHeight = Math.round(
        startHeight + (nextHeight - startHeight) * eased,
      );
      if (
        shouldApplyAnimatedResizeFrame(
          now,
          lastFrameAppliedAt,
          progress >= 1,
        )
      ) {
        lastFrameAppliedAt = now;
        applyWindowHeight(interpolatedHeight, `${source}:frame`);
      }

      if (progress >= 1) {
        windowAnimationRaf = 0;
        isWindowHeightAnimating = false;
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
    let lastFrameAppliedAt = Number.NEGATIVE_INFINITY;
    deps.logDebug("resize:follow-start", { source, durationMs });

    const step = (now: number) => {
      if (token !== windowAnimationToken) return;

      const elapsed = now - startedAt;
      const isFinalFrame = elapsed >= durationMs + 50;
      if (
        shouldApplyAnimatedResizeFrame(
          now,
          lastFrameAppliedAt,
          isFinalFrame,
        )
      ) {
        lastFrameAppliedAt = now;
        const measurement = measureWindowHeight(`${source}:follow`);
        if (measurement) {
          applyWindowHeight(measurement.nextHeight, `${source}:follow`);
        }
      }

      if (!isFinalFrame) {
        windowAnimationRaf = requestAnimationFrame(step);
      } else {
        windowAnimationRaf = 0;
        isWindowHeightAnimating = false;
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
    pendingObserverSource = source;
    if (observerResizeRaf !== 0) return;

    observerResizeRaf = requestAnimationFrame(() => {
      observerResizeRaf = 0;
      const scheduledSource = pendingObserverSource;
      const measurement = measureWindowHeight(
        `resize-to-content-${scheduledSource}`,
      );
      const rawMeasuredHeight = measurement?.rawMeasuredHeight ?? null;
      const nextHeight = measurement?.nextHeight ?? null;
      const deltaFromLast =
        nextHeight == null ? null : nextHeight - lastWindowH;
      deps.logDebug("resize:observer-measure", {
        source: scheduledSource,
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
        ...captureDebugSnapshot(`resize-to-content-${scheduledSource}`),
      });
      if (!measurement) return;
      applyMeasuredHeight(measurement, `${scheduledSource}:observer`);
    });
  }

  function handleBreakdownAccordionToggle(
    detail: AccordionToggleDetail,
  ): void {
    const direction = detail.expanding ? "expand" : "collapse";
    const source = `breakdown-${detail.scope}-${direction}`;
    const measurement = measureWindowHeight(`${source}:target`);
    if (!measurement) return;

    clearPendingResize();
    clearWindowHeightAnimation();
    applyWindowHeight(measurement.nextHeight, `${source}:target`);
  }

  function setChartHoverActive(active: boolean): void {
    chartHoverActive = active;
    deps.logDebug("resize:chart-hover-active", { active });
    if (active) {
      // Apply one deterministic resize when detail panel appears,
      // then block observer-driven feedback while hover stays active.
      syncSizeAndVerify("chart-hover-start");
    } else {
      resizeToContent("chart-hover-end");
    }
  }

  function destroy(): void {
    if (resizeTimer) clearTimeout(resizeTimer);
    cancelAnimationFrame(resizeRaf);
    cancelAnimationFrame(observerResizeRaf);
    clearWindowHeightAnimation();
    resizeTimer = null;
    resizeRaf = 0;
    observerResizeRaf = 0;
  }

  void syncSize;
  void scheduleSettledResize;

  function markInitialContentReady(): void {
    if (initialContentReady) return;
    initialContentReady = true;
    deps.logDebug("resize:initial-content-ready", {
      lastWindowH,
    });
    // Now that shrinks are unblocked, run one sync pass in case the final
    // content is shorter than the primed height we started with.
    syncSizeAndVerify("initial-content-ready");
  }

  return {
    syncSizeAndVerify,
    animateWindowHeight,
    followContentDuringTransition,
    resizeToContent,
    handleBreakdownAccordionToggle,
    refreshWindowMetrics,
    setChartHoverActive,
    markInitialContentReady,
    destroy,
    getMaxWindowH: () => maxWindowH,
    getScrollThresholdH: () => scrollThresholdH,
    getIsScrollLocked: () => isScrollLocked,
  };
}
