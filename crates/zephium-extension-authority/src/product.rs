//! Product-sealed active and explicitly approved rollback catalog admission.

use std::fmt;
use std::mem::size_of;

use sha2::{Digest, Sha256};
use zephium_core::extensions::{ExtensionAuthorityId, ExtensionPackagePayloadIdentity};
#[cfg(zephium_internal_repository_e2e)]
use zephium_extension_package::{CanonicalExtensionTreeIndex, ExtensionReleaseLicenseRule};
use zephium_extension_package::{
    ExtensionPackageAdmissionPolicyDigest, ExtensionReleaseAdmissionPolicy,
    ExtensionReleaseCatalog, ExtensionReleaseCatalogDigest, ExtensionReleaseCatalogRevision,
    MAX_EXTENSION_RELEASE_CATALOG_BYTES, MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES,
};

use crate::inventory::digest_catalog_inventory;
#[cfg(zephium_internal_repository_e2e)]
use crate::repository_e2e_fixture::{
    ADMISSION_POLICY_DIGEST_BYTES, CATALOG_INVENTORY_SHA256_HEX, LEGAL_NOTICE_BYTES,
    LEGAL_NOTICE_LENGTH, LEGAL_NOTICE_SHA256_HEX, LICENSE_EXPRESSION, MANIFEST_BYTES,
    MANIFEST_LENGTH, MANIFEST_SHA256_HEX, PRODUCT_ACTIVE_CATALOG_BYTES,
    PRODUCT_ACTIVE_CATALOG_INVENTORY_SHA256_HEX, PRODUCT_ACTIVE_CATALOG_LENGTH,
    PRODUCT_ACTIVE_CATALOG_SHA256_HEX, PRODUCT_ACTIVE_MANIFEST_BYTES,
    PRODUCT_ACTIVE_MANIFEST_LENGTH, PRODUCT_ACTIVE_MANIFEST_SHA256_HEX,
    PRODUCT_ACTIVE_TREE_INDEX_BYTES, PRODUCT_ACTIVE_TREE_INDEX_LENGTH,
    PRODUCT_ACTIVE_TREE_INDEX_SHA256_HEX, PRODUCT_ACTIVE_TREE_SHA256_HEX, ROLLBACK_CATALOG_BYTES,
    ROLLBACK_CATALOG_LENGTH, ROLLBACK_CATALOG_SHA256_HEX, TREE_INDEX_BYTES, TREE_INDEX_LENGTH,
    TREE_INDEX_SHA256_HEX, TREE_SHA256_HEX,
};
use crate::{
    BundledCatalogAdmissionError, BundledCatalogCheckpoint, BundledCatalogDisposition,
    BundledCatalogGenerationAnchor, BundledCatalogInventoryDigest,
};

// Stable logical reserve for the witness fields, allocator indirection, and
// future fixed-size metadata. Any newly owned allocation must be charged
// separately; the compile-time assertion below prevents the wrapper itself
// from silently outgrowing this reserve.
const ADMITTED_CATALOG_ACCOUNTING_OVERHEAD: usize = 256;
const PACKAGE_AUTHORITY_ACCOUNTING_OVERHEAD: usize = 512;
const CATALOG_GENERATION_ACCOUNTING_OVERHEAD: usize = 256;

/// Maximum explicitly approved rollback catalogs retained by one product build.
pub const MAX_PRODUCT_ROLLBACK_BUNDLED_CATALOGS: usize = 2;

/// Maximum active and rollback bundled-catalog generations in one product build.
pub const MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS: usize =
    1 + MAX_PRODUCT_ROLLBACK_BUNDLED_CATALOGS;

/// Maximum logical memory retained by the product bundled-catalog authority.
///
/// The per-generation reserve is derived from the parser's exact catalog-byte
/// ceiling. In practice the authority retains only bounded policy rows and
/// fixed-size anchors, but charging the larger parser ceiling keeps this bound
/// valid if the policy representation grows without silently making the
/// product authority unbounded.
pub const MAX_BUNDLED_PACKAGE_AUTHORITY_RETAINED_BYTES: usize =
    PACKAGE_AUTHORITY_ACCOUNTING_OVERHEAD
        + MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS
            * (MAX_EXTENSION_RELEASE_CATALOG_BYTES + CATALOG_GENERATION_ACCOUNTING_OVERHEAD);

/// Maximum logical memory retained by one admitted bundled-catalog witness.
pub const MAX_ADMITTED_BUNDLED_CATALOG_RETAINED_BYTES: usize =
    MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES + ADMITTED_CATALOG_ACCOUNTING_OVERHEAD;

/// Maximum logical memory retained by one admitted acquired-catalog witness.
pub const MAX_ADMITTED_ACQUIRED_CATALOG_RETAINED_BYTES: usize =
    MAX_ADMITTED_BUNDLED_CATALOG_RETAINED_BYTES;

/// Maximum logical memory retained by one admitted rollback-catalog witness.
pub const MAX_ADMITTED_ROLLBACK_BUNDLED_CATALOG_RETAINED_BYTES: usize =
    MAX_ADMITTED_BUNDLED_CATALOG_RETAINED_BYTES;

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

/// Product-sealed role of an exact recognized bundled-catalog generation.
///
/// This is a classification result, not an admission, repository-recording,
/// materialization, or activation capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "active and rollback catalog generations have different authority"]
pub enum ProductBundledCatalogGenerationRole {
    /// The only generation allowed to mint a monotonic repository witness.
    Active,
    /// An explicitly approved generation usable only through rollback APIs.
    Rollback,
}

/// Product-owned authentication boundary for release-bundled catalog metadata.
///
/// This type has no public anchor, policy, or trust-provider constructor. The
/// only production constructor reads product-sealed compile-time state. Its
/// legacy name describes where the catalog trust root lives, not where every
/// package payload lives: acquired payload rows require the distinct
/// [`Self::admit_acquired_catalog`] operation and witness.
pub struct BundledPackageAuthority {
    active: SealedBundledCatalogGeneration,
    rollback: Box<[SealedBundledCatalogGeneration]>,
    retained_bytes: usize,
}

impl BundledPackageAuthority {
    /// Opens the product-sealed authority or explicitly reports that this
    /// build has no approved extension catalog anchor.
    pub fn product() -> Result<Self, BundledCatalogAdmissionError> {
        let (active, rollback) = sealed_product_bundled_catalog_generations()?
            .ok_or(BundledCatalogAdmissionError::Unprovisioned)?;
        Self::from_sealed_generations(active, rollback)
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
        self.active
            .admit(catalog_bytes, CatalogPayloadClass::BundledTree)
            .map(|data| AdmittedBundledCatalog { data })
    }

    /// Authenticates an exact active catalog and preserves its payload class.
    ///
    /// Unlike probing [`Self::admit_catalog`] and
    /// [`Self::admit_acquired_catalog`] in sequence, this operation checks the
    /// sealed length and digest and parses the canonical catalog exactly once.
    /// The returned enum retains the nominal witness boundary: callers must
    /// still handle bundled trees and acquired CRX packages through their
    /// distinct materializers.
    pub fn admit_active_catalog(
        &self,
        catalog_bytes: &[u8],
    ) -> Result<AdmittedActiveCatalog, BundledCatalogAdmissionError> {
        let (data, payload_class) = self.active.admit_classified(catalog_bytes)?;
        Ok(match payload_class {
            CatalogPayloadClass::BundledTree => {
                AdmittedActiveCatalog::Bundled(AdmittedBundledCatalog { data })
            }
            CatalogPayloadClass::AcquiredZip => {
                AdmittedActiveCatalog::Acquired(AdmittedAcquiredCatalog { data })
            }
        })
    }

