use chrono::Timelike;
use tauri::State;

use crate::commands::period::{format_day_label, resolve_period_bounds_for_provider, PeriodBounds};
use crate::commands::AppState;
use crate::models::{ChartBucket, ChartSegment, DeviceUsagePayload};
use crate::usage::device_aggregation::{
    archived_hour_keys, bucket_key_for_local, bucket_label_for_key, build_device_chart_buckets,
    build_device_summary_from_parsed, build_device_summary_merged, enrich_cost_percentages,
    enumerate_agg_devices, live_index, live_records_in, provider_includes_remote_ssh_usage,
};
use crate::usage::integrations::UsageIntegrationSelection;
use crate::usage::ssh_config::{discover_ssh_hosts, SshHostInfo};
use crate::usage::ssh_remote::{
    CompactUsageRecord, SshHostConfig, SshHostStatus, SshSyncResult, SshTestResult,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RemoteDeviceIncludeFlag {
    pub alias: String,
    #[serde(default = "crate::usage::ssh_remote::default_true")]
    pub include_in_stats: bool,
}

async fn period_bounds_for(
    state: &AppState,
    provider: &str,
    period: &str,
    offset: i32,
) -> Result<PeriodBounds, String> {
    let cached = state.cached_rate_limits.read().await;
    resolve_period_bounds_for_provider(period, offset, provider, cached.as_ref())
        .map_err(|e| format!("Invalid period: {e}"))
}

fn validate_ssh_alias(alias: &str) -> Result<(), String> {
    if alias.is_empty() {
        return Err("SSH alias cannot be empty".to_string());
    }
    if alias.starts_with('-') {
        return Err("SSH alias cannot start with a hyphen".to_string());
    }
    if alias.starts_with('.') || alias.contains("..") {
        return Err("SSH alias cannot start with a dot or contain '..'".to_string());
    }
    if !alias
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(
            "SSH alias can only contain alphanumeric characters, hyphens, underscores, and dots"
                .to_string(),
        );
    }
    Ok(())
}

/// Invalidate the cached usage-view payloads (in-memory + disk) so the next
/// `get_usage_data` recomputes with the current device set. The enabled /
/// include-in-stats flags only change a total on a cache MISS, so a config
/// mutation that doesn't clear these caches leaves the displayed cost frozen.
///
/// Under the compute gate, so no compute that read the old device set can
/// store its view after the clear. Disk first: the disk clear waits out the
/// disk hits in flight, and the memory clear after it drops the copies they
/// made. Then a refresh publishes the new totals on every surface, and its
/// sample drops whatever else was built from the old set.
async fn invalidate_usage_view_cache(state: &AppState) {
    // A config change may be the fix for a host the background sync holds back.
    if let Ok(mut holds) = state.refresh.ssh_holds.lock() {
        holds.clear();
    }
    {
        let _gate = state.compute.lock().await;
        state.clear_payload_disk_cache().await;
        state.parser.clear_payload_cache_prefix("usage-view:");
    }
    state
        .refresh
        .pending_change
        .store(true, std::sync::atomic::Ordering::SeqCst);
    crate::refresh::request_refresh(state);
}

/// Get all SSH hosts discovered from ~/.ssh/config.
#[tauri::command]
pub async fn get_ssh_hosts() -> Result<Vec<SshHostInfo>, String> {
    let entries = discover_ssh_hosts();
    Ok(entries.iter().map(SshHostInfo::from).collect())
}

/// Get the status of all configured SSH hosts (sync time, entry count, etc.).
#[tauri::command]
pub async fn get_ssh_host_statuses(
    state: State<'_, AppState>,
) -> Result<Vec<SshHostStatus>, String> {
    let configs = state.ssh_hosts.read().await;
    let cache_mgr = state.ssh_cache.read().await;

    match cache_mgr.as_ref() {
        Some(mgr) => Ok(mgr.host_statuses(&configs)),
        None => Ok(Vec::new()),
    }
}

/// Add a new SSH host to the monitored list.
#[tauri::command]
pub async fn add_ssh_host(alias: String, state: State<'_, AppState>) -> Result<(), String> {
    validate_ssh_alias(&alias)?;

    {
        let mut configs = state.ssh_hosts.write().await;

        if configs.iter().any(|c| c.alias == alias) {
            return Err(format!("Host '{alias}' is already configured"));
        }

        configs.push(SshHostConfig {
            alias,
            enabled: true,
            include_in_stats: true,
        });
    }

    // A newly-added host defaults to enabled+included, so it changes totals now.
    invalidate_usage_view_cache(&state).await;
    Ok(())
}

