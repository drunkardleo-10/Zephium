#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

//! Extension-isolated hidden WebView2 construction for owned agent contexts.
//!
//! The adapter reuses one already-proven extension-enabled environment. The
//! Wry startup fence runs against the exact controller profile before WebView
//! initialization or the bootstrap navigation. It either rejoins the selected
//! empty profile or a deterministic extension-free automation subprofile.

use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment, ICoreWebView2Profile, ICoreWebView2Profile7,
    ICoreWebView2_13, ICoreWebView2_2,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetParent, IsChild, IsWindow, IsWindowVisible,
};
use windows_core::{Interface as _, PWSTR};
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::{
    DownloadPolicy, PageClosePolicy, Rect, WebView, WebViewBuilder, WebViewBuilderExtWindows as _,
    WebViewExtWindows as _,
};
use zephium_agentic::{ContextConstructionProof, ContextOwnedViewport, ContextProfileStorageClass};
use zephium_agentic::{
    SemanticRuntimeInvocation, SemanticRuntimePortFailure, SemanticScreenshotNativeCapture,
    SemanticScreenshotNativeFailure, SemanticScreenshotNativeRequest, SemanticSnapshot,
};
use zephium_core::ids::ProfileId;

use crate::platform::agent_navigation::AgentNavigationController;
pub(crate) use crate::platform::agent_navigation::{
    AgentNavigationCommit, AgentNavigationTerminal,
};

use super::{
    profile_inventory_is_empty, WindowsNativeExtensionFailure, WindowsNativeExtensionProfile,
};

const PROFILE_NAME_UTF16_LIMIT: usize = 64;
const PROFILE_NAME_UTF8_LIMIT: usize = 64;

/// Closed construction failure mapped by the native host boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentOwnedViewConstructionError {
    /// The selected environment, profile name, or private-mode bit diverged.
    Storage,
    /// Extension-enabled empty-inventory proof could not be established.
    ExtensionIsolation,
    /// Wry/WebView2 refused construction, hardening, callbacks, or hidden state.
    Native,
}

/// Exact profile authority selected before one controller is constructed.
#[derive(Clone)]
pub(crate) enum AgentOwnedProfile {
    Selected(WindowsNativeExtensionProfile),
    Automation { name: String },
}

impl AgentOwnedProfile {
    pub(crate) const fn selected(profile: WindowsNativeExtensionProfile) -> Self {
        Self::Selected(profile)
    }

    pub(crate) fn automation(profile: ProfileId) -> Self {
        Self::Automation {
            name: format!("agent-{profile}"),
        }
    }

    pub(crate) const fn proof(&self) -> ContextConstructionProof {
        match self {
            Self::Selected(_) => {
                ContextConstructionProof::WindowsOwnedSelectedProfileEmptyInventory
            }
            Self::Automation { .. } => {
                ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory
            }
        }
    }
}

/// Typed callback cohort retained by one WebView2 controller generation.
pub(crate) struct AgentOwnedViewCallbacks<Navigation, RendererLost, BrowserLost, Invariant, Panic> {
    navigation: Navigation,
    renderer_lost: RendererLost,
    browser_lost: BrowserLost,
    invariant: Invariant,
    panic: Panic,
}

impl<Navigation, RendererLost, BrowserLost, Invariant, Panic>
    AgentOwnedViewCallbacks<Navigation, RendererLost, BrowserLost, Invariant, Panic>
{
    pub(crate) const fn new(
        navigation: Navigation,
        renderer_lost: RendererLost,
        browser_lost: BrowserLost,
        invariant: Invariant,
        panic: Panic,
    ) -> Self {
        Self {
            navigation,
            renderer_lost,
            browser_lost,
            invariant,
            panic,
        }
    }
}

/// Exact native page and its one-at-a-time navigation policy.
pub(crate) struct AgentOwnedView {
    navigation: AgentNavigationController,
    semantic: Option<super::semantic_runtime::AgentSemanticRuntimeRegistration>,
    profile: AgentOwnedProfile,
    storage_class: ContextProfileStorageClass,
    expected_user_data_folder: std::path::PathBuf,
    expected_parent: HWND,
    viewport: ContextOwnedViewport,
    _crash_observer: super::CrashObserver,
    _security_policy: super::SecurityPolicy,
    view: WebView,
}

