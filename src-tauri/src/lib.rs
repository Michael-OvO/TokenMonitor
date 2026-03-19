mod commands;
mod models;
mod parser;
mod pricing;
mod rate_limits;
mod tray_render;

#[cfg(target_os = "macos")]
use commands::apply_default_window_surface;
use commands::{sync_tray_title, AppState};
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
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
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
                                let _ = window.show();
                                if window.current_monitor().ok().flatten().is_some() {
                                    let _ = window.move_window(Position::TrayCenter);
                                }
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // Set autosaveName so macOS remembers the user's chosen menu bar position
            #[cfg(target_os = "macos")]
            if let Some(tray) = app.tray_by_id("main-tray") {
                let _ = tray.with_inner_tray_icon(|inner| {
                    if let Some(ns_status_item) = inner.ns_status_item() {
                        let name = objc2_foundation::NSString::from_str("TokenMonitor");
                        ns_status_item.setAutosaveName(Some(&name));
                    }
                });
            }

            // Hide window on focus loss (popover behavior)
            let window = app.get_webview_window("main").unwrap();
            #[cfg(target_os = "macos")]
            if let Err(err) = apply_default_window_surface(&app.handle()) {
                eprintln!("failed to apply default native window surface: {err}");
            }
            let window_clone = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::Focused(false) = event {
                    let _ = window_clone.hide();
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
            commands::get_usage_data,
            commands::get_monthly_usage,
            commands::get_known_models,
            commands::get_last_usage_debug,
            commands::set_window_surface,
            commands::set_glass_effect,
            commands::set_refresh_interval,
            commands::set_tray_config,
            commands::clear_cache,
            commands::get_rate_limits,
        ])
        .run(tauri::generate_context!())
        .expect("error running TokenMonitor");
}

async fn background_loop(app: tauri::AppHandle) {
    tokio::time::sleep(Duration::from_secs(1)).await;

    let state = app.state::<AppState>();

    sync_tray_title(&app, &state).await;

    let mut update_counter: u64 = 0;
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

        state.parser.clear_payload_cache();
        sync_tray_title(&app, &state).await;
        let _ = app.emit("data-updated", update_counter);
    }
}
