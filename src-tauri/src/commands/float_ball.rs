use super::AppState;
use serde::Serialize;
use tauri::{Manager, WebviewWindow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FloatBallAnchor {
    Top,
    Left,
    Right,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FloatBallExpandDirection {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FloatBallState {
    pub(crate) anchor: Option<FloatBallAnchor>,
    pub(crate) expand_direction: FloatBallExpandDirection,
    pub(crate) expanded: bool,
}

impl Default for FloatBallState {
    fn default() -> Self {
        Self {
            // Anchor implicitly depends on screen pos, but left anchor expands right
            anchor: Some(FloatBallAnchor::Left),
            expand_direction: FloatBallExpandDirection::Right,
            expanded: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FloatBallLayout {
    pub expand_direction: FloatBallExpandDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FloatBallRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FloatBallBounds {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FloatBallSizes {
    ball: i32,
    expanded_width: i32,
    expanded_height: i32,
    expand_margin: i32,
}

impl FloatBallBounds {
    fn clamp_x(self, x: i32, width: i32) -> i32 {
        let max = self.right - width;
        if max <= self.left {
            self.left
        } else {
            x.clamp(self.left, max)
        }
    }

    fn clamp_y(self, y: i32, height: i32) -> i32 {
        let max = self.bottom - height;
        if max <= self.top {
            self.top
        } else {
            y.clamp(self.top, max)
        }
    }
}

fn float_ball_bounds(window: &WebviewWindow) -> Result<FloatBallBounds, String> {
    #[cfg(target_os = "windows")]
    {
        let hwnd_raw = window.hwnd().map_err(|e| e.to_string())?;
        let hwnd = windows::Win32::Foundation::HWND(hwnd_raw.0 as *mut _);
        if let Some(work) = crate::platform::windows::window::get_work_area(hwnd) {
            return Ok(FloatBallBounds {
                left: work.left,
                top: work.top,
                right: work.right,
                bottom: work.bottom,
            });
        }
    }

    let monitor = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .or_else(|| {
            tracing::debug!("float_ball_bounds: current_monitor returned None, using primary");
            window.primary_monitor().ok().flatten()
        })
        .ok_or_else(|| String::from("No monitor found for float ball"))?;
    let position = monitor.position();
    let size = monitor.size();

    Ok(FloatBallBounds {
        left: position.x,
        top: position.y,
        right: position.x + size.width as i32,
        bottom: position.y + size.height as i32,
    })
}

fn current_float_ball_rect(window: &WebviewWindow) -> Result<FloatBallRect, String> {
    let position = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    Ok(FloatBallRect {
        x: position.x,
        y: position.y,
        width: size.width as i32,
        height: size.height as i32,
    })
}

fn ball_rect_from_window(
    rect: FloatBallRect,
    state: FloatBallState,
    ball_size: i32,
) -> FloatBallRect {
    if !state.expanded || rect.width <= ball_size + 1 {
        return FloatBallRect {
            x: rect.x,
            y: rect.y,
            width: ball_size,
            height: ball_size,
        };
    }

    let x = match state.expand_direction {
        FloatBallExpandDirection::Right => rect.x,
        FloatBallExpandDirection::Left => rect.x + rect.width - ball_size,
    };

    let y = match state.anchor {
        Some(FloatBallAnchor::Top) => rect.y,
        Some(FloatBallAnchor::Left)
        | Some(FloatBallAnchor::Right)
        | Some(FloatBallAnchor::Bottom)
        | None => rect.y + rect.height - ball_size,
    };

    FloatBallRect {
        x,
        y,
        width: ball_size,
        height: ball_size,
    }
}

fn inset_bounds(bounds: FloatBallBounds, margin: i32) -> FloatBallBounds {
    FloatBallBounds {
        left: bounds.left + margin,
        top: bounds.top + margin,
        right: bounds.right - margin,
        bottom: bounds.bottom - margin,
    }
}

fn collapsed_rect_from_ball(
    bounds: FloatBallBounds,
    ball_rect: FloatBallRect,
    anchor: Option<FloatBallAnchor>,
    ball_size: i32,
) -> FloatBallRect {
    let x = match anchor {
        Some(FloatBallAnchor::Left) => bounds.left - (ball_size / 2),
        Some(FloatBallAnchor::Right) => bounds.right - (ball_size / 2),
        Some(FloatBallAnchor::Top) | Some(FloatBallAnchor::Bottom) | None => {
            bounds.clamp_x(ball_rect.x, ball_size)
        }
    };
    let y = match anchor {
        Some(FloatBallAnchor::Top) => bounds.top - (ball_size / 2),
        Some(FloatBallAnchor::Bottom) => bounds.bottom - (ball_size / 2),
        Some(FloatBallAnchor::Left) | Some(FloatBallAnchor::Right) | None => {
            bounds.clamp_y(ball_rect.y, ball_size)
        }
    };

    FloatBallRect {
        x,
        y,
        width: ball_size,
        height: ball_size,
    }
}

fn expanded_rect_from_ball(
    bounds: FloatBallBounds,
    ball_rect: FloatBallRect,
    anchor: Option<FloatBallAnchor>,
    expand_direction: FloatBallExpandDirection,
    sizes: FloatBallSizes,
) -> FloatBallRect {
    let inner_bounds = inset_bounds(bounds, sizes.expand_margin);

    let x = match anchor {
        Some(FloatBallAnchor::Left) => inner_bounds.left,
        Some(FloatBallAnchor::Right) => inner_bounds.right - sizes.expanded_width,
        Some(FloatBallAnchor::Top) | Some(FloatBallAnchor::Bottom) | None => match expand_direction
        {
            FloatBallExpandDirection::Right => {
                inner_bounds.clamp_x(ball_rect.x, sizes.expanded_width)
            }
            FloatBallExpandDirection::Left => inner_bounds.clamp_x(
                ball_rect.x - (sizes.expanded_width - sizes.ball),
                sizes.expanded_width,
            ),
        },
    };
    let y = match anchor {
        Some(FloatBallAnchor::Top) => inner_bounds.top,
        Some(FloatBallAnchor::Bottom) => inner_bounds.bottom - sizes.expanded_height,
        Some(FloatBallAnchor::Left) | Some(FloatBallAnchor::Right) | None => inner_bounds.clamp_y(
            ball_rect.y + sizes.ball - sizes.expanded_height,
            sizes.expanded_height,
        ),
    };

    FloatBallRect {
        x,
        y,
        width: sizes.expanded_width,
        height: sizes.expanded_height,
    }
}

fn choose_float_ball_anchor(
    bounds: FloatBallBounds,
    ball_rect: FloatBallRect,
    threshold: i32,
) -> Option<FloatBallAnchor> {
    let candidates = [
        ((ball_rect.y - bounds.top).abs(), FloatBallAnchor::Top),
        (
            ((bounds.bottom - ball_rect.height) - ball_rect.y).abs(),
            FloatBallAnchor::Bottom,
        ),
        ((ball_rect.x - bounds.left).abs(), FloatBallAnchor::Left),
        (
            ((bounds.right - ball_rect.width) - ball_rect.x).abs(),
            FloatBallAnchor::Right,
        ),
    ];

    let (min_distance, closest) = candidates.iter().min_by_key(|(d, _)| *d).copied().unwrap(); // safe: non-empty array

    if min_distance > threshold {
        None
    } else {
        Some(closest)
    }
}

fn choose_expand_direction(
    anchor: Option<FloatBallAnchor>,
    bounds: FloatBallBounds,
    ball_rect: FloatBallRect,
    expanded_width: i32,
    expand_margin: i32,
    current_direction: FloatBallExpandDirection,
) -> FloatBallExpandDirection {
    let inner_bounds = inset_bounds(bounds, expand_margin);
    let room_right = inner_bounds.right - (ball_rect.x + expanded_width);
    let room_left = ball_rect.x - (expanded_width - ball_rect.width) - inner_bounds.left;

    let preferred = match anchor {
        Some(FloatBallAnchor::Left) => FloatBallExpandDirection::Right,
        Some(FloatBallAnchor::Right) => FloatBallExpandDirection::Left,
        Some(FloatBallAnchor::Top) | Some(FloatBallAnchor::Bottom) | None => {
            if room_right > room_left {
                FloatBallExpandDirection::Right
            } else if room_left > room_right {
                FloatBallExpandDirection::Left
            } else {
                current_direction
            }
        }
    };

    // Verify the preferred direction has enough room; flip if not.
    match preferred {
        FloatBallExpandDirection::Right if room_right < 0 && room_left >= 0 => {
            FloatBallExpandDirection::Left
        }
        FloatBallExpandDirection::Left if room_left < 0 && room_right >= 0 => {
            FloatBallExpandDirection::Right
        }
        _ => preferred,
    }
}

fn layout_float_ball_rect(
    bounds: FloatBallBounds,
    ball_rect: FloatBallRect,
    anchor: Option<FloatBallAnchor>,
    expanded: bool,
    expand_direction: FloatBallExpandDirection,
    sizes: FloatBallSizes,
) -> FloatBallRect {
    if !expanded {
        return collapsed_rect_from_ball(bounds, ball_rect, anchor, sizes.ball);
    }

    expanded_rect_from_ball(bounds, ball_rect, anchor, expand_direction, sizes)
}

fn apply_float_ball_window_rect(window: &WebviewWindow, rect: FloatBallRect) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd_raw) = window.hwnd() {
            let hwnd = windows::Win32::Foundation::HWND(hwnd_raw.0 as *mut _);
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                    hwnd,
                    None,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
                );
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        window
            .set_size(tauri::PhysicalSize::new(
                rect.width as u32,
                rect.height as u32,
            ))
            .map_err(|e| e.to_string())?;
        window
            .set_position(tauri::PhysicalPosition::new(rect.x, rect.y))
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

// ── Float Ball commands ─────────────────────────────────────────────

const BALL_SIZE: f64 = 56.0;
const EXPANDED_W: f64 = 152.0; // ball (56) + panel (56 × 1.7 ≈ 96)
const EXPANDED_H: f64 = 56.0; // same height as collapsed ball
const SNAP_THRESHOLD_PX: f64 = (BALL_SIZE / 2.0) * 1.5;
const EXPAND_MARGIN: f64 = 8.0; // minimum gap from screen edges when expanded

fn compute_scaled_sizes(scale: f64) -> FloatBallSizes {
    FloatBallSizes {
        ball: (BALL_SIZE * scale).round() as i32,
        expanded_width: (EXPANDED_W * scale).round() as i32,
        expanded_height: (EXPANDED_H * scale).round() as i32,
        expand_margin: (EXPAND_MARGIN * scale).round() as i32,
    }
}

/// Create and show the floating ball window. Noop if it already exists.
#[tauri::command]
pub async fn create_float_ball(app: tauri::AppHandle) -> Result<(), String> {
    if app.get_webview_window("float-ball").is_some() {
        return Ok(());
    }

    // Suppress main-window auto-hide: creating a new window may steal focus.
    app.state::<AppState>()
        .suppress_auto_hide
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let window = tauri::WebviewWindowBuilder::new(
        &app,
        "float-ball",
        tauri::WebviewUrl::App("float-ball.html".into()),
    )
    .title("TokenMonitor Ball")
    .inner_size(BALL_SIZE, BALL_SIZE)
    .transparent(true)
    .shadow(false)
    .decorations(false)
    .always_on_top(true)
    .visible(true)
    .skip_taskbar(true)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .build()
    .map_err(|e| e.to_string())?;

    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let sizes = compute_scaled_sizes(scale);
    let bounds = float_ball_bounds(&window)?;
    let rect = FloatBallRect {
        x: bounds.right - (sizes.ball / 2),
        y: bounds.bottom - sizes.ball,
        width: sizes.ball,
        height: sizes.ball,
    };
    let initial_state = FloatBallState {
        anchor: Some(FloatBallAnchor::Right),
        expand_direction: FloatBallExpandDirection::Left,
        expanded: false,
    };
    apply_float_ball_window_rect(&window, rect)?;

    {
        let state = app.state::<AppState>();
        let mut float_state = state.float_ball_state.write().await;
        *float_state = initial_state;
    }

    Ok(())
}

/// Destroy the floating ball window.
#[tauri::command]
pub async fn destroy_float_ball(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("float-ball") {
        window.close().map_err(|e| e.to_string())?;
    }

    let state = app.state::<AppState>();
    let mut float_state = state.float_ball_state.write().await;
    *float_state = FloatBallState::default();

    Ok(())
}

/// Resize the float ball window for expand/collapse.
#[tauri::command]
pub async fn set_float_ball_expanded(
    app: tauri::AppHandle,
    expanded: bool,
) -> Result<FloatBallLayout, String> {
    let Some(window) = app.get_webview_window("float-ball") else {
        return Ok(FloatBallLayout {
            expand_direction: FloatBallExpandDirection::Right,
        });
    };

    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let sizes = compute_scaled_sizes(scale);
    let bounds = float_ball_bounds(&window)?;
    let state = app.state::<AppState>();
    let mut float_state = state.float_ball_state.write().await;
    let window_rect = current_float_ball_rect(&window)?;
    let ball_rect = ball_rect_from_window(window_rect, *float_state, sizes.ball);
    let direction = choose_expand_direction(
        float_state.anchor,
        bounds,
        ball_rect,
        sizes.expanded_width,
        sizes.expand_margin,
        float_state.expand_direction,
    );
    let target_rect = layout_float_ball_rect(
        bounds,
        ball_rect,
        float_state.anchor,
        expanded,
        direction,
        sizes,
    );

    apply_float_ball_window_rect(&window, target_rect)?;

    float_state.expand_direction = direction;
    float_state.expanded = expanded;

    Ok(FloatBallLayout {
        expand_direction: direction,
    })
}

/// Move the float ball window to the given physical screen coordinates.
/// Used by the frontend's pointer-capture drag implementation.
#[tauri::command]
pub async fn move_float_ball_to(app: tauri::AppHandle, x: i32, y: i32) -> Result<(), String> {
    let Some(window) = app.get_webview_window("float-ball") else {
        return Ok(());
    };

    let state = app.state::<AppState>();
    let mut float_state = state.float_ball_state.write().await;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let sizes = compute_scaled_sizes(scale);

    let bounds = float_ball_bounds(&window)?;
    let (win_w, win_h) = if float_state.expanded {
        (sizes.expanded_width, sizes.expanded_height)
    } else {
        (sizes.ball, sizes.ball)
    };

    let (clamped_x, clamped_y) = if float_state.expanded {
        let inner_bounds = inset_bounds(bounds, sizes.expand_margin);
        (
            inner_bounds.clamp_x(x, win_w),
            inner_bounds.clamp_y(y, win_h),
        )
    } else {
        // Clamp so that at least half the ball stays on screen while dragging.
        let half = sizes.ball / 2;
        (
            x.clamp(bounds.left - half, bounds.right - win_w + half),
            y.clamp(bounds.top - half, bounds.bottom - win_h + half),
        )
    };

    let rect = FloatBallRect {
        x: clamped_x,
        y: clamped_y,
        width: win_w,
        height: win_h,
    };

    float_state.anchor = None;
    apply_float_ball_window_rect(&window, rect)?;
    Ok(())
}

/// Snap the float ball to the nearest screen edge (if within threshold).
#[tauri::command]
pub async fn snap_float_ball(app: tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("float-ball") else {
        return Ok(());
    };

    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let sizes = compute_scaled_sizes(scale);
    let snap_threshold_px = (SNAP_THRESHOLD_PX * scale).round() as i32;
    let bounds = float_ball_bounds(&window)?;
    let state = app.state::<AppState>();
    let mut float_state = state.float_ball_state.write().await;
    if float_state.expanded {
        return Ok(());
    }
    let window_rect = current_float_ball_rect(&window)?;
    let ball_rect = ball_rect_from_window(window_rect, *float_state, sizes.ball);

    let Some(anchor) = choose_float_ball_anchor(bounds, ball_rect, snap_threshold_px) else {
        // Ball is far from all edges — stay in place.
        float_state.anchor = None;
        return Ok(());
    };

    let direction = choose_expand_direction(
        Some(anchor),
        bounds,
        ball_rect,
        sizes.expanded_width,
        sizes.expand_margin,
        float_state.expand_direction,
    );
    let target_rect = layout_float_ball_rect(
        bounds,
        ball_rect,
        Some(anchor),
        float_state.expanded,
        direction,
        sizes,
    );

    apply_float_ball_window_rect(&window, target_rect)?;
    float_state.anchor = Some(anchor);
    float_state.expand_direction = direction;

    Ok(())
}

// ── Taskbar panel commands (Windows only) ────────────────────────────

/// Initialize and embed the taskbar panel. Windows only; noop on other platforms.
#[tauri::command]
pub async fn init_taskbar_panel(app: tauri::AppHandle) -> Result<bool, String> {
    // Suppress main-window auto-hide: taskbar panel creation may shift focus.
    app.state::<AppState>()
        .suppress_auto_hide
        .store(true, std::sync::atomic::Ordering::SeqCst);

    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::taskbar::init_taskbar_panel().map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(false)
    }
}

/// Destroy the taskbar panel. Windows only; noop on other platforms.
#[tauri::command]
pub async fn destroy_taskbar_panel_cmd() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::taskbar::destroy_taskbar_panel();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bounds() -> FloatBallBounds {
        FloatBallBounds {
            left: 0,
            top: 0,
            right: 600,
            bottom: 400,
        }
    }

    fn sizes(margin: i32) -> FloatBallSizes {
        FloatBallSizes {
            ball: 56,
            expanded_width: 152,
            expanded_height: 56,
            expand_margin: margin,
        }
    }

    #[test]
    fn choose_expand_direction_prefers_right_for_bottom_anchor_when_space_exists() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 180,
            y: 344,
            width: 56,
            height: 56,
        };

        assert_eq!(
            choose_expand_direction(
                Some(FloatBallAnchor::Bottom),
                bounds,
                ball_rect,
                152,
                8,
                FloatBallExpandDirection::Right,
            ),
            FloatBallExpandDirection::Right
        );
    }

    #[test]
    fn choose_expand_direction_flips_left_when_right_space_runs_out() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 480,
            y: 344,
            width: 56,
            height: 56,
        };

        assert_eq!(
            choose_expand_direction(
                Some(FloatBallAnchor::Bottom),
                bounds,
                ball_rect,
                152,
                8,
                FloatBallExpandDirection::Right,
            ),
            FloatBallExpandDirection::Left
        );
    }

    #[test]
    fn expanded_bottom_anchor_stays_within_bounds_with_margin() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 420,
            y: 344,
            width: 56,
            height: 56,
        };

        let rect = layout_float_ball_rect(
            bounds,
            ball_rect,
            Some(FloatBallAnchor::Bottom),
            true,
            FloatBallExpandDirection::Left,
            sizes(8),
        );

        // Expanded rect stays within bounds with 8px margin
        assert_eq!(rect.y, 400 - 56 - 8); // bottom - expanded_height - margin
        assert!(rect.x >= 8); // at least margin from left
        assert!(rect.x + rect.width <= 592); // at most margin from right
    }

    #[test]
    fn collapsed_right_anchor_keeps_half_the_ball_hidden_at_the_edge() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 560,
            y: 200,
            width: 56,
            height: 56,
        };

        let rect = layout_float_ball_rect(
            bounds,
            ball_rect,
            Some(FloatBallAnchor::Right),
            false,
            FloatBallExpandDirection::Left,
            sizes(0),
        );

        assert_eq!(rect.x, 572);
        assert_eq!(rect.width, 56);
    }

    #[test]
    fn collapsed_bottom_anchor_half_hidden() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 300,
            y: 380,
            width: 56,
            height: 56,
        };

        let rect = layout_float_ball_rect(
            bounds,
            ball_rect,
            Some(FloatBallAnchor::Bottom),
            false,
            FloatBallExpandDirection::Right,
            sizes(0),
        );

        assert_eq!(rect.y, 400 - 28);
        assert_eq!(rect.height, 56);
    }

    #[test]
    fn collapsed_top_anchor_half_hidden() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 300,
            y: 10,
            width: 56,
            height: 56,
        };

        let rect = layout_float_ball_rect(
            bounds,
            ball_rect,
            Some(FloatBallAnchor::Top),
            false,
            FloatBallExpandDirection::Right,
            sizes(0),
        );

        assert_eq!(rect.y, -28);
        assert_eq!(rect.height, 56);
    }

    #[test]
    fn expanded_right_anchor_stays_within_bounds_with_margin() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 572,
            y: 200,
            width: 56,
            height: 56,
        };

        let rect = layout_float_ball_rect(
            bounds,
            ball_rect,
            Some(FloatBallAnchor::Right),
            true,
            FloatBallExpandDirection::Left,
            sizes(8),
        );

        // Right edge at bounds.right - margin = 592
        assert_eq!(rect.x, 600 - 152 - 8);
        assert_eq!(rect.x + rect.width, 592);
    }

    #[test]
    fn expanded_left_anchor_stays_within_bounds_with_margin() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: -28,
            y: 200,
            width: 56,
            height: 56,
        };

        let rect = layout_float_ball_rect(
            bounds,
            ball_rect,
            Some(FloatBallAnchor::Left),
            true,
            FloatBallExpandDirection::Right,
            sizes(8),
        );

        // Left edge at bounds.left + margin = 8
        assert_eq!(rect.x, 8);
    }

    #[test]
    fn expanded_top_anchor_stays_within_bounds_with_margin() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 300,
            y: -28,
            width: 56,
            height: 56,
        };

        let rect = layout_float_ball_rect(
            bounds,
            ball_rect,
            Some(FloatBallAnchor::Top),
            true,
            FloatBallExpandDirection::Right,
            sizes(8),
        );

        // Top edge at bounds.top + margin = 8
        assert_eq!(rect.y, 8);
    }

    #[test]
    fn expanded_free_floating_ball_stays_inside_margin() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 548,
            y: 180,
            width: 56,
            height: 56,
        };

        let rect = layout_float_ball_rect(
            bounds,
            ball_rect,
            None,
            true,
            FloatBallExpandDirection::Left,
            sizes(8),
        );

        assert_eq!(rect.x + rect.width, 592);
        assert!(rect.x >= 8);
    }

    #[test]
    fn collapsed_free_floating_ball_stays_at_current_position() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 260,
            y: 120,
            width: 56,
            height: 56,
        };

        let rect = layout_float_ball_rect(
            bounds,
            ball_rect,
            None,
            false,
            FloatBallExpandDirection::Right,
            sizes(8),
        );

        assert_eq!(rect.x, 260);
        assert_eq!(rect.y, 120);
    }

    #[test]
    fn collapse_after_expanded_drag_keeps_dragged_ball_position() {
        let expanded_rect = FloatBallRect {
            x: 180,
            y: 110,
            width: 152,
            height: 56,
        };
        let state = FloatBallState {
            anchor: None,
            expand_direction: FloatBallExpandDirection::Left,
            expanded: true,
        };

        let ball_rect = ball_rect_from_window(expanded_rect, state, 56);
        let collapsed_rect = layout_float_ball_rect(
            sample_bounds(),
            ball_rect,
            None,
            false,
            FloatBallExpandDirection::Left,
            sizes(8),
        );

        assert_eq!(ball_rect.x, 276);
        assert_eq!(collapsed_rect.x, 276);
        assert_eq!(collapsed_rect.y, 110);
    }

    #[test]
    fn snap_threshold_returns_none_when_far_from_all_edges() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 200,
            y: 150,
            width: 56,
            height: 56,
        };

        assert_eq!(choose_float_ball_anchor(bounds, ball_rect, 20), None);
    }

    #[test]
    fn snap_threshold_returns_anchor_when_close_to_edge() {
        let bounds = sample_bounds();
        // Ball 10px from left edge
        let ball_rect = FloatBallRect {
            x: 10,
            y: 200,
            width: 56,
            height: 56,
        };

        assert_eq!(
            choose_float_ball_anchor(bounds, ball_rect, 20),
            Some(FloatBallAnchor::Left)
        );
    }

    #[test]
    fn snap_threshold_uses_one_and_a_half_ball_radii() {
        let bounds = sample_bounds();
        let threshold = SNAP_THRESHOLD_PX.round() as i32;
        let ball_rect = FloatBallRect {
            x: 40,
            y: 200,
            width: 56,
            height: 56,
        };

        assert_eq!(
            choose_float_ball_anchor(bounds, ball_rect, threshold),
            Some(FloatBallAnchor::Left)
        );
    }

    #[test]
    fn snap_threshold_selects_top_when_closest() {
        let bounds = sample_bounds();
        // Ball 5px from top edge
        let ball_rect = FloatBallRect {
            x: 300,
            y: 5,
            width: 56,
            height: 56,
        };

        assert_eq!(
            choose_float_ball_anchor(bounds, ball_rect, 20),
            Some(FloatBallAnchor::Top)
        );
    }
}
