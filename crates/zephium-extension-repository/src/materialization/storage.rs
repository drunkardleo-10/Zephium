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
    read_required_sealed_record_with(directory, name, maximum, |reader| {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

fn read_required_sealed_record_with(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
    read: impl FnOnce(&mut dyn std::io::Read) -> Result<Vec<u8>, std::io::Error>,
) -> Result<Vec<u8>, ExtensionRepositoryError> {
    let read = directory
        .with_bounded_sealed_regular_reader(name, ByteLimit::new(maximum)?, read)
        .map_err(map_recovery_fs)?
        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
    read.map_err(|_| ExtensionRepositoryError::FileSystem(PrivateFsError::Io))
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

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    use zephium_private_fs::LockedPrivateNamespace;

    use super::*;

    #[test]
    fn callback_read_failure_remains_retryable_after_filesystem_reproof() {
        #[cfg(target_os = "macos")]
        let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let temporary = tempfile::tempdir_in("/tmp").unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let namespace =
            LockedPrivateNamespace::open_or_create(temporary.path().join("repository")).unwrap();
        let name = PrivateComponent::new("sealed.record").unwrap();
        namespace
            .directory()
            .write_new_synced(&name, b"authenticated", ByteLimit::new(32).unwrap())
            .unwrap();
        namespace
            .directory()
            .seal_verified_regular(&name)
            .unwrap()
            .unwrap();

        assert_eq!(
            read_required_sealed_record_with(namespace.directory(), &name, 32, |_reader| {
                Err(std::io::Error::other("injected read failure"))
            }),
            Err(ExtensionRepositoryError::FileSystem(PrivateFsError::Io))
        );
        assert_eq!(
            read_required_sealed_record(namespace.directory(), &name, 32).unwrap(),
            b"authenticated"
        );
    }
}
