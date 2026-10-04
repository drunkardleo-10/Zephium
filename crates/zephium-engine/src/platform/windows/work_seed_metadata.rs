//! Nonsecret, append-only initial-seed decisions inside the erased profile tree.
//!
//! The host publishes a stamp only after a successful seed or observation of
//! retained cookies, before exposing the page to navigation or Human activity.

use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    EqualSid, GetAce, GetSecurityDescriptorControl, GetTokenInformation, IsValidSid, TokenUser,
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    CreateDirectoryW, FileDispositionInfo, GetFileInformationByHandle, GetFinalPathNameByHandleW,
    MoveFileExW, SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ALL_ACCESS,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_DELETE_ON_CLOSE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, MOVEFILE_WRITE_THROUGH, READ_CONTROL,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const STAMP: &[u8] = b"ZWS1\n";
const DIRECTORY: &str = "work-seed-v1";
const MAX_ENTRIES: usize = 4096;
const MAX_ANCESTORS: usize = 128;
const MAX_SITES: usize = 256;
const MAX_STORES_PER_SITE: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WorkStoreDescriptor {
    pub(crate) https: bool,
    pub(crate) port: u16,
}

pub(crate) struct WorkSeedMetadata {
    path: PathBuf,
    // The no-delete-sharing sentinel keeps the writable directory nonempty,
    // excluding in-place junction changes without blocking child publication.
    _sentinel: File,
    // No-delete ancestor pins exclude replacement. Handles die before erasure.
    _directories: Vec<File>,
}

