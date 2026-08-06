use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::ExtensionServiceWorkerIdentity;

// An overflowing `Instant + Duration` represents a wait beyond the platform's
// monotonic range. Sleep in bounded chunks so it remains interruptible by a
// status notification and never becomes an immediate timeout.
const OVERFLOW_WAIT_SLICE: Duration = Duration::from_secs(24 * 60 * 60);

/// Process-local lifecycle phase of an extension-service worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServicePhase {
    /// The native thread exists but has not published readiness yet.
    Starting,
    /// The worker is admitting and recovering its private repository.
    OpeningRepository,
    /// The worker is loading the complete durable native-ownership journal.
    LoadingOwnershipJournal,
    /// The worker is reconciling cleanup-only ownership and package state.
    ReconcilingCleanup,
    /// Startup recovery completed with no unresolved native-owner rows.
    Ready,
    /// Durable possible-owner rows remain and extension activation is disabled.
    CleanupRequired,
    /// Startup did not make a readiness claim and may be retried by the owner.
    StartupUnavailable,
    /// Startup failed closed; extension activation remains disabled.
    StartupFailed,
    /// Admission is sealed and the reserved shutdown barrier is queued.
    ShutdownQueued,
    /// The owner joined the worker and validated its clean-exit evidence.
    Stopped,
    /// The lifecycle protocol failed or its deadline elapsed without proof.
    Failed,
}

/// Immutable lifecycle observation for one exact worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionServiceStatusSnapshot {
    worker: ExtensionServiceWorkerIdentity,
    revision: u64,
    phase: ExtensionServicePhase,
}

impl ExtensionServiceStatusSnapshot {
    pub(crate) const fn starting(worker: ExtensionServiceWorkerIdentity) -> Self {
        Self {
            worker,
            revision: 1,
            phase: ExtensionServicePhase::Starting,
        }
    }

    /// Returns the exact worker described by this snapshot.
    pub const fn worker(self) -> ExtensionServiceWorkerIdentity {
        self.worker
    }

    /// Returns the non-zero, monotonically increasing process-local revision.
    pub const fn revision(self) -> u64 {
        self.revision
    }

    /// Returns the observed lifecycle phase.
    pub const fn phase(self) -> ExtensionServicePhase {
        self.phase
    }
}

/// Result of a deadline-bounded status wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceStatusWait {
    /// A snapshot from another worker or a newer same-worker revision exists.
    Changed(ExtensionServiceStatusSnapshot),
    /// The deadline elapsed; the included snapshot is still the latest one.
    TimedOut(ExtensionServiceStatusSnapshot),
}

pub(crate) struct SharedStatus {
    snapshot: Mutex<ExtensionServiceStatusSnapshot>,
    changed: Condvar,
}

impl SharedStatus {
    pub(crate) fn new(worker: ExtensionServiceWorkerIdentity) -> Self {
        Self {
            snapshot: Mutex::new(ExtensionServiceStatusSnapshot::starting(worker)),
            changed: Condvar::new(),
        }
    }

    pub(crate) fn snapshot(&self) -> ExtensionServiceStatusSnapshot {
        *self.lock()
    }

    #[cfg(test)]
    pub(crate) fn publish_ready(&self) {
        self.publish_startup(ExtensionServicePhase::Ready);
    }

    pub(crate) fn publish_startup(&self, phase: ExtensionServicePhase) {
        let mut snapshot = self.lock();
        if matches!(
            snapshot.phase,
            ExtensionServicePhase::Ready
                | ExtensionServicePhase::CleanupRequired
                | ExtensionServicePhase::StartupFailed
                | ExtensionServicePhase::ShutdownQueued
                | ExtensionServicePhase::Stopped
                | ExtensionServicePhase::Failed
        ) {
            return;
        }
        Self::publish_locked(&mut snapshot, phase, &self.changed);
    }

