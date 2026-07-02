//! Converts the overlay Tauri window's NSWindow into an NSPanel subclass:
//! key-capable while borderless, non-activating, visible on every Space and
//! over fullscreen apps. The delegate tao installed survives the class swap,
//! so tauri focus events keep working.

use objc2::{define_class, ClassType, MainThreadOnly};
use objc2_app_kit::{NSPanel, NSWindowCollectionBehavior, NSWindowStyleMask};
use tauri::WebviewWindow;

define_class!(
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumPanel"]
    pub struct PanelClass;

    impl PanelClass {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            false
        }
    }
);

fn with_panel(window: &WebviewWindow, f: impl FnOnce(&NSPanel)) {
    debug_assert!(objc2_foundation::MainThreadMarker::new().is_some());
    if let Ok(ptr) = window.ns_window() {
        // SAFETY: ns_window returns a live NSWindow owned by tao; after
        // convert() its class is our NSPanel subclass (no extra ivars).
        f(unsafe { &*(ptr as *const NSPanel) });
    }
}

pub fn convert(window: &WebviewWindow) {
    let Ok(ptr) = window.ns_window() else {
        return;
    };
    unsafe {
        objc2::ffi::object_setClass(
            ptr.cast(),
            (PanelClass::class() as *const objc2::runtime::AnyClass).cast(),
        );
    }
    with_panel(window, |panel| {
        panel.setStyleMask(panel.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        panel.setCollectionBehavior(
            panel.collectionBehavior()
                | NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setHidesOnDeactivate(false);
    });
}

pub fn show(window: &WebviewWindow) {
    with_panel(window, |panel| {
        if let Some(view) = panel.contentView() {
            panel.makeFirstResponder(Some(&view));
        }
        panel.orderFrontRegardless();
        panel.makeKeyWindow();
    });
}

pub fn hide(window: &WebviewWindow) {
    with_panel(window, |panel| {
        panel.orderOut(None);
    });
}

pub fn is_visible(window: &WebviewWindow) -> bool {
    let mut visible = false;
    with_panel(window, |panel| visible = panel.isVisible());
    visible
}
