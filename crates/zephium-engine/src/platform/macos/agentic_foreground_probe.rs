//! Release-excluded rendering ownership in the real foreground application.
//! No application bootstrap, event pump, provider or page-authored capability.

#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use std::time::{Duration, Instant};

use objc2::rc::{Retained, Weak};
use objc2::MainThreadOnly as _;
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSResponder, NSView, NSWindow, NSWindowOcclusionState,
    NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSAlignmentOptions, NSPoint, NSRect, NSSize};
use objc2_web_kit::{WKWebView, WKWebsiteDataStore};
use zephium_agentic::{ContextJoin, ForegroundRenderingState};

const RENDERING_BUDGET: Duration = Duration::from_secs(5);

/// Closed, content-free location of a refused native attestation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForegroundFailurePhase {
    Host,
    Prepare,
    Present,
    Poll,
    Cleanup,
}

/// Only fixed host predicates: never native addresses, page content or geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForegroundFailurePredicate {
    MainThread,
    MainWindow,
    OriginalParent,
    HiddenPage,
    FixedViewport,
    HumanResponderNotPage,
    Screen,
    ScreenViewportFit,
    BackingAlignedFrame,
    Deadline,
    ContextJoin,
    PreparedState,
    SurfaceOwner,
    HiddenSurface,
    NoKeyCapability,
    NoMainCapability,
    SurfaceAlpha,
    ContentView,
    OnScreenFrame,
    StableSurfaceFrame,
    VisibleSurface,
    VisibleHierarchy,
    NoKeyOwnership,
    NoMainOwnership,
    IgnoreMouse,
    OpaqueSurface,
    PageAlpha,
    ExactPageWindow,
    HumanOwnersUnchanged,
    ExactRestoredFrame,
    ExactRestoredParent,
    PriorCleanupFailure,
    WatchdogScheduled,
    LeaseOwner,
    HostDispatch,
    ContextBinding,
    AcquisitionAvailable,
    SoleCohort,
    RendererLive,
    EphemeralProfile,
    NoPendingNavigation,
    NoPendingRecovery,
    NoPendingScreenshot,
    NoSemanticInvocation,
    NoSemanticSnapshot,
    SemanticIdle,
    FixedFixture,
    ObserveCapability,
    NavigateCapability,
    ExactCapabilities,
    ExactDocument,
}

/// One exact failed predicate, without a native handle or caller-controlled text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForegroundNativeFailure {
    pub phase: ForegroundFailurePhase,
    pub predicate: ForegroundFailurePredicate,
}

/// Bounded first-cause evidence. Cleanup cannot replace a primary refusal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForegroundNativeFailures {
    pub primary: Option<ForegroundNativeFailure>,
    pub cleanup: Option<ForegroundNativeFailure>,
}

/// Closed diagnostic owner identity. Presentation belongs to the exact page,
/// never to a fabricated actor used to correlate the qualification run.
#[derive(Clone, Eq, PartialEq)]
enum ForegroundOwner {
    Legacy(ContextJoin),
    #[cfg(feature = "native-agentic-work-resource-probe")]
    Resource(zephium_agentic::WorkBrowserResourceJoin),
}
impl From<ContextJoin> for ForegroundOwner {
    fn from(context: ContextJoin) -> Self {
        Self::Legacy(context)
    }
}
impl From<&ForegroundOwner> for ForegroundOwner {
    fn from(owner: &ForegroundOwner) -> Self {
        owner.clone()
    }
}

struct NativeFailureTrace {
    context: ForegroundOwner,
    failures: ForegroundNativeFailures,
}

impl NativeFailureTrace {
    fn evidence(&self, context: impl Into<ForegroundOwner>) -> Option<ForegroundNativeFailures> {
        (self.context == context.into()).then_some(self.failures)
    }

    fn record(
        &mut self,
        context: impl Into<ForegroundOwner>,
        failure: ForegroundNativeFailure,
    ) -> bool {
        if context.into() != self.context {
            return false;
        }
        let slot = if failure.phase == ForegroundFailurePhase::Cleanup {
            &mut self.failures.cleanup
        } else {
            &mut self.failures.primary
        };
        if slot.is_none() {
            *slot = Some(failure);
        }
        true
    }
}

