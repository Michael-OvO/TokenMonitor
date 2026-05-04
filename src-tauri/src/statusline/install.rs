//! Install / uninstall the TokenMonitor statusline into Claude Code.
//!
//! Claude Code's `~/.claude/settings.json` accepts a `statusLine` object with
//! a `command` string. We:
//!   1. Write the script to `~/.tokenmonitor/statusline/<script>` with
//!      executable bits set so CC's shell can run it.
//!   2. Read `~/.claude/settings.json` (or create a fresh `{}` if absent).
//!   3. Back up the file to `settings.json.tokenmonitor.bak` if not already
//!      present, so the user can revert.
//!   4. Write the new `statusLine` field, preserving every other key.
//!
//! Uninstall reverses step 4 only, leaving the script on disk in case the
//! user opts back in.
//!
//! The storage location is intentionally a plain user-home dotfile, **not**
//! Tauri's `app_data_dir()` — see the comment in `statusline/mod.rs` for
//! why (macOS Sequoia's App Data Access TCC).

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::scripts;

/// Result reported back to the frontend after `install_statusline`. Errors
/// are returned as `Err(String)` from the IPC layer; this enum captures the
/// successful states we want the UI to differentiate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum InstallOutcome {
    /// Wrote the script + patched settings.json. The previous statusline
    /// command (if any) is included so the UI can offer a chain-it-back
    /// follow-up.
    Installed { previous_command: Option<String> },
    /// Settings.json already pointed at our script — no change needed.
    AlreadyInstalled,
}

/// Result of `check_statusline_installed`. Granular states let the
/// onboarding UI distinguish "needs install" from "needs reinstall".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum InstalledState {
    /// settings.json's statusLine.command points at our script *and* the
    /// script exists on disk.
    Installed,
    /// settings.json has no statusLine, or it points elsewhere.
    NotInstalled,
    /// settings.json points at our script, but the script file is missing.
    /// The user probably wiped `~/.tokenmonitor/`; reinstall will re-create.
    ScriptMissing,
}

/// Resolve `~/.claude/settings.json`. Honors `$CLAUDE_CONFIG_DIR` (single
/// path, comma form not supported here — we'd write into the first dir, but
/// the read path varies, and we'd rather refuse than guess wrong).
fn claude_settings_path() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("CLAUDE_CONFIG_DIR") {
        let first = raw.split(',').map(str::trim).find(|p| !p.is_empty())?;
        return Some(PathBuf::from(first).join("settings.json"));
    }
    dirs::home_dir().map(|h| h.join(".claude").join("settings.json"))
}

fn load_settings(path: &Path) -> io::Result<Map<String, Value>> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let raw = fs::read_to_string(path)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Map::new());
    }
    let value: Value =
        serde_json::from_str(trimmed).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "settings.json root is not an object",
        )),
    }
}

fn save_settings(path: &Path, settings: &Map<String, Value>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(&Value::Object(settings.clone()))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    // Write to a sibling temp + rename so a crash mid-write can't leave the
    // user with a corrupt settings file (CC reads it on startup).
    let tmp = path.with_extension("json.tokenmonitor.tmp");
    fs::write(&tmp, serialized)?;
    fs::rename(&tmp, path)
}

