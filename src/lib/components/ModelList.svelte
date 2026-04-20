<script lang="ts">
  import { modelColor, formatCost, formatTokens } from "../utils/format.js";
  import { settings } from "../stores/settings.js";
  import type { ModelSummary } from "../types/index.js";

  interface Props { models: ModelSummary[] }
  let { models }: Props = $props();

  let hiddenModels = $state<string[]>([]);
  $effect(() => {
    const unsub = settings.subscribe((s) => (hiddenModels = s.hiddenModels));
    return unsub;
  });

  let showChanges = $state(false);
  $effect(() => {
    const unsub = settings.subscribe((s) => (showChanges = s.showModelChangeStats));
    return unsub;
  });

  let rows = $derived(
    [...models]
      .filter((m) => !hiddenModels.includes(m.model_key))
      .sort((a, b) => b.cost - a.cost)
  );
</script>

<div class="mdl">
  <div class="mdl-head">
    <span class="mdl-title">Models</span>
    <span class="mdl-count">{rows.length}</span>
  </div>
  <div class="mdl-list">
    {#each rows as row}
      <div class="mr">
        <span class="mb" style="background:{modelColor(row.model_key)}"></span>
        <span class="mn">{row.display_name}</span>
        {#if showChanges && row.change_stats}
          <span class="mcs">
            <span class="mcs-plus">+{row.change_stats.added_lines.toLocaleString()}</span>
            <span class="mcs-slash">/</span>
            <span class="mcs-minus">−{row.change_stats.removed_lines.toLocaleString()}</span>
          </span>
        {/if}
        <span class="mc">{formatCost(row.cost)}</span>
        <span class="mt">{formatTokens(row.tokens)}</span>
      </div>
    {/each}
  </div>
</div>

<style>
  .mdl { padding: 8px 12px 10px; animation: fadeUp .28s ease both .12s; }
  .mdl-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 6px;
    padding: 0 2px;
  }
  .mdl-title {
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3);

      }
  .mdl-count {
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t4);
    font-variant-numeric: tabular-nums;
  }
  .mr {
    display: flex; align-items: center; min-height: 24px; padding: 6px 7px; border-radius: 6px;
    transition: background .15s; gap: 7px;
  }
  .mr:hover { background: var(--surface-2); }
  .mb { width: 2.5px; height: 14px; border-radius: 1.5px; flex-shrink: 0; }
  .mn { font: 400 10px/1.2 'Inter', sans-serif; color: var(--t2); flex: 1; letter-spacing: .1px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mc { font: 500 10px/1.2 'Inter', sans-serif; color: var(--t1); font-variant-numeric: tabular-nums; }
  .mt { font: 400 9px/1.2 'Inter', sans-serif; color: var(--t3); font-variant-numeric: tabular-nums; min-width: 32px; text-align: right; }
  .mcs {
    font: 400 9px/1 'Inter', sans-serif;
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }
  .mcs-plus { color: var(--ch-plus); }
  .mcs-minus { color: var(--ch-minus); }
  .mcs-slash { color: var(--t4); font-size: 8px; margin: 0 1px; }
</style>
