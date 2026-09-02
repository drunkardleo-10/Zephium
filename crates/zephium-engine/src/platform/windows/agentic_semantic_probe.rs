#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

//! Release-excluded physical qualification of the production WebView2 semantic adapter.

use std::cell::{Cell, RefCell};
use std::num::NonZeroIsize;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, OnceLock,
};
use std::time::{Duration, Instant};

use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
    ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl, ICoreWebView2Environment,
};
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Diagnostics::Debug::IsDebuggerPresent;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, GetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow, IsChild,
    MsgWaitForMultipleObjectsEx, PeekMessageW, PostQuitMessage, RegisterClassW, TranslateMessage,
    CW_USEDEFAULT, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WM_QUIT, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
};
use windows_core::{HRESULT, HSTRING, PCWSTR};
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::{
    DownloadPolicy, NewWindowResponse, PageClosePolicy, PermissionResponse, Rect, WebContext,
    WebView, WebViewBuilder, WebViewBuilderExtWindows as _, WebViewExtWindows as _,
};
use zephium_agentic::{
    encode_semantic_runtime_invocation, ContextCapabilities, ContextCapability,
    ContextConstructionProof, ContextId, ContextIdentity, ContextJoin, ContextKind,
    ContextNavigationRedirectPolicy, ContextNavigationTarget, ContextOperationId,
    ContextOwnedViewport, ContextProfileStorageClass, ContextRegistry, ContextRunId,
    ContextSettlement, EvidenceLabel, FixtureRoute, FixtureServer, FrameId, Platform,
    RuntimeFingerprint, SemanticCompleteness, SemanticFrameJoin, SemanticFrameTrust,
    SemanticInvocationId, SemanticObservationBudget, SemanticObservationId,
    SemanticObservationRequest, SemanticOperationClass, SemanticOrigin, SemanticRole,
    SemanticRuntimeBudget, SemanticRuntimeFault, SemanticRuntimePortFailure,
    SemanticRuntimeResultError, SemanticSensitivity, SemanticSnapshot, SemanticSnapshotGeneration,
    SemanticValueSummary, WindowsSemanticProbeEvidence, WindowsSemanticProbeFailure,
    WindowsSemanticProbeFailureCode, WindowsSemanticProbeMode, WindowsSemanticProbeStage,
    WindowsSemanticTeardownEvidence, MAX_CONTEXT_NAVIGATION_REDIRECTS,
};
use zephium_core::ids::ProfileId;

use crate::platform::agent_suspension::{AgentSuspendClaim, AgentSuspendNativeDisposition};

use super::{
    attest_environment, browser_process, browser_process_for_environment,
    install_browser_process_exit_observer, AgentNavigationCommit, AgentNavigationTerminal,
    AgentOwnedProfile, AgentOwnedView, AgentOwnedViewCallbacks, BrowserProcess,
    BrowserProcessExitObserver, ContentPolicyRegistration, NativeContentPolicy,
};

const RUN_TIMEOUT: Duration = Duration::from_secs(90);
const CONSTRUCTION_TIMEOUT: Duration = Duration::from_secs(15);
const NAVIGATION_TIMEOUT: Duration = Duration::from_secs(10);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);
const SUSPEND_TIMEOUT: Duration = Duration::from_secs(10);
const FAULT_TIMEOUT: Duration = Duration::from_secs(5);
const NATIVE_CLEANUP_TIMEOUT: Duration = Duration::from_millis(750);
const PROCESS_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const PUMP_SLICE: Duration = Duration::from_millis(5);
const MAX_DOCUMENT_LOADING_RETRIES: u16 = 512;
const MAX_NAVIGATION_OPERATIONS: u64 = 4;
const PROBE_WIDTH: i32 = 1_280;
const PROBE_HEIGHT: i32 = 800;

#[derive(Clone, Copy)]
struct ProbeError(WindowsSemanticProbeFailure);

impl ProbeError {
    const fn new(
        code: WindowsSemanticProbeFailureCode,
        stage: WindowsSemanticProbeStage,
        retryable: bool,
    ) -> Self {
        Self(WindowsSemanticProbeFailure {
            code,
            stage,
            retryable,
        })
    }

    const fn harness(stage: WindowsSemanticProbeStage) -> Self {
        Self::new(WindowsSemanticProbeFailureCode::HarnessFailure, stage, true)
    }

    const fn verify(stage: WindowsSemanticProbeStage) -> Self {
        Self::new(
            WindowsSemanticProbeFailureCode::VerificationFailed,
            stage,
            false,
        )
    }

    const fn timeout(stage: WindowsSemanticProbeStage) -> Self {
        Self::new(WindowsSemanticProbeFailureCode::TimedOut, stage, true)
    }
}

type ProbeResult<T> = Result<T, ProbeError>;

struct ProbeHostWindow {
    hwnd: HWND,
}

impl ProbeHostWindow {
    fn new() -> ProbeResult<Self> {
        // SAFETY: a null module name requests the current process image and
        // carries no borrowed buffer or caller-owned lifetime.
        let module = unsafe { GetModuleHandleW(None) }
            .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        let class_name = probe_window_class()
            .ok_or_else(|| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        // SAFETY: the registered class name has process lifetime, the module
        // is the current executable, and all optional owner/menu/parameter
        // pointers are intentionally null. `ProbeHostWindow` owns the result.
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
        .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        if hwnd.0.is_null() {
            return Err(ProbeError::harness(WindowsSemanticProbeStage::Construct));
        }
        Ok(Self { hwnd })
    }
}

impl HasWindowHandle for ProbeHostWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = NonZeroIsize::new(self.hwnd.0 as isize).ok_or(HandleError::Unavailable)?;
        let handle = RawWindowHandle::Win32(Win32WindowHandle::new(raw));
        // SAFETY: the host owns this HWND until every borrowed child view is closed.
        Ok(unsafe { WindowHandle::borrow_raw(handle) })
    }
}

impl Drop for ProbeHostWindow {
    fn drop(&mut self) {
        if !self.hwnd.0.is_null() {
            // SAFETY: this object uniquely owns the non-null top-level HWND;
            // the production owned view is closed before the host is dropped.
            let _ = unsafe { DestroyWindow(self.hwnd) };
            self.hwnd = HWND::default();
        }
    }
}

