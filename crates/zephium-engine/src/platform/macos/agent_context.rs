//! Extension-free, hidden WKWebView construction for owned agent contexts.

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use raw_window_handle::HasWindowHandle;
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::{
    DownloadPolicy, NavigationEvent, NavigationEventPhase, PageClosePolicy, Rect, WebView,
    WebViewBuilder, WebViewBuilderExtDarwin as _, WebViewBuilderExtMacos as _,
};
use zephium_agentic::{
    ContextNavigationTarget, ContextOperationJoin, ContextOperationKind, ContextOwnedViewport,
    ContextPortFailure, ContextProfileStorageClass, SemanticRuntimeInvocation,
    SemanticRuntimePortFailure, SemanticScreenshotNativeCapture, SemanticScreenshotNativeFailure,
    SemanticScreenshotNativeRequest, SemanticSnapshot,
};
use zephium_core::ids::ProfileId;

use super::semantic_runtime::{
    AgentSemanticRuntimeController, AgentSemanticRuntimeDispatchError, AgentSemanticRuntimeFailure,
    AgentSemanticRuntimeRegistration,
};
use super::WebsiteDataStore;

/// Closed construction failure mapped to the public native-port taxonomy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentOwnedViewConstructionError {
    /// The exact selected-profile storage class or identity was not retained.
    Storage,
    /// Extension absence or exact private semantic-script inventory could not be proven.
    ExtensionIsolation,
    /// Wry/WebKit refused hidden child-view construction or hardening.
    Native,
}

/// Closed committed target for one exact native page load.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AgentNavigationCommit {
    /// A validated HTTP(S) document committed.
    Web(ContextNavigationTarget),
    /// The construction/recovery-only empty document committed.
    Bootstrap,
}

/// One exact terminal native observation for a shell-requested page load.
pub(crate) struct AgentNavigationTerminal {
    operation: ContextOperationJoin,
    outcome: Result<AgentNavigationCommit, ContextPortFailure>,
}

impl AgentNavigationTerminal {
    pub(crate) const fn operation(&self) -> ContextOperationJoin {
        self.operation
    }

    pub(crate) fn into_outcome(self) -> Result<AgentNavigationCommit, ContextPortFailure> {
        self.outcome
    }
}

struct AgentNavigationObservation {
    document_committed: bool,
    terminal: Option<AgentNavigationTerminal>,
}

impl AgentNavigationObservation {
    const fn none() -> Self {
        Self {
            document_committed: false,
            terminal: None,
        }
    }

    const fn document_committed() -> Self {
        Self {
            document_committed: true,
            terminal: None,
        }
    }

    fn terminal(document_committed: bool, terminal: AgentNavigationTerminal) -> Self {
        Self {
            document_committed,
            terminal: Some(terminal),
        }
    }

    const fn did_commit_document(&self) -> bool {
        self.document_committed
    }

    fn into_terminal(self) -> Option<AgentNavigationTerminal> {
        self.terminal
    }

    #[cfg(test)]
    fn is_none(&self) -> bool {
        self.terminal.is_none()
    }

    #[cfg(test)]
    fn is_some(&self) -> bool {
        self.terminal.is_some()
    }

    #[cfg(test)]
    fn expect(self, message: &str) -> AgentNavigationTerminal {
        self.terminal.expect(message)
    }
}

#[derive(Clone)]
enum AgentLoadExpectation {
    Web(ContextNavigationTarget),
    Bootstrap,
}

impl AgentLoadExpectation {
    fn matches(&self, candidate: &str) -> bool {
        match self {
            Self::Web(expected) => ContextNavigationTarget::parse(candidate)
                .ok()
                .is_some_and(|candidate| candidate == *expected),
            Self::Bootstrap => candidate == "about:blank",
        }
    }

    fn commit(&self, candidate: &str) -> Result<AgentNavigationCommit, ContextPortFailure> {
        match self {
            Self::Web(expected) => ContextNavigationTarget::parse(candidate)
                .ok()
                .filter(|candidate| candidate == expected)
                .map(AgentNavigationCommit::Web)
                .ok_or(ContextPortFailure::NativeRefused),
            Self::Bootstrap if candidate == "about:blank" => Ok(AgentNavigationCommit::Bootstrap),
            Self::Bootstrap => Err(ContextPortFailure::NativeRefused),
        }
    }
}

struct AgentNavigationArm {
    operation: ContextOperationJoin,
    expected: AgentLoadExpectation,
    terminal_claimed: Arc<AtomicBool>,
    native_id: Option<wry::NavigationId>,
}

