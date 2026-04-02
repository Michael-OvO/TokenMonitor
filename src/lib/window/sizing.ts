export const WINDOW_WIDTH = 340;
// Extra window padding created a visible native strip below the card,
// which read as a second bottom border in the popover.
export const WINDOW_HEIGHT_PADDING = 0;
export const MIN_WINDOW_HEIGHT = 100;
export const DEFAULT_MAX_WINDOW_HEIGHT = 2400;
export const WINDOW_MONITOR_MARGIN = 24;
export const RESIZE_SETTLE_DELAY_MS = 16;

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

/** Pixel tolerance below which a resize is not worth the visual cost. */
const RESIZE_TOLERANCE_PX = 2;

export function classifyResize(
  targetHeight: number,
  lastWindowHeight: number,
  minimumHeight = MIN_WINDOW_HEIGHT,
): ResizeDisposition {
  if (!Number.isFinite(targetHeight) || targetHeight < minimumHeight) {
    return "skip";
  }

  if (Math.abs(targetHeight - lastWindowHeight) <= RESIZE_TOLERANCE_PX) {
    return "skip";
  }

  return targetHeight > lastWindowHeight ? "grow" : "shrink";
}
