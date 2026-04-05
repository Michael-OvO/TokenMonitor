import { describe, expect, it } from "vitest";

import type { ChartBucket } from "../types/index.js";
import { filterVisibleChartBuckets, getXAxisLabels, shouldHideFutureBucket } from "./chartBuckets.js";

function bucket(label: string, sortKey?: string): ChartBucket {
  return {
    label,
    sort_key: sortKey,
    total: 0,
    segments: [],
  };
}

describe("chartBuckets", () => {
  it("hides future hourly buckets for the live day view", () => {
    const now = new Date(2026, 3, 5, 11, 30, 0);
    const buckets = [
      bucket("8AM", "08"),
      bucket("11AM", "11"),
      bucket("12PM", "12"),
      bucket("2PM", "14"),
    ];

    expect(filterVisibleChartBuckets(buckets, "day", 0, now).map((entry) => entry.label)).toEqual([
      "8AM",
      "11AM",
    ]);
  });

  it("hides future daily buckets for the live month view", () => {
    const now = new Date(2026, 3, 5, 11, 30, 0);
    const buckets = [
      bucket("Apr 1", "2026-04-01"),
      bucket("Apr 5", "2026-04-05"),
      bucket("Apr 6", "2026-04-06"),
      bucket("Apr 12", "2026-04-12"),
    ];

    expect(filterVisibleChartBuckets(buckets, "month", 0, now).map((entry) => entry.label)).toEqual([
      "Apr 1",
      "Apr 5",
    ]);
  });

  it("hides future monthly buckets for the live year view", () => {
    const now = new Date(2026, 3, 5, 11, 30, 0);
    const buckets = [
      bucket("Jan", "2026-01"),
      bucket("Apr", "2026-04"),
      bucket("May", "2026-05"),
      bucket("Dec", "2026-12"),
    ];

    expect(filterVisibleChartBuckets(buckets, "year", 0, now).map((entry) => entry.label)).toEqual([
      "Jan",
      "Apr",
    ]);
  });

  it("hides future rolling buckets for the live 5h view", () => {
    const now = new Date("2026-04-05T11:30:00-04:00");
    const buckets = [
      bucket("9:00", "2026-04-05T09:00:00-04:00"),
      bucket("11:00", "2026-04-05T11:00:00-04:00"),
      bucket("12:00", "2026-04-05T12:00:00-04:00"),
    ];

    expect(filterVisibleChartBuckets(buckets, "5h", 0, now).map((entry) => entry.label)).toEqual([
      "9:00",
      "11:00",
    ]);
  });

  it("keeps future-looking buckets for older offsets and unknown sort keys", () => {
    const now = new Date(2026, 3, 5, 11, 30, 0);
    const buckets = [
      bucket("Apr 5", "2026-04-05"),
      bucket("Apr 20", "2026-04-20"),
      bucket("Other"),
    ];

    expect(filterVisibleChartBuckets(buckets, "month", 1, now).map((entry) => entry.label)).toEqual([
      "Apr 5",
      "Apr 20",
      "Other",
    ]);
    expect(shouldHideFutureBucket("not-a-date", "month", 0, now)).toBe(false);
  });

  it("derives axis labels from the visible bucket list", () => {
    const visibleBuckets = [
      bucket("Apr 1"),
      bucket("Apr 2"),
      bucket("Apr 3"),
      bucket("Apr 4"),
      bucket("Apr 5"),
      bucket("Apr 6"),
    ];

    expect(getXAxisLabels(visibleBuckets)).toEqual(["Apr 1", "Apr 4", "Apr 6"]);
    expect(getXAxisLabels([bucket("Apr 1"), bucket("Apr 5")])).toEqual(["Apr 1", "Apr 5"]);
    expect(getXAxisLabels([bucket("Apr 5")])).toEqual(["Apr 5"]);
  });
});
