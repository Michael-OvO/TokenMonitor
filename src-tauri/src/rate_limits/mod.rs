mod claude;
mod claude_cli;
mod codex;
mod http;

use crate::models::{ProviderRateLimits, RateLimitsPayload};
use chrono::{DateTime, Utc};
use std::path::Path;

use claude::fetch_claude_rate_limits;
use claude_cli::fetch_claude_rate_limits_via_cli;
use codex::extract_codex_rate_limits;
use http::{
    mark_rate_limits_stale, merge_provider_rate_limits, provider_cooldown_is_active,
    provider_rate_limit_error,
};

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

        if let Some(rate_limits) = cached_claude.clone() {
            if provider_cooldown_is_active(&rate_limits, now) {
                return Some(mark_rate_limits_stale(rate_limits));
            }
        }

        // Always try the OAuth API first — it returns all windows (five_hour
        // + weekly) and does NOT consume any rate-limit budget.
        match fetch_claude_rate_limits().await {
            Ok(rate_limits) => Some(rate_limits),
            Err(error) => {
                tracing::debug!(error = %error.message, "Claude OAuth API failed, considering CLI fallback");

                // Only fall back to the CLI probe when the cached data is
                // stale or missing.  The CLI costs rate-limit budget and
                // can only report one window, so we throttle it.
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
                        Some(provider_rate_limit_error("claude", error))
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
