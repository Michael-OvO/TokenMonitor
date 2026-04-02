use super::tray::{patch_tray_utilization, sync_tray_title, tray_utilization_from_rate_limits};
use super::{AppState, UsageDebugReport};
use crate::models::*;
use tauri::State;

/// Apply native window surface adjustments.
/// On Windows: sets DWM rounded corners. On other platforms: noop.
#[tauri::command]
pub async fn set_window_surface(
    _app: tauri::AppHandle,
    _state: State<'_, AppState>,
    _surface: serde_json::Value,
    _corner_radius: Option<f64>,
) -> Result<(), String> {
    Ok(())
}

/// Noop -- glass effects removed for cross-platform compatibility.
#[tauri::command]
pub async fn set_glass_effect(
    _app: tauri::AppHandle,
    _state: State<'_, AppState>,
    _enabled: bool,
) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn set_refresh_interval(interval: u64, state: State<'_, AppState>) -> Result<(), String> {
    let mut current = state.refresh_interval.write().await;
    *current = interval;
    Ok(())
}

/// Set Dock icon visibility (macOS only). Noop on other platforms.
#[tauri::command]
pub async fn set_dock_icon_visible(app: tauri::AppHandle, visible: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        // Changing the activation policy may deactivate the app, causing the
        // main window to lose focus.  Suppress the resulting auto-hide.
        app.state::<AppState>()
            .suppress_auto_hide
            .store(true, std::sync::atomic::Ordering::SeqCst);

        crate::platform::macos::set_dock_icon_visible(&app, visible)?;

        // Re-focus main window after the policy change so it stays visible,
        // but only if it was already showing — avoid pulling a hidden window
        // to the center of the screen on startup.
        if let Some(win) = app.get_webview_window("main") {
            if win.is_visible().unwrap_or(false) {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, visible);

    Ok(())
}

#[tauri::command]
pub async fn clear_cache(state: State<'_, AppState>) -> Result<(), String> {
    state.parser.clear_cache();
    *state.cached_rate_limits.write().await = None;
    *state.last_usage_debug.write().await = None;
    Ok(())
}

#[tauri::command]
pub async fn clear_payload_cache(state: State<'_, AppState>) -> Result<(), String> {
    state.parser.clear_payload_cache();
    Ok(())
}

#[tauri::command]
pub async fn clear_usage_view_cache(state: State<'_, AppState>) -> Result<(), String> {
    state.parser.clear_payload_cache_prefix("usage-view:");
    Ok(())
}

/// Reposition window so its bottom edge aligns with the work area bottom (taskbar top).
/// Called from the frontend after every window resize.
#[tauri::command]
pub async fn reposition_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "windows")]
        {
            crate::platform::windows::window::align_to_work_area(&window);
        }
        #[cfg(not(target_os = "windows"))]
        {
            crate::platform::clamp_window_to_work_area(&window);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn set_window_size_and_align(
    app: tauri::AppHandle,
    width: f64,
    height: f64,
) -> Result<(), String> {
    use tauri::{LogicalSize, Manager, Size};
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "windows")]
        {
            if let Some(monitor) = window.current_monitor().ok().flatten() {
                let scale = monitor.scale_factor();
                let physical_width = (width * scale).round() as u32;
                let physical_height = (height * scale).round() as u32;
                crate::platform::windows::window::set_size_and_align(
                    &window,
                    physical_width,
                    physical_height,
                );
            } else {
                let _ = window.set_size(Size::Logical(LogicalSize::new(width, height)));
                crate::platform::windows::window::align_to_work_area(&window);
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            // Capture position before resize so we can keep the anchored edge fixed.
            let old_pos = window.outer_position().ok();
            let old_size = window.outer_size().ok();

            let _ = window.set_size(Size::Logical(LogicalSize::new(width, height)));

            // For bottom-anchored windows, move upward so the bottom edge stays put.
            // macOS/Linux keep top-left fixed after set_size, so top-anchored is free.
            if let (Some(pos), Some(old_sz)) = (old_pos, old_size) {
                let old_bottom = pos.y + old_sz.height as i32;
                if let Some(monitor) = window.current_monitor().ok().flatten() {
                    let work_top = monitor.position().y;
                    let work_bottom = monitor.position().y + monitor.size().height as i32;
                    let top_gap = (pos.y - work_top).abs();
                    let bottom_gap = (work_bottom - old_bottom).abs();
                    if top_gap > bottom_gap {
                        let new_size = window.outer_size().unwrap_or(old_sz);
                        let new_y = (old_bottom - new_size.height as i32).max(work_top);
                        if new_y != pos.y {
                            let _ = window.set_position(tauri::PhysicalPosition::new(pos.x, new_y));
                        }
                    }
                }
            }
            crate::platform::clamp_window_to_work_area(&window);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_rate_limits(
    provider: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<RateLimitsPayload, String> {
    let selection = match provider.as_deref() {
        None | Some("all") => crate::rate_limits::RateLimitSelection::All,
        Some("claude") => crate::rate_limits::RateLimitSelection::Claude,
        Some("codex") => crate::rate_limits::RateLimitSelection::Codex,
        Some(other) => return Err(format!("Invalid provider for rate limits: {other}")),
    };

    let codex_dir = state.parser.codex_dir().to_path_buf();
    let cached = state.cached_rate_limits.read().await.clone();
    let fresh =
        crate::rate_limits::fetch_selected_rate_limits(&codex_dir, selection, cached.as_ref())
            .await;

    let merged = crate::rate_limits::merge_rate_limits(fresh, cached.as_ref());

    *state.cached_rate_limits.write().await = Some(merged.clone());
    patch_tray_utilization(&state, tray_utilization_from_rate_limits(Some(&merged))).await;

    sync_tray_title(&app, &state).await;

    Ok(merged)
}

#[tauri::command]
pub async fn get_last_usage_debug(
    state: State<'_, AppState>,
) -> Result<Option<UsageDebugReport>, String> {
    Ok(state.last_usage_debug.read().await.clone())
}
