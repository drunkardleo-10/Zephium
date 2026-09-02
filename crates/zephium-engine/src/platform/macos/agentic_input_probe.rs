#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

//! Release-excluded macOS native-input/isTrusted risk probe.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use objc2::{
    define_class, msg_send,
    rc::{Retained, Weak},
    runtime::{NSObject, ProtocolObject},
    DefinedClass, MainThreadOnly,
};
use objc2_foundation::{
    MainThreadMarker, NSDate, NSObjectProtocol, NSPoint, NSProcessInfo, NSRect, NSRunLoop, NSSize,
    NSString,
};
use objc2_web_kit::{
    WKContentWorld, WKScriptMessage, WKScriptMessageHandler, WKUserContentController, WKUserScript,
    WKUserScriptInjectionTime, WKWebViewConfiguration,
};
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};
use serde::Deserialize;
use wry::{
    NewWindowResponse, PageLoadEvent, WebViewBuilderExtMacos as _, WebViewExtMacOS as _, WryWebView,
};
use zephium_agentic::{
    ActivationEvidence, BackendAvailability, BackendCapability, CaseEvidence, CaseOutcome,
    EvidenceLabel, FixtureCase, FixtureServer, FixtureTarget, FocusEvidence, FocusOwner,
    GateOutcome, InputBackend, InputEventEvidence, InputEventKind, Platform, PresentationState,
    ProbeFailure, ProbeFailureCode, ProbeRunPermit, ProbeStage, ResourceEvidence, RunEvidence,
    RunMatrixRequest, RuntimeFingerprint, TargetEvidence, TeardownEvidence,
    MACOS_NATIVE_INPUT_RUNTIME_V1, MACOS_PROBE_CONTENT_WORLD_V1, MACOS_PROBE_HANDLER_V1,
    MAX_NATIVE_INPUT_RUNTIME_ROW, NATIVE_INPUT_RUNTIME_PROTOCOL_V1,
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
const RUNTIME_RESULT_TIMEOUT: Duration = Duration::from_secs(2);
const TEARDOWN_WINDOW: Duration = Duration::from_millis(250);
const RUN_LOOP_SLICE: Duration = Duration::from_millis(5);
const MAX_RUNTIME_MESSAGE_UTF16: usize = 32 * 1_024;
const MAX_RUNTIME_MESSAGE_UTF8: usize = 32 * 1_024;

struct ProbeHostView {
    view: Retained<NSView>,
}

struct EphemeralProbeProfile {
    partition: Partition,
    store: super::WebsiteDataStore,
}

struct NativeDispatchControl<'a> {
    permit: &'a ProbeRunPermit,
    poll_control: &'a mut dyn FnMut(),
    deadline: Instant,
}

impl NativeDispatchControl<'_> {
    fn check(&mut self) -> Result<(), AdapterError> {
        (self.poll_control)();
        if self.permit.is_cancelled() {
            return Err(AdapterError::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(AdapterError::Timeout);
        }
        Ok(())
    }
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
    InvalidEvidence,
    NativeTeardown,
    ProfileTeardown,
    FixtureTeardown,
}

impl AdapterError {
    const fn code(self) -> ProbeFailureCode {
        match self {
            Self::Cancelled => ProbeFailureCode::Cancelled,
            Self::Timeout => ProbeFailureCode::Timeout,
            Self::NativeConstruction | Self::Navigation => ProbeFailureCode::HarnessFailure,
            Self::NativeTeardown => ProbeFailureCode::NativeTeardownIncomplete,
            Self::ProfileTeardown => ProbeFailureCode::ProfileTeardownIncomplete,
            Self::FixtureTeardown => ProbeFailureCode::FixtureTeardownIncomplete,
            Self::InvalidEvidence => ProbeFailureCode::VerificationFailed,
        }
    }

    const fn retryable(self) -> bool {
        matches!(self, Self::Timeout | Self::Navigation)
    }
}

type RunParts = (
    RuntimeFingerprint,
    Vec<BackendCapability>,
    Vec<CaseEvidence>,
);

struct PendingTeardown {
    run_id: u64,
    started: Instant,
    execution: Result<RunParts, ProbeFailure>,
    page: Weak<WryWebView>,
    window: Weak<NSWindow>,
    profile_store: Weak<objc2_web_kit::WKWebsiteDataStore>,
    server: FixtureServer,
    teardown_started: Instant,
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
    device_pixel_ratio: f64,
}

