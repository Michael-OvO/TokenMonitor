use super::AppState;
use serde::Serialize;
use tauri::{Manager, WebviewWindow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FloatBallAnchor {
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
    /// Last-set window rect (authoritative). Used instead of querying the WM,
    /// which returns stale/incorrect values on Linux.
    pub(crate) last_rect: Option<FloatBallRect>,
    /// Monotonic drag-move sequence so stale async IPC moves cannot override
    /// the final drag position or a subsequent snap.
    pub(crate) last_move_sequence: u64,
}

impl Default for FloatBallState {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            anchor: None,
            #[cfg(not(target_os = "linux"))]
            // Anchor implicitly depends on screen pos, but left anchor expands right
            anchor: Some(FloatBallAnchor::Left),
            expand_direction: FloatBallExpandDirection::Right,
            expanded: false,
            last_rect: None,
            last_move_sequence: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FloatBallLayout {
    pub expand_direction: FloatBallExpandDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FloatBallRect {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FloatBallEdgeDistances {
    top: i32,
    bottom: i32,
    left: i32,
    right: i32,
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
    #[cfg(target_os = "linux")]
    let (left, top, width, height) = {
        let work_area = monitor.work_area();
        (
            work_area.position.x,
            work_area.position.y,
            work_area.size.width,
            work_area.size.height,
        )
    };
    #[cfg(not(target_os = "linux"))]
    let (left, top, width, height) = {
        let position = monitor.position();
        let size = monitor.size();
        (position.x, position.y, size.width, size.height)
    };

    Ok(FloatBallBounds {
        left,
        top,
        right: left + width as i32,
        bottom: top + height as i32,
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
    // On Linux the window is always expanded-size (even when collapsed),
    // so we always extract ball position from expand_direction.
    let x = match state.expand_direction {
        FloatBallExpandDirection::Right => rect.x,
        FloatBallExpandDirection::Left => rect.x + rect.width - ball_size,
    };

    #[cfg(target_os = "linux")]
    let y = rect.y;
    #[cfg(not(target_os = "linux"))]
    let y = if !state.expanded || rect.width <= ball_size + 1 {
        rect.y
    } else {
        match state.anchor {
            Some(FloatBallAnchor::Top) => rect.y,
            Some(FloatBallAnchor::Left)
            | Some(FloatBallAnchor::Right)
            | Some(FloatBallAnchor::Bottom)
            | None => rect.y + rect.height - ball_size,
        }
    };

    // On non-Linux collapsed, the window IS the ball, so use window position.
    #[cfg(not(target_os = "linux"))]
    if !state.expanded || rect.width <= ball_size + 1 {
        return FloatBallRect {
            x: rect.x,
            y: rect.y,
            width: ball_size,
            height: ball_size,
        };
    }

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

fn clamp_anchored_expand_x(
    inner_bounds: FloatBallBounds,
    target_x: i32,
    width: i32,
    expand_direction: FloatBallExpandDirection,
) -> i32 {
    let max_x = inner_bounds.right - width;
    if max_x <= inner_bounds.left {
        return inner_bounds.left;
    }

    match expand_direction {
        FloatBallExpandDirection::Right => target_x.min(max_x),
        FloatBallExpandDirection::Left => target_x.max(inner_bounds.left),
    }
}

fn clamp_anchored_expand_y(
    inner_bounds: FloatBallBounds,
    target_y: i32,
    height: i32,
    anchor: FloatBallAnchor,
) -> i32 {
    let max_y = inner_bounds.bottom - height;
    if max_y <= inner_bounds.top {
        return inner_bounds.top;
    }

    match anchor {
        FloatBallAnchor::Top => target_y.min(max_y),
        FloatBallAnchor::Bottom => target_y.max(inner_bounds.top),
        FloatBallAnchor::Left | FloatBallAnchor::Right => target_y.clamp(inner_bounds.top, max_y),
    }
}

fn collapsed_rect_from_ball(
    bounds: FloatBallBounds,
    ball_rect: FloatBallRect,
    anchor: Option<FloatBallAnchor>,
    ball_size: i32,
) -> FloatBallRect {
    // On Linux, WMs clamp windows to stay on-screen, so we flush the ball
    // to the edge.  On macOS/Windows the ball can be half off-screen.
    #[cfg(target_os = "linux")]
    let edge_offset = 0;
    #[cfg(not(target_os = "linux"))]
    let edge_offset = ball_size / 2;

    let x = match anchor {
        Some(FloatBallAnchor::Left) => bounds.left - edge_offset,
        Some(FloatBallAnchor::Right) => bounds.right - ball_size + edge_offset,
        Some(FloatBallAnchor::Top) | Some(FloatBallAnchor::Bottom) | None => {
            bounds.clamp_x(ball_rect.x, ball_size)
        }
    };
    let y = match anchor {
        Some(FloatBallAnchor::Top) => bounds.top - edge_offset,
        Some(FloatBallAnchor::Bottom) => bounds.bottom - ball_size + edge_offset,
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
    let anchored_x = match expand_direction {
        FloatBallExpandDirection::Right => ball_rect.x,
        FloatBallExpandDirection::Left => ball_rect.x - (sizes.expanded_width - sizes.ball),
    };

    let x = match anchor {
        Some(FloatBallAnchor::Left) | Some(FloatBallAnchor::Right) => clamp_anchored_expand_x(
            inner_bounds,
            anchored_x,
            sizes.expanded_width,
            expand_direction,
        ),
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
        Some(FloatBallAnchor::Top) => clamp_anchored_expand_y(
            inner_bounds,
            ball_rect.y,
            sizes.expanded_height,
            FloatBallAnchor::Top,
        ),
        Some(FloatBallAnchor::Bottom) => clamp_anchored_expand_y(
            inner_bounds,
            ball_rect.y + sizes.ball - sizes.expanded_height,
            sizes.expanded_height,
            FloatBallAnchor::Bottom,
        ),
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

fn float_ball_edge_distances(
    bounds: FloatBallBounds,
    ball_rect: FloatBallRect,
) -> FloatBallEdgeDistances {
    FloatBallEdgeDistances {
        top: (ball_rect.y - bounds.top).abs(),
        bottom: ((bounds.bottom - ball_rect.height) - ball_rect.y).abs(),
        left: (ball_rect.x - bounds.left).abs(),
        right: ((bounds.right - ball_rect.width) - ball_rect.x).abs(),
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
        Some(FloatBallAnchor::Top) | Some(FloatBallAnchor::Bottom) | None => current_direction,
    };

    // Keep the existing visual side when it still fits. Only flip if the
    // preferred side runs out of room, or if neither side fits and one side is
    // strictly better than the other.
    match preferred {
        FloatBallExpandDirection::Right if room_right >= 0 => FloatBallExpandDirection::Right,
        FloatBallExpandDirection::Left if room_left >= 0 => FloatBallExpandDirection::Left,
        FloatBallExpandDirection::Right if room_left >= 0 => FloatBallExpandDirection::Left,
        FloatBallExpandDirection::Left if room_right >= 0 => FloatBallExpandDirection::Right,
        _ if room_right > room_left => FloatBallExpandDirection::Right,
        _ if room_left > room_right => FloatBallExpandDirection::Left,
        _ => current_direction,
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

fn float_ball_state_for_layout(
    anchor: Option<FloatBallAnchor>,
    expand_direction: FloatBallExpandDirection,
    expanded: bool,
    last_move_sequence: u64,
) -> FloatBallState {
    FloatBallState {
        anchor,
        expand_direction,
        expanded,
        last_rect: None,
        last_move_sequence,
    }
}

/// On Linux the float ball window is always expanded-size (never resized).
/// The ball sits at one end depending on expand direction.
/// Returns the x-offset of the ball within the fixed window.
#[cfg(target_os = "linux")]
fn linux_ball_offset_x(expand_direction: FloatBallExpandDirection, sizes: &FloatBallSizes) -> i32 {
    match expand_direction {
        FloatBallExpandDirection::Right => 0,
        FloatBallExpandDirection::Left => sizes.expanded_width - sizes.ball,
    }
}

/// Compute the fixed-size window rect given the ball's screen position.
#[cfg(target_os = "linux")]
fn linux_window_rect_from_ball(
    ball_x: i32,
    ball_y: i32,
    expand_direction: FloatBallExpandDirection,
    sizes: &FloatBallSizes,
) -> FloatBallRect {
    let offset = linux_ball_offset_x(expand_direction, sizes);
    FloatBallRect {
        x: ball_x - offset,
        y: ball_y,
        width: sizes.expanded_width,
        height: sizes.expanded_height,
    }
}

/// Set the GDK input shape so transparent regions are click-through.
/// Collapsed: only the ball area accepts input.
/// Expanded: the full window accepts input.
#[cfg(target_os = "linux")]
fn update_linux_input_shape(
    window: &WebviewWindow,
    expand_direction: FloatBallExpandDirection,
    is_expanded: bool,
    sizes: &FloatBallSizes,
) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let ball_offset = linux_ball_offset_x(expand_direction, sizes);

    // Input shape coordinates are in logical (GDK) pixels
    let (shape_x, shape_y, shape_w, shape_h) = if is_expanded {
        (
            0,
            0,
            (sizes.expanded_width as f64 / scale).round() as i32,
            (sizes.expanded_height as f64 / scale).round() as i32,
        )
    } else {
        let logical_offset = (ball_offset as f64 / scale).round() as i32;
        let logical_ball = (sizes.ball as f64 / scale).round() as i32;
        (logical_offset, 0, logical_ball, logical_ball)
    };

    let _ = window.with_webview(move |webview| {
        use gtk::prelude::*;

        let inner = webview.inner();
        let widget: &gtk::Widget = inner.as_ref();

        if let Some(toplevel) = widget.toplevel() {
            if let Ok(gtk_win) = toplevel.downcast::<gtk::Window>() {
                if let Some(gdk_window) = gtk_win.window() {
                    let rect = cairo::RectangleInt::new(shape_x, shape_y, shape_w, shape_h);
                    let region = cairo::Region::create_rectangle(&rect);
                    gdk_window.input_shape_combine_region(&region, 0, 0);
                    tracing::debug!(
                        shape_x,
                        shape_y,
                        shape_w,
                        shape_h,
                        is_expanded,
                        "float_ball input shape updated"
                    );
                }
            }
        }
    });
}

/// One-time GTK setup for the fixed-size float ball window on Linux.
/// Sets widget size_request, geometry hints, and disables WebKitGTK scrollbars
/// so the window stays at exactly expanded_width × expanded_height.
#[cfg(target_os = "linux")]
fn setup_linux_fixed_window(window: &WebviewWindow, sizes: &FloatBallSizes) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let logical_w = (sizes.expanded_width as f64 / scale).round() as i32;
    let logical_h = (sizes.expanded_height as f64 / scale).round() as i32;

    let _ = window.with_webview(move |webview| {
        use gtk::prelude::*;

        let inner = webview.inner();
        let widget: &gtk::Widget = inner.as_ref();
        widget.set_size_request(logical_w, logical_h);
        widget.set_hexpand(false);
        widget.set_vexpand(false);

        let mut current = widget.parent();
        while let Some(ancestor) = current {
            ancestor.set_size_request(logical_w, logical_h);

            if let Ok(sw) = ancestor.clone().downcast::<gtk::ScrolledWindow>() {
                sw.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
                sw.set_min_content_width(logical_w);
                sw.set_min_content_height(logical_h);
                sw.set_max_content_width(logical_w);
                sw.set_max_content_height(logical_h);
                sw.set_overlay_scrolling(false);
            }

            current = ancestor.parent();
        }

        if let Some(toplevel) = widget.toplevel() {
            if let Ok(gtk_win) = toplevel.downcast::<gtk::Window>() {
                let gravity = gtk::gdk::Gravity::NorthWest;
                let geometry = gtk::gdk::Geometry::new(
                    logical_w, logical_h, logical_w, logical_h, logical_w, logical_h, 1, 1, 0.0,
                    0.0, gravity,
                );
                let hints = gtk::gdk::WindowHints::MIN_SIZE
                    | gtk::gdk::WindowHints::MAX_SIZE
                    | gtk::gdk::WindowHints::BASE_SIZE;
                gtk_win.set_gravity(gravity);
                gtk_win.set_geometry_hints(None::<&gtk::Widget>, Some(&geometry), hints);
            }
        }

        tracing::debug!(
            logical_w,
            logical_h,
            "float_ball GTK fixed-size setup complete"
        );
    });
}

#[allow(unused_variables)]
fn apply_float_ball_window_rect(
    window: &WebviewWindow,
    rect: FloatBallRect,
    resize: bool,
    expand_direction: FloatBallExpandDirection,
) -> Result<(), String> {
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

    #[cfg(target_os = "linux")]
    {
        // Linux float ball window is fixed-size (never resized). All calls
        // are position-only. The `resize` parameter is ignored.
        window
            .set_position(tauri::PhysicalPosition::new(rect.x, rect.y))
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        let size = tauri::PhysicalSize::new(rect.width as u32, rect.height as u32);
        let _ = window.set_size(size);
        window
            .set_position(tauri::PhysicalPosition::new(rect.x, rect.y))
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn apply_and_store_rect(
    window: &WebviewWindow,
    rect: FloatBallRect,
    state: &mut FloatBallState,
    resize: bool,
    expand_direction: FloatBallExpandDirection,
) -> Result<(), String> {
    apply_float_ball_window_rect(window, rect, resize, expand_direction)?;
    state.last_rect = Some(rect);
    Ok(())
}

// Probe/corrective logic removed: Linux float ball uses a fixed-size window
// with GDK input shape, eliminating the WM geometry races that probes were
// designed to detect and correct.

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
        tracing::debug!("float_ball create skipped: window already exists");
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
    // Linux: create at expanded size (never resized afterwards).
    // Other platforms: create at ball size (resized on expand/collapse).
    .inner_size(
        if cfg!(target_os = "linux") {
            EXPANDED_W
        } else {
            BALL_SIZE
        },
        if cfg!(target_os = "linux") {
            EXPANDED_H
        } else {
            BALL_SIZE
        },
    )
    .transparent(true)
    .shadow(false)
    .decorations(false)
    .always_on_top(true)
    .visible(false)
    .skip_taskbar(true)
    // On Linux, keep resizable so GTK can process programmatic size changes.
    // The window is undecorated, so the user can't drag-resize it anyway.
    // min/max size constraints prevent WM-initiated resizes.
    .resizable(cfg!(target_os = "linux"))
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .build()
    .map_err(|e| e.to_string())?;

    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let sizes = compute_scaled_sizes(scale);
    let bounds = float_ball_bounds(&window)?;

    // Initial ball position: bottom-right of screen.
    #[cfg(target_os = "linux")]
    let ball_x = bounds.right - sizes.ball;
    #[cfg(not(target_os = "linux"))]
    let ball_x = bounds.right - (sizes.ball / 2);
    let ball_y = bounds.bottom - sizes.ball;

    // On Linux: window is always expanded-size. Ball sits at one end.
    // On other platforms: window starts at ball size.
    #[cfg(target_os = "linux")]
    let initial_direction = FloatBallExpandDirection::Left;
    #[cfg(not(target_os = "linux"))]
    let initial_direction = FloatBallExpandDirection::Left;

    #[cfg(target_os = "linux")]
    let rect = linux_window_rect_from_ball(ball_x, ball_y, initial_direction, &sizes);
    #[cfg(not(target_os = "linux"))]
    let rect = FloatBallRect {
        x: ball_x,
        y: ball_y,
        width: sizes.ball,
        height: sizes.ball,
    };

    let initial_state = FloatBallState {
        #[cfg(target_os = "linux")]
        anchor: None,
        #[cfg(not(target_os = "linux"))]
        anchor: Some(FloatBallAnchor::Right),
        expand_direction: initial_direction,
        expanded: false,
        last_rect: Some(rect),
        last_move_sequence: 0,
    };
    tracing::info!(scale, ?bounds, ?sizes, ?rect, "float_ball create");
    apply_float_ball_window_rect(&window, rect, true, initial_direction)?;
    window.show().map_err(|e| e.to_string())?;

    // One-time GTK setup: lock window at expanded size, disable scrollbars.
    // Must run after show() so the GTK widget tree is realized.
    #[cfg(target_os = "linux")]
    setup_linux_fixed_window(&window, &sizes);

    // Set initial GDK input shape (collapsed = ball-only region)
    #[cfg(target_os = "linux")]
    update_linux_input_shape(&window, initial_direction, false, &sizes);

    {
        let state = app.state::<AppState>();
        let mut float_state = state.float_ball_state.write().await;
        *float_state = initial_state;
    }

    // Linux WMs may ignore the initial set_position on first show.
    // Retry position-only (no size correction needed with fixed window).
    #[cfg(target_os = "linux")]
    {
        let w = window.clone();
        let target = rect;
        std::thread::spawn(move || {
            for (attempt, delay_ms) in [100_u64, 200, 350, 500, 800].into_iter().enumerate() {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));

                match current_float_ball_rect(&w) {
                    Ok(actual)
                        if (actual.x - target.x).abs() <= 1 && (actual.y - target.y).abs() <= 1 =>
                    {
                        tracing::debug!(
                            attempt = attempt + 1,
                            delay_ms,
                            ?target,
                            "float_ball create position confirmed"
                        );
                        return;
                    }
                    Ok(actual) => {
                        tracing::debug!(
                            attempt = attempt + 1,
                            delay_ms,
                            ?target,
                            ?actual,
                            "float_ball create retry repositioning"
                        );
                    }
                    Err(error) => {
                        tracing::debug!(
                            attempt = attempt + 1,
                            delay_ms,
                            error = %error,
                            "float_ball create retry read failed"
                        );
                    }
                }

                let _ = w.set_position(tauri::PhysicalPosition::new(target.x, target.y));
            }
        });
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
    interaction_id: Option<String>,
    source: Option<String>,
) -> Result<FloatBallLayout, String> {
    let Some(window) = app.get_webview_window("float-ball") else {
        tracing::debug!(
            expanded,
            interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
            source = source.as_deref().unwrap_or("unknown"),
            "float_ball set_expanded skipped: window missing"
        );
        return Ok(FloatBallLayout {
            expand_direction: FloatBallExpandDirection::Right,
        });
    };

    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let sizes = compute_scaled_sizes(scale);
    let bounds = float_ball_bounds(&window)?;
    let state = app.state::<AppState>();
    let mut float_state = state.float_ball_state.write().await;
    let window_rect = float_state.last_rect.unwrap_or_else(|| {
        current_float_ball_rect(&window).unwrap_or(FloatBallRect {
            x: bounds.right - sizes.ball,
            y: bounds.bottom - sizes.ball,
            width: sizes.expanded_width,
            height: sizes.expanded_height,
        })
    });
    let ball_rect = ball_rect_from_window(window_rect, *float_state, sizes.ball);
    let edge_distances = float_ball_edge_distances(bounds, ball_rect);
    let direction = if expanded {
        choose_expand_direction(
            float_state.anchor,
            bounds,
            ball_rect,
            sizes.expanded_width,
            sizes.expand_margin,
            float_state.expand_direction,
        )
    } else {
        // Collapse should preserve the current visual anchor of the ball.
        // The detail panel disappears; it should not re-pick a new side.
        float_state.expand_direction
    };
    let target_rect = layout_float_ball_rect(
        bounds,
        ball_rect,
        float_state.anchor,
        expanded,
        direction,
        sizes,
    );
    let target_ball_rect = ball_rect_from_window(
        target_rect,
        float_ball_state_for_layout(
            float_state.anchor,
            direction,
            expanded,
            float_state.last_move_sequence,
        ),
        sizes.ball,
    );
    tracing::info!(
        expanded,
        interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
        source = source.as_deref().unwrap_or("unknown"),
        scale,
        ?bounds,
        ?sizes,
        anchor = ?float_state.anchor,
        previous_direction = ?float_state.expand_direction,
        ?window_rect,
        ?ball_rect,
        ?edge_distances,
        next_direction = ?direction,
        ?target_rect,
        ?target_ball_rect,
        ball_shift_x = target_ball_rect.x - ball_rect.x,
        ball_shift_y = target_ball_rect.y - ball_rect.y,
        "float_ball set_expanded"
    );

    // On Linux: no resize. Reposition window if direction changed, then update input shape.
    // On other platforms: resize + reposition as before.
    #[cfg(target_os = "linux")]
    {
        // The window is fixed-size. Compute the correct window position for
        // the ball's current screen position + the (possibly new) direction.
        let linux_rect =
            linux_window_rect_from_ball(target_ball_rect.x, target_ball_rect.y, direction, &sizes);
        apply_and_store_rect(&window, linux_rect, &mut float_state, false, direction)?;
        // Ordering: update input shape BEFORE frontend processes the response
        // (expand shape first on expand, CSS collapses first on collapse from frontend side).
        update_linux_input_shape(&window, direction, expanded, &sizes);
    }
    #[cfg(not(target_os = "linux"))]
    {
        // On Windows, briefly hide the window during collapse resize to prevent
        // WebView2 flicker. When direction="left" the ball is at x=96; SetWindowPos
        // must move+shrink simultaneously, and DWM clips stale content to the new
        // width for 1-2 frames before WebView2 re-renders — showing empty space
        // instead of the ball. Hiding makes the resize invisible.
        #[cfg(target_os = "windows")]
        let collapse_hwnd = if !expanded {
            window.hwnd().ok().map(|raw| {
                let hwnd = windows::Win32::Foundation::HWND(raw.0 as *mut _);
                // SAFETY: hwnd is a valid window handle obtained from Tauri.
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                        hwnd,
                        windows::Win32::UI::WindowsAndMessaging::SW_HIDE,
                    );
                }
                hwnd
            })
        } else {
            None
        };

        apply_and_store_rect(&window, target_rect, &mut float_state, true, direction)?;

        #[cfg(target_os = "windows")]
        if let Some(hwnd) = collapse_hwnd {
            // SAFETY: hwnd is the same valid handle from above.
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
                );
            }
        }
    }

    float_state.expand_direction = direction;
    float_state.expanded = expanded;

    Ok(FloatBallLayout {
        expand_direction: direction,
    })
}

/// Temporarily expand or restore the GDK input shape during drag.
/// During drag the entire window must accept pointer events so that
/// `setPointerCapture` continues to receive `pointermove` events even
/// when the cursor leaves the 56×56 ball region.
#[tauri::command]
pub async fn set_float_ball_dragging(
    #[allow(unused_variables)] app: tauri::AppHandle,
    #[allow(unused_variables)] dragging: bool,
    #[allow(unused_variables)] interaction_id: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let Some(window) = app.get_webview_window("float-ball") else {
            return Ok(());
        };
        let scale = window.scale_factor().map_err(|e| e.to_string())?;
        let sizes = compute_scaled_sizes(scale);
        let state = app.state::<AppState>();
        let float_state = state.float_ball_state.read().await;

        if dragging {
            // Full window accepts input during drag
            update_linux_input_shape(&window, float_state.expand_direction, true, &sizes);
        } else {
            // Restore based on actual expand state
            update_linux_input_shape(
                &window,
                float_state.expand_direction,
                float_state.expanded,
                &sizes,
            );
        }
        tracing::debug!(
            dragging,
            interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
            expanded = float_state.expanded,
            "float_ball dragging input shape"
        );
    }
    Ok(())
}

/// Move the float ball window to the given physical screen coordinates.
/// Used by the frontend's pointer-capture drag implementation.
#[tauri::command]
pub async fn move_float_ball_to(
    app: tauri::AppHandle,
    x: i32,
    y: i32,
    sequence: u64,
    interaction_id: Option<String>,
) -> Result<(), String> {
    let Some(window) = app.get_webview_window("float-ball") else {
        tracing::debug!(
            interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
            sequence,
            x,
            y,
            "float_ball move skipped: window missing"
        );
        return Ok(());
    };

    let state = app.state::<AppState>();
    let mut float_state = state.float_ball_state.write().await;
    if sequence <= float_state.last_move_sequence {
        tracing::debug!(
            interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
            sequence,
            last_move_sequence = float_state.last_move_sequence,
            x,
            y,
            "float_ball move ignored: stale sequence"
        );
        return Ok(());
    }
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let sizes = compute_scaled_sizes(scale);

    let bounds = float_ball_bounds(&window)?;

    // On Linux: window is always expanded-size. Clamp the BALL position,
    // then derive window position from it.
    // On other platforms: window is ball-size when collapsed; clamp window directly.
    #[cfg(target_os = "linux")]
    let (clamp_rule, rect) = {
        // Frontend sends window coordinates. Derive ball screen position.
        let ball_offset = linux_ball_offset_x(float_state.expand_direction, &sizes);
        let ball_x = x + ball_offset;
        let ball_y = y;

        let (rule, clamped_ball_x, clamped_ball_y) = if float_state.expanded {
            let inner_bounds = inset_bounds(bounds, sizes.expand_margin);
            (
                "linux-expanded-inner-margin",
                inner_bounds.clamp_x(ball_x, sizes.expanded_width),
                inner_bounds.clamp_y(ball_y, sizes.expanded_height),
            )
        } else {
            // Clamp ball to stay fully visible on screen
            (
                "linux-ball-full-visible",
                bounds.clamp_x(ball_x, sizes.ball),
                bounds.clamp_y(ball_y, sizes.ball),
            )
        };

        let win_rect = linux_window_rect_from_ball(
            clamped_ball_x,
            clamped_ball_y,
            float_state.expand_direction,
            &sizes,
        );
        (rule, win_rect)
    };
    #[cfg(not(target_os = "linux"))]
    let (clamp_rule, rect) = {
        let (win_w, win_h) = if float_state.expanded {
            (sizes.expanded_width, sizes.expanded_height)
        } else {
            (sizes.ball, sizes.ball)
        };

        let (rule, clamped_x, clamped_y) = if float_state.expanded {
            let inner_bounds = inset_bounds(bounds, sizes.expand_margin);
            (
                "expanded-inner-margin",
                inner_bounds.clamp_x(x, win_w),
                inner_bounds.clamp_y(y, win_h),
            )
        } else {
            let half = sizes.ball / 2;
            (
                "collapsed-half-visible",
                x.clamp(bounds.left - half, bounds.right - win_w + half),
                y.clamp(bounds.top - half, bounds.bottom - win_h + half),
            )
        };

        (
            rule,
            FloatBallRect {
                x: clamped_x,
                y: clamped_y,
                width: win_w,
                height: win_h,
            },
        )
    };
    if rect.x != x || rect.y != y {
        tracing::debug!(
            interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
            sequence,
            clamp_rule,
            requested_x = x,
            requested_y = y,
            actual_x = rect.x,
            actual_y = rect.y,
            expanded = float_state.expanded,
            ?bounds,
            "float_ball move clamped"
        );
    }
    tracing::debug!(
        interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
        sequence,
        clamp_rule,
        requested_x = x,
        requested_y = y,
        actual_x = rect.x,
        actual_y = rect.y,
        scale,
        expanded = float_state.expanded,
        anchor = ?float_state.anchor,
        ?sizes,
        ?rect,
        "float_ball move applied"
    );

    float_state.anchor = None;
    let expand_direction = float_state.expand_direction;
    apply_and_store_rect(&window, rect, &mut float_state, false, expand_direction)?;
    float_state.last_move_sequence = sequence;
    Ok(())
}

/// Snap the float ball to the nearest screen edge (if within threshold).
#[tauri::command]
pub async fn snap_float_ball(
    app: tauri::AppHandle,
    interaction_id: Option<String>,
) -> Result<(), String> {
    let Some(window) = app.get_webview_window("float-ball") else {
        tracing::debug!(
            interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
            "float_ball snap skipped: window missing"
        );
        return Ok(());
    };

    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let sizes = compute_scaled_sizes(scale);
    let snap_threshold_px = (SNAP_THRESHOLD_PX * scale).round() as i32;
    let bounds = float_ball_bounds(&window)?;
    let state = app.state::<AppState>();
    let mut float_state = state.float_ball_state.write().await;
    if float_state.expanded {
        tracing::debug!(
            interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
            "float_ball snap skipped: window is expanded"
        );
        return Ok(());
    }
    let window_rect = float_state.last_rect.unwrap_or_else(|| {
        current_float_ball_rect(&window).unwrap_or(FloatBallRect {
            x: bounds.right - sizes.ball,
            y: bounds.bottom - sizes.ball,
            width: sizes.expanded_width,
            height: sizes.expanded_height,
        })
    });
    let ball_rect = ball_rect_from_window(window_rect, *float_state, sizes.ball);
    let edge_distances = float_ball_edge_distances(bounds, ball_rect);
    tracing::info!(
        interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
        scale,
        snap_threshold_px,
        ?bounds,
        ?sizes,
        anchor = ?float_state.anchor,
        direction = ?float_state.expand_direction,
        ?window_rect,
        ?ball_rect,
        ?edge_distances,
        "float_ball snap requested"
    );

    let Some(anchor) = choose_float_ball_anchor(bounds, ball_rect, snap_threshold_px) else {
        float_state.anchor = None;
        tracing::info!(
            interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
            snap_threshold_px,
            ?bounds,
            ?ball_rect,
            ?edge_distances,
            "float_ball snap skipped: no edge within threshold"
        );
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

    // On Linux: derive fixed-size window rect from snapped ball position.
    // On other platforms: use layout_float_ball_rect which sizes to collapsed.
    let snapped_ball = collapsed_rect_from_ball(bounds, ball_rect, Some(anchor), sizes.ball);
    #[cfg(target_os = "linux")]
    let target_rect =
        linux_window_rect_from_ball(snapped_ball.x, snapped_ball.y, direction, &sizes);
    #[cfg(not(target_os = "linux"))]
    let target_rect = layout_float_ball_rect(
        bounds,
        ball_rect,
        Some(anchor),
        float_state.expanded,
        direction,
        sizes,
    );

    tracing::info!(
        interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
        anchor = ?anchor,
        direction = ?direction,
        ?target_rect,
        ?snapped_ball,
        ball_shift_x = snapped_ball.x - ball_rect.x,
        ball_shift_y = snapped_ball.y - ball_rect.y,
        "float_ball snap resolved"
    );

    #[cfg(target_os = "linux")]
    {
        apply_and_store_rect(&window, target_rect, &mut float_state, false, direction)?;
        update_linux_input_shape(&window, direction, false, &sizes);
    }
    #[cfg(not(target_os = "linux"))]
    {
        apply_and_store_rect(&window, target_rect, &mut float_state, true, direction)?;
    }
    float_state.anchor = Some(anchor);
    float_state.expand_direction = direction;

    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FloatBallPosition {
    pub x: i32,
    pub y: i32,
    pub anchor: Option<FloatBallAnchor>,
    pub expanded: bool,
    /// X-offset of the ball within the window. On Linux (fixed-size window),
    /// this is non-zero when expand_direction is Left. On other platforms, 0.
    pub ball_offset_x: i32,
}

/// Return the authoritative window position from state (not from the WM).
/// Used by the frontend's drag-start to avoid stale `outerPosition()` reads.
#[tauri::command]
pub async fn get_float_ball_position(
    app: tauri::AppHandle,
    interaction_id: Option<String>,
) -> Result<FloatBallPosition, String> {
    let state = app.state::<AppState>();
    let float_state = state.float_ball_state.read().await;
    let rect = float_state.last_rect.ok_or("Float ball not initialized")?;
    tracing::debug!(
        interaction_id = interaction_id.as_deref().unwrap_or("n/a"),
        ?rect,
        "float_ball get_position"
    );
    #[cfg(target_os = "linux")]
    let ball_offset_x = {
        let scale = app
            .get_webview_window("float-ball")
            .and_then(|w| w.scale_factor().ok())
            .unwrap_or(1.0);
        let sizes = compute_scaled_sizes(scale);
        linux_ball_offset_x(float_state.expand_direction, &sizes)
    };
    #[cfg(not(target_os = "linux"))]
    let ball_offset_x = 0;

    Ok(FloatBallPosition {
        x: rect.x,
        y: rect.y,
        anchor: float_state.anchor,
        expanded: float_state.expanded,
        ball_offset_x,
    })
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
    fn choose_expand_direction_preserves_current_side_for_free_floating_ball_when_it_fits() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 420,
            y: 200,
            width: 56,
            height: 56,
        };

        assert_eq!(
            choose_expand_direction(
                None,
                bounds,
                ball_rect,
                152,
                8,
                FloatBallExpandDirection::Right,
            ),
            FloatBallExpandDirection::Right
        );
        assert_eq!(
            choose_expand_direction(
                None,
                bounds,
                ball_rect,
                152,
                8,
                FloatBallExpandDirection::Left,
            ),
            FloatBallExpandDirection::Left
        );
    }

    #[test]
    fn expanded_bottom_anchor_keeps_ball_position() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 420,
            y: 372,
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

        assert_eq!(rect.y, 372);
        assert_eq!(rect.x, 324);
    }

    #[test]
    fn collapsed_right_anchor_snaps_to_edge() {
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

        // On Linux: flush with edge (600-56=544).
        // On other platforms: half-hidden (600-28=572).
        #[cfg(target_os = "linux")]
        assert_eq!(rect.x, 544);
        #[cfg(not(target_os = "linux"))]
        assert_eq!(rect.x, 572);
        assert_eq!(rect.width, 56);
    }

    #[test]
    fn collapsed_bottom_anchor_snaps_to_edge() {
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

        #[cfg(target_os = "linux")]
        assert_eq!(rect.y, 400 - 56);
        #[cfg(not(target_os = "linux"))]
        assert_eq!(rect.y, 400 - 28);
        assert_eq!(rect.height, 56);
    }

    #[test]
    fn collapsed_top_anchor_snaps_to_edge() {
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

        #[cfg(target_os = "linux")]
        assert_eq!(rect.y, 0);
        #[cfg(not(target_os = "linux"))]
        assert_eq!(rect.y, -28);
        assert_eq!(rect.height, 56);
    }

    #[test]
    fn expanded_right_anchor_keeps_ball_at_the_edge() {
        let bounds = sample_bounds();
        #[cfg(target_os = "linux")]
        let ball_x = 544;
        #[cfg(not(target_os = "linux"))]
        let ball_x = 572;
        let ball_rect = FloatBallRect {
            x: ball_x,
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

        // Expanded panel extends left from ball position.
        assert_eq!(rect.x + rect.width - 56, ball_x);
    }

    #[test]
    fn expanded_left_anchor_keeps_ball_at_the_edge() {
        let bounds = sample_bounds();
        #[cfg(target_os = "linux")]
        let ball_x = 0;
        #[cfg(not(target_os = "linux"))]
        let ball_x = -28;
        let ball_rect = FloatBallRect {
            x: ball_x,
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

        // Expanded panel extends right from ball position.
        assert_eq!(rect.x, ball_x);
    }

    #[test]
    fn expanded_top_anchor_keeps_ball_at_the_edge() {
        let bounds = sample_bounds();
        #[cfg(target_os = "linux")]
        let ball_y = 0;
        #[cfg(not(target_os = "linux"))]
        let ball_y = -28;
        let ball_rect = FloatBallRect {
            x: 300,
            y: ball_y,
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

        assert_eq!(rect.y, ball_y);
    }

    #[test]
    fn anchored_expand_clamps_only_when_the_monitor_is_too_narrow() {
        let bounds = FloatBallBounds {
            left: 0,
            top: 0,
            right: 120,
            bottom: 400,
        };
        let ball_rect = FloatBallRect {
            x: 92,
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

        assert_eq!(rect.x, 8);
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
            last_rect: None,
            last_move_sequence: 0,
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
