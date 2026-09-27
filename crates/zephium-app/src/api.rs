//! Stable application-shell protocol exposed to the desktop composition root.

#[path = "api_store_install.rs"]
mod store_install;
pub(crate) use store_install::StoreExtensionOrigin;
pub use store_install::{
    StoreExtensionContext, StoreExtensionPackageSubmission, StoreExtensionPreparationCompletion,
    StoreExtensionUpdateDispatch, StoreExtensionUpdateResult,
};

use std::fmt;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};

use zephium_core::blocker::{ContentPolicyGeneration, ProfileContentPolicyStatus};
use zephium_core::extensions::{
    ExtensionActionRevision, ExtensionGrantRevision, ExtensionInstallCatalogRevision,
    ExtensionInstallRevision, ExtensionPopupAnchor, ExtensionProfilePolicyRevision,
    ExtensionRuntimeInstance,
};
use zephium_core::geometry::Size;
use zephium_core::ids::{ExtensionInstallId, ItemId, ProfileId};
use zephium_core::ports::blocker::ContentBlocker;
use zephium_core::ports::chrome::Chrome as GeometryChrome;
use zephium_core::ports::engine::{DiscardProbeId, Engine, EngineEvent, NavigationPresentationId};
use zephium_core::ports::extensions::ExtensionServiceLifecycle;
use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationCallback, ExtensionAcquiredCatalogActivationOutcome,
    ExtensionAcquiredCatalogActivationRequest, ExtensionAcquiredPackageProvisioningCallback,
    ExtensionAcquiredPackageProvisioningOutcome, ExtensionAcquiredPackageProvisioningRequest,
    ExtensionDistributionStatus, ExtensionGrantEditOutcome, ExtensionGrantEditTarget,
    ExtensionInstallOutcome, ExtensionManagementCatalogOutcome, ExtensionManagementSettlement,
    ExtensionProfilePolicyEditOutcome, ExtensionRepositoryMaintenanceOutcome,
    ExtensionRuntimeGrantOutcome, ExtensionRuntimeGrantRequestId, ExtensionSetEnabledOutcome,
    ExtensionUninstallOutcome, IsolatedExtensionDocumentKind, IsolatedExtensionResourceOutcome,
};
use zephium_core::ports::store::Store;
use zephium_core::ports::store::{
    PagePermissionCatalogLoadOutcome, PagePermissionCatalogMutationOutcome,
};
use zephium_core::split::Axis;
use zephium_ipc::{BlockerStatusView, Projection, TabView};

use crate::store_reads::StoreReadResult;

#[cfg(feature = "agentic-browser")]
use zephium_agentic::AgentBrowserLifecycle;

struct AcquiredPackageSubmissionInner {
    request: ExtensionAcquiredPackageProvisioningRequest,
    deadline: std::time::Instant,
    done: ExtensionAcquiredPackageProvisioningCallback,
}

/// Opaque, exactly-once transfer of one authenticated acquired package into
/// the Shell-owned extension lifecycle.
///
/// Cloning shares the same one-shot slot solely because [`Command`] is
/// cloneable for post-shutdown recovery. Product composition cannot inspect,
/// replace, or duplicate the move-only package bytes.
#[derive(Clone)]
pub struct AcquiredExtensionPackageSubmission {
    inner: Arc<Mutex<Option<AcquiredPackageSubmissionInner>>>,
}

impl AcquiredExtensionPackageSubmission {
    pub(crate) fn new(
        request: ExtensionAcquiredPackageProvisioningRequest,
        deadline: std::time::Instant,
        done: ExtensionAcquiredPackageProvisioningCallback,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some(AcquiredPackageSubmissionInner {
                request,
                deadline,
                done,
            }))),
        }
    }

    pub(crate) fn take(
        &self,
    ) -> Option<(
        ExtensionAcquiredPackageProvisioningRequest,
        std::time::Instant,
        ExtensionAcquiredPackageProvisioningCallback,
    )> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()?;
        Some((inner.request, inner.deadline, inner.done))
    }

    pub(crate) fn settle_unavailable(&self) {
        if let Some((_request, _deadline, done)) = self.take() {
            settle_callback(
                done,
                ExtensionAcquiredPackageProvisioningOutcome::Unavailable,
            );
        }
    }
}

