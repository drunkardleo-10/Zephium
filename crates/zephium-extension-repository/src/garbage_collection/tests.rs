use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use zephium_private_fs::{ByteLimit, LockedPrivateNamespace, PrivateEntryName};

use super::*;
use crate::repository_e2e_fixture as fixture;
use crate::state::Digest32;

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
    let catalog = crate::codec::digest(fixture::ACTIVE_CATALOG_BYTES);
    crate::storage::ensure_catalog_object(
        repository.writer_catalogs(),
        catalog,
        fixture::ACTIVE_CATALOG_BYTES,
    )
    .unwrap();

    let tree = Digest32::from_bytes([0xf1; 32]);
    let tree_name = materialization::gc_tree_object(tree);
    let directory = repository
        .writer_materialization()
        .unwrap()
        ._trees
        .create_new_private_child(&tree_name)
        .unwrap();
    let payload = PrivateEntryName::new("payload.js").unwrap();
    directory
        .write_new_entry_synced(&payload, b"fixture", ByteLimit::new(32).unwrap())
        .unwrap();
    assert!(directory
        .seal_verified_entry_regular(&payload)
        .unwrap()
        .is_some());
    let sealed = directory.seal().unwrap();
    drop(sealed);
    (catalog, tree)
}

fn begin_one_batch(repository: &mut ExtensionRepository) -> MaterializationGarbageCollectionIntent {
    repository.refresh_outer_authority_or_seal().unwrap();
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

#[test]
fn no_garbage_is_exactly_zero_durable_writes() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let before = snapshot(&harness.repository_path);
    assert_eq!(
        repository.collect_bundled_package_garbage(),
        Ok(BundledPackageGarbageCollectionOutcome::NoGarbage)
    );
    assert_eq!(snapshot(&harness.repository_path), before);
}

#[test]
fn residue_batch_deletes_tree_and_outer_catalog_then_reopens_idempotently() {
    let harness = Harness::new();
    let mut repository = harness.open();
    let (catalog, tree) = install_residue(&mut repository);
    drop(repository);

    let mut repository = harness.open();
    assert_eq!(
        repository.collect_bundled_package_garbage(),
        Ok(BundledPackageGarbageCollectionOutcome::Collected {
            settled_targets: 2,
            more_garbage: false,
        })
    );
    assert!(!repository.writer_catalog_object_ids().contains(&catalog));
    assert!(!repository
        .writer_materialization()
        .unwrap()
        ._tree_object_ids
        .contains(&tree));
    assert_eq!(
        repository.collect_bundled_package_garbage(),
        Ok(BundledPackageGarbageCollectionOutcome::NoGarbage)
    );
    drop(repository);
    let mut reopened = harness.open();
    assert_eq!(
        reopened.collect_bundled_package_garbage(),
        Ok(BundledPackageGarbageCollectionOutcome::NoGarbage)
    );
}

#[test]
fn open_settles_an_existing_intent_without_planning_another_batch() {
    let harness = Harness::new();
    let mut repository = harness.open();
    install_residue(&mut repository);
    drop(repository);

    let mut repository = harness.open();
    let intent = begin_one_batch(&mut repository);
    assert_eq!(
        intent.catalog_object_ids.len() + intent.tree_objects.len(),
        2
    );
    drop(repository);

    let mut reopened = harness.open();
    assert!(reopened
        .writer_materialization()
        .unwrap()
        ._gc_intent
        .is_none());
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
