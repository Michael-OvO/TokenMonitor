<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { formatCost, formatTimeAgo, deviceColor } from "../utils/format.js";
  import { activePeriod, activeOffset, activeProvider } from "../stores/usage.js";
  import type { DeviceUsagePayload, DeviceSummary } from "../types/index.js";

  interface Props {
    onBack: () => void;
    onDeviceSelect?: (device: string) => void;
    onSettings?: () => void;
  }

  let { onBack, onDeviceSelect, onSettings }: Props = $props();

  let data = $state<DeviceUsagePayload | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let syncing = $state(false);
  let provider = $derived($activeProvider);
  let period = $derived($activePeriod);
  let offset = $derived($activeOffset);

  /** Format a raw UTC offset like "+0800" or "-0500" into "UTC+8" or "UTC-5". */
  function formatTzOffset(raw: string): string {
    const match = raw.match(/^([+-])(\d{2})(\d{2})$/);
    if (!match) return `UTC${raw}`;
    const [, sign, hh, mm] = match;
    const h = parseInt(hh, 10);
    const m = parseInt(mm, 10);
    const suffix = m > 0 ? `:${mm}` : "";
    return `UTC${sign}${h}${suffix}`;
  }

  const STATUS_COLORS: Record<string, string> = {
    online: "#22c55e",
    offline: "#9ca3af",
    syncing: "#eab308",
    error: "#f97316",
    unknown: "#6b7280",
  };

  async function fetchDeviceData() {
    loading = true;
    error = null;
    try {
      data = await invoke<DeviceUsagePayload>("get_device_usage", {
        provider,
        period,
        offset,
      });
    } catch (e) {
      error = String(e);
      data = null;
    }
    loading = false;
  }

  // Fetch on mount and refetch when provider/period/offset changes.
  // In Svelte 5, $effect runs immediately on mount, so onMount is not needed.
  $effect(() => {
    void provider;
    void period;
    void offset;
    fetchDeviceData();
  });

  // Sort: local device first, then remote sorted by cost descending.
  let sortedDevices = $derived.by(() => {
    if (!data) return [];
    const local: DeviceSummary[] = [];
    const remote: DeviceSummary[] = [];
    for (const d of data.devices) {
      if (d.is_local) {
        local.push(d);
      } else {
        remote.push(d);
      }
    }
    const sortedRemote = [...remote].sort((a, b) => b.total_cost - a.total_cost);
    return [...local, ...sortedRemote];
  });

  // Identify remote enabled devices for Sync All.
  let remoteDevices = $derived(
    sortedDevices.filter((d) => !d.is_local),
  );

  async function syncAll() {
    if (syncing || remoteDevices.length === 0) return;
    syncing = true;
    try {
      for (const d of remoteDevices) {
        await invoke("sync_ssh_host", { alias: d.device });
      }
      await fetchDeviceData();
    } finally {
      syncing = false;
    }
  }

  function handleDeviceClick(device: string) {
    onDeviceSelect?.(device);
  }
</script>