thread_local! { static FAILURE_TRACE: std::cell::RefCell<Option<NativeFailureTrace>> = const { std::cell::RefCell::new(None) }; }

pub(crate) fn native_failure_evidence(context: ContextJoin) -> Option<ForegroundNativeFailures> {
    owner_failure_evidence(&context.into())
}

fn owner_failure_evidence(context: &ForegroundOwner) -> Option<ForegroundNativeFailures> {
    FAILURE_TRACE.with(|slot| {
        slot.try_borrow()
            .ok()
            .and_then(|trace| trace.as_ref().and_then(|trace| trace.evidence(context)))
    })
}

fn begin_failure_trace(context: impl Into<ForegroundOwner>) {
    let context = context.into();
    FAILURE_TRACE.with(|slot| {
        if let Ok(mut trace) = slot.try_borrow_mut() {
            if trace.is_none() {
                *trace = Some(NativeFailureTrace {
                    context,
                    failures: ForegroundNativeFailures::default(),
                });
            }
        }
    });
}

fn failed(
    context: impl Into<ForegroundOwner>,
    phase: ForegroundFailurePhase,
    predicate: ForegroundFailurePredicate,
) -> ForegroundRenderingState {
    FAILURE_TRACE.with(|slot| {
        if let Ok(mut trace) = slot.try_borrow_mut() {
            if let Some(trace) = trace.as_mut() {
                trace.record(context, ForegroundNativeFailure { phase, predicate });
            }
        }
    });
    ForegroundRenderingState::Failed
}

// Preserve the original native checks' order and short-circuit behavior.
macro_rules! first_failed_predicate {
    ($($predicate:expr => $admitted:expr),+ $(,)?) => {
        None::<ForegroundFailurePredicate>$(.or_else(|| (!$admitted).then_some($predicate)))+
    };
}

struct WeakNativeWitness {
    context: ForegroundOwner,
    page: Weak<WKWebView>,
    surface: Weak<NSWindow>,
    store: Weak<WKWebsiteDataStore>,
}
thread_local! { static WEAK_WITNESS: std::cell::RefCell<Option<WeakNativeWitness>> = const { std::cell::RefCell::new(None) }; }

/// Weak observations own no native resource. The normal application shutdown
/// must release the cached ephemeral store before the final drain can pass.
pub(crate) fn native_witness_drained(context: ContextJoin) -> Option<bool> {
    owner_witness_drained(&context.into())
}

fn owner_witness_drained(context: &ForegroundOwner) -> Option<bool> {
    MainThreadMarker::new()?;
    WEAK_WITNESS.with(|slot| {
        slot.try_borrow().ok().and_then(|witness| {
            witness
                .as_ref()
                .filter(|witness| &witness.context == context)
                .map(|witness| {
                    witness.page.load().is_none()
                        && witness.surface.load().is_none()
                        && witness.store.load().is_none()
                })
        })
    })
}

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
            integral_origin(screen.origin.x, screen.size.width, 1280.0)?,
            integral_origin(screen.origin.y, screen.size.height, 800.0)?,
        ),
        viewport().size,
    );
    admitted_frame(frame, screen).then_some(frame)
}

// A fractional-point origin can make AppKit expand a borderless window when
// normalizing its edges. Choose an integral placement without changing size or
// clipping a fractional/negative visible-frame boundary. No fitting placement
// means refusal, not rounding the viewport or moving it outside that boundary.
fn integral_origin(origin: f64, available: f64, required: f64) -> Option<f64> {
    let first = origin.ceil();
    let last = (origin + (available - required)).floor();
    let center = (origin + (available - required) / 2.0).floor();
    if ![first, last, center].into_iter().all(f64::is_finite) || first > last {
        return None;
    }
    Some(center.clamp(first, last))
}

