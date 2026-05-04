//! Reader for the JSONL event file the statusline script appends to.
//!
//! The file grows with every prompt the user fires in CC, so we trim it on
//! read whenever it crosses a soft size budget. The reader is read-only from
//! the caller's perspective — `latest_active_session()` returns the most
//! recently observed session info or `None` when the file is empty / missing.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

/// Soft cap on the events file. When exceeded on read we keep only the last
/// `RETAIN_TAIL_LINES` lines. CC fires the statusline on every prompt; a 1 MB
/// budget holds tens of thousands of events, more than any sensible debug
/// window.
const SOFT_SIZE_LIMIT_BYTES: u64 = 1_024 * 1_024;
const RETAIN_TAIL_LINES: usize = 200;

/// One envelope appended by the statusline script. We deliberately avoid
/// strict typing on `payload` so a CC version bump that adds new fields
/// doesn't break parsing — we only read the fields we care about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatuslineEvent {
    /// RFC3339 UTC timestamp of when the script ran.
    pub ts: String,
    /// Raw JSON object Claude Code wrote on stdin.
    pub payload: serde_json::Value,
}

/// One rate-limit window pulled out of `payload.rate_limits` — Claude Code
/// already computes utilization% and reset time server-side and ships them
/// in the statusline JSON, so we just relay them.
#[derive(Debug, Clone, Copy)]
pub struct StatuslineWindow {
    pub used_percentage: f64,
    /// Unix epoch seconds. Convert to RFC3339 with `DateTime::from_timestamp`.
    pub resets_at_unix: i64,
}

/// What the rest of the app cares about: which session and model is currently
/// active, when we last heard from CC, where its transcript lives, and the
/// server-side rate-limit windows CC bundled with the statusline event.
///
/// `transcript_path`, `model_id`, and `cwd` aren't consumed by the rate-limit
/// pipeline yet, but they're plumbed through because the IPC
/// `read_latest_statusline_ping` surfaces them to the onboarding UI for the
/// "we just saw your prompt" confirmation.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct LatestActiveSession {
    /// CC session UUID (`payload.session_id`).
    pub session_id: Option<String>,
    /// Path to the session's JSONL transcript (`payload.transcript_path`).
    pub transcript_path: Option<String>,
    /// Currently selected model id (`payload.model.id`).
    pub model_id: Option<String>,
    /// User-facing model name (`payload.model.display_name`).
    pub model_display_name: Option<String>,
    /// Working directory when the prompt fired (`payload.cwd`).
    pub cwd: Option<String>,
    /// Timestamp the script wrote — i.e. when CC last fired its prompt hook.
    pub last_seen: DateTime<Utc>,
    /// 5h window from `payload.rate_limits.five_hour`. Absent on very old
    /// CC versions that don't ship the field.
    pub five_hour: Option<StatuslineWindow>,
    /// 7-day window from `payload.rate_limits.seven_day`.
    pub seven_day: Option<StatuslineWindow>,
}

impl LatestActiveSession {
    /// Whether the session is active "now": last prompt within `freshness`.
    /// Used to decide if we should tag rate-limit data as `stale`.
    pub fn is_fresh(&self, freshness: Duration, now: DateTime<Utc>) -> bool {
        now - self.last_seen <= freshness
    }
}

