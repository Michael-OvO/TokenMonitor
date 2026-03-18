import type { TrayConfig, RateLimitsPayload } from "./types/index.js";

function primaryUtilization(
  rateLimits: RateLimitsPayload | null,
  provider: 'claude' | 'codex',
): number | null {
  const data = rateLimits?.[provider];
  if (!data || data.windows.length === 0) return null;
  return Math.round(data.windows[0].utilization);
}

export function formatTrayTitle(
  config: TrayConfig,
  rateLimits: RateLimitsPayload | null,
  totalCost: number,
): string {
  const parts: string[] = [];

  // Percentages — independent of barDisplay
  if (config.showPercentages) {
    const claudePct = primaryUtilization(rateLimits, 'claude');
    const codexPct = primaryUtilization(rateLimits, 'codex');

    if (claudePct !== null && codexPct !== null) {
      if (config.percentageFormat === 'compact') {
        parts.push(`${claudePct} · ${codexPct}`);
      } else {
        parts.push(`Claude Code ${claudePct}%  Codex ${codexPct}%`);
      }
    } else if (claudePct !== null) {
      if (config.percentageFormat === 'compact') {
        parts.push(`${claudePct}`);
      } else {
        parts.push(`Claude Code ${claudePct}%`);
      }
    } else if (codexPct !== null) {
      if (config.percentageFormat === 'compact') {
        parts.push(`${codexPct}`);
      } else {
        parts.push(`Codex ${codexPct}%`);
      }
    }
  }

  // Cost
  if (config.showCost) {
    if (config.costPrecision === 'whole') {
      parts.push(`$${Math.round(totalCost)}`);
    } else {
      parts.push(`$${totalCost.toFixed(2)}`);
    }
  }

  return parts.join("  ");
}
