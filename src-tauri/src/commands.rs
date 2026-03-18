use crate::models::*;
use crate::parser::{UsageParser, UsageQueryDebugReport};
use chrono::{Datelike, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tauri::{Manager, State};
use tokio::sync::RwLock;

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSColor, NSView, NSWindow};
#[cfg(target_os = "macos")]
use objc2_quartz_core::CALayer;
#[cfg(target_os = "macos")]
use tauri::AppHandle;
#[cfg(target_os = "macos")]
use tokio::sync::oneshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayConfig {
    pub bar_display: String,       // "off" | "single" | "both"
    pub bar_provider: String,      // "claude" | "codex"
    pub show_percentages: bool,
    pub percentage_format: String, // "compact" | "verbose"
    pub show_cost: bool,
    pub cost_precision: String,    // "whole" | "full"
}

impl Default for TrayConfig {
    fn default() -> Self {
        Self {
            bar_display: "both".to_string(),
            bar_provider: "claude".to_string(),
            show_percentages: false,
            percentage_format: "compact".to_string(),
            show_cost: true,
            cost_precision: "full".to_string(),
        }
    }
}

pub struct AppState {
    pub parser: Arc<UsageParser>,
    pub refresh_interval: Arc<RwLock<u64>>,
    pub tray_config: Arc<RwLock<TrayConfig>>,
    pub last_usage_debug: Arc<RwLock<Option<UsageDebugReport>>>,
    pub cached_rate_limits: Arc<RwLock<Option<RateLimitsPayload>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            parser: Arc::new(UsageParser::new()),
            refresh_interval: Arc::new(RwLock::new(30)),
            tray_config: Arc::new(RwLock::new(TrayConfig::default())),
            last_usage_debug: Arc::new(RwLock::new(None)),
            cached_rate_limits: Arc::new(RwLock::new(None)),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDebugReport {
    pub request_kind: String,
    pub requested_provider: String,
    pub period: Option<String>,
    pub offset: Option<i32>,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub queries: Vec<UsageQueryDebugReport>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSurface {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    #[serde(default = "default_surface_alpha")]
    pub alpha: u8,
}

const DEFAULT_WINDOW_CORNER_RADIUS: f64 = 14.0;
const DEFAULT_DARK_SURFACE: WindowSurface = WindowSurface {
    red: 0x14,
    green: 0x14,
    blue: 0x16,
    alpha: 0xFF,
};

const fn default_surface_alpha() -> u8 {
    0xFF
}

#[cfg(target_os = "macos")]
fn apply_window_surface(
    window: &tauri::WebviewWindow,
    surface: WindowSurface,
    corner_radius: f64,
) -> Result<(), String> {
    let ns_window = window
        .ns_window()
        .map_err(|e| format!("Failed to access NSWindow: {e}"))?;
    let ns_window = unsafe { &*(ns_window.cast::<NSWindow>()) };

    let color = NSColor::colorWithSRGBRed_green_blue_alpha(
        f64::from(surface.red) / 255.0,
        f64::from(surface.green) / 255.0,
        f64::from(surface.blue) / 255.0,
        f64::from(surface.alpha) / 255.0,
    );
    let cg_color = color.CGColor();
    let clear = NSColor::clearColor();

    ns_window.setOpaque(false);
    // Keep the window itself clear so the clipped corner triangles stay
    // transparent, and let the native content-view layer provide the
    // surface fill that resizes with the window.
    ns_window.setBackgroundColor(Some(&clear));

    let content_view = ns_window
        .contentView()
        .ok_or_else(|| String::from("NSWindow is missing a content view"))?;
    let content_view: &NSView = &content_view;

    content_view.setWantsLayer(true);
    let layer = match content_view.layer() {
        Some(layer) => layer,
        None => {
            let layer = CALayer::layer();
            content_view.setLayer(Some(&layer));
            layer
        }
    };

    layer.setBackgroundColor(Some(&cg_color));
    layer.setCornerRadius(corner_radius);
    layer.setMasksToBounds(true);
    layer.setOpaque(false);

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn apply_default_window_surface(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| String::from("Main window not found"))?;
    apply_window_surface(&window, DEFAULT_DARK_SURFACE, DEFAULT_WINDOW_CORNER_RADIUS)
}

fn format_tray_title(config: &TrayConfig, total_cost: f64) -> String {
    let mut parts: Vec<String> = Vec::new();

    if config.show_cost {
        if config.cost_precision == "whole" {
            parts.push(format!("${}", total_cost.round() as i64));
        } else {
            parts.push(format!("${:.2}", total_cost));
        }
    }

    parts.join("  ")
}

pub async fn sync_tray_title(app: &tauri::AppHandle, state: &AppState) {
    let config = state.tray_config.read().await.clone();
    let title = if config.show_cost {
        let today = Local::now().format("%Y%m%d").to_string();
        let claude = state.parser.get_daily("claude", &today);
        let codex = state.parser.get_daily("codex", &today);
        format_tray_title(&config, claude.total_cost + codex.total_cost)
    } else {
        String::new()
    };

    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_title(Some(title));
    }
}

async fn set_last_usage_debug(state: &AppState, report: UsageDebugReport) {
    let mut current = state.last_usage_debug.write().await;
    *current = Some(report);
}

fn capture_query_debug(parser: &UsageParser) -> Result<UsageQueryDebugReport, String> {
    parser
        .last_query_debug()
        .ok_or_else(|| String::from("Usage debug report was not available"))
}

#[tauri::command]
pub async fn set_window_surface(
    app: tauri::AppHandle,
    surface: WindowSurface,
    corner_radius: Option<f64>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let window = app
            .get_webview_window("main")
            .ok_or_else(|| String::from("Main window not found"))?;
        let next_radius = corner_radius.unwrap_or(DEFAULT_WINDOW_CORNER_RADIUS);
        let (tx, rx) = oneshot::channel();
        let window_for_main_thread = window.clone();

        window
            .run_on_main_thread(move || {
                let _ = tx.send(apply_window_surface(
                    &window_for_main_thread,
                    surface,
                    next_radius,
                ));
            })
            .map_err(|e| format!("Failed to schedule native window surface update: {e}"))?;

        return rx
            .await
            .map_err(|_| String::from("Native window surface update was cancelled"))?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, surface, corner_radius);
        Ok(())
    }
}