impl AgentOwnedView {
    pub(crate) const fn view(&self) -> &WebView {
        &self.view
    }

    pub(crate) const fn navigation(&self) -> &AgentNavigationController {
        &self.navigation
    }

    pub(crate) fn semantic(
        &self,
    ) -> Option<&super::semantic_runtime::AgentSemanticRuntimeController> {
        self.semantic
            .as_ref()
            .map(super::semantic_runtime::AgentSemanticRuntimeRegistration::controller)
    }

    pub(crate) fn prepare_semantic_document_load(&mut self) -> Result<(), ()> {
        self.semantic().ok_or(())?.begin_document_load()
    }

    pub(crate) fn retire_semantic_runtime(&mut self) -> bool {
        self.semantic
            .take()
            .is_some_and(|registration| registration.retire().is_ok())
    }

    pub(crate) const fn semantic_is_live(&self) -> bool {
        self.semantic.is_some()
    }

    // The host intentionally refuses this call until physical Windows
    // isolated-world qualification promotes the platform support claim.
    #[allow(dead_code)]
    pub(crate) fn dispatch_semantic(
        &self,
        invocation: SemanticRuntimeInvocation,
        completion: impl FnOnce(Result<SemanticSnapshot, SemanticRuntimePortFailure>) + 'static,
    ) -> Result<(), SemanticRuntimePortFailure> {
        let Some(semantic) = self.semantic() else {
            completion(Err(SemanticRuntimePortFailure::Retired));
            return Err(SemanticRuntimePortFailure::Retired);
        };
        semantic.dispatch(invocation, completion)
    }

    // The physical adapter is compiled and statically gated now, but the host
    // intentionally cannot reach it until Windows semantic qualification also
    // unlocks exact snapshot-generation tracking on that platform.
    #[allow(dead_code)]
    pub(crate) fn dispatch_screenshot(
        &self,
        request: SemanticScreenshotNativeRequest,
        admitted_at: Instant,
        cancelled: Arc<AtomicBool>,
        completion: impl FnOnce(Result<SemanticScreenshotNativeCapture, SemanticScreenshotNativeFailure>)
            + 'static,
        callback_panicked: impl Fn() + 'static,
    ) -> Result<(), SemanticScreenshotNativeFailure> {
        if !self
            .semantic()
            .is_some_and(|semantic| semantic.document_content_available_for_audit() == Some(true))
            || attest_hidden_owner(&self.view, self.expected_parent, self.viewport).is_err()
        {
            return Err(SemanticScreenshotNativeFailure::NotReady);
        }
        super::semantic_screenshot::capture_viewport(
            &self.view,
            request,
            admitted_at,
            cancelled,
            completion,
            callback_panicked,
        )
    }

    pub(crate) fn semantic_pending_for_audit(&self) -> Option<bool> {
        self.semantic()?.pending_for_audit()
    }

    pub(crate) fn semantic_work_drained_for_audit(&self) -> Option<bool> {
        self.semantic()?.work_drained_for_audit()
    }

    pub(crate) fn attest(&self, deadline: Instant) -> Result<(), AgentOwnedViewConstructionError> {
        attest_profile_before_initialization(
            &self.profile,
            &self.view.environment(),
            &self.view.webview(),
            self.storage_class,
            &self.expected_user_data_folder,
            deadline,
        )?;
        if !self
            .semantic()
            .is_some_and(|semantic| semantic.attest(&self.view.webview()))
        {
            return Err(AgentOwnedViewConstructionError::ExtensionIsolation);
        }
        attest_hidden_owner(&self.view, self.expected_parent, self.viewport)
    }

    pub(crate) fn close(&mut self) -> Result<(), wry::WebView2CleanupDebt> {
        if self.semantic_is_live() {
            let _ = self.retire_semantic_runtime();
        }
        self.view.close()
    }
}

