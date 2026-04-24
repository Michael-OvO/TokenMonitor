<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { isMacOS } from "../utils/platform.js";
  import { settings, updateSetting } from "../stores/settings.js";
  import { logger } from "../utils/logger.js";
  import {
    markClaudeKeychainAccessHandled,
    requestClaudeKeychainAccessOnce,
  } from "../permissions/keychain.js";

  interface Props {
    onDismiss: () => void;
  }
  let { onDismiss }: Props = $props();

  let enableRateLimits = $state($settings.rateLimitsEnabled);
  let enableAutostart = $state($settings.launchAtLogin);
  let showKeychainTutorial = $state(false);
  let busy = $state(false);

  // The Keychain prompt only matters on macOS, and only when the user is
  // about to opt into rate limits without having gone through this flow
  // before. On Windows/Linux Claude credentials sit in a plain file we can
  // read without prompting, so we always skip the tutorial there.
  let needsKeychainStep = $derived(
    isMacOS() && enableRateLimits && !$settings.keychainAccessRequested,
  );

  async function persistChoicesAndDismiss() {
    try {
      await updateSetting("rateLimitsEnabled", enableRateLimits);
      await updateSetting("launchAtLogin", enableAutostart);
      await updateSetting("hasSeenWelcome", true);
      await invoke("set_rate_limits_enabled", { enabled: enableRateLimits });
      if (enableAutostart) {
        await invoke("plugin:autostart|enable").catch(() => {});
      }
    } catch (e) {
      logger.error("welcome", `Failed to persist welcome choices: ${e}`);
    }
    onDismiss();
  }

  async function handleGetStarted() {
    if (busy) return;
    if (needsKeychainStep) {
      // Move to the inline tutorial step instead of dismissing — the user
      // needs to know what's about to happen before macOS pops the prompt.
      showKeychainTutorial = true;
      return;
    }
    busy = true;
    await persistChoicesAndDismiss();
    busy = false;
  }

  async function handleAllowKeychain() {
    if (busy) return;
    busy = true;
    await requestClaudeKeychainAccessOnce("welcome");
    await persistChoicesAndDismiss();
    busy = false;
  }

  async function handleSkipKeychain() {
    if (busy) return;
    busy = true;
    // User chose not to grant access — still record the request so the
    // tutorial never reappears. Rate limits will run via the CLI probe.
    await markClaudeKeychainAccessHandled();
    await persistChoicesAndDismiss();
    busy = false;
  }
</script>

<div class="welcome" role="dialog" aria-labelledby="welcome-title">
  <div class="welcome-body">
    {#if !showKeychainTutorial}
      <div class="welcome-header">
        <h1 id="welcome-title" class="welcome-title">Welcome to TokenMonitor</h1>
        <p class="welcome-lede">
          Your Claude Code and Codex usage, tracked locally.
          Nothing leaves your machine.
        </p>
        <p class="welcome-access">
          TokenMonitor reads Claude Code and Codex session logs from your home
          folder to calculate usage and cost. It does not scan Desktop,
          Documents, Downloads, network volumes, or external drives unless your
          CLI config is stored there.
        </p>
      </div>

      <div class="opt-group" aria-label="Optional features">
        <p class="opt-group-label">Optional features</p>

        <label class="opt-row">
          <input type="checkbox" bind:checked={enableRateLimits} />
          <span class="opt-text">
            <span class="opt-title">Live rate limits</span>
            <span class="opt-hint">
              {isMacOS()
                ? "One-time Keychain setup, then completely silent."
                : "Reads your Claude Code credentials file."}
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
        You can change either of these anytime in Settings.
      </p>

      <button class="welcome-cta" type="button" onclick={handleGetStarted} disabled={busy}>
        {needsKeychainStep ? "Continue" : "Get started"}
      </button>
    {:else}
      <div class="welcome-header">
        <h1 id="welcome-title" class="welcome-title">One-time Keychain setup</h1>
        <p class="welcome-lede">
          Live rate limits read your Claude Code OAuth token from the macOS
          Keychain. macOS will ask you once - after that, TokenMonitor never
          shows another prompt.
        </p>
      </div>

      <ol class="tutorial">
        <li>
          <span class="tutorial-step">1</span>
          <div class="tutorial-text">
            Click <strong>Allow Keychain access</strong> below.
          </div>
        </li>
        <li>
          <span class="tutorial-step">2</span>
          <div class="tutorial-text">
            macOS pops a window titled <em>"token-monitor wants to use your
            confidential information…"</em>.
          </div>
        </li>
        <li>
          <span class="tutorial-step">3</span>
          <div class="tutorial-text">
            Click <strong>Always Allow</strong> (the rightmost button).
            That's it - you're done forever.
          </div>
        </li>
      </ol>

      <p class="welcome-reassure">
        Skip if you'd rather not grant access. Rate limits still work via a
        slower fallback that doesn't touch the Keychain.
      </p>

      <div class="welcome-actions">
        <button
          class="welcome-secondary"
          type="button"
          onclick={handleSkipKeychain}
          disabled={busy}
        >
          Skip
        </button>
        <button
          class="welcome-cta welcome-cta-flex"
          type="button"
          onclick={handleAllowKeychain}
          disabled={busy}
        >
          Allow Keychain access
        </button>
      </div>
    {/if}
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
  .welcome-access {
    font: 400 10.5px/1.45 "Inter", sans-serif;
    color: var(--t3);
    margin: 4px 0 0;
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

  .welcome-actions {
    display: flex;
    gap: 8px;
  }
  .welcome-cta-flex { flex: 1; }
  .welcome-secondary {
    padding: 9px 14px;
    border: 1px solid var(--border, rgba(127, 127, 127, 0.25));
    border-radius: 7px;
    background: transparent;
    color: var(--t2);
    font: 500 12px/1 "Inter", sans-serif;
    cursor: pointer;
    transition: background var(--t-fast) ease, color var(--t-fast) ease;
  }
  .welcome-secondary:hover:not(:disabled) {
    background: var(--surface-hover, rgba(127, 127, 127, 0.08));
    color: var(--t1);
  }
  .welcome-secondary:disabled { opacity: 0.6; cursor: default; }

  .tutorial {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .tutorial li {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    padding: 6px 8px;
    border-radius: 6px;
    background: var(--surface-2);
  }
  .tutorial-step {
    flex-shrink: 0;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--accent, #6366f1);
    color: white;
    font: 600 10px/18px "Inter", sans-serif;
    text-align: center;
  }
  .tutorial-text {
    font: 400 11px/1.4 "Inter", sans-serif;
    color: var(--t2);
    min-width: 0;
  }
  .tutorial-text strong {
    color: var(--t1);
    font-weight: 600;
  }
  .tutorial-text em {
    color: var(--t1);
    font-style: italic;
  }
</style>
