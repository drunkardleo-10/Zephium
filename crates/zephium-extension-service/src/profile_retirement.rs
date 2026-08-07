//! Worker-local monotonic profile-retirement fencing.

use std::collections::BTreeMap;
use std::time::Instant;

use zephium_core::ids::ProfileId;
use zephium_core::session::MAX_SESSION_PROFILES;
use zephium_extension_repository::{
    ExtensionRepositoryError, ProfilePackageAbsenceRevalidationError, ProfilePackageObligation,
};
use zephium_extension_runtime_api::ExtensionRuntimeHostProfileAbsenceDisposition;
use zephium_private_fs::PrivateFsError;
use zephium_store::ExtensionServiceStoreAuthority;

use crate::cleanup::{
    reconcile_startup, CancellationCheck, CleanupAttempt, CleanupFailure, CleanupScope,
    CleanupStartupOutcome, CleanupUnavailable,
};
use crate::journal_store::JournalProjection;
use crate::native_recovery::NativeRecoveryState;
use crate::repository::ServiceRepository;
use crate::runtime_coordinator::{
    RuntimeCoordinator, RuntimeCoordinatorFailureReason, RuntimeCoordinatorResources,
    RuntimeRetirementOutcome, RuntimeRetirementUnavailableReason,
};

const MAX_PROFILE_RETIREMENT_STATES: usize = 64;
const _: () = assert!(MAX_SESSION_PROFILES == MAX_PROFILE_RETIREMENT_STATES);

/// Retryable reason profile retirement could not complete in this attempt.
///
/// Every variant is non-authorizing: profile data must remain intact. An
/// admitted command retains its installed worker-local fence for retry;
/// [`Self::WorkerBusy`] means the reserved command itself was not admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionServiceProfileRetirementUnavailableReason {
    /// The exact operation or observation deadline elapsed.
    DeadlineReached,
    /// Another reserved retirement barrier is still queued.
    WorkerBusy,
    /// Cooperative extension-service shutdown interrupted reconciliation.
    CancellationRequested,
    /// Store observation or mutation could not currently settle.
    StoreUnavailable,
    /// Repository opening, recovery, or bounded I/O could not currently settle.
    RepositoryUnavailable,
    /// The native registry could not currently complete its profile fence.
    NativeRuntimeUnavailable,
    /// Durable package, journal, or native-runtime obligations still remain.
    ObligationsRemain,
}

/// Sticky reason one profile's retirement protocol failed closed.
///
/// No variant is deletion evidence. Once committed to a profile tombstone by
/// the worker, later calls for that profile return the same failed-closed class
/// and ingress stays fenced for the rest of that worker lifetime. Admission
/// failures can report [`Self::WorkerUnavailable`] before worker settlement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionServiceProfileRetirementFailureReason {
    /// The worker or its reserved mailbox barrier is no longer available.
    WorkerUnavailable,
    /// More distinct profile tombstones were requested than the session bound.
    RegistryCapacityExhausted,
    /// Durable repository state or authenticated package closure was corrupt.
    RepositoryCorrupt,
    /// Store's ownership journal violated its exact bounded protocol.
    OwnershipJournalInvalid,
    /// Native registry identity or reservation invariants failed.
    NativeRuntimeInvariant,
    /// A service-owned ordering or identity invariant failed.
    InternalProtocolViolation,
}

