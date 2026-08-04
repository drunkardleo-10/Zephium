//! Product-sealed compatibility and admission for extension manifests.
//!
//! [`zephium_extension_package::admit_extension_manifest`] deliberately
//! remains a structural parser: its caller supplies the compatibility policy,
//! so neither that function nor its output can authorize native activation.
//! This module is the product authority layered above it. A successful witness
//! binds an authenticated bundled catalog generation, one exact package
//! selected from that catalog, an exact canonical tree, exact manifest bytes,
//! a closed backend target, and Zephium-owned versioned compatibility data.
//! Active and rollback catalogs mint different manifest witness types so an
//! ordinary activation adapter cannot consume rollback authority by accident.

use std::fmt;
use std::mem::size_of;

use thiserror::Error;
use zephium_core::extensions::{
    ExtensionAuthorityId, ExtensionCompatibilityLevel, ExtensionCompatibilityProfileDigest,
    ExtensionCompatibilityTargetId, ExtensionManifestDeclaration, ExtensionManifestDescriptor,
    ExtensionManifestDigest, ExtensionPackageIdentity, ExtensionPackageKey,
    ExtensionPackageRevision, ExtensionTreeDigest, MAX_EXTENSION_MANIFEST_DECLARATIONS,
    MAX_EXTENSION_MANIFEST_RETAINED_BYTES,
};
#[cfg(zephium_internal_repository_e2e)]
use zephium_extension_package::ExtensionReleaseCatalog;
use zephium_extension_package::{
    admit_extension_manifest, AdmittedExtensionManifest, CanonicalExtensionTreeIndex,
    ChromiumManifestKey, ExtensionManifestAdmissionDigest, ExtensionManifestAdmissionError,
    ExtensionManifestCompatibilityPolicy, ExtensionManifestCompatibilitySubject,
    ExtensionManifestMetadata, ExtensionManifestResourcePlan, ExtensionReleaseCatalogDigest,
    ExtensionReleaseCatalogRevision, ExtensionTreeIndexDigest, MAX_EXTENSION_MANIFEST_BYTES,
    MAX_EXTENSION_MANIFEST_PLAN_RETAINED_BYTES, MAX_EXTENSION_PACKAGE_LINES,
    MAX_EXTENSION_RELEASE_CATALOG_BYTES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

use crate::product::AdmittedCatalogData;
#[cfg(zephium_internal_repository_e2e)]
use crate::repository_e2e_fixture::{
    ACTIVE_CATALOG_BYTES, MANIFEST_BYTES, PACKAGE_KEY_BYTES, ROLLBACK_CATALOG_BYTES,
    TREE_INDEX_BYTES,
};
use crate::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, BundledCatalogInventoryDigest,
    MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS, MAX_PRODUCT_ROLLBACK_BUNDLED_CATALOGS,
};

#[cfg(test)]
mod tests;

const MAX_PRODUCT_RUNTIME_TARGETS: usize = 4;
const MACOS_NATIVE_COMPATIBILITY_TARGET: &str = "macos.wkwebextension.v1";
const MACOS_COMPATIBILITY_TARGET: &str = "macos.zephium-mv3-compat.v1";
const LINUX_COMPATIBILITY_TARGET: &str = "linux.zephium-mv3-compat.v1";
const WINDOWS_NATIVE_COMPATIBILITY_TARGET: &str = "windows.webview2.v1";
const POLICY_ROW_ACCOUNTING_OVERHEAD: usize = 64;
const PROFILE_ACCOUNTING_OVERHEAD: usize = 512;
const AUTHORITY_ACCOUNTING_OVERHEAD: usize = 512;
const GENERATION_ACCOUNTING_OVERHEAD: usize = 256;
const WITNESS_ACCOUNTING_OVERHEAD: usize = 16 * 1024;

/// Maximum backend/package profiles for one exact catalog generation.
pub const MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION: usize =
    MAX_EXTENSION_PACKAGE_LINES * MAX_PRODUCT_RUNTIME_TARGETS;

/// Stable maximum backend/package profiles for any one catalog generation.
///
/// This existing exported ceiling intentionally remains generation-scoped so
/// downstream package-object bounds do not silently triple when rollback
/// catalogs are provisioned.
pub const MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES: usize =
    MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION;

/// Maximum product-sealed generation/backend/package profiles in one build.
pub const MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_ACROSS_GENERATIONS: usize =
    MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION
        * MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS;

/// Maximum logical bytes retained by profiles for one exact catalog generation.
pub const MAX_PRODUCT_EXTENSION_MANIFEST_GENERATION_RETAINED_BYTES: usize = 512 * 1024;

/// Maximum logical bytes retained by the sealed manifest authority.
pub const MAX_PRODUCT_EXTENSION_MANIFEST_AUTHORITY_RETAINED_BYTES: usize =
    AUTHORITY_ACCOUNTING_OVERHEAD
        + MAX_PRODUCT_ROLLBACK_BUNDLED_CATALOGS * size_of::<SealedManifestCatalogAnchor>()
        + MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS
            * MAX_PRODUCT_EXTENSION_MANIFEST_GENERATION_RETAINED_BYTES;

/// Maximum logical bytes retained by one product-admitted manifest witness.
pub const MAX_PRODUCT_ADMITTED_EXTENSION_MANIFEST_RETAINED_BYTES: usize =
    MAX_EXTENSION_MANIFEST_RETAINED_BYTES
        + MAX_EXTENSION_MANIFEST_PLAN_RETAINED_BYTES
        + MAX_EXTENSION_MANIFEST_BYTES
        + WITNESS_ACCOUNTING_OVERHEAD;

/// Exact extension backend whose reviewed compatibility matrix was applied.
///
/// This selector is intentionally independent of the compile target. A macOS
/// build can contain both native `WKWebExtension` and compatibility-runtime
/// profiles, for example, but a witness for one target grants no authority to
/// the other. Native plan construction must require exact target equality.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum ProductExtensionRuntimeTarget {
    /// Apple's native `WKWebExtension` runtime.
    MacosNative,
    /// Zephium's compatibility runtime hosted by `WKWebView`.
    MacosCompatibility,
    /// Zephium's compatibility runtime hosted by WebKitGTK.
    LinuxCompatibility,
    /// WebView2's native browser-extension runtime.
    WindowsNative,
}

