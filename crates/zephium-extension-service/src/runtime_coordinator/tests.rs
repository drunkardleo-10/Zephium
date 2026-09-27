use super::slot::MAX_COORDINATOR_HOST_COMPANION_RETAINED_BYTES;
use super::*;
use crate::journal_store::{
    JournalBackend, JournalLoadFailure, MAX_JOURNAL_ACTIVATION_AMBIGUITY_ADDITIONAL_RETAINED_BYTES,
    MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES,
};
use std::cell::Cell;
use std::time::{Duration, Instant};
use zephium_core::extensions::{
    ExtensionGrantBrowsingContext, ExtensionNativeOwnershipJournal,
    ExtensionNativeOwnershipJournalMutation,
};
use zephium_core::ids::ExtensionInstallId;
use zephium_core::ports::store::{
    ExtensionNativeOwnershipJournalLoadOutcome, ExtensionNativeOwnershipJournalMutationOutcome,
};
use zephium_extension_runtime_api::MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES;
use zephium_store::ExtensionServiceStoreCallOutcome;

#[derive(Clone, Copy)]
enum ProjectionLoadFixture {
    Loaded,
    NotAdmitted,
    TimedOutAfterAdmission,
}

struct ProjectionBackend {
    outcome: ProjectionLoadFixture,
    load_calls: Cell<u8>,
}

impl ProjectionBackend {
    const fn new(outcome: ProjectionLoadFixture) -> Self {
        Self {
            outcome,
            load_calls: Cell::new(0),
        }
    }
}

impl JournalBackend for ProjectionBackend {
    fn load_until(
        &self,
        _deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipJournalLoadOutcome> {
        self.load_calls.set(self.load_calls.get() + 1);
        match self.outcome {
            ProjectionLoadFixture::Loaded => ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalLoadOutcome::Loaded(
                    ExtensionNativeOwnershipJournal::empty(),
                ),
            ),
            ProjectionLoadFixture::NotAdmitted => ExtensionServiceStoreCallOutcome::NotAdmitted,
            ProjectionLoadFixture::TimedOutAfterAdmission => {
                ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission
            }
        }
    }