// NSScreen maps global screen points to backing pixels. Never accept its
// normalized rectangle as a replacement: it must attest the immutable plan.
fn backing_frame_admitted(frame: NSRect, screen: NSRect, scale: f64, aligned: NSRect) -> bool {
    admitted_frame(frame, screen)
        && scale.is_finite()
        && scale > 0.0
        && (frame.size.width * scale).is_finite()
        && (frame.size.height * scale).is_finite()
        && aligned == frame
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
        && (frame.origin.x + frame.size.width) - frame.origin.x == frame.size.width
        && (frame.origin.y + frame.size.height) - frame.origin.y == frame.size.height
        && screen.size.width >= 1280.0
        && screen.size.height >= 800.0
        && frame.origin.x >= screen.origin.x
        && frame.origin.y >= screen.origin.y
        && frame.origin.x + frame.size.width <= screen.origin.x + screen.size.width
        && frame.origin.y + frame.size.height <= screen.origin.y + screen.size.height
}

/// Retained inside the exact legacy context or Work resource, never a parallel view.
pub(crate) struct ForegroundRenderingLease {
    context: ForegroundOwner,
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
    #[cfg(feature = "native-agentic-work-resource-probe")]
    pub(crate) fn prepare_resource(
        resource: &zephium_agentic::WorkBrowserResourceJoin,
        view: &wry::WebView,
        deadline: Instant,
    ) -> Result<Self, ForegroundRenderingState> {
        Self::prepare_owner(ForegroundOwner::Resource(resource.clone()), view, deadline)
    }
    #[cfg(feature = "native-agentic-work-resource-probe")]
    pub(crate) fn present_resource(
        &mut self,
        resource: &zephium_agentic::WorkBrowserResourceJoin,
    ) -> ForegroundRenderingState {
        self.present_owner(&ForegroundOwner::Resource(resource.clone()))
    }
    #[cfg(feature = "native-agentic-work-resource-probe")]
    pub(crate) fn guard_resource(
        &mut self,
        resource: &zephium_agentic::WorkBrowserResourceJoin,
    ) -> ForegroundRenderingState {
        self.guard_owner(&ForegroundOwner::Resource(resource.clone()))
    }
    pub(crate) fn begin_attempt(context: ContextJoin) {
        begin_failure_trace(context);
    }

    pub(crate) fn admission_deadline(now: Instant) -> Option<Instant> {
        now.checked_add(RENDERING_BUDGET)
    }

    pub(crate) fn host_failed(
        context: ContextJoin,
        predicate: ForegroundFailurePredicate,
    ) -> ForegroundRenderingState {
        failed(context, ForegroundFailurePhase::Host, predicate)
    }

    /// Prepare a hidden auxiliary container; the host must retain this owner
    /// before calling present. No page or human focus changes at preparation.
    pub(crate) fn prepare(
        context: ContextJoin,
        view: &wry::WebView,
        deadline: Instant,
    ) -> Result<Self, ForegroundRenderingState> {
        Self::prepare_owner(context.into(), view, deadline)
    }

