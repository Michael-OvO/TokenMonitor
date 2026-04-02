use crate::commands::tray::{BarDisplay, TrayConfig};

/// Original tray icon dimensions (@2x retina)
const ICON_W: u32 = 44;
const ICON_H: u32 = 44;

/// Bar area dimensions (@2x).
/// Windows system tray icons render at a different effective size than macOS menu
/// bar icons, so we scale bar width by ~1.5× on non-macOS platforms to keep the
/// visual proportion consistent.
const BAR_GAP: u32 = 8; // gap between icon and bars

#[cfg(target_os = "macos")]
const BAR_W_BOTH: u32 = 68; // width for stacked two-bar display
#[cfg(not(target_os = "macos"))]
const BAR_W_BOTH: u32 = 102; // 68 × 1.5 — wider bars for Windows/Linux tray

#[cfg(target_os = "macos")]
const BAR_W_SINGLE: u32 = 76; // width for single-bar display
#[cfg(not(target_os = "macos"))]
const BAR_W_SINGLE: u32 = 114; // 76 × 1.5

const BAR_H_BOTH: u32 = 6; // bar height when showing two bars
const BAR_H_SINGLE: u32 = 10; // bar height when showing one bar
const BAR_SPACING: u32 = 6; // vertical gap between two bars
const BAR_RADIUS: u32 = 3; // rounded corner radius (@2x)

/// Total canvas width when bars are shown
const CANVAS_W_BOTH: u32 = ICON_W + BAR_GAP + BAR_W_BOTH;
const CANVAS_W_SINGLE: u32 = ICON_W + BAR_GAP + BAR_W_SINGLE;

/// RGBA color
#[derive(Clone, Copy)]
struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

const CLAUDE_COLOR: Color = Color {
    r: 212,
    g: 165,
    b: 116,
    a: 255,
}; // #d4a574
const CODEX_COLOR: Color = Color {
    r: 122,
    g: 175,
    b: 255,
    a: 255,
}; // #7aafff

/// Track and icon colors adapt to menu bar appearance
const TRACK_COLOR_DARK: Color = Color {
    r: 255,
    g: 255,
    b: 255,
    a: 50,
}; // white @ ~20% for dark bar
const TRACK_COLOR_LIGHT: Color = Color {
    r: 0,
    g: 0,
    b: 0,
    a: 40,
}; // black @ ~16% for light bar

/// Detect whether the system tray area uses a dark appearance.
/// Currently defaults to true (dark). Can be improved with platform-specific
/// detection in the future (e.g., Windows registry, Tauri theme API).
pub fn is_menu_bar_dark() -> bool {
    true
}

/// Check if we have any utilization data to render bars with.
fn has_utilization(config: &TrayConfig, claude_util: Option<f64>, codex_util: Option<f64>) -> bool {
    match config.bar_display {
        BarDisplay::Both => claude_util.is_some() || codex_util.is_some(),
        BarDisplay::Single => {
            if config.bar_provider == "claude" {
                claude_util.is_some()
            } else {
                codex_util.is_some()
            }
        }
        BarDisplay::Off => false,
    }
}

fn canvas_width(config: &TrayConfig) -> u32 {
    match config.bar_display {
        BarDisplay::Both => CANVAS_W_BOTH,
        BarDisplay::Single => CANVAS_W_SINGLE,
        BarDisplay::Off => ICON_W,
    }
}

fn bar_width(config: &TrayConfig) -> u32 {
    if config.bar_display == BarDisplay::Both {
        BAR_W_BOTH
    } else {
        BAR_W_SINGLE
    }
}

