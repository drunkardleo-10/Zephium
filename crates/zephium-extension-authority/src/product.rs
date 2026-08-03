//! Product-sealed bundled catalog admission.

use std::fmt;

use sha2::{Digest, Sha256};
use zephium_core::extensions::{ExtensionAuthorityId, ExtensionPackagePayloadIdentity};
use zephium_extension_package::{
    ExtensionPackageAdmissionPolicyDigest, ExtensionReleaseAdmissionPolicy,
    ExtensionReleaseCatalog, ExtensionReleaseCatalogDigest, ExtensionReleaseCatalogRevision,
    MAX_EXTENSION_RELEASE_CATALOG_BYTES, MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES,
};

use crate::inventory::digest_catalog_inventory;
use crate::{
    BundledCatalogAdmissionError, BundledCatalogCheckpoint, BundledCatalogDisposition,
    BundledCatalogInventoryDigest,
};

// Stable logical reserve for the witness fields, allocator indirection, and
// future fixed-size metadata. Any newly owned allocation must be charged
// separately; the compile-time assertion below prevents the wrapper itself
// from silently outgrowing this reserve.
const ADMITTED_CATALOG_ACCOUNTING_OVERHEAD: usize = 256;

/// Maximum logical memory retained by one admitted bundled-catalog witness.
pub const MAX_ADMITTED_BUNDLED_CATALOG_RETAINED_BYTES: usize =
    MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES + ADMITTED_CATALOG_ACCOUNTING_OVERHEAD;

/// Availability of the product-sealed bundled extension authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "the bundled product-authority availability must be handled"]
pub enum BundledProductAuthorityStatus {
    /// No reviewed catalog/artifact/redistribution anchor is compiled in.
    Unprovisioned,
    /// A bounded anchor and matching policy label are compiled in.
    ///
    /// Exact caller-supplied catalog bytes still require full admission.
    Configured,
    /// Compiled anchor material exists but is internally inconsistent.
    InvalidProvisioning,
}

/// Product-owned authentication boundary for bundled extension catalogs.
///
/// This type has no public anchor, policy, or trust-provider constructor. The
/// only production constructor reads product-sealed compile-time state.
pub struct BundledPackageAuthority {
    anchor: SealedBundledCatalogAnchor,
    policy: ExtensionReleaseAdmissionPolicy,
}

impl BundledPackageAuthority {
    /// Opens the product-sealed authority or explicitly reports that this
    /// build has no approved bundled extension package anchor.
    pub fn product() -> Result<Self, BundledCatalogAdmissionError> {
        let anchor = SEALED_PRODUCT_BUNDLED_CATALOG_ANCHOR
            .ok_or(BundledCatalogAdmissionError::Unprovisioned)?;
        let policy = sealed_product_admission_policy()
            .ok_or(BundledCatalogAdmissionError::InvalidProductConfiguration)?;
        Self::from_sealed_parts(anchor, policy)
    }

    /// Reports whether product-sealed anchor and policy material is configured.
    ///
    /// [`BundledProductAuthorityStatus::Configured`] does not authenticate any
    /// catalog bytes; callers must still successfully call [`Self::admit_catalog`].
    pub fn product_status() -> BundledProductAuthorityStatus {
        match Self::product() {
            Ok(_) => BundledProductAuthorityStatus::Configured,
            Err(BundledCatalogAdmissionError::Unprovisioned) => {
                BundledProductAuthorityStatus::Unprovisioned
            }
            Err(_) => BundledProductAuthorityStatus::InvalidProvisioning,
        }
    }

