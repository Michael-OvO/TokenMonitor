<script lang="ts">
  import { formatCost, formatTokens } from "../utils/format.js";
  import { settings } from "../stores/settings.js";
  import { activePeriod } from "../stores/usage.js";
  import type { UsagePayload, UsagePeriod } from "../types/index.js";

  interface Props { data: UsagePayload }
  let { data }: Props = $props();

  let threshold = $state(0); // 0 = disabled by default; user configures in Settings
  let period = $state<UsagePeriod>("day");
  $effect(() => {
    const unsub = settings.subscribe((s) => (threshold = s.costAlertThreshold));
    return unsub;
  });
  $effect(() => {
    const unsub = activePeriod.subscribe((value) => (period = value));
    return unsub;
  });

  let overBudget = $derived(threshold > 0 && data.total_cost >= threshold);
  let isLive = $derived(!!data.active_block?.is_active);
  let burnRate = $derived(data.active_block?.burn_rate_per_hour ?? 0);

  let inLabel = $derived(formatTokens(data.input_tokens));
  let outLabel = $derived(formatTokens(data.output_tokens));
  let countLabel = $derived(
    period === "5h"
      ? "Sessions"
      : period === "day"
        ? "Active Hours"
        : period === "year"
          ? "Active Months"
          : "Active Days"
  );
</script>

<div class="met">
  <div class="m" class:alert={overBudget} class:live={isLive}>
    <div class="m-v">{formatCost(data.total_cost)}</div>
    <div class="m-l">
      {#if isLive}<span class="live-dot"></span>{/if}{overBudget ? "Over budget" : "Cost"}
    </div>
    {#if isLive && burnRate > 0}
      <div class="m-s">{formatCost(burnRate)}/h</div>
    {/if}
  </div>
  <div class="m">
    <div class="m-v">{formatTokens(data.total_tokens)}</div>
    <div class="m-l">Tokens</div>
    {#if data.input_tokens > 0}
      <div class="m-s">{inLabel} in · {outLabel} out</div>
    {/if}
  </div>
  <div class="m">
    <div class="m-v">{data.session_count}</div>
    <div class="m-l">{countLabel}</div>
  </div>
</div>

<style>
  .met { display: flex; padding: 12px 12px 10px; gap: 4px; animation: fadeUp .28s ease both .07s; }
  .m {
    flex: 1; padding: 8px 9px;
    background: var(--surface-2); border-radius: 7px;
    transition: background .18s;
  }
  .m:hover { background: var(--surface-hover); }
  .m-v {
    font: 400 13px/1 'Inter', sans-serif;
    color: var(--t1); font-variant-numeric: tabular-nums;
    letter-spacing: -.2px;
  }
  .m-l {
    display: flex; align-items: center; gap: 3px;
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3); text-transform: uppercase;
    letter-spacing: .7px; margin-top: 4px;
  }
  .m-s {
    font: 400 8px/1 'Inter', sans-serif;
    color: var(--t4); margin-top: 2px; letter-spacing: .1px;
  }
  .m.alert {
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.25);
  }
  .m.alert .m-v { color: #ef4444; }
  .m.alert .m-l { color: #f87171; }
  .live-dot {
    width: 4px; height: 4px; border-radius: 50%;
    background: var(--accent);
    flex-shrink: 0;
    animation: livePulse 2s ease-in-out infinite;
  }
  @keyframes livePulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }
</style>
