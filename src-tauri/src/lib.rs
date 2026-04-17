mod commands;
mod logging;
mod models;
mod platform;
mod rate_limits;
mod stats;
mod tray;
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

    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .manage(AppState::new())
        .setup(|app| {
            // Initialize logging first — must happen before any tracing macros.
            if let Ok(app_data) = app.path().app_data_dir() {
                let logging_state = logging::init_logging(&app_data);
                app.manage(logging_state);
            }

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
                                move_window_near_tray(&window);
                                platform::clamp_window_to_work_area(&window);
                                let _ = window.show();
                            }
                            let _ = window.set_focus();
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
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

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
            }

            // Spawn background setup + polling
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                background_loop(app_handle).await;
            });

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
            commands::config::set_refresh_interval,
            commands::tray::set_tray_config,
            commands::tray::get_status_widget_summary,
            commands::config::clear_cache,
            commands::config::clear_payload_cache,
            commands::config::clear_usage_view_cache,
            commands::config::reposition_window,
            commands::config::set_window_size_and_align,
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
        ])
        .run(tauri::generate_context!())
        .expect("error running TokenMonitor");
}

/// Fetch fresh rate limits and update cached state + tray utilization.
async fn refresh_rate_limits(app: &tauri::AppHandle, state: &AppState) {
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

    // Emit so the float ball and main window pick up the new data.
    let _ = app.emit("status-widget-updated", ());
}

async fn background_loop(app: tauri::AppHandle) {
    tokio::time::sleep(Duration::from_secs(1)).await;
    tracing::info!("Background refresh loop started");

    let state = app.state::<AppState>();

    sync_tray_title(&app, &state).await;

    let mut update_counter: u64 = 0;
    let mut ssh_sync_counter: u64 = 0;
    let mut rate_limit_counter: u64 = 0;
    let mut pricing_counter: u64 = 0;

    // SSH sync interval: every 10 local refresh cycles (~5 min at 30s interval).
    const SSH_SYNC_EVERY_N_CYCLES: u64 = 10;
    // Rate limit refresh: every 5 cycles (~2.5 min at 30s interval).
    const RATE_LIMIT_REFRESH_EVERY_N_CYCLES: u64 = 5;
    // Pricing refresh: every 120 cycles (~1h at 30s interval).
    // The actual TTL check (7 days) is inside should_refresh(), so this just
    // controls how often we check the TTL, not how often we actually fetch.
    const PRICING_CHECK_EVERY_N_CYCLES: u64 = 120;

    loop {
        let interval_secs = {
            let interval = state.refresh_interval.read().await;
            *interval
        };

        if interval_secs == 0 {
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        update_counter += 1;
        ssh_sync_counter += 1;
        rate_limit_counter += 1;
        pricing_counter += 1;

        let changed = state.parser.invalidate_if_changed();
        if changed {
            tracing::debug!(
                cycle = update_counter,
                "Parser cache invalidated, data changed"
            );
        }

        // Periodically refresh rate limits so the tray icon and float ball
        // stay up to date even when the main window is hidden.
        if rate_limit_counter >= RATE_LIMIT_REFRESH_EVERY_N_CYCLES {
            rate_limit_counter = 0;
            refresh_rate_limits(&app, &state).await;
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
            }
        }

        sync_tray_title(&app, &state).await;

        // Archive completed hours for data loss prevention.
        // Runs every cycle (~30s) but is fast: only writes when new hours are complete.
        archive_local_usage(&state);

        // Periodically sync SSH hosts in background.
        if ssh_sync_counter >= SSH_SYNC_EVERY_N_CYCLES {
            ssh_sync_counter = 0;
            let ssh_changed = sync_ssh_hosts(&state).await;
            if ssh_changed {
                // Invalidate parser cache so device data reflects new remote files.
                state.parser.invalidate_if_changed();
                let _ = app.emit("data-updated", update_counter);
            }
            // Archive SSH device data for data loss prevention.
            archive_ssh_device_usage(&state).await;
        }

        if changed {
            let _ = app.emit("data-updated", update_counter);
        }
    }
}

/// Archive completed hours for all local providers.
/// Fast no-op when no new hours have completed since the last archive.
fn archive_local_usage(state: &AppState) {
    let Some(archive) = state.parser.archive() else {
        return;
    };

    let now = chrono::Local::now();
    let current_date = now.date_naive();
    let current_hour = now.hour() as u8;

    // Archive each local provider independently.
    for (provider, integration_id) in [
        ("claude", usage::integrations::UsageIntegrationId::Claude),
        ("codex", usage::integrations::UsageIntegrationId::Codex),
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
async fn archive_ssh_device_usage(state: &AppState) {
    let Some(archive) = state.parser.archive() else {
        return;
    };

    let configs = state.ssh_hosts.read().await;
    let enabled: Vec<String> = configs
        .iter()
        .filter(|c| c.enabled)
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
                let dt = chrono::DateTime::parse_from_rfc3339(&r.ts)
                    .or_else(|_| chrono::DateTime::parse_from_str(&r.ts, "%Y-%m-%dT%H:%M:%S%.f%z"));
                let dt = match dt {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!(
                            device = alias.as_str(),
                            ts = %r.ts,
                            error = %e,
                            "Skipping record with unparseable timestamp"
                        );
                        return None;
                    }
                };
                let local_dt = dt.with_timezone(&chrono::Local);
                Some(usage::parser::ParsedEntry {
                    timestamp: local_dt,
                    model: r.model.clone(),
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

/// Sync all enabled SSH hosts sequentially. Returns true if any data changed.
async fn sync_ssh_hosts(state: &AppState) -> bool {
    let configs = state.ssh_hosts.read().await;
    let enabled: Vec<String> = configs
        .iter()
        .filter(|c| c.enabled)
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
