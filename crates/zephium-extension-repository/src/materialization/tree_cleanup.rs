//! Bounded, descriptor-relative removal of private package trees.
//!
//! The traversal never follows links or delegates recursion to the host. It
//! admits every exact child through the held parent, unseals directories
//! top-down, removes verified regular files, and consumes directories
//! bottom-up under one global entry and depth budget.

use zephium_extension_package::{MAX_EXTENSION_RELATIVE_PATH_DEPTH, MAX_EXTENSION_TREE_ENTRIES};
use zephium_private_fs::{
    OpenedPrivateDirectory, PrivateChildKind, PrivateFsError, PrivateFsTransitionError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TreeCleanupError {
    InvalidShape,
    Filesystem(PrivateFsError),
    SettlementAmbiguous,
}

pub(crate) fn remove_tree_directory(
    opened: OpenedPrivateDirectory,
) -> Result<usize, TreeCleanupError> {
    let mut observed_entries = 0_usize;
    clear_and_remove_directory(opened, 0, &mut observed_entries)?;
    Ok(observed_entries)
}

fn clear_and_remove_directory(
    opened: OpenedPrivateDirectory,
    depth: usize,
    observed_entries: &mut usize,
) -> Result<(), TreeCleanupError> {
    let directory = match opened {
        OpenedPrivateDirectory::Writable(directory) => directory,
        OpenedPrivateDirectory::Sealed(directory) => directory.unseal().map_err(map_transition)?,
    };
    let names = directory
        .list_entry_names(MAX_EXTENSION_TREE_ENTRIES)
        .map_err(map_filesystem)?;
    for name in names {
        let child_depth = depth.checked_add(1).ok_or(TreeCleanupError::InvalidShape)?;
        if child_depth > MAX_EXTENSION_RELATIVE_PATH_DEPTH {
            return Err(TreeCleanupError::InvalidShape);
        }
        *observed_entries = observed_entries
            .checked_add(1)
            .ok_or(TreeCleanupError::InvalidShape)?;
        if *observed_entries > MAX_EXTENSION_TREE_ENTRIES {
            return Err(TreeCleanupError::InvalidShape);
        }
        let kind = directory
            .inspect_entry(&name)
            .map_err(map_filesystem)?
            .ok_or(TreeCleanupError::InvalidShape)?;
        match kind {
            PrivateChildKind::RegularFile(_) => {
                if !directory
                    .remove_verified_entry_regular(&name)
                    .map_err(map_filesystem)?
                {
                    return Err(TreeCleanupError::InvalidShape);
                }
            }
            PrivateChildKind::Directory(identity) => {
                let child = directory
                    .open_entry_child_any_mode(&name)
                    .map_err(map_filesystem)?;
                if child.identity() != identity {
                    return Err(TreeCleanupError::SettlementAmbiguous);
                }
                clear_and_remove_directory(child, child_depth, observed_entries)?;
            }
        }
    }
    directory.remove_empty().map_err(map_transition)
}

fn map_transition<State>(error: PrivateFsTransitionError<State>) -> TreeCleanupError {
    let (error, state) = error.into_parts();
    if state.is_some() && !is_terminal(error) {
        TreeCleanupError::Filesystem(error)
    } else {
        TreeCleanupError::SettlementAmbiguous
    }
}

fn map_filesystem(error: PrivateFsError) -> TreeCleanupError {
    if is_terminal(error) {
        TreeCleanupError::SettlementAmbiguous
    } else if error == PrivateFsError::BoundExceeded {
        TreeCleanupError::InvalidShape
    } else {
        TreeCleanupError::Filesystem(error)
    }
}

const fn is_terminal(error: PrivateFsError) -> bool {
    matches!(
        error,
        PrivateFsError::IdentityAmbiguous
            | PrivateFsError::SettlementUnknown
            | PrivateFsError::Quarantined
    )
}
