<script lang="ts">
  import { modelColor, formatCost, formatTokens, deviceColor } from "../utils/format.js";
  import { settings } from "../stores/settings.js";
  import { logger } from "../utils/logger.js";
  import type { AccordionToggleDetail, DeviceSummary, ModelSummary, SubagentStats } from "../types/index.js";

  const ACCORDION_TRANSITION_MS = 300;
  const MAX_VISIBLE_DEVICES = 5;

  interface Props {
    models: ModelSummary[];
    onAccordionToggle?: (detail: AccordionToggleDetail) => void;
    subagentStats: SubagentStats | null;
    deviceBreakdown?: DeviceSummary[] | null;
    onDeviceSelect?: (device: string) => void;
    onShowAllDevices?: () => void;
    onToggleDeviceStats?: (device: string, includeInStats: boolean) => void;
  }
  let { models, onAccordionToggle, subagentStats, deviceBreakdown, onDeviceSelect, onShowAllDevices, onToggleDeviceStats }: Props = $props();

  let hiddenModels = $state<string[]>([]);
  $effect(() => {
    const unsub = settings.subscribe((s) => (hiddenModels = s.hiddenModels));
    return unsub;
  });

  let rows = $derived(
    [...models]
      .filter((m) => !hiddenModels.includes(m.model_key))
      .sort((a, b) => b.cost - a.cost)
  );

  // Sort devices: local first, then by cost descending.
  let sortedDevices = $derived(
    deviceBreakdown
      ? [...deviceBreakdown].sort((a, b) => {
          if (a.is_local && !b.is_local) return -1;
          if (!a.is_local && b.is_local) return 1;
          return b.total_cost - a.total_cost;
        })
      : []
  );

  let visibleDevices = $derived(sortedDevices.slice(0, MAX_VISIBLE_DEVICES));
  let hasMoreDevices = $derived(sortedDevices.length > MAX_VISIBLE_DEVICES);
  let devicesTotalCost = $derived(sortedDevices.reduce((sum, d) => sum + d.total_cost, 0));
  let maxDeviceCost = $derived(Math.max(...sortedDevices.map((d) => d.total_cost), 0.01));

  type AccordionScope = "main" | "subagents" | "devices";

  // Replace three separate boolean states with one map
  let accordionStates = $state<Record<AccordionScope, boolean>>({
    main: false,
    subagents: false,
    devices: false,
  });

  // Replace three separate innerEl states with one map
  let innerEls = $state<Record<AccordionScope, HTMLDivElement | null>>({
    main: null,
    subagents: null,
    devices: null,
  });

  // Mutual exclusion table
  const MUTUAL_EXCLUSIONS: Record<AccordionScope, AccordionScope[]> = {
    main: ["devices"],
    subagents: ["devices"],
    devices: ["main", "subagents"],
  };

  function toggleAccordion(scope: AccordionScope) {
    const expanding = !accordionStates[scope];
    logger.info("navigation", `Accordion: ${scope} ${expanding ? "expand" : "collapse"}`);
    const height = innerEls[scope]?.scrollHeight ?? 0;

    if (height > 0) {
      onAccordionToggle?.({
        durationMs: ACCORDION_TRANSITION_MS,
        expanding,
        height: Math.ceil(height),
        scope,
      });
    }

    // Update state immutably
    const next = { ...accordionStates, [scope]: expanding };
    if (expanding) {
      for (const exclusive of MUTUAL_EXCLUSIONS[scope]) {
        next[exclusive] = false;
      }
    }
    accordionStates = next;
  }
</script>

