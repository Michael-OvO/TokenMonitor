use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};

use crate::commands::period::{resolve_period_bounds_for_provider, PeriodBounds};
use crate::commands::AppState;
use crate::models::{ChartBucket, ChartSegment, DeviceModelSummary, DeviceSummary};
use crate::usage::archive::{ArchiveFrontier, ArchiveManager};
use crate::usage::integrations::{
    remote_record_matches_provider, UsageIntegrationId, UsageIntegrationSelection,
};
use crate::usage::ssh_remote::{CompactUsageRecord, SshCacheManager, SshHostConfig};
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) fn parse_remote_ts(ts: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .or_else(|_| chrono::DateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .ok()
}

/// A host's SSH records by time, each timestamp parsed and converted to local
/// time once, when the records memo loads them: a conversion is a time-zone
/// lookup (a system call on Windows), and every view compute filters every
/// record by its local time several times over.
#[derive(Default)]
pub struct RecordIndex {
    records: Arc<Vec<CompactUsageRecord>>,
    /// (local time, position in `records`) by time. `None`, a timestamp that
    /// does not parse, sorts first.
    rows: Vec<(Option<DateTime<Local>>, usize)>,
    /// [`local_zone_stamp`] at the conversion.
    pub(crate) zone: (i32, i32),
}

impl RecordIndex {
    pub(crate) fn new(records: Arc<Vec<CompactUsageRecord>>) -> Self {
        let mut rows: Vec<_> = records
            .iter()
            .enumerate()
            .map(|(i, r)| (parse_remote_ts(&r.ts).map(|t| t.with_timezone(&Local)), i))
            .collect();
        rows.sort_by_key(|(local, _)| *local);
        Self {
            records,
            rows,
            zone: local_zone_stamp(),
        }
    }

    /// Every record with its local time, `None` when that does not parse.
    pub(crate) fn all(
        &self,
    ) -> impl Iterator<Item = (Option<&DateTime<Local>>, &CompactUsageRecord)> {
        self.rows
            .iter()
            .map(|(local, i)| (local.as_ref(), &self.records[*i]))
    }

    /// The records inside `bounds`, oldest first.
    pub(crate) fn in_bounds(
        &self,
        bounds: &PeriodBounds,
    ) -> impl Iterator<Item = (&DateTime<Local>, &CompactUsageRecord)> {
        let before = |at: DateTime<Local>| {
            self.rows
                .partition_point(|(local, _)| local.is_none_or(|t| t < at))
        };
        let (start, end) = (before(bounds.range_start), before(bounds.range_end));
        self.rows[start..end.max(start)]
            .iter()
            .filter_map(|(local, i)| Some((local.as_ref()?, &self.records[*i])))
    }
}

/// Local's UTC offsets in January and July of this year: they move with the
/// time zone, which invalidates every local time converted in the old one.
pub(crate) fn local_zone_stamp() -> (i32, i32) {
    let year = chrono::Utc::now().year();
    let offset = |month| {
        chrono::NaiveDate::from_ymd_opt(year, month, 1)
            .and_then(|day| day.and_hms_opt(0, 0, 0))
            .map_or(0, |at| {
                Local.offset_from_utc_datetime(&at).local_minus_utc()
            })
    };
    (offset(1), offset(7))
}

pub(crate) fn archived_hour_keys(
    entries: &[crate::usage::parser::ParsedEntry],
) -> HashSet<(chrono::NaiveDate, u8)> {
    entries
        .iter()
        .map(|e| (e.timestamp.date_naive(), e.timestamp.hour() as u8))
        .collect()
}

/// Keep live SSH rows for hours the frontier covers but archive has no rows.
/// `local` is the row's local time, `None` when its timestamp does not parse.
pub(crate) fn ssh_live_hour_open(
    frontier: Option<&ArchiveFrontier>,
    archived_hours: &HashSet<(chrono::NaiveDate, u8)>,
    local: Option<&DateTime<Local>>,
) -> bool {
    let (Some(f), Some(local)) = (frontier, local) else {
        return true;
    };
    let hour = (local.date_naive(), local.hour() as u8);
    !(f.covers(hour.0, hour.1) && archived_hours.contains(&hour))
}

/// The live records a view counts: inside `bounds`, of `provider`'s model
/// family, and in an hour the archive holds no rows for.
pub(crate) fn live_records_in<'a>(
    index: &'a RecordIndex,
    provider: &'a str,
    bounds: &PeriodBounds,
    frontier: Option<&'a ArchiveFrontier>,
    archived_hours: &'a HashSet<(chrono::NaiveDate, u8)>,
) -> impl Iterator<Item = (&'a DateTime<Local>, &'a CompactUsageRecord)> {
    index.in_bounds(bounds).filter(move |(local, record)| {
        compact_record_matches_provider(record, provider)
            && ssh_live_hour_open(frontier, archived_hours, Some(local))
    })
}

