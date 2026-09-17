//! Keeps hosted agent pages out of the human window's responder chain. WebKit
//! asks the window to make a page first responder whenever page content
//! requests focus; for registered pages that request is refused, so keyboard
//! input never leaves the chrome. No page class is changed.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use objc2::rc::Retained;
use objc2::runtime::{Bool, Imp, Sel};
use objc2::sel;
use objc2::ClassType;
use objc2_app_kit::{NSResponder, NSView, NSWindow};
use std::sync::{Mutex, OnceLock};

type MakeFirstResponder = unsafe extern "C-unwind" fn(&NSWindow, Sel, Option<&NSResponder>) -> Bool;

static PASSIVE: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static ORIGINAL: OnceLock<Option<Imp>> = OnceLock::new();

/// Registers the page and installs the window hook once. False when the hook
/// could not be installed; the page must then not be presented.
pub(crate) fn register(page: &NSView) -> bool {
    if ORIGINAL.get_or_init(install).is_none() {
        return false;
    }
    let Ok(mut passive) = PASSIVE.lock() else {
        return false;
    };
    let address = address(page);
    if !passive.contains(&address) {
        passive.push(address);
    }
    true
}

pub(crate) fn unregister(page: &NSView) {
    if let Ok(mut passive) = PASSIVE.lock() {
        passive.retain(|entry| *entry != address(page));
    }
}

fn address(view: &NSView) -> usize {
    std::ptr::from_ref(view).addr()
}

fn install() -> Option<Imp> {
    let method = NSWindow::class().instance_method(sel!(makeFirstResponder:))?;
    let replacement: MakeFirstResponder = refuse_passive;
    // SAFETY: the replacement has the exact signature of
    // -[NSWindow makeFirstResponder:]; the IMP type erases only the arguments.
    let imp: Imp = unsafe { std::mem::transmute(replacement) };
    // SAFETY: AppKit keeps calling this selector with the same signature; the
    // original implementation is retained and forwarded for every other responder.
    Some(unsafe { method.set_implementation(imp) })
}

unsafe extern "C-unwind" fn refuse_passive(
    window: &NSWindow,
    command: Sel,
    responder: Option<&NSResponder>,
) -> Bool {
    if responder.is_some_and(is_passive) {
        return Bool::NO;
    }
    let Some(original) = ORIGINAL.get().copied().flatten() else {
        return Bool::NO;
    };
    // SAFETY: `install` stored the original -[NSWindow makeFirstResponder:] IMP.
    let original: MakeFirstResponder = unsafe { std::mem::transmute(original) };
    // SAFETY: forwarded with the exact arguments AppKit passed in.
    unsafe { original(window, command, responder) }
}

/// The responder is a registered page or one of its native descendants.
fn is_passive(responder: &NSResponder) -> bool {
    let Ok(passive) = PASSIVE.lock() else {
        return false;
    };
    if passive.is_empty() {
        return false;
    }
    let Some(view) = responder.downcast_ref::<NSView>() else {
        return false;
    };
    if passive.contains(&address(view)) {
        return true;
    }
    // SAFETY: the responder is a live view on the main thread; superviews are
    // read only for identity comparison.
    let mut parent: Option<Retained<NSView>> = unsafe { view.superview() };
    while let Some(view) = parent {
        if passive.contains(&address(&view)) {
            return true;
        }
        // SAFETY: as above.
        parent = unsafe { view.superview() };
    }
    false
}
