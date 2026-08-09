use std::collections::BTreeSet;
use std::fs;
use std::io::{Cursor, Read, Write as _};
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;
use zephium_core::extensions::ExtensionPackageKey;
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, BundledPackageAuthority,
    ProductExtensionRuntimeTarget,
};
use zephium_extension_package::CanonicalExtensionTreeIndex;
use zephium_private_fs::{
    ByteLimit, LockedPrivateNamespace, OpenedPrivateDirectory, PrivateComponent, PrivateDirectory,
    PrivateEntryName, PrivateFsOperationMeasurement, PrivateFsOperationSnapshot,
};

use super::*;
use crate::repository_e2e_fixture as fixture;
use crate::state::Digest32;

struct FixtureSource {
    catalog: crate::BundledReleaseCatalogSourceIdentity,
    package: Option<crate::BundledReleasePackageSourceIdentity>,
}

impl FixtureSource {
    fn active(catalog: &AdmittedBundledCatalog) -> Self {
        Self {
            catalog: crate::BundledReleaseCatalogSourceIdentity::from_generation(
                catalog.generation_anchor(),
            )
            .unwrap(),
            package: None,
        }
    }

    fn rollback(catalog: &AdmittedRollbackBundledCatalog) -> Self {
        Self {
            catalog: crate::BundledReleaseCatalogSourceIdentity::from_generation(
                catalog.generation_anchor(),
            )
            .unwrap(),
            package: None,
        }
    }
}

impl crate::BundledReleaseByteSource for FixtureSource {
    fn with_resource<T, E, F>(
        &mut self,
        resource: crate::BundledReleaseResource<'_>,
        callback: F,
    ) -> Result<Result<T, E>, crate::BundledReleaseSourceError>
    where
        F: FnOnce(&mut dyn Read) -> Result<T, E>,
    {
        assert_eq!(resource.package().catalog(), self.catalog);
        match self.package {
            Some(package) => assert_eq!(resource.package(), package),
            None => self.package = Some(resource.package()),
        }
        let bytes = match resource.kind() {
            crate::BundledReleaseResourceKind::TreeIndex { .. } => fixture::TREE_INDEX_BYTES,
            crate::BundledReleaseResourceKind::TreeFile { target, .. } => {
                fixture::tree_file_bytes(target.as_str())
                    .ok_or(crate::BundledReleaseSourceError::UnsupportedResource)?
            }
            crate::BundledReleaseResourceKind::LegalNotice { target, .. }
                if target.as_str() == "licenses/fixture.txt" =>
            {
                fixture::LEGAL_NOTICE_BYTES
            }
            _ => return Err(crate::BundledReleaseSourceError::UnsupportedResource),
        };
        assert_eq!(resource.kind().expected_length(), bytes.len() as u64);
        assert_eq!(
            resource.kind().expected_sha256(),
            <[u8; 32]>::from(Sha256::digest(bytes))
        );
        Ok(callback(&mut Cursor::new(bytes)))
    }
}

const fn runtime_target() -> ProductExtensionRuntimeTarget {
    #[cfg(target_os = "macos")]
    return ProductExtensionRuntimeTarget::MacosCompatibility;
    #[cfg(target_os = "linux")]
    return ProductExtensionRuntimeTarget::LinuxCompatibility;
    #[cfg(target_os = "windows")]
    return ProductExtensionRuntimeTarget::WindowsNative;
    #[allow(unreachable_code)]
    ProductExtensionRuntimeTarget::MacosCompatibility
}

fn package_key() -> ExtensionPackageKey {
    ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES)
}

fn establish_active_catalog_set(
    repository: &mut ExtensionRepository,
    active: &AdmittedBundledCatalog,
) {
    let mut materialize = FixtureSource::active(active);
    assert!(matches!(
        repository.materialize_active_bundled_package(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut materialize,
        ),
        Ok(crate::BundledPackageMaterializationOutcome::Materialized)
            | Ok(crate::BundledPackageMaterializationOutcome::IdempotentReplay)
    ));
    let selection = [crate::BundledPackageRuntimeSelection::new(
        package_key(),
        runtime_target(),
    )];
    let mut stage = FixtureSource::active(active);
    let identity = match repository
        .stage_active_bundled_catalog_set(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            &mut stage,
        )
        .unwrap()
    {
        crate::BundledCatalogSetStageOutcome::Staged(identity)
        | crate::BundledCatalogSetStageOutcome::IdempotentCandidate(identity) => identity,
        other => panic!("unexpected active stage outcome: {other:?}"),
    };
    let mut promote = FixtureSource::active(active);
    assert!(matches!(
        repository.promote_active_bundled_catalog_set(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            identity,
            &mut promote,
        ),
        Ok(crate::BundledCatalogSetPromotionOutcome::Promoted(_))
            | Ok(crate::BundledCatalogSetPromotionOutcome::IdempotentCurrent(
                _
            ))
    ));
}

#[derive(Debug, Eq, PartialEq)]
struct RetainedClosure {
    catalog_set_ids: BTreeSet<Digest32>,
    package_record_ids: BTreeSet<Digest32>,
    tree_ids: BTreeSet<Digest32>,
}

fn retained_closure(repository: &mut ExtensionRepository) -> RetainedClosure {
    let runtime = repository.writer_materialization().unwrap();
    let closure = RetainedClosure {
        catalog_set_ids: runtime._pin_roots._catalog_set_ids.clone(),
        package_record_ids: runtime._pin_roots._package_record_ids.clone(),
        tree_ids: runtime._pin_roots._tree_ids.clone(),
    };
    assert_eq!(closure.catalog_set_ids.len(), 1);
    assert_eq!(closure.package_record_ids.len(), 1);
    assert_eq!(closure.tree_ids.len(), 1);
    assert!(closure
        .catalog_set_ids
        .iter()
        .all(|id| runtime._catalog_sets.contains_key(id)));
    assert!(closure
        .package_record_ids
        .iter()
        .all(|id| runtime._package_records.contains_key(id)));
    assert!(closure
        .tree_ids
        .iter()
        .all(|id| runtime._tree_object_ids.contains(id)));
    closure
}

struct Harness {
    temporary: TempDir,
    repository_path: PathBuf,
}

impl Harness {
    fn new() -> Self {
        #[cfg(target_os = "macos")]
        let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let temporary = tempfile::tempdir_in("/tmp").unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let repository_path = temporary.path().join("repository");
        Self {
            temporary,
            repository_path,
        }
    }

    fn open(&self) -> ExtensionRepository {
        let namespace = LockedPrivateNamespace::open_or_create(&self.repository_path).unwrap();
        ExtensionRepository::open(namespace).unwrap()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        make_removable(self.temporary.path());
    }
}