/// Read the events file and return the most recent envelope's session info.
/// Returns `Ok(None)` when the file doesn't exist or contains no parseable
/// lines — a brand-new install before CC has fired.
pub fn latest_active_session(events_file: &Path) -> io::Result<Option<LatestActiveSession>> {
    if !events_file.exists() {
        return Ok(None);
    }
    let file = fs::File::open(events_file)?;
    let reader = BufReader::new(file);

    let mut latest: Option<(DateTime<Utc>, StatuslineEvent)> = None;
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<StatuslineEvent>(trimmed) else {
            continue;
        };
        let Ok(ts) = DateTime::parse_from_rfc3339(&event.ts) else {
            continue;
        };
        let ts_utc = ts.with_timezone(&Utc);
        match latest.as_ref() {
            Some((prev, _)) if *prev >= ts_utc => {}
            _ => latest = Some((ts_utc, event)),
        }
    }

    Ok(latest.map(|(ts, event)| {
        let payload = &event.payload;
        LatestActiveSession {
            session_id: string_field(payload, "session_id"),
            transcript_path: string_field(payload, "transcript_path"),
            model_id: payload
                .get("model")
                .and_then(|m| m.get("id"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            model_display_name: payload
                .get("model")
                .and_then(|m| m.get("display_name"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            cwd: string_field(payload, "cwd"),
            last_seen: ts,
            five_hour: extract_window(payload, "five_hour"),
            seven_day: extract_window(payload, "seven_day"),
        }
    }))
}

fn string_field(payload: &serde_json::Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Pull `payload.rate_limits.<window>.{used_percentage, resets_at}` out of
/// the CC envelope. Absent on older CC versions; the caller falls back to
/// the JSONL aggregator in that case.
fn extract_window(payload: &serde_json::Value, name: &str) -> Option<StatuslineWindow> {
    let window = payload.get("rate_limits")?.get(name)?;
    let used_percentage = window
        .get("used_percentage")
        .and_then(|v| v.as_f64())
        .or_else(|| {
            window
                .get("used_percentage")
                .and_then(|v| v.as_u64())
                .map(|n| n as f64)
        })?;
    let resets_at_unix = window.get("resets_at").and_then(|v| v.as_i64())?;
    Some(StatuslineWindow {
        used_percentage,
        resets_at_unix,
    })
}

/// Trim the events file in place when it grows past `SOFT_SIZE_LIMIT_BYTES`.
/// Keeps only the last `RETAIN_TAIL_LINES` lines so live tailing still has
/// some history. Errors are swallowed — retention is best-effort and a
/// failed trim shouldn't break a rate-limit refresh.
pub fn maybe_trim(events_file: &Path) {
    let Ok(meta) = fs::metadata(events_file) else {
        return;
    };
    if meta.len() <= SOFT_SIZE_LIMIT_BYTES {
        return;
    }

    let Ok(file) = fs::File::open(events_file) else {
        return;
    };
    // Read all lines into memory. The soft cap is 1 MB so this is fine; if we
    // ever raise the cap, swap to a tail-seek strategy.
    let lines: Vec<String> = BufReader::new(file).lines().map_while(Result::ok).collect();

    let kept = if lines.len() > RETAIN_TAIL_LINES {
        &lines[lines.len() - RETAIN_TAIL_LINES..]
    } else {
        &lines[..]
    };

    // Write back via a sibling temp + rename so a concurrent appender from
    // the statusline script doesn't see a half-truncated file.
    let tmp = events_file.with_extension("jsonl.tmp");
    let Ok(mut out) = fs::File::create(&tmp) else {
        return;
    };
    for line in kept {
        if writeln!(out, "{line}").is_err() {
            return;
        }
    }
    let _ = fs::rename(&tmp, events_file);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_events(dir: &Path, events: &[&str]) -> std::path::PathBuf {
        let path = dir.join("events.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        for e in events {
            writeln!(file, "{e}").unwrap();
        }
        path
    }

    #[test]
    fn returns_none_when_file_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.jsonl");
        assert!(latest_active_session(&path).unwrap().is_none());
    }

    #[test]
    fn picks_latest_by_timestamp() {
        let dir = tempdir().unwrap();
        let path = write_events(
            dir.path(),
            &[
                r#"{"ts":"2026-04-29T10:00:00Z","payload":{"session_id":"old","model":{"id":"opus-4-7"}}}"#,
                r#"{"ts":"2026-04-29T11:00:00Z","payload":{"session_id":"new","model":{"id":"sonnet-4-6"}}}"#,
            ],
        );
        let session = latest_active_session(&path).unwrap().unwrap();
        assert_eq!(session.session_id.as_deref(), Some("new"));
        assert_eq!(session.model_id.as_deref(), Some("sonnet-4-6"));
    }

    #[test]
    fn skips_malformed_lines() {
        let dir = tempdir().unwrap();
        let path = write_events(
            dir.path(),
            &[
                "not-json-at-all",
                r#"{"ts":"bad-timestamp","payload":{}}"#,
                r#"{"ts":"2026-04-29T10:00:00Z","payload":{"session_id":"good"}}"#,
            ],
        );
        let session = latest_active_session(&path).unwrap().unwrap();
        assert_eq!(session.session_id.as_deref(), Some("good"));
    }

    #[test]
    fn freshness_check() {
        let now = Utc::now();
        let session = LatestActiveSession {
            session_id: None,
            transcript_path: None,
            model_id: None,
            model_display_name: None,
            cwd: None,
            last_seen: now - Duration::minutes(2),
            five_hour: None,
            seven_day: None,
        };
        assert!(session.is_fresh(Duration::minutes(5), now));
        assert!(!session.is_fresh(Duration::minutes(1), now));
    }

    #[test]
    fn extracts_rate_limit_windows_from_payload() {
        let dir = tempdir().unwrap();
        let path = write_events(
            dir.path(),
            &[
                r#"{"ts":"2026-04-29T11:00:00Z","payload":{"session_id":"s1","rate_limits":{"five_hour":{"used_percentage":6,"resets_at":1777516200},"seven_day":{"used_percentage":21,"resets_at":1777622400}}}}"#,
            ],
        );
        let session = latest_active_session(&path).unwrap().unwrap();
        let five_hour = session.five_hour.expect("five_hour window");
        assert_eq!(five_hour.used_percentage, 6.0);
        assert_eq!(five_hour.resets_at_unix, 1_777_516_200);
        let seven_day = session.seven_day.expect("seven_day window");
        assert_eq!(seven_day.used_percentage, 21.0);
        assert_eq!(seven_day.resets_at_unix, 1_777_622_400);
    }

    #[test]
    fn missing_rate_limits_field_yields_none() {
        let dir = tempdir().unwrap();
        let path = write_events(
            dir.path(),
            &[r#"{"ts":"2026-04-29T11:00:00Z","payload":{"session_id":"s1"}}"#],
        );
        let session = latest_active_session(&path).unwrap().unwrap();
        assert!(session.five_hour.is_none());
        assert!(session.seven_day.is_none());
    }

    #[test]
    fn trim_no_op_when_under_limit() {
        let dir = tempdir().unwrap();
        let path = write_events(
            dir.path(),
            &[r#"{"ts":"2026-01-01T00:00:00Z","payload":{}}"#],
        );
        let before = fs::read_to_string(&path).unwrap();
        maybe_trim(&path);
        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(before, after);
    }
}
