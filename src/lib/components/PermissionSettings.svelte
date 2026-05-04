<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { isMacOS } from "../utils/platform.js";
  import { settings, updateSetting } from "../stores/settings.js";
  import {
    checkStatusline,
    installStatusline,
    uninstallStatusline,
    readLatestStatuslinePing,
    type InstalledState,
    type LatestStatuslinePing,
  } from "../permissions/statusline.js";
  import { fetchRateLimits } from "../stores/rateLimits.js";
  import { logger } from "../utils/logger.js";
  import ToggleSwitch from "./ToggleSwitch.svelte";

  /**
   * Interactive Privacy & Permissions panel for the Settings view. Lets the
   * user:
   *   - Toggle the local session-log parser (`usageAccessEnabled`)
   *   - Toggle live rate-limit fetching (`rateLimitsEnabled`)
   *   - Install / uninstall the Claude Code statusline integration
   *   - Open System Settings → Privacy & Security when macOS App Data
   *     access has been denied (the only path back from a TCC denial)
   */
  type OsState = "loading" | "granted" | "denied" | "not_applicable";
  type AppDataResp = { status: "granted" | "denied" | "not_applicable" };
  type StatuslineUiState = "loading" | "installed" | "not_installed" | "script_missing";

  let appDataOsState = $state<OsState>("loading");
  let statuslineState = $state<StatuslineUiState>("loading");
  let lastPing = $state<LatestStatuslinePing | null>(null);

  let usageBusy = $state(false);
  let rateLimitsBusy = $state(false);
  let statuslineBusy = $state(false);
  let openSettingsBusy = $state(false);

  async function detectStates() {
    if (!isMacOS()) {
      appDataOsState = "not_applicable";
    } else {
      try {
        const r = await invoke<AppDataResp>("check_app_data_access");
        appDataOsState =
          r.status === "granted" || r.status === "not_applicable" ? "granted" : "denied";
      } catch (e) {
        logger.error("permissions", `App Data probe failed: ${e}`);
        appDataOsState = "denied";
      }
    }
    try {
      const probe: InstalledState = await checkStatusline();
      statuslineState =
        probe.status === "installed"
          ? "installed"
          : probe.status === "script_missing"
            ? "script_missing"
            : "not_installed";
    } catch (e) {
      logger.error("permissions", `Statusline probe failed: ${e}`);
      statuslineState = "not_installed";
    }
    try {
      lastPing = await readLatestStatuslinePing();
    } catch {
      lastPing = null;
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
        void fetchRateLimits("claude", { force: true });
      }
    } catch (e) {
      logger.error("permissions", `Rate-limits toggle failed: ${e}`);
    } finally {
      rateLimitsBusy = false;
    }
  }

  async function handleInstallStatusline() {
    if (statuslineBusy) return;
    statuslineBusy = true;
    try {
      await installStatusline("settings");
      await detectStates();
      if (!$settings.rateLimitsEnabled) {
        await updateSetting("rateLimitsEnabled", true);
        await invoke("set_rate_limits_enabled", { enabled: true });
      }
      void fetchRateLimits("claude", { force: true });
    } catch (e) {
      logger.error("permissions", `Statusline install failed: ${e}`);
    } finally {
      statuslineBusy = false;
    }
  }

  async function handleUninstallStatusline() {
    if (statuslineBusy) return;
    statuslineBusy = true;
    try {
      await uninstallStatusline("settings");
      await detectStates();
    } catch (e) {
      logger.error("permissions", `Statusline uninstall failed: ${e}`);
    } finally {
      statuslineBusy = false;
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
      setTimeout(() => { openSettingsBusy = false; void detectStates(); }, 600);
    }
  }

  /** Human freshness suffix for the "Connected" status. Returns `null`
   * for anything younger than 5 minutes — the previous behavior, which
   * tick-tocked "12s ago, 13s ago, 14s ago…" in a healthy install,
   * created low-grade churn that read as "alarm" rather than "fine".
   * The user only needs a timing hint once activity has visibly paused.
   */
  function freshnessSuffix(iso: string): string | null {
    const seconds = Math.max(0, Math.round((Date.now() - new Date(iso).getTime()) / 1000));
    if (seconds < 300) return null; // healthy: just say "Connected"
    const minutes = Math.round(seconds / 60);
    if (minutes < 60) return `${minutes} min ago`;
    const hours = Math.round(minutes / 60);
    if (hours < 24) return `${hours} hr ago`;
    const days = Math.round(hours / 24);
    return days === 1 ? "yesterday" : `${days} days ago`;
  }

  /** Re-render the freshness suffix every 30s so an idle row eventually
   * picks up the "5 min ago" transition without the constant 1-Hz refresh
   * the old `formatPing` required. */
  let freshnessTick = $state(0);
  $effect(() => {
    const id = setInterval(() => { freshnessTick += 1; }, 30_000);
    return () => clearInterval(id);
  });
  let connectedSuffix = $derived.by(() => {
    void freshnessTick;
    return lastPing?.lastSeenIso ? freshnessSuffix(lastPing.lastSeenIso) : null;
  });
