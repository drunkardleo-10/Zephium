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
use zephium_extension_package::{
    MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES, MAX_EXTENSION_TREE_BYTES, MAX_EXTENSION_TREE_ENTRIES,
};

use super::records::{CatalogAnchor, PackageRecord};
use crate::state::Digest32;
use crate::ExtensionRepositoryError;

// Schema v3 never shipped. It is deliberately rejected instead of migrated:
// v4 makes an in-progress collector part of the durable retention graph, and
// silently defaulting that field while opening pre-v4 bytes would erase the
// distinction between "never planned" and "plan metadata was lost".
pub(crate) const MATERIALIZATION_STATE_SCHEMA_VERSION: u32 = 4;
pub(crate) const MATERIALIZATION_JOURNAL_SCHEMA_VERSION: u32 = 4;
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
pub(crate) const MATERIALIZATION_GC_INTENT_SCHEMA_VERSION: u32 = 1;
pub(crate) const MAX_GC_CATALOG_OBJECT_TARGETS: usize = MAX_EXTENSION_PACKAGE_LINES;
pub(crate) const MAX_GC_CATALOG_SET_TARGETS: usize = MAX_EXTENSION_PACKAGE_LINES;
pub(crate) const MAX_GC_PACKAGE_RECORD_TARGETS: usize = MAX_COMPLETED_PACKAGE_RECORDS;
pub(crate) const MAX_GC_DATA_OBJECT_TARGETS: usize = MAX_EXTENSION_PACKAGE_LINES;
pub(crate) const MAX_GC_TREE_JOBS: usize = MAX_EXTENSION_PACKAGE_LINES;
pub(crate) const MAX_GC_TREE_ENTRIES: usize =
    checked_mul(MAX_EXTENSION_TREE_ENTRIES, MAX_GC_TREE_JOBS);
pub(crate) const MAX_GC_KNOWN_TREE_BYTES: u64 = MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES;
pub(crate) const MAX_GC_INTENT_RETAINED_BYTES: usize = 16 * 1024;

