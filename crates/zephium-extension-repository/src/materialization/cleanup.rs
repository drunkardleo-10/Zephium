//! Exact reconciliation proof for one interrupted package build.
//!
//! Cleanup is intentionally narrower than garbage collection: it may remove
//! only stage names structurally owned by the current durable build intent.
//! Content-addressed finals and retired trees are inventoried before and after
//! reconciliation and must remain byte-for-byte name-identical.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;
use zephium_private_fs::{DirectoryIdentity, PrivateComponent, PrivateFsError};

use super::names::{self, RecordObjectKind, TreeNameKind};
use super::runtime::MaterializationRuntime;
use super::tree_writer::cleanup_tree_stage;
use crate::state::Digest32;

/// Stable, path-free failure while reconciling writer-owned stage residue.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum CleanupError {
    /// The live runtime and durable build intent do not describe one state.
    #[error("extension package cleanup state does not match its durable intent")]
    BuildStateMismatch,
    /// An observed name, role, digest, generation, or inventory was not the
    /// exact bounded shape owned by the current build.
    #[error("extension package cleanup inventory is not exact")]
    ExactMismatch,
    /// A private-filesystem operation failed before any consuming transition
    /// could have committed.
    #[error("extension package cleanup filesystem failed: {0}")]
    Filesystem(PrivateFsError),
    /// A removal or identity transition may have committed, or the namespace
    /// was already quarantined. The owning repository must seal itself.
    #[error("extension package cleanup settlement is ambiguous")]
    SettlementAmbiguous,
}

/// Linear proof that the current intent owns no physical writer stage.
///
/// The proof is deliberately non-`Clone`, non-serializable, and bound to the
/// exact materialization root, records directory, trees directory, intent
/// generation, and package-record identity. Final and retired objects are not
/// absence targets and remain untouched.
#[must_use = "stage absence must be consumed by abort or package completion"]
pub(crate) struct BuildStagesAbsent {
    root_identity: DirectoryIdentity,
    records_identity: DirectoryIdentity,
    trees_identity: DirectoryIdentity,
    intent_generation: u64,
    package_record_id: Digest32,
}

impl BuildStagesAbsent {
    /// Revalidates this proof against the live runtime immediately before a
    /// consuming state transition.
    pub(super) fn validate(&self, runtime: &MaterializationRuntime) -> Result<(), CleanupError> {
        let intent = IntentStageIdentity::from_runtime(runtime)?;
        if runtime._root.identity() != self.root_identity
            || runtime._records.identity() != self.records_identity
            || runtime._trees.identity() != self.trees_identity
            || intent.generation != self.intent_generation
            || intent.package_record_id != self.package_record_id
            || runtime._build_stage.is_some()
            || !runtime._record_stages.is_empty()
        {
            return Err(CleanupError::BuildStateMismatch);
        }

        let inventory = inspect_stage_inventory(runtime, intent)?;
        if inventory.tree.stage.is_some() || !inventory.records.stages.is_empty() {
            return Err(CleanupError::ExactMismatch);
        }
        Ok(())
    }
}

/// Proves that a matching durable build already has no physical stages.
///
/// This is used after exact CAS publication to bind completion to a freshly
/// observed zero-stage inventory. It never removes anything.
pub(super) fn prove_build_stages_absent(
    runtime: &MaterializationRuntime,
) -> Result<BuildStagesAbsent, CleanupError> {
    let intent = IntentStageIdentity::from_runtime(runtime)?;
    if runtime._build_stage.is_some() || !runtime._record_stages.is_empty() {
        return Err(CleanupError::BuildStateMismatch);
    }
    let inventory = inspect_stage_inventory(runtime, intent)?;
    if inventory.tree.stage.is_some() || !inventory.records.stages.is_empty() {
        return Err(CleanupError::ExactMismatch);
    }
    Ok(proof(runtime, intent))
}