fn probe_window_class() -> Option<PCWSTR> {
    static CLASS: OnceLock<Option<Vec<u16>>> = OnceLock::new();
    CLASS
        .get_or_init(|| {
            let name = "ZephiumAgenticSemanticProbeV1"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            // SAFETY: a null module name requests the current process image.
            let module = unsafe { GetModuleHandleW(None) }.ok()?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(probe_window_proc),
                lpszClassName: PCWSTR(name.as_ptr()),
                hInstance: module.into(),
                ..Default::default()
            };
            // SAFETY: `name` is NUL-terminated and remains retained in the
            // `OnceLock` on success; the callback uses the Win32 ABI.
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
    // SAFETY: Win32 supplied this exact callback tuple to the registered
    // window procedure; unhandled messages are delegated unchanged.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[derive(Default)]
struct CallbackState {
    navigation: RefCell<Option<AgentNavigationTerminal>>,
    renderer_lost: Cell<bool>,
    browser_lost: Cell<bool>,
    invariant_failures: Cell<u8>,
    callback_panicked: Cell<bool>,
    suspend_callback_pending: Cell<bool>,
    suspend_callback_failed: Cell<bool>,
}

impl CallbackState {
    fn record_invariant(&self) {
        if let Some(next) = self.invariant_failures.get().checked_add(1) {
            self.invariant_failures.set(next);
        } else {
            self.callback_panicked.set(true);
        }
    }

    fn fatal(&self, allowance: CallbackAllowance) -> bool {
        self.callback_panicked.get()
            || self.suspend_callback_failed.get()
            || self.browser_lost.get()
            || (self.renderer_lost.get() && !allowance.renderer_lost)
            || self.invariant_failures.get() > allowance.invariant_failures
    }
}

#[derive(Clone, Copy)]
struct CallbackAllowance {
    invariant_failures: u8,
    renderer_lost: bool,
}

impl CallbackAllowance {
    const NONE: Self = Self {
        invariant_failures: 0,
        renderer_lost: false,
    };
    const EVENT_FLOOD: Self = Self {
        invariant_failures: 1,
        renderer_lost: false,
    };
    const RENDERER_LOST: Self = Self {
        invariant_failures: 0,
        renderer_lost: true,
    };
}

struct NativeStateGuard {
    debugger_expected: bool,
    debugger_drift: Cell<bool>,
    focus_theft: Cell<bool>,
}

impl NativeStateGuard {
    fn new(mode: WindowsSemanticProbeMode) -> ProbeResult<Self> {
        let debugger_expected = mode == WindowsSemanticProbeMode::HiddenDebuggerCoexistence;
        let debugger_attached = debugger_is_attached();
        if debugger_expected && !debugger_attached {
            return Err(ProbeError::new(
                WindowsSemanticProbeFailureCode::DebuggerRequired,
                WindowsSemanticProbeStage::Admit,
                false,
            ));
        }
        if !debugger_expected && debugger_attached {
            return Err(ProbeError::new(
                WindowsSemanticProbeFailureCode::DebuggerForbidden,
                WindowsSemanticProbeStage::Admit,
                false,
            ));
        }
        Ok(Self {
            debugger_expected,
            debugger_drift: Cell::new(false),
            focus_theft: Cell::new(false),
        })
    }

    fn sample(&self, host: &ProbeHostWindow, view: Option<&WebView>) {
        if debugger_is_attached() != self.debugger_expected {
            self.debugger_drift.set(true);
        }
        let (foreground, active, focus) = native_focus_sample();
        let view_has_focus = view.is_some_and(|view| focus_is_owned_by_view(view, focus));
        if foreground == host.hwnd || active == host.hwnd || focus == host.hwnd || view_has_focus {
            self.focus_theft.set(true);
        }
    }

    fn failed(&self) -> bool {
        self.debugger_drift.get() || self.focus_theft.get()
    }
}

fn debugger_is_attached() -> bool {
    // SAFETY: this process-state query takes no pointer and returns a scalar.
    unsafe { IsDebuggerPresent() }.as_bool()
}

/// Samples independent Win32 focus projections at one explicit observation
/// boundary. This does not claim the three calls form an atomic snapshot.
fn native_focus_sample() -> (HWND, HWND, HWND) {
    // SAFETY: these Win32 queries take no caller pointers and return borrowed
    // opaque values only; the qualifier never dereferences returned handles.
    unsafe { (GetForegroundWindow(), GetActiveWindow(), GetFocus()) }
}

fn focus_is_owned_by_view(view: &WebView, focus: HWND) -> bool {
    let container = view.hwnd();
    // SAFETY: both values are opaque HWND identities. `IsChild` performs the
    // relationship query without transferring or dereferencing either handle.
    !focus.0.is_null() && (focus == container || unsafe { IsChild(container, focus) }.as_bool())
}

#[derive(Default)]
struct ExecutionFacts {
    runtime: Option<RuntimeFingerprint>,
    snapshots: u8,
    document_epochs: u8,
    first_snapshot_verified: bool,
    replacement_snapshot_verified: bool,
    replacement_stale_state_absent: bool,
    page_world_bridge_absent: bool,
    secrets_redacted: bool,
    event_flood_refused: bool,
    recovered_after_event_flood: bool,
    renderer_loss_observed: bool,
    renderer_lost_refused: bool,
    suspend_callback_succeeded: bool,
    suspended_state_attested: bool,
    resume_state_attested: bool,
    post_resume_snapshot_verified: bool,
    suspend_ms: u32,
    redirect_chain_verified: bool,
    redirect_chain_hops_observed: u8,
    redirect_limit_refused: bool,
    redirect_limit_hops_observed: u8,
    redirect_recovery_verified: bool,
    peak_pending_invocations: u8,
    semantic_work_drained: bool,
}

struct TeardownResult {
    runtime_retired: bool,
    view_closed: bool,
    browser_process_exited: bool,
    profile_removed: bool,
    fixture_drained: bool,
    work_drained: bool,
    cleanup_ms: u32,
}

/// Runs one exact release-excluded semantic qualification on the owning STA thread.
pub(crate) fn run(
    request_id: u64,
    mode: WindowsSemanticProbeMode,
) -> Result<WindowsSemanticProbeEvidence, WindowsSemanticProbeFailure> {
    if request_id == 0 {
        return Err(ProbeError::new(
            WindowsSemanticProbeFailureCode::InvalidRequest,
            WindowsSemanticProbeStage::Admit,
            false,
        )
        .0);
    }
    let native_guard = NativeStateGuard::new(mode).map_err(|error| error.0)?;
    let started = Instant::now();
    let run_deadline = started
        .checked_add(RUN_TIMEOUT)
        .ok_or_else(|| ProbeError::timeout(WindowsSemanticProbeStage::Admit).0)?;
    let server = FixtureServer::start()
        .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct).0)?;
    let profile = tempfile::tempdir()
        .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct).0)?;
    let host = ProbeHostWindow::new().map_err(|error| error.0)?;
    native_guard.sample(&host, None);

    let mut context = None;
    let mut bootstrap = None;
    let mut environment = None;
    let mut process = None;
    let mut observer = None;
    let mut view = None;
    let mut content_policy = None;
    let mut registry = None;
    let callbacks = Rc::new(CallbackState::default());
    let mut facts = ExecutionFacts::default();

    let execution = (|| -> ProbeResult<()> {
        let construction_deadline = earlier_deadline(run_deadline, CONSTRUCTION_TIMEOUT)?;
        let mut web_context = WebContext::new(Some(profile.path().to_path_buf()));
        let captured_environment: Rc<RefCell<Option<ICoreWebView2Environment>>> =
            Rc::new(RefCell::new(None));
        let capture_failed = Rc::new(Cell::new(false));
        let environment_callback = Rc::clone(&captured_environment);
        let capture_failed_callback = Rc::clone(&capture_failed);
        let builder = WebViewBuilder::new_with_web_context(&mut web_context)
            .with_url("about:blank")
            .with_bounds(Rect {
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
            .with_hotkeys_zoom(false)
            .with_clipboard(false)
            .with_fullscreen_enabled(false)
            .with_picture_in_picture_enabled(false)
            .with_general_autofill_enabled(false)
            .with_permission_handler(|_| PermissionResponse::Deny)
            .with_download_policy(DownloadPolicy::DenyWithoutMetadata)
            .with_page_close_policy(PageClosePolicy::Ignore)
            .with_navigation_handler(|target| target == "about:blank")
            .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
            .with_browser_extensions_enabled(true)
            .with_browser_accelerator_keys(false)
            .with_default_context_menus(false)
            .with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI")
            .with_environment_created_handler(move |created| {
                let Ok(mut slot) = environment_callback.try_borrow_mut() else {
                    capture_failed_callback.set(true);
                    return;
                };
                if slot.is_some() {
                    capture_failed_callback.set(true);
                } else {
                    *slot = Some(created.clone());
                }
            });
        bootstrap = Some(
            builder
                .build_as_child(&host)
                .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?,
        );
        let captured = captured_environment
            .try_borrow_mut()
            .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?
            .take()
            .or_else(|| bootstrap.as_ref().map(|view| view.environment()))
            .ok_or_else(|| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        if capture_failed.get() {
            return Err(ProbeError::harness(WindowsSemanticProbeStage::Construct));
        }
        attest_environment(&captured, profile.path())
            .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        let browser = browser_process_for_environment(&captured)
            .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        let exit_observer = install_browser_process_exit_observer(&captured, browser.id(), |_| {})
            .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        environment = Some(captured);
        process = Some(browser);
        observer = Some(exit_observer);
        context = Some(web_context);
        close_bootstrap(&mut bootstrap, construction_deadline, &host, &native_guard)?;

        let profile_id = ProfileId::generate();
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            profile_id,
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[
                ContextCapability::Observe,
                ContextCapability::Navigate,
                ContextCapability::Suspend,
            ],
        )
        .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        let mut context_registry = ContextRegistry::new();
        context_registry
            .reserve(identity, capabilities)
            .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        let construction = context_registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1)
                    .ok_or_else(|| ProbeError::harness(WindowsSemanticProbeStage::Construct))?,
            )
            .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;

        let navigation_callbacks = Rc::clone(&callbacks);
        let renderer_callbacks = Rc::clone(&callbacks);
        let browser_callbacks = Rc::clone(&callbacks);
        let invariant_callbacks = Rc::clone(&callbacks);
        let panic_callbacks = Rc::clone(&callbacks);
        let (owned, proof) = super::build_owned_agent_view(
            &host,
            ContextOwnedViewport::STANDARD,
            environment
                .as_ref()
                .ok_or_else(|| ProbeError::harness(WindowsSemanticProbeStage::Construct))?,
            AgentOwnedProfile::automation(profile_id),
            ContextProfileStorageClass::Ephemeral,
            profile.path(),
            construction_deadline,
            AgentOwnedViewCallbacks::new(
                move |terminal| {
                    if navigation_callbacks
                        .navigation
                        .try_borrow_mut()
                        .is_ok_and(|mut slot| {
                            if slot.is_some() {
                                false
                            } else {
                                *slot = Some(terminal);
                                true
                            }
                        })
                    {
                        return;
                    }
                    navigation_callbacks.record_invariant();
                },
                || {},
                move || renderer_callbacks.renderer_lost.set(true),
                move || browser_callbacks.browser_lost.set(true),
                move || invariant_callbacks.record_invariant(),
                move || panic_callbacks.callback_panicked.set(true),
            ),
        )
        .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        if proof != ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory {
            return Err(ProbeError::verify(WindowsSemanticProbeStage::Construct));
        }
        view = Some(owned);
        content_policy = Some(
            super::install_content_policy_on_view(
                view.as_ref()
                    .ok_or_else(|| ProbeError::harness(WindowsSemanticProbeStage::Construct))?
                    .view(),
                &NativeContentPolicy::AllowAll,
            )
            .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?,
        );
        context_registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        registry = Some(context_registry);

        let owned = view
            .as_ref()
            .ok_or_else(|| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        owned
            .attest(construction_deadline)
            .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        let view_process = browser_process(owned.view())
            .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
        if process.as_ref().map(BrowserProcess::id) != Some(view_process.id())
            || !view_process.is_running()
        {
            return Err(ProbeError::harness(WindowsSemanticProbeStage::Construct));
        }
        native_guard.sample(&host, Some(owned.view()));
        if native_guard.failed() || callbacks.fatal(CallbackAllowance::NONE) {
            return Err(ProbeError::verify(WindowsSemanticProbeStage::Construct));
        }
        facts.runtime = Some(runtime_fingerprint()?);

        execute_mode(
            mode,
            identity.id(),
            view.as_mut()
                .ok_or_else(|| ProbeError::harness(WindowsSemanticProbeStage::Construct))?,
            registry
                .as_mut()
                .ok_or_else(|| ProbeError::harness(WindowsSemanticProbeStage::Construct))?,
            &server,
            &callbacks,
            &host,
            &native_guard,
            run_deadline,
            &mut facts,
        )?;
        let owned = view
            .as_ref()
            .ok_or_else(|| ProbeError::harness(WindowsSemanticProbeStage::Verify))?;
        facts.semantic_work_drained = owned.semantic_work_drained_for_audit() == Some(true);
        owned
            .attest(run_deadline)
            .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Verify))?;
        native_guard.sample(&host, Some(owned.view()));
        let allowance = match mode {
            WindowsSemanticProbeMode::HiddenEventFlood => CallbackAllowance::EVENT_FLOOD,
            WindowsSemanticProbeMode::HiddenRendererLoss => CallbackAllowance::RENDERER_LOST,
            WindowsSemanticProbeMode::HiddenFixedDocuments
            | WindowsSemanticProbeMode::HiddenRedirectLifecycle
            | WindowsSemanticProbeMode::HiddenSuspendResume
            | WindowsSemanticProbeMode::HiddenDebuggerCoexistence => CallbackAllowance::NONE,
        };
        if callbacks.fatal(allowance)
            || native_guard.failed()
            || !facts.semantic_work_drained
            || !server.is_healthy()
        {
            return Err(ProbeError::verify(WindowsSemanticProbeStage::Verify));
        }
        Ok(())
    })();

    let teardown = teardown(
        &mut bootstrap,
        &mut view,
        content_policy,
        context,
        environment,
        process,
        observer,
        host,
        profile,
        server,
        &callbacks,
        &native_guard,
        mode,
    );
    if !teardown.runtime_retired
        || !teardown.view_closed
        || !teardown.browser_process_exited
        || !teardown.profile_removed
        || !teardown.fixture_drained
        || !teardown.work_drained
    {
        return Err(ProbeError::new(
            WindowsSemanticProbeFailureCode::TeardownIncomplete,
            WindowsSemanticProbeStage::Teardown,
            true,
        )
        .0);
    }
    execution.map_err(|error| error.0)?;
    let runtime = facts
        .runtime
        .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Verify).0)?;
    Ok(WindowsSemanticProbeEvidence {
        run_id: request_id,
        runtime,
        mode,
        ephemeral_profile: true,
        in_private: true,
        extensions_absent: true,
        presentation_hidden: true,
        viewport_width: ContextOwnedViewport::STANDARD.width(),
        viewport_height: ContextOwnedViewport::STANDARD.height(),
        loopback_only: true,
        snapshots: facts.snapshots,
        document_epochs: facts.document_epochs,
        first_snapshot_verified: facts.first_snapshot_verified,
        replacement_snapshot_verified: facts.replacement_snapshot_verified,
        replacement_stale_state_absent: facts.replacement_stale_state_absent,
        page_world_bridge_absent: facts.page_world_bridge_absent,
        secrets_redacted: facts.secrets_redacted,
        event_flood_refused: facts.event_flood_refused,
        recovered_after_event_flood: facts.recovered_after_event_flood,
        renderer_loss_observed: facts.renderer_loss_observed,
        renderer_lost_refused: facts.renderer_lost_refused,
        suspend_callback_succeeded: facts.suspend_callback_succeeded,
        suspended_state_attested: facts.suspended_state_attested,
        resume_state_attested: facts.resume_state_attested,
        post_resume_snapshot_verified: facts.post_resume_snapshot_verified,
        suspend_ms: facts.suspend_ms,
        redirect_chain_verified: facts.redirect_chain_verified,
        redirect_chain_hops_observed: facts.redirect_chain_hops_observed,
        redirect_limit_refused: facts.redirect_limit_refused,
        redirect_limit_hops_observed: facts.redirect_limit_hops_observed,
        redirect_recovery_verified: facts.redirect_recovery_verified,
        debugger_attached: native_guard.debugger_expected,
        focus_theft_observed: native_guard.focus_theft.get(),
        peak_pending_invocations: facts.peak_pending_invocations,
        semantic_work_drained: facts.semantic_work_drained,
        elapsed_ms: duration_ms_u64(started.elapsed()),
        teardown: WindowsSemanticTeardownEvidence {
            runtime_retired: teardown.runtime_retired,
            view_closed: teardown.view_closed,
            browser_process_exited: teardown.browser_process_exited,
            profile_removed: teardown.profile_removed,
            fixture_drained: teardown.fixture_drained,
            work_drained: teardown.work_drained,
            retained_native_views: 0,
            cleanup_ms: teardown.cleanup_ms,
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn execute_mode(
    mode: WindowsSemanticProbeMode,
    context_id: ContextId,
    view: &mut AgentOwnedView,
    registry: &mut ContextRegistry,
    server: &FixtureServer,
    callbacks: &Rc<CallbackState>,
    host: &ProbeHostWindow,
    native_guard: &NativeStateGuard,
    run_deadline: Instant,
    facts: &mut ExecutionFacts,
) -> ProbeResult<()> {
    let mut next_operation = 2_u64;
    let mut next_invocation = 1_u64;
    let first_url = server.url(FixtureRoute::SemanticRuntime);
    let first = navigate(
        view,
        registry,
        context_id,
        take_operation(&mut next_operation)?,
        &first_url,
        callbacks,
        CallbackAllowance::NONE,
        host,
        native_guard,
        run_deadline,
    )?;
    facts.document_epochs = 1;
    let first_snapshot = capture_snapshot(
        view,
        first,
        &first_url,
        SemanticSnapshotGeneration::INITIAL,
        &mut next_invocation,
        callbacks,
        CallbackAllowance::NONE,
        host,
        native_guard,
        run_deadline,
        &mut facts.peak_pending_invocations,
    )?;
    verify_first_snapshot(&first_snapshot)?;
    let next_first_snapshot_generation = first_snapshot
        .generation()
        .next()
        .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Observe))?;
    registry
        .acknowledge_observation(context_id, first)
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Verify))?;
    facts.snapshots = 1;
    facts.first_snapshot_verified = true;
    facts.page_world_bridge_absent = true;
    facts.secrets_redacted = true;

    match mode {
        WindowsSemanticProbeMode::HiddenFixedDocuments
        | WindowsSemanticProbeMode::HiddenDebuggerCoexistence => {
            let replacement_url = server.url(FixtureRoute::SemanticRuntimeReplacement);
            let replacement = navigate(
                view,
                registry,
                context_id,
                take_operation(&mut next_operation)?,
                &replacement_url,
                callbacks,
                CallbackAllowance::NONE,
                host,
                native_guard,
                run_deadline,
            )?;
            facts.document_epochs = 2;
            let snapshot = capture_snapshot(
                view,
                replacement,
                &replacement_url,
                SemanticSnapshotGeneration::INITIAL,
                &mut next_invocation,
                callbacks,
                CallbackAllowance::NONE,
                host,
                native_guard,
                run_deadline,
                &mut facts.peak_pending_invocations,
            )?;
            verify_replacement_snapshot(&snapshot)?;
            registry
                .acknowledge_observation(context_id, replacement)
                .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Verify))?;
            facts.snapshots = 2;
            facts.replacement_snapshot_verified = true;
            facts.replacement_stale_state_absent = true;
        }
        WindowsSemanticProbeMode::HiddenRedirectLifecycle => {
            let loop_url = server.url(FixtureRoute::SemanticRedirectLoopA);
            let loop_target = ContextNavigationTarget::parse(&loop_url)
                .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
            let loop_policy = ContextNavigationRedirectPolicy::same_origin(&loop_target)
                .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
            let refused_hops = refuse_redirect_limit(
                view,
                registry,
                context_id,
                take_operation(&mut next_operation)?,
                &loop_url,
                loop_policy,
                callbacks,
                host,
                native_guard,
                run_deadline,
            )?;
            if refused_hops != MAX_CONTEXT_NAVIGATION_REDIRECTS as u8 {
                return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
            }
            facts.redirect_limit_refused = true;
            facts.redirect_limit_hops_observed = refused_hops;

            let redirect_url = server.url(FixtureRoute::SemanticRedirectStart);
            let final_url = server.url(FixtureRoute::SemanticRedirectFinal);
            let redirect_target = ContextNavigationTarget::parse(&redirect_url)
                .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
            let redirect_policy = ContextNavigationRedirectPolicy::same_origin(&redirect_target)
                .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
            let (redirected, chain_hops) = navigate_redirect(
                view,
                registry,
                context_id,
                take_operation(&mut next_operation)?,
                &redirect_url,
                &final_url,
                redirect_policy,
                callbacks,
                host,
                native_guard,
                run_deadline,
            )?;
            if chain_hops != 2 {
                return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
            }
            facts.document_epochs = 2;
            let snapshot = capture_snapshot(
                view,
                redirected,
                &final_url,
                SemanticSnapshotGeneration::INITIAL,
                &mut next_invocation,
                callbacks,
                CallbackAllowance::NONE,
                host,
                native_guard,
                run_deadline,
                &mut facts.peak_pending_invocations,
            )?;
            verify_first_snapshot(&snapshot)?;
            registry
                .acknowledge_observation(context_id, redirected)
                .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Verify))?;
            facts.snapshots = 2;
            facts.redirect_chain_verified = true;
            facts.redirect_chain_hops_observed = chain_hops;
            facts.redirect_recovery_verified = true;
        }
        WindowsSemanticProbeMode::HiddenSuspendResume => {
            wait_for_semantic_drain(
                view,
                callbacks,
                CallbackAllowance::NONE,
                host,
                native_guard,
                run_deadline,
            )?;
            let suspended_at = Instant::now();
            let resumed = suspend_and_resume(
                view,
                registry,
                context_id,
                take_operation(&mut next_operation)?,
                take_operation(&mut next_operation)?,
                callbacks,
                host,
                native_guard,
                run_deadline,
            )?;
            facts.suspend_callback_succeeded = true;
            facts.suspended_state_attested = true;
            facts.resume_state_attested = true;
            facts.suspend_ms = duration_ms_u32(resumed.suspended_at.duration_since(suspended_at));
            let snapshot = capture_snapshot(
                view,
                resumed.context,
                &first_url,
                next_first_snapshot_generation,
                &mut next_invocation,
                callbacks,
                CallbackAllowance::NONE,
                host,
                native_guard,
                run_deadline,
                &mut facts.peak_pending_invocations,
            )?;
            verify_first_snapshot(&snapshot)?;
            registry
                .acknowledge_observation(context_id, resumed.context)
                .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Verify))?;
            facts.snapshots = 2;
            facts.post_resume_snapshot_verified = true;
        }
        WindowsSemanticProbeMode::HiddenEventFlood => {
            let flood_url = server.url(FixtureRoute::SemanticRuntimeEventFlood);
            let flood = navigate(
                view,
                registry,
                context_id,
                take_operation(&mut next_operation)?,
                &flood_url,
                callbacks,
                CallbackAllowance::NONE,
                host,
                native_guard,
                run_deadline,
            )?;
            facts.document_epochs = 2;
            let outcome = capture_outcome(
                view,
                flood,
                &flood_url,
                SemanticSnapshotGeneration::INITIAL,
                &mut next_invocation,
                callbacks,
                CallbackAllowance::EVENT_FLOOD,
                host,
                native_guard,
                run_deadline,
                &mut facts.peak_pending_invocations,
            )?;
            if outcome != Err(SemanticRuntimePortFailure::Transport)
                || callbacks.invariant_failures.get() != 1
            {
                return Err(ProbeError::verify(WindowsSemanticProbeStage::Fault));
            }
            wait_for_semantic_drain(
                view,
                callbacks,
                CallbackAllowance::EVENT_FLOOD,
                host,
                native_guard,
                run_deadline,
            )?;
            facts.event_flood_refused = true;

            let replacement_url = server.url(FixtureRoute::SemanticRuntimeReplacement);
            let replacement = navigate(
                view,
                registry,
                context_id,
                take_operation(&mut next_operation)?,
                &replacement_url,
                callbacks,
                CallbackAllowance::EVENT_FLOOD,
                host,
                native_guard,
                run_deadline,
            )?;
            facts.document_epochs = 3;
            let snapshot = capture_snapshot(
                view,
                replacement,
                &replacement_url,
                SemanticSnapshotGeneration::INITIAL,
                &mut next_invocation,
                callbacks,
                CallbackAllowance::EVENT_FLOOD,
                host,
                native_guard,
                run_deadline,
                &mut facts.peak_pending_invocations,
            )?;
            verify_replacement_snapshot(&snapshot)?;
            registry
                .acknowledge_observation(context_id, replacement)
                .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Verify))?;
            facts.snapshots = 2;
            facts.replacement_snapshot_verified = true;
            facts.replacement_stale_state_absent = true;
            facts.recovered_after_event_flood = true;
        }
        WindowsSemanticProbeMode::HiddenRendererLoss => {
            request_fixed_renderer_crash(view)?;
            let deadline = earlier_deadline(run_deadline, FAULT_TIMEOUT)?;
            while !callbacks.renderer_lost.get() && Instant::now() < deadline {
                if callbacks.fatal(CallbackAllowance::RENDERER_LOST) {
                    return Err(ProbeError::verify(WindowsSemanticProbeStage::Fault));
                }
                pump_and_sample(deadline, host, Some(view.view()), native_guard)?;
            }
            if !callbacks.renderer_lost.get() {
                return Err(ProbeError::timeout(WindowsSemanticProbeStage::Fault));
            }
            registry
                .renderer_lost(context_id, first)
                .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Fault))?;
            let outcome = capture_outcome(
                view,
                first,
                &first_url,
                next_first_snapshot_generation,
                &mut next_invocation,
                callbacks,
                CallbackAllowance::RENDERER_LOST,
                host,
                native_guard,
                run_deadline,
                &mut facts.peak_pending_invocations,
            )?;
            if outcome != Err(SemanticRuntimePortFailure::RendererLost) {
                return Err(ProbeError::verify(WindowsSemanticProbeStage::Fault));
            }
            facts.renderer_loss_observed = true;
            facts.renderer_lost_refused = true;
        }
    }
    Ok(())
}

