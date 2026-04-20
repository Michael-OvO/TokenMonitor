<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { formatCost, formatTokens, modelColor } from "../utils/format.js";
  import { activePeriod, activeOffset } from "../stores/usage.js";
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
  let period = $derived($activePeriod);
  let offset = $derived($activeOffset);

  async function fetchDeviceUsage() {
    loading = true;
    error = null;
    try {
      data = await invoke<UsagePayload>("get_single_device_usage", {
        device,
        period,
        offset,
      });
    } catch (e) {
      error = String(e);
      data = null;
    }
    loading = false;
  }

  // Fetch on mount and re-fetch when period or offset changes.
  // In Svelte 5, $effect runs immediately on mount, so onMount is not needed.
  $effect(() => {
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
    <div class="placeholder">Loading...</div>
  {:else if error}
    <div class="placeholder error-text">{error}</div>
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

  .placeholder {
    padding: 30px 10px;
    text-align: center;
    font: 400 10px/1.6 'Inter', sans-serif;
    color: var(--t3);
  }
  .error-text {
    color: #ef4444;
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
