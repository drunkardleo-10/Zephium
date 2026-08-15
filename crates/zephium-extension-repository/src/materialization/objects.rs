//! Content-addressed publication and exact package-closure verification.
//!
//! This layer sits between authority-preserving package preparation and the
//! durable completion transition. It never treats a digest-derived name as
//! proof of content: every reused or newly published final is reopened through
//! the private-filesystem boundary and verified against the prepared package.

use std::collections::BTreeSet;
use std::io::{self, Read};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use thiserror::Error;
use zephium_core::extensions::ExtensionPackagePayloadIdentity;
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, PortableRelativePath, MAX_EXTENSION_LEGAL_NOTICE_BYTES,
    MAX_EXTENSION_TREE_INDEX_BYTES,
};
use zephium_private_fs::{
    ByteLimit, DirectoryIdentity, FileIdentity, PrivateComponent, PrivateDirectory, PrivateFsError,
    SealedPrivateDirectory, StreamingFileLength, StreamingWriteError,
};

use super::cleanup::{prove_build_stages_absent, BuildStagesAbsent, CleanupError};
use super::names::{self, RecordNameKind, TreeNameKind};
use super::policy::{
    reserve_two_transition_intent_generation, validate_completed_tree_budget,
    validate_package_anchor_consistency, PackagePolicyError,
};
use super::prepare::{PreparedActivePackage, PreparedRollbackPackage};
use super::records::{
    CatalogSetRecord, PackageRecord, StoredPayloadIdentity, MAX_CATALOG_SET_RECORD_BYTES,
    MAX_PACKAGE_RECORD_BYTES,
};
use super::runtime::MaterializationRuntime;
use super::source::{
    BundledReleaseByteSource, BundledReleasePackageSourceIdentity, BundledReleaseResource,
    BundledReleaseSourceError,
};
use super::state::{MAX_COMPLETED_PACKAGE_RECORDS, MAX_DURABLE_GENERATION};
use super::tree_writer::{
    build_authenticated_tree_stage, cleanup_tree_stage, verify_sealed_tree,
    AuthenticatedSealedTree, TreeWriterError,
};
use crate::operation::with_external_callback;
use crate::state::Digest32;

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
std::thread_local! {
    static COMPLETED_PACKAGE_VERIFICATION_COUNT: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

/// Stable, path-free failure while publishing or verifying package objects.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum PackageObjectError {
    /// The current durable intent, generation, or prepared package does not
    /// equal the preflight token.
    #[error("extension package object build state does not match its preflight")]
    BuildStateMismatch,
    /// A bounded content-addressed inventory has no room for this closure.
    #[error("extension package object capacity is exhausted")]
    CapacityExhausted,
    /// The durable state has no room for both an intent and its settlement.
    #[error("extension package object generation space is exhausted")]
    GenerationExhausted,
    /// A digest-derived final or lifecycle name has incompatible content or
    /// state and may never be replaced in place.
    #[error("extension package content-addressed object collided")]
    Collision,
    /// Exact bytes, lengths, digests, inventory, or metadata did not match.
    #[error("extension package object closure is not exact")]
    ExactMismatch,
    /// The fixed bundled-resource adapter failed its path-free source boundary.
    #[error("bundled extension package source failed: {0}")]
    Source(BundledReleaseSourceError),
    /// A private-filesystem operation failed before an ambiguous commit point.
    #[error("extension package object filesystem failed: {0}")]
    Filesystem(PrivateFsError),
    /// A mutation may have committed or cleanup could not prove exact absence.
    #[error("extension package object settlement is ambiguous")]
    SettlementAmbiguous,
}

/// Whether preflight expects the caller to commit a new build intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackageObjectIntentDisposition {
    /// No durable build intent exists; consume this token to commit the exact
    /// successor intent, then recover and preflight again before publication.
    RequiresCommit,
    /// The exact build intent is already durable, normally after clean retry or
    /// startup recovery.
    AlreadyCommitted,
    /// The exact package is already a durable completion root; only fresh
    /// read-only closure verification is permitted.
    CompletedReplay,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct MissingPackageObjects {
    tree: bool,
    tree_index: bool,
    legal: bool,
    package_record: bool,
}

impl MissingPackageObjects {
    fn regular_count(self) -> usize {
        usize::from(self.tree_index) + usize::from(self.legal) + usize::from(self.package_record)
    }
}

/// Linear proof that the exact physical inventories can admit one package.
///
/// The token is deliberately non-`Clone`, non-serializable, parent-identity
/// bound, and generation bound. Its observed presence bits are accounting and
/// stale-plan checks only; they are never content authority.
#[must_use = "package object capacity must be consumed by its exact lifecycle operation"]
pub(crate) struct PackageObjectCapacity {
    disposition: PackageObjectIntentDisposition,
    preflight_state_generation: u64,
    expected_intent_generation: Option<u64>,
    package_record_id: Digest32,
    tree_id: Digest32,
    tree_index_id: Digest32,
    legal_id: Digest32,
    records_parent: DirectoryIdentity,
    trees_parent: DirectoryIdentity,
    record: PackageRecord,
    missing: MissingPackageObjects,
}

impl PackageObjectCapacity {
    pub(crate) const fn intent_disposition(&self) -> PackageObjectIntentDisposition {
        self.disposition
    }

    /// Consumes the only token shape that may start a durable build.
    ///
    /// Publication deliberately cannot reuse this token after the state
    /// transition. The writer must recover and preflight again, obtaining an
    /// [`PackageObjectIntentDisposition::AlreadyCommitted`] token that is
    /// bound to the newly durable intent.
    pub(super) fn into_begin_parts(
        self,
    ) -> Result<(u64, DirectoryIdentity, DirectoryIdentity, PackageRecord), PackageObjectError>
    {
        if self.disposition != PackageObjectIntentDisposition::RequiresCommit
            || self.expected_intent_generation != self.preflight_state_generation.checked_add(1)
        {
            return Err(PackageObjectError::BuildStateMismatch);
        }
        Ok((
            self.preflight_state_generation,
            self.records_parent,
            self.trees_parent,
            self.record,
        ))
    }
}

/// Non-forgeable, fully reverified package closure retaining its typed
/// authority-preserving preparation capability.
///
/// `Prepared` is either [`PreparedActivePackage`] or
/// [`PreparedRollbackPackage`]. No public constructor exists and the type is
/// neither cloneable nor serializable.
#[must_use = "a verified package closure must be consumed by typed completion"]
struct VerifiedPackageClosure<Prepared> {
    intent_generation: u64,
    record_id: Digest32,
    record: PackageRecord,
    tree_root: Arc<SealedPrivateDirectory>,
    records_parent: DirectoryIdentity,
    trees_parent: DirectoryIdentity,
    stages_absent: BuildStagesAbsent,
    prepared: Prepared,
}

/// Nominal active-role completion capability.
#[must_use = "a verified active package closure must be consumed by active completion"]
pub(crate) struct VerifiedActivePackageClosure(VerifiedPackageClosure<PreparedActivePackage>);

/// Nominal rollback-role completion capability.
#[must_use = "a verified rollback package closure must be consumed by rollback completion"]
pub(crate) struct VerifiedRollbackPackageClosure(VerifiedPackageClosure<PreparedRollbackPackage>);

/// Freshly reverified durable package that requires no state transition.
///
/// This type intentionally has no conversion to [`VerifiedPackageClosure`]:
/// completed replay can satisfy an idempotent materialization request, but it
/// can never be submitted to the completion transaction a second time.
#[must_use = "a verified completed package must be projected or discarded"]
struct VerifiedCompletedPackage<Prepared> {
    record_id: Digest32,
    record: PackageRecord,
    tree_root: Arc<SealedPrivateDirectory>,
    records_parent: DirectoryIdentity,
    trees_parent: DirectoryIdentity,
    prepared: Prepared,
}

/// Nominal active-role idempotent replay capability.
#[must_use = "a verified completed active package must be projected or discarded"]
pub(crate) struct VerifiedCompletedActivePackage(VerifiedCompletedPackage<PreparedActivePackage>);

/// Nominal rollback-role idempotent replay capability.
#[must_use = "a verified completed rollback package must be projected or discarded"]
pub(crate) struct VerifiedCompletedRollbackPackage(
    VerifiedCompletedPackage<PreparedRollbackPackage>,
);

impl<Prepared> VerifiedCompletedPackage<Prepared> {
    fn into_parts(
        self,
    ) -> (
        Digest32,
        PackageRecord,
        Arc<SealedPrivateDirectory>,
        DirectoryIdentity,
        DirectoryIdentity,
        Prepared,
    ) {
        (
            self.record_id,
            self.record,
            self.tree_root,
            self.records_parent,
            self.trees_parent,
            self.prepared,
        )
    }
}

impl<Prepared> VerifiedPackageClosure<Prepared> {
    /// Consumes the closure for a completion transition while keeping the
    /// active/rollback prepared capability in the return type.
    fn into_completion_parts(
        self,
    ) -> (
        u64,
        Digest32,
        PackageRecord,
        Arc<SealedPrivateDirectory>,
        DirectoryIdentity,
        DirectoryIdentity,
        BuildStagesAbsent,
        Prepared,
    ) {
        (
            self.intent_generation,
            self.record_id,
            self.record,
            self.tree_root,
            self.records_parent,
            self.trees_parent,
            self.stages_absent,
            self.prepared,
        )
    }
}

