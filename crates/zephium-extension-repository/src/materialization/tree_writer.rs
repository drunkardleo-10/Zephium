//! Exact, path-free construction and verification of sealed extension trees.

use std::collections::BTreeSet;
use std::io::{self, Cursor, Read};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use thiserror::Error;
use zephium_core::extensions::ExtensionTreeDigest;
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, ExtensionTreeFile, PortableRelativePath,
    MAX_EXTENSION_RELATIVE_PATH_DEPTH, MAX_EXTENSION_TREE_ENTRIES,
};
use zephium_private_fs::{
    ByteLimit, DirectoryIdentity, FileIdentity, OpenedPrivateDirectory, PrivateChildKind,
    PrivateComponent, PrivateDirectory, PrivateEntryName, PrivateFsError, PrivateFsTransitionError,
    SealedPrivateDirectory, StreamingFileLength, StreamingWriteError,
};

use super::source::{
    BundledReleaseByteSource, BundledReleasePackageSourceIdentity, BundledReleaseResource,
    BundledReleaseSourceError,
};
use crate::operation::with_external_callback;

/// One sealed, exact tree that still has its create-new stage name.
///
/// The capability is deliberately linear: publication consumes it, and this
/// type does not implement `Clone`. The captured parent identity makes the
/// public operation below enforce the repository's same-parent layout even on
/// platforms whose primitive could move a directory between parents.
pub(crate) struct AuthenticatedTreeStage {
    directory: SealedPrivateDirectory,
    parent_identity: DirectoryIdentity,
    _inventory: VerifiedTreeInventory,
}

impl AuthenticatedTreeStage {
    /// Publishes this exact stage with the platform's true no-replace rename.
    ///
    /// A successful result is a freshly opened capability for the destination
    /// identity. A terminal transition deliberately returns no stale stage
    /// authority and is reported as settlement ambiguity.
    pub(crate) fn publish_same_parent_noreplace(
        self,
        parent: &PrivateDirectory,
        destination: &PrivateComponent,
    ) -> Result<Arc<SealedPrivateDirectory>, TreeWriterError> {
        if parent.identity() != self.parent_identity {
            return Err(map_filesystem(PrivateFsError::NamespaceMismatch));
        }
        self.directory
            .publish_noreplace(parent, destination)
            .map(Arc::new)
            .map_err(map_transition)
    }
}

/// Revalidated exact tree bytes plus the only native root capability that may
/// later be used to construct a runtime lease.
///
/// This proof is internal and non-cloneable. In particular, possessing an
/// `Arc<SealedPrivateDirectory>` alone is not equivalent to this proof: every
/// receipt boundary must call [`verify_sealed_tree`] again against freshly
/// admitted package metadata.
pub(crate) struct AuthenticatedSealedTree {
    root: Arc<SealedPrivateDirectory>,
    tree_sha256: ExtensionTreeDigest,
    file_count: usize,
    total_bytes: u64,
}

impl AuthenticatedSealedTree {
    pub(crate) const fn tree_sha256(&self) -> ExtensionTreeDigest {
        self.tree_sha256
    }

    pub(crate) const fn file_count(&self) -> usize {
        self.file_count
    }

    pub(crate) const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub(crate) fn into_root(self) -> Arc<SealedPrivateDirectory> {
        self.root
    }
}

/// Stable path-free failure at the sealed-tree writer boundary.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum TreeWriterError {
    /// The fixed signed-resource adapter failed its own source boundary.
    #[error("bundled extension tree source failed: {0}")]
    Source(BundledReleaseSourceError),
    /// A private-filesystem operation failed while its namespace remained
    /// usable and every attempted consuming transition stayed pre-commit.
    #[error("bundled extension tree filesystem failed: {0}")]
    Filesystem(PrivateFsError),
    /// Observed bytes, names, kinds, counts, or digests did not equal the
    /// authenticated canonical inventory.
    #[error("bundled extension tree did not match its canonical inventory")]
    ExactMismatch,
    /// A consuming seal, unseal, publication, or removal may have committed,
    /// and no live capability remains from which to continue safely.
    #[error("bundled extension tree transition settlement is ambiguous")]
    TransitionAmbiguous,
}

