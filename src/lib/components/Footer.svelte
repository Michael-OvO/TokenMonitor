<script lang="ts">
  import { formatCost, formatTimeAgo } from "../utils/format.js";
  import { footerFiveHourPct } from "../footerView.js";
  import type { UsagePayload, RateLimitsPayload, UsageProvider } from "../types/index.js";

  interface Props {
    data: UsagePayload;
    provider: UsageProvider;
    rateLimits?: RateLimitsPayload | null;
    onSettings: () => void;
    onCalendar: () => void;
  }
  let { data, provider, rateLimits, onSettings, onCalendar }: Props = $props();

  let refreshTick = $state(0);
  let fiveHourPct = $derived.by(() => {
    refreshTick;
    return footerFiveHourPct(rateLimits, provider, Date.now());
  });
  let timeAgo = $derived.by(() => {
    refreshTick;
    return formatTimeAgo(data.last_updated);
  });

  // Update "time ago" every 10 seconds
  $effect(() => {
    const interval = setInterval(() => {
      refreshTick += 1;
    }, 10_000);
    return () => clearInterval(interval);
  });
</script>

<div class="ft">
  <div class="ft-l">
    {#if fiveHourPct != null}
      <span>5h · {fiveHourPct}% used</span>
    {:else}
      <span>5h · {formatCost(data.five_hour_cost)}</span>
    {/if}
  </div>
</div>
<div class="ft2">
  <span class="ft-ts">
    {#if data.from_cache}cached · {/if}{timeAgo}
  </span>
  <div class="ft-actions">
    <button class="gear" onclick={onCalendar} aria-label="Calendar">
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <rect x="3" y="4" width="18" height="18" rx="2" ry="2"></rect>
        <line x1="16" y1="2" x2="16" y2="6"></line>
        <line x1="8" y1="2" x2="8" y2="6"></line>
        <line x1="3" y1="10" x2="21" y2="10"></line>
      </svg>
    </button>
    <button class="gear" onclick={onSettings} aria-label="Settings">
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="3"></circle>
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
      </svg>
    </button>
  </div>
</div>

<style>
  .ft {
    padding: 8px 12px 4px;
    display: flex; justify-content: space-between; align-items: center;
    animation: fadeUp .28s ease both .14s;
  }
  .ft-l { display: flex; align-items: center; gap: 5px; font: 400 9px/1 'Inter', sans-serif; color: var(--t2); }
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
  .ft-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }
</style>