struct SuspendResumeResult {
    context: ContextJoin,
    suspended_at: Instant,
}

#[allow(clippy::too_many_arguments)]
fn suspend_and_resume(
    view: &AgentOwnedView,
    registry: &mut ContextRegistry,
    context_id: ContextId,
    suspend_operation: u64,
    resume_operation: u64,
    callbacks: &Rc<CallbackState>,
    host: &ProbeHostWindow,
    native_guard: &NativeStateGuard,
    run_deadline: Instant,
) -> ProbeResult<SuspendResumeResult> {
    if callbacks.suspend_callback_pending.get()
        || callbacks.suspend_callback_failed.get()
        || callbacks.fatal(CallbackAllowance::NONE)
    {
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Suspend));
    }
    let suspend = registry
        .begin_suspend(
            context_id,
            ContextOperationId::new(suspend_operation)
                .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Suspend))?,
        )
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Suspend))?;
    callbacks.suspend_callback_pending.set(true);
    let claim = AgentSuspendClaim::new();
    let callback_claim = claim.clone();
    let result = Rc::new(RefCell::new(None));
    let callback_result = Rc::clone(&result);
    let completion_callbacks = Rc::clone(callbacks);
    let panic_callbacks = Rc::clone(callbacks);
    if view
        .try_suspend(
            move |native_succeeded| {
                let disposition = callback_claim.native_completed();
                completion_callbacks.suspend_callback_pending.set(false);
                let Ok(mut slot) = callback_result.try_borrow_mut() else {
                    completion_callbacks.suspend_callback_failed.set(true);
                    return;
                };
                if slot.is_some() {
                    completion_callbacks.suspend_callback_failed.set(true);
                } else {
                    *slot = Some((disposition, native_succeeded, Instant::now()));
                }
            },
            move || {
                panic_callbacks.suspend_callback_pending.set(false);
                panic_callbacks.suspend_callback_failed.set(true);
            },
        )
        .is_err()
    {
        callbacks.suspend_callback_pending.set(false);
        claim.retire();
        let _ = registry.settle_suspend(context_id, suspend, ContextSettlement::Refused);
        return Err(ProbeError::harness(WindowsSemanticProbeStage::Suspend));
    }

    let deadline = earlier_deadline(run_deadline, SUSPEND_TIMEOUT)?;
    while result.borrow().is_none()
        && callbacks.suspend_callback_pending.get()
        && !callbacks.fatal(CallbackAllowance::NONE)
        && !native_guard.failed()
        && Instant::now() < deadline
    {
        pump_and_sample(deadline, host, Some(view.view()), native_guard)?;
    }
    let terminal = result
        .try_borrow_mut()
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Suspend))?
        .take();
    let Some((disposition, native_succeeded, suspended_at)) = terminal else {
        let _ = claim.timeout();
        claim.retire();
        let _ = registry.settle_suspend(context_id, suspend, ContextSettlement::Refused);
        return if Instant::now() >= deadline {
            Err(ProbeError::timeout(WindowsSemanticProbeStage::Suspend))
        } else {
            Err(ProbeError::verify(WindowsSemanticProbeStage::Suspend))
        };
    };
    let suspended = view.attest_suspension_state() == Ok(true);
    native_guard.sample(host, Some(view.view()));
    if disposition != AgentSuspendNativeDisposition::Terminal
        || !native_succeeded
        || !suspended
        || callbacks.suspend_callback_pending.get()
        || callbacks.fatal(CallbackAllowance::NONE)
        || native_guard.failed()
    {
        if suspended {
            let _ = view.resume_and_attest_active();
        }
        let _ = registry.settle_suspend(context_id, suspend, ContextSettlement::Refused);
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Suspend));
    }
    registry
        .settle_suspend(context_id, suspend, ContextSettlement::Applied)
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Suspend))?;

    let resume = registry
        .begin_resume(
            context_id,
            ContextOperationId::new(resume_operation)
                .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Suspend))?,
        )
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Suspend))?;
    let active = view.resume_and_attest_active() == Ok(true);
    native_guard.sample(host, Some(view.view()));
    registry
        .settle_resume(
            context_id,
            resume,
            if active {
                ContextSettlement::Applied
            } else {
                ContextSettlement::Refused
            },
        )
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Suspend))?;
    if !active || callbacks.fatal(CallbackAllowance::NONE) || native_guard.failed() {
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Suspend));
    }
    let context = registry
        .join(context_id)
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Suspend))?;
    Ok(SuspendResumeResult {
        context,
        suspended_at,
    })
}

