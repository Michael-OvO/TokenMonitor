use super::tray::{patch_tray_utilization, sync_tray_title, tray_utilization_from_rate_limits};
use super::{AppState, UsageDebugReport};
use crate::models::*;
use crate::secrets;
use crate::usage::cursor_parser::CursorAuthStatus;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn set_refresh_interval(interval: u64, state: State<'_, AppState>) -> Result<(), String> {
    let mut current = state.refresh_interval.write().await;
    *current = interval;
    Ok(())
}

/// Push the display currency the user picked in Settings down to Rust.
///
/// The tray title, the Cursor API meter label and the float-ball amount are all
/// rendered on this side, so without this they keep printing dollars while the
/// popover shows euros. Re-renders the tray straight away — the menu bar should
/// change when the setting does, not at the next refresh tick.
#[tauri::command]
pub async fn set_currency(
    code: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::usage::money::set_active_currency(&code);
    super::tray::apply_tray_title_now(&app, &state).await;
    Ok(())
}

/// Enable or disable live rate-limit fetching.
///
/// When disabled, the background loop skips `refresh_rate_limits`, so the app
/// spawns no CLI probe and reads no Claude credentials until the user
/// explicitly opts in (via the welcome card or the rate-limits CTA).
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

/// Result of an App Data TCC probe. We can't query macOS directly for
/// the user's recorded TCC decision, so we infer it from a `read_dir`
/// outcome: success means access was granted (or never required because
/// the directory doesn't exist on this machine); a permission error means
/// it was denied (or never asked).
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "snake_case", tag = "status")]
#[allow(dead_code)]
pub enum AppDataAccessState {
    /// At least one root is readable, *or* none of the roots exist (so no
    /// prompt would ever fire — treat as a no-op grant).
    Granted,
    /// All existing roots returned a permission error. The user either
    /// previously denied the prompt or it has never been answered. Either
    /// way, the next step is System Settings — no further `read_dir` will
    /// re-fire the sheet.
    Denied,
    /// macOS Sequoia (App Data TCC) doesn't apply on this OS.
    NotApplicable,
}

/// Probe Claude Code / Codex CLI session-log roots to determine the App
/// Data TCC state without firing the user-facing prompt — the prompt only
/// fires on the *first* `read_dir` after a fresh install / TCC reset.
/// After that, this call returns the cached decision silently.
///
/// macOS only; other OSes return `NotApplicable` because there's no App
/// Data TCC layer for them to deny.
#[tauri::command]
pub async fn check_app_data_access() -> Result<AppDataAccessState, String> {
    #[cfg(target_os = "macos")]
    {
        use std::fs;
        use std::io::ErrorKind;

        let mut roots: Vec<std::path::PathBuf> = crate::paths::claude_project_roots_default();
        if let Some(p) = crate::paths::codex_sessions_default() {
            roots.push(p);
        }

        let mut any_existing = false;
        let mut any_readable = false;
        let mut any_permission_denied = false;

        for root in &roots {
            // `metadata()` doesn't trigger AppData TCC — it only checks the
            // path's *existence*. If the path doesn't exist, no permission
            // question applies.
            if !root.exists() {
                continue;
            }
            any_existing = true;
            match fs::read_dir(root) {
                Ok(_) => {
                    any_readable = true;
                }
                Err(err) if matches!(err.kind(), ErrorKind::PermissionDenied) => {
                    any_permission_denied = true;
                }
                Err(_) => {
                    // Other errors (EIO, ENOTDIR, etc.) — don't infer from these.
                }
            }
        }

        if !any_existing {
            return Ok(AppDataAccessState::Granted);
        }
        if any_readable {
            return Ok(AppDataAccessState::Granted);
        }
        if any_permission_denied {
            return Ok(AppDataAccessState::Denied);
        }
        // No clear signal — treat as Denied so the UI surfaces the action.
        Ok(AppDataAccessState::Denied)
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(AppDataAccessState::NotApplicable)
    }
}

/// Force a `read_dir` against Claude Code / Codex CLI session-log roots.
/// On Sequoia this triggers the App Data TCC prompt the *first* time it's
/// called for a given app/path pair. After the user answers, this call
/// becomes a noop and the answer is read from
/// [`check_app_data_access`].
#[tauri::command]
pub async fn request_app_data_access() -> Result<u32, String> {
    use std::fs;

    let mut probed: u32 = 0;
    let mut roots: Vec<std::path::PathBuf> = crate::paths::claude_project_roots_default();
    if let Some(p) = crate::paths::codex_sessions_default() {
        roots.push(p);
    }

    for root in roots {
        let _ = fs::read_dir(&root);
        probed += 1;
        tracing::debug!(path = %root.display(), "Requested App Data TCC for path");
    }

    Ok(probed)
}

/// Open the macOS System Settings pane where the user can manage App Data
/// permissions. Used when [`check_app_data_access`] returns `Denied` —
/// the OS won't re-fire the prompt, so the user has to flip the switch
/// themselves. macOS only.
#[tauri::command]
pub async fn open_app_data_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        // The "App Management" pane on Sequoia covers App Data access.
        // Apple doesn't expose a deeper anchor, so we land on the Privacy
        // & Security root if the App-Management URL fails.
        let urls = [
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AppBundles",
            "x-apple.systempreferences:com.apple.preference.security?Privacy",
        ];
        for url in urls {
            if Command::new("open").arg(url).status().is_ok() {
                return Ok(());
            }
        }
        Err("Failed to open System Settings".to_string())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
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