<div class="devices-view">
  <div class="header">
    <button class="back" type="button" onclick={onBack}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="15 18 9 12 15 6"></polyline>
      </svg>
      <span>Devices</span>
    </button>
    {#if data}
      <span class="total">{formatCost(data.total_cost)}</span>
    {/if}
  </div>

  <div class="scroll">
    {#if loading}
      <div class="skeleton-cards" aria-busy="true" aria-label="Loading devices">
        {#each [1, 2, 3] as _}
          <div class="skeleton-card">
            <div class="skeleton-row">
              <div class="skeleton skeleton-dot"></div>
              <div class="skeleton skeleton-text" style="width: 80px"></div>
              <div class="skeleton skeleton-text-r" style="width: 40px"></div>
            </div>
            <div class="skeleton skeleton-bar"></div>
            <div class="skeleton-row">
              <div class="skeleton skeleton-text-sm" style="width: 60px"></div>
              <div class="skeleton skeleton-text-sm" style="width: 30px"></div>
            </div>
          </div>
        {/each}
      </div>
    {:else if error}
      <div class="error-state">
        <svg class="empty-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#ef4444" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="10"></circle>
          <line x1="15" y1="9" x2="9" y2="15"></line>
          <line x1="9" y1="9" x2="15" y2="15"></line>
        </svg>
        <div class="error-title">Failed to load devices</div>
        <div class="error-text">{error}</div>
        <button class="retry-btn" type="button" onclick={fetchDeviceData}>Retry</button>
      </div>
    {:else if sortedDevices.length > 0}
      {#each sortedDevices as device (device.device)}
        <button
          class="device-card"
          type="button"
          onclick={() => handleDeviceClick(device.device)}
        >
          <div class="device-header">
            <div class="device-name-row">
              <span
                class="status-dot"
                style:background={STATUS_COLORS[device.status] ?? "#6b7280"}
                title={device.status}
              ></span>
              <span class="device-name">{device.device}</span>
              {#if device.is_local}
                <span class="local-badge">This device</span>
              {/if}
            </div>
            <div class="device-cost-row">
              <span class="device-pct">{device.cost_percentage.toFixed(0)}%</span>
              <span class="device-cost">{formatCost(device.total_cost)}</span>
            </div>
          </div>

          <div class="device-bar-bg">
            <div
              class="device-bar-fill"
              style:width="{Math.max(device.cost_percentage, 2)}%"
              style:background={deviceColor(device.device)}
            ></div>
          </div>

          {#if !device.is_local && device.last_synced}
            <div class="last-synced">Synced {formatTimeAgo(device.last_synced)}{#if device.remote_tz} · {formatTzOffset(device.remote_tz)}{/if}</div>
          {/if}

          {#if device.error_message}
            <div class="device-error">{device.error_message}</div>
          {/if}

          <div class="model-list">
            {#each device.model_breakdown as model (model.model_key)}
              <div class="model-row">
                <div class="model-dot" style:background={deviceColor(device.device)}></div>
                <span class="model-name">{model.display_name}</span>
                <span class="model-cost">{formatCost(model.cost)}</span>
              </div>
            {/each}
          </div>
        </button>
      {/each}
    {:else}
      <div class="empty-state">
        <svg class="empty-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--t4)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <rect x="2" y="3" width="20" height="14" rx="2" ry="2"></rect>
          <line x1="8" y1="21" x2="16" y2="21"></line>
          <line x1="12" y1="17" x2="12" y2="21"></line>
        </svg>
        <div class="empty-title">No device data</div>
        <div class="empty-text">Configure SSH hosts in Settings to see remote device costs.</div>
      </div>
    {/if}
  </div>

  {#if !loading && !error && sortedDevices.length > 0}
    <div class="action-bar">
      {#if remoteDevices.length > 0}
        <button
          class="sync-btn"
          class:spinning={syncing}
          type="button"
          onclick={syncAll}
          disabled={syncing}
        >
          <svg class="sync-icon" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="23 4 23 10 17 10"></polyline>
            <polyline points="1 20 1 14 7 14"></polyline>
            <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
          </svg>
          {syncing ? "Syncing..." : "Sync All"}
        </button>
      {/if}
      {#if onSettings}
        <button class="settings-link" type="button" onclick={onSettings}>
          Settings
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .devices-view {
    height: 460px;
    display: flex;
    flex-direction: column;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px 6px;
  }

  .back {
    display: flex;
    align-items: center;
    gap: 4px;
    background: none;
    border: none;
    cursor: pointer;
    color: var(--t1);
    font: 600 12px/1 'Inter', sans-serif;
    padding: 0;
  }
  .back:hover { color: var(--t2); }

  .total {
    font: 600 12px/1 'Inter', sans-serif;
    color: var(--t1);
    font-variant-numeric: tabular-nums;
  }

  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 10px 10px;
  }

  /* ── Skeleton loading ── */
  .skeleton-cards { padding: 0 10px; }
  .skeleton-card {
    padding: 10px 12px;
    margin-bottom: 8px;
    background: var(--surface-2);
    border-radius: 8px;
  }
  .skeleton-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
  }
  .skeleton-dot { width: 6px; height: 6px; border-radius: 50%; }
  .skeleton-text { height: 10px; }
  .skeleton-text-r { height: 10px; margin-left: auto; }
  .skeleton-bar { height: 4px; width: 100%; margin-bottom: 6px; }
  .skeleton-text-sm { height: 8px; }

  /* ── Empty & error states ── */
  .empty-state, .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 30px 10px;
    text-align: center;
  }
  .empty-icon { display: block; margin-bottom: 4px; opacity: 0.6; }
  .empty-title, .error-title {
    font: 500 11px/1 'Inter', sans-serif;
    color: var(--t1);
  }
  .empty-text {
    font: 400 9px/1.4 'Inter', sans-serif;
    color: var(--t3);
    max-width: 220px;
  }
  .error-text {
    font: 400 9px/1.4 'Inter', sans-serif;
    color: #ef4444;
    max-width: 220px;
  }
  .retry-btn {
    margin-top: 8px;
    padding: 5px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 5px;
    background: transparent;
    color: var(--t2);
    font: 500 9px/1 'Inter', sans-serif;
    cursor: pointer;
    transition: background var(--t-fast) ease, color var(--t-fast) ease;
  }
  .retry-btn:hover {
    background: var(--surface-hover);
    color: var(--t1);
  }

  .device-card {
    display: block;
    width: 100%;
    text-align: left;
    background: var(--surface-2);
    border-radius: 8px;
    padding: 10px 12px;
    margin-bottom: 8px;
    border: 1px solid transparent;
    cursor: pointer;
    transition: border-color 0.15s ease;
  }
  .device-card:hover {
    border-color: var(--t4);
    background: var(--surface-hover);
  }

  .device-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 6px;
  }

  .device-name-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .device-name {
    font: 600 10px/1.2 'Inter', sans-serif;
    color: var(--t1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .local-badge {
    font: 500 7.5px/1 'Inter', sans-serif;
    color: var(--t3);
    background: var(--surface-hover, rgba(128, 128, 128, 0.12));
    border-radius: 4px;
    padding: 2px 5px;
    flex-shrink: 0;
    white-space: nowrap;
  }

  .device-cost-row {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .device-pct {
    font: 400 8px/1 'Inter', sans-serif;
    color: var(--t3);
    font-variant-numeric: tabular-nums;
  }

  .device-cost {
    font: 600 10px/1 'Inter', sans-serif;
    color: var(--t1);
    font-variant-numeric: tabular-nums;
  }

  .device-bar-bg {
    height: 4px;
    background: var(--surface-hover);
    border-radius: 2px;
    margin-bottom: 6px;
    overflow: hidden;
  }

  .device-bar-fill {
    height: 100%;
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .last-synced {
    font: 400 7.5px/1 'Inter', sans-serif;
    color: var(--t4);
    margin-bottom: 4px;
  }

  .device-error {
    font: 400 7.5px/1.3 'Inter', sans-serif;
    color: #f97316;
    margin-bottom: 4px;
  }

  .model-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .model-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .model-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .model-name {
    flex: 1;
    font: 400 9px/1.25 'Inter', sans-serif;
    color: var(--t2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .model-cost {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t2);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }

  /* ── Action bar ── */

  .action-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-top: 1px solid var(--surface-hover);
  }

  .sync-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font: 500 9px/1 'Inter', sans-serif;
    color: var(--t1);
    background: var(--surface-2);
    border: 1px solid var(--t4);
    border-radius: 6px;
    padding: 5px 10px;
    cursor: pointer;
    transition: background 0.15s ease, border-color 0.15s ease;
  }
  .sync-btn:hover:not(:disabled) {
    border-color: var(--t3);
    background: var(--surface-hover);
  }
  .sync-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .sync-btn.spinning .sync-icon {
    animation: refresh-spin 900ms linear infinite;
    transform-origin: center;
  }
  @keyframes refresh-spin {
    to { transform: rotate(360deg); }
  }

  .settings-link {
    font: 400 9px/1 'Inter', sans-serif;
    color: var(--t3);
    background: none;
    border: none;
    cursor: pointer;
    padding: 4px 0;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .settings-link:hover {
    color: var(--t2);
  }
</style>
