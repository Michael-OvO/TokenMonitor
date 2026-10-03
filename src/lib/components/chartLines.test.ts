import { describe, expect, it } from "vitest";
import type { ChartBucket } from "../types/index.js";
import { lineChartPeak, lineSeriesValues } from "./chartLines.js";

function bucket(label: string, segs: Array<[string, number]>): ChartBucket {
  const segments = segs.map(([key, cost]) => ({ model: key, model_key: key, cost, tokens: cost * 1000 }));
  return { label, total: segments.reduce((sum, s) => sum + s.cost, 0), segments };
}

const cost = (s: { cost: number }) => s.cost;

describe("lineSeriesValues", () => {
  it("gives each model its own value, not a running total", () => {
    const buckets = [
      bucket("Sep 1", [["opus", 120], ["sonnet", 40]]),
      bucket("Sep 2", [["opus", 450], ["gpt", 300], ["sonnet", 150]]),
      bucket("Sep 3", [["gpt", 80]]),
    ];

    expect(lineSeriesValues(buckets, ["opus", "gpt", "sonnet"], cost)).toEqual([
      [120, 450, 0],
      [0, 300, 80],
      [40, 150, 0],
    ]);
  });

  it("sums segments that share a key, as local and remote devices do", () => {
    const buckets = [bucket("a", [["opus", 2], ["opus", 1.5]])];

    expect(lineSeriesValues(buckets, ["opus"], cost)).toEqual([[3.5]]);
  });

  it("reads whichever metric it is given", () => {
    const buckets = [bucket("a", [["opus", 2], ["gpt", 1]])];

    expect(lineSeriesValues(buckets, ["opus", "gpt"], (s) => s.tokens)).toEqual([[2000], [1000]]);
  });
});

describe("lineChartPeak", () => {
  // A $900 day where no one model passes $430.
  const series = [
    [120, 430, 0],
    [0, 310, 80],
    [40, 160, 0],
  ];
  const totals = [160, 900, 80];

  it("reaches the busiest bucket's total while the total line shows", () => {
    expect(lineChartPeak(series, totals, true)).toBe(900);
  });

  it("refits to the busiest model once the total line is hidden", () => {
    expect(lineChartPeak(series, totals, false)).toBe(430);
  });

  it("is zero with nothing to draw", () => {
    expect(lineChartPeak([], [], true)).toBe(0);
    expect(lineChartPeak([], [], false)).toBe(0);
  });
});