fn make_removable(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                make_removable(&entry.path());
            }
        }
    } else if !metadata.file_type().is_symlink() {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

#[derive(Debug, Eq, PartialEq)]
struct SnapshotEntry {
    path: PathBuf,
    inode: u64,
    mode: u32,
    bytes: Option<Vec<u8>>,
}

fn snapshot(root: &Path) -> Vec<SnapshotEntry> {
    fn visit(root: &Path, current: &Path, output: &mut Vec<SnapshotEntry>) {
        let mut entries = fs::read_dir(current)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            let relative = entry.strip_prefix(root).unwrap().to_path_buf();
            let metadata = fs::symlink_metadata(&entry).unwrap();
            assert!(!metadata.file_type().is_symlink());
            if metadata.is_dir() {
                output.push(SnapshotEntry {
                    path: relative,
                    inode: metadata.ino(),
                    mode: metadata.mode(),
                    bytes: None,
                });
                visit(root, &entry, output);
            } else {
                output.push(SnapshotEntry {
                    path: relative,
                    inode: metadata.ino(),
                    mode: metadata.mode(),
                    bytes: Some(fs::read(entry).unwrap()),
                });
            }
        }
    }
    let mut output = Vec::new();
    visit(root, root, &mut output);
    output
}

fn install_residue(repository: &mut ExtensionRepository) -> (Digest32, Digest32) {
    install_residue_with_entries(repository, 1)
}

fn install_residue_with_entries(
    repository: &mut ExtensionRepository,
    entries: usize,
) -> (Digest32, Digest32) {
    let catalog = crate::codec::digest(fixture::ACTIVE_CATALOG_BYTES);
    crate::storage::ensure_catalog_object(
        repository.writer_catalogs(),
        catalog,
        fixture::ACTIVE_CATALOG_BYTES,
    )
    .unwrap();

    let tree = install_orphan_tree(repository, entries);
    (catalog, tree)
}

fn install_orphan_tree(repository: &mut ExtensionRepository, entries: usize) -> Digest32 {
    let tree = Digest32::from_bytes([0xf1; 32]);
    let tree_name = materialization::gc_tree_object(tree);
    let directory = repository
        .writer_materialization()
        .unwrap()
        ._trees
        .create_new_private_child(&tree_name)
        .unwrap();
    for index in 0..entries {
        let payload = PrivateEntryName::new(format!("payload-{index:04}.js")).unwrap();
        directory
            .write_new_entry_synced(&payload, b"fixture", ByteLimit::new(32).unwrap())
            .unwrap();
        assert!(directory
            .seal_verified_entry_regular(&payload)
            .unwrap()
            .is_some());
    }
    let sealed = directory.seal().unwrap();
    drop(sealed);
    tree
}

struct ProductShapedResidue {
    active_catalog: Digest32,
    garbage_catalog: Digest32,
    garbage_tree: Digest32,
    retained: RetainedClosure,
}

fn install_product_shaped_residue(repository: &mut ExtensionRepository) -> ProductShapedResidue {
    let authority = BundledPackageAuthority::product().unwrap();
    let active = authority
        .admit_catalog(fixture::ACTIVE_CATALOG_BYTES)
        .unwrap();
    let rollback = authority
        .admit_rollback_catalog(fixture::ROLLBACK_CATALOG_BYTES)
        .unwrap();
    establish_active_catalog_set(repository, &active);
    repository
        .writer_ensure_rollback_catalog(&rollback, fixture::ROLLBACK_CATALOG_BYTES)
        .unwrap();

    let active_catalog = crate::codec::digest(fixture::ACTIVE_CATALOG_BYTES);
    let garbage_catalog = crate::codec::digest(fixture::ROLLBACK_CATALOG_BYTES);
    assert_ne!(active_catalog, garbage_catalog);
    let retained = retained_closure(repository);
    let garbage_tree = install_orphan_tree(repository, 1);
    ProductShapedResidue {
        active_catalog,
        garbage_catalog,
        garbage_tree,
        retained,
    }
}

fn begin_one_batch(repository: &mut ExtensionRepository) -> MaterializationGarbageCollectionIntent {
    repository.validate_outer_controls_or_seal().unwrap();
    let high_water = repository
        .writer_catalog_high_water()
        .map(CatalogAnchor::from_high_water);
    let catalog_ids = repository.writer_catalog_object_ids().clone();
    let plan = materialization::plan_garbage_collection(
        repository.writer_materialization().unwrap(),
        high_water,
        &catalog_ids,
    )
    .unwrap()
    .unwrap();
    let runtime = repository.writer_take_materialization().unwrap();
    repository
        .finish_collection_transition(begin_garbage_collection(runtime, plan))
        .unwrap();
    repository
        .writer_materialization()
        .unwrap()
        ._gc_intent
        .clone()
        .unwrap()
}

fn fixture_digest(domain: &str, index: usize) -> Digest32 {
    let mut digest = Sha256::new();
    digest.update(b"zephium/internal-gc-maximum-cohort/v1/");
    digest.update(domain.as_bytes());
    digest.update(b"/");
    digest.update(index.to_string().as_bytes());
    Digest32::from_bytes(digest.finalize().into())
}

