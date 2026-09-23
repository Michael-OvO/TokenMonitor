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
use chrono::{DateTime, Duration, Utc};
use std::path::{Path, PathBuf};
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
    let session = statusline::source::latest_active_session(&statusline::events_file())
        .ok()
        .flatten()?;

    if !session.is_fresh(STATUSLINE_FRESHNESS, Utc::now()) {
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
use codex::extract_codex_rate_limits;
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
/// Cursor's probe is the dearest of the four (a `sqlite3` spawn to read the
/// IDE's token, then an HTTPS round trip), and until it had this gate it ran
/// on every statusline event, several times a minute while Claude Code works.
const CURSOR_MIN_REFETCH_SECS: i64 = 300;
/// Claude Code's statusline only reports the plan-wide 5h and 7d windows. The
/// model-specific weekly windows (Weekly Fable, Weekly Opus, ...) exist only in
/// the CLI probe and the OAuth API, so while the statusline is live those are
/// refreshed on this cadence and carried forward in between.
const CLAUDE_MODEL_WINDOWS_REFRESH_SECS: u64 = 900;

/// When the model-specific Claude windows were last probed (or attempted).
static CLAUDE_MODEL_WINDOWS_REFRESHED_AT: Mutex<Option<Instant>> = Mutex::new(None);

fn last_model_windows_refresh() -> Option<Instant> {
    *CLAUDE_MODEL_WINDOWS_REFRESHED_AT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn mark_model_windows_refreshed(now: Instant) {
    *CLAUDE_MODEL_WINDOWS_REFRESHED_AT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(now);
}

/// Whether the model-specific windows should be probed again: never done yet,
/// or the refresh interval has passed since the last attempt.
fn model_windows_refresh_due(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|at| now.duration_since(at).as_secs() >= CLAUDE_MODEL_WINDOWS_REFRESH_SECS)
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
/// has not already reset is kept, and plan details stay with the base. The
/// result is timestamped by the live source.
fn overlay_live_windows(
    base: Option<ProviderRateLimits>,
    live: ProviderRateLimits,
    now: DateTime<Utc>,
) -> ProviderRateLimits {
    let Some(base) = base else {
        return live;
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

pub async fn fetch_selected_rate_limits(
    codex_dir: &Path,
    selection: RateLimitSelection,
    cached: Option<&RateLimitsPayload>,
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
                let base =
                    if model_windows_refresh_due(last_model_windows_refresh(), Instant::now()) {
                        // Mark the attempt whatever it yields, so a failing probe
                        // retries on the same cadence instead of every event.
                        mark_model_windows_refreshed(Instant::now());
                        let rich = probe_claude_rich(cached_claude.as_ref(), now).await;
                        if rich.windows.is_empty() {
                            cached_claude.clone()
                        } else {
                            Some(rich)
                        }
                    } else {
                        cached_claude.clone()
                    };
                return Some(overlay_live_windows(base, live, now));
            }

            // Shared throttle for the remaining paths: a 5-minute floor keeps us
            // from spawning a CLI (or spending API budget) every 2.5 min.
            if is_fresh(cached_claude.as_ref(), CLAUDE_MIN_REFETCH_SECS, now) {
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
            if is_fresh(cached_cursor.as_ref(), CURSOR_MIN_REFETCH_SECS, now) {
                return cached_cursor;
            }

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
            if is_fresh(cached_kimi.as_ref(), KIMI_MIN_REFETCH_SECS, now) {
                return cached_kimi;
            }

            // Honor a 429 cooldown: error payloads have no windows, so `is_fresh`
            // never trips and we'd otherwise re-hit the API every refresh cycle.
            if let Some(rate_limits) = cached_kimi.clone() {
                if provider_cooldown_is_active(&rate_limits, now) {
                    return Some(mark_rate_limits_stale(rate_limits));
                }
            }

            match fetch_kimi_rate_limits().await {
                Ok(rate_limits) => Some(rate_limits),
                Err(error) => {
                    tracing::warn!(error = %error.message, "Kimi rate-limit fetch failed");
                    Some(provider_rate_limit_error("kimi", error))
                }
            }
        }
        .await;
        (result, provider_t0.elapsed())
    };

    let ((claude, claude_took), (codex, codex_took), (cursor, cursor_took), (kimi, kimi_took)) =
        tokio::join!(claude_future, codex_future, cursor_future, kimi_future);
    tracing::info!(
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
        assert!(model_windows_refresh_due(None, now));
        assert!(!model_windows_refresh_due(Some(now), now));
        let later = now + std::time::Duration::from_secs(CLAUDE_MODEL_WINDOWS_REFRESH_SECS);
        assert!(model_windows_refresh_due(Some(now), later));
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
