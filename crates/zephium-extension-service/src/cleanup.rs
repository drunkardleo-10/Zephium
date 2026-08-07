//! Cleanup-first startup reconciliation.

use std::time::Instant;

use zephium_core::extensions::{
    ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipIntent,
    ExtensionNativeOwnershipJournalMutation, ExtensionNativeOwnershipJournalRevision,
    ExtensionNativeOwnershipPhase, ExtensionPackagePinReleaseBinding,
    MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES,
};
use zephium_core::ids::ProfileId;
use zephium_extension_repository::{
    BundledPackageBuildSettlementError, BundledPackageLeaseReleaseError,
    BundledPackageLeaseReleaseOutcome, ExtensionRepositoryError,
};
use zephium_private_fs::PrivateFsError;

use crate::journal_store::{
    JournalBackend, JournalLoadFailure, JournalMutationFailure, JournalProjection,
};
use crate::native_recovery::{
    NativeRecoveryFailure, NativeRecoveryState, NativeRecoveryStep, NativeRecoveryUnavailable,
};
use crate::repository::{ServiceRepository, ServiceRepositoryOpenError};

const MAX_CLEANUP_FRONTIERS: usize = MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES * 8 + 16;
const MAX_REPOSITORY_REOPENS_PER_ATTEMPT: usize = 2;

pub(crate) trait CancellationCheck {
    fn is_cancelled(&self) -> bool;

    fn deadline_reached(&self, deadline: Instant) -> bool {
        Instant::now() >= deadline
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CleanupProgress {
    LoadingOwnershipJournal,
    ReconcilingCleanup,
}

/// Exact ownership-journal subset one cleanup attempt may mutate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CleanupScope {
    All,
    Profile(ProfileId),
}

/// Immutable boundaries for one cleanup attempt.
pub(crate) struct CleanupAttempt<'attempt, C: CancellationCheck> {
    scope: CleanupScope,
    cancellation: &'attempt C,
    deadline: Instant,
}

impl<'attempt, C: CancellationCheck> CleanupAttempt<'attempt, C> {
    pub(crate) fn new(scope: CleanupScope, cancellation: &'attempt C, deadline: Instant) -> Self {
        Self {
            scope,
            cancellation,
            deadline,
        }
    }
}

impl CleanupScope {
    fn includes_profile(self, profile: ProfileId) -> bool {
        matches!(self, Self::All) || matches!(self, Self::Profile(expected) if expected == profile)
    }

    fn includes_entry(self, entry: &ExtensionNativeOwnershipEntry) -> bool {
        self.includes_profile(entry.key().profile())
    }
}

pub(crate) trait CleanupRepositoryBackend {
    fn is_open(&self) -> bool;
    fn open(&mut self) -> Result<(), ServiceRepositoryOpenError>;
    fn reopen(&mut self) -> Result<(), ServiceRepositoryOpenError>;
    fn reconcile_release(
        &mut self,
        binding: &ExtensionPackagePinReleaseBinding,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError>;
}

impl CleanupRepositoryBackend for ServiceRepository {
    fn is_open(&self) -> bool {
        self.is_open()
    }

    fn open(&mut self) -> Result<(), ServiceRepositoryOpenError> {
        self.open()
    }

    fn reopen(&mut self) -> Result<(), ServiceRepositoryOpenError> {
        self.reopen()
    }