    fn mutate_until(
        &self,
        _journal: &ExtensionNativeOwnershipJournal,
        _mutation: ExtensionNativeOwnershipJournalMutation,
        _deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome> {
        panic!("projection-readiness tests never mutate the journal")
    }
}

fn key(profile: u128, install: u128) -> ExtensionNativeOwnershipKey {
    ExtensionNativeOwnershipKey::new(
        ProfileId::from(profile),
        ExtensionInstallId::from(install),
        ExtensionGrantBrowsingContext::Regular,
    )
}

#[test]
fn every_pending_slot_consumes_capacity_without_eviction() {
    let mut coordinator = RuntimeCoordinator::new();
    for install in 1..=MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES {
        coordinator
            .admit_planning_slot(key(1, install as u128))
            .unwrap();
    }
    assert_eq!(
        coordinator.admit_planning_slot(key(1, 99)),
        Err(RuntimeActivationOutcome::CapacityExceeded)
    );
    assert_eq!(
        coordinator.slot_count(),
        MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES
    );
    for install in 1..=MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES {
        assert!(coordinator.slot(key(1, install as u128)).is_some());
    }
}

#[test]
fn duplicate_key_reuses_its_exact_slot_without_spending_capacity() {
    let mut coordinator = RuntimeCoordinator::new();
    let key = key(7, 11);
    coordinator.admit_planning_slot(key).unwrap();
    coordinator.admit_planning_slot(key).unwrap();
    assert_eq!(coordinator.slot_count(), 1);
}

#[test]
fn profile_capacity_is_separate_from_extension_capacity_and_counts_pending_owners() {
    let mut coordinator = RuntimeCoordinator::new();
    let profile_limit = zephium_core::ports::extensions::MAX_EXTENSION_ACTIVE_PROFILES;
    for profile in 1..=profile_limit {
        coordinator
            .admit_planning_slot(key(profile as u128, 1))
            .unwrap();
    }
    assert_eq!(
        coordinator.admit_planning_slot(key(profile_limit as u128 + 1, 1)),
        Err(RuntimeActivationOutcome::CapacityExceeded)
    );
    // Existing profiles can still add extensions beyond the old three-slot cap.
    coordinator.admit_planning_slot(key(1, 2)).unwrap();
    assert_eq!(coordinator.slot_count(), profile_limit + 1);
    assert!(coordinator
        .slot(key(profile_limit as u128 + 1, 1))
        .is_none());
}

#[test]
fn unknown_projection_reloads_and_advances_every_settlement_family() {
    // These are the three production callers of the shared readiness gate:
    // Planned/Begin, PreparingAccess/MayOwn, and exact row-fence settlement.
    for authority_name in ["Planned", "PreparingAccess", "RowFence"] {
        let backend = ProjectionBackend::new(ProjectionLoadFixture::Loaded);
        let mut projection = crate::journal_store::JournalProjection::unknown();
        let readiness = reload_projection_if_unknown(&mut projection, &backend, Instant::now());
        let settlement_calls = Cell::new(0_u8);
        let mut authority = Some(authority_name);

        let settled = run_after_projection_readiness(readiness, || {
            settlement_calls.set(settlement_calls.get() + 1);
            authority.take()
        });

        assert_eq!(settled, Ok(Some(authority_name)));
        assert!(projection.known().is_some());
        assert_eq!(backend.load_calls.get(), 1);
        assert_eq!(settlement_calls.get(), 1);
        assert_eq!(authority, None);
    }
}

#[test]
fn failed_projection_reload_suppresses_settlement_and_retains_authority() {
    for (outcome, expected) in [
        (
            ProjectionLoadFixture::NotAdmitted,
            JournalLoadFailure::NotAdmitted,
        ),
        (
            ProjectionLoadFixture::TimedOutAfterAdmission,
            JournalLoadFailure::TimedOutAfterAdmission,
        ),
    ] {
        for authority_name in ["Planned", "PreparingAccess", "RowFence"] {
            let backend = ProjectionBackend::new(outcome);
            let mut projection = crate::journal_store::JournalProjection::unknown();
            let readiness = reload_projection_if_unknown(&mut projection, &backend, Instant::now());
            let settlement_calls = Cell::new(0_u8);
            let mut authority = Some(authority_name);

            let settled = run_after_projection_readiness(readiness, || {
                settlement_calls.set(settlement_calls.get() + 1);
                authority.take()
            });

            assert_eq!(settled, Err(expected));
            assert!(projection.known().is_none());
            assert_eq!(backend.load_calls.get(), 1);
            assert_eq!(settlement_calls.get(), 0);
            assert_eq!(authority, Some(authority_name));
        }
    }
}

#[test]
fn journal_settlement_barrier_blocks_only_unrelated_ingress() {
    let owner = key(7, 11);
    let unrelated = key(7, 12);

    assert!(!journal_settlement_barrier_blocks(None, unrelated));
    assert!(!journal_settlement_barrier_blocks(Some(owner), owner));
    assert!(journal_settlement_barrier_blocks(Some(owner), unrelated));
}

#[test]
fn profile_and_global_drains_prioritize_the_barrier_owner() {
    let mut coordinator = RuntimeCoordinator::new();
    let first = key(7, 11);
    let second = key(7, 12);
    let barrier = key(7, 13);
    coordinator.admit_planning_slot(first).unwrap();
    coordinator.admit_planning_slot(second).unwrap();
    coordinator.admit_planning_slot(barrier).unwrap();

    assert_eq!(
        coordinator
            .keys_matching_after_barrier(Some(ProfileId::from(7)), Some(barrier))
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        [barrier, first, second]
    );
    assert_eq!(
        coordinator
            .keys_matching_after_barrier(None, Some(barrier))
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        [barrier, first, second]
    );
}

#[test]
fn expired_fresh_activations_never_consume_capacity() {
    let mut coordinator = RuntimeCoordinator::new();
    let expired = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("monotonic clock supports a one-second lookback");

    for install in 1..=MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES {
        assert_eq!(
            coordinator.admit_fresh_before_deadline(key(1, install as u128), expired),
            Err(RuntimeActivationOutcome::Unavailable(
                outcome::RuntimeActivationUnavailableReason::DeadlineReached
            ))
        );
    }
    assert_eq!(coordinator.slot_count(), 0);
}

#[test]
fn runtime_generation_burn_is_monotonic_and_never_wraps() {
    let mut coordinator = RuntimeCoordinator::new();
    assert_eq!(coordinator.burn_generation().unwrap().get(), 1);
    assert_eq!(coordinator.burn_generation().unwrap().get(), 2);

    coordinator.next_generation = ExtensionRuntimeGeneration::new(u64::MAX);
    assert_eq!(coordinator.burn_generation().unwrap().get(), u64::MAX);
    assert_eq!(
        coordinator.burn_generation(),
        Err(RuntimeCoordinatorFailureReason::GenerationExhausted)
    );
}

#[test]
fn fail_stop_is_first_failure_sticky_and_blocks_new_ingress() {
    let mut coordinator = RuntimeCoordinator::new();
    coordinator.enter_fail_stop(RuntimeCoordinatorFailureReason::JournalDiverged);
    coordinator.enter_fail_stop(RuntimeCoordinatorFailureReason::HostInvariant);

    assert_eq!(
        coordinator.admit_planning_slot(key(1, 1)),
        Err(RuntimeActivationOutcome::FailedClosed(
            RuntimeCoordinatorFailureReason::JournalDiverged
        ))
    );
    assert_eq!(coordinator.slot_count(), 0);
}

#[test]
fn empty_profile_retirement_cannot_hide_global_fail_stop() {
    let mut coordinator = RuntimeCoordinator::new();
    coordinator.enter_fail_stop(RuntimeCoordinatorFailureReason::JournalDiverged);

    assert_eq!(
        coordinator.profile_retirement_preflight(ProfileId::from(77)),
        Some(RuntimeRetirementOutcome::FailedClosed(
            RuntimeCoordinatorFailureReason::JournalDiverged
        ))
    );
}

#[test]
fn acquisition_plan_refusals_are_terminal_and_identity_specific() {
    use zephium_extension_repository::BundledRuntimeAcquisitionPlanRefusalReason as Reason;

    assert_eq!(
        activation::acquisition_plan_refusal_failure(Reason::WrongRepositoryOpen),
        RuntimeCoordinatorFailureReason::RepositoryInvariant
    );
    assert_eq!(
        activation::acquisition_plan_refusal_failure(Reason::AppliedOwnershipMismatch),
        RuntimeCoordinatorFailureReason::JournalDiverged
    );
}

#[test]
fn repository_release_retries_only_explicitly_transient_failures() {
    use retirement::RepositoryReleaseErrorDisposition::{FailStop, Retry};
    use zephium_extension_repository::{
        BundledPackageLeaseReleaseError as ReleaseError, ExtensionRepositoryError,
    };
    use zephium_private_fs::PrivateFsError;

    for transient in [
        ReleaseError::BuildInProgress,
        ReleaseError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::LockUnavailable,
        )),
        ReleaseError::Repository(ExtensionRepositoryError::FileSystem(PrivateFsError::InUse)),
        ReleaseError::Repository(ExtensionRepositoryError::FileSystem(PrivateFsError::Io)),
    ] {
        assert_eq!(
            retirement::repository_release_error_disposition(transient),
            Retry
        );
    }

    for terminal in [
        ReleaseError::ConcurrentLease,
        ReleaseError::WrongRepository,
        ReleaseError::JournalPinMismatch,
        ReleaseError::Repository(ExtensionRepositoryError::SettlementAmbiguous),
        ReleaseError::Repository(ExtensionRepositoryError::RecoveryAmbiguous),
        ReleaseError::Repository(ExtensionRepositoryError::Sealed),
        ReleaseError::Repository(ExtensionRepositoryError::StateCorrupt),
        ReleaseError::Repository(ExtensionRepositoryError::CallbackReentry),
        ReleaseError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::PrimitiveUnavailable,
        )),
    ] {
        assert_eq!(
            retirement::repository_release_error_disposition(terminal),
            FailStop
        );
    }
}

