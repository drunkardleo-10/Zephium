//! Runtime retirement, authority recombination, and exact pin release.

use std::panic::{self, AssertUnwindSafe};
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPhase,
    ExtensionPackagePinReleaseBinding,
};
use zephium_core::ids::ProfileId;
use zephium_extension_repository::{
    BundledPackageLeaseReleaseError, BundledPackageLeaseReleaseOutcome, ExtensionRepositoryError,
};
use zephium_extension_runtime_api::{
    ExtensionRuntimeHostBindError, ExtensionRuntimePublicationReclaimError,
    ExtensionRuntimeReconciliationSettlement, ExtensionRuntimeRetirementSettlement,
};
use zephium_private_fs::PrivateFsError;

use crate::journal_store::{JournalActivationReconciliation, JournalLoadFailure};

use super::outcome::{
    RuntimeCoordinatorFailureReason, RuntimeDrainOutcome, RuntimeRetirementOutcome,
    RuntimeRetirementUnavailableReason,
};
use super::reconciliation::{
    reconcile_row_ambiguity, reconcile_row_conflict, settle_row_fence, RowFenceProgress,
    RowFenceWait,
};
use super::slot::{
    MayOwnAccessState, NativeRetirementCallPanicAuthority, NativeRetirementState,
    NativeRetirementUncertainState, PostHostAbsenceBasis, PostHostReleaseAuthority,
    PreparingAccessState, ReleaseAuthority, ReleaseCompletion, ReleaseRuntimeState,
    RuntimeFailStopRetained, RuntimeOperationControl, RuntimeRowFenceAuthority,
    RuntimeRowFenceMutation, RuntimeRowFenceOperation, RuntimeSlotState,
};
use super::{RuntimeCoordinator, RuntimeCoordinatorResources};

const MAX_RETIREMENT_STATE_TRANSITIONS_PER_CALL: usize = 48;

pub(super) enum RetirementDrive {
    Continue(RuntimeSlotState),
    Stop {
        state: RuntimeSlotState,
        reason: RuntimeRetirementUnavailableReason,
    },
    Complete(ReleaseCompletion),
    FailStop {
        retained: RuntimeFailStopRetained,
        reason: RuntimeCoordinatorFailureReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReleaseAuthorityClass {
    NoRepositoryPin,
    Lease,
    PreHost,
    PreHostRefusal,
    PostHost,
    RejoinRefusal,
    Repository,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReleaseAction {
    FenceDefiniteAbsence,
    ClearDurableRow,
    ConvertLease,
    RecoverPreHost,
    QuarantinePreHostRecoveryRefusal,
    RecoverPostHost,
    QuarantinePostHostRejoinRefusal,
    SettleRepositoryPin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RepositoryReleaseErrorDisposition {
    Retry,
    FailStop,
}

pub(super) const fn repository_release_error_disposition(
    error: BundledPackageLeaseReleaseError,
) -> RepositoryReleaseErrorDisposition {
    match error {
        BundledPackageLeaseReleaseError::BuildInProgress
        | BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::LockUnavailable | PrivateFsError::InUse | PrivateFsError::Io,
        )) => RepositoryReleaseErrorDisposition::Retry,
        // Same-open release authority cannot heal identity, concurrency,
        // corruption, ambiguous-settlement, sealed-repository, callback, or
        // platform failures. Unknown future variants are terminal by default.
        _ => RepositoryReleaseErrorDisposition::FailStop,
    }
}

impl ReleaseAuthority {
    const fn class(&self) -> ReleaseAuthorityClass {
        match self {
            Self::NoRepositoryPin(_) => ReleaseAuthorityClass::NoRepositoryPin,
            Self::Lease(_) => ReleaseAuthorityClass::Lease,
            Self::PreHost(_) => ReleaseAuthorityClass::PreHost,
            Self::PreHostRefusal(_) => ReleaseAuthorityClass::PreHostRefusal,
            Self::PostHost(_) => ReleaseAuthorityClass::PostHost,
            Self::RejoinRefusal(_) => ReleaseAuthorityClass::RejoinRefusal,
            Self::Repository(_) => ReleaseAuthorityClass::Repository,
        }
    }
}

pub(super) fn release_action(
    intent: ExtensionNativeOwnershipIntent,
    phase: ExtensionNativeOwnershipPhase,
    authority: ReleaseAuthorityClass,
) -> Result<ReleaseAction, RuntimeCoordinatorFailureReason> {
    if intent != ExtensionNativeOwnershipIntent::Release
        || phase != ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
    {
        return match (intent, phase, authority) {
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeAbsentPreparing
                | ExtensionNativeOwnershipPhase::NativeMayOwn,
                ReleaseAuthorityClass::NoRepositoryPin
                | ReleaseAuthorityClass::Lease
                | ReleaseAuthorityClass::PreHost
                | ReleaseAuthorityClass::PostHost,
            )
            | (
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
                ReleaseAuthorityClass::PostHost,
            ) => Ok(ReleaseAction::FenceDefiniteAbsence),
            _ => Err(RuntimeCoordinatorFailureReason::InternalProtocolViolation),
        };
    }
    Ok(match authority {
        ReleaseAuthorityClass::NoRepositoryPin => ReleaseAction::ClearDurableRow,
        ReleaseAuthorityClass::Lease => ReleaseAction::ConvertLease,
        ReleaseAuthorityClass::PreHost => ReleaseAction::RecoverPreHost,
        ReleaseAuthorityClass::PreHostRefusal => ReleaseAction::QuarantinePreHostRecoveryRefusal,
        ReleaseAuthorityClass::PostHost => ReleaseAction::RecoverPostHost,
        ReleaseAuthorityClass::RejoinRefusal => ReleaseAction::QuarantinePostHostRejoinRefusal,
        ReleaseAuthorityClass::Repository => ReleaseAction::SettleRepositoryPin,
    })
}

impl RuntimeCoordinator {
    pub(crate) fn retire_key_until(
        &mut self,
        mut resources: RuntimeCoordinatorResources<'_>,
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
    ) -> RuntimeRetirementOutcome {
        self.activation_issues.clear(key);
        self.retire_key_with_resources(&mut resources, key, deadline)
    }

