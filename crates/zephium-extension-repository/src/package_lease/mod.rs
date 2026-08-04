//! Opaque authenticated package leases and exact owner-pin release authority.

mod api;
mod policy;
mod repository;
mod resource;
mod runtime;

pub use api::{
    ActiveBundledPackageLease, ActiveBundledPackageReleaseRequest, BundledCatalogGenerationRole,
    BundledCurrentCatalogSet, BundledPackageLeaseError, BundledPackageLeaseReleaseError,
    BundledPackageLeaseReleaseOutcome, BundledPackageResourceError, RollbackBundledPackageLease,
    RollbackBundledPackageReleaseRequest,
};
pub(crate) use runtime::PackageLeaseRuntime;

#[cfg(test)]
mod tests;

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
mod repository_e2e;