    fn reconcile_release(
        &mut self,
        binding: &ExtensionPackagePinReleaseBinding,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        self.reconcile_release(binding)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CleanupUnavailable {
    Cancelled,
    DeadlineExpired,
    RepositoryLocked,
    RepositoryInUse,
    RepositoryIo,
    RepositoryRecoveryPending,
    StoreNotAdmitted,
    StoreObservationPending,
    NativeRuntimeUnavailable,
    NativeRuntimeCapacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CleanupFailure {
    UnsupportedPlatform,
    UnsafeRepository,
    RepositoryCorrupt,
    RepositoryRecoveryAmbiguous,
    StoreJournalLoadFailed,
    StoreJournalInvalid,
    StoreMutationInvariant,
    StoreProjectionMismatch,
    InvalidJournalTransition,
    PackagePinMismatch,
    ConcurrentPackageLease,
    NativeBindingInvalid,
    NativeHostInvariant,
    FrontierLimitExceeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CleanupStartupOutcome {
    Ready {
        journal_revision: ExtensionNativeOwnershipJournalRevision,
    },
    CleanupRequired {
        journal_revision: ExtensionNativeOwnershipJournalRevision,
        possible_owner_count: u16,
    },
    Unavailable(CleanupUnavailable),
    Failed(CleanupFailure),
}

enum SettleEntryOutcome {
    Progress,
    Reload,
    Reopen,
    Unavailable(CleanupUnavailable),
    Failed(CleanupFailure),
}

pub(crate) fn reconcile_startup(
    journal_backend: &impl JournalBackend,
    projection: &mut JournalProjection,
    repository: &mut impl CleanupRepositoryBackend,
    mut native_recovery: Option<&mut NativeRecoveryState>,
    attempt: CleanupAttempt<'_, impl CancellationCheck>,
    mut publish_progress: impl FnMut(CleanupProgress),
) -> CleanupStartupOutcome {
    let CleanupAttempt {
        scope,
        cancellation,
        deadline,
    } = attempt;
    if let Some(outcome) = frontier_refusal(cancellation, deadline) {
        return outcome;
    }
    if !repository.is_open() {
        if let Err(error) = repository.open() {
            return classify_open_error(error);
        }
    }
    if let Some(outcome) = frontier_refusal(cancellation, deadline) {
        return outcome;
    }
    publish_progress(CleanupProgress::LoadingOwnershipJournal);
    if let Err(error) = projection.reload(journal_backend, deadline) {
        return classify_load_error(error);
    }
    publish_progress(CleanupProgress::ReconcilingCleanup);

    let mut frontiers = 0_usize;
    let mut repository_reopens = 0_usize;
    loop {
        if let Some(outcome) = frontier_refusal(cancellation, deadline) {
            return outcome;
        }
        frontiers = frontiers.saturating_add(1);
        if frontiers > MAX_CLEANUP_FRONTIERS {
            return CleanupStartupOutcome::Failed(CleanupFailure::FrontierLimitExceeded);
        }

        let Some(journal) = projection.known() else {
            publish_progress(CleanupProgress::LoadingOwnershipJournal);
            if let Err(error) = projection.reload(journal_backend, deadline) {
                return classify_load_error(error);
            }
            publish_progress(CleanupProgress::ReconcilingCleanup);
            continue;
        };
        let native_frontier_profile = match native_recovery.as_ref() {
            Some(recovery) => match recovery.frontier_profile() {
                Ok(profile) => profile,
                Err(reason) => {
                    return CleanupStartupOutcome::Failed(map_native_failure(reason));
                }
            },
            None => None,
        };
        if native_frontier_profile.is_some_and(|profile| scope.includes_profile(profile)) {
            let Some(native_recovery) = native_recovery.as_deref_mut() else {
                return CleanupStartupOutcome::Failed(CleanupFailure::NativeHostInvariant);
            };
            match native_recovery.advance(journal_backend, projection, cancellation, deadline) {
                NativeRecoveryStep::Progress => {
                    repository_reopens = 0;
                    continue;
                }
                NativeRecoveryStep::Reload => {
                    if let Err(error) = projection.reload(journal_backend, deadline) {
                        return classify_load_error(error);
                    }
                    continue;
                }
                NativeRecoveryStep::Unsupported => {
                    return cleanup_required_from_projection(projection, scope);
                }
                NativeRecoveryStep::Unavailable(reason) => {
                    return CleanupStartupOutcome::Unavailable(map_native_unavailable(reason));
                }
                NativeRecoveryStep::Failed(reason) => {
                    return CleanupStartupOutcome::Failed(map_native_failure(reason));
                }
            }
        }

        let definite_absence = journal
            .entries()
            .iter()
            .find(|entry| scope.includes_entry(entry) && native_absence_is_definite(entry))
            .cloned();
        let Some(entry) = definite_absence else {
            let Some(entry) = journal
                .entries()
                .iter()
                .find(|entry| scope.includes_entry(entry) && native_owner_is_possible(entry))
                .cloned()
            else {
                return cleanup_required_from_projection(projection, scope);
            };
            let Some(native_recovery) = native_recovery.as_deref_mut() else {
                return cleanup_required_from_projection(projection, scope);
            };
            if native_recovery.has_frontier() {
                // A scoped attempt must never advance or replace another
                // profile's exact native frontier. Its single recovery slot is
                // retryably unavailable until that unrelated owner settles.
                return CleanupStartupOutcome::Unavailable(
                    CleanupUnavailable::NativeRuntimeCapacity,
                );
            }
            match native_recovery.begin(entry) {
                NativeRecoveryStep::Progress => {
                    repository_reopens = 0;
                    continue;
                }
                NativeRecoveryStep::Reload => {
                    if let Err(error) = projection.reload(journal_backend, deadline) {
                        return classify_load_error(error);
                    }
                    continue;
                }
                NativeRecoveryStep::Unsupported => {
                    return cleanup_required_from_projection(projection, scope);
                }
                NativeRecoveryStep::Unavailable(reason) => {
                    return CleanupStartupOutcome::Unavailable(map_native_unavailable(reason));
                }
                NativeRecoveryStep::Failed(reason) => {
                    return CleanupStartupOutcome::Failed(map_native_failure(reason));
                }
            }
        };

        match settle_definite_absence(
            journal_backend,
            projection,
            repository,
            cancellation,
            deadline,
            entry,
        ) {
            SettleEntryOutcome::Progress => repository_reopens = 0,
            SettleEntryOutcome::Reload => {
                if let Err(error) = projection.reload(journal_backend, deadline) {
                    return classify_load_error(error);
                }
            }
            SettleEntryOutcome::Reopen => {
                repository_reopens = repository_reopens.saturating_add(1);
                if repository_reopens > MAX_REPOSITORY_REOPENS_PER_ATTEMPT {
                    return CleanupStartupOutcome::Failed(
                        CleanupFailure::RepositoryRecoveryAmbiguous,
                    );
                }
                if let Some(outcome) = frontier_refusal(cancellation, deadline) {
                    return outcome;
                }
                if let Err(error) = repository.reopen() {
                    return classify_open_error(error);
                }
                projection.invalidate();
            }
            SettleEntryOutcome::Unavailable(reason) => {
                return CleanupStartupOutcome::Unavailable(reason);
            }
            SettleEntryOutcome::Failed(reason) => {
                return CleanupStartupOutcome::Failed(reason);
            }
        }
    }
}

fn native_absence_is_definite(entry: &ExtensionNativeOwnershipEntry) -> bool {
    matches!(
        (entry.intent(), entry.phase()),
        (
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeAbsentPreparing
        ) | (
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
        )
    )
}

fn native_owner_is_possible(entry: &ExtensionNativeOwnershipEntry) -> bool {
    matches!(
        (entry.intent(), entry.phase()),
        (
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn
                | ExtensionNativeOwnershipPhase::NativeOwned,
        ) | (
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        )
    )
}

fn cleanup_required_from_projection(
    projection: &JournalProjection,
    scope: CleanupScope,
) -> CleanupStartupOutcome {
    let Some(journal) = projection.known() else {
        return CleanupStartupOutcome::Failed(CleanupFailure::StoreJournalInvalid);
    };
    let possible_owner_count = journal
        .entries()
        .iter()
        .filter(|entry| scope.includes_entry(entry))
        .count();
    if possible_owner_count == 0 {
        return CleanupStartupOutcome::Ready {
            journal_revision: journal.revision(),
        };
    }
    let Ok(possible_owner_count) = u16::try_from(possible_owner_count) else {
        return CleanupStartupOutcome::Failed(CleanupFailure::StoreJournalInvalid);
    };
    CleanupStartupOutcome::CleanupRequired {
        journal_revision: journal.revision(),
        possible_owner_count,
    }
}

const fn map_native_unavailable(reason: NativeRecoveryUnavailable) -> CleanupUnavailable {
    match reason {
        NativeRecoveryUnavailable::Cancelled => CleanupUnavailable::Cancelled,
        NativeRecoveryUnavailable::DeadlineExpired => CleanupUnavailable::DeadlineExpired,
        NativeRecoveryUnavailable::StoreNotAdmitted => CleanupUnavailable::StoreNotAdmitted,
        NativeRecoveryUnavailable::StoreObservationPending => {
            CleanupUnavailable::StoreObservationPending
        }
        NativeRecoveryUnavailable::BackendUnavailable => {
            CleanupUnavailable::NativeRuntimeUnavailable
        }
        NativeRecoveryUnavailable::CapacityExceeded => CleanupUnavailable::NativeRuntimeCapacity,
    }
}

const fn map_native_failure(reason: NativeRecoveryFailure) -> CleanupFailure {
    match reason {
        NativeRecoveryFailure::StoreJournalLoadFailed => CleanupFailure::StoreJournalLoadFailed,
        NativeRecoveryFailure::StoreMutationInvariant => CleanupFailure::StoreMutationInvariant,
        NativeRecoveryFailure::StoreProjectionMismatch => CleanupFailure::StoreProjectionMismatch,
        NativeRecoveryFailure::InvalidJournalTransition => CleanupFailure::InvalidJournalTransition,
        NativeRecoveryFailure::InvalidBinding => CleanupFailure::NativeBindingInvalid,
        NativeRecoveryFailure::HostInvariant => CleanupFailure::NativeHostInvariant,
    }
}

fn settle_definite_absence(
    journal_backend: &impl JournalBackend,
    projection: &mut JournalProjection,
    repository: &mut impl CleanupRepositoryBackend,
    cancellation: &impl CancellationCheck,
    deadline: Instant,
    mut entry: ExtensionNativeOwnershipEntry,
) -> SettleEntryOutcome {
    if entry.intent() == ExtensionNativeOwnershipIntent::Acquire {
        let mutation = ExtensionNativeOwnershipJournalMutation::transition(
            entry.cas(),
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
        );
        match projection.mutate(journal_backend, mutation, deadline) {
            Ok(journal) => {
                let Some(updated) = journal.get(entry.key()).cloned() else {
                    return SettleEntryOutcome::Failed(CleanupFailure::StoreProjectionMismatch);
                };
                entry = updated;
            }
            Err(error) => return classify_mutation_error(error),
        }
    }

    if let Some(outcome) = frontier_refusal(cancellation, deadline) {
        return cleanup_refusal(outcome);
    }
    let expected = entry.clone();
    let journal = match projection.reload(journal_backend, deadline) {
        Ok(journal) => journal,
        Err(error) => return cleanup_from_load_error(error),
    };
    if journal.get(entry.key()) != Some(&expected) {
        return SettleEntryOutcome::Reload;
    }
    let binding = match ExtensionPackagePinReleaseBinding::mint(&entry) {
        Ok(binding) => binding,
        Err(_) => return SettleEntryOutcome::Failed(CleanupFailure::InvalidJournalTransition),
    };

    if let Some(outcome) = frontier_refusal(cancellation, deadline) {
        return cleanup_refusal(outcome);
    }
    match repository.reconcile_release(&binding) {
        Ok(
            BundledPackageLeaseReleaseOutcome::Released
            | BundledPackageLeaseReleaseOutcome::AlreadyReleased,
        ) => {}
        Ok(_) => return SettleEntryOutcome::Failed(CleanupFailure::RepositoryCorrupt),
        Err(error) => return classify_release_error(error),
    }

    if let Some(outcome) = frontier_refusal(cancellation, deadline) {
        return cleanup_refusal(outcome);
    }
    let journal = match projection.reload(journal_backend, deadline) {
        Ok(journal) => journal,
        Err(error) => return cleanup_from_load_error(error),
    };
    if journal.get(entry.key()) != Some(&expected) {
        return SettleEntryOutcome::Reload;
    }
    if let Some(outcome) = frontier_refusal(cancellation, deadline) {
        return cleanup_refusal(outcome);
    }
    match projection.mutate(
        journal_backend,
        ExtensionNativeOwnershipJournalMutation::clear(entry.cas()),
        deadline,
    ) {
        Ok(_) => SettleEntryOutcome::Progress,
        Err(error) => classify_mutation_error(error),
    }
}

fn frontier_refusal(
    cancellation: &impl CancellationCheck,
    deadline: Instant,
) -> Option<CleanupStartupOutcome> {
    if cancellation.is_cancelled() {
        Some(CleanupStartupOutcome::Unavailable(
            CleanupUnavailable::Cancelled,
        ))
    } else if cancellation.deadline_reached(deadline) {
        Some(CleanupStartupOutcome::Unavailable(
            CleanupUnavailable::DeadlineExpired,
        ))
    } else {
        None
    }
}

fn cleanup_refusal(outcome: CleanupStartupOutcome) -> SettleEntryOutcome {
    match outcome {
        CleanupStartupOutcome::Unavailable(reason) => SettleEntryOutcome::Unavailable(reason),
        CleanupStartupOutcome::Failed(reason) => SettleEntryOutcome::Failed(reason),
        CleanupStartupOutcome::Ready { .. } | CleanupStartupOutcome::CleanupRequired { .. } => {
            SettleEntryOutcome::Failed(CleanupFailure::InvalidJournalTransition)
        }
    }
}

fn classify_load_error(error: JournalLoadFailure) -> CleanupStartupOutcome {
    match error {
        JournalLoadFailure::NotAdmitted => {
            CleanupStartupOutcome::Unavailable(CleanupUnavailable::StoreNotAdmitted)
        }
        JournalLoadFailure::TimedOutAfterAdmission => {
            CleanupStartupOutcome::Unavailable(CleanupUnavailable::StoreObservationPending)
        }
        JournalLoadFailure::Failed => {
            CleanupStartupOutcome::Failed(CleanupFailure::StoreJournalLoadFailed)
        }
    }
}

fn cleanup_from_load_error(error: JournalLoadFailure) -> SettleEntryOutcome {
    match classify_load_error(error) {
        CleanupStartupOutcome::Unavailable(reason) => SettleEntryOutcome::Unavailable(reason),
        CleanupStartupOutcome::Failed(reason) => SettleEntryOutcome::Failed(reason),
        CleanupStartupOutcome::Ready { .. } | CleanupStartupOutcome::CleanupRequired { .. } => {
            SettleEntryOutcome::Failed(CleanupFailure::StoreJournalInvalid)
        }
    }
}

fn classify_mutation_error(error: JournalMutationFailure) -> SettleEntryOutcome {
    match error {
        JournalMutationFailure::NotAdmitted => {
            SettleEntryOutcome::Unavailable(CleanupUnavailable::StoreNotAdmitted)
        }
        JournalMutationFailure::ReloadRequired => SettleEntryOutcome::Reload,
        JournalMutationFailure::ProjectionMismatch => {
            SettleEntryOutcome::Failed(CleanupFailure::StoreProjectionMismatch)
        }
        JournalMutationFailure::InvalidLocalTransition => {
            SettleEntryOutcome::Failed(CleanupFailure::InvalidJournalTransition)
        }
        JournalMutationFailure::StoreInvariant => {
            SettleEntryOutcome::Failed(CleanupFailure::StoreMutationInvariant)
        }
    }
}

fn classify_open_error(error: ServiceRepositoryOpenError) -> CleanupStartupOutcome {
    match error {
        ServiceRepositoryOpenError::OutstandingAuthority => {
            // Startup cannot manufacture same-open runtime authority, and the
            // live coordinator must drain every such value before handing a
            // reopen frontier to cleanup. Observing one here is therefore an
            // internal lifecycle-ordering violation, not a retryable private-
            // namespace lock held by another process.
            CleanupStartupOutcome::Failed(CleanupFailure::ConcurrentPackageLease)
        }
        ServiceRepositoryOpenError::Namespace(PrivateFsError::LockUnavailable) => {
            CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryLocked)
        }
        ServiceRepositoryOpenError::Namespace(PrivateFsError::Io) => {
            CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryIo)
        }
        ServiceRepositoryOpenError::Namespace(
            PrivateFsError::IdentityAmbiguous
            | PrivateFsError::SettlementUnknown
            | PrivateFsError::Quarantined,
        ) => CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryRecoveryPending),
        ServiceRepositoryOpenError::Namespace(PrivateFsError::PrimitiveUnavailable) => {
            CleanupStartupOutcome::Failed(CleanupFailure::UnsupportedPlatform)
        }
        ServiceRepositoryOpenError::Namespace(_) => {
            CleanupStartupOutcome::Failed(CleanupFailure::UnsafeRepository)
        }
        ServiceRepositoryOpenError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::LockUnavailable,
        )) => CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryLocked),
        ServiceRepositoryOpenError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::Io,
        )) => CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryIo),
        ServiceRepositoryOpenError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::InUse,
        )) => CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryInUse),
        ServiceRepositoryOpenError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::IdentityAmbiguous
            | PrivateFsError::SettlementUnknown
            | PrivateFsError::Quarantined,
        )) => CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryRecoveryPending),
        ServiceRepositoryOpenError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::PrimitiveUnavailable,
        )) => CleanupStartupOutcome::Failed(CleanupFailure::UnsupportedPlatform),
        ServiceRepositoryOpenError::Repository(
            ExtensionRepositoryError::RecoveryAmbiguous
            | ExtensionRepositoryError::SettlementAmbiguous
            | ExtensionRepositoryError::Sealed,
        ) => CleanupStartupOutcome::Failed(CleanupFailure::RepositoryRecoveryAmbiguous),
        ServiceRepositoryOpenError::Repository(ExtensionRepositoryError::StateCorrupt) => {
            CleanupStartupOutcome::Failed(CleanupFailure::RepositoryCorrupt)
        }
        ServiceRepositoryOpenError::Repository(_) => {
            CleanupStartupOutcome::Failed(CleanupFailure::RepositoryCorrupt)
        }
        ServiceRepositoryOpenError::BuildSettlement(
            BundledPackageBuildSettlementError::Repository(
                ExtensionRepositoryError::SettlementAmbiguous
                | ExtensionRepositoryError::RecoveryAmbiguous
                | ExtensionRepositoryError::Sealed
                | ExtensionRepositoryError::GarbageCollectionInProgress
                | ExtensionRepositoryError::CatalogAdvanceBlockedByBuild
                | ExtensionRepositoryError::FileSystem(
                    PrivateFsError::IdentityAmbiguous
                    | PrivateFsError::SettlementUnknown
                    | PrivateFsError::Quarantined,
                ),
            ),
        ) => CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryRecoveryPending),
        ServiceRepositoryOpenError::BuildSettlement(
            BundledPackageBuildSettlementError::Repository(ExtensionRepositoryError::FileSystem(
                PrivateFsError::LockUnavailable,
            )),
        ) => CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryLocked),
        ServiceRepositoryOpenError::BuildSettlement(
            BundledPackageBuildSettlementError::Repository(ExtensionRepositoryError::FileSystem(
                PrivateFsError::Io,
            )),
        ) => CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryIo),
        ServiceRepositoryOpenError::BuildSettlement(
            BundledPackageBuildSettlementError::Repository(ExtensionRepositoryError::FileSystem(
                PrivateFsError::PrimitiveUnavailable,
            )),
        ) => CleanupStartupOutcome::Failed(CleanupFailure::UnsupportedPlatform),
        ServiceRepositoryOpenError::BuildSettlement(_)
        | ServiceRepositoryOpenError::UnexpectedBuildSettlementOutcome => {
            CleanupStartupOutcome::Failed(CleanupFailure::RepositoryCorrupt)
        }
    }
}

