//! Typed contract between the Rust core and the frame. Pure data, no tauri;
//! the desktop crate maps `Projection` onto typed events and exports the TS
//! bindings. ULIDs cross the boundary as strings.

use serde::{Deserialize, Serialize};
use specta::Type;

pub mod work;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TabView {
    pub id: String,
    /// Process-local monotonically increasing projection revision, encoded as
    /// fixed-width hexadecimal so JavaScript can compare it without losing
    /// integer precision. Privileged chrome rejects an older per-tab delta
    /// after a newer presentation barrier has applied.
    pub projection_revision: String,
    pub title: String,
    pub url: Option<String>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub favicon: Option<String>,
}

/// Non-authorizing identity for one live extension runtime. Privileged chrome
/// may echo this value only as part of an action gesture; the Shell rejoins it
/// to the focused profile, active tab, browser-surface generation, and current
/// action revision before any native work is admitted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionActionRuntimeView {
    pub install_id: String,
    /// Process-local nonzero generation, encoded as fixed-width hexadecimal
    /// so JavaScript never rounds a Rust `u64`.
    pub generation: String,
}

/// One effective toolbar action for the focused profile's active logical tab.
/// Labels and badges are bounded before this projection is constructed. The
/// optional icon is the canonical base64 encoding of exactly 32x32 RGBA bytes;
/// privileged chrome performs no extension-controlled image decoding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionActionView {
    pub runtime: ExtensionActionRuntimeView,
    pub revision: String,
    pub label: String,
    pub badge: String,
    pub icon_rgba_base64: Option<String>,
    pub enabled: bool,
    pub presents_popup: bool,
    pub unread_badge: bool,
}

/// Exact replacement action cohort. An empty `actions` collection removes all
/// previously visible actions for this profile/tab. The frame additionally
/// joins `profile_id` and `tab_id` to its current Items projection, so event
/// reordering cannot expose a stale action after focus changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionActionsView {
    pub projection_revision: String,
    pub profile_id: String,
    pub tab_id: Option<String>,
    pub actions: Vec<ExtensionActionView>,
}

/// Closed, sanitized reason why a trusted toolbar gesture could not complete.
/// Native strings, extension content, URLs, and runtime identities are never
/// forwarded through this transient user-notice channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionActionFailure {
    InvalidRequest,
    RuntimeUnavailable,
    RuntimeSuperseded,
    TabUnavailable,
    TabDiscarded,
    ActionUnavailable,
    ActionDisabled,
    CapacityExceeded,
    PopupUnavailable,
    PopupCapacityExceeded,
    NativeAdmissionFailed,
    ShuttingDown,
    UnsupportedPlatform,
}

/// Actor-ordered, context-bound transient failure notice. The revision lets
/// privileged chrome discard a delayed eval and present each current failure
/// at most once; the profile/tab join prevents a late native refusal from
/// appearing after the user has switched context.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionActionFailedView {
    pub projection_revision: String,
    pub profile_id: String,
    pub tab_id: String,
    pub reason: ExtensionActionFailure,
}

/// One actor-ordered request for privileged chrome to invoke the exact
/// browser-owned action button already projected for the focused tab. The
/// frame contributes only that button's current geometry; every authorizing
/// identity is revalidated by Shell and the native host on the normal action
/// path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionActionShortcutView {
    pub projection_revision: String,
    pub profile_id: String,
    pub tab_id: String,
    pub runtime: ExtensionActionRuntimeView,
    pub action_revision: String,
}

/// Settlement of the focused profile's lazy installed-extension projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionManagementPhase {
    Loading,
    Ready,
    NotConfigured,
    CatalogNotSynchronized,
    UpdateConsentRequired,
    Unavailable,
    Rejected,
    FailedClosed,
}

/// Process-immutable product availability for the extension-management UX.
/// This is a non-authorizing presentation fact. It carries no catalog,
/// package, profile, repository, or runtime identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionManagementAvailabilityView {
    Configured,
    NotConfigured,
    Unavailable,
}

/// Actor-ordered projection of extension-management product availability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionManagementAvailabilityChangedView {
    pub projection_revision: String,
    pub availability: ExtensionManagementAvailabilityView,
}