/// Constructs and bottom-up seals one exact create-new tree stage.
///
/// `manifest_bytes` must be the exact buffer that passed manifest admission;
/// it is written directly and is never reopened from the release source during
/// this construction. Every other file is requested through a complete
/// catalog/package/resource identity, streamed once, hashed while the private
/// filesystem performs exact-length and EOF validation, and bound across its
/// writable-to-sealed identity transition.
pub(crate) fn build_authenticated_tree_stage<S: BundledReleaseByteSource>(
    trees: &PrivateDirectory,
    stage_name: &PrivateComponent,
    package: BundledReleasePackageSourceIdentity,
    index: &CanonicalExtensionTreeIndex,
    manifest_bytes: &[u8],
    source: &mut S,
) -> Result<AuthenticatedTreeStage, TreeWriterError> {
    validate_build_inputs(package, index, manifest_bytes)?;

    let parent_identity = trees.identity();
    let root = trees
        .create_new_private_child(stage_name)
        .map_err(map_filesystem)?;
    let mut directories = vec![root];
    let mut directory_names = Vec::<PrivateEntryName>::new();

    for file in index.files() {
        let components = file.path().as_str().split('/').collect::<Vec<_>>();
        let parent_components = &components[..components.len().saturating_sub(1)];
        let common = directory_names
            .iter()
            .zip(parent_components)
            .take_while(|(current, expected)| current.as_str() == **expected)
            .count();

        while directory_names.len() > common {
            seal_top_directory(&mut directories)?;
            directory_names.pop();
        }
        for component in &parent_components[common..] {
            let name = entry_name(component)?;
            let child = directories
                .last()
                .ok_or(TreeWriterError::ExactMismatch)?
                .create_new_entry_child(&name)
                .map_err(map_filesystem)?;
            directories.push(child);
            directory_names.push(name);
        }

        let file_name = entry_name(
            components
                .last()
                .copied()
                .ok_or(TreeWriterError::ExactMismatch)?,
        )?;
        let parent = directories.last().ok_or(TreeWriterError::ExactMismatch)?;
        let writable_identity = if file.path().as_str() == "manifest.json" {
            write_retained_manifest(parent, &file_name, file, manifest_bytes)?
        } else {
            write_source_file(parent, &file_name, package, file, source)?
        };
        seal_file_with_same_identity(parent, &file_name, writable_identity)?;
    }

    while directories.len() > 1 {
        seal_top_directory(&mut directories)?;
    }
    let root = directories.pop().ok_or(TreeWriterError::ExactMismatch)?;
    let identity = root.identity();
    let sealed = root.seal().map_err(map_transition)?;
    if sealed.identity() != identity {
        return Err(map_filesystem(PrivateFsError::IdentityAmbiguous));
    }
    let inventory = verify_sealed_tree_contents(&sealed, index)?;

    Ok(AuthenticatedTreeStage {
        directory: sealed,
        parent_identity,
        _inventory: inventory,
    })
}

/// Exhaustively re-authenticates one sealed tree from its observed inventory.
///
/// Enumeration leads the traversal: every observed name is admitted, matched
/// to one expected file or implicit directory, and recursively verified. The
/// final exact file, directory, and byte counts also prove that the canonical
/// inventory has no missing entries. No package path is resolved as a host
/// filesystem path.
pub(crate) fn verify_sealed_tree(
    root: &Arc<SealedPrivateDirectory>,
    index: &CanonicalExtensionTreeIndex,
) -> Result<AuthenticatedSealedTree, TreeWriterError> {
    let inventory = verify_sealed_tree_contents(root.as_ref(), index)?;
    Ok(AuthenticatedSealedTree {
        root: Arc::clone(root),
        tree_sha256: inventory.tree_sha256,
        file_count: inventory.file_count,
        total_bytes: inventory.total_bytes,
    })
}

fn verify_sealed_tree_contents(
    root: &SealedPrivateDirectory,
    index: &CanonicalExtensionTreeIndex,
) -> Result<VerifiedTreeInventory, TreeWriterError> {
    let expected_directories = expected_directories(index)?;
    let mut observed = ObservedTree::default();
    verify_sealed_directory(root, "", index, &expected_directories, &mut observed)?;

    if observed.files != index.files().len()
        || observed.directories != index.implicit_directory_count()
        || observed.entries != index.total_entry_count()
        || observed.bytes != index.total_bytes()
    {
        return Err(TreeWriterError::ExactMismatch);
    }

    Ok(VerifiedTreeInventory {
        tree_sha256: index.tree_sha256(),
        file_count: observed.files,
        total_bytes: observed.bytes,
    })
}

