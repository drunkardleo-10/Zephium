#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

//! Extension-free, hidden WKWebView construction for owned agent contexts.

use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use raw_window_handle::HasWindowHandle;
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::{
    DownloadPolicy, PageClosePolicy, Rect, WebView, WebViewBuilder, WebViewBuilderExtDarwin as _,
    WebViewBuilderExtMacos as _,
};
use zephium_agentic::{
    ContextOwnedViewport, ContextProfileStorageClass, SemanticActionNativeFailure,
    SemanticActionNativeRequest, SemanticActionNativeSettlement, SemanticRuntimeInvocation,
    SemanticRuntimePortFailure, SemanticScreenshotNativeCapture, SemanticScreenshotNativeFailure,
    SemanticScreenshotNativeRequest, SemanticSnapshot,
};
use zephium_core::ids::ProfileId;

use crate::platform::agent_navigation::AgentNavigationController;
pub(crate) use crate::platform::agent_navigation::{
    AgentNavigationCommit, AgentNavigationTerminal,
};

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
            // SAFETY: `mtm` proves all WebKit messages run on the main thread;
            // the UUID and returned retained objects remain live throughout
            // the calls, and each result is attested before use.
            let (store, configuration, actual_store) = unsafe {
                let store = WKWebsiteDataStore::dataStoreForIdentifier(&identifier, mtm);
                if !store.isPersistent()
                    || store.identifier().map(|value| value.as_bytes()) != Some(profile.bytes())
                {
                    return Err(AgentOwnedViewConstructionError::Storage);
                }
                let configuration = WKWebViewConfiguration::new(mtm);
                configuration.setWebsiteDataStore(&store);
                let actual_store = configuration.websiteDataStore();
                (store, configuration, actual_store)
            };
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
    work_navigation: Option<crate::platform::work_document_navigation::WorkDocumentNavigation>,
    semantic: Option<AgentSemanticRuntimeRegistration>,
    viewport: ContextOwnedViewport,
    _navigation_observer: super::InstalledNavigationObserver,
    view: WebView,
}

impl AgentOwnedView {
    pub(crate) const fn view(&self) -> &WebView {
        &self.view
    }

