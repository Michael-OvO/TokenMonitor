//! Windows taskbar embedding — shows a monitoring panel inside the Windows taskbar.
//!
//! Creates a raw Win32 child window (no WebView) and embeds it into the taskbar
//! between the app list and the system tray area using `SetParent()`.
//! Renders metrics text ("C:45% X:23% $3.2") with GDI, DPI-aware.
//!
//! Handles:
//! - Explorer.exe restart recovery via `TaskbarCreated` registered message
//! - DPI changes via `WM_DPICHANGED`
//! - Taskbar resize via `WM_SIZE`
//! - Graceful fallback if embedding fails

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETTASKBARPOS, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::*;

// ── Constants ────────────────────────────────────────────────────────────────

const CLASS_NAME: PCWSTR = w!("TokenMonitorTaskbarPanel");
const PANEL_MIN_WIDTH: i32 = 400;
const PANEL_MAX_WIDTH: i32 = 600;
const FONT_NAME: PCWSTR = w!("Segoe UI");

// ── Shared state ─────────────────────────────────────────────────────────────

/// Structured metrics displayed in the taskbar panel.
static PANEL_METRICS: OnceLock<Mutex<PanelMetrics>> = OnceLock::new();

/// Whether the panel is currently embedded in the taskbar.
static PANEL_EMBEDDED: AtomicBool = AtomicBool::new(false);

/// Registered message ID for TaskbarCreated (Explorer restart detection).
static WM_TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// Handle to the embedded panel window.
/// SAFETY: HWND is a raw pointer but we only access it from the main thread
/// or behind a Mutex. Wrapping in a Send+Sync newtype for static storage.
struct SendHwnd(Option<HWND>);
unsafe impl Send for SendHwnd {}
unsafe impl Sync for SendHwnd {}

static PANEL_HWND: OnceLock<Mutex<SendHwnd>> = OnceLock::new();

/// Whether dark mode is active on the taskbar (for text color).
static TASKBAR_DARK: AtomicBool = AtomicBool::new(true);

fn panel_metrics() -> &'static Mutex<PanelMetrics> {
    PANEL_METRICS.get_or_init(|| Mutex::new(PanelMetrics::default()))
}

// ── Data types ──────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct PanelMetrics {
    claude_util: Option<f64>,
    codex_util: Option<f64>,
    total_cost: f64,
}

struct TextSegment {
    text: String,
    color: u32, // COLORREF in 0x00BBGGRR format
}

impl PanelMetrics {
    fn segments(&self, is_dark: bool) -> Vec<TextSegment> {
        let dim = if is_dark {
            0x00_88_88_88
        } else {
            0x00_66_66_66
        };
        let claude_color: u32 = 0x00_74_A5_D4; // #D4A574 in BGR
        let codex_color: u32 = 0x00_FF_AF_7A; // #7AAFFF in BGR
        let cost_color: u32 = if is_dark {
            0x00_80_C9_80
        } else {
            0x00_3A_8A_3A
        };

        let mut parts: Vec<Vec<TextSegment>> = Vec::new();

        if let Some(c) = self.claude_util {
            parts.push(vec![
                TextSegment {
                    text: "C:".into(),
                    color: dim,
                },
                TextSegment {
                    text: format!("{}%", c.round() as i64),
                    color: claude_color,
                },
            ]);
        }

        if let Some(x) = self.codex_util {
            parts.push(vec![
                TextSegment {
                    text: "X:".into(),
                    color: dim,
                },
                TextSegment {
                    text: format!("{}%", x.round() as i64),
                    color: codex_color,
                },
            ]);
        }

        if self.total_cost > 0.01 {
            parts.push(vec![TextSegment {
                text: format!("${:.1}", self.total_cost),
                color: cost_color,
            }]);
        }

        if parts.is_empty() {
            let fallback = if is_dark {
                0x00_FF_FF_FF
            } else {
                0x00_00_00_00
            };
            return vec![TextSegment {
                text: "--".into(),
                color: fallback,
            }];
        }

        let mut result = Vec::new();
        for (i, part) in parts.into_iter().enumerate() {
            if i > 0 {
                result.push(TextSegment {
                    text: " · ".into(),
                    color: dim,
                });
            }
            result.extend(part);
        }
        result
    }

