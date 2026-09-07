use std::collections::BTreeSet;

use super::{
    validate_catalog_set_tree_budget, validate_completed_tree_budget,
    validate_package_anchor_consistency, CatalogAnchor, Digest32, ExtensionRepositoryError,
    MaterializationState, RecordInventory, TreeInventory,
};

pub(super) fn validate_prepared_gc_transition_shape(
    current: &MaterializationState,
    next: &MaterializationState,
) -> Result<(), ExtensionRepositoryError> {
    match (&current.gc_intent, &next.gc_intent) {
        (None, Some(intent)) => {
            let mut reconstructed = next.completed_package_record_ids.clone();
            reconstructed.extend(&intent.package_record_ids);
            reconstructed.sort_unstable();
            if reconstructed.windows(2).any(|pair| pair[0] >= pair[1])
                || reconstructed != current.completed_package_record_ids
                || current.generation.checked_add(1) != Some(next.generation)
                || next.schema_version != current.schema_version
                || next.candidate_catalog_set_id != current.candidate_catalog_set_id
                || next.current_catalog_set_id != current.current_catalog_set_id
                || next.previous_catalog_set_id != current.previous_catalog_set_id
                || next.package_pins != current.package_pins
                || current.build_intent.is_some()
                || next.build_intent.is_some()
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }
        (Some(intent), None) => {
            if current.generation.checked_add(1) != Some(next.generation)
                || next.schema_version != current.schema_version
                || next.completed_package_record_ids != current.completed_package_record_ids
                || next.candidate_catalog_set_id != current.candidate_catalog_set_id
                || next.current_catalog_set_id != current.current_catalog_set_id
                || next.previous_catalog_set_id != current.previous_catalog_set_id
                || next.package_pins != current.package_pins
                || current.build_intent.is_some()
                || next.build_intent.is_some()
                || intent.generation != current.generation
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }
        (None, None) => {}
        (Some(_), Some(_)) => return Err(ExtensionRepositoryError::RecoveryAmbiguous),
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) struct GarbageCollectionFrontier {
    pub(super) all_intact: bool,
    pub(super) all_absent: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum GarbageCollectionTargetPhase {
    Removed,
    Retiring,
    Intact,
}

/// Proves the one canonical crash frontier of a durable collection batch.
///
/// Targets are removed in their encoded order across catalog sets, package
/// markers, indexes, legal objects, retired trees, tree objects, and finally
/// outer catalog objects. Recovery accepts only an absent prefix followed by
/// at most one tree-retirement boundary and an intact suffix. This rules out
/// arbitrary holes while allowing every actual process-loss point.
pub(super) fn validate_gc_partial_frontier(
    state: &MaterializationState,
    records: &RecordInventory,
    trees: &TreeInventory,
    catalog_object_ids: Option<&BTreeSet<Digest32>>,
    catalog_high_water: Option<CatalogAnchor>,
) -> Result<GarbageCollectionFrontier, ExtensionRepositoryError> {
    state.validate()?;
    let Some(intent) = &state.gc_intent else {
        return Ok(GarbageCollectionFrontier {
            all_intact: true,
            all_absent: true,
        });
    };

    let mut phases = Vec::with_capacity(
        intent.catalog_set_record_ids.len()
            + intent.package_record_ids.len()
            + intent.tree_index_ids.len()
            + intent.legal_artifact_ids.len()
            + intent.retired_trees.len()
            + intent.tree_objects.len()
            + intent.catalog_object_ids.len(),
    );
    phases.extend(intent.catalog_set_record_ids.iter().map(|target| {
        if records.catalog_sets.contains_key(target) {
            GarbageCollectionTargetPhase::Intact
        } else {
            GarbageCollectionTargetPhase::Removed
        }
    }));
    phases.extend(intent.package_record_ids.iter().map(|target| {
        if records.packages.contains_key(target) {
            GarbageCollectionTargetPhase::Intact
        } else {
            GarbageCollectionTargetPhase::Removed
        }
    }));
    phases.extend(intent.tree_index_ids.iter().map(|target| {
        if records.tree_indexes.contains(target) {
            GarbageCollectionTargetPhase::Intact
        } else {
            GarbageCollectionTargetPhase::Removed
        }
    }));
    phases.extend(intent.legal_artifact_ids.iter().map(|target| {
        if records.legal_artifacts.contains(target) {
            GarbageCollectionTargetPhase::Intact
        } else {
            GarbageCollectionTargetPhase::Removed
        }
    }));
    for target in &intent.retired_trees {
        let exact = trees
            .retired
            .contains_key(&(target.tree_sha256, target.retirement_generation));
        let alias = trees.objects.contains_key(&target.tree_sha256)
            || trees.retired.keys().any(|(digest, generation)| {
                *digest == target.tree_sha256 && *generation != target.retirement_generation
            });
        if alias {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        phases.push(if exact {
            GarbageCollectionTargetPhase::Intact
        } else {
            GarbageCollectionTargetPhase::Removed
        });
    }
    for target in &intent.tree_objects {
        let object = trees.objects.contains_key(&target.tree_sha256);
        let retiring = trees
            .retired
            .contains_key(&(target.tree_sha256, intent.generation));
        let wrong_retirement = trees.retired.keys().any(|(digest, generation)| {
            *digest == target.tree_sha256 && *generation != intent.generation
        });
        if wrong_retirement || object && retiring {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        phases.push(if object {
            GarbageCollectionTargetPhase::Intact
        } else if retiring {
            GarbageCollectionTargetPhase::Retiring
        } else {
            GarbageCollectionTargetPhase::Removed
        });
    }
    if !intent.catalog_object_ids.is_empty() && catalog_object_ids.is_none() {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    phases.extend(intent.catalog_object_ids.iter().map(|target| {
        if catalog_object_ids.is_some_and(|objects| objects.contains(target)) {
            GarbageCollectionTargetPhase::Intact
        } else {
            GarbageCollectionTargetPhase::Removed
        }
    }));

    let mut intact_suffix_started = false;
    let mut retiring_seen = false;
    for phase in &phases {
        match phase {
            GarbageCollectionTargetPhase::Removed if intact_suffix_started || retiring_seen => {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            GarbageCollectionTargetPhase::Retiring if intact_suffix_started || retiring_seen => {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            GarbageCollectionTargetPhase::Retiring => retiring_seen = true,
            GarbageCollectionTargetPhase::Intact => intact_suffix_started = true,
            GarbageCollectionTargetPhase::Removed => {}
        }
    }
    let frontier = GarbageCollectionFrontier {
        all_intact: phases
            .iter()
            .all(|phase| *phase == GarbageCollectionTargetPhase::Intact),
        all_absent: phases
            .iter()
            .all(|phase| *phase == GarbageCollectionTargetPhase::Removed),
    };

    if frontier.all_intact {
        validate_gc_intact_semantics(
            state,
            records,
            trees,
            catalog_object_ids,
            catalog_high_water,
        )?;
        return Ok(frontier);
    }

    validate_gc_partial_semantics(
        state,
        records,
        trees,
        catalog_object_ids,
        catalog_high_water,
    )?;
    Ok(frontier)
}

fn validate_gc_partial_semantics(
    state: &MaterializationState,
    records: &RecordInventory,
    trees: &TreeInventory,
    catalog_object_ids: Option<&BTreeSet<Digest32>>,
    catalog_high_water: Option<CatalogAnchor>,
) -> Result<(), ExtensionRepositoryError> {
    let intent = state
        .gc_intent
        .as_ref()
        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
    let target_packages = intent
        .package_record_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let target_sets = intent
        .catalog_set_record_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let target_indexes = intent
        .tree_index_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let target_legal = intent
        .legal_artifact_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let target_trees = intent
        .tree_objects
        .iter()
        .map(|target| target.tree_sha256)
        .collect::<BTreeSet<_>>();

    if state
        .catalog_pin_ids()
        .iter()
        .any(|set_id| target_sets.contains(set_id))
        || state.package_pins.iter().any(|pin| {
            target_sets.contains(&pin.catalog_set_record_id)
                || target_packages.contains(&pin.package_record_id)
        })
        || catalog_high_water.is_some_and(|high_water| {
            intent
                .catalog_object_ids
                .binary_search(&high_water.catalog_sha256)
                .is_ok()
        })
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if catalog_object_ids.is_some_and(|objects| {
        intent.catalog_object_ids.iter().any(|target| {
            objects.contains(target)
                && Some(*target) == catalog_high_water.map(|h| h.catalog_sha256)
        })
    }) {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }

    // Retained records remain the positive root graph throughout settlement.
    // No target CAS identity may be referenced by one of them.
    for (record_id, record) in &records.packages {
        if target_packages.contains(record_id) {
            if intent.cohort.is_none_or(|cohort| record.catalog != cohort)
                || !records
                    .tree_indexes
                    .contains(&record.tree_index.index_sha256)
                || !records.legal_artifacts.contains(&record.legal.sha256)
                || !(trees.objects.contains_key(&record.tree_index.tree_sha256)
                    || trees
                        .retired
                        .contains_key(&(record.tree_index.tree_sha256, intent.generation)))
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            continue;
        }
        if target_indexes.contains(&record.tree_index.index_sha256)
            || target_legal.contains(&record.legal.sha256)
            || target_trees.contains(&record.tree_index.tree_sha256)
            || intent
                .catalog_object_ids
                .binary_search(&record.catalog.catalog_sha256)
                .is_ok()
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    for (set_id, set) in &records.catalog_sets {
        if target_sets.contains(set_id) {
            if intent.cohort.is_none_or(|cohort| set.catalog != cohort)
                || set
                    .packages
                    .iter()
                    .any(|row| !target_packages.contains(&row.package_record_id))
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            continue;
        }
        if intent
            .catalog_object_ids
            .binary_search(&set.catalog.catalog_sha256)
            .is_ok()
            || set
                .packages
                .iter()
                .any(|row| target_packages.contains(&row.package_record_id))
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    if intent.cohort.is_none() && (!target_packages.is_empty() || !target_sets.is_empty()) {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if let Some(cohort) = intent.cohort {
        if records.packages.iter().any(|(record_id, record)| {
            record.catalog == cohort && !target_packages.contains(record_id)
        }) || records
            .catalog_sets
            .iter()
            .any(|(set_id, set)| set.catalog == cohort && !target_sets.contains(set_id))
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    Ok(())
}

/// Proves the fully intact semantic frontier before a new batch mutates data.
fn validate_gc_intact_semantics(
    state: &MaterializationState,
    records: &RecordInventory,
    trees: &TreeInventory,
    catalog_object_ids: Option<&BTreeSet<Digest32>>,
    catalog_high_water: Option<CatalogAnchor>,
) -> Result<(), ExtensionRepositoryError> {
    state.validate()?;
    let Some(intent) = &state.gc_intent else {
        return Ok(());
    };

    let target_packages = intent
        .package_record_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let target_sets = intent
        .catalog_set_record_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let target_indexes = intent
        .tree_index_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let target_legal = intent
        .legal_artifact_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let target_trees = intent
        .tree_objects
        .iter()
        .map(|tree| tree.tree_sha256)
        .collect::<BTreeSet<_>>();

    // Every declared target must still exist under its exact final name. Test
    // fixtures intentionally have no outer catalog namespace; they may prove
    // only plans that do not target an outer catalog object.
    if intent
        .catalog_object_ids
        .iter()
        .any(|target| catalog_object_ids.is_none_or(|objects| !objects.contains(target)))
        || intent
            .catalog_set_record_ids
            .iter()
            .any(|target| !records.catalog_sets.contains_key(target))
        || intent
            .package_record_ids
            .iter()
            .any(|target| !records.packages.contains_key(target))
        || intent
            .tree_index_ids
            .iter()
            .any(|target| !records.tree_indexes.contains(target))
        || intent
            .legal_artifact_ids
            .iter()
            .any(|target| !records.legal_artifacts.contains(target))
        || intent
            .tree_objects
            .iter()
            .any(|target| !trees.objects.contains_key(&target.tree_sha256))
        || intent.retired_trees.iter().any(|target| {
            !trees
                .retired
                .contains_key(&(target.tree_sha256, target.retirement_generation))
        })
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }

    // A plan must never target a selected/owner-pinned set or package. State
    // validation already makes targeted packages disjoint from the completed
    // ledger; spell the owner relation out here so this remains true if the
    // ledger representation changes.
    if state
        .catalog_pin_ids()
        .iter()
        .any(|set_id| target_sets.contains(set_id))
        || state.package_pins.iter().any(|pin| {
            target_sets.contains(&pin.catalog_set_record_id)
                || target_packages.contains(&pin.package_record_id)
        })
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }

    validate_package_anchor_consistency(records.packages.values())
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;

    // Transition 1 removes target markers from the completed ledger but does
    // not delete anything. Their complete closure must therefore remain
    // intact even when an object was omitted from this bounded CAS sub-batch.
    // A later deletion slice may relax this only after marker deletion is
    // represented by an explicit typed phase.
    for record_id in &intent.package_record_ids {
        let record = records
            .packages
            .get(record_id)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if !records
            .tree_indexes
            .contains(&record.tree_index.index_sha256)
            || !records.legal_artifacts.contains(&record.legal.sha256)
            || !trees.objects.contains_key(&record.tree_index.tree_sha256)
            || catalog_object_ids
                .is_some_and(|objects| !objects.contains(&record.catalog.catalog_sha256))
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    let preplan_packages = state
        .completed_package_record_ids
        .iter()
        .chain(&intent.package_record_ids)
        .map(|record_id| {
            records
                .packages
                .get(record_id)
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_completed_tree_budget(preplan_packages)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;

    // Content-addressed objects may be shared across catalog cohorts. A
    // target is deletable only when no retained package record names it.
    for (record_id, record) in &records.packages {
        if target_packages.contains(record_id) {
            continue;
        }
        if target_indexes.contains(&record.tree_index.index_sha256)
            || target_legal.contains(&record.legal.sha256)
            || target_trees.contains(&record.tree_index.tree_sha256)
            || intent
                .catalog_object_ids
                .binary_search(&record.catalog.catalog_sha256)
                .is_ok()
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    for (set_id, set) in &records.catalog_sets {
        if !target_sets.contains(set_id)
            && (intent
                .catalog_object_ids
                .binary_search(&set.catalog.catalog_sha256)
                .is_ok()
                || set
                    .packages
                    .iter()
                    .any(|row| target_packages.contains(&row.package_record_id)))
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
    }
    if catalog_high_water.is_some_and(|high_water| {
        intent
            .catalog_object_ids
            .binary_search(&high_water.catalog_sha256)
            .is_ok()
    }) {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }

    match intent.cohort {
        Some(cohort) => {
            // Cohort deletion is exact: every package/set carrying the anchor
            // is in the batch, and no target from another cohort is smuggled
            // into its negative retention root.
            let observed_packages = records
                .packages
                .iter()
                .filter_map(|(record_id, record)| (record.catalog == cohort).then_some(*record_id))
                .collect::<Vec<_>>();
            let observed_sets = records
                .catalog_sets
                .iter()
                .filter_map(|(set_id, set)| (set.catalog == cohort).then_some(*set_id))
                .collect::<Vec<_>>();
            if observed_packages != intent.package_record_ids
                || observed_sets != intent.catalog_set_record_ids
                || intent.package_record_ids.iter().any(|record_id| {
                    records
                        .packages
                        .get(record_id)
                        .is_none_or(|record| record.catalog != cohort)
                })
                || intent.catalog_set_record_ids.iter().any(|set_id| {
                    records
                        .catalog_sets
                        .get(set_id)
                        .is_none_or(|set| set.catalog != cohort)
                })
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }

            // Every row in a deleted set belongs to the same deleted package
            // cohort and preserves its exact package-key/runtime projection.
            // Unselected sets are otherwise dormant and are not checked by
            // the live catalog-set root validator.
            for set_id in &intent.catalog_set_record_ids {
                let set = records
                    .catalog_sets
                    .get(set_id)
                    .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
                let mut set_packages = Vec::with_capacity(set.packages.len());
                for row in &set.packages {
                    let package = records
                        .packages
                        .get(&row.package_record_id)
                        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
                    if !target_packages.contains(&row.package_record_id)
                        || package.catalog != set.catalog
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

            // Production planning includes the cohort's exact outer object
            // unless that object is the monotonic high-water root. Structural
            // recovery fixtures have no outer inventory and cannot assert this
            // production-only projection.
            if catalog_object_ids.is_some() {
                let expected_catalog_targets = if catalog_high_water == Some(cohort) {
                    Vec::new()
                } else {
                    vec![cohort.catalog_sha256]
                };
                if intent.catalog_object_ids != expected_catalog_targets {
                    return Err(ExtensionRepositoryError::RecoveryAmbiguous);
                }
            }

            // Bounded CAS sub-batches may omit some cohort-owned objects, but
            // each object they do name must be owned by at least one target
            // package and tree accounting must match its authenticated anchor.
            if intent.tree_index_ids.iter().any(|target| {
                !intent.package_record_ids.iter().any(|record_id| {
                    records
                        .packages
                        .get(record_id)
                        .is_some_and(|record| record.tree_index.index_sha256 == *target)
                })
            }) || intent.legal_artifact_ids.iter().any(|target| {
                !intent.package_record_ids.iter().any(|record_id| {
                    records
                        .packages
                        .get(record_id)
                        .is_some_and(|record| record.legal.sha256 == *target)
                })
            }) || intent.tree_objects.iter().any(|target| {
                !intent.package_record_ids.iter().any(|record_id| {
                    records.packages.get(record_id).is_some_and(|record| {
                        record.tree_index.tree_sha256 == target.tree_sha256
                            && Some(record.tree_index.total_entry_count)
                                == target.known_total_entry_count
                            && Some(record.tree_index.tree_bytes) == target.known_tree_bytes
                    })
                })
            }) {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }
        None => {
            // Residue batches contain no catalog-bound records. Every CAS
            // target must be unreachable from the entire package/set graph;
            // retained-record checks above therefore cover all references.
            if !target_packages.is_empty()
                || !target_sets.is_empty()
                || records.packages.values().any(|record| {
                    target_indexes.contains(&record.tree_index.index_sha256)
                        || target_legal.contains(&record.legal.sha256)
                        || target_trees.contains(&record.tree_index.tree_sha256)
                })
                || records.catalog_sets.values().any(|set| {
                    intent
                        .catalog_object_ids
                        .binary_search(&set.catalog.catalog_sha256)
                        .is_ok()
                })
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }
    }

    Ok(())
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
pub(super) fn validate_gc_intact_predelete_frontier(
    state: &MaterializationState,
    records: &RecordInventory,
    trees: &TreeInventory,
    catalog_object_ids: Option<&BTreeSet<Digest32>>,
    catalog_high_water: Option<CatalogAnchor>,
) -> Result<(), ExtensionRepositoryError> {
    validate_gc_intact_semantics(
        state,
        records,
        trees,
        catalog_object_ids,
        catalog_high_water,
    )
}
