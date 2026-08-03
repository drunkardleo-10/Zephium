//! Win32 identity, ACL, durable rename, and file-lock primitives.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::{File, Metadata};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;

use windows::Win32::Foundation::{
    GetLastError, LocalFree, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_SUCCESS, GENERIC_ALL,
    GENERIC_WRITE, HANDLE, HLOCAL,
};
use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows::Win32::Security::{
    EqualSid, GetAce, GetLengthSid, GetTokenInformation, IsValidSid, TokenUser, ACCESS_ALLOWED_ACE,
    ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    PSID, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, LockFileEx, MoveFileExW,
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_APPEND_DATA, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_WRITE_THROUGH, FILE_ID_INFO, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA,
    LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, MOVEFILE_REPLACE_EXISTING,
    MOVEFILE_WRITE_THROUGH, WRITE_DAC, WRITE_OWNER,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::System::IO::OVERLAPPED;

use super::{DirectoryMode, RegularMode};
use crate::PrivateFsError;

const FILE_ATTRIBUTE_REPARSE_POINT_RAW: u32 = 0x400;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RawIdentity {
    volume_serial_number: u64,
    file_id: [u8; 16],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// The common namespace layer must be able to describe successful inspection,
// while the current Windows backend deliberately refuses namespace admission
// before either successful variant can be constructed.
#[allow(dead_code)]
pub(crate) enum RawChildKind {
    Regular(RawIdentity),
    Directory(RawIdentity),
}

#[derive(Clone, Copy)]
pub(crate) enum OpenPurpose {
    Read,
    Mutation,
    Lock,
}

pub(crate) fn admit_namespace_support() -> Result<(), PrivateFsError> {
    // Win32 path APIs cannot provide descriptor-relative create, enumerate,
    // unlink, and rename as one coherent boundary. Do not expose the private
    // NT API implementation until it has a dedicated live-Windows proof.
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn open_regular(
    _directory: &File,
    directory_path: &Path,
    name: &str,
    purpose: OpenPurpose,
) -> Result<(File, RawIdentity), PrivateFsError> {
    let path = directory_path.join(name);
    let before = identity_for_path(&path, NodeKind::Regular)?;
    let mut options = std::fs::OpenOptions::new();
    let share_mode = match purpose {
        OpenPurpose::Read => FILE_SHARE_READ.0,
        OpenPurpose::Mutation => (FILE_SHARE_READ | FILE_SHARE_DELETE).0,
        // Other lockers must be able to open this node and lose at LockFileEx,
        // while replacement/deletion remains blocked for the lock lifetime.
        OpenPurpose::Lock => (FILE_SHARE_READ | FILE_SHARE_WRITE).0,
    };
    let extra_flags = if matches!(purpose, OpenPurpose::Lock) {
        FILE_FLAG_WRITE_THROUGH.0
    } else {
        0
    };
    options
        .read(true)
        .write(matches!(purpose, OpenPurpose::Lock))
        .share_mode(share_mode)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | extra_flags);
    let file = options
        .open(&path)
        .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    let opened = identity_for_file(&file, NodeKind::Regular)?;
    if !private_node_acl(&file) {
        return Err(PrivateFsError::Unsafe);
    }
    let after = identity_for_path(&path, NodeKind::Regular)
        .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    if before != opened || opened != after {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    Ok((file, opened))
}

pub(crate) fn open_sealed_regular(
    _directory: &File,
    _directory_path: &Path,
    _name: &str,
) -> Result<(File, RawIdentity), PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn create_new_regular(
    _directory: &File,
    directory_path: &Path,
    name: &str,
) -> Result<(File, RawIdentity), PrivateFsError> {
    let path = directory_path.join(name);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_WRITE_THROUGH).0)
        .open(&path)
        .map_err(|error| map_failed_create(error, &path, true))?;
    let admission = (|| {
        let identity = identity_for_file(&file, NodeKind::Regular)?;
        if !private_node_acl(&file) || identity_for_path(&path, NodeKind::Regular)? != identity {
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
    _directory: &File,
    directory_path: &Path,
    name: &str,
    file: &File,
    expected: RawIdentity,
) -> Result<(), PrivateFsError> {
    let path = directory_path.join(name);
    if identity_for_file(file, NodeKind::Regular)? != expected
        || identity_for_path(&path, NodeKind::Regular)? != expected
        || !private_node_acl(file)
    {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    Ok(())
}

pub(crate) fn revalidate_regular_mode(
    _directory: &File,
    _directory_path: &Path,
    _name: &str,
    _file: &File,
    _expected: RawIdentity,
    _mode: RegularMode,
) -> Result<(), PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn set_regular_mode(_file: &File, _mode: RegularMode) -> Result<(), PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn create_directory(
    _parent: &File,
    parent_path: &Path,
    name: &str,
) -> Result<bool, PrivateFsError> {
    let path = parent_path.join(name);
    let created = match std::fs::create_dir(&path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(map_failed_create(error, &path, false)),
    };
    Ok(created)
}

pub(crate) fn open_child_directory(
    _parent: &File,
    parent_path: &Path,
    name: &str,
) -> Result<(File, RawIdentity), PrivateFsError> {
    open_directory(&parent_path.join(name))
}

pub(crate) fn open_sealed_child_directory(
    _parent: &File,
    _parent_path: &Path,
    _name: &str,
) -> Result<(File, RawIdentity), PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn revalidate_child_directory(
    _parent: &File,
    _parent_path: &Path,
    _name: &str,
    _directory: &File,
    _expected: RawIdentity,
    _mode: DirectoryMode,
) -> Result<(), PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn set_directory_mode(_file: &File, _mode: DirectoryMode) -> Result<(), PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn inspect_child(
    _parent: &File,
    _parent_path: &Path,
    _name: &str,
) -> Result<Option<RawChildKind>, PrivateFsError> {
    // Keep the new entry-inspection surface behind the same unavailable NT
    // descriptor-relative adapter gate as namespace activation.
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn verify_exact_name(_file: &File, _expected: &str) -> Result<(), PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn list_names(
    _directory: &File,
    directory_path: &Path,
    max_entries: usize,
) -> Result<Vec<String>, PrivateFsError> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(directory_path).map_err(|_| PrivateFsError::Io)? {
        if names.len() == max_entries {
            return Err(PrivateFsError::BoundExceeded);
        }
        let entry = entry.map_err(|_| PrivateFsError::Io)?;
        names.push(
            entry
                .file_name()
                .into_string()
                .map_err(|_| PrivateFsError::Unsafe)?,
        );
    }
    Ok(names)
}

pub(crate) fn directory_is_empty(_directory: &File) -> Result<bool, PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn remove_regular(
    _directory: &File,
    directory_path: &Path,
    name: &str,
) -> Result<(), PrivateFsError> {
    std::fs::remove_file(directory_path.join(name)).map_err(|_| PrivateFsError::Io)
}

pub(crate) fn remove_directory(
    _parent: &File,
    _parent_path: &Path,
    _name: &str,
) -> Result<(), PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn open_directory(path: &Path) -> Result<(File, RawIdentity), PrivateFsError> {
    let before = identity_for_path(path, NodeKind::Directory)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0)
        .open(path)
        .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    let opened = identity_for_file(&file, NodeKind::Directory)?;
    if !private_node_acl(&file) {
        return Err(PrivateFsError::Unsafe);
    }
    let after = identity_for_path(path, NodeKind::Directory)
        .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    if before != opened || opened != after {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    Ok((file, opened))
}

pub(crate) fn open_directory_with_mode(
    _path: &Path,
    _mode: DirectoryMode,
) -> Result<(File, RawIdentity), PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn same_open_identity(file: &File, expected: RawIdentity) -> bool {
    identity_for_file(file, NodeKind::Directory)
        .or_else(|_| identity_for_file(file, NodeKind::Regular))
        .is_ok_and(|identity| identity == expected)
}

pub(crate) fn validate_private_directory_node(
    _path: &Path,
    metadata: &Metadata,
) -> Result<(), PrivateFsError> {
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT_RAW != 0 {
        return Err(PrivateFsError::Unsafe);
    }
    Ok(())
}

pub(crate) fn validate_ancestor_node(
    _path: &Path,
    metadata: &Metadata,
) -> Result<(), PrivateFsError> {
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT_RAW != 0 {
        return Err(PrivateFsError::Unsafe);
    }
    Ok(())
}

pub(crate) fn lock_exclusive(file: &File) -> bool {
    let mut overlapped = OVERLAPPED::default();
    // SAFETY: `file` owns a valid synchronous handle; the nonblocking call
    // borrows the live OVERLAPPED only for this invocation. Closing the file
    // releases the full-range lock.
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

pub(crate) fn atomic_replace(
    _directory: &File,
    directory_path: &Path,
    source: &str,
    destination: &str,
) -> Result<(), PrivateFsError> {
    move_file(
        &directory_path.join(source),
        &directory_path.join(destination),
        MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
    )
}

pub(crate) fn atomic_publish_noreplace(
    _directory: &File,
    directory_path: &Path,
    source: &str,
    destination: &str,
) -> Result<(), PrivateFsError> {
    match move_file(
        &directory_path.join(source),
        &directory_path.join(destination),
        MOVEFILE_WRITE_THROUGH,
    ) {
        Ok(()) => Ok(()),
        Err(PrivateFsError::AlreadyExists) => Err(PrivateFsError::AlreadyExists),
        Err(_) => Err(PrivateFsError::Io),
    }
}

pub(crate) fn atomic_publish_noreplace_between(
    _source_directory: &File,
    _source_directory_path: &Path,
    _source: &str,
    _destination_directory: &File,
    _destination_directory_path: &Path,
    _destination: &str,
) -> Result<(), PrivateFsError> {
    Err(PrivateFsError::PrimitiveUnavailable)
}

pub(crate) fn sync_directory(_file: &File) -> Result<(), PrivateFsError> {
    // Payload handles are flushed and replacement uses WRITE_THROUGH. Win32
    // does not document FlushFileBuffers as a directory durability primitive.
    Ok(())
}

pub(crate) fn sync_ancestor_directory(_path: &Path) -> Result<(), PrivateFsError> {
    Ok(())
}

#[derive(Clone, Copy)]
enum NodeKind {
    Regular,
    Directory,
}

fn identity_for_path(path: &Path, kind: NodeKind) -> Result<RawIdentity, PrivateFsError> {
    let extra_flags = if matches!(kind, NodeKind::Directory) {
        FILE_FLAG_BACKUP_SEMANTICS.0
    } else {
        0
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | extra_flags)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                PrivateFsError::NotFound
            } else {
                PrivateFsError::Unsafe
            }
        })?;
    identity_for_file(&file, kind)
}

fn identity_for_file(file: &File, kind: NodeKind) -> Result<RawIdentity, PrivateFsError> {
    let handle = HANDLE(file.as_raw_handle());
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the borrowed File owns a valid synchronous filesystem handle and
    // the correctly typed output remains live for the complete call.
    unsafe { GetFileInformationByHandle(handle, &mut information) }
        .map_err(|_| PrivateFsError::Unsafe)?;
    let is_directory = information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    if is_directory != matches!(kind, NodeKind::Directory)
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || matches!(kind, NodeKind::Regular) && information.nNumberOfLinks != 1
    {
        return Err(PrivateFsError::Unsafe);
    }
    let mut identity = FILE_ID_INFO::default();
    // SAFETY: the output is correctly sized/aligned for FileIdInfo and live
    // for the complete synchronous call.
    unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&raw mut identity).cast(),
            u32::try_from(std::mem::size_of::<FILE_ID_INFO>())
                .map_err(|_| PrivateFsError::PrimitiveUnavailable)?,
        )
    }
    .map_err(|_| PrivateFsError::Unsafe)?;
    Ok(RawIdentity {
        volume_serial_number: identity.VolumeSerialNumber,
        file_id: identity.FileId.Identifier,
    })
}

