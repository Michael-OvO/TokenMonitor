<script lang="ts">
  import type { UsagePeriod } from "../types/index.js";

  interface Props {
    active: UsagePeriod;
    onChange: (period: UsagePeriod) => void;
  }
  let { active, onChange }: Props = $props();

  const tabs: Array<{ value: UsagePeriod; label: string }> = [
    { value: "5h", label: "Usage" },
    { value: "day", label: "Day" },
    { value: "week", label: "Week" },
    { value: "month", label: "Month" },
    { value: "year", label: "Year" },
  ];

  let activeIdx = $derived(Math.max(tabs.findIndex((t) => t.value === active), 0));
</script>

<div class="tabs">
  <div class="sl" style="width: calc((100% - 5px) / {tabs.length}); transform: translateX({activeIdx * 100}%)"></div>
  {#each tabs as tab}
    <button
      class:on={active === tab.value}
      onclick={() => onChange(tab.value)}
      title={tab.label}
    >
      {tab.label}
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    margin: 6px 12px 0;
    background: var(--surface-2);
    border-radius: 6px;
    padding: 2.5px;
    position: relative;
    animation: fadeUp var(--t-slow) var(--ease-out) both .05s;
  }
  .sl {
    position: absolute; top: 2.5px; left: 2.5px;
    height: calc(100% - 5px);
    background: var(--accent-soft, rgba(255,255,255,0.07));
    border-radius: 5px;
    transition: transform var(--t-slow) var(--ease-out), width var(--t-slow) var(--ease-out);
  }
  button {
    flex: 1; min-width: 0; padding: 6px 8px; border: none; background: none;
    font: 500 8.5px/1 'Inter', sans-serif;
    color: var(--t3); cursor: pointer; position: relative; z-index: 1;
    letter-spacing: .2px; transition: color var(--t-normal) ease;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  button.on { color: var(--t1); }

  :global(:root[data-glass="true"]) .tabs {
    background: rgba(255, 255, 255, 0.07);
    backdrop-filter: blur(20px) saturate(1.8);
    -webkit-backdrop-filter: blur(20px) saturate(1.8);
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.12);
  }
  :global(:root[data-glass="true"]) .sl {
    background: rgba(255, 255, 255, 0.14);
  }
  :global(:root[data-glass="true"][data-theme="light"]) .tabs {
    background: rgba(0, 0, 0, 0.04);
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.4);
  }
  :global(:root[data-glass="true"][data-theme="light"]) .sl {
    background: rgba(255, 255, 255, 0.45);
  }
</style>
