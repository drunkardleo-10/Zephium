//! Bounded pin-rooted package-closure garbage collection.
//!
//! Collection is a two-transition protocol. The first transition durably
//! removes package markers from the completed ledger and publishes the exact
//! negative retention roots. Physical deletion then follows one canonical
//! order. A fresh combined inventory mints an in-memory absence proof for the
//! second transition, which alone may clear the intent.

use std::collections::BTreeSet;

use zephium_private_fs::{OpenedPrivateDirectory, PrivateFsError};

use crate::materialization::{
    self, begin_garbage_collection, complete_garbage_collection, prove_garbage_collection_absence,
    remove_tree_directory, CatalogAnchor, MaterializationGarbageCollectionIntent,
    MaterializationRuntime, MaterializationTransitionError, TreeCleanupError,
};
use crate::names;
use crate::operation::RepositoryOperationGuard;
use crate::storage::{map_recovery_fs, validate_named_catalog_object};
use crate::{ExtensionRepository, ExtensionRepositoryError};

/// Result of one explicit bounded garbage-collection operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a collected batch may leave another bounded batch to schedule"]
pub enum BundledPackageGarbageCollectionOutcome {
    /// The fresh pin-rooted inventory contains no garbage. No durable write was
    /// performed by this operation.
    NoGarbage,
    /// One exact durable batch settled completely.
    Collected {
        /// Number of exact regular-object and tree-root targets settled.
        settled_targets: usize,
        /// Whether a fresh post-settlement inventory still contains garbage.
        more_garbage: bool,
    },
}

impl ExtensionRepository {
    /// Collects at most one bounded unreachable bundled-package cohort.
    ///
    /// The operation performs no external callback and never evicts a selected
    /// or owner-pinned package. A pending crash frontier is resumed first;
    /// planning always restarts from a fresh recovered inventory.
    pub fn collect_bundled_package_garbage(
        &mut self,
    ) -> Result<BundledPackageGarbageCollectionOutcome, ExtensionRepositoryError> {
        let runtime = self.runtime.clone();
        let operation = runtime.enter().map_err(|error| error.repository_error())?;
        self.collect_bundled_package_garbage_under_gate(&operation)
    }

    fn collect_bundled_package_garbage_under_gate(
        &mut self,
        _operation: &RepositoryOperationGuard<'_>,
    ) -> Result<BundledPackageGarbageCollectionOutcome, ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        if self.writer_materialization()?._gc_intent.is_some() {
            let settled_targets = self.settle_pending_garbage_collection()?;
            let more_garbage = self.fresh_garbage_is_present()?;
            return Ok(BundledPackageGarbageCollectionOutcome::Collected {
                settled_targets,
                more_garbage,
            });
        }

        self.refresh_outer_authority_or_seal()?;
        let high_water = self
            .writer_catalog_high_water()
            .map(CatalogAnchor::from_high_water);
        let catalog_object_ids = self.writer_catalog_object_ids().clone();
        let plan = materialization::plan_garbage_collection(
            self.writer_materialization()?,
            high_water,
            &catalog_object_ids,
        )?;
        let Some(plan) = plan else {
            return Ok(BundledPackageGarbageCollectionOutcome::NoGarbage);
        };

