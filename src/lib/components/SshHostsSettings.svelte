<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { settings, updateSetting, type Settings as SettingsType } from "../stores/settings.js";
  import { clearUsageCache } from "../stores/usage.js";
  import { logger } from "../utils/logger.js";
  import ToggleSwitch from "./ToggleSwitch.svelte";
  import type { SshHostInfo, SshSyncResult, SshTestResult as SshTestResultType } from "../types/index.js";

  let current = $derived($settings as SettingsType);

  let sshHosts = $state<SshHostInfo[]>([]);
  let sshConfiguredHosts = $state<{ alias: string; enabled: boolean; include_in_stats: boolean }[]>([]);
  let sshTestResults = $state<Record<string, SshTestResultType>>({});
  let sshTestingHost = $state<string | null>(null);
  let sshSyncing = $state(false);
  let sshSyncResult = $state<{ total: number; msg: string } | null>(null);
  let destroyed = false;
  let expanded = $state(false);
  let enabledCount = $derived(
    sshConfiguredHosts.filter(
      (h) => h.enabled && sshHosts.some((s) => s.alias === h.alias),
    ).length,
  );

  let sortedSshHosts = $derived(
    [...sshHosts].sort((a, b) => a.alias.localeCompare(b.alias, undefined, { sensitivity: "base" })),
  );

  onMount(() => {
    destroyed = false;
    sshConfiguredHosts = current.sshHosts.map((h) => ({
      alias: h.alias,
      enabled: h.enabled,
      include_in_stats: h.include_in_stats ?? false,
    }));

    invoke<SshHostInfo[]>("get_ssh_hosts")
      .then((hosts) => { sshHosts = hosts; })
      .catch((e) => { logger.warn("ssh", `Failed to load SSH hosts: ${e}`); });
    invoke<{ alias: string; enabled: boolean }[]>("get_ssh_host_statuses")
      .then((statuses) => {
        sshConfiguredHosts = statuses.map((s) => ({
          alias: s.alias,
          enabled: s.enabled,
          include_in_stats:
            sshConfiguredHosts.find((host) => host.alias === s.alias)?.include_in_stats ??
            current.sshHosts.find((host) => host.alias === s.alias)?.include_in_stats ??
            false,
        }));
      })
      .catch((e) => { logger.warn("ssh", `Failed to load SSH host statuses: ${e}`); });

    return () => {
      destroyed = true;
    };
  });

  async function testSshHost(alias: string) {
    logger.info("ssh", `Testing: ${alias}`);
    sshTestingHost = alias;
    try {
      const result = await invoke<SshTestResultType>("test_ssh_connection", { alias });
      sshTestResults = { ...sshTestResults, [alias]: result };
    } catch (e) {
      sshTestResults = { ...sshTestResults, [alias]: { success: false, message: String(e), durationMs: 0 } };
    }
    sshTestingHost = null;
  }

  function persistSshHosts(hosts: { alias: string; enabled: boolean; include_in_stats: boolean }[]) {
    updateSetting(
      "sshHosts",
      hosts.map((h) => ({
        alias: h.alias,
        enabled: h.enabled,
        include_in_stats: h.include_in_stats,
      })),
    );
  }

  async function addSshHost(alias: string) {
    logger.info("ssh", `Adding: ${alias}`);
    try {
      await invoke("add_ssh_host", { alias });
      sshConfiguredHosts = [...sshConfiguredHosts, { alias, enabled: true, include_in_stats: false }];
      persistSshHosts(sshConfiguredHosts);
      clearUsageCache();
    } catch (e) {
      console.error("Failed to add SSH host:", e);
    }
  }

  async function toggleSshHost(alias: string, enabled: boolean) {
    logger.info("ssh", `Toggle: ${alias} enabled=${enabled}`);
    try {
      await invoke("toggle_ssh_host", { alias, enabled });
      sshConfiguredHosts = sshConfiguredHosts.map((h) =>
        h.alias === alias ? { ...h, enabled } : h,
      );
      persistSshHosts(sshConfiguredHosts);
      clearUsageCache();
    } catch (e) {
      console.error("Failed to toggle SSH host:", e);
    }
  }

  async function syncAllSshHosts() {
    logger.info("ssh", "Sync all started");
    sshSyncing = true;
    sshSyncResult = null;
    const startTime = performance.now();
    let totalRecords = 0;
    let failedHosts: string[] = [];
    for (const host of sshConfiguredHosts.filter((h) => h.enabled)) {
      if (destroyed) return;
      try {
        const result = await invoke<SshSyncResult>("sync_ssh_host", { alias: host.alias });
        if (destroyed) return;
        sshTestResults = {
          ...sshTestResults,
          [host.alias]: {
            success: result.testSuccess,
            message: result.testMessage,
            durationMs: result.testDurationMs,
          },
        };
        if (!result.testSuccess) {
          failedHosts.push(host.alias);
        } else {
          totalRecords += result.recordsSynced;
        }
      } catch (e) {
        if (destroyed) return;
        failedHosts.push(host.alias);
        console.error(`Sync failed for ${host.alias}:`, e);
      }
    }
    if (destroyed) return;
    if (totalRecords > 0) {
      clearUsageCache();
    }
    sshSyncing = false;
    const elapsed = ((performance.now() - startTime) / 1000).toFixed(1);
    if (failedHosts.length > 0) {
      sshSyncResult = { total: totalRecords, msg: `Failed: ${failedHosts.join(", ")} (${elapsed}s)` };
    } else {
      const detail = totalRecords > 0 ? `Synced ${totalRecords} records` : "Already up to date";
      sshSyncResult = { total: totalRecords, msg: `${detail} in ${elapsed}s` };
    }
    logger.info("ssh", `Sync done: ${totalRecords} records, ${failedHosts.length} failures`);
    setTimeout(() => { if (!destroyed) sshSyncResult = null; }, 4000);
  }
