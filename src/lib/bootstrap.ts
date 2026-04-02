import { invoke } from "@tauri-apps/api/core";
import { activePeriod, activeProvider } from "./stores/usage.js";
import { applyGlass, applyTheme, resolveVisibleProvider, type Settings } from "./stores/settings.js";
import { syncTrayConfig } from "./tray/sync.js";
import { syncNativeWindowSurface } from "./window/appearance.js";
import { isMacOS, isWindows } from "./utils/platform.js";
import { logger } from "./utils/logger.js";

type StartupDeps = {
  invokeFn?: typeof invoke;
  applyThemeFn?: typeof applyTheme;
  applyGlassFn?: typeof applyGlass;
  syncNativeWindowSurfaceFn?: (invokeFn?: typeof invoke, glassEnabled?: boolean) => Promise<void>;
};

export async function initializeRuntimeFromSettings(
  saved: Settings,
  deps: StartupDeps = {},
) {
  const invokeFn = deps.invokeFn ?? invoke;
  const applyThemeFn = deps.applyThemeFn ?? applyTheme;
  const applyGlassFn = deps.applyGlassFn ?? applyGlass;
  const syncNativeWindowSurfaceFn =
    deps.syncNativeWindowSurfaceFn ?? syncNativeWindowSurface;
  const provider = resolveVisibleProvider(saved.defaultProvider, saved.headerTabs);

  // Initialize frontend logger
  logger.setIpcReady();
  if (saved.debugLogging) {
    logger.setLevel("debug");
  }
  logger.info("bootstrap", `Initializing: provider=${provider}, period=${saved.defaultPeriod}, theme=${saved.theme}`);

  applyThemeFn(saved.theme);
  applyGlassFn(saved.glassEffect);
  activeProvider.set(provider);
  activePeriod.set(saved.defaultPeriod);

  if (isMacOS()) {
    // Fire all macOS-only IPC calls concurrently — they are independent.
    await Promise.allSettled([
      invokeFn("set_glass_effect", { enabled: saved.glassEffect }),
      invokeFn("set_dock_icon_visible", { visible: saved.showDockIcon }),
      syncNativeWindowSurfaceFn(invokeFn, saved.glassEffect),
    ]);
  }

  const calls: Promise<unknown>[] = [
    invokeFn("set_refresh_interval", { interval: saved.refreshInterval }),
    syncTrayConfig(saved.trayConfig, null, invokeFn),
  ];
  if (saved.sshHosts.length > 0) {
    calls.push(invokeFn("init_ssh_hosts", { hosts: saved.sshHosts }));
  }
  if (saved.floatBall) {
    calls.push(invokeFn("create_float_ball"));
  }
  if (isWindows() && saved.taskbarPanel) {
    calls.push(invokeFn("init_taskbar_panel"));
  }
  await Promise.allSettled(calls);

  // Sync debug log level to Rust backend
  if (saved.debugLogging) {
    invokeFn("set_log_level", { level: "debug" }).catch(() => {});
  }

  logger.info("bootstrap", "Initialization complete");

  return {
    provider,
    period: saved.defaultPeriod,
  };
}
