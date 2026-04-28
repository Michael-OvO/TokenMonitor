//! Windows-specific window helpers: DWM corner rounding and work-area positioning.

use tauri::WebviewWindow;
use windows::core::w;
use windows::Win32::Foundation::{HWND, RECT};

use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorCorner {
    TopLeft = 0,
    TopRight = 1,
    BottomLeft = 2,
    BottomRight = 3,
}

impl AnchorCorner {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::TopLeft,
            1 => Self::TopRight,
            2 => Self::BottomLeft,
            _ => Self::BottomRight,
        }
    }

    fn is_bottom(self) -> bool {
        matches!(self, Self::BottomLeft | Self::BottomRight)
    }
}

static ANCHOR: AtomicU8 = AtomicU8::new(AnchorCorner::BottomRight as u8);

fn current_anchor() -> AnchorCorner {
    AnchorCorner::from_u8(ANCHOR.load(Ordering::Relaxed))
}

pub fn is_anchor_bottom() -> bool {
    current_anchor().is_bottom()
}

fn detect_anchor_corner(work: RECT, win_x: i32, win_y: i32, win_w: i32, win_h: i32) -> AnchorCorner {
    let work_cx = (work.left + work.right) / 2;
    let work_cy = (work.top + work.bottom) / 2;
    let win_cx = win_x + win_w / 2;
    let win_cy = win_y + win_h / 2;
    match (win_cx >= work_cx, win_cy >= work_cy) {
        (false, false) => AnchorCorner::TopLeft,
        (true, false) => AnchorCorner::TopRight,
        (false, true) => AnchorCorner::BottomLeft,
        (true, true) => AnchorCorner::BottomRight,
    }
}

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

fn aligned_window_origin(
    work: RECT,
    current_rect: RECT,
    width: i32,
    height: i32,
    anchor: AnchorCorner,
) -> (i32, i32) {
    let mut target_x = current_rect.left;

    if target_x + width > work.right {
        target_x = work.right - width;
    }
    if target_x < work.left {
        target_x = work.left;
    }

    let target_y = if anchor.is_bottom() {
        work.bottom - height
    } else {
        work.top
    };
    let clamped_y = target_y.clamp(work.top, (work.bottom - height).max(work.top));

    (target_x, clamped_y)
}

fn anchored_resize_origin(
    work: RECT,
    current_rect: RECT,
    width: i32,
    height: i32,
    anchor: AnchorCorner,
) -> (i32, i32) {
    let mut target_x = current_rect.left;
    if target_x + width > work.right {
        target_x = work.right - width;
    }
    if target_x < work.left {
        target_x = work.left;
    }

    let target_y = if anchor.is_bottom() {
        work.bottom - height
    } else {
        current_rect.top
    };
    let clamped_y = target_y.clamp(work.top, (work.bottom - height).max(work.top));

    (target_x, clamped_y)
}

fn resize_window_pos_flags() -> windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS {
    SWP_NOZORDER | SWP_NOACTIVATE
}

/// Position the window near the system tray area.
///
/// Uses Win32 APIs to find the `TrayNotifyWnd` inside `Shell_TrayWnd`, then
/// places the window centered on the tray area. Detects which corner of the
/// screen the final position is nearest to and stores it as the resize anchor.
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
            let x = cx - win_w / 2;
            x.max(work.left).min(work.right - win_w)
        }
        None => work.right - win_w,
    };

    let tray_center_y = unsafe {
        FindWindowW(w!("Shell_TrayWnd"), None)
            .ok()
            .and_then(|taskbar| {
                let mut tray_rect = RECT::default();
                GetWindowRect(taskbar, &mut tray_rect)
                    .ok()
                    .map(|_| (tray_rect.top + tray_rect.bottom) / 2)
            })
    };

    let work_cy = (work.top + work.bottom) / 2;
    let tray_is_top = tray_center_y.is_some_and(|cy| cy < work_cy);

    let target_y = if tray_is_top {
        work.top
    } else {
        (work.bottom - win_h).max(work.top)
    };

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

    let anchor = detect_anchor_corner(work, target_x, target_y, win_w, win_h);
    ANCHOR.store(anchor as u8, Ordering::Relaxed);
}