fn take_operation(next: &mut u64) -> ProbeResult<u64> {
    let operation = *next;
    if operation == 0 || operation > MAX_NAVIGATION_OPERATIONS {
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
    }
    *next = next
        .checked_add(1)
        .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
    Ok(operation)
}

#[allow(clippy::too_many_arguments)]
fn navigate(
    view: &mut AgentOwnedView,
    registry: &mut ContextRegistry,
    id: ContextId,
    operation_id: u64,
    url: &str,
    callbacks: &CallbackState,
    allowance: CallbackAllowance,
    host: &ProbeHostWindow,
    native_guard: &NativeStateGuard,
    run_deadline: Instant,
) -> ProbeResult<ContextJoin> {
    let (context, redirects) = navigate_with_redirects(
        view,
        registry,
        id,
        operation_id,
        url,
        url,
        None,
        callbacks,
        allowance,
        host,
        native_guard,
        run_deadline,
    )?;
    if redirects != 0 {
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
    }
    Ok(context)
}

#[allow(clippy::too_many_arguments)]
fn navigate_redirect(
    view: &mut AgentOwnedView,
    registry: &mut ContextRegistry,
    id: ContextId,
    operation_id: u64,
    url: &str,
    expected_final_url: &str,
    redirect_policy: ContextNavigationRedirectPolicy,
    callbacks: &CallbackState,
    host: &ProbeHostWindow,
    native_guard: &NativeStateGuard,
    run_deadline: Instant,
) -> ProbeResult<(ContextJoin, u8)> {
    navigate_with_redirects(
        view,
        registry,
        id,
        operation_id,
        url,
        expected_final_url,
        Some(redirect_policy),
        callbacks,
        CallbackAllowance::NONE,
        host,
        native_guard,
        run_deadline,
    )
}

