import { describe, it, expect } from "vitest";
import { intensityLevel, computeEarned, heatmapColor } from "./calendar.js";

// ── intensityLevel ──

describe("intensityLevel", () => {
  it("returns 0 for zero cost", () => {
    expect(intensityLevel(0, 100)).toBe(0);
  });

  it("returns 0 when max is zero", () => {
    expect(intensityLevel(50, 0)).toBe(0);
  });

  it("returns 1 for 1-25% of max", () => {
    expect(intensityLevel(25, 100)).toBe(1);
    expect(intensityLevel(1, 100)).toBe(1);
  });

  it("returns 2 for 26-50% of max", () => {
    expect(intensityLevel(50, 100)).toBe(2);
    expect(intensityLevel(26, 100)).toBe(2);
  });

  it("returns 3 for 51-75% of max", () => {
    expect(intensityLevel(75, 100)).toBe(3);
    expect(intensityLevel(51, 100)).toBe(3);
  });

  it("returns 4 for 76-100% of max", () => {
    expect(intensityLevel(100, 100)).toBe(4);
    expect(intensityLevel(76, 100)).toBe(4);
  });
});

// ── computeEarned ──

describe("computeEarned", () => {
  it("returns null when plan is 0", () => {
    expect(computeEarned(347, 0)).toBeNull();
  });

  it("returns positive when spend exceeds plan", () => {
    expect(computeEarned(347, 200)).toBe(147);
  });

  it("returns negative when spend is under plan", () => {
    expect(computeEarned(15, 20)).toBe(-5);
  });

  it("returns 0 when spend equals plan", () => {
    expect(computeEarned(200, 200)).toBe(0);
  });
});

// ── heatmapColor ──

describe("heatmapColor", () => {
  it("returns surface-2 for level 0", () => {
    expect(heatmapColor(0, true, "claude")).toBe("var(--surface-2)");
  });

  it("returns terracotta for Claude with brand theming", () => {
    expect(heatmapColor(4, true, "claude")).toBe("rgba(196, 112, 75, 0.9)");
  });

  it("returns blue for Codex with brand theming", () => {
    expect(heatmapColor(2, true, "codex")).toBe("rgba(74, 123, 157, 0.4)");
  });

  it("returns green when brand theming is off", () => {
    expect(heatmapColor(3, false, "claude")).toBe("rgba(77, 175, 74, 0.65)");
  });

  it("returns green for 'all' provider regardless of brand theming", () => {
    expect(heatmapColor(1, true, "all")).toBe("rgba(77, 175, 74, 0.15)");
  });
});
