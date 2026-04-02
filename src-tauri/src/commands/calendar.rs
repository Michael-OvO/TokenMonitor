use super::period::{first_of_next_month, month_offset_from_now};
use super::usage_query::get_provider_data;
use super::{
    maybe_capture_query_debug, parse_usage_selection, set_last_usage_debug, AppState,
    UsageDebugReport,
};
use crate::models::*;
use crate::usage::integrations::{all_usage_integrations, UsageIntegrationSelection};
use crate::usage::parser::UsageQueryDebugReport;
use chrono::{Datelike, NaiveDate};
use std::collections::HashMap;
use tauri::State;

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

fn calendar_days_from_payload(
    payload: &UsagePayload,
    month_start: NaiveDate,
    end_date: NaiveDate,
) -> Vec<CalendarDay> {
    payload
        .chart_buckets
        .iter()
        .filter_map(|bucket| {
            let date = NaiveDate::parse_from_str(&bucket.sort_key, "%Y-%m-%d").ok()?;
            if date >= month_start && date < end_date {
                Some(CalendarDay {
                    day: date.day(),
                    cost: bucket.total,
                })
            } else {
                None
            }
        })
        .collect()
}

fn merge_calendar_days(days: &mut Vec<CalendarDay>, incoming: Vec<CalendarDay>) {
    let mut day_map: HashMap<u32, f64> = HashMap::new();
    for day in days.drain(..).chain(incoming.into_iter()) {
        *day_map.entry(day.day).or_insert(0.0) += day.cost;
    }

    let mut merged: Vec<CalendarDay> = day_map
        .into_iter()
        .map(|(day, cost)| CalendarDay { day, cost })
        .collect();
    merged.sort_by_key(|day| day.day);
    *days = merged;
}

pub(crate) fn get_monthly_usage_with_debug_sync(
    state: &AppState,
    provider: &str,
    year: i32,
    month: u32,
) -> Result<(MonthlyUsagePayload, Vec<UsageQueryDebugReport>), String> {
    let selection = parse_usage_selection(provider)?;
    let month_offset = month_offset_from_now(year, month);
    let month_start = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| format!("Invalid date: year={year}, month={month}"))?;

    let end_date = first_of_next_month(year, month);

    #[allow(clippy::type_complexity)]
    let fetch_for_provider = |prov: &str| -> Result<
        (
            Vec<CalendarDay>,
            UsageSource,
            Option<String>,
            Option<UsageQueryDebugReport>,
        ),
        String,
    > {
        let usage = get_provider_data(&state.parser, prov, "month", month_offset)?;
        let query = maybe_capture_query_debug(&state.parser, &usage)?;
        let days = calendar_days_from_payload(&usage, month_start, end_date);
        Ok((days, usage.usage_source, usage.usage_warning, query))
    };

    let (days, usage_source, usage_warning, queries) = match selection {
        UsageIntegrationSelection::All => {
            let mut day_map: HashMap<u32, f64> = HashMap::new();
            let mut queries = Vec::new();
            let mut usage_source = UsageSource::Parser;
            let mut usage_warning = None;
            let mut initialized = false;

            for integration_id in all_usage_integrations() {
                let (integration_days, source, warning, query) =
                    fetch_for_provider(integration_id.as_str())?;
                if let Some(query) = query {
                    queries.push(query);
                }
                let warning =
                    warning.map(|warning| format!("{}: {warning}", integration_id.display_name()));
                if !initialized {
                    usage_source = source;
                    usage_warning = warning;
                    initialized = true;
                } else {
                    usage_source = merge_usage_source(usage_source, source);
                    usage_warning = merge_usage_warning(usage_warning, warning);
                }
                for day in integration_days {
                    *day_map.entry(day.day).or_insert(0.0) += day.cost;
                }
            }

            let mut merged: Vec<CalendarDay> = day_map
                .into_iter()
                .map(|(day, cost)| CalendarDay { day, cost })
                .collect();
            merged.sort_by_key(|d| d.day);
            (merged, usage_source, usage_warning, queries)
        }
        UsageIntegrationSelection::Single(integration_id) => {
            let (days, source, warning, query) = fetch_for_provider(integration_id.as_str())?;
            (days, source, warning, query.into_iter().collect())
        }
    };

    let total_cost: f64 = days.iter().map(|d| d.cost).sum();
    Ok((
        MonthlyUsagePayload {
            year,
            month,
            days,
            total_cost,
            usage_source,
            usage_warning,
        },
        queries,
    ))
}

async fn get_monthly_usage_with_debug(
    state: &AppState,
    provider: &str,
    year: i32,
    month: u32,
) -> Result<(MonthlyUsagePayload, Vec<UsageQueryDebugReport>), String> {
    let month_offset = month_offset_from_now(year, month);
    let month_start = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| format!("Invalid date: year={year}, month={month}"))?;
    let end_date = first_of_next_month(year, month);

    let (mut payload, queries) = get_monthly_usage_with_debug_sync(state, provider, year, month)?;

    if let Some(included) =
        crate::commands::ssh::build_included_devices_payload(state, provider, "month", month_offset)
            .await
    {
        let included_days = calendar_days_from_payload(&included, month_start, end_date);
        payload.usage_source = merge_usage_source(payload.usage_source, included.usage_source);
        payload.usage_warning =
            merge_usage_warning(payload.usage_warning.take(), included.usage_warning);
        merge_calendar_days(&mut payload.days, included_days);
        payload.total_cost = payload.days.iter().map(|day| day.cost).sum();
    }

    Ok((payload, queries))
}

