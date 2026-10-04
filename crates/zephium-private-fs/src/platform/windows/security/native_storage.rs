//! Distinct inheritable ACL contract for the native storage ancestor.

use super::*;

pub(in crate::platform::windows) fn descriptor() -> Result<Descriptor, PrivateFsError> {
    let user = CurrentUser::open(64 * 1024).ok_or(PrivateFsError::Unsafe)?;
    let sid = sid_string(user.sid())?;
    let text = format!("O:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)");
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let mut result = PSECURITY_DESCRIPTOR::default();
    // SAFETY: terminated bounded SDDL and live output; Descriptor owns LocalFree.
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

pub(in crate::platform::windows) fn verify(file: &File) -> Result<(), PrivateFsError> {
    verify_flags(file, 3, true)
}

fn verify_flags(file: &File, flags: u8, protected: bool) -> Result<(), PrivateFsError> {
    let user = CurrentUser::open(64 * 1024).ok_or(PrivateFsError::Unsafe)?;
    let (sd, owner, acl) = query(file)?;
    if unsafe { EqualSid(owner, user.sid()) }.is_err() {
        return Err(PrivateFsError::Unsafe);
    }
    let mut control = 0u16;
    let mut revision = 0;
    // SAFETY: descriptor remains owned through the query and outputs are live.
    unsafe { GetSecurityDescriptorControl(sd.0, &raw mut control, &raw mut revision) }
        .map_err(|_| PrivateFsError::Unsafe)?;
    if (control & 0x1000 != 0) != protected {
        return Err(PrivateFsError::Unsafe);
    }
    let rows = entries(acl)?;
    if rows.len() != 3 {
        return Err(PrivateFsError::Unsafe);
    }
    let mut found = [false; 3];
    for (kind, observed_flags, mask, sid) in rows {
        if kind != 0 || observed_flags != flags || mask != ALL {
            return Err(PrivateFsError::Unsafe);
        }
        let index = if unsafe { EqualSid(sid, user.sid()) }.is_ok() {
            0
        } else {
            match sid_string(sid)?.as_str() {
                "S-1-5-18" => 1,
                "S-1-5-32-544" => 2,
                _ => return Err(PrivateFsError::Unsafe),
            }
        };
        if std::mem::replace(&mut found[index], true) {
            return Err(PrivateFsError::Unsafe);
        }
    }
    if found.iter().all(|value| *value) {
        Ok(())
    } else {
        Err(PrivateFsError::Unsafe)
    }
}

pub(in crate::platform::windows) fn verify_inherited(
    file: &File,
    directory: bool,
) -> Result<(), PrivateFsError> {
    verify_flags(file, if directory { 0x13 } else { 0x10 }, false)
}
