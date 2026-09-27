//! Bounded package storage and collection. Installed roots come from the
//! exclusive Store coordinator; native reservations are retained independently.
use super::*;
use std::{collections::BTreeSet, sync::atomic::Ordering, time::Instant};
use zephium_core::extensions::{ExtensionBetaObjectDigest, ExtensionNativePackageSource};
use zephium_private_fs::{
    ByteLimit, OpenedPrivateDirectory, PrivateDirectory, SealedPrivateDirectory, TreeRemovalLimits,
};

/// Aggregate logical bytes for original packages, transformed trees and controls.
pub const MAX_BETA_REPOSITORY_BYTES: u64 = 512 * 1024 * 1024;
/// Independent aggregate filesystem-entry bound, including directories.
pub const MAX_BETA_REPOSITORY_ENTRIES: usize = 65_536;
const COLLECTION_BATCH: usize = 8;
const SLOT_ENTRIES: usize = 2 * zephium_extension_package::MAX_EXTENSION_TREE_ENTRIES + 16;
const SLOT_DEPTH: usize = zephium_extension_package::MAX_EXTENSION_RELATIVE_PATH_DEPTH + 3;
const FILE_BYTES: usize = zephium_core::extensions::MAX_EXTENSION_ARCHIVE_BYTES as usize
    + zephium_extension_package::MAX_CRX3_HEADER_BYTES
    + 12;

/// Metadata-only physical inventory of logical lengths and entry counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BetaStorageUsage {
    bytes: u64,
    entries: usize,
}
impl BetaStorageUsage {
    /// Sum of verified file lengths. No executable payload is read to measure it.
    pub const fn bytes(self) -> u64 {
        self.bytes
    }
    /// Files and directories beneath immutable object names.
    pub const fn entries(self) -> usize {
        self.entries
    }
}
/// Result of one bounded collection batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BetaStorageCollection {
    removed: usize,
    more: bool,
}
impl BetaStorageCollection {
    /// Objects durably removed in this batch.
    pub const fn removed(self) -> usize {
        self.removed
    }
    /// Whether another low-frequency batch can reclaim more objects.
    pub const fn has_more(self) -> bool {
        self.more
    }
}

