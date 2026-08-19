use std::time::Instant;

use crate::extensions::{
    ExtensionCatalogSetDigest, ExtensionInstallCatalogRevision, ExtensionInstallRevision,
    ExtensionNativeOwnershipKey, ExtensionPackageIdentity, ExtensionRuntimeGeneration,
    MAX_EXTENSION_API_PERMISSIONS, MAX_EXTENSION_HOST_PERMISSION_PATTERNS,
};
use crate::ids::{ExtensionInstallId, ProfileId};

#[path = "extensions/distribution.rs"]
mod distribution;
#[path = "extensions/management.rs"]
mod management;
#[path = "extensions/provisioning.rs"]
mod provisioning;
#[path = "extensions/runtime_grants.rs"]
mod runtime_grants;

pub use distribution::{
    ExtensionDistributionCompletionStatus, ExtensionDistributionFailureReason,
    ExtensionDistributionFailureStage, ExtensionDistributionState, ExtensionDistributionStatus,
};
pub use management::{
    ExtensionInstallCandidateEntry, ExtensionManagementCatalog, ExtensionManagementCompatibility,
    ExtensionManagementEntry, ExtensionManagementGrantState, ExtensionManagementLimitation,
    ExtensionManagementProjectionError, ExtensionManagementProvenance,
    ExtensionManagementRuntimeState, ExtensionManagementSource,
    MAX_EXTENSION_MANAGEMENT_CATALOG_RETAINED_BYTES, MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES,
    MAX_EXTENSION_MANAGEMENT_LICENSE_EXPRESSION_BYTES, MAX_EXTENSION_MANAGEMENT_LIMITATIONS,
    MAX_EXTENSION_MANAGEMENT_SOURCE_URL_BYTES, MAX_EXTENSION_MANAGEMENT_UPSTREAM_VERSION_BYTES,
};
pub use provisioning::{
    acquired_runtime_selections_are_canonical, ExtensionAcquiredCatalogActivationCallback,
    ExtensionAcquiredCatalogActivationOutcome, ExtensionAcquiredCatalogActivationRequest,
    ExtensionAcquiredPackageProvisioningCallback, ExtensionAcquiredPackageProvisioningOutcome,
    ExtensionAcquiredPackageProvisioningRequest, ExtensionAcquiredProvisioningRequestError,
    ExtensionAcquiredRuntimeProfile, ExtensionAcquiredRuntimeSelection,
    MAX_ACQUIRED_CATALOG_SELECTIONS, MAX_EXTENSION_ACQUIRED_CATALOG_BYTES,
    MAX_EXTENSION_ACQUIRED_CRX_BYTES, MAX_EXTENSION_ACQUIRED_LEGAL_NOTICE_BYTES,
    MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES,
};
pub use runtime_grants::{
    ExtensionRuntimeGrantOutcome, ExtensionRuntimeGrantPrompt, ExtensionRuntimeGrantPromptError,
    ExtensionRuntimeGrantPromptSettlement, ExtensionRuntimeGrantRequest,
    ExtensionRuntimeGrantRequestError, ExtensionRuntimeGrantRequestId,
    ExtensionRuntimeGrantRuntimeState, MAX_EXTENSION_RUNTIME_GRANT_REQUEST_RETAINED_BYTES,
};

/// The extension runtime pool has three background slots, so startup can
/// expose at most three distinct profiles with executable runtime authority.
/// Keep this projection fixed-size and allocation-free.
pub const MAX_EXTENSION_ACTIVE_PROFILES: usize = 3;

/// At most one user-visible optional-grant prompt may be outstanding for each
/// admitted background runtime. This fixed ceiling bounds native completion
/// ownership independently from ordinary tab/browser request traffic.
pub const MAX_PENDING_EXTENSION_RUNTIME_GRANT_REQUESTS: usize = MAX_EXTENSION_ACTIVE_PROFILES;

/// Exact profiles whose extension runtimes were active when startup settled.
///
/// This value is routing data, not Store, package, grant, or native-owner
/// authority. It exists solely so the Shell can publish logical browser tabs
/// to the already-authorized native controller without broadcasting every
/// profile or waking the inert extension path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExtensionActiveProfiles {
    entries: [[u8; 16]; MAX_EXTENSION_ACTIVE_PROFILES],
    length: u8,
}

const _: () = assert!(std::mem::size_of::<ExtensionActiveProfiles>() <= 64);

impl ExtensionActiveProfiles {
    pub const EMPTY: Self = Self {
        entries: [[0; 16]; MAX_EXTENSION_ACTIVE_PROFILES],
        length: 0,
    };

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub const fn len(&self) -> usize {
        self.length as usize
    }

    pub fn contains(&self, profile: ProfileId) -> bool {
        self.entries[..self.len()].contains(&profile.bytes())
    }

    /// Inserts one unique profile without allocation. Re-inserting an exact
    /// identity succeeds; exceeding the shared runtime ceiling is refused.
    pub fn try_insert(&mut self, profile: ProfileId) -> bool {
        if self.contains(profile) {
            return true;
        }
        let index = self.len();
        if index == MAX_EXTENSION_ACTIVE_PROFILES {
            return false;
        }
        self.entries[index] = profile.bytes();
        self.length += 1;
        true
    }

