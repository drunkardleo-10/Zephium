//! The actor shell around the pure core. One thread owns all state; commands
//! enter through a queue (UI intents and engine events alike), effects leave
//! through ports, projections go to the UI.

mod store_reads;

#[doc(hidden)]
pub use store_reads::StoreReadResult;
#[cfg(test)]
use store_reads::FAVICON_CACHE_MAX_AGE_SECONDS;
use store_reads::{run as run_store_reader, StoreReadQueue, StoreReaderStopGuard};

use std::collections::VecDeque;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::thread::JoinHandle;

use zephium_core::geometry::{Rect, Size};
use zephium_core::ids::{ItemId, ProfileId, SpaceId, WindowId};
use zephium_core::item::{Lifecycle, Placement, SpaceSection, TabState};
use zephium_core::items::{Effect, Items};
use zephium_core::layout;
use zephium_core::ports::chrome::{Chrome as GeometryChrome, ChromeFrame};
use zephium_core::ports::engine::{
    DiscardProbeId, Engine, EngineEvent, NativeAction, NativeDispatch, NavigationPresentationId,
    Partition, ProfileDataErasureOutcome, ZoomRequestId,
};
use zephium_core::ports::net::Net;
use zephium_core::ports::store::{
    PendingProfileDeletion, ProfileDeletionAuthorizeOutcome, ProfileDeletionFinalizeOutcome,
    ProfileDeletionLoad, SessionLoad, Store, StoreShutdownOutcome, MAX_FAVICON_BATCH_ORIGINS,
};
use zephium_core::profiles::{Profile, ProfileKind, Profiles};
use zephium_core::session;
use zephium_core::spaces::{Space, Spaces};
use zephium_core::split::{self, Axis, Edge, Pane};
use zephium_core::windows::{WindowKind, Windows};
use zephium_core::{commands, navigation};
use zephium_ipc::{
    DividerView, ItemsState, LayoutState, OperationDisposition, OperationOutcome, OperationReason,
    Projection, RuntimeStatus, SearchAction, SearchResult, SearchResults, TabView,
};

pub type SharedEngine = Arc<dyn Engine + Send + Sync>;
pub type SharedStore = Arc<dyn Store + Send + Sync>;
pub type SharedNet = Arc<dyn Net + Send + Sync>;
pub type SharedChrome = Arc<dyn PresentationChrome + Send + Sync>;
pub type EmitFn = Box<dyn Fn(Projection) + Send + Sync>;

/// Exact privileged-chrome work that must complete before one raw document
/// can become visible. The tab projection is carried in the same native eval
/// as the acknowledgement, avoiding an ordering assumption between generic
/// projection delivery and native content presentation.
#[derive(Clone, Debug, PartialEq)]
pub struct ChromePresentation {
    pub id: ItemId,
    pub navigation: NavigationPresentationId,
    pub url: String,
    pub tab: TabView,
    pub active: Option<ItemId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromePresentationDispatch {
    /// The adapter applied and verified the projection synchronously. Used by
    /// deterministic embedders/tests; production native adapters are async.
    Applied,
    /// Callback ownership was accepted. It must report verification success
    /// or failure without blocking its native UI thread.
    Scheduled,
    /// No callback ownership transfer occurred.
    Rejected,
}

pub type ChromePresentationCallback = Box<dyn FnOnce(bool) + Send>;

/// Geometry plus the privileged DOM acknowledgement required by the raw-view
/// anti-spoof boundary.
pub trait PresentationChrome: GeometryChrome {
    fn apply_tab_for_presentation(
        &self,
        presentation: ChromePresentation,
        done: ChromePresentationCallback,
    ) -> ChromePresentationDispatch;
}

/// Terminal result of the ordered application shutdown protocol.
///
/// A retryable failure happens before native teardown and leaves the actor
/// live. `Unclean` is terminal: either the actor exited without completing the
/// barrier or native teardown did not prove private engine data was removed,
/// so the process must exit unsuccessfully.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownOutcome {
    RetryableFailure,
    Clean,
    Unclean,
}

#[derive(Clone, Debug)]
pub enum Command {
    /// A privileged user mutation with an externally visible admission and
    /// actor-order disposition identity. Engine callbacks and replaceable UI
    /// facts never use this wrapper.
    Operation {
        operation_id: String,
        command: Box<Command>,
    },
    Bootstrap,
    Open,
    Activate(ItemId),
    Close(ItemId),
    Navigate {
        id: ItemId,
        input: String,
    },
    Reload(ItemId),
    GoBack(ItemId),
    GoForward(ItemId),
    SplitWith {
        other: ItemId,
        axis: Axis,
    },
    Unsplit,
    SetWindowSize(Size),
    /// Whether the OS can currently present the main window. Minimized
    /// windows hide native content views so the engine can lower their memory
    /// priority and, after the normal idle grace, suspend them.
    SetWindowVisible(bool),
    SetSidebarWidth(f64),
    DragOver {
        x: f64,
        y: f64,
    },
    DropTab {
        id: ItemId,
        x: f64,
        y: f64,
    },
    DividerGrab {
        x: f64,
        y: f64,
    },
    DividerDrag {
        x: f64,
        y: f64,
    },
    DividerRelease {
        /// Final pointer position, folded into the same ordered mutation as
        /// release so separate IPC deliveries cannot persist a stale ratio.
        x: Option<f64>,
        y: Option<f64>,
    },
    Run(String),
    Search(String),
    OpenUrl(String),
    SetAppSetting {
        key: String,
        value: String,
    },
    /// Permanently removes one inactive named profile through the durable
    /// cross-store deletion coordinator. The profile id comes only from
    /// privileged chrome and is revalidated against authoritative state.
    DeleteProfile(ProfileId),
    /// Bounded retry for the renderer-owned asynchronous favicon decode.
    FaviconPoll {
        id: ItemId,
        attempt: u8,
    },
    /// Bounded admission retry for one exact committed navigation. Normal
    /// presentation is requested immediately after its URL reaches chrome;
    /// stale identities can never reveal overlapping content.
    PresentationFallback {
        id: ItemId,
        navigation: NavigationPresentationId,
        /// Absolute dispatch-admission bound. Retries and overlapping
        /// navigations cannot move it later.
        hard_deadline: std::time::Instant,
    },
    /// Result of one privileged eval-with-callback presentation barrier. The
    /// callback is untrusted lifecycle timing: the actor revalidates every
    /// field against its current exact pending obligation.
    ChromePresentationApplied {
        id: ItemId,
        navigation: NavigationPresentationId,
        url: String,
        active: Option<ItemId>,
        projection_revision: String,
        applied: bool,
    },
    /// Fail-closed deadline for one exact renderer discard-safety probe.
    DiscardProbeTimeout {
        id: ItemId,
        probe: DiscardProbeId,
    },
    /// Exact-generation wakeup for a native profile-erasure callback. The
    /// outcome itself stays in a bounded inbox so queue overload cannot lose
    /// the security-critical proof.
    ProfileDeletionReady(ProfileId),
    /// One bounded-backoff retry for journal reconciliation, native erasure,
    /// or local SQLite finalization.
    ProfileDeletionRetry {
        profile: ProfileId,
        generation: u64,
    },
    /// Completion from the bounded storage-read worker. Every result carries
    /// the exact request generation and is revalidated against current shell
    /// state before it can affect privileged projections.
    StoreRead(StoreReadResult),
    /// Internal one-shot debounce fired by the queue's single timer thread.
    Persist,
    /// Periodic maintenance heartbeat; idle tabs suspend or hibernate even
    /// when no user command arrives.
    Tick,
    Engine(EngineEvent),
    /// Ordered process-boundary barrier. The actor snapshots after every
    /// command already queued ahead of this one, then flushes the store.
    Shutdown {
        deadline: std::time::Instant,
        ack: SyncSender<ShutdownOutcome>,
    },
}

pub struct Handle {
    queue: CommandQueue,
    workers: Arc<WorkerThreads>,
    counted: bool,
}

pub struct ShutdownRequest {
    deadline: std::time::Instant,
    receiver: Receiver<ShutdownOutcome>,
    workers: Arc<WorkerThreads>,
}

#[derive(Default)]
struct WorkerThreads {
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
    queue: Weak<CommandQueueInner>,
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
    fn new(queue: CommandQueue) -> Self {
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
const NORMAL_COMMAND_CAPACITY: usize = 960;
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
const COMMAND_QUEUE_CAPACITY: usize = NORMAL_COMMAND_CAPACITY + MAX_CRITICAL_LIFECYCLE_FACTS + 1;
const LIFECYCLE_COMMAND_CAPACITY: usize = COMMAND_QUEUE_CAPACITY - 1;
// During a failed store barrier, keep at most the bounded set of lifecycle
// facts the native engine can produce for the maximum item/profile counts.
const POST_BARRIER_CRITICAL_CAPACITY: usize = MAX_CRITICAL_LIFECYCLE_FACTS;
// More simultaneous native renderers are neither usable in the current tiled
// layout nor safe to reconstruct synchronously after a restore/process loss.
const MAX_VISIBLE_PANES: usize = 8;
const TRACKED_ICON_ORIGIN_CAPACITY: usize = 2048;
const ICON_CACHE_CAPACITY: usize = 512;
const FAVICON_POLL_DELAYS: [std::time::Duration; 7] = [
    std::time::Duration::from_millis(100),
    std::time::Duration::from_millis(250),
    std::time::Duration::from_millis(500),
    std::time::Duration::from_secs(1),
    std::time::Duration::from_millis(1500),
    std::time::Duration::from_millis(2500),
    std::time::Duration::from_secs(4),
];
// A committed document is acknowledged as soon as privileged chrome verifies
// its exact revision-bearing URL projection. This deadline bounds only
// callback/native-dispatch admission retries; it is never an intentional
// first-paint delay or authority for a timeout reveal.
const PRESENTATION_ADMISSION_HARD_LIMIT: std::time::Duration = std::time::Duration::from_secs(2);
const PRESENTATION_ADMISSION_RETRY_DELAYS: [std::time::Duration; 7] = [
    std::time::Duration::from_millis(25),
    std::time::Duration::from_millis(50),
    std::time::Duration::from_millis(100),
    std::time::Duration::from_millis(200),
    std::time::Duration::from_millis(400),
    std::time::Duration::from_millis(800),
    std::time::Duration::from_secs(1),
];
const MAX_PRESENTATION_ADMISSION_REJECTIONS: u8 = 12;
const MAX_OPERATION_ID_BYTES: usize = 64;
const PERSIST_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(400);
const PERSIST_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(2);
const MAINTENANCE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
// A normal browsing set stays warm up to the soft target. Above the pressure
// watermark, hidden pages enter bounded exact-safety probing before the long
// idle grace. Eight additional slots cover the largest visible split/recovery
// batch before the absolute logical admission ceiling. Pages are never force-
// discarded to make room: unsafe pages remain resident and excess creates
// fail synchronously as ordinary hibernated tabs in the model.
const LIVE_VIEW_SOFT_LIMIT: usize = 12;
const LIVE_VIEW_PRESSURE_LIMIT: usize = 24;
const LIVE_VIEW_ABSOLUTE_LIMIT: usize = LIVE_VIEW_PRESSURE_LIMIT + MAX_VISIBLE_PANES;
const MAX_CONCURRENT_DISCARD_PROBES: usize = 4;
const DISCARD_IDLE_GRACE: std::time::Duration = std::time::Duration::from_secs(15 * 60);
const DISCARD_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const DISCARD_PROTECTED_RETRY: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const PROFILE_DELETION_STORE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const PROFILE_DELETION_RETRY_BASE: std::time::Duration = std::time::Duration::from_millis(250);
const PROFILE_DELETION_RETRY_MAX: std::time::Duration = std::time::Duration::from_secs(30);
const MAX_ASYNC_SEARCH_QUERY_BYTES: usize = 4 * 1024;
const STORE_READ_RESULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
// URL-only native observations are recoverability checkpoints, not structural
// mutations. Structural changes retain the fast debounce; URL churn is
// globally coalesced to one full snapshot per five minutes.
const URL_CHECKPOINT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const URL_CHECKPOINT_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(not(test))]
// FIFO wait, storage-reader quiescence, snapshot construction, durability,
// native teardown, and thread joins consume this one caller-owned deadline.
const END_TO_END_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
#[cfg(test)]
const END_TO_END_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct NativeWork {
    scheduled: bool,
    rejected: bool,
    unsupported: bool,
}

impl NativeWork {
    fn record(&mut self, admission: NativeDispatch) {
        match admission {
            NativeDispatch::Scheduled => self.scheduled = true,
            NativeDispatch::Rejected => self.rejected = true,
            NativeDispatch::Unsupported => self.unsupported = true,
        }
    }

    fn merge(&mut self, other: Self) {
        self.scheduled |= other.scheduled;
        self.rejected |= other.rejected;
        self.unsupported |= other.unsupported;
    }
}

fn operation_result(outcome: OperationOutcome, reason: OperationReason) -> OperationDisposition {
    OperationDisposition {
        operation_id: String::new(),
        outcome,
        reason,
    }
}

fn mutation_result(native: NativeWork) -> OperationDisposition {
    if native.rejected {
        operation_result(
            OperationOutcome::NativeAdmissionFailed,
            OperationReason::NativeDispatchRejected,
        )
    } else if native.unsupported {
        operation_result(
            OperationOutcome::Rejected,
            OperationReason::UnsupportedCommand,
        )
    } else if native.scheduled {
        operation_result(
            OperationOutcome::Deferred,
            OperationReason::NativeWorkPending,
        )
    } else {
        operation_result(OperationOutcome::Applied, OperationReason::MutationApplied)
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

#[derive(Clone)]
struct CommandQueue {
    inner: Arc<CommandQueueInner>,
}

struct CommandQueueInner {
    state: Mutex<CommandQueueState>,
    ready: Condvar,
    timer_state: Mutex<TimerState>,
    timer_ready: Condvar,
}

#[derive(Default)]
struct TimerState {
    stopped: bool,
    persist_deadline: Option<std::time::Instant>,
    favicon_deadlines: std::collections::HashMap<ItemId, (std::time::Instant, u8)>,
    presentation_deadlines: std::collections::HashMap<ItemId, PresentationDeadline>,
    discard_deadlines: std::collections::HashMap<ItemId, (std::time::Instant, DiscardProbeId)>,
    profile_deletion_deadlines: std::collections::HashMap<ProfileId, (std::time::Instant, u64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PresentationDeadline {
    wake: std::time::Instant,
    hard: std::time::Instant,
    navigation: NavigationPresentationId,
}

enum TimerWake {
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
struct CommandQueueState {
    commands: VecDeque<Command>,
    // Critical native callbacks that race an in-progress store barrier. They
    // are not reported as accepted behind Shutdown, but must remain available
    // if that barrier fails and the live browser resumes.
    post_barrier_critical: VecDeque<Command>,
    closed: bool,
    shutdown_enqueued: bool,
    handles: usize,
}

enum TryPushError {
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
    fn new() -> Self {
        Self {
            inner: Arc::new(CommandQueueInner {
                state: Mutex::new(CommandQueueState::default()),
                ready: Condvar::new(),
                timer_state: Mutex::new(TimerState::default()),
                timer_ready: Condvar::new(),
            }),
        }
    }

    fn retain_handle(&self) -> bool {
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

    fn release_handle(&self) {
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

    fn try_push(&self, command: Command) -> Result<(), TryPushError> {
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

    fn recv(&self) -> Option<Command> {
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
    fn try_recv(&self) -> Option<Command> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.commands.pop_front()
    }

    fn reopen_after_failed_shutdown(&self) -> Vec<Command> {
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

    fn close_and_drain(&self) -> Vec<Command> {
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

    fn stop_ticker(&self) {
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

    fn schedule_persist(&self, deadline: std::time::Instant) {
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

    fn cancel_persist(&self) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        timer.persist_deadline = None;
    }

    fn schedule_favicon(&self, id: ItemId, attempt: u8, deadline: std::time::Instant) {
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

    fn cancel_favicon(&self, id: ItemId) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        timer.favicon_deadlines.remove(&id);
    }

    fn schedule_presentation(
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

    fn cancel_presentation(&self, id: ItemId) {
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
    fn retry_presentation(
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

    fn schedule_discard_probe(
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

    fn cancel_discard_probe(&self, id: ItemId) {
        let mut timer = self
            .inner
            .timer_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        timer.discard_deadlines.remove(&id);
    }

    fn schedule_profile_deletion(
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

    fn cancel_profile_deletion(&self, profile: ProfileId) {
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
    fn wait_for_timer(&self, maintenance_deadline: std::time::Instant) -> TimerWake {
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
    fn wait_for_tick(&self, interval: std::time::Duration) -> bool {
        matches!(
            self.wait_for_timer(std::time::Instant::now() + interval),
            TimerWake::Maintenance
        )
    }
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

/// Adds one command to the single ordered queue. High-frequency engine state
/// may replace an older value only inside the trailing engine-only burst. It
/// can never cross a UI command, crash/result event, or shutdown barrier, so
/// a newer callback cannot make an older state appear after navigation.
fn enqueue(
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

fn valid_native_split_update(current: &Pane, candidate: &Pane) -> bool {
    match (current, candidate) {
        (Pane::Leaf(expected), Pane::Leaf(actual)) => expected == actual,
        (
            Pane::Branch {
                axis: expected_axis,
                a: expected_a,
                b: expected_b,
                ..
            },
            Pane::Branch { axis, ratio, a, b },
        ) => {
            axis == expected_axis
                && ratio.is_finite()
                && (0.05..=0.95).contains(ratio)
                && valid_native_split_update(expected_a, a)
                && valid_native_split_update(expected_b, b)
        }
        _ => false,
    }
}

pub fn spawn(
    engine: SharedEngine,
    store: SharedStore,
    chrome: SharedChrome,
    net: SharedNet,
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
    let mut shell = Shell::with_store_reads(engine, store, chrome, net, emit, store_reads.clone());
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

#[derive(Clone)]
struct IconAttempt {
    profile: ProfileId,
    origin: String,
    next_attempt: u8,
}

#[derive(Clone, PartialEq, Eq)]
struct PendingFaviconStoreRead {
    generation: u64,
    profile: ProfileId,
    origin: String,
}

struct PendingFaviconBatch {
    generation: u64,
    profile: ProfileId,
    space: SpaceId,
    requested: std::collections::HashSet<String>,
}

struct PendingSearch {
    generation: u64,
    profile: ProfileId,
    lookup_query: String,
    display_query: String,
    open_urls: std::collections::HashSet<String>,
    base_results: Vec<SearchResult>,
}

#[derive(Clone)]
enum PendingDiscardProbe {
    Probing {
        probe: DiscardProbeId,
        committed_url: String,
        deadline: std::time::Instant,
    },
    Closing {
        probe: DiscardProbeId,
        recreate: bool,
        deferred_navigation: Option<String>,
    },
}

enum ProfileDeletionPhase {
    /// Authorization RPC entered the storage actor but its result missed the
    /// caller deadline. An ordered journal read must resolve that attempt
    /// before a retry builds a fresh post-removal snapshot from the current
    /// aggregates. Retaining the original snapshot here would let a later
    /// retry overwrite survivor mutations accepted in the meantime.
    Authorizing {
        may_reauthorize: bool,
    },
    /// `AlreadyAuthorized` proved the barrier, but an exact journal read is
    /// still needed to choose native erasure versus local-only finalization.
    ResolveAuthorizedJournal,
    NativeReady,
    NativeInFlight {
        attempt: u64,
    },
    FinalizeReady,
}

struct ProfileDeletionState {
    phase: ProfileDeletionPhase,
    operation_id: Option<String>,
    /// Session revision represented by the snapshot used for the latest
    /// authorization attempt. If journal proof arrives after this changes, the
    /// authorization barrier is still valid but the newer survivor state needs
    /// a fresh ordinary persistence pass after the logical tombstone lands.
    authorization_revision: u128,
    attempt_generation: u64,
    retry_generation: u64,
    retry_exponent: u8,
}

impl ProfileDeletionState {
    fn new(
        phase: ProfileDeletionPhase,
        operation_id: Option<String>,
        authorization_revision: u128,
    ) -> Self {
        Self {
            phase,
            operation_id,
            authorization_revision,
            attempt_generation: 0,
            retry_generation: 0,
            retry_exponent: 0,
        }
    }
}

type ProfileDeletionInbox =
    Arc<Mutex<std::collections::HashMap<ProfileId, (u64, ProfileDataErasureOutcome)>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingPresentation {
    navigation: NavigationPresentationId,
    url: String,
    hard_deadline: std::time::Instant,
    admission_rejections: u8,
    chrome_applied: bool,
    chrome_request_in_flight: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingZoom {
    request: ZoomRequestId,
    desired_scale: f64,
}

#[derive(Clone, Debug)]
struct GrabbedDivider {
    window: WindowId,
    topology: Pane,
    divider: split::Divider,
}

pub struct Shell {
    profiles: Profiles,
    spaces: Spaces,
    items: Items,
    windows: Windows,
    pending_size: Size,
    icons_checked: std::collections::HashSet<(ProfileId, String)>,
    icon_values: std::collections::HashMap<(ProfileId, String), String>,
    icon_cache_order: VecDeque<(ProfileId, String)>,
    icon_attempts: std::collections::HashMap<ItemId, IconAttempt>,
    icon_load_completion_pending: std::collections::HashMap<ItemId, (ProfileId, String)>,
    favicon_store_reads: std::collections::HashMap<ItemId, PendingFaviconStoreRead>,
    favicon_store_generation: u64,
    pending_favicon_batch: Option<PendingFaviconBatch>,
    favicon_batch_generation: u64,
    pending_search: Option<PendingSearch>,
    search_generation: u64,
    pending_presentations: std::collections::HashMap<ItemId, PendingPresentation>,
    presented_navigations: std::collections::HashMap<ItemId, (NavigationPresentationId, String)>,
    /// A fresh tab keeps its real privileged New Tab document until the first
    /// exact committed-URL presentation eval replaces and verifies it. Native
    /// content geometry is admitted only after that callback.
    deferred_first_content_layout: std::collections::HashSet<ItemId>,
    /// Last revision actually offered to privileged chrome for each item.
    /// Exact eval callbacks must still match this value when the actor
    /// receives them; a newer masked or full projection invalidates an older
    /// success without unrelated-tab churn causing starvation.
    last_tab_projection_revision: std::cell::RefCell<std::collections::HashMap<ItemId, String>>,
    projection_sequence: std::cell::Cell<u128>,
    next_zoom_request: u64,
    pending_zooms: std::collections::HashMap<ItemId, PendingZoom>,
    divider: Option<GrabbedDivider>,
    recent: Vec<ItemId>,
    last_focus: std::collections::HashMap<ItemId, std::time::Instant>,
    last_visits: std::collections::HashMap<ItemId, (String, std::time::Instant)>,
    dormant_min: std::time::Duration,
    dormant_sent: Vec<ItemId>,
    discard_idle_min: std::time::Duration,
    discard_probe_timeout: std::time::Duration,
    discard_protected_retry: std::time::Duration,
    live_view_soft_limit: usize,
    live_view_pressure_limit: usize,
    next_discard_probe: u64,
    discard_probes: std::collections::HashMap<ItemId, PendingDiscardProbe>,
    discard_protected_until: std::collections::HashMap<ItemId, std::time::Instant>,
    window_visible: bool,
    runtime_restart_required: bool,
    crashes: std::collections::HashMap<ItemId, std::time::Instant>,
    crash_presentations: std::collections::HashSet<ItemId>,
    bootstrapped: bool,
    /// Monotonic process-local identity for the session state represented by
    /// persistence scheduling. A u128 wrap would require more mutations than
    /// the process can physically execute; wrapping keeps this path infallible
    /// in release builds while preserving a fail-safe practical bound.
    session_revision: u128,
    persist_first_dirty: Option<std::time::Instant>,
    url_checkpoint_dirty: std::collections::HashSet<ItemId>,
    last_url_checkpoint: std::time::Instant,
    shutdown_result: Option<ShutdownOutcome>,
    self_queue: Option<CommandQueue>,
    profile_deletions: std::collections::HashMap<ProfileId, ProfileDeletionState>,
    /// Exact startup cohort whose per-profile history/favicon database was
    /// preserved but disabled by storage validation. Session/meta state and
    /// native website data remain independently usable.
    degraded_storage_profiles: std::collections::HashSet<ProfileId>,
    profile_deletion_inbox: ProfileDeletionInbox,
    profile_deletion_batch_deadline: Option<std::time::Instant>,
    engine: SharedEngine,
    store: SharedStore,
    store_reads: Option<StoreReadQueue>,
    chrome: SharedChrome,
    _net: SharedNet,
    emit: EmitFn,
}

impl Shell {
    #[cfg(test)]
    pub fn new(
        engine: SharedEngine,
        store: SharedStore,
        chrome: SharedChrome,
        net: SharedNet,
        emit: EmitFn,
    ) -> Self {
        Self::with_store_reads(engine, store, chrome, net, emit, None)
    }

    fn with_store_reads(
        engine: SharedEngine,
        store: SharedStore,
        chrome: SharedChrome,
        net: SharedNet,
        emit: EmitFn,
        store_reads: impl Into<Option<StoreReadQueue>>,
    ) -> Self {
        Self {
            profiles: Profiles::default(),
            spaces: Spaces::default(),
            items: Items::default(),
            windows: Windows::default(),
            pending_size: Size::default(),
            icons_checked: std::collections::HashSet::new(),
            icon_values: std::collections::HashMap::new(),
            icon_cache_order: VecDeque::new(),
            icon_attempts: std::collections::HashMap::new(),
            icon_load_completion_pending: std::collections::HashMap::new(),
            favicon_store_reads: std::collections::HashMap::new(),
            favicon_store_generation: 0,
            pending_favicon_batch: None,
            favicon_batch_generation: 0,
            pending_search: None,
            search_generation: 0,
            pending_presentations: std::collections::HashMap::new(),
            presented_navigations: std::collections::HashMap::new(),
            deferred_first_content_layout: std::collections::HashSet::new(),
            last_tab_projection_revision: std::cell::RefCell::new(std::collections::HashMap::new()),
            projection_sequence: std::cell::Cell::new(0),
            next_zoom_request: 0,
            pending_zooms: std::collections::HashMap::new(),
            divider: None,
            recent: Vec::new(),
            last_focus: std::collections::HashMap::new(),
            last_visits: std::collections::HashMap::new(),
            dormant_min: std::time::Duration::from_secs(5 * 60),
            dormant_sent: Vec::new(),
            discard_idle_min: DISCARD_IDLE_GRACE,
            discard_probe_timeout: DISCARD_PROBE_TIMEOUT,
            discard_protected_retry: DISCARD_PROTECTED_RETRY,
            live_view_soft_limit: LIVE_VIEW_SOFT_LIMIT,
            live_view_pressure_limit: LIVE_VIEW_PRESSURE_LIMIT,
            next_discard_probe: 0,
            discard_probes: std::collections::HashMap::new(),
            discard_protected_until: std::collections::HashMap::new(),
            window_visible: true,
            runtime_restart_required: false,
            crashes: std::collections::HashMap::new(),
            crash_presentations: std::collections::HashSet::new(),
            bootstrapped: false,
            session_revision: 0,
            persist_first_dirty: None,
            url_checkpoint_dirty: std::collections::HashSet::new(),
            last_url_checkpoint: std::time::Instant::now(),
            shutdown_result: None,
            self_queue: None,
            profile_deletions: std::collections::HashMap::new(),
            degraded_storage_profiles: std::collections::HashSet::new(),
            profile_deletion_inbox: Arc::new(Mutex::new(std::collections::HashMap::new())),
            profile_deletion_batch_deadline: None,
            engine,
            store,
            store_reads: store_reads.into(),
            chrome,
            _net: net,
            emit,
        }
    }

    pub fn handle(&mut self, cmd: Command) {
        // No late engine/UI/timer work may mutate state after the final
        // snapshot. Repeated shutdown requests receive the original result.
        if let Some(outcome) = self.shutdown_result {
            if let Command::Shutdown { ack, .. } = cmd {
                let _ = ack.send(outcome);
            }
            return;
        }
        match cmd {
            Command::Operation {
                operation_id,
                command,
            } => {
                // Public construction rejects nested operations and shutdown
                // barriers. Keep the actor defensive if a future in-process
                // caller bypasses that constructor.
                if matches!(
                    command.as_ref(),
                    Command::Operation { .. } | Command::Shutdown { .. }
                ) {
                    return;
                }
                let command = *command;
                if let Command::DeleteProfile(profile) = &command {
                    let profile = *profile;
                    let mut completion =
                        self.begin_profile_deletion(profile, Some(operation_id.clone()));
                    completion.operation_id = operation_id;
                    let deferred = completion.outcome == OperationOutcome::Deferred;
                    if deferred {
                        // Admission already told the caller this operation is
                        // owned by the FIFO. Retain its id and emit no
                        // misleading terminal disposition while either durable phase
                        // is pending or retrying.
                        self.drive_profile_deletion(profile);
                    } else {
                        (self.emit)(Projection::OperationProcessed(completion));
                    }
                    return;
                }
                let mut completion = self.handle_operation(command);
                completion.operation_id = operation_id;
                (self.emit)(Projection::OperationProcessed(completion));
            }
            Command::Bootstrap => self.bootstrap(),
            Command::Open => {
                let _ = self.operation_open();
            }
            Command::Activate(id) => {
                let _ = self.operation_activate(id);
            }
            Command::Close(id) => {
                let _ = self.operation_close(id);
            }
            Command::Navigate { id, input } => {
                let _ = self.operation_navigate(id, input);
            }
            Command::Reload(id) => {
                let _ = self.operation_reload(id);
            }
            Command::GoBack(id) => {
                let _ = self.operation_history(id, false);
            }
            Command::GoForward(id) => {
                let _ = self.operation_history(id, true);
            }
            Command::SplitWith { other, axis } => {
                let _ = self.operation_split(other, axis);
            }
            Command::Unsplit => {
                let _ = self.operation_unsplit();
            }
            Command::SetWindowSize(size) => match self.windows.focused_mut() {
                Some(win) => {
                    win.size = size;
                    // macOS resizes natively via autoresizing masks; Windows
                    // and Linux have no equivalent, the shell must relayout.
                    let _ = self.relayout();
                }
                None => self.pending_size = size,
            },
            Command::SetWindowVisible(visible) => {
                if self.window_visible != visible {
                    self.window_visible = visible;
                    // Do not immediately suspend a page that was actively in
                    // use when the window minimized. The ordinary idle grace
                    // still applies after every content view becomes hidden.
                    if !visible {
                        // OS pointer capture cannot remain authoritative while
                        // its window is hidden/minimized.
                        self.divider = None;
                        if let Some(active) = self.windows.focused().and_then(|w| w.active) {
                            self.touch(active);
                        }
                    }
                    let _ = self.relayout();
                    self.maintain_views();
                }
            }
            Command::SetSidebarWidth(width) => {
                if let Some(win) = self.windows.focused_mut() {
                    win.metrics.sidebar_width = width.clamp(180.0, 420.0);
                }
                let _ = self.relayout();
            }
            Command::DragOver { x, y } => {
                if let Some(win) = self.windows.focused().map(|w| w.id) {
                    let zone = self.resolve_drop(x, y).map(|d| d.zone);
                    let _ = self.engine.set_drop_indicator(win, zone);
                }
            }
            Command::DropTab { id, x, y } => {
                let _ = self.operation_drop_tab(id, x, y);
            }
            Command::DividerGrab { x, y } => self.divider = self.locate_divider(x, y),
            Command::DividerDrag { x, y } => self.divider_drag(x, y),
            Command::DividerRelease { x, y } => {
                let _ = self.operation_divider_release(x.zip(y));
            }
            Command::Run(id) => {
                let _ = self.operation_run_command(&id);
            }
            Command::Search(query) => self.search(&query),
            Command::OpenUrl(input) => {
                let _ = self.operation_open_url(input);
            }
            Command::SetAppSetting { key, value } => {
                let _ = self.operation_set_app_setting(key, value);
            }
            // Profile deletion is accepted only through `Command::Operation`
            // so every foreground request has one truthful terminal identity.
            Command::DeleteProfile(_) => {}
            Command::FaviconPoll { id, attempt } => self.poll_favicon(id, attempt),
            Command::PresentationFallback {
                id,
                navigation,
                hard_deadline,
            } => self.on_presentation_fallback(id, navigation, hard_deadline),
            Command::ChromePresentationApplied {
                id,
                navigation,
                url,
                active,
                projection_revision,
                applied,
            } => self.on_chrome_presentation_applied(
                id,
                navigation,
                url,
                active,
                projection_revision,
                applied,
            ),
            Command::DiscardProbeTimeout { id, probe } => self.on_discard_probe_timeout(id, probe),
            Command::ProfileDeletionReady(profile) => {
                self.consume_profile_deletion_outcome(profile)
            }
            Command::ProfileDeletionRetry {
                profile,
                generation,
            } => {
                if self
                    .profile_deletions
                    .get(&profile)
                    .is_some_and(|state| state.retry_generation == generation)
                {
                    self.drive_profile_deletion(profile);
                }
            }
            Command::StoreRead(result) => self.on_store_read(result),
            Command::Persist => self.persist(),
            Command::Tick => {
                self.drain_profile_deletion_inbox();
                self.reconcile_runtime_restart_requirement();
                if self.maintain_views() {
                    self.project_items();
                }
            }
            Command::Engine(event) => self.on_engine_event(event),
            Command::Shutdown { deadline, ack } => {
                if std::time::Instant::now() >= deadline {
                    self.retryable_shutdown_failure(ack);
                    return;
                }
                if self
                    .store_reads
                    .as_ref()
                    .is_some_and(|reads| !reads.quiesce_until(deadline))
                {
                    self.retryable_shutdown_failure(ack);
                    return;
                }
                self.clear_pending_store_reads();
                self.persist();
                match self.store.shutdown_until(deadline) {
                    StoreShutdownOutcome::Clean => {}
                    StoreShutdownOutcome::RetryableFailure => {
                        // The store proves its terminal command was not
                        // entered, so native teardown has not started and a
                        // temporarily unavailable filesystem can be retried
                        // without losing the live engine or storage actor.
                        self.retryable_shutdown_failure(ack);
                        return;
                    }
                    StoreShutdownOutcome::Unclean => {
                        // Durability/actor ownership crossed an uncertain
                        // terminal boundary. Continued browsing cannot be
                        // reconciled safely; let the outer watchdog provide
                        // bounded process teardown and report non-zero.
                        eprintln!(
                            "shutdown: storage actor termination was not proven before the deadline"
                        );
                        if let Some(reads) = &self.store_reads {
                            reads.stop();
                        }
                        self.shutdown_result = Some(ShutdownOutcome::Unclean);
                        let _ = ack.send(ShutdownOutcome::Unclean);
                        return;
                    }
                }
                if let Some(reads) = &self.store_reads {
                    reads.stop();
                }
                let (native_done, native_wait) = sync_channel(1);
                self.engine.shutdown(Box::new(move |clean| {
                    let _ = native_done.send(clean);
                }));
                let native_budget = deadline.saturating_duration_since(std::time::Instant::now());
                let clean = native_wait.recv_timeout(native_budget).unwrap_or(false);
                if !clean {
                    // Teardown may already have destroyed some or all native
                    // state. Never resume the actor after this point; process
                    // exit is the final bounded cleanup and the next startup
                    // refuses stale private data it cannot remove.
                    eprintln!(
                        "shutdown: native cleanup did not acknowledge cleanly; forcing process exit"
                    );
                }
                let outcome = if clean {
                    ShutdownOutcome::Clean
                } else {
                    ShutdownOutcome::Unclean
                };
                self.shutdown_result = Some(outcome);
                let _ = ack.send(outcome);
            }
        }
    }

    fn retryable_shutdown_failure(&mut self, ack: SyncSender<ShutdownOutcome>) {
        let recovered = self
            .self_queue
            .as_ref()
            .map(CommandQueue::reopen_after_failed_shutdown)
            .unwrap_or_default();
        // These callbacks were truthfully rejected after the barrier, but the
        // browser is about to resume. Fold their bounded latest state into the
        // actor before acknowledging failure or accepting newly dispatched
        // work.
        for command in recovered {
            self.handle(command);
        }
        if let Some(queue) = &self.self_queue {
            // A timer wake removes its entry before trying to enter the actor.
            // If it raced the shutdown barrier it was truthfully rejected as
            // sealed, so explicitly restore every still-live exact reveal
            // obligation when the retryable barrier reopens. Both maps remain
            // bounded to one entry per logical item.
            let now = std::time::Instant::now();
            for (id, pending) in &self.pending_presentations {
                queue.schedule_presentation(*id, pending.navigation, now, pending.hard_deadline);
            }
        }
        if let Some(reads) = &self.store_reads {
            reads.resume();
        }
        // A failed terminal storage admission may have invalidated pending
        // presentation reads. Restart only currently visible leaves (at most
        // the split ceiling), never all restored tabs.
        let visible = self
            .pane_tree()
            .map(|tree| tree.tabs())
            .or_else(|| {
                self.windows
                    .focused()
                    .and_then(|window| window.active)
                    .map(|id| vec![id])
            })
            .unwrap_or_default();
        for id in visible {
            self.maybe_discover_favicon(id);
        }
        let _ = ack.send(ShutdownOutcome::RetryableFailure);
    }

    fn handle_operation(&mut self, command: Command) -> OperationDisposition {
        match command {
            Command::Open => self.operation_open(),
            Command::Activate(id) => self.operation_activate(id),
            Command::Close(id) => self.operation_close(id),
            Command::Navigate { id, input } => self.operation_navigate(id, input),
            Command::Reload(id) => self.operation_reload(id),
            Command::GoBack(id) => self.operation_history(id, false),
            Command::GoForward(id) => self.operation_history(id, true),
            Command::SplitWith { other, axis } => self.operation_split(other, axis),
            Command::Unsplit => self.operation_unsplit(),
            Command::DropTab { id, x, y } => self.operation_drop_tab(id, x, y),
            Command::DividerRelease { x, y } => self.operation_divider_release(x.zip(y)),
            Command::Run(id) => self.operation_run_command(&id),
            Command::OpenUrl(input) => self.operation_open_url(input),
            Command::SetAppSetting { key, value } => self.operation_set_app_setting(key, value),
            _ => operation_result(
                OperationOutcome::Rejected,
                OperationReason::UnsupportedCommand,
            ),
        }
    }

    fn operation_open(&mut self) -> OperationDisposition {
        if self.windows.focused().is_none() {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        }
        let Some((_id, effects)) = self.open_tab_with_id() else {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::ItemLimitReached,
            );
        };
        mutation_result(self.commit(effects))
    }

    fn operation_open_url(&mut self, input: String) -> OperationDisposition {
        if navigation::classify(&input).is_none() {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput);
        }
        if self.windows.focused().is_none() {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        }
        let Some((id, mut effects)) = self.open_tab_with_id() else {
            // Reaching the bounded item limit must not repurpose and navigate
            // the caller's existing active tab.
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::ItemLimitReached,
            );
        };
        effects.extend(self.items.navigate(id, &input));
        mutation_result(self.commit(effects))
    }

    fn operation_activate(&mut self, id: ItemId) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        let active = self.windows.focused().and_then(|window| window.active);
        let has_view = self.items.tab(id).is_some_and(TabState::has_view);
        let discard_closing = matches!(
            self.discard_probes.get(&id),
            Some(PendingDiscardProbe::Closing { .. })
        );
        if active == Some(id) && has_view && !discard_closing {
            self.touch(id);
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        let effects = self.focus_tab(id);
        let native = self.commit(effects);
        if discard_closing {
            operation_result(
                OperationOutcome::Deferred,
                OperationReason::DiscardCompletionPending,
            )
        } else {
            mutation_result(native)
        }
    }

    fn operation_close(&mut self, id: ItemId) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        let discard_closing = matches!(
            self.discard_probes.get(&id),
            Some(PendingDiscardProbe::Closing { .. })
        );
        let native = self.close(id);
        if discard_closing && !native.rejected {
            operation_result(
                OperationOutcome::Deferred,
                OperationReason::DiscardCompletionPending,
            )
        } else {
            mutation_result(native)
        }
    }

    fn operation_navigate(&mut self, id: ItemId, input: String) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        if navigation::classify(&input).is_none() {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput);
        }
        if let Some(PendingDiscardProbe::Closing {
            recreate,
            deferred_navigation,
            ..
        }) = self.discard_probes.get_mut(&id)
        {
            *recreate = true;
            *deferred_navigation = Some(input);
            return operation_result(
                OperationOutcome::Deferred,
                OperationReason::DiscardCompletionPending,
            );
        }
        self.cancel_discard_probe(id);
        let effects = self.items.navigate(id, &input);
        if effects.is_empty() {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput);
        }
        mutation_result(self.commit(effects))
    }

    fn operation_reload(&mut self, id: ItemId) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        if self.recreate_after_inflight_discard(id) {
            return operation_result(
                OperationOutcome::Deferred,
                OperationReason::DiscardCompletionPending,
            );
        }
        self.cancel_discard_probe(id);
        if !self.items.tab(id).is_some_and(TabState::has_view) {
            let effects = self.items.ensure_view(id);
            if effects.is_empty() {
                return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
            }
            return mutation_result(self.commit(effects));
        }
        let mut native = NativeWork::default();
        native.record(self.engine.reload(id));
        mutation_result(native)
    }

    fn operation_history(&mut self, id: ItemId, forward: bool) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        let Some(tab) = self.items.tab(id) else {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        };
        let available = if forward {
            tab.can_go_forward
        } else {
            tab.can_go_back
        };
        if !available || !tab.has_view() {
            return operation_result(OperationOutcome::NoOp, OperationReason::HistoryUnavailable);
        }
        if self.recreate_after_inflight_discard(id) {
            // Native back/forward history belongs to the controller that is
            // already closing and cannot be reconstructed by URL alone.
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::HistoryUnavailable,
            );
        }
        self.cancel_discard_probe(id);
        let admission = if forward {
            self.engine.go_forward(id)
        } else {
            self.engine.go_back(id)
        };
        let mut native = NativeWork::default();
        native.record(admission);
        mutation_result(native)
    }

    fn operation_split(&mut self, other: ItemId, axis: Axis) -> OperationDisposition {
        let Some((active, profile, space)) = self
            .windows
            .focused()
            .and_then(|win| win.active.map(|active| (active, win.profile, win.space)))
        else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        if !self.item_in_scope(active, profile, space) || !self.item_in_scope(other, profile, space)
        {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        if active == other {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        let Some(mut tree) = self.pane_tree() else {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::LayoutUnavailable,
            );
        };
        if !self.pane_in_scope(&tree, profile, space) || tree.tabs().len() >= MAX_VISIBLE_PANES {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::LayoutUnavailable,
            );
        }
        if tree.contains(other) {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        let closing = [active, other].into_iter().any(|id| {
            matches!(
                self.discard_probes.get(&id),
                Some(PendingDiscardProbe::Closing { .. })
            )
        });
        let effects = self.items.ensure_view(other);
        let mut native = self.apply(effects);
        if !self.items.tab(other).is_some_and(TabState::has_view) {
            // A synchronous create-dispatch refusal already rolled back the
            // optimistic view bit. Do not install a split whose new leaf can
            // never be represented by the native layout admitted below.
            return mutation_result(native);
        }
        self.touch(other);
        if !tree.split(active, other, axis, false) {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        if let Some(win) = self.windows.focused_mut() {
            win.splits = Some(tree);
        }
        native.merge(self.commit(Vec::new()));
        if closing && !native.rejected {
            operation_result(
                OperationOutcome::Deferred,
                OperationReason::DiscardCompletionPending,
            )
        } else {
            mutation_result(native)
        }
    }

    fn operation_unsplit(&mut self) -> OperationDisposition {
        let Some(win) = self.windows.focused_mut() else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        if win.splits.take().is_none() {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        mutation_result(self.commit(Vec::new()))
    }

    fn operation_drop_tab(&mut self, id: ItemId, x: f64, y: f64) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        let Some(window) = self.windows.focused().map(|window| window.id) else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        let Some(drop) = self.resolve_drop(x, y) else {
            let _ = self.engine.set_drop_indicator(window, None);
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        };
        let result = self.apply_drop(drop.tab, id, drop.edge);
        let _ = self.engine.set_drop_indicator(window, None);
        result
    }

    fn operation_divider_release(
        &mut self,
        final_pointer: Option<(f64, f64)>,
    ) -> OperationDisposition {
        if self.divider.is_none() {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        if let Some((x, y)) = final_pointer {
            self.divider_drag(x, y);
        }
        if self.divider.take().is_none() {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        self.schedule_persist();
        operation_result(OperationOutcome::Applied, OperationReason::MutationApplied)
    }

    fn operation_set_app_setting(&mut self, key: String, value: String) -> OperationDisposition {
        if key != "appearance" || !matches!(value.as_str(), "system" | "light" | "dark") {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput);
        }
        if !self.store.set_app_setting(key, value.clone()) {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::StoreAdmissionRejected,
            );
        }
        // This projection is downstream of truthful store-queue admission.
        // The desktop composition root applies native theme state from this
        // signal, never optimistically from the IPC request itself.
        (self.emit)(Projection::UiCommand(format!("theme.{value}")));
        operation_result(
            OperationOutcome::Deferred,
            OperationReason::StoreWorkPending,
        )
    }

    fn operation_run_command(&mut self, id: &str) -> OperationDisposition {
        let active = self.windows.focused().and_then(|window| window.active);
        match id {
            "tab.new" => self.operation_open(),
            "tab.close" => active.map_or_else(
                || operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow),
                |id| self.operation_close(id),
            ),
            "tab.next" => self.operation_cycle_tab(1),
            "tab.previous" => self.operation_cycle_tab(-1),
            "nav.back" => active.map_or_else(
                || operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow),
                |id| self.operation_history(id, false),
            ),
            "nav.forward" => active.map_or_else(
                || operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow),
                |id| self.operation_history(id, true),
            ),
            "nav.reload" => active.map_or_else(
                || operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow),
                |id| self.operation_reload(id),
            ),
            "nav.stop" => active.map_or_else(
                || operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow),
                |id| {
                    let mut native = NativeWork::default();
                    native.record(self.engine.stop(id));
                    mutation_result(native)
                },
            ),
            "zoom.in" => self.operation_adjust_zoom(Some(0.1)),
            "zoom.out" => self.operation_adjust_zoom(Some(-0.1)),
            "zoom.reset" => self.operation_adjust_zoom(None),
            "url.focus" => {
                (self.emit)(Projection::UiCommand("url.focus".into()));
                operation_result(OperationOutcome::Applied, OperationReason::MutationApplied)
            }
            _ => operation_result(
                OperationOutcome::Rejected,
                OperationReason::UnsupportedCommand,
            ),
        }
    }

    fn operation_cycle_tab(&mut self, step: isize) -> OperationDisposition {
        let Some(win) = self.windows.focused() else {
            return operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow);
        };
        let Some(active) = win.active else {
            return operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow);
        };
        let tabs = self.today_tabs(win.space);
        let Some(position) = tabs.iter().position(|id| *id == active) else {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        };
        if tabs.len() < 2 {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        let next = (position as isize + step).rem_euclid(tabs.len() as isize) as usize;
        self.operation_activate(tabs[next])
    }

    fn operation_adjust_zoom(&mut self, delta: Option<f64>) -> OperationDisposition {
        let Some(active) = self.windows.focused().and_then(|window| window.active) else {
            return operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow);
        };
        let Some(settled) = self.items.tab(active).map(|tab| tab.zoom) else {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        };
        let current = self
            .pending_zooms
            .get(&active)
            .map_or(settled, |pending| pending.desired_scale);
        let zoom = match delta {
            Some(delta) => (current + delta).clamp(0.3, 3.0),
            None => 1.0,
        };
        if (zoom - current).abs() < f64::EPSILON {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        // `Items.zoom` is durable authoritative state. Keep the responsive
        // desired value in a separate bounded map until the exact native view
        // generation reports what it actually applied; otherwise any
        // unrelated session save could persist an optimistic lie.
        let admission = self.request_zoom(active, zoom);
        let mut native = NativeWork::default();
        native.record(admission);
        mutation_result(native)
    }

    fn request_zoom(&mut self, id: ItemId, desired_scale: f64) -> NativeDispatch {
        let Some(next) = self.next_zoom_request.checked_add(1) else {
            // Reusing an identity could let a late result settle a newer
            // request. Saturation permanently rejects new zoom work.
            return NativeDispatch::Rejected;
        };
        self.next_zoom_request = next;
        let request = ZoomRequestId(next);
        let admission = self.engine.zoom(id, desired_scale, request);
        if admission == NativeDispatch::Scheduled {
            self.pending_zooms.insert(
                id,
                PendingZoom {
                    request,
                    desired_scale,
                },
            );
        }
        admission
    }

    fn on_zoom_settled(
        &mut self,
        id: ItemId,
        request: ZoomRequestId,
        applied_scale: f64,
        succeeded: bool,
    ) {
        let Some(pending) = self.pending_zooms.get(&id).copied() else {
            return;
        };
        if pending.request != request {
            // A newest-per-item native settlement contains the cumulative
            // applied scale. Older results cannot settle a newer desired
            // value and are deliberately ignored.
            return;
        }
        // This exact terminal result owns the obligation even when its native
        // payload is malformed. Retire it before validation so a corrupt or
        // incompatible engine response cannot leave all future zoom input
        // based on a value that will never settle.
        self.pending_zooms.remove(&id);
        if !applied_scale.is_finite() || !(0.3..=3.0).contains(&applied_scale) {
            eprintln!("engine: rejected malformed native zoom settlement");
            return;
        }
        if !succeeded {
            eprintln!("engine: native zoom request was not applied");
        } else if (applied_scale - pending.desired_scale).abs() > f64::EPSILON {
            eprintln!("engine: native zoom settled at an unexpected scale");
        }
        let Some(previous) = self.items.tab(id).map(|tab| tab.zoom) else {
            return;
        };
        if (previous - applied_scale).abs() <= f64::EPSILON {
            return;
        }
        self.items.set_zoom(id, applied_scale);
        self.schedule_persist();
        self.project_tab(id);
    }

    fn begin_profile_deletion(
        &mut self,
        profile: ProfileId,
        operation_id: Option<String>,
    ) -> OperationDisposition {
        // One foreground deletion at a time keeps durable/native ownership
        // unambiguous. Crash-recovered journal rows may coexist, but a new
        // deletion waits until those obligations are resolved.
        if !self.profile_deletions.is_empty() {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::ProfileDeletionInProgress,
            );
        }
        let Some(filtered) = self.filtered_session_for_profile_deletion(profile) else {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::ProfileDeletionPolicyRejected,
            );
        };
        let deadline = std::time::Instant::now() + PROFILE_DELETION_STORE_TIMEOUT;
        let authorization_revision = self.session_revision;
        let outcome = self
            .store
            .authorize_profile_deletion(profile, filtered, deadline);
        match outcome {
            ProfileDeletionAuthorizeOutcome::Authorized => {
                self.profile_deletions.insert(
                    profile,
                    ProfileDeletionState::new(
                        ProfileDeletionPhase::NativeReady,
                        operation_id,
                        authorization_revision,
                    ),
                );
                self.apply_profile_tombstone(profile);
                operation_result(
                    OperationOutcome::Deferred,
                    OperationReason::NativeWorkPending,
                )
            }
            ProfileDeletionAuthorizeOutcome::AlreadyAuthorized => {
                self.profile_deletions.insert(
                    profile,
                    ProfileDeletionState::new(
                        ProfileDeletionPhase::ResolveAuthorizedJournal,
                        operation_id,
                        authorization_revision,
                    ),
                );
                self.apply_profile_tombstone(profile);
                operation_result(
                    OperationOutcome::Deferred,
                    OperationReason::StoreWorkPending,
                )
            }
            ProfileDeletionAuthorizeOutcome::OutcomeUnknown => {
                self.profile_deletions.insert(
                    profile,
                    ProfileDeletionState::new(
                        ProfileDeletionPhase::Authorizing {
                            may_reauthorize: false,
                        },
                        operation_id,
                        authorization_revision,
                    ),
                );
                operation_result(
                    OperationOutcome::Deferred,
                    OperationReason::StoreWorkPending,
                )
            }
            ProfileDeletionAuthorizeOutcome::NotRegistered
            | ProfileDeletionAuthorizeOutcome::SessionConflict
            | ProfileDeletionAuthorizeOutcome::InvalidSession
            | ProfileDeletionAuthorizeOutcome::NotAdmitted
            | ProfileDeletionAuthorizeOutcome::Failed => operation_result(
                OperationOutcome::Rejected,
                OperationReason::StoreAdmissionRejected,
            ),
        }
    }

    /// Builds the exact canonical post-removal state without mutating any
    /// aggregate. Only an inactive, non-default, non-private profile may pass.
    fn filtered_session_for_profile_deletion(
        &self,
        profile: ProfileId,
    ) -> Option<session::SessionState> {
        let candidate = self.profiles.get(profile)?;
        if !self.bootstrapped
            || candidate.kind != ProfileKind::Named
            || self
                .windows
                .focused()
                .is_some_and(|window| window.profile == profile)
            || self
                .profiles
                .iter()
                .filter(|profile| profile.kind != ProfileKind::Incognito)
                .count()
                <= 1
        {
            return None;
        }

        let window = self.windows.focused();
        let mut filtered = session::snapshot(
            &self.profiles,
            &self.spaces,
            &self.items,
            window.map(|window| window.space),
            window.and_then(|window| window.active),
            window.and_then(|window| window.splits.as_ref()),
        );
        if !filtered
            .profiles
            .iter()
            .any(|candidate| candidate.id == profile)
        {
            return None;
        }
        let removed_spaces: std::collections::HashSet<SpaceId> = filtered
            .spaces
            .iter()
            .filter(|space| space.profile == profile)
            .map(|space| space.id)
            .collect();
        filtered
            .profiles
            .retain(|candidate| candidate.id != profile);
        filtered.spaces.retain(|space| space.profile != profile);
        filtered.items.retain(|item| match item.placement {
            Placement::Favorites {
                profile: item_profile,
            } => item_profile != profile,
            Placement::Space { space, .. } => !removed_spaces.contains(&space),
        });

        (session::canonicalize(filtered.clone()) == filtered).then_some(filtered)
    }

    /// Applies only the logical half of an already-durable authorization.
    /// Native `Close` effects are deliberately suppressed: the engine's
    /// profile erasure owns closure and exact retirement of every view.
    fn apply_profile_tombstone(&mut self, profile: ProfileId) {
        let mut favicon_items: std::collections::HashSet<ItemId> = self
            .icon_attempts
            .iter()
            .filter_map(|(id, attempt)| (attempt.profile == profile).then_some(*id))
            .collect();
        favicon_items.extend(
            self.icon_load_completion_pending
                .iter()
                .filter_map(|(id, (item_profile, _))| (*item_profile == profile).then_some(*id)),
        );
        favicon_items.extend(
            self.favicon_store_reads
                .iter()
                .filter_map(|(id, pending)| (pending.profile == profile).then_some(*id)),
        );
        for id in favicon_items {
            self.cancel_favicon_attempt(id);
        }
        let discard_items: Vec<ItemId> = self
            .discard_probes
            .keys()
            .copied()
            .filter(|id| self.profile_of_item(*id) == Some(profile))
            .collect();
        for id in discard_items {
            self.discard_probes.remove(&id);
            if let Some(queue) = &self.self_queue {
                queue.cancel_discard_probe(id);
            }
        }

        let removed_spaces = self.spaces.remove_for_profile(profile);
        let _native_erasure_owns_close = self.items.remove_for_profile(&removed_spaces);
        self.profiles.remove(profile);

        self.recent.retain(|id| self.items.tab(*id).is_some());
        self.last_focus
            .retain(|id, _| self.items.tab(*id).is_some());
        self.last_visits
            .retain(|id, _| self.items.tab(*id).is_some());
        self.dormant_sent.retain(|id| self.items.tab(*id).is_some());
        self.discard_protected_until
            .retain(|id, _| self.items.tab(*id).is_some());
        self.crashes.retain(|id, _| self.items.tab(*id).is_some());
        self.crash_presentations
            .retain(|id| self.items.tab(*id).is_some());
        self.icons_checked
            .retain(|(item_profile, _)| *item_profile != profile);
        self.icon_values
            .retain(|(item_profile, _), _| *item_profile != profile);
        self.icon_cache_order
            .retain(|(item_profile, _)| *item_profile != profile);
        if self
            .pending_favicon_batch
            .as_ref()
            .is_some_and(|pending| pending.profile == profile)
        {
            self.pending_favicon_batch = None;
        }
        if self
            .pending_search
            .as_ref()
            .is_some_and(|pending| pending.profile == profile)
        {
            self.pending_search = None;
        }

        // The authorization transaction already published the exact session.
        // An older debounce must never later overwrite that barrier.
        self.persist_first_dirty = None;
        self.url_checkpoint_dirty.clear();
        self.last_url_checkpoint = std::time::Instant::now();
        if let Some(queue) = &self.self_queue {
            queue.cancel_persist();
        }
        self.project_items();
    }

    fn drive_profile_deletion(&mut self, profile: ProfileId) {
        enum Action {
            ReconcileAuthorization(bool),
            ResolveAuthorizedJournal,
            StartNative(u64),
            Finalize,
            None,
        }

        let action = {
            let Some(state) = self.profile_deletions.get_mut(&profile) else {
                return;
            };
            match state.phase {
                ProfileDeletionPhase::Authorizing {
                    may_reauthorize, ..
                } => Action::ReconcileAuthorization(may_reauthorize),
                ProfileDeletionPhase::ResolveAuthorizedJournal => Action::ResolveAuthorizedJournal,
                ProfileDeletionPhase::NativeReady => {
                    state.attempt_generation = state.attempt_generation.wrapping_add(1);
                    if state.attempt_generation == 0 {
                        state.attempt_generation = 1;
                    }
                    let attempt = state.attempt_generation;
                    state.phase = ProfileDeletionPhase::NativeInFlight { attempt };
                    state.retry_exponent = 0;
                    Action::StartNative(attempt)
                }
                ProfileDeletionPhase::NativeInFlight { .. } => Action::None,
                ProfileDeletionPhase::FinalizeReady => Action::Finalize,
            }
        };
        if let Some(queue) = &self.self_queue {
            queue.cancel_profile_deletion(profile);
        }
        match action {
            Action::ReconcileAuthorization(may_reauthorize) => {
                self.reconcile_profile_authorization(profile, may_reauthorize)
            }
            Action::ResolveAuthorizedJournal => self.resolve_authorized_journal(profile),
            Action::StartNative(attempt) => self.start_native_profile_erasure(profile, attempt),
            Action::Finalize => self.finalize_profile_deletion(profile),
            Action::None => {}
        }
    }

    fn reconcile_profile_authorization(&mut self, profile: ProfileId, may_reauthorize: bool) {
        match self.store.pending_profile_deletions() {
            ProfileDeletionLoad::Loaded(pending) => {
                if let Some(deletion) = pending
                    .into_iter()
                    .find(|deletion| deletion.profile == profile)
                {
                    self.authorization_is_durable(profile, deletion.native_erasure_verified);
                    return;
                }
                if !may_reauthorize {
                    self.schedule_profile_deletion_retry(profile);
                    return;
                }
                // The ordered absence proves the previous attempt did not
                // publish an authorization. Rebuild from current aggregates so
                // survivor mutations accepted since that attempt cannot be
                // overwritten by a stale snapshot, and re-run all policy and
                // canonical-form checks at the same boundary.
                let Some(filtered) = self.filtered_session_for_profile_deletion(profile) else {
                    self.finish_profile_deletion_operation(
                        profile,
                        OperationOutcome::Rejected,
                        OperationReason::ProfileDeletionPolicyRejected,
                    );
                    return;
                };
                if let Some(state) = self.profile_deletions.get_mut(&profile) {
                    state.authorization_revision = self.session_revision;
                }
                let outcome = self.store.authorize_profile_deletion(
                    profile,
                    filtered,
                    std::time::Instant::now() + PROFILE_DELETION_STORE_TIMEOUT,
                );
                self.handle_profile_authorization_outcome(profile, outcome);
            }
            ProfileDeletionLoad::Failed => self.schedule_profile_deletion_retry(profile),
        }
    }

    fn handle_profile_authorization_outcome(
        &mut self,
        profile: ProfileId,
        outcome: ProfileDeletionAuthorizeOutcome,
    ) {
        match outcome {
            ProfileDeletionAuthorizeOutcome::Authorized => {
                self.authorization_is_durable(profile, false)
            }
            ProfileDeletionAuthorizeOutcome::AlreadyAuthorized => {
                self.apply_profile_tombstone(profile);
                if let Some(state) = self.profile_deletions.get_mut(&profile) {
                    state.phase = ProfileDeletionPhase::ResolveAuthorizedJournal;
                    state.retry_exponent = 0;
                }
                self.resolve_authorized_journal(profile);
            }
            ProfileDeletionAuthorizeOutcome::OutcomeUnknown => {
                // The ordered journal query is the only legal arbiter after a
                // deadline. Absence is not used to guess success; the next
                // bounded retry may re-run the idempotent authorization.
                self.reconcile_profile_authorization(profile, false);
            }
            ProfileDeletionAuthorizeOutcome::NotRegistered
            | ProfileDeletionAuthorizeOutcome::SessionConflict
            | ProfileDeletionAuthorizeOutcome::InvalidSession
            | ProfileDeletionAuthorizeOutcome::NotAdmitted
            | ProfileDeletionAuthorizeOutcome::Failed => self.finish_profile_deletion_operation(
                profile,
                OperationOutcome::Rejected,
                OperationReason::StoreAdmissionRejected,
            ),
        }
    }

    fn resolve_authorized_journal(&mut self, profile: ProfileId) {
        match self.store.pending_profile_deletions() {
            ProfileDeletionLoad::Loaded(pending) => {
                if let Some(deletion) = pending
                    .into_iter()
                    .find(|deletion| deletion.profile == profile)
                {
                    self.authorization_is_durable(profile, deletion.native_erasure_verified);
                } else {
                    // `AlreadyAuthorized` and a missing journal row conflict.
                    // Keep the logical tombstone and retry; never recreate the
                    // profile or infer that local deletion completed without
                    // an in-process native proof.
                    self.schedule_profile_deletion_retry(profile);
                }
            }
            ProfileDeletionLoad::Failed => self.schedule_profile_deletion_retry(profile),
        }
    }

    fn authorization_is_durable(&mut self, profile: ProfileId, native_verified: bool) {
        let survivor_state_changed = self
            .profile_deletions
            .get(&profile)
            .is_some_and(|state| state.authorization_revision != self.session_revision);
        self.apply_profile_tombstone(profile);
        if let Some(state) = self.profile_deletions.get_mut(&profile) {
            state.phase = if native_verified {
                ProfileDeletionPhase::FinalizeReady
            } else {
                ProfileDeletionPhase::NativeReady
            };
            state.retry_exponent = 0;
        }
        if survivor_state_changed {
            // The deletion authorization durably represents its own exact
            // snapshot, but newer survivor state was accepted while the reply
            // was uncertain. `apply_profile_tombstone` cancels the pre-barrier
            // debounce; establish a new post-barrier durability deadline
            // without pretending this scheduling pass is another mutation.
            self.schedule_current_session_persist();
        }
        self.drive_profile_deletion(profile);
    }

    fn start_native_profile_erasure(&mut self, profile: ProfileId, attempt: u64) {
        let inbox = self.profile_deletion_inbox.clone();
        let wake = self.self_queue.as_ref().map(|queue| CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        });
        self.engine.erase_profile_data(
            profile,
            Box::new(move |outcome| {
                let mut pending = inbox
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if pending.len() < zephium_core::session::MAX_SESSION_PROFILES
                    || pending.contains_key(&profile)
                {
                    pending.insert(profile, (attempt, outcome));
                }
                drop(pending);
                if let Some(wake) = wake {
                    let _ = wake.dispatch(Command::ProfileDeletionReady(profile));
                }
            }),
        );
        // Some engines can reject or complete synchronously. Drain only after
        // the caller has installed the exact in-flight generation.
        self.consume_profile_deletion_outcome(profile);
    }

    fn drain_profile_deletion_inbox(&mut self) {
        let profiles: Vec<ProfileId> = self
            .profile_deletion_inbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .copied()
            .collect();
        for profile in profiles {
            self.consume_profile_deletion_outcome(profile);
        }
    }

    fn consume_profile_deletion_outcome(&mut self, profile: ProfileId) {
        let pending = self
            .profile_deletion_inbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&profile);
        let Some((attempt, outcome)) = pending else {
            return;
        };
        let exact = self.profile_deletions.get(&profile).is_some_and(|state| {
            matches!(
                state.phase,
                ProfileDeletionPhase::NativeInFlight {
                    attempt: expected
                } if expected == attempt
            )
        });
        if !exact {
            return;
        }
        match outcome {
            ProfileDataErasureOutcome::Verified => {
                if let Some(state) = self.profile_deletions.get_mut(&profile) {
                    state.phase = ProfileDeletionPhase::FinalizeReady;
                    state.retry_exponent = 0;
                }
                self.drive_profile_deletion(profile);
            }
            ProfileDataErasureOutcome::Failed | ProfileDataErasureOutcome::TimedOut => {
                if let Some(state) = self.profile_deletions.get_mut(&profile) {
                    state.phase = ProfileDeletionPhase::NativeReady;
                }
                self.schedule_profile_deletion_retry(profile);
            }
        }
    }

    fn finalize_profile_deletion(&mut self, profile: ProfileId) {
        let deadline = self
            .profile_deletion_batch_deadline
            .unwrap_or_else(|| std::time::Instant::now() + PROFILE_DELETION_STORE_TIMEOUT);
        if std::time::Instant::now() >= deadline {
            self.schedule_profile_deletion_retry(profile);
            return;
        }
        let outcome = self.store.finalize_profile_deletion(profile, deadline);
        match outcome {
            ProfileDeletionFinalizeOutcome::Completed => self.finish_profile_deletion_operation(
                profile,
                OperationOutcome::Applied,
                OperationReason::ProfileDeletionCompleted,
            ),
            ProfileDeletionFinalizeOutcome::NotAuthorized
            | ProfileDeletionFinalizeOutcome::NotAdmitted
            | ProfileDeletionFinalizeOutcome::OutcomeUnknown
            | ProfileDeletionFinalizeOutcome::Failed => {
                if std::time::Instant::now() >= deadline {
                    self.schedule_profile_deletion_retry(profile);
                } else {
                    self.reconcile_profile_finalization(profile);
                }
            }
        }
    }

    fn reconcile_profile_finalization(&mut self, profile: ProfileId) {
        match self.store.pending_profile_deletions() {
            ProfileDeletionLoad::Loaded(pending) => {
                if pending.iter().any(|deletion| deletion.profile == profile) {
                    if let Some(state) = self.profile_deletions.get_mut(&profile) {
                        state.phase = ProfileDeletionPhase::FinalizeReady;
                    }
                    self.schedule_profile_deletion_retry(profile);
                } else {
                    // Journal removal is the store's last ordered step, so an
                    // authoritative absence after native proof establishes
                    // both phases complete even if the RPC reply was lost.
                    self.finish_profile_deletion_operation(
                        profile,
                        OperationOutcome::Applied,
                        OperationReason::ProfileDeletionCompleted,
                    );
                }
            }
            ProfileDeletionLoad::Failed => {
                self.schedule_profile_deletion_retry(profile);
            }
        }
    }

    fn finish_profile_deletion_operation(
        &mut self,
        profile: ProfileId,
        outcome: OperationOutcome,
        reason: OperationReason,
    ) {
        if outcome == OperationOutcome::Applied
            && reason == OperationReason::ProfileDeletionCompleted
        {
            self.degraded_storage_profiles.remove(&profile);
        }
        let operation_id = self
            .profile_deletions
            .remove(&profile)
            .and_then(|state| state.operation_id);
        if let Some(queue) = &self.self_queue {
            queue.cancel_profile_deletion(profile);
        }
        self.profile_deletion_inbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&profile);
        if let Some(operation_id) = operation_id {
            (self.emit)(Projection::OperationProcessed(OperationDisposition {
                operation_id,
                outcome,
                reason,
            }));
        }
    }

    fn schedule_profile_deletion_retry(&mut self, profile: ProfileId) {
        let Some(state) = self.profile_deletions.get_mut(&profile) else {
            return;
        };
        if let ProfileDeletionPhase::Authorizing {
            may_reauthorize, ..
        } = &mut state.phase
        {
            *may_reauthorize = true;
        }
        state.retry_generation = state.retry_generation.wrapping_add(1);
        if state.retry_generation == 0 {
            state.retry_generation = 1;
        }
        let generation = state.retry_generation;
        let multiplier = 1_u32 << u32::from(state.retry_exponent.min(7));
        let delay = PROFILE_DELETION_RETRY_BASE
            .saturating_mul(multiplier)
            .min(PROFILE_DELETION_RETRY_MAX);
        state.retry_exponent = state.retry_exponent.saturating_add(1).min(7);
        if let Some(queue) = &self.self_queue {
            queue.schedule_profile_deletion(profile, generation, std::time::Instant::now() + delay);
        }
    }

    fn bootstrap(&mut self) {
        // Runtime update state is independent of session recovery and chrome
        // reloads. Querying the sticky engine state also repairs a callback
        // that arrived before the shell callback ingress was installed.
        if !self.reconcile_runtime_restart_requirement() {
            self.project_runtime_status();
        }
        // The chrome re-invokes bootstrap whenever its webview reloads (dev
        // HMR, crash recovery); state and native surfaces must not be rebuilt.
        if self.windows.focused().is_some() {
            let _ = self.relayout();
            self.project_items();
            return;
        }
        let pending_deletions = match self.store.pending_profile_deletions() {
            ProfileDeletionLoad::Loaded(pending) => pending,
            ProfileDeletionLoad::Failed => {
                eprintln!(
                    "bootstrap: profile deletion journal is unavailable; refusing initialization"
                );
                return;
            }
        };
        let mut journal_profiles = std::collections::HashSet::new();
        if pending_deletions.len() > zephium_core::session::MAX_SESSION_PROFILES
            || pending_deletions
                .iter()
                .any(|deletion| !journal_profiles.insert(deletion.profile))
        {
            eprintln!("bootstrap: profile deletion journal exceeds its unique bounded cohort");
            return;
        }
        let mut active_item = None;
        let mut active_space = None;
        let mut splits = None;
        let mut session_absent = false;
        match self.store.load_session() {
            SessionLoad::Loaded(state) => {
                let restored = session::restore(state);
                self.profiles = restored.profiles;
                self.spaces = restored.spaces;
                self.items = restored.items;
                active_item = restored.active_item;
                active_space = restored.active_space;
                splits = restored.splits;
            }
            SessionLoad::LoadedWithDegradedProfiles { state, profiles } => {
                let session_profiles: std::collections::HashSet<_> =
                    state.profiles.iter().map(|profile| profile.id).collect();
                let mut degraded = std::collections::HashSet::new();
                if profiles.is_empty()
                    || profiles.len() > zephium_core::session::MAX_SESSION_PROFILES
                    || profiles.iter().any(|profile| {
                        !session_profiles.contains(profile) || !degraded.insert(*profile)
                    })
                {
                    eprintln!(
                        "bootstrap: ancillary-profile degradation report is invalid; refusing initialization"
                    );
                    return;
                }
                let mut labels: Vec<_> = degraded.iter().map(ToString::to_string).collect();
                labels.sort_unstable();
                eprintln!(
                    "bootstrap: ancillary history/favicon storage is disabled for profiles: {}",
                    labels.join(",")
                );
                self.degraded_storage_profiles = degraded;

                let restored = session::restore(state);
                self.profiles = restored.profiles;
                self.spaces = restored.spaces;
                self.items = restored.items;
                active_item = restored.active_item;
                active_space = restored.active_space;
                splits = restored.splits;
            }
            SessionLoad::Absent => session_absent = true,
            SessionLoad::RecoveryRequired { reason } => {
                // The store has preserved the exact authoritative bytes and
                // entered a sticky read-only mode. Do not construct first-run
                // state or let a later shutdown overwrite recoverable data.
                eprintln!("bootstrap: explicit session recovery required: {reason}");
                return;
            }
            SessionLoad::Failed => {
                // Never convert corruption or I/O failure into first-run
                // state. Since bootstrapped remains false, every persistence
                // path also refuses to overwrite the recoverable snapshot.
                eprintln!("bootstrap: session storage is unavailable; refusing initialization");
                return;
            }
        }
        if (!pending_deletions.is_empty() && session_absent)
            || pending_deletions
                .iter()
                .any(|deletion| self.profiles.get(deletion.profile).is_some())
        {
            // The store contract atomically removes every journaled profile
            // from a still-authoritative session. Any overlap/absence means
            // the two durable facts disagree; creating views would guess.
            eprintln!(
                "bootstrap: profile deletion journal conflicts with the authoritative session"
            );
            return;
        }

        for PendingProfileDeletion {
            profile,
            native_erasure_verified,
        } in pending_deletions
        {
            let phase = if native_erasure_verified {
                ProfileDeletionPhase::FinalizeReady
            } else {
                ProfileDeletionPhase::NativeReady
            };
            self.profile_deletions.insert(
                profile,
                ProfileDeletionState::new(phase, None, self.session_revision),
            );
            // Normally no aggregate row remains. This defensive cleanup also
            // cancels any runtime-only work restored by a future caller.
            self.apply_profile_tombstone(profile);
        }
        let recovered_deletions: Vec<ProfileId> = self.profile_deletions.keys().copied().collect();
        let recovery_deadline = std::time::Instant::now() + PROFILE_DELETION_STORE_TIMEOUT;
        self.profile_deletion_batch_deadline = Some(recovery_deadline);
        for profile in recovered_deletions {
            if std::time::Instant::now() < recovery_deadline {
                self.drive_profile_deletion(profile);
            } else {
                self.schedule_profile_deletion_retry(profile);
            }
        }
        self.profile_deletion_batch_deadline = None;
        if splits
            .as_ref()
            .is_some_and(|tree| tree.tabs().len() > MAX_VISIBLE_PANES)
        {
            eprintln!("session: discarded oversized split layout");
            splits = None;
        }

        let Some(space) = active_space
            .or_else(|| {
                self.profiles
                    .default_profile()
                    .and_then(|p| self.spaces.first_for(p))
            })
            .or_else(|| self.create_default_space())
        else {
            eprintln!("bootstrap: bounded profile/space aggregate cannot create first-run state");
            return;
        };
        let Some(profile) = self.spaces.get(space).map(|space| space.profile) else {
            eprintln!("bootstrap: selected space has no authoritative profile ownership");
            return;
        };

        // Restore is semi-trusted input. Recheck focus and every pane against
        // the actual window selected after fallback, before creating any
        // native view or assigning a renderer partition.
        active_item = active_item.filter(|id| self.item_in_scope(*id, profile, space));
        splits = splits.filter(|tree| self.pane_in_scope(tree, profile, space));

        let window = self
            .windows
            .create(WindowKind::Main, profile, space, self.pending_size);

        let mut fx = Vec::new();
        if let Some(tree) = splits {
            for leaf in tree.tabs() {
                fx.extend(self.items.ensure_view(leaf));
                self.touch(leaf);
            }
            if let Some(win) = self.windows.get_mut(window) {
                win.splits = Some(tree);
            }
        }
        match active_item.or_else(|| self.today_tabs(space).first().copied()) {
            Some(item) => fx.extend(self.focus_tab(item)),
            None => fx.extend(self.open_tab()),
        }
        // Restored/hibernated tabs may not create a native view and therefore
        // have no URL/load callback to trigger favicon hydration. Load the
        // active space's already-decoded fixed rasters in one bounded store
        // request before the first sidebar projection.
        self.hydrate_favicon_cache(profile, space);
        self.apply(fx);
        let _ = self.relayout();
        self.project_items();
        self.bootstrapped = true;
    }

    fn create_default_space(&mut self) -> Option<SpaceId> {
        let mut created_profile = None;
        let profile = if let Some(profile) = self.profiles.default_profile() {
            profile
        } else {
            let mut inserted = None;
            for _ in 0..8 {
                let id = ProfileId::generate();
                if self.profiles.insert(Profile {
                    id,
                    name: "Personal".into(),
                    kind: ProfileKind::Default,
                }) {
                    inserted = Some(id);
                    break;
                }
            }
            let id = inserted?;
            created_profile = Some(id);
            id
        };
        for _ in 0..8 {
            let id = SpaceId::generate();
            if self.spaces.insert(Space {
                id,
                profile,
                name: "Space".into(),
            }) {
                return Some(id);
            }
        }
        if let Some(profile) = created_profile {
            self.profiles.remove(profile);
        }
        None
    }

    fn open_tab(&mut self) -> Vec<Effect> {
        self.open_tab_with_id()
            .map(|(_, effects)| effects)
            .unwrap_or_default()
    }

    fn open_tab_with_id(&mut self) -> Option<(ItemId, Vec<Effect>)> {
        let win = self.windows.focused_mut()?;
        let space = win.space;
        let id = ItemId::generate();
        if !self.items.insert_tab(
            id,
            Placement::Space {
                space,
                section: SpaceSection::Today,
            },
        ) {
            return None;
        }
        Some((id, self.focus_tab(id)))
    }

    /// Moves window focus to `id`: lifecycle bookkeeping plus a lazy view.
    fn focus_tab(&mut self, id: ItemId) -> Vec<Effect> {
        let Some((profile, space)) = self.windows.focused().map(|win| (win.profile, win.space))
        else {
            return Vec::new();
        };
        if !self.item_in_scope(id, profile, space) {
            return Vec::new();
        }
        let Some(win) = self.windows.focused_mut() else {
            return Vec::new();
        };
        let prev = win.active.replace(id).filter(|p| *p != id);
        if let Some(prev) = prev {
            self.items.set_lifecycle(prev, Lifecycle::Inactive);
        }
        self.items.set_lifecycle(id, Lifecycle::Active);
        self.recent.retain(|r| *r != id);
        self.recent.push(id);
        self.touch(id);
        self.items.ensure_view(id)
    }

    fn close(&mut self, id: ItemId) -> NativeWork {
        if !self.item_in_focused_scope(id) {
            return NativeWork::default();
        }
        self.cancel_pending_presentation(id);
        self.cancel_favicon_attempt(id);
        if matches!(
            self.discard_probes.get(&id),
            Some(PendingDiscardProbe::Closing { .. })
        ) {
            // Native destruction is already admitted. Clear the logical view
            // before `remove` so it does not issue a second same-id close.
            self.items.mark_view_discarded(id);
            self.discard_probes.remove(&id);
            if let Some(queue) = &self.self_queue {
                queue.cancel_discard_probe(id);
            }
        } else {
            self.cancel_discard_probe(id);
        }
        let Some(win) = self.windows.focused_mut() else {
            return NativeWork::default();
        };
        if let Some(tree) = win.splits.take() {
            win.splits = tree.remove(id);
        }
        let space = win.space;
        let was_active = win.active == Some(id);
        if was_active {
            win.active = None;
        }
        let tabs_before = self.today_tabs(space);
        let pos = tabs_before.iter().position(|x| *x == id);
        let mut fx = self.items.remove(id);
        if was_active {
            let tabs = self.today_tabs(space);
            if let (Some(pos), false) = (pos, tabs.is_empty()) {
                fx.extend(self.focus_tab(tabs[pos.min(tabs.len() - 1)]));
            }
        }
        self.commit(fx)
    }

    /// Removes a failed native leaf from the retained split immediately. A
    /// create failure may arrive after `operation_split`/`apply_drop` has
    /// committed its optimistic topology; retaining that leaf would let a
    /// later single-tab retry silently resurrect the old group.
    fn collapse_failed_split_leaf(&mut self, id: ItemId) -> bool {
        let Some((tree, failed_was_active)) = self.windows.focused().and_then(|window| {
            window
                .splits
                .as_ref()
                .filter(|tree| tree.contains(id))
                .cloned()
                .map(|tree| (tree, window.active == Some(id)))
        }) else {
            return false;
        };
        self.divider = None;
        let remaining = tree.remove(id);
        let replacement = failed_was_active
            .then(|| {
                remaining.as_ref().and_then(|tree| {
                    tree.tabs().into_iter().find(|candidate| {
                        self.items.tab(*candidate).is_some_and(TabState::has_view)
                    })
                })
            })
            .flatten();
        if let Some(window) = self.windows.focused_mut() {
            window.splits = remaining;
            if replacement.is_some() {
                // `focus_tab` owns lifecycle/recent bookkeeping below.
                window.active = None;
            }
        }
        if let Some(replacement) = replacement {
            // The replacement was selected from live split leaves, so this
            // normally emits no construction effect. Keep the invariant even
            // if future lifecycle states add another recoverable resident.
            let effects = self.focus_tab(replacement);
            let _ = self.apply(effects);
        }
        true
    }

    fn apply_drop(&mut self, target: ItemId, dropped: ItemId, edge: Edge) -> OperationDisposition {
        let Some((profile, space)) = self.windows.focused().map(|win| (win.profile, win.space))
        else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        if !self.item_in_scope(target, profile, space)
            || !self.item_in_scope(dropped, profile, space)
        {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        if target == dropped {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        let Some(mut tree) = self.pane_tree() else {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::LayoutUnavailable,
            );
        };
        if !self.pane_in_scope(&tree, profile, space) || tree.tabs().len() >= MAX_VISIBLE_PANES {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::LayoutUnavailable,
            );
        }
        if tree.contains(dropped) {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        let effects = self.items.ensure_view(dropped);
        let mut native = self.apply(effects);
        if !self.items.tab(dropped).is_some_and(TabState::has_view) {
            // Match `operation_split`: synchronous native refusal must leave
            // the previously rendered topology authoritative.
            return mutation_result(native);
        }
        self.touch(dropped);
        if !tree.split(target, dropped, edge.axis(), edge.before()) {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        if let Some(win) = self.windows.focused_mut() {
            win.splits = Some(tree);
        }
        native.merge(self.commit(Vec::new()));
        mutation_result(native)
    }

    /// `x`/`y` are window coords (the desktop layer normalizes per platform).
    fn resolve_drop(&self, x: f64, y: f64) -> Option<split::Drop> {
        let win = self.windows.focused()?;
        let tree = self.pane_tree()?;
        let region =
            layout::compute(win.size, win.mode, win.metrics, self.present(&tree)).content?;
        let local = Rect::new(0.0, 0.0, region.width, region.height);
        split::drop_target(&tree, local, win.metrics.gap, x - region.x, y - region.y)
    }

    fn track_pending_presentation(
        &mut self,
        id: ItemId,
        navigation: NavigationPresentationId,
        url: String,
        now: std::time::Instant,
    ) -> PendingPresentation {
        let candidate_hard = now
            .checked_add(PRESENTATION_ADMISSION_HARD_LIMIT)
            .unwrap_or(now);
        let pending = match self.pending_presentations.entry(id) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                // A hidden view may commit another navigation before the
                // first one presents. Advance the exact identity but preserve
                // the original absolute cap so navigation churn cannot keep
                // trusted backing over executable content indefinitely.
                let hard_deadline = entry.get().hard_deadline.min(candidate_hard);
                let admission_rejections = entry.get().admission_rejections;
                let same_fact = entry.get().navigation == navigation && entry.get().url == url;
                let pending = PendingPresentation {
                    navigation,
                    url,
                    hard_deadline,
                    admission_rejections,
                    chrome_applied: same_fact && entry.get().chrome_applied,
                    chrome_request_in_flight: same_fact && entry.get().chrome_request_in_flight,
                };
                entry.insert(pending.clone());
                pending
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let pending = PendingPresentation {
                    navigation,
                    url,
                    hard_deadline: candidate_hard,
                    admission_rejections: 0,
                    chrome_applied: false,
                    chrome_request_in_flight: false,
                };
                entry.insert(pending.clone());
                pending
            }
        };
        pending
    }

    fn cancel_pending_presentation(&mut self, id: ItemId) {
        self.pending_presentations.remove(&id);
        self.presented_navigations.remove(&id);
        self.deferred_first_content_layout.remove(&id);
        if let Ok(mut revisions) = self.last_tab_projection_revision.try_borrow_mut() {
            revisions.remove(&id);
        }
        if let Some(queue) = &self.self_queue {
            queue.cancel_presentation(id);
        }
    }

    fn cancel_exact_pending_presentation(
        &mut self,
        id: ItemId,
        navigation: NavigationPresentationId,
    ) -> bool {
        if !self
            .pending_presentations
            .get(&id)
            .is_some_and(|pending| pending.navigation == navigation)
        {
            return false;
        }
        self.pending_presentations.remove(&id);
        if let Some(queue) = &self.self_queue {
            queue.cancel_presentation(id);
        }
        true
    }

    fn presentation_matches_current_url(&self, id: ItemId, pending: &PendingPresentation) -> bool {
        self.items.tab(id).is_some_and(|tab| {
            tab.has_view()
                && tab
                    .url
                    .as_ref()
                    .is_some_and(|url| url.as_str() == pending.url)
        })
    }

    fn schedule_exact_presentation_retry(
        &mut self,
        id: ItemId,
        navigation: NavigationPresentationId,
        reason: &'static str,
    ) {
        let now = std::time::Instant::now();
        let Some(current) = self
            .pending_presentations
            .get_mut(&id)
            .filter(|current| current.navigation == navigation)
        else {
            return;
        };
        current.chrome_request_in_flight = false;
        current.admission_rejections = current.admission_rejections.saturating_add(1);
        if now >= current.hard_deadline
            || current.admission_rejections >= MAX_PRESENTATION_ADMISSION_REJECTIONS
        {
            self.fail_exact_pending_presentation(id, navigation, reason);
            return;
        }
        let retry_index = usize::from(current.admission_rejections.saturating_sub(1))
            .min(PRESENTATION_ADMISSION_RETRY_DELAYS.len() - 1);
        let wake = now
            .checked_add(PRESENTATION_ADMISSION_RETRY_DELAYS[retry_index])
            .unwrap_or(now)
            .min(current.hard_deadline);
        let hard_deadline = current.hard_deadline;
        if let Some(queue) = &self.self_queue {
            queue.retry_presentation(id, navigation, wake, hard_deadline);
        } else {
            self.fail_exact_pending_presentation(
                id,
                navigation,
                "privileged presentation retry queue is unavailable",
            );
        }
    }

    /// Applies the exact tab projection inside privileged chrome and waits for
    /// its eval callback before allowing the raw child to reveal. This method
    /// never blocks the shell actor or a native UI thread.
    fn request_chrome_presentation(&mut self, id: ItemId, navigation: NavigationPresentationId) {
        let Some(pending) = self
            .pending_presentations
            .get(&id)
            .cloned()
            .filter(|pending| pending.navigation == navigation)
        else {
            return;
        };
        if !self.presentation_matches_current_url(id, &pending) {
            self.cancel_exact_pending_presentation(id, navigation);
            return;
        }
        if pending.chrome_applied {
            let _ = self.admit_pending_presentation(id, navigation);
            return;
        }
        if pending.chrome_request_in_flight {
            return;
        }
        if std::time::Instant::now() >= pending.hard_deadline {
            self.fail_exact_pending_presentation(
                id,
                navigation,
                "privileged chrome did not verify the committed URL before its deadline",
            );
            return;
        }

        let Some(tab) = self.items.tab(id) else {
            self.cancel_exact_pending_presentation(id, navigation);
            return;
        };
        let projection =
            self.presentation_tab_view(id, tab, self.favicon_key(tab, self.profile_of_item(id)));
        let projection_revision = projection.projection_revision.clone();
        self.record_tab_projection_revision(id, &projection_revision);
        let active = self.windows.focused().and_then(|window| window.active);
        let Some(queue) = self.self_queue.clone() else {
            let synchronous_projection = projection.clone();
            let dispatch = self.chrome.apply_tab_for_presentation(
                ChromePresentation {
                    id,
                    navigation,
                    url: pending.url.clone(),
                    tab: projection,
                    active,
                },
                Box::new(|_| {}),
            );
            if dispatch == ChromePresentationDispatch::Applied {
                // A synchronous adapter already applied this exact revision;
                // mirror it to non-chrome projection observers afterward.
                // Privileged chrome ignores the equal-revision duplicate.
                (self.emit)(Projection::Tab(synchronous_projection));
                self.on_chrome_presentation_applied(
                    id,
                    navigation,
                    pending.url,
                    active,
                    projection_revision,
                    true,
                );
            } else {
                self.fail_exact_pending_presentation(
                    id,
                    navigation,
                    "asynchronous privileged presentation has no callback ingress",
                );
            }
            return;
        };
        let callback = CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        };
        let callback_url = pending.url.clone();
        let callback_active = active;
        let callback_projection_revision = projection_revision.clone();
        if let Some(current) = self
            .pending_presentations
            .get_mut(&id)
            .filter(|current| current.navigation == navigation)
        {
            current.chrome_request_in_flight = true;
        }
        let synchronous_projection = projection.clone();
        let dispatch = self.chrome.apply_tab_for_presentation(
            ChromePresentation {
                id,
                navigation,
                url: pending.url.clone(),
                tab: projection,
                active,
            },
            Box::new(move |applied| {
                let _ = callback.dispatch(Command::ChromePresentationApplied {
                    id,
                    navigation,
                    url: callback_url,
                    active: callback_active,
                    projection_revision: callback_projection_revision,
                    applied,
                });
            }),
        );
        match dispatch {
            ChromePresentationDispatch::Applied => {
                (self.emit)(Projection::Tab(synchronous_projection));
                self.on_chrome_presentation_applied(
                    id,
                    navigation,
                    pending.url,
                    active,
                    projection_revision,
                    true,
                );
            }
            ChromePresentationDispatch::Scheduled => {
                // Callback loss and a full callback queue are both covered by
                // this exact timer. A retry can duplicate an in-flight eval,
                // but stale results cannot pass actor revalidation.
                let retry_index = usize::from(pending.admission_rejections)
                    .min(PRESENTATION_ADMISSION_RETRY_DELAYS.len() - 1);
                let now = std::time::Instant::now();
                let wake = now
                    .checked_add(PRESENTATION_ADMISSION_RETRY_DELAYS[retry_index])
                    .unwrap_or(now)
                    .min(pending.hard_deadline);
                queue.retry_presentation(id, navigation, wake, pending.hard_deadline);
            }
            ChromePresentationDispatch::Rejected => self.schedule_exact_presentation_retry(
                id,
                navigation,
                "privileged chrome presentation admission remained unavailable",
            ),
        }
    }

    fn on_chrome_presentation_applied(
        &mut self,
        id: ItemId,
        navigation: NavigationPresentationId,
        url: String,
        active: Option<ItemId>,
        projection_revision: String,
        applied: bool,
    ) {
        let pending_exact = self.pending_presentations.get(&id).is_some_and(|pending| {
            pending.navigation == navigation
                && pending.url == url
                && self.presentation_matches_current_url(id, pending)
        });
        if !pending_exact {
            return;
        }
        let revision_exact = self
            .last_tab_projection_revision
            .try_borrow()
            .is_ok_and(|revisions| revisions.get(&id) == Some(&projection_revision));
        let active_exact = self.windows.focused().and_then(|window| window.active) == active;
        if !revision_exact || !active_exact {
            // The original exact timer remains armed. It will clear the
            // in-flight bit and issue a newer projection; this stale callback
            // cannot reveal content under a later masked/full tab state.
            return;
        }
        if !applied {
            self.schedule_exact_presentation_retry(
                id,
                navigation,
                "privileged chrome rejected the exact committed URL projection",
            );
            return;
        }
        if let Some(pending) = self.pending_presentations.get_mut(&id) {
            pending.chrome_applied = true;
            pending.chrome_request_in_flight = false;
        }
        if let Some(queue) = &self.self_queue {
            queue.cancel_presentation(id);
        }
        let _ = self.admit_pending_presentation(id, navigation);
    }

    /// Retains the exact hidden-document obligation until the native reveal
    /// task has actually entered its owning UI queue. Public dispatcher
    /// overload is recoverable, so a rejection gets one bounded per-tab timer
    /// with capped backoff instead of becoming a permanently hidden page.
    fn admit_pending_presentation(
        &mut self,
        id: ItemId,
        navigation: NavigationPresentationId,
    ) -> NativeDispatch {
        let Some(pending) = self
            .pending_presentations
            .get(&id)
            .cloned()
            .filter(|pending| pending.navigation == navigation)
        else {
            return NativeDispatch::Rejected;
        };

        // Actor ordering is not enough: the privileged renderer must have
        // completed and verified the exact revision-bearing tab projection.
        if !pending.chrome_applied || !self.presentation_matches_current_url(id, &pending) {
            self.cancel_exact_pending_presentation(id, navigation);
            return NativeDispatch::Rejected;
        }

        // On a fresh tab the exact eval is also the first projection allowed
        // to remove the real New Tab surface. Queue privileged frame + raw
        // content geometry only afterward, on the same ordered native
        // dispatcher used by presentation. Refusal retains the hidden exact
        // obligation and follows the ordinary bounded retry path.
        if self.deferred_first_content_layout.contains(&id) {
            let layout = self.relayout();
            if layout != NativeDispatch::Scheduled {
                self.schedule_exact_presentation_retry(
                    id,
                    navigation,
                    "first content layout was not admitted after privileged chrome verification",
                );
                return layout;
            }
        }

        let admission = self.engine.present_navigation(id, navigation);
        match admission {
            NativeDispatch::Scheduled => {
                // The native task is generation/epoch checked again when it
                // executes. Clear only the same obligation: a re-entrant newer
                // commit must retain its own hidden-document gate and timer.
                self.presented_navigations
                    .insert(id, (navigation, pending.url.clone()));
                self.deferred_first_content_layout.remove(&id);
                self.cancel_exact_pending_presentation(id, navigation);
            }
            NativeDispatch::Rejected => {
                self.schedule_exact_presentation_retry(
                    id,
                    navigation,
                    "native presentation dispatcher remained unavailable",
                );
            }
            NativeDispatch::Unsupported => {
                // Emitting Pending/Ready proves this exact native surface is
                // gated. Claiming the matching acknowledgement is unsupported
                // is therefore a lifecycle invariant failure, not permission
                // to forget a potentially permanently hidden document.
                self.fail_exact_pending_presentation(
                    id,
                    navigation,
                    "gated native presentation acknowledgement is unsupported",
                );
            }
        }
        admission
    }

    fn fail_exact_pending_presentation(
        &mut self,
        id: ItemId,
        navigation: NavigationPresentationId,
        reason: &'static str,
    ) {
        if !self
            .pending_presentations
            .get(&id)
            .is_some_and(|pending| pending.navigation == navigation)
        {
            return;
        }
        eprintln!("engine: {reason}; retiring exact hidden view");
        // `close` revokes the engine's item token synchronously before its
        // native cleanup is dispatched. If that dispatch is itself rejected,
        // the production engine seals native authority and invokes its fatal
        // lifecycle callback; either way this id cannot silently keep using
        // the hidden generation after the shell marks it failed.
        let _ = self.engine.close(id);
        self.on_view_creation_failed(id);
    }

    fn on_view_creation_failed(&mut self, id: ItemId) {
        self.pending_zooms.remove(&id);
        self.cancel_pending_presentation(id);
        self.cancel_discard_probe(id);
        self.items.view_creation_failed(id);
        let split_collapsed = self.collapse_failed_split_leaf(id);
        if split_collapsed {
            self.schedule_persist();
        }
        let _ = self.relayout();
        if split_collapsed {
            self.project_items();
        } else {
            self.project_tab(id);
        }
    }

    fn on_presentation_fallback(
        &mut self,
        id: ItemId,
        navigation: NavigationPresentationId,
        hard_deadline: std::time::Instant,
    ) {
        let Some(pending) = self.pending_presentations.get(&id).cloned() else {
            return;
        };
        if pending.navigation != navigation || pending.hard_deadline != hard_deadline {
            // The timer wake escaped before a newer navigation/ready/close
            // replaced this exact obligation.
            return;
        }
        let Some(_tab) = self.items.tab(id).filter(|tab| tab.has_view()) else {
            self.cancel_pending_presentation(id);
            return;
        };
        let now = std::time::Instant::now();
        if now >= hard_deadline {
            self.fail_exact_pending_presentation(
                id,
                navigation,
                "exact presentation barrier exceeded its hard deadline",
            );
            return;
        }
        if pending.chrome_request_in_flight {
            if let Some(current) = self.pending_presentations.get_mut(&id) {
                current.chrome_request_in_flight = false;
                current.admission_rejections = current.admission_rejections.saturating_add(1);
                if current.admission_rejections >= MAX_PRESENTATION_ADMISSION_REJECTIONS {
                    self.fail_exact_pending_presentation(
                        id,
                        navigation,
                        "privileged chrome presentation callback remained unavailable",
                    );
                    return;
                }
            }
        }
        // Page loading state is irrelevant. Re-drive the missing chrome or
        // native admission, but never turn the deadline into a timeout reveal.
        self.request_chrome_presentation(id, navigation);
    }

    fn on_presentation_fact(
        &mut self,
        id: ItemId,
        navigation: NavigationPresentationId,
        url: String,
    ) {
        let Ok(url) = url::Url::parse(&url) else {
            eprintln!("engine: rejected malformed native presentation URL");
            return;
        };
        if !navigation::is_allowed(&url)
            || !self.items.tab(id).is_some_and(|tab| {
                tab.has_view() && tab.url.as_ref().is_some_and(|current| current == &url)
            })
        {
            // A newer URL may already have replaced this coalesced native
            // fact. Do not cancel its obligation and never acknowledge the
            // stale token against whichever URL happens to be current.
            return;
        }
        if self
            .presented_navigations
            .get(&id)
            .is_some_and(|(presented, presented_url)| {
                *presented == navigation && presented_url == url.as_str()
            })
        {
            // Native Finished re-emits the same exact fact so a coalesced-away
            // Pending can still present. Once admission was already proven,
            // the duplicate is an idempotent no-op rather than another native
            // visibility/layout pass.
            return;
        }
        if self
            .pending_presentations
            .get(&id)
            .is_some_and(|pending| pending.navigation != navigation)
        {
            // Replace the retained shell obligation and its one timer as one
            // logical transition. An already-escaped old wake remains safe:
            // `on_presentation_fallback` checks both token and hard deadline.
            if let Some(queue) = &self.self_queue {
                queue.cancel_presentation(id);
            }
        }
        let pending = self.track_pending_presentation(
            id,
            navigation,
            url.as_str().to_owned(),
            std::time::Instant::now(),
        );
        self.request_chrome_presentation(id, pending.navigation);
    }

    fn on_engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::RuntimeRestartRequired => {
                if !self.runtime_restart_required {
                    self.runtime_restart_required = true;
                    self.project_runtime_status();
                }
            }
            EngineEvent::SplitChanged { window, tree } => {
                // Native divider drags may update ratios only. Never let a
                // stale or malformed callback mutate topology, swap tabs, or
                // inject a non-finite layout value or cross-window item into
                // Rust-owned state.
                let valid = self
                    .windows
                    .get(window)
                    .and_then(|win| {
                        win.splits.as_ref().map(|current| {
                            valid_native_split_update(current, &tree)
                                && self.pane_in_scope(current, win.profile, win.space)
                                && self.pane_in_scope(&tree, win.profile, win.space)
                        })
                    })
                    .unwrap_or(false);
                if valid {
                    if let Some(win) = self.windows.get_mut(window) {
                        win.splits = Some(tree);
                    }
                    self.schedule_persist();
                    let _ = self.relayout();
                } else {
                    eprintln!("engine: rejected invalid native split tree");
                    let _ = self.relayout();
                }
            }
            EngineEvent::NavState {
                id,
                can_go_back,
                can_go_forward,
            } => {
                self.items.set_nav_flags(id, can_go_back, can_go_forward);
                self.project_tab(id);
            }
            EngineEvent::NewWindowRequested { id, url } => self.open_linked_tab(id, &url),
            EngineEvent::FaviconPixels { id, page_url, rgba } => {
                self.favicon_pixels(id, &page_url, rgba);
            }
            EngineEvent::DiscardSafety {
                id,
                probe,
                can_discard,
            } => self.on_discard_safety(id, probe, can_discard),
            EngineEvent::ViewDiscarded { id, profile, probe } => {
                self.on_view_discarded(id, profile, probe)
            }
            EngineEvent::PermissionRequested { .. } => {}
            EngineEvent::DownloadRequested { .. } => {}
            EngineEvent::PresentationPending {
                id,
                navigation,
                url,
            }
            | EngineEvent::PresentationReady {
                id,
                navigation,
                url,
            } => self.on_presentation_fact(id, navigation, url),
            EngineEvent::NavigationFailed { id, request } => {
                // Only the matching latest intent is affected. The displayed
                // URL was never changed optimistically, so a stale native
                // rejection cannot roll chrome or persistence forward or back.
                if self.items.navigation_failed(id, request) {
                    self.project_tab(id);
                }
            }
            EngineEvent::ZoomSettled {
                id,
                request,
                applied_scale,
                succeeded,
            } => self.on_zoom_settled(id, request, applied_scale, succeeded),
            EngineEvent::NativeActionFailed { id, action } => {
                if self.items.tab(id).is_some_and(TabState::has_view) {
                    let action = match action {
                        NativeAction::Reload => "reload",
                        NativeAction::GoBack => "back",
                        NativeAction::GoForward => "forward",
                    };
                    // The engine separately re-observes authoritative
                    // source/history. Keep this diagnostic bounded and never
                    // include a page-derived URL or native error string.
                    eprintln!("engine: native {action} action failed");
                }
            }
            EngineEvent::ViewCreationFailed { id } => {
                self.on_view_creation_failed(id);
            }
            EngineEvent::ProfileProcessExited { profile, ids } => {
                self.on_profile_process_exit(profile, ids)
            }
            EngineEvent::Crashed { id } => self.on_crashed(id),
            EngineEvent::Captured { .. } => {}
            EngineEvent::HtmlExtracted { .. } => {}
            EngineEvent::ShortcutPressed { .. } => {}
            EngineEvent::TitleChanged { id, title } => {
                self.crash_presentations.remove(&id);
                self.items.set_title(id, title);
                self.project_tab(id);
            }
            EngineEvent::LoadingChanged { id, loading } => {
                if loading {
                    self.cancel_discard_probe(id);
                    self.crash_presentations.remove(&id);
                }
                self.items.set_loading(id, loading);
                self.project_tab(id);
                if !loading {
                    // The first URL observation may precede the renderer's
                    // asynchronous image decode. Poll immediately at load
                    // completion as well as through the bounded timer.
                    self.favicon_load_completed(id);
                    self.engine.warm_spare(self.partition_of(id));
                }
            }
            EngineEvent::UrlChanged { id, url } => {
                self.cancel_discard_probe(id);
                let Ok(committed_url) = url::Url::parse(&url) else {
                    eprintln!("engine: rejected invalid or unknown committed URL event");
                    return;
                };
                if !navigation::is_allowed(&committed_url) {
                    eprintln!("engine: rejected invalid or unknown committed URL event");
                    return;
                }
                let first_committed_url = self.items.tab(id).is_some_and(|tab| tab.url.is_none());
                let replace_stale_title = self
                    .items
                    .tab(id)
                    .and_then(|tab| tab.url.as_ref())
                    .is_none_or(|previous| !same_browser_origin(previous, &committed_url));
                if !self.items.set_committed_url(id, committed_url.clone()) {
                    eprintln!("engine: rejected invalid or unknown committed URL event");
                    return;
                }
                if replace_stale_title {
                    // Browser chrome may be projected before the new document
                    // publishes a title. Never carry a prior origin's trusted
                    // label across the exact URL acknowledgement that unlocks
                    // presentation; use a neutral URL-derived label meanwhile.
                    self.items
                        .set_title(id, neutral_title_for_url(&committed_url));
                }
                self.maybe_discover_favicon(id);
                // History is attributed to the profile that owns the item,
                // not the focused window; incognito profiles never record.
                let recording = self.profile_of_item(id).filter(|p| {
                    self.profiles
                        .get(*p)
                        .is_some_and(|x| x.kind != ProfileKind::Incognito)
                });
                if let Some(profile) = recording.filter(|_| self.should_record_visit(id, &url)) {
                    let title = self
                        .items
                        .tab(id)
                        .map(|t| t.title.clone())
                        .unwrap_or_default();
                    self.store.record_visit(profile, url, title);
                }
                self.schedule_url_checkpoint(id);
                if first_committed_url {
                    // Keep the real privileged New Tab projection and native
                    // frame until the exact presentation eval replaces it.
                    // All generic projections remain URL-free for this item,
                    // so they cannot create an empty gap ahead of that eval.
                    self.deferred_first_content_layout.insert(id);
                } else {
                    self.project_tab(id);
                }
            }
        }
    }

    fn search(&mut self, query: &str) {
        let Some((profile, space)) = self
            .windows
            .focused()
            .map(|window| (window.profile, window.space))
        else {
            return;
        };
        let q = query.trim();
        let needle = q.to_lowercase();
        let tabs = self.today_tabs(space);
        let mut results = Vec::new();
        self.search_generation = self.search_generation.wrapping_add(1);
        if self.search_generation == 0 {
            self.search_generation = 1;
        }
        let generation = self.search_generation;
        self.pending_search = None;

        if q.is_empty() {
            results.extend(
                tabs.iter()
                    .filter_map(|id| {
                        self.items
                            .tab(*id)
                            .map(|t| tab_result(*id, t, self.favicon_key(t, Some(profile))))
                    })
                    .take(8),
            );
        } else {
            let matched: Vec<(ItemId, &TabState)> = tabs
                .iter()
                .filter_map(|id| self.items.tab(*id).map(|t| (*id, t)))
                .filter(|(_, t)| {
                    t.title.to_lowercase().contains(&needle)
                        || t.url
                            .as_ref()
                            .is_some_and(|u| u.as_str().to_lowercase().contains(&needle))
                })
                .take(4)
                .collect();
            let open_urls: std::collections::HashSet<String> = matched
                .iter()
                .filter_map(|(_, t)| t.url.as_ref().map(ToString::to_string))
                .collect();
            results.extend(
                matched
                    .iter()
                    .map(|(id, t)| tab_result(*id, t, self.favicon_key(t, Some(profile)))),
            );

            if let Some(url) = navigation::classify(q) {
                if navigation::is_query(q) {
                    results.push(SearchResult {
                        kind: "search".into(),
                        title: format!("Search for \"{q}\""),
                        detail: "DuckDuckGo".into(),
                        favicon: None,
                        action: SearchAction::OpenUrl {
                            url: url.to_string(),
                        },
                    });
                } else {
                    results.push(SearchResult {
                        kind: "url".into(),
                        title: format!("Open {url}"),
                        detail: "New Tab".into(),
                        favicon: self.favicon_key_for_url(profile, url.as_str()),
                        action: SearchAction::OpenUrl {
                            url: url.to_string(),
                        },
                    });
                }
            }

            results.extend(
                commands::REGISTRY
                    .iter()
                    .filter(|c| c.id != "launcher.toggle")
                    .filter(|c| c.title.to_lowercase().contains(&needle))
                    .take(3)
                    .map(|c| SearchResult {
                        kind: "command".into(),
                        title: c.title.into(),
                        detail: c.accelerator.unwrap_or_default().into(),
                        favicon: None,
                        action: SearchAction::RunCommand { id: c.id.into() },
                    }),
            );

            results.truncate(10);

            // Project local tab/URL/command matches immediately. History is
            // presentation-only and arrives asynchronously; the exact query
            // generation below prevents a slow old result replacing newer UI.
            (self.emit)(Projection::Search(SearchResults {
                query: query.into(),
                results: results.clone(),
            }));
            if q.len() > MAX_ASYNC_SEARCH_QUERY_BYTES {
                return;
            }
            self.pending_search = Some(PendingSearch {
                generation,
                profile,
                lookup_query: q.to_owned(),
                display_query: query.to_owned(),
                open_urls,
                base_results: results,
            });
            if let Some(reads) = &self.store_reads {
                if !reads.request_history(generation, profile, q.to_owned()) {
                    self.pending_search = None;
                }
            } else {
                #[cfg(test)]
                {
                    let hits = self.store.search_history(profile, q, 6);
                    self.on_store_read(StoreReadResult::History {
                        generation,
                        profile,
                        query: q.to_owned(),
                        hits,
                    });
                }
                #[cfg(not(test))]
                {
                    // Production construction always installs the reader.
                    // Keep this defensive branch nonblocking if a future
                    // internal constructor violates that invariant.
                    self.pending_search = None;
                }
            }
            return;
        }

        (self.emit)(Projection::Search(SearchResults {
            query: query.into(),
            results,
        }));
    }

    fn item_origin(&self, id: ItemId) -> Option<(ProfileId, String)> {
        let profile = self.profile_of_item(id)?;
        let origin = self
            .items
            .tab(id)
            .and_then(|t| t.url.as_ref())
            .and_then(origin_of)?;
        Some((profile, origin))
    }

    fn hydrate_favicon_cache(&mut self, profile: ProfileId, space: SpaceId) {
        let mut requested = std::collections::HashSet::new();
        let mut origins = Vec::new();
        for id in self.today_tabs(space) {
            let Some(origin) = self
                .items
                .tab(id)
                .and_then(|tab| tab.url.as_ref())
                .and_then(origin_of)
            else {
                continue;
            };
            if requested.insert(origin.clone()) {
                origins.push(origin);
                if origins.len() == MAX_FAVICON_BATCH_ORIGINS {
                    break;
                }
            }
        }
        if origins.is_empty() {
            return;
        }
        self.favicon_batch_generation = self.favicon_batch_generation.wrapping_add(1);
        if self.favicon_batch_generation == 0 {
            self.favicon_batch_generation = 1;
        }
        let generation = self.favicon_batch_generation;
        self.pending_favicon_batch = Some(PendingFaviconBatch {
            generation,
            profile,
            space,
            requested,
        });
        if let Some(reads) = &self.store_reads {
            if !reads.request_favicon_batch(generation, profile, space, origins) {
                self.pending_favicon_batch = None;
            }
        } else {
            #[cfg(test)]
            {
                let rasters = self.store.favicon_rasters(profile, &origins);
                self.on_store_read(StoreReadResult::FaviconBatch {
                    generation,
                    profile,
                    space,
                    origins,
                    rasters,
                });
            }
            #[cfg(not(test))]
            {
                self.pending_favicon_batch = None;
            }
        }
    }

    fn clear_pending_store_reads(&mut self) {
        self.pending_search = None;
        self.pending_favicon_batch = None;
        self.favicon_store_reads.clear();
    }

    fn on_store_read(&mut self, result: StoreReadResult) {
        match result {
            StoreReadResult::History {
                generation,
                profile,
                query,
                hits,
            } => self.on_history_read(generation, profile, query, hits),
            StoreReadResult::Favicon {
                generation,
                id,
                profile,
                origin,
                rgba,
            } => self.on_favicon_read(generation, id, profile, origin, rgba),
            StoreReadResult::FaviconBatch {
                generation,
                profile,
                space,
                origins,
                rasters,
            } => self.on_favicon_batch_read(generation, profile, space, origins, rasters),
        }
    }

    fn on_history_read(
        &mut self,
        generation: u64,
        profile: ProfileId,
        query: String,
        hits: Vec<zephium_core::ports::store::HistoryHit>,
    ) {
        let exact = self.pending_search.as_ref().is_some_and(|pending| {
            pending.generation == generation
                && pending.profile == profile
                && pending.lookup_query == query
        });
        if !exact {
            return;
        }
        let Some(pending) = self.pending_search.take() else {
            return;
        };
        if self
            .windows
            .focused()
            .is_none_or(|window| window.profile != profile)
        {
            return;
        }

        // Treat even our own store adapter as a serialization boundary: cap
        // cardinality, revalidate URLs, sanitize titles, and deduplicate before
        // values reach privileged launcher markup.
        let mut results = pending.base_results;
        let mut seen = pending.open_urls;
        for hit in hits.into_iter().take(6) {
            if !navigation::is_allowed_str(&hit.url) || !seen.insert(hit.url.clone()) {
                continue;
            }
            let title = zephium_core::item::sanitize_page_title(&hit.title);
            results.push(SearchResult {
                kind: "history".into(),
                title: if title.is_empty() {
                    hit.url.clone()
                } else {
                    title
                },
                detail: hit.url.clone(),
                favicon: self.favicon_key_for_url(profile, &hit.url),
                action: SearchAction::OpenUrl { url: hit.url },
            });
        }
        results.truncate(10);
        (self.emit)(Projection::Search(SearchResults {
            query: pending.display_query,
            results,
        }));
    }

    fn on_favicon_read(
        &mut self,
        generation: u64,
        id: ItemId,
        profile: ProfileId,
        origin: String,
        rgba: Option<Vec<u8>>,
    ) {
        let exact = self.favicon_store_reads.get(&id).is_some_and(|pending| {
            pending.generation == generation
                && pending.profile == profile
                && pending.origin == origin
        });
        if !exact {
            return;
        }
        self.favicon_store_reads.remove(&id);
        if let Some(queue) = &self.self_queue {
            queue.cancel_favicon(id);
        }
        if self.item_origin(id) != Some((profile, origin.clone())) {
            return;
        }
        let key = (profile, origin.clone());
        if rgba
            .as_deref()
            .is_some_and(|bytes| self.cache_icon(key.clone(), bytes))
        {
            self.icons_checked.insert(key);
            self.project_tab(id);
        } else {
            self.start_favicon_discovery(id, profile, origin);
        }
    }

    fn on_favicon_batch_read(
        &mut self,
        generation: u64,
        profile: ProfileId,
        space: SpaceId,
        origins: Vec<String>,
        rasters: Vec<(String, Vec<u8>)>,
    ) {
        let exact = self.pending_favicon_batch.as_ref().is_some_and(|pending| {
            pending.generation == generation && pending.profile == profile && pending.space == space
        });
        if !exact {
            return;
        }
        let Some(pending) = self.pending_favicon_batch.take() else {
            return;
        };
        let origin_set: std::collections::HashSet<_> = origins.iter().cloned().collect();
        if origins.len() > MAX_FAVICON_BATCH_ORIGINS
            || origin_set.len() != origins.len()
            || origin_set != pending.requested
            || self
                .spaces
                .get(space)
                .is_none_or(|candidate| candidate.profile != profile)
        {
            return;
        }

        let mut accepted = std::collections::HashSet::new();
        let mut changed = false;
        for (origin, rgba) in rasters.into_iter().take(MAX_FAVICON_BATCH_ORIGINS) {
            if origin_set.contains(&origin) && accepted.insert(origin.clone()) {
                let key = (profile, origin);
                if self.cache_icon(key, &rgba) {
                    // Batch hydration intentionally does not mark freshness:
                    // it restores an immediate sidebar image, while the next
                    // live navigation still performs the one-query age check
                    // and refreshes an old raster through the renderer.
                    changed = true;
                }
            }
        }
        if changed {
            self.project_items();
        }
    }

    fn maybe_discover_favicon(&mut self, id: ItemId) {
        let Some((profile, origin)) = self.item_origin(id) else {
            return;
        };
        if self.icons_checked.contains(&(profile, origin.clone())) {
            return;
        }
        if self.icons_checked.len() >= TRACKED_ICON_ORIGIN_CAPACITY {
            return;
        }

        // Persistent profiles can hydrate the already-decoded fixed raster.
        // Private profiles deliberately bypass SQLite but still use the same
        // renderer-side decoder and a bounded in-memory cache.
        let incognito = self
            .profiles
            .get(profile)
            .is_some_and(|profile| profile.kind == ProfileKind::Incognito);
        if incognito {
            self.start_favicon_discovery(id, profile, origin);
            return;
        }

        let exact_pending = self
            .favicon_store_reads
            .get(&id)
            .is_some_and(|pending| pending.profile == profile && pending.origin == origin);
        if exact_pending {
            return;
        }
        self.cancel_favicon_attempt(id);
        self.favicon_store_generation = self.favicon_store_generation.wrapping_add(1);
        if self.favicon_store_generation == 0 {
            self.favicon_store_generation = 1;
        }
        let generation = self.favicon_store_generation;
        self.favicon_store_reads.insert(
            id,
            PendingFaviconStoreRead {
                generation,
                profile,
                origin: origin.clone(),
            },
        );
        if let Some(reads) = &self.store_reads {
            if reads.request_favicon(generation, id, profile, origin.clone()) {
                if let Some(queue) = &self.self_queue {
                    queue.schedule_favicon(
                        id,
                        0,
                        std::time::Instant::now() + STORE_READ_RESULT_TIMEOUT,
                    );
                }
                return;
            }
            self.favicon_store_reads.remove(&id);
        } else {
            #[cfg(test)]
            {
                let rgba = self.store.fresh_favicon_raster(
                    profile,
                    &origin,
                    FAVICON_CACHE_MAX_AGE_SECONDS,
                );
                self.on_store_read(StoreReadResult::Favicon {
                    generation,
                    id,
                    profile,
                    origin,
                    rgba,
                });
                return;
            }
            #[cfg(not(test))]
            {
                self.favicon_store_reads.remove(&id);
            }
        }
        // Store-read pressure must not make favicons permanently disappear;
        // fall back to the already-bounded renderer discovery pipeline.
        if let Some((current_profile, current_origin)) = self.item_origin(id) {
            self.start_favicon_discovery(id, current_profile, current_origin);
        }
    }

    fn start_favicon_discovery(&mut self, id: ItemId, profile: ProfileId, origin: String) {
        if self
            .icon_attempts
            .get(&id)
            .is_some_and(|attempt| attempt.profile == profile && attempt.origin == origin)
        {
            return;
        }
        if self
            .icon_load_completion_pending
            .get(&id)
            .is_some_and(|pending| pending == &(profile, origin.clone()))
        {
            // The timed budget already expired while this exact document was
            // loading. Wait for its authoritative load-complete edge instead
            // of letting same-origin URL callbacks restart an unbounded loop.
            return;
        }
        self.cancel_favicon_attempt(id);
        self.icon_attempts.insert(
            id,
            IconAttempt {
                profile,
                origin,
                next_attempt: 1,
            },
        );
        let _ = self.engine.discover_favicon(id);
        self.schedule_favicon_poll(id, 1);
    }

    fn schedule_favicon_poll(&self, id: ItemId, attempt: u8) {
        let Some(delay) = FAVICON_POLL_DELAYS.get(usize::from(attempt.saturating_sub(1))) else {
            return;
        };
        if let Some(queue) = &self.self_queue {
            queue.schedule_favicon(id, attempt, std::time::Instant::now() + *delay);
        }
    }

    fn cancel_favicon_attempt(&mut self, id: ItemId) {
        self.icon_attempts.remove(&id);
        self.icon_load_completion_pending.remove(&id);
        self.favicon_store_reads.remove(&id);
        if let Some(reads) = &self.store_reads {
            reads.cancel_favicon(id);
        }
        if let Some(queue) = &self.self_queue {
            queue.cancel_favicon(id);
        }
    }

    fn favicon_load_completed(&mut self, id: ItemId) {
        if self.icon_attempts.contains_key(&id) {
            let _ = self.engine.discover_favicon(id);
            return;
        }
        // If the pre-completion budget expired, this is its sole restart. If
        // no such marker exists, maybe_discover_favicon still respects the
        // per-origin terminal no-icon cache and therefore stays bounded under
        // duplicate load-complete notifications.
        self.icon_load_completion_pending.remove(&id);
        self.maybe_discover_favicon(id);
    }

    fn poll_favicon(&mut self, id: ItemId, attempt: u8) {
        if attempt == 0 {
            let Some(pending) = self.favicon_store_reads.remove(&id) else {
                return;
            };
            if let Some(reads) = &self.store_reads {
                reads.cancel_favicon(id);
            }
            if self.item_origin(id) == Some((pending.profile, pending.origin.clone())) {
                self.start_favicon_discovery(id, pending.profile, pending.origin);
            }
            return;
        }
        let Some(current) = self.icon_attempts.get(&id).cloned() else {
            return;
        };
        if current.next_attempt != attempt
            || self.item_origin(id) != Some((current.profile, current.origin.clone()))
        {
            self.cancel_favicon_attempt(id);
            return;
        }

        let _ = self.engine.discover_favicon(id);
        let next = attempt.saturating_add(1);
        if usize::from(next) <= FAVICON_POLL_DELAYS.len() {
            if let Some(active) = self.icon_attempts.get_mut(&id) {
                active.next_attempt = next;
            }
            self.schedule_favicon_poll(id, next);
        } else {
            self.icon_attempts.remove(&id);
            if self.items.tab(id).is_some_and(|tab| tab.loading) {
                self.icon_load_completion_pending
                    .insert(id, (current.profile, current.origin));
            } else {
                // A fully loaded document with no pixels has conclusively
                // consumed its bounded budget. Cache that negative result so
                // repeated completion events cannot rebuild forever.
                self.icon_load_completion_pending.remove(&id);
                self.icons_checked.insert((current.profile, current.origin));
            }
        }
    }

    fn favicon_pixels(&mut self, id: ItemId, page_url: &str, rgba: Vec<u8>) {
        let Some((profile, origin)) = self.item_origin(id) else {
            return;
        };
        let Some(source_origin) = url::Url::parse(page_url)
            .ok()
            .and_then(|url| origin_of(&url))
        else {
            return;
        };
        if source_origin != origin || zephium_core::icon::validated_rgba32(&rgba).is_none() {
            return;
        }
        let key = (profile, origin.clone());
        if self.icons_checked.len() >= TRACKED_ICON_ORIGIN_CAPACITY
            && !self.icons_checked.contains(&key)
        {
            return;
        }
        if !self.cache_icon(key.clone(), &rgba) {
            return;
        }
        self.icons_checked.insert(key);
        self.cancel_favicon_attempt(id);
        if self
            .profiles
            .get(profile)
            .is_some_and(|profile| profile.kind != ProfileKind::Incognito)
        {
            self.store.save_favicon(
                profile,
                origin,
                Some(zephium_core::icon::RGBA32_MIME.to_owned()),
                rgba,
            );
        }
        self.project_items();
    }

    fn cache_icon(&mut self, key: (ProfileId, String), rgba: &[u8]) -> bool {
        let Some(value) = zephium_core::icon::chrome_value(rgba) else {
            return false;
        };
        self.icon_cache_order.retain(|candidate| candidate != &key);
        while self.icon_values.len() >= ICON_CACHE_CAPACITY && !self.icon_values.contains_key(&key)
        {
            let Some(evicted) = self.icon_cache_order.pop_front() else {
                break;
            };
            self.icon_values.remove(&evicted);
            // `icons_checked` also carries terminal negative results. A
            // positive entry that leaves the bounded raster cache must lose
            // only its positive terminal marker so a later visit may hydrate
            // it from SQLite (or rediscover it for a private profile).
            self.icons_checked.remove(&evicted);
        }
        self.icon_values.insert(key.clone(), value);
        self.icon_cache_order.push_back(key);
        true
    }

    fn profile_of_item(&self, id: ItemId) -> Option<ProfileId> {
        match self.items.get(id)?.placement {
            Placement::Favorites { profile } => Some(profile),
            Placement::Space { space, .. } => self.spaces.get(space).map(|s| s.profile),
        }
    }

    /// A space is an authorization boundary for UI-directed item IDs. Tabs
    /// placed in a space require an exact match; profile-wide favorites may
    /// appear in any space owned by that same profile.
    fn item_in_scope(&self, id: ItemId, profile: ProfileId, space: SpaceId) -> bool {
        if self
            .spaces
            .get(space)
            .is_none_or(|candidate| candidate.profile != profile)
        {
            return false;
        }
        let Some(item) = self.items.get(id).filter(|item| item.tab().is_some()) else {
            return false;
        };
        match item.placement {
            Placement::Favorites {
                profile: item_profile,
            } => item_profile == profile,
            Placement::Space {
                space: item_space, ..
            } => item_space == space,
        }
    }

    fn item_in_focused_scope(&self, id: ItemId) -> bool {
        self.windows
            .focused()
            .is_some_and(|win| self.item_in_scope(id, win.profile, win.space))
    }

    fn pane_in_scope(&self, tree: &Pane, profile: ProfileId, space: SpaceId) -> bool {
        let tabs = tree.tabs();
        tabs.len() <= MAX_VISIBLE_PANES
            && tabs
                .into_iter()
                .all(|id| self.item_in_scope(id, profile, space))
    }

    fn partition_of(&self, id: ItemId) -> Partition {
        let profile = self
            .profile_of_item(id)
            .or_else(|| self.windows.focused().map(|w| w.profile))
            .unwrap_or_else(|| {
                self.profiles
                    .default_profile()
                    .unwrap_or(ProfileId::from(0))
            });
        match self.profiles.get(profile).map(|p| p.kind) {
            Some(ProfileKind::Named) => Partition::Persistent(profile),
            Some(ProfileKind::Incognito) => Partition::Ephemeral(profile),
            Some(ProfileKind::Default) | None => Partition::Default(profile),
        }
    }

    /// window.open / target=_blank lands as a new Today tab next to its
    /// source, routed through the same navigation policy. The split group
    /// survives: the new tab shows alone, the group stays a tab away.
    fn open_linked_tab(&mut self, source: ItemId, url: &str) {
        let Some((profile, space)) = self.windows.focused().map(|win| (win.profile, win.space))
        else {
            return;
        };
        if !self.item_in_scope(source, profile, space) {
            return;
        }
        let id = ItemId::generate();
        if !self.items.insert_tab(
            id,
            Placement::Space {
                space,
                section: SpaceSection::Today,
            },
        ) {
            return;
        }
        let mut fx = self.focus_tab(id);
        fx.extend(self.items.navigate(id, url));
        self.commit(fx);
    }

    fn commit(&mut self, effects: Vec<Effect>) -> NativeWork {
        // Every ordinary committed mutation may change focus, view
        // residency, or split topology. A captured divider path cannot cross
        // that boundary; resize/sidebar geometry updates deliberately bypass
        // `commit` and remain draggable through path-based recomputation.
        self.divider = None;
        let mut native = self.apply(effects);
        self.schedule_persist();
        // Visibility has to land before dormancy. WebView2 only accepts a
        // suspend request for an already-hidden controller; recording the
        // request first would permanently lose the transition.
        native.record(self.relayout());
        self.maintain_views();
        self.project_items();
        native
    }

    // Sleeping-tabs model: hidden views carry a low-memory hint, then a
    // bounded set of exact-generation/epoch probes may authorize full native
    // discard. Positive renderer results are only advisory until this actor
    // rechecks URL, loading, visibility, idle age, and the current budget.
    fn maintain_views(&mut self) -> bool {
        self.crashes.retain(|id, _| self.items.tab(*id).is_some());
        self.crash_presentations
            .retain(|id| self.items.tab(*id).is_some());
        self.pending_zooms
            .retain(|id, _| self.items.tab(*id).is_some_and(TabState::has_view));
        if self.windows.focused().is_none() {
            return false;
        }
        let shown: std::collections::HashSet<ItemId> = if self.window_visible {
            self.pane_tree()
                .map(|t| t.tabs().into_iter().collect())
                .unwrap_or_default()
        } else {
            std::collections::HashSet::new()
        };
        self.recent.retain(|id| self.items.tab(*id).is_some());
        self.last_focus
            .retain(|id, _| self.items.tab(*id).is_some());
        self.last_visits
            .retain(|id, _| self.items.tab(*id).is_some());
        self.discard_protected_until.retain(|id, until| {
            self.items.tab(*id).is_some() && *until > std::time::Instant::now()
        });

        // A timeout command is ordinary coalescible work and may be displaced
        // during a full lifecycle burst. The maintenance pass independently
        // expires the same fixed set so overload cannot strand all probe slots.
        let expired: Vec<ItemId> = self
            .discard_probes
            .iter()
            .filter_map(|(id, state)| match state {
                PendingDiscardProbe::Probing { deadline, .. }
                    if *deadline <= std::time::Instant::now() =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .collect();
        for id in expired {
            self.discard_probes.remove(&id);
            if let Some(queue) = &self.self_queue {
                queue.cancel_discard_probe(id);
            }
            self.protect_discard_candidate(id);
        }

        // Conceptual visible leaves remain protected even while the OS window
        // is minimized. Dormancy may hide them, but discard must not destroy a
        // split the user expects to reappear atomically.
        let protected = self.discard_protected_leaves();
        let live_count = self.items.view_ids().len();
        let invalid_probes: Vec<ItemId> = self
            .discard_probes
            .iter()
            .filter_map(|(id, state)| match state {
                PendingDiscardProbe::Probing { committed_url, .. } => {
                    let valid = live_count > self.live_view_soft_limit
                        && !protected.contains(id)
                        && self.items.tab(*id).is_some_and(|tab| {
                            tab.has_view()
                                && !tab.loading
                                && tab
                                    .url
                                    .as_ref()
                                    .is_some_and(|url| url.as_str() == committed_url)
                        });
                    (!valid).then_some(*id)
                }
                PendingDiscardProbe::Closing { .. } => self.items.tab(*id).is_none().then_some(*id),
            })
            .collect();
        for id in invalid_probes {
            if self.items.tab(id).is_none() {
                self.discard_probes.remove(&id);
                if let Some(queue) = &self.self_queue {
                    queue.cancel_discard_probe(id);
                }
            } else {
                self.cancel_discard_probe(id);
            }
        }

        let mut dormant: Vec<ItemId> = self
            .items
            .view_ids()
            .into_iter()
            .filter(|id| {
                !shown.contains(id)
                    && !self.discard_probes.contains_key(id)
                    && self.idle_for(*id, self.dormant_min)
            })
            .collect();
        dormant.sort();
        if dormant != self.dormant_sent {
            self.dormant_sent = dormant.clone();
            self.engine.set_dormant(dormant);
        }

        if live_count <= self.live_view_soft_limit {
            return false;
        }
        let mut candidates: Vec<ItemId> = self
            .items
            .view_ids()
            .into_iter()
            .filter(|id| {
                !protected.contains(id)
                    && !self.discard_probes.contains_key(id)
                    && self
                        .discard_protected_until
                        .get(id)
                        .is_none_or(|until| *until <= std::time::Instant::now())
                    && self.items.tab(*id).is_some_and(|tab| {
                        tab.has_view()
                            && !tab.loading
                            && tab.url.is_some()
                            && (live_count > self.live_view_pressure_limit
                                || self.idle_for(*id, self.discard_idle_min))
                    })
            })
            .collect();
        candidates.sort_by_key(|id| (self.last_focus.get(id).copied(), *id));

        let available = MAX_CONCURRENT_DISCARD_PROBES.saturating_sub(self.discard_probes.len());
        for id in candidates.into_iter().take(available) {
            let Some(next) = self.next_discard_probe.checked_add(1) else {
                // Reusing a process-local correlation id could accept a very
                // late callback. Saturation permanently disables new probes.
                break;
            };
            self.next_discard_probe = next;
            let probe = DiscardProbeId(next);
            let deadline = std::time::Instant::now() + self.discard_probe_timeout;
            let Some(committed_url) = self
                .items
                .tab(id)
                .and_then(|tab| tab.url.as_ref())
                .map(ToString::to_string)
            else {
                continue;
            };
            self.discard_probes.insert(
                id,
                PendingDiscardProbe::Probing {
                    probe,
                    committed_url,
                    deadline,
                },
            );
            // A suspended WebView2 cannot reliably execute the DOM query.
            // Replace the desired dormant set first; main-thread FIFO then
            // guarantees resume is requested before the probe evaluation.
            if self.dormant_sent.contains(&id) {
                self.dormant_sent.retain(|dormant| *dormant != id);
                self.engine.set_dormant(self.dormant_sent.clone());
            }
            if !self.engine.probe_discard_safety(id, probe) {
                self.discard_probes.remove(&id);
                self.protect_discard_candidate(id);
                continue;
            }
            if let Some(queue) = &self.self_queue {
                queue.schedule_discard_probe(id, probe, deadline);
            }
        }
        false
    }

    fn discard_protected_leaves(&self) -> std::collections::HashSet<ItemId> {
        self.pane_tree()
            .map(|tree| tree.tabs().into_iter().collect())
            .unwrap_or_default()
    }

    fn candidate_is_still_discardable(&self, id: ItemId, committed_url: &str) -> bool {
        let live_count = self.items.view_ids().len();
        live_count > self.live_view_soft_limit
            && !self.discard_protected_leaves().contains(&id)
            && self.items.tab(id).is_some_and(|tab| {
                tab.has_view()
                    && !tab.loading
                    && tab
                        .url
                        .as_ref()
                        .is_some_and(|url| url.as_str() == committed_url)
            })
            && (live_count > self.live_view_pressure_limit
                || self.idle_for(id, self.discard_idle_min))
    }

    fn protect_discard_candidate(&mut self, id: ItemId) {
        self.discard_protected_until
            .insert(id, std::time::Instant::now() + self.discard_protected_retry);
    }

    fn cancel_discard_probe(&mut self, id: ItemId) {
        match self.discard_probes.get_mut(&id) {
            Some(PendingDiscardProbe::Closing { recreate, .. }) => {
                // Physical close already owns the native generation. Preserve
                // the acknowledgement obligation and recreate after it lands.
                *recreate = true;
            }
            Some(PendingDiscardProbe::Probing { .. }) => {
                self.discard_probes.remove(&id);
                if let Some(queue) = &self.self_queue {
                    queue.cancel_discard_probe(id);
                }
            }
            None => {}
        }
    }

    fn recreate_after_inflight_discard(&mut self, id: ItemId) -> bool {
        if let Some(PendingDiscardProbe::Closing { recreate, .. }) =
            self.discard_probes.get_mut(&id)
        {
            *recreate = true;
            true
        } else {
            false
        }
    }

    fn on_discard_probe_timeout(&mut self, id: ItemId, probe: DiscardProbeId) {
        let exact = matches!(
            self.discard_probes.get(&id),
            Some(PendingDiscardProbe::Probing { probe: pending, .. }) if *pending == probe
        );
        if exact {
            self.discard_probes.remove(&id);
            self.protect_discard_candidate(id);
            self.maintain_views();
        }
    }

    fn on_discard_safety(&mut self, id: ItemId, probe: DiscardProbeId, can_discard: bool) {
        let Some(PendingDiscardProbe::Probing {
            probe: pending,
            committed_url,
            ..
        }) = self.discard_probes.get(&id).cloned()
        else {
            return;
        };
        if pending != probe {
            return;
        }
        if let Some(queue) = &self.self_queue {
            queue.cancel_discard_probe(id);
        }
        if !can_discard || !self.candidate_is_still_discardable(id, &committed_url) {
            self.discard_probes.remove(&id);
            if !can_discard {
                self.protect_discard_candidate(id);
            }
            self.maintain_views();
            return;
        }

        self.discard_probes.insert(
            id,
            PendingDiscardProbe::Closing {
                probe,
                recreate: false,
                deferred_navigation: None,
            },
        );
        // `discard_view` retires the exact native generation at its public
        // boundary. Any queued zoom result is now terminally stale.
        self.pending_zooms.remove(&id);
        if !self.engine.discard_view(id, probe) {
            // Engine dispatch failure after lifecycle retirement is terminal
            // at the native boundary. Keep the closing obligation visible;
            // pretending the old view survived would permit unsafe reuse.
            eprintln!("engine: native discard was not admitted");
        }
    }

    fn on_view_discarded(&mut self, id: ItemId, profile: ProfileId, probe: DiscardProbeId) {
        let Some(PendingDiscardProbe::Closing {
            probe: pending,
            recreate,
            deferred_navigation,
        }) = self.discard_probes.get(&id).cloned()
        else {
            return;
        };
        if pending != probe || self.profile_of_item(id) != Some(profile) {
            return;
        }
        self.pending_zooms.remove(&id);
        self.cancel_pending_presentation(id);
        self.discard_probes.remove(&id);
        if !self.items.mark_view_discarded(id) {
            return;
        }

        let mut effects = if let Some(input) = deferred_navigation {
            self.items.navigate(id, &input)
        } else if recreate || self.discard_protected_leaves().contains(&id) {
            self.items.ensure_view(id)
        } else {
            Vec::new()
        };
        // If a split/focus change happened between native acknowledgement and
        // this actor turn, the final visibility check wins.
        if effects.is_empty() && self.discard_protected_leaves().contains(&id) {
            effects.extend(self.items.ensure_view(id));
        }
        self.apply(effects);
        let _ = self.relayout();
        self.maintain_views();
        self.project_tab(id);
    }

    fn idle_for(&self, id: ItemId, min: std::time::Duration) -> bool {
        self.last_focus.get(&id).is_none_or(|t| t.elapsed() >= min)
    }

    fn should_record_visit(&mut self, id: ItemId, url: &str) -> bool {
        const REPEATED_URL_MIN: std::time::Duration = std::time::Duration::from_secs(30);
        const NAVIGATION_MIN: std::time::Duration = std::time::Duration::from_secs(1);
        let now = std::time::Instant::now();
        if let Some((previous, recorded)) = self.last_visits.get(&id) {
            let minimum = if previous == url {
                REPEATED_URL_MIN
            } else {
                NAVIGATION_MIN
            };
            if now.duration_since(*recorded) < minimum {
                return false;
            }
        }
        self.last_visits.insert(id, (url.to_owned(), now));
        true
    }

    // One automatic relaunch per crash burst: a second death inside the
    // window means the page kills its web process deterministically, and a
    // reload loop would peg the machine.
    fn retire_crashed_view(&mut self, id: ItemId) {
        self.pending_zooms.remove(&id);
        let title = self.items.tab(id).map(|tab| tab.title.clone());
        self.items.view_creation_failed(id);
        if let Some(title) = title {
            // `view_creation_failed` owns the generic create-failure label.
            // A renderer crash is different: its label is transient chrome
            // state and must not replace the last committed document title.
            self.items.set_title(id, title);
        }
        self.crash_presentations.insert(id);
    }

    fn on_crashed(&mut self, id: ItemId) {
        const RETRY_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);
        if self.items.tab(id).is_none() {
            return;
        }
        self.cancel_pending_presentation(id);
        self.cancel_discard_probe(id);
        // `Crashed` is emitted only after the engine has physically removed
        // and revoked the exact native-view generation. Sending a second,
        // id-only close here could be reordered behind recovery and destroy
        // the replacement generation.
        self.retire_crashed_view(id);
        self.items.set_loading(id, false);
        let recent = self
            .crashes
            .insert(id, std::time::Instant::now())
            .is_some_and(|t| t.elapsed() < RETRY_WINDOW);
        let active = self.windows.focused().and_then(|window| window.active);
        let effects = if !recent && active == Some(id) {
            self.items.ensure_view(id)
        } else {
            Vec::new()
        };
        self.apply(effects);
        if active == Some(id) {
            let _ = self.relayout();
            self.maintain_views();
        }
        // Crash status is runtime truth, not useful session state. Avoid a
        // full O(items) snapshot/relayout for every hidden WebKit view when a
        // shared renderer process reports a burst of per-view terminations.
        self.project_tab(id);
    }

    fn on_profile_process_exit(&mut self, profile: ProfileId, ids: Vec<ItemId>) {
        const RETRY_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);
        let visible_order: Vec<ItemId> =
            self.pane_tree().map(|tree| tree.tabs()).unwrap_or_default();
        let visible: std::collections::HashSet<ItemId> = visible_order.iter().copied().collect();
        let mut suppressed_visible = std::collections::HashSet::new();
        let mut effects = Vec::new();
        for id in ids {
            if self.profile_of_item(id) != Some(profile) {
                continue;
            }
            self.cancel_pending_presentation(id);
            self.cancel_discard_probe(id);
            let repeated = self
                .crashes
                .insert(id, std::time::Instant::now())
                .is_some_and(|time| time.elapsed() < RETRY_WINDOW);
            self.retire_crashed_view(id);
            // A browser-process loss can report hundreds of tabs at once.
            // Recreate every currently visible split leaf, because native
            // layout admission requires a live token for every leaf. Hidden
            // tabs still recover lazily, avoiding a process/controller storm.
            if visible.contains(&id) && !repeated {
                effects.extend(self.items.ensure_view(id));
            } else if visible.contains(&id) {
                suppressed_visible.insert(id);
            }
        }
        if self
            .windows
            .focused()
            .and_then(|window| window.active)
            .is_some_and(|active| suppressed_visible.contains(&active))
        {
            // A repeatedly crashing active leaf has no native token. Focus a
            // successfully recreated sibling so layout collapses to that live
            // leaf until the protected tab is explicitly activated again.
            if let Some(replacement) = visible_order.into_iter().find(|id| {
                !suppressed_visible.contains(id)
                    && self.items.tab(*id).is_some_and(TabState::has_view)
            }) {
                if let Some(window) = self.windows.focused_mut() {
                    window.active = Some(replacement);
                }
                self.items.set_lifecycle(replacement, Lifecycle::Active);
                self.touch(replacement);
            }
        }
        self.commit(effects);
    }

    fn touch(&mut self, id: ItemId) {
        self.cancel_discard_probe(id);
        self.last_focus.insert(id, std::time::Instant::now());
    }

    fn apply(&mut self, effects: Vec<Effect>) -> NativeWork {
        let mut native = NativeWork::default();
        if effects.is_empty() {
            return native;
        }
        // `Items` sets a tab's logical view bit before returning CreateView.
        // A batch (restore or multi-leaf process recovery) can therefore make
        // every prospective view appear resident before the first native
        // create is considered. Subtract this exact, unique optimistic set to
        // recover the pre-batch resident count, then account only successful
        // creates in actor order. This keeps the ceiling exact without
        // rejecting an early visible leaf because a later leaf pre-set its
        // bit.
        let optimistic_creates: std::collections::HashSet<ItemId> = effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::CreateView { id, .. }
                    if self.items.tab(*id).is_some_and(TabState::has_view) =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .collect();
        let mut logical_residents = self
            .items
            .view_ids()
            .len()
            .saturating_sub(optimistic_creates.len());
        let mut rejected_creates = std::collections::HashSet::new();
        let bounds = self.content_region();
        for effect in effects {
            match effect {
                Effect::CreateView { id, url } => {
                    if logical_residents >= LIVE_VIEW_ABSOLUTE_LIMIT {
                        self.pending_zooms.remove(&id);
                        self.items.view_creation_failed(id);
                        rejected_creates.insert(id);
                        native.rejected = true;
                        continue;
                    }
                    if !self
                        .engine
                        .create_view(id, self.partition_of(id), &url, bounds)
                    {
                        // Dispatch rejection is synchronous and must not rely
                        // on a callback entering an already-overloaded queue.
                        self.pending_zooms.remove(&id);
                        self.items.view_creation_failed(id);
                        rejected_creates.insert(id);
                        native.rejected = true;
                        continue;
                    }
                    logical_residents += 1;
                    native.scheduled = true;
                    // restored or revived views keep their zoom
                    let zoom = self.items.tab(id).map(|t| t.zoom).unwrap_or(1.0);
                    if zoom != 1.0 {
                        let admission = self.request_zoom(id, zoom);
                        native.record(admission);
                        if admission != NativeDispatch::Scheduled {
                            // A newly constructed view starts at 1.0. If its
                            // restore request never reached the native queue,
                            // do not retain or later persist the stale scale.
                            self.items.set_zoom(id, 1.0);
                            self.schedule_persist();
                            self.project_tab(id);
                        }
                    }
                }
                Effect::Navigate { id, url, request } => {
                    if rejected_creates.contains(&id) {
                        // `view_creation_failed` already revoked this pending
                        // request. Do not dispatch navigation to a controller
                        // that this batch deliberately did not create.
                        continue;
                    }
                    if !self.engine.navigate(id, &url, request) {
                        self.items.navigation_failed(id, request);
                        native.rejected = true;
                    } else {
                        native.scheduled = true;
                    }
                }
                Effect::Close { id } => {
                    self.pending_zooms.remove(&id);
                    native.record(self.engine.close(id));
                }
            }
        }
        native
    }

    fn relayout(&self) -> NativeDispatch {
        let Some(win) = self.windows.focused() else {
            return NativeDispatch::Rejected;
        };
        let tree = self.pane_tree();
        let present = tree.as_ref().is_some_and(|t| self.present(t));
        let mut l = layout::compute(win.size, win.mode, win.metrics, present);
        if !self.window_visible {
            l.content = None;
        }
        // Raw native children still receive their final geometry while a
        // first navigation is provisional, but macOS must not shrink the
        // privileged chrome away from a fresh New Tab surface until at least
        // one visible leaf completed its exact chrome-verification transition.
        // The content stage itself is transparent and presentation-gated, so
        // it can sit above this real UI without painting an artificial box.
        let chrome_present = self.window_visible
            && tree.as_ref().is_some_and(|tree| {
                tree.tabs().iter().any(|id| {
                    self.items.tab(*id).is_some_and(|tab| {
                        tab.has_view()
                            && tab.url.is_some()
                            && !self.deferred_first_content_layout.contains(id)
                    })
                })
            });
        let chrome_layout = layout::compute(win.size, win.mode, win.metrics, chrome_present);
        if !self.chrome.position(ChromeFrame {
            rect: chrome_layout.chrome,
            fill_width: chrome_layout.content.is_none(),
        }) {
            return NativeDispatch::Rejected;
        }
        let dividers = match (&tree, l.content) {
            (Some(tree), Some(region)) => {
                let local = Rect::new(0.0, 0.0, region.width, region.height);
                split::dividers(tree, local, win.metrics.gap)
                    .into_iter()
                    .map(|d| DividerView {
                        x: region.x + d.strip.x,
                        y: region.y + d.strip.y,
                        width: d.strip.width,
                        height: d.strip.height,
                        vertical: d.axis == Axis::Row,
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        (self.emit)(Projection::Layout(LayoutState { dividers }));
        self.engine.set_content(win.id, tree, l.content)
    }

    fn locate_divider(&self, x: f64, y: f64) -> Option<GrabbedDivider> {
        let win = self.windows.focused()?;
        let tree = self.pane_tree()?;
        let region =
            layout::compute(win.size, win.mode, win.metrics, self.present(&tree)).content?;
        let local = Rect::new(0.0, 0.0, region.width, region.height);
        let divider = split::divider_at(&tree, local, win.metrics.gap, x - region.x, y - region.y)?;
        Some(GrabbedDivider {
            window: win.id,
            topology: tree,
            divider,
        })
    }

    fn divider_drag(&mut self, x: f64, y: f64) {
        let Some(grabbed) = self.divider.as_ref() else {
            return;
        };
        let grabbed_window = grabbed.window;
        let grabbed_path = grabbed.divider.path.clone();
        let Some(win) = self.windows.focused() else {
            self.divider = None;
            return;
        };
        let current_tree = self.pane_tree();
        let topology_is_current = win.id == grabbed_window
            && current_tree
                .as_ref()
                .is_some_and(|tree| grabbed.topology.same_topology(tree));
        if !topology_is_current {
            // Focus/topology changed while the pointer was captured. The same
            // binary path may now identify a different live branch.
            self.divider = None;
            return;
        }
        let Some(region) = layout::compute(win.size, win.mode, win.metrics, true).content else {
            return;
        };
        let gap = win.metrics.gap;
        let Some(tree) = current_tree.as_ref() else {
            self.divider = None;
            return;
        };
        let local = Rect::new(0.0, 0.0, region.width, region.height);
        let Some(current) = split::divider_at_path(tree, local, gap, &grabbed_path) else {
            // The split tree changed while the pointer was captured. Its old
            // path is no longer authority for any live branch.
            self.divider = None;
            return;
        };
        let ratio = split::ratio_for(current.axis, current.rect, gap, x - region.x, y - region.y);
        if let Some(win) = self.windows.focused_mut() {
            if let Some(tree) = win.splits.as_mut() {
                tree.set_ratio(&current.path, ratio);
            }
        }
        let _ = self.relayout();
    }

    fn present(&self, tree: &Pane) -> bool {
        tree.tabs()
            .iter()
            .any(|id| self.items.tab(*id).is_some_and(TabState::has_view))
    }

    // The split group persists across tab switches (Arc model): members show
    // the whole group, other tabs show alone, the group is a tab away.
    fn pane_tree(&self) -> Option<Pane> {
        let win = self.windows.focused()?;
        let active = win.active?;
        if !self.item_in_scope(active, win.profile, win.space) {
            return None;
        }
        if let Some(tree) = win.splits.clone() {
            if tree.contains(active)
                && self.pane_in_scope(&tree, win.profile, win.space)
                && tree
                    .tabs()
                    .into_iter()
                    .all(|id| self.items.tab(id).is_some_and(TabState::has_view))
            {
                return Some(tree);
            }
        }
        self.items
            .tab(active)
            .is_some_and(TabState::has_view)
            .then_some(Pane::Leaf(active))
    }

    fn content_region(&self) -> Rect {
        let Some(win) = self.windows.focused() else {
            return Rect::default();
        };
        layout::compute(win.size, win.mode, win.metrics, true)
            .content
            .unwrap_or_default()
    }

    fn today_tabs(&self, space: SpaceId) -> Vec<ItemId> {
        self.items
            .roots(Placement::Space {
                space,
                section: SpaceSection::Today,
            })
            .iter()
            .copied()
            .filter(|id| self.items.tab(*id).is_some())
            .collect()
    }

    fn schedule_persist(&mut self) {
        if !self.bootstrapped {
            return;
        }
        self.session_revision = self.session_revision.wrapping_add(1);
        self.schedule_current_session_persist();
    }

    fn schedule_url_checkpoint(&mut self, id: ItemId) {
        if !self.bootstrapped || self.items.tab(id).is_none() {
            return;
        }
        self.session_revision = self.session_revision.wrapping_add(1);
        let first_url_dirty = self.url_checkpoint_dirty.is_empty();
        self.url_checkpoint_dirty.insert(id);
        // A pending structural snapshot already includes the newest URL and
        // retains its much shorter durability deadline.
        if self.persist_first_dirty.is_some() || !first_url_dirty {
            return;
        }
        let now = std::time::Instant::now();
        let interval_floor = self
            .last_url_checkpoint
            .checked_add(URL_CHECKPOINT_INTERVAL)
            .unwrap_or(now + URL_CHECKPOINT_INTERVAL);
        let deadline = interval_floor.max(now + URL_CHECKPOINT_DEBOUNCE);
        if let Some(queue) = self.self_queue.as_ref() {
            queue.schedule_persist(deadline);
        } else {
            #[cfg(test)]
            {
                // Deterministic unit shells do not own the production timer.
                self.persist();
            }
            #[cfg(not(test))]
            {
                // Production construction always installs the timer before
                // the actor can receive a URL. If that invariant changes,
                // defer to the exact shutdown snapshot instead of restoring
                // hostile per-URL full rewrites.
                eprintln!("persistence: URL checkpoint timer is unavailable");
            }
        }
    }

    /// Schedules the current authoritative state without advancing its logical
    /// revision. Used after a durable deletion barrier when mutations already
    /// counted by `schedule_persist` need a new post-barrier debounce.
    fn schedule_current_session_persist(&mut self) {
        if !self.bootstrapped {
            return;
        }
        let now = std::time::Instant::now();
        let first = *self.persist_first_dirty.get_or_insert(now);
        let deadline = (now + PERSIST_DEBOUNCE).min(first + PERSIST_MAX_AGE);
        if let Some(queue) = self.self_queue.as_ref() {
            queue.schedule_persist(deadline);
        } else {
            // Directly-constructed shells are used by deterministic unit
            // tests and embedders without the production timer thread.
            self.persist();
        }
    }

    fn persist(&mut self) {
        self.persist_first_dirty = None;
        // The durable session is loaded by Bootstrap. Before that ordered
        // point, an empty in-memory shell is not authoritative: persisting it
        // during an immediate quit would erase a valid previous session.
        if !self.bootstrapped {
            return;
        }
        self.url_checkpoint_dirty.clear();
        self.last_url_checkpoint = std::time::Instant::now();
        let win = self.windows.focused();
        let state = session::snapshot(
            &self.profiles,
            &self.spaces,
            &self.items,
            win.map(|w| w.space),
            win.and_then(|w| w.active),
            win.and_then(|w| w.splits.as_ref()),
        );
        self.store.save_session(state);
    }

    fn project_runtime_status(&self) {
        (self.emit)(Projection::RuntimeStatus(RuntimeStatus {
            restart_required: self.runtime_restart_required,
        }));
    }

    fn reconcile_runtime_restart_requirement(&mut self) -> bool {
        if self.runtime_restart_required || !self.engine.runtime_restart_required() {
            return false;
        }
        self.runtime_restart_required = true;
        self.project_runtime_status();
        true
    }

    fn project_items(&self) {
        let Some(win) = self.windows.focused() else {
            return;
        };
        let profile = win.profile;
        let tabs: Vec<TabView> = self
            .today_tabs(win.space)
            .into_iter()
            .filter_map(|id| {
                self.items
                    .tab(id)
                    .map(|tab| self.generic_tab_view(id, tab, Some(profile)))
            })
            .collect();
        self.record_tab_projection_revisions(&tabs);
        (self.emit)(Projection::Items(ItemsState {
            projection_revision: format!("{:032x}", self.next_projection_revision()),
            tabs,
            active: win.active.map(|i| i.to_string()),
        }));
    }

    fn project_tab(&self, id: ItemId) {
        let profile = self.profile_of_item(id);
        if let Some(tab) = self.items.tab(id) {
            let projection = self.generic_tab_view(id, tab, profile);
            self.record_tab_projection_revision(id, &projection.projection_revision);
            (self.emit)(Projection::Tab(projection));
        }
    }

    fn record_tab_projection_revisions(&self, tabs: &[TabView]) {
        let Ok(mut revisions) = self.last_tab_projection_revision.try_borrow_mut() else {
            // The shell actor is single-threaded and these borrows never span
            // callbacks. Retaining the older value fails closed by making an
            // otherwise valid presentation callback stale.
            return;
        };
        for tab in tabs {
            if let Some(id) = ItemId::parse(&tab.id) {
                revisions.insert(id, tab.projection_revision.clone());
            }
        }
    }

    fn record_tab_projection_revision(&self, id: ItemId, revision: &str) {
        if let Ok(mut revisions) = self.last_tab_projection_revision.try_borrow_mut() {
            revisions.insert(id, revision.to_owned());
        }
    }

    fn generic_tab_view(&self, id: ItemId, tab: &TabState, profile: Option<ProfileId>) -> TabView {
        let mut view = self.presentation_tab_view(id, tab, self.favicon_key(tab, profile));
        if self.deferred_first_content_layout.contains(&id) {
            // A full Items snapshot may still be necessary for focus or tab
            // topology. Preserve that delivery while ensuring the exact
            // presentation eval remains the first URL-bearing projection.
            view.url = None;
            view.title = "New Tab".into();
            view.loading = false;
            view.can_go_back = false;
            view.can_go_forward = false;
            view.favicon = None;
        }
        view
    }

    fn presentation_tab_view(
        &self,
        id: ItemId,
        tab: &TabState,
        favicon: Option<String>,
    ) -> TabView {
        let mut view = tab_view(id, tab, favicon, self.next_projection_revision());
        if self.crash_presentations.contains(&id) {
            view.title = "Page crashed".into();
        }
        view
    }

    fn next_projection_revision(&self) -> u128 {
        // Saturation is fail-closed: subsequent equal revisions are ignored
        // by privileged chrome, so no older projection can become current.
        let next = self.projection_sequence.get().saturating_add(1);
        self.projection_sequence.set(next);
        next
    }

    // Chrome receives only a fixed-shape raster value; it never constructs a
    // page-controlled image URL or invokes a privileged image decoder.
    fn favicon_key(&self, tab: &TabState, profile: Option<ProfileId>) -> Option<String> {
        let origin = tab.url.as_ref().and_then(origin_of)?;
        self.favicon_key_for(profile?, &origin)
    }

    fn favicon_key_for(&self, profile: ProfileId, origin: &str) -> Option<String> {
        self.icon_values.get(&(profile, origin.to_owned())).cloned()
    }

    fn favicon_key_for_url(&self, profile: ProfileId, url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        self.favicon_key_for(profile, &origin_of(&parsed)?)
    }
}

fn tab_result(id: ItemId, tab: &TabState, favicon: Option<String>) -> SearchResult {
    let detail = tab
        .url
        .as_ref()
        .and_then(|u| u.host_str().map(ToString::to_string))
        .unwrap_or_default();
    SearchResult {
        kind: "tab".into(),
        title: tab.title.clone(),
        detail,
        favicon,
        action: SearchAction::ActivateTab { id: id.to_string() },
    }
}

fn tab_view(id: ItemId, tab: &TabState, favicon: Option<String>, revision: u128) -> TabView {
    TabView {
        id: id.to_string(),
        projection_revision: format!("{revision:032x}"),
        title: tab.title.clone(),
        url: tab.url.as_ref().map(ToString::to_string),
        loading: tab.loading,
        can_go_back: tab.can_go_back,
        can_go_forward: tab.can_go_forward,
        favicon,
    }
}

fn origin_of(url: &url::Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    match url.origin() {
        url::Origin::Tuple(..) => Some(url.origin().ascii_serialization()),
        url::Origin::Opaque(_) => None,
    }
}

fn same_browser_origin(left: &url::Url, right: &url::Url) -> bool {
    match (origin_of(left), origin_of(right)) {
        (Some(left), Some(right)) => left == right,
        // `about:blank` is the only admitted opaque browser target. Treat it
        // as same-document only when its canonical URL is exactly unchanged.
        (None, None) => left.as_str() == right.as_str(),
        _ => false,
    }
}

fn neutral_title_for_url(url: &url::Url) -> String {
    let Some(host) = url.host_str() else {
        return url.as_str().to_owned();
    };
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use zephium_core::ids::WindowId;
    use zephium_core::ports::engine::{
        ContentScope, NavigationRequestId, UserContent, ZoomRequestId,
    };
    use zephium_core::session::{
        PersistedItem, PersistedKind, PersistedProfile, PersistedSpace, SessionState,
    };

    type HeldErasure = (ProfileId, Box<dyn FnOnce(ProfileDataErasureOutcome) + Send>);

    #[derive(Default)]
    struct FakeEngine {
        calls: Mutex<Vec<String>>,
        navigation_requests: Mutex<Vec<NavigationRequestId>>,
        zoom_requests: Mutex<Vec<(ItemId, f64, ZoomRequestId)>>,
        shutdown_result: Mutex<Option<bool>>,
        skip_shutdown_callback: std::sync::atomic::AtomicBool,
        reject_create_dispatch: std::sync::atomic::AtomicBool,
        reject_navigation_dispatch: std::sync::atomic::AtomicBool,
        reject_native_dispatch: std::sync::atomic::AtomicBool,
        unsupported_presentation: std::sync::atomic::AtomicBool,
        runtime_restart_required: std::sync::atomic::AtomicBool,
        erasure_outcomes: Mutex<VecDeque<ProfileDataErasureOutcome>>,
        held_erasures: Mutex<Vec<HeldErasure>>,
        hold_erasures: std::sync::atomic::AtomicBool,
    }

    impl FakeEngine {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
        fn log(&self, s: String) {
            self.calls.lock().unwrap().push(s);
        }
        fn last_layout(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find_map(|c| {
                    c.split_once(' ')
                        .filter(|(head, _)| head.starts_with("layout@"))
                        .map(|(_, rest)| rest)
                })
                .map(|s| {
                    s.split(',')
                        .filter(|x| !x.is_empty())
                        .map(Into::into)
                        .collect()
                })
                .unwrap_or_default()
        }

        fn last_navigation_request(&self) -> NavigationRequestId {
            *self
                .navigation_requests
                .lock()
                .unwrap()
                .last()
                .expect("a navigation request must have been admitted")
        }

        fn last_zoom_request(&self) -> (ItemId, f64, ZoomRequestId) {
            *self
                .zoom_requests
                .lock()
                .unwrap()
                .last()
                .expect("a zoom request must have been admitted")
        }

        fn native_admission(&self) -> NativeDispatch {
            if self
                .reject_native_dispatch
                .load(std::sync::atomic::Ordering::Acquire)
            {
                NativeDispatch::Rejected
            } else {
                NativeDispatch::Scheduled
            }
        }

        fn push_erasure_outcomes(
            &self,
            outcomes: impl IntoIterator<Item = ProfileDataErasureOutcome>,
        ) {
            self.erasure_outcomes.lock().unwrap().extend(outcomes);
        }

        fn complete_held_erasure(&self, outcome: ProfileDataErasureOutcome) {
            let (_, done) = self.held_erasures.lock().unwrap().remove(0);
            done(outcome);
        }
    }

    impl Engine for FakeEngine {
        fn runtime_restart_required(&self) -> bool {
            self.runtime_restart_required
                .load(std::sync::atomic::Ordering::Acquire)
        }

        fn create_view(&self, id: ItemId, partition: Partition, url: &str, _bounds: Rect) -> bool {
            if self
                .reject_create_dispatch
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return false;
            }
            let kind = match partition {
                Partition::Default(_) => "default",
                Partition::Persistent(_) => "persistent",
                Partition::Ephemeral(_) => "ephemeral",
            };
            self.log(format!("create {id} {url} [{kind}]"));
            true
        }
        fn navigate(&self, id: ItemId, url: &str, request: NavigationRequestId) -> bool {
            if self
                .reject_navigation_dispatch
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return false;
            }
            self.navigation_requests.lock().unwrap().push(request);
            self.log(format!("navigate {id} {url}"));
            true
        }
        fn present_navigation(
            &self,
            id: ItemId,
            navigation: NavigationPresentationId,
        ) -> NativeDispatch {
            self.log(format!("present {id} {}", navigation.into_raw()));
            if self
                .unsupported_presentation
                .load(std::sync::atomic::Ordering::Acquire)
            {
                NativeDispatch::Unsupported
            } else {
                self.native_admission()
            }
        }
        fn reload(&self, id: ItemId) -> NativeDispatch {
            self.log(format!("reload {id}"));
            self.native_admission()
        }
        fn stop(&self, _id: ItemId) -> NativeDispatch {
            self.native_admission()
        }
        fn go_back(&self, _id: ItemId) -> NativeDispatch {
            self.native_admission()
        }
        fn go_forward(&self, _id: ItemId) -> NativeDispatch {
            self.native_admission()
        }
        fn close(&self, id: ItemId) -> NativeDispatch {
            self.log(format!("close {id}"));
            self.native_admission()
        }
        fn set_content(
            &self,
            window: WindowId,
            tree: Option<Pane>,
            region: Option<Rect>,
        ) -> NativeDispatch {
            let ids: Vec<String> = match (tree, region) {
                (Some(t), Some(_)) => t.tabs().iter().map(|id| id.to_string()).collect(),
                _ => Vec::new(),
            };
            self.log(format!("layout@{window} {}", ids.join(",")));
            self.native_admission()
        }
        fn set_drop_indicator(&self, _window: WindowId, _zone: Option<Rect>) -> NativeDispatch {
            self.native_admission()
        }
        fn zoom(&self, id: ItemId, scale: f64, request: ZoomRequestId) -> NativeDispatch {
            self.log(format!("zoom {id} {scale}"));
            let admission = self.native_admission();
            if admission == NativeDispatch::Scheduled {
                self.zoom_requests
                    .lock()
                    .unwrap()
                    .push((id, scale, request));
            }
            admission
        }
        fn set_muted(&self, _id: ItemId, _muted: bool) -> NativeDispatch {
            NativeDispatch::Unsupported
        }
        fn find(&self, _id: ItemId, _query: Option<&str>) -> NativeDispatch {
            NativeDispatch::Unsupported
        }
        fn capture(&self, _id: ItemId) -> NativeDispatch {
            NativeDispatch::Unsupported
        }
        fn extract_html(&self, _id: ItemId) -> NativeDispatch {
            self.native_admission()
        }
        fn discover_favicon(&self, id: ItemId) -> NativeDispatch {
            self.log(format!("discover {id}"));
            self.native_admission()
        }
        fn probe_discard_safety(&self, id: ItemId, probe: DiscardProbeId) -> bool {
            self.log(format!("probe-discard {id} {}", probe.0));
            true
        }
        fn discard_view(&self, id: ItemId, probe: DiscardProbeId) -> bool {
            self.log(format!("discard {id} {}", probe.0));
            true
        }
        fn set_dormant(&self, ids: Vec<ItemId>) {
            let mut ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
            ids.sort();
            self.log(format!("dormant {}", ids.join(",")));
        }
        fn print(&self, _id: ItemId) -> NativeDispatch {
            self.native_admission()
        }
        fn set_user_content(&self, _scope: ContentScope, _content: UserContent) {}
        fn set_shortcuts(&self, _shortcuts: Vec<zephium_core::ports::engine::Shortcut>) {}
        fn set_content_rules(&self, _profile: ProfileId, _compiled: String) {}
        fn erase_profile_data(
            &self,
            profile: ProfileId,
            done: Box<dyn FnOnce(zephium_core::ports::engine::ProfileDataErasureOutcome) + Send>,
        ) {
            self.log(format!("erase-profile {profile}"));
            if self
                .hold_erasures
                .load(std::sync::atomic::Ordering::Acquire)
            {
                self.held_erasures.lock().unwrap().push((profile, done));
                return;
            }
            let outcome = self
                .erasure_outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(ProfileDataErasureOutcome::Failed);
            done(outcome);
        }
        fn shutdown(&self, done: Box<dyn FnOnce(bool) + Send>) {
            if !self
                .skip_shutdown_callback
                .load(std::sync::atomic::Ordering::Acquire)
            {
                done(self.shutdown_result.lock().unwrap().unwrap_or(true));
            }
        }
    }

    #[derive(Default)]
    struct FakeStore {
        saved: Mutex<Option<SessionState>>,
        events: Mutex<Vec<&'static str>>,
        flush_result: Mutex<Option<bool>>,
        load_failed: Mutex<bool>,
        recovery_reason: Mutex<Option<String>>,
        degraded_profiles: Mutex<Vec<ProfileId>>,
        panic_on_load: std::sync::atomic::AtomicBool,
        history: Vec<zephium_core::ports::store::HistoryHit>,
        history_delay_ms: std::sync::atomic::AtomicU64,
        visits: Mutex<Vec<String>>,
        icon_ages: Mutex<std::collections::HashMap<String, i64>>,
        icons: Mutex<Vec<(String, Vec<u8>)>>,
        reject_settings: std::sync::atomic::AtomicBool,
        pending_deletions: Mutex<Vec<PendingProfileDeletion>>,
        pending_load_failures: std::sync::atomic::AtomicUsize,
        authorize_outcomes: Mutex<VecDeque<ProfileDeletionAuthorizeOutcome>>,
        authorize_unknown_commits: std::sync::atomic::AtomicBool,
        authorized_sessions: Mutex<Vec<(ProfileId, SessionState)>>,
        finalize_outcomes: Mutex<VecDeque<ProfileDeletionFinalizeOutcome>>,
        finalize_unknown_completes: std::sync::atomic::AtomicBool,
    }

    impl Store for FakeStore {
        fn save_session(&self, session: SessionState) {
            *self.saved.lock().unwrap() = Some(session);
            self.events.lock().unwrap().push("save");
        }
        fn flush(&self) -> bool {
            self.events.lock().unwrap().push("flush");
            self.flush_result.lock().unwrap().unwrap_or(true)
        }
        fn load_session(&self) -> SessionLoad {
            assert!(
                !self
                    .panic_on_load
                    .load(std::sync::atomic::Ordering::Acquire),
                "injected store panic"
            );
            if *self.load_failed.lock().unwrap() {
                return SessionLoad::Failed;
            }
            if let Some(reason) = self.recovery_reason.lock().unwrap().clone() {
                return SessionLoad::RecoveryRequired { reason };
            }
            let Some(state) = self.saved.lock().unwrap().clone() else {
                return SessionLoad::Absent;
            };
            let profiles = self.degraded_profiles.lock().unwrap().clone();
            if profiles.is_empty() {
                SessionLoad::Loaded(state)
            } else {
                SessionLoad::LoadedWithDegradedProfiles { state, profiles }
            }
        }
        fn record_visit(&self, _profile: ProfileId, url: String, _title: String) {
            self.visits.lock().unwrap().push(url);
        }
        fn app_setting(&self, _key: &str) -> Option<String> {
            None
        }
        fn set_app_setting(&self, _key: String, _value: String) -> bool {
            !self
                .reject_settings
                .load(std::sync::atomic::Ordering::Acquire)
        }
        fn search_history(
            &self,
            _profile: ProfileId,
            _query: &str,
            _limit: u32,
        ) -> Vec<zephium_core::ports::store::HistoryHit> {
            let delay = self
                .history_delay_ms
                .load(std::sync::atomic::Ordering::Acquire);
            if delay != 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            self.history.clone()
        }
        fn favicon_age(&self, _profile: ProfileId, origin: &str) -> Option<i64> {
            self.icon_ages.lock().unwrap().get(origin).copied()
        }
        fn save_favicon(
            &self,
            _profile: ProfileId,
            origin: String,
            _content_type: Option<String>,
            bytes: Vec<u8>,
        ) {
            self.icons.lock().unwrap().push((origin, bytes));
        }
        fn favicon_bytes(
            &self,
            _profile: ProfileId,
            origin: &str,
        ) -> Option<(Option<String>, Vec<u8>)> {
            self.icons
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|(stored_origin, _)| stored_origin == origin)
                .map(|(_, bytes)| {
                    (
                        Some(zephium_core::icon::RGBA32_MIME.to_owned()),
                        bytes.clone(),
                    )
                })
        }
        fn fresh_favicon_raster(
            &self,
            profile: ProfileId,
            origin: &str,
            max_age_seconds: i64,
        ) -> Option<Vec<u8>> {
            self.favicon_age(profile, origin)
                .is_some_and(|age| age <= max_age_seconds)
                .then(|| self.favicon_bytes(profile, origin))
                .flatten()
                .map(|(_, bytes)| bytes)
        }
        fn pending_profile_deletions(&self) -> ProfileDeletionLoad {
            if self
                .pending_load_failures
                .fetch_update(
                    std::sync::atomic::Ordering::AcqRel,
                    std::sync::atomic::Ordering::Acquire,
                    |remaining| remaining.checked_sub(1),
                )
                .is_ok()
            {
                return ProfileDeletionLoad::Failed;
            }
            ProfileDeletionLoad::Loaded(self.pending_deletions.lock().unwrap().clone())
        }
        fn authorize_profile_deletion(
            &self,
            profile: ProfileId,
            filtered_session: SessionState,
            _deadline: std::time::Instant,
        ) -> ProfileDeletionAuthorizeOutcome {
            self.events.lock().unwrap().push("authorize-delete");
            self.authorized_sessions
                .lock()
                .unwrap()
                .push((profile, filtered_session.clone()));
            let outcome = self
                .authorize_outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(ProfileDeletionAuthorizeOutcome::NotRegistered);
            if matches!(
                outcome,
                ProfileDeletionAuthorizeOutcome::Authorized
                    | ProfileDeletionAuthorizeOutcome::AlreadyAuthorized
            ) || (outcome == ProfileDeletionAuthorizeOutcome::OutcomeUnknown
                && self
                    .authorize_unknown_commits
                    .load(std::sync::atomic::Ordering::Acquire))
            {
                *self.saved.lock().unwrap() = Some(filtered_session);
                let mut pending = self.pending_deletions.lock().unwrap();
                if !pending.iter().any(|deletion| deletion.profile == profile) {
                    pending.push(PendingProfileDeletion {
                        profile,
                        native_erasure_verified: false,
                    });
                }
            }
            outcome
        }
        fn finalize_profile_deletion(
            &self,
            profile: ProfileId,
            _deadline: std::time::Instant,
        ) -> ProfileDeletionFinalizeOutcome {
            self.events.lock().unwrap().push("finalize-delete");
            let outcome = self
                .finalize_outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(ProfileDeletionFinalizeOutcome::NotAuthorized);
            if outcome == ProfileDeletionFinalizeOutcome::Completed
                || (outcome == ProfileDeletionFinalizeOutcome::OutcomeUnknown
                    && self
                        .finalize_unknown_completes
                        .load(std::sync::atomic::Ordering::Acquire))
            {
                self.pending_deletions
                    .lock()
                    .unwrap()
                    .retain(|deletion| deletion.profile != profile);
            } else if let Some(deletion) = self
                .pending_deletions
                .lock()
                .unwrap()
                .iter_mut()
                .find(|deletion| deletion.profile == profile)
            {
                // A finalize attempt durably records native proof before its
                // local purge can fail.
                deletion.native_erasure_verified = true;
            }
            outcome
        }
    }

    struct FakeChrome;
    impl GeometryChrome for FakeChrome {
        fn position(&self, _frame: ChromeFrame) -> bool {
            true
        }
    }
    impl PresentationChrome for FakeChrome {
        fn apply_tab_for_presentation(
            &self,
            _presentation: ChromePresentation,
            _done: ChromePresentationCallback,
        ) -> ChromePresentationDispatch {
            ChromePresentationDispatch::Applied
        }
    }

    #[derive(Default)]
    struct AsyncChrome {
        pending: Mutex<VecDeque<(ChromePresentation, ChromePresentationCallback)>>,
        reject_admission: std::sync::atomic::AtomicBool,
    }

    impl GeometryChrome for AsyncChrome {
        fn position(&self, _frame: ChromeFrame) -> bool {
            true
        }
    }

    impl PresentationChrome for AsyncChrome {
        fn apply_tab_for_presentation(
            &self,
            presentation: ChromePresentation,
            done: ChromePresentationCallback,
        ) -> ChromePresentationDispatch {
            if self
                .reject_admission
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return ChromePresentationDispatch::Rejected;
            }
            self.pending.lock().unwrap().push_back((presentation, done));
            ChromePresentationDispatch::Scheduled
        }
    }

    impl AsyncChrome {
        fn presentations(&self) -> Vec<ChromePresentation> {
            self.pending
                .lock()
                .unwrap()
                .iter()
                .map(|(presentation, _)| presentation.clone())
                .collect()
        }

        fn complete_next(&self, applied: bool) -> ChromePresentation {
            let (presentation, done) = self
                .pending
                .lock()
                .unwrap()
                .pop_front()
                .expect("an exact privileged presentation must be pending");
            done(applied);
            presentation
        }
    }

    type CannedFetch = Option<(Option<String>, Vec<u8>)>;

    #[derive(Default)]
    struct FakeNet {
        replies: Mutex<Vec<CannedFetch>>,
        urls: Mutex<Vec<String>>,
    }

    impl Net for FakeNet {
        fn fetch(
            &self,
            url: String,
            _max_bytes: usize,
            done: Box<dyn FnOnce(Option<zephium_core::ports::net::Fetched>) + Send>,
        ) -> bool {
            self.urls.lock().unwrap().push(url);
            let mut replies = self.replies.lock().unwrap();
            let reply = if replies.is_empty() {
                None
            } else {
                replies.remove(0)
            };
            done(
                reply.map(|(content_type, bytes)| zephium_core::ports::net::Fetched {
                    content_type,
                    bytes,
                }),
            );
            true
        }
    }

    // Materializes projections the way the frontend store does: snapshots
    // replace, deltas patch one row.
    type Screen = Arc<Mutex<ItemsState>>;

    fn apply_projection(view: &mut ItemsState, p: Projection) {
        match p {
            Projection::Items(s) => *view = s,
            Projection::Tab(t) => {
                if let Some(slot) = view.tabs.iter_mut().find(|x| x.id == t.id) {
                    *slot = t;
                }
            }
            Projection::UiCommand(_) => {}
            Projection::Search(_) => {}
            Projection::Layout(_) => {}
            Projection::RuntimeStatus(_) => {}
            Projection::OperationProcessed(_) => {}
        }
    }

    fn setup_with(store: Arc<FakeStore>) -> (Shell, Arc<FakeEngine>, Screen) {
        let engine = Arc::new(FakeEngine::default());
        let screen: Screen = Arc::new(Mutex::new(ItemsState {
            projection_revision: String::new(),
            tabs: Vec::new(),
            active: None,
        }));
        let sink = screen.clone();
        let mut shell = Shell::new(
            engine.clone(),
            store,
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |p| apply_projection(&mut sink.lock().unwrap(), p)),
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        (shell, engine, screen)
    }

    fn setup() -> (Shell, Arc<FakeEngine>, Screen) {
        setup_with(Arc::new(FakeStore::default()))
    }

    fn setup_with_async_chrome() -> (Shell, Arc<FakeEngine>, Arc<AsyncChrome>, Screen) {
        let engine = Arc::new(FakeEngine::default());
        let chrome = Arc::new(AsyncChrome::default());
        let screen: Screen = Arc::new(Mutex::new(ItemsState {
            projection_revision: String::new(),
            tabs: Vec::new(),
            active: None,
        }));
        let sink = screen.clone();
        let mut shell = Shell::new(
            engine.clone(),
            Arc::new(FakeStore::default()),
            chrome.clone(),
            Arc::new(FakeNet::default()),
            Box::new(move |projection| apply_projection(&mut sink.lock().unwrap(), projection)),
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        (shell, engine, chrome, screen)
    }

    type OperationLog = Arc<Mutex<Vec<OperationDisposition>>>;

    fn setup_with_operation_log(
        store: Arc<FakeStore>,
    ) -> (Shell, Arc<FakeEngine>, Screen, OperationLog) {
        let engine = Arc::new(FakeEngine::default());
        let screen: Screen = Arc::new(Mutex::new(ItemsState {
            projection_revision: String::new(),
            tabs: Vec::new(),
            active: None,
        }));
        let operations: OperationLog = Arc::new(Mutex::new(Vec::new()));
        let sink = screen.clone();
        let operation_sink = operations.clone();
        let mut shell = Shell::new(
            engine.clone(),
            store,
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |projection| {
                if let Projection::OperationProcessed(completion) = &projection {
                    operation_sink.lock().unwrap().push(completion.clone());
                }
                apply_projection(&mut sink.lock().unwrap(), projection);
            }),
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        (shell, engine, screen, operations)
    }

    fn add_inactive_named_profile(shell: &mut Shell, seed: u128) -> ProfileId {
        let profile = ProfileId::from(seed);
        let space = SpaceId::from(seed + 1);
        assert!(shell.profiles.insert(Profile {
            id: profile,
            name: "Deletable".into(),
            kind: ProfileKind::Named,
        }));
        assert!(shell.spaces.insert(Space {
            id: space,
            profile,
            name: "Deletable".into(),
        }));
        profile
    }

    fn delete_operation(operation_id: &str, profile: ProfileId) -> Command {
        Command::Operation {
            operation_id: operation_id.into(),
            command: Box::new(Command::DeleteProfile(profile)),
        }
    }

    fn test_shutdown_deadline() -> std::time::Instant {
        std::time::Instant::now() + END_TO_END_SHUTDOWN_TIMEOUT
    }

    fn last(screen: &Screen) -> ItemsState {
        screen.lock().unwrap().clone()
    }

    fn active_id(screen: &Screen) -> ItemId {
        ItemId::parse(&last(screen).active.unwrap()).unwrap()
    }

    fn persisted_zoom(store: &FakeStore, id: ItemId) -> f64 {
        let saved = store.saved.lock().unwrap().clone().unwrap();
        let item = saved.items.iter().find(|item| item.id == id).unwrap();
        let PersistedKind::Tab { zoom, .. } = &item.kind else {
            panic!("expected persisted tab")
        };
        *zoom
    }

    fn navigate_and_commit(shell: &mut Shell, id: ItemId, input: &str) {
        let url = navigation::classify(input)
            .expect("test navigation must be valid")
            .to_string();
        shell.handle(Command::Navigate {
            id,
            input: input.into(),
        });
        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: url.clone(),
        }));
        present_committed(shell, id, &url);
    }

    fn present_committed(shell: &mut Shell, id: ItemId, url: &str) {
        static NEXT_PRESENTATION: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(10_000);
        let navigation = NEXT_PRESENTATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        shell.handle(Command::Engine(presentation_pending(
            id,
            NavigationPresentationId::from_raw(navigation),
            url,
        )));
    }

    fn commit_url(shell: &mut Shell, id: ItemId, url: &str) {
        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: url.into(),
        }));
    }

    fn presentation_pending(
        id: ItemId,
        navigation: NavigationPresentationId,
        url: &str,
    ) -> EngineEvent {
        EngineEvent::PresentationPending {
            id,
            navigation,
            url: url.into(),
        }
    }

    fn presentation_ready(
        id: ItemId,
        navigation: NavigationPresentationId,
        url: &str,
    ) -> EngineEvent {
        EngineEvent::PresentationReady {
            id,
            navigation,
            url: url.into(),
        }
    }

    fn first_probing_discard(shell: &Shell) -> (ItemId, DiscardProbeId) {
        shell
            .discard_probes
            .iter()
            .find_map(|(id, state)| match state {
                PendingDiscardProbe::Probing { probe, .. } => Some((*id, *probe)),
                PendingDiscardProbe::Closing { .. } => None,
            })
            .expect("a discard probe must be in flight")
    }

    fn acknowledge_safe_discard(shell: &mut Shell, id: ItemId, probe: DiscardProbeId) {
        let profile = shell.profile_of_item(id).unwrap();
        shell.handle(Command::Engine(EngineEvent::DiscardSafety {
            id,
            probe,
            can_discard: true,
        }));
        assert!(matches!(
            shell.discard_probes.get(&id),
            Some(PendingDiscardProbe::Closing { probe: pending, .. }) if *pending == probe
        ));
        shell.handle(Command::Engine(EngineEvent::ViewDiscarded {
            id,
            profile,
            probe,
        }));
    }

    #[test]
    fn committed_url_projection_precedes_immediate_exact_presentation() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://example.test/".into(),
        });
        let navigation = NavigationPresentationId::from_raw(41);
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        assert!(shell.items.tab(id).is_some_and(TabState::has_view));
        shell.items.set_loading(id, true);
        commit_url(&mut shell, id, "https://example.test/");
        assert_eq!(
            last(&screen)
                .tabs
                .into_iter()
                .find(|tab| tab.id == id.to_string())
                .and_then(|tab| tab.url),
            None,
            "the ordinary URL fact must retain the real New Tab projection"
        );

        shell.handle(Command::Engine(presentation_pending(
            id,
            navigation,
            "https://example.test/",
        )));
        assert_eq!(
            last(&screen)
                .tabs
                .into_iter()
                .find(|tab| tab.id == id.to_string())
                .and_then(|tab| tab.url),
            Some("https://example.test/".into())
        );
        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 41")));

        // Finished is an idempotent re-drive, not a second reveal or a
        // first-paint prerequisite.
        shell.handle(Command::Engine(presentation_ready(
            id,
            navigation,
            "https://example.test/",
        )));

        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| *call == &format!("present {id} 41"))
                .count(),
            1
        );
        assert!(queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .presentation_deadlines
            .is_empty());
    }

    #[test]
    fn raw_presentation_waits_for_exact_privileged_chrome_callback() {
        let (mut shell, engine, chrome, screen) = setup_with_async_chrome();
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://verified.example/".into(),
        });
        assert!(shell.items.tab(id).is_some_and(TabState::has_view));
        let layout_calls_before_commit = engine
            .calls()
            .iter()
            .filter(|call| call.starts_with("layout@"))
            .count();
        commit_url(&mut shell, id, "https://verified.example/");
        assert_eq!(
            shell
                .items
                .tab(id)
                .and_then(|tab| tab.url.as_ref())
                .map(url::Url::as_str),
            Some("https://verified.example/")
        );
        let navigation = NavigationPresentationId::from_raw(4201);

        shell.handle(Command::Engine(presentation_pending(
            id,
            navigation,
            "https://verified.example/",
        )));

        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with("layout@"))
                .count(),
            layout_calls_before_commit,
            "the real New Tab frame remains allocated until exact chrome verification"
        );
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 4201")));
        let presentation = chrome.presentations().pop().unwrap();
        assert_eq!(presentation.id, id);
        assert_eq!(presentation.url, "https://verified.example/");
        assert_eq!(presentation.active, Some(id));
        assert_eq!(
            presentation.tab.url.as_deref(),
            Some("https://verified.example/")
        );
        assert!(queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .presentation_deadlines
            .contains_key(&id));

        chrome.complete_next(true);
        let callback = queue
            .try_recv()
            .expect("callback must enter the actor queue");
        assert!(matches!(
            &callback,
            Command::ChromePresentationApplied {
                id: observed,
                navigation: observed_navigation,
                url,
                active: Some(observed_active),
                projection_revision: _,
                applied: true,
            } if *observed == id
                && *observed_navigation == navigation
                && url == "https://verified.example/"
                && *observed_active == id
        ));
        shell.handle(callback);

        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with("layout@"))
                .count(),
            layout_calls_before_commit + 1
        );
        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 4201")));
        assert!(!shell.pending_presentations.contains_key(&id));
        assert!(!queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .presentation_deadlines
            .contains_key(&id));
    }

    #[test]
    fn newer_same_tab_projection_invalidates_a_queued_chrome_success_callback() {
        let (mut shell, engine, chrome, screen) = setup_with_async_chrome();
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://ordered.example/".into(),
        });
        commit_url(&mut shell, id, "https://ordered.example/");
        let navigation = NavigationPresentationId::from_raw(4_211);
        shell.handle(Command::Engine(presentation_pending(
            id,
            navigation,
            "https://ordered.example/",
        )));
        let first = chrome.presentations().into_iter().next().unwrap();
        let hard_deadline = shell.pending_presentations[&id].hard_deadline;

        // The native eval reports success, but its callback has not yet
        // reached the actor. Model every generic projection source that may
        // run in that interval; each remains New-Tab-masked yet advances this
        // item's exact emitted revision.
        chrome.complete_next(true);
        shell.handle(Command::Engine(EngineEvent::NavState {
            id,
            can_go_back: true,
            can_go_forward: false,
        }));
        shell.handle(Command::Engine(EngineEvent::LoadingChanged {
            id,
            loading: true,
        }));
        shell.handle(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "New document title".into(),
        }));
        shell.project_items();
        let latest_revision = shell
            .last_tab_projection_revision
            .borrow()
            .get(&id)
            .cloned()
            .unwrap();
        assert!(latest_revision > first.tab.projection_revision);
        let masked = last(&screen)
            .tabs
            .into_iter()
            .find(|tab| tab.id == id.to_string())
            .unwrap();
        assert_eq!(masked.url, None);
        assert_eq!(masked.title, "New Tab");

        let stale_success = queue.try_recv().unwrap();
        shell.handle(stale_success);
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 4211")));
        assert!(shell.pending_presentations.contains_key(&id));

        // The existing exact timer reprojects the newest authoritative tab.
        shell.on_presentation_fallback(id, navigation, hard_deadline);
        let retry = chrome.presentations().into_iter().next().unwrap();
        assert!(retry.tab.projection_revision > latest_revision);
        assert_eq!(retry.tab.title, "New document title");
        chrome.complete_next(true);
        shell.handle(queue.try_recv().unwrap());

        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 4211")));
        assert!(!shell.pending_presentations.contains_key(&id));
    }

    #[test]
    fn overlapping_privileged_callbacks_cannot_acknowledge_the_newer_document() {
        let (mut shell, engine, chrome, screen) = setup_with_async_chrome();
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://first.example/".into(),
        });
        assert!(shell.items.tab(id).is_some_and(TabState::has_view));
        let retired = NavigationPresentationId::from_raw(4202);
        let current = NavigationPresentationId::from_raw(4203);

        commit_url(&mut shell, id, "https://first.example/");
        shell.handle(Command::Engine(presentation_pending(
            id,
            retired,
            "https://first.example/",
        )));
        commit_url(&mut shell, id, "https://second.example/");
        shell.handle(Command::Engine(presentation_pending(
            id,
            current,
            "https://second.example/",
        )));
        assert_eq!(chrome.presentations().len(), 2);
        let revisions = chrome
            .presentations()
            .into_iter()
            .map(|presentation| presentation.tab.projection_revision)
            .collect::<Vec<_>>();
        assert!(revisions[0] < revisions[1]);

        let first = chrome.complete_next(true);
        assert_eq!(first.navigation, retired);
        shell.handle(queue.try_recv().unwrap());
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 4202")));
        assert_eq!(shell.pending_presentations[&id].navigation, current);

        let second = chrome.complete_next(true);
        assert_eq!(second.navigation, current);
        shell.handle(queue.try_recv().unwrap());
        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 4203")));
    }

    #[test]
    fn lost_privileged_callback_keeps_one_exact_retry_and_never_timeout_reveals() {
        let (mut shell, engine, chrome, screen) = setup_with_async_chrome();
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://lost-callback.example/".into(),
        });
        assert!(shell.items.tab(id).is_some_and(TabState::has_view));
        let navigation = NavigationPresentationId::from_raw(4204);
        commit_url(&mut shell, id, "https://lost-callback.example/");
        shell.handle(Command::Engine(presentation_pending(
            id,
            navigation,
            "https://lost-callback.example/",
        )));
        let first = shell.pending_presentations[&id].clone();
        assert!(first.chrome_request_in_flight);

        shell.on_presentation_fallback(id, navigation, first.hard_deadline);
        assert_eq!(chrome.presentations().len(), 2);
        assert_eq!(shell.pending_presentations[&id].admission_rejections, 1);
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 4204")));

        let expired = std::time::Instant::now();
        shell
            .pending_presentations
            .get_mut(&id)
            .unwrap()
            .hard_deadline = expired;
        shell.on_presentation_fallback(id, navigation, expired);
        assert!(!shell.items.tab(id).unwrap().has_view());
        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("close {id}")));
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 4204")));
    }

    #[test]
    fn cross_origin_commit_neutralizes_prior_title_before_exact_presentation_ack() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "https://trusted.example/account");
        shell.handle(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "Trusted Account".into(),
        }));
        assert_eq!(shell.items.tab(id).unwrap().title, "Trusted Account");

        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://redirected.example/login".into(),
        }));
        let tab = shell.items.tab(id).unwrap();
        assert_eq!(
            tab.url.as_ref().map(url::Url::as_str),
            Some("https://redirected.example/login")
        );
        assert_eq!(tab.title, "redirected.example");

        let navigation = NavigationPresentationId::from_raw(410);
        shell.handle(Command::Engine(presentation_pending(
            id,
            navigation,
            "https://redirected.example/login",
        )));
        shell.handle(Command::Engine(presentation_ready(
            id,
            navigation,
            "https://redirected.example/login",
        )));
        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 410")));
        assert_eq!(shell.items.tab(id).unwrap().title, "redirected.example");

        shell.handle(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "Redirected Login".into(),
        }));
        assert_eq!(shell.items.tab(id).unwrap().title, "Redirected Login");
    }

    #[test]
    fn same_origin_history_url_observation_preserves_current_document_title() {
        let (mut shell, _engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "https://same.example/first");
        shell.handle(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "Same document title".into(),
        }));

        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://same.example/second#state".into(),
        }));
        assert_eq!(shell.items.tab(id).unwrap().title, "Same document title");
    }

    #[test]
    fn loading_state_never_delays_exact_committed_url_presentation() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://example.test/".into(),
        });
        shell.items.set_loading(id, true);
        let navigation = NavigationPresentationId::from_raw(42);
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        commit_url(&mut shell, id, "https://example.test/");

        shell.handle(Command::Engine(presentation_pending(
            id,
            navigation,
            "https://example.test/",
        )));

        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 42")));
        assert!(!shell.pending_presentations.contains_key(&id));
        assert!(queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .presentation_deadlines
            .is_empty());
    }

    #[test]
    fn presentation_hard_limit_retires_hidden_content_instead_of_timeout_revealing() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://example.test/".into(),
        });
        shell.items.set_loading(id, true);
        let navigation = NavigationPresentationId::from_raw(43);
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        commit_url(&mut shell, id, "https://example.test/");
        let hard_deadline = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(1))
            .unwrap_or_else(std::time::Instant::now);
        shell.pending_presentations.insert(
            id,
            PendingPresentation {
                navigation,
                url: "https://example.test/".into(),
                hard_deadline,
                admission_rejections: 0,
                chrome_applied: true,
                chrome_request_in_flight: false,
            },
        );

        shell.on_presentation_fallback(id, navigation, hard_deadline);

        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("present {id} 43")));
        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("close {id}")));
        assert!(!shell.items.tab(id).unwrap().has_view());
        assert!(!shell.pending_presentations.contains_key(&id));
        assert!(!queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .presentation_deadlines
            .contains_key(&id));
    }

    #[test]
    fn ready_presentation_rejection_retains_and_retries_the_exact_obligation() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://example.test/".into(),
        });
        let navigation = NavigationPresentationId::from_raw(431);
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        engine
            .reject_native_dispatch
            .store(true, std::sync::atomic::Ordering::Release);
        commit_url(&mut shell, id, "https://example.test/");

        // Model bounded event coalescing where Ready replaces Pending. The
        // exact reveal obligation must still be materialized before dispatch.
        shell.handle(Command::Engine(presentation_ready(
            id,
            navigation,
            "https://example.test/",
        )));

        let pending = shell.pending_presentations[&id].clone();
        assert_eq!(pending.navigation, navigation);
        assert_eq!(pending.admission_rejections, 1);
        assert_eq!(
            queue
                .inner
                .timer_state
                .lock()
                .unwrap()
                .presentation_deadlines[&id]
                .navigation,
            navigation
        );

        engine
            .reject_native_dispatch
            .store(false, std::sync::atomic::Ordering::Release);
        queue.cancel_presentation(id);
        shell.on_presentation_fallback(id, navigation, pending.hard_deadline);

        assert!(!shell.pending_presentations.contains_key(&id));
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| *call == &format!("present {id} 431"))
                .count(),
            1,
            "a rejected first layout must not attempt native reveal"
        );
    }

    #[test]
    fn fallback_presentation_rejection_retains_and_retries_the_exact_obligation() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://example.test/".into(),
        });
        shell.items.set_loading(id, false);
        let navigation = NavigationPresentationId::from_raw(432);
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        commit_url(&mut shell, id, "https://example.test/");
        engine
            .reject_native_dispatch
            .store(true, std::sync::atomic::Ordering::Release);
        shell.handle(Command::Engine(presentation_pending(
            id,
            navigation,
            "https://example.test/",
        )));
        let hard_deadline = shell.pending_presentations[&id].hard_deadline;

        assert_eq!(shell.pending_presentations[&id].admission_rejections, 1);
        assert_eq!(
            queue
                .inner
                .timer_state
                .lock()
                .unwrap()
                .presentation_deadlines[&id]
                .navigation,
            navigation
        );

        engine
            .reject_native_dispatch
            .store(false, std::sync::atomic::Ordering::Release);
        queue.cancel_presentation(id);
        shell.on_presentation_fallback(id, navigation, hard_deadline);

        assert!(!shell.pending_presentations.contains_key(&id));
        assert!(!queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .presentation_deadlines
            .contains_key(&id));
    }

    #[test]
    fn permanently_rejected_presentation_retires_the_exact_hidden_view() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://example.test/".into(),
        });
        shell.items.set_loading(id, false);
        let navigation = NavigationPresentationId::from_raw(433);
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        commit_url(&mut shell, id, "https://example.test/");
        engine
            .reject_native_dispatch
            .store(true, std::sync::atomic::Ordering::Release);
        shell.handle(Command::Engine(presentation_pending(
            id,
            navigation,
            "https://example.test/",
        )));
        let hard_deadline = shell.pending_presentations[&id].hard_deadline;

        assert_eq!(shell.pending_presentations[&id].admission_rejections, 1);
        for rejection in 2..=MAX_PRESENTATION_ADMISSION_REJECTIONS {
            // Model each coalesced timer wake entering the actor. There can be
            // only one timer and one shell obligation for this item.
            queue.cancel_presentation(id);
            shell.on_presentation_fallback(id, navigation, hard_deadline);
            if rejection < MAX_PRESENTATION_ADMISSION_REJECTIONS {
                assert_eq!(
                    shell.pending_presentations[&id].admission_rejections,
                    rejection
                );
                assert_eq!(
                    queue
                        .inner
                        .timer_state
                        .lock()
                        .unwrap()
                        .presentation_deadlines
                        .len(),
                    1
                );
            }
        }

        assert!(!shell.pending_presentations.contains_key(&id));
        assert!(!shell.items.tab(id).unwrap().has_view());
        assert!(!queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .presentation_deadlines
            .contains_key(&id));
        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("close {id}")));
    }

    #[test]
    fn unsupported_acknowledgement_is_an_exact_view_lifecycle_failure() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://example.test/".into(),
        });
        let navigation = NavigationPresentationId::from_raw(434);
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        engine
            .unsupported_presentation
            .store(true, std::sync::atomic::Ordering::Release);
        commit_url(&mut shell, id, "https://example.test/");

        shell.handle(Command::Engine(presentation_ready(
            id,
            navigation,
            "https://example.test/",
        )));

        assert!(!shell.pending_presentations.contains_key(&id));
        assert!(!shell.items.tab(id).unwrap().has_view());
        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("close {id}")));
    }

    #[test]
    fn retryable_shutdown_rearms_a_presentation_wake_rejected_by_the_barrier() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://example.test/".into(),
        });
        let navigation = NavigationPresentationId::from_raw(435);
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        commit_url(&mut shell, id, "https://example.test/");
        engine
            .reject_native_dispatch
            .store(true, std::sync::atomic::Ordering::Release);
        shell.handle(Command::Engine(presentation_pending(
            id,
            navigation,
            "https://example.test/",
        )));
        let pending = shell.pending_presentations[&id].clone();

        let (barrier_ack, _barrier_done) = sync_channel(1);
        queue
            .try_push(Command::Shutdown {
                deadline: test_shutdown_deadline(),
                ack: barrier_ack,
            })
            .unwrap_or_else(|_| panic!("shutdown barrier must enter its reserved slot"));
        assert!(matches!(queue.try_recv(), Some(Command::Shutdown { .. })));

        // Model wait_for_timer removing the entry before the corresponding
        // command discovers that the actor is sealed behind the barrier.
        queue.cancel_presentation(id);
        assert!(matches!(
            queue.try_push(Command::PresentationFallback {
                id,
                navigation,
                hard_deadline: pending.hard_deadline,
            }),
            Err(TryPushError::Sealed(_))
        ));
        assert!(!queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .presentation_deadlines
            .contains_key(&id));

        let (retry_ack, retry_done) = sync_channel(1);
        shell.retryable_shutdown_failure(retry_ack);

        assert_eq!(
            retry_done.recv().unwrap(),
            ShutdownOutcome::RetryableFailure
        );
        assert_eq!(shell.pending_presentations[&id], pending);
        assert_eq!(
            queue
                .inner
                .timer_state
                .lock()
                .unwrap()
                .presentation_deadlines[&id]
                .navigation,
            navigation
        );
    }

    #[test]
    fn escaped_stale_presentation_fallback_cannot_rearm_over_a_new_navigation() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "https://example.test/".into(),
        });
        shell.items.set_loading(id, true);
        let retired = NavigationPresentationId::from_raw(44);
        let current = NavigationPresentationId::from_raw(45);
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        engine
            .reject_native_dispatch
            .store(true, std::sync::atomic::Ordering::Release);

        commit_url(&mut shell, id, "https://first.example/");
        shell.handle(Command::Engine(presentation_pending(
            id,
            retired,
            "https://first.example/",
        )));
        let retired_hard = shell.pending_presentations[&id].hard_deadline;
        commit_url(&mut shell, id, "https://second.example/");
        shell.handle(Command::Engine(presentation_pending(
            id,
            current,
            "https://second.example/",
        )));
        let current_pending = shell.pending_presentations[&id].clone();
        let calls_before_stale_wake = engine.calls().len();

        shell.on_presentation_fallback(id, retired, retired_hard);

        assert_eq!(shell.pending_presentations[&id], current_pending);
        assert_eq!(
            queue
                .inner
                .timer_state
                .lock()
                .unwrap()
                .presentation_deadlines[&id]
                .navigation,
            current
        );
        assert_eq!(engine.calls().len(), calls_before_stale_wake);
    }

    #[test]
    fn presentation_timer_is_bounded_per_item_and_duplicate_token_keeps_earliest_deadline() {
        let queue = CommandQueue::new();
        let id = ItemId::from(7);
        let first = NavigationPresentationId::from_raw(1);
        let second = NavigationPresentationId::from_raw(2);
        let now = std::time::Instant::now();
        let first_wake = now + std::time::Duration::from_secs(1);
        let first_hard = now + std::time::Duration::from_secs(5);
        queue.schedule_presentation(id, first, first_wake, first_hard);
        queue.schedule_presentation(
            id,
            first,
            now + std::time::Duration::from_secs(2),
            now + std::time::Duration::from_secs(6),
        );
        {
            let timer = queue.inner.timer_state.lock().unwrap();
            assert_eq!(timer.presentation_deadlines.len(), 1);
            assert_eq!(
                timer.presentation_deadlines[&id],
                PresentationDeadline {
                    wake: first_wake,
                    hard: first_hard,
                    navigation: first,
                }
            );
        }

        queue.schedule_presentation(
            id,
            second,
            now + std::time::Duration::from_secs(3),
            now + std::time::Duration::from_secs(7),
        );
        assert_eq!(
            queue
                .inner
                .timer_state
                .lock()
                .unwrap()
                .presentation_deadlines[&id],
            PresentationDeadline {
                wake: first_wake,
                hard: first_hard,
                navigation: second,
            }
        );
        let shortened_hard = now + std::time::Duration::from_secs(4);
        queue.schedule_presentation(id, second, now, shortened_hard);
        assert!(matches!(
            queue.wait_for_timer(now + std::time::Duration::from_secs(1)),
            TimerWake::Presentation {
                id: observed,
                navigation,
                hard_deadline,
            } if observed == id && navigation == second && hard_deadline == shortened_hard
        ));
        assert!(queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .presentation_deadlines
            .is_empty());

        // A wake for the retired navigation may escape the timer lock before
        // the replacement is scheduled. Queue backpressure must not let that
        // old wake overwrite the replacement obligation while re-arming.
        let replacement_wake = now + std::time::Duration::from_secs(4);
        let replacement_hard = now + std::time::Duration::from_secs(8);
        queue.schedule_presentation(id, second, replacement_wake, replacement_hard);
        queue.retry_presentation(
            id,
            first,
            now + std::time::Duration::from_millis(25),
            now + std::time::Duration::from_secs(9),
        );
        assert_eq!(
            queue
                .inner
                .timer_state
                .lock()
                .unwrap()
                .presentation_deadlines[&id],
            PresentationDeadline {
                wake: replacement_wake,
                hard: replacement_hard,
                navigation: second,
            }
        );
    }

    #[test]
    fn completed_presentation_coalesces_after_its_committed_url_not_before_it() {
        let id = ItemId::from(7);
        let navigation = NavigationPresentationId::from_raw(9);
        let mut commands = VecDeque::new();
        enqueue(
            &mut commands,
            Command::Engine(EngineEvent::UrlChanged {
                id,
                url: "https://final.example/".into(),
            }),
            NORMAL_COMMAND_CAPACITY,
            true,
        )
        .unwrap();
        enqueue(
            &mut commands,
            Command::Engine(EngineEvent::PresentationPending {
                id,
                navigation,
                url: "https://final.example/".into(),
            }),
            NORMAL_COMMAND_CAPACITY,
            true,
        )
        .unwrap();
        enqueue(
            &mut commands,
            Command::Engine(EngineEvent::PresentationReady {
                id,
                navigation,
                url: "https://final.example/".into(),
            }),
            NORMAL_COMMAND_CAPACITY,
            true,
        )
        .unwrap();

        assert_eq!(commands.len(), 2);
        assert!(matches!(
            commands.pop_front(),
            Some(Command::Engine(EngineEvent::UrlChanged { id: observed, .. }))
                if observed == id
        ));
        assert!(matches!(
            commands.pop_front(),
            Some(Command::Engine(EngineEvent::PresentationReady {
                id: observed,
                navigation: observed_navigation,
                ..
            })) if observed == id && observed_navigation == navigation
        ));
    }

    #[test]
    fn stale_presentation_fallback_cannot_replace_authoritative_ready_event() {
        let id = ItemId::from(7);
        let retired = NavigationPresentationId::from_raw(8);
        let current = NavigationPresentationId::from_raw(9);
        let mut commands = VecDeque::new();
        enqueue(
            &mut commands,
            Command::Engine(EngineEvent::PresentationPending {
                id,
                navigation: current,
                url: "https://current.example/".into(),
            }),
            NORMAL_COMMAND_CAPACITY,
            true,
        )
        .unwrap();
        enqueue(
            &mut commands,
            Command::Engine(EngineEvent::PresentationReady {
                id,
                navigation: current,
                url: "https://current.example/".into(),
            }),
            NORMAL_COMMAND_CAPACITY,
            true,
        )
        .unwrap();

        // Model the old wake escaping wait_for_timer and arriving after the
        // replacement navigation's Pending + Ready burst.
        enqueue(
            &mut commands,
            Command::PresentationFallback {
                id,
                navigation: retired,
                hard_deadline: std::time::Instant::now(),
            },
            NORMAL_COMMAND_CAPACITY,
            false,
        )
        .unwrap();

        assert_eq!(commands.len(), 2);
        assert!(matches!(
            commands.pop_front(),
            Some(Command::Engine(EngineEvent::PresentationReady {
                id: observed,
                navigation: observed_navigation,
                ..
            })) if observed == id && observed_navigation == current
        ));
        assert!(matches!(
            commands.pop_front(),
            Some(Command::PresentationFallback {
                id: observed,
                navigation: observed_navigation,
                ..
            }) if observed == id && observed_navigation == retired
        ));
    }

    #[test]
    fn navigate_creates_and_shows_active_tab() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);

        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });

        assert!(engine
            .calls()
            .contains(&format!("create {id} https://example.com/ [default]")));
        assert_eq!(engine.last_layout(), vec![id.to_string()]);

        let active = id.to_string();
        let tab = last(&screen)
            .tabs
            .into_iter()
            .find(|t| t.id == active)
            .unwrap();
        assert_eq!(tab.url, None, "an intent is not a committed page");
        assert!(!tab.loading, "native load events own the loading state");

        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://example.com/".into(),
        }));
        present_committed(&mut shell, id, "https://example.com/");
        let tab = last(&screen)
            .tabs
            .into_iter()
            .find(|t| t.id == active)
            .unwrap();
        assert_eq!(tab.url.as_deref(), Some("https://example.com/"));
    }

    #[test]
    fn rejected_navigation_never_replaces_the_displayed_or_persisted_url() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "a.example");

        engine
            .reject_navigation_dispatch
            .store(true, std::sync::atomic::Ordering::Release);
        shell.handle(Command::Navigate {
            id,
            input: "b.example".into(),
        });

        let tab = last(&screen)
            .tabs
            .into_iter()
            .find(|tab| tab.id == id.to_string())
            .unwrap();
        assert_eq!(tab.url.as_deref(), Some("https://a.example/"));
        let saved = store.saved.lock().unwrap().clone().unwrap();
        let PersistedKind::Tab { url, .. } =
            &saved.items.iter().find(|item| item.id == id).unwrap().kind
        else {
            panic!("committed tab must remain persisted")
        };
        assert_eq!(url, "https://a.example/");
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call.contains("https://b.example/")));
    }

    #[test]
    fn native_navigation_failure_is_correlated_and_never_commits_intent() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "a.example");

        shell.handle(Command::Navigate {
            id,
            input: "b.example".into(),
        });
        let old_request = engine.last_navigation_request();
        shell.handle(Command::Navigate {
            id,
            input: "c.example".into(),
        });
        let current_request = engine.last_navigation_request();

        shell.handle(Command::Engine(EngineEvent::NavigationFailed {
            id,
            request: old_request,
        }));
        shell.handle(Command::Engine(EngineEvent::NavigationFailed {
            id,
            request: current_request,
        }));
        let tab = last(&screen)
            .tabs
            .into_iter()
            .find(|tab| tab.id == id.to_string())
            .unwrap();
        assert_eq!(tab.url.as_deref(), Some("https://a.example/"));
    }

    #[test]
    fn invalid_committed_url_events_have_no_state_or_history_side_effects() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, _engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "a.example");
        store.visits.lock().unwrap().clear();

        shell.handle(Command::Navigate {
            id,
            input: "b.example".into(),
        });
        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "file:///etc/passwd".into(),
        }));

        let tab = last(&screen)
            .tabs
            .into_iter()
            .find(|tab| tab.id == id.to_string())
            .unwrap();
        assert_eq!(tab.url.as_deref(), Some("https://a.example/"));
        assert!(store.visits.lock().unwrap().is_empty());

        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://b.example/".into(),
        }));
        let tab = last(&screen)
            .tabs
            .into_iter()
            .find(|tab| tab.id == id.to_string())
            .unwrap();
        assert_eq!(tab.url.as_deref(), Some("https://b.example/"));
    }

    #[test]
    fn rejected_view_creation_rolls_back_the_live_view_synchronously() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        engine
            .reject_create_dispatch
            .store(true, std::sync::atomic::Ordering::Release);

        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });

        let tab = shell.items.tab(id).unwrap();
        assert!(!tab.has_view());
        assert!(tab.url.is_none());
        assert!(!tab.loading);
        assert_eq!(tab.title, "Page failed to open");
    }

    #[test]
    fn engine_events_fold_into_projection() {
        let (mut shell, _engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });

        shell.handle(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "Example".into(),
        }));
        shell.handle(Command::Engine(EngineEvent::LoadingChanged {
            id,
            loading: false,
        }));

        let key = id.to_string();
        let tab = last(&screen)
            .tabs
            .into_iter()
            .find(|t| t.id == key)
            .unwrap();
        assert_eq!(tab.title, "Example");
        assert!(!tab.loading);
    }

    #[test]
    fn failed_native_view_can_be_retried() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });
        assert!(shell.items.tab(id).unwrap().has_view());

        shell.handle(Command::Engine(EngineEvent::ViewCreationFailed { id }));
        let tab = shell.items.tab(id).unwrap();
        assert!(!tab.has_view());
        assert!(!tab.loading);
        assert_eq!(tab.title, "Page failed to open");

        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with(&format!("create {id} ")))
                .count(),
            2
        );
    }

    #[test]
    fn switching_tabs_shows_only_active() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });
        shell.handle(Command::Open);
        let second = active_id(&screen);
        shell.handle(Command::Navigate {
            id: second,
            input: "github.com".into(),
        });
        assert_eq!(engine.last_layout(), vec![second.to_string()]);

        shell.handle(Command::Activate(first));
        assert_eq!(engine.last_layout(), vec![first.to_string()]);
    }

    #[test]
    fn forged_runtime_ids_cannot_cross_the_focused_space_or_profile() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let local = active_id(&screen);
        shell.handle(Command::Navigate {
            id: local,
            input: "local.example".into(),
        });
        let (profile, space, window) = shell
            .windows
            .focused()
            .map(|win| (win.profile, win.space, win.id))
            .unwrap();

        let sibling_space = SpaceId::from(9001);
        assert!(shell.spaces.insert(Space {
            id: sibling_space,
            profile,
            name: "Sibling".into(),
        }));
        let foreign_profile = ProfileId::from(9002);
        let foreign_space = SpaceId::from(9003);
        assert!(shell.profiles.insert(Profile {
            id: foreign_profile,
            name: "Foreign".into(),
            kind: ProfileKind::Named,
        }));
        assert!(shell.spaces.insert(Space {
            id: foreign_space,
            profile: foreign_profile,
            name: "Foreign".into(),
        }));

        let sibling = ItemId::from(9004);
        let foreign = ItemId::from(9005);
        assert!(shell.items.insert_tab(
            sibling,
            Placement::Space {
                space: sibling_space,
                section: SpaceSection::Today,
            },
        ));
        assert!(shell.items.insert_tab(
            foreign,
            Placement::Space {
                space: foreign_space,
                section: SpaceSection::Today,
            },
        ));

        let focused = shell.windows.get(window).unwrap();
        let region = layout::compute(focused.size, focused.mode, focused.metrics, true)
            .content
            .unwrap();
        let (drop_x, drop_y) = (region.x + 1.0, region.y + region.height / 2.0);
        assert!(shell.resolve_drop(drop_x, drop_y).is_some());

        for (attacker, attacker_space) in [(sibling, sibling_space), (foreign, foreign_space)] {
            let calls_before = engine.calls();
            let roots_before = shell
                .items
                .roots(Placement::Space {
                    space: attacker_space,
                    section: SpaceSection::Today,
                })
                .len();

            shell.handle(Command::Activate(attacker));
            shell.handle(Command::Navigate {
                id: attacker,
                input: "attacker.example".into(),
            });
            shell.handle(Command::Reload(attacker));
            shell.handle(Command::SplitWith {
                other: attacker,
                axis: Axis::Row,
            });
            shell.handle(Command::DropTab {
                id: attacker,
                x: drop_x,
                y: drop_y,
            });
            shell.handle(Command::Engine(EngineEvent::NewWindowRequested {
                id: attacker,
                url: "https://popup.example/".into(),
            }));
            shell.handle(Command::Close(attacker));

            assert_eq!(shell.windows.focused().unwrap().active, Some(local));
            assert!(shell.windows.focused().unwrap().splits.is_none());
            assert!(shell.items.get(attacker).is_some());
            assert!(shell.items.tab(attacker).unwrap().url.is_none());
            assert_eq!(
                shell
                    .items
                    .roots(Placement::Space {
                        space: attacker_space,
                        section: SpaceSection::Today,
                    })
                    .len(),
                roots_before,
                "a foreign popup source must not create a tab in its space"
            );
            assert_eq!(engine.calls(), calls_before);
        }

        assert_eq!(active_id(&screen), local);
        assert_eq!(shell.windows.focused().unwrap().space, space);
        assert_eq!(shell.windows.focused().unwrap().profile, profile);
    }

    #[test]
    fn split_shows_both_panes_close_collapses() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });
        shell.handle(Command::Open);
        let second = active_id(&screen);
        shell.handle(Command::Navigate {
            id: second,
            input: "github.com".into(),
        });

        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });
        let panes = engine.last_layout();
        assert_eq!(panes.len(), 2);
        assert!(panes.contains(&first.to_string()) && panes.contains(&second.to_string()));

        // closing one pane collapses the split onto the other
        shell.handle(Command::Close(first));
        assert_eq!(engine.last_layout(), vec![second.to_string()]);
    }

    #[test]
    fn background_views_are_not_discarded_without_safety_signals() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let mut ids = vec![active_id(&screen)];
        shell.handle(Command::Navigate {
            id: ids[0],
            input: "site0.com".into(),
        });
        for n in 1..15 {
            shell.handle(Command::Open);
            let id = active_id(&screen);
            shell.handle(Command::Navigate {
                id,
                input: format!("site{n}.com"),
            });
            ids.push(id);
        }
        // 15 tabs but none idle: everything stays warm
        assert!(ids
            .iter()
            .all(|id| shell.items.tab(*id).unwrap().has_view()));

        shell.handle(Command::Tick);
        assert!(ids
            .iter()
            .all(|id| shell.items.tab(*id).unwrap().has_view()));
        assert!(!engine.calls().iter().any(|call| call.starts_with("close ")));
    }

    #[test]
    fn crashed_page_recreates_once_then_reports() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "example.com");

        // First crash: the engine already removed the dead native generation;
        // the focused tab is recreated once without an id-only cleanup close.
        shell.handle(Command::Engine(EngineEvent::Crashed { id }));
        assert!(!engine.calls().iter().any(|c| c == &format!("close {id}")));
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with(&format!("create {id} ")))
                .count(),
            2
        );

        // Second crash right after: no renderer-spawn loop, the tab reports it.
        shell.handle(Command::Engine(EngineEvent::Crashed { id }));
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with(&format!("create {id} ")))
                .count(),
            2
        );
        let tab = last(&screen)
            .tabs
            .into_iter()
            .find(|t| t.id == id.to_string())
            .unwrap();
        assert_eq!(tab.title, "Page crashed");
        assert!(!tab.loading);
    }

    #[test]
    fn crash_presentation_never_overwrites_the_durable_document_title() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, _engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "durable-title.example");
        shell.handle(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "Last real title".into(),
        }));
        let profile = shell.windows.focused().unwrap().profile;

        shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
            profile,
            ids: vec![id],
        }));
        let projected = last(&screen)
            .tabs
            .into_iter()
            .find(|tab| tab.id == id.to_string())
            .unwrap();
        assert_eq!(projected.title, "Page crashed");

        let saved = store.saved.lock().unwrap().clone().unwrap();
        let durable_title = saved.items.into_iter().find_map(|item| {
            (item.id == id)
                .then_some(item.kind)
                .and_then(|kind| match kind {
                    PersistedKind::Tab { title, .. } => Some(title),
                    PersistedKind::Folder { .. } => None,
                })
        });
        assert_eq!(durable_title.as_deref(), Some("Last real title"));
    }

    #[test]
    fn hidden_renderer_crash_does_not_close_a_future_generation_or_relaunch() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let hidden = active_id(&screen);
        shell.handle(Command::Navigate {
            id: hidden,
            input: "hidden.example".into(),
        });
        shell.handle(Command::Open);
        let active = active_id(&screen);
        shell.handle(Command::Navigate {
            id: active,
            input: "active.example".into(),
        });
        store.events.lock().unwrap().clear();
        let creates_before = engine
            .calls()
            .iter()
            .filter(|call| call.starts_with(&format!("create {hidden} ")))
            .count();

        shell.handle(Command::Engine(EngineEvent::Crashed { id: hidden }));
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("close {hidden}")));
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with(&format!("create {hidden} ")))
                .count(),
            creates_before
        );
        assert!(store.events.lock().unwrap().is_empty());
        assert!(!shell.items.tab(hidden).unwrap().has_view());
    }

    #[test]
    fn profile_process_exit_recreates_focused_view_only_once() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let hidden = active_id(&screen);
        navigate_and_commit(&mut shell, hidden, "example.com");
        shell.handle(Command::Open);
        let visible = active_id(&screen);
        navigate_and_commit(&mut shell, visible, "github.com");
        let profile = shell.profile_of_item(visible).unwrap();

        shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
            profile,
            ids: vec![hidden, visible],
        }));
        assert!(!shell.items.tab(hidden).unwrap().has_view());
        assert!(shell.items.tab(visible).unwrap().has_view());
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with(&format!("create {visible} ")))
                .count(),
            2
        );

        // A second failure in the burst does not enter a rebuild loop.
        shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
            profile,
            ids: vec![visible],
        }));
        assert!(!shell.items.tab(visible).unwrap().has_view());
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with(&format!("create {visible} ")))
                .count(),
            2
        );
    }

    #[test]
    fn profile_process_exit_defers_hidden_recovery_until_activation() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let hidden = active_id(&screen);
        navigate_and_commit(&mut shell, hidden, "hidden.example");
        shell.handle(Command::Open);
        let active = active_id(&screen);
        navigate_and_commit(&mut shell, active, "active.example");
        let profile = shell.profile_of_item(active).unwrap();
        let before = engine
            .calls()
            .iter()
            .filter(|call| call.starts_with("create "))
            .count();

        shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
            profile,
            ids: vec![hidden, active],
        }));
        let after_failure = engine
            .calls()
            .iter()
            .filter(|call| call.starts_with("create "))
            .count();
        assert_eq!(after_failure, before + 1, "only the focused tab recovers");
        assert!(!shell.items.tab(hidden).unwrap().has_view());

        shell.handle(Command::Activate(hidden));
        assert!(shell.items.tab(hidden).unwrap().has_view());
        let after_activation = engine
            .calls()
            .iter()
            .filter(|call| call.starts_with("create "))
            .count();
        assert_eq!(after_activation, after_failure + 1);
    }

    #[test]
    fn profile_process_exit_recreates_every_visible_split_leaf() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "first.example");
        shell.handle(Command::Open);
        let second = active_id(&screen);
        navigate_and_commit(&mut shell, second, "second.example");
        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });
        let profile = shell.profile_of_item(second).unwrap();

        shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
            profile,
            ids: vec![first, second],
        }));

        assert!(shell.items.tab(first).unwrap().has_view());
        assert!(shell.items.tab(second).unwrap().has_view());
        assert_eq!(engine.last_layout().len(), 2);
        for id in [first, second] {
            assert_eq!(
                engine
                    .calls()
                    .iter()
                    .filter(|call| call.starts_with(&format!("create {id} ")))
                    .count(),
                2
            );
        }
    }

    #[test]
    fn multi_leaf_process_recovery_accounts_for_the_whole_optimistic_batch() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "recovery0.example");
        let mut ids = vec![first];
        for index in 1..MAX_VISIBLE_PANES {
            shell.handle(Command::Open);
            let id = active_id(&screen);
            navigate_and_commit(&mut shell, id, &format!("recovery{index}.example"));
            ids.push(id);
        }
        let tree = ids[1..]
            .iter()
            .fold(Pane::Leaf(first), |tree, id| Pane::Branch {
                axis: Axis::Row,
                ratio: 0.5,
                a: Box::new(tree),
                b: Box::new(Pane::Leaf(*id)),
            });
        shell.windows.focused_mut().unwrap().splits = Some(tree);
        let profile = shell.profile_of_item(first).unwrap();
        let creates_before = engine
            .calls()
            .iter()
            .filter(|call| call.starts_with("create "))
            .count();

        shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
            profile,
            ids: ids.clone(),
        }));

        assert_eq!(shell.items.view_ids().len(), MAX_VISIBLE_PANES);
        assert!(shell.items.view_ids().len() <= LIVE_VIEW_ABSOLUTE_LIMIT);
        assert!(ids
            .iter()
            .all(|id| shell.items.tab(*id).is_some_and(TabState::has_view)));
        assert_eq!(engine.last_layout().len(), MAX_VISIBLE_PANES);
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with("create "))
                .count(),
            creates_before + MAX_VISIBLE_PANES,
            "later optimistic leaves must not make the first recovery create appear over budget"
        );
    }

    #[test]
    fn repeated_crash_leaf_temporarily_collapses_to_a_live_split_sibling() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "first.example");
        shell.handle(Command::Open);
        let second = active_id(&screen);
        navigate_and_commit(&mut shell, second, "second.example");
        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });
        let profile = shell.profile_of_item(second).unwrap();
        shell.crashes.insert(second, std::time::Instant::now());

        shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
            profile,
            ids: vec![first, second],
        }));

        assert_eq!(shell.windows.focused().unwrap().active, Some(first));
        assert!(shell.items.tab(first).unwrap().has_view());
        assert!(!shell.items.tab(second).unwrap().has_view());
        assert_eq!(engine.last_layout(), vec![first.to_string()]);
        assert!(shell.windows.focused().unwrap().splits.is_some());
    }

    #[test]
    fn linked_tab_shows_alone_and_split_group_survives() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });
        shell.handle(Command::Open);
        let second = active_id(&screen);
        shell.handle(Command::Navigate {
            id: second,
            input: "github.com".into(),
        });
        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });
        assert_eq!(engine.last_layout().len(), 2);

        // page JS opens a link: the popup shows alone
        shell.handle(Command::Engine(EngineEvent::NewWindowRequested {
            id: second,
            url: "https://wikipedia.org/".into(),
        }));
        assert_eq!(engine.last_layout().len(), 1);

        // returning to a member restores the whole group
        shell.handle(Command::Activate(first));
        assert_eq!(engine.last_layout().len(), 2);
    }

    #[test]
    fn hidden_idle_views_go_dormant_and_wake_on_show() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });
        shell.handle(Command::Open);
        let second = active_id(&screen);
        shell.handle(Command::Navigate {
            id: second,
            input: "github.com".into(),
        });

        // nothing is idle yet: no dormancy requested
        assert!(!engine.calls().iter().any(|c| c.starts_with("dormant")));

        // once idle, the hidden view suspends; the shown one does not
        shell.dormant_min = std::time::Duration::ZERO;
        shell.handle(Command::Tick);
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("dormant {first}")));

        // refocusing moves dormancy to the other tab
        shell.handle(Command::Activate(first));
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("dormant {second}")));
        let calls = engine.calls();
        let layout = calls
            .iter()
            .rposition(|c| c.starts_with("layout@"))
            .unwrap();
        let dormant = calls
            .iter()
            .rposition(|c| c == &format!("dormant {second}"))
            .unwrap();
        assert!(layout < dormant, "a view must be hidden before suspension");
    }

    #[test]
    fn live_view_budget_discards_only_exactly_acknowledged_hidden_pages() {
        let (mut shell, _engine, screen) = setup();
        shell.live_view_soft_limit = 2;
        shell.live_view_pressure_limit = 3;
        shell.discard_idle_min = std::time::Duration::ZERO;
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "budget0.example");
        for n in 1..6 {
            shell.handle(Command::Open);
            let id = active_id(&screen);
            navigate_and_commit(&mut shell, id, &format!("budget{n}.example"));
        }
        assert_eq!(shell.items.view_ids().len(), 6);

        while shell.items.view_ids().len() > shell.live_view_soft_limit {
            let (id, probe) = first_probing_discard(&shell);
            acknowledge_safe_discard(&mut shell, id, probe);
        }

        assert_eq!(shell.items.view_ids().len(), 2);
        assert_eq!(
            shell
                .items
                .view_ids()
                .into_iter()
                .filter(|id| shell.discard_protected_leaves().contains(id))
                .count(),
            1
        );
    }

    #[test]
    fn unsafe_page_is_exempt_from_budget_and_reprobe_is_cooled_down() {
        let (mut shell, engine, screen) = setup();
        shell.live_view_soft_limit = 1;
        shell.live_view_pressure_limit = 1;
        shell.discard_idle_min = std::time::Duration::ZERO;
        shell.handle(Command::Bootstrap);
        let protected = active_id(&screen);
        navigate_and_commit(&mut shell, protected, "dirty.example");
        shell.handle(Command::Open);
        let active = active_id(&screen);
        navigate_and_commit(&mut shell, active, "active.example");
        let (id, probe) = first_probing_discard(&shell);
        assert_eq!(id, protected);

        shell.handle(Command::Engine(EngineEvent::DiscardSafety {
            id,
            probe,
            can_discard: false,
        }));
        shell.handle(Command::Tick);

        assert_eq!(shell.items.view_ids().len(), 2);
        assert!(shell.discard_protected_until.contains_key(&protected));
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with(&format!("probe-discard {protected} ")))
                .count(),
            1
        );
    }

    #[test]
    fn missing_probe_callback_times_out_fail_closed_with_bounded_state() {
        let (mut shell, _engine, screen) = setup();
        shell.live_view_soft_limit = 1;
        shell.live_view_pressure_limit = 1;
        shell.discard_idle_min = std::time::Duration::ZERO;
        shell.handle(Command::Bootstrap);
        let candidate = active_id(&screen);
        navigate_and_commit(&mut shell, candidate, "timeout.example");
        shell.handle(Command::Open);
        let active = active_id(&screen);
        navigate_and_commit(&mut shell, active, "active.example");
        let (id, probe) = first_probing_discard(&shell);

        shell.handle(Command::DiscardProbeTimeout { id, probe });

        assert!(shell.items.tab(candidate).unwrap().has_view());
        assert!(shell.discard_probes.is_empty());
        assert!(shell.discard_protected_until.contains_key(&candidate));
    }

    #[test]
    fn common_tab_counts_probe_before_and_never_exceed_the_resident_view_ceiling() {
        for count in [1_usize, 10, 24, 25, 32, 33, 50, 100] {
            let (mut shell, engine, screen) = setup();
            shell.handle(Command::Bootstrap);
            let first = active_id(&screen);
            navigate_and_commit(&mut shell, first, "count0.example");
            for index in 1..count {
                shell.handle(Command::Open);
                let id = active_id(&screen);
                shell.handle(Command::Navigate {
                    id,
                    input: format!("count{index}.example"),
                });
                if shell.items.tab(id).is_some_and(TabState::has_view) {
                    shell.handle(Command::Engine(EngineEvent::UrlChanged {
                        id,
                        url: format!("https://count{index}.example/"),
                    }));
                }
            }
            assert_eq!(last(&screen).tabs.len(), count, "tabs remain in the model");
            assert_eq!(
                shell.items.view_ids().len(),
                count.min(LIVE_VIEW_ABSOLUTE_LIMIT)
            );
            assert_eq!(
                engine
                    .calls()
                    .iter()
                    .filter(|call| call.starts_with("create "))
                    .count(),
                count.min(LIVE_VIEW_ABSOLUTE_LIMIT),
                "native creates must stop at the logical ceiling"
            );
            let expected_probes = if count.min(LIVE_VIEW_ABSOLUTE_LIMIT) > LIVE_VIEW_PRESSURE_LIMIT
            {
                MAX_CONCURRENT_DISCARD_PROBES
            } else {
                0
            };
            assert_eq!(
                shell.discard_probes.len(),
                expected_probes,
                "pressure probing must start before absolute admission is exhausted"
            );
        }
    }

    #[test]
    fn resident_ceiling_rejects_excess_operation_without_removing_the_tab() {
        let (mut shell, _engine, screen, operations) =
            setup_with_operation_log(Arc::new(FakeStore::default()));
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "ceiling0.example");
        for index in 1..LIVE_VIEW_ABSOLUTE_LIMIT {
            shell.handle(Command::Open);
            let id = active_id(&screen);
            navigate_and_commit(&mut shell, id, &format!("ceiling{index}.example"));
        }
        shell.handle(Command::Open);
        let excess = active_id(&screen);
        shell.handle(Command::Operation {
            operation_id: "resident-limit".into(),
            command: Box::new(Command::Navigate {
                id: excess,
                input: "excess.example".into(),
            }),
        });

        assert_eq!(last(&screen).tabs.len(), LIVE_VIEW_ABSOLUTE_LIMIT + 1);
        assert!(!shell.items.tab(excess).unwrap().has_view());
        assert_eq!(shell.items.view_ids().len(), LIVE_VIEW_ABSOLUTE_LIMIT);
        assert_eq!(
            operations.lock().unwrap().last().unwrap().outcome,
            OperationOutcome::NativeAdmissionFailed
        );
        assert_eq!(
            operations.lock().unwrap().last().unwrap().reason,
            OperationReason::NativeDispatchRejected
        );
    }

    #[test]
    fn optimistic_multi_leaf_batch_admits_available_slots_in_effect_order() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "baseline0.example");
        for index in 1..(LIVE_VIEW_ABSOLUTE_LIMIT - 1) {
            shell.handle(Command::Open);
            let id = active_id(&screen);
            navigate_and_commit(&mut shell, id, &format!("baseline{index}.example"));
        }
        assert_eq!(shell.items.view_ids().len(), LIVE_VIEW_ABSOLUTE_LIMIT - 1);

        let space = shell.windows.focused().unwrap().space;
        let mut effects = Vec::new();
        let mut leaves = Vec::new();
        for index in 0..MAX_VISIBLE_PANES {
            let id = ItemId::from(10_000 + index as u128);
            assert!(shell.items.insert_tab(
                id,
                Placement::Space {
                    space,
                    section: SpaceSection::Today,
                },
            ));
            effects.extend(shell.items.navigate(id, &format!("batch{index}.example")));
            leaves.push(id);
        }
        assert_eq!(
            shell.items.view_ids().len(),
            LIVE_VIEW_ABSOLUTE_LIMIT - 1 + MAX_VISIBLE_PANES,
            "all batch effects have optimistically set their logical bits"
        );

        let native = shell.apply(effects);

        assert!(native.scheduled);
        assert!(native.rejected);
        assert!(shell.items.tab(leaves[0]).unwrap().has_view());
        assert!(leaves[1..]
            .iter()
            .all(|id| !shell.items.tab(*id).unwrap().has_view()));
        assert_eq!(shell.items.view_ids().len(), LIVE_VIEW_ABSOLUTE_LIMIT);
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with("create "))
                .count(),
            LIVE_VIEW_ABSOLUTE_LIMIT,
            "the first batch leaf uses the one free slot; later leaves are rolled back"
        );
    }

    #[test]
    fn stale_or_navigation_cancelled_discard_probe_cannot_close_a_view() {
        let (mut shell, engine, screen) = setup();
        shell.live_view_soft_limit = 1;
        shell.live_view_pressure_limit = 1;
        shell.discard_idle_min = std::time::Duration::ZERO;
        shell.handle(Command::Bootstrap);
        let candidate = active_id(&screen);
        navigate_and_commit(&mut shell, candidate, "old.example");
        shell.handle(Command::Open);
        let active = active_id(&screen);
        navigate_and_commit(&mut shell, active, "active.example");
        let (_, stale) = first_probing_discard(&shell);

        navigate_and_commit(&mut shell, candidate, "new.example");
        shell.handle(Command::Engine(EngineEvent::DiscardSafety {
            id: candidate,
            probe: stale,
            can_discard: true,
        }));
        shell.handle(Command::Engine(EngineEvent::ViewDiscarded {
            id: candidate,
            profile: shell.profile_of_item(candidate).unwrap(),
            probe: stale,
        }));

        assert!(shell.items.tab(candidate).unwrap().has_view());
        assert_eq!(
            shell
                .items
                .tab(candidate)
                .unwrap()
                .url
                .as_ref()
                .unwrap()
                .host_str(),
            Some("new.example")
        );
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call.starts_with(&format!("discard {candidate} "))));
    }

    #[test]
    fn every_currently_visible_split_leaf_is_outside_the_discard_candidate_set() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "left.example");
        shell.handle(Command::Open);
        let second = active_id(&screen);
        navigate_and_commit(&mut shell, second, "right.example");
        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });
        shell.live_view_soft_limit = 0;
        shell.live_view_pressure_limit = 0;
        shell.discard_idle_min = std::time::Duration::ZERO;
        shell.handle(Command::Tick);

        assert!(shell.items.tab(first).unwrap().has_view());
        assert!(shell.items.tab(second).unwrap().has_view());
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call.starts_with("probe-discard ")));
    }

    #[test]
    fn acknowledged_discard_preserves_url_and_activation_recreates_lazily() {
        let (mut shell, engine, screen) = setup();
        shell.live_view_soft_limit = 1;
        shell.live_view_pressure_limit = 1;
        shell.discard_idle_min = std::time::Duration::ZERO;
        shell.handle(Command::Bootstrap);
        let sleeping = active_id(&screen);
        navigate_and_commit(&mut shell, sleeping, "sleeping.example/path");
        shell.handle(Command::Open);
        let active = active_id(&screen);
        navigate_and_commit(&mut shell, active, "active.example");
        let (_, probe) = first_probing_discard(&shell);
        acknowledge_safe_discard(&mut shell, sleeping, probe);

        assert!(!shell.items.tab(sleeping).unwrap().has_view());
        assert_eq!(
            shell
                .items
                .tab(sleeping)
                .unwrap()
                .url
                .as_ref()
                .unwrap()
                .as_str(),
            "https://sleeping.example/path"
        );
        shell.handle(Command::Activate(sleeping));
        assert!(shell.items.tab(sleeping).unwrap().has_view());
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with(&format!("create {sleeping} ")))
                .count(),
            2
        );
    }

    #[test]
    fn minimized_window_hides_before_dormancy_and_wakes_focused_view() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "first.example".into(),
        });
        shell.handle(Command::Open);
        let second = active_id(&screen);
        shell.handle(Command::Navigate {
            id: second,
            input: "second.example".into(),
        });
        shell.dormant_min = std::time::Duration::ZERO;

        shell.handle(Command::SetWindowVisible(false));
        assert!(engine.last_layout().is_empty());
        let calls = engine.calls();
        let hidden = calls
            .iter()
            .rposition(|call| call.starts_with("layout@") && call.ends_with(' '))
            .unwrap();
        let dormant = calls
            .iter()
            .rposition(|call| call.starts_with("dormant "))
            .unwrap();
        assert!(hidden < dormant, "controllers must hide before suspension");
        let latest_dormant = &calls[dormant];
        assert!(latest_dormant.contains(&first.to_string()));
        assert!(latest_dormant.contains(&second.to_string()));

        shell.handle(Command::SetWindowVisible(true));
        assert_eq!(engine.last_layout(), vec![second.to_string()]);
        assert_eq!(
            engine
                .calls()
                .iter()
                .rev()
                .find(|call| call.starts_with("dormant "))
                .unwrap(),
            &format!("dormant {first}")
        );
    }

    #[test]
    fn split_created_views_are_not_unsafely_discarded() {
        let (mut shell, _engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "left.com".into(),
        });
        shell.handle(Command::Open);
        let second = active_id(&screen);
        shell.handle(Command::Navigate {
            id: second,
            input: "right.com".into(),
        });
        shell.handle(Command::Activate(first));
        // splitting creates second's view without focusing it
        shell.handle(Command::SplitWith {
            other: second,
            axis: Axis::Row,
        });
        shell.handle(Command::Unsplit);
        // Even under tab pressure the never-focused split view may contain
        // unsaved renderer state, so it remains resident.
        for n in 0..13 {
            shell.handle(Command::Open);
            let id = active_id(&screen);
            shell.handle(Command::Navigate {
                id,
                input: format!("warm{n}.com"),
            });
        }
        assert!(shell.items.tab(second).unwrap().has_view());
    }

    #[test]
    fn split_layout_enforces_native_renderer_ceiling() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let anchor = active_id(&screen);
        shell.handle(Command::Navigate {
            id: anchor,
            input: "anchor.example".into(),
        });

        for n in 0..MAX_VISIBLE_PANES {
            shell.handle(Command::Open);
            let other = active_id(&screen);
            shell.handle(Command::Navigate {
                id: other,
                input: format!("pane{n}.example"),
            });
            shell.handle(Command::Activate(anchor));
            shell.handle(Command::SplitWith {
                other,
                axis: Axis::Row,
            });
        }

        assert_eq!(engine.last_layout().len(), MAX_VISIBLE_PANES);
        assert_eq!(
            shell
                .windows
                .focused()
                .and_then(|window| window.splits.as_ref())
                .map(|tree| tree.tabs().len()),
            Some(MAX_VISIBLE_PANES)
        );
    }

    #[test]
    fn split_group_survives_tab_switches() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });
        shell.handle(Command::Open);
        let second = active_id(&screen);
        shell.handle(Command::Navigate {
            id: second,
            input: "github.com".into(),
        });
        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });
        assert_eq!(engine.last_layout().len(), 2);

        // a fresh tab shows alone without dissolving the group
        shell.handle(Command::Open);
        let third = active_id(&screen);
        shell.handle(Command::Navigate {
            id: third,
            input: "wikipedia.org".into(),
        });
        assert_eq!(engine.last_layout(), vec![third.to_string()]);

        // returning to a member brings the whole group back
        shell.handle(Command::Activate(first));
        let panes = engine.last_layout();
        assert_eq!(panes.len(), 2);
        assert!(panes.contains(&first.to_string()) && panes.contains(&second.to_string()));
    }

    #[test]
    fn asynchronous_split_leaf_creation_failure_collapses_and_cannot_resurrect() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "first.example");
        shell.handle(Command::Open);
        let failed = active_id(&screen);
        navigate_and_commit(&mut shell, failed, "failed.example");
        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });
        assert_eq!(engine.last_layout().len(), 2);

        shell.handle(Command::Engine(EngineEvent::ViewCreationFailed {
            id: failed,
        }));

        let window = shell.windows.focused().unwrap();
        assert_eq!(window.active, Some(first));
        assert!(window
            .splits
            .as_ref()
            .is_none_or(|tree| !tree.contains(failed)));
        assert_eq!(engine.last_layout(), vec![first.to_string()]);
        let SessionLoad::Loaded(saved) = store.load_session() else {
            panic!("failed-leaf collapse must be durable");
        };
        assert!(saved
            .splits
            .as_ref()
            .is_none_or(|tree| !tree.contains(failed)));

        // Retrying and activating the failed tab is an explicit single-tab
        // transition; it must not revive the topology that owned its failed
        // native construction.
        shell.handle(Command::Navigate {
            id: failed,
            input: "failed.example".into(),
        });
        shell.handle(Command::Activate(failed));
        assert_eq!(engine.last_layout(), vec![failed.to_string()]);
        assert!(shell
            .windows
            .focused()
            .unwrap()
            .splits
            .as_ref()
            .is_none_or(|tree| !tree.contains(failed)));
    }

    #[test]
    fn synchronous_create_refusal_never_commits_split_or_drop_topology() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "first.example");
        shell.handle(Command::Open);
        let failed = active_id(&screen);
        navigate_and_commit(&mut shell, failed, "failed.example");
        shell.items.view_creation_failed(failed);
        shell.handle(Command::Activate(first));
        engine
            .reject_create_dispatch
            .store(true, std::sync::atomic::Ordering::Release);

        let split = shell.operation_split(failed, Axis::Row);
        assert_eq!(split.outcome, OperationOutcome::NativeAdmissionFailed);
        assert!(shell.windows.focused().unwrap().splits.is_none());
        assert_eq!(engine.last_layout(), vec![first.to_string()]);

        let drop = shell.apply_drop(first, failed, Edge::Right);
        assert_eq!(drop.outcome, OperationOutcome::NativeAdmissionFailed);
        assert!(shell.windows.focused().unwrap().splits.is_none());
        assert_eq!(engine.last_layout(), vec![first.to_string()]);
    }

    #[test]
    fn divider_drag_updates_ratio_and_projects_strips() {
        let store = Arc::new(FakeStore::default());
        let engine = Arc::new(FakeEngine::default());
        let strips: Arc<Mutex<Vec<DividerView>>> = Arc::new(Mutex::new(Vec::new()));
        let screen: Screen = Arc::new(Mutex::new(ItemsState {
            projection_revision: String::new(),
            tabs: Vec::new(),
            active: None,
        }));
        let (sink, strip_sink) = (screen.clone(), strips.clone());
        let mut shell = Shell::new(
            engine,
            store.clone(),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |p| match p {
                Projection::Layout(l) => *strip_sink.lock().unwrap() = l.dividers,
                p => apply_projection(&mut sink.lock().unwrap(), p),
            }),
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "example.com");
        shell.handle(Command::Open);
        let second = active_id(&screen);
        navigate_and_commit(&mut shell, second, "github.com");
        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });

        let before = strips.lock().unwrap().clone();
        assert_eq!(before.len(), 1);
        assert!(before[0].vertical);

        let (cx, cy) = (before[0].x + before[0].width / 2.0, before[0].y + 10.0);
        shell.handle(Command::DividerGrab { x: cx, y: cy });
        shell.handle(Command::SetWindowSize(Size::new(1600.0, 900.0)));
        assert!(shell.divider.is_some(), "resize preserves pointer capture");
        let resized = strips.lock().unwrap().clone();
        assert_eq!(resized.len(), 1);
        let (resized_x, resized_y) = (resized[0].x + resized[0].width / 2.0, resized[0].y + 10.0);
        shell.handle(Command::DividerRelease {
            x: Some(resized_x - 100.0),
            y: Some(resized_y),
        });

        let after = strips.lock().unwrap().clone();
        assert_eq!(after.len(), 1);
        assert!(
            after[0].x < resized[0].x - 50.0,
            "the captured path is resolved against resized geometry"
        );

        let SessionLoad::Loaded(saved) = store.load_session() else {
            panic!("release persists the split")
        };
        let Some(Pane::Branch { ratio, .. }) = saved.splits else {
            panic!("split persisted");
        };
        assert!(ratio < 0.5);
    }

    #[test]
    fn topology_collapse_revokes_a_captured_divider_before_its_path_can_alias() {
        let (mut shell, _engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "first.example");
        shell.handle(Command::Open);
        let second = active_id(&screen);
        navigate_and_commit(&mut shell, second, "second.example");
        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });
        shell.handle(Command::Open);
        let third = active_id(&screen);
        navigate_and_commit(&mut shell, third, "third.example");
        shell.handle(Command::Activate(first));
        shell.handle(Command::SplitWith {
            other: third,
            axis: Axis::Row,
        });

        // The outer root is [second | (first | third)]. Removing `second`
        // promotes the nested branch to root, where the old empty path would
        // otherwise authorize a different divider.
        let win = shell.windows.focused().unwrap();
        let tree = shell.pane_tree().unwrap();
        let content = layout::compute(win.size, win.mode, win.metrics, true)
            .content
            .unwrap();
        let local = Rect::new(0.0, 0.0, content.width, content.height);
        let outer = split::divider_at_path(&tree, local, win.metrics.gap, &[]).unwrap();
        shell.handle(Command::DividerGrab {
            x: content.x + outer.strip.x + outer.strip.width / 2.0,
            y: content.y + outer.strip.y + 10.0,
        });
        assert!(shell.divider.is_some());

        shell.handle(Command::Close(second));
        assert!(shell.divider.is_none(), "topology mutation revokes capture");
        shell.handle(Command::DividerRelease {
            x: Some(content.x + content.width * 0.8),
            y: Some(content.y + 10.0),
        });

        let Pane::Branch { ratio, a, b, .. } = shell.pane_tree().unwrap() else {
            panic!("the promoted first/third split remains live");
        };
        assert_eq!(ratio, 0.5, "stale root path did not mutate the new root");
        assert_eq!(a.tabs(), vec![first]);
        assert_eq!(b.tabs(), vec![third]);
    }

    #[test]
    fn native_divider_event_changes_ratio_only_and_rejects_non_finite_state() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, _engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "first.example");
        shell.handle(Command::Open);
        let second = active_id(&screen);
        navigate_and_commit(&mut shell, second, "second.example");
        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });
        let window = shell.windows.focused().unwrap().id;
        let mut changed = shell.windows.focused().unwrap().splits.clone().unwrap();
        changed.set_ratio(&[], 0.7);
        shell.handle(Command::Engine(EngineEvent::SplitChanged {
            window,
            tree: changed,
        }));
        let Pane::Branch { ratio, .. } = shell.windows.focused().unwrap().splits.as_ref().unwrap()
        else {
            panic!("split remains a branch");
        };
        assert_eq!(*ratio, 0.7);
        assert_eq!(
            store
                .saved
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|state| state.splits.as_ref())
                .and_then(|tree| match tree {
                    Pane::Branch { ratio, .. } => Some(*ratio),
                    Pane::Leaf(_) => None,
                }),
            Some(0.7)
        );

        let mut invalid = shell.windows.focused().unwrap().splits.clone().unwrap();
        let Pane::Branch { ratio, .. } = &mut invalid else {
            unreachable!()
        };
        *ratio = f64::NAN;
        shell.handle(Command::Engine(EngineEvent::SplitChanged {
            window,
            tree: invalid,
        }));
        let Pane::Branch { ratio, .. } = shell.windows.focused().unwrap().splits.as_ref().unwrap()
        else {
            unreachable!()
        };
        assert_eq!(*ratio, 0.7);
    }

    #[test]
    fn native_split_update_cannot_ratify_a_cross_space_tree() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let local = active_id(&screen);
        shell.handle(Command::Navigate {
            id: local,
            input: "local.example".into(),
        });
        let (window, profile) = shell
            .windows
            .focused()
            .map(|win| (win.id, win.profile))
            .unwrap();
        let foreign_space = SpaceId::from(9200);
        let foreign = ItemId::from(9201);
        assert!(shell.spaces.insert(Space {
            id: foreign_space,
            profile,
            name: "Foreign".into(),
        }));
        assert!(shell.items.insert_tab(
            foreign,
            Placement::Space {
                space: foreign_space,
                section: SpaceSection::Today,
            },
        ));

        // Model a corrupted/stale native topology already present in memory.
        // A ratio callback with matching topology must still fail scope
        // validation rather than blessing and persisting the foreign leaf.
        let invalid = Pane::Branch {
            axis: Axis::Row,
            ratio: 0.5,
            a: Box::new(Pane::Leaf(local)),
            b: Box::new(Pane::Leaf(foreign)),
        };
        shell.windows.get_mut(window).unwrap().splits = Some(invalid.clone());
        let mut candidate = invalid;
        candidate.set_ratio(&[], 0.7);
        shell.handle(Command::Engine(EngineEvent::SplitChanged {
            window,
            tree: candidate,
        }));

        let Pane::Branch { ratio, .. } = shell
            .windows
            .get(window)
            .and_then(|win| win.splits.as_ref())
            .unwrap()
        else {
            panic!("test corruption remains unchanged")
        };
        assert_eq!(*ratio, 0.5);
        assert_eq!(engine.last_layout(), vec![local.to_string()]);
    }

    #[test]
    fn restart_preserves_ids_actives_and_splits() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, _engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "example.com");
        shell.handle(Command::Open);
        let second = active_id(&screen);
        navigate_and_commit(&mut shell, second, "github.com");
        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });

        let before = last(&screen);

        let (mut shell2, engine2, screen2) = setup_with(store);
        shell2.handle(Command::Bootstrap);
        let after = last(&screen2);

        // same ULIDs, same order, same active tab survive the restart
        let ids = |s: &ItemsState| s.tabs.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&after), ids(&before));
        assert_eq!(after.active, before.active);
        // the split tree is restored and both panes get views again
        let panes = engine2.last_layout();
        assert_eq!(panes.len(), 2);
        assert!(panes.contains(&first.to_string()) && panes.contains(&second.to_string()));
    }

    #[test]
    fn bootstrap_never_creates_views_for_foreign_focus_or_split_references() {
        let local_profile = ProfileId::from(9100);
        let foreign_profile = ProfileId::from(9101);
        let local_space = SpaceId::from(9102);
        let sibling_space = SpaceId::from(9103);
        let foreign_space = SpaceId::from(9104);
        let local = ItemId::from(9105);
        let sibling = ItemId::from(9106);
        let foreign = ItemId::from(9107);
        let tab = |id, space, host: &str| PersistedItem {
            id,
            parent: None,
            placement: Placement::Space {
                space,
                section: SpaceSection::Today,
            },
            kind: PersistedKind::Tab {
                url: format!("https://{host}/"),
                title: host.into(),
                zoom: 1.0,
            },
        };
        let store = Arc::new(FakeStore {
            saved: Mutex::new(Some(SessionState {
                profiles: vec![
                    PersistedProfile {
                        id: local_profile,
                        name: "Local".into(),
                        kind: ProfileKind::Default,
                    },
                    PersistedProfile {
                        id: foreign_profile,
                        name: "Foreign".into(),
                        kind: ProfileKind::Named,
                    },
                ],
                spaces: vec![
                    PersistedSpace {
                        id: local_space,
                        profile: local_profile,
                        name: "Local".into(),
                    },
                    PersistedSpace {
                        id: sibling_space,
                        profile: local_profile,
                        name: "Sibling".into(),
                    },
                    PersistedSpace {
                        id: foreign_space,
                        profile: foreign_profile,
                        name: "Foreign".into(),
                    },
                ],
                items: vec![
                    tab(local, local_space, "local.example"),
                    tab(sibling, sibling_space, "sibling.example"),
                    tab(foreign, foreign_space, "foreign.example"),
                ],
                active_space: Some(local_space),
                active_item: Some(foreign),
                splits: Some(Pane::Branch {
                    axis: Axis::Row,
                    ratio: 0.5,
                    a: Box::new(Pane::Leaf(local)),
                    b: Box::new(Pane::Leaf(sibling)),
                }),
            })),
            ..Default::default()
        });

        let (mut shell, engine, screen) = setup_with(store);
        shell.handle(Command::Bootstrap);

        let win = shell.windows.focused().unwrap();
        assert_eq!(win.profile, local_profile);
        assert_eq!(win.space, local_space);
        assert_eq!(win.active, Some(local));
        assert!(win.splits.is_none());
        assert_eq!(active_id(&screen), local);
        assert_eq!(engine.last_layout(), vec![local.to_string()]);
        let calls = engine.calls();
        assert!(calls
            .iter()
            .any(|call| call.starts_with(&format!("create {local} "))));
        assert!(!calls
            .iter()
            .any(|call| call.starts_with(&format!("create {sibling} "))));
        assert!(!calls
            .iter()
            .any(|call| call.starts_with(&format!("create {foreign} "))));
    }

    #[test]
    fn restart_discards_oversized_split_before_creating_views() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, _engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let mut ids = vec![active_id(&screen)];
        navigate_and_commit(&mut shell, ids[0], "pane0.example");
        for n in 1..=MAX_VISIBLE_PANES {
            shell.handle(Command::Open);
            let id = active_id(&screen);
            navigate_and_commit(&mut shell, id, &format!("pane{n}.example"));
            ids.push(id);
        }

        let mut state = store.saved.lock().unwrap().clone().unwrap();
        state.active_item = Some(ids[0]);
        state.splits = Some(
            ids[1..]
                .iter()
                .fold(Pane::Leaf(ids[0]), |tree, id| Pane::Branch {
                    axis: Axis::Row,
                    ratio: 0.5,
                    a: Box::new(tree),
                    b: Box::new(Pane::Leaf(*id)),
                }),
        );
        *store.saved.lock().unwrap() = Some(state);

        let (mut restored, engine, _screen) = setup_with(store);
        restored.handle(Command::Bootstrap);
        assert_eq!(engine.last_layout(), vec![ids[0].to_string()]);
        assert!(restored.windows.focused().unwrap().splits.is_none());
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with("create "))
                .count(),
            1,
            "oversized restored panes must not trigger a controller storm"
        );
    }

    #[test]
    fn run_commands_drive_tabs_zoom_and_engine() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });

        shell.handle(Command::Run("tab.new".into()));
        let second = active_id(&screen);
        assert_ne!(first, second);

        shell.handle(Command::Run("tab.next".into()));
        assert_eq!(active_id(&screen), first);
        shell.handle(Command::Run("tab.previous".into()));
        assert_eq!(active_id(&screen), second);

        shell.handle(Command::Run("tab.close".into()));
        assert_eq!(active_id(&screen), first);

        shell.handle(Command::Run("zoom.in".into()));
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("zoom {first} 1.1")));
        shell.handle(Command::Run("zoom.reset".into()));
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("zoom {first} 1")));
    }

    #[test]
    fn zoom_stays_out_of_authoritative_state_until_exact_native_settlement() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "zoom-state.example");
        assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);
        assert_eq!(persisted_zoom(&store, id), 1.0);

        shell.handle(Command::Run("zoom.in".into()));
        let (_, first_scale, first) = engine.last_zoom_request();
        assert_eq!(first_scale, 1.1);
        assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);
        assert_eq!(persisted_zoom(&store, id), 1.0);

        // Rapid input is based on the pending desired value without making
        // that value part of a full-session snapshot.
        shell.handle(Command::Run("zoom.in".into()));
        let (_, second_scale, second) = engine.last_zoom_request();
        assert!((second_scale - 1.2).abs() < 1e-12);
        assert_ne!(first, second);
        assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);

        shell.handle(Command::Engine(EngineEvent::ZoomSettled {
            id,
            request: first,
            applied_scale: 1.1,
            succeeded: true,
        }));
        assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);
        assert_eq!(persisted_zoom(&store, id), 1.0);

        shell.handle(Command::Engine(EngineEvent::ZoomSettled {
            id,
            request: second,
            applied_scale: 1.2,
            succeeded: true,
        }));
        assert_eq!(shell.items.tab(id).unwrap().zoom, 1.2);
        assert_eq!(persisted_zoom(&store, id), 1.2);
        assert!(!shell.pending_zooms.contains_key(&id));
    }

    #[test]
    fn newest_zoom_failure_reports_the_cumulative_native_scale_after_coalescing() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "zoom-coalesce.example");

        shell.handle(Command::Run("zoom.in".into()));
        let (_, _, first) = engine.last_zoom_request();
        shell.handle(Command::Run("zoom.in".into()));
        let (_, _, second) = engine.last_zoom_request();
        assert_ne!(first, second);

        // The first native call succeeded, but its event was coalesced before
        // the actor observed it. The latest failure still carries 1.1 as the
        // last actually applied native scale, so both model and disk converge.
        shell.handle(Command::Engine(EngineEvent::ZoomSettled {
            id,
            request: second,
            applied_scale: 1.1,
            succeeded: false,
        }));
        assert_eq!(shell.items.tab(id).unwrap().zoom, 1.1);
        assert_eq!(persisted_zoom(&store, id), 1.1);
        assert!(!shell.pending_zooms.contains_key(&id));
    }

    #[test]
    fn exact_malformed_zoom_settlement_retires_only_its_pending_obligation() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "zoom-malformed.example");

        shell.handle(Command::Run("zoom.in".into()));
        let (_, _, request) = engine.last_zoom_request();
        assert!(shell.pending_zooms.contains_key(&id));

        shell.handle(Command::Engine(EngineEvent::ZoomSettled {
            id,
            request,
            applied_scale: f64::NAN,
            succeeded: true,
        }));

        assert!(!shell.pending_zooms.contains_key(&id));
        assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);
        assert_eq!(persisted_zoom(&store, id), 1.0);
    }

    #[test]
    fn recreated_view_zoom_failure_rolls_back_and_terminal_lifecycle_clears_pending() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "zoom-restore.example");

        // Model a persisted scale restored into a newly recreated native view.
        shell.items.set_zoom(id, 1.5);
        shell.items.view_creation_failed(id);
        let effects = shell.items.ensure_view(id);
        shell.apply(effects);
        let (_, requested, request) = engine.last_zoom_request();
        assert_eq!(requested, 1.5);
        shell.handle(Command::Engine(EngineEvent::ZoomSettled {
            id,
            request,
            applied_scale: 1.0,
            succeeded: false,
        }));
        assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);
        assert_eq!(persisted_zoom(&store, id), 1.0);

        shell.handle(Command::Run("zoom.in".into()));
        assert!(shell.pending_zooms.contains_key(&id));
        shell.handle(Command::Engine(EngineEvent::ViewCreationFailed { id }));
        assert!(!shell.pending_zooms.contains_key(&id));
    }

    #[test]
    fn url_focus_emits_ui_command() {
        let engine = Arc::new(FakeEngine::default());
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let mut shell = Shell::new(
            engine,
            Arc::new(FakeStore::default()),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |p| {
                if let Projection::UiCommand(id) = p {
                    sink.lock().unwrap().push(id);
                }
            }),
        );
        shell.handle(Command::Run("url.focus".into()));
        assert_eq!(seen.lock().unwrap().as_slice(), ["url.focus"]);
    }

    fn search_sink() -> (Arc<Mutex<Vec<SearchResults>>>, EmitFn) {
        let seen: Arc<Mutex<Vec<SearchResults>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let emit: EmitFn = Box::new(move |p| {
            if let Projection::Search(r) = p {
                sink.lock().unwrap().push(r);
            }
        });
        (seen, emit)
    }

    #[test]
    fn search_ranks_tabs_primary_action_commands_and_history() {
        let (seen, emit) = search_sink();
        let store = Arc::new(FakeStore {
            history: vec![zephium_core::ports::store::HistoryHit {
                url: "https://blog.example.com/".into(),
                title: "Example Blog".into(),
                last_visit: 1,
            }],
            ..Default::default()
        });
        let mut shell = Shell::new(
            Arc::new(FakeEngine::default()),
            store,
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            emit,
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        shell.handle(Command::Bootstrap);
        let id = shell.windows.focused().and_then(|w| w.active).unwrap();
        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });
        shell.handle(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "Example Site".into(),
        }));

        shell.handle(Command::Search("example".into()));
        let last = seen.lock().unwrap().last().unwrap().clone();
        assert_eq!(last.query, "example");
        let kinds: Vec<&str> = last.results.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, ["tab", "search", "history"]);
        assert!(matches!(
            &last.results[0].action,
            SearchAction::ActivateTab { id: tab } if *tab == id.to_string()
        ));
        // Page-derived image delivery stays absent until a sandboxed broker
        // exists; search projections must not recreate the old protocol URL.
        assert!(last.results[0].favicon.is_none());

        shell.handle(Command::Search("reload".into()));
        let last = seen.lock().unwrap().last().unwrap().clone();
        assert!(last.results.iter().any(|r| r.kind == "command"
            && matches!(&r.action, SearchAction::RunCommand { id } if id == "nav.reload")));

        shell.handle(Command::Search("example.com".into()));
        let last = seen.lock().unwrap().last().unwrap().clone();
        assert!(last.results.iter().any(|r| r.kind == "url"));

        shell.handle(Command::Search("".into()));
        let last = seen.lock().unwrap().last().unwrap().clone();
        assert!(last.results.iter().all(|r| r.kind == "tab"));
    }

    #[test]
    fn stale_history_reply_cannot_replace_a_newer_launcher_query() {
        let (seen, emit) = search_sink();
        let mut shell = Shell::new(
            Arc::new(FakeEngine::default()),
            Arc::new(FakeStore::default()),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            emit,
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        shell.handle(Command::Bootstrap);
        shell.store_reads = Some(StoreReadQueue::new());
        let profile = shell.windows.focused().unwrap().profile;

        shell.handle(Command::Search("old".into()));
        let old_generation = shell.pending_search.as_ref().unwrap().generation;
        shell.handle(Command::Search("new".into()));
        let new_generation = shell.pending_search.as_ref().unwrap().generation;
        shell.handle(Command::StoreRead(StoreReadResult::History {
            generation: old_generation,
            profile,
            query: "old".into(),
            hits: vec![zephium_core::ports::store::HistoryHit {
                url: "https://old.example/".into(),
                title: "Old".into(),
                last_visit: 1,
            }],
        }));
        assert_eq!(
            shell
                .pending_search
                .as_ref()
                .map(|pending| pending.generation),
            Some(new_generation)
        );
        assert_eq!(seen.lock().unwrap().last().unwrap().query, "new");

        shell.handle(Command::StoreRead(StoreReadResult::History {
            generation: new_generation,
            profile,
            query: "new".into(),
            hits: vec![zephium_core::ports::store::HistoryHit {
                url: "https://new.example/".into(),
                title: "New".into(),
                last_visit: 2,
            }],
        }));
        let last = seen.lock().unwrap().last().unwrap().clone();
        assert_eq!(last.query, "new");
        assert!(last.results.iter().any(|result| {
            matches!(&result.action, SearchAction::OpenUrl { url } if url == "https://new.example/")
        }));
        assert!(last.results.iter().all(|result| {
            !matches!(&result.action, SearchAction::OpenUrl { url } if url == "https://old.example/")
        }));
    }

    #[test]
    fn open_url_lands_in_a_new_tab() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::OpenUrl("github.com".into()));
        let second = active_id(&screen);
        assert_ne!(first, second);
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("create {second} https://github.com/ [default]")));
    }

    #[test]
    fn open_url_at_item_limit_never_navigates_the_active_tab() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let active = active_id(&screen);
        navigate_and_commit(&mut shell, active, "kept.example");
        let space = shell.windows.focused().unwrap().space;
        for value in 0..(zephium_core::session::MAX_SESSION_ITEMS - 1) {
            assert!(shell.items.insert_tab(
                ItemId::from(100_000 + value as u128),
                Placement::Space {
                    space,
                    section: SpaceSection::Today,
                },
            ));
        }
        let calls_before = engine.calls().len();

        let completion =
            shell.handle_operation(Command::OpenUrl("must-not-replace.example".into()));

        assert_eq!(completion.outcome, OperationOutcome::Rejected);
        assert_eq!(completion.reason, OperationReason::ItemLimitReached);
        assert_eq!(shell.windows.focused().unwrap().active, Some(active));
        assert_eq!(
            shell
                .items
                .tab(active)
                .and_then(|tab| tab.url.as_ref().map(url::Url::as_str)),
            Some("https://kept.example/")
        );
        assert_eq!(engine.calls().len(), calls_before);
    }

    #[test]
    fn bootstrap_is_idempotent_across_chrome_reloads() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });

        // chrome webview reloaded (dev HMR): same window, same stage, state kept
        shell.handle(Command::Bootstrap);
        assert_eq!(active_id(&screen), first);
        let windows: std::collections::HashSet<String> = engine
            .calls()
            .iter()
            .filter_map(|c| c.split(' ').next().map(String::from))
            .filter(|c| c.starts_with("layout@"))
            .collect();
        assert_eq!(windows.len(), 1, "one window, one stage: {windows:?}");
        assert_eq!(last(&screen).tabs.len(), 1);
    }

    #[test]
    fn profile_deletion_completes_once_only_after_native_and_store_phases() {
        let store = Arc::new(FakeStore::default());
        store
            .authorize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
        store
            .finalize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionFinalizeOutcome::Completed);
        let (mut shell, engine, _screen, operations) = setup_with_operation_log(store.clone());
        shell.handle(Command::Bootstrap);
        let profile = add_inactive_named_profile(&mut shell, 20_000);
        engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);

        shell.handle(delete_operation("delete-1", profile));

        let completions = operations.lock().unwrap().clone();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].operation_id, "delete-1");
        assert_eq!(completions[0].outcome, OperationOutcome::Applied);
        assert_eq!(
            completions[0].reason,
            OperationReason::ProfileDeletionCompleted
        );
        assert!(shell.profiles.get(profile).is_none());
        assert!(store.pending_deletions.lock().unwrap().is_empty());
        let authorized = store.authorized_sessions.lock().unwrap();
        assert_eq!(authorized.len(), 1);
        assert!(authorized[0]
            .1
            .profiles
            .iter()
            .all(|candidate| candidate.id != profile));
        assert_eq!(
            session::canonicalize(authorized[0].1.clone()),
            authorized[0].1
        );
        assert_eq!(
            store.events.lock().unwrap().as_slice(),
            ["authorize-delete", "finalize-delete"]
        );
    }

    #[test]
    fn untracked_profile_deletion_command_is_inert() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, engine, _screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let profile = add_inactive_named_profile(&mut shell, 20_500);

        shell.handle(Command::DeleteProfile(profile));

        assert!(shell.profiles.get(profile).is_some());
        assert!(store.authorized_sessions.lock().unwrap().is_empty());
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("erase-profile {profile}")));
    }

    #[test]
    fn profile_deletion_retry_emits_no_intermediate_or_duplicate_completion() {
        let store = Arc::new(FakeStore::default());
        store
            .authorize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
        store
            .finalize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionFinalizeOutcome::Completed);
        let (mut shell, engine, _screen, operations) = setup_with_operation_log(store);
        shell.handle(Command::Bootstrap);
        let profile = add_inactive_named_profile(&mut shell, 21_000);
        engine.push_erasure_outcomes([
            ProfileDataErasureOutcome::TimedOut,
            ProfileDataErasureOutcome::Verified,
        ]);

        shell.handle(delete_operation("delete-retry", profile));
        assert!(operations.lock().unwrap().is_empty());
        assert!(shell.profiles.get(profile).is_none());
        let generation = shell
            .profile_deletions
            .get(&profile)
            .unwrap()
            .retry_generation;
        shell.handle(Command::ProfileDeletionRetry {
            profile,
            generation,
        });

        let completions = operations.lock().unwrap().clone();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].operation_id, "delete-retry");
        assert_eq!(completions[0].outcome, OperationOutcome::Applied);
    }

    #[test]
    fn duplicate_profile_deletion_is_rejected_without_stealing_original_id() {
        let store = Arc::new(FakeStore::default());
        store
            .authorize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
        store
            .finalize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionFinalizeOutcome::Completed);
        let (mut shell, engine, _screen, operations) = setup_with_operation_log(store);
        shell.handle(Command::Bootstrap);
        let profile = add_inactive_named_profile(&mut shell, 22_000);
        engine
            .hold_erasures
            .store(true, std::sync::atomic::Ordering::Release);

        shell.handle(delete_operation("delete-original", profile));
        shell.handle(delete_operation("delete-duplicate", profile));
        {
            let completions = operations.lock().unwrap();
            assert_eq!(completions.len(), 1);
            assert_eq!(completions[0].operation_id, "delete-duplicate");
            assert_eq!(completions[0].outcome, OperationOutcome::Rejected);
            assert_eq!(
                completions[0].reason,
                OperationReason::ProfileDeletionInProgress
            );
        }

        engine.complete_held_erasure(ProfileDataErasureOutcome::Verified);
        shell.handle(Command::ProfileDeletionReady(profile));
        let completions = operations.lock().unwrap().clone();
        assert_eq!(completions.len(), 2);
        assert_eq!(
            completions
                .iter()
                .filter(|completion| completion.operation_id == "delete-original")
                .count(),
            1
        );
    }

    #[test]
    fn uncertain_profile_deletion_rpcs_reconcile_from_the_journal() {
        let store = Arc::new(FakeStore::default());
        store
            .authorize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionAuthorizeOutcome::OutcomeUnknown);
        store
            .authorize_unknown_commits
            .store(true, std::sync::atomic::Ordering::Release);
        store
            .finalize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionFinalizeOutcome::OutcomeUnknown);
        store
            .finalize_unknown_completes
            .store(true, std::sync::atomic::Ordering::Release);
        let (mut shell, engine, _screen, operations) = setup_with_operation_log(store.clone());
        shell.handle(Command::Bootstrap);
        let profile = add_inactive_named_profile(&mut shell, 23_000);
        engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);

        shell.handle(delete_operation("delete-uncertain", profile));

        let completions = operations.lock().unwrap().clone();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].operation_id, "delete-uncertain");
        assert_eq!(completions[0].outcome, OperationOutcome::Applied);
        assert!(store.pending_deletions.lock().unwrap().is_empty());
    }

    #[test]
    fn uncertain_profile_deletion_reauthorization_rebuilds_the_survivor_snapshot() {
        let store = Arc::new(FakeStore::default());
        store.authorize_outcomes.lock().unwrap().extend([
            ProfileDeletionAuthorizeOutcome::OutcomeUnknown,
            ProfileDeletionAuthorizeOutcome::Authorized,
        ]);
        store
            .finalize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionFinalizeOutcome::Completed);
        let (mut shell, engine, screen, operations) = setup_with_operation_log(store.clone());
        shell.handle(Command::Bootstrap);
        let survivor = active_id(&screen);
        let profile = add_inactive_named_profile(&mut shell, 23_500);
        // Fail the ordered journal read after the first durability-ambiguous
        // authorization. This leaves a real actor-order window in which newer
        // survivor mutations can be accepted before a safe retry.
        store
            .pending_load_failures
            .store(1, std::sync::atomic::Ordering::Release);

        shell.handle(delete_operation("delete-rebuild", profile));
        assert!(operations.lock().unwrap().is_empty());
        navigate_and_commit(&mut shell, survivor, "new-survivor.example");
        engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);
        let generation = shell
            .profile_deletions
            .get(&profile)
            .unwrap()
            .retry_generation;
        shell.handle(Command::ProfileDeletionRetry {
            profile,
            generation,
        });
        // A stale timer/callback cannot complete the already-removed state a
        // second time.
        shell.handle(Command::ProfileDeletionRetry {
            profile,
            generation,
        });
        shell.handle(Command::ProfileDeletionReady(profile));

        let authorized = store.authorized_sessions.lock().unwrap();
        assert_eq!(authorized.len(), 2);
        let retried = &authorized[1].1;
        assert!(retried
            .profiles
            .iter()
            .all(|candidate| candidate.id != profile));
        assert!(retried.items.iter().any(|item| {
            item.id == survivor
                && matches!(
                    &item.kind,
                    PersistedKind::Tab { url, .. } if url == "https://new-survivor.example/"
                )
        }));
        let completions = operations.lock().unwrap();
        assert_eq!(
            completions
                .iter()
                .filter(|completion| completion.operation_id == "delete-rebuild")
                .count(),
            1
        );
        assert_eq!(completions[0].outcome, OperationOutcome::Applied);
    }

    #[test]
    fn delayed_authorization_proof_reschedules_newer_survivor_durability() {
        let store = Arc::new(FakeStore::default());
        store
            .authorize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionAuthorizeOutcome::OutcomeUnknown);
        store
            .authorize_unknown_commits
            .store(true, std::sync::atomic::Ordering::Release);
        store
            .finalize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionFinalizeOutcome::Completed);
        let (mut shell, engine, screen, operations) = setup_with_operation_log(store.clone());
        shell.handle(Command::Bootstrap);
        let survivor = active_id(&screen);
        let profile = add_inactive_named_profile(&mut shell, 23_750);
        store
            .pending_load_failures
            .store(1, std::sync::atomic::Ordering::Release);

        shell.handle(delete_operation("delete-post-barrier", profile));
        navigate_and_commit(&mut shell, survivor, "post-barrier.example");
        engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);
        let generation = shell
            .profile_deletions
            .get(&profile)
            .unwrap()
            .retry_generation;
        shell.handle(Command::ProfileDeletionRetry {
            profile,
            generation,
        });

        let persisted = store.saved.lock().unwrap().clone().unwrap();
        assert!(persisted
            .profiles
            .iter()
            .all(|candidate| candidate.id != profile));
        assert!(persisted.items.iter().any(|item| {
            item.id == survivor
                && matches!(
                    &item.kind,
                    PersistedKind::Tab { url, .. } if url == "https://post-barrier.example/"
                )
        }));
        let completions = operations.lock().unwrap();
        assert_eq!(
            completions
                .iter()
                .filter(|completion| completion.operation_id == "delete-post-barrier")
                .count(),
            1
        );
        assert_eq!(completions[0].outcome, OperationOutcome::Applied);
    }

    #[test]
    fn restart_resumes_journaled_native_erasure_before_creating_views() {
        let store = Arc::new(FakeStore::default());
        store
            .authorize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
        let (mut first, first_engine, screen, first_operations) =
            setup_with_operation_log(store.clone());
        first.handle(Command::Bootstrap);
        let active = active_id(&screen);
        navigate_and_commit(&mut first, active, "survivor.example");
        let profile = add_inactive_named_profile(&mut first, 24_000);
        first_engine.push_erasure_outcomes([ProfileDataErasureOutcome::Failed]);
        first.handle(delete_operation("delete-before-crash", profile));
        assert!(first_operations.lock().unwrap().is_empty());
        assert_eq!(store.pending_deletions.lock().unwrap().len(), 1);
        drop(first);

        store
            .finalize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionFinalizeOutcome::Completed);
        let (mut restarted, engine, _screen) = setup_with(store.clone());
        engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);
        restarted.handle(Command::Bootstrap);

        let calls = engine.calls();
        let erase = calls
            .iter()
            .position(|call| call == &format!("erase-profile {profile}"))
            .unwrap();
        let first_create = calls
            .iter()
            .position(|call| call.starts_with("create "))
            .unwrap();
        assert!(erase < first_create);
        assert!(store.pending_deletions.lock().unwrap().is_empty());
        assert!(restarted.profile_deletions.is_empty());
    }

    #[test]
    fn restart_with_native_proof_skips_engine_and_finishes_local_purge() {
        let store = Arc::new(FakeStore::default());
        let default_profile = ProfileId::from(25_000);
        let default_space = SpaceId::from(25_001);
        *store.saved.lock().unwrap() = Some(SessionState {
            profiles: vec![PersistedProfile {
                id: default_profile,
                name: "Personal".into(),
                kind: ProfileKind::Default,
            }],
            spaces: vec![PersistedSpace {
                id: default_space,
                profile: default_profile,
                name: "Space".into(),
            }],
            active_space: Some(default_space),
            ..SessionState::default()
        });
        let removed = ProfileId::from(25_002);
        store
            .pending_deletions
            .lock()
            .unwrap()
            .push(PendingProfileDeletion {
                profile: removed,
                native_erasure_verified: true,
            });
        store
            .finalize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionFinalizeOutcome::Completed);
        let (mut shell, engine, _screen) = setup_with(store.clone());

        shell.handle(Command::Bootstrap);

        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("erase-profile {removed}")));
        assert!(store.pending_deletions.lock().unwrap().is_empty());
    }

    #[test]
    fn profile_deletion_policy_rejects_default_active_private_and_last_profile() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, _engine, _screen, operations) = setup_with_operation_log(store);
        shell.handle(Command::Bootstrap);
        let default = shell.windows.focused().unwrap().profile;

        shell.handle(delete_operation("delete-default", default));
        let private = ProfileId::from(26_000);
        assert!(shell.profiles.insert(Profile {
            id: private,
            name: "Private".into(),
            kind: ProfileKind::Incognito,
        }));
        shell.handle(delete_operation("delete-private", private));

        let completions = operations.lock().unwrap().clone();
        assert_eq!(completions.len(), 2);
        assert!(completions.iter().all(|completion| {
            completion.outcome == OperationOutcome::Rejected
                && completion.reason == OperationReason::ProfileDeletionPolicyRejected
        }));

        let (mut last_only, _engine, _screen) = setup();
        last_only.bootstrapped = true;
        let only = ProfileId::from(26_100);
        assert!(last_only.profiles.insert(Profile {
            id: only,
            name: "Only".into(),
            kind: ProfileKind::Named,
        }));
        assert!(last_only
            .filtered_session_for_profile_deletion(only)
            .is_none());
    }

    #[test]
    fn exhausted_space_aggregate_refuses_first_run_without_panicking() {
        let (mut shell, _engine, _screen) = setup();
        let profile = ProfileId::from(26_200);
        assert!(shell.profiles.insert(Profile {
            id: profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        }));
        for index in 0..zephium_core::session::MAX_SESSION_SPACES {
            assert!(shell.spaces.insert(Space {
                id: SpaceId::from(27_000 + index as u128),
                profile,
                name: "Full".into(),
            }));
        }

        assert!(shell.create_default_space().is_none());
    }

    #[test]
    fn unavailable_deletion_journal_prevents_bootstrap_and_native_views() {
        let store = Arc::new(FakeStore::default());
        store
            .pending_load_failures
            .store(1, std::sync::atomic::Ordering::Release);
        let (mut shell, engine, screen) = setup_with(store);

        shell.handle(Command::Bootstrap);

        assert!(!shell.bootstrapped);
        assert!(shell.windows.focused().is_none());
        assert!(last(&screen).tabs.is_empty());
        assert!(engine.calls().is_empty());
    }

    #[test]
    fn failed_session_load_never_bootstraps_or_overwrites_storage() {
        let store = Arc::new(FakeStore::default());
        *store.load_failed.lock().unwrap() = true;
        let (mut shell, engine, screen) = setup_with(store.clone());

        shell.handle(Command::Bootstrap);
        shell.persist();

        assert!(!shell.bootstrapped);
        assert!(shell.windows.focused().is_none());
        assert!(last(&screen).tabs.is_empty());
        assert!(!store.events.lock().unwrap().contains(&"save"));
        assert!(engine.calls().is_empty());
    }

    #[test]
    fn recovery_required_never_bootstraps_or_overwrites_storage() {
        let store = Arc::new(FakeStore::default());
        *store.recovery_reason.lock().unwrap() = Some("snapshot is not canonical".into());
        let (mut shell, engine, screen) = setup_with(store.clone());

        shell.handle(Command::Bootstrap);
        shell.persist();

        assert!(!shell.bootstrapped);
        assert!(shell.windows.focused().is_none());
        assert!(last(&screen).tabs.is_empty());
        assert!(!store.events.lock().unwrap().contains(&"save"));
        assert!(engine.calls().is_empty());
    }

    #[test]
    fn degraded_ancillary_profiles_are_recorded_without_blocking_exact_session_bootstrap() {
        let store = Arc::new(FakeStore::default());
        let profile = ProfileId::from(31_000);
        let space = SpaceId::from(31_001);
        *store.saved.lock().unwrap() = Some(SessionState {
            profiles: vec![PersistedProfile {
                id: profile,
                name: "Personal".into(),
                kind: ProfileKind::Default,
            }],
            spaces: vec![PersistedSpace {
                id: space,
                profile,
                name: "Space".into(),
            }],
            active_space: Some(space),
            ..SessionState::default()
        });
        store.degraded_profiles.lock().unwrap().push(profile);
        let (mut shell, _engine, _screen) = setup_with(store);

        shell.handle(Command::Bootstrap);

        assert!(shell.bootstrapped);
        assert_eq!(
            shell.degraded_storage_profiles,
            std::collections::HashSet::from([profile])
        );
        assert!(shell.profiles.get(profile).is_some());
    }

    #[test]
    fn forged_degraded_profile_report_cannot_bootstrap_an_unrelated_session() {
        let store = Arc::new(FakeStore::default());
        let profile = ProfileId::from(32_000);
        let space = SpaceId::from(32_001);
        *store.saved.lock().unwrap() = Some(SessionState {
            profiles: vec![PersistedProfile {
                id: profile,
                name: "Personal".into(),
                kind: ProfileKind::Default,
            }],
            spaces: vec![PersistedSpace {
                id: space,
                profile,
                name: "Space".into(),
            }],
            active_space: Some(space),
            ..SessionState::default()
        });
        store
            .degraded_profiles
            .lock()
            .unwrap()
            .push(ProfileId::from(99_999));
        let (mut shell, engine, _screen) = setup_with(store);

        shell.handle(Command::Bootstrap);

        assert!(!shell.bootstrapped);
        assert!(shell.degraded_storage_profiles.is_empty());
        assert!(engine.calls().is_empty());
    }

    #[test]
    fn favicon_pipeline_accepts_only_fixed_renderer_rasters_for_current_origin() {
        let engine = Arc::new(FakeEngine::default());
        let store = Arc::new(FakeStore::default());
        let net = Arc::new(FakeNet::default());
        let screen: Screen = Arc::new(Mutex::new(ItemsState {
            projection_revision: String::new(),
            tabs: Vec::new(),
            active: None,
        }));
        let sink = screen.clone();
        let mut shell = Shell::new(
            engine.clone(),
            store.clone(),
            Arc::new(FakeChrome),
            net.clone(),
            Box::new(move |p| apply_projection(&mut sink.lock().unwrap(), p)),
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });

        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://example.com/".into(),
        }));
        present_committed(&mut shell, id, "https://example.com/");
        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("discover {id}")));

        shell.handle(Command::Engine(EngineEvent::FaviconPixels {
            id,
            page_url: "https://attacker.example/".into(),
            rgba: vec![1; zephium_core::icon::RGBA32_BYTES],
        }));
        assert!(net.urls.lock().unwrap().is_empty());
        assert!(store.icons.lock().unwrap().is_empty());

        shell.handle(Command::Engine(EngineEvent::FaviconPixels {
            id,
            page_url: "https://example.com/".into(),
            rgba: vec![1; zephium_core::icon::RGBA32_BYTES - 1],
        }));
        assert!(store.icons.lock().unwrap().is_empty());

        let rgba = vec![7; zephium_core::icon::RGBA32_BYTES];
        shell.handle(Command::Engine(EngineEvent::FaviconPixels {
            id,
            page_url: "https://example.com/".into(),
            rgba: rgba.clone(),
        }));
        assert_eq!(
            store.icons.lock().unwrap().as_slice(),
            &[(String::from("https://example.com"), rgba)]
        );
        let tab = last(&screen).tabs.into_iter().next().unwrap();
        assert!(tab
            .favicon
            .as_deref()
            .is_some_and(|value| value.starts_with(zephium_core::icon::RGBA32_PREFIX)));
        let profile = shell.windows.focused().unwrap().profile;
        assert!(shell
            .favicon_key_for_url(profile, "https://example.com")
            .is_some());
    }

    #[test]
    fn stale_favicon_store_reply_cannot_cross_a_navigation_generation() {
        let (mut shell, _engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        shell.store_reads = Some(StoreReadQueue::new());
        let id = active_id(&screen);
        let profile = shell.windows.focused().unwrap().profile;

        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://first.example/".into(),
        }));
        let first_generation = shell.favicon_store_reads.get(&id).unwrap().generation;
        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://second.example/".into(),
        }));
        let second_generation = shell.favicon_store_reads.get(&id).unwrap().generation;
        let rgba = vec![91; zephium_core::icon::RGBA32_BYTES];

        shell.handle(Command::StoreRead(StoreReadResult::Favicon {
            generation: first_generation,
            id,
            profile,
            origin: "https://first.example".into(),
            rgba: Some(rgba.clone()),
        }));
        assert!(shell
            .favicon_key_for_url(profile, "https://first.example/")
            .is_none());
        assert_eq!(
            shell
                .favicon_store_reads
                .get(&id)
                .map(|pending| pending.generation),
            Some(second_generation)
        );

        shell.handle(Command::StoreRead(StoreReadResult::Favicon {
            generation: second_generation,
            id,
            profile,
            origin: "https://second.example".into(),
            rgba: Some(rgba),
        }));
        assert!(shell
            .favicon_key_for_url(profile, "https://second.example/")
            .is_some());
    }

    #[test]
    fn lost_favicon_store_completion_falls_back_instead_of_sticking_pending() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        shell.store_reads = Some(StoreReadQueue::new());
        shell.self_queue = Some(CommandQueue::new());
        let id = active_id(&screen);
        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://fallback.example/".into(),
        }));
        assert!(shell.favicon_store_reads.contains_key(&id));
        assert!(engine
            .calls()
            .iter()
            .all(|call| call != &format!("discover {id}")));

        shell.handle(Command::FaviconPoll { id, attempt: 0 });
        assert!(!shell.favicon_store_reads.contains_key(&id));
        assert!(engine
            .calls()
            .iter()
            .any(|call| call == &format!("discover {id}")));
    }

    #[test]
    fn bootstrap_hydrates_restored_tab_icons_without_native_callbacks() {
        let store = Arc::new(FakeStore::default());
        let (mut first, _engine, screen) = setup_with(store.clone());
        first.handle(Command::Bootstrap);
        let first_id = active_id(&screen);
        navigate_and_commit(&mut first, first_id, "restored-one.example");
        first.handle(Command::Open);
        let second_id = active_id(&screen);
        navigate_and_commit(&mut first, second_id, "restored-two.example");

        store.icons.lock().unwrap().extend([
            (
                "https://restored-one.example".to_owned(),
                vec![31; zephium_core::icon::RGBA32_BYTES],
            ),
            (
                "https://restored-two.example".to_owned(),
                vec![47; zephium_core::icon::RGBA32_BYTES],
            ),
        ]);
        drop(first);

        let (mut restored, engine, restored_screen) = setup_with(store);
        restored.handle(Command::Bootstrap);

        let state = last(&restored_screen);
        assert_eq!(state.tabs.len(), 2);
        assert!(state.tabs.iter().all(|tab| tab
            .favicon
            .as_deref()
            .is_some_and(|value| value.starts_with(zephium_core::icon::RGBA32_PREFIX))));
        assert!(engine
            .calls()
            .iter()
            .all(|call| !call.starts_with("discover ")));
    }

    #[test]
    fn positive_cache_eviction_allows_later_rehydration() {
        let (mut shell, _engine, _screen) = setup();
        let profile = ProfileId::from(91_000);
        let rgba = vec![83; zephium_core::icon::RGBA32_BYTES];
        let first = (profile, "https://icon-0.example".to_owned());

        for index in 0..=ICON_CACHE_CAPACITY {
            let key = (profile, format!("https://icon-{index}.example"));
            assert!(shell.cache_icon(key.clone(), &rgba));
            shell.icons_checked.insert(key);
        }

        assert!(!shell.icon_values.contains_key(&first));
        assert!(
            !shell.icons_checked.contains(&first),
            "an evicted positive must not become a permanent negative cache entry"
        );
    }

    #[test]
    fn slow_load_gets_one_fresh_bounded_favicon_pass_after_completion() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "slow-icon.example".into(),
        });
        shell.handle(Command::Engine(EngineEvent::LoadingChanged {
            id,
            loading: true,
        }));
        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://slow-icon.example/".into(),
        }));
        present_committed(&mut shell, id, "https://slow-icon.example/");

        for attempt in 1..=FAVICON_POLL_DELAYS.len() as u8 {
            shell.handle(Command::FaviconPoll { id, attempt });
        }
        let key = (
            shell.profile_of_item(id).unwrap(),
            "https://slow-icon.example".to_owned(),
        );
        assert!(!shell.icon_attempts.contains_key(&id));
        assert_eq!(shell.icon_load_completion_pending.get(&id), Some(&key));
        assert!(!shell.icons_checked.contains(&key));
        let discover = format!("discover {id}");
        let before_completion = engine
            .calls()
            .iter()
            .filter(|call| call.as_str() == discover)
            .count();

        shell.handle(Command::Engine(EngineEvent::LoadingChanged {
            id,
            loading: false,
        }));

        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.as_str() == discover)
                .count(),
            before_completion + 1
        );
        assert!(shell.icon_attempts.contains_key(&id));
        assert!(!shell.icon_load_completion_pending.contains_key(&id));

        shell.handle(Command::Engine(EngineEvent::FaviconPixels {
            id,
            page_url: "https://slow-icon.example/".into(),
            rgba: vec![17; zephium_core::icon::RGBA32_BYTES],
        }));

        assert!(last(&screen)
            .tabs
            .iter()
            .find(|tab| tab.id == id.to_string())
            .and_then(|tab| tab.favicon.as_deref())
            .is_some_and(|value| value.starts_with(zephium_core::icon::RGBA32_PREFIX)));
    }

    #[test]
    fn completed_no_icon_origin_is_terminal_for_duplicate_load_events() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "no-icon.example".into(),
        });
        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://no-icon.example/".into(),
        }));
        for attempt in 1..=FAVICON_POLL_DELAYS.len() as u8 {
            shell.handle(Command::FaviconPoll { id, attempt });
        }
        let discover = format!("discover {id}");
        let before_completion = engine
            .calls()
            .iter()
            .filter(|call| call.as_str() == discover)
            .count();

        for _ in 0..2 {
            shell.handle(Command::Engine(EngineEvent::LoadingChanged {
                id,
                loading: false,
            }));
        }

        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.as_str() == discover)
                .count(),
            before_completion,
            "a completed negative result must not restart on duplicate completion events"
        );
    }

    #[test]
    fn private_favicon_is_visible_but_never_written_to_persistent_storage() {
        let engine = Arc::new(FakeEngine::default());
        let store = Arc::new(FakeStore::default());
        let net = Arc::new(FakeNet::default());
        let screen: Screen = Arc::new(Mutex::new(ItemsState {
            projection_revision: String::new(),
            tabs: Vec::new(),
            active: None,
        }));
        let sink = screen.clone();
        let mut shell = Shell::new(
            engine,
            store.clone(),
            Arc::new(FakeChrome),
            net,
            Box::new(move |projection| {
                apply_projection(&mut sink.lock().unwrap(), projection);
            }),
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        shell.handle(Command::Bootstrap);

        let profile = ProfileId::from(9001);
        let space = SpaceId::from(9002);
        assert!(shell.profiles.insert(Profile {
            id: profile,
            name: "Private".into(),
            kind: ProfileKind::Incognito,
        }));
        assert!(shell.spaces.insert(Space {
            id: space,
            profile,
            name: "Private".into(),
        }));
        shell
            .windows
            .create(WindowKind::Main, profile, space, Size::new(1200.0, 800.0));
        shell.handle(Command::Open);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "private.example".into(),
        });
        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://private.example/".into(),
        }));
        present_committed(&mut shell, id, "https://private.example/");
        shell.handle(Command::Engine(EngineEvent::FaviconPixels {
            id,
            page_url: "https://private.example/".into(),
            rgba: vec![9; zephium_core::icon::RGBA32_BYTES],
        }));

        assert!(store.icons.lock().unwrap().is_empty());
        assert!(last(&screen)
            .tabs
            .iter()
            .find(|tab| tab.id == id.to_string())
            .and_then(|tab| tab.favicon.as_deref())
            .is_some_and(|value| value.starts_with(zephium_core::icon::RGBA32_PREFIX)));
    }

    #[test]
    fn shutdown_snapshots_flushes_and_seals_the_actor_once() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, _engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "example.com");
        // Title updates are normally not session writes. The shutdown-owned
        // snapshot must nevertheless capture the actor's latest truth.
        shell.handle(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "Final title".into(),
        }));

        let (ack, done) = sync_channel(1);
        shell.handle(Command::Shutdown {
            deadline: test_shutdown_deadline(),
            ack,
        });
        assert_eq!(done.recv().unwrap(), ShutdownOutcome::Clean);

        let events = store.events.lock().unwrap().clone();
        assert!(events.ends_with(&["save", "flush"]), "{events:?}");
        assert_eq!(events.iter().filter(|event| **event == "flush").count(), 1);
        let saved = store.saved.lock().unwrap().clone().unwrap();
        let tab = saved.items.iter().find(|item| item.id == id).unwrap();
        let PersistedKind::Tab { title, .. } = &tab.kind else {
            panic!("active item must remain a tab");
        };
        assert_eq!(title, "Final title");

        // Late UI/engine/timer work cannot mutate state after the barrier.
        let tabs = last(&screen).tabs.len();
        shell.handle(Command::Open);
        assert_eq!(last(&screen).tabs.len(), tabs);

        // A repeated close request receives the same completion without a
        // second snapshot or flush.
        let before = store.events.lock().unwrap().clone();
        let (ack, done) = sync_channel(1);
        shell.handle(Command::Shutdown {
            deadline: test_shutdown_deadline(),
            ack,
        });
        assert_eq!(done.recv().unwrap(), ShutdownOutcome::Clean);
        assert_eq!(*store.events.lock().unwrap(), before);
    }

    #[test]
    fn shutdown_deadline_includes_time_spent_waiting_before_actor_processing() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, _engine, _screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let (ack, done) = sync_channel(1);

        shell.handle(Command::Shutdown {
            deadline: std::time::Instant::now() - std::time::Duration::from_millis(1),
            ack,
        });

        assert_eq!(done.recv().unwrap(), ShutdownOutcome::RetryableFailure);
        assert!(shell.shutdown_result.is_none());
        assert!(store.events.lock().unwrap().is_empty());
    }

    #[test]
    fn failed_shutdown_barrier_keeps_state_live_for_retry() {
        let store = Arc::new(FakeStore::default());
        *store.flush_result.lock().unwrap() = Some(false);
        let (mut shell, _engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);

        let (ack, done) = sync_channel(1);
        shell.handle(Command::Shutdown {
            deadline: test_shutdown_deadline(),
            ack,
        });
        assert_eq!(done.recv().unwrap(), ShutdownOutcome::RetryableFailure);

        let before = last(&screen).tabs.len();
        shell.handle(Command::Open);
        assert_eq!(last(&screen).tabs.len(), before + 1);

        *store.flush_result.lock().unwrap() = Some(true);
        let (ack, done) = sync_channel(1);
        shell.handle(Command::Shutdown {
            deadline: test_shutdown_deadline(),
            ack,
        });
        assert_eq!(done.recv().unwrap(), ShutdownOutcome::Clean);
    }

    #[test]
    fn failed_shutdown_folds_racing_native_failure_before_resuming() {
        let store = Arc::new(FakeStore::default());
        *store.flush_result.lock().unwrap() = Some(false);
        let (mut shell, engine, screen) = setup_with(store);
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "example.com");

        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        let handle = Handle::new(queue.clone());
        let (ack, done) = sync_channel(1);
        queue
            .try_push(Command::Shutdown {
                deadline: test_shutdown_deadline(),
                ack,
            })
            .ok()
            .unwrap();
        let shutdown = queue.try_recv().unwrap();
        assert!(!handle.dispatch(Command::Engine(EngineEvent::Crashed { id })));

        shell.handle(shutdown);
        assert_eq!(done.recv().unwrap(), ShutdownOutcome::RetryableFailure);
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("close {id}")));
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with(&format!("create {id} ")))
                .count(),
            2
        );
        assert!(handle.dispatch(Command::Open));
    }

    #[test]
    fn native_shutdown_timeout_is_bounded_and_terminal() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        engine
            .skip_shutdown_callback
            .store(true, std::sync::atomic::Ordering::Release);

        let before = last(&screen).tabs.len();
        let started = std::time::Instant::now();
        let (ack, done) = sync_channel(1);
        shell.handle(Command::Shutdown {
            deadline: test_shutdown_deadline(),
            ack,
        });
        assert_eq!(done.recv().unwrap(), ShutdownOutcome::Unclean);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));

        // Native teardown may have happened even without its callback. The
        // shell must never resume and issue operations into a partial engine.
        shell.handle(Command::Open);
        assert_eq!(last(&screen).tabs.len(), before);
    }

    #[test]
    fn native_cleanup_rejection_is_terminal_and_sticky() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        *engine.shutdown_result.lock().unwrap() = Some(false);

        let before = last(&screen).tabs.len();
        let (ack, done) = sync_channel(1);
        shell.handle(Command::Shutdown {
            deadline: test_shutdown_deadline(),
            ack,
        });
        assert_eq!(done.recv().unwrap(), ShutdownOutcome::Unclean);

        shell.handle(Command::Open);
        assert_eq!(last(&screen).tabs.len(), before);

        let (ack, done) = sync_channel(1);
        shell.handle(Command::Shutdown {
            deadline: test_shutdown_deadline(),
            ack,
        });
        assert_eq!(done.recv().unwrap(), ShutdownOutcome::Unclean);
    }

    #[test]
    fn ordered_queue_coalesces_only_inside_an_engine_burst() {
        let id = ItemId::from(7);
        let queue = CommandQueue::new();
        queue
            .try_push(Command::Engine(EngineEvent::TitleChanged {
                id,
                title: "old".into(),
            }))
            .ok()
            .unwrap();
        queue
            .try_push(Command::Engine(EngineEvent::LoadingChanged {
                id,
                loading: true,
            }))
            .ok()
            .unwrap();
        queue
            .try_push(Command::Engine(EngineEvent::TitleChanged {
                id,
                title: "latest".into(),
            }))
            .ok()
            .unwrap();

        // Replacing a value moves it to its true latest position relative to
        // the other coalesced fields.
        assert!(matches!(
            queue.recv(),
            Some(Command::Engine(EngineEvent::LoadingChanged {
                loading: true,
                ..
            }))
        ));
        assert!(matches!(
            queue.recv(),
            Some(Command::Engine(EngineEvent::TitleChanged { title, .. })) if title == "latest"
        ));

        queue
            .try_push(Command::Engine(EngineEvent::TitleChanged {
                id,
                title: "before".into(),
            }))
            .ok()
            .unwrap();
        queue.try_push(Command::Reload(id)).ok().unwrap();
        queue
            .try_push(Command::Engine(EngineEvent::TitleChanged {
                id,
                title: "after".into(),
            }))
            .ok()
            .unwrap();

        assert!(matches!(
            queue.recv(),
            Some(Command::Engine(EngineEvent::TitleChanged { title, .. })) if title == "before"
        ));
        assert!(matches!(queue.recv(), Some(Command::Reload(value)) if value == id));
        assert!(matches!(
            queue.recv(),
            Some(Command::Engine(EngineEvent::TitleChanged { title, .. })) if title == "after"
        ));
    }

    #[test]
    fn native_operation_facts_are_bounded_latest_per_view() {
        let id = ItemId::from(7);
        let queue = CommandQueue::new();
        for (request, applied_scale, succeeded) in
            [(1, 1.0, false), (2, 1.1, true), (3, 1.1, false)]
        {
            queue
                .try_push(Command::Engine(EngineEvent::ZoomSettled {
                    id,
                    request: ZoomRequestId(request),
                    applied_scale,
                    succeeded,
                }))
                .ok()
                .unwrap();
        }
        queue
            .try_push(Command::Engine(EngineEvent::NativeActionFailed {
                id,
                action: NativeAction::GoBack,
            }))
            .ok()
            .unwrap();
        queue
            .try_push(Command::Engine(EngineEvent::NativeActionFailed {
                id,
                action: NativeAction::GoForward,
            }))
            .ok()
            .unwrap();

        assert!(matches!(
            queue.recv(),
            Some(Command::Engine(EngineEvent::ZoomSettled {
                request: ZoomRequestId(3),
                applied_scale,
                succeeded: false,
                ..
            })) if applied_scale == 1.1
        ));
        assert!(matches!(
            queue.recv(),
            Some(Command::Engine(EngineEvent::NativeActionFailed {
                action: NativeAction::GoForward,
                ..
            }))
        ));
        assert!(queue.try_recv().is_none());
    }

    #[test]
    fn overloaded_queue_never_blocks_and_reserves_lifecycle_capacity() {
        let id = ItemId::from(7);
        let queue = CommandQueue::new();
        for _ in 0..NORMAL_COMMAND_CAPACITY {
            queue.try_push(Command::Reload(id)).ok().unwrap();
        }
        assert!(matches!(
            queue.try_push(Command::Engine(EngineEvent::TitleChanged {
                id,
                title: "best effort".into(),
            })),
            Err(TryPushError::Full(_))
        ));
        let handle = Handle::new(queue.clone());
        assert!(!handle.dispatch(Command::Close(id)));

        // Native failure/crash transitions use a band sized for every
        // bounded recovery key. Synthetic work beyond that proven state
        // space is rejected without deleting an accepted user mutation.
        for n in 0..(LIFECYCLE_COMMAND_CAPACITY - NORMAL_COMMAND_CAPACITY) {
            queue
                .try_push(Command::Engine(EngineEvent::ViewCreationFailed {
                    id: ItemId::from(100 + n as u128),
                }))
                .ok()
                .unwrap();
        }
        assert!(matches!(
            queue.try_push(Command::Engine(EngineEvent::Crashed {
                id: ItemId::from(9999),
            })),
            Err(TryPushError::Full(_))
        ));

        // One final slot belongs only to the ordered shutdown barrier. Its
        // admission atomically seals the queue, so later work is not falsely
        // reported as accepted behind a barrier that will drain it.
        let _completion = handle.shutdown();
        assert!(!handle.dispatch(Command::Open));
        let drained: Vec<_> = std::iter::from_fn(|| queue.try_recv()).collect();
        assert_eq!(drained.len(), COMMAND_QUEUE_CAPACITY);
        assert_eq!(
            drained
                .iter()
                .filter(|command| matches!(command, Command::Reload(_)))
                .count(),
            NORMAL_COMMAND_CAPACITY
        );
        assert!(matches!(drained.last(), Some(Command::Shutdown { .. })));
    }

    #[test]
    fn lifecycle_band_can_retain_a_whole_process_crash_and_shutdown() {
        let queue = CommandQueue::new();
        for value in 0..zephium_core::session::MAX_SESSION_ITEMS {
            queue
                .try_push(Command::Engine(EngineEvent::Crashed {
                    id: ItemId::from(value as u128 + 1),
                }))
                .ok()
                .expect("every maximum-session view death must be admitted");
        }
        let (ack, _completion) = sync_channel(1);
        queue
            .try_push(Command::Shutdown {
                deadline: test_shutdown_deadline(),
                ack,
            })
            .ok()
            .expect("the shutdown barrier remains reserved after the crash burst");

        let drained: Vec<_> = std::iter::from_fn(|| queue.try_recv()).collect();
        assert_eq!(drained.len(), zephium_core::session::MAX_SESSION_ITEMS + 1);
        assert!(matches!(drained.last(), Some(Command::Shutdown { .. })));
    }

    #[test]
    fn lifecycle_overload_replaces_an_old_fact_with_the_latest_same_key() {
        let id = ItemId::from(7);
        let mut commands = VecDeque::new();
        commands.push_back(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://old.example/".into(),
        }));
        for value in 1..LIFECYCLE_COMMAND_CAPACITY {
            commands.push_back(Command::Engine(EngineEvent::Crashed {
                id: ItemId::from(value as u128 + 10_000),
            }));
        }

        enqueue(
            &mut commands,
            Command::Engine(EngineEvent::UrlChanged {
                id,
                url: "https://latest.example/".into(),
            }),
            LIFECYCLE_COMMAND_CAPACITY,
            true,
        )
        .expect("latest bounded native fact must replace its stale predecessor");

        assert_eq!(commands.len(), LIFECYCLE_COMMAND_CAPACITY);
        assert!(matches!(
            commands.back(),
            Some(Command::Engine(EngineEvent::UrlChanged { id: observed, url }))
                if *observed == id && url == "https://latest.example/"
        ));
        assert!(!commands.iter().any(|command| matches!(
            command,
            Command::Engine(EngineEvent::UrlChanged { url, .. })
                if url == "https://old.example/"
        )));
    }

    #[test]
    fn critical_overload_never_evicts_an_accepted_user_mutation() {
        let id = ItemId::from(7);
        let window: WindowId = 1;
        let mut commands = VecDeque::new();
        commands.push_back(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://committed.example/".into(),
        }));
        for _ in 1..LIFECYCLE_COMMAND_CAPACITY {
            commands.push_back(Command::Reload(id));
        }

        assert!(enqueue(
            &mut commands,
            Command::Engine(EngineEvent::SplitChanged {
                window,
                tree: Pane::Leaf(id),
            }),
            LIFECYCLE_COMMAND_CAPACITY,
            true,
        )
        .is_err());

        assert!(commands.iter().any(|command| matches!(
            command,
            Command::Engine(EngineEvent::UrlChanged { url, .. })
                if url == "https://committed.example/"
        )));
        assert_eq!(
            commands
                .iter()
                .filter(|command| matches!(command, Command::Reload(_)))
                .count(),
            LIFECYCLE_COMMAND_CAPACITY - 1
        );
        assert!(!commands
            .iter()
            .any(|command| matches!(command, Command::Engine(EngineEvent::SplitChanged { .. }))));
        assert_eq!(commands.len(), LIFECYCLE_COMMAND_CAPACITY);
    }

    #[test]
    fn ordinary_overload_evicts_only_ordinary_presentation_state() {
        let id = ItemId::from(7);
        let mut commands = VecDeque::new();
        commands.push_back(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://committed.example/".into(),
        }));
        commands.push_back(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "stale".into(),
        }));
        for _ in commands.len()..NORMAL_COMMAND_CAPACITY {
            commands.push_back(Command::Reload(id));
        }

        enqueue(
            &mut commands,
            Command::Close(id),
            NORMAL_COMMAND_CAPACITY,
            false,
        )
        .expect("ordinary intent may displace ordinary presentation state");
        assert!(commands
            .iter()
            .any(|command| matches!(command, Command::Engine(EngineEvent::UrlChanged { .. }))));
        assert!(!commands
            .iter()
            .any(|command| matches!(command, Command::Engine(EngineEvent::TitleChanged { .. }))));
        assert!(commands
            .iter()
            .any(|command| matches!(command, Command::Close(value) if *value == id)));
    }

    #[test]
    fn failed_shutdown_replays_only_late_critical_callbacks() {
        let id = ItemId::from(7);
        let queue = CommandQueue::new();
        let handle = Handle::new(queue.clone());
        let _done = handle.shutdown();

        assert!(!handle.dispatch(Command::Engine(EngineEvent::Crashed { id })));
        assert!(!handle.dispatch(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://old.example/".into(),
        })));
        assert!(!handle.dispatch(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://latest.example/".into(),
        })));
        assert!(!handle.dispatch(Command::Open));
        assert!(matches!(queue.try_recv(), Some(Command::Shutdown { .. })));
        assert!(queue.try_recv().is_none());

        let recovered = queue.reopen_after_failed_shutdown();
        assert_eq!(recovered.len(), 2);
        assert!(matches!(
            &recovered[0],
            Command::Engine(EngineEvent::Crashed { id: recovered }) if *recovered == id
        ));
        assert!(matches!(
            &recovered[1],
            Command::Engine(EngineEvent::UrlChanged { id: recovered, url })
                if *recovered == id && url == "https://latest.example/"
        ));
        assert!(
            queue.try_recv().is_none(),
            "ordinary late work stays rejected"
        );
        assert!(handle.dispatch(Command::Open));
    }

    #[test]
    fn last_public_handle_closes_actor_queue_and_wakes_ticker() {
        let queue = CommandQueue::new();
        let first = Handle::new(queue.clone());
        let last = first.clone();
        drop(first);

        let completion = last.shutdown();
        drop(last);

        assert!(matches!(
            queue.try_push(Command::Tick),
            Err(TryPushError::Closed(_))
        ));
        assert!(!queue.wait_for_tick(std::time::Duration::ZERO));
        let pending = queue.recv().expect("accepted shutdown remains ordered");
        finish_shutdown(pending, ShutdownOutcome::Clean);
        assert_eq!(completion.recv().unwrap(), ShutdownOutcome::Clean);
        assert!(queue.recv().is_none());
    }

    #[test]
    fn handle_count_overflow_seals_instead_of_aborting_or_underflowing() {
        let queue = CommandQueue::new();
        let handle = Handle::new(queue.clone());
        queue.inner.state.lock().unwrap().handles = usize::MAX;

        let uncounted = handle.clone();

        assert!(!uncounted.counted);
        assert!(matches!(
            queue.try_push(Command::Tick),
            Err(TryPushError::Closed(_))
        ));
        drop(uncounted);
        drop(handle);
    }

    #[test]
    fn unexpected_handle_release_seals_without_panicking() {
        let queue = CommandQueue::new();

        queue.release_handle();

        assert!(matches!(
            queue.try_push(Command::Tick),
            Err(TryPushError::Closed(_))
        ));
        assert!(!queue.wait_for_tick(std::time::Duration::ZERO));
    }

    #[test]
    fn dependency_callback_handle_is_weak_and_never_keeps_actor_open() {
        let queue = CommandQueue::new();
        let handle = Handle::new(queue.clone());
        let callback = handle.callback_handle();
        assert!(callback.dispatch(Command::Tick));
        assert!(queue.try_recv().is_some());

        drop(handle);
        assert!(!callback.dispatch(Command::Tick));
        drop(queue);
        assert!(!callback.dispatch(Command::Tick));
    }

    #[test]
    fn actor_exit_guard_makes_a_pending_shutdown_terminal() {
        let queue = CommandQueue::new();
        let (ack, completion) = sync_channel(1);
        queue
            .try_push(Command::Shutdown {
                deadline: test_shutdown_deadline(),
                ack,
            })
            .ok()
            .unwrap();

        {
            let _guard = ActorExitGuard(queue.clone());
        }

        assert_eq!(completion.recv().unwrap(), ShutdownOutcome::Unclean);
        assert!(matches!(
            queue.try_push(Command::Tick),
            Err(TryPushError::Closed(_))
        ));
    }

    #[test]
    fn shutdown_on_a_permanently_closed_actor_is_terminal() {
        let queue = CommandQueue::new();
        let handle = Handle::new(queue.clone());
        let drained = queue.close_and_drain();
        assert!(drained.is_empty());

        assert_eq!(handle.shutdown().recv().unwrap(), ShutdownOutcome::Unclean);
    }

    #[test]
    fn actor_panic_terminalizes_pending_and_later_shutdown_requests() {
        let store = Arc::new(FakeStore::default());
        store
            .panic_on_load
            .store(true, std::sync::atomic::Ordering::Release);
        let handle = spawn(
            Arc::new(FakeEngine::default()),
            store,
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(|_| {}),
        )
        .expect("spawn test shell");

        assert!(handle.dispatch(Command::Bootstrap));
        let pending = handle.shutdown();
        assert_eq!(
            pending
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            ShutdownOutcome::Unclean
        );
        assert_eq!(handle.shutdown().recv().unwrap(), ShutdownOutcome::Unclean);
    }

    #[test]
    fn spawned_actor_processes_dispatched_commands() {
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = spawn(
            Arc::new(FakeEngine::default()),
            Arc::new(FakeStore::default()),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |s| {
                let _ = tx.send(s);
            }),
        )
        .expect("spawn test shell");
        handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        handle.dispatch(Command::Bootstrap);
        let projection =
            std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
                .find(|p| matches!(p, Projection::Items(_)))
                .expect("bootstrap must project an items snapshot");
        let Projection::Items(s) = projection else {
            unreachable!()
        };
        assert!(s.active.is_some());
    }

    #[test]
    fn slow_history_sqlite_read_never_blocks_shell_coordination() {
        let store = Arc::new(FakeStore::default());
        store
            .history_delay_ms
            .store(500, std::sync::atomic::Ordering::Release);
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = spawn(
            Arc::new(FakeEngine::default()),
            store,
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |projection| {
                let _ = tx.send(projection);
            }),
        )
        .expect("spawn test shell");
        assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
        assert!(handle.dispatch(Command::Bootstrap));
        assert!(
            std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
                .any(|projection| matches!(projection, Projection::Items(_)))
        );

        let started = std::time::Instant::now();
        assert!(handle.dispatch(Command::Search("slow".into())));
        assert!(handle.dispatch(Command::Open));
        let opened =
            std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_millis(250)).ok())
                .find_map(|projection| match projection {
                    Projection::Items(items) if items.tabs.len() == 2 => Some(items),
                    _ => None,
                });
        assert!(
            opened.is_some(),
            "shell actor stalled behind SQLite history"
        );
        assert!(started.elapsed() < std::time::Duration::from_millis(400));
    }

    #[test]
    fn tracked_operation_has_exact_admission_and_actor_disposition_id() {
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = spawn(
            Arc::new(FakeEngine::default()),
            Arc::new(FakeStore::default()),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |projection| {
                let _ = tx.send(projection);
            }),
        )
        .expect("spawn test shell");
        assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
        assert!(handle.dispatch(Command::Bootstrap));
        assert!(handle.dispatch_operation("operation-7".into(), Command::Open));

        let completion =
            std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
                .find_map(|projection| match projection {
                    Projection::OperationProcessed(completion) => Some(completion),
                    _ => None,
                })
                .expect("admitted operation must complete in actor order");
        assert_eq!(completion.operation_id, "operation-7");
        assert_eq!(completion.outcome, OperationOutcome::Deferred);
        assert_eq!(completion.reason, OperationReason::NativeWorkPending);
        assert!(!handle.dispatch_operation(String::new(), Command::Open));
        assert!(!handle.dispatch_operation(
            "nested".into(),
            Command::Operation {
                operation_id: "inner".into(),
                command: Box::new(Command::Open),
            },
        ));
        assert!(tracked_operation_command(&Command::DividerRelease {
            x: None,
            y: None,
        }));
    }

    #[test]
    fn runtime_restart_requirement_is_sticky_deduplicated_and_replayed_on_bootstrap() {
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let sink = statuses.clone();
        let mut shell = Shell::new(
            Arc::new(FakeEngine::default()),
            Arc::new(FakeStore::default()),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |projection| {
                if let Projection::RuntimeStatus(status) = projection {
                    sink.lock().unwrap().push(status.restart_required);
                }
            }),
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        shell.handle(Command::Bootstrap);
        shell.handle(Command::Engine(EngineEvent::RuntimeRestartRequired));
        shell.handle(Command::Engine(EngineEvent::RuntimeRestartRequired));
        shell.handle(Command::Bootstrap);

        assert_eq!(*statuses.lock().unwrap(), vec![false, true, true]);
        assert!(shell.runtime_restart_required);
    }

    #[test]
    fn maintenance_reconciles_a_runtime_event_lost_before_shell_admission() {
        let engine = Arc::new(FakeEngine::default());
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let sink = statuses.clone();
        let mut shell = Shell::new(
            engine.clone(),
            Arc::new(FakeStore::default()),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |projection| {
                if let Projection::RuntimeStatus(status) = projection {
                    sink.lock().unwrap().push(status.restart_required);
                }
            }),
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        shell.handle(Command::Bootstrap);
        engine
            .runtime_restart_required
            .store(true, std::sync::atomic::Ordering::Release);

        shell.handle(Command::Tick);
        shell.handle(Command::Tick);

        assert_eq!(*statuses.lock().unwrap(), vec![false, true]);
    }

    #[test]
    fn setting_operation_reports_store_admission_without_claiming_durability() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, _engine, _screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);

        store
            .reject_settings
            .store(true, std::sync::atomic::Ordering::Release);
        let rejected = shell.handle_operation(Command::SetAppSetting {
            key: "appearance".into(),
            value: "dark".into(),
        });
        assert_eq!(rejected.outcome, OperationOutcome::Rejected);
        assert_eq!(rejected.reason, OperationReason::StoreAdmissionRejected);

        store
            .reject_settings
            .store(false, std::sync::atomic::Ordering::Release);
        let accepted = shell.handle_operation(Command::SetAppSetting {
            key: "appearance".into(),
            value: "light".into(),
        });
        assert_eq!(accepted.outcome, OperationOutcome::Deferred);
        assert_eq!(accepted.reason, OperationReason::StoreWorkPending);
    }

    #[test]
    fn operation_outcomes_reject_invalid_scope_and_report_native_admission() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "admission.example");

        let invalid = shell.handle_operation(Command::Reload(ItemId::from(u128::MAX)));
        assert_eq!(invalid.outcome, OperationOutcome::Rejected);
        assert_eq!(invalid.reason, OperationReason::InvalidScope);

        engine
            .reject_native_dispatch
            .store(true, std::sync::atomic::Ordering::Release);
        let rejected = shell.handle_operation(Command::Reload(id));
        assert_eq!(rejected.outcome, OperationOutcome::NativeAdmissionFailed);
        assert_eq!(rejected.reason, OperationReason::NativeDispatchRejected);

        engine
            .reject_native_dispatch
            .store(false, std::sync::atomic::Ordering::Release);
        let scheduled = shell.handle_operation(Command::Reload(id));
        assert_eq!(scheduled.outcome, OperationOutcome::Deferred);
        assert_eq!(scheduled.reason, OperationReason::NativeWorkPending);
    }

    #[test]
    fn history_operations_distinguish_noop_from_scheduled_dispatch() {
        let (mut shell, _engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "history.example");

        let unavailable = shell.handle_operation(Command::GoBack(id));
        assert_eq!(unavailable.outcome, OperationOutcome::NoOp);
        assert_eq!(unavailable.reason, OperationReason::HistoryUnavailable);

        shell.handle(Command::Engine(EngineEvent::NavState {
            id,
            can_go_back: true,
            can_go_forward: false,
        }));
        let back = shell.handle_operation(Command::GoBack(id));
        assert_eq!(back.outcome, OperationOutcome::Deferred);
        assert_eq!(back.reason, OperationReason::NativeWorkPending);
        let forward = shell.handle_operation(Command::GoForward(id));
        assert_eq!(forward.outcome, OperationOutcome::NoOp);
        assert_eq!(forward.reason, OperationReason::HistoryUnavailable);
    }

    #[test]
    fn navigation_waits_for_exact_inflight_discard_instead_of_claiming_success() {
        let (mut shell, _engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "before-discard.example");
        shell.discard_probes.insert(
            id,
            PendingDiscardProbe::Closing {
                probe: DiscardProbeId(77),
                recreate: false,
                deferred_navigation: None,
            },
        );

        let completion = shell.handle_operation(Command::Navigate {
            id,
            input: "after-discard.example".into(),
        });
        assert_eq!(completion.outcome, OperationOutcome::Deferred);
        assert_eq!(completion.reason, OperationReason::DiscardCompletionPending);
        assert!(matches!(
            shell.discard_probes.get(&id),
            Some(PendingDiscardProbe::Closing {
                recreate: true,
                deferred_navigation: Some(input),
                ..
            }) if input == "after-discard.example"
        ));
    }

    #[test]
    fn layout_and_zoom_report_rejection_and_roll_back_unapplied_zoom() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "layout-left.example");
        let opened = shell.operation_open();
        assert_eq!(opened.outcome, OperationOutcome::Deferred);
        let second = active_id(&screen);
        navigate_and_commit(&mut shell, second, "layout-right.example");

        let split = shell.handle_operation(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });
        assert_eq!(split.outcome, OperationOutcome::Deferred);

        let original_zoom = shell.items.tab(second).unwrap().zoom;
        engine
            .reject_native_dispatch
            .store(true, std::sync::atomic::Ordering::Release);
        let zoom = shell.handle_operation(Command::Run("zoom.in".into()));
        assert_eq!(zoom.outcome, OperationOutcome::NativeAdmissionFailed);
        assert_eq!(shell.items.tab(second).unwrap().zoom, original_zoom);

        let unsplit = shell.handle_operation(Command::Unsplit);
        assert_eq!(unsplit.outcome, OperationOutcome::NativeAdmissionFailed);
        assert!(shell.windows.focused().unwrap().splits.is_none());
    }

    #[test]
    fn hostile_url_churn_cannot_force_repeated_full_session_snapshots() {
        let store = Arc::new(FakeStore::default());
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = spawn(
            Arc::new(FakeEngine::default()),
            store.clone(),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |projection| {
                let _ = tx.send(projection);
            }),
        )
        .expect("spawn test shell");
        assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
        assert!(handle.dispatch(Command::Bootstrap));
        let state = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
            .find_map(|projection| match projection {
                Projection::Items(state) => Some(state),
                _ => None,
            })
            .expect("bootstrap snapshot");
        let id = ItemId::parse(state.active.as_deref().unwrap()).unwrap();

        for value in 0..100 {
            assert!(handle.dispatch(Command::Engine(EngineEvent::UrlChanged {
                id,
                url: format!("https://example.test/{value}"),
            })));
        }
        std::thread::sleep(PERSIST_DEBOUNCE + std::time::Duration::from_millis(150));
        assert_eq!(
            store
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == "save")
                .count(),
            0,
            "URL-only churn must not use the structural persistence cadence"
        );

        // A real structural mutation still checkpoints the latest coalesced
        // URL promptly; we do not trade SSD protection for stale clean exits.
        assert!(handle.dispatch(Command::Open));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !store.events.lock().unwrap().contains(&"save") {
            assert!(
                std::time::Instant::now() < deadline,
                "structural checkpoint did not fire"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            store
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == "save")
                .count(),
            1,
            "URL churn plus one structure change must construct one snapshot"
        );
        let saved = store.saved.lock().unwrap().clone().unwrap();
        assert!(saved.items.iter().any(|item| {
            item.id == id
                && matches!(
                    &item.kind,
                    PersistedKind::Tab { url, .. }
                        if url == "https://example.test/99"
                )
        }));
    }

    #[test]
    fn url_checkpoint_deadline_is_global_and_structure_preempts_it() {
        let (mut shell, _engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        let id = active_id(&screen);
        let checkpoint_floor = std::time::Instant::now();
        shell.last_url_checkpoint = checkpoint_floor;

        for value in 0..100 {
            shell.handle(Command::Engine(EngineEvent::UrlChanged {
                id,
                url: format!("https://churn.example/{value}"),
            }));
        }
        let url_deadline = queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .persist_deadline
            .unwrap();
        assert!(url_deadline >= checkpoint_floor + URL_CHECKPOINT_INTERVAL);
        assert_eq!(shell.url_checkpoint_dirty.len(), 1);

        let structural_started = std::time::Instant::now();
        shell.schedule_persist();
        let structural_deadline = queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .persist_deadline
            .unwrap();
        assert!(structural_deadline < url_deadline);
        assert!(structural_deadline <= structural_started + PERSIST_MAX_AGE);
    }

    #[test]
    fn spawned_shutdown_is_ordered_behind_prior_commands() {
        let store = Arc::new(FakeStore::default());
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = spawn(
            Arc::new(FakeEngine::default()),
            store.clone(),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |projection| {
                let _ = tx.send(projection);
            }),
        )
        .expect("spawn test shell");
        handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        handle.dispatch(Command::Bootstrap);
        let state = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
            .find_map(|projection| match projection {
                Projection::Items(state) => Some(state),
                _ => None,
            })
            .expect("bootstrap snapshot");
        let id = ItemId::parse(state.active.as_deref().unwrap()).unwrap();

        handle.dispatch(Command::Navigate {
            id,
            input: "example.com".into(),
        });
        handle.dispatch(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://example.com/".into(),
        }));
        handle.dispatch(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "Queued before close".into(),
        }));
        let completion = handle.shutdown();
        let workers = completion.workers.clone();
        assert_eq!(
            completion
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            ShutdownOutcome::Clean
        );
        let worker_state = workers.state.lock().unwrap();
        assert!(worker_state.actor.is_none());
        assert!(worker_state.timer.is_none());

        let saved = store.saved.lock().unwrap().clone().unwrap();
        let tab = saved.items.iter().find(|item| item.id == id).unwrap();
        let PersistedKind::Tab { title, .. } = &tab.kind else {
            panic!("active item must remain a tab");
        };
        assert_eq!(title, "Queued before close");
        assert_eq!(
            store
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == "flush")
                .count(),
            1
        );
    }

    #[test]
    fn dropping_last_handle_does_not_cancel_an_accepted_shutdown_barrier() {
        let handle = spawn(
            Arc::new(FakeEngine::default()),
            Arc::new(FakeStore::default()),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(|_| {}),
        )
        .expect("spawn test shell");
        assert!(handle.dispatch(Command::Bootstrap));
        let completion = handle.shutdown();
        drop(handle);

        assert_eq!(
            completion
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            ShutdownOutcome::Clean
        );
    }
}
