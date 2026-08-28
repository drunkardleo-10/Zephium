//! Product-shaped Windows WebView2 native-extension lifecycle adapter.
//!
//! The adapter creates no page-world bridge. Its first native operation
//! establishes an extension-enabled environment through Wry's authenticated
//! pre-initialization gate, then retains the exact environment/profile and
//! package root beside every WebView2 owner.

use std::sync::Arc;

use zephium_extension_runtime_api::{
    ExtensionRuntimeActivationDisposition, ExtensionRuntimeFailure, ExtensionRuntimeHostBindError,
    ExtensionRuntimeNativeIdentityExpectation, ExtensionRuntimeOwnershipDisposition,
    ExtensionRuntimeOwnershipEvidence, ExtensionRuntimeRetirementDisposition,
};

use crate::platform::imp::{
    begin_native_extension_activation, prepare_native_extension_activation,
    WindowsNativeExtensionActivation, WindowsNativeExtensionFailure, WindowsNativeExtensionOwner,
    WindowsNativeExtensionOwnerReconciliation, WindowsNativeExtensionProfileReconciliation,
    WindowsNativeExtensionRetirement,
};

use super::super::EngineHost;
use super::{
    fail_extension_native_terminal, ExtensionRuntimeRegistry, NativeCallNotification,
    NativeCallTicket, PlatformOwnerBundle, ReservationBinding, ReservationControl,
};

impl ExtensionRuntimeRegistry {
    fn mint_windows_absence(
        &mut self,
        ticket: NativeCallTicket,
        audit: zephium_extension_runtime_api::ExtensionRuntimeWindowsAbsenceAudit,
    ) -> Result<
        zephium_extension_runtime_api::ExtensionRuntimeAbsenceEvidence,
        ExtensionRuntimeHostBindError,
    > {
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
            .mint_windows_profile_owner_absent(ticket.attempt(), audit)
        else {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        Ok(absence)
    }
}

pub(super) fn begin_native_activation(
    host: &mut EngineHost,
    reservation: Arc<ReservationControl>,
    ticket: NativeCallTicket,
) -> Result<(), ExtensionRuntimeHostBindError> {
    let expected_owner = match &reservation.binding {
        ReservationBinding::Activation {
            expectation: ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(owner),
            ..
        } => *owner,
        _ => {
            host.extension_runtime_registry.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
    };
    let profile = reservation.owner().key.profile();
    let deadline = host
        .extension_runtime_registry
        .native_deadline(ticket)
        .ok_or(ExtensionRuntimeHostBindError::InternalInvariant)?;

    if let Err(failure) = host.preflight_windows_extension_profile(profile, deadline) {
        return complete_pre_entry_failure(host, ticket, map_native_failure(failure));
    }
    host.extension_runtime_registry
        .mark_activation_native_entered(ticket)?;
    if let Err(failure) = host.ensure_windows_extension_profile(profile, deadline) {
        return settle_entered_failure(host, ticket, profile, expected_owner, failure);
    }
    let native_profile = match host.windows_extension_profiles.get(&profile).cloned() {
        Some(profile) => profile,
        None => {
            host.extension_runtime_registry.fail_invariant();
            return settle_entered_failure(
                host,
                ticket,
                profile,
                expected_owner,
                WindowsNativeExtensionFailure::AdapterInvariant,
            );
        }
    };
    let native_root = match reservation.staged_native_root.lock() {
        Ok(mut root) => match root.take() {
            Some(root) => root,
            None => {
                host.extension_runtime_registry.fail_invariant();
                return settle_entered_failure(
                    host,
                    ticket,
                    profile,
                    expected_owner,
                    WindowsNativeExtensionFailure::AdapterInvariant,
                );
            }
        },
        Err(_) => {
            reservation
                .gate
                .inner
                .invariant_failed
                .store(true, std::sync::atomic::Ordering::Release);
            host.extension_runtime_registry.fail_invariant();
            return settle_entered_failure(
                host,
                ticket,
                profile,
                expected_owner,
                WindowsNativeExtensionFailure::AdapterInvariant,
            );
        }
    };
    let prepared = match prepare_native_extension_activation(
        &native_profile,
        profile,
        expected_owner,
        native_root,
    ) {
        Ok(prepared) => prepared,
        Err(refusal) => {
            let failure = refusal.failure();
            drop(refusal.into_native_root());
            return settle_entered_failure(host, ticket, profile, expected_owner, failure);
        }
    };

    let (disposition, owner) = match begin_native_extension_activation(prepared, deadline) {
        WindowsNativeExtensionActivation::RejectedBeforeNative {
            failure,
            native_root,
        } => {
            drop(native_root);
            return settle_entered_failure(host, ticket, profile, expected_owner, failure);
        }
        WindowsNativeExtensionActivation::Activated(owner) => (
            ExtensionRuntimeActivationDisposition::Activated(
                ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(owner.owner_id()),
            ),
            PlatformOwnerBundle::Windows(owner),
        ),
        WindowsNativeExtensionActivation::OwnershipUncertain { failure, debt } => (
            ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure: map_native_failure(failure),
                evidence: None,
            },
            PlatformOwnerBundle::Windows(WindowsNativeExtensionOwner::from_uncertain(debt)),
        ),
    };
    complete_activation(host, ticket, disposition, owner)
}

pub(super) fn begin_native_retirement(
    host: &mut EngineHost,
    ticket: NativeCallTicket,
    platform_owner: PlatformOwnerBundle,
) -> Result<(), ExtensionRuntimeHostBindError> {
    let PlatformOwnerBundle::Windows(owner) = platform_owner else {
        platform_owner.quarantine_unattributed();
        host.extension_runtime_registry.fail_invariant();
        return Err(ExtensionRuntimeHostBindError::InternalInvariant);
    };
    let observed_owner = owner.owner_id();
    let deadline = host
        .extension_runtime_registry
        .native_deadline(ticket)
        .ok_or(ExtensionRuntimeHostBindError::InternalInvariant)?;
    let (disposition, returned) = match owner.retire(deadline) {
        WindowsNativeExtensionRetirement::Absent(audit) => {
            let absence = host
                .extension_runtime_registry
                .mint_windows_absence(ticket, audit.into_runtime_audit())?;
            (
                ExtensionRuntimeRetirementDisposition::Retired(absence),
                PlatformOwnerBundle::Vacant,
            )
        }
        WindowsNativeExtensionRetirement::Retained { failure, debt } => (
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                failure: map_native_failure(failure),
                evidence: Some(ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(
                    observed_owner,
                )),
            },
            PlatformOwnerBundle::Windows(WindowsNativeExtensionOwner::from_uncertain(debt)),
        ),
    };
    if !host
        .extension_runtime_registry
        .complete_native_retirement(ticket, disposition, returned)?
    {
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
    let deadline = host
        .extension_runtime_registry
        .native_deadline(ticket)
        .ok_or(ExtensionRuntimeHostBindError::InternalInvariant)?;
    if let Some(outcome) = host
        .extension_runtime_registry
        .with_windows_reconciliation_owner(ticket, |owner| owner.reconcile(deadline))?
    {
        let disposition = match outcome {
            WindowsNativeExtensionOwnerReconciliation::Owned => {
                let owner = reservation
                    .binding
                    .expected_windows_owner()
                    .ok_or_else(|| {
                        host.extension_runtime_registry.fail_invariant();
                        ExtensionRuntimeHostBindError::InternalInvariant
                    })?;
                ExtensionRuntimeOwnershipDisposition::Owned(
                    ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(owner),
                )
            }
            WindowsNativeExtensionOwnerReconciliation::Absent(audit) => {
                ExtensionRuntimeOwnershipDisposition::Absent(
                    host.extension_runtime_registry
                        .mint_windows_absence(ticket, audit.into_runtime_audit())?,
                )
            }
            WindowsNativeExtensionOwnerReconciliation::StillUncertain(failure) => {
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: map_native_failure(failure),
                    evidence: reservation
                        .binding
                        .expected_windows_owner()
                        .map(ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension),
                }
            }
        };
        return complete_reconciliation(host, ticket, disposition, PlatformOwnerBundle::Vacant);
    }