struct AgentNavigationState {
    bootstrap_available: bool,
    bootstrap_pending: bool,
    bootstrap_native_id: Option<wry::NavigationId>,
    renderer_lost: bool,
    armed: Option<AgentNavigationArm>,
}

impl Default for AgentNavigationState {
    fn default() -> Self {
        Self {
            bootstrap_available: true,
            bootstrap_pending: false,
            bootstrap_native_id: None,
            renderer_lost: false,
            armed: None,
        }
    }
}

/// Exact, one-at-a-time navigation policy shared with Wry's native delegates.
///
/// Unarmed page navigation is denied. The one bootstrap `about:blank` permit
/// exists solely for construction and is permanently consumed or sealed by
/// the first shell arm. Redirects are deliberately denied until the policy
/// port can authorize their exact destination.
#[derive(Clone, Default)]
pub(crate) struct AgentNavigationController {
    state: Arc<Mutex<AgentNavigationState>>,
}

impl AgentNavigationController {
    pub(crate) fn arm(
        &self,
        operation: ContextOperationJoin,
        target: ContextNavigationTarget,
        terminal_claimed: Arc<AtomicBool>,
    ) -> Result<(), ()> {
        if operation.kind() != ContextOperationKind::Navigate {
            return Err(());
        }
        self.arm_exact(
            operation,
            AgentLoadExpectation::Web(target),
            terminal_claimed,
            false,
        )
    }

    /// Arms the sole page load permitted after exact renderer loss.
    pub(crate) fn arm_recovery(
        &self,
        operation: ContextOperationJoin,
        target: Option<ContextNavigationTarget>,
        terminal_claimed: Arc<AtomicBool>,
    ) -> Result<(), ()> {
        if operation.kind() != ContextOperationKind::Recover {
            return Err(());
        }
        self.arm_exact(
            operation,
            target.map_or(AgentLoadExpectation::Bootstrap, AgentLoadExpectation::Web),
            terminal_claimed,
            true,
        )
    }

    fn arm_exact(
        &self,
        operation: ContextOperationJoin,
        expected: AgentLoadExpectation,
        terminal_claimed: Arc<AtomicBool>,
        requires_renderer_loss: bool,
    ) -> Result<(), ()> {
        if terminal_claimed.load(Ordering::Acquire) {
            return Err(());
        }
        let mut state = self.state.lock().map_err(|_| ())?;
        if state.renderer_lost != requires_renderer_loss || state.armed.is_some() {
            return Err(());
        }
        state.bootstrap_available = false;
        state.bootstrap_pending = false;
        state.bootstrap_native_id = None;
        state.renderer_lost = false;
        state.armed = Some(AgentNavigationArm {
            operation,
            expected,
            terminal_claimed,
            native_id: None,
        });
        Ok(())
    }

