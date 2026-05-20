use crate::models::{ExtraUsageInfo, ProviderRateLimits, RateLimitWindow};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;

use super::http::rate_limit_error_from_response;
use super::RateLimitFetchError;

const CURSOR_USAGE_URL: &str =
    "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorPeriodUsageResponse {
    billing_cycle_end: Option<String>,
    plan_usage: Option<CursorPlanUsage>,
    spend_limit_usage: Option<CursorSpendLimitUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorPlanUsage {
    #[allow(dead_code)]
    total_percent_used: Option<f64>,
    auto_percent_used: Option<f64>,
    api_percent_used: Option<f64>,
    limit: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorSpendLimitUsage {
    individual_limit: Option<f64>,
    individual_used: Option<f64>,
    total_spend: Option<f64>,
}

fn billing_cycle_end_to_rfc3339(raw: &str) -> Option<String> {
    let ms: i64 = raw.parse().ok()?;
    DateTime::<Utc>::from_timestamp_millis(ms).map(|dt| dt.to_rfc3339())
}

fn build_cursor_rate_limits(resp: CursorPeriodUsageResponse) -> ProviderRateLimits {
    let resets_at = resp
        .billing_cycle_end
        .as_deref()
        .and_then(billing_cycle_end_to_rfc3339);

    let mut windows = Vec::new();

    if let Some(plan) = &resp.plan_usage {
        if let Some(auto_pct) = plan.auto_percent_used {
            windows.push(RateLimitWindow::new(
                "auto_composer".to_string(),
                "Auto + Composer".to_string(),
                auto_pct,
                resets_at.clone(),
            ));
        }

        if let Some(api_pct) = plan.api_percent_used {
            let label = if let Some(limit_cents) = plan.limit {
                let limit_dollars = limit_cents / 100.0;
                if limit_dollars == limit_dollars.floor() {
                    format!("API (${} included)", limit_dollars as i64)
                } else {
                    format!("API (${:.2} included)", limit_dollars)
                }
            } else {
                "API".to_string()
            };
            windows.push(RateLimitWindow::new(
                "api".to_string(),
                label,
                api_pct,
                resets_at.clone(),
            ));
        }
    }

    let extra_usage = resp.spend_limit_usage.and_then(|spend| {
        let used_cents = spend.total_spend.or(spend.individual_used)?;
        let limit_cents = spend.individual_limit?;
        let used_dollars = used_cents / 100.0;
        let limit_dollars = limit_cents / 100.0;
        let utilization = if limit_dollars > 0.0 {
            Some((used_dollars / limit_dollars * 100.0).min(100.0))
        } else {
            None
        };
        Some(ExtraUsageInfo {
            is_enabled: true,
            monthly_limit: limit_dollars,
            used_credits: used_dollars,
            utilization,
        })
    });

    ProviderRateLimits {
        provider: "cursor".to_string(),
        plan_tier: None,
        windows,
        extra_usage,
        credits: None,
        stale: false,
        error: None,
        retry_after_seconds: None,
        cooldown_until: None,
        fetched_at: Local::now().to_rfc3339(),
    }
}

pub(super) async fn fetch_cursor_rate_limits() -> Result<ProviderRateLimits, RateLimitFetchError> {
    let token = crate::usage::cursor_parser::read_cursor_ide_access_token().ok_or_else(|| {
        RateLimitFetchError::message(
            "Cursor IDE is not signed in on this machine (no access token found in state.vscdb)",
        )
    })?;

    let client = reqwest::Client::new();
    let response = client
        .post(CURSOR_USAGE_URL)
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .bearer_auth(&token)
        .body("{}")
        .send()
        .await
        .map_err(|e| {
            RateLimitFetchError::message(format!("Cursor usage API request failed: {e}"))
        })?;

    if !response.status().is_success() {
        return Err(rate_limit_error_from_response(&response));
    }

    let body = response
        .text()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Failed to read response body: {e}")))?;

    let parsed: CursorPeriodUsageResponse = serde_json::from_str(&body).map_err(|e| {
        RateLimitFetchError::message(format!("Failed to parse Cursor usage response: {e}"))
    })?;

    Ok(build_cursor_rate_limits(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_windows_from_plan_usage() {
        let resp = CursorPeriodUsageResponse {
            billing_cycle_end: Some("1714521600000".to_string()),
            plan_usage: Some(CursorPlanUsage {
                total_percent_used: Some(23.0),
                auto_percent_used: Some(2.0),
                api_percent_used: Some(100.0),
                limit: Some(7000.0),
            }),
            spend_limit_usage: None,
        };

        let result = build_cursor_rate_limits(resp);

        assert_eq!(result.provider, "cursor");
        assert_eq!(result.windows.len(), 2);
        assert_eq!(result.windows[0].window_id, "auto_composer");
        assert_eq!(result.windows[0].label, "Auto + Composer");
        assert_eq!(result.windows[0].utilization, 2.0);
        assert!(result.windows[0].resets_at.is_some());
        assert_eq!(result.windows[1].window_id, "api");
        assert_eq!(result.windows[1].label, "API ($70 included)");
        assert_eq!(result.windows[1].utilization, 100.0);
    }

    #[test]
    fn builds_extra_usage_from_spend_limit() {
        let resp = CursorPeriodUsageResponse {
            billing_cycle_end: None,
            plan_usage: None,
            spend_limit_usage: Some(CursorSpendLimitUsage {
                individual_limit: Some(5000.0),
                individual_used: Some(1107.0),
                total_spend: Some(1107.0),
            }),
        };

        let result = build_cursor_rate_limits(resp);

        let extra = result.extra_usage.unwrap();
        assert!(extra.is_enabled);
        assert!((extra.monthly_limit - 50.0).abs() < 0.01);
        assert!((extra.used_credits - 11.07).abs() < 0.01);
        assert!(extra.utilization.unwrap() > 0.0);
    }

    #[test]
    fn handles_empty_response_gracefully() {
        let resp = CursorPeriodUsageResponse {
            billing_cycle_end: None,
            plan_usage: None,
            spend_limit_usage: None,
        };

        let result = build_cursor_rate_limits(resp);

        assert!(result.windows.is_empty());
        assert!(result.extra_usage.is_none());
        assert!(result.error.is_none());
    }

    #[test]
    fn billing_cycle_end_parses_unix_ms_string() {
        let rfc = billing_cycle_end_to_rfc3339("1714521600000").unwrap();
        assert!(rfc.contains("2024-05-01"));
    }
}