/// Removes one optional interrupted tree stage through a bounded, mixed-mode
/// traversal.
///
/// Sealed directories are unsealed top-down; children are admitted in their
/// exact writable or sealed mode, regular files are removed through the held
/// parent, and directories are removed bottom-up. The global observed-entry
/// ceiling prevents hostile residue from turning recovery into unbounded work.
pub(crate) fn cleanup_tree_stage(
    trees: &PrivateDirectory,
    stage_name: &PrivateComponent,
) -> Result<bool, TreeWriterError> {
    let opened = match trees.open_private_child_any_mode(stage_name) {
        Ok(opened) => opened,
        Err(PrivateFsError::NotFound) => return Ok(false),
        Err(error) => return Err(map_filesystem(error)),
    };
    let mut observed_entries = 0_usize;
    clear_and_remove_directory(opened, 0, &mut observed_entries)?;
    Ok(true)
}

fn validate_build_inputs(
    package: BundledReleasePackageSourceIdentity,
    index: &CanonicalExtensionTreeIndex,
    manifest_bytes: &[u8],
) -> Result<(), TreeWriterError> {
    let manifest = index
        .files()
        .iter()
        .find(|file| file.path().as_str() == "manifest.json")
        .ok_or(TreeWriterError::ExactMismatch)?;
    let manifest_length =
        u64::try_from(manifest_bytes.len()).map_err(|_| TreeWriterError::ExactMismatch)?;
    let manifest_digest: [u8; 32] = Sha256::digest(manifest_bytes).into();
    if package.tree_digest() != index.tree_sha256()
        || package.manifest_digest() != index.manifest_sha256()
        || manifest.length() != manifest_length
        || manifest.sha256() != manifest_digest
    {
        return Err(TreeWriterError::ExactMismatch);
    }
    Ok(())
}

fn write_retained_manifest(
    parent: &PrivateDirectory,
    name: &PrivateEntryName,
    file: &ExtensionTreeFile,
    manifest_bytes: &[u8],
) -> Result<FileIdentity, TreeWriterError> {
    let mut reader = Cursor::new(manifest_bytes);
    parent
        .write_new_entry_from_reader(name, &mut reader, streaming_length(file.length())?)
        .map_err(map_streaming_write)
}

fn write_source_file<S: BundledReleaseByteSource>(
    parent: &PrivateDirectory,
    name: &PrivateEntryName,
    package: BundledReleasePackageSourceIdentity,
    file: &ExtensionTreeFile,
    source: &mut S,
) -> Result<FileIdentity, TreeWriterError> {
    let resource =
        BundledReleaseResource::tree_file(package, file.path(), file.length(), file.sha256());
    let nested = with_external_callback(|| {
        source.with_resource(resource, |reader| {
            let mut reader = DigestingReader::new(reader);
            let identity = parent
                .write_new_entry_from_reader(name, &mut reader, streaming_length(file.length())?)
                .map_err(map_streaming_write)?;
            let (length, digest) = reader.finish();
            if length != file.length() || digest != file.sha256() {
                return Err(TreeWriterError::ExactMismatch);
            }
            Ok(identity)
        })
    })
    .map_err(TreeWriterError::Source)?;
    nested
}

fn seal_file_with_same_identity(
    parent: &PrivateDirectory,
    name: &PrivateEntryName,
    writable_identity: FileIdentity,
) -> Result<(), TreeWriterError> {
    let sealed_identity = parent
        .seal_verified_entry_regular(name)
        .map_err(map_filesystem)?
        .ok_or(TreeWriterError::ExactMismatch)?;
    if sealed_identity != writable_identity {
        return Err(map_filesystem(PrivateFsError::IdentityAmbiguous));
    }
    Ok(())
}

fn seal_top_directory(directories: &mut Vec<PrivateDirectory>) -> Result<(), TreeWriterError> {
    let directory = directories.pop().ok_or(TreeWriterError::ExactMismatch)?;
    let identity = directory.identity();
    let sealed = directory.seal().map_err(map_transition)?;
    if sealed.identity() != identity {
        return Err(map_filesystem(PrivateFsError::IdentityAmbiguous));
    }
    Ok(())
}

fn expected_directories(
    index: &CanonicalExtensionTreeIndex,
) -> Result<BTreeSet<Box<str>>, TreeWriterError> {
    let mut directories = BTreeSet::new();
    for file in index.files() {
        for (offset, byte) in file.path().as_str().bytes().enumerate() {
            if byte == b'/' {
                directories.insert(file.path().as_str()[..offset].into());
            }
        }
    }
    if directories.len() != index.implicit_directory_count() {
        return Err(TreeWriterError::ExactMismatch);
    }
    Ok(directories)
}