    fn prepare_owner(
        owner: ForegroundOwner,
        view: &wry::WebView,
        deadline: Instant,
    ) -> Result<Self, ForegroundRenderingState> {
        let context = &owner;
        use ForegroundFailurePhase::Prepare;
        use ForegroundFailurePredicate as P;
        begin_failure_trace(context);
        let mtm = MainThreadMarker::new().ok_or_else(|| failed(context, Prepare, P::MainThread))?;
        let app = NSApplication::sharedApplication(mtm);
        let page = super::native_webview(view);
        let main = page
            .window()
            .ok_or_else(|| failed(context, Prepare, P::MainWindow))?;
        let responder = main
            .firstResponder()
            .ok_or(ForegroundRenderingState::DeferredForeground)?;
        if !foreground(&app, &main, &responder).admitted() {
            return Err(ForegroundRenderingState::DeferredForeground);
        }
        // SAFETY: the exact retained native page is read on the main thread;
        // the returned parent is retained before any hierarchy mutation.
        let original_parent = unsafe { page.superview() }
            .ok_or_else(|| failed(context, Prepare, P::OriginalParent))?;
        let original_frame = page.frame();
        if let Some(predicate) = first_failed_predicate!(
            P::HiddenPage => page.isHidden(),
            P::FixedViewport => original_frame.size == viewport().size,
            P::HumanResponderNotPage => Retained::as_ptr(&responder).addr() != Retained::as_ptr(&page).addr(),
        ) {
            return Err(failed(context, Prepare, predicate));
        }
        let native_screen = main
            .screen()
            .ok_or_else(|| failed(context, Prepare, P::Screen))?;
        let screen = native_screen.visibleFrame();
        let frame =
            surface_frame(screen).ok_or_else(|| failed(context, Prepare, P::ScreenViewportFit))?;
        if !backing_frame_admitted(
            frame,
            screen,
            native_screen.backingScaleFactor(),
            native_screen
                .backingAlignedRect_options(frame, NSAlignmentOptions::AlignAllEdgesNearest),
        ) {
            return Err(failed(context, Prepare, P::BackingAlignedFrame));
        }
        if Instant::now() >= deadline {
            return Err(ForegroundRenderingState::Expired);
        }
        // SAFETY: the main-thread marker owns AppKit allocation; all frame
        // components are finite and the retained window remains Rust-owned.
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
        // SAFETY: exact retained page/configuration are read on the main thread.
        let store = unsafe { page.configuration().websiteDataStore() };
        WEAK_WITNESS.with(|slot| {
            if let Ok(mut slot) = slot.try_borrow_mut() {
                *slot = Some(WeakNativeWitness {
                    context: context.clone(),
                    page: Weak::from_retained(&page),
                    surface: Weak::from_retained(&surface),
                    store: Weak::from_retained(&store),
                });
            }
        });
        Ok(Self {
            context: context.clone(),
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
        self.present_owner(&context.into())
    }

    fn present_owner(&mut self, context: &ForegroundOwner) -> ForegroundRenderingState {
        use ForegroundFailurePhase::Present;
        use ForegroundFailurePredicate as P;
        if let Some(predicate) = first_failed_predicate!(
            P::ContextJoin => &self.context == context,
            P::PreparedState => self.state == ForegroundRenderingState::Prepared,
        ) {
            return self.refuse(Present, predicate);
        }
        if !foreground(&self.app, &self.main, &self.responder).admitted() {
            self.state = ForegroundRenderingState::DeferredForeground;
            return self.state;
        }
        let Some(surface) = self.surface.as_ref() else {
            return self.refuse(Present, P::SurfaceOwner);
        };
        // This fallible attestation occurs only after the host retains the
        // hidden native owner, so every refusal can prove exact retirement.
        if let Some(predicate) = first_failed_predicate!(
            P::HiddenSurface => !surface.isVisible(),
            P::StableSurfaceFrame => surface.frame() == self.frame,
            P::NoKeyCapability => !surface.canBecomeKeyWindow(),
            P::NoMainCapability => !surface.canBecomeMainWindow(),
            P::SurfaceAlpha => surface.alphaValue() == 1.0,
        ) {
            return self.refuse(Present, predicate);
        }
        let Some(parent) = surface.contentView() else {
            return self.refuse(Present, P::ContentView);
        };
        if Instant::now() >= self.deadline {
            self.state = ForegroundRenderingState::Expired;
            return self.state;
        }
        // Publish the effect phase before native hierarchy/presentation calls.
        self.state = ForegroundRenderingState::Acquiring;
        parent.addSubview(&self.page);
        self.page.setFrame(viewport());
        self.page.setHidden(false);
        surface.orderFrontRegardless();
        self.poll_owner(context)
    }

    fn poll_owner(&mut self, context: &ForegroundOwner) -> ForegroundRenderingState {
        use ForegroundFailurePhase::Poll;
        use ForegroundFailurePredicate as P;
        if &self.context != context {
            return self.refuse(Poll, P::ContextJoin);
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
            if let Some(predicate) = first_failed_predicate!(
                P::OnScreenFrame => admitted_frame(self.frame, self.screen),
                P::StableSurfaceFrame => surface.frame() == self.frame,
                P::FixedViewport => self.page.frame() == viewport(),
                P::VisibleSurface => surface.isVisible(),
                P::VisibleHierarchy => !self.page.isHiddenOrHasHiddenAncestor(),
                P::NoKeyOwnership => !surface.isKeyWindow(),
                P::NoMainOwnership => !surface.isMainWindow(),
                P::NoKeyCapability => !surface.canBecomeKeyWindow(),
                P::NoMainCapability => !surface.canBecomeMainWindow(),
                P::IgnoreMouse => surface.ignoresMouseEvents(),
                P::OpaqueSurface => surface.isOpaque(),
                P::SurfaceAlpha => surface.alphaValue() == 1.0,
                P::PageAlpha => self.page.alphaValue() == 1.0,
                P::ExactPageWindow => self.page.window().is_some_and(|window| std::ptr::eq(&*window, &**surface)),
            ) {
                return self.refuse(Poll, predicate);
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
            return self.refuse(Poll, P::SurfaceOwner);
        }
        self.state
    }

    /// Hides before restoring the exact parent/frame. It never restores human
    /// focus: a changed human foreground owner remains the user's authority.
    pub(crate) fn retire(&mut self) -> ForegroundRenderingState {
        use ForegroundFailurePhase::Cleanup;
        use ForegroundFailurePredicate as P;
        if !self.cleanup_failed
            && matches!(
                self.state,
                ForegroundRenderingState::Retiring | ForegroundRenderingState::Retired
            )
        {
            return self.poll_owner(&self.context.clone());
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
        if let Some(predicate) = first_failed_predicate!(
            P::HiddenPage => self.page.isHidden(),
            P::HumanOwnersUnchanged => human_owners(&self.app) == human_before,
            P::ExactRestoredFrame => self.page.frame() == self.original_frame,
            // SAFETY: the retained page/parent are main-thread-owned and the
            // exact returned parent is compared without escaping its lifetime.
            P::ExactRestoredParent => unsafe { self.page.superview() }
                .is_some_and(|parent| std::ptr::eq(&*parent, &*self.original_parent)),
        ) {
            self.cleanup_failed = true;
            return self.refuse(Cleanup, predicate);
        }
        if self.cleanup_failed {
            return self.refuse(Cleanup, P::PriorCleanupFailure);
        }
        self.poll_owner(&self.context.clone())
    }

    /// Revalidate before each native step/completion. Revocation hides in the
    /// same main-thread step, while preserving the truthful reason to caller.
    pub(crate) fn guard(&mut self, context: ContextJoin) -> ForegroundRenderingState {
        self.guard_owner(&context.into())
    }

    fn guard_owner(&mut self, context: &ForegroundOwner) -> ForegroundRenderingState {
        let state = self.revocation.unwrap_or_else(|| self.poll_owner(context));
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

    fn refuse(
        &mut self,
        phase: ForegroundFailurePhase,
        predicate: ForegroundFailurePredicate,
    ) -> ForegroundRenderingState {
        self.state = failed(&self.context, phase, predicate);
        self.state
    }

    pub(crate) fn watchdog_failed(context: ContextJoin) -> ForegroundRenderingState {
        failed(
            context,
            ForegroundFailurePhase::Host,
            ForegroundFailurePredicate::WatchdogScheduled,
        )
    }

    pub(crate) fn owner_unavailable(context: ContextJoin) -> ForegroundRenderingState {
        failed(
            context,
            ForegroundFailurePhase::Host,
            ForegroundFailurePredicate::LeaseOwner,
        )
    }
}

#[cfg(feature = "native-agentic-work-resource-probe")]
pub(crate) fn resource_native_drain(
    resource: &zephium_agentic::WorkBrowserResourceJoin,
) -> Option<bool> {
    owner_witness_drained(&ForegroundOwner::Resource(resource.clone()))
}
/// Weak-only diagnostic sampling of the shipping observation's real owners.
/// This function grants no presentation, read or resource capability.
#[cfg(feature = "native-agentic-work-resource-probe")]
pub(crate) fn record_resource_observation_weak(
    resource: &zephium_agentic::WorkBrowserResourceJoin,
    page: &Retained<WKWebView>,
    surface: &Retained<NSWindow>,
) {
    // SAFETY: exact retained main-thread page/configuration, as in the original
    // diagnostic witness; only weak observations survive this call.
    let store = unsafe { page.configuration().websiteDataStore() };
    WEAK_WITNESS.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            *slot = Some(WeakNativeWitness {
                context: ForegroundOwner::Resource(resource.clone()),
                page: Weak::from_retained(page),
                surface: Weak::from_retained(surface),
                store: Weak::from_retained(&store),
            });
        }
    });
}
#[cfg(feature = "native-agentic-work-resource-probe")]
pub(crate) fn resource_native_failures(
    resource: &zephium_agentic::WorkBrowserResourceJoin,
) -> Option<ForegroundNativeFailures> {
    owner_failure_evidence(&ForegroundOwner::Resource(resource.clone()))
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

/// Captured before Work construction, so construction itself cannot silently
/// redefine the human ownership baseline later used by the rendering lease.
pub(crate) struct HumanForegroundGuard {
    app: Retained<NSApplication>,
    main: Retained<NSWindow>,
    responder: Retained<NSResponder>,
}

impl HumanForegroundGuard {
    pub(crate) fn capture_exact(expected_main: &NSWindow) -> Option<Self> {
        let app = NSApplication::sharedApplication(MainThreadMarker::new()?);
        let main = app.mainWindow()?;
        let responder = main.firstResponder()?;
        exact_foreground_admission(expected_main, &*main, foreground(&app, &main, &responder))
            .then_some(Self {
                app,
                main,
                responder,
            })
    }

    pub(crate) fn is_current(&self) -> bool {
        foreground(&self.app, &self.main, &self.responder).admitted()
    }
}

fn exact_foreground_admission(
    expected: *const NSWindow,
    observed: *const NSWindow,
    facts: ForegroundFacts,
) -> bool {
    !expected.is_null() && expected == observed && facts.admitted()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "native-agentic-work-resource-probe")]
    #[test]
    fn resource_failure_trace_cannot_be_rebound_to_legacy_or_recreated_identity() {
        use zephium_agentic::*;
        let (_, identity, legacy) = failure_context();
        let work = WorkId::generate();
        let id = WorkBrowserResourceId::generate();
        let resource = || {
            WorkBrowserResources::new(work, identity.profile())
                .construct(
                    id,
                    identity.id(),
                    ContextProfileStorageClass::Ephemeral,
                    AgentPolicyInstant::from_millis(0),
                )
                .unwrap()
                .resource()
                .clone()
        };
        let original = ForegroundOwner::Resource(resource());
        let recreated = ForegroundOwner::Resource(resource());
        let mut trace = NativeFailureTrace {
            context: original.clone(),
            failures: ForegroundNativeFailures::default(),
        };
        let failure = ForegroundNativeFailure {
            phase: ForegroundFailurePhase::Poll,
            predicate: ForegroundFailurePredicate::ExactPageWindow,
        };
        assert!(!trace.record(legacy, failure));
        assert!(!trace.record(&recreated, failure));
        assert!(trace.evidence(legacy).is_none());
        assert!(trace.evidence(&recreated).is_none());
        assert!(trace.record(&original, failure));
        assert_eq!(trace.evidence(&original).unwrap().primary, Some(failure));
    }

    fn failure_context() -> (
        zephium_agentic::ContextRegistry,
        zephium_agentic::ContextIdentity,
        ContextJoin,
    ) {
        use zephium_agentic::*;
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            AgentWorkProfileId::generate(),
            ContextKind::Owned,
        );
        let mut registry = ContextRegistry::new();
        registry
            .reserve(
                identity,
                ContextCapabilities::try_new(
                    ContextKind::Owned,
                    &[ContextCapability::Observe, ContextCapability::Navigate],
                )
                .unwrap(),
            )
            .unwrap();
        let operation = registry
            .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
            .unwrap();
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .unwrap();
        let context = registry.join(identity.id()).unwrap();
        (registry, identity, context)
    }

