<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { LogicalSize } from "@tauri-apps/api/dpi";
  import {
    activeProvider,
    activePeriod,
    usageData,
    isLoading,
    fetchData,
    warmCache,
    warmAllPeriods,
  } from "./lib/stores/usage.js";

  import { loadSettings, settings, applyProvider } from "./lib/stores/settings.js";
  import { initializeRuntimeFromSettings } from "./lib/bootstrap.js";

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

  let showSplash = $state(true);
  let appReady = $state(false);
  let showSettings = $state(false);
  let provider = $state<"all" | "claude" | "codex">("claude");
  let period = $state<"5h" | "day" | "week" | "month" | "year">("day");
  let data = $state($usageData);
  let loading = $state(false);
  let showRefresh = $state(false);
  let dataKey = $state("initial");
  let brandTheming = $state(true);

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
    await fetchData(p, period);
    // Guard: if the user switched again while we were fetching, bail out
    // so we don't overwrite dataKey / kick off stale warm-ups.
    if (provider !== p) return;
    dataKey = `${p}-${period}-${Date.now()}`;
    await tick();
    syncSize();
    warmAllPeriods(p, period);
    if (p === "claude") warmCache("codex", period);
    else if (p === "codex") warmCache("claude", period);
  }

  async function handlePeriodChange(p: "5h" | "day" | "week" | "month" | "year") {
    const prov = provider;
    period = p;
    activePeriod.set(p);
    await fetchData(prov, p);
    // Guard: if provider or period changed while we were fetching, bail out.
    if (period !== p || provider !== prov) return;
    dataKey = `${prov}-${p}-${Date.now()}`;
    await tick();
    syncSize();
  }

  async function handleSettingsOpen() {
    showSettings = true;
    await tick();
    syncSize();
  }

  async function handleSettingsClose() {
    showSettings = false;
    await tick();
    syncSize();
  }

  // ── Window resize ──────────────────────────────────────────────
  //
  //  syncSize()        — measure .pop and call setSize().
  //                      Used directly after `await tick()` for
  //                      user-initiated view changes.
  //
  //  resizeToContent() — called by ResizeObserver.
  //    • GROW  → immediate.  Prevents clipping during CSS
  //              transitions (detail-panel, ModelList expand).
  //    • SHRINK → debounced (16 ms + double-rAF).  Lets {#key}
  //              destroy→create and transition-end settle first.
  let resizeRaf = 0;
  let resizeTimer: ReturnType<typeof setTimeout> | null = null;
  let lastWindowH = 0;

  function syncSize() {
    const pop = document.querySelector('.pop') as HTMLElement;
    if (!pop) return;
    const h = Math.ceil(pop.getBoundingClientRect().height) + 2;
    if (h === lastWindowH || h < 100) return;
    lastWindowH = h;
    getCurrentWebviewWindow().setSize(new LogicalSize(340, h)).catch(() => {});
  }

  function resizeToContent() {
    // Grows apply immediately — if we debounce, every ResizeObserver
    // callback during a CSS transition resets the timer and the window
    // never catches up, visibly clipping the expanding content.
    const pop = document.querySelector('.pop') as HTMLElement;
    if (pop) {
      const h = Math.ceil(pop.getBoundingClientRect().height) + 2;
      if (h > lastWindowH && h >= 100) {
        if (resizeTimer) clearTimeout(resizeTimer);
        cancelAnimationFrame(resizeRaf);
        lastWindowH = h;
        getCurrentWebviewWindow().setSize(new LogicalSize(340, h)).catch(() => {});
        return;
      }
    }

    // Shrink / no-op: debounce so {#key} destroy→create cycles and
    // ResizeObserver bursts settle before we measure.
    if (resizeTimer) clearTimeout(resizeTimer);
    cancelAnimationFrame(resizeRaf);
    resizeTimer = setTimeout(() => {
      resizeRaf = requestAnimationFrame(() => {
        resizeRaf = requestAnimationFrame(() => {
          syncSize();
        });
      });
    }, 16);
  }

  onMount(async () => {
    // Load persisted settings and apply theme + defaults (non-blocking)
    try {
      const saved = await loadSettings();
      const runtime = await initializeRuntimeFromSettings(saved);
      provider = runtime.provider;
      period = runtime.period;
    } catch {
      // Settings load failed — continue with defaults
    }

    await fetchData(provider, period);
    warmAllPeriods(provider, period);
    warmAllPeriods(provider === "claude" ? "codex" : "claude");
    appReady = true;

    const pop = document.querySelector('.pop') as HTMLElement;
    let observer: ResizeObserver | undefined;
    if (pop) {
      observer = new ResizeObserver(() => resizeToContent());
      observer.observe(pop);
    }

    const unlisten = await listen("data-updated", () => {
      dataKey = `${provider}-${period}-${Date.now()}`;
      fetchData(provider, period);
    });
    return () => {
      unlisten(); observer?.disconnect();
      if (resizeTimer) clearTimeout(resizeTimer);
      cancelAnimationFrame(resizeRaf);
    };
  });
</script>

<div class="pop">
  {#if showSplash}
    <SplashScreen ready={appReady} onComplete={() => { showSplash = false; tick().then(syncSize); }} />
  {:else if appReady && !data}
    <SetupScreen />
  {:else if showSettings}
    <Settings onBack={handleSettingsClose} />
  {:else if data}
    {#if showRefresh}<div class="refresh-bar"></div>{/if}
    <Toggle active={provider} onChange={handleProviderChange} {brandTheming} />
    <TimeTabs active={period} onChange={handlePeriodChange} />
    <MetricsRow {data} />
    <div class="hr"></div>

    {#if period === "5h"}
      <!-- 5H: horizontal usage bars -->
      <UsageBars {data} />
    {:else if data.chart_buckets.length > 0}
      <!-- Day/Week/Month: stacked vertical bar chart -->
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
       compositor from retaining stale pixels during resize */
    isolation: isolate;
    -webkit-backface-visibility: hidden;
    /* Provider theme tint — transparent when neutral */
    background-image: linear-gradient(var(--provider-bg), var(--provider-bg));
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
</style>
