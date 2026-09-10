//! Kimi Code rate limits via `GET /coding/v1/usages`.
//!
//! The Kimi Code CLI stores an OAuth token set in `credentials/kimi-code.json`
//! (`access_token`, `refresh_token`, `expires_at`, …). Access tokens are
//! short-lived (15 minutes at the time of writing), so reading the file
//! verbatim only works while the CLI itself is running and refreshing it;
//! otherwise every probe returns 401. We therefore mirror the CLI: when the
//! stored token is expired (or the API rejects it) we perform a
//! `refresh_token` grant against Kimi's OAuth endpoint with the CLI's
//! published client id and write the rotated token set back to the same file
//! in the same shape, so the CLI and TokenMonitor share one token lineage.

use crate::models::{ProviderRateLimits, RateLimitWindow};
use chrono::{DateTime, Duration as ChronoDuration, Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::http::rate_limit_error_from_response;
use super::RateLimitFetchError;

const KIMI_USAGE_URL: &str = "https://api.kimi.com/coding/v1/usages";

/// Kimi's OAuth host. The CLI honors `KIMI_CODE_OAUTH_HOST` /
/// `KIMI_OAUTH_HOST` overrides, so we do too.
const DEFAULT_KIMI_OAUTH_HOST: &str = "https://auth.kimi.com";
const KIMI_OAUTH_TOKEN_PATH: &str = "/api/oauth/token";

/// Kimi Code's published OAuth client id — the same constant the open-source
/// `kimi-cli` ships, so our refresh is indistinguishable from the CLI's own.
const KIMI_CODE_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";

/// Hard cap on the refresh request so a hung auth server can't stall the
/// provider join in `fetch_selected_rate_limits`.
const REFRESH_TIMEOUT_SECS: u64 = 12;

/// Refresh this far ahead of `expires_at` so a token that is valid when we
/// read the file doesn't expire while the usage request is in flight.
const EXPIRY_SKEW_SECS: f64 = 60.0;

/// Once the refresh token itself is rejected the user has to sign in again
/// through the CLI; back off so we don't hit the auth server every cycle.
const REVOKED_COOLDOWN_SECS: i64 = 300;

/// Kimi Code CLI credential file shape (`credentials/kimi-code.json`).
///
/// Only the fields we act on are typed; everything else round-trips through
/// `extra` so a rewrite never drops keys the CLI relies on.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct KimiCredentials {
    access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    /// Unix epoch seconds; the CLI writes `time.time() + expires_in`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<f64>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl KimiCredentials {
    fn is_expired(&self, now_epoch_secs: f64) -> bool {
        match self.expires_at {
            Some(expires_at) => now_epoch_secs + EXPIRY_SKEW_SECS >= expires_at,
            // Unknown expiry (legacy file): try the API and let a 401 drive
            // the refresh instead of guessing.
            None => false,
        }
    }

    fn needs_refresh(&self, now_epoch_secs: f64) -> bool {
        self.access_token.is_empty() || self.is_expired(now_epoch_secs)
    }

    fn apply_refresh(&mut self, response: &KimiRefreshResponse, now_epoch_secs: f64) {
        self.access_token = response.access_token.clone();
        if let Some(refresh_token) = response
            .refresh_token
            .as_deref()
            .filter(|token| !token.is_empty())
        {
            self.refresh_token = Some(refresh_token.to_string());
        }
        if let Some(expires_in) = response.expires_in {
            self.expires_at = Some(now_epoch_secs + expires_in as f64);
            self.extra
                .insert("expires_in".to_string(), Value::from(expires_in));
        }
        if let Some(scope) = &response.scope {
            self.extra
                .insert("scope".to_string(), Value::from(scope.clone()));
        }
        if let Some(token_type) = &response.token_type {
            self.extra
                .insert("token_type".to_string(), Value::from(token_type.clone()));
        }
    }
}

fn parse_kimi_credentials(raw: &str) -> Result<KimiCredentials, serde_json::Error> {
    serde_json::from_str(raw)
}

fn now_epoch_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

fn load_kimi_credentials(path: &Path) -> Result<KimiCredentials, RateLimitFetchError> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        RateLimitFetchError::message(format!(
            "Failed to read Kimi credentials at {}: {e}",
            path.display()
        ))
    })?;

    parse_kimi_credentials(&raw).map_err(|e| {
        RateLimitFetchError::message(format!(
            "Failed to parse Kimi credentials at {}: {e}",
            path.display()
        ))
    })
}

