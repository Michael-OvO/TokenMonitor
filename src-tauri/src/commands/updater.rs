use super::AppState;
use crate::updater::persistence;
use crate::updater::scheduler;
use crate::updater::state::{DownloadProgress, UpdaterState};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterStatusPayload {
    pub state: UpdaterState,
    pub current_version: String,
    /// How the frontend should present the install action.
    /// "auto" → call updater_install; "manual" → open GitHub release page in browser.
    pub install_mode: &'static str,
}

/// Returns "auto" unless running from a `.deb` install on Linux
/// (detected by absence of `APPIMAGE` env var on a Linux target).
fn install_mode() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("APPIMAGE").is_some() {
            "auto"
        } else {
            "manual"
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        "auto"
    }
}

#[tauri::command]
pub async fn updater_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdaterStatusPayload, String> {
    let guard = state.updater.read().await;
    Ok(UpdaterStatusPayload {
        state: guard.clone(),
        current_version: app.package_info().version.to_string(),
        install_mode: install_mode(),
    })
}

#[tauri::command]
pub async fn updater_check_now(app: AppHandle) -> Result<(), String> {
    scheduler::run_check(&app).await
}

#[tauri::command]
pub async fn updater_set_auto_check(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    {
        let mut guard = state.updater.write().await;
        guard.auto_check_enabled = enabled;
        persistence::save(&app, &guard)?;
    }
    let _ = app.emit("updater://status-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn updater_skip_version(
    app: AppHandle,
    state: State<'_, AppState>,
    version: String,
) -> Result<(), String> {
    {
        let mut guard = state.updater.write().await;
        guard.skipped_versions.insert(version);
        persistence::save(&app, &guard)?;
    }
    let _ = app.emit("updater://status-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn updater_dismiss(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut guard = state.updater.write().await;
        guard.dismissed_for_session = true;
    }
    let _ = app.emit("updater://status-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn updater_install(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No update available".to_string())?;

    // Progress-reporting download + install. Emits updater://progress on every chunk.
    let app_clone = app.clone();
    let state_clone = state.updater.clone();
    let download_result = update
        .download_and_install(
            move |chunk, total| {
                let app = app_clone.clone();
                let state = state_clone.clone();
                tauri::async_runtime::spawn(async move {
                    let mut guard = state.write().await;
                    let downloaded = guard
                        .progress
                        .as_ref()
                        .map(|p| p.downloaded + chunk as u64)
                        .unwrap_or(chunk as u64);
                    let percent = total.map(|t| (downloaded as f32 / t as f32) * 100.0);
                    let progress = DownloadProgress {
                        downloaded,
                        total,
                        percent,
                    };
                    guard.progress = Some(progress.clone());
                    drop(guard);
                    let _ = app.emit("updater://progress", progress);
                });
            },
            || {
                // Download finished callback — nothing extra to do.
            },
        )
        .await;

    match download_result {
        Ok(()) => {
            app.restart();
        }
        Err(e) => {
            let msg = e.to_string();
            let mut guard = state.updater.write().await;
            guard.progress = None;
            guard.last_check_error = Some(msg.clone());
            drop(guard);
            let _ = app.emit("updater://status-changed", ());
            Err(msg)
        }
    }
}
