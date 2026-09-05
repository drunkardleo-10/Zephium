//! Honest on-screen rendering opportunity, with no focus or input authority.
//! This module is release-excluded; it is not a product presentation policy.
use super::*;

use objc2_app_kit::{NSScreen, NSWindowOcclusionState};

const PRESENTED_TIMEOUT: Duration = Duration::from_secs(5);

/// Exact fixed-fixture outcome, returned only after hiding and native teardown.
#[derive(Debug)]
pub struct MacosAgenticPresentedRenderingReport {
    /// Whole explicitly presented lifetime, including visibility convergence.
    pub presented_elapsed_ms: u64,
    /// Time to public native evidence that the surface has visible screen pixels.
    pub opportunity_elapsed_ms: u64,
    /// Internal responder changed to the exact owned page while input stayed excluded.
    pub exact_page_responder_observed: bool,
    /// Bounded fresh semantic samples, with offsets from opportunity acquisition.
    pub measurement: MacosAgenticRenderingProbeReport,
}

/// Separate guard: hidden probes never gain a "visibility is optional" flag.
pub(super) struct PresentedStateGuard<'a> {
    app: &'a NSApplication,
    window: &'a NSWindow,
    page: &'a WKWebView,
    first_responder: Option<usize>,
    expected_window_frame: NSRect,
    deadline: Instant,
    require_visible_pixels: Cell<bool>,
    exact_page_responder_observed: Cell<bool>,
    failure: Cell<Option<&'static str>>,
}

#[derive(Clone, Copy)]
struct PresentedFacts {
    within_deadline: bool,
    app_inactive: bool,
    no_key_authority: bool,
    no_main_authority: bool,
    mouse_ignored: bool,
    responder_owned: bool,
    surface_visible: bool,
    surface_opaque: bool,
    exact_geometry: bool,
    visible_pixels: bool,
}

fn reject_facts(facts: PresentedFacts, require_visible_pixels: bool) -> Option<&'static str> {
    for (valid, refusal) in [
        (facts.within_deadline, "presented_rendering_deadline"),
        (facts.app_inactive, "presented_application_became_active"),
        (facts.no_key_authority, "presented_key_authority"),
        (facts.no_main_authority, "presented_main_authority"),
        (facts.mouse_ignored, "presented_mouse_authority"),
        (facts.responder_owned, "presented_responder_changed"),
        (facts.surface_visible, "presented_surface_not_visible"),
        (facts.surface_opaque, "presented_surface_not_opaque"),
        (facts.exact_geometry, "presented_surface_geometry"),
        (
            !require_visible_pixels || facts.visible_pixels,
            "presented_surface_occluded",
        ),
    ] {
        if !valid {
            return Some(refusal);
        }
    }
    None
}

