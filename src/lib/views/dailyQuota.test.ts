import { describe, expect, it } from "vitest";
import { planToday } from "./dailyQuota.js";

// Wednesday, local time.
const midnight = new Date(2026, 9, 7, 0, 0, 0);
const inDays = (d: number) => new Date(midnight.getTime() + d * 86_400_000);

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
    const risky = planToday({ startUtilization: 0, utilization: 0, resetsAt: inDays(2), anchor: midnight, capPercent: 25, resetRiskPercent: 40 })!;
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
});
