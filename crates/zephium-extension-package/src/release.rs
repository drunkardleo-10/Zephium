use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::mem::size_of;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::{Host, Url};
use zephium_core::extensions::{
    ExtensionArchiveDigest, ExtensionAuthorityId, ExtensionManifestDigest,
    ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackagePayloadIdentity,
    ExtensionPackageRevision, ExtensionTreeDigest,
};

use crate::digest::decode_lower_hex_32;
use crate::relative_path::portable_path_shape_conflicts;
use crate::{
    parse_bounded_json, BoundedJsonError, BoundedJsonLimits, CanonicalExtensionTreeIndex,
    ChromiumExtensionId, ChromiumManifestKey, ChromiumManifestKeyDigest, ExtensionTreeIndexDigest,
    PortableRelativePath, MAX_EXTENSION_ARCHIVE_BYTES, MAX_EXTENSION_LEGAL_NOTICE_BYTES,
    MAX_EXTENSION_LICENSE_RULES, MAX_EXTENSION_PACKAGE_LINES,
    MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES, MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES,
    MAX_EXTENSION_TREE_BYTES, MAX_EXTENSION_TREE_FILES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

const RELEASE_CATALOG_SCHEMA_VERSION: u32 = 1;
const MAX_RELEASE_UNIX: u64 = 7_258_118_400;
const MAX_SOURCE_URL_BYTES: usize = 2 * 1024;
const MAX_UPSTREAM_VERSION_BYTES: usize = 128;
const MAX_LICENSE_EXPRESSION_BYTES: usize = 256;
const MAX_LICENSE_ATTRIBUTION_BYTES: usize = 4 * 1024;
const MAX_REDISTRIBUTION_RECORD_BYTES: usize = 4 * 1024;
const RELEASE_CATALOG_ACCOUNTING_FIXED_BYTES: usize = 1024;
const MAX_DURABLE_RELEASE_CATALOG_REVISION: u64 = i64::MAX as u64;

macro_rules! release_digest {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; 32]);

        impl $name {
            /// Constructs an exact structural digest.
            pub const fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            /// Returns exact digest bytes.
            pub const fn bytes(self) -> [u8; 32] {
                self.0
            }

            /// Borrows exact digest bytes.
            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    concat!(stringify!($name), "({:02x}{:02x}{:02x}{:02x}…)"),
                    self.0[0], self.0[1], self.0[2], self.0[3]
                )
            }
        }
    };
}

release_digest!(
    ExtensionReleaseCatalogDigest,
    "SHA-256 of exact canonical bundled extension release-catalog bytes."
);
release_digest!(
    ExtensionPackageAdmissionPolicyDigest,
    "Digest of the product-owned parser, compatibility, limit, and license policy."
);

/// Strictly positive durable sequence for authenticated release catalogs.
///
/// Package authorities persist this value as the release-catalog rollback and
/// equivocation high-water mark. It is distinct from each package update
/// line's [`ExtensionPackageRevision`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionReleaseCatalogRevision(u64);

impl ExtensionReleaseCatalogRevision {
    /// First valid durable release-catalog revision.
    pub const INITIAL: Self = Self(1);

    /// Creates a revision representable by Zephium's durable adapters.
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 || value > MAX_DURABLE_RELEASE_CATALOG_REVISION {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Returns the durable integer value.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next durable revision without wrapping.
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Self::new(next),
            None => None,
        }
    }
}

/// Signed expected Chromium native identity for one package update line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedChromiumIdentity {
    manifest_key_sha256: ChromiumManifestKeyDigest,
    extension_id: ChromiumExtensionId,
}

impl ExpectedChromiumIdentity {
    /// Derives the only valid Chromium id from the signed key digest.
    pub fn from_manifest_key_digest(manifest_key_sha256: ChromiumManifestKeyDigest) -> Self {
        Self {
            manifest_key_sha256,
            extension_id: manifest_key_sha256.derived_extension_id(),
        }
    }

    /// Returns SHA-256 of exact decoded manifest-key bytes.
    pub const fn manifest_key_sha256(&self) -> ChromiumManifestKeyDigest {
        self.manifest_key_sha256
    }

    /// Returns the signed expected Chromium extension ID.
    pub const fn extension_id(&self) -> &ChromiumExtensionId {
        &self.extension_id
    }

    /// Binds the signed expectation to the exact key parsed from manifest bytes.
    pub fn verify_manifest_key(
        &self,
        key: &ChromiumManifestKey,
    ) -> Result<(), ExtensionReleaseCatalogError> {
        if key.digest() == self.manifest_key_sha256 && key.extension_id() == &self.extension_id {
            Ok(())
        } else {
            Err(ExtensionReleaseCatalogError::ChromiumIdentity)
        }
    }
}

/// Bounded upstream and license provenance carried by a signed app release.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionReleasePackageProvenance {
    source_url: Box<str>,
    upstream_version: Box<str>,
    upstream_revision: Box<str>,
    license_expression: Box<str>,
    attribution: Box<str>,
    redistribution: Box<str>,
    legal_notice: ExtensionReleaseLegalNotice,
    corresponding_source: Option<ExtensionReleaseSourceReference>,
}

impl ExtensionReleasePackageProvenance {
    /// Returns the exact reviewed HTTPS provenance reference.
    ///
    /// This value is not a runtime download target. Product policy must judge
    /// whether its origin/path semantics are immutable enough for the named
    /// upstream; the generic parser only makes the reference canonical.
    pub fn source_url(&self) -> &str {
        &self.source_url
    }

    /// Returns the exact upstream product version.
    pub fn upstream_version(&self) -> &str {
        &self.upstream_version
    }

    /// Returns the exact lowercase Git revision.
    pub fn upstream_revision(&self) -> &str {
        &self.upstream_revision
    }

    /// Returns the exact declared SPDX or project-owned license expression.
    pub fn license_expression(&self) -> &str {
        &self.license_expression
    }

    /// Returns the exact human-readable upstream attribution record.
    pub fn attribution(&self) -> &str {
        &self.attribution
    }

    /// Returns the exact reviewed redistribution terms or approval record.
    pub fn redistribution(&self) -> &str {
        &self.redistribution
    }

    /// Returns the exact shipped legal-notice bundle binding.
    pub const fn legal_notice(&self) -> &ExtensionReleaseLegalNotice {
        &self.legal_notice
    }

    /// Returns the exact corresponding-source reference when one was reviewed.
    pub const fn corresponding_source(&self) -> Option<&ExtensionReleaseSourceReference> {
        self.corresponding_source.as_ref()
    }

    fn retained_bytes(&self) -> Option<usize> {
        size_of::<Self>()
            .checked_add(self.source_url.len())?
            .checked_add(self.upstream_version.len())?
            .checked_add(self.upstream_revision.len())?
            .checked_add(self.license_expression.len())
            .and_then(|bytes| bytes.checked_add(self.attribution.len()))?
            .checked_add(self.redistribution.len())?
            .checked_add(self.legal_notice.target.as_str().len())
            .and_then(|bytes| {
                self.corresponding_source
                    .as_ref()
                    .map_or(Some(bytes), |source| {
                        bytes
                            .checked_add(source.url.len())?
                            .checked_add(source.revision.len())
                    })
            })
    }
}

/// Reviewed exact source reference for licenses which require corresponding source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionReleaseSourceReference {
    url: Box<str>,
    revision: Box<str>,
}

impl ExtensionReleaseSourceReference {
    /// Returns the exact canonical HTTPS source reference.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the exact lowercase Git revision named by the reference.
    pub fn revision(&self) -> &str {
        &self.revision
    }
}

/// Product-owned exact license admission rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionReleaseLicenseRule {
    expression: Box<str>,
    corresponding_source_required: bool,
}

impl ExtensionReleaseLicenseRule {
    /// Creates one exact allowlist entry.
    pub fn new(
        expression: &str,
        corresponding_source_required: bool,
    ) -> Result<Self, ExtensionReleaseAdmissionPolicyError> {
        if !valid_bounded_printable(expression, MAX_LICENSE_EXPRESSION_BYTES) {
            return Err(ExtensionReleaseAdmissionPolicyError::InvalidLicenseExpression);
        }
        Ok(Self {
            expression: expression.into(),
            corresponding_source_required,
        })
    }

