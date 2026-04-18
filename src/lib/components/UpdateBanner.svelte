<script lang="ts">
  import {
    updaterStore,
    installUpdate,
    skipVersion,
    dismissBanner,
  } from "../stores/updater.js";
  import { openUrl } from "@tauri-apps/plugin-opener";

  let installing = $state(false);
  let installError = $state<string | null>(null);

  const snapshot = $derived($updaterStore);
  const available = $derived(snapshot.available);
  const progress = $derived(snapshot.progress);
  const isSkipped = $derived(
    available ? snapshot.skippedVersions.includes(available.version) : false,
  );
  const visible = $derived(
    available != null && !snapshot.dismissedForSession && !isSkipped,
  );
  const manualInstall = $derived(snapshot.installMode === "manual");

  async function onUpdate() {
    if (manualInstall && available) {
      await openUrl(
        `https://github.com/Michael-OvO/TokenMonitor/releases/tag/v${available.version}`,
      );
      await dismissBanner();
      return;
    }
    installing = true;
    installError = null;
    try {
      await installUpdate();
    } catch (e) {
      installError = e instanceof Error ? e.message : String(e);
      installing = false;
    }
  }

  async function onSkip() {
    if (!available) return;
    await skipVersion(available.version);
  }

  async function onDismiss() {
    await dismissBanner();
  }
</script>

{#if visible && available}
  <div class="banner" role="status">
    <span class="dot" aria-hidden="true"></span>

    {#if installError}
      <span class="label">Update failed</span>
      <span class="error-msg" title={installError}>{installError}</span>
      <span class="spacer"></span>
      <button class="action primary" onclick={onUpdate}>Retry</button>
      <button class="icon" onclick={onDismiss} aria-label="Dismiss">×</button>
    {:else if installing}
      <span class="label">Installing v{available.version}</span>
      {#if progress?.percent != null}
        <span class="percent">{progress.percent.toFixed(0)}%</span>
      {:else}
        <span class="percent">…</span>
      {/if}
    {:else}
      <span class="label">Update v{available.version} available</span>
      <span class="spacer"></span>
      <button class="action primary" onclick={onUpdate}>
        {manualInstall ? "Download" : "Install"}
      </button>
      <button class="action" onclick={onSkip}>Skip</button>
      <button class="icon" onclick={onDismiss} aria-label="Dismiss">×</button>
    {/if}

    {#if installing && progress?.percent != null}
      <div class="progress" style="width: {progress.percent}%"></div>
    {/if}
  </div>
{/if}

<style>
  .banner {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    font: 500 11px/1 'Inter', sans-serif;
    background: var(--surface-2);
    border-bottom: 1px solid var(--border-subtle);
    color: var(--t1);
    animation: slide-in var(--t-normal) var(--ease-out);
  }

  @keyframes slide-in {
    from { opacity: 0; transform: translateY(-4px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #E8B06F;
    flex-shrink: 0;
  }

  .label {
    color: var(--t1);
    white-space: nowrap;
  }

  .error-msg {
    color: var(--t3);
    font-weight: 400;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 140px;
  }

  .percent {
    margin-left: auto;
    color: var(--t2);
    font-variant-numeric: tabular-nums;
    font-weight: 500;
  }

  .spacer {
    flex: 1;
  }

  .action {
    background: none;
    border: none;
    padding: 2px 6px;
    font: 500 11px/1 'Inter', sans-serif;
    color: var(--t2);
    cursor: pointer;
    border-radius: 3px;
    transition: color var(--t-fast) var(--ease-out),
                background var(--t-fast) var(--ease-out);
  }
  .action:hover { color: var(--t1); background: var(--surface-hover); }

  .action.primary {
    color: var(--accent);
    font-weight: 600;
  }
  .action.primary:hover {
    background: var(--accent-soft);
  }

  .icon {
    background: none;
    border: none;
    padding: 0;
    width: 18px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--t3);
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
    border-radius: 3px;
    transition: color var(--t-fast) var(--ease-out),
                background var(--t-fast) var(--ease-out);
  }
  .icon:hover { color: var(--t1); background: var(--surface-hover); }

  .progress {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 1.5px;
    background: var(--accent);
    transition: width var(--t-normal) var(--ease-out);
    pointer-events: none;
  }
</style>
