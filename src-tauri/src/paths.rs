//! Central registry of every filesystem path the app may read.
//!
//! Every non-test `fs::read_dir` / `fs::read_to_string` / `File::open` outside
//! the app's own data directory should resolve its path through one of the
//! functions here, so that the full set of touched locations stays visible and
//! auditable in one file. This helps reason about macOS TCC prompts, document
//! privacy behavior, and surface a "what does this app read?" view in the UI.
//!
//! Paths are not created or watched here — this is purely a lookup layer.

use std::env;
use std::path::PathBuf;

/// Describes one path (or set of paths) the app may read at runtime, along
/// with the reason and whether the user can override the location.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AccessedPath {
    pub purpose: &'static str,
    pub path: PathBuf,
    pub env_override: Option<&'static str>,
}

fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

fn config() -> Option<PathBuf> {
    dirs::config_dir()
}

/// Default Claude Code project-logs roots. May be overridden by
/// `$CLAUDE_CONFIG_DIR` (comma-separated list of directories). The env-var
/// branch is handled in `usage::integrations` to preserve existing caller
/// semantics; this function returns the *default* roots only.
pub fn claude_project_roots_default() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(cfg) = config() {
        roots.push(cfg.join("claude").join("projects"));
    }
    if let Some(h) = home() {
        roots.push(h.join(".claude").join("projects"));
    }
    roots
}

/// Default Codex session-logs root. May be overridden by `$CODEX_HOME`.
pub fn codex_sessions_default() -> Option<PathBuf> {
    home().map(|h| h.join(".codex").join("sessions"))
}

/// `~/.ssh/config` — read only when SSH remote devices are configured.
pub fn ssh_config() -> Option<PathBuf> {
    home().map(|h| h.join(".ssh").join("config"))
}

/// Claude credentials JSON (non-macOS fallback when Keychain isn't used).
#[cfg_attr(not(test), allow(dead_code))]
pub fn claude_credentials_file() -> Option<PathBuf> {
    env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .ok()
        .filter(|p| p.is_dir())
        .or_else(|| home().map(|h| h.join(".claude")))
        .map(|p| p.join(".credentials.json"))
}

/// Enumerate every path the app *may* read, for audit and UI display.
#[cfg_attr(not(test), allow(dead_code))]
pub fn accessed_paths() -> Vec<AccessedPath> {
    let mut out = Vec::new();
    for p in claude_project_roots_default() {
        out.push(AccessedPath {
            purpose: "Claude Code session logs",
            path: p,
            env_override: Some("CLAUDE_CONFIG_DIR"),
        });
    }
    if let Some(p) = codex_sessions_default() {
        out.push(AccessedPath {
            purpose: "Codex CLI session logs",
            path: p,
            env_override: Some("CODEX_HOME"),
        });
    }
    if let Some(p) = ssh_config() {
        out.push(AccessedPath {
            purpose: "SSH host discovery (only when remote devices configured)",
            path: p,
            env_override: None,
        });
    }
    if let Some(p) = claude_credentials_file() {
        out.push(AccessedPath {
            purpose: "Claude OAuth credentials (non-macOS only; macOS uses Keychain)",
            path: p,
            env_override: Some("CLAUDE_CONFIG_DIR"),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessed_paths_includes_claude_and_codex_and_ssh() {
        let set: Vec<_> = accessed_paths().iter().map(|p| p.purpose).collect();
        assert!(set.iter().any(|p| p.contains("Claude Code")));
        assert!(set.iter().any(|p| p.contains("Codex")));
        assert!(set.iter().any(|p| p.contains("SSH")));
    }

    #[test]
    fn claude_project_roots_default_is_deterministic() {
        // Same inputs → same paths across two calls (no hidden state).
        assert_eq!(
            claude_project_roots_default(),
            claude_project_roots_default()
        );
    }
}
