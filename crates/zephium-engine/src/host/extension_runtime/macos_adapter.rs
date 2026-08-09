//! Product-shaped macOS native-extension activation and teardown.
//!
//! This module is deliberately selected only by the dormant activation-only
//! factory. It owns platform classification and native sequencing; the parent
//! module remains the platform-neutral registry, ticket, and authority layer.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use zephium_core::extensions::ExtensionNativeNamespaceScope;
use zephium_extension_runtime_api::{
    ExtensionRuntimeAbsenceEvidence, ExtensionRuntimeActivationDisposition,
    ExtensionRuntimeFailure, ExtensionRuntimeHostBindError, ExtensionRuntimeMacosAbsenceAudit,
    ExtensionRuntimeNativeIdentityExpectation, ExtensionRuntimeNativeOwnerId,
    ExtensionRuntimeOwnershipEvidence, ExtensionRuntimeRetirementDisposition,
};

use crate::platform::imp::{
    begin_prepared_native_runtime_activation, prepare_native_runtime_activation,
    ControllerPreparation, ControllerRegistryError, MacosNativeRuntimeActivation,
    MacosNativeRuntimeFailure, MacosNativeRuntimeRetirement,
};

use super::super::EngineHost;
use super::{
    fail_extension_native_terminal, ExtensionRuntimeRegistry, NativeCallNotification,
    NativeCallTicket, PlatformOwnerBundle, ReservationBinding, ReservationControl,
};

