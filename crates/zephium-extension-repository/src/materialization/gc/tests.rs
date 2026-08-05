use super::*;
use crate::materialization::records::tests::{catalog_set_fixture, package_record_fixture};
use crate::materialization::state::{
    DurablePackagePin, HistoricalCatalogRole, StoredBrowsingContext, MAX_DURABLE_PACKAGE_PINS,
    MAX_DURABLE_PACKAGE_PINS_PER_PROFILE,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};

#[derive(Default)]
struct Fixture {
    catalogs: BTreeSet<Digest32>,
    sets: BTreeMap<Digest32, CatalogSetRecord>,
    packages: BTreeMap<Digest32, PackageRecord>,
    indexes: BTreeSet<Digest32>,
    legal: BTreeSet<Digest32>,
    trees: BTreeSet<Digest32>,
    retired: BTreeSet<(Digest32, u64)>,
}

impl Fixture {
    fn insert_package(&mut self, package: PackageRecord) -> Digest32 {
        let id = package.record_id().unwrap();
        self.catalogs.insert(package.catalog.catalog_sha256);
        self.indexes.insert(package.tree_index.index_sha256);
        self.legal.insert(package.legal.sha256);
        self.trees.insert(package.tree_index.tree_sha256);
        self.packages.insert(id, package);
        id
    }

    fn insert_set(&mut self, set: CatalogSetRecord) -> Digest32 {
        let id = set.record_id().unwrap();
        self.catalogs.insert(set.catalog.catalog_sha256);
        self.sets.insert(id, set);
        id
    }

    fn view(&self, high_water: Option<CatalogAnchor>) -> GarbageCollectionInventory<'_> {
        GarbageCollectionInventory {
            catalog_high_water: high_water,
            catalog_object_ids: &self.catalogs,
            catalog_sets: &self.sets,
            package_records: &self.packages,
            tree_index_ids: &self.indexes,
            legal_artifact_ids: &self.legal,
            tree_object_ids: &self.trees,
            retired_tree_ids: &self.retired,
        }
    }
}

fn state_for(fixture: &Fixture) -> MaterializationState {
    MaterializationState {
        generation: 1,
        completed_package_record_ids: fixture.packages.keys().copied().collect(),
        ..MaterializationState::default()
    }
}

#[test]
fn no_garbage_is_a_zero_plan_even_at_terminal_generation() {
    let mut fixture = Fixture::default();
    let package = package_record_fixture(1);
    let high_water = package.catalog;
    fixture.insert_package(package.clone());
    let current_set_id = fixture.insert_set(catalog_set_fixture(&package));
    let mut state = state_for(&fixture);
    state.generation = super::super::state::MAX_DURABLE_GENERATION;
    state.current_catalog_set_id = Some(current_set_id);
    assert_eq!(
        plan_inventory(&state, &None, &None, fixture.view(Some(high_water))).unwrap(),
        None
    );
}

#[test]
fn garbage_plan_reserves_one_final_settlement_generation() {
    let mut fixture = Fixture::default();
    fixture.catalogs.insert(Digest32::from_bytes([250; 32]));
    let mut state = MaterializationState {
        generation: super::super::state::MAX_DURABLE_GENERATION - 1,
        ..MaterializationState::default()
    };
    assert_eq!(
        plan_inventory(&state, &None, &None, fixture.view(None)),
        Err(ExtensionRepositoryError::GenerationExhausted)
    );

    state.generation = super::super::state::MAX_DURABLE_GENERATION - 2;
    let plan = plan_inventory(&state, &None, &None, fixture.view(None))
        .unwrap()
        .unwrap();
    assert_eq!(
        plan.next_state.generation,
        super::super::state::MAX_DURABLE_GENERATION - 1
    );
    assert_eq!(plan.next_state.validate(), Ok(()));
}