    fn flat_text(&self) -> String {
        format_taskbar_text(self.claude_util, self.codex_util, self.total_cost)
    }
}

fn panel_hwnd() -> &'static Mutex<SendHwnd> {
    PANEL_HWND.get_or_init(|| Mutex::new(SendHwnd(None)))
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Initialize and embed the taskbar panel. Call from the main thread.
/// Returns `Ok(true)` if embedding succeeded, `Ok(false)` if it gracefully failed.
pub fn init_taskbar_panel() -> Result<bool> {
    unsafe {
        // Check taskbar orientation — only embed if horizontal at bottom/top
        if !is_taskbar_horizontal() {
            return Ok(false);
        }

        // Register window class
        let instance = GetModuleHandleW(None)?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(panel_wnd_proc),
            hInstance: instance.into(),
            lpszClassName: CLASS_NAME,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH(std::ptr::null_mut()), // transparent/custom paint
            ..Default::default()
        };

        let atom = RegisterClassExW(&wc);
        if atom == 0 {
            // Class might already be registered from a previous init
            let err = windows::core::Error::from_win32();
            if err.code() != HRESULT::from_win32(1410) {
                // ERROR_CLASS_ALREADY_EXISTS
                return Err(err);
            }
        }

        // Register TaskbarCreated message for Explorer restart detection
        let msg_id = RegisterWindowMessageW(w!("TaskbarCreated"));
        WM_TASKBAR_CREATED.store(msg_id, Ordering::Relaxed);

        // Find taskbar and get its dimensions
        let taskbar = find_taskbar_hwnd()?;
        let mut taskbar_rect = RECT::default();
        GetWindowRect(taskbar, &mut taskbar_rect)?;
        let taskbar_height = taskbar_rect.bottom - taskbar_rect.top;

        // Detect taskbar dark/light mode
        detect_taskbar_theme();

        // Create child window
        let dpi = GetDpiForWindow(taskbar);
        let scale = dpi as f32 / 96.0;
        let panel_width = (PANEL_MIN_WIDTH as f32 * scale) as i32;

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            w!("TokenMonitor Panel"),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            panel_width,
            taskbar_height,
            taskbar, // initial parent — will be repositioned
            None,
            instance,
            None,
        )?;

        if hwnd.0.is_null() {
            return Ok(false);
        }

        // Store handle
        panel_hwnd().lock().unwrap().0 = Some(hwnd);

        // Embed into taskbar
        if embed_in_taskbar(hwnd, taskbar).is_ok() {
            PANEL_EMBEDDED.store(true, Ordering::Relaxed);
            let _ = ShowWindow(hwnd, SW_SHOW);
            Ok(true)
        } else {
            let _ = DestroyWindow(hwnd);
            panel_hwnd().lock().unwrap().0 = None;
            Ok(false)
        }
    }
}

/// Update the metrics displayed in the taskbar panel.
#[allow(dead_code)]
pub fn update_panel_metrics(claude_util: Option<f64>, codex_util: Option<f64>, total_cost: f64) {
    if let Ok(mut current) = panel_metrics().lock() {
        current.claude_util = claude_util;
        current.codex_util = codex_util;
        current.total_cost = total_cost;
    }

    // Trigger repaint
    if let Ok(hwnd_guard) = panel_hwnd().lock() {
        if let Some(hwnd) = hwnd_guard.0 {
            unsafe {
                let _ = InvalidateRect(hwnd, None, true);
            }
        }
    }
}