/// Toggle an SSH host's enabled state.
#[tauri::command]
pub async fn toggle_ssh_host(
    alias: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut configs = state.ssh_hosts.write().await;
        if let Some(cfg) = configs.iter_mut().find(|c| c.alias == alias) {
            cfg.enabled = enabled;
        }
    }

    // Enabling/disabling a host adds/removes its cost from the totals; clear the
    // usage-view caches so the next fetch recomputes instead of re-serving the
    // pre-toggle total from the 120s in-memory / disk cache.
    invalidate_usage_view_cache(&state).await;
    Ok(())
}

/// Toggle whether a device's costs are included in the main statistics.
#[tauri::command]
pub async fn toggle_device_include_in_stats(
    alias: String,
    include_in_stats: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    validate_ssh_alias(&alias)?;
    let updated_config = {
        let mut configs = state.ssh_hosts.write().await;
        if let Some(cfg) = configs.iter_mut().find(|c| c.alias == alias) {
            cfg.include_in_stats = include_in_stats;
            true
        } else {
            false
        }
    };

    if !updated_config {
        let mut flags = state.remote_device_include_flags.write().await;
        flags.insert(alias, include_in_stats);
    }

    invalidate_usage_view_cache(&state).await;
    Ok(())
}

/// Initialize archive-only remote device include flags from persisted settings.
#[tauri::command]
pub async fn init_remote_device_include_flags(
    flags: Vec<RemoteDeviceIncludeFlag>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut normalized = HashMap::new();
    for flag in flags {
        validate_ssh_alias(&flag.alias)?;
        normalized.insert(flag.alias, flag.include_in_stats);
    }

    let mut current = state.remote_device_include_flags.write().await;
    *current = normalized;
    Ok(())
}

/// Initialize SSH hosts from persisted settings (called on startup).
#[tauri::command]
pub async fn init_ssh_hosts(
    hosts: Vec<SshHostConfig>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let valid: Vec<SshHostConfig> = hosts
        .into_iter()
        .filter(|h| {
            if let Err(e) = validate_ssh_alias(&h.alias) {
                tracing::warn!("Skipping SSH host with invalid alias {:?}: {e}", h.alias);
                false
            } else {
                true
            }
        })
        .collect();
    let mut configs = state.ssh_hosts.write().await;
    *configs = valid;
    Ok(())
}

/// Test connectivity to an SSH host.
#[tauri::command]
pub async fn test_ssh_connection(
    alias: String,
    state: State<'_, AppState>,
) -> Result<SshTestResult, String> {
    validate_ssh_alias(&alias)?;
    let test = crate::usage::ssh_remote::test_connection(&alias).await;
    // Fixed now: the background sync may try the host again.
    if test.success {
        if let Ok(mut holds) = state.refresh.ssh_holds.lock() {
            holds.remove(&alias);
        }
    }
    Ok(test)
}

/// Manually trigger a sync for a specific SSH host (with pre-test).
#[tauri::command]
pub async fn sync_ssh_host(
    alias: String,
    state: State<'_, AppState>,
) -> Result<SshSyncResult, String> {
    validate_ssh_alias(&alias)?;
    // The user is retrying the host: let the background sync try it again too.
    if let Ok(mut holds) = state.refresh.ssh_holds.lock() {
        holds.remove(&alias);
    }

    // Step 1: Test connection first.
    let test = crate::usage::ssh_remote::test_connection(&alias).await;

    if !test.success {
        return Ok(SshSyncResult {
            test_success: false,
            test_message: test.message,
            test_duration_ms: test.duration_ms,
            records_synced: 0,
            diagnostic: Some("SSH connection test failed".to_string()),
        });
    }

    // Step 2: Connection OK — proceed with sync.
    // Clone the cache manager and drop the read lock before the long SSH I/O
    // to avoid holding the RwLock across the await (potentially 10+ seconds).
    let mgr = {
        let cache_mgr = state.ssh_cache.read().await;
        cache_mgr
            .as_ref()
            .ok_or_else(|| "SSH cache not initialized".to_string())?
            .clone()
    };
    // One host sync at a time: the refresh's background sync of this host
    // would write the same temp file. Within the host timeout, like the
    // background sync: a stalled remote would otherwise hold the lock, and
    // every background sync with it, indefinitely.
    let count = crate::usage::ssh_remote::with_host_timeout(async {
        let _one_sync = state.refresh.ssh_sync.lock().await;
        mgr.sync_host(&alias).await
    })
    .await
    .map_err(|_| format!("SSH sync of '{alias}' timed out after 60s"))??;

    if count > 0 {
        apply_manual_sync(&state, &mgr, &alias);
    }

    let diagnostic = if count == 0 {
        Some(
            "No usage data found. Verify ~/.claude/projects/ or ~/.codex/sessions/ exists on the remote host."
                .to_string(),
        )
    } else {
        None
    };

    Ok(SshSyncResult {
        test_success: true,
        test_message: test.message,
        test_duration_ms: test.duration_ms,
        records_synced: count,
        diagnostic,
    })
}

