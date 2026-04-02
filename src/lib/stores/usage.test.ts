import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import type { UsagePayload } from "../types/index.js";

const mockInvoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

vi.mock("../uiStability.js", () => ({
  isResizeDebugEnabled: () => false,
  logResizeDebug: vi.fn(),
  formatDebugError: (e: unknown) => ({ message: String(e) }),
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

function makePayload(overrides: Partial<UsagePayload> = {}): UsagePayload {
  return {
    total_cost: 1.23,
    total_tokens: 5000,
    session_count: 3,
    input_tokens: 3000,
    output_tokens: 2000,
    chart_buckets: [],
    model_breakdown: [],
    active_block: null,
    five_hour_cost: 0.5,
    last_updated: "2026-03-16T00:00:00.000Z",
    from_cache: false,
    period_label: "Today",
    has_earlier_data: false,
    change_stats: null,
    subagent_stats: null,
    usage_source: "parser",
    usage_warning: null,
    device_breakdown: null,
    device_chart_buckets: null,
    ...overrides,
  };
}

async function loadUsageModule() {
  return import("./usage.js");
}

beforeEach(() => {
  vi.resetModules();
  vi.useRealTimers();
  mockInvoke.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("fetchData", () => {
  it("replaces unrelated stale UI data with an empty payload during a cold fetch", async () => {
    const { usageData, isLoading, fetchData } = await loadUsageModule();
    usageData.set(makePayload({ total_cost: 99, period_label: "Wrong view" }));

    const request = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(request.promise);

    const fetchPromise = fetchData("codex", "day");

    expect(get(usageData)).toEqual(
      expect.objectContaining({
        total_cost: 0,
        total_tokens: 0,
        session_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        chart_buckets: [],
        model_breakdown: [],
        active_block: null,
        five_hour_cost: 0,
        from_cache: false,
        period_label: "",
        has_earlier_data: false,
      }),
    );
    expect(get(isLoading)).toBe(true);

    const fresh = makePayload({ total_cost: 8.75, period_label: "March 16, 2026" });
    request.resolve(fresh);
    await fetchPromise;

    expect(mockInvoke).toHaveBeenCalledWith("get_usage_data", {
      provider: "codex",
      period: "day",
      offset: 0,
    });
    expect(get(usageData)).toEqual(fresh);
    expect(get(isLoading)).toBe(false);
  });

  it("serves a warm cache entry synchronously and refreshes it in the background", async () => {
    const { usageData, isLoading, fetchData } = await loadUsageModule();
    const cached = makePayload({ total_cost: 1.0 });
    mockInvoke.mockResolvedValueOnce(cached);
    await fetchData("claude", "day");

    const refresh = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(refresh.promise);

    const fetchPromise = fetchData("claude", "day");

    expect(get(usageData)).toEqual(cached);
    expect(get(isLoading)).toBe(false);

    await fetchPromise;
    expect(get(usageData)).toEqual(cached);

    const fresh = makePayload({ total_cost: 2.0 });
    refresh.resolve(fresh);

    await vi.waitFor(() => {
      expect(get(usageData)).toEqual(fresh);
    });
    expect(get(isLoading)).toBe(false);
  });

  it("clears a stale blocking loader when a newer view resolves from the warm cache path", async () => {
    const { usageData, isLoading, fetchData } = await loadUsageModule();
    const warmCached = makePayload({ total_cost: 3.2, period_label: "This week" });
    mockInvoke.mockResolvedValueOnce(warmCached);
    await fetchData("all", "week");

    const slow = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(slow.promise);
    const firstCall = fetchData("all", "5h");
    expect(get(isLoading)).toBe(true);

    const backgroundRefresh = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(backgroundRefresh.promise);
    const secondCall = fetchData("all", "week");

    expect(get(usageData)).toEqual(warmCached);
    expect(get(isLoading)).toBe(false);

    await secondCall;

    backgroundRefresh.resolve(makePayload({ total_cost: 4.1, period_label: "This week" }));
    await vi.waitFor(() => {
      expect(get(usageData)).toEqual(expect.objectContaining({ total_cost: 4.1 }));
    });

    slow.resolve(makePayload({ total_cost: 9.9, period_label: "Wrong result" }));
    await firstCall;

    expect(get(isLoading)).toBe(false);
  });

  it("shows expired cache data while refetching and then replaces it", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-16T12:00:00.000Z"));

    const { usageData, isLoading, fetchData } = await loadUsageModule();
    const cached = makePayload({ total_cost: 4.0 });
    mockInvoke.mockResolvedValueOnce(cached);
    await fetchData("claude", "week");

    vi.setSystemTime(new Date("2026-03-16T12:05:01.000Z"));

    const refresh = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(refresh.promise);

    const fetchPromise = fetchData("claude", "week");

    expect(get(usageData)).toEqual(cached);
    expect(get(isLoading)).toBe(true);

    const fresh = makePayload({ total_cost: 7.5 });
    refresh.resolve(fresh);
    await fetchPromise;

    expect(get(usageData)).toEqual(fresh);
    expect(get(isLoading)).toBe(false);
  });

  it("keeps expired cache data visible when a refresh fails", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-16T12:00:00.000Z"));

    const { usageData, isLoading, fetchData } = await loadUsageModule();
    const cached = makePayload({ total_cost: 4.0 });
    mockInvoke.mockResolvedValueOnce(cached);
    await fetchData("claude", "month");

    vi.setSystemTime(new Date("2026-03-16T12:05:01.000Z"));
    mockInvoke.mockRejectedValueOnce(new Error("backend unavailable"));

    await fetchData("claude", "month");

    expect(get(usageData)).toEqual(cached);
    expect(get(isLoading)).toBe(false);
  });

  it("dedupes concurrent IPC invokes for the same provider/period/offset", async () => {
    const { usageData, isLoading, fetchData } = await loadUsageModule();
    const shared = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(shared.promise);

    const first = fetchData("claude", "day");
    const second = fetchData("claude", "day");

    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(mockInvoke).toHaveBeenCalledWith("get_usage_data", {
      provider: "claude",
      period: "day",
      offset: 0,
    });

    const resolved = makePayload({ total_cost: 3.33, period_label: "Today" });
    shared.resolve(resolved);
    await Promise.all([first, second]);

    expect(get(usageData)).toEqual(resolved);
    expect(get(isLoading)).toBe(false);
  });

  it("ignores stale responses from earlier requests", async () => {
    const { usageData, isLoading, fetchData } = await loadUsageModule();
    const slow = deferred<UsagePayload>();
    const fast = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(slow.promise).mockReturnValueOnce(fast.promise);

    const firstCall = fetchData("claude", "5h");
    const secondCall = fetchData("claude", "week");

    const latest = makePayload({ total_cost: 9.5, period_label: "This week" });
    fast.resolve(latest);
    await secondCall;

    expect(get(usageData)).toEqual(latest);
    expect(get(isLoading)).toBe(false);

    slow.resolve(makePayload({ total_cost: 1.0, period_label: "Last 5 hours" }));
    await firstCall;

    expect(get(usageData)).toEqual(latest);
    expect(get(isLoading)).toBe(false);
  });

  it("caches stale responses even when they no longer apply to the UI", async () => {
    const { usageData, fetchData } = await loadUsageModule();
    const slow = deferred<UsagePayload>();
    const fast = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(slow.promise).mockReturnValueOnce(fast.promise);

    const firstCall = fetchData("claude", "5h");
    const secondCall = fetchData("claude", "week");

    const visiblePayload = makePayload({ total_cost: 9.5, period_label: "This week" });
    fast.resolve(visiblePayload);
    await secondCall;

    const cachedOnlyPayload = makePayload({ total_cost: 1.0, period_label: "Last 5 hours" });
    slow.resolve(cachedOnlyPayload);
    await firstCall;

    expect(get(usageData)).toEqual(visiblePayload);

    const backgroundRefresh = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(backgroundRefresh.promise);

    const thirdCall = fetchData("claude", "5h");

    expect(get(usageData)).toEqual(cachedOnlyPayload);
    await thirdCall;

    backgroundRefresh.resolve(makePayload({ total_cost: 1.2, period_label: "Last 5 hours" }));
    await vi.waitFor(() => {
      expect(get(usageData)).toEqual(
        expect.objectContaining({ total_cost: 1.2, period_label: "Last 5 hours" }),
      );
    });
  });
});

describe("warmCache", () => {
  it("fills the frontend cache without mutating the UI and makes the next fetch synchronous", async () => {
    const { usageData, warmCache, fetchData } = await loadUsageModule();
    const warmed = makePayload({ total_cost: 6.4 });
    const warmRequest = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(warmRequest.promise);

    warmCache("claude", "month");

    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("get_usage_data", {
        provider: "claude",
        period: "month",
        offset: 0,
      });
    });
    expect(get(usageData)).toBeNull();

    warmRequest.resolve(warmed);
    await Promise.resolve();

    const refresh = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(refresh.promise);

    const fetchPromise = fetchData("claude", "month");

    expect(get(usageData)).toEqual(warmed);
    await fetchPromise;

    const fresh = makePayload({ total_cost: 7.1 });
    refresh.resolve(fresh);

    await vi.waitFor(() => {
      expect(get(usageData)).toEqual(fresh);
    });
  });

  it("does not let a cleared cache be repopulated by an older in-flight warm request", async () => {
    const { clearUsageCache, warmCache, fetchData, usageData, isLoading } = await loadUsageModule();
    const warmRequest = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(warmRequest.promise);

    warmCache("claude", "month");
    clearUsageCache();

    warmRequest.resolve(makePayload({ total_cost: 6.4 }));
    await Promise.resolve();

    const coldRequest = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(coldRequest.promise);

    const fetchPromise = fetchData("claude", "month");

    expect(get(isLoading)).toBe(true);
    expect(get(usageData)).toEqual(
      expect.objectContaining({
        total_cost: 0,
        total_tokens: 0,
        session_count: 0,
      }),
    );

    coldRequest.resolve(makePayload({ total_cost: 7.1 }));
    await fetchPromise;

    expect(get(usageData)).toEqual(expect.objectContaining({ total_cost: 7.1 }));
  });
});

describe("selective cache invalidation", () => {
  it("clears only the targeted providers and keeps unrelated warm caches", async () => {
    const { usageData, isLoading, fetchData, clearUsageCacheForProviders } = await loadUsageModule();
    const claude = makePayload({ total_cost: 6.4, period_label: "Claude month" });
    const codex = makePayload({ total_cost: 2.5, period_label: "Codex month" });
    mockInvoke.mockResolvedValueOnce(claude);
    await fetchData("claude", "month");
    mockInvoke.mockResolvedValueOnce(codex);
    await fetchData("codex", "month");

    clearUsageCacheForProviders(["claude"]);

    const codexRefresh = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(codexRefresh.promise);
    const codexFetchPromise = fetchData("codex", "month");

    expect(get(usageData)).toEqual(codex);
    expect(get(isLoading)).toBe(false);
    await codexFetchPromise;

    const claudeRefetch = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(claudeRefetch.promise);
    const claudeFetchPromise = fetchData("claude", "month");

    expect(get(isLoading)).toBe(true);
    expect(get(usageData)).toEqual(
      expect.objectContaining({
        total_cost: 0,
        total_tokens: 0,
        session_count: 0,
      }),
    );

    claudeRefetch.resolve(claude);
    await claudeFetchPromise;
    codexRefresh.resolve(codex);
  });

  it("allows the current view to be re-seeded after a targeted invalidation", async () => {
    const { usageData, isLoading, fetchData, clearUsageCacheForProviders, seedUsageCache } =
      await loadUsageModule();
    const initial = makePayload({ total_cost: 4.2, period_label: "Today" });
    mockInvoke.mockResolvedValueOnce(initial);
    await fetchData("claude", "day");

    clearUsageCacheForProviders(["claude"]);

    const reseeded = makePayload({ total_cost: 8.8, period_label: "Today" });
    seedUsageCache("claude", "day", 0, reseeded);

    const refresh = deferred<UsagePayload>();
    mockInvoke.mockReturnValueOnce(refresh.promise);
    const fetchPromise = fetchData("claude", "day");

    expect(get(usageData)).toEqual(reseeded);
    expect(get(isLoading)).toBe(false);
    await fetchPromise;

    refresh.resolve(reseeded);
  });
});

describe("shallowPayloadEqual", () => {
  it("returns true for structurally identical payloads", async () => {
    const { shallowPayloadEqual } = await loadUsageModule();
    const a = makePayload();
    const b = makePayload();
    expect(shallowPayloadEqual(a, b)).toBe(true);
  });

  it("returns false when a key numeric field differs", async () => {
    const { shallowPayloadEqual } = await loadUsageModule();
    const a = makePayload({ total_cost: 10.02 });
    const b = makePayload({ total_cost: 0.85 });
    expect(shallowPayloadEqual(a, b)).toBe(false);
  });

  it("returns false when chart_buckets length differs", async () => {
    const { shallowPayloadEqual } = await loadUsageModule();
    const a = makePayload({ chart_buckets: [] });
    const b = makePayload({
      chart_buckets: [{ label: "12:00", sort_key: "2026-03-16T12:00:00", total: 1.0, segments: [] }],
    });
    expect(shallowPayloadEqual(a, b)).toBe(false);
  });

  it("returns false when period_label differs", async () => {
    const { shallowPayloadEqual } = await loadUsageModule();
    const a = makePayload({ period_label: "Today" });
    const b = makePayload({ period_label: "March 16, 2026" });
    expect(shallowPayloadEqual(a, b)).toBe(false);
  });

  it("returns false when model breakdown costs differ", async () => {
    const { shallowPayloadEqual } = await loadUsageModule();
    const a = makePayload({
      model_breakdown: [{ display_name: "Sonnet", model_key: "sonnet-4-6", cost: 1, tokens: 100, change_stats: null }],
    });
    const b = makePayload({
      model_breakdown: [{ display_name: "Sonnet", model_key: "sonnet-4-6", cost: 2, tokens: 100, change_stats: null }],
    });
    expect(shallowPayloadEqual(a, b)).toBe(false);
  });
});

describe("fetchData — shallow equality dedup", () => {
  it("skips usageData.set when background refresh returns identical data", async () => {
    const { usageData, fetchData } = await loadUsageModule();
    const initial = makePayload({ total_cost: 5.0 });
    mockInvoke.mockResolvedValueOnce(initial);
    await fetchData("claude", "day");

    // Track store updates
    const updates: unknown[] = [];
    const unsub = usageData.subscribe((v) => updates.push(v));
    updates.length = 0;

    // Background refresh returns identical payload (same key fields)
    const identical = makePayload({ total_cost: 5.0 });
    mockInvoke.mockResolvedValueOnce(identical);
    await fetchData("claude", "day");

    // Wait for background refresh to resolve
    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledTimes(2);
    });
    // Give the .then() callback time to run
    await new Promise((r) => setTimeout(r, 0));

    // The cache hit may fire one set (reference check), but the background
    // refresh should NOT fire another set since data is shallowly equal.
    // At most 1 update from the cache-hit path (if reference differs).
    expect(updates.length).toBeLessThanOrEqual(1);
    unsub();
  });
});

describe("warmAllPeriods", () => {
  it("warms every lightweight period except the skipped one", async () => {
    const { warmAllPeriods } = await loadUsageModule();
    mockInvoke.mockResolvedValue(makePayload());

    warmAllPeriods("claude", "day");

    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledTimes(3);
    });

    const calledPeriods = new Set(
      mockInvoke.mock.calls
        .filter(([command]) => command === "get_usage_data")
        .map(([, args]) => (args as { period: string }).period),
    );

    expect(calledPeriods).toEqual(new Set(["5h", "week", "month"]));
  });
});
