use std::path::PathBuf;

use tempfile::TempDir;
use zephium_private_fs::{ByteLimit, LockedPrivateNamespace, PrivateComponent, PrivateDirectory};

use super::*;
use crate::materialization::records::tests::{catalog_set_fixture, package_record_fixture};
use crate::materialization::records::{
    CatalogSetPackageRow, CatalogSetRecord, LegalArtifactAnchor, ManifestAnchor,
    PackageIdentityAnchor, TreeIndexAnchor, CATALOG_SET_RECORD_SCHEMA_VERSION,
    PACKAGE_RECORD_SCHEMA_VERSION,
};
use crate::materialization::state::{
    DurablePackagePin, GarbageCollectionRetiredTree, GarbageCollectionTreeObject,
    HistoricalCatalogRole, MaterializationBuildIntent, MaterializationGarbageCollectionIntent,
    StoredBrowsingContext, MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION,
    MATERIALIZATION_GC_INTENT_SCHEMA_VERSION, MATERIALIZATION_JOURNAL_SCHEMA_VERSION,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};

struct Harness {
    _temporary: TempDir,
    repository_path: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        #[cfg(unix)]
        make_fixture_tree_removable(self._temporary.path());
    }
}

#[cfg(unix)]
fn make_fixture_tree_removable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt as _;

    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_dir() {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                make_fixture_tree_removable(&entry.path());
            }
        }
    } else if metadata.is_file() {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
}

impl Harness {
    fn new() -> Self {
        #[cfg(target_os = "macos")]
        let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let temporary = tempfile::tempdir_in("/tmp").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        Self {
            repository_path: temporary.path().join("repository"),
            _temporary: temporary,
        }
    }

    fn namespace(&self) -> LockedPrivateNamespace {
        LockedPrivateNamespace::open_or_create(&self.repository_path).unwrap()
    }

    fn open_with_fault(
        &self,
        fault: FaultPoint,
    ) -> Result<(LockedPrivateNamespace, MaterializationRuntime), ExtensionRepositoryError> {
        self.open_with_recognition(fault, true)
    }

    fn open_with_recognition(
        &self,
        fault: FaultPoint,
        recognizes: bool,
    ) -> Result<(LockedPrivateNamespace, MaterializationRuntime), ExtensionRepositoryError> {
        let namespace = self.namespace();
        let exists = namespace
            .directory()
            .list_components(8)
            .unwrap()
            .iter()
            .any(|entry| entry == &names::materialization_directory());
        // Structural fixtures exercise recovery mechanics without minting a
        // product authority bypass in non-test builds. Production open uses
        // exact active/rollback generation recognition.
        let runtime = if recognizes {
            open_or_recover_test_fixture(namespace.directory(), exists, fault)?
        } else {
            open_or_recover_unrecognized_test_fixture(namespace.directory(), exists, fault)?
        };
        Ok((namespace, runtime))
    }

    fn open(&self) -> (LockedPrivateNamespace, MaterializationRuntime) {
        self.open_with_fault(FaultPoint::None).unwrap()
    }

    fn handles(&self) -> Handles {
        let namespace = self.namespace();
        let materialization = namespace
            .directory()
            .open_private_child(&names::materialization_directory())
            .unwrap();
        let trees = materialization
            .open_private_child(&names::trees_directory())
            .unwrap();
        let records = materialization
            .open_private_child(&names::records_directory())
            .unwrap();
        let journals = materialization
            .open_private_child(&names::journals_directory())
            .unwrap();
        Handles {
            _namespace: namespace,
            materialization,
            trees,
            records,
            journals,
        }
    }
}

struct Handles {
    _namespace: LockedPrivateNamespace,
    materialization: PrivateDirectory,
    trees: PrivateDirectory,
    records: PrivateDirectory,
    journals: PrivateDirectory,
}

fn write_sealed(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    bytes: &[u8],
    maximum: usize,
) {
    directory
        .write_new_synced(name, bytes, ByteLimit::new(maximum).unwrap())
        .unwrap();
    assert!(directory.seal_verified_regular(name).unwrap().is_some());
}

fn replace_control(
    directory: &PrivateDirectory,
    destination: &PrivateComponent,
    stage: &PrivateComponent,
    bytes: &[u8],
    maximum: usize,
) {
    directory
        .write_new_synced(stage, bytes, ByteLimit::new(maximum).unwrap())
        .unwrap();
    if directory.regular_exists(destination).unwrap() {
        directory
            .replace_verified_regular(stage, destination)
            .unwrap();
    } else {
        directory
            .publish_noreplace_verified_regular(stage, destination)
            .unwrap();
    }
}

fn replace_settled_state(handles: &Handles, state: &MaterializationState) {
    let bytes = codec::encode(state, MAX_MATERIALIZATION_STATE_BYTES).unwrap();
    replace_control(
        &handles.materialization,
        &names::state_file(),
        &names::state_stage(),
        &bytes,
        MAX_MATERIALIZATION_STATE_BYTES,
    );
    let checkpoint = MaterializationCheckpoint::new(state.generation, codec::digest(&bytes));
    let checkpoint_bytes =
        codec::encode(&checkpoint, MAX_MATERIALIZATION_CHECKPOINT_BYTES).unwrap();
    replace_control(
        &handles.materialization,
        &names::checkpoint_file(),
        &names::checkpoint_stage(),
        &checkpoint_bytes,
        MAX_MATERIALIZATION_CHECKPOINT_BYTES,
    );
}

fn create_sealed_tree(trees: &PrivateDirectory, tree_digest: Digest32, poison_payload_mode: bool) {
    let object_name = names::tree_object(tree_digest);
    let tree = trees.create_new_private_child(&object_name).unwrap();
    let manifest = PrivateComponent::new("manifest.json").unwrap();
    tree.write_new_synced(
        &manifest,
        b"payload bytes are intentionally not metadata",
        ByteLimit::new(1024).unwrap(),
    )
    .unwrap();
    tree.seal_verified_regular(&manifest).unwrap().unwrap();
    let sealed = tree.seal().unwrap();
    if poison_payload_mode {
        sealed
            .with_verified_path(|path| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;

                    std::fs::set_permissions(
                        path.join("manifest.json"),
                        std::fs::Permissions::from_mode(0o000),
                    )
                    .unwrap();
                }
            })
            .unwrap();
    }
}

struct FixtureIds {
    package_id: Digest32,
    catalog_set_id: Digest32,
}

fn durable_pin(
    profile: u128,
    install: u128,
    catalog_set_record_id: Digest32,
    package_record_id: Digest32,
    native_incarnation: u64,
) -> DurablePackagePin {
    DurablePackagePin {
        profile_id: ProfileId::from(profile),
        install_id: ExtensionInstallId::from(install),
        browsing_context: StoredBrowsingContext::Regular,
        catalog_set_record_id,
        catalog_role: HistoricalCatalogRole::Active,
        package_record_id,
        native_incarnation,
    }
}

fn install_complete_fixture(handles: &Handles, poison_payload_mode: bool) -> FixtureIds {
    let package = package_record_fixture(20);
    install_package_fixture(handles, &package, poison_payload_mode)
}

fn install_package_fixture(
    handles: &Handles,
    package: &PackageRecord,
    poison_payload_mode: bool,
) -> FixtureIds {
    let package_id = package.record_id().unwrap();
    let catalog_set = catalog_set_fixture(package);
    let catalog_set_id = catalog_set.record_id().unwrap();

    create_sealed_tree(
        &handles.trees,
        package.tree_index.tree_sha256,
        poison_payload_mode,
    );
    write_sealed(
        &handles.records,
        &names::tree_index_object(package.tree_index.index_sha256),
        b"not parsed during startup",
        MAX_EXTENSION_TREE_INDEX_BYTES,
    );
    write_sealed(
        &handles.records,
        &names::legal_object(package.legal.sha256),
        b"not parsed during startup",
        MAX_EXTENSION_LEGAL_NOTICE_BYTES as usize,
    );
    write_sealed(
        &handles.records,
        &names::package_record(package_id),
        &package.canonical_bytes().unwrap(),
        MAX_PACKAGE_RECORD_BYTES,
    );
    write_sealed(
        &handles.records,
        &names::catalog_set_record(catalog_set_id),
        &catalog_set.canonical_bytes().unwrap(),
        MAX_CATALOG_SET_RECORD_BYTES,
    );
    FixtureIds {
        package_id,
        catalog_set_id,
    }
}

struct GcRecoveryFixture {
    _harness: Harness,
    state: MaterializationState,
    records: RecordInventory,
    trees: TreeInventory,
    catalog_object_ids: BTreeSet<Digest32>,
    package: PackageRecord,
    ids: FixtureIds,
}

impl GcRecoveryFixture {
    fn validate(&self, high_water: Option<CatalogAnchor>) -> Result<(), ExtensionRepositoryError> {
        validate_gc_intact_predelete_frontier(
            &self.state,
            &self.records,
            &self.trees,
            Some(&self.catalog_object_ids),
            high_water,
        )
    }
}

fn gc_recovery_fixture() -> GcRecoveryFixture {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let package = package_record_fixture(20);
    let ids = install_package_fixture(&handles, &package, false);
    let state = MaterializationState {
        generation: 1,
        gc_intent: Some(MaterializationGarbageCollectionIntent {
            schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
            generation: 1,
            cohort: Some(package.catalog),
            catalog_object_ids: vec![package.catalog.catalog_sha256],
            catalog_set_record_ids: vec![ids.catalog_set_id],
            package_record_ids: vec![ids.package_id],
            tree_index_ids: vec![package.tree_index.index_sha256],
            legal_artifact_ids: vec![package.legal.sha256],
            tree_objects: vec![GarbageCollectionTreeObject {
                tree_sha256: package.tree_index.tree_sha256,
                known_total_entry_count: Some(package.tree_index.total_entry_count),
                known_tree_bytes: Some(package.tree_index.tree_bytes),
            }],
            retired_trees: Vec::new(),
        }),
        ..MaterializationState::default()
    };
    state.validate().unwrap();
    let records = inspect_records(&handles.records).unwrap();
    let trees = inspect_trees(&handles.trees).unwrap();
    drop(handles);
    GcRecoveryFixture {
        _harness: harness,
        state,
        records,
        trees,
        catalog_object_ids: BTreeSet::from([package.catalog.catalog_sha256]),
        package,
        ids,
    }
}

