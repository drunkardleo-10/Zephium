use std::fs::{self, File};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, MutexGuard};

use crate::identity::{DirectoryIdentity, FileIdentity};
use crate::lease::NamespaceLease;
use crate::platform::{self, OpenPurpose};
use crate::streaming::copy_exact;
#[cfg(test)]
use crate::streaming::ExactCopyError;
use crate::{
    PrivateComponent, PrivateEntryName, PrivateFsError, StreamingFileLength, StreamingWriteError,
    MAX_IN_MEMORY_FILE_BYTES,
};

const MAX_INVENTORY_ENTRIES: usize = 4_096;
const LOCK_COMPONENT_NAME: &str = ".zephium-private-fs-lock-v1";
const LOCK_STAGING_COMPONENT_NAME: &str = ".zephium-private-fs-lock-staging-v1";
const LOCK_FILE_CONTENT: &[u8] = b"zephium-private-fs\nlock-format=1\n";

/// Validated nonzero bound for a single in-memory file operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteLimit(usize);

impl ByteLimit {
    /// Creates a limit within the crate-wide hard allocation ceiling.
    pub fn new(max_bytes: usize) -> Result<Self, PrivateFsError> {
        if max_bytes == 0 || max_bytes > MAX_IN_MEMORY_FILE_BYTES {
            return Err(PrivateFsError::BoundExceeded);
        }
        Ok(Self(max_bytes))
    }

    /// Returns the exact accepted byte count.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

struct DirectoryCore {
    path: PathBuf,
    handle: File,
    identity: DirectoryIdentity,
    child_name: Option<OwnedChildName>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DirectoryRole {
    Root,
    Child,
}

#[derive(Clone, Copy)]
enum ChildName<'a> {
    Control(&'a PrivateComponent),
    Entry(&'a PrivateEntryName),
}

impl<'a> ChildName<'a> {
    fn as_str(self) -> &'a str {
        match self {
            Self::Control(component) => component.as_str(),
            Self::Entry(name) => name.as_str(),
        }
    }

    fn to_owned_name(self) -> OwnedChildName {
        match self {
            Self::Control(component) => OwnedChildName::Control(component.clone()),
            Self::Entry(name) => OwnedChildName::Entry(name.clone()),
        }
    }
}

#[derive(Clone)]
enum OwnedChildName {
    Control(PrivateComponent),
    Entry(PrivateEntryName),
}

impl OwnedChildName {
    fn as_str(&self) -> &str {
        match self {
            Self::Control(component) => component.as_str(),
            Self::Entry(name) => name.as_str(),
        }
    }
}

#[derive(Clone, Copy)]
enum ChildDirectoryMode {
    OpenExisting,
    CreateOrOpen,
    CreateNew,
}

/// Exact kind and opaque identity of one inspected private child entry.
///
/// This is an observation, not an open file or directory capability. The
/// parent lease remains responsible for serializing later operations, and a
/// caller must not treat an identity as proof that the entry is unchanged
/// after this method returns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateChildKind {
    /// A verified single-link private regular file.
    RegularFile(FileIdentity),
    /// A verified private directory.
    Directory(DirectoryIdentity),
}

/// Held, identity-bound private directory.
///
/// Caller-provided paths never enter this API after admission. On supported
/// platforms, every child operation is resolved relative to the held directory
/// descriptor. Protocol internals accept only [`PrivateComponent`], while
/// authenticated payload entries accept only [`PrivateEntryName`]; no raw path
/// strings cross the public boundary. Every directory also retains the root
/// namespace lease, lock, quarantine state, and operation mutex, so a child
/// cannot outlive or bypass its authority.
pub struct PrivateDirectory {
    core: DirectoryCore,
    lease: Arc<NamespaceLease>,
    role: DirectoryRole,
}

impl PrivateDirectory {
    /// Returns this directory's opaque open-handle identity.
    #[must_use]
    pub const fn identity(&self) -> DirectoryIdentity {
        self.core.identity
    }

    /// Creates or admits one private child directory.
    ///
    /// A newly created entry is settled only after the parent directory is
    /// flushed and both parent and child identities are revalidated.
    pub fn create_private_child(
        &self,
        component: &PrivateComponent,
    ) -> Result<Self, PrivateFsError> {
        self.child_directory(
            ChildName::Control(component),
            ChildDirectoryMode::CreateOrOpen,
        )
    }

    /// Opens one existing private protocol child directory without creating it.
    ///
    /// Absence returns [`PrivateFsError::NotFound`]. Existing hostile node
    /// kinds, permissions, ACLs, links, and identity races fail closed.
    pub fn open_private_child(&self, component: &PrivateComponent) -> Result<Self, PrivateFsError> {
        self.child_directory(
            ChildName::Control(component),
            ChildDirectoryMode::OpenExisting,
        )
    }

    /// Creates one new private protocol child directory.
    ///
    /// [`PrivateFsError::AlreadyExists`] is returned only after an existing
    /// directory under that name has itself passed private-directory admission.
    /// An existing hostile node is [`PrivateFsError::Unsafe`].
    pub fn create_new_private_child(
        &self,
        component: &PrivateComponent,
    ) -> Result<Self, PrivateFsError> {
        self.child_directory(ChildName::Control(component), ChildDirectoryMode::CreateNew)
    }

    /// Creates or admits one case-preserving payload child directory.
    pub fn create_entry_child(&self, name: &PrivateEntryName) -> Result<Self, PrivateFsError> {
        self.child_directory(ChildName::Entry(name), ChildDirectoryMode::CreateOrOpen)
    }

    /// Opens one existing case-preserving payload child without creating it.
    ///
    /// Absence returns [`PrivateFsError::NotFound`].
    pub fn open_entry_child(&self, name: &PrivateEntryName) -> Result<Self, PrivateFsError> {
        self.child_directory(ChildName::Entry(name), ChildDirectoryMode::OpenExisting)
    }

    /// Creates one new case-preserving payload child directory.
    ///
    /// [`PrivateFsError::AlreadyExists`] is returned only for a safely admitted
    /// existing directory. An existing file, link, special node, unsafe mode,
    /// or unsafe ACL is [`PrivateFsError::Unsafe`].
    pub fn create_new_entry_child(&self, name: &PrivateEntryName) -> Result<Self, PrivateFsError> {
        self.child_directory(ChildName::Entry(name), ChildDirectoryMode::CreateNew)
    }

    fn child_directory(
        &self,
        name: ChildName<'_>,
        mode: ChildDirectoryMode,
    ) -> Result<Self, PrivateFsError> {
        let _operation = self.begin_operation()?;
        self.reject_reserved_name(name)?;
        self.precheck_unlocked()?;
        if matches!(mode, ChildDirectoryMode::OpenExisting) {
            let child = self.lease.observe(self.open_child_unlocked(name))?;
            self.precheck_unlocked()?;
            return Ok(child);
        }

        let created = match self.create_directory_unlocked(name) {
            Ok(created) => created,
            Err(PrivateFsError::SettlementUnknown) => {
                return self
                    .lease
                    .settle_after_commit(Err(PrivateFsError::SettlementUnknown));
            }
            Err(error) => {
                self.precheck_unlocked()?;
                return Err(error);
            }
        };
        if created {
            let settlement = (|| {
                platform::sync_directory(&self.core.handle)?;
                let child = self.open_child_unlocked(name)?;
                self.verify_boundary_unlocked()?;
                Ok(child)
            })();
            return self.lease.settle_after_commit(settlement);
        }

        let observed = match self.open_child_unlocked(name) {
            Err(PrivateFsError::NotFound) => Err(PrivateFsError::IdentityAmbiguous),
            outcome => outcome,
        };
        let child = self.lease.observe(observed)?;
        self.precheck_unlocked()?;
        if matches!(mode, ChildDirectoryMode::CreateNew) {
            Err(PrivateFsError::AlreadyExists)
        } else {
            Ok(child)
        }
    }

    /// Lists a bounded, sorted snapshot of portable direct-child names.
    ///
    /// Node kinds are intentionally not trusted here. Callers must admit any
    /// named file through the regular-file methods before using it. The root
    /// namespace lock internals are never projected into caller inventory.
    pub fn list_components(
        &self,
        max_entries: usize,
    ) -> Result<Vec<PrivateComponent>, PrivateFsError> {
        let _operation = self.begin_operation()?;
        if max_entries == 0 || max_entries > MAX_INVENTORY_ENTRIES {
            return Err(PrivateFsError::BoundExceeded);
        }
        self.precheck_unlocked()?;
        let mut entries = platform::list_names(
            &self.core.handle,
            &self.core.path,
            max_entries.saturating_add(if self.role == DirectoryRole::Root {
                2
            } else {
                0
            }),
        )?
        .into_iter()
        .map(|name| PrivateComponent::new(name).map_err(|_| PrivateFsError::Unsafe))
        .collect::<Result<Vec<_>, _>>()?;
        if self.role == DirectoryRole::Root {
            if entries
                .iter()
                .any(|entry| self.lease.is_reserved_lock_staging_name(entry.as_str()))
            {
                // Staging is required to be absent when activation settles.
                // Never hide residue that appeared after that admission point.
                return Err(PrivateFsError::Unsafe);
            }
            if entries.iter().any(|entry| {
                self.lease.is_reserved_lock_name(entry.as_str())
                    && !self.lease.is_canonical_reserved_lock_name(entry.as_str())
            }) {
                return Err(PrivateFsError::Unsafe);
            }
            entries.retain(|entry| !self.lease.is_canonical_reserved_lock_name(entry.as_str()));
        }
        if entries.len() > max_entries {
            return Err(PrivateFsError::BoundExceeded);
        }
        entries.sort_unstable();
        self.precheck_unlocked()?;
        Ok(entries)
    }