fn classify_release_error(error: BundledPackageLeaseReleaseError) -> SettleEntryOutcome {
    match error {
        BundledPackageLeaseReleaseError::BuildInProgress => SettleEntryOutcome::Reopen,
        BundledPackageLeaseReleaseError::ConcurrentLease => {
            SettleEntryOutcome::Failed(CleanupFailure::ConcurrentPackageLease)
        }
        BundledPackageLeaseReleaseError::JournalPinMismatch
        | BundledPackageLeaseReleaseError::WrongRepository => {
            SettleEntryOutcome::Failed(CleanupFailure::PackagePinMismatch)
        }
        BundledPackageLeaseReleaseError::Repository(
            ExtensionRepositoryError::SettlementAmbiguous
            | ExtensionRepositoryError::RecoveryAmbiguous
            | ExtensionRepositoryError::Sealed
            | ExtensionRepositoryError::GarbageCollectionInProgress
            | ExtensionRepositoryError::CatalogAdvanceBlockedByBuild
            | ExtensionRepositoryError::FileSystem(
                PrivateFsError::IdentityAmbiguous
                | PrivateFsError::SettlementUnknown
                | PrivateFsError::Quarantined,
            ),
        ) => SettleEntryOutcome::Reopen,
        BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::LockUnavailable,
        )) => SettleEntryOutcome::Unavailable(CleanupUnavailable::RepositoryLocked),
        BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::Io,
        )) => SettleEntryOutcome::Unavailable(CleanupUnavailable::RepositoryIo),
        BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::InUse,
        )) => SettleEntryOutcome::Unavailable(CleanupUnavailable::RepositoryInUse),
        BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::FileSystem(
            PrivateFsError::PrimitiveUnavailable,
        )) => SettleEntryOutcome::Failed(CleanupFailure::UnsupportedPlatform),
        BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::StateCorrupt) => {
            SettleEntryOutcome::Failed(CleanupFailure::RepositoryCorrupt)
        }
        BundledPackageLeaseReleaseError::Repository(_) => {
            SettleEntryOutcome::Failed(CleanupFailure::RepositoryCorrupt)
        }
        _ => SettleEntryOutcome::Failed(CleanupFailure::RepositoryCorrupt),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::time::Duration;

    use zephium_core::extensions::{
        ExtensionAuthorityId, ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
        ExtensionExpectedNativeOwnershipIdentity, ExtensionGrantBrowsingContext,
        ExtensionGrantDigest, ExtensionGrantRevision, ExtensionInstallCatalogRevision,
        ExtensionInstallRevision, ExtensionManifestDigest, ExtensionNativeOwnershipJournal,
        ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPreparation, ExtensionPackageIdentity,
        ExtensionPackageKey, ExtensionPackagePayloadIdentity, ExtensionPackageRevision,
        ExtensionRuntimeBackendTarget, ExtensionTreeDigest,
    };
    use zephium_core::ids::{ExtensionInstallId, ProfileId};
    use zephium_core::ports::store::{
        ExtensionNativeOwnershipJournalLoadOutcome, ExtensionNativeOwnershipJournalMutationApplied,
        ExtensionNativeOwnershipJournalMutationOutcome,
    };
    use zephium_store::ExtensionServiceStoreCallOutcome;

    use super::*;

    const TEST_DEADLINE: Duration = Duration::from_secs(30);

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum MutationTrace {
        Begin,
        Transition {
            operation: u64,
            intent: ExtensionNativeOwnershipIntent,
            phase: ExtensionNativeOwnershipPhase,
        },
        Clear {
            operation: u64,
        },
    }

    enum LoadAction {
        Current,
        Replace(ExtensionNativeOwnershipJournal),
        Failed,
    }

    struct FakeJournalBackend {
        durable: RefCell<ExtensionNativeOwnershipJournal>,
        loads: RefCell<VecDeque<LoadAction>>,
        load_count: Cell<usize>,
        mutations: RefCell<Vec<MutationTrace>>,
    }

    impl FakeJournalBackend {
        fn new(journal: ExtensionNativeOwnershipJournal) -> Self {
            Self {
                durable: RefCell::new(journal),
                loads: RefCell::new(VecDeque::new()),
                load_count: Cell::new(0),
                mutations: RefCell::new(Vec::new()),
            }
        }

        fn with_loads(self, loads: impl IntoIterator<Item = LoadAction>) -> Self {
            self.loads.borrow_mut().extend(loads);
            self
        }

        fn durable(&self) -> ExtensionNativeOwnershipJournal {
            self.durable.borrow().clone()
        }

        fn load_count(&self) -> usize {
            self.load_count.get()
        }

        fn mutation_traces(&self) -> Vec<MutationTrace> {
            self.mutations.borrow().clone()
        }
    }

    impl JournalBackend for FakeJournalBackend {
        fn load_until(
            &self,
            _deadline: Instant,
        ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipJournalLoadOutcome> {
            self.load_count.set(self.load_count.get() + 1);
            let journal = match self.loads.borrow_mut().pop_front() {
                None | Some(LoadAction::Current) => self.durable.borrow().clone(),
                Some(LoadAction::Replace(journal)) => {
                    *self.durable.borrow_mut() = journal.clone();
                    journal
                }
                Some(LoadAction::Failed) => {
                    return ExtensionServiceStoreCallOutcome::Completed(
                        ExtensionNativeOwnershipJournalLoadOutcome::Failed,
                    );
                }
            };
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal),
            )
        }

        fn mutate_until(
            &self,
            journal: &ExtensionNativeOwnershipJournal,
            mutation: ExtensionNativeOwnershipJournalMutation,
            _deadline: Instant,
        ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome>
        {
            let trace = match &mutation {
                ExtensionNativeOwnershipJournalMutation::Begin(_) => MutationTrace::Begin,
                ExtensionNativeOwnershipJournalMutation::Transition {
                    expected,
                    intent,
                    phase,
                    ..
                } => MutationTrace::Transition {
                    operation: expected.operation().get(),
                    intent: *intent,
                    phase: *phase,
                },
                ExtensionNativeOwnershipJournalMutation::Clear { expected } => {
                    MutationTrace::Clear {
                        operation: expected.operation().get(),
                    }
                }
            };
            self.mutations.borrow_mut().push(trace);

            let durable = self.durable.borrow().clone();
            if &durable != journal {
                return ExtensionServiceStoreCallOutcome::Completed(
                    ExtensionNativeOwnershipJournalMutationOutcome::Conflict {
                        current: durable.revision(),
                    },
                );
            }
            let application = durable
                .apply(journal.revision(), mutation)
                .expect("scripted cleanup mutation must be locally valid");
            let applied = ExtensionNativeOwnershipJournalMutationApplied {
                journal_revision: application.journal().revision(),
                operation_high_water: application.journal().operation_high_water(),
                native_incarnation_high_water: application
                    .journal()
                    .native_incarnation_high_water(),
                entry: application.entry().cloned().map(Box::new),
            };
            *self.durable.borrow_mut() = application.into_journal();
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalMutationOutcome::Applied(applied),
            )
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct ReleaseTrace {
        install_id: ExtensionInstallId,
        native_incarnation: u64,
    }

    struct FakeRepository {
        open: Cell<bool>,
        open_calls: Cell<usize>,
        reopen_calls: Cell<usize>,
        reopen_results: RefCell<VecDeque<Result<(), ServiceRepositoryOpenError>>>,
        release_results: RefCell<
            VecDeque<Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError>>,
        >,
        releases: RefCell<Vec<ReleaseTrace>>,
    }

    impl FakeRepository {
        fn new() -> Self {
            Self {
                open: Cell::new(false),
                open_calls: Cell::new(0),
                reopen_calls: Cell::new(0),
                reopen_results: RefCell::new(VecDeque::new()),
                release_results: RefCell::new(VecDeque::new()),
                releases: RefCell::new(Vec::new()),
            }
        }

        fn with_release_results(
            self,
            results: impl IntoIterator<
                Item = Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError>,
            >,
        ) -> Self {
            self.release_results.borrow_mut().extend(results);
            self
        }

        fn with_reopen_results(
            self,
            results: impl IntoIterator<Item = Result<(), ServiceRepositoryOpenError>>,
        ) -> Self {
            self.reopen_results.borrow_mut().extend(results);
            self
        }

        fn releases(&self) -> Vec<ReleaseTrace> {
            self.releases.borrow().clone()
        }
    }

    impl CleanupRepositoryBackend for FakeRepository {
        fn is_open(&self) -> bool {
            self.open.get()
        }

        fn open(&mut self) -> Result<(), ServiceRepositoryOpenError> {
            self.open_calls.set(self.open_calls.get() + 1);
            self.open.set(true);
            Ok(())
        }

        fn reopen(&mut self) -> Result<(), ServiceRepositoryOpenError> {
            self.reopen_calls.set(self.reopen_calls.get() + 1);
            let result = self
                .reopen_results
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(()));
            self.open.set(result.is_ok());
            result
        }

        fn reconcile_release(
            &mut self,
            binding: &ExtensionPackagePinReleaseBinding,
        ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
            self.releases.borrow_mut().push(ReleaseTrace {
                install_id: binding.install_id(),
                native_incarnation: binding.native_incarnation().get(),
            });
            self.release_results
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(BundledPackageLeaseReleaseOutcome::Released))
        }
    }

    #[derive(Clone, Copy)]
    enum RefusalScript {
        Never,
        CancelAt(usize),
        DeadlineAt(usize),
    }

    struct ScriptedCancellation {
        checks: Cell<usize>,
        script: RefusalScript,
    }

    impl ScriptedCancellation {
        const fn new(script: RefusalScript) -> Self {
            Self {
                checks: Cell::new(0),
                script,
            }
        }

        const fn never() -> Self {
            Self::new(RefusalScript::Never)
        }
    }

    impl CancellationCheck for ScriptedCancellation {
        fn is_cancelled(&self) -> bool {
            let check = self.checks.get() + 1;
            self.checks.set(check);
            matches!(self.script, RefusalScript::CancelAt(target) if target == check)
        }

        fn deadline_reached(&self, deadline: Instant) -> bool {
            matches!(
                self.script,
                RefusalScript::DeadlineAt(target) if target == self.checks.get()
            ) || Instant::now() >= deadline
        }
    }

    fn preparation_for(profile: ProfileId, install: u128) -> ExtensionNativeOwnershipPreparation {
        let byte = u8::try_from(install).unwrap();
        ExtensionNativeOwnershipPreparation::new(
            ExtensionNativeOwnershipKey::new(
                profile,
                ExtensionInstallId::from(install),
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionPackageIdentity::new(
                ExtensionAuthorityId::from_bytes([1; 32]),
                ExtensionPackageKey::from_bytes([byte; 32]),
                ExtensionPackageRevision::INITIAL,
                ExtensionPackagePayloadIdentity::BundledTree,
                ExtensionManifestDigest::from_bytes([3; 32]),
                ExtensionTreeDigest::from_bytes([4; 32]),
            ),
            ExtensionCatalogSetDigest::from_bytes([5; 32]),
            ExtensionCatalogGenerationRole::Active,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
            ExtensionGrantRevision::INITIAL,
            ExtensionGrantDigest::from_bytes([6; 32]),
            ExtensionRuntimeBackendTarget::MacosNative,
        )
    }

    fn preparation(install: u128) -> ExtensionNativeOwnershipPreparation {
        preparation_for(ProfileId::from(1), install)
    }

    fn expected_native_identity(
        install_id: ExtensionInstallId,
    ) -> ExtensionExpectedNativeOwnershipIdentity {
        let identifier_byte = b'a' + (install_id.bytes()[15] & 0x0f);
        ExtensionExpectedNativeOwnershipIdentity::from_encoded_bytes(
            ExtensionRuntimeBackendTarget::MacosNative,
            [identifier_byte; 32],
        )
        .unwrap()
    }

    fn begin_for(
        journal: ExtensionNativeOwnershipJournal,
        profile: ProfileId,
        install: u128,
    ) -> ExtensionNativeOwnershipJournal {
        let expected = journal.revision();
        journal
            .apply(
                expected,
                ExtensionNativeOwnershipJournalMutation::begin(preparation_for(profile, install)),
            )
            .unwrap()
            .into_journal()
    }

    fn begin(
        journal: ExtensionNativeOwnershipJournal,
        install: u128,
    ) -> ExtensionNativeOwnershipJournal {
        begin_for(journal, ProfileId::from(1), install)
    }

    fn transition(
        journal: ExtensionNativeOwnershipJournal,
        key: ExtensionNativeOwnershipKey,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
    ) -> ExtensionNativeOwnershipJournal {
        let expected = journal.revision();
        let row = journal.get(key).unwrap();
        let mutation = if row.runtime_backend() == ExtensionRuntimeBackendTarget::MacosNative
            && row.intent() == ExtensionNativeOwnershipIntent::Acquire
            && row.phase() == ExtensionNativeOwnershipPhase::NativeAbsentPreparing
            && intent == ExtensionNativeOwnershipIntent::Acquire
            && phase == ExtensionNativeOwnershipPhase::NativeMayOwn
        {
            ExtensionNativeOwnershipJournalMutation::transition_with_expected_native_identity(
                row.cas(),
                expected_native_identity(key.install_id()),
            )
        } else {
            ExtensionNativeOwnershipJournalMutation::transition(row.cas(), intent, phase)
        };
        journal.apply(expected, mutation).unwrap().into_journal()
    }

    fn clear(
        journal: ExtensionNativeOwnershipJournal,
        key: ExtensionNativeOwnershipKey,
    ) -> ExtensionNativeOwnershipJournal {
        let expected = journal.revision();
        let mutation =
            ExtensionNativeOwnershipJournalMutation::clear(journal.get(key).unwrap().cas());
        journal.apply(expected, mutation).unwrap().into_journal()
    }

    fn acquire_absent(install: u128) -> ExtensionNativeOwnershipJournal {
        begin(ExtensionNativeOwnershipJournal::empty(), install)
    }

    fn release_pending(install: u128) -> ExtensionNativeOwnershipJournal {
        let journal = acquire_absent(install);
        transition(
            journal,
            preparation(install).key(),
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
        )
    }

    fn possible_owner(install: u128) -> ExtensionNativeOwnershipJournal {
        let journal = acquire_absent(install);
        transition(
            journal,
            preparation(install).key(),
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        )
    }

    fn replace_with_possible_owner(
        journal: ExtensionNativeOwnershipJournal,
        install: u128,
    ) -> ExtensionNativeOwnershipJournal {
        let key = preparation(install).key();
        let journal = clear(journal, key);
        let journal = begin(journal, install);
        transition(
            journal,
            key,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        )
    }

    fn run(
        backend: &FakeJournalBackend,
        repository: &mut FakeRepository,
        cancellation: &ScriptedCancellation,
    ) -> CleanupStartupOutcome {
        run_until(
            backend,
            repository,
            cancellation,
            Instant::now() + TEST_DEADLINE,
            |_| {},
        )
    }

    fn run_until(
        backend: &FakeJournalBackend,
        repository: &mut FakeRepository,
        cancellation: &ScriptedCancellation,
        deadline: Instant,
        publish_progress: impl FnMut(CleanupProgress),
    ) -> CleanupStartupOutcome {
        let mut projection = JournalProjection::unknown();
        reconcile_startup(
            backend,
            &mut projection,
            repository,
            None,
            CleanupAttempt::new(CleanupScope::All, cancellation, deadline),
            publish_progress,
        )
    }

    fn assert_no_clear(backend: &FakeJournalBackend) {
        assert!(
            backend
                .mutation_traces()
                .iter()
                .all(|trace| !matches!(trace, MutationTrace::Clear { .. })),
            "clear must not be admitted: {:?}",
            backend.mutation_traces()
        );
    }

    #[test]
    fn acquire_absent_transitions_releases_and_clears_exactly_once() {
        let initial = acquire_absent(1);
        let operation = initial.entries()[0].operation().get();
        let backend = FakeJournalBackend::new(initial);
        let mut repository = FakeRepository::new();
        let cancellation = ScriptedCancellation::never();
        let progress = RefCell::new(Vec::new());

        let outcome = run_until(
            &backend,
            &mut repository,
            &cancellation,
            Instant::now() + TEST_DEADLINE,
            |phase| progress.borrow_mut().push(phase),
        );

        let durable = backend.durable();
        assert_eq!(
            outcome,
            CleanupStartupOutcome::Ready {
                journal_revision: durable.revision(),
            }
        );
        assert!(durable.entries().is_empty());
        assert_eq!(
            backend.mutation_traces(),
            vec![
                MutationTrace::Transition {
                    operation,
                    intent: ExtensionNativeOwnershipIntent::Release,
                    phase: ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                },
                MutationTrace::Clear { operation },
            ]
        );
        assert_eq!(repository.releases().len(), 1);
        assert_eq!(repository.releases()[0].native_incarnation, operation);
        assert_eq!(repository.open_calls.get(), 1);
        assert_eq!(repository.reopen_calls.get(), 0);
        assert_eq!(
            *progress.borrow(),
            vec![
                CleanupProgress::LoadingOwnershipJournal,
                CleanupProgress::ReconcilingCleanup,
            ]
        );
    }

    #[test]
    fn release_pending_releases_then_clears_without_an_acquire_transition() {
        let initial = release_pending(1);
        let operation = initial.entries()[0].operation().get();
        let backend = FakeJournalBackend::new(initial);
        let mut repository = FakeRepository::new();

        let outcome = run(&backend, &mut repository, &ScriptedCancellation::never());

        assert!(matches!(outcome, CleanupStartupOutcome::Ready { .. }));
        assert_eq!(
            backend.mutation_traces(),
            vec![MutationTrace::Clear { operation }]
        );
        assert_eq!(repository.releases().len(), 1);
    }

    #[test]
    fn already_released_is_a_definite_release_fence_and_allows_clear() {
        let initial = release_pending(1);
        let operation = initial.entries()[0].operation().get();
        let backend = FakeJournalBackend::new(initial);
        let mut repository = FakeRepository::new()
            .with_release_results([Ok(BundledPackageLeaseReleaseOutcome::AlreadyReleased)]);

        let outcome = run(&backend, &mut repository, &ScriptedCancellation::never());

        assert!(matches!(outcome, CleanupStartupOutcome::Ready { .. }));
        assert_eq!(
            backend.mutation_traces(),
            vec![MutationTrace::Clear { operation }]
        );
        assert_eq!(repository.releases().len(), 1);
    }

    #[test]
    fn possible_owner_is_untouched_and_cannot_report_ready() {
        let initial = possible_owner(1);
        let expected_revision = initial.revision();
        let backend = FakeJournalBackend::new(initial.clone());
        let mut repository = FakeRepository::new();

        let outcome = run(&backend, &mut repository, &ScriptedCancellation::never());

        assert_eq!(
            outcome,
            CleanupStartupOutcome::CleanupRequired {
                journal_revision: expected_revision,
                possible_owner_count: 1,
            }
        );
        assert_eq!(backend.durable(), initial);
        assert!(backend.mutation_traces().is_empty());
        assert!(repository.releases().is_empty());
    }

    #[test]
    fn mixed_journal_cleans_definite_absence_and_retains_possible_owner() {
        let journal = begin(begin(ExtensionNativeOwnershipJournal::empty(), 1), 2);
        let definite_operation = journal.get(preparation(1).key()).unwrap().operation().get();
        let journal = transition(
            journal,
            preparation(2).key(),
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        );
        let backend = FakeJournalBackend::new(journal);
        let mut repository = FakeRepository::new();

        let outcome = run(&backend, &mut repository, &ScriptedCancellation::never());

        let durable = backend.durable();
        assert_eq!(
            outcome,
            CleanupStartupOutcome::CleanupRequired {
                journal_revision: durable.revision(),
                possible_owner_count: 1,
            }
        );
        assert_eq!(durable.entries().len(), 1);
        assert_eq!(durable.entries()[0].key(), preparation(2).key());
        assert_eq!(
            backend.mutation_traces(),
            vec![
                MutationTrace::Transition {
                    operation: definite_operation,
                    intent: ExtensionNativeOwnershipIntent::Release,
                    phase: ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                },
                MutationTrace::Clear {
                    operation: definite_operation,
                },
            ]
        );
        assert_eq!(repository.releases().len(), 1);
        assert_eq!(
            repository.releases()[0].native_incarnation,
            definite_operation
        );
    }

    #[test]
    fn profile_scope_drains_only_target_rows_and_preserves_unrelated_rows_exactly() {
        let target = ProfileId::from(1);
        let unrelated = ProfileId::from(2);
        let journal = begin_for(ExtensionNativeOwnershipJournal::empty(), target, 1);
        let journal = begin_for(journal, unrelated, 2);
        let target_entry = journal
            .get(preparation_for(target, 1).key())
            .unwrap()
            .clone();
        let unrelated_entry = journal
            .get(preparation_for(unrelated, 2).key())
            .unwrap()
            .clone();
        let backend = FakeJournalBackend::new(journal);
        let mut repository = FakeRepository::new();
        let mut projection = JournalProjection::unknown();

        let outcome = reconcile_startup(
            &backend,
            &mut projection,
            &mut repository,
            None,
            CleanupAttempt::new(
                CleanupScope::Profile(target),
                &ScriptedCancellation::never(),
                Instant::now() + TEST_DEADLINE,
            ),
            |_| {},
        );

        let durable = backend.durable();
        assert_eq!(
            outcome,
            CleanupStartupOutcome::Ready {
                journal_revision: durable.revision(),
            }
        );
        assert!(durable.get(target_entry.key()).is_none());
        assert_eq!(durable.get(unrelated_entry.key()), Some(&unrelated_entry));
        assert_eq!(repository.releases().len(), 1);
        assert_eq!(
            repository.releases()[0].install_id,
            ExtensionInstallId::from(1)
        );
        assert_eq!(
            backend.mutation_traces(),
            vec![
                MutationTrace::Transition {
                    operation: target_entry.operation().get(),
                    intent: ExtensionNativeOwnershipIntent::Release,
                    phase: ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                },
                MutationTrace::Clear {
                    operation: target_entry.operation().get(),
                },
            ]
        );
    }

    #[test]
    fn exact_row_change_at_pre_release_revalidation_never_releases_stale_owner() {
        let initial = acquire_absent(1);
        let operation = initial.entries()[0].operation().get();
        let transitioned = transition(
            initial.clone(),
            preparation(1).key(),
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
        );
        let replacement = replace_with_possible_owner(transitioned, 1);
        let replacement_revision = replacement.revision();
        let backend = FakeJournalBackend::new(initial)
            .with_loads([LoadAction::Current, LoadAction::Replace(replacement)]);
        let mut repository = FakeRepository::new();

        let outcome = run(&backend, &mut repository, &ScriptedCancellation::never());

        assert_eq!(
            outcome,
            CleanupStartupOutcome::CleanupRequired {
                journal_revision: replacement_revision,
                possible_owner_count: 1,
            }
        );
        assert_eq!(
            backend.mutation_traces(),
            vec![MutationTrace::Transition {
                operation,
                intent: ExtensionNativeOwnershipIntent::Release,
                phase: ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            }]
        );
        assert!(repository.releases().is_empty());
    }

    #[test]
    fn exact_row_change_at_post_release_revalidation_never_clears_new_owner() {
        let initial = release_pending(1);
        let operation = initial.entries()[0].operation().get();
        let replacement = replace_with_possible_owner(initial.clone(), 1);
        let replacement_revision = replacement.revision();
        let backend = FakeJournalBackend::new(initial).with_loads([
            LoadAction::Current,
            LoadAction::Current,
            LoadAction::Replace(replacement),
        ]);
        let mut repository = FakeRepository::new();

        let outcome = run(&backend, &mut repository, &ScriptedCancellation::never());

        assert_eq!(
            outcome,
            CleanupStartupOutcome::CleanupRequired {
                journal_revision: replacement_revision,
                possible_owner_count: 1,
            }
        );
        assert!(backend.mutation_traces().is_empty());
        assert_eq!(repository.releases().len(), 1);
        assert_eq!(repository.releases()[0].native_incarnation, operation);
    }

    fn ambiguous_release() -> BundledPackageLeaseReleaseError {
        BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::SettlementAmbiguous)
    }

    #[test]
    fn ambiguous_release_reopens_reloads_and_reconciles_before_clear() {
        let initial = release_pending(1);
        let operation = initial.entries()[0].operation().get();
        let backend = FakeJournalBackend::new(initial);
        let mut repository = FakeRepository::new().with_release_results([
            Err(ambiguous_release()),
            Ok(BundledPackageLeaseReleaseOutcome::Released),
        ]);

        let outcome = run(&backend, &mut repository, &ScriptedCancellation::never());

        assert!(matches!(outcome, CleanupStartupOutcome::Ready { .. }));
        assert_eq!(repository.reopen_calls.get(), 1);
        assert_eq!(repository.releases().len(), 2);
        assert_eq!(
            backend.mutation_traces(),
            vec![MutationTrace::Clear { operation }]
        );
    }

    #[test]
    fn repeated_ambiguous_release_is_terminal_and_never_clears() {
        let backend = FakeJournalBackend::new(release_pending(1));
        let mut repository = FakeRepository::new().with_release_results([
            Err(ambiguous_release()),
            Err(ambiguous_release()),
            Err(ambiguous_release()),
        ]);

        let outcome = run(&backend, &mut repository, &ScriptedCancellation::never());

        assert_eq!(
            outcome,
            CleanupStartupOutcome::Failed(CleanupFailure::RepositoryRecoveryAmbiguous)
        );
        assert_eq!(repository.reopen_calls.get(), 2);
        assert_eq!(repository.releases().len(), 3);
        assert_no_clear(&backend);
    }

    #[test]
    fn ambiguous_release_with_terminal_reopen_failure_never_clears() {
        let backend = FakeJournalBackend::new(release_pending(1));
        let mut repository = FakeRepository::new()
            .with_release_results([Err(ambiguous_release())])
            .with_reopen_results([Err(ServiceRepositoryOpenError::Repository(
                ExtensionRepositoryError::RecoveryAmbiguous,
            ))]);

        let outcome = run(&backend, &mut repository, &ScriptedCancellation::never());

        assert_eq!(
            outcome,
            CleanupStartupOutcome::Failed(CleanupFailure::RepositoryRecoveryAmbiguous)
        );
        assert_eq!(repository.reopen_calls.get(), 1);
        assert_eq!(repository.releases().len(), 1);
        assert_no_clear(&backend);
    }

    #[test]
    fn cancellation_at_every_release_cleanup_frontier_never_admits_clear() {
        for target in 1..=7 {
            let backend = FakeJournalBackend::new(release_pending(1));
            let mut repository = FakeRepository::new();
            let cancellation = ScriptedCancellation::new(RefusalScript::CancelAt(target));

            let outcome = run(&backend, &mut repository, &cancellation);

            assert_eq!(
                outcome,
                CleanupStartupOutcome::Unavailable(CleanupUnavailable::Cancelled),
                "frontier check {target}"
            );
            assert_eq!(cancellation.checks.get(), target);
            assert_no_clear(&backend);
        }
    }

    #[test]
    fn deadline_at_every_release_cleanup_frontier_never_admits_clear() {
        for target in 1..=7 {
            let backend = FakeJournalBackend::new(release_pending(1));
            let mut repository = FakeRepository::new();
            let cancellation = ScriptedCancellation::new(RefusalScript::DeadlineAt(target));

            let outcome = run(&backend, &mut repository, &cancellation);

            assert_eq!(
                outcome,
                CleanupStartupOutcome::Unavailable(CleanupUnavailable::DeadlineExpired),
                "frontier check {target}"
            );
            assert_eq!(cancellation.checks.get(), target);
            assert_no_clear(&backend);
        }
    }

    #[test]
    fn refusal_after_acquire_transition_never_releases_or_clears() {
        for script in [RefusalScript::CancelAt(4), RefusalScript::DeadlineAt(4)] {
            let initial = acquire_absent(1);
            let operation = initial.entries()[0].operation().get();
            let backend = FakeJournalBackend::new(initial);
            let mut repository = FakeRepository::new();
            let cancellation = ScriptedCancellation::new(script);

            let outcome = run(&backend, &mut repository, &cancellation);

            assert!(matches!(outcome, CleanupStartupOutcome::Unavailable(_)));
            assert_eq!(
                backend.mutation_traces(),
                vec![MutationTrace::Transition {
                    operation,
                    intent: ExtensionNativeOwnershipIntent::Release,
                    phase: ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                }]
            );
            assert!(repository.releases().is_empty());
            assert_no_clear(&backend);
        }
    }

    #[test]
    fn already_expired_real_deadline_refuses_before_repository_admission() {
        let backend = FakeJournalBackend::new(release_pending(1));
        let mut repository = FakeRepository::new();
        let cancellation = ScriptedCancellation::never();

        let outcome = run_until(
            &backend,
            &mut repository,
            &cancellation,
            Instant::now(),
            |_| {},
        );

        assert_eq!(
            outcome,
            CleanupStartupOutcome::Unavailable(CleanupUnavailable::DeadlineExpired)
        );
        assert_eq!(repository.open_calls.get(), 0);
        assert_eq!(backend.load_count(), 0);
        assert!(backend.mutation_traces().is_empty());
    }

    #[test]
    fn store_load_failure_has_its_own_failed_closed_classification() {
        let backend = FakeJournalBackend::new(release_pending(1)).with_loads([LoadAction::Failed]);
        let mut repository = FakeRepository::new();

        let outcome = run(&backend, &mut repository, &ScriptedCancellation::never());

        assert_eq!(
            outcome,
            CleanupStartupOutcome::Failed(CleanupFailure::StoreJournalLoadFailed)
        );
        assert!(repository.releases().is_empty());
        assert!(backend.mutation_traces().is_empty());
    }

    #[test]
    fn fresh_open_retries_recoverable_private_filesystem_ambiguity() {
        for error in [
            PrivateFsError::IdentityAmbiguous,
            PrivateFsError::SettlementUnknown,
            PrivateFsError::Quarantined,
        ] {
            assert_eq!(
                classify_open_error(ServiceRepositoryOpenError::Namespace(error)),
                CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryRecoveryPending)
            );
            assert_eq!(
                classify_open_error(ServiceRepositoryOpenError::Repository(
                    ExtensionRepositoryError::FileSystem(error),
                )),
                CleanupStartupOutcome::Unavailable(CleanupUnavailable::RepositoryRecoveryPending)
            );
        }
    }

    #[test]
    fn same_open_authority_at_a_cleanup_reopen_is_an_internal_ordering_failure() {
        assert_eq!(
            classify_open_error(ServiceRepositoryOpenError::OutstandingAuthority),
            CleanupStartupOutcome::Failed(CleanupFailure::ConcurrentPackageLease)
        );
    }

    #[test]
    fn definite_repository_corruption_is_not_reported_as_unknown_settlement() {
        assert_eq!(
            classify_open_error(ServiceRepositoryOpenError::Repository(
                ExtensionRepositoryError::StateCorrupt,
            )),
            CleanupStartupOutcome::Failed(CleanupFailure::RepositoryCorrupt)
        );
        assert!(matches!(
            classify_release_error(BundledPackageLeaseReleaseError::Repository(
                ExtensionRepositoryError::StateCorrupt,
            )),
            SettleEntryOutcome::Failed(CleanupFailure::RepositoryCorrupt)
        ));
    }
}
