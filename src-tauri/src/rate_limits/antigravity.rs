//! Antigravity model quotas, asked of the running app.
//!
//! A running Antigravity writes `~/.gemini/antigravity/daemon/hub_*.json`
//! with its local server's port and CSRF token. `GetUserStatus` on that
//! server returns the plan and, per model, the fraction of quota left and
//! when it resets. Nothing leaves the machine, and there are no credentials
//! of our own to keep.
//!
//! ponytail: no reading while Antigravity is closed (the last one is kept).
//! The cloud endpoint would need Antigravity's OAuth token; add if wanted.

use crate::models::{ProviderRateLimits, RateLimitWindow};
use chrono::Local;
use serde::Deserialize;
use std::time::Duration;

use super::RateLimitFetchError;

/// Not a failure: the UI shows its idle summary for it.
pub(super) const NOT_RUNNING: &str = "Antigravity is not running";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hub {
    http_port: u16,
    csrf_token: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct StatusResponse {
    user_status: UserStatus,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct UserStatus {
    user_tier: Named,
    cascade_model_config_data: ModelConfigData,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Named {
    name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ModelConfigData {
    client_model_configs: Vec<ModelConfig>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ModelConfig {
    label: String,
    quota_info: QuotaInfo,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct QuotaInfo {
    remaining_fraction: Option<f64>,
    reset_time: Option<String>,
}

/// "Gemini 4 Hydrogen (High)" → "Gemini 4 Hydrogen": the effort variants of
/// one model share its quota.
fn model_family(label: &str) -> &str {
    match label.rfind(" (") {
        Some(at) if label.ends_with(')') => &label[..at],
        _ => label,
    }
    .trim()
}

fn build_antigravity_rate_limits(response: StatusResponse) -> ProviderRateLimits {
    let mut windows: Vec<RateLimitWindow> = Vec::new();
    for config in response
        .user_status
        .cascade_model_config_data
        .client_model_configs
    {
        let quota = config.quota_info;
        // An empty `quotaInfo` is a model without a quota. With a reset time
        // but no fraction, the fraction is zero (proto3 JSON omits zeros).
        if quota.remaining_fraction.is_none() && quota.reset_time.is_none() {
            continue;
        }
        let label = model_family(&config.label);
        if label.is_empty() {
            continue;
        }
        let utilization = (1.0 - quota.remaining_fraction.unwrap_or(0.0)).clamp(0.0, 1.0) * 100.0;
        match windows.iter_mut().find(|window| window.label == label) {
            Some(window) => window.utilization = window.utilization.max(utilization),
            None => windows.push(RateLimitWindow::new(
                label
                    .to_lowercase()
                    .replace(|c: char| !c.is_ascii_alphanumeric(), "-"),
                label.to_string(),
                utilization,
                quota.reset_time,
            )),
        }
    }
    // Busiest first: the first window is the one the tray shows.
    windows.sort_by(|a, b| b.utilization.total_cmp(&a.utilization));

    ProviderRateLimits {
        provider: "antigravity".to_string(),
        plan_tier: response.user_status.user_tier.name,
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

/// Hub files, newest first. Dead processes leave theirs behind.
fn hubs() -> Vec<Hub> {
    let Some(dir) = crate::paths::antigravity_daemon_dir() else {
        return Vec::new();
    };
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("hub_") && name.ends_with(".json")
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    files.sort_by_key(|file| std::cmp::Reverse(file.0));
    files
        .into_iter()
        .take(3)
        .filter_map(|(_, path)| serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok())
        .collect()
}

pub(super) async fn fetch_antigravity_rate_limits(
) -> Result<ProviderRateLimits, RateLimitFetchError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| RateLimitFetchError::message(format!("HTTP client error: {e}")))?;

    for hub in hubs() {
        let sent = client
            .post(format!(
                "http://127.0.0.1:{}/exa.language_server_pb.LanguageServerService/GetUserStatus",
                hub.http_port
            ))
            .header("Connect-Protocol-Version", "1")
            .header("X-Codeium-Csrf-Token", &hub.csrf_token)
            .json(&serde_json::json!({
                "metadata": {"ideName": "antigravity", "extensionName": "antigravity", "locale": "en"}
            }))
            .send()
            .await;
        // A refused connection or a rejected token is a stale hub file.
        let Ok(response) = sent else { continue };
        if !response.status().is_success() {
            continue;
        }
        return response
            .json::<StatusResponse>()
            .await
            .map(build_antigravity_rate_limits)
            .map_err(|e| {
                RateLimitFetchError::message(format!("Failed to parse Antigravity status: {e}"))
            });
    }
    Err(RateLimitFetchError::message(NOT_RUNNING))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_effort_variants_and_sorts_busiest_first() {
        let body = r#"{"userStatus":{
            "userTier":{"name":"Google AI Pro"},
            "cascadeModelConfigData":{"clientModelConfigs":[
                {"label":"Gemini 4 Hydrogen (High)","quotaInfo":{"remainingFraction":0.9,"resetTime":"2026-11-13T04:00:00Z"}},
                {"label":"Gemini 4 Hydrogen (Low)","quotaInfo":{"remainingFraction":0.9,"resetTime":"2026-11-13T04:00:00Z"}},
                {"label":"Claude Sonnet 5","quotaInfo":{"resetTime":"2026-11-13T05:00:00Z"}},
                {"label":"Unmetered","quotaInfo":{}}
            ]}}}"#;
        let limits = build_antigravity_rate_limits(serde_json::from_str(body).unwrap());

        assert_eq!(limits.plan_tier.as_deref(), Some("Google AI Pro"));
        let seen: Vec<_> = limits
            .windows
            .iter()
            .map(|w| {
                (
                    w.window_id.as_str(),
                    w.label.as_str(),
                    w.utilization.round(),
                )
            })
            .collect();
        assert_eq!(
            seen,
            vec![
                ("claude-sonnet-5", "Claude Sonnet 5", 100.0),
                ("gemini-4-hydrogen", "Gemini 4 Hydrogen", 10.0),
            ]
        );
    }
}
