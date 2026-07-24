use std::fs::File;
use std::path::Path;

#[cfg(unix)]
pub(super) fn open_verified_regular(path: &Path) -> Option<File> {
    use rustix::fs::{Mode, OFlags};
    use std::os::unix::fs::MetadataExt;

    let before = std::fs::symlink_metadata(path).ok()?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.nlink() != 1
        || before.uid() != rustix::process::geteuid().as_raw()
        || before.mode() & 0o077 != 0
    {
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
    if !opened.is_file()
        || opened.nlink() != 1
        || opened.uid() != rustix::process::geteuid().as_raw()
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

#[cfg(unix)]
pub(super) fn same_open_file_identity(left: &File, right: &File) -> bool {
    use std::os::unix::fs::MetadataExt;

    let (Ok(left), Ok(right)) = (left.metadata(), right.metadata()) else {
        return false;
    };
    same_unix_identity(&left, &right) && right.nlink() == 1
}

#[cfg(unix)]
pub(super) fn try_lock_exclusive(file: &File) -> bool {
    rustix::fs::flock(file, rustix::fs::FlockOperation::NonBlockingLockExclusive).is_ok()
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub(super) fn open_verified_regular(path: &Path) -> Option<File> {
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

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowsFileIdentity {
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
    // SAFETY: `file` owns a valid synchronous filesystem handle and the
    // correctly typed output lives for the complete call.
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
    // SAFETY: the buffer is correctly sized and aligned for `FileIdInfo` and
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
pub(super) fn same_open_file_identity(left: &File, right: &File) -> bool {
    let (Some(left), Some(right)) = (identity_for_file(left), identity_for_file(right)) else {
        return false;
    };
    left == right
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub(super) fn try_lock_exclusive(file: &File) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };
    use windows::Win32::System::IO::OVERLAPPED;

    let mut overlapped = OVERLAPPED::default();
    // SAFETY: the file handle remains owned for the cache lifetime. The
    // synchronous nonblocking call borrows a valid OVERLAPPED structure only
    // for this invocation; closing the file releases the lock.
    unsafe {
        LockFileEx(
            HANDLE(file.as_raw_handle()),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            None,
            u32::MAX,
            u32::MAX,
            &raw mut overlapped,
        )
        .is_ok()
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(super) fn open_verified_regular(_path: &Path) -> Option<File> {
    None
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(super) fn try_lock_exclusive(_file: &File) -> bool {
    false
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(super) fn same_open_file_identity(_left: &File, _right: &File) -> bool {
    false
}
