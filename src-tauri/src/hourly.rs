use crate::models::*;
use chrono::{Local, Timelike};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

// ── Claude JSONL structs ──

#[derive(Deserialize)]
struct ClaudeJsonlEntry {
    #[serde(rename = "type", default)]
    entry_type: String,
    #[serde(default)]
    timestamp: String,
    message: Option<ClaudeJsonlMessage>,
}

#[derive(Deserialize)]
struct ClaudeJsonlMessage {
    model: Option<String>,
    usage: Option<ClaudeJsonlUsage>,
}

#[derive(Deserialize)]
struct ClaudeJsonlUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

// ── Codex rollout JSONL structs ──

#[derive(Deserialize)]
struct CodexRolloutEntry {
    #[serde(default)]
    timestamp: String,
    #[serde(rename = "type", default)]
    entry_type: String,
    payload: Option<CodexPayload>,
}

#[derive(Deserialize)]
struct CodexPayload {
    #[serde(rename = "type", default)]
    payload_type: String,
    info: Option<CodexTokenInfo>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct CodexTokenInfo {
    last_token_usage: Option<CodexTokenUsage>,
    model_context_window: Option<u64>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct CodexTokenUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    reasoning_output_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
}

// ── Common ──

struct HourlyRecord {
    model: String,
    tokens: u64,
}

/// Build hourly payload for Claude, Codex, or both.
pub fn get_hourly_payload(
    provider: &str,
    accurate_cost: f64,
    accurate_tokens: u64,
    model_costs: &[(String, f64, u64)],
) -> UsagePayload {
    let now = Local::now();
    let today = now.date_naive();

    let mut hourly: HashMap<u32, Vec<HourlyRecord>> = HashMap::new();

    // Parse Claude data
    if provider == "claude" || provider == "all" {
        let claude_dir = dirs::home_dir()
            .unwrap_or_default()
            .join(".claude")
            .join("projects");
        parse_claude_hourly(&claude_dir, today, &mut hourly);
    }

    // Parse Codex data
    if provider == "codex" || provider == "all" {
        let codex_dir = dirs::home_dir()
            .unwrap_or_default()
            .join(".codex")
            .join("sessions");
        parse_codex_hourly(&codex_dir, today, &mut hourly);
    }

    // Total tokens across all hours (for proportional cost distribution)
    let total_parsed_tokens: u64 = hourly.values()
        .flat_map(|recs| recs.iter())
        .map(|r| r.tokens)
        .sum();

    let min_hour = hourly.keys().copied().min().unwrap_or(now.hour());
    let max_hour = now.hour();

    let mut chart_buckets = Vec::new();

    for h in min_hour..=max_hour {
        let label = format_hour(h);
        let entries = hourly.get(&h);

        let mut segments: HashMap<String, (String, u64)> = HashMap::new();

        if let Some(items) = entries {
            for rec in items {
                let (name, key) = if rec.model.starts_with("gpt") || rec.model.starts_with("o1") || rec.model.starts_with("o3") || rec.model.starts_with("o4") {
                    normalize_codex_model(&rec.model)
                } else {
                    normalize_claude_model(&rec.model)
                };
                let seg = segments
                    .entry(key.to_string())
                    .or_insert((name.to_string(), 0));
                seg.1 += rec.tokens;
            }
        }

        let bucket_tokens: u64 = segments.values().map(|(_, t)| *t).sum();
        let bucket_proportion = if total_parsed_tokens > 0 {
            bucket_tokens as f64 / total_parsed_tokens as f64
        } else {
            0.0
        };
        let bucket_cost = accurate_cost * bucket_proportion;

        let segs: Vec<ChartSegment> = segments
            .iter()
            .map(|(key, (name, tokens))| {
                let seg_proportion = if bucket_tokens > 0 {
                    *tokens as f64 / bucket_tokens as f64
                } else {
                    0.0
                };
                ChartSegment {
                    model: name.clone(),
                    model_key: key.clone(),
                    cost: bucket_cost * seg_proportion,
                    tokens: *tokens,
                }
            })
            .collect();

        chart_buckets.push(ChartBucket {
            label,
            total: bucket_cost,
            segments: segs,
        });
    }

    let model_breakdown: Vec<ModelSummary> = model_costs
        .iter()
        .map(|(name, cost, tokens)| {
            let (display, key) = if name.starts_with("gpt") || name.starts_with("o1") || name.starts_with("o3") || name.starts_with("o4") {
                normalize_codex_model(name)
            } else {
                normalize_claude_model(name)
            };
            ModelSummary {
                display_name: display.to_string(),
                model_key: key.to_string(),
                cost: *cost,
                tokens: *tokens,
            }
        })
        .collect();

    UsagePayload {
        total_cost: accurate_cost,
        total_tokens: accurate_tokens,
        session_count: chart_buckets.iter().filter(|b| b.total > 0.0).count() as u32,
        input_tokens: 0,
        output_tokens: 0,
        chart_buckets,
        model_breakdown,
        active_block: None,
        five_hour_cost: 0.0,
        last_updated: chrono::Local::now().to_rfc3339(),
        from_cache: false,
    }
}

// ── Claude parsing ──

fn parse_claude_hourly(
    dir: &PathBuf,
    today: chrono::NaiveDate,
    hourly: &mut HashMap<u32, Vec<HourlyRecord>>,
) {
    if let Ok(files) = glob_jsonl_files(dir) {
        for path in files {
            if !modified_today(&path, today) {
                continue;
            }
            if let Ok(contents) = std::fs::read_to_string(&path) {
                for line in contents.lines() {
                    if let Ok(entry) = serde_json::from_str::<ClaudeJsonlEntry>(line) {
                        if entry.entry_type != "assistant" {
                            continue;
                        }
                        let msg = match &entry.message { Some(m) => m, None => continue };
                        let usage = match &msg.usage { Some(u) => u, None => continue };
                        let model = match &msg.model { Some(m) => m.clone(), None => continue };

                        let ts = match chrono::DateTime::parse_from_rfc3339(&entry.timestamp) {
                            Ok(dt) => dt.with_timezone(&Local),
                            Err(_) => continue,
                        };
                        if ts.date_naive() != today { continue; }

                        let tokens = usage.input_tokens.unwrap_or(0)
                            + usage.output_tokens.unwrap_or(0)
                            + usage.cache_creation_input_tokens.unwrap_or(0)
                            + usage.cache_read_input_tokens.unwrap_or(0);

                        hourly.entry(ts.hour()).or_default().push(HourlyRecord { model, tokens });
                    }
                }
            }
        }
    }
}

// ── Codex parsing ──

fn parse_codex_hourly(
    sessions_dir: &PathBuf,
    today: chrono::NaiveDate,
    hourly: &mut HashMap<u32, Vec<HourlyRecord>>,
) {
    // Codex stores sessions at ~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl
    let today_dir = sessions_dir
        .join(today.format("%Y").to_string())
        .join(today.format("%m").to_string())
        .join(today.format("%d").to_string());

    if !today_dir.exists() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(&today_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().map_or(false, |e| e == "jsonl") {
                continue;
            }

            // Track the model for this session (from session_meta)
            let mut session_model = String::from("gpt-5.4");

            if let Ok(contents) = std::fs::read_to_string(&path) {
                for line in contents.lines() {
                    if let Ok(entry) = serde_json::from_str::<CodexRolloutEntry>(line) {
                        // Extract model from turn events
                        if line.contains("\"model\"") {
                            if let Some(model) = extract_model_from_line(line) {
                                session_model = model;
                            }
                        }

                        // Look for token_count events
                        if entry.entry_type != "event_msg" {
                            continue;
                        }
                        let payload = match &entry.payload { Some(p) => p, None => continue };
                        if payload.payload_type != "token_count" {
                            continue;
                        }
                        let info = match &payload.info { Some(i) => i, None => continue };
                        let usage = match &info.last_token_usage { Some(u) => u, None => continue };

                        let ts = match chrono::DateTime::parse_from_rfc3339(&entry.timestamp) {
                            Ok(dt) => dt.with_timezone(&Local),
                            Err(_) => continue,
                        };
                        if ts.date_naive() != today { continue; }

                        let tokens = usage.input_tokens.unwrap_or(0)
                            + usage.output_tokens.unwrap_or(0)
                            + usage.reasoning_output_tokens.unwrap_or(0);

                        hourly.entry(ts.hour()).or_default().push(HourlyRecord {
                            model: session_model.clone(),
                            tokens,
                        });
                    }
                }
            }
        }
    }
}