// Candidate/current/previous retain one selected backend per package for all
// product-recognized catalog generations. One additional catalog-sized
// selection is reserved for owner-pinned update drain or bounded cache state.
// All four backend profiles are deliberately not materialized for each package.
const _: () = assert!(MAX_CATALOG_SET_SLOTS == 3);
const _: () = assert!(MAX_DRAIN_CATALOG_SELECTIONS == 1);
const _: () = assert!(MAX_COMPLETED_PACKAGE_RECORDS <= 32);
const _: () = assert!(MAX_DURABLE_PACKAGE_PINS == MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES);
const _: () = assert!(MAX_GC_PACKAGE_RECORD_TARGETS <= 32);
const _: () = assert!(MAX_GC_CATALOG_OBJECT_TARGETS <= 8);
const _: () = assert!(MAX_GC_CATALOG_SET_TARGETS <= 8);
const _: () = assert!(MAX_GC_DATA_OBJECT_TARGETS <= 8);
const _: () = assert!(MAX_GC_TREE_JOBS <= 8);
const _: () = assert!(MAX_GC_TREE_ENTRIES <= 32_768);
const _: () = assert!(MAX_GC_KNOWN_TREE_BYTES <= 256 * 1024 * 1024);

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
    pub(crate) gc_intent: Option<MaterializationGarbageCollectionIntent>,
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
            gc_intent: None,
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
        if self.build_intent.is_some() && self.gc_intent.is_some() {
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
        if let Some(intent) = &self.gc_intent {
            if self.generation == MAX_DURABLE_GENERATION {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            intent.validate(self.generation)?;
            if self
                .completed_package_record_ids
                .len()
                .checked_add(intent.package_record_ids.len())
                .is_none_or(|preplan_count| preplan_count > MAX_COMPLETED_PACKAGE_RECORDS)
                || intent.package_record_ids.iter().any(|record_id| {
                    self.completed_package_record_ids
                        .binary_search(record_id)
                        .is_ok()
                })
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
                || self.build_intent.is_some()
                || self.gc_intent.is_some())
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

/// Exact durable identity of one bounded garbage-collection batch.
///
/// A cohort is present only when deleting catalog-bound set/package metadata.
/// `None` is reserved for already-unreachable content-addressed residue and
/// retired tree roots; it must never weaken catalog/package identity checks.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MaterializationGarbageCollectionIntent {
    pub(crate) schema_version: u32,
    pub(crate) generation: u64,
    pub(crate) cohort: Option<CatalogAnchor>,
    pub(crate) catalog_object_ids: Vec<Digest32>,
    pub(crate) catalog_set_record_ids: Vec<Digest32>,
    pub(crate) package_record_ids: Vec<Digest32>,
    pub(crate) tree_index_ids: Vec<Digest32>,
    pub(crate) legal_artifact_ids: Vec<Digest32>,
    pub(crate) tree_objects: Vec<GarbageCollectionTreeObject>,
    pub(crate) retired_trees: Vec<GarbageCollectionRetiredTree>,
}

impl MaterializationGarbageCollectionIntent {
    pub(crate) fn validate(&self, state_generation: u64) -> Result<(), ExtensionRepositoryError> {
        if self.schema_version != MATERIALIZATION_GC_INTENT_SCHEMA_VERSION
            || self.generation == 0
            || self.generation != state_generation
            || self.catalog_object_ids.len() > MAX_GC_CATALOG_OBJECT_TARGETS
            || self.catalog_set_record_ids.len() > MAX_GC_CATALOG_SET_TARGETS
            || self.package_record_ids.len() > MAX_GC_PACKAGE_RECORD_TARGETS
            || self.tree_index_ids.len() > MAX_GC_DATA_OBJECT_TARGETS
            || self.legal_artifact_ids.len() > MAX_GC_DATA_OBJECT_TARGETS
            || self
                .tree_objects
                .len()
                .saturating_add(self.retired_trees.len())
                > MAX_GC_TREE_JOBS
            || !strictly_sorted(&self.catalog_object_ids)
            || !strictly_sorted(&self.catalog_set_record_ids)
            || !strictly_sorted(&self.package_record_ids)
            || !strictly_sorted(&self.tree_index_ids)
            || !strictly_sorted(&self.legal_artifact_ids)
            || self
                .tree_objects
                .windows(2)
                .any(|pair| pair[0].tree_sha256 >= pair[1].tree_sha256)
            || self
                .retired_trees
                .windows(2)
                .any(|pair| pair[0] >= pair[1] || pair[0].tree_sha256 == pair[1].tree_sha256)
            || self.tree_objects.iter().any(|tree| {
                self.retired_trees
                    .binary_search_by_key(&tree.tree_sha256, |retired| retired.tree_sha256)
                    .is_ok()
            })
            || self.is_empty()
            || self.retained_bytes() > MAX_GC_INTENT_RETAINED_BYTES
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }

        let catalog_bound_work =
            !self.catalog_set_record_ids.is_empty() || !self.package_record_ids.is_empty();
        if self.cohort.is_none() && catalog_bound_work
            || self.cohort.is_some() && self.package_record_ids.is_empty()
            || self.cohort.is_some() && !self.retired_trees.is_empty()
            || self.cohort.is_some()
                && self.tree_objects.iter().any(|tree| {
                    tree.known_total_entry_count.is_none() || tree.known_tree_bytes.is_none()
                })
            || self.cohort.is_none()
                && self.tree_objects.iter().any(|tree| {
                    tree.known_total_entry_count.is_some() || tree.known_tree_bytes.is_some()
                })
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if let Some(cohort) = self.cohort {
            cohort.validate()?;
            if self.catalog_object_ids.len() > 1
                || self
                    .catalog_object_ids
                    .first()
                    .is_some_and(|digest| *digest != cohort.catalog_sha256)
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }

        let mut charged_entries = 0_usize;
        let mut known_bytes = 0_u64;
        for tree in &self.tree_objects {
            tree.validate()?;
            charged_entries = charged_entries
                .checked_add(
                    tree.known_total_entry_count
                        .map_or(MAX_EXTENSION_TREE_ENTRIES, |count| count as usize),
                )
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            if let Some(bytes) = tree.known_tree_bytes {
                known_bytes = known_bytes
                    .checked_add(bytes)
                    .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            }
        }
        for retired in &self.retired_trees {
            retired.validate()?;
            if retired.retirement_generation >= self.generation {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            charged_entries = charged_entries
                .checked_add(MAX_EXTENSION_TREE_ENTRIES)
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        }
        if charged_entries > MAX_GC_TREE_ENTRIES || known_bytes > MAX_GC_KNOWN_TREE_BYTES {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.catalog_object_ids.is_empty()
            && self.catalog_set_record_ids.is_empty()
            && self.package_record_ids.is_empty()
            && self.tree_index_ids.is_empty()
            && self.legal_artifact_ids.is_empty()
            && self.tree_objects.is_empty()
            && self.retired_trees.is_empty()
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(
                self.catalog_object_ids
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Digest32>()),
            )
            .saturating_add(
                self.catalog_set_record_ids
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Digest32>()),
            )
            .saturating_add(
                self.package_record_ids
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Digest32>()),
            )
            .saturating_add(
                self.tree_index_ids
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Digest32>()),
            )
            .saturating_add(
                self.legal_artifact_ids
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Digest32>()),
            )
            .saturating_add(
                self.tree_objects
                    .capacity()
                    .saturating_mul(std::mem::size_of::<GarbageCollectionTreeObject>()),
            )
            .saturating_add(
                self.retired_trees
                    .capacity()
                    .saturating_mul(std::mem::size_of::<GarbageCollectionRetiredTree>()),
            )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GarbageCollectionTreeObject {
    pub(crate) tree_sha256: Digest32,
    pub(crate) known_total_entry_count: Option<u32>,
    pub(crate) known_tree_bytes: Option<u64>,
}

