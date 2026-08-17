use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use tokio::sync::{mpsc as async_mpsc, watch};
use zephium_app::CallbackHandle;
use zephium_core::ports::extensions::{
    acquired_runtime_selections_are_canonical, ExtensionAcquiredRuntimeSelection,
    ExtensionDistributionCompletionStatus, ExtensionDistributionFailureReason as StatusReason,
    ExtensionDistributionFailureStage as StatusStage, ExtensionDistributionState,
    ExtensionDistributionStatus,
};
use zephium_extension_distribution::{
    ExtensionDistributionClient, ExtensionDistributionCompletion, ExtensionDistributionCoordinator,
    ExtensionDistributionFailure, ExtensionDistributionFailurePhase,
    ExtensionDistributionFailureReason, ExtensionDistributionServicePort,
};

use crate::ShellExtensionDistributionPort;

const ADMISSION_IDLE: u8 = 0;
const ADMISSION_QUEUED: u8 = 1;
const ADMISSION_RUNNING: u8 = 2;
const ADMISSION_QUARANTINED: u8 = 3;
const ADMISSION_SHUTDOWN: u8 = 4;
const COMMAND_CAPACITY: usize = 1;
static PROCESS_LAUNCH_CLAIMED: AtomicBool = AtomicBool::new(false);

/// Failure to bind one immutable product distribution plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionDistributionPlanError {
    /// The selection was empty, oversized, overallocated, or unordered.
    InvalidSelection,
}

impl std::fmt::Display for ExtensionDistributionPlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("extension distribution runtime selection is invalid")
    }
}

impl std::error::Error for ExtensionDistributionPlanError {}

/// Product-authenticated client plus one immutable reviewed runtime selection.
///
/// Construction performs no I/O. Binding the complete selection here prevents
/// UI, timers, and callback paths from replacing package or backend choices on
/// a later refresh request.
pub struct ExtensionDistributionPlan {
    client: ExtensionDistributionClient,
    selections: BoundRuntimeSelections,
}

impl ExtensionDistributionPlan {
    /// Binds one product client to a complete canonical runtime selection.
    pub fn new(
        client: ExtensionDistributionClient,
        selections: Vec<ExtensionAcquiredRuntimeSelection>,
    ) -> Result<Self, ExtensionDistributionPlanError> {
        Ok(Self {
            client,
            selections: BoundRuntimeSelections::new(selections)?,
        })
    }
}

impl std::fmt::Debug for ExtensionDistributionPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExtensionDistributionPlan")
            .field("client", &self.client)
            .field("selection_count", &self.selections.0.len())
            .finish()
    }
}

struct BoundRuntimeSelections(Vec<ExtensionAcquiredRuntimeSelection>);

impl BoundRuntimeSelections {
    fn new(
        selections: Vec<ExtensionAcquiredRuntimeSelection>,
    ) -> Result<Self, ExtensionDistributionPlanError> {
        acquired_runtime_selections_are_canonical(&selections)
            .then_some(Self(selections))
            .ok_or(ExtensionDistributionPlanError::InvalidSelection)
    }

    fn exact_clone(&self) -> Vec<ExtensionAcquiredRuntimeSelection> {
        self.0.clone()
    }
}

/// Admission result for one explicit product update request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionDistributionRefreshAdmission {
    /// The bounded worker accepted a refresh of its immutable runtime selection.
    Accepted,
    /// A run is queued or active; no later request was retained.
    Busy,
    /// Settlement uncertainty requires a process restart.
    Quarantined,
    /// The worker is exiting or has exited.
    Shutdown,
}

/// Bounded result of consuming the unique worker owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionDistributionShutdownOutcome {
    /// Cancellation was observed and the worker thread was joined.
    Clean,
    /// The worker exited after an internal invariant or panic failed closed.
    FailedClosed,
    /// The deadline elapsed; the detached worker still owns its cancellation path.
    TimedOut,
}

