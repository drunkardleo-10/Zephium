//! Product action projection at the authenticated runtime/controller/tab join.

use zephium_core::extensions::{
    ExtensionActionRejection, ExtensionActionRequest, ExtensionActionSettlement,
    ExtensionActionSnapshot, ExtensionActionSnapshotSettlement, ExtensionBrowserSurfaceGeneration,
};
use zephium_core::ids::{ItemId, ProfileId};
use zephium_extension_runtime_api::ExtensionRuntimeHostBindError;

use crate::platform::imp::{ControllerRegistryError, MacosNativeActionFailure};

use super::{extensions::ToolbarActiveTabGrant, EngineHost};

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
                Ok(Some(tab)) => tab.into_parts().0,
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

    /// Invokes one exact, Shell-versioned, non-popup action. The runtime,
    /// controller, logical tab and current action revision are rejoined on the
    /// host thread. `activeTab` is minted only when declared and is never an
    /// action-dispatch prerequisite.
    pub(crate) fn invoke_extension_action(
        &mut self,
        request: ExtensionActionRequest,
    ) -> ExtensionActionSettlement {
        let profile = request.runtime().profile();
        let (native_tab, resident) = match self.macos_extension_controllers.action_tab(
            profile,
            request.surface_generation(),
            request.tab(),
        ) {
            Ok(Some(tab)) => tab.into_parts(),
            Ok(None) => {
                return ExtensionActionSettlement::Rejected(
                    ExtensionActionRejection::TabUnavailable,
                );
            }
            Err(error) => {
                return ExtensionActionSettlement::Rejected(map_controller_error(error));
            }
        };
        if !resident {
            return ExtensionActionSettlement::Rejected(ExtensionActionRejection::TabDiscarded);
        }
        let runtimes = match self.extension_runtime_registry.published_runtimes(profile) {
            Ok(runtimes) => runtimes,
            Err(error) => {
                return ExtensionActionSettlement::Rejected(map_runtime_error(error));
            }
        };
        let Some(runtime) = runtimes
            .into_iter()
            .find(|runtime| runtime.instance() == request.runtime())
        else {
            return ExtensionActionSettlement::Rejected(
                ExtensionActionRejection::RuntimeUnavailable,
            );
        };
        let state = match self
            .extension_runtime_registry
            .with_owned_macos_runtime(&runtime, |owner| {
                owner.action_state_for_tab(runtime.instance(), request.tab(), &native_tab)
            }) {
            Ok(Some(Ok(state))) => state,
            Ok(Some(Err(error))) => {
                return ExtensionActionSettlement::Rejected(map_native_error(error));
            }
            Ok(None) => {
                return ExtensionActionSettlement::Rejected(
                    ExtensionActionRejection::RuntimeUnavailable,
                );
            }
            Err(error) => {
                return ExtensionActionSettlement::Rejected(map_runtime_error(error));
            }
        };
        if state.revision() != request.action_revision() {
            return ExtensionActionSettlement::Rejected(
                ExtensionActionRejection::RuntimeSuperseded,
            );
        }
        if !state.is_enabled() {
            return ExtensionActionSettlement::Rejected(ExtensionActionRejection::ActionDisabled);
        }
        if state.presents_popup() {
            return ExtensionActionSettlement::Rejected(ExtensionActionRejection::PopupUnavailable);
        }

        match self
            .extension_runtime_registry
            .optional_toolbar_active_tab_witness(&runtime)
        {
            Ok(Some(witness)) => match self.grant_toolbar_active_tab(request.tab(), witness) {
                ToolbarActiveTabGrant::Granted | ToolbarActiveTabGrant::NotApplicable => {}
                ToolbarActiveTabGrant::CapacityExceeded => {
                    return ExtensionActionSettlement::Rejected(
                        ExtensionActionRejection::CapacityExceeded,
                    );
                }
                ToolbarActiveTabGrant::Invalid => {
                    return ExtensionActionSettlement::Rejected(
                        ExtensionActionRejection::NativeAdmissionFailed,
                    );
                }
            },
            Ok(None) => {}
            Err(error) => {
                return ExtensionActionSettlement::Rejected(map_runtime_error(error));
            }
        }

        match self
            .extension_runtime_registry
            .with_owned_macos_runtime(&runtime, |owner| {
                owner.perform_non_popup_action_for_tab(
                    runtime.instance(),
                    request.tab(),
                    &native_tab,
                    request.action_revision(),
                )
            }) {
            Ok(Some(Ok(()))) => ExtensionActionSettlement::Dispatched,
            Ok(Some(Err(error))) => ExtensionActionSettlement::Rejected(map_native_error(error)),
            Ok(None) => {
                ExtensionActionSettlement::Rejected(ExtensionActionRejection::RuntimeUnavailable)
            }
            Err(error) => ExtensionActionSettlement::Rejected(map_runtime_error(error)),
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
        MacosNativeActionFailure::StaleAction | MacosNativeActionFailure::PopupRequired => {
            ExtensionActionRejection::RuntimeSuperseded
        }
        MacosNativeActionFailure::ActionDisabled => ExtensionActionRejection::ActionDisabled,
        MacosNativeActionFailure::OwnerInvalid
        | MacosNativeActionFailure::ContextMismatch
        | MacosNativeActionFailure::InvalidProjection
        | MacosNativeActionFailure::NativeException => {
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