/// A manual sync rewrote `alias`'s remote cache: read its records afresh,
/// and refresh now, which drops the views built from the old ones and
/// publishes the new. Until then the published views stay.
fn apply_manual_sync(
    state: &AppState,
    mgr: &crate::usage::ssh_remote::SshCacheManager,
    alias: &str,
) {
    mgr.invalidate_records(alias);
    // The records memo no longer holds the host, so the sample's
    // revalidation cannot see the change itself.
    state
        .refresh
        .pending_change
        .store(true, std::sync::atomic::Ordering::SeqCst);
    crate::refresh::request_refresh(state);
}

/// Get device-level usage breakdown.
///
/// Returns costs grouped by device (local + each enabled SSH host).
///
/// `provider` mirrors the active dashboard tab (`"all"`, `"claude"`, `"codex"`,
/// `"cursor"`). Local entries and remote records are filtered by model family
/// so the per-device totals match the header tab's scope. Remote devices are
/// hidden entirely for providers that don't produce remote logs (e.g.
/// `cursor`).
#[tauri::command]
pub async fn get_device_usage(
    provider: String,
    period: String,
    offset: i32,
    state: State<'_, AppState>,
) -> Result<DeviceUsagePayload, String> {
    if UsageIntegrationSelection::parse(&provider).is_none() {
        return Err(format!("Invalid provider: {provider}"));
    }
    let _gate = state.compute.lock().await;

    let bounds = period_bounds_for(&state, &provider, &period, offset).await?;
    let since = bounds.start;

    let period_label = format_day_label(since);
    let parser = &state.parser;

    // 1. Local device usage — filtered by the selected provider tab.
    let mut devices = Vec::new();
    let mut total_cost = 0.0;
    if state.usage_access_enabled() {
        let local = parser.load_entries_cached(&provider, Some(since));
        let mut local_summary = build_device_summary_from_parsed("Local", &local.entries, &bounds);
        local_summary.is_local = true;
        local_summary.status = String::from("online");
        total_cost = local_summary.total_cost;
        devices.push(local_summary);
    }

    // 2. Remote device usage from archive + compact cached records.
    // Skip entirely when the active provider doesn't produce remote logs
    // (e.g. `cursor`), matching the Per-Device breakdown on the Usage page.
    let include_remote = provider_includes_remote_ssh_usage(&provider);
    let configs = state.ssh_hosts.read().await;
    let cache_mgr = state.ssh_cache.read().await;
    let archive = parser.archive();

    if include_remote {
        // Devices = enabled SSH hosts ∪ archive (file-imported / auto-synced
        // peer) devices — the SAME enumeration as the Usage-page Per-Device
        // breakdown. Without the archive set, a peer that has no live SSH
        // connection (imported from another machine's export) never appeared on
        // this page even though it shows in the breakdown.
        let mgr = cache_mgr.as_ref();
        let agg_devices = {
            let remote_include_flags = state.remote_device_include_flags.read().await;
            enumerate_agg_devices(&configs, archive.as_ref(), &remote_include_flags)
        };
        let statuses = mgr.map(|m| m.host_statuses(&configs)).unwrap_or_default();
        for dev in &agg_devices {
            let source_key = format!("device:{}", dev.alias);
            let frontier = archive.as_ref().and_then(|a| a.frontier(&source_key));

            // Load archived entries for this device (completed hours),
            // filtered to the selected provider's model family.
            let archived_entries: Vec<_> = archive
                .as_ref()
                .map(|a| a.load_archived(&source_key, Some(since)))
                .unwrap_or_default()
                .into_iter()
                .filter(|e| {
                    crate::usage::integrations::remote_record_matches_provider(&provider, &e.model)
                })
                .collect();
            let archived_hours = archived_hour_keys(&archived_entries);

            // An archive-only peer with no data for the active provider would be
            // an empty row — skip it (provider scoping). Configured SSH hosts
            // always show, even with no data, so their status stays visible.
            if !dev.configured && archived_entries.is_empty() {
                continue;
            }

            // Live compact rows only for configured SSH hosts (file-imported
            // peers have no SSH cache and contribute from archived data alone),
            // of the active provider tab's families, in hours the archive has
            // no rows for.
            let index = live_index(mgr, dev);
            let live_records: Vec<&CompactUsageRecord> = live_records_in(
                &index,
                &provider,
                &bounds,
                frontier.as_ref(),
                &archived_hours,
            )
            .map(|(_, record)| record)
            .collect();

            // Build summary: archived entries + live compact records.
            let mut summary =
                build_device_summary_merged(&dev.alias, &archived_entries, &live_records, &bounds);

            if dev.configured {
                // Enrich with live status from the SSH cache manager.
                if let Some(host_status) = statuses.iter().find(|s| s.alias == dev.alias) {
                    summary.last_synced = host_status.last_sync.clone();
                    summary.error_message = host_status.last_error.clone();
                    summary.remote_tz = host_status.remote_tz.clone();
                    summary.status = if host_status.last_error.is_some() {
                        String::from("error")
                    } else if host_status.last_sync.is_some() {
                        String::from("online")
                    } else {
                        String::from("offline")
                    };
                }
            } else {
                // File-synced peer — no live SSH connection.
                summary.status = String::from("offline");
            }
            summary.include_in_stats = dev.include_in_stats;

            total_cost += summary.total_cost;
            devices.push(summary);
        }
    }

    // 3. Log device data for debugging.
    tracing::debug!(
        "[DEVICE] get_device_usage: provider={provider} period={period} offset={offset} total_cost={total_cost:.2}"
    );
    for d in &devices {
        tracing::debug!(
            "[DEVICE] get_device_usage device={} cost={:.2} is_local={}",
            d.device,
            d.total_cost,
            d.is_local,
        );
    }

    // 4. Compute cost percentages.
    enrich_cost_percentages(&mut devices, total_cost);

    // 5. Build chart buckets by device.
    let chart_buckets = build_device_chart_buckets(&devices);

    Ok(DeviceUsagePayload {
        devices,
        total_cost,
        chart_buckets,
        last_updated: chrono::Local::now().to_rfc3339(),
        period_label,
    })
}