    let profile = reservation.owner().key.profile();
    if let Err(failure) = host.preflight_windows_extension_profile(profile, deadline) {
        return complete_reconciliation(
            host,
            ticket,
            ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: map_native_failure(failure),
                evidence: reservation.binding.known_windows_evidence(),
            },
            PlatformOwnerBundle::Vacant,
        );
    }
    if let Err(failure) = host.ensure_windows_extension_profile(profile, deadline) {
        return complete_reconciliation(
            host,
            ticket,
            ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: map_native_failure(failure),
                evidence: reservation.binding.known_windows_evidence(),
            },
            PlatformOwnerBundle::Vacant,
        );
    }
    let Some(native_profile) = host.windows_extension_profiles.get(&profile) else {
        host.extension_runtime_registry.fail_invariant();
        return Err(ExtensionRuntimeHostBindError::InternalInvariant);
    };
    let expected = reservation.binding.expected_windows_owner();
    let (disposition, owner) = match native_profile.reconcile_recovery(expected, deadline) {
        WindowsNativeExtensionProfileReconciliation::Owned(owner) => (
            ExtensionRuntimeOwnershipDisposition::Owned(
                ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(owner.owner_id()),
            ),
            PlatformOwnerBundle::Windows(owner),
        ),
        WindowsNativeExtensionProfileReconciliation::Absent(audit) => (
            ExtensionRuntimeOwnershipDisposition::Absent(
                host.extension_runtime_registry
                    .mint_windows_absence(ticket, audit.into_runtime_audit())?,
            ),
            PlatformOwnerBundle::Vacant,
        ),
        WindowsNativeExtensionProfileReconciliation::StillUncertain(failure) => (
            ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: map_native_failure(failure),
                evidence: reservation.binding.known_windows_evidence(),
            },
            PlatformOwnerBundle::Vacant,
        ),
    };
    complete_reconciliation(host, ticket, disposition, owner)
}

