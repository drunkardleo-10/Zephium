//! Release-excluded WebView2 native-input/isTrusted risk probe.
//!
//! This adapter owns one ordinary Wry child-HWND controller. It never casts
//! that controller to a composition controller: WebView2's composition input
//! APIs apply only to a controller created through the composition hosting
//! path, which the pinned Wry integration does not use.

use std::cell::{Cell, RefCell};
use std::num::NonZeroIsize;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use serde::Deserialize;
use serde_json::{json, Value};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
    ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl, ICoreWebView2Environment,
    ICoreWebView2Environment8,
};
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetActiveWindow, GetFocus, GetKeyboardLayout, MapVirtualKeyExW, SetFocus, MAPVK_VK_TO_VSC_EX,
    VK_DOWN, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
    GetForegroundWindow, GetParent, GetWindow, GetWindowThreadProcessId, IsChild, IsWindow,
    IsWindowVisible, MsgWaitForMultipleObjectsEx, PeekMessageW, PostQuitMessage, RegisterClassW,
    SendMessageTimeoutW, SetForegroundWindow, SetWindowPos, ShowWindow, TranslateMessage,
    CW_USEDEFAULT, GW_CHILD, HWND_BOTTOM, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT,
    SMTO_ABORTIFHUNG, SMTO_BLOCK, SMTO_ERRORONEXIT, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_QUIT, WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
};
use windows_core::{Interface, HRESULT, HSTRING, PCWSTR};
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::{
    DownloadPolicy, NavigationEvent, NavigationEventPhase, NavigationId, NewWindowResponse,
    PageClosePolicy, PermissionResponse, Rect as WryRect, WebView, WebViewBuilder,
    WebViewBuilderExtWindows as _, WebViewExtWindows as _,
};
use zephium_agentic::{
    windows_input_plan, windows_key_message_lparam, ActivationEvidence, CaseEvidence, CaseOutcome,
    EvidenceLabel, FixedProbeScript, FixtureCase, FixtureServer, FixtureTarget, FocusEvidence,
    FocusOwner, GateOutcome, InputBackend, InputEventEvidence, InputEventKind, Platform,
    PresentationState, ProbeFailure, ProbeFailureCode, ProbeRunPermit, ProbeStage,
    ResourceEvidence, RunEvidence, RunMatrixRequest, RuntimeFingerprint, TargetEvidence,
    TeardownEvidence, WindowsInputStep, WindowsProbeGeometry, WindowsProbeKey, WindowsProbePoint,
    MAX_EVENT_EVIDENCE, MAX_NATIVE_INPUT_RUNTIME_ROW, WINDOWS_PROBE_CAPABILITIES,
};

use super::{
    attest_environment, browser_process, browser_process_for_environment,
    install_browser_process_exit_observer, BrowserProcess, BrowserProcessExitObserver,
};

const RUN_TIMEOUT: Duration = Duration::from_secs(90);
const NAVIGATION_TIMEOUT: Duration = Duration::from_secs(10);
const CASE_TIMEOUT: Duration = Duration::from_secs(5);
const PROCESS_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const NATIVE_CLEANUP_TIMEOUT: Duration = Duration::from_millis(500);
const FIXTURE_SETTLE: Duration = Duration::from_millis(225);
const PUMP_SLICE: Duration = Duration::from_millis(5);
const SEND_TIMEOUT_MS: u32 = 250;
const MAX_CDP_PARAMETERS_BYTES: usize = 8 * 1_024;
const MAX_CDP_RESPONSE_UTF16_UNITS: usize = 128 * 1_024;
const MAX_CDP_RESPONSE_BYTES: usize = 128 * 1_024;
const MAX_EVALUATION_VALUE_BYTES: usize = 32 * 1_024;
const PROBE_WIDTH: i32 = 800;
const PROBE_HEIGHT: i32 = 700;

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
    FocusPolicy,
}

impl AdapterError {
    const fn code(self) -> ProbeFailureCode {
        match self {
            Self::Cancelled => ProbeFailureCode::Cancelled,
            Self::Timeout => ProbeFailureCode::Timeout,
            Self::NativeConstruction | Self::Navigation => ProbeFailureCode::HarnessFailure,
            Self::InvalidEvidence => ProbeFailureCode::VerificationFailed,
            Self::NativeTeardown => ProbeFailureCode::NativeTeardownIncomplete,
            Self::ProfileTeardown => ProbeFailureCode::ProfileTeardownIncomplete,
            Self::FixtureTeardown => ProbeFailureCode::FixtureTeardownIncomplete,
            Self::FocusPolicy => ProbeFailureCode::FocusPolicyViolation,
        }
    }

    const fn retryable(self) -> bool {
        matches!(self, Self::Timeout | Self::Navigation)
    }
}

struct ProbeHostWindow {
    hwnd: HWND,
}

impl ProbeHostWindow {
    fn new() -> Result<Self, AdapterError> {
        let module =
            unsafe { GetModuleHandleW(None) }.map_err(|_| AdapterError::NativeConstruction)?;
        let class_name = probe_window_class().ok_or(AdapterError::NativeConstruction)?;
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class_name,
                PCWSTR::null(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                PROBE_WIDTH,
                PROBE_HEIGHT,
                None,
                None,
                Some(module.into()),
                None,
            )
        }
        .map_err(|_| AdapterError::NativeConstruction)?;
        if hwnd.0.is_null() {
            return Err(AdapterError::NativeConstruction);
        }
        Ok(Self { hwnd })
    }
}

impl HasWindowHandle for ProbeHostWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = self.hwnd.0 as isize;
        let raw = NonZeroIsize::new(raw).ok_or(HandleError::Unavailable)?;
        let handle = RawWindowHandle::Win32(Win32WindowHandle::new(raw));
        // SAFETY: this object owns the HWND for the returned borrow and the
        // adapter drops every Wry child before destroying it.
        Ok(unsafe { WindowHandle::borrow_raw(handle) })
    }
}

impl Drop for ProbeHostWindow {
    fn drop(&mut self) {
        if !self.hwnd.0.is_null() {
            let _ = unsafe { DestroyWindow(self.hwnd) };
            self.hwnd = HWND::default();
        }
    }
}

fn probe_window_class() -> Option<PCWSTR> {
    static CLASS: OnceLock<Option<Vec<u16>>> = OnceLock::new();
    CLASS
        .get_or_init(|| {
            let name = "ZephiumAgenticInputProbeV1"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let module = unsafe { GetModuleHandleW(None) }.ok()?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(probe_window_proc),
                lpszClassName: PCWSTR(name.as_ptr()),
                hInstance: module.into(),
                ..Default::default()
            };
            (unsafe { RegisterClassW(&class) } != 0).then_some(name)
        })
        .as_ref()
        .map(|name| PCWSTR(name.as_ptr()))
}

unsafe extern "system" fn probe_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawGeometry {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    end_x: Option<f64>,
    end_y: Option<f64>,
    device_pixel_ratio: f64,
}

impl RawGeometry {
    fn validate(self) -> Result<WindowsProbeGeometry, AdapterError> {
        WindowsProbeGeometry::try_new(
            self.x,
            self.y,
            self.width,
            self.height,
            self.end_x,
            self.end_y,
            self.device_pixel_ratio,
        )
        .ok_or(AdapterError::InvalidEvidence)
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
            || self.events.len() > MAX_EVENT_EVIDENCE
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
        if self.activation_capture_scheduled != activation_event_observed
            || (self.drop_observed
                && !self
                    .events
                    .iter()
                    .any(|event| event.kind == InputEventKind::Drop))
        {
            return Err(AdapterError::InvalidEvidence);
        }
        Ok(())
    }
}

struct NavigationAttempt {
    expected_url: String,
    id: Option<NavigationId>,
    committed: bool,
    finished: bool,
    failed: bool,
}

