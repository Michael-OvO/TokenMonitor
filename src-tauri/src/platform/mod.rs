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

/// Left edge for a window resized from `current_width` to `new_width` that
/// keeps its horizontal center, so a popover placed under the tray icon stays
/// centered on it when the display zoom changes its width. An unchanged width
/// returns `current_x` untouched, so ordinary height-only resizes keep their
/// left edge exactly. Callers still clamp the result to the work area.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub fn centered_resize_x(current_x: f64, current_width: f64, new_width: f64) -> f64 {
    if new_width == current_width {
        return current_x;
    }
    current_x - (new_width - current_width) / 2.0
}

use tauri::WebviewWindow;

/// Clamp the window position so it stays fully within the monitor work area.
///
/// Keeps the current position and only adjusts if the window exceeds work area bounds.
/// Called after `move_window()` on macOS/Linux. Windows uses `set_size_and_align` instead.
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

#[cfg(test)]
mod tests {
    use super::centered_resize_x;

    #[test]
    fn centered_resize_x_unchanged_width_keeps_left_edge() {
        assert_eq!(centered_resize_x(1234.5, 340.0, 340.0), 1234.5);
    }

    #[test]
    fn centered_resize_x_wider_shifts_left_by_half_the_growth() {
        // 340 -> 510 (zoom 1.5): grows by 170, so the left edge moves 85 left.
        assert_eq!(centered_resize_x(1000.0, 340.0, 510.0), 915.0);
    }

    #[test]
    fn centered_resize_x_narrower_shifts_right_by_half_the_shrink() {
        assert_eq!(centered_resize_x(915.0, 510.0, 340.0), 1000.0);
    }
}
