//! Secret persistence layer.
//!
//! Each integration that needs to keep a user-supplied credential across
//! restarts has its own submodule here, all of which share the same
//! "OS keyring first, encrypted-permission file second" strategy via the
//! `keyring` crate.
//!
//! The Cursor module ([`cursor`]) is the first consumer; future integrations
//! (e.g. an OpenRouter API key) can be added alongside it without changing
//! the general policy.

pub mod cursor;

/// Where a persisted secret currently lives. Surfaced to the frontend so the
/// Settings UI can display a "Stored in: Keychain" / "Stored in: Local file"
/// / "Auto-detected from Cursor IDE" badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackend {
    /// OS keyring: macOS Keychain, Windows Credential Manager, or Linux
    /// Secret Service / libsecret.
    Keyring,
    /// Plain file inside the app data dir, with `0600` perms on Unix.
    /// Used as a last-resort fallback when the keyring backend is
    /// unavailable (Linux without dbus, sandboxed CI, etc.).
    File,
    /// Auto-detected from a sibling app's local storage. Currently used
    /// only by the Cursor integration, which can read the IDE's own
    /// access token from `state.vscdb` for a zero-config setup. The
    /// secret never touches our own persistence layer — Cursor IDE
    /// owns the token lifecycle (it refreshes the JWT on its own
    /// schedule), and we re-read it before each remote call.
    IdeAuto,
    /// No secret persisted.
    #[default]
    None,
}
