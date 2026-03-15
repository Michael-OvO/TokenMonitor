use crate::ccusage::CcusageRunner;
use crate::models::*;
use chrono::{Datelike, Local, NaiveDate};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::State;
use tokio::sync::RwLock;

pub struct AppState {
    pub runner: Arc<RwLock<CcusageRunner>>,
    pub setup_status: Arc<RwLock<SetupStatus>>,
    pub refresh_interval: Arc<RwLock<u64>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            runner: Arc::new(RwLock::new(CcusageRunner::new())),
            setup_status: Arc::new(RwLock::new(SetupStatus {
                ready: false,
                installing: false,
                error: None,
            })),
            refresh_interval: Arc::new(RwLock::new(30)),
        }
    }
}

const CACHE_MAX_AGE: Duration = Duration::from_secs(120);

#[tauri::command]
pub async fn get_setup_status(state: State<'_, AppState>) -> Result<SetupStatus, String> {
    let status = state.setup_status.read().await;
    Ok(status.clone())
}

#[tauri::command]
pub async fn initialize_app(state: State<'_, AppState>) -> Result<SetupStatus, String> {
    {
        let mut status = state.setup_status.write().await;
        status.installing = true;
        status.error = None;
    }

    let result = {
        let mut runner = state.runner.write().await;
        runner.ensure_installed().await
    };

    let mut status = state.setup_status.write().await;
    status.installing = false;

    match result {
        Ok(()) => {
            status.ready = true;
            status.error = None;
        }
        Err(e) => {
            status.ready = false;
            status.error = Some(e);
        }
    }

    Ok(status.clone())
}

#[tauri::command]
pub async fn set_refresh_interval(interval: u64, state: State<'_, AppState>) -> Result<(), String> {
    let mut current = state.refresh_interval.write().await;
    *current = interval;
    Ok(())
}

#[tauri::command]
pub async fn clear_cache(state: State<'_, AppState>) -> Result<(), String> {
    let runner = state.runner.read().await;
    runner.clear_cache();
    Ok(())
}

#[tauri::command]
pub async fn get_usage_data(
    provider: String,
    period: String,
    state: State<'_, AppState>,
) -> Result<UsagePayload, String> {
    let runner = state.runner.read().await;
    let today = Local::now().format("%Y%m%d").to_string();

    match provider.as_str() {
        "claude" => get_claude_data(&runner, &period, &today).await,
        "codex" => get_codex_data(&runner, &period).await,
        "all" => get_combined_data(&runner, &period, &today).await,
        _ => Err(format!("Unknown provider: {}", provider)),
    }
}

async fn get_claude_data(
    runner: &CcusageRunner,
    period: &str,
    today: &str,
) -> Result<UsagePayload, String> {
    let now = Local::now();

    match period {
        "5h" => {
            // Current 5-hour billing window
            let (json, from_cache) = runner
                .run_cached("claude", "blocks", &["--since", today], CACHE_MAX_AGE)
                .await?;
            let resp: ClaudeBlocksResponse =
                serde_json::from_str(&json).map_err(|e| format!("Parse error: {}", e))?;
            Ok(blocks_to_payload(resp, from_cache))
        }
        "day" => {
            // Hybrid: ccusage daily for accurate totals + JSONL for hourly distribution
            let (json, _from_cache) = runner
                .run_cached("claude", "daily", &["--since", today], CACHE_MAX_AGE)
                .await?;
            let resp: ClaudeDailyResponse =
                serde_json::from_str(&json).map_err(|e| format!("Parse error: {}", e))?;

            let accurate_cost: f64 = resp.daily.iter().map(|d| d.total_cost).sum();
            let accurate_tokens: u64 = resp.daily.iter().map(|d| d.total_tokens).sum();
            let model_costs: Vec<(String, f64, u64)> = resp.daily.iter()
                .flat_map(|d| d.model_breakdowns.iter())
                .map(|mb| (mb.model_name.clone(), mb.cost, mb.input_tokens + mb.output_tokens))
                .collect();

            Ok(crate::hourly::get_hourly_payload("claude", accurate_cost, accurate_tokens, &model_costs))
        }
        "week" => {
            // This week (Mon–today) — bars represent days
            let week_start = (now - chrono::Duration::days(now.weekday().num_days_from_monday() as i64))
                .format("%Y%m%d")
                .to_string();
            let (json, from_cache) = runner
                .run_cached("claude", "daily", &["--since", &week_start], CACHE_MAX_AGE)
                .await?;
            let resp: ClaudeDailyResponse =
                serde_json::from_str(&json).map_err(|e| format!("Parse error: {}", e))?;
            Ok(claude_daily_to_payload(resp, from_cache))
        }
        "month" => {
            // This month (1st–today) — bars represent days
            let month_start = NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
                .unwrap()
                .format("%Y%m%d")
                .to_string();
            let (json, from_cache) = runner
                .run_cached("claude", "daily", &["--since", &month_start], CACHE_MAX_AGE)
                .await?;
            let resp: ClaudeDailyResponse =
                serde_json::from_str(&json).map_err(|e| format!("Parse error: {}", e))?;
            Ok(claude_daily_to_payload(resp, from_cache))
        }
        "year" => {
            // This year (Jan 1–today) — bars represent months
            let year_start = NaiveDate::from_ymd_opt(now.year(), 1, 1)
                .unwrap()
                .format("%Y%m%d")
                .to_string();
            let (json, from_cache) = runner
                .run_cached("claude", "monthly", &["--since", &year_start], CACHE_MAX_AGE)
                .await?;
            let resp: ClaudeMonthlyResponse =
                serde_json::from_str(&json).map_err(|e| format!("Parse error: {}", e))?;
            Ok(claude_monthly_to_payload(resp, from_cache))
        }
        _ => Err(format!("Unknown period: {}", period)),
    }
}

