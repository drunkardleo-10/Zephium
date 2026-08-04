//! Durable metadata and opaque sealed-root recovery for materialized packages.
//!
//! This module owns the public path-free byte-source contract and the private
//! exact object/transition machinery consumed by the repository writer. It has
//! no native-activation operation, garbage collector, receipt constructor,
//! profile grant, or native-runtime mutation. The crate's package-lease layer
//! composes these private snapshots into authenticated package access and
//! durable owner-pin authority only. Opening a repository recovers only bounded
//! metadata and live sealed tree-root identities.

mod catalog_set;
mod cleanup;
mod interlock;
mod names;
mod objects;
mod package_lease;
mod policy;
mod prepare;
mod records;
mod recovery;
mod runtime;
mod source;
mod state;
mod storage;
mod transaction;
mod tree_reader;
mod tree_writer;

pub(crate) use catalog_set::{
    derive_active_catalog_set, derive_rollback_catalog_set, VerifiedActiveCatalogSet,
    VerifiedRollbackCatalogSet,
};
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) use cleanup::{
    install_orphan_package_record_stage_for_e2e, install_resumable_package_record_stage_for_e2e,
};
pub(crate) use cleanup::{reconcile_build_stages_for_abort, CleanupError};
pub(crate) use interlock::validate_catalog_advance;
pub(crate) use objects::{
    preflight_package_object_capacity, publish_or_reuse_active_package,
    publish_or_reuse_rollback_package, verify_completed_active_package,
    verify_completed_rollback_package, PackageObjectError, PackageObjectIntentDisposition,
};
pub(crate) use package_lease::{
    current_catalog_set_projection, load_active_package_snapshot, load_rollback_package_snapshot,
    validated_resumable_build_in_progress, CurrentCatalogSetProjection,
    PackageLeaseRepositoryIdentity, SnapshotLoadError, SnapshotObjectPhase,
    VerifiedActivePackageSnapshot, VerifiedCatalogRole, VerifiedRollbackPackageSnapshot,
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
    complete_active_package, complete_rollback_package, promote_active_catalog_set,
    promote_rollback_catalog_set, rollback_to_previous_catalog_set,
    stage_active_catalog_set_candidate, stage_rollback_catalog_set_candidate,
    MaterializationTransitionError,
};
pub(crate) use tree_reader::{with_verified_tree_resource, TreeResourceError};
// Kept crate-private so durable owner retention is reachable only through the
// package-lease layer's authenticated package access and pinning authority.
// Native activation still requires a service-owned join with separate native
// runtime authority; no value in this module grants it.
#[allow(unused_imports)]
pub(crate) use transaction::{
    add_owner_package_pin, plan_current_catalog_package_pin, plan_owner_package_pin_removal,
    remove_owner_package_pin, CurrentCatalogPackagePinProof, OwnerPackagePinIdentity,
    OwnerPackagePinPlan, OwnerPackagePinRemovalPlan, OwnerPackagePinRemovalProof,
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
pub(crate) use transaction::stage_active_catalog_set_candidate_at_fault;
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) use transaction::{
    add_owner_package_pin_at_fault, promote_active_catalog_set_at_fault,
    remove_owner_package_pin_at_fault, rollback_to_previous_catalog_set_at_fault,
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