    /// Authenticates and semantically admits exact canonical catalog bytes.
    ///
    /// Length is checked before hashing, and digest is checked before parsing.
    /// The resulting witness is metadata only: it is not a materialization
    /// lease, durable object pin, profile grant, or native activation authority.
    pub fn admit_catalog(
        &self,
        catalog_bytes: &[u8],
    ) -> Result<AdmittedBundledCatalog, BundledCatalogAdmissionError> {
        if catalog_bytes.len() != self.anchor.catalog_length {
            return Err(BundledCatalogAdmissionError::CatalogLengthMismatch);
        }
        let observed_digest = ExtensionReleaseCatalogDigest::from_bytes(<[u8; 32]>::from(
            Sha256::digest(catalog_bytes),
        ));
        if observed_digest != self.anchor.catalog_digest {
            return Err(BundledCatalogAdmissionError::CatalogDigestMismatch);
        }

        let catalog = ExtensionReleaseCatalog::parse_canonical(catalog_bytes)
            .map_err(BundledCatalogAdmissionError::Catalog)?;
        if catalog.authority() != self.anchor.authority {
            return Err(BundledCatalogAdmissionError::AuthorityMismatch);
        }
        if catalog.revision() != self.anchor.catalog_revision {
            return Err(BundledCatalogAdmissionError::RevisionMismatch);
        }
        if catalog.admission_policy_sha256() != self.anchor.admission_policy_digest {
            return Err(BundledCatalogAdmissionError::PolicyMismatch);
        }
        catalog
            .bind_admission_policy(&self.policy)
            .map_err(BundledCatalogAdmissionError::Catalog)?;
        if catalog
            .packages()
            .iter()
            .any(|package| package.payload() != ExtensionPackagePayloadIdentity::BundledTree)
        {
            return Err(BundledCatalogAdmissionError::UnsupportedPayload);
        }

        let inventory_digest = digest_catalog_inventory(&catalog)
            .ok_or(BundledCatalogAdmissionError::AccountingOverflow)?;
        if inventory_digest != self.anchor.inventory_digest {
            return Err(BundledCatalogAdmissionError::InventoryMismatch);
        }
        let retained_bytes = catalog
            .retained_bytes()
            .checked_add(ADMITTED_CATALOG_ACCOUNTING_OVERHEAD)
            .ok_or(BundledCatalogAdmissionError::AccountingOverflow)?;
        if retained_bytes > MAX_ADMITTED_BUNDLED_CATALOG_RETAINED_BYTES {
            return Err(BundledCatalogAdmissionError::RetainedBytesExceeded);
        }

        Ok(AdmittedBundledCatalog {
            catalog,
            inventory_digest,
            retained_bytes,
        })
    }

    fn from_sealed_parts(
        anchor: SealedBundledCatalogAnchor,
        policy: ExtensionReleaseAdmissionPolicy,
    ) -> Result<Self, BundledCatalogAdmissionError> {
        if anchor.catalog_length == 0
            || anchor.catalog_length > MAX_EXTENSION_RELEASE_CATALOG_BYTES
            || anchor.admission_policy_digest != policy.digest()
        {
            return Err(BundledCatalogAdmissionError::InvalidProductConfiguration);
        }
        Ok(Self { anchor, policy })
    }
}

/// Authenticated, semantically admitted metadata for one bundled catalog.
///
/// This witness intentionally cannot expose package bytes, filesystem paths,
/// or activation operations. It is non-serializable and is not a materialized
/// package lease, durable pin, profile grant, or native authority. A later
/// repository layer must establish all of those independently.
#[must_use = "admitted metadata must be checkpointed or deliberately discarded"]
pub struct AdmittedBundledCatalog {
    catalog: ExtensionReleaseCatalog,
    inventory_digest: BundledCatalogInventoryDigest,
    retained_bytes: usize,
}

const _: () =
    assert!(std::mem::size_of::<AdmittedBundledCatalog>() <= ADMITTED_CATALOG_ACCOUNTING_OVERHEAD);

impl fmt::Debug for AdmittedBundledCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmittedBundledCatalog")
            .field("authority", &self.authority())
            .field("revision", &self.revision())
            .field("catalog_digest", &self.catalog_digest())
            .field("inventory_digest", &self.inventory_digest)
            .field("package_count", &self.catalog.packages().len())
            .field("retained_bytes", &self.retained_bytes)
            .finish()
    }
}

impl AdmittedBundledCatalog {
    /// Returns authenticated, structurally parsed catalog metadata.
    pub const fn catalog(&self) -> &ExtensionReleaseCatalog {
        &self.catalog
    }

    /// Returns the trust-domain and epoch identity.
    pub const fn authority(&self) -> ExtensionAuthorityId {
        self.catalog.authority()
    }

