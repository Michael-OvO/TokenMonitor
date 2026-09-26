import { describe, expect, it } from "vitest";
import type { ProviderRateLimits, RateLimitWindow } from "../types/index.js";
import { formatCostRange } from "../utils/format.js";
import { budgetAsks, budgetRange, needsSpend, windowSpan, windowStart } from "./planBudget.js";

const win = (windowId: string, utilization: number, extra: Partial<RateLimitWindow> = {}): RateLimitWindow => ({
  windowId,
  label: windowId,
  utilization,
  resetsAt: "2026-09-24T12:00:00Z",
  ...extra,
});

describe("windowSpan", () => {
  it("uses the vendor's length, else known ids, else nothing", () => {
    expect(windowSpan("codex", win("primary", 0, { windowMinutes: 1_440 }))).toEqual({ minutes: 1_440, unit: "day" });
    expect(windowSpan("codex", win("secondary", 0))?.unit).toBe("wk");
    expect(windowSpan("claude", win("five_hour", 0))?.unit).toBe("5h");
    expect(windowSpan("claude", win("seven_day_opus", 0))?.unit).toBe("wk");
    expect(windowSpan("kimi", win("hour_1", 0, { windowMinutes: 60 }))?.unit).toBe("hr");
    expect(windowSpan("cursor", win("api", 0))?.unit).toBe("mo");
    expect(windowSpan("claude", win("bonus_pool", 0))).toBeNull();
  });

  it("starts a monthly window one calendar month back", () => {
    const span = windowSpan("cursor", win("api", 0))!;
    const start = windowStart(new Date(2026, 2, 31, 9).toISOString(), span);
    expect([start.getMonth(), start.getDate()]).toEqual([1, 28]);
    expect(windowStart("2026-09-24T12:00:00Z", windowSpan("claude", win("five_hour", 0))!).toISOString())
      .toBe("2026-09-24T07:00:00.000Z");
  });
});

const spent = (spend: number) => ({ spend, models: [] });

describe("budgetRange", () => {
  it("spans cheapest to dearest learned model once there is data", () => {
    const models = [
      { model: "Opus", usd: 900, lowUsd: 850, highUsd: 950 },
      { model: "Sonnet", usd: 2500, lowUsd: 2400, highUsd: 2600 },
    ];
    expect(budgetRange("claude", win("seven_day", 30), { spend: 1, models }, [])).toEqual([850, 2600]);
  });

  it("falls back to spend ÷ utilization", () => {
    const [lo, hi] = budgetRange("claude", win("five_hour", 40), spent(12), [])!;
    expect(lo).toBeLessThan(30);
    expect(hi).toBeGreaterThan(30);
  });

  it("refuses thin data and per-model meters", () => {
    expect(budgetRange("claude", win("five_hour", 4), spent(1), [])).toBeNull();
    expect(budgetRange("claude", win("seven_day_opus", 50), spent(10), [])).toBeNull();
    expect(needsSpend("kimi", "summary")).toBe(true);
  });

  it("takes a stated pool size as-is and removes it from Cursor's other pool", () => {
    const api = win("api", 50, { budgetUsd: 20 });
    const fp = win("first_party", 20);
    expect(budgetRange("cursor", api, undefined, [api, fp])).toEqual([20, 20]);
    // $30 spent, $10 of it on the API pool → $20 at 20% ≈ $100
    const [lo, hi] = budgetRange("cursor", fp, spent(30), [api, fp])!;
    expect(lo).toBeLessThan(100);
    expect(hi).toBeGreaterThan(100);
  });
});

describe("budgetAsks", () => {
  it("changes only with the bars' windows or a new reading", () => {
    const limits = (fetchedAt: string, windows: RateLimitWindow[]): ProviderRateLimits => ({
      provider: "claude",
      planTier: null,
      windows,
      extraUsage: null,
      credits: null,
      stale: false,
      error: null,
      retryAfterSeconds: null,
      cooldownUntil: null,
      fetchedAt,
    });
    const bars = () => [win("five_hour", 40), win("seven_day_opus", 10)];
    const asks = budgetAsks(limits("t1", bars()));
    expect(JSON.parse(asks).asks).toEqual([{ windowId: "five_hour", since: "2026-09-24T07:00:00.000Z" }]);
    expect(budgetAsks(limits("t1", bars()))).toBe(asks);
    expect(budgetAsks(limits("t2", bars()))).not.toBe(asks);
    expect(budgetAsks(limits("t1", [win("five_hour", 40, { resetsAt: "2026-09-24T13:00:00Z" })]))).not.toBe(asks);
  });
});

describe("formatCostRange", () => {
  it("rounds both ends to two significant figures", () => {
    expect(formatCostRange(1834, 2260)).toBe("$1,800–2,300");
    expect(formatCostRange(183, 187)).toBe("$180–190");
    expect(formatCostRange(20, 20)).toBe("$20");
  });
});