/// Result of one owner-directed profile-retirement attempt.
///
/// This ordinary copied status is deliberately non-authorizing and must never
/// be accepted by another API as profile-deletion evidence. Callers may branch
/// on the direct settlement of their invocation, while the higher-level
/// profile owner retains and enforces deletion authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "profile retirement settlement must be checked"]
pub enum ExtensionServiceProfileRetirementOutcome {
    /// The profile is durably drained, process-locally absent, and permanently
    /// fenced in this exact worker. Repeated calls return this same status.
    /// This publicly constructible variant is not capability evidence.
    Retired,
    /// This attempt was retryable and grants no deletion authority. If the
    /// worker admitted it, the permanent fence remains installed.
    Unavailable(ExtensionServiceProfileRetirementUnavailableReason),
    /// This attempt failed closed and grants no deletion authority. A failure
    /// committed by the worker remains a sticky permanent fence.
    FailedClosed(ExtensionServiceProfileRetirementFailureReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProfileRetirementState {
    Fencing,
    Retired,
    FailedClosed(ExtensionServiceProfileRetirementFailureReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProfileRetirementBegin {
    Proceed,
    Retired,
    FailedClosed(ExtensionServiceProfileRetirementFailureReason),
    CapacityExhausted,
}

/// Bounded, monotonic tombstones for one worker lifetime.
///
/// Entries are never removed or evicted. All three states reject later
/// activation/recovery ingress; only the fenced cleanup path may advance a
/// `Fencing` entry to a terminal state.
pub(crate) struct ProfileRetirementRegistry {
    states: BTreeMap<ProfileId, ProfileRetirementState>,
    saturated: bool,
}

impl ProfileRetirementRegistry {
    pub(crate) const fn new() -> Self {
        Self {
            states: BTreeMap::new(),
            saturated: false,
        }
    }

    /// Installs `Fencing` before any deadline or external-state check.
    pub(crate) fn begin(&mut self, profile: ProfileId) -> ProfileRetirementBegin {
        match self.states.get(&profile).copied() {
            Some(ProfileRetirementState::Fencing) => ProfileRetirementBegin::Proceed,
            Some(ProfileRetirementState::Retired) => ProfileRetirementBegin::Retired,
            Some(ProfileRetirementState::FailedClosed(reason)) => {
                ProfileRetirementBegin::FailedClosed(reason)
            }
            None if self.states.len() >= MAX_PROFILE_RETIREMENT_STATES => {
                // Capacity exhaustion cannot retain another exact tombstone,
                // so conservatively fence every otherwise-untracked profile
                // for the rest of this worker lifetime.
                self.saturated = true;
                ProfileRetirementBegin::CapacityExhausted
            }
            None => {
                self.states.insert(profile, ProfileRetirementState::Fencing);
                ProfileRetirementBegin::Proceed
            }
        }
    }

    pub(crate) fn mark_retired(
        &mut self,
        profile: ProfileId,
    ) -> ExtensionServiceProfileRetirementOutcome {
        match self.states.get_mut(&profile) {
            Some(state @ ProfileRetirementState::Fencing) => {
                *state = ProfileRetirementState::Retired;
                ExtensionServiceProfileRetirementOutcome::Retired
            }
            Some(ProfileRetirementState::Retired) => {
                ExtensionServiceProfileRetirementOutcome::Retired
            }
            Some(ProfileRetirementState::FailedClosed(reason)) => {
                ExtensionServiceProfileRetirementOutcome::FailedClosed(*reason)
            }
            None => ExtensionServiceProfileRetirementOutcome::FailedClosed(
                ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
            ),
        }
    }

    pub(crate) fn fail_closed(
        &mut self,
        profile: ProfileId,
        reason: ExtensionServiceProfileRetirementFailureReason,
    ) -> ExtensionServiceProfileRetirementOutcome {
        match self.states.get_mut(&profile) {
            Some(state @ ProfileRetirementState::Fencing) => {
                *state = ProfileRetirementState::FailedClosed(reason);
                ExtensionServiceProfileRetirementOutcome::FailedClosed(reason)
            }
            Some(ProfileRetirementState::Retired) => {
                ExtensionServiceProfileRetirementOutcome::Retired
            }
            Some(ProfileRetirementState::FailedClosed(current)) => {
                ExtensionServiceProfileRetirementOutcome::FailedClosed(*current)
            }
            None => ExtensionServiceProfileRetirementOutcome::FailedClosed(
                ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
            ),
        }
    }

    /// All retained states are permanent ingress fences.
    pub(crate) fn blocks_ingress(&self, profile: ProfileId) -> bool {
        self.saturated || self.states.contains_key(&profile)
    }

    pub(crate) fn has_any_fence(&self) -> bool {
        self.saturated || !self.states.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn state(&self, profile: ProfileId) -> Option<ProfileRetirementState> {
        self.states.get(&profile).copied()
    }
}

/// Exact worker-owned resources joined by one profile-retirement attempt.
pub(crate) struct ProfileRetirementResources<'worker> {
    store: &'worker ExtensionServiceStoreAuthority,
    projection: &'worker mut JournalProjection,
    repository: &'worker mut ServiceRepository,
    native_recovery: &'worker mut NativeRecoveryState,
}

impl<'worker> ProfileRetirementResources<'worker> {
    pub(crate) fn new(
        store: &'worker ExtensionServiceStoreAuthority,
        projection: &'worker mut JournalProjection,
        repository: &'worker mut ServiceRepository,
        native_recovery: &'worker mut NativeRecoveryState,
    ) -> Self {
        Self {
            store,
            projection,
            repository,
            native_recovery,
        }
    }
}

/// Advances one permanently fenced profile through its exact proof chain.
///
/// Worker-owned live and interrupted runtime slots retire before the generic
/// durable cleanup path observes their rows. Repository and native absence can
/// therefore never be inferred around a coordinator authority that still
/// belongs to this worker.
pub(crate) fn retire_profile_until(
    resources: Option<ProfileRetirementResources<'_>>,
    runtime: &mut RuntimeCoordinator,
    retirements: &mut ProfileRetirementRegistry,
    profile: ProfileId,
    deadline: Instant,
    cancellation: &impl CancellationCheck,
) -> ExtensionServiceProfileRetirementOutcome {
    // This is deliberately the first operation. In particular, an already
    // expired deadline must not bypass the monotonic ingress fence.
    match retirements.begin(profile) {
        ProfileRetirementBegin::Proceed => {}
        ProfileRetirementBegin::Retired => {
            return ExtensionServiceProfileRetirementOutcome::Retired;
        }
        ProfileRetirementBegin::FailedClosed(reason) => {
            return ExtensionServiceProfileRetirementOutcome::FailedClosed(reason);
        }
        ProfileRetirementBegin::CapacityExhausted => {
            return ExtensionServiceProfileRetirementOutcome::FailedClosed(
                ExtensionServiceProfileRetirementFailureReason::RegistryCapacityExhausted,
            );
        }
    }
    if !retirements.blocks_ingress(profile) {
        return retirements.fail_closed(
            profile,
            ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
        );
    }
    if let Some(reason) = retirement_refusal(cancellation, deadline) {
        return ExtensionServiceProfileRetirementOutcome::Unavailable(reason);
    }
    let Some(resources) = resources else {
        return retirements.fail_closed(
            profile,
            ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
        );
    };
    let ProfileRetirementResources {
        store,
        projection,
        repository,
        native_recovery,
    } = resources;

    #[allow(unreachable_patterns)]
    match runtime.retire_profile_until(
        RuntimeCoordinatorResources::new(
            store,
            &mut *projection,
            &mut *repository,
            &mut *native_recovery,
        ),
        profile,
        deadline,
    ) {
        RuntimeRetirementOutcome::Retired | RuntimeRetirementOutcome::NotPresent => {}
        RuntimeRetirementOutcome::Unavailable(reason) => {
            return match runtime_retirement_unavailable(reason) {
                Ok(reason) => ExtensionServiceProfileRetirementOutcome::Unavailable(reason),
                Err(reason) => retirements.fail_closed(profile, reason),
            };
        }
        RuntimeRetirementOutcome::FailedClosed(reason) => {
            return retirements.fail_closed(profile, runtime_retirement_failure(reason));
        }
        _ => {
            return retirements.fail_closed(
                profile,
                ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
            );
        }
    }
    if runtime.has_profile_obligation(profile) {
        return retirements.fail_closed(
            profile,
            ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
        );
    }
    if let Some(reason) = retirement_refusal(cancellation, deadline) {
        return ExtensionServiceProfileRetirementOutcome::Unavailable(reason);
    }

    // Drain only this profile's exact durable journal rows. Unrelated rows and
    // native frontiers are neither advanced nor used as deletion evidence.
    match reconcile_startup(
        store,
        &mut *projection,
        &mut *repository,
        Some(&mut *native_recovery),
        CleanupAttempt::new(CleanupScope::Profile(profile), cancellation, deadline),
        |_| {},
    ) {
        CleanupStartupOutcome::Ready { .. } => {}
        CleanupStartupOutcome::CleanupRequired { .. } => {
            return ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::ObligationsRemain,
            );
        }
        CleanupStartupOutcome::Unavailable(reason) => {
            return ExtensionServiceProfileRetirementOutcome::Unavailable(cleanup_unavailable(
                reason,
            ));
        }
        CleanupStartupOutcome::Failed(CleanupFailure::ConcurrentPackageLease) => {
            return ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::ObligationsRemain,
            );
        }
        CleanupStartupOutcome::Failed(reason) => {
            return retirements.fail_closed(profile, cleanup_failure(reason));
        }
    }

    if let Some(reason) = retirement_refusal(cancellation, deadline) {
        return ExtensionServiceProfileRetirementOutcome::Unavailable(reason);
    }
    let package_absence = match repository.audit_profile_package_obligations(profile) {
        Ok(ProfilePackageObligation::Absent(evidence)) => evidence,
        Ok(ProfilePackageObligation::Present(_)) => {
            return ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::ObligationsRemain,
            );
        }
        Err(error) => {
            return settle_repository_error(retirements, profile, error);
        }
    };

    if let Some(reason) = retirement_refusal(cancellation, deadline) {
        return ExtensionServiceProfileRetirementOutcome::Unavailable(reason);
    }
    match repository.revalidate_profile_package_absence(profile, package_absence) {
        Ok(()) => {}
        Err(ProfilePackageAbsenceRevalidationError::ObligationsRemain(_)) => {
            return ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::ObligationsRemain,
            );
        }
        Err(ProfilePackageAbsenceRevalidationError::Repository(error)) => {
            return settle_repository_error(retirements, profile, error);
        }
        Err(
            ProfilePackageAbsenceRevalidationError::ProfileMismatch
            | ProfilePackageAbsenceRevalidationError::WrongRepositoryOpen
            | ProfilePackageAbsenceRevalidationError::StaleEvidence,
        ) => {
            return retirements.fail_closed(
                profile,
                ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
            );
        }
        Err(_) => {
            return retirements.fail_closed(
                profile,
                ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
            );
        }
    }

