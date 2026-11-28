import type { UsagePeriod } from "../types/index.js";

export function periodTabs(rolling: boolean): Array<{ value: UsagePeriod; label: string }> {
  return [
    { value: "5h", label: "Usage" },
    { value: "day", label: rolling ? "Last 24h" : "Today" },
    { value: "week", label: rolling ? "Last 7d" : "WTD" },
    { value: "month", label: rolling ? "Last month" : "MTD" },
    { value: "year", label: rolling ? "Last year" : "YTD" },
  ];
}