fn invoke_navigation_callback(
    callback: &dyn Fn(AgentNavigationTerminal),
    callback_panicked: &dyn Fn(),
    terminal: AgentNavigationTerminal,
) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(terminal))).is_err() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback_panicked));
    }
}

fn invoke_unit_callback(callback: &dyn Fn(), callback_panicked: &dyn Fn()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)).is_err() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback_panicked));
    }
}

fn controller_profile(core: &ICoreWebView2) -> Result<ICoreWebView2Profile7, ()> {
    // SAFETY: `cast` yields a live, reference-counted WebView2 interface; the
    // property getter initializes and returns its own reference-counted profile.
    core.cast::<ICoreWebView2_13>()
        .and_then(|core| unsafe { core.Profile() })
        .and_then(|profile| profile.cast::<ICoreWebView2Profile7>())
        .map_err(|_| ())
}

fn profile_name(profile: &ICoreWebView2Profile7) -> Result<String, ()> {
    let profile = profile.cast::<ICoreWebView2Profile>().map_err(|_| ())?;
    let mut raw = PWSTR::null();
    // SAFETY: `profile` is a live COM interface and `raw` is a valid initialized
    // out pointer. WebView2 allocates the returned string; the bounded helper
    // consumes and releases it exactly once, including refusal paths.
    unsafe { profile.ProfileName(&mut raw) }.map_err(|_| ())?;
    super::take_pwstr_bounded(raw, PROFILE_NAME_UTF16_LIMIT, PROFILE_NAME_UTF8_LIMIT).ok_or(())
}

fn profile_is_private(profile: &ICoreWebView2Profile7) -> Result<bool, ()> {
    let profile = profile.cast::<ICoreWebView2Profile>().map_err(|_| ())?;
    let mut private = windows_core::BOOL::default();
    // SAFETY: `profile` is a live COM interface and `private` provides writable
    // storage of the exact BOOL type required by this synchronous getter.
    unsafe { profile.IsInPrivateModeEnabled(&mut private) }.map_err(|_| ())?;
    Ok(private.as_bool())
}

fn controller_environment_matches(
    environment: &ICoreWebView2Environment,
    core: &ICoreWebView2,
) -> bool {
    // SAFETY: `cast` yields a live, reference-counted WebView2 interface; the
    // environment getter returns a separately reference-counted COM object.
    core.cast::<ICoreWebView2_2>()
        .and_then(|core| unsafe { core.Environment() })
        .is_ok_and(|controller_environment| {
            super::same_environment(environment, &controller_environment)
        })
}

fn expected_parent_hwnd(
    parent: &impl HasWindowHandle,
) -> Result<HWND, AgentOwnedViewConstructionError> {
    match parent
        .window_handle()
        .map_err(|_| AgentOwnedViewConstructionError::Native)?
        .as_raw()
    {
        RawWindowHandle::Win32(parent) => Ok(HWND(parent.hwnd.get() as _)),
        _ => Err(AgentOwnedViewConstructionError::Native),
    }
}

