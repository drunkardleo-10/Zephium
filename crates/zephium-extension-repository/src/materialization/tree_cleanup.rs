//! Bounded, descriptor-relative removal of private package trees.
//!
//! The traversal never follows links or delegates recursion to the host. It
//! admits every exact child through the held parent, unseals directories
//! top-down, removes verified regular files, and consumes directories
//! bottom-up under one global entry and depth budget.

use zephium_extension_package::{MAX_EXTENSION_RELATIVE_PATH_DEPTH, MAX_EXTENSION_TREE_ENTRIES};
use zephium_private_fs::{
    OpenedPrivateDirectory, PrivateFsError, PrivateFsTransitionError, TreeRemovalLimits,
    TreeRemovalReport, MAX_TREE_REMOVAL_DEPTH, MAX_TREE_REMOVAL_ENTRIES,
};

const _: () = assert!(MAX_EXTENSION_TREE_ENTRIES <= MAX_TREE_REMOVAL_ENTRIES);
const _: () = assert!(MAX_EXTENSION_RELATIVE_PATH_DEPTH <= MAX_TREE_REMOVAL_DEPTH);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TreeCleanupError {
    InvalidShape,
    Filesystem(PrivateFsError),
    SettlementAmbiguous,
}

pub(crate) fn remove_tree_directory(
    opened: OpenedPrivateDirectory,
) -> Result<TreeRemovalReport, TreeCleanupError> {
    remove_tree_directory_bounded(opened, MAX_EXTENSION_TREE_ENTRIES)
}

pub(crate) fn remove_tree_directory_bounded(
    opened: OpenedPrivateDirectory,
    max_entries: usize,
) -> Result<TreeRemovalReport, TreeCleanupError> {
    if max_entries > MAX_EXTENSION_TREE_ENTRIES {
        return Err(TreeCleanupError::InvalidShape);
    }
    let limits = TreeRemovalLimits::new(max_entries, MAX_EXTENSION_RELATIVE_PATH_DEPTH)
        .map_err(map_filesystem)?;
    let report = opened.remove_tree_bounded(limits).map_err(map_transition)?;
    if report.observed_entries() > max_entries
        || report.maximum_depth() > MAX_EXTENSION_RELATIVE_PATH_DEPTH
        || report.regular_files_removed() + report.directories_removed().saturating_sub(1)
            != report.observed_entries()
        || report.directories_unsealed() > report.directories_removed()
        || report.directory_syncs() != 1
    {
        return Err(TreeCleanupError::SettlementAmbiguous);
    }
    Ok(report)
}

fn map_transition<State>(error: PrivateFsTransitionError<State>) -> TreeCleanupError {
    let (error, state) = error.into_parts();
    if state.is_some() && is_invalid_shape(error) {
        TreeCleanupError::InvalidShape
    } else if state.is_some() && !is_terminal(error) {
        TreeCleanupError::Filesystem(error)
    } else {
        TreeCleanupError::SettlementAmbiguous
    }
}

fn map_filesystem(error: PrivateFsError) -> TreeCleanupError {
    if is_terminal(error) {
        TreeCleanupError::SettlementAmbiguous
    } else if is_invalid_shape(error) {
        TreeCleanupError::InvalidShape
    } else {
        TreeCleanupError::Filesystem(error)
    }
}

const fn is_invalid_shape(error: PrivateFsError) -> bool {
    matches!(
        error,
        PrivateFsError::BoundExceeded | PrivateFsError::Unsafe
    )
}

const fn is_terminal(error: PrivateFsError) -> bool {
    matches!(
        error,
        PrivateFsError::IdentityAmbiguous
            | PrivateFsError::SettlementUnknown
            | PrivateFsError::Quarantined
    )
}
