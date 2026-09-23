//! Maps raw tray mouse events onto the two gestures the app reacts to.
//!
//! Kept free of platform code so the mapping is unit-testable; `lib.rs`
//! decides what each gesture does per OS.

use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayGesture {
    /// Primary button released over the icon: show or hide the popover.
    TogglePopover,
    /// Secondary button pressed over the icon: present the context menu.
    PresentMenu,
}

/// The gesture a tray event stands for, if any. Button-down for the primary
/// button and button-up for the secondary one are ignored so each click maps
/// to exactly one gesture.
pub fn gesture_for(event: &TrayIconEvent) -> Option<TrayGesture> {
    match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } => Some(TrayGesture::TogglePopover),
        TrayIconEvent::Click {
            button: MouseButton::Right,
            button_state: MouseButtonState::Down,
            ..
        } => Some(TrayGesture::PresentMenu),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::tray::TrayIconId;

    fn click(button: MouseButton, button_state: MouseButtonState) -> TrayIconEvent {
        TrayIconEvent::Click {
            id: TrayIconId::new("main-tray"),
            position: tauri::PhysicalPosition::new(0.0, 0.0),
            rect: tauri::Rect {
                position: tauri::Position::Physical(tauri::PhysicalPosition::new(0, 0)),
                size: tauri::Size::Physical(tauri::PhysicalSize::new(0, 0)),
            },
            button,
            button_state,
        }
    }

    #[test]
    fn left_release_toggles_popover() {
        assert_eq!(
            gesture_for(&click(MouseButton::Left, MouseButtonState::Up)),
            Some(TrayGesture::TogglePopover)
        );
    }

    #[test]
    fn left_press_is_ignored() {
        assert_eq!(
            gesture_for(&click(MouseButton::Left, MouseButtonState::Down)),
            None
        );
    }

    #[test]
    fn right_press_presents_menu() {
        assert_eq!(
            gesture_for(&click(MouseButton::Right, MouseButtonState::Down)),
            Some(TrayGesture::PresentMenu)
        );
    }

    #[test]
    fn right_release_and_middle_clicks_are_ignored() {
        assert_eq!(
            gesture_for(&click(MouseButton::Right, MouseButtonState::Up)),
            None
        );
        assert_eq!(
            gesture_for(&click(MouseButton::Middle, MouseButtonState::Up)),
            None
        );
        assert_eq!(
            gesture_for(&click(MouseButton::Middle, MouseButtonState::Down)),
            None
        );
    }

    #[test]
    fn non_click_events_are_ignored() {
        let rect = tauri::Rect {
            position: tauri::Position::Physical(tauri::PhysicalPosition::new(0, 0)),
            size: tauri::Size::Physical(tauri::PhysicalSize::new(0, 0)),
        };
        let enter = TrayIconEvent::Enter {
            id: TrayIconId::new("main-tray"),
            position: tauri::PhysicalPosition::new(0.0, 0.0),
            rect,
        };
        assert_eq!(gesture_for(&enter), None);
    }
}