    pub(crate) fn retire_profile_until(
        &mut self,
        mut resources: RuntimeCoordinatorResources<'_>,
        profile: ProfileId,
        deadline: Instant,
    ) -> RuntimeRetirementOutcome {
        self.activation_issues.clear_profile(profile);
        if let Some(outcome) = self.profile_retirement_preflight(profile) {
            return outcome;
        }
        let keys = self.keys_matching(Some(profile));
        for key in keys.into_iter().flatten() {
            match self.retire_key_with_resources(&mut resources, key, deadline) {
                RuntimeRetirementOutcome::Retired | RuntimeRetirementOutcome::NotPresent => {}
                pending => return pending,
            }
        }
        RuntimeRetirementOutcome::Retired
    }

    /// Global fail-stop has precedence over the absence of profile-local
    /// slots. Reporting `NotPresent` must never hide a process-wide invariant
    /// failure from a caller draining an otherwise empty profile.
    pub(super) fn profile_retirement_preflight(
        &self,
        profile: ProfileId,
    ) -> Option<RuntimeRetirementOutcome> {
        if let Some(reason) = self.fail_stop {
            Some(RuntimeRetirementOutcome::FailedClosed(reason))
        } else if !self.has_profile_obligation(profile) {
            Some(RuntimeRetirementOutcome::NotPresent)
        } else {
            None
        }
    }

    pub(crate) fn drain_all_until(
        &mut self,
        mut resources: RuntimeCoordinatorResources<'_>,
        deadline: Instant,
    ) -> RuntimeDrainOutcome {
        let keys = self.keys_matching(None);
        for key in keys.into_iter().flatten() {
            match self.retire_key_with_resources(&mut resources, key, deadline) {
                RuntimeRetirementOutcome::Retired | RuntimeRetirementOutcome::NotPresent => {}
                RuntimeRetirementOutcome::Unavailable(reason) => {
                    return RuntimeDrainOutcome::Unavailable(reason);
                }
                RuntimeRetirementOutcome::FailedClosed(reason) => {
                    return RuntimeDrainOutcome::FailedClosed(reason);
                }
            }
        }
        if let Some(reason) = self.fail_stop {
            RuntimeDrainOutcome::FailedClosed(reason)
        } else if self.has_obligation() {
            RuntimeDrainOutcome::Unavailable(
                RuntimeRetirementUnavailableReason::ActivationReconciliationPending,
            )
        } else {
            RuntimeDrainOutcome::Drained
        }
    }

    fn keys_matching(
        &self,
        profile: Option<ProfileId>,
    ) -> [Option<ExtensionNativeOwnershipKey>; crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES]
    {
        self.keys_matching_after_barrier(profile, self.journal_settlement_barrier_owner())
    }

    pub(super) fn keys_matching_after_barrier(
        &self,
        profile: Option<ProfileId>,
        barrier: Option<ExtensionNativeOwnershipKey>,
    ) -> [Option<ExtensionNativeOwnershipKey>; crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES]
    {
        let mut keys = [None; crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES];
        let mut next = 0;
        if let Some(barrier) = barrier.filter(|key| {
            profile.is_none_or(|profile| key.profile() == profile) && self.slot(*key).is_some()
        }) {
            keys[next] = Some(barrier);
            next += 1;
        }
        for slot in self.slots.iter().flatten() {
            if Some(slot.key()) != barrier
                && profile.is_none_or(|profile| slot.key().profile() == profile)
            {
                keys[next] = Some(slot.key());
                next += 1;
            }
        }
        keys
    }