impl PresentedStateGuard<'_> {
    pub(super) fn sample(&self) {
        let responder = self.window.firstResponder();
        let responder_identity = responder
            .as_ref()
            .map(|value| Retained::as_ptr(value).addr());
        let exact_page = std::ptr::from_ref(self.page).addr();
        let geometry_failure = if self.window.frame() != self.expected_window_frame {
            Some("presented_window_frame_mismatch")
        } else if self.page.frame().size != NSSize::new(1280.0, 800.0) {
            Some("presented_page_viewport_mismatch")
        } else if !self
            .page
            .window()
            .is_some_and(|window| std::ptr::eq(&*window, self.window))
        {
            Some("presented_parent_window_mismatch")
        } else {
            None
        };
        let facts = PresentedFacts {
            within_deadline: Instant::now() < self.deadline,
            app_inactive: !self.app.isActive(),
            no_key_authority: !self.window.isKeyWindow() && !self.window.canBecomeKeyWindow(),
            no_main_authority: !self.window.isMainWindow() && !self.window.canBecomeMainWindow(),
            mouse_ignored: self.window.ignoresMouseEvents(),
            responder_owned: responder_is_owned(
                responder_identity,
                self.first_responder,
                exact_page,
            ),
            surface_visible: self.window.isVisible() && !self.page.isHiddenOrHasHiddenAncestor(),
            surface_opaque: self.window.alphaValue() == 1.0
                && self.window.isOpaque()
                && self.page.alphaValue() == 1.0,
            exact_geometry: geometry_failure.is_none(),
            visible_pixels: self.has_visible_pixels(),
        };
        if self.failure.get().is_none() {
            let failure = reject_facts(facts, self.require_visible_pixels.get());
            self.failure.set(match failure {
                Some("presented_responder_changed") => Some(self.responder_refusal(responder)),
                Some("presented_surface_geometry") => geometry_failure,
                other => other,
            });
            if failure.is_none() && responder_identity == Some(exact_page) {
                self.exact_page_responder_observed.set(true);
            }
        }
    }

    fn responder_refusal(
        &self,
        responder: Option<Retained<objc2_app_kit::NSResponder>>,
    ) -> &'static str {
        let Some(responder) = responder else {
            return "presented_responder_changed_none";
        };
        if Retained::as_ptr(&responder).addr() == std::ptr::from_ref(self.window).addr() {
            return "presented_responder_changed_exact_window";
        }
        if Retained::as_ptr(&responder).addr() == std::ptr::from_ref(self.page).addr() {
            return "presented_responder_changed_exact_page";
        }
        if responder
            .downcast::<NSView>()
            .is_ok_and(|view| view.isDescendantOf(self.page))
        {
            return "presented_responder_changed_owned_descendant";
        }
        "presented_responder_changed_foreign"
    }

    fn has_visible_pixels(&self) -> bool {
        // AppKit's Visible bit means not fully occluded, not proof that every
        // pixel is uncovered. The separate view rectangle excludes parent clipping.
        self.window
            .occlusionState()
            .contains(NSWindowOcclusionState::Visible)
            && self.page.visibleRect().size == NSSize::new(1280.0, 800.0)
    }

    pub(super) fn failed(&self) -> bool {
        self.failure.get().is_some()
    }

    pub(super) fn failure_stage(&self) -> Option<&'static str> {
        self.failure.get()
    }
}

fn responder_is_owned(actual: Option<usize>, original: Option<usize>, exact_page: usize) -> bool {
    // Native identities only: no class/descendant inference grants permission.
    exact_page != 0 && (actual == original || actual == Some(exact_page))
}

struct PresentedScope<'a> {
    window: &'a NSWindow,
    page: &'a WKWebView,
    host: &'a NSView,
    original_window_frame: NSRect,
    original_host_frame: NSRect,
    original_ignores_mouse: bool,
    original_opaque: bool,
}

impl PresentedScope<'_> {
    fn hide(&self) -> Result<(), &'static str> {
        // Hide before restoring geometry, even after refusal/timeout. Neither
        // this path nor its Drop backstop can make a page key or activate an app.
        self.page.setHidden(true);
        self.window.orderOut(None);
        self.host.setFrame(self.original_host_frame);
        self.window
            .setFrame_display(self.original_window_frame, false);
        self.window
            .setIgnoresMouseEvents(self.original_ignores_mouse);
        self.window.setOpaque(self.original_opaque);
        if self.window.isVisible()
            || !self.page.isHidden()
            || self.window.frame() != self.original_window_frame
            || self.host.frame() != self.original_host_frame
            || self.window.ignoresMouseEvents() != self.original_ignores_mouse
            || self.window.isOpaque() != self.original_opaque
        {
            return Err("presented_restore_attestation");
        }
        Ok(())
    }
}

impl Drop for PresentedScope<'_> {
    fn drop(&mut self) {
        let _ = self.hide();
    }
}