        let runtime = self.writer_take_materialization()?;
        self.finish_collection_transition(begin_garbage_collection(runtime, plan))?;
        let settled_targets = self.settle_pending_garbage_collection()?;
        let more_garbage = self.fresh_garbage_is_present()?;
        Ok(BundledPackageGarbageCollectionOutcome::Collected {
            settled_targets,
            more_garbage,
        })
    }

    /// Settles only an already-durable intent. Used by open and by the explicit
    /// collector before it considers a fresh plan.
    pub(crate) fn settle_pending_garbage_collection(
        &mut self,
    ) -> Result<usize, ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        self.refresh_outer_authority_or_seal()?;
        let Some(intent) = self.writer_materialization()?._gc_intent.clone() else {
            return Ok(0);
        };
        let settled_targets = intent.catalog_set_record_ids.len()
            + intent.package_record_ids.len()
            + intent.tree_index_ids.len()
            + intent.legal_artifact_ids.len()
            + intent.retired_trees.len()
            + intent.tree_objects.len()
            + intent.catalog_object_ids.len();

        let runtime = self.writer_take_materialization()?;
        if let Err(error) = delete_inner_garbage_targets(&runtime, &intent) {
            drop(runtime);
            return Err(self.finish_physical_failure(error));
        }
        // Trees may be large. Re-read the exact outer state/checkpoint/journal
        // and actual catalog inventory at the boundary immediately before the
        // first outer unlink rather than relying on the pre-tree snapshot.
        if let Err(error) = self.refresh_outer_authority_or_seal() {
            drop(runtime);
            return Err(error);
        }
        if let Err(error) = self.delete_outer_catalog_targets(&intent) {
            drop(runtime);
            return Err(self.finish_physical_failure(error));
        }
        drop(runtime);

        // Reinventory both namespaces after the final physical mutation. This
        // replaces every cached outer identity before recovery validates the
        // partial frontier and before the absence proof is constructed.
        self.refresh_outer_authority_or_seal()?;
        self.writer_recover_materialization_or_seal()?;
        let catalog_object_ids = self.writer_catalog_object_ids().clone();
        let catalogs_parent = self.writer_catalogs().identity();
        let proof = prove_garbage_collection_absence(
            self.writer_materialization()?,
            &catalog_object_ids,
            catalogs_parent,
        );
        let proof = proof?;
        let runtime = self.writer_take_materialization()?;
        self.finish_collection_transition(complete_garbage_collection(
            runtime,
            proof,
            catalogs_parent,
        ))?;
        Ok(settled_targets)
    }

    fn delete_outer_catalog_targets(
        &self,
        intent: &MaterializationGarbageCollectionIntent,
    ) -> Result<(), PhysicalCollectionError> {
        // Recheck the monotonic outer root immediately before catalog-object
        // deletion; even an internal stale plan may never erase high-water.
        let high_water = self
            .writer_catalog_high_water()
            .map(|row| row.catalog_sha256);
        for target in &intent.catalog_object_ids {
            if high_water == Some(*target) {
                return Err(PhysicalCollectionError::Clean(
                    ExtensionRepositoryError::RecoveryAmbiguous,
                ));
            }
            remove_present_regular(
                self.writer_catalogs(),
                &names::catalog_file(*target),
                self.writer_catalog_object_ids().contains(target),
            )?;
        }
        Ok(())
    }

    fn fresh_garbage_is_present(&mut self) -> Result<bool, ExtensionRepositoryError> {
        self.refresh_outer_authority_or_seal()?;
        let high_water = self
            .writer_catalog_high_water()
            .map(CatalogAnchor::from_high_water);
        let catalog_object_ids = self.writer_catalog_object_ids().clone();
        match materialization::plan_garbage_collection(
            self.writer_materialization()?,
            high_water,
            &catalog_object_ids,
        ) {
            Ok(plan) => Ok(plan.is_some()),
            Err(ExtensionRepositoryError::GenerationExhausted) => Ok(true),
            Err(error) => {
                self.writer_seal();
                Err(error)
            }
        }
    }

    fn refresh_catalog_object_inventory(&mut self) -> Result<(), ExtensionRepositoryError> {
        let entries = self
            .writer_catalogs()
            .list_components(names::MAX_CATALOG_OBJECT_ENTRIES)
            .map_err(map_recovery_fs)?;
        let mut ids = BTreeSet::new();
        for entry in entries {
            let (digest, stage) = names::parse_catalog_file(entry.as_str())
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            if stage || !ids.insert(digest) {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            validate_named_catalog_object(self.writer_catalogs(), &entry, digest)?;
        }
        if self
            .writer_catalog_high_water()
            .is_some_and(|high_water| !ids.contains(&high_water.catalog_sha256))
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        self.writer_replace_catalog_object_ids(ids);
        Ok(())
    }

    fn refresh_outer_authority_or_seal(&mut self) -> Result<(), ExtensionRepositoryError> {
        let validation = self
            .refresh_catalog_object_inventory()
            .and_then(|()| self.writer_validate_outer_settled_projection());
        match validation {
            Ok(()) => Ok(()),
            Err(error) => {
                self.writer_seal();
                Err(error)
            }
        }
    }

    fn finish_collection_transition<Committed>(
        &mut self,
        transition: Result<Committed, MaterializationTransitionError>,
    ) -> Result<(), ExtensionRepositoryError> {
        match transition {
            Ok(_committed) => self.writer_recover_materialization_or_seal(),
            Err(MaterializationTransitionError::Clean(error)) => {
                self.writer_recover_materialization_or_seal()?;
                Err(error)
            }
            Err(MaterializationTransitionError::MustSeal(error)) => {
                self.writer_seal();
                Err(error)
            }
        }
    }

    fn writer_recover_materialization_or_seal(&mut self) -> Result<(), ExtensionRepositoryError> {
        match self.writer_recover_materialization() {
            Ok(()) => Ok(()),
            Err(error) => {
                self.writer_seal();
                Err(error)
            }
        }
    }

    fn finish_physical_failure(
        &mut self,
        failure: PhysicalCollectionError,
    ) -> ExtensionRepositoryError {
        match failure {
            PhysicalCollectionError::MustSeal(error) => {
                self.writer_seal();
                error
            }
            PhysicalCollectionError::Clean(error) => {
                let recovery = self
                    .refresh_catalog_object_inventory()
                    .and_then(|()| self.writer_validate_outer_settled_projection())
                    .and_then(|()| self.writer_recover_materialization());
                match recovery {
                    Ok(()) => error,
                    Err(recovery) => {
                        self.writer_seal();
                        recovery
                    }
                }
            }
        }
    }
}