#[cfg(test)]
fn get_monthly_usage_sync(
    state: &AppState,
    provider: &str,
    year: i32,
    month: u32,
) -> MonthlyUsagePayload {
    get_monthly_usage_with_debug_sync(state, provider, year, month)
        .map(|(payload, _)| payload)
        .expect("monthly usage debug capture should be available")
}

#[tauri::command]
pub async fn get_monthly_usage(
    provider: String,
    year: i32,
    month: u32,
    state: State<'_, AppState>,
) -> Result<MonthlyUsagePayload, String> {
    let (payload, queries) = get_monthly_usage_with_debug(&state, &provider, year, month).await?;
    set_last_usage_debug(
        &state,
        UsageDebugReport {
            request_kind: String::from("calendar-month"),
            requested_provider: provider,
            period: None,
            offset: None,
            year: Some(year),
            month: Some(month),
            queries,
        },
    )
    .await;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::parser::UsageParser;
    use crate::usage::ssh_remote::{SshCacheManager, SshHostConfig};
    use chrono::{Datelike, Local};
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::TempDir;

    impl AppState {
        fn with_parser(parser: UsageParser) -> Self {
            Self {
                parser: Arc::new(parser),
                ..Self::new()
            }
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

    fn remote_record(ts: &str, model: &str, input_tokens: u64, output_tokens: u64) -> String {
        format!(
            r#"{{"ts":"{ts}","m":"{model}","in":{input_tokens},"out":{output_tokens},"c5":0,"cr":0}}"#
        )
    }

    async fn build_state_with_remote_claude_month_data() -> (AppState, TempDir, TempDir, TempDir) {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let app_data_dir = TempDir::new().unwrap();
        let now = Local::now();
        let timestamp = now.to_rfc3339();

        write_file(
            &claude_dir.path().join("session.jsonl"),
            &format!(
                r#"{{"type":"assistant","timestamp":"{timestamp}","message":{{"model":"claude-sonnet-4-6-20260301","usage":{{"input_tokens":1000,"output_tokens":500}},"stop_reason":"end_turn"}}}}"#
            ),
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
    fn get_monthly_usage_returns_per_day_costs() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let project_dir = claude_dir.path().join("test-project");
        fs::create_dir_all(&project_dir).unwrap();

        let content = r#"{"type":"assistant","timestamp":"2026-03-05T10:00:00-04:00","message":{"model":"claude-sonnet-4-6-20260301","usage":{"input_tokens":1000,"output_tokens":500},"stop_reason":"end_turn"}}"#;
        write_file(&project_dir.join("session.jsonl"), content);

        let parser = UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        );
        let state = AppState::with_parser(parser);

        let payload = get_monthly_usage_sync(&state, "claude", 2026, 3);
        assert_eq!(payload.year, 2026);
        assert_eq!(payload.month, 3);
        assert!(!payload.days.is_empty(), "should have at least one day");
        let day5 = payload.days.iter().find(|d| d.day == 5);
        assert!(day5.is_some(), "should have data for day 5");
        assert!(day5.unwrap().cost > 0.0, "day 5 should have non-zero cost");
        assert!(payload.total_cost > 0.0);
    }

    #[test]
    fn get_monthly_usage_filters_to_requested_month() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let project_dir = claude_dir.path().join("test-project");
        fs::create_dir_all(&project_dir).unwrap();

        let feb_entry = r#"{"type":"assistant","timestamp":"2026-02-15T10:00:00-04:00","message":{"model":"claude-sonnet-4-6-20260301","usage":{"input_tokens":1000,"output_tokens":500},"stop_reason":"end_turn"}}"#;
        let mar_entry = r#"{"type":"assistant","timestamp":"2026-03-10T10:00:00-04:00","message":{"model":"claude-sonnet-4-6-20260301","usage":{"input_tokens":2000,"output_tokens":1000},"stop_reason":"end_turn"}}"#;
        write_file(
            &project_dir.join("session.jsonl"),
            &format!("{}\n{}", feb_entry, mar_entry),
        );

        let parser = UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        );
        let state = AppState::with_parser(parser);

        let payload = get_monthly_usage_sync(&state, "claude", 2026, 2);
        assert_eq!(payload.month, 2);
        for day in &payload.days {
            assert!(day.day <= 28, "Feb 2026 has no day > 28");
        }
        assert!(payload.days.iter().any(|d| d.day == 15));
    }

    #[test]
    fn get_monthly_usage_merges_providers_for_all() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();

        let claude_project = claude_dir.path().join("test-project");
        fs::create_dir_all(&claude_project).unwrap();
        let claude_entry = r#"{"type":"assistant","timestamp":"2026-03-05T10:00:00-04:00","message":{"model":"claude-sonnet-4-6-20260301","usage":{"input_tokens":1000,"output_tokens":500},"stop_reason":"end_turn"}}"#;
        write_file(&claude_project.join("session.jsonl"), claude_entry);

        let day_dir = codex_dir.path().join("2026").join("03").join("05");
        fs::create_dir_all(&day_dir).unwrap();
        let codex_entry = r#"{"type":"event_msg","timestamp":"2026-03-05T14:00:00-04:00","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":500,"output_tokens":250,"reasoning_output_tokens":0,"cached_input_tokens":0}}}}"#;
        write_file(&day_dir.join("session.jsonl"), codex_entry);

        let parser = UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        );
        let state = AppState::with_parser(parser);

        let payload = get_monthly_usage_sync(&state, "all", 2026, 3);
        let day5 = payload.days.iter().find(|d| d.day == 5);
        assert!(day5.is_some(), "should have merged day 5");
        let claude_only = get_monthly_usage_sync(&state, "claude", 2026, 3);
        let codex_only = get_monthly_usage_sync(&state, "codex", 2026, 3);
        let claude_day5_cost = claude_only
            .days
            .iter()
            .find(|d| d.day == 5)
            .map(|d| d.cost)
            .unwrap_or(0.0);
        let codex_day5_cost = codex_only
            .days
            .iter()
            .find(|d| d.day == 5)
            .map(|d| d.cost)
            .unwrap_or(0.0);
        assert!(
            (day5.unwrap().cost - (claude_day5_cost + codex_day5_cost)).abs() < 0.001,
            "merged cost should equal sum of individual provider costs"
        );
    }

    #[test]
    fn get_monthly_usage_skips_stale_query_debug_on_full_cache_hit() {
        let claude_dir = TempDir::new().unwrap();
        let codex_dir = TempDir::new().unwrap();
        let project_dir = claude_dir.path().join("test-project");
        fs::create_dir_all(&project_dir).unwrap();

        let now = Local::now();
        let current_month = NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap();
        let content = format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"model":"claude-sonnet-4-6-20260301","usage":{{"input_tokens":1000,"output_tokens":500}},"stop_reason":"end_turn"}}}}"#,
            local_timestamp(current_month, 10)
        );
        write_file(&project_dir.join("session.jsonl"), &content);

        let parser = UsageParser::with_dirs(
            claude_dir.path().to_path_buf(),
            codex_dir.path().to_path_buf(),
        );
        let state = AppState::with_parser(parser);

        let (first_payload, first_queries) =
            get_monthly_usage_with_debug_sync(&state, "claude", now.year(), now.month()).unwrap();
        assert!(
            !first_payload.days.is_empty(),
            "expected in-range monthly usage"
        );
        assert_eq!(
            first_queries.len(),
            1,
            "cold month request should capture debug"
        );

        let (_second_payload, second_queries) =
            get_monthly_usage_with_debug_sync(&state, "claude", now.year(), now.month()).unwrap();
        assert!(
            second_queries.is_empty(),
            "full-cache month hit should not reuse stale parser debug"
        );
    }

    #[tokio::test]
    async fn get_monthly_usage_merges_included_remote_devices() {
        let (state, _claude_dir, _codex_dir, _app_data_dir) =
            build_state_with_remote_claude_month_data().await;
        let now = Local::now();

        let (local_claude, _) =
            get_monthly_usage_with_debug_sync(&state, "claude", now.year(), now.month()).unwrap();
        let (merged_claude, _) =
            get_monthly_usage_with_debug(&state, "claude", now.year(), now.month())
                .await
                .unwrap();
        let (local_all, _) =
            get_monthly_usage_with_debug_sync(&state, "all", now.year(), now.month()).unwrap();
        let (merged_all, _) = get_monthly_usage_with_debug(&state, "all", now.year(), now.month())
            .await
            .unwrap();
        let (local_codex, _) =
            get_monthly_usage_with_debug_sync(&state, "codex", now.year(), now.month()).unwrap();
        let (merged_codex, _) =
            get_monthly_usage_with_debug(&state, "codex", now.year(), now.month())
                .await
                .unwrap();

        let target_day = now.day();
        let local_claude_day = local_claude
            .days
            .iter()
            .find(|day| day.day == target_day)
            .map(|day| day.cost)
            .unwrap_or(0.0);
        let merged_claude_day = merged_claude
            .days
            .iter()
            .find(|day| day.day == target_day)
            .map(|day| day.cost)
            .unwrap_or(0.0);

        assert!(
            merged_claude.total_cost > local_claude.total_cost,
            "eligible monthly calendar payloads should include remote device cost"
        );
        assert!(
            merged_all.total_cost > local_all.total_cost,
            "all-provider monthly calendar payloads should include remote device cost"
        );
        assert_eq!(
            merged_codex.total_cost, local_codex.total_cost,
            "codex monthly calendar payloads should stay local-only"
        );
        assert!(
            merged_claude_day > local_claude_day,
            "the affected calendar day should increase after merging remote usage"
        );
    }
}