impl fmt::Debug for AcquiredExtensionPackageSubmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcquiredExtensionPackageSubmission")
            .field(
                "pending",
                &self.inner.lock().map_or(true, |slot| slot.is_some()),
            )
            .finish_non_exhaustive()
    }
}

struct AcquiredCatalogSubmissionInner {
    request: ExtensionAcquiredCatalogActivationRequest,
    deadline: std::time::Instant,
    done: ExtensionAcquiredCatalogActivationCallback,
}

/// Opaque, exactly-once transfer of one source-free catalog activation into
/// the Shell-owned extension lifecycle.
#[derive(Clone)]
pub struct AcquiredExtensionCatalogSubmission {
    inner: Arc<Mutex<Option<AcquiredCatalogSubmissionInner>>>,
}

impl AcquiredExtensionCatalogSubmission {
    pub(crate) fn new(
        request: ExtensionAcquiredCatalogActivationRequest,
        deadline: std::time::Instant,
        done: ExtensionAcquiredCatalogActivationCallback,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some(AcquiredCatalogSubmissionInner {
                request,
                deadline,
                done,
            }))),
        }
    }

    pub(crate) fn take(
        &self,
    ) -> Option<(
        ExtensionAcquiredCatalogActivationRequest,
        std::time::Instant,
        ExtensionAcquiredCatalogActivationCallback,
    )> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()?;
        Some((inner.request, inner.deadline, inner.done))
    }

    pub(crate) fn settle_unavailable(&self) {
        if let Some((_request, _deadline, done)) = self.take() {
            settle_callback(done, ExtensionAcquiredCatalogActivationOutcome::Unavailable);
        }
    }
}

impl fmt::Debug for AcquiredExtensionCatalogSubmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcquiredExtensionCatalogSubmission")
            .field(
                "pending",
                &self.inner.lock().map_or(true, |slot| slot.is_some()),
            )
            .finish_non_exhaustive()
    }
}

fn settle_callback<T>(done: Box<dyn FnOnce(T) + Send>, outcome: T) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| done(outcome)));
}

pub type SharedEngine = Arc<dyn Engine + Send + Sync>;
pub type SharedStore = Arc<dyn Store + Send + Sync>;
pub type SharedBlocker = Arc<dyn ContentBlocker + Send + Sync>;
pub type SharedChrome = Arc<dyn PresentationChrome + Send + Sync>;
/// Unique application-owned lifecycle authority for the agent browser.
///
/// This exists only in the dormant agentic composition graph. It is consumed
/// before terminal Store and engine teardown and has no cloneable shutdown
/// surface.
#[cfg(feature = "agentic-browser")]
pub type AgentLifecycle = Box<dyn AgentBrowserLifecycle>;
/// Unique application-owned lifecycle authority for the extension service.
///
/// Unlike the cloneable observation handles exposed by the concrete service,
/// this owner moves onto the shell actor and is consumed exactly once during
/// ordered process shutdown.
pub type ExtensionLifecycle = Box<dyn ExtensionServiceLifecycle>;
/// Maximum user-owned extension management operations awaiting serialized
/// service settlement. This also reserves critical Shell mailbox capacity.
pub const MAX_PENDING_EXTENSION_MANAGEMENT_OPERATIONS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionManagementCompletion {
    Install(ExtensionManagementSettlement<ExtensionInstallOutcome>),
    Update(ExtensionManagementSettlement<zephium_core::ports::extensions::ExtensionUpdateOutcome>),
    SetEnabled(ExtensionManagementSettlement<ExtensionSetEnabledOutcome>),
    Uninstall(ExtensionManagementSettlement<ExtensionUninstallOutcome>),
    GrantEdit(ExtensionManagementSettlement<ExtensionGrantEditOutcome>),
    ProfilePolicy(ExtensionManagementSettlement<ExtensionProfilePolicyEditOutcome>),
}
/// Redacted terminal reason delivered to the desktop composition root when
/// the shell can no longer continue safely in the current process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellTerminalFailure {
    ExtensionStartupCleanupRequired,
    ExtensionStartupFailedClosed,
    ExtensionStartupLifecyclePanicked,
    ExtensionStartupLifecycleMissing,
    ExtensionProfileRetirementFailedClosed,
    ExtensionProfileRetirementBoundaryPanicked,
    ExtensionProfileRetirementLifecycleMissing,
    ExtensionProfileRetirementContractViolated,
    ExtensionProfileDeletionInvariant,
    ExtensionDistributionLifecyclePanicked,
    ExtensionDistributionLifecycleMissing,
    ActorExitedUnexpectedly,
}