#[allow(clippy::too_many_arguments)]
fn navigate_with_redirects(
    view: &mut AgentOwnedView,
    registry: &mut ContextRegistry,
    id: ContextId,
    operation_id: u64,
    url: &str,
    expected_final_url: &str,
    redirect_policy: Option<ContextNavigationRedirectPolicy>,
    callbacks: &CallbackState,
    allowance: CallbackAllowance,
    host: &ProbeHostWindow,
    native_guard: &NativeStateGuard,
    run_deadline: Instant,
) -> ProbeResult<(ContextJoin, u8)> {
    if callbacks.fatal(allowance) || callbacks.navigation.borrow().is_some() {
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
    }
    let operation = registry
        .begin_navigation(
            id,
            ContextOperationId::new(operation_id)
                .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?,
        )
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
    let target = ContextNavigationTarget::parse(url)
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
    let expected_final = ContextNavigationTarget::parse(expected_final_url)
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
    if view.prepare_semantic_document_load().is_err() {
        let _ = registry.settle_navigation(id, operation, ContextSettlement::Refused);
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
    }
    let terminal_claimed = Arc::new(AtomicBool::new(false));
    let armed = match redirect_policy {
        Some(policy) => view.navigation().arm_with_redirect_policy(
            operation,
            target,
            policy,
            Arc::clone(&terminal_claimed),
        ),
        None => view
            .navigation()
            .arm(operation, target, Arc::clone(&terminal_claimed)),
    };
    if armed.is_err() {
        let _ = registry.settle_navigation(id, operation, ContextSettlement::Refused);
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
    }
    if view.view().load_url(url).is_err() {
        let _ = view.navigation().disarm(operation);
        let _ = registry.settle_navigation(id, operation, ContextSettlement::Refused);
        return Err(ProbeError::harness(WindowsSemanticProbeStage::Navigate));
    }

    let deadline = earlier_deadline(run_deadline, NAVIGATION_TIMEOUT)?;
    while !callbacks.fatal(allowance)
        && callbacks.navigation.borrow().is_none()
        && Instant::now() < deadline
    {
        pump_and_sample(deadline, host, Some(view.view()), native_guard)?;
    }
    let terminal = callbacks
        .navigation
        .try_borrow_mut()
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?
        .take();
    let audit = view
        .navigation()
        .redirect_probe_audit(operation)
        .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
    let applied = terminal.is_some_and(|terminal| {
        terminal.operation() == operation
            && matches!(
                terminal.into_outcome(),
                Ok(AgentNavigationCommit::Web(committed)) if committed == expected_final
            )
    });
    let mut finished = false;
    if applied {
        loop {
            match view.navigation().document_finished_for_audit(operation) {
                Some(true) => {
                    finished = true;
                    break;
                }
                Some(false) => {}
                None => break,
            }
            if callbacks.fatal(allowance) || native_guard.failed() || Instant::now() >= deadline {
                break;
            }
            if pump_and_sample(deadline, host, Some(view.view()), native_guard).is_err() {
                break;
            }
        }
    }
    let disarmed = view.navigation().disarm(operation);
    registry
        .settle_navigation(
            id,
            operation,
            if applied {
                ContextSettlement::Applied
            } else {
                ContextSettlement::Refused
            },
        )
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
    if !applied
        || !finished
        || !disarmed
        || callbacks.fatal(allowance)
        || native_guard.failed()
        || !terminal_claimed.load(Ordering::Acquire)
        || audit.limit_refused()
    {
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
    }
    let context = registry
        .join(id)
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
    Ok((context, audit.redirects_observed()))
}

