<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { isMacOS } from "../utils/platform.js";
  import { settings, updateSetting } from "../stores/settings.js";
  import {
    requestClaudeKeychainAccessAgain,
    type KeychainAccessOutcome,
  } from "../permissions/keychain.js";
  import { fetchRateLimits } from "../stores/rateLimits.js";
  import { logger } from "../utils/logger.js";
  import ToggleSwitch from "./ToggleSwitch.svelte";

  /**
   * Interactive Privacy & Permissions panel for the Settings view.
   * Replaces the read-only `PermissionDisclosure` so the user can:
   *
   *   - Toggle the local session-log parser on/off (`usageAccessEnabled`)
   *   - Toggle live rate-limit fetching on/off (`rateLimitsEnabled`)
   *   - Re-grant Keychain access on demand
   *   - Open System Settings → Privacy & Security when macOS App Data
   *     access has been denied (the only path back from a TCC denial)
   *
   * State for the OS-level checks (`appDataState`, `keychainState`) is
   * probed on mount and re-probed whenever the user clicks an action that
   * could change it.
   */
  type OsState = "loading" | "granted" | "denied" | "not_applicable";
  type AppDataResp = { status: "granted" | "denied" | "not_applicable" };

  let appDataOsState = $state<OsState>("loading");
  let keychainOsState = $state<OsState>("loading");

  let usageBusy = $state(false);
  let rateLimitsBusy = $state(false);
  let keychainBusy = $state(false);
  let openSettingsBusy = $state(false);
  /** Outcome of the last debug refresh-grant test, surfaced inline so the
   * user can verify Phase 2 (Anthropic OAuth refresh round-trip) without
   * touching devtools. Cleared on next attempt. */
  let refreshTestBusy = $state(false);
  let refreshTestResult = $state<string | null>(null);

  async function detectStates() {
    if (!isMacOS()) {
      appDataOsState = "not_applicable";
      keychainOsState = "not_applicable";
      return;
    }
    try {
      const r = await invoke<AppDataResp>("check_app_data_access");
      appDataOsState =
        r.status === "granted" || r.status === "not_applicable" ? "granted" : "denied";
    } catch (e) {
      logger.error("permissions", `App Data probe failed: ${e}`);
      appDataOsState = "denied";
    }
    try {
      const ok = await invoke<boolean>("check_claude_keychain_access");
      keychainOsState = ok ? "granted" : "denied";
    } catch (e) {
      logger.error("permissions", `Keychain probe failed: ${e}`);
      keychainOsState = "denied";
    }
  }

  $effect(() => {
    void detectStates();
  });

  async function handleUsageToggle(enabled: boolean) {
    if (usageBusy) return;
    usageBusy = true;
    try {
      await updateSetting("usageAccessEnabled", enabled);
      await invoke("set_usage_access_enabled", { enabled });
    } catch (e) {
      logger.error("permissions", `Usage access toggle failed: ${e}`);
    } finally {
      usageBusy = false;
    }
  }

  async function handleRateLimitsToggle(enabled: boolean) {
    if (rateLimitsBusy) return;
    rateLimitsBusy = true;
    try {
      await updateSetting("rateLimitsEnabled", enabled);
      await invoke("set_rate_limits_enabled", { enabled });
      if (enabled) {
        // Force a fetch so the bars populate immediately rather than
        // waiting for the next 30s background tick.
        void fetchRateLimits("claude", { force: true });
      }
    } catch (e) {
      logger.error("permissions", `Rate-limits toggle failed: ${e}`);
    } finally {
      rateLimitsBusy = false;
    }
  }

  async function handleReGrantKeychain() {
    if (keychainBusy) return;
    keychainBusy = true;
    try {
      const outcome: KeychainAccessOutcome =
        await requestClaudeKeychainAccessAgain("settings");
      if (outcome.status === "granted") {
        keychainOsState = "granted";
        if (!$settings.rateLimitsEnabled) {
          await updateSetting("rateLimitsEnabled", true);
          await invoke("set_rate_limits_enabled", { enabled: true });
        }
        void fetchRateLimits("claude", { force: true });
      }
    } catch (e) {
      logger.error("permissions", `Keychain re-grant failed: ${e}`);
    } finally {
      keychainBusy = false;
    }
  }

  /**
   * Test the OAuth refresh-grant flow against Anthropic without waiting for
   * a real 401. Reads the refresh token from the owned mirror, POSTs to
   * Anthropic's token endpoint, writes the new tokens back. Used to verify
   * Phase 2 works for this user's mirror.
   */
  async function handleVerifyRefresh() {
    if (refreshTestBusy) return;
    refreshTestBusy = true;
    refreshTestResult = null;
    try {
      const outcome = await invoke<string>("debug_force_oauth_refresh");
      refreshTestResult = outcome;
      logger.info("permissions", `refresh-grant verification: ${outcome}`);
    } catch (e) {
      refreshTestResult = `error: ${e}`;
      logger.error("permissions", `refresh-grant verification failed: ${e}`);
    } finally {
      refreshTestBusy = false;
    }
  }

  async function handleOpenAppDataSettings() {
    if (openSettingsBusy) return;
    openSettingsBusy = true;
    try {
      await invoke("open_app_data_settings");
    } catch (e) {
      logger.error("permissions", `Open System Settings failed: ${e}`);
    } finally {
      // Brief delay so the click feedback feels intentional even though
      // the actual re-probe will happen when the user comes back.
      setTimeout(() => { openSettingsBusy = false; void detectStates(); }, 600);
    }
  }
