use crate::models::{ProviderRateLimits, RateLimitWindow};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::SystemTime;

use super::{as_f64, humanize_snake_case};

const CODEX_FILE_STALE_SECS: u64 = 600;

/// Non-window keys that appear alongside meters in Codex rate-limit payloads.
const CODEX_META_KEYS: &[&str] = &[
    "plan_type",
    "planType",
    "credits",
    "rate_limit_reached_type",
    "rateLimitReachedType",
    "limit_id",
    "limitId",
    "limit_name",
    "limitName",
];

/// Preferred display order for known Codex windows.
const KNOWN_CODEX_WINDOWS: &[&str] = &["primary", "secondary"];

#[derive(Deserialize)]
struct CodexJsonlLine {
    payload: Option<CodexPayload>,
}

#[derive(Deserialize)]
struct CodexPayload {
    rate_limits: Option<Value>,
}

pub(super) fn extract_codex_rate_limits(codex_dir: &Path) -> Result<ProviderRateLimits, String> {
    // Find the most recent JSONL file by walking the date-based directory structure
    let mut newest_file: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    find_newest_jsonl(codex_dir, &mut newest_file, 0);

    let (file_mtime, file_path) =
        newest_file.ok_or_else(|| "No Codex session files found".to_string())?;

    // Read from the end looking for rate_limits
    tracing::debug!(path = %file_path.display(), "opening file (codex rate limits)");
    let file =
        std::fs::File::open(&file_path).map_err(|e| format!("Failed to open Codex file: {e}"))?;
    let reader = BufReader::new(file);

    let mut last_rate_limits: Option<Value> = None;
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

    let windows = codex_windows_from_rate_limits(&rl);
    let plan_tier = rl
        .get("plan_type")
        .or_else(|| rl.get("planType"))
        .and_then(|v| v.as_str())
        .map(normalize_plan_type_label);

    let stale = is_codex_data_stale(file_mtime, &windows, Utc::now());

    Ok(ProviderRateLimits {
        provider: "codex".to_string(),
        plan_tier,
        windows,
        extra_usage: None,
        credits: None,
        stale,
        error: None,
        retry_after_seconds: None,
        cooldown_until: None,
        fetched_at: Local::now().to_rfc3339(),
    })
}

fn normalize_plan_type_label(plan_type: &str) -> String {
    match plan_type {
        "pro" => "Pro".to_string(),
        "plus" => "Plus".to_string(),
        "free" => "Free".to_string(),
        other => other.to_string(),
    }
}

fn is_codex_data_stale(
    file_mtime: SystemTime,
    windows: &[RateLimitWindow],
    now: DateTime<Utc>,
) -> bool {
    for window in windows {
        if let Some(resets_at) = &window.resets_at {
            if let Ok(reset_dt) = DateTime::parse_from_rfc3339(resets_at) {
                if now > reset_dt.with_timezone(&Utc) {
                    return true;
                }
            }
        }
    }

    if let Ok(elapsed) = file_mtime.elapsed() {
        if elapsed.as_secs() > CODEX_FILE_STALE_SECS {
            return true;
        }
    }

    false
}

/// Build rate-limit windows from whatever meters Codex returns.
///
/// Known `primary` / `secondary` keep duration-aware labels; any additional
/// object with `used_percent` / `usedPercent` becomes another bar so count
/// changes track the source without a TokenMonitor release.
pub(super) fn codex_windows_from_rate_limits(rl: &Value) -> Vec<RateLimitWindow> {
    let Some(obj) = rl.as_object() else {
        return Vec::new();
    };

    let mut windows = Vec::new();
    let mut consumed = std::collections::HashSet::new();
    for key in CODEX_META_KEYS {
        consumed.insert(*key);
    }

    for id in KNOWN_CODEX_WINDOWS {
        consumed.insert(*id);
        let Some(value) = obj.get(*id) else {
            continue;
        };
        if let Some(window) = codex_value_to_window(id, value) {
            windows.push(window);
        }
    }

    let mut extras: Vec<(&String, &Value)> = obj
        .iter()
        .filter(|(key, value)| !consumed.contains(key.as_str()) && looks_like_codex_window(value))
        .collect();
    extras.sort_by(|a, b| a.0.cmp(b.0));

    for (id, value) in extras {
        if let Some(window) = codex_value_to_window(id, value) {
            windows.push(window);
        }
    }

    windows
}

fn looks_like_codex_window(value: &Value) -> bool {
    value
        .get("used_percent")
        .or_else(|| value.get("usedPercent"))
        .and_then(as_f64)
        .is_some()
}

