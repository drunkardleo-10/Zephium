//! Read-only repository preflight and exact journal recovery.

use std::collections::{BTreeMap, BTreeSet};

use zephium_private_fs::{
    FileIdentity, LockedPrivateNamespace, PrivateComponent, PrivateDirectory,
};

use crate::catalog_cache::ProductCatalogAdmissionCache;
use crate::codec;
use crate::materialization::{self, MaterializationRuntime};
use crate::names::{self, checkpoint_stage, state_file, state_stage};
use crate::state::{
    PackageLineHighWater, RecoveryCheckpoint, RepositoryState, TransitionJournal,
    MAX_CHECKPOINT_BYTES, MAX_JOURNAL_BYTES, MAX_STATE_BYTES,
};
use crate::storage::{
    atomic_write_control, map_recovery_fs, read_required_recovery, read_required_state,
    remove_required, validate_catalog_object_cache, validate_named_catalog_object,
    write_checkpoint, CatalogObjectIdentities, ValidatedCatalogObject,
};
use crate::ExtensionRepositoryError;

pub(crate) struct OpenedRepository {
    pub(crate) namespace: LockedPrivateNamespace,
    pub(crate) catalogs: PrivateDirectory,
    pub(crate) journals: PrivateDirectory,
    pub(crate) catalog_object_ids: BTreeSet<crate::state::Digest32>,
    pub(crate) catalog_object_identities: CatalogObjectIdentities,
    pub(crate) catalog_admission_cache: ProductCatalogAdmissionCache,
    pub(crate) state: RepositoryState,
    pub(crate) state_bytes: Vec<u8>,
    pub(crate) materialization: MaterializationRuntime,
}

pub(crate) fn open_repository(
    namespace: LockedPrivateNamespace,
) -> Result<OpenedRepository, ExtensionRepositoryError> {
    let root = namespace.directory();
    let mut root_shape = validate_root_shape(root)?;
    let catalogs = if root_shape.has_catalogs {
        root.open_private_child(&names::catalogs_directory())?
    } else {
        root.create_new_private_child(&names::catalogs_directory())?
    };
    // Validate an existing catalogs directory before completing a partial
    // catalogs-only initialization with a new journals directory.
    let catalog_inventory = inspect_catalogs(&catalogs)?;
    let journals = if root_shape.has_journals {
        root.open_private_child(&names::journals_directory())?
    } else {
        root.create_new_private_child(&names::journals_directory())?
    };
    let journal_inventory = inspect_journals(&journals)?;
    root_shape = initialize_controls(root, root_shape, &catalog_inventory, &journal_inventory)?;
    let (state, state_bytes) = read_state(root)?;
    let checkpoint = read_checkpoint(root)?;
    let disposition = assess_recovery(
        &catalog_inventory,
        &state,
        &state_bytes,
        checkpoint,
        journal_inventory.prepared.as_ref(),
    )?;
    inspect_control_stages(
        root,
        root_shape,
        checkpoint,
        journal_inventory.prepared.as_ref(),
        disposition,
    )?;
    cleanup_stages(
        root,
        &catalogs,
        &journals,
        catalog_inventory.stages,
        journal_inventory.stages,
        root_shape,
    )?;
    let (state, state_bytes) = apply_recovery(
        root,
        &journals,
        state,
        state_bytes,
        journal_inventory.prepared,
        disposition,
    )?;
    let mut catalog_admission_cache = ProductCatalogAdmissionCache::new();
    let catalog_recovery = materialization::ProductCatalogRecovery::new(
        &catalogs,
        &catalog_inventory.ids,
        &catalog_inventory.identities,
        &mut catalog_admission_cache,
        state.checkpoint(),
    );
    let materialization = materialization::open_or_recover(
        root,
        catalog_recovery,
        root_shape.has_materialization,
        materialization::FaultPoint::None,
    )?;
    Ok(OpenedRepository {
        namespace,
        catalogs,
        journals,
        catalog_object_ids: catalog_inventory.ids,
        catalog_object_identities: catalog_inventory.identities,
        catalog_admission_cache,
        state,
        state_bytes,
        materialization,
    })
}

#[derive(Clone, Copy)]
struct RootShape {
    has_catalogs: bool,
    has_journals: bool,
    has_state: bool,
    has_checkpoint: bool,
    has_state_stage: bool,
    has_checkpoint_stage: bool,
    has_materialization: bool,
}