fn centered_frame(visible: NSRect) -> Result<NSRect, &'static str> {
    if !visible.origin.x.is_finite()
        || !visible.origin.y.is_finite()
        || !visible.size.width.is_finite()
        || !visible.size.height.is_finite()
        || visible.size.width < 1280.0
        || visible.size.height < 800.0
    {
        return Err("presented_screen_unavailable");
    }
    let frame = NSRect::new(
        NSPoint::new(
            visible.origin.x + (visible.size.width - 1280.0) / 2.0,
            visible.origin.y + (visible.size.height - 800.0) / 2.0,
        ),
        NSSize::new(1280.0, 800.0),
    );
    if !frame.origin.x.is_finite() || !frame.origin.y.is_finite() {
        return Err("presented_screen_unavailable");
    }
    Ok(frame)
}

pub(super) fn measure(
    view: &AgentOwnedView,
    context: zephium_agentic::ContextJoin,
    operation: zephium_agentic::ContextOperationJoin,
    url: &str,
    runtime: &ProbeRuntime<'_, '_>,
    host: &NSView,
) -> Result<MacosAgenticPresentedRenderingReport, &'static str> {
    let ProbeNativeState::Hidden(original) = runtime.native_guard else {
        return Err("presented_original_guard");
    };
    original.sample();
    if runtime.failed()
        || operation.context() != context
        || view
            .navigation()
            .document_finished_for_audit(operation)
            .is_none()
        || original.window.canBecomeKeyWindow()
        || original.window.canBecomeMainWindow()
        || original.window.alphaValue() != 1.0
    {
        return Err("presented_admission");
    }
    let mtm = MainThreadMarker::new().ok_or("presented_main_thread")?;
    let screen = NSScreen::mainScreen(mtm).ok_or("presented_screen_unavailable")?;
    let frame = centered_frame(screen.visibleFrame())?;
    let started = Instant::now();
    let deadline = started
        .checked_add(PRESENTED_TIMEOUT)
        .ok_or("presented_deadline")?;
    let scope = PresentedScope {
        window: original.window,
        page: original.page,
        host,
        original_window_frame: original.window.frame(),
        original_host_frame: host.frame(),
        original_ignores_mouse: original.window.ignoresMouseEvents(),
        original_opaque: original.window.isOpaque(),
    };
    let outcome = (|| {
        scope.window.setIgnoresMouseEvents(true);
        scope.window.setOpaque(true);
        scope.window.setFrame_display(frame, false);
        scope
            .host
            .setFrame(NSRect::new(NSPoint::new(0.0, 0.0), frame.size));
        scope.page.setHidden(false);
        // Publicly documented to preserve key and main windows even when this
        // application is inactive. This is real presentation, never disguised.
        scope.window.orderFrontRegardless();
        let presented = ProbeNativeState::Presented(PresentedStateGuard {
            app: original.app,
            window: original.window,
            page: original.page,
            first_responder: original.first_responder,
            expected_window_frame: frame,
            deadline,
            require_visible_pixels: Cell::new(false),
            exact_page_responder_observed: Cell::new(false),
            failure: Cell::new(None),
        });
        let presented_runtime = ProbeRuntime {
            callbacks: runtime.callbacks,
            run_loop: runtime.run_loop,
            native_guard: &presented,
        };
        let ProbeNativeState::Presented(guard) = &presented else {
            return Err("presented_guard");
        };
        presented.sample();
        while !presented_runtime.failed()
            && !guard.has_visible_pixels()
            && Instant::now() < deadline
        {
            presented_runtime.pump();
        }
        if presented_runtime.failed() {
            return Err(presented_runtime
                .failure_stage()
                .unwrap_or("presented_native_state"));
        }
        guard.require_visible_pixels.set(true);
        presented.sample();
        if presented_runtime.failed() {
            return Err("presented_opportunity_unavailable");
        }
        let opportunity_elapsed_ms =
            u64::try_from(started.elapsed().as_millis()).map_err(|_| "presented_clock")?;
        let measurement = rendering::measure(view, context, operation, url, &presented_runtime)?;
        presented.sample();
        if presented_runtime.failed() {
            return Err(presented_runtime
                .failure_stage()
                .unwrap_or("presented_native_state"));
        }
        Ok((
            opportunity_elapsed_ms,
            guard.exact_page_responder_observed.get(),
            measurement,
        ))
    })();
    scope.hide()?;
    original.sample();
    if runtime.failed() {
        return Err("presented_restored_native_state");
    }
    let presented_elapsed_ms =
        u64::try_from(started.elapsed().as_millis()).map_err(|_| "presented_clock")?;
    let (opportunity_elapsed_ms, exact_page_responder_observed, measurement) = outcome?;
    Ok(MacosAgenticPresentedRenderingReport {
        presented_elapsed_ms,
        opportunity_elapsed_ms,
        exact_page_responder_observed,
        measurement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presented_evidence_requires_independent_focus_input_geometry_and_time_facts() {
        let valid = PresentedFacts {
            within_deadline: true,
            app_inactive: true,
            no_key_authority: true,
            no_main_authority: true,
            mouse_ignored: true,
            responder_owned: true,
            surface_visible: true,
            surface_opaque: true,
            exact_geometry: true,
            visible_pixels: true,
        };
        assert_eq!(reject_facts(valid, true), None);
        for mutate in [
            |f: &mut PresentedFacts| f.within_deadline = false,
            |f: &mut PresentedFacts| f.app_inactive = false,
            |f: &mut PresentedFacts| f.no_key_authority = false,
            |f: &mut PresentedFacts| f.no_main_authority = false,
            |f: &mut PresentedFacts| f.mouse_ignored = false,
            |f: &mut PresentedFacts| f.responder_owned = false,
            |f: &mut PresentedFacts| f.surface_visible = false,
            |f: &mut PresentedFacts| f.surface_opaque = false,
            |f: &mut PresentedFacts| f.exact_geometry = false,
            |f: &mut PresentedFacts| f.visible_pixels = false,
        ] {
            let mut changed = valid;
            mutate(&mut changed);
            assert!(reject_facts(changed, true).is_some());
        }
        let mut pending = valid;
        pending.visible_pixels = false;
        assert_eq!(reject_facts(pending, false), None);
        pending.app_inactive = false;
        assert!(reject_facts(pending, false).is_some());
    }

    #[test]
    fn presented_responder_is_bound_only_to_original_or_exact_native_page() {
        assert!(responder_is_owned(Some(10), Some(10), 20));
        assert!(responder_is_owned(Some(20), Some(10), 20));
        assert!(responder_is_owned(None, None, 20));
        assert!(responder_is_owned(Some(20), None, 20));
        assert!(!responder_is_owned(None, Some(10), 20));
        for foreign in [0, 1, 11, 19, 21, usize::MAX] {
            assert!(!responder_is_owned(Some(foreign), Some(10), 20));
        }
        assert!(!responder_is_owned(Some(10), Some(10), 0));
        assert!(!responder_is_owned(Some(0), Some(10), 0));
    }

    #[test]
    fn presented_surface_requires_the_actual_fixed_viewport_to_fit_on_screen() {
        let visible = NSRect::new(NSPoint::new(-1440.0, 30.0), NSSize::new(1440.0, 900.0));
        assert_eq!(
            centered_frame(visible).unwrap(),
            NSRect::new(NSPoint::new(-1360.0, 80.0), NSSize::new(1280.0, 800.0))
        );
        for size in [
            NSSize::new(1279.0, 800.0),
            NSSize::new(1280.0, 799.0),
            NSSize::new(f64::NAN, 900.0),
            NSSize::new(1440.0, f64::INFINITY),
        ] {
            assert!(centered_frame(NSRect::new(NSPoint::new(0.0, 0.0), size)).is_err());
        }
        assert!(centered_frame(NSRect::new(NSPoint::new(f64::NAN, 0.0), visible.size)).is_err());
    }
}
