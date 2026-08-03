#[cfg(target_os = "macos")]
mod macos_acl;
#[cfg(unix)]
mod unix;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DirectoryMode {
    Writable,
    Sealed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RegularMode {
    Writable,
    Sealed,
}

#[cfg(unix)]
pub(crate) use unix::*;
#[cfg(target_os = "windows")]
pub(crate) use windows::*;

#[cfg(not(any(unix, target_os = "windows")))]
compile_error!("zephium-private-fs supports only Unix and Windows targets");