    pub(crate) fn disarm(&self, operation: ContextOperationJoin) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state
            .armed
            .as_ref()
            .is_some_and(|armed| armed.operation == operation)
        {
            state.armed = None;
            true
        } else {
            false
        }
    }

    /// Retires one exact recovery arm and restores loss state on refusal.
    pub(crate) fn settle_recovery(&self, operation: ContextOperationJoin, applied: bool) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if operation.kind() != ContextOperationKind::Recover
            || !state
                .armed
                .as_ref()
                .is_some_and(|armed| armed.operation == operation)
        {
            return false;
        }
        state.armed = None;
        // A termination callback can win after the commit callback has
        // claimed the terminal but before the host consumes either queued
        // callback. Successful settlement must not erase that newer loss.
        if !applied {
            state.renderer_lost = true;
        }
        true
    }

    fn allows(&self, candidate: &str) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.renderer_lost {
            return false;
        }
        if let Some(armed) = state.armed.as_ref() {
            if armed.terminal_claimed.load(Ordering::Acquire) {
                return false;
            }
            return armed.expected.matches(candidate);
        }
        if state.bootstrap_available && candidate == "about:blank" {
            state.bootstrap_available = false;
            state.bootstrap_pending = true;
            return true;
        }
        false
    }

    fn observe(&self, event: NavigationEvent) -> Result<AgentNavigationObservation, ()> {
        let mut state = self.state.lock().map_err(|_| ())?;
        if state.renderer_lost {
            return Ok(AgentNavigationObservation::none());
        }
        let Some(armed) = state.armed.as_mut() else {
            if !state.bootstrap_pending {
                return Ok(AgentNavigationObservation::none());
            }
            if event.phase == NavigationEventPhase::Started {
                if event.url == "about:blank" && state.bootstrap_native_id.is_none() {
                    state.bootstrap_native_id = Some(event.id);
                }
                return Ok(AgentNavigationObservation::none());
            }
            if !matches!(
                event.phase,
                NavigationEventPhase::Committed | NavigationEventPhase::Failed
            ) || state.bootstrap_native_id != Some(event.id)
            {
                return Ok(AgentNavigationObservation::none());
            }
            state.bootstrap_pending = false;
            state.bootstrap_native_id = None;
            return match event.phase {
                NavigationEventPhase::Committed if event.url == "about:blank" => {
                    Ok(AgentNavigationObservation::document_committed())
                }
                NavigationEventPhase::Failed => Ok(AgentNavigationObservation::none()),
                NavigationEventPhase::Committed => Err(()),
                NavigationEventPhase::Started
                | NavigationEventPhase::Redirected
                | NavigationEventPhase::Finished => Ok(AgentNavigationObservation::none()),
            };
        };
        if event.phase == NavigationEventPhase::Started {
            let matches_target = armed.expected.matches(&event.url);
            if matches_target && armed.native_id.is_none() {
                armed.native_id = Some(event.id);
            }
            return Ok(AgentNavigationObservation::none());
        }
        if !matches!(
            event.phase,
            NavigationEventPhase::Committed | NavigationEventPhase::Failed
        ) || armed.native_id != Some(event.id)
        {
            return Ok(AgentNavigationObservation::none());
        }
        if armed
            .terminal_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(AgentNavigationObservation::none());
        }
        let operation = armed.operation;
        let expected = armed.expected.clone();
        let outcome = match event.phase {
            NavigationEventPhase::Committed => expected.commit(&event.url),
            NavigationEventPhase::Failed => Err(ContextPortFailure::NativeRefused),
            NavigationEventPhase::Started
            | NavigationEventPhase::Redirected
            | NavigationEventPhase::Finished => return Ok(AgentNavigationObservation::none()),
        };
        let document_committed = outcome.is_ok();
        Ok(AgentNavigationObservation::terminal(
            document_committed,
            AgentNavigationTerminal { operation, outcome },
        ))
    }

    fn claim_renderer_loss(&self) -> Result<bool, ()> {
        let mut state = self.state.lock().map_err(|_| ())?;
        if state.renderer_lost {
            return Ok(false);
        }
        state.renderer_lost = true;
        if let Some(armed) = state.armed.as_ref() {
            armed.terminal_claimed.store(true, Ordering::Release);
        }
        Ok(true)
    }

    pub(crate) fn matches_for_audit(
        &self,
        pending: Option<ContextOperationJoin>,
        renderer_lost: bool,
    ) -> bool {
        self.state.lock().is_ok_and(|state| {
            state.renderer_lost == renderer_lost
                && state.armed.as_ref().map(|armed| armed.operation) == pending
        })
    }
}

fn invoke_owned_navigation_callback(
    callback: &dyn Fn(AgentNavigationTerminal),
    callback_panicked: &dyn Fn(),
    terminal: AgentNavigationTerminal,
) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(terminal))).is_err() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback_panicked));
    }
}

fn invoke_owned_unit_callback(callback: &dyn Fn(), callback_panicked: &dyn Fn()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)).is_err() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback_panicked));
    }
}

fn new_owned_agent_configuration(
    profile: ProfileId,
    storage_class: ContextProfileStorageClass,
    ephemeral_store: Option<&WebsiteDataStore>,
) -> Result<
    objc2::rc::Retained<objc2_web_kit::WKWebViewConfiguration>,
    AgentOwnedViewConstructionError,
> {
    use objc2::rc::Retained;
    use objc2_foundation::{MainThreadMarker, NSUUID};
    use objc2_web_kit::{WKWebViewConfiguration, WKWebsiteDataStore};

    match (storage_class, ephemeral_store) {
        (ContextProfileStorageClass::Ephemeral, Some(store)) => {
            super::new_configuration_with_data_store(store)
                .map_err(|_| AgentOwnedViewConstructionError::Storage)
        }
        (ContextProfileStorageClass::Durable, None) => {
            let mtm = MainThreadMarker::new().ok_or(AgentOwnedViewConstructionError::Native)?;
            let identifier = NSUUID::from_bytes(profile.bytes());
            let store = unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&identifier, mtm) };
            if !unsafe { store.isPersistent() }
                || unsafe { store.identifier() }.map(|value| value.as_bytes())
                    != Some(profile.bytes())
            {
                return Err(AgentOwnedViewConstructionError::Storage);
            }
            let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
            unsafe { configuration.setWebsiteDataStore(&store) };
            let actual_store = unsafe { configuration.websiteDataStore() };
            if Retained::as_ptr(&actual_store) != Retained::as_ptr(&store) {
                return Err(AgentOwnedViewConstructionError::Storage);
            }
            Ok(configuration)
        }
        (ContextProfileStorageClass::Durable, Some(_))
        | (ContextProfileStorageClass::Ephemeral, None) => {
            Err(AgentOwnedViewConstructionError::Storage)
        }
    }
}