    pub fn remove(&mut self, profile: ProfileId) -> bool {
        let length = self.len();
        let Some(index) = self.entries[..length]
            .iter()
            .position(|entry| *entry == profile.bytes())
        else {
            return false;
        };
        self.entries.copy_within(index + 1..length, index);
        self.entries[length - 1] = [0; 16];
        self.length -= 1;
        true
    }

    pub fn iter(self) -> impl Iterator<Item = ProfileId> {
        self.entries
            .into_iter()
            .take(self.len())
            .map(|bytes| ProfileId::from(u128::from_be_bytes(bytes)))
    }
}

/// Application-facing result of consuming the extension-service owner.
///
/// `Unclean` is terminal. The concrete service may retain more precise
/// diagnostics, but once its unique owner has been consumed there is no safe
/// in-process retry surface for application code.
#[must_use = "extension-service shutdown must be checked before Store teardown"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceShutdownOutcome {
    /// The service proved that its worker terminated and released its owned
    /// resources after draining every admitted command.
    Clean,
    /// The service could not prove clean worker termination and resource
    /// release before returning.
    Unclean,
}

/// Application-facing settlement of extension-service startup.
///
/// This projection intentionally carries no package, repository, grant, or
/// native-owner identity. `Ready` includes only the fixed, non-authoritative
/// profile set needed for Shell-owned browser routing. Only `Ready` authorizes
/// callers to continue into extension-sensitive bootstrap work such as
/// recovered profile deletion or raw content-view construction.
#[must_use = "extension-sensitive bootstrap requires an explicit Ready settlement"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceStartupOutcome {
    /// Repository recovery and every durable possible-owner cleanup completed.
    Ready(ExtensionActiveProfiles),
    /// One or more possible native owners remain and require cleanup.
    CleanupRequired,
    /// The exact startup attempt settled with a definite retryable condition.
    Unavailable,
    /// Startup detected corruption, invariant loss, or another closed failure.
    FailedClosed,
    /// The caller's observation deadline elapsed without terminal settlement.
    TimedOut,
    /// A retry was definitely not admitted because bounded transient capacity
    /// was unavailable. No startup frontier ran, so a later bounded retry is
    /// permitted while the same owner remains live.
    RetryableNotAdmitted,
}

/// Coarse settlement of one synchronous profile-retirement continuation.
///
/// This value is deliberately informational and publicly constructible. It is
/// not an unforgeable capability. Zephium's audited composition rule is that
/// Store authorization, native website-data erasure, and Store finalization
/// occur only inside the continuation passed to
/// [`ExtensionServiceLifecycle::with_profile_retired_until`]. The service
/// invokes that continuation only after its worker directly settles the exact
/// profile as retired. App-level behavior tests protect all three call sites;
/// the Rust type system does not independently enforce that architectural
/// rule.
#[must_use = "profile-retirement continuation settlement must be checked"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionProfileRetirementDisposition {
    /// The service proved retirement and invoked the continuation exactly
    /// once before returning.
    Continued,
    /// Retirement did not settle in this attempt. The continuation was not
    /// invoked and profile data must remain intact for a bounded retry.
    Unavailable,
    /// The service failed closed. The continuation was not invoked and the
    /// current process must not continue profile deletion.
    FailedClosed,
}

/// Application-facing settlement of one exact runtime activation request.
///
/// The ownership key is only a selector. The concrete service reconstructs
/// Store, package, grant, repository, and native authority inside its
/// serialized worker. Successful variants carry the complete non-authorizing
/// profile routing snapshot observed in the same worker turn.
#[must_use = "runtime activation settlement must be checked"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeActivationDisposition {
    Activated {
        generation: ExtensionRuntimeGeneration,
        active_profiles: ExtensionActiveProfiles,
    },
    AlreadyActive {
        generation: ExtensionRuntimeGeneration,
        active_profiles: ExtensionActiveProfiles,
    },
    Unavailable,
    Rejected,
    CapacityExceeded,
    ProfileFenced,
    FailedClosed,
}

/// Application-facing settlement of one exact runtime retirement request.
///
/// `NotPresent` is process-local coordinator evidence only. It does not prove
/// durable profile absence and must never authorize profile deletion. Both
/// successful variants carry the complete routing snapshot from the same
/// serialized worker turn, so Shell never guesses whether a sibling runtime
/// still keeps the profile active.
#[must_use = "runtime retirement settlement must be checked"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeRetirementDisposition {
    Retired {
        active_profiles: ExtensionActiveProfiles,
    },
    NotPresent {
        active_profiles: ExtensionActiveProfiles,
    },
    Unavailable,
    FailedClosed,
}

/// Stale-UI-resistant selector for one installed extension.
///
/// The revisions are compare-and-swap inputs, not Store or package authority.
/// Shell constructs this value only from its latest privileged management
/// projection; the serialized service reloads and revalidates the complete
/// catalog before changing runtime or durable state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionInstallSelector {
    profile: ProfileId,
    install: ExtensionInstallId,
    catalog_revision: ExtensionInstallCatalogRevision,
    install_revision: ExtensionInstallRevision,
}

