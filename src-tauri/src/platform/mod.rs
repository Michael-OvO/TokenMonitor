//! Platform-specific code, organized by OS.
//!
//! Each submodule is only compiled on its target platform.
//! Shared abstractions live here; OS-specific implementations in subfolders.

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

// ── Cross-platform window helpers ────────────────────────────────────────────

use tauri::WebviewWindow;

/// Clamp the window position so it stays fully within the monitor work area.
///
/// Keeps the current position and only adjusts if the window exceeds work area bounds.
/// Called after `move_window()` on macOS/Linux. Windows uses `align_to_work_area` instead.
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn clamp_window_to_work_area(window: &WebviewWindow) {
    let Some(monitor) = window.current_monitor().ok().flatten() else {
        tracing::debug!("clamp_window_to_work_area: no current monitor, skipping");
        return;
    };

    let Ok(outer_pos) = window.outer_position() else {
        tracing::debug!("clamp_window_to_work_area: cannot read window position, skipping");
        return;
    };
    let Ok(outer_size) = window.outer_size() else {
        tracing::debug!("clamp_window_to_work_area: cannot read window size, skipping");
        return;
    };

    let scale = monitor.scale_factor();
    let mon_pos = monitor.position();
    let mon_size = monitor.size();

    // Work area in physical pixels (monitor position + size).
    // Tauri outer_position / outer_size are already in physical pixels.
    let work_left = mon_pos.x;
    let work_top = mon_pos.y;
    let work_right = mon_pos.x + mon_size.width as i32;
    let work_bottom = mon_pos.y + mon_size.height as i32;

    // Subtract a margin so the window doesn't touch the very edge.
    let margin = (8.0 * scale) as i32;

    let win_w = outer_size.width as i32;
    let win_h = outer_size.height as i32;

    // Keep the current position; only clamp if out of bounds.
    let mut x = outer_pos.x;
    let mut y = outer_pos.y;

    // Clamp right edge
    if x + win_w > work_right - margin {
        x = work_right - margin - win_w;
    }
    // Clamp left edge
    if x < work_left + margin {
        x = work_left + margin;
    }
    // Clamp bottom edge
    if y + win_h > work_bottom - margin {
        y = work_bottom - margin - win_h;
    }
    // Clamp top edge
    if y < work_top + margin {
        y = work_top + margin;
    }

    // Only move if position actually changed
    if x != outer_pos.x || y != outer_pos.y {
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}
