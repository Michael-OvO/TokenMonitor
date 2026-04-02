use crate::models::{ExtraUsageInfo, ProviderRateLimits, RateLimitWindow};
use chrono::Local;
use serde::Deserialize;

use super::http::rate_limit_error_from_response;
use super::RateLimitFetchError;

/// Extract `claudeAiOauth.accessToken` from a JSON string.
fn extract_access_token(json_str: &str) -> Result<String, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(json_str.trim()).map_err(|e| format!("Invalid JSON: {e}"))?;

    parsed
        .get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No claudeAiOauth.accessToken in credentials".to_string())
}

/// Read OAuth token from macOS Keychain.
#[cfg(target_os = "macos")]
fn read_token_from_keychain() -> Result<String, String> {
    use std::process::Command as StdCommand;

    let output = StdCommand::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .map_err(|e| format!("Failed to run security command: {e}"))?;

    if !output.status.success() {
        return Err("Claude Code credentials not found in Keychain".to_string());
    }

    let raw = String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 from Keychain: {e}"))?;

    extract_access_token(&raw)
}

/// Read OAuth token from `~/.claude/.credentials.json` (Windows/Linux).
#[cfg(not(target_os = "macos"))]
fn read_token_from_credentials_file() -> Result<String, String> {
    let config_dir = std::env::var("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .ok()
        .filter(|p| p.is_dir())
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))
        .ok_or_else(|| "Cannot determine Claude config directory".to_string())?;

    let cred_path = config_dir.join(".credentials.json");
    let raw = std::fs::read_to_string(&cred_path)
        .map_err(|e| format!("Failed to read {}: {e}", cred_path.display()))?;

    extract_access_token(&raw)
}

/// Get Claude Code OAuth access token (cross-platform).
///
/// Resolution order:
/// 1. `CLAUDE_CODE_OAUTH_TOKEN` environment variable (JSON string)
/// 2. macOS: Keychain; Windows/Linux: `~/.claude/.credentials.json`
pub(crate) fn get_claude_oauth_token() -> Result<String, String> {
    // Environment variable override (all platforms).
    if let Ok(env_json) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        if !env_json.trim().is_empty() {
            return extract_access_token(&env_json);
        }
    }

    #[cfg(target_os = "macos")]
    {
        read_token_from_keychain()
    }

    #[cfg(not(target_os = "macos"))]
    {
        read_token_from_credentials_file()
    }
}

// ── Claude API response types ──

#[derive(Deserialize)]
pub(crate) struct ClaudeUsageResponse {
    pub five_hour: Option<ClaudeWindowData>,
    pub seven_day: Option<ClaudeWindowData>,
    pub seven_day_sonnet: Option<ClaudeWindowData>,
    pub seven_day_opus: Option<ClaudeWindowData>,
    pub seven_day_oauth_apps: Option<ClaudeWindowData>,
    pub seven_day_cowork: Option<ClaudeWindowData>,
    pub iguana_necktie: Option<ClaudeWindowData>,
    pub extra_usage: Option<ClaudeExtraUsageData>,
}

#[derive(Deserialize)]
pub(crate) struct ClaudeWindowData {
    pub utilization: f64,
    pub resets_at: String,
}

#[derive(Deserialize)]
pub(crate) struct ClaudeExtraUsageData {
    pub is_enabled: bool,
    pub monthly_limit: f64,
    pub used_credits: f64,
    pub utilization: Option<f64>,
}

pub(crate) fn normalize_claude_extra_usage(extra_usage: ClaudeExtraUsageData) -> ExtraUsageInfo {
    ExtraUsageInfo {
        is_enabled: extra_usage.is_enabled,
        // The OAuth usage endpoint reports credit values in cents.
        monthly_limit: extra_usage.monthly_limit / 100.0,
        used_credits: extra_usage.used_credits / 100.0,
        utilization: extra_usage.utilization,
    }
}

#[derive(Deserialize)]
struct ClaudeAccountResponse {
    memberships: Vec<ClaudeMembership>,
}

#[derive(Deserialize)]
struct ClaudeMembership {
    organization: ClaudeOrganization,
}

#[derive(Deserialize)]
struct ClaudeOrganization {
    capabilities: Option<Vec<String>>,
    rate_limit_tier: Option<String>,
}

