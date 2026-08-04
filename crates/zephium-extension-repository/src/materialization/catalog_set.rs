//! Authority-preserving production of exact atomic catalog selections.
//!
//! A catalog-set final is structural metadata, never activation authority. The
//! role-specific values in this module retain every freshly re-admitted package
//! witness until a journaled selection transition consumes them. There is no
//! conversion between active and rollback roles.

use std::sync::Arc;

use zephium_extension_authority::{AdmittedBundledCatalog, AdmittedRollbackBundledCatalog};
use zephium_extension_package::ExtensionReleaseCatalog;
use zephium_private_fs::{DirectoryIdentity, SealedPrivateDirectory};

use super::objects::{
    publish_or_reuse_catalog_set_record, verify_existing_catalog_set_record, PackageObjectError,
    VerifiedCompletedActivePackage, VerifiedCompletedRollbackPackage,
};
use super::prepare::{PreparedActivePackage, PreparedRollbackPackage};
use super::records::{
    CatalogAnchor, CatalogSetPackageRow, CatalogSetRecord, PackageRecord,
    CATALOG_SET_RECORD_SCHEMA_VERSION,
};
use super::runtime::MaterializationRuntime;
use crate::state::Digest32;

struct ReverifiedPackage<Prepared> {
    record_id: Digest32,
    record: PackageRecord,
    tree_root: Arc<SealedPrivateDirectory>,
    records_parent: DirectoryIdentity,
    trees_parent: DirectoryIdentity,
    prepared: Prepared,
}

struct VerifiedCatalogSet<Prepared> {
    state_generation: u64,
    records_parent: DirectoryIdentity,
    record_id: Digest32,
    record: CatalogSetRecord,
    _tree_roots: Vec<Arc<SealedPrivateDirectory>>,
    _prepared_packages: Vec<Prepared>,
}

/// Exact active-generation selection retaining active manifest witnesses.
#[must_use = "an active catalog-set proof must be selected or discarded"]
pub(crate) struct VerifiedActiveCatalogSet(VerifiedCatalogSet<PreparedActivePackage>);

/// Exact rollback-generation selection retaining rollback manifest witnesses.
#[must_use = "a rollback catalog-set proof must be selected or discarded"]
pub(crate) struct VerifiedRollbackCatalogSet(VerifiedCatalogSet<PreparedRollbackPackage>);

pub(crate) fn derive_active_catalog_set(
    runtime: &MaterializationRuntime,
    catalog: &AdmittedBundledCatalog,
    packages: Vec<VerifiedCompletedActivePackage>,
) -> Result<VerifiedActiveCatalogSet, PackageObjectError> {
    let packages = packages
        .into_iter()
        .map(|package| {
            let (record_id, record, tree_root, records_parent, trees_parent, prepared) =
                package.into_parts();
            ReverifiedPackage {
                record_id,
                record,
                tree_root,
                records_parent,
                trees_parent,
                prepared,
            }
        })
        .collect();
    derive_catalog_set(runtime, CatalogView::active(catalog), packages)
        .map(VerifiedActiveCatalogSet)
}

pub(crate) fn derive_rollback_catalog_set(
    runtime: &MaterializationRuntime,
    catalog: &AdmittedRollbackBundledCatalog,
    packages: Vec<VerifiedCompletedRollbackPackage>,
) -> Result<VerifiedRollbackCatalogSet, PackageObjectError> {
    let packages = packages
        .into_iter()
        .map(|package| {
            let (record_id, record, tree_root, records_parent, trees_parent, prepared) =
                package.into_parts();
            ReverifiedPackage {
                record_id,
                record,
                tree_root,
                records_parent,
                trees_parent,
                prepared,
            }
        })
        .collect();
    derive_catalog_set(runtime, CatalogView::rollback(catalog), packages)
        .map(VerifiedRollbackCatalogSet)
}

#[derive(Clone, Copy)]
struct CatalogView<'catalog> {
    catalog: &'catalog ExtensionReleaseCatalog,
    anchor: CatalogAnchor,
}

impl<'catalog> CatalogView<'catalog> {
    fn active(catalog: &'catalog AdmittedBundledCatalog) -> Self {
        Self {
            catalog: catalog.catalog(),
            anchor: CatalogAnchor {
                authority_id: Digest32::from_bytes(catalog.authority().bytes()),
                revision: catalog.revision().get(),
                catalog_length: catalog.catalog_length(),
                catalog_sha256: Digest32::from_bytes(catalog.catalog_digest().bytes()),
                inventory_sha256: Digest32::from_bytes(catalog.inventory_digest().bytes()),
            },
        }
    }

    fn rollback(catalog: &'catalog AdmittedRollbackBundledCatalog) -> Self {
        Self {
            catalog: catalog.catalog(),
            anchor: CatalogAnchor {
                authority_id: Digest32::from_bytes(catalog.authority().bytes()),
                revision: catalog.revision().get(),
                catalog_length: catalog.catalog_length(),
                catalog_sha256: Digest32::from_bytes(catalog.catalog_digest().bytes()),
                inventory_sha256: Digest32::from_bytes(catalog.inventory_digest().bytes()),
            },
        }
    }
}

