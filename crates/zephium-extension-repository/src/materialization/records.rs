//! Canonical content-addressed package and catalog-set metadata records.

use serde::{Deserialize, Serialize};
use zephium_core::extensions::{
    ExtensionAuthorityId, ExtensionCompatibilityTargetId, ExtensionPackageRevision,
};
use zephium_extension_authority::{
    BundledCatalogGenerationAnchor, BundledCatalogInventoryDigest, ProductExtensionRuntimeTarget,
};
use zephium_extension_package::{
    ExtensionReleaseCatalogDigest, ExtensionReleaseCatalogRevision, PortableRelativePath,
    MAX_EXTENSION_ARCHIVE_BYTES, MAX_EXTENSION_LEGAL_NOTICE_BYTES, MAX_EXTENSION_MANIFEST_BYTES,
    MAX_EXTENSION_PACKAGE_LINES, MAX_EXTENSION_RELEASE_CATALOG_BYTES, MAX_EXTENSION_TREE_BYTES,
    MAX_EXTENSION_TREE_ENTRIES, MAX_EXTENSION_TREE_FILES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

use crate::codec;
use crate::state::Digest32;
use crate::ExtensionRepositoryError;

pub(crate) const PACKAGE_RECORD_SCHEMA_VERSION: u32 = 1;
pub(crate) const CATALOG_SET_RECORD_SCHEMA_VERSION: u32 = 1;
pub(crate) const MAX_PACKAGE_RECORD_BYTES: usize = 64 * 1024;
pub(crate) const MAX_CATALOG_SET_RECORD_BYTES: usize = 32 * 1024;
pub(crate) const MAX_CATALOG_SET_PACKAGES: usize = MAX_EXTENSION_PACKAGE_LINES;

// Raising the signed catalog policy must not silently raise repository
// startup, record, or sealed-root budgets.
const _: () = assert!(MAX_CATALOG_SET_PACKAGES <= 8);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatalogAnchor {
    pub(crate) authority_id: Digest32,
    pub(crate) revision: u64,
    pub(crate) catalog_length: u64,
    pub(crate) catalog_sha256: Digest32,
    pub(crate) inventory_sha256: Digest32,
}

impl CatalogAnchor {
    fn validate(self) -> Result<(), ExtensionRepositoryError> {
        if ExtensionReleaseCatalogRevision::new(self.revision).is_none()
            || self.catalog_length == 0
            || self.catalog_length > MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }

    pub(crate) fn generation_anchor(
        self,
    ) -> Result<BundledCatalogGenerationAnchor, ExtensionRepositoryError> {
        self.validate()?;
        let revision = ExtensionReleaseCatalogRevision::new(self.revision)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        Ok(BundledCatalogGenerationAnchor::from_parts(
            ExtensionAuthorityId::from_bytes(self.authority_id.bytes()),
            revision,
            self.catalog_length,
            ExtensionReleaseCatalogDigest::from_bytes(self.catalog_sha256.bytes()),
            BundledCatalogInventoryDigest::from_bytes(self.inventory_sha256.bytes()),
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub(crate) enum StoredPayloadIdentity {
    BundledTree,
    AcquiredZip { length: u64, sha256: Digest32 },
}

impl StoredPayloadIdentity {
    fn validate(self) -> Result<(), ExtensionRepositoryError> {
        if let Self::AcquiredZip { length, .. } = self {
            if length == 0 || length > MAX_EXTENSION_ARCHIVE_BYTES {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageIdentityAnchor {
    pub(crate) authority_id: Digest32,
    pub(crate) package_key: Digest32,
    pub(crate) revision: u64,
    pub(crate) payload: StoredPayloadIdentity,
    pub(crate) manifest_sha256: Digest32,
    pub(crate) tree_sha256: Digest32,
    pub(crate) package_row_sha256: Digest32,
    pub(crate) chromium_manifest_key_sha256: Option<Digest32>,
}

impl PackageIdentityAnchor {
    fn validate(self, catalog: CatalogAnchor) -> Result<(), ExtensionRepositoryError> {
        if self.authority_id != catalog.authority_id
            || ExtensionPackageRevision::new(self.revision).is_none()
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        self.payload.validate()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TreeIndexAnchor {
    pub(crate) index_sha256: Digest32,
    pub(crate) index_length: u64,
    pub(crate) tree_sha256: Digest32,
    pub(crate) file_count: u32,
    pub(crate) directory_count: u32,
    pub(crate) total_entry_count: u32,
    pub(crate) tree_bytes: u64,
}

impl TreeIndexAnchor {
    fn validate(self, package: PackageIdentityAnchor) -> Result<(), ExtensionRepositoryError> {
        let expected_total = self
            .file_count
            .checked_add(self.directory_count)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if self.index_length == 0
            || self.index_length > MAX_EXTENSION_TREE_INDEX_BYTES as u64
            || self.tree_sha256 != package.tree_sha256
            || self.file_count == 0
            || self.file_count as usize > MAX_EXTENSION_TREE_FILES
            || self.total_entry_count != expected_total
            || self.total_entry_count as usize > MAX_EXTENSION_TREE_ENTRIES
            || self.tree_bytes == 0
            || self.tree_bytes > MAX_EXTENSION_TREE_BYTES
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredRuntimeTarget {
    MacosNative,
    MacosCompatibility,
    LinuxCompatibility,
    WindowsNative,
}

impl StoredRuntimeTarget {
    const fn product_target(self) -> ProductExtensionRuntimeTarget {
        match self {
            Self::MacosNative => ProductExtensionRuntimeTarget::MacosNative,
            Self::MacosCompatibility => ProductExtensionRuntimeTarget::MacosCompatibility,
            Self::LinuxCompatibility => ProductExtensionRuntimeTarget::LinuxCompatibility,
            Self::WindowsNative => ProductExtensionRuntimeTarget::WindowsNative,
        }
    }

    pub(crate) const fn platform_family(self) -> StoredRuntimePlatformFamily {
        match self {
            Self::MacosNative | Self::MacosCompatibility => StoredRuntimePlatformFamily::Macos,
            Self::LinuxCompatibility => StoredRuntimePlatformFamily::Linux,
            Self::WindowsNative => StoredRuntimePlatformFamily::Windows,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StoredRuntimePlatformFamily {
    Macos,
    Linux,
    Windows,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestAnchor {
    pub(crate) runtime_target: StoredRuntimeTarget,
    pub(crate) compatibility_target: String,
    pub(crate) manifest_length: u64,
    pub(crate) manifest_sha256: Digest32,
    pub(crate) admission_sha256: Digest32,
}

impl ManifestAnchor {
    fn validate(&self, package: PackageIdentityAnchor) -> Result<(), ExtensionRepositoryError> {
        if self.manifest_length == 0
            || self.manifest_length > MAX_EXTENSION_MANIFEST_BYTES as u64
            || self.manifest_sha256 != package.manifest_sha256
            || ExtensionCompatibilityTargetId::parse_exact(&self.compatibility_target).is_err()
            || self.compatibility_target
                != self
                    .runtime_target
                    .product_target()
                    .compatibility_target_id()
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LegalArtifactAnchor {
    pub(crate) target: String,
    pub(crate) kind: StoredLegalArtifactKind,
    pub(crate) length: u64,
    pub(crate) sha256: Digest32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredLegalArtifactKind {
    NoticeBundle,
}

impl LegalArtifactAnchor {
    fn validate(&self) -> Result<(), ExtensionRepositoryError> {
        if self.length == 0
            || self.length > MAX_EXTENSION_LEGAL_NOTICE_BYTES
            || PortableRelativePath::parse(&self.target).is_err()
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageRecord {
    pub(crate) schema_version: u32,
    pub(crate) catalog: CatalogAnchor,
    pub(crate) package: PackageIdentityAnchor,
    pub(crate) tree_index: TreeIndexAnchor,
    pub(crate) manifest: ManifestAnchor,
    pub(crate) legal: LegalArtifactAnchor,
}

impl PackageRecord {
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, ExtensionRepositoryError> {
        let record: Self = codec::decode_materialization(bytes, MAX_PACKAGE_RECORD_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
        record.validate()?;
        Ok(record)
    }

    pub(crate) fn canonical_bytes(&self) -> Result<Vec<u8>, ExtensionRepositoryError> {
        self.validate()?;
        codec::encode(self, MAX_PACKAGE_RECORD_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)
    }

    pub(crate) fn record_id(&self) -> Result<Digest32, ExtensionRepositoryError> {
        self.canonical_bytes().map(|bytes| codec::digest(&bytes))
    }

    pub(crate) fn validate(&self) -> Result<(), ExtensionRepositoryError> {
        if self.schema_version != PACKAGE_RECORD_SCHEMA_VERSION {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        self.catalog.validate()?;
        self.package.validate(self.catalog)?;
        self.tree_index.validate(self.package)?;
        self.manifest.validate(self.package)?;
        self.legal.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatalogSetRecord {
    pub(crate) schema_version: u32,
    pub(crate) catalog: CatalogAnchor,
    /// One deterministic backend selection per package in this catalog.
    ///
    /// This is durable structural metadata, not product authority. A future
    /// activation operation must construct the complete one-to-one projection
    /// from freshly admitted exact catalog/profile witnesses and must re-admit
    /// those bytes before issuing an execution capability.
    pub(crate) packages: Vec<CatalogSetPackageRow>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatalogSetPackageRow {
    pub(crate) package_key: Digest32,
    pub(crate) runtime_target: StoredRuntimeTarget,
    pub(crate) package_record_id: Digest32,
}

impl CatalogSetRecord {
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, ExtensionRepositoryError> {
        let record: Self = codec::decode_materialization(bytes, MAX_CATALOG_SET_RECORD_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
        record.validate()?;
        Ok(record)
    }

    pub(crate) fn canonical_bytes(&self) -> Result<Vec<u8>, ExtensionRepositoryError> {
        self.validate()?;
        codec::encode(self, MAX_CATALOG_SET_RECORD_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)
    }

    pub(crate) fn record_id(&self) -> Result<Digest32, ExtensionRepositoryError> {
        self.canonical_bytes().map(|bytes| codec::digest(&bytes))
    }

    fn validate(&self) -> Result<(), ExtensionRepositoryError> {
        if self.schema_version != CATALOG_SET_RECORD_SCHEMA_VERSION {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        self.catalog.validate()?;
        let Some(first) = self.packages.first() else {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        };
        if self.packages.len() > MAX_CATALOG_SET_PACKAGES
            || self.packages.windows(2).any(|pair| {
                pair[0].package_key >= pair[1].package_key
                    || pair[0].package_record_id == pair[1].package_record_id
            })
            || self.packages.iter().enumerate().any(|(index, row)| {
                row.runtime_target.platform_family() != first.runtime_target.platform_family()
                    || self.packages[..index]
                        .iter()
                        .any(|prior| prior.package_record_id == row.package_record_id)
            })
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn package_record_fixture(seed: u8) -> PackageRecord {
        let digest = |offset: u8| Digest32::from_bytes([seed.wrapping_add(offset); 32]);
        PackageRecord {
            schema_version: PACKAGE_RECORD_SCHEMA_VERSION,
            catalog: CatalogAnchor {
                authority_id: digest(1),
                revision: 1,
                catalog_length: 100,
                catalog_sha256: digest(2),
                inventory_sha256: digest(3),
            },
            package: PackageIdentityAnchor {
                authority_id: digest(1),
                package_key: digest(4),
                revision: 1,
                payload: StoredPayloadIdentity::BundledTree,
                manifest_sha256: digest(5),
                tree_sha256: digest(6),
                package_row_sha256: digest(7),
                chromium_manifest_key_sha256: None,
            },
            tree_index: TreeIndexAnchor {
                index_sha256: digest(8),
                index_length: 80,
                tree_sha256: digest(6),
                file_count: 2,
                directory_count: 1,
                total_entry_count: 3,
                tree_bytes: 90,
            },
            manifest: ManifestAnchor {
                runtime_target: StoredRuntimeTarget::MacosCompatibility,
                compatibility_target: "macos.zephium-mv3-compat.v1".to_owned(),
                manifest_length: 40,
                manifest_sha256: digest(5),
                admission_sha256: digest(9),
            },
            legal: LegalArtifactAnchor {
                target: "licenses/notice.txt".to_owned(),
                kind: StoredLegalArtifactKind::NoticeBundle,
                length: 20,
                sha256: digest(10),
            },
        }
    }

    pub(crate) fn catalog_set_fixture(package: &PackageRecord) -> CatalogSetRecord {
        CatalogSetRecord {
            schema_version: CATALOG_SET_RECORD_SCHEMA_VERSION,
            catalog: package.catalog,
            packages: vec![CatalogSetPackageRow {
                package_key: package.package.package_key,
                runtime_target: package.manifest.runtime_target,
                package_record_id: package.record_id().unwrap(),
            }],
        }
    }

    #[test]
    fn records_round_trip_canonically_and_are_content_addressed() {
        let package = package_record_fixture(1);
        let bytes = package.canonical_bytes().unwrap();
        assert_eq!(PackageRecord::decode(&bytes).unwrap(), package);
        assert_eq!(codec::digest(&bytes), package.record_id().unwrap());

        let catalog_set = catalog_set_fixture(&package);
        let bytes = catalog_set.canonical_bytes().unwrap();
        assert_eq!(CatalogSetRecord::decode(&bytes).unwrap(), catalog_set);
        assert_eq!(codec::digest(&bytes), catalog_set.record_id().unwrap());
    }

    #[test]
    fn duplicate_unsorted_and_overfull_catalog_sets_fail_closed() {
        let package = package_record_fixture(2);
        let id = package.record_id().unwrap();
        let mut set = catalog_set_fixture(&package);
        set.packages = vec![
            CatalogSetPackageRow {
                package_key: Digest32::from_bytes([1; 32]),
                runtime_target: StoredRuntimeTarget::MacosNative,
                package_record_id: id,
            },
            CatalogSetPackageRow {
                package_key: Digest32::from_bytes([1; 32]),
                runtime_target: StoredRuntimeTarget::MacosCompatibility,
                package_record_id: id,
            },
        ];
        assert_eq!(
            set.canonical_bytes(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        set.packages = (0..=MAX_CATALOG_SET_PACKAGES)
            .map(|index| CatalogSetPackageRow {
                package_key: Digest32::from_bytes([index as u8; 32]),
                runtime_target: StoredRuntimeTarget::MacosCompatibility,
                package_record_id: Digest32::from_bytes([index as u8; 32]),
            })
            .collect();
        assert_eq!(
            set.canonical_bytes(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn catalog_sets_allow_only_one_platform_family_and_one_backend_per_package() {
        let first = package_record_fixture(20);
        let mut second = package_record_fixture(40);
        second.catalog = first.catalog;
        second.package.authority_id = first.catalog.authority_id;
        second.manifest.runtime_target = StoredRuntimeTarget::MacosNative;
        second.manifest.compatibility_target = "macos.wkwebextension.v1".to_owned();
        let mut set = CatalogSetRecord {
            schema_version: CATALOG_SET_RECORD_SCHEMA_VERSION,
            catalog: first.catalog,
            packages: vec![
                CatalogSetPackageRow {
                    package_key: first.package.package_key,
                    runtime_target: first.manifest.runtime_target,
                    package_record_id: first.record_id().unwrap(),
                },
                CatalogSetPackageRow {
                    package_key: second.package.package_key,
                    runtime_target: second.manifest.runtime_target,
                    package_record_id: second.record_id().unwrap(),
                },
            ],
        };
        set.packages.sort_unstable_by_key(|row| row.package_key);
        assert!(set.canonical_bytes().is_ok());

        set.packages[1].runtime_target = StoredRuntimeTarget::WindowsNative;
        assert_eq!(
            set.canonical_bytes(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        set.packages[1].runtime_target = StoredRuntimeTarget::MacosNative;
        set.packages[1].package_key = set.packages[0].package_key;
        assert_eq!(
            set.canonical_bytes(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn revisions_must_fit_every_durable_adapter() {
        let mut package = package_record_fixture(4);
        package.catalog.revision = u64::MAX;
        assert_eq!(
            package.canonical_bytes(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        let mut package = package_record_fixture(4);
        package.package.revision = u64::MAX;
        assert_eq!(
            package.canonical_bytes(),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn every_exact_anchor_participates_in_the_record_identity() {
        let baseline = package_record_fixture(3);
        let baseline_id = baseline.record_id().unwrap();
        let mut variants = Vec::new();
        let mut variant = baseline.clone();
        variant.catalog.inventory_sha256 = Digest32::from_bytes([90; 32]);
        variants.push(variant);
        let mut variant = baseline.clone();
        variant.package.package_row_sha256 = Digest32::from_bytes([91; 32]);
        variants.push(variant);
        let mut variant = baseline.clone();
        variant.package.chromium_manifest_key_sha256 = Some(Digest32::from_bytes([93; 32]));
        variants.push(variant);
        let mut variant = baseline.clone();
        variant.tree_index.directory_count = 2;
        variant.tree_index.total_entry_count = 4;
        variants.push(variant);
        let mut variant = baseline.clone();
        variant.manifest.runtime_target = StoredRuntimeTarget::WindowsNative;
        variant.manifest.compatibility_target = "windows.webview2.v1".to_owned();
        variants.push(variant);
        let mut variant = baseline.clone();
        variant.legal.sha256 = Digest32::from_bytes([92; 32]);
        variants.push(variant);

        for variant in variants {
            assert_ne!(variant.record_id().unwrap(), baseline_id);
        }
    }
}
