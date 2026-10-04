use std::fs::File;
use std::path::Path;

#[cfg(unix)]
pub(crate) fn open_verified_regular(path: &Path) -> Option<File> {
    use rustix::fs::{Mode, OFlags};
    use std::os::unix::fs::MetadataExt;

    let before = std::fs::symlink_metadata(path).ok()?;
    if !before.is_file() || before.file_type().is_symlink() || before.nlink() != 1 {
        return None;
    }
    let descriptor = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .ok()?;
    let file = File::from(descriptor);
    let opened = file.metadata().ok()?;
    let after = std::fs::symlink_metadata(path).ok()?;
    let current_user = rustix::process::geteuid().as_raw();
    if !opened.is_file()
        || opened.nlink() != 1
        || opened.uid() != current_user
        || opened.mode() & 0o077 != 0
        || !same_unix_identity(&before, &opened)
        || !same_unix_identity(&opened, &after)
    {
        return None;
    }
    Some(file)
}

#[cfg(unix)]
fn same_unix_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub(crate) fn open_verified_regular(path: &Path) -> Option<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let before = identity_for_path(path)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .ok()?;
    let opened = identity_for_file(&file)?;
    let after = identity_for_path(path)?;
    (before == opened && opened == after).then_some(file)
}

/// A writable handle is confined to the already-validated owned TUF stage.
/// General catalog/CAS readers retain their read-only access and sharing.
#[cfg(all(target_os = "windows", feature = "tuf"))]
pub(crate) fn open_verified_staged_for_sync(
    path: &Path,
    observed: &File,
    max_bytes: u64,
) -> Option<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
    let expected = identity_for_file(observed)?;
    if identity_for_path(path)? != expected {
        return None;
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        // Freeze mutation/replacement while synchronizing the same native object.
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if metadata.len() == 0
        || metadata.len() > max_bytes
        || identity_for_file(&file)? != expected
        || identity_for_path(path)? != expected
    {
        return None;
    }
    Some(file)
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowsFileIdentity {
    volume_serial_number: u64,
    file_id: [u8; 16],
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
fn identity_for_path(path: &Path) -> Option<WindowsFileIdentity> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .ok()?;
    identity_for_file(&file)
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
fn identity_for_file(file: &File) -> Option<WindowsFileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_ID_INFO,
    };

    let handle = HANDLE(file.as_raw_handle());
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the borrowed File owns a valid synchronous filesystem handle,
    // and the output structure lives for the complete call.
    unsafe {
        GetFileInformationByHandle(handle, &mut information).ok()?;
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || information.nNumberOfLinks != 1
    {
        return None;
    }
    let mut identity = FILE_ID_INFO::default();
    // SAFETY: the buffer is correctly sized and aligned for FileIdInfo and
    // remains alive for the complete synchronous call.
    unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&raw mut identity).cast(),
            u32::try_from(std::mem::size_of::<FILE_ID_INFO>()).ok()?,
        )
        .ok()?;
    }
    Some(WindowsFileIdentity {
        volume_serial_number: identity.VolumeSerialNumber,
        file_id: identity.FileId.Identifier,
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn verified_identity_for_path(path: &Path) -> Option<WindowsFileIdentity> {
    identity_for_path(path)
}

#[cfg(target_os = "windows")]
pub(crate) fn verified_identity_for_file(file: &File) -> Option<WindowsFileIdentity> {
    identity_for_file(file)
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) fn open_verified_regular(_path: &Path) -> Option<File> {
    None
}

#[cfg(all(test, target_os = "windows", feature = "tuf"))]
mod staged_sync_tests {
    #[test]
    fn synchronizer_refuses_a_replaced_stage_and_keeps_general_readers_read_only() {
        use std::io::Write;
        let directory = tempfile::tempdir().expect("test directory");
        let path = directory.path().join("root.json");
        std::fs::write(&path, b"first verified object").expect("write first object");
        let mut original = super::open_verified_regular(&path).expect("verified reader");
        assert!(original.write_all(b"must not write").is_err());
        let sync = super::open_verified_staged_for_sync(&path, &original, 1024)
            .expect("exact writable synchronization handle");
        sync.sync_all().expect("native file synchronization");
        drop(sync);
        std::fs::rename(&path, directory.path().join("previous.json"))
            .expect("replace stage identity");
        std::fs::write(&path, b"different object").expect("write replacement");
        assert!(super::open_verified_staged_for_sync(&path, &original, 1024).is_none());
    }
}
