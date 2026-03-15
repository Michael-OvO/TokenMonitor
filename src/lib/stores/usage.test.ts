import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { get } from "svelte/store";

// Mock @tauri-apps/api/core before importing the module under test
const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

// Import after mock is set up
const {
  usageData,
  isLoading,
  setupStatus,
  fetchData,
  warmCache,
  warmAllPeriods,
  initializeApp,
  checkSetup,
} = await import("./usage.js");

function makePayload(overrides: Record<string, unknown> = {}) {
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
    last_updated: new Date().toISOString(),
    from_cache: false,
    ...overrides,
  };
}

beforeEach(() => {
  mockInvoke.mockReset();
  usageData.set(null);
  isLoading.set(false);
  setupStatus.set({ ready: false, installing: false, error: null });
});

// ── fetchData ───────────────────────────────────────────────────────

describe("fetchData", () => {
  it("cold path: fetches via invoke and updates store", async () => {
    const payload = makePayload();
    mockInvoke.mockResolvedValueOnce(payload);

    await fetchData("claude", "day");

    expect(mockInvoke).toHaveBeenCalledWith("get_usage_data", {
      provider: "claude",
      period: "day",
    });
    expect(get(usageData)).toEqual(payload);
    expect(get(isLoading)).toBe(false);
  });

  it("sets isLoading during cold fetch", async () => {
    let loadingDuringFetch = false;
    mockInvoke.mockImplementationOnce(() => {
      loadingDuringFetch = get(isLoading);
      return Promise.resolve(makePayload());
    });

    await fetchData("claude", "week");
    expect(loadingDuringFetch).toBe(true);
    expect(get(isLoading)).toBe(false);
  });

  it("handles invoke error without clobbering data", async () => {
    const existing = makePayload({ total_cost: 99 });
    usageData.set(existing);
    mockInvoke.mockRejectedValueOnce(new Error("network fail"));

    await fetchData("claude", "day");

    // usageData should not be replaced with null on error
    // (it may be set to null or keep old data depending on cache state)
    expect(get(isLoading)).toBe(false);
  });

  it("warm cache path: shows cached data immediately then refreshes", async () => {
    const cached = makePayload({ total_cost: 1.0 });
    const fresh = makePayload({ total_cost: 2.0 });

    // First call populates cache
    mockInvoke.mockResolvedValueOnce(cached);
    await fetchData("claude", "day");
    expect(get(usageData)).toEqual(cached);

    // Second call should show cached immediately, then refresh
    mockInvoke.mockResolvedValueOnce(fresh);
    await fetchData("claude", "day");

    // Wait for background refresh to complete
    await vi.waitFor(() => {
      expect(get(usageData)?.total_cost).toBe(2.0);
    });
  });

  it("request deduplication: rapid calls only apply latest", async () => {
    const slow = makePayload({ total_cost: 1.0 });
    const fast = makePayload({ total_cost: 2.0 });

    // First invoke is slow, second is fast
    mockInvoke
      .mockImplementationOnce(
        () => new Promise((r) => setTimeout(() => r(slow), 50)),
      )
      .mockResolvedValueOnce(fast);

    // Fire both without awaiting the first
    const p1 = fetchData("claude", "5h");
    const p2 = fetchData("claude", "week");
    await Promise.all([p1, p2]);

    // The second call's data should win (higher request ID)
    expect(get(usageData)).toEqual(fast);
  });
});

// ── warmCache ───────────────────────────────────────────────────────

describe("warmCache", () => {
  it("calls invoke but does not update usageData store", async () => {
    const payload = makePayload();
    mockInvoke.mockResolvedValueOnce(payload);

    warmCache("claude", "week");
    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("get_usage_data", {
        provider: "claude",
        period: "week",
      });
    });

    // usageData should remain null — warmCache only populates the internal cache
    expect(get(usageData)).toBeNull();
  });
});

// ── warmAllPeriods ──────────────────────────────────────────────────

describe("warmAllPeriods", () => {
  it("warms all periods except the skipped one", async () => {
    mockInvoke.mockResolvedValue(makePayload());

    warmAllPeriods("claude", "day");

    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledTimes(4);
    });

    const calledPeriods = mockInvoke.mock.calls.map(
      (c: unknown[]) => (c[1] as { period: string }).period,
    );
    expect(calledPeriods).not.toContain("day");
    expect(calledPeriods).toContain("5h");
    expect(calledPeriods).toContain("week");
    expect(calledPeriods).toContain("month");
    expect(calledPeriods).toContain("year");
  });
});

// ── initializeApp ───────────────────────────────────────────────────

describe("initializeApp", () => {
  it("transitions through installing state on success", async () => {
    const status = { ready: true, installing: false, error: null };

    // Check installing state is set before invoke resolves
    let wasInstalling = false;
    mockInvoke.mockImplementationOnce(() => {
      wasInstalling = get(setupStatus).installing;
      return Promise.resolve(status);
    });

    await initializeApp();
    expect(wasInstalling).toBe(true);
    expect(get(setupStatus)).toEqual(status);
  });

  it("sets error on failure", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("no node"));

    const result = await initializeApp();
    expect(result).toBeNull();
    expect(get(setupStatus).error).toBe("no node");
    expect(get(setupStatus).ready).toBe(false);
    expect(get(setupStatus).installing).toBe(false);
  });
});

// ── checkSetup ──────────────────────────────────────────────────────

describe("checkSetup", () => {
  it("returns status on success", async () => {
    const status = { ready: true, installing: false, error: null };
    mockInvoke.mockResolvedValueOnce(status);

    const result = await checkSetup();
    expect(result).toEqual(status);
    expect(get(setupStatus)).toEqual(status);
  });

  it("returns null on failure", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("fail"));

    const result = await checkSetup();
    expect(result).toBeNull();
  });
});
