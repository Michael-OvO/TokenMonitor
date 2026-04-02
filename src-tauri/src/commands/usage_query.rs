use super::period::*;
use super::{
    maybe_capture_query_debug, parse_usage_selection, set_last_usage_debug, AppState,
    UsageDebugReport,
};
use crate::models::*;
#[cfg(test)]
use crate::stats::change::ParsedChangeEvent;
use crate::stats::change::{aggregate_change_stats, aggregate_model_change_summary};
use crate::usage::integrations::{
    all_usage_integrations, UsageIntegrationSelection, ALL_USAGE_INTEGRATIONS_ID,
};
use crate::usage::parser::UsageParser;
use chrono::{Datelike, Local, NaiveDate};
use std::collections::{BTreeMap, HashMap};
use tauri::State;

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
    let mut model_map: HashMap<String, (String, f64, u64)> = HashMap::new();
    for bucket in &payload.chart_buckets {
        for seg in &bucket.segments {
            let entry =
                model_map
                    .entry(seg.model_key.clone())
                    .or_insert((seg.model.clone(), 0.0, 0));
            entry.1 += seg.cost;
            entry.2 += seg.tokens;
        }
    }
    payload.model_breakdown = model_map
        .into_iter()
        .map(|(key, (name, cost, tokens))| ModelSummary {
            display_name: name,
            model_key: key,
            cost,
            tokens,
            change_stats: None,
        })
        .collect();

    // Recalculate input/output tokens
    payload.input_tokens = 0;
    payload.output_tokens = 0;
}

fn parser_payload_for_period(
    parser: &UsageParser,
    provider: &str,
    period: &str,
    offset: i32,
) -> Result<UsagePayload, String> {
    let now = Local::now();
    let today = now.date_naive();

    match period {
        "5h" => {
            let today_str = today.format("%Y%m%d").to_string();
            Ok(parser.get_blocks(provider, &today_str))
        }
        "day" => {
            let target = today + chrono::Duration::days(offset as i64);
            let since_str = target.format("%Y%m%d").to_string();
            let mut payload = parser.get_hourly(provider, &since_str);
            payload.period_label = format_day_label(target);
            payload.has_earlier_data = parser.has_entries_before(provider, target);
            Ok(payload)
        }
        "week" => {
            let current_monday =
                today - chrono::Duration::days(now.weekday().num_days_from_monday() as i64);
            let target_monday = current_monday + chrono::Duration::days((offset * 7) as i64);
            let target_sunday = target_monday + chrono::Duration::days(6);
            let since_str = target_monday.format("%Y%m%d").to_string();
            let end_date = target_sunday + chrono::Duration::days(1);
            let mut payload = parser.get_daily(provider, &since_str);
            filter_buckets_to_range(&mut payload, target_monday, end_date);
            payload.period_label = format_week_label(target_monday, target_sunday);
            payload.has_earlier_data = parser.has_entries_before(provider, target_monday);
            Ok(payload)
        }
        "month" => {
            let (year, month) = resolve_month_offset(now.year(), now.month(), offset);
            let first_of_month = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
            let end_of_month = first_of_next_month(year, month);
            let since_str = first_of_month.format("%Y%m%d").to_string();
            let mut payload = parser.get_daily(provider, &since_str);
            filter_buckets_to_range(&mut payload, first_of_month, end_of_month);
            payload.period_label = format_month_label(first_of_month);
            payload.has_earlier_data = parser.has_entries_before(provider, first_of_month);
            Ok(payload)
        }
        "year" => {
            let target_year = now.year() + offset;
            let first_of_year = NaiveDate::from_ymd_opt(target_year, 1, 1).unwrap();
            let end_of_year = NaiveDate::from_ymd_opt(target_year + 1, 1, 1).unwrap();
            let since_str = first_of_year.format("%Y%m%d").to_string();
            let mut payload = parser.get_monthly(provider, &since_str);
            filter_buckets_to_range(&mut payload, first_of_year, end_of_year);
            payload.period_label = format_year_label(target_year);
            payload.has_earlier_data = parser.has_entries_before(provider, first_of_year);
            Ok(payload)
        }
        _ => Err(format!("Unknown period: {period}")),
    }
}