#[test]
fn manifest_binding_retries_only_explicitly_transient_repository_failures() {
    use activation::ManifestBindingDisposition::{FailStop, Retry};
    use zephium_extension_repository::{BundledManifestBindingsError, ExtensionRepositoryError};
    use zephium_private_fs::PrivateFsError;

    for transient in [
        BundledManifestBindingsError::BuildInProgress,
        BundledManifestBindingsError::StaleSelection,
        BundledManifestBindingsError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::LockUnavailable,
        )),
        BundledManifestBindingsError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::InUse,
        )),
        BundledManifestBindingsError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::Io,
        )),
    ] {
        assert_eq!(activation::manifest_binding_disposition(&transient), Retry);
    }

    for terminal in [
        BundledManifestBindingsError::Repository(ExtensionRepositoryError::SettlementAmbiguous),
        BundledManifestBindingsError::Repository(ExtensionRepositoryError::RecoveryAmbiguous),
        BundledManifestBindingsError::Repository(ExtensionRepositoryError::StateCorrupt),
        BundledManifestBindingsError::Repository(ExtensionRepositoryError::Sealed),
        BundledManifestBindingsError::Repository(ExtensionRepositoryError::CallbackReentry),
        BundledManifestBindingsError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::PrimitiveUnavailable,
        )),
    ] {
        assert_eq!(
            activation::manifest_binding_disposition(&terminal),
            FailStop
        );
    }
}

