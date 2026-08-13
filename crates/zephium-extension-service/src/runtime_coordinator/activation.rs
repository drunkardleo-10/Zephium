//! Fresh-runtime activation state transitions.

use std::panic::{self, AssertUnwindSafe};
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionNativeOwnershipIdentity, ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipKey,
    ExtensionNativeOwnershipPhase, ExtensionRuntimeBackendTarget, ExtensionRuntimeGeneration,
};
use zephium_core::ports::store::{
    ExtensionGrantCohortLoadOutcome, ExtensionInstallCatalogLoadOutcome,
};
use zephium_extension_repository::{
    BundledManifestBindingsError, BundledPackageLeaseError,
    BundledRuntimeHostActivationBindingError, BundledRuntimePackageAccessBuildError,
    ExtensionRepositoryError,
};
use zephium_extension_runtime_api::{
    ExtensionPackageAccessBuildError, ExtensionRuntimeActivationSettlement,
    ExtensionRuntimeHostActivationBindingError, ExtensionRuntimeHostBindError,
    ExtensionRuntimeOwnershipEvidence, ExtensionRuntimeReconciliationSettlement,
    ExtensionRuntimeResourceBuildError, ExtensionRuntimeResourcePlanBuildError,
};
use zephium_private_fs::PrivateFsError;
use zephium_store::ExtensionServiceStoreCallOutcome;

use crate::journal_store::{
    JournalActivationPreparationFailureReason, JournalActivationReconciliation,
    JournalActivationRefusalReason, JournalActivationSettlement, JournalLoadFailure,
};
use crate::repository::ServiceRuntimeHostActivationBindingRefusal;

use super::outcome::{
    RuntimeActivationOutcome, RuntimeActivationRejectionReason, RuntimeActivationUnavailableReason,
    RuntimeCoordinatorFailureReason,
};
use super::reconciliation::{
    reconcile_row_ambiguity, reconcile_row_conflict, settle_row_fence, RowFenceProgress,
    RowFenceWait,
};
use super::retirement::{drive_release, RetirementDrive};
use super::slot::{
    AcquireLeaseState, MayOwnAccessState, MayOwnConflictState, NativeActivationState,
    NativeCallPanicAuthority, NativeUncertainState, OwnedRuntimeState,
    PackageAccessBuildRefusalState, PostHostReleaseAuthority, PreparingAccessState,
    PreparingLeaseState, ReleaseAuthority, ReleaseCompletion, ReleaseRuntimeState,
    RuntimeFailStopRetained, RuntimeOperationControl, RuntimeRowFenceAuthority,
    RuntimeRowFenceMutation, RuntimeRowFenceOperation, RuntimeSlotState,
    MAX_COORDINATOR_HOST_COMPANION_RETAINED_BYTES,
    MAX_COORDINATOR_PRE_HOST_COMPANION_RETAINED_BYTES,
};
use super::{
    reload_projection_if_unknown, run_after_projection_readiness, RuntimeCoordinator,
    RuntimeCoordinatorResources,
};

const MAX_ACTIVATION_STATE_TRANSITIONS_PER_CALL: usize = 32;

enum ActivationDrive {
    Continue(RuntimeSlotState),
    Stop {
        state: Option<RuntimeSlotState>,
        outcome: RuntimeActivationOutcome,
    },
    FailStop {
        retained: RuntimeFailStopRetained,
        reason: RuntimeCoordinatorFailureReason,
    },
}

impl RuntimeCoordinator {
    pub(crate) fn activate_until(
        &mut self,
        mut resources: RuntimeCoordinatorResources<'_>,
        key: ExtensionNativeOwnershipKey,
        profile_fenced: bool,
        deadline: Instant,
    ) -> RuntimeActivationOutcome {
        if let Some(reason) = self.fail_stop {
            return RuntimeActivationOutcome::FailedClosed(reason);
        }
        if self.journal_settlement_barrier_blocks(key) {
            return RuntimeActivationOutcome::Unavailable(
                RuntimeActivationUnavailableReason::StoreObservationPending,
            );
        }
        if profile_fenced {
            return RuntimeActivationOutcome::ProfileFenced;
        }
        if let Some(slot) = self.slot(key) {
            if slot.is_live() {
                return slot.generation().map_or(
                    RuntimeActivationOutcome::FailedClosed(
                        RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                    ),
                    RuntimeActivationOutcome::AlreadyActive,
                );
            }
            if Instant::now() >= deadline {
                return RuntimeActivationOutcome::Unavailable(
                    RuntimeActivationUnavailableReason::DeadlineReached,
                );
            }
        } else {
            // `admit_fresh_before_deadline` makes the capacity check and slot
            // insertion one closed policy step; an already-expired request
            // can never consume a slot.
            if let Err(outcome) = self.admit_fresh_before_deadline(key, deadline) {
                return outcome;
            }
        }

        let Some(mut state) = self.slot_mut(key).and_then(|slot| slot.take_state()) else {
            self.enter_fail_stop(RuntimeCoordinatorFailureReason::InternalProtocolViolation);
            return RuntimeActivationOutcome::FailedClosed(
                RuntimeCoordinatorFailureReason::InternalProtocolViolation,
            );
        };

        for _ in 0..MAX_ACTIVATION_STATE_TRANSITIONS_PER_CALL {
            let drive = self.drive_activation_state(&mut resources, key, state, deadline);
            match drive {
                ActivationDrive::Continue(next) => {
                    state = next;
                    if Instant::now() >= deadline {
                        let installed = self
                            .slot_mut(key)
                            .is_some_and(|slot| slot.install_state(state));
                        if !installed {
                            self.enter_fail_stop(
                                RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                            );
                            return RuntimeActivationOutcome::FailedClosed(
                                RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                            );
                        }
                        return RuntimeActivationOutcome::Unavailable(
                            RuntimeActivationUnavailableReason::DeadlineReached,
                        );
                    }
                }
                ActivationDrive::Stop {
                    state: retained,
                    outcome,
                } => {
                    if let RuntimeActivationOutcome::FailedClosed(reason) = outcome {
                        self.enter_fail_stop(reason);
                    }
                    if let Some(retained) = retained {
                        let installed = self
                            .slot_mut(key)
                            .is_some_and(|slot| slot.install_state(retained));
                        if !installed {
                            self.enter_fail_stop(
                                RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                            );
                            return RuntimeActivationOutcome::FailedClosed(
                                RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                            );
                        }
                    } else {
                        self.remove_slot(key);
                    }
                    return outcome;
                }
                ActivationDrive::FailStop { retained, reason } => {
                    let installed = self
                        .slot_mut(key)
                        .is_some_and(|slot| slot.enter_fail_stop(reason, retained));
                    self.enter_fail_stop(reason);
                    return if installed {
                        RuntimeActivationOutcome::FailedClosed(reason)
                    } else {
                        RuntimeActivationOutcome::FailedClosed(
                            RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                        )
                    };
                }
            }
        }

        let reason = RuntimeCoordinatorFailureReason::InternalProtocolViolation;
        let installed = self.slot_mut(key).is_some_and(|slot| {
            slot.enter_fail_stop(reason, RuntimeFailStopRetained::Prior(Box::new(state)))
        });
        self.enter_fail_stop(reason);
        debug_assert!(installed);
        RuntimeActivationOutcome::FailedClosed(reason)
    }

