//! Bounded pin-rooted package-closure garbage collection.
//!
//! Collection is a two-transition protocol. The first transition durably
//! removes package markers from the completed ledger and publishes the exact
//! negative retention roots. Physical deletion then follows one canonical
//! order. A fresh combined inventory mints an in-memory absence proof for the
//! second transition, which alone may clear the intent.

use zephium_private_fs::{OpenedPrivateDirectory, PrivateFsError, TreeRemovalReport};

use crate::materialization::{
    self, begin_garbage_collection, complete_garbage_collection, prove_garbage_collection_absence,
    remove_tree_directory, remove_tree_directory_bounded, CatalogAnchor,
    MaterializationGarbageCollectionIntent, MaterializationRuntime, MaterializationTransitionError,
    TreeCleanupError, MAX_GC_CATALOG_OBJECT_TARGETS, MAX_GC_CATALOG_SET_TARGETS,
    MAX_GC_DATA_OBJECT_TARGETS, MAX_GC_PACKAGE_RECORD_TARGETS, MAX_GC_TREE_ENTRIES,
    MAX_GC_TREE_JOBS,
};
use crate::names;
use crate::operation::RepositoryOperationGuard;
use crate::{ExtensionRepository, ExtensionRepositoryError};

const MATERIALIZATION_TRANSITION_DURABILITY_SYNCS: usize = 13;
// macOS publication temporarily makes the consumed root writable, reseals it,
// and fsyncs that seal before returning authority. This required root sync is
// additional to the pre-existing retirement/deletion durability contract.
const TREE_PUBLICATION_ROOT_RESEAL_SYNCS: usize = cfg!(target_os = "macos") as usize;
const TREE_OBJECT_RETIREMENT_DURABILITY_SYNCS: usize = 2 + TREE_PUBLICATION_ROOT_RESEAL_SYNCS;
const MAX_GC_COHORT_REGULAR_TARGETS: usize =
    1 + MAX_GC_CATALOG_SET_TARGETS + MAX_GC_PACKAGE_RECORD_TARGETS + MAX_GC_DATA_OBJECT_TARGETS * 2;
const MAX_GC_RESIDUE_REGULAR_TARGETS: usize =
    MAX_GC_CATALOG_OBJECT_TARGETS + MAX_GC_DATA_OBJECT_TARGETS * 2;
const MAX_GC_REGULAR_TARGETS: usize =
    if MAX_GC_COHORT_REGULAR_TARGETS > MAX_GC_RESIDUE_REGULAR_TARGETS {
        MAX_GC_COHORT_REGULAR_TARGETS
    } else {
        MAX_GC_RESIDUE_REGULAR_TARGETS
    };
const MAX_GC_LOGICAL_TARGETS: usize = MAX_GC_REGULAR_TARGETS + MAX_GC_TREE_JOBS;
const MAX_GC_TREE_DIRECTORY_NODES: usize = MAX_GC_TREE_ENTRIES + MAX_GC_TREE_JOBS;
const MAX_GC_FRESH_DURABILITY_SYNCS: usize = 128;
// Preserve the original two-sync headroom at the maximum native cohort:
// Linux observes 94/96; macOS observes 102/104 after root-reseal hardening.
const MAX_GC_PENDING_DURABILITY_SYNCS: usize =
    96 + MAX_GC_TREE_JOBS * TREE_PUBLICATION_ROOT_RESEAL_SYNCS;
const MAX_GC_PHYSICAL_DURABILITY_SYNCS: usize =
    MAX_GC_PENDING_DURABILITY_SYNCS - MATERIALIZATION_TRANSITION_DURABILITY_SYNCS;
const MAX_GC_PENDING_CONTROL_BYTES_WRITTEN: usize =
    materialization::MAX_MATERIALIZATION_JOURNAL_BYTES
        + materialization::MAX_MATERIALIZATION_STATE_BYTES
        + materialization::MAX_MATERIALIZATION_CHECKPOINT_BYTES;
const MAX_GC_FRESH_CONTROL_BYTES_WRITTEN: usize = 2 * MAX_GC_PENDING_CONTROL_BYTES_WRITTEN;

const _: () = assert!(MAX_GC_REGULAR_TARGETS == 57);
const _: () = assert!(MAX_GC_LOGICAL_TARGETS == 65);
const _: () = assert!(MAX_GC_TREE_DIRECTORY_NODES == 32_776);
const _: () =
    assert!(MAX_GC_FRESH_CONTROL_BYTES_WRITTEN == 2 * MAX_GC_PENDING_CONTROL_BYTES_WRITTEN);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct GarbageCollectionWork {
    state_transitions: usize,
    regular_targets_removed: usize,
    tree_jobs: usize,
    tree_object_retirements: usize,
    tree_entries: usize,
    tree_maximum_depth: usize,
    tree_regular_files_removed: usize,
    tree_directories_removed: usize,
    tree_directories_unsealed: usize,
    tree_directory_syncs: usize,
}

