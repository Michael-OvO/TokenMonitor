// Approximate exchange rates from USD (updated 2025-03)
const CURRENCIES: Record<string, { symbol: string; rate: number }> = {
  USD: { symbol: "$", rate: 1 },
  EUR: { symbol: "€", rate: 0.92 },
  GBP: { symbol: "£", rate: 0.79 },
  JPY: { symbol: "¥", rate: 149.5 },
  CNY: { symbol: "¥", rate: 7.24 },
};

let activeCurrency = "USD";

export function setCurrency(currency: string) {
  activeCurrency = currency;
}

export function currencySymbol(): string {
  return CURRENCIES[activeCurrency]?.symbol ?? "$";
}

export function convertCost(value: number): number {
  const cur = CURRENCIES[activeCurrency] ?? CURRENCIES.USD;
  return value * cur.rate;
}

export function formatCost(value: number): string {
  const cur = CURRENCIES[activeCurrency] ?? CURRENCIES.USD;
  const converted = value * cur.rate;
  if (activeCurrency === "JPY") {
    return `${cur.symbol}${Math.round(converted)}`;
  }
  return `${cur.symbol}${converted.toFixed(2)}`;
}

export function formatTokens(count: number): string {
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

export function modelColor(key: string): string {
  const normalized = key.trim().toLowerCase();
  const colors: Record<string, string> = {
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
  if (colors[normalized]) return colors[normalized];
  if (normalized.includes("opus")) return colors.opus;
  if (normalized.includes("sonnet")) return colors.sonnet;
  if (normalized.includes("haiku")) return colors.haiku;
  if (normalized.startsWith("gpt-5.4")) return colors.gpt54;
  if (normalized.startsWith("gpt-5.3")) return colors.gpt53;
  if (normalized.startsWith("gpt-5.2")) return colors.gpt52;
  if (normalized.startsWith("gpt-5.1-codex-mini")) return colors.gpt51mini;
  if (normalized.startsWith("gpt-5.1-codex-max")) return colors.gpt51max;
  if (normalized.startsWith("gpt-5.1-codex")) return colors.codex;
  if (normalized.startsWith("gpt-5.1")) return colors.codex;
  if (normalized.startsWith("gpt-5-mini")) return colors.gpt5mini;
  if (normalized.startsWith("gpt-5-nano")) return colors.gpt5nano;
  if (normalized.startsWith("gpt-5-codex")) return colors.gpt5codex;
  if (normalized.startsWith("gpt-5")) return colors.gpt5;
  if (normalized.startsWith("codex-mini")) return colors.codexmini;
  if (normalized.startsWith("o4-mini")) return colors.o4mini;
  if (normalized.startsWith("o3-mini")) return colors.o3mini;
  if (normalized.startsWith("o3")) return colors.o3;
  if (normalized.startsWith("o1-mini")) return colors.o1mini;
  if (normalized.startsWith("o1")) return colors.o1;
  if (normalized === "unknown") return colors.unknown;
  return hashedModelColor(normalized);
}