/// A configured host's indexed live records; none for a file-imported peer,
/// which has no SSH cache, or when they cannot be read.
pub(crate) fn live_index(mgr: Option<&SshCacheManager>, dev: &AggDevice) -> Arc<RecordIndex> {
    let Some(mgr) = mgr.filter(|_| dev.configured) else {
        return Arc::default();
    };
    mgr.load_record_index(&dev.alias).unwrap_or_else(|e| {
        tracing::warn!("Failed to load cached records for {}: {e}", dev.alias);
        Arc::default()
    })
}

#[cfg(test)]
fn parse_remote_ts_to_local_date(ts: &str) -> chrono::NaiveDate {
    parse_remote_ts(ts)
        .map(|dt| dt.with_timezone(&chrono::Local).date_naive())
        .unwrap_or(chrono::NaiveDate::MIN)
}

/// Remote SSH records only ever carry Claude and Codex usage, so a scope
/// includes them exactly when it contains one of those integrations.
pub(crate) fn provider_includes_remote_ssh_usage(provider: &str) -> bool {
    match UsageIntegrationSelection::parse(provider) {
        Some(selection) => {
            selection.contains(UsageIntegrationId::Claude)
                || selection.contains(UsageIntegrationId::Codex)
        }
        None => false,
    }
}

pub(crate) fn compact_record_matches_provider(record: &CompactUsageRecord, provider: &str) -> bool {
    remote_record_matches_provider(provider, &record.model)
}

#[cfg(test)]
fn remote_ts_in_bounds(bounds: &PeriodBounds, ts: &str) -> bool {
    parse_remote_ts(ts)
        .map(|dt| bounds.contains_timestamp(dt.with_timezone(&chrono::Local)))
        .unwrap_or(false)
}

async fn period_bounds_for(
    state: &AppState,
    provider: &str,
    period: &str,
    offset: i32,
) -> Option<PeriodBounds> {
    let cached = state.cached_rate_limits.read().await;
    resolve_period_bounds_for_provider(period, offset, provider, cached.as_ref()).ok()
}

/// A device to aggregate in the dashboard: either a configured+enabled SSH host
/// (live SSH cache + status) or a device present only in the archive — e.g.
/// imported from a peer machine's synced export file. Archive-only devices have
/// no SSH connection; they default to included-in-stats unless a saved include
/// flag says otherwise.
pub(crate) struct AggDevice {
    pub alias: String,
    pub include_in_stats: bool,
    /// True for SSH hosts (read live cache + status); false for file-imported peers.
    pub configured: bool,
}

/// Devices to aggregate = active SSH hosts ∪ `device:*` sources present in the
/// archive (deduped by alias; a configured host wins). A DISABLED configured
/// host stays hidden — its alias is suppressed from the archive set too, so a
/// disabled host can't reappear as an "archive-only" device.
pub(crate) fn enumerate_agg_devices(
    configs: &[SshHostConfig],
    archive: Option<&ArchiveManager>,
    remote_include_flags: &std::collections::HashMap<String, bool>,
) -> Vec<AggDevice> {
    use std::collections::HashSet;
    let configured_aliases: HashSet<&str> = configs.iter().map(|c| c.alias.as_str()).collect();
    let mut out: Vec<AggDevice> = configs
        .iter()
        .filter(|c| c.enabled && c.include_in_stats)
        .map(|c| AggDevice {
            alias: c.alias.clone(),
            include_in_stats: c.include_in_stats,
            configured: true,
        })
        .collect();
    if let Some(a) = archive {
        for src in a.list_sources() {
            if let Some(alias) = src.strip_prefix("device:") {
                if !configured_aliases.contains(alias) {
                    out.push(AggDevice {
                        alias: alias.to_string(),
                        include_in_stats: remote_include_flags.get(alias).copied().unwrap_or(true),
                        configured: false,
                    });
                }
            }
        }
    }
    out
}

// ── Summary builders ────────────────────────────────────────────────────────

