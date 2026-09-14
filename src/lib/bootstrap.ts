import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { activePeriod, activeProvider } from "./stores/usage.js";
import { applyGlass, applyTheme, resolveVisibleProvider, updateSetting, type Settings } from "./stores/settings.js";
import { syncTrayConfig } from "./tray/sync.js";
import { effectiveIntegrationIds } from "./providerMetadata.js";
import {
  setNativeGlassEffect,
  syncNativeWindowSurface,
  syncNativeWindowTheme,
} from "./window/appearance.js";
import { isMacOS } from "./utils/platform.js";
import { logger } from "./utils/logger.js";
import { hydrateUpdater, installUpdaterListeners } from "./stores/updater.js";
import { setRates } from "./utils/format.js";

/**
 * Dev-only console helpers for testing the onboarding flow without
 * juggling env vars. Open DevTools (Cmd+Opt+I in `tauri dev`) and run:
 *
 *   __tmForceOnboard()       — write hasSeenWelcome=false +
 *                              lastOnboardedVersion="0.0.0" to disk and
 *                              reload, simulating a major upgrade. The
 *                              wizard opens with What's New as step 1.
 *   __tmResetOnboarding()    — clear the version stamp (null) so the
 *                              fresh-install Welcome step shows instead
 *                              of What's New.
 *   __tmCheckOnboardingFlag()— prints whether the VITE_TM_FORCE_ONBOARDING
 *                              env var made it into this build, useful for
 *                              debugging when the env-var path silently
 *                              failed to take effect.
 *
 * Production builds skip the install thanks to the DEV gate. These never
 * leak to end users.
 */
function installDevOnboardingHelpers(): void {
  if (!import.meta.env.DEV) return;
  // Vitest runs in a node environment without a `window` global. Guard
  // against that so the bootstrap function still runs in unit tests.
  if (typeof window === "undefined") return;
  // Avoid TS narrowing complaints by typing the host explicitly.
  const w = window as unknown as Record<string, unknown>;
  w.__tmForceOnboard = async () => {
    await updateSetting("hasSeenWelcome", false);
    await updateSetting("lastOnboardedVersion", "0.0.0");
    logger.info("dev", "Forced onboarding via __tmForceOnboard — reloading");
    location.reload();
  };
  w.__tmResetOnboarding = async () => {
    await updateSetting("hasSeenWelcome", false);
    await updateSetting("lastOnboardedVersion", null);
    logger.info("dev", "Reset onboarding to fresh-install state — reloading");
    location.reload();
  };
  w.__tmCheckOnboardingFlag = () => {
    const v = import.meta.env.VITE_TM_FORCE_ONBOARDING;
    // eslint-disable-next-line no-console
    console.info(
      `[TokenMonitor] VITE_TM_FORCE_ONBOARDING=${JSON.stringify(v)} (DEV=${import.meta.env.DEV})`,
    );
  };
}

type StartupDeps = {
  invokeFn?: typeof invoke;
  listenFn?: typeof listen;
  applyThemeFn?: typeof applyTheme;
  applyGlassFn?: typeof applyGlass;
  syncNativeWindowThemeFn?: (theme: Settings["theme"]) => Promise<void>;
  syncNativeWindowSurfaceFn?: () => Promise<void>;
};

