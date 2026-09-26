use crate::models::{ExtraUsageInfo, ProviderRateLimits, RateLimitWindow};
use crate::usage::cursor_parser::{forget_cursor_ide_token, read_cursor_ide_access_token};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::http::rate_limit_error_from_response;
use super::RateLimitFetchError;

/// Known Cursor plan-usage percent fields, in dashboard display order.
///
/// Cursor currently exposes two pools (docs: "First-party models" and "API").
/// `totalPercentUsed` is a rollup, not a separate dashboard bar, so it is skipped.
/// Any additional `*PercentUsed` field Cursor adds later becomes a new bar
/// automatically via [`plan_usage_windows`].
const KNOWN_PLAN_METERS: &[(&str, &str, &str)] = &[
    // (apiField, windowId, label)
    ("autoPercentUsed", "first_party", "First-party models"),
    ("apiPercentUsed", "api", "API"),
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorPeriodUsageResponse {
    billing_cycle_end: Option<String>,
    plan_usage: Option<Value>,
    spend_limit_usage: Option<CursorSpendLimitUsage>,
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

fn plan_limit_cents(plan: &Value) -> Option<f64> {
    plan.get("limit").and_then(|v| v.as_f64())
}

fn api_window_label(plan: &Value) -> String {
    let Some(limit_cents) = plan_limit_cents(plan) else {
        return "API".to_string();
    };
    // This label lands in the popover beside amounts the frontend has already
    // converted, so it has to follow the user's currency too.
    let limit_dollars = limit_cents / 100.0;
    format!(
        "API ({} included)",
        crate::usage::money::format_auto(limit_dollars)
    )
}

fn label_for_known_meter(window_id: &str, plan: &Value) -> String {
    match window_id {
        "api" => api_window_label(plan),
        _ => KNOWN_PLAN_METERS
            .iter()
            .find(|(_, id, _)| *id == window_id)
            .map(|(_, _, label)| (*label).to_string())
            .unwrap_or_else(|| humanize_percent_field(window_id)),
    }
}

/// Turn `fooBarPercentUsed` / `foo_bar` into a short display label.
fn humanize_percent_field(field: &str) -> String {
    let stem = field
        .strip_suffix("PercentUsed")
        .or_else(|| field.strip_suffix("percent_used"))
        .unwrap_or(field);
    let mut words = Vec::new();
    let mut current = String::new();
    for (i, ch) in stem.chars().enumerate() {
        if ch == '_' || ch == '-' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        if i > 0 && ch.is_uppercase() && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        words.push(current);
    }
    if words.is_empty() {
        return field.to_string();
    }
    words
        .into_iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = first.to_uppercase().collect::<String>();
                    out.push_str(&chars.as_str().to_lowercase());
                    out
                }
                None => word,
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn window_id_for_percent_field(field: &str) -> String {
    let stem = field
        .strip_suffix("PercentUsed")
        .or_else(|| field.strip_suffix("percent_used"))
        .unwrap_or(field);
    let mut out = String::new();
    for (i, ch) in stem.chars().enumerate() {
        if ch == '_' || ch == '-' {
            out.push('_');
            continue;
        }
        if i > 0 && ch.is_uppercase() {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
    }
    if out.is_empty() {
        field.to_string()
    } else {
        out
    }
}

fn as_percent(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Build rate-limit windows from whatever percent meters Cursor returns.
///
/// Known fields get Cursor's current pool names; unknown `*PercentUsed` keys
/// become additional bars so label/count changes track the API without a
/// TokenMonitor release for every Cursor rename of *structure* (new meters).
/// Known display names still need a one-line update in [`KNOWN_PLAN_METERS`]
/// when Cursor renames a pool.
fn plan_usage_windows(plan: &Value, resets_at: Option<String>) -> Vec<RateLimitWindow> {
    let Some(obj) = plan.as_object() else {
        return Vec::new();
    };

    let mut windows = Vec::new();
    let mut consumed = std::collections::HashSet::new();

    for (api_field, window_id, _) in KNOWN_PLAN_METERS {
        consumed.insert(*api_field);
        let Some(pct) = obj.get(*api_field).and_then(as_percent) else {
            continue;
        };
        let budget_usd = if *window_id == "api" {
            plan_limit_cents(plan).map(|cents| cents / 100.0)
        } else {
            None
        };
        windows.push(
            RateLimitWindow::new(
                (*window_id).to_string(),
                label_for_known_meter(window_id, plan),
                pct,
                resets_at.clone(),
            )
            .with_budget_usd(budget_usd),
        );
    }

    // Skip the rollup — Cursor's dashboard bars are the pools, not total%.
    consumed.insert("totalPercentUsed");

    let mut extras: Vec<(&String, f64)> = obj
        .iter()
        .filter(|(key, _)| key.ends_with("PercentUsed") && !consumed.contains(key.as_str()))
        .filter_map(|(key, value)| Some((key, as_percent(value)?)))
        .collect();
    extras.sort_by(|a, b| a.0.cmp(b.0));

    for (field, pct) in extras {
        windows.push(RateLimitWindow::new(
            window_id_for_percent_field(field),
            humanize_percent_field(field),
            pct,
            resets_at.clone(),
        ));
    }

    windows
}

fn build_cursor_rate_limits(resp: CursorPeriodUsageResponse) -> ProviderRateLimits {
    let resets_at = resp
        .billing_cycle_end
        .as_deref()
        .and_then(billing_cycle_end_to_rfc3339);

    let windows = resp
        .plan_usage
        .as_ref()
        .map(|plan| plan_usage_windows(plan, resets_at))
        .unwrap_or_default();

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
    // Off the async worker: a changed state DB means a `sqlite3` spawn.
    let token = tokio::task::spawn_blocking(read_cursor_ide_access_token)
        .await
        .ok()
        .flatten()
        .ok_or_else(|| {
            RateLimitFetchError::message(
                "Cursor IDE is not signed in on this machine (no access token found in state.vscdb)",
            )
        })?;

    // One client for every probe, so its connections and TLS setup are reused.
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let client = match CLIENT.get() {
        Some(client) => client,
        None => {
            let client = reqwest::Client::builder()
                .timeout(super::HTTP_TIMEOUT)
                .build()
                .map_err(|e| {
                    RateLimitFetchError::message(format!("HTTP client build failed: {e}"))
                })?;
            CLIENT.get_or_init(|| client)
        }
    };
    let response = client
        .post(format!(
            "{}/aiserver.v1.DashboardService/GetCurrentPeriodUsage",
            crate::ops::cursor_ide_api_base()
        ))
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
        if matches!(response.status().as_u16(), 401 | 403) {
            forget_cursor_ide_token();
        }
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
    use serde_json::json;

    /// This label is rendered inside the popover, right beside amounts the
    /// frontend has already converted — so a hardcoded `$` here put two
    /// currencies on one screen.
    #[test]
    fn api_window_label_follows_the_selected_currency() {
        let _guard = crate::usage::money::CurrencyGuard::new("EUR");
        crate::usage::exchange_rates::set_exchange_rates(
            [("EUR".to_string(), 0.92)].into_iter().collect(),
        );
        let plan = json!({ "limit": 7000.0 });
        assert_eq!(api_window_label(&plan), "API (\u{20ac}64.40 included)");
    }

    #[test]
    fn api_window_label_keeps_whole_dollars_bare_in_usd() {
        let _guard = crate::usage::money::CurrencyGuard::new("USD");
        let plan = json!({ "limit": 7000.0 });
        assert_eq!(api_window_label(&plan), "API ($70 included)");
    }

    #[test]
    fn builds_windows_from_plan_usage() {
        let _guard = crate::usage::money::CurrencyGuard::new("USD");
        let resp = CursorPeriodUsageResponse {
            billing_cycle_end: Some("1714521600000".to_string()),
            plan_usage: Some(json!({
                "totalPercentUsed": 23.0,
                "autoPercentUsed": 2.0,
                "apiPercentUsed": 100.0,
                "limit": 7000.0,
            })),
            spend_limit_usage: None,
        };

        let result = build_cursor_rate_limits(resp);

        assert_eq!(result.provider, "cursor");
        assert_eq!(result.windows.len(), 2);
        assert_eq!(result.windows[0].window_id, "first_party");
        assert_eq!(result.windows[0].label, "First-party models");
        assert_eq!(result.windows[0].utilization, 2.0);
        assert!(result.windows[0].resets_at.is_some());
        assert_eq!(result.windows[1].window_id, "api");
        assert_eq!(result.windows[1].label, "API ($70 included)");
        assert_eq!(result.windows[1].utilization, 100.0);
    }

    #[test]
    fn omits_missing_meters_so_bar_count_follows_api() {
        let _guard = crate::usage::money::CurrencyGuard::new("USD");
        let resp = CursorPeriodUsageResponse {
            billing_cycle_end: None,
            plan_usage: Some(json!({
                "apiPercentUsed": 40.0,
                "limit": 2000.0,
            })),
            spend_limit_usage: None,
        };

        let result = build_cursor_rate_limits(resp);
        assert_eq!(result.windows.len(), 1);
        assert_eq!(result.windows[0].window_id, "api");
        assert_eq!(result.windows[0].label, "API ($20 included)");
    }

    #[test]
    fn unknown_percent_fields_become_extra_bars() {
        let resp = CursorPeriodUsageResponse {
            billing_cycle_end: None,
            plan_usage: Some(json!({
                "autoPercentUsed": 10.0,
                "apiPercentUsed": 20.0,
                "bonusPercentUsed": 5.5,
            })),
            spend_limit_usage: None,
        };

        let result = build_cursor_rate_limits(resp);
        assert_eq!(result.windows.len(), 3);
        assert_eq!(result.windows[2].window_id, "bonus");
        assert_eq!(result.windows[2].label, "Bonus");
        assert_eq!(result.windows[2].utilization, 5.5);
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

    #[test]
    fn humanizes_camel_case_percent_fields() {
        assert_eq!(humanize_percent_field("bonusPoolPercentUsed"), "Bonus Pool");
        assert_eq!(
            window_id_for_percent_field("bonusPoolPercentUsed"),
            "bonus_pool"
        );
    }
}
