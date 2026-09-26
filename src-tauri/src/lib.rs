mod auto_hide;
mod commands;
mod logging;
mod models;
mod ops;
mod paths;
mod plan_budget;
mod platform;
mod rate_limits;
mod refresh;
mod secrets;
mod single_instance;
mod stats;
#[allow(dead_code)]
mod statusline;
mod tray;
mod updater;
mod usage;

use chrono::Timelike;
use commands::AppState;
use std::time::Duration;
#[cfg(target_os = "macos")]
use tauri::tray::TrayIconEvent;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Emitter, Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
#[cfg(target_os = "macos")]
use tauri_plugin_positioner::{Position, WindowExt};
use tray::click::{gesture_for, TrayGesture};

/// Extract (x, y) from a `tauri::Position` enum as physical-pixel f64.
#[cfg(target_os = "macos")]
fn pos_xy(p: &tauri::Position) -> (f64, f64) {
    match p {
        tauri::Position::Physical(ph) => (ph.x as f64, ph.y as f64),
        tauri::Position::Logical(lo) => (lo.x, lo.y),
    }
}

/// Extract (w, h) from a `tauri::Size` enum as physical-pixel f64.
#[cfg(target_os = "macos")]
fn size_wh(s: &tauri::Size) -> (f64, f64) {
    match s {
        tauri::Size::Physical(ph) => (ph.width as f64, ph.height as f64),
        tauri::Size::Logical(lo) => (lo.width, lo.height),
    }
}

/// Position the window centered below the tray icon using the rect from the
/// click event.  Falls back to `tauri-plugin-positioner` if the rect looks
/// invalid (zero-sized), and ultimately to `TopRight`.
#[cfg(target_os = "macos")]
fn move_window_below_tray(window: &tauri::WebviewWindow, tray_rect: &tauri::Rect) {
    let (tw, th) = size_wh(&tray_rect.size);
    let (tx, ty) = pos_xy(&tray_rect.position);

    // Only use manual positioning when the rect is plausible.
    if tw > 0.0 && th > 0.0 {
        let win_size = window
            .outer_size()
            .unwrap_or(tauri::PhysicalSize::new(680, 600));
        let x = tx + tw / 2.0 - win_size.width as f64 / 2.0;
        let y = ty + th;
        tracing::debug!(
            tray_x = tx,
            tray_y = ty,
            tray_w = tw,
            tray_h = th,
            win_x = x,
            win_y = y,
            "Positioning window below tray icon"
        );
        let _ = window.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
        return;
    }

    // Fallback: positioner plugin → TopRight
    tracing::debug!("Tray rect invalid, falling back to positioner plugin");
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let ok = catch_unwind(AssertUnwindSafe(|| {
        window.move_window(Position::TrayCenter)
    }))
    .map(|r| r.is_ok())
    .unwrap_or(false);
    if !ok {
        tracing::debug!("TrayCenter unavailable, falling back to TopRight");
        let _ = window.move_window(Position::TopRight);
    }
}

/// Fallback positioning when no tray rect is available (e.g. right-click menu "Show").
#[cfg(target_os = "macos")]
fn move_window_near_tray(window: &tauri::WebviewWindow) {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let ok = catch_unwind(AssertUnwindSafe(|| {
        window.move_window(Position::TrayCenter)
    }))
    .map(|r| r.is_ok())
    .unwrap_or(false);
    if !ok {
        tracing::debug!("TrayCenter unavailable, falling back to TopRight");
        let _ = window.move_window(Position::TopRight);
    }
}

/// Tell the popover page whether it is on screen. WebView2 is not told when
/// its window hides and keeps painting, so the page pauses its own animations,
/// timers and refetches. Sent before a show, so the page wakes up with it, and
/// after a hide.
pub(crate) fn emit_popover_visibility(window: &tauri::WebviewWindow, visible: bool) {
    if visible {
        if let Some(state) = window.try_state::<AppState>() {
            crate::refresh::popover_shown(&state);
        }
    }
    let _ = window.emit_to(window.label(), "popover-visibility", visible);
}

