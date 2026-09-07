//! Shipping-private presentation mechanics for one retained-page observation.
//! No diagnostic capability, application activation, input or page script.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use objc2::rc::{Retained, Weak};
use objc2::MainThreadOnly as _;
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSResponder, NSView, NSWindow, NSWindowOcclusionState,
    NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSAlignmentOptions, NSPoint, NSRect, NSSize};
use objc2_web_kit::WKWebView;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PresentationState {
    Prepared,
    Acquiring,
    Ready,
    Retiring,
    Retired,
    Unavailable,
    Expired,
    Failed,
}

/// The resource retains this owner before the first hierarchy mutation. The
/// host separately binds its exact lease, document and observation correlation.
pub(crate) struct WorkObservationPresentation {
    app: Retained<NSApplication>,
    main: Retained<NSWindow>,
    responder: Retained<NSResponder>,
    page: Retained<WKWebView>,
    parent: Retained<NSView>,
    original_frame: NSRect,
    surface: Option<Retained<NSWindow>>,
    retired_surface: Option<Weak<NSWindow>>,
    frame: NSRect,
    screen: NSRect,
    deadline: Instant,
    state: PresentationState,
    cleanup_failed: bool,
}

impl WorkObservationPresentation {
    #[cfg(feature = "native-agentic-work-resource-probe")]
    pub(crate) fn record_probe_weak(&self, resource: &zephium_agentic::WorkBrowserResourceJoin) {
        if let Some(surface) = &self.surface {
            super::agentic_foreground_probe::record_resource_observation_weak(
                resource, &self.page, surface,
            );
        }
    }
    pub(crate) fn prepare(
        view: &wry::WebView,
        deadline: Instant,
    ) -> Result<Self, PresentationState> {
        let mtm = MainThreadMarker::new().ok_or(PresentationState::Failed)?;
        let app = NSApplication::sharedApplication(mtm);
        let page = super::native_webview(view);
        let main = page.window().ok_or(PresentationState::Unavailable)?;
        let responder = main
            .firstResponder()
            .ok_or(PresentationState::Unavailable)?;
        if !foreground(&app, &main, &responder) {
            return Err(PresentationState::Unavailable);
        }
        // SAFETY: the exact native page is read on its owning main thread;
        // retain its original parent before any hierarchy effect.
        let parent = unsafe { page.superview() }.ok_or(PresentationState::Failed)?;
        let original_frame = page.frame();
        if !page.isHidden()
            || original_frame.size != viewport().size
            || Retained::as_ptr(&responder).addr() == Retained::as_ptr(&page).addr()
        {
            return Err(PresentationState::Failed);
        }
        let native_screen = main.screen().ok_or(PresentationState::Unavailable)?;
        let screen = native_screen.visibleFrame();
        let frame = surface_frame(screen).ok_or(PresentationState::Unavailable)?;
        let scale = native_screen.backingScaleFactor();
        if !scale.is_finite()
            || scale <= 0.0
            || !(frame.size.width * scale).is_finite()
            || !(frame.size.height * scale).is_finite()
            || native_screen
                .backingAlignedRect_options(frame, NSAlignmentOptions::AlignAllEdgesNearest)
                != frame
        {
            return Err(PresentationState::Unavailable);
        }
        if Instant::now() >= deadline {
            return Err(PresentationState::Expired);
        }
        // SAFETY: all geometry is finite, main-thread-owned, and the retained
        // owner below controls the borderless public AppKit window's lifetime.
        let surface = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: Rust retains the surface; close must not consume that retain.
        unsafe { surface.setReleasedWhenClosed(false) };
        surface.setIgnoresMouseEvents(true);
        surface.setOpaque(true);
        Ok(Self {
            app,
            main,
            responder,
            page,
            parent,
            original_frame,
            surface: Some(surface),
            retired_surface: None,
            frame,
            screen,
            deadline,
            state: PresentationState::Prepared,
            cleanup_failed: false,
        })
    }

    pub(crate) fn present(&mut self) -> PresentationState {
        if self.state != PresentationState::Prepared {
            self.state = PresentationState::Failed;
            return self.state;
        }
        if !foreground(&self.app, &self.main, &self.responder) {
            self.state = PresentationState::Unavailable;
            return self.state;
        }
        if Instant::now() >= self.deadline {
            self.state = PresentationState::Expired;
            return self.state;
        }
        let Some(surface) = &self.surface else {
            self.state = PresentationState::Failed;
            return self.state;
        };
        if surface.isVisible()
            || surface.frame() != self.frame
            || surface.canBecomeKeyWindow()
            || surface.canBecomeMainWindow()
            || surface.alphaValue() != 1.0
        {
            self.state = PresentationState::Failed;
            return self.state;
        }
        let Some(parent) = surface.contentView() else {
            self.state = PresentationState::Failed;
            return self.state;
        };
        self.state = PresentationState::Acquiring;
        parent.addSubview(&self.page);
        self.page.setFrame(viewport());
        self.page.setHidden(false);
        surface.orderFrontRegardless();
        self.poll()
    }