    if let Some(reason) = retirement_refusal(cancellation, deadline) {
        return ExtensionServiceProfileRetirementOutcome::Unavailable(reason);
    }
    let native_absence = match native_recovery.profile_absence_until(profile, deadline) {
        Ok(evidence) => evidence,
        Err(ExtensionRuntimeHostProfileAbsenceDisposition::ObligationsRemain) => {
            return ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::ObligationsRemain,
            );
        }
        Err(ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut) => {
            return ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::DeadlineReached,
            );
        }
        Err(ExtensionRuntimeHostProfileAbsenceDisposition::Unavailable) => {
            return ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::NativeRuntimeUnavailable,
            );
        }
        Err(ExtensionRuntimeHostProfileAbsenceDisposition::InvariantFailed) => {
            return retirements.fail_closed(
                profile,
                ExtensionServiceProfileRetirementFailureReason::NativeRuntimeInvariant,
            );
        }
        Err(_) => {
            return retirements.fail_closed(
                profile,
                ExtensionServiceProfileRetirementFailureReason::NativeRuntimeInvariant,
            );
        }
    };
    if !native_absence.is_for_profile(profile) || native_absence.fence_generation() == 0 {
        drop(native_absence);
        return retirements.fail_closed(
            profile,
            ExtensionServiceProfileRetirementFailureReason::NativeRuntimeInvariant,
        );
    }

    // The opaque proof remains live while the worker commits its sticky local
    // tombstone, joining native absence with the durable checks above. Neither
    // proof nor a copied boolean can authorize deletion on its own.
    let outcome = retirements.mark_retired(profile);
    drop(native_absence);
    outcome
}

