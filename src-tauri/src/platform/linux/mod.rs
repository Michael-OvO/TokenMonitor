//! Linux-specific platform code.

use std::time::Duration;
use tauri::WebviewWindow;

/// Position the window in the top-right corner of the work area.
///
/// Always uses manual calculation — the positioner plugin's `Position::TopRight`
/// returns `Ok` on Linux but frequently places the window at the wrong position
/// (especially on first show), so we bypass it entirely.
pub fn position_top_right(window: &WebviewWindow) {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| {
            window
                .available_monitors()
                .ok()
                .and_then(|m| m.into_iter().next())
        });
    let Some(monitor) = monitor else {
        tracing::debug!("position_top_right: no monitor found, skipping");
        return;
    };
    let scale = monitor.scale_factor();
    let mon_pos = monitor.position();
    let mon_size = monitor.size();
    let margin = (8.0 * scale) as i32;

    let win_size = window.outer_size().unwrap_or_else(|_| {
        // Window not yet realized — use config dimensions scaled to physical.
        tauri::PhysicalSize::new((340.0 * scale) as u32, (300.0 * scale) as u32)
    });

    let x = mon_pos.x + mon_size.width as i32 - win_size.width as i32 - margin;
    let y = mon_pos.y + margin;

    tracing::debug!(
        mon_w = mon_size.width,
        mon_h = mon_size.height,
        win_w = win_size.width,
        win_h = win_size.height,
        scale,
        x,
        y,
        "position_top_right: placing window"
    );
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

/// Spawn a deferred re-position after a short delay.
///
/// Linux window managers process `show()` asynchronously — on the very first
/// show the WM may not have realized the window yet, causing `set_position` to
/// be ignored or overridden. This function waits briefly, then re-applies the
/// top-right position on the main thread.
pub fn deferred_reposition(window: WebviewWindow) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        position_top_right(&window);
        super::clamp_window_to_work_area(&window);
    });
}