    #[test]
    fn native_failure_evidence_cannot_cross_context_or_document_identity() {
        let (mut registry, identity, context) = failure_context();
        let mut trace = NativeFailureTrace {
            context: context.into(),
            failures: ForegroundNativeFailures::default(),
        };
        let failure = ForegroundNativeFailure {
            phase: ForegroundFailurePhase::Present,
            predicate: ForegroundFailurePredicate::NoKeyCapability,
        };
        let (_, _, other) = failure_context();
        let successor = registry
            .begin_navigation(
                identity.id(),
                zephium_agentic::ContextOperationId::new(2).unwrap(),
            )
            .unwrap()
            .context();
        for wrong in [other, successor] {
            assert!(!trace.record(wrong, failure));
            assert_eq!(trace.evidence(wrong), None);
        }
        assert_eq!(
            trace.evidence(context),
            Some(ForegroundNativeFailures::default())
        );
        assert!(trace.record(context, failure));
        assert_eq!(trace.evidence(context).unwrap().primary, Some(failure));
    }

    #[test]
    fn native_failure_first_cause_and_cleanup_are_independently_sticky() {
        let (_, _, context) = failure_context();
        let mut trace = NativeFailureTrace {
            context: context.into(),
            failures: ForegroundNativeFailures::default(),
        };
        let primary = ForegroundNativeFailure {
            phase: ForegroundFailurePhase::Present,
            predicate: ForegroundFailurePredicate::NoMainCapability,
        };
        let cleanup = ForegroundNativeFailure {
            phase: ForegroundFailurePhase::Cleanup,
            predicate: ForegroundFailurePredicate::ExactRestoredParent,
        };
        assert!(trace.record(context, primary));
        assert!(trace.record(
            context,
            ForegroundNativeFailure {
                phase: ForegroundFailurePhase::Poll,
                predicate: ForegroundFailurePredicate::FixedViewport
            }
        ));
        assert!(trace.record(context, cleanup));
        assert!(trace.record(
            context,
            ForegroundNativeFailure {
                phase: ForegroundFailurePhase::Cleanup,
                predicate: ForegroundFailurePredicate::PriorCleanupFailure
            }
        ));
        assert_eq!(
            trace.evidence(context),
            Some(ForegroundNativeFailures {
                primary: Some(primary),
                cleanup: Some(cleanup)
            })
        );
    }

