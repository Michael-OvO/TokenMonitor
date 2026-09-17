import { describe, it, expect, beforeEach } from "vitest";
import {
  setCurrency,
  currencySymbol,
  convertCost,
  formatCost,
  formatCreditAmount,
  formatModelCost,
  formatTokens,
  formatTimeAgo,
  formatDuration,
  modelColor,
  formatDeviceName,
  deviceIdentityKey,
  deviceDisplayNames,
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

// `UsageBars` used to format credit/spend-limit amounts with its own
// `Intl.NumberFormat({ currency: "USD" })`, so those two figures stayed in
// dollars while everything around them followed the user's currency.
describe("formatCreditAmount", () => {
  it("drops the cents for whole amounts and keeps thousands separators", () => {
    expect(formatCreditAmount(70)).toBe("$70");
    expect(formatCreditAmount(1500)).toBe("$1,500");
  });

  it("keeps two decimals for fractional amounts", () => {
    expect(formatCreditAmount(70.5)).toBe("$70.50");
  });

  it("converts and stamps the selected currency", () => {
    setCurrency("EUR");
    expect(formatCreditAmount(70)).toBe("€64.40");
  });

  it("follows the JPY no-decimals rule", () => {
    setCurrency("JPY");
    expect(formatCreditAmount(1)).toBe("¥150");
  });
});

describe("formatModelCost", () => {
  it("returns N/A when model pricing is unavailable", () => {
    expect(formatModelCost(0, false)).toBe("N/A");
  });

  it("returns Free when model pricing is known and the cost is zero", () => {
    expect(formatModelCost(0, true)).toBe("Free");
    expect(formatModelCost(0, undefined)).toBe("Free");
  });

  it("keeps normal cost formatting for non-zero costs when pricing is available or unspecified", () => {
    expect(formatModelCost(1.5, undefined)).toBe("$1.50");
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

describe("formatDuration", () => {
  it.each([
    [((2 * 7 + 3) * 24 + 4) * 3_600_000 + 5 * 60_000, "2w 3d 4h"],
    [18 * 3_600_000, "18h"],
    [(7 * 24 + 1) * 3_600_000 + 60_000, "1w 1h 1m"],
    [30_000, "1m"],
  ])("formats %d ms as %s", (ms, expected) => {
    expect(formatDuration(ms)).toBe(expected);
  });
});

// ── modelColor ──────────────────────────────────────────────────────

describe("modelColor", () => {
  const hue = (c: string) => Number(c.match(/^hsl\((-?\d+)/)![1]);

  it("keeps every model of a vendor inside that vendor's hue band", () => {
    const anthropic = ["opus", "opus-4-6", "sonnet-4-6", "haiku-4-5", "claude-fable-5-1", "mythos-9"];
    const openai = ["gpt-5.4", "gpt-5.3-codex", "gpt-5", "o3-mini", "o4-mini-2025-04-16", "codex-mini-latest"];
    for (const k of anthropic) expect(Math.abs(hue(modelColor(k)) - 22)).toBeLessThanOrEqual(10);
    for (const k of openai) expect(Math.abs(hue(modelColor(k)) - 207)).toBeLessThanOrEqual(10);
    expect(Math.abs(hue(modelColor("gemini-2.5-pro")) - 262)).toBeLessThanOrEqual(10);
    expect(Math.abs(hue(modelColor("kimi-k2")) - 318)).toBeLessThanOrEqual(10);
    // Bare K-series keys from the Kimi Code CLI stay in the Kimi band.
    for (const k of ["k3", "k3-instruct", "k2"]) expect(Math.abs(hue(modelColor(k)) - 318)).toBeLessThanOrEqual(10);
    expect(Math.abs(hue(modelColor("qwen3-coder")) - 345)).toBeLessThanOrEqual(10);
    expect(Math.abs(hue(modelColor("glm-4.5")) - 145)).toBeLessThanOrEqual(10);
    expect(Math.abs(hue(modelColor("deepseek-v3")) - 235)).toBeLessThanOrEqual(10);
    expect(Math.abs(hue(modelColor("composer-1")) - 290)).toBeLessThanOrEqual(10);
  });

  it("is deterministic and gives siblings distinct shades", () => {
    expect(modelColor("gpt-5.4")).toBe(modelColor("GPT-5.4 "));
    expect(modelColor("gpt-5.4")).not.toBe(modelColor("gpt-5.2"));
    expect(modelColor("opus-4-6")).not.toBe(modelColor("sonnet-4-6"));
  });

  it("falls back to greys for unknown vendors", () => {
    expect(modelColor("nonexistent")).toMatch(/^hsl\(0 0% /);
    expect(modelColor("unknown")).toMatch(/^hsl\(0 0% /);
  });
});

// ── device display names ───────────────────────────────────────────────

describe("formatDeviceName", () => {
  it("replaces dashes with spaces", () => {
    expect(formatDeviceName("AWS-RustDesk")).toBe("AWS RustDesk");
  });

  it("strips a trailing OS parenthetical", () => {
    expect(formatDeviceName("thomas-Linux-Desktop (Linux)")).toBe("thomas Linux Desktop");
  });

  it("leaves a plain name unchanged", () => {
    expect(formatDeviceName("tianhe")).toBe("tianhe");
    expect(formatDeviceName("Local")).toBe("Local");
  });

  it("strips auto-sync OS and hash suffixes", () => {
    expect(formatDeviceName("thomas-Linux-Desktop-Linux-3033b0e0")).toBe("Thomas Linux Desktop");
    expect(formatDeviceName("thomas-Linux-Desktop-Linux")).toBe("Thomas Linux Desktop");
  });
});

describe("deviceIdentityKey", () => {
  it("treats auto-sync slug variants and display names as the same device", () => {
    const plain = deviceIdentityKey("Thomas Linux Desktop");
    expect(deviceIdentityKey("thomas-Linux-Desktop-Linux-3033b0e0")).toBe(plain);
    expect(deviceIdentityKey("thomas-Linux-Desktop-Linux")).toBe(plain);
  });
});

describe("deviceDisplayNames", () => {
  it("maps each name to its base form when there is no collision", () => {
    const m = deviceDisplayNames(["AWS-RustDesk", "tianhe", "Local"]);
    expect(m.get("AWS-RustDesk")).toBe("AWS RustDesk");
    expect(m.get("tianhe")).toBe("tianhe");
    expect(m.get("Local")).toBe("Local");
  });

  it("keeps the parenthetical when two names collide on the base", () => {
    const m = deviceDisplayNames(["Desktop (Linux)", "Desktop (Windows)"]);
    expect(m.get("Desktop (Linux)")).toBe("Desktop (Linux)");
    expect(m.get("Desktop (Windows)")).toBe("Desktop (Windows)");
  });

  it("drops the parenthetical when names do not collide", () => {
    const m = deviceDisplayNames(["Desktop (Linux)", "Laptop (Windows)"]);
    expect(m.get("Desktop (Linux)")).toBe("Desktop");
    expect(m.get("Laptop (Windows)")).toBe("Laptop");
  });

  it("falls back to the raw name for paren-less collisions", () => {
    const m = deviceDisplayNames(["foo-bar", "foo bar"]);
    expect(m.get("foo-bar")).toBe("foo-bar");
    expect(m.get("foo bar")).toBe("foo bar");
  });

  it("collapses auto-sync slug variants to the friendly base", () => {
    const m = deviceDisplayNames([
      "thomas-Linux-Desktop-Linux",
      "thomas-Linux-Desktop-Linux-3033b0e0",
    ]);
    expect(m.get("thomas-Linux-Desktop-Linux")).toBe("Thomas Linux Desktop");
    expect(m.get("thomas-Linux-Desktop-Linux-3033b0e0")).toBe("Thomas Linux Desktop");
  });
});