    fn retire_key_with_resources(
        &mut self,
        resources: &mut RuntimeCoordinatorResources<'_>,
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
    ) -> RuntimeRetirementOutcome {
        if let Some(reason) = self.fail_stop {
            return RuntimeRetirementOutcome::FailedClosed(reason);
        }
        if self.journal_settlement_barrier_blocks(key) {
            return RuntimeRetirementOutcome::Unavailable(
                RuntimeRetirementUnavailableReason::StoreObservationPending,
            );
        }
        if Instant::now() >= deadline {
            return RuntimeRetirementOutcome::Unavailable(
                RuntimeRetirementUnavailableReason::DeadlineReached,
            );
        }
        if self.slot(key).is_none() {
            return RuntimeRetirementOutcome::NotPresent;
        }
        let Some(mut state) = self.slot_mut(key).and_then(|slot| slot.take_state()) else {
            self.enter_fail_stop(RuntimeCoordinatorFailureReason::InternalProtocolViolation);
            return RuntimeRetirementOutcome::FailedClosed(
                RuntimeCoordinatorFailureReason::InternalProtocolViolation,
            );
        };

        for _ in 0..MAX_RETIREMENT_STATE_TRANSITIONS_PER_CALL {
            match drive_retirement_state(resources, key, state, deadline) {
                RetirementDrive::Continue(next) => {
                    state = next;
                    if Instant::now() >= deadline {
                        return self.retain_retirement_wait(
                            key,
                            state,
                            RuntimeRetirementUnavailableReason::DeadlineReached,
                        );
                    }
                }
                RetirementDrive::Stop { state, reason } => {
                    return self.retain_retirement_wait(key, state, reason);
                }
                RetirementDrive::Complete(_) => {
                    self.remove_slot(key);
                    return RuntimeRetirementOutcome::Retired;
                }
                RetirementDrive::FailStop { retained, reason } => {
                    let installed = self
                        .slot_mut(key)
                        .is_some_and(|slot| slot.enter_fail_stop(reason, retained));
                    self.enter_fail_stop(reason);
                    return if installed {
                        RuntimeRetirementOutcome::FailedClosed(reason)
                    } else {
                        RuntimeRetirementOutcome::FailedClosed(
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
        RuntimeRetirementOutcome::FailedClosed(reason)
    }

    fn retain_retirement_wait(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        state: RuntimeSlotState,
        reason: RuntimeRetirementUnavailableReason,
    ) -> RuntimeRetirementOutcome {
        let installed = self
            .slot_mut(key)
            .is_some_and(|slot| slot.install_state(state));
        if installed {
            RuntimeRetirementOutcome::Unavailable(reason)
        } else {
            self.enter_fail_stop(RuntimeCoordinatorFailureReason::InternalProtocolViolation);
            RuntimeRetirementOutcome::FailedClosed(
                RuntimeCoordinatorFailureReason::InternalProtocolViolation,
            )
        }
    }
}

pub(super) fn drive_retirement_state(
    resources: &mut RuntimeCoordinatorResources<'_>,
    key: ExtensionNativeOwnershipKey,
    state: RuntimeSlotState,
    deadline: Instant,
) -> RetirementDrive {
    match state {
        RuntimeSlotState::Planning | RuntimeSlotState::Planned(_) => {
            RetirementDrive::Complete(ReleaseCompletion::Retired)
        }
        RuntimeSlotState::BeginConflict(conflict) => {
            retire_begin_conflict(resources, key, *conflict, deadline)
        }
        RuntimeSlotState::BeginAmbiguous(ambiguity) => {
            retire_begin_ambiguity(resources, *ambiguity, deadline)
        }
        RuntimeSlotState::AcquireLease(state) => {
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry: state.preparing,
                authority: ReleaseAuthority::NoRepositoryPin(Some(Box::new(state.plan))),
                completion: ReleaseCompletion::Retired,
            })))
        }
        RuntimeSlotState::PreparingLease(state) => {
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry: state.preparing,
                authority: ReleaseAuthority::Lease(Box::new(state.lease)),
                completion: ReleaseCompletion::Retired,
            })))
        }
        RuntimeSlotState::PackageAccessBuildRefusal(state) => {
            let super::slot::PackageAccessBuildRefusalState { refusal, preparing } = *state;
            match refusal.try_into_lease() {
                Ok(lease) => RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(
                    ReleaseRuntimeState {
                        current_entry: preparing,
                        authority: ReleaseAuthority::Lease(Box::new(lease)),
                        completion: ReleaseCompletion::Retired,
                    },
                ))),
                Err(refusal) => RetirementDrive::FailStop {
                    retained: RuntimeFailStopRetained::Prior(Box::new(
                        RuntimeSlotState::PackageAccessBuildRefusal(Box::new(
                            super::slot::PackageAccessBuildRefusalState { refusal, preparing },
                        )),
                    )),
                    reason: RuntimeCoordinatorFailureReason::RepositoryInvariant,
                },
            }
        }
        RuntimeSlotState::PreparingAccess(state) => {
            let PreparingAccessState { access, preparing } = *state;
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry: preparing,
                authority: ReleaseAuthority::PreHost(Box::new(access)),
                completion: ReleaseCompletion::Retired,
            })))
        }
        RuntimeSlotState::MayOwnConflict(state) => {
            retire_may_own_conflict(resources, *state, deadline)
        }
        RuntimeSlotState::MayOwnAmbiguous(ambiguity) => {
            retire_may_own_ambiguity(resources, key, *ambiguity, deadline)
        }
        RuntimeSlotState::MayOwnAccess(state) => {
            let MayOwnAccessState { access, entry } = *state;
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry: entry,
                authority: ReleaseAuthority::PreHost(Box::new(access)),
                completion: ReleaseCompletion::Retired,
            })))
        }
        RuntimeSlotState::HostBindingRefusal(refusal) => {
            match refusal.try_into_access_and_entry() {
                Ok((access, entry)) => RetirementDrive::Continue(RuntimeSlotState::Release(
                    Box::new(ReleaseRuntimeState {
                        current_entry: entry,
                        authority: ReleaseAuthority::PreHost(Box::new(access)),
                        completion: ReleaseCompletion::Retired,
                    }),
                )),
                Err(refusal) => RetirementDrive::FailStop {
                    retained: RuntimeFailStopRetained::Prior(Box::new(
                        RuntimeSlotState::HostBindingRefusal(Box::new(refusal)),
                    )),
                    reason: RuntimeCoordinatorFailureReason::HostInvariant,
                },
            }
        }
        RuntimeSlotState::NativeActivation(state) => {
            let super::slot::NativeActivationState {
                initial_entry,
                request,
                absence,
                pending,
                recovery,
            } = *state;
            let (access, lifecycle) = request.cancel();
            drop(lifecycle);
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry: initial_entry,
                authority: ReleaseAuthority::PostHost(Box::new(PostHostReleaseAuthority {
                    access,
                    operation: RuntimeOperationControl::Pending(Box::new(pending)),
                    recovery,
                    absence: absence.map_or(
                        PostHostAbsenceBasis::ActivationUnentered,
                        PostHostAbsenceBasis::Proven,
                    ),
                })),
                completion: ReleaseCompletion::Retired,
            })))
        }
        RuntimeSlotState::NativeUncertain(state) => {
            reconcile_activation_uncertainty_for_retirement(*state, deadline)
        }
        RuntimeSlotState::Owned(state) => retire_owned(resources, *state, deadline),
        RuntimeSlotState::NativeRetirement(state) => settle_native_retirement(*state, deadline),
        RuntimeSlotState::NativeRetirementUncertain(state) => {
            reconcile_retirement_uncertainty(*state, deadline)
        }
        RuntimeSlotState::Release(state) => drive_release(resources, *state, deadline),
        RuntimeSlotState::GrantRebindPending(operation) => {
            retirement_from_row_fence(settle_row_fence(resources, *operation, deadline))
        }
        RuntimeSlotState::RowFenceConflict(state) => {
            retirement_from_row_fence(reconcile_row_conflict(resources, *state, deadline))
        }
        RuntimeSlotState::RowFenceAmbiguous(state) => {
            retirement_from_row_fence(reconcile_row_ambiguity(resources, *state, deadline))
        }
        RuntimeSlotState::FailStop(state) => RetirementDrive::FailStop {
            reason: state.reason,
            retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::FailStop(state))),
        },
    }
}

