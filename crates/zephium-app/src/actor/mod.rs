//! Actor ownership, worker lifetime, and terminal shutdown.

mod mailbox;
#[cfg(test)]
mod tests;

use mailbox::CommandQueueInner;
pub(super) use mailbox::{CommandQueue, TimerWake, TryPushError};

use std::sync::mpsc::{sync_channel, Receiver, TrySendError};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::thread::JoinHandle;

use crate::shell::{
    Shell, ShellPorts, END_TO_END_SHUTDOWN_TIMEOUT, MAINTENANCE_INTERVAL, MAX_OPERATION_ID_BYTES,
};
use crate::store_reads::{run as run_store_reader, StoreReadQueue, StoreReaderStopGuard};
use crate::{
    Command, ContentPolicyStatusQueryOutcome, EmitFn, ExtensionLifecycle, SharedBlocker,
    SharedChrome, SharedEngine, SharedStore, ShellTerminalFailureCallback, ShutdownOutcome,
};
use zephium_core::ids::ProfileId;
use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationCallback, ExtensionAcquiredCatalogActivationRequest,
    ExtensionAcquiredPackageProvisioningCallback, ExtensionAcquiredPackageProvisioningRequest,
    ExtensionDistributionStatus, ExtensionManagementAdmission,
};
use zephium_ipc::BlockerStatusView;

const FAILED_SPAWN_CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);
type WorkerTask = Box<dyn FnOnce() + Send + 'static>;

struct ShellHandoff {
    engine: SharedEngine,
    store: SharedStore,
    blocker: SharedBlocker,
    extension_service: ExtensionLifecycle,
    terminal_failure: ShellTerminalFailureCallback,
    chrome: SharedChrome,
    emit: EmitFn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActorStartupState {
    Pending,
    Waiting,
    Admitted,
    Started,
    Cancelled,
}

/// Exact composition-root admission between guarded Shell construction and the
/// first external port call. Cancellation may overtake admission until the
/// actor consumes it, closing the concurrent terminal-start window.
struct ActorStartupGate {
    state: Mutex<ActorStartupState>,
    changed: Condvar,
}

impl ActorStartupGate {
    fn new() -> Self {
        Self {
            state: Mutex::new(ActorStartupState::Pending),
            changed: Condvar::new(),
        }
    }

    fn admit(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !matches!(
            *state,
            ActorStartupState::Pending | ActorStartupState::Waiting
        ) {
            return false;
        }
        *state = ActorStartupState::Admitted;
        self.changed.notify_one();
        true
    }

    fn cancel(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !matches!(
            *state,
            ActorStartupState::Pending | ActorStartupState::Waiting | ActorStartupState::Admitted
        ) {
            return false;
        }
        *state = ActorStartupState::Cancelled;
        self.changed.notify_one();
        true
    }

    /// Makes an already-admitted transition irrevocable for the compatibility
    /// `spawn` API. The suspended composition path deliberately does not call
    /// this: its terminal coordinator may still overtake admission until the
    /// actor consumes the gate.
    fn commit_admission(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match *state {
            ActorStartupState::Admitted => {
                *state = ActorStartupState::Started;
                self.changed.notify_one();
                true
            }
            ActorStartupState::Started => true,
            ActorStartupState::Pending
            | ActorStartupState::Waiting
            | ActorStartupState::Cancelled => false,
        }
    }

    fn wait_for_admission(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            match *state {
                ActorStartupState::Pending => {
                    *state = ActorStartupState::Waiting;
                    self.changed.notify_all();
                }
                ActorStartupState::Waiting => {
                    state = self
                        .changed
                        .wait(state)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                ActorStartupState::Admitted => {
                    *state = ActorStartupState::Started;
                    return true;
                }
                ActorStartupState::Started => return true,
                ActorStartupState::Cancelled => return false,
            }
        }
    }

    #[cfg(test)]
    fn wait_until_waiting(&self, deadline: std::time::Instant) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while *state == ActorStartupState::Pending {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, timeout) = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
            if timeout.timed_out() && *state == ActorStartupState::Pending {
                return false;
            }
        }
        *state == ActorStartupState::Waiting
    }
}

pub struct Handle {
    pub(super) queue: CommandQueue,
    pub(super) workers: Arc<WorkerThreads>,
    startup: Arc<ActorStartupGate>,
    pub(super) counted: bool,
}

pub struct ShutdownRequest {
    deadline: std::time::Instant,
    receiver: Receiver<ShutdownOutcome>,
    pub(super) workers: Arc<WorkerThreads>,
}

#[must_use = "the bounded status request must be received or deliberately dropped"]
pub struct ContentPolicyStatusRequest {
    receiver: Receiver<ContentPolicyStatusQueryOutcome>,
}

#[must_use = "the bounded focused status request must be received or deliberately dropped"]
pub struct FocusedContentPolicyStatusRequest {
    receiver: Receiver<BlockerStatusView>,
}

#[derive(Default)]
pub(super) struct WorkerThreads {
    state: Mutex<WorkerThreadState>,
}

