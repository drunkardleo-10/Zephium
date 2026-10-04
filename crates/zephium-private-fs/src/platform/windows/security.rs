//! Protected, explicit owner/system/administrator DACLs for private nodes.

pub(super) mod native_storage;

use std::ffi::c_void;
use std::fs::File;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SetSecurityInfo, SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    EqualSid, GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetTokenInformation,
    IsValidSid, TokenUser, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::PrivateFsError;

const ALL: u32 = 0x001f_01ff;

fn owner_access(sealed: bool, directory: bool) -> u32 {
    // Payload files need no execute or ownership-transfer permission. Only
    // directories receive traverse, and only writable directories delete-child.
    super::native::READ
        | super::native::METADATA
        | if sealed { 0 } else { super::native::WRITE }
        | if directory {
            0x20 | if sealed { 0 } else { 0x40 }
        } else {
            0
        }
}

pub(super) struct Descriptor(pub(super) PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0 .0))) };
    }
}

pub(super) fn descriptor(sealed: bool, directory: bool) -> Result<Descriptor, PrivateFsError> {
    let user = CurrentUser::open(64 * 1024).ok_or(PrivateFsError::Unsafe)?;
    let sid = sid_string(user.sid())?;
    let mask = owner_access(sealed, directory);
    let text = format!("O:{sid}D:P(A;;0x{mask:08x};;;{sid})(A;;FA;;;SY)(A;;FA;;;BA)");
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let mut result = PSECURITY_DESCRIPTOR::default();
    // SAFETY: bounded NUL-terminated SDDL; Windows allocates the descriptor.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide.as_ptr()),
            1,
            &raw mut result,
            None,
        )
    }
    .map_err(|_| PrivateFsError::Unsafe)?;
    if result.0.is_null() {
        return Err(PrivateFsError::Unsafe);
    }
    Ok(Descriptor(result))
}

pub(super) fn set_mode(file: &File, sealed: bool) -> Result<(), PrivateFsError> {
    let desired = descriptor(sealed, super::identity(file, None)?.1)?;
    let mut present = windows::core::BOOL(0);
    let mut defaulted = windows::core::BOOL(0);
    let mut acl = std::ptr::null_mut();
    // SAFETY: owned valid descriptor and live output slots.
    unsafe {
        GetSecurityDescriptorDacl(
            desired.0,
            &raw mut present,
            &raw mut acl,
            &raw mut defaulted,
        )
    }
    .map_err(|_| PrivateFsError::Unsafe)?;
    if !present.as_bool() || acl.is_null() {
        return Err(PrivateFsError::Unsafe);
    }
    let status = unsafe {
        SetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(acl),
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(PrivateFsError::Io);
    }
    if mode(file)? != sealed {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    Ok(())
}

fn query(file: &File) -> Result<(Descriptor, PSID, *mut ACL), PrivateFsError> {
    let mut owner = PSID::default();
    let mut acl = std::ptr::null_mut();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: all outputs belong to the returned LocalAlloc descriptor.
    let status = unsafe {
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&raw mut owner),
            None,
            Some(&raw mut acl),
            None,
            Some(&raw mut sd),
        )
    };
    if status != ERROR_SUCCESS || sd.0.is_null() {
        return Err(PrivateFsError::Unsafe);
    }
    let guard = Descriptor(sd);
    if owner.is_invalid() || acl.is_null() || !unsafe { IsValidSid(owner).as_bool() } {
        return Err(PrivateFsError::Unsafe);
    }
    Ok((guard, owner, acl))
}

fn sid_string(sid: PSID) -> Result<String, PrivateFsError> {
    let mut text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.map_err(|_| PrivateFsError::Unsafe)?;
    if text.is_null() {
        return Err(PrivateFsError::Unsafe);
    }
    let result = unsafe { text.to_string() }.map_err(|_| PrivateFsError::Unsafe);
    let _ = unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    result
}

