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

// ── Resize debug stubs ──────────────────────────────────────────────
// These replace the deleted resizeDebug.ts module. The resize debug
// overlay was removed during refactoring; these stubs keep callsites
// compiling and allow debug logging to be re-enabled via localStorage.

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

export function captureResizeDebugSnapshot(
  _reason: string,
  _el: HTMLElement | null,
  _meta: Record<string, unknown>,
): Record<string, unknown> {
  return {};
}