impl WorkSeedMetadata {
    pub(crate) fn open(profile_directory: &Path) -> Result<Self, ()> {
        if !profile_directory.is_absolute()
            || profile_directory.canonicalize().map_err(|_| ())? != profile_directory
        {
            return Err(());
        }
        let mut directories = pin_directories(profile_directory)?;
        let path = profile_directory.join(DIRECTORY);
        match fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if create_private_directory(&path).is_err() && fs::symlink_metadata(&path).is_err()
                {
                    return Err(());
                }
            }
            Err(_) => return Err(()),
        }
        let directory = open_directory(&path, true)?;
        verify_private_acl(&directory, true)?;
        let sentinel_path = path.join(format!("pin-{}", zephium_core::ids::ProfileId::generate()));
        let sentinel = OpenOptions::new()
            .read(true)
            .write(true)
            .access_mode(FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0 | DELETE.0 | READ_CONTROL.0)
            .create_new(true)
            .share_mode(FILE_SHARE_READ.0)
            .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_DELETE_ON_CLOSE).0)
            .open(&sentinel_path)
            .map_err(|_| ())?;
        // A redirected empty creation carries no person data and is removed
        // by this exact handle's delete-on-close if validation fails.
        verify_file(&sentinel, &sentinel_path)?;
        verify_private_acl(&sentinel, false)?;
        if sentinel.metadata().map_err(|_| ())?.len() != 0 {
            return Err(());
        }
        // The profile is now nonempty because the pinned metadata child
        // exists, so it can share write access without admitting a junction.
        let profile_pin = open_directory(profile_directory, true)?;
        let Some(last) = directories.last_mut() else {
            return Err(());
        };
        *last = profile_pin;
        directories.push(directory);
        Ok(Self {
            path,
            _sentinel: sentinel,
            _directories: directories,
        })
    }

    pub(crate) fn is_initialized(&self, site_name: &str, origin: &str) -> Result<bool, ()> {
        self.read_stamp(&self.path.join(stamp_name(site_name, origin)?))
    }

    pub(crate) fn initialize(&self, site_name: &str, origin: &str) -> Result<(), ()> {
        let destination = self.path.join(stamp_name(site_name, origin)?);
        self.publish_stamp(&destination)
    }

    /// Called only for an actual approved Work construction, never a presence
    /// query. The directory stores no hostname, URL, origin or cookie content.
    pub(crate) fn record_store(
        &self,
        registrable_site: &str,
        https: bool,
        port: u16,
    ) -> Result<(), ()> {
        if port == 0 {
            return Err(());
        }
        let site = site_digest(registrable_site)?;
        let descriptor = WorkStoreDescriptor { https, port };
        let destination = self.path.join(descriptor_name(&site, descriptor));
        if self.read_stamp(&destination)? {
            return Ok(());
        }
        if self.known_stores(registrable_site)?.len() >= MAX_STORES_PER_SITE {
            return Err(());
        }
        let mut sites = std::collections::HashSet::new();
        for name in self.entry_names()? {
            if let Some((known_site, _)) = parse_descriptor_name(&name)? {
                sites.insert(known_site);
                if sites.len() > MAX_SITES {
                    return Err(());
                }
            }
        }
        if sites.len() == MAX_SITES && !sites.contains(&site) {
            return Err(());
        }
        self.publish_stamp(&destination)
    }

    /// Returns only previously recorded, validated scheme/port pairs for this
    /// site's digest. It does not create browser stores or infer arbitrary ports.
    pub(crate) fn known_stores(
        &self,
        registrable_site: &str,
    ) -> Result<Vec<WorkStoreDescriptor>, ()> {
        let site = site_digest(registrable_site)?;
        let mut stores = Vec::new();
        for name in self.entry_names()? {
            if let Some((known_site, descriptor)) = parse_descriptor_name(&name)? {
                if known_site == site {
                    if !self.read_stamp(&self.path.join(&name))? {
                        return Err(());
                    }
                    stores.push(descriptor);
                    if stores.len() > MAX_STORES_PER_SITE {
                        return Err(());
                    }
                }
            }
        }
        stores.sort_by_key(|descriptor| (descriptor.https, descriptor.port));
        Ok(stores)
    }

    fn entry_names(&self) -> Result<Vec<String>, ()> {
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.path).map_err(|_| ())? {
            if names.len() >= MAX_ENTRIES {
                return Err(());
            }
            let entry = entry.map_err(|_| ())?;
            names.push(entry.file_name().into_string().map_err(|_| ())?);
        }
        Ok(names)
    }

    fn publish_stamp(&self, destination: &Path) -> Result<(), ()> {
        if self.read_stamp(destination)? {
            return Ok(());
        }
        // Bound both allocation and storage. Refuse growth at the limit while
        // leaving already published decisions readable.
        let mut entries = 0;
        for entry in fs::read_dir(&self.path).map_err(|_| ())? {
            entry.map_err(|_| ())?;
            entries += 1;
            if entries >= MAX_ENTRIES {
                return Err(());
            }
        }
        let suffix = zephium_core::ids::ProfileId::generate();
        let temporary = self.path.join(format!("pending-{suffix}"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .access_mode(FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0 | DELETE.0 | READ_CONTROL.0)
            .create_new(true)
            .share_mode((FILE_SHARE_READ | FILE_SHARE_DELETE).0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(&temporary)
            .map_err(|_| ())?;
        let mut staged = StagedStamp {
            file,
            published: false,
        };
        verify_file(&staged.file, &temporary)?;
        verify_private_acl(&staged.file, false)?;
        staged.file.write_all(STAMP).map_err(|_| ())?;
        staged.file.sync_all().map_err(|_| ())?;
        let from = wide(&temporary)?;
        let to = wide(destination)?;
        // SAFETY: every ancestor and the private parent remain pinned; the
        // staging handle excludes other writers. No replacement/copy flag is
        // supplied, so a concurrent initialized decision is never overwritten.
        match unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        } {
            Ok(()) => {
                staged.published = true;
                verify_file(&staged.file, destination)?;
                if staged.file.metadata().map_err(|_| ())?.len() != STAMP.len() as u64 {
                    return Err(());
                }
                Ok(())
            }
            Err(error) if matches!(error.code().0 as u32 & 0xffff, 80 | 183) => {
                if self.read_stamp(destination)? {
                    Ok(())
                } else {
                    Err(())
                }
            }
            Err(_) => Err(()),
        }
    }

    fn read_stamp(&self, path: &Path) -> Result<bool, ()> {
        let file = match OpenOptions::new()
            .read(true)
            .access_mode(FILE_GENERIC_READ.0 | READ_CONTROL.0)
            .share_mode(FILE_SHARE_READ.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(()),
        };
        verify_file(&file, path)?;
        verify_private_acl(&file, false)?;
        if file.metadata().map_err(|_| ())?.len() != STAMP.len() as u64 {
            return Err(());
        }
        let mut bytes = Vec::with_capacity(STAMP.len() + 1);
        file.take((STAMP.len() + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| ())?;
        if bytes == STAMP {
            Ok(true)
        } else {
            Err(())
        }
    }
}

fn site_digest(site: &str) -> Result<String, ()> {
    if site.is_empty()
        || site.len() > 253
        || !site.is_ascii()
        || site.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(());
    }
    Ok(format!("{:x}", Sha256::digest(site.as_bytes())))
}
fn descriptor_name(site: &str, descriptor: WorkStoreDescriptor) -> String {
    format!(
        "store-{site}-{}-{}.stamp",
        u8::from(descriptor.https),
        descriptor.port
    )
}
fn parse_descriptor_name(name: &str) -> Result<Option<(String, WorkStoreDescriptor)>, ()> {
    let Some(value) = name.strip_prefix("store-") else {
        return Ok(None);
    };
    let (site, scalar) = value.split_once('-').ok_or(())?;
    if site.len() != 64
        || !site
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(());
    }
    let (scheme, port) = scalar
        .strip_suffix(".stamp")
        .ok_or(())?
        .split_once('-')
        .ok_or(())?;
    let https = match scheme {
        "0" => false,
        "1" => true,
        _ => return Err(()),
    };
    let port = port.parse::<u16>().map_err(|_| ())?;
    let descriptor = WorkStoreDescriptor { https, port };
    if port == 0 || descriptor_name(site, descriptor) != name {
        return Err(());
    }
    Ok(Some((site.into(), descriptor)))
}