    #[test]
    fn native_failure_predicates_preserve_order_and_short_circuit_without_changing_acceptance() {
        use ForegroundFailurePredicate as P;
        for first_false in 0..=3 {
            let checked = std::cell::Cell::new(0);
            let allowed = |index| {
                checked.set(checked.get() + 1);
                index != first_false
            };
            let failed = first_failed_predicate!(
                P::HiddenSurface => allowed(0),
                P::NoKeyCapability => allowed(1),
                P::NoMainCapability => allowed(2),
            );
            let expected = [
                Some(P::HiddenSurface),
                Some(P::NoKeyCapability),
                Some(P::NoMainCapability),
                None,
            ][first_false];
            assert_eq!(failed, expected);
            assert_eq!(checked.get(), (first_false + 1).min(3));
        }
    }

    #[test]
    fn foreground_admission_cannot_substitute_another_active_key_main_window() {
        // Pointer identities only: no Objective-C object is made or dereferenced.
        let windows = [0_u8; 2];
        let expected = std::ptr::from_ref(&windows[0]).cast::<NSWindow>();
        let other = std::ptr::from_ref(&windows[1]).cast::<NSWindow>();
        let facts = ForegroundFacts {
            active: true,
            main_visible: true,
            exact_key: true,
            exact_main: true,
            exact_responder: true,
        };
        assert!(exact_foreground_admission(expected, expected, facts));
        assert!(!exact_foreground_admission(expected, other, facts));
        assert!(!exact_foreground_admission(
            std::ptr::null(),
            std::ptr::null(),
            facts
        ));
    }

