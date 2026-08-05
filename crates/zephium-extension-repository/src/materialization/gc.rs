//! Pure bounded garbage-collection planning.
//!
//! Planning proves reachability against one immutable recovered inventory. It
//! performs no filesystem mutation and grants no deletion authority. Durable
//! intent publication and physical settlement are separate transitions.

use std::collections::{BTreeMap, BTreeSet};

use super::policy::{
    reserve_two_transition_intent_generation, validate_catalog_set_tree_budget,
    validate_completed_tree_budget, validate_package_anchor_consistency,
};
use super::records::{CatalogAnchor, CatalogSetRecord, PackageRecord};
use super::runtime::MaterializationRuntime;
use super::state::{
    GarbageCollectionRetiredTree, GarbageCollectionTreeObject, MaterializationBuildIntent,
    MaterializationGarbageCollectionIntent, MaterializationState,
    MATERIALIZATION_GC_INTENT_SCHEMA_VERSION, MAX_GC_CATALOG_OBJECT_TARGETS,
    MAX_GC_CATALOG_SET_TARGETS, MAX_GC_DATA_OBJECT_TARGETS, MAX_GC_KNOWN_TREE_BYTES,
    MAX_GC_PACKAGE_RECORD_TARGETS, MAX_GC_TREE_ENTRIES, MAX_GC_TREE_JOBS,
};
use crate::state::Digest32;
use crate::ExtensionRepositoryError;
use zephium_private_fs::DirectoryIdentity;

/// One deterministic bounded batch and the completed ledger after its commit.
///
/// `more_garbage` is computed against the same immutable inventory. It is a
/// scheduling hint only; every later batch must recover and prove reachability
/// again rather than reusing this plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GarbageCollectionPlan {
    /// Exact transition-1 successor. Target package records are pruned from
    /// the completed ledger atomically with durable intent publication.
    next_state: MaterializationState,
    more_garbage: bool,
}

/// Live proof that every target of one durable collection intent is absent.
///
/// This value is deliberately non-serializable. The repository constructs it
/// only after a fresh combined inventory and binds it to both mutable object
/// parents before the completion transition may clear durable intent.
pub(crate) struct GarbageCollectionAbsenceProof {
    intent: MaterializationGarbageCollectionIntent,
    state_sha256: Digest32,
    generation: u64,
    records_parent: DirectoryIdentity,
    trees_parent: DirectoryIdentity,
    catalogs_parent: DirectoryIdentity,
}

impl GarbageCollectionAbsenceProof {
    pub(super) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) const fn records_parent(&self) -> DirectoryIdentity {
        self.records_parent
    }

    pub(super) const fn trees_parent(&self) -> DirectoryIdentity {
        self.trees_parent
    }

    pub(super) const fn catalogs_parent(&self) -> DirectoryIdentity {
        self.catalogs_parent
    }

    pub(super) fn intent(&self) -> &MaterializationGarbageCollectionIntent {
        &self.intent
    }

    pub(super) const fn state_sha256(&self) -> Digest32 {
        self.state_sha256
    }
}

impl GarbageCollectionPlan {
    pub(super) fn into_state(self) -> MaterializationState {
        self.next_state
    }
}

pub(crate) fn prove_garbage_collection_absence(
    runtime: &MaterializationRuntime,
    catalog_object_ids: &BTreeSet<Digest32>,
    catalogs_parent: DirectoryIdentity,
) -> Result<GarbageCollectionAbsenceProof, ExtensionRepositoryError> {
    let intent = runtime
        ._gc_intent
        .as_ref()
        .filter(|intent| runtime._state.gc_intent.as_ref() == Some(*intent))
        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
    if intent
        .catalog_object_ids
        .iter()
        .any(|target| catalog_object_ids.contains(target))
        || intent
            .catalog_set_record_ids
            .iter()
            .any(|target| runtime._catalog_sets.contains_key(target))
        || intent
            .package_record_ids
            .iter()
            .any(|target| runtime._package_records.contains_key(target))
        || intent
            .tree_index_ids
            .iter()
            .any(|target| runtime._tree_index_ids.contains(target))
        || intent
            .legal_artifact_ids
            .iter()
            .any(|target| runtime._legal_artifact_ids.contains(target))
        || intent.tree_objects.iter().any(|target| {
            runtime._tree_object_ids.contains(&target.tree_sha256)
                || runtime
                    ._retired_tree_ids
                    .iter()
                    .any(|(digest, _)| *digest == target.tree_sha256)
        })
        || intent.retired_trees.iter().any(|target| {
            runtime._tree_object_ids.contains(&target.tree_sha256)
                || runtime
                    ._retired_tree_ids
                    .iter()
                    .any(|(digest, _)| *digest == target.tree_sha256)
        })
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(GarbageCollectionAbsenceProof {
        intent: intent.clone(),
        state_sha256: crate::codec::digest(&runtime._state_bytes),
        generation: intent.generation,
        records_parent: runtime._records.identity(),
        trees_parent: runtime._trees.identity(),
        catalogs_parent,
    })
}