async fn get_combined_data(
    runner: &CcusageRunner,
    period: &str,
    today: &str,
) -> Result<UsagePayload, String> {
    let (claude, codex) = tokio::join!(
        get_claude_data(runner, period, today),
        get_codex_data(runner, period),
    );

    match (claude, codex) {
        (Ok(c), Ok(x)) => Ok(merge_payloads(c, x)),
        (Ok(c), Err(_)) => Ok(c),
        (Err(_), Ok(x)) => Ok(x),
        (Err(e), _) => Err(e),
    }
}

fn merge_payloads(mut c: UsagePayload, x: UsagePayload) -> UsagePayload {
    // Merge chart buckets by label (date)
    let mut bucket_map: std::collections::BTreeMap<String, ChartBucket> =
        std::collections::BTreeMap::new();
    for b in c.chart_buckets.iter().chain(x.chart_buckets.iter()) {
        let entry = bucket_map.entry(b.label.clone()).or_insert_with(|| ChartBucket {
            label: b.label.clone(),
            total: 0.0,
            segments: vec![],
        });
        entry.total += b.total;
        entry.segments.extend(b.segments.clone());
    }

    c.total_cost += x.total_cost;
    c.total_tokens += x.total_tokens;
    c.session_count += x.session_count;
    c.input_tokens += x.input_tokens;
    c.output_tokens += x.output_tokens;
    c.chart_buckets = bucket_map.into_values().collect();
    c.model_breakdown.extend(x.model_breakdown);
    c.five_hour_cost += x.five_hour_cost;
    c.from_cache = c.from_cache && x.from_cache;
    c
}

async fn get_codex_data(
    runner: &CcusageRunner,
    period: &str,
) -> Result<UsagePayload, String> {
    let now = Local::now();
    let today = now.format("%Y%m%d").to_string();

    // Day tab: hourly view using JSONL + ccusage totals
    if period == "day" {
        let (json, _) = runner
            .run_cached("codex", "daily", &["--since", &today], CACHE_MAX_AGE)
            .await?;
        let resp: CodexDailyResponse =
            serde_json::from_str(&json).map_err(|e| format!("Parse error: {}", e))?;
        let accurate_cost: f64 = resp.daily.iter().map(|d| d.cost_usd).sum();
        let accurate_tokens: u64 = resp.daily.iter().map(|d| d.total_tokens).sum();
        let model_costs: Vec<(String, f64, u64)> = resp.daily.iter()
            .flat_map(|d| d.models.iter())
            .map(|(name, u)| (name.clone(), 0.0, u.total_tokens)) // codex daily doesn't split cost per model cleanly
            .collect();
        // Distribute cost proportionally
        let total_tok: u64 = model_costs.iter().map(|(_, _, t)| t).sum();
        let model_costs: Vec<(String, f64, u64)> = model_costs.iter()
            .map(|(n, _, t)| (n.clone(), if total_tok > 0 { accurate_cost * (*t as f64 / total_tok as f64) } else { 0.0 }, *t))
            .collect();
        return Ok(crate::hourly::get_hourly_payload("codex", accurate_cost, accurate_tokens, &model_costs));
    }

    let since = match period {
        "5h" => today,
        "week" => (now - chrono::Duration::days(now.weekday().num_days_from_monday() as i64))
            .format("%Y%m%d").to_string(),
        "month" => NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
            .unwrap().format("%Y%m%d").to_string(),
        "year" => NaiveDate::from_ymd_opt(now.year(), 1, 1)
            .unwrap().format("%Y%m%d").to_string(),
        _ => return Err(format!("Unknown period: {}", period)),
    };

    let (json, from_cache) = runner
        .run_cached("codex", "daily", &["--since", &since], CACHE_MAX_AGE)
        .await?;
    let resp: CodexDailyResponse =
        serde_json::from_str(&json).map_err(|e| format!("Parse error: {}", e))?;
    Ok(codex_daily_to_payload(resp, from_cache))
}