    #[test]
    fn captured_foreground_facts_do_not_authorize_a_later_changed_owner() {
        let window = 0_u8;
        let expected = std::ptr::from_ref(&window).cast::<NSWindow>();
        let captured = ForegroundFacts {
            active: true,
            main_visible: true,
            exact_key: true,
            exact_main: true,
            exact_responder: true,
        };
        assert!(exact_foreground_admission(expected, expected, captured));
        for change in [
            |facts: &mut ForegroundFacts| facts.exact_key = false,
            |facts: &mut ForegroundFacts| facts.exact_main = false,
            |facts: &mut ForegroundFacts| facts.exact_responder = false,
            |facts: &mut ForegroundFacts| facts.active = false,
        ] {
            let mut current = captured;
            change(&mut current);
            assert!(!exact_foreground_admission(expected, expected, current));
        }
    }

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

    #[test]
    fn foreground_geometry_avoids_the_observed_half_point_expansion() {
        let screen = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 803.0));
        let planned = surface_frame(screen).unwrap();
        assert_eq!(
            planned,
            NSRect::new(NSPoint::new(0.0, 1.0), viewport().size)
        );
        // Exact supplied AppKit result for the old half-point placement. The
        // viewport contract must continue refusing this enlarged rectangle.
        let enlarged = NSRect::new(NSPoint::new(0.0, 1.0), NSSize::new(1280.0, 801.0));
        assert!(!admitted_frame(enlarged, screen));
        assert!(!backing_frame_admitted(planned, screen, 2.0, enlarged));
    }

    #[test]
    fn foreground_geometry_integral_placement_respects_all_screen_origins() {
        for (origin, size, expected) in [
            ((1440.0, 23.0), (1440.0, 901.0), (1520.0, 73.0)),
            ((-1440.0, -901.0), (1440.0, 901.0), (-1360.0, -851.0)),
            ((-0.5, -0.5), (1281.0, 801.0), (0.0, 0.0)),
            ((0.25, 0.25), (1281.0, 801.0), (1.0, 1.0)),
            ((-1281.75, -801.75), (1281.0, 801.0), (-1281.0, -801.0)),
            ((0.0, 0.0), (1280.0, 800.0), (0.0, 0.0)),
        ] {
            let screen = NSRect::new(
                NSPoint::new(origin.0, origin.1),
                NSSize::new(size.0, size.1),
            );
            let planned = surface_frame(screen).unwrap();
            assert_eq!(planned.origin, NSPoint::new(expected.0, expected.1));
            assert_eq!(planned.size, viewport().size);
            assert!(admitted_frame(planned, screen));
        }
        // There is room for the nominal size but no integral-point origin.
        // Flooring outside the screen or resizing to fit is not permitted.
        for origin in [0.25, -0.25] {
            assert!(
                surface_frame(NSRect::new(NSPoint::new(origin, origin), viewport().size,))
                    .is_none()
            );
        }
    }

    #[test]
    fn foreground_geometry_refuses_nonfinite_overflow_and_unrepresentable_edges() {
        for invalid in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            -f64::MAX,
        ] {
            for screen in [
                NSRect::new(NSPoint::new(invalid, 0.0), viewport().size),
                NSRect::new(NSPoint::new(0.0, invalid), viewport().size),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(invalid, 800.0)),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, invalid)),
            ] {
                assert!(surface_frame(screen).is_none());
            }
        }
        assert!(surface_frame(NSRect::new(
            NSPoint::new(f64::MAX, 0.0),
            NSSize::new(f64::MAX, 800.0),
        ))
        .is_none());
    }

    #[test]
    fn foreground_backing_alignment_attests_but_never_replaces_the_plan() {
        let screen = NSRect::new(NSPoint::new(-1440.0, 20.0), NSSize::new(1440.0, 900.0));
        let frame = surface_frame(screen).unwrap();
        // These are deterministic samples of the alignment contract, not a
        // claim that an AppKit display at each scale was exercised natively.
        for scale in [1.0, 2.0, 1.25, 1.5, 3.0] {
            assert!(backing_frame_admitted(frame, screen, scale, frame));
            for changed in [
                NSRect::new(
                    NSPoint::new(frame.origin.x + 0.5, frame.origin.y),
                    frame.size,
                ),
                NSRect::new(
                    NSPoint::new(frame.origin.x, frame.origin.y - 0.5),
                    frame.size,
                ),
                NSRect::new(frame.origin, NSSize::new(1280.0, 801.0)),
                NSRect::new(frame.origin, NSSize::new(1279.0, 800.0)),
                NSRect::new(NSPoint::new(f64::NAN, frame.origin.y), frame.size),
            ] {
                assert!(!backing_frame_admitted(frame, screen, scale, changed));
            }
        }
        for scale in [
            0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
        ] {
            assert!(!backing_frame_admitted(frame, screen, scale, frame));
        }
    }
}
