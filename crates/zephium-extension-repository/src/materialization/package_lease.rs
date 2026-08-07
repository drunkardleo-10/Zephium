//! Fresh repository-owned package admission for runtime leases.

use std::mem::size_of;
use std::sync::Arc;

use thiserror::Error;
use zephium_core::extensions::{
    ExtensionCatalogGenerationRole, ExtensionGrantBrowsingContext, ExtensionGrantCohortError,
    ExtensionGrantManifestBinding, ExtensionGrantManifestBindings, ExtensionInstall,
    ExtensionInstallCatalog, ExtensionManifestDescriptor, ExtensionPackageIdentity,
    ExtensionPackageKey, ExtensionPackagePinAcquisitionBinding, ExtensionRuntimeBackendTarget,
    ExtensionRuntimeEligibility,
};
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, BundledCatalogAdmissionError,
    BundledPackageAuthority, ProductAdmittedExtensionManifest,
    ProductAdmittedRollbackExtensionManifest, ProductBundledCatalogGenerationRole,
    ProductExtensionManifestAdmissionError, ProductExtensionManifestAuthority,
    ProductExtensionManifestAuthorityError, ProductExtensionRuntimeTarget,
    MAX_PRODUCT_ADMITTED_EXTENSION_MANIFEST_RETAINED_BYTES,
};
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, ChromiumManifestKey, ExtensionReleaseCatalog,
    ExtensionReleaseCatalogRevision, PortableRelativePath, MAX_EXTENSION_MANIFEST_BYTES,
    MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES,
};
use zephium_private_fs::{DirectoryIdentity, SealedPrivateDirectory};

use super::cleanup::{validate_resumable_build_projection, CleanupError};
use super::names::{catalog_set_record, package_record, tree_index_object, tree_object};
use super::objects::{
    preflight_package_object_capacity, verify_completed_active_package,
    verify_completed_rollback_package, PackageObjectError, PackageObjectIntentDisposition,
};
use super::prepare::{
    open_product_manifest_authority, prepare_active_package_from_preparsed,
    prepare_rollback_package_from_preparsed, PreparationError,
};
use super::records::{
    CatalogSetRecord, PackageRecord, MAX_CATALOG_SET_RECORD_BYTES, MAX_PACKAGE_RECORD_BYTES,
};
use super::state::{DurablePackagePin, HistoricalCatalogRole, StoredBrowsingContext};
use super::storage::read_required_sealed_record;
use super::tree_reader::{with_verified_tree_resource, TreeResourceError};
use super::{MaterializationRuntime, MAX_MATERIALIZATION_STATE_BYTES};
use crate::state::Digest32;
use crate::ExtensionRepositoryError;

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
std::thread_local! {
    static REPOSITORY_PACKAGE_IO_COUNT: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

/// Hard logical ceiling for one shared immutable lease snapshot.
pub(crate) const MAX_PACKAGE_LEASE_SNAPSHOT_RETAINED_BYTES: usize =
    MAX_PRODUCT_ADMITTED_EXTENSION_MANIFEST_RETAINED_BYTES
        + MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES
        + 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VerifiedCatalogRole {
    Active,
    Rollback,
}

/// Stable classification while joining a fresh product-verified package
/// snapshot to one Store-native acquisition row.
///
/// Caller-owned binding mismatches are clean refusals. `DurableRowMismatch`
/// reports repository incoherence and must follow the existing fail-closed
/// snapshot path.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum PackagePinAdmissionError {
    #[error("private extension package pins are not supported yet")]
    PrivateUnsupported,
    #[error("extension package-pin catalog selection is stale")]
    StaleCatalogSet,
    #[error("extension package-pin catalog role differs")]
    WrongCatalogRole,
    #[error("extension package-pin package identity differs")]
    PackageMismatch,
    #[error("extension package-pin runtime backend differs")]
    RuntimeBackendMismatch,
    #[error("extension package-pin durable row differs")]
    DurableRowMismatch,
}

