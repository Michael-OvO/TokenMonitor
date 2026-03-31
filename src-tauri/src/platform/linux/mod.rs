//! Linux-specific platform code.

use tauri::WebviewWindow;
use tauri_plugin_positioner::{Position, WindowExt};

/// Position the window in the top-right corner of the work area.
///
/// On Linux the system tray typically lives in a top panel (GNOME, KDE, etc.),
/// so we default to top-right positioning instead of trying to detect the tray
/// rect (which is unreliable across desktop environments).
pub fn position_top_right(window: &WebviewWindow) {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let ok = catch_unwind(AssertUnwindSafe(|| window.move_window(Position::TopRight)))
        .map(|r| r.is_ok())
        .unwrap_or(false);
    if !ok {
        tracing::debug!("TopRight positioner failed, falling back to manual calculation");
        position_top_right_manual(window);
    }
}

/// Manual top-right positioning using monitor dimensions.
fn position_top_right_manual(window: &WebviewWindow) {
    let Some(monitor) = window.current_monitor().ok().flatten() else {
        return;
    };
    let scale = monitor.scale_factor();
    let mon_pos = monitor.position();
    let mon_size = monitor.size();
    let margin = (8.0 * scale) as i32;

    let win_size = window
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(680, 600));

    let x = mon_pos.x + mon_size.width as i32 - win_size.width as i32 - margin;
    let y = mon_pos.y + margin;

    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}
