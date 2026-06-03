//! Minimal Tauri-free stub of `crate::secrets`.
//!
//! The real module (src-tauri/src/secrets/) links Tauri + keyring. The only
//! item the included parser code references is `StorageBackend` (used by
//! cursor_parser.rs), so we replicate just that enum, matching the original's
//! variants and derives exactly.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackend {
    Keyring,
    File,
    IdeAuto,
    #[default]
    None,
}