#[test]
fn runtime_planning_classifies_capacity_transience_and_invariants_explicitly() {
    use activation::RuntimePlanningDisposition::{
        FailStop, PackageUnavailable, RetainedBytesExceeded, Retry,
    };
    use zephium_extension_repository::{BundledPackageLeaseError, ExtensionRepositoryError};
    use zephium_private_fs::PrivateFsError;

    for transient in [
        BundledPackageLeaseError::BuildInProgress,
        BundledPackageLeaseError::StaleSelection,
        BundledPackageLeaseError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::Io,
        )),
    ] {
        assert_eq!(activation::runtime_planning_disposition(&transient), Retry);
    }
    assert_eq!(
        activation::runtime_planning_disposition(&BundledPackageLeaseError::CapacityExhausted),
        RetainedBytesExceeded
    );
    assert_eq!(
        activation::runtime_planning_disposition(&BundledPackageLeaseError::NoCurrentSelection),
        PackageUnavailable
    );
    for terminal in [
        BundledPackageLeaseError::RetainedBytesOverflow,
        BundledPackageLeaseError::OwnerConflict,
        BundledPackageLeaseError::LeaseAlreadyOpen,
        BundledPackageLeaseError::Repository(ExtensionRepositoryError::SettlementAmbiguous),
    ] {
        assert!(matches!(
            activation::runtime_planning_disposition(&terminal),
            FailStop(_)
        ));
    }
}

#[test]
fn package_access_build_classification_preserves_accounting_and_invariants() {
    use activation::PackageAccessBuildDisposition::{FailStop, Reject};
    use outcome::{
        RuntimeActivationRejectionReason as RejectReason, RuntimeCoordinatorFailureReason,
    };
    use zephium_extension_repository::BundledRuntimePackageAccessBuildError as Error;
    use zephium_extension_runtime_api::{
        ExtensionPackageAccessBuildError, ExtensionRuntimeResourceBuildError,
        ExtensionRuntimeResourcePlanBuildError,
    };

    assert_eq!(
        activation::package_access_build_disposition(Error::UnsupportedRuntimeTarget),
        Reject(RejectReason::HostUnsupported)
    );
    for retained in [
        Error::RetainedBytesExceeded,
        Error::PackageAccess(ExtensionPackageAccessBuildError::RetainedBytesExceeded),
        Error::ResourcePlan(
            ExtensionRuntimeResourcePlanBuildError::RetainedBytesExceeded {
                retained_bytes: 2,
                maximum: 1,
            },
        ),
    ] {
        assert_eq!(
            activation::package_access_build_disposition(retained),
            Reject(RejectReason::RetainedBytesExceeded)
        );
    }
    for overflow in [
        Error::RetainedBytesOverflow,
        Error::PackageAccess(ExtensionPackageAccessBuildError::RetainedBytesOverflow),
        Error::ResourcePlan(ExtensionRuntimeResourcePlanBuildError::AccountingOverflow),
    ] {
        assert_eq!(
            activation::package_access_build_disposition(overflow),
            FailStop(RuntimeCoordinatorFailureReason::RetainedBytesOverflow)
        );
    }
    assert_eq!(
        activation::package_access_build_disposition(Error::ResourceBinding {
            ordinal: 0,
            error: ExtensionRuntimeResourceBuildError::InvalidPath,
        }),
        Reject(RejectReason::PackageUnavailable)
    );
    assert_eq!(
        activation::package_access_build_disposition(Error::InternalBindingMismatch),
        FailStop(RuntimeCoordinatorFailureReason::RepositoryInvariant)
    );
}

