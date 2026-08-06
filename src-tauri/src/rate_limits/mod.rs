mod claude;
mod claude_cli;
mod codex;
mod codex_cli;
mod cursor;
mod http;
// Refresh-grant flow uses the owned-mirror keychain item, which is
// macOS-only. Gating the module avoids dead-code warnings on
// Linux/Windows where nothing imports it.
#[cfg(target_os = "macos")]
mod oauth_refresh;

use crate::models::RateLimitWindow;
use crate::models::{ProviderRateLimits, RateLimitsPayload};
use crate::statusline;
use chrono::{DateTime, Duration, Utc};
use std::path::{Path, PathBuf};

pub(crate) fn command_in_path(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        #[cfg(target_os = "windows")]
        {
            // On Windows, prefer .cmd/.exe over bare names — npm installs a
            // POSIX shell shim as the bare name that cannot be executed
            // directly by CreateProcessW (error 193).
            let cmd = dir.join(format!("{binary}.cmd"));
            if cmd.is_file() {
                return Some(cmd);
            }
            let exe = dir.join(format!("{binary}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let candidate = dir.join(binary);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Freshness window for statusline data. If the last CC prompt was within
/// this duration, the statusline `used_percentage` is authoritative and we
/// skip the OAuth/CLI probe entirely.
const STATUSLINE_FRESHNESS: Duration = Duration::minutes(10);

/// Try to build a `ProviderRateLimits` from the most recent statusline event.
/// Returns `None` if the statusline is not installed, has no events, or the
/// most recent event is older than `STATUSLINE_FRESHNESS`.
fn fetch_claude_from_statusline() -> Option<ProviderRateLimits> {
    let session = statusline::source::latest_active_session(&statusline::events_file())
        .ok()
        .flatten()?;

    if !session.is_fresh(STATUSLINE_FRESHNESS, Utc::now()) {
        return None;
    }

    // We need at least one window to consider this a usable payload.
    if session.five_hour.is_none() && session.seven_day.is_none() {
        return None;
    }

    let mut windows = Vec::with_capacity(2);
    if let Some(w) = session.five_hour {
        windows.push(RateLimitWindow::new(
            "five_hour".to_string(),
            "Session (5hr)".to_string(),
            w.used_percentage,
            DateTime::from_timestamp(w.resets_at_unix, 0).map(|dt| dt.to_rfc3339()),
        ));
    }
    if let Some(w) = session.seven_day {
        windows.push(RateLimitWindow::new(
            "seven_day".to_string(),
            "Weekly (7d)".to_string(),
            w.used_percentage,
            DateTime::from_timestamp(w.resets_at_unix, 0).map(|dt| dt.to_rfc3339()),
        ));
    }

    Some(ProviderRateLimits {
        provider: "claude".to_string(),
        plan_tier: None,
        windows,
        extra_usage: None,
        credits: None,
        stale: false,
        error: None,
        retry_after_seconds: None,
        cooldown_until: None,
        fetched_at: session.last_seen.to_rfc3339(),
    })
}

use claude::fetch_claude_rate_limits;
use codex::extract_codex_rate_limits;
use codex_cli::fetch_codex_rate_limits_via_cli;
use cursor::fetch_cursor_rate_limits;
use http::{
    mark_rate_limits_stale, merge_provider_rate_limits, provider_cooldown_is_active,
    provider_rate_limit_error,
};

/// Trigger the one-time interactive Keychain prompt so the user can grant
/// "Always Allow" access for TokenMonitor. Only the explicit setup flow
/// should invoke this — every other Keychain read uses the silent path.
#[cfg(target_os = "macos")]
pub fn request_claude_keychain_access() -> Result<(), String> {
    claude::prime_token_from_keychain_interactive()
}

/// Returns `true` when a silent token read currently succeeds — either
/// from our owned mirror item or from Claude Code-credentials' ACL. Used
/// by the onboarding wizard to detect the already-granted state without
/// requiring a click. Never opens a UI prompt.
#[cfg(target_os = "macos")]
pub fn has_silent_claude_token() -> bool {
    claude::get_claude_oauth_token().is_ok()
}

/// Test-only: force a refresh-grant attempt against the owned mirror's
/// refresh token. Returns a short status string describing the outcome.
/// Used to exercise the refresh path live without waiting for Anthropic
/// to rotate the access token naturally.
#[cfg(target_os = "macos")]
pub async fn debug_force_refresh() -> String {
    claude::debug_force_refresh().await
}

/// Minimum seconds between Claude rate-limit probes.  Both the OAuth API and
/// the CLI fallback count against the user's rate-limit budget, so we avoid
/// re-fetching when the cached data is still recent.  The frontend enforces a
/// matching 5-minute interval via `minFetchIntervalMs`.
const CLAUDE_MIN_REFETCH_SECS: i64 = 300;
const CODEX_MIN_REFETCH_SECS: i64 = 300;

#[derive(Debug, Clone)]
pub(crate) struct RateLimitFetchError {
    message: String,
    retry_after_seconds: Option<u64>,
    cooldown_until: Option<String>,
}

impl RateLimitFetchError {
    fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retry_after_seconds: None,
            cooldown_until: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitSelection {
    All,
    Claude,
    Codex,
    Cursor,
}

impl RateLimitSelection {
    pub fn includes_claude(self) -> bool {
        matches!(self, Self::All | Self::Claude)
    }

    pub fn includes_codex(self) -> bool {
        matches!(self, Self::All | Self::Codex)
    }

    pub fn includes_cursor(self) -> bool {
        matches!(self, Self::All | Self::Cursor)
    }
}

/// Returns `true` when the cached provider data was fetched recently enough
/// that we should skip a new probe.  Only considers data with at least one
/// usable window — error-only payloads are never treated as fresh so we
/// retry immediately instead of showing "No rate limit data".
fn is_fresh(cached: Option<&ProviderRateLimits>, min_age_secs: i64, now: DateTime<Utc>) -> bool {
    cached
        .filter(|rl| !rl.windows.is_empty())
        .and_then(|rl| DateTime::parse_from_rfc3339(&rl.fetched_at).ok())
        .map(|fetched| (now - fetched.with_timezone(&Utc)).num_seconds() < min_age_secs)
        .unwrap_or(false)
}

/// Backoff gate for the Claude *OAuth* fallback only.
///
/// Each OAuth probe spends two requests (usage + account) against Anthropic's
/// abuse guard, and the background refresh fires every ~2.5 min regardless of
/// what the UI is doing. So a `429 Retry-After: 3600` has to stop us here —
/// the frontend's own deferral only gates its own calls, and without this the
/// backend loop keeps hammering a cooled-down endpoint for the whole hour.
///
/// Deliberately does *not* gate the CLI path: the 429 belongs to the API
/// endpoint we call directly, and Claude Code asking on its own behalf is
/// unaffected by it.
fn oauth_cooldown_hold(
    cached: Option<&ProviderRateLimits>,
    now: DateTime<Utc>,
) -> Option<ProviderRateLimits> {
    let cached = cached?;
    provider_cooldown_is_active(cached, now).then(|| mark_rate_limits_stale(cached.clone()))
}

pub fn merge_rate_limits(
    fresh: RateLimitsPayload,
    cached: Option<&RateLimitsPayload>,
) -> RateLimitsPayload {
    RateLimitsPayload {
        claude: merge_provider_rate_limits(
            fresh.claude,
            cached.and_then(|payload| payload.claude.clone()),
        ),
        codex: merge_provider_rate_limits(
            fresh.codex,
            cached.and_then(|payload| payload.codex.clone()),
        ),
        cursor: merge_provider_rate_limits(
            fresh.cursor,
            cached.and_then(|payload| payload.cursor.clone()),
        ),
    }
}

pub async fn fetch_selected_rate_limits(
    codex_dir: &Path,
    selection: RateLimitSelection,
    cached: Option<&RateLimitsPayload>,
) -> RateLimitsPayload {
    let codex_dir = codex_dir.to_path_buf();

    let cached_claude = cached.and_then(|payload| payload.claude.clone());
    let cached_codex = cached.and_then(|payload| payload.codex.clone());
    let cached_cursor = cached.and_then(|payload| payload.cursor.clone());

    let claude_future = async {
        if !selection.includes_claude() {
            return cached_claude;
        }

        let now = Utc::now();

        // Primary: statusline — CC pushes server-authoritative used_percentage
        // on every prompt, no network call, no budget cost.
        if let Some(sl) = tokio::task::spawn_blocking(fetch_claude_from_statusline)
            .await
            .ok()
            .flatten()
        {
            tracing::debug!("Claude rate limits served from statusline");
            return Some(sl);
        }

        // Shared throttle for both remaining paths. The windows we track are
        // 5h and 7d, so a 5-minute floor loses nothing visible and keeps us
        // from spawning a CLI (or spending API budget) every 2.5 min.
        if is_fresh(cached_claude.as_ref(), CLAUDE_MIN_REFETCH_SECS, now) {
            return cached_claude;
        }

        // Secondary: ask Claude Code itself via `claude -p "/usage"`. Costs no
        // tokens (`num_turns: 0`) and leaves credentials, refresh, and
        // rate-limit retry entirely to the CLI — same division of labour as
        // the Codex app-server probe.
        match claude_cli::fetch_claude_rate_limits_via_cli().await {
            Ok(rate_limits) => {
                tracing::debug!("Claude rate limits served from CLI /usage");
                return Some(rate_limits);
            }
            Err(error) => {
                tracing::debug!(error = %error.message, "Claude CLI /usage probe failed");
            }
        }

        // Last resort: call Anthropic's OAuth API ourselves. This is the only
        // path that spends the account's rate-limit budget, so it is the only
        // one the server cooldown holds back.
        if let Some(held) = oauth_cooldown_hold(cached_claude.as_ref(), now) {
            return Some(held);
        }

        match fetch_claude_rate_limits().await {
            Ok(rate_limits) => Some(rate_limits),
            Err(error) => {
                tracing::debug!(error = %error.message, "Claude OAuth API failed");

                tracing::warn!(
                    error = %error.message,
                    "Claude rate-limit: statusline + API both failed"
                );
                Some(provider_rate_limit_error("claude", error))
            }
        }
    };

    let codex_future = async move {
        if !selection.includes_codex() {
            return cached_codex;
        }

        let now = Utc::now();
        if is_fresh(cached_codex.as_ref(), CODEX_MIN_REFETCH_SECS, now) {
            return cached_codex;
        }

        match fetch_codex_rate_limits_via_cli().await {
            Ok(rate_limits) => Some(rate_limits),
            Err(cli_err) => {
                tracing::debug!(error = %cli_err.message, "Codex app-server probe failed, falling back to file");
                match tokio::task::spawn_blocking(move || extract_codex_rate_limits(&codex_dir))
                    .await
                {
                    Ok(Ok(rate_limits)) => Some(rate_limits),
                    Ok(Err(error)) => Some(provider_rate_limit_error(
                        "codex",
                        RateLimitFetchError::message(error),
                    )),
                    Err(error) => Some(provider_rate_limit_error(
                        "codex",
                        RateLimitFetchError::message(format!("Task failed: {error}")),
                    )),
                }
            }
        }
    };

    let cursor_future = async {
        if !selection.includes_cursor() {
            return cached_cursor;
        }

        let now = Utc::now();

        if let Some(rate_limits) = cached_cursor.clone() {
            if provider_cooldown_is_active(&rate_limits, now) {
                return Some(mark_rate_limits_stale(rate_limits));
            }
        }

        match fetch_cursor_rate_limits().await {
            Ok(rate_limits) => Some(rate_limits),
            Err(error) => {
                tracing::warn!(error = %error.message, "Cursor rate-limit fetch failed");
                Some(provider_rate_limit_error("cursor", error))
            }
        }
    };

    let (claude, codex, cursor) = tokio::join!(claude_future, codex_future, cursor_future);
    RateLimitsPayload {
        claude,
        codex,
        cursor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    use crate::models::RateLimitWindow;

    fn make_provider_with_windows(
        fetched_at: &str,
        windows: Vec<RateLimitWindow>,
    ) -> ProviderRateLimits {
        ProviderRateLimits {
            provider: "claude".to_string(),
            plan_tier: None,
            windows,
            extra_usage: None,
            credits: None,
            stale: false,
            error: None,
            retry_after_seconds: None,
            cooldown_until: None,
            fetched_at: fetched_at.to_string(),
        }
    }

    fn sample_window() -> RateLimitWindow {
        RateLimitWindow::new(
            "five_hour".to_string(),
            "Session (5hr)".to_string(),
            0.0,
            None,
        )
    }

    #[test]
    fn is_fresh_returns_true_when_within_window_and_has_data() {
        let now = Utc::now();
        let recent = make_provider_with_windows(
            &(now - Duration::seconds(60)).to_rfc3339(),
            vec![sample_window()],
        );
        assert!(is_fresh(Some(&recent), 300, now));
    }

    #[test]
    fn is_fresh_returns_false_when_expired() {
        let now = Utc::now();
        let old = make_provider_with_windows(
            &(now - Duration::seconds(600)).to_rfc3339(),
            vec![sample_window()],
        );
        assert!(!is_fresh(Some(&old), 300, now));
    }

    #[test]
    fn is_fresh_returns_false_when_no_cache() {
        assert!(!is_fresh(None, 300, Utc::now()));
    }

    #[test]
    fn oauth_is_held_back_while_the_server_cooldown_is_active() {
        let now = Utc::now();
        // A 429 an hour ago with Retry-After: 3600 — stale windows, cooldown
        // still 55 min out. Calling the API again just re-arms the ban.
        let mut cached = make_provider_with_windows(
            &(now - Duration::minutes(60)).to_rfc3339(),
            vec![sample_window()],
        );
        cached.error = Some("Usage API returned 429 Too Many Requests".to_string());
        cached.cooldown_until = Some((now + Duration::minutes(55)).to_rfc3339());

        let held = oauth_cooldown_hold(Some(&cached), now).expect("OAuth must be held back");
        assert!(held.stale);
    }

    #[test]
    fn oauth_runs_once_the_cooldown_has_expired() {
        let now = Utc::now();
        let mut cached = make_provider_with_windows(
            &(now - Duration::minutes(60)).to_rfc3339(),
            vec![sample_window()],
        );
        cached.cooldown_until = Some((now - Duration::minutes(1)).to_rfc3339());

        assert!(oauth_cooldown_hold(Some(&cached), now).is_none());
        assert!(oauth_cooldown_hold(None, now).is_none());
    }

    #[test]
    fn is_fresh_returns_false_when_cached_has_no_windows() {
        let now = Utc::now();
        let error_only =
            make_provider_with_windows(&(now - Duration::seconds(10)).to_rfc3339(), vec![]);
        assert!(!is_fresh(Some(&error_only), 300, now));
    }
}
