use super::state::UpdaterState;
use serde_json::{json, Value};
use std::collections::HashSet;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "updater.json";

/// Load persisted updater state. Returns defaults if the store doesn't exist
/// or contains no updater keys.
#[allow(dead_code)]
pub fn load<R: Runtime>(app: &AppHandle<R>) -> UpdaterState {
    let Ok(store) = app.store(STORE_FILE) else {
        return UpdaterState::new();
    };
    let mut state = UpdaterState::new();

    if let Some(Value::Bool(b)) = store.get("auto_check_enabled") {
        state.auto_check_enabled = b;
    }
    if let Some(Value::Array(arr)) = store.get("skipped_versions") {
        state.skipped_versions = arr
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<HashSet<_>>();
    }
    if let Some(Value::String(v)) = store.get("last_notified_version") {
        state.last_notified_version = Some(v);
    }
    if let Some(Value::String(ts)) = store.get("last_check_at") {
        state.last_check = chrono::DateTime::parse_from_rfc3339(&ts)
            .ok()
            .map(|dt| dt.with_timezone(&chrono::Utc));
    }
    state
}

/// Persist the subset of state that survives restarts.
#[allow(dead_code)]
pub fn save<R: Runtime>(app: &AppHandle<R>, state: &UpdaterState) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set("auto_check_enabled", json!(state.auto_check_enabled));
    let skipped: Vec<&String> = state.skipped_versions.iter().collect();
    store.set("skipped_versions", json!(skipped));
    store.set("last_notified_version", json!(state.last_notified_version));
    store.set(
        "last_check_at",
        json!(state.last_check.map(|d| d.to_rfc3339())),
    );
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}
