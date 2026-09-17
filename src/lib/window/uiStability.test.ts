import { describe, expect, it } from "vitest";
import {
  remainingRefreshVisibilityMs,
  shouldSkipResizeByJitter,
} from "./uiStability.js";

describe("remainingRefreshVisibilityMs", () => {
  it("returns 0 when indicator is hidden", () => {
    expect(
      remainingRefreshVisibilityMs({
        isVisible: false,
        shownAt: 1000,
        now: 1200,
        minVisibleMs: 900,
      }),
    ).toBe(0);
  });

  it("returns the remaining minimum visibility duration", () => {
    expect(
      remainingRefreshVisibilityMs({
        isVisible: true,
        shownAt: 1000,
        now: 1400,
        minVisibleMs: 900,
      }),
    ).toBe(500);
  });

  it("never returns a negative value", () => {
    expect(
      remainingRefreshVisibilityMs({
        isVisible: true,
        shownAt: 1000,
        now: 2200,
        minVisibleMs: 900,
      }),
    ).toBe(0);
  });
});

describe("shouldSkipResizeByJitter", () => {
  it("skips tiny height deltas", () => {
    expect(shouldSkipResizeByJitter(400, 401, 2)).toBe(true);
    expect(shouldSkipResizeByJitter(400, 402, 2)).toBe(true);
  });

  it("does not skip material height changes", () => {
    expect(shouldSkipResizeByJitter(400, 403, 2)).toBe(false);
    expect(shouldSkipResizeByJitter(400, 390, 2)).toBe(false);
  });
});
