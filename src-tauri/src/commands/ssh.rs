use chrono::Timelike;
use std::sync::atomic::Ordering;
use tauri::State;

use crate::commands::AppState;
use crate::models::{ChartBucket, ChartSegment, DeviceUsagePayload};
use crate::usage::device_aggregation::{
    bucket_key_for_timestamp, bucket_label_for_key, build_device_chart_buckets,
    build_device_summary_from_parsed, build_device_summary_merged, compact_record_matches_provider,
    enrich_cost_percentages, enumerate_agg_devices, parse_remote_ts,
    provider_includes_remote_ssh_usage,
};
use crate::usage::integrations::UsageIntegrationSelection;
use crate::usage::ssh_config::{discover_ssh_hosts, SshHostInfo};
use crate::usage::ssh_remote::{
    CompactUsageRecord, SshHostConfig, SshHostStatus, SshSyncResult, SshTestResult,
};
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RemoteDeviceIncludeFlag {
    pub alias: String,
    #[serde(default = "default_true")]
    pub include_in_stats: bool,
}

fn default_true() -> bool {
    true
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

fn usage_access_enabled(state: &AppState) -> bool {
    state.usage_access_enabled.load(Ordering::SeqCst)
}

/// Invalidate the cached usage-view payloads (in-memory + disk) so the next
/// `get_usage_data` recomputes with the current device set. The enabled /
/// include-in-stats flags only change a total on a cache MISS, so a config
/// mutation that doesn't clear these caches leaves the displayed cost frozen
/// until the 120s TTL lapses. Mirrors `sync_ssh_host`'s cache clear.
async fn invalidate_usage_view_cache(state: &AppState) {
    state.parser.clear_payload_cache_prefix("usage-view:");
    if let Some(ref disk_cache) = *state.payload_disk_cache.read().await {
        disk_cache.clear_prefix("usage-view:");
    }
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

/// Remove an SSH host from the monitored list.
#[tauri::command]
pub async fn remove_ssh_host(alias: String, state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut configs = state.ssh_hosts.write().await;
        configs.retain(|c| c.alias != alias);
    }
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
pub async fn test_ssh_connection(alias: String) -> Result<SshTestResult, String> {
    validate_ssh_alias(&alias)?;
    Ok(crate::usage::ssh_remote::test_connection(&alias).await)
}

/// Manually trigger a sync for a specific SSH host (with pre-test).
#[tauri::command]
pub async fn sync_ssh_host(
    alias: String,
    state: State<'_, AppState>,
) -> Result<SshSyncResult, String> {
    validate_ssh_alias(&alias)?;

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
    let count = mgr.sync_host(&alias).await?;

    if count > 0 {
        state.parser.clear_payload_cache();
        if let Some(ref disk_cache) = *state.payload_disk_cache.read().await {
            disk_cache.clear_prefix("usage-view:");
        }
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
    use crate::commands::period::{compute_date_bounds, format_day_label};

    if UsageIntegrationSelection::parse(&provider).is_none() {
        return Err(format!("Invalid provider: {provider}"));
    }

    let (since, end) =
        compute_date_bounds(&period, offset).ok_or_else(|| format!("Invalid period: {period}"))?;

    let period_label = format_day_label(since);
    let parser = &state.parser;

    // 1. Local device usage — filtered by the selected provider tab.
    let mut devices = Vec::new();
    let mut total_cost = 0.0;
    if usage_access_enabled(&state) {
        let (local_entries, _, _) = parser.load_entries(&provider, Some(since));
        let mut local_summary =
            build_device_summary_from_parsed("Local", &local_entries, since, end);
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

            // Live compact rows only for configured SSH hosts; file-imported
            // peers have no SSH cache and contribute from archived data alone.
            let records: Vec<CompactUsageRecord> = if dev.configured {
                match mgr {
                    Some(m) => m.load_cached_records(&dev.alias).unwrap_or_else(|e| {
                        tracing::warn!("Failed to load cached records for {}: {e}", dev.alias);
                        Vec::new()
                    }),
                    None => Vec::new(),
                }
            } else {
                Vec::new()
            };

            // Filter live records:
            //  • drop model families outside the active provider tab
            //  • drop hours already covered by the archive frontier
            let live_records: Vec<&CompactUsageRecord> = records
                .iter()
                .filter(|r| compact_record_matches_provider(r, &provider))
                .filter(|r| {
                    if let Some(ref f) = frontier {
                        let dt = parse_remote_ts(&r.ts).map(|d| d.with_timezone(&chrono::Local));
                        match dt {
                            Some(local) => !f.covers(local.date_naive(), local.hour() as u8),
                            None => true, // Can't parse → include as live.
                        }
                    } else {
                        true
                    }
                })
                .collect();

            // An archive-only peer with no data for the active provider would be
            // an empty row — skip it (provider scoping). Configured SSH hosts
            // always show, even with no data, so their status stays visible.
            if !dev.configured && archived_entries.is_empty() && live_records.is_empty() {
                continue;
            }

            // Build summary: archived entries + live compact records.
            let mut summary = build_device_summary_merged(
                &dev.alias,
                &archived_entries,
                &live_records,
                since,
                end,
            );

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
    tracing::info!(
        "[DEVICE] get_device_usage: provider={provider} period={period} offset={offset} total_cost={total_cost:.2}"
    );
    for d in &devices {
        tracing::info!(
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
    use crate::commands::period::{compute_date_bounds, format_day_label};
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

    let (since, end) =
        compute_date_bounds(&period, offset).ok_or_else(|| format!("Invalid period: {period}"))?;

    let period_label = format_day_label(since);
    let parser = &state.parser;

    type ModelAggMap = HashMap<String, (String, f64, u64, bool)>;
    let mut model_map: ModelAggMap = HashMap::new();
    let mut bucket_map: HashMap<String, ModelAggMap> = HashMap::new();

    if device == "Local" {
        if !usage_access_enabled(&state) {
            return Ok(UsagePayload {
                period_label,
                usage_warning: Some(String::from("Usage access has not been enabled yet.")),
                ..UsagePayload::default()
            });
        }
        let (entries, _, _) = parser.load_entries(&provider, Some(since));
        for entry in &entries {
            if entry.timestamp.date_naive() >= end {
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

            let bk = bucket_key_for_timestamp(&entry.timestamp.fixed_offset(), &period);
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

        // Archived completed hours, filtered to the active provider's family.
        if let Some(ref a) = archive {
            for entry in a.load_archived(&source_key, Some(since)) {
                if !remote_record_matches_provider(&provider, &entry.model) {
                    continue;
                }
                let date = entry.timestamp.date_naive();
                if date < since || date >= end {
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

                let bk = bucket_key_for_timestamp(&entry.timestamp.fixed_offset(), &period);
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
            let records = match mgr.load_cached_records(&device) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Failed to load cached records for {device}: {e}");
                    Vec::new()
                }
            };
            for record in &records {
                if !remote_record_matches_provider(&provider, &record.model) {
                    continue;
                }
                let parsed_ts = match parse_remote_ts(&record.ts) {
                    Some(ts) => ts,
                    None => continue,
                };
                let local = parsed_ts.with_timezone(&chrono::Local);
                if let Some(ref f) = frontier {
                    if f.covers(local.date_naive(), local.hour() as u8) {
                        continue;
                    }
                }
                let record_date = local.date_naive();

                if record_date < since || record_date >= end {
                    continue;
                }

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

                let bk = bucket_key_for_timestamp(&parsed_ts, &period);
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