/// Show + position + focus the main window using the same per-OS logic as the
/// tray "Show" menu item. Must run on the main thread (GTK/AppKit constraint).
fn show_main_window_inner(window: &tauri::WebviewWindow) {
    emit_popover_visibility(window, true);
    #[cfg(target_os = "windows")]
    {
        platform::windows::window::position_near_tray(window);
        let _ = window.show();
    }
    #[cfg(target_os = "linux")]
    {
        // Pre-hint position before show (WM may respect this).
        platform::linux::position_top_right(window);
        let _ = window.show();
        // Immediate re-position (works if WM realized fast enough).
        platform::linux::position_top_right(window);
        platform::clamp_window_to_work_area(window);
        // Deferred re-position to catch slow WM realization.
        platform::linux::deferred_reposition(window.clone());
    }
    #[cfg(target_os = "macos")]
    {
        move_window_near_tray(window);
        platform::clamp_window_to_work_area(window);
        let _ = window.show();
    }
    #[cfg(target_os = "windows")]
    platform::windows::window::activate_window(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window.set_focus();
}

/// Thread-safe entry point to surface the main window. Dispatches the actual
/// show to the main thread, so it is safe to call from the single-instance
/// accept loop (a non-main thread) on FOCUS.
pub(crate) fn show_main_window(app: &tauri::AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window("main") {
            show_main_window_inner(&window);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Force X11 backend on Wayland sessions so set_position() works.
    // Wayland compositors ignore client-side window positioning entirely,
    // which causes the popover to appear at a compositor-chosen default
    // position instead of the top-right corner.
    #[cfg(target_os = "linux")]
    if std::env::var("GDK_BACKEND").is_err() && std::env::var("WAYLAND_DISPLAY").is_ok() {
        std::env::set_var("GDK_BACKEND", "x11");
    }

    // Single-instance guard: detect an already-running TokenMonitor (or a
    // foreign process squatting the loopback lock port) and act on the user's
    // choice — all BEFORE building any tray/window, so a declined launch exits
    // cleanly without ever showing UI. See `single_instance` module.
    if single_instance::acquire_or_exit() == single_instance::Acquire::Exit {
        return;
    }

    // Parse on at most half the cores, so even a cold parse never saturates
    // the machine.
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(
            std::thread::available_parallelism()
                .map(|n| (n.get() / 2).max(1))
                .unwrap_or(1),
        )
        .build_global();

    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .setup(|app| {
            let setup_t0 = std::time::Instant::now();
            // Initialize logging first — must happen before any tracing macros.
            if let Ok(app_data) = app.path().app_data_dir() {
                let logging_state = logging::init_logging(&app_data);
                app.manage(logging_state);
            }
            tracing::info!("[PROFILE] setup:logging = {:?}", setup_t0.elapsed());

            // Kimi logs the selected config alias (`kimi-for-coding`, `k3`,
            // …) rather than the product name; pull the real display name
            // ("K2.7 Coding") from the Kimi CLI's config.toml so the tab shows
            // it. Key stays the alias so pricing and chart colors are
            // unaffected.
            models::set_model_display_overrides(usage::kimi_parser::kimi_model_display_names());

            // Owner instance: start the lock-port accept loop now that the
            // AppHandle exists. It answers PROBE / QUIT / FOCUS from any future
            // launch attempt. No-op when the guard is bypassed or not held.
            single_instance::spawn_accept_loop(app.handle().clone());

            // Build tray menu (right-click on macOS/Windows, any click on Linux).
            let show = MenuItemBuilder::with_id("show", "Show TokenMonitor").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit TokenMonitor").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show)
                .separator()
                .item(&quit)
                .build()?;

            // Build tray icon (44×44 @2x retina base icon)
            let tray_icon = Image::new_owned(
                include_bytes!("../icons/tray-icon@2x.rgba").to_vec(),
                44,
                44,
            );
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .icon_as_template(true)
                .title("$--.--")
                .tooltip("TokenMonitor")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id() == "quit" {
                        app.exit(0);
                    } else if event.id() == "show" {
                        if let Some(window) = app.get_webview_window("main") {
                            show_main_window_inner(&window);
                        }
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

                    match gesture_for(&event) {
                        Some(TrayGesture::PresentMenu) => {
                            // Windows/Linux: tray-icon pops the attached menu itself.
                            // macOS: the menu is detached (see `platform::macos::tray_menu`),
                            // so present it here.
                            #[cfg(target_os = "macos")]
                            platform::macos::tray_menu::present(tray);
                        }
                        Some(TrayGesture::TogglePopover) => {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                if window.is_visible().unwrap_or(false) {
                                    tracing::info!("tray click: hiding the popover");
                                    let _ = window.hide();
                                    emit_popover_visibility(&window, false);
                                } else {
                                    tracing::info!("tray click: showing the popover");
                                    emit_popover_visibility(&window, true);
                                    #[cfg(target_os = "windows")]
                                    {
                                        platform::windows::window::position_near_tray(&window);
                                        let _ = window.show();
                                    }
                                    #[cfg(target_os = "linux")]
                                    {
                                        // Pre-hint position before show (WM may respect this).
                                        platform::linux::position_top_right(&window);
                                        let _ = window.show();
                                        // Immediate re-position (works if WM realized fast enough).
                                        platform::linux::position_top_right(&window);
                                        platform::clamp_window_to_work_area(&window);
                                        // Deferred re-position to catch slow WM realization.
                                        platform::linux::deferred_reposition(window.clone());
                                    }
                                    #[cfg(target_os = "macos")]
                                    {
                                        if let TrayIconEvent::Click { rect, .. } = &event {
                                            move_window_below_tray(&window, rect);
                                        }
                                        platform::clamp_window_to_work_area(&window);
                                        let _ = window.show();
                                    }
                                    #[cfg(target_os = "windows")]
                                    platform::windows::window::activate_window(&window);
                                    #[cfg(not(target_os = "windows"))]
                                    let _ = window.set_focus();
                                }
                            }
                        }
                        None => {}
                    }
                })
                .build(app)?;
            // macOS 27 stops delivering left clicks to the tray view while an
            // NSMenu is attached to the status item, so keep the menu detached
            // and present it ourselves on right click.
            #[cfg(target_os = "macos")]
            platform::macos::tray_menu::detach(&_tray);
            tracing::info!("[PROFILE] setup:tray+window = {:?}", setup_t0.elapsed());

            // Hide window on focus loss (popover behavior), but not when
            // focus moves to another app window (e.g. float-ball) or when a
            // settings toggle causes transient focus loss (dock icon, etc.).
            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::Focused(true) = event {
                        tracing::info!("popover gained focus");
                    }
                    if let WindowEvent::Focused(false) = event {
                        let handle = window_clone.app_handle().clone();
                        let win = window_clone.clone();
                        std::thread::spawn(move || {
                            // Brief delay to let the OS settle focus on the new window.
                            std::thread::sleep(Duration::from_millis(150));

                            // A command (create_float_ball, set_dock_icon_visible, etc.)
                            // armed the gate because it was about to cause this blur.
                            if handle.state::<AppState>().auto_hide_gate.take() {
                                tracing::info!(
                                    "popover blur ignored: a command armed the auto-hide gate"
                                );
                                return;
                            }

                            let windows = handle.webview_windows();
                            let focused: Vec<&str> = windows
                                .iter()
                                .filter(|(_, w)| w.is_focused().unwrap_or(false))
                                .map(|(label, _)| label.as_str())
                                .collect();
                            if focused.is_empty() {
                                tracing::info!("popover blur: no app window focused, hiding");
                                let _ = win.hide();
                                emit_popover_visibility(&win, false);
                            } else {
                                tracing::info!(
                                    "popover blur: focus stayed in the app ({}), keeping it",
                                    focused.join(", ")
                                );
                            }
                        });
                    }
                });
            }

            // Pre-position the hidden window on Linux so the WM has a position
            // hint before the first show — some compositors respect this.
            #[cfg(target_os = "linux")]
            if let Some(ref w) = app.get_webview_window("main") {
                platform::linux::position_top_right(w);
            }
            tracing::info!("[PROFILE] setup:focus-handler = {:?}", setup_t0.elapsed());

            // Initialize SSH cache manager and usage archive with Tauri app data dir.
            if let Ok(app_data) = app.path().app_data_dir() {
                let state = app.state::<commands::AppState>();
                let mut cache = state.ssh_cache.blocking_write();
                *cache = Some(usage::ssh_remote::SshCacheManager::new(&app_data));

                // Initialize usage archive for data loss prevention.
                let archive = usage::archive::ArchiveManager::new(&app_data);
                state.parser.set_archive(archive);
                tracing::info!(
                    "Usage archive initialized at {:?}",
                    app_data.join("usage-archive")
                );

                plan_budget::init(&app_data);

                // Initialize payload disk cache for instant cold-start.
                let disk_cache = usage::payload_disk_cache::PayloadDiskCache::new(&app_data);
                *state.payload_disk_cache.blocking_write() = Some(disk_cache);

                // Load cached dynamic pricing and exchange rates immediately.
                if let Some(rates) = usage::litellm::load_cached(&app_data) {
                    usage::pricing::set_dynamic_pricing(rates);
                }
                if let Some(rates) = usage::exchange_rates::load_cached(&app_data) {
                    usage::exchange_rates::set_exchange_rates(rates);
                }

                // Tables past their TTL (or version) are fetched now and
                // applied by the first sample after they arrive, which also
                // tells the webview (`exchange-rates-updated`). This counts as
                // the refresh's hourly check.
                if let Ok(mut last) = state.refresh.last_pricing_check.lock() {
                    *last = Some(std::time::Instant::now());
                }
                let pricing_app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = pricing_app.state::<AppState>();
                    refresh::fetch_price_tables_once(&state, &app_data).await;
                });
            }
            tracing::info!("[PROFILE] setup:data-init = {:?}", setup_t0.elapsed());

            // Hydrate the in-memory Cursor secret cache from keyring (or
            // file fallback) so the first usage refresh after launch can
            // hit the remote API even without the frontend's
            // `set_cursor_auth_config`. Best-effort: a missing/locked
            // keychain just leaves the cache empty. In the background: the
            // keychain and IDE-token reads take tens of ms, and the setup
            // hook holds up the app's start.
            let prime_handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                commands::config::prime_cursor_auth_from_disk(&prime_handle);
            });

            // Load persisted updater state
            {
                let state = app.state::<commands::AppState>();
                let loaded = updater::persistence::load(app.handle());
                let mut guard = state.updater.blocking_write();
                *guard = loaded;
            }

            // Spawn the updater scheduler
            updater::scheduler::spawn(app.handle().clone());
            tracing::info!("[PROFILE] setup:updater-done = {:?}", setup_t0.elapsed());

            // The refresh loop owns every periodic job: one sample per tick,
            // published together.
            tauri::async_runtime::spawn(refresh::run(app.handle().clone()));

            tracing::info!("[PROFILE] setup:TOTAL = {:?}", setup_t0.elapsed());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::usage_query::get_usage_data,
            commands::calendar::get_monthly_usage,
            commands::usage_query::get_known_models,
            plan_budget::get_plan_budget,
            commands::config::get_last_usage_debug,
            commands::config::set_dock_icon_visible,
            commands::config::suppress_next_auto_hide,
            commands::config::set_auto_export_config,
            commands::config::set_refresh_interval,
            commands::config::set_currency,
            commands::config::set_period_config,
            commands::config::set_rate_limits_enabled,
            commands::config::set_usage_access_enabled,
            commands::config::set_cursor_auth_config,
            commands::config::clear_cursor_auth_config,
            commands::config::open_cursor_app,
            commands::config::retry_cursor_auth,
            commands::config::get_cursor_auth_status,
            commands::config::request_app_data_access,
            commands::config::check_app_data_access,
            commands::config::open_app_data_settings,
            commands::statusline::install_statusline,
            commands::statusline::check_statusline,
            commands::statusline::set_claude_plan_tier,
            commands::tray::set_tray_config,
            commands::tray::set_enabled_integrations,
            commands::tray::get_status_widget_summary,
            commands::config::clear_cache,
            commands::config::clear_payload_cache,
            commands::config::set_window_size_and_align,
            commands::config::get_window_anchor_edge,
            commands::config::get_rate_limits,
            commands::float_ball::create_float_ball,
            commands::float_ball::destroy_float_ball,
            commands::float_ball::set_float_ball_expanded,
            commands::float_ball::set_float_ball_dragging,
            commands::float_ball::move_float_ball_to,
            commands::float_ball::snap_float_ball,
            commands::float_ball::get_float_ball_position,
            commands::ssh::get_ssh_hosts,
            commands::ssh::get_ssh_host_statuses,
            commands::ssh::init_ssh_hosts,
            commands::ssh::init_remote_device_include_flags,
            commands::ssh::add_ssh_host,
            commands::ssh::toggle_ssh_host,
            commands::ssh::test_ssh_connection,
            commands::ssh::sync_ssh_host,
            commands::ssh::get_device_usage,
            commands::ssh::get_single_device_usage,
            commands::ssh::toggle_device_include_in_stats,
            commands::logging::log_frontend_message,
            commands::logging::set_log_level,
            commands::logging::get_log_level,
            commands::updater::updater_status,
            commands::updater::updater_check_now,
            commands::updater::updater_install,
            commands::updater::updater_set_auto_check,
            commands::updater::updater_set_channel,
            commands::updater::updater_discover_channels,
            commands::updater::updater_fetch_channel_pubkey,
            commands::updater::updater_skip_version,
            commands::updater::updater_dismiss,
            commands::config::get_exchange_rates,
            commands::config::quit_app,
            commands::config::start_cache_warmup,
            commands::config::cancel_cache_warmup,
            commands::usage_io::export_usage_data,
            commands::usage_io::import_usage_data,
            commands::usage_io::sync_remote_devices,
            refresh::refresh_ready,
            refresh::refresh_on_focus,
        ])
        .run(tauri::generate_context!())
        .expect("error running TokenMonitor");
}