    pub(crate) fn poll(&mut self) -> PresentationState {
        if self.state == PresentationState::Retiring {
            if self
                .retired_surface
                .as_ref()
                .is_some_and(|surface| surface.load().is_none())
            {
                self.retired_surface = None;
                self.state = if self.cleanup_failed {
                    PresentationState::Failed
                } else {
                    PresentationState::Retired
                };
            }
            return self.state;
        }
        if !matches!(
            self.state,
            PresentationState::Acquiring | PresentationState::Ready
        ) {
            return self.state;
        }
        if !foreground(&self.app, &self.main, &self.responder) {
            self.state = PresentationState::Unavailable;
        } else if Instant::now() >= self.deadline {
            self.state = PresentationState::Expired;
        } else if let Some(surface) = &self.surface {
            let exact = admitted_frame(self.frame, self.screen)
                && surface.frame() == self.frame
                && self.page.frame() == viewport()
                && surface.isVisible()
                && !self.page.isHiddenOrHasHiddenAncestor()
                && !surface.isKeyWindow()
                && !surface.isMainWindow()
                && !surface.canBecomeKeyWindow()
                && !surface.canBecomeMainWindow()
                && surface.ignoresMouseEvents()
                && surface.isOpaque()
                && surface.alphaValue() == 1.0
                && self.page.alphaValue() == 1.0
                && self
                    .page
                    .window()
                    .is_some_and(|window| std::ptr::eq(&*window, &**surface));
            let visible = surface
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible)
                && self.page.visibleRect() == viewport();
            self.state = if !exact {
                PresentationState::Failed
            } else if visible {
                PresentationState::Ready
            } else if self.state == PresentationState::Acquiring {
                PresentationState::Acquiring
            } else {
                PresentationState::Unavailable
            };
        } else {
            self.state = PresentationState::Failed;
        }
        self.state
    }

    /// Hide before restoring hierarchy. Never repair or reacquire human focus.
    /// The weak window must actually drain before this returns Retired.
    pub(crate) fn retire(&mut self) -> PresentationState {
        if matches!(
            self.state,
            PresentationState::Retiring | PresentationState::Retired
        ) {
            return self.poll();
        }
        let before = human_owners(&self.app);
        self.state = PresentationState::Retiring;
        self.page.setHidden(true);
        if let Some(surface) = self.surface.take() {
            surface.orderOut(None);
            self.parent.addSubview(&self.page);
            self.page.setFrame(self.original_frame);
            self.retired_surface = Some(Weak::from_retained(&surface));
            surface.close();
        }
        // SAFETY: the retained native page and original parent remain alive on
        // the main thread. No pointer escapes this identity comparison.
        let original_parent = unsafe { self.page.superview() }
            .is_some_and(|parent| std::ptr::eq(&*parent, &*self.parent));
        self.cleanup_failed |= !self.page.isHidden()
            || human_owners(&self.app) != before
            || self.page.frame() != self.original_frame
            || !original_parent;
        self.poll()
    }

    pub(crate) fn visible_for_audit(&self) -> bool {
        self.surface
            .as_ref()
            .is_some_and(|surface| surface.isVisible())
            || !self.page.isHidden()
    }
    pub(crate) fn human_current(&self) -> bool {
        foreground(&self.app, &self.main, &self.responder)
    }
}
#[cfg(feature = "native-agentic-work-resource-probe")]
pub(crate) fn retained_page_hidden(view: &wry::WebView) -> bool {
    super::native_webview(view).isHidden()
}
impl Drop for WorkObservationPresentation {
    fn drop(&mut self) {
        let _ = self.retire();
    }
}

