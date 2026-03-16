import { invoke } from "@tauri-apps/api/core";
import { activePeriod, activeProvider } from "./stores/usage.js";
import { applyTheme, type Settings } from "./stores/settings.js";

type StartupDeps = {
  invokeFn?: typeof invoke;
  applyThemeFn?: typeof applyTheme;
};

export async function initializeRuntimeFromSettings(
  saved: Settings,
  deps: StartupDeps = {},
) {
  const invokeFn = deps.invokeFn ?? invoke;
  const applyThemeFn = deps.applyThemeFn ?? applyTheme;

  applyThemeFn(saved.theme);
  activeProvider.set(saved.defaultProvider);
  activePeriod.set(saved.defaultPeriod);

  try {
    await invokeFn("set_refresh_interval", { interval: saved.refreshInterval });
    await invokeFn("set_show_tray_amount", { show: saved.showTrayAmount });
  } catch {
    // Keep startup resilient if the backend IPC is not ready yet.
  }

  return {
    provider: saved.defaultProvider,
    period: saved.defaultPeriod,
  };
}