#[allow(clippy::too_many_arguments)]
fn refuse_redirect_limit(
    view: &mut AgentOwnedView,
    registry: &mut ContextRegistry,
    id: ContextId,
    operation_id: u64,
    url: &str,
    redirect_policy: ContextNavigationRedirectPolicy,
    callbacks: &CallbackState,
    host: &ProbeHostWindow,
    native_guard: &NativeStateGuard,
    run_deadline: Instant,
) -> ProbeResult<u8> {
    if callbacks.fatal(CallbackAllowance::NONE) || callbacks.navigation.borrow().is_some() {
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
    }
    let operation = registry
        .begin_navigation(
            id,
            ContextOperationId::new(operation_id)
                .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?,
        )
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
    let target = ContextNavigationTarget::parse(url)
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
    if view.prepare_semantic_document_load().is_err() {
        let _ = registry.settle_navigation(id, operation, ContextSettlement::Refused);
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
    }
    let terminal_claimed = Arc::new(AtomicBool::new(false));
    if view
        .navigation()
        .arm_with_redirect_policy(
            operation,
            target,
            redirect_policy,
            Arc::clone(&terminal_claimed),
        )
        .is_err()
    {
        let _ = registry.settle_navigation(id, operation, ContextSettlement::Refused);
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
    }
    if view.view().load_url(url).is_err() {
        let _ = view.navigation().disarm(operation);
        let _ = registry.settle_navigation(id, operation, ContextSettlement::Refused);
        return Err(ProbeError::harness(WindowsSemanticProbeStage::Navigate));
    }

    let deadline = earlier_deadline(run_deadline, NAVIGATION_TIMEOUT)?;
    while !callbacks.fatal(CallbackAllowance::NONE)
        && callbacks.navigation.borrow().is_none()
        && Instant::now() < deadline
    {
        pump_and_sample(deadline, host, Some(view.view()), native_guard)?;
    }
    let terminal = callbacks
        .navigation
        .try_borrow_mut()
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?
        .take();
    let refused = terminal.is_some_and(|terminal| {
        terminal.operation() == operation
            && terminal.into_outcome() == Err(zephium_agentic::ContextPortFailure::NativeRefused)
    });
    let audit = view
        .navigation()
        .redirect_probe_audit(operation)
        .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
    super::stop_loading(view.view());
    native_guard.sample(host, Some(view.view()));
    let disarmed = view.navigation().disarm(operation);
    registry
        .settle_navigation(id, operation, ContextSettlement::Refused)
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))?;
    if !refused
        || !audit.limit_refused()
        || !disarmed
        || callbacks.fatal(CallbackAllowance::NONE)
        || native_guard.failed()
        || !terminal_claimed.load(Ordering::Acquire)
    {
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Navigate));
    }
    Ok(audit.redirects_observed())
}

#[allow(clippy::too_many_arguments)]
fn capture_snapshot(
    view: &AgentOwnedView,
    context: ContextJoin,
    url: &str,
    first_generation: SemanticSnapshotGeneration,
    next_invocation: &mut u64,
    callbacks: &CallbackState,
    allowance: CallbackAllowance,
    host: &ProbeHostWindow,
    native_guard: &NativeStateGuard,
    run_deadline: Instant,
    peak_pending: &mut u8,
) -> ProbeResult<SemanticSnapshot> {
    capture_outcome(
        view,
        context,
        url,
        first_generation,
        next_invocation,
        callbacks,
        allowance,
        host,
        native_guard,
        run_deadline,
        peak_pending,
    )?
    .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Observe))
}

