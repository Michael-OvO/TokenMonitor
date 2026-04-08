use crate::models::{ProviderRateLimits, RateLimitWindow};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::sync::OnceLock;
use std::time::Duration as StdDuration;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

use super::RateLimitFetchError;

static CACHED_CLI_PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// Windows: CREATE_NO_WINDOW flag prevents a console window from flashing.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const CLAUDE_CLI_PROBE_TIMEOUT_SECONDS: u64 = 20;
const CLAUDE_CLI_PROBE_PROMPT: &str = "Respond with OK only.";
const CLAUDE_CLI_PATH_ENV: &str = "CLAUDE_CLI_PATH";
#[cfg(not(target_os = "windows"))]
const CLAUDE_CLI_RESOLVE_COMMAND: &str = "command -v claude";

#[derive(Debug, Deserialize)]
pub(crate) struct ClaudeCliStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub rate_limit_info: Option<ClaudeCliRateLimitInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeCliRateLimitInfo {
    pub status: String,
    pub resets_at: Option<i64>,
    pub rate_limit_type: Option<String>,
    pub utilization: Option<f64>,
}

fn command_in_path(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
        // Windows: also check .cmd and .exe extensions
        #[cfg(target_os = "windows")]
        {
            let cmd = dir.join(format!("{binary}.cmd"));
            if cmd.is_file() {
                return Some(cmd);
            }
            let exe = dir.join(format!("{binary}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

fn common_claude_cli_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // Unix-only well-known paths
    #[cfg(not(target_os = "windows"))]
    {
        candidates.push(PathBuf::from("/opt/homebrew/bin/claude"));
        candidates.push(PathBuf::from("/usr/local/bin/claude"));
        candidates.push(PathBuf::from("/usr/bin/claude"));
    }

    if let Some(home) = dirs::home_dir() {
        #[cfg(not(target_os = "windows"))]
        {
            candidates.push(home.join(".local").join("bin").join("claude"));
            candidates.push(home.join(".npm-global").join("bin").join("claude"));
        }

        #[cfg(target_os = "windows")]
        {
            // npm global installs on Windows
            if let Ok(appdata) = std::env::var("APPDATA") {
                let appdata = PathBuf::from(appdata);
                candidates.push(appdata.join("npm").join("claude.cmd"));
                candidates.push(appdata.join("npm").join("claude"));
            }
            // pnpm global
            if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
                let localappdata = PathBuf::from(localappdata);
                candidates.push(localappdata.join("pnpm").join("claude.cmd"));
                candidates.push(localappdata.join("pnpm").join("claude"));
            }
        }

        // nvm versions directory
        #[cfg(not(target_os = "windows"))]
        let nvm_dir = home.join(".nvm").join("versions").join("node");
        #[cfg(target_os = "windows")]
        let nvm_dir = std::env::var("NVM_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("APPDATA")
                    .map(|a| PathBuf::from(a).join("nvm"))
                    .unwrap_or_else(|_| home.join("AppData").join("Roaming").join("nvm"))
            });

        if let Ok(entries) = std::fs::read_dir(nvm_dir) {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect();
            versions.sort_unstable_by(|a, b| b.cmp(a));
            for version in versions {
                #[cfg(not(target_os = "windows"))]
                candidates.push(version.join("bin").join("claude"));
                #[cfg(target_os = "windows")]
                {
                    candidates.push(version.join("claude.cmd"));
                    candidates.push(version.join("claude"));
                }
            }
        }
    }

    candidates
}

pub(crate) fn resolve_claude_cli_path() -> Result<PathBuf, String> {
    CACHED_CLI_PATH
        .get_or_init(resolve_claude_cli_path_uncached)
        .clone()
}

fn resolve_claude_cli_path_uncached() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(CLAUDE_CLI_PATH_ENV).map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
    }

    if let Some(path) = command_in_path("claude") {
        return Ok(path);
    }

    if let Some(path) = common_claude_cli_paths()
        .into_iter()
        .find(|candidate| candidate.is_file())
    {
        return Ok(path);
    }

    // Platform-specific shell fallback to resolve Claude CLI path
    #[cfg(target_os = "windows")]
    let output = {
        use std::os::windows::process::CommandExt;
        StdCommand::new("cmd.exe")
            .args(["/c", "where", "claude"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| format!("Failed to resolve Claude CLI path: {error}"))?
    };

    #[cfg(not(target_os = "windows"))]
    let output = StdCommand::new("/usr/bin/env")
        .args(["zsh", "-lc", CLAUDE_CLI_RESOLVE_COMMAND])
        .output()
        .or_else(|_| {
            // Fallback to bash if zsh is not available (Linux)
            StdCommand::new("/usr/bin/env")
                .args(["bash", "-lc", CLAUDE_CLI_RESOLVE_COMMAND])
                .output()
        })
        .map_err(|error| format!("Failed to resolve Claude CLI path: {error}"))?;

    if !output.status.success() {
        return Err("Claude CLI was not found on this system".to_string());
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("Invalid UTF-8 while resolving Claude CLI path: {error}"))?;

    stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .ok_or_else(|| "Claude CLI was not found on this system".to_string())
}

