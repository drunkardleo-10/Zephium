use std::cell::Cell;
use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::evidence::ExtensionServiceShutdownEvidence;
#[cfg(test)]
use crate::mailbox::NormalAdmission;
use crate::mailbox::{Delivery, Mailbox, ShutdownAdmission};
use crate::ports::{ExtensionServiceShutdownOutcome, ExtensionServiceStatusPort};
use crate::status::{
    ExtensionServicePhase, ExtensionServiceStatusSnapshot, ExtensionServiceStatusWait, SharedStatus,
};
use crate::ExtensionServiceWorkerIdentity;

const WORKER_NAME: &str = "zephium-extension-service";

/// Maximum time used by [`ExtensionServiceOwner::shutdown`].
pub const EXTENSION_SERVICE_DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

const EXTENSION_SERVICE_DROP_GRACE: Duration = Duration::from_millis(100);
const THREAD_FINISH_POLL_INTERVAL: Duration = Duration::from_millis(1);

#[cfg(test)]
struct TestDropProbe(Arc<AtomicBool>);

#[cfg(test)]
impl Drop for TestDropProbe {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

enum WorkerCommand {
    #[cfg(test)]
    Drive,
    #[cfg(test)]
    Panic,
    #[cfg(test)]
    ExitWithoutEvidence,
    #[cfg(test)]
    RetainDropProbe(TestDropProbe),
    #[cfg(test)]
    Block(Receiver<()>),
}

struct WorkerCancellation {
    requested: AtomicBool,
}

impl WorkerCancellation {
    fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
        }
    }

    fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

/// Thread-safe, cloneable observation handle for the extension service.
///
/// The handle intentionally carries neither mutation inputs nor shutdown
/// authority. Dropping every handle does not stop the worker; the unique
/// [`ExtensionServiceOwner`] controls its lifetime.
#[derive(Clone)]
pub struct ExtensionServiceHandle {
    worker: ExtensionServiceWorkerIdentity,
    status: Arc<SharedStatus>,
}

impl ExtensionServiceHandle {
    /// Returns the exact process-local worker identity.
    pub const fn worker_identity(&self) -> ExtensionServiceWorkerIdentity {
        self.worker
    }

    /// Returns the latest lifecycle snapshot without blocking on actor work.
    pub fn status(&self) -> ExtensionServiceStatusSnapshot {
        self.status.snapshot()
    }

    /// Waits without polling for a status snapshot newer than `after`.
    ///
    /// Passing a snapshot from another worker returns immediately so a
    /// replacement worker cannot be hidden behind a larger stale revision.
    pub fn wait_for_status_change(
        &self,
        after: ExtensionServiceStatusSnapshot,
        timeout: Duration,
    ) -> ExtensionServiceStatusWait {
        self.status.wait_for_change(after, timeout)
    }
}

impl ExtensionServiceStatusPort for ExtensionServiceHandle {
    fn extension_service_status(&self) -> ExtensionServiceStatusSnapshot {
        self.status()
    }

    fn wait_for_extension_service_status(
        &self,
        after: ExtensionServiceStatusSnapshot,
        timeout: Duration,
    ) -> ExtensionServiceStatusWait {
        self.wait_for_status_change(after, timeout)
    }
}

/// Unique, move-only owner of the serialized extension-service worker.
///
/// The owner can move between threads but cannot be shared by reference.
/// Consuming [`Self::shutdown_until`] returns explicit terminal evidence or a
/// precise failure. Dropping the owner grants the worker a short, bounded grace
/// period and then detaches it if necessary; callers that require proof must
/// shut down explicitly. The cloneable observation handle is both `Send` and
/// `Sync`.
pub struct ExtensionServiceOwner {
    worker: ExtensionServiceWorkerIdentity,
    mailbox: Arc<Mailbox<WorkerCommand>>,
    status: Arc<SharedStatus>,
    cancellation: Arc<WorkerCancellation>,
    completion: Receiver<ExtensionServiceShutdownEvidence>,
    thread: Option<JoinHandle<()>>,
    _not_sync: PhantomData<Cell<()>>,
}