#[tauri::command]
pub async fn set_refresh_interval(interval: u64, state: State<'_, AppState>) -> Result<(), String> {
    let mut current = state.refresh_interval.write().await;
    *current = interval;
    Ok(())
}

#[tauri::command]
pub async fn set_tray_config(
    config: TrayConfig,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut current = state.tray_config.write().await;
    *current = config;
    drop(current);

    sync_tray_title(&app, &state).await;

    Ok(())
}

#[tauri::command]
pub async fn clear_cache(state: State<'_, AppState>) -> Result<(), String> {
    state.parser.clear_cache();
    Ok(())
}

#[tauri::command]
pub async fn get_rate_limits(
    provider: Option<String>,
    state: State<'_, AppState>,
) -> Result<RateLimitsPayload, String> {
    let selection = match provider.as_deref() {
        None | Some("all") => crate::rate_limits::RateLimitSelection::All,
        Some("claude") => crate::rate_limits::RateLimitSelection::Claude,
        Some("codex") => crate::rate_limits::RateLimitSelection::Codex,
        Some(other) => return Err(format!("Invalid provider for rate limits: {other}")),
    };

    let codex_dir = state.parser.codex_dir().to_path_buf();
    let cached = state.cached_rate_limits.read().await.clone();
    let fresh =
        crate::rate_limits::fetch_selected_rate_limits(&codex_dir, selection, cached.as_ref())
            .await;

    let merged = crate::rate_limits::merge_rate_limits(fresh, cached.as_ref());

    *state.cached_rate_limits.write().await = Some(merged.clone());
    Ok(merged)
}

#[tauri::command]
pub async fn get_last_usage_debug(
    state: State<'_, AppState>,
) -> Result<Option<UsageDebugReport>, String> {
    Ok(state.last_usage_debug.read().await.clone())
}

#[tauri::command]
pub async fn get_known_models(
    provider: String,
    state: State<'_, AppState>,
) -> Result<Vec<KnownModel>, String> {
    match provider.as_str() {
        "claude" | "codex" | "all" => {
            let (entries, _) = state.parser.load_entries(&provider, None);
            let mut models = BTreeMap::<String, KnownModel>::new();
            for entry in entries {
                let model = crate::models::known_model_from_raw(&entry.model);
                models.entry(model.model_key.clone()).or_insert(model);
            }
            Ok(models.into_values().collect())
        }
        _ => Err(format!("Unknown provider: {}", provider)),
    }
}

