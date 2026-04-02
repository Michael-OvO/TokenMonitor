import { describe, expect, it } from "vitest";
import type { ProviderRateLimits, RateLimitsPayload } from "../types/index.js";
import { footerFiveHourPct } from "./footer.js";

function providerRateLimits(
  provider: "claude" | "codex",
  windows: ProviderRateLimits["windows"],
): ProviderRateLimits {
  return {
    provider,
    planTier: null,
    windows,
    extraUsage: null,
    stale: false,
    error: null,
    retryAfterSeconds: null,
    cooldownUntil: null,
    fetchedAt: "2026-03-17T12:00:00.000Z",
  };
}

function makePayload(): RateLimitsPayload {
  return {
    claude: providerRateLimits("claude", [
      {
        windowId: "five_hour",
        label: "Session (5hr)",
        utilization: 61,
        resetsAt: "2026-03-17T14:00:00.000Z",
      },
    ]),
    codex: providerRateLimits("codex", [
      {
        windowId: "primary",
        label: "Session (5hr)",
        utilization: 4,
        resetsAt: "2026-03-17T14:00:00.000Z",
      },
    ]),
  };
}

describe("footerFiveHourPct", () => {
  it("uses only the selected Claude provider window", () => {
    expect(footerFiveHourPct(makePayload(), "claude", Date.UTC(2026, 2, 17, 13, 0, 0))).toBe(61);
  });

  it("uses only the selected Codex provider window", () => {
    expect(footerFiveHourPct(makePayload(), "codex", Date.UTC(2026, 2, 17, 13, 0, 0))).toBe(4);
  });

  it("returns null when the selected provider has no usable 5h window", () => {
    const payload = makePayload();
    if (!payload.claude) throw new Error("expected claude payload");
    payload.claude.windows = [];

    expect(footerFiveHourPct(payload, "claude", Date.UTC(2026, 2, 17, 13, 0, 0))).toBeNull();
  });

  it("returns null when all providers are selected", () => {
    expect(footerFiveHourPct(makePayload(), "all")).toBeNull();
  });

  it("returns 0 when codex has not emitted 5h metadata yet", () => {
    const payload = makePayload();
    if (!payload.codex) throw new Error("expected codex payload");
    payload.codex.windows = [];
    payload.codex.error = "No rate limit data in Codex session files";

    expect(footerFiveHourPct(payload, "codex", Date.UTC(2026, 2, 17, 13, 0, 0))).toBe(0);
  });

  it("hides expired codex 5h usage after the refresh grace period", () => {
    const payload = makePayload();
    if (!payload.codex) throw new Error("expected codex payload");
    payload.codex.fetchedAt = "2026-03-17T12:00:00.000Z";

    expect(
      footerFiveHourPct(payload, "codex", Date.UTC(2026, 2, 17, 14, 1, 30)),
    ).toBeNull();
  });
});