#[allow(clippy::too_many_arguments)]
fn capture_outcome(
    view: &AgentOwnedView,
    context: ContextJoin,
    url: &str,
    first_generation: SemanticSnapshotGeneration,
    next_invocation: &mut u64,
    callbacks: &CallbackState,
    allowance: CallbackAllowance,
    host: &ProbeHostWindow,
    native_guard: &NativeStateGuard,
    run_deadline: Instant,
    peak_pending: &mut u8,
) -> ProbeResult<Result<SemanticSnapshot, SemanticRuntimePortFailure>> {
    let origin = SemanticOrigin::parse(url)
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Observe))?;
    let frame = SemanticFrameJoin::try_new(
        context,
        FrameId::MAIN,
        context.frame_generation(),
        origin,
        SemanticFrameTrust::SameOrigin,
    )
    .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Observe))?;
    let deadline = earlier_deadline(run_deadline, SNAPSHOT_TIMEOUT)?;

    for retry in 0..MAX_DOCUMENT_LOADING_RETRIES {
        if callbacks.fatal(allowance) || native_guard.failed() {
            return Err(ProbeError::verify(WindowsSemanticProbeStage::Observe));
        }
        if Instant::now() >= deadline {
            return Err(ProbeError::timeout(WindowsSemanticProbeStage::Observe));
        }
        let invocation_value = *next_invocation;
        *next_invocation = next_invocation
            .checked_add(1)
            .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Observe))?;
        let invocation_id = SemanticInvocationId::new(invocation_value)
            .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Observe))?;
        let generation = first_generation
            .get()
            .checked_add(u64::from(retry))
            .and_then(SemanticSnapshotGeneration::new)
            .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Observe))?;
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(invocation_value)
                .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Observe))?,
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let invocation = encode_semantic_runtime_invocation(
            &request,
            frame.clone(),
            invocation_id,
            generation,
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Observe))?;
        let result = Rc::new(RefCell::new(None));
        let completion_failed = Rc::new(Cell::new(false));
        let completion = Rc::clone(&result);
        let completion_failure = Rc::clone(&completion_failed);
        let dispatch = view.dispatch_semantic(invocation, move |outcome| {
            let Ok(mut slot) = completion.try_borrow_mut() else {
                completion_failure.set(true);
                return;
            };
            if slot.is_some() {
                completion_failure.set(true);
            } else {
                *slot = Some(outcome);
            }
        });
        if dispatch.is_ok() {
            *peak_pending = (*peak_pending).max(1);
        } else if result.borrow().is_none() {
            return Err(ProbeError::verify(WindowsSemanticProbeStage::Observe));
        }
        while result.borrow().is_none()
            && !completion_failed.get()
            && !callbacks.fatal(allowance)
            && !native_guard.failed()
            && Instant::now() < deadline
        {
            pump_and_sample(deadline, host, Some(view.view()), native_guard)?;
        }
        if completion_failed.get() || callbacks.fatal(allowance) || native_guard.failed() {
            return Err(ProbeError::verify(WindowsSemanticProbeStage::Observe));
        }
        let outcome = result
            .try_borrow_mut()
            .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Observe))?
            .take();
        let Some(outcome) = outcome else {
            if view
                .semantic()
                .is_some_and(|semantic| semantic.timeout(invocation_id))
            {
                return Err(ProbeError::timeout(WindowsSemanticProbeStage::Observe));
            }
            return Err(ProbeError::verify(WindowsSemanticProbeStage::Observe));
        };
        match outcome {
            Err(SemanticRuntimePortFailure::Result(SemanticRuntimeResultError::Runtime(
                SemanticRuntimeFault::DocumentLoading,
            ))) => {
                pump_and_sample(deadline, host, Some(view.view()), native_guard)?;
            }
            terminal => return Ok(terminal),
        }
    }
    Err(ProbeError::timeout(WindowsSemanticProbeStage::Observe))
}

fn wait_for_semantic_drain(
    view: &AgentOwnedView,
    callbacks: &CallbackState,
    allowance: CallbackAllowance,
    host: &ProbeHostWindow,
    native_guard: &NativeStateGuard,
    run_deadline: Instant,
) -> ProbeResult<()> {
    let deadline = earlier_deadline(run_deadline, SNAPSHOT_TIMEOUT)?;
    loop {
        if callbacks.fatal(allowance) || native_guard.failed() {
            return Err(ProbeError::verify(WindowsSemanticProbeStage::Fault));
        }
        match view.semantic_work_drained_for_audit() {
            Some(true) => return Ok(()),
            Some(false) => {}
            None => return Err(ProbeError::verify(WindowsSemanticProbeStage::Fault)),
        }
        if Instant::now() >= deadline {
            return Err(ProbeError::timeout(WindowsSemanticProbeStage::Fault));
        }
        pump_and_sample(deadline, host, Some(view.view()), native_guard)?;
    }
}

#[windows_core::implement(ICoreWebView2CallDevToolsProtocolMethodCompletedHandler)]
struct FixedCrashCompletion;

impl ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl for FixedCrashCompletion_Impl {
    fn Invoke(&self, _result: HRESULT, _response: &PCWSTR) -> windows_core::Result<()> {
        Ok(())
    }
}

fn request_fixed_renderer_crash(view: &AgentOwnedView) -> ProbeResult<()> {
    let handler: ICoreWebView2CallDevToolsProtocolMethodCompletedHandler =
        FixedCrashCompletion.into();
    let method = HSTRING::from("Page.crash");
    let parameters = HSTRING::from("{}");
    // SAFETY: the production view and fixed completion handler are live owned
    // references; both fixed HSTRING arguments outlive the call. WebView2
    // retains the handler for its asynchronous completion.
    unsafe {
        view.view()
            .webview()
            .CallDevToolsProtocolMethod(&method, &parameters, &handler)
    }
    .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Fault))?;
    Ok(())
}

fn verify_first_snapshot(snapshot: &SemanticSnapshot) -> ProbeResult<()> {
    if snapshot.completeness() != SemanticCompleteness::Complete
        || !snapshot_contains(snapshot, "First semantic epoch")
        || !snapshot_contains(snapshot, "Page bridge absent")
        || !snapshot_contains(snapshot, "Primary semantic action")
        || snapshot_contains(snapshot, "Closed internal must remain absent")
        || snapshot_contains(snapshot, "Page bridge present")
        || snapshot_contains(snapshot, "page-world-forgery")
        || snapshot_contains(snapshot, "fixture-password-value")
        || snapshot_contains(snapshot, "Bearer abcdefghijklmnop")
        || !snapshot_contains(snapshot, "Open shadow semantic action")
        || !snapshot.nodes().iter().any(|node| {
            node.role() == SemanticRole::Button
                && node.operations().contains(SemanticOperationClass::Click)
        })
        || !snapshot
            .nodes()
            .iter()
            .any(|node| node.role() == SemanticRole::FrameBoundary)
    {
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Verify));
    }
    let password = snapshot
        .nodes()
        .iter()
        .find(|node| node.role() == SemanticRole::Password)
        .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Verify))?;
    let mut token_candidates = snapshot.nodes().iter().filter(|node| {
        node.role() == SemanticRole::Textbox
            && node.value() == Some(&SemanticValueSummary::Redacted)
            && node.sensitivity() == SemanticSensitivity::Secret
    });
    let token = token_candidates
        .next()
        .ok_or_else(|| ProbeError::verify(WindowsSemanticProbeStage::Verify))?;
    if token_candidates.next().is_some()
        || password.value() != Some(&SemanticValueSummary::Redacted)
        || password.sensitivity() != SemanticSensitivity::Secret
        || token.value() != Some(&SemanticValueSummary::Redacted)
        || token.sensitivity() != SemanticSensitivity::Secret
    {
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Verify));
    }
    Ok(())
}

fn verify_replacement_snapshot(snapshot: &SemanticSnapshot) -> ProbeResult<()> {
    if snapshot.completeness() != SemanticCompleteness::Complete
        || !snapshot_contains(snapshot, "Replacement semantic epoch")
        || !snapshot_contains(snapshot, "Replacement semantic action")
        || !snapshot_contains(snapshot, "Page bridge absent")
        || snapshot_contains(snapshot, "First semantic epoch")
        || snapshot_contains(snapshot, "Page bridge present")
        || snapshot_contains(snapshot, "replacement-page-world-forgery")
    {
        return Err(ProbeError::verify(WindowsSemanticProbeStage::Verify));
    }
    Ok(())
}

fn snapshot_contains(snapshot: &SemanticSnapshot, needle: &str) -> bool {
    snapshot.nodes().iter().any(|node| {
        node.name()
            .is_some_and(|value| value.as_str().contains(needle))
            || node
                .text()
                .is_some_and(|value| value.as_str().contains(needle))
            || matches!(
                node.value(),
                Some(SemanticValueSummary::Text(value)) if value.as_str().contains(needle)
            )
    })
}

