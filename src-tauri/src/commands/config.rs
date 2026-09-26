use super::tray::{patch_tray_utilization, tray_utilization_from_rate_limits};
use super::{AppState, UsageDebugReport};
use crate::models::*;
use crate::secrets;
use crate::usage::cursor_parser::CursorAuthStatus;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn set_refresh_interval(interval: u64, state: State<'_, AppState>) -> Result<(), String> {
    apply_refresh_interval(&state, interval).await;
    Ok(())
}

/// Store the interval and wake the refresh loop, which re-plans against the
/// new grid at once without running a cycle for it.
pub(crate) async fn apply_refresh_interval(state: &AppState, interval: u64) {
    *state.refresh_interval.write().await = interval;
    state
        .parser
        .set_payload_ttl_secs(crate::usage::parser::payload_ttl_for(interval));
    state.refresh.wake.notify_one();
}

/// Push the week-start day and rolling-window toggle from Settings down to
/// Rust, where every period window is resolved (`commands::period`).
#[tauri::command]
pub async fn set_period_config(week_start: String, rolling: bool) -> Result<(), String> {
    let day: chrono::Weekday = week_start
        .parse()
        .map_err(|_| format!("Unknown weekday: {week_start}"))?;
    super::period::set_period_config(day, rolling);
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
/// When disabled, the refresh cycle skips its rate-limit probes, so the app
/// spawns no CLI probe and reads no Claude credentials until the user
/// explicitly opts in (via the welcome card or the rate-limits CTA).
#[tauri::command]
pub async fn set_rate_limits_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    apply_rate_limits_enabled(&state, enabled);
    Ok(())
}

fn apply_rate_limits_enabled(state: &AppState, enabled: bool) {
    let was_enabled = state
        .rate_limits_enabled
        .swap(enabled, std::sync::atomic::Ordering::SeqCst);
    if enabled && !was_enabled {
        // Nothing has been probed yet: probe now, not at the next tick.
        crate::refresh::request_refresh(state);
    }
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
    apply_usage_access(&state, enabled);
    Ok(())
}

