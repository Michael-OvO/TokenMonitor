import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import type { UsagePayload } from "../types/index.js";

const mockLoad = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/plugin-store", () => ({ load: mockLoad }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../utils/logger.js", () => ({
  logger: { error: vi.fn(), warn: vi.fn(), info: vi.fn(), debug: vi.fn() },
}));

function makeStore() {
  const values = new Map<string, unknown>();
  return {
    values,
    get: vi.fn(async (key: string) => values.get(key) ?? null),
    set: vi.fn(async (key: string, value: unknown) => {
      values.set(key, value);
    }),
    save: vi.fn().mockResolvedValue(undefined),
  };
}

function makePayload(overrides: Partial<UsagePayload> = {}): UsagePayload {
  return {
    total_cost: 4.2,
    total_tokens: 100,
    session_count: 1,
    input_tokens: 60,
    output_tokens: 40,
    cache_read_tokens: 0,
    cache_write_5m_tokens: 0,
    cache_write_1h_tokens: 0,
    web_search_requests: 0,
    chart_buckets: [],
    model_breakdown: [],
    active_block: null,
    five_hour_cost: 0,
    last_updated: "2026-11-16T10:00:00.000Z",
    from_cache: false,
    period_label: "Today",
    has_earlier_data: false,
    change_stats: null,
    subagent_stats: null,
    usage_source: "parser",
    usage_warning: null,
    device_breakdown: null,
    device_chart_buckets: null,
    provider_detected: null,
    ...overrides,
  };
}

async function loadModules() {
  const snapshot = await import("./usageSnapshot.js");
  const { settings } = await import("./settings.js");
  return { ...snapshot, settings: get(settings) };
}

let store: ReturnType<typeof makeStore>;

beforeEach(() => {
  vi.resetModules();
  store = makeStore();
  mockLoad.mockReset();
  mockLoad.mockResolvedValue(store);
});

describe("usage snapshot", () => {
  it("reads back what it wrote under the same key, marked cached", async () => {
    const { usageSnapshotKey, writeUsageSnapshot, readUsageSnapshot, settings } = await loadModules();
    const key = usageSnapshotKey("all", "day", settings);
    const now = new Date(2026, 10, 16, 10, 0).getTime();

    await writeUsageSnapshot(key, "day", makePayload(), now);
    const read = await readUsageSnapshot(key, now + 60_000);

    expect(read).toEqual(makePayload({ from_cache: true }));
  });

  it("skips a repeat write of the same payload", async () => {
    const { usageSnapshotKey, writeUsageSnapshot, settings } = await loadModules();
    const key = usageSnapshotKey("all", "day", settings);

    await writeUsageSnapshot(key, "day", makePayload());
    await writeUsageSnapshot(key, "day", makePayload());
    expect(store.save).toHaveBeenCalledTimes(1);

    await writeUsageSnapshot(key, "day", makePayload({ last_updated: "2026-11-16T10:05:00.000Z" }));
    expect(store.save).toHaveBeenCalledTimes(2);
  });

  it("keys by scope, currency, hidden models and header tabs", async () => {
    const { usageSnapshotKey, settings } = await loadModules();
    const { createDefaultHeaderTabs } = await import("../providerMetadata.js");
    const base = usageSnapshotKey("all", "day", settings);

    const tabs = createDefaultHeaderTabs();
    tabs.codex = { ...tabs.codex, enabled: false };
    expect(usageSnapshotKey("all", "day", { ...settings, headerTabs: tabs })).not.toBe(base);
    expect(usageSnapshotKey("all", "day", { ...settings, currency: "EUR" })).not.toBe(base);
    expect(usageSnapshotKey("all", "day", { ...settings, hiddenModels: ["opus"] })).not.toBe(base);
    expect(usageSnapshotKey("all", "week", settings)).not.toBe(base);
    expect(usageSnapshotKey("claude", "day", settings)).not.toBe(base);
    // Hidden-model order does not matter.
    expect(usageSnapshotKey("all", "day", { ...settings, hiddenModels: ["a", "b"] })).toBe(
      usageSnapshotKey("all", "day", { ...settings, hiddenModels: ["b", "a"] }),
    );
  });

  it("returns null for another key or a snapshot from an earlier period", async () => {
    const { usageSnapshotKey, writeUsageSnapshot, readUsageSnapshot, settings } = await loadModules();
    const key = usageSnapshotKey("all", "day", settings);
    const saved = new Date(2026, 10, 16, 23, 0).getTime();
    await writeUsageSnapshot(key, "day", makePayload(), saved);

    expect(await readUsageSnapshot(usageSnapshotKey("all", "day", { ...settings, currency: "EUR" }), saved)).toBeNull();
    expect(await readUsageSnapshot(key, new Date(2026, 10, 17, 8, 0).getTime())).toBeNull();
  });

  it("never throws when the store is unavailable", async () => {
    mockLoad.mockRejectedValue(new Error("no store"));
    const { writeUsageSnapshot, readUsageSnapshot } = await loadModules();

    await expect(writeUsageSnapshot("k", "day", makePayload())).resolves.toBeUndefined();
    await expect(readUsageSnapshot("k")).resolves.toBeNull();
  });
});

describe("isSnapshotCurrent", () => {
  it("accepts only the period the snapshot still describes", async () => {
    const { isSnapshotCurrent } = await loadModules();
    const now = new Date(2026, 10, 16, 12, 0).getTime();
    const minutesAgo = (m: number) => now - m * 60_000;

    expect(isSnapshotCurrent("5h", minutesAgo(30), now)).toBe(true);
    expect(isSnapshotCurrent("5h", minutesAgo(90), now)).toBe(false);
    expect(isSnapshotCurrent("day", new Date(2026, 10, 16, 1, 0).getTime(), now)).toBe(true);
    expect(isSnapshotCurrent("day", new Date(2026, 10, 15, 23, 0).getTime(), now)).toBe(false);
    expect(isSnapshotCurrent("month", new Date(2026, 10, 1).getTime(), now)).toBe(true);
    expect(isSnapshotCurrent("month", new Date(2026, 9, 31).getTime(), now)).toBe(false);
    expect(isSnapshotCurrent("year", new Date(2026, 0, 2).getTime(), now)).toBe(true);
    expect(isSnapshotCurrent("day", now + 60_000, now)).toBe(false);
  });
});

describe("isDefaultView", () => {
  it("matches the default tab and period at offset 0 only", async () => {
    const { isDefaultView, settings } = await loadModules();
    const tab = settings.defaultProvider;
    const period = settings.defaultPeriod;

    expect(isDefaultView(tab, period, 0, settings)).toBe(true);
    expect(isDefaultView(tab, period, -1, settings)).toBe(false);
    expect(isDefaultView(tab, period === "day" ? "week" : "day", 0, settings)).toBe(false);
  });
});
