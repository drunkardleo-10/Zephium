//! Host-side ownership of Shell-projected extension window/tab routing.

use zephium_core::extensions::{
    ExtensionBrowserRequestId, ExtensionBrowserRequestSettlement, ExtensionBrowserSurface,
    ExtensionCompatibilityBrokerRequestId, ExtensionCompatibilityBrokerSettlement,
    ExtensionRuntimeInstance, MAX_EXTENSION_BROWSER_WINDOWS,
};
use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::extensions::{
    ExtensionRuntimeGrantPromptSettlement, ExtensionRuntimeGrantRequestId,
};

use super::EngineHost;

impl EngineHost {
    pub(crate) fn settle_extension_browser_request(
        &mut self,
        profile: ProfileId,
        request: ExtensionBrowserRequestId,
        settlement: ExtensionBrowserRequestSettlement,
    ) -> bool {
        #[cfg(target_os = "macos")]
        {
            matches!(
                self.macos_extension_controllers
                    .settle_browser_request(profile, request, settlement,),
                Ok(
                    crate::platform::imp::ControllerBrowserRequestSettlement::Settled
                        | crate::platform::imp::ControllerBrowserRequestSettlement::Stale
                )
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (profile, request, settlement);
            false
        }
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

    pub(crate) fn settle_extension_compatibility_broker_request(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionCompatibilityBrokerRequestId,
        settlement: ExtensionCompatibilityBrokerSettlement,
    ) -> bool {
        #[cfg(target_os = "macos")]
        {
            matches!(
                self.macos_extension_controllers
                    .settle_compatibility_broker_request(runtime, request, settlement),
                Ok(
                    crate::platform::imp::ControllerCompatibilityBrokerSettlement::Settled
                        | crate::platform::imp::ControllerCompatibilityBrokerSettlement::Stale
                )
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (runtime, request, settlement);
            false
        }
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
        self.macos_extension_controllers
            .apply_browser_surface(&surface, |id| {
                partitions
                    .get(&id)
                    .filter(|partition| partition.profile() == profile)
                    .and_then(|_| views.get(&id))
                    .map(|view| crate::platform::imp::native_webview(&view.view))
            })
            .map(|_| ())
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
        self.macos_extension_controllers
            .bind_browser_surface_view(profile, id, webview.as_ref())
            .is_ok()
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
