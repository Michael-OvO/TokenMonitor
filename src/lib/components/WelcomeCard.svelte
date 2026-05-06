<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { settings, updateSetting } from "../stores/settings.js";
  import { logger } from "../utils/logger.js";
  import PermissionDisclosure from "./PermissionDisclosure.svelte";

  interface Props {
    onDismiss: () => void;
  }
  let { onDismiss }: Props = $props();

  let enableRateLimits = $state($settings.rateLimitsEnabled);
  let enableAutostart = $state($settings.launchAtLogin);
  let busy = $state(false);

  async function persistChoicesAndDismiss() {
    try {
      await updateSetting("rateLimitsEnabled", enableRateLimits);
      await updateSetting("launchAtLogin", enableAutostart);
      await updateSetting("hasSeenWelcome", true);
      await invoke("set_rate_limits_enabled", { enabled: enableRateLimits });
      if (enableAutostart) {
        await invoke("plugin:autostart|enable").catch((e) => logger.debug("welcome", `autostart enable failed: ${e}`));
      }
    } catch (e) {
      logger.error("welcome", `Failed to persist welcome choices: ${e}`);
    }
    onDismiss();
  }

  async function handleGetStarted() {
    if (busy) return;
    busy = true;
    await persistChoicesAndDismiss();
    busy = false;
  }
</script>

<div class="welcome" role="dialog" aria-labelledby="welcome-title">
  <div class="welcome-body">
    <div class="welcome-header">
      <h1 id="welcome-title" class="welcome-title">Welcome to TokenMonitor</h1>
      <p class="welcome-lede">
        Your Claude Code and Codex usage, tracked locally.
        Nothing leaves your machine.
      </p>
    </div>

    <PermissionDisclosure mode="welcome" />

    <div class="opt-group" aria-label="Optional features">
      <p class="opt-group-label">Optional features</p>

      <label class="opt-row">
        <input type="checkbox" bind:checked={enableRateLimits} />
        <span class="opt-text">
          <span class="opt-title">Live rate limits</span>
          <span class="opt-hint">
            Computed locally from a small statusline script Claude Code runs on every prompt.
          </span>
        </span>
      </label>

      <label class="opt-row">
        <input type="checkbox" bind:checked={enableAutostart} />
        <span class="opt-text">
          <span class="opt-title">Start at login</span>
          <span class="opt-hint">Open automatically when you sign in.</span>
        </span>
      </label>
    </div>

    <p class="welcome-reassure">
      TokenMonitor never opens a Keychain, notification, or folder permission prompt — live limits are computed entirely from local files.
    </p>

    <button class="welcome-cta" type="button" onclick={handleGetStarted} disabled={busy}>
      Get started
    </button>
  </div>
</div>

<style>
  .welcome {
    display: flex;
    flex-direction: column;
    padding: 20px 18px 16px;
    animation: fadeUp var(--t-slow) var(--ease-out) both .08s;
  }
  .welcome-body {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .welcome-header {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .welcome-title {
    font: 600 15px/1.2 "Inter", sans-serif;
    color: var(--t1);
    margin: 0;
  }
  .welcome-lede {
    font: 400 11px/1.5 "Inter", sans-serif;
    color: var(--t2);
    margin: 0;
  }
  .opt-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 10px 6px;
    border-radius: 8px;
    background: var(--surface-2);
  }
  .opt-group-label {
    font: 500 9px/1 "Inter", sans-serif;
    color: var(--t3);
    margin: 0 0 2px;
  }
  .opt-row {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    padding: 4px 2px;
    cursor: pointer;
    border-radius: 5px;
    transition: background var(--t-fast) ease;
  }
  .opt-row:hover { background: var(--surface-hover, rgba(127, 127, 127, 0.08)); }
  .opt-row input[type="checkbox"] {
    margin-top: 2px;
    flex-shrink: 0;
    accent-color: var(--accent, #6366f1);
  }
  .opt-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .opt-title {
    font: 500 11px/1.3 "Inter", sans-serif;
    color: var(--t1);
  }
  .opt-hint {
    font: 400 10px/1.35 "Inter", sans-serif;
    color: var(--t3);
  }

  .welcome-reassure {
    font: 400 10px/1.4 "Inter", sans-serif;
    color: var(--t4);
    margin: 0;
    text-align: center;
  }

  .welcome-cta {
    padding: 9px 14px;
    border: none;
    border-radius: 7px;
    background: var(--accent, #6366f1);
    color: white;
    font: 600 12px/1 "Inter", sans-serif;
    cursor: pointer;
    transition: filter var(--t-fast) ease, transform var(--t-fast) ease;
  }
  .welcome-cta:hover:not(:disabled) { filter: brightness(1.08); }
  .welcome-cta:active:not(:disabled) { transform: translateY(1px); }
  .welcome-cta:disabled { opacity: 0.6; cursor: default; }

</style>