impl BetaPackageRepository {
    /// Measures all current storage using verified metadata, without loading scripts.
    pub fn storage_usage(&mut self) -> Result<BetaStorageUsage, BetaRepositoryError> {
        let runtime = self.runtime.clone();
        let _operation = runtime
            .enter()
            .map_err(native::repository_operation_error)?;
        self.check()?;
        measure(self)
    }
    /// Collects only objects absent from a complete, fresh Store retention
    /// snapshot and from every live native reservation. The exclusive coordinator
    /// must include disabled installs, pending reviews and unresolved journal rows,
    /// and must not interleave new ownership/install mutations before this call.
    /// No destructor performs collection. Interrupted deletion remains an
    /// unreferenced slot for the next process to finish under the same rules.
    pub fn collect_unreferenced(
        &mut self,
        roots: &[ExtensionBetaObjectDigest],
        deadline: Instant,
    ) -> Result<BetaStorageCollection, BetaRepositoryError> {
        let runtime = self.runtime.clone();
        let _operation = runtime
            .enter()
            .map_err(native::repository_operation_error)?;
        self.check()?;
        if roots.len() > MAX_BETA_REPOSITORY_SLOTS + 3 {
            return Err(BetaRepositoryError::Capacity);
        }
        let mut retained: BTreeSet<_> = roots
            .iter()
            .map(|id| BetaPackageObjectId(id.bytes()))
            .collect();
        {
            let reservations = self
                .reservations
                .lock()
                .map_err(|_| BetaRepositoryError::Quarantined)?;
            for reservation in reservations
                .values()
                .filter(|reservation| reservation.active.load(Ordering::Acquire))
            {
                let ExtensionNativePackageSource::BetaObject(id) = reservation.entry.source()
                else {
                    return Err(BetaRepositoryError::Integrity);
                };
                retained.insert(BetaPackageObjectId(id.bytes()));
            }
        }
        let garbage: Vec<_> = self
            .slots
            .keys()
            .filter(|id| !retained.contains(id))
            .copied()
            .collect();
        let mut removed = 0;
        for id in garbage.iter().take(COLLECTION_BATCH) {
            if Instant::now() >= deadline {
                break;
            }
            if let Some(slot) = self.slots.get_mut(id) {
                drop(slot.take());
            }
            let directory = self
                .namespace
                .directory()
                .open_private_child_any_mode(&id.component())
                .map_err(|error| match error {
                    zephium_private_fs::PrivateFsError::InUse
                    | zephium_private_fs::PrivateFsError::Io
                    | zephium_private_fs::PrivateFsError::LockUnavailable => {
                        BetaRepositoryError::StorageUnavailable
                    }
                    _ => BetaRepositoryError::Storage,
                })?;
            if let Err(error) = directory.remove_tree_bounded(
                TreeRemovalLimits::new(SLOT_ENTRIES, zephium_private_fs::MAX_TREE_REMOVAL_DEPTH)
                    .map_err(|_| BetaRepositoryError::Capacity)?,
            ) {
                let (reason, unchanged) = error.into_parts();
                if reason == zephium_private_fs::PrivateFsError::BoundExceeded
                    && unchanged.is_some()
                {
                    // The two artifact wrappers can put a valid deepest source
                    // beyond the generic tree-remover's depth. Remove the known
                    // artifact layout at its extension root using its existing
                    // bounded recovery path, then remove the empty slot.
                    drop(unchanged);
                    self.workspace(*id)?
                        .discard()
                        .map_err(BetaRepositoryError::Admission)?;
                    if let Some(slot) = self.slots.get_mut(id) {
                        drop(slot.take());
                    }
                    let result = self
                        .namespace
                        .directory()
                        .remove_empty_private_child(&id.component())
                        .map_err(|_| BetaRepositoryError::Storage)
                        .and_then(|removed| {
                            removed.then_some(()).ok_or(BetaRepositoryError::Integrity)
                        });
                    self.settle(result)?;
                    self.slots.remove(id);
                    removed += 1;
                    continue;
                }

                if unchanged.is_some()
                    && matches!(
                        reason,
                        zephium_private_fs::PrivateFsError::InUse
                            | zephium_private_fs::PrivateFsError::Io
                            | zephium_private_fs::PrivateFsError::LockUnavailable
                    )
                {
                    return Err(BetaRepositoryError::StorageUnavailable);
                }
                return self.settle(Err(BetaRepositoryError::Storage));
            }
            self.slots.remove(id);
            removed += 1;
        }
        Ok(BetaStorageCollection {
            removed,
            more: removed < garbage.len(),
        })
    }
}