impl ProductExtensionRuntimeTarget {
    /// Returns the only compatibility-profile identifier valid for this backend.
    ///
    /// Durable adapters use this closed mapping when reconstructing metadata;
    /// parsing an otherwise valid identifier is not sufficient because it
    /// could belong to a different native runtime.
    pub const fn compatibility_target_id(self) -> &'static str {
        match self {
            Self::MacosNative => MACOS_NATIVE_COMPATIBILITY_TARGET,
            Self::MacosCompatibility => MACOS_COMPATIBILITY_TARGET,
            Self::LinuxCompatibility => LINUX_COMPATIBILITY_TARGET,
            Self::WindowsNative => WINDOWS_NATIVE_COMPATIBILITY_TARGET,
        }
    }
}

/// Availability of product-sealed manifest compatibility authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "the manifest product-authority availability must be handled"]
pub enum ProductExtensionManifestAuthorityStatus {
    /// No reviewed exact manifest matrix and admission anchor is compiled in.
    Unprovisioned,
    /// At least one bounded backend/package profile is compiled in.
    Configured,
    /// Compiled product profile material is internally inconsistent.
    InvalidProvisioning,
}

/// Failure to open the product-sealed manifest authority.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProductExtensionManifestAuthorityError {
    /// No reviewed exact manifest compatibility anchor is compiled in.
    #[error("extension manifest product authority is unprovisioned")]
    Unprovisioned,
    /// Compiled policy or anchor data is inconsistent or exceeds a bound.
    #[error("extension manifest product authority configuration is invalid")]
    InvalidProductConfiguration,
}

/// Stable fail-closed rejection from product manifest admission.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProductExtensionManifestAdmissionError {
    /// No sealed policy exists for this exact backend and package key.
    #[error("extension package is not provisioned for the selected runtime target")]
    ProfileNotProvisioned,
    /// No rollback policy exists for this exact catalog, backend, and package.
    #[error(
        "extension rollback package is not product-provisioned for the selected runtime target"
    )]
    RollbackProfileNotProvisioned,
    /// Authenticated catalog trust-domain identity differs from the profile.
    #[error("extension catalog authority differs from the manifest profile")]
    CatalogAuthorityMismatch,
    /// Authenticated catalog revision differs from the profile.
    #[error("extension catalog revision differs from the manifest profile")]
    CatalogRevisionMismatch,
    /// Exact canonical catalog byte length differs from the profile.
    #[error("extension catalog byte length differs from the manifest profile")]
    CatalogLengthMismatch,
    /// Authenticated exact catalog digest differs from the profile.
    #[error("extension catalog digest differs from the manifest profile")]
    CatalogDigestMismatch,
    /// Authenticated catalog inventory differs from the profile.
    #[error("extension catalog inventory differs from the manifest profile")]
    CatalogInventoryMismatch,
    /// The authenticated catalog does not contain the exact selected key.
    #[error("extension package key is absent from the authenticated catalog")]
    PackageNotFound,
    /// The package revision differs from the exact sealed revision.
    #[error("extension package revision differs from the manifest profile")]
    PackageRevisionMismatch,
    /// Some other field of the complete package identity differs.
    #[error("extension package identity differs from the manifest profile")]
    PackageIdentityMismatch,
    /// Exact tree-index digest, length, tree digest, or package binding differs.
    #[error("extension tree index differs from the manifest profile")]
    TreeIndexMismatch,
    /// Exact manifest bytes differ from the sealed manifest digest.
    #[error("extension manifest bytes differ from the manifest profile")]
    ManifestDigestMismatch,
    /// Structural manifest parsing or exact resource binding failed.
    #[error("extension manifest is structurally inadmissible: {0}")]
    Structural(#[source] ExtensionManifestAdmissionError),
    /// Structural admission returned a different compatibility target.
    #[error("extension manifest compatibility target differs from the manifest profile")]
    CompatibilityTargetMismatch,
    /// The policy rows are not an exact one-to-one map for this manifest.
    #[error("extension manifest declarations differ from the sealed compatibility matrix")]
    PolicyDeclarationMismatch,
    /// The exact policy result differs from the reviewed compatibility result.
    #[error("extension manifest compatibility result differs from the manifest profile")]
    CompatibilityDigestMismatch,
    /// An unsupported, unassessed, or unmodeled declaration blocks activation.
    #[error("extension declaration is not activatable on the selected runtime: {0:?}")]
    UnsupportedDeclaration(ExtensionManifestDeclaration),
    /// The final exact manifest admission digest differs from the profile.
    #[error("extension manifest admission digest differs from the manifest profile")]
    AdmissionDigestMismatch,
    /// Logical retained-memory accounting overflowed or exceeded its ceiling.
    #[error("product-admitted extension manifest retained-memory limit was exceeded")]
    RetainedBytesExceeded,
}

/// Product-owned authority for exact backend-specific manifest admission.
///
/// There is no public profile, policy, anchor, or trust-provider constructor.
/// Product provisioning is compile-time state, never an environment variable,
/// runtime configuration file, or caller-supplied policy.
pub struct ProductExtensionManifestAuthority {
    active_catalog: SealedManifestCatalogAnchor,
    rollback_catalogs: Box<[SealedManifestCatalogAnchor]>,
    profiles: Box<[SealedManifestProfile]>,
    retained_bytes: usize,
}

impl ProductExtensionManifestAuthority {
    /// Opens the product-sealed authority or reports explicit non-provisioning.
    pub fn product() -> Result<Self, ProductExtensionManifestAuthorityError> {
        let provisioning = sealed_product_manifest_provisioning()?
            .ok_or(ProductExtensionManifestAuthorityError::Unprovisioned)?;
        Self::from_sealed_provisioning(provisioning)
    }

