<script lang="ts">
  import {
    getUsageProviderLabel,
    getUsageProviderLogoKind,
  } from "../providerMetadata.js";
  import type { UsageProvider } from "../types/index.js";

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
      {#if activeLogoKind === "all"}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd">
          <path d="M2.4,12 A9.6,9.6 0 1,1 21.6,12 A9.6,9.6 0 1,1 2.4,12 M7.35,9.9 A1.65,1.65 0 1,1 10.65,9.9 A1.65,1.65 0 1,1 7.35,9.9 M13.35,9.9 A1.65,1.65 0 1,1 16.65,9.9 A1.65,1.65 0 1,1 13.35,9.9"/>
        </svg>
        <span>{activeLabel}</span>
      {:else if activeLogoKind === "claude"}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd">
          <path clip-rule="evenodd" d="M20.998 10.949H24v3.102h-3v3.028h-1.487V20H18v-2.921h-1.487V20H15v-2.921H9V20H7.488v-2.921H6V20H4.487v-2.921H3V14.05H0V10.95h3V5h17.998v5.949zM6 10.949h1.488V8.102H6v2.847zm10.51 0H18V8.102h-1.49v2.847z"/>
        </svg>
        <span>{activeLabel}</span>
      {:else if activeLogoKind === "codex"}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd">
          <path clip-rule="evenodd" d="M8.086.457a6.105 6.105 0 013.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 00.107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 01-.18 1.631.167.167 0 00.04.155 5.982 5.982 0 011.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 01-2.934 1.851.162.162 0 00-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 00-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 01-2.595-.622 6.058 6.058 0 01-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 01-.495-1.283 6.11 6.11 0 01-.017-3.064.166.166 0 00.008-.074.115.115 0 00-.037-.064 5.958 5.958 0 01-1.38-2.202 5.196 5.196 0 01-.333-1.589 6.915 6.915 0 01.188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 00.087-.087A6.016 6.016 0 015.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 00-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 001.46.864l1.94-3.272a.849.849 0 00.007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 000 1.695h4.848a.849.849 0 000-1.696h-4.848z"/>
        </svg>
        <span>{activeLabel}</span>
      {:else}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3.5" y="3.5" width="17" height="17" rx="4"></rect>
          <path d="M8 12h8"></path>
          <path d="M12 8v8"></path>
        </svg>
        <span>{activeLabel}</span>
      {/if}
    </div>
  {/if}
  <div class="tog">
    <div class="sl" style="width: calc({100 / options.length}% - 2.5px); transform: translateX({activeIdx * 100}%)"></div>
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
    font: 500 10.5px/1 'Inter', sans-serif;
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
    font: 600 11px/1 'Inter', sans-serif;
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
  .provider-logo.generic {
    color: var(--t2);
  }
</style>