    fn drive_activation_state(
        &mut self,
        resources: &mut RuntimeCoordinatorResources<'_>,
        key: ExtensionNativeOwnershipKey,
        state: RuntimeSlotState,
        deadline: Instant,
    ) -> ActivationDrive {
        match state {
            RuntimeSlotState::Planning => plan_activation(resources, key, deadline),
            RuntimeSlotState::Planned(plan) => settle_begin(resources, *plan, deadline),
            RuntimeSlotState::BeginConflict(conflict) => {
                reconcile_begin_conflict(resources, key, *conflict, deadline)
            }
            RuntimeSlotState::BeginAmbiguous(ambiguity) => {
                reconcile_begin_ambiguity(resources, *ambiguity, deadline)
            }
            RuntimeSlotState::AcquireLease(state) => acquire_lease(resources, *state),
            RuntimeSlotState::PreparingLease(state) => {
                let generation = match self.ensure_slot_generation(key) {
                    Ok(generation) => generation,
                    Err(reason) => {
                        return ActivationDrive::FailStop {
                            retained: RuntimeFailStopRetained::Prior(Box::new(
                                RuntimeSlotState::PreparingLease(state),
                            )),
                            reason,
                        };
                    }
                };
                build_package_access(*state, generation)
            }
            RuntimeSlotState::PackageAccessBuildRefusal(state) => {
                settle_package_access_refusal(*state)
            }
            RuntimeSlotState::PreparingAccess(state) => settle_may_own(resources, *state, deadline),
            RuntimeSlotState::MayOwnConflict(state) => {
                reconcile_may_own_conflict(resources, *state, deadline)
            }
            RuntimeSlotState::MayOwnAmbiguous(ambiguity) => {
                reconcile_may_own_ambiguity(resources, key, *ambiguity, deadline)
            }
            RuntimeSlotState::MayOwnAccess(state) => bind_host(resources, *state),
            RuntimeSlotState::HostBindingRefusal(refusal) => settle_host_binding_refusal(*refusal),
            RuntimeSlotState::NativeActivation(state) => settle_native_activation(*state, deadline),
            RuntimeSlotState::NativeUncertain(state) => {
                reconcile_native_uncertainty(*state, deadline)
            }
            RuntimeSlotState::NativeRetirement(state) => ActivationDrive::Stop {
                state: Some(RuntimeSlotState::NativeRetirement(state)),
                outcome: RuntimeActivationOutcome::Unavailable(
                    RuntimeActivationUnavailableReason::RetirementInProgress,
                ),
            },
            RuntimeSlotState::NativeRetirementUncertain(state) => ActivationDrive::Stop {
                state: Some(RuntimeSlotState::NativeRetirementUncertain(state)),
                outcome: RuntimeActivationOutcome::Unavailable(
                    RuntimeActivationUnavailableReason::RetirementInProgress,
                ),
            },
            RuntimeSlotState::Owned(state) => {
                if state.current_entry.intent() == ExtensionNativeOwnershipIntent::Release {
                    return ActivationDrive::Stop {
                        state: Some(RuntimeSlotState::Owned(state)),
                        outcome: RuntimeActivationOutcome::Unavailable(
                            RuntimeActivationUnavailableReason::RetirementInProgress,
                        ),
                    };
                }
                let Some(generation) = self
                    .slot(key)
                    .and_then(super::slot::RuntimeSlot::generation)
                else {
                    return ActivationDrive::FailStop {
                        retained: RuntimeFailStopRetained::Prior(Box::new(
                            RuntimeSlotState::Owned(state),
                        )),
                        reason: RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                    };
                };
                drive_owned(resources, *state, generation, deadline)
            }
            RuntimeSlotState::Release(state) => {
                activation_from_retirement_drive(drive_release(resources, *state, deadline))
            }
            RuntimeSlotState::GrantRebindPending(operation) => ActivationDrive::Stop {
                state: Some(RuntimeSlotState::GrantRebindPending(operation)),
                outcome: RuntimeActivationOutcome::Unavailable(
                    RuntimeActivationUnavailableReason::StoreObservationPending,
                ),
            },
            RuntimeSlotState::FailStop(state) => ActivationDrive::Stop {
                outcome: RuntimeActivationOutcome::FailedClosed(state.reason),
                state: Some(RuntimeSlotState::FailStop(state)),
            },
            RuntimeSlotState::RowFenceConflict(state) => {
                if state.operation.is_owned_retirement_transition() {
                    return ActivationDrive::Stop {
                        state: Some(RuntimeSlotState::RowFenceConflict(state)),
                        outcome: RuntimeActivationOutcome::Unavailable(
                            RuntimeActivationUnavailableReason::RetirementInProgress,
                        ),
                    };
                }
                activation_from_row_fence(reconcile_row_conflict(resources, *state, deadline))
            }
            RuntimeSlotState::RowFenceAmbiguous(state) => {
                if state.operation.is_owned_retirement_transition() {
                    return ActivationDrive::Stop {
                        state: Some(RuntimeSlotState::RowFenceAmbiguous(state)),
                        outcome: RuntimeActivationOutcome::Unavailable(
                            RuntimeActivationUnavailableReason::RetirementInProgress,
                        ),
                    };
                }
                activation_from_row_fence(reconcile_row_ambiguity(resources, *state, deadline))
            }
        }
    }
}

fn drive_owned(
    resources: &mut RuntimeCoordinatorResources<'_>,
    state: OwnedRuntimeState,
    generation: ExtensionRuntimeGeneration,
    deadline: Instant,
) -> ActivationDrive {
    let evidence = state.owner.ownership_evidence();
    if let RuntimeOperationControl::Pending(_) = &state.operation {
        let required_identity = match durable_identity_for_evidence(&state.current_entry, evidence)
        {
            Ok(identity) => identity,
            Err(reason) => {
                return ActivationDrive::FailStop {
                    retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Owned(
                        Box::new(state),
                    ))),
                    reason,
                };
            }
        };
        if let Some(required_identity) = required_identity {
            match state.current_entry.native_identity() {
                None => {
                    return activation_from_row_fence(settle_row_fence(
                        resources,
                        RuntimeRowFenceOperation {
                            mutation: RuntimeRowFenceMutation::AttachNativeIdentity(
                                required_identity,
                            ),
                            authority: RuntimeRowFenceAuthority::Owned(Box::new(state)),
                        },
                        deadline,
                    ));
                }
                Some(persisted) if persisted != required_identity => {
                    return ActivationDrive::FailStop {
                        retained: RuntimeFailStopRetained::Prior(Box::new(
                            RuntimeSlotState::Owned(Box::new(state)),
                        )),
                        reason: RuntimeCoordinatorFailureReason::JournalDiverged,
                    };
                }
                Some(_) => {}
            }
        } else if state.current_entry.native_identity().is_some() {
            return ActivationDrive::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Owned(
                    Box::new(state),
                ))),
                reason: RuntimeCoordinatorFailureReason::JournalDiverged,
            };
        }

        match (state.current_entry.intent(), state.current_entry.phase()) {
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
            ) => {
                return activation_from_row_fence(settle_row_fence(
                    resources,
                    RuntimeRowFenceOperation {
                        mutation: RuntimeRowFenceMutation::Transition {
                            intent: ExtensionNativeOwnershipIntent::Acquire,
                            phase: ExtensionNativeOwnershipPhase::NativeOwned,
                        },
                        authority: RuntimeRowFenceAuthority::Owned(Box::new(state)),
                    },
                    deadline,
                ));
            }
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeOwned,
            ) => {}
            _ => {
                return ActivationDrive::FailStop {
                    retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Owned(
                        Box::new(state),
                    ))),
                    reason: RuntimeCoordinatorFailureReason::JournalDiverged,
                };
            }
        }
    }

    // Publication authority is deliberately not cached across state-machine
    // yields. Pending may survive the identity/Owned row fences, and a failed
    // publish returns PublicationRequest to the slot. Reload and prove the
    // exact row immediately before every authorization and publish attempt.
    //
    // The authorize/publish dispatch itself runs inside the gate below. This
    // makes it structurally impossible to accidentally move either native
    // call past a failed freshness check during a later refactor.
    let revalidation_stage = publication_revalidation_stage(&state.operation);
    let revalidation = if publication_revalidation_required(revalidation_stage) {
        resources
            .projection
            .reload(resources.store, deadline)
            .map(|journal| journal.get(state.current_entry.key()) == Some(&state.current_entry))
    } else {
        Ok(true)
    };
    let mut retained_state = Some(state);
    match run_after_publication_revalidation(revalidation_stage, revalidation, || {
        drive_owned_operation(
            retained_state
                .take()
                .expect("publication gate invokes its action at most once"),
            evidence,
            generation,
        )
    }) {
        Ok(drive) => drive,
        Err(failure) => {
            let state = retained_state
                .take()
                .expect("failed publication freshness gate retains exact authority");
            match failure {
                PublicationRevalidationFailure::Diverged => ActivationDrive::FailStop {
                    retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Owned(
                        Box::new(state),
                    ))),
                    reason: RuntimeCoordinatorFailureReason::JournalDiverged,
                },
                PublicationRevalidationFailure::NotAdmitted => activation_unavailable(
                    RuntimeSlotState::Owned(Box::new(state)),
                    RuntimeActivationUnavailableReason::StoreNotAdmitted,
                ),
                PublicationRevalidationFailure::TimedOutAfterAdmission => activation_unavailable(
                    RuntimeSlotState::Owned(Box::new(state)),
                    RuntimeActivationUnavailableReason::StoreObservationPending,
                ),
                PublicationRevalidationFailure::Failed => ActivationDrive::FailStop {
                    retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Owned(
                        Box::new(state),
                    ))),
                    reason: RuntimeCoordinatorFailureReason::JournalInvalid,
                },
            }
        }
    }
}

