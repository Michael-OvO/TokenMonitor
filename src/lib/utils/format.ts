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
  const hours = Math.floor(ms / 3_600_000);
  const minutes = Math.floor((ms % 3_600_000) / 60_000);
  if (hours >= 24) {
    const days = Math.floor(hours / 24);
    return `Resets in ${days}d ${hours % 24}h`;
  }
  return hours > 0 ? `Resets in ${hours}h ${minutes}m` : `Resets in ${minutes}m`;
}

export function formatRetryIn(isoString: string | null, now = Date.now()): string {
  if (!isoString) return "";
  const ms = new Date(isoString).getTime() - now;
  if (ms <= 0) return "Retrying...";

  const totalSeconds = Math.ceil(ms / 1000);
  if (totalSeconds < 60) return `Retry in ${totalSeconds}s`;

  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.ceil((totalSeconds % 3600) / 60);
  if (hours > 0) return `Retry in ${hours}h ${minutes}m`;
  return `Retry in ${minutes}m`;
}

function hashString(value: string): number {
  let hash = 0;
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash * 31 + value.charCodeAt(i)) >>> 0;
  }
  return hash;
}

function hashedModelColor(key: string): string {
  const hue = hashString(key) % 360;
  return `hsl(${hue} 58% 56%)`;
}

const EXACT_COLORS: Record<string, string> = {
  opus: "var(--opus)",
  sonnet: "var(--sonnet)",
  haiku: "var(--haiku)",
  gpt54: "var(--gpt54)",
  gpt53: "var(--gpt53)",
  gpt52: "var(--gpt52)",
  gpt51max: "var(--gpt53)",
  gpt51mini: "var(--o3mini)",
  gpt51: "var(--gpt52)",
  gpt5codex: "var(--codex)",
  codexmini: "var(--o4mini)",
  gpt5mini: "var(--o3mini)",
  gpt5nano: "var(--o1mini)",
  gpt5: "var(--codex)",
  o3: "var(--o3)",
  o3mini: "var(--o3mini)",
  o4mini: "var(--o4mini)",
  o1: "var(--o1)",
  o1mini: "var(--o1mini)",
  codex: "var(--codex)",
  unknown: "var(--t3)",
};

interface IncludesRule {
  readonly pattern: string;
  readonly color: string;
}

interface PrefixRule {
  readonly prefix: string;
  readonly color: string;
}

const INCLUDES_RULES: readonly IncludesRule[] = [
  { pattern: "opus", color: EXACT_COLORS.opus },
  { pattern: "sonnet", color: EXACT_COLORS.sonnet },
  { pattern: "haiku", color: EXACT_COLORS.haiku },
] as const;

// Order matters: longer prefixes must come before shorter ones (e.g. "gpt-5.1-codex-mini" before "gpt-5.1")
const PREFIX_RULES: readonly PrefixRule[] = [
  { prefix: "gpt-5.4", color: EXACT_COLORS.gpt54 },
  { prefix: "gpt-5.3", color: EXACT_COLORS.gpt53 },
  { prefix: "gpt-5.2", color: EXACT_COLORS.gpt52 },
  { prefix: "gpt-5.1-codex-mini", color: EXACT_COLORS.gpt51mini },
  { prefix: "gpt-5.1-codex-max", color: EXACT_COLORS.gpt51max },
  { prefix: "gpt-5.1-codex", color: EXACT_COLORS.codex },
  { prefix: "gpt-5.1", color: EXACT_COLORS.codex },
  { prefix: "gpt-5-mini", color: EXACT_COLORS.gpt5mini },
  { prefix: "gpt-5-nano", color: EXACT_COLORS.gpt5nano },
  { prefix: "gpt-5-codex", color: EXACT_COLORS.gpt5codex },
  { prefix: "gpt-5", color: EXACT_COLORS.gpt5 },
  { prefix: "codex-mini", color: EXACT_COLORS.codexmini },
  { prefix: "o4-mini", color: EXACT_COLORS.o4mini },
  { prefix: "o3-mini", color: EXACT_COLORS.o3mini },
  { prefix: "o3", color: EXACT_COLORS.o3 },
  { prefix: "o1-mini", color: EXACT_COLORS.o1mini },
  { prefix: "o1", color: EXACT_COLORS.o1 },
] as const;