/// Seconds past local midnight that every scheduled refresh is aligned to.
/// One second rather than zero so the tick that re-dates the UI is
/// unambiguously *on* the new day, never a hair before it.
const REFRESH_ALIGN_OFFSET_SECS: i64 = 1;

pub(crate) const SECS_PER_DAY: i64 = 86_400;

/// How long to sleep so the next wake lands on the next aligned refresh tick:
/// `local midnight + 1s + k * interval`.
///
/// Anchoring the phase to local midnight instead of to process start is what
/// makes a refresh always land at 00:00:01 local. Every "current period" view
/// (`offset = 0` = today) is date-relative, so without a tick right after
/// midnight a window left open across the boundary keeps showing the previous
/// day. Ticks that would overshoot the next midnight are clamped back to it,
/// which also covers intervals that don't divide the day evenly.
pub(crate) fn secs_until_next_refresh(
    now: chrono::DateTime<chrono::Local>,
    interval_secs: u64,
) -> f64 {
    let interval = (interval_secs.max(1) as i64).min(SECS_PER_DAY);
    let secs_of_day = now.num_seconds_from_midnight() as i64;
    let elapsed = secs_of_day - REFRESH_ALIGN_OFFSET_SECS;

    let next_secs_of_day = if elapsed < 0 {
        // Between midnight and the day's first tick.
        REFRESH_ALIGN_OFFSET_SECS
    } else {
        REFRESH_ALIGN_OFFSET_SECS + (elapsed / interval + 1) * interval
    }
    // Never step over the next midnight tick.
    .min(SECS_PER_DAY + REFRESH_ALIGN_OFFSET_SECS);

    let subsec = f64::from(now.nanosecond().min(999_999_999)) / 1e9;
    ((next_secs_of_day - secs_of_day) as f64 - subsec).max(0.001)
}

