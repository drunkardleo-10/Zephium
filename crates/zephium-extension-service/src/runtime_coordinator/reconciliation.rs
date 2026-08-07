//! Exact durable-row mutation and ambiguity reconciliation.

use std::mem::size_of;
use std::time::Instant;

use zephium_core::extensions::ExtensionNativeOwnershipEntry;

use crate::journal_store::{
    JournalLoadFailure, RowFenceConflictInspection, RowFencePreparationFailureReason,
    RowFenceReconciliation, RowFenceRefusalReason, RowFenceSettlement,
};

use super::outcome::RuntimeCoordinatorFailureReason;
use super::slot::{
    OwnedRuntimeState, PostHostReleaseAuthority, ReleaseAuthority, ReleaseCompletion,
    ReleaseRuntimeState, RuntimeFailStopRetained, RuntimeOperationControl,
    RuntimeRowFenceAmbiguousState, RuntimeRowFenceAuthority, RuntimeRowFenceConflictState,
    RuntimeRowFenceDivergedAuthority, RuntimeRowFenceMutation, RuntimeRowFenceOperation,
    RuntimeSlot, RuntimeSlotState,
};
use super::{
    reload_projection_if_unknown, run_after_projection_readiness, RuntimeCoordinatorResources,
};

const HEAP_ALLOCATION_OVERHEAD_BYTES: usize = 2 * size_of::<usize>();

pub(super) enum RowFenceWait {
    StoreNotAdmitted,
    StoreObservationPending,
}

pub(super) enum RowFenceProgress {
    Continue(RuntimeSlotState),
    Wait {
        state: RuntimeSlotState,
        reason: RowFenceWait,
    },
    Cleared(ReleaseCompletion),
    FailStop {
        retained: RuntimeFailStopRetained,
        reason: RuntimeCoordinatorFailureReason,
    },
}

impl RuntimeRowFenceAuthority {
    fn current_entry(&self) -> &ExtensionNativeOwnershipEntry {
        match self {
            Self::Owned(state) => &state.current_entry,
            Self::Release(state) => &state.current_entry,
        }
    }

    fn into_state(self) -> RuntimeSlotState {
        match self {
            Self::Owned(state) => RuntimeSlotState::Owned(state),
            Self::Release(state) => RuntimeSlotState::Release(state),
        }
    }

    fn install_entry(&mut self, entry: ExtensionNativeOwnershipEntry) {
        match self {
            Self::Owned(state) => state.current_entry = entry,
            Self::Release(state) => state.current_entry = entry,
        }
    }

    fn caller_retained_bytes(&self) -> Option<usize> {
        let authority = match self {
            Self::Owned(state) => owned_retained_bytes(state),
            Self::Release(state) => release_retained_bytes(state),
        }?;
        authority
            .checked_add(size_of::<RuntimeSlot>())?
            .checked_add(size_of::<RuntimeRowFenceOperation>())?
            .checked_add(2 * HEAP_ALLOCATION_OVERHEAD_BYTES)
    }
}

fn owned_retained_bytes(state: &OwnedRuntimeState) -> Option<usize> {
    state
        .owner
        .retained_bytes()
        .checked_add(operation_retained_bytes(&state.operation))?
        .checked_add(state.recovery.retained_bytes())?
        // Conservatively include the complete wrapper. Its authority fields
        // are intentionally double-counted; host admission reserved this same
        // stable companion charge before any native call was possible.
        .checked_add(size_of::<OwnedRuntimeState>())
}

fn operation_retained_bytes(operation: &RuntimeOperationControl) -> usize {
    match operation {
        RuntimeOperationControl::Pending(pending) => pending.retained_bytes(),
        RuntimeOperationControl::PublicationRequest(request) => request.retained_bytes(),
        RuntimeOperationControl::Published(receipt) => receipt.retained_bytes(),
    }
}

