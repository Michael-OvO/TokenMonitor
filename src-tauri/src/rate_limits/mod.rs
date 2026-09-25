mod claude;
mod claude_cli;
mod codex;
mod codex_cli;
mod cursor;
mod http;
mod kimi;

use crate::models::RateLimitWindow;
use crate::models::{ProviderRateLimits, RateLimitsPayload};
use crate::statusline;
use crate::usage::integrations::UsageIntegrationId;
use crate::usage::parser::cursor_idle_backoff;
use chrono::{DateTime, Duration, Utc};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

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

fn as_f64(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn humanize_snake_case(field: &str) -> String {
    field
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = first.to_uppercase().collect::<String>();
                    out.push_str(&chars.as_str().to_lowercase());
                    out
                }
                None => part.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Freshness window for statusline data. If the last CC prompt was within
/// this duration, the statusline `used_percentage` is authoritative and we
/// skip the OAuth/CLI probe entirely.
const STATUSLINE_FRESHNESS: Duration = Duration::minutes(10);

/// Try to build a `ProviderRateLimits` from the most recent statusline event.
/// Returns `None` if the statusline is not installed, has no events, or the
/// most recent event is older than `STATUSLINE_FRESHNESS`.
fn fetch_claude_from_statusline() -> Option<ProviderRateLimits> {
    let now = Utc::now();
    let session = statusline::source::latest_active_session(
        &statusline::events_file(),
        now - STATUSLINE_FRESHNESS,
    )
    .ok()
    .flatten()?;

    if !session.is_fresh(STATUSLINE_FRESHNESS, now) {
        return None;
    }

    // We need at least one window to consider this a usable payload.
    if session.windows.is_empty() {
        return None;
    }

    let windows = session
        .windows
        .iter()
        .map(|named| {
            RateLimitWindow::new(
                named.window_id.clone(),
                claude::claude_window_label(&named.window_id),
                named.window.used_percentage,
                DateTime::from_timestamp(named.window.resets_at_unix, 0).map(|dt| dt.to_rfc3339()),
            )
        })
        .collect();

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
use codex::{codex_log_reading, extract_codex_rate_limits};
use codex_cli::fetch_codex_rate_limits_via_cli;
use cursor::fetch_cursor_rate_limits;
use http::{
    mark_rate_limits_stale, merge_provider_rate_limits, provider_cooldown_is_active,
    provider_rate_limit_error,
};
use kimi::fetch_kimi_rate_limits;

/// Minimum seconds between Claude rate-limit probes. Spans both the CLI probe
/// (a process spawn) and the OAuth fallback (two requests against the
/// account's budget), so we skip re-fetching while the cached data is recent.
const CLAUDE_MIN_REFETCH_SECS: i64 = 300;
const CODEX_MIN_REFETCH_SECS: i64 = 300;
const KIMI_MIN_REFETCH_SECS: i64 = 300;
/// Cursor's probe is an HTTPS round trip (plus a `sqlite3` spawn to re-read
/// the IDE's token when its state DB changed), and until it had this gate it
/// ran on every statusline event, several times a minute while Claude Code works.
const CURSOR_MIN_REFETCH_SECS: i64 = 300;
/// Cursor's floor in use: [`CURSOR_MIN_REFETCH_SECS`], doubled by each probe
/// whose meters had not moved (see `cursor_idle_backoff`), and back to the
/// base on Cursor activity ([`reset_cursor_refetch_floor`]).
static CURSOR_REFETCH_SECS: AtomicU64 = AtomicU64::new(CURSOR_MIN_REFETCH_SECS as u64);
/// Bound on every rate-limit HTTP call, so a stalled connection cannot hold
/// a refresh open indefinitely.
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Cursor may be in use: probe its meters on the base floor again.
pub(crate) fn reset_cursor_refetch_floor() {
    CURSOR_REFETCH_SECS.store(CURSOR_MIN_REFETCH_SECS as u64, Ordering::Relaxed);
}

/// Whether a reading shows the same meters as `cached`.
fn same_meters(fresh: &ProviderRateLimits, cached: Option<&ProviderRateLimits>) -> bool {
    let meters =
        |rl: &ProviderRateLimits| serde_json::to_value((&rl.windows, &rl.extra_usage)).ok();
    cached.is_some_and(|cached| meters(fresh) == meters(cached))
}

/// Claude Code's statusline only reports the plan-wide 5h and 7d windows. The
/// model-specific weekly windows (Weekly Fable, Weekly Opus, ...) exist only in
/// the CLI probe and the OAuth API, so while the statusline is live those are
/// refreshed on this cadence and carried forward in between.
const CLAUDE_MODEL_WINDOWS_REFRESH_SECS: u64 = 900;

/// When the model-specific Claude windows were last probed (or attempted).
static CLAUDE_MODEL_WINDOWS_REFRESHED_AT: Mutex<Option<Instant>> = Mutex::new(None);
/// Codex's usage-limit resets come only from the app-server probe, so while
/// logged readings stand in for it, it still runs on this cadence.
const CODEX_RESETS_REFRESH_SECS: u64 = 900;
/// When the Codex app-server was last probed (or attempted).
static CODEX_APP_SERVER_PROBED_AT: Mutex<Option<Instant>> = Mutex::new(None);

fn last_attempt(slot: &Mutex<Option<Instant>>) -> Option<Instant> {
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn mark_attempt(slot: &Mutex<Option<Instant>>, now: Instant) {
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(now);
}

/// Whether a probe run every `every_secs` is due: never done yet, or the
/// interval less [`PROBE_SLACK_SECS`] (refresh ticks are not exact) has passed
/// since the last attempt.
fn refresh_due(last: Option<Instant>, now: Instant, every_secs: u64) -> bool {
    last.is_none_or(|at| now.duration_since(at).as_secs() + PROBE_SLACK_SECS as u64 >= every_secs)
}

fn window_expired(window: &RateLimitWindow, now: DateTime<Utc>) -> bool {
    window
        .resets_at
        .as_deref()
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .is_some_and(|resets_at| resets_at.with_timezone(&Utc) < now)
}

/// Lay the live statusline windows over the richer `base` payload: a window
/// the statusline reports replaces the base one, every other base window that
/// has not already reset is kept, and plan details stay with the base, as does
/// an OAuth cooldown still running (it gates the next rich probe). The result
/// is timestamped by the live source.
fn overlay_live_windows(
    base: Option<ProviderRateLimits>,
    live: ProviderRateLimits,
    now: DateTime<Utc>,
) -> ProviderRateLimits {
    let Some(base) = base else {
        return live;
    };
    let (cooldown_until, retry_after_seconds) = if provider_cooldown_is_active(&base, now) {
        (base.cooldown_until.clone(), base.retry_after_seconds)
    } else {
        (live.cooldown_until.clone(), live.retry_after_seconds)
    };
    let mut windows = live.windows.clone();
    for window in base.windows {
        let already_live = windows.iter().any(|w| w.window_id == window.window_id);
        if already_live || window_expired(&window, now) {
            continue;
        }
        windows.push(window);
    }
    ProviderRateLimits {
        windows,
        plan_tier: base.plan_tier.or(live.plan_tier),
        extra_usage: base.extra_usage.or(live.extra_usage),
        credits: base.credits.or(live.credits),
        cooldown_until,
        retry_after_seconds,
        ..live
    }
}

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

/// Which providers a rate-limit fetch may probe; the others keep whatever
/// is cached. Built from the integrations the user has enabled, so a provider
/// that is switched off never costs a probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RateLimitSelection {
    claude: bool,
    codex: bool,
    cursor: bool,
    kimi: bool,
}

impl RateLimitSelection {
    /// The selection that probes exactly the integrations in `ids`.
    pub fn enabled(ids: &[UsageIntegrationId]) -> Self {
        let mut selection = Self::default();
        for id in ids {
            match id {
                UsageIntegrationId::Claude => selection.claude = true,
                UsageIntegrationId::Codex => selection.codex = true,
                UsageIntegrationId::Cursor => selection.cursor = true,
                UsageIntegrationId::Kimi => selection.kimi = true,
            }
        }
        selection
    }

    pub fn includes_claude(self) -> bool {
        self.claude
    }

    pub fn includes_codex(self) -> bool {
        self.codex
    }

    pub fn includes_cursor(self) -> bool {
        self.cursor
    }

    pub fn includes_kimi(self) -> bool {
        self.kimi
    }
}

/// Seconds taken off a provider's refetch floor, so a reading from one
/// refresh tick has expired by the next tick of the same length instead of
/// being a few seconds short of it and skipping that tick.
const PROBE_SLACK_SECS: i64 = 15;
/// How long an error-only payload holds off the next probe, so a broken
/// provider is retried soon but cannot spawn a process on every refresh.
const ERROR_RETRY_SECS: i64 = 120;

/// Returns `true` when the cached provider data was fetched recently enough
/// that we should skip a new probe. A reading with windows stays fresh for
/// its floor less [`PROBE_SLACK_SECS`]; an error-only payload for
/// [`ERROR_RETRY_SECS`]. An explicit user retry (`retry_errors`) probes a
/// provider whose last probe failed at once, even when the failure kept the
/// previous reading's windows.
fn is_fresh(
    cached: Option<&ProviderRateLimits>,
    min_age_secs: i64,
    now: DateTime<Utc>,
    retry_errors: bool,
) -> bool {
    cached
        .and_then(|rl| {
            if retry_errors && (rl.error.is_some() || rl.windows.is_empty()) {
                return None;
            }
            let max_age_secs = if rl.windows.is_empty() {
                ERROR_RETRY_SECS
            } else {
                min_age_secs - PROBE_SLACK_SECS
            };
            let fetched = DateTime::parse_from_rfc3339(&rl.fetched_at).ok()?;
            Some((now - fetched.with_timezone(&Utc)).num_seconds() < max_age_secs)
        })
        .unwrap_or(false)
}

/// Whether a reading Codex logged can stand in for a probe: it has meters, was
/// logged after the `cached` reading was taken, is no older than a probed
/// reading still kept, and no window has reset since (a probe would show that
/// window at zero).
fn stands_in_for_probe(
    logged: &ProviderRateLimits,
    cached: Option<&ProviderRateLimits>,
    now: DateTime<Utc>,
) -> bool {
    let taken = |rl: &ProviderRateLimits| DateTime::parse_from_rfc3339(&rl.fetched_at).ok();
    let newer = match (taken(logged), cached.and_then(taken)) {
        (Some(logged_at), Some(cached_at)) => logged_at > cached_at,
        (logged_at, _) => logged_at.is_some(),
    };
    newer
        && !logged.windows.is_empty()
        && is_fresh(Some(logged), CODEX_MIN_REFETCH_SECS, now, false)
        && !codex::has_reset(&logged.windows, now)
}

/// Codex logs no usage-limit resets, so a logged reading keeps the ones the
/// last probe saw (with its credits, when the log has none).
fn keep_probed_resets(
    mut logged: ProviderRateLimits,
    cached: Option<&ProviderRateLimits>,
) -> ProviderRateLimits {
    let Some(probed) = cached.and_then(|rl| rl.credits.as_ref()) else {
        return logged;
    };
    match logged.credits.as_mut() {
        Some(credits) => credits.usage_limit_resets = probed.usage_limit_resets.clone(),
        None => logged.credits = Some(probed.clone()),
    }
    logged
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

/// The two sources that know every Claude window: `claude -p "/usage"`
/// first (no tokens, credentials handled by the CLI), then the OAuth API
/// behind its server cooldown. Failure yields an error payload, as before.
async fn probe_claude_rich(
    cached: Option<&ProviderRateLimits>,
    now: DateTime<Utc>,
) -> ProviderRateLimits {
    match claude_cli::fetch_claude_rate_limits_via_cli().await {
        Ok(rate_limits) => {
            tracing::debug!("Claude rate limits served from CLI /usage");
            return rate_limits;
        }
        Err(error) => {
            tracing::debug!(error = %error.message, "Claude CLI /usage probe failed");
        }
    }

    // Last resort: call Anthropic's OAuth API ourselves. This is the only
    // path that spends the account's rate-limit budget, so it is the only
    // one the server cooldown holds back.
    if let Some(held) = oauth_cooldown_hold(cached, now) {
        return held;
    }

    match fetch_claude_rate_limits().await {
        Ok(rate_limits) => rate_limits,
        Err(error) => {
            tracing::debug!(error = %error.message, "Claude OAuth API failed");
            tracing::warn!(
                error = %error.message,
                "Claude rate-limit: statusline + API both failed"
            );
            provider_rate_limit_error("claude", error)
        }
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
        cursor: merge_provider_rate_limits(
            fresh.cursor,
            cached.and_then(|payload| payload.cursor.clone()),
        ),
        kimi: merge_provider_rate_limits(
            fresh.kimi,
            cached.and_then(|payload| payload.kimi.clone()),
        ),
    }
}

/// Probe the selected providers one at a time, so their CLI children never
/// run side by side. Once `deadline` has passed, the providers not reached
/// yet keep their cached value; a probe already under way is never cut short.
/// `retry_errors` is for an explicit user retry: a provider whose last probe
/// failed is probed again at once, while a good reading keeps its floor.
pub async fn fetch_selected_rate_limits_until(
    codex_dir: &Path,
    selection: RateLimitSelection,
    cached: Option<&RateLimitsPayload>,
    deadline: Option<std::time::Instant>,
    retry_errors: bool,
) -> RateLimitsPayload {
    let codex_dir = codex_dir.to_path_buf();

    let cached_claude = cached.and_then(|payload| payload.claude.clone());
    let cached_codex = cached.and_then(|payload| payload.codex.clone());
    let cached_cursor = cached.and_then(|payload| payload.cursor.clone());
    let cached_kimi = cached.and_then(|payload| payload.kimi.clone());

    let claude_future = async {
        let provider_t0 = std::time::Instant::now();
        let result = async {
            if !selection.includes_claude() {
                return cached_claude;
            }

            let now = Utc::now();

            // Primary: statusline — CC pushes server-authoritative used_percentage
            // on every prompt, no network call, no budget cost. It carries only
            // the plan-wide 5h and 7d windows, so it does not stand alone: the
            // model-specific weekly windows come from the richer probe, refreshed
            // every CLAUDE_MODEL_WINDOWS_REFRESH_SECS and carried forward from the
            // cache in between, with the live values laid on top.
            if let Some(live) = tokio::task::spawn_blocking(fetch_claude_from_statusline)
                .await
                .ok()
                .flatten()
            {
                tracing::debug!("Claude rate limits served from statusline");
                let slot = &CLAUDE_MODEL_WINDOWS_REFRESHED_AT;
                let base = if refresh_due(
                    last_attempt(slot),
                    Instant::now(),
                    CLAUDE_MODEL_WINDOWS_REFRESH_SECS,
                ) {
                    // Mark the attempt whatever it yields, so a failing probe
                    // retries on the same cadence instead of every event.
                    mark_attempt(slot, Instant::now());
                    let rich = probe_claude_rich(cached_claude.as_ref(), now).await;
                    // A failed probe keeps the cached windows and carries
                    // its OAuth cooldown, like the fallback path.
                    merge_provider_rate_limits(Some(rich), cached_claude.clone())
                } else {
                    cached_claude.clone()
                };
                return Some(overlay_live_windows(base, live, now));
            }

            // Shared throttle for the remaining paths: a 5-minute floor keeps us
            // from spawning a CLI (or spending API budget) every 2.5 min.
            if is_fresh(
                cached_claude.as_ref(),
                CLAUDE_MIN_REFETCH_SECS,
                now,
                retry_errors,
            ) {
                return cached_claude;
            }

            Some(probe_claude_rich(cached_claude.as_ref(), now).await)
        }
        .await;
        (result, provider_t0.elapsed())
    };

    let codex_future = async move {
        let provider_t0 = std::time::Instant::now();
        let result = async {
        if !selection.includes_codex() {
            return cached_codex;
        }

        let now = Utc::now();
        if is_fresh(cached_codex.as_ref(), CODEX_MIN_REFETCH_SECS, now, retry_errors) {
            return cached_codex;
        }

        // Codex logs the meters the server reports after every turn: while it
        // is used here, those stand in for a probe, except when the reset
        // credits only the probe reports are due (at launch, then every
        // CODEX_RESETS_REFRESH_SECS).
        let slot = &CODEX_APP_SERVER_PROBED_AT;
        if !refresh_due(last_attempt(slot), Instant::now(), CODEX_RESETS_REFRESH_SECS) {
            let since = now - Duration::seconds(CODEX_MIN_REFETCH_SECS - PROBE_SLACK_SECS);
            let log_dir = codex_dir.clone();
            let logged = tokio::task::spawn_blocking(move || codex_log_reading(&log_dir, since))
                .await
                .ok()
                .flatten()
                .filter(|logged| stands_in_for_probe(logged, cached_codex.as_ref(), now))
                .map(|logged| keep_probed_resets(logged, cached_codex.as_ref()));
            if logged.is_some() {
                tracing::debug!("Codex rate limits served from its session logs");
                return logged;
            }
        }

        mark_attempt(slot, Instant::now());
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
        }
        .await;
        (result, provider_t0.elapsed())
    };

    let cursor_future = async {
        let provider_t0 = std::time::Instant::now();
        let result = async {
            if !selection.includes_cursor() {
                return cached_cursor;
            }

            let now = Utc::now();
            let floor = CURSOR_REFETCH_SECS.load(Ordering::Relaxed);
            if is_fresh(cached_cursor.as_ref(), floor as i64, now, retry_errors) {
                return cached_cursor;
            }

            if let Some(rate_limits) = cached_cursor.clone() {
                if provider_cooldown_is_active(&rate_limits, now) {
                    return Some(mark_rate_limits_stale(rate_limits));
                }
            }

            match fetch_cursor_rate_limits().await {
                Ok(rate_limits) => {
                    let changed = !same_meters(&rate_limits, cached_cursor.as_ref());
                    let base = CURSOR_MIN_REFETCH_SECS as u64;
                    CURSOR_REFETCH_SECS
                        .store(cursor_idle_backoff(floor, base, changed), Ordering::Relaxed);
                    Some(rate_limits)
                }
                Err(error) => {
                    tracing::warn!(error = %error.message, "Cursor rate-limit fetch failed");
                    Some(provider_rate_limit_error("cursor", error))
                }
            }
        }
        .await;
        (result, provider_t0.elapsed())
    };

    let kimi_future = async {
        let provider_t0 = std::time::Instant::now();
        let result = async {
            if !selection.includes_kimi() {
                return cached_kimi;
            }

            let now = Utc::now();
            if is_fresh(
                cached_kimi.as_ref(),
                KIMI_MIN_REFETCH_SECS,
                now,
                retry_errors,
            ) {
                return cached_kimi;
            }

            // Honor a 429 cooldown: `is_fresh` holds an error payload back for
            // only ERROR_RETRY_SECS, so a longer server cooldown needs this too.
            if let Some(rate_limits) = cached_kimi.clone() {
                if provider_cooldown_is_active(&rate_limits, now) {
                    return Some(mark_rate_limits_stale(rate_limits));
                }
            }

            match fetch_kimi_rate_limits().await {
                Ok(rate_limits) => Some(rate_limits),
                Err(error) if error.message == kimi::NOT_SIGNED_IN => {
                    tracing::debug!("Kimi Code CLI is not signed in; no Kimi rate limits");
                    Some(provider_rate_limit_error("kimi", error))
                }
                Err(error) => {
                    tracing::warn!(error = %error.message, "Kimi rate-limit fetch failed");
                    Some(provider_rate_limit_error("kimi", error))
                }
            }
        }
        .await;
        (result, provider_t0.elapsed())
    };

    let past_deadline = || deadline.is_some_and(|at| std::time::Instant::now() >= at);
    let kept = |cached: Option<ProviderRateLimits>| (cached, std::time::Duration::ZERO);
    let (claude, claude_took) = if past_deadline() {
        kept(cached.and_then(|payload| payload.claude.clone()))
    } else {
        claude_future.await
    };
    let (codex, codex_took) = if past_deadline() {
        kept(cached.and_then(|payload| payload.codex.clone()))
    } else {
        codex_future.await
    };
    let (cursor, cursor_took) = if past_deadline() {
        kept(cached.and_then(|payload| payload.cursor.clone()))
    } else {
        cursor_future.await
    };
    let (kimi, kimi_took) = if past_deadline() {
        kept(cached.and_then(|payload| payload.kimi.clone()))
    } else {
        kimi_future.await
    };
    tracing::debug!(
        "[PROFILE] rate-limits: selection={selection:?} claude={claude_took:?} codex={codex_took:?} cursor={cursor_took:?} kimi={kimi_took:?}"
    );
    RateLimitsPayload {
        claude,
        codex,
        cursor,
        kimi,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: &str, utilization: f64, resets_at: Option<&str>) -> RateLimitWindow {
        RateLimitWindow::new(
            id.to_string(),
            id.to_string(),
            utilization,
            resets_at.map(str::to_string),
        )
    }

    fn limits(windows: Vec<RateLimitWindow>, fetched_at: &str) -> ProviderRateLimits {
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

    fn overlay_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn live_statusline_windows_overlay_the_cached_model_windows() {
        let base = limits(
            vec![
                window("five_hour", 10.0, Some("2026-09-23T15:00:00Z")),
                window("seven_day", 20.0, Some("2026-09-27T00:00:00Z")),
                window("seven_day_fable", 30.0, Some("2026-09-27T00:00:00Z")),
            ],
            "2026-09-23T11:00:00Z",
        );
        let live = limits(
            vec![
                window("five_hour", 12.0, Some("2026-09-23T15:00:00Z")),
                window("seven_day", 22.0, Some("2026-09-27T00:00:00Z")),
            ],
            "2026-09-23T11:59:00Z",
        );
        let merged = overlay_live_windows(Some(base), live, overlay_now());
        let ids: Vec<&str> = merged
            .windows
            .iter()
            .map(|w| w.window_id.as_str())
            .collect();
        assert_eq!(ids, ["five_hour", "seven_day", "seven_day_fable"]);
        let pct: Vec<f64> = merged.windows.iter().map(|w| w.utilization).collect();
        assert_eq!(pct, [12.0, 22.0, 30.0]);
        assert_eq!(merged.fetched_at, "2026-09-23T11:59:00Z");
    }

    #[test]
    fn model_windows_that_already_reset_are_not_carried_forward() {
        let base = limits(
            vec![window(
                "seven_day_fable",
                90.0,
                Some("2026-09-23T00:00:00Z"),
            )],
            "2026-09-22T00:00:00Z",
        );
        let live = limits(vec![window("five_hour", 1.0, None)], "2026-09-23T11:59:00Z");
        let merged = overlay_live_windows(Some(base), live, overlay_now());
        let ids: Vec<&str> = merged
            .windows
            .iter()
            .map(|w| w.window_id.as_str())
            .collect();
        assert_eq!(ids, ["five_hour"]);
    }

    #[test]
    fn plan_details_come_from_the_richer_source() {
        let mut base = limits(vec![], "2026-09-23T11:00:00Z");
        base.plan_tier = Some("max20x".to_string());
        let live = limits(vec![window("five_hour", 1.0, None)], "2026-09-23T11:59:00Z");
        let merged = overlay_live_windows(Some(base), live, overlay_now());
        assert_eq!(merged.plan_tier.as_deref(), Some("max20x"));
    }

    #[test]
    fn without_a_cache_the_live_windows_stand_alone() {
        let live = limits(vec![window("five_hour", 1.0, None)], "2026-09-23T11:59:00Z");
        let merged = overlay_live_windows(None, live.clone(), overlay_now());
        assert_eq!(merged.windows.len(), 1);
        assert_eq!(merged.fetched_at, live.fetched_at);
    }

    #[test]
    fn model_windows_refresh_is_due_at_first_and_after_the_interval() {
        let now = Instant::now();
        let every = CLAUDE_MODEL_WINDOWS_REFRESH_SECS;
        assert!(refresh_due(None, now, every));
        assert!(!refresh_due(Some(now), now, every));
        let at = |secs: u64| now + std::time::Duration::from_secs(secs);
        // A tick that lands a moment short of the interval still counts.
        assert!(refresh_due(Some(now), at(every - 1), every));
        assert!(!refresh_due(Some(now), at(every - 60), every));
    }

    #[test]
    fn a_running_oauth_cooldown_survives_the_overlay() {
        let now = overlay_now();
        let mut base = limits(vec![], "2026-09-23T11:00:00Z");
        base.cooldown_until = Some("2026-09-23T12:55:00Z".to_string());
        base.retry_after_seconds = Some(3600);
        let live = limits(vec![window("five_hour", 1.0, None)], "2026-09-23T11:59:00Z");
        let merged = overlay_live_windows(Some(base.clone()), live.clone(), now);
        assert!(oauth_cooldown_hold(Some(&merged), now).is_some());

        base.cooldown_until = Some("2026-09-23T11:30:00Z".to_string());
        let merged = overlay_live_windows(Some(base), live, now);
        assert_eq!(merged.cooldown_until, None, "a lapsed cooldown is dropped");
    }

    #[test]
    fn enabled_selection_probes_only_the_listed_providers() {
        let selection =
            RateLimitSelection::enabled(&[UsageIntegrationId::Claude, UsageIntegrationId::Codex]);
        assert!(selection.includes_claude());
        assert!(selection.includes_codex());
        assert!(!selection.includes_cursor());
        assert!(!selection.includes_kimi());
    }

    #[test]
    fn enabled_selection_with_nothing_enabled_probes_nobody() {
        let selection = RateLimitSelection::enabled(&[]);
        assert!(!selection.includes_claude());
        assert!(!selection.includes_codex());
        assert!(!selection.includes_cursor());
        assert!(!selection.includes_kimi());
    }

    #[test]
    fn enabled_selection_with_every_integration_probes_everyone() {
        let selection = RateLimitSelection::enabled(&[
            UsageIntegrationId::Claude,
            UsageIntegrationId::Codex,
            UsageIntegrationId::Cursor,
            UsageIntegrationId::Kimi,
        ]);
        assert!(selection.includes_claude());
        assert!(selection.includes_codex());
        assert!(selection.includes_cursor());
        assert!(selection.includes_kimi());
    }
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
    fn humanize_snake_case_title_cases_parts() {
        assert_eq!(humanize_snake_case("bonus_pool"), "Bonus Pool");
        assert_eq!(humanize_snake_case("five-hour"), "Five Hour");
    }

    #[test]
    fn is_fresh_returns_true_when_within_window_and_has_data() {
        let now = Utc::now();
        let recent = make_provider_with_windows(
            &(now - Duration::seconds(60)).to_rfc3339(),
            vec![sample_window()],
        );
        assert!(is_fresh(Some(&recent), 300, now, false));
    }

    #[test]
    fn is_fresh_returns_false_when_expired() {
        let now = Utc::now();
        let old = make_provider_with_windows(
            &(now - Duration::seconds(600)).to_rfc3339(),
            vec![sample_window()],
        );
        assert!(!is_fresh(Some(&old), 300, now, false));
    }

    #[test]
    fn is_fresh_returns_false_when_no_cache() {
        assert!(!is_fresh(None, 300, Utc::now(), false));
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
    fn is_fresh_expires_a_reading_the_slack_before_its_floor() {
        // A 300 s floor must re-probe on every 300 s tick, whose reading is a
        // few seconds younger than 300 s when the next tick comes round.
        let now = Utc::now();
        let at = |age: i64| {
            make_provider_with_windows(
                &(now - Duration::seconds(age)).to_rfc3339(),
                vec![sample_window()],
            )
        };
        assert!(!is_fresh(Some(&at(290)), 300, now, false));
        assert!(is_fresh(Some(&at(280)), 300, now, false));
    }

    #[test]
    fn is_fresh_backs_off_an_error_only_payload() {
        let now = Utc::now();
        let at = |age: i64| {
            let mut error_only =
                make_provider_with_windows(&(now - Duration::seconds(age)).to_rfc3339(), vec![]);
            error_only.error = Some("Claude CLI /usage probe failed".to_string());
            error_only
        };
        assert!(is_fresh(Some(&at(60)), 300, now, false));
        assert!(!is_fresh(Some(&at(130)), 300, now, false));
    }

    #[test]
    fn a_user_retry_probes_a_failed_provider_that_kept_its_old_windows() {
        let now = Utc::now();
        let fetched_at = (now - Duration::seconds(60)).to_rfc3339();
        // A probe failed and the merge kept the previous reading's windows.
        let mut failed = make_provider_with_windows(&fetched_at, vec![sample_window()]);
        failed.stale = true;
        failed.error = Some("Codex app-server probe failed".to_string());

        assert!(
            !is_fresh(Some(&failed), 300, now, true),
            "a user retry probes it at once"
        );
        assert!(
            is_fresh(Some(&failed), 300, now, false),
            "the refresh keeps its floor"
        );
        let good = make_provider_with_windows(&fetched_at, vec![sample_window()]);
        assert!(
            is_fresh(Some(&good), 300, now, true),
            "a good reading keeps its floor on a retry too"
        );
    }

    #[tokio::test]
    async fn providers_reached_after_the_deadline_keep_their_cached_value() {
        // Every reading is long past its floor: without the deadline each
        // provider would be probed.
        let old = (Utc::now() - Duration::hours(2)).to_rfc3339();
        let reading = |provider: &str| {
            let mut reading = make_provider_with_windows(&old, vec![sample_window()]);
            reading.provider = provider.to_string();
            Some(reading)
        };
        let cached = RateLimitsPayload {
            claude: reading("claude"),
            codex: reading("codex"),
            cursor: reading("cursor"),
            kimi: reading("kimi"),
        };
        let every = RateLimitSelection::enabled(&[
            UsageIntegrationId::Claude,
            UsageIntegrationId::Codex,
            UsageIntegrationId::Cursor,
            UsageIntegrationId::Kimi,
        ]);
        let codex_dir = tempfile::TempDir::new().unwrap();

        let passed = std::time::Instant::now();
        let fresh = fetch_selected_rate_limits_until(
            codex_dir.path(),
            every,
            Some(&cached),
            Some(passed),
            false,
        )
        .await;

        let json = |payload: &RateLimitsPayload| serde_json::to_value(payload).unwrap();
        assert_eq!(json(&fresh), json(&cached));
    }

    #[test]
    fn a_logged_codex_reading_stands_in_only_when_newer_recent_and_unreset() {
        let now = Utc::now();
        let reading = |age: i64, resets_in: i64| {
            make_provider_with_windows(
                &(now - Duration::seconds(age)).to_rfc3339(),
                vec![RateLimitWindow::new(
                    "primary".into(),
                    "Session (5hr)".into(),
                    5.0,
                    Some((now + Duration::seconds(resets_in)).to_rfc3339()),
                )],
            )
        };
        let probed = reading(400, 3600);
        let stands_in = |logged, cached| stands_in_for_probe(&logged, cached, now);
        assert!(
            stands_in(reading(60, 3600), Some(&probed)),
            "logged since the probe"
        );
        assert!(stands_in(reading(60, 3600), None), "nothing probed yet");
        assert!(
            !stands_in(reading(500, 3600), None),
            "older than a kept probe"
        );
        let later = reading(100, 3600);
        assert!(!stands_in(reading(200, 3600), Some(&later)), "not newer");
        assert!(
            !stands_in(reading(60, -10), Some(&probed)),
            "a window reset since"
        );
    }

    #[test]
    fn an_explicit_retry_skips_only_the_error_back_off() {
        let now = Utc::now();
        let fetched_at = (now - Duration::seconds(60)).to_rfc3339();
        let mut error_only = make_provider_with_windows(&fetched_at, vec![]);
        error_only.error = Some("Claude CLI /usage probe failed".to_string());
        assert!(!is_fresh(Some(&error_only), 300, now, true));

        let reading = make_provider_with_windows(&fetched_at, vec![sample_window()]);
        assert!(is_fresh(Some(&reading), 300, now, true));
    }

    #[test]
    fn a_logged_codex_reading_keeps_the_probed_usage_limit_resets() {
        use crate::models::{CreditsInfo, UsageLimitResets};
        let credits = |balance: f64, resets: Option<u32>| CreditsInfo {
            balance: Some(balance),
            has_credits: true,
            unlimited: false,
            usage_limit_resets: resets.map(|available| UsageLimitResets {
                available,
                resets: vec![],
            }),
        };
        let mut probed = make_provider_with_windows("2026-09-25T10:00:00Z", vec![]);
        probed.credits = Some(credits(100.0, Some(2)));
        let mut logged = make_provider_with_windows("2026-09-25T10:04:00Z", vec![]);
        logged.credits = Some(credits(90.0, None));

        let kept = keep_probed_resets(logged.clone(), Some(&probed))
            .credits
            .unwrap();
        assert_eq!(kept.balance, Some(90.0), "the logged balance is newer");
        assert_eq!(kept.usage_limit_resets.map(|r| r.available), Some(2));

        logged.credits = None;
        let kept = keep_probed_resets(logged.clone(), Some(&probed));
        assert_eq!(kept.credits.unwrap().balance, Some(100.0));
        assert!(keep_probed_resets(logged, None).credits.is_none());
    }
}