/// Archive completed hours for all local providers: those before the hour of
/// `horizon`, the time of the sweep the file cache was revalidated by. What
/// was logged after it is not in the cache yet, so its hour stays open.
/// Fast no-op when no new hours have completed since the last archive.
pub(crate) fn archive_local_usage(state: &AppState, horizon: chrono::DateTime<chrono::Local>) {
    let Some(archive) = state.parser.archive() else {
        return;
    };

    let current_date = horizon.date_naive();
    let current_hour = horizon.hour() as u8;

    // Archive each local provider independently.
    for (provider, integration_id) in [
        ("claude", usage::integrations::UsageIntegrationId::Claude),
        ("codex", usage::integrations::UsageIntegrationId::Codex),
        ("cursor", usage::integrations::UsageIntegrationId::Cursor),
        ("kimi", usage::integrations::UsageIntegrationId::Kimi),
    ] {
        let source_key = format!("local:{provider}");

        // Quick check: skip if frontier is already up to date.
        let frontier = archive.frontier(&source_key);
        if frontier.is_some_and(|f| f.is_up_to_date(current_date, current_hour)) {
            continue;
        }
        // A frontier stuck behind an idle hour stays "not up to date"; scan it
        // once per hour instead of reloading full history on every call.
        if !archive.should_scan(&source_key, current_date, current_hour, 0) {
            continue;
        }

        // Hours up to the frontier are skipped, so load from its day on: the
        // mtime filter then leaves the older logs unread. Not Cursor's: an
        // all-time load gets its remote rows only from an all-time fetch,
        // which keeps them out of the archive as before.
        let since = frontier
            .filter(|_| integration_id != usage::integrations::UsageIntegrationId::Cursor)
            .map(|f| f.date);
        let (entries, _, _) = state.parser.load_entries(integration_id.as_str(), since);

        let count = archive.archive_completed_hours(
            &entries,
            &source_key,
            provider,
            current_date,
            current_hour,
        );

        if count > 0 {
            tracing::debug!(
                provider = provider,
                records = count,
                "Archived {count} hourly aggregate records for {provider}"
            );
        }
    }
}