#[derive(Default)]
struct ObservedTree {
    entries: usize,
    files: usize,
    directories: usize,
    bytes: u64,
}

struct VerifiedTreeInventory {
    tree_sha256: ExtensionTreeDigest,
    file_count: usize,
    total_bytes: u64,
}

fn verify_sealed_directory(
    directory: &SealedPrivateDirectory,
    prefix: &str,
    index: &CanonicalExtensionTreeIndex,
    expected_directories: &BTreeSet<Box<str>>,
    observed: &mut ObservedTree,
) -> Result<(), TreeWriterError> {
    let names = directory
        .list_entry_names(MAX_EXTENSION_TREE_ENTRIES)
        .map_err(map_filesystem)?;
    for name in names {
        observed.entries = observed
            .entries
            .checked_add(1)
            .ok_or(TreeWriterError::ExactMismatch)?;
        if observed.entries > MAX_EXTENSION_TREE_ENTRIES {
            return Err(TreeWriterError::ExactMismatch);
        }

        let path = logical_child_path(prefix, &name)?;
        let expected_file = index.file(&path);
        let expected_directory = expected_directories.contains(path.as_str());
        let kind = directory
            .inspect_entry(&name)
            .map_err(map_filesystem)?
            .ok_or(TreeWriterError::ExactMismatch)?;
        match (kind, expected_file, expected_directory) {
            (PrivateChildKind::RegularFile(_), Some(file), false) => {
                verify_sealed_file(directory, &name, file)?;
                observed.files = observed
                    .files
                    .checked_add(1)
                    .ok_or(TreeWriterError::ExactMismatch)?;
                observed.bytes = observed
                    .bytes
                    .checked_add(file.length())
                    .ok_or(TreeWriterError::ExactMismatch)?;
            }
            (PrivateChildKind::Directory(identity), None, true) => {
                let child = directory
                    .open_sealed_entry_child(&name)
                    .map_err(map_filesystem)?;
                if child.identity() != identity {
                    return Err(map_filesystem(PrivateFsError::IdentityAmbiguous));
                }
                observed.directories = observed
                    .directories
                    .checked_add(1)
                    .ok_or(TreeWriterError::ExactMismatch)?;
                verify_sealed_directory(
                    &child,
                    path.as_str(),
                    index,
                    expected_directories,
                    observed,
                )?;
            }
            _ => return Err(TreeWriterError::ExactMismatch),
        }
    }
    Ok(())
}

fn verify_sealed_file(
    directory: &SealedPrivateDirectory,
    name: &PrivateEntryName,
    expected: &ExtensionTreeFile,
) -> Result<(), TreeWriterError> {
    let limit_bytes = usize::try_from(expected.length())
        .map_err(|_| TreeWriterError::ExactMismatch)?
        .max(1);
    let result = directory
        .with_bounded_entry_regular_reader(
            name,
            ByteLimit::new(limit_bytes).map_err(map_filesystem)?,
            digest_reader,
        )
        .map_err(|error| {
            if error == PrivateFsError::BoundExceeded {
                TreeWriterError::ExactMismatch
            } else {
                map_filesystem(error)
            }
        })?
        .ok_or(TreeWriterError::ExactMismatch)?
        .map_err(|_| map_filesystem(PrivateFsError::Io))?;
    if result.0 != expected.length() || result.1 != expected.sha256() {
        return Err(TreeWriterError::ExactMismatch);
    }
    Ok(())
}

fn digest_reader(reader: &mut dyn Read) -> io::Result<(u64, [u8; 32])> {
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(u64::try_from(read).unwrap_or(u64::MAX))
            .ok_or_else(|| io::Error::other("bounded tree-file byte count overflow"))?;
        digest.update(&buffer[..read]);
    }
    Ok((bytes, digest.finalize().into()))
}

