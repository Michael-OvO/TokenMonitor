use super::period::*;
use super::{
    maybe_capture_query_debug, merge_usage_source, merge_usage_warning, parse_usage_selection,
    set_last_usage_debug, AppState, UsageDebugReport,
};
use crate::models::*;
#[cfg(test)]
use crate::stats::change::ParsedChangeEvent;
use crate::stats::change::{aggregate_change_stats, aggregate_model_change_summary};
use crate::usage::integrations::{
    all_usage_integrations, UsageIntegrationId, UsageIntegrationSelection,
    ALL_USAGE_INTEGRATIONS_ID,
};
use crate::usage::parser::{LogAppends, UsageParser};
#[cfg(test)]
use chrono::Datelike;
use chrono::{NaiveDate, Timelike};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager, State};

// Tied to PRICING_VERSION so a rates bump invalidates persisted usage payloads.
const USAGE_PAYLOAD_CACHE_VERSION: &str = crate::usage::pricing::PRICING_VERSION;

/// Fetch the Cursor range an open view lacks, in the background. The one
/// publish outside the refresh cycle: a widening fetch adds only days before
/// what the cache covered, so today's numbers and the tray stay as published,
/// and only a fetch that changed the cache clears the views built without
/// its data and emits `data-updated` for the open view to fill in. A first
/// fetch, which also brings today's data, requests a refresh so the tray
/// catches up.
pub(crate) fn spawn_cursor_remote_fetch_if_needed(
    app: &tauri::AppHandle,
    state: &AppState,
    since: Option<NaiveDate>,
) {
    if !state.parser.needs_cursor_remote_fetch(since)
        || state.cursor_remote_fetch_inflight.load(Ordering::SeqCst)
    {
        return;
    }

    let app_handle = app.clone();
    tokio::spawn(async move {
        let state = app_handle.state::<AppState>();
        // Only a first fetch brings today's data; the cycle that ran while it
        // was in flight could not fetch and published a day cost without it.
        let today_uncovered = state
            .parser
            .cursor_remote_uncovered(Some(chrono::Local::now().date_naive()));
        let fetched = fetch_cursor_remote_now(&state, since).await;
        if fetched == CursorFetch::Unchanged {
            return;
        }
        // The store bumped the Cursor generation before these clears, so a
        // view still computing from the old data will not be cached after
        // them (get_usage_data_inner). Drop the persisted views it changed
        // too, so the refetch recomputes with the fresh remote data; disk
        // first, so a disk hit in flight cannot copy one back into memory.
        let stale = |key: &str| view_built_on_changed_cursor_data(key, fetched);
        state.clear_payload_disk_cache_where(stale).await;
        state.parser.clear_payload_cache_where(stale);
        let generation = state.refresh.generation.load(Ordering::SeqCst);
        let _ = app_handle.emit("data-updated", generation);
        if today_uncovered {
            crate::refresh::request_refresh(&state);
        }
    });
}

/// What a Cursor remote fetch changed in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorFetch {
    /// Nothing: it failed, was superseded, or found the same data.
    Unchanged,
    /// It widened the cache: only days before this one, the first it had
    /// covered, were added.
    AddedBefore(NaiveDate),
    /// Any of the data.
    Changed,
}

/// A view payload's key (a `usage-view:` or `full:` key in memory, or its
/// file name on disk, where `:` and `+` are `_`), read back: the
/// integrations it counts, its period, and the first day it covers. `None`
/// for a key of another shape.
fn parse_view_key(key: &str) -> Option<(Vec<UsageIntegrationId>, &str, Option<NaiveDate>)> {
    let mut parts = key.split(|c: char| !(c.is_alphanumeric() || c == '-'));
    if !matches!(parts.next(), Some("usage-view" | "full")) {
        return None;
    }
    parts.next(); // USAGE_PAYLOAD_CACHE_VERSION
    let mut counts = Vec::new();
    loop {
        match parts.next()? {
            ALL_USAGE_INTEGRATIONS_ID => counts.extend(all_usage_integrations()),
            period @ ("5h" | "day" | "week" | "month" | "year") => {
                parts.next(); // the offset
                let start = parts
                    .next()
                    .and_then(|date| NaiveDate::parse_from_str(date, "%Y%m%d").ok());
                return Some((counts, period, start));
            }
            part => counts.extend(UsageIntegrationId::parse(part)),
        }
    }
}

/// Whether the view payload at `key` (see [`parse_view_key`]) may hold
/// Cursor data `fetched` changed: one counting Cursor, and for a widening,
/// starting before the days it had. A key of another shape counts as stale.
fn view_built_on_changed_cursor_data(key: &str, fetched: CursorFetch) -> bool {
    let Some((counts, _, start)) = parse_view_key(key) else {
        return true;
    };
    counts.contains(&UsageIntegrationId::Cursor)
        && match fetched {
            CursorFetch::Unchanged => false,
            CursorFetch::AddedBefore(covered) => start.is_none_or(|start| start < covered),
            CursorFetch::Changed => true,
        }
}

/// Whether the view payload at `key` (see [`parse_view_key`]) may hold rows
/// `appends` added: one counting an integration they came from, over a
/// period reaching the first day they may be dated. A key of another shape
/// counts as stale.
pub(crate) fn view_built_on_appended_logs(key: &str, appends: &LogAppends) -> bool {
    let Some((counts, period, start)) = parse_view_key(key) else {
        return true;
    };
    // The most days a period spans from its first; a 5h window can cross
    // midnight.
    let days = match period {
        "day" => 1,
        "5h" => 2,
        "week" => 7,
        "month" => 31,
        _ => 366,
    };
    counts.iter().any(|id| appends.integrations.contains(id))
        && start.is_none_or(|start| appends.since < start + chrono::Days::new(days))
}

/// Fetch Cursor's remote usage since `since` and merge it into the cache.
/// Returns what the stored data changed; the store bumps the Cursor
/// generation, so the caller's clears that follow drop every view built from
/// the old data. Returns `Unchanged` without fetching while another fetch is
/// in flight.
///
/// A failed fetch records a cooldown on the parser (see
/// `UsageParser::note_cursor_remote_failure`) so a persistently failing remote
/// is retried at most once per cooldown instead of on every refresh.
///
/// A fetch still running when the Cursor data is cleared (the account may
/// have changed) lands neither its entries nor its failure: both belong to
/// the credentials it began with.
pub(crate) async fn fetch_cursor_remote_now(
    state: &AppState,
    since: Option<NaiveDate>,
) -> CursorFetch {
    let Some(_inflight) = crate::refresh::InFlight::claim(&state.cursor_remote_fetch_inflight)
    else {
        return CursorFetch::Unchanged;
    };
    let parser = &state.parser;
    // Before the credentials are read, so a clear after it supersedes them.
    let generation = parser.cursor_remote_generation();
    // After it: a clear since would drop the store this describes. Only this
    // fetch, in flight alone, may store until then.
    let widening_from = parser.cursor_widening_from(since);
    tracing::debug!("[cursor-async] Starting Cursor remote fetch since={since:?}");
    let result = tokio::task::spawn_blocking(move || {
        crate::usage::cursor_parser::fetch_cursor_remote_entries(since)
    })
    .await;
    let failed = |error: String| {
        tracing::warn!("[cursor-async] {error}");
        // Mark the failure so `needs_cursor_remote_fetch` goes quiet for a
        // cooldown window: a failure produced no new data, and retrying at
        // once would only fail again.
        parser.note_cursor_remote_failure_if_current(generation);
        CursorFetch::Unchanged
    };
    match result {
        Ok(Ok(Some(entries))) => {
            let fetched = entries.len();
            match parser.store_cursor_remote_if_current(entries, since, generation) {
                Some(changed) => {
                    tracing::debug!(
                        "[cursor-async] Fetch complete: {fetched} entries, changed={changed}"
                    );
                    match (changed, widening_from) {
                        (false, _) => CursorFetch::Unchanged,
                        (true, Some(covered)) => CursorFetch::AddedBefore(covered),
                        (true, None) => CursorFetch::Changed,
                    }
                }
                None => {
                    tracing::info!(
                        "[cursor-async] Dropped {fetched} entries: the Cursor data was cleared while fetching"
                    );
                    CursorFetch::Unchanged
                }
            }
        }
        Ok(Ok(None)) => {
            tracing::debug!("[cursor-async] No cursor auth configured");
            CursorFetch::Unchanged
        }
        Ok(Err(e)) => failed(format!("Cursor remote fetch failed: {e}")),
        Err(e) => failed(format!("Cursor remote task panicked: {e}")),
    }
}

/// Ensure every day in [start, end) has a chart bucket, inserting empty buckets
/// for days with no data.  This prevents the chart from appearing "cut off" when
/// the current month hasn't ended yet.
fn pad_daily_buckets(payload: &mut UsagePayload, start: NaiveDate, end: NaiveDate) {
    let existing: std::collections::HashSet<String> = payload
        .chart_buckets
        .iter()
        .map(|b| b.sort_key.clone())
        .collect();

    let mut date = start;
    while date < end {
        let key = date.format("%Y-%m-%d").to_string();
        if !existing.contains(&key) {
            payload.chart_buckets.push(ChartBucket {
                label: date.format("%b %-d").to_string(),
                sort_key: key,
                total: 0.0,
                segments: Vec::new(),
            });
        }
        date += chrono::Duration::days(1);
    }

    payload
        .chart_buckets
        .sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
}

/// Filter a UsagePayload's chart_buckets to only include dates in [start, end).
/// Recalculates total_cost, total_tokens, and model_breakdown from the retained buckets.
fn filter_buckets_to_range(payload: &mut UsagePayload, start: NaiveDate, end: NaiveDate) {
    payload.chart_buckets.retain(|bucket| {
        parse_bucket_start_date(&bucket.sort_key)
            .map(|d| d >= start && d < end)
            .unwrap_or(false)
    });

    payload.total_cost = payload.chart_buckets.iter().map(|b| b.total).sum();
    payload.total_tokens = payload
        .chart_buckets
        .iter()
        .flat_map(|b| &b.segments)
        .map(|s| s.tokens)
        .sum();
    payload.session_count = payload
        .chart_buckets
        .iter()
        .filter(|b| b.total > 0.0)
        .count() as u32;

    // Rebuild model_breakdown from retained buckets
    let mut model_map: HashMap<String, (String, f64, u64, bool)> = HashMap::new();
    for bucket in &payload.chart_buckets {
        for seg in &bucket.segments {
            let entry =
                model_map
                    .entry(seg.model_key.clone())
                    .or_insert((seg.model.clone(), 0.0, 0, true));
            entry.1 += seg.cost;
            entry.2 += seg.tokens;
            entry.3 &= seg.pricing_available;
        }
    }
    payload.model_breakdown = model_map
        .into_iter()
        .map(
            |(key, (name, cost, tokens, pricing_available))| ModelSummary {
                display_name: name,
                model_key: key,
                cost,
                tokens,
                pricing_available,
                change_stats: None,
            },
        )
        .collect();

    // Recalculate input/output/cache tokens (populated later from raw entries)
    payload.input_tokens = 0;
    payload.output_tokens = 0;
    payload.cache_read_tokens = 0;
    payload.cache_write_5m_tokens = 0;
    payload.cache_write_1h_tokens = 0;
    payload.web_search_requests = 0;
}