/// Archive SSH device usage data from remote-cache into hourly aggregates.
pub(crate) async fn archive_ssh_device_usage(state: &AppState) {
    let Some(archive) = state.parser.archive() else {
        return;
    };

    let configs = state.ssh_hosts.read().await;
    let enabled: Vec<String> = configs
        .iter()
        .filter(|c| c.enabled && c.include_in_stats)
        .map(|c| c.alias.clone())
        .collect();
    drop(configs);

    if enabled.is_empty() {
        return;
    }

    let cache_mgr = state.ssh_cache.read().await;
    let Some(mgr) = cache_mgr.as_ref() else {
        return;
    };

    let now = chrono::Local::now();
    let current_date = now.date_naive();
    let current_hour = now.hour() as u8;

    for alias in &enabled {
        let source_key = format!("device:{alias}");

        // Quick frontier check: skip if already up to date.
        if let Some(frontier) = archive.frontier(&source_key) {
            if frontier.is_up_to_date(current_date, current_hour) {
                continue;
            }
        }
        // The shared records may predate the latest sync, so take their version
        // and write time from the same memo entry.
        let cached = match mgr.load_cached_records_stamped(alias) {
            Ok(r) => r,
            Err(_) => continue,
        };
        // The file holds the remote rows only up to the sync that wrote it:
        // archive just the hours before that one, or the rest of its hour would
        // be hidden behind the frontier and never archived.
        let Some(written_at) = cached.written_at else {
            continue;
        };
        let written = chrono::DateTime::<chrono::Local>::from(written_at).min(now);
        let (horizon_date, horizon_hour) = (written.date_naive(), written.hour() as u8);
        if !archive.should_scan(&source_key, horizon_date, horizon_hour, cached.stamp) {
            continue;
        }
        // Convert CompactUsageRecord → ParsedEntry for archiving.
        let entries: Vec<usage::parser::ParsedEntry> = cached
            .index
            .all()
            .filter_map(|(local, r)| {
                let Some(local) = local else {
                    tracing::warn!(
                        device = alias.as_str(),
                        ts = %r.ts,
                        "Skipping record with unparseable timestamp"
                    );
                    return None;
                };
                Some(usage::parser::ParsedEntry {
                    timestamp: *local,
                    model: if r.speed.as_deref() == Some("fast") {
                        format!("{}-fast", r.model)
                    } else {
                        r.model.clone()
                    },
                    input_tokens: r.input_tokens,
                    output_tokens: r.output_tokens,
                    cache_creation_5m_tokens: r.cache_5m,
                    cache_creation_1h_tokens: r.cache_1h,
                    cache_read_tokens: r.cache_read,
                    web_search_requests: 0,
                    unique_hash: None,
                    session_key: format!("ssh:{alias}"),
                    agent_scope: crate::stats::subagent::AgentScope::Main,
                })
            })
            .collect();

        // Determine provider from records (SSH can have both Claude and Codex).
        // Archive as "all" since device records mix providers.
        let count = archive.archive_completed_hours(
            &entries,
            &source_key,
            "all",
            horizon_date,
            horizon_hour,
        );

        if count > 0 {
            tracing::debug!(
                device = alias.as_str(),
                records = count,
                "Archived {count} hourly aggregate records for device {alias}"
            );
        }
    }
}

