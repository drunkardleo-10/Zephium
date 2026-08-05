//! Durable metadata and opaque sealed-root recovery for materialized packages.
//!
//! This module owns the public path-free byte-source contract and the private
//! exact object/transition machinery consumed by the repository writer. It
//! supplies the bounded GC plan, proof, transition, and tree-removal
//! primitives; the root repository module owns their serialized orchestration.
//! It has no native-activation operation, receipt constructor, profile grant,
//! or native-runtime mutation. The crate's package-lease layer
//! composes these private snapshots into authenticated package access and
//! durable owner-pin authority only. Opening a repository recovers only bounded
//! metadata and live sealed tree-root identities.

mod catalog_set;
mod cleanup;
mod gc;
mod interlock;
mod names;
mod objects;
mod package_lease;
mod policy;
mod prepare;
mod records;
mod recovery;
mod runtime;
mod settlement;
mod source;
mod state;
mod storage;
mod transaction;
mod tree_cleanup;
mod tree_reader;
mod tree_writer;

pub(crate) use catalog_set::{
    derive_active_catalog_set, derive_rollback_catalog_set, VerifiedActiveCatalogSet,
    VerifiedRollbackCatalogSet,
};
pub(crate) use cleanup::{
    inspect_package_build_commit_marker, reconcile_build_stages_for_abort, CleanupError,
    PackageBuildCommitMarker,
};
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) use cleanup::{
    install_orphan_package_record_stage_for_e2e, install_resumable_package_record_stage_for_e2e,
};
pub(crate) use gc::{plan_garbage_collection, prove_garbage_collection_absence};
pub(crate) use interlock::validate_catalog_advance;
pub(crate) use names::{
    catalog_set_record as gc_catalog_set_record, legal_object as gc_legal_object,
    package_record as gc_package_record, tree_index_object as gc_tree_index_object,
    tree_object as gc_tree_object, tree_retired as gc_tree_retired,
};
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) use objects::publish_intent_package_record_marker_for_e2e;
pub(crate) use objects::{
    preflight_package_object_capacity, publish_or_reuse_active_package,
    publish_or_reuse_rollback_package, verify_completed_active_package,
    verify_completed_rollback_package, PackageObjectError, PackageObjectIntentDisposition,
};
pub(crate) use package_lease::{
    current_catalog_set_projection, load_active_manifest_bindings,
    load_active_package_pin_admission, load_rollback_manifest_bindings,
    load_rollback_package_pin_admission, validated_resumable_build_in_progress,
    CurrentCatalogSetProjection, PackageLeaseRepositoryIdentity, PackagePinAdmissionError,
    PackagePinLoadError, SnapshotLoadError, SnapshotObjectPhase, VerifiedActivePackageSnapshot,
    VerifiedCatalogRole, VerifiedPackagePinAdmission, VerifiedRollbackPackageSnapshot,
};
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) use package_lease::{repository_package_io_count, reset_repository_package_io_count};
pub(crate) use prepare::{
    open_product_manifest_authority, prepare_active_package, prepare_rollback_package,
    PreparationError, PreparedActivePackage, PreparedRollbackPackage,
};
pub(crate) use records::{
    CatalogAnchor, MAX_CATALOG_SET_PACKAGES, MAX_CATALOG_SET_RECORD_BYTES, MAX_PACKAGE_RECORD_BYTES,
};
pub(crate) use recovery::{is_pristine_for_outer_initialization, open_or_recover, FaultPoint};
pub(crate) use runtime::MaterializationRuntime;
pub(crate) use settlement::{
    authenticate_interrupted_package, InterruptedPackageAuthenticationError,
    VerifiedInterruptedPackageClosure,
};
pub use source::{
    BundledReleaseByteSource, BundledReleaseCatalogSourceIdentity,
    BundledReleasePackageSourceIdentity, BundledReleaseResource, BundledReleaseResourceKind,
    BundledReleaseSourceError,
};
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) use state::MATERIALIZATION_GC_INTENT_SCHEMA_VERSION;
pub(crate) use state::{
    MaterializationGarbageCollectionIntent, MAX_COMPLETED_PACKAGE_RECORDS,
    MAX_DURABLE_PACKAGE_PINS, MAX_GC_CATALOG_OBJECT_TARGETS, MAX_GC_CATALOG_SET_TARGETS,
    MAX_GC_DATA_OBJECT_TARGETS, MAX_GC_PACKAGE_RECORD_TARGETS, MAX_GC_TREE_JOBS,
    MAX_MATERIALIZATION_CHECKPOINT_BYTES, MAX_MATERIALIZATION_JOURNAL_BYTES,
    MAX_MATERIALIZATION_STATE_BYTES,
};
pub(crate) use transaction::{
    abort_package_build, begin_active_package_build, begin_garbage_collection,
    begin_rollback_package_build, complete_active_package, complete_garbage_collection,
    complete_rollback_package, promote_active_catalog_set, promote_rollback_catalog_set,
    rollback_to_previous_catalog_set, stage_active_catalog_set_candidate,
    stage_rollback_catalog_set_candidate, MaterializationTransitionError,
};
pub(crate) use tree_cleanup::{remove_tree_directory, TreeCleanupError};
pub(crate) use tree_reader::{with_verified_tree_resource, TreeResourceError};
// Kept crate-private so durable owner retention is reachable only through the
// package-lease layer's authenticated package access and pinning authority.
// Native activation still requires a service-owned join with separate native
// runtime authority; no value in this module grants it.
#[allow(unused_imports)]
pub(crate) use transaction::{
    add_owner_package_pin, plan_current_catalog_package_pin, plan_owner_package_pin_removal,
    preflight_package_pin_release, remove_owner_package_pin, resolve_recovered_package_pin_release,
    verify_package_pin_release_admission, CurrentCatalogPackagePinProof, OwnerPackagePinIdentity,
    OwnerPackagePinPlan, OwnerPackagePinRemovalPlan, OwnerPackagePinRemovalProof,
    PackagePinReleaseAdmission, PackagePinReleaseAdmissionError, RecoveredPackagePinRelease,
};

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) use objects::{
    completed_package_verification_count, publish_or_reuse_active_package_at_fault,
    reset_completed_package_verification_count, ObjectPublicationFaultPoint,
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