    /// Returns the exact allowed expression.
    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// Returns whether a matching package must bind corresponding source.
    pub const fn corresponding_source_required(&self) -> bool {
        self.corresponding_source_required
    }
}

/// Trusted product policy used to semantically admit release rows.
///
/// The digest is the broader compiled parser/compatibility/license-policy
/// anchor. The exact rules are carried alongside it so equality with a
/// catalog-controlled digest can never substitute for actual license checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionReleaseAdmissionPolicy {
    digest: ExtensionPackageAdmissionPolicyDigest,
    license_rules: Box<[ExtensionReleaseLicenseRule]>,
}

impl ExtensionReleaseAdmissionPolicy {
    /// Constructs one product-owned policy after bounding and canonicalizing its rules.
    pub fn new(
        digest: ExtensionPackageAdmissionPolicyDigest,
        mut license_rules: Vec<ExtensionReleaseLicenseRule>,
    ) -> Result<Self, ExtensionReleaseAdmissionPolicyError> {
        if license_rules.is_empty() || license_rules.len() > MAX_EXTENSION_LICENSE_RULES {
            return Err(ExtensionReleaseAdmissionPolicyError::LicenseRuleCount {
                count: license_rules.len(),
                max: MAX_EXTENSION_LICENSE_RULES,
            });
        }
        license_rules.sort_unstable_by(|left, right| left.expression.cmp(&right.expression));
        if license_rules
            .windows(2)
            .any(|pair| pair[0].expression == pair[1].expression)
        {
            return Err(ExtensionReleaseAdmissionPolicyError::DuplicateLicenseExpression);
        }
        Ok(Self {
            digest,
            license_rules: license_rules.into_boxed_slice(),
        })
    }

    /// Returns the exact broader product-policy digest.
    pub const fn digest(&self) -> ExtensionPackageAdmissionPolicyDigest {
        self.digest
    }

    /// Returns canonical exact license rules.
    pub fn license_rules(&self) -> &[ExtensionReleaseLicenseRule] {
        &self.license_rules
    }

    fn license_rule(&self, expression: &str) -> Option<&ExtensionReleaseLicenseRule> {
        self.license_rules
            .binary_search_by(|rule| rule.expression.as_ref().cmp(expression))
            .ok()
            .map(|index| &self.license_rules[index])
    }
}

/// Stable product-policy construction failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionReleaseAdmissionPolicyError {
    /// License rule count is zero or exceeds the product ceiling.
    LicenseRuleCount {
        /// Observed rules.
        count: usize,
        /// Maximum rules.
        max: usize,
    },
    /// A license expression is empty, boundary-whitespace-bearing, non-ASCII,
    /// control-bearing, or oversized.
    InvalidLicenseExpression,
    /// The same exact expression appears more than once.
    DuplicateLicenseExpression,
}

impl fmt::Display for ExtensionReleaseAdmissionPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LicenseRuleCount { count, max } => {
                write!(
                    formatter,
                    "extension release policy has {count} license rules; maximum is {max}"
                )
            }
            Self::InvalidLicenseExpression => {
                formatter.write_str("extension release policy license expression is invalid")
            }
            Self::DuplicateLicenseExpression => {
                formatter.write_str("extension release policy repeats a license expression")
            }
        }
    }
}

impl Error for ExtensionReleaseAdmissionPolicyError {}

/// Exact release-resource binding for one package's shipped legal notices.
///
/// Parsing this value does not prove the resource exists. Release-seed
/// admission must read the target from the signed application resources and
/// verify its exact length and digest before a package becomes distributable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionReleaseLegalNotice {
    target: PortableRelativePath,
    kind: ExtensionReleaseLegalArtifactKind,
    length: u64,
    sha256: [u8; 32],
}

impl ExtensionReleaseLegalNotice {
    /// Returns the canonical release-relative `licenses/` target.
    pub const fn target(&self) -> &PortableRelativePath {
        &self.target
    }

    /// Returns the reviewed role of this exact legal artifact.
    pub const fn kind(&self) -> ExtensionReleaseLegalArtifactKind {
        self.kind
    }

    /// Returns the exact legal-notice byte length.
    pub const fn length(&self) -> u64 {
        self.length
    }

    /// Returns SHA-256 of the exact legal-notice bytes.
    pub const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    /// Verifies exact legal-notice bytes supplied by signed release resources.
    pub fn verify_bytes(&self, bytes: &[u8]) -> Result<(), ExtensionReleaseCatalogError> {
        if u64::try_from(bytes.len()).ok() != Some(self.length)
            || <[u8; 32]>::from(Sha256::digest(bytes)) != self.sha256
        {
            return Err(ExtensionReleaseCatalogError::LegalNoticeMismatch);
        }
        Ok(())
    }
}

/// Closed role vocabulary for a shipped legal artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionReleaseLegalArtifactKind {
    /// Aggregate license, attribution, and third-party notice bundle.
    NoticeBundle,
}

/// One exact extension package named by a signed release catalog.
///
/// The tagged payload distinguishes an authenticated bundled tree from a
/// future exact acquired ZIP. A signed bundled-tree seed verifies the catalog,
/// tree index, and every materialized file directly; no archive evidence is
/// invented or required.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionReleasePackage {
    identity: ExtensionPackageIdentity,
    tree_index_sha256: ExtensionTreeIndexDigest,
    tree_index_length: u64,
    tree_file_count: usize,
    tree_bytes: u64,
    chromium: Option<ExpectedChromiumIdentity>,
    provenance: ExtensionReleasePackageProvenance,
}

impl ExtensionReleasePackage {
    /// Returns the complete structural package identity.
    pub const fn identity(&self) -> &ExtensionPackageIdentity {
        &self.identity
    }

    /// Returns the exact bundled-tree or acquired-ZIP payload evidence.
    pub const fn payload(&self) -> ExtensionPackagePayloadIdentity {
        self.identity.payload()
    }

    /// Returns SHA-256 of exact canonical tree-index bytes.
    pub const fn tree_index_sha256(&self) -> ExtensionTreeIndexDigest {
        self.tree_index_sha256
    }

    /// Returns the signed exact canonical tree-index byte length.
    pub const fn tree_index_length(&self) -> u64 {
        self.tree_index_length
    }

    /// Returns the signed exact tree file count.
    pub const fn tree_file_count(&self) -> usize {
        self.tree_file_count
    }

    /// Returns the signed exact tree byte count.
    pub const fn tree_bytes(&self) -> u64 {
        self.tree_bytes
    }

    /// Returns signed expected Chromium identity when this package may target WebView2.
    pub const fn chromium(&self) -> Option<&ExpectedChromiumIdentity> {
        self.chromium.as_ref()
    }

    /// Returns signed source and license provenance.
    pub const fn provenance(&self) -> &ExtensionReleasePackageProvenance {
        &self.provenance
    }

    /// Binds an independently parsed exact tree index to this release entry.
    pub fn bind_tree_index<'a>(
        &'a self,
        index: &'a CanonicalExtensionTreeIndex,
    ) -> Result<ExtensionReleaseTreeBinding<'a>, ExtensionReleaseCatalogError> {
        if self.tree_index_sha256 != index.index_sha256()
            || self.tree_index_length != index.index_bytes()
            || self.identity.tree_sha256() != index.tree_sha256()
            || self.tree_file_count != index.files().len()
            || self.tree_bytes != index.total_bytes()
        {
            return Err(ExtensionReleaseCatalogError::TreeIndexMismatch);
        }
        if self.identity.manifest_sha256() != index.manifest_sha256() {
            return Err(ExtensionReleaseCatalogError::ManifestMismatch);
        }
        Ok(ExtensionReleaseTreeBinding {
            package: self,
            index,
        })
    }
}

/// Exact release package and canonical tree index after cross-validation.
#[derive(Clone, Copy, Debug)]
pub struct ExtensionReleaseTreeBinding<'a> {
    package: &'a ExtensionReleasePackage,
    index: &'a CanonicalExtensionTreeIndex,
}

