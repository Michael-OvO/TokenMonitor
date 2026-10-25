import { describe, expect, it } from "vitest";
import { planToday } from "./dailyQuota.js";
import type { ResetWatch } from "../types/index.js";

// Wednesday, local time.
const midnight = new Date(2026, 9, 7, 0, 0, 0);
const inDays = (d: number) => new Date(midnight.getTime() + d * 86_400_000);

const forecast = (first = 25, second = 50, observed = midnight): ResetWatch => ({
  resetChance24hPercent: first,
  resetChance48hPercent: second,
  confidence: "low",
  observedAt: observed.toISOString(),
  expiresAt: new Date(observed.getTime() + 30 * 60_000).toISOString(),
});

describe("planToday", () => {
  it("spreads the remaining quota evenly over the days to reset", () => {
    const plan = planToday({ startUtilization: 20, utilization: 30, resetsAt: inDays(4), anchor: midnight, capPercent: 100 })!;
    expect(plan.targetPercent).toBeCloseTo(20); // 80% over 4 days
    expect(plan.usedTodayPercent).toBeCloseTo(10);
    expect(plan.leftPercent).toBeCloseTo(10);
    expect(plan.capped).toBe(false);
  });

  it("caps a day, harder on weekends, and loosens with reset risk", () => {
    const plan = planToday({ startUtilization: 0, utilization: 40, resetsAt: inDays(2), anchor: midnight, capPercent: 25 })!;
    expect(plan.targetPercent).toBe(25);
    expect(plan.capped).toBe(true);
    const saturday = new Date(2026, 9, 10);
    const sat = planToday({ startUtilization: 0, utilization: 0, resetsAt: new Date(2026, 9, 11, 12), anchor: saturday, capPercent: 25 })!;
    expect(sat.targetPercent).toBeCloseTo(18.75);
    const risky = planToday({ startUtilization: 0, utilization: 0, resetsAt: inDays(2), anchor: midnight, capPercent: 25, resetForecast: forecast(40, 40), now: midnight })!;
    expect(risky.targetPercent).toBeCloseTo(40);
  });

  it("uses everything left when the reset comes before the day ends", () => {
    const plan = planToday({ startUtilization: 90, utilization: 90, resetsAt: inDays(0.5), anchor: midnight, capPercent: 25 })!;
    expect(plan.targetPercent).toBeCloseTo(10);
  });

  it("counts only the rest of the day when the plan starts late", () => {
    const noon = inDays(0.5);
    const plan = planToday({ startUtilization: 0, utilization: 0, resetsAt: inDays(5.5), anchor: noon, capPercent: 100 })!;
    expect(plan.targetPercent).toBeCloseTo(10); // half a day of 100%/5 days
  });

  it("has no plan once the window has reset", () => {
    expect(planToday({ startUtilization: 0, utilization: 0, resetsAt: midnight, anchor: midnight, capPercent: 25 })).toBeNull();
  });

  it("weights the 24h and 48h scenarios without double counting cumulative probability", () => {
    const input = { startUtilization: 20, utilization: 30, resetsAt: inDays(4), anchor: midnight, capPercent: 100, now: midnight };
    const normal = planToday(input)!;
    const adjusted = planToday({ ...input, resetForecast: forecast() })!;
    expect(normal.targetPercent).toBe(20);
    expect(adjusted.targetPercent).toBeCloseTo(80 / 2.75);
    expect(adjusted.leftPercent).toBeCloseTo(80 / 2.75 - 10);
    expect(planToday({ ...input, utilization: 40, resetForecast: forecast() })!.targetPercent).toBe(adjusted.targetPercent);
  });

  it("falls back exactly for absent, expired, future or invalid forecasts", () => {
    const input = { startUtilization: 20, utilization: 30, resetsAt: inDays(4), anchor: midnight, capPercent: 25, now: midnight };
    const normal = planToday(input);
    for (const resetForecast of [
      null,
      forecast(0, 0),
      forecast(25, 50, new Date(midnight.getTime() - 30 * 60_000)),
      forecast(25, 50, new Date(midnight.getTime() + 6 * 60_000)),
      forecast(60, 50),
      forecast(-1, 50),
      forecast(25, 101),
      forecast(NaN, 50),
      { ...forecast(), observedAt: "invalid" },
      { ...forecast(), expiresAt: midnight.toISOString() },
    ]) {
      expect(planToday({ ...input, resetForecast })).toEqual(normal);
    }
  });

  it("does not raise a cap for forecast horizons after the official reset", () => {
    const input = { startUtilization: 20, utilization: 20, resetsAt: inDays(0.5), anchor: midnight, capPercent: 25, now: midnight };
    expect(planToday({ ...input, resetForecast: forecast(100, 100) })).toEqual(planToday(input));
  });

  it("anchors forecast horizons to their observation time, including a partial day", () => {
    const noon = inDays(0.5);
    const input = { startUtilization: 20, utilization: 20, resetsAt: inDays(5), capPercent: 100, resetForecast: forecast(100, 100, noon), now: noon };
    expect(planToday({ ...input, anchor: midnight })!.targetPercent).toBeCloseTo(80 / 1.5);
    expect(planToday({ ...input, anchor: noon })!.targetPercent).toBeCloseTo(40);
  });

  it("keeps the weekend adjustment and cannot recommend more than the remaining quota", () => {
    const saturday = new Date(2026, 9, 10);
    const input = { startUtilization: 0, utilization: 0, resetsAt: new Date(2026, 9, 17), anchor: saturday, capPercent: 25, resetForecast: forecast(100, 100, saturday), now: saturday };
    expect(planToday(input)!.targetPercent).toBeCloseTo(25 * 2.5 * 0.75);
    expect(planToday({ ...input, startUtilization: 90, utilization: 95 })!.targetPercent).toBe(10);
    expect(planToday({ ...input, startUtilization: 100, utilization: 100 })!.targetPercent).toBe(0);
  });
});
