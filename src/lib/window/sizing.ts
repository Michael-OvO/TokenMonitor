export const WINDOW_WIDTH = 340;
// Extra window padding created a visible native strip below the card,
// which read as a second bottom border in the popover.
export const WINDOW_HEIGHT_PADDING = 0;
export const MIN_WINDOW_HEIGHT = 100;
export const DEFAULT_MAX_WINDOW_HEIGHT = 2400;
export const WINDOW_MONITOR_MARGIN = 24;
/** When content height exceeds this ratio of screen work area, enable scrolling. */
export const SCROLL_THRESHOLD_RATIO = 0.75;
/* 560 rather than 500: the 5h view with two providers, their windows, and the
   Codex reset timeline runs past 500 and the tail hid under the sticky footer. */
export const SCROLL_THRESHOLD_CAP = 560;
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
  return Math.min(Math.floor((workAreaPhysicalHeight / scaleFactor) * ratio), SCROLL_THRESHOLD_CAP);
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


// ── Fixed window height ──

export const FIXED_HEIGHT_CAP = 560;
export const FIXED_HEIGHT_SCREEN_RATIO = 0.392;

export function resolveFixedWindowHeight(
  monitorPhysicalWidth: number,
  scaleFactor: number,
  cap = FIXED_HEIGHT_CAP,
  ratio = FIXED_HEIGHT_SCREEN_RATIO,
): number {
  if (!Number.isFinite(monitorPhysicalWidth) || monitorPhysicalWidth <= 0) {
    return cap;
  }
  if (!Number.isFinite(scaleFactor) || scaleFactor <= 0) {
    return cap;
  }
  const logicalWidth = monitorPhysicalWidth / scaleFactor;
  return Math.min(cap, Math.floor(logicalWidth * ratio));
}

// ── Display scale (popover zoom by monitor size) ──

/** Logical screen size that renders at 100%: a 14" MacBook Pro at default scaling. */
export const DISPLAY_SCALE_BASE_WIDTH = 1512;
export const DISPLAY_SCALE_BASE_HEIGHT = 982;
export const DISPLAY_SCALE_MAX = 1.5;
/** Auto scale is rounded to 1/20 (5%) steps so tiny monitor differences don't change it. */
const DISPLAY_SCALE_STEPS_PER_UNIT = 20;

export const SUPPORTED_DISPLAY_SCALES = ["auto", "1", "1.15", "1.3", "1.5"] as const;
export type DisplayScaleSetting = (typeof SUPPORTED_DISPLAY_SCALES)[number];
export const DEFAULT_DISPLAY_SCALE: DisplayScaleSetting = "auto";

export function resolveAutoDisplayScale(
  physicalWidth: number,
  physicalHeight: number,
  scaleFactor: number,
): number {
  if (
    !Number.isFinite(physicalWidth) || physicalWidth <= 0 ||
    !Number.isFinite(physicalHeight) || physicalHeight <= 0 ||
    !Number.isFinite(scaleFactor) || scaleFactor <= 0
  ) {
    return 1;
  }
  const ratio = Math.min(
    physicalWidth / scaleFactor / DISPLAY_SCALE_BASE_WIDTH,
    physicalHeight / scaleFactor / DISPLAY_SCALE_BASE_HEIGHT,
  );
  const stepped = Math.round(ratio * DISPLAY_SCALE_STEPS_PER_UNIT) / DISPLAY_SCALE_STEPS_PER_UNIT;
  return Math.min(DISPLAY_SCALE_MAX, Math.max(1, stepped));
}

export function resolveDisplayScale(
  setting: DisplayScaleSetting,
  monitor: { size: { width: number; height: number }; scaleFactor: number } | null,
): number {
  if (setting !== "auto") return Number(setting);
  if (!monitor) return 1;
  return resolveAutoDisplayScale(monitor.size.width, monitor.size.height, monitor.scaleFactor);
}
