//! Read-modify-write helpers for Claude Code's `settings.json`, shared by the
//! statusline install and the headless-usage (OTel env) setup.

use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// `~/.claude/settings.json`, or `settings.json` in the first
/// `$CLAUDE_CONFIG_DIR` entry.
pub fn settings_path() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("CLAUDE_CONFIG_DIR") {
        let first = raw.split(',').map(str::trim).find(|p| !p.is_empty())?;
        return Some(PathBuf::from(first).join("settings.json"));
    }
    dirs::home_dir().map(|h| h.join(".claude").join("settings.json"))
}

/// Missing or empty file → `{}`. Invalid JSON or a non-object root is an
/// error, so callers refuse to write rather than clobber the user's file.
pub fn load(path: &Path) -> io::Result<Map<String, Value>> {
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

/// Pretty JSON through a sibling temp file + rename.
pub fn save(path: &Path, settings: &Map<String, Value>) -> io::Result<()> {
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

/// Copy the file to `settings.json.tokenmonitor.bak` the first time we touch
/// it; an existing backup is never overwritten.
pub fn backup_once(path: &Path) {
    if path.exists() {
        let backup = path.with_extension("json.tokenmonitor.bak");
        if !backup.exists() {
            let _ = fs::copy(path, &backup);
        }
    }
}
