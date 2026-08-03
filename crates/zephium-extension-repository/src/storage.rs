//! Private-filesystem publication and bounded control-file primitives.

use zephium_extension_package::{ExtensionReleaseCatalog, MAX_EXTENSION_RELEASE_CATALOG_BYTES};
use zephium_private_fs::{ByteLimit, PrivateComponent, PrivateDirectory, PrivateFsError};

use crate::codec;
use crate::names::{
    self, catalog_file, catalog_stage, checkpoint_file, checkpoint_stage, journal_file,
    journal_stage,
};
use crate::state::{
    Digest32, RecoveryCheckpoint, TransitionJournal, CHECKPOINT_SCHEMA_VERSION,
    MAX_CHECKPOINT_BYTES, MAX_JOURNAL_BYTES,
};
use crate::ExtensionRepositoryError;

pub(crate) fn ensure_catalog_object(
    catalogs: &PrivateDirectory,
    digest: Digest32,
    bytes: &[u8],
) -> Result<(), ExtensionRepositoryError> {
    let destination = catalog_file(digest);
    if catalogs.regular_exists(&destination)? {
        let stored = read_required(catalogs, &destination, MAX_EXTENSION_RELEASE_CATALOG_BYTES)?;
        if stored != bytes {
            return Err(ExtensionRepositoryError::StateCorrupt);
        }
        return Ok(());
    }
    let entries = catalogs
        .list_components(names::MAX_CATALOG_OBJECT_ENTRIES)
        .map_err(map_recovery_fs)?;
    if entries.len() >= names::MAX_CATALOG_OBJECT_ENTRIES {
        return Err(ExtensionRepositoryError::CatalogObjectLimit);
    }
    let stage = catalog_stage(digest);
    if catalogs.regular_exists(&stage)? {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    match catalogs.write_new_synced(
        &stage,
        bytes,
        ByteLimit::new(MAX_EXTENSION_RELEASE_CATALOG_BYTES)?,
    ) {
        Ok(_) => {}
        Err(PrivateFsError::AlreadyExists) => {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous)
        }
        Err(error) => return Err(ExtensionRepositoryError::FileSystem(error)),
    }
    if catalogs
        .publish_noreplace_verified_regular(&stage, &destination)
        .is_err()
    {
        return Err(ExtensionRepositoryError::SettlementAmbiguous);
    }
    let stored = read_required(catalogs, &destination, MAX_EXTENSION_RELEASE_CATALOG_BYTES)
        .map_err(|_| ExtensionRepositoryError::SettlementAmbiguous)?;
    if stored != bytes {
        return Err(ExtensionRepositoryError::SettlementAmbiguous);
    }
    Ok(())
}

pub(crate) fn validate_named_catalog_object(
    catalogs: &PrivateDirectory,
    name: &PrivateComponent,
    expected_digest: Digest32,
) -> Result<(), ExtensionRepositoryError> {
    let bytes = read_required_recovery(catalogs, name, MAX_EXTENSION_RELEASE_CATALOG_BYTES)?;
    if codec::digest(&bytes) != expected_digest
        || ExtensionReleaseCatalog::parse_canonical(&bytes).is_err()
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

pub(crate) fn publish_journal(
    journals: &PrivateDirectory,
    journal: &TransitionJournal,
    bytes: &[u8],
) -> Result<PrivateComponent, ExtensionRepositoryError> {
    if !journals
        .list_components(names::MAX_JOURNAL_ENTRIES)
        .map_err(map_recovery_fs)?
        .is_empty()
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    let journal_digest = codec::digest(bytes);
    let stage = journal_stage(journal.generation, journal_digest);
    let destination = journal_file(journal.generation, journal_digest);
    match journals.write_new_synced(&stage, bytes, ByteLimit::new(MAX_JOURNAL_BYTES)?) {
        Ok(_) => {}
        Err(PrivateFsError::AlreadyExists) => {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous)
        }
        Err(error) => return Err(ExtensionRepositoryError::FileSystem(error)),
    }
    if journals
        .publish_noreplace_verified_regular(&stage, &destination)
        .is_err()
    {
        return Err(ExtensionRepositoryError::SettlementAmbiguous);
    }
    let stored = read_required(journals, &destination, MAX_JOURNAL_BYTES)
        .map_err(|_| ExtensionRepositoryError::SettlementAmbiguous)?;
    if stored != bytes {
        return Err(ExtensionRepositoryError::SettlementAmbiguous);
    }
    Ok(destination)
}

pub(crate) fn write_checkpoint(
    root: &PrivateDirectory,
    checkpoint: &RecoveryCheckpoint,
) -> Result<(), ExtensionRepositoryError> {
    if checkpoint.schema_version != CHECKPOINT_SCHEMA_VERSION {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    let bytes = codec::encode(checkpoint, MAX_CHECKPOINT_BYTES)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    atomic_write_control(
        root,
        &checkpoint_file(),
        &checkpoint_stage(),
        &bytes,
        MAX_CHECKPOINT_BYTES,
    )
}

pub(crate) fn atomic_write_control(
    directory: &PrivateDirectory,
    destination: &PrivateComponent,
    stage: &PrivateComponent,
    bytes: &[u8],
    maximum: usize,
) -> Result<(), ExtensionRepositoryError> {
    if directory.regular_exists(stage)? {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    directory.write_new_synced(stage, bytes, ByteLimit::new(maximum)?)?;
    if directory.regular_exists(destination)? {
        directory.replace_verified_regular(stage, destination)?;
    } else {
        directory.publish_noreplace_verified_regular(stage, destination)?;
    }
    let stored = read_required(directory, destination, maximum)?;
    if stored != bytes {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

fn read_optional(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Option<Vec<u8>>, ExtensionRepositoryError> {
    directory
        .read_bounded_regular(name, ByteLimit::new(maximum)?)
        .map_err(ExtensionRepositoryError::FileSystem)
}

pub(crate) fn read_optional_state(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Option<Vec<u8>>, ExtensionRepositoryError> {
    directory
        .read_bounded_regular(name, ByteLimit::new(maximum)?)
        .map_err(|error| match error {
            PrivateFsError::BoundExceeded => ExtensionRepositoryError::StateCorrupt,
            other => ExtensionRepositoryError::FileSystem(other),
        })
}

pub(crate) fn read_optional_recovery(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Option<Vec<u8>>, ExtensionRepositoryError> {
    directory
        .read_bounded_regular(name, ByteLimit::new(maximum)?)
        .map_err(map_recovery_fs)
}

pub(crate) fn read_required(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Vec<u8>, ExtensionRepositoryError> {
    read_optional(directory, name, maximum)?.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
}

pub(crate) fn read_required_state(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Vec<u8>, ExtensionRepositoryError> {
    read_optional_state(directory, name, maximum)?.ok_or(ExtensionRepositoryError::StateCorrupt)
}

pub(crate) fn read_required_recovery(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Vec<u8>, ExtensionRepositoryError> {
    read_optional_recovery(directory, name, maximum)?
        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
}

pub(crate) fn remove_required(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
) -> Result<(), ExtensionRepositoryError> {
    if !directory
        .remove_verified_regular(name)
        .map_err(map_recovery_fs)?
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

pub(crate) fn map_recovery_fs(error: PrivateFsError) -> ExtensionRepositoryError {
    match error {
        PrivateFsError::BoundExceeded => ExtensionRepositoryError::RecoveryAmbiguous,
        other => ExtensionRepositoryError::FileSystem(other),
    }
}