/// Plans against the exact identities retained by repository recovery.
pub(crate) fn plan_garbage_collection(
    runtime: &MaterializationRuntime,
    catalog_high_water: Option<CatalogAnchor>,
    catalog_object_ids: &BTreeSet<Digest32>,
) -> Result<Option<GarbageCollectionPlan>, ExtensionRepositoryError> {
    let inventory = GarbageCollectionInventory {
        catalog_high_water,
        catalog_object_ids,
        catalog_sets: &runtime._catalog_sets,
        package_records: &runtime._package_records,
        tree_index_ids: &runtime._tree_index_ids,
        legal_artifact_ids: &runtime._legal_artifact_ids,
        tree_object_ids: &runtime._tree_object_ids,
        retired_tree_ids: &runtime._retired_tree_ids,
    };
    plan_inventory(
        &runtime._state,
        &runtime._build_intent,
        &runtime._gc_intent,
        inventory,
    )
}

#[derive(Clone, Copy)]
struct GarbageCollectionInventory<'a> {
    catalog_high_water: Option<CatalogAnchor>,
    catalog_object_ids: &'a BTreeSet<Digest32>,
    catalog_sets: &'a BTreeMap<Digest32, CatalogSetRecord>,
    package_records: &'a BTreeMap<Digest32, PackageRecord>,
    tree_index_ids: &'a BTreeSet<Digest32>,
    legal_artifact_ids: &'a BTreeSet<Digest32>,
    tree_object_ids: &'a BTreeSet<Digest32>,
    retired_tree_ids: &'a BTreeSet<(Digest32, u64)>,
}

fn plan_inventory(
    state: &MaterializationState,
    runtime_build_intent: &Option<MaterializationBuildIntent>,
    runtime_gc_intent: &Option<MaterializationGarbageCollectionIntent>,
    inventory: GarbageCollectionInventory<'_>,
) -> Result<Option<GarbageCollectionPlan>, ExtensionRepositoryError> {
    validate_planning_frontier(state, runtime_build_intent, runtime_gc_intent, inventory)?;

    let rooted_set_ids = state.catalog_pin_ids().into_iter().collect::<BTreeSet<_>>();
    // Outer monotonic authority roots only its exact catalog object. Package
    // closures remain cache data unless a selection slot or owner roots their
    // whole catalog set.
    let mut rooted_catalogs = BTreeSet::new();
    for set_id in &rooted_set_ids {
        let set = inventory
            .catalog_sets
            .get(set_id)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        rooted_catalogs.insert(set.catalog);
    }

    let residue = collect_residue(inventory);
    if residue.has_any() {
        let generation = reserve_intent_generation(state.generation)?;
        let intent = residue.bounded_intent(generation);
        intent.validate(generation)?;
        let more_garbage = residue.has_deferred(&intent)
            || inventory
                .package_records
                .values()
                .any(|record| !rooted_catalogs.contains(&record.catalog))
            || inventory
                .catalog_sets
                .values()
                .any(|record| !rooted_catalogs.contains(&record.catalog));
        return Ok(Some(GarbageCollectionPlan {
            next_state: planned_state(
                state,
                generation,
                state.completed_package_record_ids.clone(),
                intent,
            )?,
            more_garbage,
        }));
    }

    let dead_catalogs = inventory
        .package_records
        .values()
        .map(|record| record.catalog)
        .chain(inventory.catalog_sets.values().map(|record| record.catalog))
        .filter(|catalog| !rooted_catalogs.contains(catalog))
        .collect::<BTreeSet<_>>();
    let Some(cohort) = dead_catalogs.first().copied() else {
        return Ok(None);
    };
    let generation = reserve_intent_generation(state.generation)?;
    let plan = plan_catalog_cohort(
        state,
        inventory,
        cohort,
        generation,
        dead_catalogs.len() > 1,
    )?;
    plan.next_state.validate()?;
    Ok(Some(plan))
}

