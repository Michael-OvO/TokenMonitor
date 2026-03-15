<script lang="ts">
  import { formatCost, formatTimeAgo } from "../utils/format.js";
  import type { UsagePayload } from "../types/index.js";

  interface Props {
    data: UsagePayload;
    onSettings: () => void;
  }
  let { data, onSettings }: Props = $props();

  let timeAgo = $state(formatTimeAgo(data.last_updated));

  // Update "time ago" every 10 seconds
  $effect(() => {
    timeAgo = formatTimeAgo(data.last_updated);
    const interval = setInterval(() => {
      timeAgo = formatTimeAgo(data.last_updated);
    }, 10_000);
    return () => clearInterval(interval);
  });
</script>

<div class="ft">
  <div class="ft-l">
    {#if data.active_block?.is_active}
      <div class="dot"></div>
      <span>Active · {formatCost(data.active_block.burn_rate_per_hour)}/hr</span>
    {:else}
      <span class="ft-idle">No active session</span>
    {/if}
  </div>
  {#if data.five_hour_cost > 0}
    <div class="ft-r">5h · {formatCost(data.five_hour_cost)}</div>
  {/if}
</div>
<div class="ft2">
  <span class="ft-ts">
    {#if data.from_cache}cached · {/if}{timeAgo}
  </span>
  <button class="gear" onclick={onSettings} aria-label="Settings">
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <circle cx="12" cy="12" r="3"></circle>
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
    </svg>
  </button>
</div>

<style>
  .ft {
    border-top: 1px solid var(--border-subtle);
    padding: 7px 12px 4px;
    display: flex; justify-content: space-between; align-items: center;
    animation: fadeUp .28s ease both .14s;
  }
  .ft-l { display: flex; align-items: center; gap: 5px; font: 400 9px/1 'Inter', sans-serif; color: var(--t2); }
  .ft-idle { color: var(--t3); }
  .dot { width: 5px; height: 5px; border-radius: 50%; background: var(--accent); animation: softPulse 2.5s ease-in-out infinite; }
  .ft-r { font: 400 8.5px/1 'Inter', sans-serif; color: var(--t3); font-variant-numeric: tabular-nums; }
  .ft2 {
    padding: 2px 12px 7px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    animation: fadeUp .28s ease both .16s;
  }
  .ft-ts { font: 400 9px/1 'Inter', sans-serif; color: var(--t4); }
  .gear {
    background: none;
    border: none;
    color: var(--t4);
    cursor: pointer;
    padding: 2px;
    display: flex;
    align-items: center;
    transition: color 0.15s ease;
  }
  .gear:hover {
    color: var(--t2);
  }
</style>