fn drive_owned_operation(
    state: OwnedRuntimeState,
    evidence: ExtensionRuntimeOwnershipEvidence,
    generation: ExtensionRuntimeGeneration,
) -> ActivationDrive {
    let OwnedRuntimeState {
        current_entry,
        owner,
        operation,
        recovery,
    } = state;
    match operation {
        RuntimeOperationControl::Pending(pending) => {
            match pending.authorize(current_entry.clone(), evidence) {
                Ok(request) => ActivationDrive::Continue(RuntimeSlotState::Owned(Box::new(
                    OwnedRuntimeState {
                        current_entry,
                        owner,
                        operation: RuntimeOperationControl::PublicationRequest(Box::new(request)),
                        recovery,
                    },
                ))),
                Err(refusal) => {
                    let (pending, refused_entry, refused_evidence) = refusal.into_parts();
                    let reason = if refused_entry == current_entry && refused_evidence == evidence {
                        RuntimeCoordinatorFailureReason::HostInvariant
                    } else {
                        RuntimeCoordinatorFailureReason::InternalProtocolViolation
                    };
                    ActivationDrive::FailStop {
                        retained: RuntimeFailStopRetained::Prior(Box::new(
                            RuntimeSlotState::Owned(Box::new(OwnedRuntimeState {
                                current_entry,
                                owner,
                                operation: RuntimeOperationControl::Pending(Box::new(pending)),
                                recovery,
                            })),
                        )),
                        reason,
                    }
                }
            }
        }
        RuntimeOperationControl::PublicationRequest(request) => {
            match panic::catch_unwind(AssertUnwindSafe(|| request.publish())) {
                Err(_) => ActivationDrive::FailStop {
                    retained: RuntimeFailStopRetained::PublicationCallPanicked(Box::new(
                        super::slot::PublicationCallPanicAuthority {
                            owner,
                            recovery,
                            current_entry,
                        },
                    )),
                    reason: RuntimeCoordinatorFailureReason::NativeCallPanicked,
                },
                Ok(publication) => match publication {
                    Ok(receipt) => ActivationDrive::Stop {
                        state: Some(RuntimeSlotState::Owned(Box::new(OwnedRuntimeState {
                            current_entry,
                            owner,
                            operation: RuntimeOperationControl::Published(Box::new(receipt)),
                            recovery,
                        }))),
                        outcome: RuntimeActivationOutcome::Activated(generation),
                    },
                    Err(refusal) => {
                        let reason = refusal.reason();
                        match refusal.try_into_request() {
                            Ok(request) => {
                                let retained =
                                    RuntimeSlotState::Owned(Box::new(OwnedRuntimeState {
                                        current_entry,
                                        owner,
                                        operation: RuntimeOperationControl::PublicationRequest(
                                            Box::new(request),
                                        ),
                                        recovery,
                                    }));
                                match publication_failure_disposition(reason) {
                                    PublicationFailureDisposition::Retry => activation_unavailable(
                                        retained,
                                        RuntimeActivationUnavailableReason::PublicationPending(
                                            reason,
                                        ),
                                    ),
                                    PublicationFailureDisposition::Fail(failure) => {
                                        ActivationDrive::FailStop {
                                            retained: RuntimeFailStopRetained::Prior(Box::new(
                                                retained,
                                            )),
                                            reason: failure,
                                        }
                                    }
                                }
                            }
                            Err(refusal) => ActivationDrive::FailStop {
                                retained: RuntimeFailStopRetained::Publication(Box::new(
                                    super::slot::PublicationFailStopAuthority {
                                        owner,
                                        refusal,
                                        recovery,
                                        current_entry,
                                    },
                                )),
                                reason: RuntimeCoordinatorFailureReason::HostInvariant,
                            },
                        }
                    }
                },
            }
        }
        RuntimeOperationControl::Published(receipt) => ActivationDrive::Stop {
            state: Some(RuntimeSlotState::Owned(Box::new(OwnedRuntimeState {
                current_entry,
                owner,
                operation: RuntimeOperationControl::Published(receipt),
                recovery,
            }))),
            outcome: RuntimeActivationOutcome::Activated(generation),
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PublicationRevalidationStage {
    PendingAuthorization,
    PublicationAttempt,
    Published,
}

fn publication_revalidation_stage(
    operation: &RuntimeOperationControl,
) -> PublicationRevalidationStage {
    match operation {
        RuntimeOperationControl::Pending(_) => PublicationRevalidationStage::PendingAuthorization,
        RuntimeOperationControl::PublicationRequest(_) => {
            PublicationRevalidationStage::PublicationAttempt
        }
        RuntimeOperationControl::Published(_) => PublicationRevalidationStage::Published,
    }
}

pub(super) const fn publication_revalidation_required(stage: PublicationRevalidationStage) -> bool {
    matches!(
        stage,
        PublicationRevalidationStage::PendingAuthorization
            | PublicationRevalidationStage::PublicationAttempt
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PublicationRevalidationFailure {
    Diverged,
    NotAdmitted,
    TimedOutAfterAdmission,
    Failed,
}

/// Executes an authorization or publication call only after the immediately
/// preceding Store reload proved the exact owned row. Keeping the action
/// inside this seam lets tests prove call suppression without fabricating
/// move-only host capabilities.
pub(super) fn run_after_publication_revalidation<T>(
    stage: PublicationRevalidationStage,
    revalidation: Result<bool, JournalLoadFailure>,
    action: impl FnOnce() -> T,
) -> Result<T, PublicationRevalidationFailure> {
    if !publication_revalidation_required(stage) {
        return Ok(action());
    }
    match revalidation {
        Ok(true) => Ok(action()),
        Ok(false) => Err(PublicationRevalidationFailure::Diverged),
        Err(JournalLoadFailure::NotAdmitted) => Err(PublicationRevalidationFailure::NotAdmitted),
        Err(JournalLoadFailure::TimedOutAfterAdmission) => {
            Err(PublicationRevalidationFailure::TimedOutAfterAdmission)
        }
        Err(JournalLoadFailure::Failed) => Err(PublicationRevalidationFailure::Failed),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PublicationFailureDisposition {
    Retry,
    Fail(RuntimeCoordinatorFailureReason),
}

pub(super) const fn publication_failure_disposition(
    reason: ExtensionRuntimeHostBindError,
) -> PublicationFailureDisposition {
    match reason {
        ExtensionRuntimeHostBindError::Unavailable => PublicationFailureDisposition::Retry,
        ExtensionRuntimeHostBindError::RetainedBytesOverflow => {
            PublicationFailureDisposition::Fail(
                RuntimeCoordinatorFailureReason::RetainedBytesOverflow,
            )
        }
        ExtensionRuntimeHostBindError::UnsupportedBackend
        | ExtensionRuntimeHostBindError::CapacityExceeded
        | ExtensionRuntimeHostBindError::OwnerConflict
        | ExtensionRuntimeHostBindError::Sealed
        | ExtensionRuntimeHostBindError::IdentityExhausted
        | ExtensionRuntimeHostBindError::RetainedBytesExceeded
        | ExtensionRuntimeHostBindError::InternalInvariant => {
            PublicationFailureDisposition::Fail(RuntimeCoordinatorFailureReason::HostInvariant)
        }
        _ => PublicationFailureDisposition::Fail(RuntimeCoordinatorFailureReason::HostInvariant),
    }
}

fn durable_identity_for_evidence(
    entry: &zephium_core::extensions::ExtensionNativeOwnershipEntry,
    evidence: ExtensionRuntimeOwnershipEvidence,
) -> Result<Option<ExtensionNativeOwnershipIdentity>, RuntimeCoordinatorFailureReason> {
    let (expected_backend, bytes) = match evidence {
        ExtensionRuntimeOwnershipEvidence::MacosWebExtension(identity) => (
            ExtensionRuntimeBackendTarget::MacosNative,
            Some(identity.encoded_bytes()),
        ),
        ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(identity) => (
            ExtensionRuntimeBackendTarget::WindowsNative,
            Some(identity.encoded_bytes()),
        ),
        ExtensionRuntimeOwnershipEvidence::Compatibility => match entry.runtime_backend() {
            ExtensionRuntimeBackendTarget::MacosCompatibility
            | ExtensionRuntimeBackendTarget::LinuxCompatibility => {
                return Ok(None);
            }
            ExtensionRuntimeBackendTarget::MacosNative
            | ExtensionRuntimeBackendTarget::WindowsNative => {
                return Err(RuntimeCoordinatorFailureReason::HostInvariant);
            }
        },
        _ => return Err(RuntimeCoordinatorFailureReason::HostInvariant),
    };
    if entry.runtime_backend() != expected_backend {
        return Err(RuntimeCoordinatorFailureReason::HostInvariant);
    }
    let Some(bytes) = bytes else {
        return Err(RuntimeCoordinatorFailureReason::HostInvariant);
    };
    ExtensionNativeOwnershipIdentity::from_encoded_bytes(expected_backend, bytes)
        .map(Some)
        .map_err(|_| RuntimeCoordinatorFailureReason::HostInvariant)
}

fn activation_from_row_fence(progress: RowFenceProgress) -> ActivationDrive {
    match progress {
        RowFenceProgress::Continue(state) => ActivationDrive::Continue(state),
        RowFenceProgress::Wait { state, reason } => activation_unavailable(
            state,
            match reason {
                RowFenceWait::StoreNotAdmitted => {
                    RuntimeActivationUnavailableReason::StoreNotAdmitted
                }
                RowFenceWait::StoreObservationPending => {
                    RuntimeActivationUnavailableReason::StoreObservationPending
                }
            },
        ),
        RowFenceProgress::Cleared(completion) => ActivationDrive::Stop {
            state: None,
            outcome: activation_completion_outcome(completion),
        },
        RowFenceProgress::FailStop { retained, reason } => {
            ActivationDrive::FailStop { retained, reason }
        }
    }
}

fn activation_from_retirement_drive(drive: RetirementDrive) -> ActivationDrive {
    match drive {
        RetirementDrive::Continue(state) => ActivationDrive::Continue(state),
        RetirementDrive::Stop { state, reason } => activation_unavailable(
            state,
            match reason {
                super::outcome::RuntimeRetirementUnavailableReason::DeadlineReached => {
                    RuntimeActivationUnavailableReason::DeadlineReached
                }
                super::outcome::RuntimeRetirementUnavailableReason::StoreNotAdmitted => {
                    RuntimeActivationUnavailableReason::StoreNotAdmitted
                }
                super::outcome::RuntimeRetirementUnavailableReason::StoreObservationPending => {
                    RuntimeActivationUnavailableReason::StoreObservationPending
                }
                super::outcome::RuntimeRetirementUnavailableReason::RepositoryUnavailable => {
                    RuntimeActivationUnavailableReason::RepositoryUnavailable
                }
                super::outcome::RuntimeRetirementUnavailableReason::NativeRetained(_)
                | super::outcome::RuntimeRetirementUnavailableReason::NativeOwnershipUncertain(_)
                | super::outcome::RuntimeRetirementUnavailableReason::PublicationReclaimPending
                | super::outcome::RuntimeRetirementUnavailableReason::ActivationReconciliationPending => {
                    RuntimeActivationUnavailableReason::RetirementInProgress
                }
            },
        ),
        RetirementDrive::Complete(completion) => ActivationDrive::Stop {
            state: None,
            outcome: activation_completion_outcome(completion),
        },
        RetirementDrive::FailStop { retained, reason } => {
            ActivationDrive::FailStop { retained, reason }
        }
    }
}

pub(super) const fn activation_completion_outcome(
    completion: ReleaseCompletion,
) -> RuntimeActivationOutcome {
    match completion {
        ReleaseCompletion::ActivationRejected(reason) => RuntimeActivationOutcome::Rejected(reason),
        ReleaseCompletion::ActivationUnavailable(reason) => {
            RuntimeActivationOutcome::Unavailable(reason)
        }
        // Explicit retirement may already be in Release when activation is
        // called again. Completing that cleanup removes the slot but is not a
        // coordinator invariant failure.
        ReleaseCompletion::Retired => RuntimeActivationOutcome::Unavailable(
            RuntimeActivationUnavailableReason::RetirementInProgress,
        ),
    }
}

fn plan_activation(
    resources: &mut RuntimeCoordinatorResources<'_>,
    key: ExtensionNativeOwnershipKey,
    deadline: Instant,
) -> ActivationDrive {
    let catalog = match resources
        .store
        .load_install_catalog_until(key.profile(), deadline)
    {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::Loaded(catalog),
        ) => catalog,
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::NotRegistered
            | ExtensionInstallCatalogLoadOutcome::DegradedProfile,
        ) => return activation_rejected(RuntimeActivationRejectionReason::ProfileUnavailable),
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallCatalogLoadOutcome::Failed) => {
            return activation_failed_without_authority(
                RuntimeCoordinatorFailureReason::JournalInvalid,
            );
        }
        ExtensionServiceStoreCallOutcome::NotAdmitted => {
            return activation_unavailable(
                RuntimeSlotState::Planning,
                RuntimeActivationUnavailableReason::StoreNotAdmitted,
            );
        }
        ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return activation_unavailable(
                RuntimeSlotState::Planning,
                RuntimeActivationUnavailableReason::StoreObservationPending,
            );
        }
    };

    let bindings = match resources
        .repository
        .authenticate_runtime_manifest_bindings(&catalog)
    {
        Ok(bindings) => bindings,
        Err(error) => return manifest_binding_failure(error),
    };
    let (current, store_bindings, manifest) =
        match bindings.into_store_bindings_and_manifest(key.install_id()) {
            Ok(parts) => parts,
            Err(_refusal) => {
                return activation_rejected(RuntimeActivationRejectionReason::PackageUnavailable);
            }
        };
    let cohort =
        match resources
            .store
            .load_grant_cohort_until(key.profile(), store_bindings, deadline)
        {
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionGrantCohortLoadOutcome::Loaded(cohort),
            ) => cohort,
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionGrantCohortLoadOutcome::NotRegistered
                | ExtensionGrantCohortLoadOutcome::DegradedProfile,
            ) => return activation_rejected(RuntimeActivationRejectionReason::ProfileUnavailable),
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionGrantCohortLoadOutcome::Invalid,
            ) => {
                return activation_rejected(RuntimeActivationRejectionReason::StoreCohortChanged);
            }
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionGrantCohortLoadOutcome::Failed,
            ) => {
                return activation_failed_without_authority(
                    RuntimeCoordinatorFailureReason::JournalInvalid,
                );
            }
            ExtensionServiceStoreCallOutcome::NotAdmitted => {
                return activation_unavailable(
                    RuntimeSlotState::Planning,
                    RuntimeActivationUnavailableReason::StoreNotAdmitted,
                );
            }
            ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
                return activation_unavailable(
                    RuntimeSlotState::Planning,
                    RuntimeActivationUnavailableReason::StoreObservationPending,
                );
            }
        };
    let eligibility = match cohort.runtime_eligibility(key.install_id(), key.browsing_context()) {
        Ok(eligibility) => eligibility,
        Err(reason) => {
            return activation_rejected(RuntimeActivationRejectionReason::InstallUnavailable(
                reason,
            ));
        }
    };
    let plan = match resources
        .repository
        .plan_runtime_acquisition_with_additional_companion_retained_bytes(
            current,
            eligibility,
            manifest,
            MAX_COORDINATOR_PRE_HOST_COMPANION_RETAINED_BYTES,
        ) {
        Ok(plan) => plan,
        Err(refusal) => return runtime_planning_failure(refusal),
    };
    if resources.projection.known().is_none() {
        match resources.projection.reload(resources.store, deadline) {
            Ok(_) => {}
            Err(failure) => {
                return retain_plan_for_load_failure(plan, failure);
            }
        }
    }
    ActivationDrive::Continue(RuntimeSlotState::Planned(Box::new(plan)))
}