#[tauri::command]
pub async fn get_usage_data(
    provider: String,
    period: String,
    offset: i32,
    state: State<'_, AppState>,
) -> Result<UsagePayload, String> {
    let parser = &state.parser;

    match provider.as_str() {
        "claude" | "codex" => {
            let payload = get_provider_data(parser, &provider, &period, offset)?;
            let query = capture_query_debug(parser)?;
            set_last_usage_debug(
                &state,
                UsageDebugReport {
                    request_kind: String::from("usage"),
                    requested_provider: provider,
                    period: Some(period),
                    offset: Some(offset),
                    year: None,
                    month: None,
                    queries: vec![query],
                },
            )
            .await;
            Ok(payload)
        }
        "all" => {
            let claude = get_provider_data(parser, "claude", &period, offset)?;
            let claude_query = capture_query_debug(parser)?;
            let codex = get_provider_data(parser, "codex", &period, offset)?;
            let codex_query = capture_query_debug(parser)?;
            set_last_usage_debug(
                &state,
                UsageDebugReport {
                    request_kind: String::from("usage"),
                    requested_provider: provider,
                    period: Some(period),
                    offset: Some(offset),
                    year: None,
                    month: None,
                    queries: vec![claude_query, codex_query],
                },
            )
            .await;
            Ok(merge_payloads(claude, codex))
        }
        _ => Err(format!("Unknown provider: {}", provider)),
    }
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
        })
        .collect();

    // Recalculate input/output tokens
    payload.input_tokens = 0;
    payload.output_tokens = 0;
}

fn parse_bucket_start_date(sort_key: &str) -> Result<NaiveDate, chrono::ParseError> {
    NaiveDate::parse_from_str(sort_key, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(&format!("{sort_key}-01"), "%Y-%m-%d"))
}

fn get_provider_data(
    parser: &UsageParser,
    provider: &str,
    period: &str,
    offset: i32,
) -> Result<UsagePayload, String> {
    let now = Local::now();
    let today = now.date_naive();

    let mut payload = match period {
        "5h" => {
            let today_str = today.format("%Y%m%d").to_string();
            parser.get_blocks(provider, &today_str)
        }
        "day" => {
            let target = today + chrono::Duration::days(offset as i64);
            let since_str = target.format("%Y%m%d").to_string();
            let mut p = parser.get_hourly(provider, &since_str);
            p.period_label = format_day_label(target);
            p.has_earlier_data = parser.has_entries_before(provider, target);
            p
        }
        "week" => {
            let current_monday =
                today - chrono::Duration::days(now.weekday().num_days_from_monday() as i64);
            let target_monday = current_monday + chrono::Duration::days((offset * 7) as i64);
            let target_sunday = target_monday + chrono::Duration::days(6);
            let since_str = target_monday.format("%Y%m%d").to_string();
            let end_date = target_sunday + chrono::Duration::days(1);
            let mut p = parser.get_daily(provider, &since_str);
            filter_buckets_to_range(&mut p, target_monday, end_date);
            p.period_label = format_week_label(target_monday, target_sunday);
            p.has_earlier_data = parser.has_entries_before(provider, target_monday);
            p
        }
        "month" => {
            let mut target_year = now.year();
            let mut target_month = now.month() as i32 + offset;
            while target_month <= 0 {
                target_year -= 1;
                target_month += 12;
            }
            while target_month > 12 {
                target_year += 1;
                target_month -= 12;
            }
            let first_of_month =
                NaiveDate::from_ymd_opt(target_year, target_month as u32, 1).unwrap();
            let end_of_month = if target_month == 12 {
                NaiveDate::from_ymd_opt(target_year + 1, 1, 1).unwrap()
            } else {
                NaiveDate::from_ymd_opt(target_year, (target_month + 1) as u32, 1).unwrap()
            };
            let since_str = first_of_month.format("%Y%m%d").to_string();
            let mut p = parser.get_daily(provider, &since_str);
            filter_buckets_to_range(&mut p, first_of_month, end_of_month);
            p.period_label = format_month_label(first_of_month);
            p.has_earlier_data = parser.has_entries_before(provider, first_of_month);
            p
        }
        "year" => {
            let target_year = now.year() + offset;
            let first_of_year = NaiveDate::from_ymd_opt(target_year, 1, 1).unwrap();
            let end_of_year = NaiveDate::from_ymd_opt(target_year + 1, 1, 1).unwrap();
            let since_str = first_of_year.format("%Y%m%d").to_string();
            let mut p = parser.get_monthly(provider, &since_str);
            filter_buckets_to_range(&mut p, first_of_year, end_of_year);
            p.period_label = format_year_label(target_year);
            p.has_earlier_data = parser.has_entries_before(provider, first_of_year);
            p
        }
        _ => return Err(format!("Unknown period: {}", period)),
    };

    if period == "5h" {
        payload.period_label = String::new();
        payload.has_earlier_data = false;
    }

    Ok(payload)
}