fn parser_payload_for_period(
    parser: &UsageParser,
    provider: &str,
    period: &str,
    bounds: &PeriodBounds,
) -> Result<UsagePayload, String> {
    let since_str = bounds.start.format("%Y%m%d").to_string();

    let mut payload = match period {
        "5h" => parser.get_time_range(provider, bounds.range_start, bounds.range_end),
        "day" => parser.get_hourly(provider, &since_str),
        "week" => {
            let mut p = parser.get_daily(provider, &since_str);
            filter_buckets_to_range(&mut p, bounds.start, bounds.end);
            pad_daily_buckets(&mut p, bounds.start, bounds.end);
            p
        }
        "month" => {
            let mut p = parser.get_daily(provider, &since_str);
            filter_buckets_to_range(&mut p, bounds.start, bounds.end);
            pad_daily_buckets(&mut p, bounds.start, bounds.end);
            p
        }
        "year" => {
            let mut p = parser.get_monthly(provider, &since_str);
            filter_buckets_to_range(&mut p, bounds.start, bounds.end);
            p
        }
        _ => return Err(format!("Unknown period: {period}")),
    };

    payload.period_label = bounds.period_label.clone();
    if period != "5h" {
        payload.has_earlier_data = parser.has_entries_before(provider, bounds.start);
    }

    Ok(payload)
}

/// The `items` inside the period: the loaded slice itself when all are (a
/// current period), else a filtered copy.
fn in_bounds<T: Clone>(items: &[T], inside: impl Fn(&T) -> bool) -> Cow<'_, [T]> {
    if items.iter().all(&inside) {
        Cow::Borrowed(items)
    } else {
        Cow::Owned(items.iter().filter(|item| inside(item)).cloned().collect())
    }
}

fn attach_local_stats(
    parser: &UsageParser,
    payload: &mut UsagePayload,
    provider: &str,
    bounds: &PeriodBounds,
) {
    let loaded = parser.load_entries_cached(provider, Some(bounds.start));
    let entries = in_bounds(&loaded.entries, |entry| {
        bounds.contains_timestamp(entry.timestamp)
    });
    let change_events = in_bounds(&loaded.change_events, |event| {
        bounds.contains_timestamp(event.timestamp)
    });

    payload.change_stats =
        aggregate_change_stats(&change_events, payload.total_cost, payload.total_tokens);
    for model in &mut payload.model_breakdown {
        model.change_stats = aggregate_model_change_summary(&change_events, &model.model_key);
    }

    if payload.usage_source == UsageSource::Parser {
        payload.input_tokens = entries.iter().map(|entry| entry.input_tokens).sum();
        payload.output_tokens = entries.iter().map(|entry| entry.output_tokens).sum();
        payload.cache_read_tokens = entries.iter().map(|e| e.cache_read_tokens).sum();
        payload.cache_write_5m_tokens = entries.iter().map(|e| e.cache_creation_5m_tokens).sum();
        payload.cache_write_1h_tokens = entries.iter().map(|e| e.cache_creation_1h_tokens).sum();
        payload.web_search_requests = entries.iter().map(|e| e.web_search_requests).sum();
    }

    payload.subagent_stats = crate::stats::subagent::aggregate_subagent_stats(
        &entries,
        &change_events,
        payload.total_cost,
    );
}

/// Shared by `usage-view:` and `full:` so both miss after midnight. Today's
/// hourly chart runs to the current hour, so its tags carry the hour: a view
/// no change dropped is still redrawn each hour. The 5h tags also carry the
/// official reset (a same-day roll), to the minute so the sources' jitter
/// around it does not count. A reset still ahead pins the window, which then
/// moves only with the data, whose change drops the view; the burn rate,
/// which runs on the clock, is brought up to date at each hit
/// ([`rebase_burn_rate`]). Otherwise the window rolls with the clock, and
/// the refresh `generation` makes it one recompute per sample.
fn usage_cache_tags_with_reset(
    period: &str,
    offset: i32,
    generation: u64,
    five_hour_reset: Option<chrono::DateTime<chrono::Local>>,
) -> String {
    let date_tag = resolve_period_bounds_with_reset(period, offset, five_hour_reset)
        .map(|b| b.start.format("%Y%m%d").to_string())
        .unwrap_or_default();
    if period == "day" && offset == 0 {
        return format!("{date_tag}:h{}", chrono::Local::now().hour());
    }
    if period != "5h" {
        return date_tag;
    }
    let reset_tag = five_hour_reset
        .map(|r| format!(":r{}", (r.timestamp() + 30).div_euclid(60) * 60))
        .unwrap_or_default();
    if five_hour_reset.is_some_and(|r| r > chrono::Local::now()) {
        return format!("{date_tag}{reset_tag}");
    }
    format!("{date_tag}{reset_tag}:g{generation}")
}

/// A 5h view's burn rate and projection over its window from
/// `window_start` to now: a view reused from an earlier sample is brought up
/// to now. Views without a live block have none.
fn rebase_burn_rate(payload: &mut UsagePayload, window_start: chrono::DateTime<chrono::Local>) {
    if let Some(block) = payload.active_block.as_mut() {
        let elapsed = chrono::Local::now() - window_start;
        let hours = elapsed.num_milliseconds().max(1) as f64 / 3_600_000.0;
        block.burn_rate_per_hour = block.cost / hours;
        block.projected_cost = block.burn_rate_per_hour * 5.0;
    }
}

fn final_usage_cache_key_with_reset(
    provider: &str,
    period: &str,
    offset: i32,
    generation: u64,
    five_hour_reset: Option<chrono::DateTime<chrono::Local>>,
) -> String {
    format!(
        "usage-view:{USAGE_PAYLOAD_CACHE_VERSION}:{provider}:{period}:{offset}:{}",
        usage_cache_tags_with_reset(period, offset, generation, five_hour_reset)
    )
}

fn full_usage_cache_key_with_reset(
    provider: &str,
    period: &str,
    offset: i32,
    generation: u64,
    five_hour_reset: Option<chrono::DateTime<chrono::Local>>,
) -> String {
    format!(
        "full:{USAGE_PAYLOAD_CACHE_VERSION}:{provider}:{period}:{offset}:{}",
        usage_cache_tags_with_reset(period, offset, generation, five_hour_reset)
    )
}

async fn finalize_usage_payload(
    state: &AppState,
    provider: &str,
    period: &str,
    offset: i32,
    mut payload: UsagePayload,
) -> UsagePayload {
    let (device_breakdown, device_chart_buckets, included) = tokio::join!(
        crate::usage::device_aggregation::build_device_breakdown_for_payload(
            state, provider, period, offset,
        ),
        crate::usage::device_aggregation::build_device_time_chart_buckets(
            state, provider, period, offset,
        ),
        crate::usage::device_aggregation::build_included_devices_payload(
            state, provider, period, offset,
        ),
    );

    tracing::debug!(
        "[DEVICE] finalize_usage_payload: provider={provider} period={period} offset={offset}"
    );
    tracing::debug!(
        "[DEVICE] local payload before merge: total_cost={:.2}, total_tokens={}",
        payload.total_cost,
        payload.total_tokens,
    );
    if let Some(ref bd) = device_breakdown {
        for d in bd {
            tracing::debug!(
                "[DEVICE] breakdown: device={} cost={:.2} is_local={} include_in_stats={}",
                d.device,
                d.total_cost,
                d.is_local,
                d.include_in_stats,
            );
        }
    } else {
        tracing::debug!("[DEVICE] device_breakdown = None");
    }
    tracing::debug!(
        "[DEVICE] build_included_devices_payload returned: {:?}",
        included.as_ref().map(|p| format!(
            "total_cost={:.2} models={}",
            p.total_cost,
            p.model_breakdown.len()
        )),
    );

    payload.device_breakdown = device_breakdown;
    payload.device_chart_buckets = device_chart_buckets;

    if let Some(included) = included {
        payload = merge_payloads(payload, included);
    }

    tracing::debug!(
        "[DEVICE] final merged payload: total_cost={:.2}, total_tokens={}",
        payload.total_cost,
        payload.total_tokens,
    );

    payload
}

fn get_provider_chart_data(
    parser: &UsageParser,
    provider: &str,
    period: &str,
    offset: i32,
    five_hour_reset: Option<chrono::DateTime<chrono::Local>>,
) -> Result<UsagePayload, String> {
    let bounds = resolve_period_bounds_with_reset(period, offset, five_hour_reset)?;
    parser_payload_for_period(parser, provider, period, &bounds)
}

pub(crate) fn get_provider_data(
    parser: &UsageParser,
    provider: &str,
    period: &str,
    offset: i32,
) -> Result<UsagePayload, String> {
    get_provider_data_for_interval(parser, provider, period, offset, 0, None)
}

fn get_provider_data_for_interval(
    parser: &UsageParser,
    provider: &str,
    period: &str,
    offset: i32,
    generation: u64,
    five_hour_reset: Option<chrono::DateTime<chrono::Local>>,
) -> Result<UsagePayload, String> {
    let bounds = resolve_period_bounds_with_reset(period, offset, five_hour_reset)?;
    let cache_key =
        full_usage_cache_key_with_reset(provider, period, offset, generation, five_hour_reset);
    if let Some(mut cached) = parser.check_cache(&cache_key) {
        rebase_burn_rate(&mut cached, bounds.range_start);
        return Ok(cached);
    }

    // Same rule as the final store in `get_usage_data_inner`: a later view
    // compute would trust this entry, so it must not hold a superseded
    // Cursor snapshot.
    let cursor_generation = parser.cursor_remote_generation();
    let mut payload = parser_payload_for_period(parser, provider, period, &bounds)?;
    attach_local_stats(parser, &mut payload, provider, &bounds);

    parser.store_cache_at_cursor_generation(&cache_key, payload.clone(), cursor_generation);

    Ok(payload)
}