// ── Shared helpers ──

/// Aggregate model breakdowns into deduplicated ModelSummary list.
fn aggregate_claude_models<'a>(
    breakdowns: impl Iterator<Item = &'a ClaudeModelBreakdown>,
) -> Vec<ModelSummary> {
    let mut map: HashMap<String, (String, f64, u64)> = HashMap::new();
    for mb in breakdowns {
        let (name, key) = normalize_claude_model(&mb.model_name);
        let entry = map
            .entry(key.to_string())
            .or_insert((name.to_string(), 0.0, 0));
        entry.1 += mb.cost;
        entry.2 += mb.input_tokens + mb.output_tokens;
    }
    map.into_iter()
        .map(|(key, (name, cost, tokens))| ModelSummary {
            display_name: name,
            model_key: key,
            cost,
            tokens,
        })
        .collect()
}

/// Sum input/output tokens across model breakdowns (separated from map to avoid side-effects).
fn count_claude_tokens<'a>(
    breakdowns: impl Iterator<Item = &'a ClaudeModelBreakdown>,
) -> (u64, u64) {
    breakdowns.fold((0, 0), |(inp, out), mb| {
        (
            inp + mb.input_tokens + mb.cache_creation_tokens + mb.cache_read_tokens,
            out + mb.output_tokens,
        )
    })
}

/// Convert a ClaudeModelBreakdown into a ChartSegment.
fn mb_to_segment(mb: &ClaudeModelBreakdown) -> ChartSegment {
    let (name, key) = normalize_claude_model(&mb.model_name);
    ChartSegment {
        model: name.to_string(),
        model_key: key.to_string(),
        cost: mb.cost,
        tokens: mb.input_tokens + mb.output_tokens,
    }
}

// ── Transform functions ──