    /// Reports whether exact product compatibility profiles are configured.
    pub fn product_status() -> ProductExtensionManifestAuthorityStatus {
        match Self::product() {
            Ok(_) => ProductExtensionManifestAuthorityStatus::Configured,
            Err(ProductExtensionManifestAuthorityError::Unprovisioned) => {
                ProductExtensionManifestAuthorityStatus::Unprovisioned
            }
            Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration) => {
                ProductExtensionManifestAuthorityStatus::InvalidProvisioning
            }
        }
    }

    /// Admits exact manifest bytes for one authenticated catalog package.
    ///
    /// The caller supplies only a package key, never a release package or tree
    /// binding. This method selects the package from `catalog` internally and
    /// establishes every binding before minting the product witness.
    pub fn admit_manifest(
        &self,
        catalog: &AdmittedBundledCatalog,
        runtime_target: ProductExtensionRuntimeTarget,
        package_key: ExtensionPackageKey,
        tree_index: &CanonicalExtensionTreeIndex,
        manifest_bytes: &[u8],
    ) -> Result<ProductAdmittedExtensionManifest, ProductExtensionManifestAdmissionError> {
        let profile = self
            .profile(self.active_catalog, runtime_target, package_key)
            .ok_or(ProductExtensionManifestAdmissionError::ProfileNotProvisioned)?;
        let data = admit_manifest_data(
            profile,
            catalog.data(),
            runtime_target,
            package_key,
            tree_index,
            manifest_bytes,
        )?;
        Ok(ProductAdmittedExtensionManifest {
            data,
            _seal: ProductManifestWitnessSeal(()),
        })
    }

    /// Admits exact manifest bytes for an explicitly approved rollback package.
    ///
    /// This method accepts only the distinct rollback-catalog capability and
    /// returns a distinct rollback-manifest capability. It cannot be confused
    /// with ordinary active activation at a type-correct call site.
    pub fn admit_rollback_manifest(
        &self,
        catalog: &AdmittedRollbackBundledCatalog,
        runtime_target: ProductExtensionRuntimeTarget,
        package_key: ExtensionPackageKey,
        tree_index: &CanonicalExtensionTreeIndex,
        manifest_bytes: &[u8],
    ) -> Result<ProductAdmittedRollbackExtensionManifest, ProductExtensionManifestAdmissionError>
    {
        let catalog_anchor = SealedManifestCatalogAnchor::from_data(catalog.data());
        if self
            .rollback_catalogs
            .binary_search(&catalog_anchor)
            .is_err()
        {
            return Err(ProductExtensionManifestAdmissionError::RollbackProfileNotProvisioned);
        }
        let profile = self
            .profile(catalog_anchor, runtime_target, package_key)
            .ok_or(ProductExtensionManifestAdmissionError::RollbackProfileNotProvisioned)?;
        let data = admit_manifest_data(
            profile,
            catalog.data(),
            runtime_target,
            package_key,
            tree_index,
            manifest_bytes,
        )?;
        Ok(ProductAdmittedRollbackExtensionManifest {
            data,
            _seal: ProductRollbackManifestWitnessSeal(()),
        })
    }

    /// Returns the authority's explicit logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    fn profile(
        &self,
        catalog: SealedManifestCatalogAnchor,
        runtime_target: ProductExtensionRuntimeTarget,
        package_key: ExtensionPackageKey,
    ) -> Option<&SealedManifestProfile> {
        self.profiles
            .binary_search_by(|profile| {
                profile
                    .catalog
                    .cmp(&catalog)
                    .then_with(|| profile.runtime_target.cmp(&runtime_target))
                    .then_with(|| profile.package.key.cmp(&package_key))
            })
            .ok()
            .map(|index| &self.profiles[index])
    }

    #[cfg(test)]
    fn from_sealed_profiles(
        profiles: Box<[SealedManifestProfile]>,
    ) -> Result<Self, ProductExtensionManifestAuthorityError> {
        let active_catalog = profiles
            .first()
            .map(|profile| profile.catalog)
            .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?;
        Self::from_sealed_provisioning(SealedManifestAuthorityProvisioning {
            active_catalog,
            rollback_catalogs: Box::new([]),
            profiles,
        })
    }

    fn from_sealed_provisioning(
        provisioning: SealedManifestAuthorityProvisioning,
    ) -> Result<Self, ProductExtensionManifestAuthorityError> {
        let SealedManifestAuthorityProvisioning {
            active_catalog,
            rollback_catalogs,
            profiles,
        } = provisioning;
        validate_catalog_generations(active_catalog, &rollback_catalogs)?;
        if profiles.is_empty()
            || profiles.len() > MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_ACROSS_GENERATIONS
        {
            return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
        }
        if profiles.windows(2).any(|pair| {
            (pair[0].catalog, pair[0].runtime_target, pair[0].package.key)
                >= (pair[1].catalog, pair[1].runtime_target, pair[1].package.key)
        }) {
            return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
        }
        if profiles.iter().any(|profile| {
            profile.catalog != active_catalog
                && rollback_catalogs.binary_search(&profile.catalog).is_err()
        }) {
            return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
        }

        let mut retained_bytes = AUTHORITY_ACCOUNTING_OVERHEAD
            .checked_add(
                (1 + rollback_catalogs.len())
                    .checked_mul(GENERATION_ACCOUNTING_OVERHEAD)
                    .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?,
            )
            .and_then(|bytes| {
                bytes.checked_add(
                    rollback_catalogs
                        .len()
                        .checked_mul(size_of::<SealedManifestCatalogAnchor>())?,
                )
            })
            .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?
            .checked_add(
                profiles
                    .len()
                    .checked_mul(size_of::<SealedManifestProfile>())
                    .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?,
            )
            .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?;
        for catalog in std::iter::once(active_catalog).chain(rollback_catalogs.iter().copied()) {
            let generation_profile_count = profiles
                .iter()
                .filter(|profile| profile.catalog == catalog)
                .count();
            if generation_profile_count == 0
                || generation_profile_count > MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION
            {
                return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
            }
            let mut generation_retained_bytes = GENERATION_ACCOUNTING_OVERHEAD;
            for profile in profiles.iter().filter(|profile| profile.catalog == catalog) {
                profile.validate_configuration()?;
                let profile_bytes = PROFILE_ACCOUNTING_OVERHEAD
                    .checked_add(profile.policy.retained_bytes)
                    .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?;
                generation_retained_bytes = generation_retained_bytes
                    .checked_add(size_of::<SealedManifestProfile>())
                    .and_then(|bytes| bytes.checked_add(profile_bytes))
                    .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?;
                retained_bytes = retained_bytes
                    .checked_add(profile_bytes)
                    .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?;
            }
            if generation_retained_bytes > MAX_PRODUCT_EXTENSION_MANIFEST_GENERATION_RETAINED_BYTES
            {
                return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
            }
        }
        if retained_bytes > MAX_PRODUCT_EXTENSION_MANIFEST_AUTHORITY_RETAINED_BYTES {
            return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
        }

        Ok(Self {
            active_catalog,
            rollback_catalogs,
            profiles,
            retained_bytes,
        })
    }
}

