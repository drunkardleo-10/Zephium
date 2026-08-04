//! Stable path-free package-lease values and their local access behavior.

use std::fmt;
use std::io::Read;
use std::mem::size_of;
use std::sync::Arc;

use thiserror::Error;
use zephium_core::extensions::{ExtensionManifestDescriptor, ExtensionPackageIdentity};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_extension_authority::{
    BundledCatalogAdmissionError, ProductExtensionManifestAdmissionError,
    ProductExtensionManifestAuthorityError,
};
use zephium_extension_package::{ExtensionReleaseCatalogRevision, PortableRelativePath};

use super::resource;
use super::runtime::{LeasePresence, RepositoryOpenEpoch};
use crate::materialization::{
    OwnerPackagePinIdentity, PackageLeaseRepositoryIdentity, VerifiedActivePackageSnapshot,
    VerifiedRollbackPackageSnapshot,
};
use crate::operation::{RepositoryOperationError, RepositoryRuntime};
use crate::{BundledCatalogSetIdentity, ExtensionRepositoryError};

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
    pub(super) identity: BundledCatalogSetIdentity,
    pub(super) role: BundledCatalogGenerationRole,
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
    /// A package callback attempted to enter a repository or another lease.
    #[error("extension package callbacks cannot enter repository operations")]
    CallbackReentry,
}

pub(super) struct PackageLeaseCore<Snapshot> {
    pub(super) open_epoch: Arc<RepositoryOpenEpoch>,
    pub(super) runtime: RepositoryRuntime,
    pub(super) repository: PackageLeaseRepositoryIdentity,
    pub(super) current_set: BundledCatalogSetIdentity,
    pub(super) profile: ProfileId,
    pub(super) install: ExtensionInstallId,
    pub(super) pin: OwnerPackagePinIdentity,
    pub(super) presence: Arc<LeasePresence>,
    pub(super) snapshot: Arc<Snapshot>,
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
    pub(super) core: PackageLeaseCore<VerifiedActivePackageSnapshot>,
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
    pub(super) core: PackageLeaseCore<VerifiedRollbackPackageSnapshot>,
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
            /// operation locks. Re-entering any repository or package lease on this
            /// thread is rejected before lock acquisition; the callback must not
            /// delegate repository access to another thread. Integrity verification
            /// drains and hashes the remainder even when the callback returns `Err(E)`.
            pub fn with_resource_reader<T, E>(
                &self,
                path: &PortableRelativePath,
                callback: impl FnOnce(&mut dyn Read) -> Result<T, E>,
            ) -> Result<Result<T, E>, BundledPackageResourceError> {
                let _operation = self.core.runtime.enter().map_err(|error| match error {
                    RepositoryOperationError::CallbackReentry => {
                        BundledPackageResourceError::CallbackReentry
                    }
                    RepositoryOperationError::Unhealthy | RepositoryOperationError::Poisoned => {
                        BundledPackageResourceError::LeaseInactive
                    }
                })?;
                resource::with_resource(
                    &self.core.runtime,
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
                    runtime: _,
                    repository,
                    current_set: _,
                    profile,
                    install,
                    pin,
                    presence,
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
pub(super) enum PackageReleaseRequestState {
    ReleasePending,
    Released,
}

pub(super) struct PackageReleaseRequestCore {
    pub(super) open_epoch: Arc<RepositoryOpenEpoch>,
    pub(super) repository: PackageLeaseRepositoryIdentity,
    pub(super) profile: ProfileId,
    pub(super) install: ExtensionInstallId,
    pub(super) pin: OwnerPackagePinIdentity,
    pub(super) presence: Arc<LeasePresence>,
    pub(super) state: PackageReleaseRequestState,
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
    pub(super) core: PackageReleaseRequestCore,
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
    pub(super) core: PackageReleaseRequestCore,
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
