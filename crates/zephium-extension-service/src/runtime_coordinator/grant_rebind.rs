//! Live grant-authority replacement without native-owner recreation.

use std::time::Instant;

use zephium_core::extensions::{
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPhase,
    ExtensionRuntimeEligibility, ExtensionRuntimeGeneration,
};

use super::outcome::{
    RuntimeCoordinatorFailureReason, RuntimeGrantRebindOutcome, RuntimeGrantRebindUnavailableReason,
};
use super::reconciliation::{
    reconcile_row_ambiguity, reconcile_row_conflict, settle_row_fence, RowFenceProgress,
    RowFenceWait,
};
use super::slot::{
    RuntimeFailStopRetained, RuntimeOperationControl, RuntimeRowFenceAuthority,
    RuntimeRowFenceMutation, RuntimeRowFenceOperation, RuntimeSlotState,
};
use super::{RuntimeCoordinator, RuntimeCoordinatorResources};

impl RuntimeCoordinator {
    /// Finishes any earlier ambiguous grant rebind before a new permission
    /// request is evaluated. `NoPending` means the caller may proceed with a
    /// fresh Store patch; every other result is a complete settlement.
    pub(crate) fn reconcile_pending_grant_rebind_until(
        &mut self,
        mut resources: RuntimeCoordinatorResources<'_>,
        key: ExtensionNativeOwnershipKey,
        generation: ExtensionRuntimeGeneration,
        deadline: Instant,
    ) -> RuntimeGrantRebindOutcome {
        if let Some(reason) = self.fail_stop {
            return RuntimeGrantRebindOutcome::FailedClosed(reason);
        }
        let Some(slot) = self.slot(key) else {
            return RuntimeGrantRebindOutcome::Conflict;
        };
        if slot.generation() != Some(generation) {
            return RuntimeGrantRebindOutcome::Conflict;
        }
        let Some(state) = self.slot_mut(key).and_then(|slot| slot.take_state()) else {
            return self
                .fail_without_state(RuntimeCoordinatorFailureReason::InternalProtocolViolation);
        };
        let is_pending = match &state {
            RuntimeSlotState::GrantRebindPending(operation) => operation.is_live_grant_rebind(),
            RuntimeSlotState::RowFenceConflict(state) => state.operation.is_live_grant_rebind(),
            RuntimeSlotState::RowFenceAmbiguous(state) => state.operation.is_live_grant_rebind(),
            _ => false,
        };
        if !is_pending {
            if !self
                .slot_mut(key)
                .is_some_and(|slot| slot.install_state(state))
            {
                return self.fail_without_state(
                    RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                );
            }
            return RuntimeGrantRebindOutcome::NoPending;
        }
        if Instant::now() >= deadline {
            if !self
                .slot_mut(key)
                .is_some_and(|slot| slot.install_state(state))
            {
                return self.fail_without_state(
                    RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                );
            }
            return RuntimeGrantRebindOutcome::Unavailable(
                RuntimeGrantRebindUnavailableReason::DeadlineReached,
            );
        }
        let progress = match state {
            RuntimeSlotState::GrantRebindPending(operation) if operation.is_live_grant_rebind() => {
                settle_row_fence(&mut resources, *operation, deadline)
            }
            RuntimeSlotState::RowFenceConflict(state) if state.operation.is_live_grant_rebind() => {
                reconcile_row_conflict(&mut resources, *state, deadline)
            }
            RuntimeSlotState::RowFenceAmbiguous(state)
                if state.operation.is_live_grant_rebind() =>
            {
                reconcile_row_ambiguity(&mut resources, *state, deadline)
            }
            other => {
                return self.fail_with_state(
                    key,
                    other,
                    RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                )
            }
        };
        self.finish_grant_rebind_progress(key, progress)
    }

    /// Rebinds one exact live runtime after the profile grant patch committed.
    ///
    /// The row fence first commits and reconciles the Store-validated durable
    /// journal edge. Only its exact `Applied` frontier may invoke the engine
    /// receipt, ensuring in-memory authority is never broader than durable
    /// profile authority. The native owner and process generation are kept.
    pub(crate) fn rebind_live_grants_until(
        &mut self,
        mut resources: RuntimeCoordinatorResources<'_>,
        key: ExtensionNativeOwnershipKey,
        generation: ExtensionRuntimeGeneration,
        eligibility: ExtensionRuntimeEligibility,
        deadline: Instant,
    ) -> RuntimeGrantRebindOutcome {
        if let Some(reason) = self.fail_stop {
            return RuntimeGrantRebindOutcome::FailedClosed(reason);
        }
        if Instant::now() >= deadline {
            return RuntimeGrantRebindOutcome::Unavailable(
                RuntimeGrantRebindUnavailableReason::DeadlineReached,
            );
        }
        if self.journal_settlement_barrier_blocks(key) {
            return RuntimeGrantRebindOutcome::Unavailable(
                RuntimeGrantRebindUnavailableReason::StoreObservationPending,
            );
        }
        let Some(slot) = self.slot(key) else {
            return RuntimeGrantRebindOutcome::Conflict;
        };
        if slot.generation() != Some(generation) || !slot.is_live() {
            return RuntimeGrantRebindOutcome::Conflict;
        }
        let Some(state) = self.slot_mut(key).and_then(|slot| slot.take_state()) else {
            return self
                .fail_without_state(RuntimeCoordinatorFailureReason::InternalProtocolViolation);
        };
        let RuntimeSlotState::Owned(owned) = state else {
            let installed = self
                .slot_mut(key)
                .is_some_and(|slot| slot.install_state(state));
            if !installed {
                return self.fail_without_state(
                    RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                );
            }
            return RuntimeGrantRebindOutcome::Conflict;
        };
        if !valid_rebind_frontier(&owned, key, &eligibility) {
            return self.fail_with_state(
                key,
                RuntimeSlotState::Owned(owned),
                RuntimeCoordinatorFailureReason::JournalDiverged,
            );
        }

        let progress = settle_row_fence(
            &mut resources,
            RuntimeRowFenceOperation {
                mutation: RuntimeRowFenceMutation::RebindGrants(Box::new(eligibility)),
                authority: RuntimeRowFenceAuthority::Owned(owned),
            },
            deadline,
        );
        self.finish_grant_rebind_progress(key, progress)
    }

