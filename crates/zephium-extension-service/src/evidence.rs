use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

use zephium_core::extensions::{
    ExtensionNativeOwnershipJournalRevision, MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES,
};
use zephium_core::ports::extensions::ExtensionActiveProfiles;

const _: () = assert!(MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES <= u16::MAX as usize);

static NEXT_WORKER_IDENTITY: AtomicU64 = AtomicU64::new(1);

/// Opaque process-local identity of one extension-service worker.
///
/// This identity prevents lifecycle observations from being confused across
/// worker replacement. It is structural evidence only and grants no package,
/// profile, repository, or native-runtime authority.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExtensionServiceWorkerIdentity(NonZeroU64);

impl ExtensionServiceWorkerIdentity {
    pub(crate) fn mint() -> Option<Self> {
        NEXT_WORKER_IDENTITY
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .ok()
            .and_then(NonZeroU64::new)
            .map(Self)
    }

    /// Returns the opaque process-local numeric identity.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Proof that one exact extension-service worker drained and released resources.
///
/// The proof is minted only after the worker consumes the reserved shutdown
/// barrier and drops its owned state. It is returned publicly only after the
/// worker thread is joined. It is not durable and is not native ownership
/// evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionServiceShutdownEvidence {
    worker: ExtensionServiceWorkerIdentity,
    accepted_commands: u64,
    completed_commands: u64,
}

impl ExtensionServiceShutdownEvidence {
    pub(crate) const fn new(
        worker: ExtensionServiceWorkerIdentity,
        accepted_commands: u64,
        completed_commands: u64,
    ) -> Self {
        Self {
            worker,
            accepted_commands,
            completed_commands,
        }
    }

    /// Returns the exact worker that exited.
    pub const fn worker(self) -> ExtensionServiceWorkerIdentity {
        self.worker
    }

    /// Returns the number of ordinary and retirement commands admitted before shutdown.
    pub const fn accepted_commands(self) -> u64 {
        self.accepted_commands
    }

    /// Returns the number of ordinary and retirement commands completed before shutdown.
    pub const fn completed_commands(self) -> u64 {
        self.completed_commands
    }
}

/// Proof that one worker completed repository recovery and observed no
/// unresolved native-ownership rows at one exact journal revision.
///
/// This process-local, path-free evidence grants no Store, repository, package,
/// profile, or native-runtime authority. Its fields are private so only the
/// serialized service can mint it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionServiceReadyEvidence {
    worker: ExtensionServiceWorkerIdentity,
    journal_revision: ExtensionNativeOwnershipJournalRevision,
    active_profiles: ExtensionActiveProfiles,
    active_runtime_count: u16,
    rejected_runtime_count: u16,
    capacity_deferred_runtime_count: u16,
    degraded_profile_count: u16,
}

impl ExtensionServiceReadyEvidence {
    pub(crate) const fn new(
        worker: ExtensionServiceWorkerIdentity,
        journal_revision: ExtensionNativeOwnershipJournalRevision,
    ) -> Self {
        Self {
            worker,
            journal_revision,
            active_profiles: ExtensionActiveProfiles::EMPTY,
            active_runtime_count: 0,
            rejected_runtime_count: 0,
            capacity_deferred_runtime_count: 0,
            degraded_profile_count: 0,
        }
    }

    pub(crate) fn after_hydration(
        worker: ExtensionServiceWorkerIdentity,
        journal_revision: ExtensionNativeOwnershipJournalRevision,
        active_profiles: ExtensionActiveProfiles,
        active_runtime_count: usize,
        rejected_runtime_count: usize,
        capacity_deferred_runtime_count: usize,
        degraded_profile_count: usize,
    ) -> Option<Self> {
        if active_runtime_count > zephium_core::ports::extensions::MAX_EXTENSION_ACTIVE_PROFILES
            || active_profiles.len() > active_runtime_count
            || (active_runtime_count == 0) != active_profiles.is_empty()
        {
            return None;
        }
        Some(Self {
            worker,
            journal_revision,
            active_profiles,
            active_runtime_count: u16::try_from(active_runtime_count).ok()?,
            rejected_runtime_count: u16::try_from(rejected_runtime_count).ok()?,
            capacity_deferred_runtime_count: u16::try_from(capacity_deferred_runtime_count).ok()?,
            degraded_profile_count: u16::try_from(degraded_profile_count).ok()?,
        })
    }

    /// Returns the exact process-local worker that completed startup.
    pub const fn worker(self) -> ExtensionServiceWorkerIdentity {
        self.worker
    }

    /// Returns the complete journal revision observed after startup hydration.
    pub const fn journal_revision(self) -> ExtensionNativeOwnershipJournalRevision {
        self.journal_revision
    }

