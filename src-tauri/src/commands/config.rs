use super::tray::{patch_tray_utilization, sync_tray_title, tray_utilization_from_rate_limits};
use super::{AppState, UsageDebugReport};
use crate::models::*;
use crate::secrets;
use crate::usage::cursor_parser::CursorAuthStatus;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, State};

#[cfg(target_os = "macos")]
static CLAUDE_KEYCHAIN_REQUESTED_THIS_RUN: AtomicBool = AtomicBool::new(false);

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

/// Enable or disable live rate-limit fetching.
///
/// When disabled, the background loop skips `refresh_rate_limits`, so the app
/// never touches the Claude OAuth token in the macOS Keychain. This lets us
/// open the app without firing any Keychain prompt until the user explicitly
/// opts in (via the welcome card or the rate-limits CTA).
#[tauri::command]
pub async fn set_rate_limits_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .rate_limits_enabled
        .store(enabled, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Enable or disable local Claude/Codex session-log reads.
///
/// Brand-new installs keep this off until the welcome disclosure has been
/// dismissed, so any macOS TCC prompt caused by unusual log locations is
/// preceded by app-owned context.
#[tauri::command]
pub async fn set_usage_access_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .usage_access_enabled
        .store(enabled, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Set or refresh the user's Cursor secret.
///
/// **Empty / `None` does NOT clear** persisted credentials — that's reserved
/// for [`clear_cursor_auth_config`]. The reason is that frontend bootstrap
/// passes whatever lives in `settings.json` on every launch (legacy migration
/// path), and we don't want a stale-empty `cursorApiKey` to wipe a perfectly
/// good keyring entry.
///
/// Behavior:
/// - Non-empty input → persist to keyring (preferred) or 0600-perm file
///   fallback, then update the in-memory cache and return the resulting
///   status (with `storage_backend` populated).
/// - Empty / `None` input → leave persisted state alone; if the keyring
///   already has a secret, sync the in-memory cache from it and report
///   that. This is the bootstrap-with-empty-settings.json path.
#[tauri::command]
pub async fn set_cursor_auth_config(
    app: AppHandle,
    api_key: Option<String>,
) -> Result<CursorAuthStatus, String> {
    let trimmed = api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if let Some(value) = trimmed {
        let backend = secrets::cursor::store(&app, Some(&value))?;
        Ok(crate::usage::cursor_parser::set_cursor_auth_config(
            Some(value),
            backend,
        ))
    } else if let Some((existing, backend)) = secrets::cursor::load(&app) {
        Ok(crate::usage::cursor_parser::set_cursor_auth_config(
            Some(existing),
            backend,
        ))
    } else {
        // No user-pasted secret in either layer. Refresh the IDE token
        // cache; if the IDE provides one, the active credential becomes
        // an `IdeBearer` with `StorageBackend::IdeAuto`. Otherwise the
        // user is genuinely not connected.
        let ide_present = crate::usage::cursor_parser::prime_ide_access_token();
        let backend = if ide_present {
            secrets::StorageBackend::IdeAuto
        } else {
            secrets::StorageBackend::None
        };
        Ok(crate::usage::cursor_parser::set_cursor_auth_config(
            None, backend,
        ))
    }
}

/// Hard-clear the user-pasted Cursor secret. Wipes both the keyring entry
/// and the file fallback (best-effort) and resets the override cache. Bound
/// to the Settings UI's "Disconnect" button.
///
/// **The IDE auto-detected token is NOT cleared** — a Disconnect on the
/// pasted-secret layer should fall through to the same zero-config state
/// the user would have had if they'd never pasted anything. To genuinely
/// stop reading from Cursor IDE, the user signs out of the IDE itself
/// (which empties `cursorAuth/accessToken` in `state.vscdb`).
#[tauri::command]
pub async fn clear_cursor_auth_config(app: AppHandle) -> Result<CursorAuthStatus, String> {
    secrets::cursor::store(&app, None)?;
    let ide_present = crate::usage::cursor_parser::prime_ide_access_token();
    let backend = if ide_present {
        secrets::StorageBackend::IdeAuto
    } else {
        secrets::StorageBackend::None
    };
    Ok(crate::usage::cursor_parser::set_cursor_auth_config(
        None, backend,
    ))
}

#[tauri::command]
pub async fn get_cursor_auth_status() -> Result<CursorAuthStatus, String> {
    Ok(crate::usage::cursor_parser::cursor_auth_status())
}

#[tauri::command]
pub async fn retry_cursor_auth() -> Result<CursorAuthStatus, String> {
    let ide_present = crate::usage::cursor_parser::prime_ide_access_token();
    let backend = if ide_present {
        secrets::StorageBackend::IdeAuto
    } else {
        secrets::StorageBackend::None
    };
    Ok(crate::usage::cursor_parser::set_cursor_auth_config(
        None, backend,
    ))
}

#[tauri::command]
pub async fn open_cursor_app() -> Result<(), String> {
    launch_cursor().map_err(|e| format!("Failed to launch Cursor: {e}"))
}

fn launch_cursor() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-a", "Cursor"])
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        // Try cursor.cmd on PATH first
        if let Ok(_child) = std::process::Command::new("cursor.cmd")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
        {
            return Ok(());
        }

        // Fallback to known install location
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let exe = std::path::PathBuf::from(local_app_data)
                .join("Programs")
                .join("Cursor")
                .join("Cursor.exe");
            if exe.is_file() {
                std::process::Command::new(&exe)
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
                    .map_err(|e| e.to_string())?;
                return Ok(());
            }
        }

        Err("Cursor not found on PATH or in default install location".into())
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("cursor")
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("Unsupported platform".into())
    }
}