fn release_retained_bytes(state: &ReleaseRuntimeState) -> Option<usize> {
    let authority = match &state.authority {
        ReleaseAuthority::NoRepositoryPin(plan) => plan.as_deref().map_or(
            0,
            crate::repository::ServiceRuntimeAcquisitionPlan::retained_bytes,
        ),
        ReleaseAuthority::Lease(lease) => lease.retained_bytes(),
        ReleaseAuthority::PreHost(access) => access.retained_bytes(),
        ReleaseAuthority::PreHostRefusal(_) => return None,
        ReleaseAuthority::PostHost(post_host) => post_host_retained_bytes(post_host)?,
        ReleaseAuthority::Repository(release) => release.retained_bytes(),
        // Both refusal states can exist only after the row has already reached
        // AbsentReleasePending. They must first recover their underlying
        // accounted facade and therefore never enter a row fence directly.
        ReleaseAuthority::RejoinRefusal(_) => return None,
    };
    authority
        .checked_add(size_of::<ReleaseRuntimeState>())
        .and_then(|bytes| bytes.checked_add(HEAP_ALLOCATION_OVERHEAD_BYTES))
}

fn post_host_retained_bytes(state: &PostHostReleaseAuthority) -> Option<usize> {
    state
        .access
        .retained_bytes()
        .checked_add(operation_retained_bytes(&state.operation))?
        .checked_add(state.recovery.retained_bytes())?
        .checked_add(size_of::<PostHostReleaseAuthority>())
}

pub(super) fn settle_row_fence(
    resources: &mut RuntimeCoordinatorResources<'_>,
    operation: RuntimeRowFenceOperation,
    deadline: Instant,
) -> RowFenceProgress {
    let readiness = reload_projection_if_unknown(resources.projection, resources.store, deadline);
    let mut retained_operation = Some(operation);
    match run_after_projection_readiness(readiness, || {
        settle_known_row_fence(
            resources,
            retained_operation
                .take()
                .expect("projection gate invokes row-fence settlement at most once"),
            deadline,
        )
    }) {
        Ok(progress) => progress,
        Err(failure) => retain_row_state_for_load_failure(
            retained_operation
                .take()
                .expect("failed projection gate retains exact row-fence authority")
                .authority
                .into_state(),
            failure,
        ),
    }
}

