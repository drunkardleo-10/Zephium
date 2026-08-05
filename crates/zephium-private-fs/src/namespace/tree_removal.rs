//! Linear high-level private-tree removal.

use super::OpenedPrivateDirectory;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::{terminal_settlement_error, verify_core_boundary, verify_parent_authority};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::platform::{self, DirectoryMode, TreeRemovalFaults};
use crate::{PrivateFsError, PrivateFsTransitionError};

/// Hard ceiling for entries observed by one bounded tree removal.
///
/// The private-filesystem layer intentionally permits callers with larger
/// policy cohorts while bounding its complete preflight plan to O(entries).
/// Subsystems should pass their smaller authenticated format ceiling.
pub const MAX_TREE_REMOVAL_ENTRIES: usize = 32_768;

/// Hard ceiling for descendant depth observed by one bounded tree removal.
pub const MAX_TREE_REMOVAL_DEPTH: usize = 32;

/// Validated resource limits for one private-tree removal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TreeRemovalLimits {
    max_entries: usize,
    max_depth: usize,
}

impl TreeRemovalLimits {
    /// Creates limits within the fixed crate-wide ceilings.
    ///
    /// Zero is meaningful independently for proving an authenticated empty
    /// tree without widening either policy bound.
    pub fn new(max_entries: usize, max_depth: usize) -> Result<Self, PrivateFsError> {
        if max_entries > MAX_TREE_REMOVAL_ENTRIES || max_depth > MAX_TREE_REMOVAL_DEPTH {
            return Err(PrivateFsError::BoundExceeded);
        }
        Ok(Self {
            max_entries,
            max_depth,
        })
    }

    /// Returns the maximum number of descendants that may be observed.
    #[must_use]
    pub const fn max_entries(self) -> usize {
        self.max_entries
    }

    /// Returns the maximum descendant depth, with direct children at depth one.
    #[must_use]
    pub const fn max_depth(self) -> usize {
        self.max_depth
    }
}

/// Exact work settled by one successful bounded tree removal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TreeRemovalReport {
    observed_entries: usize,
    maximum_depth: usize,
    regular_files_removed: usize,
    directories_removed: usize,
    directories_unsealed: usize,
    directory_syncs: usize,
}

impl TreeRemovalReport {
    /// Returns removed descendants, excluding the consumed root directory.
    #[must_use]
    pub const fn observed_entries(self) -> usize {
        self.observed_entries
    }

    /// Returns the deepest observed descendant level, with root-only as zero.
    #[must_use]
    pub const fn maximum_depth(self) -> usize {
        self.maximum_depth
    }

    /// Returns removed regular descendants.
    #[must_use]
    pub const fn regular_files_removed(self) -> usize {
        self.regular_files_removed
    }

    /// Returns removed directories, including the consumed root directory.
    #[must_use]
    pub const fn directories_removed(self) -> usize {
        self.directories_removed
    }

    /// Returns directories changed from sealed to writable before removal.
    #[must_use]
    pub const fn directories_unsealed(self) -> usize {
        self.directories_unsealed
    }

    /// Returns durability flushes, including the final immediate-parent flush.
    #[must_use]
    pub const fn directory_syncs(self) -> usize {
        self.directory_syncs
    }
}

