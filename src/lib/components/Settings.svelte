<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    getVisibleHeaderProviders,
    settings,
    updateSetting,
    type Settings as SettingsType,
  } from "../stores/settings.js";
  import { clearUsageCache } from "../stores/usage.js";
  import { updaterStore, checkNow, setAutoCheck } from "../stores/updater.js";
  import { isMacOS } from "../utils/platform.js";
  import { currencySymbol } from "../utils/format.js";
  import { logger } from "../utils/logger.js";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import SegmentedControl from "./SegmentedControl.svelte";
  import ToggleSwitch from "./ToggleSwitch.svelte";

  import AppearanceSettings from "./AppearanceSettings.svelte";
  import HeaderTabsSettings from "./HeaderTabsSettings.svelte";
  import TrayConfigSettings from "./TrayConfigSettings.svelte";
  import HiddenModelsSettings from "./HiddenModelsSettings.svelte";
  import SshHostsSettings from "./SshHostsSettings.svelte";
  import UpdateBanner from "./UpdateBanner.svelte";
  import PermissionDisclosure from "./PermissionDisclosure.svelte";

  interface Props {
    onBack: () => void;
  }

  let { onBack }: Props = $props();
  let current = $derived($settings as SettingsType);
  let appVersion = $state("");
  let checking = $state(false);
  let cursorAuthStatus = $state<CursorAuthStatus | null>(null);
  let cursorAuthMessage = $state<string | null>(null);
  let cursorExpanded = $state(false);
  let privacyExpanded = $state(false);
  let cursorRetrying = $state(false);
  let cursorRetryTimer: ReturnType<typeof setTimeout> | null = null;
  let cursorRetryCount = $state(0);
  let costInput = $state("50.00");
  let costEnabled = $state(true);
  let costInputFocused = $state(false);

  $effect(() => {
    costEnabled = current.costAlertThreshold > 0;
    if (!costInputFocused) {
      costInput = current.costAlertThreshold > 0 ? current.costAlertThreshold.toFixed(2) : "50.00";
    }
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
  const CURSOR_RETRY_INTERVAL_MS = 4000;
  const CURSOR_RETRY_MAX = 8;

  type CursorAuthStatus = {
    source: string;
    configured: boolean;
    message: string;
    lastWarning: string | null;
  };

  type UpdateStatus = {
    label: string;
    tone: "ok" | "warn" | "amber" | "neutral";
  };
  let updateStatus = $derived.by<UpdateStatus>(() => {
    if (checking) return { label: "Checking…", tone: "neutral" };
    const s = $updaterStore;
    if (s.available) return { label: `v${s.available.version} available`, tone: "amber" };
    if (s.lastCheckError) return { label: "Unable to check", tone: "warn" };
    if (s.lastCheck) return { label: "Up to date", tone: "ok" };
    return { label: "Not checked yet", tone: "neutral" };
  });

  const currencies = [
    { value: "USD", label: "USD ($)" },
    { value: "EUR", label: "EUR (€)" },
    { value: "GBP", label: "GBP (£)" },
    { value: "JPY", label: "JPY (¥)" },
    { value: "CNY", label: "CNY (¥)" },
  ];

  let defaultProviderOptions = $derived.by(() =>
    getVisibleHeaderProviders(current.headerTabs).map((provider) => ({
      value: provider,
      label: current.headerTabs[provider].label,
    })),
  );


  onMount(() => {
    getVersion().then((v) => { appVersion = v; }).catch((e) => logger.debug("settings", `getVersion failed: ${e}`));
    refreshCursorAuthStatus();
    isEnabled()
      .then((enabled) => {
        if (enabled !== current.launchAtLogin) {
          updateSetting("launchAtLogin", enabled);
        }
      })
      .catch((e) => logger.debug("settings", `isEnabled check failed: ${e}`));
  });

  function handleProvider(val: string) {
    updateSetting("defaultProvider", val as SettingsType["defaultProvider"]);
  }

  function handlePeriod(val: string) {
    updateSetting("defaultPeriod", val as SettingsType["defaultPeriod"]);
  }

  function handleCurrency(val: string) {
    updateSetting("currency", val as string);
  }

  function handleRefresh(val: string) {
    const interval = parseInt(val, 10) || 0;
    logger.info("settings", `Refresh interval IPC: ${interval}s`);
    updateSetting("refreshInterval", interval);
    invoke("set_refresh_interval", { interval }).catch((e) => logger.debug("settings", `set_refresh_interval failed: ${e}`));
  }

  function cursorStatusLabel(status: CursorAuthStatus | null): string {
    if (!status?.configured) return "Not connected";
    if (status.source === "official_api_key") return "Official API key";
    return "Connected";
  }

  function cursorStatusTone(status: CursorAuthStatus | null): UpdateStatus["tone"] {
    if (!status?.configured) return "warn";
    return status.lastWarning ? "amber" : "ok";
  }

  async function refreshCursorAuthStatus() {
    try {
      cursorAuthStatus = await invoke<CursorAuthStatus>("get_cursor_auth_status");
    } catch (error) {
      cursorAuthStatus = null;
      cursorAuthMessage = `Unable to read Cursor auth status: ${error}`;
    }
  }

  async function syncCursorAuth(apiKey = current.cursorApiKey) {
    cursorAuthStatus = await invoke<CursorAuthStatus>("set_cursor_auth_config", {
      apiKey,
    });
    clearUsageCache();
    await invoke("clear_payload_cache").catch((e) => logger.debug("settings", `clear_payload_cache failed: ${e}`));
  }

  async function handleCursorApiKeyInput(value: string) {
    await updateSetting("cursorApiKey", value);
    await syncCursorAuth(value);
  }

  async function openCursorDashboard() {
    const url = "https://cursor.com/dashboard";
    cursorAuthMessage = null;
    logger.info("settings", `Opening Cursor Dashboard: ${url}`);
    try {
      await openUrl(url);
    } catch (error) {
      const message = `Unable to open Cursor Dashboard: ${error}`;
      cursorAuthMessage = message;
      logger.warn("settings", message);
    }
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


  async function handleDockIcon(checked: boolean) {
    logger.info("settings", `Dock icon: ${checked}`);
    updateSetting("showDockIcon", checked);
    try {
      await invoke("set_dock_icon_visible", { visible: checked });
    } catch (e) {
      console.error("Failed to toggle Dock icon visibility:", e);
    }
  }

  async function onCheckUpdatesNow() {
    logger.info("settings", "Manual update check requested");
    checking = true;
    try {
      await checkNow();
    } catch (e) {
      logger.warn("settings", `Update check failed: ${e}`);
    } finally {
      checking = false;
    }
  }

  function formatRelativeTime(iso: string | null): string {
    if (!iso) return "never";
    const diff = Date.now() - new Date(iso).getTime();
    if (diff < 60_000) return "just now";
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
    if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
    return `${Math.floor(diff / 86_400_000)}d ago`;
  }

  let resetStatus = $state<"idle" | "done" | "error">("idle");
  let resetTimer: ReturnType<typeof setTimeout> | null = null;

  async function resetCache() {
    logger.info("settings", "Cache reset by user");
    clearUsageCache();
    try {
      await invoke("clear_cache");
      resetStatus = "done";
    } catch (error) {
      console.error("Failed to clear backend cache:", error);
      resetStatus = "error";
    }
    if (resetTimer) clearTimeout(resetTimer);
    resetTimer = setTimeout(() => { resetStatus = "idle"; }, 2000);
  }

  function stopCursorRetry() {
    cursorRetrying = false;
    cursorRetryCount = 0;
    if (cursorRetryTimer) {
      clearTimeout(cursorRetryTimer);
      cursorRetryTimer = null;
    }
  }

  function scheduleCursorRetry() {
    cursorRetryTimer = setTimeout(async () => {
      cursorRetryCount++;
      try {
        const status = await invoke<CursorAuthStatus>("retry_cursor_auth");
        cursorAuthStatus = status;
        if (status.configured && !status.lastWarning) {
          stopCursorRetry();
          clearUsageCache();
          await invoke("clear_payload_cache").catch(() => {});
          return;
        }
      } catch (_) {
        // Retry silently
      }
      if (cursorRetryCount < CURSOR_RETRY_MAX && cursorRetrying) {
        scheduleCursorRetry();
      } else {
        stopCursorRetry();
      }
    }, CURSOR_RETRY_INTERVAL_MS);
  }

  async function openCursorAndRetry() {
    cursorAuthMessage = null;
    try {
      await invoke("open_cursor_app");
      stopCursorRetry();
      cursorRetrying = true;
      cursorRetryCount = 0;
      scheduleCursorRetry();
    } catch (error) {
      cursorAuthMessage = `Unable to launch Cursor: ${error}`;
    }
  }

  onDestroy(() => {
    stopCursorRetry();
  });
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
    <!-- 1. Appearance -->
    <AppearanceSettings />

    <!-- 2. Display -->
    <div class="group">
      <div class="group-label">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <rect x="2" y="3" width="20" height="14" rx="2" ry="2"></rect>
          <line x1="8" y1="21" x2="16" y2="21"></line>
          <line x1="12" y1="17" x2="12" y2="21"></line>
        </svg>
        Display
      </div>
      <div class="card">
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
        <div class="row">
          <span class="label">Model Change Stats</span>
          <ToggleSwitch
            checked={current.showModelChangeStats}
            onChange={(checked) => updateSetting("showModelChangeStats", checked)}
          />
        </div>
      </div>
    </div>

    <!-- 3. Visibility -->
    <div class="group">
      <div class="group-label">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"></path>
          <circle cx="12" cy="12" r="3"></circle>
        </svg>
        Visibility
      </div>
      <div class="card">
        <HeaderTabsSettings />
        <HiddenModelsSettings />
        <SshHostsSettings />
      </div>
    </div>

    <!-- 4. Menu Bar / Floating Ball -->
    <TrayConfigSettings />

    <!-- 5. Integrations -->
    <div class="group">
      <div class="group-label">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <rect x="2" y="2" width="9" height="9" rx="2" ry="2"></rect>
          <rect x="13" y="2" width="9" height="9" rx="2" ry="2"></rect>
          <rect x="13" y="13" width="9" height="9" rx="2" ry="2"></rect>
          <path d="M8 13L8 22"></path>
          <path d="M3 18L13 18"></path>
        </svg>
        Integrations
      </div>
      <div class="card">
        <button class="row collapsible-toggle" type="button" onclick={() => (cursorExpanded = !cursorExpanded)}>
          <span class="label"><svg class="cursor-icon" width="13" height="13" viewBox="0 0 512 512" fill="currentColor"><path d="m415.035 156.35-151.503-87.4695c-4.865-2.8094-10.868-2.8094-15.733 0l-151.4969 87.4695c-4.0897 2.362-6.6146 6.729-6.6146 11.459v176.383c0 4.73 2.5249 9.097 6.6146 11.458l151.5039 87.47c4.865 2.809 10.868 2.809 15.733 0l151.504-87.47c4.089-2.361 6.614-6.728 6.614-11.458v-176.383c0-4.73-2.525-9.097-6.614-11.459zm-9.516 18.528-146.255 253.32c-.988 1.707-3.599 1.01-3.599-.967v-165.872c0-3.314-1.771-6.379-4.644-8.044l-143.645-82.932c-1.707-.988-1.01-3.599.968-3.599h292.509c4.154 0 6.75 4.503 4.673 8.101h-.007z"/></svg>Cursor</span>
          <div class="collapsible-right">
            <span class="status status-{cursorStatusTone(cursorAuthStatus)}">
              <span class="status-dot"></span>{cursorStatusLabel(cursorAuthStatus)}
            </span>
            <svg class="collapsible-chevron" class:open={cursorExpanded} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="6 9 12 15 18 9"></polyline>
            </svg>
          </div>
        </button>
        <div class="cursor-collapse" class:open={cursorExpanded}>
          <div class="collapse-inner">
            <div class="cursor-section">
            <label class="label" for="cursor-api-key">Official API Key</label>
            <input
              id="cursor-api-key"
              class="secret-input"
              type="password"
              autocomplete="off"
              placeholder="key_..."
              value={current.cursorApiKey}
              oninput={(e) => handleCursorApiKeyInput((e.target as HTMLInputElement).value)}
            />
            <div class="hint">
              Preferred path. Create an Admin API key in Cursor Dashboard settings, then paste it here.
            </div>
            <div class="cursor-actions">
              <button
                type="button"
                class="secondary-btn"
                onclick={openCursorDashboard}
              >
                click me to find your cursor API key
              </button>
            </div>
            <div class="hint">
              In Cursor Dashboard, open Settings → Advanced → Admin API Keys.
            </div>
            {#if cursorAuthMessage || cursorAuthStatus?.lastWarning}
              <div class="cursor-message">{cursorAuthMessage ?? cursorAuthStatus?.lastWarning}</div>
            {#if cursorStatusTone(cursorAuthStatus) === "amber"}
              <div class="cursor-actions" style="margin-top: 6px;">
                <button
                  type="button"
                  class="secondary-btn"
                  disabled={cursorRetrying}
                  onclick={openCursorAndRetry}
                >
                  {cursorRetrying ? "Waiting for Cursor..." : "Open Cursor to refresh token"}
                </button>
              </div>
            {/if}
            {/if}
          </div>
        </div>
      </div>
      </div>
    </div>

    <!-- 6. System -->
    <div class="group">
      <div class="group-label">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="3"></circle>
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
        </svg>
        System
      </div>
      <div class="card">
        <div class="row border">
          <span class="label">Launch at Login</span>
          <ToggleSwitch
            checked={current.launchAtLogin}
            onChange={handleAutostart}
          />
        </div>
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
          <span class="label">Debug Logging</span>
          <ToggleSwitch
            checked={current.debugLogging}
            onChange={handleDebugLogging}
          />
        </div>
        <div class="row center">
          <div class="actions">
            <button class="reset-btn" class:done={resetStatus === "done"} class:error={resetStatus === "error"} onclick={resetCache}>
              {#if resetStatus === "done"}Cache Reset ✓
              {:else if resetStatus === "error"}Reset Failed
              {:else}Reset Cache{/if}
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- 7. Updates -->
    <div class="group">
      <div class="group-label">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 12A9 9 0 0 1 3 12a9 9 0 0 1 15-6.7L21 8"></path>
          <polyline points="21 3 21 8 16 8"></polyline>
          <path d="M12 8v4l3 3"></path>
        </svg>
        Updates
      </div>
      <div class="card">
        <div class="row border">
          <span class="label">Automatic Updates</span>
          <ToggleSwitch
            checked={$updaterStore.autoCheckEnabled}
            onChange={(v) => setAutoCheck(v)}
          />
        </div>
        <div class="row border">
          <span class="label">Current Version</span>
          <div class="value-group">
            <span class="value">v{$updaterStore.currentVersion}</span>
            <span class="status status-{updateStatus.tone}">
              <span class="status-dot"></span>{updateStatus.label}
            </span>
          </div>
        </div>
        <div class="row border">
          <span class="label">Last Checked</span>
          <div class="value-group">
            <span class="value">{formatRelativeTime($updaterStore.lastCheck)}</span>
            <button
              type="button"
              class="refresh-btn"
              class:spinning={checking}
              disabled={checking}
              aria-label="Check for updates"
              title="Check for updates"
              onclick={onCheckUpdatesNow}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <polyline points="23 4 23 10 17 10"></polyline>
                <polyline points="1 20 1 14 7 14"></polyline>
                <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
              </svg>
            </button>
          </div>
        </div>
      </div>
    </div>

    <div class="update-bottom">
      <UpdateBanner />
    </div>

    <!-- 8. Privacy & Permissions -->
    <div class="group">
      <div class="group-label">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect>
          <path d="M7 11V7a5 5 0 0 1 10 0v4"></path>
        </svg>
        Privacy & Permissions
      </div>
      <div class="card">
        <button class="row collapsible-toggle" type="button" onclick={() => (privacyExpanded = !privacyExpanded)}>
          <span class="label">Permissions</span>
          <div class="collapsible-right">
            <svg class="collapsible-chevron" class:open={privacyExpanded} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="6 9 12 15 18 9"></polyline>
            </svg>
          </div>
        </button>
        <div class="privacy-collapse" class:open={privacyExpanded}>
          <div class="collapse-inner">
            <PermissionDisclosure mode="settings" />
          </div>
        </div>
      </div>
    </div>

    <div class="quit-section">
      <button type="button" class="quit-btn" onclick={() => invoke("quit_app")}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"></path>
          <polyline points="16 17 21 12 16 7"></polyline>
          <line x1="21" y1="12" x2="9" y2="12"></line>
        </svg>
        Quit TokenMonitor
      </button>
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
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.15);
    position: relative;
    z-index: 1;
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

  /* `.group-label` is defined globally in `src/app.css` so every
     settings group renders with the same heading weight and size. */

  .card {
    background: var(--surface-2);
    border-radius: 8px;
    overflow: hidden;
  }

  .card > :global(.block:first-child) {
    border-top: none;
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
    transition: transform var(--t-normal) ease;
    transform: rotate(-90deg);
  }
  .collapsible-chevron.open {
    transform: rotate(0deg);
  }
  .cursor-collapse,
  .privacy-collapse {
    display: grid;
    grid-template-rows: 0fr;
    transition: grid-template-rows var(--t-normal) ease;
  }
  .cursor-collapse.open,
  .privacy-collapse.open {
    grid-template-rows: 1fr;
  }
  .collapse-inner {
    overflow: hidden;
  }

  .cursor-section {
    padding: 8px 10px;
  }
  .secret-input {
    width: 100%;
    box-sizing: border-box;
    margin-top: 6px;
    background: var(--surface-hover);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 6px 7px;
    font: 400 10px/1 'Inter', sans-serif;
    color: var(--t1);
    outline: none;
  }
  .secret-input:focus {
    border-color: color-mix(in srgb, var(--accent, #6366f1) 55%, var(--border));
  }
  .hint {
    margin-top: 5px;
    font: 400 8.5px/1.35 'Inter', sans-serif;
    color: var(--t4);
  }
  .cursor-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .cursor-message {
    margin-top: 6px;
    font: 400 8.5px/1.35 'Inter', sans-serif;
    color: #E8A060;
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

  .cursor-icon {
    margin-right: 5px;
    vertical-align: -2px;
    opacity: 0.85;
  }

  .value {
    font: 400 12px/1 'Inter', sans-serif;
    color: var(--t3);
  }
  .value-group {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font: 500 11px/1 'Inter', sans-serif;
    padding-left: 2px;
    transition: color 180ms ease;
  }
  .status-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
    flex-shrink: 0;
    opacity: 0.9;
  }
  .status-ok      { color: var(--ch-plus); }
  .status-amber   { color: #E8A060; }
  .status-warn    { color: var(--ch-minus); }
  .status-neutral { color: var(--t3); }

  .refresh-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: none;
    border: none;
    color: var(--t3);
    cursor: pointer;
    border-radius: 4px;
    transition: color 120ms ease, background 120ms ease;
  }
  .refresh-btn:hover:not(:disabled) {
    color: var(--t1);
    background: var(--surface-hover);
  }
  .refresh-btn:disabled {
    cursor: default;
  }
  .refresh-btn.spinning svg {
    animation: refresh-spin 900ms linear infinite;
    transform-origin: center;
  }
  @keyframes refresh-spin {
    to { transform: rotate(360deg); }
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
  .reset-btn.done {
    color: var(--ch-plus);
  }
  .reset-btn.error {
    color: var(--ch-minus);
  }
  .secondary-btn {
    background: none;
    border: 1px solid var(--border-subtle);
    border-radius: 5px;
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
    cursor: pointer;
    padding: 5px 7px;
  }
  .secondary-btn:hover:not(:disabled) {
    color: var(--t1);
    background: var(--surface-hover);
  }
  .secondary-btn:disabled {
    opacity: .55;
    cursor: default;
  }

  .update-bottom :global(.banner) {
    border-bottom: none;
    border-top: 1px solid var(--border-subtle);
    border-radius: 0 0 8px 8px;
  }

  .quit-section {
    display: flex;
    justify-content: center;
    padding: 12px 0 4px;
  }

  .quit-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    font: 500 11px/1 'Inter', sans-serif;
    color: var(--t2);
    cursor: pointer;
    padding: 7px 14px;
    transition: background var(--t-fast) ease, color var(--t-fast) ease, border-color var(--t-fast) ease;
  }
  .quit-btn:hover {
    background: rgba(208, 104, 104, 0.1);
    color: var(--ch-minus);
    border-color: rgba(208, 104, 104, 0.3);
  }
</style>