pub(crate) fn format_cli_timestamp(seconds: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(seconds, 0).map(|dt| dt.to_rfc3339())
}

pub(crate) fn retry_after_from_reset(reset_at: Option<&str>, now: DateTime<Utc>) -> Option<u64> {
    let reset_at = reset_at
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|dt| dt.with_timezone(&Utc))?;
    Some(reset_at.signed_duration_since(now).num_seconds().max(0) as u64)
}

fn claude_cli_window_spec(rate_limit_type: &str) -> Option<(&'static str, &'static str)> {
    match rate_limit_type {
        "five_hour" => Some(("five_hour", "Session (5hr)")),
        "seven_day" => Some(("seven_day", "Weekly (7 day)")),
        "seven_day_sonnet" => Some(("seven_day_sonnet", "Weekly Sonnet")),
        "seven_day_opus" => Some(("seven_day_opus", "Weekly Opus")),
        "seven_day_oauth_apps" => Some(("seven_day_oauth_apps", "Weekly OAuth Apps")),
        "seven_day_cowork" => Some(("seven_day_cowork", "Weekly Cowork")),
        _ => None,
    }
}

pub(crate) fn parse_claude_cli_rate_limit_info(
    output: &str,
) -> Result<ClaudeCliRateLimitInfo, String> {
    output
        .lines()
        .rev()
        .find_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let event = serde_json::from_str::<ClaudeCliStreamEvent>(line).ok()?;
            if event.event_type == "rate_limit_event" {
                event.rate_limit_info
            } else {
                None
            }
        })
        .ok_or_else(|| "Claude CLI did not emit a rate_limit_event".to_string())
}

pub(crate) fn rate_limits_from_claude_cli_info(
    info: ClaudeCliRateLimitInfo,
    cached: Option<&ProviderRateLimits>,
) -> Result<ProviderRateLimits, RateLimitFetchError> {
    let (window_id, label) = info
        .rate_limit_type
        .as_deref()
        .and_then(claude_cli_window_spec)
        .ok_or_else(|| {
            RateLimitFetchError::message("Claude CLI did not report a recognized rate limit window")
        })?;

    let reset_at = info.resets_at.and_then(format_cli_timestamp);
    let cached_window = cached.and_then(|payload| {
        payload
            .windows
            .iter()
            .find(|window| window.window_id == window_id)
    });

    let mut used_cached_window_data = cached
        .map(|payload| {
            payload
                .windows
                .iter()
                .any(|window| window.window_id != window_id)
        })
        .unwrap_or(false);

    // Determine utilization for this window.
    let utilization = match info.utilization {
        Some(utilization) => utilization,
        None => {
            if let Some(window) = cached_window {
                used_cached_window_data = true;
                window.utilization
            } else if info.status == "rejected" {
                100.0
            } else {
                // CLI returned "allowed" without a utilization value and no
                // cached window — the user simply has not consumed any quota
                // in this window yet, so report 0%.
                0.0
            }
        }
    };

    let mut windows = cached
        .map(|payload| payload.windows.clone())
        .unwrap_or_default();

    let next_window = RateLimitWindow::new(
        window_id.to_string(),
        label.to_string(),
        utilization,
        reset_at
            .clone()
            .or_else(|| cached_window.and_then(|window| window.resets_at.clone())),
    );

    if let Some(existing) = windows
        .iter_mut()
        .find(|window| window.window_id == window_id)
    {
        *existing = next_window;
    } else {
        windows.push(next_window);
    }

    let now = Utc::now();
    let cooldown_until = if info.status == "rejected" {
        reset_at.clone()
    } else {
        None
    };

    Ok(ProviderRateLimits {
        provider: "claude".to_string(),
        plan_tier: cached.and_then(|payload| payload.plan_tier.clone()),
        windows,
        extra_usage: cached.and_then(|payload| payload.extra_usage.clone()),
        stale: used_cached_window_data,
        error: None,
        retry_after_seconds: retry_after_from_reset(cooldown_until.as_deref(), now),
        cooldown_until,
        fetched_at: Local::now().to_rfc3339(),
    })
}

