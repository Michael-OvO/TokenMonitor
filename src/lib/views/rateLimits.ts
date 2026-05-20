import {
  getRateLimitExpiredWindowGraceMs,
  getRateLimitFallbackWindow,
  isRateLimitMissingMetadataError,
  isRateLimitProvider,
} from "../providerMetadata.js";
import { formatResetsIn, formatRetryIn } from "../utils/format.js";
import type { ProviderRateLimits, RateLimitWindow } from "../types/index.js";

export type ProviderRateLimitViewState = "ready" | "error" | "empty" | "idle";

function resetAtMs(resetsAt: string | null): number | null {
  if (!resetsAt) return null;
  const ms = new Date(resetsAt).getTime();
  return Number.isFinite(ms) ? ms : null;
}

function isExpiredProviderWindow(
  rateLimits: ProviderRateLimits | null | undefined,
  resetsAt: string | null,
  now: number,
): boolean {
  if (!rateLimits || !isRateLimitProvider(rateLimits.provider)) return false;
  if (providerHasActiveCooldown(rateLimits, now)) return false;

  const resetMs = resetAtMs(resetsAt);
  if (resetMs === null) return false;

  const graceMs = getRateLimitExpiredWindowGraceMs(rateLimits.provider);
  return graceMs > 0 && resetMs + graceMs <= now;
}

function fallbackProviderWindow(
  rateLimits: ProviderRateLimits | null | undefined,
  now: number,
): RateLimitWindow | null {
  if (!rateLimits || !isRateLimitProvider(rateLimits.provider)) return null;
  const fallbackWindow = getRateLimitFallbackWindow(rateLimits.provider);
  if (!fallbackWindow) return null;
  if (providerHasActiveCooldown(rateLimits, now)) return null;
  if (!isRateLimitMissingMetadataError(rateLimits.provider, rateLimits.error)) return null;

  return fallbackWindow;
}

export function currentRateLimitWindows(
  rateLimits: ProviderRateLimits | null | undefined,
  now = Date.now(),
): RateLimitWindow[] {
  if (!rateLimits) return [];
  const windows = rateLimits.windows.filter(
    (window) => !isExpiredProviderWindow(rateLimits, window.resetsAt, now),
  );

  const fallbackWindow = fallbackProviderWindow(rateLimits, now);
  if (!fallbackWindow) return windows;

  // Ensure the primary fallback window is always present — even when other
  // windows (e.g. weekly) survive, the 5h window should show as 0% rather
  // than disappearing when its reset time has passed.
  if (!windows.some((w) => w.windowId === fallbackWindow.windowId)) {
    return [fallbackWindow, ...windows];
  }

  return windows;
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
  if (
    rateLimits
    && isRateLimitProvider(rateLimits.provider)
    && getRateLimitExpiredWindowGraceMs(rateLimits.provider) > 0
    && rateLimits.windows.length > 0
  ) {
    return "idle";
  }
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
    && (rateLimits?.stale || isExpiredProviderWindow(rateLimits, resetsAt, now));

  if (shouldAwaitRefresh) {
    if (providerHasActiveCooldown(rateLimits, now)) {
      return formatRetryIn(rateLimits!.cooldownUntil, now);
    }
    return "Awaiting refresh";
  }

  if (rateLimits?.stale && resetMs > now) {
    return `${formatResetsIn(resetsAt)} (stale)`;
  }

  return formatResetsIn(resetsAt);
}
