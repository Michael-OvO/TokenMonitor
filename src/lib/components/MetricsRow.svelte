<script lang="ts">
  import { formatCost, formatTokens } from "../utils/format.js";
  import { settings } from "../stores/settings.js";
  import type { UsagePayload } from "../types/index.js";

  interface Props { data: UsagePayload }
  let { data }: Props = $props();

  let threshold = $state(0);
  $effect(() => {
    const unsub = settings.subscribe((s) => (threshold = s.costAlertThreshold));
    return unsub;
  });

  let overBudget = $derived(threshold > 0 && data.total_cost >= threshold);
  let isLive = $derived(!!data.active_block?.is_active);
  let burnRate = $derived(data.active_block?.burn_rate_per_hour ?? 0);

  let inLabel = $derived(formatTokens(data.input_tokens));
  let outLabel = $derived(formatTokens(data.output_tokens));

  // Change stats derivations
  let cs = $derived(data.change_stats);
  let hasChanges = $derived(cs != null && (cs.added_lines > 0 || cs.removed_lines > 0));

  let compTotal = $derived(
    cs ? cs.code_lines_changed + cs.docs_lines_changed + cs.config_lines_changed + cs.other_lines_changed : 0
  );

  let compPcts = $derived(
    cs && compTotal > 0
      ? {
          code: (cs.code_lines_changed / compTotal) * 100,
          docs: (cs.docs_lines_changed / compTotal) * 100,
          config: (cs.config_lines_changed / compTotal) * 100,
          other: (cs.other_lines_changed / compTotal) * 100,
        }
      : { code: 0, docs: 0, config: 0, other: 0 }
  );

  let effLabel = $derived(
    cs?.cost_per_100_net_lines != null
      ? `${formatCost(cs.cost_per_100_net_lines)}/100L`
      : "—"
  );

  let compAriaLabel = $derived(
    compTotal > 0
      ? `Composition: code ${compPcts.code.toFixed(0)}%, docs ${compPcts.docs.toFixed(0)}%, config ${compPcts.config.toFixed(0)}%, other ${compPcts.other.toFixed(0)}%`
      : "No composition data"
  );
</script>

<div class="met">
  <!-- Top row: Cost + Tokens -->
  <div class="met-top">
    <div class="m" class:alert={overBudget} class:live={isLive}>
      <div class="m-v">{formatCost(data.total_cost)}</div>
      <div class="m-l">
        {#if isLive}<span class="live-dot"></span>{/if}{overBudget ? "Over budget" : "Cost"}
      </div>
      {#if isLive && burnRate > 0}
        <div class="m-s">{formatCost(burnRate)}/h</div>
      {:else if hasChanges}
        <div class="m-s">
          <span class="ch-plus">+{cs?.added_lines}</span><span class="ch-slash"> / </span><span class="ch-minus">&minus;{cs?.removed_lines}</span><span class="ch-sep"> &middot; {cs?.files_touched} files</span>
        </div>
      {/if}
    </div>

    <div class="m">
      <div class="m-v">{formatTokens(data.total_tokens)}</div>
      <div class="m-l">Tokens</div>
      {#if data.input_tokens > 0}
        <div class="m-s">{inLabel} in &middot; {outLabel} out</div>
      {/if}
    </div>
  </div>

  <!-- Full-width Composition row (only when data exists) -->
  {#if compTotal > 0}
    <div class="m met-comp">
      <div class="comp-head">
        <span>Composition</span>
        <span class="comp-eff">{effLabel}</span>
      </div>
      <div class="comp-bar" role="img" aria-label={compAriaLabel}>
        {#if compPcts.code > 0}
          <div class="comp-seg" style="width:{compPcts.code}%;background:var(--comp-code)"></div>
        {/if}
        {#if compPcts.docs > 0}
          <div class="comp-seg" style="width:{compPcts.docs}%;background:var(--comp-docs)"></div>
        {/if}
        {#if compPcts.config > 0}
          <div class="comp-seg" style="width:{compPcts.config}%;background:var(--comp-config)"></div>
        {/if}
        {#if compPcts.other > 0}
          <div class="comp-seg" style="width:{compPcts.other}%;background:var(--comp-other)"></div>
        {/if}
      </div>
      <div class="comp-legend">
        {#if compPcts.code > 0}<span class="comp-item"><span class="comp-dot" style="background:var(--comp-code)"></span>code {compPcts.code.toFixed(0)}%</span>{/if}
        {#if compPcts.docs > 0}<span class="comp-item"><span class="comp-dot" style="background:var(--comp-docs)"></span>docs {compPcts.docs.toFixed(0)}%</span>{/if}
        {#if compPcts.config > 0}<span class="comp-item"><span class="comp-dot" style="background:var(--comp-config)"></span>config {compPcts.config.toFixed(0)}%</span>{/if}
        {#if compPcts.other > 0}<span class="comp-item"><span class="comp-dot" style="background:var(--comp-other)"></span>other {compPcts.other.toFixed(0)}%</span>{/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .met {
    padding: 12px 12px 8px;
    animation: fadeUp var(--t-slow) var(--ease-out) both .07s;
  }
  .met-top {
    display: flex; gap: 4px; margin-bottom: 4px;
  }
  .met-top > .m { flex: 1; min-width: 0; }
  .met-comp { width: 100%; }

  .m {
    padding: 8px;
    background: var(--surface-2); border-radius: 8px;
    transition: background var(--t-fast) ease;
  }
  .m:hover { background: var(--surface-hover); }
  .m-v {
    font: 400 13px/1 'Inter', sans-serif;
    color: var(--t1); font-variant-numeric: tabular-nums;
    letter-spacing: -.2px;
  }
  .m-l {
    display: flex; align-items: center; gap: 4px;
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3);
    margin-top: 4px;
  }
  .m-s {
    font: 400 8px/1 'Inter', sans-serif;
    color: var(--t4); margin-top: 2px; letter-spacing: .1px;
  }

  /* Alert state */
  .m.alert {
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.25);
  }
  .m.alert .m-v { color: #ef4444; }
  .m.alert .m-l { color: var(--ch-minus); }

  /* Live dot */
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

  /* Change lines in cost subtitle */
  .ch-plus { color: var(--ch-plus); }
  .ch-minus { color: var(--ch-minus); }
  .ch-slash { color: var(--t4); }
  .ch-sep { color: var(--t4); }

  /* Composition */
  .comp-head {
    display: flex; justify-content: space-between; align-items: center;
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3);
      }
  .comp-eff {
    font-weight: 400; text-transform: none;
    letter-spacing: .1px; color: var(--t4);
  }
  .comp-bar {
    display: flex; height: 5px; border-radius: 3px;
    overflow: hidden; background: var(--surface-2);
    margin-top: 5px;
    animation: hBarGrow .35s ease both .12s;
  }
  .comp-seg { height: 100%; min-width: 2px; }
  .comp-legend {
    display: flex; gap: 6px; flex-wrap: wrap;
    font: 400 7.5px/1 'Inter', sans-serif;
    color: var(--t4); letter-spacing: .1px;
    margin-top: 4px;
  }
  .comp-item { display: flex; align-items: center; gap: 2px; }
  .comp-dot {
    width: 4px; height: 4px; border-radius: 50%;
    flex-shrink: 0;
  }

  @keyframes hBarGrow {
    from { transform: scaleX(0); }
    to { transform: scaleX(1); }
  }
</style>