#[derive(Default)]
struct NavigationTracker {
    attempt: RefCell<Option<NavigationAttempt>>,
    invalid: Cell<bool>,
}

impl NavigationTracker {
    fn arm(&self, expected_url: String) -> Result<(), AdapterError> {
        if self.invalid.get() || self.attempt.borrow().is_some() {
            return Err(AdapterError::Navigation);
        }
        *self
            .attempt
            .try_borrow_mut()
            .map_err(|_| AdapterError::Navigation)? = Some(NavigationAttempt {
            expected_url,
            id: None,
            committed: false,
            finished: false,
            failed: false,
        });
        Ok(())
    }

    fn receive(&self, event: NavigationEvent) {
        if self.receive_checked(event).is_err() {
            self.invalid.set(true);
        }
    }

    fn receive_checked(&self, event: NavigationEvent) -> Result<(), AdapterError> {
        let mut slot = self
            .attempt
            .try_borrow_mut()
            .map_err(|_| AdapterError::Navigation)?;
        let Some(attempt) = slot.as_mut() else {
            // Initial about:blank and later same-document fixture callbacks
            // carry no authority when no exact row is armed.
            return Ok(());
        };
        if event.url != attempt.expected_url {
            return Err(AdapterError::Navigation);
        }
        match event.phase {
            NavigationEventPhase::Started if attempt.id.is_none() => {
                attempt.id = Some(event.id);
            }
            NavigationEventPhase::Redirected => return Err(AdapterError::Navigation),
            NavigationEventPhase::Committed
                if attempt.id == Some(event.id) && !attempt.committed =>
            {
                attempt.committed = true;
            }
            NavigationEventPhase::Finished
                if attempt.id == Some(event.id) && attempt.committed && !attempt.finished =>
            {
                attempt.finished = true;
            }
            NavigationEventPhase::Failed if attempt.id == Some(event.id) => {
                attempt.failed = true;
            }
            NavigationEventPhase::Started
            | NavigationEventPhase::Committed
            | NavigationEventPhase::Finished
            | NavigationEventPhase::Failed => return Err(AdapterError::Navigation),
        }
        Ok(())
    }

    fn take_finished(&self) -> Result<bool, AdapterError> {
        if self.invalid.get() {
            return Err(AdapterError::Navigation);
        }
        let mut slot = self
            .attempt
            .try_borrow_mut()
            .map_err(|_| AdapterError::Navigation)?;
        let Some(attempt) = slot.as_ref() else {
            return Err(AdapterError::Navigation);
        };
        if attempt.failed {
            return Err(AdapterError::Navigation);
        }
        if attempt.finished {
            *slot = None;
            return Ok(true);
        }
        Ok(false)
    }
}

struct TeardownResult {
    view_closed: bool,
    work_drained: bool,
    profile_removed: bool,
    fixture_drained: bool,
    cleanup_ms: u32,
}