/// Failure to construct the process-unique product distribution worker.
#[derive(Debug)]
pub enum ExtensionDistributionWorkerLaunchError {
    /// A product worker was already launched in this process. Its quarantine
    /// or shutdown state cannot be bypassed by constructing a replacement.
    AlreadyLaunched,
    /// The dedicated worker thread could not be created.
    Thread(std::io::Error),
}

impl std::fmt::Display for ExtensionDistributionWorkerLaunchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyLaunched => {
                formatter.write_str("extension distribution worker was already launched")
            }
            Self::Thread(error) => write!(formatter, "extension distribution worker: {error}"),
        }
    }
}

impl std::error::Error for ExtensionDistributionWorkerLaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AlreadyLaunched => None,
            Self::Thread(error) => Some(error),
        }
    }
}

/// Cloneable, observation-and-admission-only handle for the dormant worker.
#[derive(Clone)]
pub struct ExtensionDistributionHandle {
    shared: Arc<Shared>,
    commands: async_mpsc::Sender<()>,
}

impl ExtensionDistributionHandle {
    /// Requests one complete synchronization without blocking the caller.
    #[must_use = "refresh admission determines request ownership"]
    pub fn request_synchronize(&self) -> ExtensionDistributionRefreshAdmission {
        let _gate = self
            .shared
            .admission_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match self.shared.admission.load(Ordering::Acquire) {
            ADMISSION_IDLE => {}
            ADMISSION_QUEUED | ADMISSION_RUNNING => {
                return ExtensionDistributionRefreshAdmission::Busy;
            }
            ADMISSION_QUARANTINED => {
                return ExtensionDistributionRefreshAdmission::Quarantined;
            }
            _ => return ExtensionDistributionRefreshAdmission::Shutdown,
        }
        self.shared
            .admission
            .store(ADMISSION_QUEUED, Ordering::Release);
        match self.commands.try_send(()) {
            Ok(()) => ExtensionDistributionRefreshAdmission::Accepted,
            Err(async_mpsc::error::TrySendError::Full(_)) => {
                self.shared
                    .admission
                    .store(ADMISSION_IDLE, Ordering::Release);
                ExtensionDistributionRefreshAdmission::Busy
            }
            Err(async_mpsc::error::TrySendError::Closed(_)) => {
                self.shared
                    .admission
                    .store(ADMISSION_SHUTDOWN, Ordering::Release);
                ExtensionDistributionRefreshAdmission::Shutdown
            }
        }
    }