    /// Lists a bounded, byte-sorted snapshot of direct payload entry names.
    ///
    /// Exact spelling is preserved. Node kinds are not trusted by enumeration;
    /// callers use [`Self::inspect_entry`] or a typed open/read method before
    /// consuming an entry. At the root, the exact canonical lock is hidden,
    /// while staging residue or a differently cased lock alias fails closed.
    pub fn list_entry_names(
        &self,
        max_entries: usize,
    ) -> Result<Vec<PrivateEntryName>, PrivateFsError> {
        let _operation = self.begin_operation()?;
        if max_entries == 0 || max_entries > MAX_INVENTORY_ENTRIES {
            return Err(PrivateFsError::BoundExceeded);
        }
        self.precheck_unlocked()?;
        let mut entries = platform::list_names(
            &self.core.handle,
            &self.core.path,
            max_entries.saturating_add(if self.role == DirectoryRole::Root {
                2
            } else {
                0
            }),
        )?
        .into_iter()
        .map(|name| PrivateEntryName::new(name).map_err(|_| PrivateFsError::Unsafe))
        .collect::<Result<Vec<_>, _>>()?;
        if self.role == DirectoryRole::Root {
            if entries
                .iter()
                .any(|entry| self.lease.is_reserved_lock_staging_name(entry.as_str()))
            {
                return Err(PrivateFsError::Unsafe);
            }
            if entries.iter().any(|entry| {
                self.lease.is_reserved_lock_name(entry.as_str())
                    && !self.lease.is_canonical_reserved_lock_name(entry.as_str())
            }) {
                return Err(PrivateFsError::Unsafe);
            }
            entries.retain(|entry| !self.lease.is_canonical_reserved_lock_name(entry.as_str()));
        }
        if entries.len() > max_entries {
            return Err(PrivateFsError::BoundExceeded);
        }
        entries.sort_unstable();
        self.precheck_unlocked()?;
        Ok(entries)
    }

    /// Returns whether a verified regular child exists.
    ///
    /// A symlink, directory, hard link, reparse point, unsafe permission, or
    /// reserved root-lock reference is an error rather than an affirmative
    /// result.
    pub fn regular_exists(&self, component: &PrivateComponent) -> Result<bool, PrivateFsError> {
        self.regular_exists_named(ChildName::Control(component))
    }

    /// Returns whether a verified case-preserving regular payload entry exists.
    ///
    /// Absence is `false`; every present non-regular or unsafe node is an error.
    pub fn entry_regular_exists(&self, name: &PrivateEntryName) -> Result<bool, PrivateFsError> {
        self.regular_exists_named(ChildName::Entry(name))
    }

    fn regular_exists_named(&self, name: ChildName<'_>) -> Result<bool, PrivateFsError> {
        let _operation = self.begin_operation()?;
        self.reject_reserved_name(name)?;
        self.precheck_unlocked()?;
        let exists = self.open_optional_regular_unlocked(name, OpenPurpose::Read)?;
        self.precheck_unlocked()?;
        Ok(exists.is_some())
    }

    /// Returns the opaque identity of an optional verified regular child.
    pub fn regular_identity(
        &self,
        component: &PrivateComponent,
    ) -> Result<Option<FileIdentity>, PrivateFsError> {
        self.regular_identity_named(ChildName::Control(component))
    }

    /// Returns the opaque identity of an optional verified regular payload entry.
    pub fn entry_regular_identity(
        &self,
        name: &PrivateEntryName,
    ) -> Result<Option<FileIdentity>, PrivateFsError> {
        self.regular_identity_named(ChildName::Entry(name))
    }

    fn regular_identity_named(
        &self,
        name: ChildName<'_>,
    ) -> Result<Option<FileIdentity>, PrivateFsError> {
        let _operation = self.begin_operation()?;
        self.reject_reserved_name(name)?;
        self.precheck_unlocked()?;
        let identity = self
            .open_optional_regular_unlocked(name, OpenPurpose::Read)?
            .map(|verified| verified.identity);
        self.precheck_unlocked()?;
        Ok(identity)
    }

    /// Inspects one optional payload entry without following links.
    ///
    /// On Unix this is descriptor-relative and nonblocking. A symlink,
    /// hard-linked regular file, FIFO, socket, device, unsafe permission or
    /// ACL, or identity race is rejected rather than reported as a kind.
    pub fn inspect_entry(
        &self,
        name: &PrivateEntryName,
    ) -> Result<Option<PrivateChildKind>, PrivateFsError> {
        let _operation = self.begin_operation()?;
        let name = ChildName::Entry(name);
        self.reject_reserved_name(name)?;
        self.precheck_unlocked()?;
        let inspected = self.lease.observe(platform::inspect_child(
            &self.core.handle,
            &self.core.path,
            name.as_str(),
        ))?;
        let inspected = inspected.map(|kind| match kind {
            platform::RawChildKind::Regular(identity) => {
                PrivateChildKind::RegularFile(FileIdentity(identity))
            }
            platform::RawChildKind::Directory(identity) => {
                PrivateChildKind::Directory(DirectoryIdentity(identity))
            }
        });
        self.precheck_unlocked()?;
        Ok(inspected)
    }

    /// Reads one optional regular child without exceeding `limit`.
    ///
    /// Empty files are returned as empty vectors. Subsystems decide whether
    /// emptiness is valid for their own protocol. The open handle and its
    /// descriptor-relative name are revalidated after the bounded read.
    pub fn read_bounded_regular(
        &self,
        component: &PrivateComponent,
        limit: ByteLimit,
    ) -> Result<Option<Vec<u8>>, PrivateFsError> {
        self.read_bounded_regular_named(ChildName::Control(component), limit)
    }

    /// Reads one optional regular payload entry without exceeding `limit`.
    ///
    /// The exact case-preserving name and open identity are revalidated after
    /// the bounded read. Empty files are returned as empty vectors.
    pub fn read_bounded_entry_regular(
        &self,
        name: &PrivateEntryName,
        limit: ByteLimit,
    ) -> Result<Option<Vec<u8>>, PrivateFsError> {
        self.read_bounded_regular_named(ChildName::Entry(name), limit)
    }

