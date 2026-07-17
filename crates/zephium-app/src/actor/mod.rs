//! Actor ownership, worker lifetime, and terminal shutdown.

mod mailbox;
#[cfg(test)]
mod tests;

use mailbox::CommandQueueInner;
pub(super) use mailbox::{CommandQueue, TimerWake, TryPushError};

use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::thread::JoinHandle;

use crate::shell::{
    Shell, END_TO_END_SHUTDOWN_TIMEOUT, MAINTENANCE_INTERVAL, MAX_OPERATION_ID_BYTES,
};
use crate::store_reads::{run as run_store_reader, StoreReadQueue, StoreReaderStopGuard};
use crate::{Command, EmitFn, SharedChrome, SharedEngine, SharedStore, ShutdownOutcome};

pub struct Handle {
    pub(super) queue: CommandQueue,
    pub(super) workers: Arc<WorkerThreads>,
    pub(super) counted: bool,
}

pub struct ShutdownRequest {
    deadline: std::time::Instant,
    receiver: Receiver<ShutdownOutcome>,
    pub(super) workers: Arc<WorkerThreads>,
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
    Timer(std::io::Error),
    StoreReader(std::io::Error),
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Actor(error) => write!(formatter, "could not start shell actor: {error}"),
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
            Self::Actor(error) | Self::Timer(error) | Self::StoreReader(error) => Some(error),
        }
    }
}

fn spawn_worker(
    name: &'static str,
    task: impl FnOnce() + Send + 'static,
) -> std::io::Result<WorkerThread> {
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
}

impl Clone for Handle {
    fn clone(&self) -> Self {
        let counted = self.queue.retain_handle();
        Self {
            queue: self.queue.clone(),
            workers: self.workers.clone(),
            counted,
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // The actor owns an internal queue reference so native callbacks can
        // re-enter it, but that reference must not keep the actor and ticker
        // alive after every public owner has gone away.
        if self.counted {
            self.queue.release_handle();
        }
    }
}

impl Handle {
    #[cfg(test)]
    pub(super) fn new(queue: CommandQueue) -> Self {
        Self::with_workers(queue, Arc::new(WorkerThreads::default()))
    }

    fn with_workers(queue: CommandQueue, workers: Arc<WorkerThreads>) -> Self {
        let counted = queue.retain_handle();
        Self {
            queue,
            workers,
            counted,
        }
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

    /// Requests an ordered shutdown without waiting for queue capacity on the
    /// caller (normally the UI thread). Only `RetryableFailure` leaves the
    /// actor and native engine available for another attempt.
    pub fn shutdown(&self) -> ShutdownRequest {
        let (ack, done) = sync_channel(1);
        // The one process-boundary budget starts at caller admission. Time
        // spent behind already-accepted FIFO work is real shutdown latency
        // and must not be hidden by restarting the clock in the actor.
        let deadline = std::time::Instant::now() + END_TO_END_SHUTDOWN_TIMEOUT;
        let command = Command::Shutdown { deadline, ack };
        match self.queue.try_push(command) {
            Ok(()) => {}
            // Normal admission reserves one slot for this barrier, so Full is
            // an invariant failure rather than a reason to block a native
            // close callback. A live prior barrier is likewise retryable by
            // the coordinator; a permanently closed actor is terminal.
            Err(TryPushError::Full(command) | TryPushError::Sealed(command)) => {
                finish_shutdown(command, ShutdownOutcome::RetryableFailure);
            }
            Err(TryPushError::Closed(command)) => {
                finish_shutdown(command, ShutdownOutcome::Unclean);
            }
        }
        ShutdownRequest {
            deadline,
            receiver: done,
            workers: self.workers.clone(),
        }
    }
}

fn finish_shutdown(command: Command, outcome: ShutdownOutcome) {
    if let Command::Shutdown { ack, .. } = command {
        let _ = ack.send(outcome);
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
            | Command::OpenUrl(_)
            | Command::SetAppSetting { .. }
            | Command::DeleteProfile(_)
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
            finish_shutdown(pending, ShutdownOutcome::Unclean);
        }
    }
}

pub fn spawn(
    engine: SharedEngine,
    store: SharedStore,
    chrome: SharedChrome,
    emit: EmitFn,
) -> Result<Handle, SpawnError> {
    // A hostile page can generate title/load/navigation events much faster
    // than projections can be persisted. Backpressure bounds memory instead
    // of letting the actor queue grow without limit.
    let queue = CommandQueue::new();
    let workers = Arc::new(WorkerThreads::default());
    let handle = Handle::with_workers(queue.clone(), workers.clone());
    let store_reads = StoreReadQueue::new();
    let store_reader = spawn_worker("zephium-store-reader", {
        let reader_store = store.clone();
        let reader_queue = store_reads.clone();
        let callback = handle.callback_handle();
        move || run_store_reader(reader_store, reader_queue, callback)
    })
    .map_err(SpawnError::StoreReader)?;
    workers.install_store_reader(store_reader);
    let actor_queue = queue.clone();
    let actor_store_reads = store_reads.clone();
    let mut shell = Shell::with_store_reads(engine, store, chrome, emit, store_reads.clone());
    shell.attach_queue(queue.clone());
    let actor = match spawn_worker("zephium-shell", move || {
        let _exit_guard = ActorExitGuard(actor_queue.clone());
        let _store_reader_guard = StoreReaderStopGuard::new(actor_store_reads);
        while let Some(command) = actor_queue.recv() {
            shell.handle(command);
            if shell.is_shutdown() {
                break;
            }
        }
    }) {
        Ok(actor) => actor,
        Err(error) => {
            store_reads.stop();
            let _ =
                workers.join_until(std::time::Instant::now() + std::time::Duration::from_secs(1));
            return Err(SpawnError::Actor(error));
        }
    };
    workers.install_actor(actor);
    let timer_queue = queue.clone();
    let timer = match spawn_worker("zephium-timer", move || {
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
                TimerWake::Stopped => break,
            }
        }
    }) {
        Ok(timer) => timer,
        Err(error) => {
            // No Handle escapes this failed construction. Seal the queue and
            // make a bounded effort to reap the actor already started above.
            for pending in queue.close_and_drain() {
                finish_shutdown(pending, ShutdownOutcome::Unclean);
            }
            store_reads.stop();
            let _ =
                workers.join_until(std::time::Instant::now() + std::time::Duration::from_secs(1));
            return Err(SpawnError::Timer(error));
        }
    };
    workers.install_timer(timer);
    Ok(handle)
}
