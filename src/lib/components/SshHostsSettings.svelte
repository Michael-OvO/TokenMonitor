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
  let devicesExpanded = $state(false);

  onMount(() => {
    destroyed = false;
    sshConfiguredHosts = current.sshHosts.map((h) => ({
      alias: h.alias,
      enabled: h.enabled,
      include_in_stats: h.include_in_stats ?? false,
    }));

    invoke<SshHostInfo[]>("get_ssh_hosts")
      .then((hosts) => {
        sshHosts = [...hosts].sort((a, b) => 
          a.alias.localeCompare(b.alias, undefined, { sensitivity: "base" })
        );
      })
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

  async function toggleSshHost(alias: string, enabled: boolean) {
    logger.info("ssh", `Toggle: ${alias} enabled=${enabled}`);
    try {
      if (!sshConfiguredHosts.some(h => h.alias === alias)) {
        await invoke("add_ssh_host", { alias });
        sshConfiguredHosts = [...sshConfiguredHosts, { alias, enabled, include_in_stats: false }];
      } else {
        await invoke("toggle_ssh_host", { alias, enabled });
        sshConfiguredHosts = sshConfiguredHosts.map((h) =>
          h.alias === alias ? { ...h, enabled } : h,
        );
      }
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
    const enabledHosts = sshConfiguredHosts.filter((h) => h.enabled);
    let totalRecords = 0;
    let failedHosts: string[] = [];
    let connectedCount = 0;

    for (const host of enabledHosts) {
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
          connectedCount++;
          totalRecords += result.recordsSynced;
        }
        sshSyncResult = { total: totalRecords, msg: `${connectedCount} of ${enabledHosts.length} servers connected` };
      } catch (e) {
        if (destroyed) return;
        failedHosts.push(host.alias);
        console.error(`Sync failed for ${host.alias}:`, e);
      }
    }
    if (destroyed) return;
    sshSyncing = false;
    const elapsed = ((performance.now() - startTime) / 1000).toFixed(1);
    if (failedHosts.length > 0) {
      sshSyncResult = { total: totalRecords, msg: `Failed: ${failedHosts.join(", ")} (${elapsed}s)` };
    } else {
      sshSyncResult = { total: totalRecords, msg: `Finished syncing in ${elapsed}s` };
    }
    logger.info("ssh", `Sync done: ${totalRecords} records, ${failedHosts.length} failures`);
    setTimeout(() => { if (!destroyed) sshSyncResult = null; }, 4000);
  }
</script>

<div class="block">
  <button class="row collapsible-toggle" type="button" onclick={() => (devicesExpanded = !devicesExpanded)}>
    <span class="label">Remote Devices</span>
    <div class="collapsible-right">
      {#if !devicesExpanded && sshConfiguredHosts.filter(h => h.enabled).length > 0}
        <span role="button" tabindex="0" class="ssh-btn sync-collapsed" onclick={(e) => { e.stopPropagation(); syncAllSshHosts(); }} onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.stopPropagation(); e.preventDefault(); syncAllSshHosts(); }}} aria-disabled={sshSyncing}>
          {sshSyncing ? "Syncing..." : "Sync All"}
        </span>
      {/if}
      <span class="count">{sshConfiguredHosts.filter(h => h.enabled).length} of {sshHosts.length}</span>
      <svg class="collapsible-chevron" class:open={devicesExpanded} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="6 9 12 15 18 9"></polyline>
      </svg>
    </div>
  </button>
  <div class="devices-collapse" class:open={devicesExpanded}>
    <div class="collapse-inner">
      <div class="ssh-section">
      <div class="ssh-hosts">
        {#each sshHosts as host (host.alias)}
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
              <ToggleSwitch
                checked={sshConfiguredHosts.find(h => h.alias === host.alias)?.enabled ?? false}
                onChange={(checked) => toggleSshHost(host.alias, checked)}
              />
            </div>
          </div>
        {/each}
        {#if sshHosts.length === 0}
          <div class="ssh-empty">No hosts found in ~/.ssh/config</div>
        {/if}
      </div>
      {#if sshConfiguredHosts.length > 0}
        <div class="ssh-sync-row">
          <span class="ssh-sync-label">
            {#if sshSyncResult}
              <span class="ssh-sync-status" class:ssh-sync-error={sshSyncResult.msg.startsWith("Failed")}>{sshSyncResult.msg}</span>
            {:else}
              {sshConfiguredHosts.filter(h => h.enabled).length} device(s) enabled
            {/if}
          </span>
          <button class="ssh-btn" onclick={syncAllSshHosts} disabled={sshSyncing}>
            {sshSyncing ? "Syncing..." : "Sync Now"}
          </button>
        </div>
      {/if}
    </div>
  </div>
  </div>
</div>

<style>
  .block {
    border-top: 1px solid var(--border-subtle);
  }
  .ssh-section {
    padding: 6px 0;
    border-top: 1px solid var(--border-subtle);
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
  .ssh-btn:hover {
    color: var(--t1);
    border-color: var(--t3);
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
  .ssh-sync-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px;
    border-top: 1px solid var(--border);
  }
  .ssh-sync-label {
    font: 400 8px/1 'Inter', sans-serif;
    color: var(--t3);
  }
  .ssh-sync-status {
    color: var(--accent, #4caf50);
  }
  .ssh-sync-error {
    color: #f44336;
  }
  .collapsible-toggle {
    width: 100%;
    background: none;
    border: none;
    cursor: pointer;
    user-select: none;
    padding: 7px 10px;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .collapsible-toggle:hover {
    background: var(--surface-hover);
  }
  .label {
    font: 400 10px/1 'Inter', sans-serif;
    color: var(--t1);
  }
  .collapsible-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .collapsible-chevron {
    color: var(--t3);
    transition: transform var(--t-normal, 200ms) ease;
    transform: rotate(-90deg);
  }
  .collapsible-chevron.open {
    transform: rotate(0deg);
  }
  .count {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
  }
  .devices-collapse {
    display: grid;
    grid-template-rows: 0fr;
    transition: grid-template-rows var(--t-normal, 200ms) ease;
  }
  .devices-collapse.open {
    grid-template-rows: 1fr;
  }
  .collapse-inner {
    overflow: hidden;
  }
</style>
