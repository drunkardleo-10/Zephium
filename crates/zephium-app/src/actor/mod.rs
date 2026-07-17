//! Actor ownership, bounded mailbox, timers, and terminal shutdown.

use std::collections::VecDeque;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::thread::JoinHandle;

use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::{DiscardProbeId, EngineEvent, NavigationPresentationId};

use crate::store_reads::{run as run_store_reader, StoreReadQueue, StoreReaderStopGuard};
use crate::{
    Command, EmitFn, SharedChrome, SharedEngine, SharedStore, Shell, ShutdownOutcome,
    StoreReadResult, END_TO_END_SHUTDOWN_TIMEOUT, MAINTENANCE_INTERVAL, MAX_OPERATION_ID_BYTES,
};

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

pub(super) fn finish_shutdown(command: Command, outcome: ShutdownOutcome) {
    if let Command::Shutdown { ack, .. } = command {
        let _ = ack.send(outcome);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CoalescedKey {
    Title(ItemId),
    Url(ItemId),
    Loading(ItemId),
    Favicon(ItemId),
    FaviconPoll(ItemId),
    Presentation(ItemId),
    ChromePresentation(ItemId),
    PresentationFallback(ItemId),
    DiscardProbeTimeout(ItemId),
    ProfileDeletionReady(ProfileId),
    ProfileDeletionRetry(ProfileId),
    StoreHistory,
    StoreFavicon(ItemId),
    StoreFaviconBatch,
    Navigation(ItemId),
    Zoom(ItemId),
    NativeAction(ItemId),
    Split(zephium_core::ids::WindowId),
    WindowSize,
    WindowVisible,
    SidebarWidth,
    DragOver,
    DividerDrag,
    Search,
    Persist,
    Tick,
}

impl CoalescedKey {
    fn of(event: &EngineEvent) -> Option<Self> {
        Some(match event {
            EngineEvent::TitleChanged { id, .. } => Self::Title(*id),
            EngineEvent::UrlChanged { id, .. } => Self::Url(*id),
            EngineEvent::LoadingChanged { id, .. } => Self::Loading(*id),
            EngineEvent::FaviconPixels { id, .. } => Self::Favicon(*id),
            EngineEvent::PresentationPending { id, .. }
            | EngineEvent::PresentationReady { id, .. } => Self::Presentation(*id),
            EngineEvent::NavState { id, .. } => Self::Navigation(*id),
            EngineEvent::ZoomSettled { id, .. } => Self::Zoom(*id),
            EngineEvent::NativeActionFailed { id, .. } => Self::NativeAction(*id),
            EngineEvent::SplitChanged { window, .. } => Self::Split(*window),
            _ => return None,
        })
    }
}

// The ordinary UI band stays small, while the lifecycle band can retain one
// latest URL, view-state and navigation result per maximum session item plus
// profile/split/runtime-update facts and the final shutdown barrier. A shared WebKit process
// may terminate all 1,024 live views in one native callback burst.
pub(super) const NORMAL_COMMAND_CAPACITY: usize = 960;
// Each tracked tab can have one latest URL, presentation, navigation failure,
// terminal view-state, zoom settlement, and native-action failure fact. Each
// profile can independently have one process-
// exit fact and one durable-deletion callback wakeup. The current single-
// window shell can have one native split fact, and the process can have one
// sticky runtime-update fact. Reserve all of those independently of the
// already-accepted user FIFO.
const MAX_CRITICAL_LIFECYCLE_FACTS: usize = zephium_core::session::MAX_SESSION_ITEMS * 7
    + zephium_core::session::MAX_SESSION_PROFILES * 2
    + 2;
pub(super) const COMMAND_QUEUE_CAPACITY: usize =
    NORMAL_COMMAND_CAPACITY + MAX_CRITICAL_LIFECYCLE_FACTS + 1;
pub(super) const LIFECYCLE_COMMAND_CAPACITY: usize = COMMAND_QUEUE_CAPACITY - 1;
// During a failed store barrier, keep at most the bounded set of lifecycle
// facts the native engine can produce for the maximum item/profile counts.
const POST_BARRIER_CRITICAL_CAPACITY: usize = MAX_CRITICAL_LIFECYCLE_FACTS;

pub(super) fn tracked_operation_command(command: &Command) -> bool {
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

#[derive(Clone)]
pub(super) struct CommandQueue {
    pub(super) inner: Arc<CommandQueueInner>,
}

pub(super) struct CommandQueueInner {
    pub(super) state: Mutex<CommandQueueState>,
    ready: Condvar,
    pub(super) timer_state: Mutex<TimerState>,
    timer_ready: Condvar,
}

#[derive(Default)]
pub(super) struct TimerState {
    stopped: bool,
    pub(super) persist_deadline: Option<std::time::Instant>,
    favicon_deadlines: std::collections::HashMap<ItemId, (std::time::Instant, u8)>,
    pub(super) presentation_deadlines: std::collections::HashMap<ItemId, PresentationDeadline>,
    discard_deadlines: std::collections::HashMap<ItemId, (std::time::Instant, DiscardProbeId)>,
    profile_deletion_deadlines: std::collections::HashMap<ProfileId, (std::time::Instant, u64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PresentationDeadline {
    pub(super) wake: std::time::Instant,
    pub(super) hard: std::time::Instant,
    pub(super) navigation: NavigationPresentationId,
}

pub(super) enum TimerWake {
    Maintenance,
    Persist,
    Favicon {
        id: ItemId,
        attempt: u8,
    },
    Presentation {
        id: ItemId,
        navigation: NavigationPresentationId,
        hard_deadline: std::time::Instant,
    },
    DiscardProbe {
        id: ItemId,
        probe: DiscardProbeId,
    },
    ProfileDeletion {
        profile: ProfileId,
        generation: u64,
    },
    Stopped,
}

#[derive(Default)]
pub(super) struct CommandQueueState {
    commands: VecDeque<Command>,
    // Critical native callbacks that race an in-progress store barrier. They
    // are not reported as accepted behind Shutdown, but must remain available
    // if that barrier fails and the live browser resumes.
    post_barrier_critical: VecDeque<Command>,
    closed: bool,
    shutdown_enqueued: bool,
    pub(super) handles: usize,
}

pub(super) enum TryPushError {
    Full(Command),
    Sealed(Command),
    Closed(Command),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RecoveryKey {
    RuntimeRestart,
    Url(ItemId),
    Presentation(ItemId),
    ChromePresentation(ItemId),
    ViewState(ItemId),
    NavigationFailure(ItemId),
    Zoom(ItemId),
    NativeAction(ItemId),
    Profile(ProfileId),
    ProfileDeletion(ProfileId),
    Split(zephium_core::ids::WindowId),
}

fn recovery_key(command: &Command) -> Option<RecoveryKey> {
    match command {
        Command::Engine(EngineEvent::RuntimeRestartRequired) => Some(RecoveryKey::RuntimeRestart),
        Command::Engine(EngineEvent::UrlChanged { id, .. }) => Some(RecoveryKey::Url(*id)),
        Command::Engine(
            EngineEvent::PresentationPending { id, .. } | EngineEvent::PresentationReady { id, .. },
        ) => Some(RecoveryKey::Presentation(*id)),
        Command::ChromePresentationApplied { id, .. } => Some(RecoveryKey::ChromePresentation(*id)),
        Command::Engine(EngineEvent::NavigationFailed { id, .. }) => {
            Some(RecoveryKey::NavigationFailure(*id))
        }
        Command::Engine(EngineEvent::ZoomSettled { id, .. }) => Some(RecoveryKey::Zoom(*id)),
        Command::Engine(EngineEvent::NativeActionFailed { id, .. }) => {
            Some(RecoveryKey::NativeAction(*id))
        }
        Command::Engine(
            EngineEvent::ViewCreationFailed { id }
            | EngineEvent::Crashed { id }
            | EngineEvent::ViewDiscarded { id, .. },
        ) => Some(RecoveryKey::ViewState(*id)),
        Command::Engine(EngineEvent::ProfileProcessExited { profile, .. }) => {
            Some(RecoveryKey::Profile(*profile))
        }
        Command::ProfileDeletionReady(profile) => Some(RecoveryKey::ProfileDeletion(*profile)),
        Command::Engine(EngineEvent::SplitChanged { window, .. }) => {
            Some(RecoveryKey::Split(*window))
        }
        _ => None,
    }
}

fn retain_post_barrier(commands: &mut VecDeque<Command>, command: Command) {
    if let Some(key) = recovery_key(&command) {
        if let Some(index) = commands
            .iter()
            .rposition(|queued| recovery_key(queued) == Some(key))
        {
            commands.remove(index);
        }
    }
    if commands.len() < POST_BARRIER_CRITICAL_CAPACITY {
        commands.push_back(command);
    }
}

impl CommandQueue {
    pub(super) fn new() -> Self {
        Self {
            inner: Arc::new(CommandQueueInner {
                state: Mutex::new(CommandQueueState::default()),
                ready: Condvar::new(),
                timer_state: Mutex::new(TimerState::default()),
                timer_ready: Condvar::new(),
            }),
        }
    }

    pub(super) fn retain_handle(&self) -> bool {
        {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.closed {
            } else if let Some(handles) = state.handles.checked_add(1) {
                state.handles = handles;
                return true;
            } else {
                // `Clone` cannot report failure. Seal the actor rather than
                // aborting or returning an uncounted live ingress that could
                // outlast the final owner. The returned Handle records that
                // it was not counted, so its Drop cannot underflow state.
                state.closed = true;
                state.post_barrier_critical.clear();
                self.inner.ready.notify_all();
            }
        }
        self.stop_ticker();
        false
    }

    pub(super) fn release_handle(&self) {
        let should_stop = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.handles == 0 {
                // A bookkeeping invariant failure must terminate admission,
                // not abort a release build. There is no safe owner count to
                // reconstruct, so fail closed and wake both actor threads.
                state.closed = true;
                state.post_barrier_critical.clear();
                self.inner.ready.notify_all();
                true
            } else {
                state.handles -= 1;
                if state.handles != 0 || state.closed {
                    false
                } else {
                    // Reject new internal/native work, but let the actor drain
                    // commands already accepted before its final public owner
                    // went away. In particular, dropping the handle after
                    // requesting Shutdown must not cancel that barrier.
                    state.closed = true;
                    state.post_barrier_critical.clear();
                    self.inner.ready.notify_all();
                    true
                }
            }
        };
        if should_stop {
            self.stop_ticker();
        }
    }

    pub(super) fn try_push(&self, command: Command) -> Result<(), TryPushError> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let shutdown = matches!(&command, Command::Shutdown { .. });
        if state.closed {
            return Err(TryPushError::Closed(command));
        }
        if state.shutdown_enqueued {
            if command_is_critical(&command) {
                // Retain a clone only for failed-barrier recovery. Returning
                // Sealed remains truthful to the native callback: this work
                // was not admitted after the ordered shutdown point.
                retain_post_barrier(&mut state.post_barrier_critical, command.clone());
            }
            return Err(TryPushError::Sealed(command));
        }
        let capacity = if shutdown {
            COMMAND_QUEUE_CAPACITY
        } else if command_is_critical(&command) {
            LIFECYCLE_COMMAND_CAPACITY
        } else {
            NORMAL_COMMAND_CAPACITY
        };
        let critical = command_is_critical(&command);
        enqueue(&mut state.commands, command, capacity, critical).map_err(TryPushError::Full)?;
        if shutdown {
            // Seal admission at the same ordered point as the barrier. No
            // caller can receive `true` for work that would land behind it.
            state.shutdown_enqueued = true;
        }
        self.inner.ready.notify_one();
        Ok(())
    }

    pub(super) fn recv(&self) -> Option<Command> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if let Some(command) = state.commands.pop_front() {
                return Some(command);
            }
            if state.closed {
                return None;
            }
            state = self
                .inner
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    #[cfg(test)]
    pub(super) fn try_recv(&self) -> Option<Command> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.commands.pop_front()
    }

    pub(super) fn reopen_after_failed_shutdown(&self) -> Vec<Command> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.closed {
            state.shutdown_enqueued = false;
            return state.post_barrier_critical.drain(..).collect();
        }
        Vec::new()
    }

    pub(super) fn close_and_drain(&self) -> Vec<Command> {
        let pending = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.closed = true;
            state.post_barrier_critical.clear();
            let pending = state.commands.drain(..).collect();
            self.inner.ready.notify_all();
            pending
        };
        self.stop_ticker();
        pending
    }

    pub(super) fn stop_ticker(&self) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        timer.stopped = true;
        timer.persist_deadline = None;
        timer.favicon_deadlines.clear();
        timer.presentation_deadlines.clear();
        timer.discard_deadlines.clear();
        timer.profile_deletion_deadlines.clear();
        self.inner.timer_ready.notify_all();
    }

    pub(super) fn schedule_persist(&self, deadline: std::time::Instant) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if timer.stopped {
            return;
        }
        timer.persist_deadline = Some(deadline);
        self.inner.timer_ready.notify_one();
    }

    pub(super) fn cancel_persist(&self) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        timer.persist_deadline = None;
    }

    pub(super) fn schedule_favicon(&self, id: ItemId, attempt: u8, deadline: std::time::Instant) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if timer.stopped {
            return;
        }
        timer.favicon_deadlines.insert(id, (deadline, attempt));
        self.inner.timer_ready.notify_one();
    }

    pub(super) fn cancel_favicon(&self, id: ItemId) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        timer.favicon_deadlines.remove(&id);
    }

    pub(super) fn schedule_presentation(
        &self,
        id: ItemId,
        navigation: NavigationPresentationId,
        wake: std::time::Instant,
        hard: std::time::Instant,
    ) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if timer.stopped {
            return;
        }
        // One native generation has at most one initially hidden epoch. Keep
        // the earliest deadline for a duplicate token so same-document URL
        // churn cannot postpone presentation indefinitely.
        match timer.presentation_deadlines.entry(id) {
            std::collections::hash_map::Entry::Occupied(mut entry)
                if entry.get().navigation == navigation =>
            {
                let current = entry.get_mut();
                current.wake = current.wake.min(wake);
                current.hard = current.hard.min(hard);
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                // A hidden page can commit a chain of cross-document
                // navigations before first presentation. Advance the exact
                // token, but retain both original absolute bounds so hostile
                // navigation churn cannot restart either grace period.
                let current = *entry.get();
                entry.insert(PresentationDeadline {
                    wake: current.wake.min(wake),
                    hard: current.hard.min(hard),
                    navigation,
                });
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(PresentationDeadline {
                    wake,
                    hard,
                    navigation,
                });
            }
        }
        self.inner.timer_ready.notify_one();
    }

    pub(super) fn cancel_presentation(&self, id: ItemId) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        timer.presentation_deadlines.remove(&id);
    }

    /// Re-arms a fallback that could not enter the actor queue without
    /// allowing an escaped wake for an older navigation to replace a newer
    /// native presentation obligation for the same logical tab.
    pub(super) fn retry_presentation(
        &self,
        id: ItemId,
        navigation: NavigationPresentationId,
        wake: std::time::Instant,
        hard: std::time::Instant,
    ) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if timer.stopped {
            return;
        }
        match timer.presentation_deadlines.entry(id) {
            std::collections::hash_map::Entry::Occupied(mut entry)
                if entry.get().navigation == navigation =>
            {
                let current = entry.get_mut();
                current.wake = current.wake.min(wake);
                current.hard = current.hard.min(hard);
            }
            std::collections::hash_map::Entry::Occupied(_) => return,
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(PresentationDeadline {
                    wake,
                    hard,
                    navigation,
                });
            }
        }
        self.inner.timer_ready.notify_one();
    }

    pub(super) fn schedule_discard_probe(
        &self,
        id: ItemId,
        probe: DiscardProbeId,
        deadline: std::time::Instant,
    ) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if timer.stopped {
            return;
        }
        timer.discard_deadlines.insert(id, (deadline, probe));
        self.inner.timer_ready.notify_one();
    }

    pub(super) fn cancel_discard_probe(&self, id: ItemId) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        timer.discard_deadlines.remove(&id);
    }

    pub(super) fn schedule_profile_deletion(
        &self,
        profile: ProfileId,
        generation: u64,
        deadline: std::time::Instant,
    ) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if timer.stopped {
            return;
        }
        timer
            .profile_deletion_deadlines
            .insert(profile, (deadline, generation));
        self.inner.timer_ready.notify_one();
    }

    pub(super) fn cancel_profile_deletion(&self, profile: ProfileId) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        timer.profile_deletion_deadlines.remove(&profile);
    }

    /// Waits for either the low-frequency maintenance heartbeat or the one
    /// coalesced persistence deadline. Recomputing after every notification
    /// lets navigation churn move the debounce later without polling.
    pub(super) fn wait_for_timer(&self, maintenance_deadline: std::time::Instant) -> TimerWake {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if timer.stopped {
                return TimerWake::Stopped;
            }
            let now = std::time::Instant::now();
            if timer
                .persist_deadline
                .is_some_and(|deadline| now >= deadline)
            {
                timer.persist_deadline = None;
                return TimerWake::Persist;
            }
            let next_favicon = timer
                .favicon_deadlines
                .iter()
                .min_by_key(|(_, (deadline, _))| *deadline)
                .map(|(id, (deadline, attempt))| (*id, *deadline, *attempt));
            if let Some((id, deadline, attempt)) = next_favicon {
                if now >= deadline {
                    timer.favicon_deadlines.remove(&id);
                    return TimerWake::Favicon { id, attempt };
                }
            }
            let next_presentation = timer
                .presentation_deadlines
                .iter()
                .min_by_key(|(_, deadline)| deadline.wake)
                .map(|(id, deadline)| (*id, *deadline));
            if let Some((id, deadline)) = next_presentation {
                if now >= deadline.wake {
                    timer.presentation_deadlines.remove(&id);
                    return TimerWake::Presentation {
                        id,
                        navigation: deadline.navigation,
                        hard_deadline: deadline.hard,
                    };
                }
            }
            let next_discard = timer
                .discard_deadlines
                .iter()
                .min_by_key(|(_, (deadline, _))| *deadline)
                .map(|(id, (deadline, probe))| (*id, *deadline, *probe));
            if let Some((id, deadline, probe)) = next_discard {
                if now >= deadline {
                    timer.discard_deadlines.remove(&id);
                    return TimerWake::DiscardProbe { id, probe };
                }
            }
            let next_profile_deletion = timer
                .profile_deletion_deadlines
                .iter()
                .min_by_key(|(_, (deadline, _))| *deadline)
                .map(|(profile, (deadline, generation))| (*profile, *deadline, *generation));
            if let Some((profile, deadline, generation)) = next_profile_deletion {
                if now >= deadline {
                    timer.profile_deletion_deadlines.remove(&profile);
                    return TimerWake::ProfileDeletion {
                        profile,
                        generation,
                    };
                }
            }
            if now >= maintenance_deadline {
                return TimerWake::Maintenance;
            }
            let mut deadline = timer
                .persist_deadline
                .map_or(maintenance_deadline, |persist| {
                    persist.min(maintenance_deadline)
                });
            if let Some((_, favicon, _)) = next_favicon {
                deadline = deadline.min(favicon);
            }
            if let Some((_, presentation)) = next_presentation {
                deadline = deadline.min(presentation.wake);
            }
            if let Some((_, discard, _)) = next_discard {
                deadline = deadline.min(discard);
            }
            if let Some((_, profile_deletion, _)) = next_profile_deletion {
                deadline = deadline.min(profile_deletion);
            }
            let timeout = deadline.saturating_duration_since(now);
            let (next, _) = self
                .inner
                .timer_ready
                .wait_timeout(timer, timeout)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            timer = next;
        }
    }

    #[cfg(test)]
    pub(super) fn wait_for_tick(&self, interval: std::time::Duration) -> bool {
        matches!(
            self.wait_for_timer(std::time::Instant::now() + interval),
            TimerWake::Maintenance
        )
    }
}