fn extract_status_line_command(settings: &Map<String, Value>) -> Option<String> {
    settings
        .get("statusLine")
        .and_then(|v| v.get("command"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    // Windows doesn't use the unix exec bit. PowerShell -File invocation
    // runs the script directly; nothing to set.
    Ok(())
}

/// Write the script to disk and patch CC's settings.json. Returns
/// `InstallOutcome` describing what changed.
pub fn install() -> Result<InstallOutcome, String> {
    let dir = super::statusline_dir();
    let events_file = super::events_file();
    let script_path = super::script_path();

    fs::create_dir_all(&dir).map_err(|e| format!("create statusline dir: {e}"))?;

    let body = scripts::render(
        dir.to_string_lossy().as_ref(),
        events_file.to_string_lossy().as_ref(),
    );
    fs::write(&script_path, body).map_err(|e| format!("write script: {e}"))?;
    make_executable(&script_path).map_err(|e| format!("chmod script: {e}"))?;

    let settings_path = claude_settings_path()
        .ok_or_else(|| "Could not resolve ~/.claude/settings.json".to_string())?;
    let mut settings = load_settings(&settings_path)
        .map_err(|e| format!("read {}: {e}", settings_path.display()))?;

    let our_command = scripts::settings_command(script_path.to_string_lossy().as_ref());
    let previous = extract_status_line_command(&settings);
    if previous.as_deref() == Some(our_command.as_str()) {
        return Ok(InstallOutcome::AlreadyInstalled);
    }

    // One-shot backup the first time we touch the file. We never overwrite
    // an existing backup — the user might have been mid-edit when they hit
    // re-install and we'd rather keep their oldest known-good copy.
    if settings_path.exists() {
        let backup = settings_path.with_extension("json.tokenmonitor.bak");
        if !backup.exists() {
            let _ = fs::copy(&settings_path, &backup);
        }
    }

    settings.insert(
        "statusLine".to_string(),
        json!({ "type": "command", "command": our_command }),
    );

    save_settings(&settings_path, &settings)
        .map_err(|e| format!("write {}: {e}", settings_path.display()))?;

    Ok(InstallOutcome::Installed {
        previous_command: previous,
    })
}

/// Remove our `statusLine` entry from settings.json. Leaves the script on
/// disk — easier to reinstall and harmless if abandoned.
pub fn uninstall() -> Result<(), String> {
    let script_path = super::script_path();
    let settings_path = claude_settings_path()
        .ok_or_else(|| "Could not resolve ~/.claude/settings.json".to_string())?;

    if !settings_path.exists() {
        return Ok(());
    }
    let mut settings = load_settings(&settings_path).map_err(|e| format!("read settings: {e}"))?;

    let our_command = scripts::settings_command(script_path.to_string_lossy().as_ref());
    let current = extract_status_line_command(&settings);

    // Only remove the field when it currently points at our script. If the
    // user installed something else, leave it alone — they own that key.
    if current.as_deref() == Some(our_command.as_str()) {
        settings.remove("statusLine");
        save_settings(&settings_path, &settings).map_err(|e| format!("write settings: {e}"))?;
    }

    Ok(())
}

/// Probe the install state — used by the onboarding UI to render the right
/// CTA without trying to flip anything.
pub fn check() -> InstalledState {
    let script_path = super::script_path();
    let Some(settings_path) = claude_settings_path() else {
        return InstalledState::NotInstalled;
    };
    let Ok(settings) = load_settings(&settings_path) else {
        return InstalledState::NotInstalled;
    };
    let our_command = scripts::settings_command(script_path.to_string_lossy().as_ref());
    match extract_status_line_command(&settings).as_deref() {
        Some(cmd) if cmd == our_command => {
            if script_path.exists() {
                InstalledState::Installed
            } else {
                InstalledState::ScriptMissing
            }
        }
        _ => InstalledState::NotInstalled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    /// Cargo runs tests in parallel by default and both
    /// `claude_settings_path` and `statusline_dir` read process-global env
    /// vars, so we serialize tests behind a single mutex. Without this,
    /// concurrent tests would trip over each other's overrides.
    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    /// Run `f` with `CLAUDE_CONFIG_DIR` and `TOKEN_MONITOR_STATUSLINE_DIR`
    /// pointed at fresh tempdirs. Yields the home tempdir + the resolved
    /// claude dir to the closure.
    fn with_isolated_dirs<R>(f: impl FnOnce(&Path, &Path) -> R) -> R {
        // We override env vars rather than HOME because `dirs::home_dir()`
        // on macOS prefers the passwd entry over $HOME — setting HOME
        // doesn't reliably redirect the resolver.
        let _guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        let statusline_dir = tmp.path().join(".tokenmonitor").join("statusline");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::create_dir_all(&statusline_dir).unwrap();
        let prev_claude = std::env::var_os("CLAUDE_CONFIG_DIR");
        let prev_sl = std::env::var_os("TOKEN_MONITOR_STATUSLINE_DIR");
        std::env::set_var("CLAUDE_CONFIG_DIR", &claude_dir);
        std::env::set_var("TOKEN_MONITOR_STATUSLINE_DIR", &statusline_dir);
        let result = f(tmp.path(), &claude_dir);
        match prev_claude {
            Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
        match prev_sl {
            Some(v) => std::env::set_var("TOKEN_MONITOR_STATUSLINE_DIR", v),
            None => std::env::remove_var("TOKEN_MONITOR_STATUSLINE_DIR"),
        }
        result
    }

    #[test]
    fn check_reports_not_installed_for_fresh_home() {
        with_isolated_dirs(|_home, _claude_dir| {
            assert!(matches!(check(), InstalledState::NotInstalled));
        });
    }

    #[test]
    fn install_creates_script_and_patches_settings() {
        with_isolated_dirs(|_home, claude_dir| {
            let outcome = install().unwrap();
            assert!(matches!(outcome, InstallOutcome::Installed { .. }));
            assert!(super::super::script_path().exists());

            let settings_path = claude_dir.join("settings.json");
            let raw = fs::read_to_string(&settings_path).unwrap();
            let parsed: Value = serde_json::from_str(&raw).unwrap();
            let cmd = parsed
                .get("statusLine")
                .and_then(|v| v.get("command"))
                .and_then(|v| v.as_str())
                .unwrap();
            assert!(cmd.contains("tokenmonitor-statusline"));
        });
    }

    #[test]
    fn install_writes_under_dot_tokenmonitor() {
        with_isolated_dirs(|_home, _claude_dir| {
            install().unwrap();
            let script = super::super::script_path();
            // Regression guard: the script path must include the
            // `.tokenmonitor/statusline` segment. If a future refactor
            // accidentally returns the Tauri app_data dir again, this
            // test fails before it can ship.
            let path_str = script.to_string_lossy();
            assert!(
                path_str.contains(".tokenmonitor"),
                "script path {path_str} should be under ~/.tokenmonitor/"
            );
            assert!(path_str.contains("statusline"));
        });
    }

    #[test]
    fn install_preserves_existing_keys_and_backs_up() {
        with_isolated_dirs(|_home, claude_dir| {
            let settings_path = claude_dir.join("settings.json");
            fs::write(
                &settings_path,
                r#"{"theme":"dark","statusLine":{"type":"command","command":"echo hi"}}"#,
            )
            .unwrap();

            install().unwrap();

            let raw = fs::read_to_string(&settings_path).unwrap();
            let parsed: Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(parsed.get("theme").and_then(|v| v.as_str()), Some("dark"));
            let backup = settings_path.with_extension("json.tokenmonitor.bak");
            assert!(backup.exists());
        });
    }

    #[test]
    fn install_returns_already_installed_on_second_call() {
        with_isolated_dirs(|_home, _claude_dir| {
            install().unwrap();
            let second = install().unwrap();
            assert!(matches!(second, InstallOutcome::AlreadyInstalled));
        });
    }

    #[test]
    fn uninstall_removes_only_our_entry() {
        with_isolated_dirs(|_home, claude_dir| {
            install().unwrap();
            uninstall().unwrap();
            let raw = fs::read_to_string(claude_dir.join("settings.json")).unwrap();
            let parsed: Value = serde_json::from_str(&raw).unwrap();
            assert!(parsed.get("statusLine").is_none());
        });
    }

    #[test]
    fn uninstall_leaves_third_party_statusline_alone() {
        with_isolated_dirs(|_home, claude_dir| {
            fs::write(
                claude_dir.join("settings.json"),
                r#"{"statusLine":{"type":"command","command":"some-other-tool"}}"#,
            )
            .unwrap();
            uninstall().unwrap();
            let raw = fs::read_to_string(claude_dir.join("settings.json")).unwrap();
            assert!(raw.contains("some-other-tool"));
        });
    }
}
