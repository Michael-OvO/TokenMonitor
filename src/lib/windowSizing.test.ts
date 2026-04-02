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