/// Removes only exact stage residue owned by the current durable build intent.
///
/// The complete bounded inventories are parsed before any removal. A foreign
/// or malformed stage therefore fails closed without deleting it. Successful
/// cleanup rescans both directories, proves all retained final/retired names
/// unchanged and every stage absent, then clears the corresponding live
/// projections and returns an exact-runtime-bound linear proof.
pub(crate) fn reconcile_build_stages_for_abort(
    runtime: &mut MaterializationRuntime,
) -> Result<BuildStagesAbsent, CleanupError> {
    let intent = IntentStageIdentity::from_runtime(runtime)?;
    let before = inspect_stage_inventory(runtime, intent)?;
    validate_runtime_projection(runtime, &before)?;

    let mut changed = false;
    for (kind, (_, stage_name)) in &before.records.stages {
        match runtime._records.remove_verified_regular(stage_name) {
            Ok(true) => {
                changed = true;
                if runtime._record_stages.remove(kind).is_none() {
                    return Err(CleanupError::SettlementAmbiguous);
                }
            }
            Ok(false) => return Err(CleanupError::SettlementAmbiguous),
            Err(_error) if changed => return Err(CleanupError::SettlementAmbiguous),
            Err(error) => return Err(map_filesystem(error)),
        }
    }

    // A recovered tree-stage projection holds a live directory capability.
    // Drop it before reopening and consuming the exact stage by name.
    drop(runtime._build_stage.take());
    if let Some(stage_name) = &before.tree.stage {
        match cleanup_tree_stage(&runtime._trees, stage_name) {
            Ok(true) => {}
            Ok(false) | Err(_) => return Err(CleanupError::SettlementAmbiguous),
        }
    }

    let after = inspect_stage_inventory(runtime, intent)?;
    if after.tree.stage.is_some()
        || !after.records.stages.is_empty()
        || after.tree.retained != before.tree.retained
        || after.records.retained != before.records.retained
    {
        return Err(CleanupError::SettlementAmbiguous);
    }
    if runtime._root.identity() != before.root_identity
        || runtime._records.identity() != before.records_identity
        || runtime._trees.identity() != before.trees_identity
    {
        return Err(CleanupError::SettlementAmbiguous);
    }

    runtime._record_stages.clear();
    let proof = proof(runtime, intent);
    proof.validate(runtime)?;
    Ok(proof)
}

fn proof(runtime: &MaterializationRuntime, intent: IntentStageIdentity) -> BuildStagesAbsent {
    BuildStagesAbsent {
        root_identity: runtime._root.identity(),
        records_identity: runtime._records.identity(),
        trees_identity: runtime._trees.identity(),
        intent_generation: intent.generation,
        package_record_id: intent.package_record_id,
    }
}

#[derive(Clone, Copy)]
struct IntentStageIdentity {
    generation: u64,
    package_record_id: Digest32,
    tree_id: Digest32,
    tree_index_id: Digest32,
    legal_id: Digest32,
}

impl IntentStageIdentity {
    fn from_runtime(runtime: &MaterializationRuntime) -> Result<Self, CleanupError> {
        runtime
            ._state
            .validate()
            .map_err(|_| CleanupError::BuildStateMismatch)?;
        if runtime._state.build_intent != runtime._build_intent {
            return Err(CleanupError::BuildStateMismatch);
        }
        let intent = runtime
            ._build_intent
            .as_ref()
            .ok_or(CleanupError::BuildStateMismatch)?;
        Ok(Self {
            generation: intent.generation,
            package_record_id: intent.package_record_id,
            tree_id: intent.package_record.tree_index.tree_sha256,
            tree_index_id: intent.package_record.tree_index.index_sha256,
            legal_id: intent.package_record.legal.sha256,
        })
    }

    const fn expected_record_digest(self, kind: RecordObjectKind) -> Option<Digest32> {
        match kind {
            RecordObjectKind::Package => Some(self.package_record_id),
            RecordObjectKind::TreeIndex => Some(self.tree_index_id),
            RecordObjectKind::Legal => Some(self.legal_id),
            RecordObjectKind::CatalogSet => None,
        }
    }
}

struct StageInventory {
    root_identity: DirectoryIdentity,
    records_identity: DirectoryIdentity,
    trees_identity: DirectoryIdentity,
    tree: TreeStageInventory,
    records: RecordStageInventory,
}

