use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
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
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FloatBallBounds {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FloatBallSizes {
    pub(crate) ball: i32,
    pub(crate) expanded_width: i32,
    pub(crate) expanded_height: i32,
    pub(crate) expand_margin: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FloatBallEdgeDistances {
    pub(crate) top: i32,
    pub(crate) bottom: i32,
    pub(crate) left: i32,
    pub(crate) right: i32,
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

// ── Constants ──────────────────────────────────────────────────────────

pub(crate) const BALL_SIZE: f64 = 56.0;
pub(crate) const EXPANDED_W: f64 = 152.0; // ball (56) + panel (56 × 1.7 ≈ 96)
pub(crate) const EXPANDED_H: f64 = 56.0; // same height as collapsed ball
#[cfg(test)]
pub(crate) const SNAP_THRESHOLD_PX: f64 = (BALL_SIZE / 2.0) * 1.5;
pub(crate) const EXPAND_MARGIN: f64 = 8.0; // minimum gap from screen edges when expanded

pub(crate) fn compute_scaled_sizes(scale: f64) -> FloatBallSizes {
    FloatBallSizes {
        ball: (BALL_SIZE * scale).round() as i32,
        expanded_width: (EXPANDED_W * scale).round() as i32,
        expanded_height: (EXPANDED_H * scale).round() as i32,
        expand_margin: (EXPAND_MARGIN * scale).round() as i32,
    }
}

// ── Pure geometry functions ────────────────────────────────────────────

impl FloatBallBounds {
    pub(crate) fn clamp_x(self, x: i32, width: i32) -> i32 {
        let max = self.right - width;
        if max <= self.left {
            self.left
        } else {
            x.clamp(self.left, max)
        }
    }

    pub(crate) fn clamp_y(self, y: i32, height: i32) -> i32 {
        let max = self.bottom - height;
        if max <= self.top {
            self.top
        } else {
            y.clamp(self.top, max)
        }
    }
}

pub(crate) fn ball_rect_from_window(
    rect: FloatBallRect,
    state: FloatBallState,
    ball_size: i32,
) -> FloatBallRect {
    let x = match state.expand_direction {
        FloatBallExpandDirection::Right => rect.x,
        FloatBallExpandDirection::Left => rect.x + rect.width - ball_size,
    };

    FloatBallRect {
        x,
        y: rect.y,
        width: ball_size,
        height: ball_size,
    }
}

pub(crate) fn inset_bounds(bounds: FloatBallBounds, margin: i32) -> FloatBallBounds {
    FloatBallBounds {
        left: bounds.left + margin,
        top: bounds.top + margin,
        right: bounds.right - margin,
        bottom: bounds.bottom - margin,
    }
}

pub(crate) fn clamp_anchored_expand_x(
    inner_bounds: FloatBallBounds,
    target_x: i32,
    width: i32,
    _expand_direction: FloatBallExpandDirection,
) -> i32 {
    let max_x = inner_bounds.right - width;
    if max_x <= inner_bounds.left {
        return inner_bounds.left;
    }

    target_x.clamp(inner_bounds.left, max_x)
}

pub(crate) fn clamp_anchored_expand_y(
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

pub(crate) fn collapsed_rect_from_ball(
    bounds: FloatBallBounds,
    ball_rect: FloatBallRect,
    anchor: Option<FloatBallAnchor>,
    ball_size: i32,
) -> FloatBallRect {
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

pub(crate) fn expanded_rect_from_ball(
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

#[cfg(test)]
pub(crate) fn choose_float_ball_anchor(
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

pub(crate) fn choose_horizontal_snap_anchor(
    bounds: FloatBallBounds,
    ball_rect: FloatBallRect,
) -> FloatBallAnchor {
    let dist_left = (ball_rect.x - bounds.left).abs();
    let dist_right = ((bounds.right - ball_rect.width) - ball_rect.x).abs();
    if dist_left <= dist_right {
        FloatBallAnchor::Left
    } else {
        FloatBallAnchor::Right
    }
}

pub(crate) fn float_ball_edge_distances(
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

pub(crate) fn choose_expand_direction(
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

pub(crate) fn layout_float_ball_rect(
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

pub(crate) fn float_ball_state_for_layout(
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

// ── Fixed-window pure geometry helpers ────────────────────────────────

/// The float ball window is always expanded-size (never resized).
/// The ball sits at one end depending on expand direction.
/// Returns the x-offset of the ball within the fixed window.
pub(crate) fn ball_offset_x(
    expand_direction: FloatBallExpandDirection,
    sizes: &FloatBallSizes,
) -> i32 {
    match expand_direction {
        FloatBallExpandDirection::Right => 0,
        FloatBallExpandDirection::Left => sizes.expanded_width - sizes.ball,
    }
}

/// Compute the fixed-size window rect given the ball's screen position.
pub(crate) fn window_rect_from_ball(
    ball_x: i32,
    ball_y: i32,
    expand_direction: FloatBallExpandDirection,
    sizes: &FloatBallSizes,
) -> FloatBallRect {
    let offset = ball_offset_x(expand_direction, sizes);
    FloatBallRect {
        x: ball_x - offset,
        y: ball_y,
        width: sizes.expanded_width,
        height: sizes.expanded_height,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

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
        assert_eq!(rect.y, 400 - 56 + 28);
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
    fn expanded_right_anchor_stays_on_screen() {
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

        assert!(rect.x >= 8, "left edge within margin");
        assert!(rect.x + rect.width <= 592, "right edge within margin");
        assert_eq!(rect.x + rect.width, 592);
    }

    #[test]
    fn expanded_left_anchor_stays_on_screen() {
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

        assert!(rect.x >= 8, "left edge within margin");
        assert!(rect.x + rect.width <= 592, "right edge within margin");
        assert_eq!(rect.x, 8);
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

    #[test]
    fn horizontal_snap_picks_left_when_closer() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 100,
            y: 200,
            width: 56,
            height: 56,
        };
        assert_eq!(
            choose_horizontal_snap_anchor(bounds, ball_rect),
            FloatBallAnchor::Left,
        );
    }

    #[test]
    fn horizontal_snap_picks_right_when_closer() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 400,
            y: 200,
            width: 56,
            height: 56,
        };
        assert_eq!(
            choose_horizontal_snap_anchor(bounds, ball_rect),
            FloatBallAnchor::Right,
        );
    }

    #[test]
    fn horizontal_snap_picks_left_when_equidistant() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 272,
            y: 200,
            width: 56,
            height: 56,
        };
        assert_eq!(
            choose_horizontal_snap_anchor(bounds, ball_rect),
            FloatBallAnchor::Left,
        );
    }

    #[test]
    fn collapsed_left_anchor_half_hidden() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 30,
            y: 200,
            width: 56,
            height: 56,
        };
        let rect = collapsed_rect_from_ball(bounds, ball_rect, Some(FloatBallAnchor::Left), 56);
        #[cfg(target_os = "linux")]
        assert_eq!(rect.x, 0);
        #[cfg(not(target_os = "linux"))]
        assert_eq!(rect.x, -28);
        assert_eq!(rect.y, 200);
    }

    #[test]
    fn collapsed_right_anchor_half_hidden() {
        let bounds = sample_bounds();
        let ball_rect = FloatBallRect {
            x: 500,
            y: 150,
            width: 56,
            height: 56,
        };
        let rect = collapsed_rect_from_ball(bounds, ball_rect, Some(FloatBallAnchor::Right), 56);
        #[cfg(target_os = "linux")]
        assert_eq!(rect.x, 544);
        #[cfg(not(target_os = "linux"))]
        assert_eq!(rect.x, 572);
        assert_eq!(rect.y, 150);
    }

    #[test]
    fn expand_from_half_hidden_left_stays_on_screen() {
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

        let rect = expanded_rect_from_ball(
            bounds,
            ball_rect,
            Some(FloatBallAnchor::Left),
            FloatBallExpandDirection::Right,
            sizes(8),
        );

        assert!(
            rect.x >= 8,
            "expanded rect x={} must be >= inner_bounds.left=8",
            rect.x
        );
        assert!(
            rect.x + rect.width <= 592,
            "expanded rect right={} must be <= inner_bounds.right=592",
            rect.x + rect.width
        );
    }
}
