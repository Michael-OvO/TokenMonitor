use crate::models::{ProviderRateLimits, RateLimitWindow};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Deserialize)]
struct CodexJsonlLine {
    payload: Option<CodexPayload>,
}

#[derive(Deserialize)]
struct CodexPayload {
    rate_limits: Option<CodexRateLimitData>,
}

#[derive(Deserialize)]
struct CodexRateLimitData {
    primary: Option<CodexWindowData>,
    secondary: Option<CodexWindowData>,
    plan_type: Option<String>,
}

#[derive(Deserialize)]
struct CodexWindowData {
    used_percent: f64,
    window_minutes: u64,
    resets_at: u64,
}

pub(super) fn extract_codex_rate_limits(codex_dir: &Path) -> Result<ProviderRateLimits, String> {
    // Find the most recent JSONL file by walking the date-based directory structure
    let mut newest_file: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    find_newest_jsonl(codex_dir, &mut newest_file, 0);

    let file_path = newest_file
        .map(|(_, p)| p)
        .ok_or_else(|| "No Codex session files found".to_string())?;

    // Read from the end looking for rate_limits
    let file =
        std::fs::File::open(&file_path).map_err(|e| format!("Failed to open Codex file: {e}"))?;
    let reader = BufReader::new(file);

    let mut last_rate_limits: Option<CodexRateLimitData> = None;
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if !line.contains("rate_limits") {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<CodexJsonlLine>(&line) {
            if let Some(payload) = entry.payload {
                if let Some(rl) = payload.rate_limits {
                    last_rate_limits = Some(rl);
                }
            }
        }
    }

    let rl =
        last_rate_limits.ok_or_else(|| "No rate limit data in Codex session files".to_string())?;

    let mut windows = Vec::new();
    if let Some(primary) = &rl.primary {
        windows.push(codex_window_to_rate_limit("primary", primary));
    }
    if let Some(secondary) = &rl.secondary {
        windows.push(codex_window_to_rate_limit("secondary", secondary));
    }

    let plan_tier = rl.plan_type.as_ref().map(|p| match p.as_str() {
        "pro" => "Pro".to_string(),
        "plus" => "Plus".to_string(),
        "free" => "Free".to_string(),
        other => other.to_string(),
    });

    Ok(ProviderRateLimits {
        provider: "codex".to_string(),
        plan_tier,
        windows,
        extra_usage: None,
        stale: false,
        error: None,
        retry_after_seconds: None,
        cooldown_until: None,
        fetched_at: Local::now().to_rfc3339(),
    })
}

fn codex_window_to_rate_limit(id: &str, w: &CodexWindowData) -> RateLimitWindow {
    let label = match (id, w.window_minutes) {
        ("primary", 300) => "Session (5hr)".to_string(),
        ("primary", _) => format!("Primary ({}m)", w.window_minutes),
        ("secondary", 10080) => "Weekly (7 day)".to_string(),
        ("secondary", _) => format!("Secondary ({}m)", w.window_minutes),
        _ => id.to_string(),
    };

    let resets_at =
        DateTime::<Utc>::from_timestamp(w.resets_at as i64, 0).map(|dt| dt.to_rfc3339());

    RateLimitWindow {
        window_id: id.to_string(),
        label,
        utilization: w.used_percent,
        resets_at,
    }
}

fn find_newest_jsonl(
    dir: &Path,
    newest: &mut Option<(std::time::SystemTime, std::path::PathBuf)>,
    depth: u32,
) {
    if depth > 5 {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_newest_jsonl(&path, newest, depth + 1);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            if let Ok(meta) = std::fs::metadata(&path) {
                if let Ok(mtime) = meta.modified() {
                    if newest.as_ref().is_none_or(|(prev, _)| mtime > *prev) {
                        *newest = Some((mtime, path));
                    }
                }
            }
        }
    }
}
