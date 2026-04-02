<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { settings } from "../stores/settings.js";
  import { activeProvider } from "../stores/usage.js";
  import { formatCost } from "../utils/format.js";
  import { planTierCost } from "../utils/plans.js";
  import { intensityLevel, computeEarned, heatmapColor } from "../utils/calendar.js";
  import { rateLimitsData } from "../stores/rateLimits.js";
  import { isRateLimitProvider } from "../providerMetadata.js";
  import { logger } from "../utils/logger.js";
  import type { MonthlyUsagePayload, RateLimitsPayload, UsageProvider } from "../types/index.js";

  interface Props {
    onBack: () => void;
  }

  let { onBack }: Props = $props();

  // Current month being viewed
  let viewYear = $state(new Date().getFullYear());
  let viewMonth = $state(new Date().getMonth() + 1); // 1-indexed

  let data = $state<MonthlyUsagePayload | null>(null);
  let loading = $state(false);
  let provider = $state<UsageProvider>("claude");
  let brandTheming = $state(true);
  let rateLimits = $state<RateLimitsPayload | null>(null);


  // Subscribe to stores
  $effect(() => {
    const unsub1 = activeProvider.subscribe((p) => (provider = p));
    const unsub2 = settings.subscribe((s) => { brandTheming = s.brandTheming; });
    const unsub3 = rateLimitsData.subscribe((r) => { rateLimits = r; });
    return () => { unsub1(); unsub2(); unsub3(); };
  });

  // Fetch data when month/year/provider changes
  $effect(() => {
    fetchMonth(provider, viewYear, viewMonth);
  });

  async function fetchMonth(prov: UsageProvider, year: number, month: number) {
    loading = true;
    try {
      data = await invoke<MonthlyUsagePayload>("get_monthly_usage", {
        provider: prov,
        year,
        month,
      });
    } catch (e) {
      console.error("Failed to fetch monthly usage:", e);
      data = {
        year,
        month,
        days: [],
        total_cost: 0,
        usage_source: "parser",
        usage_warning: typeof e === "string"
          ? e
          : e instanceof Error
            ? e.message
            : "Failed to load monthly usage.",
      };
    } finally {
      loading = false;
    }
  }

  function prevMonth() {
    logger.info("calendar", "Previous month");
    if (viewMonth === 1) {
      viewYear -= 1;
      viewMonth = 12;
    } else {
      viewMonth -= 1;
    }
  }

  function nextMonth() {
    const now = new Date();
    if (viewYear === now.getFullYear() && viewMonth === now.getMonth() + 1) return;
    logger.info("calendar", "Next month");
    if (viewMonth === 12) {
      viewYear += 1;
      viewMonth = 1;
    } else {
      viewMonth += 1;
    }
  }

  // Calendar grid helpers
  const MONTH_NAMES = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
  ];

  let monthLabel = $derived(`${MONTH_NAMES[viewMonth - 1]} ${viewYear}`);

  let isCurrentMonth = $derived.by(() => {
    const now = new Date();
    return viewYear === now.getFullYear() && viewMonth === now.getMonth() + 1;
  });

  let daysInMonth = $derived(new Date(viewYear, viewMonth, 0).getDate());

  // Monday = 0, ..., Sunday = 6
  let firstDayOffset = $derived.by(() => {
    const jsDay = new Date(viewYear, viewMonth - 1, 1).getDay(); // 0=Sun
    return jsDay === 0 ? 6 : jsDay - 1; // convert to Mon-start
  });

  // Build cost lookup from data
  let costByDay = $derived.by(() => {
    const map = new Map<number, number>();
    if (data) {
      for (const d of data.days) {
        map.set(d.day, d.cost);
      }
    }
    return map;
  });

  // Max daily spend (for intensity calculation) — only past/today days
  let maxDailyCost = $derived.by(() => {
    if (costByDay.size === 0) return 0;
    const now = new Date();
    const today = isCurrentMonth
      ? now.getDate()
      : daysInMonth;
    let max = 0;
    for (const [day, cost] of costByDay) {
      if (day <= today && cost > max) max = cost;
    }
    return max;
  });

  function isFutureDay(day: number): boolean {
    if (!isCurrentMonth) return false;
    return day > new Date().getDate();
  }

  let detectedPlanTier = $derived.by(() => {
    if (isRateLimitProvider(provider)) return rateLimits?.[provider]?.planTier ?? null;
    return null;
  });

  let detectedPlanCost = $derived(planTierCost(detectedPlanTier, provider));

  let earned = $derived(data ? computeEarned(data.total_cost, detectedPlanCost) : null);
</script>