/// Stale-resistant selector for one authenticated package offered by the
/// current curated catalog.
///
/// This value is structural routing data, not package or installation
/// authority. The extension service reauthenticates the exact current catalog,
/// package, and manifest before it can persist anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionInstallCandidateSelector {
    profile: ProfileId,
    expected_catalog_revision: ExtensionInstallCatalogRevision,
    catalog_set: ExtensionCatalogSetDigest,
    package: ExtensionPackageIdentity,
}

impl ExtensionInstallCandidateSelector {
    pub const fn new(
        profile: ProfileId,
        expected_catalog_revision: ExtensionInstallCatalogRevision,
        catalog_set: ExtensionCatalogSetDigest,
        package: ExtensionPackageIdentity,
    ) -> Self {
        Self {
            profile,
            expected_catalog_revision,
            catalog_set,
            package,
        }
    }

    pub const fn profile(&self) -> ProfileId {
        self.profile
    }

    pub const fn expected_catalog_revision(&self) -> ExtensionInstallCatalogRevision {
        self.expected_catalog_revision
    }

    pub const fn catalog_set(&self) -> ExtensionCatalogSetDigest {
        self.catalog_set
    }

    pub const fn package(&self) -> &ExtensionPackageIdentity {
        &self.package
    }
}

/// Bounded user selection for optional grants during curated installation.
///
/// Indices address the canonical optional-permission arrays in the exact
/// revisioned candidate retained by Shell. They deliberately carry no API
/// name, host pattern, package path, or profile authority. The serialized
/// extension service reauthenticates the candidate and resolves every index
/// against the same canonically sorted manifest declarations before writing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionInitialGrantSelection {
    optional_api_indices: Box<[u8]>,
    optional_host_indices: Box<[u8]>,
    file_access: bool,
    private_access: bool,
}

const _: () = assert!(MAX_EXTENSION_API_PERMISSIONS <= (u8::MAX as usize) + 1);
const _: () = assert!(MAX_EXTENSION_HOST_PERMISSION_PATTERNS <= (u8::MAX as usize) + 1);

impl ExtensionInitialGrantSelection {
    /// Canonicalizes one browser-owned selection and rejects duplicates or
    /// entries outside the candidate projection supplied by Shell.
    pub fn new(
        mut optional_api_indices: Vec<u8>,
        optional_api_count: usize,
        mut optional_host_indices: Vec<u8>,
        optional_host_count: usize,
        file_access: bool,
        private_access: bool,
    ) -> Result<Self, ExtensionInitialGrantSelectionError> {
        if optional_api_count > MAX_EXTENSION_API_PERMISSIONS
            || optional_host_count > MAX_EXTENSION_HOST_PERMISSION_PATTERNS
            || optional_api_indices.len() > optional_api_count
            || optional_host_indices.len() > optional_host_count
        {
            return Err(ExtensionInitialGrantSelectionError::OutOfBounds);
        }
        optional_api_indices.sort_unstable();
        optional_host_indices.sort_unstable();
        if optional_api_indices
            .windows(2)
            .any(|pair| pair[0] == pair[1])
            || optional_host_indices
                .windows(2)
                .any(|pair| pair[0] == pair[1])
        {
            return Err(ExtensionInitialGrantSelectionError::DuplicateIndex);
        }
        if optional_api_indices
            .last()
            .is_some_and(|index| usize::from(*index) >= optional_api_count)
            || optional_host_indices
                .last()
                .is_some_and(|index| usize::from(*index) >= optional_host_count)
        {
            return Err(ExtensionInitialGrantSelectionError::OutOfBounds);
        }
        Ok(Self {
            optional_api_indices: optional_api_indices.into_boxed_slice(),
            optional_host_indices: optional_host_indices.into_boxed_slice(),
            file_access,
            private_access,
        })
    }

    /// Canonical optional API indexes selected by the user.
    pub fn optional_api_indices(&self) -> &[u8] {
        &self.optional_api_indices
    }

    /// Canonical optional host indexes selected by the user.
    pub fn optional_host_indices(&self) -> &[u8] {
        &self.optional_host_indices
    }

    /// Whether matching `file://` host declarations may be granted.
    pub const fn file_access(&self) -> bool {
        self.file_access
    }

    /// Whether the extension may run in private browsing contexts.
    pub const fn private_access(&self) -> bool {
        self.private_access
    }
}

/// Stable refusal for a malformed browser-owned optional selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionInitialGrantSelectionError {
    /// An index or declared count exceeds its exact bounded candidate array.
    OutOfBounds,
    /// The same optional declaration was selected more than once.
    DuplicateIndex,
}

impl std::fmt::Display for ExtensionInitialGrantSelectionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfBounds => formatter.write_str("optional grant selection is out of bounds"),
            Self::DuplicateIndex => {
                formatter.write_str("optional grant selection contains a duplicate index")
            }
        }
    }
}

impl std::error::Error for ExtensionInitialGrantSelectionError {}

impl ExtensionInstallSelector {
    pub const fn new(
        profile: ProfileId,
        install: ExtensionInstallId,
        catalog_revision: ExtensionInstallCatalogRevision,
        install_revision: ExtensionInstallRevision,
    ) -> Self {
        Self {
            profile,
            install,
            catalog_revision,
            install_revision,
        }
    }

    pub const fn profile(self) -> ProfileId {
        self.profile
    }

    pub const fn install(self) -> ExtensionInstallId {
        self.install
    }

    pub const fn catalog_revision(self) -> ExtensionInstallCatalogRevision {
        self.catalog_revision
    }

