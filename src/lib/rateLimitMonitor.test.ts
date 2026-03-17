import { describe, expect, it } from "vitest";
import {
  createRateLimitsMonitorState,
  mergeProviderRateLimits,
  providerDeferredUntil,
  scopeRateLimitRequestState,
  shouldSuppressProviderError,
} from "./rateLimitMonitor.js";
import type { ProviderRateLimits } from "./types/index.js";

function providerRateLimits(
  provider: "claude" | "codex",
  overrides: Partial<ProviderRateLimits> = {},
): ProviderRateLimits {
  return {
    provider,
    planTier: provider === "claude" ? "Pro" : null,
    windows: [],
    extraUsage: null,
    stale: false,
    error: null,
    retryAfterSeconds: null,
    cooldownUntil: null,
    fetchedAt: "2026-03-17T12:00:00.000Z",
    ...overrides,
  };
}

describe("mergeProviderRateLimits", () => {
  it("keeps cached windows when a fresh provider response is an empty error", () => {
    const cached = providerRateLimits("claude", {
      windows: [
        {
          windowId: "five_hour",
          label: "Session (5hr)",
          utilization: 31,
          resetsAt: "2026-03-17T14:00:00.000Z",
        },
      ],
      fetchedAt: "2026-03-17T12:00:00.000Z",
    });
    const fresh = providerRateLimits("claude", {
      windows: [],
      error: "429 Too Many Requests",
      cooldownUntil: "2026-03-17T12:05:00.000Z",
      fetchedAt: "2026-03-17T12:01:00.000Z",
    });

    expect(mergeProviderRateLimits(fresh, cached)).toEqual({
      ...cached,
      stale: true,
      error: "429 Too Many Requests",
      cooldownUntil: "2026-03-17T12:05:00.000Z",
      fetchedAt: "2026-03-17T12:01:00.000Z",
    });
  });
});

describe("providerDeferredUntil", () => {
  it("chooses the later of cooldown and throttling windows", () => {
    const rateLimits = providerRateLimits("claude", {
      cooldownUntil: "2026-03-17T12:01:00.000Z",
      fetchedAt: "2026-03-17T12:00:30.000Z",
    });

    expect(
      providerDeferredUntil(rateLimits, "claude", Date.parse("2026-03-17T12:00:45.000Z")),
    ).toBe("2026-03-17T12:05:30.000Z");
  });
});

describe("scopeRateLimitRequestState", () => {
  it("aggregates all-scope monitor state across providers", () => {
    const state = createRateLimitsMonitorState();
    state.claude.loaded = true;
    state.claude.deferredUntil = "2026-03-17T12:05:00.000Z";
    state.codex.loading = true;
    state.codex.loaded = true;

    expect(scopeRateLimitRequestState(state, "all")).toEqual({
      loading: true,
      loaded: true,
      error: null,
      deferredUntil: "2026-03-17T12:05:00.000Z",
    });
  });
});

describe("shouldSuppressProviderError", () => {
  it("suppresses only the first failure when prior usable data exists", () => {
    expect(shouldSuppressProviderError(true, 0)).toBe(true);
    expect(shouldSuppressProviderError(true, 1)).toBe(false);
    expect(shouldSuppressProviderError(false, 0)).toBe(false);
  });
});
