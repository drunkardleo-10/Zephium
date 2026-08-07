//! Crash-durable authority and materialization metadata for extensions.
//!
//! This crate owns the private repository namespace, monotonic catalog and
//! package-line high-water marks, exact bounded package objects, materialization
//! records, and live opaque sealed-tree identities. Materialization deliberately
//! grants no profile authority, exposes no filesystem path, issues no activation
//! receipt, and mutates no native extension runtime. Recording an authenticated
//! catalog proves only that an equal or older catalog can no longer be accepted
//! in this authority epoch. Package leases authorize exact resource access and
//! durable owner pinning only; native activation requires a service-owned join
//! with separate native-runtime authority.

#![deny(missing_docs)]
#![deny(unsafe_code)]

#[cfg(all(zephium_internal_repository_e2e, not(debug_assertions)))]
compile_error!("the internal repository E2E authority is forbidden in optimized builds");

mod admission;
mod catalog_cache;
mod catalog_selection;
mod codec;
mod error;
mod garbage_collection;
mod materialization;
mod names;
mod operation;
mod package_lease;
mod recovery;
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
#[path = "../../zephium-extension-authority/src/repository_e2e_fixture.rs"]
mod repository_e2e_fixture;
mod settlement;
mod state;
mod storage;
mod writer;

pub use admission::{BundledCatalogRecordOutcome, ExtensionRepository};
pub use catalog_selection::{
    BundledCatalogSetError, BundledCatalogSetIdentity, BundledCatalogSetPromotionOutcome,
    BundledCatalogSetRollbackOutcome, BundledCatalogSetStageOutcome,
    BundledPackageRuntimeSelection,
};
pub use error::ExtensionRepositoryError;
pub use garbage_collection::BundledPackageGarbageCollectionOutcome;
pub use materialization::{
    BundledReleaseByteSource, BundledReleaseCatalogSourceIdentity,
    BundledReleasePackageSourceIdentity, BundledReleaseResource, BundledReleaseResourceKind,
    BundledReleaseSourceError,
};
pub use package_lease::{
    ActiveBundledPackageLease, ActiveBundledPackageReleaseRequest,
    ActiveBundledRuntimeHostActivation, ActiveBundledRuntimeHostActivationBindingRefusal,
    ActiveBundledRuntimePackageAccess, ActiveBundledRuntimePackageAccessBuildRefusal,
    ActiveBundledRuntimePackageRecoveryError, ActiveBundledRuntimePackageRecoveryRefusal,
    ActiveBundledRuntimePackageRecoveryToken, ActiveBundledRuntimePackageRejoinRefusal,
    BundledCatalogGenerationRole, BundledCurrentCatalogSet, BundledCurrentManifestBindings,
    BundledManifestBindingsError, BundledPackageLease, BundledPackageLeaseError,
    BundledPackageLeaseReleaseError, BundledPackageLeaseReleaseOutcome,
    BundledPackageResourceError, BundledRuntimeAcquisitionError, BundledRuntimeAcquisitionPlan,
    BundledRuntimeAcquisitionPlanRefusal, BundledRuntimeAcquisitionPlanRefusalReason,
    BundledRuntimeAcquisitionPlanningRefusal, BundledRuntimeHostActivationBindingError,
    BundledRuntimePackageAccessBuildError, ProfilePackageAbsenceEvidence,
    ProfilePackageAbsenceRevalidationError, ProfilePackageObligation, ProfilePackageObligationKind,
    RollbackBundledPackageLease, RollbackBundledPackageReleaseRequest,
    RollbackBundledRuntimeHostActivation, RollbackBundledRuntimeHostActivationBindingRefusal,
    RollbackBundledRuntimePackageAccess, RollbackBundledRuntimePackageAccessBuildRefusal,
    RollbackBundledRuntimePackageRecoveryError, RollbackBundledRuntimePackageRecoveryRefusal,
    RollbackBundledRuntimePackageRecoveryToken, RollbackBundledRuntimePackageRejoinRefusal,
    MAX_BUNDLED_RUNTIME_ACQUISITION_PLAN_RETAINED_BYTES,
    MAX_BUNDLED_RUNTIME_PRE_HOST_REFUSAL_ADDITIONAL_RETAINED_BYTES,
};
pub use settlement::{BundledPackageBuildSettlementError, BundledPackageBuildSettlementOutcome};
pub use writer::{BundledPackageMaterializationError, BundledPackageMaterializationOutcome};