#[test]
fn completed_only_dead_cohort_is_disposable_and_order_is_deterministic() {
    let mut fixture = Fixture::default();
    let high_water_package = package_record_fixture(40);
    let high_water = high_water_package.catalog;
    let high_water_id = fixture.insert_package(high_water_package);
    let first = package_record_fixture(1);
    let first_anchor = first.catalog;
    let second = package_record_fixture(20);
    let second_anchor = second.catalog;
    let first_id = fixture.insert_package(first);
    let second_id = fixture.insert_package(second);
    let state = state_for(&fixture);

    let plan = plan_inventory(&state, &None, &None, fixture.view(Some(high_water)))
        .unwrap()
        .unwrap();
    let (expected_anchor, expected_id) = [
        (high_water, high_water_id),
        (first_anchor, first_id),
        (second_anchor, second_id),
    ]
    .into_iter()
    .min_by_key(|(anchor, _)| *anchor)
    .unwrap();
    let intent = plan.next_state.gc_intent.as_ref().unwrap();
    assert_eq!(intent.cohort, Some(expected_anchor));
    assert_eq!(intent.package_record_ids, vec![expected_id]);
    assert!(plan.more_garbage);
    assert!(intent.package_record_ids.iter().all(|record_id| plan
        .next_state
        .completed_package_record_ids
        .binary_search(record_id)
        .is_err()));
    assert_eq!(
        plan_inventory(&state, &None, &None, fixture.view(Some(high_water)))
            .unwrap()
            .unwrap(),
        plan
    );
}

#[test]
fn shared_content_addresses_survive_until_every_record_is_collectible() {
    let mut fixture = Fixture::default();
    let mut first = package_record_fixture(1);
    let mut second = package_record_fixture(20);
    second.tree_index = first.tree_index;
    second.package.tree_sha256 = first.package.tree_sha256;
    second.legal = first.legal.clone();
    first.catalog.revision = 1;
    second.catalog.revision = 2;
    let first_anchor = first.catalog;
    fixture.insert_package(first);
    fixture.insert_package(second);
    let state = state_for(&fixture);
    let plan = plan_inventory(&state, &None, &None, fixture.view(None))
        .unwrap()
        .unwrap();
    let intent = plan.next_state.gc_intent.as_ref().unwrap();
    assert_eq!(intent.cohort, Some(first_anchor));
    assert!(intent.tree_objects.is_empty());
    assert!(intent.tree_index_ids.is_empty());
    assert!(intent.legal_artifact_ids.is_empty());
}

#[test]
fn residue_is_cleanup_only_bounded_and_precedes_catalog_cohorts() {
    let mut fixture = Fixture::default();
    let package = package_record_fixture(1);
    fixture.insert_package(package);
    let orphan_catalog = Digest32::from_bytes([250; 32]);
    let orphan_tree = Digest32::from_bytes([251; 32]);
    fixture.catalogs.insert(orphan_catalog);
    fixture.trees.insert(orphan_tree);
    for generation in 1..=MAX_GC_TREE_JOBS as u64 {
        fixture.retired.insert((
            Digest32::from_bytes([200 + generation as u8; 32]),
            generation,
        ));
    }
    let mut state = state_for(&fixture);
    state.generation = MAX_GC_TREE_JOBS as u64 + 1;
    let plan = plan_inventory(&state, &None, &None, fixture.view(None))
        .unwrap()
        .unwrap();
    let intent = plan.next_state.gc_intent.as_ref().unwrap();
    assert_eq!(intent.cohort, None);
    assert_eq!(intent.catalog_object_ids, vec![orphan_catalog]);
    assert_eq!(intent.retired_trees.len(), MAX_GC_TREE_JOBS);
    assert!(intent.tree_objects.is_empty());
    assert!(plan.more_garbage);
}

#[test]
fn high_water_roots_only_catalog_bytes_while_owner_drain_roots_its_whole_set() {
    let mut fixture = Fixture::default();
    let high = package_record_fixture(1);
    let high_anchor = high.catalog;
    let high_id = fixture.insert_package(high);
    let drain = package_record_fixture(20);
    let drain_id = fixture.insert_package(drain.clone());
    let drain_set_id = fixture.insert_set(catalog_set_fixture(&drain));
    let mut state = state_for(&fixture);
    state.current_catalog_set_id = None;
    state.package_pins.push(DurablePackagePin {
        profile_id: ProfileId::from(1),
        install_id: ExtensionInstallId::from(1),
        browsing_context: StoredBrowsingContext::Regular,
        catalog_set_record_id: drain_set_id,
        catalog_role: HistoricalCatalogRole::Rollback,
        package_record_id: drain_id,
        native_incarnation: 1,
    });
    assert!(state.validate().is_ok());
    let plan = plan_inventory(&state, &None, &None, fixture.view(Some(high_anchor)))
        .unwrap()
        .unwrap();
    let intent = plan.next_state.gc_intent.as_ref().unwrap();
    assert_eq!(intent.cohort, Some(high_anchor));
    assert_eq!(intent.package_record_ids, vec![high_id]);
    assert!(intent.catalog_object_ids.is_empty());
    assert!(plan
        .next_state
        .completed_package_record_ids
        .contains(&drain_id));
    assert!(plan
        .next_state
        .package_pins
        .iter()
        .all(|pin| pin.catalog_set_record_id == drain_set_id));
}

