<script lang="ts">
  import type { UsagePeriod } from "../types/index.js";

  interface Props {
    active: UsagePeriod;
    onChange: (period: UsagePeriod) => void;
  }
  let { active, onChange }: Props = $props();

  const tabs: Array<{ value: UsagePeriod; label: string }> = [
    { value: "5h", label: "5H" },
    { value: "day", label: "Day" },
    { value: "week", label: "Week" },
    { value: "month", label: "Month" },
    { value: "year", label: "Year" },
  ];
</script>

<div class="tabs">
  {#each tabs as tab}
    <button
      class:on={active === tab.value}
      onclick={() => onChange(tab.value)}
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
    border-radius: 5px;
    padding: 2px;
    gap: 1px;
    animation: fadeUp var(--t-slow) var(--ease-out) both .05s;
  }
  button {
    flex: 1;
    padding: 3px 7px;
    border: none;
    background: transparent;
    font: 500 8.5px/1 'Inter', sans-serif;
    color: var(--t3);
    cursor: pointer;
    border-radius: 4px;
    transition: color var(--t-fast) ease, background var(--t-fast) ease;
  }
  button:hover { color: var(--t2); }
  button.on {
    color: var(--t1);
    background: var(--accent-soft, rgba(255,255,255,0.07));
  }

  :global(:root[data-glass="true"]) .tabs {
    background: rgba(255, 255, 255, 0.07);
    backdrop-filter: blur(20px) saturate(1.8);
    -webkit-backdrop-filter: blur(20px) saturate(1.8);
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.12);
  }
  :global(:root[data-glass="true"]) button.on {
    background: rgba(255, 255, 255, 0.14);
  }
  :global(:root[data-glass="true"][data-theme="light"]) .tabs {
    background: rgba(0, 0, 0, 0.04);
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.4);
  }
  :global(:root[data-glass="true"][data-theme="light"]) button.on {
    background: rgba(255, 255, 255, 0.45);
  }
</style>
