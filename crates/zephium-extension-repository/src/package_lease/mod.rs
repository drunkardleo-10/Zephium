//! Opaque authenticated package leases and exact owner-pin release authority.

mod resource;

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::mem::size_of;
use std::sync::{Arc, Weak};

use thiserror::Error;
use zephium_core::extensions::{
    ExtensionManifestDescriptor, ExtensionPackageIdentity, ExtensionRuntimeEligibility,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_extension_authority::{
    BundledCatalogAdmissionError, BundledPackageAuthority, ProductExtensionManifestAdmissionError,
    ProductExtensionManifestAuthorityError,
};
use zephium_extension_package::{ExtensionReleaseCatalogRevision, PortableRelativePath};

use crate::materialization::{
    add_owner_package_pin, current_catalog_set_projection, load_active_package_snapshot,
    load_rollback_package_snapshot, plan_current_catalog_package_pin,
    plan_owner_package_pin_removal, remove_owner_package_pin,
    validated_resumable_build_in_progress, MaterializationTransitionError, OwnerPackagePinIdentity,
    OwnerPackagePinPlan, OwnerPackagePinRemovalPlan, PackageLeaseRepositoryIdentity,
    PackageObjectError, SnapshotLoadError, SnapshotObjectPhase, VerifiedActivePackageSnapshot,
    VerifiedCatalogRole, VerifiedRollbackPackageSnapshot, MAX_COMPLETED_PACKAGE_RECORDS,
    MAX_DURABLE_PACKAGE_PINS,
};
use crate::operation::{RepositoryHealth, RepositoryOperationError, RepositoryOperationGate};
use crate::state::Digest32;
use crate::writer::{completed_error_requires_sealing, preflight_error_requires_sealing};
use crate::{
    BundledCatalogSetIdentity, BundledPackageMaterializationError, ExtensionRepository,
    ExtensionRepositoryError,
};

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
std::thread_local! {
    static POST_PIN_REVERIFY_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
    static RELEASE_PLANNING_ERROR_HOOK: std::cell::Cell<Option<MaterializationTransitionError>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
fn arm_post_pin_reverify_hook(hook: impl FnOnce() + 'static) {
    POST_PIN_REVERIFY_HOOK.with(|slot| {
        assert!(slot.borrow_mut().replace(Box::new(hook)).is_none());
    });
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
fn run_post_pin_reverify_hook() {
    POST_PIN_REVERIFY_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook();
        }
    });
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
fn arm_release_planning_error_hook(error: MaterializationTransitionError) {
    RELEASE_PLANNING_ERROR_HOOK.with(|slot| {
        assert!(slot.replace(Some(error)).is_none());
    });
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
fn take_release_planning_error_hook() -> Option<MaterializationTransitionError> {
    RELEASE_PLANNING_ERROR_HOOK.with(|slot| slot.take())
}

struct RepositoryOpenEpoch;

struct LeasePresence {
    open_epoch: Arc<RepositoryOpenEpoch>,
    repository: PackageLeaseRepositoryIdentity,
    profile: ProfileId,
    install: ExtensionInstallId,
}

type LeaseOwner = (ProfileId, ExtensionInstallId);

pub(crate) struct PackageLeaseRuntime {
    open_epoch: Arc<RepositoryOpenEpoch>,
    operation_gate: Arc<RepositoryOperationGate>,
    health: Arc<RepositoryHealth>,
    live: BTreeMap<LeaseOwner, Weak<LeasePresence>>,
    active_cache: BTreeMap<Digest32, Weak<VerifiedActivePackageSnapshot>>,
    rollback_cache: BTreeMap<Digest32, Weak<VerifiedRollbackPackageSnapshot>>,
}

impl PackageLeaseRuntime {
    pub(crate) fn new() -> Self {
        Self {
            open_epoch: Arc::new(RepositoryOpenEpoch),
            operation_gate: Arc::new(RepositoryOperationGate::new()),
            health: Arc::new(RepositoryHealth::new()),
            live: BTreeMap::new(),
            active_cache: BTreeMap::new(),
            rollback_cache: BTreeMap::new(),
        }
    }

    pub(crate) fn health(&self) -> &Arc<RepositoryHealth> {
        &self.health
    }

    fn open_epoch(&self) -> &Arc<RepositoryOpenEpoch> {
        &self.open_epoch
    }

    pub(crate) fn operation_gate(&self) -> &Arc<RepositoryOperationGate> {
        &self.operation_gate
    }

    fn share_active(
        &mut self,
        fresh: VerifiedActivePackageSnapshot,
    ) -> Result<Arc<VerifiedActivePackageSnapshot>, LocalLeaseError> {
        self.active_cache
            .retain(|_, value| value.strong_count() != 0);
        let record_id = fresh.record_id();
        if let Some(cached) = self.active_cache.get(&record_id).and_then(Weak::upgrade) {
            if !cached.exactly_matches(&fresh) {
                self.health.poison();
                return Err(LocalLeaseError::SnapshotMismatch);
            }
            return Ok(cached);
        }
        if self.active_cache.len() >= MAX_COMPLETED_PACKAGE_RECORDS {
            return Err(LocalLeaseError::CapacityExhausted);
        }
        let shared = Arc::new(fresh);
        self.active_cache.insert(record_id, Arc::downgrade(&shared));
        Ok(shared)
    }

    fn share_rollback(
        &mut self,
        fresh: VerifiedRollbackPackageSnapshot,
    ) -> Result<Arc<VerifiedRollbackPackageSnapshot>, LocalLeaseError> {
        self.rollback_cache
            .retain(|_, value| value.strong_count() != 0);
        let record_id = fresh.record_id();
        if let Some(cached) = self.rollback_cache.get(&record_id).and_then(Weak::upgrade) {
            if !cached.exactly_matches(&fresh) {
                self.health.poison();
                return Err(LocalLeaseError::SnapshotMismatch);
            }
            return Ok(cached);
        }
        if self.rollback_cache.len() >= MAX_COMPLETED_PACKAGE_RECORDS {
            return Err(LocalLeaseError::CapacityExhausted);
        }
        let shared = Arc::new(fresh);
        self.rollback_cache
            .insert(record_id, Arc::downgrade(&shared));
        Ok(shared)
    }

    fn reserve(
        &mut self,
        repository: PackageLeaseRepositoryIdentity,
        profile: ProfileId,
        install: ExtensionInstallId,
    ) -> Result<Arc<LeasePresence>, LocalLeaseError> {
        self.live.retain(|_, value| value.strong_count() != 0);
        let owner = (profile, install);
        if self.live.get(&owner).and_then(Weak::upgrade).is_some() {
            return Err(LocalLeaseError::AlreadyOpen);
        }
        self.live.remove(&owner);
        if self.live.len() >= MAX_DURABLE_PACKAGE_PINS {
            return Err(LocalLeaseError::CapacityExhausted);
        }
        let presence = Arc::new(LeasePresence {
            open_epoch: Arc::clone(&self.open_epoch),
            repository,
            profile,
            install,
        });
        self.live.insert(owner, Arc::downgrade(&presence));
        Ok(presence)
    }

    fn install_release_presence(
        &mut self,
        presence: &Arc<LeasePresence>,
    ) -> Result<(), LocalLeaseError> {
        self.live.retain(|_, value| value.strong_count() != 0);
        let owner = (presence.profile, presence.install);
        match self.live.get(&owner).and_then(Weak::upgrade) {
            Some(current) if Arc::ptr_eq(&current, presence) => Ok(()),
            Some(_) => Err(LocalLeaseError::ConcurrentLease),
            None => {
                if self.live.len() >= MAX_DURABLE_PACKAGE_PINS {
                    return Err(LocalLeaseError::ConcurrentLease);
                }
                self.live.insert(owner, Arc::downgrade(presence));
                Ok(())
            }
        }
    }

    fn retire_presence(&mut self, presence: &Arc<LeasePresence>) {
        let owner = (presence.profile, presence.install);
        if self
            .live
            .get(&owner)
            .and_then(Weak::upgrade)
            .is_some_and(|current| Arc::ptr_eq(&current, presence))
        {
            self.live.remove(&owner);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalLeaseError {
    AlreadyOpen,
    ConcurrentLease,
    SnapshotMismatch,
    CapacityExhausted,
}

/// Product-sealed role of the repository's exact current catalog set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BundledCatalogGenerationRole {
    /// Ordinary active product generation.
    Active,
    /// Explicitly product-approved rollback generation.
    Rollback,
}

/// Path-free identity and role of the exact current bundled catalog set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BundledCurrentCatalogSet {
    identity: BundledCatalogSetIdentity,
    role: BundledCatalogGenerationRole,
}

impl BundledCurrentCatalogSet {
    /// Returns the content-addressed current selection identity.
    pub const fn identity(self) -> BundledCatalogSetIdentity {
        self.identity
    }

    /// Returns whether the current selection is active or rollback-authorized.
    pub const fn role(self) -> BundledCatalogGenerationRole {
        self.role
    }
}

/// Stable, path-free package-lease acquisition failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BundledPackageLeaseError {
    /// Repository recovery or private storage refused the operation.
    #[error("extension package lease repository operation failed: {0}")]
    Repository(#[source] ExtensionRepositoryError),
    /// No current atomic bundled catalog set exists.
    #[error("extension repository has no current bundled catalog set")]
    NoCurrentSelection,
    /// A coherent crash-resumable package build must settle before leasing.
    #[error("an extension package build is in progress")]
    BuildInProgress,
    /// The current selection differs from the caller's exact observed identity.
    #[error("extension bundled catalog selection changed")]
    StaleSelection,
    /// The eligibility package is absent from the exact current selection.
    #[error("eligible extension package is not selected by the current catalog set")]
    PackageNotSelected,
    /// Active and rollback lease methods cannot consume the other role.
    #[error("extension catalog set has the wrong generation role")]
    WrongCatalogRole,
    /// Fresh product admission did not equal the complete store eligibility.
    #[error("extension runtime eligibility differs from the authenticated package")]
    EligibilityMismatch,
    /// The owner already has a different durable package pin.
    #[error("extension owner is pinned to another package")]
    OwnerConflict,
    /// This open repository already has a live or release-pending owner lease.
    #[error("extension owner already has a live package lease")]
    LeaseAlreadyOpen,
    /// Product-sealed catalog authority is unavailable.
    #[error("bundled extension catalog authority is unavailable: {0}")]
    CatalogAuthority(#[source] BundledCatalogAdmissionError),
    /// Exact repository-owned catalog bytes failed fresh product admission.
    #[error("bundled extension catalog admission failed: {0}")]
    CatalogAdmission(#[source] BundledCatalogAdmissionError),
    /// Product manifest authority is unavailable.
    #[error("extension manifest authority is unavailable: {0}")]
    ManifestAuthority(#[source] ProductExtensionManifestAuthorityError),
    /// Exact repository-owned manifest bytes failed fresh product admission.
    #[error("extension manifest admission failed: {0}")]
    ManifestAdmission(#[source] ProductExtensionManifestAdmissionError),
    /// The selected package has not completed durable materialization.
    #[error("selected extension package is not durably materialized")]
    PackageNotMaterialized,
    /// A repository-owned final differs from its authenticated identity.
    #[error("extension package durable object closure is not exact")]
    DurableObjectMismatch,
    /// A bounded repository or retained-memory inventory is exhausted.
    #[error("extension package lease capacity is exhausted")]
    CapacityExhausted,
}

/// Stable, path-free failure while releasing an exact owner pin.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BundledPackageLeaseReleaseError {
    /// Repository recovery or durable mutation failed.
    #[error("extension package release repository operation failed: {0}")]
    Repository(#[source] ExtensionRepositoryError),
    /// The release request belongs to another namespace or repository open.
    #[error("extension package release request belongs to another repository open")]
    WrongRepository,
    /// A coherent crash-resumable package build must settle before release.
    #[error("an extension package build is in progress")]
    BuildInProgress,
    /// Another live lease owns this owner slot in this repository open.
    #[error("another live extension package lease owns this release slot")]
    ConcurrentLease,
    /// The durable owner pin has a different package record or incarnation.
    #[error("extension package release request is stale")]
    StaleLease,
}

/// Exact durable release outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[must_use = "new release and idempotent absence have different effects"]
pub enum BundledPackageLeaseReleaseOutcome {
    /// The exact durable owner pin was removed.
    Released,
    /// The owner pin was already absent or this request already settled.
    AlreadyReleased,
}

/// Stable, path-free package-resource read failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BundledPackageResourceError {
    /// The repository health shared by this lease has failed closed.
    #[error("extension package lease is inactive")]
    LeaseInactive,
    /// The exact path is absent from the authenticated closed tree index.
    #[error("extension package resource is not declared")]
    ResourceNotDeclared,
    /// The private filesystem could not make the declared resource readable.
    #[error("extension package resource is unavailable")]
    ResourceReadUnavailable,
    /// Resource bytes, length, type, or identity differ from the tree index.
    #[error("extension package resource differs from its authenticated identity")]
    DurableResourceMismatch,
    /// The private namespace failed closed after an identity ambiguity.
    #[error("extension package private namespace is quarantined")]
    NamespaceQuarantined,
}

struct PackageLeaseCore<Snapshot> {
    open_epoch: Arc<RepositoryOpenEpoch>,
    operation_gate: Arc<RepositoryOperationGate>,
    repository: PackageLeaseRepositoryIdentity,
    current_set: BundledCatalogSetIdentity,
    profile: ProfileId,
    install: ExtensionInstallId,
    pin: OwnerPackagePinIdentity,
    presence: Arc<LeasePresence>,
    health: Arc<RepositoryHealth>,
    snapshot: Arc<Snapshot>,
}

/// Authenticated active-generation package access and durable pinning authority.
///
/// This lease grants path-free reads of one exact package and retains its owner
/// pin. It is not native activation authority. Activation requires a separate
/// service-owned join with native-runtime authority.
///
/// Dropping this lease performs no repository read, write, or mutation and
/// never removes its durable owner pin.
/// The caller must consume it into an exact release request and settle that
/// request after native teardown. A crash or accidental drop therefore fails
/// safe: exact reacquisition replays the pin, while absent-install orphan
/// reconciliation requires a separate Store-authorized service.
///
/// ```compile_fail
/// use zephium_extension_repository::ActiveBundledPackageLease;
/// fn require_clone<T: Clone>() {}
/// fn cannot_clone() { require_clone::<ActiveBundledPackageLease>(); }
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::ActiveBundledPackageLease;
/// fn require_serialize<T: serde::Serialize>() {}
/// fn cannot_serialize() { require_serialize::<ActiveBundledPackageLease>(); }
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::ActiveBundledPackageLease;
/// fn forge() -> ActiveBundledPackageLease { ActiveBundledPackageLease {} }
/// ```
#[must_use = "an active package lease must remain owned until native teardown settles"]
pub struct ActiveBundledPackageLease {
    core: PackageLeaseCore<VerifiedActivePackageSnapshot>,
}

/// Authenticated rollback-generation package access and durable pinning authority.
///
/// This lease grants path-free reads of one exact rollback package and retains
/// its owner pin. It is not native activation authority. Recovery activation
/// requires a separate service-owned join with native-runtime authority.
///
/// Dropping this lease performs no repository read, write, or mutation and
/// never removes its durable owner pin.
/// The caller must consume it into an exact release request and settle that
/// request after native teardown. A crash or accidental drop therefore fails
/// safe: exact reacquisition replays the pin, while absent-install orphan
/// reconciliation requires a separate Store-authorized service.
///
/// ```compile_fail
/// use zephium_extension_repository::RollbackBundledPackageLease;
/// fn require_clone<T: Clone>() {}
/// fn cannot_clone() { require_clone::<RollbackBundledPackageLease>(); }
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::RollbackBundledPackageLease;
/// fn require_serialize<T: serde::Serialize>() {}
/// fn cannot_serialize() { require_serialize::<RollbackBundledPackageLease>(); }
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::RollbackBundledPackageLease;
/// fn forge() -> RollbackBundledPackageLease { RollbackBundledPackageLease {} }
/// ```
#[must_use = "a rollback package lease must remain owned until native teardown settles"]
pub struct RollbackBundledPackageLease {
    core: PackageLeaseCore<VerifiedRollbackPackageSnapshot>,
}

impl fmt::Debug for ActiveBundledPackageLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_lease(formatter, "ActiveBundledPackageLease", &self.core)
    }
}

impl fmt::Debug for RollbackBundledPackageLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_lease(formatter, "RollbackBundledPackageLease", &self.core)
    }
}

fn debug_lease<Snapshot>(
    formatter: &mut fmt::Formatter<'_>,
    name: &str,
    _core: &PackageLeaseCore<Snapshot>,
) -> fmt::Result {
    formatter.debug_struct(name).finish_non_exhaustive()
}

macro_rules! impl_lease {
    ($lease:ident, $request:ident, $snapshot:ty) => {
        impl $lease {
            /// Returns the exact profile owner.
            pub const fn profile(&self) -> ProfileId {
                self.core.profile
            }

            /// Returns the exact profile-scoped installation owner.
            pub const fn install_id(&self) -> ExtensionInstallId {
                self.core.install
            }

            /// Returns the atomic catalog-set identity used at acquisition.
            pub const fn current_catalog_set(&self) -> BundledCatalogSetIdentity {
                self.core.current_set
            }

            /// Returns the complete authenticated package identity.
            pub fn package(&self) -> &ExtensionPackageIdentity {
                self.core.snapshot.package()
            }

            /// Returns the exact admitted structural manifest descriptor.
            pub fn manifest(&self) -> &ExtensionManifestDescriptor {
                self.core.snapshot.descriptor()
            }

            /// Returns the authenticated catalog revision.
            pub fn catalog_revision(&self) -> ExtensionReleaseCatalogRevision {
                self.core.snapshot.catalog_revision()
            }

            /// Returns a conservative logical retained-memory charge.
            ///
            /// Immutable snapshots are physically shared by exact package record,
            /// so this logical per-lease charge does not imply duplicate allocation.
            pub fn retained_bytes(&self) -> usize {
                size_of::<Self>().saturating_add(self.core.snapshot.retained_bytes())
            }

            /// Reads one declared resource through the sealed descriptor-relative tree.
            ///
            /// The callback runs synchronously under the repository and namespace
            /// operation locks and must not re-enter this repository or another lease
            /// issued by the same open. Integrity verification drains and hashes the
            /// remainder even when the callback returns `Err(E)`.
            pub fn with_resource_reader<T, E>(
                &self,
                path: &PortableRelativePath,
                callback: impl FnOnce(&mut dyn Read) -> Result<T, E>,
            ) -> Result<Result<T, E>, BundledPackageResourceError> {
                let _operation = self
                    .core
                    .operation_gate
                    .enter(&self.core.health)
                    .map_err(|_| BundledPackageResourceError::LeaseInactive)?;
                resource::with_resource(
                    &self.core.health,
                    self.core.snapshot.root(),
                    self.core.snapshot.index(),
                    path,
                    callback,
                )
            }

            /// Consumes package access and pinning authority before teardown.
            ///
            /// The returned request retains no manifest, tree, filesystem handle, or
            /// runtime authority. After native teardown it may be retried only against
            /// the same still-open repository following a clean transient failure.
            /// Dropping either value performs no repository read, write, or mutation
            /// and never unpins; callers must explicitly settle the exact request.
            /// Recovery after repository reopen requires exact lease reacquisition or
            /// Store-authorized reconciliation.
            pub fn into_release_request(self) -> $request {
                let PackageLeaseCore {
                    open_epoch,
                    operation_gate: _,
                    repository,
                    current_set: _,
                    profile,
                    install,
                    pin,
                    presence,
                    health: _,
                    snapshot: _,
                } = self.core;
                $request {
                    core: PackageReleaseRequestCore {
                        open_epoch,
                        repository,
                        profile,
                        install,
                        pin,
                        presence,
                        state: PackageReleaseRequestState::ReleasePending,
                    },
                }
            }
        }
    };
}

impl_lease!(
    ActiveBundledPackageLease,
    ActiveBundledPackageReleaseRequest,
    VerifiedActivePackageSnapshot
);
impl_lease!(
    RollbackBundledPackageLease,
    RollbackBundledPackageReleaseRequest,
    VerifiedRollbackPackageSnapshot
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PackageReleaseRequestState {
    ReleasePending,
    Released,
}

struct PackageReleaseRequestCore {
    open_epoch: Arc<RepositoryOpenEpoch>,
    repository: PackageLeaseRepositoryIdentity,
    profile: ProfileId,
    install: ExtensionInstallId,
    pin: OwnerPackagePinIdentity,
    presence: Arc<LeasePresence>,
    state: PackageReleaseRequestState,
}

/// Opaque retryable active-generation release authority.
///
/// Dropping this request performs no repository read, write, or mutation and
/// never unpins. It must be settled explicitly after native teardown against
/// the same repository open. Reopened repositories require exact lease
/// reacquisition or Store-authorized orphan reconciliation.
///
/// ```compile_fail
/// use zephium_extension_repository::ActiveBundledPackageReleaseRequest;
/// fn require_clone<T: Clone>() {}
/// fn cannot_clone() { require_clone::<ActiveBundledPackageReleaseRequest>(); }
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::ActiveBundledPackageReleaseRequest;
/// fn require_deserialize<T: for<'de> serde::Deserialize<'de>>() {}
/// fn cannot_deserialize() { require_deserialize::<ActiveBundledPackageReleaseRequest>(); }
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::ActiveBundledPackageReleaseRequest;
/// fn forge() -> ActiveBundledPackageReleaseRequest {
///     ActiveBundledPackageReleaseRequest {}
/// }
/// ```
#[must_use = "a pending active package release must settle or remain durable for reconciliation"]
pub struct ActiveBundledPackageReleaseRequest {
    core: PackageReleaseRequestCore,
}

/// Opaque retryable rollback-generation release authority.
///
/// Dropping this request performs no repository read, write, or mutation and
/// never unpins. It must be settled explicitly after native teardown against
/// the same repository open. Reopened repositories require exact lease
/// reacquisition or Store-authorized orphan reconciliation.
///
/// ```compile_fail
/// use zephium_extension_repository::RollbackBundledPackageReleaseRequest;
/// fn require_clone<T: Clone>() {}
/// fn cannot_clone() { require_clone::<RollbackBundledPackageReleaseRequest>(); }
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::RollbackBundledPackageReleaseRequest;
/// fn require_deserialize<T: for<'de> serde::Deserialize<'de>>() {}
/// fn cannot_deserialize() { require_deserialize::<RollbackBundledPackageReleaseRequest>(); }
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::RollbackBundledPackageReleaseRequest;
/// fn forge() -> RollbackBundledPackageReleaseRequest {
///     RollbackBundledPackageReleaseRequest {}
/// }
/// ```
#[must_use = "a pending rollback package release must settle or remain durable for reconciliation"]
pub struct RollbackBundledPackageReleaseRequest {
    core: PackageReleaseRequestCore,
}

impl fmt::Debug for ActiveBundledPackageReleaseRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_release(formatter, "ActiveBundledPackageReleaseRequest", &self.core)
    }
}

impl fmt::Debug for RollbackBundledPackageReleaseRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_release(
            formatter,
            "RollbackBundledPackageReleaseRequest",
            &self.core,
        )
    }
}

