use crate::models::{ProviderRateLimits, RateLimitWindow};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::SystemTime;

const CODEX_FILE_STALE_SECS: u64 = 600;

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

    let (file_mtime, file_path) =
        newest_file.ok_or_else(|| "No Codex session files found".to_string())?;

    // Read from the end looking for rate_limits
    tracing::debug!(path = %file_path.display(), "opening file (codex rate limits)");
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

    RateLimitWindow::new(id.to_string(), label, w.used_percent, resets_at)
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
    use std::time::Duration;

    #[test]
    fn codex_window_preserves_used_percent_scale() {
        let window = codex_window_to_rate_limit(
            "primary",
            &CodexWindowData {
                used_percent: 1.0,
                window_minutes: 300,
                resets_at: 1_763_128_800,
            },
        );

        assert_eq!(window.label, "Session (5hr)");
        assert_eq!(window.utilization, 1.0);
        assert_eq!(window.window_id, "primary");
    }

    #[test]
    fn stale_when_resets_at_in_the_past() {
        let now = Utc::now();
        let past_reset = (now - chrono::Duration::hours(1)).to_rfc3339();
        let windows = vec![RateLimitWindow::new(
            "primary".into(),
            "Session (5hr)".into(),
            50.0,
            Some(past_reset),
        )];
        let recent_mtime = SystemTime::now();
        assert!(is_codex_data_stale(recent_mtime, &windows, now));
    }

    #[test]
    fn not_stale_when_recent_file_and_future_reset() {
        let now = Utc::now();
        let future_reset = (now + chrono::Duration::hours(3)).to_rfc3339();
        let windows = vec![RateLimitWindow::new(
            "primary".into(),
            "Session (5hr)".into(),
            23.0,
            Some(future_reset),
        )];
        let recent_mtime = SystemTime::now();
        assert!(!is_codex_data_stale(recent_mtime, &windows, now));
    }

    #[test]
    fn stale_when_file_is_old_even_with_future_reset() {
        let now = Utc::now();
        let future_reset = (now + chrono::Duration::hours(3)).to_rfc3339();
        let windows = vec![RateLimitWindow::new(
            "primary".into(),
            "Session (5hr)".into(),
            23.0,
            Some(future_reset),
        )];
        let old_mtime = SystemTime::now() - Duration::from_secs(CODEX_FILE_STALE_SECS + 60);
        assert!(is_codex_data_stale(old_mtime, &windows, now));
    }

    #[test]
    fn not_stale_with_empty_windows() {
        let now = Utc::now();
        let recent_mtime = SystemTime::now();
        assert!(!is_codex_data_stale(recent_mtime, &[], now));
    }
}