#[derive(Default)]
struct TreeStageInventory {
    stage: Option<PrivateComponent>,
    retained: BTreeSet<PrivateComponent>,
}

#[derive(Default)]
struct RecordStageInventory {
    stages: BTreeMap<RecordObjectKind, (Digest32, PrivateComponent)>,
    retained: BTreeSet<PrivateComponent>,
}

fn inspect_stage_inventory(
    runtime: &MaterializationRuntime,
    intent: IntentStageIdentity,
) -> Result<StageInventory, CleanupError> {
    let root_identity = runtime._root.identity();
    let records_identity = runtime._records.identity();
    let trees_identity = runtime._trees.identity();
    let tree = inspect_tree_stages(runtime, intent)?;
    let records = inspect_record_stages(runtime, intent)?;
    if runtime._root.identity() != root_identity
        || runtime._records.identity() != records_identity
        || runtime._trees.identity() != trees_identity
    {
        return Err(CleanupError::SettlementAmbiguous);
    }
    Ok(StageInventory {
        root_identity,
        records_identity,
        trees_identity,
        tree,
        records,
    })
}

fn inspect_tree_stages(
    runtime: &MaterializationRuntime,
    intent: IntentStageIdentity,
) -> Result<TreeStageInventory, CleanupError> {
    let entries = runtime
        ._trees
        .list_components(names::MAX_TREE_ENTRIES)
        .map_err(map_filesystem)?;
    let mut inventory = TreeStageInventory::default();
    let mut observed_digests = BTreeSet::new();
    for entry in entries {
        let (digest, kind) =
            names::parse_tree_name(entry.as_str()).ok_or(CleanupError::ExactMismatch)?;
        if !observed_digests.insert(digest) {
            return Err(CleanupError::ExactMismatch);
        }
        match kind {
            TreeNameKind::Stage(generation)
                if generation == intent.generation && digest == intent.tree_id =>
            {
                if inventory.stage.replace(entry).is_some() {
                    return Err(CleanupError::ExactMismatch);
                }
            }
            TreeNameKind::Stage(_) => return Err(CleanupError::ExactMismatch),
            TreeNameKind::Object | TreeNameKind::Retired(_) => {
                inventory.retained.insert(entry);
            }
        }
    }
    Ok(inventory)
}

fn inspect_record_stages(
    runtime: &MaterializationRuntime,
    intent: IntentStageIdentity,
) -> Result<RecordStageInventory, CleanupError> {
    let entries = runtime
        ._records
        .list_components(names::MAX_RECORD_ENTRIES)
        .map_err(map_filesystem)?;
    let mut inventory = RecordStageInventory::default();
    let mut final_ids = BTreeSet::new();
    let mut stage_ids = BTreeSet::new();
    for entry in entries {
        let (digest, kind) =
            names::parse_record_name(entry.as_str()).ok_or(CleanupError::ExactMismatch)?;
        let object_kind = kind.object_kind();
        if kind.is_stage() {
            if intent.expected_record_digest(object_kind) != Some(digest)
                || !stage_ids.insert((object_kind, digest))
                || inventory
                    .stages
                    .insert(object_kind, (digest, entry))
                    .is_some()
            {
                return Err(CleanupError::ExactMismatch);
            }
        } else {
            if !final_ids.insert((object_kind, digest)) {
                return Err(CleanupError::ExactMismatch);
            }
            inventory.retained.insert(entry);
        }
    }
    if stage_ids.iter().any(|id| final_ids.contains(id)) {
        return Err(CleanupError::ExactMismatch);
    }
    Ok(inventory)
}

fn validate_runtime_projection(
    runtime: &MaterializationRuntime,
    inventory: &StageInventory,
) -> Result<(), CleanupError> {
    if runtime._root.identity() != inventory.root_identity
        || runtime._records.identity() != inventory.records_identity
        || runtime._trees.identity() != inventory.trees_identity
        || runtime._build_stage.is_some() != inventory.tree.stage.is_some()
        || runtime._record_stages != inventory.records.stages
    {
        return Err(CleanupError::BuildStateMismatch);
    }
    Ok(())
}

