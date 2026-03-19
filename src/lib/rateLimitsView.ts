import { formatResetsIn, formatRetryIn } from "./utils/format.js";
import type { ProviderRateLimits, RateLimitWindow } from "./types/index.js";

export type ProviderRateLimitViewState = "ready" | "error" | "empty" | "idle";
const RESETTING_GRACE_MS = 60_000;
const CODEX_FIVE_HOUR_FALLBACK_WINDOW: RateLimitWindow = {
  windowId: "primary",
  label: "Session (5hr)",
  utilization: 0,
  resetsAt: null,
};

function resetAtMs(resetsAt: string | null): number | null {
  if (!resetsAt) return null;
  const ms = new Date(resetsAt).getTime();
  return Number.isFinite(ms) ? ms : null;
}

function isExpiredCodexWindow(
  rateLimits: ProviderRateLimits | null | undefined,
  resetsAt: string | null,
  now: number,
): boolean {
  if (rateLimits?.provider !== "codex") return false;
  if (providerHasActiveCooldown(rateLimits, now)) return false;

  const resetMs = resetAtMs(resetsAt);
  if (resetMs === null) return false;

  return resetMs + RESETTING_GRACE_MS <= now;
}

function isMissingCodexMetadataError(error: string | null): boolean {
  if (!error) return true;

  const normalized = error.toLowerCase();
  return normalized.includes("no codex session files found")
    || normalized.includes("no rate limit data in codex session files");
}

function fallbackCodexWindow(
  rateLimits: ProviderRateLimits | null | undefined,
  now: number,
): RateLimitWindow | null {
  if (!rateLimits || rateLimits.provider !== "codex") return null;
  if (rateLimits.windows.length > 0) return null;
  if (providerHasActiveCooldown(rateLimits, now)) return null;
  if (!isMissingCodexMetadataError(rateLimits.error)) return null;

  return CODEX_FIVE_HOUR_FALLBACK_WINDOW;
}

export function currentRateLimitWindows(
  rateLimits: ProviderRateLimits | null | undefined,
  now = Date.now(),
): RateLimitWindow[] {
  if (!rateLimits) return [];
  const windows = rateLimits.windows.filter(
    (window) => !isExpiredCodexWindow(rateLimits, window.resetsAt, now),
  );

  if (windows.length > 0) return windows;

  const fallbackWindow = fallbackCodexWindow(rateLimits, now);
  return fallbackWindow ? [fallbackWindow] : [];
}

export function hasRateLimitWindows(
  rateLimits: ProviderRateLimits | null | undefined,
  now = Date.now(),
): boolean {
  return currentRateLimitWindows(rateLimits, now).length > 0;
}

export function providerRateLimitViewState(
  rateLimits: ProviderRateLimits | null | undefined,
  now = Date.now(),
): ProviderRateLimitViewState {
  if (hasRateLimitWindows(rateLimits, now)) return "ready";
  if (rateLimits?.error) return "error";
  if (rateLimits?.provider === "codex" && rateLimits.windows.length > 0) return "idle";
  return "empty";
}

export function providerHasActiveCooldown(
  rateLimits: ProviderRateLimits | null | undefined,
  now = Date.now(),
): boolean {
  if (!rateLimits?.cooldownUntil) return false;
  return new Date(rateLimits.cooldownUntil).getTime() > now;
}

export function rateLimitWindowResetLabel(
  rateLimits: ProviderRateLimits | null | undefined,
  resetsAt: string | null,
  now = Date.now(),
): string {
  if (!resetsAt) return "";

  const resetMs = resetAtMs(resetsAt);
  if (resetMs === null) return "";

  const shouldAwaitRefresh = resetMs <= now
    && (rateLimits?.stale || isExpiredCodexWindow(rateLimits, resetsAt, now));

  if (shouldAwaitRefresh) {
    if (providerHasActiveCooldown(rateLimits, now)) {
      return formatRetryIn(rateLimits.cooldownUntil, now);
    }
    return "Awaiting refresh";
  }

  return formatResetsIn(resetsAt);
}