#[test]
fn may_own_refusals_preserve_store_failure_taxonomy() {
    use activation::MayOwnRefusalDisposition::{FailStop, Rollback};
    use outcome::{RuntimeActivationRejectionReason as Rejection, RuntimeCoordinatorFailureReason};
    use zephium_core::extensions::ExtensionRuntimeEligibilityDenial;
    use zephium_core::ports::store::ExtensionNativeOwnershipActivationStale;

    for reason in [
        crate::journal_store::JournalActivationRefusalReason::SessionRecoveryRequired,
        crate::journal_store::JournalActivationRefusalReason::Invalid,
        crate::journal_store::JournalActivationRefusalReason::RevisionExhausted,
        crate::journal_store::JournalActivationRefusalReason::Failed,
    ] {
        assert_eq!(
            activation::may_own_refusal_disposition(reason),
            FailStop(RuntimeCoordinatorFailureReason::JournalInvalid)
        );
    }
    for reason in [
        crate::journal_store::JournalActivationRefusalReason::NotRegistered,
        crate::journal_store::JournalActivationRefusalReason::DegradedProfile,
    ] {
        assert_eq!(
            activation::may_own_refusal_disposition(reason),
            Rollback(Rejection::ProfileUnavailable)
        );
    }
    assert_eq!(
        activation::may_own_refusal_disposition(
            crate::journal_store::JournalActivationRefusalReason::Stale(
                ExtensionNativeOwnershipActivationStale::CatalogRevision,
            ),
        ),
        Rollback(Rejection::StoreCohortChanged)
    );
    assert_eq!(
        activation::may_own_refusal_disposition(
            crate::journal_store::JournalActivationRefusalReason::EligibilityChanged(
                ExtensionRuntimeEligibilityDenial::RequiredAuthorityMissing,
            ),
        ),
        Rollback(Rejection::StoreCohortChanged)
    );
    assert_eq!(
        activation::may_own_refusal_disposition(
            crate::journal_store::JournalActivationRefusalReason::LimitReached,
        ),
        Rollback(Rejection::RetainedBytesExceeded)
    );
}

#[test]
fn publication_revalidates_both_authorization_and_every_attempt() {
    use activation::PublicationRevalidationStage as Stage;

    assert!(activation::publication_revalidation_required(
        Stage::PendingAuthorization
    ));
    assert!(activation::publication_revalidation_required(
        Stage::PublicationAttempt
    ));
    assert!(!activation::publication_revalidation_required(
        Stage::Published
    ));
}

#[test]
fn divergent_pending_row_suppresses_authorization_dispatch() {
    use activation::{PublicationRevalidationFailure as Failure, PublicationRevalidationStage};
    use std::cell::Cell;

    let authorization_calls = Cell::new(0_u8);
    let result = activation::run_after_publication_revalidation(
        PublicationRevalidationStage::PendingAuthorization,
        Ok(false),
        || authorization_calls.set(authorization_calls.get() + 1),
    );

    assert_eq!(result, Err(Failure::Diverged));
    assert_eq!(authorization_calls.get(), 0);
}

#[test]
fn publication_request_after_yield_revalidates_before_dispatch() {
    use activation::{PublicationRevalidationFailure as Failure, PublicationRevalidationStage};
    use std::cell::Cell;

    let authorization_calls = Cell::new(0_u8);
    let publish_calls = Cell::new(0_u8);
    assert_eq!(
        activation::run_after_publication_revalidation(
            PublicationRevalidationStage::PendingAuthorization,
            Ok(true),
            || {
                authorization_calls.set(authorization_calls.get() + 1);
                "request"
            },
        ),
        Ok("request")
    );

    // The request is retained across the coordinator yield. A divergent row
    // on the next drive must stop before the publication port is entered.
    assert_eq!(
        activation::run_after_publication_revalidation(
            PublicationRevalidationStage::PublicationAttempt,
            Ok(false),
            || publish_calls.set(publish_calls.get() + 1),
        ),
        Err(Failure::Diverged)
    );
    assert_eq!(authorization_calls.get(), 1);
    assert_eq!(publish_calls.get(), 0);
}

