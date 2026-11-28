//! Gemini CLI quotas via the Code Assist API, the one `/stats` in the CLI
//! reads.
//!
//! The CLI keeps its Google OAuth tokens in `~/.gemini/oauth_creds.json`
//! (`access_token`, `refresh_token`, `expiry_date` in epoch millis). Access
//! tokens last an hour and the CLI only renews them while it runs, so an
//! expired one is renewed here with the CLI's own installed-app client, read
//! from the installed CLI. The
//! new token is kept in memory only: Google refresh tokens do not rotate, so
//! the CLI's file stays valid untouched.
//!
//! `loadCodeAssist` gives the account's project id and tier;
//! `retrieveUserQuota` then lists a bucket per model with the fraction of
//! quota left and its reset time.
//!
//! ponytail: API-key and Vertex sign-ins have no quota endpoint and read as
//! not signed in. Tokens the CLI stores in the OS keychain are not read.

use crate::models::{ProviderRateLimits, RateLimitWindow};
use chrono::{Local, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Mutex;

use super::http::rate_limit_error_from_response;
use super::RateLimitFetchError;

const CODE_ASSIST_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// The installed CLI's OAuth client `(id, secret)`, read once found.
static CLI_CLIENT: Mutex<Option<(String, String)>> = Mutex::new(None);

/// The string a JS `name = "…"` (or `'…'`) assigns.
fn js_const(source: &str, name: &str) -> Option<String> {
    let pattern = format!("{name} = ");
    let rest = &source[source.find(&pattern)? + pattern.len()..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &rest[1..];
    Some(rest[..rest.find(quote)?].to_string())
}

/// The Gemini CLI's installed-application OAuth client, read from the CLI
/// installed here (npm package `@google/gemini-cli`) rather than shipped.
///
/// ponytail: finds npm/Homebrew installs through `gemini` on PATH; other
/// layouts read as an expired sign-in until the CLI renews its own token.
fn read_cli_client() -> Option<(String, String)> {
    let shim = super::command_in_path("gemini")?;
    let shim = std::fs::canonicalize(&shim).unwrap_or(shim);
    let root = shim.ancestors().find_map(|dir| {
        [
            dir.to_path_buf(),
            dir.join("node_modules/@google/gemini-cli"),
        ]
        .into_iter()
        .find(|p| p.ends_with("@google/gemini-cli") && p.is_dir())
    })?;
    // Bundled releases, then the older unbundled core, nested or hoisted.
    let oauth2 = "dist/src/code_assist/oauth2.js";
    let mut files: Vec<_> = std::fs::read_dir(root.join("bundle"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "js"))
        .collect();
    files.push(
        root.join("node_modules/@google/gemini-cli-core")
            .join(oauth2),
    );
    files.push(root.with_file_name("gemini-cli-core").join(oauth2));
    files.into_iter().find_map(|path| {
        let source = std::fs::read_to_string(path).ok()?;
        Some((
            js_const(&source, "OAUTH_CLIENT_ID")?,
            js_const(&source, "OAUTH_CLIENT_SECRET")?,
        ))
    })
}

async fn cli_client() -> Result<(String, String), RateLimitFetchError> {
    if let Some(client) = CLI_CLIENT.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Ok(client);
    }
    let found = tokio::task::spawn_blocking(read_cli_client)
        .await
        .ok()
        .flatten()
        .ok_or_else(|| RateLimitFetchError::message(SIGN_IN_EXPIRED))?;
    *CLI_CLIENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(found.clone());
    Ok(found)
}

/// Renew this far ahead of `expiry_date`.
const EXPIRY_SKEW_MS: i64 = 60_000;

/// Not a failure: the UI shows its idle summary for it.
pub(super) const NOT_SIGNED_IN: &str = "Gemini CLI is not signed in on this machine";
const SIGN_IN_EXPIRED: &str = "Gemini CLI sign-in has expired; run `gemini` to sign in again";

#[derive(Deserialize, Default)]
#[serde(default)]
struct Credentials {
    access_token: String,
    refresh_token: Option<String>,
    expiry_date: Option<i64>,
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
    expires_in: Option<i64>,
}

/// The access token renewed here and its expiry (epoch millis).
static RENEWED: Mutex<Option<(String, i64)>> = Mutex::new(None);

fn read_credentials() -> Result<Credentials, RateLimitFetchError> {
    let path = crate::paths::gemini_oauth_creds_file()
        .ok_or_else(|| RateLimitFetchError::message(NOT_SIGNED_IN))?;
    let raw =
        std::fs::read_to_string(&path).map_err(|_| RateLimitFetchError::message(NOT_SIGNED_IN))?;
    serde_json::from_str(&raw).map_err(|e| {
        RateLimitFetchError::message(format!(
            "Failed to parse Gemini credentials at {}: {e}",
            path.display()
        ))
    })
}

async fn refresh(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<String, RateLimitFetchError> {
    let (client_id, client_secret) = cli_client().await?;
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &client_id),
            ("client_secret", &client_secret),
        ])
        .send()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Gemini token refresh failed: {e}")))?;
    let status = response.status();
    if status == reqwest::StatusCode::BAD_REQUEST || status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(RateLimitFetchError::message(SIGN_IN_EXPIRED));
    }
    if !status.is_success() {
        return Err(rate_limit_error_from_response(&response));
    }
    let renewed: RefreshResponse = response
        .json()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Gemini token refresh failed: {e}")))?;
    let expires_at = Utc::now().timestamp_millis() + renewed.expires_in.unwrap_or(3600) * 1000;
    *RENEWED.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((renewed.access_token.clone(), expires_at));
    Ok(renewed.access_token)
}

