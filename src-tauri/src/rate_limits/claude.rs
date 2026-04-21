use crate::models::{ExtraUsageInfo, ProviderRateLimits, RateLimitWindow};
use chrono::Local;
use serde::Deserialize;
use std::sync::Mutex;

use super::http::rate_limit_error_from_response;
use super::RateLimitFetchError;

/// In-process cache of the Claude OAuth access token.
///
/// Claude Code rewrites the `Claude Code-credentials` Keychain item each time
/// it rotates its OAuth token. That rewrite resets the item's ACL / partition
/// list, so the user's "Always Allow" grant for TokenMonitor is lost — and
/// without a cache the next background refresh (every ~2.5 min) re-prompts.
/// Caching lets us reuse the token across refresh cycles and only touch the
/// Keychain on a cold cache or when the API returns 401 (real rotation).
static CACHED_ACCESS_TOKEN: Mutex<Option<String>> = Mutex::new(None);

fn cached_access_token() -> Option<String> {
    CACHED_ACCESS_TOKEN.lock().ok().and_then(|g| g.clone())
}

fn store_access_token(token: &str) {
    if let Ok(mut guard) = CACHED_ACCESS_TOKEN.lock() {
        *guard = Some(token.to_string());
    }
}

fn invalidate_access_token_cache() {
    if let Ok(mut guard) = CACHED_ACCESS_TOKEN.lock() {
        *guard = None;
    }
}

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

/// Read OAuth token from macOS Keychain via Security.framework.
///
/// `skip_authenticated_items(true)` sets `kSecUseAuthenticationUI =
/// kSecUseAuthenticationUISkip`, so `SecItemCopyMatching` silently returns
/// `errSecItemNotFound` instead of putting up a Keychain prompt when the item
/// would require one. This is the only way to guarantee zero recurring prompts
/// — Claude Code rewrites the credentials item on every OAuth rotation and
/// resets its ACL / partition list, so the user's "Always Allow" grant for
/// TokenMonitor is dropped along with the old item, and any future read would
/// otherwise re-prompt.
///
/// When access is silently denied the caller falls through to the CLI probe
/// in `rate_limits/mod.rs`, which goes through the Claude Code binary itself
/// (already trusted for its own item) and so doesn't hit our prompt path.
///
/// Searches by service only (equivalent to `security -s "…" -w`), so we don't
/// need to guess what account name Claude Code used when writing the item.
#[cfg(target_os = "macos")]
fn read_token_from_keychain() -> Result<String, String> {
    use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};

    let results = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service("Claude Code-credentials")
        .load_data(true)
        .limit(1)
        .skip_authenticated_items(true)
        .search()
        .map_err(|e| format!("Claude Code credentials not available in Keychain: {e}"))?;

    let data = results
        .into_iter()
        .find_map(|r| match r {
            SearchResult::Data(bytes) => Some(bytes),
            _ => None,
        })
        .ok_or_else(|| "Keychain returned no data for Claude Code-credentials".to_string())?;

    let raw = String::from_utf8(data).map_err(|e| format!("Invalid UTF-8 from Keychain: {e}"))?;
    extract_access_token(&raw)
}

/// Interactive Keychain read used by the one-time setup flow.
///
/// Unlike [`read_token_from_keychain`], this deliberately does **not** set
/// `skip_authenticated_items(true)`, so macOS will show the user-auth prompt
/// when needed. This is the only path in the app that allows that prompt to
/// appear — it's invoked from the explicit "Allow Keychain access" button in
/// the welcome flow, never from background refreshes. On success the token is
/// stored in the in-process cache so the very next API call succeeds without
/// re-reading the Keychain.
#[cfg(target_os = "macos")]
pub(super) fn prime_token_from_keychain_interactive() -> Result<(), String> {
    use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};

    let results = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service("Claude Code-credentials")
        .load_data(true)
        .limit(1)
        .search()
        .map_err(|e| format!("Keychain access denied or unavailable: {e}"))?;

    let data = results
        .into_iter()
        .find_map(|r| match r {
            SearchResult::Data(bytes) => Some(bytes),
            _ => None,
        })
        .ok_or_else(|| "Claude Code credentials not found in Keychain".to_string())?;

    let raw = String::from_utf8(data).map_err(|e| format!("Invalid UTF-8 from Keychain: {e}"))?;
    let token = extract_access_token(&raw)?;
    store_access_token(&token);
    Ok(())
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
/// 1. `CLAUDE_CODE_OAUTH_TOKEN` environment variable (JSON string) — never cached
/// 2. In-process cache (set on previous successful read)
/// 3. macOS: Keychain; Windows/Linux: `~/.claude/.credentials.json`
///
/// The result of (3) is stored in the cache. Callers that observe a 401 from
/// the API should call [`invalidate_access_token_cache`] before retrying so
/// the next call re-reads from the source.
pub(crate) fn get_claude_oauth_token() -> Result<String, String> {
    // Environment variable override (all platforms). Cheap to read each call,
    // and we don't want to cache an env value that the user might change.
    if let Ok(env_json) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        if !env_json.trim().is_empty() {
            return extract_access_token(&env_json);
        }
    }

    if let Some(cached) = cached_access_token() {
        return Ok(cached);
    }

    #[cfg(target_os = "macos")]
    let token = read_token_from_keychain()?;

    #[cfg(not(target_os = "macos"))]
    let token = read_token_from_credentials_file()?;

    store_access_token(&token);
    Ok(token)
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
    pub resets_at: Option<String>,
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

