mod commands;
mod logging;
mod models;
mod paths;
mod platform;
mod rate_limits;
mod secrets;
mod single_instance;
mod stats;
#[allow(dead_code)]
mod statusline;
mod tray;
mod updater;
mod usage;

use chrono::Timelike;
use commands::{
    sync_tray_title,
    tray::{patch_tray_utilization, tray_utilization_from_rate_limits},
    AppState,
};
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
#[cfg(target_os = "macos")]
use tauri_plugin_positioner::{Position, WindowExt};

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

/// Show + position + focus the main window using the same per-OS logic as the
/// tray "Show" menu item. Must run on the main thread (GTK/AppKit constraint).
fn show_main_window_inner(window: &tauri::WebviewWindow) {
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

    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
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

            // Kimi logs a stable alias (`kimi-for-coding`) rather than the model
            // version; pull the real display name ("K2.7 Code High Speed") from
            // the Kimi CLI's config.toml so the tab shows it. Key stays the alias
            // so pricing and chart colors are unaffected.
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

                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        #[cfg(target_os = "macos")]
                        rect,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
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
                                    move_window_below_tray(&window, &rect);
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
                })
                .build(app)?;
            tracing::info!("[PROFILE] setup:tray+window = {:?}", setup_t0.elapsed());

            // Hide window on focus loss (popover behavior), but not when
            // focus moves to another app window (e.g. float-ball) or when a
            // settings toggle causes transient focus loss (dock icon, etc.).
            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::Focused(false) = event {
                        let handle = window_clone.app_handle().clone();
                        let win = window_clone.clone();
                        std::thread::spawn(move || {
                            // Brief delay to let the OS settle focus on the new window.
                            std::thread::sleep(Duration::from_millis(150));

                            // A command (create_float_ball, set_dock_icon_visible, etc.)
                            // requested that the next blur be ignored.
                            let state = handle.state::<AppState>();
                            if state
                                .suppress_auto_hide
                                .swap(false, std::sync::atomic::Ordering::SeqCst)
                            {
                                return;
                            }

                            let any_app_window_focused = handle
                                .webview_windows()
                                .values()
                                .any(|w| w.is_focused().unwrap_or(false));
                            if !any_app_window_focused {
                                let _ = win.hide();
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

                // Initialize payload disk cache for instant cold-start.
                let disk_cache = usage::payload_disk_cache::PayloadDiskCache::new(&app_data);
                *state.payload_disk_cache.blocking_write() = Some(disk_cache);

                // Load cached dynamic pricing immediately (non-blocking).
                if let Some(rates) = usage::litellm::load_cached(&app_data) {
                    usage::pricing::set_dynamic_pricing(rates);
                }

                // Spawn async refresh if cache is stale (>7 days) or version-mismatched.
                if usage::litellm::should_refresh(&app_data) {
                    let data_dir = app_data.clone();
                    tauri::async_runtime::spawn(async move {
                        match usage::litellm::fetch_and_cache(&data_dir).await {
                            Ok(rates) => {
                                usage::pricing::set_dynamic_pricing(rates);
                                tracing::info!("Dynamic pricing refreshed (LiteLLM + OpenRouter)");
                            }
                            Err(e) => {
                                tracing::warn!("Pricing fetch failed (using fallback): {e}");
                            }
                        }
                    });
                }

                // Load cached exchange rates immediately.
                if let Some(rates) = usage::exchange_rates::load_cached(&app_data) {
                    usage::exchange_rates::set_exchange_rates(rates);
                }

                // Spawn async refresh if exchange rate cache is stale (>24h).
                if usage::exchange_rates::should_refresh(&app_data) {
                    let data_dir = app_data.clone();
                    tauri::async_runtime::spawn(async move {
                        match usage::exchange_rates::fetch_and_cache(&data_dir).await {
                            Ok(rates) => {
                                usage::exchange_rates::set_exchange_rates(rates);
                                tracing::info!("Exchange rates refreshed (frankfurter.dev)");
                            }
                            Err(e) => {
                                tracing::warn!("Exchange rate fetch failed (using fallback): {e}");
                            }
                        }
                    });
                }
            }
            tracing::info!("[PROFILE] setup:data-init = {:?}", setup_t0.elapsed());

            // Hydrate the in-memory Cursor secret cache from keyring (or
            // file fallback) so the first usage refresh after launch can
            // hit the remote API without waiting for the frontend to
            // round-trip a `set_cursor_auth_config` call. Best-effort: a
            // missing/locked keychain just leaves the cache empty.
            commands::config::prime_cursor_auth_from_disk(app.handle());
            tracing::info!("[PROFILE] setup:cursor-prime = {:?}", setup_t0.elapsed());

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

            // Spawn background setup + polling
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                background_loop(app_handle).await;
            });

            // Reactive statusline poll — fires within ~2s of any Claude Code
            // prompt by watching the events-file mtime. Independent of the
            // slow background loop so the dashboard keeps pace with the
            // user without cranking up the global refresh interval.
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                fast_statusline_poll(app_handle).await;
            });

            tracing::info!("[PROFILE] setup:TOTAL = {:?}", setup_t0.elapsed());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::usage_query::get_usage_data,
            commands::calendar::get_monthly_usage,
            commands::usage_query::get_known_models,
            commands::config::get_last_usage_debug,
            commands::config::set_window_surface,
            commands::config::set_glass_effect,
            commands::config::set_dock_icon_visible,
            commands::config::suppress_next_auto_hide,
            commands::config::set_auto_export_config,
            commands::config::set_refresh_interval,
            commands::config::set_rate_limits_enabled,
            commands::config::set_usage_access_enabled,
            commands::config::set_cursor_auth_config,
            commands::config::clear_cursor_auth_config,
            commands::config::open_cursor_app,
            commands::config::retry_cursor_auth,
            commands::config::get_cursor_auth_status,
            commands::config::request_claude_keychain_access,
            commands::config::check_claude_keychain_access,
            commands::config::debug_force_oauth_refresh,
            commands::config::request_app_data_access,
            commands::config::check_app_data_access,
            commands::config::open_app_data_settings,
            commands::statusline::install_statusline,
            commands::statusline::check_statusline,
            commands::statusline::uninstall_statusline,
            commands::statusline::set_claude_plan_tier,
            commands::statusline::read_latest_statusline_ping,
            commands::tray::set_tray_config,
            commands::tray::get_status_widget_summary,
            commands::config::clear_cache,
            commands::config::clear_payload_cache,
            commands::config::clear_usage_view_cache,
            commands::config::reposition_window,
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
            commands::float_ball::init_taskbar_panel,
            commands::float_ball::destroy_taskbar_panel_cmd,
            commands::ssh::get_ssh_hosts,
            commands::ssh::get_ssh_host_statuses,
            commands::ssh::init_ssh_hosts,
            commands::ssh::init_remote_device_include_flags,
            commands::ssh::add_ssh_host,
            commands::ssh::remove_ssh_host,
            commands::ssh::toggle_ssh_host,
            commands::ssh::test_ssh_connection,
            commands::ssh::sync_ssh_host,
            commands::ssh::get_device_usage,
            commands::ssh::get_single_device_usage,
            commands::ssh::toggle_device_include_in_stats,
            commands::logging::log_frontend_message,
            commands::logging::set_log_level,
            commands::logging::get_log_level,
            commands::logging::get_log_dir,
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
            commands::config::get_warmup_status,
            commands::usage_io::export_usage_data,
            commands::usage_io::import_usage_data,
            commands::usage_io::sync_remote_devices,
        ])
        .run(tauri::generate_context!())
        .expect("error running TokenMonitor");
}