/// Failure to create the dedicated extension-service worker thread.
#[derive(Debug)]
pub struct ExtensionServiceSpawnError {
    source: io::Error,
}

impl ExtensionServiceSpawnError {
    /// Returns the operating-system thread creation failure.
    pub const fn source_error(&self) -> &io::Error {
        &self.source
    }
}

impl fmt::Display for ExtensionServiceSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "cannot start extension service: {}", self.source)
    }
}

impl std::error::Error for ExtensionServiceSpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

impl ExtensionServiceOwner {
    /// Starts one empty serialized extension-service worker.
    ///
    /// This returns after thread creation, not readiness; callers observe
    /// readiness through [`ExtensionServiceHandle`]. That separation lets a
    /// shutdown request cancel future worker-side startup recovery.
    ///
    /// Repository construction is deliberately absent from this foundation;
    /// a later typed launch input will be opened on this worker rather than on
    /// the browser/UI startup thread.
    pub fn spawn() -> Result<Self, ExtensionServiceSpawnError> {
        let worker =
            ExtensionServiceWorkerIdentity::mint().ok_or_else(|| ExtensionServiceSpawnError {
                source: io::Error::other("extension-service worker identity exhausted"),
            })?;
        let mailbox = Arc::new(Mailbox::new());
        let status = Arc::new(SharedStatus::new(worker));
        let cancellation = Arc::new(WorkerCancellation::new());
        let (completion_tx, completion) = mpsc::sync_channel(1);
        let worker_mailbox = Arc::clone(&mailbox);
        let worker_status = Arc::clone(&status);
        let worker_cancellation = Arc::clone(&cancellation);
        let thread = thread::Builder::new()
            .name(WORKER_NAME.to_owned())
            .spawn(move || {
                let worker_result = panic::catch_unwind(AssertUnwindSafe(|| {
                    let mut state = WorkerState::new();
                    // Future typed startup recovery runs here and receives
                    // `worker_cancellation`; spawn deliberately returns before
                    // readiness so shutdown can cancel that recovery.
                    worker_status.publish_ready();
                    run_worker(&mut state, &worker_mailbox, &worker_cancellation)
                }));

                worker_mailbox.close();
                match worker_result {
                    Ok(Some(summary)) => {
                        // The actor state and every future repository/native
                        // resource it owns are dropped before clean-exit
                        // evidence can become observable.
                        drop(worker_cancellation);
                        drop(worker_status);
                        drop(worker_mailbox);
                        let evidence = ExtensionServiceShutdownEvidence::new(
                            worker,
                            summary.accepted_normal,
                            summary.completed_normal,
                        );
                        let _ = completion_tx.try_send(evidence);
                    }
                    Ok(None) => {
                        worker_status.publish(ExtensionServicePhase::Failed);
                    }
                    Err(payload) => {
                        worker_status.publish(ExtensionServicePhase::Failed);
                        drop(worker_cancellation);
                        drop(worker_status);
                        drop(worker_mailbox);
                        drop(completion_tx);
                        panic::resume_unwind(payload);
                    }
                }
            })
            .map_err(|source| ExtensionServiceSpawnError { source })?;

        Ok(Self {
            worker,
            mailbox,
            status,
            cancellation,
            completion,
            thread: Some(thread),
            _not_sync: PhantomData,
        })
    }

    /// Creates a cloneable, read-only handle for this exact worker.
    pub fn handle(&self) -> ExtensionServiceHandle {
        ExtensionServiceHandle {
            worker: self.worker,
            status: Arc::clone(&self.status),
        }
    }

    /// Seals admission and waits up to the default bounded shutdown timeout.
    #[must_use = "extension-service shutdown must be checked before shutting down its Store"]
    pub fn shutdown(mut self) -> ExtensionServiceShutdownOutcome {
        let started = Instant::now();
        let deadline = started
            .checked_add(EXTENSION_SERVICE_DEFAULT_SHUTDOWN_TIMEOUT)
            .unwrap_or(started);
        self.shutdown_inner(deadline)
    }

