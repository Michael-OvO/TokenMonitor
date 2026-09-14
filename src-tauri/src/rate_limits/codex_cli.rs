use crate::models::{CreditsInfo, ProviderRateLimits};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

use super::{codex::codex_windows_from_rate_limits, RateLimitFetchError};

static CACHED_CODEX_CLI_PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();

const CODEX_CLI_PATH_ENV: &str = "CODEX_CLI_PATH";
const CODEX_APP_SERVER_TIMEOUT_SECONDS: u64 = 15;

// ── Codex CLI path resolution ──

fn common_codex_cli_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    #[cfg(not(target_os = "windows"))]
    {
        candidates.push(PathBuf::from("/opt/homebrew/bin/codex"));
        candidates.push(PathBuf::from("/usr/local/bin/codex"));
        candidates.push(PathBuf::from("/usr/bin/codex"));
    }

    if let Some(home) = dirs::home_dir() {
        #[cfg(not(target_os = "windows"))]
        {
            candidates.push(home.join(".local").join("bin").join("codex"));
            candidates.push(home.join(".npm-global").join("bin").join("codex"));
        }

        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                let appdata = PathBuf::from(appdata);
                candidates.push(appdata.join("npm").join("codex.cmd"));
                candidates.push(appdata.join("npm").join("codex"));
            }
            if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
                let localappdata = PathBuf::from(localappdata);
                candidates.push(localappdata.join("pnpm").join("codex.cmd"));
                candidates.push(localappdata.join("pnpm").join("codex"));
            }
        }

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

        if let Ok(entries) = std::fs::read_dir(&nvm_dir) {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect();
            versions.sort_unstable_by(|a, b| b.cmp(a));
            for version in versions {
                #[cfg(not(target_os = "windows"))]
                candidates.push(version.join("bin").join("codex"));
                #[cfg(target_os = "windows")]
                {
                    candidates.push(version.join("codex.cmd"));
                    candidates.push(version.join("codex"));
                }
            }
        }
    }

    candidates
}

pub(crate) fn resolve_codex_cli_path() -> Result<PathBuf, String> {
    CACHED_CODEX_CLI_PATH
        .get_or_init(resolve_codex_cli_path_uncached)
        .clone()
}

fn resolve_codex_cli_path_uncached() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(CODEX_CLI_PATH_ENV).map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
    }

    if let Some(path) = super::command_in_path("codex") {
        return Ok(path);
    }

    if let Some(path) = common_codex_cli_paths()
        .into_iter()
        .find(|candidate| candidate.is_file())
    {
        return Ok(path);
    }

    Err("Codex CLI was not found on this system".to_string())
}

// ── App-server JSON-RPC response types ──

#[derive(Deserialize)]
struct AppServerResponse<T> {
    #[allow(dead_code)]
    id: Option<u64>,
    result: Option<T>,
    error: Option<AppServerError>,
}

