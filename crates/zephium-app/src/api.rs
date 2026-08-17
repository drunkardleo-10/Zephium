//! Stable application-shell protocol exposed to the desktop composition root.

use std::fmt;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};

use zephium_core::blocker::{ContentPolicyGeneration, ProfileContentPolicyStatus};
use zephium_core::extensions::{
    ExtensionActionRevision, ExtensionInstallCatalogRevision, ExtensionInstallRevision,
    ExtensionPopupAnchor, ExtensionRuntimeInstance,
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
    ExtensionInstallOutcome, ExtensionManagementCatalogOutcome, ExtensionManagementSettlement,
    ExtensionRepositoryMaintenanceOutcome, ExtensionRuntimeGrantOutcome,
    ExtensionRuntimeGrantRequestId, ExtensionSetEnabledOutcome, ExtensionUninstallOutcome,
};
use zephium_core::ports::store::Store;
use zephium_core::ports::store::{
    PagePermissionCatalogLoadOutcome, PagePermissionCatalogMutationOutcome,
};
use zephium_core::split::Axis;
use zephium_ipc::{BlockerStatusView, Projection, TabView};

use crate::store_reads::StoreReadResult;

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
/// Unique application-owned lifecycle authority for the extension service.
///
/// Unlike the cloneable observation handles exposed by the concrete service,
/// this owner moves onto the shell actor and is consumed exactly once during
/// ordered process shutdown.
pub type ExtensionLifecycle = Box<dyn ExtensionServiceLifecycle>;
/// Maximum user-owned extension management operations awaiting serialized
/// service settlement. This also reserves critical Shell mailbox capacity.
pub const MAX_PENDING_EXTENSION_MANAGEMENT_OPERATIONS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionManagementCompletion {
    Install(ExtensionManagementSettlement<ExtensionInstallOutcome>),
    SetEnabled(ExtensionManagementSettlement<ExtensionSetEnabledOutcome>),
    Uninstall(ExtensionManagementSettlement<ExtensionUninstallOutcome>),
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
/// A retryable failure happens before the unique extension-service owner or
/// native engine is torn down and leaves the actor live. `Unclean` is
/// terminal: either the actor exited without completing the barrier or one of
/// extension, Store, blocker, or native cleanup was not proven, so the process
/// must exit unsuccessfully.
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
    /// Removes one exact installed extension from the focused profile after
    /// the extension service proves regular/private native absence.
    UninstallFocusedExtension {
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
    /// Internal exactly-once callback from one bounded repository-maintenance
    /// turn. It is never accepted through public operation dispatch.
    ExtensionRepositoryMaintenanceSettled(ExtensionRepositoryMaintenanceOutcome),
    /// Internal move-only package handoff from the product distribution
    /// worker. Public operation dispatch never admits this command.
    ProvisionAcquiredExtensionPackage(AcquiredExtensionPackageSubmission),
    /// Internal source-free catalog activation handoff from the product
    /// distribution worker. Public operation dispatch never admits it.
    ActivateAcquiredExtensionCatalog(AcquiredExtensionCatalogSubmission),
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