/// Exact native page and its closed navigation policy handle.
pub(crate) struct AgentOwnedView {
    navigation: AgentNavigationController,
    semantic: Option<AgentSemanticRuntimeRegistration>,
    viewport: ContextOwnedViewport,
    view: WebView,
}

impl AgentOwnedView {
    pub(crate) const fn view(&self) -> &WebView {
        &self.view
    }

    pub(crate) const fn navigation(&self) -> &AgentNavigationController {
        &self.navigation
    }

    pub(crate) fn semantic(&self) -> Option<&AgentSemanticRuntimeController> {
        self.semantic
            .as_ref()
            .map(AgentSemanticRuntimeRegistration::controller)
    }

    pub(crate) fn prepare_semantic_document_load(&mut self) -> Result<(), ()> {
        self.semantic.as_mut().ok_or(())?.prepare_document_load()
    }

    pub(crate) fn retire_semantic_runtime(&mut self) -> bool {
        self.semantic
            .take()
            .is_some_and(|registration| registration.retire().is_ok())
    }

    pub(crate) fn dispatch_semantic(
        &self,
        invocation: SemanticRuntimeInvocation,
        completion: impl FnOnce(Result<SemanticSnapshot, SemanticRuntimePortFailure>) + 'static,
    ) -> Result<(), SemanticRuntimePortFailure> {
        let Some(semantic) = self.semantic() else {
            completion(Err(SemanticRuntimePortFailure::Retired));
            return Err(SemanticRuntimePortFailure::Retired);
        };
        semantic
            .dispatch(invocation, move |outcome| {
                completion(outcome.map_err(map_semantic_runtime_failure));
            })
            .map_err(map_semantic_dispatch_failure)
    }