fn admit_manifest_data(
    profile: &SealedManifestProfile,
    catalog: &AdmittedCatalogData,
    runtime_target: ProductExtensionRuntimeTarget,
    package_key: ExtensionPackageKey,
    tree_index: &CanonicalExtensionTreeIndex,
    manifest_bytes: &[u8],
) -> Result<ProductAdmittedManifestData, ProductExtensionManifestAdmissionError> {
    profile.verify_catalog(catalog)?;
    let package = catalog
        .catalog()
        .package(package_key)
        .ok_or(ProductExtensionManifestAdmissionError::PackageNotFound)?;
    if package.identity().revision() != profile.package.revision {
        return Err(ProductExtensionManifestAdmissionError::PackageRevisionMismatch);
    }
    if package.identity() != &profile.package.identity {
        return Err(ProductExtensionManifestAdmissionError::PackageIdentityMismatch);
    }
    if tree_index.index_sha256() != profile.package.tree_index_digest
        || tree_index.index_bytes() != profile.package.tree_index_length
        || tree_index.tree_sha256() != profile.package.tree_digest
    {
        return Err(ProductExtensionManifestAdmissionError::TreeIndexMismatch);
    }
    let binding = package
        .bind_tree_index(tree_index)
        .map_err(|_| ProductExtensionManifestAdmissionError::TreeIndexMismatch)?;
    if tree_index.manifest_sha256() != profile.package.manifest_digest
        || sha256_manifest(manifest_bytes) != profile.package.manifest_digest
    {
        return Err(ProductExtensionManifestAdmissionError::ManifestDigestMismatch);
    }

    let admitted = admit_extension_manifest(binding, manifest_bytes, &profile.policy)
        .map_err(ProductExtensionManifestAdmissionError::Structural)?;
    profile.verify_admission(&admitted)?;
    let retained_bytes = admitted
        .retained_bytes()
        .checked_add(WITNESS_ACCOUNTING_OVERHEAD)
        .ok_or(ProductExtensionManifestAdmissionError::RetainedBytesExceeded)?;
    if retained_bytes > MAX_PRODUCT_ADMITTED_EXTENSION_MANIFEST_RETAINED_BYTES {
        return Err(ProductExtensionManifestAdmissionError::RetainedBytesExceeded);
    }

    Ok(ProductAdmittedManifestData {
        manifest: admitted,
        runtime_target,
        catalog_authority: profile.catalog.authority,
        catalog_revision: profile.catalog.revision,
        catalog_length: profile.catalog.length,
        catalog_digest: profile.catalog.digest,
        catalog_inventory_digest: profile.catalog.inventory_digest,
        tree_index_digest: profile.package.tree_index_digest,
        tree_index_length: profile.package.tree_index_length,
        retained_bytes,
    })
}

/// Exact product-admitted manifest witness required by later activation code.
///
/// This type has private fields, no public constructor, no `Clone`, no Serde
/// implementation, and no `from_parts` escape hatch. Digest values and the
/// structural projections exposed below remain non-capabilities when separated
/// from this witness.
///
/// ```compile_fail
/// use zephium_extension_authority::ProductAdmittedExtensionManifest;
/// fn require_clone<T: Clone>() {}
/// fn duplicate() {
///     require_clone::<ProductAdmittedExtensionManifest>();
/// }
/// ```
///
/// ```compile_fail
/// use zephium_extension_authority::ProductAdmittedExtensionManifest;
/// fn forge() -> ProductAdmittedExtensionManifest {
///     ProductAdmittedExtensionManifest {}
/// }
/// ```
#[must_use = "the product manifest witness must be consumed by a target-matched plan or discarded"]
pub struct ProductAdmittedExtensionManifest {
    data: ProductAdmittedManifestData,
    _seal: ProductManifestWitnessSeal,
}

struct ProductAdmittedManifestData {
    manifest: AdmittedExtensionManifest,
    runtime_target: ProductExtensionRuntimeTarget,
    catalog_authority: ExtensionAuthorityId,
    catalog_revision: ExtensionReleaseCatalogRevision,
    catalog_length: u64,
    catalog_digest: ExtensionReleaseCatalogDigest,
    catalog_inventory_digest: BundledCatalogInventoryDigest,
    tree_index_digest: ExtensionTreeIndexDigest,
    tree_index_length: u64,
    retained_bytes: usize,
}

impl fmt::Debug for ProductAdmittedExtensionManifest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProductAdmittedExtensionManifest")
            .field("runtime_target", &self.runtime_target())
            .field("catalog_authority", &self.catalog_authority())
            .field("catalog_revision", &self.catalog_revision())
            .field("catalog_length", &self.catalog_length())
            .field("catalog_digest", &self.catalog_digest())
            .field("package", self.package_identity())
            .field("tree_index_digest", &self.tree_index_digest())
            .field("admission_digest", &self.admission_digest())
            .field("retained_bytes", &self.retained_bytes())
            .finish()
    }
}

impl ProductAdmittedExtensionManifest {
    /// Returns the exact backend target this witness authorizes.
    ///
    /// Later native plan construction must compare this value to its adapter
    /// target; operating-system equality is not sufficient.
    pub const fn runtime_target(&self) -> ProductExtensionRuntimeTarget {
        self.data.runtime_target
    }

    /// Returns the authenticated catalog trust-domain identity.
    pub const fn catalog_authority(&self) -> ExtensionAuthorityId {
        self.data.catalog_authority
    }