impl Geometry {
    fn validate(self) -> Result<Self, AdapterError> {
        let values = [
            self.x,
            self.y,
            self.width,
            self.height,
            self.device_pixel_ratio,
        ];
        if !values.into_iter().all(f64::is_finite)
            || self.width <= 0.0
            || self.height <= 0.0
            || self.width > 2_000.0
            || self.height > 2_000.0
            || self.x.abs() > 4_000.0
            || self.y.abs() > 4_000.0
            || !(0.25..=8.0).contains(&self.device_pixel_ratio)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProbeRowKey {
    row: u16,
    case: FixtureCase,
    backend: InputBackend,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ProbeRuntimeFaultCode {
    FixtureNotReady,
    MissingTarget,
    InvalidEvidence,
    MissingEvidence,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
enum ProbeRuntimeMessage {
    Ready {
        protocol: u8,
        row: u16,
        case: FixtureCase,
        backend: InputBackend,
        geometry: Geometry,
    },
    Result {
        protocol: u8,
        row: u16,
        case: FixtureCase,
        backend: InputBackend,
        state: FixtureState,
    },
    Fault {
        protocol: u8,
        row: u16,
        case: FixtureCase,
        backend: InputBackend,
        code: ProbeRuntimeFaultCode,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProbeMailboxPhase {
    Idle,
    AwaitingReady,
    AwaitingResult,
    Complete,
}

struct ProbeRuntimeMailbox {
    expected: Cell<Option<ProbeRowKey>>,
    expected_url: RefCell<Option<String>>,
    page: RefCell<Option<Weak<WryWebView>>>,
    phase: Cell<ProbeMailboxPhase>,
    ready: RefCell<Option<Geometry>>,
    result: RefCell<Option<FixtureState>>,
    failed: Cell<bool>,
}

impl ProbeRuntimeMailbox {
    fn new() -> Self {
        Self {
            expected: Cell::new(None),
            expected_url: RefCell::new(None),
            page: RefCell::new(None),
            phase: Cell::new(ProbeMailboxPhase::Idle),
            ready: RefCell::new(None),
            result: RefCell::new(None),
            failed: Cell::new(false),
        }
    }

    fn bind_page(&self, page: &Retained<WryWebView>) -> Result<(), AdapterError> {
        let mut slot = self
            .page
            .try_borrow_mut()
            .map_err(|_| AdapterError::NativeConstruction)?;
        if slot.is_some() {
            return Err(AdapterError::NativeConstruction);
        }
        *slot = Some(Weak::from_retained(page));
        Ok(())
    }

    fn arm(&self, key: ProbeRowKey, expected_url: String) -> Result<(), AdapterError> {
        if key.row == 0
            || key.row > MAX_NATIVE_INPUT_RUNTIME_ROW
            || self.failed.get()
            || self.phase.get() != ProbeMailboxPhase::Idle
            || self.expected.get().is_some()
            || self.ready.borrow().is_some()
            || self.result.borrow().is_some()
        {
            return Err(AdapterError::InvalidEvidence);
        }
        self.expected.set(Some(key));
        *self
            .expected_url
            .try_borrow_mut()
            .map_err(|_| AdapterError::InvalidEvidence)? = Some(expected_url);
        self.phase.set(ProbeMailboxPhase::AwaitingReady);
        Ok(())
    }

    fn receive(&self, message: &WKScriptMessage) {
        if self.receive_checked(message).is_err() {
            self.failed.set(true);
        }
    }

    fn receive_checked(&self, message: &WKScriptMessage) -> Result<(), AdapterError> {
        if self.failed.get() || self.phase.get() == ProbeMailboxPhase::Idle {
            return Err(AdapterError::InvalidEvidence);
        }
        let expected_page = self
            .page
            .try_borrow()
            .map_err(|_| AdapterError::InvalidEvidence)?
            .as_ref()
            .and_then(Weak::load)
            .ok_or(AdapterError::InvalidEvidence)?;
        // SAFETY: WebKit supplied this live message to the main-thread-only
        // handler, and objc2 retains the optional page result.
        let message_page = unsafe { message.webView() }.ok_or(AdapterError::InvalidEvidence)?;
        if Retained::as_ptr(&message_page).cast::<c_void>()
            != Retained::as_ptr(&expected_page).cast::<c_void>()
        {
            return Err(AdapterError::InvalidEvidence);
        }
        // SAFETY: the live WebKit message owns valid frame metadata for the
        // duration of this callback, and objc2 retains the returned object.
        let frame = unsafe { message.frameInfo() };
        // SAFETY: `frame` is the retained metadata returned by this message.
        if !unsafe { frame.isMainFrame() } {
            return Err(AdapterError::InvalidEvidence);
        }
        // SAFETY: `frame` remains retained and WebKit exposes its optional
        // owning page through this property.
        let frame_page = unsafe { frame.webView() }.ok_or(AdapterError::InvalidEvidence)?;
        if Retained::as_ptr(&frame_page).cast::<c_void>()
            != Retained::as_ptr(&expected_page).cast::<c_void>()
        {
            return Err(AdapterError::InvalidEvidence);
        }
        // SAFETY: `frame` remains retained; objc2 retains the request returned
        // by WebKit before this callback can release its framework arguments.
        let request = unsafe { frame.request() };
        let frame_url = request
            .URL()
            .and_then(|url| url.absoluteString())
            .map(|url| url.to_string())
            .ok_or(AdapterError::InvalidEvidence)?;
        let frame_url = frame_url.split('#').next().unwrap_or_default();
        let expected_url = self
            .expected_url
            .try_borrow()
            .map_err(|_| AdapterError::InvalidEvidence)?;
        if expected_url.as_deref() != Some(frame_url) {
            return Err(AdapterError::InvalidEvidence);
        }

        // SAFETY: the live callback message owns its body and objc2 returns a
        // retained Objective-C object, which is downcast before use.
        let body = unsafe { message.body() }
            .downcast::<NSString>()
            .map_err(|_| AdapterError::InvalidEvidence)?;
        if body.length() > MAX_RUNTIME_MESSAGE_UTF16 {
            return Err(AdapterError::InvalidEvidence);
        }
        let body = body.to_string();
        if body.len() > MAX_RUNTIME_MESSAGE_UTF8 {
            return Err(AdapterError::InvalidEvidence);
        }
        let envelope: ProbeRuntimeMessage =
            serde_json::from_str(&body).map_err(|_| AdapterError::InvalidEvidence)?;
        let expected = self.expected.get().ok_or(AdapterError::InvalidEvidence)?;
        match envelope {
            ProbeRuntimeMessage::Ready {
                protocol,
                row,
                case,
                backend,
                geometry,
            } => {
                if protocol != NATIVE_INPUT_RUNTIME_PROTOCOL_V1
                    || (ProbeRowKey { row, case, backend }) != expected
                    || self.phase.get() != ProbeMailboxPhase::AwaitingReady
                {
                    return Err(AdapterError::InvalidEvidence);
                }
                *self
                    .ready
                    .try_borrow_mut()
                    .map_err(|_| AdapterError::InvalidEvidence)? = Some(geometry.validate()?);
                self.phase.set(ProbeMailboxPhase::AwaitingResult);
            }
            ProbeRuntimeMessage::Result {
                protocol,
                row,
                case,
                backend,
                state,
            } => {
                if protocol != NATIVE_INPUT_RUNTIME_PROTOCOL_V1
                    || (ProbeRowKey { row, case, backend }) != expected
                    || self.phase.get() != ProbeMailboxPhase::AwaitingResult
                {
                    return Err(AdapterError::InvalidEvidence);
                }
                state.validate(case)?;
                *self
                    .result
                    .try_borrow_mut()
                    .map_err(|_| AdapterError::InvalidEvidence)? = Some(state);
                self.phase.set(ProbeMailboxPhase::Complete);
            }
            ProbeRuntimeMessage::Fault {
                protocol,
                row,
                case,
                backend,
                code,
            } => {
                let _ = code;
                if protocol != NATIVE_INPUT_RUNTIME_PROTOCOL_V1
                    || (ProbeRowKey { row, case, backend }) != expected
                {
                    return Err(AdapterError::InvalidEvidence);
                }
                return Err(AdapterError::InvalidEvidence);
            }
        }
        Ok(())
    }

    fn take_ready(&self) -> Result<Option<Geometry>, AdapterError> {
        if self.failed.get() {
            return Err(AdapterError::InvalidEvidence);
        }
        self.ready
            .try_borrow_mut()
            .map(|mut ready| ready.take())
            .map_err(|_| AdapterError::InvalidEvidence)
    }

    fn take_result(&self) -> Result<Option<FixtureState>, AdapterError> {
        if self.failed.get() {
            return Err(AdapterError::InvalidEvidence);
        }
        if self.phase.get() != ProbeMailboxPhase::Complete {
            return Ok(None);
        }
        let result = self
            .result
            .try_borrow_mut()
            .map_err(|_| AdapterError::InvalidEvidence)?
            .take()
            .ok_or(AdapterError::InvalidEvidence)?;
        self.expected.set(None);
        self.expected_url
            .try_borrow_mut()
            .map_err(|_| AdapterError::InvalidEvidence)?
            .take();
        self.phase.set(ProbeMailboxPhase::Idle);
        Ok(Some(result))
    }
}

struct ProbeMessageHandlerIvars {
    world: Retained<WKContentWorld>,
    handler_name: Retained<NSString>,
    mailbox: Rc<ProbeRuntimeMailbox>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumAgenticProbeMessageHandler"]
    #[ivars = ProbeMessageHandlerIvars]
    struct ProbeMessageHandler;

    unsafe impl NSObjectProtocol for ProbeMessageHandler {}

    unsafe impl WKScriptMessageHandler for ProbeMessageHandler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive_script_message(
            this: &ProbeMessageHandler,
            _controller: &WKUserContentController,
            message: &WKScriptMessage,
        ) {
            let ivars = this.ivars();
            // SAFETY: WebKit invokes this main-thread-only callback with a live
            // message; objc2 retains both properties before they are compared.
            let (world, name) = unsafe { (message.world(), message.name()) };
            if Retained::as_ptr(&world) != Retained::as_ptr(&ivars.world)
                || !name.isEqualToString(&ivars.handler_name)
            {
                ivars.mailbox.failed.set(true);
                return;
            }
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ivars.mailbox.receive(message);
            }))
            .is_err()
            {
                ivars.mailbox.failed.set(true);
            }
        }
    }
);

struct ProbeRuntimeRegistration {
    controller: Retained<WKUserContentController>,
    world: Retained<WKContentWorld>,
    handler_name: Retained<NSString>,
    _handler: Retained<ProbeMessageHandler>,
}

impl Drop for ProbeRuntimeRegistration {
    fn drop(&mut self) {
        // SAFETY: all receiver arguments are retained fields created on the
        // AppKit main thread. Exceptions are caught before crossing Rust Drop.
        let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            self.controller
                .removeScriptMessageHandlerForName_contentWorld(&self.handler_name, &self.world);
            self.controller.removeAllUserScripts();
        }));
    }
}