fn entries(acl: *mut ACL) -> Result<Vec<(u8, u8, u32, PSID)>, PrivateFsError> {
    let base = acl as usize;
    let size = usize::from(unsafe { (*acl).AclSize });
    if size < std::mem::size_of::<ACL>() {
        return Err(PrivateFsError::Unsafe);
    }
    let end = base.checked_add(size).ok_or(PrivateFsError::Unsafe)?;
    let count = unsafe { (*acl).AceCount };
    if count > 256 {
        return Err(PrivateFsError::BoundExceeded);
    }
    let mut result = Vec::with_capacity(usize::from(count));
    for index in 0..u32::from(count) {
        let mut ace: *mut c_void = std::ptr::null_mut();
        unsafe { GetAce(acl, index, &raw mut ace) }.map_err(|_| PrivateFsError::Unsafe)?;
        let start = ace as usize;
        if start < base + std::mem::size_of::<ACL>()
            || !start.is_multiple_of(std::mem::align_of::<ACCESS_ALLOWED_ACE>())
            || start
                .checked_add(std::mem::size_of::<ACE_HEADER>())
                .is_none_or(|value| value > end)
        {
            return Err(PrivateFsError::Unsafe);
        }
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        if start
            .checked_add(usize::from(header.AceSize))
            .is_none_or(|value| value > end)
        {
            return Err(PrivateFsError::Unsafe);
        }
        // Permit only ordinary allow/deny ACEs. No callbacks, object-specific
        // masks or unparsed SIDs can silently pass the private-node boundary.
        let offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        if !matches!(header.AceType, 0 | 1) || usize::from(header.AceSize) < offset + 8 {
            return Err(PrivateFsError::Unsafe);
        }
        let raw = unsafe { ace.cast::<u8>().add(offset) };
        let subauthorities = unsafe { *raw.add(1) } as usize;
        if subauthorities > 15 || offset + 8 + 4 * subauthorities > usize::from(header.AceSize) {
            return Err(PrivateFsError::Unsafe);
        }
        let sid = PSID(raw.cast());
        if !unsafe { IsValidSid(sid).as_bool() } {
            return Err(PrivateFsError::Unsafe);
        }
        let mask = unsafe { (*ace.cast::<ACCESS_ALLOWED_ACE>()).Mask };
        result.push((header.AceType, header.AceFlags, mask, sid));
    }
    Ok(result)
}

/// Owned, bounded owner/DACL identity for independently opened handles.
#[derive(Eq, PartialEq)]
pub(super) struct SecuritySnapshot {
    owner: String,
    control: u16,
    entries: Vec<(u8, u8, u32, String)>,
}