fn settle_begin(
    resources: &mut RuntimeCoordinatorResources<'_>,
    plan: crate::repository::ServiceRuntimeAcquisitionPlan,
    deadline: Instant,
) -> ActivationDrive {
    let readiness = reload_projection_if_unknown(resources.projection, resources.store, deadline);
    let mut retained_plan = Some(plan);
    match run_after_projection_readiness(readiness, || {
        settle_known_begin(
            resources,
            retained_plan
                .take()
                .expect("projection gate invokes Begin settlement at most once"),
            deadline,
        )
    }) {
        Ok(drive) => drive,
        Err(failure) => retain_plan_for_load_failure(
            retained_plan
                .take()
                .expect("failed projection gate retains the exact plan"),
            failure,
        ),
    }
}

fn settle_known_begin(
    resources: &mut RuntimeCoordinatorResources<'_>,
    plan: crate::repository::ServiceRuntimeAcquisitionPlan,
    deadline: Instant,
) -> ActivationDrive {
    match resources
        .projection
        .settle_fenced_begin(resources.store, plan, deadline)
    {
        Ok(JournalActivationSettlement::Applied(applied)) => {
            let (plan, preparing) = applied.into_parts();
            ActivationDrive::Continue(RuntimeSlotState::AcquireLease(Box::new(
                AcquireLeaseState { plan, preparing },
            )))
        }
        Ok(JournalActivationSettlement::NotAdmitted(returned)) => activation_unavailable(
            RuntimeSlotState::Planned(Box::new(returned.into_authority())),
            RuntimeActivationUnavailableReason::StoreNotAdmitted,
        ),
        Ok(JournalActivationSettlement::Refused(refusal)) => {
            let reason = begin_refusal_reason(refusal.reason());
            drop(refusal.into_returned());
            reason
        }
        Ok(JournalActivationSettlement::Conflict(conflict)) => ActivationDrive::Stop {
            state: Some(RuntimeSlotState::BeginConflict(Box::new(conflict))),
            outcome: RuntimeActivationOutcome::Unavailable(
                RuntimeActivationUnavailableReason::StoreObservationPending,
            ),
        },
        Ok(JournalActivationSettlement::ReloadRequired(ambiguity)) => ActivationDrive::Stop {
            state: Some(RuntimeSlotState::BeginAmbiguous(Box::new(ambiguity))),
            outcome: RuntimeActivationOutcome::Unavailable(
                RuntimeActivationUnavailableReason::StoreObservationPending,
            ),
        },
        Err(failure) => {
            let reason = failure.reason();
            let plan = failure.into_authority();
            if reason == JournalActivationPreparationFailureReason::ReloadRequired {
                reload_plan_after_preparation_failure(resources, plan, deadline)
            } else {
                begin_preparation_failure(plan, reason)
            }
        }
    }
}

fn reconcile_begin_conflict(
    resources: &mut RuntimeCoordinatorResources<'_>,
    key: ExtensionNativeOwnershipKey,
    conflict: crate::journal_store::JournalActivationConflict<
        crate::repository::ServiceRuntimeAcquisitionPlan,
    >,
    deadline: Instant,
) -> ActivationDrive {
    match resources.projection.reload(resources.store, deadline) {
        Ok(journal) if journal.get(key).is_none() => {
            let plan = conflict.into_returned().into_authority();
            ActivationDrive::Continue(RuntimeSlotState::Planned(Box::new(plan)))
        }
        Ok(_) => ActivationDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::BeginConflict(
                Box::new(conflict),
            ))),
            reason: RuntimeCoordinatorFailureReason::JournalDiverged,
        },
        Err(failure) => retain_begin_conflict_for_load_failure(conflict, failure),
    }
}

fn reconcile_begin_ambiguity(
    resources: &mut RuntimeCoordinatorResources<'_>,
    ambiguity: crate::journal_store::JournalActivationReloadRequired<
        crate::repository::ServiceRuntimeAcquisitionPlan,
    >,
    deadline: Instant,
) -> ActivationDrive {
    if let Err(failure) = resources.projection.reload(resources.store, deadline) {
        return retain_begin_ambiguity_for_load_failure(ambiguity, failure);
    }
    match ambiguity.reconcile(resources.projection) {
        JournalActivationReconciliation::Applied(applied) => {
            let (plan, preparing) = applied.into_parts();
            ActivationDrive::Continue(RuntimeSlotState::AcquireLease(Box::new(
                AcquireLeaseState { plan, preparing },
            )))
        }
        JournalActivationReconciliation::NotApplied(returned) => ActivationDrive::Continue(
            RuntimeSlotState::Planned(Box::new(returned.into_authority())),
        ),
        JournalActivationReconciliation::ReloadPending(ambiguity) => activation_unavailable(
            RuntimeSlotState::BeginAmbiguous(Box::new(ambiguity)),
            RuntimeActivationUnavailableReason::StoreObservationPending,
        ),
        JournalActivationReconciliation::Diverged(diverged) => ActivationDrive::FailStop {
            retained: RuntimeFailStopRetained::BeginDiverged(Box::new(diverged)),
            reason: RuntimeCoordinatorFailureReason::JournalDiverged,
        },
    }
}

