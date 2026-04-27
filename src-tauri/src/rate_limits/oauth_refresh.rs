//! OAuth refresh-token grant against Anthropic's token endpoint.
//!
//! When the API returns 401 we don't have to fall back to the interactive
//! Keychain prompt — Claude Code's credentials JSON includes a `refreshToken`
//! that we can exchange for a fresh access token via the standard OAuth2
//! refresh flow. Doing so lets us survive every Anthropic-side access-token
//! rotation without the user re-granting Keychain access.
//!
//! The exchange uses Claude Code's published OAuth client_id (constant
//! across installs), which is the same identity Claude Code itself
//! presents when refreshing — so Anthropic treats our refresh as
//! indistinguishable from a Claude Code refresh.
//!
//! Failure modes that should *not* trigger Keychain re-grant:
//!   - transient network failure (`reqwest::Error` not status-related)
//!   - 5xx from Anthropic
//!
//! Failure modes that *should* trigger re-grant (refresh token revoked):
//!   - 400 `invalid_grant`
//!   - 401 on the refresh endpoint itself

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Anthropic's OAuth token endpoint. Same URL Claude Code uses for refresh.
const REFRESH_ENDPOINT: &str = "https://console.anthropic.com/v1/oauth/token";

/// Claude Code's published OAuth client identifier. Unchanged across all
/// installs; we present the same identity Claude Code does so Anthropic
/// rate-limits / accounts the refresh under the same client.
const CLAUDE_CODE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// Hard cap on the refresh request — Anthropic's token endpoint typically
/// answers in well under a second. If the request hangs we'd rather surface
/// an error than block the async refresh loop.
const REFRESH_TIMEOUT_SECS: u64 = 12;

#[derive(Serialize)]
struct RefreshRequest<'a> {
    grant_type: &'a str,
    refresh_token: &'a str,
    client_id: &'a str,
}

#[derive(Deserialize, Debug)]
pub(crate) struct RefreshResponse {
    pub access_token: String,
    /// Anthropic *may* rotate the refresh token in the response. We persist
    /// it back into the mirror when present, otherwise keep the existing one.
    pub refresh_token: Option<String>,
    /// Lifetime of the new access token in seconds.
    pub expires_in: Option<u64>,
}

/// Outcome of a refresh attempt. We split "definitely revoked" from
/// "transient" so the caller can decide whether to surface re-grant UI
/// (revoked) or just keep the existing mirror and try again next cycle
/// (transient).
#[derive(Debug)]
pub(crate) enum RefreshOutcome {
    Refreshed(RefreshResponse),
    /// Anthropic explicitly rejected the refresh token. Caller should
    /// delete the mirror and prompt the user.
    Revoked(String),
    /// Network failure / 5xx. Caller should leave the mirror in place
    /// and rely on the next refresh cycle.
    Transient(String),
}

pub(crate) async fn refresh_oauth_token(refresh_token: &str) -> RefreshOutcome {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(REFRESH_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => return RefreshOutcome::Transient(format!("client build: {e}")),
    };

    let resp = match client
        .post(REFRESH_ENDPOINT)
        .json(&RefreshRequest {
            grant_type: "refresh_token",
            refresh_token,
            client_id: CLAUDE_CODE_CLIENT_ID,
        })
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return RefreshOutcome::Transient(format!("network: {e}"));
        }
    };

    let status = resp.status();
    if status.is_success() {
        match resp.json::<RefreshResponse>().await {
            Ok(parsed) => RefreshOutcome::Refreshed(parsed),
            Err(e) => RefreshOutcome::Transient(format!("response parse: {e}")),
        }
    } else if status == reqwest::StatusCode::BAD_REQUEST
        || status == reqwest::StatusCode::UNAUTHORIZED
    {
        let body = resp.text().await.unwrap_or_default();
        // `invalid_grant` is the OAuth2 standard for "refresh token no
        // longer valid". Anthropic also returns 401 when the token has
        // been revoked outright.
        RefreshOutcome::Revoked(format!("status={status} body={body}"))
    } else {
        let body = resp.text().await.unwrap_or_default();
        RefreshOutcome::Transient(format!("status={status} body={body}"))
    }
}
