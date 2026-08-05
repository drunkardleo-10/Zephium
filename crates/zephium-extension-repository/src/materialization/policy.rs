//! Shared logical limits and identity-consistency policy for package closures.

use std::collections::{BTreeMap, BTreeSet};

use zephium_extension_package::MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES;

use super::records::{PackageRecord, StoredPayloadIdentity, MAX_CATALOG_SET_PACKAGES};
use super::state::{
    MAX_COMPLETED_PACKAGE_RECORDS, MAX_DURABLE_GENERATION, MAX_RETAINED_CATALOG_SELECTIONS,
};
use crate::state::Digest32;

// Three selected catalog generations plus one bounded drain/cache selection.
// A set chooses one backend per package; it never materializes all backend
// profiles. The aggregate physical ceiling is therefore four catalog budgets.
const _: () = assert!(MAX_CATALOG_SET_PACKAGES > 0);
const _: () = assert!(MAX_COMPLETED_PACKAGE_RECORDS.is_multiple_of(MAX_CATALOG_SET_PACKAGES));
const _: () = assert!(
    MAX_COMPLETED_PACKAGE_RECORDS / MAX_CATALOG_SET_PACKAGES == MAX_RETAINED_CATALOG_SELECTIONS
);

pub(crate) const MAX_COMPLETED_TREE_BYTES: u64 = checked_mul_u64(
    MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES,
    (MAX_COMPLETED_PACKAGE_RECORDS / MAX_CATALOG_SET_PACKAGES) as u64,
);

const fn checked_mul_u64(left: u64, right: u64) -> u64 {
    match left.checked_mul(right) {
        Some(value) => value,
        None => panic!("completed materialization tree budget overflow"),
    }
}

/// Returns one durable successor inside the canonical signed-generation range.
pub(crate) const fn next_durable_generation(current: u64) -> Option<u64> {
    match current.checked_add(1) {
        Some(next) if next <= MAX_DURABLE_GENERATION => Some(next),
        _ => None,
    }
}

/// Reserves any durable intent generation only when its settlement also fits.
pub(crate) const fn reserve_two_transition_intent_generation(current: u64) -> Option<u64> {
    match next_durable_generation(current) {
        Some(intent) if next_durable_generation(intent).is_some() => Some(intent),
        _ => None,
    }
}

/// Stable internal package-closure policy failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackagePolicyError {
    AnchorConflict,
    TreeBudgetExceeded,
    AccountingOverflow,
}

/// Proves that equal content addresses have one exact structural meaning.
pub(crate) fn validate_package_anchor_consistency<'a>(
    packages: impl IntoIterator<Item = &'a PackageRecord>,
) -> Result<(), PackagePolicyError> {
    let mut catalog_anchors = BTreeMap::new();
    let mut package_row_anchors = BTreeMap::new();
    let mut tree_index_anchors = BTreeMap::new();
    let mut tree_anchors = BTreeMap::new();
    let mut manifest_lengths = BTreeMap::new();
    let mut legal_lengths = BTreeMap::new();
    let mut archive_lengths = BTreeMap::new();
    for package in packages {
        if !insert_consistent(
            &mut catalog_anchors,
            package.catalog.catalog_sha256,
            package.catalog,
        ) || !insert_consistent(
            &mut package_row_anchors,
            package.package.package_row_sha256,
            package.package,
        ) || !insert_consistent(
            &mut tree_index_anchors,
            package.tree_index.index_sha256,
            package.tree_index,
        ) || !insert_consistent(
            &mut tree_anchors,
            package.tree_index.tree_sha256,
            package.tree_index,
        ) || !insert_consistent(
            &mut manifest_lengths,
            package.manifest.manifest_sha256,
            package.manifest.manifest_length,
        ) || !insert_consistent(
            &mut legal_lengths,
            package.legal.sha256,
            package.legal.length,
        ) {
            return Err(PackagePolicyError::AnchorConflict);
        }
        if let StoredPayloadIdentity::AcquiredZip { length, sha256 } = package.package.payload {
            if !insert_consistent(&mut archive_lengths, sha256, length) {
                return Err(PackagePolicyError::AnchorConflict);
            }
        }
    }
    Ok(())
}

