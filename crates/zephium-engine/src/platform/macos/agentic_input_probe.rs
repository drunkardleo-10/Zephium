//! Release-excluded macOS native-input/isTrusted risk probe.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::MainThreadOnly as _;
use objc2_foundation::{
    MainThreadMarker, NSDate, NSError, NSPoint, NSProcessInfo, NSRect, NSRunLoop, NSSize, NSString,
};
use objc2_web_kit::WKContentWorld;
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};
use serde::Deserialize;
use wry::{
    NavigationEvent, NavigationEventPhase, NavigationId, NewWindowResponse, PageLoadEvent,
    WebViewBuilderExtMacos as _, WebViewExtMacOS as _, WryWebView,
};
use zephium_agentic::{
    ActivationEvidence, BackendAvailability, BackendCapability, CaseEvidence, CaseOutcome,
    EvidenceLabel, FixtureCase, FixtureRoute, FixtureServer, FixtureTarget, FocusEvidence,
    FocusOwner, GateOutcome, InputBackend, InputEventEvidence, InputEventKind, Platform,
    PresentationState, ProbeFailure, ProbeFailureCode, ProbeRunPermit, ProbeStage,
    ResourceEvidence, RunEvidence, RunMatrixRequest, RuntimeFingerprint, TargetEvidence,
    TeardownEvidence,
};
use zephium_core::ids::ProfileId;
use zephium_core::ports::engine::Partition;

use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEvent,
    NSEventModifierFlags, NSEventType, NSObjectNSAccessibility as _, NSView, NSWindow,
    NSWindowStyleMask,
};

const RUN_TIMEOUT: Duration = Duration::from_secs(60);
const NAVIGATION_TIMEOUT: Duration = Duration::from_secs(10);
const EVALUATION_TIMEOUT: Duration = Duration::from_secs(5);
const SETTLE_WINDOW: Duration = Duration::from_millis(90);
const TEARDOWN_WINDOW: Duration = Duration::from_millis(250);
const RUN_LOOP_SLICE: Duration = Duration::from_millis(5);
const MAX_EVALUATION_RESULT_UTF16: usize = 32 * 1_024;
const MAX_EVALUATION_RESULT_UTF8: usize = 32 * 1_024;
const MAX_PENDING_NAVIGATION_TERMINALS: usize = 8;

struct ProbeHostView {
    view: Retained<NSView>,
}

struct EphemeralProbeProfile {
    partition: Partition,
    store: super::WebsiteDataStore,
}

impl EphemeralProbeProfile {
    fn new() -> Result<Self, AdapterError> {
        let partition = Partition::Ephemeral(ProfileId::generate());
        let store =
            super::new_ephemeral_data_store().map_err(|_| AdapterError::NativeConstruction)?;
        Ok(Self { partition, store })
    }

    fn configuration(
        &self,
    ) -> Result<Retained<objc2_web_kit::WKWebViewConfiguration>, AdapterError> {
        if !matches!(self.partition, Partition::Ephemeral(_)) {
            return Err(AdapterError::NativeConstruction);
        }
        super::new_configuration_with_data_store(&self.store)
            .map_err(|_| AdapterError::NativeConstruction)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NavigationTerminal {
    Finished,
    Failed,
}

#[derive(Default)]
struct NavigationTracker {
    terminals: RefCell<VecDeque<(NavigationId, NavigationTerminal)>>,
    failed: Cell<bool>,
}

impl NavigationTracker {
    fn observe(&self, event: NavigationEvent) {
        let terminal = match event.phase {
            NavigationEventPhase::Finished => NavigationTerminal::Finished,
            NavigationEventPhase::Failed => NavigationTerminal::Failed,
            NavigationEventPhase::Started
            | NavigationEventPhase::Redirected
            | NavigationEventPhase::Committed => return,
        };
        let Ok(mut terminals) = self.terminals.try_borrow_mut() else {
            self.failed.set(true);
            return;
        };
        if terminals.iter().any(|(id, _)| *id == event.id) {
            self.failed.set(true);
            return;
        }
        if terminals.len() >= MAX_PENDING_NAVIGATION_TERMINALS {
            self.failed.set(true);
            return;
        }
        terminals.push_back((event.id, terminal));
    }

    fn take(&self, expected: NavigationId) -> Result<Option<NavigationTerminal>, AdapterError> {
        if self.failed.get() {
            return Err(AdapterError::Navigation);
        }
        let mut terminals = self
            .terminals
            .try_borrow_mut()
            .map_err(|_| AdapterError::Navigation)?;
        let Some(index) = terminals.iter().position(|(id, _)| *id == expected) else {
            return Ok(None);
        };
        Ok(terminals.remove(index).map(|(_, terminal)| terminal))
    }

    fn reset_for_reload(&self) -> Result<(), AdapterError> {
        if self.failed.get() {
            return Err(AdapterError::Navigation);
        }
        self.terminals
            .try_borrow_mut()
            .map_err(|_| AdapterError::Navigation)?
            .clear();
        Ok(())
    }
}

impl HasWindowHandle for ProbeHostView {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let pointer = NonNull::from(&*self.view).cast::<c_void>();
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(pointer));
        // SAFETY: `self.view` is retained for the returned borrow and its
        // retained NSWindow outlives the Wry child.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdapterError {
    Cancelled,
    Timeout,
    NativeConstruction,
    Navigation,
    Evaluation,
    InvalidEvidence,
    Teardown,
}

impl AdapterError {
    const fn code(self) -> ProbeFailureCode {
        match self {
            Self::Cancelled => ProbeFailureCode::Cancelled,
            Self::Timeout => ProbeFailureCode::Timeout,
            Self::NativeConstruction | Self::Navigation | Self::Evaluation | Self::Teardown => {
                ProbeFailureCode::HarnessFailure
            }
            Self::InvalidEvidence => ProbeFailureCode::VerificationFailed,
        }
    }

