import { describe, expect, it } from "vitest";
import { periodTabs } from "./periodMode.js";

describe("period mode labels", () => {
  it("uses calendar-to-date labels including Today", () => {
    expect(periodTabs(false).map(t => t.label)).toEqual(["Usage", "Today", "WTD", "MTD", "YTD"]);
  });
  it("uses trailing labels without changing period identities", () => {
    expect(periodTabs(true).map(t => t.label)).toEqual(["Usage", "Last 24h", "Last 7d", "Last month", "Last year"]);
    expect(periodTabs(true).map(t => t.value)).toEqual(periodTabs(false).map(t => t.value));
  });
});