#[allow(unreachable_patterns)]
const fn runtime_retirement_unavailable(
    reason: RuntimeRetirementUnavailableReason,
) -> Result<
    ExtensionServiceProfileRetirementUnavailableReason,
    ExtensionServiceProfileRetirementFailureReason,
> {
    Ok(match reason {
        RuntimeRetirementUnavailableReason::DeadlineReached => {
            ExtensionServiceProfileRetirementUnavailableReason::DeadlineReached
        }
        RuntimeRetirementUnavailableReason::StoreNotAdmitted
        | RuntimeRetirementUnavailableReason::StoreObservationPending => {
            ExtensionServiceProfileRetirementUnavailableReason::StoreUnavailable
        }
        RuntimeRetirementUnavailableReason::RepositoryUnavailable => {
            ExtensionServiceProfileRetirementUnavailableReason::RepositoryUnavailable
        }
        RuntimeRetirementUnavailableReason::NativeRetained(_)
        | RuntimeRetirementUnavailableReason::NativeOwnershipUncertain(_)
        | RuntimeRetirementUnavailableReason::PublicationReclaimPending => {
            ExtensionServiceProfileRetirementUnavailableReason::NativeRuntimeUnavailable
        }
        RuntimeRetirementUnavailableReason::ActivationReconciliationPending => {
            ExtensionServiceProfileRetirementUnavailableReason::ObligationsRemain
        }
        _ => {
            return Err(ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation);
        }
    })
}

