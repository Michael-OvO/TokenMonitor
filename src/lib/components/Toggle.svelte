<script lang="ts">
  import {
    getUsageProviderLabel,
    getUsageProviderLogoKind,
  } from "../providerMetadata.js";
  import type { UsageProvider } from "../types/index.js";
  import ProviderLogo from "./ProviderLogo.svelte";

  type ToggleOption = {
    value: UsageProvider;
    label: string;
  };

  interface Props {
    active: UsageProvider;
    onChange: (provider: UsageProvider) => void;
    brandTheming?: boolean;
    options: ToggleOption[];
  }
  let { active, onChange, brandTheming = true, options }: Props = $props();

  let activeIdx = $derived(Math.max(options.findIndex((o) => o.value === active), 0));
  let activeOption = $derived(options[activeIdx] ?? options[0]);
  let showLogo = $derived(brandTheming);
  let activeLogoKind = $derived(getUsageProviderLogoKind(active));
  let activeLabel = $derived(activeOption?.label ?? getUsageProviderLabel(active));
</script>

<div class="tog-wrap">
  {#if showLogo}
    <div class={`provider-logo ${activeLogoKind}`}>
      <ProviderLogo kind={activeLogoKind} size={14} />
      <span>{activeLabel}</span>
    </div>
  {/if}
  <div class="tog">
    <div class="sl" style="width: calc((100% - 5px) / {options.length}); transform: translateX({activeIdx * 100}%)"></div>
    {#each options as opt}
      <button class:on={active === opt.value} onclick={() => onChange(opt.value)} title={opt.label}>
        {opt.label}
      </button>
    {/each}
  </div>
</div>

<style>
  .tog-wrap { padding: 10px 12px 0; animation: fadeUp var(--t-slow) var(--ease-out) both .03s; }
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
    transition: transform var(--t-slow) var(--ease-out), width var(--t-slow) var(--ease-out);
  }
  button {
    flex: 1; min-width: 0; padding: 6px 8px; border: none; background: none;
    font: 500 8.5px/1 system-ui, sans-serif;
    color: var(--t3); cursor: pointer; position: relative; z-index: 1;
    letter-spacing: .2px; transition: color var(--t-normal) ease;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    font: 600 11px/1 system-ui, sans-serif;
    letter-spacing: .2px;
  }
  .provider-logo.all {
    color: var(--t2);
  }
  .provider-logo.claude {
    color: var(--accent);
  }
  .provider-logo.codex {
    color: var(--accent);
  }
  .provider-logo.cursor {
    color: var(--accent);
  }
  .provider-logo.kimi {
    color: var(--accent);
  }
  .provider-logo.generic {
    color: var(--t2);
  }
</style>
