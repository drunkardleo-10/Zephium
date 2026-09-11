#[cfg(feature = "agentic-browser")]
mod agent_context;
#[cfg(feature = "agentic-browser")]
mod agent_history;
#[cfg(feature = "agentic-browser")]
pub(crate) use agent_history::AgentHistoryBackTicket;
#[cfg(feature = "native-agentic-foreground-probe")]
pub(crate) mod agentic_foreground_driver;
#[cfg(feature = "native-agentic-foreground-probe")]
mod agentic_foreground_probe;
#[cfg(feature = "native-agentic-input-probe")]
mod agentic_input_probe;
#[cfg(feature = "native-agentic-work-resource-probe")]
pub(crate) mod agentic_resource_composition_probe;
#[cfg(feature = "native-agentic-work-resource-probe")]
pub(crate) mod agentic_resource_driver;
#[cfg(feature = "native-agentic-semantic-probe")]
mod agentic_semantic_probe;
#[cfg(feature = "native-agentic-foreground-probe")]
pub(crate) use agentic_foreground_probe::ForegroundRenderingLease;
mod content_filter;
mod credentials;
#[cfg(feature = "agentic-browser")]
mod semantic_action;
#[cfg(feature = "agentic-browser")]
mod semantic_runtime;
#[cfg(feature = "agentic-browser")]
mod semantic_screenshot;
#[cfg(feature = "agentic-browser")]
mod work_observation_presentation;
#[cfg(feature = "native-agentic-work-resource-probe")]
pub(crate) use work_observation_presentation::retained_page_hidden;
#[cfg(feature = "agentic-browser")]
pub(crate) use work_observation_presentation::{PresentationState, WorkObservationPresentation};
// Pure policy translation and the profile-scoped native extension lifecycle.
mod extensions;
mod native;
mod navigation;
mod stage;
#[cfg(feature = "native-web-extension-probes")]
mod web_extensions;

pub(crate) use extensions::{
    begin_prepared_native_runtime_activation, prepare_native_runtime_activation,
    schedule_authorization_retry, ControllerActionPopupPreparation,
    ControllerBrowserRequestSettlement, ControllerCommandDispatch,
    ControllerCompatibilityBrokerSettlement, ControllerErasureSettlement, ControllerErasureTicket,
    ControllerNamespaceRecoveryAudit, ControllerPreparation, ControllerRegistryError,
    ControllerRuntimeGrantSettlement, MacosNativeActionFailure, MacosNativeRuntimeActivation,
    MacosNativeRuntimeFailure, MacosNativeRuntimeOwner, MacosNativeRuntimeOwnerIdentity,
    MacosNativeRuntimeReconciliation, MacosNativeRuntimeRetirement, NativeHostWorkerEvent,
    PersistentControllerRegistry, ProfileControllerErasure, PublisherNativeMessagingAuthorization,
    PublisherNativeMessagingRequestId,
};
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use extensions::{ControllerSurfaceApplication, ProbeControllerPreparation};

