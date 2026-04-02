import { describe, expect, it } from "vitest";
import {
  DEFAULT_MAX_WINDOW_HEIGHT,
  MIN_WINDOW_HEIGHT,
  WINDOW_MONITOR_MARGIN,
  WINDOW_HEIGHT_PADDING,
  clampWindowHeight,
  classifyResize,
  measureTargetWindowHeight,
  resolveMonitorMaxWindowHeight,
} from "./sizing.js";

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

  it("skips differences within the 2px tolerance", () => {
    expect(classifyResize(321, 320)).toBe("skip");
    expect(classifyResize(322, 320)).toBe("skip");
    expect(classifyResize(319, 320)).toBe("skip");
    expect(classifyResize(318, 320)).toBe("skip");
  });

  it("marks larger heights as grow operations", () => {
    expect(classifyResize(360, 320)).toBe("grow");
    expect(classifyResize(323, 320)).toBe("grow"); // 3px > tolerance
  });

  it("marks smaller heights as shrink operations", () => {
    expect(classifyResize(280, 320)).toBe("shrink");
    expect(classifyResize(317, 320)).toBe("shrink"); // 3px > tolerance
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