    /// Returns the latest fixed-size process-local worker status.
    pub fn status(&self) -> ExtensionDistributionStatus {
        *self
            .shared
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Unique owner of the product extension-distribution worker thread.
pub struct ExtensionDistributionWorker {
    handle: ExtensionDistributionHandle,
    shutdown: watch::Sender<bool>,
    exited: Receiver<WorkerExit>,
    join: Option<JoinHandle<()>>,
}

impl ExtensionDistributionWorker {
    /// Starts one dedicated current-thread async runtime without issuing I/O.
    ///
    /// The caller supplies product-authenticated client configuration and an
    /// already published Shell callback. No synchronization occurs until an
    /// explicit request is admitted through [`ExtensionDistributionHandle`].
    pub fn launch(
        plan: ExtensionDistributionPlan,
        shell: CallbackHandle,
    ) -> Result<Self, ExtensionDistributionWorkerLaunchError> {
        if !claim_process_launch(&PROCESS_LAUNCH_CLAIMED) {
            return Err(ExtensionDistributionWorkerLaunchError::AlreadyLaunched);
        }
        let port: Arc<dyn ExtensionDistributionServicePort> =
            Arc::new(ShellExtensionDistributionPort::new(shell.clone()));
        let ExtensionDistributionPlan { client, selections } = plan;
        let coordinator = ExtensionDistributionCoordinator::new(client, port);
        let worker = launch_runner(
            coordinator_runner(coordinator, selections),
            Arc::new(ShellStatusPort(shell)),
        );
        if worker.is_err() {
            // No worker thread exists and no request could have been accepted,
            // so this construction-only failure is safe to retry.
            PROCESS_LAUNCH_CLAIMED.store(false, Ordering::Release);
        }
        worker.map_err(ExtensionDistributionWorkerLaunchError::Thread)
    }

    /// Returns a cloneable non-owning update/status handle.
    pub fn handle(&self) -> ExtensionDistributionHandle {
        self.handle.clone()
    }

    /// Cancels any fetch or settlement wait and consumes the unique owner.
    ///
    /// Cancellation never claims service cleanup. The Shell's ordered
    /// extension-service shutdown remains responsible for draining any request
    /// already accepted before this worker observed cancellation.
    pub fn shutdown_until(mut self, deadline: Instant) -> ExtensionDistributionShutdownOutcome {
        self.request_shutdown();
        let remaining = deadline.saturating_duration_since(Instant::now());
        let exit = if remaining.is_zero() {
            self.exited.try_recv().ok()
        } else {
            self.exited.recv_timeout(remaining).ok()
        };
        let Some(exit) = exit else {
            self.join.take();
            return ExtensionDistributionShutdownOutcome::TimedOut;
        };
        let joined = self.join.take().is_some_and(|join| join.join().is_ok());
        if !joined || exit == WorkerExit::FailedClosed {
            ExtensionDistributionShutdownOutcome::FailedClosed
        } else {
            ExtensionDistributionShutdownOutcome::Clean
        }
    }

    fn request_shutdown(&self) {
        let _gate = self
            .handle
            .shared
            .admission_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.handle
            .shared
            .admission
            .store(ADMISSION_SHUTDOWN, Ordering::Release);
        let _ = self.shutdown.send(true);
    }
}

fn claim_process_launch(claimed: &AtomicBool) -> bool {
    claimed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

impl Drop for ExtensionDistributionWorker {
    fn drop(&mut self) {
        self.request_shutdown();
        if self.join.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }
}

impl std::fmt::Debug for ExtensionDistributionWorker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExtensionDistributionWorker")
            .field("status", &self.handle.status())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ExtensionDistributionHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExtensionDistributionHandle")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

struct Shared {
    admission_gate: Mutex<()>,
    admission: AtomicU8,
    status: Mutex<ExtensionDistributionStatus>,
    status_port: Arc<dyn DistributionStatusPort>,
}

impl Shared {
    fn new(status_port: Arc<dyn DistributionStatusPort>) -> Self {
        Self {
            admission_gate: Mutex::new(()),
            admission: AtomicU8::new(ADMISSION_IDLE),
            status: Mutex::new(
                ExtensionDistributionStatus::new(1, ExtensionDistributionState::Idle)
                    .expect("the initial generation is nonzero"),
            ),
            status_port,
        }
    }

    fn publish_current(&self) {
        let status = *self
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.status_port.publish(status);
    }

    fn publish(&self, state: ExtensionDistributionState) -> bool {
        let status = {
            let mut status = self
                .status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(generation) = status.generation().checked_add(1) else {
                self.admission
                    .store(ADMISSION_QUARANTINED, Ordering::Release);
                return false;
            };
            let next = ExtensionDistributionStatus::new(generation, state)
                .expect("a checked successor is nonzero");
            *status = next;
            next
        };
        self.status_port.publish(status);
        true
    }
}

trait DistributionStatusPort: Send + Sync + 'static {
    fn publish(&self, status: ExtensionDistributionStatus);
}

struct ShellStatusPort(CallbackHandle);

impl DistributionStatusPort for ShellStatusPort {
    fn publish(&self, status: ExtensionDistributionStatus) {
        let _ = self.0.publish_extension_distribution_status(status);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerExit {
    Clean,
    FailedClosed,
}

enum RunOutcome {
    Complete(ExtensionDistributionCompletionStatus),
    Failed {
        stage: StatusStage,
        reason: StatusReason,
        quarantined: bool,
    },
}

trait DistributionRunner: Send + Sync + 'static {
    fn synchronize(&self) -> Pin<Box<dyn Future<Output = RunOutcome> + Send + '_>>;
}

struct CoordinatorRunner {
    coordinator: ExtensionDistributionCoordinator,
    selections: BoundRuntimeSelections,
}

fn coordinator_runner(
    coordinator: ExtensionDistributionCoordinator,
    selections: BoundRuntimeSelections,
) -> CoordinatorRunner {
    CoordinatorRunner {
        coordinator,
        selections,
    }
}

impl DistributionRunner for CoordinatorRunner {
    fn synchronize(&self) -> Pin<Box<dyn Future<Output = RunOutcome> + Send + '_>> {
        let selections = self.selections.exact_clone();
        Box::pin(async move {
            match self.coordinator.synchronize(selections).await {
                Ok(completion) => completion_status(completion).map_or(
                    RunOutcome::Failed {
                        stage: StatusStage::Catalog,
                        reason: StatusReason::Accounting,
                        quarantined: true,
                    },
                    RunOutcome::Complete,
                ),
                Err(failure) => RunOutcome::Failed {
                    stage: failure_stage(failure),
                    reason: failure_reason(failure),
                    quarantined: self.coordinator.is_quarantined(),
                },
            }
        })
    }
}

fn launch_runner<R: DistributionRunner>(
    runner: R,
    status_port: Arc<dyn DistributionStatusPort>,
) -> std::io::Result<ExtensionDistributionWorker> {
    let shared = Arc::new(Shared::new(status_port));
    let (commands, command_rx) = async_mpsc::channel(COMMAND_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let (exit_tx, exited) = mpsc::sync_channel(1);
    let thread_shared = Arc::clone(&shared);
    let join = thread::Builder::new()
        .name("zephium-extension-distribution".into())
        .spawn(move || {
            let exit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                thread_shared.publish_current();
                run_worker(runner, command_rx, shutdown_rx, &thread_shared)
            }))
            .unwrap_or_else(|_| {
                thread_shared
                    .admission
                    .store(ADMISSION_QUARANTINED, Ordering::Release);
                // Status projection is an observation boundary, not trusted
                // worker control flow. If that boundary caused the original
                // panic, a second publication may panic too; contain it so
                // the unique owner still receives a terminal exit and can
                // join the thread deterministically.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = thread_shared.publish(ExtensionDistributionState::Quarantined {
                        stage: StatusStage::Catalog,
                        reason: StatusReason::SubmissionPanicked,
                    });
                }));
                WorkerExit::FailedClosed
            });
            let _ = exit_tx.send(exit);
        })?;
    Ok(ExtensionDistributionWorker {
        handle: ExtensionDistributionHandle { shared, commands },
        shutdown,
        exited,
        join: Some(join),
    })
}

