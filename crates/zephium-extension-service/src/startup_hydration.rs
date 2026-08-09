//! Startup-only activation of durable enabled extension runtimes.
//!
//! Cleanup and hydration are separate frontiers. Once cleanup has proven the
//! durable journal safe, retries must resume hydration directly: running crash
//! recovery again would race runtime owners already acquired earlier in the
//! same hydration attempt.

use std::time::Instant;

use zephium_core::extensions::{
    ExtensionNativeOwnershipJournalRevision, ExtensionNativeOwnershipKey,
};
use zephium_core::ids::ProfileId;
use zephium_core::ports::extensions::ExtensionActiveProfiles;
use zephium_store::{
    ExtensionRuntimeStartupInventoryLoadOutcome, ExtensionServiceStoreAuthority,
    ExtensionServiceStoreCallOutcome,
};

use crate::cleanup::CancellationCheck;
use crate::journal_store::JournalProjection;
use crate::native_recovery::NativeRecoveryState;
use crate::repository::ServiceRepository;
use crate::runtime_coordinator::{
    RuntimeActivationOutcome, RuntimeActivationRejectionReason, RuntimeActivationUnavailableReason,
    RuntimeCoordinator, RuntimeCoordinatorFailureReason, RuntimeCoordinatorResources,
};

/// Exact bounded startup degradations retained by the worker for later status
/// projection. None of these selectors or reasons is runtime authority.
#[derive(Debug, Default)]
pub(crate) struct StartupRuntimeHydrationReport {
    active_count: u16,
    active_profiles: ExtensionActiveProfiles,
    rejected: Vec<(
        ExtensionNativeOwnershipKey,
        RuntimeActivationRejectionReason,
    )>,
    capacity_deferred: Vec<ExtensionNativeOwnershipKey>,
    degraded_profiles: Vec<ProfileId>,
}

impl StartupRuntimeHydrationReport {
    pub(crate) const fn active_count(&self) -> u16 {
        self.active_count
    }

    pub(crate) const fn active_profiles(&self) -> ExtensionActiveProfiles {
        self.active_profiles
    }

    pub(crate) fn rejected_count(&self) -> usize {
        self.rejected.len()
    }

    pub(crate) fn capacity_deferred_count(&self) -> usize {
        self.capacity_deferred.len()
    }

    pub(crate) fn degraded_profile_count(&self) -> usize {
        self.degraded_profiles.len()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartupRuntimeHydrationUnavailable {
    Cancelled,
    DeadlineReached,
    StoreNotAdmitted,
    StoreObservationPending,
    Runtime(RuntimeActivationUnavailableReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartupRuntimeHydrationFailure {
    InventoryLoadFailed,
    ProjectionMissing,
    ProfileFenced,
    Coordinator(RuntimeCoordinatorFailureReason),
}

pub(crate) enum StartupRuntimeHydrationOutcome {
    Ready {
        journal_revision: ExtensionNativeOwnershipJournalRevision,
        report: StartupRuntimeHydrationReport,
    },
    Unavailable(StartupRuntimeHydrationUnavailable),
    Failed(StartupRuntimeHydrationFailure),
}

/// Replays the ordinary activation transaction for every enabled regular
/// runtime before application bootstrap may construct profile webviews.
pub(crate) fn hydrate_startup_runtimes(
    store: &ExtensionServiceStoreAuthority,
    projection: &mut JournalProjection,
    repository: &mut ServiceRepository,
    native_recovery: &mut NativeRecoveryState,
    coordinator: &mut RuntimeCoordinator,
    cancellation: &impl CancellationCheck,
    deadline: Instant,
) -> StartupRuntimeHydrationOutcome {
    if cancellation.is_cancelled() {
        return StartupRuntimeHydrationOutcome::Unavailable(
            StartupRuntimeHydrationUnavailable::Cancelled,
        );
    }
    if Instant::now() >= deadline {
        return StartupRuntimeHydrationOutcome::Unavailable(
            StartupRuntimeHydrationUnavailable::DeadlineReached,
        );
    }

    let inventory = match store.load_runtime_startup_inventory_until(deadline) {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionRuntimeStartupInventoryLoadOutcome::Loaded(inventory),
        ) => inventory,
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionRuntimeStartupInventoryLoadOutcome::Failed,
        ) => {
            return StartupRuntimeHydrationOutcome::Failed(
                StartupRuntimeHydrationFailure::InventoryLoadFailed,
            )
        }
        ExtensionServiceStoreCallOutcome::NotAdmitted => {
            return StartupRuntimeHydrationOutcome::Unavailable(
                StartupRuntimeHydrationUnavailable::StoreNotAdmitted,
            )
        }
        ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return StartupRuntimeHydrationOutcome::Unavailable(
                StartupRuntimeHydrationUnavailable::StoreObservationPending,
            )
        }
    };

    let mut report = StartupRuntimeHydrationReport {
        degraded_profiles: inventory.degraded_profiles().to_vec(),
        ..StartupRuntimeHydrationReport::default()
    };
    for &key in inventory.keys() {
        if cancellation.is_cancelled() {
            return StartupRuntimeHydrationOutcome::Unavailable(
                StartupRuntimeHydrationUnavailable::Cancelled,
            );
        }
        if Instant::now() >= deadline {
            return StartupRuntimeHydrationOutcome::Unavailable(
                StartupRuntimeHydrationUnavailable::DeadlineReached,
            );
        }
        let outcome = coordinator.activate_until(
            RuntimeCoordinatorResources::new(store, projection, repository, native_recovery),
            key,
            false,
            deadline,
        );
        match outcome {
            RuntimeActivationOutcome::Activated(_) | RuntimeActivationOutcome::AlreadyActive(_) => {
                let Some(next) = report.active_count.checked_add(1) else {
                    return StartupRuntimeHydrationOutcome::Failed(
                        StartupRuntimeHydrationFailure::ProjectionMissing,
                    );
                };
                report.active_count = next;
                if !report.active_profiles.try_insert(key.profile()) {
                    return StartupRuntimeHydrationOutcome::Failed(
                        StartupRuntimeHydrationFailure::ProjectionMissing,
                    );
                }
            }
            RuntimeActivationOutcome::Rejected(reason) => report.rejected.push((key, reason)),
            RuntimeActivationOutcome::CapacityExceeded => report.capacity_deferred.push(key),
            RuntimeActivationOutcome::Unavailable(reason) => {
                return StartupRuntimeHydrationOutcome::Unavailable(
                    StartupRuntimeHydrationUnavailable::Runtime(reason),
                )
            }
            RuntimeActivationOutcome::ProfileFenced => {
                return StartupRuntimeHydrationOutcome::Failed(
                    StartupRuntimeHydrationFailure::ProfileFenced,
                )
            }
            RuntimeActivationOutcome::FailedClosed(reason) => {
                return StartupRuntimeHydrationOutcome::Failed(
                    StartupRuntimeHydrationFailure::Coordinator(reason),
                )
            }
        }
    }

    let Some(journal_revision) = projection.known().map(|journal| journal.revision()) else {
        return StartupRuntimeHydrationOutcome::Failed(
            StartupRuntimeHydrationFailure::ProjectionMissing,
        );
    };
    StartupRuntimeHydrationOutcome::Ready {
        journal_revision,
        report,
    }
}
