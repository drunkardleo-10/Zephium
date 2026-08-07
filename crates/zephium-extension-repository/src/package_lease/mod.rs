//! Opaque authenticated package leases and exact owner-pin release authority.

mod acquisition_plan;
mod api;
mod manifest_bindings;
mod policy;
mod profile_audit;
mod repository;
mod resource;
mod runtime;
mod runtime_access;

pub use acquisition_plan::{
    BundledRuntimeAcquisitionError, BundledRuntimeAcquisitionPlan,
    BundledRuntimeAcquisitionPlanRefusal, BundledRuntimeAcquisitionPlanRefusalReason,
    BundledRuntimeAcquisitionPlanningRefusal, MAX_BUNDLED_RUNTIME_ACQUISITION_PLAN_RETAINED_BYTES,
};
pub use api::{
    ActiveBundledPackageLease, ActiveBundledPackageReleaseRequest, BundledCatalogGenerationRole,
    BundledCurrentCatalogSet, BundledPackageLease, BundledPackageLeaseError,
    BundledPackageLeaseReleaseError, BundledPackageLeaseReleaseOutcome,
    BundledPackageResourceError, RollbackBundledPackageLease, RollbackBundledPackageReleaseRequest,
};
pub use manifest_bindings::{BundledCurrentManifestBindings, BundledManifestBindingsError};
pub use profile_audit::{
    ProfilePackageAbsenceEvidence, ProfilePackageAbsenceRevalidationError,
    ProfilePackageObligation, ProfilePackageObligationKind,
};
pub(crate) use runtime::PackageLeaseRuntime;
pub use runtime_access::{
    ActiveBundledRuntimeHostActivation, ActiveBundledRuntimeHostActivationBindingRefusal,
    ActiveBundledRuntimePackageAccess, ActiveBundledRuntimePackageAccessBuildRefusal,
    ActiveBundledRuntimePackageRecoveryError, ActiveBundledRuntimePackageRecoveryRefusal,
    ActiveBundledRuntimePackageRecoveryToken, ActiveBundledRuntimePackageRejoinRefusal,
    BundledRuntimeHostActivationBindingError, BundledRuntimePackageAccessBuildError,
    RollbackBundledRuntimeHostActivation, RollbackBundledRuntimeHostActivationBindingRefusal,
    RollbackBundledRuntimePackageAccess, RollbackBundledRuntimePackageAccessBuildRefusal,
    RollbackBundledRuntimePackageRecoveryError, RollbackBundledRuntimePackageRecoveryRefusal,
    RollbackBundledRuntimePackageRecoveryToken, RollbackBundledRuntimePackageRejoinRefusal,
};

#[cfg(test)]
mod tests;

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
mod repository_e2e;