#[derive(Default)]
struct WorkerThreadState {
    actor: Option<WorkerThread>,
    timer: Option<WorkerThread>,
    store_reader: Option<WorkerThread>,
}

struct WorkerThread {
    join: JoinHandle<()>,
    exited: Receiver<()>,
}

#[derive(Debug)]
pub enum SpawnError {
    Actor(std::io::Error),
    ActorHandoff(std::io::Error),
    Timer(std::io::Error),
    StoreReader(std::io::Error),
}

/// Lossless application-composition failure.
///
/// Worker construction can fail on a native setup thread, where waiting on
/// extension recovery or teardown would deadlock platforms that must service
/// that work from the same event loop. The move-only lifecycle owner is
/// therefore returned untouched to the composition root, together with proof
/// of whether the helper workers that were admitted before the failure were
/// reaped inside the bounded cleanup budget.
#[must_use = "recover and explicitly dispose the returned extension lifecycle owner"]
pub struct SpawnFailure {
    error: SpawnError,
    extension_lifecycle: ExtensionLifecycle,
    worker_cleanup_proven: bool,
}

impl SpawnFailure {
    fn new(
        error: SpawnError,
        extension_lifecycle: ExtensionLifecycle,
        worker_cleanup_proven: bool,
    ) -> Self {
        Self {
            error,
            extension_lifecycle,
            worker_cleanup_proven,
        }
    }

    pub fn error(&self) -> &SpawnError {
        &self.error
    }

    /// Whether every app-owned helper worker admitted before the failure was
    /// observed exited and joined before the rollback deadline.
    pub fn worker_cleanup_proven(&self) -> bool {
        self.worker_cleanup_proven
    }

    /// Recovers the concrete failure and the unique, never-settled lifecycle
    /// owner. Callers decide how to dispose it away from any native UI thread.
    pub fn into_parts(self) -> (SpawnError, ExtensionLifecycle) {
        (self.error, self.extension_lifecycle)
    }
}

impl std::fmt::Debug for SpawnFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SpawnFailure")
            .field("error", &self.error)
            .field("worker_cleanup_proven", &self.worker_cleanup_proven)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Display for SpawnFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(formatter)
    }
}

impl std::error::Error for SpawnFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Actor(error) => write!(formatter, "could not start shell actor: {error}"),
            Self::ActorHandoff(error) => {
                write!(formatter, "could not hand the shell to its actor: {error}")
            }
            Self::Timer(error) => write!(formatter, "could not start shell timer: {error}"),
            Self::StoreReader(error) => {
                write!(formatter, "could not start shell storage reader: {error}")
            }
        }
    }
}

impl std::error::Error for SpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Actor(error)
            | Self::ActorHandoff(error)
            | Self::Timer(error)
            | Self::StoreReader(error) => Some(error),
        }
    }
}

fn spawn_worker(name: &'static str, task: WorkerTask) -> std::io::Result<WorkerThread> {
    let (exited, exit_proof) = sync_channel(1);
    let join = thread::Builder::new().name(name.into()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
        // The task's stack and owned ports have unwound before this signal.
        // Re-raise a panic so the retained JoinHandle still reports failure.
        let _ = exited.send(());
        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    })?;
    Ok(WorkerThread {
        join,
        exited: exit_proof,
    })
}

fn join_worker_until(slot: &mut Option<WorkerThread>, deadline: std::time::Instant) -> bool {
    let Some(worker) = slot.as_mut() else {
        return true;
    };
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    if remaining.is_zero() || worker.exited.recv_timeout(remaining).is_err() {
        return false;
    }
    while !worker.join.is_finished() && std::time::Instant::now() < deadline {
        thread::yield_now();
    }
    if !worker.join.is_finished() {
        return false;
    }
    slot.take().is_some_and(|worker| worker.join.join().is_ok())
}

impl WorkerThreads {
    #[cfg(test)]
    pub(super) fn actor_and_timer_stopped(&self) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.actor.is_none() && state.timer.is_none()
    }

    fn install_actor(&self, worker: WorkerThread) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .actor = Some(worker);
    }

    fn install_timer(&self, worker: WorkerThread) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .timer = Some(worker);
    }

    fn install_store_reader(&self, worker: WorkerThread) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .store_reader = Some(worker);
    }

    fn join_until(&self, deadline: std::time::Instant) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let actor_clean = join_worker_until(&mut state.actor, deadline);
        let timer_clean = join_worker_until(&mut state.timer, deadline);
        let store_reader_clean = join_worker_until(&mut state.store_reader, deadline);
        actor_clean && timer_clean && store_reader_clean
    }
}

fn cleanup_failed_workers(
    queue: &CommandQueue,
    store_reads: &StoreReadQueue,
    workers: &WorkerThreads,
) -> bool {
    let deadline = std::time::Instant::now() + FAILED_SPAWN_CLEANUP_TIMEOUT;
    let queue_clean = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for pending in queue.close_and_drain() {
            finish_unprocessed_command(pending, ShutdownOutcome::Unclean);
        }
    }))
    .is_ok();
    let reads_clean =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store_reads.stop())).is_ok();
    let workers_clean = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        workers.join_until(deadline)
    }))
    .unwrap_or(false);
    let clean = queue_clean && reads_clean && workers_clean;
    if !clean {
        crate::diagnostic!(
            "startup: app helper-worker cleanup was not proven after construction failed"
        );
    }
    clean
}

