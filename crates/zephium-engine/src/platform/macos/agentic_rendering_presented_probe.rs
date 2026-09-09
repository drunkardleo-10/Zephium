//! Honest on-screen rendering opportunity, with no focus or input authority.
//! This module is release-excluded; it is not a product presentation policy.
use super::*;

use objc2_app_kit::{NSEventMask, NSEventType, NSScreen, NSWindowOcclusionState};

const PRESENTED_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_APPKIT_EVENTS: usize = 32;

fn launch_policy_valid(
    actual: NSApplicationActivationPolicy,
    expected: NSApplicationActivationPolicy,
    active: bool,
    windows: usize,
) -> bool {
    actual == expected && !active && windows == 0
}

pub(super) fn initialize_inactive(app: &NSApplication) -> Result<(), &'static str> {
    if app.isActive() || !app.windows().is_empty() {
        return Err("presented_launch_existing_authority");
    }
    // finishLaunching is documented to activate an ordinary/accessory app.
    // Complete it before any window exists, under the public prohibition on
    // both activation and window creation; only then permit accessory windows.
    if app.activationPolicy() != NSApplicationActivationPolicy::Prohibited
        && !app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited)
    {
        return Err("presented_launch_prohibition_transition");
    }
    if !launch_policy_valid(
        app.activationPolicy(),
        NSApplicationActivationPolicy::Prohibited,
        app.isActive(),
        app.windows().len(),
    ) {
        return Err("presented_launch_prohibition");
    }
    app.finishLaunching();
    if !launch_policy_valid(
        app.activationPolicy(),
        NSApplicationActivationPolicy::Prohibited,
        app.isActive(),
        app.windows().len(),
    ) {
        return Err("presented_launch_activated");
    }
    if app.activationPolicy() != NSApplicationActivationPolicy::Accessory
        && !app.setActivationPolicy(NSApplicationActivationPolicy::Accessory)
    {
        return Err("presented_launch_accessory_transition");
    }
    if !launch_policy_valid(
        app.activationPolicy(),
        NSApplicationActivationPolicy::Accessory,
        app.isActive(),
        app.windows().len(),
    ) {
        return Err("presented_launch_accessory");
    }
    Ok(())
}

fn appkit_event_permitted(event_type: NSEventType, dispatched: usize) -> bool {
    event_type == NSEventType::AppKitDefined && dispatched < MAX_APPKIT_EVENTS
}

fn viewport_frame() -> NSRect {
    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 800.0))
}

fn opportunity_refusal(window_has_pixels: bool, page_unclipped: bool) -> Option<&'static str> {
    match (window_has_pixels, page_unclipped) {
        (true, true) => None,
        (false, true) => Some("presented_window_occluded"),
        (true, false) => Some("presented_page_clipped"),
        (false, false) => Some("presented_window_occluded_and_page_clipped"),
    }
}

/// Exact fixed-fixture outcome, returned only after hiding and native teardown.
#[derive(Debug)]
pub struct MacosAgenticPresentedRenderingReport {
    /// Whole explicitly presented lifetime, including visibility convergence.
    pub presented_elapsed_ms: u64,
    /// Time to public native evidence that the surface has visible screen pixels.
    pub opportunity_elapsed_ms: u64,
    /// Internal responder changed to the exact owned page while input stayed excluded.
    pub exact_page_responder_observed: bool,
    /// Exact already-queued non-input native lifecycle events dispatched once.
    pub appkit_events_dispatched: usize,
    /// Bounded fresh semantic samples, with offsets from opportunity acquisition.
    pub measurement: MacosAgenticRenderingProbeReport,
}

/// Failed attempt evidence; a provisional measurement never certifies cleanup.
#[derive(Debug)]
pub struct MacosAgenticPresentedRenderingFailure {
    /// Authoritative refusal, including a later restoration/teardown failure.
    pub stage: &'static str,
    /// Earlier refusal, retained when later cleanup supersedes it.
    pub prior_stage: Option<&'static str>,
    /// Completed content-free samples only, not a successful native qualification.
    pub provisional_measurement: Option<MacosAgenticRenderingProbeReport>,
}

