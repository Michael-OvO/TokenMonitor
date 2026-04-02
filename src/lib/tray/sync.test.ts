import { describe, expect, it, vi } from "vitest";
import type { RateLimitsPayload, TrayConfig } from "../types/index.js";
import { syncTrayConfig, trayConfigPayload } from "./sync.js";

const CONFIG: TrayConfig = {
  barDisplay: "both",
  barProvider: "claude",
  showPercentages: true,
  percentageFormat: "compact",
  showCost: true,
  costPrecision: "full",
};

const RATE_LIMITS: RateLimitsPayload = {
  claude: {
    provider: "claude",
    planTier: "Max 5x",
    windows: [{ windowId: "c", label: "Primary", utilization: 72, resetsAt: null }],
    extraUsage: null,
    stale: false,
    error: null,
    retryAfterSeconds: null,
    cooldownUntil: null,
    fetchedAt: "2026-03-18T00:00:00Z",
  },
  codex: {
    provider: "codex",
    planTier: "Pro",
    windows: [{ windowId: "x", label: "Primary", utilization: 35, resetsAt: null }],
    extraUsage: null,
    stale: false,
    error: null,
    retryAfterSeconds: null,
    cooldownUntil: null,
    fetchedAt: "2026-03-18T00:00:00Z",
  },
};

describe("trayConfigPayload", () => {
  it("extracts primary utilization for both providers", () => {
    expect(trayConfigPayload(CONFIG, RATE_LIMITS)).toEqual({
      config: CONFIG,
      claudeUtil: 72,
      codexUtil: 35,
    });
  });

  it("falls back to null utilization when no rate limits exist", () => {
    expect(trayConfigPayload(CONFIG, null)).toEqual({
      config: CONFIG,
      claudeUtil: null,
      codexUtil: null,
    });
  });
});

describe("syncTrayConfig", () => {
  it("forwards the normalized tray payload to the backend", async () => {
    const invokeFn = vi.fn().mockResolvedValue(undefined);

    await syncTrayConfig(CONFIG, RATE_LIMITS, invokeFn);

    expect(invokeFn).toHaveBeenCalledWith("set_tray_config", {
      config: CONFIG,
      claudeUtil: 72,
      codexUtil: 35,
    });
  });
});