fn run_worker<R: DistributionRunner>(
    runner: R,
    commands: async_mpsc::Receiver<()>,
    shutdown: watch::Receiver<bool>,
    shared: &Arc<Shared>,
) -> WorkerExit {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            shared
                .admission
                .store(ADMISSION_QUARANTINED, Ordering::Release);
            let _ = shared.publish(ExtensionDistributionState::Quarantined {
                stage: StatusStage::Catalog,
                reason: StatusReason::Accounting,
            });
            return WorkerExit::FailedClosed;
        }
    };
    runtime.block_on(run_loop(runner, commands, shutdown, shared))
}

async fn run_loop<R: DistributionRunner>(
    runner: R,
    mut commands: async_mpsc::Receiver<()>,
    mut shutdown: watch::Receiver<bool>,
    shared: &Arc<Shared>,
) -> WorkerExit {
    loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    publish_shutdown(shared);
                    return WorkerExit::Clean;
                }
                continue;
            }
            command = commands.recv() => {
                let Some(command) = command else {
                    publish_shutdown(shared);
                    return WorkerExit::Clean;
                };
                command
            }
        };
        if shared
            .admission
            .compare_exchange(
                ADMISSION_QUEUED,
                ADMISSION_RUNNING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            publish_shutdown(shared);
            return WorkerExit::Clean;
        }
        if !shared.publish(ExtensionDistributionState::Synchronizing) {
            return WorkerExit::FailedClosed;
        }
        let run = runner.synchronize();
        tokio::pin!(run);
        let outcome = tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    publish_shutdown(shared);
                    return WorkerExit::Clean;
                }
                continue;
            }
            outcome = &mut run => outcome,
        };
        match outcome {
            RunOutcome::Complete(completion) => {
                shared.admission.store(ADMISSION_IDLE, Ordering::Release);
                if !shared.publish(ExtensionDistributionState::Ready(completion)) {
                    return WorkerExit::FailedClosed;
                }
            }
            RunOutcome::Failed {
                stage,
                reason,
                quarantined,
            } => {
                let state = if quarantined {
                    shared
                        .admission
                        .store(ADMISSION_QUARANTINED, Ordering::Release);
                    ExtensionDistributionState::Quarantined { stage, reason }
                } else {
                    shared.admission.store(ADMISSION_IDLE, Ordering::Release);
                    ExtensionDistributionState::Failed { stage, reason }
                };
                if !shared.publish(state) {
                    return WorkerExit::FailedClosed;
                }
            }
        }
    }
}