fn install_probe_runtime(
    mtm: MainThreadMarker,
    configuration: &WKWebViewConfiguration,
    mailbox: Rc<ProbeRuntimeMailbox>,
) -> Result<ProbeRuntimeRegistration, AdapterError> {
    // SAFETY: `configuration` is retained and used on the main thread proven
    // by `mtm`; objc2 retains the returned controller.
    let controller = unsafe { configuration.userContentController() };
    let world_name = NSString::from_str(MACOS_PROBE_CONTENT_WORLD_V1);
    // SAFETY: the non-null name and main-thread marker satisfy WebKit's class
    // method requirements; objc2 retains the returned world.
    let world = unsafe { WKContentWorld::worldWithName(&world_name, mtm) };
    let handler_name = NSString::from_str(MACOS_PROBE_HANDLER_V1);
    let handler = ProbeMessageHandler::alloc(mtm).set_ivars(ProbeMessageHandlerIvars {
        world: world.clone(),
        handler_name: handler_name.clone(),
        mailbox,
    });
    // SAFETY: the allocated object has initialized ivars and `init` returns the
    // retained subclass instance under Objective-C's initializer convention.
    let handler: Retained<ProbeMessageHandler> = unsafe { msg_send![super(handler), init] };
    let protocol_handler = ProtocolObject::from_ref(&*handler);
    // SAFETY: controller, handler, world, and name are live retained objects on
    // the main thread. Objective-C exceptions are contained at this boundary.
    objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        controller.addScriptMessageHandler_contentWorld_name(
            protocol_handler,
            &world,
            &handler_name,
        );
    }))
    .map_err(|_| AdapterError::NativeConstruction)?;
    let registration = ProbeRuntimeRegistration {
        controller: controller.clone(),
        world: world.clone(),
        handler_name,
        _handler: handler,
    };
    let source = NSString::from_str(MACOS_NATIVE_INPUT_RUNTIME_V1);
    // SAFETY: all arguments are retained Objective-C values on the main
    // thread, and the enum/content-world values are valid WebKit inputs.
    let script = unsafe {
        WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
            WKUserScript::alloc(mtm),
            &source,
            WKUserScriptInjectionTime::AtDocumentEnd,
            true,
            &world,
        )
    };
    // SAFETY: controller and script are retained on the main thread and any
    // Objective-C exception is caught before it reaches Rust.
    objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        controller.addUserScript(&script);
    }))
    .map_err(|_| AdapterError::NativeConstruction)?;
    Ok(registration)
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
    let pending = objc2::rc::autoreleasepool(|_| {
        begin_in_autorelease_pool(request_id, matrix, permit, &mut poll_control)
    })?;
    finish_teardown(pending, &mut poll_control)
}

