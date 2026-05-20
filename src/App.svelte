<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { get } from "svelte/store";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { logger } from "./lib/utils/logger.js";
  import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
  import {
    activeProvider,
    activePeriod,
    activeOffset,
    usageData,
    isLoading,
    isPlaceholderLoading,
    fetchData,
    warmCache,
    warmAllPeriods,
    clearUsageCache,
    clearUsageCacheForProviders,
    seedUsageCache,
  } from "./lib/stores/usage.js";
  import {
    ALL_USAGE_PROVIDER_ID,
    DEFAULT_USAGE_PROVIDER,
    getAdjacentWarmProviders,
    getRateLimitIdleSummary,
    getUsageProviderLabel,
    getUsageProviderTitle,
    isRateLimitProvider,
    rateLimitProvidersForScope,
  } from "./lib/providerMetadata.js";

  import {
    rateLimitsData,
    rateLimitsRequestState,
    hydrateRateLimits,
    fetchRateLimits,
  } from "./lib/stores/rateLimits.js";
  import { providerPayload } from "./lib/views/rateLimitMonitor.js";
  import { hasRateLimitWindows } from "./lib/views/rateLimits.js";
  import { setDeviceIncludeFlag, setSshHostIncludeFlag } from "./lib/views/deviceStats.js";
  import {
    DEFAULT_HEADER_TABS,
    areHeaderTabsEqual,
    applyProvider,
    getLastWindowHeight,
    getVisibleHeaderProviders,
    loadSettings,
    resolveVisibleProvider,
    setLastWindowHeight,
    settings,
    updateSetting,
  } from "./lib/stores/settings.js";
  import { initializeRuntimeFromSettings } from "./lib/bootstrap.js";
  import { syncTrayConfig } from "./lib/tray/sync.js";
  import { DEFAULT_MAX_WINDOW_HEIGHT } from "./lib/windowSizing.js";
  import { createResizeOrchestrator, type ResizeOrchestrator } from "./lib/resizeOrchestrator.js";
  import { syncNativeWindowSurface } from "./lib/window/appearance.js";
  import {
    captureResizeDebugSnapshot,
    formatDebugError,
    initResizeDebug,
    isResizeDebugEnabled,
    logResizeDebug,
  } from "./lib/uiStability.js";
  import { setupAppEventListeners } from "./lib/appEventListeners.js";
  import { isMacOS, isWindows } from "./lib/utils/platform.js";
  import {
    markClaudeKeychainAccessHandled,
    requestClaudeKeychainAccessOnce,
  } from "./lib/permissions/keychain.js";
  import { installStatusline, checkStatusline, type InstalledState } from "./lib/permissions/statusline.js";

  import Toggle from "./lib/components/Toggle.svelte";
  import TimeTabs from "./lib/components/TimeTabs.svelte";
  import MetricsRow from "./lib/components/MetricsRow.svelte";
  import Chart from "./lib/components/Chart.svelte";
  import UsageBars from "./lib/components/UsageBars.svelte";
  import Breakdown from "./lib/components/Breakdown.svelte";
  import Footer from "./lib/components/Footer.svelte";
  import SetupScreen from "./lib/components/SetupScreen.svelte";
  import SplashScreen from "./lib/components/SplashScreen.svelte";
  import Settings from "./lib/components/Settings.svelte";
  import Calendar from "./lib/components/Calendar.svelte";
  import DateNav from "./lib/components/DateNav.svelte";
  import DevicesView from "./lib/components/DevicesView.svelte";
  import SingleDeviceView from "./lib/components/SingleDeviceView.svelte";
  import UpdateBanner from "./lib/components/UpdateBanner.svelte";
  import PermissionDisclosure from "./lib/components/PermissionDisclosure.svelte";
  import PermissionsOnboarding from "./lib/components/PermissionsOnboarding.svelte";
  import type { HeaderTabs, UsagePayload, UsagePeriod, UsageProvider, RateLimitsPayload } from "./lib/types/index.js";

  let showSplash = $state(true);
  let appReady = $state(false);
  let showSettings = $state(false);
  let showCalendar = $state(false);
  let showDevices = $state(false);
  let selectedDevice = $state<string | null>(null);
  let provider = $state<UsageProvider>(DEFAULT_USAGE_PROVIDER);
  let period = $state<UsagePeriod>("day");
  let offset = $state(0);
  let data = $state($usageData);
  let loading = $state(false);
  let placeholderLoading = $state(false);
  let showRefresh = $state(false);
  let rateLimits = $state<RateLimitsPayload | null>(null);
  let rateLimitsRequest = $state({
    loading: false,
    loaded: false,
    error: null as string | null,
    deferredUntil: null as string | null,
  });
  let showKeychainPermissionPanel = $state(false);
  let keychainPermissionBusy = $state(false);
  let statuslineBusy = $state(false);
  let statuslineInstalled = $state<boolean | null>(null);
  let statuslineProbeStatus = $state<InstalledState["status"] | null>(null);
  /** Bartender-style onboarding wizard. Driven by the `hasSeenWelcome`
   * setting now that the test-only auto-open `$effect` has been removed
   * — first launch shows the wizard, every subsequent launch skips it. */
  let showPermissionsOnboarding = $derived(!$settings.hasSeenWelcome);

  async function handleOnboardingFinish() {
    // Order matters: enable parser → wait for the first usage fetch to
    // populate `data` → only then flip `hasSeenWelcome`. The derived
    // `showPermissionsOnboarding` watches that flag, so flipping it is
    // what unmounts the wizard. Doing it last guarantees the dashboard
    // has data ready and the user never sees a "No usage data found"
    // flash between wizard close and first IPC return.
    await invoke("set_usage_access_enabled", { enabled: true }).catch((err) => {
      logger.error("permissions", `Failed to enable usage access: ${err}`);
    });
    await loadInitialData();
    await updateSetting("hasSeenWelcome", true);
    await tick();
    syncSizeAndVerify("onboarding-finish");
  }

  let brandTheming = $state(true);
  let headerTabs = $state<HeaderTabs>(DEFAULT_HEADER_TABS);
  let popEl: HTMLDivElement | null = null;
  let resizeOrch: ResizeOrchestrator | null = null;
  let scrollThresholdH = $state(DEFAULT_MAX_WINDOW_HEIGHT);
  // Written by orchestrator callback; read by future scroll-lock UI indicator.
  // @ts-expect-error Assigned via callback, read access planned
  let isScrollLocked = $state(false);
  let dismissedWarningText = $state<string | null>(null);
  let deviceToggleGuard = 0;
  let initialDataLoad: Promise<void> | null = null;
  const REMOTE_USAGE_CACHE_PROVIDERS: UsageProvider[] = [ALL_USAGE_PROVIDER_ID, "claude"];

  // ── View transition logic ──
  type ViewKey = 'splash' | 'welcome' | 'setup' | 'settings' | 'calendar' | 'single-device' | 'devices' | 'main' | 'loading';
  const DRILL_VIEWS: ViewKey[] = ['settings', 'calendar', 'devices', 'single-device'];

  let viewKey = $derived.by<ViewKey>(() => {
    if (showSplash) return 'splash';
    if (!$settings.hasSeenWelcome) return 'welcome';
    if (appReady && !data) return 'setup';
    if (showSettings) return 'settings';
    if (showCalendar) return 'calendar';
    if (selectedDevice) return 'single-device';
    if (showDevices) return 'devices';
    if (data) return 'main';
    return 'loading';
  });

  let prevViewKey: ViewKey = 'splash';
  let viewTransitionClass = $state('');

  const NO_TRANSITION_VIEWS: ViewKey[] = ['splash', 'loading'];

  $effect(() => {
    const current = viewKey;
    const prev = untrack(() => prevViewKey);
    if (current === prev) return;

    if (NO_TRANSITION_VIEWS.includes(current) || NO_TRANSITION_VIEWS.includes(prev)) {
      viewTransitionClass = '';
    } else {
      const isDrillIn = DRILL_VIEWS.includes(current) && !DRILL_VIEWS.includes(prev);
      const isDrillOut = !DRILL_VIEWS.includes(current) && DRILL_VIEWS.includes(prev);
      const isDeeper = DRILL_VIEWS.indexOf(current) > DRILL_VIEWS.indexOf(prev) && DRILL_VIEWS.includes(current) && DRILL_VIEWS.includes(prev);

      if (isDrillIn || isDeeper) {
        viewTransitionClass = 'view-enter-right';
      } else if (isDrillOut) {
        viewTransitionClass = 'view-enter-left';
      } else {
        viewTransitionClass = 'view-enter-fade';
      }
      resizeOrch?.followContentDuringTransition(200, `view-transition-${current}`);
    }
    prevViewKey = current;
  });

  // ── Data content fade on provider/period/offset switch ──
  let dataKey = $derived(`${provider}-${period}-${offset}`);
  let dataTransitionCounter = $state(0);

  $effect(() => {
    void dataKey;
    dataTransitionCounter = untrack(() => dataTransitionCounter) + 1;
  });

  let headerToggleOptions = $derived.by(() =>
    getVisibleHeaderProviders(headerTabs).map((value) => ({
      value,
      label: headerTabs[value].label,
    })),
  );
  let visibleUsableRateLimitProviders = $derived.by(() =>
    rateLimitProvidersForScope(provider).filter((candidate) =>
      hasRateLimitWindows(providerPayload(rateLimits, candidate)),
    ),
  );
  // Subscribe to stores
  $effect(() => {
    const unsub1 = usageData.subscribe((v) => { if (deviceToggleGuard === 0) data = v; });
    const unsub2 = isLoading.subscribe((v) => (loading = v));
    const unsubPL = isPlaceholderLoading.subscribe((v) => (placeholderLoading = v));
    const unsub3 = settings.subscribe((s) => {
      brandTheming = s.brandTheming;
      if (!areHeaderTabsEqual(headerTabs, s.headerTabs)) {
        headerTabs = s.headerTabs;
      }
    });
    const unsub4 = rateLimitsData.subscribe((v) => (rateLimits = v));
    const unsub5 = rateLimitsRequestState.subscribe((v) => (rateLimitsRequest = v));
    return () => { unsub1(); unsub2(); unsubPL(); unsub3(); unsub4(); unsub5(); };
  });

  // Apply/remove data-provider attribute reactively
  $effect(() => {
    applyProvider(provider, brandTheming);
  });

  $effect(() => {
    const nextProvider = resolveVisibleProvider(provider, headerTabs);
    if (nextProvider !== provider) {
      void handleProviderChange(nextProvider);
    }
  });

  // Only show refresh indicator after 300ms — hides it entirely for
  // cache-warm loads that resolve in milliseconds.
  $effect(() => {
    if (loading && data) {
      const timer = setTimeout(() => { showRefresh = true; }, 300);
      return () => { clearTimeout(timer); showRefresh = false; };
    } else {
      showRefresh = false;
    }
  });

  function emptyPeriodLabel(p: UsagePeriod, o: number): string {
    if (o === 0) {
      if (p === "day") return "Clean slate today";
      if (p === "week") return "Nothing this week yet";
      if (p === "month") return "Nothing this month yet";
      return "No usage yet";
    }
    if (p === "day") return "A quiet day";
    return "No usage data for this period";
  }

  function providerNotInstalledTitle(p: UsageProvider): string {
    return `${getUsageProviderTitle(p)} not installed`;
  }

  function providerNotInstalledHint(p: UsageProvider): string {
    if (p === "claude") return "Install Claude Code CLI to start tracking usage.";
    if (p === "codex") return "Install Codex CLI to start tracking usage.";
    if (p === "cursor") return "Install Cursor IDE to start tracking usage.";
    return "Install the provider to start tracking usage.";
  }

  async function loadInitialData() {
    if (initialDataLoad) return initialDataLoad;

    initialDataLoad = (async () => {
      await fetchData(provider, period, offset);
      logResizeDebug("app:data-ready", { provider, period, offset });

      if (period === "5h") {
        await fetchRateLimits(provider);
      } else {
        await hydrateRateLimits();
      }

      await syncTrayConfig(get(settings).trayConfig, get(rateLimitsData)).catch((e) => logger.debug("tray", `syncTrayConfig failed: ${e}`));
      warmAllPeriods(provider, period);
      for (const warmProvider of getAdjacentWarmProviders(provider)) {
        warmAllPeriods(warmProvider);
      }
    })();

    return initialDataLoad;
  }

  async function enableRateLimits() {
    showKeychainPermissionPanel = false;
    await updateSetting("rateLimitsEnabled", true);
    await invoke("set_rate_limits_enabled", { enabled: true });
    // Force the fetch — the cached payload may carry a cooldownUntil from a
    // previous CLI rejection that the user has just resolved by re-granting,
    // and the normal eligibility filter would otherwise skip the call.
    await fetchRateLimits(provider, { force: true });
    await tick();
    syncSizeAndVerify("rate-limits-enabled");
  }

  async function handleEnableRateLimits() {
    if (keychainPermissionBusy) return;
    keychainPermissionBusy = true;
    try {
      await enableRateLimits();
    } finally {
      keychainPermissionBusy = false;
    }
  }

  async function handleShowKeychainFallback() {
    if (keychainPermissionBusy) return;
    showKeychainPermissionPanel = true;
    await tick();
    syncSizeAndVerify("keychain-permission-open");
  }

  async function handleAllowKeychainForRateLimits() {
    if (keychainPermissionBusy) return;
    keychainPermissionBusy = true;
    try {
      await requestClaudeKeychainAccessOnce("rate-limits");
      await enableRateLimits();
    } finally {
      keychainPermissionBusy = false;
    }
  }

  async function handleSkipKeychainForRateLimits() {
    if (keychainPermissionBusy) return;
    keychainPermissionBusy = true;
    try {
      await markClaudeKeychainAccessHandled();
      await enableRateLimits();
    } finally {
      keychainPermissionBusy = false;
    }
  }

  async function handleInstallStatusline() {
    if (statuslineBusy) return;
    statuslineBusy = true;
    try {
      await installStatusline();
      statuslineInstalled = true;
      statuslineProbeStatus = "installed";
      await updateSetting("statuslineInstalled", true);
    } catch (e) {
      console.error("Statusline install failed:", e);
    } finally {
      statuslineBusy = false;
    }
  }

  async function refreshStatuslineProbe() {
    try {
      const state = await checkStatusline();
      statuslineInstalled = state.status === "installed";
      statuslineProbeStatus = state.status;
      if (statuslineInstalled !== $settings.statuslineInstalled) {
        await updateSetting("statuslineInstalled", statuslineInstalled);
      }
    } catch {
      // non-critical
    }
  }

  async function handleProviderChange(p: UsageProvider) {
    const nextProvider = resolveVisibleProvider(p, headerTabs);
    if (provider === nextProvider) return;
    logger.info("navigation", `Provider: ${nextProvider}`);
    provider = nextProvider;
    activeProvider.set(nextProvider);
    await fetchData(nextProvider, period, offset);
    if (provider !== nextProvider) return;
    if (period === "5h") await fetchRateLimits(nextProvider);
    if (provider !== nextProvider) return;
    await tick();
    syncSizeAndVerify("provider-change");
    warmAllPeriods(nextProvider, period);
    for (const warmProvider of getAdjacentWarmProviders(nextProvider)) {
      warmCache(warmProvider, period);
    }
  }

  async function handlePeriodChange(p: UsagePeriod) {
    if (period === p && offset === 0) return;
    logger.info("navigation", `Period: ${p}`);
    const prov = provider;
    period = p;
    offset = 0;
    activePeriod.set(p);
    activeOffset.set(0);
    await fetchData(prov, p, 0);
    if (period !== p || provider !== prov) return;
    if (p === "5h") await fetchRateLimits(provider);
    if (period !== p || provider !== prov) return;
    await tick();
    syncSizeAndVerify("period-change");
  }

  async function handleOffsetChange(delta: number) {
    logger.info("navigation", `Offset: delta=${delta}`);
    const prov = provider;
    const per = period;
    offset += delta;
    activeOffset.set(offset);
    await fetchData(prov, per, offset);
    if (period !== per || provider !== prov) return;
    await tick();
    syncSizeAndVerify("offset-change");
    // Warm adjacent offsets for instant navigation
    warmCache(prov, per, offset - 1);
    if (offset < 0) warmCache(prov, per, offset + 1);
  }

  async function handleOffsetReset() {
    if (offset === 0) return;
    logger.info("navigation", "Offset reset");
    const prov = provider;
    const per = period;
    offset = 0;
    activeOffset.set(0);
    await fetchData(prov, per, 0);
    if (period !== per || provider !== prov) return;
    await tick();
    syncSizeAndVerify("offset-reset");
  }

  async function handleSettingsOpen() {
    logger.info("navigation", "Settings opened");
    showCalendar = false;
    showSettings = true;
    await tick();
    syncSizeAndVerify("settings-open");
  }

  async function handleSettingsClose() {
    logger.info("navigation", "Settings closed");
    showSettings = false;
    await tick();
    syncSizeAndVerify("settings-close");
  }

  async function handleCalendarOpen() {
    logger.info("navigation", "Calendar opened");
    showSettings = false;
    showCalendar = true;
    await tick();
    syncSizeAndVerify("calendar-open");
  }

  async function handleCalendarClose() {
    logger.info("navigation", "Calendar closed");
    showCalendar = false;
    await tick();
    syncSizeAndVerify("calendar-close");
  }

  function handleDeviceSelect(device: string) {
    logger.info("device", `Selected: ${device}`);
    selectedDevice = device;
  }

  async function handleDeviceBack() {
    logger.info("device", "Device view closed");
    selectedDevice = null;
    await tick();
    syncSizeAndVerify("device-back");
  }

  async function handleToggleDeviceStats(device: string, includeInStats: boolean) {
    logger.info("device", `Stats toggle: ${device} include=${includeInStats}`);
    const previousData = data;
    const previousHosts = get(settings).sshHosts;

    // Guard: prevent background fetchData refreshes from overwriting the
    // optimistic update via the usageData subscription during the async flow.
    // Without this, a pending stale-while-revalidate background refresh can
    // resolve mid-toggle and revert the checkbox via usageData.set(oldData).
    deviceToggleGuard++;
    // Reject any in-flight background refreshes by bumping the request ID.
    clearUsageCacheForProviders(REMOTE_USAGE_CACHE_PROVIDERS);

    // Optimistic UI: immediately flip the checkbox so the user sees instant feedback.
    data = setDeviceIncludeFlag(data, device, includeInStats);

    let failed = false;
    try {
      // 1. Update Rust in-memory state + persist to settings.
      await invoke("toggle_device_include_in_stats", { alias: device, includeInStats });
      const updatedHosts = setSshHostIncludeFlag(previousHosts, device, includeInStats);
      await updateSetting("sshHosts", updatedHosts);

      // 2. Clear only the final usage-view cache so provider aggregates stay warm.
      await invoke("clear_usage_view_cache");

      // 3. Direct IPC fetch + store update (bypasses fetchData's stale-while-revalidate).
      const freshData = await invoke<UsagePayload>("get_usage_data", { provider, period, offset });
      seedUsageCache(provider, period, offset, freshData);
      data = freshData;
      usageData.set(freshData);
    } catch (err) {
      console.error("Failed to toggle device stats:", err);
      failed = true;
      data = previousData;
      if (previousData) {
        seedUsageCache(provider, period, offset, previousData);
        usageData.set(previousData);
      }
    } finally {
      deviceToggleGuard--;
    }

    // Rollback Rust state + settings after the guard is released so the
    // subscription is active again for any store updates from persistence.
    if (failed) {
      try {
        await invoke("toggle_device_include_in_stats", {
          alias: device,
          includeInStats: !includeInStats,
        });
      } catch (rollbackErr) {
        console.error("Failed to rollback device stats toggle:", rollbackErr);
      }
      try {
        await updateSetting("sshHosts", previousHosts);
      } catch (settingsRollbackErr) {
        console.error("Failed to rollback sshHosts setting:", settingsRollbackErr);
      }
    }
  }

  // ── Window resize (delegated to resizeOrchestrator.ts) ──
  //
  // All resize state, measurement, throttling, and animation logic lives in
  // the orchestrator closure. App.svelte only holds the reactive Svelte state
  // (scrollThresholdH, isScrollLocked) which the orchestrator updates via
  // callbacks, and delegates all calls through resizeOrch.

  const tauriWindow = getCurrentWindow();

  /** Convenience wrapper — forwards to orchestrator or no-ops before init. */
  function syncSizeAndVerify(source?: string) {
    resizeOrch?.syncSizeAndVerify(source);
  }

  onMount(() => {
    let cancelled = false;
    let observer: ResizeObserver | undefined;
    let unlisten: (() => void) | undefined;
    let unlistenWindowResize: (() => void) | undefined;

    // Create the resize orchestrator (all resize state lives in its closure)
    let persistedWindowHeight = 0;
    let persistWindowHeightTimer: ReturnType<typeof setTimeout> | null = null;
    const persistWindowHeight = (height: number) => {
      if (height <= 0 || height === persistedWindowHeight) return;
      persistedWindowHeight = height;
      if (persistWindowHeightTimer) clearTimeout(persistWindowHeightTimer);
      persistWindowHeightTimer = setTimeout(() => {
        persistWindowHeightTimer = null;
        void setLastWindowHeight(height);
      }, 400);
    };

    resizeOrch = createResizeOrchestrator({
      getPopEl: () => popEl,
      invoke: (cmd, args) => invoke(cmd, args),
      onScrollLockChange: (locked) => {
        isScrollLocked = locked;
      },
      currentMonitor: () => currentMonitor(),
      logDebug: logResizeDebug,
      captureDebugSnapshot: (reason) =>
        captureResizeDebugSnapshot(reason, popEl, {
          maxWindowH: resizeOrch?.getMaxWindowH() ?? DEFAULT_MAX_WINDOW_HEIGHT,
          scrollThresholdH: resizeOrch?.getScrollThresholdH() ?? DEFAULT_MAX_WINDOW_HEIGHT,
          isScrollLocked: resizeOrch?.getIsScrollLocked() ?? false,
        }),
      formatDebugError,
      isDebugEnabled: isResizeDebugEnabled,
      onHeightApplied: persistWindowHeight,
    });

    /** Local snapshot helper for event handlers that need debug snapshots. */
    const captureSnapshot = (reason: string) =>
      isResizeDebugEnabled()
        ? captureResizeDebugSnapshot(reason, popEl, {
            maxWindowH: resizeOrch?.getMaxWindowH() ?? DEFAULT_MAX_WINDOW_HEIGHT,
            scrollThresholdH: resizeOrch?.getScrollThresholdH() ?? DEFAULT_MAX_WINDOW_HEIGHT,
            isScrollLocked: resizeOrch?.getIsScrollLocked() ?? false,
          })
        : {};

    initResizeDebug();

    const cleanupListeners = setupAppEventListeners({
      onResize: () => {
        logResizeDebug("browser:resize", captureSnapshot("browser-resize"));
      },
      onFocus: () => {
        logResizeDebug("window:focus", captureSnapshot("window-focus"));
        void syncNativeWindowSurface(undefined, get(settings).glassEffect).catch((e) => logger.debug("appearance", `syncNativeWindowSurface failed: ${e}`));
        syncSizeAndVerify("window-focus");
        clearUsageCache();
        fetchData(provider, period, offset);
        if (period === "5h") fetchRateLimits(provider);
      },
      onBlur: () => {
        logResizeDebug("window:blur", captureSnapshot("window-blur"));
      },
      onError: (event) => {
        logResizeDebug("window:error", {
          message: event.message,
          filename: event.filename || null,
          lineno: event.lineno || null,
          colno: event.colno || null,
          error: formatDebugError(event.error),
        });
      },
      onUnhandledRejection: (event) => {
        logResizeDebug("window:unhandledrejection", {
          reason: formatDebugError(event.reason),
        });
      },
      onVisibilityChange: () => {
        logResizeDebug("document:visibility-change", {
          hidden: document.hidden,
          visibilityState: document.visibilityState,
          ...captureSnapshot("document-visibility-change"),
        });
      },
      onColorSchemeChange: (matchesLight) => {
        const followsSystemTheme = !document.documentElement.hasAttribute("data-theme");
        logResizeDebug("theme:system-change", {
          matchesLight,
          followsSystemTheme,
        });

        const updates: Promise<unknown>[] = [syncTrayConfig(get(settings).trayConfig, null)];
        if (followsSystemTheme) {
          updates.push(syncNativeWindowSurface(undefined, get(settings).glassEffect));
        }

        void Promise.allSettled(updates);
      },
    });
    logResizeDebug("app:mount", captureSnapshot("mount"));

    const init = async () => {
      await resizeOrch!.refreshWindowMetrics();
      scrollThresholdH = resizeOrch!.getScrollThresholdH();

      // Load persisted settings and apply theme + defaults (non-blocking)
      try {
        const saved = await loadSettings();
        if (cancelled) return;
        const runtime = await initializeRuntimeFromSettings(saved);
        if (cancelled) return;
        provider = runtime.provider;
        period = runtime.period;
        logResizeDebug("app:settings-loaded", {
          provider: runtime.provider,
          period: runtime.period,
        });
      } catch {
        // Settings load failed — continue with defaults
        logResizeDebug("app:settings-load-failed", {});
      }

      // Restore the window to its last-known height before the chart renders.
      // Combined with the orchestrator's initial-content gate, this hides the
      // "small → big" pop-out users were seeing on cold launch.
      const restoredHeight = await getLastWindowHeight();
      if (!cancelled && restoredHeight) {
        try {
          await invoke("set_window_size_and_align", {
            width: 340,
            height: restoredHeight,
          });
          persistedWindowHeight = restoredHeight;
          logResizeDebug("app:window-height-restored", { height: restoredHeight });
        } catch (e) {
          logResizeDebug("app:window-height-restore-failed", {
            error: formatDebugError(e),
          });
        }
      }

      if (get(settings).hasSeenWelcome) {
        await loadInitialData();
        if (cancelled) return;
      }
      if (isWindows()) {
        try {
          const edge = await invoke<string>("get_window_anchor_edge");
          document.documentElement.setAttribute("data-anchor", edge);
        } catch { /* non-critical */ }
      }

      void refreshStatuslineProbe();
      appReady = true;

      if (popEl) {
        observer = new ResizeObserver((entries) => {
          logResizeDebug("resize:observer-fired", {
            entries: entries.map((entry) => ({
              width: entry.contentRect.width,
              height: entry.contentRect.height,
            })),
            ...captureSnapshot("resize-observer"),
          });
          resizeOrch?.resizeToContent("resize-observer");
        });
        observer.observe(popEl);
        syncSizeAndVerify("initial-mount");
        // Wait one frame so the Chart finishes its first paint, then allow
        // shrink-direction resizes. Until now the orchestrator has buffered
        // shrinks so the window doesn't collapse between "data arrived" and
        // "chart rendered".
        await tick();
        resizeOrch?.markInitialContentReady();
      }

      unlisten = await listen("data-updated", () => {
        logResizeDebug("app:data-updated-event", {
          provider,
          period,
          offset,
        });
        clearUsageCache();
        fetchData(provider, period, offset);
        if (period === "5h") fetchRateLimits(provider);
      });

      unlistenWindowResize = await tauriWindow.onResized(({ payload }) => {
        logResizeDebug("tauri:window-resized", {
          width: payload.width,
          height: payload.height,
          ...captureSnapshot("tauri-window-resized"),
        });
      });

      if (cancelled) {
        unlisten();
        unlisten = undefined;
        unlistenWindowResize?.();
        unlistenWindowResize = undefined;
      }
    };

    function onChartHover(e: Event) {
      const active = (e as CustomEvent<{ active: boolean }>).detail.active;
      resizeOrch?.setChartHoverActive(active);
    }
    window.addEventListener("chart-hover", onChartHover);

    init();

    return () => {
      cancelled = true;
      unlisten?.();
      unlistenWindowResize?.();
      observer?.disconnect();
      window.removeEventListener("chart-hover", onChartHover);
      resizeOrch?.destroy();
      resizeOrch = null;
      cleanupListeners();
    };
  });