/// Fetch fresh rate limits and update cached state + tray utilization.
async fn refresh_rate_limits(app: &tauri::AppHandle, state: &AppState) {
    statusline::source::maybe_trim(&statusline::events_file());

    let codex_dir = state.parser.codex_dir().to_path_buf();
    let cached = state.cached_rate_limits.read().await.clone();
    let fresh = rate_limits::fetch_selected_rate_limits(
        &codex_dir,
        rate_limits::RateLimitSelection::All,
        cached.as_ref(),
    )
    .await;

    let merged = rate_limits::merge_rate_limits(fresh, cached.as_ref());

    *state.cached_rate_limits.write().await = Some(merged.clone());
    patch_tray_utilization(state, tray_utilization_from_rate_limits(Some(&merged))).await;

    tracing::debug!("Background rate-limit refresh complete");

    let _ = app.emit("status-widget-updated", ());
}

/// SSH sync interval: every 10 local refresh cycles (~5 min at 30s interval).
const SSH_SYNC_EVERY_N_CYCLES: u64 = 10;
/// Rate limit refresh: every 5 cycles (~2.5 min at 30s interval).
const RATE_LIMIT_REFRESH_EVERY_N_CYCLES: u64 = 5;
/// Pricing/exchange-rate TTL check: every 120 cycles (~1h at 30s interval).
const PRICING_CHECK_EVERY_N_CYCLES: u64 = 120;

/// When auto-refresh is "off" (refresh_interval == 0), the 2s statusline poll
/// is the only thing detecting usage-log changes, so its (expensive) full-file
/// scan falls back to this cadence instead of running every 2s. Matches the
/// default refresh interval.
const OFF_MODE_SWEEP_SECS: u64 = 30;