impl GarbageCollectionWork {
    fn record_tree(&mut self, report: TreeRemovalReport) -> Result<(), PhysicalCollectionError> {
        self.tree_jobs = checked_work_add(self.tree_jobs, 1)?;
        self.tree_entries = checked_work_add(self.tree_entries, report.observed_entries())?;
        self.tree_maximum_depth = self.tree_maximum_depth.max(report.maximum_depth());
        self.tree_regular_files_removed = checked_work_add(
            self.tree_regular_files_removed,
            report.regular_files_removed(),
        )?;
        self.tree_directories_removed =
            checked_work_add(self.tree_directories_removed, report.directories_removed())?;
        self.tree_directories_unsealed = checked_work_add(
            self.tree_directories_unsealed,
            report.directories_unsealed(),
        )?;
        self.tree_directory_syncs =
            checked_work_add(self.tree_directory_syncs, report.directory_syncs())?;
        Ok(())
    }

    fn durability_syncs(self) -> Option<usize> {
        self.state_transitions
            .checked_mul(MATERIALIZATION_TRANSITION_DURABILITY_SYNCS)?
            .checked_add(self.regular_targets_removed)?
            .checked_add(
                self.tree_object_retirements
                    .checked_mul(TREE_OBJECT_RETIREMENT_DURABILITY_SYNCS)?,
            )?
            .checked_add(self.tree_directory_syncs)
    }

    fn physical_durability_syncs(self) -> Option<usize> {
        self.regular_targets_removed
            .checked_add(
                self.tree_object_retirements
                    .checked_mul(TREE_OBJECT_RETIREMENT_DURABILITY_SYNCS)?,
            )?
            .checked_add(self.tree_directory_syncs)
    }

    fn validate_physical(self) -> Result<(), ExtensionRepositoryError> {
        if self.regular_targets_removed > MAX_GC_REGULAR_TARGETS
            || self.tree_jobs > MAX_GC_TREE_JOBS
            || self.tree_object_retirements > self.tree_jobs
            || self.tree_entries > MAX_GC_TREE_ENTRIES
            || self.tree_maximum_depth
                > zephium_extension_package::MAX_EXTENSION_RELATIVE_PATH_DEPTH
            || self.tree_directories_removed > MAX_GC_TREE_DIRECTORY_NODES
            || self.tree_regular_files_removed
                + self.tree_directories_removed.saturating_sub(self.tree_jobs)
                != self.tree_entries
            || self.tree_directories_unsealed > self.tree_directories_removed
            || self.tree_directory_syncs != self.tree_jobs
            || self
                .physical_durability_syncs()
                .is_none_or(|syncs| syncs > MAX_GC_PHYSICAL_DURABILITY_SYNCS)
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }

    fn validate(self, expected_state_transitions: usize) -> Result<(), ExtensionRepositoryError> {
        self.validate_physical()?;
        let sync_ceiling = match expected_state_transitions {
            0 => 0,
            1 => MAX_GC_PENDING_DURABILITY_SYNCS,
            2 => MAX_GC_FRESH_DURABILITY_SYNCS,
            _ => return Err(ExtensionRepositoryError::RecoveryAmbiguous),
        };
        if self.state_transitions != expected_state_transitions
            || self
                .durability_syncs()
                .is_none_or(|syncs| syncs > sync_ceiling)
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }
}

fn checked_work_add(left: usize, right: usize) -> Result<usize, PhysicalCollectionError> {
    left.checked_add(right)
        .ok_or(PhysicalCollectionError::MustSeal(
            ExtensionRepositoryError::SettlementAmbiguous,
        ))
}

pub(crate) struct GarbageCollectionSettlement {
    settled_targets: usize,
    work: GarbageCollectionWork,
}