/// Runs one closed native-input matrix on the owning Windows STA thread.
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
    let server = FixtureServer::start().map_err(|_| {
        failure(
            ProbeFailureCode::HarnessFailure,
            ProbeStage::Construct,
            None,
            None,
            true,
        )
    })?;
    let profile = tempfile::tempdir().map_err(|_| {
        adapter_failure(
            AdapterError::NativeConstruction,
            ProbeStage::Construct,
            None,
            None,
        )
    })?;
    let host = ProbeHostWindow::new()
        .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
    let origin = server.url(zephium_agentic::FixtureRoute::NativeInput);
    let origin = origin
        .strip_suffix("/native-input-v1.html")
        .ok_or_else(|| {
            adapter_failure(
                AdapterError::NativeConstruction,
                ProbeStage::Construct,
                None,
                None,
            )
        })?
        .to_owned();
    let navigation = Rc::new(NavigationTracker::default());
    let navigation_callback = Rc::clone(&navigation);
    let popup_requested = Rc::new(Cell::new(false));
    let popup_callback = Rc::clone(&popup_requested);
    let captured_environment: Rc<RefCell<Option<ICoreWebView2Environment>>> =
        Rc::new(RefCell::new(None));
    let capture_failed = Rc::new(Cell::new(false));
    let environment_callback = Rc::clone(&captured_environment);
    let capture_failed_callback = Rc::clone(&capture_failed);
    let policy_origin = origin.clone();
    let mut context = wry::WebContext::new(Some(profile.path().to_path_buf()));
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(WryRect {
            position: Position::Logical(LogicalPosition::new(0.0, 0.0)),
            size: Size::Logical(LogicalSize::new(
                f64::from(PROBE_WIDTH),
                f64::from(PROBE_HEIGHT),
            )),
        })
        .with_incognito(true)
        .with_visible(false)
        .with_focused(false)
        .with_devtools(false)
        .with_autoplay(false)
        .with_fullscreen_enabled(false)
        .with_picture_in_picture_enabled(false)
        .with_general_autofill_enabled(false)
        .with_permission_handler(|_| PermissionResponse::Deny)
        .with_download_policy(DownloadPolicy::DenyWithoutMetadata)
        .with_page_close_policy(PageClosePolicy::Ignore)
        .with_navigation_handler(move |target| fixed_loopback_target(&target, &policy_origin))
        .with_navigation_event_handler(move |event| navigation_callback.receive(event))
        .with_new_window_req_handler(move |_url, _features| {
            popup_callback.set(true);
            NewWindowResponse::Deny
        })
        .with_browser_extensions_enabled(false)
        .with_browser_accelerator_keys(false)
        .with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI")
        .with_environment_created_handler(move |environment| {
            let Ok(mut slot) = environment_callback.try_borrow_mut() else {
                capture_failed_callback.set(true);
                return;
            };
            if slot.is_some() {
                capture_failed_callback.set(true);
                return;
            }
            *slot = Some(environment.clone());
        });

    let mut webview = builder.build_as_child(&host).ok();
    let environment = captured_environment
        .try_borrow_mut()
        .map(|mut slot| slot.take())
        .unwrap_or_else(|_| {
            capture_failed.set(true);
            None
        })
        .or_else(|| webview.as_ref().map(|view| view.environment()));
    let process = environment
        .as_ref()
        .and_then(|environment| browser_process_for_environment(environment).ok());
    let observer = match (environment.as_ref(), process.as_ref()) {
        (Some(environment), Some(process)) => {
            install_browser_process_exit_observer(environment, process.id(), |_| {}).ok()
        }
        _ => None,
    };

    let mut security_policy = None;
    let execution = (|| -> Result<_, ProbeFailure> {
        let environment = environment.as_ref().ok_or_else(|| {
            adapter_failure(
                AdapterError::NativeConstruction,
                ProbeStage::Construct,
                None,
                None,
            )
        })?;
        let view = webview.as_ref().ok_or_else(|| {
            adapter_failure(
                AdapterError::NativeConstruction,
                ProbeStage::Construct,
                None,
                None,
            )
        })?;
        if capture_failed.get() || process.as_ref().is_none() || observer.as_ref().is_none() {
            return Err(adapter_failure(
                AdapterError::NativeConstruction,
                ProbeStage::Construct,
                None,
                None,
            ));
        }
        attest_environment(environment, profile.path()).map_err(|_| {
            adapter_failure(
                AdapterError::NativeConstruction,
                ProbeStage::Construct,
                None,
                None,
            )
        })?;
        let view_process = browser_process(view).map_err(|_| {
            adapter_failure(
                AdapterError::NativeConstruction,
                ProbeStage::Construct,
                None,
                None,
            )
        })?;
        if process.as_ref().map(BrowserProcess::id) != Some(view_process.id())
            || !view_process.is_running()
        {
            return Err(adapter_failure(
                AdapterError::NativeConstruction,
                ProbeStage::Construct,
                None,
                None,
            ));
        }
        security_policy = Some(super::configure(view, 0.0, true, profile.path()).map_err(
            |_| {
                adapter_failure(
                    AdapterError::NativeConstruction,
                    ProbeStage::Construct,
                    None,
                    None,
                )
            },
        )?);
        apply_presentation(&host, view, matrix.presentation)
            .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
        attest_native_view(&host, view, matrix.presentation)
            .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
        verify_nonactivating_presentation(&host, view, matrix.presentation)
            .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
        let runtime = runtime_fingerprint()
            .map_err(|error| adapter_failure(error, ProbeStage::Construct, None, None))?;
        let capabilities = WINDOWS_PROBE_CAPABILITIES.to_vec();
        let core = view.webview();
        let mut cases = Vec::with_capacity(matrix.cases.len() * matrix.backends.len());
        let mut row = 0_u16;
        'matrix: for case in matrix.cases.iter().copied() {
            for backend in matrix.backends.iter().copied() {
                poll_control();
                if permit.is_cancelled() || Instant::now() >= run_deadline {
                    cases.push(cancelled_case(
                        case,
                        backend,
                        matrix.presentation,
                        environment,
                    ));
                    break 'matrix;
                }
                row = row.checked_add(1).ok_or_else(|| {
                    adapter_failure(
                        AdapterError::InvalidEvidence,
                        ProbeStage::Admit,
                        Some(case),
                        Some(backend),
                    )
                })?;
                if row > MAX_NATIVE_INPUT_RUNTIME_ROW {
                    return Err(adapter_failure(
                        AdapterError::InvalidEvidence,
                        ProbeStage::Admit,
                        Some(case),
                        Some(backend),
                    ));
                }
                popup_requested.set(false);
                let url = server.native_input_url(row, case, backend).ok_or_else(|| {
                    adapter_failure(
                        AdapterError::InvalidEvidence,
                        ProbeStage::Navigate,
                        Some(case),
                        Some(backend),
                    )
                })?;
                navigation.arm(url.clone()).map_err(|error| {
                    adapter_failure(error, ProbeStage::Navigate, Some(case), Some(backend))
                })?;
                view.load_url(&url).map_err(|_| {
                    adapter_failure(
                        AdapterError::Navigation,
                        ProbeStage::Navigate,
                        Some(case),
                        Some(backend),
                    )
                })?;
                wait_for_navigation(
                    &navigation,
                    permit,
                    &mut poll_control,
                    earlier_deadline(run_deadline, NAVIGATION_TIMEOUT).map_err(|error| {
                        adapter_failure(error, ProbeStage::Navigate, Some(case), Some(backend))
                    })?,
                )
                .map_err(|error| {
                    adapter_failure(error, ProbeStage::Navigate, Some(case), Some(backend))
                })?;
                let case_deadline =
                    earlier_deadline(run_deadline, CASE_TIMEOUT).map_err(|error| {
                        adapter_failure(error, ProbeStage::Execute, Some(case), Some(backend))
                    })?;
                let evidence = run_case(
                    &host,
                    view,
                    &core,
                    environment,
                    &popup_requested,
                    case,
                    backend,
                    matrix.presentation,
                    permit,
                    &mut poll_control,
                    case_deadline,
                )?;
                cases.push(evidence);
                if permit.is_cancelled() {
                    break 'matrix;
                }
            }
        }
        Ok((runtime, capabilities, cases))
    })();

    let teardown_started = Instant::now();
    drop(security_policy.take());
    let teardown = teardown(
        &mut webview,
        context,
        host,
        environment,
        process,
        observer,
        profile,
        server,
        &mut poll_control,
        teardown_started,
    );
    if !teardown.view_closed {
        return Err(adapter_failure(
            AdapterError::NativeTeardown,
            ProbeStage::Teardown,
            None,
            None,
        ));
    }
    if !teardown.profile_removed {
        return Err(adapter_failure(
            AdapterError::ProfileTeardown,
            ProbeStage::Teardown,
            None,
            None,
        ));
    }
    if !teardown.fixture_drained {
        return Err(adapter_failure(
            AdapterError::FixtureTeardown,
            ProbeStage::Teardown,
            None,
            None,
        ));
    }
    let (runtime, capabilities, cases) = execution?;
    let evidence = RunEvidence {
        run_id: request_id,
        runtime,
        capabilities,
        peak_queue_depth: u8::from(!cases.is_empty()),
        cases,
        elapsed_ms: duration_ms_u64(started.elapsed()),
        teardown: TeardownEvidence {
            view_closed: teardown.view_closed,
            work_drained: teardown.work_drained,
            retained_native_views: u8::from(!teardown.view_closed),
            cleanup_ms: teardown.cleanup_ms,
        },
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

#[allow(clippy::too_many_arguments)]
fn run_case(
    host: &ProbeHostWindow,
    view: &WebView,
    core: &ICoreWebView2,
    environment: &ICoreWebView2Environment,
    popup_requested: &Cell<bool>,
    case: FixtureCase,
    backend: InputBackend,
    presentation: PresentationState,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    deadline: Instant,
) -> Result<CaseEvidence, ProbeFailure> {
    let started = Instant::now();
    attest_native_view(host, view, presentation)
        .map_err(|error| adapter_failure(error, ProbeStage::Observe, Some(case), Some(backend)))?;
    let ready = evaluate_fixed(
        core,
        FixedProbeScript::Ready,
        permit,
        poll_control,
        deadline,
    )
    .map_err(|error| adapter_failure(error, ProbeStage::Observe, Some(case), Some(backend)))?;
    if ready != "ready" {
        return Err(adapter_failure(
            AdapterError::InvalidEvidence,
            ProbeStage::Observe,
            Some(case),
            Some(backend),
        ));
    }
    let geometry_json = evaluate_fixed(
        core,
        FixedProbeScript::Geometry(case),
        permit,
        poll_control,
        deadline,
    )
    .map_err(|error| adapter_failure(error, ProbeStage::Observe, Some(case), Some(backend)))?;
    let geometry = serde_json::from_str::<RawGeometry>(&geometry_json)
        .map_err(|_| {
            adapter_failure(
                AdapterError::InvalidEvidence,
                ProbeStage::Observe,
                Some(case),
                Some(backend),
            )
        })?
        .validate()
        .map_err(|error| adapter_failure(error, ProbeStage::Observe, Some(case), Some(backend)))?;
    let resources_before = live_resource_sample(environment);
    let foreground_before = unsafe { GetForegroundWindow() };
    let active_before = unsafe { GetActiveWindow() };
    let thread_focus_before = unsafe { GetFocus() };
    let focus_before =
        native_focus_owner(host, view, foreground_before, thread_focus_before, false);
    let mut focus_trace = DispatchFocusTrace::default();
    let mut observe_focus = || {
        focus_trace.observe(
            host,
            view,
            presentation,
            foreground_before,
            active_before,
            thread_focus_before,
        );
    };
    let outcome_hint = execute_backend(
        view,
        core,
        case,
        backend,
        matrix_focus_policy(presentation),
        geometry,
        permit,
        poll_control,
        &mut observe_focus,
        deadline,
    )
    .map_err(|error| adapter_failure(error, ProbeStage::Execute, Some(case), Some(backend)))?;
    let foreground_during = unsafe { GetForegroundWindow() };
    let active_during = unsafe { GetActiveWindow() };
    let thread_focus_during = unsafe { GetFocus() };
    let focus_during =
        native_focus_owner(host, view, foreground_during, thread_focus_during, false);
    pump_for(FIXTURE_SETTLE, permit, poll_control, deadline)
        .map_err(|error| adapter_failure(error, ProbeStage::Settle, Some(case), Some(backend)))?;
    let state_json = evaluate_fixed(core, FixedProbeScript::Read, permit, poll_control, deadline)
        .map_err(|error| {
        adapter_failure(error, ProbeStage::Observe, Some(case), Some(backend))
    })?;
    let state = serde_json::from_str::<FixtureState>(&state_json).map_err(|_| {
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
    let foreground_after = unsafe { GetForegroundWindow() };
    let active_after = unsafe { GetActiveWindow() };
    let thread_focus_after = unsafe { GetFocus() };
    let focus_after = native_focus_owner(
        host,
        view,
        foreground_after,
        thread_focus_after,
        target_received_focus,
    );
    attest_native_view(host, view, presentation)
        .map_err(|error| adapter_failure(error, ProbeStage::Observe, Some(case), Some(backend)))?;
    let popup_requested = popup_requested.get();
    let outcome = outcome_hint.unwrap_or_else(|| classify_outcome(case, &state, popup_requested));
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
            probe_host_became_key: focus_trace.probe_host_became_key
                || (foreground_before != host.hwnd
                    && (foreground_during == host.hwnd || foreground_after == host.hwnd))
                || (active_before != host.hwnd
                    && (active_during == host.hwnd || active_after == host.hwnd)),
            browse_focus_was_stolen: focus_trace.browse_focus_was_stolen
                || (presentation != PresentationState::VisibleFocused
                    && ((foreground_before != host.hwnd
                        && (foreground_during == host.hwnd || foreground_after == host.hwnd))
                        || (active_before != host.hwnd
                            && (active_during == host.hwnd || active_after == host.hwnd))
                        || (!focus_is_owned_by_view(view, thread_focus_before)
                            && (focus_is_owned_by_view(view, thread_focus_during)
                                || focus_is_owned_by_view(view, thread_focus_after))))),
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
            popup_requested,
            popup_observed: state.popup_observed,
            clipboard_gate: state.clipboard_gate,
        },
        resources_before,
        resources_after: live_resource_sample(environment),
        elapsed_ms: duration_ms_u32(started.elapsed()),
    })
}