    pub const fn install_revision(self) -> ExtensionInstallRevision {
        self.install_revision
    }
}

/// Why enabled user intent was durably accepted but the regular runtime is
/// not active in the current process.
///
/// This is a truthful state, not a successful enablement claim. Restart or a
/// later explicit retry re-enters the ordinary authenticated activation path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionActivationPendingReason {
    Unavailable,
    Rejected,
    CapacityExceeded,
    ProfileFenced,
    FailedClosed,
}

/// Exact result of one serialized enable/disable transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionSetEnabledOutcome {
    Enabled {
        generation: ExtensionRuntimeGeneration,
        changed: bool,
    },
    Disabled {
        changed: bool,
    },
    /// Durable desired-enabled state is true, but activation did not settle.
    PendingActivation(ExtensionActivationPendingReason),
    /// The privileged projection was stale. No durable mutation was admitted.
    Conflict,
    /// Current package/grant state cannot authorize the requested transition.
    Rejected,
    Unavailable,
    /// A Store mutation was admitted but its commit could not be observed.
    /// The process must reconcile before accepting another management write.
    OutcomeUnknown,
    FailedClosed,
}

/// Exact result of one serialized uninstall transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionUninstallOutcome {
    Uninstalled,
    Conflict,
    Rejected,
    Unavailable,
    OutcomeUnknown,
    FailedClosed,
}

/// Why an install committed atomically but could not affirm enabled intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionInstallEnablementPendingReason {
    Conflict,
    Rejected,
    Unavailable,
}

/// Truthful post-install runtime state from the same serialized worker turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionInstalledRuntimeState {
    Active(ExtensionRuntimeGeneration),
    PendingActivation(ExtensionActivationPendingReason),
    Disabled(ExtensionInstallEnablementPendingReason),
}

/// Exact result of installing one authenticated curated package.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionInstallOutcome {
    Installed {
        install: ExtensionInstallId,
        runtime: ExtensionInstalledRuntimeState,
    },
    /// The retained UI catalog or package selection is stale.
    Conflict,
    AlreadyInstalled,
    Rejected,
    Unavailable,
    OutcomeUnknown,
    FailedClosed,
}

/// Management settlement plus the complete same-turn browser-routing cohort.
///
/// `None` means the worker could not establish a trustworthy runtime
/// projection. Shell must retain its previous projection in that case.
#[must_use = "extension management settlement and routing projection must be checked"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionManagementSettlement<T> {
    outcome: T,
    active_profiles: Option<ExtensionActiveProfiles>,
}

/// Non-blocking admission result for a management transaction.
///
/// `Accepted` transfers exactly-once callback ownership to the service. Every
/// refusal leaves durable and native state untouched and drops the callback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionManagementAdmission {
    Accepted,
    Busy,
    Unavailable,
}

/// Exact result of one explicit privileged management-catalog read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionManagementCatalogOutcome {
    /// Authenticated package identity, atomic grants, and live runtime state
    /// were joined for the complete profile catalog.
    Loaded(ExtensionManagementCatalog),
    /// Product authority is configured, but no authenticated catalog
    /// generation has been activated in the local repository yet.
    CatalogNotSynchronized,
    /// A newer authenticated package adds required authority. The old package
    /// and runtime remain intact until a dedicated consent transaction exists.
    UpdateConsentRequired,
    /// The request was coherent but could not complete before its deadline or
    /// while startup/profile state temporarily refused it.
    Unavailable,
    /// Exact package metadata cannot be safely represented by this product.
    Rejected,
    /// Repository, Store, native-runtime, or protocol integrity was lost.
    FailedClosed,
}

/// Non-blocking admission result for one management-catalog read.
///
/// `Accepted` transfers exactly-once callback ownership. Refused reads have no
/// side effects and drop the callback without invoking it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionManagementCatalogAdmission {
    Accepted,
    Busy,
    Unavailable,
}

/// Non-authorizing product capability exposed solely for management UX.
///
/// This value never grants repository, package, Store, or native-runtime
/// access. It lets privileged chrome distinguish an intentionally inert build
/// from a configured worker that is temporarily unavailable, without probing
/// the worker or creating extension resources.
#[must_use = "extension management availability must be projected truthfully"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionManagementAvailability {
    /// Product authority exists and the lifecycle owns a management worker.
    Configured,
    /// This exact product build intentionally contains no extension authority.
    NotConfigured,
    /// The lifecycle cannot currently make a trustworthy capability claim.
    Unavailable,
}

/// Result of one bounded repository-maintenance turn.
///
/// This projection deliberately exposes no package identities or filesystem
/// details. Maintenance is an internal availability concern, not extension or
/// browser-chrome authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRepositoryMaintenanceOutcome {
    /// A fresh authenticated inventory contained no unreachable package data.
    NoGarbage,
    /// One bounded durable batch was collected successfully.
    Collected {
        /// A later low-frequency turn should inspect another bounded batch.
        more_garbage: bool,
    },
    /// Startup, the deadline, or a transient filesystem condition refused the
    /// turn without weakening repository integrity.
    Unavailable,
    /// Repository or service integrity could not be established.
    FailedClosed,
}

