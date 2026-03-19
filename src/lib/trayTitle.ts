import type { DefaultProvider, TrayConfig, RateLimitsPayload } from "./types/index.js";
import { primaryUtilization as rawUtilization } from "./traySync.js";

function primaryPct(
  rateLimits: RateLimitsPayload | null,
  provider: DefaultProvider,
): number | null {
  const val = rawUtilization(rateLimits, provider);
  return val !== null ? Math.round(val) : null;
}

const PROVIDER_LABELS: Array<{ provider: DefaultProvider; label: string }> = [
  { provider: 'claude', label: 'Claude Code' },
  { provider: 'codex', label: 'Codex' },
];

function formatPercentages(
  config: TrayConfig,
  rateLimits: RateLimitsPayload | null,
): string | null {
  if (!config.showPercentages) return null;

  const entries = PROVIDER_LABELS
    .map(({ provider, label }) => ({ label, pct: primaryPct(rateLimits, provider) }))
    .filter((e): e is { label: string; pct: number } => e.pct !== null);

  if (entries.length === 0) return null;

  if (config.percentageFormat === 'compact') {
    return entries.map((e) => `${e.pct}`).join(' · ');
  }
  return entries.map((e) => `${e.label} ${e.pct}%`).join('  ');
}

function formatCostPart(config: TrayConfig, totalCost: number): string | null {
  if (!config.showCost) return null;
  return config.costPrecision === 'whole'
    ? `$${Math.round(totalCost)}`
    : `$${totalCost.toFixed(2)}`;
}

export function formatTrayTitle(
  config: TrayConfig,
  rateLimits: RateLimitsPayload | null,
  totalCost: number,
): string {
  return [formatPercentages(config, rateLimits), formatCostPart(config, totalCost)]
    .filter(Boolean)
    .join("  ");
}
