import { describe, expect, it } from "vitest";
import {
  hasRateLimitWindows,
  providerHasActiveCooldown,
  providerRateLimitViewState,
  rateLimitWindowResetLabel,
} from "./rateLimitsView.js";
import type { ProviderRateLimits } from "./types/index.js";

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
    stale: false,
    error: null,
    retryAfterSeconds: null,
    cooldownUntil: null,
    fetchedAt: "2026-03-17T07:00:00.000Z",
    ...overrides,
  };
}

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
});
