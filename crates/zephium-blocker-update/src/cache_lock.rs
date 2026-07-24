use std::fs::File;
use std::path::Path;

/// Exact operating-system lock held for the complete cache-store lifetime.
pub(crate) struct CacheLock {
    _file: File,
}

impl CacheLock {
    #[cfg(unix)]
    pub(crate) fn acquire(path: &Path) -> Option<Self> {
        use rustix::fs::{FlockOperation, Mode, OFlags};
        use std::os::unix::fs::MetadataExt;

        let before = match std::fs::symlink_metadata(path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return None,
        };
        if before.as_ref().is_some_and(|metadata| {
            !metadata.is_file() || metadata.file_type().is_symlink() || metadata.nlink() != 1
        }) {
            return None;
        }
        let descriptor = rustix::fs::open(
            path,
            OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::from_raw_mode(0o600),
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
            || before
                .as_ref()
                .is_some_and(|metadata| !same_unix_identity(metadata, &opened))
            || !same_unix_identity(&opened, &after)
        {
            return None;
        }
        rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive).ok()?;
        let locked_path = std::fs::symlink_metadata(path).ok()?;
        if !same_unix_identity(&opened, &locked_path) {
            return None;
        }
        if before.is_none() {
            file.sync_all().ok()?;
            File::open(path.parent()?).ok()?.sync_all().ok()?;
        }
        Some(Self { _file: file })
    }

    #[cfg(target_os = "windows")]
    #[allow(unsafe_code)]
    pub(crate) fn acquire(path: &Path) -> Option<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            LockFileEx, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_WRITE_THROUGH, FILE_SHARE_READ,
            FILE_SHARE_WRITE, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
        };
        use windows::Win32::System::IO::OVERLAPPED;

        let before = crate::file_identity::verified_identity_for_path(path);
        if before.is_none()
            && !matches!(
                std::fs::symlink_metadata(path),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            )
        {
            return None;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE).0)
            .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_WRITE_THROUGH).0)
            .open(path)
            .ok()?;
        let opened = crate::file_identity::verified_identity_for_file(&file)?;
        let mut overlapped = OVERLAPPED::default();
        // SAFETY: `file` owns a valid synchronous file handle, `overlapped`
        // remains alive for this non-overlapped immediate call, and the
        // one-byte range is held until the owning File is dropped.
        unsafe {
            LockFileEx(
                HANDLE(file.as_raw_handle()),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                None,
                1,
                0,
                &raw mut overlapped,
            )
        }
        .ok()?;
        let after = crate::file_identity::verified_identity_for_path(path)?;
        if before.as_ref().is_some_and(|identity| *identity != opened) || opened != after {
            return None;
        }
        file.sync_all().ok()?;
        Some(Self { _file: file })
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    pub(crate) fn acquire(_path: &Path) -> Option<Self> {
        None
    }
}

#[cfg(unix)]
fn same_unix_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino()
}
