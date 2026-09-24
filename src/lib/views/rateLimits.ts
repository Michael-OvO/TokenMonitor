import {
  getRateLimitExpiredWindowGraceMs,
  getRateLimitFallbackWindow,
  isRateLimitMissingMetadataError,
  isRateLimitProvider,
} from "../providerMetadata.js";
import { formatResetsIn, formatRetryIn } from "../utils/format.js";
import type {
  ProviderRateLimits,
  RateLimitWindow,
  UsageLimitReset,
  UsageLimitResets,
} from "../types/index.js";

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

export interface ResetTimelineMarker {
  /** True position along the strip, 0..100. */
  leftPct: number;
  /** Where the dot is drawn: nudged right of `leftPct` when a neighbour is too close. */
  dotPct: number;
  /** "Oct 5". */
  dateLabel: string;
  /** Compact countdown: "11d", "18h", "40m". */
  leftLabel: string;
  daysLeft: number;
  /** Expires within three days. */
  urgent: boolean;
  /** Native tooltip text. */
  title: string;
}

export interface ResetTimelineLayout {
  available: number;
  /** Countdown to the soonest expiry, for the row header; null with no live resets. */
  nextLeftLabel: string | null;
  horizonDays: number;
  /** Week boundaries along the strip, 0..100, excluding the ends. */
  weekTickPcts: number[];
  /** One per live reset, soonest first; the chips below the strip follow the same order. */
  markers: ResetTimelineMarker[];
}

const DAY_MS = 86_400_000;
const HOUR_MS = 3_600_000;
const MINUTE_MS = 60_000;
const MIN_HORIZON_DAYS = 28;
const URGENT_DAYS = 3;
/** Dots closer than this (percent of the strip) would touch, so the later one is nudged right. */
const MIN_DOT_GAP_PCT = 4;

/** One coarse unit, rounded up so a reset never reads as already gone: "11d", "18h", "40m". */
export function formatCompactTimeLeft(ms: number): string {
  if (ms >= DAY_MS) return `${Math.ceil(ms / DAY_MS)}d`;
  if (ms >= HOUR_MS) return `${Math.ceil(ms / HOUR_MS)}h`;
  return `${Math.max(1, Math.ceil(ms / MINUTE_MS))}m`;
}

const shortDate = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric" });
const fullDateTime = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

/**
 * Lay the still-valid usage-limit resets on a strip from today to the last
 * expiry rounded up to whole weeks, never under four. Every reset keeps its
 * own dot (nudged apart when two would touch) and its own chip with date
 * and countdown, in the same order. Null when the provider reports no
 * resets at all.
 */
export function resetTimelineLayout(
  resets: UsageLimitResets | null | undefined,
  now = Date.now(),
): ResetTimelineLayout | null {
  if (!resets) return null;
  const live = resets.resets
    .map((reset) => ({ reset, expiresMs: resetAtMs(reset.expiresAt) }))
    .filter(
      (entry): entry is { reset: UsageLimitReset; expiresMs: number } =>
        entry.expiresMs !== null && entry.expiresMs > now,
    )
    .sort((a, b) => a.expiresMs - b.expiresMs);

  const lastDays = live.length > 0 ? (live[live.length - 1].expiresMs - now) / DAY_MS : 0;
  const horizonDays = Math.max(MIN_HORIZON_DAYS, Math.ceil(lastDays / 7) * 7);
  const weekTickPcts: number[] = [];
  for (let day = 7; day < horizonDays; day += 7) weekTickPcts.push((day / horizonDays) * 100);

  let previousDotPct = -Infinity;
  const markers = live.map(({ reset, expiresMs }) => {
    const daysLeft = (expiresMs - now) / DAY_MS;
    const leftPct = Math.min(100, (daysLeft / horizonDays) * 100);
    const dotPct = Math.min(100, Math.max(leftPct, previousDotPct + MIN_DOT_GAP_PCT));
    previousDotPct = dotPct;
    const grantedMs = resetAtMs(reset.grantedAt);
    const title = [
      reset.title ?? "Reset",
      `expires ${fullDateTime.format(expiresMs)}`,
      grantedMs !== null ? `granted ${shortDate.format(grantedMs)}` : null,
    ]
      .filter(Boolean)
      .join(" · ");
    return {
      leftPct,
      dotPct,
      dateLabel: shortDate.format(expiresMs),
      leftLabel: formatCompactTimeLeft(expiresMs - now),
      daysLeft,
      urgent: daysLeft <= URGENT_DAYS,
      title,
    };
  });

  return {
    available: resets.available,
    nextLeftLabel: markers.length > 0 ? markers[0].leftLabel : null,
    horizonDays,
    weekTickPcts,
    markers,
  };
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