// Brand families beyond Anthropic/OpenAI. Each family maps to three shades
// (deep/mid/soft) so same-brand models stay in the same hue but are
// distinguishable by version tier. The version tier is picked by the major
// version number extracted from the key — newer/flagship models get the
// deepest shade so they stand out in charts.
interface FamilyRule {
  readonly prefix: string;
  readonly shades: readonly [string, string, string];
  readonly tier: (major: number) => 0 | 1 | 2;
}

const FAMILY_RULES: readonly FamilyRule[] = [
  {
    prefix: "gemini",
    shades: ["var(--gemini)", "var(--gemini-mid)", "var(--gemini-soft)"],
    tier: (m) => (m >= 3 ? 0 : m >= 2 ? 1 : 2),
  },
  {
    prefix: "glm",
    shades: ["var(--glm)", "var(--glm-mid)", "var(--glm-soft)"],
    tier: (m) => (m >= 5 ? 0 : m >= 4 ? 1 : 2),
  },
  {
    prefix: "deepseek",
    shades: ["var(--deepseek)", "var(--deepseek-mid)", "var(--deepseek-soft)"],
    tier: (m) => (m >= 3 ? 0 : m >= 2 ? 1 : 2),
  },
  {
    prefix: "composer",
    shades: ["var(--composer)", "var(--composer-mid)", "var(--composer-soft)"],
    tier: () => 0,
  },
  {
    prefix: "kimi",
    shades: ["var(--kimi)", "var(--kimi-mid)", "var(--kimi-soft)"],
    // Kimi K2 is the current flagship; earlier k-series fall to mid.
    tier: (m) => (m >= 2 ? 0 : 1),
  },
  {
    prefix: "qwen",
    shades: ["var(--qwen)", "var(--qwen-mid)", "var(--qwen-soft)"],
    tier: (m) => (m >= 3 ? 0 : m >= 2 ? 1 : 2),
  },
] as const;

// Extract the first integer found after the brand prefix — covers
// "glm-4.5" (→ 4), "kimi-k2" (→ 2), "qwen3-coder" (→ 3),
// "gemini-2.5-pro" (→ 2), "deepseek-v3" (→ 3). Returns NaN if none.
function extractMajor(suffix: string): number {
  const match = suffix.match(/\d+/);
  return match ? parseInt(match[0], 10) : NaN;
}

// Moonshot's bare K-series keys ("k2", "k3", "k3-instruct") — the Kimi Code
// CLI logs these without the "kimi" prefix (mirrors
// `looks_like_moonshot_k_series` in src-tauri/src/models.rs).
function isMoonshotKSeries(key: string): boolean {
  return /^k\d+([.-]|$)/.test(key);
}

function familyColor(key: string): string | null {
  for (const rule of FAMILY_RULES) {
    let major: number;
    if (key.startsWith(rule.prefix)) {
      major = extractMajor(key.slice(rule.prefix.length));
    } else if (rule.prefix === "kimi" && isMoonshotKSeries(key)) {
      // Bare K-series keys carry the version right after the "k".
      major = extractMajor(key);
    } else {
      continue;
    }
    const tier = Number.isNaN(major) ? 2 : rule.tier(major);
    return rule.shades[tier];
  }
  return null;
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

export function modelColor(key: string): string {
  const normalized = key.trim().toLowerCase();

  const exact = EXACT_COLORS[normalized];
  if (exact) return exact;

  for (const rule of INCLUDES_RULES) {
    if (normalized.includes(rule.pattern)) return rule.color;
  }
  for (const rule of PREFIX_RULES) {
    if (normalized.startsWith(rule.prefix)) return rule.color;
  }

  const family = familyColor(normalized);
  if (family) return family;

  return hashedModelColor(normalized);
}