fn install_prepared_transition(handles: &Handles, ids: &FixtureIds) -> PrivateComponent {
    let previous_bytes = read_required_control(
        &handles.materialization,
        &names::state_file(),
        MAX_MATERIALIZATION_STATE_BYTES,
    )
    .unwrap();
    let next_state = MaterializationState {
        generation: 1,
        completed_package_record_ids: vec![ids.package_id],
        candidate_catalog_set_id: Some(ids.catalog_set_id),
        ..MaterializationState::default()
    };
    let next_bytes = codec::encode(&next_state, MAX_MATERIALIZATION_STATE_BYTES).unwrap();
    let journal = MaterializationJournal {
        schema_version: MATERIALIZATION_JOURNAL_SCHEMA_VERSION,
        generation: 1,
        previous_state_sha256: codec::digest(&previous_bytes),
        next_state_sha256: codec::digest(&next_bytes),
        next_state,
    };
    let bytes = codec::encode(&journal, MAX_MATERIALIZATION_JOURNAL_BYTES).unwrap();
    let name = names::journal_file(1, codec::digest(&bytes)).unwrap();
    handles
        .journals
        .write_new_synced(
            &name,
            &bytes,
            ByteLimit::new(MAX_MATERIALIZATION_JOURNAL_BYTES).unwrap(),
        )
        .unwrap();
    name
}

#[test]
fn every_initialization_frontier_recovers_idempotently() {
    let frontiers = [
        FaultPoint::AfterMaterializationDirectory,
        FaultPoint::AfterTreesDirectory,
        FaultPoint::AfterRecordsDirectory,
        FaultPoint::AfterJournalsDirectory,
        FaultPoint::AfterInitialState,
        FaultPoint::AfterInitialCheckpoint,
    ];
    for frontier in frontiers {
        let harness = Harness::new();
        assert!(matches!(
            harness.open_with_fault(frontier),
            Err(ExtensionRepositoryError::InjectedCrash)
        ));
        let (namespace, runtime) = harness.open();
        assert_eq!(runtime._state, MaterializationState::default());
        drop(runtime);
        drop(namespace);
        assert_eq!(harness.open().1._state, MaterializationState::default());
    }
}

#[test]
fn pending_gc_intent_reopens_before_any_target_is_deleted() {
    let harness = Harness::new();
    let (namespace, runtime) = harness.open();
    drop(runtime);
    drop(namespace);
    let handles = harness.handles();
    let package = package_record_fixture(20);
    let ids = install_package_fixture(&handles, &package, false);
    let intent = MaterializationGarbageCollectionIntent {
        schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
        generation: 1,
        cohort: Some(package.catalog),
        catalog_object_ids: Vec::new(),
        catalog_set_record_ids: vec![ids.catalog_set_id],
        package_record_ids: vec![ids.package_id],
        tree_index_ids: vec![package.tree_index.index_sha256],
        legal_artifact_ids: vec![package.legal.sha256],
        tree_objects: vec![GarbageCollectionTreeObject {
            tree_sha256: package.tree_index.tree_sha256,
            known_total_entry_count: Some(package.tree_index.total_entry_count),
            known_tree_bytes: Some(package.tree_index.tree_bytes),
        }],
        retired_trees: Vec::new(),
    };
    let state = MaterializationState {
        generation: 1,
        gc_intent: Some(intent.clone()),
        ..MaterializationState::default()
    };
    state.validate().unwrap();
    replace_settled_state(&handles, &state);
    drop(handles);

    let (namespace, runtime) = harness.open();
    assert_eq!(runtime._state.gc_intent, Some(intent.clone()));
    assert_eq!(runtime._gc_intent, Some(intent.clone()));
    assert!(runtime._package_records.contains_key(&ids.package_id));
    assert!(runtime._catalog_sets.contains_key(&ids.catalog_set_id));
    assert!(runtime._sealed_tree_roots.is_empty());
    assert!(runtime
        ._tree_object_ids
        .contains(&package.tree_index.tree_sha256));
    drop(runtime);
    drop(namespace);

    let handles = harness.handles();
    let mut mismatched = intent;
    mismatched.generation = 2;
    mismatched.cohort = Some(package_record_fixture(90).catalog);
    let mismatched_state = MaterializationState {
        generation: 2,
        gc_intent: Some(mismatched),
        ..MaterializationState::default()
    };
    mismatched_state.validate().unwrap();
    replace_settled_state(&handles, &mismatched_state);
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
}