pub(super) struct ActorExitGuard(pub(super) CommandQueue);

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

/// Adds one command to the single ordered queue. High-frequency engine state
/// may replace an older value only inside the trailing engine-only burst. It
/// can never cross a UI command, crash/result event, or shutdown barrier, so
/// a newer callback cannot make an older state appear after navigation.
pub(super) fn enqueue(
    commands: &mut VecDeque<Command>,
    command: Command,
    hard_capacity: usize,
    critical: bool,
) -> Result<(), Command> {
    let coalesced_key = command_coalesced_key(&command);
    if let Some(key) = coalesced_key {
        let burst_start = commands
            .iter()
            .rposition(|queued| command_coalesced_key(queued).is_none())
            .map_or(0, |index| index + 1);
        if let Some(index) = (burst_start..commands.len())
            .rev()
            .find(|index| command_coalesced_key(&commands[*index]) == Some(key))
        {
            commands.remove(index);
            commands.push_back(command);
            return Ok(());
        }
    }
    if commands.len() >= hard_capacity {
        // At the absolute lifecycle ceiling, replace an older fact for the
        // same bounded native object and move the newest observation to the
        // back. This preserves its ordering relative to intervening commands
        // while preventing a page from crowding out a renderer-death or final
        // source update with repeated callbacks for one tab.
        if critical {
            if let Some(key) = recovery_key(&command) {
                if let Some(index) = commands
                    .iter()
                    .position(|queued| recovery_key(queued) == Some(key))
                {
                    commands.remove(index);
                    commands.push_back(command);
                    return Ok(());
                }
            }
        }
        // An operation already reported as accepted is immutable FIFO state.
        // Lifecycle facts may replace only bounded presentation facts; they
        // must never evict open/close/navigate/settings or another mutation.
        let index = commands.iter().position(|queued| {
            !command_is_critical(queued) && command_coalesced_key(queued).is_some()
        });
        let Some(index) = index else {
            return Err(command);
        };
        commands.remove(index);
    }
    commands.push_back(command);
    Ok(())
}