<div class="calendar">
  <!-- Header -->
  <div class="header">
    <button class="back" onclick={onBack}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="15 18 9 12 15 6"></polyline>
      </svg>
      <span>Calendar</span>
    </button>
  </div>

  <div class="scroll">
    <!-- Month navigation -->
    <div class="month-nav">
      <button class="nav-arrow" onclick={prevMonth}>‹</button>
      <span class="month-label">{monthLabel}</span>
      <button
        class="nav-arrow"
        class:disabled={isCurrentMonth}
        onclick={nextMonth}
        disabled={isCurrentMonth}
      >›</button>
    </div>
    {#if data?.usage_warning}
      <div class="usage-warning">
        <div class="usage-warning-title">
          {#if data.usage_source === "mixed"}
            Mixed usage sources
          {:else}
            Using legacy usage calculation
          {/if}
        </div>
        <div class="usage-warning-text">{data.usage_warning}</div>
      </div>
    {/if}

    <!-- Day-of-week headers -->
    <div class="day-headers">
      {#each ["M", "T", "W", "T", "F", "S", "S"] as day}
        <span class="day-header">{day}</span>
      {/each}
    </div>

    <!-- Heatmap grid -->
    <div class="grid" class:loading>
      <!-- Empty cells for offset -->
      {#each Array(firstDayOffset) as _}
        <div class="cell empty"></div>
      {/each}

      {#each Array(daysInMonth) as _, i}
        {@const day = i + 1}
        {@const cost = costByDay.get(day) ?? 0}
        {@const future = isFutureDay(day)}
        {@const level = future ? 0 : intensityLevel(cost, maxDailyCost)}
        <div
          class="cell"
          class:future
          style:background={heatmapColor(level, brandTheming, provider)}
        >
          <span class="day-num">{day}</span>
        </div>
      {/each}
    </div>

    <!-- Summary -->
    <div class="summary">
      <div class="summary-label">
        MONTHLY USAGE
        {#if detectedPlanTier}
          <span class="plan-badge">{detectedPlanTier}</span>
        {/if}
      </div>
      <div class="summary-values">
        <span class="summary-total">{formatCost(data?.total_cost ?? 0)}</span>
        {#if earned !== null}
          <span class="summary-dot">·</span>
          {#if earned >= 0}
            <span class="summary-earned positive">+{formatCost(earned)}</span>
          {:else}
            <span class="summary-earned negative">{formatCost(Math.abs(earned))} remaining</span>
          {/if}
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .calendar {
    animation: slideIn 0.22s cubic-bezier(.25,.8,.25,1) both;
    height: 460px;
    display: flex;
    flex-direction: column;
  }

  @keyframes slideIn {
    from { opacity: 0; transform: translateX(12px); }
    to { opacity: 1; transform: translateX(0); }
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

  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 10px 10px;
    scrollbar-width: none;
  }
  .scroll::-webkit-scrollbar { display: none; }

  .month-nav {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 4px 4px 8px;
  }
  .usage-warning {
    margin: 0 4px 10px;
    padding: 9px 10px;
    border-radius: 10px;
    background: color-mix(in srgb, #d88d31 14%, var(--surface));
    border: 1px solid color-mix(in srgb, #d88d31 30%, transparent);
  }
  .usage-warning-title {
    font: 600 10px/1.2 'Inter', sans-serif;
    color: var(--t1);
    margin-bottom: 4px;
  }
  .usage-warning-text {
    font: 400 9px/1.35 'Inter', sans-serif;
    color: var(--t2);
    white-space: pre-wrap;
  }

  .nav-arrow {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--t3);
    font-size: 14px;
    padding: 2px 6px;
    transition: color 0.15s ease;
  }
  .nav-arrow:hover:not(.disabled) { color: var(--t2); }
  .nav-arrow.disabled {
    opacity: 0.2;
    cursor: default;
  }

  .month-label {
    font: 600 13px/1 'Inter', sans-serif;
    color: var(--t1);
  }

  .day-headers {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 3px;
    text-align: center;
    padding: 0 2px 4px;
  }

  .day-header {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t4);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 3px;
    padding: 0 2px;
    transition: opacity 0.15s ease;
  }
  .grid.loading { opacity: 0.3; }

  .cell {
    aspect-ratio: 1;
    border-radius: 3px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--surface-2);
  }
  .cell.empty {
    background: transparent;
  }
  .cell.future {
    background: var(--surface-2);
  }

  .day-num {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t2);
    font-variant-numeric: tabular-nums;
  }
  .cell.future .day-num {
    color: var(--t4);
  }

  .summary {
    border-top: 1px solid var(--border-subtle);
    padding-top: 14px;
    margin-top: 14px;
    text-align: center;
  }

  .summary-label {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    font: 500 10px/1 'Inter', sans-serif;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: var(--t4);
    margin-bottom: 6px;
  }

  .plan-badge {
    font: 500 9px/1 'Inter', sans-serif;
    letter-spacing: 0.4px;
    text-transform: none;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
    border-radius: 4px;
    padding: 2px 5px;
  }

  .summary-values {
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 6px;
  }

  .summary-total {
    font: 600 18px/1 'Inter', sans-serif;
    color: var(--t1);
    font-variant-numeric: tabular-nums;
  }

  .summary-dot {
    font: 400 11px/1 'Inter', sans-serif;
    color: var(--t3);
  }

  .summary-earned {
    font: 600 14px/1 'Inter', sans-serif;
    font-variant-numeric: tabular-nums;
  }
  .summary-earned.positive { color: #4daf4a; }
  .summary-earned.negative { color: var(--t3); }
</style>