#[test]
fn gc_intact_frontier_requires_every_declared_target_class() {
    for missing in [
        "catalog",
        "catalog_set",
        "package",
        "tree_index",
        "legal",
        "tree",
    ] {
        let mut fixture = gc_recovery_fixture();
        match missing {
            "catalog" => {
                fixture
                    .catalog_object_ids
                    .remove(&fixture.package.catalog.catalog_sha256);
            }
            "catalog_set" => {
                fixture
                    .records
                    .catalog_sets
                    .remove(&fixture.ids.catalog_set_id);
            }
            "package" => {
                fixture.records.packages.remove(&fixture.ids.package_id);
            }
            "tree_index" => {
                fixture
                    .records
                    .tree_indexes
                    .remove(&fixture.package.tree_index.index_sha256);
            }
            "legal" => {
                fixture
                    .records
                    .legal_artifacts
                    .remove(&fixture.package.legal.sha256);
            }
            "tree" => {
                fixture
                    .trees
                    .objects
                    .remove(&fixture.package.tree_index.tree_sha256);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            fixture.validate(None),
            Err(ExtensionRepositoryError::RecoveryAmbiguous),
            "missing {missing} target must fail closed"
        );
    }
}

#[test]
fn gc_intact_frontier_rejects_reachable_or_semantically_false_targets() {
    let mut selected = gc_recovery_fixture();
    selected.state.candidate_catalog_set_id = Some(selected.ids.catalog_set_id);
    assert_eq!(
        selected.validate(None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );

    for shared_target in ["tree_index", "legal", "tree"] {
        let mut shared = gc_recovery_fixture();
        let intent = shared.state.gc_intent.as_mut().unwrap();
        match shared_target {
            "tree_index" => {
                intent.legal_artifact_ids.clear();
                intent.tree_objects.clear();
            }
            "legal" => {
                intent.tree_index_ids.clear();
                intent.tree_objects.clear();
            }
            "tree" => {
                intent.tree_index_ids.clear();
                intent.legal_artifact_ids.clear();
            }
            _ => unreachable!(),
        }
        let mut retained = package_record_fixture(90);
        retained.tree_index = shared.package.tree_index;
        retained.package.tree_sha256 = shared.package.package.tree_sha256;
        retained.legal = shared.package.legal.clone();
        let retained_id = retained.record_id().unwrap();
        let retained_set = catalog_set_fixture(&retained);
        let retained_set_id = retained_set.record_id().unwrap();
        shared
            .records
            .packages
            .insert(retained_id, retained.clone());
        shared
            .records
            .catalog_sets
            .insert(retained_set_id, retained_set);
        shared
            .catalog_object_ids
            .insert(retained.catalog.catalog_sha256);
        shared.state.completed_package_record_ids = vec![retained_id];
        shared.state.validate().unwrap();
        assert_eq!(
            shared.validate(None),
            Err(ExtensionRepositoryError::RecoveryAmbiguous),
            "shared {shared_target} target must remain retained"
        );
    }

    for mismatch in ["package_key", "runtime_target"] {
        let mut row_mismatch = gc_recovery_fixture();
        let mut set = row_mismatch
            .records
            .catalog_sets
            .remove(&row_mismatch.ids.catalog_set_id)
            .unwrap();
        match mismatch {
            "package_key" => {
                set.packages[0].package_key = Digest32::from_bytes([241; 32]);
            }
            "runtime_target" => {
                set.packages[0].runtime_target = StoredRuntimeTarget::MacosNative;
            }
            _ => unreachable!(),
        }
        let mismatched_set_id = set.record_id().unwrap();
        row_mismatch
            .records
            .catalog_sets
            .insert(mismatched_set_id, set);
        row_mismatch
            .state
            .gc_intent
            .as_mut()
            .unwrap()
            .catalog_set_record_ids = vec![mismatched_set_id];
        assert_eq!(
            row_mismatch.validate(None),
            Err(ExtensionRepositoryError::RecoveryAmbiguous),
            "mismatched target-set {mismatch} must fail closed"
        );
    }

    let mut retained_set_reference = gc_recovery_fixture();
    let mut dangling_set = catalog_set_fixture(&retained_set_reference.package);
    dangling_set.catalog.revision += 1;
    dangling_set.catalog.catalog_sha256 = Digest32::from_bytes([242; 32]);
    dangling_set.catalog.inventory_sha256 = Digest32::from_bytes([243; 32]);
    let dangling_set_id = dangling_set.record_id().unwrap();
    retained_set_reference
        .records
        .catalog_sets
        .insert(dangling_set_id, dangling_set);
    retained_set_reference
        .catalog_object_ids
        .insert(Digest32::from_bytes([242; 32]));
    assert_eq!(
        retained_set_reference.validate(None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );

    for metric in ["entries", "bytes"] {
        let mut wrong_tree_metrics = gc_recovery_fixture();
        let tree = &mut wrong_tree_metrics
            .state
            .gc_intent
            .as_mut()
            .unwrap()
            .tree_objects[0];
        match metric {
            "entries" => {
                tree.known_total_entry_count =
                    Some(wrong_tree_metrics.package.tree_index.total_entry_count + 1);
            }
            "bytes" => {
                tree.known_tree_bytes = Some(wrong_tree_metrics.package.tree_index.tree_bytes + 1);
            }
            _ => unreachable!(),
        }
        wrong_tree_metrics.state.validate().unwrap();
        assert_eq!(
            wrong_tree_metrics.validate(None),
            Err(ExtensionRepositoryError::RecoveryAmbiguous),
            "mismatched tree {metric} must fail closed"
        );
    }

    let mut partial_cohort = gc_recovery_fixture();
    let intent = partial_cohort.state.gc_intent.as_mut().unwrap();
    intent.tree_index_ids.clear();
    intent.legal_artifact_ids.clear();
    intent.tree_objects.clear();
    let mut omitted = partial_cohort.package.clone();
    omitted.package.package_key = Digest32::from_bytes([245; 32]);
    omitted.package.revision += 1;
    omitted.package.package_row_sha256 = Digest32::from_bytes([246; 32]);
    let omitted_id = omitted.record_id().unwrap();
    let omitted_set = catalog_set_fixture(&omitted);
    partial_cohort
        .records
        .packages
        .insert(omitted_id, omitted.clone());
    partial_cohort
        .records
        .catalog_sets
        .insert(omitted_set.record_id().unwrap(), omitted_set);
    partial_cohort.state.completed_package_record_ids = vec![omitted_id];
    partial_cohort.state.validate().unwrap();
    assert_eq!(
        partial_cohort.validate(None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );

    let high_water_target = gc_recovery_fixture();
    assert_eq!(
        high_water_target.validate(Some(high_water_target.package.catalog)),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );

    let mut missing_catalog_target = gc_recovery_fixture();
    missing_catalog_target
        .state
        .gc_intent
        .as_mut()
        .unwrap()
        .catalog_object_ids
        .clear();
    assert_eq!(
        missing_catalog_target.validate(None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
    assert_eq!(
        missing_catalog_target.validate(Some(missing_catalog_target.package.catalog)),
        Ok(())
    );
}

#[test]
fn gc_intact_frontier_keeps_unlisted_target_package_closure_intact() {
    let mut fixture = gc_recovery_fixture();
    fixture
        .state
        .gc_intent
        .as_mut()
        .unwrap()
        .legal_artifact_ids
        .clear();
    fixture
        .records
        .legal_artifacts
        .remove(&fixture.package.legal.sha256);
    fixture.state.validate().unwrap();
    assert_eq!(
        fixture.validate(None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
}

#[test]
fn gc_intact_frontier_enforces_target_catalog_set_tree_budget() {
    let mut fixture = gc_recovery_fixture();
    fixture.records.packages.clear();
    fixture.records.catalog_sets.clear();
    let mut rows = Vec::new();
    let mut package_ids = Vec::new();
    for index in 0..3_u8 {
        let mut package = fixture.package.clone();
        package.package.package_key = Digest32::from_bytes([150 + index; 32]);
        package.package.revision = u64::from(index) + 1;
        package.package.package_row_sha256 = Digest32::from_bytes([160 + index; 32]);
        package.tree_index.tree_bytes = zephium_extension_package::MAX_EXTENSION_TREE_BYTES;
        let package_id = package.record_id().unwrap();
        rows.push(CatalogSetPackageRow {
            package_key: package.package.package_key,
            runtime_target: package.manifest.runtime_target,
            package_record_id: package_id,
        });
        package_ids.push(package_id);
        fixture.records.packages.insert(package_id, package);
    }
    let set = CatalogSetRecord {
        schema_version: CATALOG_SET_RECORD_SCHEMA_VERSION,
        catalog: fixture.package.catalog,
        packages: rows,
    };
    let set_id = set.record_id().unwrap();
    fixture.records.catalog_sets.insert(set_id, set);
    package_ids.sort_unstable();
    let intent = fixture.state.gc_intent.as_mut().unwrap();
    intent.package_record_ids = package_ids;
    intent.catalog_set_record_ids = vec![set_id];
    intent.tree_objects[0].known_tree_bytes =
        Some(zephium_extension_package::MAX_EXTENSION_TREE_BYTES);
    fixture.state.validate().unwrap();
    assert_eq!(
        fixture.validate(None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
}

#[test]
fn gc_intact_frontier_enforces_the_preplan_completed_tree_budget() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let mut packages = Vec::new();
    let mut ids = Vec::new();
    for index in 0..9_u8 {
        let mut package = package_record_fixture(index * 20 + 1);
        package.tree_index.tree_bytes = zephium_extension_package::MAX_EXTENSION_TREE_BYTES;
        let package_ids = install_package_fixture(&handles, &package, false);
        if index > 0 {
            assert!(handles
                .records
                .remove_verified_regular(&names::catalog_set_record(package_ids.catalog_set_id))
                .unwrap());
        }
        packages.push(package);
        ids.push(package_ids);
    }
    let records = inspect_records(&handles.records).unwrap();
    let trees = inspect_trees(&handles.trees).unwrap();
    let target = &packages[0];
    let target_ids = &ids[0];
    let mut completed_package_record_ids = ids
        .iter()
        .skip(1)
        .map(|ids| ids.package_id)
        .collect::<Vec<_>>();
    completed_package_record_ids.sort_unstable();
    let state = MaterializationState {
        generation: 1,
        completed_package_record_ids,
        gc_intent: Some(MaterializationGarbageCollectionIntent {
            schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
            generation: 1,
            cohort: Some(target.catalog),
            catalog_object_ids: vec![target.catalog.catalog_sha256],
            catalog_set_record_ids: vec![target_ids.catalog_set_id],
            package_record_ids: vec![target_ids.package_id],
            tree_index_ids: vec![target.tree_index.index_sha256],
            legal_artifact_ids: vec![target.legal.sha256],
            tree_objects: vec![GarbageCollectionTreeObject {
                tree_sha256: target.tree_index.tree_sha256,
                known_total_entry_count: Some(target.tree_index.total_entry_count),
                known_tree_bytes: Some(target.tree_index.tree_bytes),
            }],
            retired_trees: Vec::new(),
        }),
        ..MaterializationState::default()
    };
    state.validate().unwrap();
    let catalog_ids = packages
        .iter()
        .map(|package| package.catalog.catalog_sha256)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        validate_gc_intact_predelete_frontier(&state, &records, &trees, Some(&catalog_ids), None,),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
}

#[test]
fn gc_intact_frontier_requires_exact_retired_tree_identity() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let digest = Digest32::from_bytes([244; 32]);
    drop(
        handles
            .trees
            .create_new_private_child(&names::tree_retired(digest, 1).unwrap())
            .unwrap(),
    );
    let records = inspect_records(&handles.records).unwrap();
    let trees = inspect_trees(&handles.trees).unwrap();
    let state = MaterializationState {
        generation: 2,
        gc_intent: Some(MaterializationGarbageCollectionIntent {
            schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
            generation: 2,
            cohort: None,
            catalog_object_ids: Vec::new(),
            catalog_set_record_ids: Vec::new(),
            package_record_ids: Vec::new(),
            tree_index_ids: Vec::new(),
            legal_artifact_ids: Vec::new(),
            tree_objects: Vec::new(),
            retired_trees: vec![GarbageCollectionRetiredTree {
                tree_sha256: digest,
                retirement_generation: 1,
            }],
        }),
        ..MaterializationState::default()
    };
    state.validate().unwrap();
    let catalog_ids = BTreeSet::new();
    assert_eq!(
        validate_gc_intact_predelete_frontier(&state, &records, &trees, Some(&catalog_ids), None,),
        Ok(())
    );
    let mismatched_state = MaterializationState {
        generation: 3,
        gc_intent: Some(MaterializationGarbageCollectionIntent {
            schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
            generation: 3,
            cohort: None,
            catalog_object_ids: Vec::new(),
            catalog_set_record_ids: Vec::new(),
            package_record_ids: Vec::new(),
            tree_index_ids: Vec::new(),
            legal_artifact_ids: Vec::new(),
            tree_objects: Vec::new(),
            retired_trees: vec![GarbageCollectionRetiredTree {
                tree_sha256: digest,
                retirement_generation: 2,
            }],
        }),
        ..MaterializationState::default()
    };
    mismatched_state.validate().unwrap();
    assert_eq!(
        validate_gc_intact_predelete_frontier(
            &mismatched_state,
            &records,
            &trees,
            Some(&catalog_ids),
            None,
        ),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
}

#[test]
fn invalid_prepared_gc_successor_is_rejected_before_control_mutation() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let previous_state_bytes = read_required_control(
        &handles.materialization,
        &names::state_file(),
        MAX_MATERIALIZATION_STATE_BYTES,
    )
    .unwrap();
    let previous_checkpoint_bytes = read_required_control(
        &handles.materialization,
        &names::checkpoint_file(),
        MAX_MATERIALIZATION_CHECKPOINT_BYTES,
    )
    .unwrap();
    let next_state = MaterializationState {
        generation: 1,
        gc_intent: Some(MaterializationGarbageCollectionIntent {
            schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
            generation: 1,
            cohort: None,
            catalog_object_ids: Vec::new(),
            catalog_set_record_ids: Vec::new(),
            package_record_ids: Vec::new(),
            tree_index_ids: Vec::new(),
            legal_artifact_ids: vec![Digest32::from_bytes([240; 32])],
            tree_objects: Vec::new(),
            retired_trees: Vec::new(),
        }),
        ..MaterializationState::default()
    };
    next_state.validate().unwrap();
    let next_state_bytes = codec::encode(&next_state, MAX_MATERIALIZATION_STATE_BYTES).unwrap();
    let journal = MaterializationJournal {
        schema_version: MATERIALIZATION_JOURNAL_SCHEMA_VERSION,
        generation: 1,
        previous_state_sha256: codec::digest(&previous_state_bytes),
        next_state_sha256: codec::digest(&next_state_bytes),
        next_state,
    };
    let journal_bytes = codec::encode(&journal, MAX_MATERIALIZATION_JOURNAL_BYTES).unwrap();
    let journal_name = names::journal_file(1, codec::digest(&journal_bytes)).unwrap();
    handles
        .journals
        .write_new_synced(
            &journal_name,
            &journal_bytes,
            ByteLimit::new(MAX_MATERIALIZATION_JOURNAL_BYTES).unwrap(),
        )
        .unwrap();
    drop(handles);

    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));

    let handles = harness.handles();
    assert_eq!(
        read_required_control(
            &handles.materialization,
            &names::state_file(),
            MAX_MATERIALIZATION_STATE_BYTES,
        )
        .unwrap(),
        previous_state_bytes
    );
    assert_eq!(
        read_required_control(
            &handles.materialization,
            &names::checkpoint_file(),
            MAX_MATERIALIZATION_CHECKPOINT_BYTES,
        )
        .unwrap(),
        previous_checkpoint_bytes
    );
    assert!(handles.journals.regular_exists(&journal_name).unwrap());
}

#[test]
fn existing_catalog_only_repository_migrates_without_reinitializing_it() {
    let harness = Harness::new();
    let namespace = harness.namespace();
    namespace
        .directory()
        .create_new_private_child(&crate::names::catalogs_directory())
        .unwrap();
    namespace
        .directory()
        .create_new_private_child(&crate::names::journals_directory())
        .unwrap();
    drop(namespace);

    let repository = crate::ExtensionRepository::open(harness.namespace()).unwrap();
    drop(repository);
    let handles = harness.handles();
    assert!(handles
        .materialization
        .regular_exists(&names::state_file())
        .unwrap());
    assert!(handles
        .materialization
        .regular_exists(&names::checkpoint_file())
        .unwrap());
}

#[test]
fn product_generation_recognition_is_lazy_and_never_trusts_structural_metadata_alone() {
    let empty =
        CatalogGenerationRecognizer::open(CatalogGenerationPolicy::Product, false, None, None)
            .unwrap();
    assert!(empty.product.is_none());

    let mut structural_only = package_record_fixture(19).catalog;
    structural_only.catalog_length = 1;
    if let Ok(mut recognizer) =
        CatalogGenerationRecognizer::open(CatalogGenerationPolicy::Product, true, None, None)
    {
        assert_eq!(
            recognizer.authenticate(structural_only),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }
}

fn catalog_bound_package_fixture() -> (ExtensionReleaseCatalog, PackageRecord) {
    let hex = |byte: u8| format!("{byte:02x}").repeat(32);
    let bytes = format!(
        concat!(
            r#"{{"schema_version":1,"catalog_revision":1,"created_unix":1700000001,"authority_id":"{}","admission_policy_sha256":"{}","packages":[{{"package_key":"{}","revision":1,"payload":{{"kind":"bundled_tree"}},"manifest_sha256":"{}","tree_sha256":"{}","tree_index_sha256":"{}","tree_index_length":1,"tree_file_count":1,"tree_bytes":4,"chromium":null,"provenance":{{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Example","redistribution":"Reviewed bundled release","legal_notice":{{"target":"licenses/example.txt","kind":"notice_bundle","length":1,"sha256":"{}"}},"corresponding_source":null}}}}]}}"#
        ),
        hex(1),
        hex(2),
        hex(7),
        hex(20),
        hex(40),
        hex(60),
        hex(80),
    )
    .into_bytes();
    let catalog = ExtensionReleaseCatalog::parse_canonical(&bytes).unwrap();
    let release_package = &catalog.packages()[0];
    let record = PackageRecord {
        schema_version: PACKAGE_RECORD_SCHEMA_VERSION,
        catalog: CatalogAnchor {
            authority_id: Digest32::from_bytes(catalog.authority().bytes()),
            revision: catalog.revision().get(),
            catalog_length: bytes.len() as u64,
            catalog_sha256: Digest32::from_bytes(catalog.digest().bytes()),
            inventory_sha256: Digest32::from_bytes([3; 32]),
        },
        package: PackageIdentityAnchor {
            authority_id: Digest32::from_bytes(release_package.identity().authority().bytes()),
            package_key: Digest32::from_bytes(release_package.identity().key().bytes()),
            revision: release_package.identity().revision().get(),
            payload: StoredPayloadIdentity::BundledTree,
            manifest_sha256: Digest32::from_bytes(
                release_package.identity().manifest_sha256().bytes(),
            ),
            tree_sha256: Digest32::from_bytes(release_package.identity().tree_sha256().bytes()),
            package_row_sha256: digest_package_row(release_package).unwrap(),
            chromium_manifest_key_sha256: None,
        },
        tree_index: TreeIndexAnchor {
            index_sha256: Digest32::from_bytes(release_package.tree_index_sha256().bytes()),
            index_length: release_package.tree_index_length(),
            tree_sha256: Digest32::from_bytes(release_package.identity().tree_sha256().bytes()),
            file_count: release_package.tree_file_count() as u32,
            directory_count: 0,
            total_entry_count: release_package.tree_file_count() as u32,
            tree_bytes: release_package.tree_bytes(),
        },
        manifest: ManifestAnchor {
            runtime_target: StoredRuntimeTarget::MacosCompatibility,
            compatibility_target: "macos.zephium-mv3-compat.v1".to_owned(),
            manifest_length: 1,
            manifest_sha256: Digest32::from_bytes(
                release_package.identity().manifest_sha256().bytes(),
            ),
            admission_sha256: Digest32::from_bytes([90; 32]),
        },
        legal: LegalArtifactAnchor {
            target: release_package
                .provenance()
                .legal_notice()
                .target()
                .as_str()
                .to_owned(),
            kind: StoredLegalArtifactKind::NoticeBundle,
            length: release_package.provenance().legal_notice().length(),
            sha256: Digest32::from_bytes(release_package.provenance().legal_notice().sha256()),
        },
    };
    record.validate().unwrap();
    (catalog, record)
}

#[test]
fn exact_catalog_row_authenticates_every_release_anchor() {
    let (catalog, baseline) = catalog_bound_package_fixture();
    assert_eq!(
        validate_package_against_catalog(&baseline, &catalog),
        Ok(())
    );

    let mut variants = Vec::new();
    let mut variant = baseline.clone();
    variant.package.package_row_sha256 = Digest32::from_bytes([101; 32]);
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.package.payload = StoredPayloadIdentity::AcquiredZip {
        length: 1,
        sha256: Digest32::from_bytes([102; 32]),
    };
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.package.manifest_sha256 = Digest32::from_bytes([103; 32]);
    variant.manifest.manifest_sha256 = variant.package.manifest_sha256;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.package.tree_sha256 = Digest32::from_bytes([104; 32]);
    variant.tree_index.tree_sha256 = variant.package.tree_sha256;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.tree_index.index_sha256 = Digest32::from_bytes([105; 32]);
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.tree_index.index_length += 1;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.tree_index.file_count += 1;
    variant.tree_index.total_entry_count += 1;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.tree_index.tree_bytes += 1;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.package.chromium_manifest_key_sha256 = Some(Digest32::from_bytes([106; 32]));
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.legal.target = "licenses/other.txt".to_owned();
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.legal.length += 1;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.legal.sha256 = Digest32::from_bytes([107; 32]);
    variants.push(variant);

    for variant in variants {
        assert_eq!(
            validate_package_against_catalog(&variant, &catalog),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }
}

#[test]
fn active_and_rollback_roots_are_bound_to_the_outer_high_water() {
    let (_, package) = catalog_bound_package_fixture();
    let mut active = package.catalog;
    active.revision = 2;
    assert_eq!(
        validate_catalog_role_against_high_water(
            ProductBundledCatalogGenerationRole::Active,
            active,
            Some(active),
        ),
        Ok(())
    );

    let mut different_active = active;
    different_active.catalog_sha256 = Digest32::from_bytes([111; 32]);
    assert_eq!(
        validate_catalog_role_against_high_water(
            ProductBundledCatalogGenerationRole::Active,
            different_active,
            Some(active),
        ),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );

    let mut rollback = active;
    rollback.revision = active.revision - 1;
    assert_eq!(
        validate_catalog_role_against_high_water(
            ProductBundledCatalogGenerationRole::Rollback,
            rollback,
            Some(active),
        ),
        Ok(())
    );
    // Before the new binary records its active generation, the durable
    // high-water is the prior binary's active generation, now classified as
    // an approved rollback. Existing roots must remain open at equality while
    // the new active root remains blocked until the outer state advances.
    assert_eq!(
        validate_catalog_role_against_high_water(
            ProductBundledCatalogGenerationRole::Rollback,
            rollback,
            Some(rollback),
        ),
        Ok(())
    );
    assert_eq!(
        validate_catalog_role_against_high_water(
            ProductBundledCatalogGenerationRole::Active,
            active,
            Some(rollback),
        ),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
    let mut equivocated_at_high_water = rollback;
    equivocated_at_high_water.catalog_sha256 = Digest32::from_bytes([113; 32]);
    assert_eq!(
        validate_catalog_role_against_high_water(
            ProductBundledCatalogGenerationRole::Rollback,
            equivocated_at_high_water,
            Some(rollback),
        ),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
    rollback.authority_id = Digest32::from_bytes([112; 32]);
    assert_eq!(
        validate_catalog_role_against_high_water(
            ProductBundledCatalogGenerationRole::Rollback,
            rollback,
            Some(active),
        ),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
}

#[test]
fn live_catalog_set_is_a_complete_exact_catalog_projection() {
    let (catalog, package) = catalog_bound_package_fixture();
    let mut recognizer = CatalogGenerationRecognizer {
        product: None,
        catalog_objects: None,
        catalog_high_water: None,
        admitted_catalogs: BTreeMap::from([(package.catalog, Some(catalog))]),
        structural_test_recognizes: false,
    };
    let mut set = catalog_set_fixture(&package);
    assert_eq!(recognizer.validate_catalog_set_projection(&set), Ok(()));

    set.packages[0].package_key = Digest32::from_bytes([108; 32]);
    assert_eq!(
        recognizer.validate_catalog_set_projection(&set),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
    set.packages.clear();
    assert_eq!(
        recognizer.validate_catalog_set_projection(&set),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );

    // Keep the variable mutable so this test also compiles through the same
    // authenticator shape used by production recovery.
    assert!(recognizer.authenticate(package.catalog).is_ok());
}

#[test]
fn only_live_roots_require_current_product_generation_recognition() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let ids = install_complete_fixture(&handles, false);
    let mut state = MaterializationState {
        generation: 1,
        completed_package_record_ids: vec![ids.package_id],
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);

    // A completed-only entry may be an obsolete rollback materialization. Its
    // full physical closure is retained, but it is not a product trust root.
    let (_, runtime) = harness
        .open_with_recognition(FaultPoint::None, false)
        .unwrap();
    assert_eq!(
        runtime._pin_roots._package_record_ids,
        BTreeSet::from([ids.package_id])
    );
    drop(runtime);

    let handles = harness.handles();
    state.candidate_catalog_set_id = Some(ids.catalog_set_id);
    replace_settled_state(&handles, &state);
    drop(handles);
    assert!(matches!(
        harness.open_with_recognition(FaultPoint::None, false),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
    assert!(harness
        .open_with_recognition(FaultPoint::None, true)
        .is_ok());

    let handles = harness.handles();
    state.candidate_catalog_set_id = None;
    state.package_pins = vec![durable_pin(1, 1, ids.catalog_set_id, ids.package_id, 1)];
    replace_settled_state(&handles, &state);
    drop(handles);
    assert!(matches!(
        harness.open_with_recognition(FaultPoint::None, false),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
    assert!(harness
        .open_with_recognition(FaultPoint::None, true)
        .is_ok());
}

#[test]
fn live_build_intent_requires_current_product_generation_recognition() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let package = package_record_fixture(22);
    let package_id = package.record_id().unwrap();
    let state = MaterializationState {
        generation: 1,
        build_intent: Some(MaterializationBuildIntent {
            schema_version: MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION,
            generation: 1,
            package_record_id: package_id,
            package_record: package,
        }),
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);

    assert!(matches!(
        harness.open_with_recognition(FaultPoint::None, false),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
    assert!(harness
        .open_with_recognition(FaultPoint::None, true)
        .is_ok());
}

#[test]
fn prepared_live_roots_require_recognition_before_recovery_mutates_state() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let ids = install_complete_fixture(&handles, false);
    install_prepared_transition(&handles, &ids);
    drop(handles);

    assert!(matches!(
        harness.open_with_recognition(FaultPoint::None, false),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
    assert_eq!(harness.open().1._state.generation, 1);
}

#[test]
fn every_recovery_frontier_converges_on_one_state() {
    for frontier in [
        FaultPoint::AfterRecoveryState,
        FaultPoint::AfterRecoveryCheckpoint,
        FaultPoint::AfterJournalRetirement,
    ] {
        let harness = Harness::new();
        drop(harness.open());
        let handles = harness.handles();
        let ids = install_complete_fixture(&handles, false);
        install_prepared_transition(&handles, &ids);
        drop(handles);

        assert!(matches!(
            harness.open_with_fault(frontier),
            Err(ExtensionRepositoryError::InjectedCrash)
        ));
        let (_, runtime) = harness.open();
        assert_eq!(runtime._state.generation, 1);
        assert_eq!(
            runtime._state.candidate_catalog_set_id,
            Some(ids.catalog_set_id)
        );
        assert!(runtime._pin_roots._tree_ids.len() == 1);
        assert!(runtime._journals.list_components(2).unwrap().is_empty());
    }
}

#[test]
fn owner_scoped_package_pins_retain_their_complete_exact_catalog_set() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let ids = install_complete_fixture(&handles, false);
    let state = MaterializationState {
        generation: 1,
        completed_package_record_ids: vec![ids.package_id],
        package_pins: vec![durable_pin(1, 1, ids.catalog_set_id, ids.package_id, 1)],
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);

    let (_, runtime) = harness.open();
    assert_eq!(
        runtime._pin_roots._catalog_set_ids,
        BTreeSet::from([ids.catalog_set_id])
    );
    assert_eq!(
        runtime._pin_roots._package_record_ids,
        BTreeSet::from([ids.package_id])
    );
    assert_eq!(runtime._pin_roots._tree_ids.len(), 1);
}

#[test]
fn owner_pin_recovery_rejects_missing_set_row_and_package_mapping() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let ids = install_complete_fixture(&handles, false);
    let state = MaterializationState {
        generation: 1,
        completed_package_record_ids: vec![ids.package_id],
        package_pins: vec![durable_pin(
            1,
            1,
            Digest32::from_bytes([240; 32]),
            ids.package_id,
            1,
        )],
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));

    let baseline = package_record_fixture(20);
    let mut second = baseline.clone();
    second.package.package_key = Digest32::from_bytes([211; 32]);
    second.package.package_row_sha256 = Digest32::from_bytes([212; 32]);
    second.package.tree_sha256 = Digest32::from_bytes([213; 32]);
    second.tree_index.tree_sha256 = second.package.tree_sha256;
    second.tree_index.index_sha256 = Digest32::from_bytes([214; 32]);
    second.legal.sha256 = Digest32::from_bytes([215; 32]);

    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let first_ids = install_package_fixture(&handles, &baseline, false);
    let second_ids = install_package_fixture(&handles, &second, false);
    let mut completed = vec![first_ids.package_id, second_ids.package_id];
    completed.sort_unstable();
    let state = MaterializationState {
        generation: 1,
        completed_package_record_ids: completed,
        package_pins: vec![durable_pin(
            1,
            1,
            first_ids.catalog_set_id,
            second_ids.package_id,
            1,
        )],
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));

    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let _first_ids = install_package_fixture(&handles, &baseline, false);
    let second_ids = install_package_fixture(&handles, &second, false);
    let mismatched_set = CatalogSetRecord {
        schema_version: CATALOG_SET_RECORD_SCHEMA_VERSION,
        catalog: baseline.catalog,
        packages: vec![CatalogSetPackageRow {
            package_key: baseline.package.package_key,
            runtime_target: baseline.manifest.runtime_target,
            package_record_id: second_ids.package_id,
        }],
    };
    let mismatched_set_id = mismatched_set.record_id().unwrap();
    write_sealed(
        &handles.records,
        &names::catalog_set_record(mismatched_set_id),
        &mismatched_set.canonical_bytes().unwrap(),
        MAX_CATALOG_SET_RECORD_BYTES,
    );
    let state = MaterializationState {
        generation: 1,
        completed_package_record_ids: vec![second_ids.package_id],
        package_pins: vec![durable_pin(
            1,
            1,
            mismatched_set_id,
            second_ids.package_id,
            1,
        )],
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
}

#[test]
fn startup_does_not_open_or_read_tree_payload_files() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let ids = install_complete_fixture(&handles, true);
    install_prepared_transition(&handles, &ids);
    create_sealed_tree(&handles.trees, Digest32::from_bytes([200; 32]), false);
    drop(handles);

    let (_, runtime) = harness.open();
    assert_eq!(runtime._state.generation, 1);
    assert_eq!(runtime._sealed_tree_roots.len(), 1);
    for tree in runtime._sealed_tree_roots.values() {
        tree.with_verified_path(|path| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;

                std::fs::set_permissions(
                    path.join("manifest.json"),
                    std::fs::Permissions::from_mode(0o400),
                )
                .unwrap();
            }
        })
        .unwrap();
    }
}

#[test]
fn writable_final_metadata_and_tree_roots_are_refused() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let package = package_record_fixture(30);
    let id = package.record_id().unwrap();
    handles
        .records
        .write_new_synced(
            &names::package_record(id),
            &package.canonical_bytes().unwrap(),
            ByteLimit::new(MAX_PACKAGE_RECORD_BYTES).unwrap(),
        )
        .unwrap();
    drop(handles);
    assert!(harness.open_with_fault(FaultPoint::None).is_err());

    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    handles
        .trees
        .create_new_private_child(&names::tree_object(Digest32::from_bytes([1; 32])))
        .unwrap();
    drop(handles);
    assert!(harness.open_with_fault(FaultPoint::None).is_err());
}

#[test]
fn package_record_commit_markers_must_be_rooted_and_completed_records_require_full_closure() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let package = package_record_fixture(40);
    let id = package.record_id().unwrap();
    write_sealed(
        &handles.records,
        &names::package_record(id),
        &package.canonical_bytes().unwrap(),
        MAX_PACKAGE_RECORD_BYTES,
    );
    drop(handles);

    // Package-record finals are commit markers, not inert residue. An orphan
    // marker is rejected even when its canonical record bytes are valid.
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));

    let handles = harness.handles();
    let state = MaterializationState {
        generation: 1,
        completed_package_record_ids: vec![id],
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
}

#[test]
fn inert_catalog_sets_may_have_dangling_rows_without_poisoning_recovery() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let package = package_record_fixture(41);
    let mut catalog_set = catalog_set_fixture(&package);
    catalog_set.packages[0].package_record_id = Digest32::from_bytes([250; 32]);
    let catalog_set_id = catalog_set.record_id().unwrap();
    write_sealed(
        &handles.records,
        &names::catalog_set_record(catalog_set_id),
        &catalog_set.canonical_bytes().unwrap(),
        MAX_CATALOG_SET_RECORD_BYTES,
    );
    drop(handles);

    let (_, runtime) = harness
        .open_with_recognition(FaultPoint::None, false)
        .unwrap();
    assert!(runtime._catalog_sets.contains_key(&catalog_set_id));
}

#[test]
fn dangling_state_duplicate_ids_and_slot_aliases_fail_closed() {
    let cases = [0_u8, 1, 2];
    for case in cases {
        let harness = Harness::new();
        drop(harness.open());
        let handles = harness.handles();
        let id = Digest32::from_bytes([7; 32]);
        let mut state = MaterializationState {
            generation: 1,
            completed_package_record_ids: vec![id],
            ..MaterializationState::default()
        };
        if case == 1 {
            state.completed_package_record_ids.push(id);
        }
        if case == 2 {
            state.completed_package_record_ids.clear();
            state.candidate_catalog_set_id = Some(id);
            state.current_catalog_set_id = Some(id);
        }
        replace_settled_state(&handles, &state);
        drop(handles);
        assert!(harness.open_with_fault(FaultPoint::None).is_err());
    }
}

#[test]
fn exact_prepublication_record_stage_is_cleaned_but_aliases_are_refused() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let stage = names::package_record_stage(Digest32::from_bytes([4; 32]));
    handles
        .records
        .write_new_synced(&stage, b"untrusted stage", ByteLimit::new(1024).unwrap())
        .unwrap();
    let journal_stage = names::journal_stage(1, Digest32::from_bytes([5; 32])).unwrap();
    handles
        .journals
        .write_new_synced(
            &journal_stage,
            b"{torn journal stage",
            ByteLimit::new(1024).unwrap(),
        )
        .unwrap();
    drop(handles);
    let (_, runtime) = harness.open();
    assert!(!runtime._records.regular_exists(&stage).unwrap());
    assert!(!runtime._journals.regular_exists(&journal_stage).unwrap());

    drop(runtime);
    let handles = harness.handles();
    handles
        .records
        .with_verified_path(|path| {
            let alias = path.join(format!("{}.PACKAGE.JSON", "05".repeat(32)));
            std::fs::write(&alias, b"{}").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;

                std::fs::set_permissions(alias, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
        })
        .unwrap();
    drop(handles);
    assert!(harness.open_with_fault(FaultPoint::None).is_err());
}

#[test]
fn at_most_one_stage_per_record_kind_is_accepted() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    for seed in [1_u8, 2] {
        handles
            .records
            .write_new_synced(
                &names::package_record_stage(Digest32::from_bytes([seed; 32])),
                b"stage",
                ByteLimit::new(16).unwrap(),
            )
            .unwrap();
    }
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
}

#[cfg(unix)]
#[test]
fn hostile_record_nodes_and_unknown_materialization_entries_fail_closed() {
    use std::os::unix::fs::symlink;

    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    handles
        .records
        .with_verified_path(|path| {
            symlink(
                "missing",
                path.join(format!("{}.package.json", "06".repeat(32))),
            )
            .unwrap();
        })
        .unwrap();
    drop(handles);
    assert!(harness.open_with_fault(FaultPoint::None).is_err());

    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    handles
        .materialization
        .write_new_synced(
            &PrivateComponent::new("unknown").unwrap(),
            b"x",
            ByteLimit::new(8).unwrap(),
        )
        .unwrap();
    drop(handles);
    assert!(harness.open_with_fault(FaultPoint::None).is_err());
}

#[test]
fn two_prepared_journals_and_tree_slot_aliases_are_ambiguous() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let ids = install_complete_fixture(&handles, false);
    let first = install_prepared_transition(&handles, &ids);
    let first_bytes =
        read_required_control(&handles.journals, &first, MAX_MATERIALIZATION_JOURNAL_BYTES)
            .unwrap();
    let second = names::journal_file(1, Digest32::from_bytes([99; 32])).unwrap();
    handles
        .journals
        .write_new_synced(
            &second,
            &first_bytes,
            ByteLimit::new(MAX_MATERIALIZATION_JOURNAL_BYTES).unwrap(),
        )
        .unwrap();
    drop(handles);
    assert!(harness.open_with_fault(FaultPoint::None).is_err());

    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let digest = Digest32::from_bytes([8; 32]);
    let object = handles
        .trees
        .create_new_private_child(&names::tree_object(digest))
        .unwrap();
    drop(object.seal().unwrap());
    let stage = handles
        .trees
        .create_new_private_child(&names::tree_stage(digest, 1).unwrap())
        .unwrap();
    drop(stage.seal().unwrap());
    drop(handles);
    assert!(harness.open_with_fault(FaultPoint::None).is_err());
}

#[test]
fn retired_tree_identities_recover_without_retaining_directory_handles() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();

    let mixed_digest = Digest32::from_bytes([31; 32]);
    let mixed_name = names::tree_retired(mixed_digest, 1).unwrap();
    let mixed = handles.trees.create_new_private_child(&mixed_name).unwrap();
    let sealed_child = mixed
        .create_new_private_child(&PrivateComponent::new("sealed-child").unwrap())
        .unwrap();
    drop(sealed_child.seal().unwrap());
    drop(mixed);

    let sealed_digest = Digest32::from_bytes([32; 32]);
    let sealed_name = names::tree_retired(sealed_digest, 2).unwrap();
    let sealed = handles
        .trees
        .create_new_private_child(&sealed_name)
        .unwrap();
    drop(sealed.seal().unwrap());
    drop(handles);

    let (_, runtime) = harness.open();
    assert_eq!(
        runtime._retired_tree_ids,
        BTreeSet::from([(mixed_digest, 1), (sealed_digest, 2)])
    );
}

#[test]
fn retired_tree_inventory_has_an_independent_bound_and_reserves_one_stage() {
    assert_eq!(
        names::MAX_TREE_ENTRIES,
        names::MAX_FINAL_PACKAGE_RECORDS + names::MAX_RETIRED_TREE_RECORDS + 1
    );

    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    for index in 0..=names::MAX_RETIRED_TREE_RECORDS {
        let mut bytes = [0_u8; 32];
        bytes[..8].copy_from_slice(&(index as u64).to_be_bytes());
        handles
            .trees
            .create_new_private_child(&names::tree_retired(Digest32::from_bytes(bytes), 1).unwrap())
            .unwrap();
    }
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
}

#[test]
fn durable_build_intent_recovers_matching_mixed_and_sealed_stages() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let package = package_record_fixture(90);
    let package_id = package.record_id().unwrap();
    let state = MaterializationState {
        generation: 1,
        build_intent: Some(MaterializationBuildIntent {
            schema_version: MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION,
            generation: 1,
            package_record_id: package_id,
            package_record: package.clone(),
        }),
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);

    let (_, runtime) = harness.open();
    assert!(runtime._build_intent.is_some());
    assert!(runtime._build_stage.is_none());
    drop(runtime);

    let handles = harness.handles();
    let stage_name = names::tree_stage(package.tree_index.tree_sha256, 1).unwrap();
    let stage = handles.trees.create_new_private_child(&stage_name).unwrap();
    let sealed_child = stage
        .create_new_private_child(&PrivateComponent::new("sealed-child").unwrap())
        .unwrap();
    drop(sealed_child.seal().unwrap());
    drop(stage);
    let record_stage = names::package_record_stage(package_id);
    handles
        .records
        .write_new_synced(
            &record_stage,
            &package.canonical_bytes().unwrap(),
            ByteLimit::new(MAX_PACKAGE_RECORD_BYTES).unwrap(),
        )
        .unwrap();
    drop(handles);

    let (_, runtime) = harness.open();
    assert!(matches!(
        runtime._build_stage,
        Some(MaterializationTreeCapability::Writable { .. })
    ));
    assert!(runtime._records.regular_exists(&record_stage).unwrap());
    drop(runtime);

    let handles = harness.handles();
    let stage = handles.trees.open_private_child(&stage_name).unwrap();
    drop(stage.seal().unwrap());
    drop(handles);
    let (_, runtime) = harness.open();
    assert!(matches!(
        runtime._build_stage,
        Some(MaterializationTreeCapability::Sealed { .. })
    ));
    assert_eq!(runtime._record_stages.len(), 1);
}

#[test]
fn tree_build_stage_without_its_exact_durable_intent_fails_closed() {
    for mismatch in [false, true] {
        let harness = Harness::new();
        drop(harness.open());
        let handles = harness.handles();
        let package = package_record_fixture(100);
        if mismatch {
            let state = MaterializationState {
                generation: 1,
                build_intent: Some(MaterializationBuildIntent {
                    schema_version: MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION,
                    generation: 1,
                    package_record_id: package.record_id().unwrap(),
                    package_record: package.clone(),
                }),
                ..MaterializationState::default()
            };
            replace_settled_state(&handles, &state);
        }
        let digest = if mismatch {
            Digest32::from_bytes([101; 32])
        } else {
            package.tree_index.tree_sha256
        };
        handles
            .trees
            .create_new_private_child(&names::tree_stage(digest, 1).unwrap())
            .unwrap();
        drop(handles);
        assert!(matches!(
            harness.open_with_fault(FaultPoint::None),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        ));
    }
}

#[test]
fn missing_initial_controls_never_demote_data_bearing_inventory() {
    for case in [
        "tree_object",
        "package_record",
        "retired_tree",
        "tree_stage",
        "record_stage",
        "journal_stage",
    ] {
        let harness = Harness::new();
        drop(harness.open());
        let handles = harness.handles();
        let digest = Digest32::from_bytes([case.as_bytes()[0]; 32]);
        match case {
            "tree_object" => create_sealed_tree(&handles.trees, digest, false),
            "package_record" => {
                let package = package_record_fixture(111);
                let id = package.record_id().unwrap();
                write_sealed(
                    &handles.records,
                    &names::package_record(id),
                    &package.canonical_bytes().unwrap(),
                    MAX_PACKAGE_RECORD_BYTES,
                );
            }
            "retired_tree" => {
                handles
                    .trees
                    .create_new_private_child(&names::tree_retired(digest, 1).unwrap())
                    .unwrap();
            }
            "tree_stage" => {
                handles
                    .trees
                    .create_new_private_child(&names::tree_stage(digest, 1).unwrap())
                    .unwrap();
            }
            "record_stage" => {
                handles
                    .records
                    .write_new_synced(
                        &names::package_record_stage(digest),
                        b"stage",
                        ByteLimit::new(16).unwrap(),
                    )
                    .unwrap();
            }
            "journal_stage" => {
                handles
                    .journals
                    .write_new_synced(
                        &names::journal_stage(1, digest).unwrap(),
                        b"stage",
                        ByteLimit::new(16).unwrap(),
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(handles
            .materialization
            .remove_verified_regular(&names::state_file())
            .unwrap());
        assert!(handles
            .materialization
            .remove_verified_regular(&names::checkpoint_file())
            .unwrap());
        drop(handles);

        assert!(matches!(
            harness.open_with_fault(FaultPoint::None),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        ));
    }
}

#[test]
fn either_missing_initial_control_refuses_existing_data() {
    for missing in ["state", "checkpoint"] {
        let harness = Harness::new();
        drop(harness.open());
        let handles = harness.handles();
        let package = package_record_fixture(if missing == "state" { 112 } else { 113 });
        let id = package.record_id().unwrap();
        write_sealed(
            &handles.records,
            &names::package_record(id),
            &package.canonical_bytes().unwrap(),
            MAX_PACKAGE_RECORD_BYTES,
        );
        let control = if missing == "state" {
            names::state_file()
        } else {
            names::checkpoint_file()
        };
        assert!(handles
            .materialization
            .remove_verified_regular(&control)
            .unwrap());
        drop(handles);

        assert!(matches!(
            harness.open_with_fault(FaultPoint::None),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        ));
    }
}

#[test]
fn torn_initialization_stages_are_safely_discarded() {
    let harness = Harness::new();
    let namespace = harness.namespace();
    let root = namespace
        .directory()
        .create_new_private_child(&names::materialization_directory())
        .unwrap();
    root.create_new_private_child(&names::trees_directory())
        .unwrap();
    root.create_new_private_child(&names::records_directory())
        .unwrap();
    root.create_new_private_child(&names::journals_directory())
        .unwrap();
    let state = MaterializationState::default();
    root.write_new_synced(
        &names::state_stage(),
        b"{torn state stage",
        ByteLimit::new(MAX_MATERIALIZATION_STATE_BYTES).unwrap(),
    )
    .unwrap();
    drop(root);
    drop(namespace);
    assert_eq!(harness.open().1._state, state);

    let handles = harness.handles();
    handles
        .materialization
        .remove_verified_regular(&names::checkpoint_file())
        .unwrap();
    handles
        .materialization
        .write_new_synced(
            &names::checkpoint_stage(),
            b"torn checkpoint stage",
            ByteLimit::new(MAX_MATERIALIZATION_CHECKPOINT_BYTES).unwrap(),
        )
        .unwrap();
    drop(handles);
    assert_eq!(harness.open().1._state, state);
}

#[test]
fn torn_transition_control_stages_are_discarded_and_regenerated() {
    for stage_bytes in [vec![b'x'], vec![b'x'; MAX_MATERIALIZATION_STATE_BYTES + 1]] {
        let harness = Harness::new();
        drop(harness.open());
        let handles = harness.handles();
        let ids = install_complete_fixture(&handles, false);
        install_prepared_transition(&handles, &ids);
        handles
            .materialization
            .write_new_synced(
                &names::state_stage(),
                &stage_bytes,
                ByteLimit::new(stage_bytes.len()).unwrap(),
            )
            .unwrap();
        drop(handles);

        let runtime = harness.open().1;
        assert_eq!(runtime._state.generation, 1);
        drop(runtime);
        let handles = harness.handles();
        assert!(!handles
            .materialization
            .regular_exists(&names::state_stage())
            .unwrap());
    }

    for stage_bytes in [
        vec![b'x'],
        vec![b'x'; MAX_MATERIALIZATION_CHECKPOINT_BYTES + 1],
    ] {
        let harness = Harness::new();
        drop(harness.open());
        let handles = harness.handles();
        let ids = install_complete_fixture(&handles, false);
        install_prepared_transition(&handles, &ids);
        drop(handles);
        assert!(matches!(
            harness.open_with_fault(FaultPoint::AfterRecoveryState),
            Err(ExtensionRepositoryError::InjectedCrash)
        ));
        let handles = harness.handles();
        handles
            .materialization
            .write_new_synced(
                &names::checkpoint_stage(),
                &stage_bytes,
                ByteLimit::new(stage_bytes.len()).unwrap(),
            )
            .unwrap();
        drop(handles);

        let runtime = harness.open().1;
        assert_eq!(runtime._state.generation, 1);
        drop(runtime);
        let handles = harness.handles();
        assert!(!handles
            .materialization
            .regular_exists(&names::checkpoint_stage())
            .unwrap());
    }
}

#[cfg(unix)]
#[test]
fn per_kind_record_ceiling_is_enforced_before_record_admission() {
    use std::os::unix::fs::PermissionsExt as _;

    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    handles
        .records
        .with_verified_path(|path| {
            for index in 0..=names::MAX_FINAL_PACKAGE_RECORDS {
                let mut bytes = [0_u8; 32];
                bytes[..8].copy_from_slice(&(index as u64).to_be_bytes());
                let name = names::package_record(Digest32::from_bytes(bytes));
                let path = path.join(name.as_str());
                std::fs::write(&path, b"invalid record must not be parsed").unwrap();
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o400)).unwrap();
            }
        })
        .unwrap();
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
}

#[test]
fn separate_index_and_legal_kinds_may_share_one_content_digest() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let mut package = package_record_fixture(60);
    package.legal.sha256 = package.tree_index.index_sha256;
    let package_id = package.record_id().unwrap();
    let catalog_set = catalog_set_fixture(&package);
    let catalog_set_id = catalog_set.record_id().unwrap();
    create_sealed_tree(&handles.trees, package.tree_index.tree_sha256, false);
    for (name, maximum) in [
        (
            names::tree_index_object(package.tree_index.index_sha256),
            MAX_EXTENSION_TREE_INDEX_BYTES,
        ),
        (
            names::legal_object(package.legal.sha256),
            MAX_EXTENSION_LEGAL_NOTICE_BYTES as usize,
        ),
    ] {
        write_sealed(&handles.records, &name, b"shared", maximum);
    }
    write_sealed(
        &handles.records,
        &names::package_record(package_id),
        &package.canonical_bytes().unwrap(),
        MAX_PACKAGE_RECORD_BYTES,
    );
    write_sealed(
        &handles.records,
        &names::catalog_set_record(catalog_set_id),
        &catalog_set.canonical_bytes().unwrap(),
        MAX_CATALOG_SET_RECORD_BYTES,
    );
    install_prepared_transition(
        &handles,
        &FixtureIds {
            package_id,
            catalog_set_id,
        },
    );
    drop(handles);
    assert_eq!(harness.open().1._state.generation, 1);
}

#[test]
fn content_address_conflicts_are_scoped_to_completed_records_and_build_intent() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let first = package_record_fixture(65);
    let mut second = first.clone();
    second.package.package_key = Digest32::from_bytes([210; 32]);
    second.package.package_row_sha256 = Digest32::from_bytes([211; 32]);
    second.package.tree_sha256 = Digest32::from_bytes([212; 32]);
    second.tree_index.tree_sha256 = second.package.tree_sha256;

    for package in [&first, &second] {
        create_sealed_tree(&handles.trees, package.tree_index.tree_sha256, false);
        let id = package.record_id().unwrap();
        write_sealed(
            &handles.records,
            &names::package_record(id),
            &package.canonical_bytes().unwrap(),
            MAX_PACKAGE_RECORD_BYTES,
        );
    }
    write_sealed(
        &handles.records,
        &names::tree_index_object(first.tree_index.index_sha256),
        b"one index object",
        MAX_EXTENSION_TREE_INDEX_BYTES,
    );
    write_sealed(
        &handles.records,
        &names::legal_object(first.legal.sha256),
        b"shared legal object",
        MAX_EXTENSION_LEGAL_NOTICE_BYTES as usize,
    );
    drop(handles);

    // Package-record finals may no longer remain unrooted, independent of the
    // content-address conflict they would otherwise describe.
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));

    let first_id = first.record_id().unwrap();
    let second_id = second.record_id().unwrap();
    let handles = harness.handles();
    let state = MaterializationState {
        generation: 1,
        completed_package_record_ids: vec![first_id],
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);

    // Rooting only one marker still leaves the other orphaned and is rejected.
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));

    let handles = harness.handles();
    let state = MaterializationState {
        generation: 1,
        completed_package_record_ids: vec![first_id],
        build_intent: Some(MaterializationBuildIntent {
            schema_version: MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION,
            generation: 1,
            package_record_id: second_id,
            package_record: second.clone(),
        }),
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));

    let handles = harness.handles();
    let mut completed = vec![first_id, second_id];
    completed.sort_unstable();
    let state = MaterializationState {
        generation: 1,
        completed_package_record_ids: completed,
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
}

