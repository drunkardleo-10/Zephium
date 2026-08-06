use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

use zephium_core::extensions::{
    ExtensionNativeOwnershipJournalRevision, MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES,
};

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
    accepted_normal_commands: u64,
    completed_normal_commands: u64,
}

impl ExtensionServiceShutdownEvidence {
    pub(crate) const fn new(
        worker: ExtensionServiceWorkerIdentity,
        accepted_normal_commands: u64,
        completed_normal_commands: u64,
    ) -> Self {
        Self {
            worker,
            accepted_normal_commands,
            completed_normal_commands,
        }
    }

    /// Returns the exact worker that exited.
    pub const fn worker(self) -> ExtensionServiceWorkerIdentity {
        self.worker
    }

    /// Returns the number of normal commands admitted before shutdown.
    pub const fn accepted_normal_commands(self) -> u64 {
        self.accepted_normal_commands
    }

    /// Returns the number of normal commands completed before shutdown.
    pub const fn completed_normal_commands(self) -> u64 {
        self.completed_normal_commands
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
}

impl ExtensionServiceReadyEvidence {
    pub(crate) const fn new(
        worker: ExtensionServiceWorkerIdentity,
        journal_revision: ExtensionNativeOwnershipJournalRevision,
    ) -> Self {
        Self {
            worker,
            journal_revision,
        }
    }

    /// Returns the exact process-local worker that completed startup.
    pub const fn worker(self) -> ExtensionServiceWorkerIdentity {
        self.worker
    }

    /// Returns the complete journal revision observed after cleanup.
    pub const fn journal_revision(self) -> ExtensionNativeOwnershipJournalRevision {
        self.journal_revision
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
    fn ready_evidence_retains_only_worker_and_revision() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let revision = ExtensionNativeOwnershipJournalRevision::INITIAL;
        let evidence = ExtensionServiceReadyEvidence::new(worker, revision);

        assert_eq!(evidence.worker(), worker);
        assert_eq!(evidence.journal_revision(), revision);
    }
}