fn apply_period_context(
    parser: &UsageParser,
    payload: &mut UsagePayload,
    provider: &str,
    period: &str,
    offset: i32,
) -> Result<(), String> {
    let now = Local::now();
    let today = now.date_naive();

    match period {
        "5h" => {
            payload.period_label.clear();
            payload.has_earlier_data = false;
        }
        "day" => {
            let target = today + chrono::Duration::days(offset as i64);
            payload.period_label = format_day_label(target);
            payload.has_earlier_data = parser.has_entries_before(provider, target);
        }
        "week" => {
            let current_monday =
                today - chrono::Duration::days(now.weekday().num_days_from_monday() as i64);
            let target_monday = current_monday + chrono::Duration::days((offset * 7) as i64);
            let target_sunday = target_monday + chrono::Duration::days(6);
            payload.period_label = format_week_label(target_monday, target_sunday);
            payload.has_earlier_data = parser.has_entries_before(provider, target_monday);
        }
        "month" => {
            let (year, month) = resolve_month_offset(now.year(), now.month(), offset);
            let first_of_month = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
            payload.period_label = format_month_label(first_of_month);
            payload.has_earlier_data = parser.has_entries_before(provider, first_of_month);
        }
        "year" => {
            let target_year = now.year() + offset;
            let first_of_year = NaiveDate::from_ymd_opt(target_year, 1, 1).unwrap();
            payload.period_label = format_year_label(target_year);
            payload.has_earlier_data = parser.has_entries_before(provider, first_of_year);
        }
        _ => return Err(format!("Unknown period: {period}")),
    }

    Ok(())
}

fn attach_local_stats(
    parser: &UsageParser,
    payload: &mut UsagePayload,
    provider: &str,
    period: &str,
    offset: i32,
) {
    if let Some((start_date, end_date)) = compute_date_bounds(period, offset) {
        let (mut entries, mut change_events, _reports) =
            parser.load_entries(provider, Some(start_date));

        change_events.retain(|event| {
            let date = event.timestamp.date_naive();
            date >= start_date && date < end_date
        });
        entries.retain(|entry| {
            let date = entry.timestamp.date_naive();
            date >= start_date && date < end_date
        });

        payload.change_stats =
            aggregate_change_stats(&change_events, payload.total_cost, payload.total_tokens);
        for model in &mut payload.model_breakdown {
            model.change_stats = aggregate_model_change_summary(&change_events, &model.model_key);
        }

        if period != "5h" && payload.usage_source == UsageSource::Parser {
            payload.input_tokens = entries.iter().map(|entry| entry.input_tokens).sum();
            payload.output_tokens = entries.iter().map(|entry| entry.output_tokens).sum();
        }

        payload.subagent_stats = crate::stats::subagent::aggregate_subagent_stats(
            &entries,
            &change_events,
            payload.total_cost,
        );
    }
}

fn final_usage_cache_key(provider: &str, period: &str, offset: i32) -> String {
    format!("usage-view:{provider}:{period}:{offset}")
}

async fn finalize_usage_payload(
    state: &AppState,
    provider: &str,
    period: &str,
    offset: i32,
    mut payload: UsagePayload,
) -> UsagePayload {
    payload.device_breakdown =
        crate::commands::ssh::build_device_breakdown_for_payload(state, provider, period, offset)
            .await;
    payload.device_chart_buckets =
        crate::commands::ssh::build_device_time_chart_buckets(state, provider, period, offset)
            .await;

    if let Some(included) =
        crate::commands::ssh::build_included_devices_payload(state, provider, period, offset).await
    {
        payload = merge_payloads(payload, included);
    }

    payload
}

pub(crate) fn get_provider_data(
    parser: &UsageParser,
    provider: &str,
    period: &str,
    offset: i32,
) -> Result<UsagePayload, String> {
    // Single cache layer: stores the complete payload including stats.
    let cache_key = format!("full:{}:{}:{}", provider, period, offset);
    if let Some(cached) = parser.check_cache(&cache_key) {
        return Ok(cached);
    }

    let mut payload = parser_payload_for_period(parser, provider, period, offset)?;

    apply_period_context(parser, &mut payload, provider, period, offset)?;
    attach_local_stats(parser, &mut payload, provider, period, offset);

    // Store the complete payload so cache hits skip everything above.
    parser.store_cache(&cache_key, payload.clone());

    Ok(payload)
}

fn merge_usage_source(left: UsageSource, right: UsageSource) -> UsageSource {
    if left == right {
        left
    } else {
        UsageSource::Mixed
    }
}

fn merge_usage_warning(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (None, None) => None,
        (Some(warning), None) | (None, Some(warning)) => Some(warning),
        (Some(left), Some(right)) if left == right => Some(left),
        (Some(left), Some(right)) => Some(format!("{left}\n{right}")),
    }
}

