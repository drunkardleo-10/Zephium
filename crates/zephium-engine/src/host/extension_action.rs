//! Product action projection at the authenticated runtime/controller/tab join.

use zephium_core::extensions::{
    ExtensionActionRejection, ExtensionActionRequest, ExtensionActionSettlement,
    ExtensionActionSnapshot, ExtensionActionSnapshotSettlement, ExtensionBrowserSurfaceGeneration,
    ExtensionOptionsPageSettlement, ExtensionRuntimeInstance,
};
use zephium_core::ids::{ItemId, ProfileId};
use zephium_extension_runtime_api::ExtensionRuntimeHostBindError;

use crate::platform::imp::{ControllerRegistryError, MacosNativeActionFailure};

use objc2::rc::Retained;
use objc2_app_kit::NSView;
use raw_window_handle::RawWindowHandle;

use super::resources::{NativeResourceAdmissionError, NativeResourceClass};
use super::{extensions::ToolbarActiveTabGrant, EngineHost};

pub(crate) enum ExtensionActionInvocationOutcome {
    Settled(ExtensionActionSettlement),
    Pending,
}

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

    /// Invokes one exact, Shell-versioned action. The runtime, controller,
    /// logical tab and current action revision are rejoined on the host thread.
    /// Popup actions additionally reserve native callback and resource
    /// authority before entering WebKit. `activeTab` is minted only when
    /// declared and is never an action-dispatch prerequisite.
    pub(crate) fn invoke_extension_action(
        &mut self,
        request: ExtensionActionRequest,
    ) -> ExtensionActionInvocationOutcome {
        let profile = request.runtime().profile();
        let (native_tab, resident) = match self.macos_extension_controllers.action_tab(
            profile,
            request.surface_generation(),
            request.tab(),
        ) {
            Ok(Some(tab)) => tab.into_parts(),
            Ok(None) => {
                return settled(ExtensionActionRejection::TabUnavailable);
            }
            Err(error) => {
                return settled(map_controller_error(error));
            }
        };
        if !resident {
            return settled(ExtensionActionRejection::TabDiscarded);
        }
        let runtimes = match self.extension_runtime_registry.published_runtimes(profile) {
            Ok(runtimes) => runtimes,
            Err(error) => {
                return settled(map_runtime_error(error));
            }
        };
        let Some(runtime) = runtimes
            .into_iter()
            .find(|runtime| runtime.instance() == request.runtime())
        else {
            return settled(ExtensionActionRejection::RuntimeUnavailable);
        };
        let state = match self
            .extension_runtime_registry
            .with_owned_macos_runtime(&runtime, |owner| {
                owner.action_state_for_tab(runtime.instance(), request.tab(), &native_tab)
            }) {
            Ok(Some(Ok(state))) => state,
            Ok(Some(Err(error))) => {
                return settled(map_native_error(error));
            }
            Ok(None) => {
                return settled(ExtensionActionRejection::RuntimeUnavailable);
            }
            Err(error) => {
                return settled(map_runtime_error(error));
            }
        };
        if state.revision() != request.action_revision() {
            return settled(ExtensionActionRejection::RuntimeSuperseded);
        }
        if !state.is_enabled() {
            return settled(ExtensionActionRejection::ActionDisabled);
        }

        let popup_reserved = if state.presents_popup() {
            let popup_owner = match self
                .extension_runtime_registry
                .with_owned_macos_runtime(&runtime, |owner| owner.action_popup_owner())
            {
                Ok(Some(Ok(context))) => context,
                Ok(Some(Err(error))) => return settled(map_native_error(error)),
                Ok(None) => return settled(ExtensionActionRejection::RuntimeUnavailable),
                Err(error) => return settled(map_runtime_error(error)),
            };
            let Some(parent) = popup_parent_view(&self.parent) else {
                return settled(ExtensionActionRejection::NativeAdmissionFailed);
            };
            let lease = match self
                .native_resources
                .try_acquire(NativeResourceClass::ExtensionPopup)
            {
                Ok(lease) => lease,
                Err(error) => return settled(map_popup_resource_error(error)),
            };
            match self.macos_extension_controllers.begin_action_popup(
                profile,
                request,
                popup_owner,
                native_tab.clone(),
                parent,
                lease,
            ) {
                Ok(Ok(())) => true,
                Ok(Err(reason)) => return settled(reason),
                Err(error) => return settled(map_controller_error(error)),
            }
        } else {
            false
        };

        match self
            .extension_runtime_registry
            .optional_toolbar_active_tab_witness(&runtime)
        {
            Ok(Some(witness)) => match self.grant_toolbar_active_tab(request.tab(), witness) {
                ToolbarActiveTabGrant::Granted | ToolbarActiveTabGrant::NotApplicable => {}
                ToolbarActiveTabGrant::CapacityExceeded => {
                    let reason = ExtensionActionRejection::CapacityExceeded;
                    if popup_reserved {
                        self.macos_extension_controllers.cancel_action_popup(
                            profile,
                            request.id(),
                            reason,
                        );
                        return ExtensionActionInvocationOutcome::Pending;
                    }
                    return settled(reason);
                }
                ToolbarActiveTabGrant::Invalid => {
                    let reason = ExtensionActionRejection::NativeAdmissionFailed;
                    if popup_reserved {
                        self.macos_extension_controllers.cancel_action_popup(
                            profile,
                            request.id(),
                            reason,
                        );
                        return ExtensionActionInvocationOutcome::Pending;
                    }
                    return settled(reason);
                }
            },
            Ok(None) => {}
            Err(error) => {
                let reason = map_runtime_error(error);
                if popup_reserved {
                    self.macos_extension_controllers.cancel_action_popup(
                        profile,
                        request.id(),
                        reason,
                    );
                    return ExtensionActionInvocationOutcome::Pending;
                }
                return settled(reason);
            }
        }

        if popup_reserved {
            let result =
                self.extension_runtime_registry
                    .with_owned_macos_runtime(&runtime, |owner| {
                        owner.perform_popup_action_for_tab(
                            runtime.instance(),
                            request.tab(),
                            &native_tab,
                            request.action_revision(),
                        )
                    });
            if !matches!(result, Ok(Some(Ok(())))) {
                let reason = match result {
                    Ok(Some(Err(error))) => map_native_error(error),
                    Ok(None) => ExtensionActionRejection::RuntimeUnavailable,
                    Err(error) => map_runtime_error(error),
                    Ok(Some(Ok(()))) => unreachable!(),
                };
                self.macos_extension_controllers
                    .cancel_action_popup(profile, request.id(), reason);
            }
            return ExtensionActionInvocationOutcome::Pending;
        }

        let settlement =
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
                Ok(Some(Err(error))) => {
                    ExtensionActionSettlement::Rejected(map_native_error(error))
                }
                Ok(None) => ExtensionActionSettlement::Rejected(
                    ExtensionActionRejection::RuntimeUnavailable,
                ),
                Err(error) => ExtensionActionSettlement::Rejected(map_runtime_error(error)),
            };
        ExtensionActionInvocationOutcome::Settled(settlement)
    }

    /// Opens an installed extension's exact declared options page from trusted
    /// browser chrome. Runtime, controller, context, parent view, and resource
    /// lease are rejoined on the host thread before any native window exists.
    pub(crate) fn open_extension_options(
        &mut self,
        runtime: ExtensionRuntimeInstance,
    ) -> ExtensionOptionsPageSettlement {
        let profile = runtime.profile();
        let runtimes = match self.extension_runtime_registry.published_runtimes(profile) {
            Ok(runtimes) => runtimes,
            Err(error) => {
                return ExtensionOptionsPageSettlement::Rejected(map_runtime_error(error))
            }
        };
        let Some(runtime) = runtimes
            .into_iter()
            .find(|candidate| candidate.instance() == runtime)
        else {
            return ExtensionOptionsPageSettlement::Rejected(
                ExtensionActionRejection::RuntimeUnavailable,
            );
        };
        let owner = match self
            .extension_runtime_registry
            .with_owned_macos_runtime(&runtime, |owner| owner.action_popup_owner())
        {
            Ok(Some(Ok(owner))) => owner,
            Ok(Some(Err(error))) => {
                return ExtensionOptionsPageSettlement::Rejected(map_native_error(error))
            }
            Ok(None) => {
                return ExtensionOptionsPageSettlement::Rejected(
                    ExtensionActionRejection::RuntimeUnavailable,
                )
            }
            Err(error) => {
                return ExtensionOptionsPageSettlement::Rejected(map_runtime_error(error))
            }
        };
        let Some(parent) = popup_parent_view(&self.parent) else {
            return ExtensionOptionsPageSettlement::Rejected(
                ExtensionActionRejection::NativeAdmissionFailed,
            );
        };
        let lease = match self
            .native_resources
            .try_acquire(NativeResourceClass::ExtensionPopup)
        {
            Ok(lease) => lease,
            Err(error) => {
                return ExtensionOptionsPageSettlement::Rejected(map_popup_resource_error(error))
            }
        };
        match self
            .macos_extension_controllers
            .open_options_page(profile, owner, parent, lease)
        {
            Ok(Ok(())) => ExtensionOptionsPageSettlement::Opened,
            Ok(Err(reason)) => ExtensionOptionsPageSettlement::Rejected(reason),
            Err(error) => ExtensionOptionsPageSettlement::Rejected(map_controller_error(error)),
        }
    }

    pub(crate) fn timeout_extension_action_popup(
        &mut self,
        profile: ProfileId,
        request: zephium_core::extensions::ExtensionActionRequestId,
    ) {
        self.macos_extension_controllers
            .timeout_action_popup(profile, request);
    }
}

fn settled(reason: ExtensionActionRejection) -> ExtensionActionInvocationOutcome {
    ExtensionActionInvocationOutcome::Settled(ExtensionActionSettlement::Rejected(reason))
}

fn popup_parent_view(parent: &super::ParentHandle) -> Option<Retained<NSView>> {
    let RawWindowHandle::AppKit(handle) = parent.0 else {
        return None;
    };
    // SAFETY: the application window outlives the engine host and all popup
    // requests; retaining its content view gives AppKit a stable anchor.
    unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
}

fn map_popup_resource_error(error: NativeResourceAdmissionError) -> ExtensionActionRejection {
    match error {
        NativeResourceAdmissionError::ClassExhausted(NativeResourceClass::ExtensionPopup)
        | NativeResourceAdmissionError::GlobalExhausted => {
            ExtensionActionRejection::PopupCapacityExceeded
        }
        NativeResourceAdmissionError::ClassExhausted(_)
        | NativeResourceAdmissionError::AccountingInvariant => {
            ExtensionActionRejection::NativeAdmissionFailed
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
