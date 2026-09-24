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
  /** Position along the strip, 0..100 (the mean for a cluster). */
  leftPct: number;
  /** "Oct 4", or a range for a cluster: "Oct 4–5", "Sep 30–Oct 2". */
  dateLabel: string;
  /** Compact countdown: "11d", "18h"; a range for a cluster: "10–11d". */
  leftLabel: string;
  /** Days to the earliest expiry in the marker. */
  daysLeft: number;
  /** Any member expires within three days. */
  urgent: boolean;
  /** Native tooltip text, one line per reset. */
  title: string;
  /** A label sitting under the "today" or horizon cap drops to the second baseline. */
  labelRow: 0 | 1;
  /** Resets merged into this marker; more than one when their labels would collide. */
  count: number;
}

export interface ResetTimelineLayout {
  available: number;
  /** Countdown to the soonest expiry, for the row header; null with no live resets. */
  nextLeftLabel: string | null;
  horizonDays: number;
  horizonLabel: string;
  /** Week boundaries along the strip, 0..100, excluding the ends. */
  weekTickPcts: number[];
  markers: ResetTimelineMarker[];
}

const DAY_MS = 86_400_000;
const HOUR_MS = 3_600_000;
const MINUTE_MS = 60_000;
const MIN_HORIZON_DAYS = 28;
const URGENT_DAYS = 3;
/** Labels closer than this would overlap, so the resets behind them merge into one marker. */
const LABEL_MIN_GAP_PCT = 11;

/** One coarse unit, rounded up so a reset never reads as already gone: "11d", "18h", "40m". */
export function formatCompactTimeLeft(ms: number): string {
  if (ms >= DAY_MS) return `${Math.ceil(ms / DAY_MS)}d`;
  if (ms >= HOUR_MS) return `${Math.ceil(ms / HOUR_MS)}h`;
  return `${Math.max(1, Math.ceil(ms / MINUTE_MS))}m`;
}

const shortDate = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric" });
const dayOfMonth = new Intl.DateTimeFormat("en-US", { day: "numeric" });
const monthOf = new Intl.DateTimeFormat("en-US", { month: "short" });
const fullDateTime = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

/** "Oct 4" for one day, "Oct 4–5" within a month, "Sep 30–Oct 2" across months. */
function dateRangeLabel(firstMs: number, lastMs: number): string {
  const first = shortDate.format(firstMs);
  const last = shortDate.format(lastMs);
  if (first === last) return first;
  if (monthOf.format(firstMs) === monthOf.format(lastMs)) return `${first}–${dayOfMonth.format(lastMs)}`;
  return `${first}–${last}`;
}

/** "11d" for one, "10–11d" when both share a unit, "17h–2d" otherwise. */
function countdownRangeLabel(firstMs: number, lastMs: number): string {
  const first = formatCompactTimeLeft(firstMs);
  const last = formatCompactTimeLeft(lastMs);
  if (first === last) return first;
  const unit = (label: string) => label.slice(-1);
  if (unit(first) === unit(last)) return `${first.slice(0, -1)}–${last}`;
  return `${first}–${last}`;
}

/**
 * Lay the still-valid usage-limit resets on a strip from today to the last
 * expiry rounded up to whole weeks, never under four. Resets whose labels
 * would overlap merge into one marker with a range label; a label that would
 * sit under the "today" or horizon cap drops to the second baseline. Null
 * when the provider reports no resets at all.
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

  const pctOf = (expiresMs: number) => Math.min(100, ((expiresMs - now) / DAY_MS / horizonDays) * 100);

  // Group resets whose labels would collide with the first of the group.
  const clusters: Array<typeof live> = [];
  for (const entry of live) {
    const current = clusters[clusters.length - 1];
    if (current && pctOf(entry.expiresMs) - pctOf(current[0].expiresMs) < LABEL_MIN_GAP_PCT) {
      current.push(entry);
    } else {
      clusters.push([entry]);
    }
  }

  const markers = clusters.map((members) => {
    const firstMs = members[0].expiresMs;
    const lastMs = members[members.length - 1].expiresMs;
    const leftPct = members.reduce((sum, m) => sum + pctOf(m.expiresMs), 0) / members.length;
    const daysLeft = (firstMs - now) / DAY_MS;
    // The "today" cap sits at 0 and the horizon cap at 100.
    const labelRow: 0 | 1 =
      leftPct < LABEL_MIN_GAP_PCT || leftPct > 100 - LABEL_MIN_GAP_PCT ? 1 : 0;
    const title = members
      .map(({ reset, expiresMs }) => {
        const grantedMs = resetAtMs(reset.grantedAt);
        return [
          reset.title ?? "Reset",
          `expires ${fullDateTime.format(expiresMs)}`,
          grantedMs !== null ? `granted ${shortDate.format(grantedMs)}` : null,
        ]
          .filter(Boolean)
          .join(" · ");
      })
      .join("\n");
    return {
      leftPct,
      dateLabel: dateRangeLabel(firstMs, lastMs),
      leftLabel: countdownRangeLabel(firstMs - now, lastMs - now),
      daysLeft,
      urgent: daysLeft <= URGENT_DAYS,
      title,
      labelRow,
      count: members.length,
    };
  });

  return {
    available: resets.available,
    nextLeftLabel: live.length > 0 ? formatCompactTimeLeft(live[0].expiresMs - now) : null,
    horizonDays,
    horizonLabel: `+${horizonDays / 7}w`,
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
