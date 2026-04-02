<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { settings, updateSetting, type Settings as SettingsType } from "../stores/settings.js";
  import { clearUsageCache } from "../stores/usage.js";
  import { isMacOS, isWindows } from "../utils/platform.js";
  import { logger } from "../utils/logger.js";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import ToggleSwitch from "./ToggleSwitch.svelte";

  import ThemeSettings from "./ThemeSettings.svelte";
  import HeaderTabsSettings from "./HeaderTabsSettings.svelte";
  import TrayConfigSettings from "./TrayConfigSettings.svelte";
  import HiddenModelsSettings from "./HiddenModelsSettings.svelte";
  import SshHostsSettings from "./SshHostsSettings.svelte";

  interface Props {
    onBack: () => void;
  }

  let { onBack }: Props = $props();
  let current = $derived($settings as SettingsType);
  let appVersion = $state("");

  const currencies = [
    { value: "USD", label: "USD ($)" },
    { value: "EUR", label: "EUR (\u20ac)" },
    { value: "GBP", label: "GBP (\u00a3)" },
    { value: "JPY", label: "JPY (\u00a5)" },
    { value: "CNY", label: "CNY (\u00a5)" },
  ];

  onMount(() => {
    getVersion().then((v) => { appVersion = v; }).catch(() => {});
    isEnabled()
      .then((enabled) => {
        if (enabled !== current.launchAtLogin) {
          updateSetting("launchAtLogin", enabled);
        }
      })
      .catch(() => {});
  });

  function handleCurrency(val: string) {
    updateSetting("currency", val as string);
  }

  async function handleDebugLogging(checked: boolean) {
    logger.info("settings", `Debug logging: ${checked}`);
    updateSetting("debugLogging", checked);
    try {
      await invoke("set_log_level", { level: checked ? "debug" : "info" });
    } catch (e) {
      console.error("Failed to set log level:", e);
    }
  }

  async function handleAutostart(checked: boolean) {
    logger.info("settings", `Autostart: ${checked}`);
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

  async function handleDockIcon(checked: boolean) {
    logger.info("settings", `Dock icon: ${checked}`);
    updateSetting("showDockIcon", checked);
    try {
      await invoke("set_dock_icon_visible", { visible: checked });
    } catch (e) {
      console.error("Failed to toggle Dock icon visibility:", e);
    }
  }

  async function resetCache() {
    logger.info("settings", "Cache reset by user");
    clearUsageCache();
    try {
      await invoke("clear_cache");
    } catch (error) {
      console.error("Failed to clear backend cache:", error);
    }
  }
</script>

<div class="settings">
  <!-- Header -->
  <div class="header">
    <button class="back" type="button" onclick={onBack}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="15 18 9 12 15 6"></polyline>
      </svg>
      <span>Settings</span>
    </button>
    {#if appVersion}<span class="ver">v{appVersion}</span>{/if}
  </div>

  <div class="scroll">
    <ThemeSettings />
    <HeaderTabsSettings />
    <TrayConfigSettings />
    <HiddenModelsSettings />
    <SshHostsSettings />

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
          <span class="label">Floating Ball</span>
          <ToggleSwitch
            checked={current.floatBall}
            onChange={handleFloatBall}
          />
        </div>
        {#if isWindows()}
        <div class="row border">
          <span class="label">Taskbar Panel</span>
          <ToggleSwitch
            checked={current.taskbarPanel}
            onChange={handleTaskbarPanel}
          />
        </div>
        {/if}
        {#if isMacOS()}
        <div class="row border">
          <span class="label">Show Dock Icon</span>
          <ToggleSwitch
            checked={current.showDockIcon}
            onChange={handleDockIcon}
          />
        </div>
        {/if}
        <div class="row border">
          <span class="label">Debug Logging</span>
          <ToggleSwitch
            checked={current.debugLogging}
            onChange={handleDebugLogging}
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
            <button class="reset-btn" onclick={resetCache}>Reset Cache</button>
          </div>
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  .settings {
    height: 460px;
    display: flex;
    flex-direction: column;
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

  .actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .label {
    font: 400 10px/1 'Inter', sans-serif;
    color: var(--t1);
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
</style>