/// Clean up duplicate device sources in the archive. Handles two classes left by
/// older builds:
///  1. PHANTOM self-duplicates — this machine's own data re-imported from its own
///     export file under a drifted device slug (a computer rename / transient
///     hostname-lookup failure), counted once as `local` and again as a device.
///  2. LEGACY hash-less peer aliases — a peer imported under `device:<slugify
///     (label)>` (old manual-import path) when auto-sync keys off the filename
///     slug `device:<slugify(label)>-<hash>`, so one machine became two devices.
///
/// Both are merged/removed safely and idempotently (a no-op once cleaned).
/// Returns whether it changed the archive; the caller decides when views pick
/// that up.
pub(crate) async fn cleanup_duplicate_devices(state: &AppState) -> bool {
    let Some(archive) = state.parser.archive() else {
        return false;
    };
    let configured: std::collections::HashSet<String> = {
        let hosts = state.ssh_hosts.read().await;
        hosts.iter().map(|h| h.alias.clone()).collect()
    };
    let now = chrono::Local::now();
    let mut removed = archive.remove_self_duplicate_devices(&configured);
    removed.extend(archive.merge_legacy_alias_duplicates(
        &configured,
        now.date_naive(),
        now.hour() as u8,
    ));
    let folded_cursor = archive.fold_shared_cursor_into_local(now.date_naive(), now.hour() as u8);
    if folded_cursor > 0 {
        tracing::info!(
            folded = folded_cursor,
            "Folded account-scoped Cursor rows from device archives into local:cursor"
        );
    }
    let folded_all = archive.fold_device_providers_into_all();
    if folded_all > 0 {
        tracing::info!(
            folded = folded_all,
            "Folded provider-tagged device rows into p=all (removes SSH double count)"
        );
    }
    let changed = !removed.is_empty() || folded_cursor > 0 || folded_all > 0;
    if changed {
        tracing::info!(
            count = removed.len(),
            "Cleaned up {} duplicate device source(s) from the archive",
            removed.len()
        );
    }
    changed
}