    /// Returns the authenticated catalog revision.
    pub const fn revision(&self) -> ExtensionReleaseCatalogRevision {
        self.catalog.revision()
    }

    /// Returns SHA-256 of exact authenticated canonical catalog bytes.
    pub const fn catalog_digest(&self) -> ExtensionReleaseCatalogDigest {
        self.catalog.digest()
    }

    /// Returns the redundant deterministic package-inventory digest.
    pub const fn inventory_digest(&self) -> BundledCatalogInventoryDigest {
        self.inventory_digest
    }

    /// Returns the explicit logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Creates the structural checkpoint projection suitable for durable state.
    ///
    /// Persisting it and resolving commit ambiguity are outside this crate.
    pub const fn checkpoint(&self) -> BundledCatalogCheckpoint {
        BundledCatalogCheckpoint::from_parts(
            self.authority(),
            self.revision(),
            self.catalog_digest(),
            self.inventory_digest,
        )
    }

    /// Classifies this admission against an optional durable high-water mark.
    pub fn disposition_against(
        &self,
        checkpoint: Option<&BundledCatalogCheckpoint>,
    ) -> BundledCatalogDisposition {
        checkpoint.map_or(BundledCatalogDisposition::Candidate, |floor| {
            floor.classify(&self.checkpoint())
        })
    }
}

#[derive(Clone, Copy)]
struct SealedBundledCatalogAnchor {
    catalog_length: usize,
    catalog_digest: ExtensionReleaseCatalogDigest,
    authority: ExtensionAuthorityId,
    catalog_revision: ExtensionReleaseCatalogRevision,
    admission_policy_digest: ExtensionPackageAdmissionPolicyDigest,
    inventory_digest: BundledCatalogInventoryDigest,
}

// Deliberately absent until exact approved package, license, corresponding
// source, and redistribution artifacts are bound into a signed Zephium build.
// This private compile-time slot is the only production trust root; it must
// never be populated from runtime configuration, an environment variable, or
// caller-provided bytes.
const SEALED_PRODUCT_BUNDLED_CATALOG_ANCHOR: Option<SealedBundledCatalogAnchor> = None;