/// Process-local regular-runtime state for one installed extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionManagementRuntimeView {
    Disabled,
    PendingActivation,
    ProfilePaused,
    Active,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionProfilePolicyView {
    pub revision: String,
    pub paused: bool,
    pub denied_site_count: u16,
    pub current_site_available: bool,
    pub current_site_denied: bool,
}

/// Reviewed compatibility of the exact authenticated manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionManagementCompatibilityView {
    Compatible,
    Degraded,
}

/// Browser-authenticated acquisition/support lane for extension management UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionManagementSourceView {
    ZephiumVerified,
    ExternalCompatibility,
    DeveloperLocal,
}

/// Browser-authenticated, inert upstream identity for extension management UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionManagementProvenanceView {
    pub source_url: String,
    pub upstream_version: String,
    pub license_expression: String,
    pub attribution: String,
}

/// One browser-owned explanation for a reviewed platform degradation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtensionManagementLimitationView {
    ApiPermission { name: String },
    HostAccess,
    Background,
    Action,
    Offscreen,
    NativeMessaging,
    BrowserOverride,
    ExtensionPagesCsp,
    Sandbox,
    ContentScripts,
    WebAccessibleResources,
    MinimumBrowserVersion,
    Commands,
    SidePanel,
    ManagedStorage,
    OptionsPage,
    DeclarativeNetRequest,
}

/// Non-authorizing summary of the atomic grant row joined to an install.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionManagementGrantView {
    pub initialized: bool,
    pub revision: Option<String>,
    pub api_permissions: Vec<String>,
    pub host_permissions: Vec<String>,
    pub file_access: bool,
    pub private_access: bool,
}

/// One authenticated installed extension in browser-owned management UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionManagementEntryView {
    pub install_id: String,
    pub install_revision: String,
    pub name: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub version: String,
    pub has_options_page: bool,
    pub source: ExtensionManagementSourceView,
    /// Decimal Unix seconds of the authenticated Verified catalog release.
    pub verified_catalog_unix: Option<String>,
    pub provenance: Option<ExtensionManagementProvenanceView>,
    pub runtime: ExtensionManagementRuntimeView,
    /// Present only when `runtime` is `active`.
    pub runtime_generation: Option<String>,
    pub grants: ExtensionManagementGrantView,
    /// Canonically ordered optional API declarations. Mutations return only
    /// the array index plus the exact grant revision.
    pub optional_api: Vec<String>,
    /// Canonically ordered optional host declarations.
    pub optional_hosts: Vec<String>,
    pub compatibility: ExtensionManagementCompatibilityView,
    pub limitations: Vec<ExtensionManagementLimitationView>,
}

/// One authenticated package offered by Zephium's current curated catalog.
/// `candidate_index` is an opaque, short-lived selector into the exact
/// revisioned catalog retained by Shell. Privileged chrome may only echo it;
/// it conveys no package, repository, profile, or permission authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionInstallCandidateView {
    pub candidate_index: u8,
    pub name: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub version: String,
    pub source: ExtensionManagementSourceView,
    /// Decimal Unix seconds of the authenticated Verified catalog release.
    pub verified_catalog_unix: Option<String>,
    pub provenance: Option<ExtensionManagementProvenanceView>,
    pub required_api: Vec<String>,
    pub required_hosts: Vec<String>,
    /// Canonically ordered optional API grants. The frontend returns only
    /// selected array indexes; Shell rejoins them to its retained candidate.
    pub optional_api: Vec<String>,
    /// Canonically ordered optional host grants.
    pub optional_hosts: Vec<String>,
    pub supports_file_access: bool,
    pub file_access_available: bool,
    pub private_access_available: bool,
    pub compatibility: ExtensionManagementCompatibilityView,
    pub limitations: Vec<ExtensionManagementLimitationView>,
}

/// One exact authenticated replacement awaiting changed-authority or newly
/// degraded compatibility review. `review_id` is an opaque,
/// subscription-local echo token; no package identity or permission name is
/// accepted back from privileged chrome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionUpdateConsentView {
    pub review_id: String,
    pub name: String,
    pub version: String,
    pub source: ExtensionManagementSourceView,
    pub verified_catalog_unix: Option<String>,
    pub provenance: Option<ExtensionManagementProvenanceView>,
    pub added_required_api: Vec<String>,
    pub added_required_hosts: Vec<String>,
    pub compatibility: ExtensionManagementCompatibilityView,
    pub limitations: Vec<ExtensionManagementLimitationView>,
}