macro_rules! impl_nominal_verified_closure {
    ($wrapper:ident, $prepared:ty) => {
        impl $wrapper {
            pub(super) fn into_completion_parts(
                self,
            ) -> (
                u64,
                Digest32,
                PackageRecord,
                Arc<SealedPrivateDirectory>,
                DirectoryIdentity,
                DirectoryIdentity,
                BuildStagesAbsent,
                $prepared,
            ) {
                self.0.into_completion_parts()
            }
        }
    };
}

impl_nominal_verified_closure!(VerifiedActivePackageClosure, PreparedActivePackage);
impl_nominal_verified_closure!(VerifiedRollbackPackageClosure, PreparedRollbackPackage);

macro_rules! impl_nominal_completed_package {
    ($wrapper:ident, $prepared:ty) => {
        impl $wrapper {
            pub(crate) fn into_parts(
                self,
            ) -> (
                Digest32,
                PackageRecord,
                Arc<SealedPrivateDirectory>,
                DirectoryIdentity,
                DirectoryIdentity,
                $prepared,
            ) {
                self.0.into_parts()
            }
        }
    };
}

impl_nominal_completed_package!(VerifiedCompletedActivePackage, PreparedActivePackage);
impl_nominal_completed_package!(VerifiedCompletedRollbackPackage, PreparedRollbackPackage);

/// Inventories exact lifecycle names and reserves bounded physical capacity.
///
/// A matching already-durable intent is accepted so a clean failure after one
/// or more final publications can retry without rewriting exact finals. Any
/// stage residue must first be reconciled by the explicit recovery path.
pub(crate) fn preflight_package_object_capacity(
    runtime: &MaterializationRuntime,
    record: &PackageRecord,
) -> Result<PackageObjectCapacity, PackageObjectError> {
    record
        .validate()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    let package_record_id = record
        .record_id()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    let (disposition, expected_intent_generation) =
        intent_preflight(runtime, record, package_record_id)?;

    if disposition != PackageObjectIntentDisposition::CompletedReplay {
        validate_completed_record_capacity(runtime, record, package_record_id)?;
        validate_anchor_consistency(runtime, record, package_record_id)?;
    }

    let record_inventory = inspect_record_capacity(&runtime._records)?;
    let tree_inventory = inspect_tree_capacity(&runtime._trees, record.tree_index.tree_sha256)?;
    let missing = MissingPackageObjects {
        tree: !tree_inventory.target_object,
        tree_index: !record_inventory
            .tree_indexes
            .contains(&record.tree_index.index_sha256),
        legal: !record_inventory.legal.contains(&record.legal.sha256),
        package_record: !record_inventory.packages.contains(&package_record_id),
    };

    if disposition == PackageObjectIntentDisposition::CompletedReplay
        && missing != MissingPackageObjects::default()
    {
        return Err(PackageObjectError::ExactMismatch);
    }

    validate_physical_capacity(&record_inventory, &tree_inventory, missing)?;

    Ok(PackageObjectCapacity {
        disposition,
        preflight_state_generation: runtime._state.generation,
        expected_intent_generation,
        package_record_id,
        tree_id: record.tree_index.tree_sha256,
        tree_index_id: record.tree_index.index_sha256,
        legal_id: record.legal.sha256,
        records_parent: runtime._records.identity(),
        trees_parent: runtime._trees.identity(),
        record: record.clone(),
        missing,
    })
}

/// Publishes or reuses an active package while retaining the active witness.
pub(crate) fn publish_or_reuse_active_package<S: BundledReleaseByteSource>(
    runtime: &mut MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedActivePackage,
    source: &mut S,
) -> Result<VerifiedActivePackageClosure, PackageObjectError> {
    publish_or_reuse_active_package_with_fault(
        runtime,
        capacity,
        prepared,
        source,
        ObjectPublicationFaultPoint::None,
    )
}

fn publish_or_reuse_active_package_with_fault<S: BundledReleaseByteSource>(
    runtime: &mut MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedActivePackage,
    source: &mut S,
    fault: ObjectPublicationFaultPoint,
) -> Result<VerifiedActivePackageClosure, PackageObjectError> {
    let view = PreparedPackageView {
        package_source: prepared.package_source(),
        tree_index: prepared.tree_index(),
        tree_index_bytes: prepared.tree_index_bytes(),
        manifest_bytes: prepared.manifest_bytes(),
        record: prepared.record(),
    };
    let verified = publish_or_reuse_package(runtime, capacity, view, source, fault)?;
    Ok(VerifiedActivePackageClosure(VerifiedPackageClosure {
        intent_generation: verified.intent_generation,
        record_id: verified.record_id,
        record: verified.record,
        tree_root: verified.tree_root,
        records_parent: verified.records_parent,
        trees_parent: verified.trees_parent,
        stages_absent: verified.stages_absent,
        prepared,
    }))
}

/// Publishes or reuses an explicitly authorized rollback package while
/// retaining the distinct rollback witness.
pub(crate) fn publish_or_reuse_rollback_package<S: BundledReleaseByteSource>(
    runtime: &mut MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedRollbackPackage,
    source: &mut S,
) -> Result<VerifiedRollbackPackageClosure, PackageObjectError> {
    publish_or_reuse_rollback_package_with_fault(
        runtime,
        capacity,
        prepared,
        source,
        ObjectPublicationFaultPoint::None,
    )
}

/// Reauthenticates a marker-committed active build entirely from durable
/// repository objects.
///
/// This operation never consults a package byte source. The package-record
/// final is only a commit marker; every catalog, manifest, index, legal, tree,
/// and record binding must still be freshly verified before a completion
/// capability is returned.
pub(crate) fn verify_interrupted_active_package(
    runtime: &MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedActivePackage,
) -> Result<VerifiedActivePackageClosure, PackageObjectError> {
    let view = PreparedPackageView {
        package_source: prepared.package_source(),
        tree_index: prepared.tree_index(),
        tree_index_bytes: prepared.tree_index_bytes(),
        manifest_bytes: prepared.manifest_bytes(),
        record: prepared.record(),
    };
    let verified = verify_interrupted_package(runtime, capacity, view)?;
    Ok(VerifiedActivePackageClosure(VerifiedPackageClosure {
        intent_generation: verified.intent_generation,
        record_id: verified.record_id,
        record: verified.record,
        tree_root: verified.tree_root,
        records_parent: verified.records_parent,
        trees_parent: verified.trees_parent,
        stages_absent: verified.stages_absent,
        prepared,
    }))
}

/// Reauthenticates a marker-committed rollback build entirely from durable
/// repository objects while retaining the nominal rollback witness.
pub(crate) fn verify_interrupted_rollback_package(
    runtime: &MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedRollbackPackage,
) -> Result<VerifiedRollbackPackageClosure, PackageObjectError> {
    let view = PreparedPackageView {
        package_source: prepared.package_source(),
        tree_index: prepared.tree_index(),
        tree_index_bytes: prepared.tree_index_bytes(),
        manifest_bytes: prepared.manifest_bytes(),
        record: prepared.record(),
    };
    let verified = verify_interrupted_package(runtime, capacity, view)?;
    Ok(VerifiedRollbackPackageClosure(VerifiedPackageClosure {
        intent_generation: verified.intent_generation,
        record_id: verified.record_id,
        record: verified.record,
        tree_root: verified.tree_root,
        records_parent: verified.records_parent,
        trees_parent: verified.trees_parent,
        stages_absent: verified.stages_absent,
        prepared,
    }))
}

fn publish_or_reuse_rollback_package_with_fault<S: BundledReleaseByteSource>(
    runtime: &mut MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedRollbackPackage,
    source: &mut S,
    fault: ObjectPublicationFaultPoint,
) -> Result<VerifiedRollbackPackageClosure, PackageObjectError> {
    let view = PreparedPackageView {
        package_source: prepared.package_source(),
        tree_index: prepared.tree_index(),
        tree_index_bytes: prepared.tree_index_bytes(),
        manifest_bytes: prepared.manifest_bytes(),
        record: prepared.record(),
    };
    let verified = publish_or_reuse_package(runtime, capacity, view, source, fault)?;
    Ok(VerifiedRollbackPackageClosure(VerifiedPackageClosure {
        intent_generation: verified.intent_generation,
        record_id: verified.record_id,
        record: verified.record,
        tree_root: verified.tree_root,
        records_parent: verified.records_parent,
        trees_parent: verified.trees_parent,
        stages_absent: verified.stages_absent,
        prepared,
    }))
}

/// Freshly verifies an already-completed active package without mutating or
/// consulting source bytes beyond the preparation that admitted its exact
/// catalog, index, and manifest.
pub(crate) fn verify_completed_active_package(
    runtime: &MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedActivePackage,
) -> Result<VerifiedCompletedActivePackage, PackageObjectError> {
    let view = PreparedPackageView {
        package_source: prepared.package_source(),
        tree_index: prepared.tree_index(),
        tree_index_bytes: prepared.tree_index_bytes(),
        manifest_bytes: prepared.manifest_bytes(),
        record: prepared.record(),
    };
    let verified = verify_completed_package(runtime, capacity, view)?;
    Ok(VerifiedCompletedActivePackage(VerifiedCompletedPackage {
        record_id: verified.record_id,
        record: verified.record,
        tree_root: verified.tree_root,
        records_parent: verified.records_parent,
        trees_parent: verified.trees_parent,
        prepared,
    }))
}