pub(crate) use content_filter::{
    compile as compile_content_policy, content_policy_digest, enumerate_content_policy_cache,
    install_on_view as install_content_policy_on_view, remove_content_policy_cache_identifier,
    same_policy as same_content_policy, ContentPolicyCacheMaintenanceCancellation,
    ContentPolicyCachePage, ContentPolicyCompilationCancellation, ContentPolicyRegistration,
    NativeContentPolicy,
};
pub use credentials::{
    passkey_authorization_state, request_passkey_authorization,
    MacosPasskeyAuthorizationRequestFailure, MacosPasskeyAuthorizationState,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{cell::Cell, cell::RefCell, rc::Rc};

#[cfg(feature = "agentic-browser")]
pub(crate) use agent_context::{
    build_owned_agent_view, build_owned_work_view, AgentNavigationCommit, AgentNavigationTerminal,
    AgentOwnedView, AgentOwnedViewCallbacks, AgentOwnedViewConstructionError,
};
#[cfg(feature = "native-agentic-input-probe")]
pub(crate) use agentic_input_probe::run as run_agentic_input_matrix;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run as run_agentic_semantic_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run_history_runtime as run_agentic_history_runtime_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run_model_click as run_agentic_semantic_model_click_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run_model_public_fill as run_agentic_semantic_model_public_fill_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run_model_two_action as run_agentic_semantic_model_two_action_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run_model_workflow as run_agentic_semantic_model_workflow_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run_rendering as run_agentic_rendering_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run_rendering_opportunity as run_agentic_rendering_opportunity_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run_rendering_presented as run_agentic_rendering_presented_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run_work_actor as run_agentic_work_actor_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run_work_application as run_agentic_work_application_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub(crate) use agentic_semantic_probe::run_work_application_with_events as run_work_application_with_events_probe;
#[cfg(feature = "native-agentic-semantic-probe")]
pub use agentic_semantic_probe::MacosAgenticHistoryRuntimeProbeReport;
#[cfg(feature = "native-agentic-semantic-probe")]
pub use agentic_semantic_probe::MacosAgenticRenderingProbeReport;
#[cfg(feature = "native-agentic-semantic-probe")]
pub use agentic_semantic_probe::{
    MacosAgenticPresentedRenderingFailure, MacosAgenticPresentedRenderingReport,
};
#[cfg(feature = "native-agentic-semantic-probe")]
pub use agentic_semantic_probe::{MacosAgenticRenderingOpportunityReport, RenderingOpportunity};
#[cfg(feature = "native-agentic-semantic-probe")]
pub use agentic_semantic_probe::{
    MacosAgenticSemanticModelActionTerminal, MacosAgenticSemanticModelClickTerminal,
    MacosAgenticSemanticProbeAuthority, MacosAgenticSemanticTwoActionScenario,
};

use dispatch2::DispatchObject as _;

#[cfg(feature = "native-page-permission-probes")]
pub(crate) use native::run_page_permission_probe;
#[cfg(feature = "native-isolation-probes")]
pub(crate) use native::run_principal_isolation_probe;
pub(crate) use native::webkit as native_webview;
pub use native::{
    add_user_script, configure, query_document_activity, stop_loading, user_script_refusal,
    user_style_refusal,
};
pub(crate) use navigation::signal_same_document_navigation;
pub use navigation::NavigationObserver;
use objc2::rc::Retained;
use objc2_web_kit::{WKWebView, WKWebViewConfiguration, WKWebsiteDataStore};
pub use stage::ContentStage;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_bitwarden_core_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_extension_compatibility_fixture_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_onepassword_compatibility_artifact_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_onepassword_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_representative_extension_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_representative_stock_extension_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_resource_transport_probe as run_web_extension_resource_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_stock_password_manager_compatibility_artifact_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_stock_password_manager_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_vimium_compatibility_artifact_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_web_extension_alarm_delivery_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_web_extension_permission_callback_cohort_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_web_extension_permission_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_web_extension_permission_replacement_settlement_probe;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use web_extensions::run_web_extension_probe;
pub type InstalledNavigationObserver = objc2::rc::Retained<NavigationObserver>;

pub(crate) struct ContentPolicyTimeout {
    source: dispatch2::DispatchRetained<dispatch2::DispatchSource>,
}

impl ContentPolicyTimeout {
    pub(crate) fn cancel(self) {
        drop(self);
    }
}

impl Drop for ContentPolicyTimeout {
    fn drop(&mut self) {
        self.source.cancel();
    }
}

pub(crate) fn schedule_content_policy_timeout(
    duration: Duration,
    callback: impl FnOnce() + Send + 'static,
) -> Option<ContentPolicyTimeout> {
    let Ok(deadline) = dispatch2::DispatchTime::try_from(duration) else {
        return None;
    };
    let timer_type = std::ptr::addr_of!(dispatch2::_dispatch_source_type_timer).cast_mut();
    // SAFETY: the process-global timer source type is the exact libdispatch
    // constant required for a handle-less one-shot timer.
    let source = unsafe {
        dispatch2::DispatchSource::new(timer_type, 0, 0, Some(dispatch2::DispatchQueue::main()))
    };
    let callback = Arc::new(std::sync::Mutex::new(Some(callback)));
    let callback_for_handler = callback.clone();
    let handler: block2::RcBlock<dyn Fn()> = block2::RcBlock::new(move || {
        if let Some(callback) = callback_for_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback));
        }
    });
    // SAFETY: libdispatch copies the heap block and retains it until this
    // activated source fires or is canceled.
    unsafe {
        source.set_event_handler_with_block(block2::RcBlock::as_ptr(&handler));
    }
    source.set_timer(deadline, u64::MAX, 100_000_000);
    source.activate();
    Some(ContentPolicyTimeout { source })
}

