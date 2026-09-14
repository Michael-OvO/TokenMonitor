use crate::models::{ExtraUsageInfo, ProviderRateLimits, RateLimitWindow};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;

use super::http::rate_limit_error_from_response;
use super::{as_f64, humanize_snake_case, RateLimitFetchError};

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

fn read_token_from_credentials_path(cred_path: &Path) -> Result<String, String> {
    tracing::debug!(path = %cred_path.display(), "reading file (claude credentials)");
    let raw = std::fs::read_to_string(cred_path)
        .map_err(|e| format!("Failed to read {}: {e}", cred_path.display()))?;

    extract_access_token(&raw)
}

/// Read OAuth token from `~/.claude/.credentials.json`.
///
/// A plain file read in the Claude config directory the app already discloses
/// — no Keychain, no prompt. Present on Linux and Windows, and on Macs where
/// Claude Code was configured to keep credentials on disk.
fn read_token_from_credentials_file() -> Result<String, String> {
    let cred_path = crate::paths::claude_credentials_file()
        .ok_or_else(|| "Cannot determine Claude credentials file path".to_string())?;
    read_token_from_credentials_path(&cred_path)
}

/// Keychain item Claude Code stores its own OAuth credentials in.
#[cfg(target_os = "macos")]
const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// Read OAuth token from Claude Code's own login-Keychain item.
///
/// The default on macOS is the Keychain, not the file — so without this step
/// the OAuth fallback is dead on a stock Mac install.
///
/// The read goes through `/usr/bin/security` deliberately. Claude Code writes
/// the item with that same binary, so its ACL trusts it and the read is silent
/// for any process running as this user. Reading in-process through
/// Security.framework instead fails with errSecAuthFailed (-25293) against
/// that ACL, and the recovery path for that is the modal password panel.
/// See [`crate::platform::macos::keychain`].
#[cfg(target_os = "macos")]
fn read_token_from_keychain() -> Result<String, String> {
    use crate::platform::macos::keychain::find_generic_password;

    // Claude Code sets the account to the login name, but has not always; fall
    // back to a service-only lookup rather than missing an older item.
    let account = std::env::var("USER").ok();
    let raw = match account
        .as_deref()
        .map(|acct| find_generic_password(CLAUDE_KEYCHAIN_SERVICE, Some(acct)))
    {
        Some(Ok(raw)) => raw,
        _ => find_generic_password(CLAUDE_KEYCHAIN_SERVICE, None)?,
    };

    extract_access_token(&raw)
}

/// Get Claude Code OAuth access token (cross-platform).
///
/// Resolution order:
/// 1. `CLAUDE_CODE_OAUTH_TOKEN` environment variable (JSON string) — never cached
/// 2. In-process cache (set on previous successful read)
/// 3. `~/.claude/.credentials.json`
/// 4. macOS only: Claude Code's `Claude Code-credentials` Keychain item
///
/// On a successful read the token is stored in the in-process cache. Callers
/// that observe a 401 from the API drop that cache so the next call re-reads
/// the credentials instead of replaying the stale token.
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

    let file_error = match read_token_from_credentials_file() {
        Ok(token) => {
            store_access_token(&token);
            return Ok(token);
        }
        Err(error) => error,
    };

    #[cfg(target_os = "macos")]
    {
        match read_token_from_keychain() {
            Ok(token) => {
                store_access_token(&token);
                Ok(token)
            }
            // Both sources failed: report both, since "no credentials file" on
            // its own sends people looking for a file that is not supposed to
            // exist on a Keychain-backed install.
            Err(keychain_error) => Err(format!(
                "{file_error}; Keychain unavailable ({keychain_error})"
            )),
        }
    }

    #[cfg(not(target_os = "macos"))]
    Err(file_error)
}

// ── Claude API response types ──

/// Known Claude usage windows, in dashboard display order.
///
/// Anthropic's OAuth usage payload and Claude Code's statusline `rate_limits`
/// object share these keys. Missing keys are omitted; any additional object
/// with a numeric `utilization` / `used_percentage` becomes a new bar via
/// [`claude_usage_windows`] / statusline extraction.
const KNOWN_CLAUDE_WINDOWS: &[(&str, &str)] = &[
    // (apiField, label)
    ("five_hour", "Session (5hr)"),
    ("seven_day", "Weekly (7 day)"),
    ("seven_day_sonnet", "Weekly Sonnet"),
    ("seven_day_opus", "Weekly Opus"),
    ("seven_day_fable", "Weekly Fable"),
    ("seven_day_oauth_apps", "Weekly OAuth Apps"),
    ("seven_day_cowork", "Weekly Cowork"),
];

#[derive(Deserialize)]
pub(crate) struct ClaudeExtraUsageData {
    pub is_enabled: bool,
    pub monthly_limit: f64,
    pub used_credits: f64,
    pub utilization: Option<f64>,
}

