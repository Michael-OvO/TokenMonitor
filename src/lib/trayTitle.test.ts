import { describe, it, expect } from "vitest";
import { formatTrayTitle } from "./trayTitle.js";
import type { TrayConfig, RateLimitsPayload } from "./types/index.js";

const DEFAULT_CONFIG: TrayConfig = {
  barDisplay: 'both',
  barProvider: 'claude',
  showPercentages: true,
  percentageFormat: 'compact',
  showCost: true,
  costPrecision: 'full',
};

const RATE_LIMITS: RateLimitsPayload = {
  claude: {
    provider: 'claude',
    planTier: 'Max 5x',
    windows: [{ windowId: 'w1', label: 'Primary', utilization: 72, resetsAt: null }],
    extraUsage: null,
    stale: false,
    error: null,
    retryAfterSeconds: null,
    cooldownUntil: null,
    fetchedAt: '2026-03-18T00:00:00Z',
  },
  codex: {
    provider: 'codex',
    planTier: 'Pro',
    windows: [{ windowId: 'w2', label: 'Primary', utilization: 35, resetsAt: null }],
    extraUsage: null,
    stale: false,
    error: null,
    retryAfterSeconds: null,
    cooldownUntil: null,
    fetchedAt: '2026-03-18T00:00:00Z',
  },
};

describe("formatTrayTitle", () => {
  it("returns compact percentages + full cost", () => {
    expect(formatTrayTitle(DEFAULT_CONFIG, RATE_LIMITS, 12.456)).toBe("72 · 35  $12.46");
  });

  it("returns compact percentages + whole cost", () => {
    const config = { ...DEFAULT_CONFIG, costPrecision: 'whole' as const };
    expect(formatTrayTitle(config, RATE_LIMITS, 12.456)).toBe("72 · 35  $12");
  });

  it("returns only cost when percentages off", () => {
    const config = { ...DEFAULT_CONFIG, showPercentages: false };
    expect(formatTrayTitle(config, RATE_LIMITS, 12.456)).toBe("$12.46");
  });

  it("returns only percentages when cost off", () => {
    const config = { ...DEFAULT_CONFIG, showCost: false };
    expect(formatTrayTitle(config, RATE_LIMITS, 12.456)).toBe("72 · 35");
  });

  it("returns empty string when both off", () => {
    const config = { ...DEFAULT_CONFIG, showPercentages: false, showCost: false };
    expect(formatTrayTitle(config, RATE_LIMITS, 12.456)).toBe("");
  });

  it("shows both percentages even when barDisplay is single", () => {
    const config = { ...DEFAULT_CONFIG, barDisplay: 'single' as const, barProvider: 'claude' as const };
    expect(formatTrayTitle(config, RATE_LIMITS, 12.456)).toBe("72 · 35  $12.46");
  });

  it("shows percentages when barDisplay is off", () => {
    const config = { ...DEFAULT_CONFIG, barDisplay: 'off' as const };
    expect(formatTrayTitle(config, RATE_LIMITS, 12.456)).toBe("72 · 35  $12.46");
  });

  it("shows verbose format", () => {
    const config = { ...DEFAULT_CONFIG, percentageFormat: 'verbose' as const };
    expect(formatTrayTitle(config, RATE_LIMITS, 12.456)).toBe("Claude Code 72%  Codex 35%  $12.46");
  });

  it("handles null rate limits gracefully", () => {
    const config = { ...DEFAULT_CONFIG };
    expect(formatTrayTitle(config, null, 5.0)).toBe("$5.00");
  });

  it("returns empty string when bars off, percentages off, cost off", () => {
    const config = { ...DEFAULT_CONFIG, barDisplay: 'off' as const, showPercentages: false, showCost: false };
    expect(formatTrayTitle(config, null, 0)).toBe("");
  });
});