impl std::fmt::Display for ShellTerminalFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ExtensionStartupCleanupRequired => "extension cleanup is still required",
            Self::ExtensionStartupFailedClosed => "extension startup failed closed",
            Self::ExtensionStartupLifecyclePanicked => "extension startup lifecycle panicked",
            Self::ExtensionStartupLifecycleMissing => {
                "extension startup lifecycle owner is missing"
            }
            Self::ExtensionProfileRetirementFailedClosed => {
                "extension profile retirement failed closed"
            }
            Self::ExtensionProfileRetirementBoundaryPanicked => {
                "extension profile retirement boundary panicked"
            }
            Self::ExtensionProfileRetirementLifecycleMissing => {
                "extension profile retirement lifecycle owner is missing"
            }
            Self::ExtensionProfileRetirementContractViolated => {
                "extension profile retirement continuation contract was violated"
            }
            Self::ExtensionProfileDeletionInvariant => {
                "profile deletion violated a post-retirement invariant"
            }
            Self::ExtensionDistributionLifecyclePanicked => {
                "extension distribution lifecycle panicked"
            }
            Self::ExtensionDistributionLifecycleMissing => {
                "extension distribution lifecycle owner is missing"
            }
            Self::ActorExitedUnexpectedly => "application shell actor exited unexpectedly",
        })
    }
}

/// One-shot terminal shell handoff. Startup failures invoke it so the desktop
/// can enqueue Shell-owned orderly shutdown; unexpected actor exit invokes it
/// after bounded best-effort cleanup. Platform adapters must dispatch onto
/// their native event loop rather than performing teardown inline.
pub type ShellTerminalFailureCallback = Box<dyn FnOnce(ShellTerminalFailure) + Send>;
pub type EmitFn = Box<dyn Fn(Projection) + Send + Sync>;

/// Exact privileged-chrome work that must complete before one raw document
/// can become visible. The tab projection is carried in the same native eval
/// as the acknowledgement, avoiding an ordering assumption between generic
/// projection delivery and native content presentation.
#[derive(Clone, Debug, PartialEq)]
pub struct ChromePresentation {
    pub settings_visible: bool,
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
    fn restore_browser_chrome(
        &self,
        _revision: u64,
        _items: zephium_ipc::ItemsState,
        _done: ChromePresentationCallback,
    ) -> ChromePresentationDispatch {
        ChromePresentationDispatch::Rejected
    }

    fn apply_tab_for_presentation(
        &self,
        presentation: ChromePresentation,
        done: ChromePresentationCallback,
    ) -> ChromePresentationDispatch;
}

/// Terminal result of the ordered application shutdown protocol.
///
/// A retryable failure happens before the unique extension-service owner or
/// native engine is torn down and leaves the actor live. `Unclean` is
/// terminal: either the actor exited without completing the barrier or one of
/// agent lifecycle, extension, Store, blocker, or native cleanup was not
/// proven, so the process must exit unsuccessfully.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownOutcome {
    RetryableFailure,
    Clean,
    Unclean,
}