fn attest_hidden_owner(
    view: &WebView,
    expected_parent: HWND,
    viewport: ContextOwnedViewport,
) -> Result<(), AgentOwnedViewConstructionError> {
    let container = view.hwnd();
    // SAFETY: these Win32 predicates accept opaque HWND values, including
    // stale ones, without dereferencing caller memory. Successful IsWindow
    // checks establish both handles before the parent query is trusted.
    let hierarchy_matches = unsafe {
        IsWindow(Some(expected_parent)).as_bool()
            && IsWindow(Some(container)).as_bool()
            && GetParent(container).ok() == Some(expected_parent)
    };
    if !hierarchy_matches {
        return Err(AgentOwnedViewConstructionError::Native);
    }
    let controller = view.controller();
    let mut controller_parent = HWND::default();
    let mut controller_visible = windows_core::BOOL::default();
    // SAFETY: `container` was proven to identify a live window above and this
    // query neither receives nor retains caller-owned pointers.
    let dpi = unsafe { GetDpiForWindow(container) };
    let Some(expected_width) = expected_physical_extent(viewport.width(), dpi) else {
        return Err(AgentOwnedViewConstructionError::Native);
    };
    let Some(expected_height) = expected_physical_extent(viewport.height(), dpi) else {
        return Err(AgentOwnedViewConstructionError::Native);
    };
    let mut container_bounds = RECT::default();
    let mut controller_bounds = RECT::default();
    // SAFETY: `controller` is the live reference-counted controller owned by
    // `view`; every out argument is initialized writable storage of the exact
    // COM/Win32 type, and `container` was proven live above.
    let native_state_matches = unsafe {
        controller.ParentWindow(&mut controller_parent).is_ok()
            && controller_parent == container
            && controller.IsVisible(&mut controller_visible).is_ok()
            && !controller_visible.as_bool()
            && !IsWindowVisible(container).as_bool()
            && GetClientRect(container, &mut container_bounds).is_ok()
            && controller.Bounds(&mut controller_bounds).is_ok()
    };
    if !native_state_matches
        || container_bounds.left != 0
        || container_bounds.top != 0
        || container_bounds.right != expected_width
        || container_bounds.bottom != expected_height
        || controller_bounds.left != 0
        || controller_bounds.top != 0
        || controller_bounds.right != expected_width
        || controller_bounds.bottom != expected_height
    {
        return Err(AgentOwnedViewConstructionError::Native);
    }
    // SAFETY: GetFocus returns an opaque HWND for this UI thread. `container`
    // is live, and IsChild accepts a null or stale candidate without touching
    // caller memory.
    let focus_inside = unsafe {
        let focus = GetFocus();
        !focus.0.is_null() && (focus == container || IsChild(container, focus).as_bool())
    };
    if focus_inside {
        return Err(AgentOwnedViewConstructionError::Native);
    }
    Ok(())
}

fn expected_physical_extent(logical: u16, dpi: u32) -> Option<i32> {
    if dpi == 0 {
        return None;
    }
    let scaled = u64::from(logical)
        .checked_mul(u64::from(dpi))?
        .checked_add(48)?
        / 96;
    i32::try_from(scaled).ok().filter(|extent| *extent > 0)
}

fn attest_profile_before_initialization(
    binding: &AgentOwnedProfile,
    environment: &ICoreWebView2Environment,
    core: &ICoreWebView2,
    storage_class: ContextProfileStorageClass,
    expected_user_data_folder: &Path,
    deadline: Instant,
) -> Result<(), AgentOwnedViewConstructionError> {
    super::attest_environment(environment, expected_user_data_folder)
        .map_err(|_| AgentOwnedViewConstructionError::Storage)?;
    if !controller_environment_matches(environment, core) {
        return Err(AgentOwnedViewConstructionError::Storage);
    }
    match binding {
        AgentOwnedProfile::Selected(profile) => {
            if storage_class != ContextProfileStorageClass::Durable {
                return Err(AgentOwnedViewConstructionError::Storage);
            }
            profile
                .attest_controller(environment, core, deadline, &[])
                .map_err(map_inventory_failure)
        }
        AgentOwnedProfile::Automation { name } => {
            if name.len() > PROFILE_NAME_UTF8_LIMIT
                || !name.is_ascii()
                || !name.starts_with("agent-")
            {
                return Err(AgentOwnedViewConstructionError::Storage);
            }
            let profile = controller_profile(core)
                .map_err(|_| AgentOwnedViewConstructionError::ExtensionIsolation)?;
            if profile_name(&profile).as_deref() != Ok(name.as_str())
                || profile_is_private(&profile)
                    != Ok(storage_class == ContextProfileStorageClass::Ephemeral)
            {
                return Err(AgentOwnedViewConstructionError::Storage);
            }
            match profile_inventory_is_empty(&profile, deadline) {
                Ok(true) => Ok(()),
                Ok(false) => Err(AgentOwnedViewConstructionError::ExtensionIsolation),
                Err(failure) => Err(map_inventory_failure(failure)),
            }
        }
    }
}

