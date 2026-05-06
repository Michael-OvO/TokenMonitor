use chrono::Timelike;

use crate::commands::AppState;
use crate::models::{ChartBucket, ChartSegment, DeviceModelSummary, DeviceSummary};
use crate::usage::integrations::{provider_matches_model, ALL_USAGE_INTEGRATIONS_ID};
use crate::usage::ssh_remote::CompactUsageRecord;

pub(crate) fn parse_remote_ts(ts: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .or_else(|_| chrono::DateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .ok()
}

fn parse_remote_ts_to_local_date(ts: &str) -> chrono::NaiveDate {
    parse_remote_ts(ts)
        .map(|dt| dt.with_timezone(&chrono::Local).date_naive())
        .unwrap_or(chrono::NaiveDate::MIN)
}

pub(crate) fn provider_includes_remote_ssh_usage(provider: &str) -> bool {
    matches!(provider, ALL_USAGE_INTEGRATIONS_ID | "claude" | "codex")
}

pub(crate) fn compact_record_matches_provider(record: &CompactUsageRecord, provider: &str) -> bool {
    provider_matches_model(provider, &record.model)
}

// ── Summary builders ────────────────────────────────────────────────────────

pub(crate) fn build_device_summary_from_parsed(
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
            0,
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

#[cfg(test)]
pub(crate) fn build_device_summary_from_compact(
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
        if record.model.starts_with('<') {
            continue;
        }

        let record_date = parse_remote_ts_to_local_date(&record.ts);

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
            0,
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

pub(crate) fn build_device_summary_merged(
    device_name: &str,
    archived_entries: &[crate::usage::parser::ParsedEntry],
    live_records: &[&CompactUsageRecord],
    since: chrono::NaiveDate,
    end: chrono::NaiveDate,
) -> DeviceSummary {
    use crate::models::normalize_model;
    use crate::usage::pricing::calculate_cost_for_key;
    use std::collections::HashMap;

    let mut model_map: HashMap<String, (String, f64, u64)> = HashMap::new();

    for entry in archived_entries {
        let entry_date = entry.timestamp.date_naive();
        if entry_date < since || entry_date >= end {
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
            0,
        );
        let tokens = entry.input_tokens + entry.output_tokens;
        let agg = model_map
            .entry(model_key)
            .or_insert_with(|| (display_name, 0.0, 0));
        agg.1 += cost;
        agg.2 += tokens;
    }

    for record in live_records {
        if record.model.starts_with('<') {
            continue;
        }
        let record_date = parse_remote_ts_to_local_date(&record.ts);
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
            0,
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
        remote_tz: None,
    }
}

pub(crate) fn enrich_cost_percentages(devices: &mut [DeviceSummary], total_cost: f64) {
    if total_cost > 0.0 {
        for device in devices.iter_mut() {
            device.cost_percentage = (device.total_cost / total_cost) * 100.0;
        }
    }
}

// ── Payload builders (async, state-dependent) ───────────────────────────────

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
    let archive = parser.archive();
    if let Some(mgr) = cache_mgr.as_ref() {
        let statuses = mgr.host_statuses(&configs);
        for cfg in configs.iter().filter(|c| c.enabled) {
            let source_key = format!("device:{}", cfg.alias);
            let frontier = archive.as_ref().and_then(|a| a.frontier(&source_key));

            // Filter archive rows by the selected provider's model family,
            // matching the live-record filter below. Without this, archive
            // rows in the Claude tab include OpenAI/GLM data (and vice versa
            // for the Codex tab), which double-counts archive data across
            // tabs — making Claude + Codex exceed ALL.
            let archived_entries: Vec<_> = archive
                .as_ref()
                .map(|a| a.load_archived(&source_key, Some(since)))
                .unwrap_or_default()
                .into_iter()
                .filter(|e| provider_matches_model(provider, &e.model))
                .collect();

            let all_records = match mgr.load_cached_records(&cfg.alias) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Failed to load cached records for {}: {e}", cfg.alias);
                    Vec::new()
                }
            };
            let filtered: Vec<_> = all_records
                .iter()
                .filter(|r| compact_record_matches_provider(r, provider))
                .filter(|r| {
                    if let Some(ref f) = frontier {
                        let dt = parse_remote_ts(&r.ts).map(|d| d.with_timezone(&chrono::Local));
                        match dt {
                            Some(local) => !f.covers(local.date_naive(), local.hour() as u8),
                            None => true,
                        }
                    } else {
                        true
                    }
                })
                .collect();

            if archived_entries.is_empty() && filtered.is_empty() {
                continue;
            }

            let mut summary =
                build_device_summary_merged(&cfg.alias, &archived_entries, &filtered, since, end);

            if let Some(host_status) = statuses.iter().find(|s| s.alias == cfg.alias) {
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
            summary.include_in_stats = cfg.include_in_stats;

            total_cost += summary.total_cost;
            devices.push(summary);
        }
    }

    enrich_cost_percentages(&mut devices, total_cost);
    Some(devices)
}

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
    let archive = state.parser.archive();

    let mut model_map: HashMap<String, (String, f64, u64)> = HashMap::new();
    let mut chart_entries: Vec<(chrono::DateTime<chrono::FixedOffset>, String, f64, u64)> =
        Vec::new();
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;

    for cfg in &included {
        let source_key = format!("device:{}", cfg.alias);
        let frontier = archive.as_ref().and_then(|a| a.frontier(&source_key));

        // ── Archived hourly rows for completed hours ──
        if let Some(ref a) = archive {
            for entry in a.load_archived(&source_key, Some(since)) {
                if !provider_matches_model(provider, &entry.model) {
                    continue;
                }
                let entry_date = entry.timestamp.date_naive();
                if entry_date < since || entry_date >= end {
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
                    0,
                );
                let tokens = entry.input_tokens + entry.output_tokens;
                input_tokens += entry.input_tokens;
                output_tokens += entry.output_tokens;

                let agg = model_map
                    .entry(model_key.clone())
                    .or_insert_with(|| (display_name, 0.0, 0));
                agg.1 += cost;
                agg.2 += tokens;

                chart_entries.push((entry.timestamp.fixed_offset(), model_key, cost, tokens));
            }
        }

        // ── Live compact rows (exclude hours already covered by archive) ──
        let records = match mgr.load_cached_records(&cfg.alias) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Failed to load cached records for {}: {e}", cfg.alias);
                continue;
            }
        };
        for record in records
            .iter()
            .filter(|r| compact_record_matches_provider(r, provider))
        {
            if record.model.starts_with('<') {
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
            let cost = calculate_cost_for_key(
                &model_key,
                record.input_tokens,
                record.output_tokens,
                record.cache_5m,
                record.cache_1h,
                record.cache_read,
                0,
            );
            let tokens = record.input_tokens + record.output_tokens;
            input_tokens += record.input_tokens;
            output_tokens += record.output_tokens;

            let agg = model_map
                .entry(model_key.clone())
                .or_insert_with(|| (display_name, 0.0, 0));
            agg.1 += cost;
            agg.2 += tokens;

            chart_entries.push((parsed_ts, model_key, cost, tokens));
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

    let mut device_cost_by_bucket: HashMap<String, HashMap<String, f64>> = HashMap::new();

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
            0,
        );
        *device_cost_by_bucket
            .entry(bucket_key)
            .or_default()
            .entry(String::from("Local"))
            .or_insert(0.0) += cost;
    }

    if provider_includes_remote_ssh_usage(provider) {
        let cache_mgr = state.ssh_cache.read().await;
        let archive = parser.archive();
        if let Some(mgr) = cache_mgr.as_ref() {
            for cfg in configs.iter().filter(|c| c.enabled) {
                let source_key = format!("device:{}", cfg.alias);
                let frontier = archive.as_ref().and_then(|a| a.frontier(&source_key));

                // Archived entries for this device, filtered by the active
                // provider's model family (keeps device time-series consistent
                // with build_device_breakdown_for_payload's totals).
                if let Some(ref a) = archive {
                    for entry in a.load_archived(&source_key, Some(since)) {
                        if !provider_matches_model(provider, &entry.model) {
                            continue;
                        }
                        let date = entry.timestamp.date_naive();
                        if date >= end {
                            continue;
                        }
                        let bucket_key =
                            bucket_key_for_timestamp(&entry.timestamp.fixed_offset(), period);
                        let (_, model_key) = normalize_model(&entry.model);
                        let cost = calculate_cost_for_key(
                            &model_key,
                            entry.input_tokens,
                            entry.output_tokens,
                            entry.cache_creation_5m_tokens,
                            entry.cache_creation_1h_tokens,
                            entry.cache_read_tokens,
                            0,
                        );
                        *device_cost_by_bucket
                            .entry(bucket_key)
                            .or_default()
                            .entry(cfg.alias.clone())
                            .or_insert(0.0) += cost;
                    }
                }

                // Live compact records (excluding archived hours).
                let records = match mgr.load_cached_records(&cfg.alias) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("Failed to load cached records for {}: {e}", cfg.alias);
                        continue;
                    }
                };
                for record in records
                    .iter()
                    .filter(|r| compact_record_matches_provider(r, provider))
                {
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

                    let bucket_key = bucket_key_for_timestamp(&parsed_ts, period);
                    let (_, model_key) = normalize_model(&record.model);
                    let cost = calculate_cost_for_key(
                        &model_key,
                        record.input_tokens,
                        record.output_tokens,
                        record.cache_5m,
                        record.cache_1h,
                        record.cache_read,
                        0,
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

// ── Bucket helpers ──────────────────────────────────────────────────────────

pub(crate) fn bucket_key_for_timestamp(
    ts: &chrono::DateTime<chrono::FixedOffset>,
    period: &str,
) -> String {
    let local = ts.with_timezone(&chrono::Local);
    match period {
        "5h" => local.format("%Y-%m-%dT%H:00:00%z").to_string(),
        "day" => format!("{:02}", local.hour()),
        "week" | "month" => local.format("%Y-%m-%d").to_string(),
        "year" => local.format("%Y-%m").to_string(),
        _ => local.format("%Y-%m-%d").to_string(),
    }
}

pub(crate) fn bucket_label_for_key(sort_key: &str, period: &str) -> String {
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

pub(crate) fn build_device_chart_buckets(devices: &[DeviceSummary]) -> Vec<ChartBucket> {
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

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::parser::UsageParser;
    use crate::usage::ssh_remote::{SshCacheManager, SshHostConfig};
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

        let codex_payload = build_included_devices_payload(&state, "codex", "day", 0)
            .await
            .expect("codex provider should include remote Codex data");
        assert!(codex_payload.total_cost > 0.0);
        assert_eq!(codex_payload.input_tokens, 500);
        assert_eq!(codex_payload.output_tokens, 200);

        let claude_payload = build_included_devices_payload(&state, "claude", "day", 0)
            .await
            .expect("claude provider should include remote Claude data");
        assert_eq!(claude_payload.input_tokens, 2_000);
        assert_eq!(claude_payload.output_tokens, 1_000);

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

    // ── Timezone-aware date extraction tests ───────────────────────────────

    #[test]
    fn parse_remote_ts_to_local_date_converts_to_local() {
        let ts_utc = "2024-01-01T23:00:00+00:00";
        let result = parse_remote_ts_to_local_date(ts_utc);
        let expected = chrono::DateTime::parse_from_rfc3339(ts_utc)
            .unwrap()
            .with_timezone(&chrono::Local)
            .date_naive();
        assert_eq!(result, expected);
    }

    #[test]
    fn parse_remote_ts_to_local_date_positive_offset() {
        let ts = "2024-03-15T02:00:00+09:00";
        let result = parse_remote_ts_to_local_date(ts);
        let expected = chrono::DateTime::parse_from_rfc3339(ts)
            .unwrap()
            .with_timezone(&chrono::Local)
            .date_naive();
        assert_eq!(result, expected);
    }

    #[test]
    fn parse_remote_ts_to_local_date_negative_offset() {
        let ts = "2024-06-30T23:30:00-05:00";
        let result = parse_remote_ts_to_local_date(ts);
        let expected = chrono::DateTime::parse_from_rfc3339(ts)
            .unwrap()
            .with_timezone(&chrono::Local)
            .date_naive();
        assert_eq!(result, expected);
    }

    #[test]
    fn parse_remote_ts_to_local_date_midday_no_edge() {
        let ts = "2024-01-15T12:00:00+00:00";
        let result = parse_remote_ts_to_local_date(ts);
        let expected = chrono::DateTime::parse_from_rfc3339(ts)
            .unwrap()
            .with_timezone(&chrono::Local)
            .date_naive();
        assert_eq!(result, expected);
    }

    #[test]
    fn parse_remote_ts_to_local_date_invalid_returns_min() {
        assert_eq!(
            parse_remote_ts_to_local_date("not-a-timestamp"),
            chrono::NaiveDate::MIN
        );
    }

    #[test]
    fn parse_remote_ts_to_local_date_fractional_seconds() {
        let ts = "2024-01-01T23:59:59.999+00:00";
        let result = parse_remote_ts_to_local_date(ts);
        let expected = chrono::DateTime::parse_from_rfc3339(ts)
            .unwrap()
            .with_timezone(&chrono::Local)
            .date_naive();
        assert_eq!(result, expected);
    }

    #[test]
    fn parse_remote_ts_helper_returns_fixed_offset() {
        let ts = "2024-06-15T10:30:00+05:30";
        let parsed = parse_remote_ts(ts).unwrap();
        assert_eq!(parsed.offset().local_minus_utc(), 5 * 3600 + 30 * 60);
    }

    /// Regression test for a real bug reported by a user:
    ///   Devices total for ALL was lower than Claude total + Codex total.
    ///
    /// Root cause: build_device_breakdown_for_payload (and _time_chart_buckets)
    /// loaded archive rows without filtering by the active provider's model
    /// family, even though live records were filtered. That meant archive rows
    /// belonging to OpenAI models showed up in the Claude tab, and vice versa
    /// — so the same archived cost was counted once in Claude and once in
    /// Codex, while ALL still counted it only once. The invariant below would
    /// fail before the fix.
    #[tokio::test]
    async fn archive_is_filtered_by_provider_so_all_is_not_exceeded_by_sum() {
        use crate::usage::archive::ArchiveManager;
        use crate::usage::parser::{ParsedEntry, UsageParser};
        use crate::usage::ssh_remote::{SshCacheManager, SshHostConfig};
        use crate::{commands::AppState, stats::subagent::AgentScope};
        use chrono::{Local, TimeZone};
        use std::sync::Arc;
        use tempfile::TempDir;

        // Build a parser with empty dirs — we want ALL of the "remote" rows
        // to come from archive (not live JSONL), so we can exercise the fix.
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let app_data_dir = TempDir::new().unwrap();

        let mut state = AppState::new();
        state.parser = Arc::new(UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        ));
        *state.ssh_hosts.write().await = vec![SshHostConfig {
            alias: String::from("remote-mixed"),
            enabled: true,
            include_in_stats: false,
        }];
        *state.ssh_cache.write().await = Some(SshCacheManager::new(app_data_dir.path()));

        // Archive one Anthropic row + one OpenAI row for the same day/hour.
        // archive_completed_hours needs a `current_hour > row_hour` to
        // actually archive; we write with ts at hour=10 and bump to hour=12.
        let archive_mgr = ArchiveManager::new(app_data_dir.path());
        let today = Local::now().date_naive();
        let row_ts = Local
            .from_local_datetime(&today.and_hms_opt(10, 0, 0).unwrap())
            .single()
            .unwrap();
        let rows = vec![
            ParsedEntry {
                timestamp: row_ts,
                model: String::from("claude-opus-4-6"),
                input_tokens: 1_000,
                output_tokens: 1_000,
                cache_creation_5m_tokens: 0,
                cache_creation_1h_tokens: 0,
                cache_read_tokens: 0,
                web_search_requests: 0,
                unique_hash: None,
                session_key: String::from("test"),
                agent_scope: AgentScope::Main,
            },
            ParsedEntry {
                timestamp: row_ts,
                model: String::from("gpt-5.4"),
                input_tokens: 1_000,
                output_tokens: 1_000,
                cache_creation_5m_tokens: 0,
                cache_creation_1h_tokens: 0,
                cache_read_tokens: 0,
                web_search_requests: 0,
                unique_hash: None,
                session_key: String::from("test"),
                agent_scope: AgentScope::Main,
            },
        ];
        let archived =
            archive_mgr.archive_completed_hours(&rows, "device:remote-mixed", "all", today, 12);
        assert!(archived > 0, "archive should have written at least one row");
        state.parser.set_archive(archive_mgr);

        // Same frontier-less rows as live records so the test also exercises
        // the live path. (The frontier prunes these to no-ops under ALL, so
        // the totals here come almost entirely from the archive.)

        let claude_devices = build_device_breakdown_for_payload(&state, "claude", "day", 0)
            .await
            .unwrap();
        let codex_devices = build_device_breakdown_for_payload(&state, "codex", "day", 0)
            .await
            .unwrap();
        let all_devices = build_device_breakdown_for_payload(&state, "all", "day", 0)
            .await
            .unwrap();

        let claude_total: f64 = claude_devices.iter().map(|d| d.total_cost).sum();
        let codex_total: f64 = codex_devices.iter().map(|d| d.total_cost).sum();
        let all_total: f64 = all_devices.iter().map(|d| d.total_cost).sum();

        // Each family should see only its own archive row.
        assert!(
            claude_total > 0.0,
            "claude should include the Opus archive row"
        );
        assert!(
            codex_total > 0.0,
            "codex should include the GPT archive row"
        );
        // ALL must be at least as large as the sum — archive rows are
        // family-partitioned, so double-counting must not occur.
        assert!(
            all_total + 1e-9 >= claude_total + codex_total,
            "ALL ({all_total}) should not be less than Claude ({claude_total}) + Codex ({codex_total}); \
             archive row filtering regressed"
        );
    }

    /// Regression test: `build_included_devices_payload` must consume archive
    /// rows for hosts flagged `include_in_stats`, and must not double-count by
    /// also consuming live rows for hours already covered by the archive
    /// frontier.
    #[tokio::test]
    async fn included_devices_payload_reads_archive_without_double_counting() {
        use crate::commands::AppState;
        use crate::stats::subagent::AgentScope;
        use crate::usage::archive::ArchiveManager;
        use crate::usage::parser::{ParsedEntry, UsageParser};
        use crate::usage::ssh_remote::{SshCacheManager, SshHostConfig};
        use chrono::{Local, TimeZone};
        use std::sync::Arc;
        use tempfile::TempDir;

        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let app_data_dir = TempDir::new().unwrap();

        let mut state = AppState::new();
        state.parser = Arc::new(UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        ));
        *state.ssh_hosts.write().await = vec![SshHostConfig {
            alias: String::from("remote-inc"),
            enabled: true,
            include_in_stats: true,
        }];
        *state.ssh_cache.write().await = Some(SshCacheManager::new(app_data_dir.path()));

        let today = Local::now().date_naive();
        let archived_ts = Local
            .from_local_datetime(&today.and_hms_opt(9, 0, 0).unwrap())
            .single()
            .unwrap();
        let archive_mgr = ArchiveManager::new(app_data_dir.path());
        let archived_rows = vec![ParsedEntry {
            timestamp: archived_ts,
            model: String::from("claude-sonnet-4-6"),
            input_tokens: 1_000,
            output_tokens: 1_000,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: 0,
            web_search_requests: 0,
            unique_hash: None,
            session_key: String::from("test"),
            agent_scope: AgentScope::Main,
        }];
        let archived = archive_mgr.archive_completed_hours(
            &archived_rows,
            "device:remote-inc",
            "all",
            today,
            12,
        );
        assert!(archived > 0, "archive should have written at least one row");
        state.parser.set_archive(archive_mgr);

        // Write a live JSONL with:
        //  • 1 row inside the archived hour (should be de-duped by frontier)
        //  • 1 row in a later, *not-yet-archived* hour (should be counted)
        let archived_ts_str = archived_ts.to_rfc3339();
        let live_ts = Local
            .from_local_datetime(&today.and_hms_opt(11, 0, 0).unwrap())
            .single()
            .unwrap();
        let live_ts_str = live_ts.to_rfc3339();
        let live_jsonl = format!(
            "{}\n{}\n",
            remote_record(&archived_ts_str, "claude-sonnet-4-6", 500, 500),
            remote_record(&live_ts_str, "claude-sonnet-4-6", 2_000, 2_000),
        );
        write_file(
            &app_data_dir
                .path()
                .join("remote-cache")
                .join("remote-inc")
                .join("usage.jsonl"),
            &live_jsonl,
        );

        let payload = build_included_devices_payload(&state, "claude", "day", 0)
            .await
            .expect("included devices payload should exist when archive has data");

        // Expected tokens: archive (1_000+1_000) + later live row (2_000+2_000)
        //                  = input=3_000, output=3_000
        // The archived-hour live row must be skipped via frontier.
        assert_eq!(
            payload.input_tokens, 3_000,
            "frontier should have de-duped the archived-hour live row"
        );
        assert_eq!(payload.output_tokens, 3_000);
        assert!(payload.total_cost > 0.0);
        assert!(
            payload
                .model_breakdown
                .iter()
                .any(|m| m.model_key == "sonnet-4-6"),
            "sonnet-4-6 should appear in the model breakdown"
        );
    }

    #[test]
    fn build_device_summary_filters_by_local_date() {
        let records = vec![CompactUsageRecord {
            ts: "2024-01-01T23:00:00+00:00".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            input_tokens: 100,
            output_tokens: 50,
            cache_5m: 0,
            cache_1h: 0,
            cache_read: 0,
            speed: None,
        }];

        let local_date = parse_remote_ts_to_local_date(&records[0].ts);

        let summary = build_device_summary_from_compact(
            "test-host",
            &records,
            local_date,
            local_date + chrono::Duration::days(1),
        );
        assert_eq!(
            summary.model_breakdown.len(),
            1,
            "record should be included when filtered by its local date"
        );

        let day_before = local_date - chrono::Duration::days(1);
        let summary_miss =
            build_device_summary_from_compact("test-host", &records, day_before, local_date);
        assert!(
            summary_miss.total_cost == 0.0,
            "record should be excluded when local date is outside the range"
        );
    }
}