fn merge_payloads(mut c: UsagePayload, x: UsagePayload) -> UsagePayload {
    let mut bucket_map: BTreeMap<String, ChartBucket> = BTreeMap::new();
    let c_buckets = std::mem::take(&mut c.chart_buckets);
    for b in c_buckets.into_iter().chain(x.chart_buckets) {
        let entry = bucket_map
            .entry(b.sort_key.clone())
            .or_insert_with(|| ChartBucket {
                label: b.label,
                sort_key: b.sort_key,
                total: 0.0,
                segments: vec![],
            });
        entry.total += b.total;
        entry.segments.extend(b.segments);
    }

    let mut model_map: HashMap<String, ModelSummary> = HashMap::new();
    let c_models = std::mem::take(&mut c.model_breakdown);
    for model in c_models.into_iter().chain(x.model_breakdown) {
        let entry = model_map
            .entry(model.model_key.clone())
            .or_insert_with(|| ModelSummary {
                display_name: model.display_name,
                model_key: model.model_key,
                cost: 0.0,
                tokens: 0,
                pricing_available: true,
                change_stats: None,
            });
        entry.cost += model.cost;
        entry.tokens += model.tokens;
        entry.pricing_available &= model.pricing_available;
    }

    c.total_cost += x.total_cost;
    c.total_tokens += x.total_tokens;
    c.input_tokens += x.input_tokens;
    c.output_tokens += x.output_tokens;
    c.cache_read_tokens += x.cache_read_tokens;
    c.cache_write_5m_tokens += x.cache_write_5m_tokens;
    c.cache_write_1h_tokens += x.cache_write_1h_tokens;
    c.web_search_requests += x.web_search_requests;

    if let Some(ref mut c_stats) = c.subagent_stats {
        if let Some(x_stats) = x.subagent_stats {
            c_stats.main.cost += x_stats.main.cost;
            c_stats.main.input_tokens += x_stats.main.input_tokens;
            c_stats.main.output_tokens += x_stats.main.output_tokens;
            c_stats.main.cache_read_tokens += x_stats.main.cache_read_tokens;
            c_stats.main.cache_write_5m_tokens += x_stats.main.cache_write_5m_tokens;
            c_stats.main.cache_write_1h_tokens += x_stats.main.cache_write_1h_tokens;
            c_stats.subagents.cost += x_stats.subagents.cost;
            c_stats.subagents.input_tokens += x_stats.subagents.input_tokens;
            c_stats.subagents.output_tokens += x_stats.subagents.output_tokens;
            c_stats.subagents.cache_read_tokens += x_stats.subagents.cache_read_tokens;
            c_stats.subagents.cache_write_5m_tokens += x_stats.subagents.cache_write_5m_tokens;
            c_stats.subagents.cache_write_1h_tokens += x_stats.subagents.cache_write_1h_tokens;
        } else if x.total_cost > 0.0 {
            c_stats.main.cost += x.total_cost;
            c_stats.main.input_tokens += x.input_tokens;
            c_stats.main.output_tokens += x.output_tokens;
        }
    } else {
        c.subagent_stats = x.subagent_stats;
    }

    c.chart_buckets = bucket_map.into_values().collect();
    c.session_count = c.chart_buckets.iter().filter(|b| b.total > 0.0).count() as u32;
    c.model_breakdown = model_map.into_values().collect();
    c.model_breakdown.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    c.active_block = merge_active_blocks(c.active_block, x.active_block);
    c.five_hour_cost += x.five_hour_cost;
    c.from_cache = c.from_cache && x.from_cache;
    c.usage_source = merge_usage_source(c.usage_source, x.usage_source);
    c.usage_warning = merge_usage_warning(c.usage_warning, x.usage_warning);
    c.has_earlier_data = c.has_earlier_data || x.has_earlier_data;
    c
}

fn merge_active_blocks(
    left: Option<ActiveBlock>,
    right: Option<ActiveBlock>,
) -> Option<ActiveBlock> {
    match (
        left.filter(|block| block.is_active),
        right.filter(|block| block.is_active),
    ) {
        (None, None) => None,
        (Some(block), None) | (None, Some(block)) => Some(block),
        (Some(a), Some(b)) => Some(ActiveBlock {
            cost: a.cost + b.cost,
            burn_rate_per_hour: a.burn_rate_per_hour + b.burn_rate_per_hour,
            projected_cost: a.projected_cost + b.projected_cost,
            is_active: true,
        }),
    }
}

#[cfg(test)]
pub(crate) fn load_change_events_for_period(
    parser: &UsageParser,
    provider: &str,
    period: &str,
    offset: i32,
) -> Vec<ParsedChangeEvent> {
    let Ok(bounds) = resolve_period_bounds(period, offset) else {
        return Vec::new();
    };

    let (_entries, mut change_events, _reports) = parser.load_entries(provider, Some(bounds.start));
    change_events.retain(|event| bounds.contains_timestamp(event.timestamp));
    change_events
}

#[tauri::command]
pub async fn get_known_models(
    provider: String,
    state: State<'_, AppState>,
) -> Result<Vec<KnownModel>, String> {
    parse_usage_selection(&provider)?;
    if !state.usage_access_enabled() {
        return Ok(Vec::new());
    }
    let _gate = state.compute.lock().await;

    let (entries, _, _) = state.parser.load_entries(&provider, None);
    let mut models = BTreeMap::<String, KnownModel>::new();
    for entry in entries {
        let model = crate::models::known_model_from_raw(&entry.model);
        models.entry(model.model_key.clone()).or_insert(model);
    }

    // Also include models discovered from SSH remote caches.
    if crate::usage::device_aggregation::provider_includes_remote_ssh_usage(&provider) {
        let cache_guard = state.ssh_cache.read().await;
        if let Some(mgr) = cache_guard.as_ref() {
            let hosts = state.ssh_hosts.read().await;
            for cfg in hosts.iter().filter(|c| c.enabled) {
                if let Ok(records) = mgr.load_cached_records_shared(&cfg.alias) {
                    for record in records.iter() {
                        if record.model.starts_with('<') {
                            continue;
                        }
                        if !crate::usage::device_aggregation::compact_record_matches_provider(
                            record, &provider,
                        ) {
                            continue;
                        }
                        let model = crate::models::known_model_from_raw(&record.model);
                        models.entry(model.model_key.clone()).or_insert(model);
                    }
                }
            }
        }
    }

    Ok(models.into_values().collect())
}

#[tauri::command]
pub async fn get_usage_data(
    app: tauri::AppHandle,
    provider: String,
    period: String,
    offset: i32,
    background: Option<bool>,
    state: State<'_, AppState>,
) -> Result<UsagePayload, String> {
    if background != Some(true) {
        // The refresh recomputes this view every cycle, so keep only a scope
        // that parses, in its canonical spelling.
        if let Ok(selection) = parse_usage_selection(&provider) {
            if let Ok(mut view) = state.refresh.active_view.lock() {
                *view = Some((selection.to_string(), period.clone(), offset));
            }
        }
    }
    get_usage_data_inner(Some(&app), &state, &provider, &period, offset).await
}

pub(crate) async fn get_usage_data_inner(
    app: Option<&tauri::AppHandle>,
    state: &AppState,
    provider: &str,
    period: &str,
    offset: i32,
) -> Result<UsagePayload, String> {
    let _ipc_t0 = std::time::Instant::now();
    tracing::debug!(
        "[PROFILE] get_usage_data: provider={provider} period={period} offset={offset}"
    );
    if !state.usage_access_enabled() {
        return Ok(UsagePayload {
            usage_warning: Some(String::from("Usage access has not been enabled yet.")),
            ..UsagePayload::default()
        });
    }
    tracing::debug!(
        "[PROFILE] get_usage_data: access-check = {:?}",
        _ipc_t0.elapsed()
    );

    let parser = &state.parser;
    let selection = parse_usage_selection(provider)?;
    // Canonical spelling (sorted, de-duplicated) so every cache key, disk
    // entry and debug report for the same set of integrations lines up no
    // matter how the caller spelled the scope.
    let canonical_provider = selection.to_string();
    let provider = canonical_provider.as_str();
    let five_hour_reset = if period == "5h" {
        let cached = state.cached_rate_limits.read().await;
        official_five_hour_reset(provider, cached.as_ref())
    } else {
        None
    };
    let generation = state.refresh.generation.load(Ordering::SeqCst);
    let final_cache_key =
        final_usage_cache_key_with_reset(provider, period, offset, generation, five_hour_reset);
    // A rolling 5h key carries the refresh generation (see
    // usage_cache_tags_with_reset), so every refresh mints a fresh key. The
    // disk cache has no TTL and the generation restarts at launch, so
    // persisted 5h entries would pile up and could even be re-served by a
    // later session. Keep 5h in memory only.
    let use_disk_cache = period != "5h";
    if let Some(hit) = serve_memory_hit(
        app,
        state,
        &final_cache_key,
        provider,
        period,
        offset,
        five_hour_reset,
    )
    .await
    {
        return Ok(hit);
    }

    // Disk cache fallback: return stale data instantly, caller refreshes in background.
    if use_disk_cache {
        if let Some(ref disk_cache) = *state.payload_disk_cache.read().await {
            if let Some(mut cached) = disk_cache.load(&final_cache_key) {
                // Ignore payloads persisted while async cursor data was still
                // pending: the disk cache has no TTL, so serving a `cursor_loading`
                // payload would show empty cursor usage forever (and the disk hit
                // returns before the background-fetch spawn below, so it can never
                // self-heal). Treat it as a miss to recompute + re-trigger fetch.
                if cached.cursor_loading {
                    tracing::info!(
                    "[DEVICE] get_usage_data DISK CACHE SKIP (incomplete/cursor_loading): key={final_cache_key}"
                );
                } else {
                    tracing::debug!(
                    "[DEVICE] get_usage_data HIT DISK CACHE: key={final_cache_key} total_cost={:.2}",
                    cached.total_cost,
                );
                    // The first sample dropped what the previous session
                    // left, and every sample since the views its changes
                    // reached: after it, a disk hit is this session's compute
                    // of data the current sample holds, only evicted from
                    // memory by its TTL.
                    cached.from_cache = last_sample(state).is_none();
                    parser.store_cache(&final_cache_key, cached.clone());
                    stamp_published(state, &mut cached);
                    return Ok(cached);
                }
            }
        }
    }

    // One heavy job at a time, in arrival order. Re-check once admitted: an
    // identical miss queued ahead of this one has just stored the payload.
    let _gate = state.compute.lock().await;
    if let Some(hit) = serve_memory_hit(
        app,
        state,
        &final_cache_key,
        provider,
        period,
        offset,
        five_hour_reset,
    )
    .await
    {
        return Ok(hit);
    }
    tracing::debug!("[DEVICE] get_usage_data CACHE MISS: key={final_cache_key}");
    tracing::debug!(
        "[PROFILE] get_usage_data: cache-miss (memory+disk), starting full parse. elapsed={:?}",
        _ipc_t0.elapsed()
    );

    // Read before anything touches the Cursor cache; the stores below drop
    // the payload if a Cursor fetch changed the remote data meanwhile.
    let cursor_generation = parser.cursor_remote_generation();
    let mut payload = match &selection {
        UsageIntegrationSelection::Single(integration_id) => {
            let mut payload = get_provider_data_for_interval(
                parser,
                provider,
                period,
                offset,
                generation,
                five_hour_reset,
            )?;
            payload.provider_detected =
                Some(integration_id.detect_roots().iter().any(|r| r.exists()));
            set_last_usage_debug(
                state,
                UsageDebugReport {
                    request_kind: String::from("usage"),
                    requested_provider: integration_id.as_str().to_string(),
                    period: Some(period.to_string()),
                    offset: Some(offset),
                    year: None,
                    month: None,
                    queries: maybe_capture_query_debug(parser, &payload)?
                        .into_iter()
                        .collect(),
                },
            )
            .await;
            // An inner `full:` hit is still this session's compute, not a
            // copy restored from disk.
            payload.from_cache = false;

            finalize_usage_payload(state, provider, period, offset, payload).await
        }
        UsageIntegrationSelection::All | UsageIntegrationSelection::Subset(_) => {
            // `All` and `Subset` share the merge path: iterate the selection's ids.
            let bounds = resolve_period_bounds_with_reset(period, offset, five_hour_reset)?;
            let mut merged: Option<UsagePayload> = None;
            let mut queries = Vec::new();

            for integration_id in selection.integration_ids() {
                let mut payload = get_provider_chart_data(
                    parser,
                    integration_id.as_str(),
                    period,
                    offset,
                    five_hour_reset,
                )?;
                if let Some(warning) = payload.usage_warning.take() {
                    payload.usage_warning =
                        Some(format!("{}: {warning}", integration_id.display_name()));
                }
                if let Some(query) = maybe_capture_query_debug(parser, &payload)? {
                    queries.push(query);
                }
                merged = Some(match merged {
                    Some(current) => merge_payloads(current, payload),
                    None => payload,
                });
            }

            let mut merged = merged.unwrap_or_default();

            set_last_usage_debug(
                state,
                UsageDebugReport {
                    request_kind: String::from("usage"),
                    requested_provider: provider.to_string(),
                    period: Some(period.to_string()),
                    offset: Some(offset),
                    year: None,
                    month: None,
                    queries,
                },
            )
            .await;

            // Aggregate stats once from the selected providers' entries.
            attach_local_stats(parser, &mut merged, provider, &bounds);

            finalize_usage_payload(state, provider, period, offset, merged).await
        }
    };
    tracing::debug!(
        "[PROFILE] get_usage_data: payload-built = {:?}",
        _ipc_t0.elapsed()
    );

    // Check if cursor remote data needs async fetching. The cache is range-aware:
    // a fetch is only needed when it doesn't already cover this period's `since`.
    // Its age does not matter here; the periodic refresh keeps it fresh.
    let cursor_included = selection.includes_cursor();
    let cursor_since = resolve_period_bounds_with_reset(period, offset, five_hour_reset)
        .ok()
        .map(|b| b.start);
    let cursor_uncovered = cursor_included && parser.cursor_remote_uncovered(cursor_since);
    // A fetch that just failed sits in its retry cooldown: nothing will be
    // spawned now, but the remote data is still unsettled. Keep the payload
    // marked incomplete so the TTL-less disk cache below can't preserve a
    // cursor-less payload past the outage.
    let cursor_remote_unsettled =
        cursor_uncovered || (cursor_included && parser.cursor_remote_failure_cooldown_active());
    if cursor_remote_unsettled {
        payload.cursor_loading = true;
    }

    // A Cursor fetch that changed the remote data during the compute left
    // this payload built from the old snapshot. Its completion clears the
    // caches and emits `data-updated`, so cache nothing (memory or disk)
    // and let that refetch recompute.
    let cached = parser.store_cache_at_cursor_generation(
        &final_cache_key,
        payload.clone(),
        cursor_generation,
    );
    parser.clear_entries_cache();

    // Persist to disk for next cold start (fire-and-forget). Never persist an
    // incomplete payload: a `cursor_loading` payload is missing its async
    // remote data, and the TTL-less disk cache would serve it forever.
    if cached && use_disk_cache && !payload.cursor_loading {
        if let Some(ref disk_cache) = *state.payload_disk_cache.read().await {
            disk_cache.save(&final_cache_key, &payload);
            // The change may also land during the save, after its completion
            // already cleared the disk prefix: undo the save ourselves.
            if parser.cursor_remote_generation() != cursor_generation {
                disk_cache.remove(&final_cache_key);
            }
        }
    }

    // Spawn background Cursor remote fetch if needed.
    if cursor_uncovered {
        if let Some(app_ref) = app {
            spawn_cursor_remote_fetch_if_needed(app_ref, state, cursor_since);
        }
    }

    // The one INFO line per compute; the steps above log at debug.
    tracing::info!(
        "[PROFILE] get_usage_data: TOTAL = {:?} (provider={provider} period={period} offset={offset})",
        _ipc_t0.elapsed()
    );
    stamp_published(state, &mut payload);
    Ok(payload)
}