fn acquire_lease(
    resources: &mut RuntimeCoordinatorResources<'_>,
    state: AcquireLeaseState,
) -> ActivationDrive {
    let AcquireLeaseState { plan, preparing } = state;
    match resources.repository.acquire_runtime_lease(plan, &preparing) {
        Ok(lease) => ActivationDrive::Continue(RuntimeSlotState::PreparingLease(Box::new(
            PreparingLeaseState { lease, preparing },
        ))),
        Err(error) => match error.try_into_plan() {
            Ok((reason, plan)) => ActivationDrive::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::AcquireLease(
                    Box::new(AcquireLeaseState { plan, preparing }),
                ))),
                reason: acquisition_plan_refusal_failure(reason),
            },
            Err(error) => ActivationDrive::FailStop {
                retained: RuntimeFailStopRetained::RepositoryAcquisition(Box::new(error)),
                reason: RuntimeCoordinatorFailureReason::RepositoryInvariant,
            },
        },
    }
}

pub(super) const fn acquisition_plan_refusal_failure(
    reason: zephium_extension_repository::BundledRuntimeAcquisitionPlanRefusalReason,
) -> RuntimeCoordinatorFailureReason {
    use zephium_extension_repository::BundledRuntimeAcquisitionPlanRefusalReason as Reason;

    // Both refusals are pure same-open/exact-row validations. Neither performs
    // repository I/O, so retrying the unchanged plan can never heal it.
    match reason {
        Reason::WrongRepositoryOpen => RuntimeCoordinatorFailureReason::RepositoryInvariant,
        Reason::AppliedOwnershipMismatch => RuntimeCoordinatorFailureReason::JournalDiverged,
        _ => RuntimeCoordinatorFailureReason::RepositoryInvariant,
    }
}

fn build_package_access(
    state: PreparingLeaseState,
    generation: zephium_core::extensions::ExtensionRuntimeGeneration,
) -> ActivationDrive {
    let PreparingLeaseState { lease, preparing } = state;
    match lease.into_runtime_package_access_with_additional_companion_retained_bytes(
        generation,
        MAX_COORDINATOR_PRE_HOST_COMPANION_RETAINED_BYTES,
    ) {
        Ok(access) => ActivationDrive::Continue(RuntimeSlotState::PreparingAccess(Box::new(
            PreparingAccessState { access, preparing },
        ))),
        Err(refusal) => ActivationDrive::Continue(RuntimeSlotState::PackageAccessBuildRefusal(
            Box::new(PackageAccessBuildRefusalState { refusal, preparing }),
        )),
    }
}

fn settle_package_access_refusal(state: PackageAccessBuildRefusalState) -> ActivationDrive {
    let PackageAccessBuildRefusalState { refusal, preparing } = state;
    match package_access_build_disposition(refusal.reason()) {
        PackageAccessBuildDisposition::FailStop(reason) => match refusal.try_into_lease() {
            Ok(lease) => ActivationDrive::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(
                    RuntimeSlotState::PreparingLease(Box::new(PreparingLeaseState {
                        lease,
                        preparing,
                    })),
                )),
                reason,
            },
            Err(refusal) => ActivationDrive::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(
                    RuntimeSlotState::PackageAccessBuildRefusal(Box::new(
                        PackageAccessBuildRefusalState { refusal, preparing },
                    )),
                )),
                reason: RuntimeCoordinatorFailureReason::RepositoryInvariant,
            },
        },
        PackageAccessBuildDisposition::Reject(rejection) => match refusal.try_into_lease() {
            Ok(lease) => release_prehost(
                preparing,
                ReleaseAuthority::Lease(Box::new(lease)),
                rejection,
            ),
            Err(refusal) => ActivationDrive::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(
                    RuntimeSlotState::PackageAccessBuildRefusal(Box::new(
                        PackageAccessBuildRefusalState { refusal, preparing },
                    )),
                )),
                reason: RuntimeCoordinatorFailureReason::RepositoryInvariant,
            },
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PackageAccessBuildDisposition {
    Reject(RuntimeActivationRejectionReason),
    FailStop(RuntimeCoordinatorFailureReason),
}

pub(super) const fn package_access_build_disposition(
    reason: BundledRuntimePackageAccessBuildError,
) -> PackageAccessBuildDisposition {
    use BundledRuntimePackageAccessBuildError as Error;

    match reason {
        Error::UnsupportedRuntimeTarget => {
            PackageAccessBuildDisposition::Reject(RuntimeActivationRejectionReason::HostUnsupported)
        }
        Error::RetainedBytesExceeded
        | Error::ResourcePlan(ExtensionRuntimeResourcePlanBuildError::RetainedBytesExceeded {
            ..
        })
        | Error::PackageAccess(ExtensionPackageAccessBuildError::RetainedBytesExceeded) => {
            PackageAccessBuildDisposition::Reject(
                RuntimeActivationRejectionReason::RetainedBytesExceeded,
            )
        }
        Error::RetainedBytesOverflow
        | Error::ResourcePlan(ExtensionRuntimeResourcePlanBuildError::AccountingOverflow)
        | Error::PackageAccess(ExtensionPackageAccessBuildError::RetainedBytesOverflow) => {
            PackageAccessBuildDisposition::FailStop(
                RuntimeCoordinatorFailureReason::RetainedBytesOverflow,
            )
        }
        Error::ResourceBinding {
            error:
                ExtensionRuntimeResourceBuildError::InvalidPath
                | ExtensionRuntimeResourceBuildError::LengthExceeded { .. },
            ..
        }
        | Error::ResourcePlan(
            ExtensionRuntimeResourcePlanBuildError::Empty
            | ExtensionRuntimeResourcePlanBuildError::TooManyEntries { .. }
            | ExtensionRuntimeResourcePlanBuildError::NonCanonicalOrder
            | ExtensionRuntimeResourcePlanBuildError::PathCollision
            | ExtensionRuntimeResourcePlanBuildError::ManifestMissing
            | ExtensionRuntimeResourcePlanBuildError::ManifestLength,
        )
        | Error::NativeIdentityUnavailable => PackageAccessBuildDisposition::Reject(
            RuntimeActivationRejectionReason::PackageUnavailable,
        ),
        Error::InternalBindingMismatch
        | Error::ResourceBinding { .. }
        | Error::ResourcePlan(_)
        | Error::PackageAccess(_) => PackageAccessBuildDisposition::FailStop(
            RuntimeCoordinatorFailureReason::RepositoryInvariant,
        ),
        _ => PackageAccessBuildDisposition::FailStop(
            RuntimeCoordinatorFailureReason::RepositoryInvariant,
        ),
    }
}

fn settle_may_own(
    resources: &mut RuntimeCoordinatorResources<'_>,
    state: PreparingAccessState,
    deadline: Instant,
) -> ActivationDrive {
    let readiness = reload_projection_if_unknown(resources.projection, resources.store, deadline);
    let mut retained_state = Some(state);
    match run_after_projection_readiness(readiness, || {
        settle_known_may_own(
            resources,
            retained_state
                .take()
                .expect("projection gate invokes MayOwn settlement at most once"),
            deadline,
        )
    }) {
        Ok(drive) => drive,
        Err(failure) => retain_state_for_load_failure(
            RuntimeSlotState::PreparingAccess(Box::new(
                retained_state
                    .take()
                    .expect("failed projection gate retains exact package access"),
            )),
            failure,
        ),
    }
}

fn settle_known_may_own(
    resources: &mut RuntimeCoordinatorResources<'_>,
    state: PreparingAccessState,
    deadline: Instant,
) -> ActivationDrive {
    let PreparingAccessState { access, preparing } = state;
    match resources.projection.settle_preparing_to_may_own(
        resources.store,
        access,
        preparing.clone(),
        deadline,
    ) {
        Ok(JournalActivationSettlement::Applied(applied)) => {
            let (access, entry) = applied.into_parts();
            ActivationDrive::Continue(RuntimeSlotState::MayOwnAccess(Box::new(
                MayOwnAccessState { access, entry },
            )))
        }
        Ok(JournalActivationSettlement::NotAdmitted(returned)) => activation_unavailable(
            RuntimeSlotState::PreparingAccess(Box::new(PreparingAccessState {
                access: returned.into_authority(),
                preparing,
            })),
            RuntimeActivationUnavailableReason::StoreNotAdmitted,
        ),
        Ok(JournalActivationSettlement::Refused(refusal)) => {
            let disposition = may_own_refusal_disposition(refusal.reason());
            let access = refusal.into_returned().into_authority();
            match disposition {
                MayOwnRefusalDisposition::Rollback(rejection) => release_prehost(
                    preparing,
                    ReleaseAuthority::PreHost(Box::new(access)),
                    rejection,
                ),
                MayOwnRefusalDisposition::FailStop(reason) => ActivationDrive::FailStop {
                    retained: RuntimeFailStopRetained::Prior(Box::new(
                        RuntimeSlotState::PreparingAccess(Box::new(PreparingAccessState {
                            access,
                            preparing,
                        })),
                    )),
                    reason,
                },
            }
        }
        Ok(JournalActivationSettlement::Conflict(conflict)) => ActivationDrive::Stop {
            state: Some(RuntimeSlotState::MayOwnConflict(Box::new(
                MayOwnConflictState {
                    preparing,
                    conflict,
                },
            ))),
            outcome: RuntimeActivationOutcome::Unavailable(
                RuntimeActivationUnavailableReason::StoreObservationPending,
            ),
        },
        Ok(JournalActivationSettlement::ReloadRequired(ambiguity)) => ActivationDrive::Stop {
            state: Some(RuntimeSlotState::MayOwnAmbiguous(Box::new(ambiguity))),
            outcome: RuntimeActivationOutcome::Unavailable(
                RuntimeActivationUnavailableReason::StoreObservationPending,
            ),
        },
        Err(failure) => {
            let reason = failure.reason();
            let access = failure.into_authority();
            match reason {
                JournalActivationPreparationFailureReason::ReloadRequired => {
                    reload_preparing_access_after_preparation_failure(
                        resources, access, preparing, deadline,
                    )
                }
                JournalActivationPreparationFailureReason::RetainedBytesExceeded => {
                    release_prehost(
                        preparing,
                        ReleaseAuthority::PreHost(Box::new(access)),
                        RuntimeActivationRejectionReason::RetainedBytesExceeded,
                    )
                }
                JournalActivationPreparationFailureReason::RetainedBytesOverflow => {
                    ActivationDrive::FailStop {
                        retained: RuntimeFailStopRetained::Prior(Box::new(
                            RuntimeSlotState::PreparingAccess(Box::new(PreparingAccessState {
                                access,
                                preparing,
                            })),
                        )),
                        reason: RuntimeCoordinatorFailureReason::RetainedBytesOverflow,
                    }
                }
                JournalActivationPreparationFailureReason::CurrentRowMismatch => {
                    ActivationDrive::FailStop {
                        retained: RuntimeFailStopRetained::Prior(Box::new(
                            RuntimeSlotState::PreparingAccess(Box::new(PreparingAccessState {
                                access,
                                preparing,
                            })),
                        )),
                        reason: RuntimeCoordinatorFailureReason::JournalDiverged,
                    }
                }
                _ => ActivationDrive::FailStop {
                    retained: RuntimeFailStopRetained::Prior(Box::new(
                        RuntimeSlotState::PreparingAccess(Box::new(PreparingAccessState {
                            access,
                            preparing,
                        })),
                    )),
                    reason: RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                },
            }
        }
    }
}

