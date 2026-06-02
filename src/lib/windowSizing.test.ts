import { describe, expect, it } from "vitest";
import {
  DEFAULT_MAX_WINDOW_HEIGHT,
  MIN_WINDOW_HEIGHT,
  RESIZE_HYSTERESIS_PX,
  WINDOW_MONITOR_MARGIN,
  WINDOW_HEIGHT_PADDING,
  clampWindowHeight,
  classifyResize,
  isWindowScrollLocked,
  measureTargetWindowHeight,
  resolveEffectiveWindowMaxHeight,
  resolveMonitorMaxWindowHeight,
  resolveScrollThresholdHeight,
  SCROLL_THRESHOLD_CAP,
  resolveFixedWindowHeight,
  FIXED_HEIGHT_CAP,
} from "./windowSizing.js";

describe("measureTargetWindowHeight", () => {
  it("rounds content height up and adds the window padding", () => {
    expect(measureTargetWindowHeight(245.2)).toBe(246 + WINDOW_HEIGHT_PADDING);
  });
});

describe("classifyResize", () => {
  it("skips heights below the minimum viable window size", () => {
    expect(classifyResize(MIN_WINDOW_HEIGHT - 1, 320)).toBe("skip");
  });

  it("skips unchanged heights", () => {
    expect(classifyResize(320, 320)).toBe("skip");
  });

  it("marks larger heights as grow operations", () => {
    expect(classifyResize(360, 320)).toBe("grow");
  });

  it("marks smaller heights as shrink operations", () => {
    expect(classifyResize(280, 320)).toBe("shrink");
  });

  it("skips tiny height changes within hysteresis to break observer feedback loops", () => {
    expect(classifyResize(321, 320)).toBe("skip");
    expect(classifyResize(319, 320)).toBe("skip");
    expect(classifyResize(320 + RESIZE_HYSTERESIS_PX, 320)).toBe("skip");
  });

  it("still grows or shrinks when the delta exceeds hysteresis", () => {
    expect(classifyResize(324, 320)).toBe("grow");
    expect(classifyResize(316, 320)).toBe("shrink");
  });
});

describe("clampWindowHeight", () => {
  it("caps the target height at the configured max", () => {
    expect(clampWindowHeight(1400, 900)).toBe(900);
  });

  it("never returns less than the minimum height", () => {
    expect(clampWindowHeight(50, 900)).toBe(MIN_WINDOW_HEIGHT);
  });
});

describe("resolveEffectiveWindowMaxHeight", () => {
  it("caps the outer window height at the scroll threshold when scrolling is active", () => {
    expect(resolveEffectiveWindowMaxHeight(900, 640)).toBe(640);
  });

  it("falls back to the monitor max when the scroll threshold is unusable", () => {
    expect(resolveEffectiveWindowMaxHeight(900, Number.NaN)).toBe(900);
  });
});

describe("isWindowScrollLocked", () => {
  it("locks when intrinsic content exceeds the effective window max", () => {
    expect(isWindowScrollLocked(641, 640)).toBe(true);
  });

  it("does not lock when the content still fits", () => {
    expect(isWindowScrollLocked(640, 640)).toBe(false);
  });
});

describe("resolveMonitorMaxWindowHeight", () => {
  it("converts the monitor work area into a logical max height", () => {
    expect(resolveMonitorMaxWindowHeight(1800, 2)).toBe(876);
  });

  it("falls back when the monitor values are unusable", () => {
    expect(resolveMonitorMaxWindowHeight(Number.NaN, 2)).toBe(DEFAULT_MAX_WINDOW_HEIGHT);
    expect(resolveMonitorMaxWindowHeight(1800, 0)).toBe(DEFAULT_MAX_WINDOW_HEIGHT);
  });

  it("honors the configured fallback cap after subtracting the monitor margin", () => {
    expect(resolveMonitorMaxWindowHeight(3000, 1)).toBe(DEFAULT_MAX_WINDOW_HEIGHT);
    expect(resolveMonitorMaxWindowHeight(1180, 1)).toBe(1180 - WINDOW_MONITOR_MARGIN);
  });
});

describe("resolveScrollThresholdHeight", () => {
  it("caps the threshold at SCROLL_THRESHOLD_CAP even on large monitors", () => {
    expect(resolveScrollThresholdHeight(2160, 1)).toBe(SCROLL_THRESHOLD_CAP);
  });

  it("returns the ratio-based value when below the cap", () => {
    expect(resolveScrollThresholdHeight(600, 1, 0.75)).toBe(450);
  });

  it("falls back to DEFAULT_MAX_WINDOW_HEIGHT for invalid inputs", () => {
    expect(resolveScrollThresholdHeight(Number.NaN, 1)).toBe(DEFAULT_MAX_WINDOW_HEIGHT);
    expect(resolveScrollThresholdHeight(1080, 0)).toBe(DEFAULT_MAX_WINDOW_HEIGHT);
  });
});

describe("resolveFixedWindowHeight", () => {
  it("returns cap when monitor width is invalid", () => {
    expect(resolveFixedWindowHeight(NaN, 2)).toBe(FIXED_HEIGHT_CAP);
    expect(resolveFixedWindowHeight(0, 2)).toBe(FIXED_HEIGHT_CAP);
    expect(resolveFixedWindowHeight(-100, 2)).toBe(FIXED_HEIGHT_CAP);
  });

  it("returns cap when scale factor is invalid", () => {
    expect(resolveFixedWindowHeight(2560, 0)).toBe(FIXED_HEIGHT_CAP);
    expect(resolveFixedWindowHeight(2560, NaN)).toBe(FIXED_HEIGHT_CAP);
    expect(resolveFixedWindowHeight(2560, -1)).toBe(FIXED_HEIGHT_CAP);
  });

  it("computes floor(logicalWidth * ratio) for normal inputs", () => {
    // 2560 physical / 2 scale = 1280 logical * 0.392 = 501.76 → capped at 500
    expect(resolveFixedWindowHeight(2560, 2)).toBe(FIXED_HEIGHT_CAP);
  });

  it("returns ratio-based value when below cap", () => {
    // 1920 physical / 2 scale = 960 logical * 0.392 = 376.32 → floor = 376
    expect(resolveFixedWindowHeight(1920, 2)).toBe(376);
  });

  it("respects custom cap and ratio", () => {
    // 1920 / 1 = 1920 * 0.5 = 960 → capped at 400
    expect(resolveFixedWindowHeight(1920, 1, 400, 0.5)).toBe(400);
    // 800 / 1 = 800 * 0.5 = 400 → exactly at cap
    expect(resolveFixedWindowHeight(800, 1, 400, 0.5)).toBe(400);
    // 600 / 1 = 600 * 0.5 = 300 → below cap
    expect(resolveFixedWindowHeight(600, 1, 400, 0.5)).toBe(300);
  });

  it("handles scale factor of 1 (non-retina)", () => {
    // 1440 / 1 = 1440 * 0.392 = 564.48 → capped at 500
    expect(resolveFixedWindowHeight(1440, 1)).toBe(FIXED_HEIGHT_CAP);
    // 1000 / 1 = 1000 * 0.392 = 392 → below cap
    expect(resolveFixedWindowHeight(1000, 1)).toBe(392);
  });
});