fn derive_catalog_set<Prepared: PreparedRecord>(
    runtime: &MaterializationRuntime,
    catalog: CatalogView<'_>,
    mut packages: Vec<ReverifiedPackage<Prepared>>,
) -> Result<VerifiedCatalogSet<Prepared>, PackageObjectError> {
    if packages.len() != catalog.catalog.packages().len() || packages.is_empty() {
        return Err(PackageObjectError::ExactMismatch);
    }
    packages.sort_unstable_by_key(|package| package.record.package.package_key);
    if packages
        .iter()
        .zip(catalog.catalog.packages())
        .any(|(selected, authenticated)| {
            selected.record.catalog != catalog.anchor
                || selected.record.package.package_key.bytes()
                    != authenticated.identity().key().bytes()
                || selected.record.record_id().ok() != Some(selected.record_id)
                || selected.prepared_record() != &selected.record
                || selected.records_parent != runtime._records.identity()
                || selected.trees_parent != runtime._trees.identity()
        })
    {
        return Err(PackageObjectError::ExactMismatch);
    }

    let rows = packages
        .iter()
        .map(|package| CatalogSetPackageRow {
            package_key: package.record.package.package_key,
            runtime_target: package.record.manifest.runtime_target,
            package_record_id: package.record_id,
        })
        .collect();
    let record = CatalogSetRecord {
        schema_version: CATALOG_SET_RECORD_SCHEMA_VERSION,
        catalog: catalog.anchor,
        packages: rows,
    };
    let record_id = record
        .record_id()
        .map_err(|_| PackageObjectError::ExactMismatch)?;
    Ok(VerifiedCatalogSet {
        state_generation: runtime._state.generation,
        records_parent: runtime._records.identity(),
        record_id,
        record,
        _tree_roots: packages
            .iter()
            .map(|package| Arc::clone(&package.tree_root))
            .collect(),
        _prepared_packages: packages
            .into_iter()
            .map(|package| package.prepared)
            .collect(),
    })
}

trait PreparedRecord {
    fn prepared_record(&self) -> &PackageRecord;
}

impl PreparedRecord for PreparedActivePackage {
    fn prepared_record(&self) -> &PackageRecord {
        self.record()
    }
}

impl PreparedRecord for PreparedRollbackPackage {
    fn prepared_record(&self) -> &PackageRecord {
        self.record()
    }
}

impl<Prepared: PreparedRecord> ReverifiedPackage<Prepared> {
    fn prepared_record(&self) -> &PackageRecord {
        self.prepared.prepared_record()
    }
}

pub(super) struct CatalogSetTransitionProof<Prepared> {
    pub(super) state_generation: u64,
    pub(super) records_parent: DirectoryIdentity,
    pub(super) record_id: Digest32,
    pub(super) record: CatalogSetRecord,
    pub(super) _tree_roots: Vec<Arc<SealedPrivateDirectory>>,
    pub(super) _prepared_packages: Vec<Prepared>,
}

impl<Prepared> VerifiedCatalogSet<Prepared> {
    fn into_transition_proof(self) -> CatalogSetTransitionProof<Prepared> {
        CatalogSetTransitionProof {
            state_generation: self.state_generation,
            records_parent: self.records_parent,
            record_id: self.record_id,
            record: self.record,
            _tree_roots: self._tree_roots,
            _prepared_packages: self._prepared_packages,
        }
    }
}

impl VerifiedActiveCatalogSet {
    pub(crate) const fn record_id(&self) -> Digest32 {
        self.0.record_id
    }

    pub(crate) fn publish(
        &self,
        runtime: &mut MaterializationRuntime,
    ) -> Result<(), PackageObjectError> {
        self.0.publish(runtime)
    }

    pub(crate) fn verify_stored(
        &self,
        runtime: &MaterializationRuntime,
    ) -> Result<(), PackageObjectError> {
        self.0.verify_stored(runtime)
    }

    pub(super) fn into_transition_proof(self) -> CatalogSetTransitionProof<PreparedActivePackage> {
        self.0.into_transition_proof()
    }
}

impl VerifiedRollbackCatalogSet {
    pub(crate) const fn record_id(&self) -> Digest32 {
        self.0.record_id
    }

    pub(crate) fn publish(
        &self,
        runtime: &mut MaterializationRuntime,
    ) -> Result<(), PackageObjectError> {
        self.0.publish(runtime)
    }

    pub(crate) fn verify_stored(
        &self,
        runtime: &MaterializationRuntime,
    ) -> Result<(), PackageObjectError> {
        self.0.verify_stored(runtime)
    }

    pub(super) fn into_transition_proof(
        self,
    ) -> CatalogSetTransitionProof<PreparedRollbackPackage> {
        self.0.into_transition_proof()
    }
}

impl<Prepared> VerifiedCatalogSet<Prepared> {
    fn validate_runtime(&self, runtime: &MaterializationRuntime) -> Result<(), PackageObjectError> {
        if self.state_generation != runtime._state.generation
            || self.records_parent != runtime._records.identity()
            || self.record.record_id().ok() != Some(self.record_id)
        {
            return Err(PackageObjectError::BuildStateMismatch);
        }
        Ok(())
    }

    fn publish(&self, runtime: &mut MaterializationRuntime) -> Result<(), PackageObjectError> {
        self.validate_runtime(runtime)?;
        publish_or_reuse_catalog_set_record(runtime, &self.record).map(|published| {
            debug_assert_eq!(published, self.record_id);
        })
    }

    fn verify_stored(&self, runtime: &MaterializationRuntime) -> Result<(), PackageObjectError> {
        self.validate_runtime(runtime)?;
        verify_existing_catalog_set_record(runtime, &self.record)
    }
}
