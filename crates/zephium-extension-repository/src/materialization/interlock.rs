//! Pre-journal coordination between outer catalog authority and live roots.

use std::collections::{BTreeMap, BTreeSet};

use zephium_extension_authority::{
    BundledCatalogGenerationAnchor, BundledPackageAuthority, ProductBundledCatalogGenerationRole,
};

use super::records::{CatalogAnchor, CatalogSetRecord};
use super::runtime::MaterializationRuntime;
use super::state::{
    MaterializationBuildIntent, MaterializationGarbageCollectionIntent, MaterializationState,
};
use crate::state::{Digest32, StoredCatalogCheckpoint};
use crate::ExtensionRepositoryError;

pub(crate) fn validate_catalog_advance(
    runtime: &MaterializationRuntime,
    candidate: &StoredCatalogCheckpoint,
) -> Result<(), ExtensionRepositoryError> {
    let live_catalogs = collect_live_catalogs(runtime)?;
    if live_catalogs.is_empty() {
        return Ok(());
    }
    let authority = BundledPackageAuthority::product()
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    validate_live_catalogs(
        &live_catalogs,
        CatalogAnchor::from_high_water(candidate),
        |anchor| authority.recognize_generation(&anchor),
    )
}

fn collect_live_catalogs(
    runtime: &MaterializationRuntime,
) -> Result<BTreeSet<CatalogAnchor>, ExtensionRepositoryError> {
    collect_live_catalogs_from(
        &runtime._state,
        &runtime._build_intent,
        &runtime._gc_intent,
        &runtime._catalog_sets,
    )
}

