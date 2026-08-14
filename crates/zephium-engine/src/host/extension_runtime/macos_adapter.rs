//! Product-shaped macOS native-extension activation and teardown.
//!
//! This module owns platform classification and native sequencing for the
//! macOS lifecycle factory; the parent module remains the platform-neutral
//! registry, ticket, and authority layer.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use zephium_core::extensions::ExtensionNativeNamespaceScope;
use zephium_extension_runtime_api::{
    ExtensionRuntimeAbsenceEvidence, ExtensionRuntimeActivationDisposition,
    ExtensionRuntimeFailure, ExtensionRuntimeHostBindError, ExtensionRuntimeMacosAbsenceAudit,
    ExtensionRuntimeNativeIdentityExpectation, ExtensionRuntimeNativeOwnerId,
    ExtensionRuntimeOwnershipDisposition, ExtensionRuntimeOwnershipEvidence,
    ExtensionRuntimeRetirementDisposition,
};

use crate::platform::imp::{
    begin_prepared_native_runtime_activation, prepare_native_runtime_activation,
    ControllerNamespaceRecoveryAudit, ControllerPreparation, ControllerRegistryError,
    MacosNativeRuntimeActivation, MacosNativeRuntimeFailure, MacosNativeRuntimeReconciliation,
    MacosNativeRuntimeRetirement,
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
            report_product_probe_failure("controller preparation", error);
            return complete_pre_entry_failure(host, ticket, map_controller_failure(error));
        }
    }
    if let Err(error) = host.reconcile_extension_browser_surface(profile) {
        report_product_probe_failure("browser-surface reconciliation", error);
        return complete_pre_entry_failure(host, ticket, map_controller_failure(error));
    }
    let controller = match host
        .macos_extension_controllers
        .controller_for_native_runtime(profile, ExtensionNativeNamespaceScope::MacosControllerV1)
    {
        Ok(controller) => controller,
        Err(error) => {
            report_product_probe_failure("controller acquisition", error);
            return complete_pre_entry_failure(host, ticket, map_controller_failure(error));
        }
    };

    let prepared = {
        let bootstrap_grants = match &reservation.binding {
            ReservationBinding::Activation {
                bootstrap_grants, ..
            } => match bootstrap_grants.lock() {
                Ok(grants) => grants,
                Err(_) => {
                    reservation
                        .gate
                        .inner
                        .invariant_failed
                        .store(true, Ordering::Release);
                    host.extension_runtime_registry.fail_invariant();
                    return Err(ExtensionRuntimeHostBindError::InternalInvariant);
                }
            },
            ReservationBinding::Recovery { .. } => {
                host.extension_runtime_registry.fail_invariant();
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        let Some(grants) = bootstrap_grants.as_deref() else {
            host.extension_runtime_registry.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
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
        match prepare_native_runtime_activation(
            native_root,
            grants.native_snapshot(),
            expected_owner,
            controller,
        ) {
            Ok(prepared) => prepared,
            Err(failure) => {
                report_product_probe_failure("native package preparation", failure);
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
    if let Err(failure) = start {
        report_product_probe_failure("native activation entry", failure);
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
    host.macos_extension_controllers
        .cancel_action_popup_context(
            ticket.owner().key.profile(),
            owner.action_popup_context_identity(),
            zephium_core::extensions::ExtensionActionRejection::RuntimeUnavailable,
        );
    host.macos_extension_controllers
        .cancel_runtime_grant_context(
            ticket.owner().key.profile(),
            owner.runtime_grant_context_identity(),
        );
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

pub(super) fn begin_native_reconciliation(
    host: &mut EngineHost,
    reservation: Arc<ReservationControl>,
    ticket: NativeCallTicket,
) -> Result<(), ExtensionRuntimeHostBindError> {
    if matches!(&reservation.binding, ReservationBinding::Activation { .. }) {
        if let Some((observed_owner, outcome)) = host
            .extension_runtime_registry
            .with_macos_reconciliation_owner(ticket, |owner| {
                (owner.owner_id(), owner.reconcile())
            })?
        {
            let disposition = match outcome {
                MacosNativeRuntimeReconciliation::Owned => {
                    ExtensionRuntimeOwnershipDisposition::Owned(
                        ExtensionRuntimeOwnershipEvidence::MacosWebExtension(observed_owner),
                    )
                }
                MacosNativeRuntimeReconciliation::Absent(audit) => {
                    ExtensionRuntimeOwnershipDisposition::Absent(
                        host.extension_runtime_registry.mint_macos_absence(
                            ticket,
                            observed_owner,
                            audit,
                        )?,
                    )
                }
                MacosNativeRuntimeReconciliation::StillUncertain(failure) => {
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: map_native_failure(failure),
                        evidence: Some(ExtensionRuntimeOwnershipEvidence::MacosWebExtension(
                            observed_owner,
                        )),
                    }
                }
            };
            return complete_reconciliation(host, ticket, disposition);
        }
    }

    let known_evidence = match &reservation.binding {
        // A catalog-authenticated expectation constrains which native owner
        // may be accepted, but it is not evidence that the adapter observed
        // that owner. Only recovery rows can carry previously observed native
        // ownership into this controller-wide audit.
        ReservationBinding::Activation { .. } => None,
        ReservationBinding::Recovery { expectation, .. } => (*expectation).known_evidence(),
    };
    let audit = host
        .macos_extension_controllers
        .audit_native_runtime_recovery(
            reservation.owner().key.profile(),
            ExtensionNativeNamespaceScope::MacosControllerV1,
        );
    let disposition = match audit {
        Ok(ControllerNamespaceRecoveryAudit::Absent(audit)) => {
            ExtensionRuntimeOwnershipDisposition::Absent(
                host.extension_runtime_registry
                    .mint_macos_controller_absence(ticket, audit)?,
            )
        }
        Ok(ControllerNamespaceRecoveryAudit::OwnersPresent) => {
            ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::BackendUnavailable,
                evidence: known_evidence,
            }
        }
        Ok(ControllerNamespaceRecoveryAudit::RuntimeUnavailable) => {
            ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::UnsupportedTarget,
                evidence: known_evidence,
            }
        }
        Err(failure) => ExtensionRuntimeOwnershipDisposition::StillUncertain {
            failure: map_controller_failure(failure),
            evidence: known_evidence,
        },
    };
    complete_reconciliation(host, ticket, disposition)
}

fn complete_reconciliation(
    host: &mut EngineHost,
    ticket: NativeCallTicket,
    disposition: ExtensionRuntimeOwnershipDisposition,
) -> Result<(), ExtensionRuntimeHostBindError> {
    if !host
        .extension_runtime_registry
        .complete_native_reconciliation(ticket, disposition, PlatformOwnerBundle::Vacant)?
    {
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

    fn mint_macos_controller_absence(
        &mut self,
        ticket: NativeCallTicket,
        audit: zephium_extension_runtime_api::ExtensionRuntimeMacosControllerAbsenceAudit,
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
            .mint_macos_controller_namespace_absent(ticket.attempt(), audit)
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
        MacosNativeRuntimeActivation::RejectedWithoutAbsenceProof(failure) => {
            report_product_probe_failure("native activation settlement", failure);
            (
                ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                    failure: map_native_failure(failure),
                    evidence: None,
                },
                PlatformOwnerBundle::Vacant,
            )
        }
        MacosNativeRuntimeActivation::RejectedAfterCleanup {
            failure,
            owner_id,
            audit,
        } => {
            report_product_probe_failure("native activation settlement after cleanup", failure);
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
            report_product_probe_failure("native activation ownership settlement", failure);
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

#[cfg(feature = "native-extension-product-probes")]
fn report_product_probe_failure(phase: &'static str, failure: impl std::fmt::Display) {
    // The native/controller failure enums are closed, path-free classes. Keep
    // raw NSError strings and package paths out of this probe diagnostic.
    eprintln!("extension-product-probe-native-phase: {phase}: {failure}");
}

#[cfg(not(feature = "native-extension-product-probes"))]
fn report_product_probe_failure(_phase: &'static str, _failure: impl std::fmt::Display) {}

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
