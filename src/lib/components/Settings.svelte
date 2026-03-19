<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { settings, updateSetting, applyTheme, applyGlass, type Settings as SettingsType } from "../stores/settings.js";
  import { currencySymbol, modelColor } from "../utils/format.js";
  import { copyResizeDebugToClipboard, logResizeDebug } from "../resizeDebug.js";
  import { syncNativeWindowSurface } from "../windowAppearance.js";
  import type { KnownModel, TrayConfig, RateLimitsPayload } from "../types/index.js";
  import { rateLimitsData } from "../stores/rateLimits.js";
  import { syncTrayConfig } from "../traySync.js";
  import { formatTrayTitle } from "../trayTitle.js";
  import SegmentedControl from "./SegmentedControl.svelte";
  import ToggleSwitch from "./ToggleSwitch.svelte";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

  interface Props {
    onBack: () => void;
  }

  let { onBack }: Props = $props();

  let current = $state<SettingsType>({
    theme: "dark",
    defaultProvider: "claude",
    defaultPeriod: "day",
    refreshInterval: 30,
    costAlertThreshold: 50,
    launchAtLogin: false,
    currency: "USD",
    hiddenModels: [],
    brandTheming: true,
    trayConfig: {
      barDisplay: 'both',
      barProvider: 'claude',
      showPercentages: false,
      percentageFormat: 'compact',
      showCost: true,
      costPrecision: 'full',
    },
    claudePlan: 0,
    codexPlan: 0,
    glassEffect: true,
  });

  let costInput = $state("50.00");
  let costEnabled = $state(true);
  let copiedDebug = $state(false);
  let availableModels = $state<KnownModel[]>([]);

  const PREVIEW_RATE_LIMITS = {
    claude: { provider: 'claude', planTier: null, windows: [{ windowId: 'p', label: 'Primary', utilization: 72, resetsAt: null }], extraUsage: null, stale: false, error: null, cooldownUntil: null, retryAfterSeconds: null, fetchedAt: '' },
    codex: { provider: 'codex', planTier: null, windows: [{ windowId: 'p', label: 'Primary', utilization: 35, resetsAt: null }], extraUsage: null, stale: false, error: null, cooldownUntil: null, retryAfterSeconds: null, fetchedAt: '' },
  } as RateLimitsPayload;

  // Use a function call to ensure Svelte tracks all trayConfig fields
  let titlePreview = $derived.by(() => {
    const cfg = current.trayConfig;
    return formatTrayTitle(cfg, PREVIEW_RATE_LIMITS, 17.19);
  });

  let previewBarDisplay = $derived(current.trayConfig.barDisplay);
  let previewBarProvider = $derived(current.trayConfig.barProvider);

  $effect(() => {
    const unsub = settings.subscribe((s) => {
      current = s;
      costEnabled = s.costAlertThreshold > 0;
      costInput = s.costAlertThreshold > 0 ? s.costAlertThreshold.toFixed(2) : "50.00";
    });
    return unsub;
  });

  // Check actual autostart state on mount
  $effect(() => {
    isEnabled().then((enabled) => {
      if (enabled !== current.launchAtLogin) {
        updateSetting("launchAtLogin", enabled);
      }
    }).catch(() => {});
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

  function handleTheme(val: string) {
    const theme = val as SettingsType["theme"];
    updateSetting("theme", theme);
    applyTheme(theme);
    void syncNativeWindowSurface(invoke, current.glassEffect).catch(() => {});
  }

  async function handleGlassEffect(checked: boolean) {
    updateSetting("glassEffect", checked);
    applyGlass(checked);
    try {
      await invoke("set_glass_effect", { enabled: checked });
      await syncNativeWindowSurface(invoke, checked);
    } catch (e) {
      console.error("Failed to toggle glass effect:", e);
    }
  }

  function handleProvider(val: string) {
    updateSetting("defaultProvider", val as SettingsType["defaultProvider"]);
  }

  function handleBrandTheming(checked: boolean) {
    updateSetting("brandTheming", checked);
  }

  function handleClaudePlan(val: string) {
    updateSetting("claudePlan", parseInt(val, 10) || 0);
  }

  function handleCodexPlan(val: string) {
    updateSetting("codexPlan", parseInt(val, 10) || 0);
  }

  function handleTrayConfig<K extends keyof TrayConfig>(key: K, value: TrayConfig[K]) {
    const next = { ...current.trayConfig, [key]: value };
    updateSetting("trayConfig", next);
    void syncTrayConfig(next, $rateLimitsData).catch(() => {});
  }

  function handlePeriod(val: string) {
    updateSetting("defaultPeriod", val as SettingsType["defaultPeriod"]);
  }

  function handleRefresh(val: string) {
    const interval = parseInt(val, 10) || 0;
    updateSetting("refreshInterval", interval);
    invoke("set_refresh_interval", { interval }).catch(() => {});
  }

  function handleCurrency(val: string) {
    updateSetting("currency", val as string);
  }

  async function handleAutostart(checked: boolean) {
    try {
      if (checked) {
        await enable();
      } else {
        await disable();
      }
      updateSetting("launchAtLogin", checked);
    } catch (e) {
      console.error("Failed to toggle autostart:", e);
    }
  }

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

  function resetCache() {
    invoke("clear_cache").catch(() => {});
  }

  async function copyDebugLog() {
    logResizeDebug("debug:copy-requested", {
      source: "settings",
    });
    try {
      await copyResizeDebugToClipboard();
      copiedDebug = true;
      setTimeout(() => {
        copiedDebug = false;
      }, 1500);
    } catch (error) {
      copiedDebug = false;
      console.error("Failed to copy debug log:", error);
    }
  }

  const currencies = [
    { value: "USD", label: "USD ($)" },
    { value: "EUR", label: "EUR (€)" },
    { value: "GBP", label: "GBP (£)" },
    { value: "JPY", label: "JPY (¥)" },
    { value: "CNY", label: "CNY (¥)" },
  ];
</script>

<div class="settings">
  <!-- Header -->
  <div class="header">
    <button class="back" onclick={onBack}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="15 18 9 12 15 6"></polyline>
      </svg>
      <span>Settings</span>
    </button>
    <span class="ver">v0.2.0</span>
  </div>

  <div class="scroll">
    <!-- General -->
    <div class="group">
      <div class="group-label">General</div>
      <div class="card">
        <div class="row border">
          <span class="label">Theme</span>
          <SegmentedControl
            options={[
              { value: "light", label: "Light" },
              { value: "dark", label: "Dark" },
              { value: "system", label: "System" },
            ]}
            value={current.theme}
            onChange={handleTheme}
          />
        </div>
        <div class="row border">
          <span class="label">Default Provider</span>
          <SegmentedControl
            options={[
              { value: "claude", label: "Claude" },
              { value: "codex", label: "Codex" },
            ]}
            value={current.defaultProvider}
            onChange={handleProvider}
          />
        </div>
        <div class="row border">
          <span class="label">Default Period</span>
          <SegmentedControl
            options={[
              { value: "5h", label: "5H" },
              { value: "day", label: "Day" },
              { value: "week", label: "Week" },
              { value: "month", label: "Mo" },
            ]}
            value={current.defaultPeriod}
            onChange={handlePeriod}
          />
        </div>
        <div class="row border">
          <span class="label">Refresh</span>
          <SegmentedControl
            options={[
              { value: "30", label: "30s" },
              { value: "60", label: "1m" },
              { value: "300", label: "5m" },
              { value: "0", label: "Off" },
            ]}
            value={String(current.refreshInterval)}
            onChange={handleRefresh}
          />
        </div>
        <div class="row border">
          <span class="label">Brand Theming</span>
          <ToggleSwitch
            checked={current.brandTheming}
            onChange={handleBrandTheming}
          />
        </div>
        <div class="row">
          <span class="label">Glass Effect</span>
          <ToggleSwitch
            checked={current.glassEffect}
            onChange={handleGlassEffect}
          />
        </div>
      </div>
    </div>

    <!-- Menu Bar -->
    <div class="group">
      <div class="group-label">Menu Bar</div>

      <div class="tray-preview">
        <div class="tp-inner">
          <!-- Icon (TokenMonitor winking face) -->
          <svg class="tp-icon" width="14" height="14" viewBox="0 0 44 44" fill="none">
            <circle cx="22" cy="22" r="20" fill="currentColor"/>
            <circle cx="16" cy="23" r="3" fill="#262628"/>
            <path d="M28 20l-4 3.5 4 3.5" stroke="#262628" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
          </svg>
          <!-- Bars -->
          {#if previewBarDisplay === 'both'}
            <div class="tp-bars">
              <div class="tp-track"><div class="tp-fill claude" style="width:72%"></div></div>
              <div class="tp-track"><div class="tp-fill codex" style="width:35%"></div></div>
            </div>
          {:else if previewBarDisplay === 'single'}
            <div class="tp-bars">
              <div class="tp-track single">
                <div class="tp-fill {previewBarProvider}" style="width:72%"></div>
              </div>
            </div>
          {/if}
          <!-- Text -->
          {#if titlePreview}
            <span class="tp-text">{titlePreview}</span>
          {/if}
        </div>
      </div>

      <!-- Bars card -->
      <div class="card" style="margin-bottom: 4px;">
        <div class="row border">
          <span class="label">Display</span>
          <SegmentedControl
            options={[
              { value: "off", label: "Off" },
              { value: "single", label: "Single" },
              { value: "both", label: "Both" },
            ]}
            value={current.trayConfig.barDisplay}
            onChange={(v) => handleTrayConfig("barDisplay", v as TrayConfig["barDisplay"])}
          />
        </div>
        <div class="row" class:dim={current.trayConfig.barDisplay !== 'single'}>
          <span class="label">Provider</span>
          <SegmentedControl
            options={[
              { value: "claude", label: "Claude" },
              { value: "codex", label: "Codex" },
            ]}
            value={current.trayConfig.barProvider}
            onChange={(v) => handleTrayConfig("barProvider", v as TrayConfig["barProvider"])}
          />
        </div>
      </div>

      <!-- Percentages card -->
      <div class="card" style="margin-bottom: 4px;">
        <div class="row border">
          <span class="label">Show Percentages</span>
          <ToggleSwitch
            checked={current.trayConfig.showPercentages}
            onChange={(checked) => handleTrayConfig("showPercentages", checked)}
          />
        </div>
        <div class="row" class:dim={!current.trayConfig.showPercentages}>
          <span class="label">Format</span>
          <SegmentedControl
            options={[
              { value: "compact", label: "72 · 35" },
              { value: "verbose", label: "Claude Code 72% Codex 35%" },
            ]}
            value={current.trayConfig.percentageFormat}
            onChange={(v) => handleTrayConfig("percentageFormat", v as TrayConfig["percentageFormat"])}
          />
        </div>
      </div>

      <!-- Cost card -->
      <div class="card">
        <div class="row border">
          <span class="label">Show Cost</span>
          <ToggleSwitch
            checked={current.trayConfig.showCost}
            onChange={(checked) => handleTrayConfig("showCost", checked)}
          />
        </div>
        <div class="row" class:dim={!current.trayConfig.showCost}>
          <span class="label">Precision</span>
          <SegmentedControl
            options={[
              { value: "whole", label: "$17" },
              { value: "full", label: "$17.19" },
            ]}
            value={current.trayConfig.costPrecision}
            onChange={(v) => handleTrayConfig("costPrecision", v as TrayConfig["costPrecision"])}
          />
        </div>
      </div>
    </div>

    <!-- Plan -->
    <div class="group">
      <div class="group-label">Plan</div>
      <div class="card">
        <div class="row border">
          <span class="label">Claude Plan</span>
          <SegmentedControl
            options={[
              { value: "0", label: "None" },
              { value: "20", label: "$20" },
              { value: "100", label: "$100" },
              { value: "200", label: "$200" },
            ]}
            value={String(current.claudePlan)}
            onChange={handleClaudePlan}
          />
        </div>
        <div class="row">
          <span class="label">Codex Plan</span>
          <SegmentedControl
            options={[
              { value: "0", label: "None" },
              { value: "20", label: "$20" },
              { value: "200", label: "$200" },
            ]}
            value={String(current.codexPlan)}
            onChange={handleCodexPlan}
          />
        </div>
      </div>
    </div>

    <!-- Monitoring -->
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

    <!-- System -->
    <div class="group">
      <div class="group-label">System</div>
      <div class="card">
        <div class="row border">
          <span class="label">Launch at Login</span>
          <ToggleSwitch
            checked={current.launchAtLogin}
            onChange={handleAutostart}
          />
        </div>
        <div class="row border">
          <span class="label">Currency</span>
          <select
            class="currency-select"
            value={current.currency}
            onchange={(e) => handleCurrency((e.target as HTMLSelectElement).value)}
          >
            {#each currencies as cur}
              <option value={cur.value}>{cur.label}</option>
            {/each}
          </select>
        </div>
        <div class="row center">
          <div class="actions">
            <button class="reset-btn" onclick={copyDebugLog}>
              {copiedDebug ? "Copied Debug Log" : "Copy Debug Log"}
            </button>
            <button class="reset-btn" onclick={resetCache}>Reset Cache</button>
          </div>
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  .settings {
    animation: slideIn 0.22s cubic-bezier(.25,.8,.25,1) both;
    height: 460px;
    display: flex;
    flex-direction: column;
  }

  @keyframes slideIn {
    from { opacity: 0; transform: translateX(12px); }
    to { opacity: 1; transform: translateX(0); }
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px 6px;
  }

  .back {
    display: flex;
    align-items: center;
    gap: 4px;
    background: none;
    border: none;
    cursor: pointer;
    color: var(--t1);
    font: 600 12px/1 'Inter', sans-serif;
    padding: 0;
  }
  .back:hover { color: var(--t2); }

  .ver {
    font: 400 8.5px/1 'Inter', sans-serif;
    color: var(--t4);
  }

  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 10px 10px;
    scrollbar-width: none;
  }
  .scroll::-webkit-scrollbar { display: none; }

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
  .row.center {
    justify-content: center;
  }
  .row.dim {
    opacity: 0.25;
    pointer-events: none;
    transition: opacity 0.15s ease;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 10px;
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

  .currency-select {
    background: var(--surface-hover);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 3px 6px;
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t1);
    cursor: pointer;
    outline: none;
    -webkit-appearance: none;
    appearance: none;
  }

  .reset-btn {
    background: none;
    border: none;
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t4);
    cursor: pointer;
    padding: 2px 8px;
  }
  .reset-btn:hover {
    color: var(--t2);
  }

  /* Tray preview — always renders as a dark macOS menu bar fragment,
     regardless of app theme (the real menu bar is always dark). */
  .tray-preview {
    background: var(--surface-2);
    border-radius: 8px;
    padding: 8px 10px;
    margin-bottom: 4px;
    display: flex;
    justify-content: center;
  }
  .tp-inner {
    display: flex;
    align-items: center;
    gap: 5px;
    /* Always dark — matches real macOS dark menu bar */
    background: #262628;
    border-radius: 5px;
    padding: 4px 8px;
    height: 22px;
    border: 0.5px solid rgba(255,255,255,0.06);
  }
  .tp-icon {
    /* Always white inside the dark preview strip */
    color: rgba(255,255,255,0.85);
    flex-shrink: 0;
  }
  .tp-bars {
    display: flex;
    flex-direction: column;
    gap: 1.5px;
  }
  .tp-track {
    width: 30px;
    height: 2.5px;
    background: rgba(255,255,255,0.12);
    border-radius: 1.25px;
    overflow: hidden;
  }
  .tp-track.single {
    width: 38px;
    height: 3.5px;
    border-radius: 1.75px;
  }
  .tp-fill {
    height: 100%;
    border-radius: inherit;
  }
  .tp-fill.claude { background: #d4a574; }
  .tp-fill.codex { background: #7aafff; }
  .tp-text {
    font: 400 10px/1 'Inter', -apple-system, sans-serif;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.2px;
    /* Always light text inside dark preview strip */
    color: rgba(255,255,255,0.88);
    white-space: nowrap;
  }
</style>
