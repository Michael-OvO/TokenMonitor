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

function makeStore(saved: Record<string, unknown> = {}) {
  const values = new Map(Object.entries(saved));
  return {
    get: vi.fn(async (key: string) => values.get(key) ?? null),
    set: vi.fn(async (key: string, value: unknown) => {
      values.set(key, value);
    }),
    save: vi.fn().mockResolvedValue(undefined),
  };
}

function installStoreMap(stores: Record<string, ReturnType<typeof makeStore>>) {
  mockLoad.mockImplementation(async (file: string) => {
    const store = stores[file];
    if (!store) {
      throw new Error(`Unexpected store load for ${file}`);
    }
    return store;
  });
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
  it("hydrates provider-specific stores and migrates legacy payloads", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-17T12:00:30.000Z"));

    const legacyPayload = makePayload({
      claude: providerRateLimits("claude", {
        windows: [
          {
            windowId: "five_hour",
            label: "Session (5hr)",
            utilization: 33,
            resetsAt: "2026-03-17T14:00:00.000Z",
          },
        ],
        cooldownUntil: "2026-03-17T12:05:00.000Z",
        fetchedAt: "2026-03-17T12:00:00.000Z",
      }),
    });

    const legacyStore = makeStore({ payload: legacyPayload });
    const claudeStore = makeStore();
    const codexStore = makeStore({
      payload: providerRateLimits("codex", {
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
      lastSuccessfulAt: "2026-03-17T11:58:00.000Z",
    });

    installStoreMap({
      "rate-limits.json": legacyStore,
      "rate-limits-claude.json": claudeStore,
      "rate-limits-codex.json": codexStore,
    });

    const { hydrateRateLimits, rateLimitsData, rateLimitsMonitorState, rateLimitsRequestState } =
      await loadRateLimitStore();

    await hydrateRateLimits();

    expect(get(rateLimitsData)).toEqual({
      claude: legacyPayload.claude,
      codex: expect.objectContaining({
        provider: "codex",
      }),
    });
    expect(get(rateLimitsMonitorState).claude.lastSuccessAt).toBe("2026-03-17T12:00:00.000Z");
    expect(get(rateLimitsRequestState).deferredUntil).toBe("2026-03-17T12:05:00.000Z");
    expect(claudeStore.set).toHaveBeenCalledWith(
      "payload",
      expect.objectContaining({ provider: "claude" }),
    );
  });
});