impl<'a> ExtensionReleaseTreeBinding<'a> {
    /// Returns the signed package entry.
    pub const fn package(self) -> &'a ExtensionReleasePackage {
        self.package
    }

    /// Returns the exact canonical resource inventory.
    pub const fn index(self) -> &'a CanonicalExtensionTreeIndex {
        self.index
    }
}

/// Canonical bundled extension release catalog.
///
/// Successful parsing is structural only. The caller must authenticate exact
/// catalog bytes through the signed application release and compare
/// [`Self::admission_policy_sha256`] to a product-owned expected policy before
/// any package can be leased or activated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionReleaseCatalog {
    revision: ExtensionReleaseCatalogRevision,
    created_unix: u64,
    authority: ExtensionAuthorityId,
    admission_policy_sha256: ExtensionPackageAdmissionPolicyDigest,
    packages: Box<[ExtensionReleasePackage]>,
    digest: ExtensionReleaseCatalogDigest,
    retained_bytes: usize,
}

impl ExtensionReleaseCatalog {
    /// Parses one exact canonical catalog with complete structural validation.
    pub fn parse_canonical(bytes: &[u8]) -> Result<Self, ExtensionReleaseCatalogError> {
        let bounded = parse_bounded_json(bytes, BoundedJsonLimits::release_catalog())
            .map_err(ExtensionReleaseCatalogError::Json)?;
        let raw: RawReleaseCatalog = serde_json::from_value(bounded.into_value())
            .map_err(|_| ExtensionReleaseCatalogError::Malformed)?;
        let encoded =
            serde_json::to_vec(&raw).map_err(|_| ExtensionReleaseCatalogError::Malformed)?;
        if encoded.as_slice() != bytes {
            return Err(ExtensionReleaseCatalogError::NonCanonical);
        }
        if raw.schema_version != RELEASE_CATALOG_SCHEMA_VERSION {
            return Err(ExtensionReleaseCatalogError::UnsupportedSchema);
        }
        let revision = ExtensionReleaseCatalogRevision::new(raw.catalog_revision)
            .ok_or(ExtensionReleaseCatalogError::InvalidCatalogRevision)?;
        if raw.created_unix == 0 || raw.created_unix > MAX_RELEASE_UNIX {
            return Err(ExtensionReleaseCatalogError::InvalidCreationTime);
        }
        if raw.packages.is_empty() || raw.packages.len() > MAX_EXTENSION_PACKAGE_LINES {
            return Err(ExtensionReleaseCatalogError::PackageCount {
                count: raw.packages.len(),
                max: MAX_EXTENSION_PACKAGE_LINES,
            });
        }
        let authority = ExtensionAuthorityId::from_bytes(
            decode_lower_hex_32(&raw.authority_id)
                .map_err(|_| ExtensionReleaseCatalogError::Digest)?,
        );
        let admission_policy_sha256 = ExtensionPackageAdmissionPolicyDigest(
            decode_lower_hex_32(&raw.admission_policy_sha256)
                .map_err(|_| ExtensionReleaseCatalogError::Digest)?,
        );

        let mut packages = Vec::with_capacity(raw.packages.len());
        let mut previous_key = None;
        let mut aggregate_tree_bytes = 0_u64;
        let mut legal_notice_targets = BTreeSet::new();
        for raw_package in raw.packages {
            let key_bytes = decode_lower_hex_32(&raw_package.package_key)
                .map_err(|_| ExtensionReleaseCatalogError::Digest)?;
            if previous_key.is_some_and(|previous: [u8; 32]| previous >= key_bytes) {
                return Err(ExtensionReleaseCatalogError::NonCanonicalPackageOrder);
            }
            previous_key = Some(key_bytes);
            let key = ExtensionPackageKey::from_bytes(key_bytes);
            let revision = ExtensionPackageRevision::new(raw_package.revision)
                .ok_or(ExtensionReleaseCatalogError::InvalidPackageRevision)?;
            let payload = match raw_package.payload {
                RawPackagePayload::BundledTree => ExtensionPackagePayloadIdentity::BundledTree,
                RawPackagePayload::AcquiredZip { length, sha256 } => {
                    let sha256 = ExtensionArchiveDigest::from_bytes(
                        decode_lower_hex_32(&sha256)
                            .map_err(|_| ExtensionReleaseCatalogError::Digest)?,
                    );
                    ExtensionPackagePayloadIdentity::acquired_zip(length, sha256).ok_or(
                        ExtensionReleaseCatalogError::ArchiveSize {
                            bytes: length,
                            max: MAX_EXTENSION_ARCHIVE_BYTES,
                        },
                    )?
                }
            };
            let manifest_sha256 = ExtensionManifestDigest::from_bytes(
                decode_lower_hex_32(&raw_package.manifest_sha256)
                    .map_err(|_| ExtensionReleaseCatalogError::Digest)?,
            );
            let tree_sha256 = ExtensionTreeDigest::from_bytes(
                decode_lower_hex_32(&raw_package.tree_sha256)
                    .map_err(|_| ExtensionReleaseCatalogError::Digest)?,
            );
            let tree_index_sha256 = ExtensionTreeIndexDigest::from_bytes(
                decode_lower_hex_32(&raw_package.tree_index_sha256)
                    .map_err(|_| ExtensionReleaseCatalogError::Digest)?,
            );
            let maximum_tree_index_bytes = MAX_EXTENSION_TREE_INDEX_BYTES as u64;
            if raw_package.tree_index_length == 0
                || raw_package.tree_index_length > maximum_tree_index_bytes
            {
                return Err(ExtensionReleaseCatalogError::TreeIndexSize {
                    bytes: raw_package.tree_index_length,
                    max: maximum_tree_index_bytes,
                });
            }
            let tree_file_count = usize::try_from(raw_package.tree_file_count).map_err(|_| {
                ExtensionReleaseCatalogError::TreeFileCount {
                    count: raw_package.tree_file_count,
                    max: MAX_EXTENSION_TREE_FILES,
                }
            })?;
            if tree_file_count == 0 || tree_file_count > MAX_EXTENSION_TREE_FILES {
                return Err(ExtensionReleaseCatalogError::TreeFileCount {
                    count: raw_package.tree_file_count,
                    max: MAX_EXTENSION_TREE_FILES,
                });
            }
            if raw_package.tree_bytes == 0 || raw_package.tree_bytes > MAX_EXTENSION_TREE_BYTES {
                return Err(ExtensionReleaseCatalogError::TreeSize {
                    bytes: raw_package.tree_bytes,
                    max: MAX_EXTENSION_TREE_BYTES,
                });
            }
            aggregate_tree_bytes = aggregate_tree_bytes
                .checked_add(raw_package.tree_bytes)
                .ok_or(ExtensionReleaseCatalogError::CatalogTreeSize {
                    bytes: u64::MAX,
                    max: MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES,
                })?;
            if aggregate_tree_bytes > MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES {
                return Err(ExtensionReleaseCatalogError::CatalogTreeSize {
                    bytes: aggregate_tree_bytes,
                    max: MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES,
                });
            }
            let chromium = raw_package
                .chromium
                .map(|raw| {
                    Ok(ExpectedChromiumIdentity::from_manifest_key_digest(
                        ChromiumManifestKeyDigest::from_bytes(
                            decode_lower_hex_32(&raw.manifest_key_sha256)
                                .map_err(|_| ExtensionReleaseCatalogError::Digest)?,
                        ),
                    ))
                })
                .transpose()?;
            let provenance = validate_provenance(raw_package.provenance)?;
            if chromium.as_ref().is_some_and(|candidate| {
                packages.iter().any(|existing: &ExtensionReleasePackage| {
                    existing.chromium.as_ref().is_some_and(|identity| {
                        identity.extension_id == candidate.extension_id
                            || identity.manifest_key_sha256 == candidate.manifest_key_sha256
                    })
                })
            }) {
                return Err(ExtensionReleaseCatalogError::DuplicateChromiumIdentity);
            }
            let legal_notice_collision_key = provenance.legal_notice.target.collision_key();
            if let Some(existing) = packages
                .iter()
                .map(|package: &ExtensionReleasePackage| &package.provenance.legal_notice)
                .find(|notice| {
                    notice
                        .target
                        .as_str()
                        .eq_ignore_ascii_case(provenance.legal_notice.target.as_str())
                })
            {
                if existing != &provenance.legal_notice {
                    return Err(ExtensionReleaseCatalogError::ConflictingLegalNoticeTarget);
                }
            } else if portable_path_shape_conflicts(
                &legal_notice_targets,
                &legal_notice_collision_key,
            ) {
                return Err(ExtensionReleaseCatalogError::ConflictingLegalNoticeTarget);
            }
            legal_notice_targets.insert(legal_notice_collision_key);
            packages.push(ExtensionReleasePackage {
                identity: ExtensionPackageIdentity::new(
                    authority,
                    key,
                    revision,
                    payload,
                    manifest_sha256,
                    tree_sha256,
                ),
                tree_index_sha256,
                tree_index_length: raw_package.tree_index_length,
                tree_file_count,
                tree_bytes: raw_package.tree_bytes,
                chromium,
                provenance,
            });
        }

        let retained_bytes = packages.iter().try_fold(
            RELEASE_CATALOG_ACCOUNTING_FIXED_BYTES
                .checked_add(size_of::<Self>())
                .and_then(|value| {
                    packages
                        .len()
                        .checked_mul(size_of::<ExtensionReleasePackage>())
                        .and_then(|bytes| value.checked_add(bytes))
                })
                .ok_or(ExtensionReleaseCatalogError::AccountingOverflow)?,
            |total, package| {
                total
                    .checked_add(
                        package
                            .provenance
                            .retained_bytes()
                            .ok_or(ExtensionReleaseCatalogError::AccountingOverflow)?,
                    )
                    .and_then(|value| {
                        value.checked_add(
                            package
                                .chromium
                                .as_ref()
                                .map_or(0, |identity| identity.extension_id.as_str().len()),
                        )
                    })
                    .ok_or(ExtensionReleaseCatalogError::AccountingOverflow)
            },
        )?;
        if retained_bytes > MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES {
            return Err(ExtensionReleaseCatalogError::RetainedBytes {
                bytes: retained_bytes,
                max: MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES,
            });
        }

        Ok(Self {
            revision,
            created_unix: raw.created_unix,
            authority,
            admission_policy_sha256,
            packages: packages.into_boxed_slice(),
            digest: ExtensionReleaseCatalogDigest(Sha256::digest(bytes).into()),
            retained_bytes,
        })
    }

