//! On-demand AppKit routing for native WebExtension keyboard commands.
//!
//! The monitor exists only while at least one authenticated native extension
//! context is loaded. Browser-owned menu accelerators run first. Remaining
//! key-down events may be consumed only by the host's exact focused-tab and
//! collision checks; an unavailable or reentrant host always receives the
//! original event unchanged.

use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSApplication, NSEvent, NSEventMask};
use objc2_foundation::MainThreadMarker;

pub(super) struct ExtensionCommandMonitor {
    token: Option<Retained<AnyObject>>,
}

impl ExtensionCommandMonitor {
    pub(super) fn install() -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let handler: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent> =
            RcBlock::new(move |event: NonNull<NSEvent>| {
                // SAFETY: AppKit supplies a live event for this synchronous
                // local-monitor callback. Returning the same pointer passes it
                // through; null consumes it.
                let event = unsafe { event.as_ref() };
                let browser_menu_handled = objc2::exception::catch(AssertUnwindSafe(|| {
                    NSApplication::sharedApplication(mtm)
                        .mainMenu()
                        .is_some_and(|menu| menu.performKeyEquivalent(event))
                }))
                .unwrap_or(false);
                if browser_menu_handled || crate::host::try_dispatch_macos_extension_command(event)
                {
                    std::ptr::null_mut()
                } else {
                    event as *const NSEvent as *mut NSEvent
                }
            });
        // SAFETY: AppKit copies the block and returns an opaque retained
        // monitor token. The token is removed exactly once by Drop.
        let token = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler)?
        };
        Some(Self { token: Some(token) })
    }

    pub(super) fn remove(mut self) -> bool {
        let Some(token) = self.token.take() else {
            return true;
        };
        objc2::exception::catch(AssertUnwindSafe(|| unsafe {
            NSEvent::removeMonitor(&token);
        }))
        .is_ok()
    }
}

impl Drop for ExtensionCommandMonitor {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            let _ = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
                NSEvent::removeMonitor(&token);
            }));
        }
    }
}
