<script lang="ts">
  interface Option {
    value: string;
    label: string;
  }

  interface Props {
    options: Option[];
    value: string;
    onChange: (value: string) => void;
  }

  let { options, value, onChange }: Props = $props();
</script>

<div class="seg">
  {#each options as opt}
    <button
      class="seg-btn"
      class:active={value === opt.value}
      onclick={() => onChange(opt.value)}
    >
      {opt.label}
    </button>
  {/each}
</div>

<style>
  .seg {
    display: flex;
    background: var(--surface-2);
    border-radius: 5px;
    overflow: hidden;
    gap: 1px;
  }
  .seg-btn {
    padding: 3px 7px;
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t3);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: color var(--t-fast) ease, background var(--t-fast) ease;
    white-space: nowrap;
  }
  .seg-btn:hover:not(.active) {
    color: var(--t2);
    background: var(--surface-hover);
    border-radius: 4px;
  }
  .seg-btn.active {
    background: var(--surface-hover);
    color: var(--t1);
    border-radius: 4px;
  }

  /* ── Liquid glass treatment ── */
  :global(:root[data-glass="true"]) .seg {
    background: rgba(255, 255, 255, 0.07);
    border: none;
    backdrop-filter: blur(20px) saturate(1.8);
    -webkit-backdrop-filter: blur(20px) saturate(1.8);
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.12);
  }
  :global(:root[data-glass="true"]) .seg-btn.active {
    background: rgba(255, 255, 255, 0.14);
    box-shadow: none;
  }
  :global(:root[data-glass="true"]) .seg-btn:hover:not(.active) {
    background: rgba(255, 255, 255, 0.06);
  }
  :global(:root[data-glass="true"][data-theme="light"]) .seg {
    background: rgba(0, 0, 0, 0.04);
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.4);
  }
  :global(:root[data-glass="true"][data-theme="light"]) .seg-btn.active {
    background: rgba(255, 255, 255, 0.45);
  }
  :global(:root[data-glass="true"][data-theme="light"]) .seg-btn:hover:not(.active) {
    background: rgba(0, 0, 0, 0.03);
  }
</style>
