//! Shipping-private presentation mechanics for one retained-page observation.
//! The page is hosted inside the human window beneath its chrome: WebKit keeps
//! painting it, nothing appears on screen, and no input can reach it.
//! No diagnostic capability, application activation, input or page script.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use objc2::rc::{Retained, Weak};
use objc2_app_kit::{
    NSApplication, NSView, NSWindow, NSWindowOcclusionState, NSWindowOrderingMode,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use objc2_web_kit::WKWebView;
#[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
use std::rc::Rc;
use std::time::Instant;

#[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
use crate::WorkObservationPresentationFailure as PresentationFailure;

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

#[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
const PREPARE_VIEW_FAILURES: [PresentationFailure; 3] = [
    PresentationFailure::PreparePageNotHidden,
    PresentationFailure::PrepareFrameMismatch,
    PresentationFailure::PreparePageIsResponder,
];
#[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
const POLL_FAILURES: [PresentationFailure; 5] = [
    PresentationFailure::PollPageFrameMismatch,
    PresentationFailure::PollPageHidden,
    PresentationFailure::PollPageAlphaMismatch,
    PresentationFailure::PollPageWindowMismatch,
    PresentationFailure::PollPageIsResponder,
];
#[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
const RETIRE_FAILURES: [PresentationFailure; 4] = [
    PresentationFailure::RetirePageStillVisible,
    PresentationFailure::RetireHumanOwnershipChanged,
    PresentationFailure::RetireFrameMismatch,
    PresentationFailure::RetireParentMismatch,
];

#[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
fn classify_predicates<const N: usize>(
    facts: [bool; N],
    failures: [PresentationFailure; N],
) -> Option<PresentationFailure> {
    facts
        .into_iter()
        .zip(failures)
        .find_map(|(valid, failure)| (!valid).then_some(failure))
}

#[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
fn invoke_failure_diagnostic(
    diagnostic: &dyn Fn(PresentationFailure),
    failure: PresentationFailure,
) {
    // Diagnostics are observational only. A diagnostic consumer panic cannot
    // alter native ownership, presentation state or the semantic terminal.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| diagnostic(failure)));
}

/// The resource retains this owner before the first hierarchy mutation. The
/// host separately binds its exact lease, document and observation correlation.
pub(crate) struct WorkObservationPresentation {
    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
    failure_diagnostic: Rc<dyn Fn(PresentationFailure)>,
    app: Retained<NSApplication>,
    main: Retained<NSWindow>,
    page: Retained<WKWebView>,
    parent: Retained<NSView>,
    original_frame: NSRect,
    deadline: Instant,
    state: PresentationState,
    cleanup_failed: bool,
}

impl WorkObservationPresentation {
    #[cfg(feature = "native-agentic-work-resource-probe")]
    pub(crate) fn record_probe_weak(&self, resource: &zephium_agentic::WorkBrowserResourceJoin) {
        super::agentic_foreground_probe::record_resource_observation_weak(
            resource, &self.page, &self.main,
        );
    }
    pub(crate) fn prepare(
        view: &wry::WebView,
        deadline: Instant,
        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")] failure_diagnostic: impl Fn(PresentationFailure)
            + 'static,
    ) -> Result<Self, PresentationState> {
        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
        let failure_diagnostic: Rc<dyn Fn(PresentationFailure)> = Rc::new(failure_diagnostic);
        let Some(mtm) = MainThreadMarker::new() else {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            invoke_failure_diagnostic(
                failure_diagnostic.as_ref(),
                PresentationFailure::PrepareMainThread,
            );
            return Err(PresentationState::Failed);
        };
        let app = NSApplication::sharedApplication(mtm);
        let page = super::native_webview(view);
        let main = page.window().ok_or(PresentationState::Unavailable)?;
        if !window_present(
            &app,
            &main,
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            failure_diagnostic.as_ref(),
        ) {
            return Err(PresentationState::Unavailable);
        }
        // SAFETY: the exact native page is read on its owning main thread;
        // retain its original parent before any hierarchy effect.
        let Some(parent) = (unsafe { page.superview() }) else {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            invoke_failure_diagnostic(
                failure_diagnostic.as_ref(),
                PresentationFailure::PrepareMissingParent,
            );
            return Err(PresentationState::Failed);
        };
        let original_frame = page.frame();
        let resting_facts = [
            page.isHidden(),
            original_frame.size == viewport().size,
            !responder_inside(&main, &page),
        ];
        if !resting_facts.into_iter().all(|fact| fact) {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            if let Some(failure) = classify_predicates(resting_facts, PREPARE_VIEW_FAILURES) {
                invoke_failure_diagnostic(failure_diagnostic.as_ref(), failure);
            }
            return Err(PresentationState::Failed);
        }
        if Instant::now() >= deadline {
            return Err(PresentationState::Expired);
        }
        Ok(Self {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            failure_diagnostic,
            app,
            main,
            page,
            parent,
            original_frame,
            deadline,
            state: PresentationState::Prepared,
            cleanup_failed: false,
        })
    }

    pub(crate) fn present(&mut self) -> PresentationState {
        if self.state == PresentationState::Ready {
            return self.poll();
        }
        if self.state != PresentationState::Prepared {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            invoke_failure_diagnostic(
                self.failure_diagnostic.as_ref(),
                PresentationFailure::PresentInvalidState,
            );
            self.state = PresentationState::Failed;
            return self.state;
        }
        if !window_present(
            &self.app,
            &self.main,
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            self.failure_diagnostic.as_ref(),
        ) {
            self.state = PresentationState::Unavailable;
            return self.state;
        }
        if Instant::now() >= self.deadline {
            self.state = PresentationState::Expired;
            return self.state;
        }
        // A page that requests focus must never take the keyboard from the
        // human window: the window refuses it for the presentation lifetime.
        if !super::passive_page::register(&self.page) {
            self.state = PresentationState::Failed;
            return self.state;
        }
        self.state = PresentationState::Acquiring;
        // Re-adding beneath every sibling keeps the page under the chrome and
        // the vibrancy backdrop, so hit-testing never reaches it.
        self.parent
            .addSubview_positioned_relativeTo(&self.page, NSWindowOrderingMode::Below, None);
        self.page.setFrame(viewport());
        self.page.setHidden(false);
        self.poll()
    }

    pub(crate) fn renew(&mut self, deadline: Instant) -> bool {
        if !matches!(
            self.state,
            PresentationState::Ready | PresentationState::Acquiring
        ) || !self.human_current()
            || Instant::now() >= deadline
        {
            return false;
        }
        self.deadline = deadline;
        matches!(
            self.poll(),
            PresentationState::Ready | PresentationState::Acquiring
        )
    }

    pub(crate) fn poll(&mut self) -> PresentationState {
        if self.state == PresentationState::Retiring {
            self.state = if self.cleanup_failed {
                PresentationState::Failed
            } else {
                PresentationState::Retired
            };
            return self.state;
        }
        if !matches!(
            self.state,
            PresentationState::Acquiring | PresentationState::Ready
        ) {
            return self.state;
        }
        if !window_present(
            &self.app,
            &self.main,
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            self.failure_diagnostic.as_ref(),
        ) {
            self.state = PresentationState::Unavailable;
            return self.state;
        }
        if Instant::now() >= self.deadline {
            self.state = PresentationState::Expired;
            return self.state;
        }
        let poll_facts = [
            self.page.frame() == viewport(),
            !self.page.isHiddenOrHasHiddenAncestor(),
            self.page.alphaValue() == 1.0,
            self.page
                .window()
                .is_some_and(|window| std::ptr::eq(&*window, &*self.main)),
            !responder_inside(&self.main, &self.page),
        ];
        let exact = poll_facts.into_iter().all(|fact| fact);
        // A hidden or minimized window pauses the page (Acquiring) until it is
        // shown again; the deadline bounds the pause. Only a broken hierarchy fails.
        let visible = !self.main.isMiniaturized()
            && self
                .main
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible)
            && !self.page.visibleRect().is_empty();
        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
        if let Some(failure) = classify_predicates(poll_facts, POLL_FAILURES) {
            invoke_failure_diagnostic(self.failure_diagnostic.as_ref(), failure);
        }
        self.state = if !exact {
            PresentationState::Failed
        } else if visible {
            PresentationState::Ready
        } else {
            PresentationState::Acquiring
        };
        self.state
    }

    /// Hide before restoring hierarchy. Never repair or reacquire human focus.
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
        self.page.setFrame(self.original_frame);
        super::passive_page::unregister(&self.page);
        // SAFETY: the retained native page and original parent remain alive on
        // the main thread. No pointer escapes this identity comparison.
        let original_parent = unsafe { self.page.superview() }
            .is_some_and(|parent| std::ptr::eq(&*parent, &*self.parent));
        let human_ownership_changed = human_owners(&self.app) != before;
        let retirement_facts = [
            self.page.isHidden(),
            !human_ownership_changed,
            self.page.frame() == self.original_frame,
            original_parent,
        ];
        let cleanup_failed = !retirement_facts.into_iter().all(|fact| fact);
        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
        if !self.cleanup_failed && cleanup_failed {
            if let Some(failure) = classify_predicates(retirement_facts, RETIRE_FAILURES) {
                invoke_failure_diagnostic(self.failure_diagnostic.as_ref(), failure);
            }
        }
        self.cleanup_failed |= cleanup_failed;
        self.poll()
    }

    pub(crate) fn visible_for_audit(&self) -> bool {
        !self.page.isHidden()
    }
    pub(crate) fn human_current(&self) -> bool {
        let present = window_present(
            &self.app,
            &self.main,
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            self.failure_diagnostic.as_ref(),
        );
        present
            && match self.state {
                PresentationState::Prepared
                | PresentationState::Retiring
                | PresentationState::Retired => true,
                PresentationState::Acquiring | PresentationState::Ready => {
                    hosted_current(&self.page, &self.main)
                }
                PresentationState::Unavailable
                | PresentationState::Expired
                | PresentationState::Failed => false,
            }
    }

    /// Fixed native ownership fence retained until the runtime hands the action
    /// to the page. It confers no activation, focus, or pointer authority.
    pub(crate) fn human_fence(&self) -> Box<dyn Fn() -> bool> {
        let app = self.app.clone();
        let main = self.main.clone();
        // A fence observes these owners; it must not prolong their native
        // retirement while the semantic channel drains its original callback.
        let page = Weak::from_retained(&self.page);
        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
        let failure_diagnostic = self.failure_diagnostic.clone();
        Box::new(move || {
            window_present(
                &app,
                &main,
                #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                failure_diagnostic.as_ref(),
            ) && page.load().is_some_and(|page| hosted_current(&page, &main))
        })
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

/// The human window must still exist, shown or minimized. Activation, key
/// status and the responder are not required: the page sits beneath the chrome,
/// so no input reaches it whichever window the human is using.
fn window_present(
    app: &NSApplication,
    main: &NSWindow,
    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")] diagnostic: &dyn Fn(
        PresentationFailure,
    ),
) -> bool {
    let facts = [main.isVisible(), main.isMiniaturized()];
    let present = present_facts(facts);
    #[cfg(not(feature = "native-agentic-work-lifetime-diagnostic"))]
    let _ = app;
    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
    if !present {
        invoke_failure_diagnostic(
            diagnostic,
            PresentationFailure::HumanOwnership {
                app_active: app.isActive(),
                main_visible: facts[0],
                main_not_minimized: !facts[1],
                key_window_matches: app
                    .keyWindow()
                    .is_some_and(|window| std::ptr::eq(&*window, main)),
                main_window_matches: app
                    .mainWindow()
                    .is_some_and(|window| std::ptr::eq(&*window, main)),
                responder_matches: true,
            },
        );
    }
    present
}

