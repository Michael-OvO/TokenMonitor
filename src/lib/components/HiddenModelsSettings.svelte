<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { settings, updateSetting, type Settings as SettingsType } from "../stores/settings.js";
  import { currencySymbol, modelColor } from "../utils/format.js";
  import type { KnownModel } from "../types/index.js";
  import ToggleSwitch from "./ToggleSwitch.svelte";

  let current = $derived($settings as SettingsType);

  let costInput = $state("50.00");
  let costEnabled = $state(true);
  let costInputFocused = $state(false);
  let availableModels = $state<KnownModel[]>([]);
  let modelsLoading = $state(true);

  $effect(() => {
    costEnabled = current.costAlertThreshold > 0;
    // Don't overwrite the input while the user is actively editing.
    if (!costInputFocused) {
      costInput = current.costAlertThreshold > 0 ? current.costAlertThreshold.toFixed(2) : "50.00";
    }
  });

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

  function handleCostBlur() {
    const val = parseFloat(costInput);
    if (!isNaN(val) && val >= 0) {
      updateSetting("costAlertThreshold", val);
      costInput = val.toFixed(2);
    } else {
      costInput = current.costAlertThreshold.toFixed(2);
    }
  }

  function handleCostKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      (e.target as HTMLInputElement).blur();
    }
  }

  function toggleModel(key: string) {
    const hidden = current.hiddenModels.includes(key)
      ? current.hiddenModels.filter((m) => m !== key)
      : [...current.hiddenModels, key];
    updateSetting("hiddenModels", hidden);
  }
</script>

<div class="group">
  <div class="group-label">Monitoring</div>
  <div class="card">
    <div class="row border">
      <span class="label">Cost Alert</span>
      <div class="cost-row-right">
        {#if costEnabled}
          <div class="cost-input">
            <span class="dollar">{currencySymbol()}</span>
            <input
              type="text"
              bind:value={costInput}
              onfocus={() => { costInputFocused = true; }}
              onblur={() => { costInputFocused = false; handleCostBlur(); }}
              onkeydown={handleCostKeydown}
              class="cost-field"
            />
          </div>
        {/if}
        <ToggleSwitch
          checked={costEnabled}
          onChange={(checked) => {
            costEnabled = checked;
            if (!checked) {
              updateSetting("costAlertThreshold", 0);
            } else {
              const val = parseFloat(costInput);
              updateSetting("costAlertThreshold", !isNaN(val) && val > 0 ? val : 50);
            }
          }}
        />
      </div>
    </div>
    <div class="row border">
      <span class="label">Model Change Stats</span>
      <ToggleSwitch
        checked={current.showModelChangeStats}
        onChange={(checked) => updateSetting("showModelChangeStats", checked)}
      />
    </div>
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

<style>
  .group {
    margin-bottom: 8px;
  }
  /* `.group-label` is defined globally in `src/app.css`. */
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
  .row.border {
    border-bottom: 1px solid var(--border-subtle);
  }
  .label {
    font: 400 10px/1 'Inter', sans-serif;
    color: var(--t1);
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
  .cost-row-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .cost-input {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .dollar {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
  }
  .cost-field {
    background: var(--surface-hover);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 3px 6px;
    width: 54px;
    text-align: right;
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t1);
    outline: none;
  }
  .cost-field:focus {
    border-color: var(--t3);
  }
</style>
