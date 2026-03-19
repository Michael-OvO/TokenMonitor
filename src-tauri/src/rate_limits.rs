use crate::models::{ExtraUsageInfo, ProviderRateLimits, RateLimitWindow, RateLimitsPayload};
use chrono::{DateTime, Duration, Local, Utc};
use reqwest::header::{HeaderMap, RETRY_AFTER};
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration as StdDuration;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

#[derive(Debug, Clone)]
struct RateLimitFetchError {
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

// ─────────────────────────────────────────────────────────────────────────────
// Claude: OAuth token from macOS Keychain + API call
// ─────────────────────────────────────────────────────────────────────────────

fn get_claude_oauth_token() -> Result<String, String> {
    let output = StdCommand::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .map_err(|e| format!("Failed to run security command: {e}"))?;

    if !output.status.success() {
        return Err("Claude Code credentials not found in Keychain".to_string());
    }

    let raw = String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 from Keychain: {e}"))?;

    let parsed: serde_json::Value =
        serde_json::from_str(raw.trim()).map_err(|e| format!("Invalid JSON in Keychain: {e}"))?;

    parsed
        .get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No accessToken in Keychain credentials".to_string())
}

// ── Claude API response types ──

const CLAUDE_CLI_PROBE_TIMEOUT_SECONDS: u64 = 20;
const CLAUDE_CLI_PROBE_PROMPT: &str = "Respond with OK only.";
const CLAUDE_CLI_PATH_ENV: &str = "CLAUDE_CLI_PATH";
const CLAUDE_CLI_RESOLVE_COMMAND: &str = "command -v claude";

#[derive(Deserialize)]
struct ClaudeUsageResponse {
    five_hour: Option<ClaudeWindowData>,
    seven_day: Option<ClaudeWindowData>,
    seven_day_sonnet: Option<ClaudeWindowData>,
    seven_day_opus: Option<ClaudeWindowData>,
    seven_day_oauth_apps: Option<ClaudeWindowData>,
    seven_day_cowork: Option<ClaudeWindowData>,
    iguana_necktie: Option<ClaudeWindowData>,
    extra_usage: Option<ClaudeExtraUsageData>,
}

#[derive(Deserialize)]
struct ClaudeWindowData {
    utilization: f64,
    resets_at: String,
}

#[derive(Deserialize)]
struct ClaudeExtraUsageData {
    is_enabled: bool,
    monthly_limit: f64,
    used_credits: f64,
    utilization: Option<f64>,
}

fn normalize_claude_extra_usage(extra_usage: ClaudeExtraUsageData) -> ExtraUsageInfo {
    ExtraUsageInfo {
        is_enabled: extra_usage.is_enabled,
        // The OAuth usage endpoint reports credit values in cents.
        monthly_limit: extra_usage.monthly_limit / 100.0,
        used_credits: extra_usage.used_credits / 100.0,
        utilization: extra_usage.utilization,
    }
}

#[derive(Deserialize)]
struct ClaudeAccountResponse {
    memberships: Vec<ClaudeMembership>,
}

#[derive(Deserialize)]
struct ClaudeMembership {
    organization: ClaudeOrganization,
}

#[derive(Deserialize)]
struct ClaudeOrganization {
    capabilities: Option<Vec<String>>,
    rate_limit_tier: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeCliStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    rate_limit_info: Option<ClaudeCliRateLimitInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCliRateLimitInfo {
    status: String,
    resets_at: Option<i64>,
    rate_limit_type: Option<String>,
    utilization: Option<f64>,
}

async fn fetch_claude_rate_limits() -> Result<ProviderRateLimits, RateLimitFetchError> {
    let token = get_claude_oauth_token().map_err(|error| RateLimitFetchError::message(error))?;

    let client = reqwest::Client::new();

    // Fetch usage + account in parallel
    let usage_fut = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .bearer_auth(&token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send();

    let account_fut = client
        .get("https://api.anthropic.com/api/oauth/account")
        .bearer_auth(&token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send();

    let (usage_res, account_res) = tokio::join!(usage_fut, account_fut);

    // Parse usage response
    let usage_resp = usage_res
        .map_err(|e| RateLimitFetchError::message(format!("Usage API request failed: {e}")))?;
    if !usage_resp.status().is_success() {
        return Err(rate_limit_error_from_response(&usage_resp));
    }
    let usage: ClaudeUsageResponse = usage_resp.json().await.map_err(|e| {
        RateLimitFetchError::message(format!("Failed to parse usage response: {e}"))
    })?;

    // Parse account response (non-fatal if it fails)
    let plan_tier = match account_res {
        Ok(resp) if resp.status().is_success() => resp
            .json::<ClaudeAccountResponse>()
            .await
            .ok()
            .and_then(|acct| detect_claude_plan(&acct)),
        _ => None,
    };

    // Build windows from non-null entries
    let mut windows = Vec::new();
    let window_specs: &[(&str, &str, &Option<ClaudeWindowData>)] = &[
        ("five_hour", "Session (5hr)", &usage.five_hour),
        ("seven_day", "Weekly (7 day)", &usage.seven_day),
        ("seven_day_sonnet", "Weekly Sonnet", &usage.seven_day_sonnet),
        ("seven_day_opus", "Weekly Opus", &usage.seven_day_opus),
        (
            "seven_day_oauth_apps",
            "Weekly OAuth Apps",
            &usage.seven_day_oauth_apps,
        ),
        ("seven_day_cowork", "Weekly Cowork", &usage.seven_day_cowork),
        ("iguana_necktie", "Iguana Necktie", &usage.iguana_necktie),
    ];

    for (id, label, data) in window_specs {
        if let Some(w) = data {
            windows.push(RateLimitWindow {
                window_id: id.to_string(),
                label: label.to_string(),
                utilization: w.utilization,
                resets_at: Some(w.resets_at.clone()),
            });
        }
    }

    let extra_usage = usage.extra_usage.map(normalize_claude_extra_usage);

    Ok(ProviderRateLimits {
        provider: "claude".to_string(),
        plan_tier,
        windows,
        extra_usage,
        stale: false,
        error: None,
        retry_after_seconds: None,
        cooldown_until: None,
        fetched_at: Local::now().to_rfc3339(),
    })
}

fn command_in_path(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn common_claude_cli_paths() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
        PathBuf::from("/usr/bin/claude"),
    ];

    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local").join("bin").join("claude"));
        candidates.push(home.join(".npm-global").join("bin").join("claude"));

        let nvm_dir = home.join(".nvm").join("versions").join("node");
        if let Ok(entries) = std::fs::read_dir(nvm_dir) {
            let mut versions = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>();
            versions.sort();
            versions.reverse();
            for version in versions {
                candidates.push(version.join("bin").join("claude"));
            }
        }
    }

    candidates
}

fn resolve_claude_cli_path() -> Result<PathBuf, String> {
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

    let output = StdCommand::new("/bin/zsh")
        .args(["-lc", CLAUDE_CLI_RESOLVE_COMMAND])
        .output()
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

fn format_cli_timestamp(seconds: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(seconds, 0).map(|dt| dt.to_rfc3339())
}

fn retry_after_from_reset(reset_at: Option<&str>, now: DateTime<Utc>) -> Option<u64> {
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

fn parse_claude_cli_rate_limit_info(output: &str) -> Result<ClaudeCliRateLimitInfo, String> {
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

fn rate_limits_from_claude_cli_info(
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

    let utilization = match info.utilization {
        Some(utilization) => utilization,
        None => {
            if let Some(window) = cached_window {
                used_cached_window_data = true;
                window.utilization
            } else if info.status == "rejected" {
                100.0
            } else {
                // CLI no longer emits utilization when status is "allowed" —
                // treat as 0% (not rate-limited) and mark the result stale.
                used_cached_window_data = true;
                0.0
            }
        }
    };

    let mut windows = cached
        .map(|payload| payload.windows.clone())
        .unwrap_or_default();

    let next_window = RateLimitWindow {
        window_id: window_id.to_string(),
        label: label.to_string(),
        utilization,
        resets_at: reset_at
            .clone()
            .or_else(|| cached_window.and_then(|window| window.resets_at.clone())),
    };

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

async fn fetch_claude_rate_limits_via_cli(
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

fn detect_claude_plan(acct: &ClaudeAccountResponse) -> Option<String> {
    for membership in &acct.memberships {
        if let Some(caps) = &membership.organization.capabilities {
            if caps.iter().any(|c| c == "claude_max") {
                // Use rate_limit_tier for more detail if available
                if let Some(tier) = &membership.organization.rate_limit_tier {
                    return Some(format_claude_plan_tier(tier));
                }
                return Some("Max".to_string());
            }
        }
    }
    // Fallback: check first membership with capabilities
    for membership in &acct.memberships {
        if let Some(caps) = &membership.organization.capabilities {
            if caps.contains(&"chat".to_string()) && !caps.contains(&"api".to_string()) {
                return Some("Pro".to_string());
            }
        }
    }
    None
}

fn format_claude_plan_tier(tier: &str) -> String {
    if tier.contains("claude_max_20x") {
        "Max 20x".to_string()
    } else if tier.contains("claude_max_5x") {
        "Max 5x".to_string()
    } else if tier.contains("claude_max") {
        "Max 5x".to_string() // base Max plan is the 5x tier ($100)
    } else if tier.contains("pro") {
        "Pro".to_string()
    } else {
        tier.to_string()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Codex: parse rate_limits from JSONL session files
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CodexJsonlLine {
    payload: Option<CodexPayload>,
}

#[derive(Deserialize)]
struct CodexPayload {
    rate_limits: Option<CodexRateLimitData>,
}

#[derive(Deserialize)]
struct CodexRateLimitData {
    primary: Option<CodexWindowData>,
    secondary: Option<CodexWindowData>,
    plan_type: Option<String>,
}

#[derive(Deserialize)]
struct CodexWindowData {
    used_percent: f64,
    window_minutes: u64,
    resets_at: u64,
}

fn extract_codex_rate_limits(codex_dir: &Path) -> Result<ProviderRateLimits, String> {
    // Find the most recent JSONL file by walking the date-based directory structure
    let mut newest_file: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    find_newest_jsonl(codex_dir, &mut newest_file, 0);

    let file_path = newest_file
        .map(|(_, p)| p)
        .ok_or_else(|| "No Codex session files found".to_string())?;

    // Read from the end looking for rate_limits
    let file =
        std::fs::File::open(&file_path).map_err(|e| format!("Failed to open Codex file: {e}"))?;
    let reader = BufReader::new(file);

    let mut last_rate_limits: Option<CodexRateLimitData> = None;
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if !line.contains("rate_limits") {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<CodexJsonlLine>(&line) {
            if let Some(payload) = entry.payload {
                if let Some(rl) = payload.rate_limits {
                    last_rate_limits = Some(rl);
                }
            }
        }
    }

    let rl =
        last_rate_limits.ok_or_else(|| "No rate limit data in Codex session files".to_string())?;

    let mut windows = Vec::new();
    if let Some(primary) = &rl.primary {
        windows.push(codex_window_to_rate_limit("primary", primary));
    }
    if let Some(secondary) = &rl.secondary {
        windows.push(codex_window_to_rate_limit("secondary", secondary));
    }

    let plan_tier = rl.plan_type.as_ref().map(|p| match p.as_str() {
        "pro" => "Pro".to_string(),
        "plus" => "Plus".to_string(),
        "free" => "Free".to_string(),
        other => other.to_string(),
    });

    Ok(ProviderRateLimits {
        provider: "codex".to_string(),
        plan_tier,
        windows,
        extra_usage: None,
        stale: false,
        error: None,
        retry_after_seconds: None,
        cooldown_until: None,
        fetched_at: Local::now().to_rfc3339(),
    })
}

fn codex_window_to_rate_limit(id: &str, w: &CodexWindowData) -> RateLimitWindow {
    let label = match (id, w.window_minutes) {
        ("primary", 300) => "Session (5hr)".to_string(),
        ("primary", _) => format!("Primary ({}m)", w.window_minutes),
        ("secondary", 10080) => "Weekly (7 day)".to_string(),
        ("secondary", _) => format!("Secondary ({}m)", w.window_minutes),
        _ => id.to_string(),
    };

    let resets_at =
        DateTime::<Utc>::from_timestamp(w.resets_at as i64, 0).map(|dt| dt.to_rfc3339());

    RateLimitWindow {
        window_id: id.to_string(),
        label,
        utilization: w.used_percent,
        resets_at,
    }
}

fn find_newest_jsonl(
    dir: &Path,
    newest: &mut Option<(std::time::SystemTime, std::path::PathBuf)>,
    depth: u32,
) {
    if depth > 5 {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_newest_jsonl(&path, newest, depth + 1);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            if let Ok(meta) = std::fs::metadata(&path) {
                if let Ok(mtime) = meta.modified() {
                    if newest.as_ref().map_or(true, |(prev, _)| mtime > *prev) {
                        *newest = Some((mtime, path));
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Combined: fetch both providers
// ─────────────────────────────────────────────────────────────────────────────

fn parse_retry_after_seconds(headers: &HeaderMap, now: DateTime<Utc>) -> Option<u64> {
    let raw = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();

    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(seconds);
    }

    let retry_at = DateTime::parse_from_rfc2822(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            DateTime::parse_from_rfc3339(raw)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        })?;

    let remaining = retry_at.signed_duration_since(now).num_seconds();
    Some(remaining.max(0) as u64)
}

fn parse_header_datetime(headers: &HeaderMap, name: &str) -> Option<DateTime<Utc>> {
    let raw = headers.get(name)?.to_str().ok()?.trim();

    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            DateTime::parse_from_rfc2822(raw)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        })
}

fn parse_reset_datetimes(headers: &HeaderMap) -> Vec<DateTime<Utc>> {
    [
        "anthropic-ratelimit-requests-reset",
        "anthropic-ratelimit-tokens-reset",
        "anthropic-ratelimit-input-tokens-reset",
        "anthropic-ratelimit-output-tokens-reset",
    ]
    .into_iter()
    .filter_map(|name| parse_header_datetime(headers, name))
    .collect()
}

fn cooldown_metadata(headers: &HeaderMap, now: DateTime<Utc>) -> (Option<u64>, Option<String>) {
    let retry_after_until = parse_retry_after_seconds(headers, now)
        .map(|seconds| now + Duration::seconds(seconds as i64));
    let reset_until = parse_reset_datetimes(headers).into_iter().max();
    let cooldown_until = match (retry_after_until, reset_until) {
        (Some(retry_after_until), Some(reset_until)) => Some(retry_after_until.max(reset_until)),
        (Some(retry_after_until), None) => Some(retry_after_until),
        (None, Some(reset_until)) => Some(reset_until),
        (None, None) => None,
    };

    let retry_after_seconds = cooldown_until
        .as_ref()
        .map(|retry_at| retry_at.signed_duration_since(now).num_seconds().max(0) as u64);

    (
        retry_after_seconds,
        cooldown_until.map(|retry_at| retry_at.to_rfc3339()),
    )
}

fn rate_limit_error_from_response(response: &reqwest::Response) -> RateLimitFetchError {
    let now = Utc::now();
    let (retry_after_seconds, cooldown_until) = cooldown_metadata(response.headers(), now);

    RateLimitFetchError {
        message: format!("Usage API returned {}", response.status()),
        retry_after_seconds,
        cooldown_until,
    }
}

fn provider_rate_limit_error(provider: &str, error: RateLimitFetchError) -> ProviderRateLimits {
    ProviderRateLimits {
        provider: provider.to_string(),
        plan_tier: None,
        windows: vec![],
        extra_usage: None,
        stale: false,
        error: Some(error.message),
        retry_after_seconds: error.retry_after_seconds,
        cooldown_until: error.cooldown_until,
        fetched_at: Local::now().to_rfc3339(),
    }
}

fn mark_rate_limits_stale(mut rate_limits: ProviderRateLimits) -> ProviderRateLimits {
    rate_limits.stale = true;
    rate_limits
}

fn provider_cooldown_is_active(rate_limits: &ProviderRateLimits, now: DateTime<Utc>) -> bool {
    rate_limits
        .cooldown_until
        .as_deref()
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|dt| dt.with_timezone(&Utc) > now)
        .unwrap_or(false)
}

fn merge_provider_rate_limits(
    fresh: Option<ProviderRateLimits>,
    cached: Option<ProviderRateLimits>,
) -> Option<ProviderRateLimits> {
    match (fresh, cached) {
        (Some(fresh), Some(cached))
            if fresh.windows.is_empty() && fresh.error.is_some() && !cached.windows.is_empty() =>
        {
            Some(ProviderRateLimits {
                stale: true,
                error: fresh.error,
                retry_after_seconds: fresh.retry_after_seconds,
                cooldown_until: fresh.cooldown_until,
                fetched_at: fresh.fetched_at,
                ..cached
            })
        }
        (Some(fresh), _) => Some(fresh),
        (None, cached) => cached,
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
                    eprintln!(
                        "Claude CLI rate-limit fallback failed after API error: {}",
                        cli_error.message
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

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

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
    fn parses_retry_after_seconds_header() {
        let now = DateTime::parse_from_rfc3339("2026-03-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("42"));

        assert_eq!(parse_retry_after_seconds(&headers, now), Some(42));
    }

    #[test]
    fn parses_retry_after_http_date_header() {
        let now = DateTime::parse_from_rfc3339("2026-03-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut headers = HeaderMap::new();
        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_static("Tue, 17 Mar 2026 12:00:30 GMT"),
        );

        assert_eq!(parse_retry_after_seconds(&headers, now), Some(30));
    }

    #[test]
    fn uses_the_latest_retry_boundary_when_retry_after_and_reset_headers_disagree() {
        let now = DateTime::parse_from_rfc3339("2026-03-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("5"));
        headers.insert(
            "anthropic-ratelimit-requests-reset",
            HeaderValue::from_static("2026-03-17T12:01:00Z"),
        );
        headers.insert(
            "anthropic-ratelimit-output-tokens-reset",
            HeaderValue::from_static("2026-03-17T12:02:00Z"),
        );

        let (retry_after_seconds, cooldown_until) = cooldown_metadata(&headers, now);

        assert_eq!(retry_after_seconds, Some(120));
        assert_eq!(cooldown_until.as_deref(), Some("2026-03-17T12:02:00+00:00"));
    }

    #[test]
    fn normalizes_claude_extra_usage_from_cents_to_usd() {
        let extra_usage = normalize_claude_extra_usage(ClaudeExtraUsageData {
            is_enabled: true,
            monthly_limit: 5000.0,
            used_credits: 710.0,
            utilization: Some(14.2),
        });

        assert!(extra_usage.is_enabled);
        assert_eq!(extra_usage.monthly_limit, 50.0);
        assert_eq!(extra_usage.used_credits, 7.1);
        assert_eq!(extra_usage.utilization, Some(14.2));
    }

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
    fn cli_fallback_marks_allowed_window_without_utilization_or_cache_as_stale_zeroed_data() {
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

        assert!(rate_limits.stale);
        assert_eq!(rate_limits.error, None);
        assert_eq!(rate_limits.windows.len(), 1);
        assert_eq!(rate_limits.windows[0].window_id, "five_hour");
        assert_eq!(rate_limits.windows[0].utilization, 0.0);
        assert_eq!(
            rate_limits.windows[0].resets_at.as_deref(),
            Some("2026-03-17T21:00:00+00:00")
        );
    }

    #[test]
    fn merges_cached_windows_with_fresh_error_metadata() {
        let cached = provider_rate_limits(
            vec![RateLimitWindow {
                window_id: "five_hour".to_string(),
                label: "Session (5hr)".to_string(),
                utilization: 33.0,
                resets_at: Some("2026-03-17T14:00:00Z".to_string()),
            }],
            None,
            None,
        );
        let mut fresh_error = provider_rate_limits(
            vec![],
            Some("Usage API returned 429 Too Many Requests"),
            Some("2026-03-17T12:05:00Z"),
        );
        fresh_error.fetched_at = "2026-03-17T12:04:00Z".to_string();

        let merged = merge_provider_rate_limits(Some(fresh_error), Some(cached)).unwrap();

        assert!(merged.stale);
        assert_eq!(merged.windows.len(), 1);
        assert_eq!(
            merged.error.as_deref(),
            Some("Usage API returned 429 Too Many Requests"),
        );
        assert_eq!(
            merged.cooldown_until.as_deref(),
            Some("2026-03-17T12:05:00Z"),
        );
        assert_eq!(merged.fetched_at, "2026-03-17T12:04:00Z");
    }

    #[test]
    fn detects_active_provider_cooldown() {
        let now = DateTime::parse_from_rfc3339("2026-03-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let rate_limits = provider_rate_limits(vec![], Some("429"), Some("2026-03-17T12:01:00Z"));

        assert!(provider_cooldown_is_active(&rate_limits, now));
    }
}