fn lower_hex(bytes: [u8; 32]) -> String {
    bytes
        .into_iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_sealed_object(
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

#[derive(Clone, Copy)]
struct MaximumCohortTree {
    tree_sha256: Digest32,
    index_sha256: Digest32,
    index_length: u64,
    file_count: u32,
    directory_count: u32,
    total_entry_count: u32,
    tree_bytes: u64,
    legal_sha256: Digest32,
    legal_length: u64,
}

#[derive(Serialize)]
struct FixtureMaterializationCheckpoint {
    schema_version: u32,
    generation: u64,
    state_sha256: Digest32,
}

struct MaximumCohortFixture {
    active_catalog: Digest32,
    garbage_catalog: Digest32,
    catalog_set_record_ids: BTreeSet<Digest32>,
    package_record_ids: BTreeSet<Digest32>,
    tree_index_ids: BTreeSet<Digest32>,
    legal_artifact_ids: BTreeSet<Digest32>,
    tree_object_ids: BTreeSet<Digest32>,
    actual_tree_entries: usize,
}

fn install_maximum_cohort(repository: &mut ExtensionRepository) -> MaximumCohortFixture {
    let authority = BundledPackageAuthority::product().unwrap();
    let active = authority
        .admit_catalog(fixture::ACTIVE_CATALOG_BYTES)
        .unwrap();
    let rollback = authority
        .admit_rollback_catalog(fixture::ROLLBACK_CATALOG_BYTES)
        .unwrap();
    let _catalog_outcome = repository
        .record_bundled_catalog(&active, fixture::ACTIVE_CATALOG_BYTES)
        .unwrap();

    let mut materialize = FixtureSource::rollback(&rollback);
    assert!(matches!(
        repository.materialize_rollback_bundled_package(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut materialize,
        ),
        Ok(crate::BundledPackageMaterializationOutcome::Materialized)
            | Ok(crate::BundledPackageMaterializationOutcome::IdempotentReplay)
    ));
    let selection = [crate::BundledPackageRuntimeSelection::new(
        package_key(),
        runtime_target(),
    )];
    let mut stage = FixtureSource::rollback(&rollback);
    assert!(matches!(
        repository.stage_rollback_bundled_catalog_set(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            &selection,
            &mut stage,
        ),
        Ok(crate::BundledCatalogSetStageOutcome::Staged(_))
            | Ok(crate::BundledCatalogSetStageOutcome::IdempotentCandidate(_))
    ));

    let mut runtime = repository.writer_take_materialization().unwrap();
    assert_eq!(runtime._package_records.len(), 1);
    assert_eq!(runtime._catalog_sets.len(), 1);
    let template_package = runtime._package_records.values().next().unwrap().clone();
    let template_package_id = template_package.record_id().unwrap();
    let template_set = runtime._catalog_sets.values().next().unwrap().clone();
    let template_set_id = template_set.record_id().unwrap();
    assert_eq!(template_set.packages.len(), 1);
    assert_eq!(
        template_set.packages[0].package_record_id,
        template_package_id
    );
    assert_eq!(template_package.tree_index.file_count, 3);
    assert_eq!(template_package.tree_index.directory_count, 0);
    assert_eq!(template_package.tree_index.total_entry_count, 3);

    let mut trees = vec![MaximumCohortTree {
        tree_sha256: template_package.tree_index.tree_sha256,
        index_sha256: template_package.tree_index.index_sha256,
        index_length: template_package.tree_index.index_length,
        file_count: template_package.tree_index.file_count,
        directory_count: template_package.tree_index.directory_count,
        total_entry_count: template_package.tree_index.total_entry_count,
        tree_bytes: template_package.tree_index.tree_bytes,
        legal_sha256: template_package.legal.sha256,
        legal_length: template_package.legal.length,
    }];
    for group in 1..MAX_GC_TREE_JOBS {
        let payload_name = format!("payload-{group:02}.js");
        let payload_bytes = format!("maximum-cohort-tree-{group:02}").into_bytes();
        let payload_sha256: [u8; 32] = Sha256::digest(&payload_bytes).into();
        let index_bytes = format!(
            "{{\"schema_version\":1,\"files\":[{{\"path\":\"manifest.json\",\"length\":{},\"sha256\":\"{}\"}},{{\"path\":\"{}\",\"length\":{},\"sha256\":\"{}\"}}]}}",
            fixture::MANIFEST_BYTES.len(),
            lower_hex(Sha256::digest(fixture::MANIFEST_BYTES).into()),
            payload_name,
            payload_bytes.len(),
            lower_hex(payload_sha256),
        )
        .into_bytes();
        let index = CanonicalExtensionTreeIndex::parse_canonical(&index_bytes).unwrap();
        assert_eq!(
            index.manifest_sha256().bytes(),
            Sha256::digest(fixture::MANIFEST_BYTES).as_slice()
        );

        let tree_sha256 = Digest32::from_bytes(index.tree_sha256().bytes());
        let index_sha256 = Digest32::from_bytes(index.index_sha256().bytes());
        let legal_bytes = format!("maximum-cohort-legal-{group:02}").into_bytes();
        let legal_sha256 = Digest32::from_bytes(Sha256::digest(&legal_bytes).into());

        let tree_name = materialization::gc_tree_object(tree_sha256);
        let directory = runtime._trees.create_new_private_child(&tree_name).unwrap();
        for (name, bytes) in [
            ("manifest.json", fixture::MANIFEST_BYTES),
            (payload_name.as_str(), payload_bytes.as_slice()),
        ] {
            let name = PrivateEntryName::new(name).unwrap();
            directory
                .write_new_entry_synced(&name, bytes, ByteLimit::new(bytes.len()).unwrap())
                .unwrap();
            assert!(directory
                .seal_verified_entry_regular(&name)
                .unwrap()
                .is_some());
        }
        drop(directory.seal().unwrap());
        write_sealed_object(
            &runtime._records,
            &materialization::gc_tree_index_object(index_sha256),
            &index_bytes,
            index_bytes.len(),
        );
        write_sealed_object(
            &runtime._records,
            &materialization::gc_legal_object(legal_sha256),
            &legal_bytes,
            legal_bytes.len(),
        );
        runtime._tree_object_ids.insert(tree_sha256);
        runtime._tree_index_ids.insert(index_sha256);
        runtime._legal_artifact_ids.insert(legal_sha256);
        trees.push(MaximumCohortTree {
            tree_sha256,
            index_sha256,
            index_length: index.index_bytes(),
            file_count: u32::try_from(index.files().len()).unwrap(),
            directory_count: u32::try_from(index.implicit_directory_count()).unwrap(),
            total_entry_count: u32::try_from(index.total_entry_count()).unwrap(),
            tree_bytes: index.total_bytes(),
            legal_sha256,
            legal_length: u64::try_from(legal_bytes.len()).unwrap(),
        });
    }
    assert_eq!(trees.len(), MAX_GC_TREE_JOBS);

    let mut packages = Vec::with_capacity(MAX_GC_PACKAGE_RECORD_TARGETS);
    packages.push((template_package_id, template_package.clone()));
    for index in 1..MAX_GC_PACKAGE_RECORD_TARGETS {
        let tree = trees[index % trees.len()];
        let mut package = template_package.clone();
        package.package.package_key = fixture_digest("package-key", index);
        package.package.revision = u64::try_from(index).unwrap() + 1;
        package.package.package_row_sha256 = fixture_digest("package-row", index);
        package.package.tree_sha256 = tree.tree_sha256;
        package.tree_index.index_sha256 = tree.index_sha256;
        package.tree_index.index_length = tree.index_length;
        package.tree_index.tree_sha256 = tree.tree_sha256;
        package.tree_index.file_count = tree.file_count;
        package.tree_index.directory_count = tree.directory_count;
        package.tree_index.total_entry_count = tree.total_entry_count;
        package.tree_index.tree_bytes = tree.tree_bytes;
        package.legal.sha256 = tree.legal_sha256;
        package.legal.length = tree.legal_length;
        let record_id = package.record_id().unwrap();
        let bytes = package.canonical_bytes().unwrap();
        write_sealed_object(
            &runtime._records,
            &materialization::gc_package_record(record_id),
            &bytes,
            materialization::MAX_PACKAGE_RECORD_BYTES,
        );
        assert!(runtime
            ._package_records
            .insert(record_id, package.clone())
            .is_none());
        packages.push((record_id, package));
    }
    assert_eq!(packages.len(), MAX_GC_PACKAGE_RECORD_TARGETS);

    let mut catalog_set_record_ids = BTreeSet::from([template_set_id]);
    for (package_record_id, package) in packages.iter().skip(1).take(MAX_GC_CATALOG_SET_TARGETS - 1)
    {
        let mut set = template_set.clone();
        let mut row = set.packages[0];
        row.package_key = package.package.package_key;
        row.runtime_target = package.manifest.runtime_target;
        row.package_record_id = *package_record_id;
        set.packages = vec![row];
        let record_id = set.record_id().unwrap();
        let bytes = set.canonical_bytes().unwrap();
        write_sealed_object(
            &runtime._records,
            &materialization::gc_catalog_set_record(record_id),
            &bytes,
            materialization::MAX_CATALOG_SET_RECORD_BYTES,
        );
        assert!(runtime._catalog_sets.insert(record_id, set).is_none());
        assert!(catalog_set_record_ids.insert(record_id));
    }
    assert_eq!(catalog_set_record_ids.len(), MAX_GC_CATALOG_SET_TARGETS);

    let mut state = runtime._state.clone();
    state.generation = state.generation.checked_add(1).unwrap();
    state.completed_package_record_ids = packages.iter().map(|(id, _)| *id).collect();
    state.completed_package_record_ids.sort_unstable();
    state.candidate_catalog_set_id = None;
    state.current_catalog_set_id = None;
    state.previous_catalog_set_id = None;
    state.package_pins.clear();
    state.build_intent = None;
    state.gc_intent = None;
    state.validate().unwrap();
    let state_bytes =
        crate::codec::encode(&state, materialization::MAX_MATERIALIZATION_STATE_BYTES).unwrap();
    let checkpoint = FixtureMaterializationCheckpoint {
        schema_version: 1,
        generation: state.generation,
        state_sha256: crate::codec::digest(&state_bytes),
    };
    let checkpoint_bytes = crate::codec::encode(
        &checkpoint,
        materialization::MAX_MATERIALIZATION_CHECKPOINT_BYTES,
    )
    .unwrap();
    crate::storage::atomic_write_control(
        &runtime._root,
        &PrivateComponent::new("state.json").unwrap(),
        &PrivateComponent::new("state.stage").unwrap(),
        &state_bytes,
        materialization::MAX_MATERIALIZATION_STATE_BYTES,
    )
    .unwrap();
    crate::storage::atomic_write_control(
        &runtime._root,
        &PrivateComponent::new("recovery-checkpoint.json").unwrap(),
        &PrivateComponent::new("recovery-checkpoint.stage").unwrap(),
        &checkpoint_bytes,
        materialization::MAX_MATERIALIZATION_CHECKPOINT_BYTES,
    )
    .unwrap();
    drop(runtime);
    repository.writer_recover_materialization().unwrap();

    let active_catalog = crate::codec::digest(fixture::ACTIVE_CATALOG_BYTES);
    let garbage_catalog = crate::codec::digest(fixture::ROLLBACK_CATALOG_BYTES);
    let package_record_ids = packages.into_iter().map(|(id, _)| id).collect();
    let tree_index_ids = trees.iter().map(|tree| tree.index_sha256).collect();
    let legal_artifact_ids = trees.iter().map(|tree| tree.legal_sha256).collect();
    let tree_object_ids = trees.iter().map(|tree| tree.tree_sha256).collect();
    let actual_tree_entries = trees
        .iter()
        .map(|tree| tree.total_entry_count as usize)
        .sum();
    let fixture = MaximumCohortFixture {
        active_catalog,
        garbage_catalog,
        catalog_set_record_ids,
        package_record_ids,
        tree_index_ids,
        legal_artifact_ids,
        tree_object_ids,
        actual_tree_entries,
    };
    assert_maximum_cohort_is_intact(repository, &fixture);
    fixture
}

fn assert_maximum_cohort_is_intact(
    repository: &mut ExtensionRepository,
    fixture: &MaximumCohortFixture,
) {
    assert_eq!(repository.writer_catalog_object_ids().len(), 2);
    assert!(repository
        .writer_catalog_object_ids()
        .contains(&fixture.active_catalog));
    assert!(repository
        .writer_catalog_object_ids()
        .contains(&fixture.garbage_catalog));
    assert_eq!(
        repository
            .writer_catalog_high_water()
            .map(|row| row.catalog_sha256),
        Some(fixture.active_catalog)
    );
    let runtime = repository.writer_materialization().unwrap();
    assert!(runtime._pin_roots._catalog_set_ids.is_empty());
    assert_eq!(
        runtime._pin_roots._package_record_ids,
        fixture.package_record_ids
    );
    assert_eq!(runtime._pin_roots._tree_ids, fixture.tree_object_ids);
    assert_eq!(
        runtime
            ._catalog_sets
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        fixture.catalog_set_record_ids
    );
    assert_eq!(
        runtime
            ._package_records
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        fixture.package_record_ids
    );
    assert_eq!(runtime._tree_index_ids, fixture.tree_index_ids);
    assert_eq!(runtime._legal_artifact_ids, fixture.legal_artifact_ids);
    assert_eq!(runtime._tree_object_ids, fixture.tree_object_ids);
    assert_eq!(runtime._state.completed_package_record_ids.len(), 32);
    assert!(runtime._state.candidate_catalog_set_id.is_none());
    assert!(runtime._state.current_catalog_set_id.is_none());
    assert!(runtime._state.previous_catalog_set_id.is_none());
    assert!(runtime._state.package_pins.is_empty());
}

fn assert_maximum_intent(
    intent: &MaterializationGarbageCollectionIntent,
    fixture: &MaximumCohortFixture,
) {
    assert_eq!(
        intent.cohort.map(|anchor| anchor.catalog_sha256),
        Some(fixture.garbage_catalog)
    );
    assert_eq!(intent.catalog_object_ids, vec![fixture.garbage_catalog]);
    assert_eq!(
        intent
            .catalog_set_record_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        fixture.catalog_set_record_ids
    );
    assert_eq!(
        intent
            .package_record_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        fixture.package_record_ids
    );
    assert_eq!(
        intent
            .tree_index_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        fixture.tree_index_ids
    );
    assert_eq!(
        intent
            .legal_artifact_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        fixture.legal_artifact_ids
    );
    assert_eq!(
        intent
            .tree_objects
            .iter()
            .map(|tree| tree.tree_sha256)
            .collect::<BTreeSet<_>>(),
        fixture.tree_object_ids
    );
    assert_eq!(
        intent.catalog_set_record_ids.len(),
        MAX_GC_CATALOG_SET_TARGETS
    );
    assert_eq!(
        intent.package_record_ids.len(),
        MAX_GC_PACKAGE_RECORD_TARGETS
    );
    assert_eq!(intent.tree_index_ids.len(), MAX_GC_DATA_OBJECT_TARGETS);
    assert_eq!(intent.legal_artifact_ids.len(), MAX_GC_DATA_OBJECT_TARGETS);
    assert_eq!(intent.tree_objects.len(), MAX_GC_TREE_JOBS);
    assert!(intent.retired_trees.is_empty());
}

fn assert_maximum_cohort_collected(
    repository: &mut ExtensionRepository,
    fixture: &MaximumCohortFixture,
) {
    assert_eq!(repository.writer_catalog_object_ids().len(), 1);
    assert!(repository
        .writer_catalog_object_ids()
        .contains(&fixture.active_catalog));
    assert!(!repository
        .writer_catalog_object_ids()
        .contains(&fixture.garbage_catalog));
    assert_eq!(
        repository
            .writer_catalog_high_water()
            .map(|row| row.catalog_sha256),
        Some(fixture.active_catalog)
    );
    let runtime = repository.writer_materialization().unwrap();
    assert!(runtime._catalog_sets.is_empty());
    assert!(runtime._package_records.is_empty());
    assert!(runtime._tree_index_ids.is_empty());
    assert!(runtime._legal_artifact_ids.is_empty());
    assert!(runtime._tree_object_ids.is_empty());
    assert!(runtime._retired_tree_ids.is_empty());
    assert!(runtime._state.completed_package_record_ids.is_empty());
    assert!(runtime._state.gc_intent.is_none());
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ComposedMaximumCohortOperations {
    durability_syncs: usize,
    regular_creates: usize,
    maximum_regular_unlinks: usize,
    maximum_directory_unlinks: usize,
    maximum_total_unlinks: usize,
    renames: usize,
    maximum_directory_mode_changes: usize,
}

fn compose_maximum_cohort_operations(
    filesystem: PrivateFsOperationSnapshot,
    work: GarbageCollectionWork,
) -> ComposedMaximumCohortOperations {
    let observed_tree_unlinks = work
        .tree_regular_files_removed
        .checked_add(work.tree_directories_removed)
        .unwrap();
    assert_eq!(observed_tree_unlinks, work.tree_entries + work.tree_jobs);
    let fixed_regular_unlinks = filesystem
        .regular_unlinks()
        .checked_sub(work.tree_regular_files_removed)
        .unwrap();
    let fixed_directory_unlinks = filesystem
        .directory_unlinks()
        .checked_sub(work.tree_directories_removed)
        .unwrap();
    let fixed_directory_mode_changes = filesystem
        .directory_mode_changes()
        .checked_sub(work.tree_directories_unsealed)
        .unwrap();
    // Every authenticated package tree has at least one regular file. Across
    // eight maximum-entry jobs this leaves at most 32,760 implicit directories
    // plus eight retired roots, while every entry/root still costs one unlink.
    let maximum_tree_directories_removed = MAX_GC_TREE_ENTRIES;
    ComposedMaximumCohortOperations {
        durability_syncs: filesystem.file_syncs() + filesystem.directory_syncs(),
        regular_creates: filesystem.regular_creates(),
        maximum_regular_unlinks: fixed_regular_unlinks + MAX_GC_TREE_ENTRIES,
        maximum_directory_unlinks: fixed_directory_unlinks + maximum_tree_directories_removed,
        maximum_total_unlinks: fixed_regular_unlinks
            + fixed_directory_unlinks
            + MAX_GC_TREE_DIRECTORY_NODES,
        renames: filesystem.renames(),
        maximum_directory_mode_changes: fixed_directory_mode_changes
            + maximum_tree_directories_removed,
    }
}

#[test]
fn no_garbage_is_exactly_zero_durable_writes() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let before = snapshot(&harness.repository_path);
    let catalog_measurement = crate::storage::CatalogValidationMeasurement::begin();
    let recovery_measurement =
        materialization::measurement::MaterializationRecoveryMeasurement::begin().unwrap();
    let filesystem_measurement = PrivateFsOperationMeasurement::begin().unwrap();
    let operation = repository
        .collect_bundled_package_garbage_measured()
        .unwrap();
    let filesystem = filesystem_measurement.finish().unwrap();
    let recovery = recovery_measurement.finish().unwrap();
    let catalogs = catalog_measurement.snapshot();
    assert_eq!(
        operation.outcome,
        BundledPackageGarbageCollectionOutcome::NoGarbage
    );
    assert_eq!(operation.work, GarbageCollectionWork::default());
    operation.work.validate(0).unwrap();
    assert_eq!(operation.work.durability_syncs(), Some(0));
    assert_eq!(catalogs.inventory_passes, 1);
    assert_eq!(catalogs.content_revalidated_catalogs, 0);
    assert_eq!(catalogs.content_revalidated_catalog_bytes, 0);
    assert_eq!(catalogs.fully_validated_catalogs, 0);
    assert_eq!(catalogs.fully_validated_catalog_bytes, 0);
    assert_eq!(
        recovery,
        materialization::measurement::MaterializationRecoverySnapshot::default()
    );
    assert_eq!(filesystem.file_syncs(), 0);
    assert_eq!(filesystem.directory_syncs(), 0);
    assert_eq!(filesystem.regular_creates(), 0);
    assert_eq!(filesystem.regular_unlinks(), 0);
    assert_eq!(filesystem.directory_unlinks(), 0);
    assert_eq!(filesystem.renames(), 0);
    assert_eq!(filesystem.directory_mode_changes(), 0);
    assert_eq!(filesystem.bytes_written(), 0);
    assert_eq!(snapshot(&harness.repository_path), before);
}

#[test]
fn same_inode_catalog_mutation_seals_before_any_garbage_mutation() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let residue = install_product_shaped_residue(&mut repository);
    let catalog_path = harness
        .repository_path
        .join("catalogs")
        .join(names::catalog_file(residue.active_catalog).as_str());
    let original_inode = fs::metadata(&catalog_path).unwrap().ino();
    let mut mutated = fixture::ACTIVE_CATALOG_BYTES.to_vec();
    let last = mutated
        .last_mut()
        .expect("the product catalog fixture is non-empty");
    *last ^= 1;
    let mut catalog = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&catalog_path)
        .unwrap();
    catalog.write_all(&mutated).unwrap();
    catalog.sync_all().unwrap();
    drop(catalog);
    assert_eq!(fs::metadata(&catalog_path).unwrap().ino(), original_inode);
    let before = snapshot(&harness.repository_path);

    assert_eq!(
        repository.collect_bundled_package_garbage(),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
    assert!(repository.writer_is_sealed());
    assert_eq!(snapshot(&harness.repository_path), before);
    assert!(harness
        .repository_path
        .join("materialization/trees")
        .join(materialization::gc_tree_object(residue.garbage_tree).as_str())
        .is_dir());
}

#[test]
fn product_shaped_residue_collects_with_bounded_revalidation_and_cached_admission() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let residue = install_product_shaped_residue(&mut repository);
    drop(repository);

    let mut repository = harness.open();
    assert_eq!(retained_closure(&mut repository), residue.retained);
    assert!(repository
        .writer_catalog_object_ids()
        .contains(&residue.active_catalog));
    assert!(repository
        .writer_catalog_object_ids()
        .contains(&residue.garbage_catalog));
    let catalog_measurement = crate::storage::CatalogValidationMeasurement::begin();
    let recovery_measurement =
        materialization::measurement::MaterializationRecoveryMeasurement::begin().unwrap();
    let filesystem_measurement = PrivateFsOperationMeasurement::begin().unwrap();
    let operation = repository
        .collect_bundled_package_garbage_measured()
        .unwrap();
    let filesystem = filesystem_measurement.finish().unwrap();
    let recovery = recovery_measurement.finish().unwrap();
    let catalogs = catalog_measurement.snapshot();
    assert_eq!(
        operation.outcome,
        BundledPackageGarbageCollectionOutcome::Collected {
            settled_targets: 2,
            more_garbage: false,
        }
    );
    assert_eq!(operation.work.state_transitions, 2);
    assert_eq!(operation.work.regular_targets_removed, 1);
    assert_eq!(operation.work.tree_jobs, 1);
    assert_eq!(operation.work.tree_object_retirements, 1);
    assert_eq!(operation.work.tree_entries, 1);
    assert_eq!(operation.work.tree_directory_syncs, 1);
    assert_eq!(operation.work.durability_syncs(), Some(30));
    assert!(operation.work.durability_syncs().unwrap() <= MAX_GC_FRESH_DURABILITY_SYNCS);
    assert_eq!(catalogs.inventory_passes, 3);
    assert_eq!(catalogs.content_revalidated_catalogs, 5);
    assert_eq!(
        catalogs.content_revalidated_catalog_bytes,
        3 * fixture::ACTIVE_CATALOG_BYTES.len() + 2 * fixture::ROLLBACK_CATALOG_BYTES.len()
    );
    assert_eq!(catalogs.fully_validated_catalogs, 0);
    assert_eq!(catalogs.fully_validated_catalog_bytes, 0);
    assert_eq!(recovery.completed_passes, 3);
    assert_eq!(recovery.catalog_reads, 3);
    assert_eq!(
        recovery.catalog_read_bytes,
        3 * fixture::ACTIVE_CATALOG_BYTES.len()
    );
    assert_eq!(recovery.catalog_admission_attempts, 0);
    assert_eq!(recovery.catalog_admission_bytes, 0);
    assert_eq!(recovery.catalog_cache_hits, 3);
    assert_eq!(filesystem.file_syncs(), 12);
    assert_eq!(filesystem.directory_syncs(), 18);
    assert_eq!(filesystem.file_syncs() + filesystem.directory_syncs(), 30);
    assert_eq!(filesystem.regular_creates(), 6);
    assert_eq!(filesystem.regular_unlinks(), 4);
    assert_eq!(filesystem.directory_unlinks(), 1);
    assert_eq!(filesystem.renames(), 7);
    assert_eq!(filesystem.directory_mode_changes(), 1);
    assert!(filesystem.bytes_written() <= MAX_GC_FRESH_CONTROL_BYTES_WRITTEN);
    assert_eq!(MAX_GC_FRESH_CONTROL_BYTES_WRITTEN, 2_129_920);
    assert!(repository
        .writer_catalog_object_ids()
        .contains(&residue.active_catalog));
    assert!(!repository
        .writer_catalog_object_ids()
        .contains(&residue.garbage_catalog));
    assert_eq!(retained_closure(&mut repository), residue.retained);
    assert!(!repository
        .writer_materialization()
        .unwrap()
        ._tree_object_ids
        .contains(&residue.garbage_tree));
    assert_eq!(
        repository.collect_bundled_package_garbage(),
        Ok(BundledPackageGarbageCollectionOutcome::NoGarbage)
    );
    drop(repository);
    let mut reopened = harness.open();
    assert!(reopened
        .writer_catalog_object_ids()
        .contains(&residue.active_catalog));
    assert_eq!(retained_closure(&mut reopened), residue.retained);
    assert_eq!(
        reopened.collect_bundled_package_garbage(),
        Ok(BundledPackageGarbageCollectionOutcome::NoGarbage)
    );
}