/// Get usage data for a single device.
///
/// `provider` filters rows by model family (matching the active dashboard tab)
/// so the single-device view stays consistent with the header selection.
#[tauri::command]
pub async fn get_single_device_usage(
    device: String,
    provider: String,
    period: String,
    offset: i32,
    state: State<'_, AppState>,
) -> Result<crate::models::UsagePayload, String> {
    use crate::models::{ModelSummary, UsagePayload, UsageSource};
    use crate::usage::integrations::remote_record_matches_provider;
    use crate::usage::pricing::{
        calculate_cost_for_key, pricing_available_for_key, provider_multiplier,
    };
    use std::collections::HashMap;

    validate_ssh_alias(&device).or_else(|_| {
        if device == "Local" {
            Ok(())
        } else {
            Err(format!("Invalid device name: {device}"))
        }
    })?;

    if UsageIntegrationSelection::parse(&provider).is_none() {
        return Err(format!("Invalid provider: {provider}"));
    }
    let _gate = state.compute.lock().await;

    let bounds = period_bounds_for(&state, &provider, &period, offset).await?;
    let since = bounds.start;

    let period_label = format_day_label(since);
    let parser = &state.parser;

    type ModelAggMap = HashMap<String, (String, f64, u64, bool)>;
    let mut model_map: ModelAggMap = HashMap::new();
    let mut bucket_map: HashMap<String, ModelAggMap> = HashMap::new();

    if device == "Local" {
        if !state.usage_access_enabled() {
            return Ok(UsagePayload {
                period_label,
                usage_warning: Some(String::from("Usage access has not been enabled yet.")),
                ..UsagePayload::default()
            });
        }
        let loaded = parser.load_entries_cached(&provider, Some(since));
        for entry in &loaded.entries {
            if !bounds.contains_timestamp(entry.timestamp) {
                continue;
            }
            let (display_name, model_key) = crate::models::normalize_model(&entry.model);
            let pricing_available = pricing_available_for_key(&model_key);
            let cost = calculate_cost_for_key(
                &model_key,
                entry.input_tokens,
                entry.output_tokens,
                entry.cache_creation_5m_tokens,
                entry.cache_creation_1h_tokens,
                entry.cache_read_tokens,
                0,
            ) * provider_multiplier(&entry.model);
            let tokens = entry.input_tokens + entry.output_tokens;
            let agg = model_map
                .entry(model_key.clone())
                .or_insert_with(|| (display_name.clone(), 0.0, 0, true));
            agg.1 += cost;
            agg.2 += tokens;
            agg.3 &= pricing_available;

            let bk = bucket_key_for_local(&entry.timestamp, &period);
            let bucket_model = bucket_map
                .entry(bk)
                .or_default()
                .entry(model_key)
                .or_insert_with(|| (display_name, 0.0, 0, true));
            bucket_model.1 += cost;
            bucket_model.2 += tokens;
            bucket_model.3 &= pricing_available;
        }
    } else {
        // Remote device = archive (completed hours) + live SSH cache (recent
        // hours past the archive frontier). Reading the archive is what lets a
        // file-imported / auto-synced peer (which has NO live SSH cache) render
        // here at all, and gives configured hosts their full archived history.
        let source_key = format!("device:{device}");
        let archive = parser.archive();
        let frontier = archive.as_ref().and_then(|a| a.frontier(&source_key));
        let mut archived_hours = HashSet::new();

        // Archived completed hours, filtered to the active provider's family.
        if let Some(ref a) = archive {
            for entry in a.load_archived(&source_key, Some(since)) {
                if !remote_record_matches_provider(&provider, &entry.model) {
                    continue;
                }
                archived_hours.insert((entry.timestamp.date_naive(), entry.timestamp.hour() as u8));
                if !bounds.contains_timestamp(entry.timestamp) {
                    continue;
                }
                let (display_name, model_key) = crate::models::normalize_model(&entry.model);
                let pricing_available = pricing_available_for_key(&model_key);
                let cost = calculate_cost_for_key(
                    &model_key,
                    entry.input_tokens,
                    entry.output_tokens,
                    entry.cache_creation_5m_tokens,
                    entry.cache_creation_1h_tokens,
                    entry.cache_read_tokens,
                    0,
                ) * provider_multiplier(&entry.model);
                let tokens = entry.input_tokens + entry.output_tokens;
                let agg = model_map
                    .entry(model_key.clone())
                    .or_insert_with(|| (display_name.clone(), 0.0, 0, true));
                agg.1 += cost;
                agg.2 += tokens;
                agg.3 &= pricing_available;

                let bk = bucket_key_for_local(&entry.timestamp, &period);
                let bucket_model = bucket_map
                    .entry(bk)
                    .or_default()
                    .entry(model_key)
                    .or_insert_with(|| (display_name, 0.0, 0, true));
                bucket_model.1 += cost;
                bucket_model.2 += tokens;
                bucket_model.3 &= pricing_available;
            }
        }

        // Live SSH-cache records for configured hosts, skipping hours already
        // covered by the archive frontier (so they aren't double-counted).
        let cache_mgr = state.ssh_cache.read().await;
        if let Some(mgr) = cache_mgr.as_ref() {
            let index = mgr.load_record_index(&device).unwrap_or_else(|e| {
                tracing::warn!("Failed to load cached records for {device}: {e}");
                Default::default()
            });
            for (local, record) in live_records_in(
                &index,
                &provider,
                &bounds,
                frontier.as_ref(),
                &archived_hours,
            ) {
                let (display_name, model_key) = crate::models::normalize_model(&record.model);
                let pricing_available = pricing_available_for_key(&model_key);
                let cost = calculate_cost_for_key(
                    &model_key,
                    record.input_tokens,
                    record.output_tokens,
                    record.cache_5m,
                    record.cache_1h,
                    record.cache_read,
                    0,
                ) * provider_multiplier(&record.model);
                let tokens = record.input_tokens + record.output_tokens;
                let agg = model_map
                    .entry(model_key.clone())
                    .or_insert_with(|| (display_name.clone(), 0.0, 0, true));
                agg.1 += cost;
                agg.2 += tokens;
                agg.3 &= pricing_available;

                let bk = bucket_key_for_local(local, &period);
                let bucket_model = bucket_map
                    .entry(bk)
                    .or_default()
                    .entry(model_key)
                    .or_insert_with(|| (display_name, 0.0, 0, true));
                bucket_model.1 += cost;
                bucket_model.2 += tokens;
                bucket_model.3 &= pricing_available;
            }
        }
    }

    let mut model_breakdown: Vec<ModelSummary> = model_map
        .into_iter()
        .map(
            |(model_key, (display_name, cost, tokens, pricing_available))| ModelSummary {
                display_name,
                model_key,
                cost,
                tokens,
                pricing_available,
                change_stats: None,
            },
        )
        .collect();
    model_breakdown.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut chart_buckets: Vec<ChartBucket> = bucket_map
        .into_iter()
        .map(|(key, models)| {
            let mut segments: Vec<ChartSegment> = models
                .into_iter()
                .map(
                    |(model_key, (display, cost, tokens, pricing_available))| ChartSegment {
                        model: display,
                        model_key,
                        cost,
                        tokens,
                        pricing_available,
                    },
                )
                .collect();
            segments.sort_by(|a, b| {
                b.cost
                    .partial_cmp(&a.cost)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let total: f64 = segments.iter().map(|s| s.cost).sum();
            let label = bucket_label_for_key(&key, &period);
            ChartBucket {
                label,
                sort_key: key,
                total,
                segments,
            }
        })
        .collect();
    chart_buckets.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));

    let total_cost: f64 = model_breakdown.iter().map(|m| m.cost).sum();
    let total_tokens: u64 = model_breakdown.iter().map(|m| m.tokens).sum();

    Ok(UsagePayload {
        total_cost,
        total_tokens,
        model_breakdown,
        chart_buckets,
        last_updated: chrono::Local::now().to_rfc3339(),
        usage_source: UsageSource::Parser,
        period_label,
        ..UsagePayload::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::ssh_remote::SshCacheManager;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn a_manual_sync_requests_a_refresh_and_keeps_the_views_until_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let mgr = SshCacheManager::new(dir.path());
        let cache = dir
            .path()
            .join("remote-cache")
            .join("box")
            .join("usage.jsonl");
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        let line = r#"{"ts":"2026-01-01T00:00:00Z","m":"claude-sonnet-4-6","in":1,"out":1}"#;
        std::fs::write(&cache, format!("{line}\n")).unwrap();
        assert_eq!(mgr.load_cached_records_shared("box").unwrap().len(), 1);

        let state = AppState::new();
        let view = "usage-view:all:day:0:test";
        state
            .parser
            .store_cache(view, state.parser.get_daily("claude", "20260101"));

        // The sync rewrote the host's cache.
        std::fs::write(&cache, format!("{line}\n{line}\n")).unwrap();
        apply_manual_sync(&state, &mgr, "box");

        assert_eq!(
            mgr.load_cached_records_shared("box").unwrap().len(),
            2,
            "the host's records are read afresh"
        );
        assert!(
            state.refresh.pending_change.load(Ordering::SeqCst),
            "the sample drops the views built from the old records"
        );
        assert!(
            state.refresh.requested.load(Ordering::SeqCst),
            "and runs now"
        );
        assert!(
            state.parser.check_cache_as_stored(view).is_some(),
            "until then the published view stays"
        );
    }

    #[tokio::test]
    async fn a_device_set_change_drops_the_views_and_refreshes() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = AppState::new();
        let disk = crate::usage::payload_disk_cache::PayloadDiskCache::new(dir.path());
        let view = "usage-view:all:day:0:test";
        let payload = state.parser.get_daily("claude", "20260101");
        disk.save(view, &payload);
        *state.payload_disk_cache.write().await = Some(disk);
        state.parser.store_cache(view, payload);

        // A compute in flight holds the gate: the clear waits for it, so
        // nothing it built from the old device set outlives the clear.
        let gate = state.compute.lock().await;
        let invalidate = invalidate_usage_view_cache(&state);
        tokio::pin!(invalidate);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut invalidate)
                .await
                .is_err(),
            "waits for the gate"
        );
        drop(gate);
        invalidate.await;

        assert!(state.parser.check_cache_as_stored(view).is_none());
        let disk = state.payload_disk_cache.read().await;
        assert!(disk.as_ref().unwrap().load(view).is_none());
        assert!(state.refresh.pending_change.load(Ordering::SeqCst));
        assert!(
            state.refresh.requested.load(Ordering::SeqCst),
            "the new totals are published now"
        );
    }
}