    /// Authenticates one exact active catalog of acquired CRX3 packages.
    ///
    /// This uses the same product-sealed active generation and license policy
    /// as bundled metadata, but returns a nominally distinct witness that no
    /// bundled-tree materializer can consume. Every package must name an exact
    /// `AcquiredZip` payload and a complete expected Chromium key identity.
    pub fn admit_acquired_catalog(
        &self,
        catalog_bytes: &[u8],
    ) -> Result<AdmittedAcquiredCatalog, BundledCatalogAdmissionError> {
        self.active
            .admit(catalog_bytes, CatalogPayloadClass::AcquiredZip)
            .map(|data| AdmittedAcquiredCatalog { data })
    }

    /// Authenticates one exact explicitly approved rollback catalog.
    ///
    /// Rollback admission is deliberately a different operation and returns a
    /// different non-forgeable witness. That witness has no checkpoint or
    /// monotonic-disposition projection, so it cannot lower the repository's
    /// durable catalog high-water through the ordinary record API.
    pub fn admit_rollback_catalog(
        &self,
        catalog_bytes: &[u8],
    ) -> Result<AdmittedRollbackBundledCatalog, BundledCatalogAdmissionError> {
        if self.rollback.is_empty() {
            return Err(BundledCatalogAdmissionError::RollbackCatalogNotProvisioned);
        }
        if !self
            .rollback
            .iter()
            .any(|generation| generation.anchor.catalog_length == catalog_bytes.len())
        {
            return Err(BundledCatalogAdmissionError::CatalogLengthMismatch);
        }
        let observed_digest =
            ExtensionReleaseCatalogDigest::from_bytes(Sha256::digest(catalog_bytes).into());
        let generation = self
            .rollback
            .iter()
            .find(|generation| {
                generation.anchor.catalog_length == catalog_bytes.len()
                    && generation.anchor.catalog_digest == observed_digest
            })
            .ok_or(BundledCatalogAdmissionError::CatalogDigestMismatch)?;
        generation
            .admit_prehashed(
                catalog_bytes,
                observed_digest,
                CatalogPayloadClass::BundledTree,
            )
            .map(|data| AdmittedRollbackBundledCatalog {
                data,
                _seal: RollbackCatalogWitnessSeal(()),
            })
    }

    /// Returns the authority's explicit logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Recognizes exact recovered metadata against the sealed generation set.
    ///
    /// All persisted fields, including exact canonical byte length and both
    /// digests, must match. The returned role remains non-authoritative; exact
    /// bytes still require [`Self::admit_catalog`] or
    /// [`Self::admit_rollback_catalog`] before use.
    pub fn recognize_generation(
        &self,
        candidate: &BundledCatalogGenerationAnchor,
    ) -> Option<ProductBundledCatalogGenerationRole> {
        if self.active.anchor.matches_structural(candidate) {
            return Some(ProductBundledCatalogGenerationRole::Active);
        }
        self.rollback
            .iter()
            .any(|generation| generation.anchor.matches_structural(candidate))
            .then_some(ProductBundledCatalogGenerationRole::Rollback)
    }

    #[cfg(test)]
    fn from_sealed_parts(
        anchor: SealedBundledCatalogAnchor,
        policy: ExtensionReleaseAdmissionPolicy,
    ) -> Result<Self, BundledCatalogAdmissionError> {
        Self::from_sealed_generations(
            SealedBundledCatalogGeneration { anchor, policy },
            Box::new([]),
        )
    }

    fn from_sealed_generations(
        active: SealedBundledCatalogGeneration,
        rollback: Box<[SealedBundledCatalogGeneration]>,
    ) -> Result<Self, BundledCatalogAdmissionError> {
        if rollback.len() > MAX_PRODUCT_ROLLBACK_BUNDLED_CATALOGS {
            return Err(BundledCatalogAdmissionError::InvalidProductConfiguration);
        }
        active.validate_configuration()?;
        for generation in &rollback {
            generation.validate_configuration()?;
        }
        if rollback
            .windows(2)
            .any(|pair| pair[0].anchor.catalog_revision >= pair[1].anchor.catalog_revision)
            || rollback.iter().any(|generation| {
                generation.anchor.authority != active.anchor.authority
                    || generation.anchor.catalog_revision >= active.anchor.catalog_revision
            })
        {
            return Err(BundledCatalogAdmissionError::InvalidProductConfiguration);
        }
        if rollback.iter().enumerate().any(|(index, generation)| {
            generation.anchor.catalog_digest == active.anchor.catalog_digest
                || rollback[index + 1..]
                    .iter()
                    .any(|later| later.anchor.catalog_digest == generation.anchor.catalog_digest)
        }) {
            return Err(BundledCatalogAdmissionError::InvalidProductConfiguration);
        }

        let retained_bytes = rollback.iter().try_fold(
            PACKAGE_AUTHORITY_ACCOUNTING_OVERHEAD
                .checked_add(active.retained_bytes()?)
                .ok_or(BundledCatalogAdmissionError::AccountingOverflow)?,
            |total, generation| {
                total
                    .checked_add(generation.retained_bytes()?)
                    .ok_or(BundledCatalogAdmissionError::AccountingOverflow)
            },
        )?;
        if retained_bytes > MAX_BUNDLED_PACKAGE_AUTHORITY_RETAINED_BYTES {
            return Err(BundledCatalogAdmissionError::InvalidProductConfiguration);
        }
        Ok(Self {
            active,
            rollback,
            retained_bytes,
        })
    }

    #[cfg(test)]
    pub(crate) fn admit_fixture_catalog(
        catalog_bytes: &[u8],
        policy: ExtensionReleaseAdmissionPolicy,
    ) -> Result<AdmittedBundledCatalog, BundledCatalogAdmissionError> {
        let catalog = ExtensionReleaseCatalog::parse_canonical(catalog_bytes)
            .map_err(BundledCatalogAdmissionError::Catalog)?;
        let anchor = SealedBundledCatalogAnchor {
            catalog_length: catalog_bytes.len(),
            catalog_digest: ExtensionReleaseCatalogDigest::from_bytes(
                Sha256::digest(catalog_bytes).into(),
            ),
            authority: catalog.authority(),
            catalog_revision: catalog.revision(),
            admission_policy_digest: catalog.admission_policy_sha256(),
            inventory_digest: digest_catalog_inventory(&catalog)
                .ok_or(BundledCatalogAdmissionError::AccountingOverflow)?,
        };
        Self::from_sealed_parts(anchor, policy)?.admit_catalog(catalog_bytes)
    }