pub(super) fn measure(
    repository: &BetaPackageRepository,
) -> Result<BetaStorageUsage, BetaRepositoryError> {
    let mut usage = BetaStorageUsage::default();
    for id in repository.slots.keys() {
        let directory = repository
            .namespace
            .directory()
            .open_private_child_any_mode(&id.component())
            .map_err(|_| BetaRepositoryError::Storage)?;
        visit(directory, 0, &mut usage)?;
    }
    Ok(usage)
}
enum Directory<'a> {
    Writable(&'a PrivateDirectory),
    Sealed(&'a SealedPrivateDirectory),
}
impl Directory<'_> {
    fn names(&self) -> Result<Vec<PrivateEntryName>, zephium_private_fs::PrivateFsError> {
        match self {
            Self::Writable(dir) => {
                dir.list_entry_names(zephium_extension_package::MAX_EXTENSION_TREE_ENTRIES)
            }
            Self::Sealed(dir) => {
                dir.list_entry_names(zephium_extension_package::MAX_EXTENSION_TREE_ENTRIES)
            }
        }
    }
    fn kind(
        &self,
        name: &PrivateEntryName,
    ) -> Result<Option<PrivateChildKind>, zephium_private_fs::PrivateFsError> {
        match self {
            Self::Writable(dir) => dir.inspect_entry(name),
            Self::Sealed(dir) => dir.inspect_entry(name),
        }
    }
    fn length(
        &self,
        name: &PrivateEntryName,
    ) -> Result<Option<u64>, zephium_private_fs::PrivateFsError> {
        let limit = ByteLimit::new(FILE_BYTES)?;
        match self {
            Self::Writable(dir) => dir.entry_regular_length(name, limit),
            Self::Sealed(dir) => dir.entry_regular_length(name, limit),
        }
    }
    fn child(
        &self,
        name: &PrivateEntryName,
    ) -> Result<OpenedPrivateDirectory, zephium_private_fs::PrivateFsError> {
        match self {
            Self::Writable(dir) => dir.open_entry_child_any_mode(name),
            Self::Sealed(dir) => dir
                .open_sealed_entry_child(name)
                .map(OpenedPrivateDirectory::Sealed),
        }
    }
}
fn visit(
    opened: OpenedPrivateDirectory,
    depth: usize,
    usage: &mut BetaStorageUsage,
) -> Result<(), BetaRepositoryError> {
    if depth > SLOT_DEPTH {
        return Err(BetaRepositoryError::Capacity);
    }
    usage.entries += 1;
    if usage.entries > MAX_BETA_REPOSITORY_ENTRIES {
        return Err(BetaRepositoryError::Capacity);
    }
    let directory = match &opened {
        OpenedPrivateDirectory::Writable(dir) => Directory::Writable(dir),
        OpenedPrivateDirectory::Sealed(dir) => Directory::Sealed(dir),
    };
    for name in directory
        .names()
        .map_err(|_| BetaRepositoryError::Storage)?
    {
        match directory
            .kind(&name)
            .map_err(|_| BetaRepositoryError::Storage)?
            .ok_or(BetaRepositoryError::Integrity)?
        {
            PrivateChildKind::Directory(_) => visit(
                directory
                    .child(&name)
                    .map_err(|_| BetaRepositoryError::Storage)?,
                depth + 1,
                usage,
            )?,
            PrivateChildKind::RegularFile(_) => {
                let length = directory
                    .length(&name)
                    .map_err(|_| BetaRepositoryError::Storage)?
                    .ok_or(BetaRepositoryError::Integrity)?;
                usage.bytes = usage
                    .bytes
                    .checked_add(length)
                    .ok_or(BetaRepositoryError::Capacity)?;
                usage.entries += 1;
                if usage.bytes > MAX_BETA_REPOSITORY_BYTES
                    || usage.entries > MAX_BETA_REPOSITORY_ENTRIES
                {
                    return Err(BetaRepositoryError::Capacity);
                }
            }
        }
    }
    Ok(())
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    #[test]
    fn a_pre_mutation_busy_refusal_preserves_the_repository_for_retry() {
        let temporary = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let namespace =
            LockedPrivateNamespace::open_or_create(temporary.path().join("objects")).unwrap();
        let id = BetaPackageObjectId([7; 32]);
        let orphan = namespace
            .directory()
            .create_new_private_child(&id.component())
            .unwrap();
        let mut repository = BetaPackageRepository::open(namespace).unwrap();
        let outcome = orphan
            .with_verified_path(|_| {
                repository
                    .collect_unreferenced(&[], Instant::now() + std::time::Duration::from_secs(5))
            })
            .unwrap();
        assert_eq!(outcome, Err(BetaRepositoryError::StorageUnavailable));
        assert_eq!(repository.inventory().unwrap(), vec![id]);
        drop(orphan);
        assert_eq!(
            repository
                .collect_unreferenced(&[], Instant::now() + std::time::Duration::from_secs(5))
                .unwrap()
                .removed(),
            1
        );
        assert!(repository.inventory().unwrap().is_empty());
    }
}
