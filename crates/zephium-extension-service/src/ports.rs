use std::time::{Duration, Instant};

use zephium_core::ports::extensions::{
    ExtensionServiceLifecycle,
    ExtensionServiceShutdownOutcome as CoreExtensionServiceShutdownOutcome,
};

use crate::{
    ExtensionServiceOwner, ExtensionServiceShutdownEvidence, ExtensionServiceStatusSnapshot,
    ExtensionServiceStatusWait,
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

impl ExtensionServiceLifecycle for ExtensionServiceOwner {
    fn shutdown_until(self: Box<Self>, deadline: Instant) -> CoreExtensionServiceShutdownOutcome {
        project_lifecycle_shutdown_outcome(ExtensionServiceOwner::shutdown_until(*self, deadline))
    }
}

fn project_lifecycle_shutdown_outcome(
    outcome: ExtensionServiceShutdownOutcome,
) -> CoreExtensionServiceShutdownOutcome {
    match outcome {
        ExtensionServiceShutdownOutcome::Complete(_) => CoreExtensionServiceShutdownOutcome::Clean,
        ExtensionServiceShutdownOutcome::WorkerPanicked
        | ExtensionServiceShutdownOutcome::EvidenceMissing
        | ExtensionServiceShutdownOutcome::DeadlineExceeded => {
            CoreExtensionServiceShutdownOutcome::Unclean
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExtensionServiceWorkerIdentity;

    #[test]
    fn only_evidenced_completion_projects_to_clean() {
        for outcome in [
            ExtensionServiceShutdownOutcome::WorkerPanicked,
            ExtensionServiceShutdownOutcome::EvidenceMissing,
            ExtensionServiceShutdownOutcome::DeadlineExceeded,
        ] {
            assert_eq!(
                project_lifecycle_shutdown_outcome(outcome),
                CoreExtensionServiceShutdownOutcome::Unclean
            );
        }

        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let evidence = ExtensionServiceShutdownEvidence::new(worker, 0, 0);
        assert_eq!(
            project_lifecycle_shutdown_outcome(ExtensionServiceShutdownOutcome::Complete(evidence)),
            CoreExtensionServiceShutdownOutcome::Clean
        );
    }
}