/// Exact replacement management cohort for the focused profile.
/// Loading and failure phases always carry no catalog revision or rows, so a
/// delayed failure cannot leave stale selectors actionable in privileged UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionManagementView {
    pub projection_revision: String,
    pub profile_id: String,
    pub phase: ExtensionManagementPhase,
    pub catalog_revision: Option<String>,
    /// Present only while `phase` is `ready`.
    pub profile_policy: Option<ExtensionProfilePolicyView>,
    pub entries: Vec<ExtensionManagementEntryView>,
    pub candidates: Vec<ExtensionInstallCandidateView>,
    /// Present only while `phase` is `update_consent_required`.
    pub pending_update: Option<ExtensionUpdateConsentView>,
}

/// Stable distribution failure stage exposed only to privileged chrome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtensionDistributionFailureStageView {
    Catalog,
    PackageFetch { index: u8 },
    PackageProvision { index: u8 },
    CatalogActivation,
}

/// Redacted product-distribution failure reason. Network and native error
/// strings never cross the privileged IPC boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionDistributionFailureReasonView {
    Acquisition,
    Busy,
    ServiceUnavailable,
    ServiceRejected,
    ServiceFailedClosed,
    SettlementTimedOut,
    SettlementLost,
    SubmissionPanicked,
    OutcomeUnresolved,
    ActivationRejected,
    Accounting,
}

/// Exact replacement state for the dormant product distribution worker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ExtensionDistributionStateView {
    Idle,
    Synchronizing,
    Ready {
        package_count: u8,
        materialized_packages: u8,
        reused_packages: u8,
        exact_retries: u8,
        newly_activated: bool,
    },
    Failed {
        stage: ExtensionDistributionFailureStageView,
        reason: ExtensionDistributionFailureReasonView,
    },
    Quarantined {
        stage: ExtensionDistributionFailureStageView,
        reason: ExtensionDistributionFailureReasonView,
    },
    Shutdown,
}

/// Shell-revisioned product-distribution status for privileged extension UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionDistributionView {
    pub projection_revision: String,
    pub state: ExtensionDistributionStateView,
}

/// One browser-owned optional-permission consent surface. Every identity is a
/// short-lived echo token only; the Shell rejoins it to its retained native
/// request before a user response can reach the serialized grant service.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionRuntimeGrantPromptEntryView {
    pub profile_id: String,
    pub install_id: String,
    pub runtime_generation: String,
    pub request_id: String,
    pub extension_name: String,
    pub api_permissions: Vec<String>,
    pub host_permissions: Vec<String>,
    pub private_context: bool,
    /// True after an Allow gesture while the durable grant transaction is in
    /// flight. Chrome must disable both response buttons until replacement.
    pub processing: bool,
}

/// Exact replacement for the process-wide permission prompt surface. `None`
/// closes any prior prompt; Shell serializes the bounded native cohort so the
/// frame never chooses request ordering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExtensionRuntimeGrantPromptView {
    pub projection_revision: String,
    pub prompt: Option<ExtensionRuntimeGrantPromptEntryView>,
}

/// Closed page capability names rendered by browser-owned chrome. Native
/// permission strings and page-controlled labels never cross this boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PagePermissionKindView {
    Camera,
    Microphone,
}

/// One exact, foreground page-permission request. Every identity is an opaque
/// stale fence: privileged chrome may only echo it back to the Shell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PagePermissionPromptEntryView {
    pub profile_id: String,
    pub item_id: String,
    pub request_id: String,
    pub origin: String,
    pub kinds: Vec<PagePermissionKindView>,
    /// False for ephemeral profiles; chrome must not offer durable policy.
    pub rememberable: bool,
    /// True while an exact durable remember-decision transaction is pending.
    pub processing: bool,
}

/// Exact replacement for the one process-wide page permission surface.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PagePermissionPromptView {
    pub projection_revision: String,
    pub prompt: Option<PagePermissionPromptEntryView>,
}

/// The one retained split group owned by the focused window. Members are
/// normalized references into [`ItemsState::tabs`] in native pane traversal
/// order; geometry and mutable divider ratios remain native-only authority.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SplitGroupView {
    pub members: Vec<String>,
}

/// Focused profile metadata. Profile isolation and lifecycle remain native
/// authority; this view is display-only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProfileView {
    pub id: String,
    pub name: String,
    pub kind: ProfileKindView,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKindView {
    Default,
    Named,
    Incognito,
}