</script>

<div class="ps-card">
  <!-- Session Logs ───────────────────────────────────────────── -->
  <div class="ps-row ps-row-head">
    <div class="ps-meta">
      <div class="ps-title">Session Logs</div>
      <div class="ps-sub">Read Claude Code &amp; Codex usage from your home directory.</div>
    </div>
    <ToggleSwitch
      checked={$settings.usageAccessEnabled}
      onChange={handleUsageToggle}
    />
  </div>
  {#if isMacOS()}
    <div class="ps-row ps-row-status">
      <span
        class="ps-status"
        class:status-ok={appDataOsState === "granted"}
        class:status-warn={appDataOsState === "denied"}
        class:status-loading={appDataOsState === "loading"}
      >
        <span class="ps-dot"></span>
        {appDataOsState === "granted"
          ? "macOS App Data access allowed"
          : appDataOsState === "denied"
            ? "macOS App Data access denied"
            : "Checking…"}
      </span>
      {#if appDataOsState === "denied"}
        <button
          type="button"
          class="ps-action"
          onclick={handleOpenAppDataSettings}
          disabled={openSettingsBusy}
        >
          {openSettingsBusy ? "Opening…" : "Open System Settings →"}
        </button>
      {/if}
    </div>
  {/if}

  <div class="ps-divider"></div>

  <!-- Live Rate Limits ───────────────────────────────────────── -->
  <div class="ps-row ps-row-head">
    <div class="ps-meta">
      <div class="ps-title">Live Rate Limits</div>
      <div class="ps-sub">Fetch 5-hour and weekly windows from Anthropic.</div>
    </div>
    <ToggleSwitch
      checked={$settings.rateLimitsEnabled}
      onChange={handleRateLimitsToggle}
    />
  </div>
  {#if isMacOS() && $settings.rateLimitsEnabled}
    <div class="ps-row ps-row-status">
      <span
        class="ps-status"
        class:status-ok={keychainOsState === "granted"}
        class:status-warn={keychainOsState === "denied"}
        class:status-loading={keychainOsState === "loading"}
      >
        <span class="ps-dot"></span>
        {keychainOsState === "granted"
          ? "Keychain authorized"
          : keychainOsState === "denied"
            ? "Keychain not authorized"
            : "Checking…"}
      </span>
      {#if keychainOsState !== "granted"}
        <button
          type="button"
          class="ps-action"
          onclick={handleReGrantKeychain}
          disabled={keychainBusy}
        >
          {keychainBusy ? "Opening prompt…" : "Re-grant Keychain →"}
        </button>
      {/if}
    </div>
    {#if keychainOsState !== "granted"}
      <!-- macOS keychain ACL sheet has three buttons (Always Allow / Deny /
           Allow) and "Always Allow" is the leftmost — easy to mistake for
           "Allow" (rightmost, default-highlighted). Make the instruction
           impossible to miss before the user clicks the action button. -->
      <div class="ps-callout">
        <span class="ps-callout-icon" aria-hidden="true">
          <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="8" cy="8" r="6.5"/>
            <path d="M8 4.5v4"/>
            <circle cx="8" cy="11.2" r="0.6" fill="currentColor"/>
          </svg>
        </span>
        <span>
          When the macOS prompt opens, click
          <strong>Always&nbsp;Allow</strong>
          (leftmost button, not the highlighted "Allow")
        </span>
      </div>
    {/if}
    <!-- Phase 2 verification: exchanges the mirror's refresh token at
         Anthropic's OAuth endpoint and writes the new access token back.
         Lets the user confirm rotations will be handled silently when
         Anthropic actually rotates the access token in the wild. -->
    {#if keychainOsState === "granted"}
      <div class="ps-row ps-row-status">
        <span
          class="ps-status"
          class:status-ok={refreshTestResult === "refreshed"}
          class:status-warn={refreshTestResult && refreshTestResult !== "refreshed"}
          class:status-loading={refreshTestBusy || !refreshTestResult}
        >
          <span class="ps-dot"></span>
          {#if refreshTestBusy}
            Calling Anthropic…
          {:else if refreshTestResult === "refreshed"}
            Token rotation verified
          {:else if refreshTestResult}
            Refresh outcome: {refreshTestResult}
          {:else}
            Token rotation: not yet verified
          {/if}
        </span>
        <button
          type="button"
          class="ps-action"
          onclick={handleVerifyRefresh}
          disabled={refreshTestBusy}
        >
          {refreshTestBusy ? "Verifying…" : "Verify refresh →"}
        </button>
      </div>
    {/if}
  {/if}
</div>

<style>
  .ps-card {
    background: var(--surface-2);
    border-radius: 8px;
    overflow: hidden;
  }
  .ps-row {
    padding: 9px 12px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
  }
  .ps-row-head { padding-bottom: 6px; }
  .ps-row-status {
    padding-top: 0;
    padding-bottom: 9px;
    gap: 8px;
  }
  .ps-meta {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .ps-title {
    font: 500 11px/1.2 'Inter', sans-serif;
    color: var(--t1);
    letter-spacing: -0.05px;
  }
  .ps-sub {
    font: 400 9.5px/1.4 'Inter', sans-serif;
    color: var(--t3);
  }
  .ps-divider {
    height: 1px;
    background: var(--border-subtle);
    margin: 0;
  }

  /* Status pill with a colored dot — matches the app's existing
     status-indicator language used in the Updates row. */
  .ps-status {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font: 500 9.5px/1 'Inter', sans-serif;
    color: var(--t3);
  }
  .ps-dot {
    width: 5px; height: 5px;
    border-radius: 50%;
    background: currentColor;
    flex-shrink: 0;
    opacity: 0.9;
  }
  .status-ok      { color: var(--ch-plus); }
  .status-warn    { color: #E8A060; }
  .status-loading { color: var(--t4); }

  .ps-action {
    appearance: none;
    border: none;
    background: transparent;
    color: var(--accent, #1f8cff);
    font: 500 9.5px/1 'Inter', sans-serif;
    cursor: pointer;
    padding: 3px 4px;
    border-radius: 4px;
    transition: background var(--t-fast, 120ms) ease, color var(--t-fast, 120ms) ease;
  }
  .ps-action:hover:not(:disabled) {
    background: var(--accent-soft, rgba(255,255,255,0.06));
  }
  .ps-action:disabled {
    cursor: default;
    opacity: 0.55;
  }

  /* Always-Allow guidance callout. Sits below the keychain status row so
     the user reads it before clicking the action button — there's no
     "click-then-instruct" race. Uses an info icon + amber tint so it
     reads as guidance rather than an error. */
  .ps-callout {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    margin: 4px 12px 10px;
    padding: 8px 10px;
    background: rgba(232, 160, 96, 0.10);
    border: 1px solid rgba(232, 160, 96, 0.22);
    border-radius: 7px;
    font: 400 10px/1.4 'Inter', sans-serif;
    color: var(--t2);
    letter-spacing: -0.05px;
  }
  .ps-callout strong {
    color: var(--t1);
    font-weight: 600;
  }
  .ps-callout-icon {
    display: inline-flex;
    flex-shrink: 0;
    color: #E8A060;
    margin-top: 1px;
  }
</style>