#[test]
fn unavailable_publication_retry_revalidates_and_suppresses_second_dispatch() {
    use activation::{PublicationRevalidationFailure as Failure, PublicationRevalidationStage};
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TestAttempt {
        Unavailable,
    }

    let publish_calls = Cell::new(0_u8);
    assert_eq!(
        activation::run_after_publication_revalidation(
            PublicationRevalidationStage::PublicationAttempt,
            Ok(true),
            || {
                publish_calls.set(publish_calls.get() + 1);
                TestAttempt::Unavailable
            },
        ),
        Ok(TestAttempt::Unavailable)
    );

    // `Unavailable` retains the same PublicationRequest for a retry. The
    // subsequent divergent reload must not invoke the port a second time.
    assert_eq!(
        activation::run_after_publication_revalidation(
            PublicationRevalidationStage::PublicationAttempt,
            Ok(false),
            || {
                publish_calls.set(publish_calls.get() + 1);
                TestAttempt::Unavailable
            },
        ),
        Err(Failure::Diverged)
    );
    assert_eq!(publish_calls.get(), 1);
}

#[test]
fn prehost_fail_stop_is_not_misreported_as_attached() {
    let panic_key = key(9, 4);
    let mut coordinator = RuntimeCoordinator::new();
    coordinator.admit_planning_slot(panic_key).unwrap();
    let prior = coordinator
        .slot_mut(panic_key)
        .and_then(RuntimeSlot::take_state)
        .expect("planning state exists");
    assert!(coordinator.slot_mut(panic_key).is_some_and(|slot| {
        slot.enter_fail_stop(
            RuntimeCoordinatorFailureReason::NativeCallPanicked,
            slot::RuntimeFailStopRetained::Prior(Box::new(prior)),
        )
    }));
    coordinator.enter_fail_stop(RuntimeCoordinatorFailureReason::NativeCallPanicked);

    assert!(!coordinator.has_attached_obligation());
    assert_eq!(
        coordinator.admit_planning_slot(key(9, 5)),
        Err(RuntimeActivationOutcome::FailedClosed(
            RuntimeCoordinatorFailureReason::NativeCallPanicked
        ))
    );
}

#[test]
fn profile_and_global_obligation_queries_are_exact() {
    let mut coordinator = RuntimeCoordinator::new();
    coordinator.admit_planning_slot(key(3, 1)).unwrap();
    coordinator.admit_planning_slot(key(5, 1)).unwrap();

    assert!(coordinator.has_obligation());
    assert!(coordinator.has_profile_obligation(ProfileId::from(3)));
    assert!(coordinator.has_profile_obligation(ProfileId::from(5)));
    assert!(!coordinator.has_profile_obligation(ProfileId::from(4)));
    assert!(!coordinator.has_attached_obligation());
}

#[test]
fn host_companion_reserves_every_coordinator_and_row_fence_allocation() {
    const {
        assert!(MAX_COORDINATOR_HOST_COMPANION_RETAINED_BYTES > size_of::<RuntimeSlot>());
        assert!(
            MAX_COORDINATOR_HOST_COMPANION_RETAINED_BYTES
                >= MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES
        );
        assert!(
            MAX_COORDINATOR_HOST_COMPANION_RETAINED_BYTES
                < MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        );
    }
}

#[test]
fn pre_host_companion_reserves_journal_and_rollback_wrappers() {
    const {
        assert!(
            slot::MAX_COORDINATOR_PRE_HOST_COMPANION_RETAINED_BYTES
                >= MAX_JOURNAL_ACTIVATION_AMBIGUITY_ADDITIONAL_RETAINED_BYTES
        );
        assert!(
            slot::MAX_COORDINATOR_PRE_HOST_COMPANION_RETAINED_BYTES
                >= MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES
        );
        assert!(
            slot::MAX_COORDINATOR_PRE_HOST_COMPANION_RETAINED_BYTES
                < MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        );
    }
}

#[test]
fn composite_pre_host_reservation_has_exact_one_over_and_overflow_boundaries() {
    let raw_refusal =
        zephium_extension_repository::MAX_BUNDLED_RUNTIME_PRE_HOST_REFUSAL_ADDITIONAL_RETAINED_BYTES;
    let service = crate::repository::MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES;
    let coordinator = slot::MAX_COORDINATOR_PRE_HOST_COMPANION_RETAINED_BYTES;
    let fixed = raw_refusal
        .checked_add(service)
        .and_then(|bytes| bytes.checked_add(coordinator))
        .expect("derived fixed reservations fit usize");
    let nominal_exact = MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        .checked_sub(fixed)
        .expect("fixed pre-host controls fit the per-owner ceiling");

    assert_eq!(
        nominal_exact
            .checked_add(raw_refusal)
            .and_then(|bytes| bytes.checked_add(service))
            .and_then(|bytes| bytes.checked_add(coordinator)),
        Some(MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES)
    );
    assert!(nominal_exact
        .checked_add(1)
        .and_then(|bytes| bytes.checked_add(raw_refusal))
        .and_then(|bytes| bytes.checked_add(service))
        .and_then(|bytes| bytes.checked_add(coordinator))
        .is_some_and(|bytes| bytes > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES));
    assert_eq!(usize::MAX.checked_add(fixed), None);
}