fn merge_payloads(mut c: UsagePayload, x: UsagePayload) -> UsagePayload {
    let mut bucket_map: BTreeMap<String, ChartBucket> = BTreeMap::new();
    let c_buckets = std::mem::take(&mut c.chart_buckets);
    for b in c_buckets.into_iter().chain(x.chart_buckets.into_iter()) {
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
    for model in c_models.into_iter().chain(x.model_breakdown.into_iter()) {
        let entry = model_map
            .entry(model.model_key.clone())
            .or_insert_with(|| ModelSummary {
                display_name: model.display_name,
                model_key: model.model_key,
                cost: 0.0,
                tokens: 0,
                change_stats: None,
            });
        entry.cost += model.cost;
        entry.tokens += model.tokens;
    }

    c.total_cost += x.total_cost;
    c.total_tokens += x.total_tokens;
    c.input_tokens += x.input_tokens;
    c.output_tokens += x.output_tokens;

    if let Some(ref mut c_stats) = c.subagent_stats {
        if let Some(x_stats) = x.subagent_stats {
            c_stats.main.cost += x_stats.main.cost;
            c_stats.main.input_tokens += x_stats.main.input_tokens;
            c_stats.main.output_tokens += x_stats.main.output_tokens;
            c_stats.main.cache_read_tokens += x_stats.main.cache_read_tokens;
            c_stats.subagents.cost += x_stats.subagents.cost;
            c_stats.subagents.input_tokens += x_stats.subagents.input_tokens;
            c_stats.subagents.output_tokens += x_stats.subagents.output_tokens;
            c_stats.subagents.cache_read_tokens += x_stats.subagents.cache_read_tokens;
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
    let Some((start_date, end_date)) = compute_date_bounds(period, offset) else {
        return Vec::new();
    };

    let (_entries, mut change_events, _reports) = parser.load_entries(provider, Some(start_date));
    change_events.retain(|event| {
        let date = event.timestamp.date_naive();
        date >= start_date && date < end_date
    });
    change_events
}

#[tauri::command]
pub async fn get_known_models(
    provider: String,
    state: State<'_, AppState>,
) -> Result<Vec<KnownModel>, String> {
    parse_usage_selection(&provider)?;

    let (entries, _, _) = state.parser.load_entries(&provider, None);
    let mut models = BTreeMap::<String, KnownModel>::new();
    for entry in entries {
        let model = crate::models::known_model_from_raw(&entry.model);
        models.entry(model.model_key.clone()).or_insert(model);
    }
    Ok(models.into_values().collect())
}

#[tauri::command]
pub async fn get_usage_data(
    provider: String,
    period: String,
    offset: i32,
    state: State<'_, AppState>,
) -> Result<UsagePayload, String> {
    get_usage_data_inner(&state, &provider, &period, offset).await
}

pub(crate) async fn get_usage_data_inner(
    state: &AppState,
    provider: &str,
    period: &str,
    offset: i32,
) -> Result<UsagePayload, String> {
    let parser = &state.parser;
    let selection = parse_usage_selection(provider)?;
    let final_cache_key = final_usage_cache_key(provider, period, offset);
    if let Some(cached) = parser.check_cache(&final_cache_key) {
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
        return Ok(cached);
    }

    let payload = match selection {
        UsageIntegrationSelection::Single(integration_id) => {
            let payload = get_provider_data(parser, provider, period, offset)?;
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

            finalize_usage_payload(state, provider, period, offset, payload).await
        }
        UsageIntegrationSelection::All => {
            let mut merged: Option<UsagePayload> = None;
            let mut queries = Vec::new();

            for integration_id in all_usage_integrations() {
                let mut payload =
                    get_provider_data(parser, integration_id.as_str(), period, offset)?;
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
                    requested_provider: String::from(ALL_USAGE_INTEGRATIONS_ID),
                    period: Some(period.to_string()),
                    offset: Some(offset),
                    year: None,
                    month: None,
                    queries,
                },
            )
            .await;

            // Re-aggregate change stats and subagent stats from all providers' entries
            // in a single load_entries call instead of two separate calls.
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

            finalize_usage_payload(state, provider, period, offset, merged).await
        }
    };

    parser.store_cache(&final_cache_key, payload.clone());
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::parser::UsageParser;
    use crate::usage::ssh_remote::{SshCacheManager, SshHostConfig};
    use chrono::Local;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::TempDir;

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
            usage_source: UsageSource::Ccusage,
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

        assert_eq!(payload.chart_buckets.len(), 1);
        assert!(
            payload.active_block.is_some(),
            "codex 5h should use block payloads"
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
    fn claude_day_view_falls_back_to_parser_with_warning() {
        let dir = TempDir::new().unwrap();
        let now = Local::now();
        let ts = (now - chrono::Duration::hours(1)).to_rfc3339();
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
        let ts = (now - chrono::Duration::hours(1)).to_rfc3339();
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
    fn clearing_usage_view_cache_keeps_provider_cache_entries() {
        let dir = TempDir::new().unwrap();
        let parser = UsageParser::with_claude_dir(dir.path().to_path_buf());
        let full_key = String::from("full:claude:day:0");
        let final_key = final_usage_cache_key("claude", "day", 0);

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
        let payload = get_usage_data_inner(&state, "all", "day", 0).await.unwrap();

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
        let payload = get_usage_data_inner(&state, "claude", "day", 0)
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
        let payload = get_usage_data_inner(&state, "codex", "day", 0)
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
}
