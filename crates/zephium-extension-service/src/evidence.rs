use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

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