impl From<&'static str> for MacosAgenticPresentedRenderingFailure {
    fn from(stage: &'static str) -> Self {
        Self {
            stage,
            prior_stage: None,
            provisional_measurement: None,
        }
    }
}

impl MacosAgenticPresentedRenderingFailure {
    pub(super) fn supersede(stage: &'static str, prior: Option<PresentedOutcome>) -> Self {
        match prior {
            Some(Ok(report)) => Self {
                stage,
                prior_stage: None,
                provisional_measurement: Some(report.measurement),
            },
            Some(Err(failure)) => Self {
                stage,
                prior_stage: if failure.stage == stage {
                    failure.prior_stage
                } else {
                    Some(failure.stage)
                },
                provisional_measurement: failure.provisional_measurement,
            },
            None => stage.into(),
        }
    }
}

pub(super) type PresentedOutcome =
    Result<MacosAgenticPresentedRenderingReport, MacosAgenticPresentedRenderingFailure>;

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
    appkit_events_dispatched: Cell<usize>,
    failure: Cell<Option<&'static str>>,
}

#[derive(Clone, Copy)]
struct PresentedFacts {
    within_deadline: bool,
    app_inactive: bool,
    app_accessory: bool,
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
        (facts.app_accessory, "presented_application_policy_changed"),
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
    pub(super) fn pump_appkit_event(&self) {
        self.sample();
        if self.failed() {
            return;
        }
        if self.appkit_events_dispatched.get() >= MAX_APPKIT_EVENTS {
            self.failure.set(Some("presented_appkit_event_capacity"));
            return;
        }
        // NSRunLoop services ports/timers, not NSApplication's queued events.
        // Immediate expiry, one native lifecycle event at most per bounded
        // slice. Unmatched input events remain queued; no event is constructed.
        let event = self.app.nextEventMatchingMask_untilDate_inMode_dequeue(
            NSEventMask::AppKitDefined,
            None,
            objc2_foundation::ns_string!("NSDefaultRunLoopMode"),
            true,
        );
        self.sample();
        if self.failed() {
            return;
        }
        let Some(event) = event else {
            return;
        };
        if !appkit_event_permitted(event.r#type(), self.appkit_events_dispatched.get()) {
            self.failure.set(Some("presented_appkit_event_contract"));
            return;
        }
        self.appkit_events_dispatched
            .set(self.appkit_events_dispatched.get() + 1);
        self.app.sendEvent(&event);
        self.sample();
    }