const fn map_inventory_failure(
    _failure: WindowsNativeExtensionFailure,
) -> AgentOwnedViewConstructionError {
    AgentOwnedViewConstructionError::ExtensionIsolation
}

/// Builds one initially hidden, extension-isolated selected-profile WebView2.
///
/// The only initial document is `about:blank`. No page-world script, IPC
/// handler, popup callback, generic native bridge, selector, or model-facing
/// program is installed. The semantic program is installed lazily only in a
/// native-proven non-universal isolated world. Network navigation remains
/// denied until the host arms one exact operation and attaches its native
/// content policy.
pub(crate) fn build_owned_agent_view<Navigation, RendererLost, BrowserLost, Invariant, Panic>(
    parent: &impl HasWindowHandle,
    viewport: ContextOwnedViewport,
    environment: &ICoreWebView2Environment,
    profile: AgentOwnedProfile,
    storage_class: ContextProfileStorageClass,
    expected_user_data_folder: &Path,
    deadline: Instant,
    callbacks: AgentOwnedViewCallbacks<Navigation, RendererLost, BrowserLost, Invariant, Panic>,
) -> Result<(AgentOwnedView, ContextConstructionProof), AgentOwnedViewConstructionError>
where
    Navigation: Fn(AgentNavigationTerminal) + 'static,
    RendererLost: Fn() + 'static,
    BrowserLost: Fn() + 'static,
    Invariant: Fn() + 'static,
    Panic: Fn() + 'static,
{
    if Instant::now() >= deadline {
        return Err(AgentOwnedViewConstructionError::Native);
    }
    let expected_parent = expected_parent_hwnd(parent)?;
    let proof = profile.proof();
    let semantic_plan = super::semantic_runtime::AgentSemanticRuntimePlan::prepare()
        .map_err(|_| AgentOwnedViewConstructionError::Native)?;
    let AgentOwnedViewCallbacks {
        navigation: on_navigation,
        renderer_lost: on_renderer_lost,
        browser_lost: on_browser_lost,
        invariant: on_invariant_failure,
        panic: on_callback_panic,
    } = callbacks;
    let navigation = AgentNavigationController::default();
    let navigation_policy = navigation.clone();
    let navigation_events = navigation.clone();
    let renderer_events = navigation.clone();
    let navigation_callback = Rc::new(on_navigation);
    let renderer_lost_callback = Rc::new(on_renderer_lost);
    let browser_lost_callback = Rc::new(on_browser_lost);
    let invariant_callback = Rc::new(on_invariant_failure);
    let panic_callback = Rc::new(on_callback_panic);
    let navigation_invariant = invariant_callback.clone();
    let renderer_invariant = invariant_callback.clone();
    let semantic_invariant = invariant_callback.clone();
    let navigation_panic = panic_callback.clone();
    let renderer_panic = panic_callback.clone();
    let browser_panic = panic_callback.clone();
    let semantic_panic = panic_callback.clone();
    let semantic_navigation = semantic_plan.clone();
    let semantic_renderer = semantic_plan.clone();
    let semantic_browser = semantic_plan.clone();

    let mut builder = WebViewBuilder::new()
        .with_url("about:blank")
        .with_bounds(Rect {
            position: Position::Logical(LogicalPosition::new(0.0, 0.0)),
            size: Size::Logical(LogicalSize::new(
                f64::from(viewport.width()),
                f64::from(viewport.height()),
            )),
        })
        .with_visible(false)
        .with_focused(false)
        .with_devtools(false)
        .with_autoplay(false)
        .with_hotkeys_zoom(false)
        .with_clipboard(false)
        .with_fullscreen_enabled(false)
        .with_picture_in_picture_enabled(false)
        .with_general_autofill_enabled(false)
        .with_navigation_handler(move |target| navigation_policy.allows(&target))
        .with_navigation_event_handler(move |event| match navigation_events.observe(event) {
            Ok(observation) => {
                if observation.did_commit_document()
                    && semantic_navigation.document_committed().is_err()
                {
                    invoke_unit_callback(navigation_invariant.as_ref(), navigation_panic.as_ref());
                    return;
                }
                if let Some(terminal) = observation.into_terminal() {
                    invoke_navigation_callback(
                        navigation_callback.as_ref(),
                        navigation_panic.as_ref(),
                        terminal,
                    );
                }
            }
            Err(()) => {
                invoke_unit_callback(navigation_invariant.as_ref(), navigation_panic.as_ref())
            }
        })
        .with_navigation_presentation_guard(|| {})
        .with_permission_handler(|_| wry::PermissionResponse::Deny)
        .with_download_policy(DownloadPolicy::DenyWithoutMetadata)
        .with_page_close_policy(PageClosePolicy::Ignore)
        .with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI")
        .with_browser_accelerator_keys(false)
        .with_default_context_menus(false)
        .with_environment(environment.clone());
    if storage_class == ContextProfileStorageClass::Ephemeral {
        builder = builder.with_incognito(true);
    }

    let automation_name = match &profile {
        AgentOwnedProfile::Selected(_) => None,
        AgentOwnedProfile::Automation { name } => Some(name.clone()),
    };
    if let Some(name) = automation_name {
        builder = builder.with_profile_name(name);
    }
    let expected_path = expected_user_data_folder.to_owned();
    let gate_profile = profile.clone();
    let gate = move |environment: &ICoreWebView2Environment, core: &ICoreWebView2| {
        attest_profile_before_initialization(
            &gate_profile,
            environment,
            core,
            storage_class,
            &expected_path,
            deadline,
        )
        .map_err(|_| {
            windows_core::Error::new(
                windows::Win32::Foundation::E_ACCESSDENIED,
                "owned WebView2 profile isolation gate failed",
            )
        })
    };
    builder = builder.with_browser_extension_startup_gate(gate);

    let view = builder
        .build_as_child(parent)
        .map_err(|_| AgentOwnedViewConstructionError::Native)?;
    let semantic = semantic_plan
        .bind(&view.webview(), semantic_invariant, semantic_panic)
        .map_err(|_| AgentOwnedViewConstructionError::Native)?;
    if !semantic.controller().attest(&view.webview()) {
        return Err(AgentOwnedViewConstructionError::ExtensionIsolation);
    }
    let security_policy = super::configure(
        &view,
        0.0,
        storage_class == ContextProfileStorageClass::Ephemeral,
        expected_user_data_folder,
    )
    .map_err(|_| AgentOwnedViewConstructionError::Native)?;
    attest_hidden_owner(&view, expected_parent, viewport)?;

    let crash_observer = super::install_crash_handler(&view, move |failure| match failure {
        super::ProcessFailure::Renderer => match renderer_events.claim_renderer_loss() {
            Ok(true) => {
                semantic_renderer.renderer_lost();
                invoke_unit_callback(renderer_lost_callback.as_ref(), renderer_panic.as_ref())
            }
            Ok(false) => {}
            Err(()) => invoke_unit_callback(renderer_invariant.as_ref(), renderer_panic.as_ref()),
        },
        super::ProcessFailure::Browser => {
            semantic_browser.renderer_lost();
            let _ = renderer_events.claim_renderer_loss();
            invoke_unit_callback(browser_lost_callback.as_ref(), browser_panic.as_ref());
        }
    })
    .map_err(|_| AgentOwnedViewConstructionError::Native)?;
    Ok((
        AgentOwnedView {
            navigation,
            semantic: Some(semantic),
            profile,
            storage_class,
            expected_user_data_folder: expected_user_data_folder.to_owned(),
            expected_parent,
            viewport,
            _crash_observer: crash_observer,
            _security_policy: security_policy,
            view,
        },
        proof,
    ))
}

#[cfg(test)]
mod tests {
    use super::expected_physical_extent;

    #[test]
    fn viewport_extent_uses_the_same_positive_half_up_dpi_rounding_as_wry() {
        assert_eq!(expected_physical_extent(1_280, 96), Some(1_280));
        assert_eq!(expected_physical_extent(800, 120), Some(1_000));
        assert_eq!(expected_physical_extent(1, 144), Some(2));
        assert_eq!(expected_physical_extent(1_280, 0), None);
    }
}
