//! Minimal macOS ACL probe kept behind a safe crate-private interface.

#![allow(unsafe_code)]

use std::os::fd::RawFd;

type Acl = *mut libc::c_void;

unsafe extern "C" {
    fn acl_get_fd_np(fd: libc::c_int, acl_type: libc::c_int) -> Acl;
    fn acl_free(object: *mut libc::c_void) -> libc::c_int;
}

/// Fail closed when the kernel cannot prove that the open node has no
/// extended ACL. Mode bits alone do not account for macOS ACL entries.
pub(super) fn has_no_extended_acl(fd: RawFd) -> bool {
    const ACL_TYPE_EXTENDED: libc::c_int = 0x0000_0100;
    // SAFETY: the caller borrows a live filesystem descriptor. A non-null ACL
    // is independently owned and released by the guard below.
    let acl = unsafe { acl_get_fd_np(fd, ACL_TYPE_EXTENDED) };
    if acl.is_null() {
        // Darwin reports ENOENT when this live node has no extended ACL. Any
        // other error (including an ACL-unsupported filesystem) fails closed.
        return std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT);
    }
    let _acl = OwnedAcl(acl);
    // Darwin represents absence as the ENOENT case above; a returned extended
    // ACL is therefore a policy-bearing ACL and is rejected in full.
    false
}

/// Resolves the kernel-owned path for one live descriptor and compares only
/// its final component with the authenticated ASCII spelling.
pub(super) fn has_exact_final_component(fd: RawFd, expected: &str) -> Result<bool, ()> {
    use std::ffi::CStr;

    let mut path = [0_i8; libc::PATH_MAX as usize];
    // SAFETY: `fd` is borrowed from a live `File`. `F_GETPATH` writes at most
    // `PATH_MAX` bytes to this correctly sized writable buffer and terminates
    // the result with NUL on success.
    if unsafe { libc::fcntl(fd, libc::F_GETPATH, path.as_mut_ptr()) } == -1 {
        return Err(());
    }
    // SAFETY: successful `F_GETPATH` guarantees a NUL-terminated path inside
    // the fixed-size output buffer above.
    let path = unsafe { CStr::from_ptr(path.as_ptr()) }.to_bytes();
    let actual = path.rsplit(|byte| *byte == b'/').next().unwrap_or(path);
    Ok(actual == expected.as_bytes())
}

struct OwnedAcl(Acl);

impl Drop for OwnedAcl {
    fn drop(&mut self) {
        // SAFETY: acl_get_fd_np transferred ownership of this non-null ACL.
        let _ = unsafe { acl_free(self.0) };
    }
}