    pub(crate) fn dispatch_screenshot(
        &self,
        request: SemanticScreenshotNativeRequest,
        admitted_at: Instant,
        cancelled: Arc<AtomicBool>,
        completion: impl FnOnce(Result<SemanticScreenshotNativeCapture, SemanticScreenshotNativeFailure>)
            + 'static,
        callback_panicked: impl Fn() + 'static,
    ) -> Result<(), SemanticScreenshotNativeFailure> {
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

    pub(crate) fn attest(
        &self,
        profile: ProfileId,
        storage_class: ContextProfileStorageClass,
        ephemeral_store: Option<&WebsiteDataStore>,
    ) -> Result<(), AgentOwnedViewConstructionError> {
        let semantic = self
            .semantic
            .as_ref()
            .ok_or(AgentOwnedViewConstructionError::ExtensionIsolation)?;
        attest_owned_agent_view(
            &self.view,
            semantic,
            self.viewport,
            profile,
            storage_class,
            ephemeral_store,
        )
    }
}

const fn map_semantic_dispatch_failure(
    failure: AgentSemanticRuntimeDispatchError,
) -> SemanticRuntimePortFailure {
    match failure {
        AgentSemanticRuntimeDispatchError::NotReady => SemanticRuntimePortFailure::NotReady,
        AgentSemanticRuntimeDispatchError::Busy => SemanticRuntimePortFailure::ResourceExhausted,
        AgentSemanticRuntimeDispatchError::Exhausted => SemanticRuntimePortFailure::InvocationLimit,
        AgentSemanticRuntimeDispatchError::Retired => SemanticRuntimePortFailure::Retired,
    }
}

const fn map_semantic_runtime_failure(
    failure: AgentSemanticRuntimeFailure,
) -> SemanticRuntimePortFailure {
    match failure {
        AgentSemanticRuntimeFailure::Dispatch(failure) => map_semantic_dispatch_failure(failure),
        AgentSemanticRuntimeFailure::Cancelled => SemanticRuntimePortFailure::Cancelled,
        AgentSemanticRuntimeFailure::DocumentReplaced => {
            SemanticRuntimePortFailure::DocumentReplaced
        }
        AgentSemanticRuntimeFailure::RendererLost => SemanticRuntimePortFailure::RendererLost,
        AgentSemanticRuntimeFailure::TimedOut => SemanticRuntimePortFailure::TimedOut,
        AgentSemanticRuntimeFailure::Retired => SemanticRuntimePortFailure::Retired,
        AgentSemanticRuntimeFailure::Transport => SemanticRuntimePortFailure::Transport,
        AgentSemanticRuntimeFailure::Result(failure) => SemanticRuntimePortFailure::Result(failure),
    }
}

/// Typed callback cohort retained by one native view delegate graph.
pub(crate) struct AgentOwnedViewCallbacks<Navigation, RendererLost, Invariant, Panic> {
    navigation: Navigation,
    renderer_lost: RendererLost,
    invariant: Invariant,
    panic: Panic,
}

impl<Navigation, RendererLost, Invariant, Panic>
    AgentOwnedViewCallbacks<Navigation, RendererLost, Invariant, Panic>
{
    pub(crate) const fn new(
        navigation: Navigation,
        renderer_lost: RendererLost,
        invariant: Invariant,
        panic: Panic,
    ) -> Self {
        Self {
            navigation,
            renderer_lost,
            invariant,
            panic,
        }
    }
}

/// Builds one initially hidden, extension-free selected-profile WKWebView.
///
/// The only initial document is `about:blank`. Network navigation remains
/// denied until a later exact context-navigation adapter is installed, so the
/// caller can attach native content policy before any web request exists.
pub(crate) fn build_owned_agent_view<Navigation, RendererLost, Invariant, Panic>(
    parent: &impl HasWindowHandle,
    viewport: ContextOwnedViewport,
    profile: ProfileId,
    storage_class: ContextProfileStorageClass,
    ephemeral_store: Option<&WebsiteDataStore>,
    callbacks: AgentOwnedViewCallbacks<Navigation, RendererLost, Invariant, Panic>,
) -> Result<AgentOwnedView, AgentOwnedViewConstructionError>
where
    Navigation: Fn(AgentNavigationTerminal) + 'static,
    RendererLost: Fn() + 'static,
    Invariant: Fn() + 'static,
    Panic: Fn() + 'static,
{
    let AgentOwnedViewCallbacks {
        navigation: on_navigation,
        renderer_lost: on_renderer_lost,
        invariant: on_invariant_failure,
        panic: on_callback_panic,
    } = callbacks;
    let navigation = AgentNavigationController::default();
    let navigation_policy = navigation.clone();
    let navigation_events = navigation.clone();
    let renderer_events = navigation.clone();
    let navigation_callback = Rc::new(on_navigation);
    let renderer_lost_callback = Rc::new(on_renderer_lost);
    let invariant_failure_callback = Rc::new(on_invariant_failure);
    let navigation_invariant_failure = invariant_failure_callback.clone();
    let renderer_invariant_failure = invariant_failure_callback.clone();
    let on_callback_panic = Rc::new(on_callback_panic);
    let navigation_callback_panicked = on_callback_panic.clone();
    let renderer_callback_panicked = on_callback_panic.clone();
    let configuration = new_owned_agent_configuration(profile, storage_class, ephemeral_store)?;
    let semantic = AgentSemanticRuntimeRegistration::install(
        &configuration,
        invariant_failure_callback,
        on_callback_panic,
    )
    .map_err(|_| AgentOwnedViewConstructionError::ExtensionIsolation)?;
    let navigation_semantic = semantic.controller().clone();
    let renderer_semantic = semantic.controller().clone();
    let builder = WebViewBuilder::new()
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
        .with_fullscreen_enabled(false)
        .with_picture_in_picture_enabled(false)
        .with_general_autofill_enabled(false)
        .with_navigation_handler(move |target| navigation_policy.allows(&target))
        .with_navigation_event_handler(move |event| match navigation_events.observe(event) {
            Ok(observation) => {
                if observation.did_commit_document() {
                    navigation_semantic.document_committed();
                }
                if let Some(terminal) = observation.into_terminal() {
                    invoke_owned_navigation_callback(
                        navigation_callback.as_ref(),
                        navigation_callback_panicked.as_ref(),
                        terminal,
                    );
                }
            }
            Err(()) => invoke_owned_unit_callback(
                navigation_invariant_failure.as_ref(),
                navigation_callback_panicked.as_ref(),
            ),
        })
        .with_on_web_content_process_terminate_handler(move || {
            match renderer_events.claim_renderer_loss() {
                Ok(true) => {
                    renderer_semantic.renderer_lost();
                    invoke_owned_unit_callback(
                        renderer_lost_callback.as_ref(),
                        renderer_callback_panicked.as_ref(),
                    );
                }
                Ok(false) => {}
                Err(()) => invoke_owned_unit_callback(
                    renderer_invariant_failure.as_ref(),
                    renderer_callback_panicked.as_ref(),
                ),
            }
        })
        .with_permission_handler(|_| wry::PermissionResponse::Deny)
        .with_download_policy(DownloadPolicy::DenyWithoutMetadata)
        .with_page_close_policy(PageClosePolicy::Ignore)
        .with_allow_link_preview(false)
        .with_webview_configuration(configuration);

    let builder = if storage_class == ContextProfileStorageClass::Ephemeral {
        builder.with_incognito(true)
    } else {
        builder
    };

    let view = builder
        .build_as_child(parent)
        .map_err(|_| AgentOwnedViewConstructionError::Native)?;
    semantic
        .bind_view(&super::native_webview(&view))
        .map_err(|_| AgentOwnedViewConstructionError::Native)?;
    harden_owned_agent_view(&view);
    attest_owned_agent_view(
        &view,
        &semantic,
        viewport,
        profile,
        storage_class,
        ephemeral_store,
    )?;
    Ok(AgentOwnedView {
        navigation,
        semantic: Some(semantic),
        viewport,
        view,
    })
}

fn harden_owned_agent_view(view: &WebView) {
    use objc2_app_kit::{NSAutoresizingMaskOptions as Mask, NSView};

    let page = super::native_webview(view);
    unsafe { page.setInspectable(false) };
    let native_view: &NSView = &page;
    native_view.setTranslatesAutoresizingMaskIntoConstraints(true);
    native_view.setAutoresizingMask(Mask::ViewNotSizable);
    native_view.setHidden(true);
}

pub(crate) fn attest_owned_agent_view(
    view: &WebView,
    semantic: &AgentSemanticRuntimeRegistration,
    viewport: ContextOwnedViewport,
    profile: ProfileId,
    storage_class: ContextProfileStorageClass,
    ephemeral_store: Option<&WebsiteDataStore>,
) -> Result<(), AgentOwnedViewConstructionError> {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSAutoresizingMaskOptions as Mask, NSView};

