<script lang="ts">
  interface Props {
    active: "all" | "claude" | "codex";
    onChange: (provider: "all" | "claude" | "codex") => void;
    brandTheming?: boolean;
  }
  let { active, onChange, brandTheming = true }: Props = $props();

  const options: Array<{ value: "all" | "claude" | "codex"; label: string }> = [
    { value: "all", label: "All" },
    { value: "claude", label: "Claude" },
    { value: "codex", label: "Codex" },
  ];

  let activeIdx = $derived(options.findIndex((o) => o.value === active));
  let showLogo = $derived(brandTheming && active !== "all");
</script>

<div class="tog-wrap">
  {#if showLogo}
    <div class="provider-logo" class:claude={active === "claude"} class:codex={active === "codex"}>
      {#if active === "claude"}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
          <path d="M16.98 8.38 12.95 20h-2.2L6.57 8.38h2.15l2.93 9.04h.06l2.92-9.04h2.35Z" fill="currentColor"/>
        </svg>
        <span>Claude</span>
      {:else if active === "codex"}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
          <path d="M12 2L3 7v10l9 5 9-5V7l-9-5zm0 2.18L18.36 7.5 12 10.82 5.64 7.5 12 4.18zM5 8.82l6 3.32V19l-6-3.33V8.82zm8 10.18v-6.86l6-3.32v7.18L13 19z" fill="currentColor"/>
        </svg>
        <span>Codex</span>
      {/if}
    </div>
  {/if}
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

  .provider-logo {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 0 2px 6px;
    animation: fadeUp .2s ease both;
  }
  .provider-logo span {
    font: 600 11px/1 'Inter', sans-serif;
    letter-spacing: .2px;
  }
  .provider-logo.claude {
    color: var(--accent);
  }
  .provider-logo.codex {
    color: var(--accent);
  }
</style>
