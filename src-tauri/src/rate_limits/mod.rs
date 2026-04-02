mod claude;
mod claude_cli;
mod codex;
mod http;

use crate::models::RateLimitsPayload;
use chrono::Utc;
use std::path::Path;

use claude::fetch_claude_rate_limits;
use claude_cli::fetch_claude_rate_limits_via_cli;
use codex::extract_codex_rate_limits;
use http::{
    mark_rate_limits_stale, merge_provider_rate_limits, provider_cooldown_is_active,
    provider_rate_limit_error,
};

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

        if let Some(rate_limits) = cached_claude.clone() {
            if provider_cooldown_is_active(&rate_limits, Utc::now()) {
                return Some(mark_rate_limits_stale(rate_limits));
            }
        }

        match fetch_claude_rate_limits().await {
            Ok(rate_limits) => Some(rate_limits),
            Err(error) => match fetch_claude_rate_limits_via_cli(cached_claude.as_ref()).await {
                Ok(rate_limits) => Some(rate_limits),
                Err(cli_error) => {
                    tracing::warn!(
                        api_error = %error.message,
                        cli_error = %cli_error.message,
                        "Claude rate-limit: both API and CLI fallback failed"
                    );
                    Some(provider_rate_limit_error("claude", error))
                }
            },
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