/// Enforces the aggregate physical tree-byte budget with digest deduplication.
pub(crate) fn validate_completed_tree_budget<'a>(
    packages: impl IntoIterator<Item = &'a PackageRecord>,
) -> Result<(), PackagePolicyError> {
    let mut completed_tree_ids = BTreeSet::<Digest32>::new();
    let mut completed_tree_bytes = 0_u64;
    for package in packages {
        if completed_tree_ids.insert(package.tree_index.tree_sha256) {
            completed_tree_bytes = completed_tree_bytes
                .checked_add(package.tree_index.tree_bytes)
                .ok_or(PackagePolicyError::AccountingOverflow)?;
        }
    }
    if completed_tree_bytes > MAX_COMPLETED_TREE_BYTES {
        return Err(PackagePolicyError::TreeBudgetExceeded);
    }
    Ok(())
}

/// Enforces the signed-catalog aggregate tree budget for one exact set.
///
/// Catalog sets contain one backend row per package, so this deliberately
/// charges every row just like release-catalog admission rather than applying
/// the completed-ledger CAS deduplication policy.
pub(crate) fn validate_catalog_set_tree_budget<'a>(
    packages: impl IntoIterator<Item = &'a PackageRecord>,
) -> Result<(), PackagePolicyError> {
    let mut tree_bytes = 0_u64;
    for package in packages {
        tree_bytes = tree_bytes
            .checked_add(package.tree_index.tree_bytes)
            .ok_or(PackagePolicyError::AccountingOverflow)?;
    }
    if tree_bytes > MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES {
        return Err(PackagePolicyError::TreeBudgetExceeded);
    }
    Ok(())
}

fn insert_consistent<Key, Value>(anchors: &mut BTreeMap<Key, Value>, key: Key, value: Value) -> bool
where
    Key: Ord,
    Value: Copy + Eq,
{
    match anchors.entry(key) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(value);
            true
        }
        std::collections::btree_map::Entry::Occupied(entry) => *entry.get() == value,
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    use crate::materialization::records::tests::package_record_fixture;
    use zephium_extension_package::MAX_EXTENSION_TREE_BYTES;

    #[test]
    fn equal_content_addresses_require_equal_structural_anchors() {
        let first = package_record_fixture(1);
        let mut conflicting = package_record_fixture(2);
        conflicting.catalog.catalog_sha256 = first.catalog.catalog_sha256;
        assert_eq!(
            validate_package_anchor_consistency([&first, &conflicting]),
            Err(PackagePolicyError::AnchorConflict)
        );
        assert_eq!(
            validate_package_anchor_consistency([&first, &first]),
            Ok(())
        );
    }

    #[test]
    fn shared_tree_digests_are_charged_once() {
        let first = package_record_fixture(3);
        let mut shared = package_record_fixture(4);
        shared.tree_index = first.tree_index;
        assert_eq!(validate_completed_tree_budget([&first, &shared]), Ok(()));
    }

    #[test]
    fn catalog_set_budget_charges_every_selected_package_row() {
        let mut first = package_record_fixture(3);
        first.tree_index.tree_bytes = MAX_EXTENSION_TREE_BYTES;
        let mut second = package_record_fixture(20);
        second.tree_index.tree_bytes = MAX_EXTENSION_TREE_BYTES;
        let mut third = package_record_fixture(40);
        third.tree_index.tree_bytes = 1;
        assert_eq!(validate_catalog_set_tree_budget([&first, &second]), Ok(()));
        assert_eq!(
            validate_catalog_set_tree_budget([&first, &second, &third]),
            Err(PackagePolicyError::TreeBudgetExceeded)
        );
    }

    #[test]
    fn durable_intent_generation_reserves_its_settlement() {
        assert_eq!(next_durable_generation(MAX_DURABLE_GENERATION), None);
        assert_eq!(
            reserve_two_transition_intent_generation(MAX_DURABLE_GENERATION - 1),
            None
        );
        assert_eq!(
            reserve_two_transition_intent_generation(MAX_DURABLE_GENERATION - 2),
            Some(MAX_DURABLE_GENERATION - 1)
        );
    }
}