fn blocks_to_payload(resp: ClaudeBlocksResponse, from_cache: bool) -> UsagePayload {
    let active = resp.blocks.iter().find(|b| !b.is_gap);
    let active_block_data = active.and_then(|b| {
        Some(ActiveBlock {
            cost: b.cost_usd,
            burn_rate_per_hour: b.burn_rate.as_ref()?.cost_per_hour,
            projected_cost: b.projection.as_ref()?.total_cost,
            is_active: b.is_active,
        })
    });

    let non_gap_blocks: Vec<_> = resp.blocks.iter().filter(|b| !b.is_gap).collect();

    let total_cost: f64 = non_gap_blocks.iter().map(|b| b.cost_usd).sum();
    let total_tokens: u64 = non_gap_blocks.iter().map(|b| b.total_tokens).sum();
    let session_count = non_gap_blocks.len() as u32;

    // For blocks view, each non-gap block is a chart bucket
    let chart_buckets: Vec<ChartBucket> = non_gap_blocks
        .iter()
        .map(|b| {
            let segments: Vec<ChartSegment> = b
                .models
                .iter()
                .map(|m| {
                    let (name, key) = normalize_claude_model(m);
                    ChartSegment {
                        model: name.to_string(),
                        model_key: key.to_string(),
                        cost: b.cost_usd / b.models.len().max(1) as f64,
                        tokens: b.total_tokens / b.models.len().max(1) as u64,
                    }
                })
                .collect();

            // Parse time for label
            let label = chrono::DateTime::parse_from_rfc3339(&b.start_time)
                .map(|dt| dt.format("%-I%P").to_string())
                .unwrap_or_else(|_| "?".into());

            ChartBucket {
                label,
                total: b.cost_usd,
                segments,
            }
        })
        .collect();

    // Aggregate model breakdown across all blocks
    let mut model_map: HashMap<String, (String, f64, u64)> = HashMap::new();
    for b in &non_gap_blocks {
        for m in &b.models {
            let (name, key) = normalize_claude_model(m);
            let entry = model_map
                .entry(key.to_string())
                .or_insert((name.to_string(), 0.0, 0));
            entry.1 += b.cost_usd / b.models.len().max(1) as f64;
            entry.2 += b.total_tokens / b.models.len().max(1) as u64;
        }
    }

    let model_breakdown: Vec<ModelSummary> = model_map
        .into_iter()
        .map(|(key, (name, cost, tokens))| ModelSummary {
            display_name: name,
            model_key: key,
            cost,
            tokens,
        })
        .collect();

    let five_hour_cost = active
        .map(|b| b.cost_usd)
        .unwrap_or(total_cost);

    UsagePayload {
        total_cost,
        total_tokens,
        session_count,
        input_tokens: 0,
        output_tokens: 0,
        chart_buckets,
        model_breakdown,
        active_block: active_block_data,
        five_hour_cost,
        last_updated: chrono::Local::now().to_rfc3339(),
        from_cache,
    }
}

fn claude_daily_to_payload(resp: ClaudeDailyResponse, from_cache: bool) -> UsagePayload {
    let total_cost: f64 = resp.daily.iter().map(|d| d.total_cost).sum();
    let total_tokens: u64 = resp.daily.iter().map(|d| d.total_tokens).sum();
    let all_breakdowns = || resp.daily.iter().flat_map(|d| &d.model_breakdowns);
    let (total_input, total_output) = count_claude_tokens(all_breakdowns());

    let chart_buckets: Vec<ChartBucket> = resp
        .daily
        .iter()
        .map(|day| {
            let label = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")
                .map(|d| d.format("%b %-d").to_string())
                .unwrap_or_else(|_| day.date.clone());
            ChartBucket {
                label,
                total: day.total_cost,
                segments: day.model_breakdowns.iter().map(mb_to_segment).collect(),
            }
        })
        .collect();

    UsagePayload {
        total_cost,
        total_tokens,
        session_count: resp.daily.len() as u32,
        input_tokens: total_input,
        output_tokens: total_output,
        chart_buckets,
        model_breakdown: aggregate_claude_models(all_breakdowns()),
        active_block: None,
        five_hour_cost: 0.0,
        last_updated: chrono::Local::now().to_rfc3339(),
        from_cache,
    }
}

fn claude_monthly_to_payload(resp: ClaudeMonthlyResponse, from_cache: bool) -> UsagePayload {
    let total_cost: f64 = resp.monthly.iter().map(|m| m.total_cost).sum();
    let total_tokens: u64 = resp.monthly.iter().map(|m| m.total_tokens).sum();
    let all_breakdowns = || resp.monthly.iter().flat_map(|m| &m.model_breakdowns);
    let (total_input, total_output) = count_claude_tokens(all_breakdowns());

    let chart_buckets: Vec<ChartBucket> = resp
        .monthly
        .iter()
        .map(|month| {
            let label = NaiveDate::parse_from_str(&format!("{}-01", month.month), "%Y-%m-%d")
                .map(|d| d.format("%b").to_string())
                .unwrap_or_else(|_| month.month.clone());
            ChartBucket {
                label,
                total: month.total_cost,
                segments: month.model_breakdowns.iter().map(mb_to_segment).collect(),
            }
        })
        .collect();

    UsagePayload {
        total_cost,
        total_tokens,
        session_count: resp.monthly.len() as u32,
        input_tokens: total_input,
        output_tokens: total_output,
        chart_buckets,
        model_breakdown: aggregate_claude_models(all_breakdowns()),
        active_block: None,
        five_hour_cost: 0.0,
        last_updated: chrono::Local::now().to_rfc3339(),
        from_cache,
    }
}

