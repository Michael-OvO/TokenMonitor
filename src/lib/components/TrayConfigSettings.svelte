<script lang="ts">
  import { settings, updateSetting, type Settings as SettingsType } from "../stores/settings.js";
  import { rateLimitsData } from "../stores/rateLimits.js";
  import { syncTrayConfig } from "../tray/sync.js";
  import { formatTrayTitle } from "../tray/title.js";
  import { usesFloatingStatusWidget } from "../utils/platform.js";
  import {
    getRateLimitPrimaryWindowId,
    getUsageProviderBrandColor,
    getUsageProviderLabel,
    getUsageProviderTitle,
    RATE_LIMIT_PROVIDER_ORDER,
  } from "../providerMetadata.js";
  import type {
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
      stale: false,
      error: null,
      cooldownUntil: null,
      retryAfterSeconds: null,
      fetchedAt: "",
    };
    return payload;
  }, {} as RateLimitsPayload);

  let titlePreview = $derived.by(() => {
    const cfg = current.trayConfig;
    return formatTrayTitle(cfg, PREVIEW_RATE_LIMITS, 17.19);
  });

  let previewBarDisplay = $derived(current.trayConfig.barDisplay);
  let previewBarProvider = $derived(current.trayConfig.barProvider);
  const statusWidgetLabel = usesFloatingStatusWidget() ? "Floating Ball" : "Menu Bar";
  const statusWidgetNote = usesFloatingStatusWidget()
    ? "These settings control the floating summary widget on Windows and Linux."
    : null;
  let verbosePercentagePreview = $derived.by(() =>
    RATE_LIMIT_PROVIDER_ORDER
      .map((provider) => `${getUsageProviderTitle(provider)} ${previewUtilization(provider)}%`)
      .join(" "),
  );

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
</script>

<div class="group">
  <div class="group-label">{statusWidgetLabel}</div>

  {#if usesFloatingStatusWidget()}
    <div class="widget-preview">
      <div class="widget-shell">
        <div class="widget-panel">
          <div class="widget-eyebrow">Status</div>
          <div class="widget-title">{titlePreview || "Widget active"}</div>
          {#if previewBarDisplay === 'both'}
            <div class="widget-bars">
              {#each RATE_LIMIT_PROVIDER_ORDER as provider}
                <div class="widget-row">
                  <span class="widget-tag">{provider === 'claude' ? 'C' : 'X'}</span>
                  <div class="widget-track"><div class="widget-fill" style={previewFillStyle(provider)}></div></div>
                </div>
              {/each}
            </div>
          {:else if previewBarDisplay === 'single'}
            <div class="widget-bars">
              <div class="widget-row">
                <span class="widget-tag">{previewBarProvider === 'claude' ? 'C' : 'X'}</span>
                <div class="widget-track"><div class="widget-fill" style={previewFillStyle(previewBarProvider)}></div></div>
              </div>
            </div>
          {/if}
        </div>
        <div class="widget-ball">$17</div>
      </div>
    </div>
  {:else}
    <div class="tray-preview">
      <div class="tp-inner">
        <svg class="tp-icon" width="14" height="14" viewBox="0 0 44 44" fill="none">
          <circle cx="22" cy="22" r="20" fill="currentColor"/>
          <circle cx="16" cy="23" r="3" fill="#262628"/>
          <path d="M28 20l-4 3.5 4 3.5" stroke="#262628" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
        </svg>
        {#if previewBarDisplay === 'both'}
          <div class="tp-bars">
            {#each RATE_LIMIT_PROVIDER_ORDER as provider}
              <div class="tp-track"><div class="tp-fill" style={previewFillStyle(provider)}></div></div>
            {/each}
          </div>
        {:else if previewBarDisplay === 'single'}
          <div class="tp-bars">
            <div class="tp-track single">
              <div class="tp-fill" style={previewFillStyle(previewBarProvider)}></div>
            </div>
          </div>
        {/if}
        {#if titlePreview}
          <span class="tp-text">{titlePreview}</span>
        {/if}
      </div>
    </div>
  {/if}
  {#if statusWidgetNote}
    <div class="setting-note">{statusWidgetNote}</div>
  {/if}

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
        options={RATE_LIMIT_PROVIDER_ORDER.map((provider) => ({
          value: provider,
          label: getUsageProviderLabel(provider),
        }))}
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
          { value: "compact", label: "72 \u00b7 35" },
          { value: "verbose", label: verbosePercentagePreview },
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

<style>
  .group {
    margin-bottom: 8px;
  }
  .group-label {
    font: 500 8px/1 'Inter', sans-serif;

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
  .row.dim {
    opacity: 0.25;
    pointer-events: none;
    transition: opacity 0.15s ease;
  }
  .label {
    font: 400 10px/1 'Inter', sans-serif;
    color: var(--t1);
  }
  .setting-note {
    font: 400 8px/1.35 'Inter', sans-serif;
    color: var(--t4);
    padding: 4px 4px 0;
  }

  /* Widget preview (Windows/Linux floating ball) */
  .widget-preview {
    background: var(--surface-2);
    border-radius: 8px;
    padding: 10px;
    margin-bottom: 4px;
    display: flex;
    justify-content: center;
  }
  .widget-shell {
    width: 174px;
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }
  .widget-panel {
    flex: 1;
    min-width: 0;
    border-radius: 12px;
    padding: 8px 10px;
    background: linear-gradient(180deg, rgba(32, 36, 45, 0.96), rgba(17, 20, 27, 0.98));
    border: 1px solid rgba(255,255,255,0.06);
  }
  .widget-eyebrow {
    font: 500 7px/1 'Inter', sans-serif;
    color: rgba(148,163,184,0.92);
    letter-spacing: 0.7px;

    margin-bottom: 4px;
  }
  .widget-title {
    font: 600 10px/1.1 'Inter', sans-serif;
    color: rgba(255,255,255,0.9);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-bottom: 7px;
  }
  .widget-bars {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .widget-row {
    display: grid;
    grid-template-columns: 10px 1fr;
    align-items: center;
    gap: 6px;
  }
  .widget-tag {
    font: 700 8px/1 'Inter', sans-serif;
    color: rgba(255,255,255,0.86);
  }
  .widget-track {
    height: 5px;
    border-radius: 999px;
    background: rgba(255,255,255,0.1);
    overflow: hidden;
  }
  .widget-fill {
    height: 100%;
    border-radius: inherit;
  }
  .widget-ball {
    width: 34px;
    height: 34px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: linear-gradient(160deg, #2d3138 0%, #171a20 100%);
    color: rgba(255,255,255,0.92);
    font: 700 8px/1 'Inter', sans-serif;
    box-shadow: inset 0 1px 1px rgba(255,255,255,0.1);
  }

  /* Tray preview (macOS menu bar) */
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