    /// Runs a callback with one bounded verified protocol-file reader.
    ///
    /// The callback receives only a [`Read`] capability capped at the file's
    /// admitted initial length, never a file or path. Its result is nested so
    /// caller-owned validation failures remain distinct from filesystem
    /// failures. Identity, exact spelling, length, and the namespace boundary
    /// are revalidated after the callback even when it returns an error value.
    /// The callback executes under the namespace operation lease and therefore
    /// must not re-enter this namespace.
    pub fn with_bounded_regular_reader<T, E>(
        &self,
        component: &PrivateComponent,
        limit: ByteLimit,
        callback: impl FnOnce(&mut dyn Read) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, PrivateFsError> {
        self.with_bounded_regular_reader_named(ChildName::Control(component), limit, callback)
    }

    /// Runs a callback with one bounded verified payload-entry reader.
    ///
    /// The callback receives only a [`Read`] capability capped at the file's
    /// admitted initial length. Filesystem validation always takes precedence
    /// over the nested callback result. It must not re-enter this namespace.
    pub fn with_bounded_entry_regular_reader<T, E>(
        &self,
        name: &PrivateEntryName,
        limit: ByteLimit,
        callback: impl FnOnce(&mut dyn Read) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, PrivateFsError> {
        self.with_bounded_regular_reader_named(ChildName::Entry(name), limit, callback)
    }

    fn with_bounded_regular_reader_named<T, E>(
        &self,
        name: ChildName<'_>,
        limit: ByteLimit,
        callback: impl FnOnce(&mut dyn Read) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, PrivateFsError> {
        let _operation = self.begin_operation()?;
        self.reject_reserved_name(name)?;
        self.precheck_unlocked()?;
        let Some(mut verified) = self.open_optional_regular_unlocked(name, OpenPurpose::Read)?
        else {
            self.precheck_unlocked()?;
            return Ok(None);
        };
        #[cfg(test)]
        let initial_metadata = if self
            .lease
            .take_streaming_fault(crate::lease::StreamingFault::InitialReadMetadata)
        {
            Err(PrivateFsError::IdentityAmbiguous)
        } else {
            verified
                .file
                .metadata()
                .map_err(|_| PrivateFsError::IdentityAmbiguous)
        };
        #[cfg(not(test))]
        let initial_metadata = verified
            .file
            .metadata()
            .map_err(|_| PrivateFsError::IdentityAmbiguous);
        let initial_length = self.lease.observe(initial_metadata)?.len();
        if initial_length > u64::try_from(limit.get()).unwrap_or(u64::MAX) {
            return Err(PrivateFsError::BoundExceeded);
        }

        let callback_result = {
            let mut bounded = Read::by_ref(&mut verified.file).take(initial_length);
            callback(&mut bounded)
        };
        let validation = (|| {
            let final_length = verified
                .file
                .metadata()
                .map_err(|_| PrivateFsError::IdentityAmbiguous)?
                .len();
            if final_length != initial_length {
                return Err(PrivateFsError::IdentityAmbiguous);
            }
            platform::revalidate_regular(
                &self.core.handle,
                &self.core.path,
                name.as_str(),
                &verified.file,
                verified.identity.0,
            )?;
            self.verify_boundary_unlocked()
        })();
        self.lease.observe(validation)?;
        Ok(Some(callback_result))
    }

    fn read_bounded_regular_named(
        &self,
        name: ChildName<'_>,
        limit: ByteLimit,
    ) -> Result<Option<Vec<u8>>, PrivateFsError> {
        let _operation = self.begin_operation()?;
        self.reject_reserved_name(name)?;
        self.precheck_unlocked()?;
        let Some(mut verified) = self.open_optional_regular_unlocked(name, OpenPurpose::Read)?
        else {
            self.precheck_unlocked()?;
            return Ok(None);
        };
        let metadata = verified.file.metadata().map_err(|_| PrivateFsError::Io)?;
        let length = usize::try_from(metadata.len()).map_err(|_| PrivateFsError::BoundExceeded)?;
        if length > limit.get() {
            return Err(PrivateFsError::BoundExceeded);
        }
        let mut bytes = Vec::with_capacity(length);
        Read::by_ref(&mut verified.file)
            .take(
                u64::try_from(limit.get())
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
            )
            .read_to_end(&mut bytes)
            .map_err(|_| PrivateFsError::Io)?;
        if bytes.len() != length || bytes.len() > limit.get() {
            return Err(PrivateFsError::BoundExceeded);
        }
        self.lease.observe(platform::revalidate_regular(
            &self.core.handle,
            &self.core.path,
            name.as_str(),
            &verified.file,
            verified.identity.0,
        ))?;
        self.precheck_unlocked()?;
        Ok(Some(bytes))
    }

    /// Creates, writes, and durably settles one new private regular child.
    ///
    /// The file is flushed before its descriptor-relative identity is checked;
    /// the containing directory is then flushed before the final boundary
    /// check. Any failure after create-new commits returns
    /// [`PrivateFsError::SettlementUnknown`] and quarantines the shared lease.
    pub fn write_new_synced(
        &self,
        component: &PrivateComponent,
        bytes: &[u8],
        limit: ByteLimit,
    ) -> Result<FileIdentity, PrivateFsError> {
        self.write_new_synced_named(ChildName::Control(component), bytes, limit)
    }

    /// Creates, writes, and durably settles one new regular payload entry.
    ///
    /// This is create-new only and never overwrites an existing node. Any
    /// failure after creation may have committed and therefore quarantines the
    /// shared lease with [`PrivateFsError::SettlementUnknown`].
    pub fn write_new_entry_synced(
        &self,
        name: &PrivateEntryName,
        bytes: &[u8],
        limit: ByteLimit,
    ) -> Result<FileIdentity, PrivateFsError> {
        self.write_new_synced_named(ChildName::Entry(name), bytes, limit)
    }

    /// Streams one exact-length source into a new regular payload entry.
    ///
    /// The source must yield exactly `expected_length` bytes followed by EOF.
    /// Short, long, and source-error outcomes trigger held-identity cleanup;
    /// they are returned as clean source failures only after durable removal,
    /// exact absence, and the namespace boundary have all been proven. Source
    /// errors and source bytes are never retained. The reader executes under
    /// the non-reentrant namespace operation lease and therefore must not
    /// re-enter this namespace.
    pub fn write_new_entry_from_reader<R: Read + ?Sized>(
        &self,
        name: &PrivateEntryName,
        reader: &mut R,
        expected_length: StreamingFileLength,
    ) -> Result<FileIdentity, StreamingWriteError> {
        self.write_new_from_reader_named(ChildName::Entry(name), reader, expected_length)
    }

    fn write_new_from_reader_named(
        &self,
        name: ChildName<'_>,
        reader: &mut (impl Read + ?Sized),
        expected_length: StreamingFileLength,
    ) -> Result<FileIdentity, StreamingWriteError> {
        let _operation = self.begin_operation().map_err(StreamingWriteError::from)?;
        self.reject_reserved_name(name)
            .map_err(StreamingWriteError::from)?;
        self.precheck_unlocked()
            .map_err(StreamingWriteError::from)?;

        let (file, identity) =
            match platform::create_new_regular(&self.core.handle, &self.core.path, name.as_str()) {
                Ok(created) => created,
                Err(PrivateFsError::SettlementUnknown) => {
                    return self.quarantine_streaming_write_unlocked();
                }
                Err(PrivateFsError::AlreadyExists) => {
                    let inspected = self
                        .lease
                        .observe(platform::inspect_child(
                            &self.core.handle,
                            &self.core.path,
                            name.as_str(),
                        ))
                        .map_err(StreamingWriteError::from)?;
                    let error = match inspected {
                        Some(platform::RawChildKind::Regular(_)) => PrivateFsError::AlreadyExists,
                        Some(platform::RawChildKind::Directory(_)) => PrivateFsError::Unsafe,
                        None => {
                            let error = self
                                .lease
                                .observe::<()>(Err(PrivateFsError::IdentityAmbiguous))
                                .err()
                                .unwrap_or(PrivateFsError::IdentityAmbiguous);
                            return Err(StreamingWriteError::Filesystem(error));
                        }
                    };
                    self.precheck_unlocked()
                        .map_err(StreamingWriteError::from)?;
                    return Err(StreamingWriteError::Filesystem(error));
                }
                Err(error) => {
                    self.precheck_unlocked()
                        .map_err(StreamingWriteError::from)?;
                    return Err(StreamingWriteError::Filesystem(error));
                }
            };
        let mut verified = VerifiedRegular {
            file,
            identity: FileIdentity(identity),
        };

        if platform::verify_exact_name(&verified.file, name.as_str()).is_err() {
            return self.quarantine_streaming_write_unlocked();
        }

        #[cfg(test)]
        let copy_result = if self
            .lease
            .take_streaming_fault(crate::lease::StreamingFault::Write)
        {
            Err(ExactCopyError::SinkWrite)
        } else {
            copy_exact(reader, &mut verified.file, expected_length)
        };
        #[cfg(not(test))]
        let copy_result = copy_exact(reader, &mut verified.file, expected_length);
        if let Err(error) = copy_result {
            return self.fail_streaming_write_unlocked(name, verified, error.into_public());
        }

        // `Read` is caller-controlled through the EOF probe. It may have
        // mutated the newly created inode by another descriptor while still
        // reporting exact EOF, so bind the result to the declared length only
        // after the final source invocation has returned.
        let exact_length = verified
            .file
            .metadata()
            .map(|metadata| metadata.len() == expected_length.get())
            .unwrap_or(false);
        if !exact_length {
            return self.fail_streaming_write_unlocked(
                name,
                verified,
                StreamingWriteError::SinkLengthMismatch,
            );
        }

        let file_sync = verified.file.sync_all().map_err(|_| PrivateFsError::Io);
        #[cfg(test)]
        let file_sync = file_sync.and_then(|()| {
            if self
                .lease
                .take_streaming_fault(crate::lease::StreamingFault::FileSync)
            {
                Err(PrivateFsError::Io)
            } else {
                Ok(())
            }
        });
        if let Err(error) = file_sync {
            return self.fail_streaming_write_unlocked(
                name,
                verified,
                StreamingWriteError::Filesystem(error),
            );
        }

        if platform::revalidate_regular(
            &self.core.handle,
            &self.core.path,
            name.as_str(),
            &verified.file,
            verified.identity.0,
        )
        .is_err()
        {
            return self.quarantine_streaming_write_unlocked();
        }

        let directory_sync = platform::sync_directory(&self.core.handle);
        #[cfg(test)]
        let directory_sync = directory_sync.and_then(|()| {
            if self
                .lease
                .take_streaming_fault(crate::lease::StreamingFault::DirectorySync)
            {
                Err(PrivateFsError::Io)
            } else {
                Ok(())
            }
        });
        if let Err(error) = directory_sync {
            return self.fail_streaming_write_unlocked(
                name,
                verified,
                StreamingWriteError::Filesystem(error),
            );
        }

        if self.verify_boundary_unlocked().is_err() {
            return self.quarantine_streaming_write_unlocked();
        }
        Ok(verified.identity)
    }

    fn write_new_synced_named(
        &self,
        name: ChildName<'_>,
        bytes: &[u8],
        limit: ByteLimit,
    ) -> Result<FileIdentity, PrivateFsError> {
        let _operation = self.begin_operation()?;
        self.reject_reserved_name(name)?;
        if bytes.len() > limit.get() {
            return Err(PrivateFsError::BoundExceeded);
        }
        self.precheck_unlocked()?;
        let (mut file, identity) =
            match platform::create_new_regular(&self.core.handle, &self.core.path, name.as_str()) {
                Ok(created) => {
                    if platform::verify_exact_name(&created.0, name.as_str()).is_err() {
                        return self
                            .lease
                            .settle_after_commit(Err(PrivateFsError::SettlementUnknown));
                    }
                    created
                }
                Err(PrivateFsError::SettlementUnknown) => {
                    return self
                        .lease
                        .settle_after_commit(Err(PrivateFsError::SettlementUnknown));
                }
                Err(PrivateFsError::AlreadyExists) => {
                    let inspected = self.lease.observe(platform::inspect_child(
                        &self.core.handle,
                        &self.core.path,
                        name.as_str(),
                    ))?;
                    if inspected.is_none() {
                        return self.lease.observe(Err(PrivateFsError::IdentityAmbiguous));
                    }
                    self.precheck_unlocked()?;
                    return Err(PrivateFsError::AlreadyExists);
                }
                Err(error) => {
                    self.precheck_unlocked()?;
                    return Err(error);
                }
            };
        let settlement = (|| {
            file.write_all(bytes).map_err(|_| PrivateFsError::Io)?;
            file.sync_all().map_err(|_| PrivateFsError::Io)?;
            platform::revalidate_regular(
                &self.core.handle,
                &self.core.path,
                name.as_str(),
                &file,
                identity,
            )?;
            platform::sync_directory(&self.core.handle)?;
            self.verify_boundary_unlocked()?;
            Ok(FileIdentity(identity))
        })();
        self.lease.settle_after_commit(settlement)
    }

    /// Durably removes one optional child after regular-file admission.
    ///
    /// Once unlink commits, every failure becomes an unknown settlement and
    /// quarantines the lease; callers must not infer whether residue exists.
    pub fn remove_verified_regular(
        &self,
        component: &PrivateComponent,
    ) -> Result<bool, PrivateFsError> {
        self.remove_verified_regular_named(ChildName::Control(component))
    }

    /// Durably removes one optional verified regular payload entry.
    ///
    /// Links, directories, special nodes, unsafe permissions or ACLs, case
    /// aliases, and identity races fail closed without being removed.
    pub fn remove_verified_entry_regular(
        &self,
        name: &PrivateEntryName,
    ) -> Result<bool, PrivateFsError> {
        self.remove_verified_regular_named(ChildName::Entry(name))
    }

    fn remove_verified_regular_named(&self, name: ChildName<'_>) -> Result<bool, PrivateFsError> {
        let _operation = self.begin_operation()?;
        self.reject_reserved_name(name)?;
        self.precheck_unlocked()?;
        let Some(verified) = self.open_optional_regular_unlocked(name, OpenPurpose::Mutation)?
        else {
            self.precheck_unlocked()?;
            return Ok(false);
        };
        if let Err(error) = self.remove_regular_unlocked(name) {
            let unchanged = platform::revalidate_regular(
                &self.core.handle,
                &self.core.path,
                name.as_str(),
                &verified.file,
                verified.identity.0,
            )
            .and_then(|()| self.verify_boundary_unlocked());
            return self.settle_failed_mutation_unlocked(unchanged, error);
        }
        let settlement = (|| {
            drop(verified);
            platform::sync_directory(&self.core.handle)?;
            self.ensure_absent_unlocked(name)?;
            self.verify_boundary_unlocked()?;
            Ok(true)
        })();
        self.lease.settle_after_commit(settlement)
    }

    /// Durably and atomically replaces `destination` with `source`.
    ///
    /// An existing destination remains intact until the replacing rename
    /// commits. After commit the directory is flushed, the installed identity
    /// must equal the held source identity, and the old source name must be
    /// absent. Unsettled commits quarantine the complete shared lease.
    pub fn replace_verified_regular(
        &self,
        source: &PrivateComponent,
        destination: &PrivateComponent,
    ) -> Result<FileIdentity, PrivateFsError> {
        let _operation = self.begin_operation()?;
        self.reject_reserved(source)?;
        self.reject_reserved(destination)?;
        if source == destination {
            return Err(PrivateFsError::Unsafe);
        }
        self.precheck_unlocked()?;
        let source_file = self
            .open_optional_regular_unlocked(ChildName::Control(source), OpenPurpose::Mutation)?
            .ok_or(PrivateFsError::Unsafe)?;
        source_file
            .file
            .sync_all()
            .map_err(|_| PrivateFsError::Io)?;
        self.lease.observe(platform::revalidate_regular(
            &self.core.handle,
            &self.core.path,
            source.as_str(),
            &source_file.file,
            source_file.identity.0,
        ))?;
        if let Some(destination_file) = self.open_optional_regular_unlocked(
            ChildName::Control(destination),
            OpenPurpose::Mutation,
        )? {
            drop(destination_file);
        }
        if let Err(error) = self.atomic_replace_unlocked(source, destination) {
            let unchanged = self.revalidate_failed_rename_unlocked(source, &source_file);
            return self.settle_failed_mutation_unlocked(unchanged, error);
        }
        self.settle_rename_unlocked(source, destination, source_file)
    }

    /// Durably publishes `source` only when `destination` is absent.
    ///
    /// This calls the platform's true no-replace primitive; it never emulates
    /// exclusivity with an observation followed by a replacing rename.
    pub fn publish_noreplace_verified_regular(
        &self,
        source: &PrivateComponent,
        destination: &PrivateComponent,
    ) -> Result<FileIdentity, PrivateFsError> {
        let _operation = self.begin_operation()?;
        self.reject_reserved(source)?;
        self.reject_reserved(destination)?;
        if source == destination {
            return Err(PrivateFsError::Unsafe);
        }
        self.precheck_unlocked()?;
        let source_file = self
            .open_optional_regular_unlocked(ChildName::Control(source), OpenPurpose::Mutation)?
            .ok_or(PrivateFsError::Unsafe)?;
        source_file
            .file
            .sync_all()
            .map_err(|_| PrivateFsError::Io)?;
        self.lease.observe(platform::revalidate_regular(
            &self.core.handle,
            &self.core.path,
            source.as_str(),
            &source_file.file,
            source_file.identity.0,
        ))?;
        if let Err(error) = self.atomic_publish_noreplace_unlocked(source, destination) {
            let unchanged = self.revalidate_failed_rename_unlocked(source, &source_file);
            return self.settle_failed_mutation_unlocked(unchanged, error);
        }
        self.settle_rename_unlocked(source, destination, source_file)
    }

    /// Flushes this directory's metadata after serialized boundary checks.
    pub fn sync(&self) -> Result<(), PrivateFsError> {
        let _operation = self.begin_operation()?;
        self.precheck_unlocked()?;
        platform::sync_directory(&self.core.handle)?;
        self.precheck_unlocked()
    }

    fn begin_operation(&self) -> Result<MutexGuard<'_, ()>, PrivateFsError> {
        self.lease.begin()
    }

    fn create_directory_unlocked(&self, name: ChildName<'_>) -> Result<bool, PrivateFsError> {
        let result = platform::create_directory(&self.core.handle, &self.core.path, name.as_str());
        #[cfg(test)]
        if matches!(&result, Ok(true))
            && self.lease.take_committed_mutation_fault(
                crate::lease::CommittedMutationFault::CreateDirectory,
            )
        {
            return Err(PrivateFsError::SettlementUnknown);
        }
        result
    }

    fn remove_regular_unlocked(&self, name: ChildName<'_>) -> Result<(), PrivateFsError> {
        let result = platform::remove_regular(&self.core.handle, &self.core.path, name.as_str());
        #[cfg(test)]
        if result.is_ok()
            && self
                .lease
                .take_committed_mutation_fault(crate::lease::CommittedMutationFault::Remove)
        {
            return Err(PrivateFsError::Io);
        }
        result
    }

    fn atomic_replace_unlocked(
        &self,
        source: &PrivateComponent,
        destination: &PrivateComponent,
    ) -> Result<(), PrivateFsError> {
        let result = platform::atomic_replace(
            &self.core.handle,
            &self.core.path,
            source.as_str(),
            destination.as_str(),
        );
        #[cfg(test)]
        if result.is_ok()
            && self
                .lease
                .take_committed_mutation_fault(crate::lease::CommittedMutationFault::Replace)
        {
            return Err(PrivateFsError::Io);
        }
        result
    }

    fn atomic_publish_noreplace_unlocked(
        &self,
        source: &PrivateComponent,
        destination: &PrivateComponent,
    ) -> Result<(), PrivateFsError> {
        let result = platform::atomic_publish_noreplace(
            &self.core.handle,
            &self.core.path,
            source.as_str(),
            destination.as_str(),
        );
        #[cfg(test)]
        if result.is_ok()
            && self.lease.take_committed_mutation_fault(
                crate::lease::CommittedMutationFault::PublishNoreplace,
            )
        {
            return Err(PrivateFsError::Io);
        }
        result
    }

    fn reject_reserved(&self, component: &PrivateComponent) -> Result<(), PrivateFsError> {
        self.reject_reserved_name(ChildName::Control(component))
    }

    fn reject_reserved_name(&self, name: ChildName<'_>) -> Result<(), PrivateFsError> {
        if self.role == DirectoryRole::Root && self.lease.is_reserved_name(name.as_str()) {
            return Err(PrivateFsError::ReservedComponent);
        }
        Ok(())
    }

    fn precheck_unlocked(&self) -> Result<(), PrivateFsError> {
        self.lease.observe(self.verify_boundary_unlocked())
    }

    fn verify_boundary_unlocked(&self) -> Result<(), PrivateFsError> {
        self.lease.verify_authority()?;
        verify_core_boundary(&self.core)
    }

    fn open_child_unlocked(&self, name: ChildName<'_>) -> Result<Self, PrivateFsError> {
        let (handle, identity) =
            platform::open_child_directory(&self.core.handle, &self.core.path, name.as_str())?;
        platform::verify_exact_name(&handle, name.as_str())?;
        Ok(Self {
            core: DirectoryCore {
                path: self.core.path.join(name.as_str()),
                handle,
                identity: DirectoryIdentity(identity),
                child_name: Some(name.to_owned_name()),
            },
            lease: Arc::clone(&self.lease),
            role: DirectoryRole::Child,
        })
    }

    fn open_optional_regular_unlocked(
        &self,
        name: ChildName<'_>,
        purpose: OpenPurpose,
    ) -> Result<Option<VerifiedRegular>, PrivateFsError> {
        let opened =
            platform::open_regular(&self.core.handle, &self.core.path, name.as_str(), purpose)
                .and_then(|(file, identity)| {
                    platform::verify_exact_name(&file, name.as_str())?;
                    Ok((file, identity))
                });
        match self.lease.observe(opened) {
            Ok((file, identity)) => Ok(Some(VerifiedRegular {
                file,
                identity: FileIdentity(identity),
            })),
            Err(PrivateFsError::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn ensure_absent_unlocked(&self, name: ChildName<'_>) -> Result<(), PrivateFsError> {
        if self
            .open_optional_regular_unlocked(name, OpenPurpose::Read)?
            .is_some()
        {
            return Err(PrivateFsError::IdentityAmbiguous);
        }
        Ok(())
    }

    fn settle_rename_unlocked(
        &self,
        source: &PrivateComponent,
        destination: &PrivateComponent,
        source_file: VerifiedRegular,
    ) -> Result<FileIdentity, PrivateFsError> {
        let settlement = (|| {
            platform::sync_directory(&self.core.handle)?;
            let destination_file = self
                .open_optional_regular_unlocked(ChildName::Control(destination), OpenPurpose::Read)?
                .ok_or(PrivateFsError::IdentityAmbiguous)?;
            if source_file.identity != destination_file.identity {
                return Err(PrivateFsError::IdentityAmbiguous);
            }
            self.ensure_absent_unlocked(ChildName::Control(source))?;
            self.verify_boundary_unlocked()?;
            Ok(destination_file.identity)
        })();
        self.lease.settle_after_commit(settlement)
    }

    fn revalidate_failed_rename_unlocked(
        &self,
        source: &PrivateComponent,
        source_file: &VerifiedRegular,
    ) -> Result<(), PrivateFsError> {
        platform::revalidate_regular(
            &self.core.handle,
            &self.core.path,
            source.as_str(),
            &source_file.file,
            source_file.identity.0,
        )?;
        self.verify_boundary_unlocked()
    }

    fn settle_failed_mutation_unlocked<T>(
        &self,
        unchanged: Result<(), PrivateFsError>,
        syscall_error: PrivateFsError,
    ) -> Result<T, PrivateFsError> {
        if unchanged.is_ok() {
            Err(syscall_error)
        } else {
            self.lease
                .settle_after_commit(Err(PrivateFsError::SettlementUnknown))
        }
    }

    fn fail_streaming_write_unlocked<T>(
        &self,
        name: ChildName<'_>,
        verified: VerifiedRegular,
        clean_error: StreamingWriteError,
    ) -> Result<T, StreamingWriteError> {
        if self
            .cleanup_created_regular_unlocked(name, verified)
            .is_ok()
        {
            return Err(clean_error);
        }

        self.quarantine_streaming_write_unlocked()
    }

    fn quarantine_streaming_write_unlocked<T>(&self) -> Result<T, StreamingWriteError> {
        let settlement = self
            .lease
            .settle_after_commit::<()>(Err(PrivateFsError::SettlementUnknown));
        let error = settlement
            .err()
            .unwrap_or(PrivateFsError::SettlementUnknown);
        Err(StreamingWriteError::Filesystem(error))
    }

    fn cleanup_created_regular_unlocked(
        &self,
        name: ChildName<'_>,
        verified: VerifiedRegular,
    ) -> Result<(), PrivateFsError> {
        platform::revalidate_regular(
            &self.core.handle,
            &self.core.path,
            name.as_str(),
            &verified.file,
            verified.identity.0,
        )?;
        // `Read` is caller-controlled and may have invalidated the namespace
        // while the operation lease was held. Re-establish root, lock, and
        // directory authority immediately before the cleanup mutation. If
        // authority is ambiguous, leave the held inode as recovery residue and
        // let the caller quarantine the lease.
        self.verify_boundary_unlocked()?;
        self.remove_regular_unlocked(name)?;
        drop(verified);
        platform::sync_directory(&self.core.handle)?;
        #[cfg(test)]
        if self
            .lease
            .take_streaming_fault(crate::lease::StreamingFault::CleanupDirectorySync)
        {
            return Err(PrivateFsError::Io);
        }
        self.ensure_absent_unlocked(name)?;
        self.verify_boundary_unlocked()
    }
}

/// A private namespace held under one exact nonblocking cross-process lock.
pub struct LockedPrivateNamespace {
    directory: PrivateDirectory,
}

impl LockedPrivateNamespace {
    /// Creates or admits an absolute dedicated root and acquires the crate-owned
    /// canonical namespace lock.
    ///
    /// Missing ancestors are never created. Every existing ancestor must be a
    /// non-symlink directory accepted by the platform boundary checks. Root,
    /// administrators, and principals granted mutation through an ancestor ACL
    /// remain outside this boundary's guarantee. Windows and unsupported Unix
    /// targets fail before path inspection.
    pub fn open_or_create(root: impl Into<PathBuf>) -> Result<Self, PrivateFsError> {
        platform::admit_namespace_support()?;
        let root = root.into();
        if !root.is_absolute() || root.file_name().is_none() {
            return Err(PrivateFsError::Unsafe);
        }
        let root_created = match fs::symlink_metadata(&root) {
            Ok(_) => false,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let parent = root.parent().ok_or(PrivateFsError::Unsafe)?;
                validate_directory_chain(parent, false)?;
                let builder = private_directory_builder();
                match builder.create(&root) {
                    Ok(()) => {
                        if platform::sync_ancestor_directory(parent).is_err() {
                            return Err(PrivateFsError::SettlementUnknown);
                        }
                        true
                    }
                    Err(create) if create.kind() == std::io::ErrorKind::AlreadyExists => false,
                    Err(create) => return Err(classify_root_create_error(&root, &create)),
                }
            }
            Err(_) => return Err(PrivateFsError::Io),
        };
        let core = match admit_root(root) {
            Ok(core) => core,
            Err(_) if root_created => return Err(PrivateFsError::SettlementUnknown),
            Err(error) => return Err(error),
        };
        let lock_component = canonical_lock_component()?;
        let lock_staging_component = lock_staging_component()?;
        let lock = acquire_lock(&core, &lock_component, &lock_staging_component)?;
        let lease_root_handle = core
            .handle
            .try_clone()
            .map_err(|_| PrivateFsError::SettlementUnknown)?;
        let lease = Arc::new(NamespaceLease::new(
            lock.file,
            lock.identity,
            lock_component,
            lock_staging_component,
            core.path.clone(),
            lease_root_handle,
            core.identity,
        ));
        Ok(Self {
            directory: PrivateDirectory {
                core,
                lease,
                role: DirectoryRole::Root,
            },
        })
    }

    /// Returns the held private root.
    #[must_use]
    pub const fn directory(&self) -> &PrivateDirectory {
        &self.directory
    }
}

struct VerifiedRegular {
    file: File,
    identity: FileIdentity,
}

fn admit_root(path: PathBuf) -> Result<DirectoryCore, PrivateFsError> {
    validate_directory_chain(&path, true)?;
    let (handle, identity) = platform::open_directory(&path)?;
    Ok(DirectoryCore {
        path,
        handle,
        identity: DirectoryIdentity(identity),
        child_name: None,
    })
}

fn acquire_lock(
    directory: &DirectoryCore,
    lock_name: &PrivateComponent,
    lock_staging_name: &PrivateComponent,
) -> Result<VerifiedRegular, PrivateFsError> {
    match open_lock_regular(directory, lock_name) {
        Ok(lock) => admit_existing_lock(directory, lock_name, lock_staging_name, lock),
        Err(PrivateFsError::NotFound) => {
            initialize_or_recover_lock(directory, lock_name, lock_staging_name)
        }
        Err(error) => Err(error),
    }
}

fn open_lock_regular(
    directory: &DirectoryCore,
    name: &PrivateComponent,
) -> Result<VerifiedRegular, PrivateFsError> {
    let (file, identity) = platform::open_regular(
        &directory.handle,
        &directory.path,
        name.as_str(),
        OpenPurpose::Lock,
    )?;
    platform::verify_exact_name(&file, name.as_str())?;
    Ok(VerifiedRegular {
        file,
        identity: FileIdentity(identity),
    })
}

fn revalidate_exact_lock_regular(
    directory: &DirectoryCore,
    name: &PrivateComponent,
    regular: &VerifiedRegular,
) -> Result<(), PrivateFsError> {
    platform::revalidate_regular(
        &directory.handle,
        &directory.path,
        name.as_str(),
        &regular.file,
        regular.identity.0,
    )
}

fn open_required_lock_regular(
    directory: &DirectoryCore,
    name: &PrivateComponent,
) -> Result<VerifiedRegular, PrivateFsError> {
    match open_lock_regular(directory, name) {
        Err(PrivateFsError::NotFound) => Err(PrivateFsError::IdentityAmbiguous),
        outcome => outcome,
    }
}

fn admit_existing_lock(
    directory: &DirectoryCore,
    lock_name: &PrivateComponent,
    lock_staging_name: &PrivateComponent,
    mut lock: VerifiedRegular,
) -> Result<VerifiedRegular, PrivateFsError> {
    revalidate_exact_lock_regular(directory, lock_name, &lock)?;
    if !platform::lock_exclusive(&lock.file) {
        return Err(PrivateFsError::LockUnavailable);
    }
    // The descriptor-relative lock entry and root identity must still bind to
    // the held handles after flock succeeds, before the shared lease exists.
    revalidate_exact_lock_regular(directory, lock_name, &lock)?;
    if read_lock_content(&mut lock.file)? != LOCK_FILE_CONTENT {
        return Err(PrivateFsError::Unsafe);
    }
    revalidate_exact_lock_regular(directory, lock_name, &lock)?;
    verify_core_boundary(directory)?;
    inspect_and_cleanup_lock_staging(directory, lock_name, &mut lock, lock_staging_name)?;
    if read_lock_content(&mut lock.file)? != LOCK_FILE_CONTENT {
        return Err(PrivateFsError::Unsafe);
    }
    revalidate_exact_lock_regular(directory, lock_name, &lock)?;
    Ok(lock)
}

fn initialize_or_recover_lock(
    directory: &DirectoryCore,
    lock_name: &PrivateComponent,
    lock_staging_name: &PrivateComponent,
) -> Result<VerifiedRegular, PrivateFsError> {
    let (mut staging, created) = match platform::create_new_regular(
        &directory.handle,
        &directory.path,
        lock_staging_name.as_str(),
    ) {
        Ok((file, identity)) => {
            if platform::verify_exact_name(&file, lock_staging_name.as_str()).is_err() {
                // Create-new has committed. If the exact directory-entry name
                // cannot be proven, callers must not infer what residue exists.
                return Err(PrivateFsError::SettlementUnknown);
            }
            (
                VerifiedRegular {
                    file,
                    identity: FileIdentity(identity),
                },
                true,
            )
        }
        Err(PrivateFsError::AlreadyExists) => {
            // The staging entry may have been atomically published between the
            // canonical miss and this observation. Prefer the canonical entry
            // if it now exists; otherwise admit only the staging protocol's
            // exact marker or recoverable strict-prefix scratch state.
            match open_lock_regular(directory, lock_name) {
                Ok(lock) => {
                    return admit_existing_lock(directory, lock_name, lock_staging_name, lock);
                }
                Err(PrivateFsError::NotFound) => {}
                Err(error) => return Err(error),
            }
            match open_lock_regular(directory, lock_staging_name) {
                Ok(staging) => (staging, false),
                Err(PrivateFsError::NotFound) => {
                    let lock = open_required_lock_regular(directory, lock_name)?;
                    return admit_existing_lock(directory, lock_name, lock_staging_name, lock);
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    };

    let mut staging_mutated = created;
    let preparation = prepare_staged_lock(
        directory,
        lock_staging_name,
        &mut staging,
        &mut staging_mutated,
    );
    if let Err(error) = preparation {
        return if staging_mutated {
            Err(PrivateFsError::SettlementUnknown)
        } else {
            Err(error)
        };
    }

    match platform::atomic_publish_noreplace(
        &directory.handle,
        &directory.path,
        lock_staging_name.as_str(),
        lock_name.as_str(),
    ) {
        Ok(()) => settle_published_lock(directory, lock_name, lock_staging_name, staging),
        Err(PrivateFsError::AlreadyExists) if !staging_mutated => {
            // The existing, exact staging residue was not mutated. A racing
            // initializer won publication, so admission restarts at the only
            // authoritative name.
            let lock = open_required_lock_regular(directory, lock_name)?;
            admit_existing_lock(directory, lock_name, lock_staging_name, lock)
        }
        Err(_) => Err(PrivateFsError::SettlementUnknown),
    }
}

fn prepare_staged_lock(
    directory: &DirectoryCore,
    staging_name: &PrivateComponent,
    staging: &mut VerifiedRegular,
    mutated: &mut bool,
) -> Result<(), PrivateFsError> {
    revalidate_exact_lock_regular(directory, staging_name, staging)?;
    if !platform::lock_exclusive(&staging.file) {
        return Err(PrivateFsError::LockUnavailable);
    }
    revalidate_exact_lock_regular(directory, staging_name, staging)?;

    let bytes = read_lock_content(&mut staging.file)?;
    if bytes != LOCK_FILE_CONTENT {
        if !LOCK_FILE_CONTENT.starts_with(&bytes) {
            return Err(PrivateFsError::Unsafe);
        }
        // A fixed staging entry is reserved scratch state for lock
        // initialization. Under its exact inode lock, an empty file or strict
        // marker prefix is treated as recoverable scratch residue. Canonical
        // admission never applies this recovery rule.
        *mutated = true;
        staging.file.set_len(0).map_err(|_| PrivateFsError::Io)?;
        staging.file.rewind().map_err(|_| PrivateFsError::Io)?;
        staging
            .file
            .write_all(LOCK_FILE_CONTENT)
            .map_err(|_| PrivateFsError::Io)?;
    }
    staging.file.sync_all().map_err(|_| PrivateFsError::Io)?;
    revalidate_exact_lock_regular(directory, staging_name, staging)?;
    if read_lock_content(&mut staging.file)? != LOCK_FILE_CONTENT {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    verify_core_boundary(directory)
}

fn settle_published_lock(
    directory: &DirectoryCore,
    lock_name: &PrivateComponent,
    lock_staging_name: &PrivateComponent,
    mut lock: VerifiedRegular,
) -> Result<VerifiedRegular, PrivateFsError> {
    let settlement = (|| {
        revalidate_exact_lock_regular(directory, lock_name, &lock)?;
        platform::sync_directory(&directory.handle)?;
        if read_lock_content(&mut lock.file)? != LOCK_FILE_CONTENT {
            return Err(PrivateFsError::IdentityAmbiguous);
        }
        revalidate_exact_lock_regular(directory, lock_name, &lock)?;
        verify_core_boundary(directory)?;
        inspect_and_cleanup_lock_staging(directory, lock_name, &mut lock, lock_staging_name)?;
        Ok(lock)
    })();
    settlement.map_err(|_| PrivateFsError::SettlementUnknown)
}

fn inspect_and_cleanup_lock_staging(
    directory: &DirectoryCore,
    lock_name: &PrivateComponent,
    lock: &mut VerifiedRegular,
    lock_staging_name: &PrivateComponent,
) -> Result<(), PrivateFsError> {
    let mut staging = match open_lock_regular(directory, lock_staging_name) {
        Ok(staging) => staging,
        Err(PrivateFsError::NotFound) => return Ok(()),
        Err(error) => return Err(error),
    };
    revalidate_exact_lock_regular(directory, lock_staging_name, &staging)?;
    if !platform::lock_exclusive(&staging.file) {
        return Err(PrivateFsError::LockUnavailable);
    }
    revalidate_exact_lock_regular(directory, lock_staging_name, &staging)?;
    let bytes = read_lock_content(&mut staging.file)?;
    revalidate_exact_lock_regular(directory, lock_staging_name, &staging)?;
    if bytes != LOCK_FILE_CONTENT && !LOCK_FILE_CONTENT.starts_with(&bytes) {
        return Err(PrivateFsError::Unsafe);
    }

    if platform::remove_regular(
        &directory.handle,
        &directory.path,
        lock_staging_name.as_str(),
    )
    .is_err()
    {
        return Err(PrivateFsError::SettlementUnknown);
    }
    let settlement = (|| {
        platform::sync_directory(&directory.handle)?;
        match open_lock_regular(directory, lock_staging_name) {
            Err(PrivateFsError::NotFound) => {}
            Ok(_) | Err(_) => return Err(PrivateFsError::IdentityAmbiguous),
        }
        revalidate_exact_lock_regular(directory, lock_name, lock)?;
        if read_lock_content(&mut lock.file)? != LOCK_FILE_CONTENT {
            return Err(PrivateFsError::IdentityAmbiguous);
        }
        verify_core_boundary(directory)
    })();
    settlement.map_err(|_| PrivateFsError::SettlementUnknown)
}

fn canonical_lock_component() -> Result<PrivateComponent, PrivateFsError> {
    PrivateComponent::new(LOCK_COMPONENT_NAME).map_err(|_| PrivateFsError::PrimitiveUnavailable)
}

fn lock_staging_component() -> Result<PrivateComponent, PrivateFsError> {
    PrivateComponent::new(LOCK_STAGING_COMPONENT_NAME)
        .map_err(|_| PrivateFsError::PrimitiveUnavailable)
}

fn read_lock_content(file: &mut File) -> Result<Vec<u8>, PrivateFsError> {
    file.rewind().map_err(|_| PrivateFsError::Io)?;
    let mut bytes = Vec::with_capacity(LOCK_FILE_CONTENT.len().saturating_add(1));
    Read::by_ref(file)
        .take(
            u64::try_from(LOCK_FILE_CONTENT.len())
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        )
        .read_to_end(&mut bytes)
        .map_err(|_| PrivateFsError::Io)?;
    Ok(bytes)
}

fn classify_root_create_error(root: &Path, _error: &std::io::Error) -> PrivateFsError {
    let absent = matches!(
        fs::symlink_metadata(root),
        Err(observed) if observed.kind() == std::io::ErrorKind::NotFound
    );
    if absent
        && root
            .parent()
            .is_some_and(|parent| validate_directory_chain(parent, false).is_ok())
    {
        PrivateFsError::Io
    } else {
        PrivateFsError::SettlementUnknown
    }
}

fn verify_core_boundary(directory: &DirectoryCore) -> Result<(), PrivateFsError> {
    if !platform::same_open_identity(&directory.handle, directory.identity.0) {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    if let Some(child_name) = &directory.child_name {
        platform::verify_exact_name(&directory.handle, child_name.as_str())
            .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    }
    let (current, identity) =
        platform::open_directory(&directory.path).map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    if identity != directory.identity.0 || !platform::same_open_identity(&current, identity) {
        return Err(PrivateFsError::IdentityAmbiguous);
    }
    if let Some(child_name) = &directory.child_name {
        platform::verify_exact_name(&current, child_name.as_str())
            .map_err(|_| PrivateFsError::IdentityAmbiguous)?;
    }
    Ok(())
}

fn validate_directory_chain(path: &Path, private_leaf: bool) -> Result<(), PrivateFsError> {
    if !path.is_absolute() {
        return Err(PrivateFsError::Unsafe);
    }
    let mut ancestors: Vec<&Path> = path.ancestors().collect();
    ancestors.reverse();
    for current in ancestors {
        if current.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(current).map_err(|_| PrivateFsError::Unsafe)?;
        if private_leaf && current == path {
            platform::validate_private_directory_node(current, &metadata)?;
        } else {
            platform::validate_ancestor_node(current, &metadata)?;
        }
    }
    Ok(())
}

fn private_directory_builder() -> fs::DirBuilder {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
    }
    #[cfg(not(unix))]
    {
        fs::DirBuilder::new()
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    use crate::lease::{CommittedMutationFault, StreamingFault};
    use std::fs::OpenOptions;
    use std::io::Cursor;

    struct RenameOnFirstRead {
        bytes: Cursor<Vec<u8>>,
        source: PathBuf,
        destination: PathBuf,
        renamed: bool,
    }

    impl Read for RenameOnFirstRead {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let read = self.bytes.read(buffer)?;
            if read != 0 && !self.renamed {
                fs::rename(&self.source, &self.destination)?;
                self.renamed = true;
            }
            Ok(read)
        }
    }

    struct RemoveOnFirstRead {
        bytes: Cursor<Vec<u8>>,
        target: PathBuf,
        removed: bool,
    }

    impl Read for RemoveOnFirstRead {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let read = self.bytes.read(buffer)?;
            if read != 0 && !self.removed {
                fs::remove_file(&self.target)?;
                self.removed = true;
            }
            Ok(read)
        }
    }

    #[derive(Clone, Copy)]
    enum EofMutation {
        Append,
        Truncate,
    }

    struct MutateOnEof {
        bytes: Cursor<Vec<u8>>,
        target: PathBuf,
        mutation: EofMutation,
        mutated: bool,
    }

    impl Read for MutateOnEof {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let read = self.bytes.read(buffer)?;
            if read == 0 && !self.mutated {
                let mut options = OpenOptions::new();
                options.write(true);
                match self.mutation {
                    EofMutation::Append => {
                        options.append(true).open(&self.target)?.write_all(b"x")?;
                    }
                    EofMutation::Truncate => {
                        options.truncate(true).open(&self.target)?;
                    }
                }
                self.mutated = true;
            }
            Ok(read)
        }
    }

    fn test_namespace(name: &str) -> (tempfile::TempDir, LockedPrivateNamespace) {
        let parent = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let namespace = LockedPrivateNamespace::open_or_create(parent.path().join(name)).unwrap();
        (parent, namespace)
    }

    #[test]
    fn post_commit_failure_quarantines_the_shared_lease() {
        let (_parent, namespace) = test_namespace("settlement-test");
        namespace.directory.lease.fail_next_settlement();
        assert_eq!(
            namespace.directory.write_new_synced(
                &PrivateComponent::new("value.bin").unwrap(),
                b"value",
                ByteLimit::new(16).unwrap(),
            ),
            Err(PrivateFsError::SettlementUnknown)
        );
        assert_eq!(
            namespace
                .directory
                .regular_exists(&PrivateComponent::new("value.bin").unwrap()),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn poisoned_operation_mutex_stickily_quarantines_the_lease() {
        let (_parent, namespace) = test_namespace("mutex-poison-test");
        let lease = Arc::clone(&namespace.directory.lease);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _operation = lease.begin().unwrap();
            panic!("inject operation panic");
        }));
        assert!(panic.is_err());
        assert_eq!(
            namespace.directory.list_components(8),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn committed_unlink_reported_as_error_has_unknown_sticky_settlement() {
        let (parent, namespace) = test_namespace("unlink-fault-test");
        let file = PrivateComponent::new("value.bin").unwrap();
        namespace
            .directory
            .write_new_synced(&file, b"value", ByteLimit::new(16).unwrap())
            .unwrap();
        namespace
            .directory
            .lease
            .inject_committed_mutation_fault(CommittedMutationFault::Remove);

        assert_eq!(
            namespace.directory.remove_verified_regular(&file),
            Err(PrivateFsError::SettlementUnknown)
        );
        assert!(!parent.path().join("unlink-fault-test/value.bin").exists());
        assert_eq!(
            namespace.directory.list_components(8),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn reported_stream_write_and_sync_failures_cleanly_remove_the_created_entry() {
        let (parent, namespace) = test_namespace("stream-io-cleanup-test");

        for (index, fault) in [
            StreamingFault::Write,
            StreamingFault::FileSync,
            StreamingFault::DirectorySync,
        ]
        .into_iter()
        .enumerate()
        {
            let name = PrivateEntryName::new(format!("Payload {index}.bin")).unwrap();
            namespace.directory.lease.inject_streaming_fault(fault);
            let mut source = Cursor::new(b"value");
            assert_eq!(
                namespace.directory.write_new_entry_from_reader(
                    &name,
                    &mut source,
                    StreamingFileLength::new(5).unwrap(),
                ),
                Err(StreamingWriteError::Filesystem(PrivateFsError::Io))
            );
            assert!(!parent
                .path()
                .join("stream-io-cleanup-test")
                .join(name.as_str())
                .exists());
            assert!(namespace.directory.list_entry_names(8).unwrap().is_empty());

            let mut retry = Cursor::new(b"value");
            namespace
                .directory
                .write_new_entry_from_reader(
                    &name,
                    &mut retry,
                    StreamingFileLength::new(5).unwrap(),
                )
                .unwrap();
            assert!(namespace
                .directory
                .remove_verified_entry_regular(&name)
                .unwrap());
        }
    }

    #[test]
    fn streaming_cleanup_unlink_ambiguity_is_unknown_and_stickily_quarantined() {
        let (parent, namespace) = test_namespace("stream-cleanup-unlink-test");
        let name = PrivateEntryName::new("Payload.bin").unwrap();
        namespace
            .directory
            .lease
            .inject_committed_mutation_fault(CommittedMutationFault::Remove);
        let mut short = Cursor::new(b"four");

        assert_eq!(
            namespace.directory.write_new_entry_from_reader(
                &name,
                &mut short,
                StreamingFileLength::new(5).unwrap(),
            ),
            Err(StreamingWriteError::Filesystem(
                PrivateFsError::SettlementUnknown
            ))
        );
        assert!(!parent
            .path()
            .join("stream-cleanup-unlink-test/Payload.bin")
            .exists());
        assert_eq!(
            namespace.directory.list_entry_names(8),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn streaming_cleanup_directory_sync_ambiguity_is_unknown_and_stickily_quarantined() {
        let (parent, namespace) = test_namespace("stream-cleanup-sync-test");
        let name = PrivateEntryName::new("Payload.bin").unwrap();
        namespace
            .directory
            .lease
            .inject_streaming_fault(StreamingFault::CleanupDirectorySync);
        let mut short = Cursor::new(b"four");

        assert_eq!(
            namespace.directory.write_new_entry_from_reader(
                &name,
                &mut short,
                StreamingFileLength::new(5).unwrap(),
            ),
            Err(StreamingWriteError::Filesystem(
                PrivateFsError::SettlementUnknown
            ))
        );
        assert!(!parent
            .path()
            .join("stream-cleanup-sync-test/Payload.bin")
            .exists());
        assert_eq!(
            namespace.directory.list_entry_names(8),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn bounded_reader_initial_metadata_failure_is_identity_ambiguous_and_quarantines() {
        use std::cell::Cell;

        let (_parent, namespace) = test_namespace("stream-read-metadata-test");
        let name = PrivateEntryName::new("Payload.bin").unwrap();
        namespace
            .directory
            .write_new_entry_synced(&name, b"value", ByteLimit::new(16).unwrap())
            .unwrap();
        namespace
            .directory
            .lease
            .inject_streaming_fault(StreamingFault::InitialReadMetadata);
        let called = Cell::new(false);

        assert_eq!(
            namespace.directory.with_bounded_entry_regular_reader(
                &name,
                ByteLimit::new(16).unwrap(),
                |_reader| {
                    called.set(true);
                    Ok::<_, ()>(())
                },
            ),
            Err(PrivateFsError::IdentityAmbiguous)
        );
        assert!(!called.get());
        assert_eq!(
            namespace.directory.list_entry_names(8),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn final_stream_identity_ambiguity_quarantines_without_unlinking_the_moved_inode() {
        let (parent, namespace) = test_namespace("stream-final-identity-test");
        let root = parent.path().join("stream-final-identity-test");
        let name = PrivateEntryName::new("Payload.bin").unwrap();
        let moved = root.join("Moved Payload.bin");
        let mut source = RenameOnFirstRead {
            bytes: Cursor::new(b"value".to_vec()),
            source: root.join(name.as_str()),
            destination: moved.clone(),
            renamed: false,
        };

        assert_eq!(
            namespace.directory.write_new_entry_from_reader(
                &name,
                &mut source,
                StreamingFileLength::new(5).unwrap(),
            ),
            Err(StreamingWriteError::Filesystem(
                PrivateFsError::SettlementUnknown
            ))
        );
        assert!(moved.exists(), "ambiguous moved inode must not be unlinked");
        assert_eq!(fs::read(moved).unwrap(), b"value");
        assert_eq!(
            namespace.directory.list_entry_names(8),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn final_stream_lock_ambiguity_quarantines_without_attempting_payload_cleanup() {
        let (parent, namespace) = test_namespace("stream-final-lock-test");
        let root = parent.path().join("stream-final-lock-test");
        let name = PrivateEntryName::new("Payload.bin").unwrap();
        let mut source = RemoveOnFirstRead {
            bytes: Cursor::new(b"value".to_vec()),
            target: root.join(LOCK_COMPONENT_NAME),
            removed: false,
        };

        assert_eq!(
            namespace.directory.write_new_entry_from_reader(
                &name,
                &mut source,
                StreamingFileLength::new(5).unwrap(),
            ),
            Err(StreamingWriteError::Filesystem(
                PrivateFsError::SettlementUnknown
            ))
        );
        assert_eq!(
            fs::read(root.join(name.as_str())).unwrap(),
            b"value",
            "payload must remain when lock authority is already ambiguous"
        );
        assert_eq!(
            namespace.directory.list_entry_names(8),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn failed_stream_with_lost_lock_quarantines_without_cleanup_mutation() {
        let (parent, namespace) = test_namespace("stream-failed-lock-test");
        let root = parent.path().join("stream-failed-lock-test");
        let name = PrivateEntryName::new("Payload.bin").unwrap();
        let mut source = RemoveOnFirstRead {
            bytes: Cursor::new(b"val".to_vec()),
            target: root.join(LOCK_COMPONENT_NAME),
            removed: false,
        };

        assert_eq!(
            namespace.directory.write_new_entry_from_reader(
                &name,
                &mut source,
                StreamingFileLength::new(5).unwrap(),
            ),
            Err(StreamingWriteError::Filesystem(
                PrivateFsError::SettlementUnknown
            ))
        );
        assert_eq!(
            fs::read(root.join(name.as_str())).unwrap(),
            b"val",
            "partial payload must remain when cleanup authority is ambiguous"
        );
        assert_eq!(
            namespace.directory.list_entry_names(8),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn exact_stream_length_is_revalidated_after_the_hostile_eof_probe() {
        let (parent, namespace) = test_namespace("stream-eof-mutation-test");
        let root = parent.path().join("stream-eof-mutation-test");

        for (entry, mutation) in [
            ("Appended.bin", EofMutation::Append),
            ("Truncated.bin", EofMutation::Truncate),
        ] {
            let name = PrivateEntryName::new(entry).unwrap();
            let target = root.join(name.as_str());
            let mut source = MutateOnEof {
                bytes: Cursor::new(b"value".to_vec()),
                target: target.clone(),
                mutation,
                mutated: false,
            };

            assert_eq!(
                namespace.directory.write_new_entry_from_reader(
                    &name,
                    &mut source,
                    StreamingFileLength::new(5).unwrap(),
                ),
                Err(StreamingWriteError::SinkLengthMismatch)
            );
            assert!(
                !target.exists(),
                "length-mismatched held inode must be durably cleaned"
            );
            assert_eq!(namespace.directory.list_entry_names(8).unwrap(), vec![]);
        }
    }

    #[test]
    fn committed_replace_reported_as_error_has_unknown_sticky_settlement() {
        let (parent, namespace) = test_namespace("replace-fault-test");
        let source = PrivateComponent::new("stage.bin").unwrap();
        let destination = PrivateComponent::new("current.bin").unwrap();
        namespace
            .directory
            .write_new_synced(&source, b"next", ByteLimit::new(16).unwrap())
            .unwrap();
        namespace
            .directory
            .write_new_synced(&destination, b"old", ByteLimit::new(16).unwrap())
            .unwrap();
        namespace
            .directory
            .lease
            .inject_committed_mutation_fault(CommittedMutationFault::Replace);

        assert_eq!(
            namespace
                .directory
                .replace_verified_regular(&source, &destination),
            Err(PrivateFsError::SettlementUnknown)
        );
        assert!(!parent.path().join("replace-fault-test/stage.bin").exists());
        assert_eq!(
            namespace.directory.regular_exists(&destination),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn committed_noreplace_reported_as_error_has_unknown_sticky_settlement() {
        let (parent, namespace) = test_namespace("noreplace-fault-test");
        let source = PrivateComponent::new("stage.bin").unwrap();
        let destination = PrivateComponent::new("current.bin").unwrap();
        namespace
            .directory
            .write_new_synced(&source, b"next", ByteLimit::new(16).unwrap())
            .unwrap();
        namespace
            .directory
            .lease
            .inject_committed_mutation_fault(CommittedMutationFault::PublishNoreplace);

        assert_eq!(
            namespace
                .directory
                .publish_noreplace_verified_regular(&source, &destination),
            Err(PrivateFsError::SettlementUnknown)
        );
        assert!(!parent
            .path()
            .join("noreplace-fault-test/stage.bin")
            .exists());
        assert_eq!(
            namespace.directory.regular_exists(&destination),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn ambiguous_child_creation_stickily_quarantines_the_lease() {
        let (parent, namespace) = test_namespace("mkdir-fault-test");
        let child = PrivateComponent::new("objects").unwrap();
        namespace
            .directory
            .lease
            .inject_committed_mutation_fault(CommittedMutationFault::CreateDirectory);

        assert_eq!(
            namespace.directory.create_private_child(&child).err(),
            Some(PrivateFsError::SettlementUnknown)
        );
        assert!(parent.path().join("mkdir-fault-test/objects").is_dir());
        assert_eq!(
            namespace.directory.list_components(8),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn ambiguous_entry_child_creation_stickily_quarantines_the_lease() {
        let (parent, namespace) = test_namespace("entry-mkdir-fault-test");
        let child = PrivateEntryName::new("Objects With Case").unwrap();
        namespace
            .directory
            .lease
            .inject_committed_mutation_fault(CommittedMutationFault::CreateDirectory);

        assert_eq!(
            namespace.directory.create_new_entry_child(&child).err(),
            Some(PrivateFsError::SettlementUnknown)
        );
        assert!(parent
            .path()
            .join("entry-mkdir-fault-test/Objects With Case")
            .is_dir());
        assert_eq!(
            namespace.directory.list_entry_names(8),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn failed_root_create_is_clean_only_when_the_leaf_is_proven_absent() {
        let parent = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let root = parent.path().join("root-create-fault");
        let injected = std::io::Error::from_raw_os_error(rustix::io::Errno::IO.raw_os_error());

        assert_eq!(
            classify_root_create_error(&root, &injected),
            PrivateFsError::Io
        );
        fs::create_dir(&root).unwrap();
        assert_eq!(
            classify_root_create_error(&root, &injected),
            PrivateFsError::SettlementUnknown
        );
    }

    #[test]
    fn published_lock_with_failed_root_revalidation_is_settlement_unknown() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let root = parent.path().join("lock-publish-settlement-test");
        private_directory_builder().create(&root).unwrap();
        let directory = admit_root(root.clone()).unwrap();
        let lock_name = canonical_lock_component().unwrap();
        let staging_name = lock_staging_component().unwrap();
        let (file, identity) =
            platform::create_new_regular(&directory.handle, &directory.path, staging_name.as_str())
                .unwrap();
        let mut staging = VerifiedRegular {
            file,
            identity: FileIdentity(identity),
        };
        let mut mutated = true;
        prepare_staged_lock(&directory, &staging_name, &mut staging, &mut mutated).unwrap();
        platform::atomic_publish_noreplace(
            &directory.handle,
            &directory.path,
            staging_name.as_str(),
            lock_name.as_str(),
        )
        .unwrap();

        let moved = parent.path().join("moved-lock-publish-settlement-test");
        fs::rename(&root, moved).unwrap();
        private_directory_builder().create(&root).unwrap();

        assert_eq!(
            settle_published_lock(&directory, &lock_name, &staging_name, staging).err(),
            Some(PrivateFsError::SettlementUnknown)
        );
    }

    #[test]
    fn staging_cleanup_with_failed_root_revalidation_is_settlement_unknown() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let root = parent.path().join("staging-cleanup-settlement-test");
        private_directory_builder().create(&root).unwrap();
        let directory = admit_root(root.clone()).unwrap();
        let lock_name = canonical_lock_component().unwrap();
        let staging_name = lock_staging_component().unwrap();
        let mut lock = acquire_lock(&directory, &lock_name, &staging_name).unwrap();
        let (mut staging, _) =
            platform::create_new_regular(&directory.handle, &directory.path, staging_name.as_str())
                .unwrap();
        staging.write_all(LOCK_FILE_CONTENT).unwrap();
        staging.sync_all().unwrap();
        drop(staging);

        let moved = parent.path().join("moved-staging-cleanup-settlement-test");
        fs::rename(&root, &moved).unwrap();
        private_directory_builder().create(&root).unwrap();

        assert_eq!(
            inspect_and_cleanup_lock_staging(&directory, &lock_name, &mut lock, &staging_name,),
            Err(PrivateFsError::SettlementUnknown)
        );
        assert!(!moved.join(staging_name.as_str()).exists());
    }
}