    let page = super::native_webview(view);
    let configuration = unsafe { page.configuration() };
    if unsafe { configuration.webExtensionController() }.is_some() {
        return Err(AgentOwnedViewConstructionError::ExtensionIsolation);
    }
    if semantic.attest_configuration(&configuration).is_err() {
        return Err(AgentOwnedViewConstructionError::ExtensionIsolation);
    }

    let actual_store = unsafe { configuration.websiteDataStore() };
    let persistent = unsafe { actual_store.isPersistent() };
    let identifier = unsafe { actual_store.identifier() }.map(|value| value.as_bytes());
    let storage_valid = match storage_class {
        ContextProfileStorageClass::Durable => {
            ephemeral_store.is_none() && persistent && identifier == Some(profile.bytes())
        }
        ContextProfileStorageClass::Ephemeral => {
            let Some(expected) = ephemeral_store else {
                return Err(AgentOwnedViewConstructionError::Storage);
            };
            !persistent
                && identifier.is_none()
                && Retained::as_ptr(&actual_store) == Retained::as_ptr(expected)
        }
    };
    if !storage_valid {
        return Err(AgentOwnedViewConstructionError::Storage);
    }

    let native_view: &NSView = &page;
    let frame = native_view.frame();
    if unsafe { page.isInspectable() }
        || native_view.autoresizingMask() != Mask::ViewNotSizable
        || frame.size.width != f64::from(viewport.width())
        || frame.size.height != f64::from(viewport.height())
        || !native_view.isHidden()
    {
        return Err(AgentOwnedViewConstructionError::Native);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use zephium_agentic::{
        ContextCapabilities, ContextId, ContextIdentity, ContextOperationId, ContextRegistry,
        ContextRunId,
    };
    use zephium_core::ids::ProfileId;

    fn navigation_operation() -> zephium_agentic::ContextOperationJoin {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            ProfileId::from(4),
            zephium_agentic::ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            zephium_agentic::ContextKind::Owned,
            &[zephium_agentic::ContextCapability::Navigate],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(
                identity.id(),
                construction,
                zephium_agentic::ContextSettlement::Applied,
            )
            .expect("settle construction");
        registry
            .begin_navigation(
                identity.id(),
                ContextOperationId::new(2).expect("operation"),
            )
            .expect("navigation")
    }

    fn recovery_operation() -> zephium_agentic::ContextOperationJoin {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            ProfileId::from(5),
            zephium_agentic::ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            zephium_agentic::ContextKind::Owned,
            &[zephium_agentic::ContextCapability::Recover],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(
                identity.id(),
                construction,
                zephium_agentic::ContextSettlement::Applied,
            )
            .expect("settle construction");
        let prior = registry.join(identity.id()).expect("join");
        registry
            .renderer_lost(identity.id(), prior)
            .expect("renderer loss");
        registry
            .begin_recovery(
                identity.id(),
                ContextOperationId::new(2).expect("operation"),
            )
            .expect("recovery")
    }

    #[test]
    fn construction_source_has_no_model_or_page_program_surface() {
        let source = include_str!("agent_context.rs");
        for forbidden in [
            concat!("with_ipc_", "handler"),
            concat!("with_initialization_", "script"),
            concat!("evaluate_", "script"),
            concat!("with_new_window_req_", "handler"),
            concat!("with_web_extension_", "controller"),
        ] {
            assert!(!source.contains(forbidden));
        }
        assert!(source.contains("with_navigation_handler(move |target|"));
        assert!(source.contains("state.bootstrap_available = false"));
        assert!(source.contains("controller.userScripts()"));
    }

    #[test]
    fn navigation_gate_consumes_bootstrap_and_denies_every_unarmed_page_target() {
        let gate = super::AgentNavigationController::default();
        assert!(gate.allows("about:blank"));
        assert!(!gate.allows("about:blank"));
        assert!(!gate.allows("https://example.test/"));
    }

    #[test]
    fn construction_bootstrap_commit_releases_semantic_readiness_without_a_shell_terminal() {
        let gate = super::AgentNavigationController::default();
        assert!(gate.allows("about:blank"));
        let started = gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(70),
                phase: wry::NavigationEventPhase::Started,
                url: "about:blank".to_owned(),
            })
            .expect("started");
        assert!(!started.did_commit_document());
        assert!(started.is_none());