    pub(super) fn sample(&self) {
        let responder = self.window.firstResponder();
        let responder_identity = responder
            .as_ref()
            .map(|value| Retained::as_ptr(value).addr());
        let exact_page = std::ptr::from_ref(self.page).addr();
        let geometry_failure = if self.window.frame() != self.expected_window_frame {
            Some("presented_window_frame_mismatch")
        } else if self.page.frame() != viewport_frame() {
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
        let opportunity_failure = self.opportunity_failure();
        let facts = PresentedFacts {
            within_deadline: Instant::now() < self.deadline,
            app_inactive: !self.app.isActive(),
            app_accessory: self.app.activationPolicy() == NSApplicationActivationPolicy::Accessory,
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
            visible_pixels: opportunity_failure.is_none(),
        };
        if self.failure.get().is_none() {
            let failure = reject_facts(facts, self.require_visible_pixels.get());
            self.failure.set(match failure {
                Some("presented_responder_changed") => Some(self.responder_refusal(responder)),
                Some("presented_surface_geometry") => geometry_failure,
                Some("presented_rendering_deadline") if !self.require_visible_pixels.get() => {
                    opportunity_failure.or(failure)
                }
                Some("presented_surface_occluded") => opportunity_failure,
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
        self.opportunity_failure().is_none()
    }

    fn opportunity_failure(&self) -> Option<&'static str> {
        // AppKit's Visible bit means not fully occluded, not proof that every
        // pixel is uncovered. The separate view rectangle excludes parent clipping.
        opportunity_refusal(
            self.window
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible),
            self.page.visibleRect() == viewport_frame(),
        )
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
    original_page_frame: NSRect,
    original_ignores_mouse: bool,
    original_opaque: bool,
}

impl PresentedScope<'_> {
    fn hide(&self) -> Result<(), &'static str> {
        // Hide before restoring geometry, even after refusal/timeout. Neither
        // this path nor its Drop backstop can make a page key or activate an app.
        self.page.setHidden(true);
        self.window.orderOut(None);
        self.page.setFrame(self.original_page_frame);
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
            || self.page.frame() != self.original_page_frame
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

/// Fixed local AX experiment only; reuses the same no-key/no-input presentation
/// proof and restoration scope as the rendering experiment, never production UI.
pub(super) fn with_ax_fixture(
    runtime: &ProbeRuntime<'_, '_>,
    host: &NSView,
    body: impl FnOnce(&ProbeRuntime<'_, '_>) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    let ProbeNativeState::Hidden(original) = runtime.native_guard else {
        return Err("ax_original_guard");
    };
    original.sample();
    if runtime.failed()
        || original.window.canBecomeKeyWindow()
        || original.window.canBecomeMainWindow()
    {
        return Err("ax_presented_admission");
    }
    let mtm = MainThreadMarker::new().ok_or("ax_main_thread")?;
    let screen = NSScreen::mainScreen(mtm).ok_or("ax_screen")?.visibleFrame();
    let frame = centered_frame(screen)?;
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .ok_or("ax_deadline")?;
    let scope = PresentedScope {
        window: original.window,
        page: original.page,
        host,
        original_window_frame: original.window.frame(),
        original_host_frame: host.frame(),
        original_page_frame: original.page.frame(),
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
        scope.page.setFrame(viewport_frame());
        scope.page.setHidden(false);
        scope.window.orderFrontRegardless();
        let presented = ProbeNativeState::Presented(PresentedStateGuard {
            app: original.app,
            window: original.window,
            page: original.page,
            first_responder: original.first_responder,
            expected_window_frame: admit_actual_frame(scope.window.frame(), screen)?,
            deadline,
            require_visible_pixels: Cell::new(false),
            exact_page_responder_observed: Cell::new(false),
            appkit_events_dispatched: Cell::new(0),
            failure: Cell::new(None),
        });
        let active = ProbeRuntime {
            callbacks: runtime.callbacks,
            run_loop: runtime.run_loop,
            native_guard: &presented,
        };
        let ProbeNativeState::Presented(guard) = &presented else {
            return Err("ax_presented_guard");
        };
        presented.sample();
        while !active.failed() && !guard.has_visible_pixels() && Instant::now() < deadline {
            active.pump();
        }
        guard.require_visible_pixels.set(true);
        presented.sample();
        if active.failed() {
            return Err(active.failure_stage().unwrap_or("ax_presented_unavailable"));
        }
        let result = body(&active);
        presented.sample();
        if active.failed() {
            return Err(active.failure_stage().unwrap_or("ax_presented_failed"));
        }
        result
    })();
    let restoration = scope.hide();
    original.sample();
    restoration?;
    if runtime.failed() {
        return Err(runtime.failure_stage().unwrap_or("ax_hidden_restore"));
    }
    outcome
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

fn admit_actual_frame(actual: NSRect, screen: NSRect) -> Result<NSRect, &'static str> {
    centered_frame(screen)?;
    if actual.size != NSSize::new(1280.0, 800.0) {
        return Err("presented_window_extent_changed");
    }
    let max_x = screen.origin.x + screen.size.width;
    let max_y = screen.origin.y + screen.size.height;
    let right = actual.origin.x + actual.size.width;
    let top = actual.origin.y + actual.size.height;
    if !actual.origin.x.is_finite()
        || !actual.origin.y.is_finite()
        || !max_x.is_finite()
        || !max_y.is_finite()
        || !right.is_finite()
        || !top.is_finite()
        || actual.origin.x < screen.origin.x
        || actual.origin.y < screen.origin.y
        || right > max_x
        || top > max_y
    {
        return Err("presented_window_outside_screen");
    }
    Ok(actual)
}

pub(super) fn measure(
    view: &AgentOwnedView,
    context: zephium_agentic::ContextJoin,
    operation: zephium_agentic::ContextOperationJoin,
    url: &str,
    runtime: &ProbeRuntime<'_, '_>,
    host: &NSView,
) -> PresentedOutcome {
    let ProbeNativeState::Hidden(original) = runtime.native_guard else {
        return Err("presented_original_guard".into());
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
        return Err("presented_admission".into());
    }
    let mtm = MainThreadMarker::new().ok_or("presented_main_thread")?;
    let screen = NSScreen::mainScreen(mtm).ok_or("presented_screen_unavailable")?;
    let visible_screen = screen.visibleFrame();
    let frame = centered_frame(visible_screen)?;
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
        original_page_frame: original.page.frame(),
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
        // Wry maps top-left coordinates using the original host height.
        // Resizing that host does not rebase the fixed, non-autoresizing child.
        // Preserve the exact viewport extent; restore this native origin on exit.
        scope.page.setFrame(viewport_frame());
        scope.page.setHidden(false);
        // Publicly documented to preserve key and main windows even when this
        // application is inactive. This is real presentation, never disguised.
        scope.window.orderFrontRegardless();
        // AppKit may choose a native screen-aligned origin. Admit only the
        // exact fixed extent fully on this screen, then freeze the actual frame.
        let presented_frame = admit_actual_frame(scope.window.frame(), visible_screen)?;
        let presented = ProbeNativeState::Presented(PresentedStateGuard {
            app: original.app,
            window: original.window,
            page: original.page,
            first_responder: original.first_responder,
            expected_window_frame: presented_frame,
            deadline,
            require_visible_pixels: Cell::new(false),
            exact_page_responder_observed: Cell::new(false),
            appkit_events_dispatched: Cell::new(0),
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
            guard.appkit_events_dispatched.get(),
            measurement,
        ))
    })();
    let restore = scope.hide();
    original.sample();
    let presented_elapsed_ms =
        u64::try_from(started.elapsed().as_millis()).map_err(|_| "presented_clock")?;
    let outcome = outcome
        .map(
            |(
                opportunity_elapsed_ms,
                exact_page_responder_observed,
                appkit_events_dispatched,
                measurement,
            )| MacosAgenticPresentedRenderingReport {
                presented_elapsed_ms,
                opportunity_elapsed_ms,
                exact_page_responder_observed,
                appkit_events_dispatched,
                measurement,
            },
        )
        .map_err(MacosAgenticPresentedRenderingFailure::from);
    if let Some(stage) = restore.err().or_else(|| runtime.failure_stage()) {
        return Err(MacosAgenticPresentedRenderingFailure::supersede(
            stage,
            Some(outcome),
        ));
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presented_launch_requires_exact_policy_no_activation_and_no_windows() {
        for expected in [
            NSApplicationActivationPolicy::Prohibited,
            NSApplicationActivationPolicy::Accessory,
        ] {
            assert!(launch_policy_valid(expected, expected, false, 0));
            assert!(!launch_policy_valid(expected, expected, true, 0));
            assert!(!launch_policy_valid(expected, expected, false, 1));
            assert!(!launch_policy_valid(expected, expected, false, usize::MAX));
            for wrong in [
                NSApplicationActivationPolicy::Regular,
                NSApplicationActivationPolicy(-1),
            ] {
                assert!(!launch_policy_valid(wrong, expected, false, 0));
            }
        }
        assert!(!launch_policy_valid(
            NSApplicationActivationPolicy::Accessory,
            NSApplicationActivationPolicy::Prohibited,
            false,
            0
        ));
        assert!(!launch_policy_valid(
            NSApplicationActivationPolicy::Prohibited,
            NSApplicationActivationPolicy::Accessory,
            false,
            0
        ));
    }

    #[test]
    fn presented_cleanup_refusal_preserves_provisional_measurement_without_success() {
        let report = MacosAgenticPresentedRenderingReport {
            presented_elapsed_ms: 5,
            opportunity_elapsed_ms: 1,
            exact_page_responder_observed: false,
            appkit_events_dispatched: 0,
            measurement: MacosAgenticRenderingProbeReport {
                samples: Vec::new(),
                disposition: rendering::RenderingDisposition::ControlsIncomplete,
            },
        };
        let failed = MacosAgenticPresentedRenderingFailure::supersede("restore", Some(Ok(report)));
        assert_eq!(failed.stage, "restore");
        assert_eq!(failed.prior_stage, None);
        assert!(failed.provisional_measurement.is_some());
        let failed =
            MacosAgenticPresentedRenderingFailure::supersede("teardown", Some(Err(failed)));
        assert_eq!(failed.stage, "teardown");
        assert_eq!(failed.prior_stage, Some("restore"));
        assert!(failed.provisional_measurement.is_some());
        let failed =
            MacosAgenticPresentedRenderingFailure::supersede("teardown", Some(Err(failed)));
        assert_eq!(failed.prior_stage, Some("restore"));
        assert!(failed.provisional_measurement.is_some());
        let failed =
            MacosAgenticPresentedRenderingFailure::supersede("restore", Some(Err("inner".into())));
        assert_eq!(failed.prior_stage, Some("inner"));
        assert!(failed.provisional_measurement.is_none());
    }

    #[test]
    fn presented_appkit_pump_excludes_input_unknown_types_and_capacity_overflow() {
        assert_eq!(
            NSEventMask::AppKitDefined.bits(),
            1 << NSEventType::AppKitDefined.0
        );
        for event_type in 0..=64 {
            assert_eq!(
                appkit_event_permitted(NSEventType(event_type), 0),
                event_type == NSEventType::AppKitDefined.0
            );
        }
        assert!(appkit_event_permitted(
            NSEventType::AppKitDefined,
            MAX_APPKIT_EVENTS - 1
        ));
        for count in [MAX_APPKIT_EVENTS, MAX_APPKIT_EVENTS + 1, usize::MAX] {
            assert!(!appkit_event_permitted(NSEventType::AppKitDefined, count));
        }
    }

    #[test]
    fn presented_opportunity_distinguishes_occlusion_from_parent_clipping() {
        assert_eq!(opportunity_refusal(true, true), None);
        assert_eq!(
            opportunity_refusal(false, true),
            Some("presented_window_occluded")
        );
        assert_eq!(
            opportunity_refusal(true, false),
            Some("presented_page_clipped")
        );
        assert_eq!(
            opportunity_refusal(false, false),
            Some("presented_window_occluded_and_page_clipped")
        );
        let old_child = NSRect::new(NSPoint::new(0.0, 560.0 - 800.0), viewport_frame().size);
        assert_ne!(old_child, viewport_frame());
        for changed in [
            old_child,
            NSRect::new(NSPoint::new(1.0, 0.0), viewport_frame().size),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 560.0)),
        ] {
            assert!(opportunity_refusal(true, changed == viewport_frame()).is_some());
        }
    }

    #[test]
    fn presented_evidence_requires_independent_focus_input_geometry_and_time_facts() {
        let valid = PresentedFacts {
            within_deadline: true,
            app_inactive: true,
            app_accessory: true,
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
            |f: &mut PresentedFacts| f.app_accessory = false,
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

    #[test]
    fn presented_actual_frame_allows_native_alignment_but_not_scaling_clipping_or_offscreen_position(
    ) {
        let screen = NSRect::new(NSPoint::new(-1440.0, 30.0), NSSize::new(1440.0, 901.0));
        let centered = centered_frame(screen).unwrap();
        let aligned = NSRect::new(NSPoint::new(-1360.0, 81.0), centered.size);
        assert_eq!(admit_actual_frame(aligned, screen).unwrap(), aligned);
        for size in [
            NSSize::new(1279.0, 800.0),
            NSSize::new(1280.0, 801.0),
            NSSize::new(f64::NAN, 800.0),
        ] {
            assert_eq!(
                admit_actual_frame(NSRect::new(aligned.origin, size), screen),
                Err("presented_window_extent_changed")
            );
        }
        for origin in [
            NSPoint::new(-1441.0, 30.0),
            NSPoint::new(-1440.0, 29.0),
            NSPoint::new(-1279.0, 30.0),
            NSPoint::new(-1440.0, 132.0),
            NSPoint::new(f64::NAN, 30.0),
            NSPoint::new(-1440.0, f64::INFINITY),
        ] {
            assert_eq!(
                admit_actual_frame(NSRect::new(origin, aligned.size), screen),
                Err("presented_window_outside_screen")
            );
        }
    }
}