#[test]
fn pending_one_tree_settlement_stays_inside_the_startup_sync_ceiling() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let residue = install_product_shaped_residue(&mut repository);
    drop(repository);
    let mut repository = harness.open();
    assert_eq!(retained_closure(&mut repository), residue.retained);
    let intent = begin_one_batch(&mut repository);
    assert_eq!(
        intent.catalog_object_ids.len() + intent.tree_objects.len(),
        2
    );
    assert_eq!(intent.catalog_object_ids, vec![residue.garbage_catalog]);
    assert_eq!(intent.tree_objects[0].tree_sha256, residue.garbage_tree);

    let catalog_measurement = crate::storage::CatalogValidationMeasurement::begin();
    let recovery_measurement =
        materialization::measurement::MaterializationRecoveryMeasurement::begin().unwrap();
    let filesystem_measurement = PrivateFsOperationMeasurement::begin().unwrap();
    let settlement = repository.settle_pending_garbage_collection().unwrap();
    let filesystem = filesystem_measurement.finish().unwrap();
    let recovery = recovery_measurement.finish().unwrap();
    let catalogs = catalog_measurement.snapshot();
    assert_eq!(settlement.settled_targets, 2);
    assert_eq!(settlement.work.state_transitions, 1);
    assert_eq!(settlement.work.regular_targets_removed, 1);
    assert_eq!(settlement.work.tree_jobs, 1);
    assert_eq!(settlement.work.tree_object_retirements, 1);
    assert_eq!(settlement.work.tree_directory_syncs, 1);
    assert_eq!(settlement.work.durability_syncs(), Some(17));
    assert!(settlement.work.durability_syncs().unwrap() <= MAX_GC_PENDING_DURABILITY_SYNCS);
    assert_eq!(catalogs.inventory_passes, 3);
    assert_eq!(catalogs.content_revalidated_catalogs, 5);
    assert_eq!(
        catalogs.content_revalidated_catalog_bytes,
        3 * fixture::ACTIVE_CATALOG_BYTES.len() + 2 * fixture::ROLLBACK_CATALOG_BYTES.len()
    );
    assert_eq!(catalogs.fully_validated_catalogs, 0);
    assert_eq!(catalogs.fully_validated_catalog_bytes, 0);
    assert_eq!(recovery.completed_passes, 2);
    assert_eq!(recovery.catalog_reads, 2);
    assert_eq!(
        recovery.catalog_read_bytes,
        2 * fixture::ACTIVE_CATALOG_BYTES.len()
    );
    assert_eq!(recovery.catalog_admission_attempts, 0);
    assert_eq!(recovery.catalog_admission_bytes, 0);
    assert_eq!(recovery.catalog_cache_hits, 2);
    assert_eq!(filesystem.file_syncs(), 6);
    assert_eq!(filesystem.directory_syncs(), 11);
    assert_eq!(filesystem.file_syncs() + filesystem.directory_syncs(), 17);
    assert_eq!(filesystem.regular_creates(), 3);
    assert_eq!(filesystem.regular_unlinks(), 3);
    assert_eq!(filesystem.directory_unlinks(), 1);
    assert_eq!(filesystem.renames(), 4);
    assert_eq!(filesystem.directory_mode_changes(), 1);
    assert!(filesystem.bytes_written() <= MAX_GC_PENDING_CONTROL_BYTES_WRITTEN);
    assert_eq!(MAX_GC_PENDING_CONTROL_BYTES_WRITTEN, 1_064_960);
    assert!(repository
        .writer_catalog_object_ids()
        .contains(&residue.active_catalog));
    assert!(!repository
        .writer_catalog_object_ids()
        .contains(&residue.garbage_catalog));
    assert_eq!(retained_closure(&mut repository), residue.retained);
}

