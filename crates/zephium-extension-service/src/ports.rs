use std::time::Duration;

use crate::{
    ExtensionServiceShutdownEvidence, ExtensionServiceStatusSnapshot, ExtensionServiceStatusWait,
};

/// Read-only observation port for the serialized extension service.
///
/// Mutation and shutdown authority deliberately do not cross this cloneable
/// boundary.
pub trait ExtensionServiceStatusPort: Send + Sync {
    /// Returns the latest process-local lifecycle snapshot.
    fn extension_service_status(&self) -> ExtensionServiceStatusSnapshot;

    /// Waits without polling for a snapshot newer than `after`.
    ///
    /// A cursor from another worker reports an immediate change even when its
    /// numeric revision is greater. Revisions are meaningful only inside one
    /// exact process-local worker lifetime.
    fn wait_for_extension_service_status(
        &self,
        after: ExtensionServiceStatusSnapshot,
        timeout: Duration,
    ) -> ExtensionServiceStatusWait;
}

/// Terminal result of consuming the unique extension-service owner.
#[must_use = "extension-service shutdown is not proven unless this outcome is checked"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceShutdownOutcome {
    /// The reserved barrier was consumed after every admitted normal command,
    /// every worker-owned resource was dropped, and the thread was joined.
    Complete(ExtensionServiceShutdownEvidence),
    /// The worker panicked before it could mint clean-exit evidence.
    WorkerPanicked,
    /// The worker exited without panic but did not return its required proof.
    EvidenceMissing,
    /// The deadline elapsed before termination and resource release were
    /// proven; the worker was detached and this is not cleanup evidence.
    DeadlineExceeded,
}
