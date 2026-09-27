//! Exact point reads from an already authenticated sealed extension tree.

use std::io::{self, Read};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use zephium_extension_package::{CanonicalExtensionTreeIndex, PortableRelativePath};
use zephium_private_fs::{ByteLimit, PrivateEntryName, PrivateFsError, SealedPrivateDirectory};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TreeResourceError {
    NotDeclared,
    Missing,
    Unavailable,
    Mismatch,
    Quarantined,
}

pub(crate) fn with_verified_tree_resource<T, E>(
    root: &Arc<SealedPrivateDirectory>,
    index: &CanonicalExtensionTreeIndex,
    path: &PortableRelativePath,
    callback: impl FnOnce(&mut dyn Read) -> Result<T, E>,
) -> Result<Result<T, E>, TreeResourceError> {
    // This lookup deliberately precedes every private-filesystem operation.
    let expected = index.file(path).ok_or(TreeResourceError::NotDeclared)?;
    let mut components = path.as_str().split('/').peekable();
    let mut directory = root.as_ref();
    let mut opened = Vec::new();
    let final_name = loop {
        let component = components.next().ok_or(TreeResourceError::Mismatch)?;
        let name =
            PrivateEntryName::new(component.to_owned()).map_err(|_| TreeResourceError::Mismatch)?;
        if components.peek().is_none() {
            break name;
        }
        let child = directory
            .open_sealed_entry_child(&name)
            .map_err(map_private_fs)?;
        opened.push(child);
        directory = opened.last().ok_or(TreeResourceError::Mismatch)?;
    };

    let limit = usize::try_from(expected.length())
        .map_err(|_| TreeResourceError::Mismatch)?
        .max(1);
    let nested = directory
        .with_bounded_entry_regular_reader(
            &final_name,
            ByteLimit::new(limit).map_err(map_private_fs)?,
            |reader| {
                let mut verified = HashingReader::new(reader);
                let callback_result = callback(&mut verified);
                let drain_result = io::copy(&mut verified, &mut io::sink());
                let proof = verified.finish();
                if drain_result.is_err()
                    || proof.read_failed
                    || proof.length != expected.length()
                    || proof.sha256 != expected.sha256()
                {
                    return Err(ReadVerificationError);
                }
                Ok(callback_result)
            },
        )
        .map_err(map_private_fs)?
        .ok_or(TreeResourceError::Missing)?;
    nested.map_err(|_| TreeResourceError::Mismatch)
}

struct HashingReader<'reader> {
    inner: &'reader mut dyn Read,
    digest: Sha256,
    length: u64,
    read_failed: bool,
}

impl<'reader> HashingReader<'reader> {
    fn new(inner: &'reader mut dyn Read) -> Self {
        Self {
            inner,
            digest: Sha256::new(),
            length: 0,
            read_failed: false,
        }
    }

    fn finish(self) -> ReadProof {
        ReadProof {
            length: self.length,
            sha256: self.digest.finalize().into(),
            read_failed: self.read_failed,
        }
    }
}

impl Read for HashingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(buffer) {
            Ok(read) => {
                self.length = match self
                    .length
                    .checked_add(u64::try_from(read).unwrap_or(u64::MAX))
                {
                    Some(length) => length,
                    None => {
                        self.read_failed = true;
                        return Err(io::Error::other("bounded resource byte count overflow"));
                    }
                };
                self.digest.update(&buffer[..read]);
                Ok(read)
            }
            Err(error) => {
                self.read_failed = true;
                Err(error)
            }
        }
    }
}

struct ReadProof {
    length: u64,
    sha256: [u8; 32],
    read_failed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReadVerificationError;

fn map_private_fs(error: PrivateFsError) -> TreeResourceError {
    match error {
        PrivateFsError::NotFound => TreeResourceError::Missing,
        PrivateFsError::Quarantined
        | PrivateFsError::IdentityAmbiguous
        | PrivateFsError::SettlementUnknown => TreeResourceError::Quarantined,
        PrivateFsError::BoundExceeded
        | PrivateFsError::Unsafe
        | PrivateFsError::ReservedComponent
        | PrivateFsError::AlreadyExists
        | PrivateFsError::NamespaceMismatch
        | PrivateFsError::DirectoryNotEmpty => TreeResourceError::Mismatch,
        PrivateFsError::Io
        | PrivateFsError::PrimitiveUnavailable
        | PrivateFsError::LockUnavailable
        | PrivateFsError::InUse => TreeResourceError::Unavailable,
    }
}