fn debug_release(
    formatter: &mut fmt::Formatter<'_>,
    name: &str,
    core: &PackageReleaseRequestCore,
) -> fmt::Result {
    formatter
        .debug_struct(name)
        .field("state", &core.state)
        .finish_non_exhaustive()
}

impl ExtensionRepository {
    /// Returns the freshly verified exact current catalog set and product role.
    pub fn current_bundled_catalog_set(
        &mut self,
    ) -> Result<Option<BundledCurrentCatalogSet>, BundledPackageLeaseError> {
        let operation_gate = Arc::clone(self.package_leases.operation_gate());
        let health = Arc::clone(self.package_leases.health());
        let _operation = operation_gate
            .enter(&health)
            .map_err(map_lease_operation_error)?;
        let projection = current_catalog_set_projection(self.writer_materialization()?)
            .map_err(|error| self.finish_snapshot_error(error))?;
        let Some(current) = projection else {
            return Ok(None);
        };
        let exact_catalog = self.package_lease_read_catalog_object(current.catalog_digest())?;
        let authority = BundledPackageAuthority::product()
            .map_err(BundledPackageLeaseError::CatalogAuthority)?;
        let admitted_anchor = match current.role() {
            VerifiedCatalogRole::Active => authority
                .admit_catalog(&exact_catalog)
                .map_err(|error| {
                    self.writer_seal();
                    BundledPackageLeaseError::CatalogAdmission(error)
                })?
                .generation_anchor(),
            VerifiedCatalogRole::Rollback => authority
                .admit_rollback_catalog(&exact_catalog)
                .map_err(|error| {
                    self.writer_seal();
                    BundledPackageLeaseError::CatalogAdmission(error)
                })?
                .generation_anchor(),
        };
        let expected_anchor = current
            .generation_anchor()
            .map_err(|error| self.finish_snapshot_error(error))?;
        if admitted_anchor != expected_anchor {
            self.writer_seal();
            return Err(BundledPackageLeaseError::DurableObjectMismatch);
        }
        Ok(Some(BundledCurrentCatalogSet {
            identity: current.identity().into(),
            role: match current.role() {
                VerifiedCatalogRole::Active => BundledCatalogGenerationRole::Active,
                VerifiedCatalogRole::Rollback => BundledCatalogGenerationRole::Rollback,
            },
        }))
    }

