import { describe, expect, it } from "vitest";
import type { UsagePayload } from "../types/index.js";
import { setDeviceIncludeFlag, setSshHostIncludeFlag } from "./deviceStats.js";

function makePayload(overrides: Partial<UsagePayload> = {}): UsagePayload {
  return {
    total_cost: 1,
    total_tokens: 100,
    session_count: 1,
    input_tokens: 60,
    output_tokens: 40,
    chart_buckets: [],
    model_breakdown: [],
    active_block: null,
    five_hour_cost: 0,
    last_updated: "2026-03-29T12:00:00.000Z",
    from_cache: false,
    usage_source: "parser",
    usage_warning: null,
    period_label: "Today",
    has_earlier_data: false,
    change_stats: null,
    subagent_stats: null,
    device_breakdown: null,
    device_chart_buckets: null,
    ...overrides,
  };
}

describe("setDeviceIncludeFlag", () => {
  it("updates only the targeted device flag", () => {
    const payload = makePayload({
      device_breakdown: [
        {
          device: "Local",
          total_cost: 1,
          total_tokens: 100,
          model_breakdown: [],
          is_local: true,
          status: "online",
          last_synced: null,
          error_message: null,
          cost_percentage: 50,
          include_in_stats: false,
        },
        {
          device: "remote-a",
          total_cost: 1,
          total_tokens: 100,
          model_breakdown: [],
          is_local: false,
          status: "online",
          last_synced: null,
          error_message: null,
          cost_percentage: 50,
          include_in_stats: false,
        },
      ],
    });

    const updated = setDeviceIncludeFlag(payload, "remote-a", true);

    expect(updated?.device_breakdown?.[0]?.include_in_stats).toBe(false);
    expect(updated?.device_breakdown?.[1]?.include_in_stats).toBe(true);
  });

  it("returns the original payload when no device breakdown exists", () => {
    const payload = makePayload();
    expect(setDeviceIncludeFlag(payload, "remote-a", true)).toBe(payload);
  });
});

describe("setSshHostIncludeFlag", () => {
  it("updates only the matching SSH host", () => {
    const hosts = [
      { alias: "remote-a", enabled: true, include_in_stats: false },
      { alias: "remote-b", enabled: true, include_in_stats: true },
    ];

    expect(setSshHostIncludeFlag(hosts, "remote-a", true)).toEqual([
      { alias: "remote-a", enabled: true, include_in_stats: true },
      { alias: "remote-b", enabled: true, include_in_stats: true },
    ]);
  });
});