impl ShutdownRequest {
    pub fn deadline(&self) -> std::time::Instant {
        self.deadline
    }

    /// Waits only until the process-wide shutdown deadline. Shutdown callers
    /// must never regain an unbounded wait merely by choosing this shorthand.
    pub fn recv(&self) -> Result<ShutdownOutcome, std::sync::mpsc::RecvTimeoutError> {
        self.recv_until_deadline()
    }

    pub fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<ShutdownOutcome, std::sync::mpsc::RecvTimeoutError> {
        let now = std::time::Instant::now();
        let deadline = now
            .checked_add(timeout)
            .unwrap_or(self.deadline)
            .min(self.deadline);
        self.receiver
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .map(|outcome| self.finish(outcome, deadline))
    }

    pub fn recv_until_deadline(
        &self,
    ) -> Result<ShutdownOutcome, std::sync::mpsc::RecvTimeoutError> {
        self.receiver
            .recv_timeout(
                self.deadline
                    .saturating_duration_since(std::time::Instant::now()),
            )
            .map(|outcome| self.finish(outcome, self.deadline))
    }

    fn finish(&self, outcome: ShutdownOutcome, deadline: std::time::Instant) -> ShutdownOutcome {
        if outcome == ShutdownOutcome::RetryableFailure {
            return outcome;
        }
        if self.workers.join_until(deadline) {
            outcome
        } else {
            // A clean native/store acknowledgement is insufficient when the
            // owning shell or timer thread did not terminate under the same
            // process-boundary budget.
            ShutdownOutcome::Unclean
        }
    }
}

impl ContentPolicyStatusRequest {
    /// Waits at most `timeout` for the actor-ordered status. Timeout, actor
    /// exit, and failed admission all become the explicit `Unavailable`
    /// result; callers never need an unbounded wait to inspect policy state.
    pub fn recv_timeout(&self, timeout: std::time::Duration) -> ContentPolicyStatusQueryOutcome {
        self.receiver
            .recv_timeout(timeout)
            .unwrap_or(ContentPolicyStatusQueryOutcome::Unavailable)
    }
}

impl FocusedContentPolicyStatusRequest {
    /// Waits at most `timeout` for the actor-ordered focused-profile view.
    /// Queue rejection, timeout, and actor exit return revision-zero
    /// `Unavailable`, which cannot regress a real projection.
    pub fn recv_timeout(&self, timeout: std::time::Duration) -> BlockerStatusView {
        self.receiver
            .recv_timeout(timeout)
            .unwrap_or_else(|_| BlockerStatusView::unavailable())
    }
}

/// Non-owning ingress for callbacks retained by an engine or another shell
/// dependency. It deliberately does not count as a public owner: otherwise
/// Shell -> Engine -> callback Handle -> queue keeps both actor threads alive
/// forever after the application releases its managed Handle.
#[derive(Clone)]
pub struct CallbackHandle {
    pub(super) queue: Weak<CommandQueueInner>,
}

impl CallbackHandle {
    pub fn dispatch(&self, command: Command) -> bool {
        let Some(inner) = self.queue.upgrade() else {
            return false;
        };
        CommandQueue { inner }.try_push(command).is_ok()
    }

    /// Transfers one authenticated acquired package into the Shell actor
    /// without blocking the caller or exposing the Shell-owned lifecycle.
    ///
    /// `Accepted` means the actor owns both the request and callback. Queue
    /// refusal consumes both without invoking the callback, matching the
    /// extension-distribution port contract.
    #[must_use = "admission determines acquired-package callback ownership"]
    pub fn begin_provision_acquired_extension_package(
        &self,
        request: ExtensionAcquiredPackageProvisioningRequest,
        deadline: std::time::Instant,
        done: ExtensionAcquiredPackageProvisioningCallback,
    ) -> ExtensionManagementAdmission {
        let Some(inner) = self.queue.upgrade() else {
            drop((request, done));
            return ExtensionManagementAdmission::Unavailable;
        };
        let command = Command::ProvisionAcquiredExtensionPackage(
            crate::api::AcquiredExtensionPackageSubmission::new(request, deadline, done),
        );
        match (CommandQueue { inner }).try_push(command) {
            Ok(()) => ExtensionManagementAdmission::Accepted,
            Err(TryPushError::Full(_) | TryPushError::Sealed(_) | TryPushError::Closed(_)) => {
                ExtensionManagementAdmission::Unavailable
            }
        }
    }

