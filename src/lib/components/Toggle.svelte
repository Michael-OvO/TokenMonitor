<script lang="ts">
  interface Props {
    active: "all" | "claude" | "codex";
    onChange: (provider: "all" | "claude" | "codex") => void;
  }
  let { active, onChange }: Props = $props();

  const options: Array<{ value: "all" | "claude" | "codex"; label: string }> = [
    { value: "all", label: "All" },
    { value: "claude", label: "Claude" },
    { value: "codex", label: "Codex" },
  ];

  let activeIdx = $derived(options.findIndex((o) => o.value === active));
</script>

<div class="tog-wrap">
  <div class="tog">
    <div class="sl" style="width: calc({100 / options.length}% - 2.5px); transform: translateX({activeIdx * 100}%)"></div>
    {#each options as opt}
      <button class:on={active === opt.value} onclick={() => onChange(opt.value)}>
        {opt.label}
      </button>
    {/each}
  </div>
</div>

<style>
  .tog-wrap { padding: 10px 12px 0; animation: fadeUp .28s ease both .03s; }
  .tog {
    display: flex;
    background: var(--surface-2);
    border-radius: 6px;
    padding: 2.5px;
    position: relative;
  }
  .sl {
    position: absolute; top: 2.5px; left: 2.5px;
    height: calc(100% - 5px);
    background: var(--accent-soft, rgba(255,255,255,0.07));
    border-radius: 5px;
    transition: transform .28s cubic-bezier(.4,0,.2,1), width .28s cubic-bezier(.4,0,.2,1);
  }
  button {
    flex: 1; padding: 6px 0; border: none; background: none;
    font: 500 10.5px/1 'Inter', sans-serif;
    color: var(--t3); cursor: pointer; position: relative; z-index: 1;
    letter-spacing: .2px; transition: color .22s;
  }
  button.on { color: var(--t1); }
</style>