pub(crate) fn build_device_summary_from_parsed(
    device_name: &str,
    entries: &[crate::usage::parser::ParsedEntry],
    bounds: &PeriodBounds,
) -> DeviceSummary {
    use crate::models::normalize_model;
    use crate::usage::pricing::{
        calculate_cost_for_key, pricing_available_for_key, provider_multiplier,
    };
    use std::collections::HashMap;

    let mut model_map: HashMap<String, (String, f64, u64, bool)> = HashMap::new();

    for entry in entries {
        if !bounds.contains_timestamp(entry.timestamp) {
            continue;
        }

        let (display_name, model_key) = normalize_model(&entry.model);
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
        let tokens = entry.input_tokens
            + entry.output_tokens
            + entry.cache_creation_5m_tokens
            + entry.cache_creation_1h_tokens
            + entry.cache_read_tokens;

        let agg = model_map
            .entry(model_key)
            .or_insert_with(|| (display_name, 0.0, 0, true));
        agg.1 += cost;
        agg.2 += tokens;
        agg.3 &= pricing_available;
    }

    finish_device_summary(device_name, model_map)
}

#[cfg(test)]
pub(crate) fn build_device_summary_from_compact(
    device_name: &str,
    records: &[CompactUsageRecord],
    bounds: &PeriodBounds,
) -> DeviceSummary {
    use crate::models::normalize_model;
    use crate::usage::pricing::{
        calculate_cost_for_key, pricing_available_for_key, provider_multiplier,
    };
    use std::collections::HashMap;

    let mut model_map: HashMap<String, (String, f64, u64, bool)> = HashMap::new();

    for record in records {
        if record.model.starts_with('<') {
            continue;
        }

        if !remote_ts_in_bounds(bounds, &record.ts) {
            continue;
        }

        let (display_name, model_key) = normalize_model(&record.model);
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
        let tokens = record.input_tokens
            + record.output_tokens
            + record.cache_5m
            + record.cache_1h
            + record.cache_read;

        let agg = model_map
            .entry(model_key)
            .or_insert_with(|| (display_name, 0.0, 0, true));
        agg.1 += cost;
        agg.2 += tokens;
        agg.3 &= pricing_available;
    }

    finish_device_summary(device_name, model_map)
}

/// `live_records` are the ones inside `bounds` (see [`live_records_in`]).
pub(crate) fn build_device_summary_merged(
    device_name: &str,
    archived_entries: &[crate::usage::parser::ParsedEntry],
    live_records: &[&CompactUsageRecord],
    bounds: &PeriodBounds,
) -> DeviceSummary {
    use crate::models::normalize_model;
    use crate::usage::pricing::{
        calculate_cost_for_key, pricing_available_for_key, provider_multiplier,
    };
    use std::collections::HashMap;

    let mut model_map: HashMap<String, (String, f64, u64, bool)> = HashMap::new();

    for entry in archived_entries {
        if !bounds.contains_timestamp(entry.timestamp) {
            continue;
        }
        let (display_name, model_key) = normalize_model(&entry.model);
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
        let tokens = entry.input_tokens
            + entry.output_tokens
            + entry.cache_creation_5m_tokens
            + entry.cache_creation_1h_tokens
            + entry.cache_read_tokens;
        let agg = model_map
            .entry(model_key)
            .or_insert_with(|| (display_name, 0.0, 0, true));
        agg.1 += cost;
        agg.2 += tokens;
        agg.3 &= pricing_available;
    }

    for record in live_records {
        if record.model.starts_with('<') {
            continue;
        }
        let (display_name, model_key) = normalize_model(&record.model);
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
        let tokens = record.input_tokens
            + record.output_tokens
            + record.cache_5m
            + record.cache_1h
            + record.cache_read;
        let agg = model_map
            .entry(model_key)
            .or_insert_with(|| (display_name, 0.0, 0, true));
        agg.1 += cost;
        agg.2 += tokens;
        agg.3 &= pricing_available;
    }

    finish_device_summary(device_name, model_map)
}