/// Suppress the next main-window auto-hide blur. Call this before opening a
/// native OS dialog (Save/Open panel) that steals focus from the webview —
/// without it the blur handler hides the window while the dialog is up,
/// leaving the user unable to click the originating button afterwards.
#[tauri::command]
pub fn suppress_next_auto_hide(state: State<'_, AppState>) {
    state
        .suppress_auto_hide
        .store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Update the background auto-export preferences. Mirrors the Settings toggle
/// and the chosen destination folder; the refresh loop reads this each tick.
/// The frontend is the source of truth and always passes the full state, so we
/// overwrite both fields verbatim (a `None` folder simply clears it).
///
/// This command never touches the filesystem — it only flips prefs and, on a
/// folder change, resets the auto-export runtime so the next tick writes a fresh
/// full JSONL file to the new destination (the old cursors are meaningless for a
/// different file). The runtime reset is the LAST mutation so the next tick
/// observes the new folder and synced=false together.
#[tauri::command]
pub async fn set_auto_export_config(
    app: tauri::AppHandle,
    enabled: bool,
    folder: Option<String>,
    hidden_models: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let hidden =
        crate::commands::usage_io::normalize_hidden_models(hidden_models.unwrap_or_default());
    let has_folder = folder.is_some();
    let needs_rewrite = {
        let mut cfg = state.auto_export.write().await;
        // A folder OR hidden-models change both invalidate the existing mirror:
        // the folder points the writer at a fresh file, and a hidden-models change
        // means previously-written rows must be re-filtered (a now-hidden model's
        // rows dropped, a now-visible model's rows restored). Either way the next
        // tick must do a full rewrite rather than an incremental append.
        let changed = cfg.folder != folder || cfg.hidden_models != hidden;
        cfg.enabled = enabled;
        cfg.folder = folder;
        cfg.hidden_models = hidden;
        changed
    };
    if needs_rewrite {
        let mut rt = state.auto_export_runtime.write().await;
        rt.synced = false;
        rt.cursors.clear();
    }

    // Kick an immediate sync so a freshly-configured folder pulls peers in right
    // away — e.g. on launch when the frontend pushes this config — instead of
    // waiting for the next refresh tick. Spawned so the IPC call returns promptly.
    if has_folder {
        use tauri::Manager;
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app2.state::<AppState>();
            crate::commands::usage_io::run_auto_export(&app2, &state).await;
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn clear_cache(state: State<'_, AppState>) -> Result<(), String> {
    state.parser.clear_cache();
    if let Some(ref disk_cache) = *state.payload_disk_cache.read().await {
        disk_cache.clear_all();
    }
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
    if let Some(ref disk_cache) = *state.payload_disk_cache.read().await {
        disk_cache.clear_all();
    }
    Ok(())
}

#[tauri::command]
pub async fn set_window_size_and_align(
    app: tauri::AppHandle,
    width: f64,
    height: f64,
) -> Result<(), String> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "windows")]
        {
            // `set_size_and_align` derives the work area from the window's
            // monitor via MonitorFromWindow (Win32), which still succeeds when
            // Tauri's `current_monitor()` returns None — it intermittently does
            // around DPI changes, monitor sleep/wake, and RDP reconnects. The
            // old `else` fallback used a non-atomic `set_size` that keeps the
            // window's TOP-LEFT fixed, so a shrink moved the bottom edge UP and
            // off the taskbar, and the follow-up `align_to_work_area` re-read a
            // stale (pre-resize) rect and could not recover it. Always take the
            // atomic move+resize path, using the window's own scale factor for
            // the logical->physical conversion (no `current_monitor()` needed).
            let scale = window.scale_factor().unwrap_or(1.0);
            let physical_width = (width * scale).round() as u32;
            let physical_height = (height * scale).round() as u32;
            crate::platform::windows::window::set_size_and_align(
                &window,
                physical_width,
                physical_height,
            );
        }
        #[cfg(target_os = "macos")]
        {
            // tao's `set_size` goes through `setContentSize:`, which anchors the
            // window's BOTTOM-left corner: every shrink dropped the popover away
            // from the menu bar and every grow lifted it back. Resize in one
            // atomic `setFrame:display:` on the main thread with the anchored
            // edge pinned instead (see `platform::macos::set_size_keeping_anchor`).
            let w = window.clone();
            let _ = window.run_on_main_thread(move || {
                crate::platform::macos::set_size_keeping_anchor(&w, width, height);
            });
            crate::platform::clamp_window_to_work_area(&window);
        }
        #[cfg(target_os = "linux")]
        {
            use tauri::{LogicalSize, Size};
            let _ = window.set_size(Size::Logical(LogicalSize::new(width, height)));
            // The Linux popover is a top-right tray window that is never user-movable.
            // Unlike macOS, we must NOT preserve the window's current position here:
            // a WM can drift/re-stack the window toward another window (e.g. a file
            // manager that just opened), and the old `clamp`-only path would keep it
            // wherever the WM left it. Re-anchor top-right (drift-gated so it's a
            // no-op when already in place and doesn't cause jitter).
            crate::platform::linux::reanchor_top_right_if_drifted(&window);
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
        Some("kimi") => crate::rate_limits::RateLimitSelection::Kimi,
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
                kimi: None,
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

#[tauri::command]
pub async fn start_cache_warmup(
    app: AppHandle,
    priority_provider: Option<String>,
    priority_period: Option<String>,
) -> Result<u32, String> {
    if crate::usage::cache_warmup::is_warmup_running() {
        return Err("Warmup already running".to_string());
    }
    let provider = priority_provider.unwrap_or_else(|| "all".to_string());
    let period = priority_period.unwrap_or_else(|| "day".to_string());

    tokio::spawn(async move {
        crate::usage::cache_warmup::warmup_payloads(&app, &provider, &period, true).await;
    });

    Ok(0)
}

#[tauri::command]
pub fn cancel_cache_warmup() -> Result<(), String> {
    crate::usage::cache_warmup::cancel_warmup();
    Ok(())
}