fn validate_root_shape(root: &PrivateDirectory) -> Result<RootShape, ExtensionRepositoryError> {
    let entries = root
        .list_components(names::MAX_ROOT_ENTRIES)
        .map_err(map_recovery_fs)?;
    let mut shape = RootShape {
        has_catalogs: false,
        has_journals: false,
        has_state: false,
        has_checkpoint: false,
        has_state_stage: false,
        has_checkpoint_stage: false,
        has_materialization: false,
    };
    let mut has_control = false;
    for entry in entries {
        match entry.as_str() {
            "catalogs" => {
                root.open_private_child(&entry).map_err(map_recovery_fs)?;
                shape.has_catalogs = true;
            }
            "journals" => {
                root.open_private_child(&entry).map_err(map_recovery_fs)?;
                shape.has_journals = true;
            }
            "state.json" => {
                if !root.regular_exists(&entry).map_err(map_recovery_fs)? {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
                shape.has_state = true;
                has_control = true;
            }
            "recovery-checkpoint.json" => {
                if !root.regular_exists(&entry).map_err(map_recovery_fs)? {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
                shape.has_checkpoint = true;
                has_control = true;
            }
            "state.stage" => {
                if !root.regular_exists(&entry).map_err(map_recovery_fs)? {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
                shape.has_state_stage = true;
                has_control = true;
            }
            "recovery-checkpoint.stage" => {
                if !root.regular_exists(&entry).map_err(map_recovery_fs)? {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
                shape.has_checkpoint_stage = true;
                has_control = true;
            }
            "materialization" => {
                root.open_private_child(&entry).map_err(map_recovery_fs)?;
                shape.has_materialization = true;
            }
            _ => return Err(ExtensionRepositoryError::RecoveryAmbiguous),
        }
    }

    // Creation is ordered catalogs -> journals, before any control file. These
    // are the only partially initialized shapes which opening may complete.
    if shape.has_journals && !shape.has_catalogs {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if has_control && (!shape.has_catalogs || !shape.has_journals) {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if shape.has_materialization && (!shape.has_catalogs || !shape.has_journals) {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(shape)
}

struct CatalogInventory {
    stages: Vec<PrivateComponent>,
    ids: BTreeSet<crate::state::Digest32>,
    identities: CatalogObjectIdentities,
    state_projections: BTreeMap<crate::state::Digest32, CatalogStateProjection>,
}

struct CatalogStateProjection {
    authority_id: crate::state::Digest32,
    revision: u64,
    length: u64,
    digest: crate::state::Digest32,
    package_lines: Box<[PackageLineHighWater]>,
}

impl CatalogStateProjection {
    fn from_validated(
        validated: ValidatedCatalogObject,
    ) -> Result<(FileIdentity, Self), ExtensionRepositoryError> {
        let package_lines = validated
            .catalog
            .packages()
            .iter()
            .map(PackageLineHighWater::from_package)
            .collect::<Result<Vec<_>, _>>()?
            .into_boxed_slice();
        let projection = Self {
            authority_id: crate::state::Digest32::from_bytes(validated.catalog.authority().bytes()),
            revision: validated.catalog.revision().get(),
            length: validated.length,
            digest: crate::state::Digest32::from_bytes(validated.catalog.digest().bytes()),
            package_lines,
        };
        Ok((validated.identity, projection))
    }
}

fn inspect_catalogs(
    catalogs: &PrivateDirectory,
) -> Result<CatalogInventory, ExtensionRepositoryError> {
    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    crate::storage::note_catalog_inventory_pass();
    let entries = catalogs
        .list_components(names::MAX_CATALOG_OBJECT_ENTRIES)
        .map_err(map_recovery_fs)?;
    let mut ids = BTreeSet::new();
    let mut identities = CatalogObjectIdentities::new();
    let mut state_projections = BTreeMap::new();
    let mut stages = Vec::new();
    for entry in entries {
        let (digest, stage) = names::parse_catalog_file(entry.as_str())
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if stage {
            if !catalogs.regular_exists(&entry).map_err(map_recovery_fs)? {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            stages.push((entry, digest));
        } else {
            let (identity, projection) = CatalogStateProjection::from_validated(
                validate_named_catalog_object(catalogs, &entry, digest)?,
            )?;
            if !ids.insert(digest)
                || identities.insert(digest, identity).is_some()
                || state_projections.insert(digest, projection).is_some()
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }
    }
    if stages
        .iter()
        .any(|(_, stage_digest)| ids.contains(stage_digest))
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    validate_catalog_object_cache(&ids, &identities)?;
    Ok(CatalogInventory {
        stages: stages.into_iter().map(|(name, _)| name).collect(),
        ids,
        identities,
        state_projections,
    })
}

struct JournalInventory {
    stages: Vec<PrivateComponent>,
    prepared: Option<(PrivateComponent, TransitionJournal)>,
}

fn initialize_controls(
    root: &PrivateDirectory,
    mut shape: RootShape,
    catalogs: &CatalogInventory,
    journals: &JournalInventory,
) -> Result<RootShape, ExtensionRepositoryError> {
    if shape.has_state && shape.has_checkpoint {
        return Ok(shape);
    }
    let pristine_materialization =
        materialization::is_pristine_for_outer_initialization(root, shape.has_materialization)?;
    if !catalogs.ids.is_empty()
        || !catalogs.stages.is_empty()
        || !journals.stages.is_empty()
        || journals.prepared.is_some()
        || !pristine_materialization
    {
        // Missing outer controls next to any authority or writer-owned data
        // could represent deletion of the monotonic high-water. Never mint a
        // generation-zero state over that evidence.
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }

    let default_state = RepositoryState::default();
    let default_bytes = codec::encode(&default_state, MAX_STATE_BYTES)
        .map_err(|_| ExtensionRepositoryError::StateCorrupt)?;
    if !shape.has_state {
        if shape.has_checkpoint || shape.has_checkpoint_stage {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if shape.has_state_stage {
            remove_required(root, &state_stage())?;
            shape.has_state_stage = false;
        }
        atomic_write_control(
            root,
            &state_file(),
            &state_stage(),
            &default_bytes,
            MAX_STATE_BYTES,
        )?;
        shape.has_state = true;
    }

    if !shape.has_checkpoint {
        let observed = read_required_state(root, &state_file(), MAX_STATE_BYTES)?;
        if observed != default_bytes || shape.has_state_stage {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if shape.has_checkpoint_stage {
            remove_required(root, &checkpoint_stage())?;
            shape.has_checkpoint_stage = false;
        }
        write_checkpoint(
            root,
            &RecoveryCheckpoint::new(0, codec::digest(&default_bytes)),
        )?;
        shape.has_checkpoint = true;
    }
    Ok(shape)
}

fn inspect_journals(
    journals: &PrivateDirectory,
) -> Result<JournalInventory, ExtensionRepositoryError> {
    let entries = journals
        .list_components(names::MAX_JOURNAL_ENTRIES)
        .map_err(map_recovery_fs)?;
    let mut stages = Vec::new();
    let mut final_journal = None;
    for entry in entries {
        let (name_generation, name_digest, stage) = names::parse_journal_file(entry.as_str())
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if stage {
            if !journals.regular_exists(&entry).map_err(map_recovery_fs)? {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            stages.push((entry, name_generation, name_digest));
            continue;
        }
        if final_journal.is_some() {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let bytes = read_required_recovery(journals, &entry, MAX_JOURNAL_BYTES)?;
        let journal: TransitionJournal = codec::decode(&bytes, MAX_JOURNAL_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
        journal.validate()?;
        let encoded_next = codec::encode(&journal.next_state, MAX_STATE_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
        if journal.generation != name_generation
            || codec::digest(&bytes) != name_digest
            || codec::digest(&encoded_next) != journal.next_state_sha256
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        final_journal = Some((entry, journal));
    }
    // Publication begins only from an empty journals directory and atomically
    // consumes its one stage. A final journal plus any stage cannot be emitted
    // by this protocol, regardless of whether their names happen to match.
    if final_journal.is_some() && !stages.is_empty() {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(JournalInventory {
        stages: stages.into_iter().map(|(name, _, _)| name).collect(),
        prepared: final_journal,
    })
}

fn read_state(
    root: &PrivateDirectory,
) -> Result<(RepositoryState, Vec<u8>), ExtensionRepositoryError> {
    let name = state_file();
    let bytes = read_required_state(root, &name, MAX_STATE_BYTES)?;
    let state: RepositoryState = codec::decode(&bytes, MAX_STATE_BYTES)
        .map_err(|_| ExtensionRepositoryError::StateCorrupt)?;
    state.validate()?;
    Ok((state, bytes))
}

fn read_checkpoint(
    root: &PrivateDirectory,
) -> Result<RecoveryCheckpoint, ExtensionRepositoryError> {
    let name = names::checkpoint_file();
    let bytes = read_required_recovery(root, &name, MAX_CHECKPOINT_BYTES)?;
    let checkpoint: RecoveryCheckpoint = codec::decode(&bytes, MAX_CHECKPOINT_BYTES)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    checkpoint.validate()?;
    Ok(checkpoint)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryDisposition {
    Settled,
    ApplyPrepared,
    RetirePrepared,
    CheckpointPrepared,
}

fn assess_recovery(
    catalogs: &CatalogInventory,
    state: &RepositoryState,
    state_bytes: &[u8],
    checkpoint: RecoveryCheckpoint,
    journal: Option<&(PrivateComponent, TransitionJournal)>,
) -> Result<RecoveryDisposition, ExtensionRepositoryError> {
    validate_state_catalog(catalogs, state)?;
    let state_digest = codec::digest(state_bytes);
    let checkpoint_successor = checkpoint.generation.checked_add(1);
    if checkpoint.generation > state.generation
        || (state.generation != checkpoint.generation
            && Some(state.generation) != checkpoint_successor)
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if checkpoint.generation == state.generation && checkpoint.state_sha256 != state_digest {
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
                .is_some_and(|generation| prepared.generation == generation)
                && prepared.previous_state_sha256 == state_digest
            {
                let next_bytes = codec::encode(&prepared.next_state, MAX_STATE_BYTES)
                    .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
                if codec::digest(&next_bytes) != prepared.next_state_sha256 {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
                validate_state_catalog(catalogs, &prepared.next_state)?;
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

fn inspect_control_stages(
    _root: &PrivateDirectory,
    shape: RootShape,
    checkpoint: RecoveryCheckpoint,
    journal: Option<&(PrivateComponent, TransitionJournal)>,
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
    // A surviving create-new stage has not crossed its atomic publication
    // rename. Its payload is scratch and may be torn; the final journal and
    // committed controls, not scratch-byte equality, determine recovery.
    Ok(())
}

fn cleanup_stages(
    root: &PrivateDirectory,
    catalogs: &PrivateDirectory,
    journals: &PrivateDirectory,
    catalog_stages: Vec<PrivateComponent>,
    journal_stages: Vec<PrivateComponent>,
    shape: RootShape,
) -> Result<(), ExtensionRepositoryError> {
    // Every stage is create-new and every publication is one atomic rename.
    // Therefore a still-named stage is definitively pre-publication. Catalog
    // and journal stages precede any control-file mutation; state stages are
    // backed by the still-durable prepared journal; checkpoint stages follow
    // the exact durable state. Removing only preflight-validated stages cannot
    // discard the sole committed copy of any transition.
    for stage in catalog_stages {
        remove_required(catalogs, &stage)?;
    }
    for stage in journal_stages {
        remove_required(journals, &stage)?;
    }
    if shape.has_state_stage {
        remove_required(root, &state_stage())?;
    }
    if shape.has_checkpoint_stage {
        remove_required(root, &checkpoint_stage())?;
    }
    Ok(())
}

fn apply_recovery(
    root: &PrivateDirectory,
    journals: &PrivateDirectory,
    mut state: RepositoryState,
    mut state_bytes: Vec<u8>,
    journal: Option<(PrivateComponent, TransitionJournal)>,
    disposition: RecoveryDisposition,
) -> Result<(RepositoryState, Vec<u8>), ExtensionRepositoryError> {
    match disposition {
        RecoveryDisposition::Settled => {
            if journal.is_some() {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }
        RecoveryDisposition::ApplyPrepared => {
            let (name, prepared) = journal.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            let next_bytes = codec::encode(&prepared.next_state, MAX_STATE_BYTES)
                .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
            atomic_write_control(
                root,
                &state_file(),
                &state_stage(),
                &next_bytes,
                MAX_STATE_BYTES,
            )?;
            write_checkpoint(
                root,
                &RecoveryCheckpoint::new(prepared.generation, prepared.next_state_sha256),
            )?;
            remove_required(journals, &name)?;
            state = prepared.next_state;
            state_bytes = next_bytes;
        }
        RecoveryDisposition::RetirePrepared => {
            let (name, _) = journal.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            remove_required(journals, &name)?;
        }
        RecoveryDisposition::CheckpointPrepared => {
            let (name, prepared) = journal.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            write_checkpoint(
                root,
                &RecoveryCheckpoint::new(prepared.generation, prepared.next_state_sha256),
            )?;
            remove_required(journals, &name)?;
        }
    }
    Ok((state, state_bytes))
}

fn validate_state_catalog(
    catalogs: &CatalogInventory,
    state: &RepositoryState,
) -> Result<(), ExtensionRepositoryError> {
    state.validate()?;
    let Some(checkpoint) = state.checkpoint() else {
        return Ok(());
    };
    let catalog = catalogs
        .state_projections
        .get(&checkpoint.catalog_sha256)
        .ok_or(ExtensionRepositoryError::StateCorrupt)?;
    if catalog.authority_id != checkpoint.authority_id
        || catalog.revision != checkpoint.revision
        || catalog.length != checkpoint.catalog_length
        || catalog.digest != checkpoint.catalog_sha256
    {
        return Err(ExtensionRepositoryError::StateCorrupt);
    }
    for candidate in &catalog.package_lines {
        let durable = state
            .line(candidate.package_key)
            .ok_or(ExtensionRepositoryError::StateCorrupt)?;
        if durable.revision != candidate.revision
            || durable.package_row_sha256 != candidate.package_row_sha256
        {
            return Err(ExtensionRepositoryError::StateCorrupt);
        }
    }
    Ok(())
}
