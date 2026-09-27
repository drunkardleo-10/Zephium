//! Host-side ownership of Shell-projected extension window/tab routing.

#[cfg(target_os = "macos")]
use zephium_core::extensions::{
    ExtensionBrowserRequestId, ExtensionBrowserRequestSettlement,
    ExtensionCompatibilityBrokerRequestId, ExtensionCompatibilityBrokerSettlement,
    ExtensionRuntimeInstance,
};
use zephium_core::extensions::{ExtensionBrowserSurface, MAX_EXTENSION_BROWSER_WINDOWS};
#[cfg(target_os = "macos")]
use zephium_core::ids::ItemId;
use zephium_core::ids::ProfileId;
#[cfg(target_os = "macos")]
use zephium_core::ports::extensions::{
    ExtensionRuntimeGrantPromptSettlement, ExtensionRuntimeGrantRequestId,
};

#[cfg(target_os = "macos")]
use super::permits::{navigation_callback_matches, EventPermit};
#[cfg(target_os = "macos")]
use super::resources::{NativeResourceAdmissionError, NativeResourceClass};
use super::EngineHost;
#[cfg(target_os = "macos")]
use crate::navigation_epoch::{NavigationEpoch, NavigationEpochTracker};
#[cfg(target_os = "macos")]
use crate::platform::imp::{
    schedule_authorization_retry, NativeHostWorkerEvent, PublisherNativeMessagingAuthorization,
    PublisherNativeMessagingRequestId,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSString, NSURL};
#[cfg(target_os = "macos")]
use zephium_extension_runtime_api::ExtensionRuntimeHostBindError;

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
#[repr(usize)]
enum NativeMessagingDenial {
    SubjectAbsent,
    RuntimeAbsent,
    RuntimeError,
    OwnerAbsent,
    RequirementAbsent,
    OwnerError,
}

#[cfg(target_os = "macos")]
impl NativeMessagingDenial {
    const fn label(self) -> &'static str {
        match self {
            Self::SubjectAbsent => "subject-absent",
            Self::RuntimeAbsent => "runtime-absent",
            Self::RuntimeError => "runtime-error",
            Self::OwnerAbsent => "owner-absent",
            Self::RequirementAbsent => "requirement-absent",
            Self::OwnerError => "owner-error",
        }
    }
}

#[cfg(target_os = "macos")]
fn native_messaging_denial_due(count: &mut u64) -> bool {
    *count = count.saturating_add(1);
    count.is_power_of_two()
}

