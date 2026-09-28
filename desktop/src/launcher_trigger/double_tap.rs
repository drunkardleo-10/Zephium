//! Double-tapping ⌘ or ⌥ opens the launcher. It collides with no key
//! combination, which is why it is worth an Accessibility permission to
//! those who opt in; without that permission macOS does not deliver another
//! application's modifier changes, and nothing here is installed.
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSWorkspace};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
use tauri::AppHandle;

use super::DoubleTap;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
}

/// A tap is a press released within this; a double tap is a second press
/// within `GAP` of the first release. Close to the system's own double-click
/// feel without catching a modifier held and let go while thinking.
const TAP: f64 = 0.25;
const GAP: f64 = 0.30;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Idle,
    Held(f64),
    Released(f64),
}

/// Recognises a modifier pressed and released on its own, twice in quick
/// succession. Anything else in between — another modifier, any key, too
/// slow a press — starts over, so ⌘C followed by ⌘V never counts.
#[derive(Debug)]
pub(super) struct Detector {
    phase: Phase,
}

impl Detector {
    pub(super) const fn new() -> Self {
        Self { phase: Phase::Idle }
    }

    /// `alone` is the target modifier down with nothing else; `none` is every
    /// modifier up. Returns true on the press that completes a double tap.
    pub(super) fn modifiers(&mut self, alone: bool, none: bool, at: f64) -> bool {
        match (self.phase, alone, none) {
            (Phase::Released(up), true, _) if at - up <= GAP => {
                self.phase = Phase::Idle;
                return true;
            }
            (_, true, _) => self.phase = Phase::Held(at),
            (Phase::Held(down), _, true) if at - down <= TAP => self.phase = Phase::Released(at),
            _ => self.phase = Phase::Idle,
        }
        false
    }

    pub(super) fn key(&mut self) {
        self.phase = Phase::Idle;
    }
}

struct Installed {
    mode: DoubleTap,
    global: Retained<AnyObject>,
    local: Retained<AnyObject>,
}

thread_local! {
    static INSTALLED: RefCell<Option<Installed>> = const { RefCell::new(None) };
    static DETECTOR: RefCell<Detector> = const { RefCell::new(Detector::new()) };
    static TARGET: Cell<DoubleTap> = const { Cell::new(DoubleTap::Off) };
}

pub fn trusted() -> bool {
    // SAFETY: a side-effect-free query of this process's trust.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Shows the system's own prompt, which also lists Zephium in the
/// Accessibility pane so the person only has to switch it on.
pub fn request() {
    let key = NSString::from_str("AXTrustedCheckOptionPrompt");
    let yes = NSNumber::new_bool(true);
    let options = NSDictionary::from_slices(&[&*key], &[&*yes]);
    // SAFETY: NSDictionary is toll-free bridged to CFDictionary, and the
    // dictionary outlives the call.
    unsafe {
        AXIsProcessTrustedWithOptions(Retained::as_ptr(&options).cast());
    }
}

pub fn open_settings() {
    let url = NSURL::URLWithString(&NSString::from_str(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    ));
    if let Some(url) = url {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

fn remove() {
    if let Some(installed) = INSTALLED.with(|installed| installed.borrow_mut().take()) {
        // SAFETY: both tokens came from NSEvent's own monitor registration.
        unsafe {
            NSEvent::removeMonitor(&installed.global);
            NSEvent::removeMonitor(&installed.local);
        }
    }
}

/// Brings the observers in line with the chosen mode and current trust.
/// Idempotent, so it can run whenever either may have changed.
pub fn apply(app: &AppHandle, mode: DoubleTap) {
    if MainThreadMarker::new().is_none() {
        let app_for_main = app.clone();
        let _ = app.run_on_main_thread(move || apply(&app_for_main, mode));
        return;
    }
    let current = INSTALLED.with(|installed| installed.borrow().as_ref().map(|i| i.mode));
    if mode == DoubleTap::Off || !trusted() {
        remove();
        return;
    }
    if current == Some(mode) {
        return;
    }
    remove();
    TARGET.with(|target| target.set(mode));
    DETECTOR.with(|detector| detector.borrow_mut().key());

    let observe = {
        let app = app.clone();
        move |event: &NSEvent| {
            if event.r#type() == objc2_app_kit::NSEventType::KeyDown {
                DETECTOR.with(|detector| detector.borrow_mut().key());
                return;
            }
            let relevant = event.modifierFlags()
                & (NSEventModifierFlags::Command
                    | NSEventModifierFlags::Option
                    | NSEventModifierFlags::Control
                    | NSEventModifierFlags::Shift);
            let target = match TARGET.with(Cell::get) {
                DoubleTap::Command => NSEventModifierFlags::Command,
                DoubleTap::Option => NSEventModifierFlags::Option,
                DoubleTap::Off => return,
            };
            let fired = DETECTOR.with(|detector| {
                detector.borrow_mut().modifiers(
                    relevant == target,
                    relevant.is_empty(),
                    event.timestamp(),
                )
            });
            if fired {
                let _ = crate::execute_command(&app, "launcher.toggle");
            }
        }
    };
    let mask = NSEventMask::FlagsChanged | NSEventMask::KeyDown;
    let elsewhere = {
        let observe = observe.clone();
        RcBlock::new(move |event: NonNull<NSEvent>| {
            // SAFETY: AppKit passes a live event for the duration of the block.
            observe(unsafe { event.as_ref() });
        })
    };
    let here = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: as above; the event is handed back untouched.
        observe(unsafe { event.as_ref() });
        event.as_ptr()
    });
    let global = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &elsewhere);
    // SAFETY: the block returns the event it was given, as AppKit requires.
    let local = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &here) };
    if let (Some(global), Some(local)) = (global, local) {
        INSTALLED.with(|installed| {
            *installed.borrow_mut() = Some(Installed {
                mode,
                global,
                local,
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_quick_taps_open_the_launcher() {
        let mut d = Detector::new();
        assert!(!d.modifiers(true, false, 0.00));
        assert!(!d.modifiers(false, true, 0.10));
        assert!(d.modifiers(true, false, 0.25));
    }

    #[test]
    fn a_chord_or_a_slow_press_does_not() {
        let mut d = Detector::new();
        // ⌘C then ⌘V.
        d.modifiers(true, false, 0.0);
        d.key();
        d.modifiers(false, true, 0.1);
        assert!(!d.modifiers(true, false, 0.2));

        let mut d = Detector::new();
        d.modifiers(true, false, 0.0);
        d.modifiers(false, true, 0.6);
        assert!(!d.modifiers(true, false, 0.7));

        let mut d = Detector::new();
        d.modifiers(true, false, 0.0);
        d.modifiers(false, true, 0.1);
        assert!(!d.modifiers(true, false, 0.6));

        // ⌘ then ⌘⇧ is a chord in progress, not a tap.
        let mut d = Detector::new();
        d.modifiers(true, false, 0.0);
        d.modifiers(false, false, 0.05);
        d.modifiers(false, true, 0.1);
        assert!(!d.modifiers(true, false, 0.2));
    }
}