const fn map_filesystem(error: PrivateFsError) -> CleanupError {
    match error {
        PrivateFsError::IdentityAmbiguous
        | PrivateFsError::SettlementUnknown
        | PrivateFsError::Quarantined => CleanupError::SettlementAmbiguous,
        PrivateFsError::BoundExceeded | PrivateFsError::Unsafe => CleanupError::ExactMismatch,
        other => CleanupError::Filesystem(other),
    }
}

#[cfg(test)]
mod tests {
    use super::super::records::tests::package_record_fixture;
    use super::*;

    #[test]
    fn intent_stage_identity_excludes_catalog_set_stages() {
        let package = package_record_fixture(20);
        let identity = IntentStageIdentity {
            generation: 7,
            package_record_id: Digest32::from_bytes([90; 32]),
            tree_id: package.tree_index.tree_sha256,
            tree_index_id: package.tree_index.index_sha256,
            legal_id: package.legal.sha256,
        };

        assert_eq!(
            identity.expected_record_digest(RecordObjectKind::Package),
            Some(identity.package_record_id)
        );
        assert_eq!(
            identity.expected_record_digest(RecordObjectKind::TreeIndex),
            Some(package.tree_index.index_sha256)
        );
        assert_eq!(
            identity.expected_record_digest(RecordObjectKind::Legal),
            Some(package.legal.sha256)
        );
        assert_eq!(
            identity.expected_record_digest(RecordObjectKind::CatalogSet),
            None
        );
    }

