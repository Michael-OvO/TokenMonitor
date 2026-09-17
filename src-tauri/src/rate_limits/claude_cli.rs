//! Claude rate limits via Claude Code's own `/usage` slash command.
//!
//! Mirrors the Codex app-server approach: instead of calling Anthropic's API
//! ourselves with a borrowed OAuth token, we spawn the vendor CLI and let it
//! answer. Claude Code owns the credentials, the token refresh, and any
//! rate-limit retry — we only parse the two lines it prints.
//!
//! `claude -p "/usage"` runs with `num_turns: 0` and `total_cost_usd: 0` — it
//! is a local slash command, not an inference call.

use crate::models::{ProviderRateLimits, RateLimitWindow};
use chrono::{DateTime, Datelike, Local, TimeZone};
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

use super::{claude::claude_window_label, RateLimitFetchError};

static CACHED_CLAUDE_CLI_PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();

const CLAUDE_CLI_PATH_ENV: &str = "CLAUDE_CLI_PATH";
const CLAUDE_USAGE_TIMEOUT_SECONDS: u64 = 30;

/// Fixed session id so repeated probes reuse one transcript instead of
/// littering `~/.claude/projects` with an empty session per poll. Claude Code
/// rejects `--session-id` once the session exists, so the steady-state call is
/// `--resume` and `--session-id` only runs on the very first probe.
const USAGE_SESSION_ID: &str = "7c9e6a1d-4b3f-4a2e-8d51-746f6b656e6d";

// ── CLI path resolution ──

fn common_claude_cli_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(home) = dirs::home_dir() {
        #[cfg(target_os = "windows")]
        {
            candidates.push(home.join(".local").join("bin").join("claude.exe"));
            if let Ok(appdata) = std::env::var("APPDATA") {
                let appdata = PathBuf::from(appdata);
                candidates.push(appdata.join("npm").join("claude.cmd"));
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            candidates.push(home.join(".local").join("bin").join("claude"));
            candidates.push(home.join(".claude").join("local").join("claude"));
            candidates.push(PathBuf::from("/opt/homebrew/bin/claude"));
            candidates.push(PathBuf::from("/usr/local/bin/claude"));
        }
    }

    candidates
}

pub(crate) fn resolve_claude_cli_path() -> Result<PathBuf, String> {
    CACHED_CLAUDE_CLI_PATH
        .get_or_init(resolve_claude_cli_path_uncached)
        .clone()
}

fn resolve_claude_cli_path_uncached() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(CLAUDE_CLI_PATH_ENV).map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
    }

    if let Some(path) = super::command_in_path("claude") {
        return Ok(path);
    }

    common_claude_cli_paths()
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "Claude CLI was not found on this system".to_string())
}

// ── Output parsing ──

/// Map a `/usage` row label onto a Claude window id. Rows that are neither a
/// session nor a weekly pool are dropped rather than guessed.
///
/// The per-model suffix is derived from the parenthetical rather than matched
/// against a fixed list: `Current week (Opus)` → `seven_day_opus`, so a newly
/// launched model gets its own bar without a TokenMonitor release. Hard-coding
/// the known models is what made `Current week (Fable)` collapse onto the plain
/// `seven_day` id and collide with the all-models row.
fn window_id_for_label(label: &str) -> Option<String> {
    let label = label.to_ascii_lowercase();
    if label.contains("session") {
        return Some("five_hour".to_string());
    }
    if !label.contains("week") {
        return None;
    }

    let qualifier = label
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(qualifier, _)| qualifier.trim())
        .unwrap_or_default();
    // "all models" is the aggregate pool, which the API calls plain `seven_day`.
    if qualifier.is_empty() || qualifier == "all models" {
        return Some("seven_day".to_string());
    }
    Some(format!("seven_day_{}", snake_case(qualifier)))
}

/// Lowercase a `/usage` qualifier into the API's field-name style, so CLI ids
/// match the ones the OAuth payload uses (`OAuth apps` → `oauth_apps`).
fn snake_case(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn month_from_abbrev(raw: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let lower = raw.to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|m| lower.starts_with(m))
        .map(|idx| idx as u32 + 1)
}