fn reconcile_may_own_conflict(
    resources: &mut RuntimeCoordinatorResources<'_>,
    state: MayOwnConflictState,
    deadline: Instant,
) -> ActivationDrive {
    let MayOwnConflictState {
        preparing,
        conflict,
    } = state;
    match resources.projection.reload(resources.store, deadline) {
        Ok(journal) if journal.get(preparing.key()) == Some(&preparing) => {
            let access = conflict.into_returned().into_authority();
            ActivationDrive::Continue(RuntimeSlotState::PreparingAccess(Box::new(
                PreparingAccessState { access, preparing },
            )))
        }
        Ok(_) => ActivationDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::MayOwnConflict(
                Box::new(MayOwnConflictState {
                    preparing,
                    conflict,
                }),
            ))),
            reason: RuntimeCoordinatorFailureReason::JournalDiverged,
        },
        Err(failure) => retain_may_own_conflict_for_load_failure(preparing, conflict, failure),
    }
}

fn reconcile_may_own_ambiguity(
    resources: &mut RuntimeCoordinatorResources<'_>,
    key: ExtensionNativeOwnershipKey,
    ambiguity: crate::journal_store::JournalActivationReloadRequired<
        crate::repository::ServiceRuntimePackageAccess,
    >,
    deadline: Instant,
) -> ActivationDrive {
    if let Err(failure) = resources.projection.reload(resources.store, deadline) {
        return retain_may_own_ambiguity_for_load_failure(ambiguity, failure);
    }
    match ambiguity.reconcile(resources.projection) {
        JournalActivationReconciliation::Applied(applied) => {
            let (access, entry) = applied.into_parts();
            ActivationDrive::Continue(RuntimeSlotState::MayOwnAccess(Box::new(
                MayOwnAccessState { access, entry },
            )))
        }
        JournalActivationReconciliation::NotApplied(returned) => {
            let Some(preparing) = resources
                .projection
                .known()
                .and_then(|journal| journal.get(key))
                .cloned()
            else {
                return ActivationDrive::FailStop {
                    retained: RuntimeFailStopRetained::PreHostAccess(Box::new(
                        returned.into_authority(),
                    )),
                    reason: RuntimeCoordinatorFailureReason::JournalDiverged,
                };
            };
            ActivationDrive::Continue(RuntimeSlotState::PreparingAccess(Box::new(
                PreparingAccessState {
                    access: returned.into_authority(),
                    preparing,
                },
            )))
        }
        JournalActivationReconciliation::ReloadPending(ambiguity) => activation_unavailable(
            RuntimeSlotState::MayOwnAmbiguous(Box::new(ambiguity)),
            RuntimeActivationUnavailableReason::StoreObservationPending,
        ),
        JournalActivationReconciliation::Diverged(diverged) => ActivationDrive::FailStop {
            retained: RuntimeFailStopRetained::MayOwnDiverged(Box::new(diverged)),
            reason: RuntimeCoordinatorFailureReason::JournalDiverged,
        },
    }
}

fn bind_host(
    resources: &mut RuntimeCoordinatorResources<'_>,
    state: MayOwnAccessState,
) -> ActivationDrive {
    let MayOwnAccessState { access, entry } = state;
    let Some(factory) = resources.native_recovery.idle_factory() else {
        return activation_unavailable(
            RuntimeSlotState::MayOwnAccess(Box::new(MayOwnAccessState { access, entry })),
            RuntimeActivationUnavailableReason::NativeRecoveryInProgress,
        );
    };
    match access.try_into_host_activation_with_additional_companion_retained_bytes(
        entry.clone(),
        factory,
        MAX_COORDINATOR_HOST_COMPANION_RETAINED_BYTES,
    ) {
        Ok(activation) => {
            let (activation, recovery) = activation.into_parts();
            let (request, pending) = activation.into_parts();
            ActivationDrive::Continue(RuntimeSlotState::NativeActivation(Box::new(
                NativeActivationState {
                    initial_entry: entry,
                    request,
                    absence: None,
                    pending,
                    recovery,
                },
            )))
        }
        Err(refusal) => settle_host_binding_refusal(refusal),
    }
}

fn settle_host_binding_refusal(
    refusal: ServiceRuntimeHostActivationBindingRefusal,
) -> ActivationDrive {
    let reason = refusal.reason();
    match refusal.try_into_access_and_entry() {
        Ok((access, entry)) => match host_binding_disposition(reason) {
            HostBindingDisposition::Retry(reason) => activation_unavailable(
                RuntimeSlotState::MayOwnAccess(Box::new(MayOwnAccessState { access, entry })),
                reason,
            ),
            HostBindingDisposition::Reject(rejection) => release_prehost(
                entry,
                ReleaseAuthority::PreHost(Box::new(access)),
                rejection,
            ),
            HostBindingDisposition::Fail(reason) => ActivationDrive::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::MayOwnAccess(
                    Box::new(MayOwnAccessState { access, entry }),
                ))),
                reason,
            },
        },
        Err(refusal) => ActivationDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(
                RuntimeSlotState::HostBindingRefusal(Box::new(refusal)),
            )),
            reason: RuntimeCoordinatorFailureReason::HostInvariant,
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HostBindingDisposition {
    Retry(RuntimeActivationUnavailableReason),
    Reject(RuntimeActivationRejectionReason),
    Fail(RuntimeCoordinatorFailureReason),
}

pub(super) const fn host_binding_disposition(
    reason: BundledRuntimeHostActivationBindingError,
) -> HostBindingDisposition {
    match reason {
        BundledRuntimeHostActivationBindingError::RuntimeHostFactory(
            reason @ (ExtensionRuntimeHostBindError::Unavailable
            | ExtensionRuntimeHostBindError::CapacityExceeded),
        ) => HostBindingDisposition::Retry(RuntimeActivationUnavailableReason::HostUnavailable(
            reason,
        )),
        BundledRuntimeHostActivationBindingError::RuntimeHostFactory(
            ExtensionRuntimeHostBindError::UnsupportedBackend,
        ) => HostBindingDisposition::Reject(RuntimeActivationRejectionReason::HostUnsupported),
        BundledRuntimeHostActivationBindingError::RuntimeHostFactory(
            ExtensionRuntimeHostBindError::RetainedBytesExceeded,
        )
        | BundledRuntimeHostActivationBindingError::RetainedBytesExceeded => {
            HostBindingDisposition::Reject(RuntimeActivationRejectionReason::RetainedBytesExceeded)
        }
        BundledRuntimeHostActivationBindingError::RuntimeHostFactory(
            ExtensionRuntimeHostBindError::RetainedBytesOverflow,
        )
        | BundledRuntimeHostActivationBindingError::RetainedBytesOverflow => {
            HostBindingDisposition::Fail(RuntimeCoordinatorFailureReason::RetainedBytesOverflow)
        }
        BundledRuntimeHostActivationBindingError::RuntimeHost(
            ExtensionRuntimeHostActivationBindingError::RetainedBytesExceeded,
        ) => {
            HostBindingDisposition::Reject(RuntimeActivationRejectionReason::RetainedBytesExceeded)
        }
        BundledRuntimeHostActivationBindingError::RuntimeHost(
            ExtensionRuntimeHostActivationBindingError::RetainedBytesOverflow,
        ) => HostBindingDisposition::Fail(RuntimeCoordinatorFailureReason::RetainedBytesOverflow),
        BundledRuntimeHostActivationBindingError::RuntimeHostFactory(
            ExtensionRuntimeHostBindError::OwnerConflict
            | ExtensionRuntimeHostBindError::Sealed
            | ExtensionRuntimeHostBindError::IdentityExhausted
            | ExtensionRuntimeHostBindError::InternalInvariant,
        )
        | BundledRuntimeHostActivationBindingError::RepositoryBindingMismatch
        | BundledRuntimeHostActivationBindingError::RuntimeHost(_) => {
            HostBindingDisposition::Fail(RuntimeCoordinatorFailureReason::HostInvariant)
        }
        _ => HostBindingDisposition::Fail(RuntimeCoordinatorFailureReason::HostInvariant),
    }
}

