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

  return window?.utilization ?? null;
}