    #[cfg(test)]
    pub(crate) fn admit_fixture_acquired_catalog(
        catalog_bytes: &[u8],
        policy: ExtensionReleaseAdmissionPolicy,
    ) -> Result<AdmittedAcquiredCatalog, BundledCatalogAdmissionError> {
        let catalog = ExtensionReleaseCatalog::parse_canonical(catalog_bytes)
            .map_err(BundledCatalogAdmissionError::Catalog)?;
        let anchor = SealedBundledCatalogAnchor {
            catalog_length: catalog_bytes.len(),
            catalog_digest: ExtensionReleaseCatalogDigest::from_bytes(
                Sha256::digest(catalog_bytes).into(),
            ),
            authority: catalog.authority(),
            catalog_revision: catalog.revision(),
            admission_policy_digest: catalog.admission_policy_sha256(),
            inventory_digest: digest_catalog_inventory(&catalog)
                .ok_or(BundledCatalogAdmissionError::AccountingOverflow)?,
        };
        Self::from_sealed_parts(anchor, policy)?.admit_acquired_catalog(catalog_bytes)
    }

    #[cfg(test)]
    pub(crate) fn admit_fixture_rollback_catalog(
        active_catalog_bytes: &[u8],
        active_policy: ExtensionReleaseAdmissionPolicy,
        rollback_catalog_bytes: &[u8],
        rollback_policy: ExtensionReleaseAdmissionPolicy,
    ) -> Result<AdmittedRollbackBundledCatalog, BundledCatalogAdmissionError> {
        let active = SealedBundledCatalogGeneration {
            anchor: anchor_for_fixture(active_catalog_bytes)?,
            policy: active_policy,
        };
        let rollback = SealedBundledCatalogGeneration {
            anchor: anchor_for_fixture(rollback_catalog_bytes)?,
            policy: rollback_policy,
        };
        Self::from_sealed_generations(active, vec![rollback].into_boxed_slice())?
            .admit_rollback_catalog(rollback_catalog_bytes)
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
    data: AdmittedCatalogData,
}

pub(crate) struct AdmittedCatalogData {
    catalog: ExtensionReleaseCatalog,
    catalog_length: u64,
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
            .field("catalog_length", &self.catalog_length())
            .field("catalog_digest", &self.catalog_digest())
            .field("inventory_digest", &self.inventory_digest())
            .field("package_count", &self.catalog().packages().len())
            .field("retained_bytes", &self.retained_bytes())
            .finish()
    }
}

impl AdmittedBundledCatalog {
    /// Returns authenticated, structurally parsed catalog metadata.
    pub const fn catalog(&self) -> &ExtensionReleaseCatalog {
        &self.data.catalog
    }

    /// Returns the trust-domain and epoch identity.
    pub const fn authority(&self) -> ExtensionAuthorityId {
        self.data.catalog.authority()
    }

    /// Returns the authenticated catalog revision.
    pub const fn revision(&self) -> ExtensionReleaseCatalogRevision {
        self.data.catalog.revision()
    }

    /// Returns SHA-256 of exact authenticated canonical catalog bytes.
    pub const fn catalog_digest(&self) -> ExtensionReleaseCatalogDigest {
        self.data.catalog.digest()
    }

    /// Returns the exact authenticated canonical catalog byte length.
    pub const fn catalog_length(&self) -> u64 {
        self.data.catalog_length
    }

    /// Returns the redundant deterministic package-inventory digest.
    pub const fn inventory_digest(&self) -> BundledCatalogInventoryDigest {
        self.data.inventory_digest
    }

    /// Returns the explicit logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.data.retained_bytes
    }

    /// Creates the structural checkpoint projection suitable for durable state.
    ///
    /// Persisting it and resolving commit ambiguity are outside this crate.
    pub const fn checkpoint(&self) -> BundledCatalogCheckpoint {
        BundledCatalogCheckpoint::from_parts(
            self.authority(),
            self.revision(),
            self.catalog_digest(),
            self.inventory_digest(),
        )
    }

    /// Returns the exact structural generation projection for durable metadata.
    ///
    /// This projection is not an admission or activation capability.
    pub const fn generation_anchor(&self) -> BundledCatalogGenerationAnchor {
        BundledCatalogGenerationAnchor::from_parts(
            self.authority(),
            self.revision(),
            self.catalog_length(),
            self.catalog_digest(),
            self.inventory_digest(),
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

    pub(crate) const fn data(&self) -> &AdmittedCatalogData {
        &self.data
    }
}

/// Authenticated metadata for one active catalog of acquired CRX3 packages.
///
/// The exact catalog is still product-sealed; only package payload bytes are
/// acquired on demand. This witness is nominally distinct from
/// [`AdmittedBundledCatalog`], non-serializable, non-cloneable, and grants no
/// network, archive, repository, profile, or native-runtime authority.
///
/// ```compile_fail
/// use zephium_extension_authority::AdmittedAcquiredCatalog;
/// fn require_clone<T: Clone>() {}
/// fn duplicate() {
///     require_clone::<AdmittedAcquiredCatalog>();
/// }
/// ```
#[must_use = "admitted acquired metadata must be checkpointed or deliberately discarded"]
pub struct AdmittedAcquiredCatalog {
    data: AdmittedCatalogData,
}

/// Authenticated metadata for one active catalog, classified by payload kind.
///
/// This is a closed dispatch witness, not a shared materialization capability.
/// Its variants deliberately retain the nominal bundled-tree and acquired-CRX
/// witness types so one representation cannot be passed to the other's byte
/// source or materializer.
///
/// ```compile_fail
/// use zephium_extension_authority::AdmittedActiveCatalog;
/// fn require_clone<T: Clone>() {}
/// fn duplicate() {
///     require_clone::<AdmittedActiveCatalog>();
/// }
/// ```
#[derive(Debug)]
#[must_use = "admitted active metadata must be handled or deliberately discarded"]
pub enum AdmittedActiveCatalog {
    /// An active catalog whose packages are product-bundled canonical trees.
    Bundled(AdmittedBundledCatalog),
    /// An active catalog whose packages are acquired authenticated CRX files.
    Acquired(AdmittedAcquiredCatalog),
}

const _: () = assert!(size_of::<AdmittedActiveCatalog>() <= ADMITTED_CATALOG_ACCOUNTING_OVERHEAD);

impl AdmittedActiveCatalog {
    /// Returns authenticated, structurally parsed catalog metadata.
    pub const fn catalog(&self) -> &ExtensionReleaseCatalog {
        match self {
            Self::Bundled(catalog) => catalog.catalog(),
            Self::Acquired(catalog) => catalog.catalog(),
        }
    }

    /// Returns the exact structural generation projection for durable metadata.
    pub const fn generation_anchor(&self) -> BundledCatalogGenerationAnchor {
        match self {
            Self::Bundled(catalog) => catalog.generation_anchor(),
            Self::Acquired(catalog) => catalog.generation_anchor(),
        }
    }
}

const _: () =
    assert!(std::mem::size_of::<AdmittedAcquiredCatalog>() <= ADMITTED_CATALOG_ACCOUNTING_OVERHEAD);

impl fmt::Debug for AdmittedAcquiredCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmittedAcquiredCatalog")
            .field("authority", &self.authority())
            .field("revision", &self.revision())
            .field("catalog_length", &self.catalog_length())
            .field("catalog_digest", &self.catalog_digest())
            .field("inventory_digest", &self.inventory_digest())
            .field("package_count", &self.catalog().packages().len())
            .field("retained_bytes", &self.retained_bytes())
            .finish()
    }
}