async fn fast_statusline_poll(app: tauri::AppHandle) {
    use std::fs;
    use std::time::SystemTime;

    const POLL_INTERVAL_SECS: u64 = 2;

    let path = crate::statusline::events_file();
    let mut last_mtime: Option<SystemTime> =
        fs::metadata(&path).ok().and_then(|m| m.modified().ok());

    let state = app.state::<AppState>();

    // Accumulates elapsed time so the heavy usage-log scan runs at the
    // configured refresh interval rather than on every 2s tick.
    let mut secs_since_sweep: u64 = 0;

    loop {
        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;

        if !state
            .usage_access_enabled
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            continue;
        }

        let now_mtime = fs::metadata(&path).ok().and_then(|m| m.modified().ok());
        let events_moved = now_mtime != last_mtime;
        if events_moved {
            last_mtime = now_mtime;
        }

        // Detecting in-place log appends means stat-ing every session file —
        // far too heavy to run every 2s on a large log set. Throttle it to the
        // user's configured refresh interval (Settings); the cheap events-file
        // check above still runs every 2s for rate limits. "Off" (0) falls back
        // to a sane cadence so usage numbers never freeze entirely.
        secs_since_sweep += POLL_INTERVAL_SECS;
        let sweep_interval_secs = {
            let configured = *state.refresh_interval.read().await;
            if configured == 0 {
                OFF_MODE_SWEEP_SECS
            } else {
                configured
            }
        };
        let parser_changed = if secs_since_sweep >= sweep_interval_secs {
            secs_since_sweep = 0;
            state.parser.invalidate_if_changed()
        } else {
            false
        };

        if !events_moved && !parser_changed {
            continue;
        }

        state.parser.clear_payload_cache();
        if parser_changed {
            // Source logs changed: drop the no-TTL disk cache too, otherwise the
            // next fetch re-serves a stale per-provider payload despite the
            // memory clear above (see AppState::clear_payload_disk_cache).
            state.clear_payload_disk_cache().await;
        }

        if events_moved
            && state
                .rate_limits_enabled
                .load(std::sync::atomic::Ordering::SeqCst)
        {
            refresh_rate_limits(&app, &state).await;
        }
        sync_tray_title(&app, &state).await;
        let _ = app.emit("data-updated", 0u64);
    }
}