const PAGE_URL_UTF16_LIMIT: usize = 8 * 1_024;
const PAGE_URL_UTF8_LIMIT: usize = 8 * 1_024;

pub fn install_navigation_observer(
    view: &wry::WebView,
    on_change: impl Fn(navigation::NavigationObservation) + 'static,
) -> Result<InstalledNavigationObserver, &'static str> {
    use objc2_foundation::MainThreadMarker;

    let mtm = MainThreadMarker::new().ok_or("WKWebView observer requires the main thread")?;
    Ok(NavigationObserver::install(
        mtm,
        &native::webkit(view),
        on_change,
    ))
}

pub fn current_url(view: &wry::WebView) -> Option<String> {
    bounded_current_url(view).ok()
}

#[derive(Clone, Copy)]
enum CurrentUrlUnavailable {
    MissingNativeUrl,
    MissingAbsoluteString,
    Utf16Limit,
    Utf8Limit,
}

fn bounded_current_url(view: &wry::WebView) -> Result<String, CurrentUrlUnavailable> {
    bounded_native_current_url_result(&native::webkit(view))
}

/// One allocation-bounded native URL sample for KVO evidence. Absence and
/// oversize intentionally collapse to `None`: Ready Work documents treat
/// either as refusal, while pre-Ready Work treats it as evidence only and
/// ordinary Browse uses the notification as a refresh request.
fn bounded_native_current_url(view: &WKWebView) -> Option<String> {
    bounded_native_current_url_result(view).ok()
}

fn bounded_native_current_url_result(view: &WKWebView) -> Result<String, CurrentUrlUnavailable> {
    let url = unsafe { view.URL() }.ok_or(CurrentUrlUnavailable::MissingNativeUrl)?;
    let value = url
        .absoluteString()
        .ok_or(CurrentUrlUnavailable::MissingAbsoluteString)?;
    bounded_absolute_url(&value)
}

fn bounded_absolute_url(
    value: &objc2_foundation::NSString,
) -> Result<String, CurrentUrlUnavailable> {
    if value.length() > PAGE_URL_UTF16_LIMIT {
        return Err(CurrentUrlUnavailable::Utf16Limit);
    }
    let value = value.to_string();
    (value.len() <= PAGE_URL_UTF8_LIMIT)
        .then_some(value)
        .ok_or(CurrentUrlUnavailable::Utf8Limit)
}

#[cfg(feature = "native-agentic-work-resource-probe")]
pub(crate) fn current_document_evidence(
    view: &wry::WebView,
    gate: &super::work_document_navigation::WorkDocumentNavigation,
    expected: &zephium_agentic::ContextNavigationTarget,
) -> (
    bool,
    super::work_document_navigation::CurrentDocumentEvidence,
) {
    use super::work_document_navigation::CurrentDocumentEvidence as E;
    match bounded_current_url(view) {
        Ok(current) => (gate.ready(Some(&current)), E::compare(expected, &current)),
        Err(CurrentUrlUnavailable::MissingNativeUrl) => (false, E::MissingNativeUrl),
        Err(CurrentUrlUnavailable::MissingAbsoluteString) => (false, E::MissingAbsoluteString),
        Err(CurrentUrlUnavailable::Utf16Limit) => (false, E::Utf16Limit),
        Err(CurrentUrlUnavailable::Utf8Limit) => (false, E::Utf8Limit),
    }
}

pub fn enforce_navigation_pending(view: &wry::WebView) -> bool {
    view.set_visible(false).is_ok()
}

pub(crate) type WebsiteDataStore = Retained<WKWebsiteDataStore>;

/// Allocate one non-persistent store for a private profile. The host retains
/// this object independently of every view so all tabs in that profile share
/// cookies and origin storage, while a construction failure cannot make the
/// native erasure obligation disappear.
pub(crate) fn new_ephemeral_data_store() -> Result<WebsiteDataStore, &'static str> {
    use objc2_foundation::MainThreadMarker;

    let mtm = MainThreadMarker::new().ok_or("WKWebsiteDataStore requires the main thread")?;
    let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
    validate_ephemeral_data_store(&store)?;
    Ok(store)
}