#[test]
fn real_maximum_cohort_has_constant_bounded_durability_and_operation_amplification() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let fixture = install_maximum_cohort(&mut repository);
    drop(repository);
    let mut repository = harness.open();
    assert_maximum_cohort_is_intact(&mut repository, &fixture);

    let filesystem_measurement = PrivateFsOperationMeasurement::begin().unwrap();
    let operation = repository
        .collect_bundled_package_garbage_measured()
        .unwrap();
    let filesystem = filesystem_measurement.finish().unwrap();
    assert_eq!(
        operation.outcome,
        BundledPackageGarbageCollectionOutcome::Collected {
            settled_targets: MAX_GC_LOGICAL_TARGETS,
            more_garbage: false,
        }
    );
    assert_eq!(operation.work.state_transitions, 2);
    assert_eq!(
        operation.work.regular_targets_removed,
        MAX_GC_REGULAR_TARGETS
    );
    assert_eq!(operation.work.tree_jobs, MAX_GC_TREE_JOBS);
    assert_eq!(operation.work.tree_object_retirements, MAX_GC_TREE_JOBS);
    assert_eq!(operation.work.tree_entries, fixture.actual_tree_entries);
    assert_eq!(
        operation.work.tree_regular_files_removed,
        fixture.actual_tree_entries
    );
    assert_eq!(operation.work.tree_directories_removed, MAX_GC_TREE_JOBS);
    assert_eq!(operation.work.tree_directories_unsealed, MAX_GC_TREE_JOBS);
    assert_eq!(operation.work.tree_directory_syncs, MAX_GC_TREE_JOBS);
    operation.work.validate(2).unwrap();
    assert_eq!(operation.work.durability_syncs(), Some(107));
    assert_eq!(filesystem.file_syncs(), 12);
    assert_eq!(filesystem.directory_syncs(), 95);
    assert_eq!(filesystem.regular_creates(), 6);
    assert_eq!(
        filesystem.regular_unlinks(),
        MAX_GC_REGULAR_TARGETS + fixture.actual_tree_entries + 2
    );
    assert_eq!(filesystem.directory_unlinks(), MAX_GC_TREE_JOBS);
    assert_eq!(filesystem.renames(), 14);
    assert_eq!(filesystem.directory_mode_changes(), MAX_GC_TREE_JOBS);
    assert!(filesystem.bytes_written() <= MAX_GC_FRESH_CONTROL_BYTES_WRITTEN);
    assert_eq!(
        compose_maximum_cohort_operations(filesystem, operation.work),
        ComposedMaximumCohortOperations {
            durability_syncs: 107,
            regular_creates: 6,
            maximum_regular_unlinks: 32_827,
            maximum_directory_unlinks: 32_768,
            maximum_total_unlinks: 32_835,
            renames: 14,
            maximum_directory_mode_changes: 32_768,
        }
    );
    assert_maximum_cohort_collected(&mut repository, &fixture);
    drop(repository);

    let mut reopened = harness.open();
    assert_maximum_cohort_collected(&mut reopened, &fixture);
    assert_eq!(
        reopened.collect_bundled_package_garbage(),
        Ok(BundledPackageGarbageCollectionOutcome::NoGarbage)
    );
}

