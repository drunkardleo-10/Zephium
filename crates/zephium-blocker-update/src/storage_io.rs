//! Shared private-file and atomic-publication primitives for source updates.

use crate::types::FailureKind;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use thiserror::Error;

pub(crate) fn create_private_directory(path: &Path) -> Result<(), StoreError> {
    if !path.is_absolute() {
        return Err(StoreError::UnsafePath);
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            validate_directory_chain(path, true)
        }
        Ok(_) => Err(StoreError::UnsafePath),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or(StoreError::UnsafePath)?;
            validate_directory_chain(parent, false)?;
            let builder = private_directory_builder();
            builder.create(path).map_err(|_| StoreError::Io)?;
            sync_directory(parent)?;
            validate_directory_chain(path, true)
        }
        Err(_) => Err(StoreError::Io),
    }
}

#[cfg(feature = "tuf")]
pub(crate) fn create_child_directory(
    parent: &Path,
    name: &str,
) -> Result<std::path::PathBuf, StoreError> {
    validate_private_directory(parent)?;
    let path = parent.join(name);
    match fs::symlink_metadata(&path) {
        Ok(_) => validate_private_directory(&path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let builder = private_directory_builder();
            builder.create(&path).map_err(|_| StoreError::Io)?;
            sync_directory(parent)?;
            validate_private_directory(&path)?;
        }
        Err(_) => return Err(StoreError::Io),
    }
    Ok(path)
}

pub(crate) fn private_directory_builder() -> fs::DirBuilder {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
    }
    #[cfg(not(unix))]
    {
        fs::DirBuilder::new()
    }
}

pub(crate) fn validate_directory(path: &Path) -> Result<(), StoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| StoreError::Io)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(StoreError::UnsafePath);
    }
    Ok(())
}

pub(crate) fn validate_private_directory(path: &Path) -> Result<(), StoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| StoreError::Io)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(StoreError::UnsafePath);
    }
    validate_directory_security(path, &metadata, true)
}

#[cfg(unix)]
pub(crate) fn validate_directory_chain(path: &Path, private_leaf: bool) -> Result<(), StoreError> {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if current.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(&current).map_err(|_| StoreError::UnsafePath)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(StoreError::UnsafePath);
        }
        validate_directory_security(&current, &metadata, private_leaf && current == path)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn validate_directory_chain(path: &Path, private_leaf: bool) -> Result<(), StoreError> {
    if !path.is_absolute() {
        return Err(StoreError::UnsafePath);
    }
    let mut ancestors: Vec<&Path> = path.ancestors().collect();
    ancestors.reverse();
    for current in ancestors {
        let metadata = fs::symlink_metadata(current).map_err(|_| StoreError::UnsafePath)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(StoreError::UnsafePath);
        }
        validate_directory_security(current, &metadata, private_leaf && current == path)?;
    }
    Ok(())
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) fn validate_directory_chain(
    _path: &Path,
    _private_leaf: bool,
) -> Result<(), StoreError> {
    Err(StoreError::DurabilityPrimitiveUnavailable)
}

#[cfg(unix)]
pub(crate) fn validate_directory_security(
    _path: &Path,
    metadata: &fs::Metadata,
    private_leaf: bool,
) -> Result<(), StoreError> {
    use std::os::unix::fs::MetadataExt;

    let owner = metadata.uid();
    let current_user = rustix::process::geteuid().as_raw();
    if owner != 0 && owner != current_user {
        return Err(StoreError::UnsafePath);
    }
    let mode = metadata.mode();
    if private_leaf {
        if owner != current_user || mode & 0o077 != 0 {
            return Err(StoreError::UnsafePath);
        }
    } else if mode & 0o022 != 0 {
        // Root-owned sticky ancestors such as /tmp cannot redirect a
        // current-user-owned child entry and are the sole writable exception.
        if owner != 0 || mode & 0o1000 == 0 {
            return Err(StoreError::UnsafePath);
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn validate_directory_security(
    _path: &Path,
    metadata: &fs::Metadata,
    _private_leaf: bool,
) -> Result<(), StoreError> {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(StoreError::UnsafePath);
    }
    // The caller supplies the per-user application-data boundary. Every child
    // handle is subsequently opened with OPEN_REPARSE_POINT and exact file-ID
    // verification; ACL ownership of the OS-managed boundary is a packaged
    // installer test because std exposes no stable owner query.
    Ok(())
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) fn validate_directory_security(
    _path: &Path,
    _metadata: &fs::Metadata,
    _private_leaf: bool,
) -> Result<(), StoreError> {
    Err(StoreError::DurabilityPrimitiveUnavailable)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().ok_or(StoreError::UnsafePath)?;
    validate_private_directory(parent)?;
    let file_name = path.file_name().ok_or(StoreError::UnsafePath)?;
    let stage = parent.join(format!(".{}.stage", file_name.to_string_lossy()));
    remove_verified_regular_if_present(&stage)?;

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&stage).map_err(|_| StoreError::Io)?;
    file.write_all(bytes).map_err(|_| StoreError::Io)?;
    file.sync_all().map_err(|_| StoreError::Io)?;
    drop(file);
    atomic_replace(&stage, path)?;

    let persisted = read_bounded_regular(
        path,
        u64::try_from(bytes.len()).map_err(|_| StoreError::Io)?,
    )?;
    if persisted != bytes {
        return Err(StoreError::DurabilityPrimitiveUnavailable);
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn atomic_replace(source: &Path, destination: &Path) -> Result<(), StoreError> {
    fs::rename(source, destination).map_err(|_| StoreError::Io)?;
    sync_directory(destination.parent().ok_or(StoreError::UnsafePath)?)
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub(crate) fn atomic_replace(source: &Path, destination: &Path) -> Result<(), StoreError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both buffers are owned, NUL-terminated UTF-16 paths and remain
    // alive for the complete synchronous call. Source and destination are
    // constructed in the same already-validated directory, so WRITE_THROUGH
    // cannot degrade into a cross-volume copy.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|_| StoreError::DurabilityPrimitiveUnavailable)
}

pub(crate) fn sync_directory(path: &Path) -> Result<(), StoreError> {
    validate_directory(path)?;
    #[cfg(unix)]
    {
        fs::File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|_| StoreError::Io)?;
    }
    // Win32 MoveFileExW(WRITE_THROUGH) is the documented persistence barrier
    // for the replacement itself. Directory handles do not have a documented
    // FlushFileBuffers contract.
    Ok(())
}

pub(crate) fn remove_verified_regular(path: &Path) -> Result<(), StoreError> {
    let file = crate::file_identity::open_verified_regular(path).ok_or(StoreError::UnsafePath)?;
    drop(file);
    fs::remove_file(path).map_err(|_| StoreError::Io)
}

pub(crate) fn read_bounded_regular(path: &Path, max_bytes: u64) -> Result<Vec<u8>, StoreError> {
    let mut file = match crate::file_identity::open_verified_regular(path) {
        Some(file) => file,
        None => match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(StoreError::NotFound);
            }
            _ => return Err(StoreError::UnsafePath),
        },
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(StoreError::NotFound);
        }
        Err(_) => return Err(StoreError::Io),
    };
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(StoreError::UnsafePath);
    }
    let capacity = usize::try_from(metadata.len()).map_err(|_| StoreError::Io)?;
    let mut bytes = Vec::with_capacity(capacity);
    std::io::Read::by_ref(&mut file)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| StoreError::Io)?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > max_bytes {
        return Err(StoreError::UnsafePath);
    }
    Ok(bytes)
}

