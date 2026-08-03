//! Read-only repository preflight and exact journal recovery.

use zephium_extension_package::{ExtensionReleaseCatalog, MAX_EXTENSION_RELEASE_CATALOG_BYTES};
use zephium_private_fs::{LockedPrivateNamespace, PrivateComponent, PrivateDirectory};

use crate::codec;
use crate::names::{self, catalog_file, checkpoint_stage, state_file, state_stage};
use crate::state::{
    validate_catalog_lines, RecoveryCheckpoint, RepositoryState, TransitionJournal,
    MAX_CHECKPOINT_BYTES, MAX_JOURNAL_BYTES, MAX_STATE_BYTES,
};
use crate::storage::{
    atomic_write_control, map_recovery_fs, read_optional_recovery, read_optional_state,
    read_required_recovery, read_required_state, remove_required, validate_named_catalog_object,
    write_checkpoint,
};
use crate::ExtensionRepositoryError;

pub(crate) struct OpenedRepository {
    pub(crate) namespace: LockedPrivateNamespace,
    pub(crate) catalogs: PrivateDirectory,
    pub(crate) journals: PrivateDirectory,
    pub(crate) state: RepositoryState,
    pub(crate) state_bytes: Vec<u8>,
}

pub(crate) fn open_repository(
    namespace: LockedPrivateNamespace,
) -> Result<OpenedRepository, ExtensionRepositoryError> {
    let root = namespace.directory();
    let root_shape = validate_root_shape(root)?;
    let catalogs = if root_shape.has_catalogs {
        root.open_private_child(&names::catalogs_directory())?
    } else {
        root.create_new_private_child(&names::catalogs_directory())?
    };
    // Validate an existing catalogs directory before completing a partial
    // catalogs-only initialization with a new journals directory.
    let catalog_stages = inspect_catalogs(&catalogs)?;
    let journals = if root_shape.has_journals {
        root.open_private_child(&names::journals_directory())?
    } else {
        root.create_new_private_child(&names::journals_directory())?
    };
    let journal_inventory = inspect_journals(&journals)?;
    let (state, state_bytes) = read_state(root)?;
    let checkpoint = read_checkpoint(root, &RepositoryState::default())?;
    let disposition = assess_recovery(
        &catalogs,
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
        catalog_stages,
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
    Ok(OpenedRepository {
        namespace,
        catalogs,
        journals,
        state,
        state_bytes,
    })
}

#[derive(Clone, Copy)]
struct RootShape {
    has_catalogs: bool,
    has_journals: bool,
    has_state_stage: bool,
    has_checkpoint_stage: bool,
}

fn validate_root_shape(root: &PrivateDirectory) -> Result<RootShape, ExtensionRepositoryError> {
    let entries = root
        .list_components(names::MAX_ROOT_ENTRIES)
        .map_err(map_recovery_fs)?;
    let mut shape = RootShape {
        has_catalogs: false,
        has_journals: false,
        has_state_stage: false,
        has_checkpoint_stage: false,
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
            "state.json" | "recovery-checkpoint.json" => {
                if !root.regular_exists(&entry).map_err(map_recovery_fs)? {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
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
    Ok(shape)
}

fn inspect_catalogs(
    catalogs: &PrivateDirectory,
) -> Result<Vec<PrivateComponent>, ExtensionRepositoryError> {
    let entries = catalogs
        .list_components(names::MAX_CATALOG_OBJECT_ENTRIES)
        .map_err(map_recovery_fs)?;
    let mut finals = Vec::new();
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
            validate_named_catalog_object(catalogs, &entry, digest)?;
            finals.push(digest);
        }
    }
    if stages
        .iter()
        .any(|(_, stage_digest)| finals.contains(stage_digest))
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(stages.into_iter().map(|(name, _)| name).collect())
}

struct JournalInventory {
    stages: Vec<PrivateComponent>,
    prepared: Option<(PrivateComponent, TransitionJournal)>,
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
    let bytes = match read_optional_state(root, &name, MAX_STATE_BYTES)? {
        Some(bytes) => bytes,
        None => codec::encode(&RepositoryState::default(), MAX_STATE_BYTES)
            .map_err(|_| ExtensionRepositoryError::StateCorrupt)?,
    };
    let state: RepositoryState = codec::decode(&bytes, MAX_STATE_BYTES)
        .map_err(|_| ExtensionRepositoryError::StateCorrupt)?;
    state.validate()?;
    Ok((state, bytes))
}

fn read_checkpoint(
    root: &PrivateDirectory,
    default_state: &RepositoryState,
) -> Result<RecoveryCheckpoint, ExtensionRepositoryError> {
    let name = names::checkpoint_file();
    let checkpoint = match read_optional_recovery(root, &name, MAX_CHECKPOINT_BYTES)? {
        Some(bytes) => codec::decode(&bytes, MAX_CHECKPOINT_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?,
        None => {
            let default_bytes = codec::encode(default_state, MAX_STATE_BYTES)
                .map_err(|_| ExtensionRepositoryError::StateCorrupt)?;
            RecoveryCheckpoint::new(0, codec::digest(&default_bytes))
        }
    };
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
    catalogs: &PrivateDirectory,
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
    root: &PrivateDirectory,
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
        let (_, prepared) = journal.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        let expected = codec::encode(&prepared.next_state, MAX_STATE_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
        let observed = read_required_recovery(root, &state_stage(), MAX_STATE_BYTES)?;
        if observed != expected {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    if shape.has_checkpoint_stage {
        if disposition != RecoveryDisposition::CheckpointPrepared {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let (_, prepared) = journal.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        let expected_checkpoint =
            RecoveryCheckpoint::new(prepared.generation, prepared.next_state_sha256);
        let expected = codec::encode(&expected_checkpoint, MAX_CHECKPOINT_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
        let observed = read_required_recovery(root, &checkpoint_stage(), MAX_CHECKPOINT_BYTES)?;
        if observed != expected || checkpoint.generation >= prepared.generation {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
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
    catalogs: &PrivateDirectory,
    state: &RepositoryState,
) -> Result<(), ExtensionRepositoryError> {
    state.validate()?;
    let Some(checkpoint) = state.checkpoint() else {
        return Ok(());
    };
    let name = catalog_file(checkpoint.catalog_sha256);
    let bytes = read_required_state(catalogs, &name, MAX_EXTENSION_RELEASE_CATALOG_BYTES)?;
    if u64::try_from(bytes.len()).ok() != Some(checkpoint.catalog_length)
        || codec::digest(&bytes) != checkpoint.catalog_sha256
    {
        return Err(ExtensionRepositoryError::StateCorrupt);
    }
    let catalog = ExtensionReleaseCatalog::parse_canonical(&bytes)
        .map_err(|_| ExtensionRepositoryError::StateCorrupt)?;
    if catalog.authority().bytes() != checkpoint.authority_id.bytes()
        || catalog.revision().get() != checkpoint.revision
        || catalog.digest().bytes() != checkpoint.catalog_sha256.bytes()
    {
        return Err(ExtensionRepositoryError::StateCorrupt);
    }
    validate_catalog_lines(state, &catalog)
}
