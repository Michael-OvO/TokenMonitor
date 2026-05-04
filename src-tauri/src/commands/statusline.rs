//! IPC commands for installing / checking / removing the Claude Code
//! statusline integration. None of these touch the network or the Keychain;
//! they read and write `~/.claude/settings.json` and a script under
//! `~/.tokenmonitor/statusline/`. The location is intentionally a plain
//! user-home dotfile so the script — which CC runs as a subprocess — never
//! has to write into another app's Application Support container, sidesteps
//! macOS Sequoia's App Data Access TCC sheet entirely.

use serde::Serialize;
use tauri::State;

use super::AppState;
use crate::statusline::{
    install,
    install::{InstallOutcome, InstalledState},
    windows::ClaudePlanTier,
};

/// Install the TokenMonitor statusline into Claude Code.
///
/// Writes the script to `~/.tokenmonitor/statusline/` and patches
/// `~/.claude/settings.json` to reference it. Existing settings are
/// preserved and a `.tokenmonitor.bak` backup is created on first call.
/// Returns the previous `statusLine.command` (if any) so the UI can offer
/// a chain-it-back follow-up.
#[tauri::command]
pub async fn install_statusline() -> Result<InstallOutcome, String> {
    tokio::task::spawn_blocking(install::install)
        .await
        .map_err(|e| format!("Statusline install task failed: {e}"))?
}

/// Probe the install state without making any changes — used by the
/// onboarding wizard to decide whether to show "Install" or "Already
/// installed".
#[tauri::command]
pub async fn check_statusline() -> Result<InstalledState, String> {
    tokio::task::spawn_blocking(install::check)
        .await
        .map_err(|e| format!("Statusline check task failed: {e}"))
}

/// Remove our entry from `~/.claude/settings.json`. The script file on disk
/// is left in place; reinstalling reuses it.
#[tauri::command]
pub async fn uninstall_statusline() -> Result<(), String> {
    tokio::task::spawn_blocking(install::uninstall)
        .await
        .map_err(|e| format!("Statusline uninstall task failed: {e}"))?
}

/// Set the Claude plan tier used as the fallback budget when CC's
/// statusline payload doesn't ship `rate_limits` (very old CC versions).
/// On modern CC builds the plan tier is unused — the percentages come
/// directly from the payload.
#[tauri::command]
pub async fn set_claude_plan_tier(
    tier: String,
    five_hour_tokens: Option<u64>,
    weekly_tokens: Option<u64>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let parsed = match tier.as_str() {
        "Custom" | "custom" => ClaudePlanTier::Custom {
            five_hour_tokens: five_hour_tokens.unwrap_or(200_000),
            weekly_tokens: weekly_tokens.unwrap_or(7_000_000),
        },
        other => ClaudePlanTier::parse(other).unwrap_or_default(),
    };
    *state.claude_plan_tier.write().await = parsed;
    Ok(())
}

/// Snapshot of the most recent statusline event, surfaced to the
/// onboarding UI so it can show "Last seen 12 seconds ago" once CC has
/// fired the script at least once.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestStatuslinePing {
    pub seen: bool,
    pub last_seen_iso: Option<String>,
    pub session_id: Option<String>,
    pub model_display_name: Option<String>,
}

#[tauri::command]
pub async fn read_latest_statusline_ping() -> Result<LatestStatuslinePing, String> {
    let session = tokio::task::spawn_blocking(|| {
        crate::statusline::source::latest_active_session(&crate::statusline::events_file())
            .ok()
            .flatten()
    })
    .await
    .map_err(|e| format!("Statusline ping task failed: {e}"))?;

    Ok(match session {
        Some(s) => LatestStatuslinePing {
            seen: true,
            last_seen_iso: Some(s.last_seen.to_rfc3339()),
            session_id: s.session_id,
            model_display_name: s.model_display_name,
        },
        None => LatestStatuslinePing {
            seen: false,
            last_seen_iso: None,
            session_id: None,
            model_display_name: None,
        },
    })
}
