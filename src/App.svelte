<script lang="ts">
  import { onMount, tick } from "svelte";
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
    fetchData,
    warmCache,
    warmAllPeriods,
    clearUsageCacheForProviders,
    seedUsageCache,
  } from "./lib/stores/usage.js";
  import {
    ALL_USAGE_PROVIDER_ID,
    DEFAULT_USAGE_PROVIDER,
    getAdjacentWarmProviders,
    getRateLimitIdleSummary,
    getUsageProviderLabel,
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
  import { setDeviceIncludeFlag, setSshHostIncludeFlag } from "./lib/views/deviceStats.js";
  import {
    DEFAULT_HEADER_TABS,
    areHeaderTabsEqual,
    applyProvider,
    getVisibleHeaderProviders,
    loadSettings,
    resolveVisibleProvider,
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

  import Toggle from "./lib/components/Toggle.svelte";
  import TimeTabs from "./lib/components/TimeTabs.svelte";
  import MetricsRow from "./lib/components/MetricsRow.svelte";
  import Chart from "./lib/components/Chart.svelte";
  import UsageBars from "./lib/components/UsageBars.svelte";
  import Breakdown from "./lib/components/Breakdown.svelte";
  import Footer from "./lib/components/Footer.svelte";
  import SetupScreen from "./lib/components/SetupScreen.svelte";
  import SplashScreen from "./lib/components/SplashScreen.svelte";
  import WelcomeCard from "./lib/components/WelcomeCard.svelte";
  import Settings from "./lib/components/Settings.svelte";
  import Calendar from "./lib/components/Calendar.svelte";
  import DateNav from "./lib/components/DateNav.svelte";
  import DevicesView from "./lib/components/DevicesView.svelte";
  import SingleDeviceView from "./lib/components/SingleDeviceView.svelte";
  import UpdateBanner from "./lib/components/UpdateBanner.svelte";
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
  let showRefresh = $state(false);
  let rateLimits = $state<RateLimitsPayload | null>(null);
  let rateLimitsRequest = $state({
    loading: false,
    loaded: false,
    error: null as string | null,
    deferredUntil: null as string | null,
  });
  let brandTheming = $state(true);
  let headerTabs = $state<HeaderTabs>(DEFAULT_HEADER_TABS);
  let popEl: HTMLDivElement | null = null;
  let resizeOrch: ResizeOrchestrator | null = null;
  let scrollThresholdH = $state(DEFAULT_MAX_WINDOW_HEIGHT);
  let isScrollLocked = $state(false);
  let deviceToggleGuard = 0;
  const REMOTE_USAGE_CACHE_PROVIDERS: UsageProvider[] = [ALL_USAGE_PROVIDER_ID, "claude"];

  let headerToggleOptions = $derived.by(() =>
    getVisibleHeaderProviders(headerTabs).map((value) => ({
      value,
      label: headerTabs[value].label,
    })),
  );
  let visibleRateLimitProviders = $derived.by(() =>
    rateLimitProvidersForScope(provider).filter((candidate) => Boolean(providerPayload(rateLimits, candidate))),
  );

  // Subscribe to stores
  $effect(() => {
    const unsub1 = usageData.subscribe((v) => { if (deviceToggleGuard === 0) data = v; });
    const unsub2 = isLoading.subscribe((v) => (loading = v));
    const unsub3 = settings.subscribe((s) => {
      brandTheming = s.brandTheming;
      if (!areHeaderTabsEqual(headerTabs, s.headerTabs)) {
        headerTabs = s.headerTabs;
      }
    });
    const unsub4 = rateLimitsData.subscribe((v) => (rateLimits = v));
    const unsub5 = rateLimitsRequestState.subscribe((v) => (rateLimitsRequest = v));
    return () => { unsub1(); unsub2(); unsub3(); unsub4(); unsub5(); };
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
    const colorScheme = window.matchMedia("(prefers-color-scheme: light)");

    // Create the resize orchestrator (all resize state lives in its closure)
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

    const handleColorSchemeChange = () => {
      const followsSystemTheme = !document.documentElement.hasAttribute("data-theme");
      logResizeDebug("theme:system-change", {
        matchesLight: colorScheme.matches,
        followsSystemTheme,
      });

      const updates: Promise<unknown>[] = [syncTrayConfig(get(settings).trayConfig, null)];
      if (followsSystemTheme) {
        updates.push(syncNativeWindowSurface(undefined, get(settings).glassEffect));
      }

      void Promise.allSettled(updates);
    };
    const handleBrowserResize = () => {
      logResizeDebug("browser:resize", captureSnapshot("browser-resize"));
    };
    const handleWindowFocus = () => {
      logResizeDebug("window:focus", captureSnapshot("window-focus"));
      void syncNativeWindowSurface(undefined, get(settings).glassEffect).catch(() => {});
      syncSizeAndVerify("window-focus");
    };
    const handleWindowBlur = () => {
      logResizeDebug("window:blur", captureSnapshot("window-blur"));
    };
    const handleVisibilityChange = () => {
      logResizeDebug("document:visibility-change", {
        hidden: document.hidden,
        visibilityState: document.visibilityState,
        ...captureSnapshot("document-visibility-change"),
      });
    };
    const handleWindowError = (event: ErrorEvent) => {
      logResizeDebug("window:error", {
        message: event.message,
        filename: event.filename || null,
        lineno: event.lineno || null,
        colno: event.colno || null,
        error: formatDebugError(event.error),
      });
    };
    const handleUnhandledRejection = (event: PromiseRejectionEvent) => {
      logResizeDebug("window:unhandledrejection", {
        reason: formatDebugError(event.reason),
      });
    };
    initResizeDebug();
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

      await fetchData(provider, period, offset);
      if (cancelled) return;
      logResizeDebug("app:data-ready", {
        provider,
        period,
        offset,
        ...captureSnapshot("data-ready"),
      });

      if (period === "5h") {
        await fetchRateLimits(provider);
      } else {
        await hydrateRateLimits();
      }
      if (cancelled) return;
      await syncTrayConfig(get(settings).trayConfig, get(rateLimitsData)).catch(() => {});
      if (cancelled) return;
      warmAllPeriods(provider, period);
      for (const warmProvider of getAdjacentWarmProviders(provider)) {
        warmAllPeriods(warmProvider);
      }
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
      }

      unlisten = await listen("data-updated", () => {
        logResizeDebug("app:data-updated-event", {
          provider,
          period,
          offset,
        });
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

    init();
    window.addEventListener("resize", handleBrowserResize);
    window.addEventListener("focus", handleWindowFocus);
    window.addEventListener("blur", handleWindowBlur);
    window.addEventListener("error", handleWindowError);
    window.addEventListener("unhandledrejection", handleUnhandledRejection);
    document.addEventListener("visibilitychange", handleVisibilityChange);

    if (typeof colorScheme.addEventListener === "function") {
      colorScheme.addEventListener("change", handleColorSchemeChange);
    } else {
      colorScheme.addListener(handleColorSchemeChange);
    }

    return () => {
      cancelled = true;
      unlisten?.();
      unlistenWindowResize?.();
      observer?.disconnect();
      resizeOrch?.destroy();
      resizeOrch = null;
      window.removeEventListener("resize", handleBrowserResize);
      window.removeEventListener("focus", handleWindowFocus);
      window.removeEventListener("blur", handleWindowBlur);
      window.removeEventListener("error", handleWindowError);
      window.removeEventListener("unhandledrejection", handleUnhandledRejection);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      if (typeof colorScheme.removeEventListener === "function") {
        colorScheme.removeEventListener("change", handleColorSchemeChange);
      } else {
        colorScheme.removeListener(handleColorSchemeChange);
      }
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
    <UpdateBanner />
    {#if showSplash}
      <SplashScreen ready={appReady} onComplete={() => { showSplash = false; tick().then(() => syncSizeAndVerify("splash-complete")); }} />
    {:else if !$settings.hasSeenWelcome}
      <WelcomeCard onDismiss={() => { tick().then(() => syncSizeAndVerify("welcome-dismiss")); }} />
    {:else if appReady && !data}
      <SetupScreen />
    {:else if showSettings}
      <Settings onBack={handleSettingsClose} />
    {:else if showCalendar}
      <Calendar onBack={handleCalendarClose} />
    {:else if selectedDevice}
      <SingleDeviceView device={selectedDevice} onBack={handleDeviceBack} />
    {:else if showDevices}
      <DevicesView onBack={() => { showDevices = false; }} onDeviceSelect={handleDeviceSelect} onSettings={handleSettingsOpen} />
    {:else if data}
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
      <MetricsRow {data} />
      <div class="hr"></div>

      {#if period === "5h" && !$settings.rateLimitsEnabled}
        <div class="rate-limit-empty">
          <div class="rate-limit-empty-title">Live rate limits are off</div>
          <div class="rate-limit-empty-text">
            Turn this on to see session &amp; weekly usage.
            {#if !$settings.keychainAccessRequested}
              macOS will ask once for Keychain access — click <strong>Always Allow</strong> when it appears.
            {/if}
          </div>
          <button
            type="button"
            class="rate-limit-cta"
            onclick={async () => {
              await updateSetting("rateLimitsEnabled", true);
              await invoke("set_rate_limits_enabled", { enabled: true });
              if (!$settings.keychainAccessRequested) {
                try {
                  await invoke("request_claude_keychain_access");
                } catch (err) {
                  logger.error("rate-limits", `Keychain access request failed: ${err}`);
                }
                await updateSetting("keychainAccessRequested", true);
              }
              await fetchRateLimits();
            }}
          >
            Enable rate limits
          </button>
        </div>
      {:else if period === "5h" && visibleRateLimitProviders.length > 0}
        {#each visibleRateLimitProviders as rateLimitProvider, index}
          <UsageBars
            providerLabel={provider === ALL_USAGE_PROVIDER_ID ? getUsageProviderLabel(rateLimitProvider) : undefined}
            rateLimits={providerPayload(rateLimits, rateLimitProvider)!}
          />
          {#if index < visibleRateLimitProviders.length - 1}
            <div class="hr"></div>
          {/if}
        {/each}
      {:else if period === "5h" && rateLimitsRequest.loading}
        <div class="loading-bars"><div class="spinner"></div></div>
      {:else if period === "5h"}
        <div class="rate-limit-empty">
          <div class="rate-limit-empty-title">Rate limits unavailable</div>
          <div class="rate-limit-empty-text">
            {#if isRateLimitProvider(provider) && (data.total_tokens > 0 || data.total_cost > 0)}
              {getRateLimitIdleSummary(provider)}
            {:else}
              {rateLimitsRequest.error ?? "Unable to load rate limit data right now."}
            {/if}
          </div>
        </div>
      {:else if data.total_cost === 0 && data.total_tokens === 0}
        <div class="empty-period">{emptyPeriodLabel(period, offset)}</div>
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
      <Footer {data} {provider} {period} {rateLimits} onSettings={handleSettingsOpen} onCalendar={handleCalendarOpen} onDevices={() => { showDevices = true; }} />
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
  .rate-limit-empty {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 18px 14px 16px;
    animation: fadeUp .28s ease both .09s;
  }
  .rate-limit-empty-title {
    font: 500 11px/1 'Inter', sans-serif;
    color: var(--t1);
  }
  .rate-limit-empty-text {
    font: 400 9px/1.4 'Inter', sans-serif;
    color: var(--t3);
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
  .rate-limit-cta:hover { filter: brightness(1.08); }
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
    text-align: center;
    color: var(--t3);
    font: 400 10px/1 'Inter', sans-serif;
    padding: 32px 0;
  }
</style>
