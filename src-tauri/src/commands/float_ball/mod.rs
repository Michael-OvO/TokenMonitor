mod layout;

pub(crate) use layout::FloatBallState;
#[allow(unused_imports)]
pub use layout::{FloatBallAnchor, FloatBallExpandDirection, FloatBallLayout, FloatBallPosition};

use layout::{
    ball_rect_from_window, choose_expand_direction, choose_float_ball_anchor,
    collapsed_rect_from_ball, compute_scaled_sizes, float_ball_edge_distances,
    float_ball_state_for_layout, inset_bounds, layout_float_ball_rect, FloatBallBounds,
    FloatBallRect, BALL_SIZE, EXPANDED_H, EXPANDED_W, SNAP_THRESHOLD_PX,
};

#[cfg(target_os = "linux")]
use layout::FloatBallSizes;

use super::AppState;
use tauri::{Manager, WebviewWindow};

#[cfg(target_os = "linux")]
use layout::{linux_ball_offset_x, linux_window_rect_from_ball};

// ── Window helpers (platform-specific) ─────────────────────────────────

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

// ── Float Ball commands ─────────────────────────────────────────────

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
    .resizable(cfg!(target_os = "linux"))
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .build()
    .map_err(|e| e.to_string())?;

    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let sizes = compute_scaled_sizes(scale);
    let bounds = float_ball_bounds(&window)?;

    #[cfg(target_os = "linux")]
    let ball_x = bounds.right - sizes.ball;
    #[cfg(not(target_os = "linux"))]
    let ball_x = bounds.right - (sizes.ball / 2);
    let ball_y = bounds.bottom - sizes.ball;

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

    #[cfg(target_os = "linux")]
    setup_linux_fixed_window(&window, &sizes);

    #[cfg(target_os = "linux")]
    update_linux_input_shape(&window, initial_direction, false, &sizes);

    {
        let state = app.state::<AppState>();
        let mut float_state = state.float_ball_state.write().await;
        *float_state = initial_state;
    }

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

    #[cfg(target_os = "linux")]
    {
        let linux_rect =
            linux_window_rect_from_ball(target_ball_rect.x, target_ball_rect.y, direction, &sizes);
        apply_and_store_rect(&window, linux_rect, &mut float_state, false, direction)?;
        update_linux_input_shape(&window, direction, expanded, &sizes);
    }
    #[cfg(not(target_os = "linux"))]
    {
        #[cfg(target_os = "windows")]
        let collapse_hwnd = if !expanded {
            window.hwnd().ok().map(|raw| {
                let hwnd = windows::Win32::Foundation::HWND(raw.0 as *mut _);
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
            update_linux_input_shape(&window, float_state.expand_direction, true, &sizes);
        } else {
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

    #[cfg(target_os = "linux")]
    let (clamp_rule, rect) = {
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

/// Return the authoritative window position from state (not from the WM).
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