</script>

<div class="pop">
  <div
    class="pop-content"
    bind:this={popEl}
    style:max-height="{scrollThresholdH}px"
    style:overflow-y={scrollThresholdH < DEFAULT_MAX_WINDOW_HEIGHT ? 'auto' : 'visible'}
  >
    {#if !showSettings}<UpdateBanner />{/if}
    {#if showSplash}
      <SplashScreen ready={appReady} onComplete={() => { showSplash = false; tick().then(() => syncSizeAndVerify("splash-complete")); }} />
    {:else if showPermissionsOnboarding}
      <PermissionsOnboarding
        onFinish={handleOnboardingFinish}
      />
    {:else if appReady && !data}
      <div class={viewTransitionClass}><SetupScreen /></div>
    {:else if showSettings}
      <div class={viewTransitionClass}><Settings onBack={handleSettingsClose} /></div>
    {:else if showCalendar}
      <div class={viewTransitionClass}><Calendar onBack={handleCalendarClose} /></div>
    {:else if selectedDevice}
      <div class={viewTransitionClass}><SingleDeviceView device={selectedDevice} onBack={handleDeviceBack} /></div>
    {:else if showDevices}
      <div class={viewTransitionClass}><DevicesView onBack={() => { showDevices = false; }} onDeviceSelect={handleDeviceSelect} onSettings={handleSettingsOpen} /></div>
    {:else if data}
      <div class={viewTransitionClass}>
      {#if showRefresh}<div class="refresh-bar" aria-hidden="true"></div>{/if}
      <Toggle
        active={provider}
        options={headerToggleOptions}
        onChange={handleProviderChange}
        {brandTheming}
      />
      <TimeTabs active={period} onChange={handlePeriodChange} />
      {#if period !== "5h" && data}
        <DateNav
          periodLabel={data.period_label}
          hasEarlierData={data.has_earlier_data}
          isAtPresent={offset === 0}
          onBack={() => handleOffsetChange(-1)}
          onForward={() => handleOffsetChange(1)}
          onReset={handleOffsetReset}
        />
      {/if}
      {#if placeholderLoading}
        <div class="loading">
          <div class="spinner"></div>
          <div class="loading-text">Loading data...</div>
        </div>
      {:else}
        <div class="data-content" style:animation-name={dataTransitionCounter > 1 ? 'contentFade' : 'none'}>
        <MetricsRow {data} />
        {#if data.usage_warning && data.usage_warning !== dismissedWarningText}
          <div class="usage-warning">
            <div class="usage-warning-header">
              <div class="usage-warning-title">Usage warning</div>
              <button
                class="usage-warning-dismiss"
                onclick={() => { dismissedWarningText = data?.usage_warning ?? null; }}
                aria-label="Dismiss warning"
              >&times;</button>
            </div>
            <div class="usage-warning-text">{data.usage_warning}</div>
          </div>
        {/if}
        <div class="hr"></div>

        {#if period === "5h"}
          {#if showKeychainPermissionPanel && isMacOS() && !$settings.keychainAccessRequested}
            <div class="rate-limit-permission" role="dialog" aria-labelledby="rate-limit-permission-title">
              <div class="rate-limit-empty-title" id="rate-limit-permission-title">
                Keychain fallback for live limits
              </div>
              <div class="rate-limit-empty-text">
                TokenMonitor normally reads Claude live limits from your Claude
                credentials file without any macOS prompt. If that file is missing
                or unreadable, you can allow a one-time Keychain fallback.
              </div>
              <PermissionDisclosure mode="rate-limit" />
              <div class="rate-limit-empty-text">
                macOS may show a Keychain window after you continue. Choose
                <strong>Always Allow</strong> if you want future fallback checks to stay silent.
              </div>
              <div class="rate-limit-actions">
                <button
                  type="button"
                  class="rate-limit-secondary"
                  onclick={handleSkipKeychainForRateLimits}
                  disabled={keychainPermissionBusy}
                >
                  Do not use Keychain
                </button>
                <button
                  type="button"
                  class="rate-limit-cta"
                  onclick={handleAllowKeychainForRateLimits}
                  disabled={keychainPermissionBusy}
                >
                  Allow Keychain access
                </button>
              </div>
            </div>
          {:else if $settings.rateLimitsEnabled && visibleUsableRateLimitProviders.length > 0}
            {#each visibleUsableRateLimitProviders as rateLimitProvider, index}
              <UsageBars
                providerLabel={provider === ALL_USAGE_PROVIDER_ID ? getUsageProviderLabel(rateLimitProvider) : undefined}
                rateLimits={providerPayload(rateLimits, rateLimitProvider)!}
              />
              {#if index < visibleUsableRateLimitProviders.length - 1}
                <div class="hr"></div>
              {/if}
            {/each}
            {#if statuslineProbeStatus === "script_missing" || statuslineProbeStatus === "not_installed"}
              <div class="rate-limit-stale-banner" data-state="warn">
                <div class="rl-stale-row">
                  <span class="rl-stale-dot" aria-hidden="true"></span>
                  <span class="rl-stale-headline">Statusline needs attention</span>
                </div>
                <div class="rl-stale-body">
                  Reinstall the statusline to restore live updates.
                </div>
                <button
                  type="button"
                  class="rate-limit-cta"
                  onclick={handleInstallStatusline}
                  disabled={statuslineBusy}
                >
                  {statuslineBusy ? "Reinstalling…" : "Reinstall statusline"}
                </button>
              </div>
            {/if}
          {:else if !$settings.rateLimitsEnabled}
            <div class="rate-limit-empty">
              <svg class="empty-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--t4)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect>
                <path d="M7 11V7a5 5 0 0 1 10 0v4"></path>
              </svg>
              <div class="rate-limit-empty-title">Live rate limits are off</div>
              <div class="rate-limit-empty-text">
                Turn this on to see live rate-limit percentages.
                TokenMonitor uses your Claude credentials file first and does not open Keychain from this button.
              </div>
              <button
                type="button"
                class="rate-limit-cta"
                onclick={handleEnableRateLimits}
                disabled={keychainPermissionBusy}
              >
                Enable rate limits
              </button>
            </div>
          {:else if rateLimitsRequest.loading}
            <div class="rate-limit-skeleton" aria-busy="true">
              {#each [1, 2] as _}
                <div class="rate-limit-skeleton-row">
                  <div class="skeleton" style="width: 50px; height: 8px"></div>
                  <div class="skeleton" style="width: 100%; height: 14px; border-radius: 7px"></div>
                </div>
              {/each}
            </div>
          {:else}
            <div class="rate-limit-empty">
              <svg class="empty-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--t4)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="12" cy="12" r="10"></circle>
                <line x1="12" y1="8" x2="12" y2="12"></line>
                <line x1="12" y1="16" x2="12.01" y2="16"></line>
              </svg>
              <div class="rate-limit-empty-title">Rate limits unavailable</div>
              <div class="rate-limit-empty-text">
                {#if isRateLimitProvider(provider) && (data.total_tokens > 0 || data.total_cost > 0)}
                  {getRateLimitIdleSummary(provider)}
                {:else}
                  {rateLimitsRequest.error ?? "Unable to load rate limit data right now."}
                {/if}
              </div>
              {#if isMacOS() && !$settings.keychainAccessRequested}
                <button
                  type="button"
                  class="rate-limit-secondary"
                  onclick={handleShowKeychainFallback}
                  disabled={keychainPermissionBusy}
                >
                  Review Keychain fallback
                </button>
              {/if}
            </div>
          {/if}
        {:else if data.total_cost === 0 && data.total_tokens === 0}
          <div class="empty-period">
            {#if data.provider_detected === false}
              <svg class="empty-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--t4)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                <polyline points="7 10 12 15 17 10"></polyline>
                <line x1="12" y1="15" x2="12" y2="3"></line>
              </svg>
              <span class="empty-title">{providerNotInstalledTitle(provider)}</span>
              <span class="empty-subtitle">{providerNotInstalledHint(provider)}</span>
            {:else}
              <svg class="empty-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--t4)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12 2L2 7l10 5 10-5-10-5z"></path>
                <path d="M2 17l10 5 10-5"></path>
                <path d="M2 12l10 5 10-5"></path>
              </svg>
              <span>{emptyPeriodLabel(period, offset)}</span>
            {/if}
          </div>
        {:else}
          <Chart buckets={data.chart_buckets} dataKey={`${provider}-${period}-${offset}`} deviceBuckets={data.device_chart_buckets} />
        {/if}

        {#if (period !== "5h" && data.model_breakdown.length > 0) || data.subagent_stats || (data.device_breakdown && data.device_breakdown.length > 0)}
          <div class="hr"></div>
          <Breakdown
            models={period !== "5h" ? data.model_breakdown : []}
            onAccordionToggle={(detail) => resizeOrch?.handleBreakdownAccordionToggle(detail)}
            subagentStats={data.subagent_stats}
            deviceBreakdown={data.device_breakdown}
            onDeviceSelect={handleDeviceSelect}
            onShowAllDevices={() => { showDevices = true; }}
            onToggleDeviceStats={handleToggleDeviceStats}
          />
        {/if}
      </div>
      {/if}
      <Footer {data} {provider} {period} {rateLimits} onSettings={handleSettingsOpen} onCalendar={handleCalendarOpen} onDevices={() => { showDevices = true; }} />
      </div>
    {:else}
      <div class="loading">
        <div class="spinner"></div>
        <div class="loading-text">Loading data...</div>
      </div>
    {/if}
  </div>
</div>


<style>
  .pop {
    width: 340px;
    min-height: 200px;
    box-shadow: none;
    animation: popIn var(--t-slow) var(--ease-out) both;
  }
  .pop-content {
    position: relative;
    min-width: 0;
    scrollbar-width: none;
    -ms-overflow-style: none;
  }
  .pop-content::-webkit-scrollbar {
    display: none;
  }
  .hr { height: 1px; background: var(--border-subtle); margin: 0 12px; }
  .loading {
    display: flex; flex-direction: column; align-items: center;
    justify-content: center; padding: 48px 24px;
  }
  .spinner {
    width: 18px; height: 18px;
    border: 2px solid var(--border);
    border-top-color: var(--t2);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    margin-bottom: 10px;
  }
  .loading-text {
    font: 400 10px/1 'Inter', sans-serif;
    color: var(--t3);
  }
  .loading-bars {
    display: flex; align-items: center; justify-content: center;
    padding: 24px 0;
  }
  .rate-limit-empty,
  .rate-limit-permission,
  .rate-limit-stale-banner {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 18px 14px 16px;
    animation: fadeUp .28s ease both .09s;
  }
  .rate-limit-stale-banner {
    padding-top: 8px;
  }
  .rl-stale-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .rl-stale-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--t4);
    flex-shrink: 0;
  }
  .rate-limit-stale-banner[data-state="warn"] .rl-stale-dot {
    background: #d88d31;
  }
  .rl-stale-headline {
    font: 500 10px/1 'Inter', sans-serif;
    color: var(--t2);
  }
  .rl-stale-body {
    font: 400 9px/1.4 'Inter', sans-serif;
    color: var(--t3);
    padding-left: 12px;
  }
  .rate-limit-permission {
    gap: 7px;
  }
  .rate-limit-empty-title {
    font: 500 11px/1 'Inter', sans-serif;
    color: var(--t1);
  }
  .rate-limit-empty-text {
    font: 400 9px/1.4 'Inter', sans-serif;
    color: var(--t3);
  }
  .rate-limit-note {
    margin: 6px 14px 0;
    font: 400 9px/1.35 'Inter', sans-serif;
    color: var(--t3);
  }
  .usage-warning {
    margin: 8px 14px 0;
    padding: 8px 9px;
    border-radius: 8px;
    background: color-mix(in srgb, #d88d31 12%, transparent);
    border: 1px solid color-mix(in srgb, #d88d31 30%, transparent);
  }
  .usage-warning-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 3px;
  }
  .usage-warning-header .usage-warning-title {
    margin-bottom: 0;
  }
  .usage-warning-title {
    font: 600 9px/1.2 'Inter', sans-serif;
    color: var(--t1);
  }
  .usage-warning-dismiss {
    background: none;
    border: none;
    padding: 0;
    width: 18px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--t3);
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
    border-radius: 3px;
    flex-shrink: 0;
  }
  .usage-warning-dismiss:hover {
    color: var(--t1);
    background: var(--surface-hover);
  }
  .usage-warning-text {
    font: 400 8.5px/1.35 'Inter', sans-serif;
    color: var(--t2);
  }
  .rate-limit-cta {
    margin-top: 10px;
    align-self: flex-start;
    padding: 6px 11px;
    border: none;
    border-radius: 6px;
    background: var(--accent, #6366f1);
    color: white;
    font: 500 10px/1 'Inter', sans-serif;
    cursor: pointer;
    transition: filter var(--t-fast) ease;
  }
  .rate-limit-cta:hover:not(:disabled) { filter: brightness(1.08); }
  .rate-limit-cta:disabled,
  .rate-limit-secondary:disabled {
    cursor: default;
    opacity: .55;
  }

  /* Polished re-grant CTA — quieter than the welcome flow's primary button.
     Reads as a soft tinted chip, not a saturated action. */
  .kc-cta {
    margin-top: 8px;
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 11px 6px 9px;
    border: 1px solid var(--accent-soft, rgba(255,255,255,0.10));
    border-radius: 999px;
    background: var(--accent-soft, rgba(255,255,255,0.06));
    color: var(--accent, var(--t1));
    font: 500 10px/1 'Inter', sans-serif;
    letter-spacing: .15px;
    cursor: pointer;
    transition: background var(--t-fast) ease, transform var(--t-fast) ease,
      border-color var(--t-fast) ease;
  }
  .kc-cta:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent, white) 18%, transparent);
    border-color: color-mix(in srgb, var(--accent, white) 30%, transparent);
    transform: translateY(-1px);
  }
  .kc-cta:active:not(:disabled) {
    transform: translateY(0);
  }
  .kc-cta:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .kc-cta-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px; height: 16px;
    color: var(--accent, var(--t1));
    opacity: 0.85;
  }
  .rate-limit-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-top: 7px;
  }
  .rate-limit-actions .rate-limit-cta {
    margin-top: 0;
  }
  .rate-limit-secondary {
    padding: 6px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: transparent;
    color: var(--t2);
    font: 500 10px/1 'Inter', sans-serif;
    cursor: pointer;
    transition: background var(--t-fast) ease, color var(--t-fast) ease;
  }
  .rate-limit-secondary:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--t1);
  }
  .refresh-bar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 2px;
    z-index: 2;
    pointer-events: none;
    background: linear-gradient(90deg, transparent 0%, var(--t3) 50%, transparent 100%);
    background-size: 200% 100%;
    animation: shimmer 1.2s ease-in-out infinite;
    border-radius: 14px 14px 0 0;
  }
  @keyframes shimmer {
    0% { background-position: 200% 0; }
    100% { background-position: -200% 0; }
  }
  .empty-period {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    text-align: center;
    color: var(--t3);
    font: 400 10px/1 'Inter', sans-serif;
    padding: 32px 0;
  }

  /* ── View entrance transitions ── */
  .view-enter-fade {
    animation: viewFadeIn var(--t-normal) var(--ease-out) both;
  }
  .view-enter-right {
    animation: viewSlideInRight var(--t-normal) var(--ease-out) both;
  }
  .view-enter-left {
    animation: viewSlideInLeft var(--t-normal) var(--ease-out) both;
  }

  /* ── Data content fade on provider/period switch ── */
  .data-content {
    animation-duration: var(--t-normal);
    animation-timing-function: var(--ease-out);
    animation-fill-mode: both;
  }

  /* ── Empty state icons ── */
  .empty-icon {
    display: block;
    margin-bottom: 2px;
    opacity: 0.6;
  }
  .rate-limit-empty .empty-icon {
    margin-bottom: 6px;
  }
  .empty-title {
    font: 600 11px/1 'Inter', sans-serif;
    color: var(--t2);
  }
  .empty-subtitle {
    font: 400 10px/1.4 'Inter', sans-serif;
    color: var(--t3);
    max-width: 220px;
  }

  /* ── Rate limit skeleton loading ── */
  .rate-limit-skeleton {
    padding: 14px 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .rate-limit-skeleton-row {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
</style>