fn stamp_name(site_name: &str, origin: &str) -> Result<String, ()> {
    if site_name.is_empty() || site_name.len() > 128 || !site_name.is_ascii() || origin.len() > 2048
    {
        return Err(());
    }
    let parsed = zephium_agentic::ContextCookieOrigin::parse(origin).map_err(|_| ())?;
    if parsed.as_url().origin().ascii_serialization() != origin {
        return Err(());
    }
    let mut digest = Sha256::new();
    digest.update(b"ZephiumWorkSeedV1\0");
    digest.update(site_name.as_bytes());
    digest.update([0]);
    digest.update(origin.as_bytes());
    Ok(format!("{:x}.stamp", digest.finalize()))
}

fn handle(file: &File) -> HANDLE {
    HANDLE(file.as_raw_handle())
}
fn wide(path: &Path) -> Result<Vec<u16>, ()> {
    let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
    if value.is_empty() || value.len() > 32766 || value.contains(&0) {
        return Err(());
    }
    value.push(0);
    Ok(value)
}
fn opened_at(file: &File, expected: &Path) -> bool {
    let mut name = vec![0u16; 32768];
    // SAFETY: borrowed live handle and writable bounded output buffer.
    let length = unsafe { GetFinalPathNameByHandleW(handle(file), &mut name, FILE_NAME_NORMALIZED) }
        as usize;
    length != 0
        && length < name.len()
        && std::ffi::OsString::from_wide(&name[..length]) == expected.as_os_str()
}
fn open_directory(path: &Path, writable_share: bool) -> Result<File, ()> {
    let file = OpenOptions::new()
        .access_mode(READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0)
        .share_mode(
            FILE_SHARE_READ.0
                | if writable_share {
                    FILE_SHARE_WRITE.0
                } else {
                    0
                },
        )
        .custom_flags((FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).0)
        .open(path)
        .map_err(|_| ())?;
    let metadata = file.metadata().map_err(|_| ())?;
    if !metadata.is_dir()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || !opened_at(&file, path)
    {
        return Err(());
    }
    Ok(file)
}
fn pin_directories(path: &Path) -> Result<Vec<File>, ()> {
    let ancestors: Vec<_> = path.ancestors().collect();
    if ancestors.len() > MAX_ANCESTORS {
        return Err(());
    }
    ancestors
        .into_iter()
        .rev()
        .map(|ancestor| open_directory(ancestor, ancestor != path))
        .collect()
}
fn verify_file(file: &File, path: &Path) -> Result<(), ()> {
    let metadata = file.metadata().map_err(|_| ())?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || !opened_at(file, path)
    {
        return Err(());
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: exact borrowed file handle and initialized native output.
    unsafe { GetFileInformationByHandle(handle(file), &mut info) }.map_err(|_| ())?;
    if info.nNumberOfLinks == 1 {
        Ok(())
    } else {
        Err(())
    }
}

struct StagedStamp {
    file: File,
    published: bool,
}
impl Drop for StagedStamp {
    fn drop(&mut self) {
        if !self.published {
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: cleanup deletes the exact owned staging object by
            // handle, even if an excluded same-user mutation renamed it.
            let _ = unsafe {
                SetFileInformationByHandle(
                    handle(&self.file),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    std::mem::size_of_val(&disposition) as u32,
                )
            };
        }
    }
}

struct LocalAllocation(HLOCAL);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: Windows transferred LocalAlloc ownership into this guard.
        let _ = unsafe { LocalFree(Some(self.0)) };
    }
}
struct CurrentUser {
    _token: OwnedHandle,
    bytes: Vec<usize>,
}
impl CurrentUser {
    fn open() -> Result<Self, ()> {
        let mut token = HANDLE::default();
        // SAFETY: valid process pseudo-handle and writable token output.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
            .map_err(|_| ())?;
        // SAFETY: successful OpenProcessToken transferred unique handle ownership.
        let token = unsafe { OwnedHandle::from_raw_handle(token.0) };
        let mut length = 0;
        // SAFETY: documented zero-capacity token-information size query.
        let _ = unsafe {
            GetTokenInformation(
                HANDLE(token.as_raw_handle()),
                TokenUser,
                None,
                0,
                &mut length,
            )
        };
        if length < std::mem::size_of::<TOKEN_USER>() as u32 || length > 65536 {
            return Err(());
        }
        let mut bytes = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        // SAFETY: allocation is aligned and has the reported writable capacity.
        unsafe {
            GetTokenInformation(
                HANDLE(token.as_raw_handle()),
                TokenUser,
                Some(bytes.as_mut_ptr().cast()),
                length,
                &mut length,
            )
        }
        .map_err(|_| ())?;
        let user = Self {
            _token: token,
            bytes,
        };
        // SAFETY: TOKEN_USER is retained inside the aligned output allocation.
        if !unsafe { IsValidSid(user.sid()).as_bool() } {
            return Err(());
        }
        Ok(user)
    }
    fn sid(&self) -> PSID {
        // SAFETY: successful GetTokenInformation initialized a complete TOKEN_USER.
        unsafe { (&*self.bytes.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }
    fn text(&self) -> Result<String, ()> {
        let mut text = PWSTR::null();
        // SAFETY: validated retained SID and writable allocated-string output.
        unsafe { ConvertSidToStringSidW(self.sid(), &mut text) }.map_err(|_| ())?;
        let _allocation = LocalAllocation(HLOCAL(text.0.cast()));
        for length in 0..256 {
            // SAFETY: documented terminated SID string; bounded to 256 units.
            if unsafe { text.0.add(length).read() } == 0 {
                // SAFETY: the preceding scan established this initialized prefix.
                return String::from_utf16(unsafe { std::slice::from_raw_parts(text.0, length) })
                    .map_err(|_| ());
            }
        }
        Err(())
    }
}
fn create_private_directory(path: &Path) -> Result<(), ()> {
    let user = CurrentUser::open()?.text()?;
    let sddl = wide(Path::new(&format!(
        "O:{user}D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;{user})"
    )))?;
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: terminated SDDL and initialized descriptor output; LocalFree guard follows.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(|_| ())?;
    let _allocation = LocalAllocation(HLOCAL(descriptor.0));
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let path = wide(path)?;
    // SAFETY: path and descriptor stay live through this create-only directory operation.
    unsafe { CreateDirectoryW(PCWSTR(path.as_ptr()), Some(&attributes)) }.map_err(|_| ())
}
fn verify_private_acl(file: &File, protected: bool) -> Result<(), ()> {
    let user = CurrentUser::open()?;
    let mut owner = PSID::default();
    let mut dacl = std::ptr::null_mut::<ACL>();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: borrowed live file/directory handle and writable outputs; descriptor is LocalFree-owned.
    let result = unsafe {
        GetSecurityInfo(
            handle(file),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut dacl),
            None,
            Some(&mut descriptor),
        )
    };
    if result != ERROR_SUCCESS {
        return Err(());
    }
    let _allocation = LocalAllocation(HLOCAL(descriptor.0));
    // SAFETY: successful GetSecurityInfo returned descriptor-owned SID and ACL.
    if dacl.is_null() || unsafe { EqualSid(owner, user.sid()) }.is_err() {
        return Err(());
    }
    let mut control = 0u16;
    let mut revision = 0;
    // SAFETY: descriptor remains owned and outputs are initialized.
    unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) }
        .map_err(|_| ())?;
    // SAFETY: the successful native descriptor query supplied a valid ACL.
    if (protected && control & SE_DACL_PROTECTED.0 == 0) || unsafe { (*dacl).AceCount } != 2 {
        return Err(());
    }
    let mut found_user = false;
    let mut found_system = false;
    for index in 0..2 {
        let mut pointer = std::ptr::null_mut();
        // SAFETY: index is below the verified ACL count and output remains writable.
        unsafe { GetAce(dacl, index, &mut pointer) }.map_err(|_| ())?;
        // SAFETY: native GetAce supplied an ACL-owned aligned ACE header.
        let header = unsafe { &*pointer.cast::<ACE_HEADER>() };
        if header.AceType != 0
            || usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
        {
            return Err(());
        }
        // SAFETY: the header now establishes the access-allowed layout and
        // its minimum size before any typed field beyond the header is read.
        let ace = unsafe { &*pointer.cast::<ACCESS_ALLOWED_ACE>() };
        if ace.Mask != FILE_ALL_ACCESS.0 {
            return Err(());
        }
        let sid = PSID((&ace.SidStart as *const u32).cast_mut().cast());
        // SAFETY: validated access-allowed ACE contains its inline native SID.
        if !unsafe { IsValidSid(sid).as_bool() } {
            return Err(());
        }
        // SAFETY: both SIDs have been validated and remain retained.
        if unsafe { EqualSid(sid, user.sid()) }.is_ok() {
            found_user = true;
        }
        // SAFETY: the inline ACE SID remains valid for this native comparison.
        else if unsafe {
            windows::Win32::Security::IsWellKnownSid(
                sid,
                windows::Win32::Security::WinLocalSystemSid,
            )
            .as_bool()
        } {
            found_system = true;
        } else {
            return Err(());
        }
    }
    if found_user && found_system {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initialized_decisions_are_idempotent_exact_and_profile_owned() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().canonicalize().unwrap();
        let metadata = WorkSeedMetadata::open(&profile).unwrap();
        assert!(!metadata
            .is_initialized("work-site-test", "https://example.test")
            .unwrap());
        metadata
            .initialize("work-site-test", "https://example.test")
            .unwrap();
        metadata
            .initialize("work-site-test", "https://example.test")
            .unwrap();
        assert!(metadata
            .is_initialized("work-site-test", "https://example.test")
            .unwrap());
        assert!(!metadata
            .is_initialized("work-site-test", "http://example.test")
            .unwrap());
        assert!(!metadata
            .is_initialized("other-site", "https://example.test")
            .unwrap());
        let entries: Vec<_> = fs::read_dir(profile.join(DIRECTORY))
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".stamp")
            })
            .collect();
        assert_eq!(entries.len(), 1);
        drop(metadata);
        let metadata = WorkSeedMetadata::open(&profile).unwrap();
        assert!(metadata
            .is_initialized("work-site-test", "https://example.test")
            .unwrap());
        drop(metadata);
        fs::remove_dir_all(&profile).unwrap();
        assert!(!profile.exists());
    }
    #[test]
    fn hostile_existing_stamps_and_parent_replacement_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().canonicalize().unwrap();
        let metadata = WorkSeedMetadata::open(&profile).unwrap();
        metadata
            .initialize("work-site-test", "https://example.test")
            .unwrap();
        let path = metadata
            .path
            .join(stamp_name("work-site-test", "https://example.test").unwrap());
        fs::write(&path, b"wrong").unwrap();
        assert!(metadata
            .is_initialized("work-site-test", "https://example.test")
            .is_err());
        assert!(metadata
            .initialize("work-site-test", "https://example.test")
            .is_err());
        assert!(fs::rename(&metadata.path, profile.join("replaced")).is_err());
        fs::remove_file(&path).unwrap();
        let target = profile.join("target");
        fs::write(&target, STAMP).unwrap();
        fs::hard_link(&target, &path).unwrap();
        assert!(metadata
            .is_initialized("work-site-test", "https://example.test")
            .is_err());
        assert_eq!(fs::read(&target).unwrap(), STAMP);
    }

    #[test]
    fn remembered_stores_are_bounded_hashed_and_survive_reopen() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().canonicalize().unwrap();
        let metadata = WorkSeedMetadata::open(&profile).unwrap();
        assert!(metadata.known_stores("example.test").unwrap().is_empty());
        metadata.record_store("example.test", false, 8080).unwrap();
        metadata.record_store("example.test", true, 443).unwrap();
        metadata.record_store("example.test", true, 443).unwrap();
        assert!(metadata.known_stores("other.test").unwrap().is_empty());
        assert!(metadata.record_store("example.test", true, 0).is_err());
        for port in 1..=6 {
            metadata.record_store("example.test", true, port).unwrap();
        }
        assert!(metadata.record_store("example.test", true, 7).is_err());
        for name in metadata.entry_names().unwrap() {
            assert!(!name.contains("example.test"));
        }
        drop(metadata);
        let metadata = WorkSeedMetadata::open(&profile).unwrap();
        let stores = metadata.known_stores("example.test").unwrap();
        assert_eq!(stores.len(), 8);
        assert!(stores.contains(&WorkStoreDescriptor {
            https: false,
            port: 8080
        }));
        assert!(stores.contains(&WorkStoreDescriptor {
            https: true,
            port: 443
        }));
    }
}
