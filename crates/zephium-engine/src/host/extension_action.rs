//! Product action projection at the authenticated runtime/controller/tab join.

use zephium_core::extensions::{
    ExtensionActionRejection, ExtensionActionSnapshot, ExtensionActionSnapshotSettlement,
    ExtensionBrowserSurfaceGeneration,
};
use zephium_core::ids::{ItemId, ProfileId};
use zephium_extension_runtime_api::ExtensionRuntimeHostBindError;

use crate::platform::imp::{ControllerRegistryError, MacosNativeActionFailure};

use super::EngineHost;

impl EngineHost {
    /// Reads effective action metadata for all published runtimes in one exact
    /// profile/tab surface generation. This path cannot create a native view:
    /// controller lookup returns only the logical `WKWebExtensionTab`, and
    /// `actionForTab:` exposes metadata without touching `popupWebView`.
    pub(crate) fn extension_actions_snapshot(
        &mut self,
        profile: ProfileId,
        tab: ItemId,
        surface_generation: ExtensionBrowserSurfaceGeneration,
    ) -> ExtensionActionSnapshotSettlement {
        let native_tab =
            match self
                .macos_extension_controllers
                .action_tab(profile, surface_generation, tab)
            {
                Ok(Some(tab)) => tab,
                Ok(None) => {
                    return ExtensionActionSnapshotSettlement::Rejected(
                        ExtensionActionRejection::TabUnavailable,
                    );
                }
                Err(error) => {
                    return ExtensionActionSnapshotSettlement::Rejected(map_controller_error(
                        error,
                    ));
                }
            };
        let runtimes = match self.extension_runtime_registry.published_runtimes(profile) {
            Ok(runtimes) => runtimes,
            Err(error) => {
                return ExtensionActionSnapshotSettlement::Rejected(map_runtime_error(error));
            }
        };
        let mut actions = Vec::with_capacity(runtimes.len());
        for runtime in runtimes {
            let state = match self
                .extension_runtime_registry
                .with_owned_macos_runtime(&runtime, |owner| {
                    owner.action_state_for_tab(runtime.instance(), tab, &native_tab)
                }) {
                Ok(Some(Ok(state))) => state,
                Ok(Some(Err(MacosNativeActionFailure::ActionUnavailable))) => continue,
                Ok(Some(Err(error))) => {
                    return ExtensionActionSnapshotSettlement::Rejected(map_native_error(error));
                }
                Ok(None) => {
                    return ExtensionActionSnapshotSettlement::Rejected(
                        ExtensionActionRejection::RuntimeUnavailable,
                    );
                }
                Err(error) => {
                    return ExtensionActionSnapshotSettlement::Rejected(map_runtime_error(error));
                }
            };
            actions.push(state);
        }
        match ExtensionActionSnapshot::new(profile, tab, surface_generation, actions) {
            Ok(snapshot) => ExtensionActionSnapshotSettlement::Applied(snapshot),
            Err(_) => ExtensionActionSnapshotSettlement::Rejected(
                ExtensionActionRejection::NativeAdmissionFailed,
            ),
        }
    }
}

fn map_controller_error(error: ControllerRegistryError) -> ExtensionActionRejection {
    match error {
        ControllerRegistryError::BrowserSurfaceStale => ExtensionActionRejection::RuntimeSuperseded,
        ControllerRegistryError::CapacityExceeded => ExtensionActionRejection::CapacityExceeded,
        ControllerRegistryError::Sealed | ControllerRegistryError::ErasureInFlight => {
            ExtensionActionRejection::ShuttingDown
        }
        _ => ExtensionActionRejection::NativeAdmissionFailed,
    }
}

fn map_runtime_error(error: ExtensionRuntimeHostBindError) -> ExtensionActionRejection {
    match error {
        ExtensionRuntimeHostBindError::Sealed => ExtensionActionRejection::ShuttingDown,
        ExtensionRuntimeHostBindError::OwnerConflict => ExtensionActionRejection::RuntimeSuperseded,
        ExtensionRuntimeHostBindError::Unavailable => ExtensionActionRejection::RuntimeUnavailable,
        ExtensionRuntimeHostBindError::UnsupportedBackend => {
            ExtensionActionRejection::UnsupportedPlatform
        }
        ExtensionRuntimeHostBindError::CapacityExceeded => {
            ExtensionActionRejection::CapacityExceeded
        }
        ExtensionRuntimeHostBindError::IdentityExhausted
        | ExtensionRuntimeHostBindError::RetainedBytesOverflow
        | ExtensionRuntimeHostBindError::RetainedBytesExceeded
        | ExtensionRuntimeHostBindError::InternalInvariant => {
            ExtensionActionRejection::NativeAdmissionFailed
        }
        _ => ExtensionActionRejection::NativeAdmissionFailed,
    }
}

fn map_native_error(error: MacosNativeActionFailure) -> ExtensionActionRejection {
    match error {
        MacosNativeActionFailure::ActionUnavailable => ExtensionActionRejection::ActionUnavailable,
        MacosNativeActionFailure::RevisionExhausted => ExtensionActionRejection::RuntimeUnavailable,
        MacosNativeActionFailure::OwnerInvalid
        | MacosNativeActionFailure::ContextMismatch
        | MacosNativeActionFailure::InvalidProjection => {
            ExtensionActionRejection::NativeAdmissionFailed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_and_terminal_failures_remain_distinguishable() {
        assert_eq!(
            map_runtime_error(ExtensionRuntimeHostBindError::Unavailable),
            ExtensionActionRejection::RuntimeUnavailable
        );
        assert_eq!(
            map_runtime_error(ExtensionRuntimeHostBindError::Sealed),
            ExtensionActionRejection::ShuttingDown
        );
        assert_eq!(
            map_controller_error(ControllerRegistryError::BrowserSurfaceStale),
            ExtensionActionRejection::RuntimeSuperseded
        );
    }
}
