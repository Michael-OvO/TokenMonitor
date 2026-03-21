<script lang="ts">
  import { formatCost, formatTokens } from "../utils/format.js";
  import { settings } from "../stores/settings.js";
  import { activePeriod } from "../stores/usage.js";
  import type { UsagePayload, UsagePeriod, ChangeStats } from "../types/index.js";

  interface Props { data: UsagePayload }
  let { data }: Props = $props();

  let threshold = $state(0);
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

  // Change stats derivations
  let cs = $derived(data.change_stats);
  let hasChanges = $derived(cs != null && (cs.added_lines > 0 || cs.removed_lines > 0));
  let netNegative = $derived(cs != null && cs.net_lines < 0);

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
  <!-- Cost (top-left) -->
  <div class="m" class:alert={overBudget} class:live={isLive}>
    <div class="m-v">{formatCost(data.total_cost)}</div>
    <div class="m-l">
      {#if isLive}<span class="live-dot"></span>{/if}{overBudget ? "Over budget" : "Cost"}
    </div>
    {#if isLive && burnRate > 0}
      <div class="m-s">{formatCost(burnRate)}/h</div>
    {/if}
  </div>

  <!-- Changes (top-right) -->
  <div class="m" class:m-quiet={!hasChanges}>
    {#if hasChanges}
      <div class="m-v" aria-label="{cs?.added_lines} added, {cs?.removed_lines} removed">
        <span class="ch-plus">+{cs?.added_lines}</span><span class="ch-slash"> / </span><span class="ch-minus">&minus;{cs?.removed_lines}</span>
      </div>
      <div class="m-l">Changes</div>
      <div class="m-s" class:ch-neg={netNegative}>
        net {netNegative ? "" : "+"}{cs?.net_lines} &middot; {cs?.files_touched} files
      </div>
    {:else}
      <div class="m-v m-v-empty">&mdash;</div>
      <div class="m-l">Changes</div>
      <div class="m-s m-empty">No structured edits detected</div>
    {/if}
  </div>

  <!-- Tokens (bottom-left) -->
  <div class="m">
    <div class="m-v">{formatTokens(data.total_tokens)}</div>
    <div class="m-l">Tokens</div>
    {#if data.input_tokens > 0}
      <div class="m-s">{inLabel} in &middot; {outLabel} out</div>
    {/if}
  </div>

  <!-- Composition (bottom-right) -->
  <div class="m" class:m-quiet={compTotal === 0}>
    {#if compTotal > 0}
      <div class="comp">
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
    {:else}
      <div class="m-v m-v-empty">&mdash;</div>
      <div class="m-l">Composition</div>
      <div class="m-s m-empty">No file changes to classify</div>
    {/if}
  </div>
</div>

<style>
  .met {
    display: flex; flex-wrap: wrap;
    padding: 12px 12px 10px; gap: 4px;
    animation: fadeUp .28s ease both .07s;
  }
  .m {
    flex: 1 1 calc(50% - 2px); min-width: calc(50% - 2px);
    padding: 8px 9px;
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

  /* Alert state */
  .m.alert {
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.25);
  }
  .m.alert .m-v { color: #ef4444; }
  .m.alert .m-l { color: #f87171; }

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

  /* Changes card */
  .ch-plus { color: #4ade80; }
  .ch-minus { color: #f87171; }
  .ch-slash { color: var(--t4); }
  .ch-neg { color: #f87171; }

  /* Empty/quiet states */
  .m-quiet .m-v { color: var(--t4); }
  .m-v-empty { color: var(--t4); }
  .m-empty { color: var(--t4); opacity: 0.7; }

  /* Composition card */
  .comp { display: flex; flex-direction: column; gap: 4px; }
  .comp-head {
    display: flex; justify-content: space-between; align-items: center;
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3); text-transform: uppercase;
    letter-spacing: .7px;
  }
  .comp-eff {
    font-weight: 400; text-transform: none;
    letter-spacing: .1px; color: var(--t4);
  }
  .comp-bar {
    display: flex; height: 5px; border-radius: 3px;
    overflow: hidden; background: var(--surface-2);
    animation: hBarGrow .35s ease both .12s;
  }
  .comp-seg { height: 100%; min-width: 2px; }
  .comp-legend {
    display: flex; gap: 6px; flex-wrap: wrap;
    font: 400 7.5px/1 'Inter', sans-serif;
    color: var(--t4); letter-spacing: .1px;
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