fn begin_in_autorelease_pool(
    request_id: u64,
    matrix: &RunMatrixRequest,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
) -> Result<PendingTeardown, ProbeFailure> {
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
    let mut admission_control = NativeDispatchControl {
        permit,
        poll_control,
        deadline: run_deadline,
    };
    admission_control
        .check()
        .map_err(|error| adapter_failure(error, ProbeStage::Admit, None, None))?;
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
    // SAFETY: the retained configuration is confined to the AppKit main
    // thread; this read attests that extension authority is absent.
    if unsafe { configuration.webExtensionController() }.is_some() {
        return Err(adapter_failure(
            AdapterError::NativeConstruction,
            ProbeStage::Construct,
            None,
            None,
        ));
    }
    let runtime_mailbox = Rc::new(ProbeRuntimeMailbox::new());
    let runtime_registration =
        install_probe_runtime(mtm, &configuration, Rc::clone(&runtime_mailbox))
            .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
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
    runtime_mailbox
        .bind_page(&page)
        .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
    // SAFETY: the retained Wry page and all returned WebKit objects remain on
    // the main thread. These property reads only attest isolation state.
    let (page_configuration, page_store, page_configuration_valid) = unsafe {
        let page_configuration = page.configuration();
        let page_store = page_configuration.websiteDataStore();
        let valid = !page_store.isPersistent()
            && page_store.identifier().is_none()
            && page_configuration.webExtensionController().is_none();
        (page_configuration, page_store, valid)
    };
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
        let mut presentation_control = NativeDispatchControl {
            permit,
            poll_control,
            deadline: run_deadline,
        };
        apply_presentation(
            &app,
            &window,
            &page,
            &webview,
            matrix.presentation,
            &mut presentation_control,
        )
        .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;

        let runtime = runtime_fingerprint()
            .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
        let capabilities = macos_capabilities();
        let mut cases = Vec::with_capacity(matrix.cases.len() * matrix.backends.len());
        let mut cancelled = false;
        let mut row = 0_u16;
        for case in matrix.cases.iter().copied() {
            for backend in matrix.backends.iter().copied() {
                poll_control();
                if permit.is_cancelled() || Instant::now() >= run_deadline {
                    cases.push(cancelled_case(case, backend, matrix.presentation));
                    cancelled = true;
                    break;
                }
                row = row.checked_add(1).ok_or_else(|| {
                    adapter_failure(
                        AdapterError::InvalidEvidence,
                        ProbeStage::Admit,
                        Some(case),
                        Some(backend),
                    )
                })?;
                popup_requested.set(false);
                let geometry = load_probe_row(
                    &webview,
                    &server,
                    &runtime_mailbox,
                    &run_loop,
                    &load_generation,
                    &load_counter_failed,
                    ProbeRowKey { row, case, backend },
                    permit,
                    poll_control,
                    run_deadline,
                )
                .map_err(|error| {
                    adapter_failure(error, ProbeStage::Navigate, Some(case), Some(backend))
                })?;
                let evidence = run_case(
                    &app,
                    &window,
                    &page,
                    &run_loop,
                    &runtime_mailbox,
                    &popup_requested,
                    case,
                    backend,
                    matrix.presentation,
                    geometry,
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

    let mut execution = execution;
    if let Ok((_, _, cases)) = execution.as_mut() {
        // If a non-focused route activated this process, retain the
        // process-level transition in one row before releasing AppKit owners.
        if matrix.presentation != PresentationState::VisibleFocused
            && !app_active_before_presentation
            && app.isActive()
            && !cases.is_empty()
            && !cases.iter().any(|case| case.focus.browse_focus_was_stolen)
        {
            cases[0].focus.browse_focus_was_stolen = true;
        }
    }

    let teardown_started = Instant::now();
    let page_weak = Weak::from_retained(&page);
    let window_weak = Weak::from_retained(&window);
    let profile_store_weak = Weak::from_retained(&profile.store);
    drop(runtime_registration);
    drop(runtime_mailbox);
    drop(page_store);
    drop(page_configuration);
    drop(page);
    drop(webview);
    window.close();
    drop(host);
    drop(window);
    drop(profile);
    drop(run_loop);

    Ok(PendingTeardown {
        run_id: request_id,
        started,
        execution,
        page: page_weak,
        window: window_weak,
        profile_store: profile_store_weak,
        server,
        teardown_started,
    })
}

fn finish_teardown(
    pending: PendingTeardown,
    poll_control: &mut impl FnMut(),
) -> Result<RunEvidence, ProbeFailure> {
    let run_loop = NSRunLoop::mainRunLoop();
    pump_for(&run_loop, TEARDOWN_WINDOW, poll_control);
    let page_drained = pending.page.load().is_none();
    let window_drained = pending.window.load().is_none();
    let profile_drained = pending.profile_store.load().is_none();
    let fixture_drained = pending.server.shutdown().is_ok();
    if !page_drained || !window_drained {
        return Err(adapter_failure(
            AdapterError::NativeTeardown,
            ProbeStage::Teardown,
            None,
            None,
        ));
    }
    if !profile_drained {
        return Err(adapter_failure(
            AdapterError::ProfileTeardown,
            ProbeStage::Teardown,
            None,
            None,
        ));
    }
    if !fixture_drained {
        return Err(adapter_failure(
            AdapterError::FixtureTeardown,
            ProbeStage::Teardown,
            None,
            None,
        ));
    }
    let teardown = TeardownEvidence {
        view_closed: page_drained,
        work_drained: window_drained && profile_drained && fixture_drained,
        retained_native_views: u8::from(!page_drained),
        cleanup_ms: duration_ms_u32(pending.teardown_started.elapsed()),
    };

    let (runtime, capabilities, cases) = pending.execution?;

    let evidence = RunEvidence {
        run_id: pending.run_id,
        runtime,
        capabilities,
        peak_queue_depth: u8::from(!cases.is_empty()),
        cases,
        elapsed_ms: duration_ms_u64(pending.started.elapsed()),
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
    // SAFETY: `window` is retained and main-thread confined by its AppKit type;
    // disabling self-release preserves Rust's explicit ownership through close.
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
    control: &mut NativeDispatchControl<'_>,
) -> Result<(), AdapterError> {
    match presentation {
        PresentationState::Hidden => {
            control.check()?;
            webview
                .set_visible(false)
                .map_err(|_| AdapterError::NativeConstruction)?;
            control.check()?;
            window.orderOut(None);
        }
        PresentationState::VisibleBackground => {
            control.check()?;
            webview
                .set_visible(true)
                .map_err(|_| AdapterError::NativeConstruction)?;
            control.check()?;
            window.orderFront(None);
        }
        PresentationState::VisibleFocused => {
            control.check()?;
            webview
                .set_visible(true)
                .map_err(|_| AdapterError::NativeConstruction)?;
            control.check()?;
            app.activate();
            #[allow(deprecated)]
            {
                control.check()?;
                app.activateIgnoringOtherApps(true);
            }
            control.check()?;
            if !window.makeFirstResponder(Some(page)) {
                return Err(AdapterError::NativeConstruction);
            }
            control.check()?;
            window.makeKeyAndOrderFront(None);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn load_probe_row(
    webview: &wry::WebView,
    server: &FixtureServer,
    mailbox: &ProbeRuntimeMailbox,
    run_loop: &NSRunLoop,
    load_generation: &Cell<u16>,
    load_counter_failed: &Cell<bool>,
    key: ProbeRowKey,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    run_deadline: Instant,
) -> Result<Geometry, AdapterError> {
    let url = server
        .native_input_url(key.row, key.case, key.backend)
        .ok_or(AdapterError::InvalidEvidence)?;
    mailbox.arm(key, url.clone())?;
    let before_generation = load_generation.get();
    let mut control = NativeDispatchControl {
        permit,
        poll_control,
        deadline: run_deadline,
    };
    control.check()?;
    webview
        .load_url(&url)
        .map_err(|_| AdapterError::Navigation)?;
    let deadline = earlier_deadline(run_deadline, NAVIGATION_TIMEOUT)?;
    let mut ready = None;
    while Instant::now() < deadline {
        poll_control();
        if permit.is_cancelled() {
            return Err(AdapterError::Cancelled);
        }
        if load_counter_failed.get() || !server.is_healthy() {
            return Err(AdapterError::Navigation);
        }
        if ready.is_none() {
            ready = mailbox.take_ready()?;
        }
        if load_generation.get() > before_generation {
            if let Some(geometry) = ready {
                return Ok(geometry);
            }
        }
        pump_once(run_loop);
    }
    Err(AdapterError::Timeout)
}

#[allow(clippy::too_many_arguments)]
fn run_case(
    app: &NSApplication,
    window: &NSWindow,
    page: &WryWebView,
    run_loop: &NSRunLoop,
    mailbox: &ProbeRuntimeMailbox,
    popup_requested: &Cell<bool>,
    case: FixtureCase,
    backend: InputBackend,
    presentation: PresentationState,
    geometry: Geometry,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    run_deadline: Instant,
) -> Result<CaseEvidence, ProbeFailure> {
    let started = Instant::now();
    let resources_before = live_resource_sample();
    let app_active_before = app.isActive();
    let key_before = window.isKeyWindow();
    let focus_before = native_focus_owner(window, false);
    let outcome_hint = execute_backend(
        window,
        page,
        case,
        backend,
        presentation,
        geometry,
        permit,
        poll_control,
        run_deadline,
    )
    .map_err(|error| adapter_failure(error, ProbeStage::Execute, Some(case), Some(backend)))?;
    let focus_during = native_focus_owner(window, false);
    let state = match wait_for_runtime_result(mailbox, run_loop, permit, poll_control, run_deadline)
    {
        Ok(state) => state,
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
    };
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
    window: &NSWindow,
    page: &WryWebView,
    case: FixtureCase,
    backend: InputBackend,
    presentation: PresentationState,
    geometry: Geometry,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    deadline: Instant,
) -> Result<Option<CaseOutcome>, AdapterError> {
    let mut control = NativeDispatchControl {
        permit,
        poll_control,
        deadline,
    };
    control.check()?;
    match backend {
        InputBackend::FixedDomRecipe => {
            if case == FixtureCase::ClosedShadow {
                return Ok(Some(CaseOutcome::Unsupported));
            }
            Ok(None)
        }
        InputBackend::MacosAppKitEvent => {
            dispatch_appkit(window, page, case, geometry, &mut control)?;
            Ok(None)
        }
        InputBackend::MacosAccessibility => {
            if !accessibility_supported_case(case) {
                return Ok(Some(CaseOutcome::Unsupported));
            }
            if dispatch_accessibility(window, page, geometry, &mut control)? {
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

fn wait_for_runtime_result(
    mailbox: &ProbeRuntimeMailbox,
    run_loop: &NSRunLoop,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    run_deadline: Instant,
) -> Result<FixtureState, AdapterError> {
    let deadline = earlier_deadline(run_deadline, RUNTIME_RESULT_TIMEOUT)?;
    loop {
        poll_control();
        if permit.is_cancelled() {
            return Err(AdapterError::Cancelled);
        }
        if let Some(state) = mailbox.take_result()? {
            return Ok(state);
        }
        if Instant::now() >= deadline {
            return Err(AdapterError::Timeout);
        }
        pump_once(run_loop);
    }
}

fn dispatch_appkit(
    window: &NSWindow,
    page: &WryWebView,
    case: FixtureCase,
    geometry: Geometry,
    control: &mut NativeDispatchControl<'_>,
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
    ) {
        control.check()?;
        if !window.makeFirstResponder(Some(page)) {
            return Err(AdapterError::NativeConstruction);
        }
    }
    match case {
        FixtureCase::Drag => {
            let end_x = geometry.end_x.ok_or(AdapterError::InvalidEvidence)?;
            let end_y = geometry.end_y.ok_or(AdapterError::InvalidEvidence)?;
            let end = window_point(page, end_x, end_y)?;
            dispatch_mouse_event(window, NSEventType::MouseMoved, point, 0.0, 0, control)?;
            dispatch_mouse_event(window, NSEventType::LeftMouseDown, point, 1.0, 1, control)?;
            dispatch_mouse_event(window, NSEventType::LeftMouseDragged, end, 1.0, 1, control)?;
            dispatch_mouse_event(window, NSEventType::LeftMouseUp, end, 0.0, 1, control)?;
        }
        _ => {
            dispatch_mouse_event(window, NSEventType::MouseMoved, point, 0.0, 0, control)?;
            dispatch_mouse_event(window, NSEventType::LeftMouseDown, point, 1.0, 1, control)?;
            dispatch_mouse_event(window, NSEventType::LeftMouseUp, point, 0.0, 1, control)?;
            match case {
                FixtureCase::TextInput | FixtureCase::ContentEditable | FixtureCase::Keyboard => {
                    dispatch_key(window, "x", "x", 7, control)?
                }
                FixtureCase::Select => {
                    dispatch_key(window, "\u{f701}", "\u{f701}", 125, control)?;
                    dispatch_key(window, "\r", "\r", 36, control)?;
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
    control: &mut NativeDispatchControl<'_>,
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
    control.check()?;
    window.sendEvent(&event);
    Ok(())
}

fn dispatch_key(
    window: &NSWindow,
    characters: &str,
    unmodified: &str,
    key_code: u16,
    control: &mut NativeDispatchControl<'_>,
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
        control.check()?;
        window.sendEvent(&event);
    }
    Ok(())
}

fn dispatch_accessibility(
    window: &NSWindow,
    page: &WryWebView,
    geometry: Geometry,
    control: &mut NativeDispatchControl<'_>,
) -> Result<bool, AdapterError> {
    let point = window_point(
        page,
        geometry.x + geometry.width / 2.0,
        geometry.y + geometry.height / 2.0,
    )?;
    let screen = window.convertPointToScreen(point);
    control.check()?;
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
    control.check()?;
    let pressed: bool = {
        // SAFETY: selector availability was checked on this exact retained
        // accessibility element. The Objective-C result is a scalar BOOL and no
        // native/page object crosses the probe contract.
        unsafe { objc2::msg_send![&*element, accessibilityPerformPress] }
    };
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
    fn every_case_has_a_closed_target() {
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
        }
    }

    #[test]
    fn native_dispatch_control_polls_and_refuses_revoked_or_expired_authority() {
        let gate = zephium_agentic::ProbeGate::new();
        let permit = gate.try_start(41).expect("admit probe");
        let polls = Cell::new(0_u8);
        let mut poll = || polls.set(polls.get().saturating_add(1));
        let mut control = NativeDispatchControl {
            permit: &permit,
            poll_control: &mut poll,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        assert_eq!(control.check(), Ok(()));
        assert!(gate.cancel(41));
        assert_eq!(control.check(), Err(AdapterError::Cancelled));
        assert_eq!(polls.get(), 2);

        let gate = zephium_agentic::ProbeGate::new();
        let permit = gate.try_start(42).expect("admit probe");
        let mut control = NativeDispatchControl {
            permit: &permit,
            poll_control: &mut poll,
            deadline: Instant::now(),
        };
        assert_eq!(control.check(), Err(AdapterError::Timeout));
        assert_eq!(polls.get(), 3);
    }

    #[test]
    fn closed_shadow_has_no_dom_bypass_recipe() {
        assert!(MACOS_NATIVE_INPUT_RUNTIME_V1.contains("name === 'closed_shadow'"));
        assert!(MACOS_NATIVE_INPUT_RUNTIME_V1.contains("return 'unsupported'"));
        assert!(accessibility_supported_case(FixtureCase::ClosedShadow));
    }

    #[test]
    fn isolated_runtime_envelope_is_closed_and_versioned() {
        let ready = serde_json::from_str::<ProbeRuntimeMessage>(
            r#"{"phase":"ready","protocol":1,"row":1,"case":"button","backend":"fixed_dom_recipe","geometry":{"x":1.0,"y":2.0,"width":3.0,"height":4.0,"endX":null,"endY":null,"devicePixelRatio":2.0}}"#,
        )
        .expect("closed ready envelope");
        assert!(matches!(
            ready,
            ProbeRuntimeMessage::Ready {
                protocol: NATIVE_INPUT_RUNTIME_PROTOCOL_V1,
                row: 1,
                case: FixtureCase::Button,
                backend: InputBackend::FixedDomRecipe,
                ..
            }
        ));
        assert!(serde_json::from_str::<ProbeRuntimeMessage>(
            r##"{"phase":"ready","protocol":1,"row":1,"case":"button","backend":"fixed_dom_recipe","geometry":{"x":1.0,"y":2.0,"width":3.0,"height":4.0,"endX":null,"endY":null,"devicePixelRatio":2.0},"selector":"#button"}"##,
        )
        .is_err());
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
}
