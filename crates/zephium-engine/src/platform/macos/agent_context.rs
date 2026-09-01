//! Extension-free, hidden WKWebView construction for owned agent contexts.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use raw_window_handle::HasWindowHandle;
use wry::{
    DownloadPolicy, NavigationEvent, NavigationEventPhase, PageClosePolicy, WebView,
    WebViewBuilder, WebViewBuilderExtDarwin as _, WebViewBuilderExtMacos as _,
};
use zephium_agentic::{
    ContextNavigationTarget, ContextOperationJoin, ContextPortFailure, ContextProfileStorageClass,
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

/// One exact terminal native observation for a shell-requested navigation.
pub(crate) struct AgentNavigationTerminal {
    operation: ContextOperationJoin,
    outcome: Result<ContextNavigationTarget, ContextPortFailure>,
}

impl AgentNavigationTerminal {
    pub(crate) const fn operation(&self) -> ContextOperationJoin {
        self.operation
    }

    pub(crate) fn into_outcome(self) -> Result<ContextNavigationTarget, ContextPortFailure> {
        self.outcome
    }
}

struct AgentNavigationArm {
    operation: ContextOperationJoin,
    target: ContextNavigationTarget,
    terminal_claimed: Arc<AtomicBool>,
    native_id: Option<wry::NavigationId>,
}

struct AgentNavigationState {
    bootstrap_available: bool,
    armed: Option<AgentNavigationArm>,
}

impl Default for AgentNavigationState {
    fn default() -> Self {
        Self {
            bootstrap_available: true,
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
        if operation.kind() != zephium_agentic::ContextOperationKind::Navigate
            || terminal_claimed.load(Ordering::Acquire)
        {
            return Err(());
        }
        let mut state = self.state.lock().map_err(|_| ())?;
        if state.armed.is_some() {
            return Err(());
        }
        state.bootstrap_available = false;
        state.armed = Some(AgentNavigationArm {
            operation,
            target,
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

    fn allows(&self, candidate: &str) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if let Some(armed) = state.armed.as_ref() {
            if armed.terminal_claimed.load(Ordering::Acquire) {
                return false;
            }
            return url::Url::parse(candidate)
                .ok()
                .is_some_and(|candidate| candidate == *armed.target.as_url());
        }
        if state.bootstrap_available && candidate == "about:blank" {
            state.bootstrap_available = false;
            return true;
        }
        false
    }

    fn observe(&self, event: NavigationEvent) -> Option<AgentNavigationTerminal> {
        let mut state = self.state.lock().ok()?;
        let armed = state.armed.as_mut()?;
        if event.phase == NavigationEventPhase::Started {
            let matches_target = ContextNavigationTarget::parse(&event.url)
                .ok()
                .is_some_and(|target| target == armed.target);
            if matches_target && armed.native_id.is_none() {
                armed.native_id = Some(event.id);
            }
            return None;
        }
        if !matches!(
            event.phase,
            NavigationEventPhase::Committed | NavigationEventPhase::Failed
        ) || armed.native_id != Some(event.id)
        {
            return None;
        }
        if armed
            .terminal_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        let operation = armed.operation;
        let expected = armed.target.clone();
        let outcome = match event.phase {
            NavigationEventPhase::Committed => match ContextNavigationTarget::parse(&event.url) {
                Ok(target) if target == expected => Ok(target),
                Ok(_) | Err(_) => Err(ContextPortFailure::NativeRefused),
            },
            NavigationEventPhase::Failed => Err(ContextPortFailure::NativeRefused),
            NavigationEventPhase::Started
            | NavigationEventPhase::Redirected
            | NavigationEventPhase::Finished => return None,
        };
        Some(AgentNavigationTerminal { operation, outcome })
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

/// Builds one initially hidden, extension-free selected-profile WKWebView.
///
/// The only initial document is `about:blank`. Network navigation remains
/// denied until a later exact context-navigation adapter is installed, so the
/// caller can attach native content policy before any web request exists.
pub(crate) fn build_owned_agent_view(
    parent: &impl HasWindowHandle,
    profile: ProfileId,
    storage_class: ContextProfileStorageClass,
    ephemeral_store: Option<&WebsiteDataStore>,
    on_navigation: impl Fn(AgentNavigationTerminal) + 'static,
) -> Result<AgentOwnedView, AgentOwnedViewConstructionError> {
    let navigation = AgentNavigationController::default();
    let navigation_policy = navigation.clone();
    let navigation_events = navigation.clone();
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
        .with_navigation_event_handler(move |event| {
            if let Some(terminal) = navigation_events.observe(event) {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    on_navigation(terminal);
                }));
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

fn attest_owned_agent_view(
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
            .is_none());
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(1),
                phase: wry::NavigationEventPhase::Started,
                url: "https://example.test/path".to_owned(),
            })
            .is_none());

        let committed = gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(1),
                phase: wry::NavigationEventPhase::Committed,
                url: "https://example.test/path".to_owned(),
            })
            .expect("commit");
        assert_eq!(committed.operation(), operation);
        assert_eq!(committed.into_outcome(), Ok(target));
        assert!(!gate.allows("https://example.test/path"));
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(1),
                phase: wry::NavigationEventPhase::Failed,
                url: "https://example.test/path".to_owned(),
            })
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
            .is_none());
        assert!(!terminal.swap(true, std::sync::atomic::Ordering::AcqRel));
        assert!(gate
            .observe(wry::NavigationEvent {
                id: wry::NavigationId::from_raw(2),
                phase: wry::NavigationEventPhase::Committed,
                url: "https://example.test/late".to_owned(),
            })
            .is_none());
        assert!(gate.disarm(operation));
    }
}