#[cfg(test)]
mod refresh_schedule_tests {
    use super::{secs_until_next_refresh, REFRESH_ALIGN_OFFSET_SECS, SECS_PER_DAY};
    use chrono::{Local, TimeZone};

    /// Local wall-clock instant. Panics on the DST gaps some zones have at
    /// 00:00 — none of the times used below fall in one.
    fn at(hour: u32, min: u32, sec: u32, milli: u32) -> chrono::DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 8, 11, hour, min, sec)
            .single()
            .expect("unambiguous local time")
            + chrono::Duration::milliseconds(i64::from(milli))
    }

    /// Seconds-of-day the sleep computed at `now` will wake up on.
    fn wake_secs_of_day(now: chrono::DateTime<Local>, interval: u64) -> f64 {
        use chrono::Timelike;
        f64::from(now.num_seconds_from_midnight())
            + f64::from(now.nanosecond()) / 1e9
            + secs_until_next_refresh(now, interval)
    }

    #[test]
    fn ticks_land_on_the_aligned_second() {
        for interval in [30_u64, 60, 120, 300, 600, 3600] {
            for (h, m, s, ms) in [
                (0, 0, 0, 0),
                (7, 13, 44, 512),
                (12, 0, 0, 0),
                (23, 30, 0, 0),
            ] {
                let wake = wake_secs_of_day(at(h, m, s, ms), interval);
                let offset_into_minute =
                    (wake - REFRESH_ALIGN_OFFSET_SECS as f64) % interval as f64;
                assert!(
                    offset_into_minute.abs() < 1e-6,
                    "interval={interval} at {h}:{m}:{s}.{ms} woke at {wake}",
                );
            }
        }
    }

    #[test]
    fn always_stops_at_one_second_past_midnight() {
        // Whatever the interval and however close to midnight, the next wake
        // never steps over 00:00:01 — that is the tick that re-dates the UI.
        for interval in [30_u64, 45, 60, 90, 120, 300, 3600] {
            for (h, m, s) in [(23, 59, 59), (23, 59, 30), (23, 30, 0), (23, 0, 1)] {
                let wake = wake_secs_of_day(at(h, m, s, 0), interval);
                assert!(
                    wake <= (SECS_PER_DAY + REFRESH_ALIGN_OFFSET_SECS) as f64 + 1e-6,
                    "interval={interval} at {h}:{m}:{s} woke at {wake}",
                );
            }
        }
        // 23:59:30 with a 90s interval would overshoot to 00:01:01 unclamped.
        let wake = wake_secs_of_day(at(23, 59, 30, 0), 90);
        assert!((wake - (SECS_PER_DAY + REFRESH_ALIGN_OFFSET_SECS) as f64).abs() < 1e-6);
    }

    #[test]
    fn first_tick_of_the_day_is_one_second_after_midnight() {
        assert!((secs_until_next_refresh(at(0, 0, 0, 0), 300) - 1.0).abs() < 1e-6);
        assert!((secs_until_next_refresh(at(0, 0, 0, 400), 300) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn subsecond_drift_is_trimmed_not_accumulated() {
        // Woken 250 ms late: the next sleep is short by that much so the
        // schedule snaps back onto the aligned second instead of drifting.
        let sleep = secs_until_next_refresh(at(10, 0, 1, 250), 30);
        assert!((sleep - 29.75).abs() < 1e-6, "sleep={sleep}");
    }

    #[test]
    fn never_returns_a_zero_sleep() {
        // A zero would spin the loop; every path keeps a floor.
        for interval in [0_u64, 1, 30] {
            for (h, m, s, ms) in [(0, 0, 1, 0), (23, 59, 59, 999), (12, 0, 1, 0)] {
                assert!(secs_until_next_refresh(at(h, m, s, ms), interval) > 0.0);
            }
        }
    }

    #[test]
    fn interval_longer_than_a_day_still_ticks_daily() {
        let wake = wake_secs_of_day(at(12, 0, 0, 0), SECS_PER_DAY as u64 * 3);
        assert!((wake - (SECS_PER_DAY + REFRESH_ALIGN_OFFSET_SECS) as f64).abs() < 1e-6);
    }
}

#[cfg(test)]
mod ssh_archive_tests {
    use super::*;
    use chrono::{DateTime, Local, TimeDelta};
    use std::sync::Arc;
    use usage::ssh_remote::{CompactUsageRecord, SshCacheManager, SshHostConfig};

    fn row(at: DateTime<Local>) -> String {
        let record = CompactUsageRecord {
            ts: at.to_rfc3339(),
            model: "claude-sonnet-4-6".into(),
            input_tokens: 100,
            output_tokens: 10,
            cache_5m: 0,
            cache_1h: 0,
            cache_read: 0,
            speed: None,
            dedupe_key: None,
        };
        serde_json::to_string(&record).unwrap() + "\n"
    }

    /// A sync at `written` that left `rows` in the host's cache file.
    fn write_cache(path: &std::path::Path, rows: &[DateTime<Local>], written: DateTime<Local>) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, rows.iter().map(|&at| row(at)).collect::<String>()).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(written.into())
            .unwrap();
    }

    fn frontier_hour(state: &AppState) -> Option<(chrono::NaiveDate, u8)> {
        let archive = state.parser.archive().unwrap();
        archive.frontier("device:host").map(|f| (f.date, f.hour))
    }

    fn hour_of(at: DateTime<Local>) -> Option<(chrono::NaiveDate, u8)> {
        Some((at.date_naive(), at.hour() as u8))
    }

    #[tokio::test]
    async fn ssh_archive_stops_before_the_hour_of_the_sync_that_wrote_the_records() {
        let tmp = tempfile::TempDir::new().unwrap();
        let parser = usage::parser::UsageParser::with_dirs(
            tmp.path().join("claude"),
            tmp.path().join("codex"),
        );
        parser.set_archive(usage::archive::ArchiveManager::new(tmp.path()));
        let mut state = AppState::new();
        state.parser = Arc::new(parser);
        *state.ssh_hosts.write().await = vec![SshHostConfig {
            alias: "host".into(),
            enabled: true,
            include_in_stats: true,
        }];
        let mgr = SshCacheManager::new(tmp.path());
        *state.ssh_cache.write().await = Some(mgr.clone());
        let path = tmp.path().join("remote-cache/host/usage.jsonl");

        // The sync that wrote the file ran partway into its hour: that hour's
        // later remote rows are not in the file yet, so it must stay open.
        let now = Local::now();
        let first = now - TimeDelta::hours(4);
        let before = first - TimeDelta::hours(1);
        write_cache(&path, &[before, first], first);
        archive_ssh_device_usage(&state).await;
        assert_eq!(frontier_hour(&state), hour_of(before));

        // A later sync rewrites the file, but the frozen records (and their
        // write time) stay the ones read above until the next revalidation.
        let second = now - TimeDelta::hours(1);
        write_cache(&path, &[before, first, second], second);
        archive_ssh_device_usage(&state).await;
        assert_eq!(frontier_hour(&state), hour_of(before));

        assert!(mgr.revalidate_records_memo());
        archive_ssh_device_usage(&state).await;
        assert_eq!(frontier_hour(&state), hour_of(first));
    }
}
