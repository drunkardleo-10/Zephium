//! Release-excluded rendering ownership in the real foreground application.
//! No application bootstrap, event pump, provider or page-authored capability.

use std::time::{Duration, Instant};

use objc2::rc::{Retained, Weak};
use objc2::MainThreadOnly as _;
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSResponder, NSView, NSWindow, NSWindowOcclusionState,
    NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use objc2_web_kit::WKWebView;
use zephium_agentic::{ContextJoin, ForegroundRenderingState};

const RENDERING_BUDGET: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
struct ForegroundFacts {
    active: bool,
    main_visible: bool,
    exact_key: bool,
    exact_main: bool,
    exact_responder: bool,
}

impl ForegroundFacts {
    fn admitted(self) -> bool {
        self.active
            && self.main_visible
            && self.exact_key
            && self.exact_main
            && self.exact_responder
    }
}

fn viewport() -> NSRect {
    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 800.0))
}

fn surface_frame(screen: NSRect) -> Option<NSRect> {
    if ![
        screen.origin.x,
        screen.origin.y,
        screen.size.width,
        screen.size.height,
    ]
    .into_iter()
    .all(f64::is_finite)
        || screen.size.width < 1280.0
        || screen.size.height < 800.0
    {
        return None;
    }
    let frame = NSRect::new(
        NSPoint::new(
            screen.origin.x + (screen.size.width - 1280.0) / 2.0,
            screen.origin.y + (screen.size.height - 800.0) / 2.0,
        ),
        viewport().size,
    );
    admitted_frame(frame, screen).then_some(frame)
}

fn admitted_frame(frame: NSRect, screen: NSRect) -> bool {
    let coordinates = [
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
    ];
    coordinates.into_iter().all(f64::is_finite)
        && frame.size == viewport().size
        && screen.size.width >= 1280.0
        && screen.size.height >= 800.0
        && frame.origin.x >= screen.origin.x
        && frame.origin.y >= screen.origin.y
        && frame.origin.x + frame.size.width <= screen.origin.x + screen.size.width
        && frame.origin.y + frame.size.height <= screen.origin.y + screen.size.height
}

/// Retained inside the existing exact AgentOwnedContext, never a parallel view.
pub(crate) struct ForegroundRenderingLease {
    context: ContextJoin,
    app: Retained<NSApplication>,
    main: Retained<NSWindow>,
    responder: Retained<NSResponder>,
    page: Retained<WKWebView>,
    original_parent: Retained<NSView>,
    original_frame: NSRect,
    surface: Option<Retained<NSWindow>>,
    retired_surface: Option<Weak<NSWindow>>,
    screen: NSRect,
    frame: NSRect,
    deadline: Instant,
    state: ForegroundRenderingState,
    cleanup_failed: bool,
    revocation: Option<ForegroundRenderingState>,
}

impl ForegroundRenderingLease {
    /// Prepare a hidden auxiliary container; the host must retain this owner
    /// before calling present. No page or human focus changes at preparation.
    pub(crate) fn prepare(
        context: ContextJoin,
        view: &wry::WebView,
    ) -> Result<Self, ForegroundRenderingState> {
        let mtm = MainThreadMarker::new().ok_or(ForegroundRenderingState::Failed)?;
        let app = NSApplication::sharedApplication(mtm);
        let page = super::native_webview(view);
        let main = page.window().ok_or(ForegroundRenderingState::Failed)?;
        let responder = main
            .firstResponder()
            .ok_or(ForegroundRenderingState::DeferredForeground)?;
        if !foreground(&app, &main, &responder).admitted() {
            return Err(ForegroundRenderingState::DeferredForeground);
        }
        // SAFETY: the exact retained native page is read on the main thread;
        // the returned parent is retained before any hierarchy mutation.
        let original_parent =
            unsafe { page.superview() }.ok_or(ForegroundRenderingState::Failed)?;
        let original_frame = page.frame();
        if !page.isHidden()
            || original_frame.size != viewport().size
            || Retained::as_ptr(&responder).addr() == Retained::as_ptr(&page).addr()
        {
            return Err(ForegroundRenderingState::Failed);
        }
        let screen = main
            .screen()
            .ok_or(ForegroundRenderingState::Failed)?
            .visibleFrame();
        let frame = surface_frame(screen).ok_or(ForegroundRenderingState::Failed)?;
        let deadline = Instant::now()
            .checked_add(RENDERING_BUDGET)
            .ok_or(ForegroundRenderingState::Failed)?;
        let surface = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: the retained owner, not NSWindow::close, releases this window.
        unsafe { surface.setReleasedWhenClosed(false) };
        surface.setIgnoresMouseEvents(true);
        surface.setOpaque(true);
        Ok(Self {
            context,
            app,
            main,
            responder,
            page,
            original_parent,
            original_frame,
            surface: Some(surface),
            retired_surface: None,
            screen,
            frame,
            deadline,
            state: ForegroundRenderingState::Prepared,
            cleanup_failed: false,
            revocation: None,
        })
    }