#[test]
fn real_maximum_cohort_pending_intent_settles_inside_the_mutation_operation_ceiling() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let fixture = install_maximum_cohort(&mut repository);
    drop(repository);
    let mut repository = harness.open();
    assert_maximum_cohort_is_intact(&mut repository, &fixture);
    let intent = begin_one_batch(&mut repository);
    assert_maximum_intent(&intent, &fixture);

    // This gate deliberately starts after the repository is open so it
    // isolates physical settlement and control-write amplification at the
    // maximum cohort. `open_settles_an_existing_intent_without_planning_another_batch`
    // independently owns cold-open inventory, parsing, and read-amplification
    // assertions.
    let filesystem_measurement = PrivateFsOperationMeasurement::begin().unwrap();
    let settlement = repository.settle_pending_garbage_collection().unwrap();
    let filesystem = filesystem_measurement.finish().unwrap();
    assert_eq!(settlement.settled_targets, MAX_GC_LOGICAL_TARGETS);
    assert_eq!(settlement.work.state_transitions, 1);
    assert_eq!(
        settlement.work.regular_targets_removed,
        MAX_GC_REGULAR_TARGETS
    );
    assert_eq!(settlement.work.tree_jobs, MAX_GC_TREE_JOBS);
    assert_eq!(settlement.work.tree_object_retirements, MAX_GC_TREE_JOBS);
    assert_eq!(settlement.work.tree_entries, fixture.actual_tree_entries);
    assert_eq!(
        settlement.work.tree_regular_files_removed,
        fixture.actual_tree_entries
    );
    assert_eq!(settlement.work.tree_directories_removed, MAX_GC_TREE_JOBS);
    assert_eq!(settlement.work.tree_directories_unsealed, MAX_GC_TREE_JOBS);
    assert_eq!(settlement.work.tree_directory_syncs, MAX_GC_TREE_JOBS);
    settlement.work.validate(1).unwrap();
    assert_eq!(settlement.work.durability_syncs(), Some(94));
    assert_eq!(filesystem.file_syncs(), 6);
    assert_eq!(filesystem.directory_syncs(), 88);
    assert_eq!(filesystem.regular_creates(), 3);
    assert_eq!(
        filesystem.regular_unlinks(),
        MAX_GC_REGULAR_TARGETS + fixture.actual_tree_entries + 1
    );
    assert_eq!(filesystem.directory_unlinks(), MAX_GC_TREE_JOBS);
    assert_eq!(filesystem.renames(), 11);
    assert_eq!(filesystem.directory_mode_changes(), MAX_GC_TREE_JOBS);
    assert!(filesystem.bytes_written() <= MAX_GC_PENDING_CONTROL_BYTES_WRITTEN);
    assert_eq!(
        compose_maximum_cohort_operations(filesystem, settlement.work),
        ComposedMaximumCohortOperations {
            durability_syncs: 94,
            regular_creates: 3,
            maximum_regular_unlinks: 32_826,
            maximum_directory_unlinks: 32_768,
            maximum_total_unlinks: 32_834,
            renames: 11,
            maximum_directory_mode_changes: 32_768,
        }
    );
    assert_maximum_cohort_collected(&mut repository, &fixture);
    assert_eq!(
        repository.collect_bundled_package_garbage(),
        Ok(BundledPackageGarbageCollectionOutcome::NoGarbage)
    );
}