fn move_file(
    source: &Path,
    destination: &Path,
    flags: windows::Win32::Storage::FileSystem::MOVE_FILE_FLAGS,
) -> Result<(), PrivateFsError> {
    let source = wide_path(source);
    let destination = wide_path(destination);
    // SAFETY: both UTF-16 paths are NUL-terminated and live for the complete
    // synchronous call. The safe caller constrains both to one directory.
    let result = unsafe {
        MoveFileExW(
            windows::core::PCWSTR(source.as_ptr()),
            windows::core::PCWSTR(destination.as_ptr()),
            flags,
        )
    };
    if result.is_ok() {
        return Ok(());
    }
    // SAFETY: queried immediately after the failed Win32 call on this thread.
    let error = unsafe { GetLastError() };
    if error == ERROR_ALREADY_EXISTS || error == ERROR_FILE_EXISTS {
        Err(PrivateFsError::AlreadyExists)
    } else {
        Err(PrivateFsError::Io)
    }
}

fn map_failed_create(
    error: std::io::Error,
    path: &Path,
    report_already_exists: bool,
) -> PrivateFsError {
    if report_already_exists && error.kind() == std::io::ErrorKind::AlreadyExists {
        PrivateFsError::AlreadyExists
    } else if matches!(
        std::fs::symlink_metadata(path),
        Err(observed) if observed.kind() == std::io::ErrorKind::NotFound
    ) {
        PrivateFsError::Io
    } else {
        PrivateFsError::SettlementUnknown
    }
}

fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn private_node_acl(file: &File) -> bool {
    const MAX_TOKEN_INFORMATION_BYTES: u32 = 64 * 1024;
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACCESS_DENIED_ACE_TYPE: u8 = 1;

    let current_user = match CurrentUser::open(MAX_TOKEN_INFORMATION_BYTES) {
        Some(user) => user,
        None => return false,
    };
    let current_user_sid = current_user.sid();
    if !unsafe { IsValidSid(current_user_sid).as_bool() } {
        return false;
    }

    let mut owner = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the borrowed filesystem handle stays live. All output slots are
    // valid and the returned descriptor is released by LocalDescriptor.
    let status = unsafe {
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&raw mut owner),
            None,
            Some(&raw mut dacl),
            None,
            Some(&raw mut descriptor),
        )
    };
    if status != ERROR_SUCCESS || descriptor.0.is_null() {
        return false;
    }
    let _descriptor = LocalDescriptor(descriptor);
    if owner.is_invalid()
        || !unsafe { IsValidSid(owner).as_bool() }
        || unsafe { EqualSid(owner, current_user_sid) }.is_err()
        || dacl.is_null()
    {
        return false;
    }

    let mutation_rights = FILE_WRITE_DATA.0
        | FILE_APPEND_DATA.0
        | FILE_WRITE_EA.0
        | FILE_WRITE_ATTRIBUTES.0
        | FILE_DELETE_CHILD.0
        | DELETE.0
        | WRITE_DAC.0
        | WRITE_OWNER.0
        | GENERIC_WRITE.0
        | GENERIC_ALL.0;
    // SAFETY: dacl points into the live kernel-allocated descriptor.
    let ace_count = unsafe { (*dacl).AceCount };
    for index in 0..u32::from(ace_count) {
        let mut ace: *mut c_void = std::ptr::null_mut();
        // SAFETY: the index is within the ACL's declared ACE count.
        if unsafe { GetAce(dacl, index, &raw mut ace) }.is_err() || ace.is_null() {
            return false;
        }
        // SAFETY: GetAce returned at least an ACE_HEADER.
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        if header.AceType == ACCESS_DENIED_ACE_TYPE {
            continue;
        }
        if header.AceType != ACCESS_ALLOWED_ACE_TYPE
            || usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
        {
            return false;
        }
        // SAFETY: the size check proves the fixed allow-ACE prefix.
        let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        if allowed.Mask & mutation_rights == 0 {
            continue;
        }
        let sid = PSID(std::ptr::addr_of!(allowed.SidStart).cast_mut().cast());
        if !unsafe { IsValidSid(sid).as_bool() } {
            return false;
        }
        // SAFETY: IsValidSid proves GetLengthSid may inspect this SID.
        let sid_bytes = unsafe { GetLengthSid(sid) } as usize;
        let sid_offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        if sid_offset
            .checked_add(sid_bytes)
            .is_none_or(|bytes| bytes > usize::from(header.AceSize))
            || !trusted_writer(sid, current_user_sid)
        {
            return false;
        }
    }
    true
}