fn planned_state(
    state: &MaterializationState,
    generation: u64,
    retained_completed_package_record_ids: Vec<Digest32>,
    intent: MaterializationGarbageCollectionIntent,
) -> Result<MaterializationState, ExtensionRepositoryError> {
    let mut next = state.clone();
    next.generation = generation;
    next.completed_package_record_ids = retained_completed_package_record_ids;
    next.gc_intent = Some(intent);
    next.validate()?;
    Ok(next)
}

fn reserve_intent_generation(current: u64) -> Result<u64, ExtensionRepositoryError> {
    reserve_two_transition_intent_generation(current)
        .ok_or(ExtensionRepositoryError::GenerationExhausted)
}

fn validate_planning_frontier(
    state: &MaterializationState,
    runtime_build_intent: &Option<MaterializationBuildIntent>,
    runtime_gc_intent: &Option<MaterializationGarbageCollectionIntent>,
    inventory: GarbageCollectionInventory<'_>,
) -> Result<(), ExtensionRepositoryError> {
    state.validate()?;
    if &state.build_intent != runtime_build_intent
        || &state.gc_intent != runtime_gc_intent
        || state.build_intent.is_some()
        || state.gc_intent.is_some()
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if state.completed_package_record_ids.len() != inventory.package_records.len()
        || state
            .completed_package_record_ids
            .iter()
            .any(|record_id| !inventory.package_records.contains_key(record_id))
        || inventory.tree_object_ids.iter().any(|tree_id| {
            inventory
                .retired_tree_ids
                .iter()
                .any(|(id, _)| id == tree_id)
        })
        || inventory
            .retired_tree_ids
            .iter()
            .any(|(_, generation)| *generation == 0 || *generation > state.generation)
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if let Some(high_water) = inventory.catalog_high_water {
        high_water.validate()?;
        if !inventory
            .catalog_object_ids
            .contains(&high_water.catalog_sha256)
            || inventory.package_records.values().any(|record| {
                record.catalog.catalog_sha256 == high_water.catalog_sha256
                    && record.catalog != high_water
            })
            || inventory.catalog_sets.values().any(|record| {
                record.catalog.catalog_sha256 == high_water.catalog_sha256
                    && record.catalog != high_water
            })
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }

    validate_package_anchor_consistency(inventory.package_records.values())
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    validate_completed_tree_budget(inventory.package_records.values())
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    for (record_id, record) in inventory.package_records {
        record.validate()?;
        if record.record_id()? != *record_id
            || !inventory
                .catalog_object_ids
                .contains(&record.catalog.catalog_sha256)
            || !inventory
                .tree_index_ids
                .contains(&record.tree_index.index_sha256)
            || !inventory.legal_artifact_ids.contains(&record.legal.sha256)
            || !inventory
                .tree_object_ids
                .contains(&record.tree_index.tree_sha256)
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    for (set_id, set) in inventory.catalog_sets {
        set.validate()?;
        if set.record_id()? != *set_id
            || !inventory
                .catalog_object_ids
                .contains(&set.catalog.catalog_sha256)
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let mut set_packages = Vec::with_capacity(set.packages.len());
        for row in &set.packages {
            let package = inventory
                .package_records
                .get(&row.package_record_id)
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            if package.catalog != set.catalog
                || package.package.package_key != row.package_key
                || package.manifest.runtime_target != row.runtime_target
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            set_packages.push(package);
        }
        validate_catalog_set_tree_budget(set_packages)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    }
    for set_id in state.catalog_pin_ids() {
        if !inventory.catalog_sets.contains_key(&set_id) {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    for pin in &state.package_pins {
        let set = inventory
            .catalog_sets
            .get(&pin.catalog_set_record_id)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if !set
            .packages
            .iter()
            .any(|row| row.package_record_id == pin.package_record_id)
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    Ok(())
}

struct GarbageResidue {
    catalog_object_ids: Vec<Digest32>,
    tree_index_ids: Vec<Digest32>,
    legal_artifact_ids: Vec<Digest32>,
    tree_object_ids: Vec<Digest32>,
    retired_tree_ids: Vec<(Digest32, u64)>,
}

impl GarbageResidue {
    fn has_any(&self) -> bool {
        !self.catalog_object_ids.is_empty()
            || !self.tree_index_ids.is_empty()
            || !self.legal_artifact_ids.is_empty()
            || !self.tree_object_ids.is_empty()
            || !self.retired_tree_ids.is_empty()
    }

    fn bounded_intent(&self, generation: u64) -> MaterializationGarbageCollectionIntent {
        let mut remaining_tree_jobs = MAX_GC_TREE_JOBS;
        let retired_trees = self
            .retired_tree_ids
            .iter()
            .take(remaining_tree_jobs)
            .map(
                |(tree_sha256, retirement_generation)| GarbageCollectionRetiredTree {
                    tree_sha256: *tree_sha256,
                    retirement_generation: *retirement_generation,
                },
            )
            .collect::<Vec<_>>();
        remaining_tree_jobs -= retired_trees.len();
        let tree_objects = self
            .tree_object_ids
            .iter()
            .take(remaining_tree_jobs)
            .map(|tree_sha256| GarbageCollectionTreeObject {
                tree_sha256: *tree_sha256,
                known_total_entry_count: None,
                known_tree_bytes: None,
            })
            .collect();
        MaterializationGarbageCollectionIntent {
            schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
            generation,
            cohort: None,
            catalog_object_ids: self
                .catalog_object_ids
                .iter()
                .take(MAX_GC_CATALOG_OBJECT_TARGETS)
                .copied()
                .collect(),
            catalog_set_record_ids: Vec::new(),
            package_record_ids: Vec::new(),
            tree_index_ids: self
                .tree_index_ids
                .iter()
                .take(MAX_GC_DATA_OBJECT_TARGETS)
                .copied()
                .collect(),
            legal_artifact_ids: self
                .legal_artifact_ids
                .iter()
                .take(MAX_GC_DATA_OBJECT_TARGETS)
                .copied()
                .collect(),
            tree_objects,
            retired_trees,
        }
    }

    fn has_deferred(&self, intent: &MaterializationGarbageCollectionIntent) -> bool {
        self.catalog_object_ids.len() > intent.catalog_object_ids.len()
            || self.tree_index_ids.len() > intent.tree_index_ids.len()
            || self.legal_artifact_ids.len() > intent.legal_artifact_ids.len()
            || self.tree_object_ids.len() > intent.tree_objects.len()
            || self.retired_tree_ids.len() > intent.retired_trees.len()
    }
}

fn collect_residue(inventory: GarbageCollectionInventory<'_>) -> GarbageResidue {
    let referenced_catalogs = inventory
        .package_records
        .values()
        .map(|record| record.catalog.catalog_sha256)
        .chain(
            inventory
                .catalog_sets
                .values()
                .map(|record| record.catalog.catalog_sha256),
        )
        .chain(
            inventory
                .catalog_high_water
                .map(|catalog| catalog.catalog_sha256),
        )
        .collect::<BTreeSet<_>>();
    let referenced_indexes = inventory
        .package_records
        .values()
        .map(|record| record.tree_index.index_sha256)
        .collect::<BTreeSet<_>>();
    let referenced_legal = inventory
        .package_records
        .values()
        .map(|record| record.legal.sha256)
        .collect::<BTreeSet<_>>();
    let referenced_trees = inventory
        .package_records
        .values()
        .map(|record| record.tree_index.tree_sha256)
        .collect::<BTreeSet<_>>();
    GarbageResidue {
        catalog_object_ids: inventory
            .catalog_object_ids
            .difference(&referenced_catalogs)
            .copied()
            .collect(),
        tree_index_ids: inventory
            .tree_index_ids
            .difference(&referenced_indexes)
            .copied()
            .collect(),
        legal_artifact_ids: inventory
            .legal_artifact_ids
            .difference(&referenced_legal)
            .copied()
            .collect(),
        tree_object_ids: inventory
            .tree_object_ids
            .difference(&referenced_trees)
            .copied()
            .collect(),
        retired_tree_ids: inventory.retired_tree_ids.iter().copied().collect(),
    }
}

fn plan_catalog_cohort(
    state: &MaterializationState,
    inventory: GarbageCollectionInventory<'_>,
    cohort: CatalogAnchor,
    generation: u64,
    mut more_garbage: bool,
) -> Result<GarbageCollectionPlan, ExtensionRepositoryError> {
    let catalog_set_record_ids = inventory
        .catalog_sets
        .iter()
        .filter_map(|(set_id, set)| (set.catalog == cohort).then_some(*set_id))
        .collect::<Vec<_>>();
    let package_record_ids = inventory
        .package_records
        .iter()
        .filter_map(|(record_id, record)| (record.catalog == cohort).then_some(*record_id))
        .collect::<Vec<_>>();
    if catalog_set_record_ids.len() > MAX_GC_CATALOG_SET_TARGETS
        || package_record_ids.len() > MAX_GC_PACKAGE_RECORD_TARGETS
        || catalog_set_record_ids.is_empty() && package_record_ids.is_empty()
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    let target_packages = package_record_ids.iter().copied().collect::<BTreeSet<_>>();
    let retained_completed_package_record_ids = state
        .completed_package_record_ids
        .iter()
        .filter(|record_id| !target_packages.contains(record_id))
        .copied()
        .collect::<Vec<_>>();

    let deletable_indexes =
        exclusively_referenced_ids(inventory.package_records, &target_packages, |record| {
            record.tree_index.index_sha256
        });
    let deletable_legal =
        exclusively_referenced_ids(inventory.package_records, &target_packages, |record| {
            record.legal.sha256
        });
    let deletable_trees =
        exclusively_referenced_ids(inventory.package_records, &target_packages, |record| {
            record.tree_index.tree_sha256
        });
    let tree_index_ids = deletable_indexes
        .iter()
        .take(MAX_GC_DATA_OBJECT_TARGETS)
        .copied()
        .collect::<Vec<_>>();
    let legal_artifact_ids = deletable_legal
        .iter()
        .take(MAX_GC_DATA_OBJECT_TARGETS)
        .copied()
        .collect::<Vec<_>>();
    more_garbage |= deletable_indexes.len() > tree_index_ids.len()
        || deletable_legal.len() > legal_artifact_ids.len();

    let mut tree_objects = Vec::new();
    let mut known_entries = 0_usize;
    let mut known_bytes = 0_u64;
    for tree_sha256 in deletable_trees {
        let record = inventory
            .package_records
            .values()
            .find(|record| record.tree_index.tree_sha256 == tree_sha256)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        let next_entries = known_entries
            .checked_add(record.tree_index.total_entry_count as usize)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        let next_bytes = known_bytes
            .checked_add(record.tree_index.tree_bytes)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if tree_objects.len() == MAX_GC_TREE_JOBS
            || next_entries > MAX_GC_TREE_ENTRIES
            || next_bytes > MAX_GC_KNOWN_TREE_BYTES
        {
            more_garbage = true;
            continue;
        }
        tree_objects.push(GarbageCollectionTreeObject {
            tree_sha256,
            known_total_entry_count: Some(record.tree_index.total_entry_count),
            known_tree_bytes: Some(record.tree_index.tree_bytes),
        });
        known_entries = next_entries;
        known_bytes = next_bytes;
    }

    let catalog_object_ids = inventory
        .catalog_object_ids
        .contains(&cohort.catalog_sha256)
        .then_some(cohort.catalog_sha256)
        .filter(|_| inventory.catalog_high_water != Some(cohort))
        .into_iter()
        .collect();
    let intent = MaterializationGarbageCollectionIntent {
        schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
        generation,
        cohort: Some(cohort),
        catalog_object_ids,
        catalog_set_record_ids,
        package_record_ids,
        tree_index_ids,
        legal_artifact_ids,
        tree_objects,
        retired_trees: Vec::new(),
    };
    Ok(GarbageCollectionPlan {
        next_state: planned_state(
            state,
            generation,
            retained_completed_package_record_ids,
            intent,
        )?,
        more_garbage,
    })
}

fn exclusively_referenced_ids(
    packages: &BTreeMap<Digest32, PackageRecord>,
    target_packages: &BTreeSet<Digest32>,
    identity: impl Fn(&PackageRecord) -> Digest32,
) -> BTreeSet<Digest32> {
    let mut references = BTreeMap::<Digest32, bool>::new();
    for (record_id, record) in packages {
        let target = target_packages.contains(record_id);
        references
            .entry(identity(record))
            .and_modify(|only_targeted| *only_targeted &= target)
            .or_insert(target);
    }
    references
        .into_iter()
        .filter_map(|(id, only_targeted)| only_targeted.then_some(id))
        .collect()
}

#[cfg(test)]
mod tests;