fn collect_live_catalogs_from(
    state: &MaterializationState,
    runtime_build_intent: &Option<MaterializationBuildIntent>,
    runtime_gc_intent: &Option<MaterializationGarbageCollectionIntent>,
    catalog_sets: &BTreeMap<Digest32, CatalogSetRecord>,
) -> Result<BTreeSet<CatalogAnchor>, ExtensionRepositoryError> {
    state.validate()?;
    if &state.build_intent != runtime_build_intent || &state.gc_intent != runtime_gc_intent {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if state.gc_intent.is_some() {
        return Err(ExtensionRepositoryError::GarbageCollectionInProgress);
    }
    if state.build_intent.is_some() {
        return Err(ExtensionRepositoryError::CatalogAdvanceBlockedByBuild);
    }

    let mut catalogs = BTreeSet::new();
    for set_id in state.catalog_pin_ids() {
        let set = catalog_sets
            .get(&set_id)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        catalogs.insert(set.catalog);
    }
    for pin in &state.package_pins {
        let set = catalog_sets
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
    Ok(catalogs)
}

fn validate_live_catalogs(
    live_catalogs: &BTreeSet<CatalogAnchor>,
    candidate: CatalogAnchor,
    mut recognize: impl FnMut(
        BundledCatalogGenerationAnchor,
    ) -> Option<ProductBundledCatalogGenerationRole>,
) -> Result<(), ExtensionRepositoryError> {
    for catalog in live_catalogs {
        let anchor = catalog.generation_anchor()?;
        let role = recognize(anchor).ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        let compatible = match role {
            ProductBundledCatalogGenerationRole::Active => *catalog == candidate,
            ProductBundledCatalogGenerationRole::Rollback => {
                catalog.authority_id == candidate.authority_id
                    && catalog.revision < candidate.revision
            }
        };
        if !compatible {
            return Err(ExtensionRepositoryError::CatalogAdvanceBlockedByLiveGeneration);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materialization::records::tests::{catalog_set_fixture, package_record_fixture};
    use crate::materialization::state::{
        DurablePackagePin, HistoricalCatalogRole, StoredBrowsingContext,
        MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION, MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
    };
    use zephium_core::ids::{ExtensionInstallId, ProfileId};

    fn anchor(authority: u8, revision: u64, identity: u8) -> CatalogAnchor {
        let digest = |value| crate::state::Digest32::from_bytes([value; 32]);
        CatalogAnchor {
            authority_id: digest(authority),
            revision,
            catalog_length: 100,
            catalog_sha256: digest(identity),
            inventory_sha256: digest(identity.wrapping_add(1)),
        }
    }

    #[test]
    fn active_must_equal_candidate_and_rollback_must_strictly_precede_it() {
        let candidate = anchor(1, 3, 30);
        let rollback = anchor(1, 2, 20);
        let mut catalogs = BTreeSet::from([candidate, rollback]);
        assert_eq!(
            validate_live_catalogs(&catalogs, candidate, |generation| {
                if generation.revision().get() == 3 {
                    Some(ProductBundledCatalogGenerationRole::Active)
                } else {
                    Some(ProductBundledCatalogGenerationRole::Rollback)
                }
            }),
            Ok(())
        );

        catalogs = BTreeSet::from([anchor(1, 2, 20)]);
        assert_eq!(
            validate_live_catalogs(&catalogs, candidate, |_| {
                Some(ProductBundledCatalogGenerationRole::Active)
            }),
            Err(ExtensionRepositoryError::CatalogAdvanceBlockedByLiveGeneration)
        );
        catalogs = BTreeSet::from([anchor(2, 2, 20)]);
        assert_eq!(
            validate_live_catalogs(&catalogs, candidate, |_| {
                Some(ProductBundledCatalogGenerationRole::Rollback)
            }),
            Err(ExtensionRepositoryError::CatalogAdvanceBlockedByLiveGeneration)
        );
        catalogs = BTreeSet::from([candidate]);
        assert_eq!(
            validate_live_catalogs(&catalogs, candidate, |_| {
                Some(ProductBundledCatalogGenerationRole::Rollback)
            }),
            Err(ExtensionRepositoryError::CatalogAdvanceBlockedByLiveGeneration)
        );
    }

    #[test]
    fn an_unrecognized_live_generation_is_an_invariant_failure() {
        let candidate = anchor(1, 2, 20);
        assert_eq!(
            validate_live_catalogs(&BTreeSet::from([candidate]), candidate, |_| None),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn collector_uses_only_selection_slots_and_owner_pins() {
        let package = package_record_fixture(10);
        let package_id = package.record_id().unwrap();
        let mut sets = BTreeMap::new();
        let mut expected = BTreeSet::new();
        let set_ids = [
            crate::state::Digest32::from_bytes([101; 32]),
            crate::state::Digest32::from_bytes([102; 32]),
            crate::state::Digest32::from_bytes([103; 32]),
        ];
        for (index, set_id) in set_ids.into_iter().enumerate() {
            let mut set = catalog_set_fixture(&package);
            set.catalog = anchor(1, index as u64 + 1, index as u8 + 1);
            expected.insert(set.catalog);
            sets.insert(set_id, set);
        }
        let owner_set_id = crate::state::Digest32::from_bytes([104; 32]);
        sets.insert(owner_set_id, catalog_set_fixture(&package));
        expected.insert(package.catalog);
        let state = MaterializationState {
            generation: 1,
            completed_package_record_ids: vec![package_id],
            candidate_catalog_set_id: Some(set_ids[0]),
            current_catalog_set_id: Some(set_ids[1]),
            previous_catalog_set_id: Some(set_ids[2]),
            package_pins: vec![DurablePackagePin {
                profile_id: ProfileId::from(1),
                install_id: ExtensionInstallId::from(1),
                browsing_context: StoredBrowsingContext::Regular,
                catalog_set_record_id: owner_set_id,
                catalog_role: HistoricalCatalogRole::Rollback,
                package_record_id: package_id,
                native_incarnation: 1,
            }],
            ..MaterializationState::default()
        };
        assert_eq!(
            collect_live_catalogs_from(&state, &None, &None, &sets).unwrap(),
            expected
        );
    }

    #[test]
    fn completed_only_records_are_inert_but_missing_live_records_are_ambiguous() {
        let package = package_record_fixture(20);
        let package_id = package.record_id().unwrap();
        let mut state = MaterializationState {
            generation: 1,
            completed_package_record_ids: vec![package_id],
            ..MaterializationState::default()
        };
        assert!(
            collect_live_catalogs_from(&state, &None, &None, &BTreeMap::new())
                .unwrap()
                .is_empty()
        );

        state.current_catalog_set_id = Some(crate::state::Digest32::from_bytes([50; 32]));
        assert_eq!(
            collect_live_catalogs_from(&state, &None, &None, &BTreeMap::new()),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
        state.current_catalog_set_id = None;
        state.package_pins = vec![DurablePackagePin {
            profile_id: ProfileId::from(1),
            install_id: ExtensionInstallId::from(1),
            browsing_context: StoredBrowsingContext::Regular,
            catalog_set_record_id: crate::state::Digest32::from_bytes([51; 32]),
            catalog_role: HistoricalCatalogRole::Active,
            package_record_id: package_id,
            native_incarnation: 1,
        }];
        assert_eq!(
            collect_live_catalogs_from(&state, &None, &None, &BTreeMap::new()),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn collector_requires_one_exact_runtime_build_intent_projection() {
        let package = package_record_fixture(30);
        let intent = MaterializationBuildIntent {
            schema_version: MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION,
            generation: 1,
            package_record_id: package.record_id().unwrap(),
            package_record: package,
        };
        let state = MaterializationState {
            generation: 1,
            build_intent: Some(intent.clone()),
            ..MaterializationState::default()
        };
        assert_eq!(
            collect_live_catalogs_from(&state, &Some(intent), &None, &BTreeMap::new()),
            Err(ExtensionRepositoryError::CatalogAdvanceBlockedByBuild)
        );
        assert_eq!(
            collect_live_catalogs_from(&state, &None, &None, &BTreeMap::new()),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn collector_requires_one_exact_runtime_gc_intent_projection() {
        let runtime_only = MaterializationGarbageCollectionIntent {
            schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
            generation: 1,
            cohort: None,
            catalog_object_ids: vec![crate::state::Digest32::from_bytes([90; 32])],
            catalog_set_record_ids: Vec::new(),
            package_record_ids: Vec::new(),
            tree_index_ids: Vec::new(),
            legal_artifact_ids: Vec::new(),
            tree_objects: Vec::new(),
            retired_trees: Vec::new(),
        };
        let state = MaterializationState {
            generation: 1,
            ..MaterializationState::default()
        };
        assert_eq!(
            collect_live_catalogs_from(&state, &None, &Some(runtime_only), &BTreeMap::new(),),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }
}