/// Build a fresh mutable configuration for one WKWebView while binding it to
/// the profile-owned store. Configurations themselves must never be shared:
/// Wry installs per-view scripts, delegates and scheme handlers into them.
pub(crate) fn new_configuration_with_data_store(
    store: &WebsiteDataStore,
) -> Result<Retained<WKWebViewConfiguration>, &'static str> {
    use objc2_foundation::MainThreadMarker;

    let mtm = MainThreadMarker::new().ok_or("WKWebViewConfiguration requires the main thread")?;
    validate_ephemeral_data_store(store)?;
    // SAFETY: `mtm` proves AppKit/WebKit main-thread affinity. `new` returns
    // an owned Objective-C object and has no additional preconditions.
    let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
    unsafe { configuration.setWebsiteDataStore(store) };
    let configured_store = unsafe { configuration.websiteDataStore() };
    if Retained::as_ptr(&configured_store) != Retained::as_ptr(store) {
        return Err("WKWebViewConfiguration did not retain the requested website data store");
    }
    Ok(configuration)
}

fn validate_ephemeral_data_store(store: &WebsiteDataStore) -> Result<(), &'static str> {
    if unsafe { store.isPersistent() } {
        return Err("private profile received a persistent WKWebsiteDataStore");
    }
    if unsafe { store.identifier() }.is_some() {
        return Err("private profile received an identifiable WKWebsiteDataStore");
    }
    Ok(())
}

#[derive(Clone)]
struct ProfileErasure {
    profile: zephium_core::ids::ProfileId,
    completion: Arc<crate::erasure::Completion>,
    attempt: Arc<AtomicBool>,
    controller_ticket: Option<ControllerErasureTicket>,
}

impl ProfileErasure {
    fn new(
        profile: zephium_core::ids::ProfileId,
        completion: Arc<crate::erasure::Completion>,
    ) -> Self {
        Self {
            profile,
            attempt: completion.attempt_flag(),
            completion,
            controller_ticket: None,
        }
    }

    fn with_controller_ticket(&self, ticket: ControllerErasureTicket) -> Self {
        Self {
            profile: self.profile,
            completion: self.completion.clone(),
            attempt: self.attempt.clone(),
            controller_ticket: Some(ticket),
        }
    }

    fn finish(&self, outcome: zephium_core::ports::engine::ProfileDataErasureOutcome) {
        self.completion.finish(outcome);
        if outcome == zephium_core::ports::engine::ProfileDataErasureOutcome::Verified {
            // `finish` first marks this exact attempt inactive. The host then
            // generation-checks the Arc before releasing its last strong
            // store handle, so a late callback cannot erase a retry's proof.
            crate::host::release_macos_erasure_obligation(
                self.profile,
                self.attempt.clone(),
                self.controller_ticket,
            );
        }
    }

    fn report_unsettled(&self, outcome: zephium_core::ports::engine::ProfileDataErasureOutcome) {
        self.completion.report_unsettled(outcome);
    }
}

#[derive(Default)]
struct EphemeralStoreCallbackGate {
    removal_seen: Cell<bool>,
    terminal_seen: Cell<bool>,
}

impl EphemeralStoreCallbackGate {
    fn admit_removal(&self) -> bool {
        !self.removal_seen.replace(true)
    }

    fn admit_terminal(&self) -> bool {
        !self.terminal_seen.replace(true)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EphemeralCohortProgress {
    Duplicate,
    Pending,
    LastVerified,
    LastFailed,
    CounterInvalid,
}

fn settle_ephemeral_obligation(
    gate: &EphemeralStoreCallbackGate,
    remaining: &AtomicUsize,
    failed: &AtomicBool,
    verified_empty: bool,
) -> EphemeralCohortProgress {
    if !gate.admit_terminal() {
        return EphemeralCohortProgress::Duplicate;
    }
    if !verified_empty {
        failed.store(true, Ordering::Release);
    }
    match remaining.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        current.checked_sub(1)
    }) {
        Ok(previous) if previous > 1 => EphemeralCohortProgress::Pending,
        Ok(1) if failed.load(Ordering::Acquire) => EphemeralCohortProgress::LastFailed,
        Ok(1) => EphemeralCohortProgress::LastVerified,
        // `checked_sub` refuses zero, so `Ok(0)` is not expected even under
        // an implementation change. Treat every impossible counter result as
        // a permanent proof failure without wrapping the counter.
        Ok(_) | Err(_) => {
            failed.store(true, Ordering::Release);
            EphemeralCohortProgress::CounterInvalid
        }
    }
}