<div class="bd">
  <div class="bd-head">
    <span class="bd-title">Breakdown</span>
  </div>

  {#if subagentStats}
    <div class="bd-sep">Agents</div>

    <!-- Main agent row -->
    <button
      class="agent-row"
      class:open={accordionStates.main}
      onclick={() => toggleAccordion("main")}
    >
      <span class="ind"><span class="ind-shape"></span></span>
      <span class="agent-bar" style="background:var(--scope-main)"></span>
      <span class="agent-name">Main</span>
      {#if subagentStats.main.top_models.length > 0}
        <span class="agent-dots" class:hidden={accordionStates.main}>
          {#each subagentStats.main.top_models as m}
            <span class="agent-dot" style="background:{modelColor(m.model_key)}" title={m.display_name}></span>
          {/each}
        </span>
      {/if}
      <span class="agent-cost">{formatCost(subagentStats.main.cost)}</span>
      <span class="agent-pct">{subagentStats.main.pct_of_total_cost?.toFixed(0) ?? "—"}%</span>
    </button>
    <div class="sub-group" class:open={accordionStates.main}>
      <div class="sub-inner" bind:this={innerEls.main}>
        {#each subagentStats.main.top_models as m, i}
          <div class="sub-row" style="transition-delay:{(i + 1) * 50}ms">
            <span class="sub-bar" style="background:{modelColor(m.model_key)}"></span>
            <div class="sub-info">
              <div class="sub-name-row">
                <span class="sub-name">{m.display_name}</span>
                <span class="sub-cost">{formatCost(m.cost)}</span>
              </div>
              <div class="sub-tokens">{formatTokens(m.input_tokens)} in · {formatTokens(m.output_tokens)} out{#if m.cache_read_tokens > 0} · {formatTokens(m.cache_read_tokens)} cache{/if}</div>
            </div>
          </div>
        {/each}
      </div>
    </div>

    <!-- Subagents row -->
    <button
      class="agent-row"
      class:open={accordionStates.subagents}
      onclick={() => toggleAccordion("subagents")}
    >
      <span class="ind"><span class="ind-shape"></span></span>
      <span class="agent-bar" style="background:var(--scope-sub)"></span>
      <span class="agent-name">Subagents <span class="agent-meta">· {subagentStats.subagents.session_count}</span></span>
      {#if subagentStats.subagents.top_models.length > 0}
        <span class="agent-dots" class:hidden={accordionStates.subagents}>
          {#each subagentStats.subagents.top_models as m}
            <span class="agent-dot" style="background:{modelColor(m.model_key)}" title={m.display_name}></span>
          {/each}
        </span>
      {/if}
      <span class="agent-cost">{formatCost(subagentStats.subagents.cost)}</span>
      <span class="agent-pct">{subagentStats.subagents.pct_of_total_cost?.toFixed(0) ?? "—"}%</span>
    </button>
    <div class="sub-group" class:open={accordionStates.subagents}>
      <div class="sub-inner" bind:this={innerEls.subagents}>
        {#each subagentStats.subagents.top_models as m, i}
          <div class="sub-row" style="transition-delay:{(i + 1) * 50}ms">
            <span class="sub-bar" style="background:{modelColor(m.model_key)}"></span>
            <div class="sub-info">
              <div class="sub-name-row">
                <span class="sub-name">{m.display_name}</span>
                <span class="sub-cost">{formatCost(m.cost)}</span>
              </div>
              <div class="sub-tokens">{formatTokens(m.input_tokens)} in · {formatTokens(m.output_tokens)} out{#if m.cache_read_tokens > 0} · {formatTokens(m.cache_read_tokens)} cache{/if}</div>
            </div>
          </div>
        {/each}
      </div>
    </div>

    <!-- Per-scope change attribution -->
    {#if subagentStats.main.added_lines > 0 || subagentStats.main.removed_lines > 0 || subagentStats.subagents.added_lines > 0 || subagentStats.subagents.removed_lines > 0}
      <div class="ch-row">
        {#if subagentStats.main.added_lines > 0 || subagentStats.main.removed_lines > 0}
          <div><span class="ch-scope">main</span> <span class="ch-plus">+{subagentStats.main.added_lines.toLocaleString()}</span>/<span class="ch-minus">&minus;{subagentStats.main.removed_lines.toLocaleString()}</span></div>
        {/if}
        {#if subagentStats.subagents.added_lines > 0 || subagentStats.subagents.removed_lines > 0}
          <div><span class="ch-scope">sub</span> <span class="ch-plus">+{subagentStats.subagents.added_lines.toLocaleString()}</span>/<span class="ch-minus">&minus;{subagentStats.subagents.removed_lines.toLocaleString()}</span></div>
        {/if}
      </div>
    {/if}
  {/if}

  <!-- Models section -->
  {#if rows.length > 0}
    <div class="bd-sep">Models</div>
    {#each rows as row}
      <div class="model-row">
        <span class="model-bar" style="background:{modelColor(row.model_key)}"></span>
        <span class="model-name">{row.display_name}</span>
        <span class="model-cost">{formatCost(row.cost)}</span>
        <span class="model-tokens">{formatTokens(row.tokens)}</span>
      </div>
    {/each}
  {/if}

  <!-- Devices section (only when SSH hosts configured) -->
  {#if sortedDevices.length > 0}
    <div class="bd-sep">Devices</div>

    <button
      class="agent-row"
      class:open={accordionStates.devices}
      onclick={() => toggleAccordion("devices")}
    >
      <span class="ind"><span class="ind-shape"></span></span>
      <span class="agent-name">Devices <span class="agent-meta">· {sortedDevices.length}</span></span>
      <span class="agent-cost">{formatCost(devicesTotalCost)}</span>
    </button>
    <div class="sub-group" class:open={accordionStates.devices}>
      <div class="sub-inner" bind:this={innerEls.devices}>
        {#each visibleDevices as device, i}
          <div class="device-row" style="transition-delay:{(i + 1) * 50}ms">
            <span class="device-color-bar" style="background:{deviceColor(device.device)}"></span>
            <button class="device-info" type="button" onclick={() => onDeviceSelect?.(device.device)}>
              <span class="device-name-row">
                <span class="device-name">{device.device}</span>
                {#if device.is_local}
                  <span class="device-badge">This device</span>
                {/if}
                <span class="device-cost">{formatCost(device.total_cost)}</span>
              </span>
              <span class="device-bar-track">
                <span
                  class="device-bar-fill"
                  style:width="{Math.max((device.total_cost / maxDeviceCost) * 100, 2)}%"
                  style:background={deviceColor(device.device)}
                ></span>
              </span>
            </button>
            {#if !device.is_local}
              <button
                class="device-stats-toggle"
                class:active={device.include_in_stats}
                type="button"
                title={device.include_in_stats ? "Included in stats — click to exclude" : "Excluded from stats — click to include"}
                onclick={(e: MouseEvent) => onToggleDeviceStats?.(device.device, !device.include_in_stats)}
              >
                <svg width="14" height="14" viewBox="0 0 14 14">
                  {#if device.include_in_stats}
                    <rect x="1" y="1" width="12" height="12" rx="2" fill="currentColor"/>
                    <path d="M4 7.2 L5.8 9 L10 4.5" stroke="var(--surface-2, #fff)" stroke-width="1.5" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
                  {:else}
                    <rect x="1.5" y="1.5" width="11" height="11" rx="1.5" stroke="currentColor" stroke-width="1.2" fill="none"/>
                  {/if}
                </svg>
              </button>
            {/if}
          </div>
        {/each}
        {#if hasMoreDevices}
          <button class="device-more" onclick={() => onShowAllDevices?.()} type="button">
            +{sortedDevices.length - MAX_VISIBLE_DEVICES} more →
          </button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .bd { padding: 8px 12px 10px; animation: fadeUp .28s ease both .12s; }
  .bd-head {
    display: flex; justify-content: space-between; margin-bottom: 6px; padding: 0 2px;
  }
  .bd-title {
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3); text-transform: uppercase; letter-spacing: .8px;
  }
  .bd-sep {
    padding: 6px 9px 2px;
    font: 500 7px/1 'Inter', sans-serif;
    color: var(--t4); text-transform: uppercase; letter-spacing: .5px;
    opacity: 0.7;
  }

  /* ── Agent rows ── */
  .agent-row {
    display: flex; align-items: center; width: 100%;
    min-height: 26px; padding: 6px 7px; gap: 7px;
    border: none; background: none; border-radius: 6px; cursor: pointer;
    font: inherit; color: inherit; text-align: left;
    transition: background 0.15s ease;
  }
  .agent-row:hover { background: var(--surface-2); }
  .agent-bar { width: 2.5px; height: 14px; border-radius: 1.5px; flex-shrink: 0; }
  .agent-name {
    font: 400 10px/1.2 'Inter', sans-serif;
    color: var(--t2); flex: 1; min-width: 0;
  }
  .agent-meta { font: 400 8px/1 'Inter', sans-serif; color: var(--t4); opacity: 0.7; }
  .agent-cost { font: 500 10px/1.2 'Inter', sans-serif; color: var(--t1); }
  .agent-pct { font: 400 9px/1.2 'Inter', sans-serif; color: var(--t3); min-width: 28px; text-align: right; }

  /* ── Dot → line indicator ── */
  .ind {
    width: 12px; height: 14px; flex-shrink: 0;
    display: flex; align-items: center; justify-content: center;
  }
  .ind-shape {
    width: 4px; height: 4px;
    background: var(--t4);
    border-radius: 2px;
    transition: width 0.25s cubic-bezier(0.25, 0, 0.15, 1),
                height 0.25s cubic-bezier(0.25, 0, 0.15, 1),
                border-radius 0.25s cubic-bezier(0.25, 0, 0.15, 1),
                background 0.15s ease;
    opacity: 0.6;
  }
  .agent-row:hover .ind-shape { opacity: 1; }
  .agent-row.open .ind-shape {
    width: 1.5px; height: 12px;
    border-radius: 1px;
    opacity: 0.4;
  }

  /* ── Model dots (collapsed) ── */
  .agent-dots {
    display: flex; gap: 3px; margin-right: 4px;
    transition: opacity 0.25s ease, transform 0.25s ease;
  }
  .agent-dots.hidden {
    opacity: 0;
    transform: scale(0.5);
    pointer-events: none;
  }
  .agent-dot {
    width: 5px; height: 5px; border-radius: 50%; flex-shrink: 0;
    opacity: 0.8;
    transition: opacity 0.15s ease;
  }
  .agent-row:hover .agent-dot { opacity: 1; }

  /* ── Expandable sub-rows (grid-template-rows for smooth height) ── */
  .sub-group {
    display: grid;
    grid-template-rows: 0fr;
    transition: grid-template-rows 0.3s cubic-bezier(0.25, 0, 0.15, 1);
  }
  .sub-group.open { grid-template-rows: 1fr; }
  .sub-inner { overflow: hidden; min-height: 0; }

  .sub-row {
    display: flex; align-items: flex-start;
    min-height: 22px; padding: 4px 7px 4px 24px; gap: 7px;
    opacity: 0; transform: translateY(-4px);
    transition: opacity 0.25s ease, transform 0.25s ease;
  }
  .sub-group.open .sub-row {
    opacity: 1; transform: translateY(0);
  }
  .sub-bar { width: 2px; height: 10px; border-radius: 1px; flex-shrink: 0; margin-top: 2px; }
  .sub-info { flex: 1; min-width: 0; }
  .sub-name-row { display: flex; align-items: center; gap: 4px; }
  .sub-name { font: 400 9px/1.2 'Inter', sans-serif; color: var(--t3); flex: 1; }
  .sub-cost { font: 400 9px/1.2 'Inter', sans-serif; color: var(--t2); }
  .sub-tokens {
    font: 400 7.5px/1 'Inter', sans-serif; color: var(--t4);
    margin-top: 2px; font-variant-numeric: tabular-nums;
    opacity: 0.8;
  }

  /* ── Change attribution ── */
  .ch-row {
    display: flex; gap: 16px; padding: 2px 9px 4px;
    font: 400 8px/1 'Inter', sans-serif; color: var(--t4);
  }
  .ch-scope { color: var(--t4); }
  .ch-plus { color: var(--ch-plus); }
  .ch-minus { color: var(--ch-minus); }

  /* ── Device rows ── */
  .device-row {
    display: flex; align-items: center;
    width: 100%; min-height: 28px; padding: 4px 7px 4px 24px; gap: 4px;
    border: none; background: none; border-radius: 6px; cursor: pointer;
    font: inherit; color: inherit; text-align: left;
    opacity: 0; transform: translateY(-4px);
    transition: opacity 0.25s ease, transform 0.25s ease, background 0.15s ease;
  }
  .sub-group.open .device-row {
    opacity: 1; transform: translateY(0);
  }
  .device-row:hover { background: var(--surface-2); }

  .device-color-bar { width: 2px; height: 14px; border-radius: 1px; flex-shrink: 0; margin-top: 4px; }
  .device-info {
    flex: 1; min-width: 0;
    border: none; background: none; cursor: pointer;
    font: inherit; color: inherit; text-align: left; padding: 0;
    -webkit-app-region: no-drag;
  }
  .device-name-row { display: flex; align-items: center; gap: 4px; }
  .device-name { font: 400 9px/1.2 'Inter', sans-serif; color: var(--t2); flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .device-badge {
    font: 500 6.5px/1 'Inter', sans-serif; color: var(--t4);
    background: var(--surface-hover, rgba(128,128,128,0.1));
    padding: 1px 4px; border-radius: 3px; flex-shrink: 0;
    text-transform: uppercase; letter-spacing: 0.3px;
  }
  .device-cost { font: 500 9px/1.2 'Inter', sans-serif; color: var(--t1); flex-shrink: 0; }
  .device-bar-track {
    height: 2px; background: var(--surface-hover, rgba(128,128,128,0.1));
    border-radius: 1px; margin-top: 3px; overflow: hidden;
  }
  .device-bar-fill {
    height: 100%; border-radius: 1px;
    transition: width 0.3s ease;
    opacity: 0.7;
  }
  .device-more {
    display: block; width: 100%; padding: 4px 7px 4px 24px;
    border: none; background: none; cursor: pointer;
    font: 400 8px/1.2 'Inter', sans-serif; color: var(--t3);
    text-align: left;
    opacity: 0; transform: translateY(-4px);
    transition: opacity 0.25s ease, transform 0.25s ease, color 0.15s ease;
    -webkit-app-region: no-drag;
  }
  .sub-group.open .device-more { opacity: 1; transform: translateY(0); }
  .device-more:hover { color: var(--t2); }

  .device-stats-toggle {
    display: flex; align-items: center; justify-content: center;
    width: 24px; height: 24px; flex-shrink: 0;
    border: none; background: none; cursor: pointer;
    color: var(--t4); border-radius: 4px;
    transition: color 0.15s ease, background 0.15s ease;
    position: relative;
    z-index: 1;
    -webkit-app-region: no-drag;
  }
  .device-stats-toggle:hover { color: var(--t2); background: var(--surface-hover, rgba(128,128,128,0.12)); }
  .device-stats-toggle.active { color: var(--accent, #4a9eff); }
  .device-stats-toggle svg { pointer-events: none; }

  /* ── Model rows ── */
  .model-row {
    display: flex; align-items: center;
    min-height: 24px; padding: 6px 7px 6px 21px; gap: 7px;
    border-radius: 6px;
    transition: background .15s;
  }
  .model-row:hover { background: var(--surface-2); }
  .model-bar { width: 2.5px; height: 14px; border-radius: 1.5px; flex-shrink: 0; }
  .model-name {
    font: 400 10px/1.2 'Inter', sans-serif;
    color: var(--t2); flex: 1; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .model-cost { font: 500 10px/1.2 'Inter', sans-serif; color: var(--t1); }
  .model-tokens {
    font: 400 9px/1.2 'Inter', sans-serif;
    color: var(--t3); min-width: 32px; text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