fn clear_and_remove_directory(
    opened: OpenedPrivateDirectory,
    depth: usize,
    observed_entries: &mut usize,
) -> Result<(), TreeWriterError> {
    let directory = match opened {
        OpenedPrivateDirectory::Writable(directory) => directory,
        OpenedPrivateDirectory::Sealed(directory) => directory.unseal().map_err(map_transition)?,
    };
    let names = directory
        .list_entry_names(MAX_EXTENSION_TREE_ENTRIES)
        .map_err(map_filesystem)?;
    for name in names {
        let child_depth = depth.checked_add(1).ok_or(TreeWriterError::ExactMismatch)?;
        if child_depth > MAX_EXTENSION_RELATIVE_PATH_DEPTH {
            return Err(TreeWriterError::ExactMismatch);
        }
        *observed_entries = observed_entries
            .checked_add(1)
            .ok_or(TreeWriterError::ExactMismatch)?;
        if *observed_entries > MAX_EXTENSION_TREE_ENTRIES {
            return Err(TreeWriterError::ExactMismatch);
        }
        let kind = directory
            .inspect_entry(&name)
            .map_err(map_filesystem)?
            .ok_or(TreeWriterError::ExactMismatch)?;
        match kind {
            PrivateChildKind::RegularFile(_) => {
                if !directory
                    .remove_verified_entry_regular(&name)
                    .map_err(map_filesystem)?
                {
                    return Err(TreeWriterError::ExactMismatch);
                }
            }
            PrivateChildKind::Directory(identity) => {
                let child = directory
                    .open_entry_child_any_mode(&name)
                    .map_err(map_filesystem)?;
                if child.identity() != identity {
                    return Err(map_filesystem(PrivateFsError::IdentityAmbiguous));
                }
                clear_and_remove_directory(child, child_depth, observed_entries)?;
            }
        }
    }
    directory.remove_empty().map_err(map_transition)
}

fn logical_child_path(
    prefix: &str,
    name: &PrivateEntryName,
) -> Result<PortableRelativePath, TreeWriterError> {
    let mut path =
        String::with_capacity(prefix.len() + usize::from(!prefix.is_empty()) + name.as_str().len());
    if !prefix.is_empty() {
        path.push_str(prefix);
        path.push('/');
    }
    path.push_str(name.as_str());
    PortableRelativePath::parse(&path).map_err(|_| TreeWriterError::ExactMismatch)
}

fn entry_name(value: &str) -> Result<PrivateEntryName, TreeWriterError> {
    PrivateEntryName::new(value).map_err(|_| TreeWriterError::ExactMismatch)
}

fn streaming_length(length: u64) -> Result<StreamingFileLength, TreeWriterError> {
    StreamingFileLength::new(length).map_err(map_filesystem)
}

fn map_streaming_write(error: StreamingWriteError) -> TreeWriterError {
    match error {
        StreamingWriteError::Filesystem(error) => map_filesystem(error),
        StreamingWriteError::SourceRead => TreeWriterError::Source(BundledReleaseSourceError::Io),
        StreamingWriteError::SourceTooShort
        | StreamingWriteError::SourceTooLong
        | StreamingWriteError::SinkLengthMismatch => TreeWriterError::ExactMismatch,
    }
}

fn map_transition<S>(error: PrivateFsTransitionError<S>) -> TreeWriterError {
    let (error, state) = error.into_parts();
    if state.is_some() && !is_terminal_filesystem(error) {
        TreeWriterError::Filesystem(error)
    } else {
        TreeWriterError::TransitionAmbiguous
    }
}

fn map_filesystem(error: PrivateFsError) -> TreeWriterError {
    if is_terminal_filesystem(error) {
        TreeWriterError::TransitionAmbiguous
    } else {
        TreeWriterError::Filesystem(error)
    }
}

const fn is_terminal_filesystem(error: PrivateFsError) -> bool {
    matches!(
        error,
        PrivateFsError::IdentityAmbiguous
            | PrivateFsError::SettlementUnknown
            | PrivateFsError::Quarantined
    )
}

struct DigestingReader<'reader> {
    inner: &'reader mut dyn Read,
    digest: Sha256,
    bytes: u64,
}

impl<'reader> DigestingReader<'reader> {
    fn new(inner: &'reader mut dyn Read) -> Self {
        Self {
            inner,
            digest: Sha256::new(),
            bytes: 0,
        }
    }

    fn finish(self) -> (u64, [u8; 32]) {
        (self.bytes, self.digest.finalize().into())
    }
}

