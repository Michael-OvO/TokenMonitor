use crate::models::*;
use crate::parser::UsageParser;
use chrono::{Datelike, Local, NaiveDate};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

pub struct AppState {
    pub parser: Arc<UsageParser>,
    pub refresh_interval: Arc<RwLock<u64>>,
    pub show_tray_amount: Arc<RwLock<bool>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            parser: Arc::new(UsageParser::new()),
            refresh_interval: Arc::new(RwLock::new(30)),
            show_tray_amount: Arc::new(RwLock::new(true)),
        }
    }
}

#[tauri::command]
pub async fn set_refresh_interval(interval: u64, state: State<'_, AppState>) -> Result<(), String> {
    let mut current = state.refresh_interval.write().await;
    *current = interval;
    Ok(())
}

#[tauri::command]
pub async fn set_show_tray_amount(show: bool, state: State<'_, AppState>) -> Result<(), String> {
    let mut current = state.show_tray_amount.write().await;
    *current = show;
    Ok(())
}

#[tauri::command]
pub async fn clear_cache(state: State<'_, AppState>) -> Result<(), String> {
    state.parser.clear_cache();
    Ok(())
}

#[tauri::command]
pub async fn get_usage_data(
    provider: String,
    period: String,
    state: State<'_, AppState>,
) -> Result<UsagePayload, String> {
    let parser = &state.parser;

    match provider.as_str() {
        "claude" | "codex" => Ok(get_provider_data(parser, &provider, &period)?),
        "all" => {
            let claude = get_provider_data(parser, "claude", &period)?;
            let codex = get_provider_data(parser, "codex", &period)?;
            Ok(merge_payloads(claude, codex))
        }
        _ => Err(format!("Unknown provider: {}", provider)),
    }
}

fn get_provider_data(
    parser: &UsageParser,
    provider: &str,
    period: &str,
) -> Result<UsagePayload, String> {
    let now = Local::now();
    let today = now.format("%Y%m%d").to_string();

    Ok(match period {
        "5h" => parser.get_blocks(provider, &today),
        "day" => parser.get_hourly(provider, &today),
        "week" => {
            let week_start = (now
                - chrono::Duration::days(now.weekday().num_days_from_monday() as i64))
            .format("%Y%m%d")
            .to_string();
            parser.get_daily(provider, &week_start)
        }
        "month" => {
            let month_start = NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
                .unwrap()
                .format("%Y%m%d")
                .to_string();
            parser.get_daily(provider, &month_start)
        }
        "year" => {
            let year_start = NaiveDate::from_ymd_opt(now.year(), 1, 1)
                .unwrap()
                .format("%Y%m%d")
                .to_string();
            parser.get_monthly(provider, &year_start)
        }
        _ => return Err(format!("Unknown period: {}", period)),
    })
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
        }
    }

    fn write_file(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
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
        let payload = get_provider_data(&parser, "codex", "5h").unwrap();

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
}
