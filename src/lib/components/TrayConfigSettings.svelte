<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { settings, updateSetting, type Settings as SettingsType } from "../stores/settings.js";
  import { rateLimitsData } from "../stores/rateLimits.js";
  import { syncTrayConfig } from "../tray/sync.js";
  import { formatTrayTitle } from "../tray/title.js";
  import { usesFloatingStatusWidget, isWindows } from "../utils/platform.js";
  import { logger } from "../utils/logger.js";
  import {
    getRateLimitPrimaryWindowId,
    getUsageProviderBrandColor,
    getUsageProviderLabel,
    getUsageProviderTitle,
    RATE_LIMIT_PROVIDER_ORDER,
  } from "../providerMetadata.js";
  import type {
    BarDisplay,
    RateLimitProviderId,
    RateLimitsPayload,
    TrayConfig,
  } from "../types/index.js";
  import SegmentedControl from "./SegmentedControl.svelte";
  import ToggleSwitch from "./ToggleSwitch.svelte";

  let current = $derived($settings as SettingsType);

  const PREVIEW_UTILIZATIONS = [72, 35, 58, 24];

  const PREVIEW_RATE_LIMITS: RateLimitsPayload = RATE_LIMIT_PROVIDER_ORDER.reduce((payload, provider, index) => {
    payload[provider] = {
      provider,
      planTier: null,
      windows: [{
        windowId: getRateLimitPrimaryWindowId(provider),
        label: "Primary",
        utilization: PREVIEW_UTILIZATIONS[index % PREVIEW_UTILIZATIONS.length] ?? 50,
        resetsAt: null,
      }],
      extraUsage: null,
      credits: null,
      stale: false,
      error: null,
      cooldownUntil: null,
      retryAfterSeconds: null,
      fetchedAt: "",
    };
    return payload;
  }, {} as RateLimitsPayload);

  const PROVIDER_SHORT_LABELS: Record<string, string> = {
    claude: "C",
    codex: "X",
    cursor: "Cr",
  };

  let titlePreview = $derived.by(() => {
    const cfg = current.trayConfig;
    return formatTrayTitle(cfg, PREVIEW_RATE_LIMITS, 17.19);
  });

  let previewBarProviders = $derived(current.trayConfig.barProviders ?? []);

  let verbosePercentagePreview = $derived.by(() => {
    const providers = previewBarProviders.length ? previewBarProviders : RATE_LIMIT_PROVIDER_ORDER;
    return providers
      .map((provider) => `${getUsageProviderTitle(provider)} ${previewUtilization(provider)}%`)
      .join(" ");
  });

  function previewUtilization(provider: RateLimitProviderId): number {
    return PREVIEW_RATE_LIMITS[provider]?.windows[0]?.utilization ?? 0;
  }

  function previewFillStyle(provider: RateLimitProviderId): string {
    const utilization = previewUtilization(provider);
    const color = getUsageProviderBrandColor(provider, 1) ?? "var(--accent)";
    return `width:${Math.max(0, Math.min(utilization, 100))}%; background:${color}`;
  }

  function handleTrayConfig<K extends keyof TrayConfig>(key: K, value: TrayConfig[K]) {
    const next = { ...current.trayConfig, [key]: value };
    updateSetting("trayConfig", next);
    void syncTrayConfig(next, $rateLimitsData).catch(() => {});
  }

  function toggleBarProvider(provider: RateLimitProviderId) {
    const providers = current.trayConfig.barProviders ?? [];
    const next = providers.includes(provider)
      ? providers.filter((p) => p !== provider)
      : [...providers, provider];
    applyBarProviders(next);
  }

  function applyBarProviders(providers: RateLimitProviderId[]) {
    const barDisplay: BarDisplay =
      providers.length === 0 ? "off"
      : providers.length === 1 ? "single"
      : "custom";
    const barProvider = providers[0] ?? current.trayConfig.barProvider;
    const next: TrayConfig = {
      ...current.trayConfig,
      barProviders: providers,
      barDisplay,
      barProvider,
    };
    updateSetting("trayConfig", next);
    void syncTrayConfig(next, $rateLimitsData).catch(() => {});
  }

  async function handleFloatBall(checked: boolean) {
    logger.info("settings", `Float ball: ${checked}`);
    updateSetting("floatBall", checked);
    try {
      if (checked) {
        await invoke("create_float_ball");
      } else {
        await invoke("destroy_float_ball");
      }
    } catch (e) {
      console.error("Failed to toggle floating ball:", e);
    }
  }

  async function handleTaskbarPanel(checked: boolean) {
    logger.info("settings", `Taskbar panel: ${checked}`);
    updateSetting("taskbarPanel", checked);
    try {
      if (checked) {
        await invoke("init_taskbar_panel");
      } else {
        await invoke("destroy_taskbar_panel_cmd");
      }
    } catch (e) {
      console.error("Failed to toggle taskbar panel:", e);
    }
  }