/// Display label for a Claude window id. Known ids get Anthropic-aligned
/// names; unknown ids are humanized from the field name so new pools surface
/// without a TokenMonitor release for *structure* changes.
pub(super) fn claude_window_label(window_id: &str) -> String {
    KNOWN_CLAUDE_WINDOWS
        .iter()
        .find(|(id, _)| *id == window_id)
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| humanize_snake_case(window_id))
}

fn claude_window_from_value(value: &Value) -> Option<(f64, Option<String>)> {
    // OAuth usage uses `utilization`; statusline uses `used_percentage`.
    let utilization = value
        .get("utilization")
        .and_then(as_f64)
        .or_else(|| value.get("used_percentage").and_then(as_f64))?;
    let resets_at = value.get("resets_at").and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => n
            .as_i64()
            .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0))
            .map(|dt| dt.to_rfc3339()),
        _ => None,
    });
    Some((utilization, resets_at))
}

/// Build rate-limit windows from whatever meters Claude/Anthropic returns.
///
/// Known fields keep stable display names; unknown window objects become
/// additional bars so count changes track the API without a release.
pub(super) fn claude_usage_windows(usage: &Value) -> Vec<RateLimitWindow> {
    let Some(obj) = usage.as_object() else {
        return Vec::new();
    };

    let mut windows = Vec::new();
    let mut consumed = std::collections::HashSet::new();
    consumed.insert("extra_usage");

    for (api_field, _) in KNOWN_CLAUDE_WINDOWS {
        consumed.insert(*api_field);
        let Some(value) = obj.get(*api_field) else {
            continue;
        };
        let Some((utilization, resets_at)) = claude_window_from_value(value) else {
            continue;
        };
        windows.push(RateLimitWindow::new(
            (*api_field).to_string(),
            claude_window_label(api_field),
            utilization,
            resets_at,
        ));
    }

    let mut extras: Vec<(&String, f64, Option<String>)> = obj
        .iter()
        .filter(|(key, _)| !consumed.contains(key.as_str()))
        .filter_map(|(key, value)| {
            let (utilization, resets_at) = claude_window_from_value(value)?;
            // The payload carries placeholder pools under internal codenames
            // (`nimbus_quill`, `amber_ladder`, …). Most are `null` and drop
            // out above, but an unreleased one can arrive as a real object at
            // 0% with no reset time — which rendered as a bar named after the
            // codename. A live pool always says when it resets.
            resets_at.as_ref()?;
            Some((key, utilization, resets_at))
        })
        .collect();
    extras.sort_by(|a, b| a.0.cmp(b.0));

    for (field, utilization, resets_at) in extras {
        windows.push(RateLimitWindow::new(
            field.clone(),
            claude_window_label(field),
            utilization,
            resets_at,
        ));
    }

    windows
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
            // Access token is stale — Claude Code's stored token lives ~8h, so
            // any overnight gap in Claude Code usage lands here. Claude Code
            // refreshes it and rewrites `.credentials.json`, so dropping our
            // in-process cache and re-reading the file is the whole recovery.
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
        Err(err) => {
            tracing::debug!(reason = %err, "Claude OAuth: no token available");
            return FetchAttempt::Other(RateLimitFetchError::message(err));
        }
    };

    let client = reqwest::Client::new();

    // Fetch usage + account in parallel
    let usage_fut = client
        .get(crate::ops::anthropic_usage_url())
        .bearer_auth(&token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send();

    let account_fut = client
        .get(crate::ops::anthropic_account_url())
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
    let usage: Value = match usage_resp.json().await {
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

    let windows = claude_usage_windows(&usage);
    let extra_usage = usage
        .get("extra_usage")
        .cloned()
        .and_then(|v| serde_json::from_value::<ClaudeExtraUsageData>(v).ok())
        .map(normalize_claude_extra_usage);

    tracing::debug!(
        windows_count = windows.len(),
        plan_tier = ?plan_tier,
        has_extra_usage = extra_usage.is_some(),
        "Claude OAuth: API success"
    );

    FetchAttempt::Ok(ProviderRateLimits {
        provider: "claude".to_string(),
        plan_tier,
        windows,
        extra_usage,
        credits: None,
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
    use std::fs;
    use tempfile::TempDir;

    fn credentials_json(token: &str) -> String {
        format!(
            r#"{{
  "claudeAiOauth": {{
    "accessToken": "{token}",
    "refreshToken": "refresh-token",
    "expiresAt": 1777084603000,
    "scopes": ["org:create_api_key"],
    "subscriptionType": "max",
    "rateLimitTier": "claude_max"
  }}
}}"#
        )
    }

    #[test]
    fn reads_access_token_from_credentials_file_payload() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".credentials.json");
        fs::write(&path, credentials_json("file-access-token")).unwrap();

        let token = read_token_from_credentials_path(&path).unwrap();

        assert_eq!(token, "file-access-token");
    }

    #[tokio::test]
    #[ignore = "requires local Claude credentials and network access"]
    async fn live_fetches_full_claude_rate_limit_windows_from_credentials_file() {
        invalidate_access_token_cache();

        let rate_limits = fetch_claude_rate_limits().await.unwrap();
        let window_ids = rate_limits
            .windows
            .iter()
            .map(|window| window.window_id.as_str())
            .collect::<Vec<_>>();
        println!("Claude rate-limit windows: {window_ids:?}");

        assert!(window_ids.contains(&"five_hour"));
        assert!(window_ids.contains(&"seven_day"));
    }

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
    fn builds_windows_from_oauth_usage_payload() {
        let usage = serde_json::json!({
            "five_hour": { "utilization": 12.5, "resets_at": "2026-07-16T20:00:00Z" },
            "seven_day": { "utilization": 40.0, "resets_at": "2026-07-20T00:00:00Z" },
            "extra_usage": {
                "is_enabled": true,
                "monthly_limit": 5000.0,
                "used_credits": 100.0,
                "utilization": 2.0
            }
        });
        let windows = claude_usage_windows(&usage);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].window_id, "five_hour");
        assert_eq!(windows[0].label, "Session (5hr)");
        assert_eq!(windows[1].window_id, "seven_day");
        assert_eq!(windows[1].label, "Weekly (7 day)");
    }

    #[test]
    fn omits_missing_claude_meters_and_surfaces_unknown_ones() {
        let usage = serde_json::json!({
            "seven_day": { "utilization": 10.0 },
            "bonus_pool": { "utilization": 3.0, "resets_at": "2026-07-20T00:00:00Z" }
        });
        let windows = claude_usage_windows(&usage);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].window_id, "seven_day");
        assert_eq!(windows[1].window_id, "bonus_pool");
        assert_eq!(windows[1].label, "Bonus Pool");
        assert_eq!(windows[1].utilization, 3.0);
    }

    /// Anthropic ships unreleased pools under internal codenames. A `null`
    /// entry drops out on its own, but a live-looking one at 0% with no reset
    /// time used to render as a bar called "Nimbus Quill".
    #[test]
    fn drops_placeholder_pools_that_never_reset() {
        let usage = serde_json::json!({
            "five_hour": { "utilization": 4.0, "resets_at": "2026-08-09T06:19:59Z" },
            "seven_day": { "utilization": 56.0, "resets_at": "2026-08-11T03:59:59Z" },
            "seven_day_opus": null,
            "tangelo": null,
            "nimbus_quill": { "utilization": 0.0, "resets_at": null },
        });
        let ids: Vec<_> = claude_usage_windows(&usage)
            .into_iter()
            .map(|window| window.window_id)
            .collect();
        assert_eq!(ids, ["five_hour", "seven_day"]);
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

    /// This module's source with the test block cut off, so the scan below
    /// cannot match its own string literals.
    #[cfg(target_os = "macos")]
    fn production_source() -> &'static str {
        const MARKER: &str = "#[cfg(test)]\nmod tests {";
        let source = include_str!("claude.rs");
        source
            .find(MARKER)
            .map(|idx| &source[..idx])
            .expect("test module marker not found — did the module header change?")
    }

    /// Every Keychain touch here must go through `/usr/bin/security`.
    ///
    /// An in-process Security.framework call against Claude Code's item fails
    /// with errSecAuthFailed and, on a write, pops the modal password panel
    /// from a background thread. That shipped once; this keeps it from
    /// shipping again.
    #[cfg(target_os = "macos")]
    #[test]
    fn keychain_access_never_uses_security_framework_in_process() {
        for banned in [
            "security_framework",
            "SecKeychain",
            "ItemSearchOptions",
            "set_generic_password",
            "delete_generic_password",
        ] {
            let offending: Vec<_> = production_source()
                .lines()
                .enumerate()
                // Doc comments legitimately name these APIs when explaining
                // why they are avoided.
                .filter(|(_, line)| !line.trim_start().starts_with("//"))
                .filter(|(_, line)| line.contains(banned))
                .map(|(idx, line)| format!("line {}: {}", idx + 1, line.trim()))
                .collect();
            assert!(
                offending.is_empty(),
                "`{banned}` must not appear outside doc comments — route Keychain \
                 access through platform::macos::keychain instead:\n{}",
                offending.join("\n")
            );
        }
    }

    /// Live check that the OAuth fallback can actually resolve a token on a
    /// stock macOS install, where Claude Code keeps credentials in the
    /// Keychain and `~/.claude/.credentials.json` does not exist. Must not
    /// prompt for a password.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires a logged-in Claude Code on this machine"]
    fn live_reads_claude_code_credentials_from_the_keychain() {
        let token = read_token_from_keychain().expect("Keychain read failed");
        assert!(
            token.starts_with("sk-ant-"),
            "unexpected token shape from the Keychain"
        );
    }
}
