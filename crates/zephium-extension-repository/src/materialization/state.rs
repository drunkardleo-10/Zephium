//! Canonical materialization state, checkpoint, and transition journal.

use serde::{Deserialize, Serialize};
use zephium_core::extensions::{
    ExtensionCatalogGenerationRole, ExtensionGrantBrowsingContext,
    MAX_EXTENSION_INSTALLS_PER_PROFILE, MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_core::session::MAX_SESSION_PROFILES;
use zephium_extension_authority::MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS;
use zephium_extension_package::MAX_EXTENSION_PACKAGE_LINES;

use super::records::PackageRecord;
use crate::state::Digest32;
use crate::ExtensionRepositoryError;

pub(crate) const MATERIALIZATION_STATE_SCHEMA_VERSION: u32 = 3;
pub(crate) const MATERIALIZATION_JOURNAL_SCHEMA_VERSION: u32 = 3;
pub(crate) const MATERIALIZATION_CHECKPOINT_SCHEMA_VERSION: u32 = 1;
pub(crate) const MAX_MATERIALIZATION_STATE_BYTES: usize = 512 * 1024;
pub(crate) const MAX_MATERIALIZATION_CHECKPOINT_BYTES: usize = 16 * 1024;
pub(crate) const MAX_MATERIALIZATION_JOURNAL_BYTES: usize = 512 * 1024;
pub(crate) const MAX_CATALOG_SET_SLOTS: usize = MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS;
pub(crate) const MAX_DRAIN_CATALOG_SELECTIONS: usize = 1;
pub(crate) const MAX_RETAINED_CATALOG_SELECTIONS: usize =
    MAX_CATALOG_SET_SLOTS + MAX_DRAIN_CATALOG_SELECTIONS;
pub(crate) const MAX_COMPLETED_PACKAGE_RECORDS: usize =
    checked_mul(MAX_EXTENSION_PACKAGE_LINES, MAX_RETAINED_CATALOG_SELECTIONS);
pub(crate) const MAX_DURABLE_PACKAGE_PINS_PER_PROFILE: usize =
    checked_mul(MAX_EXTENSION_INSTALLS_PER_PROFILE, 2);
pub(crate) const MAX_DURABLE_PACKAGE_PINS: usize =
    checked_mul(MAX_SESSION_PROFILES, MAX_DURABLE_PACKAGE_PINS_PER_PROFILE);
pub(crate) const MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION: u32 = 1;

// Candidate/current/previous retain one selected backend per package for all
// product-recognized catalog generations. One additional catalog-sized
// selection is reserved for owner-pinned update drain or bounded cache state.
// All four backend profiles are deliberately not materialized for each package.
const _: () = assert!(MAX_CATALOG_SET_SLOTS == 3);
const _: () = assert!(MAX_DRAIN_CATALOG_SELECTIONS == 1);
const _: () = assert!(MAX_COMPLETED_PACKAGE_RECORDS <= 32);
const _: () = assert!(MAX_DURABLE_PACKAGE_PINS == MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES);

pub(crate) const MAX_DURABLE_GENERATION: u64 = i64::MAX as u64;

const fn checked_mul(left: usize, right: usize) -> usize {
    match left.checked_mul(right) {
        Some(value) => value,
        None => panic!("materialization package-pin bound overflow"),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MaterializationState {
    pub(crate) schema_version: u32,
    pub(crate) generation: u64,
    /// Structurally complete materializations retained across catalog upgrades.
    ///
    /// Membership is not product authority. Any future transition that places
    /// one of these records into a catalog slot or owner pin must freshly
    /// recognize its catalog generation against the then-current authority.
    pub(crate) completed_package_record_ids: Vec<Digest32>,
    /// Prepared atomic selection snapshot.
    ///
    /// Selection slots, unlike product generation anchors, may name the same
    /// catalog while macOS changes its per-package native/compatibility mix.
    pub(crate) candidate_catalog_set_id: Option<Digest32>,
    /// Current atomic selection snapshot.
    pub(crate) current_catalog_set_id: Option<Digest32>,
    /// Rollback selection snapshot; valid only while `current` is present.
    pub(crate) previous_catalog_set_id: Option<Digest32>,
    pub(crate) package_pins: Vec<DurablePackagePin>,
    pub(crate) build_intent: Option<MaterializationBuildIntent>,
}

impl Default for MaterializationState {
    fn default() -> Self {
        Self {
            schema_version: MATERIALIZATION_STATE_SCHEMA_VERSION,
            generation: 0,
            completed_package_record_ids: Vec::new(),
            candidate_catalog_set_id: None,
            current_catalog_set_id: None,
            previous_catalog_set_id: None,
            package_pins: Vec::new(),
            build_intent: None,
        }
    }
}

impl MaterializationState {
    pub(crate) fn validate(&self) -> Result<(), ExtensionRepositoryError> {
        if self.schema_version != MATERIALIZATION_STATE_SCHEMA_VERSION
            || self.generation > MAX_DURABLE_GENERATION
            || self.completed_package_record_ids.len() > MAX_COMPLETED_PACKAGE_RECORDS
            || self.package_pins.len() > MAX_DURABLE_PACKAGE_PINS
            || !strictly_sorted(&self.completed_package_record_ids)
            || !package_pin_shape_is_bounded(&self.package_pins)
            || !package_pin_catalog_sets_are_bounded(self)
            || !package_pin_incarnations_are_valid(&self.package_pins)
            || self
                .package_pins
                .windows(2)
                .any(|pair| pair[0].owner_key() >= pair[1].owner_key())
            || self.package_pins.iter().any(|pin| {
                self.completed_package_record_ids
                    .binary_search(&pin.package_record_id)
                    .is_err()
            })
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if let Some(intent) = &self.build_intent {
            if self.generation == MAX_DURABLE_GENERATION {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            intent.validate(self.generation)?;
            if self
                .completed_package_record_ids
                .binary_search(&intent.package_record_id)
                .is_ok()
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }

        let slots = [
            self.candidate_catalog_set_id,
            self.current_catalog_set_id,
            self.previous_catalog_set_id,
        ];
        for (index, slot) in slots.iter().enumerate() {
            if slot.is_some() && slots[..index].contains(slot) {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }
        if self.previous_catalog_set_id.is_some() && self.current_catalog_set_id.is_none() {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }

        if self.generation == 0
            && (!self.completed_package_record_ids.is_empty()
                || slots.iter().any(Option::is_some)
                || !self.package_pins.is_empty()
                || self.build_intent.is_some())
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }

    pub(crate) fn catalog_pin_ids(&self) -> Vec<Digest32> {
        let mut pins = [
            self.candidate_catalog_set_id,
            self.current_catalog_set_id,
            self.previous_catalog_set_id,
        ]
        .into_iter()
        .flatten()
        .chain(
            self.package_pins
                .iter()
                .map(|pin| pin.catalog_set_record_id),
        )
        .collect::<Vec<_>>();
        pins.sort_unstable();
        pins.dedup();
        pins
    }
}

/// Closed durable encoding of one exact extension browsing partition.
///
/// Private rows are accepted by recovery so a future explicitly authorized
/// private runtime can be reconciled. The current acquisition boundary remains
/// regular-only; this durable recovery capacity grants no private permission.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredBrowsingContext {
    Regular,
    Private,
}

impl From<ExtensionGrantBrowsingContext> for StoredBrowsingContext {
    fn from(value: ExtensionGrantBrowsingContext) -> Self {
        match value {
            ExtensionGrantBrowsingContext::Regular => Self::Regular,
            ExtensionGrantBrowsingContext::Private => Self::Private,
        }
    }
}

/// Role at the moment this exact catalog set acquired its durable owner.
///
/// This value is historical identity, not current product authority. An active
/// owner can legitimately become a rollback drain after a catalog advance.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HistoricalCatalogRole {
    Active,
    Rollback,
}

impl From<ExtensionCatalogGenerationRole> for HistoricalCatalogRole {
    fn from(value: ExtensionCatalogGenerationRole) -> Self {
        match value {
            ExtensionCatalogGenerationRole::Active => Self::Active,
            ExtensionCatalogGenerationRole::Rollback => Self::Rollback,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DurablePackagePin {
    pub(crate) profile_id: ProfileId,
    pub(crate) install_id: ExtensionInstallId,
    pub(crate) browsing_context: StoredBrowsingContext,
    /// Exact catalog-set record selected when this owner was acquired.
    pub(crate) catalog_set_record_id: Digest32,
    /// Historical role of `catalog_set_record_id` at acquisition.
    pub(crate) catalog_role: HistoricalCatalogRole,
    pub(crate) package_record_id: Digest32,
    /// Store-owned persistent native incarnation used for the cross-store ABA
    /// join. It is never derived from repository generation.
    pub(crate) native_incarnation: u64,
}

impl DurablePackagePin {
    pub(crate) const fn owner_key(&self) -> (ProfileId, ExtensionInstallId, StoredBrowsingContext) {
        (self.profile_id, self.install_id, self.browsing_context)
    }
}

/// Durable reconciliation identity for one interrupted materialization.
///
/// A future producer must admit the bounded source manifest once, retain that
/// exact byte buffer while committing this intent, and write those same bytes
/// into the staged tree. Reopening mutable source bytes after the intent or
/// treating this record as authority to resume after process loss would break
/// the identity guarantee. Recovery may freshly re-admit an already complete
/// sealed closure, or durably clear a partial build and restart admission.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MaterializationBuildIntent {
    pub(crate) schema_version: u32,
    pub(crate) generation: u64,
    pub(crate) package_record_id: Digest32,
    pub(crate) package_record: PackageRecord,
}

impl MaterializationBuildIntent {
    fn validate(&self, state_generation: u64) -> Result<(), ExtensionRepositoryError> {
        self.package_record.validate()?;
        if self.schema_version != MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION
            || self.generation == 0
            || self.generation != state_generation
            || self.package_record.record_id()? != self.package_record_id
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }
}

fn strictly_sorted(values: &[Digest32]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn package_pin_shape_is_bounded(pins: &[DurablePackagePin]) -> bool {
    let mut current_profile = None;
    let mut current_install = None;
    let mut profile_count = 0_usize;
    let mut installs_in_profile = 0_usize;
    let mut pins_in_profile = 0_usize;
    for pin in pins {
        if current_profile == Some(pin.profile_id) {
            pins_in_profile += 1;
            if current_install != Some(pin.install_id) {
                current_install = Some(pin.install_id);
                installs_in_profile += 1;
            }
        } else {
            current_profile = Some(pin.profile_id);
            current_install = Some(pin.install_id);
            profile_count += 1;
            installs_in_profile = 1;
            pins_in_profile = 1;
        }
        if profile_count > MAX_SESSION_PROFILES
            || installs_in_profile > MAX_EXTENSION_INSTALLS_PER_PROFILE
            || pins_in_profile > MAX_DURABLE_PACKAGE_PINS_PER_PROFILE
        {
            return false;
        }
    }
    true
}

fn package_pin_catalog_sets_are_bounded(state: &MaterializationState) -> bool {
    let mut slots = [
        state.candidate_catalog_set_id,
        state.current_catalog_set_id,
        state.previous_catalog_set_id,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    slots.sort_unstable();
    slots.dedup();
    let mut owner_sets = state
        .package_pins
        .iter()
        .map(|pin| pin.catalog_set_record_id)
        .collect::<Vec<_>>();
    owner_sets.sort_unstable();
    owner_sets.dedup();
    let drain_count = owner_sets
        .iter()
        .filter(|set_id| slots.binary_search(set_id).is_err())
        .count();
    slots.len() + drain_count <= MAX_RETAINED_CATALOG_SELECTIONS
        && drain_count <= MAX_DRAIN_CATALOG_SELECTIONS
}

fn package_pin_incarnations_are_valid(pins: &[DurablePackagePin]) -> bool {
    let mut native_incarnations = pins
        .iter()
        .map(|pin| pin.native_incarnation)
        .collect::<Vec<_>>();
    if native_incarnations
        .iter()
        .any(|incarnation| *incarnation == 0 || *incarnation > MAX_DURABLE_GENERATION)
    {
        return false;
    }
    native_incarnations.sort_unstable();
    native_incarnations
        .windows(2)
        .all(|pair| pair[0] != pair[1])
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MaterializationJournal {
    pub(crate) schema_version: u32,
    pub(crate) generation: u64,
    pub(crate) previous_state_sha256: Digest32,
    pub(crate) next_state_sha256: Digest32,
    pub(crate) next_state: MaterializationState,
}

impl MaterializationJournal {
    pub(crate) fn validate(&self) -> Result<(), ExtensionRepositoryError> {
        if self.schema_version != MATERIALIZATION_JOURNAL_SCHEMA_VERSION
            || self.generation != self.next_state.generation
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        self.next_state.validate()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MaterializationCheckpoint {
    pub(crate) schema_version: u32,
    pub(crate) generation: u64,
    pub(crate) state_sha256: Digest32,
}

impl MaterializationCheckpoint {
    pub(crate) const fn new(generation: u64, state_sha256: Digest32) -> Self {
        Self {
            schema_version: MATERIALIZATION_CHECKPOINT_SCHEMA_VERSION,
            generation,
            state_sha256,
        }
    }

    pub(crate) fn validate(self) -> Result<(), ExtensionRepositoryError> {
        if self.schema_version != MATERIALIZATION_CHECKPOINT_SCHEMA_VERSION
            || self.generation > MAX_DURABLE_GENERATION
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: u8) -> Digest32 {
        Digest32::from_bytes([byte; 32])
    }

    fn pin(
        profile: u128,
        install: u128,
        browsing_context: StoredBrowsingContext,
        package_record_id: Digest32,
        native_incarnation: u64,
    ) -> DurablePackagePin {
        DurablePackagePin {
            profile_id: ProfileId::from(profile),
            install_id: ExtensionInstallId::from(install),
            browsing_context,
            catalog_set_record_id: digest(10),
            catalog_role: HistoricalCatalogRole::Active,
            package_record_id,
            native_incarnation,
        }
    }

    #[test]
    fn state_requires_canonical_unique_ids_and_distinct_slots() {
        let mut state = MaterializationState {
            generation: 1,
            completed_package_record_ids: vec![digest(1), digest(2)],
            candidate_catalog_set_id: Some(digest(3)),
            ..MaterializationState::default()
        };
        assert_eq!(state.validate(), Ok(()));

        state.completed_package_record_ids.swap(0, 1);
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
        state.completed_package_record_ids.sort_unstable();
        state.current_catalog_set_id = state.candidate_catalog_set_id;
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        state.current_catalog_set_id = None;
        state.candidate_catalog_set_id = Some(digest(3));
        state.previous_catalog_set_id = Some(digest(4));
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn zero_generation_is_exactly_empty() {
        let mut state = MaterializationState::default();
        state
            .package_pins
            .push(pin(1, 1, StoredBrowsingContext::Regular, digest(1), 1));
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn terminal_generation_cannot_strand_a_build_intent() {
        let package_record = super::super::records::tests::package_record_fixture(31);
        let package_record_id = package_record.record_id().unwrap();
        let state = MaterializationState {
            generation: MAX_DURABLE_GENERATION,
            build_intent: Some(MaterializationBuildIntent {
                schema_version: MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION,
                generation: MAX_DURABLE_GENERATION,
                package_record_id,
                package_record,
            }),
            ..MaterializationState::default()
        };
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn pin_projection_is_sorted_and_duplicate_free() {
        let mut owner_pin = pin(1, 1, StoredBrowsingContext::Regular, digest(9), 1);
        owner_pin.catalog_set_record_id = digest(4);
        let state = MaterializationState {
            generation: 1,
            candidate_catalog_set_id: Some(digest(3)),
            current_catalog_set_id: Some(digest(2)),
            previous_catalog_set_id: Some(digest(1)),
            package_pins: vec![owner_pin],
            ..MaterializationState::default()
        };
        assert_eq!(
            state.catalog_pin_ids(),
            vec![digest(1), digest(2), digest(3), digest(4)]
        );
    }

    #[test]
    fn completed_records_and_owner_pins_have_independent_hard_bounds() {
        let mut completed = MaterializationState {
            generation: 1,
            completed_package_record_ids: (0..=MAX_COMPLETED_PACKAGE_RECORDS)
                .map(|index| {
                    let mut bytes = [0_u8; 32];
                    bytes[..8].copy_from_slice(&(index as u64).to_be_bytes());
                    Digest32::from_bytes(bytes)
                })
                .collect(),
            ..MaterializationState::default()
        };
        assert_eq!(
            completed.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        completed.completed_package_record_ids = vec![digest(9)];
        completed.generation = MAX_DURABLE_PACKAGE_PINS as u64 + 1;
        completed.package_pins = (0..=MAX_DURABLE_PACKAGE_PINS)
            .map(|index| {
                let per_profile = MAX_DURABLE_PACKAGE_PINS_PER_PROFILE;
                pin(
                    (index / per_profile + 1) as u128,
                    (index % per_profile / 2 + 1) as u128,
                    if index % 2 == 0 {
                        StoredBrowsingContext::Regular
                    } else {
                        StoredBrowsingContext::Private
                    },
                    digest(9),
                    index as u64 + 1,
                )
            })
            .collect();
        assert_eq!(
            completed.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn package_pin_owners_are_unique_but_may_share_one_record() {
        let mut state = MaterializationState {
            generation: 2,
            completed_package_record_ids: vec![digest(7)],
            package_pins: vec![
                pin(1, 1, StoredBrowsingContext::Regular, digest(7), 1),
                pin(1, 2, StoredBrowsingContext::Regular, digest(7), 2),
            ],
            ..MaterializationState::default()
        };
        assert_eq!(state.validate(), Ok(()));
        state.package_pins[1].install_id = ExtensionInstallId::from(1);
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn regular_and_private_contexts_are_distinct_durable_owner_keys() {
        let mut state = MaterializationState {
            generation: 1,
            completed_package_record_ids: vec![digest(7)],
            package_pins: vec![
                pin(1, 1, StoredBrowsingContext::Regular, digest(7), 1),
                pin(1, 1, StoredBrowsingContext::Private, digest(7), 2),
            ],
            ..MaterializationState::default()
        };
        assert_eq!(state.validate(), Ok(()));
        state.package_pins[1].browsing_context = StoredBrowsingContext::Regular;
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn owner_pins_enforce_profile_and_per_profile_install_caps() {
        let base = MaterializationState {
            generation: MAX_DURABLE_PACKAGE_PINS as u64 + 1,
            completed_package_record_ids: vec![digest(7)],
            ..MaterializationState::default()
        };
        let mut too_many_installs = base.clone();
        too_many_installs.package_pins = (0..=MAX_EXTENSION_INSTALLS_PER_PROFILE)
            .map(|index| {
                pin(
                    1,
                    (index + 1) as u128,
                    StoredBrowsingContext::Regular,
                    digest(7),
                    index as u64 + 1,
                )
            })
            .collect();
        assert_eq!(
            too_many_installs.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        let mut too_many_profiles = base;
        too_many_profiles.package_pins = (0..=MAX_SESSION_PROFILES)
            .map(|index| {
                pin(
                    (index + 1) as u128,
                    1,
                    StoredBrowsingContext::Regular,
                    digest(7),
                    index as u64 + 1,
                )
            })
            .collect();
        assert_eq!(
            too_many_profiles.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn maximum_owner_pin_state_fits_the_bounded_duplicate_safe_codec() {
        let state = MaterializationState {
            generation: MAX_DURABLE_GENERATION,
            completed_package_record_ids: vec![digest(9)],
            package_pins: (0..MAX_DURABLE_PACKAGE_PINS)
                .map(|index| {
                    let profile_index = index / MAX_DURABLE_PACKAGE_PINS_PER_PROFILE;
                    let within_profile = index % MAX_DURABLE_PACKAGE_PINS_PER_PROFILE;
                    pin(
                        u128::MAX - (MAX_SESSION_PROFILES - profile_index - 1) as u128,
                        u128::MAX
                            - (MAX_EXTENSION_INSTALLS_PER_PROFILE - (within_profile / 2) - 1)
                                as u128,
                        if within_profile.is_multiple_of(2) {
                            StoredBrowsingContext::Regular
                        } else {
                            StoredBrowsingContext::Private
                        },
                        digest(9),
                        MAX_DURABLE_GENERATION - (MAX_DURABLE_PACKAGE_PINS - index - 1) as u64,
                    )
                })
                .collect(),
            ..MaterializationState::default()
        };
        state.validate().unwrap();
        let bytes = crate::codec::encode(&state, MAX_MATERIALIZATION_STATE_BYTES).unwrap();
        assert!(bytes.len() <= MAX_MATERIALIZATION_STATE_BYTES);
        assert_eq!(
            crate::codec::decode_materialization::<MaterializationState>(
                &bytes,
                MAX_MATERIALIZATION_STATE_BYTES,
            )
            .unwrap(),
            state
        );

        let journal = MaterializationJournal {
            schema_version: MATERIALIZATION_JOURNAL_SCHEMA_VERSION,
            generation: state.generation,
            previous_state_sha256: digest(11),
            next_state_sha256: crate::codec::digest(&bytes),
            next_state: state,
        };
        journal.validate().unwrap();
        let journal_bytes =
            crate::codec::encode(&journal, MAX_MATERIALIZATION_JOURNAL_BYTES).unwrap();
        assert!(journal_bytes.len() <= MAX_MATERIALIZATION_JOURNAL_BYTES);
        assert_eq!(
            crate::codec::decode_materialization::<MaterializationJournal>(
                &journal_bytes,
                MAX_MATERIALIZATION_JOURNAL_BYTES,
            )
            .unwrap(),
            journal
        );
    }

    #[test]
    fn owner_pin_sets_and_store_incarnations_are_exactly_bounded() {
        let mut state = MaterializationState {
            generation: 1,
            completed_package_record_ids: vec![digest(7)],
            candidate_catalog_set_id: Some(digest(1)),
            current_catalog_set_id: Some(digest(2)),
            previous_catalog_set_id: Some(digest(3)),
            package_pins: vec![pin(1, 1, StoredBrowsingContext::Regular, digest(7), 1)],
            ..MaterializationState::default()
        };
        state.package_pins[0].catalog_set_record_id = digest(4);
        assert_eq!(state.validate(), Ok(()));

        let mut second_drain = pin(1, 2, StoredBrowsingContext::Regular, digest(7), 2);
        second_drain.catalog_set_record_id = digest(5);
        state.package_pins.push(second_drain);
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        state.package_pins[1].catalog_set_record_id = digest(4);
        assert_eq!(state.validate(), Ok(()));
        state.package_pins[1].native_incarnation = 1;
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn pre_binding_schema_and_partial_pin_rows_fail_closed() {
        assert!(serde_json::from_str::<HistoricalCatalogRole>(r#""future""#).is_err());
        assert!(serde_json::from_str::<StoredBrowsingContext>(r#""guest""#).is_err());

        let old_empty = br#"{"schema_version":2,"generation":0,"completed_package_record_ids":[],"candidate_catalog_set_id":null,"current_catalog_set_id":null,"previous_catalog_set_id":null,"package_pins":[],"build_intent":null}"#;
        let decoded = crate::codec::decode_materialization::<MaterializationState>(
            old_empty,
            MAX_MATERIALIZATION_STATE_BYTES,
        )
        .unwrap();
        assert_eq!(
            decoded.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        let old_live_pin = br#"{"schema_version":2,"generation":1,"completed_package_record_ids":["0909090909090909090909090909090909090909090909090909090909090909"],"candidate_catalog_set_id":null,"current_catalog_set_id":null,"previous_catalog_set_id":null,"package_pins":[{"profile_id":"00000000000000000000000001","install_id":"00000000000000000000000001","package_record_id":"0909090909090909090909090909090909090909090909090909090909090909","incarnation":1}],"build_intent":null}"#;
        assert!(
            crate::codec::decode_materialization::<MaterializationState>(
                old_live_pin,
                MAX_MATERIALIZATION_STATE_BYTES,
            )
            .is_err()
        );
    }
}