/// One ordered space owned by the focused profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SpaceView {
    pub id: String,
    pub name: String,
}

/// Stable display section for a sidebar node. Children inherit their
/// authoritative placement from the native item aggregate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SidebarSectionView {
    Favorites,
    Pinned,
    Today,
}

/// A folder carries bounded display metadata; a tab is a normalized reference
/// into [`ItemsState::tabs`]. The node id and tab id intentionally match, but
/// the explicit reference keeps consumers from inferring that invariant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SidebarNodeKindView {
    Folder { name: String },
    Tab { tab_id: String },
}

/// One pre-order entry in the focused sidebar tree. Parents always precede
/// descendants, sibling order is native aggregate order, and `parent_id` is
/// `None` only for a section root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SidebarNodeView {
    pub id: String,
    pub parent_id: Option<String>,
    pub section: SidebarSectionView,
    pub kind: SidebarNodeKindView,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ItemsState {
    pub projection_revision: String,
    /// `None` is reserved for the frame's cold pre-bootstrap state. Native
    /// snapshots are emitted only with an exact focused profile and space.
    pub profile: Option<ProfileView>,
    pub spaces: Vec<SpaceView>,
    pub active_space_id: Option<String>,
    pub nodes: Vec<SidebarNodeView>,
    /// Every tab referenced by `nodes`, in the same pre-order traversal.
    pub tabs: Vec<TabView>,
    pub active: Option<String>,
    pub split_group: Option<SplitGroupView>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type")]
pub enum SearchAction {
    ActivateTab { id: String },
    OpenUrl { url: String },
    RunCommand { id: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Notes,
    Tasks,
    Ai,
    History,
    Downloads,
    Time,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SearchContext {
    pub window_id: String,
    pub session_id: String,
    pub request_id: String,
    pub profile_id: String,
    pub space_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PanelRoute {
    Search,
    Tool { tool: ToolKind },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PanelIntent {
    Search,
    Back,
    Dismiss,
    Tool { tool: ToolKind },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PanelState {
    pub window_id: Option<String>,
    pub revision: String,
    pub session_id: String,
    pub visible: bool,
    pub route: PanelRoute,
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub space_id: Option<String>,
    pub error: bool,
    pub corner_radius: u16,
    pub position_restorable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SearchResult {
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub favicon: Option<String>,
    pub action: SearchAction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SearchResults {
    pub context: Option<SearchContext>,
    pub query: String,
    pub results: Vec<SearchResult>,
}

/// Split divider hit-strip in window logical coordinates; the chrome renders
/// these as drag targets on platforms without native stage dividers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct DividerView {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub vertical: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct LayoutState {
    pub dividers: Vec<DividerView>,
}

/// Immediate result returned by a privileged IPC command. `accepted` with an
/// `operation_id` means the mutation was successfully and non-evictably
/// admitted to the shell's process-local ordered FIFO; it does not claim that
/// later native/store work succeeded or survive a process restart. `accepted`
/// without an id is reserved for a fully applied, privileged-UI-only action
/// such as toggling the launcher.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OperationAdmission {
    pub operation_id: Option<String>,
    pub accepted: bool,
}

/// The bounded terminal classification the actor can establish while
/// processing an admitted operation. `Deferred` means native work was queued,
/// an exact discard acknowledgement is still required, or a durable write
/// became indeterminate and entered explicit reconciliation; it never means a
/// page load, renderer callback, or unknown store transaction succeeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OperationOutcome {
    Applied,
    NoOp,
    Rejected,
    NativeAdmissionFailed,
    Deferred,
}

/// Stable, non-page-derived detail for an operation outcome. Keeping this an
/// enum prevents native errors, URLs, or attacker-controlled strings from
/// becoming an unbounded privileged IPC/logging surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OperationReason {
    MutationApplied,
    StateUnchanged,
    InvalidScope,
    NoFocusedWindow,
    ItemLimitReached,
    InvalidInput,
    HistoryUnavailable,
    LayoutUnavailable,
    UnsupportedCommand,
    NativeDispatchRejected,
    NativeWorkPending,
    DiscardCompletionPending,
    StoreWorkPending,
    StoreAdmissionRejected,
    StoreConflict,
    StoreOutcomeUnknown,
    StoreReconciliationFailed,
    ExtensionEnablementPending,
    ExtensionActivationPending,
    ExtensionRestartRequired,
    ContentPolicyApplyFailed,
    ContentPolicySourceUnavailable,
    ContentPolicySourceRefreshPending,
    ContentPolicySourceRefreshFailed,
    ContentPolicySourcesRefreshed,
    ProfileDeletionPolicyRejected,
    ProfileDeletionInProgress,
    ProfileDeletionCompleted,
}

/// The shell has processed an admitted operation in actor order. Consumers
/// reconcile logical effects from authoritative projections. A deferred
/// native navigation still resolves independently through engine events.
/// Long-running profile deletion retains its id internally and emits this
/// disposition exactly once, only after definitive rejection or both durable
/// deletion phases complete; blocker preference mutations likewise retain
/// their id through CAS and exact native settlement. Retry/reconciliation
/// state is never mislabeled as successfully applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OperationDisposition {
    pub operation_id: String,
    pub outcome: OperationOutcome,
    pub reason: OperationReason,
}

/// Process-local reconciliation state for an admitted mutation. Pending and
/// processed entries are retained in a bounded fail-closed desktop ledger;
/// processed entries remain queryable until privileged chrome acknowledges
/// them. `Unknown` means the id was never admitted in this process, was already
/// acknowledged, or belongs to a previous process lifetime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum OperationStatus {
    Unknown,
    Pending,
    Processed { disposition: OperationDisposition },
}

/// Process-local browser-runtime status. Once `restart_required` becomes true
/// it remains true until the whole application exits; recoverable bounded
/// user-content degradation is projected independently in the same snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RuntimeStatus {
    pub restart_required: bool,
    /// Bounded fail-closed aggregate of ownership scopes whose latest native
    /// user-content observation was not exactly applied. An impossible
    /// over-capacity observation contributes at most one sentinel. No script,
    /// extension, profile, or native failure identity crosses this privileged
    /// projection.
    pub user_content_degraded_scope_count: u16,
    /// Canonically ordered closed-vocabulary set. Rust emits at most five
    /// entries and privileged chrome must replace, never append, projections.
    pub security_advisories: Vec<RuntimeSecurityAdvisory>,
}

/// Non-fatal, process-local classification produced before native WebView
/// construction. Hard admission failures never reach privileged chrome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSecurityAdvisoryKind {
    ReviewOverdue,
    UpdateRecommended,
    UnreviewedRuntime,
}