impl Read for DigestingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        if read > buffer.len() {
            return Err(io::Error::other("bundled source over-reported a read"));
        }
        self.bytes = self
            .bytes
            .checked_add(u64::try_from(read).unwrap_or(u64::MAX))
            .ok_or_else(|| io::Error::other("bundled source byte count overflow"))?;
        self.digest.update(&buffer[..read]);
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digesting_reader_binds_all_reads_including_an_exact_empty_source() {
        for bytes in [&b""[..], &b"payload"[..]] {
            let mut source = Cursor::new(bytes);
            let mut reader = DigestingReader::new(&mut source);
            let mut observed = Vec::new();
            reader.read_to_end(&mut observed).unwrap();
            let (length, digest) = reader.finish();
            assert_eq!(observed, bytes);
            assert_eq!(length, bytes.len() as u64);
            assert_eq!(digest, <[u8; 32]>::from(Sha256::digest(bytes)));
        }
    }

    #[test]
    fn transition_mapping_never_projects_a_terminal_capability() {
        let terminal = PrivateFsTransitionError::<()>::Terminal {
            error: PrivateFsError::SettlementUnknown,
        };
        assert_eq!(
            map_transition(terminal),
            TreeWriterError::TransitionAmbiguous
        );
        assert_eq!(
            map_filesystem(PrivateFsError::Quarantined),
            TreeWriterError::TransitionAmbiguous
        );
        assert_eq!(
            map_filesystem(PrivateFsError::Io),
            TreeWriterError::Filesystem(PrivateFsError::Io)
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    mod native {
        use std::collections::BTreeMap;
        use std::fs;

        use zephium_core::extensions::{
            ExtensionAuthorityId, ExtensionManifestDigest, ExtensionPackageIdentity,
            ExtensionPackageKey, ExtensionPackagePayloadIdentity, ExtensionPackageRevision,
        };
        use zephium_extension_authority::{
            BundledCatalogGenerationAnchor, BundledCatalogInventoryDigest,
        };
        use zephium_extension_package::{
            ExtensionReleaseCatalogDigest, ExtensionReleaseCatalogRevision,
        };
        use zephium_private_fs::{ByteLimit, LockedPrivateNamespace};

        use super::*;
        use crate::materialization::source::{
            BundledReleaseCatalogSourceIdentity, BundledReleaseResourceKind,
        };

        const MANIFEST: &[u8] =
            br#"{"manifest_version":3,"name":"Tree Writer Fixture","version":"1.0"}"#;
        const ICON: &[u8] = b"icon bytes";
        const SCRIPT: &[u8] = b"globalThis.zephiumFixture = true;";

        struct FixtureTree {
            index: CanonicalExtensionTreeIndex,
            manifest: Box<[u8]>,
            resources: BTreeMap<Box<str>, Box<[u8]>>,
            package: BundledReleasePackageSourceIdentity,
        }

        impl FixtureTree {
            fn standard() -> Self {
                Self::from_entries(&[
                    ("assets/icons/icon.txt", ICON),
                    ("manifest.json", MANIFEST),
                    ("scripts/content.js", SCRIPT),
                ])
            }

            fn from_entries(entries: &[(&str, &[u8])]) -> Self {
                let mut index_json = String::from("{\"schema_version\":1,\"files\":[");
                let mut resources = BTreeMap::new();
                for (position, (path, bytes)) in entries.iter().enumerate() {
                    if position != 0 {
                        index_json.push(',');
                    }
                    index_json.push_str(&format!(
                        "{{\"path\":\"{path}\",\"length\":{},\"sha256\":\"{}\"}}",
                        bytes.len(),
                        lower_hex(Sha256::digest(bytes).into())
                    ));
                    resources.insert((*path).into(), Box::<[u8]>::from(*bytes));
                }
                index_json.push_str("]}");
                let index = CanonicalExtensionTreeIndex::parse_canonical(index_json.as_bytes())
                    .expect("fixture tree index is canonical");
                let authority = ExtensionAuthorityId::from_bytes([1; 32]);
                let catalog = BundledReleaseCatalogSourceIdentity::from_generation(
                    BundledCatalogGenerationAnchor::from_parts(
                        authority,
                        ExtensionReleaseCatalogRevision::INITIAL,
                        123,
                        ExtensionReleaseCatalogDigest::from_bytes([2; 32]),
                        BundledCatalogInventoryDigest::from_bytes([3; 32]),
                    ),
                )
                .unwrap();
                let identity = ExtensionPackageIdentity::new(
                    authority,
                    ExtensionPackageKey::from_bytes([4; 32]),
                    ExtensionPackageRevision::INITIAL,
                    ExtensionPackagePayloadIdentity::BundledTree,
                    ExtensionManifestDigest::from_bytes(index.manifest_sha256().bytes()),
                    index.tree_sha256(),
                );
                let package =
                    BundledReleasePackageSourceIdentity::from_package(catalog, &identity, [5; 32])
                        .unwrap();
                Self {
                    index,
                    manifest: Box::from(MANIFEST),
                    resources,
                    package,
                }
            }

            fn source(&self) -> FixtureSource {
                FixtureSource {
                    package: self.package,
                    resources: self.resources.clone(),
                    post_callback_error: None,
                    callback_count: 0,
                }
            }
        }

        struct FixtureSource {
            package: BundledReleasePackageSourceIdentity,
            resources: BTreeMap<Box<str>, Box<[u8]>>,
            post_callback_error: Option<BundledReleaseSourceError>,
            callback_count: usize,
        }

        impl BundledReleaseByteSource for FixtureSource {
            fn with_resource<T, E, F>(
                &mut self,
                resource: BundledReleaseResource<'_>,
                callback: F,
            ) -> Result<Result<T, E>, BundledReleaseSourceError>
            where
                F: FnOnce(&mut dyn Read) -> Result<T, E>,
            {
                if resource.package() != self.package {
                    return Err(BundledReleaseSourceError::Unsafe);
                }
                let target = match resource.kind() {
                    BundledReleaseResourceKind::TreeFile { target, .. } => target,
                    _ => return Err(BundledReleaseSourceError::UnsupportedResource),
                };
                let bytes = self
                    .resources
                    .get(target.as_str())
                    .ok_or(BundledReleaseSourceError::Missing)?;
                self.callback_count += 1;
                let mut reader = Cursor::new(bytes.as_ref());
                let result = callback(&mut reader);
                match self.post_callback_error {
                    Some(error) => Err(error),
                    None => Ok(result),
                }
            }
        }

        fn lower_hex(bytes: [u8; 32]) -> String {
            use std::fmt::Write as _;

            let mut encoded = String::with_capacity(64);
            for byte in bytes {
                write!(encoded, "{byte:02x}").unwrap();
            }
            encoded
        }

        fn private_trees() -> (tempfile::TempDir, LockedPrivateNamespace, PrivateDirectory) {
            let parent = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let namespace =
                LockedPrivateNamespace::open_or_create(parent.path().join("repository")).unwrap();
            let trees = namespace
                .directory()
                .create_new_private_child(&PrivateComponent::new("trees").unwrap())
                .unwrap();
            (parent, namespace, trees)
        }

        #[test]
        fn nested_stage_is_verified_before_publish_and_freshly_verified_after_publish() {
            let (_parent, _namespace, trees) = private_trees();
            let fixture = FixtureTree::standard();
            let mut source = fixture.source();
            let stage_name = PrivateComponent::new("tree.stage").unwrap();
            let object_name = PrivateComponent::new("tree.object").unwrap();

            let stage = build_authenticated_tree_stage(
                &trees,
                &stage_name,
                fixture.package,
                &fixture.index,
                &fixture.manifest,
                &mut source,
            )
            .unwrap();
            assert_eq!(stage._inventory.file_count, fixture.index.files().len());
            assert_eq!(stage._inventory.total_bytes, fixture.index.total_bytes());
            assert_eq!(stage._inventory.tree_sha256, fixture.index.tree_sha256());
            assert_eq!(source.callback_count, 2);
            verify_sealed_tree_contents(&stage.directory, &fixture.index).unwrap();

            let published = stage
                .publish_same_parent_noreplace(&trees, &object_name)
                .unwrap();
            assert_eq!(
                trees.open_private_child_any_mode(&stage_name).err(),
                Some(PrivateFsError::NotFound)
            );
            let proof = verify_sealed_tree(&published, &fixture.index).unwrap();
            assert!(Arc::ptr_eq(&proof.root, &published));
            assert_eq!(proof.tree_sha256, fixture.index.tree_sha256());
            assert_eq!(proof.file_count, fixture.index.files().len());
            assert_eq!(proof.total_bytes, fixture.index.total_bytes());

            let different = FixtureTree::from_entries(&[
                ("assets/icons/icon.txt", ICON),
                ("manifest.json", MANIFEST),
                ("scripts/content.js", b"x"),
            ]);
            assert!(matches!(
                verify_sealed_tree(&published, &different.index),
                Err(TreeWriterError::ExactMismatch)
            ));
            drop(proof);
            drop(published);
            assert!(cleanup_tree_stage(&trees, &object_name).unwrap());
        }

        #[test]
        fn exact_input_mismatches_fail_before_or_inside_the_stage_and_are_cleanable() {
            let (_parent, _namespace, trees) = private_trees();
            let fixture = FixtureTree::standard();
            let stage_name = PrivateComponent::new("mismatch.stage").unwrap();
            let mut source = fixture.source();
            assert!(matches!(
                build_authenticated_tree_stage(
                    &trees,
                    &stage_name,
                    fixture.package,
                    &fixture.index,
                    b"wrong manifest",
                    &mut source,
                ),
                Err(TreeWriterError::ExactMismatch)
            ));
            assert_eq!(source.callback_count, 0);
            assert_eq!(
                trees.open_private_child_any_mode(&stage_name).err(),
                Some(PrivateFsError::NotFound)
            );

            let mut source = fixture.source();
            source.resources.insert(
                "assets/icons/icon.txt".into(),
                Box::from(&b"ICON BYTES"[..]),
            );
            assert!(matches!(
                build_authenticated_tree_stage(
                    &trees,
                    &stage_name,
                    fixture.package,
                    &fixture.index,
                    &fixture.manifest,
                    &mut source,
                ),
                Err(TreeWriterError::ExactMismatch)
            ));
            assert!(cleanup_tree_stage(&trees, &stage_name).unwrap());
            assert!(!cleanup_tree_stage(&trees, &stage_name).unwrap());
        }

        #[test]
        fn post_callback_source_failure_leaves_only_bounded_cleanable_residue() {
            let (_parent, _namespace, trees) = private_trees();
            let fixture = FixtureTree::standard();
            let stage_name = PrivateComponent::new("source-failure.stage").unwrap();
            let mut source = fixture.source();
            source.post_callback_error = Some(BundledReleaseSourceError::IdentityAmbiguous);

            assert!(matches!(
                build_authenticated_tree_stage(
                    &trees,
                    &stage_name,
                    fixture.package,
                    &fixture.index,
                    &fixture.manifest,
                    &mut source,
                ),
                Err(TreeWriterError::Source(
                    BundledReleaseSourceError::IdentityAmbiguous
                ))
            ));
            assert_eq!(source.callback_count, 1);
            assert!(cleanup_tree_stage(&trees, &stage_name).unwrap());
            assert!(!cleanup_tree_stage(&trees, &stage_name).unwrap());
        }

        #[test]
        fn cleanup_accepts_a_bounded_mixture_of_writable_and_sealed_nodes() {
            let (_parent, _namespace, trees) = private_trees();
            let stage_name = PrivateComponent::new("mixed.stage").unwrap();
            let stage = trees.create_new_private_child(&stage_name).unwrap();
            let nested_name = PrivateEntryName::new("Nested").unwrap();
            let nested = stage.create_new_entry_child(&nested_name).unwrap();
            let sealed_file = PrivateEntryName::new("sealed.txt").unwrap();
            nested
                .write_new_entry_synced(&sealed_file, b"sealed", ByteLimit::new(16).unwrap())
                .unwrap();
            nested
                .seal_verified_entry_regular(&sealed_file)
                .unwrap()
                .unwrap();
            drop(nested.seal().unwrap());
            stage
                .write_new_entry_synced(
                    &PrivateEntryName::new("writable.tmp").unwrap(),
                    b"writable",
                    ByteLimit::new(16).unwrap(),
                )
                .unwrap();
            drop(stage);

            assert!(cleanup_tree_stage(&trees, &stage_name).unwrap());
            assert!(!cleanup_tree_stage(&trees, &stage_name).unwrap());
        }

        #[test]
        fn cleanup_rejects_nesting_beyond_the_portable_package_depth() {
            let (_parent, _namespace, trees) = private_trees();
            let stage_name = PrivateComponent::new("deep.stage").unwrap();
            let root = trees.create_new_private_child(&stage_name).unwrap();
            let mut directories = vec![root];
            for depth in 0..=MAX_EXTENSION_RELATIVE_PATH_DEPTH {
                let name = PrivateEntryName::new(format!("d{depth:02}")).unwrap();
                let child = directories
                    .last()
                    .unwrap()
                    .create_new_entry_child(&name)
                    .unwrap();
                directories.push(child);
            }
            assert_eq!(
                cleanup_tree_stage(&trees, &stage_name),
                Err(TreeWriterError::ExactMismatch)
            );
            while let Some(directory) = directories.pop() {
                directory.remove_empty().unwrap();
            }
        }
    }
}