struct GarbageCollectionOperation {
    outcome: BundledPackageGarbageCollectionOutcome,
    work: GarbageCollectionWork,
}

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
        let GarbageCollectionOperation { outcome, work } =
            self.collect_bundled_package_garbage_under_gate(&operation)?;
        let _ = work;
        Ok(outcome)
    }

    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    fn collect_bundled_package_garbage_measured(
        &mut self,
    ) -> Result<GarbageCollectionOperation, ExtensionRepositoryError> {
        let runtime = self.runtime.clone();
        let operation = runtime.enter().map_err(|error| error.repository_error())?;
        self.collect_bundled_package_garbage_under_gate(&operation)
    }

    fn collect_bundled_package_garbage_under_gate(
        &mut self,
        _operation: &RepositoryOperationGuard<'_>,
    ) -> Result<GarbageCollectionOperation, ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        self.validate_outer_destructive_boundary_or_seal()?;
        if self.writer_materialization()?._gc_intent.is_some() {
            let settlement = self.settle_pending_garbage_collection_after_outer_validation()?;
            let more_garbage = self.fresh_garbage_is_present()?;
            return Ok(GarbageCollectionOperation {
                outcome: BundledPackageGarbageCollectionOutcome::Collected {
                    settled_targets: settlement.settled_targets,
                    more_garbage,
                },
                work: settlement.work,
            });
        }
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
            let work = GarbageCollectionWork::default();
            work.validate(0)?;
            return Ok(GarbageCollectionOperation {
                outcome: BundledPackageGarbageCollectionOutcome::NoGarbage,
                work,
            });
        };

        let runtime = self.writer_take_materialization()?;
        self.finish_collection_transition(begin_garbage_collection(runtime, plan))?;
        let mut settlement = self.settle_pending_garbage_collection_after_outer_validation()?;
        settlement.work.state_transitions = settlement
            .work
            .state_transitions
            .checked_add(1)
            .ok_or(ExtensionRepositoryError::SettlementAmbiguous)?;
        settlement.work.validate(2)?;
        let more_garbage = self.fresh_garbage_is_present()?;
        Ok(GarbageCollectionOperation {
            outcome: BundledPackageGarbageCollectionOutcome::Collected {
                settled_targets: settlement.settled_targets,
                more_garbage,
            },
            work: settlement.work,
        })
    }

    /// Settles only an already-durable intent. Used by open and by the explicit
    /// collector before it considers a fresh plan.
    pub(crate) fn settle_pending_garbage_collection(
        &mut self,
    ) -> Result<GarbageCollectionSettlement, ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        self.validate_outer_destructive_boundary_or_seal()?;
        self.settle_pending_garbage_collection_after_outer_validation()
    }

    fn settle_pending_garbage_collection_after_outer_validation(
        &mut self,
    ) -> Result<GarbageCollectionSettlement, ExtensionRepositoryError> {
        let Some(intent) = self.writer_materialization()?._gc_intent.clone() else {
            return Ok(GarbageCollectionSettlement {
                settled_targets: 0,
                work: GarbageCollectionWork::default(),
            });
        };
        let settled_targets = intent.catalog_set_record_ids.len()
            + intent.package_record_ids.len()
            + intent.tree_index_ids.len()
            + intent.legal_artifact_ids.len()
            + intent.retired_trees.len()
            + intent.tree_objects.len()
            + intent.catalog_object_ids.len();
        if settled_targets > MAX_GC_LOGICAL_TARGETS {
            self.writer_seal();
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let mut work = GarbageCollectionWork::default();

        let runtime = self.writer_take_materialization()?;
        if let Err(error) = delete_inner_garbage_targets(&runtime, &intent, &mut work) {
            drop(runtime);
            return Err(self.finish_physical_failure(error));
        }
        // Trees may be large. Re-read the exact outer controls and compare the
        // complete catalog namespace with the authenticated identity cache at
        // the boundary immediately before the first outer unlink.
        if let Err(error) = self.validate_outer_destructive_boundary_or_seal() {
            drop(runtime);
            return Err(error);
        }
        if let Err(error) = self.delete_outer_catalog_targets(&intent, &mut work) {
            drop(runtime);
            return Err(self.finish_physical_failure(error));
        }
        if let Err(error) = work.validate_physical() {
            drop(runtime);
            self.writer_seal();
            return Err(error);
        }
        drop(runtime);

        // Prove that every successful unlink was reflected exactly in the
        // identity cache before recovery validates the partial frontier and
        // before the absence proof is constructed.
        self.validate_outer_destructive_boundary_or_seal()?;
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
        work.state_transitions = 1;
        if let Err(error) = work.validate(1) {
            self.writer_seal();
            return Err(error);
        }
        Ok(GarbageCollectionSettlement {
            settled_targets,
            work,
        })
    }

    fn delete_outer_catalog_targets(
        &mut self,
        intent: &MaterializationGarbageCollectionIntent,
        work: &mut GarbageCollectionWork,
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
            let was_present = self.writer_catalog_object_ids().contains(target);
            if remove_present_regular(
                self.writer_catalogs(),
                &names::catalog_file(*target),
                was_present,
            )? {
                work.regular_targets_removed = checked_work_add(work.regular_targets_removed, 1)?;
                self.writer_forget_deleted_catalog_object(*target)
                    .map_err(PhysicalCollectionError::MustSeal)?;
            }
        }
        Ok(())
    }

    fn fresh_garbage_is_present(&mut self) -> Result<bool, ExtensionRepositoryError> {
        self.validate_outer_controls_or_seal()?;
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

    fn validate_outer_controls_or_seal(&mut self) -> Result<(), ExtensionRepositoryError> {
        let validation = self.writer_validate_outer_controls();
        match validation {
            Ok(()) => Ok(()),
            Err(error) => {
                self.writer_seal();
                Err(error)
            }
        }
    }

    fn validate_outer_destructive_boundary_or_seal(
        &mut self,
    ) -> Result<(), ExtensionRepositoryError> {
        let validation = self
            .writer_validate_outer_controls()
            .and_then(|()| self.writer_validate_catalog_object_inventory());
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
                    .validate_outer_destructive_boundary_or_seal()
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
    work: &mut GarbageCollectionWork,
) -> Result<(), PhysicalCollectionError> {
    // Metadata first: a surviving package marker always retains its whole
    // closure. Outer authenticated catalog bytes remain last.
    for target in &intent.catalog_set_record_ids {
        if remove_present_regular(
            &runtime._records,
            &materialization::gc_catalog_set_record(*target),
            runtime._catalog_sets.contains_key(target),
        )? {
            work.regular_targets_removed = checked_work_add(work.regular_targets_removed, 1)?;
        }
    }
    for target in &intent.package_record_ids {
        if remove_present_regular(
            &runtime._records,
            &materialization::gc_package_record(*target),
            runtime._package_records.contains_key(target),
        )? {
            work.regular_targets_removed = checked_work_add(work.regular_targets_removed, 1)?;
        }
    }
    for target in &intent.tree_index_ids {
        if remove_present_regular(
            &runtime._records,
            &materialization::gc_tree_index_object(*target),
            runtime._tree_index_ids.contains(target),
        )? {
            work.regular_targets_removed = checked_work_add(work.regular_targets_removed, 1)?;
        }
    }
    for target in &intent.legal_artifact_ids {
        if remove_present_regular(
            &runtime._records,
            &materialization::gc_legal_object(*target),
            runtime._legal_artifact_ids.contains(target),
        )? {
            work.regular_targets_removed = checked_work_add(work.regular_targets_removed, 1)?;
        }
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
            let report = remove_tree_directory(opened).map_err(map_tree_cleanup)?;
            work.record_tree(report)?;
        }
    }
    for target in &intent.tree_objects {
        let retired_name = materialization::gc_tree_retired(target.tree_sha256, intent.generation)
            .map_err(PhysicalCollectionError::Clean)?;
        let (opened, freshly_retired) = if runtime._tree_object_ids.contains(&target.tree_sha256) {
            let object_name = materialization::gc_tree_object(target.tree_sha256);
            let object = runtime
                ._trees
                .open_sealed_private_child(&object_name)
                .map_err(map_physical_fs)?;
            let retired = object
                .publish_noreplace(&runtime._trees, &retired_name)
                .map_err(map_tree_transition)?;
            work.tree_object_retirements = checked_work_add(work.tree_object_retirements, 1)?;
            (Some(OpenedPrivateDirectory::Sealed(retired)), true)
        } else if runtime
            ._retired_tree_ids
            .contains(&(target.tree_sha256, intent.generation))
        {
            (
                Some(
                    runtime
                        ._trees
                        .open_private_child_any_mode(&retired_name)
                        .map_err(map_physical_fs)?,
                ),
                false,
            )
        } else {
            (None, false)
        };
        if let Some(opened) = opened {
            let max_entries = target.known_total_entry_count.map_or(
                zephium_extension_package::MAX_EXTENSION_TREE_ENTRIES,
                |entries| entries as usize,
            );
            let report =
                remove_tree_directory_bounded(opened, max_entries).map_err(map_tree_cleanup)?;
            if freshly_retired
                && target
                    .known_total_entry_count
                    .is_some_and(|expected| report.observed_entries() != expected as usize)
            {
                return Err(PhysicalCollectionError::MustSeal(
                    ExtensionRepositoryError::SettlementAmbiguous,
                ));
            }
            work.record_tree(report)?;
        }
    }
    Ok(())
}

fn remove_present_regular(
    directory: &zephium_private_fs::PrivateDirectory,
    name: &zephium_private_fs::PrivateComponent,
    present: bool,
) -> Result<bool, PhysicalCollectionError> {
    if present
        && !directory
            .remove_verified_regular(name)
            .map_err(map_physical_fs)?
    {
        return Err(PhysicalCollectionError::Clean(
            ExtensionRepositoryError::RecoveryAmbiguous,
        ));
    }
    Ok(present)
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