fn codex_daily_to_payload(resp: CodexDailyResponse, from_cache: bool) -> UsagePayload {
    let total_cost: f64 = resp.daily.iter().map(|d| d.cost_usd).sum();
    let total_tokens: u64 = resp.daily.iter().map(|d| d.total_tokens).sum();
    let session_count = resp.daily.len() as u32;

    let mut total_input: u64 = 0;
    let mut total_output: u64 = 0;

    let chart_buckets: Vec<ChartBucket> = resp
        .daily
        .iter()
        .map(|day| {
            let segments: Vec<ChartSegment> = day
                .models
                .iter()
                .map(|(model_name, usage)| {
                    total_input += usage.input_tokens;
                    total_output += usage.output_tokens + usage.reasoning_output_tokens;
                    let (name, key) = normalize_codex_model(model_name);
                    ChartSegment {
                        model: name.to_string(),
                        model_key: key.to_string(),
                        cost: day.cost_usd * (usage.total_tokens as f64 / day.total_tokens.max(1) as f64),
                        tokens: usage.total_tokens,
                    }
                })
                .collect();

            // Parse Codex date format "Mar 01, 2026"
            let label = chrono::NaiveDate::parse_from_str(&day.date, "%b %d, %Y")
                .map(|d| d.format("%b %-d").to_string())
                .unwrap_or_else(|_| day.date.clone());

            ChartBucket {
                label,
                total: day.cost_usd,
                segments,
            }
        })
        .collect();

    let mut model_map: HashMap<String, (String, f64, u64)> = HashMap::new();
    for day in &resp.daily {
        for (model_name, usage) in &day.models {
            let (name, key) = normalize_codex_model(model_name);
            let entry = model_map
                .entry(key.to_string())
                .or_insert((name.to_string(), 0.0, 0));
            let proportion = usage.total_tokens as f64 / day.total_tokens.max(1) as f64;
            entry.1 += day.cost_usd * proportion;
            entry.2 += usage.total_tokens;
        }
    }

    let model_breakdown: Vec<ModelSummary> = model_map
        .into_iter()
        .map(|(key, (name, cost, tokens))| ModelSummary {
            display_name: name,
            model_key: key,
            cost,
            tokens,
        })
        .collect();

    UsagePayload {
        total_cost,
        total_tokens,
        session_count,
        input_tokens: total_input,
        output_tokens: total_output,
        chart_buckets,
        model_breakdown,
        active_block: None,
        five_hour_cost: 0.0,
        last_updated: chrono::Local::now().to_rfc3339(),
        from_cache,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper builders ──

    fn make_breakdown(model: &str, cost: f64, input: u64, output: u64) -> ClaudeModelBreakdown {
        ClaudeModelBreakdown {
            model_name: model.to_string(),
            cost,
            input_tokens: input,
            output_tokens: output,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        }
    }

    fn make_block(
        start: &str,
        cost: f64,
        tokens: u64,
        is_gap: bool,
        is_active: bool,
        models: Vec<&str>,
    ) -> ClaudeBlockEntry {
        ClaudeBlockEntry {
            start_time: start.to_string(),
            end_time: start.to_string(), // not used in tests
            is_active,
            is_gap,
            entries: 1,
            cost_usd: cost,
            total_tokens: tokens,
            models: models.into_iter().map(String::from).collect(),
            burn_rate: Some(BurnRate { cost_per_hour: 0.50 }),
            projection: Some(Projection { total_cost: 5.00 }),
        }
    }

    // ── blocks_to_payload ──

    #[test]
    fn blocks_excludes_gap_blocks() {
        let resp = ClaudeBlocksResponse {
            blocks: vec![
                make_block("2025-03-15T10:00:00Z", 1.00, 1000, false, true, vec!["claude-sonnet-4-6"]),
                make_block("2025-03-15T11:00:00Z", 0.00, 0, true, false, vec![]),
                make_block("2025-03-15T12:00:00Z", 2.00, 2000, false, false, vec!["claude-opus-4-6"]),
            ],
        };
        let payload = blocks_to_payload(resp, false);

        assert_eq!(payload.total_cost, 3.00);
        assert_eq!(payload.total_tokens, 3000);
        assert_eq!(payload.session_count, 2);
        assert_eq!(payload.chart_buckets.len(), 2); // gap excluded
    }

    #[test]
    fn blocks_extracts_active_block() {
        let resp = ClaudeBlocksResponse {
            blocks: vec![
                make_block("2025-03-15T10:00:00Z", 1.50, 1000, false, true, vec!["claude-sonnet-4-6"]),
            ],
        };
        let payload = blocks_to_payload(resp, false);

        let ab = payload.active_block.unwrap();
        assert_eq!(ab.cost, 1.50);
        assert!(ab.is_active);
        assert_eq!(ab.burn_rate_per_hour, 0.50);
        assert_eq!(ab.projected_cost, 5.00);
    }

    #[test]
    fn blocks_five_hour_cost_uses_active_block() {
        let resp = ClaudeBlocksResponse {
            blocks: vec![
                make_block("2025-03-15T10:00:00Z", 1.50, 1000, false, true, vec!["claude-sonnet-4-6"]),
                make_block("2025-03-15T12:00:00Z", 3.00, 2000, false, false, vec!["claude-opus-4-5"]),
            ],
        };
        let payload = blocks_to_payload(resp, false);
        // five_hour_cost should be the active block's cost (first non-gap)
        assert_eq!(payload.five_hour_cost, 1.50);
    }

    #[test]
    fn blocks_from_cache_passed_through() {
        let resp = ClaudeBlocksResponse { blocks: vec![] };
        assert!(blocks_to_payload(resp, true).from_cache);
    }

    // ── claude_daily_to_payload ──

    #[test]
    fn daily_formats_date_labels() {
        let resp = ClaudeDailyResponse {
            daily: vec![ClaudeDayEntry {
                date: "2025-03-15".to_string(),
                total_tokens: 1000,
                total_cost: 0.50,
                model_breakdowns: vec![],
            }],
        };
        let payload = claude_daily_to_payload(resp, false);
        assert_eq!(payload.chart_buckets[0].label, "Mar 15");
    }

    #[test]
    fn daily_sums_costs_and_tokens() {
        let resp = ClaudeDailyResponse {
            daily: vec![
                ClaudeDayEntry {
                    date: "2025-03-14".to_string(),
                    total_tokens: 1000,
                    total_cost: 0.50,
                    model_breakdowns: vec![],
                },
                ClaudeDayEntry {
                    date: "2025-03-15".to_string(),
                    total_tokens: 2000,
                    total_cost: 1.00,
                    model_breakdowns: vec![],
                },
            ],
        };
        let payload = claude_daily_to_payload(resp, false);
        assert_eq!(payload.total_cost, 1.50);
        assert_eq!(payload.total_tokens, 3000);
        assert_eq!(payload.session_count, 2);
    }

    #[test]
    fn daily_deduplicates_model_breakdown() {
        let resp = ClaudeDailyResponse {
            daily: vec![
                ClaudeDayEntry {
                    date: "2025-03-14".to_string(),
                    total_tokens: 1000,
                    total_cost: 0.50,
                    model_breakdowns: vec![
                        make_breakdown("claude-sonnet-4-6-20260301", 0.30, 500, 200),
                    ],
                },
                ClaudeDayEntry {
                    date: "2025-03-15".to_string(),
                    total_tokens: 2000,
                    total_cost: 1.00,
                    model_breakdowns: vec![
                        make_breakdown("claude-sonnet-4-6-20260301", 0.70, 800, 500),
                    ],
                },
            ],
        };
        let payload = claude_daily_to_payload(resp, false);
        // Both breakdowns share the "sonnet" key -> deduplicated to 1 entry
        assert_eq!(payload.model_breakdown.len(), 1);
        assert_eq!(payload.model_breakdown[0].model_key, "sonnet");
        assert_eq!(payload.model_breakdown[0].cost, 1.00);
        assert_eq!(payload.model_breakdown[0].tokens, 2000); // 700 + 1300
    }

    // ── claude_monthly_to_payload ──

    #[test]
    fn monthly_formats_month_labels() {
        let resp = ClaudeMonthlyResponse {
            monthly: vec![ClaudeMonthEntry {
                month: "2025-03".to_string(),
                total_tokens: 50000,
                total_cost: 10.00,
                model_breakdowns: vec![],
            }],
        };
        let payload = claude_monthly_to_payload(resp, false);
        assert_eq!(payload.chart_buckets[0].label, "Mar");
    }

    #[test]
    fn monthly_sums_across_months() {
        let resp = ClaudeMonthlyResponse {
            monthly: vec![
                ClaudeMonthEntry {
                    month: "2025-01".to_string(),
                    total_tokens: 10000,
                    total_cost: 5.00,
                    model_breakdowns: vec![],
                },
                ClaudeMonthEntry {
                    month: "2025-02".to_string(),
                    total_tokens: 20000,
                    total_cost: 10.00,
                    model_breakdowns: vec![],
                },
            ],
        };
        let payload = claude_monthly_to_payload(resp, false);
        assert_eq!(payload.total_cost, 15.00);
        assert_eq!(payload.total_tokens, 30000);
    }

    // ── codex_daily_to_payload ──

    #[test]
    fn codex_parses_date_format() {
        let mut models = std::collections::HashMap::new();
        models.insert(
            "gpt-5.4-turbo".to_string(),
            CodexModelUsage {
                input_tokens: 1000,
                output_tokens: 500,
                reasoning_output_tokens: 200,
                total_tokens: 1700,
            },
        );
        let resp = CodexDailyResponse {
            daily: vec![CodexDayEntry {
                date: "Mar 01, 2026".to_string(),
                total_tokens: 1700,
                cost_usd: 0.50,
                models,
            }],
        };
        let payload = codex_daily_to_payload(resp, false);
        assert_eq!(payload.chart_buckets[0].label, "Mar 1");
    }

    #[test]
    fn codex_distributes_cost_proportionally() {
        let mut models = std::collections::HashMap::new();
        models.insert(
            "gpt-5.4".to_string(),
            CodexModelUsage {
                input_tokens: 0,
                output_tokens: 0,
                reasoning_output_tokens: 0,
                total_tokens: 750,
            },
        );
        models.insert(
            "gpt-5.3".to_string(),
            CodexModelUsage {
                input_tokens: 0,
                output_tokens: 0,
                reasoning_output_tokens: 0,
                total_tokens: 250,
            },
        );
        let resp = CodexDailyResponse {
            daily: vec![CodexDayEntry {
                date: "Mar 01, 2026".to_string(),
                total_tokens: 1000,
                cost_usd: 4.00,
                models,
            }],
        };
        let payload = codex_daily_to_payload(resp, false);
        // gpt-5.4 has 75% of tokens -> $3.00, gpt-5.3 has 25% -> $1.00
        let seg_54 = payload.chart_buckets[0]
            .segments
            .iter()
            .find(|s| s.model_key == "gpt54")
            .unwrap();
        let seg_53 = payload.chart_buckets[0]
            .segments
            .iter()
            .find(|s| s.model_key == "gpt53")
            .unwrap();
        assert!((seg_54.cost - 3.00).abs() < 0.001);
        assert!((seg_53.cost - 1.00).abs() < 0.001);
    }

    #[test]
    fn codex_counts_input_output_tokens() {
        let mut models = std::collections::HashMap::new();
        models.insert(
            "gpt-5.4".to_string(),
            CodexModelUsage {
                input_tokens: 1000,
                output_tokens: 500,
                reasoning_output_tokens: 200,
                total_tokens: 1700,
            },
        );
        let resp = CodexDailyResponse {
            daily: vec![CodexDayEntry {
                date: "Mar 01, 2026".to_string(),
                total_tokens: 1700,
                cost_usd: 1.00,
                models,
            }],
        };
        let payload = codex_daily_to_payload(resp, false);
        assert_eq!(payload.input_tokens, 1000);
        // output includes reasoning_output_tokens
        assert_eq!(payload.output_tokens, 700);
    }

    // ── merge_payloads ──

    #[test]
    fn merge_adds_costs_and_tokens() {
        let a = UsagePayload {
            total_cost: 1.00,
            total_tokens: 1000,
            session_count: 2,
            input_tokens: 500,
            output_tokens: 500,
            chart_buckets: vec![],
            model_breakdown: vec![],
            active_block: None,
            five_hour_cost: 0.50,
            last_updated: String::new(),
            from_cache: true,
        };
        let b = UsagePayload {
            total_cost: 2.00,
            total_tokens: 3000,
            session_count: 1,
            input_tokens: 1500,
            output_tokens: 1500,
            chart_buckets: vec![],
            model_breakdown: vec![],
            active_block: None,
            five_hour_cost: 1.00,
            last_updated: String::new(),
            from_cache: false,
        };
        let merged = merge_payloads(a, b);
        assert_eq!(merged.total_cost, 3.00);
        assert_eq!(merged.total_tokens, 4000);
        assert_eq!(merged.session_count, 3);
        assert_eq!(merged.input_tokens, 2000);
        assert_eq!(merged.output_tokens, 2000);
        assert_eq!(merged.five_hour_cost, 1.50);
    }

    #[test]
    fn merge_from_cache_is_and() {
        let make = |fc: bool| UsagePayload {
            total_cost: 0.0,
            total_tokens: 0,
            session_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            chart_buckets: vec![],
            model_breakdown: vec![],
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: String::new(),
            from_cache: fc,
        };
        assert!(!merge_payloads(make(true), make(false)).from_cache);
        assert!(merge_payloads(make(true), make(true)).from_cache);
        assert!(!merge_payloads(make(false), make(false)).from_cache);
    }

    #[test]
    fn merge_combines_buckets_by_label() {
        let a = UsagePayload {
            total_cost: 1.00,
            total_tokens: 0,
            session_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            chart_buckets: vec![
                ChartBucket {
                    label: "Mar 15".to_string(),
                    total: 1.00,
                    segments: vec![ChartSegment {
                        model: "Sonnet".to_string(),
                        model_key: "sonnet".to_string(),
                        cost: 1.00,
                        tokens: 1000,
                    }],
                },
            ],
            model_breakdown: vec![],
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: String::new(),
            from_cache: false,
        };
        let b = UsagePayload {
            total_cost: 2.00,
            total_tokens: 0,
            session_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            chart_buckets: vec![
                ChartBucket {
                    label: "Mar 15".to_string(),
                    total: 2.00,
                    segments: vec![ChartSegment {
                        model: "GPT-5.4".to_string(),
                        model_key: "gpt54".to_string(),
                        cost: 2.00,
                        tokens: 3000,
                    }],
                },
                ChartBucket {
                    label: "Mar 16".to_string(),
                    total: 0.50,
                    segments: vec![],
                },
            ],
            model_breakdown: vec![],
            active_block: None,
            five_hour_cost: 0.0,
            last_updated: String::new(),
            from_cache: false,
        };
        let merged = merge_payloads(a, b);
        assert_eq!(merged.chart_buckets.len(), 2);

        let mar15 = merged.chart_buckets.iter().find(|b| b.label == "Mar 15").unwrap();
        assert_eq!(mar15.total, 3.00);
        assert_eq!(mar15.segments.len(), 2); // sonnet + gpt54
    }

    // ── aggregate_claude_models ──

    #[test]
    fn aggregate_deduplicates_by_key() {
        let breakdowns = vec![
            make_breakdown("claude-sonnet-4-6-20260301", 0.50, 500, 200),
            make_breakdown("claude-sonnet-4-6-20260101", 0.30, 300, 100),
            make_breakdown("claude-opus-4-5-20250501", 1.00, 1000, 500),
        ];
        let result = aggregate_claude_models(breakdowns.iter());
        assert_eq!(result.len(), 2); // sonnet + opus

        let sonnet = result.iter().find(|m| m.model_key == "sonnet").unwrap();
        assert_eq!(sonnet.cost, 0.80);
        assert_eq!(sonnet.tokens, 1100); // (500+200) + (300+100)
    }

    // ── count_claude_tokens ──

    #[test]
    fn count_includes_cache_tokens_in_input() {
        let breakdowns = vec![ClaudeModelBreakdown {
            model_name: "claude-sonnet-4-6".to_string(),
            cost: 1.00,
            input_tokens: 200,
            output_tokens: 300,
            cache_creation_tokens: 100,
            cache_read_tokens: 50,
        }];
        let (input, output) = count_claude_tokens(breakdowns.iter());
        assert_eq!(input, 350); // 200 + 100 + 50
        assert_eq!(output, 300);
    }

    // ── mb_to_segment ──

    #[test]
    fn mb_to_segment_normalizes_model() {
        let mb = make_breakdown("claude-sonnet-4-6-20260301", 0.75, 500, 200);
        let seg = mb_to_segment(&mb);
        assert_eq!(seg.model, "Sonnet 4.6");
        assert_eq!(seg.model_key, "sonnet");
        assert_eq!(seg.cost, 0.75);
        assert_eq!(seg.tokens, 700); // 500 + 200
    }
}