fn retire_begin_conflict(
    resources: &mut RuntimeCoordinatorResources<'_>,
    key: ExtensionNativeOwnershipKey,
    conflict: crate::journal_store::JournalActivationConflict<
        crate::repository::ServiceRuntimeAcquisitionPlan,
    >,
    deadline: Instant,
) -> RetirementDrive {
    match resources.projection.reload(resources.store, deadline) {
        Ok(journal) if journal.get(key).is_none() => {
            drop(conflict.into_returned().into_authority());
            RetirementDrive::Complete(ReleaseCompletion::Retired)
        }
        Ok(_) => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::BeginConflict(
                Box::new(conflict),
            ))),
            reason: RuntimeCoordinatorFailureReason::JournalDiverged,
        },
        Err(failure) => {
            retirement_load_wait(RuntimeSlotState::BeginConflict(Box::new(conflict)), failure)
        }
    }
}

fn retire_begin_ambiguity(
    resources: &mut RuntimeCoordinatorResources<'_>,
    ambiguity: crate::journal_store::JournalActivationReloadRequired<
        crate::repository::ServiceRuntimeAcquisitionPlan,
    >,
    deadline: Instant,
) -> RetirementDrive {
    if let Err(failure) = resources.projection.reload(resources.store, deadline) {
        return retirement_load_wait(
            RuntimeSlotState::BeginAmbiguous(Box::new(ambiguity)),
            failure,
        );
    }
    match ambiguity.reconcile(resources.projection) {
        JournalActivationReconciliation::Applied(applied) => {
            let (plan, preparing) = applied.into_parts();
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry: preparing,
                authority: ReleaseAuthority::NoRepositoryPin(Some(Box::new(plan))),
                completion: ReleaseCompletion::Retired,
            })))
        }
        JournalActivationReconciliation::NotApplied(returned) => {
            drop(returned.into_authority());
            RetirementDrive::Complete(ReleaseCompletion::Retired)
        }
        JournalActivationReconciliation::ReloadPending(ambiguity) => RetirementDrive::Stop {
            state: RuntimeSlotState::BeginAmbiguous(Box::new(ambiguity)),
            reason: RuntimeRetirementUnavailableReason::StoreObservationPending,
        },
        JournalActivationReconciliation::Diverged(diverged) => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::BeginDiverged(Box::new(diverged)),
            reason: RuntimeCoordinatorFailureReason::JournalDiverged,
        },
    }
}

fn retire_may_own_conflict(
    resources: &mut RuntimeCoordinatorResources<'_>,
    state: super::slot::MayOwnConflictState,
    deadline: Instant,
) -> RetirementDrive {
    let super::slot::MayOwnConflictState {
        preparing,
        conflict,
    } = state;
    match resources.projection.reload(resources.store, deadline) {
        Ok(journal) if journal.get(preparing.key()) == Some(&preparing) => {
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry: preparing,
                authority: ReleaseAuthority::PreHost(Box::new(
                    conflict.into_returned().into_authority(),
                )),
                completion: ReleaseCompletion::Retired,
            })))
        }
        Ok(_) => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::MayOwnConflict(
                Box::new(super::slot::MayOwnConflictState {
                    preparing,
                    conflict,
                }),
            ))),
            reason: RuntimeCoordinatorFailureReason::JournalDiverged,
        },
        Err(failure) => retirement_load_wait(
            RuntimeSlotState::MayOwnConflict(Box::new(super::slot::MayOwnConflictState {
                preparing,
                conflict,
            })),
            failure,
        ),
    }
}

fn retire_may_own_ambiguity(
    resources: &mut RuntimeCoordinatorResources<'_>,
    key: ExtensionNativeOwnershipKey,
    ambiguity: crate::journal_store::JournalActivationReloadRequired<
        crate::repository::ServiceRuntimePackageAccess,
    >,
    deadline: Instant,
) -> RetirementDrive {
    if let Err(failure) = resources.projection.reload(resources.store, deadline) {
        return retirement_load_wait(
            RuntimeSlotState::MayOwnAmbiguous(Box::new(ambiguity)),
            failure,
        );
    }
    match ambiguity.reconcile(resources.projection) {
        JournalActivationReconciliation::Applied(applied) => {
            let (access, entry) = applied.into_parts();
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry: entry,
                authority: ReleaseAuthority::PreHost(Box::new(access)),
                completion: ReleaseCompletion::Retired,
            })))
        }
        JournalActivationReconciliation::NotApplied(returned) => {
            let Some(preparing) = resources
                .projection
                .known()
                .and_then(|journal| journal.get(key))
                .cloned()
            else {
                return RetirementDrive::FailStop {
                    retained: RuntimeFailStopRetained::PreHostAccess(Box::new(
                        returned.into_authority(),
                    )),
                    reason: RuntimeCoordinatorFailureReason::JournalDiverged,
                };
            };
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry: preparing,
                authority: ReleaseAuthority::PreHost(Box::new(returned.into_authority())),
                completion: ReleaseCompletion::Retired,
            })))
        }
        JournalActivationReconciliation::ReloadPending(ambiguity) => RetirementDrive::Stop {
            state: RuntimeSlotState::MayOwnAmbiguous(Box::new(ambiguity)),
            reason: RuntimeRetirementUnavailableReason::StoreObservationPending,
        },
        JournalActivationReconciliation::Diverged(diverged) => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::MayOwnDiverged(Box::new(diverged)),
            reason: RuntimeCoordinatorFailureReason::JournalDiverged,
        },
    }
}

