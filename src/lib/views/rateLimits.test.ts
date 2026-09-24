import { describe, expect, it } from "vitest";
import {
  currentRateLimitWindows,
  hasRateLimitWindows,
  providerHasActiveCooldown,
  providerRateLimitViewState,
  assignChipSides,
  formatCompactTimeLeft,
  placeChips,
  rateLimitWindowResetLabel,
  resetTimelineLayout,
} from "./rateLimits.js";
import type { ProviderRateLimits, UsageLimitReset, UsageLimitResets } from "../types/index.js";

const NOW = Date.parse("2026-09-23T12:00:00Z");
const DAY = 86_400_000;
const shortDate = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric" });

function reset(daysFromNow: number, title = "Full reset"): UsageLimitReset {
  return {
    title,
    grantedAt: new Date(NOW - 20 * DAY).toISOString(),
    expiresAt: new Date(NOW + daysFromNow * DAY).toISOString(),
  };
}

function resets(days: number[], available = days.length): UsageLimitResets {
  return { available, resets: days.map((d) => reset(d)) };
}

function providerRateLimits(
  overrides: Partial<ProviderRateLimits> = {},
): ProviderRateLimits {
  return {
    provider: "claude",
    planTier: "Pro",
    windows: [
      {
        windowId: "primary",
        label: "5h",
        utilization: 42,
        resetsAt: "2026-03-17T12:00:00.000Z",
      },
    ],
    extraUsage: null,
    credits: null,
    stale: false,
    error: null,
    retryAfterSeconds: null,
    cooldownUntil: null,
    fetchedAt: "2026-03-17T07:00:00.000Z",
    ...overrides,
  };
}

describe("formatCompactTimeLeft", () => {
  it("uses one coarse unit, rounded up", () => {
    expect(formatCompactTimeLeft(11 * DAY)).toBe("11d");
    expect(formatCompactTimeLeft(1.2 * DAY)).toBe("2d");
    expect(formatCompactTimeLeft(17.5 * 3_600_000)).toBe("18h");
    expect(formatCompactTimeLeft(40 * 60_000)).toBe("40m");
    expect(formatCompactTimeLeft(10)).toBe("1m");
  });
});

describe("assignChipSides", () => {
  it("keeps every chip below the bar while they fit", () => {
    expect(assignChipSides([50, 150, 250], [60, 60, 60])).toEqual(["below", "below", "below"]);
  });

  it("splits a close pair across the bar", () => {
    expect(assignChipSides([100, 110], [60, 60])).toEqual(["below", "above"]);
  });

  it("falls back to the less crowded side once both are taken", () => {
    expect(assignChipSides([100, 110, 120], [60, 60, 60])).toEqual(["below", "above", "below"]);
  });
});

describe("placeChips", () => {
  it("centres each chip under its anchor when there is room", () => {
    expect(placeChips([50, 200], [60, 60], 300)).toEqual([
      { leftPx: 20, row: 0 },
      { leftPx: 170, row: 0 },
    ]);
  });

  it("pushes a chip sideways just far enough to clear its neighbour", () => {
    const [first, second] = placeChips([100, 110], [60, 60], 300);
    expect(first.leftPx).toBe(70);
    expect(second.leftPx).toBe(134);
  });

  it("keeps chips inside the strip, pulling earlier ones left if needed", () => {
    const [first, second] = placeChips([280, 295], [60, 60], 300);
    expect(second.leftPx).toBe(240);
    expect(first.leftPx).toBe(176);
    expect(placeChips([5], [60], 300)[0].leftPx).toBe(0);
  });

  it("alternates rows when the chips cannot all fit in one", () => {
    const placements = placeChips([20, 60, 100, 140, 180], [70, 70, 70, 70, 70], 300);
    expect(placements.map((p) => p.row)).toEqual([0, 1, 0, 1, 0]);
    const rowZero = placements.filter((p) => p.row === 0).map((p) => p.leftPx);
    expect(rowZero[1]).toBeGreaterThanOrEqual(rowZero[0] + 70 + 4);
  });
});