    const fn retryable(self) -> bool {
        matches!(self, Self::Timeout | Self::Navigation | Self::Evaluation)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureState {
    case: FixtureCase,
    events: Vec<InputEventEvidence>,
    actual_target: Option<FixtureTarget>,
    target_verified: bool,
    active_before: bool,
    active_during_event: bool,
    active_after_event: bool,
    active_after_settle: bool,
    has_been_active: bool,
    activation_capture_scheduled: bool,
    navigation_observed: bool,
    popup_observed: bool,
    clipboard_gate: GateOutcome,
    button_count: u8,
    input_length: u16,
    content_length: u16,
    selected_index: u8,
    drop_observed: bool,
}

impl FixtureState {
    fn validate(&self, expected: FixtureCase) -> Result<(), AdapterError> {
        if self.case != expected
            || self.events.len() > zephium_agentic::MAX_EVENT_EVIDENCE
            || self.button_count > 1
            || self.input_length > 64
            || self.content_length > 64
            || self.selected_index > 1
            || (self.popup_observed && expected != FixtureCase::Popup)
        {
            return Err(AdapterError::InvalidEvidence);
        }
        let activation_event_observed = self
            .events
            .iter()
            .any(|event| matches!(event.kind, InputEventKind::Click | InputEventKind::KeyDown));
        if self.activation_capture_scheduled != activation_event_observed {
            return Err(AdapterError::InvalidEvidence);
        }
        if self.drop_observed
            && !self
                .events
                .iter()
                .any(|event| event.kind == InputEventKind::Drop)
        {
            return Err(AdapterError::InvalidEvidence);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Geometry {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    end_x: Option<f64>,
    end_y: Option<f64>,
}

impl Geometry {
    fn validate(self) -> Result<Self, AdapterError> {
        let values = [self.x, self.y, self.width, self.height];
        if !values.into_iter().all(f64::is_finite)
            || self.width <= 0.0
            || self.height <= 0.0
            || self.width > 2_000.0
            || self.height > 2_000.0
            || self.x.abs() > 4_000.0
            || self.y.abs() > 4_000.0
            || self
                .end_x
                .is_some_and(|value| !value.is_finite() || value.abs() > 4_000.0)
            || self
                .end_y
                .is_some_and(|value| !value.is_finite() || value.abs() > 4_000.0)
        {
            return Err(AdapterError::InvalidEvidence);
        }
        Ok(self)
    }
}

enum FixedScript {
    Ready,
    Reset(FixtureCase),
    Read,
    Geometry(FixtureCase),
    DomRecipe(FixtureCase),
}

impl FixedScript {
    fn source(&self) -> Option<Cow<'static, str>> {
        match *self {
            Self::Ready => Some(Cow::Borrowed(
                "typeof window.__zephiumNativeInputFixtureV1 === 'object' && document.documentElement.dataset.fixtureReady === 'v1' ? 'ready' : 'pending'",
            )),
            Self::Reset(case) => Some(Cow::Owned(format!(
                "window.__zephiumNativeInputFixtureV1.reset('{}') ? 'ok' : 'failed'",
                case_name(case)
            ))),
            Self::Read => Some(Cow::Borrowed(
                "window.__zephiumNativeInputFixtureV1.readJson()",
            )),
            Self::Geometry(case) => geometry_script(case).map(Cow::Owned),
            Self::DomRecipe(case) => dom_recipe(case).map(Cow::Borrowed),
        }
    }

    fn world(&self, mtm: MainThreadMarker) -> Retained<WKContentWorld> {
        match self {
            Self::Ready | Self::Reset(_) | Self::Read => unsafe { WKContentWorld::pageWorld(mtm) },
            Self::Geometry(_) | Self::DomRecipe(_) => unsafe {
                WKContentWorld::defaultClientWorld(mtm)
            },
        }
    }
}

pub(crate) fn run(
    request_id: u64,
    matrix: &RunMatrixRequest,
    permit: &ProbeRunPermit,
    mut poll_control: impl FnMut(),
) -> Result<RunEvidence, ProbeFailure> {
    if request_id == 0 || request_id != permit.request_id() || matrix.validate().is_err() {
        return Err(failure(
            ProbeFailureCode::InvalidRequest,
            ProbeStage::Admit,
            None,
            None,
            false,
        ));
    }
    objc2::rc::autoreleasepool(|_| {
        run_in_autorelease_pool(request_id, matrix, permit, &mut poll_control)
    })
}

fn run_in_autorelease_pool(
    request_id: u64,
    matrix: &RunMatrixRequest,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
) -> Result<RunEvidence, ProbeFailure> {
    let started = Instant::now();
    let run_deadline = started.checked_add(RUN_TIMEOUT).ok_or_else(|| {
        failure(
            ProbeFailureCode::Timeout,
            ProbeStage::Admit,
            None,
            None,
            false,
        )
    })?;
    let mtm = MainThreadMarker::new().ok_or_else(|| {
        failure(
            ProbeFailureCode::HarnessFailure,
            ProbeStage::Construct,
            None,
            None,
            false,
        )
    })?;
    let profile = EphemeralProbeProfile::new()
        .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
    let configuration = profile
        .configuration()
        .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
    if unsafe { configuration.webExtensionController() }.is_some() {
        return Err(adapter_failure(
            AdapterError::NativeConstruction,
            ProbeStage::Construct,
            None,
            None,
        ));
    }
    let server = FixtureServer::start().map_err(|_| {
        failure(
            ProbeFailureCode::HarnessFailure,
            ProbeStage::Construct,
            None,
            None,
            true,
        )
    })?;

    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    let app_active_before_presentation = app.isActive();

    let window = new_window(mtm)
        .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
    let host = ProbeHostView {
        view: window.contentView().ok_or_else(|| {
            adapter_failure(
                AdapterError::NativeConstruction,
                ProbeStage::Construct,
                None,
                None,
            )
        })?,
    };
    let load_generation = Rc::new(Cell::new(0_u16));
    let load_generation_callback = Rc::clone(&load_generation);
    let load_counter_failed = Rc::new(Cell::new(false));
    let load_counter_failed_callback = Rc::clone(&load_counter_failed);
    let navigation_tracker = Rc::new(NavigationTracker::default());
    let navigation_tracker_callback = Rc::clone(&navigation_tracker);
    let popup_requested = Rc::new(Cell::new(false));
    let popup_request_callback = Rc::clone(&popup_requested);
    let webview = wry::WebViewBuilder::new()
        .with_incognito(true)
        .with_visible(false)
        .with_webview_configuration(configuration)
        .with_on_page_load_handler(move |event, _url| {
            if matches!(event, PageLoadEvent::Finished) {
                if let Some(next) = load_generation_callback.get().checked_add(1) {
                    load_generation_callback.set(next);
                } else {
                    load_counter_failed_callback.set(true);
                }
            }
        })
        .with_navigation_event_handler(move |event| navigation_tracker_callback.observe(event))
        .with_new_window_req_handler(move |_url, _features| {
            popup_request_callback.set(true);
            NewWindowResponse::Deny
        })
        .build_as_child(&host)
        .map_err(|_| {
            adapter_failure(
                AdapterError::NativeConstruction,
                ProbeStage::Construct,
                None,
                None,
            )
        })?;
    let page = webview.webview();
    let page_configuration = unsafe { page.configuration() };
    let page_store = unsafe { page_configuration.websiteDataStore() };
    let page_configuration_valid = !unsafe { page_store.isPersistent() }
        && unsafe { page_store.identifier() }.is_none()
        && unsafe { page_configuration.webExtensionController() }.is_none();
    let run_loop = NSRunLoop::mainRunLoop();
    let execution = (|| -> Result<_, ProbeFailure> {
        if !page_configuration_valid {
            return Err(adapter_failure(
                AdapterError::NativeConstruction,
                ProbeStage::Construct,
                None,
                None,
            ));
        }
        webview
            .load_url(&server.url(FixtureRoute::NativeInput))
            .map_err(|_| {
                adapter_failure(AdapterError::Navigation, ProbeStage::Navigate, None, None)
            })?;
        wait_for_fixture(
            mtm,
            &page,
            &run_loop,
            &load_generation,
            &load_counter_failed,
            0,
            permit,
            poll_control,
            run_deadline,
        )
        .map_err(|error| adapter_failure(error, ProbeStage::Navigate, None, None))?;
        apply_presentation(&app, &window, &page, &webview, matrix.presentation)
            .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;

        let runtime = runtime_fingerprint()
            .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
        let capabilities = macos_capabilities();
        let mut cases = Vec::with_capacity(matrix.cases.len() * matrix.backends.len());
        let mut document_consumed = false;
        let mut cancelled = false;
        for case in matrix.cases.iter().copied() {
            for backend in matrix.backends.iter().copied() {
                poll_control();
                if permit.is_cancelled() || Instant::now() >= run_deadline {
                    cases.push(cancelled_case(case, backend, matrix.presentation));
                    cancelled = true;
                    break;
                }
                if document_consumed {
                    reload_fixture(
                        mtm,
                        &page,
                        &run_loop,
                        &navigation_tracker,
                        permit,
                        poll_control,
                        run_deadline,
                    )
                    .map_err(|error| {
                        adapter_failure(error, ProbeStage::Navigate, Some(case), Some(backend))
                    })?;
                }
                document_consumed = true;
                let evidence = run_case(
                    mtm,
                    &app,
                    &window,
                    &page,
                    &run_loop,
                    &popup_requested,
                    case,
                    backend,
                    matrix.presentation,
                    permit,
                    poll_control,
                    run_deadline,
                )?;
                cases.push(evidence);
                if permit.is_cancelled() {
                    cancelled = true;
                    break;
                }
            }
            if cancelled {
                break;
            }
        }
        Ok((runtime, capabilities, cases))
    })();

    let teardown_started = Instant::now();
    let page_weak = Weak::from_retained(&page);
    let window_weak = Weak::from_retained(&window);
    let profile_store_weak = Weak::from_retained(&profile.store);
    drop(page_store);
    drop(page_configuration);
    drop(page);
    drop(webview);
    window.close();
    drop(host);
    drop(window);
    drop(profile);
    pump_for(&run_loop, TEARDOWN_WINDOW, poll_control);
    let retained_native_views = u8::from(page_weak.load().is_some());
    let work_drained = window_weak.load().is_none() && profile_store_weak.load().is_none();
    server
        .shutdown()
        .map_err(|_| adapter_failure(AdapterError::Teardown, ProbeStage::Teardown, None, None))?;
    if !work_drained {
        return Err(adapter_failure(
            AdapterError::Teardown,
            ProbeStage::Teardown,
            None,
            None,
        ));
    }
    let teardown = TeardownEvidence {
        view_closed: retained_native_views == 0,
        work_drained,
        retained_native_views,
        cleanup_ms: duration_ms_u32(teardown_started.elapsed()),
    };

    let (runtime, capabilities, mut cases) = execution?;

    // If a non-focused route activated this process, every case records the
    // transition. Retain this process-level fact by requiring at least one
    // row to carry it rather than inventing a separate raw application field.
    if matrix.presentation != PresentationState::VisibleFocused
        && !app_active_before_presentation
        && app.isActive()
        && !cases.is_empty()
        && !cases.iter().any(|case| case.focus.browse_focus_was_stolen)
    {
        cases[0].focus.browse_focus_was_stolen = true;
    }

    let evidence = RunEvidence {
        run_id: request_id,
        runtime,
        capabilities,
        peak_queue_depth: u8::from(!cases.is_empty()),
        cases,
        elapsed_ms: duration_ms_u64(started.elapsed()),
        teardown,
    };
    evidence.validate().map_err(|_| {
        adapter_failure(
            AdapterError::InvalidEvidence,
            ProbeStage::Verify,
            None,
            None,
        )
    })?;
    Ok(evidence)
}

fn new_window(mtm: MainThreadMarker) -> Result<Retained<NSWindow>, AdapterError> {
    // SAFETY: the marker proves AppKit main-thread affinity. The window is
    // retained through child teardown and configured not to consume itself on close.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(80.0, 80.0), NSSize::new(760.0, 640.0)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window
        .contentView()
        .ok_or(AdapterError::NativeConstruction)?;
    Ok(window)
}

fn apply_presentation(
    app: &NSApplication,
    window: &NSWindow,
    page: &WryWebView,
    webview: &wry::WebView,
    presentation: PresentationState,
) -> Result<(), AdapterError> {
    match presentation {
        PresentationState::Hidden => {
            webview
                .set_visible(false)
                .map_err(|_| AdapterError::NativeConstruction)?;
            window.orderOut(None);
        }
        PresentationState::VisibleBackground => {
            webview
                .set_visible(true)
                .map_err(|_| AdapterError::NativeConstruction)?;
            window.orderFront(None);
        }
        PresentationState::VisibleFocused => {
            webview
                .set_visible(true)
                .map_err(|_| AdapterError::NativeConstruction)?;
            app.activate();
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
            if !window.makeFirstResponder(Some(page)) {
                return Err(AdapterError::NativeConstruction);
            }
            window.makeKeyAndOrderFront(None);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn wait_for_fixture(
    mtm: MainThreadMarker,
    page: &WryWebView,
    run_loop: &NSRunLoop,
    load_generation: &Cell<u16>,
    load_counter_failed: &Cell<bool>,
    after_generation: u16,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    run_deadline: Instant,
) -> Result<(), AdapterError> {
    let deadline = earlier_deadline(run_deadline, NAVIGATION_TIMEOUT)?;
    while Instant::now() < deadline {
        poll_control();
        if permit.is_cancelled() {
            return Err(AdapterError::Cancelled);
        }
        if load_counter_failed.get() {
            return Err(AdapterError::Navigation);
        }
        if load_generation.get() > after_generation {
            let state = evaluate_string(
                mtm,
                page,
                run_loop,
                FixedScript::Ready,
                permit,
                poll_control,
                deadline,
            )?;
            if state == "ready" {
                return Ok(());
            }
        }
        pump_once(run_loop);
    }
    Err(AdapterError::Timeout)
}

fn reload_fixture(
    mtm: MainThreadMarker,
    page: &WryWebView,
    run_loop: &NSRunLoop,
    navigation_tracker: &NavigationTracker,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    run_deadline: Instant,
) -> Result<(), AdapterError> {
    navigation_tracker.reset_for_reload()?;
    let navigation = unsafe { page.reload() }.ok_or(AdapterError::Navigation)?;
    let raw_id = u64::try_from(std::ptr::from_ref(&*navigation) as usize)
        .map_err(|_| AdapterError::Navigation)?;
    let expected = NavigationId::from_raw(raw_id);
    let deadline = earlier_deadline(run_deadline, NAVIGATION_TIMEOUT)?;
    while Instant::now() < deadline {
        poll_control();
        if permit.is_cancelled() {
            return Err(AdapterError::Cancelled);
        }
        match navigation_tracker.take(expected)? {
            Some(NavigationTerminal::Finished) => {
                let state = evaluate_string(
                    mtm,
                    page,
                    run_loop,
                    FixedScript::Ready,
                    permit,
                    poll_control,
                    deadline,
                )?;
                return if state == "ready" {
                    Ok(())
                } else {
                    Err(AdapterError::Navigation)
                };
            }
            Some(NavigationTerminal::Failed) => return Err(AdapterError::Navigation),
            None => pump_once(run_loop),
        }
    }
    Err(AdapterError::Timeout)
}

#[allow(clippy::too_many_arguments)]
fn run_case(
    mtm: MainThreadMarker,
    app: &NSApplication,
    window: &NSWindow,
    page: &WryWebView,
    run_loop: &NSRunLoop,
    popup_requested: &Cell<bool>,
    case: FixtureCase,
    backend: InputBackend,
    presentation: PresentationState,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    run_deadline: Instant,
) -> Result<CaseEvidence, ProbeFailure> {
    let started = Instant::now();
    let reset = evaluate_string(
        mtm,
        page,
        run_loop,
        FixedScript::Reset(case),
        permit,
        poll_control,
        run_deadline,
    )
    .map_err(|error| adapter_failure(error, ProbeStage::Observe, Some(case), Some(backend)))?;
    if reset != "ok" {
        return Err(adapter_failure(
            AdapterError::InvalidEvidence,
            ProbeStage::Observe,
            Some(case),
            Some(backend),
        ));
    }
    popup_requested.set(false);

    let resources_before = live_resource_sample();
    let app_active_before = app.isActive();
    let key_before = window.isKeyWindow();
    let focus_before = native_focus_owner(window, false);
    let outcome_hint = execute_backend(
        mtm,
        window,
        page,
        run_loop,
        case,
        backend,
        presentation,
        permit,
        poll_control,
        run_deadline,
    )
    .map_err(|error| adapter_failure(error, ProbeStage::Execute, Some(case), Some(backend)))?;
    let focus_during = native_focus_owner(window, false);
    match settle(run_loop, permit, poll_control, run_deadline) {
        Ok(()) => {}
        Err(AdapterError::Cancelled) => {
            return Ok(cancelled_case(case, backend, presentation));
        }
        Err(error) => {
            return Err(adapter_failure(
                error,
                ProbeStage::Settle,
                Some(case),
                Some(backend),
            ));
        }
    }
    let encoded = evaluate_string(
        mtm,
        page,
        run_loop,
        FixedScript::Read,
        permit,
        poll_control,
        run_deadline,
    )
    .map_err(|error| adapter_failure(error, ProbeStage::Observe, Some(case), Some(backend)))?;
    if encoded.len() > MAX_EVALUATION_RESULT_UTF8 {
        return Err(adapter_failure(
            AdapterError::InvalidEvidence,
            ProbeStage::Observe,
            Some(case),
            Some(backend),
        ));
    }
    let state: FixtureState = serde_json::from_str(&encoded).map_err(|_| {
        adapter_failure(
            AdapterError::InvalidEvidence,
            ProbeStage::Observe,
            Some(case),
            Some(backend),
        )
    })?;
    state
        .validate(case)
        .map_err(|error| adapter_failure(error, ProbeStage::Observe, Some(case), Some(backend)))?;
    let target = case.target();
    let target_received_focus = state
        .events
        .iter()
        .any(|event| event.kind == InputEventKind::Focus && event.target == target);
    let app_active_after = app.isActive();
    let key_after = window.isKeyWindow();
    let focus_after = native_focus_owner(window, target_received_focus);
    let native_popup_requested = popup_requested.get();
    let outcome =
        outcome_hint.unwrap_or_else(|| classify_outcome(case, &state, native_popup_requested));
    Ok(CaseEvidence {
        case,
        backend,
        presentation,
        outcome,
        events: state.events,
        focus: FocusEvidence {
            before: focus_before,
            during: focus_during,
            after: focus_after,
            probe_host_became_key: !key_before && key_after,
            browse_focus_was_stolen: presentation != PresentationState::VisibleFocused
                && ((!app_active_before && app_active_after) || (!key_before && key_after)),
            target_received_dom_focus: target_received_focus,
        },
        activation: ActivationEvidence {
            active_before: state.active_before,
            active_during_event: state.active_during_event,
            active_after_event: state.active_after_event,
            active_after_settle: state.active_after_settle,
            has_been_active: state.has_been_active,
        },
        target: TargetEvidence {
            intended: target,
            actual: state.actual_target,
            target_verified: state.target_verified,
            navigation_observed: state.navigation_observed,
            popup_requested: native_popup_requested,
            popup_observed: state.popup_observed,
            clipboard_gate: state.clipboard_gate,
        },
        resources_before,
        resources_after: live_resource_sample(),
        elapsed_ms: duration_ms_u32(started.elapsed()),
    })
}

#[allow(clippy::too_many_arguments)]
fn execute_backend(
    mtm: MainThreadMarker,
    window: &NSWindow,
    page: &WryWebView,
    run_loop: &NSRunLoop,
    case: FixtureCase,
    backend: InputBackend,
    presentation: PresentationState,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    run_deadline: Instant,
) -> Result<Option<CaseOutcome>, AdapterError> {
    match backend {
        InputBackend::FixedDomRecipe => {
            if case == FixtureCase::ClosedShadow {
                return Ok(Some(CaseOutcome::Unsupported));
            }
            let Some(_) = FixedScript::DomRecipe(case).source() else {
                return Ok(Some(CaseOutcome::Unsupported));
            };
            let result = evaluate_string(
                mtm,
                page,
                run_loop,
                FixedScript::DomRecipe(case),
                permit,
                poll_control,
                run_deadline,
            )?;
            if result == "ok" {
                Ok(None)
            } else {
                Ok(Some(CaseOutcome::VerificationFailed))
            }
        }
        InputBackend::MacosAppKitEvent => {
            let geometry = geometry(
                mtm,
                page,
                run_loop,
                case,
                permit,
                poll_control,
                run_deadline,
            )?;
            dispatch_appkit(window, page, case, geometry)?;
            Ok(None)
        }
        InputBackend::MacosAccessibility => {
            if !accessibility_supported_case(case) {
                return Ok(Some(CaseOutcome::Unsupported));
            }
            let geometry = geometry(
                mtm,
                page,
                run_loop,
                case,
                permit,
                poll_control,
                run_deadline,
            )?;
            if dispatch_accessibility(window, page, geometry)? {
                Ok(None)
            } else {
                Ok(Some(CaseOutcome::Unsupported))
            }
        }
        InputBackend::MacosFocusedOsInput => {
            if presentation == PresentationState::VisibleFocused {
                Ok(Some(CaseOutcome::NeedsHuman))
            } else {
                Ok(Some(CaseOutcome::BlockedByPolicy))
            }
        }
        InputBackend::HumanBaseline => Ok(Some(CaseOutcome::NeedsHuman)),
        InputBackend::WindowsHwndInput
        | InputBackend::WindowsCompositionInput
        | InputBackend::WindowsCdpInput => Ok(Some(CaseOutcome::Unsupported)),
    }
}

fn geometry(
    mtm: MainThreadMarker,
    page: &WryWebView,
    run_loop: &NSRunLoop,
    case: FixtureCase,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    run_deadline: Instant,
) -> Result<Geometry, AdapterError> {
    let encoded = evaluate_string(
        mtm,
        page,
        run_loop,
        FixedScript::Geometry(case),
        permit,
        poll_control,
        run_deadline,
    )?;
    if encoded.len() > 512 {
        return Err(AdapterError::InvalidEvidence);
    }
    serde_json::from_str::<Geometry>(&encoded)
        .map_err(|_| AdapterError::InvalidEvidence)?
        .validate()
}

fn dispatch_appkit(
    window: &NSWindow,
    page: &WryWebView,
    case: FixtureCase,
    geometry: Geometry,
) -> Result<(), AdapterError> {
    let point = window_point(
        page,
        geometry.x + geometry.width / 2.0,
        geometry.y + geometry.height / 2.0,
    )?;
    if matches!(
        case,
        FixtureCase::TextInput
            | FixtureCase::ContentEditable
            | FixtureCase::Keyboard
            | FixtureCase::Select
    ) && !window.makeFirstResponder(Some(page))
    {
        return Err(AdapterError::NativeConstruction);
    }
    match case {
        FixtureCase::Drag => {
            let end_x = geometry.end_x.ok_or(AdapterError::InvalidEvidence)?;
            let end_y = geometry.end_y.ok_or(AdapterError::InvalidEvidence)?;
            let end = window_point(page, end_x, end_y)?;
            dispatch_mouse_event(window, NSEventType::MouseMoved, point, 0.0, 0)?;
            dispatch_mouse_event(window, NSEventType::LeftMouseDown, point, 1.0, 1)?;
            dispatch_mouse_event(window, NSEventType::LeftMouseDragged, end, 1.0, 1)?;
            dispatch_mouse_event(window, NSEventType::LeftMouseUp, end, 0.0, 1)?;
        }
        _ => {
            dispatch_mouse_event(window, NSEventType::MouseMoved, point, 0.0, 0)?;
            dispatch_mouse_event(window, NSEventType::LeftMouseDown, point, 1.0, 1)?;
            dispatch_mouse_event(window, NSEventType::LeftMouseUp, point, 0.0, 1)?;
            match case {
                FixtureCase::TextInput | FixtureCase::ContentEditable | FixtureCase::Keyboard => {
                    dispatch_key(window, "x", "x", 7)?
                }
                FixtureCase::Select => {
                    dispatch_key(window, "\u{f701}", "\u{f701}", 125)?;
                    dispatch_key(window, "\r", "\r", 36)?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn dispatch_mouse_event(
    window: &NSWindow,
    event_type: NSEventType,
    location: NSPoint,
    pressure: f32,
    click_count: isize,
) -> Result<(), AdapterError> {
    let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        event_type,
        location,
        NSEventModifierFlags::empty(),
        NSProcessInfo::processInfo().systemUptime(),
        window.windowNumber(),
        None,
        0,
        click_count,
        pressure,
    )
    .ok_or(AdapterError::NativeConstruction)?;
    window.sendEvent(&event);
    Ok(())
}

fn dispatch_key(
    window: &NSWindow,
    characters: &str,
    unmodified: &str,
    key_code: u16,
) -> Result<(), AdapterError> {
    let characters = NSString::from_str(characters);
    let unmodified = NSString::from_str(unmodified);
    for event_type in [NSEventType::KeyDown, NSEventType::KeyUp] {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            event_type,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            NSProcessInfo::processInfo().systemUptime(),
            window.windowNumber(),
            None,
            &characters,
            &unmodified,
            false,
            key_code,
        )
        .ok_or(AdapterError::NativeConstruction)?;
        window.sendEvent(&event);
    }
    Ok(())
}

fn dispatch_accessibility(
    window: &NSWindow,
    page: &WryWebView,
    geometry: Geometry,
) -> Result<bool, AdapterError> {
    let point = window_point(
        page,
        geometry.x + geometry.width / 2.0,
        geometry.y + geometry.height / 2.0,
    )?;
    let screen = window.convertPointToScreen(point);
    let Some(element) = page.accessibilityHitTest(screen) else {
        return Ok(false);
    };
    // SAFETY: `element` is a retained Objective-C accessibility object and
    // selector introspection has no additional precondition.
    let responds: bool = unsafe {
        objc2::msg_send![&*element, respondsToSelector: objc2::sel!(accessibilityPerformPress)]
    };
    if !responds {
        return Ok(false);
    }
    // SAFETY: selector availability was checked on this exact retained
    // accessibility element. The Objective-C result is a scalar BOOL and no
    // native/page object crosses the probe contract.
    let pressed: bool = unsafe { objc2::msg_send![&*element, accessibilityPerformPress] };
    Ok(pressed)
}

fn window_point(page: &WryWebView, x: f64, y: f64) -> Result<NSPoint, AdapterError> {
    let frame = page.frame();
    if !frame.origin.x.is_finite()
        || !frame.origin.y.is_finite()
        || !frame.size.width.is_finite()
        || !frame.size.height.is_finite()
        || x < 0.0
        || y < 0.0
        || x > frame.size.width
        || y > frame.size.height
    {
        return Err(AdapterError::InvalidEvidence);
    }
    Ok(NSPoint::new(
        frame.origin.x + x,
        frame.origin.y + frame.size.height - y,
    ))
}

fn evaluate_string(
    mtm: MainThreadMarker,
    page: &WryWebView,
    run_loop: &NSRunLoop,
    recipe: FixedScript,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    outer_deadline: Instant,
) -> Result<String, AdapterError> {
    poll_control();
    if permit.is_cancelled() {
        return Err(AdapterError::Cancelled);
    }
    if Instant::now() >= outer_deadline {
        return Err(AdapterError::Timeout);
    }
    let source = recipe.source().ok_or(AdapterError::InvalidEvidence)?;
    let world = recipe.world(mtm);
    let result = Rc::new(RefCell::new(None::<Result<String, AdapterError>>));
    let settlements = Rc::new(Cell::new(0_u8));
    let callback_result = Rc::clone(&result);
    let callback_settlements = Rc::clone(&settlements);
    let completion = block2::RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        let count = callback_settlements.get().saturating_add(1);
        callback_settlements.set(count);
        let settlement = if count != 1 || !error.is_null() {
            Err(AdapterError::Evaluation)
        } else {
            unsafe { value.as_ref() }
                .and_then(AnyObject::downcast_ref::<NSString>)
                .filter(|value| value.length() <= MAX_EVALUATION_RESULT_UTF16)
                .map(ToString::to_string)
                .filter(|value| value.len() <= MAX_EVALUATION_RESULT_UTF8)
                .ok_or(AdapterError::Evaluation)
        };
        if let Ok(mut slot) = callback_result.try_borrow_mut() {
            *slot = Some(settlement);
        }
    });
    let source = NSString::from_str(&source);
    objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        page.evaluateJavaScript_inFrame_inContentWorld_completionHandler(
            &source,
            None,
            &world,
            Some(&completion),
        );
    }))
    .map_err(|_| AdapterError::Evaluation)?;

    let deadline = earlier_deadline(outer_deadline, EVALUATION_TIMEOUT)?;
    loop {
        poll_control();
        if permit.is_cancelled() {
            return Err(AdapterError::Cancelled);
        }
        if let Some(result) = result.borrow_mut().take() {
            return result;
        }
        if Instant::now() >= deadline {
            return Err(AdapterError::Timeout);
        }
        pump_once(run_loop);
    }
}

fn classify_outcome(
    case: FixtureCase,
    state: &FixtureState,
    native_popup_requested: bool,
) -> CaseOutcome {
    if case == FixtureCase::ClosedShadow
        || case == FixtureCase::ClipboardGate
        || (case == FixtureCase::Popup && state.target_verified && !native_popup_requested)
    {
        CaseOutcome::Unsupported
    } else if case == FixtureCase::Popup && state.popup_observed {
        CaseOutcome::VerificationFailed
    } else if state.target_verified && (case != FixtureCase::Popup || native_popup_requested) {
        CaseOutcome::Verified
    } else {
        CaseOutcome::VerificationFailed
    }
}

fn cancelled_case(
    case: FixtureCase,
    backend: InputBackend,
    presentation: PresentationState,
) -> CaseEvidence {
    CaseEvidence {
        case,
        backend,
        presentation,
        outcome: CaseOutcome::Cancelled,
        events: Vec::new(),
        focus: FocusEvidence {
            before: FocusOwner::None,
            during: FocusOwner::None,
            after: FocusOwner::None,
            probe_host_became_key: false,
            browse_focus_was_stolen: false,
            target_received_dom_focus: false,
        },
        activation: ActivationEvidence {
            active_before: false,
            active_during_event: false,
            active_after_event: false,
            active_after_settle: false,
            has_been_active: false,
        },
        target: TargetEvidence {
            intended: case.target(),
            actual: None,
            target_verified: false,
            navigation_observed: false,
            popup_requested: false,
            popup_observed: false,
            clipboard_gate: GateOutcome::NotApplicable,
        },
        resources_before: live_resource_sample(),
        resources_after: live_resource_sample(),
        elapsed_ms: 0,
    }
}

fn live_resource_sample() -> ResourceEvidence {
    ResourceEvidence {
        native_views: 1,
        queued_actions: 0,
        helper_processes: None,
        resident_bytes: None,
    }
}

fn native_focus_owner(window: &NSWindow, target_focused: bool) -> FocusOwner {
    if target_focused {
        FocusOwner::FixtureTarget
    } else if window.isKeyWindow() {
        FocusOwner::ProbeHost
    } else {
        FocusOwner::External
    }
}

fn runtime_fingerprint() -> Result<RuntimeFingerprint, AdapterError> {
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    let os_version = format!(
        "{}.{}.{}",
        version.majorVersion, version.minorVersion, version.patchVersion
    );
    let engine_version = wry::webview_version().map_err(|_| AdapterError::NativeConstruction)?;
    Ok(RuntimeFingerprint {
        platform: Platform::Macos,
        os_version: EvidenceLabel::new(os_version).map_err(|_| AdapterError::InvalidEvidence)?,
        engine: EvidenceLabel::new("WebKit").map_err(|_| AdapterError::InvalidEvidence)?,
        engine_version: EvidenceLabel::new(engine_version)
            .map_err(|_| AdapterError::InvalidEvidence)?,
        adapter_revision: EvidenceLabel::new("native-input-m1")
            .map_err(|_| AdapterError::InvalidEvidence)?,
    })
}

fn macos_capabilities() -> Vec<BackendCapability> {
    vec![
        BackendCapability {
            backend: InputBackend::FixedDomRecipe,
            availability: BackendAvailability::Available,
        },
        BackendCapability {
            backend: InputBackend::MacosAppKitEvent,
            availability: BackendAvailability::Available,
        },
        BackendCapability {
            backend: InputBackend::MacosAccessibility,
            availability: BackendAvailability::Available,
        },
        BackendCapability {
            backend: InputBackend::MacosFocusedOsInput,
            availability: BackendAvailability::RequiresAccessibilityPermission,
        },
        BackendCapability {
            backend: InputBackend::HumanBaseline,
            availability: BackendAvailability::RequiresVisibleFocus,
        },
        BackendCapability {
            backend: InputBackend::WindowsHwndInput,
            availability: BackendAvailability::UnsupportedByIntegration,
        },
        BackendCapability {
            backend: InputBackend::WindowsCompositionInput,
            availability: BackendAvailability::UnsupportedByIntegration,
        },
        BackendCapability {
            backend: InputBackend::WindowsCdpInput,
            availability: BackendAvailability::UnsupportedByIntegration,
        },
    ]
}

fn accessibility_supported_case(case: FixtureCase) -> bool {
    matches!(
        case,
        FixtureCase::Button
            | FixtureCase::Link
            | FixtureCase::TransientActivation
            | FixtureCase::Popup
            | FixtureCase::ClipboardGate
            | FixtureCase::Iframe
            | FixtureCase::OpenShadow
            | FixtureCase::ClosedShadow
    )
}

fn case_name(case: FixtureCase) -> &'static str {
    match case {
        FixtureCase::Button => "button",
        FixtureCase::Link => "link",
        FixtureCase::TextInput => "text_input",
        FixtureCase::ContentEditable => "content_editable",
        FixtureCase::Select => "select",
        FixtureCase::PointerMouse => "pointer_mouse",
        FixtureCase::Keyboard => "keyboard",
        FixtureCase::TransientActivation => "transient_activation",
        FixtureCase::Popup => "popup",
        FixtureCase::ClipboardGate => "clipboard_gate",
        FixtureCase::Drag => "drag",
        FixtureCase::Iframe => "iframe",
        FixtureCase::OpenShadow => "open_shadow",
        FixtureCase::ClosedShadow => "closed_shadow",
    }
}

fn dom_recipe(case: FixtureCase) -> Option<&'static str> {
    Some(match case {
        FixtureCase::Button | FixtureCase::PointerMouse => {
            "(()=>{const e=document.getElementById('button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::Link => {
            "(()=>{const e=document.getElementById('link');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::TextInput => {
            "(()=>{const e=document.getElementById('text-input');if(!e)return 'failed';e.focus();e.value='fixturex';e.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'x'}));return 'ok';})()"
        }
        FixtureCase::ContentEditable => {
            "(()=>{const e=document.getElementById('content-editable');if(!e)return 'failed';e.focus();e.textContent='fixturex';e.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'x'}));return 'ok';})()"
        }
        FixtureCase::Select => {
            "(()=>{const e=document.getElementById('select');if(!e)return 'failed';e.focus();e.selectedIndex=1;e.dispatchEvent(new Event('change',{bubbles:true}));return 'ok';})()"
        }
        FixtureCase::Keyboard => {
            "(()=>{const e=document.getElementById('text-input');if(!e)return 'failed';e.focus();e.dispatchEvent(new KeyboardEvent('keydown',{key:'x',code:'KeyX',bubbles:true}));e.dispatchEvent(new KeyboardEvent('keyup',{key:'x',code:'KeyX',bubbles:true}));return 'ok';})()"
        }
        FixtureCase::TransientActivation => {
            "(()=>{const e=document.getElementById('activation-button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::Popup => {
            "(()=>{const e=document.getElementById('popup-button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::ClipboardGate => {
            "(()=>{const e=document.getElementById('clipboard-button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::Drag => {
            "(()=>{try{const s=document.getElementById('drag-source');const d=document.getElementById('drop-target');if(!s||!d)return 'failed';const t=new DataTransfer();s.dispatchEvent(new DragEvent('dragstart',{bubbles:true,dataTransfer:t}));d.dispatchEvent(new DragEvent('dragenter',{bubbles:true,dataTransfer:t}));d.dispatchEvent(new DragEvent('dragover',{bubbles:true,cancelable:true,dataTransfer:t}));d.dispatchEvent(new DragEvent('drop',{bubbles:true,cancelable:true,dataTransfer:t}));s.dispatchEvent(new DragEvent('dragend',{bubbles:true,dataTransfer:t}));return 'ok';}catch{return 'failed';}})()"
        }
        FixtureCase::Iframe => {
            "(()=>{const e=document.getElementById('frame')?.contentDocument?.getElementById('frame-button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::OpenShadow => {
            "(()=>{const e=document.getElementById('open-shadow-host')?.shadowRoot?.getElementById('open-shadow-button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::ClosedShadow => return None,
    })
}

fn geometry_script(case: FixtureCase) -> Option<String> {
    if case == FixtureCase::Drag {
        return Some(
            "(()=>{const s=document.getElementById('drag-source');const d=document.getElementById('drop-target');if(!s||!d)return '';const r=s.getBoundingClientRect();const q=d.getBoundingClientRect();return JSON.stringify({x:r.x,y:r.y,width:r.width,height:r.height,endX:q.x+q.width/2,endY:q.y+q.height/2});})()".to_owned(),
        );
    }
    if case == FixtureCase::Iframe {
        return Some(
            "(()=>{const f=document.getElementById('frame');const e=f?.contentDocument?.getElementById('frame-button');if(!f||!e)return '';const a=f.getBoundingClientRect();const r=e.getBoundingClientRect();return JSON.stringify({x:a.x+r.x,y:a.y+r.y,width:r.width,height:r.height,endX:null,endY:null});})()".to_owned(),
        );
    }
    let expression = match case {
        FixtureCase::Button | FixtureCase::PointerMouse => "document.getElementById('button')",
        FixtureCase::Link => "document.getElementById('link')",
        FixtureCase::TextInput | FixtureCase::Keyboard => {
            "document.getElementById('text-input')"
        }
        FixtureCase::ContentEditable => "document.getElementById('content-editable')",
        FixtureCase::Select => "document.getElementById('select')",
        FixtureCase::TransientActivation => "document.getElementById('activation-button')",
        FixtureCase::Popup => "document.getElementById('popup-button')",
        FixtureCase::ClipboardGate => "document.getElementById('clipboard-button')",
        FixtureCase::OpenShadow => {
            "document.getElementById('open-shadow-host')?.shadowRoot?.getElementById('open-shadow-button')"
        }
        FixtureCase::ClosedShadow => "document.getElementById('closed-shadow-host')",
        FixtureCase::Drag | FixtureCase::Iframe => return None,
    };
    Some(format!(
        "(()=>{{const e={expression};if(!e)return '';const r=e.getBoundingClientRect();return JSON.stringify({{x:r.x,y:r.y,width:r.width,height:r.height,endX:null,endY:null}});}})()"
    ))
}

fn pump_once(run_loop: &NSRunLoop) {
    objc2::rc::autoreleasepool(|_| {
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(
            RUN_LOOP_SLICE.as_secs_f64(),
        ));
    });
}

fn pump_for(run_loop: &NSRunLoop, duration: Duration, poll_control: &mut impl FnMut()) {
    let Some(deadline) = Instant::now().checked_add(duration) else {
        return;
    };
    while Instant::now() < deadline {
        poll_control();
        pump_once(run_loop);
    }
}

fn settle(
    run_loop: &NSRunLoop,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    run_deadline: Instant,
) -> Result<(), AdapterError> {
    let deadline = earlier_deadline(run_deadline, SETTLE_WINDOW)?;
    while Instant::now() < deadline {
        poll_control();
        if permit.is_cancelled() {
            return Err(AdapterError::Cancelled);
        }
        pump_once(run_loop);
    }
    if Instant::now() >= run_deadline {
        Err(AdapterError::Timeout)
    } else {
        Ok(())
    }
}

fn earlier_deadline(outer: Instant, duration: Duration) -> Result<Instant, AdapterError> {
    let local = Instant::now()
        .checked_add(duration)
        .ok_or(AdapterError::Timeout)?;
    Ok(local.min(outer))
}

fn duration_ms_u32(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

fn duration_ms_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn adapter_failure(
    error: AdapterError,
    stage: ProbeStage,
    case: Option<FixtureCase>,
    backend: Option<InputBackend>,
) -> ProbeFailure {
    failure(error.code(), stage, case, backend, error.retryable())
}

fn failure(
    code: ProbeFailureCode,
    stage: ProbeStage,
    case: Option<FixtureCase>,
    backend: Option<InputBackend>,
    retryable: bool,
) -> ProbeFailure {
    ProbeFailure {
        code,
        stage,
        backend,
        case,
        retryable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(case: FixtureCase) -> FixtureState {
        FixtureState {
            case,
            events: vec![InputEventEvidence {
                kind: InputEventKind::Click,
                is_trusted: false,
                target: case.target(),
            }],
            actual_target: Some(case.target()),
            target_verified: true,
            active_before: false,
            active_during_event: false,
            active_after_event: false,
            active_after_settle: false,
            has_been_active: false,
            activation_capture_scheduled: true,
            navigation_observed: false,
            popup_observed: false,
            clipboard_gate: GateOutcome::NotApplicable,
            button_count: 0,
            input_length: 0,
            content_length: 0,
            selected_index: 0,
            drop_observed: false,
        }
    }

    #[test]
    fn every_case_has_a_closed_target_and_geometry_route() {
        for case in [
            FixtureCase::Button,
            FixtureCase::Link,
            FixtureCase::TextInput,
            FixtureCase::ContentEditable,
            FixtureCase::Select,
            FixtureCase::PointerMouse,
            FixtureCase::Keyboard,
            FixtureCase::TransientActivation,
            FixtureCase::Popup,
            FixtureCase::ClipboardGate,
            FixtureCase::Drag,
            FixtureCase::Iframe,
            FixtureCase::OpenShadow,
            FixtureCase::ClosedShadow,
        ] {
            let _ = case.target();
            assert!(geometry_script(case).is_some());
            assert_eq!(case_name(case).len() <= 32, true);
        }
    }

    #[test]
    fn closed_shadow_has_no_dom_bypass_recipe() {
        assert!(dom_recipe(FixtureCase::ClosedShadow).is_none());
        assert!(accessibility_supported_case(FixtureCase::ClosedShadow));
    }

    #[test]
    fn capability_inventory_is_complete_and_windows_routes_are_unsupported() {
        let capabilities = macos_capabilities();
        assert_eq!(capabilities.len(), 8);
        assert!(capabilities
            .iter()
            .filter(|capability| matches!(
                capability.backend,
                InputBackend::WindowsHwndInput
                    | InputBackend::WindowsCompositionInput
                    | InputBackend::WindowsCdpInput
            ))
            .all(|capability| capability.availability
                == BackendAvailability::UnsupportedByIntegration));
    }

    #[test]
    fn popup_request_and_admission_are_independent_proofs() {
        let no_native_request = state(FixtureCase::Popup);
        assert_eq!(
            classify_outcome(FixtureCase::Popup, &no_native_request, false),
            CaseOutcome::Unsupported
        );

        let mut admitted = state(FixtureCase::Popup);
        admitted.popup_observed = true;
        assert_eq!(
            classify_outcome(FixtureCase::Popup, &admitted, true),
            CaseOutcome::VerificationFailed
        );

        let denied = state(FixtureCase::Popup);
        assert_eq!(
            classify_outcome(FixtureCase::Popup, &denied, true),
            CaseOutcome::Verified
        );
    }

    #[test]
    fn gated_and_ordinary_case_classification_is_fail_closed() {
        assert_eq!(
            classify_outcome(
                FixtureCase::ClipboardGate,
                &state(FixtureCase::ClipboardGate),
                false
            ),
            CaseOutcome::Unsupported
        );
        assert_eq!(
            classify_outcome(FixtureCase::Button, &state(FixtureCase::Button), false),
            CaseOutcome::Verified
        );
    }

    #[test]
    fn navigation_tracker_correlates_exact_terminal_identity() {
        let tracker = NavigationTracker::default();
        let first = NavigationId::from_raw(1);
        let second = NavigationId::from_raw(2);
        tracker.observe(NavigationEvent {
            id: first,
            phase: NavigationEventPhase::Finished,
            url: "http://127.0.0.1/fixture".to_owned(),
        });
        tracker.observe(NavigationEvent {
            id: second,
            phase: NavigationEventPhase::Failed,
            url: "http://127.0.0.1/fixture".to_owned(),
        });
        assert_eq!(
            tracker.take(second).expect("tracker"),
            Some(NavigationTerminal::Failed)
        );
        assert_eq!(
            tracker.take(first).expect("tracker"),
            Some(NavigationTerminal::Finished)
        );
        assert_eq!(tracker.take(first).expect("tracker"), None);
    }

    #[test]
    fn navigation_tracker_overflow_is_sticky() {
        let tracker = NavigationTracker::default();
        for raw in 1..=MAX_PENDING_NAVIGATION_TERMINALS + 1 {
            tracker.observe(NavigationEvent {
                id: NavigationId::from_raw(raw as u64),
                phase: NavigationEventPhase::Finished,
                url: "http://127.0.0.1/fixture".to_owned(),
            });
        }
        assert_eq!(
            tracker.take(NavigationId::from_raw(1)).unwrap_err(),
            AdapterError::Navigation
        );
        assert_eq!(tracker.reset_for_reload(), Err(AdapterError::Navigation));
    }
}
