use crate::models::{CreditsInfo, ExtraUsageInfo, ProviderRateLimits, RateLimitWindow};
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

/// Every amount may be null (it is while extra usage is off).
#[derive(Deserialize)]
pub(crate) struct ClaudeExtraUsageData {
    pub is_enabled: bool,
    pub monthly_limit: Option<f64>,
    pub used_credits: Option<f64>,
    pub utilization: Option<f64>,
    /// Minor-unit digits of the amounts; absent means cents.
    #[serde(default)]
    pub decimal_places: Option<i32>,
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

    // Model-scoped weekly limits moved from their own keys (`seven_day_fable`)
    // into the `limits` list; they keep the old id.
    for limit in obj
        .get("limits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|limit| limit.get("kind").and_then(Value::as_str) == Some("weekly_scoped"))
    {
        let Some(model) = limit
            .pointer("/scope/model/display_name")
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(percent) = limit.get("percent").and_then(as_f64) else {
            continue;
        };
        let id = format!("seven_day_{}", model.to_lowercase().replace(' ', "_"));
        if windows.iter().any(|w| w.window_id == id) {
            continue;
        }
        let resets_at = limit
            .get("resets_at")
            .and_then(Value::as_str)
            .map(str::to_string);
        windows.push(RateLimitWindow::new(
            id,
            format!("Weekly {model}"),
            percent,
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
    // The OAuth usage endpoint reports credit values in minor units (cents).
    let unit = 10f64.powi(extra_usage.decimal_places.unwrap_or(2));
    ExtraUsageInfo {
        is_enabled: extra_usage.is_enabled,
        monthly_limit: extra_usage.monthly_limit.unwrap_or(0.0) / unit,
        used_credits: extra_usage.used_credits.unwrap_or(0.0) / unit,
        utilization: extra_usage.utilization,
    }
}

/// The account's usage-credit balance ("Usage credits" on claude.ai), read
/// the way Claude Code reads it: its organization from `~/.claude.json`, then
/// `prepaid/credits`. `Ok(None)` when the account has no organization or the
/// endpoint reports no balance.
pub(super) async fn fetch_claude_usage_credits() -> Result<Option<CreditsInfo>, RateLimitFetchError>
{
    let Some(org) = claude_organization_uuid() else {
        return Ok(None);
    };
    let token = get_claude_oauth_token().map_err(RateLimitFetchError::message)?;
    let resp = reqwest::Client::builder()
        .timeout(super::HTTP_TIMEOUT)
        .build()
        .map_err(|e| RateLimitFetchError::message(format!("HTTP client build failed: {e}")))?
        .get(crate::ops::anthropic_prepaid_credits_url(&org))
        .bearer_auth(&token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("prepaid/credits failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(rate_limit_error_from_response(&resp));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("prepaid/credits unreadable: {e}")))?;
    Ok(usage_credits_from_prepaid(&body))
}

/// Claude Code keeps its last `/api/oauth/usage` response in `~/.claude.json`
/// (`cachedUsageUtilization`), with every window (Weekly Fable included, in
/// `limits`) and extra usage. The reading it holds, dated when Claude Code
/// fetched it; its age is the caller's to judge. Parsed again only when the
/// file changes.
pub(super) fn claude_code_cached_usage() -> Option<ProviderRateLimits> {
    type Memo = Option<((std::time::SystemTime, u64), Option<ProviderRateLimits>)>;
    static MEMO: Mutex<Memo> = Mutex::new(None);

    let path = crate::paths::claude_global_config_file()?;
    let meta = std::fs::metadata(&path).ok()?;
    let stamp = (meta.modified().ok()?, meta.len());
    let mut memo = MEMO.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((seen, reading)) = memo.as_ref() {
        if *seen == stamp {
            return reading.clone();
        }
    }
    let reading = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|config| reading_from_claude_config(&config));
    *memo = Some((stamp, reading.clone()));
    reading
}

fn reading_from_claude_config(config: &Value) -> Option<ProviderRateLimits> {
    let cached = config.get("cachedUsageUtilization")?;
    // The cache outlives a switch of account; only the signed-in one counts.
    let account = config.pointer("/oauthAccount/accountUuid")?;
    if cached.get("accountUuid") != Some(account) {
        return None;
    }
    let fetched = cached
        .get("fetchedAtMs")
        .and_then(Value::as_i64)
        .and_then(DateTime::<Utc>::from_timestamp_millis)?;
    let usage = cached.get("utilization")?;
    let windows = claude_usage_windows(usage);
    if windows.is_empty() {
        return None;
    }
    Some(ProviderRateLimits {
        provider: "claude".to_string(),
        plan_tier: None,
        windows,
        extra_usage: usage
            .get("extra_usage")
            .cloned()
            .and_then(|v| serde_json::from_value::<ClaudeExtraUsageData>(v).ok())
            .map(normalize_claude_extra_usage),
        credits: None,
        stale: false,
        error: None,
        retry_after_seconds: None,
        cooldown_until: None,
        fetched_at: fetched.with_timezone(&Local).to_rfc3339(),
    })
}

fn claude_organization_uuid() -> Option<String> {
    let path = crate::paths::claude_global_config_file()?;
    let config: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    config["oauthAccount"]["organizationUuid"]
        .as_str()
        .map(str::to_string)
}

/// `amount` is in cents, as Claude Code reads it.
// ponytail: `currency` is null on the accounts seen so far and taken as USD;
// convert here if a non-USD balance ever shows up.
fn usage_credits_from_prepaid(body: &Value) -> Option<CreditsInfo> {
    let cents = body.get("amount").and_then(as_f64)?;
    Some(CreditsInfo {
        balance: Some(cents / 100.0),
        has_credits: cents > 0.0,
        unlimited: false,
        usage_limit_resets: None,
    })
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

    let client = match reqwest::Client::builder()
        .timeout(super::HTTP_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            return FetchAttempt::Other(RateLimitFetchError::message(format!(
                "HTTP client build failed: {e}"
            )));
        }
    };

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

    /// Prints the raw OAuth usage payload, to see which fields the API has
    /// added (reset credits, extra usage) before parsing them:
    /// `cargo test --lib rate_limits::claude::tests::live_usage_payload_shape -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "requires local Claude credentials and network access"]
    async fn live_usage_payload_shape() {
        let token = get_claude_oauth_token().expect("no Claude OAuth token");
        let usage: Value = reqwest::Client::new()
            .get(crate::ops::anthropic_usage_url())
            .bearer_auth(&token)
            .header("anthropic-beta", "oauth-2025-04-20")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        println!("{}", serde_json::to_string_pretty(&usage).unwrap());

        let credits = fetch_claude_usage_credits().await.unwrap();
        println!("usage credits: {credits:?}");
        assert!(credits.is_some(), "no organization or no prepaid balance");
    }

    #[test]
    fn reads_the_usage_credit_balance_in_cents() {
        let credits = usage_credits_from_prepaid(
            &serde_json::json!({"amount": 1240, "currency": null, "balance": null}),
        )
        .unwrap();
        assert_eq!(credits.balance, Some(12.4));
        assert!(credits.has_credits);
        let empty = usage_credits_from_prepaid(&serde_json::json!({"amount": 0})).unwrap();
        assert!(!empty.has_credits);
        assert!(usage_credits_from_prepaid(&serde_json::json!({"amount": null})).is_none());
    }

    #[test]
    fn normalizes_claude_extra_usage_from_cents_to_usd() {
        let extra_usage = normalize_claude_extra_usage(ClaudeExtraUsageData {
            is_enabled: true,
            monthly_limit: Some(5000.0),
            used_credits: Some(710.0),
            utilization: Some(14.2),
            decimal_places: None,
        });

        assert!(extra_usage.is_enabled);
        assert_eq!(extra_usage.monthly_limit, 50.0);
        assert_eq!(extra_usage.used_credits, 7.1);
        assert_eq!(extra_usage.utilization, Some(14.2));
    }

    #[test]
    fn reads_extra_usage_whose_amounts_are_null() {
        // The live shape while extra usage is off: every amount null.
        let raw = serde_json::json!({
            "credits_ever_enabled": false, "currency": null, "daily": null,
            "decimal_places": null, "disabled_reason": null, "is_enabled": false,
            "monthly_limit": null, "spend_limit_reached": false, "used_credits": null,
            "user_disabled": false, "utilization": null, "weekly": null
        });
        let extra = normalize_claude_extra_usage(serde_json::from_value(raw).unwrap());
        assert!(!extra.is_enabled);
        assert_eq!(extra.monthly_limit, 0.0);

        let raw = serde_json::json!({
            "is_enabled": true, "monthly_limit": 50000, "used_credits": 1250,
            "utilization": 2.5, "decimal_places": 3
        });
        let extra = normalize_claude_extra_usage(serde_json::from_value(raw).unwrap());
        assert_eq!((extra.monthly_limit, extra.used_credits), (50.0, 1.25));
    }

    #[test]
    #[ignore = "reads this machine's ~/.claude.json"]
    fn live_reads_claude_codes_usage_cache() {
        let reading = claude_code_cached_usage().expect("no usable cachedUsageUtilization");
        for w in &reading.windows {
            println!(
                "{} {}% resets {:?}",
                w.window_id, w.utilization, w.resets_at
            );
        }
        println!("fetched_at {}", reading.fetched_at);
    }

    #[test]
    fn reads_claude_codes_cached_usage_for_the_signed_in_account() {
        let config = serde_json::json!({
            "oauthAccount": { "accountUuid": "acct-1" },
            "cachedUsageUtilization": {
                "fetchedAtMs": 1790390027551_i64,
                "accountUuid": "acct-1",
                "utilization": {
                    "five_hour": { "utilization": 3, "resets_at": "2026-09-26T07:20:00+00:00" },
                    "seven_day": { "utilization": 41, "resets_at": "2026-09-29T04:00:00+00:00" },
                    "seven_day_opus": null,
                    "extra_usage": { "is_enabled": false, "monthly_limit": null, "used_credits": null, "utilization": null },
                    "limits": [{ "kind": "weekly_scoped", "percent": 9,
                        "resets_at": "2026-09-29T04:00:00+00:00",
                        "scope": { "model": { "display_name": "Fable" } } }]
                }
            }
        });
        let reading = reading_from_claude_config(&config).unwrap();
        let ids: Vec<&str> = reading
            .windows
            .iter()
            .map(|w| w.window_id.as_str())
            .collect();
        assert_eq!(ids, ["five_hour", "seven_day", "seven_day_fable"]);
        assert!(!reading.extra_usage.unwrap().is_enabled);
        let fetched = DateTime::parse_from_rfc3339(&reading.fetched_at).unwrap();
        assert_eq!(fetched.timestamp_millis(), 1790390027551);

        let mut other = config.clone();
        other["oauthAccount"]["accountUuid"] = "acct-2".into();
        assert!(
            reading_from_claude_config(&other).is_none(),
            "another account's cache"
        );
        assert!(reading_from_claude_config(&serde_json::json!({})).is_none());
    }

    #[test]
    fn reads_model_weekly_windows_from_the_limits_list() {
        // Live shape: no `seven_day_fable` key, the Fable pool only in `limits`.
        let usage = serde_json::json!({
            "five_hour": { "utilization": 5.0, "resets_at": "2026-09-26T01:20:00Z" },
            "seven_day": { "utilization": 40.0, "resets_at": "2026-09-29T04:00:00Z" },
            "seven_day_opus": null,
            "limits": [
                { "kind": "session", "percent": 5, "resets_at": "2026-09-26T01:20:00Z", "scope": null },
                { "kind": "weekly_all", "percent": 40, "resets_at": "2026-09-29T04:00:00Z", "scope": null },
                { "kind": "weekly_scoped", "percent": 9, "resets_at": "2026-09-29T04:00:00Z",
                  "scope": { "model": { "display_name": "Fable", "id": null }, "surface": null } }
            ]
        });
        let windows = claude_usage_windows(&usage);
        let ids: Vec<&str> = windows.iter().map(|w| w.window_id.as_str()).collect();
        assert_eq!(ids, ["five_hour", "seven_day", "seven_day_fable"]);
        assert_eq!(windows[2].label, "Weekly Fable");
        assert_eq!(windows[2].utilization, 9.0);

        // An old-style key wins over the list entry for the same pool.
        let mut usage = usage;
        usage["seven_day_fable"] =
            serde_json::json!({ "utilization": 12.0, "resets_at": "2026-09-29T04:00:00Z" });
        let windows = claude_usage_windows(&usage);
        assert_eq!(
            windows
                .iter()
                .filter(|w| w.window_id == "seven_day_fable")
                .count(),
            1
        );
        assert_eq!(windows[2].utilization, 12.0);
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
