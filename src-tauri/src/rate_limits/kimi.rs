use crate::models::{ProviderRateLimits, RateLimitWindow};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;

use super::http::rate_limit_error_from_response;
use super::RateLimitFetchError;

const KIMI_USAGE_URL: &str = "https://api.kimi.com/coding/v1/usages";

/// Kimi Code CLI credential file shape (`credentials/kimi-code.json`).
#[derive(Debug, Deserialize)]
struct KimiCredentials {
    access_token: String,
}

fn read_kimi_access_token() -> Result<String, RateLimitFetchError> {
    let path = crate::paths::kimi_credentials_file().ok_or_else(|| {
        RateLimitFetchError::message("Kimi Code CLI is not signed in on this machine")
    })?;

    let raw = std::fs::read_to_string(&path).map_err(|e| {
        RateLimitFetchError::message(format!(
            "Failed to read Kimi credentials at {}: {e}",
            path.display()
        ))
    })?;

    let parsed: KimiCredentials = serde_json::from_str(&raw).map_err(|e| {
        RateLimitFetchError::message(format!(
            "Failed to parse Kimi credentials at {}: {e}",
            path.display()
        ))
    })?;

    if parsed.access_token.is_empty() {
        return Err(RateLimitFetchError::message(
            "Kimi credentials file contains an empty access token",
        ));
    }

    Ok(parsed.access_token)
}

/// Response shape of `GET /coding/v1/usages` (verified against the live API):
///
/// ```json
/// {"usage":{"limit":"100","remaining":"100","resetTime":"2026-07-30T20:42:35Z"},
///  "limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},
///             "detail":{"limit":"100","used":"2","remaining":"98","resetTime":"..."}}]}
/// ```
///
/// The API is camelCase; the account-level `usage` summary reports only
/// `remaining` (no `used`), so usage is derived as `limit - remaining`.
#[derive(Debug, Deserialize)]
struct KimiUsagesResponse {
    #[serde(default)]
    usage: Option<KimiUsageDetail>,
    #[serde(default)]
    limits: Vec<KimiLimitItem>,
}

#[derive(Debug, Deserialize)]
struct KimiLimitItem {
    #[serde(default)]
    window: Option<KimiWindow>,
    #[serde(default)]
    detail: Option<KimiUsageDetail>,
}

#[derive(Debug, Deserialize)]
struct KimiWindow {
    #[serde(default)]
    duration: Option<u64>,
    #[serde(default, alias = "timeUnit")]
    time_unit: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct KimiUsageDetail {
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    used: Option<String>,
    #[serde(default)]
    remaining: Option<String>,
    #[serde(default, alias = "resetTime")]
    reset_time: Option<String>,
}

fn parse_usage_number(raw: Option<&str>) -> Option<u64> {
    raw.and_then(|v| v.trim().parse::<u64>().ok())
}

/// Tokens consumed in the window: reported `used` when present, otherwise
/// derived from `limit - remaining` (the account summary omits `used`).
fn used_tokens(detail: &KimiUsageDetail) -> Option<u64> {
    if let Some(used) = parse_usage_number(detail.used.as_deref()) {
        return Some(used);
    }
    match (
        parse_usage_number(detail.limit.as_deref()),
        parse_usage_number(detail.remaining.as_deref()),
    ) {
        (Some(limit), Some(remaining)) => Some(limit.saturating_sub(remaining)),
        _ => None,
    }
}

fn window_label(window: &Option<KimiWindow>) -> String {
    let Some(window) = window else {
        return "Usage".to_string();
    };

    let duration = window.duration.unwrap_or(0);
    let unit = window.time_unit.as_deref().unwrap_or("");

    // Kimi uses TIME_UNIT_MINUTE with a 300-minute window for the 5h limit.
    if unit == "TIME_UNIT_MINUTE" && duration == 300 {
        return "5h limit".to_string();
    }

    if duration == 0 {
        return "Usage".to_string();
    }

    let unit_label = if unit == "TIME_UNIT_MINUTE" {
        "min"
    } else if unit == "TIME_UNIT_HOUR" {
        "hr"
    } else if unit == "TIME_UNIT_DAY" {
        "day"
    } else if unit == "TIME_UNIT_WEEK" {
        "wk"
    } else {
        ""
    };

    if unit_label.is_empty() {
        format!("{duration} limit")
    } else {
        format!("{duration} {unit_label} limit")
    }
}

fn window_id(window: &Option<KimiWindow>) -> String {
    let Some(window) = window else {
        return "summary".to_string();
    };

    let duration = window.duration.unwrap_or(0);
    let unit = window.time_unit.as_deref().unwrap_or("");

    if unit == "TIME_UNIT_MINUTE" && duration == 300 {
        return "five_hour".to_string();
    }

    if duration == 0 {
        return "summary".to_string();
    }

    format!(
        "{}_{}",
        unit.to_lowercase().replace("time_unit_", ""),
        duration
    )
}

fn build_kimi_rate_limits(resp: KimiUsagesResponse) -> ProviderRateLimits {
    let mut windows = Vec::new();

    // The top-level `usage` object is the account-level (weekly) summary.
    if let Some(summary) = resp.usage.as_ref() {
        if let Some(window) = build_window("summary", "Weekly limit", summary) {
            windows.push(window);
        }
    }

    // `limits` holds per-window breakdowns (e.g., the 5h window).
    for item in &resp.limits {
        let id = window_id(&item.window);
        let label = window_label(&item.window);
        if let Some(window) = build_window(
            &id,
            &label,
            item.detail.as_ref().unwrap_or(&KimiUsageDetail {
                limit: None,
                used: None,
                remaining: None,
                reset_time: None,
            }),
        ) {
            windows.push(window);
        }
    }

    ProviderRateLimits {
        provider: "kimi".to_string(),
        plan_tier: None,
        windows,
        extra_usage: None,
        credits: None,
        stale: false,
        error: None,
        retry_after_seconds: None,
        cooldown_until: None,
        fetched_at: Local::now().to_rfc3339(),
    }
}

fn build_window(window_id: &str, label: &str, detail: &KimiUsageDetail) -> Option<RateLimitWindow> {
    let limit = parse_usage_number(detail.limit.as_deref())?;
    let used = used_tokens(detail).unwrap_or(0);

    if limit == 0 {
        return None;
    }

    let utilization = (used as f64 / limit as f64 * 100.0).min(100.0);
    let resets_at = detail
        .reset_time
        .as_deref()
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|dt| dt.with_timezone(&Utc).to_rfc3339());