#[allow(unreachable_patterns)]
const fn runtime_retirement_failure(
    reason: RuntimeCoordinatorFailureReason,
) -> ExtensionServiceProfileRetirementFailureReason {
    match reason {
        RuntimeCoordinatorFailureReason::JournalInvalid
        | RuntimeCoordinatorFailureReason::JournalDiverged => {
            ExtensionServiceProfileRetirementFailureReason::OwnershipJournalInvalid
        }
        RuntimeCoordinatorFailureReason::RepositoryInvariant => {
            ExtensionServiceProfileRetirementFailureReason::RepositoryCorrupt
        }
        RuntimeCoordinatorFailureReason::HostInvariant
        | RuntimeCoordinatorFailureReason::NativeCallPanicked => {
            ExtensionServiceProfileRetirementFailureReason::NativeRuntimeInvariant
        }
        RuntimeCoordinatorFailureReason::GenerationExhausted
        | RuntimeCoordinatorFailureReason::RetainedBytesOverflow
        | RuntimeCoordinatorFailureReason::InternalProtocolViolation => {
            ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation
        }
        _ => ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
    }
}

fn retirement_refusal(
    cancellation: &impl CancellationCheck,
    deadline: Instant,
) -> Option<ExtensionServiceProfileRetirementUnavailableReason> {
    if cancellation.is_cancelled() {
        Some(ExtensionServiceProfileRetirementUnavailableReason::CancellationRequested)
    } else if cancellation.deadline_reached(deadline) {
        Some(ExtensionServiceProfileRetirementUnavailableReason::DeadlineReached)
    } else {
        None
    }
}

const fn cleanup_unavailable(
    reason: CleanupUnavailable,
) -> ExtensionServiceProfileRetirementUnavailableReason {
    match reason {
        CleanupUnavailable::Cancelled => {
            ExtensionServiceProfileRetirementUnavailableReason::CancellationRequested
        }
        CleanupUnavailable::DeadlineExpired => {
            ExtensionServiceProfileRetirementUnavailableReason::DeadlineReached
        }
        CleanupUnavailable::RepositoryLocked
        | CleanupUnavailable::RepositoryInUse
        | CleanupUnavailable::RepositoryIo
        | CleanupUnavailable::RepositoryRecoveryPending => {
            ExtensionServiceProfileRetirementUnavailableReason::RepositoryUnavailable
        }
        CleanupUnavailable::StoreNotAdmitted | CleanupUnavailable::StoreObservationPending => {
            ExtensionServiceProfileRetirementUnavailableReason::StoreUnavailable
        }
        CleanupUnavailable::NativeRuntimeUnavailable
        | CleanupUnavailable::NativeRuntimeCapacity => {
            ExtensionServiceProfileRetirementUnavailableReason::NativeRuntimeUnavailable
        }
    }
}

const fn cleanup_failure(reason: CleanupFailure) -> ExtensionServiceProfileRetirementFailureReason {
    match reason {
        CleanupFailure::UnsafeRepository
        | CleanupFailure::RepositoryCorrupt
        | CleanupFailure::RepositoryRecoveryAmbiguous
        | CleanupFailure::PackagePinMismatch => {
            ExtensionServiceProfileRetirementFailureReason::RepositoryCorrupt
        }
        CleanupFailure::StoreJournalLoadFailed
        | CleanupFailure::StoreJournalInvalid
        | CleanupFailure::StoreMutationInvariant
        | CleanupFailure::StoreProjectionMismatch
        | CleanupFailure::InvalidJournalTransition => {
            ExtensionServiceProfileRetirementFailureReason::OwnershipJournalInvalid
        }
        CleanupFailure::NativeBindingInvalid | CleanupFailure::NativeHostInvariant => {
            ExtensionServiceProfileRetirementFailureReason::NativeRuntimeInvariant
        }
        CleanupFailure::UnsupportedPlatform
        | CleanupFailure::ConcurrentPackageLease
        | CleanupFailure::FrontierLimitExceeded => {
            ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation
        }
    }
}

