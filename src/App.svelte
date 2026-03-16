<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { currentMonitor } from "@tauri-apps/api/window";
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
  import DateNav from "./lib/components/DateNav.svelte";

  let showSplash = $state(true);
  let appReady = $state(false);
  let showSettings = $state(false);
  let provider = $state<"all" | "claude" | "codex">("claude");
  let period = $state<"5h" | "day" | "week" | "month" | "year">("day");
  let offset = $state(0);
  let data = $state($usageData);
  let loading = $state(false);
  let showRefresh = $state(false);
  let dataKey = $state("initial");
  let brandTheming = $state(true);
  let popEl: HTMLDivElement | null = null;
  let popContentEl: HTMLDivElement | null = null;
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
    provider = p;
    activeProvider.set(p as any);
    await fetchData(p, period, offset);
    // Guard: if the user switched again while we were fetching, bail out
    // so we don't overwrite dataKey / kick off stale warm-ups.
    if (provider !== p) return;
    dataKey = `${p}-${period}-${offset}-${Date.now()}`;
    await tick();
    syncSizeAndVerify();
    warmAllPeriods(p, period);
    if (p === "claude") warmCache("codex", period);
    else if (p === "codex") warmCache("claude", period);
  }

  async function handlePeriodChange(p: "5h" | "day" | "week" | "month" | "year") {
    const prov = provider;
    period = p;
    offset = 0;
    activePeriod.set(p);
    activeOffset.set(0);
    await fetchData(prov, p, 0);
    // Guard: if provider or period changed while we were fetching, bail out.
    if (period !== p || provider !== prov) return;
    dataKey = `${prov}-${p}-${Date.now()}`;
    await tick();
    syncSizeAndVerify();
  }

  async function handleOffsetChange(delta: number) {
    const prov = provider;
    const per = period;
    offset += delta;
    activeOffset.set(offset);
    await fetchData(prov, per, offset);
    if (period !== per || provider !== prov) return;
    dataKey = `${prov}-${per}-${offset}-${Date.now()}`;
    await tick();
    syncSizeAndVerify();
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
    dataKey = `${prov}-${per}-0-${Date.now()}`;
    await tick();
    syncSizeAndVerify();
  }

  async function handleSettingsOpen() {
    showSettings = true;
    await tick();
    syncSizeAndVerify();
  }

  async function handleSettingsClose() {
    showSettings = false;
    await tick();
    syncSizeAndVerify();
  }

  // ── Window resize ──────────────────────────────────────────────
  //
  //  syncSize()        — measure .pop-content's full content height via
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

  function clearPendingResize() {
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
    } catch {
      maxWindowH = DEFAULT_MAX_WINDOW_HEIGHT;
    }
  }

  function measureContentHeight(): number | null {
    if (!popContentEl) return null;
    // .pop-content has overflow:hidden → scrollHeight reports the FULL
    // content height (immune to WebKit's viewport capping of offsetHeight).
    // We measure .pop-content (not .pop) because .pop's inline min-height
    // is pinned to the window height during resize transitions.
    // Add 2 for .pop's 1px top + 1px bottom border.
    return measureTargetWindowHeight(popContentEl.scrollHeight + 2);
  }

  function applyWindowHeight(targetHeight: number) {
    const nextHeight = clampWindowHeight(targetHeight, maxWindowH, MIN_WINDOW_HEIGHT);
    if (classifyResize(nextHeight, lastWindowH, MIN_WINDOW_HEIGHT) === "skip") return;

    // Pin .pop to the LARGER of old/new height so it always covers the
    // window during the resize IPC round-trip — prevents the macOS
    // compositor from flashing the transparent background.
    const pinHeight = Math.max(nextHeight, lastWindowH);
    if (popEl) popEl.style.minHeight = `${pinHeight}px`;

    lastWindowH = nextHeight;
    webviewWindow.setSize(new LogicalSize(WINDOW_WIDTH, nextHeight))
      .then(() => {
        // Window is now at nextHeight — relax .pop to match.
        if (popEl) popEl.style.minHeight = `${nextHeight}px`;
      })
      .catch(() => {
        if (typeof window !== "undefined") {
          lastWindowH = window.innerHeight;
          // Pin to actual window height (setSize failed, window didn't move)
          if (popEl) popEl.style.minHeight = `${window.innerHeight}px`;
        }
      });
  }

  function syncSize() {
    const nextHeight = measureContentHeight();
    if (nextHeight == null) return;
    applyWindowHeight(nextHeight);
  }

  /** syncSize + schedule a delayed re-measurement.
   *  Catches content whose layout settles a frame or two after the
   *  initial measurement (e.g. chart detail panel pushing footer down). */
  function syncSizeAndVerify() {
    syncSize();
    scheduleSettledResize(100);
  }

  function scheduleSettledResize(delay = RESIZE_SETTLE_DELAY_MS) {
    clearPendingResize();
    resizeTimer = setTimeout(() => {
      resizeTimer = null;
      resizeRaf = requestAnimationFrame(() => {
        resizeRaf = requestAnimationFrame(() => {
          resizeRaf = 0;
          syncSize();
        });
      });
    }, delay);
  }

  function resizeToContent() {
    const measuredHeight = measureContentHeight();
    if (measuredHeight == null) return;
    const nextHeight = clampWindowHeight(measuredHeight, maxWindowH, MIN_WINDOW_HEIGHT);

    switch (classifyResize(nextHeight, lastWindowH, MIN_WINDOW_HEIGHT)) {
      case "grow":
        clearPendingResize();
        applyWindowHeight(measuredHeight);
        // Re-measure after the setSize IPC settles — catches content
        // that was still laying out when the observer first fired.
        scheduleSettledResize(100);
        return;
      case "shrink":
        scheduleSettledResize();
        return;
      default:
        return;
    }
  }

  onMount(() => {
    let cancelled = false;
    let observer: ResizeObserver | undefined;
    let unlisten: (() => void) | undefined;

    const init = async () => {
      // Pin .pop to the initial window height immediately so it fills
      // the native window before any content renders.
      if (popEl) popEl.style.minHeight = `${window.innerHeight}px`;

      await refreshWindowMetrics();

      // Load persisted settings and apply theme + defaults (non-blocking)
      try {
        const saved = await loadSettings();
        if (cancelled) return;
        const runtime = await initializeRuntimeFromSettings(saved);
        if (cancelled) return;
        provider = runtime.provider;
        period = runtime.period;
      } catch {
        // Settings load failed — continue with defaults
      }

      await fetchData(provider, period, offset);
      if (cancelled) return;

      warmAllPeriods(provider, period);
      warmAllPeriods(provider === "claude" ? "codex" : "claude");
      appReady = true;

      // Watch .pop-content (not .pop) because .pop uses min-height:100vh
      // to fill the window — its size only changes when the viewport does,
      // not when content changes.  .pop-content wraps content naturally.
      if (popContentEl) {
        observer = new ResizeObserver(() => resizeToContent());
        observer.observe(popContentEl);
        syncSizeAndVerify();
      }

      unlisten = await listen("data-updated", () => {
        dataKey = `${provider}-${period}-${offset}-${Date.now()}`;
        fetchData(provider, period, offset);
      });

      if (cancelled) {
        unlisten();
        unlisten = undefined;
      }
    };

    init();

    return () => {
      cancelled = true;
      unlisten?.();
      observer?.disconnect();
      if (resizeTimer) clearTimeout(resizeTimer);
      cancelAnimationFrame(resizeRaf);
    };
  });
</script>

<div class="pop" bind:this={popEl}>
  <div class="pop-content" bind:this={popContentEl}>
    {#if showSplash}
      <SplashScreen ready={appReady} onComplete={() => { showSplash = false; tick().then(syncSizeAndVerify); }} />
    {:else if appReady && !data}
      <SetupScreen />
    {:else if showSettings}
      <Settings onBack={handleSettingsClose} />
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
        <Chart buckets={data.chart_buckets} {dataKey} />
      {/if}

      <div class="hr"></div>
      {#if period !== "5h" && data.model_breakdown.length > 0}
        <ModelList models={data.model_breakdown} />
      {/if}
      <Footer {data} onSettings={handleSettingsOpen} />
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
  /* overflow:hidden → scrollHeight reports true content height even when
     WebKit caps offsetHeight to the viewport.  This is the measurement
     target for window sizing (instead of .pop, whose min-height:100vh
     would inflate scrollHeight). */
  .pop-content { min-width: 0; overflow: hidden; }
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