fn reconcile_activation_uncertainty_for_retirement(
    state: super::slot::NativeUncertainState,
    deadline: Instant,
) -> RetirementDrive {
    let super::slot::NativeUncertainState {
        current_entry,
        owner,
        failure: _,
        pending,
        recovery,
    } = state;
    match panic::catch_unwind(AssertUnwindSafe(|| owner.reconcile_until(deadline))) {
        Ok(ExtensionRuntimeReconciliationSettlement::Owned(owner)) => RetirementDrive::Continue(
            RuntimeSlotState::Owned(Box::new(super::slot::OwnedRuntimeState {
                current_entry,
                owner,
                operation: RuntimeOperationControl::Pending(Box::new(pending)),
                recovery,
            })),
        ),
        Ok(ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure }) => {
            RetirementDrive::Stop {
                state: RuntimeSlotState::NativeUncertain(Box::new(
                    super::slot::NativeUncertainState {
                        current_entry,
                        owner,
                        failure,
                        pending,
                        recovery,
                    },
                )),
                reason: RuntimeRetirementUnavailableReason::NativeOwnershipUncertain(failure),
            }
        }
        Ok(ExtensionRuntimeReconciliationSettlement::Absent { access, absence }) => {
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry,
                authority: ReleaseAuthority::PostHost(Box::new(PostHostReleaseAuthority {
                    access,
                    operation: RuntimeOperationControl::Pending(Box::new(pending)),
                    recovery,
                    absence: PostHostAbsenceBasis::Proven(absence),
                })),
                completion: ReleaseCompletion::Retired,
            })))
        }
        Err(_) => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::NativeCallPanicked(Box::new(
                super::slot::NativeCallPanicAuthority {
                    pending,
                    recovery,
                    current_entry,
                },
            )),
            reason: RuntimeCoordinatorFailureReason::NativeCallPanicked,
        },
    }
}

fn retire_owned(
    resources: &mut RuntimeCoordinatorResources<'_>,
    state: super::slot::OwnedRuntimeState,
    deadline: Instant,
) -> RetirementDrive {
    match (state.current_entry.intent(), state.current_entry.phase()) {
        (
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn
            | ExtensionNativeOwnershipPhase::NativeOwned,
        ) => retirement_from_row_fence(settle_row_fence(
            resources,
            RuntimeRowFenceOperation {
                mutation: RuntimeRowFenceMutation::Transition {
                    intent: ExtensionNativeOwnershipIntent::Release,
                    phase: ExtensionNativeOwnershipPhase::NativeMayOwn,
                },
                authority: RuntimeRowFenceAuthority::Owned(Box::new(state)),
            },
            deadline,
        )),
        (ExtensionNativeOwnershipIntent::Release, ExtensionNativeOwnershipPhase::NativeMayOwn) => {
            let super::slot::OwnedRuntimeState {
                current_entry,
                owner,
                operation,
                recovery,
            } = state;
            RetirementDrive::Continue(RuntimeSlotState::NativeRetirement(Box::new(
                NativeRetirementState {
                    current_entry,
                    request: owner.into_retirement_request(),
                    operation,
                    recovery,
                    completion: ReleaseCompletion::Retired,
                },
            )))
        }
        _ => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Owned(Box::new(
                state,
            )))),
            reason: RuntimeCoordinatorFailureReason::JournalDiverged,
        },
    }
}

fn settle_native_retirement(state: NativeRetirementState, deadline: Instant) -> RetirementDrive {
    let NativeRetirementState {
        current_entry,
        request,
        operation,
        recovery,
        completion,
    } = state;
    // See activation's panic note: unwind builds cannot reconstruct the moved
    // request; release builds abort. Durable MayOwn remains untouched and the
    // sibling authorities keep this slot permanently attached and fail-closed.
    match panic::catch_unwind(AssertUnwindSafe(|| request.settle_until(deadline))) {
        Ok(ExtensionRuntimeRetirementSettlement::Retired { access, absence }) => {
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry,
                authority: ReleaseAuthority::PostHost(Box::new(PostHostReleaseAuthority {
                    access,
                    operation,
                    recovery,
                    absence: PostHostAbsenceBasis::Proven(absence),
                })),
                completion,
            })))
        }
        Ok(ExtensionRuntimeRetirementSettlement::Retained { owner, failure }) => {
            RetirementDrive::Stop {
                state: RuntimeSlotState::Owned(Box::new(super::slot::OwnedRuntimeState {
                    current_entry,
                    owner,
                    operation,
                    recovery,
                })),
                reason: RuntimeRetirementUnavailableReason::NativeRetained(failure),
            }
        }
        Ok(ExtensionRuntimeRetirementSettlement::OwnershipUncertain { owner, failure }) => {
            RetirementDrive::Stop {
                state: RuntimeSlotState::NativeRetirementUncertain(Box::new(
                    NativeRetirementUncertainState {
                        current_entry,
                        owner,
                        operation,
                        recovery,
                        completion,
                    },
                )),
                reason: RuntimeRetirementUnavailableReason::NativeOwnershipUncertain(failure),
            }
        }
        Err(_) => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::NativeRetirementCallPanicked(Box::new(
                NativeRetirementCallPanicAuthority {
                    operation,
                    recovery,
                    current_entry,
                },
            )),
            reason: RuntimeCoordinatorFailureReason::NativeCallPanicked,
        },
    }
}