/// Freshly verifies an already-completed rollback-authorized package while
/// retaining the distinct rollback preparation witness.
pub(crate) fn verify_completed_rollback_package(
    runtime: &MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedRollbackPackage,
) -> Result<VerifiedCompletedRollbackPackage, PackageObjectError> {
    let view = PreparedPackageView {
        package_source: prepared.package_source(),
        tree_index: prepared.tree_index(),
        tree_index_bytes: prepared.tree_index_bytes(),
        manifest_bytes: prepared.manifest_bytes(),
        record: prepared.record(),
    };
    let verified = verify_completed_package(runtime, capacity, view)?;
    Ok(VerifiedCompletedRollbackPackage(VerifiedCompletedPackage {
        record_id: verified.record_id,
        record: verified.record,
        tree_root: verified.tree_root,
        records_parent: verified.records_parent,
        trees_parent: verified.trees_parent,
        prepared,
    }))
}

/// Exact-CAS publishes one already authority-prepared catalog selection.
///
/// The caller must retain the nominal active or rollback admission witnesses;
/// this structural helper only verifies completed-record bindings and the
/// sealed content-addressed final. A newly published final remains inert until
/// a separate journaled state transition selects it.
pub(super) fn publish_or_reuse_catalog_set_record(
    runtime: &mut MaterializationRuntime,
    record: &CatalogSetRecord,
) -> Result<Digest32, PackageObjectError> {
    runtime
        ._state
        .validate()
        .map_err(|_| PackageObjectError::BuildStateMismatch)?;
    record
        .validate()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    if !runtime.garbage_collection_is_idle()
        || runtime._state.build_intent.is_some()
        || runtime._build_intent.is_some()
        || runtime._build_stage.is_some()
        || !runtime._record_stages.is_empty()
    {
        return Err(PackageObjectError::BuildStateMismatch);
    }
    for row in &record.packages {
        if runtime
            ._state
            .completed_package_record_ids
            .binary_search(&row.package_record_id)
            .is_err()
        {
            return Err(PackageObjectError::BuildStateMismatch);
        }
        let package = runtime
            ._package_records
            .get(&row.package_record_id)
            .ok_or(PackageObjectError::BuildStateMismatch)?;
        if package.catalog != record.catalog
            || package.package.package_key != row.package_key
            || package.manifest.runtime_target != row.runtime_target
        {
            return Err(PackageObjectError::ExactMismatch);
        }
    }

    let bytes = record
        .canonical_bytes()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    let record_id = record
        .record_id()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    if bytes.len() > MAX_CATALOG_SET_RECORD_BYTES
        || Digest32::from_bytes(Sha256::digest(&bytes).into()) != record_id
    {
        return Err(PackageObjectError::ExactMismatch);
    }

    let inventory = inspect_record_capacity(&runtime._records)?;
    let existing = runtime._catalog_sets.get(&record_id);
    if existing.is_some_and(|stored| stored != record) {
        return Err(PackageObjectError::Collision);
    }
    let missing = existing.is_none();
    if missing
        && (checked_add(inventory.catalog_set_count, 1)? > names::MAX_FINAL_CATALOG_SET_RECORDS
            || checked_add(inventory.entries, 1)? > names::MAX_RECORD_ENTRIES)
    {
        return Err(PackageObjectError::CapacityExhausted);
    }
    ensure_bytes_regular_object(
        &runtime._records,
        &names::catalog_set_record_stage(record_id),
        &names::catalog_set_record(record_id),
        &bytes,
        RegularExpectation {
            length: u64::try_from(bytes.len()).map_err(|_| PackageObjectError::ExactMismatch)?,
            sha256: record_id.bytes(),
            exact_bytes: Some(&bytes),
        },
        missing,
    )?;
    runtime._catalog_sets.insert(record_id, record.clone());
    Ok(record_id)
}

/// Read-only exact verification of one already published catalog-set final.
pub(super) fn verify_existing_catalog_set_record(
    runtime: &MaterializationRuntime,
    record: &CatalogSetRecord,
) -> Result<(), PackageObjectError> {
    record
        .validate()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    let bytes = record
        .canonical_bytes()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    let record_id = record
        .record_id()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    if runtime._catalog_sets.get(&record_id) != Some(record) {
        return Err(PackageObjectError::ExactMismatch);
    }
    verify_required_regular(
        &runtime._records,
        &names::catalog_set_record(record_id),
        RegularExpectation {
            length: u64::try_from(bytes.len()).map_err(|_| PackageObjectError::ExactMismatch)?,
            sha256: record_id.bytes(),
            exact_bytes: Some(&bytes),
        },
        true,
    )
    .map_err(map_final_object_error)
}

struct PreparedPackageView<'prepared> {
    package_source: BundledReleasePackageSourceIdentity,
    tree_index: &'prepared CanonicalExtensionTreeIndex,
    tree_index_bytes: &'prepared [u8],
    manifest_bytes: &'prepared [u8],
    record: &'prepared PackageRecord,
}

struct VerifiedPackageObjects {
    intent_generation: u64,
    record_id: Digest32,
    record: PackageRecord,
    tree_root: Arc<SealedPrivateDirectory>,
    records_parent: DirectoryIdentity,
    trees_parent: DirectoryIdentity,
    stages_absent: BuildStagesAbsent,
}

struct VerifiedCompletedObjects {
    record_id: Digest32,
    record: PackageRecord,
    tree_root: Arc<SealedPrivateDirectory>,
    records_parent: DirectoryIdentity,
    trees_parent: DirectoryIdentity,
}