fn publish_shutdown(shared: &Shared) {
    shared
        .admission
        .store(ADMISSION_SHUTDOWN, Ordering::Release);
    let _ = shared.publish(ExtensionDistributionState::Shutdown);
}

fn completion_status(
    completion: ExtensionDistributionCompletion,
) -> Option<ExtensionDistributionCompletionStatus> {
    ExtensionDistributionCompletionStatus::new(
        completion.catalog_set(),
        completion.package_count(),
        completion.materialized_packages(),
        completion.reused_packages(),
        completion.exact_retries(),
        completion.newly_activated(),
    )
}

fn failure_stage(failure: ExtensionDistributionFailure) -> StatusStage {
    match failure.phase() {
        ExtensionDistributionFailurePhase::Catalog => StatusStage::Catalog,
        ExtensionDistributionFailurePhase::PackageFetch(index) => StatusStage::PackageFetch(index),
        ExtensionDistributionFailurePhase::PackageProvision(index) => {
            StatusStage::PackageProvision(index)
        }
        ExtensionDistributionFailurePhase::CatalogActivation => StatusStage::CatalogActivation,
    }
}

fn failure_reason(failure: ExtensionDistributionFailure) -> StatusReason {
    match failure.reason() {
        ExtensionDistributionFailureReason::Acquisition(_) => StatusReason::Acquisition,
        ExtensionDistributionFailureReason::CoordinatorBusy
        | ExtensionDistributionFailureReason::CoordinatorQuarantined => StatusReason::Busy,
        ExtensionDistributionFailureReason::ServiceUnavailable => StatusReason::ServiceUnavailable,
        ExtensionDistributionFailureReason::ServiceRejected => StatusReason::ServiceRejected,
        ExtensionDistributionFailureReason::ServiceFailedClosed => {
            StatusReason::ServiceFailedClosed
        }
        ExtensionDistributionFailureReason::SettlementTimedOut => StatusReason::SettlementTimedOut,
        ExtensionDistributionFailureReason::SettlementLost => StatusReason::SettlementLost,
        ExtensionDistributionFailureReason::SubmissionPanicked => StatusReason::SubmissionPanicked,
        ExtensionDistributionFailureReason::OutcomeUnresolved => StatusReason::OutcomeUnresolved,
        ExtensionDistributionFailureReason::ActivationRequestRejected => {
            StatusReason::ActivationRejected
        }
        ExtensionDistributionFailureReason::Accounting => StatusReason::Accounting,
        _ => StatusReason::Accounting,
    }
}

#[cfg(test)]
mod tests;
