<script lang="ts">
  import SettingsDisclosure from "./SettingsDisclosure.svelte";
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { invoke } from "@tauri-apps/api/core";
  import { settings, updateSetting, type Settings as SettingsType } from "../../stores/settings.js";
  import {
    clearUsageCache,
    fetchData,
    activeProvider,
    activePeriod,
    activeOffset,
  } from "../../stores/usage.js";
  import { setRemoteDeviceIncludeFlag } from "../../views/deviceStats.js";
  import { deviceDisplayNames, deviceIdentityKey, formatCost, formatTimeAgo } from "../../utils/format.js";
  import { logger } from "../../utils/logger.js";
  import ToggleSwitch from "../ToggleSwitch.svelte";
  import type {
    DeviceUsagePayload,
    SshHostInfo,
    SshHostStatus,
    SshSyncResult,
    SshTestResult as SshTestResultType,
  } from "../../types/index.js";

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

  let sshTestingAll = $state(false);

  // ponytail: tests hosts one at a time, reusing testSshHost; parallelize if host lists get long
  async function testAllSshHosts() {
    sshTestingAll = true;
    for (const host of sshHosts) await testSshHost(host.alias);
    sshTestingAll = false;
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
  <div class="devices-header">
    <button class="row collapsible-toggle" type="button" aria-expanded={devicesExpanded} onclick={() => (devicesExpanded = !devicesExpanded)}>
      <span class="label">Remote Devices</span>
      <span class="collapsible-right">
        <span class="count">{activeRemoteDeviceCount} of {totalRemoteDeviceCount}</span>
        <svg class="collapsible-chevron" class:open={devicesExpanded} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="6 9 12 15 18 9"></polyline>
        </svg>
      </span>
    </button>
    {#if !devicesExpanded && totalRemoteDeviceCount > 0}
      <button class="ssh-btn sync-collapsed" type="button" onclick={syncAllRemoteDevices} disabled={sshSyncing || sshTestingAll || sshTestingHost !== null}>
        {sshSyncing ? "Syncing…" : "Sync All"}
      </button>
    {/if}
  </div>
  <SettingsDisclosure open={devicesExpanded}>
    <div class="remote-content">
      <section class="remote-section" aria-label="SSH hosts">
        <div class="section-heading">
          <span class="section-title">SSH Hosts</span>
          <span class="section-count">{activeSshHostCount} of {sshHosts.length}</span>
        </div>
        <p class="section-description">Sync selected hosts and include their usage.</p>
        <div class="ssh-hosts">
          {#each sshHosts as host (host.alias)}
            {@const configured = sshConfiguredHosts.find((h) => h.alias === host.alias)}
            {@const name = sshHostNames.get(host.alias) ?? host.alias}
            {@const result = sshTestResults[host.alias]}
            <div class="ssh-host-row">
              <div class="device-heading">
                <div class="ssh-host-info">
                  <span class="ssh-alias" title={host.alias}>{name}</span>
                  <span class="ssh-detail">{host.user ? `${host.user}@` : ""}{host.hostname}{host.port !== 22 ? `:${host.port}` : ""}</span>
                </div>
                <ToggleSwitch
                  checked={isSshHostActive(configured)}
                  label={`Sync and include ${name}`}
                  onChange={(checked) => toggleSshHost(host.alias, checked)}
                />
              </div>
              <div class="ssh-host-actions">
                <button class="ssh-btn" type="button" aria-label={`Test connection to ${name}`} disabled={sshTestingHost !== null || sshTestingAll || sshSyncing} onclick={() => testSshHost(host.alias)}>
                  {sshTestingHost === host.alias ? "Testing…" : "Test connection"}
                </button>
                <span class="ssh-result" class:ssh-ok={result?.success} class:ssh-fail={result && !result.success} role="status" title={result?.message}>
                  {#if sshTestingHost !== host.alias && result}
                    {result.success ? "Test passed" : "Test failed"}
                  {/if}
                </span>
              </div>
              {#if result && !result.success && sshTestingHost !== host.alias}
                <p class="ssh-test-message">{result.message}</p>
              {/if}
            </div>
          {/each}
          {#if sshHosts.length === 0}
            <div class="ssh-empty">No hosts found in ~/.ssh/config</div>
          {/if}
        </div>
      </section>

      <section class="remote-section auto-section" aria-label="Auto Sync devices">
        <div class="section-heading">
          <span class="section-title">Auto Sync</span>
          <span class="section-count">{activeAutoSyncDeviceCount} of {autoSyncDevices.length}</span>
        </div>
        <p class="section-description">Include synced devices in your usage.</p>
        {#if deviceUsageLoading}
          <div class="ssh-empty" role="status">Loading devices…</div>
        {:else if deviceUsageError}
          <div class="ssh-empty error-text" role="status">{deviceUsageError}</div>
        {:else if autoSyncDevices.length > 0}
          <div class="auto-devices">
            {#each autoSyncDevices as device (device.alias)}
              {@const name = autoSyncDeviceNames.get(device.alias) ?? device.alias}
              <div class="auto-device-row">
                <div class="ssh-host-info">
                  <span class="ssh-alias" title={device.alias}>{name}</span>
                  <span class="ssh-detail" class:error-text={device.error_message}>{autoSyncDetail(device)}</span>
                </div>
                <ToggleSwitch
                  checked={device.include_in_stats}
                  label={`Include ${name} in usage`}
                  onChange={(checked) => toggleRemoteDeviceInclude(device, checked)}
                />
              </div>
            {/each}
          </div>
        {:else}
          <div class="ssh-empty">No Auto Sync devices found</div>
        {/if}
      </section>
    </div>

    <div class="ssh-sync-row">
      <span class="ssh-sync-label" role="status">
        {#if sshSyncResult}
          <span class="ssh-sync-status" class:ssh-sync-error={sshSyncResult.msg.startsWith("Failed")}>{sshSyncResult.msg}</span>
        {:else}
          {activeRemoteDeviceCount} {activeRemoteDeviceCount === 1 ? "device" : "devices"} enabled
        {/if}
      </span>
      <div class="ssh-sync-actions">
        <button class="ssh-btn sync-btn" type="button" onclick={testAllSshHosts} disabled={sshTestingAll || sshTestingHost !== null || sshSyncing || sshHosts.length === 0}>
          {sshTestingAll ? "Testing…" : "Test All"}
        </button>
        <button class="ssh-btn sync-btn" type="button" onclick={syncAllRemoteDevices} disabled={sshSyncing || sshTestingAll || sshTestingHost !== null}>
          {sshSyncing ? "Syncing…" : "Sync All"}
        </button>
      </div>
    </div>
  </SettingsDisclosure>
</div>

<style>
  .block {
    border-top: 1px solid var(--border-subtle);
  }
  .devices-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 12px;
  }
  .collapsible-toggle {
    flex: 1;
    min-width: 0;
    min-height: 36px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 10px 0;
    background: none;
    border: none;
    cursor: pointer;
    user-select: none;
    text-align: left;
  }
  .devices-header:has(.collapsible-toggle:hover) {
    background: var(--surface-hover);
  }
  .label {
    font: 400 10px/1.3 "Inter", sans-serif;
    color: var(--t1);
  }
  .collapsible-right {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 8px;
  }
  .collapsible-chevron {
    color: var(--t3);
    transform: rotate(-90deg);
  }
  .collapsible-chevron.open {
    transform: rotate(0deg);
  }
  .count {
    font: 400 9px/1 "Inter", sans-serif;
    color: var(--t3);
    white-space: nowrap;
  }
  .remote-content {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 4px 12px 12px;
  }
  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .section-title {
    font: 600 10px/1.4 "Inter", sans-serif;
    color: var(--t1);
  }
  .section-count {
    font: 400 9px/1.4 "Inter", sans-serif;
    color: var(--t2);
    white-space: nowrap;
  }
  .section-description {
    margin-top: 4px;
    font: 400 9px/1.4 "Inter", sans-serif;
    color: var(--t2);
  }
  .ssh-hosts,
  .auto-devices {
    margin-top: 8px;
  }
  .ssh-host-row,
  .auto-device-row {
    padding: 10px 0;
  }
  .ssh-host-row + .ssh-host-row,
  .auto-device-row + .auto-device-row {
    border-top: 1px solid var(--border-subtle);
  }
  .ssh-host-row:last-child,
  .auto-device-row:last-child {
    padding-bottom: 0;
  }
  .device-heading,
  .auto-device-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .ssh-host-info {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .ssh-alias {
    font: 500 10.5px/1.35 "Inter", sans-serif;
    color: var(--t1);
    overflow-wrap: anywhere;
  }
  .ssh-detail {
    font: 400 9px/1.4 "Inter", sans-serif;
    color: var(--t2);
    overflow-wrap: anywhere;
  }
  .ssh-host-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 8px;
  }
  .ssh-btn {
    flex-shrink: 0;
    min-height: 24px;
    background: var(--surface-hover);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 4px 8px;
    font: 400 9px/1.3 "Inter", sans-serif;
    color: var(--t2);
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--t-fast) ease, color var(--t-fast) ease, border-color var(--t-fast) ease;
  }
  .ssh-btn:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--t1);
    border-color: var(--t3);
  }
  .ssh-btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .ssh-btn:focus-visible,
  .collapsible-toggle:focus-visible {
    outline: 2px solid var(--t2);
    outline-offset: 2px;
  }
  .collapsible-toggle:focus-visible {
    outline-offset: -2px;
  }
  .ssh-result {
    font: 500 9px/1.4 "Inter", sans-serif;
  }
  .ssh-ok { color: var(--ch-plus); }
  .ssh-fail,
  .ssh-test-message,
  .error-text { color: var(--ch-minus); }
  .ssh-test-message {
    margin-top: 6px;
    font: 400 9px/1.4 "Inter", sans-serif;
    overflow-wrap: anywhere;
  }
  .ssh-empty {
    padding: 12px 0 4px;
    font: 400 9px/1.5 "Inter", sans-serif;
    color: var(--t2);
    overflow-wrap: anywhere;
  }
  .ssh-empty.error-text {
    color: var(--ch-minus);
  }
  .ssh-sync-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px;
    border-top: 1px solid var(--border-subtle);
  }
  .ssh-sync-actions {
    display: flex;
    flex-shrink: 0;
    gap: 8px;
  }
  .ssh-sync-label {
    flex: 1;
    min-width: 0;
    font: 400 9px/1.4 "Inter", sans-serif;
    color: var(--t2);
    overflow-wrap: anywhere;
  }
  .ssh-sync-status {
    color: var(--ch-plus);
  }
  .ssh-sync-error {
    color: var(--ch-minus);
  }
  .sync-btn {
    min-height: 28px;
    padding: 6px 10px;
    color: var(--t1);
  }
  @media (prefers-reduced-motion: reduce) {
    .ssh-btn { transition: none; }
  }
</style>
