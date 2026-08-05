//! Exact initialization, bounded inventory, and journal recovery.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use zephium_core::extensions::ExtensionPackagePayloadIdentity;
use zephium_extension_package::{
    ExtensionReleaseCatalog, ExtensionReleaseLegalArtifactKind, MAX_EXTENSION_LEGAL_NOTICE_BYTES,
    MAX_EXTENSION_TREE_INDEX_BYTES,
};
use zephium_private_fs::{
    OpenedPrivateDirectory, PrivateComponent, PrivateDirectory, SealedPrivateDirectory,
};

use super::names::{self, RecordNameKind, RecordObjectKind, TreeNameKind};
use super::policy::{
    validate_catalog_set_tree_budget, validate_completed_tree_budget,
    validate_package_anchor_consistency,
};
use super::records::{
    CatalogAnchor, CatalogSetRecord, PackageRecord, StoredLegalArtifactKind, StoredPayloadIdentity,
    StoredRuntimePlatformFamily, StoredRuntimeTarget, MAX_CATALOG_SET_RECORD_BYTES,
    MAX_PACKAGE_RECORD_BYTES,
};
use super::runtime::{
    MaterializationPinRoots, MaterializationRuntime, MaterializationTreeCapability,
};
use super::state::{
    MaterializationCheckpoint, MaterializationJournal, MaterializationState,
    MAX_MATERIALIZATION_CHECKPOINT_BYTES, MAX_MATERIALIZATION_JOURNAL_BYTES,
    MAX_MATERIALIZATION_STATE_BYTES,
};
use super::storage::{
    map_initialization_fs, read_required_control, read_required_sealed_record,
    remove_required_control, verify_required_sealed_record, write_checkpoint,
};
use crate::catalog_cache::{
    CachedProductCatalogAdmission, CachedProductCatalogAuthentication, ProductCatalogAdmissionCache,
};
use crate::codec;
use crate::state::{digest_package_row, Digest32, StoredCatalogCheckpoint};
use crate::storage::{
    atomic_write_control, map_recovery_fs, validate_catalog_object_cache, CatalogObjectIdentities,
};
use crate::ExtensionRepositoryError;

mod gc_frontier;

#[cfg(test)]
use gc_frontier::validate_gc_intact_predelete_frontier;
use gc_frontier::{validate_gc_partial_frontier, validate_prepared_gc_transition_shape};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FaultPoint {
    None,
    AfterMaterializationDirectory,
    AfterTreesDirectory,
    AfterRecordsDirectory,
    AfterJournalsDirectory,
    AfterInitialState,
    AfterInitialCheckpoint,
    AfterRecoveryState,
    AfterRecoveryCheckpoint,
    AfterJournalRetirement,
}

pub(crate) struct ProductCatalogRecovery<'a> {
    catalog_objects: &'a PrivateDirectory,
    catalog_object_ids: &'a BTreeSet<Digest32>,
    catalog_object_identities: &'a CatalogObjectIdentities,
    catalog_cache: &'a mut ProductCatalogAdmissionCache,
    catalog_high_water: Option<CatalogAnchor>,
}

impl<'a> ProductCatalogRecovery<'a> {
    pub(crate) fn new(
        catalog_objects: &'a PrivateDirectory,
        catalog_object_ids: &'a BTreeSet<Digest32>,
        catalog_object_identities: &'a CatalogObjectIdentities,
        catalog_cache: &'a mut ProductCatalogAdmissionCache,
        catalog_high_water: Option<&StoredCatalogCheckpoint>,
    ) -> Self {
        Self {
            catalog_objects,
            catalog_object_ids,
            catalog_object_identities,
            catalog_cache,
            catalog_high_water: catalog_high_water.map(catalog_anchor_from_high_water),
        }
    }
}

pub(crate) fn open_or_recover(
    repository_root: &PrivateDirectory,
    catalog_recovery: ProductCatalogRecovery<'_>,
    materialization_exists: bool,
    fault: FaultPoint,
) -> Result<MaterializationRuntime, ExtensionRepositoryError> {
    let runtime = open_or_recover_with_policy(
        repository_root,
        CatalogGenerationInput::Product(catalog_recovery),
        materialization_exists,
        fault,
    )?;
    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    super::measurement::note_completed_pass();
    Ok(runtime)
}

/// Proves that an existing materialization namespace contains only its exact
/// settled generation-zero controls and empty writer inventories.
///
/// This read-only migration check is used solely to decide whether missing
/// outer authority controls may be initialized. Any partial or data-bearing
/// inner shape fails the proof rather than being demoted to inert state.
pub(crate) fn is_pristine_for_outer_initialization(
    repository_root: &PrivateDirectory,
    materialization_exists: bool,
) -> Result<bool, ExtensionRepositoryError> {
    if !materialization_exists {
        return Ok(true);
    }
    let root = repository_root
        .open_private_child(&names::materialization_directory())
        .map_err(map_recovery_fs)?;
    let shape = inspect_root_shape(&root)?;
    if !shape.has_trees
        || !shape.has_records
        || !shape.has_journals
        || !shape.has_state
        || !shape.has_checkpoint
        || shape.has_state_stage
        || shape.has_checkpoint_stage
    {
        return Ok(false);
    }
    let trees = root
        .open_private_child(&names::trees_directory())
        .map_err(map_recovery_fs)?;
    let records = root
        .open_private_child(&names::records_directory())
        .map_err(map_recovery_fs)?;
    let journals = root
        .open_private_child(&names::journals_directory())
        .map_err(map_recovery_fs)?;
    let tree_inventory = inspect_trees(&trees)?;
    let record_inventory = inspect_records(&records)?;
    let journal_inventory = inspect_journals(&journals)?;
    if EmptyInitializationInventory::prove(&tree_inventory, &record_inventory, &journal_inventory)
        .is_none()
    {
        return Ok(false);
    }
    let (state, state_bytes) = read_state(&root)?;
    let checkpoint = read_checkpoint(&root)?;
    let default_state = MaterializationState::default();
    let default_bytes = codec::encode(&default_state, MAX_MATERIALIZATION_STATE_BYTES)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    Ok(state == default_state
        && state_bytes == default_bytes
        && checkpoint == MaterializationCheckpoint::new(0, codec::digest(&default_bytes)))
}

