<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { settings, updateSetting, type Settings as SettingsType } from "../stores/settings.js";
  import { modelColor } from "../utils/format.js";
  import type { KnownModel } from "../types/index.js";
  import ToggleSwitch from "./ToggleSwitch.svelte";

  let current = $derived($settings as SettingsType);

  let availableModels = $state<KnownModel[]>([]);
  let expanded = $state(false);

  let visibleCount = $derived(
    availableModels.filter((m) => !current.hiddenModels.includes(m.model_key)).length
  );

  function refreshModels() {
    invoke<KnownModel[]>("get_known_models", { provider: "all" })
      .then((models) => {
        availableModels = [...models].sort((a, b) =>
          a.display_name.localeCompare(b.display_name, undefined, { sensitivity: "base" })
        );
      })
      .catch((error) => {
        console.error("Failed to load known models:", error);
      });
  }

  let unlisten: UnlistenFn | undefined;

  onMount(() => {
    refreshModels();
    listen("data-updated", () => refreshModels()).then((fn) => (unlisten = fn));
  });

  onDestroy(() => {
    unlisten?.();
  });

  function toggleModel(key: string) {
    const hidden = current.hiddenModels.includes(key)
      ? current.hiddenModels.filter((m) => m !== key)
      : [...current.hiddenModels, key];
    updateSetting("hiddenModels", hidden);
  }
</script>

<div class="card">
  <button class="row vis-row" type="button" onclick={() => (expanded = !expanded)}>
    <span class="label">Model Visibility</span>
    <div class="vis-right">
      <span class="vis-count">{visibleCount} of {availableModels.length} enabled</span>
      <svg class="vis-chevron" class:open={expanded} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="6 9 12 15 18 9"></polyline>
      </svg>
    </div>
  </button>
  <div class="model-collapse" class:open={expanded}>
    {#if availableModels.length > 0}
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

<style>
  .card {
    background: var(--surface-2);
    border-radius: 8px;
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
  .vis-row {
    width: 100%;
    background: none;
    border: none;
    cursor: pointer;
    user-select: none;
  }
  .vis-row:hover {
    background: var(--surface-hover);
  }
  .vis-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .vis-count {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
  }
  .vis-chevron {
    color: var(--t3);
    transition: transform var(--t-normal) ease;
    transform: rotate(-90deg);
  }
  .vis-chevron.open {
    transform: rotate(0deg);
  }
  .model-collapse {
    max-height: 0;
    overflow: hidden;
    transition: max-height var(--t-normal) ease;
  }
  .model-collapse.open {
    max-height: 400px;
  }
  .model-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
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
    gap: 5px;
    padding: 6px 10px;
  }
  .model-name {
    flex: 1;
    font: 400 9px/1.25 'Inter', sans-serif;
    color: var(--t1);
  }
  .dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    flex-shrink: 0;
  }
</style>