fn settle_entered_failure(
    host: &mut EngineHost,
    ticket: NativeCallTicket,
    profile: zephium_core::ids::ProfileId,
    expected_owner: zephium_extension_runtime_api::ExtensionRuntimeNativeOwnerId,
    failure: WindowsNativeExtensionFailure,
) -> Result<(), ExtensionRuntimeHostBindError> {
    let deadline = host
        .extension_runtime_registry
        .native_deadline(ticket)
        .ok_or(ExtensionRuntimeHostBindError::InternalInvariant)?;
    if let Some(native_profile) = host.windows_extension_profiles.get(&profile) {
        if let Ok(audit) = native_profile.audit_owner_absent(expected_owner, deadline) {
            let absence = host
                .extension_runtime_registry
                .mint_windows_absence(ticket, audit)?;
            return complete_activation(
                host,
                ticket,
                ExtensionRuntimeActivationDisposition::Retryable {
                    failure: map_native_failure(failure),
                    absence,
                },
                PlatformOwnerBundle::Vacant,
            );
        }
    }
    complete_activation(
        host,
        ticket,
        ExtensionRuntimeActivationDisposition::OwnershipUncertain {
            failure: map_native_failure(failure),
            evidence: None,
        },
        PlatformOwnerBundle::Vacant,
    )
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
        ExtensionRuntimeFailure::BackendUnavailable
            | ExtensionRuntimeFailure::CapacityExceeded
            | ExtensionRuntimeFailure::RestartRequired
            | ExtensionRuntimeFailure::TimedOut
    ) {
        ExtensionRuntimeActivationDisposition::Retryable { failure, absence }
    } else {
        ExtensionRuntimeActivationDisposition::Rejected { failure, absence }
    };
    complete_activation(host, ticket, disposition, PlatformOwnerBundle::Vacant)
}

fn complete_activation(
    host: &mut EngineHost,
    ticket: NativeCallTicket,
    disposition: ExtensionRuntimeActivationDisposition,
    owner: PlatformOwnerBundle,
) -> Result<(), ExtensionRuntimeHostBindError> {
    if !host
        .extension_runtime_registry
        .complete_native_activation(ticket, disposition, owner)?
    {
        host.extension_runtime_registry.fail_invariant();
        return Err(ExtensionRuntimeHostBindError::InternalInvariant);
    }
    Ok(())
}

fn complete_reconciliation(
    host: &mut EngineHost,
    ticket: NativeCallTicket,
    disposition: ExtensionRuntimeOwnershipDisposition,
    owner: PlatformOwnerBundle,
) -> Result<(), ExtensionRuntimeHostBindError> {
    if !host
        .extension_runtime_registry
        .complete_native_reconciliation(ticket, disposition, owner)?
    {
        fail_extension_native_terminal(
            host,
            "Windows extension reconciliation terminal violated registry state",
        );
        return Err(ExtensionRuntimeHostBindError::InternalInvariant);
    }
    Ok(())
}

fn map_native_failure(failure: WindowsNativeExtensionFailure) -> ExtensionRuntimeFailure {
    use crate::platform::imp::WindowsNativeExtensionFailure as Failure;
    match failure {
        Failure::PrivateProfileUnsupported => ExtensionRuntimeFailure::UnsupportedTarget,
        Failure::NativeOwnerCapacityExceeded | Failure::InventoryCapacityExceeded => {
            ExtensionRuntimeFailure::CapacityExceeded
        }
        Failure::PackageRootAccess(_) | Failure::PackageRootRejected(_) => {
            ExtensionRuntimeFailure::PackageRejected
        }
        Failure::NativeCallTimedOut(_) => ExtensionRuntimeFailure::TimedOut,
        Failure::ProfileHostUnavailable => ExtensionRuntimeFailure::BackendUnavailable,
        Failure::ExistingEnvironmentModeConflict => ExtensionRuntimeFailure::RestartRequired,
        Failure::IdentityMalformed
        | Failure::IdentityMismatchQuarantined
        | Failure::InstalledOwnerDisabled => ExtensionRuntimeFailure::PackageRejected,
        Failure::EnvironmentAttestation
        | Failure::ProfileInterfaceUnavailable
        | Failure::NativeCall(_)
        | Failure::NativeCallInterruptedByShutdown(_)
        | Failure::NativeMessagePumpFailed(_)
        | Failure::NativeCallbackDisconnected(_)
        | Failure::AdapterFailStopped
        | Failure::ReentrantNativeCall
        | Failure::MissingNativeObject
        | Failure::IdentityReadbackFailed
        | Failure::EnabledReadbackFailed
        | Failure::InventoryIdentityConflict
        | Failure::InventoryOwnerMissing
        | Failure::RemovedOwnerStillPresent
        | Failure::AdapterInvariant
        | Failure::ProfileMismatch
        | Failure::InventoryMismatch
        | Failure::ProfileHostConstructionFailed
        | Failure::ProfileHostCleanupFailed => ExtensionRuntimeFailure::Internal,
    }
}
