import type { RateLimitsPayload, UsageProvider } from "./types/index.js";
import { providerPayload } from "./rateLimitMonitor.js";
import { currentRateLimitWindows } from "./rateLimitsView.js";

function fiveHourWindowId(provider: "claude" | "codex"): string {
  return provider === "claude" ? "five_hour" : "primary";
}

export function footerFiveHourPct(
  rateLimits: RateLimitsPayload | null | undefined,
  provider: UsageProvider,
  now = Date.now(),
): number | null {
  if (!rateLimits || provider === "all") return null;

  const windowId = fiveHourWindowId(provider);
  const window = currentRateLimitWindows(providerPayload(rateLimits, provider), now)
    .find((candidate) => candidate.windowId === windowId);

  return window?.utilization ?? null;
}