    #[test]
    fn quarantined_and_uncertain_filesystems_are_always_terminal() {
        for error in [
            PrivateFsError::IdentityAmbiguous,
            PrivateFsError::SettlementUnknown,
            PrivateFsError::Quarantined,
        ] {
            assert_eq!(map_filesystem(error), CleanupError::SettlementAmbiguous);
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn exact_native_reconciliation_preserves_final_and_retired_objects() {
        use std::sync::Arc;

        use zephium_private_fs::{
            ByteLimit, LockedPrivateNamespace, PrivateEntryName, SealedPrivateDirectory,
        };

        use super::super::runtime::{MaterializationPinRoots, MaterializationTreeCapability};
        use super::super::state::{
            MaterializationBuildIntent, MaterializationState,
            MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION, MAX_MATERIALIZATION_STATE_BYTES,
        };
        use crate::codec;

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
        let namespace =
            LockedPrivateNamespace::open_or_create(temporary.path().join("repository")).unwrap();
        let root = namespace
            .directory()
            .create_new_private_child(&names::materialization_directory())
            .unwrap();
        let trees = root
            .create_new_private_child(&names::trees_directory())
            .unwrap();
        let records = root
            .create_new_private_child(&names::records_directory())
            .unwrap();
        let journals = root
            .create_new_private_child(&names::journals_directory())
            .unwrap();

        let package = package_record_fixture(40);
        let package_record_id = package.record_id().unwrap();
        let generation = 1;
        let intent = MaterializationBuildIntent {
            schema_version: MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION,
            generation,
            package_record_id,
            package_record: package.clone(),
        };
        let state = MaterializationState {
            generation,
            build_intent: Some(intent.clone()),
            ..MaterializationState::default()
        };
        state.validate().unwrap();

        let final_tree_id = Digest32::from_bytes([150; 32]);
        let final_tree_name = names::tree_object(final_tree_id);
        let final_tree = trees.create_new_private_child(&final_tree_name).unwrap();
        drop(final_tree.seal().unwrap());

        let retired_tree_id = Digest32::from_bytes([151; 32]);
        let retired_tree_name = names::tree_retired(retired_tree_id, generation).unwrap();
        drop(trees.create_new_private_child(&retired_tree_name).unwrap());

        let tree_stage_name =
            names::tree_stage(package.tree_index.tree_sha256, generation).unwrap();
        let tree_stage = trees.create_new_private_child(&tree_stage_name).unwrap();
        let payload = PrivateEntryName::new("partial.txt").unwrap();
        tree_stage
            .write_new_entry_synced(&payload, b"partial", ByteLimit::new(16).unwrap())
            .unwrap();

        let retained_record_id = Digest32::from_bytes([152; 32]);
        let retained_record_name = names::legal_object(retained_record_id);
        records
            .write_new_synced(
                &retained_record_name,
                b"retained",
                ByteLimit::new(16).unwrap(),
            )
            .unwrap();
        records
            .seal_verified_regular(&retained_record_name)
            .unwrap()
            .unwrap();

        let record_stages = [
            (
                RecordObjectKind::Package,
                package_record_id,
                names::package_record_stage(package_record_id),
            ),
            (
                RecordObjectKind::TreeIndex,
                package.tree_index.index_sha256,
                names::tree_index_stage(package.tree_index.index_sha256),
            ),
            (
                RecordObjectKind::Legal,
                package.legal.sha256,
                names::legal_stage(package.legal.sha256),
            ),
        ];
        let mut projected_stages = BTreeMap::new();
        for (kind, digest, name) in record_stages {
            records
                .write_new_synced(&name, b"stage", ByteLimit::new(16).unwrap())
                .unwrap();
            projected_stages.insert(kind, (digest, name));
        }

        let mut runtime = MaterializationRuntime {
            _root: root,
            _trees: trees,
            _records: records,
            _journals: journals,
            _state_bytes: codec::encode(&state, MAX_MATERIALIZATION_STATE_BYTES).unwrap(),
            _state: state,
            _package_records: BTreeMap::new(),
            _catalog_sets: BTreeMap::new(),
            _pin_roots: MaterializationPinRoots {
                _catalog_set_ids: BTreeSet::new(),
                _package_record_ids: BTreeSet::new(),
                _tree_ids: BTreeSet::new(),
            },
            _sealed_tree_roots: BTreeMap::<Digest32, Arc<SealedPrivateDirectory>>::new(),
            _build_intent: Some(intent),
            _build_stage: Some(MaterializationTreeCapability::Writable {
                _directory: tree_stage,
            }),
            _retired_tree_ids: BTreeSet::from([(retired_tree_id, generation)]),
            _record_stages: projected_stages,
        };

        let proof = reconcile_build_stages_for_abort(&mut runtime).unwrap();
        assert_eq!(proof.intent_generation, generation);
        assert_eq!(proof.package_record_id, package_record_id);
        proof.validate(&runtime).unwrap();
        assert!(runtime._build_stage.is_none());
        assert!(runtime._record_stages.is_empty());
        assert!(matches!(
            runtime._trees.open_private_child_any_mode(&tree_stage_name),
            Err(PrivateFsError::NotFound)
        ));
        assert!(runtime
            ._trees
            .open_sealed_private_child(&final_tree_name)
            .is_ok());
        assert!(runtime
            ._trees
            .open_private_child_any_mode(&retired_tree_name)
            .is_ok());
        assert!(runtime
            ._records
            .regular_exists(&retained_record_name)
            .unwrap());
        for (_, (_, name)) in projected_stage_names(&package, package_record_id) {
            assert!(!runtime._records.regular_exists(&name).unwrap());
        }
        let _fresh_proof = prove_build_stages_absent(&runtime).unwrap();

        drop(runtime);
        drop(namespace);
        make_fixture_tree_removable(temporary.path());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn projected_stage_names(
        package: &super::super::records::PackageRecord,
        package_record_id: Digest32,
    ) -> BTreeMap<RecordObjectKind, (Digest32, PrivateComponent)> {
        BTreeMap::from([
            (
                RecordObjectKind::Package,
                (
                    package_record_id,
                    names::package_record_stage(package_record_id),
                ),
            ),
            (
                RecordObjectKind::TreeIndex,
                (
                    package.tree_index.index_sha256,
                    names::tree_index_stage(package.tree_index.index_sha256),
                ),
            ),
            (
                RecordObjectKind::Legal,
                (
                    package.legal.sha256,
                    names::legal_stage(package.legal.sha256),
                ),
            ),
        ])
    }

    #[cfg(all(unix, any(target_os = "macos", target_os = "linux")))]
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
}