async fn serve_memory_hit(
    app: Option<&tauri::AppHandle>,
    state: &AppState,
    key: &str,
    provider: &str,
    period: &str,
    offset: i32,
    five_hour_reset: Option<chrono::DateTime<chrono::Local>>,
) -> Option<UsagePayload> {
    let parser = &state.parser;
    let mut cached = parser.check_cache_as_stored(key)?;
    stamp_published(state, &mut cached);
    if cached.active_block.is_some() {
        if let Ok(bounds) = resolve_period_bounds_with_reset(period, offset, five_hour_reset) {
            rebase_burn_rate(&mut cached, bounds.range_start);
        }
    }
    tracing::debug!(
        "[DEVICE] get_usage_data HIT MEMORY CACHE: key={key} total_cost={:.2}",
        cached.total_cost,
    );
    set_last_usage_debug(
        state,
        UsageDebugReport {
            request_kind: String::from("usage"),
            requested_provider: provider.to_string(),
            period: Some(period.to_string()),
            offset: Some(offset),
            year: None,
            month: None,
            queries: vec![],
        },
    )
    .await;
    // A payload cached while its Cursor range was uncovered may have had
    // its fetch spawn dropped (another fetch was in flight, and that one
    // need not emit). Retry here, or the hit keeps serving it Cursor-less.
    if let Some(app_ref) = app.filter(|_| cached.cursor_loading) {
        let cursor_since = resolve_period_bounds_with_reset(period, offset, five_hour_reset)
            .ok()
            .map(|b| b.start);
        if parser.cursor_remote_uncovered(cursor_since) {
            spawn_cursor_remote_fetch_if_needed(app_ref, state, cursor_since);
        }
    }
    Some(cached)
}

/// Dates a payload by the sample it was computed from, so every view of one
/// sample shows the same time. A copy restored from disk keeps its own.
fn stamp_published(state: &AppState, payload: &mut UsagePayload) {
    if payload.from_cache {
        return;
    }
    if let Some(sample) = last_sample(state) {
        payload.last_updated = sample.to_rfc3339();
    }
}