async fn access_token(client: &reqwest::Client) -> Result<String, RateLimitFetchError> {
    let live = |expires_at: i64| expires_at - EXPIRY_SKEW_MS > Utc::now().timestamp_millis();
    let credentials = read_credentials()?;
    if !credentials.access_token.is_empty() && credentials.expiry_date.is_some_and(live) {
        return Ok(credentials.access_token);
    }
    if let Some((token, _)) = RENEWED
        .lock()
        .unwrap()
        .clone()
        .filter(|(_, expires_at)| live(*expires_at))
    {
        return Ok(token);
    }
    match credentials.refresh_token.as_deref() {
        Some(refresh_token) if !refresh_token.is_empty() => refresh(client, refresh_token).await,
        _ => Err(RateLimitFetchError::message(SIGN_IN_EXPIRED)),
    }
}

async fn post(
    client: &reqwest::Client,
    token: &str,
    method: &str,
    body: Value,
) -> Result<Value, RateLimitFetchError> {
    let response = client
        .post(format!("{CODE_ASSIST_URL}:{method}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Gemini {method} failed: {e}")))?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(RateLimitFetchError::message(SIGN_IN_EXPIRED));
    }
    if !response.status().is_success() {
        return Err(rate_limit_error_from_response(&response));
    }
    response
        .json()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Gemini {method} failed: {e}")))
}