impl EngineHost {
    #[cfg(target_os = "macos")]
    fn report_native_messaging_denial(&mut self, reason: NativeMessagingDenial) {
        let count = &mut self.publisher_native_messaging_denials[reason as usize];
        if native_messaging_denial_due(count) {
            crate::diagnostic!(
                "extensions: publisher native messaging authorization was unavailable; reason={}; attempts={count}",
                reason.label()
            );
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn wake_matching_document_backgrounds(
        &mut self,
        id: ItemId,
        source_permit: &EventPermit,
        source_navigation: &NavigationEpochTracker,
        epoch: NavigationEpoch,
        target: &str,
    ) -> usize {
        let Some(view) = self.views.get(&id) else {
            return 0;
        };
        if !navigation_callback_matches(
            &view.event_permit,
            &view.navigation,
            source_permit,
            source_navigation,
            epoch,
        ) || !source_navigation.matches_current_target(epoch, target)
        {
            return 0;
        }
        let Some(profile) = self
            .partitions
            .get(&id)
            .map(|partition| partition.profile())
        else {
            return 0;
        };
        if !matches!(
            self.macos_extension_controllers
                .browser_surface_ready_for_document_background(profile),
            Ok(true)
        ) {
            return 0;
        }
        if !zephium_core::navigation::is_allowed_str(target) {
            return 0;
        }
        let runtimes = match self
            .extension_runtime_registry
            .published_runtime_slots(profile)
        {
            Ok(runtimes) => runtimes,
            Err(_) => {
                crate::diagnostic!(
                    "extensions: published runtime cohort was unavailable for background wake"
                );
                return 0;
            }
        };
        if runtimes.iter().all(Option::is_none) {
            return 0;
        }
        let Some(url) = NSURL::URLWithString(&NSString::from_str(target)) else {
            return 0;
        };
        let mut scheduled = 0_usize;
        for runtime in runtimes.into_iter().flatten() {
            match self
                .extension_runtime_registry
                .with_owned_macos_runtime(&runtime, |owner| {
                    owner.begin_matching_document_background_wake(&url)
                }) {
                Ok(Some(Ok(true))) => scheduled = scheduled.saturating_add(1),
                Ok(Some(Ok(false))) | Ok(None) => {}
                Ok(Some(Err(_))) | Err(_) => {
                    crate::diagnostic!("extensions: matching document background wake was refused")
                }
            }
        }
        scheduled
    }

    #[cfg(target_os = "macos")]
    pub(super) fn wake_resident_document_backgrounds(&mut self, profile: ProfileId) -> usize {
        if !matches!(
            self.macos_extension_controllers
                .browser_surface_ready_for_document_background(profile),
            Ok(true)
        ) {
            return 0;
        }
        let targets = self
            .views
            .iter()
            .filter_map(|(id, view)| {
                self.partitions
                    .get(id)
                    .filter(|partition| partition.profile() == profile)
                    .and_then(|_| crate::platform::imp::current_url(&view.view))
                    .filter(|target| zephium_core::navigation::is_allowed_str(target))
                    .and_then(|target| NSURL::URLWithString(&NSString::from_str(&target)))
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return 0;
        }
        let runtimes = match self
            .extension_runtime_registry
            .published_runtime_slots(profile)
        {
            Ok(runtimes) => runtimes,
            Err(_) => return 0,
        };
        let mut scheduled = 0_usize;
        for runtime in runtimes.into_iter().flatten() {
            for target in &targets {
                match self
                    .extension_runtime_registry
                    .with_owned_macos_runtime(&runtime, |owner| {
                        owner.begin_matching_document_background_wake(target)
                    }) {
                    Ok(Some(Ok(true))) => {
                        scheduled = scheduled.saturating_add(1);
                        break;
                    }
                    Ok(Some(Ok(false))) | Ok(None) => {}
                    Ok(Some(Err(_))) | Err(_) => {
                        crate::diagnostic!(
                            "extensions: resident document background wake was refused"
                        );
                    }
                }
            }
        }
        scheduled
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn settle_extension_browser_request(
        &mut self,
        profile: ProfileId,
        request: ExtensionBrowserRequestId,
        settlement: ExtensionBrowserRequestSettlement,
        page_token: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
        first_url_after_reply: Option<(
            std::sync::Arc<str>,
            zephium_core::ports::engine::NavigationRequestId,
        )>,
    ) -> bool {
        #[cfg(feature = "webext")]
        match self
            .webext
            .settle_browser_request(profile, request, settlement)
        {
            super::webext::BrowserRequestOutcome::NotOurs => {}
            super::webext::BrowserRequestOutcome::Settled => {
                if let (
                    ExtensionBrowserRequestSettlement::Applied(
                        zephium_core::extensions::ExtensionBrowserRequestResult::CreatedTab(tab),
                    ),
                    Some((url, intent)),
                ) = (settlement, first_url_after_reply)
                {
                    self.sink.emit(
                        zephium_core::ports::engine::EngineEvent::ExtensionCreatedTabReplied {
                            profile,
                            request,
                            tab,
                            url,
                            intent,
                        },
                    );
                }
                return true;
            }
            super::webext::BrowserRequestOutcome::Page {
                extension_id,
                url,
                done,
            } => {
                let ExtensionBrowserRequestSettlement::Applied(
                    zephium_core::extensions::ExtensionBrowserRequestResult::ExtensionPageAuthorized {
                        tab,
                        window,
                    },
                ) = settlement
                else {
                    return true;
                };
                let sink = self.sink.clone();
                let presented = match (self.ensure_stage(window), page_token) {
                    (Some(stage), Some(permit)) => self.webext.present_page(
                        profile,
                        tab,
                        stage,
                        permit,
                        &extension_id,
                        &url,
                        &sink,
                    ),
                    _ => Err("the window is gone".to_owned()),
                };
                match presented {
                    Ok(()) => {
                        if let Some(done) = done {
                            done(Ok(Some(self.webext.tab_number(profile, tab))));
                        }
                    }
                    Err(error) => {
                        eprintln!("extensions: could not show {extension_id} page: {error}");
                        if let Some(done) = done {
                            done(Err(error));
                        }
                        self.sink.emit(
                            zephium_core::ports::engine::EngineEvent::ExtensionPageClosed {
                                profile,
                                id: tab,
                            },
                        );
                    }
                }
                return true;
            }
        }
        let created_tab = match settlement {
            ExtensionBrowserRequestSettlement::Applied(
                zephium_core::extensions::ExtensionBrowserRequestResult::CreatedTab(tab),
            ) => Some(tab),
            _ => None,
        };
        let page = match settlement {
            ExtensionBrowserRequestSettlement::Applied(
                zephium_core::extensions::ExtensionBrowserRequestResult::ExtensionPageAuthorized {
                    tab,
                    window,
                },
            ) => Some((tab, window)),
            _ => None,
        };
        let stage = page
            .and_then(|(_, window)| self.ensure_stage(window))
            .zip(page_token);
        let mut settlement = settlement;
        let extension_page_lease = if matches!(
            settlement,
            ExtensionBrowserRequestSettlement::Applied(
                zephium_core::extensions::ExtensionBrowserRequestResult::ExtensionPageAuthorized { .. }
            )
        ) {
            match self.native_resources.try_acquire_extension_guest() {
                Ok(lease) => Some(lease),
                Err(error) => {
                    let reason = match error {
                        NativeResourceAdmissionError::ClassExhausted(
                            NativeResourceClass::Tab,
                        )
                        | NativeResourceAdmissionError::GlobalExhausted => {
                            zephium_core::extensions::ExtensionBrowserRequestRejection::CapacityExceeded
                        }
                        NativeResourceAdmissionError::ClassExhausted(_)
                        | NativeResourceAdmissionError::AccountingInvariant => {
                            zephium_core::extensions::ExtensionBrowserRequestRejection::NativeAdmissionFailed
                        }
                    };
                    settlement = ExtensionBrowserRequestSettlement::Rejected(reason);
                    None
                }
            }
        } else {
            None
        };
        let outcome = self.macos_extension_controllers.settle_browser_request(
            profile,
            request,
            settlement,
            extension_page_lease,
            stage,
        );
        if let (
            Ok(crate::platform::imp::ControllerBrowserRequestSettlement::Settled),
            Some(tab),
            Some((url, intent)),
        ) = (&outcome, created_tab, first_url_after_reply)
        {
            // The native tabs.create reply has been accepted for this exact
            // logical tab. Its first network effect enters Shell only now.
            self.sink.emit(
                zephium_core::ports::engine::EngineEvent::ExtensionCreatedTabReplied {
                    profile,
                    request,
                    tab,
                    url,
                    intent,
                },
            );
        }
        let settled = matches!(
            outcome,
            Ok(
                crate::platform::imp::ControllerBrowserRequestSettlement::Settled
                    | crate::platform::imp::ControllerBrowserRequestSettlement::Stale
            )
        );
        if let Some((id, _)) = page {
            if !self
                .macos_extension_controllers
                .has_extension_page(profile, id)
            {
                self.sink.emit(
                    zephium_core::ports::engine::EngineEvent::ExtensionPageClosed { profile, id },
                );
            }
        }
        settled
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn timeout_extension_browser_request(
        &mut self,
        profile: ProfileId,
        request: ExtensionBrowserRequestId,
    ) {
        let _ = self
            .macos_extension_controllers
            .timeout_browser_request(profile, request);
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn finalize_extension_compatibility_broker_request(
        &mut self,
        profile: ProfileId,
        request: ExtensionCompatibilityBrokerRequestId,
    ) {
        let subject = self
            .macos_extension_controllers
            .compatibility_broker_subject(profile, request)
            .ok()
            .flatten();
        let witness = subject.and_then(|(context, operation)| {
            self.extension_runtime_registry
                .compatibility_broker_witness_for_macos_context(
                    profile,
                    context,
                    operation.purpose(),
                )
                .ok()
                .flatten()
        });
        if witness.is_none() {
            crate::diagnostic!(
                "extensions: compatibility broker authorization was unavailable for current runtime"
            );
        }
        let _ = self
            .macos_extension_controllers
            .finalize_compatibility_broker_request(profile, request, witness);
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn settle_extension_compatibility_broker_request(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionCompatibilityBrokerRequestId,
        settlement: ExtensionCompatibilityBrokerSettlement,
    ) -> bool {
        matches!(
            self.macos_extension_controllers
                .settle_compatibility_broker_request(runtime, request, settlement),
            Ok(
                crate::platform::imp::ControllerCompatibilityBrokerSettlement::Settled
                    | crate::platform::imp::ControllerCompatibilityBrokerSettlement::Stale
            )
        )
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn timeout_extension_compatibility_broker_request(
        &mut self,
        profile: ProfileId,
        request: ExtensionCompatibilityBrokerRequestId,
    ) {
        let _ = self
            .macos_extension_controllers
            .timeout_compatibility_broker_request(profile, request);
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn finalize_extension_identity_request(
        &mut self,
        profile: ProfileId,
        request: crate::platform::imp::IdentityRequestId,
    ) {
        let context = self
            .macos_extension_controllers
            .identity_request_context(profile, request)
            .ok()
            .flatten();
        let witness = context.and_then(|context| {
            self.extension_runtime_registry
                .compatibility_broker_witness_for_macos_context(
                    profile,
                    context,
                    zephium_core::extensions::ExtensionCompatibilityBrokerPurpose::IdentityWebAuthFlow,
                )
                .ok()
                .flatten()
        });
        let lease = witness.as_ref().and_then(|_| {
            self.native_resources
                .try_acquire(NativeResourceClass::ExtensionAuxiliary)
                .ok()
        });
        let _ = self
            .macos_extension_controllers
            .finalize_identity_request(profile, request, witness, lease);
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn timeout_extension_identity_request(
        &mut self,
        profile: ProfileId,
        request: crate::platform::imp::IdentityRequestId,
    ) {
        let _ = self
            .macos_extension_controllers
            .timeout_identity_request(profile, request);
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn finalize_extension_offscreen_request(
        &mut self,
        profile: ProfileId,
        request: crate::platform::imp::OffscreenSessionId,
    ) {
        let subject = self
            .macos_extension_controllers
            .offscreen_subject(profile, request)
            .ok()
            .flatten();
        let Some((context, needs_lease)) = subject else {
            return;
        };
        let witness = self.extension_runtime_registry
            .compatibility_broker_witness_for_macos_context(
                profile,
                context,
                zephium_core::extensions::ExtensionCompatibilityBrokerPurpose::OffscreenLocalStorage,
            )
            .ok()
            .flatten();
        let lease = if needs_lease && witness.is_some() {
            self.native_resources
                .try_acquire(NativeResourceClass::ExtensionAuxiliary)
                .ok()
        } else {
            None
        };
        let _ = self
            .macos_extension_controllers
            .authorize_offscreen(profile, request, witness, lease);
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn retry_pending_extension_offscreen_authorization(&mut self, profile: ProfileId) {
        let pending = self
            .macos_extension_controllers
            .pending_offscreen_authorization_ids(profile);
        for request in pending {
            self.finalize_extension_offscreen_request(profile, request);
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn settle_isolated_extension_resource(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        kind: zephium_core::ports::extensions::IsolatedExtensionDocumentKind,
        request: u64,
        outcome: zephium_core::ports::extensions::IsolatedExtensionResourceOutcome,
    ) -> bool {
        match kind {
            zephium_core::ports::extensions::IsolatedExtensionDocumentKind::Offscreen => {
                let context = self
                    .macos_extension_controllers
                    .offscreen_resource_context(runtime)
                    .ok()
                    .flatten();
                let witness = context.and_then(|context| {
                    self.extension_runtime_registry
                        .compatibility_broker_witness_for_macos_context(
                            runtime.profile(),
                            context,
                            zephium_core::extensions::ExtensionCompatibilityBrokerPurpose::OffscreenLocalStorage,
                        )
                        .ok()
                        .flatten()
                });
                if !witness.as_ref().is_some_and(|witness| {
                    witness.runtime_instance() == runtime
                        && witness.purpose() == zephium_core::extensions::ExtensionCompatibilityBrokerPurpose::OffscreenLocalStorage
                }) {
                    if let Some(context) = context {
                        self.macos_extension_controllers.cancel_offscreen_context(runtime.profile(), context);
                    }
                    return false;
                }
                self.macos_extension_controllers
                    .settle_offscreen_resource(runtime, request, outcome)
                    .unwrap_or(false)
            }
            zephium_core::ports::extensions::IsolatedExtensionDocumentKind::Sandbox => false,
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn finalize_extension_native_messaging_request(
        &mut self,
        profile: ProfileId,
        request: PublisherNativeMessagingRequestId,
    ) {
        let subject = self
            .macos_extension_controllers
            .native_messaging_subject(profile, request)
            .ok()
            .flatten();
        let Some((context, requested_host)) = subject else {
            self.report_native_messaging_denial(NativeMessagingDenial::SubjectAbsent);
            let _ = self
                .macos_extension_controllers
                .authorize_native_messaging(profile, request, None);
            return;
        };
        let runtime = match self
            .extension_runtime_registry
            .published_runtime_for_macos_context(profile, context)
        {
            Ok(Some(runtime)) => runtime,
            Err(ExtensionRuntimeHostBindError::Unavailable)
                if self.defer_extension_native_messaging_authorization(profile, request) =>
            {
                return;
            }
            Ok(None) => {
                self.report_native_messaging_denial(NativeMessagingDenial::RuntimeAbsent);
                let _ = self
                    .macos_extension_controllers
                    .authorize_native_messaging(profile, request, None);
                return;
            }
            Err(_) => {
                self.report_native_messaging_denial(NativeMessagingDenial::RuntimeError);
                let _ = self
                    .macos_extension_controllers
                    .authorize_native_messaging(profile, request, None);
                return;
            }
        };
        let requirement =
            match self
                .extension_runtime_registry
                .with_owned_macos_runtime(&runtime, |owner| {
                    owner
                        .publisher_native_host()
                        .filter(|requirement| requirement.host_name() == &*requested_host)
                        .cloned()
                }) {
                Ok(Some(Some(requirement))) => requirement,
                Err(ExtensionRuntimeHostBindError::Unavailable)
                    if self.defer_extension_native_messaging_authorization(profile, request) =>
                {
                    return;
                }
                Ok(None) => {
                    self.report_native_messaging_denial(NativeMessagingDenial::OwnerAbsent);
                    let _ = self
                        .macos_extension_controllers
                        .authorize_native_messaging(profile, request, None);
                    return;
                }
                Ok(Some(None)) => {
                    self.report_native_messaging_denial(NativeMessagingDenial::RequirementAbsent);
                    let _ = self
                        .macos_extension_controllers
                        .authorize_native_messaging(profile, request, None);
                    return;
                }
                Err(_) => {
                    self.report_native_messaging_denial(NativeMessagingDenial::OwnerError);
                    let _ = self
                        .macos_extension_controllers
                        .authorize_native_messaging(profile, request, None);
                    return;
                }
            };
        let authorization =
            PublisherNativeMessagingAuthorization::new(runtime.instance(), requirement);
        let _ = self.macos_extension_controllers.authorize_native_messaging(
            profile,
            request,
            Some(authorization),
        );
    }

    #[cfg(target_os = "macos")]
    fn defer_extension_native_messaging_authorization(
        &mut self,
        profile: ProfileId,
        request: PublisherNativeMessagingRequestId,
    ) -> bool {
        let reserved = self
            .macos_extension_controllers
            .reserve_native_messaging_authorization_retry(profile, request)
            .unwrap_or(false);
        if reserved {
            schedule_authorization_retry(profile, request);
        }
        reserved
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn handle_extension_native_messaging_worker_event(
        &mut self,
        profile: ProfileId,
        request: PublisherNativeMessagingRequestId,
        event: NativeHostWorkerEvent,
    ) -> bool {
        self.macos_extension_controllers
            .handle_native_messaging_worker_event(profile, request, event)
            .unwrap_or(false)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn timeout_extension_native_messaging_request(
        &mut self,
        profile: ProfileId,
        request: PublisherNativeMessagingRequestId,
    ) {
        let _ = self
            .macos_extension_controllers
            .timeout_native_messaging(profile, request);
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn finalize_extension_runtime_grant_prompt(
        &mut self,
        profile: ProfileId,
        request: ExtensionRuntimeGrantRequestId,
    ) {
        let context = match self
            .macos_extension_controllers
            .runtime_grant_context_identity(profile, request)
        {
            Ok(Some(context)) => context,
            Ok(None) | Err(_) => {
                let _ = self
                    .macos_extension_controllers
                    .finalize_runtime_grant_request(profile, request, None);
                return;
            }
        };
        let subject = self
            .extension_runtime_registry
            .published_runtime_grant_subject_for_macos_context(profile, context)
            .ok()
            .flatten()
            .map(|(runtime, name)| {
                let instance = runtime.instance();
                let key = zephium_core::extensions::ExtensionNativeOwnershipKey::new(
                    instance.profile(),
                    instance.install_id(),
                    runtime.browsing_context(),
                );
                (instance, key, name)
            });
        let _ = self
            .macos_extension_controllers
            .finalize_runtime_grant_request(profile, request, subject);
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn settle_extension_runtime_grant_prompt(
        &mut self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
        mut settlement: ExtensionRuntimeGrantPromptSettlement,
    ) -> bool {
        let profile = runtime.profile();
        if settlement == ExtensionRuntimeGrantPromptSettlement::Granted {
            let current = self
                .macos_extension_controllers
                .runtime_grant_context_identity(profile, request)
                .ok()
                .flatten()
                .and_then(|context| {
                    self.extension_runtime_registry
                        .published_runtime_for_macos_context(profile, context)
                        .ok()
                        .flatten()
                })
                .map(|fingerprint| fingerprint.instance());
            if current != Some(runtime) {
                settlement = ExtensionRuntimeGrantPromptSettlement::Unavailable;
            }
        }
        matches!(
            self.macos_extension_controllers
                .settle_runtime_grant_request(runtime, request, settlement),
            Ok(
                crate::platform::imp::ControllerRuntimeGrantSettlement::Settled
                    | crate::platform::imp::ControllerRuntimeGrantSettlement::Stale
            )
        )
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn timeout_extension_runtime_grant_prompt(
        &mut self,
        profile: ProfileId,
        request: ExtensionRuntimeGrantRequestId,
    ) {
        let _ = self
            .macos_extension_controllers
            .timeout_runtime_grant_request(profile, request);
    }

    pub(crate) fn set_extension_browser_surface(
        &mut self,
        surface: ExtensionBrowserSurface,
    ) -> bool {
        let profile = surface.profile();
        if self.erasure_tombstones.contains(&profile)
            || (!self.extension_browser_surfaces.contains_key(&profile)
                && self.extension_browser_surfaces.len() >= MAX_EXTENSION_BROWSER_WINDOWS)
        {
            return false;
        }
        if let Some(current) = self.extension_browser_surfaces.get(&profile) {
            if surface.generation() < current.generation() {
                // A coalesced or delayed replaceable fact cannot roll the
                // native graph backward.
                return true;
            }
            if surface.generation() == current.generation() {
                return &surface == current;
            }
        }
        if surface.tabs().any(|tab| {
            self.partitions
                .get(&tab.id())
                .is_some_and(|partition| partition.profile() != profile)
        }) {
            return false;
        }

        #[cfg(feature = "webext")]
        {
            let views = &self.views;
            let partitions = &self.partitions;
            self.webext.publish(&surface, |id| {
                partitions
                    .get(&id)
                    .filter(|partition| partition.profile() == profile)
                    .and_then(|_| views.get(&id))
                    .map(|view| crate::platform::imp::native_webview(&view.view))
            });
            self.extension_browser_surfaces.insert(profile, surface);
            return true;
        }
        #[cfg(target_os = "macos")]
        #[allow(unreachable_code)]
        let was_ready = self
            .macos_extension_controllers
            .browser_surface_ready_for_document_background(profile)
            .unwrap_or(false);
        #[cfg(target_os = "macos")]
        {
            let views = &self.views;
            let partitions = &self.partitions;
            if self
                .macos_extension_controllers
                .apply_browser_surface(&surface, |id| {
                    partitions
                        .get(&id)
                        .filter(|partition| partition.profile() == profile)
                        .and_then(|_| views.get(&id))
                        .map(|view| crate::platform::imp::native_webview(&view.view))
                })
                .is_err()
            {
                return false;
            }
        }

        self.extension_browser_surfaces.insert(profile, surface);
        // The first usable tab can arrive through logical publication after
        // its native view was bound. Handle that ordering as well as the
        // existing view-binding/reconciliation paths, before a popup needs its
        // background. Ordinary title/tab metadata changes must not wake it.
        #[cfg(target_os = "macos")]
        if !was_ready
            && self
                .macos_extension_controllers
                .browser_surface_ready_for_document_background(profile)
                .unwrap_or(false)
        {
            let _ = self.wake_resident_document_backgrounds(profile);
        }
        true
    }

    #[cfg(target_os = "macos")]
    pub(super) fn reconcile_extension_browser_surface(
        &mut self,
        profile: ProfileId,
    ) -> Result<(), crate::platform::imp::ControllerRegistryError> {
        let Some(surface) = self.extension_browser_surfaces.get(&profile).cloned() else {
            return Ok(());
        };
        let views = &self.views;
        let partitions = &self.partitions;
        let was_ready = self
            .macos_extension_controllers
            .browser_surface_ready_for_document_background(profile)
            .unwrap_or(false);
        self.macos_extension_controllers
            .apply_browser_surface(&surface, |id| {
                partitions
                    .get(&id)
                    .filter(|partition| partition.profile() == profile)
                    .and_then(|_| views.get(&id))
                    .map(|view| crate::platform::imp::native_webview(&view.view))
            })?;
        let became_ready = !was_ready
            && self
                .macos_extension_controllers
                .browser_surface_ready_for_document_background(profile)
                .unwrap_or(false);
        if became_ready {
            let _ = self.wake_resident_document_backgrounds(profile);
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub(super) fn bind_extension_browser_surface_view(
        &mut self,
        profile: ProfileId,
        id: ItemId,
    ) -> bool {
        let webview = self
            .views
            .get(&id)
            .map(|view| crate::platform::imp::native_webview(&view.view));
        #[cfg(feature = "webext")]
        {
            self.webext.bind_view(profile, id, webview.as_deref());
            return true;
        }
        #[allow(unreachable_code)]
        let binding = self.macos_extension_controllers.bind_browser_surface_view(
            profile,
            id,
            webview.as_ref(),
        );
        if matches!(binding, Ok(true)) {
            let _ = self.wake_resident_document_backgrounds(profile);
        }
        binding.is_ok()
    }

    #[cfg(target_os = "macos")]
    pub(super) fn unbind_extension_browser_surface_view(
        &mut self,
        profile: ProfileId,
        id: ItemId,
    ) -> bool {
        #[cfg(feature = "webext")]
        {
            self.webext.bind_view(profile, id, None);
            return true;
        }
        #[allow(unreachable_code)]
        self.macos_extension_controllers
            .bind_browser_surface_view(profile, id, None)
            .is_ok()
    }

    pub(super) fn retire_extension_browser_surface(&mut self, profile: ProfileId) {
        self.extension_browser_surfaces.remove(&profile);
    }

    #[cfg(target_os = "macos")]
    pub(super) fn clear_retired_extension_browser_surface(&mut self, profile: ProfileId) -> bool {
        self.macos_extension_controllers
            .clear_browser_surface(profile)
            .is_ok()
    }
}

#[cfg(all(test, target_os = "macos"))]
mod native_messaging_diagnostic_tests {
    use super::native_messaging_denial_due;

    #[test]
    fn repeated_denials_emit_at_powers_of_two_and_never_wrap() {
        let mut count = 0;
        let emitted = (1..=1024)
            .filter(|_| native_messaging_denial_due(&mut count))
            .collect::<Vec<_>>();
        assert_eq!(emitted, [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024]);
        assert_eq!(count, 1024);

        count = u64::MAX - 1;
        assert!(!native_messaging_denial_due(&mut count));
        assert!(!native_messaging_denial_due(&mut count));
        assert_eq!(count, u64::MAX);
    }
}
