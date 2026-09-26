// Plan budget in dollars per rate-limit window. Preferred: the backend's
// per-model budgets (cheapest to dearest model is the range). Fallback until
// it has enough readings: spend in the window ÷ how much of it that used.
// Only local logs are priced, so usage elsewhere makes the fallback read low.
import type { PlanBudget, ProviderRateLimits, RateLimitWindow } from "../types/index.js";

const WEEK = 10_080;
const MONTH = 43_200; // Cursor billing cycle; start is found by calendar month.

export interface WindowSpan {
  minutes: number;
  /** Short unit for "$X / unit": "5h", "wk", "mo", … */
  unit: string;
}

function unitFor(minutes: number): string {
  if (minutes === WEEK) return "wk";
  if (minutes % WEEK === 0) return `${minutes / WEEK}wk`;
  if (minutes === 1_440) return "day";
  if (minutes % 1_440 === 0) return `${minutes / 1_440}d`;
  if (minutes === 60) return "hr";
  if (minutes % 60 === 0) return `${minutes / 60}h`;
  return `${minutes}m`;
}

/** Real length of a window, or null when it's unknown (no pace, no budget). */
export function windowSpan(provider: string, w: RateLimitWindow): WindowSpan | null {
  if (provider === "cursor") return { minutes: MONTH, unit: "mo" };
  let minutes = w.windowMinutes ?? null;
  // Claude never reports lengths; Codex caches from before windowMinutes existed.
  if (minutes === null && (w.windowId === "five_hour" || w.windowId === "primary")) minutes = 300;
  if (minutes === null && (w.windowId.startsWith("seven_day") || w.windowId === "secondary")) minutes = WEEK;
  return minutes ? { minutes, unit: unitFor(minutes) } : null;
}

export function windowStart(resetsAt: string, span: WindowSpan): Date {
  const start = new Date(resetsAt);
  if (span.minutes !== MONTH) {
    start.setTime(start.getTime() - span.minutes * 60_000);
    return start;
  }
  // Mar 31 → Feb 28, not Mar 3.
  const day = start.getDate();
  start.setDate(1);
  start.setMonth(start.getMonth() - 1);
  start.setDate(Math.min(day, new Date(start.getFullYear(), start.getMonth() + 1, 0).getDate()));
  return start;
}

/** Windows whose meter covers all of the provider's usage, so total spend fits. */
export function needsSpend(provider: string, windowId: string): boolean {
  if (provider === "claude") return windowId === "five_hour" || windowId === "seven_day";
  if (provider === "codex") return windowId === "primary" || windowId === "secondary";
  if (provider === "cursor") return windowId === "first_party" || windowId === "auto_composer";
  return provider === "kimi";
}

export interface BudgetAsk {
  windowId: string;
  since: string;
}

/** The bars' budget asks and the reading they came with, as one string: the
 * page re-asks when it changes (a new reading or window), not each time the
 * same reading is set again. */
export function budgetAsks(limits: ProviderRateLimits): string {
  const asks: BudgetAsk[] = [];
  for (const w of limits.windows) {
    const span = windowSpan(limits.provider, w);
    if (!span || !w.resetsAt || !needsSpend(limits.provider, w.windowId)) continue;
    asks.push({ windowId: w.windowId, since: windowStart(w.resetsAt, span).toISOString() });
  }
  return JSON.stringify({ provider: limits.provider, fetchedAt: limits.fetchedAt, asks });
}

// ponytail: fixed ±20% for model mix (limits aren't priced like the API);
// derive it from history if the statusline events ever get archived.
const MODEL_MIX = 0.2;
const MIN_UTILIZATION = 5;

export function budgetRange(
  provider: string,
  w: RateLimitWindow,
  budget: PlanBudget | undefined,
  windows: RateLimitWindow[],
): [number, number] | null {
  if (w.budgetUsd) return [w.budgetUsd, w.budgetUsd];
  if (!budget || !needsSpend(provider, w.windowId)) return null;
  if (budget.models.length > 0) {
    return [
      Math.min(...budget.models.map((m) => m.lowUsd)),
      Math.max(...budget.models.map((m) => m.highUsd)),
    ];
  }
  let own = budget.spend;
  // Cursor: the rest of the spend was billed to pools with a stated size.
  if (provider === "cursor") {
    for (const o of windows) if (o.budgetUsd) own -= (o.budgetUsd * o.utilization) / 100;
  }
  const u = w.utilization;
  if (!(own > 0) || !(u >= MIN_UTILIZATION)) return null;
  // Vendors report whole percents, so the true value is within ±0.5.
  return [
    (own / Math.min(100, u + 0.5)) * 100 * (1 - MODEL_MIX),
    (own / (u - 0.5)) * 100 * (1 + MODEL_MIX),
  ];
}