    /// Returns the authenticated catalog revision.
    pub const fn catalog_revision(&self) -> ExtensionReleaseCatalogRevision {
        self.data.catalog_revision
    }

    /// Returns the exact authenticated canonical catalog byte length.
    pub const fn catalog_length(&self) -> u64 {
        self.data.catalog_length
    }

    /// Returns SHA-256 of the exact authenticated catalog document.
    pub const fn catalog_digest(&self) -> ExtensionReleaseCatalogDigest {
        self.data.catalog_digest
    }

    /// Returns the redundant authenticated catalog inventory digest.
    pub const fn catalog_inventory_digest(&self) -> BundledCatalogInventoryDigest {
        self.data.catalog_inventory_digest
    }

    /// Returns the complete exact package identity.
    pub const fn package_identity(&self) -> &ExtensionPackageIdentity {
        self.data.manifest.descriptor().package()
    }

    /// Returns the selected package key.
    pub const fn package_key(&self) -> ExtensionPackageKey {
        self.package_identity().key()
    }

    /// Returns the exact package revision.
    pub const fn package_revision(&self) -> ExtensionPackageRevision {
        self.package_identity().revision()
    }

    /// Returns the exact canonical tree-index digest.
    pub const fn tree_index_digest(&self) -> ExtensionTreeIndexDigest {
        self.data.tree_index_digest
    }

    /// Returns the exact canonical tree-index byte length.
    pub const fn tree_index_length(&self) -> u64 {
        self.data.tree_index_length
    }

    /// Returns the exact canonical resource-tree digest.
    pub const fn tree_digest(&self) -> ExtensionTreeDigest {
        self.package_identity().tree_sha256()
    }

    /// Returns SHA-256 of exact admitted `manifest.json` bytes.
    pub const fn manifest_digest(&self) -> ExtensionManifestDigest {
        self.package_identity().manifest_sha256()
    }

    /// Returns the product-owned versioned compatibility target identifier.
    pub const fn compatibility_target(&self) -> &ExtensionCompatibilityTargetId {
        self.data.manifest.descriptor().compatibility_target()
    }

    /// Returns the complete path-free descriptor as a read-only projection.
    pub const fn descriptor(&self) -> &ExtensionManifestDescriptor {
        self.data.manifest.descriptor()
    }

    /// Returns bounded display metadata as a read-only projection.
    pub const fn metadata(&self) -> &ExtensionManifestMetadata {
        self.data.manifest.metadata()
    }

    /// Returns exact authenticated manifest resources as a read-only projection.
    pub const fn resources(&self) -> &ExtensionManifestResourcePlan {
        self.data.manifest.resources()
    }

    /// Returns the exact verified Chromium manifest key when present.
    pub const fn chromium_key(&self) -> Option<&ChromiumManifestKey> {
        self.data.manifest.chromium_key()
    }

    /// Returns the final exact sealed manifest admission digest.
    pub const fn admission_digest(&self) -> ExtensionManifestAdmissionDigest {
        self.data.manifest.admission_digest()
    }

    /// Returns the witness's explicit logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.data.retained_bytes
    }
}

struct ProductManifestWitnessSeal(());

/// Exact rollback-manifest witness for an explicitly approved catalog generation.
///
/// This type has no conversion to [`ProductAdmittedExtensionManifest`], no
/// public constructor, no `Clone`, and no Serde implementation. Recovery and
/// rollback activation adapters must opt into this exact type instead of
/// accidentally consuming it through the ordinary active path.
///
/// ```compile_fail
/// use zephium_extension_authority::ProductAdmittedRollbackExtensionManifest;
/// fn require_clone<T: Clone>() {}
/// fn duplicate() {
///     require_clone::<ProductAdmittedRollbackExtensionManifest>();
/// }
/// ```
///
/// ```compile_fail
/// use zephium_extension_authority::ProductAdmittedRollbackExtensionManifest;
/// fn forge() -> ProductAdmittedRollbackExtensionManifest {
///     ProductAdmittedRollbackExtensionManifest {}
/// }
/// ```
///
/// ```compile_fail
/// use zephium_extension_authority::{
///     ProductAdmittedExtensionManifest, ProductAdmittedRollbackExtensionManifest,
/// };
/// fn activate_active(_: ProductAdmittedExtensionManifest) {}
/// fn cannot_cross_capability(rollback: ProductAdmittedRollbackExtensionManifest) {
///     activate_active(rollback);
/// }
/// ```
#[must_use = "the rollback manifest witness must be consumed by an explicit recovery plan or discarded"]
pub struct ProductAdmittedRollbackExtensionManifest {
    data: ProductAdmittedManifestData,
    _seal: ProductRollbackManifestWitnessSeal,
}

impl fmt::Debug for ProductAdmittedRollbackExtensionManifest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProductAdmittedRollbackExtensionManifest")
            .field("runtime_target", &self.runtime_target())
            .field("catalog_authority", &self.catalog_authority())
            .field("catalog_revision", &self.catalog_revision())
            .field("catalog_length", &self.catalog_length())
            .field("catalog_digest", &self.catalog_digest())
            .field("package", self.package_identity())
            .field("tree_index_digest", &self.tree_index_digest())
            .field("admission_digest", &self.admission_digest())
            .field("retained_bytes", &self.retained_bytes())
            .finish()
    }
}

impl ProductAdmittedRollbackExtensionManifest {
    /// Returns the exact backend target this rollback witness authorizes.
    pub const fn runtime_target(&self) -> ProductExtensionRuntimeTarget {
        self.data.runtime_target
    }

    /// Returns the authenticated catalog trust-domain identity.
    pub const fn catalog_authority(&self) -> ExtensionAuthorityId {
        self.data.catalog_authority
    }

    /// Returns the exact explicitly approved rollback catalog revision.
    pub const fn catalog_revision(&self) -> ExtensionReleaseCatalogRevision {
        self.data.catalog_revision
    }

    /// Returns the exact authenticated canonical rollback-catalog byte length.
    pub const fn catalog_length(&self) -> u64 {
        self.data.catalog_length
    }

    /// Returns SHA-256 of exact authenticated rollback-catalog bytes.
    pub const fn catalog_digest(&self) -> ExtensionReleaseCatalogDigest {
        self.data.catalog_digest
    }