    /// Freshly admits, verifies, pins, and leases one active-generation package.
    pub fn acquire_active_bundled_package_lease(
        &mut self,
        expected_current: BundledCatalogSetIdentity,
        eligibility: &ExtensionRuntimeEligibility,
    ) -> Result<ActiveBundledPackageLease, BundledPackageLeaseError> {
        let operation_gate = Arc::clone(self.package_leases.operation_gate());
        let health = Arc::clone(self.package_leases.health());
        let _operation = operation_gate
            .enter(&health)
            .map_err(map_lease_operation_error)?;
        let (mut current, fresh) = self.load_fresh_active(expected_current, eligibility)?;
        let plan = self.plan_exact_pin(&current, fresh.record_id(), eligibility)?;
        let (pin, fresh) = match plan {
            OwnerPackagePinPlan::IdempotentReplay { pin } => (pin, fresh),
            OwnerPackagePinPlan::Add { proof, pin } => {
                let runtime = self.writer_take_materialization()?;
                self.finish_transition(add_owner_package_pin(runtime, proof))
                    .map_err(map_transition_finish)?;
                #[cfg(all(
                    test,
                    zephium_internal_repository_e2e,
                    any(target_os = "macos", target_os = "linux")
                ))]
                run_post_pin_reverify_hook();
                let (recovered, reverified) = self
                    .load_fresh_active(expected_current, eligibility)
                    .map_err(|error| self.finish_post_pin_error(error))?;
                match self
                    .plan_exact_pin(&recovered, reverified.record_id(), eligibility)
                    .map_err(|error| self.finish_post_pin_error(error))?
                {
                    OwnerPackagePinPlan::IdempotentReplay { pin: observed } if observed == pin => {}
                    _ => {
                        self.writer_seal();
                        return Err(BundledPackageLeaseError::DurableObjectMismatch);
                    }
                }
                current = recovered;
                (pin, reverified)
            }
        };
        let snapshot = self
            .package_leases
            .share_active(fresh)
            .map_err(map_local_acquire)?;
        let presence = self
            .package_leases
            .reserve(
                current.repository(),
                eligibility.profile(),
                eligibility.install_id(),
            )
            .map_err(map_local_acquire)?;
        Ok(ActiveBundledPackageLease {
            core: PackageLeaseCore {
                open_epoch: Arc::clone(self.package_leases.open_epoch()),
                operation_gate: Arc::clone(self.package_leases.operation_gate()),
                repository: current.repository(),
                current_set: expected_current,
                profile: eligibility.profile(),
                install: eligibility.install_id(),
                pin,
                presence,
                health: Arc::clone(self.package_leases.health()),
                snapshot,
            },
        })
    }

    /// Freshly admits, verifies, pins, and leases one rollback-generation package.
    pub fn acquire_rollback_bundled_package_lease(
        &mut self,
        expected_current: BundledCatalogSetIdentity,
        eligibility: &ExtensionRuntimeEligibility,
    ) -> Result<RollbackBundledPackageLease, BundledPackageLeaseError> {
        let operation_gate = Arc::clone(self.package_leases.operation_gate());
        let health = Arc::clone(self.package_leases.health());
        let _operation = operation_gate
            .enter(&health)
            .map_err(map_lease_operation_error)?;
        let (mut current, fresh) = self.load_fresh_rollback(expected_current, eligibility)?;
        let plan = self.plan_exact_pin(&current, fresh.record_id(), eligibility)?;
        let (pin, fresh) = match plan {
            OwnerPackagePinPlan::IdempotentReplay { pin } => (pin, fresh),
            OwnerPackagePinPlan::Add { proof, pin } => {
                let runtime = self.writer_take_materialization()?;
                self.finish_transition(add_owner_package_pin(runtime, proof))
                    .map_err(map_transition_finish)?;
                #[cfg(all(
                    test,
                    zephium_internal_repository_e2e,
                    any(target_os = "macos", target_os = "linux")
                ))]
                run_post_pin_reverify_hook();
                let (recovered, reverified) = self
                    .load_fresh_rollback(expected_current, eligibility)
                    .map_err(|error| self.finish_post_pin_error(error))?;
                match self
                    .plan_exact_pin(&recovered, reverified.record_id(), eligibility)
                    .map_err(|error| self.finish_post_pin_error(error))?
                {
                    OwnerPackagePinPlan::IdempotentReplay { pin: observed } if observed == pin => {}
                    _ => {
                        self.writer_seal();
                        return Err(BundledPackageLeaseError::DurableObjectMismatch);
                    }
                }
                current = recovered;
                (pin, reverified)
            }
        };
        let snapshot = self
            .package_leases
            .share_rollback(fresh)
            .map_err(map_local_acquire)?;
        let presence = self
            .package_leases
            .reserve(
                current.repository(),
                eligibility.profile(),
                eligibility.install_id(),
            )
            .map_err(map_local_acquire)?;
        Ok(RollbackBundledPackageLease {
            core: PackageLeaseCore {
                open_epoch: Arc::clone(self.package_leases.open_epoch()),
                operation_gate: Arc::clone(self.package_leases.operation_gate()),
                repository: current.repository(),
                current_set: expected_current,
                profile: eligibility.profile(),
                install: eligibility.install_id(),
                pin,
                presence,
                health: Arc::clone(self.package_leases.health()),
                snapshot,
            },
        })
    }

    /// Removes an exact active owner pin after native teardown.
    pub fn release_active_bundled_package_lease(
        &mut self,
        request: &mut ActiveBundledPackageReleaseRequest,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        let operation_gate = Arc::clone(self.package_leases.operation_gate());
        let health = Arc::clone(self.package_leases.health());
        let _operation = operation_gate
            .enter(&health)
            .map_err(map_release_operation_error)?;
        self.release_request(&mut request.core)
    }

    /// Removes an exact rollback owner pin after native teardown.
    pub fn release_rollback_bundled_package_lease(
        &mut self,
        request: &mut RollbackBundledPackageReleaseRequest,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        let operation_gate = Arc::clone(self.package_leases.operation_gate());
        let health = Arc::clone(self.package_leases.health());
        let _operation = operation_gate
            .enter(&health)
            .map_err(map_release_operation_error)?;
        self.release_request(&mut request.core)
    }

    fn load_fresh_active(
        &mut self,
        expected_current: BundledCatalogSetIdentity,
        eligibility: &ExtensionRuntimeEligibility,
    ) -> Result<
        (
            crate::materialization::CurrentCatalogSetProjection,
            VerifiedActivePackageSnapshot,
        ),
        BundledPackageLeaseError,
    > {
        let current = self.require_current(expected_current)?;
        let catalog = self.package_lease_read_catalog_object(current.catalog_digest())?;
        let fresh = load_active_package_snapshot(
            self.writer_materialization()?,
            &current,
            &catalog,
            eligibility,
        )
        .map_err(|error| self.finish_snapshot_error(error))?;
        Ok((current, fresh))
    }

    fn load_fresh_rollback(
        &mut self,
        expected_current: BundledCatalogSetIdentity,
        eligibility: &ExtensionRuntimeEligibility,
    ) -> Result<
        (
            crate::materialization::CurrentCatalogSetProjection,
            VerifiedRollbackPackageSnapshot,
        ),
        BundledPackageLeaseError,
    > {
        let current = self.require_current(expected_current)?;
        let catalog = self.package_lease_read_catalog_object(current.catalog_digest())?;
        let fresh = load_rollback_package_snapshot(
            self.writer_materialization()?,
            &current,
            &catalog,
            eligibility,
        )
        .map_err(|error| self.finish_snapshot_error(error))?;
        Ok((current, fresh))
    }

    fn require_current(
        &mut self,
        expected: BundledCatalogSetIdentity,
    ) -> Result<crate::materialization::CurrentCatalogSetProjection, BundledPackageLeaseError> {
        let current = current_catalog_set_projection(self.writer_materialization()?)
            .map_err(|error| self.finish_snapshot_error(error))?
            .ok_or(BundledPackageLeaseError::NoCurrentSelection)?;
        if current.identity().bytes() != expected.bytes() {
            return Err(BundledPackageLeaseError::StaleSelection);
        }
        if current.build_in_progress() {
            return Err(BundledPackageLeaseError::BuildInProgress);
        }
        Ok(current)
    }

    fn plan_exact_pin(
        &mut self,
        current: &crate::materialization::CurrentCatalogSetProjection,
        record_id: Digest32,
        eligibility: &ExtensionRuntimeEligibility,
    ) -> Result<OwnerPackagePinPlan, BundledPackageLeaseError> {
        let runtime = self.writer_materialization()?;
        let owner = (eligibility.profile(), eligibility.install_id());
        if let Ok(index) = runtime
            ._state
            .package_pins
            .binary_search_by_key(&owner, |pin| (pin.profile_id, pin.install_id))
        {
            if runtime._state.package_pins[index].package_record_id != record_id {
                return Err(BundledPackageLeaseError::OwnerConflict);
            }
        }
        let planned = plan_current_catalog_package_pin(
            runtime,
            current.identity(),
            eligibility.package().key(),
            eligibility.profile(),
            eligibility.install_id(),
        );
        match planned {
            Ok(plan) => Ok(plan),
            Err(error) => Err(self.finish_lease_planning_error(error)),
        }
    }

    fn release_request(
        &mut self,
        request: &mut PackageReleaseRequestCore,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        if !Arc::ptr_eq(self.package_leases.open_epoch(), &request.open_epoch)
            || !Arc::ptr_eq(&request.presence.open_epoch, &request.open_epoch)
        {
            return Err(BundledPackageLeaseReleaseError::WrongRepository);
        }
        if request.state == PackageReleaseRequestState::Released {
            return Ok(BundledPackageLeaseReleaseOutcome::AlreadyReleased);
        }
        let build_in_progress = {
            let runtime = self.writer_materialization()?;
            validated_resumable_build_in_progress(runtime)
        };
        let build_in_progress = match build_in_progress {
            Ok(build_in_progress) => build_in_progress,
            Err(error) => return Err(self.finish_release_snapshot_error(error)),
        };
        if build_in_progress {
            return Err(BundledPackageLeaseReleaseError::BuildInProgress);
        }
        let repository = {
            let runtime = self.writer_materialization()?;
            PackageLeaseRepositoryIdentity {
                root: runtime._root.identity(),
                records: runtime._records.identity(),
                trees: runtime._trees.identity(),
            }
        };
        if repository != request.repository || request.presence.repository != request.repository {
            return Err(BundledPackageLeaseReleaseError::WrongRepository);
        }
        self.package_leases
            .install_release_presence(&request.presence)
            .map_err(|error| match error {
                LocalLeaseError::ConcurrentLease => {
                    BundledPackageLeaseReleaseError::ConcurrentLease
                }
                _ => BundledPackageLeaseReleaseError::Repository(
                    ExtensionRepositoryError::RecoveryAmbiguous,
                ),
            })?;

        let plan = {
            let runtime = self.writer_materialization()?;
            let planned = plan_owner_package_pin_removal(
                runtime,
                request.profile,
                request.install,
                request.pin,
            );
            match planned {
                Ok(plan) => plan,
                Err(error) => return Err(self.finish_release_planning_error(error)),
            }
        };
        #[cfg(all(
            test,
            zephium_internal_repository_e2e,
            any(target_os = "macos", target_os = "linux")
        ))]
        if let Some(error) = take_release_planning_error_hook() {
            return Err(self.finish_release_planning_error(error));
        }

        let outcome = match plan {
            OwnerPackagePinRemovalPlan::IdempotentReplay => {
                BundledPackageLeaseReleaseOutcome::AlreadyReleased
            }
            OwnerPackagePinRemovalPlan::Stale => {
                request.state = PackageReleaseRequestState::Released;
                self.package_leases.retire_presence(&request.presence);
                return Err(BundledPackageLeaseReleaseError::StaleLease);
            }
            OwnerPackagePinRemovalPlan::Remove(proof) => {
                let runtime = self.writer_take_materialization()?;
                self.finish_transition(remove_owner_package_pin(runtime, proof))
                    .map_err(map_release_finish)?;
                BundledPackageLeaseReleaseOutcome::Released
            }
        };
        request.state = PackageReleaseRequestState::Released;
        self.package_leases.retire_presence(&request.presence);
        Ok(outcome)
    }

    fn finish_snapshot_error(&mut self, error: SnapshotLoadError) -> BundledPackageLeaseError {
        let poison = snapshot_error_requires_poison(&error);
        let mapped = map_snapshot_error(error);
        if poison {
            self.writer_seal();
        }
        mapped
    }

    fn finish_release_snapshot_error(
        &mut self,
        error: SnapshotLoadError,
    ) -> BundledPackageLeaseReleaseError {
        let poison = snapshot_error_requires_poison(&error);
        let mapped = match map_snapshot_error(error) {
            BundledPackageLeaseError::Repository(error) => error,
            _ => ExtensionRepositoryError::RecoveryAmbiguous,
        };
        if poison {
            self.writer_seal();
        }
        BundledPackageLeaseReleaseError::Repository(mapped)
    }

    fn finish_lease_planning_error(
        &mut self,
        error: MaterializationTransitionError,
    ) -> BundledPackageLeaseError {
        match error {
            MaterializationTransitionError::Clean(error) => {
                match self.writer_recover_materialization() {
                    Ok(()) => BundledPackageLeaseError::Repository(error),
                    Err(recovery) => {
                        self.writer_seal();
                        BundledPackageLeaseError::Repository(recovery)
                    }
                }
            }
            MaterializationTransitionError::MustSeal(error) => {
                self.writer_seal();
                BundledPackageLeaseError::Repository(error)
            }
        }
    }

    fn finish_release_planning_error(
        &mut self,
        error: MaterializationTransitionError,
    ) -> BundledPackageLeaseReleaseError {
        match error {
            MaterializationTransitionError::Clean(error) => {
                match self.writer_recover_materialization() {
                    Ok(()) => BundledPackageLeaseReleaseError::Repository(error),
                    Err(recovery) => {
                        self.writer_seal();
                        BundledPackageLeaseReleaseError::Repository(recovery)
                    }
                }
            }
            MaterializationTransitionError::MustSeal(error) => {
                self.writer_seal();
                BundledPackageLeaseReleaseError::Repository(error)
            }
        }
    }

    fn finish_post_pin_error(
        &mut self,
        error: BundledPackageLeaseError,
    ) -> BundledPackageLeaseError {
        if post_pin_error_requires_poison(&error) {
            self.writer_seal();
        }
        error
    }
}