    /// Exact profiles with at least one active startup runtime. This compact
    /// routing projection is not package, grant, Store, or native authority.
    pub const fn active_profiles(self) -> ExtensionActiveProfiles {
        self.active_profiles
    }

    /// Runtimes active when startup readiness was published.
    pub const fn active_runtime_count(self) -> u16 {
        self.active_runtime_count
    }

    /// Enabled runtimes rejected by exact package, grant, or native policy.
    pub const fn rejected_runtime_count(self) -> u16 {
        self.rejected_runtime_count
    }

    /// Enabled runtimes deferred by the strict background-runtime ceiling.
    pub const fn capacity_deferred_runtime_count(self) -> u16 {
        self.capacity_deferred_runtime_count
    }

    /// Profiles whose exact ancillary extension catalog was degraded.
    pub const fn degraded_profile_count(self) -> u16 {
        self.degraded_profile_count
    }
}

/// Proof that one worker completed its safe cleanup pass but retained bounded
/// durable possible-owner rows.
///
/// The evidence is deliberately redacted: it exposes only a count, never a
/// path, profile, install, package, native identity, or cleanup binding. It is
/// not evidence that any native owner is absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionServiceCleanupEvidence {
    worker: ExtensionServiceWorkerIdentity,
    journal_revision: ExtensionNativeOwnershipJournalRevision,
    possible_owner_count: u16,
}

impl ExtensionServiceCleanupEvidence {
    pub(crate) fn new(
        worker: ExtensionServiceWorkerIdentity,
        journal_revision: ExtensionNativeOwnershipJournalRevision,
        possible_owner_count: usize,
    ) -> Option<Self> {
        if possible_owner_count == 0
            || possible_owner_count > MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES
        {
            return None;
        }
        Some(Self {
            worker,
            journal_revision,
            possible_owner_count: u16::try_from(possible_owner_count).ok()?,
        })
    }

    /// Returns the exact process-local worker that performed the cleanup pass.
    pub const fn worker(self) -> ExtensionServiceWorkerIdentity {
        self.worker
    }

    /// Returns the complete journal revision observed after the cleanup pass.
    pub const fn journal_revision(self) -> ExtensionNativeOwnershipJournalRevision {
        self.journal_revision
    }

    /// Returns the bounded number of durable possible-owner rows retained.
    pub const fn possible_owner_count(self) -> u16 {
        self.possible_owner_count
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    #[test]
    fn cleanup_evidence_requires_a_bounded_nonzero_count() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let revision = ExtensionNativeOwnershipJournalRevision::INITIAL;

        assert!(ExtensionServiceCleanupEvidence::new(worker, revision, 0).is_none());
        assert!(ExtensionServiceCleanupEvidence::new(
            worker,
            revision,
            MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES + 1,
        )
        .is_none());

        let evidence = ExtensionServiceCleanupEvidence::new(
            worker,
            revision,
            MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES,
        )
        .unwrap();
        assert_eq!(
            usize::from(evidence.possible_owner_count()),
            MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES
        );
    }

    #[test]
    fn ready_evidence_reports_bounded_hydration_settlement() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let revision = ExtensionNativeOwnershipJournalRevision::INITIAL;
        let evidence = ExtensionServiceReadyEvidence::new(worker, revision);

        assert_eq!(evidence.worker(), worker);
        assert_eq!(evidence.journal_revision(), revision);
        assert!(evidence.active_profiles().is_empty());
        assert_eq!(evidence.active_runtime_count(), 0);
        assert_eq!(evidence.rejected_runtime_count(), 0);
        assert_eq!(evidence.capacity_deferred_runtime_count(), 0);
        assert_eq!(evidence.degraded_profile_count(), 0);

        let mut active_profiles = ExtensionActiveProfiles::EMPTY;
        assert!(active_profiles.try_insert(zephium_core::ids::ProfileId::from(7)));
        let hydrated = ExtensionServiceReadyEvidence::after_hydration(
            worker,
            revision,
            active_profiles,
            1,
            2,
            3,
            4,
        )
        .unwrap();
        assert_eq!(hydrated.active_profiles(), active_profiles);
        assert_eq!(hydrated.active_runtime_count(), 1);
        assert_eq!(hydrated.rejected_runtime_count(), 2);
        assert_eq!(hydrated.capacity_deferred_runtime_count(), 3);
        assert_eq!(hydrated.degraded_profile_count(), 4);
        assert!(ExtensionServiceReadyEvidence::after_hydration(
            worker,
            revision,
            ExtensionActiveProfiles::EMPTY,
            usize::from(u16::MAX) + 1,
            0,
            0,
            0,
        )
        .is_none());
        assert!(ExtensionServiceReadyEvidence::after_hydration(
            worker,
            revision,
            ExtensionActiveProfiles::EMPTY,
            1,
            0,
            0,
            0,
        )
        .is_none());
    }
}