fn settle_known_row_fence(
    resources: &mut RuntimeCoordinatorResources<'_>,
    operation: RuntimeRowFenceOperation,
    deadline: Instant,
) -> RowFenceProgress {
    let Some(caller_retained_bytes) = operation.authority.caller_retained_bytes() else {
        return fail_stop_authority(
            operation.authority,
            RuntimeCoordinatorFailureReason::RetainedBytesOverflow,
        );
    };
    let settlement = match operation.mutation {
        RuntimeRowFenceMutation::Transition { intent, phase } => {
            resources.projection.settle_exact_row_transition(
                resources.store,
                operation.authority.current_entry(),
                intent,
                phase,
                caller_retained_bytes,
                deadline,
            )
        }
        RuntimeRowFenceMutation::AttachNativeIdentity(identity) => {
            resources.projection.settle_native_identity_attachment(
                resources.store,
                operation.authority.current_entry(),
                identity,
                caller_retained_bytes,
                deadline,
            )
        }
        RuntimeRowFenceMutation::Clear => resources.projection.settle_exact_row_clear(
            resources.store,
            operation.authority.current_entry(),
            caller_retained_bytes,
            deadline,
        ),
    };
    let settlement = match settlement {
        Ok(settlement) => settlement,
        Err(failure) => {
            let reason = match failure.reason() {
                RowFencePreparationFailureReason::RetainedBytesOverflow => {
                    RuntimeCoordinatorFailureReason::RetainedBytesOverflow
                }
                RowFencePreparationFailureReason::ReloadRequired => {
                    return match resources.projection.reload(resources.store, deadline) {
                        Ok(_) => RowFenceProgress::Continue(operation.authority.into_state()),
                        Err(failure) => retain_row_state_for_load_failure(
                            operation.authority.into_state(),
                            failure,
                        ),
                    };
                }
                RowFencePreparationFailureReason::CurrentRowMismatch => {
                    RuntimeCoordinatorFailureReason::JournalDiverged
                }
                RowFencePreparationFailureReason::RetainedBytesExceeded
                | RowFencePreparationFailureReason::ForbiddenLifecycleEdge
                | RowFencePreparationFailureReason::InvalidLocalTransition => {
                    RuntimeCoordinatorFailureReason::InternalProtocolViolation
                }
            };
            return fail_stop_authority(operation.authority, reason);
        }
    };
    match settlement {
        RowFenceSettlement::Applied(applied) => finish_applied(operation, applied.entry().cloned()),
        RowFenceSettlement::NotAdmitted => RowFenceProgress::Wait {
            state: operation.authority.into_state(),
            reason: RowFenceWait::StoreNotAdmitted,
        },
        RowFenceSettlement::Refused(refusal) => {
            let reason = match refusal.reason() {
                RowFenceRefusalReason::NotRegistered
                | RowFenceRefusalReason::DegradedProfile
                | RowFenceRefusalReason::SessionRecoveryRequired
                | RowFenceRefusalReason::Invalid
                | RowFenceRefusalReason::LimitReached
                | RowFenceRefusalReason::RevisionExhausted
                | RowFenceRefusalReason::Failed => RuntimeCoordinatorFailureReason::JournalInvalid,
            };
            fail_stop_authority(operation.authority, reason)
        }
        RowFenceSettlement::Conflict(conflict) => RowFenceProgress::Wait {
            state: RuntimeSlotState::RowFenceConflict(Box::new(RuntimeRowFenceConflictState {
                conflict,
                operation,
            })),
            reason: RowFenceWait::StoreObservationPending,
        },
        RowFenceSettlement::ReloadRequired(ambiguity) => RowFenceProgress::Wait {
            state: RuntimeSlotState::RowFenceAmbiguous(Box::new(RuntimeRowFenceAmbiguousState {
                ambiguity,
                operation,
            })),
            reason: RowFenceWait::StoreObservationPending,
        },
    }
}

pub(super) fn reconcile_row_conflict(
    resources: &mut RuntimeCoordinatorResources<'_>,
    state: RuntimeRowFenceConflictState,
    deadline: Instant,
) -> RowFenceProgress {
    let RuntimeRowFenceConflictState {
        conflict,
        operation,
    } = state;
    if let Err(failure) = resources.projection.reload(resources.store, deadline) {
        return retain_conflict_for_load_failure(conflict, operation, failure);
    }
    match conflict.inspect(resources.projection) {
        RowFenceConflictInspection::Reloaded {
            reported_revision_matches: true,
            expected_before_matches: true,
            ..
        } => settle_row_fence(resources, operation, deadline),
        RowFenceConflictInspection::Pending | RowFenceConflictInspection::Reloaded { .. } => {
            RowFenceProgress::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(
                    RuntimeSlotState::RowFenceConflict(Box::new(RuntimeRowFenceConflictState {
                        conflict,
                        operation,
                    })),
                )),
                reason: RuntimeCoordinatorFailureReason::JournalDiverged,
            }
        }
    }
}