pub(super) async fn fetch_claude_rate_limits() -> Result<ProviderRateLimits, RateLimitFetchError> {
    let token = get_claude_oauth_token().map_err(RateLimitFetchError::message)?;

    let client = reqwest::Client::new();

    // Fetch usage + account in parallel
    let usage_fut = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .bearer_auth(&token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send();

    let account_fut = client
        .get("https://api.anthropic.com/api/oauth/account")
        .bearer_auth(&token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send();

    let (usage_res, account_res) = tokio::join!(usage_fut, account_fut);

    // Parse usage response
    let usage_resp = usage_res
        .map_err(|e| RateLimitFetchError::message(format!("Usage API request failed: {e}")))?;
    if !usage_resp.status().is_success() {
        return Err(rate_limit_error_from_response(&usage_resp));
    }
    let usage: ClaudeUsageResponse = usage_resp.json().await.map_err(|e| {
        RateLimitFetchError::message(format!("Failed to parse usage response: {e}"))
    })?;

    // Parse account response (non-fatal if it fails)
    let plan_tier = match account_res {
        Ok(resp) if resp.status().is_success() => resp
            .json::<ClaudeAccountResponse>()
            .await
            .ok()
            .and_then(|acct| detect_claude_plan(&acct)),
        _ => None,
    };

    // Build windows from non-null entries
    let mut windows = Vec::new();
    let window_specs: &[(&str, &str, &Option<ClaudeWindowData>)] = &[
        ("five_hour", "Session (5hr)", &usage.five_hour),
        ("seven_day", "Weekly (7 day)", &usage.seven_day),
        ("seven_day_sonnet", "Weekly Sonnet", &usage.seven_day_sonnet),
        ("seven_day_opus", "Weekly Opus", &usage.seven_day_opus),
        (
            "seven_day_oauth_apps",
            "Weekly OAuth Apps",
            &usage.seven_day_oauth_apps,
        ),
        ("seven_day_cowork", "Weekly Cowork", &usage.seven_day_cowork),
        ("iguana_necktie", "Iguana Necktie", &usage.iguana_necktie),
    ];

    for (id, label, data) in window_specs {
        if let Some(w) = data {
            windows.push(RateLimitWindow {
                window_id: id.to_string(),
                label: label.to_string(),
                utilization: w.utilization,
                resets_at: Some(w.resets_at.clone()),
            });
        }
    }

    let extra_usage = usage.extra_usage.map(normalize_claude_extra_usage);

    Ok(ProviderRateLimits {
        provider: "claude".to_string(),
        plan_tier,
        windows,
        extra_usage,
        stale: false,
        error: None,
        retry_after_seconds: None,
        cooldown_until: None,
        fetched_at: Local::now().to_rfc3339(),
    })
}

fn detect_claude_plan(acct: &ClaudeAccountResponse) -> Option<String> {
    for membership in &acct.memberships {
        if let Some(caps) = &membership.organization.capabilities {
            if caps.iter().any(|c| c == "claude_max") {
                // Use rate_limit_tier for more detail if available
                if let Some(tier) = &membership.organization.rate_limit_tier {
                    return Some(format_claude_plan_tier(tier));
                }
                return Some("Max".to_string());
            }
        }
    }
    // Fallback: check first membership with capabilities
    for membership in &acct.memberships {
        if let Some(caps) = &membership.organization.capabilities {
            if caps.contains(&"chat".to_string()) && !caps.contains(&"api".to_string()) {
                return Some("Pro".to_string());
            }
        }
    }
    None
}

fn format_claude_plan_tier(tier: &str) -> String {
    if tier.contains("claude_max_20x") {
        "Max 20x".to_string()
    } else if tier.contains("claude_max") {
        "Max 5x".to_string() // covers claude_max_5x and base Max plan ($100)
    } else if tier.contains("pro") {
        "Pro".to_string()
    } else {
        tier.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_claude_extra_usage_from_cents_to_usd() {
        let extra_usage = normalize_claude_extra_usage(ClaudeExtraUsageData {
            is_enabled: true,
            monthly_limit: 5000.0,
            used_credits: 710.0,
            utilization: Some(14.2),
        });

        assert!(extra_usage.is_enabled);
        assert_eq!(extra_usage.monthly_limit, 50.0);
        assert_eq!(extra_usage.used_credits, 7.1);
        assert_eq!(extra_usage.utilization, Some(14.2));
    }

    #[test]
    fn extract_access_token_from_valid_json() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-test","refreshToken":"rt"}}"#;
        assert_eq!(extract_access_token(json).unwrap(), "sk-ant-oat01-test");
    }

    #[test]
    fn extract_access_token_rejects_missing_field() {
        let json = r#"{"other": "data"}"#;
        assert!(extract_access_token(json).is_err());
    }

    #[test]
    fn extract_access_token_rejects_invalid_json() {
        assert!(extract_access_token("not json").is_err());
    }

    #[test]
    fn get_claude_oauth_token_reads_env_override() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-from-env"}}"#;
        // Temporarily set the env var for this test.
        std::env::set_var("CLAUDE_CODE_OAUTH_TOKEN", json);
        let result = get_claude_oauth_token();
        std::env::remove_var("CLAUDE_CODE_OAUTH_TOKEN");
        assert_eq!(result.unwrap(), "sk-from-env");
    }
}