#[test]
fn authenticated_tree_bound_refusal_is_an_invalid_shape_not_a_retryable_io_error() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let name = materialization::gc_tree_object(Digest32::from_bytes([0xe2; 32]));
    let tree = repository
        .writer_materialization()
        .unwrap()
        ._trees
        .create_new_private_child(&name)
        .unwrap();
    for index in 0..2 {
        let entry = PrivateEntryName::new(format!("entry-{index}")).unwrap();
        tree.write_new_entry_synced(&entry, b"x", ByteLimit::new(1).unwrap())
            .unwrap();
    }
    let error =
        materialization::remove_tree_directory_bounded(OpenedPrivateDirectory::Writable(tree), 1)
            .unwrap_err();
    assert_eq!(error, TreeCleanupError::InvalidShape);
    assert!(harness
        .repository_path
        .join("materialization/trees")
        .join(name.as_str())
        .exists());
}

#[test]
fn open_settles_an_existing_intent_without_planning_another_batch() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let residue = install_product_shaped_residue(&mut repository);
    drop(repository);

    let mut repository = harness.open();
    assert_eq!(retained_closure(&mut repository), residue.retained);
    let intent = begin_one_batch(&mut repository);
    assert_eq!(
        intent.catalog_object_ids.len() + intent.tree_objects.len(),
        2
    );
    assert_eq!(intent.catalog_object_ids, vec![residue.garbage_catalog]);
    assert_eq!(intent.tree_objects[0].tree_sha256, residue.garbage_tree);
    drop(repository);

    let namespace = LockedPrivateNamespace::open_or_create(&harness.repository_path).unwrap();
    let catalog_measurement = crate::storage::CatalogValidationMeasurement::begin();
    let recovery_measurement =
        materialization::measurement::MaterializationRecoveryMeasurement::begin().unwrap();
    let filesystem_measurement = PrivateFsOperationMeasurement::begin().unwrap();
    let mut reopened = ExtensionRepository::open(namespace).unwrap();
    let filesystem = filesystem_measurement.finish().unwrap();
    let recovery = recovery_measurement.finish().unwrap();
    let catalogs = catalog_measurement.snapshot();
    assert!(reopened
        .writer_materialization()
        .unwrap()
        ._gc_intent
        .is_none());
    assert_eq!(catalogs.inventory_passes, 4);
    assert_eq!(catalogs.content_revalidated_catalogs, 5);
    assert_eq!(
        catalogs.content_revalidated_catalog_bytes,
        3 * fixture::ACTIVE_CATALOG_BYTES.len() + 2 * fixture::ROLLBACK_CATALOG_BYTES.len()
    );
    assert_eq!(catalogs.fully_validated_catalogs, 2);
    assert_eq!(
        catalogs.fully_validated_catalog_bytes,
        fixture::ACTIVE_CATALOG_BYTES.len() + fixture::ROLLBACK_CATALOG_BYTES.len()
    );
    assert_eq!(recovery.completed_passes, 3);
    assert_eq!(recovery.catalog_reads, 3);
    assert_eq!(
        recovery.catalog_read_bytes,
        3 * fixture::ACTIVE_CATALOG_BYTES.len()
    );
    assert_eq!(recovery.catalog_admission_attempts, 1);
    assert_eq!(
        recovery.catalog_admission_bytes,
        fixture::ACTIVE_CATALOG_BYTES.len()
    );
    assert_eq!(recovery.catalog_cache_hits, 2);
    assert_eq!(filesystem.file_syncs(), 6);
    assert_eq!(filesystem.directory_syncs(), 11);
    assert_eq!(filesystem.file_syncs() + filesystem.directory_syncs(), 17);
    assert_eq!(filesystem.regular_creates(), 3);
    assert_eq!(filesystem.regular_unlinks(), 3);
    assert_eq!(filesystem.directory_unlinks(), 1);
    assert_eq!(filesystem.renames(), 4);
    assert_eq!(filesystem.directory_mode_changes(), 1);
    assert!(reopened
        .writer_catalog_object_ids()
        .contains(&residue.active_catalog));
    assert!(!reopened
        .writer_catalog_object_ids()
        .contains(&residue.garbage_catalog));
    assert_eq!(retained_closure(&mut reopened), residue.retained);
    assert_eq!(
        reopened.collect_bundled_package_garbage(),
        Ok(BundledPackageGarbageCollectionOutcome::NoGarbage)
    );
}