#[test]
fn high_water_digest_alias_with_different_catalog_set_anchor_fails_closed() {
    let mut fixture = Fixture::default();
    let mut equivocal_package = package_record_fixture(1);
    let high_water = equivocal_package.catalog;
    equivocal_package.catalog.revision += 1;
    equivocal_package.catalog.inventory_sha256 = Digest32::from_bytes([250; 32]);
    assert_eq!(
        equivocal_package.catalog.catalog_sha256,
        high_water.catalog_sha256
    );
    fixture.insert_package(equivocal_package.clone());
    fixture.insert_set(catalog_set_fixture(&equivocal_package));
    let state = state_for(&fixture);
    assert_eq!(
        plan_inventory(&state, &None, &None, fixture.view(Some(high_water))),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
}

#[test]
fn unrooted_catalog_set_still_obeys_the_catalog_tree_budget() {
    use crate::materialization::records::{
        CatalogSetPackageRow, CatalogSetRecord, CATALOG_SET_RECORD_SCHEMA_VERSION,
    };

    let mut fixture = Fixture::default();
    let baseline = package_record_fixture(1);
    let mut rows = Vec::new();
    for index in 0..3_u8 {
        let mut package = baseline.clone();
        package.package.package_key = Digest32::from_bytes([60 + index; 32]);
        package.package.revision = u64::from(index) + 1;
        package.package.package_row_sha256 = Digest32::from_bytes([70 + index; 32]);
        package.package.tree_sha256 = Digest32::from_bytes([80 + index; 32]);
        package.tree_index.tree_sha256 = package.package.tree_sha256;
        package.tree_index.index_sha256 = Digest32::from_bytes([90 + index; 32]);
        package.tree_index.tree_bytes = if index < 2 {
            zephium_extension_package::MAX_EXTENSION_TREE_BYTES
        } else {
            1
        };
        package.legal.sha256 = Digest32::from_bytes([100 + index; 32]);
        let package_key = package.package.package_key;
        let runtime_target = package.manifest.runtime_target;
        let package_record_id = fixture.insert_package(package);
        rows.push(CatalogSetPackageRow {
            package_key,
            runtime_target,
            package_record_id,
        });
    }
    fixture.insert_set(CatalogSetRecord {
        schema_version: CATALOG_SET_RECORD_SCHEMA_VERSION,
        catalog: baseline.catalog,
        packages: rows,
    });
    let state = state_for(&fixture);
    assert_eq!(
        plan_inventory(&state, &None, &None, fixture.view(None)),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
}

#[test]
fn planning_rejects_an_over_budget_preplan_completed_union() {
    let mut fixture = Fixture::default();
    for index in 0..9_u8 {
        let mut package = package_record_fixture(index * 20 + 1);
        package.tree_index.tree_bytes = zephium_extension_package::MAX_EXTENSION_TREE_BYTES;
        fixture.insert_package(package);
    }
    let state = state_for(&fixture);
    assert_eq!(state.completed_package_record_ids.len(), 9);
    assert_eq!(
        plan_inventory(&state, &None, &None, fixture.view(None)),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
}

#[test]
fn maximum_pin_and_four_set_inventory_plans_deterministically_with_bounded_memory() {
    use crate::materialization::records::{
        CatalogSetPackageRow, CatalogSetRecord, CATALOG_SET_RECORD_SCHEMA_VERSION,
    };

    let mut fixture = Fixture::default();
    let mut set_ids = Vec::new();
    for set_index in 0..4_u8 {
        let mut rows = Vec::new();
        let mut catalog = None;
        for row_index in 0..8_u8 {
            let global = set_index * 8 + row_index;
            let mut package = package_record_fixture(1);
            package.catalog.revision = u64::from(set_index) + 1;
            package.catalog.catalog_sha256 = Digest32::from_bytes([40 + set_index; 32]);
            package.catalog.inventory_sha256 = Digest32::from_bytes([50 + set_index; 32]);
            package.package.authority_id = package.catalog.authority_id;
            package.package.package_key = Digest32::from_bytes([60 + global; 32]);
            package.package.revision = u64::from(global) + 1;
            package.package.package_row_sha256 = Digest32::from_bytes([100 + global; 32]);
            package.package.tree_sha256 = Digest32::from_bytes([140 + global; 32]);
            package.tree_index.tree_sha256 = package.package.tree_sha256;
            package.tree_index.index_sha256 = Digest32::from_bytes([180 + global; 32]);
            package.legal.sha256 = Digest32::from_bytes([220 + global; 32]);
            catalog = Some(package.catalog);
            let package_key = package.package.package_key;
            let runtime_target = package.manifest.runtime_target;
            let package_record_id = fixture.insert_package(package);
            rows.push(CatalogSetPackageRow {
                package_key,
                runtime_target,
                package_record_id,
            });
        }
        let set = CatalogSetRecord {
            schema_version: CATALOG_SET_RECORD_SCHEMA_VERSION,
            catalog: catalog.unwrap(),
            packages: rows,
        };
        set_ids.push(fixture.insert_set(set));
    }
    let mut state = state_for(&fixture);
    state.generation = MAX_DURABLE_PACKAGE_PINS as u64 + 1;
    state.candidate_catalog_set_id = Some(set_ids[0]);
    state.current_catalog_set_id = Some(set_ids[1]);
    state.previous_catalog_set_id = Some(set_ids[2]);
    state.package_pins = (0..MAX_DURABLE_PACKAGE_PINS)
        .map(|index| {
            let set_index = index % set_ids.len();
            let set = fixture.sets.get(&set_ids[set_index]).unwrap();
            let row = &set.packages[(index / set_ids.len()) % set.packages.len()];
            DurablePackagePin {
                profile_id: ProfileId::from(
                    (index / MAX_DURABLE_PACKAGE_PINS_PER_PROFILE + 1) as u128,
                ),
                install_id: ExtensionInstallId::from(
                    (index % MAX_DURABLE_PACKAGE_PINS_PER_PROFILE / 2 + 1) as u128,
                ),
                browsing_context: if index.is_multiple_of(2) {
                    StoredBrowsingContext::Regular
                } else {
                    StoredBrowsingContext::Private
                },
                catalog_set_record_id: set_ids[set_index],
                catalog_role: HistoricalCatalogRole::Active,
                package_record_id: row.package_record_id,
                native_incarnation: index as u64 + 1,
            }
        })
        .collect();
    assert_eq!(state.completed_package_record_ids.len(), 32);
    assert_eq!(state.package_pins.len(), 1_024);
    state.validate().unwrap();

    let orphan_catalog = Digest32::from_bytes([255; 32]);
    fixture.catalogs.insert(orphan_catalog);
    let high_water = fixture.sets.get(&set_ids[0]).unwrap().catalog;
    let plan = plan_inventory(&state, &None, &None, fixture.view(Some(high_water)))
        .unwrap()
        .unwrap();
    let repeated = plan_inventory(&state, &None, &None, fixture.view(Some(high_water)))
        .unwrap()
        .unwrap();
    assert_eq!(plan, repeated);
    assert!(!plan.more_garbage);
    assert_eq!(
        plan.next_state
            .gc_intent
            .as_ref()
            .unwrap()
            .catalog_object_ids,
        vec![orphan_catalog]
    );
    assert_eq!(plan.next_state.completed_package_record_ids.len(), 32);
    assert_eq!(plan.next_state.package_pins.len(), 1_024);
    assert!(
        plan.next_state.gc_intent.as_ref().unwrap().retained_bytes()
            <= super::super::state::MAX_GC_INTENT_RETAINED_BYTES
    );
    assert_eq!(plan.next_state.validate(), Ok(()));
}

#[test]
fn build_and_gc_frontiers_are_mutually_exclusive() {
    let fixture = Fixture::default();
    let mut state = MaterializationState {
        generation: 1,
        ..MaterializationState::default()
    };
    let package = package_record_fixture(90);
    let phantom_build = MaterializationBuildIntent {
        schema_version: super::super::state::MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION,
        generation: 1,
        package_record_id: package.record_id().unwrap(),
        package_record: package,
    };
    assert_eq!(
        plan_inventory(&state, &Some(phantom_build), &None, fixture.view(None),),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
    let empty_intent = MaterializationGarbageCollectionIntent {
        schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
        generation: 1,
        cohort: None,
        catalog_object_ids: vec![Digest32::from_bytes([1; 32])],
        catalog_set_record_ids: Vec::new(),
        package_record_ids: Vec::new(),
        tree_index_ids: Vec::new(),
        legal_artifact_ids: Vec::new(),
        tree_objects: Vec::new(),
        retired_trees: Vec::new(),
    };
    state.gc_intent = Some(empty_intent.clone());
    assert_eq!(
        plan_inventory(&state, &None, &Some(empty_intent), fixture.view(None)),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    );
}