#[test]
fn pre_host_lease_rollback_fences_absence_before_release_conversion() {
    use zephium_core::extensions::{
        ExtensionNativeOwnershipIntent as Intent, ExtensionNativeOwnershipPhase as Phase,
    };

    assert_eq!(
        retirement::release_action(
            Intent::Acquire,
            Phase::NativeAbsentPreparing,
            retirement::ReleaseAuthorityClass::Lease,
        ),
        Ok(retirement::ReleaseAction::FenceDefiniteAbsence)
    );
    assert_eq!(
        retirement::release_action(
            Intent::Release,
            Phase::NativeAbsentReleasePending,
            retirement::ReleaseAuthorityClass::Lease,
        ),
        Ok(retirement::ReleaseAction::ConvertLease)
    );
}

#[test]
fn settled_repository_pin_reenters_no_pin_state_before_row_clear() {
    use zephium_core::extensions::{
        ExtensionNativeOwnershipIntent as Intent, ExtensionNativeOwnershipPhase as Phase,
    };

    assert_eq!(
        retirement::release_action(
            Intent::Release,
            Phase::NativeAbsentReleasePending,
            retirement::ReleaseAuthorityClass::Repository,
        ),
        Ok(retirement::ReleaseAction::SettleRepositoryPin)
    );
    assert_eq!(
        retirement::release_action(
            Intent::Release,
            Phase::NativeAbsentReleasePending,
            retirement::ReleaseAuthorityClass::NoRepositoryPin,
        ),
        Ok(retirement::ReleaseAction::ClearDurableRow)
    );
}

#[test]
fn post_absence_reload_requires_an_exact_current_row_before_reclaim() {
    use crate::journal_store::JournalLoadFailure;
    use outcome::RuntimeRetirementUnavailableReason as Wait;
    use retirement::PostAbsenceReloadDisposition as Disposition;

    assert_eq!(
        retirement::classify_post_absence_reload(Ok(true)),
        Disposition::Verified
    );
    assert_eq!(
        retirement::classify_post_absence_reload(Err(JournalLoadFailure::NotAdmitted)),
        Disposition::Wait(Wait::StoreNotAdmitted)
    );
    assert_eq!(
        retirement::classify_post_absence_reload(Err(JournalLoadFailure::TimedOutAfterAdmission)),
        Disposition::Wait(Wait::StoreObservationPending)
    );
    assert_eq!(
        retirement::classify_post_absence_reload(Ok(false)),
        Disposition::Fail(RuntimeCoordinatorFailureReason::JournalDiverged)
    );
    assert_eq!(
        retirement::classify_post_absence_reload(Err(JournalLoadFailure::Failed)),
        Disposition::Fail(RuntimeCoordinatorFailureReason::JournalInvalid)
    );
}

#[test]
fn activation_rollback_preserves_its_terminal_rejection() {
    let rejection = outcome::RuntimeActivationRejectionReason::HostUnsupported;
    assert_eq!(
        activation::activation_completion_outcome(slot::ReleaseCompletion::ActivationRejected(
            rejection
        )),
        RuntimeActivationOutcome::Rejected(rejection)
    );
    assert_eq!(
        activation::activation_completion_outcome(slot::ReleaseCompletion::ActivationUnavailable(
            outcome::RuntimeActivationUnavailableReason::NativeRetryable(
                zephium_extension_runtime_api::ExtensionRuntimeFailure::Internal
            )
        )),
        RuntimeActivationOutcome::Unavailable(
            outcome::RuntimeActivationUnavailableReason::NativeRetryable(
                zephium_extension_runtime_api::ExtensionRuntimeFailure::Internal
            )
        )
    );
    assert_eq!(
        activation::activation_completion_outcome(slot::ReleaseCompletion::Retired),
        RuntimeActivationOutcome::Unavailable(
            outcome::RuntimeActivationUnavailableReason::RetirementInProgress
        )
    );
}

