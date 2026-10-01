//! Bounded handle-relative private-tree removal shared by supported adapters.

use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};

use super::{
    directory_is_empty, inspect_child, list_names, open_child_directory_any_mode, open_regular,
    relative_name_is_absent, remove_directory, remove_regular, revalidate_child_directory,
    revalidate_regular, same_open_identity, set_directory_mode, sync_directory, DirectoryMode,
    OpenPurpose, RawChildKind, RawIdentity,
};
use crate::{PrivateEntryName, PrivateFsError};

pub(crate) struct RawTreeRemovalReport {
    pub(crate) observed_entries: usize,
    pub(crate) maximum_depth: usize,
    pub(crate) regular_files_removed: usize,
    pub(crate) directories_removed: usize,
    pub(crate) directories_unsealed: usize,
    pub(crate) directory_syncs: usize,
}

pub(crate) struct TreeRemovalFailure {
    pub(crate) error: PrivateFsError,
    pub(crate) mutation_started: bool,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TreeRemovalFaults {
    pub(crate) fail_preexecution_identity: bool,
    pub(crate) fail_at_mutation: Option<usize>,
    pub(crate) fail_final_parent_sync: bool,
}

struct PlannedDirectory {
    identity: RawIdentity,
    mode: DirectoryMode,
    children: Vec<PlannedChild>,
}

struct PlannedChild {
    name: String,
    kind: PlannedChildKind,
}

enum PlannedChildKind {
    Regular(RawIdentity),
    Directory(Box<PlannedDirectory>),
}

struct RemovalContext {
    regular_files_removed: usize,
    directories_removed: usize,
    directories_unsealed: usize,
    directory_syncs: usize,
    mutations: usize,
    faults: TreeRemovalFaults,
}

impl RemovalContext {
    fn note_mutation(&mut self) -> Result<(), PrivateFsError> {
        self.mutations = self
            .mutations
            .checked_add(1)
            .ok_or(PrivateFsError::BoundExceeded)?;
        if self.faults.fail_at_mutation == Some(self.mutations) {
            return Err(PrivateFsError::Io);
        }
        Ok(())
    }