    /// Seals admission and tries to prove worker termination by `deadline`.
    ///
    /// The deadline is absolute for worker observation and joining. Tiny
    /// internal mailbox/status critical sections contain no foreign work but
    /// may add scheduler latency, so this is a bounded lifecycle policy rather
    /// than a hard real-time guarantee. When proof is not available in time,
    /// the worker is detached after receiving both a cooperative cancellation
    /// request and the reserved shutdown barrier. A deadline result is
    /// deliberately not cleanup proof.
    #[must_use = "extension-service shutdown must be checked before shutting down its Store"]
    pub fn shutdown_until(mut self, deadline: Instant) -> ExtensionServiceShutdownOutcome {
        self.shutdown_inner(deadline)
    }

    fn shutdown_inner(&mut self, deadline: Instant) -> ExtensionServiceShutdownOutcome {
        let Some(thread) = self.thread.take() else {
            self.status.publish(ExtensionServicePhase::Failed);
            return ExtensionServiceShutdownOutcome::EvidenceMissing;
        };
        self.cancellation.request();
        match self.mailbox.try_push_shutdown() {
            ShutdownAdmission::Accepted | ShutdownAdmission::AlreadyEnqueued => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
            }
            ShutdownAdmission::Closed => {}
        }

        let mut evidence = receive_completion_until(&self.completion, deadline);
        if !wait_for_thread_finish(&thread, deadline) {
            self.status.publish(ExtensionServicePhase::Failed);
            return ExtensionServiceShutdownOutcome::DeadlineExceeded;
        }
        if evidence.is_none() {
            evidence = self.completion.try_recv().ok();
        }
        if thread.join().is_err() {
            self.status.publish(ExtensionServicePhase::Failed);
            self.mailbox.close();
            return ExtensionServiceShutdownOutcome::WorkerPanicked;
        }
        match evidence {
            Some(evidence) if evidence.worker() == self.worker => {
                self.status.publish(ExtensionServicePhase::Stopped);
                ExtensionServiceShutdownOutcome::Complete(evidence)
            }
            Some(_) | None => {
                self.status.publish(ExtensionServicePhase::Failed);
                ExtensionServiceShutdownOutcome::EvidenceMissing
            }
        }
    }

    #[cfg(test)]
    fn try_drive_for_test(&self) -> NormalAdmission<WorkerCommand> {
        self.mailbox.try_push_normal(WorkerCommand::Drive)
    }

    #[cfg(test)]
    fn try_panic_for_test(&self) -> NormalAdmission<WorkerCommand> {
        self.mailbox.try_push_normal(WorkerCommand::Panic)
    }

    #[cfg(test)]
    fn try_exit_without_evidence_for_test(&self) -> NormalAdmission<WorkerCommand> {
        self.mailbox
            .try_push_normal(WorkerCommand::ExitWithoutEvidence)
    }

    #[cfg(test)]
    fn try_retain_drop_probe_for_test(
        &self,
        probe: TestDropProbe,
    ) -> NormalAdmission<WorkerCommand> {
        self.mailbox
            .try_push_normal(WorkerCommand::RetainDropProbe(probe))
    }

    #[cfg(test)]
    fn try_block_for_test(&self, release: Receiver<()>) -> NormalAdmission<WorkerCommand> {
        self.mailbox.try_push_normal(WorkerCommand::Block(release))
    }
}

impl Drop for ExtensionServiceOwner {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let started = Instant::now();
            let deadline = started
                .checked_add(EXTENSION_SERVICE_DROP_GRACE)
                .unwrap_or(started);
            let _ = self.shutdown_inner(deadline);
        }
    }
}

fn receive_completion_until(
    completion: &Receiver<ExtensionServiceShutdownEvidence>,
    deadline: Instant,
) -> Option<ExtensionServiceShutdownEvidence> {
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return completion.try_recv().ok();
    };
    completion.recv_timeout(remaining).ok()
}

fn wait_for_thread_finish(thread: &JoinHandle<()>, deadline: Instant) -> bool {
    loop {
        if thread.is_finished() {
            return true;
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return thread.is_finished();
        };
        thread::park_timeout(remaining.min(THREAD_FINISH_POLL_INTERVAL));
    }
}