/// Outcome of one API attempt. We surface 401 separately so the outer
/// function can drop the cached token and retry with a fresh read.
enum FetchAttempt {
    Ok(ProviderRateLimits),
    Unauthorized(RateLimitFetchError),
    Other(RateLimitFetchError),
}

pub(super) async fn fetch_claude_rate_limits() -> Result<ProviderRateLimits, RateLimitFetchError> {
    match try_fetch_claude_rate_limits().await {
        FetchAttempt::Ok(rate_limits) => Ok(rate_limits),
        FetchAttempt::Other(err) => Err(err),
        FetchAttempt::Unauthorized(_) => {
            // The cached token (if any) is stale — Claude Code rotated it.
            // Drop it and retry once with a fresh source read. If the second
            // attempt is also unauthorized we surface that to the caller, who
            // falls through to the CLI probe in `rate_limits/mod.rs`.
            invalidate_access_token_cache();
            match try_fetch_claude_rate_limits().await {
                FetchAttempt::Ok(rate_limits) => Ok(rate_limits),
                FetchAttempt::Unauthorized(err) | FetchAttempt::Other(err) => Err(err),
            }
        }
    }
}

async fn try_fetch_claude_rate_limits() -> FetchAttempt {
    let token = match get_claude_oauth_token() {
        Ok(token) => token,
        Err(err) => return FetchAttempt::Other(RateLimitFetchError::message(err)),
    };

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
    let usage_resp = match usage_res {
        Ok(r) => r,
        Err(e) => {
            return FetchAttempt::Other(RateLimitFetchError::message(format!(
                "Usage API request failed: {e}"
            )));
        }
    };
    if !usage_resp.status().is_success() {
        let err = rate_limit_error_from_response(&usage_resp);
        return if usage_resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            FetchAttempt::Unauthorized(err)
        } else {
            FetchAttempt::Other(err)
        };
    }
    let usage: ClaudeUsageResponse = match usage_resp.json().await {
        Ok(u) => u,
        Err(e) => {
            return FetchAttempt::Other(RateLimitFetchError::message(format!(
                "Failed to parse usage response: {e}"
            )));
        }
    };

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
            windows.push(RateLimitWindow::new(
                id.to_string(),
                label.to_string(),
                w.utilization,
                w.resets_at.clone(),
            ));
        }
    }

    let extra_usage = usage.extra_usage.map(normalize_claude_extra_usage);

    FetchAttempt::Ok(ProviderRateLimits {
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

    /// Serializes tests that touch the module-level token cache or the
    /// `CLAUDE_CODE_OAUTH_TOKEN` env var, both of which are global state.
    static SHARED_STATE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn get_claude_oauth_token_reads_env_override() {
        let _guard = SHARED_STATE_LOCK.lock().unwrap();
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-from-env"}}"#;
        // SAFETY: serialized via SHARED_STATE_LOCK so no other test reads or
        // writes the same env var concurrently.
        unsafe {
            std::env::set_var("CLAUDE_CODE_OAUTH_TOKEN", json);
        }
        let result = get_claude_oauth_token();
        unsafe {
            std::env::remove_var("CLAUDE_CODE_OAUTH_TOKEN");
        }
        assert_eq!(result.unwrap(), "sk-from-env");
    }

    #[test]
    fn access_token_cache_stores_and_invalidates() {
        let _guard = SHARED_STATE_LOCK.lock().unwrap();
        invalidate_access_token_cache();
        assert!(cached_access_token().is_none());

        store_access_token("sk-cached");
        assert_eq!(cached_access_token().as_deref(), Some("sk-cached"));

        invalidate_access_token_cache();
        assert!(cached_access_token().is_none());
    }

    #[test]
    fn get_claude_oauth_token_returns_cached_value_without_keychain() {
        let _guard = SHARED_STATE_LOCK.lock().unwrap();
        // Make sure the env var is not set so we exercise the cache branch.
        // SAFETY: serialized via SHARED_STATE_LOCK.
        unsafe {
            std::env::remove_var("CLAUDE_CODE_OAUTH_TOKEN");
        }
        store_access_token("sk-from-cache");
        let result = get_claude_oauth_token();
        invalidate_access_token_cache();
        assert_eq!(result.unwrap(), "sk-from-cache");
    }
}
