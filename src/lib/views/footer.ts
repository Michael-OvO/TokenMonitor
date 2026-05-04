import { getRateLimitPrimaryWindowId, isRateLimitProvider } from "../providerMetadata.js";
import type { RateLimitsPayload, UsageProvider } from "../types/index.js";
import { providerPayload } from "./rateLimitMonitor.js";
import { currentRateLimitWindows } from "./rateLimits.js";

export function footerFiveHourPct(
  rateLimits: RateLimitsPayload | null | undefined,
  provider: UsageProvider,
  now = Date.now(),
): number | null {
  if (!rateLimits || !isRateLimitProvider(provider)) return null;

  const windowId = getRateLimitPrimaryWindowId(provider);
  const window = currentRateLimitWindows(providerPayload(rateLimits, provider), now)
    .find((candidate) => candidate.windowId === windowId);

  // CC's statusline serializes integer-looking percentages through a
  // float-multiply (e.g. `0.14 * 100 → 14.000000000000002`). Without
  // rounding here the footer renders the raw artifact as
  // "5h · 14.000000000000002% used". UsageBars dodges this via
  // `formatRateLimitUtilizationLabel`, but the footer template
  // interpolates the raw number, so we round in the view layer to keep
  // a single rule and out of the templates. One-decimal precision matches
  // the display rounding used elsewhere (±0.05%).
  const raw = window?.utilization;
  if (raw == null) return null;
  return Math.round(raw * 10) / 10;
}