const fn matrix_focus_policy(presentation: PresentationState) -> PresentationState {
    presentation
}

/// Sticky focus-transition evidence sampled between every fixed native input
/// step. A plan-level sample alone could miss an intermediate step that entered
/// the owned subtree before a later step restored the final state.
#[derive(Default)]
struct DispatchFocusTrace {
    probe_host_became_key: bool,
    browse_focus_was_stolen: bool,
}

impl DispatchFocusTrace {
    fn observe(
        &mut self,
        host: &ProbeHostWindow,
        view: &WebView,
        presentation: PresentationState,
        foreground_before: HWND,
        active_before: HWND,
        thread_focus_before: HWND,
    ) {
        let foreground = unsafe { GetForegroundWindow() };
        let active = unsafe { GetActiveWindow() };
        let thread_focus = unsafe { GetFocus() };
        let host_became_key = (foreground_before != host.hwnd && foreground == host.hwnd)
            || (active_before != host.hwnd && active == host.hwnd);
        self.probe_host_became_key |= host_became_key;
        self.browse_focus_was_stolen |= presentation != PresentationState::VisibleFocused
            && (host_became_key
                || (!focus_is_owned_by_view(view, thread_focus_before)
                    && focus_is_owned_by_view(view, thread_focus)));
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_backend(
    view: &WebView,
    core: &ICoreWebView2,
    case: FixtureCase,
    backend: InputBackend,
    presentation: PresentationState,
    geometry: WindowsProbeGeometry,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    observe_focus: &mut impl FnMut(),
    deadline: Instant,
) -> Result<Option<CaseOutcome>, AdapterError> {
    match backend {
        InputBackend::FixedDomRecipe => {
            if case == FixtureCase::ClosedShadow {
                observe_focus();
                return Ok(Some(CaseOutcome::Unsupported));
            }
            let result = evaluate_fixed(
                core,
                FixedProbeScript::DomRecipe(case),
                permit,
                poll_control,
                deadline,
            )?;
            if result != "ok" {
                return Err(AdapterError::InvalidEvidence);
            }
            observe_focus();
            Ok(None)
        }
        InputBackend::WindowsHwndInput => {
            let plan = windows_input_plan(case, geometry).ok_or(AdapterError::InvalidEvidence)?;
            dispatch_hwnd_plan(
                view,
                geometry,
                &plan,
                permit,
                poll_control,
                observe_focus,
                deadline,
            )?;
            Ok(None)
        }
        InputBackend::WindowsCdpInput => {
            let plan = windows_input_plan(case, geometry).ok_or(AdapterError::InvalidEvidence)?;
            dispatch_cdp_plan(core, &plan, permit, poll_control, observe_focus, deadline)?;
            Ok(None)
        }
        InputBackend::WindowsCompositionInput => {
            observe_focus();
            Ok(Some(CaseOutcome::Unsupported))
        }
        InputBackend::HumanBaseline => {
            observe_focus();
            Ok(Some(CaseOutcome::NeedsHuman))
        }
        InputBackend::MacosFocusedOsInput if presentation == PresentationState::VisibleFocused => {
            observe_focus();
            Ok(Some(CaseOutcome::NeedsHuman))
        }
        InputBackend::MacosFocusedOsInput => {
            observe_focus();
            Ok(Some(CaseOutcome::BlockedByPolicy))
        }
        InputBackend::MacosAppKitEvent | InputBackend::MacosAccessibility => {
            observe_focus();
            Ok(Some(CaseOutcome::Unsupported))
        }
    }
}

fn dispatch_hwnd_plan(
    view: &WebView,
    geometry: WindowsProbeGeometry,
    plan: &[WindowsInputStep],
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    observe_focus: &mut impl FnMut(),
    deadline: Instant,
) -> Result<(), AdapterError> {
    let target = OwnedDocumentHwnd::resolve(view)?;
    let mut rect = RECT::default();
    unsafe { GetClientRect(target.document, &mut rect) }
        .map_err(|_| AdapterError::NativeConstruction)?;
    let width = rect.right.saturating_sub(rect.left);
    let height = rect.bottom.saturating_sub(rect.top);
    if width <= 0 || height <= 0 || width > i32::from(i16::MAX) || height > i32::from(i16::MAX) {
        return Err(AdapterError::InvalidEvidence);
    }
    for step in plan {
        check_dispatch_control(permit, poll_control, deadline)?;
        observe_focus();
        let timeout_ms = message_timeout_ms(deadline)?;
        match *step {
            WindowsInputStep::MouseMove {
                point,
                primary_down,
            } => {
                let point = physical_point(point, geometry.device_pixel_ratio(), width, height)?;
                target.send(
                    WM_MOUSEMOVE,
                    WPARAM(usize::from(primary_down)),
                    point_lparam(point),
                    timeout_ms,
                )?;
            }
            WindowsInputStep::PrimaryDown(point) => {
                let point = physical_point(point, geometry.device_pixel_ratio(), width, height)?;
                target.send(WM_LBUTTONDOWN, WPARAM(1), point_lparam(point), timeout_ms)?;
            }
            WindowsInputStep::PrimaryUp(point) => {
                let point = physical_point(point, geometry.device_pixel_ratio(), width, height)?;
                target.send(WM_LBUTTONUP, WPARAM(0), point_lparam(point), timeout_ms)?;
            }
            WindowsInputStep::KeyDown(key) => send_key(target, key, true, timeout_ms)?,
            WindowsInputStep::TextX => send_text_x(target, timeout_ms)?,
            WindowsInputStep::KeyUp(key) => send_key(target, key, false, timeout_ms)?,
        }
        observe_focus();
        check_dispatch_control(permit, poll_control, deadline)?;
    }
    Ok(())
}

/// Resolves the only HWND that the pinned Wry integration itself treats as
/// the WebView document. `WebViewExtWindows::hwnd()` is Wry's container; its
/// fixed window procedure forwards `WM_SETFOCUS` to the first direct child.
/// Sending mouse/key messages to the container would call only that container
/// procedure and would not dispatch them to the document descendant.
#[derive(Clone, Copy)]
struct OwnedDocumentHwnd {
    container: HWND,
    document: HWND,
    owner_thread_id: u32,
    owner_process_id: u32,
    keyboard_layout: windows::Win32::UI::Input::KeyboardAndMouse::HKL,
}

impl OwnedDocumentHwnd {
    fn resolve(view: &WebView) -> Result<Self, AdapterError> {
        let container = view.hwnd();
        if container.0.is_null() {
            return Err(AdapterError::NativeConstruction);
        }
        let document = unsafe { GetWindow(container, GW_CHILD) }
            .map_err(|_| AdapterError::NativeConstruction)?;
        let mut owner_process_id = 0_u32;
        let owner_thread_id =
            unsafe { GetWindowThreadProcessId(document, Some(&mut owner_process_id)) };
        // Microsoft documents that SendMessageTimeoutW ignores its timeout
        // when the target belongs to the caller's queue. The probe never
        // attaches input queues, and it refuses the direct same-thread case so
        // the advertised per-step ceiling cannot silently become unbounded.
        if owner_thread_id == 0
            || owner_process_id == 0
            || owner_thread_id == unsafe { GetCurrentThreadId() }
        {
            return Err(AdapterError::NativeConstruction);
        }
        let keyboard_layout = unsafe { GetKeyboardLayout(owner_thread_id) };
        if keyboard_layout.is_invalid() {
            return Err(AdapterError::NativeConstruction);
        }
        let target = Self {
            container,
            document,
            owner_thread_id,
            owner_process_id,
            keyboard_layout,
        };
        target
            .is_current()
            .then_some(target)
            .ok_or(AdapterError::NativeConstruction)
    }

    fn is_current(self) -> bool {
        let mut owner_process_id = 0_u32;
        let owner_thread_id =
            unsafe { GetWindowThreadProcessId(self.document, Some(&mut owner_process_id)) };
        !self.document.0.is_null()
            && owner_thread_id == self.owner_thread_id
            && owner_process_id == self.owner_process_id
            && owner_thread_id != unsafe { GetCurrentThreadId() }
            && unsafe { GetKeyboardLayout(owner_thread_id) } == self.keyboard_layout
            && unsafe { GetWindow(self.container, GW_CHILD) }.ok() == Some(self.document)
            && unsafe { GetParent(self.document) }.ok() == Some(self.container)
            && unsafe { IsChild(self.container, self.document) }.as_bool()
    }

    fn send(
        self,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        timeout_ms: u32,
    ) -> Result<(), AdapterError> {
        if !self.is_current() {
            return Err(AdapterError::NativeConstruction);
        }
        send_message(self.document, message, wparam, lparam, timeout_ms)?;
        self.is_current()
            .then_some(())
            .ok_or(AdapterError::NativeConstruction)
    }
}

fn focus_is_owned_by_view(view: &WebView, focus: HWND) -> bool {
    let container = view.hwnd();
    !focus.0.is_null() && (focus == container || unsafe { IsChild(container, focus) }.as_bool())
}

fn physical_point(
    point: WindowsProbePoint,
    scale: f64,
    width: i32,
    height: i32,
) -> Result<(i16, i16), AdapterError> {
    let x = (point.x() * scale).round();
    let y = (point.y() * scale).round();
    if !x.is_finite()
        || !y.is_finite()
        || x < 0.0
        || y < 0.0
        || x >= f64::from(width)
        || y >= f64::from(height)
        || x > f64::from(i16::MAX)
        || y > f64::from(i16::MAX)
    {
        return Err(AdapterError::InvalidEvidence);
    }
    Ok((x as i16, y as i16))
}

fn point_lparam((x, y): (i16, i16)) -> LPARAM {
    let packed = u32::from(x as u16) | (u32::from(y as u16) << 16);
    LPARAM(packed as isize)
}

fn send_key(
    target: OwnedDocumentHwnd,
    key: WindowsProbeKey,
    down: bool,
    timeout_ms: u32,
) -> Result<(), AdapterError> {
    let virtual_key = match key {
        WindowsProbeKey::X => 0x58_u16,
        WindowsProbeKey::ArrowDown => VK_DOWN.0,
        WindowsProbeKey::Enter => VK_RETURN.0,
    };
    let mapped_scan = unsafe {
        MapVirtualKeyExW(
            u32::from(virtual_key),
            MAPVK_VK_TO_VSC_EX,
            Some(target.keyboard_layout),
        )
    };
    let lparam =
        windows_key_message_lparam(mapped_scan, down).ok_or(AdapterError::NativeConstruction)?;
    target.send(
        if down { WM_KEYDOWN } else { WM_KEYUP },
        WPARAM(usize::from(virtual_key)),
        LPARAM(lparam),
        timeout_ms,
    )
}

fn send_text_x(target: OwnedDocumentHwnd, timeout_ms: u32) -> Result<(), AdapterError> {
    let mapped_scan = unsafe {
        MapVirtualKeyExW(
            u32::from(b'X'),
            MAPVK_VK_TO_VSC_EX,
            Some(target.keyboard_layout),
        )
    };
    let lparam =
        windows_key_message_lparam(mapped_scan, true).ok_or(AdapterError::NativeConstruction)?;
    target.send(
        WM_CHAR,
        WPARAM(usize::from(b'x')),
        LPARAM(lparam),
        timeout_ms,
    )
}

fn send_message(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    timeout_ms: u32,
) -> Result<(), AdapterError> {
    if timeout_ms == 0 || timeout_ms > SEND_TIMEOUT_MS {
        return Err(AdapterError::Timeout);
    }
    let mut result = 0_usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            message,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
            timeout_ms,
            Some(&mut result),
        )
    };
    (sent.0 != 0).then_some(()).ok_or(AdapterError::Timeout)
}