impl GarbageCollectionTreeObject {
    fn validate(self) -> Result<(), ExtensionRepositoryError> {
        match (self.known_total_entry_count, self.known_tree_bytes) {
            (Some(entries), Some(bytes))
                if entries > 0
                    && entries as usize <= MAX_EXTENSION_TREE_ENTRIES
                    && bytes > 0
                    && bytes <= MAX_EXTENSION_TREE_BYTES =>
            {
                Ok(())
            }
            (None, None) => Ok(()),
            _ => Err(ExtensionRepositoryError::RecoveryAmbiguous),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GarbageCollectionRetiredTree {
    pub(crate) tree_sha256: Digest32,
    pub(crate) retirement_generation: u64,
}

impl GarbageCollectionRetiredTree {
    fn validate(self) -> Result<(), ExtensionRepositoryError> {
        if self.retirement_generation == 0 || self.retirement_generation > MAX_DURABLE_GENERATION {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
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

impl TryFrom<ExtensionCatalogGenerationRole> for HistoricalCatalogRole {
    type Error = ();
    fn try_from(value: ExtensionCatalogGenerationRole) -> Result<Self, Self::Error> {
        match value {
            ExtensionCatalogGenerationRole::Active => Ok(Self::Active),
            ExtensionCatalogGenerationRole::Rollback => Ok(Self::Rollback),
            ExtensionCatalogGenerationRole::Beta => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
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

    fn residue_gc_intent(
        generation: u64,
        catalog_object_ids: Vec<Digest32>,
    ) -> MaterializationGarbageCollectionIntent {
        MaterializationGarbageCollectionIntent {
            schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
            generation,
            cohort: None,
            catalog_object_ids,
            catalog_set_record_ids: Vec::new(),
            package_record_ids: Vec::new(),
            tree_index_ids: Vec::new(),
            legal_artifact_ids: Vec::new(),
            tree_objects: Vec::new(),
            retired_trees: Vec::new(),
        }
    }

    #[test]
    fn gc_targets_replace_completed_roots_and_exclude_build_intents() {
        let target = digest(1);
        let mut state = MaterializationState {
            generation: 2,
            completed_package_record_ids: vec![digest(2)],
            gc_intent: Some(MaterializationGarbageCollectionIntent {
                cohort: Some(super::super::records::tests::package_record_fixture(20).catalog),
                package_record_ids: vec![target],
                ..residue_gc_intent(2, Vec::new())
            }),
            ..MaterializationState::default()
        };
        assert_eq!(state.validate(), Ok(()));

        state.completed_package_record_ids.insert(0, target);
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
        state.completed_package_record_ids.remove(0);

        let package_record = super::super::records::tests::package_record_fixture(31);
        state.build_intent = Some(MaterializationBuildIntent {
            schema_version: MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION,
            generation: 2,
            package_record_id: package_record.record_id().unwrap(),
            package_record,
        });
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn gc_package_partition_never_widens_the_completed_ledger_bound() {
        let package = super::super::records::tests::package_record_fixture(20);
        let target = digest(250);
        let intent = MaterializationGarbageCollectionIntent {
            cohort: Some(package.catalog),
            package_record_ids: vec![target],
            ..residue_gc_intent(1, Vec::new())
        };
        let mut state = MaterializationState {
            generation: 1,
            completed_package_record_ids: (0..MAX_COMPLETED_PACKAGE_RECORDS - 1)
                .map(|index| digest(index as u8 + 1))
                .collect(),
            gc_intent: Some(intent),
            ..MaterializationState::default()
        };
        assert_eq!(state.validate(), Ok(()));

        state
            .completed_package_record_ids
            .push(digest(MAX_COMPLETED_PACKAGE_RECORDS as u8));
        assert_eq!(
            state.validate(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn gc_intent_rejects_noncanonical_and_over_budget_targets() {
        let mut intent = residue_gc_intent(1, vec![digest(1), digest(1)]);
        assert_eq!(
            intent.validate(1),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        intent.catalog_object_ids = (0..=MAX_GC_CATALOG_OBJECT_TARGETS)
            .map(|index| digest(index as u8 + 1))
            .collect();
        assert_eq!(
            intent.validate(1),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        intent = residue_gc_intent(1, vec![digest(1)]);
        intent.tree_objects = (0..=MAX_GC_TREE_JOBS)
            .map(|index| GarbageCollectionTreeObject {
                tree_sha256: digest(index as u8 + 20),
                known_total_entry_count: None,
                known_tree_bytes: None,
            })
            .collect();
        assert_eq!(
            intent.validate(1),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        intent.tree_objects = (0..3)
            .map(|index| GarbageCollectionTreeObject {
                tree_sha256: digest(index + 20),
                known_total_entry_count: Some(1),
                known_tree_bytes: Some(MAX_EXTENSION_TREE_BYTES),
            })
            .collect();
        assert_eq!(
            intent.validate(1),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        intent.tree_objects.clear();
        intent.retired_trees = vec![GarbageCollectionRetiredTree {
            tree_sha256: digest(30),
            retirement_generation: 1,
        }];
        assert_eq!(
            intent.validate(1),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
        intent.generation = 2;
        assert_eq!(intent.validate(2), Ok(()));

        let package = super::super::records::tests::package_record_fixture(80);
        let catalog_only = MaterializationGarbageCollectionIntent {
            cohort: Some(package.catalog),
            ..residue_gc_intent(1, vec![package.catalog.catalog_sha256])
        };
        assert_eq!(
            catalog_only.validate(1),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
        let set_only = MaterializationGarbageCollectionIntent {
            cohort: Some(package.catalog),
            catalog_set_record_ids: vec![digest(100)],
            ..residue_gc_intent(1, vec![package.catalog.catalog_sha256])
        };
        assert_eq!(
            set_only.validate(1),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn maximum_gc_intent_is_canonical_and_memory_bounded() {
        let package = super::super::records::tests::package_record_fixture(41);
        let tree_bytes_per_job = MAX_GC_KNOWN_TREE_BYTES / MAX_GC_TREE_JOBS as u64;
        let intent = MaterializationGarbageCollectionIntent {
            schema_version: MATERIALIZATION_GC_INTENT_SCHEMA_VERSION,
            generation: 1,
            cohort: Some(package.catalog),
            catalog_object_ids: vec![package.catalog.catalog_sha256],
            catalog_set_record_ids: (0..MAX_GC_CATALOG_SET_TARGETS)
                .map(|index| digest(index as u8 + 1))
                .collect(),
            package_record_ids: (0..MAX_GC_PACKAGE_RECORD_TARGETS)
                .map(|index| digest(index as u8 + 20))
                .collect(),
            tree_index_ids: (0..MAX_GC_DATA_OBJECT_TARGETS)
                .map(|index| digest(index as u8 + 60))
                .collect(),
            legal_artifact_ids: (0..MAX_GC_DATA_OBJECT_TARGETS)
                .map(|index| digest(index as u8 + 80))
                .collect(),
            tree_objects: (0..MAX_GC_TREE_JOBS)
                .map(|index| GarbageCollectionTreeObject {
                    tree_sha256: digest(index as u8 + 100),
                    known_total_entry_count: Some(MAX_EXTENSION_TREE_ENTRIES as u32),
                    known_tree_bytes: Some(tree_bytes_per_job),
                })
                .collect(),
            retired_trees: Vec::new(),
        };
        intent.validate(1).unwrap();
        let regular_targets = intent.catalog_object_ids.len()
            + intent.catalog_set_record_ids.len()
            + intent.package_record_ids.len()
            + intent.tree_index_ids.len()
            + intent.legal_artifact_ids.len();
        let tree_jobs = intent.tree_objects.len() + intent.retired_trees.len();
        let charged_tree_entries = intent
            .tree_objects
            .iter()
            .map(|target| target.known_total_entry_count.unwrap() as usize)
            .sum::<usize>();
        assert_eq!(regular_targets, 57);
        assert_eq!(tree_jobs, 8);
        assert_eq!(regular_targets + tree_jobs, 65);
        assert_eq!(charged_tree_entries, 32_768);
        assert_eq!(
            2 * (MAX_MATERIALIZATION_JOURNAL_BYTES
                + MAX_MATERIALIZATION_STATE_BYTES
                + MAX_MATERIALIZATION_CHECKPOINT_BYTES),
            2_129_920
        );
        assert!(intent.retained_bytes() <= MAX_GC_INTENT_RETAINED_BYTES);
        let state = MaterializationState {
            generation: 1,
            gc_intent: Some(intent),
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
            generation: MAX_DURABLE_GENERATION - 1,
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
            gc_intent: Some(residue_gc_intent(
                MAX_DURABLE_GENERATION - 1,
                vec![digest(12)],
            )),
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
        assert!(
            crate::codec::decode_materialization::<MaterializationState>(
                old_empty,
                MAX_MATERIALIZATION_STATE_BYTES,
            )
            .is_err()
        );

        let old_live_pin = br#"{"schema_version":2,"generation":1,"completed_package_record_ids":["0909090909090909090909090909090909090909090909090909090909090909"],"candidate_catalog_set_id":null,"current_catalog_set_id":null,"previous_catalog_set_id":null,"package_pins":[{"profile_id":"00000000000000000000000001","install_id":"00000000000000000000000001","package_record_id":"0909090909090909090909090909090909090909090909090909090909090909","incarnation":1}],"build_intent":null}"#;
        assert!(
            crate::codec::decode_materialization::<MaterializationState>(
                old_live_pin,
                MAX_MATERIALIZATION_STATE_BYTES,
            )
            .is_err()
        );

        // Schema v3 was an unreleased checkout-local format. It is rejected
        // rather than silently defaulting the v4 collector retention field.
        let unreleased_v3 = br#"{"schema_version":3,"generation":0,"completed_package_record_ids":[],"candidate_catalog_set_id":null,"current_catalog_set_id":null,"previous_catalog_set_id":null,"package_pins":[],"build_intent":null}"#;
        assert!(
            crate::codec::decode_materialization::<MaterializationState>(
                unreleased_v3,
                MAX_MATERIALIZATION_STATE_BYTES,
            )
            .is_err()
        );

        let current_empty = br#"{"schema_version":4,"generation":0,"completed_package_record_ids":[],"candidate_catalog_set_id":null,"current_catalog_set_id":null,"previous_catalog_set_id":null,"package_pins":[],"build_intent":null,"gc_intent":null}"#;
        let decoded = crate::codec::decode_materialization::<MaterializationState>(
            current_empty,
            MAX_MATERIALIZATION_STATE_BYTES,
        )
        .unwrap();
        assert_eq!(decoded, MaterializationState::default());
    }
}
