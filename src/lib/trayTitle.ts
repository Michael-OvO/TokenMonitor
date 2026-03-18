import type { TrayConfig, RateLimitsPayload } from "./types/index.js";

function primaryUtilization(
  rateLimits: RateLimitsPayload | null,
  provider: 'claude' | 'codex',
): number | null {
  const data = rateLimits?.[provider];
  if (!data || data.windows.length === 0) return null;
  return Math.round(data.windows[0].utilization * 100);
}

export function formatTrayTitle(
  config: TrayConfig,
  rateLimits: RateLimitsPayload | null,
  totalCost: number,
): string {
  const parts: string[] = [];

  // Percentages
  if (config.showPercentages) {
    const claudePct = primaryUtilization(rateLimits, 'claude');
    const codexPct = primaryUtilization(rateLimits, 'codex');

    if (config.barDisplay === 'both') {
      if (claudePct !== null && codexPct !== null) {
        if (config.percentageFormat === 'compact') {
          parts.push(`${claudePct} · ${codexPct}`);
        } else {
          parts.push(`Claude Code ${claudePct}%  Codex ${codexPct}%`);
        }
      }
    } else if (config.barDisplay === 'single') {
      const pct = primaryUtilization(rateLimits, config.barProvider);
      if (pct !== null) {
        if (config.percentageFormat === 'compact') {
          parts.push(`${pct}`);
        } else {
          const name = config.barProvider === 'claude' ? 'Claude Code' : 'Codex';
          parts.push(`${name} ${pct}%`);
        }
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
