import type { RateLimitsPayload, UsageProvider } from "./types/index.js";

function fiveHourWindowsForProvider(
  rateLimits: RateLimitsPayload,
  provider: UsageProvider,
) {
  if (provider === "claude") {
    return (rateLimits.claude?.windows ?? []).filter((w) => w.windowId === "five_hour");
  }

  if (provider === "codex") {
    return (rateLimits.codex?.windows ?? []).filter((w) => w.windowId === "primary");
  }

  return [];
}

export function footerFiveHourPct(
  rateLimits: RateLimitsPayload | null | undefined,
  provider: UsageProvider,
): number | null {
  if (!rateLimits) return null;

  const windows = fiveHourWindowsForProvider(rateLimits, provider);
  if (windows.length === 0) return null;

  return Math.max(...windows.map((w) => w.utilization));
}
