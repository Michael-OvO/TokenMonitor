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
  import appIcon from "../assets/app-icon.png";

  /**
   * Three-step onboarding wizard: Welcome → Permissions → Done.
   *
   * Each permission card resolves to one of three concrete states detected
   * up-front via backend probes:
   *
   *   - `granted`  — already authorized; show a check pill, no button
   *   - `request`  — never asked; the button fires the OS prompt directly
   *   - `denied`   — previously denied; the button opens System Settings
   *
   * No "Allow Access" button silently flips a UI state with no system
   * effect — every click does exactly one user-visible thing.
   */
  type AccessState = "loading" | "granted" | "request" | "denied";
  type AppDataResp = { status: "granted" | "denied" | "not_applicable" };

  interface Props {
    keychainAuthorized: boolean;
    /** Called when the user clicks the final "Open TokenMonitor" CTA.
     * The parent owns the "load data → flip hasSeenWelcome → unmount
     * wizard" sequencing; the wizard awaits this before clearing its
     * `finishing` busy state so the button stays in "Loading…" until
     * the dashboard is ready. */
    onFinish: () => void | Promise<void>;
  }

  let { keychainAuthorized, onFinish }: Props = $props();

  const STEP_COUNT = 3;
  let stepIndex = $state(0);

  let appDataState = $state<AccessState>("loading");
  let keychainState = $state<AccessState>("loading");
  let appDataBusy = $state(false);
  let keychainBusy = $state(false);
  let finishing = $state(false);

  /** Probe both permissions on mount, and re-probe whenever the user
   * navigates back to step 2 (in case they granted in System Settings). */
  async function detectStates() {
    if (!isMacOS()) {
      appDataState = "granted";
      keychainState = "granted";
      return;
    }
    try {
      const appData = await invoke<AppDataResp>("check_app_data_access");
      appDataState =
        appData.status === "granted"
          ? "granted"
          : appData.status === "not_applicable"
            ? "granted"
            : "denied";
    } catch (e) {
      logger.error("permissions", `App data probe failed: ${e}`);
      appDataState = "request";
    }
    try {
      const ok = await invoke<boolean>("check_claude_keychain_access");
      keychainState = ok || keychainAuthorized ? "granted" : "request";
    } catch (e) {
      logger.error("permissions", `Keychain probe failed: ${e}`);
      keychainState = "request";
    }
  }

  $effect(() => {
    void detectStates();
  });
  $effect(() => {
    if (stepIndex === 1) void detectStates();
  });

  async function handleAppDataClick() {
    if (appDataBusy || appDataState === "granted") return;
    appDataBusy = true;
    try {
      if (appDataState === "request") {
        // Never-asked path: try a read_dir, which fires the OS prompt the
        // first time. Re-probe afterwards to capture the user's choice.
        await updateSetting("usageAccessEnabled", true);
        await invoke("set_usage_access_enabled", { enabled: true });
        await invoke("request_app_data_access").catch(() => {});
        await detectStates();
      } else {
        // Denied path: macOS won't re-fire the prompt — only the user can
        // flip it back on, so we open the right Settings pane.
        await invoke("open_app_data_settings");
      }
    } finally {
      appDataBusy = false;
    }
  }

  async function handleKeychainClick() {
    if (keychainBusy || keychainState === "granted") return;
    keychainBusy = true;
    try {
      const outcome: KeychainAccessOutcome =
        await requestClaudeKeychainAccessAgain("permissions-onboarding");
      if (outcome.status === "granted") {
        keychainState = "granted";
        await updateSetting("rateLimitsEnabled", true);
        await invoke("set_rate_limits_enabled", { enabled: true });
        // Force fetch so rate-limit bars populate before the user finishes.
        void fetchRateLimits("claude", { force: true });
      }
    } finally {
      keychainBusy = false;
    }
  }

  function nextStep() { if (stepIndex < STEP_COUNT - 1) stepIndex += 1; }
  function prevStep() { if (stepIndex > 0) stepIndex -= 1; }

  async function handleFinish() {
    if (finishing) return;
    finishing = true;
    try {
      // Sync backend flags with the detected OS-level state. This matters
      // for the "TCC already granted before the wizard ran" path, where
      // the user never clicks Allow Access (the card is already
      // "Authorized") — without this the backend's usage_access_enabled
      // and rate_limits_enabled would stay false from bootstrap.
      if (appDataState === "granted") {
        await updateSetting("usageAccessEnabled", true);
        await invoke("set_usage_access_enabled", { enabled: true }).catch(() => {});
      }
      if (keychainState === "granted") {
        await updateSetting("rateLimitsEnabled", true);
        await invoke("set_rate_limits_enabled", { enabled: true }).catch(() => {});
      }
      // Hand off to the parent — it owns the "load data, then dismiss
      // wizard" sequencing so the dashboard never flashes its
      // "No usage data found" empty state during the transition.
      await onFinish();
    } finally {
      finishing = false;
    }
  }

  function appDataLabel(): string {
    if (appDataBusy) {
      return appDataState === "denied" ? "Opening Settings…" : "Requesting…";
    }
    if (appDataState === "granted") return "Authorized";
    if (appDataState === "denied") return "Open System Settings";
    return "Allow Access";
  }
  function keychainLabel(): string {
    if (keychainBusy) return "Opening prompt…";
    if (keychainState === "granted") return "Authorized";
    return "Allow Access";
  }