/// Render a tray icon with optional progress bars.
/// Returns (rgba_bytes, width, height, should_use_template).
/// When should_use_template is true, caller should set icon_as_template(true).
///
/// `dark_bar`: true if the macOS menu bar is dark, false if light.
/// Affects icon color (white on dark, black on light) and track color.
pub fn render_tray_icon(
    base_icon: &[u8],
    config: &TrayConfig,
    claude_util: Option<f64>,
    codex_util: Option<f64>,
    dark_bar: bool,
) -> (Vec<u8>, u32, u32, bool) {
    // If bars are off, or we have no utilization data yet, return original icon for template mode
    if config.bar_display == BarDisplay::Off || !has_utilization(config, claude_util, codex_util) {
        return (base_icon.to_vec(), ICON_W, ICON_H, true);
    }

    // Icon color: white on dark menu bar, keep black on light menu bar
    let icon_rgb: u8 = if dark_bar { 255 } else { 0 };
    let track_color = if dark_bar {
        TRACK_COLOR_DARK
    } else {
        TRACK_COLOR_LIGHT
    };

    let width = canvas_width(config);
    let height = ICON_H;
    let mut buf = vec![0u8; (width * height * 4) as usize];

    // Copy base icon into left side, setting color based on menu bar appearance.
    // The base icon is a template icon (black pixels with alpha mask).
    for y in 0..ICON_H {
        for x in 0..ICON_W {
            let src_idx = ((y * ICON_W + x) * 4) as usize;
            let dst_idx = ((y * width + x) * 4) as usize;
            if src_idx + 3 < base_icon.len() {
                let a = base_icon[src_idx + 3];
                buf[dst_idx] = icon_rgb;
                buf[dst_idx + 1] = icon_rgb;
                buf[dst_idx + 2] = icon_rgb;
                buf[dst_idx + 3] = a; // preserve alpha mask
            }
        }
    }

    let bar_x = ICON_W + BAR_GAP;
    let bar_w = bar_width(config);

    if config.bar_display == BarDisplay::Both {
        // Utilization values are 0–100, normalize to 0–1 for pixel rendering
        let c_util = (claude_util.unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
        let x_util = (codex_util.unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);

        // Vertically center two bars
        let total_h = BAR_H_BOTH * 2 + BAR_SPACING;
        let top_y = (ICON_H - total_h) / 2;

        // Claude bar (top)
        draw_bar(
            &mut buf,
            width,
            bar_x,
            top_y,
            bar_w,
            BAR_H_BOTH,
            c_util,
            &CLAUDE_COLOR,
            &track_color,
        );
        // Codex bar (bottom)
        let bottom_y = top_y + BAR_H_BOTH + BAR_SPACING;
        draw_bar(
            &mut buf,
            width,
            bar_x,
            bottom_y,
            bar_w,
            BAR_H_BOTH,
            x_util,
            &CODEX_COLOR,
            &track_color,
        );
    } else if config.bar_display == BarDisplay::Single {
        let util = if config.bar_provider == "claude" {
            claude_util
        } else {
            codex_util
        };
        let u = (util.unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
        let color = if config.bar_provider == "claude" {
            &CLAUDE_COLOR
        } else {
            &CODEX_COLOR
        };

        // Vertically center single bar
        let top_y = (ICON_H - BAR_H_SINGLE) / 2;
        draw_bar(
            &mut buf,
            width,
            bar_x,
            top_y,
            bar_w,
            BAR_H_SINGLE,
            u,
            color,
            &track_color,
        );
    }

    (buf, width, height, false) // false = don't use template mode, we rendered colors
}

/// Draw a rounded progress bar: track background + filled portion.
#[allow(clippy::too_many_arguments)]
fn draw_bar(
    buf: &mut [u8],
    canvas_w: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    utilization: f64,
    fill_color: &Color,
    track_color: &Color,
) {
    let fill_w = ((w as f64) * utilization).round() as u32;

    for py in 0..h {
        for px in 0..w {
            // Check if pixel is inside rounded rect
            if !in_rounded_rect(px, py, w, h, BAR_RADIUS) {
                continue;
            }

            let dst_x = x + px;
            let dst_y = y + py;
            let idx = ((dst_y * canvas_w + dst_x) * 4) as usize;
            if idx + 3 >= buf.len() {
                continue;
            }

            if px < fill_w && in_rounded_rect(px, py, fill_w.max(BAR_RADIUS * 2), h, BAR_RADIUS) {
                // Filled portion
                buf[idx] = fill_color.r;
                buf[idx + 1] = fill_color.g;
                buf[idx + 2] = fill_color.b;
                buf[idx + 3] = fill_color.a;
            } else {
                // Track background
                buf[idx] = track_color.r;
                buf[idx + 1] = track_color.g;
                buf[idx + 2] = track_color.b;
                buf[idx + 3] = track_color.a;
            }
        }
    }
}

/// Check if (px, py) is inside a rounded rect of size (w, h) with radius r.
fn in_rounded_rect(px: u32, py: u32, w: u32, h: u32, r: u32) -> bool {
    let r = r.min(w / 2).min(h / 2);

    // Check four corners
    if px < r && py < r {
        let dx = r - px;
        let dy = r - py;
        return dx * dx + dy * dy <= r * r;
    }
    if px >= w - r && py < r {
        let dx = px - (w - r - 1);
        let dy = r - py;
        return dx * dx + dy * dy <= r * r;
    }
    if px < r && py >= h - r {
        let dx = r - px;
        let dy = py - (h - r - 1);
        return dx * dx + dy * dy <= r * r;
    }
    if px >= w - r && py >= h - r {
        let dx = px - (w - r - 1);
        let dy = py - (h - r - 1);
        return dx * dx + dy * dy <= r * r;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(bar_display: BarDisplay) -> TrayConfig {
        TrayConfig {
            bar_display,
            ..TrayConfig::default()
        }
    }

    #[test]
    fn off_returns_original_icon_with_template() {
        let icon = vec![0u8; (44 * 44 * 4) as usize];
        let (buf, w, h, tmpl) =
            render_tray_icon(&icon, &make_config(BarDisplay::Off), None, None, true);
        assert_eq!(w, 44);
        assert_eq!(h, 44);
        assert_eq!(buf.len(), icon.len());
        assert!(tmpl, "should use template mode when bars are off");
    }

    #[test]
    fn no_utilization_returns_template() {
        let icon = vec![0u8; (44 * 44 * 4) as usize];
        let (_, w, _, tmpl) =
            render_tray_icon(&icon, &make_config(BarDisplay::Both), None, None, true);
        assert_eq!(w, 44);
        assert!(tmpl, "should use template mode when no utilization data");
    }

    #[test]
    fn both_returns_wider_canvas_no_template() {
        let icon = vec![0u8; (44 * 44 * 4) as usize];
        let (buf, w, h, tmpl) = render_tray_icon(
            &icon,
            &make_config(BarDisplay::Both),
            Some(70.0),
            Some(30.0),
            true,
        );
        assert_eq!(w, CANVAS_W_BOTH);
        assert_eq!(h, 44);
        assert_eq!(buf.len(), (CANVAS_W_BOTH * 44 * 4) as usize);
        assert!(!tmpl, "should NOT use template mode when bars are rendered");
        // Verify some pixels in the bar area have non-zero alpha
        let bar_start = (ICON_W + BAR_GAP) as usize;
        let bar_mid_y = 15usize;
        let idx = (bar_mid_y * w as usize + bar_start + 10) * 4;
        assert!(buf[idx + 3] > 0, "Bar pixel should have non-zero alpha");
    }

    #[test]
    fn single_returns_wider_canvas() {
        let icon = vec![0u8; (44 * 44 * 4) as usize];
        let (buf, w, h, tmpl) = render_tray_icon(
            &icon,
            &make_config(BarDisplay::Single),
            Some(50.0),
            None,
            true,
        );
        assert_eq!(w, CANVAS_W_SINGLE);
        assert_eq!(h, 44);
        assert_eq!(buf.len(), (CANVAS_W_SINGLE * 44 * 4) as usize);
        assert!(!tmpl);
    }

    #[test]
    fn dark_bar_icon_is_white() {
        let mut icon = vec![0u8; (44 * 44 * 4) as usize];
        let idx = (10 * 44 + 10) * 4;
        icon[idx + 3] = 200; // alpha only, black pixel
        let (buf, w, _, _) = render_tray_icon(
            &icon,
            &make_config(BarDisplay::Both),
            Some(50.0),
            Some(50.0),
            true,
        );
        let dst_idx = (10 * w as usize + 10) * 4;
        assert_eq!(buf[dst_idx], 255, "R should be white on dark bar");
        assert_eq!(buf[dst_idx + 1], 255, "G should be white on dark bar");
        assert_eq!(buf[dst_idx + 2], 255, "B should be white on dark bar");
        assert_eq!(buf[dst_idx + 3], 200, "A should be preserved");
    }

    #[test]
    fn light_bar_icon_is_black() {
        let mut icon = vec![0u8; (44 * 44 * 4) as usize];
        let idx = (10 * 44 + 10) * 4;
        icon[idx + 3] = 200;
        let (buf, w, _, _) = render_tray_icon(
            &icon,
            &make_config(BarDisplay::Both),
            Some(50.0),
            Some(50.0),
            false,
        );
        let dst_idx = (10 * w as usize + 10) * 4;
        assert_eq!(buf[dst_idx], 0, "R should be black on light bar");
        assert_eq!(buf[dst_idx + 1], 0, "G should be black on light bar");
        assert_eq!(buf[dst_idx + 2], 0, "B should be black on light bar");
    }

    #[test]
    fn in_rounded_rect_center() {
        assert!(in_rounded_rect(10, 5, 20, 10, 3));
    }

    #[test]
    fn in_rounded_rect_outside_corner() {
        assert!(!in_rounded_rect(0, 0, 20, 10, 5));
    }
}
