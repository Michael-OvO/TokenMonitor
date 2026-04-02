import { getUsageProviderBrandColor } from "../providerMetadata.js";
import type { UsageProvider } from "../types/index.js";

export const INTENSITY_OPACITY = [0, 0.15, 0.40, 0.65, 0.90];

export function intensityLevel(cost: number, maxCost: number): number {
  if (maxCost === 0 || cost === 0) return 0;
  const ratio = cost / maxCost;
  if (ratio <= 0.25) return 1;
  if (ratio <= 0.50) return 2;
  if (ratio <= 0.75) return 3;
  return 4;
}

export function computeEarned(totalCost: number, planCost: number): number | null {
  if (planCost <= 0) return null;
  return totalCost - planCost;
}

export function heatmapColor(
  level: number,
  brandTheming: boolean,
  provider: UsageProvider,
): string {
  if (level === 0) return "var(--surface-2)";
  const opacity = INTENSITY_OPACITY[level];
  return brandTheming
    ? (getUsageProviderBrandColor(provider, opacity) ?? `rgba(77, 175, 74, ${opacity})`)
    : `rgba(77, 175, 74, ${opacity})`;
}