fn apply_usage_access(state: &AppState, enabled: bool) {
    let was_enabled = state
        .usage_access_enabled
        .swap(enabled, std::sync::atomic::Ordering::SeqCst);
    if enabled && !was_enabled {
        // Everything published so far was computed without access.
        crate::refresh::request_refresh(state);
    }
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
/// just leaves the corresponding layer empty. Runs in the background at
/// launch, beside the frontend's own `set_cursor_auth_config`: whichever
/// lands later is newer, so this sets nothing once that one has.
pub fn prime_cursor_auth_from_disk(app: &AppHandle) {
    use crate::usage::cursor_parser::set_cursor_auth_config_if_unchanged;
    let version = crate::usage::cursor_parser::cursor_auth_version();
    let user_secret_loaded = match secrets::cursor::load(app) {
        Some((value, backend)) => {
            set_cursor_auth_config_if_unchanged(version, Some(value), backend);
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
        set_cursor_auth_config_if_unchanged(version, None, secrets::StorageBackend::IdeAuto);
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
        let main = app.get_webview_window("main");
        let showing = main
            .as_ref()
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        // Changing the activation policy may deactivate the app and blur the
        // popover; that blur must not dismiss it. The gate only arms while the
        // popover is showing: at startup it is hidden and cannot blur.
        app.state::<AppState>().auto_hide_gate.arm(showing);

        crate::platform::macos::set_dock_icon_visible(&app, visible)?;

        // Re-focus main window after the policy change so it stays visible,
        // but only if it was already showing — avoid pulling a hidden window
        // to the center of the screen on startup.
        if let Some(win) = main {
            if showing {
                crate::emit_popover_visibility(&win, true);
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
pub fn suppress_next_auto_hide(app: tauri::AppHandle, state: State<'_, AppState>) {
    use tauri::Manager;
    let showing = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    state.auto_hide_gate.arm(showing);
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
            if crate::commands::usage_io::run_auto_export(&app2, &state).await {
                crate::refresh::request_refresh(&state);
            }
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn clear_cache(state: State<'_, AppState>) -> Result<(), String> {
    state.parser.clear_cache();
    if let Some(ssh_cache) = state.ssh_cache.read().await.as_ref() {
        ssh_cache.reset_all_caches();
    }
    {
        // After the probe in flight, which would write its readings back.
        let _io = state.rate_limits_io.lock().await;
        *state.cached_rate_limits.write().await = None;
    }
    *state.last_usage_debug.write().await = None;
    clear_payload_caches(&state).await;
    Ok(())
}

/// Settings calls this after a Cursor auth change, among others.
#[tauri::command]
pub async fn clear_payload_cache(state: State<'_, AppState>) -> Result<(), String> {
    clear_payload_caches(&state).await;
    Ok(())
}

/// Drop every computed view and the Cursor remote data (the Cursor account
/// may have changed), then ask for a refresh: it refetches, recomputes and
/// publishes them together.
async fn clear_payload_caches(state: &AppState) {
    // The Cursor generation bump comes before the clears, so a view still
    // computing from the old data is not cached after them.
    state.parser.clear_cursor_remote();
    // Disk first: the write lock waits out the disk hits in flight, and the
    // memory clear after it drops the copies they made.
    if let Some(ref disk_cache) = *state.payload_disk_cache.write().await {
        disk_cache.clear_all();
    }
    state.parser.clear_payload_cache();
    crate::refresh::request_refresh(state);
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

/// The rate limits the last refresh probed. `force` probes the selection now
/// instead, for explicit user actions (Enable rate limits, Re-grant access).
#[tauri::command]
pub async fn get_rate_limits(
    provider: Option<String>,
    force: Option<bool>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<RateLimitsPayload, String> {
    // "all" follows the enabled header tabs, like the usage views do; a
    // `+`-joined scope names its providers explicitly. Providers outside the
    // selection keep their cached value instead of costing a probe.
    let selection = match provider.as_deref() {
        None | Some("all") => {
            let enabled = state
                .enabled_integrations
                .read()
                .map(|ids| ids.clone())
                .unwrap_or_else(|poisoned| poisoned.into_inner().clone());
            crate::rate_limits::RateLimitSelection::enabled(&enabled)
        }
        Some(scope) => match crate::usage::integrations::UsageIntegrationSelection::parse(scope) {
            Some(parsed) => {
                crate::rate_limits::RateLimitSelection::enabled(&parsed.integration_ids())
            }
            None => return Err(format!("Invalid provider for rate limits: {scope}")),
        },
    };

    let force = force == Some(true);
    let payload = rate_limits_for(&state, selection, force).await;
    if force {
        super::tray::apply_tray_title_now(&app, &state).await;
    }
    Ok(payload)
}

/// How long after a forced request a probe may still start for it.
const FORCED_PROBE_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

/// The providers in `selection` whose reading is the same in `now` as in
/// `before`. A probe that ended while a forced request waited for its turn
/// answered it for the providers it read; probing those again would only
/// queue another probe, or retry an error just got.
fn not_probed_since(
    selection: crate::rate_limits::RateLimitSelection,
    before: Option<&RateLimitsPayload>,
    now: Option<&RateLimitsPayload>,
) -> crate::rate_limits::RateLimitSelection {
    use crate::usage::integrations::UsageIntegrationId as Id;
    let fetched_at = |payload: Option<&RateLimitsPayload>, id: Id| {
        payload
            .and_then(|p| match id {
                Id::Claude => p.claude.as_ref(),
                Id::Codex => p.codex.as_ref(),
                Id::Cursor => p.cursor.as_ref(),
                Id::Kimi => p.kimi.as_ref(),
            })
            .map(|limits| limits.fetched_at.clone())
    };
    let ids: Vec<Id> = [
        (Id::Claude, selection.includes_claude()),
        (Id::Codex, selection.includes_codex()),
        (Id::Cursor, selection.includes_cursor()),
        (Id::Kimi, selection.includes_kimi()),
    ]
    .into_iter()
    .filter(|(id, selected)| *selected && fetched_at(before, *id) == fetched_at(now, *id))
    .map(|(id, _)| id)
    .collect();
    crate::rate_limits::RateLimitSelection::enabled(&ids)
}

pub(crate) async fn rate_limits_for(
    state: &AppState,
    selection: crate::rate_limits::RateLimitSelection,
    force: bool,
) -> RateLimitsPayload {
    if !force || !state.usage_access_enabled() {
        return state
            .cached_rate_limits
            .read()
            .await
            .clone()
            .unwrap_or(RateLimitsPayload {
                claude: None,
                codex: None,
                cursor: None,
                kimi: None,
            });
    }

    let asked = std::time::Instant::now();
    let before = state.cached_rate_limits.read().await.clone();
    let _io = state.rate_limits_io.lock().await;
    let codex_dir = state.parser.codex_dir().to_path_buf();
    let cached = state.cached_rate_limits.read().await.clone();
    let selection = not_probed_since(selection, before.as_ref(), cached.as_ref());
    // A user retry probes a provider whose last probe failed straight away.
    // The budget counts from the ask, so requests queued behind one another
    // cannot add up to minutes: the providers left keep their cached value.
    let fresh = crate::rate_limits::fetch_selected_rate_limits_until(
        &codex_dir,
        selection,
        cached.as_ref(),
        Some(asked + FORCED_PROBE_BUDGET),
        true,
    )
    .await;

    let merged = crate::rate_limits::merge_rate_limits(fresh, cached.as_ref());
    crate::plan_budget::record(&merged);

    *state.cached_rate_limits.write().await = Some(merged.clone());
    patch_tray_utilization(state, tray_utilization_from_rate_limits(Some(&merged))).await;
    merged
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
        crate::usage::cache_warmup::warmup_payloads(&app, &provider, &period).await;
    });

    Ok(0)
}

#[tauri::command]
pub fn cancel_cache_warmup() -> Result<(), String> {
    crate::usage::cache_warmup::cancel_warmup();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::integrations::UsageIntegrationId;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn a_new_refresh_interval_wakes_the_loop_without_requesting_a_cycle() {
        let state = AppState::new();
        apply_refresh_interval(&state, 300).await;

        assert_eq!(*state.refresh_interval.read().await, 300);
        let woken =
            tokio::time::timeout(std::time::Duration::ZERO, state.refresh.wake.notified()).await;
        assert!(woken.is_ok(), "a wake permit is stored for the loop");
        assert!(
            !state.refresh.requested.load(Ordering::SeqCst),
            "the loop only re-plans against the new grid"
        );
    }

    #[test]
    fn turning_usage_access_on_requests_a_refresh() {
        let state = AppState::new();
        apply_usage_access(&state, true);
        assert!(
            state.refresh.requested.swap(false, Ordering::SeqCst),
            "the tray and views were computed without access"
        );

        apply_usage_access(&state, true);
        apply_usage_access(&state, false);
        assert!(
            !state.refresh.requested.load(Ordering::SeqCst),
            "only a switch from off to on asks for one"
        );
    }

    #[tokio::test]
    async fn clear_payload_cache_requests_refresh_and_drops_cursor_cache() {
        let state = AppState::new();
        state.parser.store_cursor_remote(Vec::new(), None);
        assert!(
            state.parser.cursor_remote_for(None).is_some(),
            "guard: Cursor data is cached"
        );

        clear_payload_caches(&state).await;

        assert!(
            state.parser.cursor_remote_for(None).is_none(),
            "the Cursor account may have changed"
        );
        assert!(
            state.refresh.requested.load(Ordering::SeqCst),
            "a refresh rebuilds the views from one sample"
        );
    }

    /// A plain read serves the refresh's reading: no probe, even when the
    /// cached one is long past every provider's refetch floor.
    #[tokio::test]
    async fn unforced_rate_limits_return_cache_without_probe() {
        let cached = RateLimitsPayload {
            claude: None,
            codex: None,
            cursor: None,
            kimi: Some(ProviderRateLimits {
                provider: "kimi".to_string(),
                plan_tier: None,
                windows: vec![RateLimitWindow::new(
                    "five_hour".to_string(),
                    "Session (5hr)".to_string(),
                    42.0,
                    None,
                )],
                extra_usage: None,
                credits: None,
                stale: false,
                error: None,
                retry_after_seconds: None,
                cooldown_until: None,
                fetched_at: "2020-01-01T00:00:00+00:00".to_string(),
            }),
        };
        let state = AppState::new();
        state.usage_access_enabled.store(true, Ordering::SeqCst);
        state.rate_limits_enabled.store(true, Ordering::SeqCst);
        *state.cached_rate_limits.write().await = Some(cached.clone());

        let selection =
            crate::rate_limits::RateLimitSelection::enabled(&[UsageIntegrationId::Kimi]);
        let served = rate_limits_for(&state, selection, false).await;

        let json = |payload: &RateLimitsPayload| serde_json::to_value(payload).unwrap();
        assert_eq!(json(&served), json(&cached));
        let stored = state.cached_rate_limits.read().await.clone().unwrap();
        assert_eq!(json(&stored), json(&cached), "fetched_at must not move");
    }

    fn read_at(provider: &str, fetched_at: &str) -> Option<ProviderRateLimits> {
        Some(ProviderRateLimits {
            provider: provider.to_string(),
            plan_tier: None,
            windows: Vec::new(),
            extra_usage: None,
            credits: None,
            stale: false,
            error: None,
            retry_after_seconds: None,
            cooldown_until: None,
            fetched_at: fetched_at.to_string(),
        })
    }

    #[test]
    fn a_probe_that_ended_while_a_forced_request_waited_answers_it() {
        use crate::rate_limits::RateLimitSelection;
        let at = |claude: &str, kimi: &str| RateLimitsPayload {
            claude: read_at("claude", claude),
            codex: None,
            cursor: None,
            kimi: read_at("kimi", kimi),
        };
        let asked = at("2026-09-25T10:00:00Z", "2026-09-25T10:00:00Z");
        // While it waited, the refresh probed Claude (and maybe errored).
        let now = at("2026-09-25T10:00:20Z", "2026-09-25T10:00:00Z");
        let both =
            RateLimitSelection::enabled(&[UsageIntegrationId::Claude, UsageIntegrationId::Kimi]);

        assert_eq!(
            not_probed_since(both, Some(&asked), Some(&now)),
            RateLimitSelection::enabled(&[UsageIntegrationId::Kimi])
        );
        assert_eq!(
            not_probed_since(both, Some(&now), Some(&now)),
            both,
            "no probe meanwhile: probe them all"
        );
        assert_eq!(not_probed_since(both, None, None), both);
    }

    #[test]
    fn turning_rate_limits_on_requests_a_refresh() {
        let state = AppState::new();
        apply_rate_limits_enabled(&state, true);
        assert!(
            state.refresh.requested.swap(false, Ordering::SeqCst),
            "nothing was probed while they were off"
        );

        apply_rate_limits_enabled(&state, true);
        apply_rate_limits_enabled(&state, false);
        assert!(
            !state.refresh.requested.load(Ordering::SeqCst),
            "only a switch from off to on asks for one"
        );
    }
}