const fn present_facts([visible, miniaturized]: [bool; 2]) -> bool {
    visible || miniaturized
}

/// Tolerate a changed human responder, never routing the human window's
/// keyboard responder into the retained page or one of its native views.
fn responder_inside(main: &NSWindow, page: &WKWebView) -> bool {
    main.firstResponder().is_some_and(|responder| {
        Retained::as_ptr(&responder).addr() == std::ptr::from_ref(page).addr()
            || responder
                .downcast::<NSView>()
                .is_ok_and(|view| view.isDescendantOf(page))
    })
}
// The page's exact native attachment and input exclusion remain independent
// fences alongside controller lease/document revocation. No focus is acquired.
fn hosted_current(page: &WKWebView, main: &NSWindow) -> bool {
    !page.isHiddenOrHasHiddenAncestor()
        && page
            .window()
            .is_some_and(|window| std::ptr::eq(&*window, main))
        && !responder_inside(main, page)
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
    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
    #[test]
    fn every_failed_presentation_predicate_has_one_exact_content_free_cause() {
        fn assert_matrix<const N: usize>(failures: [PresentationFailure; N]) {
            assert_eq!(classify_predicates([true; N], failures), None);
            for (index, expected) in failures.into_iter().enumerate() {
                let mut facts = [true; N];
                facts[index] = false;
                assert_eq!(classify_predicates(facts, failures), Some(expected));
            }
        }
        assert_matrix(PREPARE_VIEW_FAILURES);
        assert_matrix(POLL_FAILURES);
        assert_matrix(RETIRE_FAILURES);

        let trace = format!("{:?}", PresentationFailure::PollPageWindowMismatch);
        for forbidden in ["http", "/", "0x", "NSPoint", "NSRect"] {
            assert!(!trace.contains(forbidden));
        }
    }

    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
    #[test]
    fn diagnostic_consumer_panic_cannot_cross_the_presentation_boundary() {
        invoke_failure_diagnostic(
            &|_| panic!("contained diagnostic panic"),
            PresentationFailure::PresentInvalidState,
        );
    }

    #[test]
    fn a_shown_or_minimized_window_keeps_the_page_without_repairing_focus() {
        assert!(present_facts([true, false]));
        assert!(present_facts([false, true]));
        assert!(!present_facts([false, false]));
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
            "NSWindow::alloc",
            "orderFront",
        ] {
            assert!(!source.contains(forbidden));
        }
        assert!(source.contains("NSWindowOrderingMode::Below"));
        assert!(source.contains("passive_page::register(&self.page)"));
        assert!(source.contains("self.cleanup_failed |="));
        assert!(source.contains("human_owners(&self.app) != before"));
    }

    #[test]
    fn deactivation_and_key_loss_never_end_a_hosted_page() {
        let source = include_str!("work_observation_presentation.rs")
            .split("\n#[cfg(test)]")
            .next()
            .unwrap();
        let gates = source.split("fn window_present(").next().unwrap();
        for forbidden in [
            "isActive()",
            "keyWindow()",
            "mainWindow()",
            "firstResponder()",
        ] {
            assert!(!gates.contains(forbidden), "{forbidden}");
        }
    }
}
