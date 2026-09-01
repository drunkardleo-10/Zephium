//! Extension-free, hidden WKWebView construction for owned agent contexts.

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use raw_window_handle::HasWindowHandle;
use wry::{
    DownloadPolicy, NavigationEvent, NavigationEventPhase, PageClosePolicy, WebView,
    WebViewBuilder, WebViewBuilderExtDarwin as _, WebViewBuilderExtMacos as _,
};
use zephium_agentic::{
    ContextNavigationTarget, ContextOperationJoin, ContextOperationKind, ContextPortFailure,
    ContextProfileStorageClass,
};
use zephium_core::ids::ProfileId;

use super::WebsiteDataStore;

/// Closed construction failure mapped to the public native-port taxonomy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentOwnedViewConstructionError {
    /// The exact selected-profile storage class or identity was not retained.
    Storage,
    /// Extension or user-script absence could not be proven.
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
    renderer_lost: bool,
    armed: Option<AgentNavigationArm>,
}

impl Default for AgentNavigationState {
    fn default() -> Self {
        Self {
            bootstrap_available: true,
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
            return true;
        }
        false
    }

    fn observe(&self, event: NavigationEvent) -> Result<Option<AgentNavigationTerminal>, ()> {
        let mut state = self.state.lock().map_err(|_| ())?;
        if state.renderer_lost {
            return Ok(None);
        }
        let Some(armed) = state.armed.as_mut() else {
            return Ok(None);
        };
        if event.phase == NavigationEventPhase::Started {
            let matches_target = armed.expected.matches(&event.url);
            if matches_target && armed.native_id.is_none() {
                armed.native_id = Some(event.id);
            }
            return Ok(None);
        }
        if !matches!(
            event.phase,
            NavigationEventPhase::Committed | NavigationEventPhase::Failed
        ) || armed.native_id != Some(event.id)
        {
            return Ok(None);
        }
        if armed
            .terminal_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(None);
        }
        let operation = armed.operation;
        let expected = armed.expected.clone();
        let outcome = match event.phase {
            NavigationEventPhase::Committed => expected.commit(&event.url),
            NavigationEventPhase::Failed => Err(ContextPortFailure::NativeRefused),
            NavigationEventPhase::Started
            | NavigationEventPhase::Redirected
            | NavigationEventPhase::Finished => return Ok(None),
        };
        Ok(Some(AgentNavigationTerminal { operation, outcome }))
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

/// Exact native page and its closed navigation policy handle.
pub(crate) struct AgentOwnedView {
    navigation: AgentNavigationController,
    view: WebView,
}

impl AgentOwnedView {
    pub(crate) const fn view(&self) -> &WebView {
        &self.view
    }

    pub(crate) const fn navigation(&self) -> &AgentNavigationController {
        &self.navigation
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
    let builder = WebViewBuilder::new()
        .with_url("about:blank")
        .with_visible(false)
        .with_focused(false)
        .with_devtools(false)
        .with_autoplay(false)
        .with_fullscreen_enabled(false)
        .with_picture_in_picture_enabled(false)
        .with_general_autofill_enabled(false)
        .with_navigation_handler(move |target| navigation_policy.allows(&target))
        .with_navigation_event_handler(move |event| match navigation_events.observe(event) {
            Ok(Some(terminal)) => invoke_owned_navigation_callback(
                navigation_callback.as_ref(),
                navigation_callback_panicked.as_ref(),
                terminal,
            ),
            Ok(None) => {}
            Err(()) => invoke_owned_unit_callback(
                navigation_invariant_failure.as_ref(),
                navigation_callback_panicked.as_ref(),
            ),
        })
        .with_on_web_content_process_terminate_handler(move || {
            match renderer_events.claim_renderer_loss() {
                Ok(true) => invoke_owned_unit_callback(
                    renderer_lost_callback.as_ref(),
                    renderer_callback_panicked.as_ref(),
                ),
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
        .with_allow_link_preview(false);

    let builder = match (storage_class, ephemeral_store) {
        (ContextProfileStorageClass::Ephemeral, Some(store)) => {
            let configuration = super::new_configuration_with_data_store(store)
                .map_err(|_| AgentOwnedViewConstructionError::Storage)?;
            builder
                .with_incognito(true)
                .with_webview_configuration(configuration)
        }
        (ContextProfileStorageClass::Durable, None) => {
            builder.with_data_store_identifier(profile.bytes())
        }
        (ContextProfileStorageClass::Durable, Some(_))
        | (ContextProfileStorageClass::Ephemeral, None) => {
            return Err(AgentOwnedViewConstructionError::Storage);
        }
    };

    let view = builder
        .build_as_child(parent)
        .map_err(|_| AgentOwnedViewConstructionError::Native)?;
    attest_owned_agent_view(&view, profile, storage_class, ephemeral_store)?;
    Ok(AgentOwnedView { navigation, view })
}

pub(crate) fn attest_owned_agent_view(
    view: &WebView,
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
    let controller = unsafe { configuration.userContentController() };
    if unsafe { controller.userScripts() }.count() != 0 {
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

    unsafe { page.setInspectable(false) };
    let native_view: &NSView = &page;
    native_view.setTranslatesAutoresizingMaskIntoConstraints(true);
    native_view.setAutoresizingMask(Mask::ViewWidthSizable | Mask::ViewHeightSizable);
    native_view.setHidden(true);
    if !native_view.isHidden() {
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
