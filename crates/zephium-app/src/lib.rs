//! The actor shell around the pure core. One thread owns all state; commands
//! enter through a queue (UI intents and engine events alike), effects leave
//! through ports, projections go to the UI.

mod actor;
mod api;
mod shell;
mod store_reads;

use actor::CommandQueue;
#[cfg(test)]
use actor::{
    enqueue, finish_shutdown, tracked_operation_command, ActorExitGuard, PresentationDeadline,
    TimerWake, TryPushError, COMMAND_QUEUE_CAPACITY, LIFECYCLE_COMMAND_CAPACITY,
    NORMAL_COMMAND_CAPACITY,
};
pub use actor::{spawn, CallbackHandle, Handle, ShutdownRequest, SpawnError};

pub use api::{
    ChromePresentation, ChromePresentationCallback, ChromePresentationDispatch, Command, EmitFn,
    PresentationChrome, SharedChrome, SharedEngine, SharedStore, ShutdownOutcome,
};

use store_reads::StoreReadQueue;
#[doc(hidden)]
pub use store_reads::StoreReadResult;
#[cfg(test)]
use store_reads::FAVICON_CACHE_MAX_AGE_SECONDS;

use std::collections::VecDeque;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};

use zephium_core::geometry::{Rect, Size};
use zephium_core::ids::{ItemId, ProfileId, SpaceId, WindowId};
use zephium_core::item::{Lifecycle, Placement, SpaceSection, TabState};
use zephium_core::items::{Effect, Items};
use zephium_core::layout;
#[cfg(test)]
use zephium_core::ports::chrome::Chrome as GeometryChrome;
use zephium_core::ports::chrome::ChromeFrame;
#[cfg(test)]
use zephium_core::ports::engine::Engine;
use zephium_core::ports::engine::{
    DiscardProbeId, EngineEvent, NativeAction, NativeDispatch, NavigationPresentationId, Partition,
    ProfileDataErasureOutcome, ZoomRequestId,
};
#[cfg(test)]
use zephium_core::ports::store::Store;
use zephium_core::ports::store::{
    PendingProfileDeletion, ProfileDeletionAuthorizeOutcome, ProfileDeletionFinalizeOutcome,
    ProfileDeletionLoad, SessionLoad, StoreShutdownOutcome, MAX_FAVICON_BATCH_ORIGINS,
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
    emit: EmitFn,
}

impl Shell {
    #[cfg(test)]
    pub fn new(
        engine: SharedEngine,
        store: SharedStore,
        chrome: SharedChrome,
        emit: EmitFn,
    ) -> Self {
        Self::with_store_reads(engine, store, chrome, emit, None)
    }

    fn with_store_reads(
        engine: SharedEngine,
        store: SharedStore,
        chrome: SharedChrome,
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