pub(crate) fn remove_verified_regular_if_present(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(_) => remove_verified_regular(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(StoreError::Io),
    }
}

/// Stable durable-store rejection.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum StoreError {
    /// Another process owns the exact updater-cache lock.
    #[error("updater cache is already locked by another process")]
    LockUnavailable,
    /// Durable state belongs to a different bootstrap root or repository origin.
    #[error("updater cache belongs to a different repository")]
    RepositoryMismatch,
    /// Required file or object is absent.
    #[error("required updater file is absent")]
    NotFound,
    /// Filesystem I/O failed.
    #[error("updater filesystem I/O failed")]
    Io,
    /// A path resolved to a symlink, non-file, non-directory, or changed object.
    #[error("unsafe updater filesystem path")]
    UnsafePath,
    /// The operating system could not provide the required durable replacement.
    #[error("required durable replacement primitive is unavailable")]
    DurabilityPrimitiveUnavailable,
    /// Canonical state is corrupt or internally inconsistent.
    #[error("updater state is corrupt")]
    StateCorrupt,
    /// Activation journal and durable state cannot be reconciled uniquely.
    #[error("updater activation journal is ambiguous")]
    JournalAmbiguous,
    /// A CAS object does not match its content address.
    #[error("updater content-addressed object is corrupt")]
    ObjectCorrupt,
    /// Cached package structure or compiler admission is invalid.
    #[error("cached package is invalid")]
    InvalidPackage,
    /// TUF client state changed after successful repository verification.
    #[error("verified staged TUF metadata changed before activation")]
    StagedMetadataChanged,
    /// A package revision is below the durable high-water mark.
    #[error("package revision rollback detected")]
    Rollback,
    /// A package reused a durable revision with different content.
    #[error("package revision equivocation detected")]
    Equivocation,
    /// A compiler commit did not name the exact durable candidate.
    #[error("prepared package does not match the durable candidate")]
    CandidateMismatch,
    /// A durable candidate must be resolved before another package is staged.
    #[error("another package candidate is already pending")]
    CandidatePending,
    /// This exact signed package was already rejected by compiler admission.
    #[error("package was previously rejected by compiler admission")]
    CandidateRejected,
    /// A prepared candidate expired before durable promotion.
    #[error("prepared package expired before activation")]
    CandidateExpired,
    /// The system clock moved behind the durable updater high-water mark.
    #[error("system clock moved behind the durable updater high-water mark")]
    ClockRollback,
    /// The system clock or durable high-water value is outside the admitted range.
    #[error("system clock is outside the updater's admitted range")]
    ClockInvalid,
    /// A post-journal outcome is uncertain and requires process recovery.
    #[error("package activation is sealed pending startup recovery")]
    ActivationSealed,
    /// Deterministic test-only simulated crash boundary.
    #[error("injected storage fault")]
    InjectedFault,
}

impl StoreError {
    pub(crate) const fn kind(self) -> FailureKind {
        match self {
            Self::Rollback
            | Self::Equivocation
            | Self::CandidateMismatch
            | Self::CandidatePending => FailureKind::Rollback,
            Self::CandidateRejected => FailureKind::Catalog,
            Self::CandidateExpired => FailureKind::Manifest,
            Self::ClockRollback | Self::ClockInvalid => FailureKind::Clock,
            _ => FailureKind::Storage,
        }
    }
}