/// Fixed destination of the recommended maintenance action. No page or
/// network response can select this value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSecurityUpdateTarget {
    Zephium,
    OperatingSystem,
    BrowserRuntime,
}

/// Sanitized advisory delivered only to privileged main chrome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RuntimeSecurityAdvisory {
    pub kind: RuntimeSecurityAdvisoryKind,
    pub update_target: RuntimeSecurityUpdateTarget,
}

/// Effective protection for the focused profile's exact native policy.
/// This is derived in Rust from both desired and retained state. Privileged
/// chrome must not infer protection from a pending preference and accidentally
/// present a retained allow-all generation as active blocking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockerProtection {
    Disabled,
    Pending,
    Active,
    Degraded,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockerPhase {
    Unavailable,
    Uninitialized,
    Compiling,
    Installing,
    Ready,
    Failed,
    Retired,
}

/// Authority of the focused profile's durable blocker preference.
/// `Reconciling` and `Unavailable` are intentionally distinct from native
/// policy state: the browser may still know which generation is installed
/// while refusing to guess what durable preference should replace it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockerPreferenceState {
    Authoritative,
    Updating,
    Reconciling,
    Unavailable,
}

/// Sanitized state of the authenticated filter-package supply chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockerSourcePhase {
    NotConfigured,
    DurableActivationUnsupported,
    StorageUnavailable,
    ClockUnsafe,
    Idle,
    Fresh,
    Stale,
    Refreshing,
    Failed,
    Shutdown,
}

/// Authority which admitted the displayed filter package.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockerSourceProvenance {
    ReleaseBundle,
    TufRepository,
}

/// Stable package-refresh failure category. Endpoint, parser, and native
/// strings are intentionally never forwarded to privileged JavaScript.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockerSourceFailure {
    Transport,
    Metadata,
    Clock,
    Manifest,
    Target,
    License,
    Rollback,
    Storage,
    Catalog,
    Internal,
}