/// Bring the window to the foreground, ensuring it receives focus.
///
/// Uses `SetForegroundWindow` instead of `SetFocus` because the latter
/// requires the calling thread to already own the foreground — which is
/// not guaranteed on first show from a tray-icon click handler.
pub fn activate_window(window: &WebviewWindow) {
    let Ok(hwnd_raw) = window.hwnd() else { return };
    let hwnd = HWND(hwnd_raw.0 as *mut _);
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Align window to the work area using the last detected anchor corner.
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

    let (target_x, clamped_y) = aligned_window_origin(work, win_rect, win_w, win_h, current_anchor());

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

/// Atomically sets the physical size of the window and keeps the anchored edge
/// pinned to the work area. The window grows away from the anchor corner.
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

    let anchor = current_anchor();
    let (target_x, clamped_y) = anchored_resize_origin(work, win_rect, win_w, win_h, anchor);
    let old_bottom = win_rect.bottom;
    let new_bottom = clamped_y + win_h;

    tracing::debug!(
        ?anchor,
        old_top = win_rect.top,
        old_bottom,
        new_top = clamped_y,
        new_bottom,
        old_h = win_rect.bottom - win_rect.top,
        new_h = win_h,
        work_bottom = work.bottom,
        "set_size_and_align"
    );

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
    fn bottom_right_anchor_when_window_at_bottom() {
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 700, 440, 900);

        let (_, target_y) = aligned_window_origin(work, current, 340, 320, AnchorCorner::BottomRight);
        assert_eq!(target_y, 900 - 320);
    }

    #[test]
    fn bottom_anchor_even_when_window_at_top() {
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 0, 440, 280);

        let (_, target_y) = aligned_window_origin(work, current, 340, 420, AnchorCorner::BottomRight);
        assert_eq!(target_y, 900 - 420);
    }

    #[test]
    fn bottom_anchor_when_window_in_middle() {
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 200, 440, 500);

        let (_, target_y) = aligned_window_origin(work, current, 340, 360, AnchorCorner::BottomRight);
        assert_eq!(target_y, 900 - 360);
    }

    #[test]
    fn bottom_anchor_caps_at_work_area_when_behind_taskbar() {
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 640, 440, 940);

        let (_, target_y) = aligned_window_origin(work, current, 340, 320, AnchorCorner::BottomRight);
        assert_eq!(target_y, 900 - 320);
    }

    #[test]
    fn handles_window_taller_than_work_area() {
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 200, 440, 900);

        let (_, target_y) = aligned_window_origin(work, current, 340, 1000, AnchorCorner::BottomRight);
        assert_eq!(target_y, 0);
    }

    #[test]
    fn top_anchor_pins_to_work_area_top() {
        let work = rect(0, 40, 1200, 900);
        let current = rect(100, 40, 440, 360);

        let (_, target_y) = aligned_window_origin(work, current, 340, 320, AnchorCorner::TopRight);
        assert_eq!(target_y, 40);
    }

    #[test]
    fn top_left_anchor_pins_to_top() {
        let work = rect(0, 50, 1200, 900);
        let current = rect(10, 50, 350, 370);

        let (_, target_y) = aligned_window_origin(work, current, 340, 320, AnchorCorner::TopLeft);
        assert_eq!(target_y, 50);
    }

    #[test]
    fn top_anchor_taller_than_work_area_clamps() {
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 0, 440, 900);

        let (_, target_y) = aligned_window_origin(work, current, 340, 1000, AnchorCorner::TopRight);
        assert_eq!(target_y, 0);
    }

    #[test]
    fn detect_anchor_bottom_right() {
        let work = rect(0, 0, 1920, 1040);
        assert_eq!(detect_anchor_corner(work, 1500, 700, 340, 320), AnchorCorner::BottomRight);
    }

    #[test]
    fn detect_anchor_top_left() {
        let work = rect(0, 0, 1920, 1040);
        assert_eq!(detect_anchor_corner(work, 100, 50, 340, 320), AnchorCorner::TopLeft);
    }

    #[test]
    fn detect_anchor_top_right() {
        let work = rect(0, 40, 1920, 1080);
        assert_eq!(detect_anchor_corner(work, 1500, 50, 340, 320), AnchorCorner::TopRight);
    }

    #[test]
    fn detect_anchor_bottom_left() {
        let work = rect(0, 0, 1920, 1040);
        assert_eq!(detect_anchor_corner(work, 100, 700, 340, 320), AnchorCorner::BottomLeft);
    }

    #[test]
    fn resize_window_pos_flags_preserve_z_order_and_focus() {
        let flags = resize_window_pos_flags();

        assert_ne!(flags & SWP_NOZORDER, Default::default());
        assert_ne!(flags & SWP_NOACTIVATE, Default::default());
        assert_eq!(flags & SWP_NOCOPYBITS, Default::default());
    }

    #[test]
    fn anchored_resize_bottom_pins_to_work_bottom() {
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 500, 440, 820);

        let (_, target_y) =
            anchored_resize_origin(work, current, 340, 420, AnchorCorner::BottomRight);

        assert_eq!(target_y, 900 - 420);
    }

    #[test]
    fn anchored_resize_top_keeps_existing_top() {
        let work = rect(0, 40, 1200, 900);
        let current = rect(100, 80, 440, 400);

        let (_, target_y) = anchored_resize_origin(work, current, 340, 420, AnchorCorner::TopRight);

        assert_eq!(target_y, 80);
    }

    #[test]
    fn anchored_resize_bottom_clamps_when_too_tall() {
        let work = rect(0, 0, 1200, 900);
        let current = rect(100, 500, 440, 820);

        let (_, target_y) =
            anchored_resize_origin(work, current, 340, 1200, AnchorCorner::BottomRight);

        assert_eq!(target_y, 0);
    }
}