fn snapshot_error_requires_poison(error: &SnapshotLoadError) -> bool {
    match error {
        SnapshotLoadError::DurableMismatch
        | SnapshotLoadError::Preparation(_)
        | SnapshotLoadError::CatalogAdmission(_)
        | SnapshotLoadError::ManifestAdmission(_)
        | SnapshotLoadError::PackageNotMaterialized => true,
        SnapshotLoadError::Object { phase, error } => {
            snapshot_object_error_requires_poison(*phase, *error)
        }
        SnapshotLoadError::Repository(error) => matches!(
            error,
            ExtensionRepositoryError::StateCorrupt
                | ExtensionRepositoryError::RecoveryAmbiguous
                | ExtensionRepositoryError::SettlementAmbiguous
                | ExtensionRepositoryError::FileSystem(
                    zephium_private_fs::PrivateFsError::NotFound
                        | zephium_private_fs::PrivateFsError::ReservedComponent
                        | zephium_private_fs::PrivateFsError::Unsafe
                        | zephium_private_fs::PrivateFsError::BoundExceeded
                        | zephium_private_fs::PrivateFsError::AlreadyExists
                        | zephium_private_fs::PrivateFsError::NamespaceMismatch
                        | zephium_private_fs::PrivateFsError::DirectoryNotEmpty
                        | zephium_private_fs::PrivateFsError::IdentityAmbiguous
                        | zephium_private_fs::PrivateFsError::SettlementUnknown
                        | zephium_private_fs::PrivateFsError::Quarantined
                )
        ),
        SnapshotLoadError::CatalogAuthority(_)
        | SnapshotLoadError::ManifestAuthority(_)
        | SnapshotLoadError::StaleSelection
        | SnapshotLoadError::PackageNotSelected
        | SnapshotLoadError::WrongRole
        | SnapshotLoadError::EligibilityMismatch
        | SnapshotLoadError::AccountingOverflow => false,
    }
}