/// Stable diagnostics classification. Native/parser text and filter content
/// never cross the privileged IPC boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockerFailure {
    GenerationExhausted,
    CompilerDispatchRejected,
    CompilerUnavailable,
    CompileSourceUnavailable,
    CompileInvalidSource,
    CompileResourceLimit,
    CompileInternal,
    CompiledArtifactMismatch,
    NativeDispatchRejected,
    NativeUnsupported,
    NativeUnsupportedArtifact,
    NativeInvalidArtifact,
    NativeCompilation,
    NativeInstallation,
    NativeCleanup,
    NativeSuperseded,
    ContradictoryNativeSettlement,
}

/// Exact coverage of the generation which native code proved applied.
/// Counts are bounded far below JavaScript's exact-integer ceiling by the
/// blocker compiler's hard rule limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BlockerRuleCoverage {
    pub source_rules: u32,
    pub accepted_rules: u32,
    pub rejected_rules: u32,
    pub platform_omitted_rules: u32,
    pub platform_approximated_rules: u32,
    pub platform_resource_approximated_rules: u32,
    pub platform_source_kind_approximated_rules: u32,
    pub platform_attribution_approximated_rules: u32,
    pub blocking_rule_entries: u32,
}

/// Volatile, process-local health counters for the exact applied runtime
/// matcher. Decimal strings preserve the full saturating `u64` range in
/// JavaScript. These counters are never persisted and contain no request,
/// origin, URL, profile, or rule identity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BlockerRuntimeDiagnostics {
    pub total_decisions: String,
    pub candidate_budget_exhausted: String,
    pub matcher_unavailable: String,
    pub matcher_unprepared: String,
    pub attribution_unavailable: String,
    pub evaluation_errors: String,
}

/// Exact authenticated identities for source-package transition diagnostics.
/// This is boxed in [`BlockerStatusView`] so infrequent debug strings do not
/// inflate every application projection on the shell actor's hot path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BlockerSourceIdentities {
    pub package_manifest_sha256: Option<String>,
    pub candidate_revision: Option<String>,
    pub candidate_manifest_sha256: Option<String>,
    pub installed_manifest_sha256: Option<String>,
}

/// Read-only, focused-profile diagnostics delivered only to privileged main
/// chrome. It deliberately contains no profile selector, URL, origin, request
/// metadata, native error string, or filter-list text. Runtime health is
/// represented only by volatile aggregate counters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BlockerStatusView {
    pub projection_revision: String,
    pub protection: BlockerProtection,
    pub phase: BlockerPhase,
    pub preference: BlockerPreferenceState,
    pub config_revision: Option<String>,
    pub desired_enabled: Option<bool>,
    pub applied_enabled: Option<bool>,
    pub desired_generation: Option<String>,
    pub retained_generation: Option<String>,
    pub failure: Option<BlockerFailure>,
    pub retryable: bool,
    pub retries_remaining: u8,
    /// Boxed with the other diagnostic-only payloads so ordinary projection
    /// queue entries do not carry the full coverage report inline.
    pub applied_coverage: Option<Box<BlockerRuleCoverage>>,
    /// Boxed because the six decimal counters are diagnostic-only and should
    /// not inflate every projection enum value on the actor/UI hot path.
    pub runtime_diagnostics: Option<Box<BlockerRuntimeDiagnostics>>,
    pub source_phase: BlockerSourcePhase,
    pub source_failure: Option<BlockerSourceFailure>,
    pub source_package_revision: Option<String>,
    pub source_installed_revision: Option<String>,
    pub source_package_provenance: Option<BlockerSourceProvenance>,
    pub source_installed_provenance: Option<BlockerSourceProvenance>,
    pub source_identities: Option<Box<BlockerSourceIdentities>>,
    pub source_package_created_unix: Option<String>,
    pub source_package_expires_unix: Option<String>,
    pub source_package_stale: Option<bool>,
    /// Advisory source update cadence. This never downgrades a healthy
    /// release-bundled policy.
    pub source_refresh_due: bool,
    pub source_count: Option<u32>,
    pub source_bytes: Option<u32>,
    pub source_activation_pending: bool,
    pub source_material_repair_pending: bool,
    pub source_material_repair_retry_pending: bool,
    pub source_repair_retry_pending: bool,
    pub source_last_refresh_attempt_unix: Option<String>,
    pub source_refresh_operation: Option<String>,
    /// Authoritative source-policy capability for the focused profile.
    pub can_enable: bool,
    /// Authoritative refresh admission capability for the active supply mode.
    pub can_refresh_sources: bool,
}

