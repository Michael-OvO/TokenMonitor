mod claude;
mod claude_cli;
mod codex;
mod http;
// Refresh-grant flow uses the owned-mirror keychain item, which is
// macOS-only. Gating the module avoids dead-code warnings on
// Linux/Windows where nothing imports it.
#[cfg(target_os = "macos")]
mod oauth_refresh;

use crate::models::{ProviderRateLimits, RateLimitsPayload};
use chrono::{DateTime, Duration, Utc};
use std::path::Path;

use claude::fetch_claude_rate_limits;
use claude_cli::fetch_claude_rate_limits_via_cli;
use codex::extract_codex_rate_limits;
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

    fn cooldown(message: impl Into<String>, retry_after_seconds: u64) -> Self {
        let cooldown_until = Utc::now() + Duration::seconds(retry_after_seconds as i64);
        Self {
            message: message.into(),
            retry_after_seconds: Some(retry_after_seconds),
            cooldown_until: Some(cooldown_until.to_rfc3339()),
        }
    }

    fn is_claude_auth_unavailable(&self) -> bool {
        self.message.contains("Claude Code is not logged in")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitSelection {
    All,
    Claude,
    Codex,
}

impl RateLimitSelection {
    pub fn includes_claude(self) -> bool {
        matches!(self, Self::All | Self::Claude)
    }

    pub fn includes_codex(self) -> bool {
        matches!(self, Self::All | Self::Codex)
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

    let claude_future = async {
        if !selection.includes_claude() {
            return cached_claude;
        }

        let now = Utc::now();

        // Always try the OAuth API first — it's a metadata endpoint that
        // returns all windows (five_hour + weekly + per-model + extra usage)
        // and does NOT consume any rate-limit budget. It works even when the
        // user has exhausted their 5h window, so a CLI-derived
        // `cooldown_until` (set when status=rejected lands at the window
        // reset time) must NOT short-circuit it. Cooldown applies only to
        // the CLI fallback below, which actually consumes budget.
        match fetch_claude_rate_limits().await {
            Ok(rate_limits) => Some(rate_limits),
            Err(error) => {
                tracing::debug!(error = %error.message, "Claude OAuth API failed, considering CLI fallback");

                // Provider cooldown gates the CLI probe — running it during
                // a known rejection window just produces another rejection
                // and can poke the rate limit further.
                if let Some(cached) = cached_claude.as_ref() {
                    if provider_cooldown_is_active(cached, now) {
                        return Some(mark_rate_limits_stale(cached.clone()));
                    }
                }

                // Only fall back to the CLI probe when the cached data is
                // stale or missing. The CLI costs rate-limit budget and can
                // only report one window, so we throttle it.
                if is_fresh(cached_claude.as_ref(), CLAUDE_MIN_REFETCH_SECS, now) {
                    return cached_claude;
                }

                match fetch_claude_rate_limits_via_cli(cached_claude.as_ref()).await {
                    Ok(rate_limits) => Some(rate_limits),
                    Err(cli_error) => {
                        tracing::warn!(
                            api_error = %error.message,
                            cli_error = %cli_error.message,
                            "Claude rate-limit: both API and CLI fallback failed"
                        );
                        let surfaced_error = if cli_error.is_claude_auth_unavailable() {
                            cli_error
                        } else {
                            error
                        };
                        Some(provider_rate_limit_error("claude", surfaced_error))
                    }
                }
            }
        }
    };

    let codex_future = async move {
        if !selection.includes_codex() {
            return cached_codex;
        }

        match tokio::task::spawn_blocking(move || extract_codex_rate_limits(&codex_dir)).await {
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
    };

    let (claude, codex) = tokio::join!(claude_future, codex_future);
    RateLimitsPayload { claude, codex }
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
    fn is_fresh_returns_false_when_cached_has_no_windows() {
        let now = Utc::now();
        let error_only =
            make_provider_with_windows(&(now - Duration::seconds(10)).to_rfc3339(), vec![]);
        assert!(!is_fresh(Some(&error_only), 300, now));
    }
}
