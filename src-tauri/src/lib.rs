mod ccusage;
mod commands;
mod hourly;
mod models;
mod pricing;

use commands::AppState;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_positioner::{Position, WindowExt};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![])))
        .manage(AppState::new())
        .setup(|app| {
            // Build quit menu for right-click
            let quit = MenuItemBuilder::with_id("quit", "Quit TokenMonitor").build(app)?;
            let menu = MenuBuilder::new(app).item(&quit).build()?;

            // Build tray icon with dedicated high-res menu bar icon (44×44 @2x)
            let tray_icon = Image::new_owned(
                include_bytes!("../icons/tray-icon@2x.rgba").to_vec(),
                44,
                44,
            );
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .icon_as_template(true)
                .title("$--.--")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id() == "quit" {
                        app.exit(0);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.move_window(Position::TrayCenter);
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // Hide window on focus loss (popover behavior)
            let window = app.get_webview_window("main").unwrap();
            window.on_window_event(move |event| {
                if let WindowEvent::Focused(false) = event {
                    // Popover behavior: window hides when unfocused
                }
            });

            // Spawn background setup + polling
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                background_loop(app_handle).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_setup_status,
            commands::initialize_app,
            commands::get_usage_data,
            commands::set_refresh_interval,
            commands::clear_cache,
        ])
        .run(tauri::generate_context!())
        .expect("error running TokenMonitor");
}

async fn update_tray_title(app: &tauri::AppHandle, state: &AppState) {
    let runner = state.runner.read().await;
    let today = chrono::Local::now().format("%Y%m%d").to_string();

    if let Ok((json, _)) = runner
        .run_cached("claude", "daily", &["--since", &today], Duration::from_secs(60))
        .await
    {
        if let Ok(resp) = serde_json::from_str::<models::ClaudeDailyResponse>(&json) {
            let total: f64 = resp.daily.iter().map(|d| d.total_cost).sum();
            if let Some(tray) = app.tray_by_id("main-tray") {
                let _ = tray.set_title(Some(&format!("${:.2}", total)));
            }
        }
    }
}

async fn background_loop(app: tauri::AppHandle) {
    // Wait for frontend to initialize
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Auto-initialize ccusage
    let state = app.state::<AppState>();
    {
        let mut runner = state.runner.write().await;
        let _ = runner.ensure_installed().await;
        let mut status = state.setup_status.write().await;
        status.ready = true;
    }

    // Notify frontend that setup is complete
    let _ = app.emit("setup-complete", true);

    // Update tray title immediately on first launch
    update_tray_title(&app, &state).await;

    // Polling loop: refresh data and update tray title
    // Cache TTL in run_cached handles staleness naturally — no need to
    // nuke all cache files each cycle.  The tray update will refetch
    // expired entries, warming the in-memory cache for the frontend.
    let mut update_counter: u64 = 0;
    let mut hours_elapsed: u64 = 0;
    loop {
        // Read configurable refresh interval (0 = polling disabled)
        let interval_secs = {
            let interval = state.refresh_interval.read().await;
            *interval
        };

        if interval_secs == 0 {
            // Polling disabled — sleep briefly and re-check
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        update_counter += 1;
        hours_elapsed += interval_secs;

        // Update tray (TTL-expired entries refetched automatically)
        update_tray_title(&app, &state).await;

        // Notify frontend to refresh its current view
        let _ = app.emit("data-updated", update_counter);

        // Check for ccusage updates every 12 hours
        if hours_elapsed >= 43200 {
            hours_elapsed = 0;
            let runner = state.runner.read().await;
            let _ = runner.update_packages().await;
        }
    }
}
