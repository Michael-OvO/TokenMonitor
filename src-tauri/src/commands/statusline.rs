//! IPC commands for installing / checking the Claude Code statusline
//! integration. None of these touch the network or the Keychain;
//! they read and write `~/.claude/settings.json` and a script under
//! `~/.tokenmonitor/statusline/`. The location is intentionally a plain
//! user-home dotfile so the script — which CC runs as a subprocess — never
//! has to write into another app's Application Support container, sidesteps
//! macOS Sequoia's App Data Access TCC sheet entirely.

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
        "Custom" | "custom" => {
            let pro = crate::ops::claude_plan_budget("pro");
            ClaudePlanTier::Custom {
                five_hour_tokens: five_hour_tokens.unwrap_or(pro.five_hour_tokens),
                weekly_tokens: weekly_tokens.unwrap_or(pro.weekly_tokens),
            }
        }
        other => ClaudePlanTier::parse(other).unwrap_or_default(),
    };
    *state.claude_plan_tier.write().await = parsed;
    Ok(())
}
