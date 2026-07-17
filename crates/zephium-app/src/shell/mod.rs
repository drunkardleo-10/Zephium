//! Authoritative browser-shell state machine and effect coordination.

mod bootstrap;
mod effects;
mod engine_events;
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

use effects::{mutation_result, operation_result, NativeWork};
use favicons::{origin_of, FaviconState};
#[cfg(test)]
use favicons::{FAVICON_POLL_DELAYS, ICON_CACHE_CAPACITY};
#[cfg(test)]
use presentation::PendingPresentation;
use presentation::PresentationState;
#[cfg(test)]
use presentation::MAX_PRESENTATION_ADMISSION_REJECTIONS;
use profile_deletion::{
    ProfileDeletionInbox, ProfileDeletionPhase, ProfileDeletionState,
    PROFILE_DELETION_STORE_TIMEOUT,
};
use search::SearchState;
#[cfg(test)]
use view_lifecycle::MAX_CONCURRENT_DISCARD_PROBES;
use view_lifecycle::{
    PendingDiscardProbe, DISCARD_IDLE_GRACE, DISCARD_PROBE_TIMEOUT, DISCARD_PROTECTED_RETRY,
    LIVE_VIEW_ABSOLUTE_LIMIT, LIVE_VIEW_PRESSURE_LIMIT, LIVE_VIEW_SOFT_LIMIT,
};
use window_layout::GrabbedDivider;
use zoom::ZoomState;

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

#[cfg(test)]
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

pub struct Shell {
    profiles: Profiles,
    spaces: Spaces,
    items: Items,
    windows: Windows,
    pending_size: Size,
    favicons: FaviconState,
    search: SearchState,
    presentation: PresentationState,
    zoom: ZoomState,
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
            favicons: FaviconState::default(),
            search: SearchState::default(),
            presentation: PresentationState::default(),
            zoom: ZoomState::default(),
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
            for (id, pending) in &self.presentation.pending_presentations {
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

    fn clear_pending_store_reads(&mut self) {
        self.search.pending = None;
        self.favicons.pending_batch = None;
        self.favicons.store_reads.clear();
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
}

#[cfg(test)]
mod tests;