fn check_dispatch_control(
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    deadline: Instant,
) -> Result<(), AdapterError> {
    poll_control();
    if permit.is_cancelled() {
        return Err(AdapterError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(AdapterError::Timeout);
    }
    Ok(())
}

fn message_timeout_ms(deadline: Instant) -> Result<u32, AdapterError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or(AdapterError::Timeout)?;
    u32::try_from(remaining.as_millis().min(u128::from(SEND_TIMEOUT_MS)))
        .ok()
        .filter(|milliseconds| *milliseconds > 0)
        .ok_or(AdapterError::Timeout)
}

fn dispatch_cdp_plan(
    core: &ICoreWebView2,
    plan: &[WindowsInputStep],
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    observe_focus: &mut impl FnMut(),
    deadline: Instant,
) -> Result<(), AdapterError> {
    for step in plan {
        let (method, parameters) = match *step {
            WindowsInputStep::MouseMove {
                point,
                primary_down,
            } => (
                FixedCdpMethod::InputDispatchMouseEvent,
                json!({
                    "type": "mouseMoved",
                    "x": point.x(),
                    "y": point.y(),
                    "button": if primary_down { "left" } else { "none" },
                    "buttons": if primary_down { 1 } else { 0 },
                    "pointerType": "mouse"
                }),
            ),
            WindowsInputStep::PrimaryDown(point) => (
                FixedCdpMethod::InputDispatchMouseEvent,
                json!({"type":"mousePressed","x":point.x(),"y":point.y(),
                    "button":"left","buttons":1,"clickCount":1,"pointerType":"mouse"}),
            ),
            WindowsInputStep::PrimaryUp(point) => (
                FixedCdpMethod::InputDispatchMouseEvent,
                json!({"type":"mouseReleased","x":point.x(),"y":point.y(),
                    "button":"left","buttons":0,"clickCount":1,"pointerType":"mouse"}),
            ),
            WindowsInputStep::KeyDown(key) => (
                FixedCdpMethod::InputDispatchKeyEvent,
                cdp_key_parameters(key, true),
            ),
            WindowsInputStep::TextX => (
                FixedCdpMethod::InputDispatchKeyEvent,
                json!({"type":"char","text":"x","unmodifiedText":"x","key":"x",
                    "code":"KeyX","windowsVirtualKeyCode":88,"nativeVirtualKeyCode":88}),
            ),
            WindowsInputStep::KeyUp(key) => (
                FixedCdpMethod::InputDispatchKeyEvent,
                cdp_key_parameters(key, false),
            ),
        };
        let response = call_cdp(
            core,
            method,
            &parameters.to_string(),
            permit,
            poll_control,
            deadline,
        )?;
        validate_cdp_response(&response)?;
        observe_focus();
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum FixedCdpMethod {
    InputDispatchMouseEvent,
    InputDispatchKeyEvent,
    RuntimeEvaluate,
}

impl FixedCdpMethod {
    const fn as_str(self) -> &'static str {
        match self {
            Self::InputDispatchMouseEvent => "Input.dispatchMouseEvent",
            Self::InputDispatchKeyEvent => "Input.dispatchKeyEvent",
            Self::RuntimeEvaluate => "Runtime.evaluate",
        }
    }
}

fn cdp_key_parameters(key: WindowsProbeKey, down: bool) -> Value {
    let (key_name, code, virtual_key) = match key {
        WindowsProbeKey::X => ("x", "KeyX", 88),
        WindowsProbeKey::ArrowDown => ("ArrowDown", "ArrowDown", 40),
        WindowsProbeKey::Enter => ("Enter", "Enter", 13),
    };
    json!({
        "type": if down { "keyDown" } else { "keyUp" },
        "key": key_name,
        "code": code,
        "windowsVirtualKeyCode": virtual_key,
        "nativeVirtualKeyCode": virtual_key
    })
}

fn evaluate_fixed(
    core: &ICoreWebView2,
    script: FixedProbeScript,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    deadline: Instant,
) -> Result<String, AdapterError> {
    let source = script.source().ok_or(AdapterError::InvalidEvidence)?;
    let parameters = json!({
        "expression": source,
        "returnByValue": true,
        "awaitPromise": false,
        "silent": true,
        "userGesture": false
    })
    .to_string();
    let response = call_cdp(
        core,
        FixedCdpMethod::RuntimeEvaluate,
        &parameters,
        permit,
        poll_control,
        deadline,
    )?;
    validate_cdp_response(&response)?;
    let value = response
        .get("result")
        .and_then(|result| result.get("value"))
        .and_then(Value::as_str)
        .ok_or(AdapterError::InvalidEvidence)?;
    if value.len() > MAX_EVALUATION_VALUE_BYTES {
        return Err(AdapterError::InvalidEvidence);
    }
    Ok(value.to_owned())
}

fn call_cdp(
    core: &ICoreWebView2,
    method: FixedCdpMethod,
    parameters: &str,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    deadline: Instant,
) -> Result<Value, AdapterError> {
    if parameters.len() > MAX_CDP_PARAMETERS_BYTES {
        return Err(AdapterError::InvalidEvidence);
    }
    let completion: Rc<RefCell<Option<Result<String, AdapterError>>>> = Rc::new(RefCell::new(None));
    let callback_failed = Rc::new(Cell::new(false));
    let handler: ICoreWebView2CallDevToolsProtocolMethodCompletedHandler = BoundedCdpCompletion {
        completion: Rc::clone(&completion),
        failed: Rc::clone(&callback_failed),
    }
    .into();
    let method = HSTRING::from(method.as_str());
    let parameters = HSTRING::from(parameters);
    unsafe { core.CallDevToolsProtocolMethod(&method, &parameters, &handler) }
        .map_err(|_| AdapterError::NativeConstruction)?;
    loop {
        poll_control();
        if permit.is_cancelled() {
            return Err(AdapterError::Cancelled);
        }
        if callback_failed.get() {
            return Err(AdapterError::InvalidEvidence);
        }
        if let Some(response) = completion
            .try_borrow_mut()
            .map_err(|_| AdapterError::InvalidEvidence)?
            .take()
        {
            let response = response?;
            return serde_json::from_str(&response).map_err(|_| AdapterError::InvalidEvidence);
        }
        if Instant::now() >= deadline {
            return Err(AdapterError::Timeout);
        }
        pump_once(deadline)?;
    }
}

/// Direct COM completion adapter used instead of webview2-com's convenience
/// callback. The convenience layer converts the returned `PCWSTR` to an
/// unbounded `String` before invoking user code, which would make a later byte
/// check only a post-allocation limit. This implementation scans at most the
/// policy ceiling plus one UTF-16 unit before reserving an exact bounded UTF-8
/// allocation.
#[windows_core::implement(ICoreWebView2CallDevToolsProtocolMethodCompletedHandler)]
struct BoundedCdpCompletion {
    completion: Rc<RefCell<Option<Result<String, AdapterError>>>>,
    failed: Rc<Cell<bool>>,
}

impl ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl for BoundedCdpCompletion_Impl {
    fn Invoke(&self, result: HRESULT, response: &PCWSTR) -> windows_core::Result<()> {
        let value = if result.is_err() {
            Err(AdapterError::InvalidEvidence)
        } else {
            borrowed_pcwstr_bounded(
                response,
                MAX_CDP_RESPONSE_UTF16_UNITS,
                MAX_CDP_RESPONSE_BYTES,
            )
            .ok_or(AdapterError::InvalidEvidence)
        };
        match self.completion.try_borrow_mut() {
            Ok(mut slot) if slot.is_none() => *slot = Some(value),
            Ok(_) | Err(_) => self.failed.set(true),
        }
        Ok(())
    }
}

fn borrowed_pcwstr_bounded(
    source: &PCWSTR,
    max_utf16_units: usize,
    max_utf8_bytes: usize,
) -> Option<String> {
    let pointer = source.as_ptr();
    if pointer.is_null() {
        return Some(String::new());
    }
    let mut length = 0_usize;
    while length <= max_utf16_units {
        // SAFETY: WebView2's completion-result contract supplies a
        // NUL-terminated string that remains valid for this callback. The
        // scan stops after the policy ceiling plus the distinguishing unit.
        if unsafe { pointer.add(length).read() } == 0 {
            // SAFETY: the bounded scan established this initialized prefix,
            // and the native string remains borrowed through conversion.
            let units = unsafe { std::slice::from_raw_parts(pointer, length) };
            let mut utf8_bytes = 0_usize;
            for character in char::decode_utf16(units.iter().copied()) {
                let character = character.ok()?;
                utf8_bytes = utf8_bytes.checked_add(character.len_utf8())?;
                if utf8_bytes > max_utf8_bytes {
                    return None;
                }
            }
            let mut value = String::new();
            value.try_reserve_exact(utf8_bytes).ok()?;
            for character in char::decode_utf16(units.iter().copied()) {
                value.push(character.ok()?);
            }
            return Some(value);
        }
        length += 1;
    }
    None
}

fn validate_cdp_response(response: &Value) -> Result<(), AdapterError> {
    if !response.is_object()
        || response.get("error").is_some()
        || response.get("exceptionDetails").is_some()
    {
        return Err(AdapterError::InvalidEvidence);
    }
    Ok(())
}

fn classify_outcome(
    case: FixtureCase,
    state: &FixtureState,
    native_popup_requested: bool,
) -> CaseOutcome {
    if case == FixtureCase::ClipboardGate
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

fn apply_presentation(
    host: &ProbeHostWindow,
    view: &WebView,
    presentation: PresentationState,
) -> Result<(), AdapterError> {
    match presentation {
        PresentationState::Hidden => {
            view.set_visible(false)
                .map_err(|_| AdapterError::NativeConstruction)?;
            let _ = unsafe { ShowWindow(host.hwnd, SW_HIDE) };
        }
        PresentationState::VisibleBackground => {
            unsafe {
                SetWindowPos(
                    host.hwnd,
                    Some(HWND_BOTTOM),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
            }
            .map_err(|_| AdapterError::NativeConstruction)?;
            let _ = unsafe { ShowWindow(host.hwnd, SW_SHOWNOACTIVATE) };
            view.set_visible(true)
                .map_err(|_| AdapterError::NativeConstruction)?;
        }
        PresentationState::VisibleFocused => {
            let _ = unsafe { ShowWindow(host.hwnd, SW_SHOW) };
            view.set_visible(true)
                .map_err(|_| AdapterError::NativeConstruction)?;
            if !unsafe { SetForegroundWindow(host.hwnd) }.as_bool() {
                return Err(AdapterError::NativeConstruction);
            }
            unsafe { SetFocus(Some(view.hwnd())) }.map_err(|_| AdapterError::NativeConstruction)?;
        }
    }
    Ok(())
}

/// Rebinds every matrix row to the native objects and viewport created by this
/// adapter. Construction success alone is not evidence that WebView2 retained
/// the same parent, visibility, or controller bounds after navigation.
fn attest_native_view(
    host: &ProbeHostWindow,
    view: &WebView,
    presentation: PresentationState,
) -> Result<(), AdapterError> {
    let container = view.hwnd();
    if !unsafe { IsWindow(Some(host.hwnd)) }.as_bool()
        || !unsafe { IsWindow(Some(container)) }.as_bool()
        || unsafe { GetParent(container) }.ok() != Some(host.hwnd)
    {
        return Err(AdapterError::NativeConstruction);
    }

    let document = OwnedDocumentHwnd::resolve(view)?;
    if !unsafe { IsWindow(Some(document.document)) }.as_bool() {
        return Err(AdapterError::NativeConstruction);
    }

    let controller = view.controller();
    let mut controller_parent = HWND::default();
    let mut controller_visible = windows_core::BOOL::default();
    let mut container_bounds = RECT::default();
    let mut controller_bounds = RECT::default();
    let dpi = unsafe { GetDpiForWindow(container) };
    let Some(expected_width) = expected_physical_extent(PROBE_WIDTH, dpi) else {
        return Err(AdapterError::NativeConstruction);
    };
    let Some(expected_height) = expected_physical_extent(PROBE_HEIGHT, dpi) else {
        return Err(AdapterError::NativeConstruction);
    };
    let expected_visible = presentation != PresentationState::Hidden;
    if unsafe { controller.ParentWindow(&mut controller_parent) }.is_err()
        || controller_parent != container
        || unsafe { controller.IsVisible(&mut controller_visible) }.is_err()
        || controller_visible.as_bool() != expected_visible
        || unsafe { IsWindowVisible(host.hwnd) }.as_bool() != expected_visible
        || unsafe { IsWindowVisible(container) }.as_bool() != expected_visible
        || unsafe { GetClientRect(container, &mut container_bounds) }.is_err()
        || unsafe { controller.Bounds(&mut controller_bounds) }.is_err()
        || container_bounds.left != 0
        || container_bounds.top != 0
        || container_bounds.right != expected_width
        || container_bounds.bottom != expected_height
        || controller_bounds.left != 0
        || controller_bounds.top != 0
        || controller_bounds.right != expected_width
        || controller_bounds.bottom != expected_height
    {
        return Err(AdapterError::NativeConstruction);
    }
    Ok(())
}

fn expected_physical_extent(logical: i32, dpi: u32) -> Option<i32> {
    let logical = u32::try_from(logical).ok().filter(|value| *value > 0)?;
    if dpi == 0 {
        return None;
    }
    let scaled = u64::from(logical)
        .checked_mul(u64::from(dpi))?
        .checked_add(48)?
        / 96;
    i32::try_from(scaled).ok().filter(|extent| *extent > 0)
}

/// Fails before the first fixture row if constructing or presenting a
/// supposedly hidden/background probe has already activated its native host
/// or moved this thread's keyboard focus into the owned WebView subtree.
/// Per-case sampling cannot discover this retrospectively because it would
/// incorrectly treat the stolen state as the row's baseline.
fn verify_nonactivating_presentation(
    host: &ProbeHostWindow,
    view: &WebView,
    presentation: PresentationState,
) -> Result<(), AdapterError> {
    if presentation == PresentationState::VisibleFocused {
        return Ok(());
    }
    let foreground = unsafe { GetForegroundWindow() };
    let active = unsafe { GetActiveWindow() };
    let focus = unsafe { GetFocus() };
    if foreground == host.hwnd || active == host.hwnd || focus_is_owned_by_view(view, focus) {
        return Err(AdapterError::FocusPolicy);
    }
    Ok(())
}

fn fixed_loopback_target(target: &str, expected_origin: &str) -> bool {
    if target == "about:blank" {
        return true;
    }
    let Ok(url) = url::Url::parse(target) else {
        return false;
    };
    if url.origin().ascii_serialization() != expected_origin
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    matches!(
        url.path(),
        "/native-input-v1.html" | "/frame-v1.html" | "/favicon.ico"
    )
}

fn wait_for_navigation(
    navigation: &NavigationTracker,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    deadline: Instant,
) -> Result<(), AdapterError> {
    loop {
        poll_control();
        if permit.is_cancelled() {
            return Err(AdapterError::Cancelled);
        }
        if navigation.take_finished()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(AdapterError::Timeout);
        }
        pump_once(deadline)?;
    }
}

fn pump_for(
    duration: Duration,
    permit: &ProbeRunPermit,
    poll_control: &mut impl FnMut(),
    absolute_deadline: Instant,
) -> Result<(), AdapterError> {
    let deadline = earlier_deadline(absolute_deadline, duration)?;
    while Instant::now() < deadline {
        poll_control();
        if permit.is_cancelled() {
            return Err(AdapterError::Cancelled);
        }
        pump_once(deadline)?;
    }
    Ok(())
}

fn pump_once(deadline: Instant) -> Result<(), AdapterError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Ok(());
    }
    let wait_ms = remaining.as_millis().clamp(1, PUMP_SLICE.as_millis()) as u32;
    unsafe {
        let _ = MsgWaitForMultipleObjectsEx(None, wait_ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        for _ in 0..256 {
            let mut message = MSG::default();
            if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                break;
            }
            if message.message == WM_QUIT {
                PostQuitMessage(message.wParam.0 as i32);
                return Err(AdapterError::NativeConstruction);
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn teardown(
    webview: &mut Option<WebView>,
    context: wry::WebContext,
    host: ProbeHostWindow,
    environment: Option<ICoreWebView2Environment>,
    process: Option<BrowserProcess>,
    observer: Option<BrowserProcessExitObserver>,
    profile: tempfile::TempDir,
    server: FixtureServer,
    poll_control: &mut impl FnMut(),
    started: Instant,
) -> TeardownResult {
    let cleanup_deadline = Instant::now() + NATIVE_CLEANUP_TIMEOUT;
    let mut view_closed = webview.is_none();
    if let Some(view) = webview.as_mut() {
        view_closed = match view.close() {
            Ok(()) => true,
            Err(mut debt) => {
                while !debt.is_complete() && Instant::now() < cleanup_deadline {
                    poll_control();
                    let _ = debt.retry();
                    let _ = pump_once(cleanup_deadline);
                }
                debt.is_complete()
            }
        };
    }
    drop(webview.take());
    for mut debt in wry::pending_webview2_cleanup_debts() {
        while !debt.is_complete() && Instant::now() < cleanup_deadline {
            poll_control();
            let _ = debt.retry();
            let _ = pump_once(cleanup_deadline);
        }
        view_closed &= debt.is_complete();
    }
    view_closed &= !wry::webview2_cleanup_overflowed();
    drop(context);
    drop(host);
    let environment_was_created = environment.is_some();
    drop(environment);

    let process_deadline = Instant::now() + PROCESS_EXIT_TIMEOUT;
    let mut profile_exit_proven =
        !environment_was_created && process.is_none() && observer.is_none();
    if let (Some(process), Some(observer)) = (process.as_ref(), observer.as_ref()) {
        while Instant::now() < process_deadline
            && !observer.is_invalid()
            && !(observer.observed_expected_exit() && process.has_exited())
        {
            poll_control();
            let _ = pump_once(process_deadline);
        }
        profile_exit_proven = !observer.is_invalid()
            && !observer.is_pending()
            && observer.observed_expected_exit()
            && process.has_exited();
    }
    drop(observer);
    drop(process);
    let profile_removed = if profile_exit_proven && view_closed {
        profile.close().is_ok()
    } else {
        let _retained_profile = profile.keep();
        false
    };
    let fixture_drained = server.shutdown().is_ok();
    TeardownResult {
        view_closed,
        work_drained: view_closed && profile_removed && fixture_drained,
        profile_removed,
        fixture_drained,
        cleanup_ms: duration_ms_u32(started.elapsed()),
    }
}

fn live_resource_sample(environment: &ICoreWebView2Environment) -> ResourceEvidence {
    let helper_processes = environment
        .cast::<ICoreWebView2Environment8>()
        .ok()
        .and_then(|environment| unsafe { environment.GetProcessInfos() }.ok())
        .and_then(|processes| {
            let mut count = 0_u32;
            unsafe { processes.Count(&mut count) }.ok()?;
            u8::try_from(count).ok()
        });
    ResourceEvidence {
        native_views: 1,
        queued_actions: 0,
        helper_processes,
        resident_bytes: None,
    }
}

fn native_focus_owner(
    host: &ProbeHostWindow,
    view: &WebView,
    foreground: HWND,
    thread_focus: HWND,
    target_focused: bool,
) -> FocusOwner {
    if target_focused {
        FocusOwner::FixtureTarget
    } else if foreground == host.hwnd
        || thread_focus == host.hwnd
        || focus_is_owned_by_view(view, thread_focus)
    {
        FocusOwner::ProbeHost
    } else {
        FocusOwner::External
    }
}

fn runtime_fingerprint() -> Result<RuntimeFingerprint, AdapterError> {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    let status = unsafe { RtlGetVersion(&mut version) };
    if status.0 < 0 {
        return Err(AdapterError::NativeConstruction);
    }
    let os_version = format!(
        "{}.{}.{}",
        version.dwMajorVersion, version.dwMinorVersion, version.dwBuildNumber
    );
    let engine_version = wry::webview_version().map_err(|_| AdapterError::NativeConstruction)?;
    Ok(RuntimeFingerprint {
        platform: Platform::Windows,
        os_version: EvidenceLabel::new(os_version).map_err(|_| AdapterError::InvalidEvidence)?,
        engine: EvidenceLabel::new("WebView2").map_err(|_| AdapterError::InvalidEvidence)?,
        engine_version: EvidenceLabel::new(engine_version)
            .map_err(|_| AdapterError::InvalidEvidence)?,
        adapter_revision: EvidenceLabel::new("native-input-m1")
            .map_err(|_| AdapterError::InvalidEvidence)?,
    })
}

fn cancelled_case(
    case: FixtureCase,
    backend: InputBackend,
    presentation: PresentationState,
    environment: &ICoreWebView2Environment,
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
        resources_before: live_resource_sample(environment),
        resources_after: live_resource_sample(environment),
        elapsed_ms: 0,
    }
}

fn earlier_deadline(absolute: Instant, duration: Duration) -> Result<Instant, AdapterError> {
    let relative = Instant::now()
        .checked_add(duration)
        .ok_or(AdapterError::Timeout)?;
    Ok(std::cmp::min(absolute, relative))
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

    #[test]
    fn navigation_tracker_requires_one_exact_identity_sequence() {
        let tracker = NavigationTracker::default();
        let url = "http://127.0.0.1:1010/native-input-v1.html?row=1&case=button&backend=windows_hwnd_input".to_owned();
        tracker.arm(url.clone()).expect("arm");
        let id = NavigationId::from_raw(17);
        for phase in [
            NavigationEventPhase::Started,
            NavigationEventPhase::Committed,
            NavigationEventPhase::Finished,
        ] {
            tracker.receive(NavigationEvent {
                id,
                phase,
                url: url.clone(),
            });
        }
        assert_eq!(tracker.take_finished(), Ok(true));
    }

    #[test]
    fn navigation_tracker_rejects_redirect_or_identity_change() {
        let tracker = NavigationTracker::default();
        let url = "http://127.0.0.1:1010/native-input-v1.html?row=1&case=button&backend=windows_hwnd_input".to_owned();
        tracker.arm(url.clone()).expect("arm");
        tracker.receive(NavigationEvent {
            id: NavigationId::from_raw(1),
            phase: NavigationEventPhase::Started,
            url: url.clone(),
        });
        tracker.receive(NavigationEvent {
            id: NavigationId::from_raw(1),
            phase: NavigationEventPhase::Redirected,
            url,
        });
        assert_eq!(tracker.take_finished(), Err(AdapterError::Navigation));
    }

    #[test]
    fn loopback_policy_is_exact_and_credential_free() {
        let origin = "http://127.0.0.1:4242";
        assert!(fixed_loopback_target(
            "http://127.0.0.1:4242/native-input-v1.html?row=1&case=button&backend=windows_hwnd_input",
            origin
        ));
        assert!(fixed_loopback_target("about:blank", origin));
        assert!(!fixed_loopback_target(
            "http://user@127.0.0.1:4242/native-input-v1.html",
            origin
        ));
        assert!(!fixed_loopback_target(
            "http://127.0.0.1:4243/native-input-v1.html",
            origin
        ));
        assert!(!fixed_loopback_target("https://example.test/", origin));
    }

    #[test]
    fn borrowed_cdp_completion_text_is_bounded_before_allocation() {
        let ascii = [b'{' as u16, b'}' as u16, 0];
        let ascii = PCWSTR(ascii.as_ptr());
        assert_eq!(borrowed_pcwstr_bounded(&ascii, 2, 2).as_deref(), Some("{}"));
        assert!(borrowed_pcwstr_bounded(&ascii, 1, 2).is_none());

        let expansion = [0x0800_u16, 0];
        let expansion = PCWSTR(expansion.as_ptr());
        assert!(borrowed_pcwstr_bounded(&expansion, 1, 2).is_none());
        assert_eq!(
            borrowed_pcwstr_bounded(&expansion, 1, 3).as_deref(),
            Some("\u{0800}")
        );
    }

    #[test]
    fn viewport_extent_matches_wry_positive_half_up_dpi_rounding() {
        assert_eq!(expected_physical_extent(800, 96), Some(800));
        assert_eq!(expected_physical_extent(700, 120), Some(875));
        assert_eq!(expected_physical_extent(1, 144), Some(2));
        assert_eq!(expected_physical_extent(800, 0), None);
        assert_eq!(expected_physical_extent(0, 96), None);
    }
}