</script>

<div class="card">
  <div class="row ssh-toggle-row" role="button" tabindex="0" onclick={() => (expanded = !expanded)} onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); expanded = !expanded; } }}>
    <span class="label">SSH Hosts</span>
    <div class="ssh-toggle-right">
      {#if sshSyncResult}
        <span class="ssh-sync-status" class:ssh-sync-error={sshSyncResult.msg.startsWith("Failed")}>{sshSyncResult.msg}</span>
      {:else if sshHosts.length > 0}
        <span class="ssh-toggle-count">{enabledCount} of {sshHosts.length} enabled</span>
      {/if}
      <button
        class="ssh-btn ssh-sync-btn"
        class:spinning={sshSyncing}
        type="button"
        aria-label="Sync SSH hosts"
        onclick={(e) => { e.stopPropagation(); syncAllSshHosts(); }}
        disabled={sshSyncing || sshConfiguredHosts.filter((h) => h.enabled).length === 0}
      >
        <svg class="sync-icon" width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="23 4 23 10 17 10"></polyline>
          <polyline points="1 20 1 14 7 14"></polyline>
          <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
        </svg>
        <span class="sync-label">Sync</span>
      </button>
      <svg class="ssh-chevron" class:open={expanded} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="6 9 12 15 18 9"></polyline>
      </svg>
    </div>
  </div>
  <div class="ssh-collapse" class:open={expanded}>
  <div class="ssh-section">
    <div class="ssh-hosts">
      {#each sortedSshHosts as host (host.alias)}
        <div class="ssh-host-row">
          <div class="ssh-host-info">
            <span class="ssh-alias">{host.alias}</span>
            <span class="ssh-detail">{host.hostname}{host.user ? ` (${host.user})` : ''}{host.port !== 22 ? `:${host.port}` : ''}</span>
          </div>
          <div class="ssh-host-actions">
            {#if sshTestingHost === host.alias}
              <span class="ssh-testing">...</span>
            {:else if sshTestResults[host.alias]}
              <span class="ssh-result" class:ssh-ok={sshTestResults[host.alias].success} class:ssh-fail={!sshTestResults[host.alias].success}>
                {sshTestResults[host.alias].success ? 'OK' : 'Fail'}
              </span>
            {/if}
            <button class="ssh-btn" onclick={() => testSshHost(host.alias)}>Test</button>
            {#if sshConfiguredHosts.some(h => h.alias === host.alias)}
              <ToggleSwitch
                checked={sshConfiguredHosts.find(h => h.alias === host.alias)?.enabled ?? false}
                onChange={(checked) => toggleSshHost(host.alias, checked)}
              />
            {:else}
              <ToggleSwitch checked={false} onChange={() => addSshHost(host.alias)} />
            {/if}
          </div>
        </div>
      {/each}
      {#if sshHosts.length === 0}
        <div class="ssh-empty">No hosts found in ~/.ssh/config</div>
      {/if}
    </div>
  </div>
  </div>
</div>

<style>
  .card {
    background: var(--surface-2);
    border-radius: 8px;
    overflow: hidden;
  }
  .ssh-section {
    padding: 6px 0;
  }
  .ssh-host-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 5px 10px;
    min-height: 28px;
  }
  .ssh-host-row + .ssh-host-row {
    border-top: 1px solid var(--border);
  }
  .ssh-host-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .ssh-alias {
    font: 500 9px/1.2 'Inter', sans-serif;
    color: var(--t1);
  }
  .ssh-detail {
    font: 400 7.5px/1.2 'Inter', sans-serif;
    color: var(--t4);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ssh-host-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }
  .ssh-btn {
    background: var(--surface-hover);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 8px;
    font: 400 8px/1.2 'Inter', sans-serif;
    color: var(--t2);
    cursor: pointer;
  }
  .ssh-btn:hover:not(:disabled) {
    color: var(--t1);
    border-color: var(--t3);
  }
  .ssh-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .row {
    padding: 7px 10px;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .label {
    font: 400 10px/1 'Inter', sans-serif;
    color: var(--t1);
  }
  .ssh-toggle-row {
    width: 100%;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border-subtle);
    cursor: pointer;
    user-select: none;
  }
  .ssh-toggle-row:hover {
    background: var(--surface-hover);
  }
  .ssh-toggle-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .ssh-toggle-count {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
  }
  .ssh-chevron {
    color: var(--t3);
    transition: transform var(--t-normal) ease;
    transform: rotate(-90deg);
  }
  .ssh-chevron.open {
    transform: rotate(0deg);
  }
  .ssh-collapse {
    max-height: 0;
    overflow: hidden;
    transition: max-height var(--t-normal) ease;
  }
  .ssh-collapse.open {
    max-height: 500px;
  }
  .ssh-testing {
    font: 400 8px/1 'Inter', sans-serif;
    color: var(--t4);
  }
  .ssh-result {
    font: 500 8px/1 'Inter', sans-serif;
  }
  .ssh-ok { color: #22c55e; }
  .ssh-fail { color: #ef4444; }
  .ssh-empty {
    padding: 10px;
    font: 400 9px/1.4 'Inter', sans-serif;
    color: var(--t3);
  }
  .ssh-sync-btn {
    display: inline-flex;
    align-items: center;
    padding: 2px 5px;
  }
  .sync-label {
    font: 400 8px/1 'Inter', sans-serif;
    margin-left: 2px;
  }
  .ssh-sync-btn.spinning .sync-icon {
    animation: refresh-spin 900ms linear infinite;
    transform-origin: center;
  }
  .ssh-sync-status {
    font: 400 8px/1 'Inter', sans-serif;
    color: var(--accent, #4caf50);
  }
  .ssh-sync-error {
    color: #f44336;
  }
  @keyframes refresh-spin {
    to { transform: rotate(360deg); }
  }
</style>
