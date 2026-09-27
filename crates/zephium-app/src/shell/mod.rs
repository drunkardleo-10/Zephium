//! Authoritative browser-shell state machine and effect coordination.

mod blocker;
mod bootstrap;
mod effects;
mod engine_events;
mod extension_actions;
mod extension_browser_requests;
mod extension_browser_surface;
mod extension_store;
mod favicons;
mod history;
mod operations;
mod page_permissions;
mod persistence;
mod presentation;
mod profile_deletion;
mod projections;
mod scope;
mod search;
mod tabs;
mod user_content_status;
mod view_lifecycle;
mod webext;
pub use webext::{WebExtensionStatus, WebExtensionTarget};
mod window_layout;
mod zoom;

use effects::{mutation_result, operation_result, NativeWork};
use extension_actions::ExtensionActionState;
use extension_browser_surface::ExtensionBrowserSurfaceState;
use favicons::{origin_of, FaviconState};
#[cfg(test)]
use favicons::{FAVICON_POLL_DELAYS, ICON_CACHE_CAPACITY};
use page_permissions::PagePermissionPromptState;
#[cfg(test)]
use presentation::PendingPresentation;
use presentation::PresentationState;
#[cfg(test)]
use presentation::MAX_PRESENTATION_ADMISSION_REJECTIONS;
use profile_deletion::{
    ProfileDeletionCoordinator, ProfileDeletionPhase, ProfileDeletionState,
    PROFILE_DELETION_STORE_TIMEOUT,
};
use search::SearchState;
use view_lifecycle::{CrashState, PendingDiscardProbe, ResidencyState, LIVE_VIEW_ABSOLUTE_LIMIT};
#[cfg(test)]
use view_lifecycle::{LIVE_VIEW_PRESSURE_LIMIT, MAX_CONCURRENT_DISCARD_PROBES};
use window_layout::GrabbedDivider;
use zoom::ZoomState;

use persistence::PersistenceState;
#[cfg(test)]
use persistence::{PERSIST_DEBOUNCE, PERSIST_MAX_AGE, URL_CHECKPOINT_INTERVAL};