const fn snapshot_object_error_requires_poison(
    phase: SnapshotObjectPhase,
    error: PackageObjectError,
) -> bool {
    match phase {
        SnapshotObjectPhase::Preflight { had_intent } => {
            preflight_error_requires_sealing(error, had_intent)
        }
        SnapshotObjectPhase::Completed => completed_error_requires_sealing(error),
    }
}

fn post_pin_error_requires_poison(error: &BundledPackageLeaseError) -> bool {
    !matches!(
        error,
        BundledPackageLeaseError::CatalogAuthority(_)
            | BundledPackageLeaseError::ManifestAuthority(_)
            | BundledPackageLeaseError::Repository(ExtensionRepositoryError::FileSystem(
                zephium_private_fs::PrivateFsError::LockUnavailable
                    | zephium_private_fs::PrivateFsError::InUse
                    | zephium_private_fs::PrivateFsError::PrimitiveUnavailable
                    | zephium_private_fs::PrivateFsError::Io
            ))
    )
}

impl From<ExtensionRepositoryError> for BundledPackageLeaseError {
    fn from(error: ExtensionRepositoryError) -> Self {
        Self::Repository(error)
    }
}

impl From<ExtensionRepositoryError> for BundledPackageLeaseReleaseError {
    fn from(error: ExtensionRepositoryError) -> Self {
        Self::Repository(error)
    }
}