fn catalog_anchor_from_high_water(checkpoint: &StoredCatalogCheckpoint) -> CatalogAnchor {
    CatalogAnchor::from_high_water(checkpoint)
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
pub(super) fn open_or_recover_test_fixture(
    repository_root: &PrivateDirectory,
    materialization_exists: bool,
    fault: FaultPoint,
) -> Result<MaterializationRuntime, ExtensionRepositoryError> {
    open_or_recover_with_policy(
        repository_root,
        CatalogGenerationInput::StructuralTestFixture {
            catalog_object_ids: None,
            recognizes: true,
        },
        materialization_exists,
        fault,
    )
}

/// Structural recovery fixture with an explicit outer catalog inventory.
///
/// This remains test-only and deliberately retains the structural generation
/// policy: it proves collector recovery mechanics without creating a product
/// authority bypass.
#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
pub(super) fn open_or_recover_test_fixture_with_catalog_ids(
    repository_root: &PrivateDirectory,
    catalog_object_ids: &BTreeSet<Digest32>,
    materialization_exists: bool,
    fault: FaultPoint,
) -> Result<MaterializationRuntime, ExtensionRepositoryError> {
    open_or_recover_with_policy(
        repository_root,
        CatalogGenerationInput::StructuralTestFixture {
            catalog_object_ids: Some(catalog_object_ids),
            recognizes: true,
        },
        materialization_exists,
        fault,
    )
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
fn open_or_recover_unrecognized_test_fixture(
    repository_root: &PrivateDirectory,
    materialization_exists: bool,
    fault: FaultPoint,
) -> Result<MaterializationRuntime, ExtensionRepositoryError> {
    open_or_recover_with_policy(
        repository_root,
        CatalogGenerationInput::StructuralTestFixture {
            catalog_object_ids: None,
            recognizes: false,
        },
        materialization_exists,
        fault,
    )
}

fn open_or_recover_with_policy(
    repository_root: &PrivateDirectory,
    catalog_generation: CatalogGenerationInput<'_>,
    materialization_exists: bool,
    fault: FaultPoint,
) -> Result<MaterializationRuntime, ExtensionRepositoryError> {
    let root = if materialization_exists {
        repository_root
            .open_private_child(&names::materialization_directory())
            .map_err(map_recovery_fs)?
    } else {
        let root = repository_root
            .create_new_private_child(&names::materialization_directory())
            .map_err(map_initialization_fs)?;
        fail_if(fault, FaultPoint::AfterMaterializationDirectory)?;
        root
    };

    let mut shape = inspect_root_shape(&root)?;
    let trees = open_or_create_directory(
        &root,
        &names::trees_directory(),
        shape.has_trees,
        fault,
        FaultPoint::AfterTreesDirectory,
    )?;
    shape.has_trees = true;
    let records = open_or_create_directory(
        &root,
        &names::records_directory(),
        shape.has_records,
        fault,
        FaultPoint::AfterRecordsDirectory,
    )?;
    shape.has_records = true;
    let journals = open_or_create_directory(
        &root,
        &names::journals_directory(),
        shape.has_journals,
        fault,
        FaultPoint::AfterJournalsDirectory,
    )?;
    shape.has_journals = true;

    let tree_inventory = inspect_trees(&trees)?;
    let mut record_inventory = inspect_records(&records)?;
    let mut journal_inventory = inspect_journals(&journals)?;
    let empty_initialization_inventory =
        EmptyInitializationInventory::prove(&tree_inventory, &record_inventory, &journal_inventory);

    shape = initialize_controls(&root, shape, empty_initialization_inventory, fault)?;

    let (state, state_bytes) = read_state(&root)?;
    let checkpoint = read_checkpoint(&root)?;
    let requires_catalog_authority = state_requires_catalog_authority(&state)
        || journal_inventory
            .prepared
            .as_ref()
            .is_some_and(|(_, journal)| state_requires_catalog_authority(&journal.next_state));
    let catalog_object_ids = catalog_generation.catalog_object_ids();
    let catalog_high_water = catalog_generation.catalog_high_water();
    let mut catalog_recognizer =
        CatalogGenerationRecognizer::open(catalog_generation, requires_catalog_authority)?;
    let disposition = assess_recovery(
        &state,
        &state_bytes,
        checkpoint,
        journal_inventory.prepared.as_ref(),
        &record_inventory,
        &tree_inventory,
        &mut catalog_recognizer,
    )?;
    inspect_control_stages(
        &root,
        shape,
        checkpoint,
        journal_inventory.prepared.as_ref(),
        disposition,
    )?;

    if disposition == RecoveryDisposition::ApplyPrepared {
        let prepared = &journal_inventory
            .prepared
            .as_ref()
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?
            .1;
        validate_prepared_gc_transition_shape(&state, &prepared.next_state)?;
    }

    // A prepared collector transition has an asymmetric physical frontier.
    // Intent publication may be replayed only while every target is intact;
    // intent clearing may be replayed only after every target is absent. Once
    // the successor state itself is durable, the journal/checkpoint chain is
    // sufficient to finish its remaining control-file steps.
    if disposition == RecoveryDisposition::ApplyPrepared {
        let prepared = &journal_inventory
            .prepared
            .as_ref()
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?
            .1;
        match (&state.gc_intent, &prepared.next_state.gc_intent) {
            (None, Some(_)) => {
                let frontier = validate_gc_partial_frontier(
                    &prepared.next_state,
                    &record_inventory,
                    &tree_inventory,
                    catalog_object_ids,
                    catalog_high_water,
                )?;
                if !frontier.all_intact {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
            }
            (Some(_), None) => {
                let frontier = validate_gc_partial_frontier(
                    &state,
                    &record_inventory,
                    &tree_inventory,
                    catalog_object_ids,
                    catalog_high_water,
                )?;
                if !frontier.all_absent {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
            }
            (None, None) => {}
            (Some(_), Some(_)) => return Err(ExtensionRepositoryError::RecoveryAmbiguous),
        }
    }
    if matches!(
        disposition,
        RecoveryDisposition::CheckpointPrepared | RecoveryDisposition::RetirePrepared
    ) && state.gc_intent.is_some()
    {
        let frontier = validate_gc_partial_frontier(
            &state,
            &record_inventory,
            &tree_inventory,
            catalog_object_ids,
            catalog_high_water,
        )?;
        if !frontier.all_intact {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }

    // Reject a corrupt prepared GC successor before cleanup or control-state
    // recovery performs any mutation. ApplyPrepared converges to the journal
    // successor; every other disposition converges to the current state.
    let effective_state = match disposition {
        RecoveryDisposition::ApplyPrepared => {
            &journal_inventory
                .prepared
                .as_ref()
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?
                .1
                .next_state
        }
        RecoveryDisposition::Settled
        | RecoveryDisposition::CheckpointPrepared
        | RecoveryDisposition::RetirePrepared => &state,
    };
    validate_gc_partial_frontier(
        effective_state,
        &record_inventory,
        &tree_inventory,
        catalog_object_ids,
        catalog_high_water,
    )?;
    validate_package_record_roots(effective_state, &record_inventory)?;

    let preserve_record_stages = match disposition {
        RecoveryDisposition::ApplyPrepared => journal_inventory
            .prepared
            .as_ref()
            .is_some_and(|(_, prepared)| prepared.next_state.build_intent.is_some()),
        RecoveryDisposition::Settled
        | RecoveryDisposition::CheckpointPrepared
        | RecoveryDisposition::RetirePrepared => state.build_intent.is_some(),
    };
    cleanup_stages(
        &root,
        &records,
        &journals,
        &mut record_inventory.stages,
        &mut journal_inventory.stages,
        shape,
        preserve_record_stages,
    )?;
    let (state, state_bytes) = apply_recovery(
        &root,
        &journals,
        state,
        state_bytes,
        journal_inventory.prepared,
        disposition,
        fault,
    )?;
    // Enforce commit-marker reachability only after any exact prepared journal
    // has been applied. During completion recovery the current state may still
    // name the build intent while the prepared successor names the completed
    // ledger; the converged state is the sole durable root projection.
    validate_gc_partial_frontier(
        &state,
        &record_inventory,
        &tree_inventory,
        catalog_object_ids,
        catalog_high_water,
    )?;
    validate_package_record_roots(&state, &record_inventory)?;
    let pin_roots = validate_state_references(
        &state,
        &record_inventory,
        &tree_inventory,
        &mut catalog_recognizer,
    )?;
    let build_intent = state.build_intent.clone();
    let gc_intent = state.gc_intent.clone();
    let TreeInventory {
        objects,
        stage,
        retired,
    } = tree_inventory;
    // Stable handles are long-lived only for state-reachable final trees and
    // the one resumable build stage. Retired and dormant objects were opened
    // safely above, but GC reopens their exact verified names on demand rather
    // than charging every repository instance a directory descriptor.
    let tree_object_ids = objects.keys().copied().collect();
    let sealed_tree_roots = objects
        .into_iter()
        .filter(|(digest, _)| pin_roots._tree_ids.contains(digest))
        .collect();
    let retired_tree_ids = retired.into_keys().collect();

    Ok(MaterializationRuntime {
        _root: root,
        _trees: trees,
        _records: records,
        _journals: journals,
        _state: state,
        _state_bytes: state_bytes,
        _package_records: record_inventory.packages,
        _catalog_sets: record_inventory.catalog_sets,
        _pin_roots: pin_roots,
        _sealed_tree_roots: sealed_tree_roots,
        _tree_object_ids: tree_object_ids,
        _tree_index_ids: record_inventory.tree_indexes,
        _legal_artifact_ids: record_inventory.legal_artifacts,
        _build_intent: build_intent,
        _gc_intent: gc_intent,
        _build_stage: stage.map(|(_, _, stage)| stage),
        _retired_tree_ids: retired_tree_ids,
        _record_stages: record_inventory.stages,
    })
}

enum CatalogGenerationInput<'a> {
    Product(ProductCatalogRecovery<'a>),
    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
    StructuralTestFixture {
        catalog_object_ids: Option<&'a BTreeSet<Digest32>>,
        recognizes: bool,
    },
}

impl<'a> CatalogGenerationInput<'a> {
    fn catalog_object_ids(&self) -> Option<&'a BTreeSet<Digest32>> {
        match self {
            Self::Product(recovery) => Some(recovery.catalog_object_ids),
            #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
            Self::StructuralTestFixture {
                catalog_object_ids, ..
            } => *catalog_object_ids,
        }
    }

    fn catalog_high_water(&self) -> Option<CatalogAnchor> {
        match self {
            Self::Product(recovery) => recovery.catalog_high_water,
            #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
            Self::StructuralTestFixture { .. } => None,
        }
    }
}

struct CatalogGenerationRecognizer<'a> {
    catalog_objects: Option<&'a PrivateDirectory>,
    catalog_object_ids: Option<&'a BTreeSet<Digest32>>,
    catalog_object_identities: Option<&'a CatalogObjectIdentities>,
    product_cache: Option<&'a mut ProductCatalogAdmissionCache>,
    catalog_high_water: Option<CatalogAnchor>,
    authenticated_product_catalogs: BTreeMap<CatalogAnchor, CachedProductCatalogAuthentication>,
    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
    authenticated_structural_catalogs: BTreeSet<CatalogAnchor>,
    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
    structural_test_recognizes: bool,
}

impl<'a> CatalogGenerationRecognizer<'a> {
    fn open(
        input: CatalogGenerationInput<'a>,
        requires_catalog_authority: bool,
    ) -> Result<Self, ExtensionRepositoryError> {
        match input {
            CatalogGenerationInput::Product(recovery) => {
                validate_catalog_object_cache(
                    recovery.catalog_object_ids,
                    recovery.catalog_object_identities,
                )?;
                if requires_catalog_authority && recovery.catalog_high_water.is_none() {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
                Ok(Self {
                    catalog_objects: Some(recovery.catalog_objects),
                    catalog_object_ids: Some(recovery.catalog_object_ids),
                    catalog_object_identities: Some(recovery.catalog_object_identities),
                    product_cache: Some(recovery.catalog_cache),
                    catalog_high_water: recovery.catalog_high_water,
                    authenticated_product_catalogs: BTreeMap::new(),
                    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
                    authenticated_structural_catalogs: BTreeSet::new(),
                    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
                    structural_test_recognizes: false,
                })
            }
            #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
            CatalogGenerationInput::StructuralTestFixture {
                catalog_object_ids,
                recognizes,
            } => Ok(Self {
                catalog_objects: None,
                catalog_object_ids,
                catalog_object_identities: None,
                product_cache: None,
                catalog_high_water: None,
                authenticated_product_catalogs: BTreeMap::new(),
                authenticated_structural_catalogs: BTreeSet::new(),
                structural_test_recognizes: recognizes,
            }),
        }
    }

    fn authenticate(&mut self, catalog: CatalogAnchor) -> Result<(), ExtensionRepositoryError> {
        if self.authenticated_product_catalogs.contains_key(&catalog) || {
            #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
            {
                self.authenticated_structural_catalogs.contains(&catalog)
            }
            #[cfg(not(all(test, any(target_os = "macos", target_os = "linux"))))]
            {
                false
            }
        } {
            return Ok(());
        }
        if let Some(cache) = self.product_cache.as_deref_mut() {
            let objects = self
                .catalog_objects
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            let object_ids = self
                .catalog_object_ids
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            let object_identities = self
                .catalog_object_identities
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            if !object_ids.contains(&catalog.catalog_sha256) {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            let expected_identity = object_identities
                .get(&catalog.catalog_sha256)
                .copied()
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            let authentication = cache.authenticate_catalog_object(
                objects,
                catalog,
                expected_identity,
                self.catalog_high_water,
            )?;
            #[cfg(all(
                test,
                zephium_internal_repository_e2e,
                any(target_os = "macos", target_os = "linux")
            ))]
            {
                super::measurement::note_catalog_read(authentication.bytes_read());
                if authentication.cache_hit() {
                    super::measurement::note_catalog_cache_hit();
                } else {
                    super::measurement::note_catalog_admission_attempt(authentication.bytes_read());
                }
            }
            self.authenticated_product_catalogs
                .insert(catalog, authentication.authentication());
            return Ok(());
        }
        #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
        if self.structural_test_recognizes {
            if self
                .catalog_object_ids
                .is_some_and(|ids| !ids.contains(&catalog.catalog_sha256))
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            self.authenticated_structural_catalogs.insert(catalog);
            return Ok(());
        }
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    }

    fn validate_package(&self, package: &PackageRecord) -> Result<(), ExtensionRepositoryError> {
        match self.authenticated_product_catalog(package.catalog)? {
            Some(admission) => validate_package_against_catalog(package, admission.catalog()),
            #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
            None if self.structural_test_recognizes
                && self
                    .authenticated_structural_catalogs
                    .contains(&package.catalog) =>
            {
                Ok(())
            }
            _ => Err(ExtensionRepositoryError::RecoveryAmbiguous),
        }
    }

    fn validate_catalog_set_projection(
        &self,
        set: &CatalogSetRecord,
    ) -> Result<(), ExtensionRepositoryError> {
        match self.authenticated_product_catalog(set.catalog)? {
            Some(admission) => validate_catalog_set_against_catalog(set, admission.catalog()),
            #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
            None if self.structural_test_recognizes
                && self
                    .authenticated_structural_catalogs
                    .contains(&set.catalog) =>
            {
                Ok(())
            }
            _ => Err(ExtensionRepositoryError::RecoveryAmbiguous),
        }
    }

    fn authenticated_product_catalog(
        &self,
        anchor: CatalogAnchor,
    ) -> Result<Option<CachedProductCatalogAdmission<'_>>, ExtensionRepositoryError> {
        let Some(authentication) = self.authenticated_product_catalogs.get(&anchor).copied() else {
            return Ok(None);
        };
        let Some(cache) = self.product_cache.as_deref() else {
            return Ok(None);
        };
        cache
            .lookup_authenticated(authentication, self.catalog_high_water)
            .map(Some)
    }

    const fn expected_platform_family(&self) -> Option<StoredRuntimePlatformFamily> {
        if self.product_cache.is_none() {
            return None;
        }
        #[cfg(target_os = "macos")]
        return Some(StoredRuntimePlatformFamily::Macos);
        #[cfg(target_os = "linux")]
        return Some(StoredRuntimePlatformFamily::Linux);
        #[cfg(target_os = "windows")]
        return Some(StoredRuntimePlatformFamily::Windows);
        #[allow(unreachable_code)]
        None
    }
}

fn validate_catalog_set_against_catalog(
    set: &CatalogSetRecord,
    catalog: &ExtensionReleaseCatalog,
) -> Result<(), ExtensionRepositoryError> {
    if catalog.packages().len() != set.packages.len()
        || catalog
            .packages()
            .iter()
            .zip(&set.packages)
            .any(|(package, row)| package.identity().key().bytes() != row.package_key.bytes())
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

fn validate_package_against_catalog(
    record: &PackageRecord,
    catalog: &ExtensionReleaseCatalog,
) -> Result<(), ExtensionRepositoryError> {
    let package = catalog
        .packages()
        .iter()
        .find(|package| package.identity().key().bytes() == record.package.package_key.bytes())
        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
    let identity = package.identity();
    let payload_matches = match (record.package.payload, identity.payload()) {
        (StoredPayloadIdentity::BundledTree, ExtensionPackagePayloadIdentity::BundledTree) => true,
        (
            StoredPayloadIdentity::AcquiredZip {
                length: stored_length,
                sha256: stored_sha256,
            },
            ExtensionPackagePayloadIdentity::AcquiredZip { length, sha256 },
        ) => stored_length == length.get() && stored_sha256.bytes() == sha256.bytes(),
        _ => false,
    };
    let chromium_manifest_key_sha256 = package
        .chromium()
        .map(|identity| Digest32::from_bytes(identity.manifest_key_sha256().bytes()));
    let notice = package.provenance().legal_notice();
    let legal_kind_matches = matches!(
        (record.legal.kind, notice.kind()),
        (
            StoredLegalArtifactKind::NoticeBundle,
            ExtensionReleaseLegalArtifactKind::NoticeBundle
        )
    );
    if identity.authority().bytes() != record.package.authority_id.bytes()
        || identity.key().bytes() != record.package.package_key.bytes()
        || identity.revision().get() != record.package.revision
        || !payload_matches
        || identity.manifest_sha256().bytes() != record.package.manifest_sha256.bytes()
        || identity.tree_sha256().bytes() != record.package.tree_sha256.bytes()
        || digest_package_row(package)? != record.package.package_row_sha256
        || package.tree_index_sha256().bytes() != record.tree_index.index_sha256.bytes()
        || package.tree_index_length() != record.tree_index.index_length
        || u32::try_from(package.tree_file_count()).ok() != Some(record.tree_index.file_count)
        || package.tree_bytes() != record.tree_index.tree_bytes
        || chromium_manifest_key_sha256 != record.package.chromium_manifest_key_sha256
        || notice.target().as_str() != record.legal.target
        || !legal_kind_matches
        || notice.length() != record.legal.length
        || notice.sha256() != record.legal.sha256.bytes()
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

fn state_requires_catalog_authority(state: &MaterializationState) -> bool {
    state.build_intent.is_some()
        || state.candidate_catalog_set_id.is_some()
        || state.current_catalog_set_id.is_some()
        || state.previous_catalog_set_id.is_some()
        || !state.package_pins.is_empty()
}

fn open_or_create_directory(
    root: &PrivateDirectory,
    name: &PrivateComponent,
    exists: bool,
    fault: FaultPoint,
    after_creation: FaultPoint,
) -> Result<PrivateDirectory, ExtensionRepositoryError> {
    if exists {
        root.open_private_child(name).map_err(map_recovery_fs)
    } else {
        let directory = root
            .create_new_private_child(name)
            .map_err(map_initialization_fs)?;
        fail_if(fault, after_creation)?;
        Ok(directory)
    }
}

#[derive(Clone, Copy, Default)]
struct RootShape {
    has_trees: bool,
    has_records: bool,
    has_journals: bool,
    has_state: bool,
    has_checkpoint: bool,
    has_state_stage: bool,
    has_checkpoint_stage: bool,
}

fn inspect_root_shape(root: &PrivateDirectory) -> Result<RootShape, ExtensionRepositoryError> {
    let entries = root
        .list_components(names::MAX_MATERIALIZATION_ROOT_ENTRIES)
        .map_err(map_recovery_fs)?;
    let mut shape = RootShape::default();
    let mut has_control = false;
    for entry in entries {
        match entry.as_str() {
            "trees" => {
                root.open_private_child(&entry).map_err(map_recovery_fs)?;
                shape.has_trees = true;
            }
            "records" => {
                root.open_private_child(&entry).map_err(map_recovery_fs)?;
                shape.has_records = true;
            }
            "journals" => {
                root.open_private_child(&entry).map_err(map_recovery_fs)?;
                shape.has_journals = true;
            }
            "state.json" => {
                require_regular(root, &entry)?;
                shape.has_state = true;
                has_control = true;
            }
            "recovery-checkpoint.json" => {
                require_regular(root, &entry)?;
                shape.has_checkpoint = true;
                has_control = true;
            }
            "state.stage" => {
                require_regular(root, &entry)?;
                shape.has_state_stage = true;
                has_control = true;
            }
            "recovery-checkpoint.stage" => {
                require_regular(root, &entry)?;
                shape.has_checkpoint_stage = true;
                has_control = true;
            }
            _ => return Err(ExtensionRepositoryError::RecoveryAmbiguous),
        }
    }
    if shape.has_records && !shape.has_trees {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if shape.has_journals && (!shape.has_trees || !shape.has_records) {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if has_control && (!shape.has_trees || !shape.has_records || !shape.has_journals) {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(shape)
}

fn require_regular(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
) -> Result<(), ExtensionRepositoryError> {
    if !directory.regular_exists(name).map_err(map_recovery_fs)? {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

struct TreeInventory {
    objects: BTreeMap<Digest32, Arc<SealedPrivateDirectory>>,
    stage: Option<(Digest32, u64, MaterializationTreeCapability)>,
    retired: BTreeMap<(Digest32, u64), MaterializationTreeCapability>,
}

fn inspect_trees(trees: &PrivateDirectory) -> Result<TreeInventory, ExtensionRepositoryError> {
    let entries = trees
        .list_components(names::MAX_TREE_ENTRIES)
        .map_err(map_recovery_fs)?;
    let mut observed_ids = BTreeSet::new();
    let mut objects = BTreeMap::new();
    let mut stage = None;
    let mut retired = BTreeMap::new();
    for entry in entries {
        let (digest, kind) = names::parse_tree_name(entry.as_str())
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        // Object, stage, and retired roots are all immutable identities. One
        // digest may have only one exact name at startup; publication and
        // retirement are consuming same-parent renames.
        if !observed_ids.insert(digest) {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        match kind {
            TreeNameKind::Object => {
                let sealed = trees
                    .open_sealed_private_child(&entry)
                    .map_err(map_recovery_fs)?;
                objects.insert(digest, Arc::new(sealed));
            }
            TreeNameKind::Retired(generation) => {
                retired.insert((digest, generation), open_tree_capability(trees, &entry)?);
            }
            TreeNameKind::Stage(generation) => {
                if stage.is_some() {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
                let capability = open_tree_capability(trees, &entry)?;
                stage = Some((digest, generation, capability));
            }
        }
    }
    if objects.len() > names::MAX_FINAL_PACKAGE_RECORDS
        || retired.len() > names::MAX_RETIRED_TREE_RECORDS
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(TreeInventory {
        objects,
        stage,
        retired,
    })
}

fn open_tree_capability(
    trees: &PrivateDirectory,
    name: &PrivateComponent,
) -> Result<MaterializationTreeCapability, ExtensionRepositoryError> {
    match trees
        .open_private_child_any_mode(name)
        .map_err(map_recovery_fs)?
    {
        OpenedPrivateDirectory::Writable(directory) => {
            Ok(MaterializationTreeCapability::Writable {
                _directory: directory,
            })
        }
        OpenedPrivateDirectory::Sealed(directory) => Ok(MaterializationTreeCapability::Sealed {
            _directory: Arc::new(directory),
        }),
    }
}

struct RecordInventory {
    packages: BTreeMap<Digest32, PackageRecord>,
    catalog_sets: BTreeMap<Digest32, CatalogSetRecord>,
    tree_indexes: BTreeSet<Digest32>,
    legal_artifacts: BTreeSet<Digest32>,
    stages: BTreeMap<RecordObjectKind, (Digest32, PrivateComponent)>,
}

fn inspect_records(
    records: &PrivateDirectory,
) -> Result<RecordInventory, ExtensionRepositoryError> {
    let entries = records
        .list_components(names::MAX_RECORD_ENTRIES)
        .map_err(map_recovery_fs)?;
    let mut inventory = RecordInventory {
        packages: BTreeMap::new(),
        catalog_sets: BTreeMap::new(),
        tree_indexes: BTreeSet::new(),
        legal_artifacts: BTreeSet::new(),
        stages: BTreeMap::new(),
    };
    let mut parsed = Vec::with_capacity(entries.len());
    let mut final_ids = BTreeSet::new();
    let mut stage_ids = BTreeSet::new();
    let mut stage_kinds = BTreeSet::new();
    let mut final_counts = BTreeMap::new();
    for entry in entries {
        let (digest, kind) = names::parse_record_name(entry.as_str())
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        let object_kind = kind.object_kind();
        if kind.is_stage() {
            require_regular(records, &entry)?;
            if !stage_ids.insert((object_kind, digest)) || !stage_kinds.insert(object_kind) {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            inventory.stages.insert(object_kind, (digest, entry));
            continue;
        } else {
            if !final_ids.insert((object_kind, digest)) {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            let count = final_counts.entry(object_kind).or_insert(0_usize);
            *count = count
                .checked_add(1)
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            if *count > final_record_limit(object_kind) {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }
        parsed.push((entry, digest, kind));
    }
    if stage_ids.iter().any(|id| final_ids.contains(id)) {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }

    for (entry, digest, kind) in parsed {
        match kind {
            RecordNameKind::Package { stage: false } => {
                let bytes = read_required_sealed_record(records, &entry, MAX_PACKAGE_RECORD_BYTES)?;
                let record = PackageRecord::decode(&bytes)?;
                if record.record_id()? != digest {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
                inventory.packages.insert(digest, record);
            }
            RecordNameKind::CatalogSet { stage: false } => {
                let bytes =
                    read_required_sealed_record(records, &entry, MAX_CATALOG_SET_RECORD_BYTES)?;
                let record = CatalogSetRecord::decode(&bytes)?;
                if record.record_id()? != digest {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
                inventory.catalog_sets.insert(digest, record);
            }
            RecordNameKind::TreeIndex { stage: false } => {
                verify_required_sealed_record(records, &entry, MAX_EXTENSION_TREE_INDEX_BYTES)?;
                inventory.tree_indexes.insert(digest);
            }
            RecordNameKind::Legal { stage: false } => {
                verify_required_sealed_record(
                    records,
                    &entry,
                    MAX_EXTENSION_LEGAL_NOTICE_BYTES as usize,
                )?;
                inventory.legal_artifacts.insert(digest);
            }
            RecordNameKind::Package { stage: true }
            | RecordNameKind::CatalogSet { stage: true }
            | RecordNameKind::TreeIndex { stage: true }
            | RecordNameKind::Legal { stage: true } => {}
        }
    }
    Ok(inventory)
}

const fn final_record_limit(kind: RecordObjectKind) -> usize {
    match kind {
        RecordObjectKind::Package => names::MAX_FINAL_PACKAGE_RECORDS,
        RecordObjectKind::CatalogSet => names::MAX_FINAL_CATALOG_SET_RECORDS,
        RecordObjectKind::TreeIndex | RecordObjectKind::Legal => {
            names::MAX_FINAL_DATA_OBJECTS_PER_KIND
        }
    }
}

struct JournalInventory {
    stages: Vec<PrivateComponent>,
    prepared: Option<(PrivateComponent, MaterializationJournal)>,
}

/// Zero-sized proof that no writer-owned data predates initial controls.
///
/// Exhaustive destructuring makes additions to any inventory a compile-time
/// prompt to decide whether the new field is data-bearing.
struct EmptyInitializationInventory;

impl EmptyInitializationInventory {
    fn prove(
        trees: &TreeInventory,
        records: &RecordInventory,
        journals: &JournalInventory,
    ) -> Option<Self> {
        let TreeInventory {
            objects,
            stage,
            retired,
        } = trees;
        let RecordInventory {
            packages,
            catalog_sets,
            tree_indexes,
            legal_artifacts,
            stages: record_stages,
        } = records;
        let JournalInventory {
            stages: journal_stages,
            prepared,
        } = journals;
        (objects.is_empty()
            && stage.is_none()
            && retired.is_empty()
            && packages.is_empty()
            && catalog_sets.is_empty()
            && tree_indexes.is_empty()
            && legal_artifacts.is_empty()
            && record_stages.is_empty()
            && journal_stages.is_empty()
            && prepared.is_none())
        .then_some(Self)
    }
}

fn inspect_journals(
    journals: &PrivateDirectory,
) -> Result<JournalInventory, ExtensionRepositoryError> {
    let entries = journals
        .list_components(names::MAX_MATERIALIZATION_JOURNAL_ENTRIES)
        .map_err(map_recovery_fs)?;
    let mut stages = Vec::new();
    let mut prepared = None;
    for entry in entries {
        let (name_generation, name_digest, stage) = names::parse_journal_name(entry.as_str())
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if stage {
            require_regular(journals, &entry)?;
            stages.push(entry);
            continue;
        }
        let bytes = read_required_control(journals, &entry, MAX_MATERIALIZATION_JOURNAL_BYTES)?;
        let journal: MaterializationJournal =
            codec::decode_materialization(&bytes, MAX_MATERIALIZATION_JOURNAL_BYTES)
                .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
        journal.validate()?;
        let next_bytes = codec::encode(&journal.next_state, MAX_MATERIALIZATION_STATE_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
        if journal.generation != name_generation
            || codec::digest(&bytes) != name_digest
            || codec::digest(&next_bytes) != journal.next_state_sha256
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if prepared.replace((entry, journal)).is_some() {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    if prepared.is_some() && !stages.is_empty() || stages.len() > 1 {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(JournalInventory { stages, prepared })
}

fn initialize_controls(
    root: &PrivateDirectory,
    mut shape: RootShape,
    empty_initialization_inventory: Option<EmptyInitializationInventory>,
    fault: FaultPoint,
) -> Result<RootShape, ExtensionRepositoryError> {
    // Package writers can run only after both initial controls are durable.
    // Therefore any data-bearing tree, record, or journal inventory alongside
    // a missing control proves loss/corruption, not a pristine initialization
    // frontier. Never demote such data to inert garbage by minting defaults.
    if (!shape.has_state || !shape.has_checkpoint) && empty_initialization_inventory.is_none() {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    let default_state = MaterializationState::default();
    let default_bytes = codec::encode(&default_state, MAX_MATERIALIZATION_STATE_BYTES)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    let default_checkpoint = MaterializationCheckpoint::new(0, codec::digest(&default_bytes));
    if !shape.has_state {
        if shape.has_checkpoint || shape.has_checkpoint_stage {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if shape.has_state_stage {
            remove_required_control(root, &names::state_stage())?;
            shape.has_state_stage = false;
        }
        atomic_write_control(
            root,
            &names::state_file(),
            &names::state_stage(),
            &default_bytes,
            MAX_MATERIALIZATION_STATE_BYTES,
        )?;
        fail_if(fault, FaultPoint::AfterInitialState)?;
        shape.has_state = true;
        shape.has_state_stage = false;
    }

    if !shape.has_checkpoint {
        let state_bytes =
            read_required_control(root, &names::state_file(), MAX_MATERIALIZATION_STATE_BYTES)?;
        if state_bytes != default_bytes || shape.has_state_stage {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if shape.has_checkpoint_stage {
            remove_required_control(root, &names::checkpoint_stage())?;
        }
        write_checkpoint(root, default_checkpoint)?;
        fail_if(fault, FaultPoint::AfterInitialCheckpoint)?;
        shape.has_checkpoint = true;
        shape.has_checkpoint_stage = false;
    }
    Ok(shape)
}

fn read_state(
    root: &PrivateDirectory,
) -> Result<(MaterializationState, Vec<u8>), ExtensionRepositoryError> {
    let bytes = read_required_control(root, &names::state_file(), MAX_MATERIALIZATION_STATE_BYTES)?;
    let state: MaterializationState =
        codec::decode_materialization(&bytes, MAX_MATERIALIZATION_STATE_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    state.validate()?;
    Ok((state, bytes))
}

fn read_checkpoint(
    root: &PrivateDirectory,
) -> Result<MaterializationCheckpoint, ExtensionRepositoryError> {
    let bytes = read_required_control(
        root,
        &names::checkpoint_file(),
        MAX_MATERIALIZATION_CHECKPOINT_BYTES,
    )?;
    let checkpoint: MaterializationCheckpoint =
        codec::decode_materialization(&bytes, MAX_MATERIALIZATION_CHECKPOINT_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    checkpoint.validate()?;
    Ok(checkpoint)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryDisposition {
    Settled,
    ApplyPrepared,
    CheckpointPrepared,
    RetirePrepared,
}

fn assess_recovery(
    state: &MaterializationState,
    state_bytes: &[u8],
    checkpoint: MaterializationCheckpoint,
    journal: Option<&(PrivateComponent, MaterializationJournal)>,
    records: &RecordInventory,
    trees: &TreeInventory,
    catalog_recognizer: &mut CatalogGenerationRecognizer<'_>,
) -> Result<RecoveryDisposition, ExtensionRepositoryError> {
    validate_state_references(state, records, trees, catalog_recognizer)?;
    let state_digest = codec::digest(state_bytes);
    let checkpoint_successor = checkpoint.generation.checked_add(1);
    if checkpoint.generation > state.generation
        || (state.generation != checkpoint.generation
            && Some(state.generation) != checkpoint_successor)
        || checkpoint.generation == state.generation && checkpoint.state_sha256 != state_digest
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }

    match journal {
        None => {
            if checkpoint.generation != state.generation || checkpoint.state_sha256 != state_digest
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            Ok(RecoveryDisposition::Settled)
        }
        Some((_, prepared)) if checkpoint.generation == state.generation => {
            if prepared.generation == state.generation {
                if state.generation == 0
                    || prepared.next_state_sha256 != state_digest
                    || &prepared.next_state != state
                {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
                Ok(RecoveryDisposition::RetirePrepared)
            } else if state
                .generation
                .checked_add(1)
                .is_some_and(|next| prepared.generation == next)
                && prepared.previous_state_sha256 == state_digest
            {
                validate_state_references(
                    &prepared.next_state,
                    records,
                    trees,
                    catalog_recognizer,
                )?;
                Ok(RecoveryDisposition::ApplyPrepared)
            } else {
                Err(ExtensionRepositoryError::RecoveryAmbiguous)
            }
        }
        Some((_, prepared)) => {
            if Some(state.generation) != checkpoint.generation.checked_add(1)
                || prepared.generation != state.generation
                || prepared.previous_state_sha256 != checkpoint.state_sha256
                || prepared.next_state_sha256 != state_digest
                || &prepared.next_state != state
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            Ok(RecoveryDisposition::CheckpointPrepared)
        }
    }
}

fn validate_state_references(
    state: &MaterializationState,
    records: &RecordInventory,
    trees: &TreeInventory,
    catalog_recognizer: &mut CatalogGenerationRecognizer<'_>,
) -> Result<MaterializationPinRoots, ExtensionRepositoryError> {
    state.validate()?;
    validate_build_inventory(state, records, trees)?;

    // The completed ledger is a structural retention root, not durable product
    // authority. Its package closure must remain complete and internally
    // consistent across upgrades, but a dormant entry may belong to a product
    // generation that is no longer recognized.
    let completed_packages = validate_completed_package_closure(state, records, trees)?;
    let intent_package = state
        .build_intent
        .as_ref()
        .map(|intent| &intent.package_record);
    validate_package_anchor_consistency(completed_packages.iter().copied().chain(intent_package))
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;

    let catalog_set_ids = state.catalog_pin_ids().into_iter().collect::<BTreeSet<_>>();
    let mut package_record_ids = state
        .completed_package_record_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut tree_ids = completed_packages
        .iter()
        .map(|package| package.tree_index.tree_sha256)
        .collect::<BTreeSet<_>>();
    let mut selected_package_record_ids = BTreeSet::new();
    let mut live_platform_family = None;

    if let Some(package) = intent_package {
        require_authenticated_package(package, catalog_recognizer)?;
        bind_live_platform_family(
            &mut live_platform_family,
            package.manifest.runtime_target,
            catalog_recognizer.expected_platform_family(),
        )?;
    }

    for catalog_id in &catalog_set_ids {
        let catalog = records
            .catalog_sets
            .get(catalog_id)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        catalog_recognizer.authenticate(catalog.catalog)?;
        catalog_recognizer.validate_catalog_set_projection(catalog)?;
        let mut roots = ReferencedCatalogSetRoots {
            package_record_ids: &mut package_record_ids,
            tree_ids: &mut tree_ids,
            selected_package_record_ids: &mut selected_package_record_ids,
            live_platform_family: &mut live_platform_family,
        };
        validate_referenced_catalog_set(
            catalog,
            state,
            &records.packages,
            &mut roots,
            catalog_recognizer,
        )?;
    }

    // Every owner names the exact authenticated set from which its package was
    // selected. `catalog_pin_ids` retains that whole set, including one bounded
    // owner-only drain set, so recovery never reconstructs a partial selection
    // from package catalog anchors.
    for pin in &state.package_pins {
        let set = records
            .catalog_sets
            .get(&pin.catalog_set_record_id)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        let row = set
            .packages
            .iter()
            .find(|row| row.package_record_id == pin.package_record_id)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        let package = records
            .packages
            .get(&pin.package_record_id)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if package.catalog != set.catalog
            || package.package.package_key != row.package_key
            || package.manifest.runtime_target != row.runtime_target
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        bind_live_platform_family(
            &mut live_platform_family,
            row.runtime_target,
            catalog_recognizer.expected_platform_family(),
        )?;
        if !selected_package_record_ids.contains(&pin.package_record_id) {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        package_record_ids.insert(pin.package_record_id);
        tree_ids.insert(package.tree_index.tree_sha256);
    }

    Ok(MaterializationPinRoots {
        _catalog_set_ids: catalog_set_ids,
        _package_record_ids: package_record_ids,
        _tree_ids: tree_ids,
    })
}

/// Rejects orphan package-record commit markers.
///
/// Tree, index, and legal finals published before the marker may remain inert
/// after an abort and are deliberately reusable. A package-record final is
/// different: publication orders it last and permanently commits the exact
/// build to source-free completion. It must therefore be rooted by the
/// completed ledger, the one exact live build, or an exact GC cleanup target,
/// never by none of them. GC membership is a negative retention root only; it
/// grants no package or product authority.
fn validate_package_record_roots(
    state: &MaterializationState,
    records: &RecordInventory,
) -> Result<(), ExtensionRepositoryError> {
    for (record_id, record) in &records.packages {
        if state
            .completed_package_record_ids
            .binary_search(record_id)
            .is_ok()
        {
            continue;
        }
        let rooted_by_intent = state.build_intent.as_ref().is_some_and(|intent| {
            intent.package_record_id == *record_id && intent.package_record == *record
        });
        let rooted_by_gc = state.gc_intent.as_ref().is_some_and(|intent| {
            intent.cohort == Some(record.catalog)
                && intent.package_record_ids.binary_search(record_id).is_ok()
        });
        if !rooted_by_intent && !rooted_by_gc {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    Ok(())
}

fn validate_completed_package_closure<'a>(
    state: &MaterializationState,
    records: &'a RecordInventory,
    trees: &TreeInventory,
) -> Result<Vec<&'a PackageRecord>, ExtensionRepositoryError> {
    let mut packages = Vec::with_capacity(state.completed_package_record_ids.len());
    for package_id in &state.completed_package_record_ids {
        let package = records
            .packages
            .get(package_id)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if !records
            .tree_indexes
            .contains(&package.tree_index.index_sha256)
            || !records.legal_artifacts.contains(&package.legal.sha256)
            || !trees.objects.contains_key(&package.tree_index.tree_sha256)
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        packages.push(package);
    }
    validate_completed_budget(state, &records.packages)?;
    Ok(packages)
}

fn validate_completed_budget(
    state: &MaterializationState,
    packages: &BTreeMap<Digest32, PackageRecord>,
) -> Result<(), ExtensionRepositoryError> {
    let selected = state
        .completed_package_record_ids
        .iter()
        .map(|package_id| {
            packages
                .get(package_id)
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_completed_tree_budget(selected)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)
}

struct ReferencedCatalogSetRoots<'a> {
    package_record_ids: &'a mut BTreeSet<Digest32>,
    tree_ids: &'a mut BTreeSet<Digest32>,
    selected_package_record_ids: &'a mut BTreeSet<Digest32>,
    live_platform_family: &'a mut Option<StoredRuntimePlatformFamily>,
}

fn validate_referenced_catalog_set(
    set: &CatalogSetRecord,
    state: &MaterializationState,
    packages: &BTreeMap<Digest32, PackageRecord>,
    roots: &mut ReferencedCatalogSetRoots<'_>,
    catalog_recognizer: &mut CatalogGenerationRecognizer<'_>,
) -> Result<(), ExtensionRepositoryError> {
    let mut set_packages = Vec::with_capacity(set.packages.len());
    for row in &set.packages {
        if state
            .completed_package_record_ids
            .binary_search(&row.package_record_id)
            .is_err()
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let package = packages
            .get(&row.package_record_id)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if package.catalog != set.catalog
            || package.package.package_key != row.package_key
            || package.manifest.runtime_target != row.runtime_target
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        catalog_recognizer.validate_package(package)?;
        bind_live_platform_family(
            roots.live_platform_family,
            row.runtime_target,
            catalog_recognizer.expected_platform_family(),
        )?;
        set_packages.push(package);
        roots.package_record_ids.insert(row.package_record_id);
        roots.tree_ids.insert(package.tree_index.tree_sha256);
        roots
            .selected_package_record_ids
            .insert(row.package_record_id);
    }
    validate_catalog_set_tree_budget(set_packages)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)
}

fn require_authenticated_package(
    package: &PackageRecord,
    recognizer: &mut CatalogGenerationRecognizer<'_>,
) -> Result<(), ExtensionRepositoryError> {
    recognizer.authenticate(package.catalog)?;
    recognizer.validate_package(package)
}

fn bind_live_platform_family(
    live_family: &mut Option<StoredRuntimePlatformFamily>,
    target: StoredRuntimeTarget,
    expected_family: Option<StoredRuntimePlatformFamily>,
) -> Result<(), ExtensionRepositoryError> {
    let family = target.platform_family();
    if expected_family.is_some_and(|expected| expected != family)
        || live_family.is_some_and(|observed| observed != family)
    {
        // Device-local live roots may change backend within the macOS family,
        // but can never cross operating-system families. Dormant completed
        // records remain inert and are intentionally excluded from this check.
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    *live_family = Some(family);
    Ok(())
}

fn validate_build_inventory(
    state: &MaterializationState,
    records: &RecordInventory,
    trees: &TreeInventory,
) -> Result<(), ExtensionRepositoryError> {
    let Some(intent) = &state.build_intent else {
        if trees.stage.is_some() {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        return Ok(());
    };
    if let Some((digest, generation, _)) = &trees.stage {
        if *digest != intent.package_record.tree_index.tree_sha256
            || *generation != intent.generation
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    for (kind, (digest, _)) in &records.stages {
        let expected = match kind {
            RecordObjectKind::Package => intent.package_record_id,
            RecordObjectKind::TreeIndex => intent.package_record.tree_index.index_sha256,
            RecordObjectKind::Legal => intent.package_record.legal.sha256,
            RecordObjectKind::CatalogSet => {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous)
            }
        };
        if *digest != expected {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    Ok(())
}

fn inspect_control_stages(
    _root: &PrivateDirectory,
    shape: RootShape,
    checkpoint: MaterializationCheckpoint,
    journal: Option<&(PrivateComponent, MaterializationJournal)>,
    disposition: RecoveryDisposition,
) -> Result<(), ExtensionRepositoryError> {
    if shape.has_state_stage && shape.has_checkpoint_stage {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if shape.has_state_stage {
        if disposition != RecoveryDisposition::ApplyPrepared {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        journal.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
    }
    if shape.has_checkpoint_stage {
        if disposition != RecoveryDisposition::CheckpointPrepared {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let (_, prepared) = journal.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if checkpoint.generation >= prepared.generation {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    // A create-new control stage that still has its stage name is
    // definitively prepublication. Its bytes may be short, oversized, or torn
    // after power loss and are never authority. Cleanup removes it, then
    // apply_recovery deterministically regenerates the target from the final
    // journal and already-committed control state.
    Ok(())
}

fn cleanup_stages(
    root: &PrivateDirectory,
    records: &PrivateDirectory,
    journals: &PrivateDirectory,
    record_stages: &mut BTreeMap<RecordObjectKind, (Digest32, PrivateComponent)>,
    journal_stages: &mut Vec<PrivateComponent>,
    shape: RootShape,
    preserve_record_stages: bool,
) -> Result<(), ExtensionRepositoryError> {
    if !preserve_record_stages {
        for (_, (_, stage)) in std::mem::take(record_stages) {
            remove_required_control(records, &stage)?;
        }
    }
    for stage in journal_stages.drain(..) {
        remove_required_control(journals, &stage)?;
    }
    if shape.has_state_stage {
        remove_required_control(root, &names::state_stage())?;
    }
    if shape.has_checkpoint_stage {
        remove_required_control(root, &names::checkpoint_stage())?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_recovery(
    root: &PrivateDirectory,
    journals: &PrivateDirectory,
    mut state: MaterializationState,
    mut state_bytes: Vec<u8>,
    journal: Option<(PrivateComponent, MaterializationJournal)>,
    disposition: RecoveryDisposition,
    fault: FaultPoint,
) -> Result<(MaterializationState, Vec<u8>), ExtensionRepositoryError> {
    match disposition {
        RecoveryDisposition::Settled => {
            if journal.is_some() {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }
        RecoveryDisposition::ApplyPrepared => {
            let (name, prepared) = journal.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            let next_bytes = codec::encode(&prepared.next_state, MAX_MATERIALIZATION_STATE_BYTES)
                .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
            atomic_write_control(
                root,
                &names::state_file(),
                &names::state_stage(),
                &next_bytes,
                MAX_MATERIALIZATION_STATE_BYTES,
            )?;
            fail_if(fault, FaultPoint::AfterRecoveryState)?;
            write_checkpoint(
                root,
                MaterializationCheckpoint::new(prepared.generation, prepared.next_state_sha256),
            )?;
            fail_if(fault, FaultPoint::AfterRecoveryCheckpoint)?;
            remove_required_control(journals, &name)?;
            fail_if(fault, FaultPoint::AfterJournalRetirement)?;
            state = prepared.next_state;
            state_bytes = next_bytes;
        }
        RecoveryDisposition::CheckpointPrepared => {
            let (name, prepared) = journal.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            write_checkpoint(
                root,
                MaterializationCheckpoint::new(prepared.generation, prepared.next_state_sha256),
            )?;
            fail_if(fault, FaultPoint::AfterRecoveryCheckpoint)?;
            remove_required_control(journals, &name)?;
            fail_if(fault, FaultPoint::AfterJournalRetirement)?;
        }
        RecoveryDisposition::RetirePrepared => {
            let (name, _) = journal.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            remove_required_control(journals, &name)?;
            fail_if(fault, FaultPoint::AfterJournalRetirement)?;
        }
    }
    Ok((state, state_bytes))
}

fn fail_if(configured: FaultPoint, reached: FaultPoint) -> Result<(), ExtensionRepositoryError> {
    #[cfg(test)]
    if configured == reached {
        return Err(ExtensionRepositoryError::InjectedCrash);
    }
    #[cfg(not(test))]
    let _ = (configured, reached);
    Ok(())
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests;

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use std::path::Path;

    use zephium_private_fs::{LockedPrivateNamespace, PrivateFsError};

    #[test]
    fn repository_storage_remains_fail_closed_without_windows_private_fs() {
        assert!(matches!(
            LockedPrivateNamespace::open_or_create(Path::new("zephium-materialization-test")),
            Err(PrivateFsError::PrimitiveUnavailable)
        ));
    }
}
