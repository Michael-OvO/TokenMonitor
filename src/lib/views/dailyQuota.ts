// Today's share of a weekly limit, after codex-cockpit's remaining-quota plan:
// spread what was left when the day started evenly over the time to the
// official reset (A), never more than a daily cap K (C). Fixed for the day,
// so using more later does not raise the target. All values are percent of
// the whole window.

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
  /** Chance of an early reset (0-100); raises the cap by 1 + 1.5p. */
  resetRiskPercent?: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

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

  const even = remaining * Math.min(1, dayLeft / daysToReset);
  const p = clamp(input.resetRiskPercent ?? 0, 0, 100) / 100;
  const weekend = [0, 6].includes(input.anchor.getDay());
  const cap = Math.max(0, input.capPercent) * (1 + 1.5 * p) * (weekend ? WEEKEND_FACTOR : 1);
  const target = Math.min(remaining, even, cap);
  const used = Math.max(0, input.utilization - input.startUtilization);
  return {
    targetPercent: target,
    usedTodayPercent: used,
    leftPercent: target - used,
    capped: cap < Math.min(remaining, even),
  };
}
