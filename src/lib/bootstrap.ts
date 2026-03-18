import { invoke } from "@tauri-apps/api/core";
import { activePeriod, activeProvider } from "./stores/usage.js";
import { applyTheme, type Settings } from "./stores/settings.js";
import { syncNativeWindowSurface } from "./windowAppearance.js";

type StartupDeps = {
  invokeFn?: typeof invoke;
  applyThemeFn?: typeof applyTheme;
  syncNativeWindowSurfaceFn?: (invokeFn?: typeof invoke) => Promise<void>;
};

export async function initializeRuntimeFromSettings(
  saved: Settings,
  deps: StartupDeps = {},
) {
  const invokeFn = deps.invokeFn ?? invoke;
  const applyThemeFn = deps.applyThemeFn ?? applyTheme;
  const syncNativeWindowSurfaceFn =
    deps.syncNativeWindowSurfaceFn ?? syncNativeWindowSurface;

  applyThemeFn(saved.theme);
  activeProvider.set(saved.defaultProvider);
  activePeriod.set(saved.defaultPeriod);

  try {
    await syncNativeWindowSurfaceFn(invokeFn);
  } catch {
    // Keep startup resilient if the backend IPC is not ready yet.
  }

  try {
    await invokeFn("set_refresh_interval", { interval: saved.refreshInterval });
    await invokeFn("set_tray_config", { config: saved.trayConfig });
  } catch {
    // Keep startup resilient if the backend IPC is not ready yet.
  }

  return {
    provider: saved.defaultProvider,
    period: saved.defaultPeriod,
  };
}
