import type { ChartBucket, UsagePeriod } from "../types/index.js";

const HOUR_KEY_RE = /^\d{2}$/;
const DATE_KEY_RE = /^(\d{4})-(\d{2})-(\d{2})$/;
const MONTH_KEY_RE = /^(\d{4})-(\d{2})$/;

function dayStamp(date: Date): number {
  return date.getFullYear() * 10_000 + (date.getMonth() + 1) * 100 + date.getDate();
}

function monthStamp(date: Date): number {
  return date.getFullYear() * 100 + (date.getMonth() + 1);
}

function parseHourKey(sortKey: string): number | null {
  if (!HOUR_KEY_RE.test(sortKey)) return null;

  const hour = Number.parseInt(sortKey, 10);
  return hour >= 0 && hour <= 23 ? hour : null;
}

function parseDayKey(sortKey: string): number | null {
  const match = DATE_KEY_RE.exec(sortKey);
  if (!match) return null;

  const [, yearText, monthText, dayText] = match;
  const year = Number.parseInt(yearText, 10);
  const month = Number.parseInt(monthText, 10);
  const day = Number.parseInt(dayText, 10);

  if (month < 1 || month > 12 || day < 1 || day > 31) return null;

  return year * 10_000 + month * 100 + day;
}

function parseMonthKey(sortKey: string): number | null {
  const match = MONTH_KEY_RE.exec(sortKey);
  if (!match) return null;

  const [, yearText, monthText] = match;
  const year = Number.parseInt(yearText, 10);
  const month = Number.parseInt(monthText, 10);

  if (month < 1 || month > 12) return null;

  return year * 100 + month;
}

function parseDateTimeKey(sortKey: string): number | null {
  const timestamp = Date.parse(sortKey);
  return Number.isNaN(timestamp) ? null : timestamp;
}

export function shouldHideFutureBucket(
  sortKey: string | undefined,
  period: UsagePeriod,
  offset: number,
  now: Date = new Date(),
): boolean {
  if (!sortKey || offset !== 0) return false;

  switch (period) {
    case "5h": {
      const bucketTime = parseDateTimeKey(sortKey);
      return bucketTime != null ? bucketTime > now.getTime() : false;
    }
    case "day": {
      const bucketHour = parseHourKey(sortKey);
      return bucketHour != null ? bucketHour > now.getHours() : false;
    }
    case "week":
    case "month": {
      const bucketDay = parseDayKey(sortKey);
      return bucketDay != null ? bucketDay > dayStamp(now) : false;
    }
    case "year": {
      const bucketMonth = parseMonthKey(sortKey);
      return bucketMonth != null ? bucketMonth > monthStamp(now) : false;
    }
  }
}

export function filterVisibleChartBuckets(
  buckets: ChartBucket[],
  period: UsagePeriod,
  offset: number,
  now: Date = new Date(),
): ChartBucket[] {
  return buckets.filter((bucket) => !shouldHideFutureBucket(bucket.sort_key, period, offset, now));
}

export function getXAxisLabels(buckets: Array<Pick<ChartBucket, "label">>): string[] {
  if (buckets.length === 0) return [];
  if (buckets.length === 1) return [buckets[0].label];
  if (buckets.length <= 4) return [buckets[0].label, buckets[buckets.length - 1].label];

  return [
    buckets[0].label,
    buckets[Math.floor(buckets.length / 2)].label,
    buckets[buckets.length - 1].label,
  ];
}
