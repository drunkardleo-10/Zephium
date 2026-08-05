//! Exact materialization namespace and content-addressed object names.

use zephium_private_fs::PrivateComponent;

use super::state::{
    MAX_COMPLETED_PACKAGE_RECORDS, MAX_DURABLE_GENERATION, MAX_RETAINED_CATALOG_SELECTIONS,
};
use crate::state::Digest32;
use crate::ExtensionRepositoryError;

pub(crate) const MAX_MATERIALIZATION_ROOT_ENTRIES: usize = 7;
// Both sides of one maximum-package state transition plus one full generation
// of exact stage/retired residue can coexist before later bounded GC runs.
pub(crate) const MAX_FINAL_PACKAGE_RECORDS: usize = checked_mul(MAX_COMPLETED_PACKAGE_RECORDS, 2);
pub(crate) const MAX_FINAL_DATA_OBJECTS_PER_KIND: usize = MAX_FINAL_PACKAGE_RECORDS;
pub(crate) const MAX_CATALOG_SET_RECORDS_PER_STATE: usize = MAX_RETAINED_CATALOG_SELECTIONS;
pub(crate) const MAX_FINAL_CATALOG_SET_RECORDS: usize =
    checked_mul(MAX_CATALOG_SET_RECORDS_PER_STATE, 2);
pub(crate) const MAX_RETIRED_TREE_RECORDS: usize = MAX_COMPLETED_PACKAGE_RECORDS;
// One in-progress stage may coexist with the complete two-state final
// inventory and one generation of retired residue. Per-kind limits are also
// enforced during inventory so the extra slot cannot become more residue.
pub(crate) const MAX_TREE_ENTRIES: usize = checked_add(
    checked_add(MAX_FINAL_PACKAGE_RECORDS, MAX_RETIRED_TREE_RECORDS),
    1,
);
// Each package has one metadata record and two sealed data objects. Recovery
// can hold both states, both catalog-set inventories, and one stage per kind.
pub(crate) const MAX_RECORD_ENTRIES: usize = checked_add(
    checked_add(
        checked_mul(MAX_FINAL_PACKAGE_RECORDS, 3),
        MAX_FINAL_CATALOG_SET_RECORDS,
    ),
    RecordObjectKind::COUNT,
);
pub(crate) const MAX_MATERIALIZATION_JOURNAL_ENTRIES: usize = 2;

const fn checked_add(left: usize, right: usize) -> usize {
    match left.checked_add(right) {
        Some(value) => value,
        None => panic!("materialization inventory bound overflow"),
    }
}

const fn checked_mul(left: usize, right: usize) -> usize {
    match left.checked_mul(right) {
        Some(value) => value,
        None => panic!("materialization inventory bound overflow"),
    }
}

pub(crate) fn component(value: &str) -> Result<PrivateComponent, ExtensionRepositoryError> {
    PrivateComponent::new(value).map_err(|_| ExtensionRepositoryError::StateCorrupt)
}

macro_rules! fixed_component {
    ($name:ident, $value:literal) => {
        pub(crate) fn $name() -> PrivateComponent {
            component($value).expect("fixed materialization component is valid")
        }
    };
}

fixed_component!(materialization_directory, "materialization");
fixed_component!(trees_directory, "trees");
fixed_component!(records_directory, "records");
fixed_component!(journals_directory, "journals");
fixed_component!(state_file, "state.json");
fixed_component!(state_stage, "state.stage");
fixed_component!(checkpoint_file, "recovery-checkpoint.json");
fixed_component!(checkpoint_stage, "recovery-checkpoint.stage");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TreeNameKind {
    Object,
    Stage(u64),
    Retired(u64),
}

pub(crate) fn tree_object(digest: Digest32) -> PrivateComponent {
    component(&format!("{}.object", digest.to_hex())).expect("tree object name is valid")
}

pub(crate) fn tree_stage(
    digest: Digest32,
    generation: u64,
) -> Result<PrivateComponent, ExtensionRepositoryError> {
    nonzero_generation(generation)?;
    component(&format!("{}.stage-{generation:020}", digest.to_hex()))
}

pub(crate) fn tree_retired(
    digest: Digest32,
    generation: u64,
) -> Result<PrivateComponent, ExtensionRepositoryError> {
    nonzero_generation(generation)?;
    component(&format!("{}.retired-{generation:020}", digest.to_hex()))
}

