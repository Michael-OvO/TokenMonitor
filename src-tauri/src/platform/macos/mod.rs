//! macOS-specific platform code.

/// Set Dock icon visibility via activation policy.
pub fn set_dock_icon_visible(app: &tauri::AppHandle, visible: bool) -> Result<(), String> {
    use tauri::ActivationPolicy;
    let policy = if visible {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };
    app.set_activation_policy(policy)
        .map_err(|e| format!("Failed to set activation policy: {e}"))
}

// ── Anchored window resize ──────────────────────────────────────────────────
//
// AppKit's `setContentSize:` (what tao's `set_size` calls) keeps the window's
// BOTTOM-left corner fixed, so a plain `set_size` moves the top edge: a shrink
// drops the tray popover away from the menu bar and a grow lifts it back.
// The helpers below compute a frame that holds the anchored edge still; the
// geometry is in AppKit points with y growing upward.

/// Which horizontal edge of the window stays put while its height changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAnchor {
    Top,
    Bottom,
}

/// Pin whichever edge sits closer to its work-area edge. A tray popover hangs
/// just below the menu bar, so it resolves to `Top`; ties resolve to `Top`.
pub fn resolve_vertical_anchor(
    frame_y: f64,
    frame_h: f64,
    work_y: f64,
    work_h: f64,
) -> VerticalAnchor {
    let top_gap = ((work_y + work_h) - (frame_y + frame_h)).abs();
    let bottom_gap = (frame_y - work_y).abs();
    if bottom_gap < top_gap {
        VerticalAnchor::Bottom
    } else {
        VerticalAnchor::Top
    }
}

/// Frame origin y that keeps `anchor` fixed when the height changes from
/// `frame_h` to `new_h`.
pub fn anchored_origin_y(frame_y: f64, frame_h: f64, new_h: f64, anchor: VerticalAnchor) -> f64 {
    match anchor {
        VerticalAnchor::Top => frame_y + frame_h - new_h,
        VerticalAnchor::Bottom => frame_y,
    }
}

/// Resize the window in one atomic `setFrame:display:` call, holding the
/// anchored edge still. Must run on the main thread (dispatch it through
/// `run_on_main_thread`); off the main thread it logs and does nothing.
pub fn set_size_keeping_anchor(window: &tauri::WebviewWindow, width: f64, height: f64) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSWindow;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    if MainThreadMarker::new().is_none() {
        tracing::warn!("set_size_keeping_anchor must run on the main thread; skipping");
        return;
    }
    let ptr = match window.ns_window() {
        Ok(ptr) => ptr,
        Err(e) => {
            tracing::warn!("set_size_keeping_anchor: no NSWindow handle: {e}");
            return;
        }
    };
    // SAFETY: `ns_window()` hands out the live NSWindow backing this Tauri
    // window, and the main-thread check above satisfies AppKit's threading
    // rule for touching it.
    let ns_window: &NSWindow = unsafe { &*ptr.cast::<NSWindow>() };

    let frame = ns_window.frame();
    let content = ns_window.contentRectForFrameRect(frame);
    let anchor = match ns_window.screen() {
        Some(screen) => {
            let work = screen.visibleFrame();
            resolve_vertical_anchor(
                frame.origin.y,
                frame.size.height,
                work.origin.y,
                work.size.height,
            )
        }
        None => VerticalAnchor::Top,
    };
    let new_content = NSRect::new(
        NSPoint::new(
            content.origin.x,
            anchored_origin_y(content.origin.y, content.size.height, height, anchor),
        ),
        NSSize::new(width, height),
    );
    let new_frame = ns_window.frameRectForContentRect(new_content);
    tracing::debug!(?anchor, from = ?frame, to = ?new_frame, "anchored window resize");
    ns_window.setFrame_display(new_frame, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Screen geometry in AppKit points (y grows upward): a 1400pt-tall
    // visible frame starting at y=0, a 500pt-tall window.
    const WORK_Y: f64 = 0.0;
    const WORK_H: f64 = 1400.0;

    #[test]
    fn window_under_menu_bar_anchors_to_top() {
        // Top edge 25pt below the work-area top, bottom edge 875pt above its bottom.
        let anchor = resolve_vertical_anchor(875.0, 500.0, WORK_Y, WORK_H);
        assert_eq!(anchor, VerticalAnchor::Top);
    }

    #[test]
    fn window_near_bottom_anchors_to_bottom() {
        let anchor = resolve_vertical_anchor(10.0, 500.0, WORK_Y, WORK_H);
        assert_eq!(anchor, VerticalAnchor::Bottom);
    }

    #[test]
    fn equidistant_window_anchors_to_top() {
        // 450pt gap on both sides.
        let anchor = resolve_vertical_anchor(450.0, 500.0, WORK_Y, WORK_H);
        assert_eq!(anchor, VerticalAnchor::Top);
    }

    #[test]
    fn top_anchored_shrink_keeps_top_edge() {
        // Top edge at 875 + 500 = 1375 must survive a shrink to 460.
        let y = anchored_origin_y(875.0, 500.0, 460.0, VerticalAnchor::Top);
        assert_eq!(y + 460.0, 1375.0);
    }

    #[test]
    fn top_anchored_grow_keeps_top_edge() {
        let y = anchored_origin_y(875.0, 500.0, 560.0, VerticalAnchor::Top);
        assert_eq!(y + 560.0, 1375.0);
    }

    #[test]
    fn bottom_anchored_resize_keeps_origin() {
        assert_eq!(anchored_origin_y(10.0, 500.0, 460.0, VerticalAnchor::Bottom), 10.0);
        assert_eq!(anchored_origin_y(10.0, 500.0, 560.0, VerticalAnchor::Bottom), 10.0);
    }
}