fn settle_repository_error(
    retirements: &mut ProfileRetirementRegistry,
    profile: ProfileId,
    error: ExtensionRepositoryError,
) -> ExtensionServiceProfileRetirementOutcome {
    match error {
        ExtensionRepositoryError::FileSystem(
            PrivateFsError::LockUnavailable | PrivateFsError::InUse | PrivateFsError::Io,
        ) => ExtensionServiceProfileRetirementOutcome::Unavailable(
            ExtensionServiceProfileRetirementUnavailableReason::RepositoryUnavailable,
        ),
        ExtensionRepositoryError::CallbackReentry => retirements.fail_closed(
            profile,
            ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
        ),
        _ => retirements.fail_closed(
            profile,
            ExtensionServiceProfileRetirementFailureReason::RepositoryCorrupt,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_extension_runtime_api::ExtensionRuntimeFailure;

    #[test]
    fn registry_is_bounded_monotonic_and_never_evicts() {
        let mut registry = ProfileRetirementRegistry::new();
        for value in 0..MAX_PROFILE_RETIREMENT_STATES {
            let profile = ProfileId::from(value as u128 + 1);
            assert_eq!(registry.begin(profile), ProfileRetirementBegin::Proceed);
            assert!(registry.blocks_ingress(profile));
        }
        assert_eq!(
            registry.begin(ProfileId::from(u128::MAX)),
            ProfileRetirementBegin::CapacityExhausted
        );
        assert!(registry.blocks_ingress(ProfileId::from(u128::MAX)));
        for value in 0..MAX_PROFILE_RETIREMENT_STATES {
            assert_eq!(
                registry.state(ProfileId::from(value as u128 + 1)),
                Some(ProfileRetirementState::Fencing)
            );
        }
    }

    #[test]
    fn retired_and_failed_closed_tombstones_are_sticky() {
        let mut registry = ProfileRetirementRegistry::new();
        let retired = ProfileId::from(1);
        let failed = ProfileId::from(2);
        assert_eq!(registry.begin(retired), ProfileRetirementBegin::Proceed);
        assert_eq!(
            registry.mark_retired(retired),
            ExtensionServiceProfileRetirementOutcome::Retired
        );
        assert_eq!(registry.begin(retired), ProfileRetirementBegin::Retired);
        assert_eq!(
            registry.fail_closed(
                retired,
                ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
            ),
            ExtensionServiceProfileRetirementOutcome::Retired
        );

        assert_eq!(registry.begin(failed), ProfileRetirementBegin::Proceed);
        let reason = ExtensionServiceProfileRetirementFailureReason::NativeRuntimeInvariant;
        assert_eq!(
            registry.fail_closed(failed, reason),
            ExtensionServiceProfileRetirementOutcome::FailedClosed(reason)
        );
        assert_eq!(
            registry.begin(failed),
            ProfileRetirementBegin::FailedClosed(reason)
        );
        assert_eq!(
            registry.mark_retired(failed),
            ExtensionServiceProfileRetirementOutcome::FailedClosed(reason)
        );
    }

    #[test]
    fn coordinator_unavailability_never_becomes_profile_absence() {
        use ExtensionServiceProfileRetirementUnavailableReason as Public;
        use RuntimeRetirementUnavailableReason as Private;

        for (private, public) in [
            (Private::DeadlineReached, Public::DeadlineReached),
            (Private::StoreNotAdmitted, Public::StoreUnavailable),
            (Private::StoreObservationPending, Public::StoreUnavailable),
            (
                Private::RepositoryUnavailable,
                Public::RepositoryUnavailable,
            ),
            (
                Private::NativeRetained(ExtensionRuntimeFailure::Internal),
                Public::NativeRuntimeUnavailable,
            ),
            (
                Private::NativeOwnershipUncertain(ExtensionRuntimeFailure::Internal),
                Public::NativeRuntimeUnavailable,
            ),
            (
                Private::PublicationReclaimPending,
                Public::NativeRuntimeUnavailable,
            ),
            (
                Private::ActivationReconciliationPending,
                Public::ObligationsRemain,
            ),
        ] {
            assert_eq!(runtime_retirement_unavailable(private), Ok(public));
        }
    }

    #[test]
    fn every_coordinator_failure_projects_to_a_sticky_profile_failure() {
        use ExtensionServiceProfileRetirementFailureReason as Public;
        use RuntimeCoordinatorFailureReason as Private;

        for (private, public) in [
            (
                Private::GenerationExhausted,
                Public::InternalProtocolViolation,
            ),
            (Private::JournalInvalid, Public::OwnershipJournalInvalid),
            (Private::JournalDiverged, Public::OwnershipJournalInvalid),
            (Private::RepositoryInvariant, Public::RepositoryCorrupt),
            (Private::HostInvariant, Public::NativeRuntimeInvariant),
            (Private::NativeCallPanicked, Public::NativeRuntimeInvariant),
            (
                Private::RetainedBytesOverflow,
                Public::InternalProtocolViolation,
            ),
            (
                Private::InternalProtocolViolation,
                Public::InternalProtocolViolation,
            ),
        ] {
            assert_eq!(runtime_retirement_failure(private), public);
        }
    }
}