fn map_snapshot_error(error: SnapshotLoadError) -> BundledPackageLeaseError {
    match error {
        SnapshotLoadError::Repository(error) => BundledPackageLeaseError::Repository(error),
        SnapshotLoadError::CatalogAuthority(error) => {
            BundledPackageLeaseError::CatalogAuthority(error)
        }
        SnapshotLoadError::CatalogAdmission(error) => {
            BundledPackageLeaseError::CatalogAdmission(error)
        }
        SnapshotLoadError::ManifestAuthority(error) => {
            BundledPackageLeaseError::ManifestAuthority(error)
        }
        SnapshotLoadError::ManifestAdmission(error) => {
            BundledPackageLeaseError::ManifestAdmission(error)
        }
        SnapshotLoadError::StaleSelection => BundledPackageLeaseError::StaleSelection,
        SnapshotLoadError::PackageNotSelected => BundledPackageLeaseError::PackageNotSelected,
        SnapshotLoadError::WrongRole => BundledPackageLeaseError::WrongCatalogRole,
        SnapshotLoadError::EligibilityMismatch => BundledPackageLeaseError::EligibilityMismatch,
        SnapshotLoadError::PackageNotMaterialized => {
            BundledPackageLeaseError::PackageNotMaterialized
        }
        SnapshotLoadError::AccountingOverflow => BundledPackageLeaseError::CapacityExhausted,
        SnapshotLoadError::Object { error, .. } => map_snapshot_object_error(error),
        SnapshotLoadError::Preparation(_) | SnapshotLoadError::DurableMismatch => {
            BundledPackageLeaseError::DurableObjectMismatch
        }
    }
}

