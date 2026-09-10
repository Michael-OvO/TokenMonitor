<script lang="ts">
  import SettingsDisclosure from "./SettingsDisclosure.svelte";
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { invoke } from "@tauri-apps/api/core";
  import { settings, updateSetting, type Settings as SettingsType } from "../stores/settings.js";
  import {
    clearUsageCache,
    fetchData,
    activeProvider,
    activePeriod,
    activeOffset,
  } from "../stores/usage.js";
  import { setRemoteDeviceIncludeFlag } from "../views/deviceStats.js";
  import { deviceDisplayNames, deviceIdentityKey, formatCost, formatTimeAgo } from "../utils/format.js";
  import { logger } from "../utils/logger.js";
  import ToggleSwitch from "./ToggleSwitch.svelte";
  import type {
    DeviceUsagePayload,
    SshHostInfo,
    SshHostStatus,
    SshSyncResult,
    SshTestResult as SshTestResultType,
  } from "../types/index.js";

  type ConfiguredSshHost = {
    alias: string;
    enabled: boolean;
    include_in_stats: boolean;
  };

  type AutoSyncDeviceRow = {
    alias: string;
    aliases: string[];
    identityKey: string;
    total_cost: number;
    include_in_stats: boolean;
    status: string;
    last_synced: string | null;
    error_message: string | null;
    has_usage: boolean;
  };

  let current = $derived($settings as SettingsType);

  let sshHosts = $state<SshHostInfo[]>([]);
  let sshConfiguredHosts = $state<ConfiguredSshHost[]>([]);
  let sshTestResults = $state<Record<string, SshTestResultType>>({});
  let sshTestingHost = $state<string | null>(null);
  let sshSyncing = $state(false);
  let sshSyncResult = $state<{ total: number; msg: string } | null>(null);
  let deviceUsage = $state<DeviceUsagePayload | null>(null);
  let deviceUsageLoading = $state(false);
  let deviceUsageError = $state<string | null>(null);
  let destroyed = false;
  let devicesExpanded = $state(false);

  function isSshHostActive(host: ConfiguredSshHost | undefined): boolean {
    return Boolean(host?.enabled && host.include_in_stats);
  }

  let activeSshHostCount = $derived(sshConfiguredHosts.filter(isSshHostActive).length);

  let autoSyncDevices = $derived.by<AutoSyncDeviceRow[]>(() => {
    const sshDeviceKeys = new Set([
      ...sshHosts.map((host) => deviceIdentityKey(host.alias)),
      ...sshConfiguredHosts.map((host) => deviceIdentityKey(host.alias)),
    ]);
    const byIdentity = new Map<string, AutoSyncDeviceRow>();

    function addOrMerge(row: AutoSyncDeviceRow) {
      if (sshDeviceKeys.has(row.identityKey)) return;
      const existing = byIdentity.get(row.identityKey);
      if (!existing) {
        byIdentity.set(row.identityKey, row);
        return;
      }

      const useRowAlias =
        row.has_usage &&
        (!existing.has_usage || row.total_cost > existing.total_cost);
      byIdentity.set(row.identityKey, {
        alias: useRowAlias ? row.alias : existing.alias,
        aliases: Array.from(new Set([...existing.aliases, ...row.aliases])),
        identityKey: row.identityKey,
        total_cost: Math.max(existing.total_cost, row.total_cost),
        include_in_stats: existing.include_in_stats && row.include_in_stats,
        status: useRowAlias ? row.status : existing.status,
        last_synced: useRowAlias ? row.last_synced : existing.last_synced,
        error_message: row.error_message ?? existing.error_message,
        has_usage: existing.has_usage || row.has_usage,
      });
    }

    for (const device of deviceUsage?.devices ?? []) {
      if (device.is_local) continue;
      addOrMerge({
        alias: device.device,
        aliases: [device.device],
        identityKey: deviceIdentityKey(device.device),
        total_cost: device.total_cost,
        include_in_stats: device.include_in_stats,
        status: device.status,
        last_synced: device.last_synced,
        error_message: device.error_message,
        has_usage: true,
      });
    }

    for (const saved of current.remoteDeviceIncludes) {
      addOrMerge({
        alias: saved.alias,
        aliases: [saved.alias],
        identityKey: deviceIdentityKey(saved.alias),
        total_cost: 0,
        include_in_stats: saved.include_in_stats,
        status: "offline",
        last_synced: null,
        error_message: null,
        has_usage: false,
      });
    }

    return [...byIdentity.values()].sort((a, b) => {
      if (a.has_usage !== b.has_usage) return a.has_usage ? -1 : 1;
      if (b.total_cost !== a.total_cost) return b.total_cost - a.total_cost;
      return a.alias.localeCompare(b.alias, undefined, { sensitivity: "base" });
    });
  });
  let sshHostNames = $derived(deviceDisplayNames(sshHosts.map((h) => h.alias)));
  let autoSyncDeviceNames = $derived(deviceDisplayNames(autoSyncDevices.map((d) => d.alias)));
  let activeAutoSyncDeviceCount = $derived(autoSyncDevices.filter((d) => d.include_in_stats).length);
  let totalRemoteDeviceCount = $derived(sshHosts.length + autoSyncDevices.length);
  let activeRemoteDeviceCount = $derived(activeSshHostCount + activeAutoSyncDeviceCount);

  function configuredHostsFromSettings(): ConfiguredSshHost[] {
    return current.sshHosts.map((h) => ({
      alias: h.alias,
      enabled: h.enabled,
      include_in_stats: h.include_in_stats ?? true,
    }));
  }

  onMount(() => {
    destroyed = false;
    sshConfiguredHosts = configuredHostsFromSettings();

    invoke<SshHostInfo[]>("get_ssh_hosts")
      .then((hosts) => {
        sshHosts = [...hosts].sort((a, b) =>
          a.alias.localeCompare(b.alias, undefined, { sensitivity: "base" }),
        );
      })
      .catch((e) => { logger.warn("ssh", `Failed to load SSH hosts: ${e}`); });

    invoke<SshHostStatus[]>("get_ssh_host_statuses")
      .then((statuses) => {
        const savedHosts = get(settings).sshHosts;
        sshConfiguredHosts = statuses.map((s) => ({
          alias: s.alias,
          enabled: s.enabled,
          include_in_stats:
            sshConfiguredHosts.find((host) => host.alias === s.alias)?.include_in_stats ??
            savedHosts.find((host) => host.alias === s.alias)?.include_in_stats ??
            true,
        }));
      })
      .catch((e) => { logger.warn("ssh", `Failed to load SSH host statuses: ${e}`); });

    fetchRemoteDeviceData();

    return () => {
      destroyed = true;
    };
  });

  async function fetchRemoteDeviceData() {
    deviceUsageLoading = true;
    deviceUsageError = null;
    try {
      deviceUsage = await invoke<DeviceUsagePayload>("get_device_usage", {
        provider: "all",
        period: "year",
        offset: 0,
      });
    } catch (e) {
      deviceUsage = null;
      deviceUsageError = String(e);
    } finally {
      deviceUsageLoading = false;
    }
  }

  async function refreshActiveUsage() {
    clearUsageCache();
    await fetchData(get(activeProvider), get(activePeriod), get(activeOffset));
    await fetchRemoteDeviceData();
  }

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

  async function persistSshHosts(hosts: ConfiguredSshHost[]) {
    await updateSetting(
      "sshHosts",
      hosts.map((h) => ({
        alias: h.alias,
        enabled: h.enabled,
        include_in_stats: h.include_in_stats,
      })),
    );
  }

  async function toggleSshHost(alias: string, active: boolean) {
    logger.info("ssh", `Toggle: ${alias} active=${active}`);
    try {
      let nextHosts: ConfiguredSshHost[];
      if (!sshConfiguredHosts.some((h) => h.alias === alias)) {
        if (!active) return;
        await invoke("add_ssh_host", { alias });
        nextHosts = [...sshConfiguredHosts, { alias, enabled: true, include_in_stats: true }];
      } else {
        await invoke("toggle_ssh_host", { alias, enabled: active });
        await invoke("toggle_device_include_in_stats", { alias, includeInStats: active });
        nextHosts = sshConfiguredHosts.map((h) =>
          h.alias === alias ? { ...h, enabled: active, include_in_stats: active } : h,
        );
      }
      sshConfiguredHosts = nextHosts;
      await persistSshHosts(nextHosts);
      await refreshActiveUsage();
    } catch (e) {
      console.error("Failed to toggle SSH host:", e);
    }
  }

  async function toggleRemoteDeviceInclude(device: AutoSyncDeviceRow, includeInStats: boolean) {
    logger.info("device", `Toggle: ${device.alias} include=${includeInStats}`);
    const previousRemoteDevices = get(settings).remoteDeviceIncludes;
    const updatedAliases: string[] = [];

    try {
      for (const alias of device.aliases) {
        await invoke("toggle_device_include_in_stats", { alias, includeInStats });
        updatedAliases.push(alias);
      }
      const nextRemoteDevices = device.aliases.reduce(
        (devices, alias) => setRemoteDeviceIncludeFlag(devices, alias, includeInStats),
        get(settings).remoteDeviceIncludes,
      );
      await updateSetting("remoteDeviceIncludes", nextRemoteDevices);

      await refreshActiveUsage();
    } catch (e) {
      console.error("Failed to toggle remote device:", e);
      await updateSetting("remoteDeviceIncludes", previousRemoteDevices).catch(() => {});
      for (const alias of updatedAliases) {
        const previousInclude =
          previousRemoteDevices.find((d) => d.alias === alias)?.include_in_stats ?? true;
        await invoke("toggle_device_include_in_stats", {
          alias,
          includeInStats: previousInclude,
        }).catch(() => {});
      }
    }
  }

  async function syncAllRemoteDevices() {
    logger.info("device", "Sync all started");
    sshSyncing = true;
    sshSyncResult = null;
    const startTime = performance.now();
    const enabledHosts = sshConfiguredHosts.filter(isSshHostActive);
    let totalRecords = 0;
    const failedHosts: string[] = [];
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
        sshSyncResult = { total: totalRecords, msg: `${connectedCount} of ${enabledHosts.length} SSH hosts connected` };
      } catch (e) {
        if (destroyed) return;
        failedHosts.push(host.alias);
        console.error(`Sync failed for ${host.alias}:`, e);
      }
    }

    let remoteSyncFailed = false;
    try {
      await invoke("sync_remote_devices");
    } catch (e) {
      remoteSyncFailed = true;
      console.error("Remote device sync failed:", e);
    }

    if (destroyed) return;
    sshSyncing = false;
    await refreshActiveUsage();
    const elapsed = ((performance.now() - startTime) / 1000).toFixed(1);
    if (failedHosts.length > 0 || remoteSyncFailed) {
      if (remoteSyncFailed) failedHosts.push("Auto Sync");
      sshSyncResult = { total: totalRecords, msg: `Failed: ${failedHosts.join(", ")} (${elapsed}s)` };
    } else {
      sshSyncResult = { total: totalRecords, msg: `Finished syncing in ${elapsed}s` };
    }
    logger.info("ssh", `Sync done: ${totalRecords} records, ${failedHosts.length} failures`);
    setTimeout(() => { if (!destroyed) sshSyncResult = null; }, 4000);
  }

  function autoSyncDetail(device: AutoSyncDeviceRow): string {
    if (device.error_message) return device.error_message;
    if (!device.has_usage) return "Saved include preference";

    const parts = [formatCost(device.total_cost)];
    if (device.last_synced) {
      parts.push(`Synced ${formatTimeAgo(device.last_synced)}`);
    } else {
      parts.push(device.status);
    }
    return parts.join(" - ");
  }
