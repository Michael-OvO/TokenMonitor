<script lang="ts">
  type Period = "5h" | "day" | "week" | "month" | "year";
  interface Props {
    active: string;
    onChange: (period: Period) => void;
  }
  let { active, onChange }: Props = $props();

  const tabs: Array<{ value: Period; label: string }> = [
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
  .tabs { display: flex; padding: 8px 12px 0; animation: fadeUp .28s ease both .05s; }
  button {
    padding: 5px 7px; border: none; background: none;
    font: 500 9px/1 'Inter', sans-serif;
    color: var(--t3); cursor: pointer; letter-spacing: .5px;
    text-transform: uppercase; position: relative; transition: color .2s;
    border-radius: 4px;
  }
  button:hover { color: var(--t2); background: rgba(255,255,255,0.02); }
  button.on { color: var(--t1); }
  button.on::after {
    content: ''; position: absolute; bottom: 0; left: 7px; right: 7px;
    height: 1.5px; background: var(--accent); border-radius: .5px;
    animation: tabUnderline .22s ease both; transform-origin: left;
  }
</style>
