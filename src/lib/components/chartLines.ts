import type { ChartBucket } from "../types/index.js";

type Segment = ChartBucket["segments"][number];

/**
 * Each series' own value per bucket, one row per key. The line chart overlaps
 * its lines rather than stacking them, so a line's height is that model's
 * spend alone. Segments sharing a key (one model on local and remote devices)
 * are summed.
 */
export function lineSeriesValues(
  buckets: ChartBucket[],
  keys: string[],
  value: (s: Segment) => number,
): number[][] {
  return keys.map((key) =>
    buckets.map((b) =>
      b.segments.reduce((sum, s) => (s.model_key === key ? sum + value(s) : sum), 0),
    ),
  );
}

/**
 * The tallest value the line chart draws, which its y-axis has to reach: the
 * busiest bucket's total while the total line shows, else the busiest model.
 */
export function lineChartPeak(series: number[][], totals: number[], showTotal: boolean): number {
  const drawn = showTotal ? totals : series.flat();
  return drawn.reduce((peak, v) => Math.max(peak, v), 0);
}