    pub(crate) const fn navigation(&self) -> &AgentNavigationController {
        &self.navigation
    }
    pub(crate) fn work_navigation(
        &self,
    ) -> Option<&crate::platform::work_document_navigation::WorkDocumentNavigation> {
        self.work_navigation.as_ref()
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

    pub(crate) fn dispatch_semantic_action(
        &self,
        request: SemanticActionNativeRequest,
        admitted_at: Instant,
        completion: impl FnOnce(SemanticActionNativeSettlement) + 'static,
    ) {
        let Some(semantic) = self.semantic() else {
            let completed_at = request.requested_at();
            completion(request.fail(SemanticActionNativeFailure::Shutdown, completed_at));
            return;
        };
        super::semantic_action::dispatch(&self.view, semantic, request, admitted_at, completion);
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
pub(crate) struct AgentOwnedViewCallbacks<Navigation, Location, RendererLost, Invariant, Panic> {
    navigation: Navigation,
    location: Location,
    renderer_lost: RendererLost,
    invariant: Invariant,
    panic: Panic,
}

impl<Navigation, Location, RendererLost, Invariant, Panic>
    AgentOwnedViewCallbacks<Navigation, Location, RendererLost, Invariant, Panic>
{
    pub(crate) const fn new(
        navigation: Navigation,
        location: Location,
        renderer_lost: RendererLost,
        invariant: Invariant,
        panic: Panic,
    ) -> Self {
        Self {
            navigation,
            location,
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
pub(crate) fn build_owned_agent_view<Navigation, Location, RendererLost, Invariant, Panic>(
    parent: &impl HasWindowHandle,
    viewport: ContextOwnedViewport,
    profile: ProfileId,
    storage_class: ContextProfileStorageClass,
    ephemeral_store: Option<&WebsiteDataStore>,
    callbacks: AgentOwnedViewCallbacks<Navigation, Location, RendererLost, Invariant, Panic>,
) -> Result<AgentOwnedView, AgentOwnedViewConstructionError>
where
    Navigation: Fn(AgentNavigationTerminal) + 'static,
    Location: Fn() + 'static,
    RendererLost: Fn() + 'static,
    Invariant: Fn() + 'static,
    Panic: Fn() + 'static,
{
    build_private_agent_view(
        parent,
        viewport,
        profile,
        storage_class,
        ephemeral_store,
        callbacks,
        None,
    )
}

/// Same hardened selected-profile constructor with a distinct run-free,
/// one-document Work navigation gate. It never uses the legacy controller.
pub(crate) fn build_owned_work_view<Navigation, Location, RendererLost, Invariant, Panic>(
    parent: &impl HasWindowHandle,
    viewport: ContextOwnedViewport,
    profile: ProfileId,
    storage_class: ContextProfileStorageClass,
    ephemeral_store: Option<&WebsiteDataStore>,
    callbacks: AgentOwnedViewCallbacks<Navigation, Location, RendererLost, Invariant, Panic>,
) -> Result<AgentOwnedView, AgentOwnedViewConstructionError>
where
    Navigation: Fn(AgentNavigationTerminal) + 'static,
    Location: Fn() + 'static,
    RendererLost: Fn() + 'static,
    Invariant: Fn() + 'static,
    Panic: Fn() + 'static,
{
    build_private_agent_view(
        parent,
        viewport,
        profile,
        storage_class,
        ephemeral_store,
        callbacks,
        Some(crate::platform::work_document_navigation::WorkDocumentNavigation::default()),
    )
}

fn build_private_agent_view<Navigation, Location, RendererLost, Invariant, Panic>(
    parent: &impl HasWindowHandle,
    viewport: ContextOwnedViewport,
    profile: ProfileId,
    storage_class: ContextProfileStorageClass,
    ephemeral_store: Option<&WebsiteDataStore>,
    callbacks: AgentOwnedViewCallbacks<Navigation, Location, RendererLost, Invariant, Panic>,
    work_navigation: Option<crate::platform::work_document_navigation::WorkDocumentNavigation>,
) -> Result<AgentOwnedView, AgentOwnedViewConstructionError>
where
    Navigation: Fn(AgentNavigationTerminal) + 'static,
    Location: Fn() + 'static,
    RendererLost: Fn() + 'static,
    Invariant: Fn() + 'static,
    Panic: Fn() + 'static,
{
    let AgentOwnedViewCallbacks {
        navigation: on_navigation,
        location: on_location,
        renderer_lost: on_renderer_lost,
        invariant: on_invariant_failure,
        panic: on_callback_panic,
    } = callbacks;
    let navigation = AgentNavigationController::default();
    let navigation_policy = navigation.clone();
    let navigation_events = navigation.clone();
    let renderer_events = navigation.clone();
    let work_policy = work_navigation.clone();
    let work_events = work_navigation.clone();
    let work_renderer = work_navigation.clone();
    let navigation_callback = Rc::new(on_navigation);
    let location_callback = Rc::new(on_location);
    let navigation_event_location_callback = location_callback.clone();
    let renderer_lost_callback = Rc::new(on_renderer_lost);
    let invariant_failure_callback = Rc::new(on_invariant_failure);
    let navigation_invariant_failure = invariant_failure_callback.clone();
    let renderer_invariant_failure = invariant_failure_callback.clone();
    let on_callback_panic = Rc::new(on_callback_panic);
    let navigation_callback_panicked = on_callback_panic.clone();
    let location_callback_panicked = on_callback_panic.clone();
    let renderer_callback_panicked = on_callback_panic.clone();
    let configuration = new_owned_agent_configuration(profile, storage_class, ephemeral_store)?;
    let semantic = AgentSemanticRuntimeRegistration::install(
        &configuration,
        invariant_failure_callback.clone(),
        on_callback_panic.clone(),
    )
    .map_err(|_| AgentOwnedViewConstructionError::ExtensionIsolation)?;
    let navigation_semantic = semantic.controller().clone();
    let renderer_semantic = semantic.controller().clone();
    let builder = WebViewBuilder::new()
        .with_url("about:blank")
        // Ready Work contexts must keep their bounded semantic channel runnable.
        // Preserve background CPU throttling; only the Rust lifecycle may close
        // this owned page. Do not opt Browse tabs out of their ordinary policy.
        .with_background_throttling(wry::BackgroundThrottlingPolicy::Throttle)
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
        .with_navigation_handler(move |target| {
            work_policy.as_ref().map_or_else(
                || navigation_policy.allows(&target),
                |gate| gate.allows(&target),
            )
        })
        .with_navigation_event_handler(move |event| {
            if let Some(gate) = &work_events {
                match gate.observe(event) {
                    Ok((committed, notify)) => {
                        if committed {
                            navigation_semantic.document_committed();
                        }
                        if notify {
                            invoke_owned_unit_callback(
                                navigation_event_location_callback.as_ref(),
                                location_callback_panicked.as_ref(),
                            );
                        }
                    }
                    Err(()) => invoke_owned_unit_callback(
                        navigation_invariant_failure.as_ref(),
                        navigation_callback_panicked.as_ref(),
                    ),
                }
                return;
            }
            match navigation_events.observe(event) {
                Ok(observation) => {
                    if observation.did_commit_document() {
                        navigation_semantic.document_committed();
                    }
                    if observation.should_check_location() {
                        invoke_owned_unit_callback(
                            navigation_event_location_callback.as_ref(),
                            location_callback_panicked.as_ref(),
                        );
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
            }
        })
        .with_on_web_content_process_terminate_handler(move || {
            if let Some(gate) = &work_renderer {
                gate.refuse();
                renderer_semantic.renderer_lost();
                invoke_owned_unit_callback(
                    renderer_lost_callback.as_ref(),
                    renderer_callback_panicked.as_ref(),
                );
                return;
            }
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
    harden_owned_agent_view(&view)?;
    attest_owned_agent_view(
        &view,
        &semantic,
        viewport,
        profile,
        storage_class,
        ephemeral_store,
    )?;
    let location_events = navigation.clone();
    let work_location = work_navigation.clone();
    let location_invariant = invariant_failure_callback.clone();
    let location_panic = on_callback_panic.clone();
    let navigation_observer =
        super::install_navigation_observer(&view, move || {
            match work_location.as_ref().map_or_else(
                || location_events.request_location_check(),
                crate::platform::work_document_navigation::WorkDocumentNavigation::location_changed,
            ) {
                Ok(true) => {
                    invoke_owned_unit_callback(location_callback.as_ref(), location_panic.as_ref())
                }
                Ok(false) => {}
                Err(()) => {
                    invoke_owned_unit_callback(location_invariant.as_ref(), location_panic.as_ref())
                }
            }
        })
        .map_err(|_| AgentOwnedViewConstructionError::Native)?;
    Ok(AgentOwnedView {
        navigation,
        work_navigation,
        semantic: Some(semantic),
        viewport,
        _navigation_observer: navigation_observer,
        view,
    })
}

fn harden_owned_agent_view(view: &WebView) -> Result<(), AgentOwnedViewConstructionError> {
    use objc2_app_kit::{NSAutoresizingMaskOptions as Mask, NSView};
    use objc2_foundation::MainThreadMarker;

    let _mtm = MainThreadMarker::new().ok_or(AgentOwnedViewConstructionError::Native)?;
    let page = super::native_webview(view);
    // SAFETY: the marker above proves main-thread access and `page` is the
    // retained WKWebView owned by the live Wry handle for this call.
    unsafe { page.setInspectable(false) };
    let native_view: &NSView = &page;
    native_view.setTranslatesAutoresizingMaskIntoConstraints(true);
    native_view.setAutoresizingMask(Mask::ViewNotSizable);
    native_view.setHidden(true);
    Ok(())
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
    use objc2_foundation::MainThreadMarker;

    let _mtm = MainThreadMarker::new().ok_or(AgentOwnedViewConstructionError::Native)?;
    let page = super::native_webview(view);
    // SAFETY: the marker above proves main-thread access; `page` and every
    // object returned from it are retained by objc2 for these bounded reads.
    let (configuration, extension_controller, scheduling) = unsafe {
        let configuration = page.configuration();
        let extension_controller = configuration.webExtensionController();
        let scheduling = configuration.preferences().inactiveSchedulingPolicy();
        (configuration, extension_controller, scheduling)
    };
    // Available since macOS 14.0, below Zephium's deployment floor. Ready
    // owned pages must remain runnable without disabling background CPU limits.
    if scheduling != objc2_web_kit::WKInactiveSchedulingPolicy::Throttle {
        return Err(AgentOwnedViewConstructionError::Native);
    }
    if extension_controller.is_some() {
        return Err(AgentOwnedViewConstructionError::ExtensionIsolation);
    }
    if semantic.attest_configuration(&configuration).is_err() {
        return Err(AgentOwnedViewConstructionError::ExtensionIsolation);
    }

    // SAFETY: `configuration` is the retained configuration of the live page;
    // its retained data store remains valid for these identity reads.
    let (actual_store, persistent, identifier) = unsafe {
        let actual_store = configuration.websiteDataStore();
        let persistent = actual_store.isPersistent();
        let identifier = actual_store.identifier().map(|value| value.as_bytes());
        (actual_store, persistent, identifier)
    };
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
    // SAFETY: main-thread access and the live retained page were proven above.
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