fn reconcile_retirement_uncertainty(
    state: NativeRetirementUncertainState,
    deadline: Instant,
) -> RetirementDrive {
    let NativeRetirementUncertainState {
        current_entry,
        owner,
        operation,
        recovery,
        completion,
    } = state;
    match panic::catch_unwind(AssertUnwindSafe(|| owner.reconcile_until(deadline))) {
        Ok(ExtensionRuntimeReconciliationSettlement::Owned(owner)) => RetirementDrive::Continue(
            RuntimeSlotState::Owned(Box::new(super::slot::OwnedRuntimeState {
                current_entry,
                owner,
                operation,
                recovery,
            })),
        ),
        Ok(ExtensionRuntimeReconciliationSettlement::Absent { access, absence }) => {
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry,
                authority: ReleaseAuthority::PostHost(Box::new(PostHostReleaseAuthority {
                    access,
                    operation,
                    recovery,
                    absence: PostHostAbsenceBasis::Proven(absence),
                })),
                completion,
            })))
        }
        Ok(ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure }) => {
            RetirementDrive::Stop {
                state: RuntimeSlotState::NativeRetirementUncertain(Box::new(
                    NativeRetirementUncertainState {
                        current_entry,
                        owner,
                        operation,
                        recovery,
                        completion,
                    },
                )),
                reason: RuntimeRetirementUnavailableReason::NativeOwnershipUncertain(failure),
            }
        }
        Err(_) => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::NativeRetirementCallPanicked(Box::new(
                NativeRetirementCallPanicAuthority {
                    operation,
                    recovery,
                    current_entry,
                },
            )),
            reason: RuntimeCoordinatorFailureReason::NativeCallPanicked,
        },
    }
}

pub(super) fn drive_release(
    resources: &mut RuntimeCoordinatorResources<'_>,
    state: ReleaseRuntimeState,
    deadline: Instant,
) -> RetirementDrive {
    match release_action(
        state.current_entry.intent(),
        state.current_entry.phase(),
        state.authority.class(),
    ) {
        Ok(ReleaseAction::FenceDefiniteAbsence) => {
            return retirement_from_row_fence(settle_row_fence(
                resources,
                RuntimeRowFenceOperation {
                    mutation: RuntimeRowFenceMutation::Transition {
                        intent: ExtensionNativeOwnershipIntent::Release,
                        phase: ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                    },
                    authority: RuntimeRowFenceAuthority::Release(Box::new(state)),
                },
                deadline,
            ));
        }
        Ok(_) => {}
        Err(reason) => {
            return RetirementDrive::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Release(
                    Box::new(state),
                ))),
                reason,
            };
        }
    }

    let ReleaseRuntimeState {
        current_entry,
        authority,
        completion,
    } = state;
    match authority {
        ReleaseAuthority::NoRepositoryPin(plan) => {
            drop(plan);
            clear_released_row(resources, current_entry, completion, deadline)
        }
        ReleaseAuthority::Lease(lease) => {
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry,
                authority: ReleaseAuthority::Repository(Box::new(lease.into_release())),
                completion,
            })))
        }
        ReleaseAuthority::PreHost(access) => match access.try_into_release() {
            Ok(release) => RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(
                ReleaseRuntimeState {
                    current_entry,
                    authority: ReleaseAuthority::Repository(Box::new(release)),
                    completion,
                },
            ))),
            Err(refusal) => RetirementDrive::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Release(
                    Box::new(ReleaseRuntimeState {
                        current_entry,
                        authority: ReleaseAuthority::PreHostRefusal(Box::new(refusal)),
                        completion,
                    }),
                ))),
                reason: RuntimeCoordinatorFailureReason::RepositoryInvariant,
            },
        },
        // These conversion refusals are pure structural checks. A recoverable
        // authority shape permits lossless inspection but cannot become valid
        // on a later attempt; no transient repository operation occurs here.
        ReleaseAuthority::PreHostRefusal(refusal) => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Release(
                Box::new(ReleaseRuntimeState {
                    current_entry,
                    authority: ReleaseAuthority::PreHostRefusal(refusal),
                    completion,
                }),
            ))),
            reason: RuntimeCoordinatorFailureReason::RepositoryInvariant,
        },
        ReleaseAuthority::PostHost(post_host) => {
            recover_post_host(resources, current_entry, *post_host, completion, deadline)
        }
        ReleaseAuthority::RejoinRefusal(refusal) => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Release(
                Box::new(ReleaseRuntimeState {
                    current_entry,
                    authority: ReleaseAuthority::RejoinRefusal(refusal),
                    completion,
                }),
            ))),
            reason: RuntimeCoordinatorFailureReason::RepositoryInvariant,
        },
        ReleaseAuthority::Repository(mut release) => {
            let binding = match ExtensionPackagePinReleaseBinding::mint(&current_entry) {
                Ok(binding) => binding,
                Err(_) => {
                    return RetirementDrive::FailStop {
                        retained: RuntimeFailStopRetained::Prior(Box::new(
                            RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                                current_entry,
                                authority: ReleaseAuthority::Repository(release),
                                completion,
                            })),
                        )),
                        reason: RuntimeCoordinatorFailureReason::InternalProtocolViolation,
                    };
                }
            };
            match resources
                .repository
                .settle_runtime_release(&mut release, &binding)
            {
                Ok(
                    BundledPackageLeaseReleaseOutcome::Released
                    | BundledPackageLeaseReleaseOutcome::AlreadyReleased,
                ) => RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(
                    ReleaseRuntimeState {
                        current_entry,
                        authority: ReleaseAuthority::NoRepositoryPin(None),
                        completion,
                    },
                ))),
                Err(error) => match repository_release_error_disposition(error) {
                    RepositoryReleaseErrorDisposition::Retry => RetirementDrive::Stop {
                        state: RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                            current_entry,
                            authority: ReleaseAuthority::Repository(release),
                            completion,
                        })),
                        reason: RuntimeRetirementUnavailableReason::RepositoryUnavailable,
                    },
                    RepositoryReleaseErrorDisposition::FailStop => RetirementDrive::FailStop {
                        retained: RuntimeFailStopRetained::Prior(Box::new(
                            RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                                current_entry,
                                authority: ReleaseAuthority::Repository(release),
                                completion,
                            })),
                        )),
                        reason: RuntimeCoordinatorFailureReason::RepositoryInvariant,
                    },
                },
                Ok(_) => RetirementDrive::FailStop {
                    retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Release(
                        Box::new(ReleaseRuntimeState {
                            current_entry,
                            authority: ReleaseAuthority::Repository(release),
                            completion,
                        }),
                    ))),
                    reason: RuntimeCoordinatorFailureReason::RepositoryInvariant,
                },
            }
        }
    }
}

