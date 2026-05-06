import { describe, expect, it } from "vitest";
import {
  createRateLimitsMonitorState,
  mergeProviderRateLimits,
  providerDeferredUntil,
  scopeRateLimitRequestState,
  shouldSuppressProviderError,
} from "./rateLimitMonitor.js";
import type { ProviderRateLimits } from "../types/index.js";

function providerRateLimits(
  provider: "claude" | "codex",
  overrides: Partial<ProviderRateLimits> = {},
): ProviderRateLimits {
  return {
    provider,
    planTier: provider === "claude" ? "Pro" : null,
    windows: [],
    extraUsage: null,
    credits: null,
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

  it("keeps Codex utilization from moving backward within the same reset window", () => {
    const cached = providerRateLimits("codex", {
      windows: [
        {
          windowId: "primary",
          label: "Session (5hr)",
          utilization: 3,
          resetsAt: "2026-03-19T00:38:11+00:00",
        },
        {
          windowId: "secondary",
          label: "Weekly (7 day)",
          utilization: 1,
          resetsAt: "2026-03-25T19:38:11+00:00",
        },
      ],
      fetchedAt: "2026-03-18T16:43:18.569Z",
    });
    const fresh = providerRateLimits("codex", {
      windows: [
        {
          windowId: "primary",
          label: "Session (5hr)",
          utilization: 0,
          resetsAt: "2026-03-19T00:38:11+00:00",
        },
        {
          windowId: "secondary",
          label: "Weekly (7 day)",
          utilization: 0,
          resetsAt: "2026-03-25T19:38:11+00:00",
        },
      ],
      fetchedAt: "2026-03-18T16:43:45.969Z",
    });

    expect(mergeProviderRateLimits(fresh, cached)).toEqual({
      ...fresh,
      windows: [
        {
          windowId: "primary",
          label: "Session (5hr)",
          utilization: 3,
          resetsAt: "2026-03-19T00:38:11+00:00",
        },
        {
          windowId: "secondary",
          label: "Weekly (7 day)",
          utilization: 1,
          resetsAt: "2026-03-25T19:38:11+00:00",
        },
      ],
    });
  });
});

describe("providerDeferredUntil", () => {
  it("returns the cooldown time when Claude has no fetch throttle", () => {
    // Post-statusline-rewrite, Claude's `minFetchIntervalMs` is 0 — every
    // fetch is a local file read so there's no budget to conserve. The
    // only deferral source for Claude is an active server-side cooldown.
    const rateLimits = providerRateLimits("claude", {
      windows: [
        { windowId: "five_hour", label: "Session (5hr)", utilization: 10, resetsAt: "2026-03-17T17:00:00.000Z" },
      ],
      cooldownUntil: "2026-03-17T12:01:00.000Z",
      fetchedAt: "2026-03-17T12:00:30.000Z",
    });

    expect(
      providerDeferredUntil(rateLimits, "claude", Date.parse("2026-03-17T12:00:45.000Z")),
    ).toBe("2026-03-17T12:01:00.000Z");
  });

  it("returns null when Claude has neither cooldown nor throttle", () => {
    const rateLimits = providerRateLimits("claude", {
      windows: [
        { windowId: "five_hour", label: "Session (5hr)", utilization: 10, resetsAt: "2026-03-17T17:00:00.000Z" },
      ],
      fetchedAt: "2026-03-17T12:00:30.000Z",
    });

    expect(
      providerDeferredUntil(rateLimits, "claude", Date.parse("2026-03-17T12:00:45.000Z")),
    ).toBeNull();
  });

  it("does not throttle when cached data has no usable windows", () => {
    const rateLimits = providerRateLimits("claude", {
      windows: [],
      fetchedAt: "2026-03-17T12:00:30.000Z",
    });

    expect(
      providerDeferredUntil(rateLimits, "claude", Date.parse("2026-03-17T12:00:45.000Z")),
    ).toBeNull();
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
