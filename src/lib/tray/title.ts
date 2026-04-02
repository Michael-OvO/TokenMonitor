import { getUsageProviderTitle, RATE_LIMIT_PROVIDER_ORDER } from "../providerMetadata.js";
import type { RateLimitProviderId, TrayConfig, RateLimitsPayload } from "../types/index.js";
import { primaryUtilization as rawUtilization } from "./sync.js";

function primaryPct(
  rateLimits: RateLimitsPayload | null,
  provider: RateLimitProviderId,
): number | null {
  const val = rawUtilization(rateLimits, provider);
  return val !== null ? Math.round(val) : null;
}

function formatPercentages(
  config: TrayConfig,
  rateLimits: RateLimitsPayload | null,
): string | null {
  if (!config.showPercentages) return null;

  const entries = RATE_LIMIT_PROVIDER_ORDER
    .map((provider) => ({
      label: getUsageProviderTitle(provider),
      pct: primaryPct(rateLimits, provider),
    }))
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
