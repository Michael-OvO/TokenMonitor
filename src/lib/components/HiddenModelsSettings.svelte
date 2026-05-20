<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { settings, updateSetting, type Settings as SettingsType } from "../stores/settings.js";
  import { modelColor } from "../utils/format.js";
  import type { KnownModel } from "../types/index.js";
  import ToggleSwitch from "./ToggleSwitch.svelte";

  let current = $derived($settings as SettingsType);

  let availableModels = $state<KnownModel[]>([]);
  let modelsLoading = $state(true);
  let modelsExpanded = $state(false);


  onMount(() => {
    invoke<KnownModel[]>("get_known_models", { provider: "all" })
      .then((models) => {
        availableModels = [...models].sort((a, b) =>
          a.display_name.localeCompare(b.display_name, undefined, { sensitivity: "base" })
        );
      })
      .catch((error) => {
        console.error("Failed to load known models:", error);
      })
      .finally(() => {
        modelsLoading = false;
      });
  });


  function toggleModel(key: string) {
    const hidden = current.hiddenModels.includes(key)
      ? current.hiddenModels.filter((m) => m !== key)
      : [...current.hiddenModels, key];
    updateSetting("hiddenModels", hidden);
  }
</script>

<div class="block">
  <button class="row collapsible-toggle" type="button" onclick={() => (modelsExpanded = !modelsExpanded)}>
    <span class="label">Models</span>
    <div class="collapsible-right">
      <span class="count">{modelsLoading ? "..." : `${availableModels.length - current.hiddenModels.length} of ${availableModels.length}`}</span>
      <svg class="collapsible-chevron" class:open={modelsExpanded} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="6 9 12 15 18 9"></polyline>
      </svg>
    </div>
  </button>
  <div class="models-collapse" class:open={modelsExpanded}>
    <div class="collapse-inner">
      {#if modelsLoading}
      <div class="model-grid" aria-busy="true" aria-label="Loading models">
        {#each Array(4) as _, i (i)}
          <div class="model-cell skeleton-cell">
            <div class="dot skeleton-block"></div>
            <span class="skeleton-block skeleton-name"></span>
            <span class="skeleton-block skeleton-toggle"></span>
          </div>
        {/each}
      </div>
    {:else if availableModels.length > 0}
      <div class="model-grid">
        {#each availableModels as model}
          <div class="model-cell">
            <div class="dot" style:background={modelColor(model.model_key)}></div>
            <span class="model-name">{model.display_name}</span>
            <ToggleSwitch
              checked={!current.hiddenModels.includes(model.model_key)}
              color={modelColor(model.model_key)}
              onChange={() => toggleModel(model.model_key)}
            />
          </div>
        {/each}
      </div>
    {:else}
      <div class="model-empty">No models discovered yet</div>
      {/if}
    </div>
  </div>
</div>

<style>
  .block {
    border-top: 1px solid var(--border-subtle);
    overflow: hidden;
  }
  .row {
    padding: 7px 10px;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .label {
    font: 400 10px/1 'Inter', sans-serif;
    color: var(--t1);
  }
  .collapsible-toggle {
    width: 100%;
    background: none;
    border: none;
    cursor: pointer;
    user-select: none;
  }
  .collapsible-toggle:hover {
    background: var(--surface-hover);
  }
  .collapsible-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .collapsible-chevron {
    color: var(--t3);
    transition: transform 200ms ease;
    transform: rotate(-90deg);
  }
  .collapsible-chevron.open {
    transform: rotate(0deg);
  }
  .count {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
  }
  .model-grid {
    display: flex;
    flex-direction: column;
    padding: 2px 0;
  }
  .model-empty {
    padding: 10px;
    font: 400 9px/1.4 'Inter', sans-serif;
    color: var(--t3);
  }
  .model-cell {
    display: flex;
    align-items: center;
    min-height: 24px;
    gap: 6px;
    padding: 6px 10px;
  }
  .model-name {
    flex: 1;
    min-width: 0;
    font: 400 10px/1.25 'Inter', sans-serif;
    color: var(--t1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .skeleton-block {
    background: var(--surface-hover, rgba(128, 128, 128, 0.12));
    border-radius: 3px;
    animation: skeleton-pulse 1.4s ease-in-out infinite;
  }
  .skeleton-cell .dot.skeleton-block {
    border-radius: 50%;
  }
  .skeleton-name {
    flex: 1;
    height: 8px;
    max-width: 120px;
  }
  .skeleton-toggle {
    width: 22px;
    height: 12px;
    border-radius: 6px;
  }
  .skeleton-cell:nth-child(2) .skeleton-name { max-width: 90px; }
  .skeleton-cell:nth-child(3) .skeleton-name { max-width: 140px; }
  .skeleton-cell:nth-child(4) .skeleton-name { max-width: 100px; }
  @keyframes skeleton-pulse {
    0%, 100% { opacity: 0.5; }
    50% { opacity: 0.9; }
  }
  @media (prefers-reduced-motion: reduce) {
    .skeleton-block { animation: none; opacity: 0.6; }
  }
  .collapsible-toggle {
    width: 100%;
    background: none;
    border: none;
    cursor: pointer;
    user-select: none;
  }
  .collapsible-toggle:hover {
    background: var(--surface-hover);
  }
  .collapsible-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .collapsible-chevron {
    color: var(--t3);
    transition: transform var(--t-normal, 200ms) ease;
    transform: rotate(-90deg);
  }
  .collapsible-chevron.open {
    transform: rotate(0deg);
  }
  .models-collapse {
    display: grid;
    grid-template-rows: 0fr;
    transition: grid-template-rows var(--t-normal, 200ms) ease;
  }
  .models-collapse.open {
    grid-template-rows: 1fr;
  }
  .collapse-inner {
    overflow: hidden;
  }
</style>