fn merge_payloads(mut c: UsagePayload, x: UsagePayload) -> UsagePayload {
    let mut bucket_map: BTreeMap<String, ChartBucket> = BTreeMap::new();
    for b in c.chart_buckets.iter().chain(x.chart_buckets.iter()) {
        let entry = bucket_map
            .entry(b.sort_key.clone())
            .or_insert_with(|| ChartBucket {
                label: b.label.clone(),
                sort_key: b.sort_key.clone(),
                total: 0.0,
                segments: vec![],
            });
        entry.total += b.total;
        entry.segments.extend(b.segments.clone());
    }

    let mut model_map: HashMap<String, ModelSummary> = HashMap::new();
    for model in c.model_breakdown.iter().chain(x.model_breakdown.iter()) {
        let entry = model_map
            .entry(model.model_key.clone())
            .or_insert_with(|| ModelSummary {
                display_name: model.display_name.clone(),
                model_key: model.model_key.clone(),
                cost: 0.0,
                tokens: 0,
            });
        entry.cost += model.cost;
        entry.tokens += model.tokens;
    }

    c.total_cost += x.total_cost;
    c.total_tokens += x.total_tokens;
    c.input_tokens += x.input_tokens;
    c.output_tokens += x.output_tokens;
    c.chart_buckets = bucket_map.into_values().collect();
    c.session_count = c.chart_buckets.iter().filter(|b| b.total > 0.0).count() as u32;
    c.model_breakdown = model_map.into_values().collect();
    c.active_block = merge_active_blocks(c.active_block, x.active_block);
    c.five_hour_cost += x.five_hour_cost;
    c.from_cache = c.from_cache && x.from_cache;
    c.has_earlier_data = c.has_earlier_data && x.has_earlier_data;
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

// ── Period label formatting ──

fn format_day_label(date: NaiveDate) -> String {
    date.format("%B %-d, %Y").to_string()
}

fn format_week_label(monday: NaiveDate, sunday: NaiveDate) -> String {
    if monday.year() != sunday.year() {
        format!(
            "{} \u{2013} {}",
            monday.format("%b %-d, %Y"),
            sunday.format("%b %-d, %Y")
        )
    } else if monday.month() != sunday.month() {
        format!(
            "{} \u{2013} {}",
            monday.format("%b %-d"),
            sunday.format("%b %-d, %Y")
        )
    } else {
        format!(
            "{} \u{2013} {}",
            monday.format("%b %-d"),
            sunday.format("%-d, %Y")
        )
    }
}

fn format_month_label(first_of_month: NaiveDate) -> String {
    first_of_month.format("%B %Y").to_string()
}

fn format_year_label(year: i32) -> String {
    year.to_string()
}

fn get_monthly_usage_with_debug_sync(
    state: &AppState,
    provider: &str,
    year: i32,
    month: u32,
) -> Result<(MonthlyUsagePayload, Vec<UsageQueryDebugReport>), String> {
    let month_start = NaiveDate::from_ymd_opt(year, month, 1)
        .unwrap()
        .format("%Y%m%d")
        .to_string();

    let end_date = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    };

    let fetch_for_provider =
        |prov: &str| -> Result<(Vec<CalendarDay>, UsageQueryDebugReport), String> {
            let usage = state.parser.get_daily(prov, &month_start);
            let query = capture_query_debug(&state.parser)?;
            let days = usage
                .chart_buckets
                .iter()
                .filter_map(|bucket| {
                    let date = NaiveDate::parse_from_str(&bucket.sort_key, "%Y-%m-%d").ok()?;
                    if date >= NaiveDate::from_ymd_opt(year, month, 1).unwrap() && date < end_date {
                        Some(CalendarDay {
                            day: date.day(),
                            cost: bucket.total,
                        })
                    } else {
                        None
                    }
                })
                .collect();
            Ok((days, query))
        };

    let (days, queries) = match provider {
        "all" => {
            let (claude_days, claude_query) = fetch_for_provider("claude")?;
            let (codex_days, codex_query) = fetch_for_provider("codex")?;
            let mut day_map: HashMap<u32, f64> = HashMap::new();
            for d in claude_days.iter().chain(codex_days.iter()) {
                *day_map.entry(d.day).or_insert(0.0) += d.cost;
            }
            let mut merged: Vec<CalendarDay> = day_map
                .into_iter()
                .map(|(day, cost)| CalendarDay { day, cost })
                .collect();
            merged.sort_by_key(|d| d.day);
            (merged, vec![claude_query, codex_query])
        }
        prov => {
            let (days, query) = fetch_for_provider(prov)?;
            (days, vec![query])
        }
    };

    let total_cost: f64 = days.iter().map(|d| d.cost).sum();
    Ok((
        MonthlyUsagePayload {
            year,
            month,
            days,
            total_cost,
        },
        queries,
    ))
}