describe("fetchRateLimits", () => {
  it("keeps an existing provider error visible while a retry request is loading", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-17T12:10:00.000Z"));

    const legacyStore = makeStore();
    const claudeStore = makeStore({
      payload: providerRateLimits("claude", {
        windows: [],
        error: "Usage API returned 429",
        cooldownUntil: null,
        fetchedAt: "2026-03-17T11:59:00.000Z",
      }),
      lastSuccessfulAt: "2026-03-17T11:55:00.000Z",
    });
    const codexStore = makeStore();
    installStoreMap({
      "rate-limits.json": legacyStore,
      "rate-limits-claude.json": claudeStore,
      "rate-limits-codex.json": codexStore,
    });

    const request = deferred<RateLimitsPayload>();
    mockInvoke.mockReturnValueOnce(request.promise);

    const { fetchRateLimits, rateLimitsMonitorState, rateLimitsRequestState } =
      await loadRateLimitStore();

    const fetchPromise = fetchRateLimits("claude");

    await vi.waitFor(() => {
      expect(get(rateLimitsMonitorState).claude.loading).toBe(true);
    });

    expect(get(rateLimitsMonitorState).claude.error).toBe("Usage API returned 429");
    expect(get(rateLimitsRequestState).error).toBe("Usage API returned 429");

    request.resolve(
      makePayload({
        claude: providerRateLimits("claude", {
          windows: [
            {
              windowId: "five_hour",
              label: "Session (5hr)",
              utilization: 19,
              resetsAt: "2026-03-17T14:10:00.000Z",
            },
          ],
          error: null,
          fetchedAt: "2026-03-17T12:10:10.000Z",
        }),
        codex: null,
      }),
    );
    await fetchPromise;

    expect(get(rateLimitsMonitorState).claude.error).toBeNull();
    expect(get(rateLimitsRequestState).error).toBeNull();
  });

  it("fetches and persists only the requested provider while keeping provider-level monitor state", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-17T12:01:30.000Z"));

    const legacyStore = makeStore();
    const claudeStore = makeStore();
    const codexStore = makeStore();
    installStoreMap({
      "rate-limits.json": legacyStore,
      "rate-limits-claude.json": claudeStore,
      "rate-limits-codex.json": codexStore,
    });

    const request = deferred<RateLimitsPayload>();
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
        fetchedAt: "2026-03-17T12:01:00.000Z",
      }),
      codex: null,
    });
    mockInvoke.mockReturnValueOnce(request.promise);

    const { fetchRateLimits, rateLimitsData, rateLimitsMonitorState, rateLimitsRequestState } =
      await loadRateLimitStore();

    const fetchPromise = fetchRateLimits("claude");

    await vi.waitFor(() => {
      expect(get(rateLimitsMonitorState).claude.loading).toBe(true);
    });

    request.resolve(payload);
    await fetchPromise;

    expect(mockInvoke).toHaveBeenCalledWith("get_rate_limits", { provider: "claude" });
    expect(get(rateLimitsData)?.claude).toEqual(payload.claude);
    expect(get(rateLimitsMonitorState).claude).toEqual({
      loading: false,
      loaded: true,
      error: null,
      deferredUntil: "2026-03-17T12:06:00.000Z",
      failureStreak: 0,
      lastAttemptAt: "2026-03-17T12:01:00.000Z",
      lastSuccessAt: "2026-03-17T12:01:00.000Z",
    });
    expect(get(rateLimitsRequestState)).toEqual({
      loading: false,
      loaded: true,
      error: null,
      deferredUntil: "2026-03-17T12:06:00.000Z",
    });
    expect(claudeStore.set).toHaveBeenCalledWith(
      "payload",
      expect.objectContaining({ provider: "claude" }),
    );
    expect(codexStore.set).not.toHaveBeenCalled();
  });

  it("defers Claude refreshes during cooldown and retries automatically once the defer window closes", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-17T12:00:00.000Z"));

    const claudeStore = makeStore({
      payload: providerRateLimits("claude", {
        windows: [
          {
            windowId: "five_hour",
            label: "Session (5hr)",
            utilization: 61,
            resetsAt: "2026-03-17T14:00:00.000Z",
          },
        ],
        cooldownUntil: "2026-03-17T12:00:02.000Z",
        fetchedAt: "2026-03-17T12:00:00.000Z",
      }),
      lastSuccessfulAt: "2026-03-17T11:55:00.000Z",
    });
    const codexStore = makeStore();
    const legacyStore = makeStore();
    installStoreMap({
      "rate-limits.json": legacyStore,
      "rate-limits-claude.json": claudeStore,
      "rate-limits-codex.json": codexStore,
    });

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
          fetchedAt: "2026-03-17T12:05:00.000Z",
        }),
        codex: null,
      }),
    );

    const { fetchRateLimits, rateLimitsMonitorState } = await loadRateLimitStore();

    await fetchRateLimits("claude");

    expect(mockInvoke).not.toHaveBeenCalled();
    expect(get(rateLimitsMonitorState).claude.deferredUntil).toBe("2026-03-17T12:05:00.000Z");

    await vi.advanceTimersByTimeAsync(300_100);

    expect(mockInvoke).toHaveBeenCalledWith("get_rate_limits", { provider: "claude" });
    expect(get(rateLimitsMonitorState).claude.lastSuccessAt).toBe("2026-03-17T12:05:00.000Z");
  });

  it("clamps very short retry delays to avoid sub-second error thrash", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-17T12:00:00.000Z"));

    const legacyStore = makeStore();
    const claudeStore = makeStore();
    const codexStore = makeStore({
      payload: providerRateLimits("codex", {
        windows: [],
        error: "No rate limit data in Codex session files",
        cooldownUntil: "2026-03-17T12:00:00.100Z",
        fetchedAt: "2026-03-17T11:58:00.000Z",
      }),
      lastSuccessfulAt: "2026-03-17T11:58:00.000Z",
    });
    installStoreMap({
      "rate-limits.json": legacyStore,
      "rate-limits-claude.json": claudeStore,
      "rate-limits-codex.json": codexStore,
    });

    mockInvoke.mockResolvedValueOnce(
      makePayload({
        claude: null,
        codex: providerRateLimits("codex", {
          windows: [
            {
              windowId: "primary",
              label: "Session (5hr)",
              utilization: 11,
              resetsAt: "2026-03-17T13:00:00.000Z",
            },
          ],
          error: null,
          fetchedAt: "2026-03-17T12:00:01.200Z",
        }),
      }),
    );

    const { fetchRateLimits } = await loadRateLimitStore();

    await fetchRateLimits("codex");
    expect(mockInvoke).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(200);
    expect(mockInvoke).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(900);
    expect(mockInvoke).toHaveBeenCalledWith("get_rate_limits", { provider: "codex" });
  });

  it("fetches only the eligible provider in all-scope mode", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-17T12:00:30.000Z"));

    const claudeStore = makeStore({
      payload: providerRateLimits("claude", {
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
      lastSuccessfulAt: "2026-03-17T12:00:00.000Z",
    });
    const codexStore = makeStore({
      payload: providerRateLimits("codex", {
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
      lastSuccessfulAt: "2026-03-17T11:58:00.000Z",
    });
    const legacyStore = makeStore();
    installStoreMap({
      "rate-limits.json": legacyStore,
      "rate-limits-claude.json": claudeStore,
      "rate-limits-codex.json": codexStore,
    });

    mockInvoke.mockResolvedValueOnce(
      makePayload({
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
      }),
    );

    const { fetchRateLimits, rateLimitsData, rateLimitsMonitorState, rateLimitsRequestState } =
      await loadRateLimitStore();

    await fetchRateLimits("all");

    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(mockInvoke).toHaveBeenCalledWith("get_rate_limits", { provider: "codex" });
    expect(get(rateLimitsData)?.claude).toEqual(expect.objectContaining({ provider: "claude" }));
    expect(get(rateLimitsData)?.codex).toEqual(
      expect.objectContaining({
        provider: "codex",
        fetchedAt: "2026-03-17T12:00:30.000Z",
      }),
    );
    expect(get(rateLimitsMonitorState).claude.deferredUntil).toBe("2026-03-17T12:05:00.000Z");
    expect(get(rateLimitsRequestState).deferredUntil).toBe("2026-03-17T12:05:00.000Z");
  });

  it("suppresses the first transport failure when prior provider data exists", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-17T12:10:00.000Z"));

    const claudeStore = makeStore({
      payload: providerRateLimits("claude", {
        windows: [
          {
            windowId: "five_hour",
            label: "Session (5hr)",
            utilization: 28,
            resetsAt: "2026-03-17T14:00:00.000Z",
          },
        ],
        fetchedAt: "2026-03-17T11:59:00.000Z",
      }),
      lastSuccessfulAt: "2026-03-17T11:59:00.000Z",
    });
    const codexStore = makeStore();
    const legacyStore = makeStore();
    installStoreMap({
      "rate-limits.json": legacyStore,
      "rate-limits-claude.json": claudeStore,
      "rate-limits-codex.json": codexStore,
    });

    mockInvoke.mockRejectedValueOnce(new Error("backend unavailable"));

    const { fetchRateLimits, rateLimitsMonitorState, rateLimitsRequestState } =
      await loadRateLimitStore();

    await fetchRateLimits("claude");

    expect(get(rateLimitsMonitorState).claude.failureStreak).toBe(1);
    expect(get(rateLimitsMonitorState).claude.error).toBeNull();
    expect(get(rateLimitsRequestState).error).toBeNull();
  });
});