fn close_bootstrap(
    bootstrap: &mut Option<WebView>,
    deadline: Instant,
    host: &ProbeHostWindow,
    native_guard: &NativeStateGuard,
) -> ProbeResult<()> {
    let mut clean = bootstrap.is_none();
    if let Some(view) = bootstrap.as_mut() {
        clean = match view.close() {
            Ok(()) => true,
            Err(mut debt) => {
                while !debt.is_complete() && Instant::now() < deadline {
                    let _ = debt.retry();
                    pump_and_sample(deadline, host, Some(view), native_guard)?;
                }
                debt.is_complete()
            }
        };
    }
    drop(bootstrap.take());
    for mut debt in wry::pending_webview2_cleanup_debts() {
        while !debt.is_complete() && Instant::now() < deadline {
            let _ = debt.retry();
            pump_and_sample(deadline, host, None, native_guard)?;
        }
        clean &= debt.is_complete();
    }
    clean &= !wry::webview2_cleanup_overflowed();
    if clean && !native_guard.failed() {
        Ok(())
    } else {
        Err(ProbeError::harness(WindowsSemanticProbeStage::Construct))
    }
}

#[allow(clippy::too_many_arguments)]
fn teardown(
    bootstrap: &mut Option<WebView>,
    view: &mut Option<AgentOwnedView>,
    content_policy: Option<ContentPolicyRegistration>,
    context: Option<WebContext>,
    environment: Option<ICoreWebView2Environment>,
    process: Option<BrowserProcess>,
    observer: Option<BrowserProcessExitObserver>,
    host: ProbeHostWindow,
    profile: tempfile::TempDir,
    server: FixtureServer,
    callbacks: &CallbackState,
    native_guard: &NativeStateGuard,
    mode: WindowsSemanticProbeMode,
) -> TeardownResult {
    let started = Instant::now();
    let deadline = started + NATIVE_CLEANUP_TIMEOUT;
    let policy_clean = content_policy.is_none_or(|policy| policy.retire().is_ok());
    let runtime_retired = view
        .as_mut()
        .is_none_or(AgentOwnedView::retire_semantic_runtime);
    let bootstrap_closed = if bootstrap.is_none() {
        true
    } else {
        close_bootstrap(bootstrap, deadline, &host, native_guard).is_ok()
    };
    let mut view_closed = view.is_none();
    if let Some(owned) = view.as_mut() {
        view_closed = match owned.close() {
            Ok(()) => true,
            Err(mut debt) => {
                while !debt.is_complete() && Instant::now() < deadline {
                    let _ = debt.retry();
                    let _ = pump_and_sample(deadline, &host, Some(owned.view()), native_guard);
                }
                debt.is_complete()
            }
        };
    }
    drop(view.take());
    for mut debt in wry::pending_webview2_cleanup_debts() {
        while !debt.is_complete() && Instant::now() < deadline {
            let _ = debt.retry();
            let _ = pump_and_sample(deadline, &host, None, native_guard);
        }
        view_closed &= debt.is_complete();
    }
    view_closed &= bootstrap_closed && !wry::webview2_cleanup_overflowed();
    drop(context);
    let environment_was_created = environment.is_some();
    drop(environment);

    let process_deadline = Instant::now() + PROCESS_EXIT_TIMEOUT;
    let mut browser_process_exited =
        !environment_was_created && process.is_none() && observer.is_none();
    if let (Some(process), Some(observer)) = (process.as_ref(), observer.as_ref()) {
        while Instant::now() < process_deadline
            && !observer.is_invalid()
            && !(observer.observed_expected_exit() && process.has_exited())
        {
            let _ = pump_once(process_deadline);
            native_guard.sample(&host, None);
        }
        browser_process_exited = !observer.is_invalid()
            && !observer.is_pending()
            && observer.observed_expected_exit()
            && process.has_exited();
    }
    drop(observer);
    drop(process);
    native_guard.sample(&host, None);
    drop(host);
    let profile_removed = if browser_process_exited && view_closed {
        profile.close().is_ok()
    } else {
        let _retained_profile = profile.keep();
        false
    };
    let fixture_drained = server.shutdown().is_ok();
    let expected_allowance = match mode {
        WindowsSemanticProbeMode::HiddenEventFlood => CallbackAllowance::EVENT_FLOOD,
        WindowsSemanticProbeMode::HiddenRendererLoss => CallbackAllowance::RENDERER_LOST,
        WindowsSemanticProbeMode::HiddenFixedDocuments
        | WindowsSemanticProbeMode::HiddenRedirectLifecycle
        | WindowsSemanticProbeMode::HiddenSuspendResume
        | WindowsSemanticProbeMode::HiddenDebuggerCoexistence => CallbackAllowance::NONE,
    };
    let callbacks_drained = callbacks
        .navigation
        .try_borrow()
        .is_ok_and(|slot| slot.is_none())
        && !callbacks.suspend_callback_pending.get()
        && !callbacks.fatal(expected_allowance);
    TeardownResult {
        runtime_retired,
        view_closed,
        browser_process_exited,
        profile_removed,
        fixture_drained,
        work_drained: policy_clean
            && runtime_retired
            && view_closed
            && browser_process_exited
            && profile_removed
            && fixture_drained
            && callbacks_drained
            && !native_guard.failed(),
        cleanup_ms: duration_ms_u32(started.elapsed()),
    }
}

fn pump_and_sample(
    deadline: Instant,
    host: &ProbeHostWindow,
    view: Option<&WebView>,
    native_guard: &NativeStateGuard,
) -> ProbeResult<()> {
    pump_once(deadline)?;
    native_guard.sample(host, view);
    Ok(())
}

fn pump_once(deadline: Instant) -> ProbeResult<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Ok(());
    }
    let wait_ms = remaining.as_millis().clamp(1, PUMP_SLICE.as_millis()) as u32;
    // SAFETY: the qualifier owns this STA message loop. The MSG buffer is
    // valid for each call and messages are dispatched unchanged on this thread.
    unsafe {
        let _ = MsgWaitForMultipleObjectsEx(None, wait_ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        for _ in 0..256 {
            let mut message = MSG::default();
            if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                break;
            }
            if message.message == WM_QUIT {
                PostQuitMessage(message.wParam.0 as i32);
                return Err(ProbeError::harness(WindowsSemanticProbeStage::Teardown));
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

fn runtime_fingerprint() -> ProbeResult<RuntimeFingerprint> {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: the initialized structure advertises its exact size and remains
    // valid writable storage for the duration of the system query.
    let status = unsafe { RtlGetVersion(&mut version) };
    if status.0 < 0 {
        return Err(ProbeError::harness(WindowsSemanticProbeStage::Construct));
    }
    let os_version = format!(
        "{}.{}.{}",
        version.dwMajorVersion, version.dwMinorVersion, version.dwBuildNumber
    );
    let engine_version = wry::webview_version()
        .map_err(|_| ProbeError::harness(WindowsSemanticProbeStage::Construct))?;
    Ok(RuntimeFingerprint {
        platform: Platform::Windows,
        os_version: EvidenceLabel::new(os_version)
            .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Construct))?,
        engine: EvidenceLabel::new("WebView2")
            .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Construct))?,
        engine_version: EvidenceLabel::new(engine_version)
            .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Construct))?,
        adapter_revision: EvidenceLabel::new("semantic-runtime-m3-lifecycle-m2-redirect-v1")
            .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Construct))?,
    })
}

fn earlier_deadline(absolute: Instant, duration: Duration) -> ProbeResult<Instant> {
    let relative = Instant::now()
        .checked_add(duration)
        .ok_or_else(|| ProbeError::timeout(WindowsSemanticProbeStage::Admit))?;
    Ok(std::cmp::min(absolute, relative))
}

fn duration_ms_u32(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

fn duration_ms_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