</script>

<div class="ps-card">
  <!-- Session Logs ───────────────────────────────────────────── -->
  <div class="ps-row ps-row-head">
    <div class="ps-meta">
      <div class="ps-title">Session Logs</div>
      <div class="ps-sub">Track your Claude Code and Codex token usage.</div>
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
          ? "App Data access allowed"
          : appDataOsState === "denied"
            ? "App Data access denied"
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
      <div class="ps-sub">Show your 5-hour and weekly limits as Claude Code reports them.</div>
    </div>
    <ToggleSwitch
      checked={$settings.rateLimitsEnabled}
      onChange={handleRateLimitsToggle}
    />
  </div>

  {#if $settings.rateLimitsEnabled}
    <!-- Status row: status pill on the left, primary CTA only when the
         install actually needs attention. The previous "Uninstall →"
         next to a healthy green dot was the source of the "alarm next
         to a green check" effect — Uninstall now lives below as a quiet
         tertiary action so it's still discoverable but never competes
         with the primary state. -->
    <div class="ps-row ps-row-status">
      <span
        class="ps-status"
        class:status-ok={statuslineState === "installed"}
        class:status-warn={statuslineState === "not_installed" || statuslineState === "script_missing"}
        class:status-loading={statuslineState === "loading"}
      >
        <span class="ps-dot"></span>
        {#if statuslineState === "installed"}
          {connectedSuffix ? `Connected · ${connectedSuffix}` : "Connected"}
        {:else if statuslineState === "script_missing"}
          Statusline needs reinstall
        {:else if statuslineState === "not_installed"}
          Statusline not installed
        {:else}
          Checking…
        {/if}
      </span>
      {#if statuslineState === "not_installed" || statuslineState === "script_missing"}
        <button
          type="button"
          class="ps-action"
          onclick={handleInstallStatusline}
          disabled={statuslineBusy}
        >
          {statuslineBusy
            ? "Installing…"
            : statuslineState === "script_missing"
              ? "Reinstall →"
              : "Install →"}
        </button>
      {/if}
    </div>

    {#if statuslineState === "installed"}
      <!-- Tertiary maintenance action. Same place every time so power
           users can still reach it, but its visual weight (small, muted,
           bottom-aligned) keeps it from reading as a recommended next
           step. -->
      <div class="ps-row ps-row-tertiary">
        <button
          type="button"
          class="ps-action ps-action-tertiary"
          onclick={handleUninstallStatusline}
          disabled={statuslineBusy}
        >
          {statuslineBusy ? "Removing…" : "Remove statusline"}
        </button>
      </div>
    {/if}
  {/if}
</div>

<!-- The Developer panel that previously rendered here (re-run-onboarding
     buttons gated behind `import.meta.env.DEV`) has been removed now that
     v0.12 is the published baseline — the in-app affordance was only
     useful while iterating on the wizard. The same actions remain
     available in `tauri dev` via the `__tmForceOnboard()` and
     `__tmResetOnboarding()` console helpers wired up in
     `lib/bootstrap.ts`, so power users can still re-trigger the flow
     without a visible UI surface that ships to end users. -->


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
    /* Wrap-anywhere safety net at the row level so any oversized word
       (long status text, file path, identifier) breaks within the
       340px popover frame instead of pushing the toggle / action button
       off-screen. */
    overflow-wrap: anywhere;
  }
  /* Flex children of ps-row that contain text must allow themselves to
     shrink below their intrinsic content width. Without `min-width: 0`
     a long status string would force the parent to grow, then either
     overflow the popover or push the trailing action button outside
     the frame. The .ps-meta selector already has this; .ps-status
     needed it added explicitly. */
  .ps-row > * {
    min-width: 0;
  }
  .ps-row-head { padding-bottom: 6px; }
  .ps-row-status {
    padding-top: 0;
    padding-bottom: 9px;
    gap: 8px;
  }
  /* Footer-like row that holds the tertiary "Remove statusline" link.
     Right-aligns the action so it never competes with the green status
     dot above it, and tightens the vertical rhythm so the card doesn't
     gain visual weight from a row that's intentionally low-priority. */
  .ps-row-tertiary {
    padding-top: 0;
    padding-bottom: 8px;
    justify-content: flex-end;
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

  /* Status row is two separate visual layers: a colored dot (the actual
     traffic-light signal) and a neutral-secondary text label. The
     previous design colored both in saturated green for the OK state,
     which made a "fine" row look as alarmed as a "needs attention" row.
     Now the dot alone carries the color and the text inherits a calm
     muted tone, so a healthy install reads as ambient rather than
     announced. The warn state intentionally keeps the colored text
     because that *should* draw the eye. */
  .ps-status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font: 500 10px/1 'Inter', sans-serif;
    color: var(--t2);
    letter-spacing: -0.04px;
  }
  .ps-dot {
    width: 6px; height: 6px;
    border-radius: 50%;
    background: var(--t4);
    flex-shrink: 0;
    transition: background var(--t-fast, 120ms) ease;
  }
  /* OK: dot picks up the green, text stays calm. */
  .status-ok .ps-dot   { background: var(--ch-plus, #34c759); box-shadow: 0 0 0 3px rgba(52, 199, 89, 0.10); }
  .status-warn .ps-dot { background: #E8A060; box-shadow: 0 0 0 3px rgba(232, 160, 96, 0.12); }
  .status-warn         { color: #E8A060; }
  .status-loading      { color: var(--t4); }
  .status-loading .ps-dot { background: var(--t4); }

  .ps-action {
    appearance: none;
    border: none;
    background: transparent;
    color: var(--accent, #1f8cff);
    font: 500 10px/1 'Inter', sans-serif;
    cursor: pointer;
    padding: 3px 4px;
    border-radius: 4px;
    transition: background var(--t-fast, 120ms) ease,
      color var(--t-fast, 120ms) ease;
  }
  .ps-action:hover:not(:disabled) {
    background: var(--accent-soft, rgba(255,255,255,0.06));
  }
  /* Tertiary action: same hit target, but in muted text color so it
     reads as "if you really want to" rather than "do this next." Gains
     the accent color only on hover to confirm interactivity. */
  .ps-action-tertiary {
    color: var(--t3);
    font-weight: 400;
    letter-spacing: -0.02px;
  }
  .ps-action-tertiary:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.04);
    color: var(--t1);
  }
  :global([data-theme="light"]) .ps-action-tertiary:hover:not(:disabled) {
    background: rgba(0, 0, 0, 0.04);
  }
  .ps-action:disabled {
    cursor: default;
    opacity: 0.55;
  }
</style>