/// The project id in a `loadCodeAssist` reply: a string, or `{id}`.
fn project_id(assist: &Value) -> Option<String> {
    let project = assist.get("cloudaicompanionProject")?;
    project
        .as_str()
        .or_else(|| project.get("id")?.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

fn tier_name(assist: &Value) -> Option<String> {
    ["paidTier", "currentTier"]
        .iter()
        .find_map(|tier| assist.get(tier)?.get("name")?.as_str())
        .map(str::to_string)
}

fn build_gemini_rate_limits(quota: &Value, plan_tier: Option<String>) -> ProviderRateLimits {
    let mut windows: Vec<RateLimitWindow> = Vec::new();
    let buckets = quota.get("buckets").and_then(Value::as_array);
    for bucket in buckets.into_iter().flatten() {
        let Some(model) = bucket.get("modelId").and_then(Value::as_str) else {
            continue;
        };
        // `<model>_vertex` repeats the model's own bucket.
        if model.ends_with("_vertex") {
            continue;
        }
        let reset = bucket.get("resetTime").and_then(Value::as_str);
        // Proto3 JSON omits a zero fraction: a bucket with only a reset time
        // is used up.
        let remaining = match bucket.get("remainingFraction").and_then(Value::as_f64) {
            Some(fraction) => fraction,
            None if reset.is_some() => 0.0,
            None => continue,
        };
        let utilization = (1.0 - remaining).clamp(0.0, 1.0) * 100.0;
        // A model can have a bucket per token type; the tighter one binds.
        match windows.iter_mut().find(|window| window.window_id == model) {
            Some(window) => window.utilization = window.utilization.max(utilization),
            None => windows.push(RateLimitWindow::new(
                model.to_string(),
                crate::models::normalize_model(model).0,
                utilization,
                reset.map(str::to_string),
            )),
        }
    }
    // Busiest first: the first window is the one the tray shows.
    windows.sort_by(|a, b| b.utilization.total_cmp(&a.utilization));

    ProviderRateLimits {
        provider: "gemini".to_string(),
        plan_tier,
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

pub(super) async fn fetch_gemini_rate_limits() -> Result<ProviderRateLimits, RateLimitFetchError> {
    let client = reqwest::Client::builder()
        .timeout(super::HTTP_TIMEOUT)
        .build()
        .map_err(|e| RateLimitFetchError::message(format!("HTTP client error: {e}")))?;
    let token = access_token(&client).await?;

    let assist = post(
        &client,
        &token,
        "loadCodeAssist",
        json!({"metadata": {
            "ideType": "IDE_UNSPECIFIED",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }}),
    )
    .await?;
    let project = project_id(&assist)
        .or_else(|| std::env::var("GOOGLE_CLOUD_PROJECT").ok())
        .ok_or_else(|| {
            RateLimitFetchError::message("Gemini account has no Code Assist project yet")
        })?;
    let quota = post(
        &client,
        &token,
        "retrieveUserQuota",
        json!({"project": project}),
    )
    .await?;
    Ok(build_gemini_rate_limits(&quota, tier_name(&assist)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_js_string_constants() {
        let source =
            r#"var a=1;var OAUTH_CLIENT_ID = "id-1.apps";const OAUTH_CLIENT_SECRET = 's2';"#;
        assert_eq!(
            js_const(source, "OAUTH_CLIENT_ID").as_deref(),
            Some("id-1.apps")
        );
        assert_eq!(
            js_const(source, "OAUTH_CLIENT_SECRET").as_deref(),
            Some("s2")
        );
        assert_eq!(js_const(source, "MISSING"), None);
        if super::super::command_in_path("gemini").is_some() {
            let (id, _) = read_cli_client().expect("installed Gemini CLI has an OAuth client");
            assert!(id.ends_with(".apps.googleusercontent.com"));
        }
    }

    #[test]
    fn buckets_become_windows_busiest_first() {
        let quota = json!({"buckets": [
            {"modelId": "gemini-2.5-flash", "remainingFraction": 0.75, "resetTime": "2026-11-13T08:00:00Z", "tokenType": "REQUESTS"},
            {"modelId": "gemini-2.5-pro", "resetTime": "2026-11-13T08:00:00Z", "tokenType": "REQUESTS"},
            {"modelId": "gemini-2.5-pro_vertex", "remainingFraction": 1.0},
            {"modelId": "gemini-2.5-flash", "remainingFraction": 0.5, "tokenType": "TOKENS"},
            {"modelId": "no-quota"}
        ]});
        let limits = build_gemini_rate_limits(&quota, Some("Free".into()));
        let seen: Vec<_> = limits
            .windows
            .iter()
            .map(|w| (w.window_id.as_str(), w.utilization.round()))
            .collect();
        assert_eq!(
            seen,
            vec![("gemini-2.5-pro", 100.0), ("gemini-2.5-flash", 50.0)]
        );
        assert_eq!(
            limits.windows[1].resets_at.as_deref(),
            Some("2026-11-13T08:00:00Z")
        );
    }

    #[test]
    fn reads_project_and_tier_from_load_code_assist() {
        let assist = json!({"cloudaicompanionProject": {"id": "p-1"}, "currentTier": {"name": "Free"}, "paidTier": {"name": "Pro"}});
        assert_eq!(project_id(&assist).as_deref(), Some("p-1"));
        assert_eq!(tier_name(&assist).as_deref(), Some("Pro"));
        assert_eq!(
            project_id(&json!({"cloudaicompanionProject": "p-2"})).as_deref(),
            Some("p-2")
        );
    }
}