#[test]
fn open_resumes_the_exact_object_to_retired_tree_crash_frontier() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let (_, tree) = install_residue(&mut repository);
    drop(repository);

    let mut repository = harness.open();
    let intent = begin_one_batch(&mut repository);
    let runtime = repository.writer_take_materialization().unwrap();
    let object = runtime
        ._trees
        .open_sealed_private_child(&materialization::gc_tree_object(tree))
        .unwrap();
    let retired_name = materialization::gc_tree_retired(tree, intent.generation).unwrap();
    let retired = object
        .publish_noreplace(&runtime._trees, &retired_name)
        .unwrap();
    drop(retired);
    drop(runtime);
    drop(repository);

    let mut reopened = harness.open();
    assert!(!reopened
        .writer_materialization()
        .unwrap()
        ._retired_tree_ids
        .iter()
        .any(|(digest, _)| *digest == tree));
    assert_eq!(
        reopened.collect_bundled_package_garbage(),
        Ok(BundledPackageGarbageCollectionOutcome::NoGarbage)
    );
}

#[test]
fn open_resumes_partial_flat_tree_residue_under_the_retired_root() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let (_, tree) = install_residue_with_entries(&mut repository, 2);
    drop(repository);
    let mut repository = harness.open();
    let intent = begin_one_batch(&mut repository);
    let runtime = repository.writer_take_materialization().unwrap();
    let object = runtime
        ._trees
        .open_sealed_private_child(&materialization::gc_tree_object(tree))
        .unwrap();
    let retired_name = materialization::gc_tree_retired(tree, intent.generation).unwrap();
    let retired = object
        .publish_noreplace(&runtime._trees, &retired_name)
        .unwrap();
    let retired = retired.unseal().unwrap();
    assert!(retired
        .remove_verified_entry_regular(&PrivateEntryName::new("payload-0000.js").unwrap())
        .unwrap());
    drop(retired);
    drop(runtime);
    drop(repository);

    let retired_path = harness
        .repository_path
        .join("materialization/trees")
        .join(retired_name.as_str());
    assert!(!retired_path.join("payload-0000.js").exists());
    assert!(retired_path.join("payload-0001.js").exists());

    let mut reopened = harness.open();
    assert!(!retired_path.exists());
    assert_eq!(
        reopened.collect_bundled_package_garbage(),
        Ok(BundledPackageGarbageCollectionOutcome::NoGarbage)
    );
}