struct LocalDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for LocalDescriptor {
    fn drop(&mut self) {
        // SAFETY: GetSecurityInfo allocated this descriptor with LocalAlloc and
        // transferred ownership to this guard.
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0 .0))) };
    }
}

struct CurrentUser {
    _token: OwnedHandle,
    information: Vec<usize>,
}

impl CurrentUser {
    fn open(max_bytes: u32) -> Option<Self> {
        let mut token_handle = HANDLE::default();
        // SAFETY: the pseudo-process handle is valid and output slot is live.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token_handle) }
            .ok()?;
        if token_handle.is_invalid() {
            return None;
        }
        // SAFETY: OpenProcessToken transferred ownership of this handle.
        let token = unsafe { OwnedHandle::from_raw_handle(token_handle.0) };
        let token_handle = HANDLE(token.as_raw_handle());
        let mut bytes = 0u32;
        // SAFETY: a null buffer is the documented size query.
        let _ = unsafe { GetTokenInformation(token_handle, TokenUser, None, 0, &raw mut bytes) };
        if bytes < u32::try_from(std::mem::size_of::<TOKEN_USER>()).ok()? || bytes > max_bytes {
            return None;
        }
        let words = usize::try_from(bytes)
            .ok()?
            .checked_add(std::mem::size_of::<usize>() - 1)?
            / std::mem::size_of::<usize>();
        let mut information = vec![0usize; words];
        // SAFETY: the aligned buffer has the exact kernel-reported capacity.
        unsafe {
            GetTokenInformation(
                token_handle,
                TokenUser,
                Some(information.as_mut_ptr().cast()),
                bytes,
                &raw mut bytes,
            )
        }
        .ok()?;
        Some(Self {
            _token: token,
            information,
        })
    }