    /// Transfers one source-free catalog activation into the Shell actor.
    #[must_use = "admission determines acquired-catalog callback ownership"]
    pub fn begin_activate_acquired_extension_catalog(
        &self,
        request: ExtensionAcquiredCatalogActivationRequest,
        deadline: std::time::Instant,
        done: ExtensionAcquiredCatalogActivationCallback,
    ) -> ExtensionManagementAdmission {
        let Some(inner) = self.queue.upgrade() else {
            drop((request, done));
            return ExtensionManagementAdmission::Unavailable;
        };
        let command = Command::ActivateAcquiredExtensionCatalog(
            crate::api::AcquiredExtensionCatalogSubmission::new(request, deadline, done),
        );
        match (CommandQueue { inner }).try_push(command) {
            Ok(()) => ExtensionManagementAdmission::Accepted,
            Err(TryPushError::Full(_) | TryPushError::Sealed(_) | TryPushError::Closed(_)) => {
                ExtensionManagementAdmission::Unavailable
            }
        }
    }

    /// Publishes one monotonic, redacted distribution status replacement.
    /// Queue coalescing makes this non-blocking under rapid state transitions.
    pub fn publish_extension_distribution_status(
        &self,
        status: ExtensionDistributionStatus,
    ) -> bool {
        self.dispatch(Command::ExtensionDistributionStatusChanged(status))
    }
}

impl Clone for Handle {
    fn clone(&self) -> Self {
        let counted = self.queue.retain_handle();
        Self {
            queue: self.queue.clone(),
            workers: self.workers.clone(),
            startup: self.startup.clone(),
            counted,
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // The actor owns an internal queue reference so native callbacks can
        // re-enter it, but that reference must not keep the actor and ticker
        // alive after every public owner has gone away.
        if self.counted && self.queue.release_handle() {
            self.startup.cancel();
        }
    }
}

impl Handle {
    #[cfg(test)]
    pub(super) fn new(queue: CommandQueue) -> Self {
        Self::with_workers(
            queue,
            Arc::new(WorkerThreads::default()),
            Arc::new(ActorStartupGate::new()),
        )
    }

    fn with_workers(
        queue: CommandQueue,
        workers: Arc<WorkerThreads>,
        startup: Arc<ActorStartupGate>,
    ) -> Self {
        let counted = queue.retain_handle();
        Self {
            queue,
            workers,
            startup,
            counted,
        }
    }

    /// Authorizes the guarded actor to enter its first external composition
    /// port. Desktop calls this only after publishing this Handle and clearing
    /// every temporary rollback owner. The transition is exactly once.
    #[must_use = "startup admission must be observed because cancellation is terminal"]
    pub fn admit_startup(&self) -> bool {
        self.startup.admit()
    }

    fn commit_startup_admission(&self) -> bool {
        self.startup.commit_admission()
    }

    #[cfg(test)]
    pub(super) fn wait_until_startup_suspended(&self, deadline: std::time::Instant) -> bool {
        self.startup.wait_until_waiting(deadline)
    }

    /// Returns a non-owning callback ingress. Long-lived dependencies owned
    /// by the shell must use this instead of retaining a strong Handle.
    pub fn callback_handle(&self) -> CallbackHandle {
        CallbackHandle {
            queue: Arc::downgrade(&self.queue.inner),
        }
    }

    /// Attempts to enqueue without ever blocking the caller. Native WebView
    /// callbacks and window messages can execute on the UI thread; applying
    /// backpressure there can deadlock an ordered shutdown that is waiting for
    /// main-thread destruction. Overload is therefore bounded and fail-closed.
    pub fn dispatch(&self, cmd: Command) -> bool {
        self.queue.try_push(cmd).is_ok()
    }

    /// Admits a user mutation with a stable process-local identity. A `true`
    /// return is an exact FIFO ownership transfer; the queue will neither
    /// coalesce nor evict the wrapper after reporting acceptance.
    pub fn dispatch_operation(&self, operation_id: String, command: Command) -> bool {
        if operation_id.is_empty()
            || operation_id.len() > MAX_OPERATION_ID_BYTES
            || !tracked_operation_command(&command)
        {
            return false;
        }
        self.dispatch(Command::Operation {
            operation_id,
            command: Box::new(command),
        })
    }

    /// Requests an actor-ordered snapshot of one profile's content policy.
    ///
    /// This is intentionally an in-process API. The desktop does not expose
    /// it to raw page content, and callers must use the request's bounded
    /// receive method instead of blocking a native UI thread indefinitely.
    pub fn content_policy_status(&self, profile: ProfileId) -> ContentPolicyStatusRequest {
        let (reply, receiver) = sync_channel(1);
        let command = Command::ContentPolicyStatus { profile, reply };
        match self.queue.try_push(command) {
            Ok(()) => {}
            Err(
                TryPushError::Full(command)
                | TryPushError::Sealed(command)
                | TryPushError::Closed(command),
            ) => {
                finish_unprocessed_command(command, ShutdownOutcome::Unclean);
            }
        }
        ContentPolicyStatusRequest { receiver }
    }

