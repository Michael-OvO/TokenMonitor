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
  let availableModels = $state<KnownModel[]>([]);

  $effect(() => {
    costEnabled = current.costAlertThreshold > 0;
    costInput = current.costAlertThreshold > 0 ? current.costAlertThreshold.toFixed(2) : "50.00";
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
              onblur={handleCostBlur}
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
  .group {
    margin-bottom: 8px;
  }
  .group-label {
    font: 500 8px/1 'Inter', sans-serif;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: var(--t4);
    padding: 2px 4px 4px;
  }
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