fn publish_or_reuse_package<S: BundledReleaseByteSource>(
    runtime: &mut MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedPackageView<'_>,
    source: &mut S,
    fault: ObjectPublicationFaultPoint,
) -> Result<VerifiedPackageObjects, PackageObjectError> {
    validate_capacity_and_intent(runtime, &capacity, &prepared)?;
    validate_prepared_package(&prepared)?;
    let intent_generation = capacity
        .expected_intent_generation
        .ok_or(PackageObjectError::BuildStateMismatch)?;

    // Publication order is part of the crash protocol. The package record is
    // deliberately last so it can never precede any object it describes.
    ensure_tree_final(runtime, &capacity, &prepared, source)?;
    fail_after_object_publication(fault, ObjectPublicationFaultPoint::AfterTree)?;
    ensure_tree_index_final(runtime, &capacity, &prepared)?;
    fail_after_object_publication(fault, ObjectPublicationFaultPoint::AfterTreeIndex)?;
    ensure_legal_final(runtime, &capacity, &prepared, source)?;
    fail_after_object_publication(fault, ObjectPublicationFaultPoint::AfterLegal)?;
    let package_record_bytes = ensure_package_record_final(runtime, &capacity, &prepared)?;
    fail_after_object_publication(fault, ObjectPublicationFaultPoint::AfterPackageRecord)?;

    let tree_root = verify_complete_final_closure(runtime, &prepared, &package_record_bytes)?;
    fail_after_object_publication(fault, ObjectPublicationFaultPoint::AfterClosureVerification)?;
    let stages_absent = prove_build_stages_absent(runtime).map_err(map_cleanup_error)?;
    Ok(VerifiedPackageObjects {
        intent_generation,
        record_id: capacity.package_record_id,
        record: prepared.record.clone(),
        tree_root,
        records_parent: capacity.records_parent,
        trees_parent: capacity.trees_parent,
        stages_absent,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObjectPublicationFaultPoint {
    None,
    AfterTree,
    AfterTreeIndex,
    AfterLegal,
    AfterPackageRecord,
    /// Test-only transient I/O failure after the commit marker is durable.
    #[cfg(test)]
    AfterPackageRecordTransientFailure,
    AfterClosureVerification,
}

fn fail_after_object_publication(
    configured: ObjectPublicationFaultPoint,
    reached: ObjectPublicationFaultPoint,
) -> Result<(), PackageObjectError> {
    #[cfg(test)]
    if configured == ObjectPublicationFaultPoint::AfterPackageRecordTransientFailure
        && reached == ObjectPublicationFaultPoint::AfterPackageRecord
    {
        return Err(PackageObjectError::Filesystem(PrivateFsError::Io));
    }
    #[cfg(test)]
    if configured == reached {
        return Err(PackageObjectError::SettlementAmbiguous);
    }
    #[cfg(not(test))]
    let _ = (configured, reached);
    Ok(())
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn publish_or_reuse_active_package_at_fault<S: BundledReleaseByteSource>(
    runtime: &mut MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedActivePackage,
    source: &mut S,
    fault: ObjectPublicationFaultPoint,
) -> Result<VerifiedActivePackageClosure, PackageObjectError> {
    publish_or_reuse_active_package_with_fault(runtime, capacity, prepared, source, fault)
}

fn verify_completed_package(
    runtime: &MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedPackageView<'_>,
) -> Result<VerifiedCompletedObjects, PackageObjectError> {
    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    COMPLETED_PACKAGE_VERIFICATION_COUNT.with(|count| {
        count.set(
            count
                .get()
                .checked_add(1)
                .expect("completed package verification test count must fit usize"),
        );
    });
    validate_completed_capacity_and_state(runtime, &capacity, &prepared)?;
    validate_prepared_package(&prepared)?;
    let package_record_bytes = canonical_package_record_bytes(&capacity, &prepared)?;
    let tree_root = verify_complete_final_closure(runtime, &prepared, &package_record_bytes)?;
    Ok(VerifiedCompletedObjects {
        record_id: capacity.package_record_id,
        record: prepared.record.clone(),
        tree_root,
        records_parent: capacity.records_parent,
        trees_parent: capacity.trees_parent,
    })
}

fn verify_interrupted_package(
    runtime: &MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: PreparedPackageView<'_>,
) -> Result<VerifiedPackageObjects, PackageObjectError> {
    validate_capacity_and_intent(runtime, &capacity, &prepared)?;
    validate_prepared_package(&prepared)?;
    if capacity.missing != MissingPackageObjects::default() {
        // Publication orders the package record last. Once that marker exists,
        // a missing predecessor is durable incoherence, never an incomplete
        // build that may be repaired from an external source.
        return Err(PackageObjectError::ExactMismatch);
    }
    let intent_generation = capacity
        .expected_intent_generation
        .ok_or(PackageObjectError::BuildStateMismatch)?;
    let package_record_bytes = canonical_package_record_bytes(&capacity, &prepared)?;
    let tree_root = verify_complete_final_closure(runtime, &prepared, &package_record_bytes)?;
    let stages_absent = prove_build_stages_absent(runtime).map_err(map_cleanup_error)?;
    Ok(VerifiedPackageObjects {
        intent_generation,
        record_id: capacity.package_record_id,
        record: prepared.record.clone(),
        tree_root,
        records_parent: capacity.records_parent,
        trees_parent: capacity.trees_parent,
        stages_absent,
    })
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn reset_completed_package_verification_count() {
    COMPLETED_PACKAGE_VERIFICATION_COUNT.with(|count| count.set(0));
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn completed_package_verification_count() -> usize {
    COMPLETED_PACKAGE_VERIFICATION_COUNT.with(std::cell::Cell::get)
}

fn intent_preflight(
    runtime: &MaterializationRuntime,
    record: &PackageRecord,
    record_id: Digest32,
) -> Result<(PackageObjectIntentDisposition, Option<u64>), PackageObjectError> {
    runtime
        ._state
        .validate()
        .map_err(|_| PackageObjectError::BuildStateMismatch)?;
    if !runtime.garbage_collection_is_idle()
        || runtime._state.build_intent != runtime._build_intent
        || runtime._build_stage.is_some()
        || !runtime._record_stages.is_empty()
    {
        return Err(PackageObjectError::BuildStateMismatch);
    }

    if runtime
        ._state
        .completed_package_record_ids
        .binary_search(&record_id)
        .is_ok()
    {
        if runtime._build_intent.is_some()
            || runtime._package_records.get(&record_id) != Some(record)
        {
            return Err(PackageObjectError::BuildStateMismatch);
        }
        return Ok((PackageObjectIntentDisposition::CompletedReplay, None));
    }

    match runtime._build_intent.as_ref() {
        None => {
            let generation = reserve_two_transition_intent_generation(runtime._state.generation)
                .ok_or(PackageObjectError::GenerationExhausted)?;
            Ok((
                PackageObjectIntentDisposition::RequiresCommit,
                Some(generation),
            ))
        }
        Some(intent)
            if intent.generation == runtime._state.generation
                && intent.package_record_id == record_id
                && &intent.package_record == record =>
        {
            if intent.generation == MAX_DURABLE_GENERATION {
                return Err(PackageObjectError::GenerationExhausted);
            }
            Ok((
                PackageObjectIntentDisposition::AlreadyCommitted,
                Some(intent.generation),
            ))
        }
        Some(_) => Err(PackageObjectError::BuildStateMismatch),
    }
}

fn validate_completed_record_capacity(
    runtime: &MaterializationRuntime,
    record: &PackageRecord,
    record_id: Digest32,
) -> Result<(), PackageObjectError> {
    if runtime
        ._state
        .completed_package_record_ids
        .binary_search(&record_id)
        .is_ok()
    {
        return Err(PackageObjectError::BuildStateMismatch);
    }
    if runtime._state.completed_package_record_ids.len() >= MAX_COMPLETED_PACKAGE_RECORDS {
        return Err(PackageObjectError::CapacityExhausted);
    }

    let mut packages = Vec::with_capacity(
        runtime
            ._state
            .completed_package_record_ids
            .len()
            .saturating_add(1),
    );
    for completed_id in &runtime._state.completed_package_record_ids {
        packages.push(
            runtime
                ._package_records
                .get(completed_id)
                .ok_or(PackageObjectError::BuildStateMismatch)?,
        );
    }
    packages.push(record);
    validate_completed_tree_budget(packages).map_err(map_package_policy)
}

fn validate_anchor_consistency(
    runtime: &MaterializationRuntime,
    candidate: &PackageRecord,
    candidate_id: Digest32,
) -> Result<(), PackageObjectError> {
    let mut packages = Vec::with_capacity(
        runtime
            ._state
            .completed_package_record_ids
            .len()
            .saturating_add(1),
    );
    for completed_id in &runtime._state.completed_package_record_ids {
        if *completed_id == candidate_id {
            return Err(PackageObjectError::BuildStateMismatch);
        }
        packages.push(
            runtime
                ._package_records
                .get(completed_id)
                .ok_or(PackageObjectError::BuildStateMismatch)?,
        );
    }
    packages.push(candidate);
    validate_package_anchor_consistency(packages).map_err(map_package_policy)
}

fn map_package_policy(error: PackagePolicyError) -> PackageObjectError {
    match error {
        PackagePolicyError::AnchorConflict => PackageObjectError::Collision,
        PackagePolicyError::TreeBudgetExceeded | PackagePolicyError::AccountingOverflow => {
            PackageObjectError::CapacityExhausted
        }
    }
}

fn map_cleanup_error(error: CleanupError) -> PackageObjectError {
    match error {
        CleanupError::BuildStateMismatch => PackageObjectError::BuildStateMismatch,
        // A stage appearing after every publication reported settlement is no
        // longer a clean validation failure. Completion must seal/recover.
        CleanupError::ExactMismatch
        | CleanupError::CommitMarkerPresent
        | CleanupError::SettlementAmbiguous => PackageObjectError::SettlementAmbiguous,
        CleanupError::Filesystem(error) => map_inventory_fs(error),
    }
}

#[derive(Default)]
struct RecordCapacityInventory {
    entries: usize,
    package_count: usize,
    catalog_set_count: usize,
    tree_index_count: usize,
    legal_count: usize,
    packages: BTreeSet<Digest32>,
    tree_indexes: BTreeSet<Digest32>,
    legal: BTreeSet<Digest32>,
}

fn inspect_record_capacity(
    records: &PrivateDirectory,
) -> Result<RecordCapacityInventory, PackageObjectError> {
    let entries = records
        .list_components(names::MAX_RECORD_ENTRIES)
        .map_err(map_inventory_fs)?;
    let mut inventory = RecordCapacityInventory {
        entries: entries.len(),
        ..RecordCapacityInventory::default()
    };
    let mut final_ids = BTreeSet::new();
    for entry in entries {
        let (digest, kind) =
            names::parse_record_name(entry.as_str()).ok_or(PackageObjectError::ExactMismatch)?;
        if kind.is_stage() {
            return Err(PackageObjectError::BuildStateMismatch);
        }
        if !final_ids.insert((kind.object_kind(), digest)) {
            return Err(PackageObjectError::ExactMismatch);
        }
        match kind {
            RecordNameKind::Package { stage: false } => {
                inventory.package_count = checked_increment(inventory.package_count)?;
                inventory.packages.insert(digest);
            }
            RecordNameKind::CatalogSet { stage: false } => {
                inventory.catalog_set_count = checked_increment(inventory.catalog_set_count)?;
            }
            RecordNameKind::TreeIndex { stage: false } => {
                inventory.tree_index_count = checked_increment(inventory.tree_index_count)?;
                inventory.tree_indexes.insert(digest);
            }
            RecordNameKind::Legal { stage: false } => {
                inventory.legal_count = checked_increment(inventory.legal_count)?;
                inventory.legal.insert(digest);
            }
            RecordNameKind::Package { stage: true }
            | RecordNameKind::CatalogSet { stage: true }
            | RecordNameKind::TreeIndex { stage: true }
            | RecordNameKind::Legal { stage: true } => unreachable!("stage returned above"),
        }
    }
    Ok(inventory)
}

struct TreeCapacityInventory {
    entries: usize,
    object_count: usize,
    target_object: bool,
}

fn inspect_tree_capacity(
    trees: &PrivateDirectory,
    target: Digest32,
) -> Result<TreeCapacityInventory, PackageObjectError> {
    let entries = trees
        .list_components(names::MAX_TREE_ENTRIES)
        .map_err(map_inventory_fs)?;
    let mut observed = BTreeSet::new();
    let mut object_count = 0_usize;
    let mut target_object = false;
    for entry in &entries {
        let (digest, kind) =
            names::parse_tree_name(entry.as_str()).ok_or(PackageObjectError::ExactMismatch)?;
        if !observed.insert(digest) {
            return Err(PackageObjectError::ExactMismatch);
        }
        match kind {
            TreeNameKind::Object => {
                object_count = checked_increment(object_count)?;
                target_object |= digest == target;
            }
            TreeNameKind::Acquisition => return Err(PackageObjectError::BuildStateMismatch),
            TreeNameKind::Stage(_) => return Err(PackageObjectError::BuildStateMismatch),
            TreeNameKind::Retired(_) if digest == target => {
                return Err(PackageObjectError::Collision)
            }
            TreeNameKind::Retired(_) => {}
        }
    }
    Ok(TreeCapacityInventory {
        entries: entries.len(),
        object_count,
        target_object,
    })
}

fn validate_physical_capacity(
    records: &RecordCapacityInventory,
    trees: &TreeCapacityInventory,
    missing: MissingPackageObjects,
) -> Result<(), PackageObjectError> {
    if checked_add(records.package_count, usize::from(missing.package_record))?
        > names::MAX_FINAL_PACKAGE_RECORDS
        || checked_add(records.tree_index_count, usize::from(missing.tree_index))?
            > names::MAX_FINAL_DATA_OBJECTS_PER_KIND
        || checked_add(records.legal_count, usize::from(missing.legal))?
            > names::MAX_FINAL_DATA_OBJECTS_PER_KIND
        || records.catalog_set_count > names::MAX_FINAL_CATALOG_SET_RECORDS
        || checked_add(records.entries, missing.regular_count())? > names::MAX_RECORD_ENTRIES
        || checked_add(trees.object_count, usize::from(missing.tree))?
            > names::MAX_FINAL_PACKAGE_RECORDS
        || checked_add(trees.entries, usize::from(missing.tree))? > names::MAX_TREE_ENTRIES
    {
        return Err(PackageObjectError::CapacityExhausted);
    }
    Ok(())
}

fn validate_capacity_and_intent(
    runtime: &MaterializationRuntime,
    capacity: &PackageObjectCapacity,
    prepared: &PreparedPackageView<'_>,
) -> Result<(), PackageObjectError> {
    runtime
        ._state
        .validate()
        .map_err(|_| PackageObjectError::BuildStateMismatch)?;
    let expected_intent_generation = capacity
        .expected_intent_generation
        .ok_or(PackageObjectError::BuildStateMismatch)?;
    if capacity.disposition != PackageObjectIntentDisposition::AlreadyCommitted
        || runtime._records.identity() != capacity.records_parent
        || runtime._trees.identity() != capacity.trees_parent
        || &capacity.record != prepared.record
        || prepared.record.record_id().ok() != Some(capacity.package_record_id)
        || prepared.record.tree_index.tree_sha256 != capacity.tree_id
        || prepared.record.tree_index.index_sha256 != capacity.tree_index_id
        || prepared.record.legal.sha256 != capacity.legal_id
        || !runtime.garbage_collection_is_idle()
        || runtime._state.build_intent != runtime._build_intent
        || runtime._build_stage.is_some()
        || !runtime._record_stages.is_empty()
    {
        return Err(PackageObjectError::BuildStateMismatch);
    }
    let intent = runtime
        ._build_intent
        .as_ref()
        .ok_or(PackageObjectError::BuildStateMismatch)?;
    if intent.generation != expected_intent_generation
        || runtime._state.generation != expected_intent_generation
        || intent.package_record_id != capacity.package_record_id
        || &intent.package_record != prepared.record
    {
        return Err(PackageObjectError::BuildStateMismatch);
    }

    let records = inspect_record_capacity(&runtime._records)?;
    let trees = inspect_tree_capacity(&runtime._trees, capacity.tree_id)?;
    let observed = MissingPackageObjects {
        tree: !trees.target_object,
        tree_index: !records.tree_indexes.contains(&capacity.tree_index_id),
        legal: !records.legal.contains(&capacity.legal_id),
        package_record: !records.packages.contains(&capacity.package_record_id),
    };
    if observed != capacity.missing {
        return Err(PackageObjectError::Collision);
    }
    validate_physical_capacity(&records, &trees, observed)
}

fn validate_completed_capacity_and_state(
    runtime: &MaterializationRuntime,
    capacity: &PackageObjectCapacity,
    prepared: &PreparedPackageView<'_>,
) -> Result<(), PackageObjectError> {
    runtime
        ._state
        .validate()
        .map_err(|_| PackageObjectError::BuildStateMismatch)?;
    if capacity.disposition != PackageObjectIntentDisposition::CompletedReplay
        || capacity.expected_intent_generation.is_some()
        || capacity.missing != MissingPackageObjects::default()
        || capacity.preflight_state_generation != runtime._state.generation
        || runtime._records.identity() != capacity.records_parent
        || runtime._trees.identity() != capacity.trees_parent
        || &capacity.record != prepared.record
        || prepared.record.record_id().ok() != Some(capacity.package_record_id)
        || prepared.record.tree_index.tree_sha256 != capacity.tree_id
        || prepared.record.tree_index.index_sha256 != capacity.tree_index_id
        || prepared.record.legal.sha256 != capacity.legal_id
        || !runtime.garbage_collection_is_idle()
        || runtime._state.build_intent.is_some()
        || runtime._build_intent.is_some()
        || runtime._build_stage.is_some()
        || !runtime._record_stages.is_empty()
        || runtime
            ._state
            .completed_package_record_ids
            .binary_search(&capacity.package_record_id)
            .is_err()
        || runtime._package_records.get(&capacity.package_record_id) != Some(prepared.record)
    {
        return Err(PackageObjectError::BuildStateMismatch);
    }

    let records = inspect_record_capacity(&runtime._records)?;
    let trees = inspect_tree_capacity(&runtime._trees, capacity.tree_id)?;
    let observed = MissingPackageObjects {
        tree: !trees.target_object,
        tree_index: !records.tree_indexes.contains(&capacity.tree_index_id),
        legal: !records.legal.contains(&capacity.legal_id),
        package_record: !records.packages.contains(&capacity.package_record_id),
    };
    if observed != MissingPackageObjects::default() {
        return Err(PackageObjectError::ExactMismatch);
    }
    validate_physical_capacity(&records, &trees, observed)
}

fn validate_prepared_package(prepared: &PreparedPackageView<'_>) -> Result<(), PackageObjectError> {
    let record = prepared.record;
    record
        .validate()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    let source = prepared.package_source;
    let catalog = source.catalog();
    let index = prepared.tree_index;
    let index_length = u64::try_from(prepared.tree_index_bytes.len())
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    let manifest_length = u64::try_from(prepared.manifest_bytes.len())
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    let index_digest: [u8; 32] = Sha256::digest(prepared.tree_index_bytes).into();
    let manifest_digest: [u8; 32] = Sha256::digest(prepared.manifest_bytes).into();

    if source.payload() != ExtensionPackagePayloadIdentity::BundledTree
        || record.package.payload != StoredPayloadIdentity::BundledTree
        || catalog.authority().bytes() != record.catalog.authority_id.bytes()
        || catalog.revision().get() != record.catalog.revision
        || catalog.catalog_length() != record.catalog.catalog_length
        || catalog.catalog_digest().bytes() != record.catalog.catalog_sha256.bytes()
        || catalog.inventory_digest().bytes() != record.catalog.inventory_sha256.bytes()
        || source.authority().bytes() != record.package.authority_id.bytes()
        || source.package_key().bytes() != record.package.package_key.bytes()
        || source.package_revision().get() != record.package.revision
        || source.manifest_digest().bytes() != record.package.manifest_sha256.bytes()
        || source.tree_digest().bytes() != record.package.tree_sha256.bytes()
        || source.package_row_sha256() != record.package.package_row_sha256.bytes()
        || index.index_sha256().bytes() != record.tree_index.index_sha256.bytes()
        || index.index_bytes() != record.tree_index.index_length
        || index.tree_sha256().bytes() != record.tree_index.tree_sha256.bytes()
        || index.files().len() != record.tree_index.file_count as usize
        || index.implicit_directory_count() != record.tree_index.directory_count as usize
        || index.total_entry_count() != record.tree_index.total_entry_count as usize
        || index.total_bytes() != record.tree_index.tree_bytes
        || index_length != record.tree_index.index_length
        || index_length > MAX_EXTENSION_TREE_INDEX_BYTES as u64
        || index_digest != record.tree_index.index_sha256.bytes()
        || manifest_length != record.manifest.manifest_length
        || manifest_digest != record.manifest.manifest_sha256.bytes()
        || index.manifest_sha256().bytes() != manifest_digest
        || record.legal.length > MAX_EXTENSION_LEGAL_NOTICE_BYTES
        || PortableRelativePath::parse(&record.legal.target).is_err()
    {
        return Err(PackageObjectError::ExactMismatch);
    }
    Ok(())
}

fn checked_increment(value: usize) -> Result<usize, PackageObjectError> {
    checked_add(value, 1)
}

fn checked_add(left: usize, right: usize) -> Result<usize, PackageObjectError> {
    left.checked_add(right)
        .ok_or(PackageObjectError::CapacityExhausted)
}

fn map_inventory_fs(error: PrivateFsError) -> PackageObjectError {
    match error {
        PrivateFsError::BoundExceeded => PackageObjectError::ExactMismatch,
        PrivateFsError::IdentityAmbiguous
        | PrivateFsError::SettlementUnknown
        | PrivateFsError::Quarantined => PackageObjectError::SettlementAmbiguous,
        other => PackageObjectError::Filesystem(other),
    }
}

fn ensure_tree_final<S: BundledReleaseByteSource>(
    runtime: &MaterializationRuntime,
    capacity: &PackageObjectCapacity,
    prepared: &PreparedPackageView<'_>,
    source: &mut S,
) -> Result<(), PackageObjectError> {
    let destination = names::tree_object(capacity.tree_id);
    let existing = open_optional_sealed_tree(&runtime._trees, &destination)?;
    if existing.is_some() == capacity.missing.tree {
        return Err(PackageObjectError::Collision);
    }
    if let Some(root) = existing {
        verify_tree_against_record(root, prepared.tree_index, prepared.record)
            .map_err(map_final_object_error)?;
        return Ok(());
    }

    let stage_name = names::tree_stage(
        capacity.tree_id,
        capacity
            .expected_intent_generation
            .ok_or(PackageObjectError::BuildStateMismatch)?,
    )
    .map_err(|_| PackageObjectError::BuildStateMismatch)?;
    let stage = match build_authenticated_tree_stage(
        &runtime._trees,
        &stage_name,
        prepared.package_source,
        prepared.tree_index,
        prepared.manifest_bytes,
        source,
    ) {
        Ok(stage) => stage,
        Err(error) => {
            let cleanup = if matches!(
                error,
                TreeWriterError::Filesystem(PrivateFsError::AlreadyExists)
            ) {
                TreeStageCleanupExpectation::Unowned
            } else {
                TreeStageCleanupExpectation::MayBeAbsent
            };
            return Err(settle_tree_failure(
                &runtime._trees,
                &stage_name,
                error,
                cleanup,
            ));
        }
    };

    let root = match stage.publish_same_parent_noreplace(&runtime._trees, &destination) {
        Ok(root) => root,
        Err(error) => {
            return Err(settle_tree_failure(
                &runtime._trees,
                &stage_name,
                error,
                TreeStageCleanupExpectation::MustExist,
            ));
        }
    };
    verify_tree_against_record(root, prepared.tree_index, prepared.record)
        .map_err(map_final_object_error)?;
    Ok(())
}

fn open_optional_sealed_tree(
    trees: &PrivateDirectory,
    name: &PrivateComponent,
) -> Result<Option<Arc<SealedPrivateDirectory>>, PackageObjectError> {
    match trees.open_sealed_private_child(name) {
        Ok(directory) => Ok(Some(Arc::new(directory))),
        Err(PrivateFsError::NotFound) => Ok(None),
        Err(
            PrivateFsError::IdentityAmbiguous
            | PrivateFsError::SettlementUnknown
            | PrivateFsError::Quarantined,
        ) => Err(PackageObjectError::SettlementAmbiguous),
        Err(PrivateFsError::Unsafe | PrivateFsError::AlreadyExists) => {
            Err(PackageObjectError::Collision)
        }
        Err(error) => Err(PackageObjectError::Filesystem(error)),
    }
}

fn verify_tree_against_record(
    root: Arc<SealedPrivateDirectory>,
    index: &CanonicalExtensionTreeIndex,
    record: &PackageRecord,
) -> Result<AuthenticatedSealedTree, PackageObjectError> {
    let authenticated = verify_sealed_tree(&root, index).map_err(map_tree_error)?;
    if authenticated.tree_sha256().bytes() != record.tree_index.tree_sha256.bytes()
        || authenticated.file_count() != record.tree_index.file_count as usize
        || authenticated.total_bytes() != record.tree_index.tree_bytes
    {
        return Err(PackageObjectError::ExactMismatch);
    }
    Ok(authenticated)
}

fn settle_tree_failure(
    trees: &PrivateDirectory,
    stage_name: &PrivateComponent,
    error: TreeWriterError,
    cleanup: TreeStageCleanupExpectation,
) -> PackageObjectError {
    if tree_error_is_terminal(error) {
        return PackageObjectError::SettlementAmbiguous;
    }
    if cleanup == TreeStageCleanupExpectation::Unowned {
        return map_tree_error(error);
    }
    match cleanup_tree_stage(trees, stage_name) {
        Ok(true) => map_tree_error(error),
        Ok(false) if cleanup == TreeStageCleanupExpectation::MayBeAbsent => map_tree_error(error),
        Ok(false) | Err(_) => PackageObjectError::SettlementAmbiguous,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TreeStageCleanupExpectation {
    /// The failure proves this operation never owned the colliding name.
    Unowned,
    /// Validation or construction may have failed before creating the stage.
    MayBeAbsent,
    /// A recoverable consuming publish guarantees the unchanged stage remains.
    MustExist,
}

fn tree_error_is_terminal(error: TreeWriterError) -> bool {
    matches!(
        error,
        TreeWriterError::TransitionAmbiguous
            | TreeWriterError::Filesystem(
                PrivateFsError::SettlementUnknown
                    | PrivateFsError::Quarantined
                    | PrivateFsError::IdentityAmbiguous
            )
    )
}

fn map_tree_error(error: TreeWriterError) -> PackageObjectError {
    match error {
        TreeWriterError::Source(error) => PackageObjectError::Source(error),
        #[cfg(feature = "acquired-packages")]
        TreeWriterError::AcquiredArchive(_) => PackageObjectError::ExactMismatch,
        #[cfg(feature = "acquired-packages")]
        TreeWriterError::AcquiredTree(_) => PackageObjectError::ExactMismatch,
        TreeWriterError::Filesystem(PrivateFsError::AlreadyExists | PrivateFsError::Unsafe) => {
            PackageObjectError::Collision
        }
        TreeWriterError::Filesystem(PrivateFsError::BoundExceeded)
        | TreeWriterError::ExactMismatch => PackageObjectError::ExactMismatch,
        TreeWriterError::Filesystem(
            PrivateFsError::SettlementUnknown
            | PrivateFsError::Quarantined
            | PrivateFsError::IdentityAmbiguous,
        )
        | TreeWriterError::TransitionAmbiguous => PackageObjectError::SettlementAmbiguous,
        TreeWriterError::Filesystem(error) => PackageObjectError::Filesystem(error),
    }
}

fn ensure_tree_index_final(
    runtime: &MaterializationRuntime,
    capacity: &PackageObjectCapacity,
    prepared: &PreparedPackageView<'_>,
) -> Result<(), PackageObjectError> {
    let expectation = RegularExpectation {
        length: prepared.record.tree_index.index_length,
        sha256: prepared.record.tree_index.index_sha256.bytes(),
        exact_bytes: Some(prepared.tree_index_bytes),
    };
    ensure_bytes_regular_object(
        &runtime._records,
        &names::tree_index_stage(capacity.tree_index_id),
        &names::tree_index_object(capacity.tree_index_id),
        prepared.tree_index_bytes,
        expectation,
        capacity.missing.tree_index,
    )
}

fn ensure_legal_final<S: BundledReleaseByteSource>(
    runtime: &MaterializationRuntime,
    capacity: &PackageObjectCapacity,
    prepared: &PreparedPackageView<'_>,
    source: &mut S,
) -> Result<(), PackageObjectError> {
    let stage_name = names::legal_stage(capacity.legal_id);
    let final_name = names::legal_object(capacity.legal_id);
    let expectation = RegularExpectation {
        length: prepared.record.legal.length,
        sha256: prepared.record.legal.sha256.bytes(),
        exact_bytes: None,
    };
    let existing = verify_optional_sealed_regular(&runtime._records, &final_name, expectation)?;
    if existing == capacity.missing.legal {
        return Err(PackageObjectError::Collision);
    }
    if existing {
        return Ok(());
    }

    let target = PortableRelativePath::parse(&prepared.record.legal.target)
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    let resource = BundledReleaseResource::legal_notice(
        prepared.package_source,
        &target,
        prepared.record.legal.length,
        prepared.record.legal.sha256.bytes(),
    );
    let mut stage_written = false;
    let nested = with_external_callback(|| {
        source.with_resource(resource, |reader| {
            let mut digesting = DigestingReader::new(reader);
            let identity = runtime
                ._records
                .write_new_from_reader(
                    &stage_name,
                    &mut digesting,
                    streaming_length(prepared.record.legal.length)?,
                )
                .map_err(map_streaming_write)?;
            stage_written = true;
            let proof = digesting.finish();
            if proof.length != expectation.length || proof.sha256 != expectation.sha256 {
                return Err(PackageObjectError::ExactMismatch);
            }
            Ok(identity)
        })
    });
    let identity = match nested {
        Err(error) => {
            if stage_written {
                cleanup_regular_stage(&runtime._records, &stage_name)?;
            }
            return Err(PackageObjectError::Source(error));
        }
        Ok(Err(error)) => {
            if stage_written && !object_error_is_terminal(error) {
                cleanup_regular_stage(&runtime._records, &stage_name)?;
            }
            return Err(error);
        }
        Ok(Ok(identity)) => identity,
    };

    settle_regular_stage(
        &runtime._records,
        &stage_name,
        &final_name,
        identity,
        expectation,
    )
}

fn ensure_package_record_final(
    runtime: &MaterializationRuntime,
    capacity: &PackageObjectCapacity,
    prepared: &PreparedPackageView<'_>,
) -> Result<Vec<u8>, PackageObjectError> {
    let bytes = canonical_package_record_bytes(capacity, prepared)?;
    let expectation = RegularExpectation {
        length: u64::try_from(bytes.len()).map_err(|_| PackageObjectError::ExactMismatch)?,
        sha256: capacity.package_record_id.bytes(),
        exact_bytes: Some(&bytes),
    };
    ensure_bytes_regular_object(
        &runtime._records,
        &names::package_record_stage(capacity.package_record_id),
        &names::package_record(capacity.package_record_id),
        &bytes,
        expectation,
        capacity.missing.package_record,
    )?;
    Ok(bytes)
}

/// Deterministically publishes the current intent's exact commit marker.
///
/// This internal E2E hook models a marker appearing after settlement's first
/// absence observation while deliberately leaving the recovered runtime map
/// stale. It uses the production exact-CAS publication primitive and accepts
/// no package byte source.
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn publish_intent_package_record_marker_for_e2e(
    runtime: &MaterializationRuntime,
) -> Result<(), PackageObjectError> {
    let intent = runtime
        ._build_intent
        .as_ref()
        .ok_or(PackageObjectError::BuildStateMismatch)?;
    if runtime._state.build_intent.as_ref() != Some(intent)
        || runtime._state.generation != intent.generation
    {
        return Err(PackageObjectError::BuildStateMismatch);
    }
    let bytes = intent
        .package_record
        .canonical_bytes()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    if bytes.len() > MAX_PACKAGE_RECORD_BYTES
        || Digest32::from_bytes(Sha256::digest(&bytes).into()) != intent.package_record_id
    {
        return Err(PackageObjectError::ExactMismatch);
    }
    let expectation = RegularExpectation {
        length: u64::try_from(bytes.len()).map_err(|_| PackageObjectError::ExactMismatch)?,
        sha256: intent.package_record_id.bytes(),
        exact_bytes: Some(&bytes),
    };
    ensure_bytes_regular_object(
        &runtime._records,
        &names::package_record_stage(intent.package_record_id),
        &names::package_record(intent.package_record_id),
        &bytes,
        expectation,
        true,
    )
}

fn canonical_package_record_bytes(
    capacity: &PackageObjectCapacity,
    prepared: &PreparedPackageView<'_>,
) -> Result<Vec<u8>, PackageObjectError> {
    let bytes = prepared
        .record
        .canonical_bytes()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    if bytes.len() > MAX_PACKAGE_RECORD_BYTES
        || Digest32::from_bytes(Sha256::digest(&bytes).into()) != capacity.package_record_id
    {
        return Err(PackageObjectError::ExactMismatch);
    }
    Ok(bytes)
}

#[derive(Clone, Copy)]
struct RegularExpectation<'expected> {
    length: u64,
    sha256: [u8; 32],
    exact_bytes: Option<&'expected [u8]>,
}

fn ensure_bytes_regular_object(
    records: &PrivateDirectory,
    stage_name: &PrivateComponent,
    final_name: &PrivateComponent,
    bytes: &[u8],
    expectation: RegularExpectation<'_>,
    expected_missing: bool,
) -> Result<(), PackageObjectError> {
    if u64::try_from(bytes.len()).ok() != Some(expectation.length)
        || <[u8; 32]>::from(Sha256::digest(bytes)) != expectation.sha256
    {
        return Err(PackageObjectError::ExactMismatch);
    }
    let existing = verify_optional_sealed_regular(records, final_name, expectation)?;
    if existing == expected_missing {
        return Err(PackageObjectError::Collision);
    }
    if existing {
        return Ok(());
    }

    let identity = records
        .write_new_synced(stage_name, bytes, byte_limit(expectation.length)?)
        .map_err(map_create_regular_error)?;
    if let Err(error) = verify_required_regular(records, stage_name, expectation, false) {
        if !object_error_is_terminal(error) {
            cleanup_regular_stage(records, stage_name)?;
        }
        return Err(error);
    }
    settle_regular_stage(records, stage_name, final_name, identity, expectation)
}

fn settle_regular_stage(
    records: &PrivateDirectory,
    stage_name: &PrivateComponent,
    final_name: &PrivateComponent,
    writable_identity: FileIdentity,
    expectation: RegularExpectation<'_>,
) -> Result<(), PackageObjectError> {
    let sealed_identity = match records.seal_verified_regular(stage_name) {
        Ok(Some(identity)) => identity,
        Ok(None) => return Err(PackageObjectError::SettlementAmbiguous),
        Err(error) if fs_error_is_terminal(error) => {
            return Err(PackageObjectError::SettlementAmbiguous)
        }
        Err(error) => {
            cleanup_regular_stage(records, stage_name)?;
            return Err(map_regular_fs(error));
        }
    };
    if sealed_identity != writable_identity {
        return Err(PackageObjectError::SettlementAmbiguous);
    }
    if let Err(error) = verify_required_regular(records, stage_name, expectation, true) {
        if !object_error_is_terminal(error) {
            cleanup_regular_stage(records, stage_name)?;
        }
        return Err(error);
    }

    match records.publish_noreplace_verified_regular(stage_name, final_name) {
        Ok(installed_identity) if installed_identity == sealed_identity => {}
        Ok(_) => return Err(PackageObjectError::SettlementAmbiguous),
        Err(error) if fs_error_is_terminal(error) => {
            return Err(PackageObjectError::SettlementAmbiguous)
        }
        Err(error) => {
            cleanup_regular_stage(records, stage_name)?;
            return Err(map_regular_fs(error));
        }
    }
    verify_required_regular(records, final_name, expectation, true).map_err(map_final_object_error)
}

fn verify_complete_final_closure(
    runtime: &MaterializationRuntime,
    prepared: &PreparedPackageView<'_>,
    package_record_bytes: &[u8],
) -> Result<Arc<SealedPrivateDirectory>, PackageObjectError> {
    let record = prepared.record;
    let index_expectation = RegularExpectation {
        length: record.tree_index.index_length,
        sha256: record.tree_index.index_sha256.bytes(),
        exact_bytes: Some(prepared.tree_index_bytes),
    };
    verify_required_regular(
        &runtime._records,
        &names::tree_index_object(record.tree_index.index_sha256),
        index_expectation,
        true,
    )
    .map_err(map_final_object_error)?;
    let reparsed = CanonicalExtensionTreeIndex::parse_canonical(prepared.tree_index_bytes)
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    let reparsed_view = PreparedPackageView {
        package_source: prepared.package_source,
        tree_index: &reparsed,
        tree_index_bytes: prepared.tree_index_bytes,
        manifest_bytes: prepared.manifest_bytes,
        record,
    };
    validate_prepared_package(&reparsed_view)?;

    verify_required_regular(
        &runtime._records,
        &names::legal_object(record.legal.sha256),
        RegularExpectation {
            length: record.legal.length,
            sha256: record.legal.sha256.bytes(),
            exact_bytes: None,
        },
        true,
    )
    .map_err(map_final_object_error)?;

    verify_required_regular(
        &runtime._records,
        &names::package_record(
            record
                .record_id()
                .map_err(|_| PackageObjectError::ExactMismatch)?,
        ),
        RegularExpectation {
            length: u64::try_from(package_record_bytes.len())
                .map_err(|_| PackageObjectError::ExactMismatch)?,
            sha256: Sha256::digest(package_record_bytes).into(),
            exact_bytes: Some(package_record_bytes),
        },
        true,
    )
    .map_err(map_final_object_error)?;
    let decoded = PackageRecord::decode(package_record_bytes)
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    if &decoded != record || decoded.record_id().ok() != record.record_id().ok() {
        return Err(PackageObjectError::ExactMismatch);
    }

    let tree_name = names::tree_object(record.tree_index.tree_sha256);
    let root = open_optional_sealed_tree(&runtime._trees, &tree_name)?
        .ok_or(PackageObjectError::Collision)?;
    let authenticated =
        verify_tree_against_record(root, &reparsed, record).map_err(map_final_object_error)?;
    Ok(authenticated.into_root())
}

fn verify_optional_sealed_regular(
    records: &PrivateDirectory,
    name: &PrivateComponent,
    expectation: RegularExpectation<'_>,
) -> Result<bool, PackageObjectError> {
    verify_regular(records, name, expectation, true).map_err(map_final_object_error)
}

fn verify_required_regular(
    records: &PrivateDirectory,
    name: &PrivateComponent,
    expectation: RegularExpectation<'_>,
    sealed: bool,
) -> Result<(), PackageObjectError> {
    if verify_regular(records, name, expectation, sealed)? {
        Ok(())
    } else {
        Err(PackageObjectError::ExactMismatch)
    }
}

fn verify_regular(
    records: &PrivateDirectory,
    name: &PrivateComponent,
    expectation: RegularExpectation<'_>,
    sealed: bool,
) -> Result<bool, PackageObjectError> {
    let limit = byte_limit(expectation.length)?;
    let nested = if sealed {
        records.with_bounded_sealed_regular_reader(name, limit, |reader| {
            read_regular_proof(reader, expectation)
        })
    } else {
        records.with_bounded_regular_reader(name, limit, |reader| {
            read_regular_proof(reader, expectation)
        })
    }
    .map_err(map_regular_fs)?;

    let Some(proof) = nested else {
        return Ok(false);
    };
    let proof = proof.map_err(|_| PackageObjectError::Filesystem(PrivateFsError::Io))?;
    if proof.length != expectation.length
        || proof.sha256 != expectation.sha256
        || !proof.exact_bytes
    {
        return Err(PackageObjectError::ExactMismatch);
    }
    Ok(true)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RegularProof {
    length: u64,
    sha256: [u8; 32],
    exact_bytes: bool,
}

fn read_regular_proof(
    reader: &mut dyn Read,
    expectation: RegularExpectation<'_>,
) -> io::Result<RegularProof> {
    let mut digest = Sha256::new();
    let mut length = 0_u64;
    let mut exact_bytes = true;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) if read <= buffer.len() => read,
            Ok(_) => return Err(io::Error::other("regular reader over-reported a read")),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if read == 0 {
            break;
        }
        let start = usize::try_from(length)
            .map_err(|_| io::Error::other("regular object byte count overflow"))?;
        length = length
            .checked_add(
                u64::try_from(read)
                    .map_err(|_| io::Error::other("regular object byte count overflow"))?,
            )
            .ok_or_else(|| io::Error::other("regular object byte count overflow"))?;
        if let Some(expected) = expectation.exact_bytes {
            let end = start
                .checked_add(read)
                .ok_or_else(|| io::Error::other("regular object byte count overflow"))?;
            exact_bytes &= expected
                .get(start..end)
                .is_some_and(|slice| slice == &buffer[..read]);
        }
        digest.update(&buffer[..read]);
    }
    if let Some(expected) = expectation.exact_bytes {
        exact_bytes &= u64::try_from(expected.len()).ok() == Some(length);
    }
    Ok(RegularProof {
        length,
        sha256: digest.finalize().into(),
        exact_bytes,
    })
}

fn cleanup_regular_stage(
    records: &PrivateDirectory,
    stage_name: &PrivateComponent,
) -> Result<(), PackageObjectError> {
    match records.remove_verified_regular(stage_name) {
        Ok(true) => Ok(()),
        Ok(false) | Err(_) => Err(PackageObjectError::SettlementAmbiguous),
    }
}

fn streaming_length(length: u64) -> Result<StreamingFileLength, PackageObjectError> {
    StreamingFileLength::new(length).map_err(map_regular_fs)
}

fn byte_limit(length: u64) -> Result<ByteLimit, PackageObjectError> {
    let length = usize::try_from(length).map_err(|_| PackageObjectError::ExactMismatch)?;
    ByteLimit::new(length.max(1)).map_err(map_regular_fs)
}

fn map_streaming_write(error: StreamingWriteError) -> PackageObjectError {
    match error {
        StreamingWriteError::Filesystem(error) => map_create_regular_error(error),
        StreamingWriteError::SourceRead => {
            PackageObjectError::Source(BundledReleaseSourceError::Io)
        }
        StreamingWriteError::SourceTooShort
        | StreamingWriteError::SourceTooLong
        | StreamingWriteError::SinkLengthMismatch => PackageObjectError::ExactMismatch,
    }
}

fn map_create_regular_error(error: PrivateFsError) -> PackageObjectError {
    match error {
        PrivateFsError::AlreadyExists | PrivateFsError::Unsafe => PackageObjectError::Collision,
        PrivateFsError::BoundExceeded => PackageObjectError::ExactMismatch,
        error if fs_error_is_terminal(error) => PackageObjectError::SettlementAmbiguous,
        error => PackageObjectError::Filesystem(error),
    }
}

fn map_regular_fs(error: PrivateFsError) -> PackageObjectError {
    match error {
        PrivateFsError::AlreadyExists | PrivateFsError::Unsafe => PackageObjectError::Collision,
        PrivateFsError::NotFound | PrivateFsError::BoundExceeded => {
            PackageObjectError::ExactMismatch
        }
        error if fs_error_is_terminal(error) => PackageObjectError::SettlementAmbiguous,
        error => PackageObjectError::Filesystem(error),
    }
}

const fn map_final_object_error(error: PackageObjectError) -> PackageObjectError {
    match error {
        PackageObjectError::ExactMismatch => PackageObjectError::Collision,
        error => error,
    }
}

const fn fs_error_is_terminal(error: PrivateFsError) -> bool {
    matches!(
        error,
        PrivateFsError::IdentityAmbiguous
            | PrivateFsError::SettlementUnknown
            | PrivateFsError::Quarantined
    )
}

const fn object_error_is_terminal(error: PackageObjectError) -> bool {
    matches!(error, PackageObjectError::SettlementAmbiguous)
}

struct DigestingReader<'reader> {
    inner: &'reader mut dyn Read,
    digest: Sha256,
    length: u64,
}

impl<'reader> DigestingReader<'reader> {
    fn new(inner: &'reader mut dyn Read) -> Self {
        Self {
            inner,
            digest: Sha256::new(),
            length: 0,
        }
    }

    fn finish(self) -> RegularProof {
        RegularProof {
            length: self.length,
            sha256: self.digest.finalize().into(),
            exact_bytes: true,
        }
    }
}

impl Read for DigestingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        if read > buffer.len() {
            return Err(io::Error::other("bundled source over-reported a read"));
        }
        self.length = self
            .length
            .checked_add(
                u64::try_from(read)
                    .map_err(|_| io::Error::other("bundled source byte count overflow"))?,
            )
            .ok_or_else(|| io::Error::other("bundled source byte count overflow"))?;
        self.digest.update(&buffer[..read]);
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn regular_proof_checks_digest_length_and_exact_bytes_in_one_pass() {
        let bytes = b"exact immutable object";
        let expectation = RegularExpectation {
            length: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
            exact_bytes: Some(bytes),
        };
        let mut reader = Cursor::new(bytes);
        let proof = read_regular_proof(&mut reader, expectation).unwrap();
        assert_eq!(proof.length, expectation.length);
        assert_eq!(proof.sha256, expectation.sha256);
        assert!(proof.exact_bytes);

        let mut different = Cursor::new(b"exact immutable objecu");
        let proof = read_regular_proof(&mut different, expectation).unwrap();
        assert_eq!(proof.length, expectation.length);
        assert_ne!(proof.sha256, expectation.sha256);
        assert!(!proof.exact_bytes);
    }

    #[test]
    fn digesting_reader_binds_empty_and_chunked_sources() {
        for bytes in [&b""[..], &b"streamed legal bytes"[..]] {
            let mut source = Cursor::new(bytes);
            let mut reader = DigestingReader::new(&mut source);
            let mut observed = Vec::new();
            reader.read_to_end(&mut observed).unwrap();
            let proof = reader.finish();
            assert_eq!(observed, bytes);
            assert_eq!(proof.length, bytes.len() as u64);
            assert_eq!(proof.sha256, <[u8; 32]>::from(Sha256::digest(bytes)));
        }
    }

    #[test]
    fn terminal_filesystem_failures_never_project_a_reusable_namespace() {
        for error in [
            PrivateFsError::IdentityAmbiguous,
            PrivateFsError::SettlementUnknown,
            PrivateFsError::Quarantined,
        ] {
            assert_eq!(
                map_regular_fs(error),
                PackageObjectError::SettlementAmbiguous
            );
            assert_eq!(
                map_create_regular_error(error),
                PackageObjectError::SettlementAmbiguous
            );
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    mod native {
        use std::fs;

        use zephium_private_fs::LockedPrivateNamespace;

        use super::*;

        fn private_records() -> (tempfile::TempDir, LockedPrivateNamespace, PrivateDirectory) {
            let parent = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let namespace =
                LockedPrivateNamespace::open_or_create(parent.path().join("repository")).unwrap();
            let records = namespace
                .directory()
                .create_new_private_child(&PrivateComponent::new("records").unwrap())
                .unwrap();
            (parent, namespace, records)
        }

        #[test]
        fn regular_object_is_published_sealed_reused_and_freshly_reread() {
            let (_parent, _namespace, records) = private_records();
            let stage = PrivateComponent::new("fixture.stage").unwrap();
            let final_name = PrivateComponent::new("fixture.object").unwrap();
            let bytes = b"content-addressed bytes";
            let expectation = RegularExpectation {
                length: bytes.len() as u64,
                sha256: Sha256::digest(bytes).into(),
                exact_bytes: Some(bytes),
            };

            ensure_bytes_regular_object(&records, &stage, &final_name, bytes, expectation, true)
                .unwrap();
            assert_eq!(
                records.read_bounded_regular(&stage, ByteLimit::new(1).unwrap()),
                Ok(None)
            );
            verify_required_regular(&records, &final_name, expectation, true).unwrap();

            ensure_bytes_regular_object(&records, &stage, &final_name, bytes, expectation, false)
                .unwrap();
            assert_eq!(
                verify_optional_sealed_regular(&records, &final_name, expectation),
                Ok(true)
            );

            let different = b"different addressed bytes";
            let different_expectation = RegularExpectation {
                length: different.len() as u64,
                sha256: Sha256::digest(different).into(),
                exact_bytes: Some(different),
            };
            assert_eq!(
                verify_optional_sealed_regular(&records, &final_name, different_expectation,),
                Err(PackageObjectError::Collision)
            );
        }

        #[test]
        fn wrong_mode_final_and_unsettled_presence_fail_closed() {
            let (_parent, _namespace, records) = private_records();
            let final_name = PrivateComponent::new("wrong-mode.object").unwrap();
            let bytes = b"immutable";
            records
                .write_new_synced(&final_name, bytes, ByteLimit::new(bytes.len()).unwrap())
                .unwrap();
            let expectation = RegularExpectation {
                length: bytes.len() as u64,
                sha256: Sha256::digest(bytes).into(),
                exact_bytes: Some(bytes),
            };
            assert_eq!(
                verify_optional_sealed_regular(&records, &final_name, expectation),
                Err(PackageObjectError::Collision)
            );

            let stage = PrivateComponent::new("sealed.stage").unwrap();
            records
                .write_new_synced(&stage, bytes, ByteLimit::new(bytes.len()).unwrap())
                .unwrap();
            records.seal_verified_regular(&stage).unwrap().unwrap();
            cleanup_regular_stage(&records, &stage).unwrap();
            assert_eq!(
                records.read_bounded_regular(&stage, ByteLimit::new(1).unwrap()),
                Ok(None)
            );
            assert_eq!(
                cleanup_regular_stage(&records, &stage),
                Err(PackageObjectError::SettlementAmbiguous)
            );
        }
    }
}