    fn finish_grant_rebind_progress(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        progress: RowFenceProgress,
    ) -> RuntimeGrantRebindOutcome {
        match progress {
            RowFenceProgress::Continue(RuntimeSlotState::Owned(state)) => {
                let revision = state.current_entry.store_grant_revision();
                if !matches!(state.operation, RuntimeOperationControl::Published(_))
                    || !self
                        .slot_mut(key)
                        .is_some_and(|slot| slot.install_state(RuntimeSlotState::Owned(state)))
                {
                    return self.fail_without_state(
                        RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                    );
                }
                RuntimeGrantRebindOutcome::Rebound(revision)
            }
            RowFenceProgress::Continue(state) => self.fail_with_state(
                key,
                state,
                RuntimeCoordinatorFailureReason::InternalProtocolViolation,
            ),
            RowFenceProgress::Wait { state, reason } => {
                if !self
                    .slot_mut(key)
                    .is_some_and(|slot| slot.install_state(state))
                {
                    return self.fail_without_state(
                        RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                    );
                }
                RuntimeGrantRebindOutcome::Unavailable(match reason {
                    RowFenceWait::StoreNotAdmitted => {
                        RuntimeGrantRebindUnavailableReason::StoreNotAdmitted
                    }
                    RowFenceWait::StoreObservationPending => {
                        RuntimeGrantRebindUnavailableReason::StoreObservationPending
                    }
                })
            }
            RowFenceProgress::FailStop { retained, reason } => {
                let installed = self
                    .slot_mut(key)
                    .is_some_and(|slot| slot.enter_fail_stop(reason, retained));
                self.enter_fail_stop(reason);
                if installed {
                    RuntimeGrantRebindOutcome::FailedClosed(reason)
                } else {
                    RuntimeGrantRebindOutcome::FailedClosed(
                        RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                    )
                }
            }
            RowFenceProgress::Cleared(_) => {
                self.fail_without_state(RuntimeCoordinatorFailureReason::InternalProtocolViolation)
            }
        }
    }

    fn fail_with_state(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        state: RuntimeSlotState,
        reason: RuntimeCoordinatorFailureReason,
    ) -> RuntimeGrantRebindOutcome {
        let installed = self.slot_mut(key).is_some_and(|slot| {
            slot.enter_fail_stop(reason, RuntimeFailStopRetained::Prior(Box::new(state)))
        });
        self.enter_fail_stop(reason);
        if installed {
            RuntimeGrantRebindOutcome::FailedClosed(reason)
        } else {
            RuntimeGrantRebindOutcome::FailedClosed(
                RuntimeCoordinatorFailureReason::InternalProtocolViolation,
            )
        }
    }

    fn fail_without_state(
        &mut self,
        reason: RuntimeCoordinatorFailureReason,
    ) -> RuntimeGrantRebindOutcome {
        self.enter_fail_stop(reason);
        RuntimeGrantRebindOutcome::FailedClosed(reason)
    }
}

fn valid_rebind_frontier(
    state: &super::slot::OwnedRuntimeState,
    key: ExtensionNativeOwnershipKey,
    eligibility: &ExtensionRuntimeEligibility,
) -> bool {
    let entry = &state.current_entry;
    entry.key() == key
        && entry.intent() == ExtensionNativeOwnershipIntent::Acquire
        && entry.phase() == ExtensionNativeOwnershipPhase::NativeOwned
        && matches!(state.operation, RuntimeOperationControl::Published(_))
        && eligibility.profile() == key.profile()
        && eligibility.install_id() == key.install_id()
        && eligibility.browsing_context() == key.browsing_context()
        && eligibility.catalog_revision() == entry.store_catalog_revision()
        && eligibility.install_revision() == entry.store_install_revision()
        && eligibility.package() == entry.package()
        && eligibility.grant_revision() > entry.store_grant_revision()
}
