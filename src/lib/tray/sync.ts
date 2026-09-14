import { invoke } from "@tauri-apps/api/core";
import type { RateLimitProviderId, RateLimitsPayload, TrayConfig } from "../types/index.js";

type InvokeFn = typeof invoke;

export function primaryUtilization(
  rateLimits: RateLimitsPayload | null,
  provider: RateLimitProviderId,
): number | null {
  const data = rateLimits?.[provider];
  if (!data || data.windows.length === 0) return null;
  return data.windows[0].utilization;
}

export function trayConfigPayload(
  config: TrayConfig,
  rateLimits: RateLimitsPayload | null,
) {
  // Every rate-limit provider has to be sent: the tray renders bars and
  // percentages from these fields alone, so an omitted provider is invisible
  // in the menu bar no matter what the user selected in settings.
  return {
    config,
    claudeUtil: primaryUtilization(rateLimits, "claude"),
    codexUtil: primaryUtilization(rateLimits, "codex"),
    cursorUtil: primaryUtilization(rateLimits, "cursor"),
  };
}

export async function syncTrayConfig(
  config: TrayConfig,
  rateLimits: RateLimitsPayload | null,
  invokeFn: InvokeFn = invoke,
) {
  await invokeFn("set_tray_config", trayConfigPayload(config, rateLimits));
}