/// Hydrate the in-memory Cursor secret state from disk so the very first
/// usage refresh after launch can hit the remote API without waiting for
/// the frontend bootstrap to round-trip an IPC call.
///
/// Two layers, both best-effort:
///
/// 1. **User-pasted secret** — keyring (preferred) or 0600-perm file
///    fallback. If present, it's loaded into the override cache and the
///    storage backend is reported as `Keyring` / `File`.
/// 2. **Auto-detected IDE bearer** — Cursor IDE writes its current access
///    token to `~/Library/Application Support/Cursor/.../state.vscdb` and
///    rotates it on its own schedule. We populate the IDE token cache
///    independently of the user-pasted layer; [`resolve_cursor_auth`]
///    treats it as the lowest-priority fallback so an explicit user
///    paste always wins. When this is the *only* credential available
///    we additionally mark the storage backend as `IdeAuto` so the UI
///    can surface "Connected via Cursor IDE" instead of "Not connected".
///
/// Failures are silent — a missing/locked keychain or absent Cursor IDE
/// just leaves the corresponding layer empty.
pub fn prime_cursor_auth_from_disk(app: &AppHandle) {
    let user_secret_loaded = match secrets::cursor::load(app) {
        Some((value, backend)) => {
            let _ = crate::usage::cursor_parser::set_cursor_auth_config(Some(value), backend);
            true
        }
        None => false,
    };

    // Always try priming the IDE token, even if a user secret was loaded:
    // the user might later clear their pasted token via the Disconnect
    // button, at which point we want to silently fall through to IDE auth
    // without a restart.
    let ide_token_present = crate::usage::cursor_parser::prime_ide_access_token();

    if !user_secret_loaded && ide_token_present {
        // No user-pasted secret, but the IDE has a token — surface the
        // "auto-detected" backend so the Settings UI can render a
        // "Connected via Cursor IDE" badge without persisting anything.
        let _ = crate::usage::cursor_parser::set_cursor_auth_config(
            None,
            secrets::StorageBackend::IdeAuto,
        );
    }
}

/// Outcome of the one-time interactive Keychain prompt. Surfaced to the
/// frontend so it can show appropriate copy after the user responds.
///
/// Each variant is constructed on a different OS path (Granted/Denied on
/// macOS, NotApplicable everywhere else), so per-target dead-code analysis
/// flags the ones not used on the current platform. Since the enum is the
/// IPC contract — every variant is "live" from the frontend's perspective —
/// suppress the lint at the enum level instead of per-variant.
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "snake_case", tag = "status")]
#[allow(dead_code)]
pub enum KeychainAccessOutcome {
    /// User granted access (or access was already silently available).
    Granted,
    /// User denied the prompt, the item is missing, or read failed.
    Denied { reason: String },
    /// Keychain isn't part of the credentials path on this platform.
    NotApplicable,
    /// The interactive request has already been attempted in this app process.
    AlreadyRequested,
}

/// Request the one-time interactive Keychain prompt for the Claude OAuth
/// token. This is the **only** code path that allows the macOS Keychain UI
/// to appear — every other read is silent (`skip_authenticated_items`).
///
/// Whatever the user chooses, the frontend should persist
/// `keychainAccessRequested = true` so this prompt never recurs on its own.
#[tauri::command]
pub async fn request_claude_keychain_access() -> Result<KeychainAccessOutcome, String> {
    #[cfg(target_os = "macos")]
    {
        if CLAUDE_KEYCHAIN_REQUESTED_THIS_RUN.swap(true, Ordering::SeqCst) {
            return Ok(KeychainAccessOutcome::AlreadyRequested);
        }

        // Run the synchronous Keychain call on a blocking thread so we don't
        // pin the Tauri async runtime while macOS shows the auth panel.
        let outcome =
            tokio::task::spawn_blocking(crate::rate_limits::request_claude_keychain_access)
                .await
                .map_err(|e| format!("Keychain access task failed: {e}"))?;

        Ok(match outcome {
            Ok(()) => KeychainAccessOutcome::Granted,
            Err(reason) => KeychainAccessOutcome::Denied { reason },
        })
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(KeychainAccessOutcome::NotApplicable)
    }
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
    if let Some(ssh_cache) = state.ssh_cache.read().await.as_ref() {
        ssh_cache.reset_all_caches();
    }
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
pub async fn get_window_anchor_edge() -> String {
    #[cfg(target_os = "windows")]
    {
        if crate::platform::windows::window::is_anchor_bottom() {
            "bottom".to_string()
        } else {
            "top".to_string()
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        "bottom".to_string()
    }
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
        Some("cursor") => crate::rate_limits::RateLimitSelection::Cursor,
        Some(other) => return Err(format!("Invalid provider for rate limits: {other}")),
    };

    if !state
        .usage_access_enabled
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Ok(state
            .cached_rate_limits
            .read()
            .await
            .clone()
            .unwrap_or(RateLimitsPayload {
                claude: None,
                codex: None,
                cursor: None,
            }));
    }

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

#[tauri::command]
pub async fn get_exchange_rates() -> Result<std::collections::HashMap<String, f64>, String> {
    Ok(crate::usage::exchange_rates::get_all_rates())
}

#[tauri::command]
pub async fn quit_app(app: tauri::AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}