fn delete_inner_garbage_targets(
    runtime: &MaterializationRuntime,
    intent: &MaterializationGarbageCollectionIntent,
) -> Result<(), PhysicalCollectionError> {
    // Metadata first: a surviving package marker always retains its whole
    // closure. Outer authenticated catalog bytes remain last.
    for target in &intent.catalog_set_record_ids {
        remove_present_regular(
            &runtime._records,
            &materialization::gc_catalog_set_record(*target),
            runtime._catalog_sets.contains_key(target),
        )?;
    }
    for target in &intent.package_record_ids {
        remove_present_regular(
            &runtime._records,
            &materialization::gc_package_record(*target),
            runtime._package_records.contains_key(target),
        )?;
    }
    for target in &intent.tree_index_ids {
        remove_present_regular(
            &runtime._records,
            &materialization::gc_tree_index_object(*target),
            runtime._tree_index_ids.contains(target),
        )?;
    }
    for target in &intent.legal_artifact_ids {
        remove_present_regular(
            &runtime._records,
            &materialization::gc_legal_object(*target),
            runtime._legal_artifact_ids.contains(target),
        )?;
    }

    for target in &intent.retired_trees {
        let exact = (target.tree_sha256, target.retirement_generation);
        if runtime._retired_tree_ids.contains(&exact) {
            let name =
                materialization::gc_tree_retired(target.tree_sha256, target.retirement_generation)
                    .map_err(PhysicalCollectionError::Clean)?;
            let opened = runtime
                ._trees
                .open_private_child_any_mode(&name)
                .map_err(map_physical_fs)?;
            remove_tree_directory(opened).map_err(map_tree_cleanup)?;
        }
    }
    for target in &intent.tree_objects {
        let retired_name = materialization::gc_tree_retired(target.tree_sha256, intent.generation)
            .map_err(PhysicalCollectionError::Clean)?;
        let opened = if runtime._tree_object_ids.contains(&target.tree_sha256) {
            let object_name = materialization::gc_tree_object(target.tree_sha256);
            let object = runtime
                ._trees
                .open_sealed_private_child(&object_name)
                .map_err(map_physical_fs)?;
            let retired = object
                .publish_noreplace(&runtime._trees, &retired_name)
                .map_err(map_tree_transition)?;
            Some(OpenedPrivateDirectory::Sealed(retired))
        } else if runtime
            ._retired_tree_ids
            .contains(&(target.tree_sha256, intent.generation))
        {
            Some(
                runtime
                    ._trees
                    .open_private_child_any_mode(&retired_name)
                    .map_err(map_physical_fs)?,
            )
        } else {
            None
        };
        if let Some(opened) = opened {
            remove_tree_directory(opened).map_err(map_tree_cleanup)?;
        }
    }
    Ok(())
}

fn remove_present_regular(
    directory: &zephium_private_fs::PrivateDirectory,
    name: &zephium_private_fs::PrivateComponent,
    present: bool,
) -> Result<(), PhysicalCollectionError> {
    if present
        && !directory
            .remove_verified_regular(name)
            .map_err(map_physical_fs)?
    {
        return Err(PhysicalCollectionError::Clean(
            ExtensionRepositoryError::RecoveryAmbiguous,
        ));
    }
    Ok(())
}

enum PhysicalCollectionError {
    Clean(ExtensionRepositoryError),
    MustSeal(ExtensionRepositoryError),
}

fn map_physical_fs(error: PrivateFsError) -> PhysicalCollectionError {
    if is_terminal_fs(error) {
        PhysicalCollectionError::MustSeal(ExtensionRepositoryError::SettlementAmbiguous)
    } else {
        PhysicalCollectionError::Clean(ExtensionRepositoryError::FileSystem(error))
    }
}

fn map_tree_transition<State>(
    error: zephium_private_fs::PrivateFsTransitionError<State>,
) -> PhysicalCollectionError {
    let (error, state) = error.into_parts();
    if state.is_some() && !is_terminal_fs(error) {
        PhysicalCollectionError::Clean(ExtensionRepositoryError::FileSystem(error))
    } else {
        PhysicalCollectionError::MustSeal(ExtensionRepositoryError::SettlementAmbiguous)
    }
}

fn map_tree_cleanup(error: TreeCleanupError) -> PhysicalCollectionError {
    match error {
        TreeCleanupError::Filesystem(error) => map_physical_fs(error),
        TreeCleanupError::InvalidShape | TreeCleanupError::SettlementAmbiguous => {
            PhysicalCollectionError::MustSeal(ExtensionRepositoryError::SettlementAmbiguous)
        }
    }
}

const fn is_terminal_fs(error: PrivateFsError) -> bool {
    matches!(
        error,
        PrivateFsError::IdentityAmbiguous
            | PrivateFsError::SettlementUnknown
            | PrivateFsError::Quarantined
    )
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
#[path = "garbage_collection/tests.rs"]
mod tests;
