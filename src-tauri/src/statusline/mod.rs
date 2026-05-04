//! Statusline-driven Claude usage source.
//!
//! TokenMonitor installs a tiny shell/PowerShell script as Claude Code's
//! `statusLine` command. CC invokes that script on every prompt with a JSON
//! envelope (session id, transcript path, model, cwd, version, rate_limits)
//! on stdin. The script appends one JSONL event to a file under
//! `~/.tokenmonitor/statusline/` and prints nothing back to CC, leaving the
//! terminal status line clean. TokenMonitor reads the most recent event to
//! relay CC's own server-computed `used_percentage` for the 5-hour and
//! 7-day windows — no OAuth, no Keychain, no `claude` subprocess.
//!
//! Path choice: events live under `~/.tokenmonitor/`, **not** under Tauri's
//! `Application Support/com.tokenmonitor.app/` data dir. The reason is
//! macOS Sequoia's "App Data Access" TCC layer: a process running as a
//! subprocess of Claude Code (which is what the statusline script is) that
//! writes into another app's container can trigger a prompt asking the
//! user to authorize Claude Code's access to TokenMonitor's data. A plain
//! user-home dotfile dodges that entirely — the script is just touching
//! the same kind of path as `~/.claude/`, `~/.codex/`, `~/.ssh/`.
//!
//! The `TOKEN_MONITOR_STATUSLINE_DIR` env var overrides the location for
//! tests so multiple integration tests can run in parallel without racing
//! on the same on-disk file.
//!
//! Submodules:
//! - `scripts`: shell + PowerShell script bodies (string constants)
//! - `source`: event-file reader with retention trimming
//! - `windows`: rolling-window math used as a fallback when the CC payload
//!   doesn't ship `rate_limits` (very old CC builds)
//! - `install`: writes scripts + patches `~/.claude/settings.json`

pub mod install;
pub mod scripts;
pub mod source;
pub mod windows;

use std::path::PathBuf;

/// Resolve the statusline directory.
///
/// Honors `$TOKEN_MONITOR_STATUSLINE_DIR` when set (test-only override).
/// Otherwise resolves to `~/.tokenmonitor/statusline/`. We deliberately
/// avoid Tauri's `app_data_dir()` here so the script — which runs as a
/// subprocess of Claude Code — never has to write into another app's
/// container and thus never triggers macOS Sequoia's App Data TCC sheet.
pub fn statusline_dir() -> PathBuf {
    if let Ok(p) = std::env::var("TOKEN_MONITOR_STATUSLINE_DIR") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".tokenmonitor")
        .join("statusline")
}

/// JSONL events file path within the statusline dir.
pub fn events_file() -> PathBuf {
    statusline_dir().join("events.jsonl")
}

/// On-disk path of the script TokenMonitor installs into the user's home.
/// The script extension is platform-specific so users can spot it via
/// tab-completion and so Windows wires it through PowerShell rather than
/// `cmd.exe`'s parser.
pub fn script_path() -> PathBuf {
    let name = if cfg!(target_os = "windows") {
        "tokenmonitor-statusline.ps1"
    } else {
        "tokenmonitor-statusline.sh"
    };
    statusline_dir().join(name)
}