</script>

<div class="card">
  <div class="group-label">
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <rect x="2" y="3" width="20" height="14" rx="2" ry="2"></rect>
      <line x1="8" y1="21" x2="16" y2="21"></line>
      <line x1="12" y1="17" x2="12" y2="21"></line>
    </svg>
    Status Displays
  </div>

  <!-- Combined Preview -->
  <div class="preview-card">
    <div class="preview-row">
      <!-- Menu Bar -->
      <div class="preview-col">
        <span class="preview-label">Menu Bar</span>
        <div class="tray-preview">
          <div class="tp-inner">
            <svg class="tp-icon" width="14" height="14" viewBox="0 0 44 44" fill="none">
              <circle cx="22" cy="22" r="20" fill="currentColor"/>
              <circle cx="16" cy="23" r="3" fill="#262628"/>
              <path d="M28 20l-4 3.5 4 3.5" stroke="#262628" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
            </svg>
            {#if previewBarProviders.length > 0}
              <div class="tp-bars">
                {#if previewBarProviders.length === 1}
                  <div class="tp-track single">
                    <div class="tp-fill" style={previewFillStyle(previewBarProviders[0])}></div>
                  </div>
                {:else}
                  {#each previewBarProviders as provider}
                    <div class="tp-track"><div class="tp-fill" style={previewFillStyle(provider)}></div></div>
                  {/each}
                {/if}
              </div>
            {/if}
            {#if titlePreview}
              <span class="tp-text">{titlePreview}</span>
            {/if}
          </div>
        </div>
      </div>

      <!-- Floating Ball -->
      {#if usesFloatingStatusWidget() && current.floatBall}
        <div class="preview-col">
          <span class="preview-label">Floating Ball</span>
          <div class="fb-preview">
            <div class="fb-capsule">
              <div class="fb-panel">
                {#if previewBarProviders.length > 0}
                  <div class="fb-bars">
                    {#each previewBarProviders as provider}
                      <div class="fb-row">
                        <span class="fb-tag" style:color={getUsageProviderBrandColor(provider, 1)}>{PROVIDER_SHORT_LABELS[provider] ?? provider[0]?.toUpperCase()}</span>
                        <div class="fb-track">
                          {#if previewUtilization(provider) > 0}
                            <div class="fb-fill" style={previewFillStyle(provider)}></div>
                          {/if}
                        </div>
                        <span class="fb-pct" style:color={getUsageProviderBrandColor(provider, 1)}>{previewUtilization(provider)}%</span>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
              <div class="fb-ball"><span class="fb-cost">$17</span></div>
            </div>
          </div>
        </div>
      {/if}
    </div>
  </div>

  {#if usesFloatingStatusWidget()}
    <div class="section border-top">
      <div class="row" class:border={isWindows()}>
        <span class="label">Floating Ball</span>
        <ToggleSwitch
          checked={current.floatBall}
          onChange={handleFloatBall}
        />
      </div>
      {#if isWindows()}
      <div class="row">
        <span class="label">Taskbar Panel</span>
        <ToggleSwitch
          checked={current.taskbarPanel}
          onChange={handleTaskbarPanel}
        />
      </div>
      {/if}
    </div>
  {/if}

  <!-- Bars card -->
  <div class="section border-top">
    <div class="row">
      <span class="label">Bars</span>
      <div class="provider-chips">
        <button
          class="chip off-chip"
          class:active={previewBarProviders.length === 0}
          onclick={() => applyBarProviders([])}
        >Off</button>
        {#each RATE_LIMIT_PROVIDER_ORDER as provider}
          <button
            class="chip"
            class:active={previewBarProviders.includes(provider)}
            style:--chip-color={getUsageProviderBrandColor(provider, 1)}
            onclick={() => toggleBarProvider(provider)}
          >{getUsageProviderLabel(provider)}</button>
        {/each}
      </div>
    </div>
  </div>

  <!-- Percentages card -->
  <div class="section border-top">
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
          { value: "verbose", label: verbosePercentagePreview },
        ]}
        value={current.trayConfig.percentageFormat}
        onChange={(v) => handleTrayConfig("percentageFormat", v as TrayConfig["percentageFormat"])}
      />
    </div>
  </div>

  <!-- Cost card -->
  <div class="section border-top">
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

<style>
  .card {
    background: var(--surface-2);
    border-radius: 8px;
    overflow: hidden;
    margin-bottom: 8px;
  }
  .section {
    overflow: hidden;
  }
  .border-top {
    border-top: 1px solid var(--border-subtle);
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
  .row.dim {
    opacity: 0.25;
    pointer-events: none;
    transition: opacity 0.15s ease;
  }
  .label {
    font: 400 10px/1 'Inter', sans-serif;
    color: var(--t1);
  }

  /* Combined preview card */
  .preview-card {
    padding: 8px 10px;
    border-top: 1px solid var(--border-subtle);
  }
  .preview-row {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }
  .preview-col {
    flex: 1;
    min-width: 0;
  }
  .preview-label {
    display: block;
    font: 500 8px/1 'Inter', sans-serif;
    color: var(--t4);
    letter-spacing: 0.3px;
    margin-bottom: 6px;
  }

  /* Provider chips */
  .provider-chips {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .chip {
    font: 500 9px/1 'Inter', sans-serif;
    padding: 4px 8px;
    border-radius: 6px;
    border: 1px solid var(--border-subtle);
    background: var(--surface-3);
    color: var(--t3);
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .chip:hover {
    border-color: var(--t4);
  }
  .chip.active {
    background: color-mix(in srgb, var(--chip-color, var(--accent)) 15%, transparent);
    color: var(--chip-color, var(--accent));
    border-color: color-mix(in srgb, var(--chip-color, var(--accent)) 40%, transparent);
  }
  .chip.off-chip.active {
    --chip-color: var(--t2);
  }

  /* Floating Ball preview — matches actual FloatBall.svelte capsule */
  .fb-preview {
    display: flex;
    justify-content: center;
  }
  .fb-capsule {
    display: flex;
    align-items: center;
    border-radius: 999px;
    background:
      radial-gradient(150% 150% at 20% 10%, rgba(255, 255, 255, 0.12) 0%, rgba(255, 255, 255, 0) 40%),
      linear-gradient(160deg, rgba(20, 24, 32, 0.95) 0%, rgba(8, 10, 14, 0.99) 100%);
    box-shadow:
      inset 0 0 0 1px rgba(255, 255, 255, 0.1),
      inset 0 2px 4px rgba(255, 255, 255, 0.12),
      inset 0 -4px 12px rgba(0, 0, 0, 0.8);
    padding: 6px 6px 6px 10px;
    gap: 6px;
    min-height: 40px;
  }
  .fb-panel {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }
  .fb-bars {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .fb-row {
    display: grid;
    grid-template-columns: 12px 1fr 20px;
    align-items: center;
    gap: 5px;
  }
  .fb-tag {
    font: 800 7px/1 'Inter', sans-serif;
    text-shadow: 0 0 5px currentColor;
    opacity: 0.9;
  }
  .fb-track {
    height: 4px;
    border-radius: 999px;
    overflow: hidden;
    background: rgba(0, 0, 0, 0.4);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.8), 0 1px 0 rgba(255, 255, 255, 0.05);
  }
  .fb-fill {
    height: 100%;
    border-radius: inherit;
  }
  .fb-pct {
    font: 700 8px/1 'Inter', sans-serif;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .fb-ball {
    width: 28px;
    height: 28px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: transparent;
  }
  .fb-cost {
    font: 700 8px/1 'Inter', sans-serif;
    color: rgba(255, 255, 255, 0.95);
    font-variant-numeric: tabular-nums;
  }

  /* Tray preview (Menu Bar) */
  .tray-preview {
    display: flex;
    justify-content: center;
  }
  .tp-inner {
    display: flex;
    align-items: center;
    gap: 5px;
    background: #262628;
    border-radius: 5px;
    padding: 4px 8px;
    height: 22px;
    border: 0.5px solid rgba(255,255,255,0.06);
  }
  .tp-icon {
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
  .tp-text {
    font: 400 10px/1 'Inter', -apple-system, sans-serif;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.2px;
    color: rgba(255,255,255,0.88);
    white-space: nowrap;
  }
</style>