    pub(crate) fn present(&mut self, context: ContextJoin) -> ForegroundRenderingState {
        if self.context != context || self.state != ForegroundRenderingState::Prepared {
            self.state = ForegroundRenderingState::Failed;
            return self.state;
        }
        if !foreground(&self.app, &self.main, &self.responder).admitted() {
            self.state = ForegroundRenderingState::DeferredForeground;
            return self.state;
        }
        let Some(surface) = self.surface.as_ref() else {
            self.state = ForegroundRenderingState::Failed;
            return self.state;
        };
        // This fallible attestation occurs only after the host retains the
        // hidden native owner, so every refusal can prove exact retirement.
        if surface.isVisible()
            || surface.canBecomeKeyWindow()
            || surface.canBecomeMainWindow()
            || surface.alphaValue() != 1.0
        {
            self.state = ForegroundRenderingState::Failed;
            return self.state;
        }
        let Some(parent) = surface.contentView() else {
            self.state = ForegroundRenderingState::Failed;
            return self.state;
        };
        // Publish the effect phase before native hierarchy/presentation calls.
        self.state = ForegroundRenderingState::Acquiring;
        parent.addSubview(&self.page);
        self.page.setFrame(viewport());
        self.page.setHidden(false);
        surface.orderFrontRegardless();
        self.frame = surface.frame();
        self.poll(context)
    }

    pub(crate) fn poll(&mut self, context: ContextJoin) -> ForegroundRenderingState {
        if self.context != context {
            self.state = ForegroundRenderingState::Failed;
            return self.state;
        }
        if self.state == ForegroundRenderingState::Retiring {
            if self
                .retired_surface
                .as_ref()
                .is_some_and(|surface| surface.load().is_none())
            {
                self.retired_surface = None;
                self.state = cleanup_state(self.cleanup_failed, true);
            }
            return self.state;
        }
        if !matches!(
            self.state,
            ForegroundRenderingState::Acquiring | ForegroundRenderingState::Ready
        ) {
            return self.state;
        }
        if !foreground(&self.app, &self.main, &self.responder).admitted() {
            self.state = ForegroundRenderingState::DeferredForeground;
        } else if Instant::now() >= self.deadline {
            self.state = ForegroundRenderingState::Expired;
        } else if let Some(surface) = self.surface.as_ref() {
            if !admitted_frame(self.frame, self.screen)
                || surface.frame() != self.frame
                || self.page.frame() != viewport()
                || !surface.isVisible()
                || self.page.isHiddenOrHasHiddenAncestor()
                || surface.isKeyWindow()
                || surface.isMainWindow()
                || surface.canBecomeKeyWindow()
                || surface.canBecomeMainWindow()
                || !surface.ignoresMouseEvents()
                || !surface.isOpaque()
                || surface.alphaValue() != 1.0
                || self.page.alphaValue() != 1.0
                || !self
                    .page
                    .window()
                    .is_some_and(|window| std::ptr::eq(&*window, &**surface))
            {
                self.state = ForegroundRenderingState::Failed;
            } else {
                let visible = surface
                    .occlusionState()
                    .contains(NSWindowOcclusionState::Visible)
                    && self.page.visibleRect() == viewport();
                self.state = match (self.state, visible) {
                    (_, true) => ForegroundRenderingState::Ready,
                    (ForegroundRenderingState::Acquiring, false) => {
                        ForegroundRenderingState::Acquiring
                    }
                    _ => ForegroundRenderingState::DeferredForeground,
                };
            }
        } else {
            self.state = ForegroundRenderingState::Failed;
        }
        self.state
    }

    /// Hides before restoring the exact parent/frame. It never restores human
    /// focus: a changed human foreground owner remains the user's authority.
    pub(crate) fn retire(&mut self) -> ForegroundRenderingState {
        if !self.cleanup_failed
            && matches!(
                self.state,
                ForegroundRenderingState::Retiring | ForegroundRenderingState::Retired
            )
        {
            return self.poll(self.context);
        }
        let human_before = human_owners(&self.app);
        self.state = ForegroundRenderingState::Retiring;
        self.page.setHidden(true);
        if let Some(surface) = self.surface.take() {
            surface.orderOut(None);
            self.original_parent.addSubview(&self.page);
            self.page.setFrame(self.original_frame);
            self.retired_surface = Some(Weak::from_retained(&surface));
            surface.close();
        }
        if !self.page.isHidden()
            || human_owners(&self.app) != human_before
            || self.page.frame() != self.original_frame
            // SAFETY: the retained page/parent are main-thread-owned and the
            // exact returned parent is compared without escaping its lifetime.
            || !unsafe { self.page.superview() }
                .is_some_and(|parent| std::ptr::eq(&*parent, &*self.original_parent))
        {
            self.state = ForegroundRenderingState::Failed;
            self.cleanup_failed = true;
            return self.state;
        }
        if self.cleanup_failed {
            self.state = ForegroundRenderingState::Failed;
            return self.state;
        }
        self.poll(self.context)
    }