/// Parse `11:59pm` / `12am` into (hour24, minute).
fn parse_clock(raw: &str) -> Option<(u32, u32)> {
    let raw = raw.trim().to_ascii_lowercase();
    let (time, pm) = if let Some(rest) = raw.strip_suffix("pm") {
        (rest, true)
    } else {
        (raw.strip_suffix("am")?, false)
    };

    let (hour, minute) = match time.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (time.parse::<u32>().ok()?, 0),
    };
    if hour > 12 || minute > 59 {
        return None;
    }

    // 12am is midnight, 12pm is noon; everything else shifts by 12 for pm.
    let hour24 = match (hour, pm) {
        (12, false) => 0,
        (12, true) => 12,
        (h, true) => h + 12,
        (h, false) => h,
    };
    Some((hour24, minute))
}

/// Parse `Aug 9 at 2:20am (America/New_York)` into an absolute timestamp.
///
/// Claude Code used to separate the date and the clock with `, ` and now uses
/// ` at `. Both are accepted so the parser keeps working against whichever
/// wording the installed CLI emits.
///
/// ponytail: the IANA zone in parentheses is ignored and the machine's local
/// zone is used instead — resolving it properly would mean pulling in
/// `chrono-tz` for one field. Correct whenever the account timezone matches the
/// machine, which is the normal case; a mismatch skews `resets_at` by the zone
/// offset. Add `chrono-tz` if that ever matters.
fn parse_reset_time(raw: &str, now: DateTime<Local>) -> Option<DateTime<Local>> {
    let raw = raw.split(" (").next()?.trim();
    let (date_part, clock_part) = raw.split_once(" at ").or_else(|| raw.split_once(", "))?;

    let mut date_tokens = date_part.split_whitespace();
    let month = month_from_abbrev(date_tokens.next()?)?;
    let day: u32 = date_tokens.next()?.trim_end_matches(',').parse().ok()?;
    let (hour, minute) = parse_clock(clock_part)?;

    // `/usage` prints no year. Resets are always ahead of now, so assume the
    // current year and roll forward when that lands in the past (Dec → Jan).
    let candidate = Local
        .with_ymd_and_hms(now.year(), month, day, hour, minute, 0)
        .single()?;
    if candidate < now - chrono::Duration::days(1) {
        return Local
            .with_ymd_and_hms(now.year() + 1, month, day, hour, minute, 0)
            .single();
    }
    Some(candidate)
}

/// Extract rate-limit windows from `/usage` stdout.
///
/// Target rows look like:
/// ```text
/// Current session: 28% used · resets Aug 5, 11:59pm (America/New_York)
/// Current week (all models): 17% used · resets Aug 10, 11:59pm (America/New_York)
/// ```
/// Everything else in the output (cost summary, usage-behavior tips) is
/// ignored. Rows without a `resets` clause still produce a window with a null
/// reset time rather than being dropped.
pub(super) fn parse_usage_output(output: &str, now: DateTime<Local>) -> Vec<RateLimitWindow> {
    let mut windows = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        // The label separator is ": "; the colon inside "11:59pm" has no
        // trailing space, so this split can't land on the clock.
        let Some((label, rest)) = line.split_once(": ") else {
            continue;
        };
        let Some((percent_raw, after_percent)) = rest.split_once('%') else {
            continue;
        };
        if !after_percent.trim_start().starts_with("used") {
            continue;
        }
        let Ok(utilization) = percent_raw.trim().parse::<f64>() else {
            continue;
        };
        let Some(window_id) = window_id_for_label(label) else {
            continue;
        };

        let resets_at = after_percent
            .split_once("resets ")
            .and_then(|(_, raw)| parse_reset_time(raw, now))
            .map(|dt| dt.to_rfc3339());

        windows.push(RateLimitWindow::new(
            window_id.clone(),
            claude_window_label(&window_id),
            utilization,
            resets_at,
        ));
    }

    windows
}

// ── Probe ──

