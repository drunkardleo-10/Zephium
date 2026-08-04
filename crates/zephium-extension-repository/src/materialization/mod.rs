//! Durable metadata and opaque sealed-root recovery for materialized packages.
//!
//! This module owns the public path-free byte-source contract and the private
//! exact object/transition machinery consumed by the repository writer. It has
//! no activation operation, garbage collector, receipt constructor, profile
//! grant, runtime lease, or native-runtime mutation. Opening a repository
//! recovers only bounded metadata and live sealed tree-root identities.

mod cleanup;
mod interlock;
mod names;
mod objects;
mod policy;
mod prepare;
mod records;
mod recovery;
mod runtime;
mod source;
mod state;
mod storage;
mod transaction;
mod tree_writer;

pub(crate) use cleanup::{reconcile_build_stages_for_abort, CleanupError};
pub(crate) use interlock::validate_catalog_advance;
pub(crate) use objects::{
    preflight_package_object_capacity, publish_or_reuse_active_package,
    publish_or_reuse_rollback_package, verify_completed_active_package,
    verify_completed_rollback_package, PackageObjectError, PackageObjectIntentDisposition,
};
pub(crate) use prepare::{
    open_product_manifest_authority, prepare_active_package, prepare_rollback_package,
    PreparationError, PreparedActivePackage, PreparedRollbackPackage,
};
pub(crate) use records::{
    MAX_CATALOG_SET_PACKAGES, MAX_CATALOG_SET_RECORD_BYTES, MAX_PACKAGE_RECORD_BYTES,
};
pub(crate) use recovery::{is_pristine_for_outer_initialization, open_or_recover, FaultPoint};
pub(crate) use runtime::MaterializationRuntime;
pub use source::{
    BundledReleaseByteSource, BundledReleaseCatalogSourceIdentity,
    BundledReleasePackageSourceIdentity, BundledReleaseResource, BundledReleaseResourceKind,
    BundledReleaseSourceError,
};
pub(crate) use state::{
    MAX_COMPLETED_PACKAGE_RECORDS, MAX_DURABLE_PACKAGE_PINS, MAX_MATERIALIZATION_CHECKPOINT_BYTES,
    MAX_MATERIALIZATION_JOURNAL_BYTES, MAX_MATERIALIZATION_STATE_BYTES,
};
pub(crate) use transaction::{
    abort_package_build, begin_active_package_build, begin_rollback_package_build,
    complete_active_package, complete_rollback_package, MaterializationTransitionError,
};

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) use objects::{
    publish_or_reuse_active_package_at_fault, ObjectPublicationFaultPoint,
    VerifiedActivePackageClosure,
};
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) use transaction::{complete_active_package_with_fault, TransitionFaultPoint};

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
pub(crate) use records::tests::package_record_fixture;
#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
pub(crate) use state::{MaterializationBuildIntent, MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION};