    /// Revalidate before each native step/completion. Revocation hides in the
    /// same main-thread step, while preserving the truthful reason to caller.
    pub(crate) fn guard(&mut self, context: ContextJoin) -> ForegroundRenderingState {
        let state = self.revocation.unwrap_or_else(|| self.poll(context));
        if matches!(
            state,
            ForegroundRenderingState::DeferredForeground
                | ForegroundRenderingState::Expired
                | ForegroundRenderingState::Failed
        ) {
            self.revocation = Some(state);
            let cleanup = self.retire();
            if cleanup == ForegroundRenderingState::Failed {
                return cleanup;
            }
        }
        state
    }

    pub(crate) fn visible_for_audit(&self) -> bool {
        self.surface
            .as_ref()
            .is_some_and(|surface| surface.isVisible())
            || !self.page.isHidden()
    }
}

fn cleanup_state(failed: bool, drained: bool) -> ForegroundRenderingState {
    match (failed, drained) {
        (true, _) => ForegroundRenderingState::Failed,
        (false, true) => ForegroundRenderingState::Retired,
        (false, false) => ForegroundRenderingState::Retiring,
    }
}

/// Ephemeral identity comparison only: never exported, persisted or logged.
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
        key: key.as_ref().map(|window| Retained::as_ptr(window).addr()),
        main: main.as_ref().map(|window| Retained::as_ptr(window).addr()),
        key_responder: key
            .and_then(|window| window.firstResponder())
            .map(|responder| Retained::as_ptr(&responder).addr()),
        main_responder: main
            .and_then(|window| window.firstResponder())
            .map(|responder| Retained::as_ptr(&responder).addr()),
    }
}

impl Drop for ForegroundRenderingLease {
    fn drop(&mut self) {
        let _ = self.retire();
    }
}

fn foreground(app: &NSApplication, main: &NSWindow, responder: &NSResponder) -> ForegroundFacts {
    ForegroundFacts {
        active: app.isActive(),
        main_visible: main.isVisible() && !main.isMiniaturized(),
        exact_key: app
            .keyWindow()
            .is_some_and(|window| std::ptr::eq(&*window, main)),
        exact_main: app
            .mainWindow()
            .is_some_and(|window| std::ptr::eq(&*window, main)),
        exact_responder: main
            .firstResponder()
            .is_some_and(|current| std::ptr::eq(&*current, responder)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retirement_never_erases_a_prior_cleanup_failure() {
        assert_eq!(
            cleanup_state(false, false),
            ForegroundRenderingState::Retiring
        );
        assert_eq!(
            cleanup_state(false, true),
            ForegroundRenderingState::Retired
        );
        assert_eq!(cleanup_state(true, false), ForegroundRenderingState::Failed);
        assert_eq!(cleanup_state(true, true), ForegroundRenderingState::Failed);
    }

    #[test]
    fn foreground_admission_needs_every_independent_human_owner_fact() {
        let valid = ForegroundFacts {
            active: true,
            main_visible: true,
            exact_key: true,
            exact_main: true,
            exact_responder: true,
        };
        assert!(valid.admitted());
        for mutate in [
            |v: &mut ForegroundFacts| v.active = false,
            |v: &mut ForegroundFacts| v.main_visible = false,
            |v: &mut ForegroundFacts| v.exact_key = false,
            |v: &mut ForegroundFacts| v.exact_main = false,
            |v: &mut ForegroundFacts| v.exact_responder = false,
        ] {
            let mut changed = valid;
            mutate(&mut changed);
            assert!(!changed.admitted());
        }
    }

    #[test]
    fn foreground_geometry_never_scales_or_clips_the_fixed_viewport() {
        let screen = NSRect::new(NSPoint::new(-1440.0, 20.0), NSSize::new(1440.0, 900.0));
        let centered = surface_frame(screen).unwrap();
        assert!(admitted_frame(centered, screen));
        for frame in [
            NSRect::new(centered.origin, NSSize::new(1279.0, 800.0)),
            NSRect::new(NSPoint::new(-1441.0, 20.0), centered.size),
            NSRect::new(NSPoint::new(-1440.0, 121.0), centered.size),
            NSRect::new(NSPoint::new(f64::NAN, 20.0), centered.size),
        ] {
            assert!(!admitted_frame(frame, screen));
        }
        assert!(surface_frame(NSRect::new(screen.origin, NSSize::new(1279.0, 800.0))).is_none());
    }
}