fn settle_native_activation(state: NativeActivationState, deadline: Instant) -> ActivationDrive {
    let NativeActivationState {
        initial_entry,
        request,
        absence: prior_absence,
        pending,
        recovery,
    } = state;
    // Release builds use panic=abort, so a panicking platform adapter cannot
    // return control to this process. In unwind-enabled builds the moved
    // request may already have been dropped by unwinding and cannot be
    // truthfully reconstructed. We therefore retain every still-owned sibling
    // capability, keep the durable MayOwn row intact, and enter a permanent
    // attached-obligation fail-stop. No clean-shutdown evidence can be minted
    // from this state.
    let settlement = panic::catch_unwind(AssertUnwindSafe(|| request.settle_until(deadline)));
    let settlement = match settlement {
        Ok(settlement) => settlement,
        Err(_) => {
            return ActivationDrive::FailStop {
                retained: RuntimeFailStopRetained::NativeCallPanicked(Box::new(
                    NativeCallPanicAuthority {
                        pending,
                        recovery,
                        current_entry: initial_entry,
                    },
                )),
                reason: RuntimeCoordinatorFailureReason::NativeCallPanicked,
            };
        }
    };
    match settlement {
        ExtensionRuntimeActivationSettlement::Activated(owner) => {
            ActivationDrive::Continue(RuntimeSlotState::Owned(Box::new(OwnedRuntimeState {
                current_entry: initial_entry,
                owner,
                operation: RuntimeOperationControl::Pending(Box::new(pending)),
                recovery,
            })))
        }
        ExtensionRuntimeActivationSettlement::Retryable {
            request,
            failure,
            absence,
        } => activation_unavailable(
            RuntimeSlotState::NativeActivation(Box::new(NativeActivationState {
                initial_entry,
                request,
                absence: absence.or(prior_absence),
                pending,
                recovery,
            })),
            RuntimeActivationUnavailableReason::NativeRetryable(failure),
        ),
        ExtensionRuntimeActivationSettlement::Rejected {
            access,
            failure,
            absence,
        } => ActivationDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
            current_entry: initial_entry,
            authority: ReleaseAuthority::PostHost(Box::new(PostHostReleaseAuthority {
                access,
                operation: RuntimeOperationControl::Pending(Box::new(pending)),
                recovery,
                absence: super::slot::PostHostAbsenceBasis::Proven(absence),
            })),
            completion: ReleaseCompletion::ActivationRejected(
                RuntimeActivationRejectionReason::NativeRejected(failure),
            ),
        }))),
        ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, failure } => {
            activation_unavailable(
                RuntimeSlotState::NativeUncertain(Box::new(NativeUncertainState {
                    current_entry: initial_entry,
                    owner,
                    failure,
                    pending,
                    recovery,
                })),
                RuntimeActivationUnavailableReason::NativeOwnershipUncertain(failure),
            )
        }
    }
}

fn reconcile_native_uncertainty(state: NativeUncertainState, deadline: Instant) -> ActivationDrive {
    let NativeUncertainState {
        current_entry,
        owner,
        failure,
        pending,
        recovery,
    } = state;
    match panic::catch_unwind(AssertUnwindSafe(|| owner.reconcile_until(deadline))) {
        Ok(ExtensionRuntimeReconciliationSettlement::Owned(owner)) => {
            ActivationDrive::Continue(RuntimeSlotState::Owned(Box::new(OwnedRuntimeState {
                current_entry,
                owner,
                operation: RuntimeOperationControl::Pending(Box::new(pending)),
                recovery,
            })))
        }
        Ok(ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure }) => {
            activation_unavailable(
                RuntimeSlotState::NativeUncertain(Box::new(NativeUncertainState {
                    current_entry,
                    owner,
                    failure,
                    pending,
                    recovery,
                })),
                RuntimeActivationUnavailableReason::NativeOwnershipUncertain(failure),
            )
        }
        Ok(ExtensionRuntimeReconciliationSettlement::Absent { access, absence }) => {
            ActivationDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry,
                authority: ReleaseAuthority::PostHost(Box::new(PostHostReleaseAuthority {
                    access,
                    operation: RuntimeOperationControl::Pending(Box::new(pending)),
                    recovery,
                    absence: super::slot::PostHostAbsenceBasis::Proven(absence),
                })),
                completion: ReleaseCompletion::ActivationUnavailable(
                    RuntimeActivationUnavailableReason::NativeRetryable(failure),
                ),
            })))
        }
        Err(_) => ActivationDrive::FailStop {
            retained: RuntimeFailStopRetained::NativeCallPanicked(Box::new(
                NativeCallPanicAuthority {
                    pending,
                    recovery,
                    current_entry,
                },
            )),
            reason: RuntimeCoordinatorFailureReason::NativeCallPanicked,
        },
    }
}

fn manifest_binding_failure(error: BundledManifestBindingsError) -> ActivationDrive {
    match manifest_binding_disposition(&error) {
        ManifestBindingDisposition::Retry => activation_unavailable(
            RuntimeSlotState::Planning,
            RuntimeActivationUnavailableReason::RepositoryUnavailable,
        ),
        ManifestBindingDisposition::RetainedBytesExceeded => {
            activation_rejected(RuntimeActivationRejectionReason::RetainedBytesExceeded)
        }
        ManifestBindingDisposition::PackageUnavailable => {
            activation_rejected(RuntimeActivationRejectionReason::PackageUnavailable)
        }
        ManifestBindingDisposition::FailStop => activation_failed_without_authority(
            RuntimeCoordinatorFailureReason::RepositoryInvariant,
        ),
    }
}

fn runtime_planning_failure(
    refusal: crate::repository::ServiceRuntimePlanningRefusal,
) -> ActivationDrive {
    let disposition = runtime_planning_disposition(refusal.reason());
    drop(refusal.into_parts());
    match disposition {
        RuntimePlanningDisposition::Retry => activation_unavailable(
            RuntimeSlotState::Planning,
            RuntimeActivationUnavailableReason::RepositoryUnavailable,
        ),
        RuntimePlanningDisposition::RetainedBytesExceeded => {
            activation_rejected(RuntimeActivationRejectionReason::RetainedBytesExceeded)
        }
        RuntimePlanningDisposition::PackageUnavailable => {
            activation_rejected(RuntimeActivationRejectionReason::PackageUnavailable)
        }
        RuntimePlanningDisposition::FailStop(reason) => activation_failed_without_authority(reason),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RuntimePlanningDisposition {
    Retry,
    RetainedBytesExceeded,
    PackageUnavailable,
    FailStop(RuntimeCoordinatorFailureReason),
}

pub(super) const fn runtime_planning_disposition(
    error: &BundledPackageLeaseError,
) -> RuntimePlanningDisposition {
    match error {
        BundledPackageLeaseError::BuildInProgress
        | BundledPackageLeaseError::StaleSelection
        | BundledPackageLeaseError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::LockUnavailable | PrivateFsError::InUse | PrivateFsError::Io,
        )) => RuntimePlanningDisposition::Retry,
        BundledPackageLeaseError::CapacityExhausted => {
            RuntimePlanningDisposition::RetainedBytesExceeded
        }
        BundledPackageLeaseError::RetainedBytesOverflow => RuntimePlanningDisposition::FailStop(
            RuntimeCoordinatorFailureReason::RetainedBytesOverflow,
        ),
        BundledPackageLeaseError::NoCurrentSelection
        | BundledPackageLeaseError::PackageNotSelected
        | BundledPackageLeaseError::BrowsingContextUnsupported
        | BundledPackageLeaseError::CatalogAuthority(_)
        | BundledPackageLeaseError::CatalogAdmission(_)
        | BundledPackageLeaseError::ManifestAuthority(_)
        | BundledPackageLeaseError::ManifestAdmission(_)
        | BundledPackageLeaseError::PackageNotMaterialized => {
            RuntimePlanningDisposition::PackageUnavailable
        }
        BundledPackageLeaseError::Repository(_)
        | BundledPackageLeaseError::WrongCatalogRole
        | BundledPackageLeaseError::EligibilityMismatch
        | BundledPackageLeaseError::RuntimeBackendMismatch
        | BundledPackageLeaseError::OwnerConflict
        | BundledPackageLeaseError::LeaseAlreadyOpen
        | BundledPackageLeaseError::DurableObjectMismatch => RuntimePlanningDisposition::FailStop(
            RuntimeCoordinatorFailureReason::RepositoryInvariant,
        ),
        _ => RuntimePlanningDisposition::FailStop(
            RuntimeCoordinatorFailureReason::RepositoryInvariant,
        ),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ManifestBindingDisposition {
    Retry,
    RetainedBytesExceeded,
    PackageUnavailable,
    FailStop,
}

pub(super) const fn manifest_binding_disposition(
    error: &BundledManifestBindingsError,
) -> ManifestBindingDisposition {
    match error {
        BundledManifestBindingsError::BuildInProgress
        | BundledManifestBindingsError::StaleSelection
        | BundledManifestBindingsError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::LockUnavailable | PrivateFsError::InUse | PrivateFsError::Io,
        )) => ManifestBindingDisposition::Retry,
        BundledManifestBindingsError::CapacityExhausted => {
            ManifestBindingDisposition::RetainedBytesExceeded
        }
        BundledManifestBindingsError::NoCurrentSelection
        | BundledManifestBindingsError::PackageNotSelected
        | BundledManifestBindingsError::InstallPackageMismatch
        | BundledManifestBindingsError::CatalogAuthority(_)
        | BundledManifestBindingsError::CatalogAdmission(_)
        | BundledManifestBindingsError::ManifestAuthority(_)
        | BundledManifestBindingsError::ManifestAdmission(_) => {
            ManifestBindingDisposition::PackageUnavailable
        }
        BundledManifestBindingsError::Repository(_)
        | BundledManifestBindingsError::DurableObjectMismatch => {
            ManifestBindingDisposition::FailStop
        }
        _ => ManifestBindingDisposition::FailStop,
    }
}