/// Non-blocking admission result for repository maintenance.
///
/// `Accepted` transfers exactly-once callback ownership. A `Pending` result
/// means an earlier maintenance turn already owns the single process-local
/// permit; callers should wait for their ordinary low-frequency wakeup rather
/// than scheduling a hot retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRepositoryMaintenanceAdmission {
    Accepted,
    Pending,
    Busy,
    Unavailable,
}

/// Exactly-once completion callback for an admitted enable/disable request.
pub type ExtensionSetEnabledCallback =
    Box<dyn FnOnce(ExtensionManagementSettlement<ExtensionSetEnabledOutcome>) + Send>;
/// Exactly-once completion callback for an admitted uninstall request.
pub type ExtensionUninstallCallback =
    Box<dyn FnOnce(ExtensionManagementSettlement<ExtensionUninstallOutcome>) + Send>;
/// Exactly-once completion callback for an admitted curated install request.
pub type ExtensionInstallCallback =
    Box<dyn FnOnce(ExtensionManagementSettlement<ExtensionInstallOutcome>) + Send>;
/// Exactly-once completion callback for an admitted live-runtime grant request.
pub type ExtensionRuntimeGrantCallback =
    Box<dyn FnOnce(ExtensionManagementSettlement<ExtensionRuntimeGrantOutcome>) + Send>;
/// Exactly-once completion callback for an admitted management-catalog read.
pub type ExtensionManagementCatalogCallback =
    Box<dyn FnOnce(ExtensionManagementCatalogOutcome) + Send>;
/// Exactly-once completion callback for an admitted repository-maintenance
/// turn.
pub type ExtensionRepositoryMaintenanceCallback =
    Box<dyn FnOnce(ExtensionRepositoryMaintenanceOutcome) + Send>;

impl<T> ExtensionManagementSettlement<T> {
    pub const fn new(outcome: T, active_profiles: Option<ExtensionActiveProfiles>) -> Self {
        Self {
            outcome,
            active_profiles,
        }
    }

    pub const fn outcome(&self) -> &T {
        &self.outcome
    }

    pub const fn active_profiles(&self) -> Option<ExtensionActiveProfiles> {
        self.active_profiles
    }

    pub fn into_outcome(self) -> T {
        self.outcome
    }
}

/// Move-only application lifecycle boundary for the extension service.
///
/// The unique owner is held behind a `Box` and consumed by shutdown. This
/// keeps implementation-specific mutation and cleanup authority out of the
/// application layer, while allowing the owner to move onto the application
/// actor thread. No cloneable or borrowed shutdown operation exists.
pub trait ExtensionServiceLifecycle: Send {
    /// Observes the active startup attempt, or admits one bounded retry when
    /// the previous exact attempt settled unavailable.
    ///
    /// This call may wait until the absolute `deadline` and must run away from
    /// a native UI/event-loop thread: native-owner reconciliation can require
    /// that same thread to service WebKit/WebView2 work. Only
    /// [`ExtensionServiceStartupOutcome::Ready`] permits extension-sensitive
    /// application bootstrap.
    fn settle_startup_until(&mut self, deadline: Instant) -> ExtensionServiceStartupOutcome;

    /// Reports whether management is configured without touching repository
    /// state or starting work. The conservative default prevents compatibility
    /// and test lifecycles from accidentally advertising product authority.
    fn extension_management_availability(&self) -> ExtensionManagementAvailability {
        ExtensionManagementAvailability::Unavailable
    }

    /// Authenticates and durably materializes one exact package from the
    /// product-sealed acquired catalog.
    ///
    /// This is a transport boundary, not open-store installation: the
    /// implementation must reauthenticate the exact catalog, CRX developer
    /// identity, archive payload, manifest policy, legal notice, and complete
    /// extracted tree before publishing any package object. The default fails
    /// closed and consumes the move-only bytes without retaining them.
    fn provision_acquired_package_until(
        &mut self,
        _request: ExtensionAcquiredPackageProvisioningRequest,
        _deadline: Instant,
    ) -> ExtensionAcquiredPackageProvisioningOutcome {
        ExtensionAcquiredPackageProvisioningOutcome::FailedClosed
    }

    /// Non-blocking acquired-package provisioning form used by transport
    /// coordinators. `Accepted` transfers both payload and callback ownership.
    fn begin_provision_acquired_package(
        &mut self,
        _request: ExtensionAcquiredPackageProvisioningRequest,
        _deadline: Instant,
        done: ExtensionAcquiredPackageProvisioningCallback,
    ) -> ExtensionManagementAdmission {
        drop(done);
        ExtensionManagementAdmission::Unavailable
    }

    /// Source-free verifies and atomically activates one complete acquired
    /// catalog selection after every package has been materialized.
    fn activate_acquired_catalog_until(
        &mut self,
        _request: ExtensionAcquiredCatalogActivationRequest,
        _deadline: Instant,
    ) -> ExtensionAcquiredCatalogActivationOutcome {
        ExtensionAcquiredCatalogActivationOutcome::FailedClosed
    }

    /// Non-blocking complete-catalog activation form.
    fn begin_activate_acquired_catalog(
        &mut self,
        _request: ExtensionAcquiredCatalogActivationRequest,
        _deadline: Instant,
        done: ExtensionAcquiredCatalogActivationCallback,
    ) -> ExtensionManagementAdmission {
        drop(done);
        ExtensionManagementAdmission::Unavailable
    }