fn viewport() -> NSRect {
    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 800.0))
}
fn surface_frame(screen: NSRect) -> Option<NSRect> {
    let origin = |start: f64, available: f64, required: f64| {
        let first = start.ceil();
        let last = (start + available - required).floor();
        let center = (start + (available - required) / 2.0).floor();
        ([first, last, center].into_iter().all(f64::is_finite) && first <= last)
            .then(|| center.clamp(first, last))
    };
    let frame = NSRect::new(
        NSPoint::new(
            origin(screen.origin.x, screen.size.width, 1280.0)?,
            origin(screen.origin.y, screen.size.height, 800.0)?,
        ),
        viewport().size,
    );
    admitted_frame(frame, screen).then_some(frame)
}
fn admitted_frame(frame: NSRect, screen: NSRect) -> bool {
    [
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
        screen.origin.x,
        screen.origin.y,
        screen.size.width,
        screen.size.height,
        frame.origin.x + frame.size.width,
        frame.origin.y + frame.size.height,
        screen.origin.x + screen.size.width,
        screen.origin.y + screen.size.height,
    ]
    .into_iter()
    .all(f64::is_finite)
        && frame.size == viewport().size
        && screen.size.width >= 1280.0
        && screen.size.height >= 800.0
        && (frame.origin.x + frame.size.width) - frame.origin.x == frame.size.width
        && (frame.origin.y + frame.size.height) - frame.origin.y == frame.size.height
        && frame.origin.x >= screen.origin.x
        && frame.origin.y >= screen.origin.y
        && frame.origin.x + frame.size.width <= screen.origin.x + screen.size.width
        && frame.origin.y + frame.size.height <= screen.origin.y + screen.size.height
}
fn foreground(app: &NSApplication, main: &NSWindow, responder: &NSResponder) -> bool {
    foreground_facts([
        app.isActive(),
        main.isVisible(),
        !main.isMiniaturized(),
        app.keyWindow()
            .is_some_and(|window| std::ptr::eq(&*window, main)),
        app.mainWindow()
            .is_some_and(|window| std::ptr::eq(&*window, main)),
        main.firstResponder()
            .is_some_and(|current| std::ptr::eq(&*current, responder)),
    ])
}
fn foreground_facts(facts: [bool; 6]) -> bool {
    facts.into_iter().all(|fact| fact)
}
#[derive(Eq, PartialEq)]
struct HumanOwners {
    active: bool,
    key: Option<usize>,
    main: Option<usize>,
    key_responder: Option<usize>,
    main_responder: Option<usize>,
}
fn human_owners(app: &NSApplication) -> HumanOwners {
    let key = app.keyWindow();
    let main = app.mainWindow();
    HumanOwners {
        active: app.isActive(),
        key: key.as_ref().map(|w| Retained::as_ptr(w).addr()),
        main: main.as_ref().map(|w| Retained::as_ptr(w).addr()),
        key_responder: key
            .and_then(|w| w.firstResponder())
            .map(|r| Retained::as_ptr(&r).addr()),
        main_responder: main
            .and_then(|w| w.firstResponder())
            .map(|r| Retained::as_ptr(&r).addr()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_viewport_never_scales_clips_or_admits_nonfinite_screen_geometry() {
        for screen in [
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1920.0, 1080.0)),
            NSRect::new(NSPoint::new(-2000.25, 13.5), NSSize::new(1600.5, 999.5)),
            viewport(),
        ] {
            let frame = surface_frame(screen).unwrap();
            assert!(admitted_frame(frame, screen));
            assert_eq!(frame.size, viewport().size);
            assert_eq!(frame.origin.x.fract(), 0.0);
            assert_eq!(frame.origin.y.fract(), 0.0);
        }
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(surface_frame(NSRect::new(
                NSPoint::new(bad, 0.0),
                NSSize::new(1920.0, 1080.0)
            ))
            .is_none());
            assert!(surface_frame(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(bad, 1080.0)
            ))
            .is_none());
        }
        assert!(surface_frame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(1279.0, 800.0)
        ))
        .is_none());
        assert!(surface_frame(NSRect::new(
            NSPoint::new(0.25, 0.0),
            NSSize::new(1280.0, 800.0)
        ))
        .is_none());
    }
    #[test]
    fn every_human_foreground_fact_is_required_without_repairing_focus() {
        assert!(foreground_facts([true; 6]));
        for index in 0..6 {
            let mut facts = [true; 6];
            facts[index] = false;
            assert!(!foreground_facts(facts));
        }
        let source = include_str!("work_observation_presentation.rs")
            .split("\n#[cfg(test)]")
            .next()
            .unwrap();
        for forbidden in [
            "makeKey",
            "makeMain",
            "makeFirstResponder",
            "activateWith",
            "setActivation",
            "setScheduling",
            "requestAnimationFrame",
            "evaluateJavaScript",
        ] {
            assert!(!source.contains(forbidden));
        }
        assert!(source.contains("surface.canBecomeKeyWindow()"));
        assert!(source.contains("surface.canBecomeMainWindow()"));
        assert!(source.contains("surface.setIgnoresMouseEvents(true)"));
        assert!(source.contains("self.retired_surface = Some(Weak::from_retained(&surface))"));
        assert!(source.contains("surface.load().is_none()"));
        assert!(source.contains("self.cleanup_failed |="));
        assert!(source.contains("human_owners(&self.app) != before"));
    }
}