</script>

<div class="block">
  <button class="row collapsible-toggle" type="button" aria-expanded={devicesExpanded} onclick={() => (devicesExpanded = !devicesExpanded)}>
    <span class="label">Remote Devices</span>
    <div class="collapsible-right">
      {#if !devicesExpanded && totalRemoteDeviceCount > 0}
        <span
          role="button"
          tabindex="0"
          class="ssh-btn sync-collapsed"
          aria-disabled={sshSyncing}
          onclick={(e) => {
            e.stopPropagation();
            if (!sshSyncing) syncAllRemoteDevices();
          }}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.stopPropagation();
              e.preventDefault();
              if (!sshSyncing) syncAllRemoteDevices();
            }
          }}
        >
          {sshSyncing ? "Syncing..." : "Sync All"}
        </span>
      {/if}
      <span class="count">
        {activeRemoteDeviceCount} of {totalRemoteDeviceCount}
      </span>
      <svg class="collapsible-chevron" class:open={devicesExpanded} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="6 9 12 15 18 9"></polyline>
      </svg>
    </div>
  </button>
  <SettingsDisclosure open={devicesExpanded}>
    <div class="remote-section">
      <div class="section-heading">
        <span class="section-title">SSH Remote Host</span>
        <span class="section-count">{activeSshHostCount} of {sshHosts.length}</span>
      </div>
      <div class="ssh-hosts">
        {#each sshHosts as host (host.alias)}
          {@const configured = sshConfiguredHosts.find((h) => h.alias === host.alias)}
          <div class="ssh-host-row">
            <div class="ssh-host-info">
              <span class="ssh-alias">{sshHostNames.get(host.alias) ?? host.alias}</span>
              <span class="ssh-detail">{host.hostname}{host.user ? ` (${host.user})` : ""}{host.port !== 22 ? `:${host.port}` : ""}</span>
            </div>
            <div class="ssh-host-actions">
              {#if sshTestingHost === host.alias}
                <span class="ssh-testing">...</span>
              {:else if sshTestResults[host.alias]}
                <span class="ssh-result" class:ssh-ok={sshTestResults[host.alias].success} class:ssh-fail={!sshTestResults[host.alias].success}>
                  {sshTestResults[host.alias].success ? "OK" : "Fail"}
                </span>
              {/if}
              <button class="ssh-btn" type="button" onclick={() => testSshHost(host.alias)}>Test</button>
              <div class="mini-toggle">
                <ToggleSwitch
                  checked={isSshHostActive(configured)}
                  onChange={(checked) => toggleSshHost(host.alias, checked)}
                />
              </div>
            </div>
          </div>
        {/each}
        {#if sshHosts.length === 0}
          <div class="ssh-empty">No hosts found in ~/.ssh/config</div>
        {/if}
      </div>
    </div>

    <div class="remote-section auto-section">
      <div class="section-heading">
        <span class="section-title">Remote Devices</span>
        <span class="section-count">{activeAutoSyncDeviceCount} of {autoSyncDevices.length}</span>
      </div>
      {#if deviceUsageLoading}
        <div class="ssh-empty">Loading devices...</div>
      {:else if deviceUsageError}
        <div class="ssh-empty error-text">{deviceUsageError}</div>
      {:else if autoSyncDevices.length > 0}
        <div class="auto-devices">
          {#each autoSyncDevices as device (device.alias)}
            <div class="auto-device-row">
              <div class="ssh-host-info">
                <span class="ssh-alias">{autoSyncDeviceNames.get(device.alias) ?? device.alias}</span>
                <span class="ssh-detail">{autoSyncDetail(device)}</span>
              </div>
              <div class="mini-toggle">
                <ToggleSwitch
                  checked={device.include_in_stats}
                  onChange={(checked) => toggleRemoteDeviceInclude(device, checked)}
                />
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="ssh-empty">No Auto Sync devices found</div>
      {/if}
    </div>

    <div class="ssh-sync-row">
      <span class="ssh-sync-label">
        {#if sshSyncResult}
          <span class="ssh-sync-status" class:ssh-sync-error={sshSyncResult.msg.startsWith("Failed")}>{sshSyncResult.msg}</span>
        {:else}
          {activeRemoteDeviceCount} device(s) enabled
        {/if}
      </span>
      <button class="ssh-btn" type="button" onclick={syncAllRemoteDevices} disabled={sshSyncing}>
        {sshSyncing ? "Syncing..." : "Sync All"}
      </button>
    </div>
  </SettingsDisclosure>
</div>

<style>
  .block {
    border-top: 1px solid var(--border-subtle);
  }
  .remote-section {
    padding: 0;
    border-top: 1px solid var(--border-subtle);
  }
  .auto-section {
    border-top-color: var(--border);
  }
  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px 3px;
  }
  .section-title {
    font: 600 8px/1 'Inter', sans-serif;
    color: var(--t3);
    text-transform: uppercase;
    letter-spacing: 0;
  }
  .section-count {
    font: 400 8px/1 'Inter', sans-serif;
    color: var(--t4);
  }
  .ssh-host-row,
  .auto-device-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 3px 10px;
    min-height: 25px;
  }
  .ssh-host-row + .ssh-host-row,
  .auto-device-row + .auto-device-row {
    border-top: 1px solid var(--border);
  }
  .ssh-host-info {
    display: flex;
    flex-direction: column;
    gap: 0;
    min-width: 0;
    flex: 1;
  }
  .ssh-alias {
    font: 500 9px/1.1 'Inter', sans-serif;
    color: var(--t1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ssh-detail {
    font: 400 7.5px/1.1 'Inter', sans-serif;
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
  .mini-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    font: 400 7.5px/1 'Inter', sans-serif;
    color: var(--t4);
    white-space: nowrap;
  }
  .ssh-btn {
    background: var(--surface-hover);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 8px;
    font: 400 8px/1.2 'Inter', sans-serif;
    color: var(--t2);
    cursor: pointer;
    white-space: nowrap;
  }
  .ssh-btn:hover:not(:disabled) {
    color: var(--t1);
    border-color: var(--t3);
  }
  .ssh-btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .sync-collapsed[aria-disabled="true"] {
    opacity: 0.55;
    cursor: default;
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
  .error-text {
    color: #ef4444;
  }
  .ssh-sync-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 10px;
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
    transform: rotate(-90deg);
  }
  .collapsible-chevron.open {
    transform: rotate(0deg);
  }
  .count {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
    white-space: nowrap;
  }
</style>