    fn failure(&self, error: PrivateFsError) -> TreeRemovalFailure {
        TreeRemovalFailure {
            error,
            mutation_started: self.mutations != 0,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn remove_tree_bounded(
    directory: &File,
    directory_path: &Path,
    directory_identity: RawIdentity,
    directory_mode: DirectoryMode,
    parent: &File,
    parent_path: &Path,
    child_name: &str,
    max_entries: usize,
    max_depth: usize,
    faults: TreeRemovalFaults,
) -> Result<RawTreeRemovalReport, TreeRemovalFailure> {
    let mut observed_entries = 0_usize;
    let mut maximum_depth = 0_usize;
    let mut seen_directories = HashSet::from([directory_identity]);
    let plan = plan_directory(
        directory,
        directory_path,
        directory_identity,
        directory_mode,
        0,
        max_entries,
        max_depth,
        &mut observed_entries,
        &mut maximum_depth,
        &mut seen_directories,
    )
    .map_err(|error| TreeRemovalFailure {
        error,
        mutation_started: false,
    })?;
    if faults.fail_preexecution_identity {
        return Err(TreeRemovalFailure {
            error: PrivateFsError::IdentityAmbiguous,
            mutation_started: false,
        });
    }

    let mut context = RemovalContext {
        regular_files_removed: 0,
        directories_removed: 0,
        directories_unsealed: 0,
        directory_syncs: 0,
        mutations: 0,
        faults,
    };
    remove_planned_directory(
        parent,
        parent_path,
        child_name,
        directory,
        directory_path,
        &plan,
        &mut context,
    )?;

    sync_directory(parent).map_err(|error| context.failure(error))?;
    context.directory_syncs = context
        .directory_syncs
        .checked_add(1)
        .ok_or_else(|| context.failure(PrivateFsError::BoundExceeded))?;
    if context.faults.fail_final_parent_sync {
        return Err(context.failure(PrivateFsError::Io));
    }
    if !relative_name_is_absent(parent, child_name) {
        return Err(context.failure(PrivateFsError::IdentityAmbiguous));
    }
    debug_assert_eq!(
        context.regular_files_removed + context.directories_removed.saturating_sub(1),
        observed_entries
    );
    debug_assert!(context.directories_unsealed <= context.directories_removed);
    debug_assert_eq!(context.directory_syncs, 1);

    Ok(RawTreeRemovalReport {
        observed_entries,
        maximum_depth,
        regular_files_removed: context.regular_files_removed,
        directories_removed: context.directories_removed,
        directories_unsealed: context.directories_unsealed,
        directory_syncs: context.directory_syncs,
    })
}

#[allow(clippy::too_many_arguments)]
fn plan_directory(
    directory: &File,
    directory_path: &Path,
    identity: RawIdentity,
    mode: DirectoryMode,
    depth: usize,
    max_entries: usize,
    max_depth: usize,
    observed_entries: &mut usize,
    maximum_depth: &mut usize,
    seen_directories: &mut HashSet<RawIdentity>,
) -> Result<PlannedDirectory, PrivateFsError> {
    let remaining = max_entries.saturating_sub(*observed_entries);
    let mut names = list_names(directory, directory_path, remaining.saturating_add(1))?;
    names.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    let mut children = Vec::with_capacity(names.len().min(remaining));
    for name in names {
        PrivateEntryName::new(name.clone()).map_err(|_| PrivateFsError::Unsafe)?;
        *observed_entries = observed_entries
            .checked_add(1)
            .ok_or(PrivateFsError::BoundExceeded)?;
        if *observed_entries > max_entries {
            return Err(PrivateFsError::BoundExceeded);
        }
        let child_depth = depth.checked_add(1).ok_or(PrivateFsError::BoundExceeded)?;
        if child_depth > max_depth {
            return Err(PrivateFsError::BoundExceeded);
        }
        *maximum_depth = (*maximum_depth).max(child_depth);
        let kind = inspect_child(directory, directory_path, &name)?
            .ok_or(PrivateFsError::IdentityAmbiguous)?;
        let planned = match kind {
            RawChildKind::Regular(identity) => PlannedChildKind::Regular(identity),
            RawChildKind::Directory(expected) => {
                let child_path = directory_path.join(&name);
                let (child, opened_identity, child_mode) =
                    open_child_directory_any_mode(directory, directory_path, &name)?;
                if opened_identity != expected || !seen_directories.insert(opened_identity) {
                    return Err(PrivateFsError::IdentityAmbiguous);
                }
                let child_plan = plan_directory(
                    &child,
                    &child_path,
                    opened_identity,
                    child_mode,
                    child_depth,
                    max_entries,
                    max_depth,
                    observed_entries,
                    maximum_depth,
                    seen_directories,
                )?;
                PlannedChildKind::Directory(Box::new(child_plan))
            }
        };
        children.push(PlannedChild {
            name,
            kind: planned,
        });
    }
    Ok(PlannedDirectory {
        identity,
        mode,
        children,
    })
}

#[allow(clippy::too_many_arguments)]
fn remove_planned_directory(
    parent: &File,
    parent_path: &Path,
    child_name: &str,
    directory: &File,
    directory_path: &Path,
    plan: &PlannedDirectory,
    context: &mut RemovalContext,
) -> Result<(), TreeRemovalFailure> {
    revalidate_child_directory(
        parent,
        parent_path,
        child_name,
        directory,
        plan.identity,
        plan.mode,
    )
    .map_err(|error| context.failure(error))?;

    let removal_mode = if plan.mode == DirectoryMode::Sealed && !plan.children.is_empty() {
        set_directory_mode(directory, DirectoryMode::Writable)
            .map_err(|error| context.failure(error))?;
        context
            .note_mutation()
            .map_err(|error| context.failure(error))?;
        context.directories_unsealed = context
            .directories_unsealed
            .checked_add(1)
            .ok_or_else(|| context.failure(PrivateFsError::BoundExceeded))?;
        DirectoryMode::Writable
    } else {
        plan.mode
    };

    for child in &plan.children {
        match &child.kind {
            PlannedChildKind::Regular(expected) => {
                let (file, identity) = open_regular(
                    directory,
                    directory_path,
                    &child.name,
                    OpenPurpose::Mutation,
                )
                .map_err(|error| context.failure(error))?;
                if identity != *expected {
                    return Err(context.failure(PrivateFsError::IdentityAmbiguous));
                }
                revalidate_regular(directory, directory_path, &child.name, &file, *expected)
                    .map_err(|error| context.failure(error))?;
                remove_regular(directory, directory_path, &child.name)
                    .map_err(|error| context.failure(error))?;
                context
                    .note_mutation()
                    .map_err(|error| context.failure(error))?;
                if !same_open_identity(&file, *expected)
                    || !relative_name_is_absent(directory, &child.name)
                {
                    return Err(context.failure(PrivateFsError::IdentityAmbiguous));
                }
                context.regular_files_removed = context
                    .regular_files_removed
                    .checked_add(1)
                    .ok_or_else(|| context.failure(PrivateFsError::BoundExceeded))?;
            }
            PlannedChildKind::Directory(child_plan) => {
                let child_path: PathBuf = directory_path.join(&child.name);
                let (opened, identity, mode) =
                    open_child_directory_any_mode(directory, directory_path, &child.name)
                        .map_err(|error| context.failure(error))?;
                if identity != child_plan.identity || mode != child_plan.mode {
                    return Err(context.failure(PrivateFsError::IdentityAmbiguous));
                }
                remove_planned_directory(
                    directory,
                    directory_path,
                    &child.name,
                    &opened,
                    &child_path,
                    child_plan,
                    context,
                )?;
            }
        }
    }

    if !directory_is_empty(directory).map_err(|error| context.failure(error))? {
        return Err(context.failure(PrivateFsError::IdentityAmbiguous));
    }
    revalidate_child_directory(
        parent,
        parent_path,
        child_name,
        directory,
        plan.identity,
        removal_mode,
    )
    .map_err(|error| context.failure(error))?;
    remove_directory(parent, parent_path, child_name).map_err(|error| context.failure(error))?;
    context
        .note_mutation()
        .map_err(|error| context.failure(error))?;
    if !same_open_identity(directory, plan.identity) || !relative_name_is_absent(parent, child_name)
    {
        return Err(context.failure(PrivateFsError::IdentityAmbiguous));
    }
    context.directories_removed = context
        .directories_removed
        .checked_add(1)
        .ok_or_else(|| context.failure(PrivateFsError::BoundExceeded))?;
    Ok(())
}