pub(super) fn begin_native_activation(
    host: &mut EngineHost,
    reservation: Arc<ReservationControl>,
    ticket: NativeCallTicket,
) -> Result<(), ExtensionRuntimeHostBindError> {
    let expected_owner = match &reservation.binding {
        ReservationBinding::Activation {
            expectation: ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(owner),
            ..
        } => *owner,
        _ => {
            host.extension_runtime_registry.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
    };
    let profile = reservation.owner().key.profile();
    match host
        .macos_extension_controllers
        .prepare_for_native_runtime(profile, ExtensionNativeNamespaceScope::MacosControllerV1)
    {
        Ok(ControllerPreparation::Prepared) => {}
        Ok(ControllerPreparation::RuntimeUnavailable) => {
            return complete_pre_entry_failure(
                host,
                ticket,
                ExtensionRuntimeFailure::UnsupportedTarget,
            );
        }
        Err(error) => {
            return complete_pre_entry_failure(host, ticket, map_controller_failure(error));
        }
    }
    let controller = match host
        .macos_extension_controllers
        .controller_for_native_runtime(profile, ExtensionNativeNamespaceScope::MacosControllerV1)
    {
        Ok(controller) => controller,
        Err(error) => {
            return complete_pre_entry_failure(host, ticket, map_controller_failure(error));
        }
    };

    let prepared = {
        let grants = match &reservation.binding {
            ReservationBinding::Activation { grants, .. } => grants.native_snapshot(),
            ReservationBinding::Recovery { .. } => {
                host.extension_runtime_registry.fail_invariant();
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        let mut root_slot = match reservation.staged_native_root.lock() {
            Ok(root) => root,
            Err(_) => {
                reservation
                    .gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                host.extension_runtime_registry.fail_invariant();
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        let Some(native_root) = root_slot.as_mut() else {
            host.extension_runtime_registry.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        match prepare_native_runtime_activation(native_root, grants, expected_owner, controller) {
            Ok(prepared) => prepared,
            Err(failure) => {
                drop(root_slot);
                return complete_pre_entry_failure(host, ticket, map_native_failure(failure));
            }
        }
    };

    host.extension_runtime_registry
        .mark_activation_native_entered(ticket)?;
    let start = begin_prepared_native_runtime_activation(prepared, move |outcome| {
        let accepted = super::super::dispatch::with_extension_runtime_terminal(move |host| {
            if settle_native_activation(host, ticket, outcome).is_err() {
                fail_extension_native_terminal(
                    host,
                    "macOS extension activation terminal violated registry state",
                );
            }
        });
        let _terminal_transport_accepted = accepted;
    });
    if start.is_err() {
        // The native entry raised after WebKit may have copied the callback.
        // Leave the exact ticket pending: the waiter times out conservatively,
        // while a late callback can still settle the one authoritative owner.
    }
    Ok(())
}

pub(super) fn begin_native_retirement(
    host: &mut EngineHost,
    ticket: NativeCallTicket,
    platform_owner: PlatformOwnerBundle,
) -> Result<(), ExtensionRuntimeHostBindError> {
    let PlatformOwnerBundle::Macos(owner) = platform_owner else {
        platform_owner.quarantine_unattributed();
        host.extension_runtime_registry.fail_invariant();
        return Err(ExtensionRuntimeHostBindError::InternalInvariant);
    };
    let observed_owner = owner.owner_id();
    let (disposition, returned_owner) = match owner.retire() {
        MacosNativeRuntimeRetirement::Absent(audit) => {
            let absence = host.extension_runtime_registry.mint_macos_absence(
                ticket,
                observed_owner,
                audit,
            )?;
            (
                ExtensionRuntimeRetirementDisposition::Retired(absence),
                PlatformOwnerBundle::Vacant,
            )
        }
        MacosNativeRuntimeRetirement::Retained { failure, owner } => (
            ExtensionRuntimeRetirementDisposition::Retained(map_native_failure(failure)),
            PlatformOwnerBundle::Macos(owner),
        ),
    };
    if !host.extension_runtime_registry.complete_native_retirement(
        ticket,
        disposition,
        returned_owner,
    )? {
        host.extension_runtime_registry.fail_invariant();
        return Err(ExtensionRuntimeHostBindError::InternalInvariant);
    }
    Ok(())
}

impl ExtensionRuntimeRegistry {
    fn mint_macos_absence(
        &mut self,
        ticket: NativeCallTicket,
        observed_owner: ExtensionRuntimeNativeOwnerId,
        audit: ExtensionRuntimeMacosAbsenceAudit,
    ) -> Result<ExtensionRuntimeAbsenceEvidence, ExtensionRuntimeHostBindError> {
        let index = self.entry_index(ticket.owner(), ticket.registry_generation())?;
        if !matches!(
            self.entries[index]
                .reservation
                .native_call
                .notification_for(ticket),
            Ok(NativeCallNotification::Pending)
        ) || self.entries[index].native.current_ticket() != Some(ticket)
        {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let Some(absence) = self.entries[index]
            .reservation
            .absence_issuer()
            .mint_macos_zero_grants_and_unloaded(ticket.attempt(), observed_owner, audit)
        else {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        Ok(absence)
    }
}

fn complete_pre_entry_failure(
    host: &mut EngineHost,
    ticket: NativeCallTicket,
    failure: ExtensionRuntimeFailure,
) -> Result<(), ExtensionRuntimeHostBindError> {
    let absence = host
        .extension_runtime_registry
        .mint_activation_never_entered(ticket)?;
    let disposition = if matches!(
        failure,
        ExtensionRuntimeFailure::BackendUnavailable | ExtensionRuntimeFailure::CapacityExceeded
    ) {
        ExtensionRuntimeActivationDisposition::Retryable { failure, absence }
    } else {
        ExtensionRuntimeActivationDisposition::Rejected { failure, absence }
    };
    if !host.extension_runtime_registry.complete_native_activation(
        ticket,
        disposition,
        PlatformOwnerBundle::Vacant,
    )? {
        host.extension_runtime_registry.fail_invariant();
        return Err(ExtensionRuntimeHostBindError::InternalInvariant);
    }
    Ok(())
}

fn settle_native_activation(
    host: &mut EngineHost,
    ticket: NativeCallTicket,
    outcome: MacosNativeRuntimeActivation,
) -> Result<bool, ExtensionRuntimeHostBindError> {
    let (disposition, owner) = match outcome {
        MacosNativeRuntimeActivation::RejectedWithoutAbsenceProof(failure) => (
            ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure: map_native_failure(failure),
                evidence: None,
            },
            PlatformOwnerBundle::Vacant,
        ),
        MacosNativeRuntimeActivation::RejectedAfterCleanup {
            failure,
            owner_id,
            audit,
        } => {
            let failure = map_native_failure(failure);
            let absence = host
                .extension_runtime_registry
                .mint_macos_absence(ticket, owner_id, audit)?;
            let disposition = if matches!(
                failure,
                ExtensionRuntimeFailure::BackendUnavailable
                    | ExtensionRuntimeFailure::CapacityExceeded
            ) {
                ExtensionRuntimeActivationDisposition::Retryable { failure, absence }
            } else {
                ExtensionRuntimeActivationDisposition::Rejected { failure, absence }
            };
            (disposition, PlatformOwnerBundle::Vacant)
        }
        MacosNativeRuntimeActivation::Activated(owner) => {
            let evidence = ExtensionRuntimeOwnershipEvidence::MacosWebExtension(owner.owner_id());
            (
                ExtensionRuntimeActivationDisposition::Activated(evidence),
                PlatformOwnerBundle::Macos(owner),
            )
        }
        MacosNativeRuntimeActivation::OwnershipUncertain { failure, owner } => {
            let evidence = ExtensionRuntimeOwnershipEvidence::MacosWebExtension(owner.owner_id());
            (
                ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                    failure: map_native_failure(failure),
                    evidence: Some(evidence),
                },
                PlatformOwnerBundle::Macos(owner),
            )
        }
    };
    host.extension_runtime_registry
        .complete_native_activation(ticket, disposition, owner)
}

fn map_controller_failure(error: ControllerRegistryError) -> ExtensionRuntimeFailure {
    match error {
        ControllerRegistryError::CapacityExceeded => ExtensionRuntimeFailure::CapacityExceeded,
        ControllerRegistryError::InvalidRuntimeVersion
        | ControllerRegistryError::IncompleteRuntime
        | ControllerRegistryError::NamespaceReopenUnavailable
        | ControllerRegistryError::UnsupportedNamespaceScope => {
            ExtensionRuntimeFailure::UnsupportedTarget
        }
        ControllerRegistryError::Sealed | ControllerRegistryError::ErasureInFlight => {
            ExtensionRuntimeFailure::BackendUnavailable
        }
        _ => ExtensionRuntimeFailure::Internal,
    }
}

fn map_native_failure(failure: MacosNativeRuntimeFailure) -> ExtensionRuntimeFailure {
    match failure {
        MacosNativeRuntimeFailure::UnsupportedRuntime => ExtensionRuntimeFailure::UnsupportedTarget,
        MacosNativeRuntimeFailure::PackageRootAccess(_)
        | MacosNativeRuntimeFailure::PackageRootRejected(_)
        | MacosNativeRuntimeFailure::GrantPlan(_)
        | MacosNativeRuntimeFailure::ExtensionParseFailed
        | MacosNativeRuntimeFailure::ExtensionManifestInvalid
        | MacosNativeRuntimeFailure::GrantApplication(_) => {
            ExtensionRuntimeFailure::PackageRejected
        }
        MacosNativeRuntimeFailure::ControllerLoadFailed
        | MacosNativeRuntimeFailure::ControllerUnloadFailed => {
            ExtensionRuntimeFailure::BackendUnavailable
        }
        _ => ExtensionRuntimeFailure::Internal,
    }
}