fn apply_ephemeral_cohort_progress(
    progress: EphemeralCohortProgress,
    erasure: ProfileErasure,
    controller_erasure: Rc<RefCell<Option<ProfileControllerErasure>>>,
) {
    match progress {
        EphemeralCohortProgress::Duplicate | EphemeralCohortProgress::Pending => {}
        EphemeralCohortProgress::LastVerified => {
            erase_extension_controller_data(erasure, controller_erasure);
        }
        EphemeralCohortProgress::LastFailed | EphemeralCohortProgress::CounterInvalid => {
            // An ephemeral store has no durable identifier that a later
            // attempt can rediscover. Keep process-lifetime debt occupied;
            // restart is the only safe recovery without positive readback.
            erasure
                .report_unsettled(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
        }
    }
}

/// Remove a profile's named WKWebsiteDataStore only after the host has
/// released every WKWebView using it. Apple reports completion errors, but a
/// successful callback alone is not our proof boundary: enumerate the app's
/// identifiers again and acknowledge only when this identifier is absent.
pub(crate) fn erase_profile_data(
    profile: zephium_core::ids::ProfileId,
    mut ephemeral_stores: Vec<WebsiteDataStore>,
    controller_erasure: ProfileControllerErasure,
    completion: Arc<crate::erasure::Completion>,
) {
    let erasure = ProfileErasure::new(profile, completion);
    let controller_erasure = Rc::new(RefCell::new(Some(controller_erasure)));
    let mut seen = std::collections::HashSet::new();
    ephemeral_stores.retain(|store| seen.insert(Retained::as_ptr(store) as usize));
    if ephemeral_stores.is_empty() {
        erase_extension_controller_data(erasure, controller_erasure);
        return;
    }

    let remaining = Arc::new(AtomicUsize::new(ephemeral_stores.len()));
    let failed = Arc::new(AtomicBool::new(false));
    for store in ephemeral_stores {
        clear_and_verify_ephemeral_store(
            store,
            remaining.clone(),
            failed.clone(),
            erasure.clone(),
            controller_erasure.clone(),
        );
    }
}

fn clear_and_verify_ephemeral_store(
    store: WebsiteDataStore,
    remaining: Arc<AtomicUsize>,
    failed: Arc<AtomicBool>,
    erasure: ProfileErasure,
    controller_erasure: Rc<RefCell<Option<ProfileControllerErasure>>>,
) {
    use objc2_foundation::{MainThreadMarker, NSDate};

    let gate = Rc::new(EphemeralStoreCallbackGate::default());
    let Some(mtm) = MainThreadMarker::new() else {
        let progress = settle_ephemeral_obligation(&gate, &remaining, &failed, false);
        apply_ephemeral_cohort_progress(progress, erasure, controller_erasure);
        return;
    };
    let data_types = unsafe { WKWebsiteDataStore::allWebsiteDataTypes(mtm) };
    let epoch = NSDate::dateWithTimeIntervalSince1970(0.0);
    let verify_store = store.clone();
    let verify_types = data_types.clone();
    let removal_gate = gate.clone();
    let removed = block2::RcBlock::new(move || {
        use objc2_foundation::NSArray;
        use objc2_web_kit::WKWebsiteDataRecord;

        if !removal_gate.admit_removal() {
            return;
        }
        let retained_store = verify_store.clone();
        let remaining = remaining.clone();
        let failed = failed.clone();
        let erasure = erasure.clone();
        let controller_erasure = controller_erasure.clone();
        let terminal_gate = removal_gate.clone();
        let fetched = block2::RcBlock::new(
            move |records: std::ptr::NonNull<NSArray<WKWebsiteDataRecord>>| {
                let empty = unsafe { records.as_ref().count() == 0 };
                // A borrow is enough to keep the captured strong reference
                // alive while preserving the Fn (not FnOnce) block ABI.
                let _ = &retained_store;
                let progress =
                    settle_ephemeral_obligation(&terminal_gate, &remaining, &failed, empty);
                apply_ephemeral_cohort_progress(
                    progress,
                    erasure.clone(),
                    controller_erasure.clone(),
                );
            },
        );
        unsafe {
            verify_store.fetchDataRecordsOfTypes_completionHandler(&verify_types, &fetched);
        }
    });
    unsafe {
        store.removeDataOfTypes_modifiedSince_completionHandler(&data_types, &epoch, &removed);
    }
}

fn erase_extension_controller_data(
    erasure: ProfileErasure,
    controller_erasure: Rc<RefCell<Option<ProfileControllerErasure>>>,
) {
    let controller_erasure = controller_erasure
        .try_borrow_mut()
        .ok()
        .and_then(|mut controller_erasure| controller_erasure.take());
    let Some(controller_erasure) = controller_erasure else {
        // Losing the move-only controller plan would let a duplicate callback
        // skip native cleanup. Keep this attempt occupied for process life.
        erasure.report_unsettled(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
        return;
    };
    match controller_erasure {
        ProfileControllerErasure::NamespaceNotRequired => erase_named_profile_data(erasure),
        ProfileControllerErasure::ControllerAlreadyReleased(ticket) => {
            erase_named_profile_data(erasure.with_controller_ticket(ticket));
        }
        ProfileControllerErasure::Pending(controller) => {
            let completion = erasure.completion.clone();
            controller.start(completion, move |ticket| {
                erase_named_profile_data(erasure.with_controller_ticket(ticket));
            });
        }
    }
}

fn erase_named_profile_data(erasure: ProfileErasure) {
    use wry::WebViewExtDarwin;

    if initialize_data_store_enumeration().is_err() {
        erasure.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
        return;
    }
    let identifier = erasure.profile.bytes();
    let initial_erasure = erasure.clone();
    let result =
        <wry::WebView as WebViewExtDarwin>::fetch_data_store_identifiers(move |identifiers| {
            if !identifiers.contains(&identifier) {
                initial_erasure
                    .finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Verified);
                return;
            }

            let removed_erasure = initial_erasure.clone();
            <wry::WebView as WebViewExtDarwin>::remove_data_store(&identifier, move |removed| {
                if removed.is_err() {
                    removed_erasure
                        .finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
                    return;
                }

                let verified_erasure = removed_erasure.clone();
                if <wry::WebView as WebViewExtDarwin>::fetch_data_store_identifiers(
                    move |identifiers| {
                        let outcome = if identifiers.contains(&identifier) {
                            zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
                        } else {
                            zephium_core::ports::engine::ProfileDataErasureOutcome::Verified
                        };
                        verified_erasure.finish(outcome);
                    },
                )
                .is_err()
                {
                    removed_erasure
                        .finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
                }
            });
        });
    if result.is_err() {
        erasure.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
    }
}

/// Cold recovery may enumerate before any view or data store has initialized
/// WebKit's main run loop. A configuration's API::Object constructor performs
/// that initialization; enumeration itself does not. Do not access its lazy
/// data-store/process-pool properties or create a view or named profile here.
fn initialize_data_store_enumeration() -> Result<(), &'static str> {
    let mtm = objc2_foundation::MainThreadMarker::new()
        .ok_or("profile erasure requires the main thread")?;
    // SAFETY: the marker proves main-thread construction, the local retained
    // configuration is immediately released, and no browser/data-store getter
    // is invoked. The initialized WebKit run loop is process-owned.
    drop(unsafe { objc2_web_kit::WKWebViewConfiguration::new(mtm) });
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn current_url_reader_preserves_exact_utf16_and_utf8_bounds() {
        use super::*;
        use objc2_foundation::NSString;
        assert_eq!(
            bounded_absolute_url(&NSString::from_str(&"a".repeat(PAGE_URL_UTF8_LIMIT)))
                .ok()
                .unwrap()
                .len(),
            PAGE_URL_UTF8_LIMIT
        );
        assert!(matches!(
            bounded_absolute_url(&NSString::from_str(&"a".repeat(PAGE_URL_UTF16_LIMIT + 1))),
            Err(CurrentUrlUnavailable::Utf16Limit)
        ));
        assert!(matches!(
            bounded_absolute_url(&NSString::from_str(&"\u{0800}".repeat(2731))),
            Err(CurrentUrlUnavailable::Utf8Limit)
        ));
    }
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn cold_erasure_initializes_only_a_configuration_before_identifier_enumeration() {
        let source = include_str!("mod.rs");
        let erasure = source
            .split("fn erase_named_profile_data(")
            .nth(1)
            .unwrap()
            .split("/// Cold recovery")
            .next()
            .unwrap();
        assert!(
            erasure.find("initialize_data_store_enumeration()").unwrap()
                < erasure.find("fetch_data_store_identifiers").unwrap()
        );
        let initialization = source
            .split("fn initialize_data_store_enumeration()")
            .nth(1)
            .unwrap()
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(initialization.contains("MainThreadMarker::new()"));
        assert!(initialization.contains("WKWebViewConfiguration::new(mtm)"));
        for forbidden in [
            "websiteDataStore()",
            "dataStoreForIdentifier",
            "defaultDataStore",
            "processPool()",
            "WKWebView::",
        ] {
            assert!(!initialization.contains(forbidden));
        }
    }

    #[test]
    fn failed_ephemeral_verification_cannot_be_forgotten_by_a_retry() {
        let (tx, rx) = mpsc::channel();
        let attempt = Arc::new(AtomicBool::new(true));
        let completion = crate::erasure::Completion::start(
            Box::new(move |outcome| tx.send(outcome).unwrap()),
            attempt.clone(),
        );

        let remaining = AtomicUsize::new(1);
        let failed = AtomicBool::new(false);
        let progress = settle_ephemeral_obligation(
            &EphemeralStoreCallbackGate::default(),
            &remaining,
            &failed,
            false,
        );
        apply_ephemeral_cohort_progress(
            progress,
            ProfileErasure::new(zephium_core::ids::ProfileId::from(7), completion),
            Rc::new(RefCell::new(Some(
                ProfileControllerErasure::NamespaceNotRequired,
            ))),
        );

        assert_eq!(
            rx.recv_timeout(Duration::from_millis(100)).unwrap(),
            zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
        );
        assert!(attempt.load(Ordering::Acquire));
    }

    #[test]
    fn duplicate_callbacks_cannot_settle_a_two_store_cohort_twice() {
        let remaining = AtomicUsize::new(2);
        let failed = AtomicBool::new(false);
        let first = EphemeralStoreCallbackGate::default();
        let second = EphemeralStoreCallbackGate::default();
        let mut controller_starts = 0;

        assert!(first.admit_removal());
        assert!(!first.admit_removal());
        assert_eq!(
            settle_ephemeral_obligation(&first, &remaining, &failed, true),
            EphemeralCohortProgress::Pending
        );
        assert_eq!(remaining.load(Ordering::Acquire), 1);
        assert_eq!(
            settle_ephemeral_obligation(&first, &remaining, &failed, false),
            EphemeralCohortProgress::Duplicate
        );
        assert_eq!(remaining.load(Ordering::Acquire), 1);
        assert!(!failed.load(Ordering::Acquire));

        assert!(second.admit_removal());
        let last = settle_ephemeral_obligation(&second, &remaining, &failed, true);
        if last == EphemeralCohortProgress::LastVerified {
            controller_starts += 1;
        }
        assert_eq!(last, EphemeralCohortProgress::LastVerified);
        assert_eq!(controller_starts, 1);
        assert_eq!(remaining.load(Ordering::Acquire), 0);

        // A late failing callback from either native store is a duplicate. It
        // cannot underflow the cohort, alter the aggregate proof, or trigger a
        // second terminal action after verification.
        for gate in [&first, &second] {
            let duplicate = settle_ephemeral_obligation(gate, &remaining, &failed, false);
            if duplicate == EphemeralCohortProgress::LastVerified {
                controller_starts += 1;
            }
            assert_eq!(duplicate, EphemeralCohortProgress::Duplicate);
        }
        assert_eq!(controller_starts, 1);
        assert_eq!(remaining.load(Ordering::Acquire), 0);
        assert!(!failed.load(Ordering::Acquire));
    }

    #[test]
    fn unused_gate_cannot_wrap_an_exhausted_cohort_counter() {
        let remaining = AtomicUsize::new(0);
        let failed = AtomicBool::new(false);
        assert_eq!(
            settle_ephemeral_obligation(
                &EphemeralStoreCallbackGate::default(),
                &remaining,
                &failed,
                true,
            ),
            EphemeralCohortProgress::CounterInvalid
        );
        assert_eq!(remaining.load(Ordering::Acquire), 0);
        assert!(failed.load(Ordering::Acquire));
    }
}