    /// Activates one exact regular/private runtime through the service's
    /// authenticated, serialized authority transaction.
    ///
    /// This call follows the same native-event-loop restriction as startup.
    /// The default fails closed so inert and test adapters cannot accidentally
    /// claim activation without owning the runtime coordinator.
    fn activate_runtime_until(
        &mut self,
        _key: ExtensionNativeOwnershipKey,
        _deadline: Instant,
    ) -> ExtensionRuntimeActivationDisposition {
        ExtensionRuntimeActivationDisposition::FailedClosed
    }

    /// Retires one exact runtime before Shell mutates durable disable or
    /// uninstall intent.
    ///
    /// The returned active-profile projection is routing data only. The
    /// default fails closed and grants no absence authority.
    fn retire_runtime_until(
        &mut self,
        _key: ExtensionNativeOwnershipKey,
        _deadline: Instant,
    ) -> ExtensionRuntimeRetirementDisposition {
        ExtensionRuntimeRetirementDisposition::FailedClosed
    }

    /// Atomically coordinates one stale-resistant enable/disable request with
    /// native runtime ownership and durable desired state.
    ///
    /// Implementations retire both regular and private owners before a
    /// disabling write, and persist enabled intent before entering ordinary
    /// authenticated activation. The default fails closed.
    fn set_install_enabled_until(
        &mut self,
        _selector: ExtensionInstallSelector,
        _enabled: bool,
        _deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionSetEnabledOutcome> {
        ExtensionManagementSettlement::new(ExtensionSetEnabledOutcome::FailedClosed, None)
    }

    /// Non-blocking form used by the application actor. `Accepted` transfers
    /// callback ownership and the implementation must settle it exactly once
    /// after its serialized worker completes the request.
    fn begin_set_install_enabled(
        &mut self,
        _selector: ExtensionInstallSelector,
        _enabled: bool,
        _deadline: Instant,
        done: ExtensionSetEnabledCallback,
    ) -> ExtensionManagementAdmission {
        drop(done);
        ExtensionManagementAdmission::Unavailable
    }

    /// Installs one authenticated current-catalog package with its required
    /// declarations, then enters ordinary enablement and activation.
    ///
    /// The selection contains only indexes into Shell's exact canonical
    /// optional arrays plus independent file/private decisions. Callers cannot
    /// submit API names or host patterns; the serialized service derives those
    /// only from the reauthenticated manifest.
    fn install_until(
        &mut self,
        _selector: ExtensionInstallCandidateSelector,
        _selection: ExtensionInitialGrantSelection,
        _deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionInstallOutcome> {
        ExtensionManagementSettlement::new(ExtensionInstallOutcome::FailedClosed, None)
    }

    /// Non-blocking curated-install form used by the application actor.
    fn begin_install(
        &mut self,
        _selector: ExtensionInstallCandidateSelector,
        _selection: ExtensionInitialGrantSelection,
        _deadline: Instant,
        done: ExtensionInstallCallback,
    ) -> ExtensionManagementAdmission {
        drop(done);
        ExtensionManagementAdmission::Unavailable
    }

    /// Retires every regular/private owner before deleting one exact install.
    /// Grant rows are subordinate durable state and are removed by the same
    /// Store transaction. The default fails closed.
    fn uninstall_until(
        &mut self,
        _selector: ExtensionInstallSelector,
        _deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionUninstallOutcome> {
        ExtensionManagementSettlement::new(ExtensionUninstallOutcome::FailedClosed, None)
    }

    /// Non-blocking uninstall form used by the application actor.
    fn begin_uninstall(
        &mut self,
        _selector: ExtensionInstallSelector,
        _deadline: Instant,
        done: ExtensionUninstallCallback,
    ) -> ExtensionManagementAdmission {
        drop(done);
        ExtensionManagementAdmission::Unavailable
    }

    /// Applies one live runtime's declared optional API/host request only
    /// after retiring every native context for the exact install.
    ///
    /// The generation is a process-local stale-context fence. Implementations
    /// must reauthenticate the manifest and durable grant cohort, commit the
    /// request as one revision, and reactivate only contexts that were live.
    fn request_runtime_grants_until(
        &mut self,
        _key: ExtensionNativeOwnershipKey,
        _generation: ExtensionRuntimeGeneration,
        _request: ExtensionRuntimeGrantRequest,
        _deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionRuntimeGrantOutcome> {
        ExtensionManagementSettlement::new(ExtensionRuntimeGrantOutcome::FailedClosed, None)
    }

    /// Non-blocking live-runtime grant form used by native permission bridges.
    fn begin_request_runtime_grants(
        &mut self,
        _key: ExtensionNativeOwnershipKey,
        _generation: ExtensionRuntimeGeneration,
        _request: ExtensionRuntimeGrantRequest,
        _deadline: Instant,
        done: ExtensionRuntimeGrantCallback,
    ) -> ExtensionManagementAdmission {
        drop(done);
        ExtensionManagementAdmission::Unavailable
    }

    /// Starts one lazy, read-only installed-extension management projection.
    ///
    /// Implementations must authenticate the complete current catalog, join it
    /// to one atomic Store grant cohort, and observe runtime state on the same
    /// serialized worker. The read must not acquire package pins, activate an
    /// extension, construct native controllers, or add startup work.
    fn begin_load_management_catalog(
        &mut self,
        _profile: ProfileId,
        _deadline: Instant,
        done: ExtensionManagementCatalogCallback,
    ) -> ExtensionManagementCatalogAdmission {
        drop(done);
        ExtensionManagementCatalogAdmission::Unavailable
    }

    /// Whether this lifecycle owns a repository worker that can accept
    /// maintenance. The inert implementation remains allocation-free by
    /// returning `false` before Shell constructs a callback.
    fn repository_maintenance_is_available(&self) -> bool {
        false
    }

    /// Admits at most one bounded repository garbage-collection turn.
    ///
    /// Implementations must serialize the turn with all other repository work
    /// and coalesce duplicate wakeups. The default refuses without invoking
    /// the callback, so inert and compatibility implementations cannot claim
    /// maintenance they do not own.
    fn begin_repository_maintenance(
        &mut self,
        _deadline: Instant,
        done: ExtensionRepositoryMaintenanceCallback,
    ) -> ExtensionRepositoryMaintenanceAdmission {
        drop(done);
        ExtensionRepositoryMaintenanceAdmission::Unavailable
    }

    /// Permanently fences `profile`, proves every extension-owned durable,
    /// package, and native obligation absent, then invokes `continuation`
    /// exactly once before returning [`ExtensionProfileRetirementDisposition::Continued`].
    ///
    /// The continuation is the authority boundary. Implementations must drop
    /// it without invocation for every unavailable or failed-closed result.
    /// Zephium callers must put Store authorization, native website-data
    /// erasure, or Store finalization *inside* this continuation and must not
    /// branch on the copied return value to perform those effects later. This
    /// is an audited composition rule, not a type-level capability guarantee.
    ///
    /// This call may wait until the absolute `deadline` and follows the same
    /// native-event-loop restriction as [`Self::settle_startup_until`]. The
    /// default is fail-closed so test-only or compatibility lifecycle adapters
    /// cannot accidentally authorize deletion when they have no retirement
    /// implementation.
    fn with_profile_retired_until(
        &mut self,
        _profile: ProfileId,
        _deadline: Instant,
        continuation: Box<dyn FnOnce() + '_>,
    ) -> ExtensionProfileRetirementDisposition {
        drop(continuation);
        ExtensionProfileRetirementDisposition::FailedClosed
    }

    /// Seals service admission and consumes the unique owner while attempting
    /// to prove worker termination and resource release by `deadline`.
    ///
    /// The deadline is absolute. Every non-clean concrete service outcome
    /// must project to [`ExtensionServiceShutdownOutcome::Unclean`]; ownership
    /// has already been consumed and cannot be retried in-process.
    fn shutdown_until(self: Box<Self>, deadline: Instant) -> ExtensionServiceShutdownOutcome;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::*;

    struct TestLifecycle {
        consumed: Arc<AtomicBool>,
    }

    impl ExtensionServiceLifecycle for TestLifecycle {
        fn settle_startup_until(&mut self, _deadline: Instant) -> ExtensionServiceStartupOutcome {
            ExtensionServiceStartupOutcome::Ready(ExtensionActiveProfiles::EMPTY)
        }

        fn with_profile_retired_until(
            &mut self,
            _profile: ProfileId,
            _deadline: Instant,
            continuation: Box<dyn FnOnce() + '_>,
        ) -> ExtensionProfileRetirementDisposition {
            continuation();
            ExtensionProfileRetirementDisposition::Continued
        }

        fn shutdown_until(self: Box<Self>, _deadline: Instant) -> ExtensionServiceShutdownOutcome {
            self.consumed.store(true, Ordering::Release);
            ExtensionServiceShutdownOutcome::Clean
        }
    }

    fn assert_send<T: Send>() {}

    #[test]
    fn initial_grant_selection_canonicalizes_bounded_indices() {
        let selection =
            ExtensionInitialGrantSelection::new(vec![2, 0], 3, vec![1, 0], 2, true, false).unwrap();
        assert_eq!(selection.optional_api_indices(), [0, 2]);
        assert_eq!(selection.optional_host_indices(), [0, 1]);
        assert!(selection.file_access());
        assert!(!selection.private_access());
    }

    #[test]
    fn initial_grant_selection_rejects_duplicates_and_stale_indices() {
        assert_eq!(
            ExtensionInitialGrantSelection::new(vec![0, 0], 2, Vec::new(), 0, false, false),
            Err(ExtensionInitialGrantSelectionError::DuplicateIndex)
        );
        assert_eq!(
            ExtensionInitialGrantSelection::new(Vec::new(), 0, vec![1], 1, false, false),
            Err(ExtensionInitialGrantSelectionError::OutOfBounds)
        );
        assert_eq!(
            ExtensionInitialGrantSelection::new(
                Vec::new(),
                0,
                Vec::new(),
                MAX_EXTENSION_HOST_PERMISSION_PATTERNS + 1,
                false,
                false,
            ),
            Err(ExtensionInitialGrantSelectionError::OutOfBounds)
        );
    }

    #[test]
    fn lifecycle_is_send_object_safe_and_consumed_by_shutdown() {
        assert_send::<Box<dyn ExtensionServiceLifecycle>>();
        let consumed = Arc::new(AtomicBool::new(false));
        let mut lifecycle: Box<dyn ExtensionServiceLifecycle> = Box::new(TestLifecycle {
            consumed: Arc::clone(&consumed),
        });

        assert_eq!(
            lifecycle.settle_startup_until(Instant::now()),
            ExtensionServiceStartupOutcome::Ready(ExtensionActiveProfiles::EMPTY)
        );
        assert_eq!(
            lifecycle.extension_management_availability(),
            ExtensionManagementAvailability::Unavailable
        );
        let key = ExtensionNativeOwnershipKey::new(
            ProfileId::from(7),
            crate::ids::ExtensionInstallId::from(1),
            crate::extensions::ExtensionGrantBrowsingContext::Regular,
        );
        assert_eq!(
            lifecycle.activate_runtime_until(key, Instant::now()),
            ExtensionRuntimeActivationDisposition::FailedClosed
        );
        assert_eq!(
            lifecycle.retire_runtime_until(key, Instant::now()),
            ExtensionRuntimeRetirementDisposition::FailedClosed
        );
        let selector = ExtensionInstallSelector::new(
            ProfileId::from(7),
            crate::ids::ExtensionInstallId::from(1),
            crate::extensions::ExtensionInstallCatalogRevision::INITIAL,
            crate::extensions::ExtensionInstallRevision::INITIAL,
        );
        assert_eq!(
            lifecycle.set_install_enabled_until(selector, true, Instant::now()),
            ExtensionManagementSettlement::new(ExtensionSetEnabledOutcome::FailedClosed, None)
        );
        let package = crate::extensions::ExtensionPackageIdentity::new(
            crate::extensions::ExtensionAuthorityId::from_bytes([1; 32]),
            crate::extensions::ExtensionPackageKey::from_bytes([2; 32]),
            crate::extensions::ExtensionPackageRevision::INITIAL,
            crate::extensions::ExtensionPackagePayloadIdentity::BundledTree,
            crate::extensions::ExtensionManifestDigest::from_bytes([3; 32]),
            crate::extensions::ExtensionTreeDigest::from_bytes([4; 32]),
        );
        let candidate = ExtensionInstallCandidateSelector::new(
            ProfileId::from(7),
            crate::extensions::ExtensionInstallCatalogRevision::INITIAL,
            crate::extensions::ExtensionCatalogSetDigest::from_bytes([5; 32]),
            package,
        );
        assert_eq!(
            lifecycle.install_until(
                candidate,
                ExtensionInitialGrantSelection::new(Vec::new(), 0, Vec::new(), 0, false, false,)
                    .unwrap(),
                Instant::now(),
            ),
            ExtensionManagementSettlement::new(ExtensionInstallOutcome::FailedClosed, None)
        );
        assert_eq!(
            lifecycle.uninstall_until(selector, Instant::now()),
            ExtensionManagementSettlement::new(ExtensionUninstallOutcome::FailedClosed, None)
        );
        assert!(!lifecycle.repository_maintenance_is_available());
        let maintenance_callback_called = Arc::new(AtomicBool::new(false));
        let maintenance_callback_observer = Arc::clone(&maintenance_callback_called);
        assert_eq!(
            lifecycle.begin_repository_maintenance(
                Instant::now(),
                Box::new(move |_| { maintenance_callback_observer.store(true, Ordering::Release) }),
            ),
            ExtensionRepositoryMaintenanceAdmission::Unavailable
        );
        assert!(!maintenance_callback_called.load(Ordering::Acquire));
        let continued = Arc::new(AtomicBool::new(false));
        let continued_by_callback = Arc::clone(&continued);
        assert_eq!(
            lifecycle.with_profile_retired_until(
                ProfileId::from(7),
                Instant::now(),
                Box::new(move || continued_by_callback.store(true, Ordering::Release)),
            ),
            ExtensionProfileRetirementDisposition::Continued
        );
        assert!(continued.load(Ordering::Acquire));
        assert_eq!(
            lifecycle.shutdown_until(Instant::now()),
            ExtensionServiceShutdownOutcome::Clean
        );
        assert!(consumed.load(Ordering::Acquire));
    }

    #[test]
    fn active_profile_projection_is_unique_compact_and_bounded() {
        let mut profiles = ExtensionActiveProfiles::EMPTY;
        for value in 1..=MAX_EXTENSION_ACTIVE_PROFILES {
            assert!(profiles.try_insert(ProfileId::from(value as u128)));
        }
        assert!(profiles.try_insert(ProfileId::from(2)));
        assert!(!profiles.try_insert(ProfileId::from(99)));
        assert_eq!(
            profiles.iter().collect::<Vec<_>>(),
            vec![ProfileId::from(1), ProfileId::from(2), ProfileId::from(3)]
        );
        assert!(profiles.remove(ProfileId::from(2)));
        assert_eq!(
            profiles.iter().collect::<Vec<_>>(),
            vec![ProfileId::from(1), ProfileId::from(3)]
        );
        assert!(!profiles.remove(ProfileId::from(2)));
        assert!(profiles.try_insert(ProfileId::from(4)));
        assert_eq!(
            profiles.iter().collect::<Vec<_>>(),
            vec![ProfileId::from(1), ProfileId::from(3), ProfileId::from(4)]
        );
    }
}