    fn sid(&self) -> PSID {
        // SAFETY: successful construction populated at least TOKEN_USER and
        // the backing aligned allocation is borrowed for this call.
        unsafe { &*self.information.as_ptr().cast::<TOKEN_USER>() }
            .User
            .Sid
    }
}

fn trusted_writer(candidate: PSID, current_user: PSID) -> bool {
    use windows::Win32::Security::{
        CreateWellKnownSid, WinBuiltinAdministratorsSid, WinCreatorOwnerRightsSid,
        WinCreatorOwnerSid, WinLocalSystemSid, WELL_KNOWN_SID_TYPE,
    };

    // SAFETY: caller validated both live SIDs.
    if unsafe { EqualSid(candidate, current_user) }.is_ok() {
        return true;
    }
    [
        WinLocalSystemSid,
        WinBuiltinAdministratorsSid,
        WinCreatorOwnerSid,
        WinCreatorOwnerRightsSid,
    ]
    .into_iter()
    .any(|kind: WELL_KNOWN_SID_TYPE| {
        let mut storage = [0usize; 16];
        let mut bytes = u32::try_from(std::mem::size_of_val(&storage)).unwrap_or(u32::MAX);
        let trusted = PSID(storage.as_mut_ptr().cast());
        // SAFETY: storage exceeds SECURITY_MAX_SID_SIZE and the immediate
        // comparison borrows both live SIDs.
        unsafe {
            CreateWellKnownSid(kind, None, Some(trusted), &raw mut bytes).is_ok()
                && EqualSid(candidate, trusted).is_ok()
        }
    })
}