#[test]
fn host_binding_policy_retries_capacity_but_rolls_back_terminal_refusals() {
    use zephium_extension_repository::BundledRuntimeHostActivationBindingError as Error;
    use zephium_extension_runtime_api::{
        ExtensionRuntimeHostActivationBindingError as BindingError,
        ExtensionRuntimeHostBindError as HostError,
    };

    assert_eq!(
        activation::host_binding_disposition(Error::RuntimeHostFactory(HostError::Unavailable)),
        activation::HostBindingDisposition::Retry(
            outcome::RuntimeActivationUnavailableReason::HostUnavailable(HostError::Unavailable)
        )
    );
    assert_eq!(
        activation::host_binding_disposition(Error::RuntimeHostFactory(
            HostError::CapacityExceeded
        )),
        activation::HostBindingDisposition::Retry(
            outcome::RuntimeActivationUnavailableReason::HostUnavailable(
                HostError::CapacityExceeded
            )
        )
    );
    for terminal in [HostError::OwnerConflict, HostError::Sealed] {
        assert_eq!(
            activation::host_binding_disposition(Error::RuntimeHostFactory(terminal)),
            activation::HostBindingDisposition::Fail(
                RuntimeCoordinatorFailureReason::HostInvariant
            )
        );
    }
    assert_eq!(
        activation::host_binding_disposition(Error::RuntimeHostFactory(
            HostError::UnsupportedBackend
        )),
        activation::HostBindingDisposition::Reject(
            outcome::RuntimeActivationRejectionReason::HostUnsupported
        )
    );
    assert_eq!(
        activation::host_binding_disposition(Error::RetainedBytesExceeded),
        activation::HostBindingDisposition::Reject(
            outcome::RuntimeActivationRejectionReason::RetainedBytesExceeded
        )
    );
    assert_eq!(
        activation::host_binding_disposition(Error::RuntimeHost(
            BindingError::RetainedBytesExceeded
        )),
        activation::HostBindingDisposition::Reject(
            outcome::RuntimeActivationRejectionReason::RetainedBytesExceeded
        )
    );
    assert_eq!(
        activation::host_binding_disposition(Error::RuntimeHost(
            BindingError::RetainedBytesOverflow
        )),
        activation::HostBindingDisposition::Fail(
            RuntimeCoordinatorFailureReason::RetainedBytesOverflow
        )
    );
}

#[test]
fn publication_and_reclaim_retry_only_transient_host_failures() {
    use zephium_extension_runtime_api::{
        ExtensionRuntimeHostBindError as Error, ExtensionRuntimePublicationReclaimError,
    };

    assert_eq!(
        activation::publication_failure_disposition(Error::Unavailable),
        activation::PublicationFailureDisposition::Retry
    );
    for terminal in [Error::CapacityExceeded, Error::OwnerConflict, Error::Sealed] {
        assert_eq!(
            activation::publication_failure_disposition(terminal),
            activation::PublicationFailureDisposition::Fail(
                RuntimeCoordinatorFailureReason::HostInvariant
            )
        );
    }
    assert_eq!(
        activation::publication_failure_disposition(Error::InternalInvariant),
        activation::PublicationFailureDisposition::Fail(
            RuntimeCoordinatorFailureReason::HostInvariant
        )
    );
    assert_eq!(
        retirement::reclaim_failure_disposition(ExtensionRuntimePublicationReclaimError::Host(
            Error::Unavailable
        )),
        retirement::ReclaimFailureDisposition::Retry
    );
    for terminal in [Error::CapacityExceeded, Error::OwnerConflict, Error::Sealed] {
        assert_eq!(
            retirement::reclaim_failure_disposition(ExtensionRuntimePublicationReclaimError::Host(
                terminal,
            )),
            retirement::ReclaimFailureDisposition::Fail(
                RuntimeCoordinatorFailureReason::HostInvariant
            )
        );
    }
    assert_eq!(
        retirement::reclaim_failure_disposition(ExtensionRuntimePublicationReclaimError::Host(
            Error::InternalInvariant
        )),
        retirement::ReclaimFailureDisposition::Fail(RuntimeCoordinatorFailureReason::HostInvariant)
    );
}
