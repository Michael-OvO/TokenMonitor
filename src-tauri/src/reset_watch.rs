//! Opt-in "reset watch": codex-resets.com's public signal that OpenAI may soon
//! reset everyone's Codex limits. A third-party, community forecast — not
//! OpenAI's, not a calibrated probability. Only the watch level and its times
//! are kept; the request carries no user data.

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResetWatch {
    /// "elevated" or "strong".
    pub level: String,
    pub reset_chance_percent: Option<u8>,
    pub observed_at: String,
    pub expires_at: String,
}

#[derive(Deserialize)]
struct StatusResponse {
    data: StatusData,
}

#[derive(Deserialize)]
struct StatusData {
    active_watch: Option<RawWatch>,
}

#[derive(Deserialize)]
struct RawWatch {
    level: String,
    reset_chance_percent: Option<u8>,
    observed_at: String,
    expires_at: String,
}

/// A watch counts only while it is current and its level is one we know.
fn active_watch(raw: RawWatch, now: DateTime<Utc>) -> Option<ResetWatch> {
    let observed = DateTime::parse_from_rfc3339(&raw.observed_at).ok()?;
    let expires = DateTime::parse_from_rfc3339(&raw.expires_at).ok()?;
    let current =
        observed <= now + ChronoDuration::minutes(5) && expires > now && expires > observed;
    let known = matches!(raw.level.as_str(), "elevated" | "strong");
    (current && known).then(|| ResetWatch {
        level: raw.level,
        reset_chance_percent: raw.reset_chance_percent.filter(|p| *p <= 100),
        observed_at: observed.to_rfc3339(),
        expires_at: expires.to_rfc3339(),
    })
}

async fn fetch() -> Result<Option<ResetWatch>, String> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let status: StatusResponse = client
        .get(crate::ops::codex_resets_status_url())
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(status
        .data
        .active_watch
        .and_then(|w| active_watch(w, Utc::now())))
}

/// Called by the page while the setting is on; at most one request per 10 min.
/// A failed request reads as "no watch" and is retried after the same wait.
#[tauri::command]
pub async fn get_codex_reset_watch() -> Option<ResetWatch> {
    static CACHE: Mutex<Option<(Instant, Option<ResetWatch>)>> = Mutex::new(None);
    if let Some((at, watch)) = CACHE.lock().ok().and_then(|c| c.clone()) {
        if at.elapsed() < CACHE_TTL {
            return watch.filter(|w| {
                DateTime::parse_from_rfc3339(&w.expires_at).is_ok_and(|e| e > Utc::now())
            });
        }
    }
    let watch = fetch().await.unwrap_or_else(|e| {
        tracing::debug!("codex reset watch unavailable: {e}");
        None
    });
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((Instant::now(), watch.clone()));
    }
    watch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Option<ResetWatch> {
        let status: StatusResponse = serde_json::from_str(json).unwrap();
        let now = "2026-09-09T06:00:00Z".parse().unwrap();
        status.data.active_watch.and_then(|w| active_watch(w, now))
    }

    fn watch(level: &str, observed: &str, expires: &str) -> String {
        format!(
            r#"{{"data":{{"latest_reset":null,"active_watch":{{"level":"{level}","reset_chance_percent":70,"observed_at":"{observed}","expires_at":"{expires}"}}}},"meta":{{}}}}"#
        )
    }

    #[test]
    fn keeps_a_current_watch() {
        let w = parse(&watch(
            "strong",
            "2026-09-09T05:00:00Z",
            "2026-09-10T05:00:00Z",
        ))
        .unwrap();
        assert_eq!(w.level, "strong");
        assert_eq!(w.reset_chance_percent, Some(70));
    }

    #[test]
    fn drops_absent_expired_future_unknown_and_malformed_watches() {
        assert!(parse(r#"{"data":{"active_watch":null}}"#).is_none());
        for (level, observed, expires) in [
            ("strong", "2026-09-09T05:00:00Z", "2026-09-09T06:00:00Z"),
            ("strong", "2026-09-11T05:00:00Z", "2026-09-12T05:00:00Z"),
            ("none", "2026-09-09T05:00:00Z", "2026-09-10T05:00:00Z"),
            ("strong", "bad", "2026-09-10T05:00:00Z"),
        ] {
            assert!(
                parse(&watch(level, observed, expires)).is_none(),
                "{level} {observed}"
            );
        }
    }
}
