//! Exact bounded metadata-file operations for materialization recovery.

use zephium_private_fs::{ByteLimit, PrivateComponent, PrivateDirectory, PrivateFsError};

use super::names::{checkpoint_file, checkpoint_stage};
use super::state::{MaterializationCheckpoint, MAX_MATERIALIZATION_CHECKPOINT_BYTES};
use crate::codec;
use crate::storage::{atomic_write_control, map_recovery_fs};
use crate::ExtensionRepositoryError;

pub(crate) fn read_optional_control(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Option<Vec<u8>>, ExtensionRepositoryError> {
    directory
        .read_bounded_regular(name, ByteLimit::new(maximum)?)
        .map_err(map_recovery_fs)
}

pub(crate) fn read_required_control(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Vec<u8>, ExtensionRepositoryError> {
    read_optional_control(directory, name, maximum)?
        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
}

pub(crate) fn read_required_sealed_record(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Vec<u8>, ExtensionRepositoryError> {
    let read = directory
        .with_bounded_sealed_regular_reader(name, ByteLimit::new(maximum)?, |reader| {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        })
        .map_err(map_recovery_fs)?
        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
    read.map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)
}

pub(crate) fn verify_required_sealed_record(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<(), ExtensionRepositoryError> {
    directory
        .with_bounded_sealed_regular_reader(name, ByteLimit::new(maximum)?, |_reader| {
            Ok::<_, std::convert::Infallible>(())
        })
        .map_err(map_recovery_fs)?
        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?
        .map_err(|never| match never {})
}

pub(crate) fn remove_required_control(
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

pub(crate) fn write_checkpoint(
    root: &PrivateDirectory,
    checkpoint: MaterializationCheckpoint,
) -> Result<(), ExtensionRepositoryError> {
    checkpoint.validate()?;
    let bytes = codec::encode(&checkpoint, MAX_MATERIALIZATION_CHECKPOINT_BYTES)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    atomic_write_control(
        root,
        &checkpoint_file(),
        &checkpoint_stage(),
        &bytes,
        MAX_MATERIALIZATION_CHECKPOINT_BYTES,
    )
}

pub(crate) fn map_initialization_fs(error: PrivateFsError) -> ExtensionRepositoryError {
    match error {
        PrivateFsError::AlreadyExists | PrivateFsError::BoundExceeded | PrivateFsError::Unsafe => {
            ExtensionRepositoryError::RecoveryAmbiguous
        }
        other => ExtensionRepositoryError::FileSystem(other),
    }
}
