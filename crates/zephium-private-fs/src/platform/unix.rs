use std::fs::{File, Metadata};
#[cfg(target_os = "macos")]
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use rustix::fs::{Mode, OFlags};

use crate::PrivateFsError;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RawIdentity {
    device: u64,
    inode: u64,
}

#[derive(Clone, Copy)]
pub(crate) enum OpenPurpose {
    Read,
    Mutation,
    Lock,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn admit_namespace_support() -> Result<(), PrivateFsError> {
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn admit_namespace_support() -> Result<(), PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn current_user() -> u32 {
    rustix::process::geteuid().as_raw()
}

pub(crate) fn raw_identity(metadata: &Metadata) -> RawIdentity {
    RawIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

pub(crate) fn open_regular(
    directory: &File,
    _directory_path: &Path,
    name: &str,
    purpose: OpenPurpose,
) -> Result<(File, RawIdentity), PrivateFsError> {
    let access = match purpose {
        OpenPurpose::Read | OpenPurpose::Mutation => OFlags::RDONLY,
        OpenPurpose::Lock => OFlags::RDWR,
    };
    let descriptor = rustix::fs::openat(
        directory,
        name,
        access | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(map_child_open_error)?;
    let file = File::from(descriptor);
    let opened = file
        .metadata()
        .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    validate_regular_metadata(&opened)?;
    let identity = raw_identity(&opened);
    if !acl_is_private(&file) {
        return Err(PrivateFsError::Unsafe);
    }
    if relative_regular_identity(directory, name)? != identity {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    Ok((file, identity))
}

pub(crate) fn create_new_regular(
    directory: &File,
    _directory_path: &Path,
    name: &str,
) -> Result<(File, RawIdentity), PrivateFsError> {
    let descriptor = rustix::fs::openat(
        directory,
        name,
        OFlags::RDWR
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::CLOEXEC
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|error| map_failed_create(error, directory, name, true))?;
    let file = File::from(descriptor);
    let admission = (|| {
        let opened = file.metadata().map_err(|_| PrivateFsError::Io)?;
        validate_regular_metadata(&opened)?;
        let identity = raw_identity(&opened);
        if !acl_is_private(&file) || relative_regular_identity(directory, name)? != identity {
            return Err(PrivateFsError::IdentityAmbiguous);
        }
        Ok(identity)
    })();
    match admission {
        Ok(identity) => Ok((file, identity)),
        Err(_) => Err(PrivateFsError::SettlementUnknown),
    }
}

pub(crate) fn revalidate_regular(
    directory: &File,
    _directory_path: &Path,
    name: &str,
    file: &File,
    expected: RawIdentity,
) -> Result<(), PrivateFsError> {
    let opened = file
        .metadata()
        .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    if validate_regular_metadata(&opened).is_err()
        || raw_identity(&opened) != expected
        || relative_regular_identity(directory, name)? != expected
        || !acl_is_private(file)
    {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    Ok(())
}

pub(crate) fn create_directory(
    parent: &File,
    _parent_path: &Path,
    name: &str,
) -> Result<bool, PrivateFsError> {
    let created = match rustix::fs::mkdirat(parent, name, Mode::from_raw_mode(0o700)) {
        Ok(()) => true,
        Err(rustix::io::Errno::EXIST) => false,
        Err(error) => return Err(map_failed_create(error, parent, name, false)),
    };
    Ok(created)
}

pub(crate) fn open_child_directory(
    parent: &File,
    _parent_path: &Path,
    name: &str,
) -> Result<(File, RawIdentity), PrivateFsError> {
    open_relative_directory(parent, name)
}

pub(crate) fn list_names(
    directory: &File,
    _directory_path: &Path,
    max_entries: usize,
) -> Result<Vec<String>, PrivateFsError> {
    let mut reader = rustix::fs::Dir::read_from(directory).map_err(|_| PrivateFsError::Io)?;
    let mut names = Vec::new();
    for entry in &mut reader {
        let entry = entry.map_err(|_| PrivateFsError::Io)?;
        let bytes = entry.file_name().to_bytes();
        if matches!(bytes, b"." | b"..") {
            continue;
        }
        if names.len() == max_entries {
            return Err(PrivateFsError::BoundExceeded);
        }
        names.push(
            std::str::from_utf8(bytes)
                .map_err(|_| PrivateFsError::Unsafe)?
                .to_owned(),
        );
    }
    Ok(names)
}

pub(crate) fn remove_regular(
    directory: &File,
    _directory_path: &Path,
    name: &str,
) -> Result<(), PrivateFsError> {
    rustix::fs::unlinkat(directory, name, rustix::fs::AtFlags::empty())
        .map_err(|_| PrivateFsError::Io)
}

pub(crate) fn open_directory(path: &Path) -> Result<(File, RawIdentity), PrivateFsError> {
    let before = std::fs::symlink_metadata(path).map_err(map_boundary_io)?;
    validate_private_directory_node(path, &before)?;
    let descriptor = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    let file = File::from(descriptor);
    let opened = file
        .metadata()
        .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    let after = std::fs::symlink_metadata(path).map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    validate_private_directory_node(path, &opened)?;
    validate_private_directory_node(path, &after)?;
    let identity = raw_identity(&opened);
    if !acl_is_private(&file) {
        return Err(PrivateFsError::Unsafe);
    }
    if raw_identity(&before) != identity || raw_identity(&after) != identity {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    Ok((file, identity))
}

pub(crate) fn same_open_identity(file: &File, expected: RawIdentity) -> bool {
    file.metadata()
        .ok()
        .is_some_and(|metadata| raw_identity(&metadata) == expected)
}

pub(crate) fn validate_regular_metadata(metadata: &Metadata) -> Result<(), PrivateFsError> {
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.nlink() != 1
        || metadata.uid() != current_user()
        || metadata.mode() & 0o077 != 0
    {
        return Err(PrivateFsError::Unsafe);
    }
    Ok(())
}

pub(crate) fn validate_private_directory_node(
    _path: &Path,
    metadata: &Metadata,
) -> Result<(), PrivateFsError> {
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != current_user()
        || metadata.mode() & 0o077 != 0
    {
        return Err(PrivateFsError::Unsafe);
    }
    Ok(())
}

pub(crate) fn validate_ancestor_node(
    _path: &Path,
    metadata: &Metadata,
) -> Result<(), PrivateFsError> {
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(PrivateFsError::Unsafe);
    }
    let owner = metadata.uid();
    let mode = metadata.mode();
    if owner != 0 && owner != current_user() {
        return Err(PrivateFsError::Unsafe);
    }
    if mode & 0o022 != 0 && (owner != 0 || mode & 0o1000 == 0) {
        return Err(PrivateFsError::Unsafe);
    }
    Ok(())
}

pub(crate) fn lock_exclusive(file: &File) -> bool {
    rustix::fs::flock(file, rustix::fs::FlockOperation::NonBlockingLockExclusive).is_ok()
}

pub(crate) fn atomic_replace(
    directory: &File,
    _directory_path: &Path,
    source: &str,
    destination: &str,
) -> Result<(), PrivateFsError> {
    rustix::fs::renameat(directory, source, directory, destination).map_err(|_| PrivateFsError::Io)
}

pub(crate) fn atomic_publish_noreplace(
    directory: &File,
    _directory_path: &Path,
    source: &str,
    destination: &str,
) -> Result<(), PrivateFsError> {
    use rustix::fs::{renameat_with, RenameFlags};

    renameat_with(
        directory,
        source,
        directory,
        destination,
        RenameFlags::NOREPLACE,
    )
    .map_err(|error| {
        if error == rustix::io::Errno::EXIST {
            PrivateFsError::AlreadyExists
        } else {
            PrivateFsError::Io
        }
    })
}

pub(crate) fn sync_directory(file: &File) -> Result<(), PrivateFsError> {
    file.sync_all().map_err(|_| PrivateFsError::Io)
}

pub(crate) fn sync_ancestor_directory(path: &Path) -> Result<(), PrivateFsError> {
    let before = std::fs::symlink_metadata(path).map_err(map_boundary_io)?;
    validate_ancestor_node(path, &before)?;
    let descriptor = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| PrivateFsError::Unsafe)?;
    let file = File::from(descriptor);
    let opened = file.metadata().map_err(|_| PrivateFsError::Io)?;
    let after = std::fs::symlink_metadata(path).map_err(map_boundary_io)?;
    validate_ancestor_node(path, &opened)?;
    validate_ancestor_node(path, &after)?;
    if raw_identity(&before) != raw_identity(&opened)
        || raw_identity(&opened) != raw_identity(&after)
    {
        return Err(PrivateFsError::Unsafe);
    }
    file.sync_all().map_err(|_| PrivateFsError::Io)
}

fn map_boundary_io(error: std::io::Error) -> PrivateFsError {
    if error.kind() == std::io::ErrorKind::NotFound {
        PrivateFsError::Unsafe
    } else {
        PrivateFsError::Io
    }
}

fn map_child_open_error(error: rustix::io::Errno) -> PrivateFsError {
    if error == rustix::io::Errno::NOENT {
        PrivateFsError::NotFound
    } else {
        PrivateFsError::Unsafe
    }
}

fn map_failed_create(
    error: rustix::io::Errno,
    parent: &File,
    name: &str,
    report_already_exists: bool,
) -> PrivateFsError {
    if report_already_exists && error == rustix::io::Errno::EXIST {
        PrivateFsError::AlreadyExists
    } else if relative_name_is_absent(parent, name) {
        // The held parent descriptor still reports no entry after the failed
        // creating syscall, so this operation has a proven clean outcome.
        PrivateFsError::Io
    } else {
        PrivateFsError::SettlementUnknown
    }
}

fn relative_name_is_absent(parent: &File, name: &str) -> bool {
    matches!(
        rustix::fs::statat(parent, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW),
        Err(rustix::io::Errno::NOENT)
    )
}

fn relative_regular_identity(directory: &File, name: &str) -> Result<RawIdentity, PrivateFsError> {
    let descriptor = rustix::fs::openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    let file = File::from(descriptor);
    let metadata = file
        .metadata()
        .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    validate_regular_metadata(&metadata).map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    if !acl_is_private(&file) {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    Ok(raw_identity(&metadata))
}

fn open_relative_directory(
    parent: &File,
    name: &str,
) -> Result<(File, RawIdentity), PrivateFsError> {
    let descriptor = rustix::fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(map_child_open_error)?;
    let file = File::from(descriptor);
    let metadata = file
        .metadata()
        .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    validate_private_directory_node(Path::new(name), &metadata)?;
    if !acl_is_private(&file) {
        return Err(PrivateFsError::Unsafe);
    }
    let second = rustix::fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    let second = File::from(second);
    let identity = raw_identity(&metadata);
    if !same_open_identity(&second, identity) || !acl_is_private(&second) {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    Ok((file, identity))
}

#[cfg(target_os = "macos")]
fn acl_is_private(file: &File) -> bool {
    super::macos_acl::has_no_extended_acl(file.as_raw_fd())
}

#[cfg(not(target_os = "macos"))]
fn acl_is_private(_file: &File) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_errors_are_clean_only_when_the_exact_name_is_absent() {
        let parent = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let handle = File::open(parent.path()).unwrap();
        assert_eq!(
            map_failed_create(rustix::io::Errno::IO, &handle, "missing", true),
            PrivateFsError::Io
        );
        std::fs::write(parent.path().join("present"), b"payload").unwrap();
        assert_eq!(
            map_failed_create(rustix::io::Errno::IO, &handle, "present", true),
            PrivateFsError::SettlementUnknown
        );
        assert_eq!(
            map_failed_create(rustix::io::Errno::EXIST, &handle, "present", true),
            PrivateFsError::AlreadyExists
        );
    }
}
