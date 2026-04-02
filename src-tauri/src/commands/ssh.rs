use chrono::Timelike;
use tauri::State;

use crate::commands::AppState;
use crate::models::{
    ChartBucket, ChartSegment, DeviceModelSummary, DeviceSummary, DeviceUsagePayload,
};
use crate::usage::integrations::ALL_USAGE_INTEGRATIONS_ID;
use crate::usage::ssh_config::{discover_ssh_hosts, SshHostInfo};
use crate::usage::ssh_remote::{
    CompactUsageRecord, SshHostConfig, SshHostStatus, SshSyncResult, SshTestResult,
};

/// Validate an SSH alias to prevent command injection and path traversal.
/// Only allows alphanumeric characters, hyphens, underscores, and dots.
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

/// Remote SSH usage may contain both Claude and Codex records.
fn provider_includes_remote_ssh_usage(provider: &str) -> bool {
    matches!(provider, ALL_USAGE_INTEGRATIONS_ID | "claude" | "codex")
}

/// Check if a compact record's model matches the requested provider.
fn compact_record_matches_provider(record: &CompactUsageRecord, provider: &str) -> bool {
    use crate::models::{detect_model_family, ModelFamily};
    match provider {
        ALL_USAGE_INTEGRATIONS_ID => true,
        "claude" => detect_model_family(&record.model) == ModelFamily::Anthropic,
        "codex" => detect_model_family(&record.model) == ModelFamily::OpenAI,
        _ => true,
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

    let mut configs = state.ssh_hosts.write().await;

    if configs.iter().any(|c| c.alias == alias) {
        return Err(format!("Host '{alias}' is already configured"));
    }

    configs.push(SshHostConfig {
        alias,
        enabled: true,
        include_in_stats: false,
    });

    Ok(())
}

/// Remove an SSH host from the monitored list.
#[tauri::command]
pub async fn remove_ssh_host(alias: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut configs = state.ssh_hosts.write().await;
    configs.retain(|c| c.alias != alias);
    Ok(())
}

/// Toggle an SSH host's enabled state.
#[tauri::command]
pub async fn toggle_ssh_host(
    alias: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut configs = state.ssh_hosts.write().await;

    if let Some(cfg) = configs.iter_mut().find(|c| c.alias == alias) {
        cfg.enabled = enabled;
    }

    Ok(())
}

/// Toggle whether a device's costs are included in the main statistics.
#[tauri::command]
pub async fn toggle_device_include_in_stats(
    alias: String,
    include_in_stats: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut configs = state.ssh_hosts.write().await;

    if let Some(cfg) = configs.iter_mut().find(|c| c.alias == alias) {
        cfg.include_in_stats = include_in_stats;
    }

    Ok(())
}

/// Initialize SSH hosts from persisted settings (called on startup).
#[tauri::command]
pub async fn init_ssh_hosts(
    hosts: Vec<SshHostConfig>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut configs = state.ssh_hosts.write().await;
    *configs = hosts;
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
    let cache_mgr = state.ssh_cache.read().await;
    let count = match cache_mgr.as_ref() {
        Some(mgr) => mgr.sync_host(&alias).await?,
        None => return Err("SSH cache not initialized".to_string()),
    };

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
#[tauri::command]
pub async fn get_device_usage(
    period: String,
    offset: i32,
    state: State<'_, AppState>,
) -> Result<DeviceUsagePayload, String> {
    use crate::commands::period::{compute_date_bounds, format_day_label};

    let (since, end) =
        compute_date_bounds(&period, offset).ok_or_else(|| format!("Invalid period: {period}"))?;

    let period_label = format_day_label(since);
    let parser = &state.parser;

    // 1. Local device usage (all providers combined).
    let (local_entries, _, _) = parser.load_entries("all", Some(since));
    let mut local_summary = build_device_summary_from_parsed("Local", &local_entries, since, end);
    local_summary.is_local = true;
    local_summary.status = String::from("online");

    let mut devices = vec![local_summary];
    let mut total_cost = devices[0].total_cost;

    // 2. Remote device usage from compact cached records.
    let configs = state.ssh_hosts.read().await;
    let cache_mgr = state.ssh_cache.read().await;

    if let Some(mgr) = cache_mgr.as_ref() {
        let statuses = mgr.host_statuses(&configs);
        for cfg in configs.iter().filter(|c| c.enabled) {
            let records = mgr.load_cached_records(&cfg.alias);
            let mut summary = build_device_summary_from_compact(&cfg.alias, &records, since, end);

            // Enrich with status from cache manager.
            if let Some(host_status) = statuses.iter().find(|s| s.alias == cfg.alias) {
                summary.last_synced = host_status.last_sync.clone();
                summary.error_message = host_status.last_error.clone();
                summary.status = if host_status.last_error.is_some() {
                    String::from("error")
                } else if host_status.last_sync.is_some() {
                    String::from("online")
                } else {
                    String::from("offline")
                };
            }
            summary.include_in_stats = cfg.include_in_stats;

            total_cost += summary.total_cost;
            devices.push(summary);
        }
    }

    // 3. Compute cost percentages.
    enrich_cost_percentages(&mut devices, total_cost);

    // 4. Build chart buckets by device.
    let chart_buckets = build_device_chart_buckets(&devices);

    Ok(DeviceUsagePayload {
        devices,
        total_cost,
        chart_buckets,
        last_updated: chrono::Local::now().to_rfc3339(),
        period_label,
    })
}

// ── Summary builders ────────────────────────────────────────────────────────

/// Build a DeviceSummary from local ParsedEntry items.
fn build_device_summary_from_parsed(
    device_name: &str,
    entries: &[crate::usage::parser::ParsedEntry],
    _since: chrono::NaiveDate,
    end: chrono::NaiveDate,
) -> DeviceSummary {
    use crate::models::normalize_model;
    use crate::usage::pricing::calculate_cost_for_key;
    use std::collections::HashMap;

    let mut model_map: HashMap<String, (String, f64, u64)> = HashMap::new();

    for entry in entries {
        if entry.timestamp.date_naive() >= end {
            continue;
        }

        let (display_name, model_key) = normalize_model(&entry.model);
        let cost = calculate_cost_for_key(
            &model_key,
            entry.input_tokens,
            entry.output_tokens,
            entry.cache_creation_5m_tokens,
            entry.cache_creation_1h_tokens,
            entry.cache_read_tokens,
        );
        let tokens = entry.input_tokens + entry.output_tokens;

        let agg = model_map
            .entry(model_key)
            .or_insert_with(|| (display_name, 0.0, 0));
        agg.1 += cost;
        agg.2 += tokens;
    }

    finish_device_summary(device_name, model_map)
}

/// Build a DeviceSummary from compact remote records.
fn build_device_summary_from_compact(
    device_name: &str,
    records: &[CompactUsageRecord],
    since: chrono::NaiveDate,
    end: chrono::NaiveDate,
) -> DeviceSummary {
    use crate::models::normalize_model;
    use crate::usage::pricing::calculate_cost_for_key;
    use std::collections::HashMap;

    let mut model_map: HashMap<String, (String, f64, u64)> = HashMap::new();

    for record in records {
        // Skip synthetic/internal models.
        if record.model.starts_with('<') {
            continue;
        }

        // Filter by date range [since, end).
        let record_date = chrono::DateTime::parse_from_rfc3339(&record.ts)
            .or_else(|_| chrono::DateTime::parse_from_str(&record.ts, "%Y-%m-%dT%H:%M:%S%.f%z"))
            .map(|dt| dt.date_naive())
            .unwrap_or(chrono::NaiveDate::MIN);

        if record_date < since || record_date >= end {
            continue;
        }

        let (display_name, model_key) = normalize_model(&record.model);
        let cost = calculate_cost_for_key(
            &model_key,
            record.input_tokens,
            record.output_tokens,
            record.cache_5m,
            record.cache_1h,
            record.cache_read,
        );
        let tokens = record.input_tokens + record.output_tokens;

        let agg = model_map
            .entry(model_key)
            .or_insert_with(|| (display_name, 0.0, 0));
        agg.1 += cost;
        agg.2 += tokens;
    }

    finish_device_summary(device_name, model_map)
}

/// Shared logic: convert model_map into a sorted DeviceSummary.
fn finish_device_summary(
    device_name: &str,
    model_map: std::collections::HashMap<String, (String, f64, u64)>,
) -> DeviceSummary {
    let mut model_breakdown: Vec<DeviceModelSummary> = model_map
        .into_iter()
        .map(
            |(model_key, (display_name, cost, tokens))| DeviceModelSummary {
                display_name: format!("{} -- {}", display_name, device_name),
                model_key,
                cost,
                tokens,
            },
        )
        .collect();
    model_breakdown.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_cost: f64 = model_breakdown.iter().map(|m| m.cost).sum();
    let total_tokens: u64 = model_breakdown.iter().map(|m| m.tokens).sum();

    DeviceSummary {
        device: device_name.to_string(),
        total_cost,
        total_tokens,
        model_breakdown,
        is_local: false,
        status: String::from("unknown"),
        last_synced: None,
        error_message: None,
        cost_percentage: 0.0,
        include_in_stats: false,
    }
}

/// Compute cost_percentage for each device based on total cost.
fn enrich_cost_percentages(devices: &mut [DeviceSummary], total_cost: f64) {
    if total_cost > 0.0 {
        for device in devices.iter_mut() {
            device.cost_percentage = (device.total_cost / total_cost) * 100.0;
        }
    }
}

/// Build device breakdown for embedding in UsagePayload.
///
/// Returns None if no SSH hosts are configured.
/// Called from get_usage_data to populate device_breakdown field.
pub(crate) async fn build_device_breakdown_for_payload(
    state: &AppState,
    provider: &str,
    period: &str,
    offset: i32,
) -> Option<Vec<DeviceSummary>> {
    use crate::commands::period::compute_date_bounds;

    let configs = state.ssh_hosts.read().await;
    if configs.iter().all(|c| !c.enabled) {
        return None;
    }

    let (since, end) = compute_date_bounds(period, offset)?;
    let parser = &state.parser;

    let (local_entries, _, _) = parser.load_entries(provider, Some(since));
    let mut local_summary = build_device_summary_from_parsed("Local", &local_entries, since, end);
    local_summary.is_local = true;
    local_summary.status = String::from("online");

    let mut devices = vec![local_summary];
    let mut total_cost = devices[0].total_cost;

    if !provider_includes_remote_ssh_usage(provider) {
        enrich_cost_percentages(&mut devices, total_cost);
        return Some(devices);
    }

    let cache_mgr = state.ssh_cache.read().await;
    if let Some(mgr) = cache_mgr.as_ref() {
        let statuses = mgr.host_statuses(&configs);
        for cfg in configs.iter().filter(|c| c.enabled) {
            let all_records = mgr.load_cached_records(&cfg.alias);
            let filtered: Vec<_> = all_records
                .into_iter()
                .filter(|r| compact_record_matches_provider(r, provider))
                .collect();
            if filtered.is_empty() {
                continue;
            }
            let mut summary = build_device_summary_from_compact(&cfg.alias, &filtered, since, end);

            if let Some(host_status) = statuses.iter().find(|s| s.alias == cfg.alias) {
                summary.last_synced = host_status.last_sync.clone();
                summary.error_message = host_status.last_error.clone();
                summary.status = if host_status.last_error.is_some() {
                    String::from("error")
                } else if host_status.last_sync.is_some() {
                    String::from("online")
                } else {
                    String::from("offline")
                };
            }
            summary.include_in_stats = cfg.include_in_stats;

            total_cost += summary.total_cost;
            devices.push(summary);
        }
    }

    enrich_cost_percentages(&mut devices, total_cost);
    Some(devices)
}

/// Build a UsagePayload contribution from remote devices with include_in_stats=true.
///
/// Returns None if no devices have include_in_stats enabled.
/// The returned payload can be merged into the main payload via merge_payloads.
pub(crate) async fn build_included_devices_payload(
    state: &AppState,
    provider: &str,
    period: &str,
    offset: i32,
) -> Option<crate::models::UsagePayload> {
    use crate::commands::period::compute_date_bounds;
    use crate::models::{ModelSummary, UsagePayload, UsageSource};
    use crate::usage::pricing::calculate_cost_for_key;
    use std::collections::HashMap;

    if !provider_includes_remote_ssh_usage(provider) {
        return None;
    }

    let configs = state.ssh_hosts.read().await;
    let included: Vec<_> = configs
        .iter()
        .filter(|c| c.enabled && c.include_in_stats)
        .collect();
    if included.is_empty() {
        return None;
    }

    let (since, end) = compute_date_bounds(period, offset)?;
    let cache_mgr = state.ssh_cache.read().await;
    let mgr = cache_mgr.as_ref()?;

    let mut model_map: HashMap<String, (String, f64, u64)> = HashMap::new();
    let mut chart_entries: Vec<(chrono::DateTime<chrono::FixedOffset>, String, f64, u64)> =
        Vec::new();
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;

    for cfg in &included {
        let records = mgr.load_cached_records(&cfg.alias);
        for record in records
            .iter()
            .filter(|r| compact_record_matches_provider(r, provider))
        {
            // Skip synthetic/internal models.
            if record.model.starts_with('<') {
                continue;
            }

            let parsed_ts = chrono::DateTime::parse_from_rfc3339(&record.ts).or_else(|_| {
                chrono::DateTime::parse_from_str(&record.ts, "%Y-%m-%dT%H:%M:%S%.f%z")
            });
            let record_date = parsed_ts
                .as_ref()
                .map(|dt| dt.date_naive())
                .unwrap_or(chrono::NaiveDate::MIN);

            if record_date < since || record_date >= end {
                continue;
            }

            let (display_name, model_key) = crate::models::normalize_model(&record.model);
            let cost = calculate_cost_for_key(
                &model_key,
                record.input_tokens,
                record.output_tokens,
                record.cache_5m,
                record.cache_1h,
                record.cache_read,
            );
            let tokens = record.input_tokens + record.output_tokens;
            input_tokens += record.input_tokens;
            output_tokens += record.output_tokens;

            let agg = model_map
                .entry(model_key.clone())
                .or_insert_with(|| (display_name, 0.0, 0));
            agg.1 += cost;
            agg.2 += tokens;

            if let Ok(ts) = parsed_ts {
                chart_entries.push((ts, model_key, cost, tokens));
            }
        }
    }

    if model_map.is_empty() {
        return None;
    }

    let mut model_breakdown: Vec<ModelSummary> = model_map
        .into_iter()
        .map(|(model_key, (display_name, cost, tokens))| ModelSummary {
            display_name,
            model_key,
            cost,
            tokens,
            change_stats: None,
        })
        .collect();
    model_breakdown.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Build chart buckets by date (same bucketing as the main chart).
    use crate::models::{ChartBucket, ChartSegment};
    use std::collections::BTreeMap;

    let mut bucket_map: BTreeMap<String, BTreeMap<String, (String, f64, u64)>> = BTreeMap::new();
    for (ts, model_key, cost, tokens) in &chart_entries {
        let bkey = bucket_key_for_timestamp(ts, period);
        let model_entry = bucket_map.entry(bkey).or_default();
        let (_, model_cost, model_tokens) =
            model_entry.entry(model_key.clone()).or_insert_with(|| {
                let (display, _) = crate::models::normalize_model(model_key);
                (display, 0.0, 0)
            });
        *model_cost += cost;
        *model_tokens += tokens;
    }

    let chart_buckets: Vec<ChartBucket> = bucket_map
        .into_iter()
        .map(|(key, models)| {
            let mut segments: Vec<ChartSegment> = models
                .into_iter()
                .map(|(model_key, (display, cost, tokens))| ChartSegment {
                    model: display,
                    model_key,
                    cost,
                    tokens,
                })
                .collect();
            segments.sort_by(|a, b| {
                b.cost
                    .partial_cmp(&a.cost)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let total: f64 = segments.iter().map(|s| s.cost).sum();
            let label = bucket_label_for_key(&key, period);
            ChartBucket {
                label,
                sort_key: key,
                total,
                segments,
            }
        })
        .collect();

    let total_cost: f64 = model_breakdown.iter().map(|m| m.cost).sum();
    let total_tokens: u64 = model_breakdown.iter().map(|m| m.tokens).sum();

    Some(UsagePayload {
        total_cost,
        total_tokens,
        input_tokens,
        output_tokens,
        chart_buckets,
        model_breakdown,
        usage_source: UsageSource::Parser,
        ..UsagePayload::default()
    })
}

/// Build time-based chart buckets with segments grouped by device.
///
/// Unlike build_device_chart_buckets (one bucket per device), this produces
/// time-axis buckets where each bucket's segments are devices.
pub(crate) async fn build_device_time_chart_buckets(
    state: &AppState,
    provider: &str,
    period: &str,
    offset: i32,
) -> Option<Vec<ChartBucket>> {
    use crate::commands::period::compute_date_bounds;
    use crate::models::normalize_model;
    use crate::usage::pricing::calculate_cost_for_key;
    use std::collections::HashMap;

    let configs = state.ssh_hosts.read().await;
    if configs.iter().all(|c| !c.enabled) {
        return None;
    }

    let (since, end) = compute_date_bounds(period, offset)?;
    let parser = &state.parser;

    // Collect (timestamp, device, cost) tuples from all devices.
    let mut device_cost_by_bucket: HashMap<String, HashMap<String, f64>> = HashMap::new();

    // Local entries.
    let (local_entries, _, _) = parser.load_entries(provider, Some(since));
    for entry in &local_entries {
        let date = entry.timestamp.date_naive();
        if date >= end {
            continue;
        }
        let bucket_key = bucket_key_for_timestamp(&entry.timestamp.fixed_offset(), period);
        let (_, model_key) = normalize_model(&entry.model);
        let cost = calculate_cost_for_key(
            &model_key,
            entry.input_tokens,
            entry.output_tokens,
            entry.cache_creation_5m_tokens,
            entry.cache_creation_1h_tokens,
            entry.cache_read_tokens,
        );
        *device_cost_by_bucket
            .entry(bucket_key)
            .or_default()
            .entry(String::from("Local"))
            .or_insert(0.0) += cost;
    }

    // Remote entries.
    if provider_includes_remote_ssh_usage(provider) {
        let cache_mgr = state.ssh_cache.read().await;
        if let Some(mgr) = cache_mgr.as_ref() {
            for cfg in configs.iter().filter(|c| c.enabled) {
                let records = mgr.load_cached_records(&cfg.alias);
                for record in records
                    .iter()
                    .filter(|r| compact_record_matches_provider(r, provider))
                {
                    let parsed_ts =
                        chrono::DateTime::parse_from_rfc3339(&record.ts).or_else(|_| {
                            chrono::DateTime::parse_from_str(&record.ts, "%Y-%m-%dT%H:%M:%S%.f%z")
                        });
                    let record_date = parsed_ts
                        .as_ref()
                        .map(|dt| dt.date_naive())
                        .unwrap_or(chrono::NaiveDate::MIN);

                    if record_date < since || record_date >= end {
                        continue;
                    }

                    let ts_ref = match &parsed_ts {
                        Ok(ts) => ts,
                        Err(_) => continue,
                    };
                    let bucket_key = bucket_key_for_timestamp(ts_ref, period);
                    let (_, model_key) = normalize_model(&record.model);
                    let cost = calculate_cost_for_key(
                        &model_key,
                        record.input_tokens,
                        record.output_tokens,
                        record.cache_5m,
                        record.cache_1h,
                        record.cache_read,
                    );
                    *device_cost_by_bucket
                        .entry(bucket_key)
                        .or_default()
                        .entry(cfg.alias.clone())
                        .or_insert(0.0) += cost;
                }
            }
        }
    }

    // Convert to chart buckets sorted by key.
    let mut buckets: Vec<ChartBucket> = device_cost_by_bucket
        .into_iter()
        .map(|(key, device_costs)| {
            let total: f64 = device_costs.values().sum();
            let mut segments: Vec<ChartSegment> = device_costs
                .into_iter()
                .map(|(device, cost)| ChartSegment {
                    model: device.clone(),
                    model_key: device,
                    cost,
                    tokens: 0,
                })
                .collect();
            segments.sort_by(|a, b| {
                b.cost
                    .partial_cmp(&a.cost)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let label = bucket_label_for_key(&key, period);
            ChartBucket {
                label,
                sort_key: key,
                total,
                segments,
            }
        })
        .collect();

    buckets.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    Some(buckets)
}

/// Map a timestamp to a bucket key matching the main chart's bucketing.
///
/// - 5h: per-hour (`%Y-%m-%dT%H:00:00` with timezone offset)
/// - day: per-hour (`%H` → "00"–"23", matches `get_hourly` sort_key)
/// - week/month: per-day (`%Y-%m-%d`)
/// - year: per-month (`%Y-%m`)
fn bucket_key_for_timestamp(ts: &chrono::DateTime<chrono::FixedOffset>, period: &str) -> String {
    let local = ts.with_timezone(&chrono::Local);
    match period {
        "5h" => local.format("%Y-%m-%dT%H:00:00%z").to_string(),
        "day" => format!("{:02}", local.hour()),
        "week" | "month" => local.format("%Y-%m-%d").to_string(),
        "year" => local.format("%Y-%m").to_string(),
        _ => local.format("%Y-%m-%d").to_string(),
    }
}

/// Convert a bucket sort_key back to a display label matching the main chart.
///
/// - `day`: sort_key `"00"`–`"23"` → `format_hour` ("12AM", "9AM", …)
/// - `week`/`month`: sort_key `"2026-03-30"` → `"%b %-d"` ("Mar 30")
/// - `year`: sort_key `"2026-03"` → `"%b"` ("Mar")
/// - fallback: return sort_key as-is
fn bucket_label_for_key(sort_key: &str, period: &str) -> String {
    match period {
        "day" => {
            if let Ok(h) = sort_key.parse::<u32>() {
                return crate::usage::parser::format_hour(h);
            }
        }
        "week" | "month" => {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(sort_key, "%Y-%m-%d") {
                return d.format("%b %-d").to_string();
            }
        }
        "year" => {
            if let Ok(d) =
                chrono::NaiveDate::parse_from_str(&format!("{}-01", sort_key), "%Y-%m-%d")
            {
                return d.format("%b").to_string();
            }
        }
        _ => {}
    }
    sort_key.to_string()
}

/// Get usage data for a single device.
#[tauri::command]
pub async fn get_single_device_usage(
    device: String,
    period: String,
    offset: i32,
    state: State<'_, AppState>,
) -> Result<crate::models::UsagePayload, String> {
    use crate::commands::period::{compute_date_bounds, format_day_label};
    use crate::models::{ModelSummary, UsagePayload, UsageSource};
    use crate::usage::pricing::calculate_cost_for_key;
    use std::collections::HashMap;

    validate_ssh_alias(&device).or_else(|_| {
        if device == "Local" {
            Ok(())
        } else {
            Err(format!("Invalid device name: {device}"))
        }
    })?;

    let (since, end) =
        compute_date_bounds(&period, offset).ok_or_else(|| format!("Invalid period: {period}"))?;

    let period_label = format_day_label(since);
    let parser = &state.parser;

    let mut model_map: HashMap<String, (String, f64, u64)> = HashMap::new();

    if device == "Local" {
        let (entries, _, _) = parser.load_entries("all", Some(since));
        for entry in &entries {
            if entry.timestamp.date_naive() >= end {
                continue;
            }
            let (display_name, model_key) = crate::models::normalize_model(&entry.model);
            let cost = calculate_cost_for_key(
                &model_key,
                entry.input_tokens,
                entry.output_tokens,
                entry.cache_creation_5m_tokens,
                entry.cache_creation_1h_tokens,
                entry.cache_read_tokens,
            );
            let tokens = entry.input_tokens + entry.output_tokens;
            let agg = model_map
                .entry(model_key)
                .or_insert_with(|| (display_name, 0.0, 0));
            agg.1 += cost;
            agg.2 += tokens;
        }
    } else {
        let cache_mgr = state.ssh_cache.read().await;
        if let Some(mgr) = cache_mgr.as_ref() {
            let records = mgr.load_cached_records(&device);
            for record in &records {
                let record_date = chrono::DateTime::parse_from_rfc3339(&record.ts)
                    .or_else(|_| {
                        chrono::DateTime::parse_from_str(&record.ts, "%Y-%m-%dT%H:%M:%S%.f%z")
                    })
                    .map(|dt| dt.date_naive())
                    .unwrap_or(chrono::NaiveDate::MIN);

                if record_date < since || record_date >= end {
                    continue;
                }

                let (display_name, model_key) = crate::models::normalize_model(&record.model);
                let cost = calculate_cost_for_key(
                    &model_key,
                    record.input_tokens,
                    record.output_tokens,
                    record.cache_5m,
                    record.cache_1h,
                    record.cache_read,
                );
                let tokens = record.input_tokens + record.output_tokens;
                let agg = model_map
                    .entry(model_key)
                    .or_insert_with(|| (display_name, 0.0, 0));
                agg.1 += cost;
                agg.2 += tokens;
            }
        }
    }

    let mut model_breakdown: Vec<ModelSummary> = model_map
        .into_iter()
        .map(|(model_key, (display_name, cost, tokens))| ModelSummary {
            display_name,
            model_key,
            cost,
            tokens,
            change_stats: None,
        })
        .collect();
    model_breakdown.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_cost: f64 = model_breakdown.iter().map(|m| m.cost).sum();
    let total_tokens: u64 = model_breakdown.iter().map(|m| m.tokens).sum();

    Ok(UsagePayload {
        total_cost,
        total_tokens,
        model_breakdown,
        last_updated: chrono::Local::now().to_rfc3339(),
        usage_source: UsageSource::Parser,
        period_label,
        ..UsagePayload::default()
    })
}

/// Build chart buckets — one bucket per device for a stacked bar chart.
fn build_device_chart_buckets(devices: &[DeviceSummary]) -> Vec<ChartBucket> {
    devices
        .iter()
        .filter(|d| d.total_cost > 0.0)
        .map(|d| ChartBucket {
            label: d.device.clone(),
            sort_key: d.device.clone(),
            total: d.total_cost,
            segments: d
                .model_breakdown
                .iter()
                .map(|m| ChartSegment {
                    model: m.display_name.clone(),
                    model_key: m.model_key.clone(),
                    cost: m.cost,
                    tokens: m.tokens,
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::parser::UsageParser;
    use crate::usage::ssh_remote::SshCacheManager;
    use chrono::Local;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn write_file(path: &std::path::Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn claude_assistant_entry(
        ts: &str,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"model":"{model}","usage":{{"input_tokens":{input_tokens},"output_tokens":{output_tokens}}},"stop_reason":"end_turn"}}}}"#
        )
    }

    fn codex_token_count_entry(
        ts: &str,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> String {
        format!(
            r#"{{"type":"turn_context","payload":{{"cwd":"/tmp/demo","model":"{model}"}}}}
{{"type":"event_msg","timestamp":"{ts}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input_tokens},"output_tokens":{output_tokens},"reasoning_output_tokens":0,"cached_input_tokens":0}}}}}}}}"#
        )
    }

    fn remote_record(ts: &str, model: &str, input_tokens: u64, output_tokens: u64) -> String {
        format!(
            r#"{{"ts":"{ts}","m":"{model}","in":{input_tokens},"out":{output_tokens},"c5":0,"cr":0}}"#
        )
    }

    async fn build_state_with_remote_claude_data() -> (AppState, TempDir, TempDir, TempDir) {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let app_data_dir = TempDir::new().unwrap();
        let now = Local::now();
        let timestamp = now.to_rfc3339();

        write_file(
            &claude_dir.path().join("session.jsonl"),
            &claude_assistant_entry(&timestamp, "claude-sonnet-4-6-20260301", 1_000, 500),
        );

        let codex_day_dir = codex_dir
            .path()
            .join(now.format("%Y").to_string())
            .join(now.format("%m").to_string())
            .join(now.format("%d").to_string());
        write_file(
            &codex_day_dir.join("session.jsonl"),
            &codex_token_count_entry(&timestamp, "gpt-5.4", 800, 200),
        );

        let mut state = AppState::new();
        state.parser = Arc::new(UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        ));
        *state.ssh_hosts.write().await = vec![SshHostConfig {
            alias: String::from("remote-a"),
            enabled: true,
            include_in_stats: true,
        }];
        *state.ssh_cache.write().await = Some(SshCacheManager::new(app_data_dir.path()));
        write_file(
            &app_data_dir
                .path()
                .join("remote-cache")
                .join("remote-a")
                .join("usage.jsonl"),
            &remote_record(&timestamp, "claude-sonnet-4-6-20260301", 2_000, 1_000),
        );

        (state, claude_dir, codex_dir, app_data_dir)
    }

    #[tokio::test]
    async fn included_remote_payload_respects_provider_scope() {
        let (state, _claude_dir, _codex_dir, _app_data_dir) =
            build_state_with_remote_claude_data().await;

        let all_payload = build_included_devices_payload(&state, "all", "day", 0)
            .await
            .expect("all provider should include remote Claude data");
        let claude_payload = build_included_devices_payload(&state, "claude", "day", 0)
            .await
            .expect("claude provider should include remote Claude data");
        let codex_payload = build_included_devices_payload(&state, "codex", "day", 0).await;

        assert!(all_payload.total_cost > 0.0);
        assert!(claude_payload.total_cost > 0.0);
        assert_eq!(all_payload.total_cost, claude_payload.total_cost);
        assert_eq!(all_payload.input_tokens, 2_000);
        assert_eq!(all_payload.output_tokens, 1_000);
        assert!(codex_payload.is_none());
    }

    #[tokio::test]
    async fn device_breakdown_excludes_remote_rows_for_codex_pages() {
        let (state, _claude_dir, _codex_dir, _app_data_dir) =
            build_state_with_remote_claude_data().await;

        let claude_devices = build_device_breakdown_for_payload(&state, "claude", "day", 0)
            .await
            .expect("claude device breakdown should exist");
        let codex_devices = build_device_breakdown_for_payload(&state, "codex", "day", 0)
            .await
            .expect("codex device breakdown should exist");

        assert!(
            claude_devices
                .iter()
                .any(|device| device.device == "remote-a"),
            "claude pages should include remote Claude devices"
        );
        assert_eq!(codex_devices.len(), 1, "codex pages should stay local-only");
        assert_eq!(codex_devices[0].device, "Local");
        assert!(codex_devices[0].is_local);
    }

    #[tokio::test]
    async fn device_time_buckets_skip_remote_segments_for_codex_pages() {
        let (state, _claude_dir, _codex_dir, _app_data_dir) =
            build_state_with_remote_claude_data().await;

        let claude_buckets = build_device_time_chart_buckets(&state, "claude", "day", 0)
            .await
            .expect("claude device buckets should exist");
        let codex_buckets = build_device_time_chart_buckets(&state, "codex", "day", 0)
            .await
            .expect("codex device buckets should exist");

        assert!(
            claude_buckets
                .iter()
                .flat_map(|bucket| bucket.segments.iter())
                .any(|segment| segment.model_key == "remote-a"),
            "claude buckets should include remote device segments"
        );
        assert!(
            codex_buckets
                .iter()
                .flat_map(|bucket| bucket.segments.iter())
                .all(|segment| segment.model_key == "Local"),
            "codex buckets should stay local-only"
        );
    }

    #[tokio::test]
    async fn remote_codex_records_included_in_codex_provider() {
        let (state, _claude_dir, _codex_dir, app_data_dir) =
            build_state_with_remote_claude_data().await;

        let now = Local::now();
        let timestamp = now.to_rfc3339();

        // Append a Codex model record (gpt-5.4) to remote-a cache.
        let cache_path = app_data_dir
            .path()
            .join("remote-cache")
            .join("remote-a")
            .join("usage.jsonl");
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&cache_path)
            .unwrap();
        // Ensure newline separator from the existing record, then write new one.
        writeln!(file).unwrap();
        writeln!(file, "{}", remote_record(&timestamp, "gpt-5.4", 500, 200)).unwrap();

        // Codex provider should now include the remote Codex record.
        let codex_payload = build_included_devices_payload(&state, "codex", "day", 0)
            .await
            .expect("codex provider should include remote Codex data");
        assert!(codex_payload.total_cost > 0.0);
        assert_eq!(codex_payload.input_tokens, 500);
        assert_eq!(codex_payload.output_tokens, 200);

        // Claude provider should still only include Claude records from remote.
        let claude_payload = build_included_devices_payload(&state, "claude", "day", 0)
            .await
            .expect("claude provider should include remote Claude data");
        assert_eq!(claude_payload.input_tokens, 2_000);
        assert_eq!(claude_payload.output_tokens, 1_000);

        // All provider should include both.
        let all_payload = build_included_devices_payload(&state, "all", "day", 0)
            .await
            .expect("all provider should include all remote data");
        assert_eq!(all_payload.input_tokens, 2_500);
        assert_eq!(all_payload.output_tokens, 1_200);
    }

    #[tokio::test]
    async fn device_breakdown_shows_remote_for_codex_when_codex_records_exist() {
        let (state, _claude_dir, _codex_dir, app_data_dir) =
            build_state_with_remote_claude_data().await;

        let now = Local::now();
        let timestamp = now.to_rfc3339();

        // Append Codex model record to remote cache.
        let cache_path = app_data_dir
            .path()
            .join("remote-cache")
            .join("remote-a")
            .join("usage.jsonl");
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&cache_path)
            .unwrap();
        writeln!(file).unwrap();
        writeln!(file, "{}", remote_record(&timestamp, "gpt-5.4", 500, 200)).unwrap();

        let codex_devices = build_device_breakdown_for_payload(&state, "codex", "day", 0)
            .await
            .expect("codex device breakdown should exist");

        assert!(
            codex_devices.iter().any(|d| d.device == "remote-a"),
            "codex pages should now include remote device with Codex data"
        );

        // Remote device should only have Codex model costs.
        let remote = codex_devices
            .iter()
            .find(|d| d.device == "remote-a")
            .unwrap();
        assert!(remote.total_cost > 0.0);
        assert!(
            remote
                .model_breakdown
                .iter()
                .all(|m| !m.model_key.contains("claude")),
            "remote device in codex view should not contain Claude models"
        );
    }
}