fn finish_device_summary(
    device_name: &str,
    model_map: std::collections::HashMap<String, (String, f64, u64, bool)>,
) -> DeviceSummary {
    let mut model_breakdown: Vec<DeviceModelSummary> = model_map
        .into_iter()
        .map(
            // The device name is NOT appended here: these rows always render
            // nested under their device (the device card header / per-device
            // chart bucket), so a "Model -- Device" suffix is redundant noise.
            |(model_key, (display_name, cost, tokens, pricing_available))| DeviceModelSummary {
                display_name,
                model_key,
                cost,
                tokens,
                pricing_available,
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
    let configs = state.ssh_hosts.read().await;
    let bounds = period_bounds_for(state, provider, period, offset).await?;
    let since = bounds.start;
    let parser = &state.parser;
    let archive = parser.archive();

    // Devices to show = enabled SSH hosts ∪ archive (file-imported peer) devices.
    // None when there are no remote devices at all (preserves the prior behavior
    // of suppressing the per-device breakdown when only Local exists).
    let agg_devices = {
        let remote_include_flags = state.remote_device_include_flags.read().await;
        enumerate_agg_devices(&configs, archive.as_ref(), &remote_include_flags)
    };
    if agg_devices.is_empty() {
        return None;
    }

    let loaded = parser.load_entries_cached(provider, Some(since));
    let local_entries = &loaded.entries;
    let mut local_summary = build_device_summary_from_parsed("Local", local_entries, &bounds);
    local_summary.is_local = true;
    local_summary.status = String::from("online");

    let mut devices = vec![local_summary];
    let mut total_cost = devices[0].total_cost;

    if !provider_includes_remote_ssh_usage(provider) {
        enrich_cost_percentages(&mut devices, total_cost);
        return Some(devices);
    }

    let cache_mgr = state.ssh_cache.read().await;
    let mgr = cache_mgr.as_ref();
    let statuses = mgr.map(|m| m.host_statuses(&configs)).unwrap_or_default();
    for dev in &agg_devices {
        let source_key = format!("device:{}", dev.alias);
        let frontier = archive.as_ref().and_then(|a| a.frontier(&source_key));

        // Filter archive rows by the selected provider's model family, matching
        // the live-record filter below. Without this, archive rows in the Claude
        // tab include OpenAI/GLM data (and vice versa), double-counting across
        // tabs — making Claude + Codex exceed ALL.
        let archived_entries: Vec<_> = archive
            .as_ref()
            .map(|a| a.load_archived(&source_key, Some(since)))
            .unwrap_or_default()
            .into_iter()
            .filter(|e| remote_record_matches_provider(provider, &e.model))
            .collect();
        let archived_hours = archived_hour_keys(&archived_entries);

        // Live SSH-cache rows only for configured hosts; file-imported peers have
        // no SSH cache and contribute from archived data alone.
        let index = live_index(mgr, dev);

        // Skip devices with no data matching the selected provider (provider
        // scoping — a Claude-only device doesn't show as an empty Codex row).
        if archived_entries.is_empty()
            && !index.all().any(|(local, r)| {
                compact_record_matches_provider(r, provider)
                    && ssh_live_hour_open(frontier.as_ref(), &archived_hours, local)
            })
        {
            continue;
        }

        let live: Vec<&CompactUsageRecord> = live_records_in(
            &index,
            provider,
            &bounds,
            frontier.as_ref(),
            &archived_hours,
        )
        .map(|(_, record)| record)
        .collect();
        let mut summary =
            build_device_summary_merged(&dev.alias, &archived_entries, &live, &bounds);

        if dev.configured {
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

    enrich_cost_percentages(&mut devices, total_cost);
    Some(devices)
}

pub(crate) async fn build_included_devices_payload(
    state: &AppState,
    provider: &str,
    period: &str,
    offset: i32,
) -> Option<crate::models::UsagePayload> {
    use crate::models::{ModelSummary, UsagePayload, UsageSource};
    use crate::usage::pricing::{
        calculate_cost_for_key, pricing_available_for_key, provider_multiplier,
    };
    use std::collections::HashMap;

    if !provider_includes_remote_ssh_usage(provider) {
        return None;
    }

    let configs = state.ssh_hosts.read().await;
    let archive = state.parser.archive();
    // Devices = enabled SSH hosts ∪ archive (file-imported peer) devices.
    let agg_devices = {
        let remote_include_flags = state.remote_device_include_flags.read().await;
        enumerate_agg_devices(&configs, archive.as_ref(), &remote_include_flags)
    };
    if agg_devices.is_empty() {
        return None;
    }

    let bounds = period_bounds_for(state, provider, period, offset).await?;
    let since = bounds.start;
    let cache_mgr = state.ssh_cache.read().await;
    let mgr = cache_mgr.as_ref();

    let mut model_map: HashMap<String, (String, f64, u64, bool)> = HashMap::new();
    let mut chart_entries: Vec<(DateTime<Local>, String, f64, u64, bool)> = Vec::new();
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;
    let mut cache_read_tokens = 0_u64;
    let mut cache_write_5m_tokens = 0_u64;
    let mut cache_write_1h_tokens = 0_u64;

    for dev in &agg_devices {
        // Only devices flagged "include in stats" contribute to the MAIN total.
        // This is the payload merged into the dashboard total; a device the user
        // excluded (or that is double-counted elsewhere) must not inflate it.
        // The per-device breakdown still lists excluded devices separately.
        if !dev.include_in_stats {
            continue;
        }
        let source_key = format!("device:{}", dev.alias);
        let frontier = archive.as_ref().and_then(|a| a.frontier(&source_key));
        let mut archived_hours = HashSet::new();

        // ── Archived hourly rows for completed hours ──
        if let Some(ref a) = archive {
            for entry in a.load_archived(&source_key, Some(since)) {
                if !remote_record_matches_provider(provider, &entry.model) {
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
                // Same definition as the local payload (parser::entry_total_tokens):
                // cache counts, otherwise a peer's cache-heavy usage vanishes
                // from the Tokens card while still being priced into Cost.
                let tokens = entry.input_tokens
                    + entry.output_tokens
                    + entry.cache_creation_5m_tokens
                    + entry.cache_creation_1h_tokens
                    + entry.cache_read_tokens;
                input_tokens += entry.input_tokens;
                output_tokens += entry.output_tokens;
                cache_write_5m_tokens += entry.cache_creation_5m_tokens;
                cache_write_1h_tokens += entry.cache_creation_1h_tokens;
                cache_read_tokens += entry.cache_read_tokens;

                let agg = model_map
                    .entry(model_key.clone())
                    .or_insert_with(|| (display_name, 0.0, 0, true));
                agg.1 += cost;
                agg.2 += tokens;
                agg.3 &= pricing_available;

                chart_entries.push((entry.timestamp, model_key, cost, tokens, pricing_available));
            }
        }

        // ── Live compact rows for configured hosts (peers have no SSH cache) ──
        let index = live_index(mgr, dev);
        for (local, record) in live_records_in(
            &index,
            provider,
            &bounds,
            frontier.as_ref(),
            &archived_hours,
        ) {
            if record.model.starts_with('<') {
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
            let tokens = record.input_tokens
                + record.output_tokens
                + record.cache_5m
                + record.cache_1h
                + record.cache_read;
            input_tokens += record.input_tokens;
            output_tokens += record.output_tokens;
            cache_write_5m_tokens += record.cache_5m;
            cache_write_1h_tokens += record.cache_1h;
            cache_read_tokens += record.cache_read;

            let agg = model_map
                .entry(model_key.clone())
                .or_insert_with(|| (display_name, 0.0, 0, true));
            agg.1 += cost;
            agg.2 += tokens;
            agg.3 &= pricing_available;

            chart_entries.push((*local, model_key, cost, tokens, pricing_available));
        }
    }

    if model_map.is_empty() {
        return None;
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

    use std::collections::BTreeMap;

    type ModelBucket = BTreeMap<String, (String, f64, u64, bool)>;
    let mut bucket_map: BTreeMap<String, ModelBucket> = BTreeMap::new();
    for (ts, model_key, cost, tokens, pricing_available) in &chart_entries {
        let bkey = bucket_key_for_local(ts, period);
        let model_entry = bucket_map.entry(bkey).or_default();
        let (_, model_cost, model_tokens, model_pricing_available) =
            model_entry.entry(model_key.clone()).or_insert_with(|| {
                let (display, _) = crate::models::normalize_model(model_key);
                (display, 0.0, 0, true)
            });
        *model_cost += cost;
        *model_tokens += tokens;
        *model_pricing_available &= *pricing_available;
    }

    let chart_buckets: Vec<ChartBucket> = bucket_map
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
        cache_read_tokens,
        cache_write_5m_tokens,
        cache_write_1h_tokens,
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
    use crate::models::normalize_model;
    use crate::usage::pricing::{calculate_cost_for_key, provider_multiplier};
    use std::collections::HashMap;

    let configs = state.ssh_hosts.read().await;
    let parser = &state.parser;
    let archive = parser.archive();
    // Devices = enabled SSH hosts ∪ archive (file-imported peer) devices.
    let agg_devices = {
        let remote_include_flags = state.remote_device_include_flags.read().await;
        enumerate_agg_devices(&configs, archive.as_ref(), &remote_include_flags)
    };
    if agg_devices.is_empty() {
        return None;
    }

    let bounds = period_bounds_for(state, provider, period, offset).await?;
    let since = bounds.start;

    let mut device_cost_by_bucket: HashMap<String, HashMap<String, f64>> = HashMap::new();

    let loaded = parser.load_entries_cached(provider, Some(since));
    let local_entries = &loaded.entries;
    for entry in local_entries {
        if !bounds.contains_timestamp(entry.timestamp) {
            continue;
        }
        let bucket_key = bucket_key_for_local(&entry.timestamp, period);
        let (_, model_key) = normalize_model(&entry.model);
        let cost = calculate_cost_for_key(
            &model_key,
            entry.input_tokens,
            entry.output_tokens,
            entry.cache_creation_5m_tokens,
            entry.cache_creation_1h_tokens,
            entry.cache_read_tokens,
            0,
        ) * provider_multiplier(&entry.model);
        *device_cost_by_bucket
            .entry(bucket_key)
            .or_default()
            .entry(String::from("Local"))
            .or_insert(0.0) += cost;
    }

    if provider_includes_remote_ssh_usage(provider) {
        let cache_mgr = state.ssh_cache.read().await;
        let mgr = cache_mgr.as_ref();
        for dev in &agg_devices {
            // Mirror the total: only devices included in stats contribute to the
            // device cost chart, so the chart matches the headline total.
            if !dev.include_in_stats {
                continue;
            }
            let source_key = format!("device:{}", dev.alias);
            let frontier = archive.as_ref().and_then(|a| a.frontier(&source_key));
            let mut archived_hours = HashSet::new();

            // Archived entries for this device, filtered by the active provider's
            // model family. Read for EVERY device (configured or file-imported)
            // regardless of the SSH cache manager, so peers still chart.
            if let Some(ref a) = archive {
                for entry in a.load_archived(&source_key, Some(since)) {
                    if !remote_record_matches_provider(provider, &entry.model) {
                        continue;
                    }
                    archived_hours
                        .insert((entry.timestamp.date_naive(), entry.timestamp.hour() as u8));
                    if !bounds.contains_timestamp(entry.timestamp) {
                        continue;
                    }
                    let bucket_key = bucket_key_for_local(&entry.timestamp, period);
                    let (_, model_key) = normalize_model(&entry.model);
                    let cost = calculate_cost_for_key(
                        &model_key,
                        entry.input_tokens,
                        entry.output_tokens,
                        entry.cache_creation_5m_tokens,
                        entry.cache_creation_1h_tokens,
                        entry.cache_read_tokens,
                        0,
                    ) * provider_multiplier(&entry.model);
                    *device_cost_by_bucket
                        .entry(bucket_key)
                        .or_default()
                        .entry(dev.alias.clone())
                        .or_insert(0.0) += cost;
                }
            }

            // Live compact records for configured hosts (peers have none).
            let index = live_index(mgr, dev);
            for (local, record) in live_records_in(
                &index,
                provider,
                &bounds,
                frontier.as_ref(),
                &archived_hours,
            ) {
                let bucket_key = bucket_key_for_local(local, period);
                let (_, model_key) = normalize_model(&record.model);
                let cost = calculate_cost_for_key(
                    &model_key,
                    record.input_tokens,
                    record.output_tokens,
                    record.cache_5m,
                    record.cache_1h,
                    record.cache_read,
                    0,
                ) * provider_multiplier(&record.model);
                *device_cost_by_bucket
                    .entry(bucket_key)
                    .or_default()
                    .entry(dev.alias.clone())
                    .or_insert(0.0) += cost;
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
                    pricing_available: true,
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

pub(crate) fn bucket_key_for_local(local: &DateTime<Local>, period: &str) -> String {
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
                    pricing_available: m.pricing_available,
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

    #[test]
    fn enumerate_agg_devices_unions_configs_and_archive() {
        let configs = vec![
            SshHostConfig {
                alias: "A".into(),
                enabled: true,
                include_in_stats: true,
            },
            SshHostConfig {
                alias: "B".into(),
                enabled: false,
                include_in_stats: true,
            },
            SshHostConfig {
                alias: "C".into(),
                enabled: true,
                include_in_stats: false,
            },
        ];

        // Config-only (archive=None): active shown, disabled/hidden hosts omitted.
        let flags = std::collections::HashMap::new();
        let only_cfg = enumerate_agg_devices(&configs, None, &flags);
        let aliases: Vec<&str> = only_cfg.iter().map(|d| d.alias.as_str()).collect();
        assert_eq!(aliases, vec!["A"]);
        assert!(only_cfg.iter().all(|d| d.configured));

        // With archive holding device:Peer (new) and device:B (a DISABLED config):
        let tmp = TempDir::new().unwrap();
        let archive = crate::usage::archive::ArchiveManager::new(tmp.path());
        let rec = crate::usage::archive::ArchivedHourly {
            d: "2020-01-01".into(),
            h: 12,
            mk: "opus-4-6".into(),
            mn: "Opus 4.6".into(),
            raw_model: None,
            input_tokens: 1,
            out: 2,
            c5: 0,
            c1: 0,
            cr: 0,
            ws: 0,
            p: "all".into(),
        };
        let now = Local::now();
        archive.import_source(
            "device:Peer",
            std::slice::from_ref(&rec),
            now.date_naive(),
            now.hour() as u8,
        );
        archive.import_source(
            "device:B",
            std::slice::from_ref(&rec),
            now.date_naive(),
            now.hour() as u8,
        );

        let mut flags = std::collections::HashMap::new();
        flags.insert(String::from("Peer"), false);
        let merged = enumerate_agg_devices(&configs, Some(&archive), &flags);
        let names: Vec<&str> = merged.iter().map(|d| d.alias.as_str()).collect();
        assert!(names.contains(&"A"));
        assert!(
            !names.contains(&"C"),
            "an include-disabled config must stay hidden"
        );
        assert!(names.contains(&"Peer"), "file-imported peer should surface");
        assert!(
            !names.contains(&"B"),
            "a disabled config must not reappear via the archive"
        );
        let peer = merged.iter().find(|d| d.alias == "Peer").unwrap();
        assert!(!peer.configured);
        assert!(!peer.include_in_stats);
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
            include_in_stats: true,
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
        archive_mgr.import_source(
            "device:remote-inc",
            &[crate::usage::archive::ArchivedHourly {
                d: today.to_string(),
                h: 8,
                mk: String::from("opus"),
                mn: String::from("Opus"),
                raw_model: None,
                input_tokens: 10,
                out: 10,
                c5: 0,
                c1: 0,
                cr: 0,
                ws: 0,
                p: String::from("claude"),
            }],
            today,
            12,
        );
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

        // Expected tokens: legacy Opus (10+10) + archive (1_000+1_000)
        //                  + later live row (2_000+2_000) = 3_010 each.
        // The archived-hour live row must be skipped via frontier.
        assert_eq!(
            payload.input_tokens, 3_010,
            "frontier should have de-duped the archived-hour live row"
        );
        assert_eq!(payload.output_tokens, 3_010);
        assert!(payload.total_cost > 0.0);
        assert!(
            payload
                .model_breakdown
                .iter()
                .any(|m| m.model_key == "sonnet-4-6"),
            "sonnet-4-6 should appear in the model breakdown"
        );
        assert!(
            payload
                .model_breakdown
                .iter()
                .any(|m| m.model_key == "opus-5"),
            "legacy post-release Opus archive rows should appear as opus-5"
        );
        assert!(payload
            .model_breakdown
            .iter()
            .all(|m| m.model_key != "opus"));
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
            dedupe_key: None,
        }];

        let local_date = parse_remote_ts_to_local_date(&records[0].ts);
        let noon = local_date
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .single()
            .unwrap();
        let hit = crate::commands::period::resolve_period_bounds_at("day", 0, noon, None).unwrap();
        let miss =
            crate::commands::period::resolve_period_bounds_at("day", -1, noon, None).unwrap();

        let summary = build_device_summary_from_compact("test-host", &records, &hit);
        assert_eq!(
            summary.model_breakdown.len(),
            1,
            "record should be included when filtered by its local date"
        );

        let summary_miss = build_device_summary_from_compact("test-host", &records, &miss);
        assert!(
            summary_miss.total_cost == 0.0,
            "record should be excluded when local date is outside the range"
        );
    }

    /// The index keeps exactly the live records the per-record parse did:
    /// the frontier/archived-hour dedupe, provider matching and bounds, and
    /// the breakdown's has-data check over every record.
    #[test]
    fn indexed_live_records_match_the_per_record_parse() {
        use crate::usage::archive::ArchiveFrontier;
        let now = Local::now();
        let today = now.date_naive();
        let yesterday = today.pred_opt().unwrap();
        let at = |day: chrono::NaiveDate, hour: u32| {
            Local
                .from_local_datetime(&day.and_hms_opt(hour, 30, 0).unwrap())
                .earliest()
                .unwrap()
        };
        let tokyo = chrono::FixedOffset::east_opt(9 * 3600).unwrap();
        let rec = |ts: String, model: &str| CompactUsageRecord {
            ts,
            model: model.to_string(),
            input_tokens: 1,
            output_tokens: 1,
            cache_5m: 0,
            cache_1h: 0,
            cache_read: 0,
            speed: None,
            dedupe_key: None,
        };
        // Out of order and in several offsets: one in an archived hour, one
        // in a frontier gap, one before the day, one that does not parse.
        let records = Arc::new(vec![
            rec(at(today, 11).to_rfc3339(), "claude-sonnet-4-6"),
            rec(
                at(today, 9).with_timezone(&tokyo).to_rfc3339(),
                "claude-sonnet-4-6",
            ),
            rec(
                at(today, 8).with_timezone(&chrono::Utc).to_rfc3339(),
                "gpt-5.4",
            ),
            rec(at(yesterday, 23).to_rfc3339(), "claude-opus-4-6"),
            rec(String::from("not-a-timestamp"), "claude-sonnet-4-6"),
            rec(at(today, 10).to_rfc3339(), "<synthetic>"),
            rec(at(today, 1).to_rfc3339(), "gpt-5.4"),
        ]);
        let frontier = ArchiveFrontier {
            date: today,
            hour: 9,
        };
        let archived_hours: HashSet<_> = [(today, 9u8)].into();
        let old_open = |ts: &str| match parse_remote_ts(ts).map(|t| t.with_timezone(&Local)) {
            Some(local) => {
                let hour = (local.date_naive(), local.hour() as u8);
                !(frontier.covers(hour.0, hour.1) && archived_hours.contains(&hour))
            }
            None => true,
        };
        let index = RecordIndex::new(records.clone());

        for provider in ["all", "claude", "codex", "claude+cursor"] {
            for (period, offset) in [("day", 0), ("day", -1), ("week", 0), ("5h", 0)] {
                let bounds =
                    crate::commands::period::resolve_period_bounds_at(period, offset, now, None)
                        .unwrap();
                let mut old: Vec<&str> = records
                    .iter()
                    .filter(|r| compact_record_matches_provider(r, provider))
                    .filter(|r| old_open(&r.ts) && remote_ts_in_bounds(&bounds, &r.ts))
                    .map(|r| r.ts.as_str())
                    .collect();
                let mut new: Vec<&str> =
                    live_records_in(&index, provider, &bounds, Some(&frontier), &archived_hours)
                        .map(|(_, r)| r.ts.as_str())
                        .collect();
                old.sort_unstable();
                new.sort_unstable();
                assert_eq!(new, old, "{provider} {period} {offset}");
            }
            let old_any = records
                .iter()
                .any(|r| compact_record_matches_provider(r, provider) && old_open(&r.ts));
            let new_any = index.all().any(|(local, r)| {
                compact_record_matches_provider(r, provider)
                    && ssh_live_hour_open(Some(&frontier), &archived_hours, local)
            });
            assert_eq!(new_any, old_any, "{provider}");
        }
    }

    #[test]
    fn remote_ssh_usage_gate_follows_subset_membership() {
        // Existing answers.
        assert!(provider_includes_remote_ssh_usage("all"));
        assert!(provider_includes_remote_ssh_usage("claude"));
        assert!(provider_includes_remote_ssh_usage("codex"));
        assert!(!provider_includes_remote_ssh_usage("cursor"));
        assert!(!provider_includes_remote_ssh_usage("kimi"));
        assert!(!provider_includes_remote_ssh_usage("gemini"));
        // Subsets: remote records are Claude/Codex only, so include them
        // exactly when one of those two is in the scope.
        assert!(provider_includes_remote_ssh_usage("codex+kimi"));
        assert!(provider_includes_remote_ssh_usage("claude+cursor"));
        assert!(!provider_includes_remote_ssh_usage("cursor+kimi"));
    }
}