        let committed = gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(70),
                phase: wry::NavigationEventPhase::Committed,
                url: "about:blank".to_owned(),
            })
            .expect("committed");
        assert!(committed.did_commit_document());
        assert!(committed.is_none());
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(70),
                phase: wry::NavigationEventPhase::Committed,
                url: "about:blank".to_owned(),
            })
            .expect("duplicate")
            .is_none());
    }

    #[test]
    fn navigation_gate_is_exact_and_has_one_commit_or_failure_winner() {
        let gate = super::AgentNavigationController::default();
        let operation = navigation_operation();
        let target = zephium_agentic::ContextNavigationTarget::parse("https://example.test/path")
            .expect("target");
        let terminal = Arc::new(AtomicBool::new(false));
        gate.arm(operation, target.clone(), terminal).expect("arm");
        assert!(gate.allows("https://example.test/path"));
        assert!(!gate.allows("https://example.test/redirect"));
        assert!(!gate.allows("about:blank"));

        // A delayed construction callback cannot settle the newly armed
        // shell operation because it lacks the matching native start.
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(99),
                phase: wry::NavigationEventPhase::Committed,
                url: "about:blank".to_owned(),
            })
            .expect("state")
            .is_none());
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(1),
                phase: wry::NavigationEventPhase::Started,
                url: "https://example.test/path".to_owned(),
            })
            .expect("state")
            .is_none());

        let committed = gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(1),
                phase: wry::NavigationEventPhase::Committed,
                url: "https://example.test/path".to_owned(),
            })
            .expect("state")
            .expect("commit");
        assert_eq!(committed.operation(), operation);
        assert_eq!(
            committed.into_outcome(),
            Ok(super::AgentNavigationCommit::Web(target))
        );
        assert!(!gate.allows("https://example.test/path"));
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(1),
                phase: wry::NavigationEventPhase::Failed,
                url: "https://example.test/path".to_owned(),
            })
            .expect("state")
            .is_none());
    }

    #[test]
    fn navigation_timeout_claim_prevents_a_late_native_commit() {
        let gate = super::AgentNavigationController::default();
        let operation = navigation_operation();
        let target = zephium_agentic::ContextNavigationTarget::parse("https://example.test/late")
            .expect("target");
        let terminal = Arc::new(AtomicBool::new(false));
        gate.arm(operation, target, terminal.clone()).expect("arm");
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(2),
                phase: wry::NavigationEventPhase::Started,
                url: "https://example.test/late".to_owned(),
            })
            .expect("state")
            .is_none());
        assert!(!terminal.swap(true, std::sync::atomic::Ordering::AcqRel));
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(2),
                phase: wry::NavigationEventPhase::Committed,
                url: "https://example.test/late".to_owned(),
            })
            .expect("state")
            .is_none());
        assert!(gate.disarm(operation));
    }

    #[test]
    fn renderer_loss_is_one_shot_and_revokes_an_armed_navigation() {
        let gate = super::AgentNavigationController::default();
        let operation = navigation_operation();
        let target =
            zephium_agentic::ContextNavigationTarget::parse("https://example.test/renderer-loss")
                .expect("target");
        let terminal = Arc::new(AtomicBool::new(false));
        gate.arm(operation, target, terminal.clone()).expect("arm");
        assert!(gate.allows("https://example.test/renderer-loss"));

        assert_eq!(gate.claim_renderer_loss(), Ok(true));
        assert!(terminal.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(gate.claim_renderer_loss(), Ok(false));
        assert!(!gate.allows("https://example.test/renderer-loss"));
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(3),
                phase: wry::NavigationEventPhase::Committed,
                url: "https://example.test/renderer-loss".to_owned(),
            })
            .expect("state")
            .is_none());
        assert!(gate.matches_for_audit(Some(operation), true));
        assert!(gate.disarm(operation));
        assert!(gate.matches_for_audit(None, true));
    }

    #[test]
    fn recovery_rearms_only_from_loss_and_restores_repeatable_loss_detection() {
        let gate = super::AgentNavigationController::default();
        let operation = recovery_operation();
        let terminal = Arc::new(AtomicBool::new(false));
        assert!(gate
            .arm_recovery(operation, None, terminal.clone())
            .is_err());
        assert_eq!(gate.claim_renderer_loss(), Ok(true));
        gate.arm_recovery(operation, None, terminal)
            .expect("recovery arm");
        assert!(gate.allows("about:blank"));
        assert!(!gate.allows("https://example.test/"));
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(4),
                phase: wry::NavigationEventPhase::Started,
                url: "about:blank".to_owned(),
            })
            .expect("state")
            .is_none());
        let committed = gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(4),
                phase: wry::NavigationEventPhase::Committed,
                url: "about:blank".to_owned(),
            })
            .expect("state")
            .expect("commit");
        assert_eq!(committed.operation(), operation);
        assert_eq!(
            committed.into_outcome(),
            Ok(super::AgentNavigationCommit::Bootstrap)
        );
        assert!(gate.settle_recovery(operation, true));
        assert!(gate.matches_for_audit(None, false));
        assert_eq!(gate.claim_renderer_loss(), Ok(true));
    }

    #[test]
    fn loss_after_recovery_commit_survives_terminal_settlement() {
        let gate = super::AgentNavigationController::default();
        let operation = recovery_operation();
        assert_eq!(gate.claim_renderer_loss(), Ok(true));
        gate.arm_recovery(operation, None, Arc::new(AtomicBool::new(false)))
            .expect("recovery arm");
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(5),
                phase: wry::NavigationEventPhase::Started,
                url: "about:blank".to_owned(),
            })
            .expect("state")
            .is_none());
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(5),
                phase: wry::NavigationEventPhase::Committed,
                url: "about:blank".to_owned(),
            })
            .expect("state")
            .is_some());
        assert_eq!(gate.claim_renderer_loss(), Ok(true));
        assert!(gate.settle_recovery(operation, true));
        assert!(gate.matches_for_audit(None, true));
    }

    #[test]
    fn failed_web_recovery_restores_loss_and_never_widens_its_target() {
        let gate = super::AgentNavigationController::default();
        let operation = recovery_operation();
        let target =
            zephium_agentic::ContextNavigationTarget::parse("https://example.test/recover")
                .expect("target");
        assert_eq!(gate.claim_renderer_loss(), Ok(true));
        gate.arm_recovery(operation, Some(target), Arc::new(AtomicBool::new(false)))
            .expect("recovery arm");
        assert!(gate.allows("https://example.test/recover"));
        assert!(!gate.allows("https://example.test/redirect"));
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(6),
                phase: wry::NavigationEventPhase::Started,
                url: "https://example.test/recover".to_owned(),
            })
            .expect("state")
            .is_none());
        let failed = gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(6),
                phase: wry::NavigationEventPhase::Failed,
                url: "https://example.test/recover".to_owned(),
            })
            .expect("state")
            .expect("failure");
        assert_eq!(
            failed.into_outcome(),
            Err(zephium_agentic::ContextPortFailure::NativeRefused)
        );
        assert!(gate.settle_recovery(operation, false));
        assert!(gate.matches_for_audit(None, true));
        assert!(!gate.allows("https://example.test/recover"));
    }
}