fn read_kimi_credentials() -> Result<(PathBuf, KimiCredentials), RateLimitFetchError> {
    let path = crate::paths::kimi_credentials_file().ok_or_else(|| {
        RateLimitFetchError::message("Kimi Code CLI is not signed in on this machine")
    })?;

    let parsed = load_kimi_credentials(&path)?;

    if parsed.access_token.is_empty() && parsed.refresh_token.is_none() {
        return Err(RateLimitFetchError::message(
            "Kimi credentials file contains an empty access token",
        ));
    }

    Ok((path, parsed))
}

/// Atomically replace the credentials file: write a sibling temp file with
/// owner-only permissions, fsync, then rename over the original so the CLI
/// never observes a half-written token set.
fn write_kimi_credentials(path: &Path, credentials: &KimiCredentials) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(credentials).map_err(std::io::Error::other)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("kimi-code.json");
    let tmp = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let result = (|| {
        let mut file = options.open(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Response of `POST /api/oauth/token` (`grant_type=refresh_token`).
#[derive(Debug, Deserialize)]
struct KimiRefreshResponse {
    access_token: String,
    /// Kimi rotates the refresh token; persist it when present, otherwise
    /// keep the one we already hold.
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
}

/// Outcome of a refresh attempt. "Definitely revoked" is split from
/// "transient" so the caller can surface a sign-in-again message (with a
/// cooldown) for the former and simply retry next cycle for the latter.
#[derive(Debug)]
enum RefreshOutcome {
    Refreshed(KimiRefreshResponse),
    Revoked(String),
    Transient(String),
}

fn kimi_oauth_token_url_for(host_override: Option<&str>) -> String {
    let host = host_override
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(|host| host.trim_end_matches('/'))
        .unwrap_or(DEFAULT_KIMI_OAUTH_HOST);
    format!("{host}{KIMI_OAUTH_TOKEN_PATH}")
}

fn kimi_oauth_token_url() -> String {
    let host = std::env::var("KIMI_CODE_OAUTH_HOST")
        .or_else(|_| std::env::var("KIMI_OAUTH_HOST"))
        .ok();
    kimi_oauth_token_url_for(host.as_deref())
}

fn truncate_for_log(body: String) -> String {
    body.chars().take(200).collect()
}

async fn refresh_kimi_access_token(refresh_token: &str) -> RefreshOutcome {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(REFRESH_TIMEOUT_SECS))
        .build()
    {
        Ok(client) => client,
        Err(e) => return RefreshOutcome::Transient(format!("client build: {e}")),
    };

    let response = match client
        .post(kimi_oauth_token_url())
        .header("Accept", "application/json")
        .form(&[
            ("client_id", KIMI_CODE_CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
    {
        Ok(response) => response,
        Err(e) => return RefreshOutcome::Transient(format!("network: {e}")),
    };

    let status = response.status();
    if status.is_success() {
        return match response.json::<KimiRefreshResponse>().await {
            Ok(parsed) if !parsed.access_token.is_empty() => RefreshOutcome::Refreshed(parsed),
            Ok(_) => RefreshOutcome::Transient(
                "token endpoint returned an empty access token".to_string(),
            ),
            Err(e) => RefreshOutcome::Transient(format!("response parse: {e}")),
        };
    }

    let body = truncate_for_log(response.text().await.unwrap_or_default());
    if status == reqwest::StatusCode::BAD_REQUEST || status == reqwest::StatusCode::UNAUTHORIZED {
        // `invalid_grant` (400) is the OAuth2 standard for "refresh token no
        // longer valid"; 401 on the token endpoint means the same thing.
        RefreshOutcome::Revoked(format!("status={status} body={body}"))
    } else {
        RefreshOutcome::Transient(format!("status={status} body={body}"))
    }
}

fn sign_in_expired_error(detail: &str) -> RateLimitFetchError {
    let cooldown_until = Utc::now() + ChronoDuration::seconds(REVOKED_COOLDOWN_SECS);
    RateLimitFetchError {
        message: format!(
            "Kimi Code CLI sign-in has expired on this machine; run `kimi` and log in again ({detail})"
        ),
        retry_after_seconds: Some(REVOKED_COOLDOWN_SECS as u64),
        cooldown_until: Some(cooldown_until.to_rfc3339()),
    }
}

/// Exchange the stored refresh token for a fresh token set and persist it.
///
/// Kimi rotates refresh tokens, so a successful grant that is *not* written
/// back would strand the CLI with a dead refresh token — persisting is not
/// optional. If the CLI rotated the file while our request was in flight we
/// keep its newer lineage instead of clobbering it with ours.
async fn refresh_and_persist(
    path: &Path,
    credentials: KimiCredentials,
) -> Result<KimiCredentials, RateLimitFetchError> {
    let Some(refresh_token) = credentials
        .refresh_token
        .clone()
        .filter(|token| !token.is_empty())
    else {
        return Err(sign_in_expired_error(
            "credentials file has no refresh token",
        ));
    };

    match refresh_kimi_access_token(&refresh_token).await {
        RefreshOutcome::Refreshed(response) => {
            if let Ok(on_disk) = load_kimi_credentials(path) {
                if let Some(disk_refresh_token) = on_disk.refresh_token.as_deref() {
                    if disk_refresh_token != refresh_token && !on_disk.access_token.is_empty() {
                        tracing::info!(
                            "Kimi credentials rotated on disk during refresh; using the CLI's newer token set"
                        );
                        return Ok(on_disk);
                    }
                }
            }

            let mut updated = credentials;
            updated.apply_refresh(&response, now_epoch_secs());
            match write_kimi_credentials(path, &updated) {
                Ok(()) => tracing::info!(
                    path = %path.display(),
                    "Refreshed Kimi access token and persisted the rotated token set"
                ),
                Err(error) => tracing::warn!(
                    error = %error,
                    path = %path.display(),
                    "Refreshed Kimi access token but failed to persist it"
                ),
            }
            Ok(updated)
        }
        RefreshOutcome::Revoked(detail) => {
            tracing::warn!(detail = %detail, "Kimi refresh token was rejected");
            Err(sign_in_expired_error(&detail))
        }
        RefreshOutcome::Transient(detail) => Err(RateLimitFetchError::message(format!(
            "Kimi token refresh failed: {detail}"
        ))),
    }
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

async fn request_kimi_usage(access_token: &str) -> Result<reqwest::Response, RateLimitFetchError> {
    reqwest::Client::new()
        .get(KIMI_USAGE_URL)
        .header("Accept", "application/json")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Kimi usage API request failed: {e}")))
}

async fn parse_kimi_usage_response(
    response: reqwest::Response,
) -> Result<ProviderRateLimits, RateLimitFetchError> {
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

pub(super) async fn fetch_kimi_rate_limits() -> Result<ProviderRateLimits, RateLimitFetchError> {
    let (path, mut credentials) = read_kimi_credentials()?;
    let mut refreshed = false;

    if credentials.needs_refresh(now_epoch_secs()) {
        tracing::debug!("Kimi access token is expired; refreshing before the usage request");
        credentials = refresh_and_persist(&path, credentials).await?;
        refreshed = true;
    }

    let mut response = request_kimi_usage(&credentials.access_token).await?;

    // `expires_at` can lie (clock skew, server-side revocation): a 401 on a
    // token we believed valid gets exactly one refresh-and-retry.
    if response.status() == reqwest::StatusCode::UNAUTHORIZED && !refreshed {
        tracing::info!(
            "Kimi usage API rejected the stored access token; attempting a refresh-token grant"
        );
        credentials = refresh_and_persist(&path, credentials).await?;
        response = request_kimi_usage(&credentials.access_token).await?;
    }

    parse_kimi_usage_response(response).await
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
    fn creds(raw: &str) -> KimiCredentials {
        parse_kimi_credentials(raw).expect("credentials should parse")
    }

    #[test]
    fn parses_credentials_with_refresh_fields_and_preserves_unknown_keys() {
        let c = creds(
            r#"{"access_token":"at","refresh_token":"rt","expires_at":1788448361,"expires_in":900,"scope":"kimi-code","token_type":"Bearer"}"#,
        );
        assert_eq!(c.access_token, "at");
        assert_eq!(c.refresh_token.as_deref(), Some("rt"));
        assert_eq!(c.expires_at, Some(1788448361.0));
        assert_eq!(
            c.extra.get("scope").and_then(Value::as_str),
            Some("kimi-code")
        );
        assert_eq!(c.extra.get("expires_in").and_then(Value::as_u64), Some(900));
    }

    #[test]
    fn parses_legacy_credentials_without_refresh_fields() {
        let c = creds(r#"{"access_token":"at"}"#);
        assert!(c.refresh_token.is_none());
        assert!(c.expires_at.is_none());
        // Unknown expiry: try the API as-is and let a 401 drive the refresh.
        assert!(!c.is_expired(1.0e12));
    }

    #[test]
    fn detects_expired_and_about_to_expire_tokens() {
        let c = creds(r#"{"access_token":"at","expires_at":1000.0}"#);
        assert!(c.is_expired(1000.0));
        assert!(c.is_expired(1000.0 + 5000.0));
        assert!(c.is_expired(1000.0 - EXPIRY_SKEW_SECS));
        assert!(!c.is_expired(1000.0 - EXPIRY_SKEW_SECS - 1.0));
    }

    #[test]
    fn apply_refresh_rotates_tokens_and_recomputes_expiry() {
        let mut c = creds(
            r#"{"access_token":"old","refresh_token":"old-rt","expires_at":10,"expires_in":900,"scope":"kimi-code","token_type":"Bearer","device_id":"abc"}"#,
        );
        c.apply_refresh(
            &KimiRefreshResponse {
                access_token: "new".to_string(),
                refresh_token: Some("new-rt".to_string()),
                expires_in: Some(600),
                scope: None,
                token_type: None,
            },
            5000.0,
        );
        assert_eq!(c.access_token, "new");
        assert_eq!(c.refresh_token.as_deref(), Some("new-rt"));
        assert_eq!(c.expires_at, Some(5600.0));
        assert_eq!(c.extra.get("expires_in").and_then(Value::as_u64), Some(600));
        // Fields the CLI wrote that we don't understand must survive a rewrite.
        assert_eq!(
            c.extra.get("device_id").and_then(Value::as_str),
            Some("abc")
        );
        assert_eq!(
            c.extra.get("scope").and_then(Value::as_str),
            Some("kimi-code")
        );
        assert_eq!(
            c.extra.get("token_type").and_then(Value::as_str),
            Some("Bearer")
        );
    }

    #[test]
    fn apply_refresh_keeps_refresh_token_when_server_omits_it() {
        let mut c = creds(r#"{"access_token":"old","refresh_token":"keep-me","expires_at":10}"#);
        c.apply_refresh(
            &KimiRefreshResponse {
                access_token: "new".to_string(),
                refresh_token: None,
                expires_in: None,
                scope: None,
                token_type: None,
            },
            5000.0,
        );
        assert_eq!(c.access_token, "new");
        assert_eq!(c.refresh_token.as_deref(), Some("keep-me"));
        // No expires_in in the response: leave the stored expiry alone rather
        // than inventing one.
        assert_eq!(c.expires_at, Some(10.0));
    }

    #[test]
    fn writes_credentials_atomically_with_owner_only_permissions() {
        let dir = std::env::temp_dir().join(format!(
            "tokenmonitor-kimi-creds-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kimi-code.json");
        std::fs::write(&path, "{\"access_token\":\"stale\"}").unwrap();

        let c = creds(
            r#"{"access_token":"fresh","refresh_token":"rt","expires_at":123.0,"scope":"kimi-code"}"#,
        );
        write_kimi_credentials(&path, &c).unwrap();

        let round = creds(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(round.access_token, "fresh");
        assert_eq!(round.refresh_token.as_deref(), Some("rt"));
        assert_eq!(round.expires_at, Some(123.0));
        assert_eq!(
            round.extra.get("scope").and_then(Value::as_str),
            Some("kimi-code")
        );

        // No temp file left behind.
        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(
            entries.len(),
            1,
            "expected only the credentials file, found {entries:?}"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sign_in_expired_error_carries_cooldown() {
        let err = sign_in_expired_error("invalid_grant");
        assert!(
            err.message.contains("sign-in has expired"),
            "{}",
            err.message
        );
        assert_eq!(err.retry_after_seconds, Some(REVOKED_COOLDOWN_SECS as u64));
        assert!(err.cooldown_until.is_some());
    }

    #[test]
    fn oauth_token_url_honors_host_override() {
        assert_eq!(
            kimi_oauth_token_url_for(None),
            "https://auth.kimi.com/api/oauth/token"
        );
        assert_eq!(
            kimi_oauth_token_url_for(Some("https://example.test/")),
            "https://example.test/api/oauth/token"
        );
        assert_eq!(
            kimi_oauth_token_url_for(Some("   ")),
            "https://auth.kimi.com/api/oauth/token"
        );
    }
    /// Live probe against the real credentials file and Kimi's API — exercises
    /// the refresh-and-persist path end to end. Ignored by default; run with
    /// `cargo test --lib rate_limits::kimi::tests::live_fetch -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_fetch_with_refresh() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        match runtime.block_on(fetch_kimi_rate_limits()) {
            Ok(limits) => {
                let summary: Vec<_> = limits
                    .windows
                    .iter()
                    .map(|w| (w.window_id.as_str(), w.utilization, w.resets_at.clone()))
                    .collect();
                println!("live kimi fetch ok: {summary:?}");
            }
            Err(err) => panic!(
                "live kimi fetch failed: {} (retry_after={:?}, cooldown_until={:?})",
                err.message, err.retry_after_seconds, err.cooldown_until
            ),
        }
    }
}