async fn run_usage_command(cli: &PathBuf, resume: bool) -> Result<String, RateLimitFetchError> {
    let mut command = TokioCommand::new(cli);
    command.kill_on_drop(true);
    command.arg("-p").arg("/usage");
    if resume {
        command.arg("--resume").arg(USAGE_SESSION_ID);
    } else {
        command.arg("--session-id").arg(USAGE_SESSION_ID);
    }
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::piped());
    // Captured, not discarded: a bad session id or a logged-out CLI only
    // explains itself on stderr, and swallowing it turns every failure into
    // an indistinguishable "no recognizable rows".
    command.stderr(std::process::Stdio::piped());

    // Run from home so the probe's transcript always lands in the same
    // project directory regardless of where TokenMonitor was started.
    if let Some(home) = dirs::home_dir() {
        command.current_dir(home);
    }

    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let output = timeout(
        std::time::Duration::from_secs(CLAUDE_USAGE_TIMEOUT_SECONDS),
        command.output(),
    )
    .await
    .map_err(|_| RateLimitFetchError::message("Claude CLI /usage probe timed out"))?
    .map_err(|e| RateLimitFetchError::message(format!("Failed to run Claude CLI: {e}")))?;

    if !output.stderr.is_empty() {
        tracing::debug!(
            resume,
            stderr = %String::from_utf8_lossy(&output.stderr).trim(),
            "Claude CLI /usage stderr"
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) async fn fetch_claude_rate_limits_via_cli(
) -> Result<ProviderRateLimits, RateLimitFetchError> {
    let cli = resolve_claude_cli_path().map_err(RateLimitFetchError::message)?;

    // Steady state is `--resume`; the fallback covers the first-ever probe and
    // the case where the user cleaned out `~/.claude/projects`.
    let mut stdout = run_usage_command(&cli, true).await?;
    let mut windows = parse_usage_output(&stdout, Local::now());
    if windows.is_empty() {
        stdout = run_usage_command(&cli, false).await?;
        windows = parse_usage_output(&stdout, Local::now());
    }

    if windows.is_empty() {
        return Err(RateLimitFetchError::message(
            "Claude CLI /usage returned no recognizable rate-limit rows",
        ));
    }

    Ok(ProviderRateLimits {
        provider: "claude".to_string(),
        plan_tier: None,
        windows,
        extra_usage: None,
        credits: None,
        stale: false,
        error: None,
        retry_after_seconds: None,
        cooldown_until: None,
        fetched_at: Local::now().to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 8, 5, 23, 30, 0)
            .single()
            .unwrap()
    }

    /// Verbatim `claude -p "/usage"` output, captured 2026-08-08. Keep it a
    /// real capture: the reset times silently became `None` for weeks because
    /// this sample still carried the older `Aug 5, 11:59pm` wording that the
    /// CLI had already stopped emitting.
    const SAMPLE: &str = "\
You are currently using your subscription to power your Claude Code usage

Current session: 2% used · resets Aug 9 at 2:20am (America/New_York)
Current week (all models): 55% used · resets Aug 11 at 12am (America/New_York)
Current week (Fable): 67% used · resets Aug 10 at 11:59pm (America/New_York)

What's contributing to your limits usage?
Last 24h · 827 requests · 8 sessions
  90% of your usage came from subagent-heavy sessions
";

    #[test]
    fn parses_every_usage_row_with_its_reset_time() {
        let windows = parse_usage_output(SAMPLE, now());

        assert_eq!(windows.len(), 3, "tip lines must not become windows");
        assert_eq!(windows[0].window_id, "five_hour");
        assert_eq!(windows[0].label, "Session (5hr)");
        assert_eq!(windows[0].utilization, 2.0);
        assert!(windows[0]
            .resets_at
            .as_ref()
            .unwrap()
            .starts_with("2026-08-09T02:20:00"));

        assert_eq!(windows[1].window_id, "seven_day");
        assert_eq!(windows[1].utilization, 55.0);
        assert!(windows[1]
            .resets_at
            .as_ref()
            .unwrap()
            .starts_with("2026-08-11T00:00:00"));

        assert_eq!(windows[2].window_id, "seven_day_fable");
        assert_eq!(windows[2].label, "Weekly Fable");
        assert!(windows[2]
            .resets_at
            .as_ref()
            .unwrap()
            .starts_with("2026-08-10T23:59:00"));
    }

    #[test]
    fn every_row_carries_a_reset_time() {
        for window in parse_usage_output(SAMPLE, now()) {
            assert!(
                window.resets_at.is_some(),
                "{} lost its reset time — has the /usage wording changed again?",
                window.window_id
            );
        }
    }

    #[test]
    fn each_weekly_pool_gets_a_distinct_id() {
        let windows = parse_usage_output(SAMPLE, now());
        let ids: std::collections::HashSet<_> =
            windows.iter().map(|w| w.window_id.as_str()).collect();
        assert_eq!(ids.len(), windows.len(), "window ids collided: {ids:?}");
    }

    /// The pre-2026-08 wording. Users on an older Claude Code still emit it.
    #[test]
    fn parses_the_legacy_comma_separated_reset_time() {
        let windows = parse_usage_output(
            "Current session: 28% used · resets Aug 5, 11:59pm (America/New_York)",
            now(),
        );
        assert_eq!(windows.len(), 1);
        assert!(windows[0]
            .resets_at
            .as_ref()
            .unwrap()
            .starts_with("2026-08-05T23:59:00"));
    }

    #[test]
    fn parses_midnight_without_minutes() {
        let windows = parse_usage_output(
            "Current session: 22% used · resets Aug 6 at 12am (America/New_York)",
            now(),
        );
        assert_eq!(windows.len(), 1);
        assert!(windows[0]
            .resets_at
            .as_ref()
            .unwrap()
            .starts_with("2026-08-06T00:00:00"));
    }

    #[test]
    fn maps_per_model_weekly_pools() {
        for (qualifier, expected) in [
            ("Opus", "seven_day_opus"),
            ("Sonnet", "seven_day_sonnet"),
            ("Fable", "seven_day_fable"),
            // Not a model we ship a label for — the id is still derived, so
            // the pool surfaces instead of colliding with `seven_day`.
            ("OAuth apps", "seven_day_oauth_apps"),
        ] {
            let windows = parse_usage_output(
                &format!(
                    "Current week ({qualifier}): 9% used · resets Aug 10 at 11:59pm (America/New_York)"
                ),
                now(),
            );
            assert_eq!(windows.len(), 1, "{qualifier} row was dropped");
            assert_eq!(windows[0].window_id, expected);
        }
    }

    #[test]
    fn rolls_the_year_forward_across_new_year() {
        let december = Local
            .with_ymd_and_hms(2026, 12, 31, 23, 0, 0)
            .single()
            .unwrap();
        let windows = parse_usage_output(
            "Current session: 5% used · resets Jan 1 at 12am (America/New_York)",
            december,
        );
        assert!(windows[0]
            .resets_at
            .as_ref()
            .unwrap()
            .starts_with("2027-01-01T00:00:00"));
    }

    #[test]
    fn ignores_output_without_usage_rows() {
        assert!(parse_usage_output("Error: not logged in\n", now()).is_empty());
        assert!(parse_usage_output("", now()).is_empty());
    }

    /// Live probe against the real CLI. Ignored by default because it needs a
    /// logged-in Claude Code on the machine; run with
    /// `cargo test --lib claude_cli -- --ignored --nocapture` after touching
    /// the spawn logic or when CC changes its `/usage` wording.
    #[tokio::test]
    #[ignore]
    async fn live_cli_probe_returns_windows() {
        let rate_limits = fetch_claude_rate_limits_via_cli()
            .await
            .expect("CLI probe failed");
        assert!(!rate_limits.windows.is_empty());
        for window in &rate_limits.windows {
            println!(
                "{} = {}% resets {:?}",
                window.window_id, window.utilization, window.resets_at
            );
        }

        // Printing alone let a `/usage` wording change blank every reset time
        // without failing anything. Assert the shape the UI actually needs.
        let mut seen = std::collections::HashSet::new();
        for window in &rate_limits.windows {
            assert!(
                window.resets_at.is_some(),
                "{} has no reset time — the /usage wording likely changed",
                window.window_id
            );
            assert!(
                seen.insert(window.window_id.as_str()),
                "duplicate window id {}",
                window.window_id
            );
        }
    }

    #[test]
    fn parse_clock_handles_noon_and_midnight() {
        assert_eq!(parse_clock("12am"), Some((0, 0)));
        assert_eq!(parse_clock("12pm"), Some((12, 0)));
        assert_eq!(parse_clock("1pm"), Some((13, 0)));
        assert_eq!(parse_clock("11:59pm"), Some((23, 59)));
        assert_eq!(parse_clock("garbage"), None);
    }
}