async fn background_loop(app: tauri::AppHandle) {
    tokio::time::sleep(Duration::from_secs(1)).await;
    tracing::info!("Background refresh loop started");

    let state = app.state::<AppState>();

    sync_tray_title(&app, &state).await;

    // One-shot silent warmup: precompute all payload caches in background.
    if state
        .usage_access_enabled
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        let app_clone = app.clone();
        tokio::spawn(async move {
            usage::cache_warmup::warmup_payloads(&app_clone, "all", "day", false).await;
        });
    }

    // One-shot on launch: probe rate limits immediately instead of waiting for
    // the first periodic refresh (~2.5 min in). cached_rate_limits starts empty,
    // so this re-probes fresh for every provider — notably Codex, whose periodic
    // refresh is throttled to 5 min and would otherwise sit on stale hydrated
    // data showing "(stale)" right after the app opens. Spawned (not awaited):
    // the Codex app-server probe can take up to 15s and must not delay the loop.
    // Emits `data-updated` afterward so the 5h view re-pulls the fresh windows.
    if state
        .usage_access_enabled
        .load(std::sync::atomic::Ordering::SeqCst)
        && state
            .rate_limits_enabled
            .load(std::sync::atomic::Ordering::SeqCst)
    {
        let app_clone = app.clone();
        tokio::spawn(async move {
            let state = app_clone.state::<AppState>();
            refresh_rate_limits(&app_clone, &state).await;
            let _ = app_clone.emit("data-updated", 0u64);
        });
    }

    // One-shot on launch: purge duplicate device sources left by older builds
    // (the same machine counted under several drifted slugs, or a peer under both
    // its hash-less and canonical alias). The local archive persists across
    // sessions, so this works even before the first archive flush of this session.
    cleanup_duplicate_devices(&state).await;

    let mut update_counter: u64 = 0;
    let mut ssh_sync_counter: u64 = 0;
    let mut rate_limit_counter: u64 = 0;
    let mut pricing_counter: u64 = 0;
    // Tracks which refresh-interval-sized time bucket the rolling 5h window is
    // in. The 5h payload cache key is bucketed by the same size, so when this
    // advances the cached 5h numbers are stale-by-time even if no source log
    // changed — we emit `data-updated` to nudge the UI to re-fetch. Starts at 0
    // so the first cycle always emits once shortly after launch.
    let mut last_five_hour_bucket: u64 = 0;

    loop {
        let interval_secs = {
            let interval = state.refresh_interval.read().await;
            *interval
        };

        if interval_secs == 0 {
            // Auto-export has its own toggle independent of the refresh interval,
            // so honor it even when periodic refresh is Off. It's a cheap no-op
            // when nothing changed, but still lands the once-per-session full
            // sync and any import-triggered reconciliation.
            commands::usage_io::run_auto_export(&app, &state).await;
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        update_counter += 1;
        ssh_sync_counter += 1;
        rate_limit_counter += 1;
        pricing_counter += 1;

        let usage_access_enabled = state
            .usage_access_enabled
            .load(std::sync::atomic::Ordering::SeqCst);

        let mut changed = false;
        if usage_access_enabled {
            changed = state.parser.invalidate_if_changed();
            if changed {
                tracing::debug!(
                    cycle = update_counter,
                    "Parser cache invalidated, data changed"
                );
            }
        }

        // Periodically refresh rate limits so the tray icon and float ball
        // stay up to date even when the main window is hidden. Skipped when
        // the user hasn't opted in — keeps Keychain access dormant until
        // they turn rate-limit tracking on.
        if rate_limit_counter >= RATE_LIMIT_REFRESH_EVERY_N_CYCLES {
            rate_limit_counter = 0;
            if usage_access_enabled
                && state
                    .rate_limits_enabled
                    .load(std::sync::atomic::Ordering::SeqCst)
            {
                refresh_rate_limits(&app, &state).await;
            }
        }

        // Periodically check if pricing cache needs refresh (7-day TTL).
        if pricing_counter >= PRICING_CHECK_EVERY_N_CYCLES {
            pricing_counter = 0;
            if let Ok(app_data) = app.path().app_data_dir() {
                if usage::litellm::should_refresh(&app_data) {
                    tracing::info!("Pricing cache stale, refreshing...");
                    match usage::litellm::fetch_and_cache(&app_data).await {
                        Ok(rates) => {
                            usage::pricing::set_dynamic_pricing(rates);
                            tracing::info!("Dynamic pricing refreshed (background)");
                        }
                        Err(e) => {
                            tracing::warn!("Background pricing refresh failed: {e}");
                        }
                    }
                }

                // Also check exchange rate cache (24h TTL).
                if usage::exchange_rates::should_refresh(&app_data) {
                    match usage::exchange_rates::fetch_and_cache(&app_data).await {
                        Ok(rates) => {
                            usage::exchange_rates::set_exchange_rates(rates);
                            tracing::info!("Exchange rates refreshed (background)");
                        }
                        Err(e) => {
                            tracing::warn!("Background exchange rate refresh failed: {e}");
                        }
                    }
                }
            }
        }

        sync_tray_title(&app, &state).await;

        // Archive completed hours for data loss prevention.
        // Runs every cycle (~30s) but is fast: only writes when new hours are complete.
        if usage_access_enabled {
            archive_local_usage(&state);
        }

        // Periodically sync SSH hosts in background.
        if ssh_sync_counter >= SSH_SYNC_EVERY_N_CYCLES {
            ssh_sync_counter = 0;
            let ssh_changed = sync_ssh_hosts(&state).await;
            if usage_access_enabled && ssh_changed {
                // Invalidate parser cache so device data reflects new remote files.
                state.parser.invalidate_if_changed();
                if let Some(ref disk_cache) = *state.payload_disk_cache.read().await {
                    disk_cache.clear_prefix("usage-view:");
                }
                let _ = app.emit("data-updated", update_counter);
            }
            // Archive SSH device data for data loss prevention.
            archive_ssh_device_usage(&state).await;
        }

        // Mirror the latest archive to the user's auto-export folder if enabled.
        // Runs on the refresh cadence like everything else in this loop; a fast
        // no-op when disabled or no folder is configured.
        commands::usage_io::run_auto_export(&app, &state).await;

        // The rolling 5h window advances with wall-clock time, not just log
        // changes. Its cache key is bucketed by the refresh interval (see
        // final_usage_cache_key), so once we cross into a new bucket the cached
        // 5h payload is stale-by-time. Detect the roll and nudge the UI to
        // re-fetch — the existing `data-updated` listener recomputes the fresh
        // window (and re-pulls rate limits when the 5h view is open).
        let current_five_hour_bucket = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            / interval_secs;
        let five_hour_bucket_rolled = current_five_hour_bucket != last_five_hour_bucket;
        last_five_hour_bucket = current_five_hour_bucket;

        if changed {
            // Local source logs changed. `invalidate_if_changed()` above already
            // dropped the in-memory payload cache, but the no-TTL disk cache
            // would keep re-serving the pre-change per-provider payloads (the
            // Claude/Codex tabs froze for the day while `all` stayed fresh). Drop
            // the disk entries too so the next fetch recomputes from fresh logs.
            state.clear_payload_disk_cache().await;
            let _ = app.emit("data-updated", update_counter);
        } else if five_hour_bucket_rolled && usage_access_enabled {
            let _ = app.emit("data-updated", update_counter);
        }
    }
}

