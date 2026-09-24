//! Present the tray context menu without leaving it attached to the status item.
//!
//! `tray-icon` 0.24 and older (what Tauri 2.11 ships) attaches the `NSMenu` to
//! the `NSStatusItem` when the tray is built and relies on its own `NSView`
//! subview to swallow left clicks; that is how `show_menu_on_left_click(false)`
//! used to work. macOS 27 stopped forwarding mouse events to that view while
//! the status item has a menu, so every click popped the menu and
//! `TrayIconEvent::Click` never fired (tauri-apps/tauri#16035). tray-icon 0.25
//! fixes it by attaching the menu only for the duration of `performClick`;
//! this module does the same from the app side. Drop it once Tauri depends on
//! tray-icon >= 0.25.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::NSMenu;
use tauri::tray::TrayIcon;
use tauri::Runtime;

thread_local! {
    /// The menu taken off the status item at startup. `NSMenu` is main-thread
    /// only, so it lives in a main-thread slot rather than in `AppState`.
    static DETACHED_MENU: RefCell<Option<Retained<NSMenu>>> = const { RefCell::new(None) };
}

/// Take the context menu off the `NSStatusItem` so left clicks reach the tray
/// view again, and stop tray-icon from trying to pop it on right click (it
/// would find nothing attached). Call once, right after the tray is built.
pub fn detach<R: Runtime>(tray: &TrayIcon<R>) {
    let detached = tray.with_inner_tray_icon(|inner| {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let Some(item) = inner.ns_status_item() else {
            return false;
        };
        let Some(menu) = item.menu(mtm) else {
            return false;
        };
        item.setMenu(None);
        inner.set_show_menu_on_right_click(false);
        DETACHED_MENU.with(|slot| *slot.borrow_mut() = Some(menu));
        true
    });
    match detached {
        Ok(true) => tracing::debug!("Tray menu detached from the status item"),
        Ok(false) => tracing::warn!("Tray menu not detached: no status item or no menu"),
        Err(e) => tracing::warn!("Tray menu not detached: {e}"),
    }
}

/// Pop the detached menu under the status item, as a right click would.
/// `show_menu` runs the menu's tracking loop synchronously, so the menu is
/// detached again as soon as it closes.
pub fn present<R: Runtime>(tray: &TrayIcon<R>) {
    let presented = tray.with_inner_tray_icon(|inner| {
        let Some(item) = inner.ns_status_item() else {
            return false;
        };
        let Some(menu) = DETACHED_MENU.with(|slot| slot.borrow().clone()) else {
            return false;
        };
        item.setMenu(Some(&menu));
        inner.show_menu();
        item.setMenu(None);
        true
    });
    match presented {
        Ok(true) => {}
        Ok(false) => tracing::warn!("Tray menu not presented: no status item or no detached menu"),
        Err(e) => tracing::warn!("Tray menu not presented: {e}"),
    }
}
