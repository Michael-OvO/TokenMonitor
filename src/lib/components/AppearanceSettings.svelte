<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    applyGlass,
    applyTheme,
    settings,
    updateSetting,
    type Settings as SettingsType,
  } from "../stores/settings.js";
  import { syncTrayConfig } from "../tray/sync.js";
  import {
    setNativeGlassEffect,
    syncNativeWindowSurface,
    syncNativeWindowTheme,
  } from "../window/appearance.js";
  import { isMacOS, isWindows } from "../utils/platform.js";
  import { logger } from "../utils/logger.js";
  import SegmentedControl from "./SegmentedControl.svelte";
  import ToggleSwitch from "./ToggleSwitch.svelte";

  let current = $derived($settings as SettingsType);

  function handleTheme(val: string) {
    logger.info("settings", `Theme applied: ${val}`);
    const theme = val as SettingsType["theme"];
    updateSetting("theme", theme);
    applyTheme(theme);
    void Promise.allSettled([
      syncNativeWindowTheme(theme),
      syncNativeWindowSurface(invoke, current.glassEffect),
      syncTrayConfig(current.trayConfig, null),
    ]);
  }

  async function handleGlassEffect(checked: boolean) {
    logger.info("settings", `Glass effect: ${checked}`);
    updateSetting("glassEffect", checked);
    applyGlass(checked);
    try {
      await setNativeGlassEffect(checked);
      await syncNativeWindowSurface(invoke, checked);
    } catch (e) {
      console.error("Failed to toggle glass effect:", e);
    }
  }

  function handleBrandTheming(checked: boolean) {
    updateSetting("brandTheming", checked);
  }
</script>

<div class="group">
  <div class="group-label">
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"></path>
    </svg>
    Appearance
  </div>
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
      <span class="label">Brand Theming</span>
      <ToggleSwitch
        checked={current.brandTheming}
        onChange={handleBrandTheming}
      />
    </div>
    {#if isMacOS() || isWindows()}
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
</style>