pub(super) async fn fetch_claude_rate_limits_via_cli(
    cached: Option<&ProviderRateLimits>,
) -> Result<ProviderRateLimits, RateLimitFetchError> {
    let cli_path = resolve_claude_cli_path().map_err(RateLimitFetchError::message)?;

    let mut command = TokioCommand::new(cli_path);
    command.kill_on_drop(true);
    command.args([
        "-p",
        "--verbose",
        "--output-format",
        "stream-json",
        "--model",
        "haiku",
        "--effort",
        "low",
        "--tools",
        "",
        "--disable-slash-commands",
        "--strict-mcp-config",
        "--setting-sources",
        "local",
        CLAUDE_CLI_PROBE_PROMPT,
    ]);

    // Prevent a console window from flashing on Windows
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = timeout(
        StdDuration::from_secs(CLAUDE_CLI_PROBE_TIMEOUT_SECONDS),
        command.output(),
    )
    .await
    .map_err(|_| RateLimitFetchError::message("Claude CLI fallback timed out"))?
    .map_err(|error| RateLimitFetchError::message(format!("Failed to run Claude CLI: {error}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        let message = if detail.is_empty() {
            format!("Claude CLI fallback failed with status {}", output.status)
        } else {
            format!("Claude CLI fallback failed: {detail}")
        };
        return Err(RateLimitFetchError::message(message));
    }

    let stdout = String::from_utf8(output.stdout).map_err(|error| {
        RateLimitFetchError::message(format!("Invalid Claude CLI output: {error}"))
    })?;
    let info = parse_claude_cli_rate_limit_info(&stdout).map_err(RateLimitFetchError::message)?;
    rate_limits_from_claude_cli_info(info, cached)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_cli_rate_limit_event_from_stream_json() {
        let output = concat!(
            "{\"type\":\"system\",\"subtype\":\"init\"}\n",
            "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed\",\"resetsAt\":1773781200,\"rateLimitType\":\"five_hour\",\"utilization\":42.5}}\n",
            "{\"type\":\"result\",\"subtype\":\"success\"}\n",
        );

        let info = parse_claude_cli_rate_limit_info(output).unwrap();

        assert_eq!(info.status, "allowed");
        assert_eq!(info.resets_at, Some(1_773_781_200));
        assert_eq!(info.rate_limit_type.as_deref(), Some("five_hour"));
        assert_eq!(info.utilization, Some(42.5));
    }

    fn provider_rate_limits(
        windows: Vec<RateLimitWindow>,
        error: Option<&str>,
        cooldown_until: Option<&str>,
    ) -> ProviderRateLimits {
        ProviderRateLimits {
            provider: "claude".to_string(),
            plan_tier: Some("Pro".to_string()),
            windows,
            extra_usage: None,
            stale: false,
            error: error.map(ToString::to_string),
            retry_after_seconds: None,
            cooldown_until: cooldown_until.map(ToString::to_string),
            fetched_at: "2026-03-17T12:00:00Z".to_string(),
        }
    }

    #[test]
    fn cli_fallback_preserves_cached_utilization_when_event_omits_it() {
        let cached = provider_rate_limits(
            vec![
                RateLimitWindow {
                    window_id: "five_hour".to_string(),
                    label: "Session (5hr)".to_string(),
                    utilization: 33.0,
                    resets_at: Some("2026-03-17T14:00:00Z".to_string()),
                },
                RateLimitWindow {
                    window_id: "seven_day".to_string(),
                    label: "Weekly (7 day)".to_string(),
                    utilization: 18.0,
                    resets_at: Some("2026-03-20T14:00:00Z".to_string()),
                },
            ],
            None,
            None,
        );

        let rate_limits = rate_limits_from_claude_cli_info(
            ClaudeCliRateLimitInfo {
                status: "allowed".to_string(),
                resets_at: Some(1_773_781_200),
                rate_limit_type: Some("five_hour".to_string()),
                utilization: None,
            },
            Some(&cached),
        )
        .unwrap();

        assert!(rate_limits.stale);
        assert_eq!(rate_limits.error, None);
        assert_eq!(rate_limits.windows.len(), 2);
        assert_eq!(
            rate_limits
                .windows
                .iter()
                .find(|window| window.window_id == "five_hour")
                .unwrap()
                .utilization,
            33.0
        );
        assert_eq!(
            rate_limits
                .windows
                .iter()
                .find(|window| window.window_id == "five_hour")
                .unwrap()
                .resets_at
                .as_deref(),
            Some("2026-03-17T21:00:00+00:00")
        );
    }

    #[test]
    fn cli_fallback_synthesizes_full_utilization_for_rejected_window() {
        let rate_limits = rate_limits_from_claude_cli_info(
            ClaudeCliRateLimitInfo {
                status: "rejected".to_string(),
                resets_at: Some(1_773_781_200),
                rate_limit_type: Some("five_hour".to_string()),
                utilization: None,
            },
            None,
        )
        .unwrap();

        assert!(!rate_limits.stale);
        assert_eq!(rate_limits.error, None);
        assert_eq!(rate_limits.windows.len(), 1);
        assert_eq!(rate_limits.windows[0].window_id, "five_hour");
        assert_eq!(rate_limits.windows[0].utilization, 100.0);
        assert_eq!(
            rate_limits.cooldown_until.as_deref(),
            Some("2026-03-17T21:00:00+00:00")
        );
    }

    #[test]
    fn cli_fallback_shows_zero_when_allowed_without_utilization_or_cache() {
        let rate_limits = rate_limits_from_claude_cli_info(
            ClaudeCliRateLimitInfo {
                status: "allowed".to_string(),
                resets_at: Some(1_773_781_200),
                rate_limit_type: Some("five_hour".to_string()),
                utilization: None,
            },
            None,
        )
        .unwrap();

        assert!(!rate_limits.stale);
        assert_eq!(rate_limits.error, None);
        assert_eq!(rate_limits.windows.len(), 1);
        assert_eq!(rate_limits.windows[0].window_id, "five_hour");
        assert_eq!(rate_limits.windows[0].utilization, 0.0);
    }
}