fn extract_model_from_line(line: &str) -> Option<String> {
    // Quick extraction of "model":"..." from a JSON line
    let marker = "\"model\":\"";
    let start = line.find(marker)? + marker.len();
    let end = line[start..].find('"')? + start;
    Some(line[start..end].to_string())
}

// ── Helpers ──

fn format_hour(h: u32) -> String {
    match h {
        0 => "12AM".into(),
        1..=11 => format!("{}AM", h),
        12 => "12PM".into(),
        _ => format!("{}PM", h - 12),
    }
}

fn modified_today(path: &PathBuf, today: chrono::NaiveDate) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| {
            let dt: chrono::DateTime<Local> = t.into();
            dt.date_naive() == today
        })
        .unwrap_or(false)
}

fn glob_jsonl_files(dir: &PathBuf) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut results = Vec::new();
    if !dir.exists() { return Ok(results); }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Ok(mut sub) = glob_jsonl_files(&path) {
                results.append(&mut sub);
            }
        } else if path.extension().map_or(false, |e| e == "jsonl") {
            results.push(path);
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── format_hour ──

    #[test]
    fn format_hour_midnight() {
        assert_eq!(format_hour(0), "12AM");
    }

    #[test]
    fn format_hour_morning() {
        assert_eq!(format_hour(1), "1AM");
        assert_eq!(format_hour(11), "11AM");
    }

    #[test]
    fn format_hour_noon() {
        assert_eq!(format_hour(12), "12PM");
    }

    #[test]
    fn format_hour_afternoon() {
        assert_eq!(format_hour(13), "1PM");
        assert_eq!(format_hour(23), "11PM");
    }

    // ── extract_model_from_line ──

    #[test]
    fn extracts_model_from_json_line() {
        let line = r#"{"type":"assistant","model":"claude-sonnet-4-6","timestamp":"2025-03-15T10:00:00Z"}"#;
        assert_eq!(
            extract_model_from_line(line),
            Some("claude-sonnet-4-6".to_string())
        );
    }

    #[test]
    fn returns_none_when_no_model_field() {
        let line = r#"{"type":"user","timestamp":"2025-03-15T10:00:00Z"}"#;
        assert_eq!(extract_model_from_line(line), None);
    }

    #[test]
    fn returns_none_for_empty_string() {
        assert_eq!(extract_model_from_line(""), None);
    }

    // ── glob_jsonl_files ──

    #[test]
    fn finds_jsonl_files_recursively() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();

        fs::write(dir.path().join("a.jsonl"), "").unwrap();
        fs::write(sub.join("b.jsonl"), "").unwrap();
        fs::write(dir.path().join("c.txt"), "").unwrap();

        let results = glob_jsonl_files(&dir.path().to_path_buf()).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|p| p.extension().unwrap() == "jsonl"));
    }

    #[test]
    fn returns_empty_for_nonexistent_dir() {
        let results = glob_jsonl_files(&PathBuf::from("/nonexistent/path")).unwrap();
        assert!(results.is_empty());
    }

    // ── modified_today ──

    #[test]
    fn file_modified_today_returns_true() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.jsonl");
        fs::write(&path, "data").unwrap();
        assert!(modified_today(&path, Local::now().date_naive()));
    }

    #[test]
    fn nonexistent_file_returns_false() {
        assert!(!modified_today(
            &PathBuf::from("/no/such/file"),
            Local::now().date_naive()
        ));
    }
}
