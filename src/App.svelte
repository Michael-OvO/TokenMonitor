<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
  import { LogicalSize } from "@tauri-apps/api/dpi";
  import {
    activeProvider,
    activePeriod,
    activeOffset,
    usageData,
    isLoading,
    fetchData,
    warmCache,
    warmAllPeriods,
  } from "./lib/stores/usage.js";

  import { loadSettings, settings, applyProvider } from "./lib/stores/settings.js";
  import { initializeRuntimeFromSettings } from "./lib/bootstrap.js";
  import {
    DEFAULT_MAX_WINDOW_HEIGHT,
    MIN_WINDOW_HEIGHT,
    RESIZE_SETTLE_DELAY_MS,
    WINDOW_WIDTH,
    clampWindowHeight,
    classifyResize,
    measureTargetWindowHeight,
    resolveMonitorMaxWindowHeight,
  } from "./lib/windowSizing.js";
  import { syncNativeWindowSurface } from "./lib/windowAppearance.js";
  import {
    captureResizeDebugSnapshot,
    initResizeDebug,
    isResizeDebugEnabled,
    logResizeDebug,
  } from "./lib/resizeDebug.js";

  import Toggle from "./lib/components/Toggle.svelte";
  import TimeTabs from "./lib/components/TimeTabs.svelte";
  import MetricsRow from "./lib/components/MetricsRow.svelte";
  import Chart from "./lib/components/Chart.svelte";
  import UsageBars from "./lib/components/UsageBars.svelte";
  import ModelList from "./lib/components/ModelList.svelte";
  import Footer from "./lib/components/Footer.svelte";
  import SetupScreen from "./lib/components/SetupScreen.svelte";
  import SplashScreen from "./lib/components/SplashScreen.svelte";
  import Settings from "./lib/components/Settings.svelte";
  import Calendar from "./lib/components/Calendar.svelte";
  import DateNav from "./lib/components/DateNav.svelte";

  let showSplash = $state(true);
  let appReady = $state(false);
  let showSettings = $state(false);
  let showCalendar = $state(false);
  let provider = $state<"all" | "claude" | "codex">("claude");
  let period = $state<"5h" | "day" | "week" | "month" | "year">("day");
  let offset = $state(0);
  let data = $state($usageData);
  let loading = $state(false);
  let showRefresh = $state(false);
  let brandTheming = $state(true);
  let popEl: HTMLDivElement | null = null;
  let maxWindowH = DEFAULT_MAX_WINDOW_HEIGHT;

  // Subscribe to stores
  $effect(() => {
    const unsub1 = usageData.subscribe((v) => (data = v));
    const unsub2 = isLoading.subscribe((v) => (loading = v));
    const unsub3 = settings.subscribe((s) => (brandTheming = s.brandTheming));
    return () => { unsub1(); unsub2(); unsub3(); };
  });

  // Apply/remove data-provider attribute reactively
  $effect(() => {
    applyProvider(provider, brandTheming);
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

  async function handleProviderChange(p: "all" | "claude" | "codex") {
    if (provider === p) return;
    provider = p;
    activeProvider.set(p as any);
    await fetchData(p, period, offset);
    // Guard: if the user switched again while we were fetching, bail out
    // so we don't kick off stale warm-ups.
    if (provider !== p) return;
    await tick();
    syncSizeAndVerify("provider-change");
    warmAllPeriods(p, period);
    if (p === "claude") warmCache("codex", period);
    else if (p === "codex") warmCache("claude", period);
  }

  async function handlePeriodChange(p: "5h" | "day" | "week" | "month" | "year") {
    if (period === p && offset === 0) return;
    const prov = provider;
    period = p;
    offset = 0;
    activePeriod.set(p);
    activeOffset.set(0);
    await fetchData(prov, p, 0);
    // Guard: if provider or period changed while we were fetching, bail out.
    if (period !== p || provider !== prov) return;
    await tick();
    syncSizeAndVerify("period-change");
  }

  async function handleOffsetChange(delta: number) {
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
    showCalendar = false;
    showSettings = true;
    await tick();
    syncSizeAndVerify("settings-open");
  }

  async function handleSettingsClose() {
    showSettings = false;
    await tick();
    syncSizeAndVerify("settings-close");
  }

  async function handleCalendarOpen() {
    showSettings = false;
    showCalendar = true;
    await tick();
    syncSizeAndVerify("calendar-open");
  }

  async function handleCalendarClose() {
    showCalendar = false;
    await tick();
    syncSizeAndVerify("calendar-close");
  }

  // ── Window resize ──────────────────────────────────────────────
  //
  //  syncSize()        — measure .pop's full content height via
  //                      scrollHeight (immune to viewport capping) and
  //                      call setSize() immediately.  Used after
  //                      await tick() in every user-initiated view swap.
  //
  //  resizeToContent() — called by ResizeObserver.
  //    • GROW  → immediate.  Prevents clipping during CSS
  //              transitions (detail-panel, ModelList expand).
  //    • SHRINK → debounced (16 ms + double-rAF).  Lets {#key}
  //              destroy→create and transition-end settle first.
  let resizeRaf = 0;
  let resizeTimer: ReturnType<typeof setTimeout> | null = null;
  let lastWindowH = typeof window === "undefined" ? 0 : window.innerHeight;
  const webviewWindow = getCurrentWebviewWindow();
  const tauriWindow = getCurrentWindow();

  function captureDebugSnapshot(reason: string) {
    return isResizeDebugEnabled()
      ? captureResizeDebugSnapshot(reason, popEl, { lastWindowH, maxWindowH })
      : {};
  }

  function formatDebugError(error: unknown) {
    if (error instanceof Error) {
      return {
        name: error.name,
        message: error.message,
      };
    }

    if (typeof error === "string") {
      return { message: error };
    }

    if (error && typeof error === "object") {
      return JSON.parse(JSON.stringify(error));
    }

    return { message: String(error) };
  }

  function clearPendingResize() {
    logResizeDebug("resize:clear-pending", {
      hadTimer: Boolean(resizeTimer),
      hadRaf: resizeRaf !== 0,
      ...captureDebugSnapshot("clear-pending"),
    });
    if (resizeTimer) {
      clearTimeout(resizeTimer);
      resizeTimer = null;
    }

    cancelAnimationFrame(resizeRaf);
    resizeRaf = 0;
  }

  async function refreshWindowMetrics() {
    try {
      const monitor = await currentMonitor();
      if (!monitor) return;
      maxWindowH = resolveMonitorMaxWindowHeight(
        monitor.workArea.size.height,
        monitor.scaleFactor,
      );
      logResizeDebug("resize:monitor-metrics", {
        workAreaHeight: monitor.workArea.size.height,
        scaleFactor: monitor.scaleFactor,
        maxWindowH,
      });
    } catch {
      maxWindowH = DEFAULT_MAX_WINDOW_HEIGHT;
      logResizeDebug("resize:monitor-metrics-fallback", { maxWindowH });
    }
  }

  function measureContentHeight(): number | null {
    if (!popEl) return null;
    // .pop has overflow:hidden → scrollHeight reports the FULL content
    // height including any overflow below the viewport.  Add 2 for
    // .pop's 1px top + 1px bottom border (excluded from scrollHeight).
    return measureTargetWindowHeight(popEl.scrollHeight + 2);
  }

  function applyWindowHeight(targetHeight: number, source = "unknown") {
    const nextHeight = clampWindowHeight(targetHeight, maxWindowH, MIN_WINDOW_HEIGHT);
    const disposition = classifyResize(nextHeight, lastWindowH, MIN_WINDOW_HEIGHT);
    logResizeDebug("resize:apply-request", {
      source,
      targetHeight,
      nextHeight,
      disposition,
      ...captureDebugSnapshot(`apply-${source}`),
    });
    if (disposition === "skip") return;
    lastWindowH = nextHeight;
    webviewWindow
      .setSize(new LogicalSize(WINDOW_WIDTH, nextHeight))
      .then(() => {
        logResizeDebug("resize:set-size-resolved", {
          source,
          nextHeight,
          ...captureDebugSnapshot(`set-size-resolved-${source}`),
        });
      })
      .catch((error) => {
        logResizeDebug("resize:set-size-rejected", {
          source,
          nextHeight,
          error: formatDebugError(error),
          ...captureDebugSnapshot(`set-size-rejected-${source}`),
        });
        if (typeof window !== "undefined") {
          lastWindowH = window.innerHeight;
        }
      });
  }

  function syncSize(source = "unknown") {
    const nextHeight = measureContentHeight();
    logResizeDebug("resize:sync-size", {
      source,
      measuredHeight: nextHeight,
      ...captureDebugSnapshot(`sync-${source}`),
    });
    if (nextHeight == null) return;
    applyWindowHeight(nextHeight, source);
  }

  /** syncSize + schedule a delayed re-measurement.
   *  Catches content whose layout settles a frame or two after the
   *  initial measurement (e.g. chart detail panel pushing footer down). */
  function syncSizeAndVerify(source = "unknown") {
    logResizeDebug("resize:sync-and-verify", { source });
    syncSize(`${source}:initial`);
    scheduleSettledResize(100, `${source}:verify`);
  }

  function scheduleSettledResize(delay = RESIZE_SETTLE_DELAY_MS, source = "unknown") {
    logResizeDebug("resize:schedule-settled", {
      source,
      delay,
      ...captureDebugSnapshot(`schedule-${source}`),
    });
    clearPendingResize();
    resizeTimer = setTimeout(() => {
      resizeTimer = null;
      resizeRaf = requestAnimationFrame(() => {
        resizeRaf = requestAnimationFrame(() => {
          resizeRaf = 0;
          logResizeDebug("resize:settled-fire", {
            source,
            ...captureDebugSnapshot(`settled-fire-${source}`),
          });
          syncSize(`${source}:settled`);
        });
      });
    }, delay);
  }

  function resizeToContent(source = "observer") {
    const measuredHeight = measureContentHeight();
    logResizeDebug("resize:observer-measure", {
      source,
      measuredHeight,
      ...captureDebugSnapshot(`resize-to-content-${source}`),
    });
    if (measuredHeight == null) return;
    const nextHeight = clampWindowHeight(measuredHeight, maxWindowH, MIN_WINDOW_HEIGHT);
    const disposition = classifyResize(nextHeight, lastWindowH, MIN_WINDOW_HEIGHT);

    switch (disposition) {
      case "grow":
        clearPendingResize();
        applyWindowHeight(measuredHeight, `${source}:grow`);
        // Re-measure after setSize settles — the first scrollHeight
        // can miss content still laying out (e.g. detail panel + footer).
        scheduleSettledResize(100, `${source}:grow`);
        return;
      case "shrink":
        scheduleSettledResize(RESIZE_SETTLE_DELAY_MS, `${source}:shrink`);
        return;
      default:
        return;
    }
  }

  onMount(() => {
    let cancelled = false;
    let observer: ResizeObserver | undefined;
    let unlisten: (() => void) | undefined;
    let unlistenWindowResize: (() => void) | undefined;
    const colorScheme = window.matchMedia("(prefers-color-scheme: light)");
    const handleColorSchemeChange = () => {
      if (!document.documentElement.hasAttribute("data-theme")) {
        logResizeDebug("theme:system-change", {
          matchesLight: colorScheme.matches,
        });
        void syncNativeWindowSurface().catch(() => {});
      }
    };
    const handleBrowserResize = () => {
      logResizeDebug("browser:resize", captureDebugSnapshot("browser-resize"));
    };
    const handleWindowFocus = () => {
      logResizeDebug("window:focus", captureDebugSnapshot("window-focus"));
      void syncNativeWindowSurface().catch(() => {});
      syncSizeAndVerify("window-focus");
    };
    const handleWindowBlur = () => {
      logResizeDebug("window:blur", captureDebugSnapshot("window-blur"));
    };
    const handleVisibilityChange = () => {
      logResizeDebug("document:visibility-change", {
        hidden: document.hidden,
        visibilityState: document.visibilityState,
        ...captureDebugSnapshot("document-visibility-change"),
      });
    };
    initResizeDebug();
    logResizeDebug("app:mount", captureDebugSnapshot("mount"));

    const init = async () => {
      await refreshWindowMetrics();

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
        logResizeDebug("app:settings-load-failed");
      }

      await fetchData(provider, period, offset);
      if (cancelled) return;
      logResizeDebug("app:data-ready", {
        provider,
        period,
        offset,
        ...captureDebugSnapshot("data-ready"),
      });

      warmAllPeriods(provider, period);
      warmAllPeriods(provider === "claude" ? "codex" : "claude");
      appReady = true;

      if (popEl) {
        observer = new ResizeObserver((entries) => {
          logResizeDebug("resize:observer-fired", {
            entries: entries.map((entry) => ({
              width: entry.contentRect.width,
              height: entry.contentRect.height,
            })),
            ...captureDebugSnapshot("resize-observer"),
          });
          resizeToContent("resize-observer");
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
      });

      unlistenWindowResize = await tauriWindow.onResized(({ payload }) => {
        logResizeDebug("tauri:window-resized", {
          width: payload.width,
          height: payload.height,
          ...captureDebugSnapshot("tauri-window-resized"),
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
      if (resizeTimer) clearTimeout(resizeTimer);
      cancelAnimationFrame(resizeRaf);
      window.removeEventListener("resize", handleBrowserResize);
      window.removeEventListener("focus", handleWindowFocus);
      window.removeEventListener("blur", handleWindowBlur);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      if (typeof colorScheme.removeEventListener === "function") {
        colorScheme.removeEventListener("change", handleColorSchemeChange);
      } else {
        colorScheme.removeListener(handleColorSchemeChange);
      }
    };
  });
</script>

<div class="pop" bind:this={popEl}>
  <div class="pop-content">
    {#if showSplash}
      <SplashScreen ready={appReady} onComplete={() => { showSplash = false; tick().then(() => syncSizeAndVerify("splash-complete")); }} />
    {:else if appReady && !data}
      <SetupScreen />
    {:else if showSettings}
      <Settings onBack={handleSettingsClose} />
    {:else if showCalendar}
      <Calendar onBack={handleCalendarClose} />
    {:else if data}
      {#if showRefresh}<div class="refresh-bar"></div>{/if}
      <Toggle active={provider} onChange={handleProviderChange} {brandTheming} />
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

      {#if period === "5h"}
        <UsageBars {data} />
      {:else if data.total_cost === 0 && data.total_tokens === 0}
        <div class="empty-period">No usage data for this period</div>
      {:else}
        <Chart buckets={data.chart_buckets} />
      {/if}

      {#if period !== "5h" && data.model_breakdown.length > 0}
        <div class="hr"></div>
        <ModelList models={data.model_breakdown} />
      {/if}
      <Footer {data} onSettings={handleSettingsOpen} onCalendar={handleCalendarOpen} />
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
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 14px;
    width: 340px;
    min-height: 200px;
    overflow: hidden;
    box-shadow: none;
    animation: popIn .32s cubic-bezier(.25,.8,.25,1) both;
    /* Force GPU compositing layer — prevents macOS transparent window
       compositor from retaining stale pixels during resize.
       NOTE: do NOT use contain:paint here — it implies overflow:clip
       which caps scrollHeight/getBoundingClientRect, breaking the
       window-resize measurement. */
    isolation: isolate;
    -webkit-backface-visibility: hidden;
    /* Provider theme tint — transparent when neutral */
    background-image: linear-gradient(var(--provider-bg), var(--provider-bg));
  }
  .pop-content {
    min-width: 0;
    min-height: 100%;
    background: var(--surface);
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
  .refresh-bar {
    height: 2px;
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
