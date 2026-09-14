export interface RefreshVisibilityInput {
  isVisible: boolean;
  shownAt: number;
  now: number;
  minVisibleMs: number;
}

export function remainingRefreshVisibilityMs(input: RefreshVisibilityInput): number {
  if (!input.isVisible) return 0;
  const elapsed = Math.max(0, input.now - input.shownAt);
  return Math.max(0, input.minVisibleMs - elapsed);
}

export function shouldSkipResizeByJitter(
  previousHeight: number,
  nextHeight: number,
  thresholdPx: number,
): boolean {
  return Math.abs(nextHeight - previousHeight) <= thresholdPx;
}

// Enable with localStorage.setItem("resize-debug", "1")
let debugEnabled: boolean | null = null;

export function initResizeDebug(): void {
  debugEnabled = typeof localStorage !== "undefined" && localStorage.getItem("resize-debug") === "1";
}

export function isResizeDebugEnabled(): boolean {
  if (debugEnabled === null) initResizeDebug();
  return debugEnabled === true;
}

export function logResizeDebug(type: string, details: Record<string, unknown>): void {
  if (!isResizeDebugEnabled()) return;
  console.debug(`[resize-debug] ${type}`, details);
}

export function formatDebugError(error: unknown): { message: string } {
  if (error instanceof Error) return { message: error.message };
  return { message: String(error) };
}