#[derive(Deserialize)]
struct AppServerError {
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitsReadResult {
    rate_limits: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreditsSnapshot {
    has_credits: bool,
    unlimited: bool,
    balance: Option<String>,
}

// ── Mapping helpers ──

fn normalize_plan_type(plan_type: &str) -> String {
    match plan_type {
        "pro" => "Pro".to_string(),
        "plus" => "Plus".to_string(),
        "free" => "Free".to_string(),
        "prolite" => "Pro Lite".to_string(),
        "team" => "Team".to_string(),
        "enterprise" => "Enterprise".to_string(),
        "business" => "Business".to_string(),
        other => other.to_string(),
    }
}

fn credits_from_snapshot(snapshot: CreditsSnapshot) -> CreditsInfo {
    CreditsInfo {
        balance: snapshot
            .balance
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok()),
        has_credits: snapshot.has_credits,
        unlimited: snapshot.unlimited,
    }
}

pub(super) fn parse_rate_limits_response(
    json: &str,
) -> Result<ProviderRateLimits, RateLimitFetchError> {
    let resp: AppServerResponse<RateLimitsReadResult> =
        serde_json::from_str(json).map_err(|e| {
            RateLimitFetchError::message(format!("Failed to parse app-server response: {e}"))
        })?;

    if let Some(error) = resp.error {
        return Err(RateLimitFetchError::message(format!(
            "Codex app-server error: {}",
            error.message
        )));
    }

    let result = resp
        .result
        .ok_or_else(|| RateLimitFetchError::message("Codex app-server returned empty result"))?;

    let snapshot = result.rate_limits;
    let windows = codex_windows_from_rate_limits(&snapshot);

    let plan_tier = snapshot
        .get("planType")
        .or_else(|| snapshot.get("plan_type"))
        .and_then(|v| v.as_str())
        .map(normalize_plan_type);
    let credits = snapshot
        .get("credits")
        .cloned()
        .and_then(|v| serde_json::from_value::<CreditsSnapshot>(v).ok())
        .map(credits_from_snapshot);

    let rate_limit_reached = snapshot
        .get("rateLimitReachedType")
        .or_else(|| snapshot.get("rate_limit_reached_type"))
        .and_then(|v| match v {
            Value::Null => None,
            Value::String(s) if s.is_empty() => None,
            other => Some(other.clone()),
        })
        .is_some();

    let cooldown_until = if rate_limit_reached {
        windows
            .iter()
            .filter_map(|w| w.resets_at.as_ref())
            .min()
            .cloned()
    } else {
        None
    };

    let retry_after_seconds = cooldown_until.as_deref().and_then(|raw| {
        DateTime::parse_from_rfc3339(raw).ok().map(|dt| {
            dt.with_timezone(&Utc)
                .signed_duration_since(Utc::now())
                .num_seconds()
                .max(0) as u64
        })
    });

    Ok(ProviderRateLimits {
        provider: "codex".to_string(),
        plan_tier,
        windows,
        extra_usage: None,
        credits,
        stale: false,
        error: None,
        retry_after_seconds,
        cooldown_until,
        fetched_at: Local::now().to_rfc3339(),
    })
}

// ── App-server probe ──

pub(super) async fn fetch_codex_rate_limits_via_cli(
) -> Result<ProviderRateLimits, RateLimitFetchError> {
    let cli_path = resolve_codex_cli_path().map_err(RateLimitFetchError::message)?;

    let mut command = TokioCommand::new(cli_path);
    command.kill_on_drop(true);
    command.args(["app-server"]);
    command.stdin(std::process::Stdio::piped());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::null());

    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = command.spawn().map_err(|e| {
        RateLimitFetchError::message(format!("Failed to start Codex app-server: {e}"))
    })?;

    let result = timeout(
        std::time::Duration::from_secs(CODEX_APP_SERVER_TIMEOUT_SECONDS),
        app_server_exchange(&mut child),
    )
    .await
    .map_err(|_| RateLimitFetchError::message("Codex app-server probe timed out"))?;

    // Ensure child is killed on any exit path (kill_on_drop handles this on
    // drop, but explicit kill avoids leaving the process around for longer
    // than necessary).
    let _ = child.kill().await;

    result
}

/// True when `line` is a JSON-RPC message carrying `"id": id`. Responses to our
/// requests echo the request id; unsolicited notifications carry a `"method"`
/// and no id, so they probe to `false`.
fn response_has_id(line: &str, id: u64) -> bool {
    #[derive(Deserialize)]
    struct IdProbe {
        id: Option<u64>,
    }
    serde_json::from_str::<IdProbe>(line)
        .ok()
        .and_then(|probe| probe.id)
        == Some(id)
}