</script>

<div class="po" role="dialog" aria-labelledby="po-title">
  <div class="po-pager" aria-hidden="true">
    {#each Array.from({ length: STEP_COUNT }, (_, i) => i) as i}
      <span class="po-dot" class:po-dot-active={i === stepIndex}></span>
    {/each}
  </div>

  {#if stepIndex === 0}
    <!-- ── Step 1: Welcome ─────────────────────────────────────── -->
    <div class="po-step po-step-welcome">
      <div class="po-app-icon-wrap" aria-hidden="true">
        <img src={appIcon} alt="" class="po-app-icon" />
      </div>
      <h1 id="po-title" class="po-title po-title-large">TokenMonitor</h1>
      <p class="po-tagline">Your coding agents' usage, in the menu bar.</p>

      <div class="po-feature-list">
        <div class="po-feature">
          <span class="po-feature-rule"></span>
          <div class="po-feature-text">
            <span class="po-feature-title">Live cost &amp; tokens</span>
            <span class="po-feature-sub">Updated continuously from local session logs.</span>
          </div>
        </div>
        <div class="po-feature">
          <span class="po-feature-rule"></span>
          <div class="po-feature-text">
            <span class="po-feature-title">Live rate-limit windows</span>
            <span class="po-feature-sub">5-hour, weekly, per-model — straight from Anthropic.</span>
          </div>
        </div>
        <div class="po-feature">
          <span class="po-feature-rule"></span>
          <div class="po-feature-text">
            <span class="po-feature-title">Stays on this device</span>
            <span class="po-feature-sub">No account, no cloud sync, no telemetry.</span>
          </div>
        </div>
      </div>
    </div>
  {:else if stepIndex === 1}
    <!-- ── Step 2: Permissions ─────────────────────────────────── -->
    <div class="po-step">
      <h1 id="po-title" class="po-title">Grant Permissions</h1>
      <p class="po-lede">
        Read-only access to your coding agents' usage{isMacOS() ? " and Claude credentials" : ""}.
        Everything stays on this device.
      </p>

      <div class="po-cards">
        <div
          class="po-card"
          class:po-card-done={appDataState === "granted"}
          style="--icon-tint: 95, 154, 232;"
        >
          <div class="po-card-row">
            <div class="po-card-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M3 7l4-3h6l2 2h6v11a3 3 0 0 1-3 3H5a2 2 0 0 1-2-2V7z"/>
                <path d="M3 11h18"/>
              </svg>
            </div>
            <div class="po-card-meta">
              <div class="po-card-title-row">
                <span class="po-card-title">App Data Access</span>
                {#if appDataState === "granted"}
                  <span class="po-card-status" aria-label="Authorized">
                    <svg viewBox="0 0 16 16" width="10" height="10" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M3 8.5 L7 12 L13 4"/>
                    </svg>
                  </span>
                {/if}
              </div>
              <div class="po-card-text">Read your CLI session logs</div>
            </div>
          </div>
          <ul class="po-card-bullets">
            <li>Track Claude Code &amp; Codex token usage</li>
            <li>Compute cost, models, and time windows</li>
          </ul>
          {#if appDataState !== "granted"}
            <button
              type="button"
              class="po-btn"
              class:po-btn-primary={appDataState === "request"}
              class:po-btn-secondary={appDataState === "denied"}
              class:po-btn-loading={appDataState === "loading"}
              onclick={handleAppDataClick}
              disabled={appDataBusy || appDataState === "loading"}
            >
              {appDataLabel()}
            </button>
          {/if}
        </div>

        {#if isMacOS()}
          <div
            class="po-card"
            class:po-card-done={keychainState === "granted"}
            style="--icon-tint: 133, 117, 230;"
          >
            <div class="po-card-row">
              <div class="po-card-icon" aria-hidden="true">
                <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                  <circle cx="9" cy="13" r="3"/>
                  <path d="M11.5 11L20 2.5"/>
                  <path d="M16.5 6L19 8.5"/>
                </svg>
              </div>
              <div class="po-card-meta">
                <div class="po-card-title-row">
                  <span class="po-card-title">Keychain Access</span>
                  {#if keychainState === "granted"}
                    <span class="po-card-status" aria-label="Authorized">
                      <svg viewBox="0 0 16 16" width="10" height="10" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M3 8.5 L7 12 L13 4"/>
                      </svg>
                    </span>
                  {/if}
                </div>
                <div class="po-card-text">Live rate-limit windows from Anthropic</div>
              </div>
            </div>
            <ul class="po-card-bullets">
              <li>5-hour and weekly windows</li>
              <li>Per-model breakdowns &amp; extra-usage spend</li>
            </ul>
            {#if keychainState !== "granted"}
              <div class="po-callout">
                <span class="po-callout-icon" aria-hidden="true">
                  <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                    <circle cx="8" cy="8" r="6.5"/>
                    <path d="M8 4.5v4"/>
                    <circle cx="8" cy="11.2" r="0.6" fill="currentColor"/>
                  </svg>
                </span>
                <span>
                  In the macOS prompt, click <strong>Always&nbsp;Allow</strong>
                  (leftmost button) — not the highlighted "Allow"
                </span>
              </div>
              <button
                type="button"
                class="po-btn"
                class:po-btn-primary={keychainState === "request"}
                class:po-btn-loading={keychainState === "loading"}
                onclick={handleKeychainClick}
                disabled={keychainBusy || keychainState === "loading"}
              >
                {keychainLabel()}
              </button>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {:else}
    <!-- ── Step 3: Done ────────────────────────────────────────── -->
    <div class="po-step po-step-done">
      <div class="po-success-mark" aria-hidden="true">
        <svg viewBox="0 0 32 32" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round">
          <path d="M7 16.5 L13.5 23 L25 9"/>
        </svg>
      </div>
      <h1 id="po-title" class="po-title">You're all set</h1>
      <p class="po-lede">
        TokenMonitor will start tracking your usage in the background.
        You can revisit any of these settings from the gear icon in the footer.
      </p>
      <ul class="po-recap">
        <li class:po-recap-on={appDataState === "granted"}>
          <span class="po-recap-tag">{appDataState === "granted" ? "ON" : "OFF"}</span>
          App Data Access
        </li>
        {#if isMacOS()}
          <li class:po-recap-on={keychainState === "granted"}>
            <span class="po-recap-tag">{keychainState === "granted" ? "ON" : "OFF"}</span>
            Live Rate Limits (Keychain)
          </li>
        {/if}
      </ul>
    </div>
  {/if}

  <div
    class="po-foot"
    class:po-foot-centered={stepIndex === 0 || stepIndex === STEP_COUNT - 1}
  >
    {#if stepIndex > 0 && stepIndex < STEP_COUNT - 1}
      <button type="button" class="po-nav-back" onclick={prevStep}>Back</button>
    {/if}

    {#if stepIndex < STEP_COUNT - 1}
      <button type="button" class="po-nav-next" onclick={nextStep}>
        {stepIndex === 0 ? "Get Started" : "Continue"}
      </button>
    {:else}
      <button
        type="button"
        class="po-nav-finish"
        onclick={handleFinish}
        disabled={finishing}
      >
        {finishing ? "Loading…" : "Open TokenMonitor"}
      </button>
    {/if}
  </div>
</div>

<style>
  .po {
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "SF Pro",
      "Inter", "Helvetica Neue", Helvetica, Arial, sans-serif;
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 22px 18px 16px;
    min-height: 480px;
  }

  /* Pager dots */
  .po-pager {
    display: flex;
    gap: 5px;
    margin: 0 auto;
  }
  .po-dot {
    width: 6px; height: 6px; border-radius: 999px;
    background: rgba(255,255,255,0.16);
    transition: width var(--t-normal, 200ms) var(--ease-out, ease),
      background var(--t-normal, 200ms) ease;
  }
  .po-dot-active {
    width: 18px;
    background: rgba(255,255,255,0.55);
  }
  :global([data-theme="light"]) .po-dot { background: rgba(0,0,0,0.16); }
  :global([data-theme="light"]) .po-dot-active { background: rgba(0,0,0,0.55); }

  /* Step containers */
  .po-step {
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: 1;
    animation: poStepRise var(--t-slow, 320ms) var(--ease-out, ease) both;
  }
  @keyframes poStepRise {
    from { transform: translateY(6px); opacity: 0; }
    to   { transform: translateY(0);   opacity: 1; }
  }

  /* ── Welcome step ─ clean neutral hero, no colored halo ───────── */
  .po-step-welcome {
    align-items: stretch;
    text-align: center;
    gap: 14px;
  }
  .po-app-icon-wrap {
    display: flex;
    justify-content: center;
    margin: 10px 0 0;
    padding: 0;
    background: none;
  }
  .po-app-icon {
    width: 92px;
    height: 92px;
    border-radius: 22px;
    /* Solid white plate inside the squircle so the icon's transparent
       regions read against a clean ground (matches how macOS chrome
       renders app icons in alerts). */
    background: #ffffff;
    box-shadow:
      0 14px 30px rgba(0,0,0,0.40),
      0 2px 4px rgba(0,0,0,0.22);
    animation: poIconPop 480ms var(--ease-spring, cubic-bezier(0.34,1.4,0.64,1)) both 80ms;
  }
  @keyframes poIconPop {
    from { transform: scale(0.88); opacity: 0; }
    to   { transform: scale(1);    opacity: 1; }
  }
  .po-title-large {
    font-size: 22px;
    font-weight: 700;
    letter-spacing: -0.42px;
    margin-top: 4px;
  }
  .po-tagline {
    font-size: 12px;
    line-height: 1.5;
    color: var(--t3);
    margin: -2px 16px 0;
    letter-spacing: -0.05px;
  }

  /* Hairline divider above the feature list — clean section break. */
  .po-feature-list {
    display: flex;
    flex-direction: column;
    gap: 0;
    margin: 14px 0 0;
    text-align: left;
    border-top: 1px solid rgba(255,255,255,0.06);
  }
  :global([data-theme="light"]) .po-feature-list {
    border-top-color: rgba(0,0,0,0.06);
  }
  .po-feature {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 12px 4px;
    border-bottom: 1px solid rgba(255,255,255,0.06);
  }
  :global([data-theme="light"]) .po-feature {
    border-bottom-color: rgba(0,0,0,0.06);
  }
  .po-feature:last-child { border-bottom: none; }
  /* Old vertical-rule slot is now invisible — kept in markup for parity
     across themes that may want to restore it. */
  .po-feature-rule { display: none; }
  .po-feature-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .po-feature-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--t1);
    letter-spacing: -0.08px;
  }
  .po-feature-sub {
    font-size: 10.5px;
    line-height: 1.45;
    color: var(--t3);
  }

  /* Common title + lede */
  .po-title {
    font-size: 22px;
    font-weight: 700;
    line-height: 1.18;
    letter-spacing: -0.42px;
    color: var(--t1);
    margin: 0;
    text-align: center;
  }
  /* SwiftUI sheet-style lede: secondary color, regular weight,
     `.body`-class scale (~13px @ our popover ratio), looser leading,
     and a max-width so the line wrap feels intentional rather than
     edge-to-edge. */
  .po-lede {
    font-size: 13px;
    font-weight: 400;
    line-height: 1.45;
    color: var(--t2);
    letter-spacing: -0.06px;
    margin: 6px auto 0;
    max-width: 290px;
    text-align: center;
    text-wrap: balance;
  }

  /* ── Permission cards (SwiftUI-flavored) ─────────────────────────
     Material: subtle vertical gradient + inset top highlight + soft
     drop shadow to give the "lit from above" depth that flat panels
     never quite achieve. The transition list is wide so granted-state
     animations feel cohesive (background + border + shadow all at
     once, with iOS-like easing). */
  .po-cards { display: flex; flex-direction: column; gap: 10px; }
  .po-card {
    position: relative;
    background: linear-gradient(180deg,
      rgba(255,255,255,0.045) 0%,
      rgba(255,255,255,0.018) 100%);
    border: 1px solid rgba(255,255,255,0.075);
    border-radius: 14px;
    padding: 13px 13px 11px;
    display: flex;
    flex-direction: column;
    gap: 9px;
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,0.06),
      0 1px 2px rgba(0,0,0,0.18),
      0 4px 14px rgba(0,0,0,0.06);
    transition:
      background var(--t-normal, 200ms) cubic-bezier(0.4, 0, 0.2, 1),
      border-color var(--t-normal, 200ms) cubic-bezier(0.4, 0, 0.2, 1),
      box-shadow var(--t-normal, 200ms) cubic-bezier(0.4, 0, 0.2, 1);
  }
  :global([data-theme="light"]) .po-card {
    background: linear-gradient(180deg,
      rgba(255,255,255,1) 0%,
      rgba(248,248,250,1) 100%);
    border-color: rgba(0,0,0,0.075);
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,1),
      0 1px 2px rgba(0,0,0,0.04),
      0 4px 14px rgba(0,0,0,0.035);
  }
  .po-card-done {
    background: linear-gradient(180deg,
      rgba(74,212,129,0.08) 0%,
      rgba(74,212,129,0.025) 100%);
    border-color: rgba(74,212,129,0.22);
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,0.07),
      0 1px 2px rgba(0,0,0,0.16),
      0 4px 14px rgba(74,212,129,0.06);
  }
  :global([data-theme="light"]) .po-card-done {
    background: linear-gradient(180deg,
      rgba(74,180,108,0.10) 0%,
      rgba(74,180,108,0.035) 100%);
    border-color: rgba(36,160,86,0.32);
  }

  .po-card-row { display: flex; align-items: flex-start; gap: 11px; }

  /* SF Symbol-style "palette rendering" container — squircle radius,
     diagonal gradient, top-edge highlight, bottom-edge shadow, and a
     tint-colored ambient glow (rather than a neutral drop shadow). */
  .po-card-icon {
    width: 36px;
    height: 36px;
    border-radius: 10px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #fff;
    background: linear-gradient(135deg,
      rgba(var(--icon-tint, 100,140,200), 1) 0%,
      rgba(var(--icon-tint, 100,140,200), 0.74) 100%);
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,0.38),
      inset 0 -0.5px 0 rgba(0,0,0,0.10),
      0 1px 2px rgba(0,0,0,0.18),
      0 2px 8px rgba(var(--icon-tint, 100,140,200), 0.28);
    transition:
      background var(--t-normal, 200ms) cubic-bezier(0.4, 0, 0.2, 1),
      box-shadow var(--t-normal, 200ms) cubic-bezier(0.4, 0, 0.2, 1);
  }
  .po-card-done .po-card-icon {
    background: linear-gradient(135deg,
      rgba(74,212,129,1) 0%,
      rgba(74,212,129,0.74) 100%);
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,0.38),
      inset 0 -0.5px 0 rgba(0,0,0,0.10),
      0 1px 2px rgba(0,0,0,0.18),
      0 2px 8px rgba(74,212,129,0.32);
  }

  .po-card-meta { display: flex; flex-direction: column; gap: 2px; min-width: 0; flex: 1; }
  .po-card-title-row {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
  }
  .po-card-title {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--t1);
    letter-spacing: -0.08px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Inline status pill — replaces the full-width "Authorized" button
     in the granted state, mirroring how iOS Settings shows a green
     check rather than a confirmation CTA. */
  .po-card-status {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: linear-gradient(135deg,
      rgba(74,212,129,1) 0%,
      rgba(74,180,108,0.92) 100%);
    color: #fff;
    flex-shrink: 0;
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,0.4),
      0 1px 3px rgba(74,212,129,0.35);
    animation: poStatusPop 360ms cubic-bezier(0.34, 1.4, 0.64, 1) both;
  }
  @keyframes poStatusPop {
    from { opacity: 0; transform: scale(0.5); }
    to   { opacity: 1; transform: scale(1); }
  }
  .po-card-text { font-size: 10.5px; color: var(--t3); line-height: 1.35; letter-spacing: -0.02px; }
  .po-card-bullets { margin: 0 0 2px; padding: 0 0 0 12px; list-style: none; }
  .po-card-bullets li {
    position: relative;
    font-size: 10px;
    line-height: 1.55;
    color: var(--t3);
  }
  .po-card-bullets li::before {
    content: "•";
    position: absolute;
    left: -10px;
    color: var(--t4);
  }

  /* Always-Allow guidance, shown above the primary action so the user
     reads it before clicking. The macOS prompt's three buttons are
     visually similar and "Always Allow" (leftmost) is easy to miss next
     to the highlighted default "Allow". */
  .po-callout {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    margin: 4px 0 2px;
    padding: 7px 9px;
    background: rgba(232, 160, 96, 0.10);
    border: 1px solid rgba(232, 160, 96, 0.22);
    border-radius: 7px;
    font-size: 10px;
    line-height: 1.4;
    color: var(--t2);
    letter-spacing: -0.05px;
  }
  .po-callout strong {
    color: var(--t1);
    font-weight: 600;
  }
  .po-callout-icon {
    display: inline-flex;
    flex-shrink: 0;
    color: #E8A060;
    margin-top: 1px;
  }

  /* Card buttons — tri-state: primary (Allow), secondary (Open Settings),
     done (Authorized), loading (skeleton). */
  .po-btn {
    align-self: stretch;
    appearance: none;
    border: none;
    border-radius: 8px;
    padding: 8px 14px;
    font-size: 11.5px;
    font-weight: 600;
    line-height: 1;
    cursor: pointer;
    margin-top: 2px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    transition: filter var(--t-fast, 120ms) ease,
      transform var(--t-fast, 120ms) ease,
      background var(--t-fast, 120ms) ease;
  }
  .po-btn-primary {
    background: linear-gradient(180deg, #1f8cff 0%, #0a72e8 100%);
    color: #fff;
    box-shadow:
      0 0.5px 0 rgba(255,255,255,0.30) inset,
      0 1px 2px rgba(0,0,0,0.20);
  }
  .po-btn-primary:hover:not(:disabled) {
    filter: brightness(1.06); transform: translateY(-1px);
  }
  .po-btn-secondary {
    background: rgba(255,255,255,0.10);
    color: var(--t1);
    box-shadow: 0 0.5px 0 rgba(255,255,255,0.06) inset;
  }
  :global([data-theme="light"]) .po-btn-secondary {
    background: rgba(0,0,0,0.06);
  }
  .po-btn-secondary:hover:not(:disabled) {
    background: rgba(255,255,255,0.16);
    transform: translateY(-1px);
  }
  :global([data-theme="light"]) .po-btn-secondary:hover:not(:disabled) {
    background: rgba(0,0,0,0.10);
  }
  .po-btn-loading {
    background: rgba(255,255,255,0.05);
    color: var(--t4);
    cursor: default;
  }
  .po-btn:disabled { cursor: default; }

  /* Done step */
  .po-step-done { align-items: stretch; }
  .po-success-mark {
    width: 56px; height: 56px;
    margin: 4px auto 4px;
    border-radius: 16px;
    background: rgba(74,212,129,0.14);
    color: #4ad481;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 6px 22px rgba(74,212,129,0.20);
    animation: poIconPop 480ms var(--ease-spring, cubic-bezier(0.34,1.4,0.64,1)) both 80ms;
  }
  .po-recap {
    list-style: none;
    margin: 4px 0 0;
    padding: 12px;
    background: rgba(255,255,255,0.025);
    border: 1px solid rgba(255,255,255,0.05);
    border-radius: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  :global([data-theme="light"]) .po-recap {
    background: rgba(0,0,0,0.03);
    border-color: rgba(0,0,0,0.06);
  }
  .po-recap li {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 11.5px;
    color: var(--t2);
  }
  .po-recap-tag {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.4px;
    padding: 2px 6px;
    border-radius: 999px;
    background: rgba(255,255,255,0.10);
    color: var(--t3);
  }
  .po-recap-on .po-recap-tag {
    background: rgba(74,212,129,0.20);
    color: #4ad481;
  }
  .po-recap-on { color: var(--t1); }

  /* Footer navigation. Default layout splits Back / Continue on step 2.
     `.po-foot-centered` is applied on welcome + done where there's only
     a single primary action — keeps the button tightly centered instead
     of stretched to fill the row. */
  .po-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-top: auto;
    padding-top: 4px;
  }
  .po-foot-centered {
    justify-content: center;
  }
  .po-nav-back {
    appearance: none;
    border: none;
    background: transparent;
    color: var(--t2);
    padding: 9px 14px;
    border-radius: 7px;
    font-size: 11.5px;
    font-weight: 500;
    cursor: pointer;
    transition: background var(--t-fast, 120ms) ease, color var(--t-fast, 120ms) ease;
  }
  .po-nav-back:hover {
    background: rgba(255,255,255,0.06);
    color: var(--t1);
  }
  :global([data-theme="light"]) .po-nav-back:hover { background: rgba(0,0,0,0.04); }

  .po-nav-next,
  .po-nav-finish {
    appearance: none;
    border: none;
    border-radius: 8px;
    padding: 10px 22px;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
    background: linear-gradient(180deg, #1f8cff 0%, #0a72e8 100%);
    color: #fff;
    box-shadow:
      0 0.5px 0 rgba(255,255,255,0.30) inset,
      0 1px 2px rgba(0,0,0,0.20);
    transition: filter var(--t-fast, 120ms) ease, transform var(--t-fast, 120ms) ease;
  }
  /* Step 2 (split layout) — Continue sits on the right. */
  .po-foot:not(.po-foot-centered) .po-nav-next {
    margin-left: auto;
  }
  /* Centered layout — give the button a comfortable minimum width so it
     doesn't shrink to its label and look orphaned. */
  .po-foot-centered .po-nav-next,
  .po-foot-centered .po-nav-finish {
    min-width: 180px;
  }
  .po-nav-next:hover:not(:disabled),
  .po-nav-finish:hover:not(:disabled) {
    filter: brightness(1.06);
    transform: translateY(-1px);
  }
  .po-nav-finish:disabled { opacity: 0.6; cursor: default; }
</style>