fn last_sample(state: &AppState) -> Option<chrono::DateTime<chrono::Local>> {
    state.refresh.last_sample.lock().ok().and_then(|s| *s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::integrations::{all_usage_integrations, ALL_USAGE_INTEGRATIONS_ID};
    use crate::usage::parser::UsageParser;
    use crate::usage::ssh_remote::{SshCacheManager, SshHostConfig};
    use chrono::{Local, Timelike};
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn usage_cache_tags(period: &str, offset: i32, generation: u64) -> String {
        usage_cache_tags_with_reset(period, offset, generation, None)
    }

    fn final_usage_cache_key(provider: &str, period: &str, offset: i32, generation: u64) -> String {
        final_usage_cache_key_with_reset(provider, period, offset, generation, None)
    }

    fn full_usage_cache_key(provider: &str, period: &str, offset: i32, generation: u64) -> String {
        full_usage_cache_key_with_reset(provider, period, offset, generation, None)
    }

    /// A widening adds Cursor days only before those the cache had: views
    /// starting on or after them, and those without Cursor, stay cached.
    #[test]
    fn a_widening_clears_only_the_cursor_views_that_start_before_it() {
        let stale = view_built_on_changed_cursor_data;
        let start = resolve_period_bounds("month", -1).unwrap().start;
        let next_day = start + chrono::Duration::days(1);
        let view = final_usage_cache_key("claude+cursor", "month", -1, 0);
        let on_disk = view.replace([':', '+'], "_");
        for key in [&view, &on_disk] {
            assert!(stale(key, CursorFetch::AddedBefore(next_day)), "{key}");
            assert!(!stale(key, CursorFetch::AddedBefore(start)), "{key}");
            assert!(stale(key, CursorFetch::Changed), "{key}");
        }
        assert!(stale(
            &full_usage_cache_key("all", "5h", 0, 7),
            CursorFetch::Changed
        ));
        assert!(!stale(
            &final_usage_cache_key("claude", "month", -1, 0),
            CursorFetch::Changed
        ));
    }

    fn bucket(label: &str, sort_key: &str, total: f64) -> ChartBucket {
        ChartBucket {
            label: label.to_string(),
            sort_key: sort_key.to_string(),
            total,
            segments: vec![],
        }
    }

    fn model(display_name: &str, model_key: &str, cost: f64, tokens: u64) -> ModelSummary {
        ModelSummary {
            display_name: display_name.to_string(),
            model_key: model_key.to_string(),
            cost,
            tokens,
            pricing_available: true,
            change_stats: None,
        }
    }

    fn payload_with_buckets(chart_buckets: Vec<ChartBucket>) -> UsagePayload {
        UsagePayload {
            total_cost: chart_buckets.iter().map(|bucket| bucket.total).sum(),
            session_count: chart_buckets.len() as u32,
            chart_buckets,
            ..UsagePayload::default()
        }
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn local_timestamp(date: NaiveDate, hour: u32) -> String {
        date.and_hms_opt(hour, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .to_rfc3339()
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
        state
            .usage_access_enabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
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
    async fn source_change_must_invalidate_disk_cache_so_per_provider_view_refreshes() {
        // Regression for DBG-010: the no-TTL payload disk cache re-served a
        // stale per-provider payload after the in-memory cache was cleared,
        // freezing the Claude/Codex tabs for the day while the `all` view kept
        // refreshing (its disk entries are cleared by the cursor/SSH paths).
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let app_data_dir = TempDir::new().unwrap();
        let now = Local::now();

        write_file(
            &claude_dir.path().join("session.jsonl"),
            &claude_assistant_entry(&now.to_rfc3339(), "claude-sonnet-4-6-20260301", 1_000, 500),
        );

        let mut state = AppState::new();
        state
            .usage_access_enabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        state.parser = Arc::new(UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        ));
        *state.payload_disk_cache.write().await = Some(
            crate::usage::payload_disk_cache::PayloadDiskCache::new(app_data_dir.path()),
        );

        // Real computation: establishes the true cost and persists it to disk.
        let real = get_usage_data_inner(None, &state, "claude", "day", 0)
            .await
            .unwrap();
        assert!(
            real.total_cost > 0.0,
            "fixture should produce nonzero claude usage"
        );

        // Plant a stale payload on disk under the same key — as if it had been
        // written earlier in the day, before more usage was logged — then clear
        // ONLY the in-memory cache, exactly what the refresh loops used to do.
        let key = final_usage_cache_key("claude", "day", 0, 0);
        let stale = UsagePayload {
            total_cost: 999_999.0,
            ..UsagePayload::default()
        };
        state
            .payload_disk_cache
            .read()
            .await
            .as_ref()
            .unwrap()
            .save(&key, &stale);
        state.parser.clear_payload_cache();

        // Bug reproduction: a memory-only invalidation re-serves the stale disk
        // entry instead of recomputing.
        let stale_served = get_usage_data_inner(None, &state, "claude", "day", 0)
            .await
            .unwrap();
        assert_eq!(
            stale_served.total_cost, 999_999.0,
            "guard: the disk cache short-circuits recompute on a memory miss"
        );

        // The fix: clearing the disk cache (what the refresh loops now do on a
        // source change) forces a recompute that reflects the real logs again.
        state.parser.clear_payload_cache();
        state.clear_payload_disk_cache().await;
        let fresh = get_usage_data_inner(None, &state, "claude", "day", 0)
            .await
            .unwrap();
        assert_eq!(
            fresh.total_cost, real.total_cost,
            "after disk invalidation the per-provider view recomputes fresh"
        );
        assert!(
            (fresh.total_cost - 999_999.0).abs() > 1.0,
            "the stale payload must not survive disk invalidation"
        );
    }

    #[tokio::test]
    async fn memory_hit_is_not_labelled_cached() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let app_data_dir = TempDir::new().unwrap();
        write_file(
            &claude_dir.path().join("session.jsonl"),
            &claude_assistant_entry(
                &Local::now().to_rfc3339(),
                "claude-sonnet-4-6-20260301",
                1_000,
                500,
            ),
        );

        let mut state = AppState::new();
        state
            .usage_access_enabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        state.parser = Arc::new(UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        ));
        *state.payload_disk_cache.write().await = Some(
            crate::usage::payload_disk_cache::PayloadDiskCache::new(app_data_dir.path()),
        );
        let sample = Local::now() - chrono::Duration::hours(3);
        *state.refresh.last_sample.lock().unwrap() = Some(sample);
        let published = sample.to_rfc3339();

        let computed = get_usage_data_inner(None, &state, "claude", "day", 0)
            .await
            .unwrap();
        assert!(!computed.from_cache);
        assert_eq!(computed.last_updated, published);

        let hit = get_usage_data_inner(None, &state, "claude", "day", 0)
            .await
            .unwrap();
        let debug = state.last_usage_debug.read().await.clone().unwrap();
        assert!(
            debug.queries.is_empty(),
            "guard: the second request is a hit"
        );
        assert!(
            !hit.from_cache,
            "a memory hit of this session's compute is not 'cached'"
        );
        assert_eq!(hit.last_updated, published);

        // Mark the disk copy, so a read of it can be told from a recompute.
        let key = final_usage_cache_key("claude", "day", 0, 0);
        let marked = UsagePayload {
            total_cost: 999_999.0,
            last_updated: Local::now().to_rfc3339(),
            ..computed.clone()
        };
        state
            .payload_disk_cache
            .read()
            .await
            .as_ref()
            .unwrap()
            .save(&key, &marked);

        // After a sample, the disk holds only this session's computes: the
        // first sample dropped what the previous one left. A view the payload
        // TTL evicted from memory comes back from disk as current.
        state.parser.clear_payload_cache();
        let evicted = get_usage_data_inner(None, &state, "claude", "day", 0)
            .await
            .unwrap();
        assert_eq!(evicted.total_cost, 999_999.0, "guard: a disk hit");
        assert!(
            !evicted.from_cache,
            "a disk hit after a sample is not 'cached'"
        );
        assert_eq!(evicted.last_updated, published);

        // Before the first sample a disk hit is the previous session's copy:
        // it keeps its label and its own time, in memory too.
        *state.refresh.last_sample.lock().unwrap() = None;
        state.parser.clear_payload_cache();
        let disk = get_usage_data_inner(None, &state, "claude", "day", 0)
            .await
            .unwrap();
        assert_eq!(disk.total_cost, 999_999.0, "guard: a disk hit");
        assert!(disk.from_cache);
        let disk_copy_hit = get_usage_data_inner(None, &state, "claude", "day", 0)
            .await
            .unwrap();
        assert!(disk_copy_hit.from_cache);
        assert_ne!(disk_copy_hit.last_updated, published);
    }

    #[tokio::test]
    async fn gate_serialises_and_coalesces_identical_misses() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        write_file(
            &claude_dir.path().join("session.jsonl"),
            &claude_assistant_entry(
                &Local::now().to_rfc3339(),
                "claude-sonnet-4-6-20260301",
                1_000,
                500,
            ),
        );
        let mut state = AppState::new();
        state
            .usage_access_enabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        state.parser = Arc::new(UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        ));
        let st = Arc::new(state);

        // A multi-integration scope has no inner `full:` cache, so a second
        // compute would record query debug; a hit records none.
        let scope = "claude+codex";
        let gate = st.compute.lock().await;
        let spawn_miss = || {
            let st = Arc::clone(&st);
            tokio::spawn(async move { get_usage_data_inner(None, &st, scope, "day", 0).await })
        };
        let first = spawn_miss();
        let second = spawn_miss();
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(
            !first.is_finished() && !second.is_finished(),
            "a miss waits while another job holds the gate"
        );

        drop(gate);
        let first = first.await.unwrap().unwrap();
        let second = second.await.unwrap().unwrap();
        assert!(first.total_cost > 0.0, "fixture should produce usage");
        let debug = st.last_usage_debug.read().await.clone().unwrap();
        assert!(
            debug.queries.is_empty(),
            "the queued identical miss is served by the first compute"
        );
        assert!(!first.from_cache && !second.from_cache);
        assert_eq!(first.total_cost, second.total_cost);
        assert_eq!(first.last_updated, second.last_updated);
    }

    #[tokio::test]
    async fn get_usage_data_inner_returns_empty_until_usage_access_is_enabled() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let now = Local::now();
        let timestamp = now.to_rfc3339();

        write_file(
            &claude_dir.path().join("session.jsonl"),
            &claude_assistant_entry(&timestamp, "claude-sonnet-4-6-20260301", 1_000, 500),
        );

        let mut state = AppState::new();
        state.parser = Arc::new(UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        ));

        let payload = get_usage_data_inner(None, &state, "all", "day", 0)
            .await
            .unwrap();

        assert_eq!(
            payload.usage_warning.as_deref(),
            Some("Usage access has not been enabled yet.")
        );
        assert_eq!(payload.total_cost, 0.0);
        assert_eq!(payload.total_tokens, 0);
        assert!(payload.chart_buckets.is_empty());
        assert!(payload.model_breakdown.is_empty());
    }

    #[test]
    fn merge_payloads_orders_by_sort_key_and_merges_duplicate_buckets() {
        let left = payload_with_buckets(vec![
            bucket("Mar 2", "2026-03-02", 1.0),
            bucket("Mar 12", "2026-03-12", 3.0),
        ]);
        let right = payload_with_buckets(vec![
            bucket("Mar 10", "2026-03-10", 2.0),
            bucket("Mar 12", "2026-03-12", 4.0),
        ]);

        let merged = merge_payloads(left, right);
        let labels: Vec<&str> = merged
            .chart_buckets
            .iter()
            .map(|bucket| bucket.label.as_str())
            .collect();

        assert_eq!(labels, vec!["Mar 2", "Mar 10", "Mar 12"]);
        assert_eq!(merged.chart_buckets[2].total, 7.0);
        assert_eq!(merged.session_count, 3);
    }

    #[test]
    fn merge_payloads_combines_model_breakdowns_and_active_blocks() {
        let left = UsagePayload {
            total_cost: 3.0,
            total_tokens: 30,
            session_count: 1,
            input_tokens: 20,
            output_tokens: 10,
            chart_buckets: vec![bucket("9am", "2026-03-15T09:00:00-04:00", 3.0)],
            model_breakdown: vec![model("Fallback", "unknown", 3.0, 30)],
            active_block: Some(ActiveBlock {
                cost: 3.0,
                burn_rate_per_hour: 6.0,
                projected_cost: 15.0,
                is_active: true,
            }),
            five_hour_cost: 3.0,
            from_cache: true,
            ..UsagePayload::default()
        };
        let right = UsagePayload {
            total_cost: 2.0,
            total_tokens: 20,
            session_count: 1,
            input_tokens: 10,
            output_tokens: 10,
            chart_buckets: vec![bucket("9am", "2026-03-15T09:05:00-04:00", 2.0)],
            model_breakdown: vec![model("Fallback", "unknown", 2.0, 20)],
            active_block: Some(ActiveBlock {
                cost: 2.0,
                burn_rate_per_hour: 4.0,
                projected_cost: 10.0,
                is_active: true,
            }),
            five_hour_cost: 2.0,
            ..UsagePayload::default()
        };

        let merged = merge_payloads(left, right);
        let block = merged.active_block.expect("expected merged active block");

        assert_eq!(merged.model_breakdown.len(), 1);
        assert_eq!(merged.model_breakdown[0].cost, 5.0);
        assert_eq!(merged.model_breakdown[0].tokens, 50);
        assert_eq!(block.cost, 5.0);
        assert_eq!(block.burn_rate_per_hour, 10.0);
        assert_eq!(block.projected_cost, 25.0);
        assert_eq!(merged.five_hour_cost, 5.0);
        assert!(!merged.from_cache);
    }

    #[test]
    fn merge_payloads_marks_mixed_sources_and_combines_warnings() {
        let left = UsagePayload {
            usage_source: UsageSource::Mixed,
            usage_warning: Some(String::from("Claude: fallback one")),
            ..UsagePayload::default()
        };
        let right = UsagePayload {
            usage_source: UsageSource::Parser,
            usage_warning: Some(String::from("Codex: fallback two")),
            ..UsagePayload::default()
        };

        let merged = merge_payloads(left, right);
        assert_eq!(merged.usage_source, UsageSource::Mixed);
        assert_eq!(
            merged.usage_warning.as_deref(),
            Some("Claude: fallback one\nCodex: fallback two")
        );
    }

    #[test]
    fn filter_buckets_to_range_supports_monthly_sort_keys() {
        let mut payload = payload_with_buckets(vec![
            bucket("Dec", "2025-12", 1.0),
            bucket("Jan", "2026-01", 2.0),
            bucket("Feb", "2026-02", 3.0),
        ]);

        filter_buckets_to_range(
            &mut payload,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
        );

        assert_eq!(payload.chart_buckets.len(), 1);
        assert_eq!(payload.chart_buckets[0].label, "Jan");
        assert_eq!(payload.total_cost, 2.0);
    }

    #[test]
    fn year_period_filters_to_target_year_only() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let project_dir = claude_dir.path().join("test-project");
        fs::create_dir_all(&project_dir).unwrap();

        let current_year = Local::now().year();
        let previous_year = current_year - 1;
        let prior_entry = format!(
            r#"{{"type":"assistant","timestamp":"{previous_year}-06-15T10:00:00-04:00","message":{{"model":"claude-opus-4-6","usage":{{"input_tokens":1000,"output_tokens":500}},"stop_reason":"end_turn"}}}}"#
        );
        let current_entry = format!(
            r#"{{"type":"assistant","timestamp":"{current_year}-03-10T10:00:00-04:00","message":{{"model":"claude-sonnet-4-6","usage":{{"input_tokens":1000,"output_tokens":500}},"stop_reason":"end_turn"}}}}"#
        );
        write_file(
            &project_dir.join("session.jsonl"),
            &format!("{prior_entry}\n{current_entry}"),
        );

        let parser = UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        );
        let payload = get_provider_data(&parser, "claude", "year", -1).unwrap();

        assert_eq!(payload.period_label, previous_year.to_string());
        assert_eq!(payload.chart_buckets.len(), 1);
        assert_eq!(
            payload.chart_buckets[0].sort_key,
            format!("{previous_year}-06")
        );
        assert_eq!(payload.model_breakdown.len(), 1);
        assert_eq!(payload.model_breakdown[0].model_key, "opus-4-6");
    }

    #[test]
    fn codex_5h_uses_blocks_payload_shape() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let now = Local::now();
        let day_dir = codex_dir
            .path()
            .join(now.format("%Y").to_string())
            .join(now.format("%m").to_string())
            .join(now.format("%d").to_string());
        fs::create_dir_all(&day_dir).unwrap();

        let content = format!(
            r#"{{"type":"event_msg","timestamp":"{}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":1000,"output_tokens":500,"reasoning_output_tokens":100,"cached_input_tokens":50}}}}}}}}"#,
            now.to_rfc3339()
        );
        write_file(&day_dir.join("session.jsonl"), &content);

        let parser = UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        );
        let payload = get_provider_data(&parser, "codex", "5h", 0).unwrap();

        assert_eq!(
            payload
                .chart_buckets
                .iter()
                .filter(|b| b.total > 0.0)
                .count(),
            1
        );
        assert!(
            payload.active_block.is_some(),
            "codex 5h should treat the current window as live"
        );
        assert!(
            payload.five_hour_cost > 0.0,
            "block payloads should populate 5h cost"
        );
        assert_eq!(payload.usage_source, UsageSource::Parser);
        assert!(
            payload.usage_warning.is_none(),
            "codex 5h should not have a warning when using local parser directly"
        );
    }

    #[test]
    fn five_h_includes_cross_midnight_usage_inside_official_window() {
        let claude_dir = TempDir::new().unwrap();
        let now = Local::now();
        let reset = now + chrono::Duration::hours(1);
        let start = reset - chrono::Duration::hours(5);
        let inside = start + chrono::Duration::minutes(10);
        let outside = start - chrono::Duration::minutes(10);
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{outside}","message":{{"model":"claude-sonnet-4-6-20260301","usage":{{"input_tokens":1000,"output_tokens":500}},"stop_reason":"end_turn"}}}}
{{"type":"assistant","timestamp":"{inside}","message":{{"model":"claude-sonnet-4-6-20260301","usage":{{"input_tokens":2000,"output_tokens":1000}},"stop_reason":"end_turn"}}}}"#,
            outside = outside.to_rfc3339(),
            inside = inside.to_rfc3339(),
        );
        write_file(&claude_dir.path().join("session.jsonl"), &content);

        let parser = UsageParser::with_claude_dir(claude_dir.path().to_path_buf());
        let payload =
            get_provider_data_for_interval(&parser, "claude", "5h", 0, 0, Some(reset)).unwrap();

        assert_eq!(payload.input_tokens, 2000);
        assert!(payload.total_cost > 0.0);
        assert!((payload.five_hour_cost - payload.total_cost).abs() < f64::EPSILON);
    }

    #[test]
    fn claude_day_view_falls_back_to_parser_with_warning() {
        let dir = TempDir::new().unwrap();
        let now = Local::now();
        // Noon anchor: `now - 1h` crosses into yesterday near midnight.
        let ts = now
            .date_naive()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .to_rfc3339();
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"model":"claude-sonnet-4-6-20260301","usage":{{"input_tokens":1000,"output_tokens":500}},"stop_reason":"end_turn"}}}}"#,
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let payload = get_provider_data(&parser, "claude", "day", 0).unwrap();

        assert_eq!(payload.usage_source, UsageSource::Parser);
        assert!(
            payload.usage_warning.is_none(),
            "day view should not have a warning when using local parser directly"
        );
    }

    #[test]
    fn get_provider_data_uses_full_request_cache() {
        let dir = TempDir::new().unwrap();
        let now = Local::now();
        // Noon anchor: `now - 1h` crosses into yesterday near midnight.
        let ts = now
            .date_naive()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .to_rfc3339();
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"model":"claude-sonnet-4-6-20260301","usage":{{"input_tokens":1000,"output_tokens":500}},"stop_reason":"end_turn"}}}}"#,
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());

        let first = get_provider_data(&parser, "claude", "day", 0).unwrap();
        assert!(!first.from_cache, "first request should be computed");

        let second = get_provider_data(&parser, "claude", "day", 0).unwrap();
        assert!(
            second.from_cache,
            "second request should hit the full cache"
        );
    }

    #[test]
    fn five_hour_key_follows_generation_and_official_reset() {
        // A rolling 5h window moves with the clock, so it is recomputed once
        // per refresh (generation); any 5h view whenever the official window
        // rolls (reset): the date alone misses a same-day roll.
        let reset_a = Local::now() + chrono::Duration::hours(1);
        let reset_b = reset_a + chrono::Duration::minutes(1);
        let key = |period: &str, generation: u64, reset: Option<chrono::DateTime<Local>>| {
            final_usage_cache_key_with_reset("claude", period, 0, generation, reset)
        };

        assert_ne!(
            key("5h", 1, None),
            key("5h", 2, None),
            "a rolling 5h key must change with the generation"
        );
        let passed = Local::now() - chrono::Duration::minutes(1);
        assert_ne!(key("5h", 1, Some(passed)), key("5h", 2, Some(passed)));
        assert_eq!(
            key("5h", 1, Some(reset_a)),
            key("5h", 2, Some(reset_a)),
            "a reset still ahead pins the window: one view across samples"
        );
        assert_ne!(
            key("5h", 1, Some(reset_a)),
            key("5h", 1, Some(reset_b)),
            "5h key must change with the official reset"
        );
        // 08:49:59.8 from one source, 08:50:00.2 from another.
        let minute = reset_a.with_second(0).unwrap().with_nanosecond(0).unwrap();
        let jitter = chrono::Duration::milliseconds(200);
        assert_eq!(
            key("5h", 1, Some(minute - jitter)),
            key("5h", 1, Some(minute + jitter)),
            "to the minute"
        );

        // Today's hourly chart runs to the current hour; past days stay put.
        let hour_tag = format!(":h{}", Local::now().hour());
        assert!(key("day", 1, None).ends_with(&hour_tag));
        assert!(!final_usage_cache_key("claude", "day", -1, 1).contains(":h"));

        // Day-granular periods keep reusable keys, disk entries included.
        for period in ["day", "week", "month", "year"] {
            assert_eq!(
                key(period, 1, None),
                key(period, 2, Some(reset_a)),
                "{period} key must ignore the generation and the reset"
            );
        }

        // Inner `full:` keys must carry the same tags, or an outer miss after a
        // new generation / reset would still hit a stale inner entry.
        for (generation, reset) in [(1, None), (2, Some(reset_a))] {
            let tags = usage_cache_tags_with_reset("5h", 0, generation, reset);
            assert!(key("5h", generation, reset).ends_with(&tags));
            assert!(
                full_usage_cache_key_with_reset("claude", "5h", 0, generation, reset)
                    .ends_with(&tags)
            );
        }
        let day_tags = usage_cache_tags("day", 0, 1);
        assert!(full_usage_cache_key("claude", "day", 0, 2).ends_with(&day_tags));
        assert!(final_usage_cache_key("claude", "day", 0, 2).ends_with(&day_tags));
    }

    #[test]
    fn a_reused_five_hour_view_brings_its_burn_rate_up_to_now() {
        let mut payload = UsagePayload {
            active_block: Some(ActiveBlock {
                cost: 10.0,
                burn_rate_per_hour: 10.0,
                projected_cost: 50.0,
                is_active: true,
            }),
            ..UsagePayload::default()
        };
        rebase_burn_rate(&mut payload, Local::now() - chrono::Duration::hours(2));
        let block = payload.active_block.unwrap();
        assert!((block.burn_rate_per_hour - 5.0).abs() < 0.01, "{block:?}");
        assert!((block.projected_cost - 25.0).abs() < 0.05);
    }

    /// Lines appended today drop the views that count their integration and
    /// reach today; the other providers' and the earlier periods' stay.
    #[test]
    fn an_append_clears_only_the_views_it_can_reach() {
        let appends = LogAppends {
            integrations: vec![UsageIntegrationId::Claude],
            since: Local::now().date_naive(),
        };
        let stale = |key: String| {
            let on_disk = key.replace([':', '+'], "_");
            let stale = view_built_on_appended_logs(&key, &appends);
            assert_eq!(view_built_on_appended_logs(&on_disk, &appends), stale);
            stale
        };
        assert!(stale(final_usage_cache_key("claude", "day", 0, 0)));
        assert!(stale(final_usage_cache_key("all", "month", 0, 0)));
        assert!(stale(final_usage_cache_key("claude+kimi", "5h", 0, 3)));
        assert!(stale(full_usage_cache_key("claude", "year", 0, 0)));
        assert!(!stale(final_usage_cache_key("codex", "day", 0, 0)));
        assert!(!stale(final_usage_cache_key("cursor+kimi", "week", 0, 0)));
        assert!(!stale(final_usage_cache_key("claude", "day", -1, 0)));
        assert!(!stale(final_usage_cache_key("all", "month", -2, 0)));
        assert!(stale("sentinel".to_string()), "an unknown shape is dropped");
    }

    #[test]
    fn usage_payload_cache_version_follows_pricing_table() {
        assert_eq!(
            USAGE_PAYLOAD_CACHE_VERSION,
            crate::usage::pricing::PRICING_VERSION
        );
        let key = final_usage_cache_key("all", "day", 0, 0);
        assert!(
            key.contains(USAGE_PAYLOAD_CACHE_VERSION),
            "disk cache key must include pricing version, got {key}"
        );
    }

    #[test]
    fn inner_full_cache_misses_stale_date_and_5h_bucket() {
        let dir = TempDir::new().unwrap();
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let stale = UsagePayload {
            total_cost: 99.0,
            ..UsagePayload::default()
        };

        // Pre-fix inner key (no date_tag): must not be served as "today".
        parser.store_cache(
            &format!("full:{USAGE_PAYLOAD_CACHE_VERSION}:claude:day:0"),
            stale.clone(),
        );
        let day = get_provider_data(&parser, "claude", "day", 0).unwrap();
        assert!(
            !day.from_cache,
            "full: key without date_tag must miss after a date rollover"
        );
        assert_ne!(day.total_cost, 99.0);

        // A 5h entry from an earlier refresh generation must not be reused.
        parser.store_cache(&full_usage_cache_key("claude", "5h", 0, 1), stale);
        let five = get_provider_data_for_interval(&parser, "claude", "5h", 0, 2, None).unwrap();
        assert!(
            !five.from_cache,
            "full: key from an earlier 5h generation must miss"
        );
        assert_ne!(five.total_cost, 99.0);
    }

    #[test]
    fn clearing_usage_view_cache_keeps_provider_cache_entries() {
        let dir = TempDir::new().unwrap();
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let full_key = full_usage_cache_key("claude", "day", 0, 0);
        let final_key = final_usage_cache_key("claude", "day", 0, 0);

        parser.store_cache(&full_key, UsagePayload::default());
        parser.store_cache(&final_key, UsagePayload::default());

        parser.clear_payload_cache_prefix("usage-view:");

        assert!(
            parser.check_cache(&full_key).is_some(),
            "provider cache should survive usage-view invalidation"
        );
        assert!(
            parser.check_cache(&final_key).is_none(),
            "usage-view cache should be removed by prefix invalidation"
        );
    }

    #[test]
    fn a_multi_provider_load_reuses_each_providers_cached_load() {
        let dir = TempDir::new().unwrap();
        let (claude_dir, codex_dir) = (dir.path().join("claude"), dir.path().join("codex"));
        fs::create_dir_all(&codex_dir).unwrap();
        let today = Local::now().date_naive();
        // Anchor the fixture to the day being queried rather than to `now`:
        // a `now - 1h` timestamp falls on the previous day between 00:00 and
        // 01:00 local time, and the `since = today` filter would drop it.
        let ts = local_timestamp(today, 10);
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"model":"claude-sonnet-4-6-20260301","usage":{{"input_tokens":2000,"output_tokens":800}},"stop_reason":"end_turn"}}}}"#,
        );
        let session = claude_dir.join("session.jsonl");
        write_file(&session, &content);
        let parser = UsageParser::with_dirs(claude_dir, codex_dir);

        // The Claude chart of a `claude+codex` view loads first.
        let claude = parser.load_entries_cached("claude", Some(today));
        assert_eq!(claude.entries.len(), 1);
        fs::remove_file(&session).unwrap();

        // The view's stats take that load as it was, not the logs again.
        let merged = parser.load_entries_cached("claude+codex", Some(today));
        assert_eq!(merged.entries.len(), 1);
        assert_eq!(merged.reports.len(), 2, "one report per integration");
    }

    #[test]
    fn change_stats_populated_on_provider_payload() {
        // Create a Claude session with an Edit tool_use
        let dir = TempDir::new().unwrap();
        let target_date = Local::now().date_naive() - chrono::Duration::days(1);
        let ts = local_timestamp(target_date, 10);
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{ts}","requestId":"req_1","message":{{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{{"type":"tool_use","id":"tu_1","name":"Edit","input":{{"file_path":"src/main.rs","old_string":"fn old()","new_string":"fn new()\nfn extra()"}}}}],"usage":{{"input_tokens":100,"output_tokens":50}}}}}}"#,
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let payload = get_provider_data(&parser, "claude", "day", -1).unwrap();

        assert!(
            payload.change_stats.is_some(),
            "change_stats should be populated when there are edit events"
        );
        let stats = payload.change_stats.unwrap();
        assert_eq!(stats.added_lines, 2);
        assert_eq!(stats.removed_lines, 1);
        assert_eq!(stats.net_lines, 1);
        assert_eq!(stats.files_touched, 1);
        assert_eq!(stats.change_events, 1);
    }

    #[test]
    fn change_stats_none_when_no_edits() {
        let dir = TempDir::new().unwrap();
        let target_date = Local::now().date_naive() - chrono::Duration::days(1);
        let ts = local_timestamp(target_date, 10);
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"model":"claude-opus-4-6-20260301","usage":{{"input_tokens":100,"output_tokens":50}}}}}}"#,
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let payload = get_provider_data(&parser, "claude", "day", -1).unwrap();
        assert!(
            payload.change_stats.is_none(),
            "change_stats should be None when there are no edit events"
        );
    }

    #[test]
    fn model_change_stats_populated_per_model() {
        let dir = TempDir::new().unwrap();
        let target_date = Local::now().date_naive() - chrono::Duration::days(1);
        let ts1 = local_timestamp(target_date, 10);
        let ts2 = local_timestamp(target_date, 11);
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{ts1}","requestId":"req_1","message":{{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{{"type":"tool_use","id":"tu_1","name":"Edit","input":{{"file_path":"src/a.rs","old_string":"a","new_string":"b\nc"}}}}],"usage":{{"input_tokens":100,"output_tokens":50}}}}}}
{{"type":"assistant","timestamp":"{ts2}","requestId":"req_2","message":{{"id":"msg_2","model":"claude-sonnet-4-6-20260301","role":"assistant","content":[{{"type":"tool_use","id":"tu_2","name":"Edit","input":{{"file_path":"src/b.rs","old_string":"x","new_string":"y"}}}}],"usage":{{"input_tokens":200,"output_tokens":100}}}}}}"#,
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let payload = get_provider_data(&parser, "claude", "day", -1).unwrap();

        let opus = payload
            .model_breakdown
            .iter()
            .find(|m| m.model_key == "opus-4-6");
        assert!(opus.is_some(), "should have opus-4-6 in model breakdown");
        let opus_stats = opus.unwrap().change_stats.as_ref().unwrap();
        assert_eq!(opus_stats.added_lines, 2);
        assert_eq!(opus_stats.removed_lines, 1);

        let sonnet = payload
            .model_breakdown
            .iter()
            .find(|m| m.model_key == "sonnet-4-6");
        assert!(
            sonnet.is_some(),
            "should have sonnet-4-6 in model breakdown"
        );
        let sonnet_stats = sonnet.unwrap().change_stats.as_ref().unwrap();
        assert_eq!(sonnet_stats.added_lines, 1);
        assert_eq!(sonnet_stats.removed_lines, 1);
    }

    #[test]
    fn historical_day_payload_filters_usage_and_change_stats_to_target_date() {
        let dir = TempDir::new().unwrap();
        let today = Local::now().date_naive();
        let target_date = today - chrono::Duration::days(2);
        let later_date = target_date + chrono::Duration::days(1);
        let target_ts = local_timestamp(target_date, 9);
        let later_ts = local_timestamp(later_date, 9);
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{target_ts}","requestId":"req_1","message":{{"id":"msg_1","model":"claude-sonnet-4-6-20260301","role":"assistant","content":[{{"type":"tool_use","id":"tu_1","name":"Edit","input":{{"file_path":"src/target.rs","old_string":"old","new_string":"new\nextra"}}}}],"usage":{{"input_tokens":100,"output_tokens":50}}}}}}
{{"type":"assistant","timestamp":"{later_ts}","requestId":"req_2","message":{{"id":"msg_2","model":"claude-sonnet-4-6-20260301","role":"assistant","content":[{{"type":"tool_use","id":"tu_2","name":"Edit","input":{{"file_path":"src/later.rs","old_string":"x","new_string":"y\nz\nw"}}}}],"usage":{{"input_tokens":200,"output_tokens":100}}}}}}"#,
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let payload = get_provider_data(&parser, "claude", "day", -2).unwrap();

        assert_eq!(
            payload.total_tokens, 150,
            "later-day usage should be excluded"
        );
        let stats = payload.change_stats.unwrap();
        assert_eq!(stats.added_lines, 2);
        assert_eq!(stats.removed_lines, 1);
        assert_eq!(stats.files_touched, 1);
        assert_eq!(stats.change_events, 1);
    }

    #[test]
    fn month_payload_preserves_input_output_tokens_after_range_filtering() {
        let dir = TempDir::new().unwrap();
        let now = Local::now();
        let current_month = NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap();
        let later_month = if now.month() == 12 {
            NaiveDate::from_ymd_opt(now.year() + 1, 1, 1).unwrap()
        } else {
            NaiveDate::from_ymd_opt(now.year(), now.month() + 1, 1).unwrap()
        };
        let current_ts = local_timestamp(current_month, 11);
        let later_ts = local_timestamp(later_month, 11);
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{current_ts}","message":{{"model":"claude-sonnet-4-6-20260301","usage":{{"input_tokens":123,"output_tokens":45}},"stop_reason":"end_turn"}}}}
{{"type":"assistant","timestamp":"{later_ts}","message":{{"model":"claude-sonnet-4-6-20260301","usage":{{"input_tokens":999,"output_tokens":888}},"stop_reason":"end_turn"}}}}"#,
        );
        write_file(&dir.path().join("session.jsonl"), &content);

        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let payload = get_provider_data(&parser, "claude", "month", 0).unwrap();

        assert_eq!(payload.input_tokens, 123);
        assert_eq!(payload.output_tokens, 45);
        assert_eq!(payload.total_tokens, 168);
    }

    #[test]
    fn load_change_events_for_period_filters_later_months_for_all_provider() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();

        let now = Local::now();
        let target_month = NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap();
        let later_month = if now.month() == 12 {
            NaiveDate::from_ymd_opt(now.year() + 1, 1, 1).unwrap()
        } else {
            NaiveDate::from_ymd_opt(now.year(), now.month() + 1, 1).unwrap()
        };

        let claude_ts = local_timestamp(target_month, 10);
        let claude_content = format!(
            r#"{{"type":"assistant","timestamp":"{claude_ts}","requestId":"req_1","message":{{"id":"msg_1","model":"claude-opus-4-6-20260301","role":"assistant","content":[{{"type":"tool_use","id":"tu_1","name":"Edit","input":{{"file_path":"src/in_range.rs","old_string":"a","new_string":"b\nc"}}}}],"usage":{{"input_tokens":100,"output_tokens":50}}}}}}"#,
        );
        write_file(&claude_dir.path().join("session.jsonl"), &claude_content);

        let codex_session_dir = codex_dir
            .path()
            .join(later_month.format("%Y").to_string())
            .join(later_month.format("%m").to_string())
            .join(later_month.format("%d").to_string());
        fs::create_dir_all(&codex_session_dir).unwrap();
        let codex_ts = local_timestamp(later_month, 10);
        let codex_content = format!(
            r#"{{"type":"turn_context","payload":{{"cwd":"/tmp/demo","model":"gpt-5.4"}}}}
{{"type":"response_item","timestamp":"{codex_ts}","payload":{{"type":"custom_tool_call","status":"completed","name":"apply_patch","input":"*** Begin Patch\n*** Update File: src/out_of_range.rs\n@@\n-old\n+new\n+extra"}}}}"#,
        );
        write_file(&codex_session_dir.join("session.jsonl"), &codex_content);

        let parser = UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        );
        let change_events = load_change_events_for_period(&parser, "all", "month", 0);

        assert_eq!(
            change_events.len(),
            1,
            "later-month edits should be excluded"
        );
        assert_eq!(change_events[0].provider, "claude");
        assert_eq!(change_events[0].path, "src/in_range.rs");
    }

    #[tokio::test]
    async fn get_usage_data_inner_merges_remote_usage_into_all_metrics_and_models() {
        let (state, _claude_dir, _codex_dir, _app_data_dir) =
            build_state_with_remote_claude_data().await;

        let baseline = get_provider_data(&state.parser, "all", "day", 0).unwrap();
        let payload = get_usage_data_inner(None, &state, "all", "day", 0)
            .await
            .unwrap();

        assert!(payload.total_cost > baseline.total_cost);
        assert_eq!(payload.total_tokens, baseline.total_tokens + 3_000);
        assert_eq!(payload.input_tokens, baseline.input_tokens + 2_000);
        assert_eq!(payload.output_tokens, baseline.output_tokens + 1_000);

        let baseline_sonnet = baseline
            .model_breakdown
            .iter()
            .find(|model| model.model_key == "sonnet-4-6")
            .expect("baseline all payload should include local Claude sonnet usage");
        let merged_sonnet = payload
            .model_breakdown
            .iter()
            .find(|model| model.model_key == "sonnet-4-6")
            .expect("final all payload should include merged sonnet usage");
        assert!(merged_sonnet.cost > baseline_sonnet.cost);
        assert_eq!(merged_sonnet.tokens, baseline_sonnet.tokens + 3_000);
    }

    #[tokio::test]
    async fn get_usage_data_inner_merges_remote_usage_into_claude_metrics_and_models() {
        let (state, _claude_dir, _codex_dir, _app_data_dir) =
            build_state_with_remote_claude_data().await;

        let baseline = get_provider_data(&state.parser, "claude", "day", 0).unwrap();
        let payload = get_usage_data_inner(None, &state, "claude", "day", 0)
            .await
            .unwrap();

        assert!(payload.total_cost > baseline.total_cost);
        assert_eq!(payload.total_tokens, baseline.total_tokens + 3_000);
        assert_eq!(payload.input_tokens, baseline.input_tokens + 2_000);
        assert_eq!(payload.output_tokens, baseline.output_tokens + 1_000);

        let baseline_sonnet = baseline
            .model_breakdown
            .iter()
            .find(|model| model.model_key == "sonnet-4-6")
            .expect("baseline Claude payload should include local sonnet usage");
        let merged_sonnet = payload
            .model_breakdown
            .iter()
            .find(|model| model.model_key == "sonnet-4-6")
            .expect("final Claude payload should include merged sonnet usage");
        assert!(merged_sonnet.cost > baseline_sonnet.cost);
        assert_eq!(merged_sonnet.tokens, baseline_sonnet.tokens + 3_000);
    }

    #[tokio::test]
    async fn get_usage_data_inner_keeps_codex_metrics_and_models_local_only() {
        let (state, _claude_dir, _codex_dir, _app_data_dir) =
            build_state_with_remote_claude_data().await;

        let baseline = get_provider_data(&state.parser, "codex", "day", 0).unwrap();
        let payload = get_usage_data_inner(None, &state, "codex", "day", 0)
            .await
            .unwrap();

        assert_eq!(payload.total_cost, baseline.total_cost);
        assert_eq!(payload.total_tokens, baseline.total_tokens);
        assert_eq!(payload.input_tokens, baseline.input_tokens);
        assert_eq!(payload.output_tokens, baseline.output_tokens);
        assert_eq!(
            payload.model_breakdown.len(),
            baseline.model_breakdown.len()
        );
        assert_eq!(
            payload.model_breakdown[0].model_key,
            baseline.model_breakdown[0].model_key
        );
        assert_eq!(
            payload.model_breakdown[0].cost,
            baseline.model_breakdown[0].cost
        );
        assert_eq!(
            payload.model_breakdown[0].tokens,
            baseline.model_breakdown[0].tokens
        );
    }

    #[test]
    #[ignore = "manual benchmark against local Claude/Codex logs"]
    fn benchmark_real_log_cache_paths() {
        fn elapsed_ms(started_at: std::time::Instant) -> f64 {
            started_at.elapsed().as_secs_f64() * 1000.0
        }

        fn average_ms<T>(iterations: usize, mut f: impl FnMut() -> T) -> f64 {
            let started_at = std::time::Instant::now();
            for _ in 0..iterations {
                let _ = f();
            }
            elapsed_ms(started_at) / iterations as f64
        }

        fn load_all_usage(state: &AppState, period: &str, offset: i32) -> UsagePayload {
            let parser = &state.parser;
            let all_cache_key = format!("full-all:{}:{}", period, offset);
            if let Some(cached) = parser.check_cache(&all_cache_key) {
                return cached;
            }

            let mut merged: Option<UsagePayload> = None;
            for integration_id in all_usage_integrations() {
                let payload = get_provider_data(parser, integration_id.as_str(), period, offset)
                    .expect("provider data should load");
                merged = Some(match merged {
                    Some(current) => merge_payloads(current, payload),
                    None => payload,
                });
            }

            let mut merged = merged.unwrap_or_default();

            if let Some((start_date, end_date)) = compute_date_bounds(period, offset) {
                let (mut all_entries, mut all_change_events, _) =
                    parser.load_entries(ALL_USAGE_INTEGRATIONS_ID, Some(start_date));

                all_change_events.retain(|event| {
                    let date = event.timestamp.date_naive();
                    date >= start_date && date < end_date
                });
                all_entries.retain(|entry| {
                    let date = entry.timestamp.date_naive();
                    date >= start_date && date < end_date
                });

                merged.change_stats = aggregate_change_stats(
                    &all_change_events,
                    merged.total_cost,
                    merged.total_tokens,
                );
                for model in &mut merged.model_breakdown {
                    model.change_stats =
                        aggregate_model_change_summary(&all_change_events, &model.model_key);
                }
                merged.subagent_stats = crate::stats::subagent::aggregate_subagent_stats(
                    &all_entries,
                    &all_change_events,
                    merged.total_cost,
                );
            }

            parser.store_cache(&all_cache_key, merged.clone());
            merged
        }

        use super::super::calendar::get_monthly_usage_with_debug_sync;
        use super::super::tray::current_daily_total_cost_for_test;

        let state = AppState::new();
        let now = Local::now();
        let current_year = now.year();
        let current_month = now.month();

        state.parser.clear_cache();
        let started_at = std::time::Instant::now();
        let claude_month_cold =
            get_provider_data(&state.parser, "claude", "month", 0).expect("claude month cold");
        let claude_month_cold_ms = elapsed_ms(started_at);
        let claude_month_hit_ms = average_ms(200, || {
            get_provider_data(&state.parser, "claude", "month", 0).expect("claude month cache hit")
        });
        state.parser.clear_payload_cache();
        let started_at = std::time::Instant::now();
        let claude_month_warm =
            get_provider_data(&state.parser, "claude", "month", 0).expect("claude month warm");
        let claude_month_warm_ms = elapsed_ms(started_at);

        state.parser.clear_cache();
        let started_at = std::time::Instant::now();
        let all_month_cold = load_all_usage(&state, "month", 0);
        let all_month_cold_ms = elapsed_ms(started_at);
        let all_month_hit_ms = average_ms(200, || load_all_usage(&state, "month", 0));
        state.parser.clear_payload_cache();
        let started_at = std::time::Instant::now();
        let all_month_warm = load_all_usage(&state, "month", 0);
        let all_month_warm_ms = elapsed_ms(started_at);

        state.parser.clear_cache();
        let started_at = std::time::Instant::now();
        let (calendar_cold, _) =
            get_monthly_usage_with_debug_sync(&state, "all", current_year, current_month)
                .expect("calendar cold");
        let calendar_cold_ms = elapsed_ms(started_at);
        let calendar_hit_ms = average_ms(200, || {
            get_monthly_usage_with_debug_sync(&state, "all", current_year, current_month)
                .expect("calendar cache hit")
        });
        state.parser.clear_payload_cache();
        let started_at = std::time::Instant::now();
        let (calendar_warm, _) =
            get_monthly_usage_with_debug_sync(&state, "all", current_year, current_month)
                .expect("calendar warm");
        let calendar_warm_ms = elapsed_ms(started_at);

        state.parser.clear_cache();
        let started_at = std::time::Instant::now();
        let tray_cold_total = current_daily_total_cost_for_test(&state);
        let tray_cold_ms = elapsed_ms(started_at);
        let tray_hit_ms = average_ms(500, || current_daily_total_cost_for_test(&state));
        state.parser.clear_payload_cache();
        let started_at = std::time::Instant::now();
        let tray_warm_total = current_daily_total_cost_for_test(&state);
        let tray_warm_ms = elapsed_ms(started_at);

        println!(
            "BENCH claude/month total={:.2} cold_ms={:.2} full_hit_avg_ms={:.4} warm_lower_cache_ms={:.2}",
            claude_month_cold.total_cost,
            claude_month_cold_ms,
            claude_month_hit_ms,
            claude_month_warm_ms
        );
        println!(
            "BENCH all/month total={:.2} cold_ms={:.2} full_hit_avg_ms={:.4} warm_lower_cache_ms={:.2}",
            all_month_cold.total_cost,
            all_month_cold_ms,
            all_month_hit_ms,
            all_month_warm_ms
        );
        println!(
            "BENCH calendar/all/{:04}-{:02} total={:.2} cold_ms={:.2} full_hit_avg_ms={:.4} warm_lower_cache_ms={:.2}",
            current_year,
            current_month,
            calendar_cold.total_cost,
            calendar_cold_ms,
            calendar_hit_ms,
            calendar_warm_ms
        );
        println!(
            "BENCH tray/day total={:.2} cold_ms={:.2} full_hit_avg_ms={:.4} warm_lower_cache_ms={:.2}",
            tray_cold_total,
            tray_cold_ms,
            tray_hit_ms,
            tray_warm_ms
        );

        assert!(claude_month_cold.total_cost >= 0.0);
        assert!(claude_month_warm.total_cost >= 0.0);
        assert!(all_month_warm.total_cost >= 0.0);
        assert!(calendar_warm.total_cost >= 0.0);
        assert!(tray_warm_total >= 0.0);
    }

    #[test]
    #[ignore = "requires local Claude logs and TokenMonitor app data"]
    fn live_claude_usage_reads_5h_and_week_with_archive() {
        let parser = UsageParser::new();
        if let Some(app_data_dir) = dirs::data_dir().map(|dir| dir.join("com.tokenmonitor.app")) {
            parser.set_archive(crate::usage::archive::ArchiveManager::new(&app_data_dir));
        }

        let five_hour = get_provider_data(&parser, "claude", "5h", 0).unwrap();
        let week = get_provider_data(&parser, "claude", "week", 0).unwrap();

        println!(
            "Claude live usage: 5h tokens={} cost=${:.4} buckets={} | week tokens={} cost=${:.4} buckets={}",
            five_hour.total_tokens,
            five_hour.total_cost,
            five_hour.chart_buckets.len(),
            week.total_tokens,
            week.total_cost,
            week.chart_buckets.len(),
        );

        assert!(
            five_hour.total_tokens > 0,
            "expected local Claude 5h usage to be readable"
        );
        assert!(
            week.total_tokens > 0,
            "expected local Claude weekly usage to be readable"
        );
    }

    #[test]
    #[ignore = "profile all periods cold start with real local data"]
    fn profile_all_periods_cold_start() {
        fn elapsed_ms(started_at: std::time::Instant) -> f64 {
            started_at.elapsed().as_secs_f64() * 1000.0
        }

        let providers = ["claude", "all"];
        let periods = ["5h", "day", "week", "month", "year"];

        for provider in &providers {
            let state = AppState::new();
            println!("\n--- Provider: {} ---", provider);

            for period in &periods {
                state.parser.clear_cache();
                let started_at = std::time::Instant::now();
                let result = get_provider_data(&state.parser, provider, period, 0);
                let cold_ms = elapsed_ms(started_at);

                match result {
                    Ok(payload) => {
                        let json_size = serde_json::to_vec(&payload).map(|v| v.len()).unwrap_or(0);
                        println!(
                            "  {}/{} cold={:.1}ms cost=${:.2} tokens={} buckets={} json_bytes={}",
                            provider,
                            period,
                            cold_ms,
                            payload.total_cost,
                            payload.total_tokens,
                            payload.chart_buckets.len(),
                            json_size,
                        );
                    }
                    Err(e) => println!("  {}/{} ERROR: {}", provider, period, e),
                }
            }
        }
    }

    #[tokio::test]
    async fn get_usage_data_inner_subset_excludes_integrations_outside_the_selection() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
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
            &codex_token_count_entry(&timestamp, "gpt-5-codex", 800, 400),
        );

        let mut state = AppState::new();
        state.parser = Arc::new(UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        ));
        state.usage_access_enabled.store(true, Ordering::SeqCst);

        let all = get_usage_data_inner(None, &state, "all", "day", 0)
            .await
            .unwrap();
        let claude_only = get_usage_data_inner(None, &state, "claude", "day", 0)
            .await
            .unwrap();
        // Non-canonical spelling on purpose: the command must normalise it.
        let subset = get_usage_data_inner(None, &state, "kimi+claude", "day", 0)
            .await
            .unwrap();

        assert!(
            all.total_tokens > claude_only.total_tokens,
            "fixture must contribute Codex usage to the all view"
        );
        assert_eq!(subset.total_tokens, claude_only.total_tokens);
        assert!((subset.total_cost - claude_only.total_cost).abs() < 1e-9);
        assert!(
            subset
                .model_breakdown
                .iter()
                .all(|model| !model.model_key.contains("gpt")),
            "Codex models must not appear in a claude+kimi subset"
        );
        assert_eq!(
            subset.chart_buckets.iter().map(|b| b.total).sum::<f64>(),
            claude_only
                .chart_buckets
                .iter()
                .map(|b| b.total)
                .sum::<f64>()
        );
    }
}
