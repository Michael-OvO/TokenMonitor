import { invoke } from "@tauri-apps/api/core";
import type { DefaultProvider, RateLimitsPayload, TrayConfig } from "./types/index.js";

type InvokeFn = typeof invoke;

export function primaryUtilization(
  rateLimits: RateLimitsPayload | null,
  provider: DefaultProvider,
): number | null {
  const data = rateLimits?.[provider];
  if (!data || data.windows.length === 0) return null;
  return data.windows[0].utilization;
}

export function trayConfigPayload(
  config: TrayConfig,
  rateLimits: RateLimitsPayload | null,
) {
  return {
    config,
    claudeUtil: primaryUtilization(rateLimits, "claude"),
    codexUtil: primaryUtilization(rateLimits, "codex"),
  };
}

export async function syncTrayConfig(
  config: TrayConfig,
  rateLimits: RateLimitsPayload | null,
  invokeFn: InvokeFn = invoke,
) {
  await invokeFn("set_tray_config", trayConfigPayload(config, rateLimits));
}
