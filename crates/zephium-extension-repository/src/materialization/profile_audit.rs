//! Fresh, bounded durable package-pin inspection for profile retirement.

use std::collections::BTreeSet;

use zephium_core::ids::ProfileId;
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, MAX_EXTENSION_LEGAL_NOTICE_BYTES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

use super::names;
use super::package_lease::PackageLeaseRepositoryIdentity;
use super::records::{
    CatalogSetRecord, PackageRecord, MAX_CATALOG_SET_RECORD_BYTES, MAX_PACKAGE_RECORD_BYTES,
};
use super::runtime::MaterializationRuntime;
use super::state::{
    MaterializationCheckpoint, MAX_DURABLE_PACKAGE_PINS, MAX_MATERIALIZATION_CHECKPOINT_BYTES,
    MAX_MATERIALIZATION_STATE_BYTES,
};
use super::storage::{read_required_control, read_required_sealed_record};
use super::transaction::OwnerPackagePinIdentity;
use crate::codec;
use crate::storage::map_recovery_fs;
use crate::ExtensionRepositoryError;

const _: () = assert!(MAX_DURABLE_PACKAGE_PINS <= u16::MAX as usize);

/// Worker-local result of one fresh durable control and pin-root audit.
pub(crate) struct DurableProfilePackageAudit {
    generation: u64,
    repository: PackageLeaseRepositoryIdentity,
    profile_pin_count: u16,
    durable_pins: BTreeSet<OwnerPackagePinIdentity>,
}

impl DurableProfilePackageAudit {
    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn repository(&self) -> PackageLeaseRepositoryIdentity {
        self.repository
    }

    pub(crate) const fn profile_pin_count(&self) -> u16 {
        self.profile_pin_count
    }

    pub(crate) fn has_pin(&self, pin: OwnerPackagePinIdentity) -> bool {
        self.durable_pins.contains(&pin)
    }
}

/// Revalidates the bounded durable control plane and audits one profile's pins.
pub(crate) fn audit_profile_package_pins(
    runtime: &MaterializationRuntime,
    profile: ProfileId,
) -> Result<DurableProfilePackageAudit, ExtensionRepositoryError> {
    validate_durable_controls(runtime)?;
    let durable_pins = validate_pin_projection(runtime)?;
    let profile_pin_count = runtime
        ._state
        .package_pins
        .iter()
        .filter(|pin| pin.profile_id == profile)
        .count();
    let profile_pin_count = u16::try_from(profile_pin_count)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    Ok(DurableProfilePackageAudit {
        generation: runtime._state.generation,
        repository: PackageLeaseRepositoryIdentity {
            root: runtime._root.identity(),
            records: runtime._records.identity(),
            trees: runtime._trees.identity(),
        },
        profile_pin_count,
        durable_pins,
    })
}

