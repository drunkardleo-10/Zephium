//! Configures the Tao-owned overlay window without changing its Objective-C
//! class. Tao allocates a private `NSWindow` subclass with its own ivars and
//! method overrides; replacing that live object's class with an unrelated
//! `NSPanel` subclass violates Objective-C's layout and dispatch invariants.

use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
use objc2_foundation::MainThreadMarker;
use tauri::WebviewWindow;

fn with_window(window: &WebviewWindow, f: impl FnOnce(&NSWindow)) {
    if MainThreadMarker::new().is_none() {
        return;
    }
    if let Ok(ptr) = window.ns_window() {
        // SAFETY: `ns_window` returns Tao's live `NSWindow` subclass. We only
        // borrow it for this main-thread call and never change its class.
        f(unsafe { &*(ptr as *const NSWindow) });
    }
}

pub fn configure(window: &WebviewWindow) {
    with_window(window, |window| {
        window.setCollectionBehavior(
            window.collectionBehavior()
                | NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        window.setHidesOnDeactivate(false);
    });
}

pub fn show(window: &WebviewWindow) {
    with_window(window, |window| {
        if let Some(view) = window.contentView() {
            window.makeFirstResponder(Some(&view));
        }
        window.orderFrontRegardless();
        window.makeKeyWindow();
    });
}

pub fn hide(window: &WebviewWindow) {
    with_window(window, |window| {
        window.orderOut(None);
    });
}

pub fn is_visible(window: &WebviewWindow) -> bool {
    let mut visible = false;
    with_window(window, |window| visible = window.isVisible());
    visible
}