    /// Requests the focused profile's revisioned diagnostics without exposing
    /// a caller-selected profile identity.
    pub fn focused_content_policy_status(&self) -> FocusedContentPolicyStatusRequest {
        let (reply, receiver) = sync_channel(1);
        let command = Command::FocusedContentPolicyStatus { reply };
        match self.queue.try_push(command) {
            Ok(()) => {}
            Err(
                TryPushError::Full(command)
                | TryPushError::Sealed(command)
                | TryPushError::Closed(command),
            ) => {
                finish_unprocessed_command(command, ShutdownOutcome::Unclean);
            }
        }
        FocusedContentPolicyStatusRequest { receiver }
    }

    /// Requests an ordered shutdown without waiting for queue capacity on the
    /// caller (normally the UI thread). Only `RetryableFailure` leaves the
    /// actor and native engine available for another attempt.
    pub fn shutdown(&self) -> ShutdownRequest {
        self.shutdown_with_deadline(self.shutdown_deadline())
    }

    /// Creates the single absolute deadline for composition-owned preflight
    /// and the actor's complete ordered shutdown.
    ///
    /// A composition root that owns a weak-callback producer must stop and
    /// join it before calling [`Self::shutdown_with_deadline`] with this exact
    /// value. This prevents late submissions from racing service retirement
    /// without restarting the process-wide shutdown budget.
    pub fn shutdown_deadline(&self) -> std::time::Instant {
        std::time::Instant::now() + END_TO_END_SHUTDOWN_TIMEOUT
    }

    /// Requests ordered shutdown under a caller-started absolute deadline.
    ///
    /// Deadlines beyond Zephium's fixed process budget are clamped; callers
    /// cannot extend teardown by supplying a later instant.
    pub fn shutdown_with_deadline(&self, deadline: std::time::Instant) -> ShutdownRequest {
        let (ack, done) = sync_channel(1);
        let deadline = deadline.min(self.shutdown_deadline());
        // The one process-boundary budget may start before actor admission.
        // Time spent stopping composition-owned callback producers or behind
        // accepted FIFO work must not be hidden by restarting the clock here.
        // A terminal request may overtake desktop admission. Wake the actor in
        // cancelled mode before publishing the barrier so it can drain that
        // exact request without entering an external startup port.
        self.startup.cancel();
        let command = Command::Shutdown { deadline, ack };
        match self.queue.try_push(command) {
            Ok(()) => {}
            // Normal admission reserves one slot for this barrier, so Full is
            // an invariant failure rather than a reason to block a native
            // close callback. A live prior barrier is likewise retryable by
            // the coordinator; a permanently closed actor is terminal.
            Err(TryPushError::Full(command) | TryPushError::Sealed(command)) => {
                finish_unprocessed_command(command, ShutdownOutcome::RetryableFailure);
            }
            Err(TryPushError::Closed(command)) => {
                finish_unprocessed_command(command, ShutdownOutcome::Unclean);
            }
        }
        ShutdownRequest {
            deadline,
            receiver: done,
            workers: self.workers.clone(),
        }
    }
}

fn finish_unprocessed_command(command: Command, outcome: ShutdownOutcome) {
    match command {
        Command::Shutdown { ack, .. } => {
            let _ = ack.send(outcome);
        }
        Command::ContentPolicyStatus { reply, .. } => {
            let _ = reply.send(ContentPolicyStatusQueryOutcome::Unavailable);
        }
        Command::FocusedContentPolicyStatus { reply } => {
            let _ = reply.send(BlockerStatusView::unavailable());
        }
        Command::ProvisionAcquiredExtensionPackage(submission) => {
            submission.settle_unavailable();
        }
        Command::ActivateAcquiredExtensionCatalog(submission) => {
            submission.settle_unavailable();
        }
        _ => {}
    }
}

fn tracked_operation_command(command: &Command) -> bool {
    matches!(
        command,
        Command::Open
            | Command::Activate(_)
            | Command::Close(_)
            | Command::Navigate { .. }
            | Command::Reload(_)
            | Command::GoBack(_)
            | Command::GoForward(_)
            | Command::SplitWith { .. }
            | Command::Unsplit
            | Command::DropTab { .. }
            | Command::DividerRelease { .. }
            | Command::Run(_)
            | Command::InvokeExtensionAction { .. }
            | Command::InstallFocusedExtension { .. }
            | Command::ApproveFocusedExtensionUpdate { .. }
            | Command::EditFocusedExtensionOptionalGrant { .. }
            | Command::SetFocusedProfileExtensionsPaused { .. }
            | Command::SetFocusedSiteExtensionsEnabled { .. }
            | Command::SetFocusedExtensionEnabled { .. }
            | Command::UninstallFocusedExtension { .. }
            | Command::RespondToExtensionRuntimeGrantPrompt { .. }
            | Command::RespondToPagePermissionPrompt { .. }
            | Command::OpenUrl(_)
            | Command::SetAppSetting { .. }
            | Command::DeleteProfile(_)
            | Command::RetryContentPolicy { .. }
            | Command::SetFocusedContentBlockerEnabled(_)
            | Command::RetryFocusedContentPolicy { .. }
            | Command::RefreshContentBlockerSources
    )
}

struct ActorExitGuard(CommandQueue);

impl Drop for ActorExitGuard {
    fn drop(&mut self) {
        // Also runs during unwinding. Without this guard a panic in a port
        // implementation strands the ticker and lets callers enqueue into a
        // queue that will never again be drained. A barrier abandoned by an
        // exiting actor is terminal: no retry can revive this queue.
        for pending in self.0.close_and_drain() {
            finish_unprocessed_command(pending, ShutdownOutcome::Unclean);
        }
    }
}

struct ShellExitGuard(Shell);

impl std::ops::Deref for ShellExitGuard {
    type Target = Shell;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for ShellExitGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for ShellExitGuard {
    fn drop(&mut self) {
        if self.0.is_shutdown() {
            return;
        }
        // A panic or last-handle queue close can bypass Command::Shutdown.
        // Signal the composition root first so its independent hard deadline
        // is armed even if a damaged cleanup port never returns. The correlated
        // shutdown request remains queued until ActorExitGuard terminalizes it.
        self.0
            .report_terminal_failure(crate::ShellTerminalFailure::ActorExitedUnexpectedly);
        // Run every independent terminal barrier before ActorExitGuard drains
        // waiters, under one absolute deadline and without retrying a process
        // whose authoritative actor state has already been lost.
        let deadline = std::time::Instant::now() + END_TO_END_SHUTDOWN_TIMEOUT;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.0.cleanup_after_unexpected_exit_until(deadline)
        })) {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                crate::diagnostic!(
                    "shutdown: complete cleanup was not proven during unexpected shell exit"
                )
            }
        }
    }
}

