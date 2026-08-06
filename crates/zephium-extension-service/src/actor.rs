use std::cell::Cell;
use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::cleanup::{
    reconcile_startup, CancellationCheck, CleanupFailure, CleanupProgress, CleanupStartupOutcome,
    CleanupUnavailable,
};
use crate::evidence::ExtensionServiceShutdownEvidence;
use crate::journal_store::JournalProjection;
#[cfg(test)]
use crate::mailbox::NormalAdmission;
use crate::mailbox::{Delivery, Mailbox, ShutdownAdmission};
use crate::native_recovery::NativeRecoveryState;
use crate::ports::{ExtensionServiceShutdownOutcome, ExtensionServiceStatusPort};
use crate::repository::ServiceRepository;
use crate::startup::{
    CurrentStartupObservation, ExtensionServiceLaunchInput, ExtensionServiceStartupFailure,
    ExtensionServiceStartupFailureReason, ExtensionServiceStartupOutcome,
    ExtensionServiceStartupUnavailable, ExtensionServiceStartupUnavailableReason,
    ExtensionServiceStartupWait, SharedStartupOutcome, StartupAttempt, StartupAttemptWait,
    StartupRetryReservation,
};
use crate::status::{
    ExtensionServicePhase, ExtensionServiceStatusSnapshot, ExtensionServiceStatusWait, SharedStatus,
};
use crate::{
    ExtensionServiceCleanupEvidence, ExtensionServiceReadyEvidence, ExtensionServiceWorkerIdentity,
};

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
    RetryStartup {
        attempt: StartupAttempt,
        deadline: Instant,
    },
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
    shutdown_deadline: Mutex<Option<Instant>>,
}