fn validate_durable_controls(
    runtime: &MaterializationRuntime,
) -> Result<(), ExtensionRepositoryError> {
    runtime._state.validate()?;
    if !runtime.intent_projection_is_exact() {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    let canonical = codec::encode(&runtime._state, MAX_MATERIALIZATION_STATE_BYTES)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    if canonical != runtime._state_bytes
        || read_required_control(
            &runtime._root,
            &names::state_file(),
            MAX_MATERIALIZATION_STATE_BYTES,
        )? != canonical
        || runtime
            ._root
            .regular_exists(&names::state_stage())
            .map_err(map_recovery_fs)?
        || runtime
            ._root
            .regular_exists(&names::checkpoint_stage())
            .map_err(map_recovery_fs)?
        || !runtime
            ._journals
            .list_components(names::MAX_MATERIALIZATION_JOURNAL_ENTRIES)
            .map_err(map_recovery_fs)?
            .is_empty()
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }

    let checkpoint_bytes = read_required_control(
        &runtime._root,
        &names::checkpoint_file(),
        MAX_MATERIALIZATION_CHECKPOINT_BYTES,
    )?;
    let checkpoint: MaterializationCheckpoint =
        codec::decode_materialization(&checkpoint_bytes, MAX_MATERIALIZATION_CHECKPOINT_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    checkpoint.validate()?;
    if checkpoint
        != MaterializationCheckpoint::new(runtime._state.generation, codec::digest(&canonical))
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

fn validate_pin_projection(
    runtime: &MaterializationRuntime,
) -> Result<BTreeSet<OwnerPackagePinIdentity>, ExtensionRepositoryError> {
    runtime._state.validate()?;
    let expected_catalog_sets = runtime
        ._state
        .catalog_pin_ids()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let expected_packages = runtime
        ._state
        .completed_package_record_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut expected_trees = BTreeSet::new();

    for package_id in &runtime._state.completed_package_record_ids {
        let package = authenticate_package_record(runtime, *package_id)?;
        authenticate_package_closure(runtime, &package)?;
        expected_trees.insert(package.tree_index.tree_sha256);
    }

    for catalog_set_id in &expected_catalog_sets {
        let set = authenticate_catalog_set_record(runtime, *catalog_set_id)?;
        for row in &set.packages {
            if runtime
                ._state
                .completed_package_record_ids
                .binary_search(&row.package_record_id)
                .is_err()
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            let package = runtime
                ._package_records
                .get(&row.package_record_id)
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            if package.catalog != set.catalog
                || package.package.package_key != row.package_key
                || package.manifest.runtime_target != row.runtime_target
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }
    }

    let mut durable_pins = BTreeSet::new();
    for pin in &runtime._state.package_pins {
        let set = runtime
            ._catalog_sets
            .get(&pin.catalog_set_record_id)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if !set
            .packages
            .iter()
            .any(|row| row.package_record_id == pin.package_record_id)
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if !durable_pins.insert((*pin).into()) {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }

    if runtime._pin_roots._catalog_set_ids != expected_catalog_sets
        || runtime._pin_roots._package_record_ids != expected_packages
        || runtime._pin_roots._tree_ids != expected_trees
        || runtime
            ._sealed_tree_roots
            .keys()
            .copied()
            .collect::<BTreeSet<_>>()
            != expected_trees
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(durable_pins)
}

fn authenticate_package_record(
    runtime: &MaterializationRuntime,
    package_id: crate::state::Digest32,
) -> Result<PackageRecord, ExtensionRepositoryError> {
    let bytes = read_required_sealed_record(
        &runtime._records,
        &names::package_record(package_id),
        MAX_PACKAGE_RECORD_BYTES,
    )?;
    let package = PackageRecord::decode(&bytes)?;
    if package.record_id()? != package_id
        || runtime._package_records.get(&package_id) != Some(&package)
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(package)
}

fn authenticate_catalog_set_record(
    runtime: &MaterializationRuntime,
    catalog_set_id: crate::state::Digest32,
) -> Result<CatalogSetRecord, ExtensionRepositoryError> {
    let bytes = read_required_sealed_record(
        &runtime._records,
        &names::catalog_set_record(catalog_set_id),
        MAX_CATALOG_SET_RECORD_BYTES,
    )?;
    let set = CatalogSetRecord::decode(&bytes)?;
    if set.record_id()? != catalog_set_id
        || runtime._catalog_sets.get(&catalog_set_id) != Some(&set)
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(set)
}

fn authenticate_package_closure(
    runtime: &MaterializationRuntime,
    package: &PackageRecord,
) -> Result<(), ExtensionRepositoryError> {
    if !runtime
        ._tree_index_ids
        .contains(&package.tree_index.index_sha256)
        || !runtime._legal_artifact_ids.contains(&package.legal.sha256)
        || !runtime
            ._tree_object_ids
            .contains(&package.tree_index.tree_sha256)
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }

    let index_bytes = read_required_sealed_record(
        &runtime._records,
        &names::tree_index_object(package.tree_index.index_sha256),
        MAX_EXTENSION_TREE_INDEX_BYTES,
    )?;
    let index = CanonicalExtensionTreeIndex::parse_canonical(&index_bytes)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    if index.index_sha256().bytes() != package.tree_index.index_sha256.bytes()
        || index.index_bytes() != package.tree_index.index_length
        || index.tree_sha256().bytes() != package.tree_index.tree_sha256.bytes()
        || index.files().len() != package.tree_index.file_count as usize
        || index.implicit_directory_count() != package.tree_index.directory_count as usize
        || index.total_entry_count() != package.tree_index.total_entry_count as usize
        || index.total_bytes() != package.tree_index.tree_bytes
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }

    let legal_bytes = read_required_sealed_record(
        &runtime._records,
        &names::legal_object(package.legal.sha256),
        MAX_EXTENSION_LEGAL_NOTICE_BYTES as usize,
    )?;
    if u64::try_from(legal_bytes.len()).ok() != Some(package.legal.length)
        || codec::digest(&legal_bytes) != package.legal.sha256
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }

    let held_root = runtime
        ._sealed_tree_roots
        .get(&package.tree_index.tree_sha256)
        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
    let reopened_root = runtime
        ._trees
        .open_sealed_private_child(&names::tree_object(package.tree_index.tree_sha256))
        .map_err(map_recovery_fs)?;
    if reopened_root.identity() != held_root.identity() {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    use zephium_core::ids::{ExtensionInstallId, ProfileId};
    use zephium_private_fs::LockedPrivateNamespace;

    use super::*;
    use crate::materialization::state::{
        DurablePackagePin, HistoricalCatalogRole, StoredBrowsingContext,
    };
    use crate::state::Digest32;
    use crate::ExtensionRepository;

    fn empty_repository() -> (tempfile::TempDir, ExtensionRepository) {
        #[cfg(target_os = "macos")]
        let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let temporary = tempfile::tempdir_in("/tmp").unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let namespace =
            LockedPrivateNamespace::open_or_create(temporary.path().join("repository")).unwrap();
        let repository = ExtensionRepository::open(namespace).unwrap();
        (temporary, repository)
    }

    #[test]
    fn pin_projection_rejects_an_owner_without_its_authenticated_set() {
        let (_temporary, mut repository) = empty_repository();
        let mut runtime = repository.writer_take_materialization().unwrap();
        let package = Digest32::from_bytes([1; 32]);
        runtime._state.generation = 1;
        runtime._state.completed_package_record_ids.push(package);
        runtime._state.package_pins.push(DurablePackagePin {
            profile_id: ProfileId::from(3),
            install_id: ExtensionInstallId::from(5),
            browsing_context: StoredBrowsingContext::Regular,
            catalog_set_record_id: Digest32::from_bytes([2; 32]),
            catalog_role: HistoricalCatalogRole::Active,
            package_record_id: package,
            native_incarnation: 1,
        });

        assert_eq!(
            validate_pin_projection(&runtime),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn pin_projection_rejects_orphan_recovered_roots() {
        let (_temporary, mut repository) = empty_repository();
        let mut runtime = repository.writer_take_materialization().unwrap();
        runtime
            ._pin_roots
            ._package_record_ids
            .insert(Digest32::from_bytes([7; 32]));

        assert_eq!(
            validate_pin_projection(&runtime),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }
}