fn recover_post_host(
    resources: &mut RuntimeCoordinatorResources<'_>,
    current_entry: zephium_core::extensions::ExtensionNativeOwnershipEntry,
    post_host: PostHostReleaseAuthority,
    completion: ReleaseCompletion,
    deadline: Instant,
) -> RetirementDrive {
    if !post_host.accepts_absence_for(&current_entry) {
        return RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(post_host_release_state(
                current_entry,
                post_host,
                completion,
            ))),
            reason: RuntimeCoordinatorFailureReason::HostInvariant,
        };
    }
    let revalidation = match resources.projection.reload(resources.store, deadline) {
        Ok(journal) => classify_post_absence_reload(Ok(
            journal.get(current_entry.key()) == Some(&current_entry)
        )),
        Err(failure) => classify_post_absence_reload(Err(failure)),
    };
    match revalidation {
        PostAbsenceReloadDisposition::Verified => {}
        PostAbsenceReloadDisposition::Wait(reason) => {
            return RetirementDrive::Stop {
                state: post_host_release_state(current_entry, post_host, completion),
                reason,
            };
        }
        PostAbsenceReloadDisposition::Fail(reason) => {
            return RetirementDrive::FailStop {
                retained: RuntimeFailStopRetained::Prior(Box::new(post_host_release_state(
                    current_entry,
                    post_host,
                    completion,
                ))),
                reason,
            };
        }
    }
    let PostHostReleaseAuthority {
        access,
        operation,
        recovery,
        absence,
    } = post_host;
    let operation_authority = match operation {
        RuntimeOperationControl::Pending(pending) => {
            match pending.recover_after_absence(&current_entry) {
                Ok(authority) => authority,
                Err(refusal) => {
                    return RetirementDrive::FailStop {
                        retained: RuntimeFailStopRetained::Prior(Box::new(
                            RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                                current_entry,
                                authority: ReleaseAuthority::PostHost(Box::new(
                                    PostHostReleaseAuthority {
                                        access,
                                        operation: RuntimeOperationControl::Pending(Box::new(
                                            refusal.into_pending(),
                                        )),
                                        recovery,
                                        absence,
                                    },
                                )),
                                completion,
                            })),
                        )),
                        reason: RuntimeCoordinatorFailureReason::HostInvariant,
                    };
                }
            }
        }
        RuntimeOperationControl::PublicationRequest(request) => {
            match request.recover_after_absence(&current_entry) {
                Ok(authority) => authority,
                Err(refusal) => {
                    return RetirementDrive::FailStop {
                        retained: RuntimeFailStopRetained::Prior(Box::new(
                            RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                                current_entry,
                                authority: ReleaseAuthority::PostHost(Box::new(
                                    PostHostReleaseAuthority {
                                        access,
                                        operation: RuntimeOperationControl::PublicationRequest(
                                            Box::new(refusal.into_request()),
                                        ),
                                        recovery,
                                        absence,
                                    },
                                )),
                                completion,
                            })),
                        )),
                        reason: RuntimeCoordinatorFailureReason::HostInvariant,
                    };
                }
            }
        }
        RuntimeOperationControl::Published(receipt) => {
            match panic::catch_unwind(AssertUnwindSafe(|| {
                receipt.reclaim_after_absence(&current_entry)
            })) {
                Err(_) => {
                    return RetirementDrive::FailStop {
                        retained: RuntimeFailStopRetained::PublicationReclaimCallPanicked(
                            Box::new(super::slot::PublicationReclaimCallPanicAuthority {
                                access,
                                recovery,
                                current_entry,
                                absence,
                            }),
                        ),
                        reason: RuntimeCoordinatorFailureReason::NativeCallPanicked,
                    };
                }
                Ok(reclaim) => match reclaim {
                    Ok(authority) => authority,
                    Err(refusal) => {
                        let reason = refusal.reason();
                        return match refusal.try_into_receipt() {
                            Ok(receipt) => {
                                let retained =
                                    RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                                        current_entry,
                                        authority: ReleaseAuthority::PostHost(Box::new(
                                            PostHostReleaseAuthority {
                                                access,
                                                operation: RuntimeOperationControl::Published(
                                                    Box::new(receipt),
                                                ),
                                                recovery,
                                                absence,
                                            },
                                        )),
                                        completion,
                                    }));
                                match reclaim_failure_disposition(reason) {
                                    ReclaimFailureDisposition::Retry => RetirementDrive::Stop {
                                        state: retained,
                                        reason: RuntimeRetirementUnavailableReason::PublicationReclaimPending,
                                    },
                                    ReclaimFailureDisposition::Fail(failure) => {
                                        RetirementDrive::FailStop {
                                            retained: RuntimeFailStopRetained::Prior(Box::new(
                                                retained,
                                            )),
                                            reason: failure,
                                        }
                                    }
                                }
                            }
                            Err(refusal) => RetirementDrive::FailStop {
                                retained: RuntimeFailStopRetained::PublicationReclaim(Box::new(
                                    super::slot::PublicationReclaimFailStopAuthority {
                                        access,
                                        refusal,
                                        recovery,
                                        current_entry,
                                        absence,
                                    },
                                )),
                                reason: RuntimeCoordinatorFailureReason::HostInvariant,
                            },
                        };
                    }
                },
            }
        }
    };
    match recovery.try_into_release(access, operation_authority) {
        Ok(release) => {
            RetirementDrive::Continue(RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
                current_entry,
                authority: ReleaseAuthority::Repository(Box::new(release)),
                completion,
            })))
        }
        Err(refusal) => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(RuntimeSlotState::Release(
                Box::new(ReleaseRuntimeState {
                    current_entry,
                    authority: ReleaseAuthority::RejoinRefusal(Box::new(refusal)),
                    completion,
                }),
            ))),
            reason: RuntimeCoordinatorFailureReason::RepositoryInvariant,
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PostAbsenceReloadDisposition {
    Verified,
    Wait(RuntimeRetirementUnavailableReason),
    Fail(RuntimeCoordinatorFailureReason),
}