fn codex_value_to_window(id: &str, value: &Value) -> Option<RateLimitWindow> {
    let used_percent = value
        .get("used_percent")
        .or_else(|| value.get("usedPercent"))
        .and_then(as_f64)?;
    let window_minutes = value
        .get("window_minutes")
        .or_else(|| value.get("windowDurationMins"))
        .or_else(|| value.get("window_duration_mins"))
        .and_then(|v| v.as_u64())
        .unwrap_or(if id == "primary" { 300 } else { 10_080 });
    let resets_at = value
        .get("resets_at")
        .or_else(|| value.get("resetsAt"))
        .and_then(|v| v.as_u64())
        .and_then(|ts| DateTime::<Utc>::from_timestamp(ts as i64, 0).map(|dt| dt.to_rfc3339()));

    Some(RateLimitWindow::new(
        id.to_string(),
        codex_window_label(id, window_minutes),
        used_percent,
        resets_at,
    ))
}

pub(super) fn codex_window_label(id: &str, minutes: u64) -> String {
    match (id, minutes) {
        ("primary", 300) => "Session (5hr)".to_string(),
        ("secondary", 10080) => "Weekly (7 day)".to_string(),
        ("primary", _) => format!("Primary ({})", format_window_duration(minutes)),
        ("secondary", _) => format!("Secondary ({})", format_window_duration(minutes)),
        _ => format!(
            "{} ({})",
            humanize_snake_case(id),
            format_window_duration(minutes)
        ),
    }
}

fn format_window_duration(minutes: u64) -> String {
    let days = minutes / 1_440;
    let hours = minutes % 1_440 / 60;
    let minutes = minutes % 60;

    match (days, hours, minutes) {
        (days, 0, 0) if days > 0 => format!("{days}d"),
        (days, hours, 0) if days > 0 => format!("{days}d {hours}h"),
        (days, hours, minutes) if days > 0 => format!("{days}d {hours}h {minutes}m"),
        (0, hours, 0) if hours > 0 => format!("{hours}h"),
        (0, hours, minutes) if hours > 0 => format!("{hours}h {minutes}m"),
        _ => format!("{minutes}m"),
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
    tracing::debug!(path = %dir.display(), depth, "read_dir (codex find_newest_jsonl)");
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(path = %dir.display(), error = %e, "read_dir failed");
            return;
        }
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            // Don't follow symlinks — they may cross onto network/external
            // volumes and trip macOS TCC prompts.
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn codex_window_preserves_used_percent_scale() {
        let windows = codex_windows_from_rate_limits(&json!({
            "primary": {
                "used_percent": 1.0,
                "window_minutes": 300,
                "resets_at": 1_763_128_800_u64
            }
        }));

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].label, "Session (5hr)");
        assert_eq!(windows[0].utilization, 1.0);
        assert_eq!(windows[0].window_id, "primary");
    }

    #[test]
    fn formats_long_primary_window_as_days_and_hours() {
        assert_eq!(codex_window_label("primary", 10_080), "Primary (7d)");
        assert_eq!(codex_window_label("secondary", 2_040), "Secondary (1d 10h)");
    }

    #[test]
    fn unknown_codex_meters_become_extra_bars() {
        let windows = codex_windows_from_rate_limits(&json!({
            "primary": {
                "usedPercent": 10.0,
                "windowDurationMins": 300,
                "resetsAt": 1_763_128_800_u64
            },
            "bonus_pool": {
                "used_percent": 5.0,
                "window_minutes": 1440,
                "resets_at": 1_763_215_200_u64
            },
            "plan_type": "plus"
        }));

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].window_id, "primary");
        assert_eq!(windows[1].window_id, "bonus_pool");
        assert_eq!(windows[1].label, "Bonus Pool (1d)");
        assert_eq!(windows[1].utilization, 5.0);
    }

    #[test]
    fn omits_missing_codex_meters() {
        let windows = codex_windows_from_rate_limits(&json!({
            "secondary": {
                "used_percent": 40.0,
                "window_minutes": 10080,
                "resets_at": 1_763_215_200_u64
            }
        }));
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].window_id, "secondary");
        assert_eq!(windows[0].label, "Weekly (7 day)");
    }

    #[test]
    fn marks_stale_when_any_window_has_reset() {
        let windows = vec![RateLimitWindow::new(
            "primary".into(),
            "Session (5hr)".into(),
            10.0,
            Some("2020-01-01T00:00:00Z".into()),
        )];
        let now = Utc::now();
        let mtime = SystemTime::now();
        assert!(is_codex_data_stale(mtime, &windows, now));
    }

    #[test]
    fn marks_stale_when_file_mtime_is_old() {
        let windows = vec![RateLimitWindow::new(
            "primary".into(),
            "Session (5hr)".into(),
            10.0,
            Some("2099-01-01T00:00:00Z".into()),
        )];
        let now = Utc::now();
        let mtime = SystemTime::now() - Duration::from_secs(CODEX_FILE_STALE_SECS + 1);
        assert!(is_codex_data_stale(mtime, &windows, now));
    }

    #[test]
    fn fresh_when_windows_active_and_file_recent() {
        let windows = vec![RateLimitWindow::new(
            "primary".into(),
            "Session (5hr)".into(),
            10.0,
            Some("2099-01-01T00:00:00Z".into()),
        )];
        let now = Utc::now();
        let mtime = SystemTime::now();
        assert!(!is_codex_data_stale(mtime, &windows, now));
    }
}