impl AdmittedAcquiredCatalog {
    /// Returns authenticated, structurally parsed catalog metadata.
    pub const fn catalog(&self) -> &ExtensionReleaseCatalog {
        &self.data.catalog
    }

    /// Returns the trust-domain and epoch identity.
    pub const fn authority(&self) -> ExtensionAuthorityId {
        self.data.catalog.authority()
    }

    /// Returns the authenticated catalog revision.
    pub const fn revision(&self) -> ExtensionReleaseCatalogRevision {
        self.data.catalog.revision()
    }

    /// Returns SHA-256 of exact authenticated canonical catalog bytes.
    pub const fn catalog_digest(&self) -> ExtensionReleaseCatalogDigest {
        self.data.catalog.digest()
    }

    /// Returns the exact authenticated canonical catalog byte length.
    pub const fn catalog_length(&self) -> u64 {
        self.data.catalog_length
    }

    /// Returns the redundant deterministic package-inventory digest.
    pub const fn inventory_digest(&self) -> BundledCatalogInventoryDigest {
        self.data.inventory_digest
    }

    /// Returns the explicit logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.data.retained_bytes
    }

    /// Creates the structural checkpoint projection suitable for durable state.
    pub const fn checkpoint(&self) -> BundledCatalogCheckpoint {
        BundledCatalogCheckpoint::from_parts(
            self.authority(),
            self.revision(),
            self.catalog_digest(),
            self.inventory_digest(),
        )
    }

    /// Returns the exact structural generation projection for durable metadata.
    pub const fn generation_anchor(&self) -> BundledCatalogGenerationAnchor {
        BundledCatalogGenerationAnchor::from_parts(
            self.authority(),
            self.revision(),
            self.catalog_length(),
            self.catalog_digest(),
            self.inventory_digest(),
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

    pub(crate) const fn data(&self) -> &AdmittedCatalogData {
        &self.data
    }
}

/// Authenticated metadata for one explicitly approved rollback catalog.
///
/// This capability is deliberately distinct from [`AdmittedBundledCatalog`].
/// It has no durable checkpoint or monotonic-disposition API and cannot be
/// passed to the repository's active-catalog recording method. It is also
/// non-serializable, non-cloneable, and has no public constructor.
///
/// ```compile_fail
/// use zephium_extension_authority::AdmittedRollbackBundledCatalog;
/// fn require_clone<T: Clone>() {}
/// fn duplicate() {
///     require_clone::<AdmittedRollbackBundledCatalog>();
/// }
/// ```
///
/// ```compile_fail
/// use zephium_extension_authority::AdmittedRollbackBundledCatalog;
/// fn forge() -> AdmittedRollbackBundledCatalog {
///     AdmittedRollbackBundledCatalog {}
/// }
/// ```
///
/// ```compile_fail
/// use zephium_extension_authority::AdmittedRollbackBundledCatalog;
/// fn lower_high_water(rollback: &AdmittedRollbackBundledCatalog) {
///     let _ = rollback.checkpoint();
/// }
/// ```
#[must_use = "rollback metadata must be materialized for explicit recovery or discarded"]
pub struct AdmittedRollbackBundledCatalog {
    data: AdmittedCatalogData,
    _seal: RollbackCatalogWitnessSeal,
}

impl fmt::Debug for AdmittedRollbackBundledCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmittedRollbackBundledCatalog")
            .field("authority", &self.authority())
            .field("revision", &self.revision())
            .field("catalog_length", &self.catalog_length())
            .field("catalog_digest", &self.catalog_digest())
            .field("inventory_digest", &self.inventory_digest())
            .field("package_count", &self.catalog().packages().len())
            .field("retained_bytes", &self.retained_bytes())
            .finish()
    }
}

impl AdmittedRollbackBundledCatalog {
    /// Returns authenticated, structurally parsed rollback-catalog metadata.
    pub const fn catalog(&self) -> &ExtensionReleaseCatalog {
        &self.data.catalog
    }

    /// Returns the shared trust-domain and epoch identity.
    pub const fn authority(&self) -> ExtensionAuthorityId {
        self.data.catalog.authority()
    }

    /// Returns the exact explicitly approved rollback revision.
    pub const fn revision(&self) -> ExtensionReleaseCatalogRevision {
        self.data.catalog.revision()
    }

    /// Returns SHA-256 of exact authenticated canonical rollback-catalog bytes.
    pub const fn catalog_digest(&self) -> ExtensionReleaseCatalogDigest {
        self.data.catalog.digest()
    }

    /// Returns the exact authenticated canonical rollback-catalog byte length.
    pub const fn catalog_length(&self) -> u64 {
        self.data.catalog_length
    }

    /// Returns the deterministic package-inventory digest.
    pub const fn inventory_digest(&self) -> BundledCatalogInventoryDigest {
        self.data.inventory_digest
    }

    /// Returns the explicit logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.data.retained_bytes
    }

    /// Returns the exact structural rollback-generation projection.
    ///
    /// This projection has no monotonic checkpoint authority.
    pub const fn generation_anchor(&self) -> BundledCatalogGenerationAnchor {
        BundledCatalogGenerationAnchor::from_parts(
            self.authority(),
            self.revision(),
            self.catalog_length(),
            self.catalog_digest(),
            self.inventory_digest(),
        )
    }

    pub(crate) const fn data(&self) -> &AdmittedCatalogData {
        &self.data
    }
}

impl AdmittedCatalogData {
    pub(crate) const fn catalog(&self) -> &ExtensionReleaseCatalog {
        &self.catalog
    }

    pub(crate) const fn authority(&self) -> ExtensionAuthorityId {
        self.catalog.authority()
    }

    pub(crate) const fn revision(&self) -> ExtensionReleaseCatalogRevision {
        self.catalog.revision()
    }

    pub(crate) const fn catalog_digest(&self) -> ExtensionReleaseCatalogDigest {
        self.catalog.digest()
    }

    pub(crate) const fn catalog_length(&self) -> u64 {
        self.catalog_length
    }

    pub(crate) const fn inventory_digest(&self) -> BundledCatalogInventoryDigest {
        self.inventory_digest
    }
}

struct RollbackCatalogWitnessSeal(());

#[derive(Clone, Copy)]
struct SealedBundledCatalogAnchor {
    catalog_length: usize,
    catalog_digest: ExtensionReleaseCatalogDigest,
    authority: ExtensionAuthorityId,
    catalog_revision: ExtensionReleaseCatalogRevision,
    admission_policy_digest: ExtensionPackageAdmissionPolicyDigest,
    inventory_digest: BundledCatalogInventoryDigest,
}

impl SealedBundledCatalogAnchor {
    fn matches_structural(&self, candidate: &BundledCatalogGenerationAnchor) -> bool {
        u64::try_from(self.catalog_length).is_ok_and(|length| {
            candidate.authority() == self.authority
                && candidate.revision() == self.catalog_revision
                && candidate.catalog_length() == length
                && candidate.catalog_digest() == self.catalog_digest
                && candidate.inventory_digest() == self.inventory_digest
        })
    }
}

struct SealedBundledCatalogGeneration {
    anchor: SealedBundledCatalogAnchor,
    policy: ExtensionReleaseAdmissionPolicy,
}