describe("resetTimelineLayout", () => {
  it("is absent when the provider reports no resets", () => {
    expect(resetTimelineLayout(null, NOW)).toBeNull();
    expect(resetTimelineLayout(undefined, NOW)).toBeNull();
  });

  it("places dots proportionally on a four-week strip with weekly ticks", () => {
    const layout = resetTimelineLayout(resets([14]), NOW)!;
    expect(layout.available).toBe(1);
    expect(layout.horizonDays).toBe(28);
    expect(layout.weekTickPcts).toEqual([25, 50, 75]);
    expect(layout.markers).toHaveLength(1);
    expect(layout.markers[0].leftPct).toBeCloseTo(50, 6);
    expect(layout.markers[0].dotPct).toBeCloseTo(50, 6);
    expect(layout.markers[0].dateLabel).toBe(shortDate.format(NOW + 14 * DAY));
    expect(layout.markers[0].title).toContain("Full reset · expires ");
    expect(layout.markers[0].title).toContain("granted ");
  });

  it("grows the horizon to whole weeks past the last expiry", () => {
    const layout = resetTimelineLayout(resets([33, 5]), NOW)!;
    expect(layout.horizonDays).toBe(35);
    expect(layout.markers.map((m) => m.daysLeft)).toEqual([5, 33]);
    expect(layout.markers[1].leftPct).toBeCloseTo((33 / 35) * 100, 6);
  });

  it("flags resets expiring within three days", () => {
    const layout = resetTimelineLayout(resets([2, 10]), NOW)!;
    expect(layout.markers.map((m) => m.urgent)).toEqual([true, false]);
  });

  it("keeps every reset as its own marker and nudges touching dots apart", () => {
    const close = resetTimelineLayout(resets([10, 10.5]), NOW)!;
    expect(close.markers).toHaveLength(2);
    expect(close.markers[0].dotPct).toBeCloseTo((10 / 28) * 100, 6);
    expect(close.markers[1].leftPct).toBeCloseTo((10.5 / 28) * 100, 6);
    expect(close.markers[1].dotPct).toBeCloseTo((10 / 28) * 100 + 4, 6);
    const apart = resetTimelineLayout(resets([10, 20]), NOW)!;
    expect(apart.markers.map((m) => m.dotPct)).toEqual(apart.markers.map((m) => m.leftPct));
  });

  it("never pushes a nudged dot past the end of the strip", () => {
    const layout = resetTimelineLayout(resets([27.9, 28]), NOW)!;
    expect(layout.markers[1].dotPct).toBe(100);
  });

  it("labels each reset with a compact countdown and surfaces the nearest one", () => {
    const layout = resetTimelineLayout(resets([10.4, 29]), NOW)!;
    expect(layout.markers.map((m) => m.leftLabel)).toEqual(["11d", "29d"]);
    expect(layout.nextLeftLabel).toBe("11d");
    expect(resetTimelineLayout(resets([-1]), NOW)!.nextLeftLabel).toBeNull();
  });

  it("ignores expired resets but keeps the provider's count", () => {
    const layout = resetTimelineLayout(resets([-1, 5], 2), NOW)!;
    expect(layout.markers).toHaveLength(1);
    expect(layout.available).toBe(2);
  });
});

describe("hasRateLimitWindows", () => {
  it("returns false when the provider payload is missing", () => {
    expect(hasRateLimitWindows(null)).toBe(false);
    expect(hasRateLimitWindows(undefined)).toBe(false);
  });

  it("returns false for error payloads that contain no windows", () => {
    expect(
      hasRateLimitWindows(
        providerRateLimits({
          windows: [],
          error: "429 Too Many Requests",
        }),
      ),
    ).toBe(false);
  });

  it("returns true when at least one rate-limit window is present", () => {
    expect(hasRateLimitWindows(providerRateLimits())).toBe(true);
  });
});

describe("providerRateLimitViewState", () => {
  it("returns ready when a provider has windows", () => {
    expect(providerRateLimitViewState(providerRateLimits())).toBe("ready");
  });

  it("returns error when a provider payload has no windows and includes an error", () => {
    expect(
      providerRateLimitViewState(
        providerRateLimits({
          windows: [],
          error: "429 Too Many Requests",
        }),
      ),
    ).toBe("error");
  });

  it("returns ready for codex when metadata has not been emitted yet", () => {
    expect(
      providerRateLimitViewState(
        providerRateLimits({
          provider: "codex",
          planTier: null,
          windows: [],
          error: "No rate limit data in Codex session files",
        }),
      ),
    ).toBe("ready");
  });

  it("returns empty when a provider payload has no windows and no error", () => {
    expect(
      providerRateLimitViewState(
        providerRateLimits({
          windows: [],
          error: null,
        }),
      ),
    ).toBe("empty");
  });

  it("returns ready when codex only has expired windows — fallback provides a zeroed window", () => {
    expect(
      providerRateLimitViewState(
        providerRateLimits({
          provider: "codex",
          planTier: null,
        }),
        Date.UTC(2026, 2, 17, 12, 1, 30),
      ),
    ).toBe("ready");
  });
});