#[test]
fn only_referenced_catalog_sets_bind_rows_to_package_key_and_runtime() {
    for mismatch in ["package_key", "runtime_target"] {
        let harness = Harness::new();
        drop(harness.open());
        let handles = harness.handles();
        let first = package_record_fixture(70);
        let mut second = first.clone();
        match mismatch {
            "package_key" => {
                second.package.package_key = Digest32::from_bytes([201; 32]);
            }
            "runtime_target" => {
                second.manifest.runtime_target =
                    crate::materialization::records::StoredRuntimeTarget::WindowsNative;
                second.manifest.compatibility_target = "windows.webview2.v1".to_owned();
                second.manifest.admission_sha256 = Digest32::from_bytes([200; 32]);
            }
            _ => unreachable!(),
        }
        let second_id = second.record_id().unwrap();
        let mut set = catalog_set_fixture(&first);
        set.packages[0].package_record_id = second_id;
        let set_id = set.record_id().unwrap();

        create_sealed_tree(&handles.trees, first.tree_index.tree_sha256, false);
        write_sealed(
            &handles.records,
            &names::tree_index_object(first.tree_index.index_sha256),
            b"index",
            MAX_EXTENSION_TREE_INDEX_BYTES,
        );
        write_sealed(
            &handles.records,
            &names::legal_object(first.legal.sha256),
            b"legal",
            MAX_EXTENSION_LEGAL_NOTICE_BYTES as usize,
        );
        write_sealed(
            &handles.records,
            &names::package_record(second_id),
            &second.canonical_bytes().unwrap(),
            MAX_PACKAGE_RECORD_BYTES,
        );
        write_sealed(
            &handles.records,
            &names::catalog_set_record(set_id),
            &set.canonical_bytes().unwrap(),
            MAX_CATALOG_SET_RECORD_BYTES,
        );
        // Root the package marker while leaving the malformed catalog set
        // unselected. Unlike package-record markers, catalog-set finals remain
        // inert until a slot or owner pin names them.
        let state = MaterializationState {
            generation: 1,
            completed_package_record_ids: vec![second_id],
            ..MaterializationState::default()
        };
        replace_settled_state(&handles, &state);
        drop(handles);
        assert!(harness.open_with_fault(FaultPoint::None).is_ok());

        let handles = harness.handles();
        let state = MaterializationState {
            generation: 1,
            completed_package_record_ids: vec![second_id],
            candidate_catalog_set_id: Some(set_id),
            ..MaterializationState::default()
        };
        replace_settled_state(&handles, &state);
        drop(handles);
        assert!(matches!(
            harness.open_with_fault(FaultPoint::None),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        ));
    }
}