/// Browser-owned response vocabulary for one exact page capability request.
/// `Always*` is authority to mutate the durable per-profile catalog; `*Once`
/// never writes policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagePermissionPromptDecision {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}

/// Ordered result of a trusted profile content-policy status query.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentPolicyStatusQueryOutcome {
    Found(ProfileContentPolicyStatus),
    UnknownProfile,
    /// The query was not admitted or the actor exited before replying.
    Unavailable,
}

/// A bounded browser-owned destination rendered by the existing chrome view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserPage {
    Work,
    Settings,
    Extensions,
    History,
    Downloads,
    Tasks,
    Notes,
}

impl BrowserPage {
    pub fn command_id(self) -> &'static str {
        match self {
            Self::Work => "browser.work",
            Self::Settings => "browser.settings",
            Self::Extensions => "browser.extensions",
            Self::History => "browser.history",
            Self::Downloads => "browser.downloads",
            Self::Tasks => "browser.tasks",
            Self::Notes => "browser.notes",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Command {
    ResourceCall {
        expected_profile: ProfileId,
        call: Arc<zephium_core::resources::ResourceCall>,
        done: ResourceCompletion,
    },
    DownloadCall {
        expected_profile: ProfileId,
        call: Box<zephium_core::downloads::DownloadCall>,
        done: zephium_core::downloads::DownloadCompletion,
    },
    HistoryCall {
        expected_profile: ProfileId,
        call: Box<zephium_ipc::HistoryCall>,
        done: HistoryCompletion,
    },
    /// Hands the shell its notes service, once, after startup.
    AttachNotes(NotesAttachment),
    NoteCall {
        expected_profile: ProfileId,
        call: Arc<zephium_core::notes::NoteCall>,
        done: NoteCompletion,
    },
    #[cfg(feature = "work-execution")]
    AttachWork(crate::work::WorkAttachment),
    #[cfg(feature = "work-execution")]
    AdmitWork(crate::work::WorkSubmission),
    #[cfg(feature = "work-execution")]
    WorkControl(Box<crate::work::WorkCommand>),
    #[cfg(feature = "work-execution")]
    WorkWake,
    /// A privileged user mutation with an externally visible admission and
    /// actor-order disposition identity. Engine callbacks and replaceable UI
    /// facts never use this wrapper.
    Operation {
        operation_id: String,
        command: Box<Command>,
    },
    Bootstrap,
    /// Wake hint only; Shell reobserves the extension service before admission.
    ExtensionStartupChanged,
    Open,
    Activate(ItemId),
    Close(ItemId),
    SetTabEssential {
        id: ItemId,
        essential: bool,
        before: Option<ItemId>,
    },
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
    /// The sidebar's width, and whether it changed by a deliberate change of
    /// shape — a toggle, a snap, a tool opening — that the content should
    /// travel with, rather than by a drag that it should simply follow.
    SetSidebarWidth(f64, bool),
    ShowBrowserPage(Option<BrowserPage>),
    BrowserChromeRestored {
        revision: u64,
        applied: bool,
    },
    DragOver {
        point: Option<(f64, f64)>,
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
    /// Trusted browser-chrome toolbar intent. The Shell derives the active tab
    /// and exact browser-surface generation; callers can only echo one action
    /// runtime/revision from the latest privileged projection.
    InvokeExtensionAction {
        runtime: ExtensionRuntimeInstance,
        revision: ExtensionActionRevision,
        anchor: ExtensionPopupAnchor,
    },
    /// Changes one installed extension in the focused profile. All selector
    /// fields must originate from the latest privileged management
    /// projection; Shell supplies the profile identity itself.
    SetFocusedExtensionEnabled {
        install: ExtensionInstallId,
        expected_catalog: ExtensionInstallCatalogRevision,
        expected_install: ExtensionInstallRevision,
        enabled: bool,
    },
    /// Changes one optional API or host grant from the exact installed row.
    /// The target index is resolved again against authenticated manifest data.
    EditFocusedExtensionOptionalGrant {
        install: ExtensionInstallId,
        expected_catalog: ExtensionInstallCatalogRevision,
        expected_install: ExtensionInstallRevision,
        expected_grant: ExtensionGrantRevision,
        target: ExtensionGrantEditTarget,
        granted: bool,
    },
    /// Pauses or resumes every extension in the focused profile.
    SetFocusedProfileExtensionsPaused {
        expected_policy: ExtensionProfilePolicyRevision,
        paused: bool,
    },
    /// Enables or disables extensions on the focused tab's browser-derived
    /// whole-host scope. No URL or host crosses privileged IPC.
    SetFocusedSiteExtensionsEnabled {
        expected_policy: ExtensionProfilePolicyRevision,
        enabled: bool,
    },
    /// Installs one exact package from the latest privileged management
    /// projection. Shell derives the focused profile and complete package
    /// selector; chrome can choose only projected optional-entry indexes and
    /// the two explicit browsing-scope decisions.
    InstallFocusedExtension {
        candidate_index: u8,
        expected_catalog: ExtensionInstallCatalogRevision,
        optional_api_indices: Vec<u8>,
        optional_host_indices: Vec<u8>,
        file_access: bool,
        private_access: bool,
    },
    /// Approves the exact changed-required-authority update retained by the
    /// focused profile's current management subscription. The opaque review
    /// token carries no package or permission authority.
    ApproveFocusedExtensionUpdate {
        review: u64,
    },
    /// Removes one exact installed extension from the focused profile after
    /// the extension service proves regular/private native absence.
    UninstallFocusedExtension {
        install: ExtensionInstallId,
        expected_catalog: ExtensionInstallCatalogRevision,
        expected_install: ExtensionInstallRevision,
    },
    /// Opens the exact declared options page for one currently projected
    /// active install. Shell rejoins all authority from its visible catalog.
    OpenFocusedExtensionOptions {
        install: ExtensionInstallId,
        expected_catalog: ExtensionInstallCatalogRevision,
        expected_install: ExtensionInstallRevision,
    },
    /// Opens or closes the focused profile's lazy privileged management
    /// subscription. Opening performs one explicit authenticated read; closing
    /// invalidates late callbacks and retains no polling work.
    SetExtensionManagementVisible(bool),
    /// Browser-owned response to the exact currently projected native
    /// optional-grant prompt. Public composition wraps this durable mutation
    /// in `Operation`; all identities are stale-resistant echo tokens.
    RespondToExtensionRuntimeGrantPrompt {
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
        allow: bool,
    },
    /// Browser-owned response to the exact currently projected foreground
    /// page request. Public composition wraps this in `Operation`.
    RespondToPagePermissionPrompt {
        profile: ProfileId,
        item: ItemId,
        request: zephium_core::permissions::PagePermissionRequestId,
        decision: PagePermissionPromptDecision,
    },
    /// Internal callback from one admitted on-demand durable catalog read.
    PagePermissionCatalogLoaded {
        profile: ProfileId,
        item: ItemId,
        request: zephium_core::permissions::PagePermissionRequestId,
        outcome: Box<PagePermissionCatalogLoadOutcome>,
    },
    /// Internal callback from one admitted remembered-decision mutation.
    PagePermissionCatalogMutated {
        profile: ProfileId,
        item: ItemId,
        request: zephium_core::permissions::PagePermissionRequestId,
        outcome: Box<PagePermissionCatalogMutationOutcome>,
    },
    /// Hard Shell-side bound shorter than the native completion watchdog.
    PagePermissionTimeout {
        profile: ProfileId,
        item: ItemId,
        request: zephium_core::permissions::PagePermissionRequestId,
    },
    /// Internal exactly-once handoff from an admitted extension-service
    /// management callback. It is never accepted through public operation
    /// dispatch.
    ExtensionManagementSettled {
        request: u64,
        completion: ExtensionManagementCompletion,
    },
    /// Internal exactly-once handoff for one admitted catalog read.
    ExtensionManagementCatalogSettled {
        request: u64,
        profile: ProfileId,
        outcome: ExtensionManagementCatalogOutcome,
    },
    /// Internal exactly-once callback from the serialized grant transaction.
    ExtensionRuntimeGrantSettled {
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
        settlement: Box<ExtensionManagementSettlement<ExtensionRuntimeGrantOutcome>>,
    },
    /// Move-only authenticated bytes from the extension service. Clones of
    /// this internal command share one take-once slot; no public operation can
    /// construct an authoritative native URL-scheme response from it.
    IsolatedExtensionResourceSettled {
        runtime: ExtensionRuntimeInstance,
        kind: IsolatedExtensionDocumentKind,
        request: u64,
        outcome: Arc<Mutex<Option<IsolatedExtensionResourceOutcome>>>,
    },
    /// Internal exactly-once callback from one bounded repository-maintenance
    /// turn. It is never accepted through public operation dispatch.
    ExtensionRepositoryMaintenanceSettled(ExtensionRepositoryMaintenanceOutcome),
    /// Internal move-only package handoff from the product distribution
    /// worker. Public operation dispatch never admits this command.
    ProvisionAcquiredExtensionPackage(AcquiredExtensionPackageSubmission),
    /// Native query for one foreground store listing; never public operation dispatch.
    ResolveStoreExtensionContext {
        tab: ItemId,
        reply: SyncSender<Option<StoreExtensionContext>>,
    },
    /// Captures an installed extension selected from the active management catalog.
    ResolveStoreExtensionUpdateContext {
        install: zephium_core::ids::ExtensionInstallId,
        reply: SyncSender<Option<StoreExtensionContext>>,
    },
    /// Bounded native downloader handoff, revalidated by Shell before preparation.
    PrepareStoreExtensionPackage(StoreExtensionPackageSubmission),
    /// Internal service callback; consent presentation still requires current context.
    StoreExtensionPreparationCompleted(StoreExtensionPreparationCompletion),
    /// In-process composition only; not exposed as renderer IPC.
    ConfigureStoreExtensionUpdates(StoreExtensionUpdateDispatch),
    StoreExtensionUpdateCatalog {
        token: u64,
        profile: ProfileId,
        outcome: ExtensionManagementCatalogOutcome,
    },
    StoreExtensionUpdateFinished {
        token: u64,
        result: StoreExtensionUpdateResult,
    },
    /// Internal source-free catalog activation handoff from the product
    /// distribution worker. Public operation dispatch never admits it.
    ActivateAcquiredExtensionCatalog(AcquiredExtensionCatalogSubmission),
    /// Latest redacted state from the explicitly constructed product
    /// distribution worker. This is replaceable observation, not authority.
    ExtensionDistributionStatusChanged(ExtensionDistributionStatus),
    SearchSupplementaryFinished {
        context: Box<zephium_ipc::SearchContext>,
        query: String,
    },
    SearchAdditional {
        context: Box<zephium_ipc::SearchContext>,
        query: String,
        results: Vec<zephium_ipc::SearchResult>,
    },
    Search(String),
    SearchScoped {
        query: String,
        context: Box<zephium_ipc::SearchContext>,
    },
    CancelSearch {
        session_id: String,
    },
    RunSearchAction {
        context: Box<zephium_ipc::SearchContext>,
        action: zephium_ipc::SearchAction,
    },
    OpenUrl {
        input: String,
        new_tab: bool,
    },
    SetAppSetting {
        key: String,
        value: String,
    },
    /// Permanently removes one inactive named profile through the durable
    /// cross-store deletion coordinator. The profile id comes only from
    /// privileged chrome and is revalidated against authoritative state.
    DeleteProfile(ProfileId),
    /// Explicitly retries one exact failed content-policy generation.
    ///
    /// The failed generation comes from a trusted status query. Requiring it
    /// prevents a duplicated or delayed command from retrying a newer failure
    /// after state has already advanced.
    RetryContentPolicy {
        profile: ProfileId,
        failed_generation: ContentPolicyGeneration,
    },
    /// Changes only the actor-selected focused profile. The operation remains
    /// pending until both the exact durable CAS and native policy generation
    /// settle.
    SetFocusedContentBlockerEnabled(bool),
    /// Retries the focused profile's exact failed generation without exposing
    /// a profile selector to privileged IPC.
    RetryFocusedContentPolicy {
        failed_generation: ContentPolicyGeneration,
    },
    /// Requests one authenticated source-package refresh. This is a global
    /// browser maintenance operation, not a profile or page capability.
    RefreshContentBlockerSources,
    /// Trusted, bounded actor query. Raw page content has no command bridge
    /// and the desktop layer does not expose this variant over IPC.
    ContentPolicyStatus {
        profile: ProfileId,
        reply: SyncSender<ContentPolicyStatusQueryOutcome>,
    },
    /// Read-only privileged-chrome reconciliation query. The actor chooses the
    /// focused profile and assigns the projection revision; IPC callers cannot
    /// enumerate or select another profile.
    FocusedContentPolicyStatus {
        reply: SyncSender<BlockerStatusView>,
    },
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
    /// Small wake for one exact compiler result retained in the bounded
    /// profile inbox. The immutable compiled artifact never enters the actor
    /// command queue.
    BlockerReady(ProfileId),
    /// Wake for one token-tagged durable preference mutation or
    /// reconciliation result retained in the bounded blocker inbox.
    BlockerStoreReady(ProfileId),
    /// Exact bounded-backoff retry for an indeterminate durable preference
    /// reconciliation.
    BlockerPreferenceRetry {
        profile: ProfileId,
        token: u64,
    },
    /// Bounded follow-up for an exact user-requested refresh or the reserved
    /// internal activation token. Ordinary refresh scheduling remains on the
    /// low-frequency maintenance heartbeat.
    BlockerCatalogPoll {
        operation: u64,
        attempt: u8,
    },
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

type Completion<T> = Arc<Mutex<Option<Box<dyn FnOnce(T) + Send>>>>;

#[derive(Clone)]
pub struct ResourceCompletion(Completion<zephium_core::resources::ResourceReply>);
impl ResourceCompletion {
    pub fn new(done: impl FnOnce(zephium_core::resources::ResourceReply) + Send + 'static) -> Self {
        Self(Arc::new(Mutex::new(Some(Box::new(done)))))
    }
    pub fn finish(self, reply: zephium_core::resources::ResourceReply) {
        let done = self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(done) = done {
            done(reply);
        }
    }
}
impl fmt::Debug for ResourceCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ResourceCompletion")
    }
}

#[derive(Clone)]
pub struct NotesAttachment(pub zephium_core::ports::notes::SharedNotes);
impl fmt::Debug for NotesAttachment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NotesAttachment")
    }
}

#[derive(Clone)]
pub struct NoteCompletion(Completion<zephium_core::notes::NoteReply>);
impl NoteCompletion {
    pub fn new(done: impl FnOnce(zephium_core::notes::NoteReply) + Send + 'static) -> Self {
        Self(Arc::new(Mutex::new(Some(Box::new(done)))))
    }
    pub fn finish(self, reply: zephium_core::notes::NoteReply) {
        let done = self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(done) = done {
            done(reply);
        }
    }
}
impl fmt::Debug for NoteCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NoteCompletion")
    }
}

#[derive(Clone)]
pub struct HistoryCompletion(Completion<zephium_ipc::HistoryResponse>);
impl HistoryCompletion {
    pub fn new(done: impl FnOnce(zephium_ipc::HistoryResponse) + Send + 'static) -> Self {
        Self(Arc::new(Mutex::new(Some(Box::new(done)))))
    }
    pub fn finish(self, response: zephium_ipc::HistoryResponse) {
        let done = self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(done) = done {
            done(response);
        }
    }
}
impl fmt::Debug for HistoryCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HistoryCompletion")
    }
}
