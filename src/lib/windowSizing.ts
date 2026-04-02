export const WINDOW_WIDTH = 340;
// Extra window padding created a visible native strip below the card,
// which read as a second bottom border in the popover.
export const WINDOW_HEIGHT_PADDING = 0;
export const MIN_WINDOW_HEIGHT = 100;
export const DEFAULT_MAX_WINDOW_HEIGHT = 2400;
export const WINDOW_MONITOR_MARGIN = 24;
/** When content height exceeds this ratio of screen work area, enable scrolling. */
export const SCROLL_THRESHOLD_RATIO = 0.75;
export const RESIZE_SETTLE_DELAY_MS = 100;
/** Ignore sub-pixel / 1px oscillation between ResizeObserver and setSize (feedback loop). */
export const RESIZE_HYSTERESIS_PX = 3;

export type ResizeDisposition = "grow" | "shrink" | "skip";

export function measureTargetWindowHeight(contentHeight: number): number {
  return Math.ceil(contentHeight) + WINDOW_HEIGHT_PADDING;
}

export function clampWindowHeight(
  targetHeight: number,
  maxWindowHeight = DEFAULT_MAX_WINDOW_HEIGHT,
  minimumHeight = MIN_WINDOW_HEIGHT,
): number {
  const boundedMax = Math.max(maxWindowHeight, minimumHeight);
  return Math.max(minimumHeight, Math.min(targetHeight, boundedMax));
}

export function resolveEffectiveWindowMaxHeight(
  maxWindowHeight: number,
  scrollThresholdHeight: number,
  minimumHeight = MIN_WINDOW_HEIGHT,
): number {
  const finiteMax = Number.isFinite(maxWindowHeight) ? maxWindowHeight : DEFAULT_MAX_WINDOW_HEIGHT;
  const finiteScrollThreshold =
    Number.isFinite(scrollThresholdHeight) && scrollThresholdHeight > 0
      ? scrollThresholdHeight
      : finiteMax;

  return clampWindowHeight(
    Math.min(finiteMax, finiteScrollThreshold),
    finiteMax,
    minimumHeight,
  );
}

export function isWindowScrollLocked(
  contentHeight: number,
  effectiveMaxWindowHeight: number,
): boolean {
  if (!Number.isFinite(contentHeight) || contentHeight < MIN_WINDOW_HEIGHT) {
    return false;
  }

  return Number.isFinite(effectiveMaxWindowHeight) && contentHeight > effectiveMaxWindowHeight;
}

export function resolveMonitorMaxWindowHeight(
  workAreaPhysicalHeight: number,
  scaleFactor: number,
  fallbackMaxWindowHeight = DEFAULT_MAX_WINDOW_HEIGHT,
  monitorMargin = WINDOW_MONITOR_MARGIN,
): number {
  if (!Number.isFinite(workAreaPhysicalHeight) || workAreaPhysicalHeight <= 0) {
    return fallbackMaxWindowHeight;
  }

  if (!Number.isFinite(scaleFactor) || scaleFactor <= 0) {
    return fallbackMaxWindowHeight;
  }

  const logicalWorkAreaHeight = Math.floor(workAreaPhysicalHeight / scaleFactor) - monitorMargin;
  return clampWindowHeight(logicalWorkAreaHeight, fallbackMaxWindowHeight);
}

/** Returns the logical pixel height at which the window should enable scrolling. */
export function resolveScrollThresholdHeight(
  workAreaPhysicalHeight: number,
  scaleFactor: number,
  ratio = SCROLL_THRESHOLD_RATIO,
): number {
  if (!Number.isFinite(workAreaPhysicalHeight) || workAreaPhysicalHeight <= 0) {
    return DEFAULT_MAX_WINDOW_HEIGHT;
  }
  if (!Number.isFinite(scaleFactor) || scaleFactor <= 0) {
    return DEFAULT_MAX_WINDOW_HEIGHT;
  }
  return Math.floor((workAreaPhysicalHeight / scaleFactor) * ratio);
}

export function classifyResize(
  targetHeight: number,
  lastWindowHeight: number,
  minimumHeight = MIN_WINDOW_HEIGHT,
  hysteresisPx = RESIZE_HYSTERESIS_PX,
): ResizeDisposition {
  if (!Number.isFinite(targetHeight) || targetHeight < minimumHeight) {
    return "skip";
  }

  const last = Number.isFinite(lastWindowHeight) ? lastWindowHeight : 0;
  const delta = targetHeight - last;
  if (last > 0 && Math.abs(delta) <= hysteresisPx) {
    return "skip";
  }
  if (delta === 0) {
    return "skip";
  }

  return delta > 0 ? "grow" : "shrink";
}