/// Failure while atomically loading and Store-binding one authenticated pin.
#[derive(Debug, Error)]
pub(crate) enum PackagePinLoadError {
    #[error("extension package snapshot authentication failed")]
    Snapshot(#[from] SnapshotLoadError),
    #[error("extension package snapshot differs from the Store acquisition row")]
    Admission(#[from] PackagePinAdmissionError),
}

/// Normalized, non-serializable proof that one freshly authenticated package
/// snapshot and one Store acquisition row describe the same exact owner.
///
/// Product authority stays in this admission layer. The transaction layer
/// accepts only this closed projection and rechecks its durable set row before
/// committing the pin.
#[must_use = "a verified package-pin admission must be committed or discarded"]
pub(crate) struct VerifiedPackagePinAdmission {
    repository: PackageLeaseRepositoryIdentity,
    current_catalog_set_id: Digest32,
    package_key: Digest32,
    runtime_backend: ExtensionRuntimeBackendTarget,
    pin: DurablePackagePin,
}

impl VerifiedPackagePinAdmission {
    pub(crate) const fn repository(&self) -> PackageLeaseRepositoryIdentity {
        self.repository
    }

    pub(crate) const fn current_catalog_set_id(&self) -> Digest32 {
        self.current_catalog_set_id
    }

    pub(crate) const fn package_key(&self) -> Digest32 {
        self.package_key
    }

    pub(crate) const fn runtime_backend(&self) -> ExtensionRuntimeBackendTarget {
        self.runtime_backend
    }

    pub(crate) const fn pin(&self) -> DurablePackagePin {
        self.pin
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackageLeaseRepositoryIdentity {
    pub(crate) root: DirectoryIdentity,
    pub(crate) records: DirectoryIdentity,
    pub(crate) trees: DirectoryIdentity,
}

#[derive(Clone)]
pub(crate) struct CurrentCatalogSetProjection {
    identity: Digest32,
    role: VerifiedCatalogRole,
    record: CatalogSetRecord,
    repository: PackageLeaseRepositoryIdentity,
    build_in_progress: bool,
}

impl CurrentCatalogSetProjection {
    pub(crate) const fn identity(&self) -> Digest32 {
        self.identity
    }

    pub(crate) const fn role(&self) -> VerifiedCatalogRole {
        self.role
    }

    pub(crate) const fn catalog_digest(&self) -> Digest32 {
        self.record.catalog.catalog_sha256
    }

    pub(crate) fn generation_anchor(
        &self,
    ) -> Result<zephium_extension_authority::BundledCatalogGenerationAnchor, SnapshotLoadError>
    {
        self.record
            .catalog
            .generation_anchor()
            .map_err(SnapshotLoadError::Repository)
    }

    pub(crate) const fn repository(&self) -> PackageLeaseRepositoryIdentity {
        self.repository
    }

    pub(crate) const fn build_in_progress(&self) -> bool {
        self.build_in_progress
    }

    fn package_row(
        &self,
        package_key: ExtensionPackageKey,
    ) -> Option<super::records::CatalogSetPackageRow> {
        let key = Digest32::from_bytes(package_key.bytes());
        self.record
            .packages
            .binary_search_by_key(&key, |row| row.package_key)
            .ok()
            .and_then(|index| self.record.packages.get(index))
            .copied()
    }
}

#[derive(Debug, Error)]
pub(crate) enum SnapshotLoadError {
    #[error("repository operation failed")]
    Repository(#[from] ExtensionRepositoryError),
    #[error("bundled catalog authority failed")]
    CatalogAuthority(#[source] BundledCatalogAdmissionError),
    #[error("bundled catalog admission failed")]
    CatalogAdmission(#[source] BundledCatalogAdmissionError),
    #[error("manifest authority failed")]
    ManifestAuthority(#[source] ProductExtensionManifestAuthorityError),
    #[error("manifest admission failed")]
    ManifestAdmission(#[source] ProductExtensionManifestAdmissionError),
    #[error("package preparation failed")]
    Preparation(#[source] PreparationError),
    #[error("package object verification failed")]
    Object {
        phase: SnapshotObjectPhase,
        #[source]
        error: PackageObjectError,
    },
    #[error("current selection changed")]
    StaleSelection,
    #[error("package is not selected")]
    PackageNotSelected,
    #[error("catalog has the wrong generation role")]
    WrongRole,
    #[error("runtime eligibility differs from the admitted package")]
    EligibilityMismatch,
    #[error("the supplied install package differs from the admitted package")]
    InstallPackageMismatch,
    #[error("package is not completely materialized")]
    PackageNotMaterialized,
    #[error("durable package objects differ from their authenticated identity")]
    DurableMismatch,
    #[error("lease snapshot accounting exceeded its fixed bound")]
    AccountingOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SnapshotObjectPhase {
    Preflight { had_intent: bool },
    Completed,
}

pub(crate) fn current_catalog_set_projection(
    runtime: &MaterializationRuntime,
) -> Result<Option<CurrentCatalogSetProjection>, SnapshotLoadError> {
    let build_in_progress = validated_resumable_build_in_progress(runtime)?;
    let Some(identity) = runtime._state.current_catalog_set_id else {
        return Ok(None);
    };
    let expected = runtime
        ._catalog_sets
        .get(&identity)
        .ok_or(SnapshotLoadError::DurableMismatch)?;
    let bytes = read_required_sealed_record(
        &runtime._records,
        &catalog_set_record(identity),
        MAX_CATALOG_SET_RECORD_BYTES,
    )?;
    let record = CatalogSetRecord::decode(&bytes)?;
    if record.record_id()? != identity || &record != expected {
        return Err(SnapshotLoadError::DurableMismatch);
    }

    let authority =
        BundledPackageAuthority::product().map_err(SnapshotLoadError::CatalogAuthority)?;
    let role = match authority
        .recognize_generation(&record.catalog.generation_anchor()?)
        .ok_or(SnapshotLoadError::DurableMismatch)?
    {
        ProductBundledCatalogGenerationRole::Active => VerifiedCatalogRole::Active,
        ProductBundledCatalogGenerationRole::Rollback => VerifiedCatalogRole::Rollback,
    };
    Ok(Some(CurrentCatalogSetProjection {
        identity,
        role,
        record,
        repository: PackageLeaseRepositoryIdentity {
            root: runtime._root.identity(),
            records: runtime._records.identity(),
            trees: runtime._trees.identity(),
        },
        build_in_progress,
    }))
}

pub(crate) fn validated_resumable_build_in_progress(
    runtime: &MaterializationRuntime,
) -> Result<bool, SnapshotLoadError> {
    runtime._state.validate()?;
    let canonical = crate::codec::encode(&runtime._state, MAX_MATERIALIZATION_STATE_BYTES)
        .map_err(|_| SnapshotLoadError::DurableMismatch)?;
    if canonical != runtime._state_bytes || !runtime.intent_projection_is_exact() {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    let build_in_progress = runtime._state.build_intent.is_some()
        || runtime._build_intent.is_some()
        || runtime._build_stage.is_some()
        || !runtime._record_stages.is_empty();
    if build_in_progress {
        validate_resumable_build_projection(runtime).map_err(map_build_validation_error)?;
    }
    Ok(build_in_progress)
}

fn map_build_validation_error(error: CleanupError) -> SnapshotLoadError {
    match error {
        CleanupError::BuildStateMismatch
        | CleanupError::ExactMismatch
        | CleanupError::CommitMarkerPresent => SnapshotLoadError::DurableMismatch,
        CleanupError::SettlementAmbiguous => {
            SnapshotLoadError::Repository(ExtensionRepositoryError::SettlementAmbiguous)
        }
        CleanupError::Filesystem(error) => {
            SnapshotLoadError::Repository(ExtensionRepositoryError::FileSystem(error))
        }
    }
}

pub(crate) struct VerifiedActivePackageSnapshot {
    repository: PackageLeaseRepositoryIdentity,
    record_id: Digest32,
    record: PackageRecord,
    index: CanonicalExtensionTreeIndex,
    root: Arc<SealedPrivateDirectory>,
    manifest: ProductAdmittedExtensionManifest,
    retained_bytes: usize,
}

pub(crate) struct VerifiedRollbackPackageSnapshot {
    repository: PackageLeaseRepositoryIdentity,
    record_id: Digest32,
    record: PackageRecord,
    index: CanonicalExtensionTreeIndex,
    root: Arc<SealedPrivateDirectory>,
    manifest: ProductAdmittedRollbackExtensionManifest,
    retained_bytes: usize,
}

macro_rules! impl_snapshot_projection {
    ($snapshot:ident, $manifest:ty) => {
        impl $snapshot {
            pub(crate) const fn record_id(&self) -> Digest32 {
                self.record_id
            }

            pub(crate) const fn package(&self) -> &ExtensionPackageIdentity {
                self.manifest.package_identity()
            }

            pub(crate) const fn descriptor(&self) -> &ExtensionManifestDescriptor {
                self.manifest.descriptor()
            }

            pub(crate) const fn catalog_revision(&self) -> ExtensionReleaseCatalogRevision {
                self.manifest.catalog_revision()
            }

            pub(crate) const fn runtime_target(
                &self,
            ) -> zephium_extension_authority::ProductExtensionRuntimeTarget {
                self.manifest.runtime_target()
            }

            pub(crate) const fn chromium_key(&self) -> Option<&ChromiumManifestKey> {
                self.manifest.chromium_key()
            }

            pub(crate) const fn index(&self) -> &CanonicalExtensionTreeIndex {
                &self.index
            }

            pub(crate) const fn root(&self) -> &Arc<SealedPrivateDirectory> {
                &self.root
            }

            pub(crate) const fn retained_bytes(&self) -> usize {
                self.retained_bytes
            }

            pub(crate) fn exactly_matches(&self, other: &Self) -> bool {
                self.repository == other.repository
                    && self.record_id == other.record_id
                    && self.record == other.record
                    && self.index == other.index
                    && self.root.identity() == other.root.identity()
                    && self.manifest.runtime_target() == other.manifest.runtime_target()
                    && self.manifest.catalog_authority() == other.manifest.catalog_authority()
                    && self.manifest.catalog_revision() == other.manifest.catalog_revision()
                    && self.manifest.catalog_length() == other.manifest.catalog_length()
                    && self.manifest.catalog_digest() == other.manifest.catalog_digest()
                    && self.manifest.catalog_inventory_digest()
                        == other.manifest.catalog_inventory_digest()
                    && self.manifest.admission_digest() == other.manifest.admission_digest()
                    && self.manifest.chromium_key() == other.manifest.chromium_key()
                    && self.manifest.descriptor() == other.manifest.descriptor()
            }
        }
    };
}

impl_snapshot_projection!(
    VerifiedActivePackageSnapshot,
    ProductAdmittedExtensionManifest
);
impl_snapshot_projection!(
    VerifiedRollbackPackageSnapshot,
    ProductAdmittedRollbackExtensionManifest
);

pub(crate) fn verify_active_package_pin_admission(
    current: &CurrentCatalogSetProjection,
    snapshot: &VerifiedActivePackageSnapshot,
    binding: &ExtensionPackagePinAcquisitionBinding,
) -> Result<VerifiedPackagePinAdmission, PackagePinAdmissionError> {
    verify_package_pin_admission(
        current,
        snapshot.repository,
        snapshot.record_id,
        snapshot.package(),
        snapshot.runtime_target(),
        VerifiedCatalogRole::Active,
        binding,
    )
}

pub(crate) fn verify_rollback_package_pin_admission(
    current: &CurrentCatalogSetProjection,
    snapshot: &VerifiedRollbackPackageSnapshot,
    binding: &ExtensionPackagePinAcquisitionBinding,
) -> Result<VerifiedPackagePinAdmission, PackagePinAdmissionError> {
    verify_package_pin_admission(
        current,
        snapshot.repository,
        snapshot.record_id,
        snapshot.package(),
        snapshot.runtime_target(),
        VerifiedCatalogRole::Rollback,
        binding,
    )
}

#[allow(clippy::too_many_arguments)]
fn verify_package_pin_admission(
    current: &CurrentCatalogSetProjection,
    snapshot_repository: PackageLeaseRepositoryIdentity,
    record_id: Digest32,
    package: &ExtensionPackageIdentity,
    runtime_target: ProductExtensionRuntimeTarget,
    expected_role: VerifiedCatalogRole,
    binding: &ExtensionPackagePinAcquisitionBinding,
) -> Result<VerifiedPackagePinAdmission, PackagePinAdmissionError> {
    if binding.browsing_context() != ExtensionGrantBrowsingContext::Regular {
        return Err(PackagePinAdmissionError::PrivateUnsupported);
    }
    if current.identity.bytes() != binding.catalog_set_digest().bytes() {
        return Err(PackagePinAdmissionError::StaleCatalogSet);
    }
    let (binding_role, historical_role) = match expected_role {
        VerifiedCatalogRole::Active => (
            ExtensionCatalogGenerationRole::Active,
            HistoricalCatalogRole::Active,
        ),
        VerifiedCatalogRole::Rollback => (
            ExtensionCatalogGenerationRole::Rollback,
            HistoricalCatalogRole::Rollback,
        ),
    };
    if current.role != expected_role || binding.catalog_role() != binding_role {
        return Err(PackagePinAdmissionError::WrongCatalogRole);
    }
    if binding.package() != package {
        return Err(PackagePinAdmissionError::PackageMismatch);
    }
    let row = current
        .package_row(package.key())
        .ok_or(PackagePinAdmissionError::DurableRowMismatch)?;
    if snapshot_repository != current.repository
        || row.package_record_id != record_id
        || row.runtime_target.product_target() != runtime_target
    {
        return Err(PackagePinAdmissionError::DurableRowMismatch);
    }
    if !row
        .runtime_target
        .matches_runtime_backend(binding.runtime_backend())
    {
        return Err(PackagePinAdmissionError::RuntimeBackendMismatch);
    }
    let pin = DurablePackagePin {
        profile_id: binding.profile(),
        install_id: binding.install_id(),
        browsing_context: StoredBrowsingContext::from(binding.browsing_context()),
        catalog_set_record_id: current.identity,
        catalog_role: historical_role,
        package_record_id: record_id,
        native_incarnation: binding.native_incarnation().get(),
    };
    Ok(VerifiedPackagePinAdmission {
        repository: current.repository,
        current_catalog_set_id: current.identity,
        package_key: row.package_key,
        runtime_backend: binding.runtime_backend(),
        pin,
    })
}

/// Loads and binds one active snapshot without exposing an independently
/// supplied eligibility value to repository orchestration.
pub(crate) fn load_active_package_pin_admission(
    runtime: &MaterializationRuntime,
    current: &CurrentCatalogSetProjection,
    exact_catalog_bytes: &[u8],
    binding: &ExtensionPackagePinAcquisitionBinding,
) -> Result<(VerifiedActivePackageSnapshot, VerifiedPackagePinAdmission), PackagePinLoadError> {
    let snapshot =
        load_active_package_snapshot(runtime, current, exact_catalog_bytes, binding.eligibility())?;
    let admission = verify_active_package_pin_admission(current, &snapshot, binding)?;
    Ok((snapshot, admission))
}

/// Loads and binds one rollback snapshot without exposing an independently
/// supplied eligibility value to repository orchestration.
pub(crate) fn load_rollback_package_pin_admission(
    runtime: &MaterializationRuntime,
    current: &CurrentCatalogSetProjection,
    exact_catalog_bytes: &[u8],
    binding: &ExtensionPackagePinAcquisitionBinding,
) -> Result<(VerifiedRollbackPackageSnapshot, VerifiedPackagePinAdmission), PackagePinLoadError> {
    let snapshot = load_rollback_package_snapshot(
        runtime,
        current,
        exact_catalog_bytes,
        binding.eligibility(),
    )?;
    let admission = verify_rollback_package_pin_admission(current, &snapshot, binding)?;
    Ok((snapshot, admission))
}

pub(crate) fn load_active_package_snapshot(
    runtime: &MaterializationRuntime,
    current: &CurrentCatalogSetProjection,
    exact_catalog_bytes: &[u8],
    eligibility: &ExtensionRuntimeEligibility,
) -> Result<VerifiedActivePackageSnapshot, SnapshotLoadError> {
    if current.role != VerifiedCatalogRole::Active {
        return Err(SnapshotLoadError::WrongRole);
    }
    let authority =
        BundledPackageAuthority::product().map_err(SnapshotLoadError::CatalogAuthority)?;
    let catalog = authority
        .admit_catalog(exact_catalog_bytes)
        .map_err(SnapshotLoadError::CatalogAdmission)?;
    require_eligibility_package(catalog.catalog(), eligibility)?;
    let LoadedRepositoryPackage {
        record_id,
        record: durable_record,
        index,
        index_bytes,
        manifest_bytes,
    } = load_repository_package(runtime, current, eligibility.package().key())?;
    let manifest_authority = open_product_manifest_authority().map_err(map_preparation_error)?;
    let prepared = prepare_active_package_from_preparsed(
        &catalog,
        &manifest_authority,
        durable_record.manifest.runtime_target.product_target(),
        eligibility.package().key(),
        index,
        index_bytes,
        manifest_bytes,
    )
    .map_err(map_preparation_error)?;
    require_eligibility(
        prepared.manifest().package_identity(),
        prepared.manifest().descriptor(),
        eligibility,
    )?;
    if prepared.record() != &durable_record || prepared.record().record_id()? != record_id {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    let had_intent = runtime._build_intent.is_some();
    let capacity =
        preflight_package_object_capacity(runtime, prepared.record()).map_err(|error| {
            SnapshotLoadError::Object {
                phase: SnapshotObjectPhase::Preflight { had_intent },
                error,
            }
        })?;
    if capacity.intent_disposition() != PackageObjectIntentDisposition::CompletedReplay {
        return Err(SnapshotLoadError::PackageNotMaterialized);
    }
    let completed =
        verify_completed_active_package(runtime, capacity, prepared).map_err(|error| {
            SnapshotLoadError::Object {
                phase: SnapshotObjectPhase::Completed,
                error,
            }
        })?;
    let (verified_id, record, root, records_parent, trees_parent, prepared) =
        completed.into_parts();
    if verified_id != record_id
        || records_parent != current.repository.records
        || trees_parent != current.repository.trees
    {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    let (index, manifest) = prepared.into_lease_parts();
    let retained_bytes = snapshot_retained_bytes::<VerifiedActivePackageSnapshot>(
        manifest.retained_bytes(),
        index.retained_bytes(),
        &record,
    )?;
    Ok(VerifiedActivePackageSnapshot {
        repository: current.repository,
        record_id,
        record,
        index,
        root,
        manifest,
        retained_bytes,
    })
}

pub(crate) fn load_active_manifest_bindings(
    runtime: &MaterializationRuntime,
    current: &CurrentCatalogSetProjection,
    exact_catalog_bytes: &[u8],
    installs: &ExtensionInstallCatalog,
) -> Result<ExtensionGrantManifestBindings, SnapshotLoadError> {
    if current.role != VerifiedCatalogRole::Active {
        return Err(SnapshotLoadError::WrongRole);
    }
    let authority =
        BundledPackageAuthority::product().map_err(SnapshotLoadError::CatalogAuthority)?;
    let catalog = authority
        .admit_catalog(exact_catalog_bytes)
        .map_err(SnapshotLoadError::CatalogAdmission)?;
    require_catalog_anchor(current, catalog.generation_anchor())?;
    if installs.installs().is_empty() {
        return finish_manifest_bindings(Vec::new());
    }
    let manifest_authority = open_product_manifest_authority().map_err(map_preparation_error)?;
    let mut bindings = Vec::with_capacity(installs.installs().len());
    for install in installs.installs() {
        let manifest = load_active_manifest_from_admitted(
            runtime,
            current,
            &catalog,
            &manifest_authority,
            install,
        )?;
        bindings.push(ExtensionGrantManifestBinding::new(install.id(), manifest));
    }
    finish_manifest_bindings(bindings)
}

pub(crate) fn load_rollback_package_snapshot(
    runtime: &MaterializationRuntime,
    current: &CurrentCatalogSetProjection,
    exact_catalog_bytes: &[u8],
    eligibility: &ExtensionRuntimeEligibility,
) -> Result<VerifiedRollbackPackageSnapshot, SnapshotLoadError> {
    if current.role != VerifiedCatalogRole::Rollback {
        return Err(SnapshotLoadError::WrongRole);
    }
    let authority =
        BundledPackageAuthority::product().map_err(SnapshotLoadError::CatalogAuthority)?;
    let catalog = authority
        .admit_rollback_catalog(exact_catalog_bytes)
        .map_err(SnapshotLoadError::CatalogAdmission)?;
    require_eligibility_package(catalog.catalog(), eligibility)?;
    let LoadedRepositoryPackage {
        record_id,
        record: durable_record,
        index,
        index_bytes,
        manifest_bytes,
    } = load_repository_package(runtime, current, eligibility.package().key())?;
    let manifest_authority = open_product_manifest_authority().map_err(map_preparation_error)?;
    let prepared = prepare_rollback_package_from_preparsed(
        &catalog,
        &manifest_authority,
        durable_record.manifest.runtime_target.product_target(),
        eligibility.package().key(),
        index,
        index_bytes,
        manifest_bytes,
    )
    .map_err(map_preparation_error)?;
    require_eligibility(
        prepared.manifest().package_identity(),
        prepared.manifest().descriptor(),
        eligibility,
    )?;
    if prepared.record() != &durable_record || prepared.record().record_id()? != record_id {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    let had_intent = runtime._build_intent.is_some();
    let capacity =
        preflight_package_object_capacity(runtime, prepared.record()).map_err(|error| {
            SnapshotLoadError::Object {
                phase: SnapshotObjectPhase::Preflight { had_intent },
                error,
            }
        })?;
    if capacity.intent_disposition() != PackageObjectIntentDisposition::CompletedReplay {
        return Err(SnapshotLoadError::PackageNotMaterialized);
    }
    let completed =
        verify_completed_rollback_package(runtime, capacity, prepared).map_err(|error| {
            SnapshotLoadError::Object {
                phase: SnapshotObjectPhase::Completed,
                error,
            }
        })?;
    let (verified_id, record, root, records_parent, trees_parent, prepared) =
        completed.into_parts();
    if verified_id != record_id
        || records_parent != current.repository.records
        || trees_parent != current.repository.trees
    {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    let (index, manifest) = prepared.into_lease_parts();
    let retained_bytes = snapshot_retained_bytes::<VerifiedRollbackPackageSnapshot>(
        manifest.retained_bytes(),
        index.retained_bytes(),
        &record,
    )?;
    Ok(VerifiedRollbackPackageSnapshot {
        repository: current.repository,
        record_id,
        record,
        index,
        root,
        manifest,
        retained_bytes,
    })
}

pub(crate) fn load_rollback_manifest_bindings(
    runtime: &MaterializationRuntime,
    current: &CurrentCatalogSetProjection,
    exact_catalog_bytes: &[u8],
    installs: &ExtensionInstallCatalog,
) -> Result<ExtensionGrantManifestBindings, SnapshotLoadError> {
    if current.role != VerifiedCatalogRole::Rollback {
        return Err(SnapshotLoadError::WrongRole);
    }
    let authority =
        BundledPackageAuthority::product().map_err(SnapshotLoadError::CatalogAuthority)?;
    let catalog = authority
        .admit_rollback_catalog(exact_catalog_bytes)
        .map_err(SnapshotLoadError::CatalogAdmission)?;
    require_catalog_anchor(current, catalog.generation_anchor())?;
    if installs.installs().is_empty() {
        return finish_manifest_bindings(Vec::new());
    }
    let manifest_authority = open_product_manifest_authority().map_err(map_preparation_error)?;
    let mut bindings = Vec::with_capacity(installs.installs().len());
    for install in installs.installs() {
        let manifest = load_rollback_manifest_from_admitted(
            runtime,
            current,
            &catalog,
            &manifest_authority,
            install,
        )?;
        bindings.push(ExtensionGrantManifestBinding::new(install.id(), manifest));
    }
    finish_manifest_bindings(bindings)
}

// Manifest bootstrap deliberately stops after exact catalog/package-record,
// tree-index, and manifest admission. Full tree closure verification belongs
// only to lease acquisition; running it here would make one profile cohort
// load proportional to every byte of every installed package.
fn load_active_manifest_from_admitted(
    runtime: &MaterializationRuntime,
    current: &CurrentCatalogSetProjection,
    catalog: &AdmittedBundledCatalog,
    manifest_authority: &ProductExtensionManifestAuthority,
    install: &ExtensionInstall,
) -> Result<Arc<ExtensionManifestDescriptor>, SnapshotLoadError> {
    require_install_package(catalog.catalog(), install)?;
    let LoadedRepositoryPackage {
        record_id,
        record: durable_record,
        index,
        index_bytes,
        manifest_bytes,
    } = load_repository_package(runtime, current, install.package().key())
        .map_err(map_bootstrap_package_load_error)?;
    let prepared = prepare_active_package_from_preparsed(
        catalog,
        manifest_authority,
        durable_record.manifest.runtime_target.product_target(),
        install.package().key(),
        index,
        index_bytes,
        manifest_bytes,
    )
    .map_err(map_preparation_error)?;
    if prepared.record() != &durable_record || prepared.record().record_id()? != record_id {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    let (_index, manifest) = prepared.into_lease_parts();
    if manifest.package_identity() != install.package() {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    Ok(Arc::new(manifest.descriptor().clone()))
}

fn load_rollback_manifest_from_admitted(
    runtime: &MaterializationRuntime,
    current: &CurrentCatalogSetProjection,
    catalog: &AdmittedRollbackBundledCatalog,
    manifest_authority: &ProductExtensionManifestAuthority,
    install: &ExtensionInstall,
) -> Result<Arc<ExtensionManifestDescriptor>, SnapshotLoadError> {
    require_install_package(catalog.catalog(), install)?;
    let LoadedRepositoryPackage {
        record_id,
        record: durable_record,
        index,
        index_bytes,
        manifest_bytes,
    } = load_repository_package(runtime, current, install.package().key())
        .map_err(map_bootstrap_package_load_error)?;
    let prepared = prepare_rollback_package_from_preparsed(
        catalog,
        manifest_authority,
        durable_record.manifest.runtime_target.product_target(),
        install.package().key(),
        index,
        index_bytes,
        manifest_bytes,
    )
    .map_err(map_preparation_error)?;
    if prepared.record() != &durable_record || prepared.record().record_id()? != record_id {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    let (_index, manifest) = prepared.into_lease_parts();
    if manifest.package_identity() != install.package() {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    Ok(Arc::new(manifest.descriptor().clone()))
}

// Caller-owned install identity is rejected before any repository package I/O.
// Once this succeeds, a missing current-set row or a different admitted
// package witness is repository incoherence rather than a clean caller error.
fn require_install_package(
    catalog: &ExtensionReleaseCatalog,
    install: &ExtensionInstall,
) -> Result<(), SnapshotLoadError> {
    let package = catalog
        .package(install.package().key())
        .ok_or(SnapshotLoadError::PackageNotSelected)?;
    if package.identity() != install.package() {
        return Err(SnapshotLoadError::InstallPackageMismatch);
    }
    Ok(())
}

fn require_eligibility_package(
    catalog: &ExtensionReleaseCatalog,
    eligibility: &ExtensionRuntimeEligibility,
) -> Result<(), SnapshotLoadError> {
    let package = catalog
        .package(eligibility.package().key())
        .ok_or(SnapshotLoadError::PackageNotSelected)?;
    if package.identity() != eligibility.package() {
        return Err(SnapshotLoadError::EligibilityMismatch);
    }
    Ok(())
}

fn map_bootstrap_package_load_error(error: SnapshotLoadError) -> SnapshotLoadError {
    match error {
        SnapshotLoadError::PackageNotSelected => SnapshotLoadError::DurableMismatch,
        other => other,
    }
}

fn require_catalog_anchor(
    current: &CurrentCatalogSetProjection,
    admitted: zephium_extension_authority::BundledCatalogGenerationAnchor,
) -> Result<(), SnapshotLoadError> {
    if current.generation_anchor()? != admitted {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    Ok(())
}

fn finish_manifest_bindings(
    bindings: Vec<ExtensionGrantManifestBinding>,
) -> Result<ExtensionGrantManifestBindings, SnapshotLoadError> {
    ExtensionGrantManifestBindings::new(bindings).map_err(|error| match error {
        ExtensionGrantCohortError::TooManyBindings { .. }
        | ExtensionGrantCohortError::AccountingOverflow
        | ExtensionGrantCohortError::RetainedBytesExceeded { .. } => {
            SnapshotLoadError::AccountingOverflow
        }
        ExtensionGrantCohortError::DuplicateBinding(_)
        | ExtensionGrantCohortError::IncompleteBindings
        | ExtensionGrantCohortError::ManifestPackageMismatch(_)
        | ExtensionGrantCohortError::TooManyAuthorities { .. }
        | ExtensionGrantCohortError::DuplicateAuthority(_)
        | ExtensionGrantCohortError::UnknownAuthority
        | ExtensionGrantCohortError::AuthorityMismatch(_) => SnapshotLoadError::DurableMismatch,
    })
}

struct LoadedRepositoryPackage {
    record_id: Digest32,
    record: PackageRecord,
    index: CanonicalExtensionTreeIndex,
    index_bytes: Box<[u8]>,
    manifest_bytes: Box<[u8]>,
}

fn load_repository_package(
    runtime: &MaterializationRuntime,
    current: &CurrentCatalogSetProjection,
    package_key: ExtensionPackageKey,
) -> Result<LoadedRepositoryPackage, SnapshotLoadError> {
    if runtime._state.current_catalog_set_id != Some(current.identity)
        || runtime._root.identity() != current.repository.root
        || runtime._records.identity() != current.repository.records
        || runtime._trees.identity() != current.repository.trees
    {
        return Err(SnapshotLoadError::StaleSelection);
    }
    let row = current
        .package_row(package_key)
        .ok_or(SnapshotLoadError::PackageNotSelected)?;
    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    REPOSITORY_PACKAGE_IO_COUNT.with(|count| {
        count.set(
            count
                .get()
                .checked_add(1)
                .expect("repository package I/O test count must fit usize"),
        );
    });
    let record_bytes = read_required_sealed_record(
        &runtime._records,
        &package_record(row.package_record_id),
        MAX_PACKAGE_RECORD_BYTES,
    )?;
    let record = PackageRecord::decode(&record_bytes)?;
    if record.record_id()? != row.package_record_id
        || runtime._package_records.get(&row.package_record_id) != Some(&record)
        || record.package.package_key != row.package_key
        || record.manifest.runtime_target != row.runtime_target
        || record.catalog != current.record.catalog
    {
        return Err(SnapshotLoadError::DurableMismatch);
    }

    let index_bytes = read_required_sealed_record(
        &runtime._records,
        &tree_index_object(record.tree_index.index_sha256),
        zephium_extension_package::MAX_EXTENSION_TREE_INDEX_BYTES,
    )?;
    let index = CanonicalExtensionTreeIndex::parse_canonical(&index_bytes)
        .map_err(|_| SnapshotLoadError::DurableMismatch)?;
    if index.index_sha256().bytes() != record.tree_index.index_sha256.bytes()
        || index.index_bytes() != record.tree_index.index_length
        || index.tree_sha256().bytes() != record.tree_index.tree_sha256.bytes()
    {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    let root = Arc::new(
        runtime
            ._trees
            .open_sealed_private_child(&tree_object(record.tree_index.tree_sha256))
            .map_err(ExtensionRepositoryError::FileSystem)?,
    );
    let manifest_path = PortableRelativePath::parse("manifest.json")
        .map_err(|_| SnapshotLoadError::DurableMismatch)?;
    let manifest_length = index
        .file(&manifest_path)
        .ok_or(SnapshotLoadError::DurableMismatch)?
        .length();
    if manifest_length == 0 || manifest_length > MAX_EXTENSION_MANIFEST_BYTES as u64 {
        return Err(SnapshotLoadError::DurableMismatch);
    }
    let manifest_capacity =
        usize::try_from(manifest_length).map_err(|_| SnapshotLoadError::AccountingOverflow)?;
    let manifest_bytes = with_verified_tree_resource(&root, &index, &manifest_path, |reader| {
        let mut bytes = Vec::with_capacity(manifest_capacity);
        reader.read_to_end(&mut bytes).map(|_| bytes)
    })
    .map_err(map_tree_resource)?
    .map_err(|_| SnapshotLoadError::DurableMismatch)?;

    Ok(LoadedRepositoryPackage {
        record_id: row.package_record_id,
        record,
        index,
        index_bytes: index_bytes.into_boxed_slice(),
        manifest_bytes: manifest_bytes.into_boxed_slice(),
    })
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn reset_repository_package_io_count() {
    REPOSITORY_PACKAGE_IO_COUNT.with(|count| count.set(0));
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn repository_package_io_count() -> usize {
    REPOSITORY_PACKAGE_IO_COUNT.with(std::cell::Cell::get)
}

fn require_eligibility(
    package: &ExtensionPackageIdentity,
    descriptor: &ExtensionManifestDescriptor,
    eligibility: &ExtensionRuntimeEligibility,
) -> Result<(), SnapshotLoadError> {
    if package != eligibility.package() || descriptor != eligibility.manifest() {
        return Err(SnapshotLoadError::EligibilityMismatch);
    }
    Ok(())
}

fn snapshot_retained_bytes<Snapshot>(
    manifest_bytes: usize,
    index_bytes: usize,
    record: &PackageRecord,
) -> Result<usize, SnapshotLoadError> {
    let retained = size_of::<Snapshot>()
        .checked_add(manifest_bytes)
        .and_then(|bytes| bytes.checked_add(index_bytes))
        .and_then(|bytes| bytes.checked_add(record.manifest.compatibility_target.len()))
        .and_then(|bytes| bytes.checked_add(record.legal.target.len()))
        .ok_or(SnapshotLoadError::AccountingOverflow)?;
    if retained > MAX_PACKAGE_LEASE_SNAPSHOT_RETAINED_BYTES {
        return Err(SnapshotLoadError::AccountingOverflow);
    }
    Ok(retained)
}

fn map_preparation_error(error: PreparationError) -> SnapshotLoadError {
    match error {
        PreparationError::ManifestAuthority(error) => SnapshotLoadError::ManifestAuthority(error),
        PreparationError::ManifestAdmission(error) => SnapshotLoadError::ManifestAdmission(error),
        other => SnapshotLoadError::Preparation(other),
    }
}

fn map_tree_resource(error: TreeResourceError) -> SnapshotLoadError {
    match error {
        TreeResourceError::Unavailable => SnapshotLoadError::Repository(
            ExtensionRepositoryError::FileSystem(zephium_private_fs::PrivateFsError::Io),
        ),
        TreeResourceError::NotDeclared
        | TreeResourceError::Missing
        | TreeResourceError::Mismatch
        | TreeResourceError::Quarantined => SnapshotLoadError::DurableMismatch,
    }
}
