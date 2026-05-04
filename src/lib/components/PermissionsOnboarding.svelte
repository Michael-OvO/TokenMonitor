<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { isMacOS } from "../utils/platform.js";
  import { settings, updateSetting } from "../stores/settings.js";
  import {
    checkStatusline,
    installStatusline,
    type InstalledState,
  } from "../permissions/statusline.js";
  import {
    CURRENT_ONBOARDING_VERSION,
    changelogSince,
    type ChangelogEntry,
  } from "../changelog.js";
  import { logger } from "../utils/logger.js";
  import appIcon from "../assets/app-icon.png";

  /**
   * Three-step onboarding wizard: Welcome → Permissions → Done.
   *
   * Every action button does exactly one user-visible thing — no buttons
   * silently flip a UI state. The two cards probed on this step are:
   *   - App Data Access (macOS Sequoia App Data TCC; not applicable on
   *     Windows/Linux so we treat it as already granted there)
   *   - Claude statusline (cross-platform; writes a script and patches
   *     ~/.claude/settings.json — no OS prompt)
   */
  type AccessState = "loading" | "granted" | "request" | "denied";
  type AppDataResp = { status: "granted" | "denied" | "not_applicable" };

  interface Props {
    /** Called when the user clicks the final "Open TokenMonitor" CTA.
     * The parent owns the "load data → flip hasSeenWelcome → unmount
     * wizard" sequencing; the wizard awaits this before clearing its
     * `finishing` busy state so the button stays in "Loading…" until
     * the dashboard is ready. */
    onFinish: () => void | Promise<void>;
  }

  let { onFinish }: Props = $props();

  /** Returning user being re-onboarded into a newer build. We capture this
   * once at mount so the wizard's step layout doesn't shuffle if the
   * setting is mutated mid-flow (e.g. handleFinish writes the new
   * stamp before the component unmounts). */
  const returningUser =
    $settings.lastOnboardedVersion !== null &&
    $settings.lastOnboardedVersion !== CURRENT_ONBOARDING_VERSION;
  /** Changelog entries to render on the What's New step. For returning
   * users this is everything newer than their stored stamp; fresh users
   * see an empty list (and the What's New step is hidden — they get the
   * Welcome step instead). */
  const changelogEntries: ChangelogEntry[] = returningUser
    ? changelogSince($settings.lastOnboardedVersion)
    : [];
  /** Whether to insert the What's New step at all. We show it when we
   * have something to say AND the user is returning — fresh users skip
   * it because "what's new" is meaningless without a "before". The
   * Welcome step (logo + features) is now shown to *both* user types as
   * step 0 so the brand impression is consistent across upgrades. */
  const showWhatsNewStep = returningUser && changelogEntries.length > 0;

  /** Total number of wizard steps. Returning users with changelog
   * entries see one extra "What's New" page inserted between Welcome
   * and Permissions, taking the count from 3 → 4. */
  const STEP_COUNT = showWhatsNewStep ? 4 : 3;
  let stepIndex = $state(0);

  /** Logical name for the current step, decoupled from the numeric
   * `stepIndex` so we can render the same content (Permissions, Done)
   * regardless of whether a What's New step shifted indices. */
  const stepName = $derived.by<"welcome" | "whatsnew" | "permissions" | "done">(() => {
    if (stepIndex === 0) return "welcome";
    if (showWhatsNewStep) {
      if (stepIndex === 1) return "whatsnew";
      if (stepIndex === 2) return "permissions";
      return "done";
    }
    if (stepIndex === 1) return "permissions";
    return "done";
  });

  let appDataState = $state<AccessState>("loading");
  let statuslineState = $state<AccessState>("loading");
  let appDataBusy = $state(false);
  let statuslineBusy = $state(false);
  let finishing = $state(false);

  /** Probe both permissions on mount, and re-probe whenever the user
   * navigates back to step 2 (in case they granted in System Settings). */
  async function detectStates() {
    if (!isMacOS()) {
      appDataState = "granted";
    } else {
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
    }
    try {
      const probe: InstalledState = await checkStatusline();
      statuslineState = probe.status === "installed" ? "granted" : "request";
    } catch (e) {
      logger.error("permissions", `Statusline probe failed: ${e}`);
      statuslineState = "request";
    }
  }

  $effect(() => {
    void detectStates();
  });
  $effect(() => {
    // Re-probe when the user lands on the Permissions step. Decoupled
    // from the numeric index because adding a What's New step shifts
    // Permissions from index 1 → 2 for returning users.
    if (stepName === "permissions") void detectStates();
  });

  async function handleAppDataClick() {
    if (appDataBusy || appDataState === "granted") return;
    appDataBusy = true;
    try {
      if (appDataState === "request") {
        // Never-asked path: a read_dir call fires the OS prompt the first
        // time. Re-probe afterwards to capture the user's choice.
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

  async function handleStatuslineClick() {
    if (statuslineBusy || statuslineState === "granted") return;
    statuslineBusy = true;
    try {
      await installStatusline("permissions-onboarding");
      // Re-probe instead of optimistically flipping; the install path can
      // legitimately return AlreadyInstalled (CC settings already pointed
      // at our script from a previous session) which still counts.
      await detectStates();
      await updateSetting("rateLimitsEnabled", true);
      await invoke("set_rate_limits_enabled", { enabled: true });
    } catch (e) {
      logger.error("permissions", `Statusline install failed: ${e}`);
      statuslineState = "request";
    } finally {
      statuslineBusy = false;
    }
  }

  function nextStep() { if (stepIndex < STEP_COUNT - 1) stepIndex += 1; }
  function prevStep() { if (stepIndex > 0) stepIndex -= 1; }

  async function handleFinish() {
    if (finishing) return;
    finishing = true;
    try {
      // Sync backend flags with the detected OS-level state. Matters for
      // the "TCC already granted before the wizard ran" path, where the
      // user never clicks Allow Access — without this the backend's
      // usage_access_enabled would stay false from bootstrap.
      if (appDataState === "granted") {
        await updateSetting("usageAccessEnabled", true);
        await invoke("set_usage_access_enabled", { enabled: true }).catch(() => {});
      }
      if (statuslineState === "granted") {
        await updateSetting("rateLimitsEnabled", true);
        await invoke("set_rate_limits_enabled", { enabled: true }).catch(() => {});
      }
      // Stamp the current onboarding version so future launches don't
      // re-trigger the migration. Done before onFinish so the parent
      // sees the up-to-date setting on its very first read after the
      // wizard unmounts.
      await updateSetting("lastOnboardedVersion", CURRENT_ONBOARDING_VERSION);
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
  function statuslineLabel(): string {
    if (statuslineBusy) return "Installing…";
    if (statuslineState === "granted") return "Installed";
    return "Install";
  }

  /** Permissions-step progress narrative.
   *
   * `doneCount` drives three reward surfaces:
   *   1. The progress pill below the title ("1 of 2 enabled" → "2 of 2 ready").
   *   2. The lede copy, which shifts from feature description → "almost
   *      there" → "tap Continue when you are" — a quiet conversational
   *      pace that mirrors Apple's onboarding flows.
   *   3. The Continue button's `data-emphasised` attribute, which gives it
   *      a subtle saturation bump once both cards are green so the eye is
   *      drawn to the next move without inventing a separate "ready"
   *      banner. */
  const totalCount = 2;
  let doneCount = $derived(
    (appDataState === "granted" ? 1 : 0) +
      (statuslineState === "granted" ? 1 : 0),
  );
  let ledeCopy = $derived(
    doneCount === totalCount
      ? "Both ready. Tap Continue when you are."
      : doneCount === 0
        ? "Read-only access to your CLI usage logs, plus a small statusline script Claude Code calls on every prompt. Everything stays local."
        : "Almost there. One more to go.",
  );
</script>

<div class="po" role="dialog" aria-labelledby="po-title">
  <div class="po-pager" aria-hidden="true">
    {#each Array.from({ length: STEP_COUNT }, (_, i) => i) as i}
      <span class="po-dot" class:po-dot-active={i === stepIndex}></span>
    {/each}
  </div>

  {#if stepName === "welcome"}
    <!-- ── Step 0 (always): Welcome ────────────────────────────── -->
    <div class="po-step po-step-welcome">
      <div class="po-app-icon-wrap" aria-hidden="true">
        <img src={appIcon} alt="" class="po-app-icon" />
      </div>
      <h1 id="po-title" class="po-title po-title-large">TokenMonitor</h1>
      <span class="po-version-pill po-version-pill-welcome" aria-label="Current version">
        v{CURRENT_ONBOARDING_VERSION}
      </span>
      <p class="po-tagline">
        {returningUser
          ? "Welcome back. Updated and ready."
          : "Your coding agents' usage, in the menu bar."}
      </p>

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
            <span class="po-feature-sub">5-hour and weekly windows from a tiny local statusline.</span>
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
  {:else if stepName === "whatsnew"}
    <!-- ── Step 1 (returning users): What's New ────────────────── -->
    <div class="po-step po-step-whatsnew">
      <div class="po-whatsnew-header">
        <span class="po-version-pill" aria-label="Current version">
          v{CURRENT_ONBOARDING_VERSION}
        </span>
        <h1 id="po-title" class="po-title">What's New</h1>
      </div>
      <!-- Changelog layout: each version's notes are a stack of cards.
           One card per change, with a clean section title and a
           one-sentence body. No technical-detail subsection — the
           highlights themselves are the full story we tell users. -->
      <div class="po-changelog-scroll">
        <ul class="po-changelog">
          {#each changelogEntries as entry}
            <li class="po-changelog-entry">
              <header class="po-changelog-row">
                <span class="po-changelog-version">v{entry.version}</span>
                {#if entry.tag}
                  <span class="po-changelog-tag">{entry.tag}</span>
                {/if}
                <span class="po-changelog-date">{entry.date}</span>
              </header>
              <div class="po-changelog-title">{entry.title}</div>

              <ul class="po-changelog-highlights" aria-label="Highlights">
                {#each entry.highlights as highlight}
                  <li class="po-highlight">
                    <div class="po-highlight-title">{highlight.title}</div>
                    <div class="po-highlight-body">{highlight.description}</div>
                  </li>
                {/each}
              </ul>
            </li>
          {/each}
        </ul>
      </div>
    </div>
  {:else if stepName === "permissions"}
    <!-- ── Step 2: Permissions ─────────────────────────────────── -->
    <div class="po-step">
      <h1 id="po-title" class="po-title">Grant Permissions</h1>
      <div
        class="po-progress"
        data-state={doneCount === totalCount ? "all" : "partial"}
        aria-live="polite"
        aria-label={doneCount === totalCount
          ? "All permissions ready"
          : `${doneCount} of ${totalCount} permissions enabled`}
      >
        <span class="po-progress-num">{doneCount}</span>
        <span class="po-progress-of">of</span>
        <span class="po-progress-num">{totalCount}</span>
      </div>
      <p class="po-lede">{ledeCopy}</p>

      <div class="po-cards">
        <div
          class="po-card"
          data-done={appDataState === "granted" ? "true" : "false"}
          style="--icon-tint: 95, 154, 232;"
        >
          <div class="po-card-row">
            <div class="po-card-icon" aria-hidden="true">
              <!-- A4: terminal window — most accurate semantic for "we read
                   your CLI session logs", the actual data source.  Prompt
                   chevron + blinking-line evokes a live shell. -->
              <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <rect x="3" y="5" width="18" height="14" rx="2"/>
                <path d="M7 9.5l3 2.5-3 2.5"/>
                <line x1="13" y1="14.5" x2="17" y2="14.5"/>
              </svg>
            </div>
            <div class="po-card-meta">
              <div class="po-card-title-row">
                <span class="po-card-title">App Data Access</span>
                {#if appDataState === "granted"}
                  <svg
                    class="po-card-check"
                    aria-label="Authorized"
                    viewBox="0 0 16 16"
                    width="13"
                    height="13"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2.2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                  >
                    <path d="M3 8.6 L6.7 12 L13 4.6"/>
                  </svg>
                {/if}
              </div>
              <div class="po-card-text">Read your CLI session logs</div>
            </div>
          </div>
          <ul class="po-card-bullets">
            <li>Track Claude Code &amp; Codex token usage</li>
            <li>Compute cost, models, and time windows</li>
          </ul>
          {#if appDataState === "granted"}
            <div class="po-card-done-pill" aria-live="polite">
              <svg
                viewBox="0 0 16 16"
                width="11"
                height="11"
                fill="none"
                stroke="currentColor"
                stroke-width="2.4"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
              >
                <path d="M3 8.6 L6.7 12 L13 4.6"/>
              </svg>
              <span>Authorized</span>
            </div>
          {:else}
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

        <div
          class="po-card"
          data-done={statuslineState === "granted" ? "true" : "false"}
          style="--icon-tint: 133, 117, 230;"
        >
          <div class="po-card-row">
            <div class="po-card-icon" aria-hidden="true">
              <!-- S2: curlybraces — the script we install is literally a
                   tiny shell program; the SF-symbol-style braces read as
                   "code/script", clearly distinct from the granted-state
                   checkmark that appears next to the title on success. -->
              <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M9 5c-2 0 -2 2 -2 4 0 2 -1 3 -2 3 1 0 2 1 2 3 0 2 0 4 2 4"/>
                <path d="M15 5c2 0 2 2 2 4 0 2 1 3 2 3 -1 0 -2 1 -2 3 0 2 0 4 -2 4"/>
              </svg>
            </div>
            <div class="po-card-meta">
              <div class="po-card-title-row">
                <span class="po-card-title">Claude Statusline</span>
                {#if statuslineState === "granted"}
                  <svg
                    class="po-card-check"
                    aria-label="Installed"
                    viewBox="0 0 16 16"
                    width="13"
                    height="13"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2.2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                  >
                    <path d="M3 8.6 L6.7 12 L13 4.6"/>
                  </svg>
                {/if}
              </div>
              <div class="po-card-text">Live 5-hour and weekly windows</div>
            </div>
          </div>
          <ul class="po-card-bullets">
            <li>Adds a small entry to ~/.claude/settings.json</li>
            <li>Runs a local script — no Keychain, no network call</li>
          </ul>
          {#if statuslineState === "granted"}
            <div class="po-card-done-pill" aria-live="polite">
              <svg
                viewBox="0 0 16 16"
                width="11"
                height="11"
                fill="none"
                stroke="currentColor"
                stroke-width="2.4"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
              >
                <path d="M3 8.6 L6.7 12 L13 4.6"/>
              </svg>
              <span>Installed</span>
            </div>
          {:else}
            <button
              type="button"
              class="po-btn"
              class:po-btn-primary={statuslineState === "request"}
              class:po-btn-loading={statuslineState === "loading"}
              onclick={handleStatuslineClick}
              disabled={statuslineBusy || statuslineState === "loading"}
            >
              {statuslineLabel()}
            </button>
          {/if}
        </div>
      </div>
    </div>
  {:else if stepName === "done"}
    <!-- ── Final step: Done ────────────────────────────────────── -->
    <div class="po-step po-step-done">
      <div class="po-success-mark" aria-hidden="true">
        <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M5 12.5 L10 17 L19 7.5"/>
        </svg>
      </div>
      <h1 id="po-title" class="po-title">You're all set</h1>
      <p class="po-lede">
        TokenMonitor will start tracking your usage in the background.
        You can revisit any of these settings from the gear icon in the footer.
      </p>
      <ul class="po-recap">
        <li>
          <span class="po-recap-name">App Data Access</span>
          <span class="po-recap-status" class:po-recap-status-on={appDataState === "granted"}>
            {appDataState === "granted" ? "Allowed" : "Off"}
          </span>
        </li>
        <li>
          <span class="po-recap-name">Claude Statusline</span>
          <span class="po-recap-status" class:po-recap-status-on={statuslineState === "granted"}>
            {statuslineState === "granted" ? "Installed" : "Off"}
          </span>
        </li>
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
      <button
        type="button"
        class="po-nav-next"
        data-ready={stepName === "permissions" && doneCount === totalCount ? "true" : "false"}
        onclick={nextStep}
      >
        {stepIndex === 0 && !showWhatsNewStep ? "Get Started" : "Continue"}
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
    /* Wrap-anywhere safety net: the popover frame is fixed at 340px, so
       any single overflowing word (long URL, hash, identifier) needs to
       break instead of pushing the layout. `anywhere` allows breaking
       between any two chars only when nothing shorter would fit, so
       normal prose still wraps at spaces. */
    overflow-wrap: anywhere;
  }

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

  .po-step-welcome {
    align-items: stretch;
    text-align: center;
    gap: 14px;
  }

  /* ── Version pill ────────────────────────────────────────────
     Reused on Welcome (subtle, sits below the wordmark) and What's
     New (header-prominent). Same surface treatment as the existing
     SwiftUI-flavored cards: subtle stroke + pillow gradient + soft
     drop shadow so it reads as a tappable chip even though it's
     non-interactive. */
  .po-version-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 9px;
    border-radius: 999px;
    background: linear-gradient(180deg,
      rgba(255,255,255,0.07) 0%,
      rgba(255,255,255,0.025) 100%);
    border: 1px solid rgba(255,255,255,0.10);
    color: var(--t2);
    font: 600 9.5px/1 ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
    letter-spacing: 0.4px;
    text-transform: uppercase;
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,0.08),
      0 1px 2px rgba(0,0,0,0.10);
    align-self: center;
  }
  :global([data-theme="light"]) .po-version-pill {
    background: linear-gradient(180deg, #ffffff 0%, #f6f6f8 100%);
    border-color: rgba(0,0,0,0.08);
    color: var(--t2);
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,1),
      0 1px 2px rgba(0,0,0,0.04);
  }
  /* Welcome variant — sits between the wordmark and the tagline; tighter
     vertical rhythm so it reads as metadata, not a CTA. */
  .po-version-pill-welcome {
    margin-top: -2px;
  }

  /* ── What's New step ─────────────────────────────────────────
     Header is centered + tight; the changelog list scrolls inside
     a soft-bordered card so a long history doesn't blow the wizard
     window past its target height. */
  .po-step-whatsnew {
    align-items: stretch;
    gap: 14px;
  }
  .po-whatsnew-header {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    text-align: center;
  }
  .po-whatsnew-header .po-version-pill {
    margin-bottom: 2px;
  }

  .po-changelog-scroll {
    max-height: 280px;
    overflow-y: auto;
    padding-right: 2px;
    /* Hide native scrollbars on macOS-style scroll-on-hover; the content
       is short enough that the bar would feel intrusive. */
    scrollbar-width: thin;
  }
  .po-changelog-scroll::-webkit-scrollbar { width: 6px; }
  .po-changelog-scroll::-webkit-scrollbar-thumb {
    background: rgba(255,255,255,0.12);
    border-radius: 3px;
  }
  :global([data-theme="light"]) .po-changelog-scroll::-webkit-scrollbar-thumb {
    background: rgba(0,0,0,0.16);
  }

  .po-changelog {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .po-changelog-entry {
    background: linear-gradient(180deg,
      rgba(255,255,255,0.045) 0%,
      rgba(255,255,255,0.018) 100%);
    border: 1px solid rgba(255,255,255,0.075);
    border-radius: 12px;
    padding: 11px 12px 10px;
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,0.06),
      0 1px 2px rgba(0,0,0,0.14);
  }
  :global([data-theme="light"]) .po-changelog-entry {
    background: linear-gradient(180deg,
      rgba(255,255,255,1) 0%,
      rgba(248,248,250,1) 100%);
    border-color: rgba(0,0,0,0.06);
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,1),
      0 1px 2px rgba(0,0,0,0.03);
  }
  .po-changelog-row {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-bottom: 5px;
  }
  .po-changelog-version {
    font: 700 10.5px/1 ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
    color: var(--t1);
    letter-spacing: 0.2px;
  }
  /* Optional badge — for "Major rewrite", "Beta", etc. Uses an accent tint
     so the version still leads visually but the badge catches the eye. */
  .po-changelog-tag {
    font: 600 9px/1 'Inter', sans-serif;
    color: #1f8cff;
    background: rgba(31, 140, 255, 0.12);
    border: 1px solid rgba(31, 140, 255, 0.22);
    border-radius: 999px;
    padding: 2px 7px;
    letter-spacing: 0.2px;
  }
  :global([data-theme="light"]) .po-changelog-tag {
    /* On a near-white card the original blue washes out. Use a deeper
       hue + slightly stronger fills so the chip retains its iOS-tag
       readability without becoming saturated. */
    color: #0a72e8;
    background: rgba(31, 140, 255, 0.10);
    border-color: rgba(10, 114, 232, 0.30);
  }
  .po-changelog-date {
    font: 500 9.5px/1 'Inter', sans-serif;
    color: var(--t4);
    margin-left: auto;
  }
  .po-changelog-title {
    font: 600 12px/1.3 'Inter', sans-serif;
    color: var(--t1);
    margin-bottom: 8px;
    letter-spacing: -0.1px;
  }

  /* ── Highlights — plain bulleted list, no decorations ───────────────
     The previous version wrapped each item in a tinted card with a
     border. That added visual weight without information; with several
     highlights stacked, the page read as a row of buttons. The current
     style is a flat bullet list — small dot, bold title on its own
     line, body text on the next — so the eye runs down the column
     without bouncing between card frames. */
  .po-changelog-highlights {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 9px;
  }
  .po-highlight {
    position: relative;
    padding-left: 14px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  /* Bullet glyph: a small muted dot, sized to align with the title's
     cap-height so the row reads as a single visual line. Stays the same
     color across themes — it's an indicator, not a decoration. */
  .po-highlight::before {
    content: "";
    position: absolute;
    left: 4px;
    top: 7px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--t4);
  }
  .po-highlight-title {
    font: 600 11px/1.3 'Inter', sans-serif;
    color: var(--t1);
    letter-spacing: -0.06px;
  }
  .po-highlight-body {
    font: 400 10.5px/1.45 'Inter', sans-serif;
    color: var(--t3);
    letter-spacing: -0.02px;
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

  .po-title {
    font-size: 22px;
    font-weight: 700;
    line-height: 1.18;
    letter-spacing: -0.42px;
    color: var(--t1);
    margin: 0;
    text-align: center;
  }
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

  /* ── Step-2 progress pill ────────────────────────────────────
     Sits below the heading and above the lede. While at least one
     permission is still pending it reads neutral ("1 of 2 enabled");
     once both are granted it morphs to systemGreen ("2 of 2 ready").
     Single transition target so the moment of completion is
     unmistakable but quiet.

     Alignment note: the previous version mixed monospace (number) with
     sans (label) on `align-items: baseline` + `line-height: 1`, which
     looks misaligned because monospace baselines don't sit at the same
     y-coordinate as sans at the same nominal size. Center-aligning on
     a *unified* line-height resolves it — the height of every span is
     the same so they share a true vertical center. */
  .po-progress {
    align-self: center;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 10px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    color: var(--t3);
    margin: 2px 0 0;
    transition:
      background 280ms cubic-bezier(0.4, 0, 0.2, 1),
      border-color 280ms cubic-bezier(0.4, 0, 0.2, 1),
      color 280ms cubic-bezier(0.4, 0, 0.2, 1);
  }
  :global([data-theme="light"]) .po-progress {
    background: rgba(0, 0, 0, 0.04);
    border-color: rgba(0, 0, 0, 0.08);
  }
  .po-progress[data-state="all"] {
    background: rgba(52, 199, 89, 0.10);
    border-color: rgba(52, 199, 89, 0.22);
    color: #34c759;
  }
  :global([data-theme="light"]) .po-progress[data-state="all"] {
    background: rgba(36, 160, 86, 0.10);
    border-color: rgba(36, 160, 86, 0.30);
    color: #248058;
  }
  /* Pill content is a single Inter typeface throughout; only the weight
     and color shift between the numbers and the connector word. That
     gets us identical glyph metrics so the spans line up perfectly,
     and avoids the visual jolt of a monospace digit sitting next to a
     proportional letter. The numeric spans stay tabular-nums so the
     digit width doesn't reflow when the count ticks. */
  .po-progress-num,
  .po-progress-of {
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "SF Pro",
      "Inter", "Helvetica Neue", Helvetica, Arial, sans-serif;
    font-size: 11px;
    line-height: 1;
    letter-spacing: -0.05px;
  }
  .po-progress-num {
    font-weight: 600;
    color: var(--t1);
    transition: color 280ms ease;
    font-variant-numeric: tabular-nums;
  }
  .po-progress[data-state="all"] .po-progress-num {
    color: inherit;
  }
  .po-progress-of {
    font-weight: 400;
    color: var(--t3);
  }
  .po-progress[data-state="all"] .po-progress-of {
    color: inherit;
    opacity: 0.7;
  }

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
  /* When a card flips to granted we whisper the success on the card
     frame: a single iOS-systemGreen-tinted border at low alpha plus a
     barely-perceptible ambient halo of the same hue. No background
     repaint, no icon recolor — the surface stays identical so the
     un-granted card remains the visual lead. */
  .po-card[data-done="true"] {
    border-color: rgba(52, 199, 89, 0.20);
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,0.06),
      0 1px 2px rgba(0,0,0,0.18),
      0 0 0 1px rgba(52, 199, 89, 0.06);
    transition:
      border-color 320ms cubic-bezier(0.4, 0, 0.2, 1),
      box-shadow 320ms cubic-bezier(0.4, 0, 0.2, 1);
  }
  :global([data-theme="light"]) .po-card[data-done="true"] {
    border-color: rgba(36, 160, 86, 0.30);
    box-shadow:
      inset 0 0.5px 0 rgba(255,255,255,1),
      0 1px 2px rgba(0,0,0,0.04),
      0 0 0 1px rgba(36, 160, 86, 0.08);
  }

  .po-card-row { display: flex; align-items: flex-start; gap: 11px; }

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
  /* SF-symbol-style stroked check that springs in next to the card title
     when a permission flips to granted. iOS systemGreen (#34c759), no
     filled background, no glow — the reward is the spring curve, not
     surface area. */
  .po-card-check {
    color: #34c759;
    flex-shrink: 0;
    animation: poCheckSpring 380ms cubic-bezier(0.34, 1.4, 0.64, 1) both;
  }
  @keyframes poCheckSpring {
    0%   { opacity: 0; transform: scale(0.4) rotate(-10deg); }
    65%  { opacity: 1; transform: scale(1.12) rotate(2deg); }
    100% { opacity: 1; transform: scale(1) rotate(0); }
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
  :global([data-theme="light"]) .po-btn-loading {
    background: rgba(0,0,0,0.04);
  }
  .po-btn:disabled { cursor: default; }

  /* Replaces the action button once a permission is granted. Sized to
     match the button's vertical footprint so the card height doesn't
     reflow when state changes — the layout stays calm even though the
     content is rewarding. */
  .po-card-done-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    align-self: flex-start;
    padding: 6px 11px 6px 9px;
    border-radius: 999px;
    background: rgba(52, 199, 89, 0.10);
    border: 1px solid rgba(52, 199, 89, 0.20);
    color: #34c759;
    font: 600 11px/1 'Inter', sans-serif;
    letter-spacing: -0.05px;
    margin-top: 2px;
    /* Slight opacity-and-rise on appearance: the ledge between "loading"
       and "granted" should feel earned, not abrupt. */
    animation: poPillRise 360ms cubic-bezier(0.34, 1.4, 0.64, 1) both;
  }
  :global([data-theme="light"]) .po-card-done-pill {
    background: rgba(36, 160, 86, 0.10);
    border-color: rgba(36, 160, 86, 0.28);
    color: #248058;
  }
  @keyframes poPillRise {
    0%   { opacity: 0; transform: translateY(4px) scale(0.96); }
    100% { opacity: 1; transform: translateY(0) scale(1); }
  }

  .po-step-done {
    align-items: stretch;
    text-align: center;
    gap: 14px;
  }

  /* Single-element success affordance: a 40px circle with iOS-systemGreen
     stroke-only check. No tinted background flood, no ambient glow — the
     reward is the spring scale-in, not the surface area. Mirrors how
     Apple's "Setup Complete" sheets land a small SF Symbol rather than
     a hero illustration. */
  .po-success-mark {
    width: 40px; height: 40px;
    margin: 8px auto 0;
    border-radius: 50%;
    background: rgba(52, 199, 89, 0.12);
    color: #34c759;
    display: flex;
    align-items: center;
    justify-content: center;
    animation: poSuccessPop 420ms cubic-bezier(0.34, 1.4, 0.64, 1) both 60ms;
  }
  :global([data-theme="light"]) .po-success-mark {
    background: rgba(36, 160, 86, 0.12);
    color: #248058;
  }
  @keyframes poSuccessPop {
    0%   { opacity: 0; transform: scale(0.5); }
    65%  { opacity: 1; transform: scale(1.06); }
    100% { opacity: 1; transform: scale(1); }
  }

  /* Recap list — modeled on iOS Settings rows, not on web "tags". Name on
     the left, status on the right in secondary label color. The granted
     state colors the trailing label systemGreen but does not add chips,
     halos, or background fills. */
  .po-recap {
    list-style: none;
    margin: 6px 0 0;
    padding: 4px 0;
    background: rgba(255,255,255,0.025);
    border: 1px solid rgba(255,255,255,0.06);
    border-radius: 10px;
    display: flex;
    flex-direction: column;
    text-align: left;
  }
  :global([data-theme="light"]) .po-recap {
    background: rgba(0,0,0,0.025);
    border-color: rgba(0,0,0,0.06);
  }
  .po-recap li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 9px 14px;
    font: 500 11.5px/1.3 'Inter', sans-serif;
    color: var(--t1);
    border-bottom: 1px solid rgba(255,255,255,0.04);
  }
  :global([data-theme="light"]) .po-recap li {
    border-bottom-color: rgba(0,0,0,0.05);
  }
  .po-recap li:last-child { border-bottom: none; }

  .po-recap-name {
    color: var(--t1);
    letter-spacing: -0.05px;
  }
  .po-recap-status {
    font: 500 11px/1 'Inter', sans-serif;
    color: var(--t3);
    letter-spacing: -0.04px;
  }
  .po-recap-status-on {
    color: #34c759;
  }
  :global([data-theme="light"]) .po-recap-status-on {
    color: #248058;
  }

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
  .po-foot:not(.po-foot-centered) .po-nav-next {
    margin-left: auto;
  }
  .po-foot-centered .po-nav-next,
  .po-foot-centered .po-nav-finish {
    min-width: 180px;
  }
  .po-nav-next:hover:not(:disabled),
  .po-nav-finish:hover:not(:disabled) {
    filter: brightness(1.06);
    transform: translateY(-1px);
  }
  /* When step 2's two permissions are both granted, the Continue button
     gains a subtle saturation bump + slightly stronger shadow so the
     user's eye is drawn to the next move. We avoid a separate "ready"
     banner — the affordance the user is about to click is itself the
     thing that announces readiness. */
  .po-nav-next[data-ready="true"] {
    background: linear-gradient(180deg, #2a96ff 0%, #1382f5 100%);
    box-shadow:
      0 0.5px 0 rgba(255,255,255,0.34) inset,
      0 1px 2px rgba(0,0,0,0.22),
      0 4px 14px rgba(31, 140, 255, 0.22);
    animation: poReadyPulse 540ms cubic-bezier(0.34, 1.4, 0.64, 1) both;
  }
  @keyframes poReadyPulse {
    0%   { transform: scale(1); }
    55%  { transform: scale(1.025); }
    100% { transform: scale(1); }
  }
  .po-nav-finish:disabled { opacity: 0.6; cursor: default; }
</style>
