//! Windows-specific window helpers: DWM corner rounding and work-area positioning.

use tauri::WebviewWindow;
use windows::core::w;
use windows::Win32::Foundation::{HWND, RECT};

use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Get the work area (excludes taskbar) for the monitor containing the given HWND.
pub fn get_work_area(hwnd: HWND) -> Option<RECT> {
    unsafe {
        let hmonitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmonitor, &mut mi).as_bool() {
            Some(mi.rcWork)
        } else {
            None
        }
    }
}

// Removed apply_float_ball_region and FloatBallRegionDirection since
// region clipping causes DWM artifacts on Windows 11 for layered windows.

fn aligned_window_origin(work: RECT, current_rect: RECT, width: i32, height: i32) -> (i32, i32) {
    let mut target_x = current_rect.left;

    if target_x + width > work.right {
        target_x = work.right - width;
    }
    if target_x < work.left {
        target_x = work.left;
    }

    // Always bottom-anchored: pin window bottom to work area bottom (above taskbar).
    // This is a tray popover — position_near_tray always pins bottom to work.bottom,
    // so we maintain that invariant through all resizes regardless of current position.
    let target_y = work.bottom - height;
    let clamped_y = target_y.clamp(work.top, (work.bottom - height).max(work.top));

    (target_x, clamped_y)
}

fn resize_window_pos_flags() -> windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS {
    SWP_NOZORDER | SWP_NOACTIVATE
}

/// Position the window above the system tray area (bottom-right of screen).
///
/// Uses Win32 APIs to find the `TrayNotifyWnd` inside `Shell_TrayWnd`, then
/// places the window so its bottom-right corner is above the tray area.
/// Falls back to work-area bottom-right if the tray cannot be located.
pub fn position_near_tray(window: &WebviewWindow) {
    let Ok(hwnd_raw) = window.hwnd() else { return };
    let hwnd = HWND(hwnd_raw.0 as *mut _);

    let Some(work) = get_work_area(hwnd) else {
        return;
    };

    let mut win_rect = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut win_rect).is_err() {
            return;
        }
    }

    let win_w = win_rect.right - win_rect.left;
    let win_h = win_rect.bottom - win_rect.top;

    // Try to find the system tray area for horizontal centering
    let tray_center_x = unsafe {
        FindWindowW(w!("Shell_TrayWnd"), None)
            .ok()
            .and_then(|taskbar| FindWindowExW(taskbar, None, w!("TrayNotifyWnd"), None).ok())
            .and_then(|tray_notify| {
                let mut tray_rect = RECT::default();
                GetWindowRect(tray_notify, &mut tray_rect)
                    .ok()
                    .map(|_| (tray_rect.left + tray_rect.right) / 2)
            })
    };

    let target_x = match tray_center_x {
        Some(cx) => {
            // Center window horizontally on the tray area, clamped to work area
            let x = cx - win_w / 2;
            x.max(work.left).min(work.right - win_w)
        }
        None => {
            // Fallback: right-align to work area
            work.right - win_w
        }
    };

    // Place window bottom at work area bottom (just above the taskbar)
    let target_y = (work.bottom - win_h).max(work.top);

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            target_x,
            target_y,
            0,
            0,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSIZE,
        );
    }
}

/// Align window so its bottom edge stays at the work area bottom (above taskbar).
/// Used after every window resize.
pub fn align_to_work_area(window: &WebviewWindow) {
    let Ok(hwnd_raw) = window.hwnd() else { return };
    let hwnd = HWND(hwnd_raw.0 as *mut _);

    let Some(work) = get_work_area(hwnd) else {
        return;
    };

    let mut win_rect = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut win_rect).is_err() {
            return;
        }
    }

    let win_h = win_rect.bottom - win_rect.top;
    let win_w = win_rect.right - win_rect.left;

    let (target_x, clamped_y) = aligned_window_origin(work, win_rect, win_w, win_h);

    if target_x != win_rect.left || clamped_y != win_rect.top {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                target_x,
                clamped_y,
                0,
                0,
                SWP_NOZORDER | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
            );
        }
    }
}

/// Atomically sets the physical size of the window and keeps the bottom edge
/// pinned to the work area bottom (above taskbar). The window grows upward.
/// This prevents visual tearing when resizing heights on Windows.
pub fn set_size_and_align(window: &WebviewWindow, physical_width: u32, physical_height: u32) {
    let Ok(hwnd_raw) = window.hwnd() else { return };
    let hwnd = HWND(hwnd_raw.0 as *mut _);

    let Some(work) = get_work_area(hwnd) else {
        return;
    };

    let mut win_rect = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut win_rect).is_err() {
            return;
        }
    }

    let win_w = physical_width as i32;
    let win_h = physical_height as i32;

    let (target_x, clamped_y) = aligned_window_origin(work, win_rect, win_w, win_h);

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            target_x,
            clamped_y,
            win_w,
            win_h,
            resize_window_pos_flags(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn always_bottom_anchored_when_window_at_bottom() {
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 700, 440, 900);

        let (_, target_y) = aligned_window_origin(work, current, 340, 320);
        assert_eq!(target_y, 900 - 320); // bottom pinned to work.bottom
    }

    #[test]
    fn always_bottom_anchored_even_when_window_at_top() {
        // Window at default/stale position near top — still pins to work.bottom
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 0, 440, 280);

        let (_, target_y) = aligned_window_origin(work, current, 340, 420);
        assert_eq!(target_y, 900 - 420); // bottom pinned, not top=0
    }

    #[test]
    fn always_bottom_anchored_when_window_in_middle() {
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 200, 440, 500);

        let (_, target_y) = aligned_window_origin(work, current, 340, 360);
        assert_eq!(target_y, 900 - 360); // bottom pinned, ignores current position
    }

    #[test]
    fn bottom_anchor_caps_at_work_area_when_behind_taskbar() {
        let work = rect(0, 0, 1200, 900);
        // Window bottom extends beyond work area (behind taskbar) — still pins to work.bottom
        let current = rect(100, 640, 440, 940);

        let (_, target_y) = aligned_window_origin(work, current, 340, 320);
        assert_eq!(target_y, 900 - 320); // 580
    }

    #[test]
    fn handles_window_taller_than_work_area() {
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 200, 440, 900);

        // New height exceeds work area
        let (_, target_y) = aligned_window_origin(work, current, 340, 1000);
        // Should clamp to work.top since work.bottom - height < work.top
        assert_eq!(target_y, 0);
    }

    #[test]
    fn resize_window_pos_flags_preserve_z_order_and_focus() {
        let flags = resize_window_pos_flags();

        assert_ne!(flags & SWP_NOZORDER, Default::default());
        assert_ne!(flags & SWP_NOACTIVATE, Default::default());
        // SWP_NOCOPYBITS intentionally omitted — preserving old client bits
        // reduces flicker during animated resizes with WebView2.
        assert_eq!(flags & SWP_NOCOPYBITS, Default::default());
    }
}