    Some(RateLimitWindow::new(
        window_id.to_string(),
        label.to_string(),
        utilization,
        resets_at,
    ))
}

pub(super) async fn fetch_kimi_rate_limits() -> Result<ProviderRateLimits, RateLimitFetchError> {
    let token = read_kimi_access_token()?;

    let client = reqwest::Client::new();
    let response = client
        .get(KIMI_USAGE_URL)
        .header("Accept", "application/json")
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Kimi usage API request failed: {e}")))?;

    if !response.status().is_success() {
        return Err(rate_limit_error_from_response(&response));
    }

    let body = response
        .text()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Failed to read response body: {e}")))?;

    let parsed: KimiUsagesResponse = serde_json::from_str(&body).map_err(|e| {
        RateLimitFetchError::message(format!("Failed to parse Kimi usage response: {e}"))
    })?;

    Ok(build_kimi_rate_limits(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the live `/coding/v1/usages` response: camelCase keys, and the
    /// account summary carrying only `remaining` (no `used`).
    #[test]
    fn parses_live_response_shape() {
        let body = r#"{
            "user": {"userId": "u", "membership": {"level": "LEVEL_INTERMEDIATE"}},
            "usage": {"limit": "100", "remaining": "84", "resetTime": "2026-07-30T20:42:35.877791Z"},
            "limits": [{
                "window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"},
                "detail": {"limit": "100", "used": "2", "remaining": "98", "resetTime": "2026-07-24T02:42:35.877791Z"}
            }],
            "parallel": {"limit": "20"}
        }"#;
        let parsed: KimiUsagesResponse = serde_json::from_str(body).unwrap();
        let limits = build_kimi_rate_limits(parsed);

        assert_eq!(limits.provider, "kimi");
        assert_eq!(limits.windows.len(), 2);

        let weekly = &limits.windows[0];
        assert_eq!(weekly.window_id, "summary");
        assert_eq!(weekly.label, "Weekly limit");
        // used derived from limit - remaining = 100 - 84 = 16.
        assert!((weekly.utilization - 16.0).abs() < 0.01);
        assert_eq!(
            weekly.resets_at.as_deref(),
            Some("2026-07-30T20:42:35.877791+00:00")
        );

        let five_hour = &limits.windows[1];
        assert_eq!(five_hour.window_id, "five_hour");
        assert_eq!(five_hour.label, "5h limit");
        assert!((five_hour.utilization - 2.0).abs() < 0.01);
        assert_eq!(
            five_hour.resets_at.as_deref(),
            Some("2026-07-24T02:42:35.877791+00:00")
        );
    }

    #[test]
    fn skips_zero_limit_windows() {
        let resp = KimiUsagesResponse {
            usage: Some(KimiUsageDetail {
                limit: Some("0".to_string()),
                used: Some("0".to_string()),
                remaining: None,
                reset_time: None,
            }),
            limits: vec![],
        };

        let limits = build_kimi_rate_limits(resp);
        assert!(limits.windows.is_empty());
    }

    #[test]
    fn handles_missing_top_level_usage() {
        let resp = KimiUsagesResponse {
            usage: None,
            limits: vec![KimiLimitItem {
                window: Some(KimiWindow {
                    duration: Some(60),
                    time_unit: Some("TIME_UNIT_MINUTE".to_string()),
                }),
                detail: Some(KimiUsageDetail {
                    limit: Some("60".to_string()),
                    used: Some("30".to_string()),
                    remaining: None,
                    reset_time: None,
                }),
            }],
        };

        let limits = build_kimi_rate_limits(resp);
        assert_eq!(limits.windows.len(), 1);
        assert_eq!(limits.windows[0].window_id, "minute_60");
        assert_eq!(limits.windows[0].label, "60 min limit");
        assert!((limits.windows[0].utilization - 50.0).abs() < 0.01);
    }

    #[test]
    fn saturates_when_remaining_exceeds_limit() {
        let detail = KimiUsageDetail {
            limit: Some("100".to_string()),
            used: None,
            remaining: Some("150".to_string()),
            reset_time: None,
        };
        assert_eq!(used_tokens(&detail), Some(0));
    }
}
