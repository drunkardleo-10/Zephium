//! Authoritative browser-shell state machine and effect coordination.

mod bootstrap;
mod favicons;
mod operations;
mod persistence;
mod presentation;
mod profile_deletion;
mod projections;
mod scope;
mod search;
mod tabs;
mod view_lifecycle;
mod window_layout;
mod zoom;

use favicons::{origin_of, IconAttempt, PendingFaviconBatch, PendingFaviconStoreRead};
#[cfg(test)]
use favicons::{FAVICON_POLL_DELAYS, ICON_CACHE_CAPACITY};
use presentation::PendingPresentation;
#[cfg(test)]
use presentation::MAX_PRESENTATION_ADMISSION_REJECTIONS;
use profile_deletion::{
    ProfileDeletionInbox, ProfileDeletionPhase, ProfileDeletionState,
    PROFILE_DELETION_STORE_TIMEOUT,
};
use search::PendingSearch;
#[cfg(test)]
use view_lifecycle::MAX_CONCURRENT_DISCARD_PROBES;
use view_lifecycle::{
    PendingDiscardProbe, DISCARD_IDLE_GRACE, DISCARD_PROBE_TIMEOUT, DISCARD_PROTECTED_RETRY,
    LIVE_VIEW_ABSOLUTE_LIMIT, LIVE_VIEW_PRESSURE_LIMIT, LIVE_VIEW_SOFT_LIMIT,
};
use window_layout::GrabbedDivider;
use zoom::PendingZoom;

#[cfg(test)]
use persistence::{PERSIST_DEBOUNCE, PERSIST_MAX_AGE, URL_CHECKPOINT_INTERVAL};

#[cfg(test)]
use crate::actor::{
    enqueue, finish_shutdown, spawn, tracked_operation_command, ActorExitGuard, Handle,
    PresentationDeadline, TimerWake, TryPushError, COMMAND_QUEUE_CAPACITY,
    LIFECYCLE_COMMAND_CAPACITY, NORMAL_COMMAND_CAPACITY,
};
use crate::actor::{CallbackHandle, CommandQueue};
use crate::api::{
    ChromePresentation, ChromePresentationDispatch, Command, EmitFn, SharedChrome, SharedEngine,
    SharedStore, ShutdownOutcome,
};
#[cfg(test)]
use crate::api::{ChromePresentationCallback, PresentationChrome};
#[cfg(test)]
use crate::store_reads::FAVICON_CACHE_MAX_AGE_SECONDS;
use crate::store_reads::{StoreReadQueue, StoreReadResult};

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
pub(super) const MAX_OPERATION_ID_BYTES: usize = 64;
pub(super) const MAINTENANCE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
#[cfg(not(test))]
// FIFO wait, storage-reader quiescence, snapshot construction, durability,
// native teardown, and thread joins consume this one caller-owned deadline.
pub(super) const END_TO_END_SHUTDOWN_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(8);
#[cfg(test)]
pub(super) const END_TO_END_SHUTDOWN_TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(50);

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

    pub(super) fn with_store_reads(
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

    pub(super) fn attach_queue(&mut self, queue: CommandQueue) {
        self.self_queue = Some(queue);
    }

    pub(super) fn is_shutdown(&self) -> bool {
        self.shutdown_result.is_some()
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
mod tests;