#[cfg(test)]
use crate::actor::{spawn, Handle, TryPushError};
use crate::actor::{CallbackHandle, CommandQueue};
#[cfg(feature = "agentic-browser")]
use crate::api::AgentLifecycle;
use crate::api::PagePermissionPromptDecision;
use crate::api::{
    ChromePresentation, ChromePresentationDispatch, Command, ContentPolicyStatusQueryOutcome,
    EmitFn, SharedBlocker, SharedChrome, SharedEngine, SharedStore, ShellTerminalFailure,
    ShellTerminalFailureCallback, ShutdownOutcome,
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

#[cfg(feature = "agentic-browser")]
use zephium_agentic::AgentBrowserShutdownOutcome;
#[cfg(test)]
use zephium_core::extensions::ExtensionBrowserRequestId;
use zephium_core::extensions::{
    ExtensionBrowserRequest, ExtensionBrowserRequestAction, ExtensionBrowserRequestRejection,
    ExtensionBrowserRequestResult, ExtensionBrowserRequestSettlement, ExtensionBrowserSurface,
    ExtensionBrowserSurfaceGeneration, ExtensionBrowserTab, ExtensionBrowserWindow,
};
use zephium_core::geometry::{Rect, Size};
use zephium_core::ids::{ItemId, ProfileId, SpaceId, WindowId};
use zephium_core::item::{ItemKind, Lifecycle, Placement, SpaceSection, TabState};
use zephium_core::items::{Effect, Items};
use zephium_core::layout;
use zephium_core::ports::blocker::BlockerShutdownOutcome;
#[cfg(test)]
use zephium_core::ports::chrome::Chrome as GeometryChrome;
use zephium_core::ports::chrome::ChromeFrame;
#[cfg(test)]
use zephium_core::ports::engine::Engine;
use zephium_core::ports::engine::{
    ContentScope, DiscardProbeId, EngineEvent, NativeAction, NativeDispatch,
    NavigationPresentationId, Partition, ProfileDataErasureOutcome, StageMotion, ZoomRequestId,
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
    BlockerFailure, BlockerPhase, BlockerPreferenceState, BlockerProtection, BlockerRuleCoverage,
    BlockerRuntimeDiagnostics, BlockerSourceFailure, BlockerSourceIdentities, BlockerSourcePhase,
    BlockerSourceProvenance, BlockerStatusView, DividerView, ExtensionActionFailedView,
    ExtensionActionFailure, ExtensionActionRuntimeView, ExtensionActionShortcutView,
    ExtensionActionsView, ItemsState, LayoutState, OperationDisposition, OperationOutcome,
    OperationReason, PagePermissionKindView, PagePermissionPromptEntryView,
    PagePermissionPromptView, ProfileKindView, ProfileView, Projection, RuntimeSecurityAdvisory,
    RuntimeSecurityAdvisoryKind, RuntimeSecurityUpdateTarget, RuntimeStatus, SearchAction,
    SearchResult, SearchResults, SidebarNodeKindView, SidebarNodeView, SidebarSectionView,
    SpaceView, SplitGroupView, TabView,
};

// More simultaneous native renderers are neither usable in the current tiled
// layout nor safe to reconstruct synchronously after a restore/process loss.
const MAX_VISIBLE_PANES: usize = 8;
pub(super) const MAX_OPERATION_ID_BYTES: usize = 64;
pub(super) const MAINTENANCE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
#[cfg(not(test))]
// FIFO wait, storage-reader quiescence, snapshot construction, durability,
// native teardown, and thread joins consume
// this one caller-owned deadline.
pub(super) const END_TO_END_SHUTDOWN_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(8);
#[cfg(test)]
pub(super) const END_TO_END_SHUTDOWN_TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(50);

#[cfg(feature = "agentic-browser")]
enum AgentLifecycleOwner {
    Absent,
    Owned(AgentLifecycle),
    Consumed,
}

#[cfg(feature = "agentic-browser")]
impl AgentLifecycleOwner {
    fn new(lifecycle: Option<AgentLifecycle>) -> Self {
        lifecycle.map_or(Self::Absent, Self::Owned)
    }
}

mod browser_pages;

struct NativeOpener {
    source: ItemId,
    activate_when_presentable: bool,
}

pub struct Shell {
    #[cfg(feature = "work-execution")]
    work: Option<Box<crate::work::ApplicationWork>>,
    profiles: Profiles,
    spaces: Spaces,
    items: Items,
    recently_closed: Vec<zephium_core::session::PersistedClosedTab>,
    windows: Windows,
    pending_size: Size,
    favicons: FaviconState,
    history: history::HistoryState,
    search: SearchState,
    presentation: PresentationState,
    zoom: ZoomState,
    divider: Option<GrabbedDivider>,
    residency: ResidencyState,
    last_visits: std::collections::HashMap<ItemId, (String, std::time::Instant)>,
    window_visible: bool,
    browser_page: Option<(WindowId, crate::BrowserPage)>,
    browser_page_projected: Option<(WindowId, Option<crate::BrowserPage>)>,
    browser_return_revision: u64,
    browser_after_return: Option<Box<Command>>,
    browser_return_ready: bool,
    browser_return: Option<browser_pages::PendingBrowserReturn>,
    runtime_restart_required: bool,
    user_content_status: user_content_status::UserContentStatus,
    crash: CrashState,
    bootstrapped: bool,
    native_openers: std::collections::HashMap<ItemId, NativeOpener>,
    #[cfg(debug_assertions)]
    bootstrap_started: Option<std::time::Instant>,
    persistence: PersistenceState,
    shutdown_result: Option<ShutdownOutcome>,
    self_queue: Option<CommandQueue>,
    profile_deletion: ProfileDeletionCoordinator,
    /// Exact startup cohort whose per-profile history/favicon database was
    /// preserved but disabled by storage validation. Session/meta state and
    /// native website data remain independently usable.
    degraded_storage_profiles: std::collections::HashSet<ProfileId>,
    blocker: blocker::BlockerCoordinator,
    #[cfg(feature = "agentic-browser")]
    agent_lifecycle: AgentLifecycleOwner,
    extension_browser_surfaces: ExtensionBrowserSurfaceState,
    web_extensions: webext::WebExtensionState,
    extension_actions: ExtensionActionState,
    page_permissions: PagePermissionPromptState,
    terminal_failure: Option<ShellTerminalFailureCallback>,
    terminal_failure_handoff_panicked: bool,
    engine: SharedEngine,
    store: SharedStore,
    store_reads: Option<StoreReadQueue>,
    notes: Option<zephium_core::ports::notes::SharedNotes>,
    chrome: SharedChrome,
    emit: EmitFn,
    #[cfg(test)]
    auto_settle_content_rules: bool,
}

pub(super) struct ShellPorts {
    engine: SharedEngine,
    store: SharedStore,
    blocker: SharedBlocker,
    #[cfg(feature = "agentic-browser")]
    agent_lifecycle: Option<AgentLifecycle>,
    terminal_failure: ShellTerminalFailureCallback,
    chrome: SharedChrome,
    emit: EmitFn,
}

impl ShellPorts {
    pub(super) fn new(
        engine: SharedEngine,
        store: SharedStore,
        blocker: SharedBlocker,
        terminal_failure: ShellTerminalFailureCallback,
        chrome: SharedChrome,
        emit: EmitFn,
    ) -> Self {
        Self {
            engine,
            store,
            blocker,
            #[cfg(feature = "agentic-browser")]
            agent_lifecycle: None,
            terminal_failure,
            chrome,
            emit,
        }
    }

    #[cfg(feature = "agentic-browser")]
    pub(super) fn with_agent_lifecycle(mut self, lifecycle: Option<AgentLifecycle>) -> Self {
        self.agent_lifecycle = lifecycle;
        self
    }
}

impl Shell {
    #[cfg(test)]
    pub fn new(
        engine: SharedEngine,
        store: SharedStore,
        chrome: SharedChrome,
        emit: EmitFn,
    ) -> Self {
        Self::with_store_reads(
            ShellPorts::new(
                engine,
                store,
                Arc::new(tests::ImmediateAllowAllCompiler),
                Box::new(|_| {}),
                chrome,
                emit,
            ),
            None,
            true,
        )
    }

    #[cfg(test)]
    pub(super) fn new_with_blocker(
        engine: SharedEngine,
        store: SharedStore,
        blocker: SharedBlocker,
        chrome: SharedChrome,
        emit: EmitFn,
    ) -> Self {
        Self::with_store_reads(
            ShellPorts::new(engine, store, blocker, Box::new(|_| {}), chrome, emit),
            None,
            false,
        )
    }

    #[cfg(test)]
    pub(super) fn new_with_failure(
        engine: SharedEngine,
        store: SharedStore,
        blocker: SharedBlocker,
        terminal_failure: ShellTerminalFailureCallback,
        chrome: SharedChrome,
        emit: EmitFn,
    ) -> Self {
        Self::with_store_reads(
            ShellPorts::new(engine, store, blocker, terminal_failure, chrome, emit),
            None,
            true,
        )
    }

    #[cfg(all(test, feature = "agentic-browser"))]
    pub(super) fn new_with_agent_lifecycle(
        engine: SharedEngine,
        store: SharedStore,
        blocker: SharedBlocker,
        agent_lifecycle: AgentLifecycle,
        chrome: SharedChrome,
        emit: EmitFn,
    ) -> Self {
        Self::with_store_reads(
            ShellPorts::new(engine, store, blocker, Box::new(|_| {}), chrome, emit)
                .with_agent_lifecycle(Some(agent_lifecycle)),
            None,
            true,
        )
    }

    #[cfg(test)]
    pub(super) fn with_store_reads(
        ports: ShellPorts,
        store_reads: impl Into<Option<StoreReadQueue>>,
        #[cfg(test)] auto_settle_content_rules: bool,
    ) -> Self {
        let mut shell = Self::with_store_reads_deferred_blocker_catalog(
            ports,
            store_reads,
            #[cfg(test)]
            auto_settle_content_rules,
        );
        shell.initialize_blocker_catalog();
        shell
    }

    /// Builds only actor-owned state and does not enter any external port.
    /// The actor installs `ShellExitGuard` before completing catalog admission,
    /// so a panic cannot strand the Store/native/blocker cleanup graph outside
    /// an observable terminal path.
    pub(super) fn with_store_reads_deferred_blocker_catalog(
        ports: ShellPorts,
        store_reads: impl Into<Option<StoreReadQueue>>,
        #[cfg(test)] auto_settle_content_rules: bool,
    ) -> Self {
        let ShellPorts {
            engine,
            store,
            blocker,
            #[cfg(feature = "agentic-browser")]
            agent_lifecycle,
            terminal_failure,
            chrome,
            emit,
        } = ports;
        Self {
            profiles: Profiles::default(),
            spaces: Spaces::default(),
            items: Items::default(),
            recently_closed: Vec::new(),
            windows: Windows::default(),
            pending_size: Size::default(),
            favicons: FaviconState::default(),
            history: history::HistoryState::default(),
            search: SearchState {
                custom_url: store.app_setting("search.custom-url").unwrap_or_default(),
                engine: store
                    .app_setting("search.engine")
                    .as_deref()
                    .and_then(zephium_core::search::SearchEngine::from_id)
                    .unwrap_or_default(),
                ..SearchState::default()
            },
            presentation: PresentationState::default(),
            zoom: ZoomState::default(),
            divider: None,
            residency: ResidencyState::default(),
            last_visits: std::collections::HashMap::new(),
            window_visible: true,
            browser_page: None,
            browser_page_projected: None,
            browser_return_revision: 0,
            browser_after_return: None,
            browser_return_ready: false,
            browser_return: None,
            runtime_restart_required: false,
            user_content_status: user_content_status::UserContentStatus::default(),
            crash: CrashState::default(),
            bootstrapped: false,
            native_openers: std::collections::HashMap::new(),
            #[cfg(debug_assertions)]
            bootstrap_started: None,
            persistence: PersistenceState::default(),
            shutdown_result: None,
            self_queue: None,
            profile_deletion: ProfileDeletionCoordinator::default(),
            degraded_storage_profiles: std::collections::HashSet::new(),
            blocker: blocker::BlockerCoordinator::new_deferred(blocker),
            #[cfg(feature = "agentic-browser")]
            agent_lifecycle: AgentLifecycleOwner::new(agent_lifecycle),
            #[cfg(feature = "work-execution")]
            work: None,
            extension_browser_surfaces: ExtensionBrowserSurfaceState::default(),
            web_extensions: webext::WebExtensionState::default(),
            extension_actions: ExtensionActionState::default(),
            page_permissions: PagePermissionPromptState::default(),
            terminal_failure: Some(terminal_failure),
            terminal_failure_handoff_panicked: false,
            engine,
            store,
            store_reads: store_reads.into(),
            notes: None,
            chrome,
            emit,
            #[cfg(test)]
            auto_settle_content_rules,
        }
    }

    pub(super) fn initialize_blocker_catalog(&mut self) {
        self.blocker.initialize_catalog();
    }

    pub(super) fn attach_queue(&mut self, queue: CommandQueue) {
        self.attach_queue_for_terminal_cleanup(queue);
        self.schedule_blocker_catalog_activation_poll();
    }

    /// Installs only the actor's self-queue for a startup cancellation. No
    /// timer or external coordinator work is admitted before the queued
    /// terminal barrier is consumed.
    pub(super) fn attach_queue_for_terminal_cleanup(&mut self, queue: CommandQueue) {
        self.self_queue = Some(queue);
    }

    pub(super) fn is_shutdown(&self) -> bool {
        self.shutdown_result.is_some()
    }

    pub(super) fn terminal_failure_handoff_panicked(&self) -> bool {
        self.terminal_failure_handoff_panicked
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
            #[cfg(feature = "work-execution")]
            Command::AttachWork(attachment) => {
                if let Some(mut work) = crate::work::ApplicationWork::take_attachment(&attachment) {
                    if !work.belongs_to_store(&self.store)
                        || !work.belongs_to_engine(&self.engine)
                        || !work.accepts_predecessor(self.work.as_deref())
                        || !matches!(self.agent_lifecycle, AgentLifecycleOwner::Absent)
                    {
                        work.refuse_attachment();
                    } else {
                        if let Some(previous) = &self.work {
                            previous.retire_projection();
                        }
                        work.initialize();
                        self.work = Some(Box::new(work));
                    }
                }
            }
            #[cfg(feature = "work-execution")]
            Command::AdmitWork(submission) => {
                if let Some(work) = &mut self.work {
                    work.admit(submission);
                }
            }
            #[cfg(feature = "work-execution")]
            Command::WorkControl(control) => {
                if let Some(work) = &mut self.work {
                    work.control(*control);
                }
            }
            #[cfg(feature = "work-execution")]
            Command::WorkWake => {}
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
                if let Command::RespondToPagePermissionPrompt {
                    profile,
                    item,
                    request,
                    decision,
                } = &command
                {
                    if let Some(mut completion) = self.begin_page_permission_response(
                        operation_id.clone(),
                        *profile,
                        *item,
                        *request,
                        *decision,
                    ) {
                        completion.operation_id = operation_id;
                        (self.emit)(Projection::OperationProcessed(completion));
                    }
                    return;
                }
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
                if let Command::SetFocusedContentBlockerEnabled(enabled) = &command {
                    if let Some(mut completion) =
                        self.begin_focused_blocker_mutation(operation_id.clone(), *enabled)
                    {
                        completion.operation_id = operation_id;
                        (self.emit)(Projection::OperationProcessed(completion));
                    }
                    return;
                }
                if matches!(&command, Command::RefreshContentBlockerSources) {
                    if let Some(mut completion) =
                        self.begin_blocker_catalog_refresh(operation_id.clone())
                    {
                        completion.operation_id = operation_id;
                        (self.emit)(Projection::OperationProcessed(completion));
                    }
                    return;
                }
                let mut completion = self.handle_operation(command);
                completion.operation_id = operation_id;
                (self.emit)(Projection::OperationProcessed(completion));
            }
            Command::Bootstrap => self.bootstrap(),
            Command::SetWebExtensions {
                profile,
                extensions,
            } => self.set_web_extensions(profile, extensions),
            Command::ResolveWebExtensionTarget { tab, reply } => {
                let _ = reply.try_send(self.web_extension_target(tab));
            }
            Command::WebExtensionStatus { profile, reply } => {
                let _ = reply.try_send(self.web_extension_status(profile));
            }
            Command::Open => {
                let _ = self.operation_open();
            }
            Command::Activate(id) => {
                let _ = self.operation_activate(id);
            }
            Command::SetTabEssential {
                id,
                essential,
                before,
            } => {
                let _ = self.operation_set_tab_essential(id, essential, before);
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
                        self.cancel_page_permission_if_not_foreground();
                    }
                    let _ = self.relayout();
                    self.maintain_views();
                }
            }
            Command::BrowserChromeRestored { revision, applied } => {
                self.browser_chrome_restored(revision, applied)
            }
            Command::ShowBrowserPage(page) => {
                let _ = self.operation_show_browser_page(page);
            }
            Command::SetSidebarWidth(width, animate) => {
                if let Some(win) = self.windows.focused_mut() {
                    win.metrics.sidebar_width = zephium_core::layout::clamp_sidebar_width(width);
                }
                if animate {
                    if let Some(win) = self.windows.focused() {
                        let _ = self.engine.hint_stage_motion(win.id, StageMotion::Slide);
                    }
                }
                let _ = self.relayout_with(animate);
            }
            Command::DownloadCall {
                expected_profile,
                call,
                done,
            } => {
                use zephium_core::downloads::{DownloadError, DownloadResponse};
                let authorized = self
                    .windows
                    .focused()
                    .is_some_and(|window| window.profile == expected_profile)
                    && !self.profile_deletion_quarantines(expected_profile)
                    && call.validate();
                if !authorized {
                    done.finish(DownloadResponse::Error {
                        error: DownloadError::Invalid,
                    });
                } else {
                    let partition = match self
                        .profiles
                        .get(expected_profile)
                        .map(|profile| profile.kind)
                    {
                        Some(zephium_core::profiles::ProfileKind::Incognito) => {
                            zephium_core::ports::engine::Partition::Ephemeral(expected_profile)
                        }
                        Some(_) => {
                            zephium_core::ports::engine::Partition::Persistent(expected_profile)
                        }
                        None => {
                            done.finish(DownloadResponse::Error {
                                error: DownloadError::Invalid,
                            });
                            return;
                        }
                    };
                    self.engine.download_call(partition, *call, done);
                }
            }
            Command::HistoryCall {
                expected_profile,
                call,
                done,
            } => self.history_call(expected_profile, *call, done),
            Command::AttachNotes(attachment) => {
                self.notes.get_or_insert(attachment.0);
            }
            Command::NoteCall {
                expected_profile,
                call,
                done,
            } => {
                use zephium_core::notes::{NoteError, NoteReply, NoteResponse};
                // Private profiles keep nothing on disk, notes included.
                let profile = self
                    .windows
                    .focused()
                    .map(|window| window.profile)
                    .filter(|profile| *profile == expected_profile)
                    .filter(|profile| {
                        self.profiles.get(*profile).is_some_and(|p| {
                            p.kind != zephium_core::profiles::ProfileKind::Incognito
                        })
                    });
                match (profile, &self.notes) {
                    (Some(profile), Some(notes)) => notes.call(
                        profile,
                        Arc::unwrap_or_clone(call),
                        Box::new(move |response| {
                            done.finish(NoteReply {
                                profile: Some(profile.to_string()),
                                response,
                            })
                        }),
                    ),
                    _ => done.finish(NoteReply {
                        profile: None,
                        response: NoteResponse::Error {
                            error: NoteError::Unavailable,
                        },
                    }),
                }
            }
            Command::ResourceCall {
                expected_profile,
                call,
                done,
            } => {
                use zephium_core::resources::{ResourceError, ResourceReply, ResourceResponse};
                if let Some(profile) = self
                    .windows
                    .focused()
                    .map(|window| window.profile)
                    .filter(|profile| *profile == expected_profile)
                {
                    self.store.resource_call(
                        profile,
                        Arc::unwrap_or_clone(call),
                        Box::new(move |response| {
                            done.finish(ResourceReply {
                                profile: Some(profile.to_string()),
                                response,
                            })
                        }),
                    );
                } else {
                    done.finish(ResourceReply {
                        profile: None,
                        response: ResourceResponse::Error {
                            error: ResourceError::Unavailable,
                        },
                    });
                }
            }
            Command::DragOver { point } => {
                if let Some(win) = self.windows.focused().map(|w| w.id) {
                    let zone = point
                        .and_then(|(x, y)| self.resolve_drop(x, y))
                        .map(|d| d.zone);
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
            // This privileged mutation must carry a desktop operation id.
            Command::InvokeExtensionAction { .. } => {}
            Command::RespondToPagePermissionPrompt { .. } => {}
            Command::PagePermissionCatalogLoaded {
                profile,
                item,
                request,
                outcome,
            } => self.settle_page_permission_catalog_load(profile, item, request, *outcome),
            Command::PagePermissionCatalogMutated {
                profile,
                item,
                request,
                outcome,
            } => self.settle_page_permission_catalog_mutation(profile, item, request, *outcome),
            Command::PagePermissionTimeout {
                profile,
                item,
                request,
            } => self.on_page_permission_timeout(profile, item, request),
            Command::Search(query) => {
                self.search.context = None;
                self.search(&query);
            }
            Command::SearchSupplementaryFinished { context, query } => {
                self.search_supplementary_finished(*context, query)
            }
            Command::SearchAdditional {
                context,
                query,
                results,
            } => self.search_additional(*context, query, results),
            Command::SearchScoped { query, context } => self.search_scoped(&query, *context),
            Command::CancelSearch { session_id } => self.cancel_scoped_search(&session_id),
            Command::RunSearchAction { context, action } => {
                let _ = self.operation_run_search_action(*context, action);
            }
            Command::OpenUrl { input, new_tab } => {
                let _ = self.operation_open_url(input, new_tab);
            }
            Command::SetAppSetting { key, value } => {
                let _ = self.operation_set_app_setting(key, value);
            }
            // These mutations are accepted only through `Command::Operation`
            // so every foreground request has one truthful terminal identity.
            Command::DeleteProfile(_)
            | Command::RetryContentPolicy { .. }
            | Command::SetFocusedContentBlockerEnabled(_)
            | Command::RetryFocusedContentPolicy { .. }
            | Command::RefreshContentBlockerSources => {}
            Command::ContentPolicyStatus { profile, reply } => {
                let outcome = self
                    .blocker
                    .status(profile)
                    .map(ContentPolicyStatusQueryOutcome::Found)
                    .unwrap_or(ContentPolicyStatusQueryOutcome::UnknownProfile);
                let _ = reply.send(outcome);
            }
            Command::FocusedContentPolicyStatus { reply } => {
                self.maintain_blocker_catalog();
                let _ = reply.send(self.focused_blocker_status_view());
            }
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
            Command::BlockerReady(profile) => self.consume_blocker_compile_result(profile),
            Command::BlockerStoreReady(profile) => self.consume_blocker_store_result(profile),
            Command::BlockerPreferenceRetry { profile, token } => {
                self.on_blocker_preference_reconciliation_retry(profile, token)
            }
            Command::BlockerCatalogPoll { operation, attempt } => {
                self.on_blocker_catalog_poll(operation, attempt)
            }
            Command::ProfileDeletionRetry {
                profile,
                generation,
            } => {
                if self
                    .profile_deletion
                    .states
                    .get(&profile)
                    .is_some_and(|state| state.retry_generation == generation)
                {
                    self.drive_profile_deletion(profile);
                }
            }
            Command::StoreRead(result) => self.on_store_read(result),
            Command::Persist => self.persist(),
            Command::Tick => {
                if !self.bootstrapped {
                    self.bootstrap();
                    if !self.bootstrapped {
                        return;
                    }
                }
                self.maintain_blocker_catalog();
                self.drain_blocker_inbox();
                self.drive_blocker_preference_reconciliations();
                self.drain_profile_deletion_inbox();
                self.reconcile_runtime_restart_requirement();
                let extension_surfaces = self.retry_extension_browser_surfaces();
                if extension_surfaces.native.rejected {
                    crate::diagnostic!(
                        "extensions: maintenance could not reconcile browser metadata"
                    );
                }
                let extension_actions = self.maintain_extension_actions();
                if extension_actions.rejected {
                    crate::diagnostic!("extensions: maintenance could not refresh toolbar actions");
                }
                if self.maintain_views() {
                    self.project_items();
                }
            }
            Command::Engine(event) => self.on_engine_event(event),
            Command::Shutdown { deadline, ack } => self.shutdown_until(deadline, ack),
        }
        #[cfg(feature = "work-execution")]
        self.poll_work();
    }

    #[cfg(feature = "work-execution")]
    fn poll_work(&mut self) {
        if let Some(work) = &mut self.work {
            work.poll();
            if let Some(queue) = &self.self_queue {
                queue.schedule_work(work.next_deadline());
            }
        }
    }

    fn shutdown_until(&mut self, deadline: std::time::Instant, ack: SyncSender<ShutdownOutcome>) {
        #[cfg(feature = "work-execution")]
        if let Some(work) = &mut self.work {
            work.begin_shutdown();
        }
        if std::time::Instant::now() >= deadline {
            self.retryable_shutdown_failure(ack);
            return;
        }

        self.cancel_pending_page_permission_for_shutdown();

        let mut terminal_clean = true;
        if let Some(reads) = &self.store_reads {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                reads.quiesce_until(deadline)
            })) {
                Ok(true) => {}
                Ok(false) => {
                    self.retryable_shutdown_failure(ack);
                    return;
                }
                Err(_) => {
                    crate::diagnostic!("shutdown: storage-reader quiescence panicked");
                    terminal_clean = false;
                }
            }
        }
        self.clear_pending_store_reads();

        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.persist())).is_err() {
            crate::diagnostic!("shutdown: final session snapshot panicked");
            terminal_clean = false;
        }

        // Preserve retryability only while every earlier boundary is still
        // known-good.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.store.flush_until(deadline)
        })) {
            Ok(true) => {}
            Ok(false) if terminal_clean => {
                self.retryable_shutdown_failure(ack);
                return;
            }
            Ok(false) => {
                crate::diagnostic!("shutdown: storage durability preflight was not proven");
                terminal_clean = false;
            }
            Err(_) => {
                crate::diagnostic!("shutdown: storage durability preflight panicked");
                terminal_clean = false;
            }
        }

        #[cfg(feature = "agentic-browser")]
        let agent_lifecycle_clean = self.shutdown_agent_lifecycle_until(deadline);
        #[cfg(not(feature = "agentic-browser"))]
        let agent_lifecycle_clean = true;
        // Fold every result already published before Store's terminal
        // barrier while ordinary Store/native admission is still valid. Any
        // follow-up reconciliation is then ordered ahead of Store shutdown.
        // A projection or adapter panic is terminal, but cannot skip the
        // independent Store/native/blocker barriers below.
        let pre_store_coordination_clean = if std::time::Instant::now() >= deadline {
            false
        } else {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.drain_blocker_inbox();
            }))
            .is_ok()
        };
        if !pre_store_coordination_clean {
            crate::diagnostic!("shutdown: pre-Store blocker result folding panicked");
        }
        let storage_clean = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.store.shutdown_until(deadline)
        })) {
            Ok(StoreShutdownOutcome::Clean) => true,
            Ok(StoreShutdownOutcome::RetryableFailure) => {
                crate::diagnostic!("shutdown: storage rejected terminal teardown");
                false
            }
            Ok(StoreShutdownOutcome::Unclean) => {
                crate::diagnostic!(
                    "shutdown: storage actor termination was not proven before the deadline"
                );
                false
            }
            Err(_) => {
                crate::diagnostic!("shutdown: storage terminal barrier panicked");
                false
            }
        };

        // Projection callbacks are composition ports too. Contain this phase
        // independently so an unhealthy UI cannot skip native or blocker
        // teardown after terminal ownership transfer has begun.
        let post_store_coordination_clean =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.discard_blocker_inbox_for_shutdown();
                self.finish_pending_blocker_operations_for_shutdown();
                self.fail_pending_history_calls();
            }))
            .is_ok();
        if !post_store_coordination_clean {
            crate::diagnostic!("shutdown: pending operation finalization panicked");
        }
        let coordination_clean = pre_store_coordination_clean && post_store_coordination_clean;
        let reads_stopped = self.store_reads.as_ref().is_none_or(|reads| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reads.stop())).is_ok()
        });
        if !reads_stopped {
            crate::diagnostic!("shutdown: storage-reader stop panicked");
        }

        let (native_clean, blocker_clean) = self.shutdown_native_and_blocker_until(deadline);

        let clean = terminal_clean
            && agent_lifecycle_clean
            && storage_clean
            && coordination_clean
            && reads_stopped
            && native_clean
            && blocker_clean;
        let outcome = if clean {
            ShutdownOutcome::Clean
        } else {
            ShutdownOutcome::Unclean
        };
        self.shutdown_result = Some(outcome);
        let _ = ack.send(outcome);
    }

    fn shutdown_native_and_blocker_until(&self, deadline: std::time::Instant) -> (bool, bool) {
        let (native_done, native_wait) = sync_channel(1);
        let native_admitted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.engine.shutdown(Box::new(move |clean| {
                let _ = native_done.send(clean);
            }));
        }))
        .is_ok();
        if !native_admitted {
            crate::diagnostic!("shutdown: native cleanup admission panicked");
        }

        // Native teardown and blocker joins are independent and share the
        // same caller-owned absolute deadline.
        let blocker_clean = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.blocker.shutdown_until(deadline)
        })) {
            Ok(BlockerShutdownOutcome::Clean) => true,
            Ok(BlockerShutdownOutcome::Unclean) => {
                crate::diagnostic!(
                    "shutdown: content-policy compiler termination was not proven before the deadline"
                );
                false
            }
            Err(_) => {
                crate::diagnostic!("shutdown: content-policy compiler shutdown panicked");
                false
            }
        };
        let native_budget = deadline.saturating_duration_since(std::time::Instant::now());
        // Observe the callback independently even when admission panicked: a
        // faulty adapter may have retained callback ownership before unwind.
        let native_ack_clean = native_wait.recv_timeout(native_budget).unwrap_or(false);
        let native_clean = native_admitted && native_ack_clean;
        if !native_clean {
            crate::diagnostic!(
                "shutdown: native cleanup did not acknowledge cleanly; forcing process exit"
            );
        }
        (native_clean, blocker_clean)
    }

    /// Best-effort terminal cleanup for actor unwind or loss of the last
    /// public handle. No mutable state is persisted and no refusal is
    /// retryable because the sole authoritative actor is already exiting.
    pub(super) fn cleanup_after_unexpected_exit_until(
        &mut self,
        deadline: std::time::Instant,
    ) -> bool {
        let reads_quiesced = self.store_reads.as_ref().is_none_or(|reads| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                reads.quiesce_until(deadline)
            }))
            .unwrap_or(false)
        });
        let reads_stopped = self.store_reads.as_ref().is_none_or(|reads| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reads.stop())).is_ok()
        });
        #[cfg(feature = "agentic-browser")]
        let agent_lifecycle_clean = self.shutdown_agent_lifecycle_until(deadline);
        #[cfg(not(feature = "agentic-browser"))]
        let agent_lifecycle_clean = true;
        let storage_clean = matches!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.store.shutdown_until(deadline)
            })),
            Ok(StoreShutdownOutcome::Clean)
        );
        if !storage_clean {
            crate::diagnostic!(
                "shutdown: storage cleanup was not proven during unexpected shell exit"
            );
        }
        let (native_clean, blocker_clean) = self.shutdown_native_and_blocker_until(deadline);
        reads_quiesced
            && reads_stopped
            && agent_lifecycle_clean
            && storage_clean
            && native_clean
            && blocker_clean
    }

    pub(super) fn report_terminal_failure(&mut self, failure: ShellTerminalFailure) {
        let Some(callback) = self.terminal_failure.take() else {
            return;
        };
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(failure))).is_err() {
            crate::diagnostic!("shutdown: terminal shell failure handoff panicked");
            self.terminal_failure_handoff_panicked = true;
        }
    }

    /// Consumes the optional complete agent runtime before terminal Store and
    /// engine teardown. A clean result cannot exist without its native zero
    /// proof; the proof is deliberately consumed inside the actor barrier.
    #[cfg(feature = "agentic-browser")]
    fn shutdown_agent_lifecycle_until(&mut self, deadline: std::time::Instant) -> bool {
        #[cfg(feature = "work-execution")]
        if let Some(work) = &mut self.work {
            return work.shutdown_until(deadline);
        }
        let owner = std::mem::replace(&mut self.agent_lifecycle, AgentLifecycleOwner::Consumed);
        let lifecycle = match owner {
            AgentLifecycleOwner::Absent => return true,
            AgentLifecycleOwner::Owned(lifecycle) => lifecycle,
            AgentLifecycleOwner::Consumed => {
                crate::diagnostic!("shutdown: agent browser lifecycle owner is missing");
                return false;
            }
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            lifecycle.shutdown_until(deadline)
        })) {
            Ok(AgentBrowserShutdownOutcome::Clean(native_zero_proof)) => {
                drop(native_zero_proof);
                true
            }
            Ok(AgentBrowserShutdownOutcome::Unclean) => {
                crate::diagnostic!(
                    "shutdown: agent browser lifecycle did not prove complete cleanup"
                );
                false
            }
            Err(_) => {
                crate::diagnostic!("shutdown: agent browser lifecycle shutdown panicked");
                false
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
        let now = std::time::Instant::now();
        if let Some(queue) = &self.self_queue {
            // A timer wake removes its entry before trying to enter the actor.
            // If it raced the shutdown barrier it was truthfully rejected as
            // sealed, so explicitly restore every still-live exact reveal
            // obligation when the retryable barrier reopens. Both maps remain
            // bounded to one entry per logical item.
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
                stale,
            } => self.on_favicon_read(generation, id, profile, origin, rgba, stale),
            StoreReadResult::FaviconBatch {
                generation,
                profile,
                space,
                origins,
                rasters,
            } => self.on_favicon_batch_read(generation, profile, space, origins, rasters),
            StoreReadResult::HistorySurface {
                token,
                profile,
                visits,
                next,
                removed,
            } => self.on_history_surface_read(token, profile, visits, next, removed),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