    pub(crate) fn publish(&self, phase: ExtensionServicePhase) {
        let mut snapshot = self.lock();
        if matches!(
            snapshot.phase,
            ExtensionServicePhase::Stopped | ExtensionServicePhase::Failed
        ) {
            return;
        }
        Self::publish_locked(&mut snapshot, phase, &self.changed);
    }

    fn publish_locked(
        snapshot: &mut ExtensionServiceStatusSnapshot,
        phase: ExtensionServicePhase,
        changed: &Condvar,
    ) {
        if snapshot.phase == phase {
            return;
        }
        let Some(revision) = snapshot.revision.checked_add(1) else {
            snapshot.phase = ExtensionServicePhase::Failed;
            changed.notify_all();
            return;
        };
        snapshot.revision = revision;
        snapshot.phase = phase;
        changed.notify_all();
    }

    pub(crate) fn wait_for_change(
        &self,
        after: ExtensionServiceStatusSnapshot,
        timeout: Duration,
    ) -> ExtensionServiceStatusWait {
        let started = Instant::now();
        let deadline = started.checked_add(timeout);
        let mut snapshot = self.lock();
        loop {
            if snapshot.worker != after.worker || snapshot.revision > after.revision {
                return ExtensionServiceStatusWait::Changed(*snapshot);
            }
            let remaining = match deadline {
                Some(deadline) => {
                    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                        return ExtensionServiceStatusWait::TimedOut(*snapshot);
                    };
                    remaining
                }
                None => OVERFLOW_WAIT_SLICE,
            };
            let waited = self
                .changed
                .wait_timeout(snapshot, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            snapshot = waited.0;
            if deadline.is_some()
                && waited.1.timed_out()
                && snapshot.worker == after.worker
                && snapshot.revision <= after.revision
            {
                return ExtensionServiceStatusWait::TimedOut(*snapshot);
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, ExtensionServiceStatusSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_readiness_cannot_regress_a_queued_shutdown() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let status = SharedStatus::new(worker);
        status.publish(ExtensionServicePhase::ShutdownQueued);
        let queued = status.snapshot();

        status.publish_ready();

        assert_eq!(status.snapshot(), queued);
    }

    #[test]
    fn terminal_status_cannot_regress_to_a_late_shutdown_publication() {
        for terminal in [
            ExtensionServicePhase::Stopped,
            ExtensionServicePhase::Failed,
        ] {
            let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
            let status = SharedStatus::new(worker);
            status.publish(terminal);
            let settled = status.snapshot();

            status.publish(ExtensionServicePhase::ShutdownQueued);

            assert_eq!(status.snapshot(), settled);
        }
    }

    #[test]
    fn readiness_can_follow_startup_reconciliation() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let status = SharedStatus::new(worker);
        status.publish(ExtensionServicePhase::OpeningRepository);
        status.publish(ExtensionServicePhase::LoadingOwnershipJournal);
        status.publish(ExtensionServicePhase::ReconcilingCleanup);

        status.publish_ready();

        assert_eq!(status.snapshot().phase(), ExtensionServicePhase::Ready);
    }

    #[test]
    fn readiness_cannot_regress_a_settled_startup_outcome() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let status = SharedStatus::new(worker);
        status.publish(ExtensionServicePhase::CleanupRequired);
        let settled = status.snapshot();

        status.publish_ready();

        assert_eq!(status.snapshot(), settled);
    }

    #[test]
    fn overflowing_timeout_remains_a_real_wait() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let status = std::sync::Arc::new(SharedStatus::new(worker));
        let cursor = status.snapshot();
        let publisher = status.clone();
        let change = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1));
            publisher.publish_ready();
        });

        assert!(matches!(
            status.wait_for_change(cursor, Duration::MAX),
            ExtensionServiceStatusWait::Changed(snapshot)
                if snapshot.phase() == ExtensionServicePhase::Ready
        ));
        change.join().unwrap();
    }
}