/// Archive completed hours for all local providers.
/// Fast no-op when no new hours have completed since the last archive.
pub(crate) fn archive_local_usage(state: &AppState) {
    let Some(archive) = state.parser.archive() else {
        return;
    };

    // Re-read the Kimi CLI's config.toml so a model switch ("K2.7 Coding" →
    // "K3") shows up without an app restart. Cheap: one small file per data
    // home, and archive runs are already periodic.
    models::set_model_display_overrides(usage::kimi_parser::kimi_model_display_names());

    let now = chrono::Local::now();
    let current_date = now.date_naive();
    let current_hour = now.hour() as u8;

    // Archive each local provider independently.
    for (provider, integration_id) in [
        ("claude", usage::integrations::UsageIntegrationId::Claude),
        ("codex", usage::integrations::UsageIntegrationId::Codex),
        ("cursor", usage::integrations::UsageIntegrationId::Cursor),
        ("kimi", usage::integrations::UsageIntegrationId::Kimi),
    ] {
        let source_key = format!("local:{provider}");

        // Quick check: skip if frontier is already up to date.
        if let Some(frontier) = archive.frontier(&source_key) {
            if frontier.is_up_to_date(current_date, current_hour) {
                continue;
            }
        }

        // Load entries for this provider. The parser's file cache makes this fast.
        let (entries, _, _) = state.parser.load_entries(integration_id.as_str(), None);

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

        let records = match mgr.load_cached_records(alias) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Convert CompactUsageRecord → ParsedEntry for archiving.
        let entries: Vec<usage::parser::ParsedEntry> = records
            .iter()
            .filter_map(|r| {
                let dt = match usage::device_aggregation::parse_remote_ts(&r.ts) {
                    Some(d) => d,
                    None => {
                        tracing::warn!(
                            device = alias.as_str(),
                            ts = %r.ts,
                            "Skipping record with unparseable timestamp"
                        );
                        return None;
                    }
                };
                let local_dt = dt.with_timezone(&chrono::Local);
                Some(usage::parser::ParsedEntry {
                    timestamp: local_dt,
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
            current_date,
            current_hour,
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
/// Both are merged/removed safely and idempotently (a no-op once cleaned);
/// invalidates the usage-view caches when it actually changes something.
pub(crate) async fn cleanup_duplicate_devices(state: &AppState) {
    let Some(archive) = state.parser.archive() else {
        return;
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
    if !removed.is_empty() {
        tracing::info!(
            count = removed.len(),
            "Cleaned up {} duplicate device source(s) from the archive",
            removed.len()
        );
        state.parser.clear_payload_cache();
        if let Some(ref disk_cache) = *state.payload_disk_cache.read().await {
            disk_cache.clear_prefix("usage-view:");
        }
    }
}

/// Sync all active SSH hosts sequentially. Returns true if any data changed.
async fn sync_ssh_hosts(state: &AppState) -> bool {
    let configs = state.ssh_hosts.read().await;
    let enabled: Vec<String> = configs
        .iter()
        .filter(|c| c.enabled && c.include_in_stats)
        .map(|c| c.alias.clone())
        .collect();
    drop(configs); // Release read lock before async work.

    if enabled.is_empty() {
        return false;
    }

    let cache_mgr = state.ssh_cache.read().await;
    let mgr = match cache_mgr.as_ref() {
        Some(m) => m,
        None => return false,
    };

    let mut any_synced = false;
    for alias in &enabled {
        // Pre-test: skip sync if connection fails.
        let test = usage::ssh_remote::test_connection(alias).await;
        if !test.success {
            tracing::warn!(
                alias = %alias,
                error = %test.message,
                "SSH connection test failed, skipping sync"
            );
            continue;
        }

        match mgr.sync_host(alias).await {
            Ok(count) if count > 0 => {
                any_synced = true;
            }
            Err(e) => {
                tracing::error!(alias = %alias, error = %e, "SSH sync failed");
            }
            _ => {}
        }
    }

    any_synced
}