/// Read app-server lines until the response matching `id` arrives, skipping
/// blanks and interleaved notifications. The app-server pushes unsolicited
/// notifications (e.g. `remoteControl/status/changed`) between responses, so a
/// single `read_line` after a request can land on a notification instead of the
/// result — the bug that made every Codex probe fall back to the stale log file.
async fn read_json_rpc_response<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    id: u64,
) -> Result<String, RateLimitFetchError> {
    // Bounded so a chatty server can never spin forever; the outer 15s timeout
    // is the real ceiling. 64 lines is far more than the handshake ever emits.
    const MAX_LINES: usize = 64;
    for _ in 0..MAX_LINES {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await.map_err(|e| {
            RateLimitFetchError::message(format!("Failed to read app-server response: {e}"))
        })?;
        if bytes == 0 {
            return Err(RateLimitFetchError::message(
                "Codex app-server closed before responding",
            ));
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() && response_has_id(trimmed, id) {
            return Ok(trimmed.to_string());
        }
    }
    Err(RateLimitFetchError::message(
        "Codex app-server sent no matching response",
    ))
}

async fn app_server_exchange(
    child: &mut tokio::process::Child,
) -> Result<ProviderRateLimits, RateLimitFetchError> {
    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| RateLimitFetchError::message("Failed to open app-server stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RateLimitFetchError::message("Failed to open app-server stdout"))?;

    let mut reader = BufReader::new(stdout);

    // 1. Send initialize
    let init_msg = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"clientInfo":{"name":"TokenMonitor","version":"1.0"}}}"#;
    stdin
        .write_all(init_msg.as_bytes())
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Failed to write init: {e}")))?;
    stdin
        .write_all(b"\n")
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Failed to write newline: {e}")))?;
    stdin
        .flush()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Failed to flush init: {e}")))?;

    // 2. Read initialize response (id:0), skipping any interleaved notifications.
    let _init_line = read_json_rpc_response(&mut reader, 0).await?;

    // 3. Send rateLimits/read
    let rl_msg = r#"{"jsonrpc":"2.0","id":1,"method":"account/rateLimits/read","params":null}"#;
    stdin.write_all(rl_msg.as_bytes()).await.map_err(|e| {
        RateLimitFetchError::message(format!("Failed to write rate limits request: {e}"))
    })?;
    stdin
        .write_all(b"\n")
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Failed to write newline: {e}")))?;
    stdin
        .flush()
        .await
        .map_err(|e| RateLimitFetchError::message(format!("Failed to flush request: {e}")))?;

    // 4. Read the id:1 response, skipping notifications (e.g.
    // `remoteControl/status/changed`) the server interleaves between replies.
    let rl_line = read_json_rpc_response(&mut reader, 1).await?;
    parse_rate_limits_response(&rl_line)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RESPONSE: &str = r#"{"id":1,"result":{"rateLimits":{"limitId":"codex","limitName":null,"primary":{"usedPercent":5,"windowDurationMins":300,"resetsAt":1777276169},"secondary":{"usedPercent":46,"windowDurationMins":10080,"resetsAt":1777406032},"credits":{"hasCredits":true,"unlimited":false,"balance":"2116.0215000000"},"planType":"plus","rateLimitReachedType":null},"rateLimitsByLimitId":{"codex":{"limitId":"codex","limitName":null,"primary":{"usedPercent":5,"windowDurationMins":300,"resetsAt":1777276169},"secondary":{"usedPercent":46,"windowDurationMins":10080,"resetsAt":1777406032},"credits":{"hasCredits":true,"unlimited":false,"balance":"2116.0215000000"},"planType":"plus","rateLimitReachedType":null}}}}"#;

    #[test]
    fn parses_valid_app_server_response() {
        let result = parse_rate_limits_response(SAMPLE_RESPONSE).unwrap();

        assert_eq!(result.provider, "codex");
        assert_eq!(result.plan_tier.as_deref(), Some("Plus"));
        assert!(!result.stale);
        assert_eq!(result.windows.len(), 2);

        let primary = result
            .windows
            .iter()
            .find(|w| w.window_id == "primary")
            .unwrap();
        assert_eq!(primary.utilization, 5.0);
        assert_eq!(primary.label, "Session (5hr)");
        assert!(primary.resets_at.is_some());

        let secondary = result
            .windows
            .iter()
            .find(|w| w.window_id == "secondary")
            .unwrap();
        assert_eq!(secondary.utilization, 46.0);
        assert_eq!(secondary.label, "Weekly (7 day)");

        let credits = result.credits.unwrap();
        assert!(credits.has_credits);
        assert!(!credits.unlimited);
        assert!((credits.balance.unwrap() - 2116.0215).abs() < 0.001);
    }

    #[test]
    fn parses_response_without_credits() {
        let json = r#"{"id":1,"result":{"rateLimits":{"primary":{"usedPercent":10,"windowDurationMins":300,"resetsAt":1777276169},"secondary":null,"credits":null,"planType":"free","rateLimitReachedType":null}}}"#;
        let result = parse_rate_limits_response(json).unwrap();

        assert_eq!(result.windows.len(), 1);
        assert!(result.credits.is_none());
        assert_eq!(result.plan_tier.as_deref(), Some("Free"));
    }

    #[test]
    fn parses_error_response() {
        let json = r#"{"id":1,"error":{"code":-32600,"message":"Not initialized"}}"#;
        let err = parse_rate_limits_response(json).unwrap_err();
        assert!(err.message.contains("Not initialized"));
    }

    #[test]
    fn parses_rate_limit_reached_response() {
        let json = r#"{"id":1,"result":{"rateLimits":{"primary":{"usedPercent":100,"windowDurationMins":300,"resetsAt":1777276169},"secondary":{"usedPercent":46,"windowDurationMins":10080,"resetsAt":1777406032},"credits":null,"planType":"plus","rateLimitReachedType":"rate_limit_reached"}}}"#;
        let result = parse_rate_limits_response(json).unwrap();

        assert!(result.cooldown_until.is_some());
        assert_eq!(result.windows[0].utilization, 100.0);
    }

    #[tokio::test]
    async fn read_json_rpc_response_skips_interleaved_notifications() {
        // Real capture: the app-server pushes a `remoteControl/status/changed`
        // notification between the init response (id:0) and the rate-limits
        // response (id:1). Reading a single line after the request grabbed the
        // notification and the probe failed → fell back to the stale log file.
        let stream = concat!(
            r#"{"id":0,"result":{"userAgent":"x"}}"#,
            "\n",
            r#"{"method":"remoteControl/status/changed","params":{"status":"disabled"}}"#,
            "\n",
            r#"{"id":1,"result":{"rateLimits":{"primary":{"usedPercent":76,"windowDurationMins":300,"resetsAt":1781919685},"secondary":{"usedPercent":32,"windowDurationMins":10080,"resetsAt":1782346631},"credits":null,"planType":"plus","rateLimitReachedType":null}}}"#,
            "\n",
        );
        let mut reader = BufReader::new(stream.as_bytes());

        // Init handshake consumes id:0, skipping nothing yet.
        let init = read_json_rpc_response(&mut reader, 0).await.unwrap();
        assert!(init.contains("\"id\":0"));

        // The rate-limits read must skip the notification and land on id:1.
        let rl_line = read_json_rpc_response(&mut reader, 1).await.unwrap();
        let result = parse_rate_limits_response(&rl_line).unwrap();
        let primary = result
            .windows
            .iter()
            .find(|w| w.window_id == "primary")
            .unwrap();
        assert_eq!(primary.utilization, 76.0);
        assert!(!result.stale);
    }

    #[test]
    fn parses_unlimited_credits() {
        let json = r#"{"id":1,"result":{"rateLimits":{"primary":{"usedPercent":10,"windowDurationMins":300,"resetsAt":1777276169},"secondary":null,"credits":{"hasCredits":true,"unlimited":true,"balance":null},"planType":"enterprise","rateLimitReachedType":null}}}"#;
        let result = parse_rate_limits_response(json).unwrap();

        let credits = result.credits.unwrap();
        assert!(credits.unlimited);
        assert!(credits.balance.is_none());
    }
}