    /// Returns the rollback catalog's deterministic inventory digest.
    pub const fn catalog_inventory_digest(&self) -> BundledCatalogInventoryDigest {
        self.data.catalog_inventory_digest
    }

    /// Returns the complete exact package identity.
    pub const fn package_identity(&self) -> &ExtensionPackageIdentity {
        self.data.manifest.descriptor().package()
    }

    /// Returns the selected package key.
    pub const fn package_key(&self) -> ExtensionPackageKey {
        self.package_identity().key()
    }

    /// Returns the exact package revision.
    pub const fn package_revision(&self) -> ExtensionPackageRevision {
        self.package_identity().revision()
    }

    /// Returns the exact canonical tree-index digest.
    pub const fn tree_index_digest(&self) -> ExtensionTreeIndexDigest {
        self.data.tree_index_digest
    }

    /// Returns the exact canonical tree-index byte length.
    pub const fn tree_index_length(&self) -> u64 {
        self.data.tree_index_length
    }

    /// Returns the exact canonical resource-tree digest.
    pub const fn tree_digest(&self) -> ExtensionTreeDigest {
        self.package_identity().tree_sha256()
    }

    /// Returns SHA-256 of exact admitted `manifest.json` bytes.
    pub const fn manifest_digest(&self) -> ExtensionManifestDigest {
        self.package_identity().manifest_sha256()
    }

    /// Returns the product-owned versioned compatibility target identifier.
    pub const fn compatibility_target(&self) -> &ExtensionCompatibilityTargetId {
        self.data.manifest.descriptor().compatibility_target()
    }

    /// Returns the complete path-free descriptor as a read-only projection.
    pub const fn descriptor(&self) -> &ExtensionManifestDescriptor {
        self.data.manifest.descriptor()
    }

    /// Returns bounded display metadata as a read-only projection.
    pub const fn metadata(&self) -> &ExtensionManifestMetadata {
        self.data.manifest.metadata()
    }

    /// Returns exact authenticated manifest resources as a read-only projection.
    pub const fn resources(&self) -> &ExtensionManifestResourcePlan {
        self.data.manifest.resources()
    }

    /// Returns the exact verified Chromium manifest key when present.
    pub const fn chromium_key(&self) -> Option<&ChromiumManifestKey> {
        self.data.manifest.chromium_key()
    }

    /// Returns the final exact sealed manifest admission digest.
    pub const fn admission_digest(&self) -> ExtensionManifestAdmissionDigest {
        self.data.manifest.admission_digest()
    }

    /// Returns the witness's explicit logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.data.retained_bytes
    }
}

struct ProductRollbackManifestWitnessSeal(());

#[derive(Clone)]
struct SealedManifestProfile {
    runtime_target: ProductExtensionRuntimeTarget,
    catalog: SealedManifestCatalogAnchor,
    package: SealedManifestPackageAnchor,
    policy: SealedManifestCompatibilityPolicy,
}

impl SealedManifestProfile {
    fn validate_configuration(&self) -> Result<(), ProductExtensionManifestAuthorityError> {
        if self.catalog.length == 0
            || self.catalog.length > MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64
            || self.package.key != self.package.identity.key()
            || self.package.revision != self.package.identity.revision()
            || self.package.identity.authority() != self.catalog.authority
            || self.package.manifest_digest != self.package.identity.manifest_sha256()
            || self.package.tree_digest != self.package.identity.tree_sha256()
            || self.package.tree_index_length == 0
            || self.package.tree_index_length > MAX_EXTENSION_TREE_INDEX_BYTES as u64
            || self.policy.target != self.package.compatibility_target
            || self.package.compatibility_target.as_str()
                != self.runtime_target.compatibility_target_id()
        {
            return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
        }
        self.policy.validate_configuration()?;
        Ok(())
    }

    fn verify_catalog(
        &self,
        catalog: &AdmittedCatalogData,
    ) -> Result<(), ProductExtensionManifestAdmissionError> {
        if catalog.authority() != self.catalog.authority {
            return Err(ProductExtensionManifestAdmissionError::CatalogAuthorityMismatch);
        }
        if catalog.revision() != self.catalog.revision {
            return Err(ProductExtensionManifestAdmissionError::CatalogRevisionMismatch);
        }
        if catalog.catalog_length() != self.catalog.length {
            return Err(ProductExtensionManifestAdmissionError::CatalogLengthMismatch);
        }
        if catalog.catalog_digest() != self.catalog.digest {
            return Err(ProductExtensionManifestAdmissionError::CatalogDigestMismatch);
        }
        if catalog.inventory_digest() != self.catalog.inventory_digest {
            return Err(ProductExtensionManifestAdmissionError::CatalogInventoryMismatch);
        }
        Ok(())
    }

