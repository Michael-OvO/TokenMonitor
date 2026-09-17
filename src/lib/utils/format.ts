const CURRENCY_SYMBOLS: Record<string, string> = {
  USD: "$",
  EUR: "€",
  GBP: "£",
  JPY: "¥",
  CNY: "¥",
};

const DEFAULT_RATES: Record<string, number> = {
  EUR: 0.92,
  GBP: 0.79,
  JPY: 149.5,
  CNY: 7.24,
};

let exchangeRates: Record<string, number> = { ...DEFAULT_RATES };
let activeCurrency = "USD";

export function setCurrency(currency: string) {
  activeCurrency = currency;
}

export function setRates(rates: Record<string, number>) {
  exchangeRates = { ...DEFAULT_RATES, ...rates };
}

export function currencySymbol(): string {
  return CURRENCY_SYMBOLS[activeCurrency] ?? "$";
}

function rateFor(currency: string): number {
  if (currency === "USD") return 1;
  return exchangeRates[currency] ?? 1;
}

export function convertCost(value: number): number {
  return value * rateFor(activeCurrency);
}

export function formatCost(value: number): string {
  const symbol = CURRENCY_SYMBOLS[activeCurrency] ?? "$";
  const converted = value * rateFor(activeCurrency);
  if (activeCurrency === "JPY") {
    return `${symbol}${Math.round(converted)}`;
  }
  return `${symbol}${converted.toFixed(2)}`;
}

/**
 * Credit / spend-limit amounts: thousands separators, and no cents when the
 * converted amount lands on a whole unit. The whole-or-not decision has to
 * happen after conversion — $70 is round, €64.40 is not.
 *
 * Mirrors `format_auto` in `src-tauri/src/usage/money.rs`, which does the same
 * job for the labels Rust builds.
 */
export function formatCreditAmount(value: number): string {
  const symbol = CURRENCY_SYMBOLS[activeCurrency] ?? "$";
  const converted = value * rateFor(activeCurrency);
  const isWhole = Math.abs(converted - Math.round(converted)) < 0.005;
  const digits = isWhole || activeCurrency === "JPY" ? 0 : 2;
  return `${symbol}${converted.toLocaleString("en-US", {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  })}`;
}

export function formatModelCost(value: number, pricingAvailable: boolean | undefined): string {
  if (pricingAvailable === false) return "N/A";
  if (value === 0) return "Free";
  return formatCost(value);
}

export function formatTokens(count: number): string {
  if (count >= 1_000_000_000) return `${(count / 1_000_000_000).toFixed(1)}B`;
  if (count >= 1_000_000) return `${(count / 1_000_000).toFixed(1)}M`;
  if (count >= 1_000) return `${Math.round(count / 1_000)}K`;
  return count.toString();
}

export function formatTimeAgo(isoString: string): string {
  const seconds = Math.floor((Date.now() - new Date(isoString).getTime()) / 1000);
  if (seconds < 5) return "just now";
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  return `${Math.floor(seconds / 3600)}h ago`;
}

export function formatResetsIn(isoString: string | null): string {
  if (!isoString) return "";
  const ms = new Date(isoString).getTime() - Date.now();
  if (ms <= 0) return "Resetting...";
  return `Resets in ${formatDuration(ms)}`;
}

export function formatDuration(ms: number): string {
  if (ms <= 0) return "";
  const totalMinutes = Math.ceil(ms / 60_000);
  const units = [
    ["w", Math.floor(totalMinutes / 10_080)],
    ["d", Math.floor((totalMinutes % 10_080) / 1_440)],
    ["h", Math.floor((totalMinutes % 1_440) / 60)],
    ["m", totalMinutes % 60],
  ] as const;
  return units
    .filter(([, value]) => value > 0)
    .slice(0, 3)
    .map(([unit, value]) => `${value}${unit}`)
    .join(" ");
}

export function formatRetryIn(isoString: string | null, now = Date.now()): string {
  if (!isoString) return "";
  const ms = new Date(isoString).getTime() - now;
  if (ms <= 0) return "Retrying...";
  return `Retry in ${formatDuration(ms)}`;
}

function hashString(value: string): number {
  let hash = 0;
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash * 31 + value.charCodeAt(i)) >>> 0;
  }
  return hash;
}

// ── Device colors ──
// Palette chosen to be visually distinct from model colors and from each other.
// Deterministic: same alias always maps to the same color.
const DEVICE_COLOR_PALETTE = [
  "#6366f1", // indigo
  "#f59e0b", // amber
  "#10b981", // emerald
  "#ef4444", // red
  "#8b5cf6", // violet
  "#06b6d4", // cyan
  "#f97316", // orange
  "#ec4899", // pink
  "#14b8a6", // teal
  "#a855f7", // purple
];

export function deviceColor(alias: string): string {
  const idx = hashString(alias) % DEVICE_COLOR_PALETTE.length;
  return DEVICE_COLOR_PALETTE[idx];
}

// ── Device display names ──
// A device's raw name (`DeviceSummary.device` / a chart segment's device key)
// doubles as its identity — used for color, selection, sync, and IPC — so it is
// never mutated. These helpers derive a friendlier *display* string only:
//   • drop a trailing OS-style parenthetical, e.g. "My Mac (macOS)" → "My Mac"
//   • turn "-" into spaces, e.g. "AWS-RustDesk" → "AWS RustDesk"
// When two distinct devices would collapse to the same display string, the
// parenthetical is kept on both so they stay distinguishable.

const DEVICE_TRAILING_PARENS = /\s*\(([^()]*)\)\s*$/;
const DEVICE_HASH_SUFFIX = /(?:[-\s]+)[0-9a-fA-F]{8}$/;
const DEVICE_OS_SUFFIX = /(?:[-\s]+)(macOS|Windows|Linux)$/i;