pub(super) fn reconcile_row_ambiguity(
    resources: &mut RuntimeCoordinatorResources<'_>,
    state: RuntimeRowFenceAmbiguousState,
    deadline: Instant,
) -> RowFenceProgress {
    let RuntimeRowFenceAmbiguousState {
        ambiguity,
        operation,
    } = state;
    if let Err(failure) = resources.projection.reload(resources.store, deadline) {
        return retain_ambiguity_for_load_failure(ambiguity, operation, failure);
    }
    match ambiguity.reconcile(resources.projection) {
        RowFenceReconciliation::Applied(applied) => {
            finish_applied(operation, applied.entry().cloned())
        }
        RowFenceReconciliation::NotApplied(not_applied) => {
            if not_applied.entry() != operation.authority.current_entry() {
                return fail_stop_authority(
                    operation.authority,
                    RuntimeCoordinatorFailureReason::JournalDiverged,
                );
            }
            settle_row_fence(resources, operation, deadline)
        }
        RowFenceReconciliation::Pending(ambiguity) => RowFenceProgress::Wait {
            state: RuntimeSlotState::RowFenceAmbiguous(Box::new(RuntimeRowFenceAmbiguousState {
                ambiguity,
                operation,
            })),
            reason: RowFenceWait::StoreObservationPending,
        },
        RowFenceReconciliation::Diverged(diverged) => RowFenceProgress::FailStop {
            retained: RuntimeFailStopRetained::RowFenceDiverged(Box::new(
                RuntimeRowFenceDivergedAuthority {
                    diverged,
                    operation,
                },
            )),
            reason: RuntimeCoordinatorFailureReason::JournalDiverged,
        },
    }
}

fn finish_applied(
    mut operation: RuntimeRowFenceOperation,
    applied_entry: Option<ExtensionNativeOwnershipEntry>,
) -> RowFenceProgress {
    match (operation.mutation, applied_entry) {
        (RuntimeRowFenceMutation::Clear, None) => match operation.authority {
            RuntimeRowFenceAuthority::Release(state) => RowFenceProgress::Cleared(state.completion),
            RuntimeRowFenceAuthority::Owned(state) => fail_stop_authority(
                RuntimeRowFenceAuthority::Owned(state),
                RuntimeCoordinatorFailureReason::InternalProtocolViolation,
            ),
        },
        (RuntimeRowFenceMutation::Transition { .. }, Some(entry))
        | (RuntimeRowFenceMutation::AttachNativeIdentity(_), Some(entry)) => {
            operation.authority.install_entry(entry);
            RowFenceProgress::Continue(operation.authority.into_state())
        }
        (_, _) => fail_stop_authority(
            operation.authority,
            RuntimeCoordinatorFailureReason::JournalDiverged,
        ),
    }
}

fn retain_conflict_for_load_failure(
    conflict: crate::journal_store::RowFenceConflict,
    operation: RuntimeRowFenceOperation,
    failure: JournalLoadFailure,
) -> RowFenceProgress {
    retain_row_state_for_load_failure(
        RuntimeSlotState::RowFenceConflict(Box::new(RuntimeRowFenceConflictState {
            conflict,
            operation,
        })),
        failure,
    )
}

fn retain_ambiguity_for_load_failure(
    ambiguity: crate::journal_store::RowFenceReloadRequired,
    operation: RuntimeRowFenceOperation,
    failure: JournalLoadFailure,
) -> RowFenceProgress {
    retain_row_state_for_load_failure(
        RuntimeSlotState::RowFenceAmbiguous(Box::new(RuntimeRowFenceAmbiguousState {
            ambiguity,
            operation,
        })),
        failure,
    )
}

fn retain_row_state_for_load_failure(
    state: RuntimeSlotState,
    failure: JournalLoadFailure,
) -> RowFenceProgress {
    match failure {
        JournalLoadFailure::NotAdmitted => RowFenceProgress::Wait {
            state,
            reason: RowFenceWait::StoreNotAdmitted,
        },
        JournalLoadFailure::TimedOutAfterAdmission => RowFenceProgress::Wait {
            state,
            reason: RowFenceWait::StoreObservationPending,
        },
        JournalLoadFailure::Failed => RowFenceProgress::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(state)),
            reason: RuntimeCoordinatorFailureReason::JournalInvalid,
        },
    }
}

fn fail_stop_authority(
    authority: RuntimeRowFenceAuthority,
    reason: RuntimeCoordinatorFailureReason,
) -> RowFenceProgress {
    RowFenceProgress::FailStop {
        retained: RuntimeFailStopRetained::Prior(Box::new(authority.into_state())),
        reason,
    }
}