pub(super) fn snapshot(file: &File) -> Result<SecuritySnapshot, PrivateFsError> {
    let (sd, owner, acl) = query(file)?;
    let mut control = 0u16;
    let mut revision = 0;
    // SAFETY: the queried descriptor stays owned until all SID/ACE data has
    // been copied; the outputs are live for this synchronous query.
    unsafe { GetSecurityDescriptorControl(sd.0, &raw mut control, &raw mut revision) }
        .map_err(|_| PrivateFsError::Unsafe)?;
    let owner = sid_string(owner)?;
    let entries = entries(acl)?
        .into_iter()
        .map(|(kind, flags, mask, sid)| sid_string(sid).map(|sid| (kind, flags, mask, sid)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SecuritySnapshot {
        owner,
        control,
        entries,
    })
}

pub(super) fn mode(file: &File) -> Result<bool, PrivateFsError> {
    let directory = super::identity(file, None)?.1;
    let user = CurrentUser::open(64 * 1024).ok_or(PrivateFsError::Unsafe)?;
    let (sd, owner, acl) = query(file)?;
    if unsafe { EqualSid(owner, user.sid()) }.is_err() {
        return Err(PrivateFsError::Unsafe);
    }
    let mut control = 0u16;
    let mut revision = 0;
    unsafe { GetSecurityDescriptorControl(sd.0, &raw mut control, &raw mut revision) }
        .map_err(|_| PrivateFsError::Unsafe)?;
    if control & 0x1000 == 0 {
        return Err(PrivateFsError::Unsafe);
    } // SE_DACL_PROTECTED
    let mut owner_mask = None;
    let mut system = false;
    let mut admins = false;
    let rows = entries(acl)?;
    if rows.len() != 3 {
        return Err(PrivateFsError::Unsafe);
    }
    for (kind, flags, mask, sid) in rows {
        if kind != 0 || flags != 0 {
            return Err(PrivateFsError::Unsafe);
        }
        if unsafe { EqualSid(sid, user.sid()) }.is_ok() {
            if owner_mask.replace(mask).is_some() {
                return Err(PrivateFsError::Unsafe);
            }
        } else {
            match sid_string(sid)?.as_str() {
                "S-1-5-18" if mask == ALL && !system => system = true,
                "S-1-5-32-544" if mask == ALL && !admins => admins = true,
                _ => return Err(PrivateFsError::Unsafe),
            }
        }
    }
    if !system || !admins {
        return Err(PrivateFsError::Unsafe);
    }
    match owner_mask {
        Some(mask) if mask == owner_access(false, directory) => Ok(false),
        Some(mask) if mask == owner_access(true, directory) => Ok(true),
        _ => Err(PrivateFsError::Unsafe),
    }
}

pub(super) fn ancestor(file: &File) -> Result<(), PrivateFsError> {
    let user = CurrentUser::open(64 * 1024).ok_or(PrivateFsError::Unsafe)?;
    let (_sd, owner, acl) = query(file)?;
    if unsafe { EqualSid(owner, user.sid()) }.is_err() && !trusted_system_sid(owner)? {
        return Err(PrivateFsError::Unsafe);
    }
    // Creating a sibling is not authority to replace an existing protected
    // child. Refuse grants that can rewrite ACLs/ownership or delete children.
    // DELETE, WRITE_DAC, WRITE_OWNER, DELETE_CHILD, WRITE_EA,
    // WRITE_ATTRIBUTES, GENERIC_ALL and GENERIC_WRITE.
    const DANGEROUS: u32 = 0x000d_0150 | 0x5000_0000;
    for (kind, flags, mask, sid) in entries(acl)? {
        if kind == 1 || flags & 8 != 0 || mask & DANGEROUS == 0 {
            continue;
        }
        if unsafe { EqualSid(sid, user.sid()) }.is_ok() {
            continue;
        }
        if !trusted_system_sid(sid)? {
            return Err(PrivateFsError::Unsafe);
        }
    }
    Ok(())
}

fn trusted_system_sid(sid: PSID) -> Result<bool, PrivateFsError> {
    Ok(matches!(
        sid_string(sid)?.as_str(),
        "S-1-5-18"
            | "S-1-5-32-544"
            | "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"
    ))
}

struct CurrentUser {
    _token: OwnedHandle,
    information: Vec<usize>,
}
impl CurrentUser {
    fn open(max: u32) -> Option<Self> {
        let mut raw = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut raw) }.ok()?;
        if raw.is_invalid() {
            return None;
        }
        let token = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let mut bytes = 0;
        let _ = unsafe {
            GetTokenInformation(
                HANDLE(token.as_raw_handle()),
                TokenUser,
                None,
                0,
                &raw mut bytes,
            )
        };
        if bytes < std::mem::size_of::<TOKEN_USER>() as u32 || bytes > max {
            return None;
        }
        let mut information = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            GetTokenInformation(
                HANDLE(token.as_raw_handle()),
                TokenUser,
                Some(information.as_mut_ptr().cast()),
                bytes,
                &raw mut bytes,
            )
        }
        .ok()?;
        let result = Self {
            _token: token,
            information,
        };
        let base = result.information.as_ptr() as usize;
        let end = base.checked_add(bytes as usize)?;
        let sid = result.sid().0 as usize;
        if sid < base || sid.checked_add(8)? > end {
            return None;
        }
        let count = unsafe { *((sid as *const u8).add(1)) } as usize;
        if count > 15 || sid.checked_add(8 + count * 4)? > end {
            return None;
        }
        unsafe { IsValidSid(result.sid()).as_bool() }.then_some(result)
    }
    fn sid(&self) -> PSID {
        unsafe { (*self.information.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }
}