type DeviceNameInfo = {
  base: string;
  paren: string | null;
  slugLike: boolean;
};

function normalizeDeviceIdentity(value: string): string {
  return value.replace(/\s+/g, " ").trim().toLocaleLowerCase();
}

function capitalizeFirstWord(value: string): string {
  return value.replace(/[A-Za-z]/, (letter) => letter.toLocaleUpperCase());
}

function deviceNameInfo(raw: string): DeviceNameInfo {
  const trimmed = raw.trim();
  const paren = deviceNameParen(trimmed);
  let stem = trimmed.replace(DEVICE_TRAILING_PARENS, "");
  const hadHash = DEVICE_HASH_SUFFIX.test(stem);
  stem = stem.replace(DEVICE_HASH_SUFFIX, "");
  const hadOsSuffix = !paren && DEVICE_OS_SUFFIX.test(stem);
  stem = stem.replace(DEVICE_OS_SUFFIX, "");

  const spaced = stem.replace(/-/g, " ").replace(/\s+/g, " ").trim();
  const slugLike = hadHash || hadOsSuffix;
  const base = spaced || trimmed;
  return {
    base: slugLike ? capitalizeFirstWord(base) : base,
    paren,
    slugLike,
  };
}

function deviceNameBase(raw: string): string {
  return deviceNameInfo(raw).base;
}

function deviceNameParen(raw: string): string | null {
  const inner = raw.match(DEVICE_TRAILING_PARENS)?.[1]?.trim();
  return inner ? inner : null;
}

/** Friendly display form of a single device name (no collision awareness). */
export function formatDeviceName(raw: string): string {
  return deviceNameBase(raw);
}

/** Stable key for deciding whether two raw device aliases represent one device. */
export function deviceIdentityKey(raw: string): string {
  const info = deviceNameInfo(raw);
  if (info.paren) return normalizeDeviceIdentity(raw.replace(/-/g, " "));
  return normalizeDeviceIdentity(info.base);
}

/**
 * Map each raw device name to its display name. When several distinct raw names
 * share the same base form, the parenthetical (e.g. the OS) is kept so the
 * collided devices stay distinguishable; a collided name with no parenthetical
 * falls back to its raw form.
 */
export function deviceDisplayNames(rawNames: Iterable<string>): Map<string, string> {
  const names = Array.from(new Set(rawNames));
  const baseCounts = new Map<string, number>();
  for (const raw of names) {
    const base = deviceNameBase(raw);
    baseCounts.set(base, (baseCounts.get(base) ?? 0) + 1);
  }
  const out = new Map<string, string>();
  for (const raw of names) {
    const info = deviceNameInfo(raw);
    const base = info.base;
    if ((baseCounts.get(base) ?? 0) > 1) {
      if (info.slugLike) {
        out.set(raw, base);
      } else {
        out.set(raw, info.paren ? `${base} (${info.paren})` : raw);
      }
    } else {
      out.set(raw, base);
    }
  }
  return out;
}

// ── Model colors ──
// One hue per vendor family (mirrors `detect_model_family` in
// src-tauri/src/models.rs); every model of that vendor lands somewhere inside
// that hue band. The exact spot is a deterministic hash of the key spread over
// 30 slots (5 hue nudges × 6 lightness steps), so new models never leave their
// family's colour range and need no table entry.
// ponytail: hash → slot can collide for two same-vendor models; assign slots
// in first-seen order if that ever shows up in a real chart.
const MODEL_SLOTS_PER_FAMILY = 30;
const FAMILY_HUES: readonly { readonly test: RegExp; readonly hue: number }[] = [
  { test: /claude|fable|mythos|opus|sonnet|haiku/, hue: 22 }, // warm orange
  { test: /^gpt|^o\d|codex/, hue: 207 }, // blue
  { test: /^gemini/, hue: 262 }, // indigo
  // Bare K-series ("k3", "k3-instruct") is how the Kimi Code CLI logs Moonshot
  // models (mirrors `looks_like_moonshot_k_series` in src-tauri/src/models.rs).
  { test: /^kimi|moonshot|^k\d+([. ]|$)/, hue: 318 }, // magenta
  { test: /^qwen/, hue: 345 }, // rose
  { test: /^glm/, hue: 145 }, // green
  { test: /^deepseek/, hue: 235 }, // cobalt
  { test: /^composer/, hue: 290 }, // violet
  { test: /^grok|xai/, hue: 100 }, // lime
];
// Hues ≥20° away from every family band, so an unknown vendor never reads as a
// known one. Picked per key by hash, then nudged/lightened like family models.
const UNCLAIMED_HUES = [50, 72, 122, 170, 185];

export function modelColor(key: string): string {
  // Cursor slugs look like "cursor-claude-4.5-sonnet": drop the "cursor"
  // prefix and dashes so the vendor name leads and the ^ anchors match.
  const normalized = key
    .trim()
    .toLowerCase()
    .replace(/-/g, " ")
    .replace(/^cursor\s+/, "")
    .replace(/\s+/g, " ");
  const family = FAMILY_HUES.find((f) => f.test.test(normalized));
  const hash = hashString(normalized);
  const slot = hash % MODEL_SLOTS_PER_FAMILY;
  const base = family?.hue ?? UNCLAIMED_HUES[Math.floor(hash / MODEL_SLOTS_PER_FAMILY) % UNCLAIMED_HUES.length];
  const hue = base + ((slot % 5) - 2) * 5; // −10…+10°
  const light = 42 + Math.floor(slot / 5) * 5; // 42…67 %
  return `hsl(${hue} 58% ${light}%)`;
}