pub(super) const fn classify_post_absence_reload(
    result: Result<bool, JournalLoadFailure>,
) -> PostAbsenceReloadDisposition {
    match result {
        Ok(true) => PostAbsenceReloadDisposition::Verified,
        Ok(false) => {
            PostAbsenceReloadDisposition::Fail(RuntimeCoordinatorFailureReason::JournalDiverged)
        }
        Err(JournalLoadFailure::NotAdmitted) => {
            PostAbsenceReloadDisposition::Wait(RuntimeRetirementUnavailableReason::StoreNotAdmitted)
        }
        Err(JournalLoadFailure::TimedOutAfterAdmission) => PostAbsenceReloadDisposition::Wait(
            RuntimeRetirementUnavailableReason::StoreObservationPending,
        ),
        Err(JournalLoadFailure::Failed) => {
            PostAbsenceReloadDisposition::Fail(RuntimeCoordinatorFailureReason::JournalInvalid)
        }
    }
}

fn post_host_release_state(
    current_entry: zephium_core::extensions::ExtensionNativeOwnershipEntry,
    post_host: PostHostReleaseAuthority,
    completion: ReleaseCompletion,
) -> RuntimeSlotState {
    RuntimeSlotState::Release(Box::new(ReleaseRuntimeState {
        current_entry,
        authority: ReleaseAuthority::PostHost(Box::new(post_host)),
        completion,
    }))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReclaimFailureDisposition {
    Retry,
    Fail(RuntimeCoordinatorFailureReason),
}

pub(super) const fn reclaim_failure_disposition(
    reason: ExtensionRuntimePublicationReclaimError,
) -> ReclaimFailureDisposition {
    match reason {
        ExtensionRuntimePublicationReclaimError::Host(
            ExtensionRuntimeHostBindError::Unavailable,
        ) => ReclaimFailureDisposition::Retry,
        ExtensionRuntimePublicationReclaimError::Host(
            ExtensionRuntimeHostBindError::RetainedBytesOverflow,
        ) => {
            ReclaimFailureDisposition::Fail(RuntimeCoordinatorFailureReason::RetainedBytesOverflow)
        }
        ExtensionRuntimePublicationReclaimError::Authorization(_)
        | ExtensionRuntimePublicationReclaimError::Host(_) => {
            ReclaimFailureDisposition::Fail(RuntimeCoordinatorFailureReason::HostInvariant)
        }
        _ => ReclaimFailureDisposition::Fail(RuntimeCoordinatorFailureReason::HostInvariant),
    }
}

fn clear_released_row(
    resources: &mut RuntimeCoordinatorResources<'_>,
    current_entry: zephium_core::extensions::ExtensionNativeOwnershipEntry,
    completion: ReleaseCompletion,
    deadline: Instant,
) -> RetirementDrive {
    retirement_from_row_fence(settle_row_fence(
        resources,
        RuntimeRowFenceOperation {
            mutation: RuntimeRowFenceMutation::Clear,
            authority: RuntimeRowFenceAuthority::Release(Box::new(ReleaseRuntimeState {
                current_entry,
                authority: ReleaseAuthority::NoRepositoryPin(None),
                completion,
            })),
        },
        deadline,
    ))
}

fn retirement_from_row_fence(progress: RowFenceProgress) -> RetirementDrive {
    match progress {
        RowFenceProgress::Continue(state) => RetirementDrive::Continue(state),
        RowFenceProgress::Wait { state, reason } => RetirementDrive::Stop {
            state,
            reason: match reason {
                RowFenceWait::StoreNotAdmitted => {
                    RuntimeRetirementUnavailableReason::StoreNotAdmitted
                }
                RowFenceWait::StoreObservationPending => {
                    RuntimeRetirementUnavailableReason::StoreObservationPending
                }
            },
        },
        RowFenceProgress::Cleared(completion) => RetirementDrive::Complete(completion),
        RowFenceProgress::FailStop { retained, reason } => {
            RetirementDrive::FailStop { retained, reason }
        }
    }
}

fn retirement_load_wait(state: RuntimeSlotState, failure: JournalLoadFailure) -> RetirementDrive {
    match failure {
        JournalLoadFailure::NotAdmitted => RetirementDrive::Stop {
            state,
            reason: RuntimeRetirementUnavailableReason::StoreNotAdmitted,
        },
        JournalLoadFailure::TimedOutAfterAdmission => RetirementDrive::Stop {
            state,
            reason: RuntimeRetirementUnavailableReason::StoreObservationPending,
        },
        JournalLoadFailure::Failed => RetirementDrive::FailStop {
            retained: RuntimeFailStopRetained::Prior(Box::new(state)),
            reason: RuntimeCoordinatorFailureReason::JournalInvalid,
        },
    }
}
