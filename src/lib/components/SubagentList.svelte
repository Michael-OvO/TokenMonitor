<script lang="ts">
  import { modelColor, formatCost, formatTokens } from "../utils/format.js";
  import type { SubagentStats } from "../types/index.js";

  interface Props { stats: SubagentStats }
  let { stats }: Props = $props();

  let mainPct = $derived(
    stats.main.cost + stats.subagents.cost > 0
      ? (stats.main.cost / (stats.main.cost + stats.subagents.cost)) * 100
      : 100
  );
</script>

<div class="sa">
  <div class="sa-head">
    <span class="sa-title">Agent Breakdown</span>
  </div>

  <!-- Proportion bar -->
  <div class="sa-bar">
    <div class="sa-bar-main" style="width:{mainPct}%"></div>
    <div class="sa-bar-sub" style="width:{100 - mainPct}%"></div>
  </div>

  <!-- Two cards -->
  <div class="sa-cards">
    <!-- Main card -->
    <div class="sa-card">
      <div class="sa-card-head">
        <span class="sa-dot" style="background:var(--scope-main)"></span>
        <span class="sa-label">Main</span>
      </div>
      <div class="sa-val">{formatCost(stats.main.cost)}</div>
      <div class="sa-sub">
        {formatTokens(stats.main.tokens)} tokens
        {#if stats.main.pct_of_total_cost != null}· {stats.main.pct_of_total_cost.toFixed(0)}%{/if}
      </div>
      {#if stats.main.top_models.length > 0}
        <div class="sa-models">
          {#each stats.main.top_models as m}
            <div class="sa-model-row">
              <span class="sa-model-dot" style="background:{modelColor(m.model_key)}"></span>
              <span class="sa-model-name">{m.display_name}</span>
              <span class="sa-model-cost">{formatCost(m.cost)}</span>
            </div>
          {/each}
        </div>
      {/if}
      {#if stats.main.added_lines > 0 || stats.main.removed_lines > 0}
        <div class="sa-changes">
          <span class="ch-plus">+{stats.main.added_lines.toLocaleString()}</span>
          <span class="ch-slash"> / </span>
          <span class="ch-minus">&minus;{stats.main.removed_lines.toLocaleString()}</span>
          <span class="sa-changes-label"> lines</span>
        </div>
      {/if}
    </div>

    <!-- Subagents card -->
    <div class="sa-card">
      <div class="sa-card-head">
        <span class="sa-dot" style="background:var(--scope-sub)"></span>
        <span class="sa-label">Subagents</span>
      </div>
      <div class="sa-val">{formatCost(stats.subagents.cost)}</div>
      <div class="sa-sub">
        {formatTokens(stats.subagents.tokens)}
        {#if stats.subagents.pct_of_total_cost != null}
          · <span class="sa-pct">{stats.subagents.pct_of_total_cost.toFixed(0)}%</span>
        {/if}
        · {stats.subagents.session_count} spawned
      </div>
      {#if stats.subagents.top_models.length > 0}
        <div class="sa-models">
          {#each stats.subagents.top_models as m}
            <div class="sa-model-row">
              <span class="sa-model-dot" style="background:{modelColor(m.model_key)}"></span>
              <span class="sa-model-name">{m.display_name}</span>
              <span class="sa-model-cost">{formatCost(m.cost)}</span>
            </div>
          {/each}
        </div>
      {/if}
      {#if stats.subagents.added_lines > 0 || stats.subagents.removed_lines > 0}
        <div class="sa-changes">
          <span class="ch-plus">+{stats.subagents.added_lines.toLocaleString()}</span>
          <span class="ch-slash"> / </span>
          <span class="ch-minus">&minus;{stats.subagents.removed_lines.toLocaleString()}</span>
          <span class="sa-changes-label"> lines</span>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .sa { padding: 10px 12px; animation: fadeUp .28s ease both .07s; }
  .sa-head {
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3);
    margin-bottom: 8px;
  }
  .sa-bar {
    display: flex; height: 6px; border-radius: 3px;
    overflow: hidden; margin-bottom: 8px;
  }
  .sa-bar-main { background: var(--scope-main); }
  .sa-bar-sub { background: var(--scope-sub); }

  .sa-cards { display: flex; gap: 4px; }
  .sa-card {
    flex: 1; min-width: 0;
    background: var(--surface-2); border-radius: 7px;
    padding: 8px 9px; transition: background .18s;
  }
  .sa-card:hover { background: var(--surface-hover); }

  .sa-card-head {
    display: flex; align-items: center; gap: 5px; margin-bottom: 5px;
  }
  .sa-dot { width: 5px; height: 5px; border-radius: 50%; flex-shrink: 0; }
  .sa-label {
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3);
  }
  .sa-val {
    font: 400 13px/1 'Inter', sans-serif;
    color: var(--t1); letter-spacing: -.2px;
    font-variant-numeric: tabular-nums;
  }
  .sa-sub {
    font: 400 8px/1 'Inter', sans-serif;
    color: var(--t4); margin-top: 3px;
  }
  .sa-pct { color: var(--scope-sub); }

  .sa-models {
    margin-top: 6px; padding-top: 5px;
    border-top: 1px solid var(--surface-2);
  }
  .sa-model-row {
    display: flex; align-items: center; gap: 4px; margin-bottom: 2px;
    font: 400 7.5px/1 'Inter', sans-serif;
  }
  .sa-model-dot { width: 3px; height: 3px; border-radius: 50%; flex-shrink: 0; }
  .sa-model-name { color: var(--t4); flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sa-model-cost { color: var(--t3); flex-shrink: 0; }

  .sa-changes {
    margin-top: 5px;
    font: 400 7.5px/1 'Inter', sans-serif; color: var(--t4);
  }
  .ch-plus { color: var(--ch-plus); }
  .ch-minus { color: var(--ch-minus); }
  .ch-slash { color: var(--t4); }
  .sa-changes-label { color: var(--t4); }
</style>