    /// Returns the strictly positive release-catalog revision.
    pub const fn revision(&self) -> ExtensionReleaseCatalogRevision {
        self.revision
    }

    /// Returns the signed release creation time in Unix seconds.
    pub const fn created_unix(&self) -> u64 {
        self.created_unix
    }

    /// Returns the package trust-domain and epoch identity.
    pub const fn authority(&self) -> ExtensionAuthorityId {
        self.authority
    }

    /// Returns the signed expected package-admission policy digest.
    pub const fn admission_policy_sha256(&self) -> ExtensionPackageAdmissionPolicyDigest {
        self.admission_policy_sha256
    }

    /// Binds this structural catalog to the product-owned admission policy.
    ///
    /// The returned witness does not authenticate the catalog bytes. The
    /// package authority must separately establish that exact catalog bytes
    /// came from its trusted release channel.
    pub fn bind_admission_policy(
        &self,
        policy: &ExtensionReleaseAdmissionPolicy,
    ) -> Result<ExtensionReleasePolicyBinding<'_>, ExtensionReleaseCatalogError> {
        if self.admission_policy_sha256 != policy.digest {
            return Err(ExtensionReleaseCatalogError::AdmissionPolicyMismatch);
        }
        for package in &self.packages {
            let provenance = &package.provenance;
            let rule = policy
                .license_rule(&provenance.license_expression)
                .ok_or(ExtensionReleaseCatalogError::LicenseNotAllowed)?;
            if rule.corresponding_source_required && provenance.corresponding_source.is_none() {
                return Err(ExtensionReleaseCatalogError::CorrespondingSourceRequired);
            }
        }
        Ok(ExtensionReleasePolicyBinding { catalog: self })
    }

    /// Returns canonical package entries ordered by update-line key.
    pub fn packages(&self) -> &[ExtensionReleasePackage] {
        &self.packages
    }

    /// Finds one exact update-line key without allocating.
    pub fn package(&self, key: ExtensionPackageKey) -> Option<&ExtensionReleasePackage> {
        self.packages
            .binary_search_by_key(&key, |package| package.identity.key())
            .ok()
            .map(|index| &self.packages[index])
    }

    /// Returns SHA-256 of exact canonical catalog bytes.
    pub const fn digest(&self) -> ExtensionReleaseCatalogDigest {
        self.digest
    }

    /// Returns logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// Structural catalog paired with the exact expected admission policy.
///
/// This is a policy-equality witness, not a package-authority authentication
/// or activation lease.
#[derive(Clone, Copy, Debug)]
pub struct ExtensionReleasePolicyBinding<'a> {
    catalog: &'a ExtensionReleaseCatalog,
}

impl<'a> ExtensionReleasePolicyBinding<'a> {
    /// Returns the structurally parsed catalog whose policy digest matched.
    pub const fn catalog(self) -> &'a ExtensionReleaseCatalog {
        self.catalog
    }
}

fn validate_provenance(
    raw: RawProvenance,
) -> Result<ExtensionReleasePackageProvenance, ExtensionReleaseCatalogError> {
    if !valid_https_source_url(&raw.source_url) {
        return Err(ExtensionReleaseCatalogError::InvalidSourceUrl);
    }
    if !valid_bounded_printable(&raw.upstream_version, MAX_UPSTREAM_VERSION_BYTES) {
        return Err(ExtensionReleaseCatalogError::InvalidUpstreamVersion);
    }
    if !matches!(raw.upstream_revision.len(), 40 | 64)
        || !raw
            .upstream_revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(ExtensionReleaseCatalogError::InvalidUpstreamRevision);
    }
    if !valid_bounded_printable(&raw.license_expression, MAX_LICENSE_EXPRESSION_BYTES) {
        return Err(ExtensionReleaseCatalogError::InvalidLicenseExpression);
    }
    if !valid_bounded_printable(&raw.attribution, MAX_LICENSE_ATTRIBUTION_BYTES) {
        return Err(ExtensionReleaseCatalogError::InvalidAttribution);
    }
    if !valid_bounded_printable(&raw.redistribution, MAX_REDISTRIBUTION_RECORD_BYTES) {
        return Err(ExtensionReleaseCatalogError::InvalidRedistribution);
    }
    let notice_target = PortableRelativePath::parse(&raw.legal_notice.target)
        .map_err(|_| ExtensionReleaseCatalogError::InvalidLegalNotice)?;
    if !notice_target.as_str().starts_with("licenses/")
        || raw.legal_notice.length == 0
        || raw.legal_notice.length > MAX_EXTENSION_LEGAL_NOTICE_BYTES
    {
        return Err(ExtensionReleaseCatalogError::InvalidLegalNotice);
    }
    let notice_sha256 = decode_lower_hex_32(&raw.legal_notice.sha256)
        .map_err(|_| ExtensionReleaseCatalogError::InvalidLegalNotice)?;
    let corresponding_source = raw
        .corresponding_source
        .map(|source| {
            if !valid_https_source_url(&source.url)
                || !valid_git_revision(&source.revision)
                || source.revision != raw.upstream_revision
            {
                return Err(ExtensionReleaseCatalogError::InvalidCorrespondingSource);
            }
            Ok(ExtensionReleaseSourceReference {
                url: source.url.into_boxed_str(),
                revision: source.revision.into_boxed_str(),
            })
        })
        .transpose()?;
    Ok(ExtensionReleasePackageProvenance {
        source_url: raw.source_url.into_boxed_str(),
        upstream_version: raw.upstream_version.into_boxed_str(),
        upstream_revision: raw.upstream_revision.into_boxed_str(),
        license_expression: raw.license_expression.into_boxed_str(),
        attribution: raw.attribution.into_boxed_str(),
        redistribution: raw.redistribution.into_boxed_str(),
        legal_notice: ExtensionReleaseLegalNotice {
            target: notice_target,
            kind: match raw.legal_notice.kind {
                RawLegalArtifactKind::NoticeBundle => {
                    ExtensionReleaseLegalArtifactKind::NoticeBundle
                }
            },
            length: raw.legal_notice.length,
            sha256: notice_sha256,
        },
        corresponding_source,
    })
}

