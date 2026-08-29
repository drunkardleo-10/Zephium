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

impl EngineHost {
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
    ) -> bool {
        let mut settlement = settlement;
        let extension_page_lease = if matches!(
            settlement,
            ExtensionBrowserRequestSettlement::Applied(
                zephium_core::extensions::ExtensionBrowserRequestResult::ExtensionPageAuthorized
            )
        ) {
            match self
                .native_resources
                .try_acquire(NativeResourceClass::ExtensionPopup)
            {
                Ok(lease) => Some(lease),
                Err(error) => {
                    let reason = match error {
                        NativeResourceAdmissionError::ClassExhausted(
                            NativeResourceClass::ExtensionPopup,
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
        matches!(
            self.macos_extension_controllers.settle_browser_request(
                profile,
                request,
                settlement,
                extension_page_lease,
            ),
            Ok(
                crate::platform::imp::ControllerBrowserRequestSettlement::Settled
                    | crate::platform::imp::ControllerBrowserRequestSettlement::Stale
            )
        )
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
            publisher_native_messaging_lab_diagnostic("subject-absent");
            crate::diagnostic!(
                "extensions: publisher native messaging authorization was unavailable"
            );
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
                publisher_native_messaging_lab_diagnostic("runtime-retry");
                return;
            }
            Ok(None) => {
                publisher_native_messaging_lab_diagnostic("runtime-absent");
                crate::diagnostic!(
                    "extensions: publisher native messaging authorization was unavailable"
                );
                let _ = self
                    .macos_extension_controllers
                    .authorize_native_messaging(profile, request, None);
                return;
            }
            Err(_) => {
                publisher_native_messaging_lab_diagnostic("runtime-error");
                crate::diagnostic!(
                    "extensions: publisher native messaging authorization was unavailable"
                );
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
                    publisher_native_messaging_lab_diagnostic("owner-retry");
                    return;
                }
                Ok(None) => {
                    publisher_native_messaging_lab_diagnostic("owner-absent");
                    crate::diagnostic!(
                        "extensions: publisher native messaging authorization was unavailable"
                    );
                    let _ = self
                        .macos_extension_controllers
                        .authorize_native_messaging(profile, request, None);
                    return;
                }
                Ok(Some(None)) => {
                    publisher_native_messaging_lab_diagnostic("requirement-absent");
                    publisher_native_messaging_host_lab_diagnostic(&requested_host);
                    crate::diagnostic!(
                        "extensions: publisher native messaging authorization was unavailable"
                    );
                    let _ = self
                        .macos_extension_controllers
                        .authorize_native_messaging(profile, request, None);
                    return;
                }
                Err(_) => {
                    publisher_native_messaging_lab_diagnostic("owner-error");
                    crate::diagnostic!(
                        "extensions: publisher native messaging authorization was unavailable"
                    );
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

#[cfg(all(target_os = "macos", feature = "native-extension-lab-diagnostics"))]
fn publisher_native_messaging_lab_diagnostic(phase: &'static str) {
    crate::diagnostic!("extensions: publisher native messaging host phase={phase}");
}

#[cfg(all(target_os = "macos", not(feature = "native-extension-lab-diagnostics")))]
fn publisher_native_messaging_lab_diagnostic(_phase: &'static str) {}

#[cfg(all(target_os = "macos", feature = "native-extension-lab-diagnostics"))]
fn publisher_native_messaging_host_lab_diagnostic(requested_host: &str) {
    crate::diagnostic!(
        "extensions: publisher native messaging requested unmatched host={requested_host}"
    );
}

#[cfg(all(target_os = "macos", not(feature = "native-extension-lab-diagnostics")))]
fn publisher_native_messaging_host_lab_diagnostic(_requested_host: &str) {}