pub fn spawn(
    engine: SharedEngine,
    store: SharedStore,
    blocker: SharedBlocker,
    extension_service: ExtensionLifecycle,
    terminal_failure: ShellTerminalFailureCallback,
    chrome: SharedChrome,
    emit: EmitFn,
) -> Result<Handle, SpawnFailure> {
    let handle = spawn_suspended(
        engine,
        store,
        blocker,
        extension_service,
        terminal_failure,
        chrome,
        emit,
    )?;
    let admitted = handle.admit_startup();
    debug_assert!(admitted, "new Shell startup gate must be pending");
    let committed = handle.commit_startup_admission();
    debug_assert!(committed, "compatibility startup admission must commit");
    Ok(handle)
}

/// Starts helper workers and transfers the move-only lifecycle into a guarded
/// Shell while keeping every external actor port suspended.
///
/// The composition root must publish the returned Handle, clear every
/// temporary rollback owner, and then call [`Handle::admit_startup`]. Calling
/// [`Handle::shutdown`] first cancels admission and still drives the ordered
/// terminal barrier without entering startup ports.
pub fn spawn_suspended(
    engine: SharedEngine,
    store: SharedStore,
    blocker: SharedBlocker,
    extension_service: ExtensionLifecycle,
    terminal_failure: ShellTerminalFailureCallback,
    chrome: SharedChrome,
    emit: EmitFn,
) -> Result<Handle, SpawnFailure> {
    spawn_suspended_with_worker_spawner(
        ShellHandoff {
            engine,
            store,
            blocker,
            extension_service,
            terminal_failure,
            chrome,
            emit,
        },
        spawn_worker,
    )
}