fn begin_refusal_reason(reason: JournalActivationRefusalReason) -> ActivationDrive {
    match reason {
        JournalActivationRefusalReason::NotRegistered
        | JournalActivationRefusalReason::DegradedProfile => {
            activation_rejected(RuntimeActivationRejectionReason::ProfileUnavailable)
        }
        JournalActivationRefusalReason::Stale(_)
        | JournalActivationRefusalReason::EligibilityChanged(_) => {
            activation_rejected(RuntimeActivationRejectionReason::StoreCohortChanged)
        }
        JournalActivationRefusalReason::LimitReached => {
            activation_rejected(RuntimeActivationRejectionReason::RetainedBytesExceeded)
        }
        JournalActivationRefusalReason::SessionRecoveryRequired
        | JournalActivationRefusalReason::Invalid
        | JournalActivationRefusalReason::RevisionExhausted
        | JournalActivationRefusalReason::Failed => {
            activation_failed_without_authority(RuntimeCoordinatorFailureReason::JournalInvalid)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MayOwnRefusalDisposition {
    Rollback(RuntimeActivationRejectionReason),
    FailStop(RuntimeCoordinatorFailureReason),
}

pub(super) const fn may_own_refusal_disposition(
    reason: JournalActivationRefusalReason,
) -> MayOwnRefusalDisposition {
    match reason {
        JournalActivationRefusalReason::NotRegistered
        | JournalActivationRefusalReason::DegradedProfile => {
            MayOwnRefusalDisposition::Rollback(RuntimeActivationRejectionReason::ProfileUnavailable)
        }
        JournalActivationRefusalReason::Stale(_)
        | JournalActivationRefusalReason::EligibilityChanged(_) => {
            MayOwnRefusalDisposition::Rollback(RuntimeActivationRejectionReason::StoreCohortChanged)
        }
        JournalActivationRefusalReason::LimitReached => MayOwnRefusalDisposition::Rollback(
            RuntimeActivationRejectionReason::RetainedBytesExceeded,
        ),
        JournalActivationRefusalReason::SessionRecoveryRequired
        | JournalActivationRefusalReason::Invalid
        | JournalActivationRefusalReason::RevisionExhausted
        | JournalActivationRefusalReason::Failed => {
            MayOwnRefusalDisposition::FailStop(RuntimeCoordinatorFailureReason::JournalInvalid)
        }
    }
}

fn begin_preparation_failure(
    plan: crate::repository::ServiceRuntimeAcquisitionPlan,
    reason: JournalActivationPreparationFailureReason,
) -> ActivationDrive {
    match reason {
        JournalActivationPreparationFailureReason::ReloadRequired => activation_unavailable(
            RuntimeSlotState::Planned(Box::new(plan)),
            RuntimeActivationUnavailableReason::StoreObservationPending,
        ),
        JournalActivationPreparationFailureReason::RetainedBytesExceeded => {
            drop(plan);
            activation_rejected(RuntimeActivationRejectionReason::RetainedBytesExceeded)
        }
        JournalActivationPreparationFailureReason::RetainedBytesOverflow => {
            ActivationDrive::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Planned(
                    Box::new(plan),
                ))),
                reason: RuntimeCoordinatorFailureReason::RetainedBytesOverflow,
            }
        }
        JournalActivationPreparationFailureReason::CurrentRowMismatch => {
            ActivationDrive::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Planned(
                    Box::new(plan),
                ))),
                reason: RuntimeCoordinatorFailureReason::JournalDiverged,
            }
        }
        _ => ActivationDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Planned(
                Box::new(plan),
            ))),
            reason: RuntimeCoordinatorFailureReason::InternalProtocolViolation,
        },
    }
}

fn reload_plan_after_preparation_failure(
    resources: &mut RuntimeCoordinatorResources<'_>,
    plan: crate::repository::ServiceRuntimeAcquisitionPlan,
    deadline: Instant,
) -> ActivationDrive {
    match resources.projection.reload(resources.store, deadline) {
        Ok(_) => ActivationDrive::Continue(RuntimeSlotState::Planned(Box::new(plan))),
        Err(failure) => retain_plan_for_load_failure(plan, failure),
    }
}

fn reload_preparing_access_after_preparation_failure(
    resources: &mut RuntimeCoordinatorResources<'_>,
    access: crate::repository::ServiceRuntimePackageAccess,
    preparing: zephium_core::extensions::ExtensionNativeOwnershipEntry,
    deadline: Instant,
) -> ActivationDrive {
    let state =
        RuntimeSlotState::PreparingAccess(Box::new(PreparingAccessState { access, preparing }));
    match resources.projection.reload(resources.store, deadline) {
        Ok(_) => ActivationDrive::Continue(state),
        Err(failure) => retain_state_for_load_failure(state, failure),
    }
}

fn retain_plan_for_load_failure(
    plan: crate::repository::ServiceRuntimeAcquisitionPlan,
    failure: JournalLoadFailure,
) -> ActivationDrive {
    match failure {
        JournalLoadFailure::NotAdmitted => activation_unavailable(
            RuntimeSlotState::Planned(Box::new(plan)),
            RuntimeActivationUnavailableReason::StoreNotAdmitted,
        ),
        JournalLoadFailure::TimedOutAfterAdmission => activation_unavailable(
            RuntimeSlotState::Planned(Box::new(plan)),
            RuntimeActivationUnavailableReason::StoreObservationPending,
        ),
        JournalLoadFailure::Failed => ActivationDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Planned(
                Box::new(plan),
            ))),
            reason: RuntimeCoordinatorFailureReason::JournalInvalid,
        },
    }
}

fn retain_begin_conflict_for_load_failure(
    conflict: crate::journal_store::JournalActivationConflict<
        crate::repository::ServiceRuntimeAcquisitionPlan,
    >,
    failure: JournalLoadFailure,
) -> ActivationDrive {
    let state = RuntimeSlotState::BeginConflict(Box::new(conflict));
    retain_state_for_load_failure(state, failure)
}

fn retain_begin_ambiguity_for_load_failure(
    ambiguity: crate::journal_store::JournalActivationReloadRequired<
        crate::repository::ServiceRuntimeAcquisitionPlan,
    >,
    failure: JournalLoadFailure,
) -> ActivationDrive {
    let state = RuntimeSlotState::BeginAmbiguous(Box::new(ambiguity));
    retain_state_for_load_failure(state, failure)
}

fn retain_may_own_conflict_for_load_failure(
    preparing: zephium_core::extensions::ExtensionNativeOwnershipEntry,
    conflict: crate::journal_store::JournalActivationConflict<
        crate::repository::ServiceRuntimePackageAccess,
    >,
    failure: JournalLoadFailure,
) -> ActivationDrive {
    let state = RuntimeSlotState::MayOwnConflict(Box::new(MayOwnConflictState {
        preparing,
        conflict,
    }));
    retain_state_for_load_failure(state, failure)
}

fn retain_may_own_ambiguity_for_load_failure(
    ambiguity: crate::journal_store::JournalActivationReloadRequired<
        crate::repository::ServiceRuntimePackageAccess,
    >,
    failure: JournalLoadFailure,
) -> ActivationDrive {
    let state = RuntimeSlotState::MayOwnAmbiguous(Box::new(ambiguity));
    retain_state_for_load_failure(state, failure)
}

fn retain_state_for_load_failure(
    state: RuntimeSlotState,
    failure: JournalLoadFailure,
) -> ActivationDrive {
    match failure {
        JournalLoadFailure::NotAdmitted => {
            activation_unavailable(state, RuntimeActivationUnavailableReason::StoreNotAdmitted)
        }
        JournalLoadFailure::TimedOutAfterAdmission => activation_unavailable(
            state,
            RuntimeActivationUnavailableReason::StoreObservationPending,
        ),
        JournalLoadFailure::Failed => ActivationDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(state)),
            reason: RuntimeCoordinatorFailureReason::JournalInvalid,
        },
    }
}

fn activation_unavailable(
    state: RuntimeSlotState,
    reason: RuntimeActivationUnavailableReason,
) -> ActivationDrive {
    ActivationDrive::Stop {
        state: Some(state),
        outcome: RuntimeActivationOutcome::Unavailable(reason),
    }
}

fn activation_rejected(reason: RuntimeActivationRejectionReason) -> ActivationDrive {
    ActivationDrive::Stop {
        state: None,
        outcome: RuntimeActivationOutcome::Rejected(reason),
    }
}

fn activation_failed_without_authority(reason: RuntimeCoordinatorFailureReason) -> ActivationDrive {
    ActivationDrive::Stop {
        state: None,
        outcome: RuntimeActivationOutcome::FailedClosed(reason),
    }
}

fn release_prehost(
    current_entry: zephium_core::extensions::ExtensionNativeOwnershipEntry,
    authority: ReleaseAuthority,
    rejection: RuntimeActivationRejectionReason,
) -> ActivationDrive {
    ActivationDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
        current_entry,
        authority,
        completion: ReleaseCompletion::ActivationRejected(rejection),
    })))
}
