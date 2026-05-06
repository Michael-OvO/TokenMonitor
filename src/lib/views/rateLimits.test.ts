import { describe, expect, it } from "vitest";
import {
  currentRateLimitWindows,
  hasRateLimitWindows,
  providerHasActiveCooldown,
  providerRateLimitViewState,
  rateLimitWindowResetLabel,
} from "./rateLimits.js";
import type { ProviderRateLimits } from "../types/index.js";

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