fn map_snapshot_object_error(error: PackageObjectError) -> BundledPackageLeaseError {
    match error {
        PackageObjectError::BuildStateMismatch => {
            BundledPackageLeaseError::Repository(ExtensionRepositoryError::RecoveryAmbiguous)
        }
        PackageObjectError::CapacityExhausted => BundledPackageLeaseError::CapacityExhausted,
        PackageObjectError::GenerationExhausted => {
            BundledPackageLeaseError::Repository(ExtensionRepositoryError::GenerationExhausted)
        }
        PackageObjectError::Collision | PackageObjectError::ExactMismatch => {
            BundledPackageLeaseError::DurableObjectMismatch
        }
        PackageObjectError::Source(_) => BundledPackageLeaseError::DurableObjectMismatch,
        PackageObjectError::Filesystem(error) => {
            BundledPackageLeaseError::Repository(ExtensionRepositoryError::FileSystem(error))
        }
        PackageObjectError::SettlementAmbiguous => {
            BundledPackageLeaseError::Repository(ExtensionRepositoryError::SettlementAmbiguous)
        }
    }
}

fn map_local_acquire(error: LocalLeaseError) -> BundledPackageLeaseError {
    match error {
        LocalLeaseError::AlreadyOpen => BundledPackageLeaseError::LeaseAlreadyOpen,
        LocalLeaseError::CapacityExhausted => BundledPackageLeaseError::CapacityExhausted,
        LocalLeaseError::SnapshotMismatch => BundledPackageLeaseError::DurableObjectMismatch,
        LocalLeaseError::ConcurrentLease => BundledPackageLeaseError::LeaseAlreadyOpen,
    }
}

fn map_lease_operation_error(error: RepositoryOperationError) -> BundledPackageLeaseError {
    BundledPackageLeaseError::Repository(error.repository_error())
}

fn map_release_operation_error(error: RepositoryOperationError) -> BundledPackageLeaseReleaseError {
    BundledPackageLeaseReleaseError::Repository(error.repository_error())
}

fn map_transition_finish(error: BundledPackageMaterializationError) -> BundledPackageLeaseError {
    match error {
        BundledPackageMaterializationError::Repository(error) => {
            BundledPackageLeaseError::Repository(error)
        }
        _ => BundledPackageLeaseError::DurableObjectMismatch,
    }
}

fn map_release_finish(
    error: BundledPackageMaterializationError,
) -> BundledPackageLeaseReleaseError {
    match error {
        BundledPackageMaterializationError::Repository(error) => {
            BundledPackageLeaseReleaseError::Repository(error)
        }
        _ => {
            BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::RecoveryAmbiguous)
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
mod repository_e2e;
