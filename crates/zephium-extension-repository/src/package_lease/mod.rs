//! Opaque authenticated package leases and exact owner-pin release authority.

mod api;
mod manifest_bindings;
mod policy;
mod repository;
mod resource;
mod runtime;
mod runtime_access;

pub use api::{
    ActiveBundledPackageLease, ActiveBundledPackageReleaseRequest, BundledCatalogGenerationRole,
    BundledCurrentCatalogSet, BundledPackageLeaseError, BundledPackageLeaseReleaseError,
    BundledPackageLeaseReleaseOutcome, BundledPackageResourceError, RollbackBundledPackageLease,
    RollbackBundledPackageReleaseRequest,
};
pub use manifest_bindings::{BundledCurrentManifestBindings, BundledManifestBindingsError};
pub(crate) use runtime::PackageLeaseRuntime;
pub use runtime_access::{
    ActiveBundledRuntimePackageAccessBuildRefusal, ActiveBundledRuntimePackageRecoveryError,
    ActiveBundledRuntimePackageRecoveryRefusal, BundledRuntimePackageAccessBuildError,
    RollbackBundledRuntimePackageAccessBuildRefusal, RollbackBundledRuntimePackageRecoveryError,
    RollbackBundledRuntimePackageRecoveryRefusal,
};

#[cfg(test)]
mod tests;

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
mod repository_e2e;