/// Native state transitions whose loss can leave Rust believing a destroyed
/// or failed view is still live. They receive reserved admission and may
/// displace older presentation facts at the absolute lifecycle ceiling.
fn command_is_critical(command: &Command) -> bool {
    matches!(
        command,
        Command::ProfileDeletionReady(_)
            | Command::ChromePresentationApplied { .. }
            | Command::Engine(
                EngineEvent::UrlChanged { .. }
                    | EngineEvent::PresentationPending { .. }
                    | EngineEvent::PresentationReady { .. }
                    | EngineEvent::RuntimeRestartRequired
                    | EngineEvent::NavigationFailed { .. }
                    | EngineEvent::ZoomSettled { .. }
                    | EngineEvent::NativeActionFailed { .. }
                    | EngineEvent::ViewCreationFailed { .. }
                    | EngineEvent::ProfileProcessExited { .. }
                    | EngineEvent::Crashed { .. }
                    | EngineEvent::ViewDiscarded { .. }
                    | EngineEvent::SplitChanged { .. }
            )
    )
}

fn command_coalesced_key(command: &Command) -> Option<CoalescedKey> {
    match command {
        Command::Engine(event) => CoalescedKey::of(event),
        Command::SetWindowSize(_) => Some(CoalescedKey::WindowSize),
        Command::SetWindowVisible(_) => Some(CoalescedKey::WindowVisible),
        Command::SetSidebarWidth(_) => Some(CoalescedKey::SidebarWidth),
        Command::DragOver { .. } => Some(CoalescedKey::DragOver),
        Command::DividerDrag { .. } => Some(CoalescedKey::DividerDrag),
        Command::Search(_) => Some(CoalescedKey::Search),
        Command::FaviconPoll { id, .. } => Some(CoalescedKey::FaviconPoll(*id)),
        Command::PresentationFallback { id, .. } => Some(CoalescedKey::PresentationFallback(*id)),
        Command::ChromePresentationApplied { id, .. } => {
            Some(CoalescedKey::ChromePresentation(*id))
        }
        Command::DiscardProbeTimeout { id, .. } => Some(CoalescedKey::DiscardProbeTimeout(*id)),
        Command::ProfileDeletionReady(profile) => {
            Some(CoalescedKey::ProfileDeletionReady(*profile))
        }
        Command::ProfileDeletionRetry { profile, .. } => {
            Some(CoalescedKey::ProfileDeletionRetry(*profile))
        }
        Command::StoreRead(StoreReadResult::History { .. }) => Some(CoalescedKey::StoreHistory),
        Command::StoreRead(StoreReadResult::Favicon { id, .. }) => {
            Some(CoalescedKey::StoreFavicon(*id))
        }
        Command::StoreRead(StoreReadResult::FaviconBatch { .. }) => {
            Some(CoalescedKey::StoreFaviconBatch)
        }
        Command::Persist => Some(CoalescedKey::Persist),
        Command::Tick => Some(CoalescedKey::Tick),
        _ => None,
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
    shell.self_queue = Some(queue.clone());
    let actor = match spawn_worker("zephium-shell", move || {
        let _exit_guard = ActorExitGuard(actor_queue.clone());
        let _store_reader_guard = StoreReaderStopGuard::new(actor_store_reads);
        while let Some(command) = actor_queue.recv() {
            shell.handle(command);
            if shell.shutdown_result.is_some() {
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