impl OpenedPrivateDirectory {
    /// Removes this exact private directory tree within fixed entry/depth limits.
    ///
    /// The whole shape is admitted before the first mutation. Descendants are
    /// traversed in byte-sorted order through held descriptors without following
    /// links. Writable and sealed nodes may be mixed. Descendant mutations are
    /// intentionally not flushed: interrupted residue remains unreachable under
    /// the caller's already-hidden retired root and is safe to resume. Successful
    /// return follows root removal with exactly one flush of its captured parent,
    /// proving the whole retired name durably absent without O(tree) flushes.
    ///
    /// A clean pre-mutation refusal returns this unchanged capability. Once any
    /// chmod or unlink may have committed, every failure is terminal,
    /// quarantines the namespace, and reports [`PrivateFsError::SettlementUnknown`].
    pub fn remove_tree_bounded(
        self,
        limits: TreeRemovalLimits,
    ) -> Result<TreeRemovalReport, PrivateFsTransitionError<Self>> {
        remove_tree_bounded_platform(self, limits)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn remove_tree_bounded_platform(
    directory: OpenedPrivateDirectory,
    _limits: TreeRemovalLimits,
) -> Result<TreeRemovalReport, PrivateFsTransitionError<OpenedPrivateDirectory>> {
    Err(PrivateFsTransitionError::recoverable(
        PrivateFsError::PrimitiveUnavailable,
        directory,
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn remove_tree_bounded_platform(
    directory: OpenedPrivateDirectory,
    limits: TreeRemovalLimits,
) -> Result<TreeRemovalReport, PrivateFsTransitionError<OpenedPrivateDirectory>> {
    let (core, lease, mode) = match &directory {
        OpenedPrivateDirectory::Writable(directory) => {
            (&directory.core, &directory.lease, DirectoryMode::Writable)
        }
        OpenedPrivateDirectory::Sealed(directory) => {
            (&directory.core, &directory.lease, DirectoryMode::Sealed)
        }
    };
    let operation = match lease.begin() {
        Ok(operation) => operation,
        Err(error) => return Err(PrivateFsTransitionError::terminal(error)),
    };
    if let Err(error) = lease.observe(
        lease
            .verify_authority()
            .and_then(|()| verify_core_boundary(core, mode)),
    ) {
        drop(operation);
        return Err(PrivateFsTransitionError::terminal(error));
    }
    if let Err(error) = lease.mutation_allowed() {
        drop(operation);
        return Err(PrivateFsTransitionError::recoverable(error, directory));
    }
    let Some(parent) = core.parent.as_ref() else {
        drop(operation);
        return Err(PrivateFsTransitionError::recoverable(
            PrivateFsError::Unsafe,
            directory,
        ));
    };
    if parent.mode != DirectoryMode::Writable {
        drop(operation);
        return Err(PrivateFsTransitionError::recoverable(
            PrivateFsError::Unsafe,
            directory,
        ));
    }

    let faults = TreeRemovalFaults {
        #[cfg(test)]
        fail_preexecution_identity: lease.take_tree_removal_preexecution_identity_fault(),
        #[cfg(not(test))]
        fail_preexecution_identity: false,
        #[cfg(test)]
        fail_at_mutation: lease.take_tree_removal_mutation_fault_ordinal(),
        #[cfg(not(test))]
        fail_at_mutation: None,
        #[cfg(test)]
        fail_final_parent_sync: lease.take_tree_removal_final_parent_sync_fault(),
        #[cfg(not(test))]
        fail_final_parent_sync: false,
    };
    let removal = platform::remove_tree_bounded(
        &core.handle,
        &core.path,
        core.identity.0,
        mode,
        &parent.handle,
        &parent.path,
        parent.child_name.as_str(),
        limits.max_entries,
        limits.max_depth,
        faults,
    );
    match removal {
        Ok(report) => {
            let settled = verify_parent_authority(parent).and_then(|()| lease.verify_authority());
            drop(operation);
            if settled.is_err() {
                return Err(PrivateFsTransitionError::terminal(
                    terminal_settlement_error(lease),
                ));
            }
            Ok(TreeRemovalReport {
                observed_entries: report.observed_entries,
                maximum_depth: report.maximum_depth,
                regular_files_removed: report.regular_files_removed,
                directories_removed: report.directories_removed,
                directories_unsealed: report.directories_unsealed,
                directory_syncs: report.directory_syncs,
            })
        }
        Err(failure) if failure.mutation_started => {
            let _ = failure.error;
            drop(operation);
            Err(PrivateFsTransitionError::terminal(
                terminal_settlement_error(lease),
            ))
        }
        Err(failure) => {
            if failure.error == PrivateFsError::IdentityAmbiguous {
                let error = lease
                    .observe::<()>(Err(failure.error))
                    .err()
                    .unwrap_or(failure.error);
                drop(operation);
                return Err(PrivateFsTransitionError::terminal(error));
            }
            let unchanged =
                verify_core_boundary(core, mode).and_then(|()| lease.verify_authority());
            drop(operation);
            if unchanged.is_ok() {
                Err(PrivateFsTransitionError::recoverable(
                    failure.error,
                    directory,
                ))
            } else {
                Err(PrivateFsTransitionError::terminal(
                    terminal_settlement_error(lease),
                ))
            }
        }
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    use std::os::unix::net::UnixListener;

    use tempfile::TempDir;

    use super::*;
    use crate::{ByteLimit, LockedPrivateNamespace, PrivateComponent, PrivateEntryName};

    fn component(name: &str) -> PrivateComponent {
        PrivateComponent::new(name).unwrap()
    }

    fn entry(name: &str) -> PrivateEntryName {
        PrivateEntryName::new(name).unwrap()
    }

    fn test_namespace(name: &str) -> (TempDir, LockedPrivateNamespace) {
        #[cfg(target_os = "macos")]
        let parent = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let parent = tempfile::tempdir_in("/tmp").unwrap();
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let namespace = LockedPrivateNamespace::open_or_create(parent.path().join(name)).unwrap();
        (parent, namespace)
    }

    fn write_entry(directory: &super::super::PrivateDirectory, name: &str, sealed: bool) {
        let name = entry(name);
        directory
            .write_new_entry_synced(&name, b"payload", ByteLimit::new(32).unwrap())
            .unwrap();
        if sealed {
            assert!(directory
                .seal_verified_entry_regular(&name)
                .unwrap()
                .is_some());
        }
    }

    #[test]
    fn limits_validate_the_fixed_resource_ceiling() {
        assert_eq!(
            TreeRemovalLimits::new(1, 1).unwrap(),
            TreeRemovalLimits {
                max_entries: 1,
                max_depth: 1,
            }
        );
        assert_eq!(
            TreeRemovalLimits::new(0, 0).unwrap(),
            TreeRemovalLimits {
                max_entries: 0,
                max_depth: 0,
            }
        );
        for (entries, depth) in [
            (MAX_TREE_REMOVAL_ENTRIES + 1, 1),
            (1, MAX_TREE_REMOVAL_DEPTH + 1),
        ] {
            assert_eq!(
                TreeRemovalLimits::new(entries, depth),
                Err(PrivateFsError::BoundExceeded)
            );
        }
    }

    #[test]
    fn mixed_modes_remove_with_one_final_parent_sync() {
        let (parent, namespace) = test_namespace("tree-removal-mixed");
        let tree_name = component("tree");
        let tree = namespace
            .directory
            .create_new_private_child(&tree_name)
            .unwrap();
        write_entry(&tree, "Zulu.js", false);
        write_entry(&tree, "Alpha.js", true);

        let sealed_child = tree.create_new_entry_child(&entry("Sealed Child")).unwrap();
        write_entry(&sealed_child, "Manifest.json", true);
        let sealed_child = sealed_child.seal().unwrap();
        drop(sealed_child);

        let writable_child = tree
            .create_new_entry_child(&entry("Writable Child"))
            .unwrap();
        write_entry(&writable_child, "Payload.bin", false);
        drop(writable_child);

        let report = OpenedPrivateDirectory::Writable(tree)
            .remove_tree_bounded(TreeRemovalLimits::new(6, 2).unwrap())
            .unwrap();
        assert_eq!(report.observed_entries(), 6);
        assert_eq!(report.maximum_depth(), 2);
        assert_eq!(report.regular_files_removed(), 4);
        assert_eq!(report.directories_removed(), 3);
        assert_eq!(report.directories_unsealed(), 1);
        assert_eq!(report.directory_syncs(), 1);
        assert!(!parent.path().join("tree-removal-mixed/tree").exists());
    }

    #[test]
    fn empty_root_settles_with_only_the_final_parent_sync() {
        let (parent, namespace) = test_namespace("tree-removal-empty");
        let tree = namespace
            .directory
            .create_new_private_child(&component("tree"))
            .unwrap();
        let tree = tree.seal().unwrap();
        let report = OpenedPrivateDirectory::Sealed(tree)
            .remove_tree_bounded(TreeRemovalLimits::new(0, 0).unwrap())
            .unwrap();
        assert_eq!(report.observed_entries(), 0);
        assert_eq!(report.maximum_depth(), 0);
        assert_eq!(report.regular_files_removed(), 0);
        assert_eq!(report.directories_removed(), 1);
        assert_eq!(report.directories_unsealed(), 0);
        assert_eq!(report.directory_syncs(), 1);
        assert!(!parent.path().join("tree-removal-empty/tree").exists());
    }

    #[test]
    fn flat_4096_entry_tree_still_uses_one_directory_sync() {
        let (parent, namespace) = test_namespace("tree-removal-flat-max");
        let tree = namespace
            .directory
            .create_new_private_child(&component("tree"))
            .unwrap();
        let tree_path = parent.path().join("tree-removal-flat-max/tree");
        for index in 0..4_096_u16 {
            let path = tree_path.join(format!("Entry-{index:04}.bin"));
            fs::write(&path, b"x").unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let report = OpenedPrivateDirectory::Writable(tree)
            .remove_tree_bounded(TreeRemovalLimits::new(4_096, 1).unwrap())
            .unwrap();
        assert_eq!(report.observed_entries(), 4_096);
        assert_eq!(report.maximum_depth(), 1);
        assert_eq!(report.regular_files_removed(), 4_096);
        assert_eq!(report.directories_removed(), 1);
        assert_eq!(report.directories_unsealed(), 0);
        assert_eq!(report.directory_syncs(), 1);
        assert!(!tree_path.exists());
    }

    #[test]
    fn entry_and_depth_bounds_refuse_cleanly_before_mutation() {
        let (_parent, namespace) = test_namespace("tree-removal-bounds");
        let tree = namespace
            .directory
            .create_new_private_child(&component("tree"))
            .unwrap();
        write_entry(&tree, "A", false);
        write_entry(&tree, "B", false);
        let error = OpenedPrivateDirectory::Writable(tree)
            .remove_tree_bounded(TreeRemovalLimits::new(1, 2).unwrap())
            .unwrap_err();
        let (kind, state) = error.into_parts();
        assert_eq!(kind, PrivateFsError::BoundExceeded);
        let tree = state.expect("entry-bound refusal must return the unchanged capability");
        assert_eq!(
            match &tree {
                OpenedPrivateDirectory::Writable(directory) => directory
                    .list_entry_names(4)
                    .unwrap()
                    .into_iter()
                    .map(|name| name.as_str().to_owned())
                    .collect::<Vec<_>>(),
                OpenedPrivateDirectory::Sealed(_) => unreachable!(),
            },
            vec!["A".to_owned(), "B".to_owned()]
        );
        drop(tree);

        let nested = namespace
            .directory
            .create_new_private_child(&component("nested"))
            .unwrap();
        let child = nested.create_new_entry_child(&entry("Child")).unwrap();
        write_entry(&child, "Leaf", false);
        drop(child);
        let error = OpenedPrivateDirectory::Writable(nested)
            .remove_tree_bounded(TreeRemovalLimits::new(3, 1).unwrap())
            .unwrap_err();
        assert_eq!(error.error(), PrivateFsError::BoundExceeded);
        assert!(error.is_recoverable());
    }

    #[test]
    fn links_special_nodes_and_preexecution_identity_races_fail_closed() {
        for hostile in ["symlink", "hardlink", "special"] {
            let (parent, namespace) = test_namespace(&format!("tree-removal-{hostile}"));
            let tree = namespace
                .directory
                .create_new_private_child(&component("tree"))
                .unwrap();
            let tree_path = parent.path().join(format!("tree-removal-{hostile}/tree"));
            let _special = match hostile {
                "symlink" => {
                    symlink("missing", tree_path.join("Hostile")).unwrap();
                    None
                }
                "hardlink" => {
                    fs::write(tree_path.join("Primary"), b"payload").unwrap();
                    fs::set_permissions(
                        tree_path.join("Primary"),
                        fs::Permissions::from_mode(0o600),
                    )
                    .unwrap();
                    fs::hard_link(tree_path.join("Primary"), tree_path.join("Alias")).unwrap();
                    None
                }
                "special" => match UnixListener::bind(tree_path.join("Socket")) {
                    Ok(listener) => Some(listener),
                    Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => continue,
                    Err(error) => panic!("failed to create special-node fixture: {error}"),
                },
                _ => unreachable!(),
            };
            let error = OpenedPrivateDirectory::Writable(tree)
                .remove_tree_bounded(TreeRemovalLimits::new(8, 2).unwrap())
                .unwrap_err();
            assert_eq!(error.error(), PrivateFsError::Unsafe, "{hostile}");
            assert!(error.is_recoverable(), "{hostile}");
            assert!(tree_path.exists(), "{hostile}");
        }

        let (_parent, namespace) = test_namespace("tree-removal-identity-race");
        let tree = namespace
            .directory
            .create_new_private_child(&component("tree"))
            .unwrap();
        write_entry(&tree, "Payload", false);
        tree.lease.inject_tree_removal_preexecution_identity_fault();
        let error = OpenedPrivateDirectory::Writable(tree)
            .remove_tree_bounded(TreeRemovalLimits::new(2, 1).unwrap())
            .unwrap_err();
        assert_eq!(error.error(), PrivateFsError::IdentityAmbiguous);
        assert!(!error.is_recoverable());
        assert_eq!(
            namespace.directory.list_components(4),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn byte_sorted_first_mutation_and_final_sync_failures_are_terminal() {
        let (parent, namespace) = test_namespace("tree-removal-order-fault");
        let tree = namespace
            .directory
            .create_new_private_child(&component("tree"))
            .unwrap();
        write_entry(&tree, "Zulu", false);
        write_entry(&tree, "Alpha", false);
        tree.lease.inject_tree_removal_mutation_fault(1);
        let error = OpenedPrivateDirectory::Writable(tree)
            .remove_tree_bounded(TreeRemovalLimits::new(2, 1).unwrap())
            .unwrap_err();
        assert_eq!(error.error(), PrivateFsError::SettlementUnknown);
        assert!(!error.is_recoverable());
        let tree_path = parent.path().join("tree-removal-order-fault/tree");
        assert!(!tree_path.join("Alpha").exists());
        assert!(tree_path.join("Zulu").exists());
        assert_eq!(
            namespace.directory.list_components(4),
            Err(PrivateFsError::Quarantined)
        );

        let (parent, namespace) = test_namespace("tree-removal-parent-sync-fault");
        let tree = namespace
            .directory
            .create_new_private_child(&component("tree"))
            .unwrap();
        tree.lease.inject_tree_removal_final_parent_sync_fault();
        let error = OpenedPrivateDirectory::Writable(tree)
            .remove_tree_bounded(TreeRemovalLimits::new(1, 1).unwrap())
            .unwrap_err();
        assert_eq!(error.error(), PrivateFsError::SettlementUnknown);
        assert!(!error.is_recoverable());
        assert!(!parent
            .path()
            .join("tree-removal-parent-sync-fault/tree")
            .exists());
        assert_eq!(
            namespace.directory.list_components(4),
            Err(PrivateFsError::Quarantined)
        );
    }

    #[test]
    fn every_chmod_unlink_and_root_rmdir_fault_is_terminal() {
        for ordinal in 1..=3 {
            let (_parent, namespace) = test_namespace(&format!("tree-removal-mutation-{ordinal}"));
            let tree = namespace
                .directory
                .create_new_private_child(&component("tree"))
                .unwrap();
            write_entry(&tree, "Payload", true);
            let tree = tree.seal().unwrap();
            tree.lease.inject_tree_removal_mutation_fault(ordinal);
            let error = OpenedPrivateDirectory::Sealed(tree)
                .remove_tree_bounded(TreeRemovalLimits::new(1, 1).unwrap())
                .unwrap_err();
            assert_eq!(
                error.error(),
                PrivateFsError::SettlementUnknown,
                "mutation ordinal {ordinal}"
            );
            assert!(!error.is_recoverable(), "mutation ordinal {ordinal}");
            assert_eq!(
                namespace.directory.list_components(4),
                Err(PrivateFsError::Quarantined),
                "mutation ordinal {ordinal}"
            );
        }
    }
}