    fn verify_admission(
        &self,
        admitted: &AdmittedExtensionManifest,
    ) -> Result<(), ProductExtensionManifestAdmissionError> {
        let descriptor = admitted.descriptor();
        if descriptor.package() != &self.package.identity {
            return Err(ProductExtensionManifestAdmissionError::PackageIdentityMismatch);
        }
        if descriptor.compatibility_target() != &self.package.compatibility_target {
            return Err(ProductExtensionManifestAdmissionError::CompatibilityTargetMismatch);
        }
        let classifications = descriptor.compatibility();
        if classifications.len() != self.policy.rows.len()
            || classifications
                .iter()
                .zip(self.policy.rows.iter())
                .any(|(classification, row)| {
                    classification.declaration() != &row.declaration
                        || classification.level() != row.level
                })
        {
            return Err(ProductExtensionManifestAdmissionError::PolicyDeclarationMismatch);
        }
        if descriptor.compatibility_digest() != self.package.compatibility_digest {
            return Err(ProductExtensionManifestAdmissionError::CompatibilityDigestMismatch);
        }
        if let Some(classification) = classifications.iter().find(|classification| {
            matches!(
                classification.level(),
                ExtensionCompatibilityLevel::Unsupported | ExtensionCompatibilityLevel::Unassessed
            ) || matches!(
                classification.declaration(),
                ExtensionManifestDeclaration::UnmodeledAuthority(_)
            )
        }) {
            return Err(
                ProductExtensionManifestAdmissionError::UnsupportedDeclaration(
                    classification.declaration().clone(),
                ),
            );
        }
        if admitted.admission_digest() != self.package.admission_digest {
            return Err(ProductExtensionManifestAdmissionError::AdmissionDigestMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct SealedManifestCatalogAnchor {
    authority: ExtensionAuthorityId,
    revision: ExtensionReleaseCatalogRevision,
    length: u64,
    digest: ExtensionReleaseCatalogDigest,
    inventory_digest: BundledCatalogInventoryDigest,
}

impl SealedManifestCatalogAnchor {
    fn from_data(catalog: &AdmittedCatalogData) -> Self {
        Self {
            authority: catalog.authority(),
            revision: catalog.revision(),
            length: catalog.catalog_length(),
            digest: catalog.catalog_digest(),
            inventory_digest: catalog.inventory_digest(),
        }
    }
}

#[derive(Clone)]
struct SealedManifestPackageAnchor {
    key: ExtensionPackageKey,
    revision: ExtensionPackageRevision,
    identity: ExtensionPackageIdentity,
    tree_index_digest: ExtensionTreeIndexDigest,
    tree_index_length: u64,
    tree_digest: ExtensionTreeDigest,
    manifest_digest: ExtensionManifestDigest,
    compatibility_target: ExtensionCompatibilityTargetId,
    compatibility_digest: ExtensionCompatibilityProfileDigest,
    admission_digest: ExtensionManifestAdmissionDigest,
}

#[derive(Clone)]
struct SealedManifestCompatibilityPolicy {
    target: ExtensionCompatibilityTargetId,
    rows: Box<[SealedManifestCompatibilityRow]>,
    retained_bytes: usize,
}

impl SealedManifestCompatibilityPolicy {
    #[cfg(any(test, zephium_internal_repository_e2e))]
    fn new(
        target: ExtensionCompatibilityTargetId,
        rows: Box<[SealedManifestCompatibilityRow]>,
    ) -> Result<Self, ProductExtensionManifestAuthorityError> {
        let retained_bytes = Self::calculate_retained_bytes(&target, &rows)?;
        let policy = Self {
            target,
            rows,
            retained_bytes,
        };
        policy.validate_configuration()?;
        Ok(policy)
    }

    fn validate_configuration(&self) -> Result<(), ProductExtensionManifestAuthorityError> {
        if self.rows.is_empty() || self.rows.len() > MAX_EXTENSION_MANIFEST_DECLARATIONS {
            return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
        }
        if self
            .rows
            .windows(2)
            .any(|pair| pair[0].declaration >= pair[1].declaration)
            || self
                .rows
                .iter()
                .any(|row| row.level == ExtensionCompatibilityLevel::Unassessed)
        {
            return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
        }
        if self.retained_bytes != Self::calculate_retained_bytes(&self.target, &self.rows)? {
            return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
        }
        Ok(())
    }

    fn calculate_retained_bytes(
        target: &ExtensionCompatibilityTargetId,
        rows: &[SealedManifestCompatibilityRow],
    ) -> Result<usize, ProductExtensionManifestAuthorityError> {
        let retained_bytes = size_of::<Self>()
            .checked_add(
                rows.len()
                    .checked_mul(
                        size_of::<SealedManifestCompatibilityRow>()
                            + POLICY_ROW_ACCOUNTING_OVERHEAD,
                    )
                    .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?,
            )
            .and_then(|bytes| bytes.checked_add(target.as_str().len()))
            .and_then(|bytes| {
                rows.iter().try_fold(bytes, |total, row| {
                    total.checked_add(row.declaration.canonical_value().map_or(0, str::len))
                })
            })
            .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?;
        if retained_bytes > MAX_PRODUCT_EXTENSION_MANIFEST_AUTHORITY_RETAINED_BYTES {
            return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
        }
        Ok(retained_bytes)
    }
}

impl ExtensionManifestCompatibilityPolicy for SealedManifestCompatibilityPolicy {
    fn target(&self) -> &ExtensionCompatibilityTargetId {
        &self.target
    }

    fn classify(
        &self,
        subject: ExtensionManifestCompatibilitySubject<'_>,
    ) -> Option<ExtensionCompatibilityLevel> {
        self.rows
            .binary_search_by(|row| row.declaration.cmp(subject.declaration()))
            .ok()
            .map(|index| self.rows[index].level)
    }
}

#[derive(Clone)]
struct SealedManifestCompatibilityRow {
    declaration: ExtensionManifestDeclaration,
    level: ExtensionCompatibilityLevel,
}

fn sha256_manifest(bytes: &[u8]) -> ExtensionManifestDigest {
    use sha2::{Digest, Sha256};

    ExtensionManifestDigest::from_bytes(Sha256::digest(bytes).into())
}

struct SealedManifestAuthorityProvisioning {
    active_catalog: SealedManifestCatalogAnchor,
    rollback_catalogs: Box<[SealedManifestCatalogAnchor]>,
    profiles: Box<[SealedManifestProfile]>,
}

fn validate_catalog_generations(
    active: SealedManifestCatalogAnchor,
    rollback: &[SealedManifestCatalogAnchor],
) -> Result<(), ProductExtensionManifestAuthorityError> {
    if rollback.len() > MAX_PRODUCT_ROLLBACK_BUNDLED_CATALOGS
        || rollback
            .windows(2)
            .any(|pair| pair[0].revision >= pair[1].revision)
        || rollback.iter().any(|catalog| {
            catalog.authority != active.authority || catalog.revision >= active.revision
        })
    {
        return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
    }
    if rollback.iter().enumerate().any(|(index, catalog)| {
        catalog.digest == active.digest
            || rollback[index + 1..]
                .iter()
                .any(|later| later.digest == catalog.digest)
    }) {
        return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
    }
    Ok(())
}

// Deliberately absent from ordinary builds until the exact reviewed Bitwarden
// Core manifests, redistribution artifacts, active and rollback catalog
// anchors, per-backend compatibility matrices, and admission digests are
// available. This is the only production provisioning slot; never populate it
// from runtime bytes, configuration, or environment variables.
#[cfg(not(zephium_internal_repository_e2e))]
fn sealed_product_manifest_provisioning(
) -> Result<Option<SealedManifestAuthorityProvisioning>, ProductExtensionManifestAuthorityError> {
    Ok(None)
}

#[cfg(zephium_internal_repository_e2e)]
fn sealed_product_manifest_provisioning(
) -> Result<Option<SealedManifestAuthorityProvisioning>, ProductExtensionManifestAuthorityError> {
    let active = repository_e2e_manifest_profile(ACTIVE_CATALOG_BYTES)?;
    let rollback = repository_e2e_manifest_profile(ROLLBACK_CATALOG_BYTES)?;
    let active_catalog = active.catalog;
    let rollback_catalogs = vec![rollback.catalog].into_boxed_slice();
    let mut profiles = vec![active, rollback];
    profiles.sort_unstable_by(|left, right| {
        (left.catalog, left.runtime_target, left.package.key).cmp(&(
            right.catalog,
            right.runtime_target,
            right.package.key,
        ))
    });
    Ok(Some(SealedManifestAuthorityProvisioning {
        active_catalog,
        rollback_catalogs,
        profiles: profiles.into_boxed_slice(),
    }))
}

#[cfg(zephium_internal_repository_e2e)]
struct RepositoryE2eCompatibilityPolicy {
    target: ExtensionCompatibilityTargetId,
}

#[cfg(zephium_internal_repository_e2e)]
impl ExtensionManifestCompatibilityPolicy for RepositoryE2eCompatibilityPolicy {
    fn target(&self) -> &ExtensionCompatibilityTargetId {
        &self.target
    }

    fn classify(
        &self,
        _subject: ExtensionManifestCompatibilitySubject<'_>,
    ) -> Option<ExtensionCompatibilityLevel> {
        Some(ExtensionCompatibilityLevel::Compatible)
    }
}

#[cfg(zephium_internal_repository_e2e)]
fn repository_e2e_manifest_profile(
    catalog_bytes: &[u8],
) -> Result<SealedManifestProfile, ProductExtensionManifestAuthorityError> {
    let catalog = ExtensionReleaseCatalog::parse_canonical(catalog_bytes)
        .map_err(invalid_repository_e2e_configuration)?;
    let tree = CanonicalExtensionTreeIndex::parse_canonical(TREE_INDEX_BYTES)
        .map_err(invalid_repository_e2e_configuration)?;
    let package_key = ExtensionPackageKey::from_bytes(PACKAGE_KEY_BYTES);
    if catalog.packages().len() != 1 || catalog.packages()[0].identity().key() != package_key {
        return Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration);
    }
    let package = catalog
        .package(package_key)
        .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?;
    let runtime_target = repository_e2e_runtime_target();
    let compatibility_target =
        ExtensionCompatibilityTargetId::parse_exact(runtime_target.compatibility_target_id())
            .map_err(invalid_repository_e2e_configuration)?;
    let fixture_policy = RepositoryE2eCompatibilityPolicy {
        target: compatibility_target.clone(),
    };
    let binding = package
        .bind_tree_index(&tree)
        .map_err(invalid_repository_e2e_configuration)?;
    let admitted = admit_extension_manifest(binding, MANIFEST_BYTES, &fixture_policy)
        .map_err(invalid_repository_e2e_configuration)?;
    let rows = admitted
        .descriptor()
        .compatibility()
        .iter()
        .map(|classification| SealedManifestCompatibilityRow {
            declaration: classification.declaration().clone(),
            level: classification.level(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let policy = SealedManifestCompatibilityPolicy::new(compatibility_target.clone(), rows)
        .map_err(invalid_repository_e2e_configuration)?;
    let length =
        u64::try_from(catalog_bytes.len()).map_err(invalid_repository_e2e_configuration)?;
    let inventory_digest = crate::inventory::digest_catalog_inventory(&catalog)
        .ok_or(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)?;

    Ok(SealedManifestProfile {
        runtime_target,
        catalog: SealedManifestCatalogAnchor {
            authority: catalog.authority(),
            revision: catalog.revision(),
            length,
            digest: catalog.digest(),
            inventory_digest,
        },
        package: SealedManifestPackageAnchor {
            key: package_key,
            revision: package.identity().revision(),
            identity: package.identity().clone(),
            tree_index_digest: tree.index_sha256(),
            tree_index_length: tree.index_bytes(),
            tree_digest: tree.tree_sha256(),
            manifest_digest: tree.manifest_sha256(),
            compatibility_target,
            compatibility_digest: admitted.descriptor().compatibility_digest(),
            admission_digest: admitted.admission_digest(),
        },
        policy,
    })
}

#[cfg(zephium_internal_repository_e2e)]
fn invalid_repository_e2e_configuration<Error>(
    _error: Error,
) -> ProductExtensionManifestAuthorityError {
    ProductExtensionManifestAuthorityError::InvalidProductConfiguration
}

#[cfg(all(zephium_internal_repository_e2e, target_os = "macos"))]
const fn repository_e2e_runtime_target() -> ProductExtensionRuntimeTarget {
    ProductExtensionRuntimeTarget::MacosCompatibility
}

#[cfg(all(zephium_internal_repository_e2e, target_os = "linux"))]
const fn repository_e2e_runtime_target() -> ProductExtensionRuntimeTarget {
    ProductExtensionRuntimeTarget::LinuxCompatibility
}

#[cfg(all(zephium_internal_repository_e2e, target_os = "windows"))]
const fn repository_e2e_runtime_target() -> ProductExtensionRuntimeTarget {
    ProductExtensionRuntimeTarget::WindowsNative
}

const _: () = assert!(MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION == 32);
const _: () = assert!(MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES == 32);
const _: () = assert!(MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_ACROSS_GENERATIONS == 96);
const _: () = assert!(
    MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_ACROSS_GENERATIONS
        == MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION
            * MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS
);
const _: () = assert!(size_of::<ProductAdmittedExtensionManifest>() <= WITNESS_ACCOUNTING_OVERHEAD);
const _: () =
    assert!(size_of::<ProductAdmittedRollbackExtensionManifest>() <= WITNESS_ACCOUNTING_OVERHEAD);