#[allow(dead_code)]
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
    let (payload, queries) = get_monthly_usage_with_debug_sync(&state, &provider, year, month)?;
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
    use std::fs;
    use std::path::Path;
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
        }
    }

    fn payload_with_buckets(chart_buckets: Vec<ChartBucket>) -> UsagePayload {
        UsagePayload {
            total_cost: chart_buckets.iter().map(|bucket| bucket.total).sum(),
            total_tokens: 0,
            session_count: chart_buckets.len() as u32,
            input_tokens: 0,
            output_tokens: 0,
            chart_buckets,
            model_breakdown: vec![],
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            period_label: String::new(),
            has_earlier_data: false,
        }
    }

    fn write_file(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
    }

    #[test]
    fn format_tray_title_returns_empty_string_when_hidden() {
        let config = TrayConfig {
            show_cost: false,
            ..TrayConfig::default()
        };
        assert_eq!(format_tray_title(&config, 12.34), "");
    }

    #[test]
    fn format_tray_title_formats_cost_when_visible() {
        let config = TrayConfig {
            show_cost: true,
            ..TrayConfig::default()
        };
        assert_eq!(format_tray_title(&config, 12.345), "$12.35");
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
            last_updated: Local::now().to_rfc3339(),
            from_cache: true,
            period_label: String::new(),
            has_earlier_data: false,
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
            last_updated: Local::now().to_rfc3339(),
            from_cache: false,
            period_label: String::new(),
            has_earlier_data: false,
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
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Period label formatting
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn period_label_day_format() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        assert_eq!(format_day_label(date), "March 15, 2026");
    }

    #[test]
    fn period_label_week_same_month() {
        let monday = NaiveDate::from_ymd_opt(2026, 3, 9).unwrap();
        let sunday = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        assert_eq!(format_week_label(monday, sunday), "Mar 9 \u{2013} 15, 2026");
    }

    #[test]
    fn period_label_week_cross_month() {
        let monday = NaiveDate::from_ymd_opt(2026, 3, 30).unwrap();
        let sunday = NaiveDate::from_ymd_opt(2026, 4, 5).unwrap();
        assert_eq!(
            format_week_label(monday, sunday),
            "Mar 30 \u{2013} Apr 5, 2026"
        );
    }

    #[test]
    fn period_label_week_cross_year() {
        let monday = NaiveDate::from_ymd_opt(2025, 12, 29).unwrap();
        let sunday = NaiveDate::from_ymd_opt(2026, 1, 4).unwrap();
        assert_eq!(
            format_week_label(monday, sunday),
            "Dec 29, 2025 \u{2013} Jan 4, 2026"
        );
    }

    #[test]
    fn period_label_month_format() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        assert_eq!(format_month_label(date), "March 2026");
    }

    #[test]
    fn period_label_year_format() {
        assert_eq!(format_year_label(2026), "2026");
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
        let state = AppState {
            parser: Arc::new(parser),
            refresh_interval: Arc::new(RwLock::new(30)),
            tray_config: Arc::new(RwLock::new(TrayConfig::default())),
            last_usage_debug: Arc::new(RwLock::new(None)),
            cached_rate_limits: Arc::new(RwLock::new(None)),
        };

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
        let state = AppState {
            parser: Arc::new(parser),
            refresh_interval: Arc::new(RwLock::new(30)),
            tray_config: Arc::new(RwLock::new(TrayConfig::default())),
            last_usage_debug: Arc::new(RwLock::new(None)),
            cached_rate_limits: Arc::new(RwLock::new(None)),
        };

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
        let state = AppState {
            parser: Arc::new(parser),
            refresh_interval: Arc::new(RwLock::new(30)),
            tray_config: Arc::new(RwLock::new(TrayConfig::default())),
            last_usage_debug: Arc::new(RwLock::new(None)),
            cached_rate_limits: Arc::new(RwLock::new(None)),
        };

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
}