impl BlockerStatusView {
    /// Static reconciliation result used only when no actor-owned revision can
    /// be obtained. Revision zero cannot overwrite a real actor projection.
    pub fn unavailable() -> Self {
        Self {
            projection_revision: "00000000000000000000000000000000".into(),
            protection: BlockerProtection::Unavailable,
            phase: BlockerPhase::Unavailable,
            preference: BlockerPreferenceState::Unavailable,
            config_revision: None,
            desired_enabled: None,
            applied_enabled: None,
            desired_generation: None,
            retained_generation: None,
            failure: None,
            retryable: false,
            retries_remaining: 0,
            applied_coverage: None,
            runtime_diagnostics: None,
            source_phase: BlockerSourcePhase::NotConfigured,
            source_failure: None,
            source_package_revision: None,
            source_installed_revision: None,
            source_package_provenance: None,
            source_installed_provenance: None,
            source_identities: None,
            source_package_created_unix: None,
            source_package_expires_unix: None,
            source_package_stale: None,
            source_refresh_due: false,
            source_count: None,
            source_bytes: None,
            source_activation_pending: false,
            source_material_repair_pending: false,
            source_material_repair_retry_pending: false,
            source_repair_retry_pending: false,
            source_last_refresh_attempt_unix: None,
            source_refresh_operation: None,
            can_enable: false,
            can_refresh_sources: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PanelOwner {
    pub window_id: String,
    pub profile_id: String,
    pub profile_name: String,
    pub space_id: String,
}

/// Snapshots for structural changes, single-row deltas for per-tab churn.
#[derive(Clone, Debug)]
pub enum Projection {
    WorkEnvironmentChanged(work::WorkEnvironmentChangedV1),
    WorkChanged(work::WorkChangedV1),
    PanelOwner(PanelOwner),
    Items(ItemsState),
    Tab(TabView),
    ExtensionActions(ExtensionActionsView),
    ExtensionActionFailed(ExtensionActionFailedView),
    ExtensionActionShortcut(ExtensionActionShortcutView),
    ExtensionManagementAvailability(ExtensionManagementAvailabilityChangedView),
    ExtensionManagement(ExtensionManagementView),
    ExtensionDistribution(ExtensionDistributionView),
    ExtensionRuntimeGrantPrompt(ExtensionRuntimeGrantPromptView),
    PagePermissionPrompt(PagePermissionPromptView),
    UiCommand(String),
    Search(SearchResults),
    Layout(LayoutState),
    RuntimeStatus(RuntimeStatus),
    BlockerStatus(BlockerStatusView),
    OperationProcessed(OperationDisposition),
}

#[cfg(test)]
mod blocker_status_tests {
    use super::*;

    #[test]
    fn unavailable_status_is_bounded_and_cannot_supersede_actor_state() {
        let status = BlockerStatusView::unavailable();
        assert_eq!(
            status.projection_revision,
            "00000000000000000000000000000000"
        );
        assert_eq!(status.protection, BlockerProtection::Unavailable);
        assert_eq!(status.phase, BlockerPhase::Unavailable);
        assert_eq!(status.preference, BlockerPreferenceState::Unavailable);
        assert!(status.config_revision.is_none());
        assert!(status.desired_generation.is_none());
        assert!(status.retained_generation.is_none());
        assert!(status.failure.is_none());
        assert!(status.applied_coverage.is_none());
        assert_eq!(status.source_phase, BlockerSourcePhase::NotConfigured);
        assert!(status.source_failure.is_none());
        assert!(status.source_package_revision.is_none());
        assert!(status.source_installed_revision.is_none());
        assert!(status.source_identities.is_none());
        assert!(!status.source_activation_pending);
        assert!(!status.source_material_repair_pending);
        assert!(!status.source_material_repair_retry_pending);
        assert!(!status.source_repair_retry_pending);
        assert!(!status.retryable);
        assert_eq!(status.retries_remaining, 0);
    }
}

/// Shared Rust-owned Notes/Tasks wire model.
pub use zephium_core::resources::{ResourceCall, ResourceReply, ResourceResponse};