struct WorkerState {
    completed_normal: u64,
    #[cfg(test)]
    retained_probe: Option<TestDropProbe>,
}

impl WorkerState {
    fn new() -> Self {
        Self {
            completed_normal: 0,
            #[cfg(test)]
            retained_probe: None,
        }
    }

    fn complete(&mut self, command: WorkerCommand) -> bool {
        #[cfg(not(test))]
        {
            match command {}
        }
        #[cfg(test)]
        {
            match command {
                WorkerCommand::Drive => {}
                WorkerCommand::Panic => {
                    panic!("extension-service worker panic requested by test")
                }
                WorkerCommand::ExitWithoutEvidence => return false,
                WorkerCommand::RetainDropProbe(probe) => self.retained_probe = Some(probe),
                WorkerCommand::Block(release) => {
                    let _ = release.recv();
                }
            }
            self.completed_normal = self
                .completed_normal
                .checked_add(1)
                .expect("mailbox admission proves the completion counter bound");
            true
        }
    }
}

struct WorkerShutdownSummary {
    accepted_normal: u64,
    completed_normal: u64,
}

fn run_worker(
    state: &mut WorkerState,
    mailbox: &Mailbox<WorkerCommand>,
    cancellation: &WorkerCancellation,
) -> Option<WorkerShutdownSummary> {
    loop {
        match mailbox.receive() {
            Delivery::Normal(command) => {
                if !state.complete(command) {
                    return None;
                }
            }
            Delivery::Shutdown { accepted_normal } => {
                if !cancellation.is_requested() || state.completed_normal != accepted_normal {
                    return None;
                }
                return Some(WorkerShutdownSummary {
                    accepted_normal,
                    completed_normal: state.completed_normal,
                });
            }
            Delivery::Closed => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::EXTENSION_SERVICE_NORMAL_CAPACITY;

    fn assert_send<T: Send>() {}
    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn owner_is_send_and_handle_is_send_sync() {
        assert_send::<ExtensionServiceOwner>();
        assert_send_sync::<ExtensionServiceHandle>();
        let owner = ExtensionServiceOwner::spawn().unwrap();
        let handle = owner.handle();
        let ready = handle.wait_for_status_change(handle.status(), Duration::from_secs(1));
        let snapshot = match ready {
            ExtensionServiceStatusWait::Changed(snapshot) => snapshot,
            ExtensionServiceStatusWait::TimedOut(snapshot) => snapshot,
        };
        assert_eq!(snapshot.worker(), handle.worker_identity());
        assert_eq!(snapshot.phase(), ExtensionServicePhase::Ready);
        let outcome = thread::spawn(move || owner.shutdown()).join().unwrap();
        assert!(matches!(
            outcome,
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
    }

    #[test]
    fn shutdown_drains_a_full_normal_fifo_and_returns_exact_evidence() {
        let owner = ExtensionServiceOwner::spawn().unwrap();
        let worker = owner.worker;
        for _ in 0..EXTENSION_SERVICE_NORMAL_CAPACITY {
            assert!(matches!(
                owner.try_drive_for_test(),
                NormalAdmission::Accepted
            ));
        }
        let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown() else {
            panic!("worker must drain and exit cleanly")
        };
        assert_eq!(evidence.worker(), worker);
        assert_eq!(
            evidence.accepted_normal_commands(),
            EXTENSION_SERVICE_NORMAL_CAPACITY as u64
        );
        assert_eq!(
            evidence.completed_normal_commands(),
            EXTENSION_SERVICE_NORMAL_CAPACITY as u64
        );
    }

    #[test]
    fn dropping_owner_orders_shutdown_and_joins_before_returning() {
        let handle = {
            let owner = ExtensionServiceOwner::spawn().unwrap();
            let handle = owner.handle();
            assert!(matches!(
                owner.try_drive_for_test(),
                NormalAdmission::Accepted
            ));
            handle
        };
        assert_eq!(handle.status().phase(), ExtensionServicePhase::Stopped);
    }

    #[test]
    fn explicit_shutdown_reports_worker_panic_and_publishes_failure() {
        let owner = ExtensionServiceOwner::spawn().unwrap();
        let handle = owner.handle();
        assert!(matches!(
            owner.try_panic_for_test(),
            NormalAdmission::Accepted
        ));
        assert_eq!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::WorkerPanicked
        );
        assert_eq!(handle.status().phase(), ExtensionServicePhase::Failed);
    }

    #[test]
    fn explicit_shutdown_distinguishes_clean_exit_without_evidence() {
        let owner = ExtensionServiceOwner::spawn().unwrap();
        let handle = owner.handle();
        assert!(matches!(
            owner.try_exit_without_evidence_for_test(),
            NormalAdmission::Accepted
        ));
        assert_eq!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::EvidenceMissing
        );
        assert_eq!(handle.status().phase(), ExtensionServicePhase::Failed);
    }

    #[test]
    fn completion_evidence_is_observable_only_after_worker_resources_drop() {
        let owner = ExtensionServiceOwner::spawn().unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        assert!(matches!(
            owner.try_retain_drop_probe_for_test(TestDropProbe(Arc::clone(&dropped))),
            NormalAdmission::Accepted
        ));
        assert!(!dropped.load(Ordering::Acquire));
        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert!(dropped.load(Ordering::Acquire));
    }

    #[test]
    fn absolute_shutdown_deadline_returns_without_joining_a_blocked_worker() {
        let owner = ExtensionServiceOwner::spawn().unwrap();
        let handle = owner.handle();
        let (release_tx, release) = mpsc::sync_channel(0);
        assert!(matches!(
            owner.try_block_for_test(release),
            NormalAdmission::Accepted
        ));
        let releaser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            let _ = release_tx.send(());
        });
        let started = Instant::now();
        let deadline = started.checked_add(Duration::from_millis(20)).unwrap();
        assert_eq!(
            owner.shutdown_until(deadline),
            ExtensionServiceShutdownOutcome::DeadlineExceeded
        );
        assert!(started.elapsed() < Duration::from_millis(200));
        assert_eq!(handle.status().phase(), ExtensionServicePhase::Failed);
        releaser.join().unwrap();
    }

    #[test]
    fn drop_has_bounded_grace_and_publishes_failure_before_detaching() {
        let owner = ExtensionServiceOwner::spawn().unwrap();
        let handle = owner.handle();
        let (release_tx, release) = mpsc::sync_channel(0);
        assert!(matches!(
            owner.try_block_for_test(release),
            NormalAdmission::Accepted
        ));
        let releaser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            let _ = release_tx.send(());
        });
        let started = Instant::now();
        drop(owner);
        assert!(started.elapsed() < Duration::from_millis(200));
        assert_eq!(handle.status().phase(), ExtensionServicePhase::Failed);
        releaser.join().unwrap();
    }

    #[test]
    fn unchanged_status_wait_observes_its_deadline() {
        let owner = ExtensionServiceOwner::spawn().unwrap();
        let handle = owner.handle();
        let snapshot = loop {
            let snapshot = handle.status();
            if snapshot.phase() == ExtensionServicePhase::Ready {
                break snapshot;
            }
            thread::yield_now();
        };
        assert!(matches!(
            handle.wait_for_status_change(snapshot, Duration::from_millis(1)),
            ExtensionServiceStatusWait::TimedOut(value) if value == snapshot
        ));
        let _ = owner.shutdown();
    }

    #[test]
    fn status_wait_cursor_is_scoped_to_its_exact_worker() {
        let first = ExtensionServiceOwner::spawn().unwrap();
        let first_handle = first.handle();
        let first_snapshot = loop {
            let snapshot = first_handle.status();
            if snapshot.phase() == ExtensionServicePhase::Ready {
                break snapshot;
            }
            thread::yield_now();
        };
        let _ = first.shutdown();

        let second = ExtensionServiceOwner::spawn().unwrap();
        let second_handle = second.handle();
        assert!(matches!(
            second_handle.wait_for_status_change(first_snapshot, Duration::ZERO),
            ExtensionServiceStatusWait::Changed(snapshot)
                if snapshot.worker() == second_handle.worker_identity()
        ));
        let _ = second.shutdown();
    }
}
