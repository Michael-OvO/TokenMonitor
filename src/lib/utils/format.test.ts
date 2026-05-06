import { describe, it, expect, beforeEach } from "vitest";
import {
  setCurrency,
  currencySymbol,
  convertCost,
  formatCost,
  formatTokens,
  formatTimeAgo,
  modelColor,
} from "./format.js";

beforeEach(() => {
  setCurrency("USD");
});

// ── formatCost ──────────────────────────────────────────────────────

describe("formatCost", () => {
  it("formats USD with two decimals", () => {
    expect(formatCost(1.5)).toBe("$1.50");
  });

  it("formats zero", () => {
    expect(formatCost(0)).toBe("$0.00");
  });

  it("converts and formats EUR", () => {
    setCurrency("EUR");
    expect(formatCost(1.0)).toBe("€0.92");
  });

  it("converts and formats GBP", () => {
    setCurrency("GBP");
    expect(formatCost(1.0)).toBe("£0.79");
  });

  it("formats JPY with no decimals (rounded)", () => {
    setCurrency("JPY");
    expect(formatCost(1.0)).toBe("¥150");
  });

  it("converts and formats CNY", () => {
    setCurrency("CNY");
    expect(formatCost(1.0)).toBe("¥7.24");
  });

  it("falls back to USD for unknown currency", () => {
    setCurrency("XYZ");
    expect(formatCost(1.0)).toBe("$1.00");
  });
});

// ── convertCost ─────────────────────────────────────────────────────

describe("convertCost", () => {
  it("returns same value for USD", () => {
    expect(convertCost(10)).toBe(10);
  });

  it("converts to GBP", () => {
    setCurrency("GBP");
    expect(convertCost(10)).toBeCloseTo(7.9);
  });

  it("converts to JPY", () => {
    setCurrency("JPY");
    expect(convertCost(1)).toBeCloseTo(149.5);
  });

  it("falls back to USD rate for unknown currency", () => {
    setCurrency("NOPE");
    expect(convertCost(5)).toBe(5);
  });
});

// ── currencySymbol ──────────────────────────────────────────────────

describe("currencySymbol", () => {
  it.each([
    ["USD", "$"],
    ["EUR", "€"],
    ["GBP", "£"],
    ["JPY", "¥"],
    ["CNY", "¥"],
  ])("returns correct symbol for %s", (code, symbol) => {
    setCurrency(code);
    expect(currencySymbol()).toBe(symbol);
  });

  it("falls back to $ for unknown currency", () => {
    setCurrency("UNKNOWN");
    expect(currencySymbol()).toBe("$");
  });
});

// ── formatTokens ────────────────────────────────────────────────────

describe("formatTokens", () => {
  it.each([
    [0, "0"],
    [999, "999"],
    [1000, "1K"],
    [1500, "2K"],
    [999999, "1000K"],
    [1_000_000, "1.0M"],
    [1_500_000, "1.5M"],
    [10_000_000, "10.0M"],
    [1_000_000_000, "1.0B"],
    [6_225_500_000, "6.2B"],
  ])("formats %d as %s", (input, expected) => {
    expect(formatTokens(input)).toBe(expected);
  });
});

// ── formatTimeAgo ───────────────────────────────────────────────────

describe("formatTimeAgo", () => {
  it("returns 'just now' for <5 seconds ago", () => {
    const iso = new Date(Date.now() - 2000).toISOString();
    expect(formatTimeAgo(iso)).toBe("just now");
  });

  it("returns seconds for <60s", () => {
    const iso = new Date(Date.now() - 30_000).toISOString();
    expect(formatTimeAgo(iso)).toBe("30s ago");
  });

  it("returns minutes for <1h", () => {
    const iso = new Date(Date.now() - 5 * 60_000).toISOString();
    expect(formatTimeAgo(iso)).toBe("5m ago");
  });

  it("returns hours for >=1h", () => {
    const iso = new Date(Date.now() - 2 * 3_600_000).toISOString();
    expect(formatTimeAgo(iso)).toBe("2h ago");
  });
});

// ── modelColor ──────────────────────────────────────────────────────

describe("modelColor", () => {
  it.each([
    ["opus", "var(--opus)"],
    ["opus-4-6", "var(--opus)"],
    ["sonnet", "var(--sonnet)"],
    ["sonnet-4-6", "var(--sonnet)"],
    ["haiku", "var(--haiku)"],
    ["haiku-4-5", "var(--haiku)"],
    ["gpt54", "var(--gpt54)"],
    ["gpt53", "var(--gpt53)"],
    ["gpt52", "var(--gpt52)"],
    ["gpt-5.4", "var(--gpt54)"],
    ["gpt-5.3-codex", "var(--gpt53)"],
    ["gpt-5.2", "var(--gpt52)"],
    ["codex", "var(--codex)"],
    ["unknown", "var(--t3)"],
    // ── Gemini: major>=3 deep, ==2 mid, else soft ──
    ["gemini-3.0", "var(--gemini)"],
    ["gemini-2.5-pro", "var(--gemini-mid)"],
    ["gemini-1.5-flash", "var(--gemini-soft)"],
    // ── GLM: >=5 deep, ==4 mid, else soft ──
    ["glm-5", "var(--glm)"],
    ["glm-4.5", "var(--glm-mid)"],
    ["glm-3-turbo", "var(--glm-soft)"],
    // ── DeepSeek: >=3 deep, ==2 mid, else soft ──
    ["deepseek-v3", "var(--deepseek)"],
    ["deepseek-v2.5", "var(--deepseek-mid)"],
    ["deepseek-chat", "var(--deepseek-soft)"], // no version → soft
    // ── Kimi: K2+ deep, else mid ──
    ["kimi-k2", "var(--kimi)"],
    ["kimi-k1", "var(--kimi-mid)"],
    // ── Qwen: >=3 deep, ==2 mid, else soft ──
    ["qwen3-coder", "var(--qwen)"],
    ["qwen2.5-max", "var(--qwen-mid)"],
    // ── Composer: single tier ──
    ["composer-1", "var(--composer)"],
  ])("returns correct CSS var for %s", (key, expected) => {
    expect(modelColor(key)).toBe(expected);
  });

  it("keeps palette colors deterministic for raw codex model names", () => {
    expect(modelColor("gpt-5.4")).toBe(modelColor("gpt-5.4"));
    expect(modelColor("gpt-5.2")).toBe(modelColor("gpt-5.2"));
    expect(modelColor("gpt-5.4")).toBe("var(--gpt54)");
    expect(modelColor("gpt-5.2")).toBe("var(--gpt52)");
  });

  it("returns a hashed fallback for unrecognized keys", () => {
    expect(modelColor("nonexistent")).toMatch(/^hsl\(/);
  });
});
