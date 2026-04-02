<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    applyGlass,
    applyTheme,
    getVisibleHeaderProviders,
    settings,
    updateSetting,
    type Settings as SettingsType,
  } from "../stores/settings.js";
  import { syncNativeWindowSurface } from "../window/appearance.js";
  import { isMacOS } from "../utils/platform.js";
  import { logger } from "../utils/logger.js";
  import SegmentedControl from "./SegmentedControl.svelte";
  import ToggleSwitch from "./ToggleSwitch.svelte";

  let current = $derived($settings as SettingsType);

  let defaultProviderOptions = $derived.by(() =>
    getVisibleHeaderProviders(current.headerTabs).map((provider) => ({
      value: provider,
      label: current.headerTabs[provider].label,
    })),
  );

  function handleTheme(val: string) {
    logger.info("settings", `Theme applied: ${val}`);
    const theme = val as SettingsType["theme"];
    updateSetting("theme", theme);
    applyTheme(theme);
    void syncNativeWindowSurface(invoke, current.glassEffect).catch(() => {});
  }

  async function handleGlassEffect(checked: boolean) {
    logger.info("settings", `Glass effect: ${checked}`);
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

  function handlePeriod(val: string) {
    updateSetting("defaultPeriod", val as SettingsType["defaultPeriod"]);
  }

  function handleRefresh(val: string) {
    const interval = parseInt(val, 10) || 0;
    logger.info("settings", `Refresh interval IPC: ${interval}s`);
    updateSetting("refreshInterval", interval);
    invoke("set_refresh_interval", { interval }).catch(() => {});
  }
</script>

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
        options={defaultProviderOptions}
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
    {#if isMacOS()}
    <div class="row">
      <span class="label">Glass Effect</span>
      <ToggleSwitch
        checked={current.glassEffect}
        onChange={handleGlassEffect}
      />
    </div>
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
</style>