describe("currentRateLimitWindows", () => {
  it("keeps current codex windows before the refresh grace period elapses", () => {
    expect(
      currentRateLimitWindows(
        providerRateLimits({
          provider: "codex",
          planTier: null,
        }),
        Date.UTC(2026, 2, 17, 12, 0, 30),
      ),
    ).toHaveLength(1);
  });

  it("falls back to zeroed 5h window after codex windows expire", () => {
    expect(
      currentRateLimitWindows(
        providerRateLimits({
          provider: "codex",
          planTier: null,
        }),
        Date.UTC(2026, 2, 17, 12, 1, 30),
      ),
    ).toEqual([
      {
        windowId: "primary",
        label: "Session (5hr)",
        utilization: 0,
        resetsAt: null,
      },
    ]);
  });

  it("injects zeroed 5h fallback when only the codex primary window has expired", () => {
    expect(
      currentRateLimitWindows(
        providerRateLimits({
          provider: "codex",
          planTier: "Pro",
          windows: [
            { windowId: "primary", label: "Session (5hr)", utilization: 5, resetsAt: "2026-03-17T08:00:00.000Z" },
            { windowId: "secondary", label: "Weekly (7 day)", utilization: 36, resetsAt: "2026-03-20T18:00:00.000Z" },
          ],
        }),
        Date.UTC(2026, 2, 17, 12, 1, 30),
      ),
    ).toEqual([
      { windowId: "primary", label: "Session (5hr)", utilization: 0, resetsAt: null },
      { windowId: "secondary", label: "Weekly (7 day)", utilization: 36, resetsAt: "2026-03-20T18:00:00.000Z" },
    ]);
  });

  it("synthesizes a zeroed codex 5h window when metadata is missing", () => {
    expect(
      currentRateLimitWindows(
        providerRateLimits({
          provider: "codex",
          planTier: null,
          windows: [],
          error: "No rate limit data in Codex session files",
        }),
      ),
    ).toEqual([
      {
        windowId: "primary",
        label: "Session (5hr)",
        utilization: 0,
        resetsAt: null,
      },
    ]);
  });

  it("does not synthesize a zeroed codex window for unrelated errors", () => {
    expect(
      currentRateLimitWindows(
        providerRateLimits({
          provider: "codex",
          planTier: null,
          windows: [],
          error: "Usage API returned 500",
        }),
      ),
    ).toEqual([]);
  });
});

describe("providerHasActiveCooldown", () => {
  it("returns false when the provider payload has no cooldown", () => {
    expect(providerHasActiveCooldown(providerRateLimits(), Date.UTC(2026, 2, 17, 11))).toBe(false);
  });

  it("returns true while the cooldown deadline is still in the future", () => {
    expect(
      providerHasActiveCooldown(
        providerRateLimits({
          windows: [],
          error: "429 Too Many Requests",
          cooldownUntil: "2026-03-17T12:05:00.000Z",
        }),
        Date.UTC(2026, 2, 17, 12, 4, 0),
      ),
    ).toBe(true);
  });
});

describe("rateLimitWindowResetLabel", () => {
  it("shows the retry countdown when stale data is waiting for a cooldown to expire", () => {
    expect(
      rateLimitWindowResetLabel(
        providerRateLimits({
          stale: true,
          cooldownUntil: "2026-03-17T12:10:00.000Z",
        }),
        "2026-03-17T12:00:00.000Z",
        Date.UTC(2026, 2, 17, 12, 5, 0),
      ),
    ).toBe("Retry in 5m");
  });

  it("keeps the awaiting-refresh label when stale data has no active cooldown", () => {
    expect(
      rateLimitWindowResetLabel(
        providerRateLimits({
          stale: true,
        }),
        "2026-03-17T12:00:00.000Z",
        Date.UTC(2026, 2, 17, 12, 5, 0),
      ),
    ).toBe("Awaiting refresh");
  });

  it("shows awaiting-refresh for expired codex windows after a short grace period", () => {
    expect(
      rateLimitWindowResetLabel(
        providerRateLimits({
          provider: "codex",
          planTier: null,
        }),
        "2026-03-17T12:00:00.000Z",
        Date.UTC(2026, 2, 17, 12, 1, 30),
      ),
    ).toBe("Awaiting refresh");
  });

  it("appends (stale) when data is stale but the window has not yet reset", () => {
    expect(
      rateLimitWindowResetLabel(
        providerRateLimits({
          provider: "codex",
          stale: true,
        }),
        "2026-03-17T14:00:00.000Z",
        Date.UTC(2026, 2, 17, 11, 0, 0),
      ),
    ).toMatch(/\(stale\)$/);
  });

  it("does not append (stale) when data is fresh", () => {
    expect(
      rateLimitWindowResetLabel(
        providerRateLimits({
          provider: "codex",
          stale: false,
        }),
        "2026-03-17T14:00:00.000Z",
        Date.UTC(2026, 2, 17, 11, 0, 0),
      ),
    ).not.toMatch(/stale/);
  });
});