#[cfg(test)]
fn spawn_with_worker_spawner(
    ports: ShellHandoff,
    worker_spawner: impl FnMut(&'static str, WorkerTask) -> std::io::Result<WorkerThread>,
) -> Result<Handle, SpawnFailure> {
    let handle = spawn_suspended_with_worker_spawner(ports, worker_spawner)?;
    let admitted = handle.admit_startup();
    debug_assert!(admitted, "new Shell startup gate must be pending");
    let committed = handle.commit_startup_admission();
    debug_assert!(committed, "compatibility startup admission must commit");
    Ok(handle)
}

fn spawn_suspended_with_worker_spawner(
    ports: ShellHandoff,
    mut worker_spawner: impl FnMut(&'static str, WorkerTask) -> std::io::Result<WorkerThread>,
) -> Result<Handle, SpawnFailure> {
    let ShellHandoff {
        engine,
        store,
        blocker,
        extension_service,
        terminal_failure,
        chrome,
        emit,
    } = ports;
    // A hostile page can generate title/load/navigation events much faster
    // than projections can be persisted. Backpressure bounds memory instead
    // of letting the actor queue grow without limit.
    let queue = CommandQueue::new();
    let workers = Arc::new(WorkerThreads::default());
    let startup = Arc::new(ActorStartupGate::new());
    let handle = Handle::with_workers(queue.clone(), workers.clone(), startup.clone());
    let store_reads = StoreReadQueue::new();
    let mut extension_service = Some(extension_service);
    let store_reader_task: WorkerTask = Box::new({
        let reader_store = store.clone();
        let reader_queue = store_reads.clone();
        let callback = handle.callback_handle();
        move || run_store_reader(reader_store, reader_queue, callback)
    });
    let store_reader = match worker_spawner("zephium-store-reader", store_reader_task) {
        Ok(worker) => worker,
        Err(error) => {
            let extension_service = extension_service
                .take()
                .expect("pending extension-service owner is unique");
            let cleanup_proven = cleanup_failed_workers(&queue, &store_reads, &workers);
            return Err(SpawnFailure::new(
                SpawnError::StoreReader(error),
                extension_service,
                cleanup_proven,
            ));
        }
    };
    workers.install_store_reader(store_reader);
    let (shell_handoff, shell_receiver) = sync_channel::<ShellHandoff>(1);
    let actor_queue = queue.clone();
    let actor_store_reads = store_reads.clone();
    let actor_startup = startup;
    let actor_task: WorkerTask = Box::new(move || {
        let _exit_guard = ActorExitGuard(actor_queue.clone());
        let shell_store_reads = actor_store_reads.clone();
        let _store_reader_guard = StoreReaderStopGuard::new(actor_store_reads);
        let Ok(handoff) = shell_receiver.recv() else {
            return;
        };
        let ShellHandoff {
            engine,
            store,
            blocker,
            extension_service,
            terminal_failure,
            chrome,
            emit,
        } = handoff;
        let shell = Shell::with_store_reads_deferred_blocker_catalog(
            ShellPorts::new(
                engine,
                store,
                blocker,
                extension_service,
                terminal_failure,
                chrome,
                emit,
            ),
            shell_store_reads,
            #[cfg(test)]
            false,
        );
        let mut shell = ShellExitGuard(shell);
        // No external composition port is entered until the complete Shell is
        // guarded and desktop has published its authoritative Handle and
        // cleared every temporary rollback owner.
        if !actor_startup.wait_for_admission() {
            shell.attach_queue_for_terminal_cleanup(actor_queue.clone());
            while let Some(command) = actor_queue.recv() {
                if matches!(command, Command::Shutdown { .. }) {
                    shell.handle(command);
                    break;
                }
                finish_unprocessed_command(command, ShutdownOutcome::Unclean);
            }
            return;
        }
        // A panic in the initial blocker snapshot now runs only after the
        // composition root can route the early terminal signal to this exact
        // managed Shell.
        shell.initialize_blocker_catalog();
        shell.attach_queue(actor_queue.clone());
        while let Some(command) = actor_queue.recv() {
            shell.handle(command);
            if shell.is_shutdown() || shell.terminal_failure_handoff_panicked() {
                break;
            }
        }
    });
    let actor = match worker_spawner("zephium-shell", actor_task) {
        Ok(actor) => actor,
        Err(error) => {
            drop(shell_handoff);
            let extension_service = extension_service
                .take()
                .expect("pending extension-service owner is unique");
            let cleanup_proven = cleanup_failed_workers(&queue, &store_reads, &workers);
            return Err(SpawnFailure::new(
                SpawnError::Actor(error),
                extension_service,
                cleanup_proven,
            ));
        }
    };
    workers.install_actor(actor);
    let timer_queue = queue.clone();
    let timer_task: WorkerTask = Box::new(move || {
        let mut maintenance_deadline = std::time::Instant::now() + MAINTENANCE_INTERVAL;
        loop {
            match timer_queue.wait_for_timer(maintenance_deadline) {
                TimerWake::Maintenance => {
                    maintenance_deadline = std::time::Instant::now() + MAINTENANCE_INTERVAL;
                    match timer_queue.try_push(Command::Tick) {
                        Ok(()) | Err(TryPushError::Full(_)) | Err(TryPushError::Sealed(_)) => {}
                        Err(TryPushError::Closed(_)) => break,
                    }
                }
                TimerWake::ExtensionStartup => match timer_queue.try_push(Command::Bootstrap) {
                    Ok(()) | Err(TryPushError::Sealed(_)) => {}
                    Err(TryPushError::Full(_)) => timer_queue.schedule_extension_startup(
                        std::time::Instant::now() + std::time::Duration::from_millis(25),
                    ),
                    Err(TryPushError::Closed(_)) => break,
                },
                TimerWake::Persist => match timer_queue.try_push(Command::Persist) {
                    Ok(()) | Err(TryPushError::Sealed(_)) => {}
                    Err(TryPushError::Full(_)) => timer_queue.schedule_persist(
                        std::time::Instant::now() + std::time::Duration::from_millis(25),
                    ),
                    Err(TryPushError::Closed(_)) => break,
                },
                TimerWake::Favicon { id, attempt } => {
                    match timer_queue.try_push(Command::FaviconPoll { id, attempt }) {
                        Ok(()) | Err(TryPushError::Sealed(_)) => {}
                        Err(TryPushError::Full(_)) => timer_queue.schedule_favicon(
                            id,
                            attempt,
                            std::time::Instant::now() + std::time::Duration::from_millis(25),
                        ),
                        Err(TryPushError::Closed(_)) => break,
                    }
                }
                TimerWake::Presentation {
                    id,
                    navigation,
                    hard_deadline,
                } => {
                    match timer_queue.try_push(Command::PresentationFallback {
                        id,
                        navigation,
                        hard_deadline,
                    }) {
                        Ok(()) | Err(TryPushError::Sealed(_)) => {}
                        Err(TryPushError::Full(_)) => timer_queue.retry_presentation(
                            id,
                            navigation,
                            std::time::Instant::now() + std::time::Duration::from_millis(25),
                            hard_deadline,
                        ),
                        Err(TryPushError::Closed(_)) => break,
                    }
                }
                TimerWake::DiscardProbe { id, probe } => {
                    match timer_queue.try_push(Command::DiscardProbeTimeout { id, probe }) {
                        Ok(()) | Err(TryPushError::Sealed(_)) => {}
                        Err(TryPushError::Full(_)) => timer_queue.schedule_discard_probe(
                            id,
                            probe,
                            std::time::Instant::now() + std::time::Duration::from_millis(25),
                        ),
                        Err(TryPushError::Closed(_)) => break,
                    }
                }
                TimerWake::ProfileDeletion {
                    profile,
                    generation,
                } => match timer_queue.try_push(Command::ProfileDeletionRetry {
                    profile,
                    generation,
                }) {
                    Ok(()) | Err(TryPushError::Sealed(_)) => {}
                    Err(TryPushError::Full(_)) => timer_queue.schedule_profile_deletion(
                        profile,
                        generation,
                        std::time::Instant::now() + std::time::Duration::from_millis(25),
                    ),
                    Err(TryPushError::Closed(_)) => break,
                },
                TimerWake::BlockerPreference { profile, token } => {
                    match timer_queue.try_push(Command::BlockerPreferenceRetry { profile, token }) {
                        Ok(()) | Err(TryPushError::Sealed(_)) => {}
                        Err(TryPushError::Full(_)) => timer_queue
                            .schedule_blocker_preference_reconciliation(
                                profile,
                                token,
                                std::time::Instant::now() + std::time::Duration::from_millis(25),
                            ),
                        Err(TryPushError::Closed(_)) => break,
                    }
                }
                TimerWake::BlockerCatalog { operation, attempt } => {
                    match timer_queue.try_push(Command::BlockerCatalogPoll { operation, attempt }) {
                        Ok(()) | Err(TryPushError::Sealed(_)) => {}
                        Err(TryPushError::Full(_)) => timer_queue.schedule_blocker_catalog_poll(
                            operation,
                            attempt,
                            std::time::Instant::now() + std::time::Duration::from_millis(25),
                        ),
                        Err(TryPushError::Closed(_)) => break,
                    }
                }
                TimerWake::PagePermission {
                    profile,
                    item,
                    request,
                } => match timer_queue.try_push(Command::PagePermissionTimeout {
                    profile,
                    item,
                    request,
                }) {
                    Ok(()) | Err(TryPushError::Sealed(_)) => {}
                    Err(TryPushError::Full(_)) => timer_queue.schedule_page_permission(
                        profile,
                        item,
                        request,
                        std::time::Instant::now() + std::time::Duration::from_millis(25),
                    ),
                    Err(TryPushError::Closed(_)) => break,
                },
                TimerWake::Stopped => break,
            }
        }
    });
    let timer = match worker_spawner("zephium-timer", timer_task) {
        Ok(timer) => timer,
        Err(error) => {
            // Wake the actor waiter before joining it. The unique service
            // owner is still local and has not crossed the handoff boundary.
            drop(shell_handoff);
            let extension_service = extension_service
                .take()
                .expect("pending extension-service owner is unique");
            let cleanup_proven = cleanup_failed_workers(&queue, &store_reads, &workers);
            return Err(SpawnFailure::new(
                SpawnError::Timer(error),
                extension_service,
                cleanup_proven,
            ));
        }
    };
    workers.install_timer(timer);

    // Every fallible worker construction completed before the unique owner
    // crosses into the actor. Shell itself is constructed only after receipt,
    // so a disconnected handoff returns the owner as a first-class field
    // without calling any lifecycle method on this setup thread.
    let handoff = ShellHandoff {
        engine,
        store,
        blocker,
        extension_service: extension_service
            .take()
            .expect("pending extension-service owner is unique"),
        terminal_failure,
        chrome,
        emit,
    };
    match shell_handoff.try_send(handoff) {
        Ok(()) => {}
        Err(TrySendError::Full(handoff) | TrySendError::Disconnected(handoff)) => {
            let ShellHandoff {
                engine,
                store,
                blocker,
                extension_service,
                terminal_failure,
                chrome,
                emit,
            } = handoff;
            let cleanup_proven = cleanup_failed_workers(&queue, &store_reads, &workers);
            // These cloneable composition ports are not returned, but keep a
            // faulty destructor from preventing recovery of the unique owner.
            for clean in [
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(engine))).is_ok(),
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(store))).is_ok(),
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(blocker))).is_ok(),
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(terminal_failure)))
                    .is_ok(),
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(chrome))).is_ok(),
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(emit))).is_ok(),
            ] {
                if !clean {
                    crate::diagnostic!(
                        "startup: a composition port panicked while its handoff was rolled back"
                    );
                }
            }
            return Err(SpawnFailure::new(
                SpawnError::ActorHandoff(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "shell actor exited before accepting its unique owner",
                )),
                extension_service,
                cleanup_proven,
            ));
        }
    }
    Ok(handle)
}
