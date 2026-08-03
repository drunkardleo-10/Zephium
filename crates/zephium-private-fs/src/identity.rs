use crate::platform::RawIdentity;
use std::fmt;

/// Opaque identity of one verified open regular file.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct FileIdentity(pub(crate) RawIdentity);

/// Opaque identity of one verified open private directory.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct DirectoryIdentity(pub(crate) RawIdentity);

impl fmt::Debug for FileIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("FileIdentity(..)")
    }
}

impl fmt::Debug for DirectoryIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DirectoryIdentity(..)")
    }
}
