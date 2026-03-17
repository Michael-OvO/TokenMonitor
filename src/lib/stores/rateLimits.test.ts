import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import type { ProviderRateLimits, RateLimitsPayload } from "../types/index.js";

const mockInvoke = vi.fn();
const mockLoad = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

vi.mock("@tauri-apps/plugin-store", () => ({
  load: (...args: unknown[]) => mockLoad(...args),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

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
    fetchedAt: "2026-03-17T00:00:00.000Z",
    ...overrides,
  };
}

function makePayload(overrides: Partial<RateLimitsPayload> = {}): RateLimitsPayload {
  return {
    claude: providerRateLimits("claude"),
    codex: providerRateLimits("codex"),
    ...overrides,
  };
}

function makePersistedStore(saved: RateLimitsPayload | null = null) {
  return {
    get: vi.fn().mockResolvedValue(saved),
    set: vi.fn().mockResolvedValue(undefined),
    save: vi.fn().mockResolvedValue(undefined),
  };
}

async function loadRateLimitStore() {
  return import("./rateLimits.js");
}

beforeEach(() => {
  vi.resetModules();
  vi.useRealTimers();
  mockInvoke.mockReset();
  mockLoad.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("hydrateRateLimits", () => {
  it("loads the persisted payload into the active store", async () => {
    const cached = makePayload({
      claude: providerRateLimits("claude", {
        stale: true,
        error: "Usage API returned 429 Too Many Requests",
        retryAfterSeconds: 45,
        cooldownUntil: "2026-03-17T12:00:45.000Z",
      }),
    });
    const store = makePersistedStore(cached);
    mockLoad.mockResolvedValueOnce(store);

    const { hydrateRateLimits, rateLimitsData, rateLimitsRequestState } =
      await loadRateLimitStore();
    await hydrateRateLimits();

    expect(store.get).toHaveBeenCalledWith("payload");
    expect(get(rateLimitsData)).toEqual(cached);
    expect(get(rateLimitsRequestState).deferredUntil).toBe(
      "2026-03-17T12:00:45.000Z",
    );
  });
});

describe("fetchRateLimits", () => {
  it("tracks loading, scopes the backend request, and persists the resolved payload", async () => {
    const request = deferred<RateLimitsPayload>();
    const store = makePersistedStore();
    mockLoad.mockResolvedValueOnce(store);
    mockInvoke.mockReturnValueOnce(request.promise);

    const { fetchRateLimits, rateLimitsData, rateLimitsRequestState } =
      await loadRateLimitStore();

    const fetchPromise = fetchRateLimits("claude");

    await vi.waitFor(() => {
      expect(get(rateLimitsRequestState)).toEqual({
        loading: true,
        loaded: false,
        error: null,
        deferredUntil: null,
      });
    });

    const payload = makePayload({
      claude: providerRateLimits("claude", {
        windows: [
          {
            windowId: "five_hour",
            label: "Session (5hr)",
            utilization: 24,
            resetsAt: "2026-03-17T14:00:00.000Z",
          },
        ],
      }),
    });

    request.resolve(payload);
    await fetchPromise;

    expect(mockInvoke).toHaveBeenCalledWith("get_rate_limits", {
      provider: "claude",
    });
    expect(get(rateLimitsData)).toEqual(payload);
    expect(store.set).toHaveBeenCalledWith("payload", payload);
    expect(store.save).toHaveBeenCalledTimes(1);
    expect(get(rateLimitsRequestState)).toEqual({
      loading: false,
      loaded: true,
      error: null,
      deferredUntil: null,
    });
  });

  it("defers provider refreshes while a persisted cooldown is still active and retries automatically", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-17T12:00:00.000Z"));

    const cached = makePayload({
      claude: providerRateLimits("claude", {
        stale: true,
        windows: [
          {
            windowId: "five_hour",
            label: "Session (5hr)",
            utilization: 61,
            resetsAt: "2026-03-17T14:00:00.000Z",
          },
        ],
        error: "Usage API returned 429 Too Many Requests",
        retryAfterSeconds: 2,
        cooldownUntil: "2026-03-17T12:00:02.000Z",
        fetchedAt: "2026-03-17T12:00:00.000Z",
      }),
    });
    const store = makePersistedStore(cached);
    mockLoad.mockResolvedValueOnce(store);
    mockInvoke.mockResolvedValueOnce(
      makePayload({
        claude: providerRateLimits("claude", {
          windows: [
            {
              windowId: "five_hour",
              label: "Session (5hr)",
              utilization: 18,
              resetsAt: "2026-03-17T14:30:00.000Z",
            },
          ],
        }),
      }),
    );

    const { fetchRateLimits, rateLimitsRequestState } = await loadRateLimitStore();

    await fetchRateLimits("claude");

    expect(mockInvoke).not.toHaveBeenCalled();
    expect(get(rateLimitsRequestState)).toEqual({
      loading: false,
      loaded: true,
      error: null,
      deferredUntil: "2026-03-17T12:05:00.000Z",
    });

    await fetchRateLimits("claude");
    expect(mockInvoke).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(300_100);

    expect(mockInvoke).toHaveBeenCalledWith("get_rate_limits", {
      provider: "claude",
    });
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(get(rateLimitsRequestState).loading).toBe(false);
    expect(get(rateLimitsRequestState).error).toBeNull();
  });

  it("throttles Claude refreshes to once every five minutes after a successful fetch", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-17T12:00:30.000Z"));

    const cached = makePayload({
      claude: providerRateLimits("claude", {
        windows: [
          {
            windowId: "five_hour",
            label: "Session (5hr)",
            utilization: 14,
            resetsAt: "2026-03-17T14:00:00.000Z",
          },
        ],
        fetchedAt: "2026-03-17T12:00:00.000Z",
      }),
    });
    const store = makePersistedStore(cached);
    mockLoad.mockResolvedValueOnce(store);
    mockInvoke.mockResolvedValueOnce(
      makePayload({
        claude: providerRateLimits("claude", {
          windows: [
            {
              windowId: "five_hour",
              label: "Session (5hr)",
              utilization: 18,
              resetsAt: "2026-03-17T14:30:00.000Z",
            },
          ],
          fetchedAt: "2026-03-17T12:01:00.000Z",
        }),
      }),
    );

    const { fetchRateLimits, rateLimitsRequestState } = await loadRateLimitStore();

    await fetchRateLimits("claude");

    expect(mockInvoke).not.toHaveBeenCalled();
    expect(get(rateLimitsRequestState)).toEqual({
      loading: false,
      loaded: true,
      error: null,
      deferredUntil: "2026-03-17T12:05:00.000Z",
    });

    await vi.advanceTimersByTimeAsync(270_100);

    expect(mockInvoke).toHaveBeenCalledWith("get_rate_limits", {
      provider: "claude",
    });
    expect(mockInvoke).toHaveBeenCalledTimes(1);
  });

  it("preserves cached Claude windows when a fresh Claude response is an empty 429", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-17T12:05:00.000Z"));

    const cachedClaude = providerRateLimits("claude", {
      windows: [
        {
          windowId: "five_hour",
          label: "Session (5hr)",
          utilization: 31,
          resetsAt: "2026-03-17T14:00:00.000Z",
        },
      ],
      planTier: "Max 5x",
      fetchedAt: "2026-03-17T12:00:00.000Z",
    });
    const cached = makePayload({ claude: cachedClaude });
    const store = makePersistedStore(cached);
    mockLoad.mockResolvedValueOnce(store);
    mockInvoke.mockResolvedValueOnce({
      claude: providerRateLimits("claude", {
        windows: [],
        error: "Usage API returned 429 Too Many Requests",
        cooldownUntil: "2026-03-17T12:02:00.000Z",
        retryAfterSeconds: 120,
        fetchedAt: "2026-03-17T12:01:00.000Z",
      }),
      codex: providerRateLimits("codex", {
        windows: [
          {
            windowId: "primary",
            label: "Session (5hr)",
            utilization: 8,
            resetsAt: "2026-03-17T14:30:00.000Z",
          },
        ],
      }),
    } satisfies RateLimitsPayload);

    const { fetchRateLimits, rateLimitsData } = await loadRateLimitStore();

    await fetchRateLimits("claude");

    expect(get(rateLimitsData)?.claude).toEqual({
      ...cachedClaude,
      stale: true,
      error: "Usage API returned 429 Too Many Requests",
      retryAfterSeconds: 120,
      cooldownUntil: "2026-03-17T12:02:00.000Z",
      fetchedAt: "2026-03-17T12:01:00.000Z",
    });
    expect(store.set).toHaveBeenCalledWith(
      "payload",
      expect.objectContaining({
        claude: expect.objectContaining({
          windows: cachedClaude.windows,
          error: "Usage API returned 429 Too Many Requests",
          fetchedAt: "2026-03-17T12:01:00.000Z",
        }),
      }),
    );
  });

  it("refreshes only the eligible provider when all-scope Claude data is still throttled", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-17T12:00:30.000Z"));

    const cached = makePayload({
      claude: providerRateLimits("claude", {
        windows: [
          {
            windowId: "five_hour",
            label: "Session (5hr)",
            utilization: 14,
            resetsAt: "2026-03-17T14:00:00.000Z",
          },
        ],
        fetchedAt: "2026-03-17T12:00:00.000Z",
      }),
      codex: providerRateLimits("codex", {
        windows: [
          {
            windowId: "primary",
            label: "Session (5hr)",
            utilization: 7,
            resetsAt: "2026-03-17T13:00:00.000Z",
          },
        ],
        fetchedAt: "2026-03-17T11:58:00.000Z",
      }),
    });
    const store = makePersistedStore(cached);
    const codexOnlyPayload: RateLimitsPayload = {
      claude: null,
      codex: providerRateLimits("codex", {
        windows: [
          {
            windowId: "primary",
            label: "Session (5hr)",
            utilization: 9,
            resetsAt: "2026-03-17T13:30:00.000Z",
          },
        ],
        fetchedAt: "2026-03-17T12:00:30.000Z",
      }),
    };
    mockLoad.mockResolvedValueOnce(store);
    mockInvoke.mockResolvedValueOnce(codexOnlyPayload);

    const { fetchRateLimits, rateLimitsData, rateLimitsRequestState } = await loadRateLimitStore();

    await fetchRateLimits("all");

    expect(mockInvoke).toHaveBeenCalledWith("get_rate_limits", {
      provider: "codex",
    });
    expect(get(rateLimitsData)?.claude).toEqual(cached.claude);
    expect(get(rateLimitsData)?.codex).toEqual(codexOnlyPayload.codex);
    expect(get(rateLimitsRequestState)).toEqual({
      loading: false,
      loaded: true,
      error: null,
      deferredUntil: "2026-03-17T12:05:00.000Z",
    });
  });

  it("marks the request as loaded with an IPC error when invoke fails", async () => {
    const store = makePersistedStore();
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mockLoad.mockResolvedValueOnce(store);
    mockInvoke.mockRejectedValueOnce(new Error("backend unavailable"));

    const { fetchRateLimits, rateLimitsRequestState } = await loadRateLimitStore();

    await fetchRateLimits("codex");

    expect(get(rateLimitsRequestState)).toEqual({
      loading: false,
      loaded: true,
      error: "backend unavailable",
      deferredUntil: null,
    });
    expect(errorSpy).toHaveBeenCalled();
  });
});
