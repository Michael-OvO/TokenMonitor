//! Persistent storage for the user-supplied Cursor secret.
//!
//! The secret is whatever the user pasted into the Settings UI — either an
//! Enterprise admin API key (`key_…`) or a dashboard `WorkosCursorSessionToken`.
//! Classification happens in `crate::usage::parser`; this module is
//! intentionally agnostic about the secret's shape.
//!
//! Storage policy (best-effort across two layers):
//!   1. **OS keyring** via the `keyring` crate. macOS uses Keychain Services,
//!      Windows uses Credential Manager, Linux uses the FreeDesktop Secret
//!      Service over D-Bus. This is the preferred path: secrets never touch
//!      the filesystem in plaintext.
//!   2. **Filesystem fallback** at `<app_data>/cursor-secret.txt` with
//!      `0600` perms on Unix. Used when the keyring backend is unavailable
//!      (e.g. Linux without `dbus`/`gnome-keyring`, or sandboxed CI).
//!
//! Operations:
//!   - [`store`]`(Some(value))` → persist to keyring (preferred) or file;
//!     clears the other layer to avoid duplicate copies.
//!   - [`store`]`(None)` → best-effort clear across both layers.
//!   - [`load`] → returns whichever layer has a value, or `None`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use super::StorageBackend;

/// Service name registered with the OS keyring. Stable across versions; do
/// not change without a migration path or users will lose their secrets.
const KEYRING_SERVICE: &str = "TokenMonitor::Cursor";
/// Account name within the service. Single secret per service for now.
const KEYRING_ACCOUNT: &str = "cursor-secret";
/// Filename used for the disk fallback when keyring is unavailable.
const FALLBACK_FILENAME: &str = "cursor-secret.txt";

/// Persist (or clear) the Cursor secret. Returns the backend that was used.
///
/// `None` means the caller is asking for a hard clear; both layers are
/// cleared best-effort. `Some(value)` writes to whichever layer accepts.
pub fn store(app: &AppHandle, secret: Option<&str>) -> Result<StorageBackend, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot resolve app data dir: {e}"))?;
    store_in_dir(&app_data, secret)
}

/// Read the currently persisted secret, if any. Tries keyring first.
pub fn load(app: &AppHandle) -> Option<(String, StorageBackend)> {
    let app_data = app.path().app_data_dir().ok()?;
    load_from_dir(&app_data)
}

// ── Internals (Path-based for testability) ───────────────────────────────────

fn store_in_dir(app_data: &Path, secret: Option<&str>) -> Result<StorageBackend, String> {
    let normalized = secret.map(str::trim).filter(|s| !s.is_empty());

    if let Some(value) = normalized {
        match try_set_keyring(value) {
            Ok(()) => {
                // Clear any stale fallback file so the secret never lives
                // in two places at once.
                let _ = remove_fallback_file(app_data);
                return Ok(StorageBackend::Keyring);
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Cursor secret keyring write failed; falling back to file"
                );
            }
        }
        write_fallback_file(app_data, value)?;
        // We tried the keyring above and it failed; if a stale keyring item
        // somehow exists, leave it for now — the next successful write will
        // overwrite it. (Best-effort.)
        Ok(StorageBackend::File)
    } else {
        // Hard clear request: best-effort across both layers.
        let _ = try_clear_keyring();
        let _ = remove_fallback_file(app_data);
        Ok(StorageBackend::None)
    }
}

fn load_from_dir(app_data: &Path) -> Option<(String, StorageBackend)> {
    if let Some(value) = try_get_keyring() {
        return Some((value, StorageBackend::Keyring));
    }
    read_fallback_file(app_data).map(|v| (v, StorageBackend::File))
}

// ── Keyring layer ────────────────────────────────────────────────────────────

fn try_set_keyring(value: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("keyring entry: {e}"))?;
    entry
        .set_password(value)
        .map_err(|e| format!("keyring set: {e}"))?;
    Ok(())
}

fn try_get_keyring() -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).ok()?;
    match entry.get_password() {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(keyring::Error::NoEntry) => None,
        Err(other) => {
            tracing::debug!(error = %other, "Cursor keyring read failed");
            None
        }
    }
}

fn try_clear_keyring() -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("keyring entry: {e}"))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(other) => Err(format!("keyring delete: {other}")),
    }
}

// ── File-fallback layer ──────────────────────────────────────────────────────

fn fallback_path(app_data: &Path) -> PathBuf {
    app_data.join(FALLBACK_FILENAME)
}

fn write_fallback_file(app_data: &Path, value: &str) -> Result<(), String> {
    fs::create_dir_all(app_data).map_err(|e| format!("create app data dir: {e}"))?;
    let path = fallback_path(app_data);
    let mut file = fs::File::create(&path).map_err(|e| format!("create secret file: {e}"))?;
    file.write_all(value.as_bytes())
        .map_err(|e| format!("write secret file: {e}"))?;
    file.sync_all().ok();
    set_owner_only_perms(&path)?;
    Ok(())
}

fn read_fallback_file(app_data: &Path) -> Option<String> {
    let path = fallback_path(app_data);
    let raw = fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn remove_fallback_file(app_data: &Path) -> Result<(), String> {
    let path = fallback_path(app_data);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove secret file: {e}")),
    }
}

#[cfg(unix)]
fn set_owner_only_perms(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, perms).map_err(|e| format!("set perms: {e}"))
}

#[cfg(not(unix))]
fn set_owner_only_perms(_path: &Path) -> Result<(), String> {
    // On Windows we rely on the per-user app data directory ACL set by the
    // Tauri bundler. Fine-grained NTFS ACLs are out of scope for now.
    Ok(())
}

#[cfg(test)]
mod tests {
    //! These tests cover the file-fallback layer only — exercising the
    //! keyring layer would write to the host's actual keychain, which is
    //! unfriendly for CI and developer machines. Round-trip via the keyring
    //! path is verified manually during dev (paste token in Settings →
    //! restart → confirm it reloads).

    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_then_read_round_trip_via_file_fallback() {
        let dir = TempDir::new().unwrap();
        write_fallback_file(dir.path(), "user_01ABCD::secret").unwrap();
        let value = read_fallback_file(dir.path()).unwrap();
        assert_eq!(value, "user_01ABCD::secret");
    }

    #[test]
    fn read_returns_none_when_file_absent() {
        let dir = TempDir::new().unwrap();
        assert!(read_fallback_file(dir.path()).is_none());
    }

    #[test]
    fn read_returns_none_for_blank_file() {
        let dir = TempDir::new().unwrap();
        let path = fallback_path(dir.path());
        fs::write(&path, "   \n\t").unwrap();
        assert!(read_fallback_file(dir.path()).is_none());
    }

    #[test]
    fn read_strips_surrounding_whitespace() {
        let dir = TempDir::new().unwrap();
        let path = fallback_path(dir.path());
        fs::write(&path, "  abc-token  \n").unwrap();
        assert_eq!(read_fallback_file(dir.path()).unwrap(), "abc-token");
    }

    #[test]
    fn remove_is_idempotent() {
        let dir = TempDir::new().unwrap();
        // No file yet — should still succeed.
        remove_fallback_file(dir.path()).unwrap();
        // Create then remove twice in a row.
        write_fallback_file(dir.path(), "x").unwrap();
        remove_fallback_file(dir.path()).unwrap();
        remove_fallback_file(dir.path()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_sets_owner_only_perms() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        write_fallback_file(dir.path(), "secret").unwrap();
        let metadata = fs::metadata(fallback_path(dir.path())).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0600 perms, got {mode:o}");
    }
}