/// Destroy the taskbar panel and clean up.
#[allow(dead_code)]
pub fn destroy_taskbar_panel() {
    PANEL_EMBEDDED.store(false, Ordering::Relaxed);
    if let Ok(mut hwnd_guard) = panel_hwnd().lock() {
        if let Some(hwnd) = hwnd_guard.0.take() {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

/// Check if the panel is currently embedded.
#[allow(dead_code)]
pub fn is_embedded() -> bool {
    PANEL_EMBEDDED.load(Ordering::Relaxed)
}

/// Format metrics into taskbar display text.
pub fn format_taskbar_text(
    claude_util: Option<f64>,
    codex_util: Option<f64>,
    total_cost: f64,
) -> String {
    let mut parts = Vec::new();

    if let Some(c) = claude_util {
        parts.push(format!("C:{}%", c.round() as i64));
    }
    if let Some(x) = codex_util {
        parts.push(format!("X:{}%", x.round() as i64));
    }
    if total_cost > 0.01 {
        parts.push(format!("${:.1}", total_cost));
    }

    if parts.is_empty() {
        "--".to_string()
    } else {
        parts.join(" ")
    }
}

// ── Win32 internals ──────────────────────────────────────────────────────────

unsafe fn find_taskbar_hwnd() -> Result<HWND> {
    let hwnd = FindWindowW(w!("Shell_TrayWnd"), None)?;
    if hwnd.0.is_null() {
        return Err(Error::new(HRESULT(-1), "Shell_TrayWnd not found"));
    }
    Ok(hwnd)
}

unsafe fn is_taskbar_horizontal() -> bool {
    let mut abd = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    SHAppBarMessage(ABM_GETTASKBARPOS, &mut abd);
    let edge = abd.uEdge;
    // ABE_BOTTOM = 3, ABE_TOP = 1 → horizontal
    edge == 3 || edge == 1
}

unsafe fn embed_in_taskbar(panel: HWND, taskbar: HWND) -> Result<()> {
    // Find the tray notify area to position panel to its left
    let tray_notify = FindWindowExW(taskbar, None, w!("TrayNotifyWnd"), None)?;

    let mut taskbar_rect = RECT::default();
    let mut tray_rect = RECT::default();
    GetWindowRect(taskbar, &mut taskbar_rect)?;
    GetWindowRect(tray_notify, &mut tray_rect)?;

    let taskbar_height = taskbar_rect.bottom - taskbar_rect.top;

    // Calculate panel size based on DPI
    let dpi = GetDpiForWindow(taskbar);
    let scale = dpi as f32 / 96.0;

    // Measure text to determine width
    let flat_text = panel_metrics().lock().unwrap().flat_text();
    let panel_width = measure_text_width(&flat_text, dpi).min(PANEL_MAX_WIDTH);
    let panel_width = panel_width.max((PANEL_MIN_WIDTH as f32 * scale) as i32);

    // Position: just left of the tray notify area, in taskbar-local coordinates
    let tray_local_left = tray_rect.left - taskbar_rect.left;
    let panel_x = tray_local_left - panel_width - (8.0 * scale) as i32;
    let panel_y = 0;

    // Reparent into taskbar (should already be child from CreateWindowExW)
    SetParent(panel, taskbar)?;

    // Position within taskbar
    SetWindowPos(
        panel,
        None,
        panel_x,
        panel_y,
        panel_width,
        taskbar_height,
        SWP_NOZORDER | SWP_NOACTIVATE,
    )?;

    Ok(())
}

unsafe fn measure_text_width(text: &str, dpi: u32) -> i32 {
    let scale = dpi as f32 / 96.0;
    let font_height = (28.0 * scale) as i32;

    let hdc = GetDC(None);
    let font = CreateFontW(
        font_height,
        0,
        0,
        0,
        FW_SEMIBOLD.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
        FONT_NAME,
    );
    let old_font = SelectObject(hdc, font);

    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(hdc, &wide, &mut size);

    SelectObject(hdc, old_font);
    let _ = DeleteObject(font);
    ReleaseDC(None, hdc);

    // Add horizontal padding
    size.cx + (32.0 * scale) as i32
}

unsafe fn detect_taskbar_theme() {
    // Check Windows registry for taskbar dark mode
    use windows::Win32::System::Registry::*;

    let mut hkey = HKEY::default();
    let subkey = w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, 0, KEY_READ, &mut hkey).is_ok() {
        let mut data: u32 = 1;
        let mut size = std::mem::size_of::<u32>() as u32;
        let value_name = w!("SystemUsesLightTheme");
        if RegQueryValueExW(
            hkey,
            value_name,
            None,
            None,
            Some(std::ptr::from_mut(&mut data).cast()),
            Some(&mut size),
        )
        .is_ok()
        {
            // data == 0 means dark mode, data == 1 means light mode
            TASKBAR_DARK.store(data == 0, Ordering::Relaxed);
        }
        let _ = RegCloseKey(hkey);
    }
}

// ── Window Procedure ─────────────────────────────────────────────────────────

unsafe extern "system" fn panel_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Handle TaskbarCreated (Explorer restart)
    let taskbar_created_id = WM_TASKBAR_CREATED.load(Ordering::Relaxed);
    if taskbar_created_id != 0 && msg == taskbar_created_id {
        // Explorer restarted — re-embed after a short delay
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let _ = re_embed_panel();
        });
        return LRESULT(0);
    }

    match msg {
        WM_PAINT => {
            paint_panel(hwnd);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // DPI changed — re-layout
            if let Ok(taskbar) = find_taskbar_hwnd() {
                let _ = embed_in_taskbar(hwnd, taskbar);
            }
            let _ = InvalidateRect(hwnd, None, true);
            LRESULT(0)
        }
        WM_THEMECHANGED | WM_SETTINGCHANGE => {
            detect_taskbar_theme();
            let _ = InvalidateRect(hwnd, None, true);
            LRESULT(0)
        }
        WM_DESTROY => {
            PANEL_EMBEDDED.store(false, Ordering::Relaxed);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_panel(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect).unwrap_or_default();

    let dpi = GetDpiForWindow(hwnd);
    let scale = dpi as f32 / 96.0;
    let font_height = (28.0 * scale) as i32;

    // Background: match taskbar color
    let is_dark = TASKBAR_DARK.load(Ordering::Relaxed);
    let bg_color = if is_dark {
        COLORREF(0x00_2D_2D_2D)
    } else {
        COLORREF(0x00_F3_F3_F3)
    };
    let bg_brush = CreateSolidBrush(bg_color);
    FillRect(hdc, &rect, bg_brush);
    let _ = DeleteObject(bg_brush);

    // Create font
    let font = CreateFontW(
        font_height,
        0,
        0,
        0,
        FW_SEMIBOLD.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
        FONT_NAME,
    );
    let old_font = SelectObject(hdc, font);
    SetBkMode(hdc, TRANSPARENT);

    // Build colored segments from structured metrics
    let metrics = panel_metrics().lock().unwrap().clone();
    let segments = metrics.segments(is_dark);

    // Draw each segment left-to-right with its own color, vertically centered
    let padding = (16.0 * scale) as i32;
    let mut x = rect.left + padding;
    let center_y = (rect.top + rect.bottom) / 2;

    for seg in &segments {
        SetTextColor(hdc, COLORREF(seg.color));
        let wide: Vec<u16> = seg.text.encode_utf16().collect();
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, &wide, &mut size);

        let text_y = center_y - size.cy / 2;
        let _ = TextOutW(hdc, x, text_y, &wide);
        x += size.cx;
    }

    // Cleanup
    SelectObject(hdc, old_font);
    let _ = DeleteObject(font);
    let _ = EndPaint(hwnd, &ps);
}

fn re_embed_panel() -> Result<()> {
    let hwnd = panel_hwnd()
        .lock()
        .unwrap()
        .0
        .ok_or_else(|| Error::new(HRESULT(-1), "No panel HWND to re-embed"))?;

    unsafe {
        let taskbar = find_taskbar_hwnd()?;
        detect_taskbar_theme();
        embed_in_taskbar(hwnd, taskbar)?;
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = InvalidateRect(hwnd, None, true);
    }

    PANEL_EMBEDDED.store(true, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_taskbar_text_both_providers() {
        let text = format_taskbar_text(Some(45.3), Some(23.7), 3.2);
        assert_eq!(text, "C:45% X:24% $3.2");
    }

    #[test]
    fn format_taskbar_text_claude_only() {
        let text = format_taskbar_text(Some(80.0), None, 12.5);
        assert_eq!(text, "C:80% $12.5");
    }

    #[test]
    fn format_taskbar_text_no_data() {
        let text = format_taskbar_text(None, None, 0.0);
        assert_eq!(text, "--");
    }

    #[test]
    fn format_taskbar_text_cost_only() {
        let text = format_taskbar_text(None, None, 5.67);
        assert_eq!(text, "$5.7");
    }
}