impl WorkerCancellation {
    fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            shutdown_deadline: Mutex::new(None),
        }
    }

    fn request(&self, deadline: Instant) {
        *self.lock_shutdown_deadline() = Some(deadline);
        self.requested.store(true, Ordering::Release);
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    fn shutdown_deadline(&self) -> Option<Instant> {
        *self.lock_shutdown_deadline()
    }

    fn lock_shutdown_deadline(&self) -> MutexGuard<'_, Option<Instant>> {
        self.shutdown_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl CancellationCheck for WorkerCancellation {
    fn is_cancelled(&self) -> bool {
        self.is_requested()
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
/// shut down explicitly. An attached unresolved native recovery proxy is
/// retained in a parked fail-stop worker rather than destroyed, so such a
/// shutdown cannot produce clean evidence. The cloneable observation handle is
/// both `Send` and `Sync`.
pub struct ExtensionServiceOwner {
    worker: ExtensionServiceWorkerIdentity,
    mailbox: Arc<Mailbox<WorkerCommand>>,
    status: Arc<SharedStatus>,
    startup: Arc<SharedStartupOutcome>,
    cancellation: Arc<WorkerCancellation>,
    completion: Receiver<ExtensionServiceShutdownEvidence>,
    thread: Option<JoinHandle<()>>,
    _not_sync: PhantomData<Cell<()>>,
}

/// Failure to start the dedicated extension-service worker thread.
///
/// The refusal retains the exact move-only launch input. A transient operating
/// system thread-creation failure therefore cannot consume Store's one-shot
/// native-ownership capability or force a process restart merely to retry.
pub struct ExtensionServiceSpawnError {
    source: io::Error,
    input: ExtensionServiceLaunchInput,
}

impl ExtensionServiceSpawnError {
    /// Returns the operating-system worker-start failure.
    pub const fn source_error(&self) -> &io::Error {
        &self.source
    }

    /// Recovers the exact launch input without weakening either authority.
    #[must_use = "the retained Store authority cannot be claimed again"]
    pub fn into_launch_input(self) -> ExtensionServiceLaunchInput {
        self.input
    }
}

impl fmt::Debug for ExtensionServiceSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionServiceSpawnError")
            .field("source", &self.source)
            .field("input", &"[retained]")
            .finish()
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

type WorkerTask = Box<dyn FnOnce() + Send + 'static>;

struct WorkerSpawnFailure {
    source: io::Error,
    launch: Option<(ExtensionServiceLaunchInput, Instant)>,
}

fn spawn_system_worker(task: WorkerTask) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name(WORKER_NAME.to_owned())
        .spawn(task)
}

impl ExtensionServiceOwner {
    /// Launches the serialized extension service with its unique durable and
    /// native-host authorities.
    ///
    /// This returns after thread creation, before repository I/O. Use
    /// [`Self::wait_for_startup_until`] to obtain explicit readiness or
    /// cleanup-required evidence. Repository opening and every cleanup
    /// frontier run only on the dedicated worker. If worker startup fails,
    /// [`ExtensionServiceSpawnError::into_launch_input`] returns the exact
    /// one-shot authorities for a later retry.
    pub fn launch(
        input: ExtensionServiceLaunchInput,
        startup_deadline: Instant,
    ) -> Result<Self, ExtensionServiceSpawnError> {
        Self::launch_with_spawner(input, startup_deadline, spawn_system_worker)
    }

    #[cfg(test)]
    fn spawn_empty_for_test() -> io::Result<Self> {
        Self::spawn_worker_with(None, spawn_system_worker).map_err(|failure| failure.source)
    }

    fn launch_with_spawner(
        input: ExtensionServiceLaunchInput,
        startup_deadline: Instant,
        spawner: impl FnOnce(WorkerTask) -> io::Result<JoinHandle<()>>,
    ) -> Result<Self, ExtensionServiceSpawnError> {
        Self::spawn_worker_with(Some((input, startup_deadline)), spawner).map_err(|failure| {
            let (input, _) = failure
                .launch
                .expect("production launch always retains its exact input");
            ExtensionServiceSpawnError {
                source: failure.source,
                input,
            }
        })
    }

    fn spawn_worker_with(
        launch: Option<(ExtensionServiceLaunchInput, Instant)>,
        spawner: impl FnOnce(WorkerTask) -> io::Result<JoinHandle<()>>,
    ) -> Result<Self, WorkerSpawnFailure> {
        let Some(worker) = ExtensionServiceWorkerIdentity::mint() else {
            return Err(WorkerSpawnFailure {
                source: io::Error::other("extension-service worker identity exhausted"),
                launch,
            });
        };
        let mailbox = Arc::new(Mailbox::new());
        let status = Arc::new(SharedStatus::new(worker));
        let initial_attempt = launch.is_some().then_some(StartupAttempt::INITIAL);
        let startup = Arc::new(SharedStartupOutcome::new(initial_attempt));
        let cancellation = Arc::new(WorkerCancellation::new());
        let (completion_tx, completion) = mpsc::sync_channel(1);
        let worker_mailbox = Arc::clone(&mailbox);
        let worker_status = Arc::clone(&status);
        let worker_startup = Arc::clone(&startup);
        let worker_cancellation = Arc::clone(&cancellation);
        let (launch_tx, launch_rx) =
            mpsc::sync_channel::<Option<(ExtensionServiceLaunchInput, Instant)>>(1);
        let task: WorkerTask = Box::new(move || {
            let Ok(launch) = launch_rx.recv() else {
                worker_mailbox.close();
                worker_status.publish(ExtensionServicePhase::Failed);
                return;
            };
            let mut state = WorkerState::new(launch.map(|(input, deadline)| {
                (
                    WorkerStartupState::new(input),
                    deadline,
                    StartupAttempt::INITIAL,
                )
            }));
            let worker_result = panic::catch_unwind(AssertUnwindSafe(|| {
                if !state.start(
                    worker,
                    &worker_status,
                    &worker_startup,
                    &worker_cancellation,
                ) {
                    return None;
                }
                run_worker(
                    &mut state,
                    worker,
                    &worker_mailbox,
                    &worker_status,
                    &worker_startup,
                    &worker_cancellation,
                )
            }));

            worker_mailbox.close();
            match worker_result {
                Ok(Some(summary)) => {
                    if state.has_attached_native_obligation() {
                        worker_status.publish(ExtensionServicePhase::Failed);
                        retain_fail_stopped_native_obligation(&mut state);
                    }
                    // The actor state and every future repository/native
                    // resource it owns are dropped before clean-exit
                    // evidence can become observable.
                    drop(state);
                    drop(worker_cancellation);
                    drop(worker_startup);
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
                    fail_active_startup(worker, &worker_startup);
                    worker_status.publish(ExtensionServicePhase::Failed);
                    if state.has_attached_native_obligation() {
                        retain_fail_stopped_native_obligation(&mut state);
                    }
                }
                Err(payload) => {
                    fail_active_startup(worker, &worker_startup);
                    worker_status.publish(ExtensionServicePhase::Failed);
                    if state.has_attached_native_obligation() {
                        retain_fail_stopped_native_obligation(&mut state);
                    }
                    drop(state);
                    drop(worker_cancellation);
                    drop(worker_startup);
                    drop(worker_status);
                    drop(worker_mailbox);
                    drop(completion_tx);
                    panic::resume_unwind(payload);
                }
            }
        });
        let thread = match spawner(task) {
            Ok(thread) => thread,
            Err(source) => return Err(WorkerSpawnFailure { source, launch }),
        };
        if let Err(error) = launch_tx.try_send(launch) {
            let launch = match error {
                mpsc::TrySendError::Full(launch) | mpsc::TrySendError::Disconnected(launch) => {
                    launch
                }
            };
            mailbox.close();
            drop(launch_tx);
            let _ = thread.join();
            return Err(WorkerSpawnFailure {
                source: io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "extension-service launch handoff was not received",
                ),
                launch,
            });
        }

        Ok(Self {
            worker,
            mailbox,
            status,
            startup,
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

    /// Waits for the first startup settlement until an absolute observation
    /// deadline.
    ///
    /// A timeout carries no readiness claim. The worker may continue its
    /// currently admitted bounded filesystem operation and publish a result
    /// later; callers may invoke this method again.
    #[must_use = "extension activation requires an explicit Ready settlement"]
    pub fn wait_for_startup_until(&self, deadline: Instant) -> ExtensionServiceStartupWait {
        match self.startup.current() {
            CurrentStartupObservation::Await(attempt) => {
                self.wait_for_startup_attempt_until(attempt, deadline)
            }
            CurrentStartupObservation::Settled(outcome) => {
                ExtensionServiceStartupWait::Settled(outcome)
            }
            CurrentStartupObservation::Idle => {
                self.status.publish(ExtensionServicePhase::Failed);
                ExtensionServiceStartupWait::NotAdmitted(self.status.snapshot())
            }
        }
    }

    /// Retries a definitely unavailable startup attempt under one absolute
    /// operation and observation deadline.
    ///
    /// Ready, cleanup-required, and failed-closed outcomes are terminal for
    /// this worker and are returned without enqueueing work. If initial startup
    /// is still running, this method observes that attempt rather than
    /// admitting a concurrent one.
    #[must_use = "extension activation requires an explicit Ready settlement"]
    pub fn retry_startup_until(&mut self, deadline: Instant) -> ExtensionServiceStartupWait {
        match self.startup.current() {
            CurrentStartupObservation::Await(attempt) => {
                return self.wait_for_startup_attempt_until(attempt, deadline);
            }
            CurrentStartupObservation::Settled(outcome)
                if !matches!(outcome, ExtensionServiceStartupOutcome::Unavailable(_)) =>
            {
                return ExtensionServiceStartupWait::Settled(outcome);
            }
            CurrentStartupObservation::Settled(_) => {}
            CurrentStartupObservation::Idle => {
                self.status.publish(ExtensionServicePhase::Failed);
                return ExtensionServiceStartupWait::NotAdmitted(self.status.snapshot());
            }
        }
        if Instant::now() >= deadline {
            return ExtensionServiceStartupWait::TimedOut(self.status.snapshot());
        }
        let attempt = match self.startup.reserve_retry() {
            StartupRetryReservation::Settled(outcome) => {
                return ExtensionServiceStartupWait::Settled(outcome);
            }
            StartupRetryReservation::Observe(attempt) => {
                return self.wait_for_startup_attempt_until(attempt, deadline);
            }
            StartupRetryReservation::Reserved(attempt) => attempt,
            StartupRetryReservation::Exhausted | StartupRetryReservation::InvariantViolation => {
                self.status.publish(ExtensionServicePhase::Failed);
                return ExtensionServiceStartupWait::NotAdmitted(self.status.snapshot());
            }
        };
        match self
            .mailbox
            .try_push_normal(WorkerCommand::RetryStartup { attempt, deadline })
        {
            crate::mailbox::NormalAdmission::Accepted => {}
            crate::mailbox::NormalAdmission::Full(_)
            | crate::mailbox::NormalAdmission::Sealed(_)
            | crate::mailbox::NormalAdmission::Closed(_) => {
                if !self.startup.cancel_retry_reservation(attempt) {
                    self.status.publish(ExtensionServicePhase::Failed);
                }
                return ExtensionServiceStartupWait::NotAdmitted(self.status.snapshot());
            }
            crate::mailbox::NormalAdmission::CounterExhausted(_) => {
                let _ = self.startup.cancel_retry_reservation(attempt);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                return ExtensionServiceStartupWait::NotAdmitted(self.status.snapshot());
            }
        }
        self.wait_for_startup_attempt_until(attempt, deadline)
    }

    fn wait_for_startup_attempt_until(
        &self,
        attempt: StartupAttempt,
        deadline: Instant,
    ) -> ExtensionServiceStartupWait {
        match self.startup.wait_for_attempt_until(attempt, deadline) {
            StartupAttemptWait::Settled(outcome) => ExtensionServiceStartupWait::Settled(outcome),
            StartupAttemptWait::TimedOut => {
                ExtensionServiceStartupWait::TimedOut(self.status.snapshot())
            }
            StartupAttemptWait::InvariantViolation => {
                self.status.publish(ExtensionServicePhase::Failed);
                ExtensionServiceStartupWait::NotAdmitted(self.status.snapshot())
            }
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
        self.cancellation.request(deadline);
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
    startup: Option<WorkerStartupState>,
    initial_startup: Option<(Instant, StartupAttempt)>,
    #[cfg(test)]
    retained_probe: Option<TestDropProbe>,
}

impl WorkerState {
    fn new(startup: Option<(WorkerStartupState, Instant, StartupAttempt)>) -> Self {
        let (startup, initial_startup) = match startup {
            Some((startup, deadline, attempt)) => (Some(startup), Some((deadline, attempt))),
            None => (None, None),
        };
        Self {
            completed_normal: 0,
            startup,
            initial_startup,
            #[cfg(test)]
            retained_probe: None,
        }
    }

    fn start(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
    ) -> bool {
        let Some((deadline, attempt)) = self.initial_startup.take() else {
            #[cfg(test)]
            status.publish_ready();
            return true;
        };
        self.attempt_startup(
            worker,
            attempt,
            deadline,
            status,
            startup_outcome,
            cancellation,
        )
    }

    fn has_attached_native_obligation(&self) -> bool {
        self.startup
            .as_ref()
            .is_some_and(|startup| startup.native_recovery.has_attached_obligation())
    }

    fn attempt_startup(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        attempt: StartupAttempt,
        deadline: Instant,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
    ) -> bool {
        let Some(startup) = self.startup.as_mut() else {
            return false;
        };
        if !startup_outcome.is_active(attempt) {
            return false;
        }
        status.publish_startup(ExtensionServicePhase::OpeningRepository);
        let outcome = reconcile_startup(
            &startup.store,
            &mut startup.projection,
            &mut startup.repository,
            Some(&mut startup.native_recovery),
            cancellation,
            deadline,
            |progress| match progress {
                CleanupProgress::LoadingOwnershipJournal => {
                    status.publish_startup(ExtensionServicePhase::LoadingOwnershipJournal);
                }
                CleanupProgress::ReconcilingCleanup => {
                    status.publish_startup(ExtensionServicePhase::ReconcilingCleanup);
                }
            },
        );
        let outcome = public_startup_outcome(worker, outcome);
        publish_startup_settlement(status, startup_outcome, attempt, outcome, || {})
    }

    fn complete(
        &mut self,
        command: WorkerCommand,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
    ) -> bool {
        match command {
            WorkerCommand::RetryStartup { attempt, deadline } => {
                if !self.attempt_startup(
                    worker,
                    attempt,
                    deadline,
                    status,
                    startup_outcome,
                    cancellation,
                ) {
                    return false;
                }
            }
            #[cfg(test)]
            WorkerCommand::Drive => {}
            #[cfg(test)]
            WorkerCommand::Panic => {
                panic!("extension-service worker panic requested by test")
            }
            #[cfg(test)]
            WorkerCommand::ExitWithoutEvidence => return false,
            #[cfg(test)]
            WorkerCommand::RetainDropProbe(probe) => self.retained_probe = Some(probe),
            #[cfg(test)]
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

    fn drain_attached_native_before_shutdown(
        &mut self,
        status: &SharedStatus,
        deadline: Instant,
    ) -> bool {
        let Some(startup) = self.startup.as_mut() else {
            return true;
        };
        if !startup.native_recovery.has_attached_obligation() {
            return true;
        }

        status.publish_startup(ExtensionServicePhase::ReconcilingCleanup);
        let cancellation = ShutdownDrainCancellation;
        let _ = reconcile_startup(
            &startup.store,
            &mut startup.projection,
            &mut startup.repository,
            Some(&mut startup.native_recovery),
            &cancellation,
            deadline,
            |_| {},
        );
        !startup.native_recovery.has_attached_obligation()
    }
}

struct ShutdownDrainCancellation;

impl CancellationCheck for ShutdownDrainCancellation {
    fn is_cancelled(&self) -> bool {
        false
    }
}

struct WorkerStartupState {
    store: zephium_store::ExtensionNativeOwnershipStoreAuthority,
    repository: ServiceRepository,
    projection: JournalProjection,
    native_recovery: NativeRecoveryState,
}

impl WorkerStartupState {
    fn new(input: ExtensionServiceLaunchInput) -> Self {
        Self {
            store: input.store_authority,
            repository: ServiceRepository::new(input.repository_root),
            projection: JournalProjection::unknown(),
            native_recovery: NativeRecoveryState::new(input.host_factory),
        }
    }
}

fn publish_startup_settlement(
    status: &SharedStatus,
    startup: &SharedStartupOutcome,
    attempt: StartupAttempt,
    outcome: ExtensionServiceStartupOutcome,
    after_evidence: impl FnOnce(),
) -> bool {
    if !startup.settle(attempt, outcome) {
        return false;
    }
    // Startup evidence is authoritative. Make it observable before the
    // informational lifecycle phase can advertise a terminal startup state.
    after_evidence();
    let phase = match outcome {
        ExtensionServiceStartupOutcome::Ready(_) => ExtensionServicePhase::Ready,
        ExtensionServiceStartupOutcome::CleanupRequired(_) => {
            ExtensionServicePhase::CleanupRequired
        }
        ExtensionServiceStartupOutcome::Unavailable(_) => ExtensionServicePhase::StartupUnavailable,
        ExtensionServiceStartupOutcome::FailedClosed(_) => ExtensionServicePhase::StartupFailed,
    };
    status.publish_startup(phase);
    true
}

fn fail_active_startup(worker: ExtensionServiceWorkerIdentity, startup: &SharedStartupOutcome) {
    let CurrentStartupObservation::Await(attempt) = startup.current() else {
        return;
    };
    let _ = startup.settle(
        attempt,
        ExtensionServiceStartupOutcome::FailedClosed(ExtensionServiceStartupFailure::new(
            worker,
            ExtensionServiceStartupFailureReason::InternalProtocolViolation,
        )),
    );
}

fn public_startup_outcome(
    worker: ExtensionServiceWorkerIdentity,
    outcome: CleanupStartupOutcome,
) -> ExtensionServiceStartupOutcome {
    match outcome {
        CleanupStartupOutcome::Ready { journal_revision } => ExtensionServiceStartupOutcome::Ready(
            ExtensionServiceReadyEvidence::new(worker, journal_revision),
        ),
        CleanupStartupOutcome::CleanupRequired {
            journal_revision,
            possible_owner_count,
        } => match ExtensionServiceCleanupEvidence::new(
            worker,
            journal_revision,
            usize::from(possible_owner_count),
        ) {
            Some(evidence) => ExtensionServiceStartupOutcome::CleanupRequired(evidence),
            None => {
                ExtensionServiceStartupOutcome::FailedClosed(ExtensionServiceStartupFailure::new(
                    worker,
                    ExtensionServiceStartupFailureReason::InternalProtocolViolation,
                ))
            }
        },
        CleanupStartupOutcome::Unavailable(reason) => ExtensionServiceStartupOutcome::Unavailable(
            ExtensionServiceStartupUnavailable::new(worker, public_unavailable_reason(reason)),
        ),
        CleanupStartupOutcome::Failed(reason) => ExtensionServiceStartupOutcome::FailedClosed(
            ExtensionServiceStartupFailure::new(worker, public_failure_reason(reason)),
        ),
    }
}

const fn public_unavailable_reason(
    reason: CleanupUnavailable,
) -> ExtensionServiceStartupUnavailableReason {
    match reason {
        CleanupUnavailable::Cancelled => {
            ExtensionServiceStartupUnavailableReason::CancellationRequested
        }
        CleanupUnavailable::DeadlineExpired => {
            ExtensionServiceStartupUnavailableReason::DeadlineReached
        }
        CleanupUnavailable::RepositoryLocked => {
            ExtensionServiceStartupUnavailableReason::RepositoryLocked
        }
        CleanupUnavailable::StoreNotAdmitted => {
            ExtensionServiceStartupUnavailableReason::StoreNotAdmitted
        }
        CleanupUnavailable::RepositoryInUse
        | CleanupUnavailable::RepositoryIo
        | CleanupUnavailable::RepositoryRecoveryPending
        | CleanupUnavailable::StoreObservationPending
        | CleanupUnavailable::NativeRuntimeUnavailable
        | CleanupUnavailable::NativeRuntimeCapacity => {
            ExtensionServiceStartupUnavailableReason::ReconciliationPending
        }
    }
}

const fn public_failure_reason(reason: CleanupFailure) -> ExtensionServiceStartupFailureReason {
    match reason {
        CleanupFailure::UnsupportedPlatform => {
            ExtensionServiceStartupFailureReason::PrivateFilesystemUnavailable
        }
        CleanupFailure::RepositoryRecoveryAmbiguous => {
            ExtensionServiceStartupFailureReason::RepositorySettlementUnknown
        }
        CleanupFailure::UnsafeRepository | CleanupFailure::RepositoryCorrupt => {
            ExtensionServiceStartupFailureReason::RepositoryRecoveryFailed
        }
        CleanupFailure::StoreJournalInvalid => {
            ExtensionServiceStartupFailureReason::OwnershipJournalInvalid
        }
        CleanupFailure::StoreJournalLoadFailed => {
            ExtensionServiceStartupFailureReason::OwnershipJournalLoadFailed
        }
        CleanupFailure::StoreMutationInvariant => {
            ExtensionServiceStartupFailureReason::OwnershipMutationRejected
        }
        CleanupFailure::StoreProjectionMismatch => {
            ExtensionServiceStartupFailureReason::OwnershipMutationProjectionMismatch
        }
        CleanupFailure::InvalidJournalTransition
        | CleanupFailure::PackagePinMismatch
        | CleanupFailure::ConcurrentPackageLease
        | CleanupFailure::NativeBindingInvalid
        | CleanupFailure::NativeHostInvariant
        | CleanupFailure::FrontierLimitExceeded => {
            ExtensionServiceStartupFailureReason::InternalProtocolViolation
        }
    }
}

struct WorkerShutdownSummary {
    accepted_normal: u64,
    completed_normal: u64,
}

fn run_worker(
    state: &mut WorkerState,
    worker: ExtensionServiceWorkerIdentity,
    mailbox: &Mailbox<WorkerCommand>,
    status: &SharedStatus,
    startup_outcome: &SharedStartupOutcome,
    cancellation: &WorkerCancellation,
) -> Option<WorkerShutdownSummary> {
    loop {
        match mailbox.receive() {
            Delivery::Normal(command) => {
                if !state.complete(command, worker, status, startup_outcome, cancellation) {
                    return None;
                }
            }
            Delivery::Shutdown { accepted_normal } => {
                if !cancellation.is_requested() || state.completed_normal != accepted_normal {
                    return None;
                }
                let deadline = cancellation.shutdown_deadline()?;
                if !state.drain_attached_native_before_shutdown(status, deadline) {
                    status.publish(ExtensionServicePhase::Failed);
                    retain_fail_stopped_native_obligation(state);
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

fn retain_fail_stopped_native_obligation(_state: &mut WorkerState) -> ! {
    // There is no truthful passive-`Drop` path for an attached unresolved
    // engine registry proxy. Keep the single bounded worker and its authority
    // parked until process teardown; the owner observes an unclean shutdown,
    // and engine/Store shutdown remain blocked by the retained obligation.
    loop {
        thread::park();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::EXTENSION_SERVICE_NORMAL_CAPACITY;
    use zephium_core::ports::extensions::{
        ExtensionServiceLifecycle,
        ExtensionServiceShutdownOutcome as CoreExtensionServiceShutdownOutcome,
    };
    use zephium_extension_runtime_api::{
        ExtensionRuntimeHostActivationContext, ExtensionRuntimeHostActivationPorts,
        ExtensionRuntimeHostBindError, ExtensionRuntimeHostFactory,
        ExtensionRuntimeHostFactoryPort, ExtensionRuntimeHostOwnershipPort,
        ExtensionRuntimeHostRecoveryContext,
    };

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    use zephium_core::ports::store::StoreShutdownOutcome;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use zephium_core::{
        extensions::{
            ExtensionAuthorityId, ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
            ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision,
            ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionManifestDigest,
            ExtensionNativeOwnershipJournalMutation, ExtensionNativeOwnershipJournalRevision,
            ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPhase,
            ExtensionNativeOwnershipPreparation, ExtensionPackageIdentity, ExtensionPackageKey,
            ExtensionPackagePayloadIdentity, ExtensionPackagePinReleaseBinding,
            ExtensionPackageRevision, ExtensionRuntimeBackendTarget, ExtensionTreeDigest,
        },
        ids::{ExtensionInstallId, ProfileId},
        ports::store::{
            ExtensionNativeOwnershipJournalLoadOutcome,
            ExtensionNativeOwnershipJournalMutationApplied,
            ExtensionNativeOwnershipJournalMutationOutcome, Store,
        },
        profiles::ProfileKind,
        session::{PersistedProfile, SessionState},
    };
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    use zephium_store::SqliteStore;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use zephium_store::{
        ExtensionNativeOwnershipStoreAuthority, ExtensionNativeOwnershipStoreCallOutcome,
    };

    fn assert_send<T: Send>() {}
    fn assert_send_sync<T: Send + Sync>() {}

    struct UnsupportedHostFactoryPort;

    impl ExtensionRuntimeHostFactoryPort for UnsupportedHostFactoryPort {
        fn bind_activation(
            &mut self,
            _context: &ExtensionRuntimeHostActivationContext<'_>,
        ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
            Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
        }

        fn bind_recovery(
            &mut self,
            _context: ExtensionRuntimeHostRecoveryContext,
        ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError>
        {
            Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
        }
    }

    fn unsupported_host_factory() -> ExtensionRuntimeHostFactory {
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(UnsupportedHostFactoryPort))
    }

    #[test]
    fn owner_is_send_and_handle_is_send_sync() {
        assert_send::<ExtensionServiceOwner>();
        assert_send_sync::<ExtensionServiceHandle>();
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
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
    fn lifecycle_port_is_object_safe_and_projects_clean_shutdown() {
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let lifecycle: Box<dyn ExtensionServiceLifecycle> = Box::new(owner);

        assert_eq!(
            lifecycle.shutdown_until(Instant::now() + Duration::from_secs(1)),
            CoreExtensionServiceShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    fn production_input_fixture() -> (
        tempfile::TempDir,
        Arc<SqliteStore>,
        ExtensionServiceLaunchInput,
    ) {
        // macOS exposes `/var` through a filesystem alias. The private
        // namespace correctly rejects that path identity, so place the live
        // fixture below the already admitted checkout rather than weakening
        // production path validation for a test convenience.
        let app_data = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let authority = store.claim_extension_native_ownership_authority().unwrap();
        let repository_root =
            crate::ExtensionRepositoryRoot::from_app_data_directory(app_data.path()).unwrap();
        let input = crate::ExtensionServiceLaunchInput::new(
            authority,
            repository_root,
            unsupported_host_factory(),
        );
        (app_data, store, input)
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn production_startup_fails_closed_before_windows_repository_admission() {
        let (app_data, store, input) = production_input_fixture();
        let repository_path = app_data
            .path()
            .join(crate::EXTENSION_REPOSITORY_DIRECTORY_NAME);
        assert!(!repository_path.exists());

        let owner =
            ExtensionServiceOwner::launch(input, Instant::now() + Duration::from_secs(5)).unwrap();
        let handle = owner.handle();
        let startup = owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5));

        assert!(matches!(
            startup,
            ExtensionServiceStartupWait::Settled(
                ExtensionServiceStartupOutcome::FailedClosed(failure)
            ) if failure.reason()
                == ExtensionServiceStartupFailureReason::PrivateFilesystemUnavailable
        ));
        assert_ne!(handle.status().phase(), ExtensionServicePhase::Ready);
        assert!(
            !repository_path.exists(),
            "unsupported Windows admission must not inspect or create the repository path"
        );
        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn production_launch_fixture(
        startup_deadline: Instant,
    ) -> (tempfile::TempDir, Arc<SqliteStore>, ExtensionServiceOwner) {
        let (app_data, store, input) = production_input_fixture();
        let owner = ExtensionServiceOwner::launch(input, startup_deadline).unwrap();
        (app_data, store, owner)
    }

    #[test]
    fn authoritative_startup_outcome_precedes_terminal_status_publication() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let status = SharedStatus::new(worker);
        status.publish(ExtensionServicePhase::ReconcilingCleanup);
        let startup = SharedStartupOutcome::new(Some(StartupAttempt::INITIAL));
        let outcome = ExtensionServiceStartupOutcome::Ready(ExtensionServiceReadyEvidence::new(
            worker,
            zephium_core::extensions::ExtensionNativeOwnershipJournalRevision::INITIAL,
        ));
        let hook_ran = Cell::new(false);

        assert!(publish_startup_settlement(
            &status,
            &startup,
            StartupAttempt::INITIAL,
            outcome,
            || {
                assert_eq!(
                    startup.current(),
                    CurrentStartupObservation::Settled(outcome)
                );
                assert_eq!(
                    status.snapshot().phase(),
                    ExtensionServicePhase::ReconcilingCleanup
                );
                hook_ran.set(true);
            },
        ));
        assert!(hook_ran.get());
        assert_eq!(status.snapshot().phase(), ExtensionServicePhase::Ready);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn thread_spawn_refusal_returns_the_one_shot_launch_authority() {
        let (_app_data, store, input) = production_input_fixture();
        let error = match ExtensionServiceOwner::launch_with_spawner(
            input,
            Instant::now() + Duration::from_secs(5),
            |_task| Err(io::Error::other("injected thread-spawn refusal")),
        ) {
            Ok(_) => panic!("injected thread spawn unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error.source_error().kind(), io::ErrorKind::Other);

        let owner = ExtensionServiceOwner::launch(
            error.into_launch_input(),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
        ));
        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn production_startup_mints_ready_only_after_real_empty_recovery() {
        let (_app_data, store, owner) =
            production_launch_fixture(Instant::now() + Duration::from_secs(5));
        let worker = owner.worker;

        let startup = owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5));
        let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) =
            startup
        else {
            panic!("empty production repository and journal must settle ready: {startup:?}")
        };
        assert_eq!(evidence.worker(), worker);
        assert_eq!(evidence.journal_revision().get(), 1);
        assert_eq!(
            owner.handle().status().phase(),
            ExtensionServicePhase::Ready
        );

        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn production_startup_reconciles_a_real_release_pending_row_before_ready() {
        fn mutation_applied(
            authority: &ExtensionNativeOwnershipStoreAuthority,
            expected: ExtensionNativeOwnershipJournalRevision,
            mutation: ExtensionNativeOwnershipJournalMutation,
        ) -> ExtensionNativeOwnershipJournalMutationApplied {
            match authority.mutate_until(
                expected,
                mutation,
                Instant::now() + Duration::from_secs(5),
            ) {
                ExtensionNativeOwnershipStoreCallOutcome::Completed(
                    ExtensionNativeOwnershipJournalMutationOutcome::Applied(applied),
                ) => applied,
                outcome => panic!("native-ownership fixture mutation failed: {outcome:?}"),
            }
        }

        let app_data = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let profile = ProfileId::from(1);
        let install = ExtensionInstallId::from(1);
        let store = Arc::new(SqliteStore::open(app_data.path()).unwrap());
        store.save_session(SessionState {
            profiles: vec![PersistedProfile {
                id: profile,
                name: "Extension recovery".into(),
                kind: ProfileKind::Default,
            }],
            ..SessionState::default()
        });
        assert!(store.flush_until(Instant::now() + Duration::from_secs(5)));

        let authority = store.claim_extension_native_ownership_authority().unwrap();
        let runtime_backend = if cfg!(target_os = "macos") {
            ExtensionRuntimeBackendTarget::MacosNative
        } else {
            ExtensionRuntimeBackendTarget::LinuxCompatibility
        };
        let preparation = ExtensionNativeOwnershipPreparation::new(
            ExtensionNativeOwnershipKey::new(
                profile,
                install,
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionPackageIdentity::new(
                ExtensionAuthorityId::from_bytes([1; 32]),
                ExtensionPackageKey::from_bytes([2; 32]),
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
            runtime_backend,
        );
        let begun = mutation_applied(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            ExtensionNativeOwnershipJournalMutation::begin(preparation),
        );
        let preparing = begun
            .entry
            .as_deref()
            .expect("begin must return its exact row");
        let release_pending = mutation_applied(
            &authority,
            begun.journal_revision,
            ExtensionNativeOwnershipJournalMutation::transition(
                preparing.cas(),
                zephium_core::extensions::ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            ),
        );
        let release_entry = release_pending
            .entry
            .as_deref()
            .expect("transition must return its exact row")
            .clone();
        let release_binding = ExtensionPackagePinReleaseBinding::mint(&release_entry).unwrap();

        let repository_root =
            crate::ExtensionRepositoryRoot::from_app_data_directory(app_data.path()).unwrap();
        let owner = ExtensionServiceOwner::launch(
            crate::ExtensionServiceLaunchInput::new(
                authority,
                repository_root,
                unsupported_host_factory(),
            ),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
        let startup = owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5));
        let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) =
            startup
        else {
            panic!("release-pending production recovery did not settle ready: {startup:?}")
        };
        assert!(evidence.journal_revision().get() > release_pending.journal_revision.get());
        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));

        let namespace = zephium_private_fs::LockedPrivateNamespace::open_or_create(
            app_data
                .path()
                .join(crate::EXTENSION_REPOSITORY_DIRECTORY_NAME),
        )
        .unwrap();
        let mut repository =
            zephium_extension_repository::ExtensionRepository::open(namespace).unwrap();
        assert_eq!(
            repository
                .reconcile_bundled_package_pin_release(&release_binding)
                .unwrap(),
            zephium_extension_repository::BundledPackageLeaseReleaseOutcome::AlreadyReleased
        );
        drop(repository);

        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
        drop(store);

        let reopened = Arc::new(SqliteStore::open(app_data.path()).unwrap());
        let reopened_authority = reopened
            .claim_extension_native_ownership_authority()
            .unwrap();
        let journal = match reopened_authority.load_until(Instant::now() + Duration::from_secs(5)) {
            ExtensionNativeOwnershipStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal),
            ) => journal,
            outcome => panic!("settled native-ownership journal did not reload: {outcome:?}"),
        };
        assert!(journal.entries().is_empty());
        assert_eq!(journal.revision(), evidence.journal_revision());
        drop(reopened_authority);
        assert_eq!(
            reopened.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn unavailable_startup_can_retry_without_reclaiming_authority() {
        let (_app_data, store, mut owner) = production_launch_fixture(Instant::now());

        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Unavailable(
                unavailable
            )) if unavailable.reason()
                == ExtensionServiceStartupUnavailableReason::DeadlineReached
        ));
        let retry = owner.retry_startup_until(Instant::now() + Duration::from_secs(5));
        assert!(
            matches!(
                retry,
                ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
            ),
            "retry did not settle ready: {retry:?}"
        );
        assert!(matches!(
            owner.retry_startup_until(Instant::now()),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
        ));

        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn timed_out_retry_is_observed_once_without_duplicate_admission() {
        let (_app_data, store, mut owner) = production_launch_fixture(Instant::now());
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Unavailable(_))
        ));

        let (release_tx, release) = mpsc::sync_channel(0);
        assert!(matches!(
            owner.try_block_for_test(release),
            NormalAdmission::Accepted
        ));
        assert!(matches!(
            owner.retry_startup_until(Instant::now() + Duration::from_millis(20)),
            ExtensionServiceStartupWait::TimedOut(_)
        ));
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_millis(20)),
            ExtensionServiceStartupWait::TimedOut(_)
        ));
        assert!(matches!(
            owner.retry_startup_until(Instant::now() + Duration::from_millis(20)),
            ExtensionServiceStartupWait::TimedOut(_)
        ));

        release_tx.send(()).unwrap();
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Unavailable(
                unavailable
            )) if unavailable.reason()
                == ExtensionServiceStartupUnavailableReason::DeadlineReached
        ));
        assert!(matches!(
            owner.retry_startup_until(Instant::now() + Duration::from_secs(5)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
        ));
        let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown() else {
            panic!("correlated retry worker did not stop cleanly")
        };
        // One Block plus the expired and successful retries. The two
        // observations while the expired retry was in flight admitted no
        // duplicate commands.
        assert_eq!(evidence.accepted_normal_commands(), 3);
        assert_eq!(evidence.completed_normal_commands(), 3);
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[test]
    fn shutdown_drains_a_full_normal_fifo_and_returns_exact_evidence() {
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
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
            let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
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
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
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
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
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
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
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
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
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
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
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
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
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
        let first = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let first_handle = first.handle();
        let first_snapshot = loop {
            let snapshot = first_handle.status();
            if snapshot.phase() == ExtensionServicePhase::Ready {
                break snapshot;
            }
            thread::yield_now();
        };
        let _ = first.shutdown();

        let second = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let second_handle = second.handle();
        assert!(matches!(
            second_handle.wait_for_status_change(first_snapshot, Duration::ZERO),
            ExtensionServiceStatusWait::Changed(snapshot)
                if snapshot.worker() == second_handle.worker_identity()
        ));
        let _ = second.shutdown();
    }
}
