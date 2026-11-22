import type { ResetWatch } from "../types/index.js";

const DAY_MS = 86_400_000;
const WEEKEND_FACTOR = 0.75;

export interface DailyPlan {
  /** What today should use in total. */
  targetPercent: number;
  /** Used since the plan started (day start, or the window start if later). */
  usedTodayPercent: number;
  /** targetPercent - usedTodayPercent; negative when over. */
  leftPercent: number;
  /** The daily cap, not the even spread, set the target. */
  capped: boolean;
}

export interface DailyPlanInput {
  /** Meter reading when the plan started. */
  startUtilization: number;
  utilization: number;
  resetsAt: Date;
  /** max(local day start, window start). */
  anchor: Date;
  /** Daily cap K, percent of the window. */
  capPercent: number;
  resetForecast?: ResetWatch | null;
  now?: Date;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export function activeResetForecast(forecast: ResetWatch | null | undefined, now: Date): ResetWatch | null {
  if (!forecast) return null;
  const observed = Date.parse(forecast.observedAt);
  const expires = Date.parse(forecast.expiresAt);
  const current = now.getTime();
  const first = forecast.resetChance24hPercent;
  const second = forecast.resetChance48hPercent;
  return Number.isFinite(first) && Number.isFinite(second)
    && first >= 0 && first <= second && second <= 100
    && observed <= current + 5 * 60_000 && current < observed + 30 * 60_000
    && expires > current && expires > observed
    ? forecast : null;
}

export function startOfLocalDay(now: Date): Date {
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  return d;
}

export function planToday(input: DailyPlanInput): DailyPlan | null {
  const anchorMs = input.anchor.getTime();
  const daysToReset = (input.resetsAt.getTime() - anchorMs) / DAY_MS;
  if (!(daysToReset > 0)) return null;
  const remaining = clamp(100 - input.startUtilization, 0, 100);
  const dayEnd = startOfLocalDay(input.anchor);
  dayEnd.setDate(dayEnd.getDate() + 1);
  const dayLeft = clamp((dayEnd.getTime() - anchorMs) / DAY_MS, 0, 1);

  const forecast = activeResetForecast(input.resetForecast, input.now ?? new Date());
  let planningDays = daysToReset;
  let earlyResetChance = 0;
  if (forecast) {
    const observedDays = (Date.parse(forecast.observedAt) - anchorMs) / DAY_MS;
    const firstDays = Math.min(daysToReset, Math.max(0, observedDays) + 1);
    const secondDays = Math.min(daysToReset, Math.max(0, observedDays) + 2);
    const firstChance = forecast.resetChance24hPercent / 100;
    const secondChance = (forecast.resetChance48hPercent - forecast.resetChance24hPercent) / 100;
    planningDays = firstChance * firstDays + secondChance * secondDays
      + (1 - forecast.resetChance48hPercent / 100) * daysToReset;
    earlyResetChance = (firstDays < daysToReset ? firstChance : 0)
      + (secondDays < daysToReset ? secondChance : 0);
  }
  const planned = remaining * Math.min(1, dayLeft / planningDays);
  const weekend = [0, 6].includes(input.anchor.getDay());
  const cap = Math.max(0, input.capPercent) * (1 + 1.5 * earlyResetChance) * (weekend ? WEEKEND_FACTOR : 1);
  const target = Math.min(remaining, planned, cap);
  const used = Math.max(0, input.utilization - input.startUtilization);
  return {
    targetPercent: target,
    usedTodayPercent: used,
    leftPercent: target - used,
    capped: cap < Math.min(remaining, planned),
  };
}