fn valid_git_revision(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_bounded_printable(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.is_ascii()
        && value.bytes().all(|byte| !byte.is_ascii_control())
        && value.as_bytes().first() != Some(&b' ')
        && value.as_bytes().last() != Some(&b' ')
}

fn valid_https_source_url(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_SOURCE_URL_BYTES || !value.is_ascii() {
        return false;
    }
    let Ok(parsed) = Url::parse(value) else {
        return false;
    };
    parsed.as_str() == value
        && parsed.scheme() == "https"
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.port().is_none()
        && matches!(parsed.host(), Some(Host::Domain(_)))
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.path() != "/"
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawReleaseCatalog {
    schema_version: u32,
    catalog_revision: u64,
    created_unix: u64,
    authority_id: String,
    admission_policy_sha256: String,
    packages: Vec<RawReleasePackage>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawReleasePackage {
    package_key: String,
    revision: u64,
    payload: RawPackagePayload,
    manifest_sha256: String,
    tree_sha256: String,
    tree_index_sha256: String,
    tree_index_length: u64,
    tree_file_count: u32,
    tree_bytes: u64,
    chromium: Option<RawChromiumIdentity>,
    provenance: RawProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RawPackagePayload {
    BundledTree,
    AcquiredZip { length: u64, sha256: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawChromiumIdentity {
    manifest_key_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawProvenance {
    source_url: String,
    upstream_version: String,
    upstream_revision: String,
    license_expression: String,
    attribution: String,
    redistribution: String,
    legal_notice: RawLegalNotice,
    corresponding_source: Option<RawSourceReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawLegalNotice {
    target: String,
    kind: RawLegalArtifactKind,
    length: u64,
    sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RawLegalArtifactKind {
    NoticeBundle,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawSourceReference {
    url: String,
    revision: String,
}

/// Stable release-catalog rejection reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionReleaseCatalogError {
    /// The bounded JSON boundary rejected source bytes.
    Json(BoundedJsonError),
    /// Typed decoding failed or unknown fields were present.
    Malformed,
    /// JSON is valid but not the one canonical serialized representation.
    NonCanonical,
    /// The catalog schema version is unsupported.
    UnsupportedSchema,
    /// Catalog revision is zero or exceeds the durable signed-integer range.
    InvalidCatalogRevision,
    /// Release creation time is zero or outside the admitted range.
    InvalidCreationTime,
    /// Package count is empty or exceeds its ceiling.
    PackageCount {
        /// Observed packages.
        count: usize,
        /// Maximum packages.
        max: usize,
    },
    /// A SHA-256 field is not exact lowercase hexadecimal.
    Digest,
    /// Package rows are not strictly ordered by unique update-line key.
    NonCanonicalPackageOrder,
    /// Package revision is outside the core durable range.
    InvalidPackageRevision,
    /// Acquired archive bytes are zero or exceed their ceiling.
    ArchiveSize {
        /// Observed bytes.
        bytes: u64,
        /// Maximum bytes.
        max: u64,
    },
    /// Signed canonical tree-index bytes are zero or exceed their ceiling.
    TreeIndexSize {
        /// Observed bytes.
        bytes: u64,
        /// Maximum bytes.
        max: u64,
    },
    /// Signed tree file count is zero or exceeds its ceiling.
    TreeFileCount {
        /// Observed files.
        count: u32,
        /// Maximum files.
        max: usize,
    },
    /// Signed tree bytes are zero or exceed their ceiling.
    TreeSize {
        /// Observed bytes.
        bytes: u64,
        /// Maximum bytes.
        max: u64,
    },
    /// Aggregate catalog tree bytes overflow or exceed the curated ceiling.
    CatalogTreeSize {
        /// Observed aggregate bytes.
        bytes: u64,
        /// Maximum aggregate bytes.
        max: u64,
    },
    /// Signed expected Chromium identity is not canonical.
    ChromiumIdentity,
    /// Distinct update lines claim the same Chromium native identity.
    DuplicateChromiumIdentity,
    /// Catalog policy digest does not match the product-owned expected policy.
    AdmissionPolicyMismatch,
    /// A package license expression is absent from trusted product policy.
    LicenseNotAllowed,
    /// Trusted product policy requires corresponding-source evidence.
    CorrespondingSourceRequired,
    /// Provenance source reference is not an exact bounded HTTPS URL.
    InvalidSourceUrl,
    /// Upstream version text is empty, non-ASCII, or oversized.
    InvalidUpstreamVersion,
    /// Upstream revision is not a lowercase 40- or 64-character Git hash.
    InvalidUpstreamRevision,
    /// License expression is empty, non-ASCII, or oversized.
    InvalidLicenseExpression,
    /// Attribution is empty, non-ASCII, or oversized.
    InvalidAttribution,
    /// Redistribution terms are empty, non-ASCII, or oversized.
    InvalidRedistribution,
    /// The legal-notice target, length, or digest is invalid.
    InvalidLegalNotice,
    /// Corresponding-source URL/revision evidence is invalid or mismatched.
    InvalidCorrespondingSource,
    /// Supplied legal-notice bytes do not match their exact release binding.
    LegalNoticeMismatch,
    /// Legal-notice targets alias or conflict as files and directories.
    ConflictingLegalNoticeTarget,
    /// A parsed tree index does not match the signed index/tree/count metrics.
    TreeIndexMismatch,
    /// Indexed root manifest bytes do not match signed package identity.
    ManifestMismatch,
    /// Logical memory accounting overflowed.
    AccountingOverflow,
    /// Logical retained memory exceeds its ceiling.
    RetainedBytes {
        /// Observed retained bytes.
        bytes: usize,
        /// Maximum retained bytes.
        max: usize,
    },
}

impl fmt::Display for ExtensionReleaseCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "extension release JSON is invalid: {error}"),
            Self::Malformed => formatter.write_str("extension release catalog is malformed"),
            Self::NonCanonical => formatter.write_str("extension release catalog is not canonical"),
            Self::UnsupportedSchema => {
                formatter.write_str("extension release catalog schema is unsupported")
            }
            Self::InvalidCatalogRevision => {
                formatter.write_str("extension release catalog revision is invalid")
            }
            Self::InvalidCreationTime => {
                formatter.write_str("extension release catalog creation time is invalid")
            }
            Self::PackageCount { count, max } => write!(
                formatter,
                "extension release catalog has {count} packages; maximum is {max}"
            ),
            Self::Digest => formatter.write_str("extension release digest is invalid"),
            Self::NonCanonicalPackageOrder => {
                formatter.write_str("extension release packages are not strictly ordered")
            }
            Self::InvalidPackageRevision => {
                formatter.write_str("extension release package revision is invalid")
            }
            Self::ArchiveSize { bytes, max } => write!(
                formatter,
                "extension release archive uses {bytes} bytes; maximum is {max}"
            ),
            Self::TreeIndexSize { bytes, max } => write!(
                formatter,
                "extension release tree index uses {bytes} bytes; maximum is {max}"
            ),
            Self::TreeFileCount { count, max } => write!(
                formatter,
                "extension release tree has {count} files; maximum is {max}"
            ),
            Self::TreeSize { bytes, max } => write!(
                formatter,
                "extension release tree uses {bytes} bytes; maximum is {max}"
            ),
            Self::CatalogTreeSize { bytes, max } => write!(
                formatter,
                "extension release catalog trees use {bytes} bytes; maximum is {max}"
            ),
            Self::ChromiumIdentity => {
                formatter.write_str("extension release Chromium identity is invalid")
            }
            Self::DuplicateChromiumIdentity => {
                formatter.write_str("extension release Chromium identities are not unique")
            }
            Self::AdmissionPolicyMismatch => {
                formatter.write_str("extension release admission policy does not match")
            }
            Self::LicenseNotAllowed => {
                formatter.write_str("extension release license is not allowed by product policy")
            }
            Self::CorrespondingSourceRequired => formatter.write_str(
                "extension release package lacks required corresponding-source evidence",
            ),
            Self::InvalidSourceUrl => {
                formatter.write_str("extension release source URL is invalid")
            }
            Self::InvalidUpstreamVersion => {
                formatter.write_str("extension release upstream version is invalid")
            }
            Self::InvalidUpstreamRevision => {
                formatter.write_str("extension release upstream revision is invalid")
            }
            Self::InvalidLicenseExpression => {
                formatter.write_str("extension release license expression is invalid")
            }
            Self::InvalidAttribution => {
                formatter.write_str("extension release attribution is invalid")
            }
            Self::InvalidRedistribution => {
                formatter.write_str("extension release redistribution record is invalid")
            }
            Self::InvalidLegalNotice => {
                formatter.write_str("extension release legal-notice binding is invalid")
            }
            Self::InvalidCorrespondingSource => {
                formatter.write_str("extension release corresponding-source evidence is invalid")
            }
            Self::LegalNoticeMismatch => {
                formatter.write_str("extension release legal-notice bytes do not match")
            }
            Self::ConflictingLegalNoticeTarget => formatter
                .write_str("extension release legal-notice targets conflict across platforms"),
            Self::TreeIndexMismatch => {
                formatter.write_str("extension release tree index does not match package identity")
            }
            Self::ManifestMismatch => {
                formatter.write_str("extension release manifest does not match package identity")
            }
            Self::AccountingOverflow => {
                formatter.write_str("extension release catalog memory accounting overflowed")
            }
            Self::RetainedBytes { bytes, max } => write!(
                formatter,
                "extension release catalog retains {bytes} bytes; maximum is {max}"
            ),
        }
    }
}

impl Error for ExtensionReleaseCatalogError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChromiumManifestKey;

    fn hex(bytes: [u8; 32]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn canonical_tree() -> CanonicalExtensionTreeIndex {
        let bytes = format!(
            concat!(
                r#"{{"schema_version":1,"files":["#,
                r#"{{"path":"manifest.json","length":4,"sha256":"{}"}},"#,
                r#"{{"path":"script.js","length":7,"sha256":"{}"}}]}}"#
            ),
            hex([3; 32]),
            hex([4; 32])
        )
        .into_bytes();
        CanonicalExtensionTreeIndex::parse_canonical(&bytes).unwrap()
    }

    fn raw_catalog(tree: &CanonicalExtensionTreeIndex) -> RawReleaseCatalog {
        let key = ChromiumManifestKey::parse_canonical("Xw==").unwrap();
        RawReleaseCatalog {
            schema_version: RELEASE_CATALOG_SCHEMA_VERSION,
            catalog_revision: 1,
            created_unix: 1_700_000_000,
            authority_id: hex([1; 32]),
            admission_policy_sha256: hex([2; 32]),
            packages: vec![RawReleasePackage {
                package_key: hex([5; 32]),
                revision: 1,
                payload: RawPackagePayload::BundledTree,
                manifest_sha256: hex(tree.manifest_sha256().bytes()),
                tree_sha256: hex(tree.tree_sha256().bytes()),
                tree_index_sha256: hex(tree.index_sha256().bytes()),
                tree_index_length: tree.index_bytes(),
                tree_file_count: tree.files().len() as u32,
                tree_bytes: tree.total_bytes(),
                chromium: Some(RawChromiumIdentity {
                    manifest_key_sha256: hex(key.digest().bytes()),
                }),
                provenance: RawProvenance {
                    source_url:
                        "https://github.com/example/project/releases/download/v1/package.zip"
                            .to_owned(),
                    upstream_version: "1.0.0".to_owned(),
                    upstream_revision: "a".repeat(40),
                    license_expression: "GPL-3.0-only".to_owned(),
                    attribution: "Bitwarden, Inc. and contributors".to_owned(),
                    redistribution: "Reviewed unmodified upstream release".to_owned(),
                    legal_notice: RawLegalNotice {
                        target: "licenses/example-extension.txt".to_owned(),
                        kind: RawLegalArtifactKind::NoticeBundle,
                        length: 128,
                        sha256: hex([7; 32]),
                    },
                    corresponding_source: Some(RawSourceReference {
                        url: "https://github.com/example/project/tree/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_owned(),
                        revision: "a".repeat(40),
                    }),
                },
            }],
        }
    }

    fn admission_policy(
        digest_byte: u8,
        license: &str,
        corresponding_source_required: bool,
    ) -> ExtensionReleaseAdmissionPolicy {
        ExtensionReleaseAdmissionPolicy::new(
            ExtensionPackageAdmissionPolicyDigest::from_bytes([digest_byte; 32]),
            vec![ExtensionReleaseLicenseRule::new(license, corresponding_source_required).unwrap()],
        )
        .unwrap()
    }

    #[test]
    fn parses_and_binds_one_exact_release_package() {
        let tree = canonical_tree();
        let bytes = serde_json::to_vec(&raw_catalog(&tree)).unwrap();
        let catalog = ExtensionReleaseCatalog::parse_canonical(&bytes).unwrap();
        assert_eq!(catalog.revision(), ExtensionReleaseCatalogRevision::INITIAL);
        assert_eq!(catalog.packages().len(), 1);
        let package = &catalog.packages()[0];
        assert_eq!(
            package.payload(),
            ExtensionPackagePayloadIdentity::BundledTree
        );
        assert_eq!(package.tree_index_length(), tree.index_bytes());
        let binding = package.bind_tree_index(&tree).unwrap();
        assert_eq!(binding.package().identity(), package.identity());
        assert_eq!(binding.index(), &tree);
        assert!(catalog.retained_bytes() <= MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES);
    }

    #[test]
    fn canonical_release_schema_has_a_fixed_cross_version_golden() {
        const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
        let golden = format!(
            concat!(
                r#"{{"schema_version":1,"catalog_revision":1,"created_unix":1,"authority_id":"{0}","admission_policy_sha256":"{0}","packages":[{{"package_key":"{0}","revision":1,"payload":{{"kind":"bundled_tree"}},"manifest_sha256":"{0}","tree_sha256":"{0}","tree_index_sha256":"{0}","tree_index_length":1,"tree_file_count":1,"tree_bytes":1,"chromium":null,"provenance":{{"source_url":"https://example.com/releases/v1/source","upstream_version":"1","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Example","redistribution":"Reviewed","legal_notice":{{"target":"licenses/example.txt","kind":"notice_bundle","length":1,"sha256":"{0}"}},"corresponding_source":null}}}}]}}"#,
            ),
            ZERO,
        );
        let catalog = ExtensionReleaseCatalog::parse_canonical(golden.as_bytes()).unwrap();
        assert_eq!(catalog.packages().len(), 1);
        assert_eq!(
            catalog.packages()[0].payload(),
            ExtensionPackagePayloadIdentity::BundledTree
        );
    }

    #[test]
    fn acquired_zip_payload_binds_exact_length_and_digest_without_affecting_bundled_trees() {
        const ZIP_DIGEST: &str = "0606060606060606060606060606060606060606060606060606060606060606";
        assert_eq!(
            serde_json::to_string(&RawPackagePayload::AcquiredZip {
                length: 19,
                sha256: ZIP_DIGEST.to_owned(),
            })
            .unwrap(),
            format!(r#"{{"kind":"acquired_zip","length":19,"sha256":"{ZIP_DIGEST}"}}"#),
            "acquired-ZIP v1 canonical representation changed"
        );

        let tree = canonical_tree();
        let mut raw = raw_catalog(&tree);
        raw.packages[0].payload = RawPackagePayload::AcquiredZip {
            length: 19,
            sha256: hex([6; 32]),
        };
        let catalog =
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()).unwrap();
        let (length, digest) = catalog.packages()[0]
            .payload()
            .acquired_zip_evidence()
            .unwrap();
        assert_eq!(length.get(), 19);
        assert_eq!(digest.bytes(), [6; 32]);
    }

    #[test]
    fn catalog_revision_owns_the_durable_high_water_domain() {
        assert_eq!(ExtensionReleaseCatalogRevision::new(0), None);
        let maximum = ExtensionReleaseCatalogRevision::new(i64::MAX as u64).unwrap();
        assert_eq!(maximum.get(), i64::MAX as u64);
        assert_eq!(maximum.next(), None);
        assert_eq!(
            ExtensionReleaseCatalogRevision::INITIAL
                .next()
                .unwrap()
                .get(),
            2
        );
    }

    #[test]
    fn policy_binding_requires_the_exact_product_owned_digest() {
        let tree = canonical_tree();
        let bytes = serde_json::to_vec(&raw_catalog(&tree)).unwrap();
        let catalog = ExtensionReleaseCatalog::parse_canonical(&bytes).unwrap();
        let expected = admission_policy(2, "GPL-3.0-only", true);
        let binding = catalog.bind_admission_policy(&expected).unwrap();
        assert_eq!(binding.catalog().digest(), catalog.digest());

        let mismatched = admission_policy(3, "GPL-3.0-only", true);
        assert_eq!(
            catalog.bind_admission_policy(&mismatched).unwrap_err(),
            ExtensionReleaseCatalogError::AdmissionPolicyMismatch
        );

        let disallowed = admission_policy(2, "MPL-2.0", false);
        assert_eq!(
            catalog.bind_admission_policy(&disallowed).unwrap_err(),
            ExtensionReleaseCatalogError::LicenseNotAllowed
        );

        let mut raw = raw_catalog(&tree);
        raw.packages[0].provenance.corresponding_source = None;
        let catalog =
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()).unwrap();
        assert_eq!(
            catalog.bind_admission_policy(&expected).unwrap_err(),
            ExtensionReleaseCatalogError::CorrespondingSourceRequired
        );
    }

    #[test]
    fn admission_policy_rules_are_exact_bounded_and_unique() {
        assert_eq!(
            ExtensionReleaseLicenseRule::new("", false),
            Err(ExtensionReleaseAdmissionPolicyError::InvalidLicenseExpression)
        );
        let rule = ExtensionReleaseLicenseRule::new("GPL-3.0-only", true).unwrap();
        assert_eq!(
            ExtensionReleaseAdmissionPolicy::new(
                ExtensionPackageAdmissionPolicyDigest::from_bytes([1; 32]),
                vec![rule.clone(), rule],
            ),
            Err(ExtensionReleaseAdmissionPolicyError::DuplicateLicenseExpression)
        );
        assert_eq!(
            ExtensionReleaseAdmissionPolicy::new(
                ExtensionPackageAdmissionPolicyDigest::from_bytes([1; 32]),
                Vec::new(),
            ),
            Err(ExtensionReleaseAdmissionPolicyError::LicenseRuleCount {
                count: 0,
                max: MAX_EXTENSION_LICENSE_RULES,
            })
        );
    }

    #[test]
    fn chromium_id_is_derived_and_exact_manifest_key_binding_is_required() {
        let tree = canonical_tree();
        let bytes = serde_json::to_vec(&raw_catalog(&tree)).unwrap();
        let catalog = ExtensionReleaseCatalog::parse_canonical(&bytes).unwrap();
        let expected = catalog.packages()[0].chromium().unwrap();
        let key = ChromiumManifestKey::parse_canonical("Xw==").unwrap();
        assert_eq!(expected.extension_id(), key.extension_id());
        assert_eq!(expected.verify_manifest_key(&key), Ok(()));
        let other = ChromiumManifestKey::parse_canonical("WA==").unwrap();
        assert_eq!(
            expected.verify_manifest_key(&other),
            Err(ExtensionReleaseCatalogError::ChromiumIdentity)
        );
    }

    #[test]
    fn catalog_rejects_duplicate_json_keys_and_unknown_fields() {
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(br#"{"schema_version":1,"schema_version":1}"#),
            Err(ExtensionReleaseCatalogError::Json(
                BoundedJsonError::DuplicateKey
            ))
        );
        let tree = canonical_tree();
        let mut value = serde_json::to_value(raw_catalog(&tree)).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_owned(), serde_json::Value::Bool(true));
        let bytes = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&bytes),
            Err(ExtensionReleaseCatalogError::Malformed)
        );

        let mut value = serde_json::to_value(raw_catalog(&tree)).unwrap();
        value["packages"][0]
            .as_object_mut()
            .unwrap()
            .insert("tree_index_bytes".to_owned(), serde_json::Value::from(1));
        let bytes = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&bytes),
            Err(ExtensionReleaseCatalogError::Malformed),
            "release-package rows must deny unknown tree-index fields"
        );

        let mut value = serde_json::to_value(raw_catalog(&tree)).unwrap();
        value["packages"][0]
            .as_object_mut()
            .unwrap()
            .remove("tree_index_length");
        let bytes = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&bytes),
            Err(ExtensionReleaseCatalogError::Malformed),
            "tree_index_length is required in every release-package row"
        );
    }

    #[test]
    fn catalog_order_prevents_duplicate_update_lines() {
        let tree = canonical_tree();
        let mut raw = raw_catalog(&tree);
        raw.packages.push(raw.packages[0].clone());
        let bytes = serde_json::to_vec(&raw).unwrap();
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&bytes),
            Err(ExtensionReleaseCatalogError::NonCanonicalPackageOrder)
        );
    }

    #[test]
    fn binding_checks_index_tree_metrics_and_manifest_independently() {
        let tree = canonical_tree();
        let index_mutations: [fn(&mut RawReleasePackage); 5] = [
            |package| package.tree_index_sha256 = hex([98; 32]),
            |package| package.tree_index_length += 1,
            |package| package.tree_sha256 = hex([98; 32]),
            |package| package.tree_file_count += 1,
            |package| package.tree_bytes += 1,
        ];
        for mutate in index_mutations {
            let mut raw = raw_catalog(&tree);
            mutate(&mut raw.packages[0]);
            let bytes = serde_json::to_vec(&raw).unwrap();
            let catalog = ExtensionReleaseCatalog::parse_canonical(&bytes).unwrap();
            assert_eq!(
                catalog.packages()[0].bind_tree_index(&tree).unwrap_err(),
                ExtensionReleaseCatalogError::TreeIndexMismatch
            );
        }

        let mut raw = raw_catalog(&tree);
        raw.packages[0].manifest_sha256 = hex([99; 32]);
        let bytes = serde_json::to_vec(&raw).unwrap();
        let catalog = ExtensionReleaseCatalog::parse_canonical(&bytes).unwrap();
        assert_eq!(
            catalog.packages()[0].bind_tree_index(&tree).unwrap_err(),
            ExtensionReleaseCatalogError::ManifestMismatch
        );
    }

    #[test]
    fn noncanonical_whitespace_and_query_bearing_urls_are_refused() {
        let tree = canonical_tree();
        let canonical = serde_json::to_vec(&raw_catalog(&tree)).unwrap();
        let mut spaced = canonical.clone();
        spaced.insert(1, b' ');
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&spaced),
            Err(ExtensionReleaseCatalogError::NonCanonical)
        );

        let mut raw = raw_catalog(&tree);
        raw.packages[0].provenance.source_url =
            "https://github.com/example/project/releases/latest?asset=x".to_owned();
        let bytes = serde_json::to_vec(&raw).unwrap();
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&bytes),
            Err(ExtensionReleaseCatalogError::InvalidSourceUrl)
        );
    }

    #[test]
    fn provenance_url_requires_exact_canonical_https_domain_form() {
        let tree = canonical_tree();
        let invalid = [
            "http://github.com/example/package.zip",
            "https://github.com/",
            "https://GITHUB.com/example/package.zip",
            "https://github.com:443/example/package.zip",
            "https://user@github.com/example/package.zip",
            "https://user:secret@github.com/example/package.zip",
            "https://127.0.0.1/example/package.zip",
            "https://[::1]/example/package.zip",
            "https://github.com/example/package.zip?download=1",
            "https://github.com/example/package.zip#asset",
            "https://github.com/example package.zip",
            "https://github.com\\example/package.zip",
        ];
        for source_url in invalid {
            let mut raw = raw_catalog(&tree);
            raw.packages[0].provenance.source_url = source_url.to_owned();
            let bytes = serde_json::to_vec(&raw).unwrap();
            assert_eq!(
                ExtensionReleaseCatalog::parse_canonical(&bytes),
                Err(ExtensionReleaseCatalogError::InvalidSourceUrl),
                "unexpectedly admitted {source_url}"
            );
        }
    }

    #[test]
    fn legal_provenance_and_notice_binding_are_mandatory_and_bounded() {
        let tree = canonical_tree();

        let notice = b"exact reviewed legal notice";
        let mut raw = raw_catalog(&tree);
        raw.packages[0].provenance.legal_notice.length = notice.len() as u64;
        raw.packages[0].provenance.legal_notice.sha256 =
            hex(<[u8; 32]>::from(Sha256::digest(notice)));
        let catalog =
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()).unwrap();
        let binding = catalog.packages()[0].provenance().legal_notice();
        assert_eq!(binding.verify_bytes(notice), Ok(()));
        assert_eq!(
            binding.verify_bytes(b"wrong"),
            Err(ExtensionReleaseCatalogError::LegalNoticeMismatch)
        );

        let mut raw = raw_catalog(&tree);
        raw.packages[0].provenance.attribution.clear();
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::InvalidAttribution)
        );

        let mut raw = raw_catalog(&tree);
        raw.packages[0].provenance.redistribution = "approval\nforged".to_owned();
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::InvalidRedistribution)
        );

        type ProvenanceTextMutation = (fn(&mut RawProvenance), ExtensionReleaseCatalogError);
        let text_mutators: [ProvenanceTextMutation; 4] = [
            (
                |provenance| provenance.upstream_version = "   ".to_owned(),
                ExtensionReleaseCatalogError::InvalidUpstreamVersion,
            ),
            (
                |provenance| provenance.license_expression = "   ".to_owned(),
                ExtensionReleaseCatalogError::InvalidLicenseExpression,
            ),
            (
                |provenance| provenance.attribution = "   ".to_owned(),
                ExtensionReleaseCatalogError::InvalidAttribution,
            ),
            (
                |provenance| provenance.redistribution = "   ".to_owned(),
                ExtensionReleaseCatalogError::InvalidRedistribution,
            ),
        ];
        for (mutate, expected) in text_mutators {
            let mut raw = raw_catalog(&tree);
            mutate(&mut raw.packages[0].provenance);
            assert_eq!(
                ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
                Err(expected)
            );
        }
        assert_eq!(
            ExtensionReleaseLicenseRule::new("   ", false),
            Err(ExtensionReleaseAdmissionPolicyError::InvalidLicenseExpression)
        );

        let mutators: [fn(&mut RawLegalNotice); 3] = [
            |notice: &mut RawLegalNotice| notice.target = "notice.txt".to_owned(),
            |notice: &mut RawLegalNotice| notice.length = 0,
            |notice: &mut RawLegalNotice| notice.sha256 = "A".repeat(64),
        ];
        for mutate in mutators {
            let mut raw = raw_catalog(&tree);
            mutate(&mut raw.packages[0].provenance.legal_notice);
            assert_eq!(
                ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
                Err(ExtensionReleaseCatalogError::InvalidLegalNotice)
            );
        }

        let mut raw = raw_catalog(&tree);
        raw.packages[0]
            .provenance
            .corresponding_source
            .as_mut()
            .unwrap()
            .revision = "b".repeat(40);
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::InvalidCorrespondingSource)
        );
    }

    #[test]
    fn catalog_rejects_native_identity_and_notice_alias_conflicts() {
        let tree = canonical_tree();
        let mut raw = raw_catalog(&tree);
        let mut duplicate = raw.packages[0].clone();
        duplicate.package_key = hex([6; 32]);
        raw.packages.push(duplicate);
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::DuplicateChromiumIdentity)
        );

        let mut raw = raw_catalog(&tree);
        let mut conflicting = raw.packages[0].clone();
        conflicting.package_key = hex([6; 32]);
        conflicting.chromium = None;
        conflicting.provenance.legal_notice.target = "licenses/Example-Extension.txt".to_owned();
        conflicting.provenance.legal_notice.sha256 = hex([8; 32]);
        raw.packages.push(conflicting);
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::ConflictingLegalNoticeTarget)
        );

        let mut raw = raw_catalog(&tree);
        raw.packages[0].provenance.legal_notice.target = "licenses/A/b.txt".to_owned();
        let mut conflicting = raw.packages[0].clone();
        conflicting.package_key = hex([6; 32]);
        conflicting.chromium = None;
        conflicting.provenance.legal_notice.target = "licenses/a".to_owned();
        raw.packages.push(conflicting);
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::ConflictingLegalNoticeTarget),
            "a legal notice cannot be both a file and a shared-resource directory"
        );
    }

    #[test]
    fn release_resource_limits_fail_at_the_catalog_boundary() {
        let tree = canonical_tree();

        let mut raw = raw_catalog(&tree);
        raw.catalog_revision = 0;
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::InvalidCatalogRevision)
        );

        let mut raw = raw_catalog(&tree);
        raw.packages[0].payload = RawPackagePayload::AcquiredZip {
            length: MAX_EXTENSION_ARCHIVE_BYTES + 1,
            sha256: hex([6; 32]),
        };
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::ArchiveSize {
                bytes: MAX_EXTENSION_ARCHIVE_BYTES + 1,
                max: MAX_EXTENSION_ARCHIVE_BYTES,
            })
        );

        let maximum_tree_index_bytes = MAX_EXTENSION_TREE_INDEX_BYTES as u64;
        let mut raw = raw_catalog(&tree);
        raw.packages[0].tree_index_length = maximum_tree_index_bytes;
        let catalog = ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap())
            .expect("the exact tree-index size ceiling is admissible");
        assert_eq!(
            catalog.packages()[0].tree_index_length(),
            maximum_tree_index_bytes
        );

        for invalid_length in [0, maximum_tree_index_bytes + 1] {
            let mut raw = raw_catalog(&tree);
            raw.packages[0].tree_index_length = invalid_length;
            assert_eq!(
                ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
                Err(ExtensionReleaseCatalogError::TreeIndexSize {
                    bytes: invalid_length,
                    max: maximum_tree_index_bytes,
                })
            );
        }

        let mut raw = raw_catalog(&tree);
        raw.packages[0].tree_file_count = u32::try_from(MAX_EXTENSION_TREE_FILES + 1).unwrap();
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::TreeFileCount {
                count: u32::try_from(MAX_EXTENSION_TREE_FILES + 1).unwrap(),
                max: MAX_EXTENSION_TREE_FILES,
            })
        );

        let mut raw = raw_catalog(&tree);
        raw.packages[0].tree_bytes = MAX_EXTENSION_TREE_BYTES + 1;
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::TreeSize {
                bytes: MAX_EXTENSION_TREE_BYTES + 1,
                max: MAX_EXTENSION_TREE_BYTES,
            })
        );

        let mut raw = raw_catalog(&tree);
        let package = raw.packages[0].clone();
        raw.packages = (0..=MAX_EXTENSION_PACKAGE_LINES)
            .map(|index| {
                let mut package = package.clone();
                package.package_key = hex([u8::try_from(index + 1).unwrap(); 32]);
                package
            })
            .collect();
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::PackageCount {
                count: MAX_EXTENSION_PACKAGE_LINES + 1,
                max: MAX_EXTENSION_PACKAGE_LINES,
            })
        );

        let mut raw = raw_catalog(&tree);
        let package = raw.packages[0].clone();
        raw.packages = (1_u8..=3)
            .map(|key| {
                let mut package = package.clone();
                package.package_key = hex([key; 32]);
                package.tree_bytes = MAX_EXTENSION_TREE_BYTES;
                package.chromium = None;
                package
            })
            .collect();
        let aggregate = MAX_EXTENSION_TREE_BYTES * 3;
        assert_eq!(
            ExtensionReleaseCatalog::parse_canonical(&serde_json::to_vec(&raw).unwrap()),
            Err(ExtensionReleaseCatalogError::CatalogTreeSize {
                bytes: aggregate,
                max: MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES,
            })
        );
    }
}
