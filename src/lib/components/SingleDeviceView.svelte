<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { formatCost, formatTokens, modelColor } from "../utils/format.js";
  import { activePeriod, activeOffset, activeProvider } from "../stores/usage.js";
  import type { UsagePayload } from "../types/index.js";
  import Chart from "./Chart.svelte";

  interface Props {
    device: string;
    onBack: () => void;
  }

  let { device, onBack }: Props = $props();

  let data = $state<UsagePayload | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let provider = $derived($activeProvider);
  let period = $derived($activePeriod);
  let offset = $derived($activeOffset);

  async function fetchDeviceUsage() {
    loading = true;
    error = null;
    try {
      data = await invoke<UsagePayload>("get_single_device_usage", {
        device,
        provider,
        period,
        offset,
      });
    } catch (e) {
      error = String(e);
      data = null;
    }
    loading = false;
  }

  // Fetch on mount and re-fetch when provider/period/offset changes.
  // In Svelte 5, $effect runs immediately on mount, so onMount is not needed.
  $effect(() => {
    void provider;
    void period;
    void offset;
    fetchDeviceUsage();
  });

  let sortedModels = $derived(
    data
      ? data.model_breakdown.slice().sort((a, b) => b.cost - a.cost)
      : [],
  );
</script>

<div class="sdv">
  <div class="header">
    <button class="back" type="button" onclick={onBack}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="15 18 9 12 15 6"></polyline>
      </svg>
      <span>{device}</span>
    </button>
    {#if data}
      <span class="total">{formatCost(data.total_cost)}</span>
    {/if}
  </div>

  {#if loading}
    <div class="skeleton-content" aria-busy="true">
      <div class="skeleton skeleton-chart"></div>
      <div class="skeleton-models">
        {#each [1, 2] as _}
          <div class="skeleton-model-row">
            <div class="skeleton skeleton-bar-sm"></div>
            <div class="skeleton skeleton-name"></div>
            <div class="skeleton skeleton-cost"></div>
          </div>
        {/each}
      </div>
    </div>
  {:else if error}
    <div class="error-state">
      <svg class="empty-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#ef4444" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="10"></circle>
        <line x1="15" y1="9" x2="9" y2="15"></line>
        <line x1="9" y1="9" x2="15" y2="15"></line>
      </svg>
      <div class="error-title">Failed to load data</div>
      <div class="error-text">{error}</div>
      <button class="retry-btn" type="button" onclick={fetchDeviceUsage}>Retry</button>
    </div>
  {:else if data}
    <div class="scroll">
      <!-- Chart section -->
      {#if data.chart_buckets.length > 0}
        <Chart buckets={data.chart_buckets} dataKey={device} />
      {/if}

      <!-- Model breakdown -->
      {#if sortedModels.length > 0}
        <div class="models-section">
          <span class="models-title">Models</span>
          <div class="model-list">
            {#each sortedModels as model (model.model_key)}
              <div class="model-row">
                <div class="model-bar" style:background={modelColor(model.model_key)}></div>
                <span class="model-name">{model.display_name}</span>
                <span class="model-cost">{formatCost(model.cost)}</span>
                <span class="model-tokens">{formatTokens(model.tokens)}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .sdv {
    display: flex;
    flex-direction: column;
    animation: fadeUp .28s ease both .05s;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px 6px;
  }

  .back {
    display: flex;
    align-items: center;
    gap: 4px;
    background: none;
    border: none;
    cursor: pointer;
    color: var(--t1);
    font: 600 12px/1 'Inter', sans-serif;
    padding: 0;
  }
  .back:hover { color: var(--t2); }

  .total {
    font: 600 12px/1 'Inter', sans-serif;
    color: var(--t1);
    font-variant-numeric: tabular-nums;
  }

  .scroll {
    flex: 1;
    overflow-y: auto;
  }

  /* ── Skeleton loading ── */
  .skeleton-content { padding: 10px 12px; }
  .skeleton-chart { height: 108px; width: 100%; margin-bottom: 12px; border-radius: 6px; }
  .skeleton-models { display: flex; flex-direction: column; gap: 8px; }
  .skeleton-model-row { display: flex; align-items: center; gap: 8px; }
  .skeleton-bar-sm { width: 3px; height: 14px; border-radius: 1.5px; }
  .skeleton-name { height: 10px; width: 70px; }
  .skeleton-cost { height: 10px; width: 35px; margin-left: auto; }

  /* ── Error state ── */
  .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 30px 10px;
    text-align: center;
  }
  .empty-icon { display: block; margin-bottom: 4px; opacity: 0.6; }
  .error-title {
    font: 500 11px/1 'Inter', sans-serif;
    color: var(--t1);
  }
  .error-text {
    font: 400 9px/1.4 'Inter', sans-serif;
    color: #ef4444;
    max-width: 220px;
  }
  .retry-btn {
    margin-top: 8px;
    padding: 5px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 5px;
    background: transparent;
    color: var(--t2);
    font: 500 9px/1 'Inter', sans-serif;
    cursor: pointer;
    transition: background var(--t-fast) ease, color var(--t-fast) ease;
  }
  .retry-btn:hover {
    background: var(--surface-hover);
    color: var(--t1);
  }

  .models-section {
    padding: 8px 12px 10px;
    animation: fadeUp .28s ease both .12s;
  }

  .models-title {
    display: block;
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3);

        margin-bottom: 8px;
  }

  .model-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .model-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .model-bar {
    width: 3px;
    height: 14px;
    border-radius: 1.5px;
    flex-shrink: 0;
  }

  .model-name {
    flex: 1;
    font: 400 10px/1.25 'Inter', sans-serif;
    color: var(--t2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .model-cost {
    font: 500 10px/1 'Inter', sans-serif;
    color: var(--t1);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }

  .model-tokens {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
    min-width: 32px;
    text-align: right;
  }
</style>