export async function initializeRuntimeFromSettings(
  saved: Settings,
  deps: StartupDeps = {},
) {
  const invokeFn = deps.invokeFn ?? invoke;
  const listenFn = deps.listenFn ?? listen;
  const applyThemeFn = deps.applyThemeFn ?? applyTheme;
  const applyGlassFn = deps.applyGlassFn ?? applyGlass;
  const syncNativeWindowThemeFn =
    deps.syncNativeWindowThemeFn ?? syncNativeWindowTheme;
  const syncNativeWindowSurfaceFn =
    deps.syncNativeWindowSurfaceFn ?? syncNativeWindowSurface;
  const provider = resolveVisibleProvider(saved.defaultProvider, saved.headerTabs);

  // Initialize frontend logger
  logger.setIpcReady();
  if (saved.debugLogging) {
    logger.setLevel("debug");
  }
  logger.info("bootstrap", `Initializing: provider=${provider}, period=${saved.defaultPeriod}, theme=${saved.theme}`);

  // Load dynamic exchange rates from Rust backend (non-blocking).
  const pullExchangeRates = (reason: string) =>
    invokeFn<Record<string, number>>("get_exchange_rates")
      .then((rates) => {
        if (rates && Object.keys(rates).length > 0) {
          setRates(rates);
          logger.info("bootstrap", `Exchange rates loaded (${reason}): ${Object.keys(rates).length} currencies`);
        } else {
          // Cold start with no cache on disk: the backend is still fetching.
          // The `exchange-rates-updated` listener below picks them up — until
          // then `format.ts` runs on its hardcoded fallback table.
          logger.info("bootstrap", `Exchange rates not ready yet (${reason}); awaiting backend fetch`);
        }
      })
      .catch((e) => logger.debug("bootstrap", `Exchange rates fetch failed (${reason}): ${e}`));

  pullExchangeRates("startup");

  // Rates land asynchronously on a first launch and again on the 24h refresh.
  // Without this the webview would keep whatever it read at startup — on a
  // fresh install, the fallback table, for the entire session.
  listenFn("exchange-rates-updated", () => {
    void pullExchangeRates("backend-refresh");
  }).catch((e) => logger.debug("bootstrap", `Exchange rate listener failed: ${e}`));

  // Wire dev-only onboarding helpers onto `window`. No-op in production.
  installDevOnboardingHelpers();

  applyThemeFn(saved.theme);
  applyGlassFn(saved.glassEffect);
  activeProvider.set(provider);
  activePeriod.set(saved.defaultPeriod);

  // Keep native chrome and the webview surface aligned with the selected theme.
  await Promise.allSettled([
    setNativeGlassEffect(saved.glassEffect),
    syncNativeWindowThemeFn(saved.theme),
    syncNativeWindowSurfaceFn(),
  ]);

  if (isMacOS()) {
    // Fire macOS-only IPC calls concurrently — they are independent.
    await Promise.allSettled([
      invokeFn("set_dock_icon_visible", { visible: saved.showDockIcon }),
    ]);
  }

  // Enable usage access + Cursor auth before the first tray cost sync so the
  // menu bar does not flash `$0` while access/auth are still gated off.
  // Parser runs only when both gates are true: the user has finished
  // onboarding (`hasSeenWelcome`) AND the user-controlled toggle in
  // Settings → Privacy & Permissions (`usageAccessEnabled`) is on.
  await Promise.allSettled([
    invokeFn("set_usage_access_enabled", {
      enabled: saved.hasSeenWelcome && saved.usageAccessEnabled,
    }),
    invokeFn("set_cursor_auth_config", {
      apiKey: saved.cursorApiKey,
    }),
    // The tray title, the Cursor meter label and the float-ball amount are
    // rendered in Rust, which cannot read the settings store. Push the currency
    // before the first tray sync so the menu bar never flashes the wrong symbol.
    invokeFn("set_currency", { code: saved.currency }),
  ]);

  const calls: Promise<unknown>[] = [
    invokeFn("set_refresh_interval", { interval: saved.refreshInterval }),
    invokeFn("set_rate_limits_enabled", { enabled: saved.rateLimitsEnabled }),
    invokeFn("set_enabled_integrations", {
      ids: effectiveIntegrationIds(saved.headerTabs),
    }),
    invokeFn("set_auto_export_config", {
      enabled: saved.autoExportEnabled,
      folder: saved.autoExportFolder,
      hiddenModels: saved.hiddenModels,
    }),
    invokeFn("init_remote_device_include_flags", {
      flags: saved.remoteDeviceIncludes,
    }),
    invokeFn("set_claude_plan_tier", {
      tier: saved.claudePlanTier,
      fiveHourTokens: saved.claudePlanCustomFiveHourTokens,
      weeklyTokens: saved.claudePlanCustomWeeklyTokens,
    }),
    syncTrayConfig(saved.trayConfig, null, invokeFn),
  ];
  if (saved.sshHosts.length > 0) {
    calls.push(invokeFn("init_ssh_hosts", { hosts: saved.sshHosts }));
  }
  if (saved.floatBall) {
    calls.push(invokeFn("create_float_ball"));
  }
  await Promise.allSettled(calls);

  // Sync debug log level to Rust backend
  if (saved.debugLogging) {
    invokeFn("set_log_level", { level: "debug" }).catch((e) => logger.debug("bootstrap", `set_log_level failed: ${e}`));
  }

  // Wire updater listeners + initial status pull.
  await installUpdaterListeners();
  await hydrateUpdater();

  logger.info("bootstrap", "Initialization complete");

  return {
    provider,
    period: saved.defaultPeriod,
  };
}