type SealedBundledCatalogGenerations = (
    SealedBundledCatalogGeneration,
    Box<[SealedBundledCatalogGeneration]>,
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CatalogPayloadClass {
    BundledTree,
    AcquiredZip,
}

impl SealedBundledCatalogGeneration {
    fn validate_configuration(&self) -> Result<(), BundledCatalogAdmissionError> {
        if self.anchor.catalog_length == 0
            || self.anchor.catalog_length > MAX_EXTENSION_RELEASE_CATALOG_BYTES
            || self.anchor.admission_policy_digest != self.policy.digest()
        {
            return Err(BundledCatalogAdmissionError::InvalidProductConfiguration);
        }
        Ok(())
    }

    fn retained_bytes(&self) -> Result<usize, BundledCatalogAdmissionError> {
        self.policy.license_rules().iter().try_fold(
            size_of::<Self>()
                .checked_add(CATALOG_GENERATION_ACCOUNTING_OVERHEAD)
                .ok_or(BundledCatalogAdmissionError::AccountingOverflow)?,
            |total, rule| {
                total
                    .checked_add(size_of_val(rule))
                    .and_then(|bytes| bytes.checked_add(rule.expression().len()))
                    .ok_or(BundledCatalogAdmissionError::AccountingOverflow)
            },
        )
    }

    fn admit(
        &self,
        catalog_bytes: &[u8],
        payload_class: CatalogPayloadClass,
    ) -> Result<AdmittedCatalogData, BundledCatalogAdmissionError> {
        if catalog_bytes.len() != self.anchor.catalog_length {
            return Err(BundledCatalogAdmissionError::CatalogLengthMismatch);
        }
        let observed_digest =
            ExtensionReleaseCatalogDigest::from_bytes(Sha256::digest(catalog_bytes).into());
        self.admit_prehashed(catalog_bytes, observed_digest, payload_class)
    }

    fn admit_classified(
        &self,
        catalog_bytes: &[u8],
    ) -> Result<(AdmittedCatalogData, CatalogPayloadClass), BundledCatalogAdmissionError> {
        if catalog_bytes.len() != self.anchor.catalog_length {
            return Err(BundledCatalogAdmissionError::CatalogLengthMismatch);
        }
        let observed_digest =
            ExtensionReleaseCatalogDigest::from_bytes(Sha256::digest(catalog_bytes).into());
        let data = self.admit_common_prehashed(catalog_bytes, observed_digest)?;
        let payload_class = classify_catalog_payload(&data.catalog)
            .ok_or(BundledCatalogAdmissionError::UnsupportedPayload)?;
        Ok((data, payload_class))
    }

    fn admit_prehashed(
        &self,
        catalog_bytes: &[u8],
        observed_digest: ExtensionReleaseCatalogDigest,
        payload_class: CatalogPayloadClass,
    ) -> Result<AdmittedCatalogData, BundledCatalogAdmissionError> {
        let data = self.admit_common_prehashed(catalog_bytes, observed_digest)?;
        if !catalog_payload_matches(&data.catalog, payload_class) {
            return Err(BundledCatalogAdmissionError::UnsupportedPayload);
        }
        Ok(data)
    }

    fn admit_common_prehashed(
        &self,
        catalog_bytes: &[u8],
        observed_digest: ExtensionReleaseCatalogDigest,
    ) -> Result<AdmittedCatalogData, BundledCatalogAdmissionError> {
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
        Ok(AdmittedCatalogData {
            catalog,
            catalog_length: u64::try_from(catalog_bytes.len())
                .map_err(|_| BundledCatalogAdmissionError::AccountingOverflow)?,
            inventory_digest,
            retained_bytes,
        })
    }
}

fn catalog_payload_matches(
    catalog: &ExtensionReleaseCatalog,
    payload_class: CatalogPayloadClass,
) -> bool {
    catalog
        .packages()
        .iter()
        .all(|package| match payload_class {
            CatalogPayloadClass::BundledTree => {
                package.payload() == ExtensionPackagePayloadIdentity::BundledTree
            }
            CatalogPayloadClass::AcquiredZip => {
                matches!(
                    package.payload(),
                    ExtensionPackagePayloadIdentity::AcquiredZip { .. }
                ) && package.chromium().is_some()
            }
        })
}

fn classify_catalog_payload(catalog: &ExtensionReleaseCatalog) -> Option<CatalogPayloadClass> {
    // Empty catalogs have no acquired bytes and retain the historical bundled
    // classification. Durable catalog sets require at least one selected
    // package, but keeping this deterministic also preserves existing catalog
    // admission behavior for metadata-only empty generations.
    let Some(first) = catalog.packages().first() else {
        return Some(CatalogPayloadClass::BundledTree);
    };
    let payload_class = match first.payload() {
        ExtensionPackagePayloadIdentity::BundledTree => CatalogPayloadClass::BundledTree,
        ExtensionPackagePayloadIdentity::AcquiredZip { .. } => CatalogPayloadClass::AcquiredZip,
    };
    catalog_payload_matches(catalog, payload_class).then_some(payload_class)
}

// Deliberately absent until exact approved package, license, corresponding
// source, and redistribution artifacts are bound into a signed Zephium build.
// This private compile-time slot is the only production trust root; it must
// never be populated from runtime configuration, an environment variable, or
// caller-provided bytes.
#[cfg(not(zephium_internal_repository_e2e))]
fn sealed_product_bundled_catalog_generations(
) -> Result<Option<SealedBundledCatalogGenerations>, BundledCatalogAdmissionError> {
    // Active and rollback generations, including each exact per-generation
    // policy, must land atomically once reviewed release artifacts exist.
    // Returning `None` preserves an explicit fail-closed production build.
    Ok(None)
}

#[cfg(zephium_internal_repository_e2e)]
fn sealed_product_bundled_catalog_generations(
) -> Result<Option<SealedBundledCatalogGenerations>, BundledCatalogAdmissionError> {
    let active = repository_e2e_generation(
        PRODUCT_ACTIVE_CATALOG_BYTES,
        PRODUCT_ACTIVE_CATALOG_LENGTH,
        PRODUCT_ACTIVE_CATALOG_SHA256_HEX,
        ProductFixtureArtifacts {
            manifest_bytes: PRODUCT_ACTIVE_MANIFEST_BYTES,
            manifest_length: PRODUCT_ACTIVE_MANIFEST_LENGTH,
            manifest_sha256_hex: PRODUCT_ACTIVE_MANIFEST_SHA256_HEX,
            tree_index_bytes: PRODUCT_ACTIVE_TREE_INDEX_BYTES,
            tree_index_length: PRODUCT_ACTIVE_TREE_INDEX_LENGTH,
            tree_index_sha256_hex: PRODUCT_ACTIVE_TREE_INDEX_SHA256_HEX,
            tree_sha256_hex: PRODUCT_ACTIVE_TREE_SHA256_HEX,
            catalog_inventory_sha256_hex: PRODUCT_ACTIVE_CATALOG_INVENTORY_SHA256_HEX,
        },
    )?;
    let rollback = repository_e2e_generation(
        ROLLBACK_CATALOG_BYTES,
        ROLLBACK_CATALOG_LENGTH,
        ROLLBACK_CATALOG_SHA256_HEX,
        ProductFixtureArtifacts {
            manifest_bytes: MANIFEST_BYTES,
            manifest_length: MANIFEST_LENGTH,
            manifest_sha256_hex: MANIFEST_SHA256_HEX,
            tree_index_bytes: TREE_INDEX_BYTES,
            tree_index_length: TREE_INDEX_LENGTH,
            tree_index_sha256_hex: TREE_INDEX_SHA256_HEX,
            tree_sha256_hex: TREE_SHA256_HEX,
            catalog_inventory_sha256_hex: CATALOG_INVENTORY_SHA256_HEX,
        },
    )?;
    Ok(Some((active, vec![rollback].into_boxed_slice())))
}

#[cfg(zephium_internal_repository_e2e)]
#[derive(Clone, Copy)]
struct ProductFixtureArtifacts {
    manifest_bytes: &'static [u8],
    manifest_length: usize,
    manifest_sha256_hex: &'static str,
    tree_index_bytes: &'static [u8],
    tree_index_length: usize,
    tree_index_sha256_hex: &'static str,
    tree_sha256_hex: &'static str,
    catalog_inventory_sha256_hex: &'static str,
}

#[cfg(zephium_internal_repository_e2e)]
fn repository_e2e_generation(
    bytes: &[u8],
    expected_length: usize,
    expected_sha256_hex: &str,
    artifacts: ProductFixtureArtifacts,
) -> Result<SealedBundledCatalogGeneration, BundledCatalogAdmissionError> {
    if !repository_e2e_exact_bytes_match(bytes, expected_length, expected_sha256_hex)
        || !repository_e2e_exact_bytes_match(
            artifacts.manifest_bytes,
            artifacts.manifest_length,
            artifacts.manifest_sha256_hex,
        )
        || !repository_e2e_exact_bytes_match(
            artifacts.tree_index_bytes,
            artifacts.tree_index_length,
            artifacts.tree_index_sha256_hex,
        )
        || !repository_e2e_exact_bytes_match(
            LEGAL_NOTICE_BYTES,
            LEGAL_NOTICE_LENGTH,
            LEGAL_NOTICE_SHA256_HEX,
        )
    {
        return Err(BundledCatalogAdmissionError::InvalidProductConfiguration);
    }
    let catalog = ExtensionReleaseCatalog::parse_canonical(bytes)
        .map_err(|_| BundledCatalogAdmissionError::InvalidProductConfiguration)?;
    let tree_index = CanonicalExtensionTreeIndex::parse_canonical(artifacts.tree_index_bytes)
        .map_err(|_| BundledCatalogAdmissionError::InvalidProductConfiguration)?;
    let Some(package) = catalog.packages().first() else {
        return Err(BundledCatalogAdmissionError::InvalidProductConfiguration);
    };
    let anchor = anchor_for_fixture(bytes)
        .map_err(|_| BundledCatalogAdmissionError::InvalidProductConfiguration)?;
    if catalog.packages().len() != 1
        || package.bind_tree_index(&tree_index).is_err()
        || !repository_e2e_digest_matches(
            tree_index.manifest_sha256().bytes(),
            artifacts.manifest_sha256_hex,
        )
        || !repository_e2e_digest_matches(
            tree_index.index_sha256().bytes(),
            artifacts.tree_index_sha256_hex,
        )
        || !repository_e2e_digest_matches(
            tree_index.tree_sha256().bytes(),
            artifacts.tree_sha256_hex,
        )
        || usize::try_from(package.provenance().legal_notice().length()).ok()
            != Some(LEGAL_NOTICE_LENGTH)
        || !repository_e2e_digest_matches(
            package.provenance().legal_notice().sha256(),
            LEGAL_NOTICE_SHA256_HEX,
        )
        || !repository_e2e_digest_matches(
            anchor.inventory_digest.bytes(),
            artifacts.catalog_inventory_sha256_hex,
        )
    {
        return Err(BundledCatalogAdmissionError::InvalidProductConfiguration);
    }
    let policy = ExtensionReleaseAdmissionPolicy::new(
        ExtensionPackageAdmissionPolicyDigest::from_bytes(ADMISSION_POLICY_DIGEST_BYTES),
        vec![ExtensionReleaseLicenseRule::new(LICENSE_EXPRESSION, false)
            .map_err(|_| BundledCatalogAdmissionError::InvalidProductConfiguration)?],
    )
    .map_err(|_| BundledCatalogAdmissionError::InvalidProductConfiguration)?;
    Ok(SealedBundledCatalogGeneration { anchor, policy })
}

#[cfg(zephium_internal_repository_e2e)]
fn repository_e2e_exact_bytes_match(
    bytes: &[u8],
    expected_length: usize,
    expected_sha256_hex: &str,
) -> bool {
    bytes.len() == expected_length
        && repository_e2e_digest_matches(Sha256::digest(bytes).into(), expected_sha256_hex)
}

#[cfg(zephium_internal_repository_e2e)]
fn repository_e2e_digest_matches(bytes: [u8; 32], expected_lower_hex: &str) -> bool {
    let expected = expected_lower_hex.as_bytes();
    expected.len() == 64
        && bytes.iter().enumerate().all(|(index, byte)| {
            expected[index * 2] == repository_e2e_lower_hex_digit(byte >> 4)
                && expected[index * 2 + 1] == repository_e2e_lower_hex_digit(byte & 0x0f)
        })
}

#[cfg(zephium_internal_repository_e2e)]
const fn repository_e2e_lower_hex_digit(nibble: u8) -> u8 {
    match nibble {
        0..=9 => b'0' + nibble,
        10..=15 => b'a' + nibble - 10,
        _ => b'?',
    }
}

#[cfg(any(test, zephium_internal_repository_e2e))]
fn anchor_for_fixture(
    catalog_bytes: &[u8],
) -> Result<SealedBundledCatalogAnchor, BundledCatalogAdmissionError> {
    let catalog = ExtensionReleaseCatalog::parse_canonical(catalog_bytes)
        .map_err(BundledCatalogAdmissionError::Catalog)?;
    Ok(SealedBundledCatalogAnchor {
        catalog_length: catalog_bytes.len(),
        catalog_digest: ExtensionReleaseCatalogDigest::from_bytes(
            Sha256::digest(catalog_bytes).into(),
        ),
        authority: catalog.authority(),
        catalog_revision: catalog.revision(),
        admission_policy_digest: catalog.admission_policy_sha256(),
        inventory_digest: digest_catalog_inventory(&catalog)
            .ok_or(BundledCatalogAdmissionError::AccountingOverflow)?,
    })
}

const _: () = assert!(MAX_PRODUCT_ROLLBACK_BUNDLED_CATALOGS == 2);
const _: () = assert!(MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS == 3);
const _: () =
    assert!(MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS == 1 + MAX_PRODUCT_ROLLBACK_BUNDLED_CATALOGS);

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
                r#"{{"package_key":"{}","revision":1,"payload":{},"manifest_sha256":"{}","tree_sha256":"{}","tree_index_sha256":"{}","tree_index_length":1,"tree_file_count":1,"tree_bytes":4,"chromium":null,"provenance":{{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Example contributors","redistribution":"Reviewed bundled release","legal_notice":{{"target":"licenses/example.txt","kind":"notice_bundle","length":1,"sha256":"{}"}},"corresponding_source":null}}}}"#
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

    fn generation_for(bytes: &[u8], policy_digest: u8) -> SealedBundledCatalogGeneration {
        SealedBundledCatalogGeneration {
            anchor: anchor_for(bytes),
            policy: policy(policy_digest),
        }
    }

    #[cfg(not(zephium_internal_repository_e2e))]
    #[test]
    fn production_authority_is_explicitly_unprovisioned() {
        assert_eq!(MAX_PRODUCT_ROLLBACK_BUNDLED_CATALOGS, 2);
        assert_eq!(MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS, 3);
        assert!(matches!(
            BundledPackageAuthority::product(),
            Err(BundledCatalogAdmissionError::Unprovisioned)
        ));
        assert_eq!(
            BundledPackageAuthority::product_status(),
            BundledProductAuthorityStatus::Unprovisioned
        );
    }

    #[cfg(zephium_internal_repository_e2e)]
    #[test]
    fn internal_repository_fixture_admits_only_its_active_and_rollback_generations() {
        use crate::repository_e2e_fixture::{
            ACTIVE_CATALOG_BYTES, LEGAL_NOTICE_BYTES, ROLLBACK_CATALOG_BYTES,
        };

        let authority = BundledPackageAuthority::product().unwrap();
        assert_eq!(
            BundledPackageAuthority::product_status(),
            BundledProductAuthorityStatus::Configured
        );

        let active = authority.admit_catalog(ACTIVE_CATALOG_BYTES).unwrap();
        let rollback = authority
            .admit_rollback_catalog(ROLLBACK_CATALOG_BYTES)
            .unwrap();
        assert_eq!(active.revision().get(), 2);
        assert_eq!(rollback.revision().get(), 1);
        assert_eq!(
            active.catalog().packages()[0]
                .provenance()
                .legal_notice()
                .sha256(),
            <[u8; 32]>::from(Sha256::digest(LEGAL_NOTICE_BYTES))
        );
        assert_eq!(
            authority.recognize_generation(&active.generation_anchor()),
            Some(ProductBundledCatalogGenerationRole::Active)
        );
        assert_eq!(
            authority.recognize_generation(&rollback.generation_anchor()),
            Some(ProductBundledCatalogGenerationRole::Rollback)
        );
        assert!(matches!(
            authority.admit_catalog(ROLLBACK_CATALOG_BYTES),
            Err(BundledCatalogAdmissionError::CatalogDigestMismatch)
        ));
        assert!(matches!(
            authority.admit_rollback_catalog(ACTIVE_CATALOG_BYTES),
            Err(BundledCatalogAdmissionError::CatalogDigestMismatch)
        ));
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
    fn active_and_two_exact_rollback_generations_are_distinct_capabilities() {
        let rollback_one = catalog_bytes_with(
            AUTHORITY_BYTE,
            1,
            3,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#)],
        );
        let rollback_two = catalog_bytes_with(
            AUTHORITY_BYTE,
            2,
            4,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#)],
        );
        let active = catalog_bytes_with(
            AUTHORITY_BYTE,
            3,
            POLICY_BYTE,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#)],
        );
        let authority = BundledPackageAuthority::from_sealed_generations(
            generation_for(&active, POLICY_BYTE),
            vec![
                generation_for(&rollback_one, 3),
                generation_for(&rollback_two, 4),
            ]
            .into_boxed_slice(),
        )
        .unwrap();

        let admitted_active = authority.admit_catalog(&active).unwrap();
        let admitted_one = authority.admit_rollback_catalog(&rollback_one).unwrap();
        let admitted_two = authority.admit_rollback_catalog(&rollback_two).unwrap();
        assert_eq!(admitted_active.revision().get(), 3);
        assert_eq!(admitted_one.revision().get(), 1);
        assert_eq!(admitted_two.revision().get(), 2);
        assert_ne!(admitted_one.catalog_digest(), admitted_two.catalog_digest());
        assert!(
            admitted_one.retained_bytes() <= MAX_ADMITTED_ROLLBACK_BUNDLED_CATALOG_RETAINED_BYTES
        );
        assert!(authority.retained_bytes() <= MAX_BUNDLED_PACKAGE_AUTHORITY_RETAINED_BYTES);

        assert_eq!(
            authority.admit_catalog(&rollback_two).unwrap_err(),
            BundledCatalogAdmissionError::CatalogDigestMismatch
        );
        assert_eq!(
            authority.admit_rollback_catalog(&active).unwrap_err(),
            BundledCatalogAdmissionError::CatalogDigestMismatch
        );

        let active_anchor = admitted_active.generation_anchor();
        let rollback_anchor = admitted_one.generation_anchor();
        assert_eq!(
            active_anchor.catalog_length(),
            u64::try_from(active.len()).unwrap()
        );
        assert_eq!(
            rollback_anchor.catalog_length(),
            u64::try_from(rollback_one.len()).unwrap()
        );
        assert_eq!(
            authority.recognize_generation(&active_anchor),
            Some(ProductBundledCatalogGenerationRole::Active)
        );
        assert_eq!(
            authority.recognize_generation(&rollback_anchor),
            Some(ProductBundledCatalogGenerationRole::Rollback)
        );

        let invented = [
            BundledCatalogGenerationAnchor::from_parts(
                ExtensionAuthorityId::from_bytes([9; 32]),
                admitted_one.revision(),
                u64::try_from(rollback_one.len()).unwrap(),
                admitted_one.catalog_digest(),
                admitted_one.inventory_digest(),
            ),
            BundledCatalogGenerationAnchor::from_parts(
                admitted_one.authority(),
                ExtensionReleaseCatalogRevision::new(2).unwrap(),
                u64::try_from(rollback_one.len()).unwrap(),
                admitted_one.catalog_digest(),
                admitted_one.inventory_digest(),
            ),
            BundledCatalogGenerationAnchor::from_parts(
                admitted_one.authority(),
                admitted_one.revision(),
                u64::try_from(rollback_one.len() + 1).unwrap(),
                admitted_one.catalog_digest(),
                admitted_one.inventory_digest(),
            ),
            BundledCatalogGenerationAnchor::from_parts(
                admitted_one.authority(),
                admitted_one.revision(),
                u64::try_from(rollback_one.len()).unwrap(),
                ExtensionReleaseCatalogDigest::from_bytes([9; 32]),
                admitted_one.inventory_digest(),
            ),
            BundledCatalogGenerationAnchor::from_parts(
                admitted_one.authority(),
                admitted_one.revision(),
                u64::try_from(rollback_one.len()).unwrap(),
                admitted_one.catalog_digest(),
                BundledCatalogInventoryDigest::from_bytes([9; 32]),
            ),
        ];
        assert!(invented
            .iter()
            .all(|anchor| authority.recognize_generation(anchor).is_none()));
    }

    #[test]
    fn generation_configuration_is_same_epoch_ordered_unique_and_bounded() {
        let oldest = catalog_bytes_with(
            AUTHORITY_BYTE,
            1,
            POLICY_BYTE,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#)],
        );
        let middle = catalog_bytes_with(
            AUTHORITY_BYTE,
            2,
            POLICY_BYTE,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#)],
        );
        let active = catalog_bytes_with(
            AUTHORITY_BYTE,
            3,
            POLICY_BYTE,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#)],
        );
        let foreign = catalog_bytes_with(
            9,
            1,
            POLICY_BYTE,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#)],
        );

        let invalid = |rollback: Vec<SealedBundledCatalogGeneration>| {
            assert!(matches!(
                BundledPackageAuthority::from_sealed_generations(
                    generation_for(&active, POLICY_BYTE),
                    rollback.into_boxed_slice(),
                ),
                Err(BundledCatalogAdmissionError::InvalidProductConfiguration)
            ));
        };
        invalid(vec![generation_for(&foreign, POLICY_BYTE)]);
        invalid(vec![
            generation_for(&middle, POLICY_BYTE),
            generation_for(&oldest, POLICY_BYTE),
        ]);
        invalid(vec![
            generation_for(&oldest, POLICY_BYTE),
            generation_for(&oldest, POLICY_BYTE),
        ]);
        invalid(vec![generation_for(&active, POLICY_BYTE)]);
        invalid(vec![
            generation_for(&oldest, POLICY_BYTE),
            generation_for(&middle, POLICY_BYTE),
            generation_for(&middle, POLICY_BYTE),
        ]);

        let mut duplicate_digest = generation_for(&middle, POLICY_BYTE);
        duplicate_digest.anchor.catalog_digest = anchor_for(&oldest).catalog_digest;
        invalid(vec![generation_for(&oldest, POLICY_BYTE), duplicate_digest]);
    }

    #[test]
    fn rollback_generation_requires_its_exact_bytes_and_policy() {
        let rollback = catalog_bytes_with(
            AUTHORITY_BYTE,
            1,
            3,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#)],
        );
        let active = catalog_bytes_with(
            AUTHORITY_BYTE,
            2,
            POLICY_BYTE,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#)],
        );
        assert!(matches!(
            BundledPackageAuthority::from_sealed_generations(
                generation_for(&active, POLICY_BYTE),
                vec![generation_for(&rollback, POLICY_BYTE)].into_boxed_slice(),
            ),
            Err(BundledCatalogAdmissionError::InvalidProductConfiguration)
        ));

        let authority = BundledPackageAuthority::from_sealed_generations(
            generation_for(&active, POLICY_BYTE),
            vec![generation_for(&rollback, 3)].into_boxed_slice(),
        )
        .unwrap();
        let mut changed = rollback.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        assert_eq!(
            authority.admit_rollback_catalog(&changed).unwrap_err(),
            BundledCatalogAdmissionError::CatalogDigestMismatch
        );
    }

    #[test]
    fn active_only_authority_refuses_rollback_admission_explicitly() {
        let bytes = catalog_bytes();
        assert_eq!(
            authority_for(&bytes)
                .admit_rollback_catalog(&bytes)
                .unwrap_err(),
            BundledCatalogAdmissionError::RollbackCatalogNotProvisioned
        );
    }

    #[test]
    fn exact_fixture_is_admitted_as_metadata_only() {
        let bytes = catalog_bytes();
        let authority = authority_for(&bytes);
        assert!(matches!(
            authority.admit_active_catalog(&bytes).unwrap(),
            AdmittedActiveCatalog::Bundled(_)
        ));
        let admitted = authority.admit_catalog(&bytes).unwrap();
        assert_eq!(admitted.catalog().packages().len(), 1);
        assert_eq!(admitted.authority().bytes(), [AUTHORITY_BYTE; 32]);
        assert_eq!(admitted.revision().get(), 1);
        assert_eq!(
            admitted.catalog_length(),
            u64::try_from(bytes.len()).unwrap()
        );
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
    fn acquired_catalog_admission_is_nominal_and_requires_full_chromium_identity() {
        let acquired_package = package_json(
            7,
            &format!(
                r#"{{"kind":"acquired_zip","length":4,"sha256":"{}"}}"#,
                hex(8)
            ),
        )
        .replace(
            r#""chromium":null"#,
            &format!(r#""chromium":{{"manifest_key_sha256":"{}"}}"#, hex(9)),
        );
        let bytes = catalog_bytes_with(AUTHORITY_BYTE, 1, POLICY_BYTE, &[acquired_package]);
        let authority = authority_for(&bytes);
        assert_eq!(
            authority.admit_catalog(&bytes).unwrap_err(),
            BundledCatalogAdmissionError::UnsupportedPayload
        );
        let acquired = authority.admit_acquired_catalog(&bytes).unwrap();
        assert_eq!(acquired.catalog().packages().len(), 1);
        assert!(matches!(
            acquired.catalog().packages()[0].payload(),
            ExtensionPackagePayloadIdentity::AcquiredZip { .. }
        ));
        assert!(acquired.retained_bytes() <= MAX_ADMITTED_ACQUIRED_CATALOG_RETAINED_BYTES);
        assert_eq!(
            acquired.disposition_against(None),
            BundledCatalogDisposition::Candidate
        );
        assert!(matches!(
            authority.admit_active_catalog(&bytes).unwrap(),
            AdmittedActiveCatalog::Acquired(_)
        ));

        let bundled_bytes = catalog_bytes();
        assert_eq!(
            authority_for(&bundled_bytes)
                .admit_acquired_catalog(&bundled_bytes)
                .unwrap_err(),
            BundledCatalogAdmissionError::UnsupportedPayload
        );

        let missing_chromium = catalog_bytes_with(
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
        assert_eq!(
            authority_for(&missing_chromium)
                .admit_acquired_catalog(&missing_chromium)
                .unwrap_err(),
            BundledCatalogAdmissionError::UnsupportedPayload
        );
    }

    #[test]
    fn active_catalog_classification_rejects_mixed_payload_representations() {
        let acquired = package_json(
            8,
            &format!(
                r#"{{"kind":"acquired_zip","length":4,"sha256":"{}"}}"#,
                hex(10)
            ),
        )
        .replace(
            r#""chromium":null"#,
            &format!(r#""chromium":{{"manifest_key_sha256":"{}"}}"#, hex(11)),
        );
        let bytes = catalog_bytes_with(
            AUTHORITY_BYTE,
            1,
            POLICY_BYTE,
            &[package_json(7, r#"{"kind":"bundled_tree"}"#), acquired],
        );
        assert_eq!(
            authority_for(&bytes)
                .admit_active_catalog(&bytes)
                .unwrap_err(),
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
                67, 122, 176, 172, 80, 12, 108, 188, 222, 236, 67, 183, 197, 182, 21, 209, 184, 96,
                96, 204, 102, 239, 228, 66, 35, 50, 243, 80, 36, 123, 220, 173,
            ]
        );
    }

    #[test]
    fn inventory_digest_redundantly_binds_tree_index_length() {
        let first = catalog_bytes();
        let second = String::from_utf8(first.clone())
            .unwrap()
            .replace(r#""tree_index_length":1"#, r#""tree_index_length":2"#)
            .into_bytes();
        let first = ExtensionReleaseCatalog::parse_canonical(&first).unwrap();
        let second = ExtensionReleaseCatalog::parse_canonical(&second).unwrap();

        assert_ne!(
            digest_catalog_inventory(&first),
            digest_catalog_inventory(&second),
            "sealed inventory framing must bind tree-index byte length"
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
                30, 44, 28, 186, 57, 81, 90, 235, 123, 146, 89, 78, 170, 146, 219, 35, 127, 29, 39,
                162, 172, 202, 81, 107, 72, 119, 142, 196, 184, 15, 30, 142,
            ]
        );
    }
}