pub(crate) fn parse_tree_name(value: &str) -> Option<(Digest32, TreeNameKind)> {
    if let Some(digest) = value.strip_suffix(".object") {
        let digest = Digest32::from_lower_hex(digest)?;
        return (tree_object(digest).as_str() == value).then_some((digest, TreeNameKind::Object));
    }
    let (digest, suffix) = value.split_once('.')?;
    let digest = Digest32::from_lower_hex(digest)?;
    if let Some(generation) = suffix.strip_prefix("stage-") {
        let generation = parse_nonzero_generation(generation)?;
        return (tree_stage(digest, generation).ok()?.as_str() == value)
            .then_some((digest, TreeNameKind::Stage(generation)));
    }
    if let Some(generation) = suffix.strip_prefix("retired-") {
        let generation = parse_nonzero_generation(generation)?;
        return (tree_retired(digest, generation).ok()?.as_str() == value)
            .then_some((digest, TreeNameKind::Retired(generation)));
    }
    None
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecordNameKind {
    Package { stage: bool },
    CatalogSet { stage: bool },
    TreeIndex { stage: bool },
    Legal { stage: bool },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum RecordObjectKind {
    Package,
    CatalogSet,
    TreeIndex,
    Legal,
}

impl RecordObjectKind {
    pub(crate) const COUNT: usize = 4;
}

impl RecordNameKind {
    pub(crate) const fn object_kind(self) -> RecordObjectKind {
        match self {
            Self::Package { .. } => RecordObjectKind::Package,
            Self::CatalogSet { .. } => RecordObjectKind::CatalogSet,
            Self::TreeIndex { .. } => RecordObjectKind::TreeIndex,
            Self::Legal { .. } => RecordObjectKind::Legal,
        }
    }

    pub(crate) const fn is_stage(self) -> bool {
        match self {
            Self::Package { stage }
            | Self::CatalogSet { stage }
            | Self::TreeIndex { stage }
            | Self::Legal { stage } => stage,
        }
    }
}

pub(crate) fn package_record(digest: Digest32) -> PrivateComponent {
    component(&format!("{}.package.json", digest.to_hex())).expect("package record name is valid")
}

pub(crate) fn package_record_stage(digest: Digest32) -> PrivateComponent {
    component(&format!("{}.package.stage", digest.to_hex()))
        .expect("package record stage name is valid")
}

pub(crate) fn catalog_set_record(digest: Digest32) -> PrivateComponent {
    component(&format!("{}.catalog-set.json", digest.to_hex()))
        .expect("catalog-set record name is valid")
}

pub(crate) fn catalog_set_record_stage(digest: Digest32) -> PrivateComponent {
    component(&format!("{}.catalog-set.stage", digest.to_hex()))
        .expect("catalog-set record stage name is valid")
}

pub(crate) fn tree_index_object(digest: Digest32) -> PrivateComponent {
    component(&format!("{}.index.json", digest.to_hex())).expect("tree-index object name is valid")
}

pub(crate) fn tree_index_stage(digest: Digest32) -> PrivateComponent {
    component(&format!("{}.index.stage", digest.to_hex())).expect("tree-index stage name is valid")
}

pub(crate) fn legal_object(digest: Digest32) -> PrivateComponent {
    component(&format!("{}.legal", digest.to_hex())).expect("legal object name is valid")
}

pub(crate) fn legal_stage(digest: Digest32) -> PrivateComponent {
    component(&format!("{}.legal.stage", digest.to_hex())).expect("legal stage name is valid")
}

pub(crate) fn parse_record_name(value: &str) -> Option<(Digest32, RecordNameKind)> {
    let (digest, kind) = if let Some(value) = value.strip_suffix(".package.json") {
        (value, RecordNameKind::Package { stage: false })
    } else if let Some(value) = value.strip_suffix(".package.stage") {
        (value, RecordNameKind::Package { stage: true })
    } else if let Some(value) = value.strip_suffix(".catalog-set.json") {
        (value, RecordNameKind::CatalogSet { stage: false })
    } else if let Some(value) = value.strip_suffix(".catalog-set.stage") {
        (value, RecordNameKind::CatalogSet { stage: true })
    } else if let Some(value) = value.strip_suffix(".index.json") {
        (value, RecordNameKind::TreeIndex { stage: false })
    } else if let Some(value) = value.strip_suffix(".index.stage") {
        (value, RecordNameKind::TreeIndex { stage: true })
    } else if let Some(value) = value.strip_suffix(".legal") {
        (value, RecordNameKind::Legal { stage: false })
    } else if let Some(value) = value.strip_suffix(".legal.stage") {
        (value, RecordNameKind::Legal { stage: true })
    } else {
        return None;
    };
    let digest = Digest32::from_lower_hex(digest)?;
    let canonical = match kind {
        RecordNameKind::Package { stage: false } => package_record(digest),
        RecordNameKind::Package { stage: true } => package_record_stage(digest),
        RecordNameKind::CatalogSet { stage: false } => catalog_set_record(digest),
        RecordNameKind::CatalogSet { stage: true } => catalog_set_record_stage(digest),
        RecordNameKind::TreeIndex { stage: false } => tree_index_object(digest),
        RecordNameKind::TreeIndex { stage: true } => tree_index_stage(digest),
        RecordNameKind::Legal { stage: false } => legal_object(digest),
        RecordNameKind::Legal { stage: true } => legal_stage(digest),
    };
    (canonical.as_str() == value).then_some((digest, kind))
}

pub(crate) fn journal_file(
    generation: u64,
    digest: Digest32,
) -> Result<PrivateComponent, ExtensionRepositoryError> {
    nonzero_generation(generation)?;
    component(&format!("{generation:020}-{}.json", digest.to_hex()))
}

pub(crate) fn journal_stage(
    generation: u64,
    digest: Digest32,
) -> Result<PrivateComponent, ExtensionRepositoryError> {
    nonzero_generation(generation)?;
    component(&format!("{generation:020}-{}.stage", digest.to_hex()))
}

pub(crate) fn parse_journal_name(value: &str) -> Option<(u64, Digest32, bool)> {
    let (stem, stage) = if let Some(value) = value.strip_suffix(".json") {
        (value, false)
    } else if let Some(value) = value.strip_suffix(".stage") {
        (value, true)
    } else {
        return None;
    };
    let (generation, digest) = stem.split_once('-')?;
    let generation = parse_nonzero_generation(generation)?;
    let digest = Digest32::from_lower_hex(digest)?;
    let canonical = if stage {
        journal_stage(generation, digest).ok()?
    } else {
        journal_file(generation, digest).ok()?
    };
    (canonical.as_str() == value).then_some((generation, digest, stage))
}

fn parse_generation(value: &str) -> Option<u64> {
    if value.len() != 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let generation: u64 = value.parse().ok()?;
    (format!("{generation:020}") == value).then_some(generation)
}

fn parse_nonzero_generation(value: &str) -> Option<u64> {
    parse_generation(value)
        .filter(|generation| *generation != 0 && *generation <= MAX_DURABLE_GENERATION)
}

fn nonzero_generation(generation: u64) -> Result<(), ExtensionRepositoryError> {
    if generation == 0 || generation > MAX_DURABLE_GENERATION {
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: u8) -> Digest32 {
        Digest32::from_bytes([byte; 32])
    }

    #[test]
    fn object_names_round_trip_only_in_canonical_form() {
        let digest = digest(7);
        assert_eq!(
            parse_tree_name(tree_object(digest).as_str()),
            Some((digest, TreeNameKind::Object))
        );
        assert_eq!(
            parse_tree_name(tree_stage(digest, 9).unwrap().as_str()),
            Some((digest, TreeNameKind::Stage(9)))
        );
        assert_eq!(
            parse_tree_name(tree_retired(digest, 10).unwrap().as_str()),
            Some((digest, TreeNameKind::Retired(10)))
        );
        assert_eq!(parse_tree_name("AA.object"), None);
        assert_eq!(
            parse_tree_name(&format!("{}.stage-9", digest.to_hex())),
            None
        );
    }

    #[test]
    fn record_and_journal_names_round_trip() {
        let digest = digest(11);
        assert_eq!(
            parse_record_name(package_record(digest).as_str()),
            Some((digest, RecordNameKind::Package { stage: false }))
        );
        assert_eq!(
            parse_record_name(catalog_set_record_stage(digest).as_str()),
            Some((digest, RecordNameKind::CatalogSet { stage: true }))
        );
        assert_eq!(
            parse_journal_name(journal_file(3, digest).unwrap().as_str()),
            Some((3, digest, false))
        );
        assert_eq!(parse_journal_name("3-deadbeef.json"), None);
        assert_eq!(
            journal_file(0, digest),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
        assert_eq!(
            tree_stage(digest, 0),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
        assert_eq!(
            tree_retired(digest, u64::MAX),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }
}