#[test]
fn every_live_root_is_bound_to_one_device_platform_family() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let macos = package_record_fixture(20);
    let macos_ids = install_package_fixture(&handles, &macos, false);
    let mut windows = package_record_fixture(90);
    windows.manifest.runtime_target = StoredRuntimeTarget::WindowsNative;
    windows.manifest.compatibility_target = "windows.webview2.v1".to_owned();
    let windows_ids = install_package_fixture(&handles, &windows, false);
    let mut completed = vec![macos_ids.package_id, windows_ids.package_id];
    completed.sort_unstable();
    let state = MaterializationState {
        generation: 1,
        completed_package_record_ids: completed,
        candidate_catalog_set_id: Some(macos_ids.catalog_set_id),
        current_catalog_set_id: Some(windows_ids.catalog_set_id),
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);

    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
}

#[test]
fn owner_drain_is_one_catalog_sized_selection() {
    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let first = package_record_fixture(20);
    let first_ids = install_package_fixture(&handles, &first, false);
    let second = package_record_fixture(60);
    let second_ids = install_package_fixture(&handles, &second, false);
    let mut completed = vec![first_ids.package_id, second_ids.package_id];
    completed.sort_unstable();
    let state = MaterializationState {
        generation: 2,
        completed_package_record_ids: completed,
        package_pins: vec![
            durable_pin(1, 1, first_ids.catalog_set_id, first_ids.package_id, 1),
            durable_pin(1, 2, second_ids.catalog_set_id, second_ids.package_id, 2),
        ],
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));

    let harness = Harness::new();
    drop(harness.open());
    let handles = harness.handles();
    let baseline = package_record_fixture(120);
    let mut package_ids = Vec::new();
    let mut set_rows = Vec::new();
    for index in 1..=3_u8 {
        let mut package = baseline.clone();
        package.package.package_key = Digest32::from_bytes([index; 32]);
        package.package.package_row_sha256 = Digest32::from_bytes([index + 10; 32]);
        package.package.tree_sha256 = Digest32::from_bytes([index + 20; 32]);
        package.tree_index.tree_sha256 = package.package.tree_sha256;
        package.tree_index.index_sha256 = Digest32::from_bytes([index + 30; 32]);
        package.tree_index.tree_bytes = zephium_extension_package::MAX_EXTENSION_TREE_BYTES;
        package.legal.sha256 = Digest32::from_bytes([index + 40; 32]);
        let ids = install_package_fixture(&handles, &package, false);
        package_ids.push(ids.package_id);
        set_rows.push(CatalogSetPackageRow {
            package_key: package.package.package_key,
            runtime_target: package.manifest.runtime_target,
            package_record_id: ids.package_id,
        });
    }
    package_ids.sort_unstable();
    set_rows.sort_by_key(|row| row.package_key);
    let oversized_set = CatalogSetRecord {
        schema_version: CATALOG_SET_RECORD_SCHEMA_VERSION,
        catalog: baseline.catalog,
        packages: set_rows,
    };
    let oversized_set_id = oversized_set.record_id().unwrap();
    write_sealed(
        &handles.records,
        &names::catalog_set_record(oversized_set_id),
        &oversized_set.canonical_bytes().unwrap(),
        MAX_CATALOG_SET_RECORD_BYTES,
    );
    let package_pins = package_ids
        .iter()
        .enumerate()
        .map(|(index, package_record_id)| {
            durable_pin(
                1,
                index as u128 + 1,
                oversized_set_id,
                *package_record_id,
                index as u64 + 1,
            )
        })
        .collect();
    let state = MaterializationState {
        generation: package_ids.len() as u64,
        completed_package_record_ids: package_ids,
        package_pins,
        ..MaterializationState::default()
    };
    replace_settled_state(&handles, &state);
    drop(handles);
    assert!(matches!(
        harness.open_with_fault(FaultPoint::None),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
}

#[test]
fn completed_tree_budget_is_hard_and_deduplicates_shared_roots() {
    let mut state = MaterializationState {
        generation: 1,
        ..MaterializationState::default()
    };
    let mut packages = BTreeMap::new();
    for seed in 1..=9_u8 {
        let mut package = package_record_fixture(seed.wrapping_mul(10));
        package.tree_index.tree_bytes = zephium_extension_package::MAX_EXTENSION_TREE_BYTES;
        let id = package.record_id().unwrap();
        state.completed_package_record_ids.push(id);
        packages.insert(id, package);
    }
    state.completed_package_record_ids.sort_unstable();
    assert_eq!(
        validate_completed_budget(&state, &packages),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );

    let shared = packages.values().next().unwrap().tree_index.tree_sha256;
    for package in packages.values_mut() {
        package.tree_index.tree_sha256 = shared;
        package.package.tree_sha256 = shared;
    }
    assert_eq!(validate_completed_budget(&state, &packages), Ok(()));
}

#[test]
fn one_catalog_set_cannot_exceed_the_authenticated_catalog_tree_budget() {
    let baseline = package_record_fixture(110);
    let mut packages = BTreeMap::new();
    let mut rows = Vec::new();
    for seed in 1..=3_u8 {
        let mut package = baseline.clone();
        package.package.package_key = Digest32::from_bytes([seed; 32]);
        package.package.package_row_sha256 = Digest32::from_bytes([seed + 10; 32]);
        package.package.tree_sha256 = Digest32::from_bytes([seed + 20; 32]);
        package.tree_index.tree_sha256 = package.package.tree_sha256;
        package.tree_index.index_sha256 = Digest32::from_bytes([seed + 30; 32]);
        package.tree_index.tree_bytes = zephium_extension_package::MAX_EXTENSION_TREE_BYTES;
        let package_id = package.record_id().unwrap();
        rows.push(crate::materialization::records::CatalogSetPackageRow {
            package_key: package.package.package_key,
            runtime_target: package.manifest.runtime_target,
            package_record_id: package_id,
        });
        packages.insert(package_id, package);
    }
    let mut set = catalog_set_fixture(&baseline);
    set.packages = rows;
    let mut state = MaterializationState {
        generation: 1,
        completed_package_record_ids: packages.keys().copied().collect(),
        ..MaterializationState::default()
    };
    state.completed_package_record_ids.sort_unstable();
    let mut package_roots = BTreeSet::new();
    let mut tree_roots = BTreeSet::new();
    let mut selected_package_roots = BTreeSet::new();
    let mut live_platform_family = None;
    let mut recognizer = CatalogGenerationRecognizer::open(
        CatalogGenerationPolicy::StructuralTestFixture { recognizes: true },
        true,
        None,
        None,
    )
    .unwrap();
    recognizer.authenticate(set.catalog).unwrap();
    let mut roots = ReferencedCatalogSetRoots {
        package_record_ids: &mut package_roots,
        tree_ids: &mut tree_roots,
        selected_package_record_ids: &mut selected_package_roots,
        live_platform_family: &mut live_platform_family,
    };
    assert_eq!(
        validate_referenced_catalog_set(&set, &state, &packages, &mut roots, &mut recognizer,),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
}