fn sealed_product_admission_policy() -> Option<ExtensionReleaseAdmissionPolicy> {
    // The policy and anchor must land atomically once the reviewed release
    // artifact exists. Returning `None` preserves an explicit fail-closed build.
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_extension_package::{ExtensionReleaseCatalogError, ExtensionReleaseLicenseRule};

    const AUTHORITY_BYTE: u8 = 1;
    const POLICY_BYTE: u8 = 2;

    fn hex(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    fn package_json(key: u8, payload: &str) -> String {
        format!(
            concat!(
                r#"{{"package_key":"{}","revision":1,"payload":{},"manifest_sha256":"{}","tree_sha256":"{}","tree_index_sha256":"{}","tree_file_count":1,"tree_bytes":4,"chromium":null,"provenance":{{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Example contributors","redistribution":"Reviewed bundled release","legal_notice":{{"target":"licenses/example.txt","kind":"notice_bundle","length":1,"sha256":"{}"}},"corresponding_source":null}}}}"#
            ),
            hex(key),
            payload,
            hex(3),
            hex(4),
            hex(5),
            hex(6),
        )
    }

    fn catalog_bytes_with(
        authority: u8,
        revision: u64,
        policy: u8,
        packages: &[String],
    ) -> Vec<u8> {
        format!(
            concat!(
                r#"{{"schema_version":1,"catalog_revision":{},"created_unix":1700000000,"authority_id":"{}","admission_policy_sha256":"{}","packages":[{}]}}"#
            ),
            revision,
            hex(authority),
            hex(policy),
            packages.join(","),
        )
        .into_bytes()
    }

    fn catalog_bytes() -> Vec<u8> {
        catalog_bytes_with(
            AUTHORITY_BYTE,
            1,
            POLICY_BYTE,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#)],
        )
    }

    fn policy(digest: u8) -> ExtensionReleaseAdmissionPolicy {
        ExtensionReleaseAdmissionPolicy::new(
            ExtensionPackageAdmissionPolicyDigest::from_bytes([digest; 32]),
            vec![ExtensionReleaseLicenseRule::new("MPL-2.0", false).unwrap()],
        )
        .unwrap()
    }

    fn anchor_for(bytes: &[u8]) -> SealedBundledCatalogAnchor {
        let catalog = ExtensionReleaseCatalog::parse_canonical(bytes).unwrap();
        SealedBundledCatalogAnchor {
            catalog_length: bytes.len(),
            catalog_digest: ExtensionReleaseCatalogDigest::from_bytes(Sha256::digest(bytes).into()),
            authority: catalog.authority(),
            catalog_revision: catalog.revision(),
            admission_policy_digest: catalog.admission_policy_sha256(),
            inventory_digest: digest_catalog_inventory(&catalog).unwrap(),
        }
    }

    fn authority_for(bytes: &[u8]) -> BundledPackageAuthority {
        BundledPackageAuthority::from_sealed_parts(anchor_for(bytes), policy(POLICY_BYTE)).unwrap()
    }

    #[test]
    fn production_authority_is_explicitly_unprovisioned() {
        assert!(matches!(
            BundledPackageAuthority::product(),
            Err(BundledCatalogAdmissionError::Unprovisioned)
        ));
        assert_eq!(
            BundledPackageAuthority::product_status(),
            BundledProductAuthorityStatus::Unprovisioned
        );
    }

    #[test]
    fn sealed_product_configuration_rejects_invalid_bounds_and_policy_pairing() {
        let bytes = catalog_bytes();

        let mut anchor = anchor_for(&bytes);
        anchor.catalog_length = 0;
        assert!(matches!(
            BundledPackageAuthority::from_sealed_parts(anchor, policy(POLICY_BYTE)),
            Err(BundledCatalogAdmissionError::InvalidProductConfiguration)
        ));

        let mut anchor = anchor_for(&bytes);
        anchor.catalog_length = MAX_EXTENSION_RELEASE_CATALOG_BYTES + 1;
        assert!(matches!(
            BundledPackageAuthority::from_sealed_parts(anchor, policy(POLICY_BYTE)),
            Err(BundledCatalogAdmissionError::InvalidProductConfiguration)
        ));

        let anchor = anchor_for(&bytes);
        assert!(matches!(
            BundledPackageAuthority::from_sealed_parts(anchor, policy(9)),
            Err(BundledCatalogAdmissionError::InvalidProductConfiguration)
        ));
    }

    #[test]
    fn exact_fixture_is_admitted_as_metadata_only() {
        let bytes = catalog_bytes();
        let admitted = authority_for(&bytes).admit_catalog(&bytes).unwrap();
        assert_eq!(admitted.catalog().packages().len(), 1);
        assert_eq!(admitted.authority().bytes(), [AUTHORITY_BYTE; 32]);
        assert_eq!(admitted.revision().get(), 1);
        assert_eq!(
            admitted.retained_bytes(),
            admitted.catalog().retained_bytes() + ADMITTED_CATALOG_ACCOUNTING_OVERHEAD
        );
        assert!(admitted.retained_bytes() <= MAX_ADMITTED_BUNDLED_CATALOG_RETAINED_BYTES);
        assert_eq!(
            admitted.disposition_against(None),
            BundledCatalogDisposition::Candidate
        );
    }

    #[test]
    fn exact_length_is_checked_before_hashing_or_parsing() {
        let bytes = catalog_bytes();
        let authority = authority_for(&bytes);
        let mut extra = bytes.clone();
        extra.extend_from_slice(br#"not-json-and-not-part-of-the-slice"#);
        assert_eq!(
            authority.admit_catalog(&extra).unwrap_err(),
            BundledCatalogAdmissionError::CatalogLengthMismatch
        );
    }

    #[test]
    fn exact_catalog_digest_is_checked_before_parsing() {
        let bytes = catalog_bytes();
        let authority = authority_for(&bytes);
        let malformed_same_length = vec![b'!'; bytes.len()];
        assert_eq!(
            authority.admit_catalog(&malformed_same_length).unwrap_err(),
            BundledCatalogAdmissionError::CatalogDigestMismatch
        );
    }

    #[test]
    fn authority_revision_policy_and_inventory_are_redundantly_bound() {
        let bytes = catalog_bytes();

        let mut anchor = anchor_for(&bytes);
        anchor.authority = ExtensionAuthorityId::from_bytes([9; 32]);
        assert_eq!(
            BundledPackageAuthority::from_sealed_parts(anchor, policy(POLICY_BYTE))
                .unwrap()
                .admit_catalog(&bytes)
                .unwrap_err(),
            BundledCatalogAdmissionError::AuthorityMismatch
        );

        let mut anchor = anchor_for(&bytes);
        anchor.catalog_revision = ExtensionReleaseCatalogRevision::new(2).unwrap();
        assert_eq!(
            BundledPackageAuthority::from_sealed_parts(anchor, policy(POLICY_BYTE))
                .unwrap()
                .admit_catalog(&bytes)
                .unwrap_err(),
            BundledCatalogAdmissionError::RevisionMismatch
        );

        let mut anchor = anchor_for(&bytes);
        anchor.admission_policy_digest = ExtensionPackageAdmissionPolicyDigest::from_bytes([9; 32]);
        assert_eq!(
            BundledPackageAuthority::from_sealed_parts(anchor, policy(9))
                .unwrap()
                .admit_catalog(&bytes)
                .unwrap_err(),
            BundledCatalogAdmissionError::PolicyMismatch
        );

        let mut anchor = anchor_for(&bytes);
        anchor.inventory_digest = BundledCatalogInventoryDigest::from_bytes([9; 32]);
        assert_eq!(
            BundledPackageAuthority::from_sealed_parts(anchor, policy(POLICY_BYTE))
                .unwrap()
                .admit_catalog(&bytes)
                .unwrap_err(),
            BundledCatalogAdmissionError::InventoryMismatch
        );
    }

    #[test]
    fn bundled_trust_path_rejects_acquired_archives() {
        let bytes = catalog_bytes_with(
            AUTHORITY_BYTE,
            1,
            POLICY_BYTE,
            &[package_json(
                7,
                &format!(
                    r#"{{"kind":"acquired_zip","length":4,"sha256":"{}"}}"#,
                    hex(8)
                ),
            )],
        );
        let authority = authority_for(&bytes);
        assert_eq!(
            authority.admit_catalog(&bytes).unwrap_err(),
            BundledCatalogAdmissionError::UnsupportedPayload
        );
    }

    #[test]
    fn malformed_noncanonical_and_package_ordering_are_rejected() {
        let malformed = br#"{"schema_version":1"#.to_vec();
        let mut anchor = anchor_for(&catalog_bytes());
        anchor.catalog_length = malformed.len();
        anchor.catalog_digest =
            ExtensionReleaseCatalogDigest::from_bytes(Sha256::digest(&malformed).into());
        let authority =
            BundledPackageAuthority::from_sealed_parts(anchor, policy(POLICY_BYTE)).unwrap();
        assert!(matches!(
            authority.admit_catalog(&malformed),
            Err(BundledCatalogAdmissionError::Catalog(_))
        ));

        let mut noncanonical = catalog_bytes();
        noncanonical.push(b'\n');
        let mut anchor = anchor_for(&catalog_bytes());
        anchor.catalog_length = noncanonical.len();
        anchor.catalog_digest =
            ExtensionReleaseCatalogDigest::from_bytes(Sha256::digest(&noncanonical).into());
        let authority =
            BundledPackageAuthority::from_sealed_parts(anchor, policy(POLICY_BYTE)).unwrap();
        assert_eq!(
            authority.admit_catalog(&noncanonical).unwrap_err(),
            BundledCatalogAdmissionError::Catalog(ExtensionReleaseCatalogError::NonCanonical)
        );

        let unordered = catalog_bytes_with(
            AUTHORITY_BYTE,
            1,
            POLICY_BYTE,
            &[
                package_json(8, r#"{"kind":"bundled_tree"}"#),
                package_json(7, r#"{"kind":"bundled_tree"}"#),
            ],
        );
        let mut anchor = anchor_for(&catalog_bytes());
        anchor.catalog_length = unordered.len();
        anchor.catalog_digest =
            ExtensionReleaseCatalogDigest::from_bytes(Sha256::digest(&unordered).into());
        let authority =
            BundledPackageAuthority::from_sealed_parts(anchor, policy(POLICY_BYTE)).unwrap();
        assert_eq!(
            authority.admit_catalog(&unordered).unwrap_err(),
            BundledCatalogAdmissionError::Catalog(
                ExtensionReleaseCatalogError::NonCanonicalPackageOrder
            )
        );
    }

    #[test]
    fn checkpoint_dispositions_cover_rollback_equivocation_replay_and_candidate() {
        let bytes = catalog_bytes();
        let admitted = authority_for(&bytes).admit_catalog(&bytes).unwrap();
        let exact = admitted.checkpoint();
        assert_eq!(
            admitted.disposition_against(Some(&exact)),
            BundledCatalogDisposition::IdempotentReplay
        );

        let rollback_floor = BundledCatalogCheckpoint::from_parts(
            admitted.authority(),
            ExtensionReleaseCatalogRevision::new(2).unwrap(),
            admitted.catalog_digest(),
            admitted.inventory_digest(),
        );
        assert_eq!(
            admitted.disposition_against(Some(&rollback_floor)),
            BundledCatalogDisposition::Rollback
        );

        let equivocation_floor = BundledCatalogCheckpoint::from_parts(
            admitted.authority(),
            admitted.revision(),
            ExtensionReleaseCatalogDigest::from_bytes([9; 32]),
            admitted.inventory_digest(),
        );
        assert_eq!(
            admitted.disposition_against(Some(&equivocation_floor)),
            BundledCatalogDisposition::Equivocation
        );

        let older_floor = BundledCatalogCheckpoint::from_parts(
            admitted.authority(),
            ExtensionReleaseCatalogRevision::new(1).unwrap(),
            admitted.catalog_digest(),
            admitted.inventory_digest(),
        );
        let higher_bytes = catalog_bytes_with(
            AUTHORITY_BYTE,
            2,
            POLICY_BYTE,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#)],
        );
        let higher = authority_for(&higher_bytes)
            .admit_catalog(&higher_bytes)
            .unwrap();
        assert_eq!(
            higher.disposition_against(Some(&older_floor)),
            BundledCatalogDisposition::Candidate
        );
    }

    #[test]
    fn inventory_digest_has_a_fixed_cross_version_golden() {
        let bytes = catalog_bytes();
        let catalog = ExtensionReleaseCatalog::parse_canonical(&bytes).unwrap();
        assert_eq!(
            digest_catalog_inventory(&catalog).unwrap().bytes(),
            [
                151, 140, 108, 241, 9, 163, 44, 218, 18, 59, 84, 175, 161, 154, 205, 176, 92, 123,
                173, 8, 156, 66, 140, 159, 177, 224, 188, 5, 38, 75, 238, 12,
            ]
        );
    }

    #[test]
    fn inventory_golden_covers_optional_fields_and_multiple_rows() {
        let optional_package = package_json(8, r#"{"kind":"bundled_tree"}"#)
            .replace(
                r#""chromium":null"#,
                &format!(
                    r#""chromium":{{"manifest_key_sha256":"{}"}}"#,
                    hex(9)
                ),
            )
            .replace(
                r#""corresponding_source":null"#,
                &format!(
                    r#""corresponding_source":{{"url":"https://example.com/source/{}","revision":"{}"}}"#,
                    "a".repeat(40),
                    "a".repeat(40),
                ),
            );
        let bytes = catalog_bytes_with(
            AUTHORITY_BYTE,
            1,
            POLICY_BYTE,
            &[
                package_json(7, r#"{"kind":"bundled_tree"}"#),
                optional_package,
            ],
        );
        let catalog = ExtensionReleaseCatalog::parse_canonical(&bytes).unwrap();
        assert_eq!(
            digest_catalog_inventory(&catalog).unwrap().bytes(),
            [
                104, 37, 211, 145, 129, 149, 144, 165, 0, 41, 202, 15, 147, 100, 176, 103, 75, 93,
                75, 75, 162, 83, 75, 248, 75, 106, 58, 82, 151, 88, 251, 163,
            ]
        );
    }
}
