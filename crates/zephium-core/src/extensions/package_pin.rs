//! Process-local joins between Store native ownership and repository pins.
//!
//! These values prevent repository package-pin operations from accepting an
//! independently assembled bag of owner, package, catalog, and eligibility
//! fields. They are deliberately not Store freshness authority: the runtime
//! service must still serialize the native-ownership journal and revalidate
//! its exact row before advancing or clearing that row.

use std::error::Error;
use std::fmt;
use std::mem::size_of;

use crate::ids::{ExtensionInstallId, ProfileId};

use super::runtime::ExtensionRuntimeOperationAuthorityLineage;

use super::{
    ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
    ExtensionExpectedNativeOwnershipIdentity, ExtensionGrantBrowsingContext, ExtensionGrantDigest,
    ExtensionGrantRevision, ExtensionInstallCatalogRevision, ExtensionInstallRevision,
    ExtensionNativeIncarnation, ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipEntryCas,
    ExtensionNativeOwnershipEntryRevision, ExtensionNativeOwnershipIdentity,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipOperation,
    ExtensionNativeOwnershipPhase, ExtensionPackageIdentity, ExtensionRuntimeBackendTarget,
    ExtensionRuntimeEligibility, ExtensionRuntimeGeneration, ExtensionRuntimeOperationAuthority,
};

const RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES: usize = 2 * size_of::<usize>();

/// Stable fail-closed refusal to join runtime eligibility to an acquisition row.
///
/// No variant retains extension identity, package data, or browsing data.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ExtensionPackagePinAcquisitionDenial {
    /// The journal row does not represent an acquisition operation.
    OwnershipIntentMismatch,
    /// Native absence has not been durably established at the preparation frontier.
    OwnershipPhaseMismatch,
    /// The eligibility belongs to another durable profile.
    EligibilityProfileMismatch,
    /// The eligibility belongs to another profile-scoped install.
    EligibilityInstallMismatch,
    /// The eligibility belongs to another browsing partition.
    EligibilityBrowsingContextMismatch,
    /// The journal and eligibility do not identify the same package.
    EligibilityPackageMismatch,
    /// The complete Store install-catalog revisions differ.
    EligibilityCatalogRevisionMismatch,
    /// The exact Store install-row revisions differ.
    EligibilityInstallRevisionMismatch,
    /// The exact Store grant-row revisions differ.
    EligibilityGrantRevisionMismatch,
    /// The complete Store grant-authority digests differ.
    EligibilityGrantDigestMismatch,
    /// Runtime authority belongs to another package-pin operation lineage.
    OperationAuthorityLineageMismatch,
}

impl fmt::Display for ExtensionPackagePinAcquisitionDenial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "extension package-pin acquisition denied: {self:?}"
        )
    }
}

impl Error for ExtensionPackagePinAcquisitionDenial {}

fn acquisition_denial(
    entry: &ExtensionNativeOwnershipEntry,
    eligibility: &ExtensionRuntimeEligibility,
) -> Option<ExtensionPackagePinAcquisitionDenial> {
    if entry.intent() != ExtensionNativeOwnershipIntent::Acquire {
        return Some(ExtensionPackagePinAcquisitionDenial::OwnershipIntentMismatch);
    }
    if entry.phase() != ExtensionNativeOwnershipPhase::NativeAbsentPreparing {
        return Some(ExtensionPackagePinAcquisitionDenial::OwnershipPhaseMismatch);
    }
    let key = entry.key();
    if key.browsing_context() != eligibility.browsing_context() {
        return Some(ExtensionPackagePinAcquisitionDenial::EligibilityBrowsingContextMismatch);
    }
    if key.profile() != eligibility.profile() {
        return Some(ExtensionPackagePinAcquisitionDenial::EligibilityProfileMismatch);
    }
    if key.install_id() != eligibility.install_id() {
        return Some(ExtensionPackagePinAcquisitionDenial::EligibilityInstallMismatch);
    }
    if entry.package() != eligibility.package() {
        return Some(ExtensionPackagePinAcquisitionDenial::EligibilityPackageMismatch);
    }
    if entry.store_catalog_revision() != eligibility.catalog_revision() {
        return Some(ExtensionPackagePinAcquisitionDenial::EligibilityCatalogRevisionMismatch);
    }
    if entry.store_install_revision() != eligibility.install_revision() {
        return Some(ExtensionPackagePinAcquisitionDenial::EligibilityInstallRevisionMismatch);
    }
    if entry.store_grant_revision() != eligibility.grant_revision() {
        return Some(ExtensionPackagePinAcquisitionDenial::EligibilityGrantRevisionMismatch);
    }
    if entry.grant_digest() != eligibility.grant_digest() {
        return Some(ExtensionPackagePinAcquisitionDenial::EligibilityGrantDigestMismatch);
    }
    None
}

/// Stable fail-closed refusal to project a cleanup-only release row.
///
/// No variant retains extension identity, package data, or browsing data.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ExtensionPackagePinReleaseDenial {
    /// The journal row does not represent a release operation.
    OwnershipIntentMismatch,
    /// Native absence is not definite or native cleanup remains outstanding.
    OwnershipPhaseMismatch,
}

impl fmt::Display for ExtensionPackagePinReleaseDenial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "extension package-pin release denied: {self:?}")
    }
}

impl Error for ExtensionPackagePinReleaseDenial {}

macro_rules! binding_identity_accessors {
    () => {
        /// Complete profile/install/browsing-partition owner key.
        pub const fn key(&self) -> super::ExtensionNativeOwnershipKey {
            self.entry.key()
        }

        /// Durable profile owning this repository pin.
        pub const fn profile(&self) -> ProfileId {
            self.entry.key().profile()
        }

        /// Stable profile-scoped installation identity.
        pub const fn install_id(&self) -> ExtensionInstallId {
            self.entry.key().install_id()
        }

        /// Exact browsing partition represented by this owner.
        pub const fn browsing_context(&self) -> ExtensionGrantBrowsingContext {
            self.entry.key().browsing_context()
        }

        /// Exact package identity recorded before the native ownership call.
        pub fn package(&self) -> &ExtensionPackageIdentity {
            self.entry.package()
        }

        /// Exact authenticated catalog-set content identity selected at acquisition.
        pub const fn catalog_set_digest(&self) -> ExtensionCatalogSetDigest {
            self.entry.catalog_set_digest()
        }

        /// Historical active/rollback role selected at acquisition.
        pub const fn catalog_role(&self) -> ExtensionCatalogGenerationRole {
            self.entry.catalog_role()
        }

        /// Store catalog revision joined at acquisition.
        pub const fn store_catalog_revision(&self) -> ExtensionInstallCatalogRevision {
            self.entry.store_catalog_revision()
        }

        /// Store install-row revision joined at acquisition.
        pub const fn store_install_revision(&self) -> ExtensionInstallRevision {
            self.entry.store_install_revision()
        }

        /// Store grant-row revision joined at acquisition.
        pub const fn store_grant_revision(&self) -> ExtensionGrantRevision {
            self.entry.store_grant_revision()
        }

        /// Complete grant-authority digest joined at acquisition.
        pub const fn grant_digest(&self) -> ExtensionGrantDigest {
            self.entry.grant_digest()
        }

        /// Exact reviewed backend selected for this owner.
        pub const fn runtime_backend(&self) -> ExtensionRuntimeBackendTarget {
            self.entry.runtime_backend()
        }

        /// Catalog-authenticated native identity already persisted on the row,
        /// when this projection represents a post-authentication frontier.
        ///
        /// Fresh acquisition bindings are minted from `NativeAbsentPreparing`
        /// before repository authentication and therefore return `None` here.
        pub const fn expected_native_identity(
            &self,
        ) -> Option<ExtensionExpectedNativeOwnershipIdentity> {
            self.entry.expected_native_identity()
        }

        /// Exact backend-native owner identity, when one was observed.
        pub const fn native_identity(&self) -> Option<ExtensionNativeOwnershipIdentity> {
            self.entry.native_identity()
        }

        /// Persistent Store allocation identity preventing owner ABA.
        pub const fn native_incarnation(&self) -> ExtensionNativeIncarnation {
            self.entry.native_incarnation()
        }

        /// Store journal operation identity captured by this projection.
        pub const fn journal_operation(&self) -> ExtensionNativeOwnershipOperation {
            self.entry.operation()
        }

        /// Exact Store row revision captured by this projection.
        pub const fn journal_entry_revision(&self) -> ExtensionNativeOwnershipEntryRevision {
            self.entry.revision()
        }

        /// Complete compare-and-swap identity of the Store row at minting.
        pub const fn journal_entry_cas(&self) -> ExtensionNativeOwnershipEntryCas {
            self.entry.cas()
        }
    };
}

/// Move-only composite for acquiring one exact durable repository package pin.
///
/// Minting consumes the exact [`ExtensionRuntimeEligibility`] and joins all of
/// its durable identity and grant inputs to one Store-validated
/// `Acquire/NativeAbsentPreparing` ownership row. This prevents a repository
/// API from accepting eligibility separately from the pin owner it validates.
/// Possession proves that the structural join was valid when minted; it does
/// not prove that the Store row is still current or that repository bytes are
/// authenticated.
///
/// The composite deliberately cannot be cloned:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinAcquisitionBinding;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionPackagePinAcquisitionBinding>();
/// ```
///
/// Its fields are not a public construction surface:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinAcquisitionBinding;
/// let _ = ExtensionPackagePinAcquisitionBinding {
///     entry: panic!("not constructible"),
///     eligibility: panic!("not constructible"),
/// };
/// ```
///
/// It is process-local and cannot cross a serialization boundary:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinAcquisitionBinding;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<ExtensionPackagePinAcquisitionBinding>();
/// ```
///
/// Nor can one be reconstructed from persisted input:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinAcquisitionBinding;
/// fn requires_deserialize<T: serde::de::DeserializeOwned>() {}
/// requires_deserialize::<ExtensionPackagePinAcquisitionBinding>();
/// ```
#[must_use = "the serialized runtime service must retain this unresolved acquisition binding"]
pub struct ExtensionPackagePinAcquisitionBinding {
    entry: ExtensionNativeOwnershipEntry,
    eligibility: ExtensionRuntimeEligibility,
}

impl ExtensionPackagePinAcquisitionBinding {
    /// Mints a composite only from the exact safe pre-native-call frontier.
    ///
    /// The eligibility is consumed on both success and refusal. Callers must
    /// project a fresh complete Store cohort before retrying a denied join.
    pub fn mint(
        entry: &ExtensionNativeOwnershipEntry,
        eligibility: ExtensionRuntimeEligibility,
    ) -> Result<Self, ExtensionPackagePinAcquisitionDenial> {
        if let Some(reason) = acquisition_denial(entry, &eligibility) {
            return Err(reason);
        }

        Ok(Self {
            entry: entry.clone(),
            eligibility,
        })
    }

    /// Exact move-only runtime eligibility carried by this pre-handoff composite.
    ///
    /// The repository may borrow this for package and manifest validation,
    /// then must consume the composite through [`Self::into_runtime_parts`]
    /// before package access crosses into the native host. Only the resulting
    /// [`ExtensionRuntimeOperationAuthority`] may mint operation witnesses for
    /// the exact generation; the held pin deliberately cannot. Eligibility is
    /// intentionally not recoverable independently from this binding.
    pub const fn eligibility(&self) -> &ExtensionRuntimeEligibility {
        &self.eligibility
    }

    /// Conservative heap-plus-inline charge while this authority is retained.
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.eligibility.retained_heap_bytes())
    }

    /// Linearly separates runtime operation authority from the held pin.
    ///
    /// The opaque result keeps both pieces together until the repository has
    /// completed every fallible package-access preparation step. After it is
    /// consumed, the held binding must remain captive in the access provider
    /// and every refusal must recombine the exact operation authority before
    /// reconstructing an acquisition binding.
    pub fn into_runtime_parts(
        self,
        generation: ExtensionRuntimeGeneration,
    ) -> ExtensionPackagePinRuntimeParts {
        let lineage = ExtensionRuntimeOperationAuthorityLineage::from_entry(&self.entry);
        ExtensionPackagePinRuntimeParts {
            held: ExtensionPackagePinHeldBinding { entry: self.entry },
            operation_authority: self
                .eligibility
                .into_operation_authority(generation, lineage),
        }
    }

    binding_identity_accessors!();
}

impl fmt::Debug for ExtensionPackagePinAcquisitionBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionPackagePinAcquisitionBinding")
            .field("owner", &"<redacted>")
            .field("package", &"<redacted>")
            .field("catalog", &"<redacted>")
            .field("role", &self.catalog_role())
            .field("backend", &self.runtime_backend())
            .field("journal", &"<redacted>")
            .field("eligibility", &"<redacted>")
            .finish()
    }
}

/// Opaque linear handoff of a held repository pin and runtime authority.
///
/// This intermediate exists so a caller cannot accidentally extract one piece
/// while fallible package-access preparation is still underway. It is
/// move-only, process-local, and has no public fields.
///
/// The split deliberately predates catalog-expected native identity. That fact
/// is authenticated later by the repository and persisted in the current
/// `NativeMayOwn` row; it must then be joined at the host boundary rather than
/// synthesized into this earlier operation lineage.
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinRuntimeParts;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionPackagePinRuntimeParts>();
/// ```
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinRuntimeParts;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<ExtensionPackagePinRuntimeParts>();
/// ```
#[must_use = "runtime handoff parts must stay paired until package access is bound"]
pub struct ExtensionPackagePinRuntimeParts {
    held: ExtensionPackagePinHeldBinding,
    operation_authority: ExtensionRuntimeOperationAuthority,
}

impl ExtensionPackagePinRuntimeParts {
    /// Consumes the opaque handoff at the exact package-access ownership edge.
    pub fn into_held_binding_and_operation_authority(
        self,
    ) -> (
        ExtensionPackagePinHeldBinding,
        ExtensionRuntimeOperationAuthority,
    ) {
        (self.held, self.operation_authority)
    }

    /// Conservative logical charge retained by both linear pieces.
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.operation_authority.retained_heap_bytes())
    }
}

impl fmt::Debug for ExtensionPackagePinRuntimeParts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionPackagePinRuntimeParts")
            .field("held_pin", &"<redacted>")
            .field("operation_authority", &"<redacted>")
            .finish()
    }
}

/// Move-only same-process authority retaining one exact durable package pin.
///
/// The repository keeps this compact value inside delegated package access;
/// it intentionally contains no manifest/grant owners and cannot mint runtime
/// operation capabilities. Reopened cleanup cannot reconstruct it. When
/// package-access construction refuses before activation, the repository may
/// recombine it only with the exact operation authority split from the same
/// Store-bound fields.
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinHeldBinding;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionPackagePinHeldBinding>();
/// ```
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinHeldBinding;
/// fn requires_deserialize<T: serde::de::DeserializeOwned>() {}
/// requires_deserialize::<ExtensionPackagePinHeldBinding>();
/// ```
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinHeldBinding;
/// fn cannot_mint(binding: &ExtensionPackagePinHeldBinding) {
///     let _ = binding.eligibility();
/// }
/// ```
#[must_use = "the exact held package pin must remain captive until teardown settlement"]
pub struct ExtensionPackagePinHeldBinding {
    entry: ExtensionNativeOwnershipEntry,
}

impl ExtensionPackagePinHeldBinding {
    /// Reconstructs the original acquisition binding only from an exact pair.
    ///
    /// A mismatch is an internal authority-routing defect. The refusal keeps
    /// both capabilities captive so neither can be reused or silently dropped
    /// as if a valid acquisition had been recovered.
    pub fn try_recombine(
        self,
        operation_authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<ExtensionPackagePinAcquisitionBinding, ExtensionPackagePinRecombineRefusal> {
        if !operation_authority.matches_native_ownership_lineage(&self.entry) {
            return Err(ExtensionPackagePinRecombineRefusal {
                reason: ExtensionPackagePinAcquisitionDenial::OperationAuthorityLineageMismatch,
                parts: Box::new(ExtensionPackagePinRuntimeParts {
                    held: self,
                    operation_authority,
                }),
            });
        }
        if let Some(reason) = acquisition_denial(&self.entry, operation_authority.eligibility()) {
            return Err(ExtensionPackagePinRecombineRefusal {
                reason,
                parts: Box::new(ExtensionPackagePinRuntimeParts {
                    held: self,
                    operation_authority,
                }),
            });
        }
        Ok(ExtensionPackagePinAcquisitionBinding {
            entry: self.entry,
            eligibility: operation_authority.into_eligibility(),
        })
    }

    /// Conservative inline charge for the compact held-pin authority.
    pub const fn retained_bytes(&self) -> usize {
        size_of::<Self>()
    }

    binding_identity_accessors!();
}

impl fmt::Debug for ExtensionPackagePinHeldBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionPackagePinHeldBinding")
            .field("owner", &"<redacted>")
            .field("package", &"<redacted>")
            .field("role", &self.catalog_role())
            .field("backend", &self.runtime_backend())
            .field("journal", &"<redacted>")
            .finish()
    }
}

/// Fail-closed result of attempting to join unrelated linear authorities.
///
/// A mismatch is impossible in a correct runtime pipeline. Both exact pieces
/// remain recoverable only so the serialized coordinator can quarantine them
/// while the durable journal drives cleanup; neither piece is discarded by
/// the failed join.
#[must_use = "a recombination mismatch retains both authorities in quarantine"]
pub struct ExtensionPackagePinRecombineRefusal {
    reason: ExtensionPackagePinAcquisitionDenial,
    parts: Box<ExtensionPackagePinRuntimeParts>,
}

impl ExtensionPackagePinRecombineRefusal {
    /// Stable, identity-free reason the exact join was refused.
    pub const fn reason(&self) -> ExtensionPackagePinAcquisitionDenial {
        self.reason
    }

    /// Recovers both exact capabilities for explicit coordinator quarantine.
    pub fn into_parts(
        self,
    ) -> (
        ExtensionPackagePinHeldBinding,
        ExtensionRuntimeOperationAuthority,
    ) {
        (*self.parts).into_held_binding_and_operation_authority()
    }

    /// Conservative logical charge retained by the quarantined pair.
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(self.parts.retained_bytes())
            .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
    }
}

impl fmt::Debug for ExtensionPackagePinRecombineRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionPackagePinRecombineRefusal")
            .field("reason", &self.reason)
            .field("authority", &"<redacted>")
            .finish()
    }
}

/// Move-only structural projection for releasing one exact repository pin.
///
/// Minting requires the Store-validated
/// `Release/NativeAbsentReleasePending` frontier, where native absence is
/// definite and only subordinate package/resource release remains. This type
/// deliberately carries no runtime eligibility or package-access authority,
/// so reopened crash recovery cannot reconstruct executable package access.
/// Possession does not prove that the Store row is still current; the service
/// remains responsible for journal serialization and final compare-and-swap.
///
/// The projection deliberately cannot be cloned:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinReleaseBinding;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionPackagePinReleaseBinding>();
/// ```
///
/// Its fields are not a public construction surface:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinReleaseBinding;
/// let _ = ExtensionPackagePinReleaseBinding {
///     entry: panic!("not constructible"),
/// };
/// ```
///
/// It is process-local and cannot cross a serialization boundary:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinReleaseBinding;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<ExtensionPackagePinReleaseBinding>();
/// ```
///
/// Nor can one be reconstructed from persisted input:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinReleaseBinding;
/// fn requires_deserialize<T: serde::de::DeserializeOwned>() {}
/// requires_deserialize::<ExtensionPackagePinReleaseBinding>();
/// ```
///
/// Cleanup cannot recover runtime eligibility or executable package access:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionPackagePinReleaseBinding;
/// fn escape_cleanup(binding: &ExtensionPackagePinReleaseBinding) {
///     let _ = binding.eligibility();
/// }
/// ```
#[must_use = "retain this binding until the exact repository pin settles"]
pub struct ExtensionPackagePinReleaseBinding {
    entry: ExtensionNativeOwnershipEntry,
}

impl ExtensionPackagePinReleaseBinding {
    /// Mints a cleanup-only projection only after native absence is definite.
    pub fn mint(
        entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<Self, ExtensionPackagePinReleaseDenial> {
        if entry.intent() != ExtensionNativeOwnershipIntent::Release {
            return Err(ExtensionPackagePinReleaseDenial::OwnershipIntentMismatch);
        }
        if entry.phase() != ExtensionNativeOwnershipPhase::NativeAbsentReleasePending {
            return Err(ExtensionPackagePinReleaseDenial::OwnershipPhaseMismatch);
        }
        Ok(Self {
            entry: entry.clone(),
        })
    }

    binding_identity_accessors!();
}

impl fmt::Debug for ExtensionPackagePinReleaseBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionPackagePinReleaseBinding")
            .field("owner", &"<redacted>")
            .field("package", &"<redacted>")
            .field("catalog", &"<redacted>")
            .field("role", &self.catalog_role())
            .field("backend", &self.runtime_backend())
            .field("journal", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::extensions::{
        ApiPermissionName, ExtensionApiPermissionSet, ExtensionArchiveDigest, ExtensionAuthorityId,
        ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
        ExtensionCompatibilityTargetId, ExtensionContentSecurityPolicyDeclaration,
        ExtensionGrantAuthority, ExtensionGrantCohort, ExtensionGrantManifestBinding,
        ExtensionGrantManifestBindings, ExtensionHostPermissionSet, ExtensionInstall,
        ExtensionInstallCatalog, ExtensionManifestDeclarations, ExtensionManifestDescriptor,
        ExtensionManifestDigest, ExtensionManifestExecutionSurfaces,
        ExtensionManifestResourceDigest, ExtensionPackageKey, ExtensionPackagePayloadIdentity,
        ExtensionPackageRevision, ExtensionTreeDigest,
    };
    use crate::injection::{MatchOptions, MatchPattern, MatchSet};

    const ALL_URLS: &str = "<all_urls>";

    fn package(seed: u8) -> ExtensionPackageIdentity {
        ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([seed; 32]),
            ExtensionPackageKey::from_bytes([seed.wrapping_add(1); 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionPackagePayloadIdentity::acquired_zip(
                3,
                ExtensionArchiveDigest::from_bytes([seed.wrapping_add(2); 32]),
            )
            .unwrap(),
            ExtensionManifestDigest::from_bytes([seed.wrapping_add(3); 32]),
            ExtensionTreeDigest::from_bytes([seed.wrapping_add(4); 32]),
        )
    }

    fn api(names: &[&str]) -> ExtensionApiPermissionSet {
        ExtensionApiPermissionSet::new(
            names
                .iter()
                .map(|name| ApiPermissionName::parse_exact(name).unwrap())
                .collect(),
        )
        .unwrap()
    }

    fn manifest(package: ExtensionPackageIdentity) -> Arc<ExtensionManifestDescriptor> {
        let declarations = ExtensionManifestDeclarations::new(
            api(&[]),
            api(&["activeTab", "scripting"]),
            None,
            Some(
                ExtensionHostPermissionSet::new(
                    MatchSet::parse(
                        [ALL_URLS],
                        std::iter::empty::<&str>(),
                        MatchOptions::default(),
                    )
                    .unwrap(),
                )
                .unwrap(),
            ),
            None,
            None,
            Vec::new(),
            ExtensionManifestExecutionSurfaces::new(
                Vec::new(),
                ExtensionContentSecurityPolicyDeclaration::new(
                    ExtensionManifestResourceDigest::from_bytes([61; 32]),
                ),
                None,
                Vec::new(),
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap();
        let compatibility = declarations
            .declaration_keys()
            .into_iter()
            .map(|declaration| {
                ExtensionCompatibilityClassification::new(
                    declaration,
                    ExtensionCompatibilityLevel::Compatible,
                )
            })
            .collect();
        Arc::new(
            ExtensionManifestDescriptor::new(
                package,
                3,
                declarations,
                ExtensionCompatibilityTargetId::parse_exact("test.package.pin.v1").unwrap(),
                compatibility,
            )
            .unwrap(),
        )
    }

    fn runtime_eligibility_for(
        profile: ProfileId,
        install_id: ExtensionInstallId,
    ) -> ExtensionRuntimeEligibility {
        let manifest = manifest(package(17));
        let install = ExtensionInstall::from_persisted(
            install_id,
            ExtensionInstallRevision::new(19).unwrap(),
            manifest.package().clone(),
            true,
        );
        let catalog = ExtensionInstallCatalog::from_persisted(
            ExtensionInstallCatalogRevision::new(23).unwrap(),
            Some(install_id),
            vec![install.clone()],
        )
        .unwrap();
        let bindings =
            ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
                install_id,
                Arc::clone(&manifest),
            )])
            .unwrap();
        let authority = ExtensionGrantAuthority::initialize(
            &install,
            ["activeTab", "scripting"]
                .into_iter()
                .map(|name| ApiPermissionName::parse_exact(name).unwrap())
                .collect(),
            vec![MatchPattern::parse(ALL_URLS).unwrap()],
            false,
            false,
            &manifest,
        )
        .unwrap();
        ExtensionGrantCohort::from_persisted(profile, catalog, bindings, vec![authority])
            .unwrap()
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap()
    }

    fn runtime_eligibility() -> ExtensionRuntimeEligibility {
        runtime_eligibility_for(ProfileId::from(11), ExtensionInstallId::from(13))
    }

    struct EntryFixture {
        key: super::super::ExtensionNativeOwnershipKey,
        package: ExtensionPackageIdentity,
        catalog_set_digest: ExtensionCatalogSetDigest,
        catalog_role: ExtensionCatalogGenerationRole,
        catalog_revision: ExtensionInstallCatalogRevision,
        install_revision: ExtensionInstallRevision,
        grant_revision: ExtensionGrantRevision,
        grant_digest: ExtensionGrantDigest,
        backend: ExtensionRuntimeBackendTarget,
    }

    impl EntryFixture {
        fn from_eligibility(eligibility: &ExtensionRuntimeEligibility) -> Self {
            Self {
                key: super::super::ExtensionNativeOwnershipKey::new(
                    eligibility.profile(),
                    eligibility.install_id(),
                    eligibility.browsing_context(),
                ),
                package: eligibility.package().clone(),
                catalog_set_digest: ExtensionCatalogSetDigest::from_bytes([29; 32]),
                catalog_role: ExtensionCatalogGenerationRole::Active,
                catalog_revision: eligibility.catalog_revision(),
                install_revision: eligibility.install_revision(),
                grant_revision: eligibility.grant_revision(),
                grant_digest: eligibility.grant_digest(),
                backend: ExtensionRuntimeBackendTarget::LinuxCompatibility,
            }
        }

        fn entry(
            &self,
            revision: u64,
            intent: ExtensionNativeOwnershipIntent,
            phase: ExtensionNativeOwnershipPhase,
        ) -> ExtensionNativeOwnershipEntry {
            self.entry_with_operation(revision, 1, intent, phase)
        }

        fn entry_with_operation(
            &self,
            revision: u64,
            operation: u64,
            intent: ExtensionNativeOwnershipIntent,
            phase: ExtensionNativeOwnershipPhase,
        ) -> ExtensionNativeOwnershipEntry {
            ExtensionNativeOwnershipEntry::from_persisted(
                self.key,
                ExtensionNativeOwnershipOperation::new(operation).unwrap(),
                ExtensionNativeOwnershipEntryRevision::new(revision).unwrap(),
                self.package.clone(),
                self.catalog_set_digest,
                self.catalog_role,
                self.catalog_revision,
                self.install_revision,
                self.grant_revision,
                self.grant_digest,
                self.backend,
                ExtensionNativeIncarnation::new(operation).unwrap(),
                intent,
                phase,
            )
            .unwrap()
        }

        fn acquisition(&self) -> ExtensionNativeOwnershipEntry {
            self.entry(
                1,
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
            )
        }

        fn release_pending(&self) -> ExtensionNativeOwnershipEntry {
            self.entry(
                4,
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            )
        }
    }

    #[test]
    fn acquisition_binding_carries_the_complete_exact_join() {
        let eligibility = runtime_eligibility();
        let fixture = EntryFixture::from_eligibility(&eligibility);
        let entry = fixture.acquisition();
        let binding = ExtensionPackagePinAcquisitionBinding::mint(&entry, eligibility).unwrap();

        assert_eq!(binding.key(), entry.key());
        assert_eq!(binding.profile(), entry.key().profile());
        assert_eq!(binding.install_id(), entry.key().install_id());
        assert_eq!(binding.browsing_context(), entry.key().browsing_context());
        assert_eq!(binding.package(), entry.package());
        assert_eq!(binding.catalog_set_digest(), entry.catalog_set_digest());
        assert_eq!(binding.catalog_role(), entry.catalog_role());
        assert_eq!(
            binding.store_catalog_revision(),
            entry.store_catalog_revision()
        );
        assert_eq!(
            binding.store_install_revision(),
            entry.store_install_revision()
        );
        assert_eq!(binding.store_grant_revision(), entry.store_grant_revision());
        assert_eq!(binding.grant_digest(), entry.grant_digest());
        assert_eq!(binding.runtime_backend(), entry.runtime_backend());
        assert_eq!(binding.expected_native_identity(), None);
        assert_eq!(binding.native_identity(), None);
        assert_eq!(binding.native_incarnation(), entry.native_incarnation());
        assert_eq!(binding.journal_operation(), entry.operation());
        assert_eq!(binding.journal_entry_revision(), entry.revision());
        assert_eq!(binding.journal_entry_cas(), entry.cas());
        assert_eq!(binding.eligibility().profile(), entry.key().profile());
        assert_eq!(binding.eligibility().package(), entry.package());
        assert!(binding.retained_bytes() >= size_of::<ExtensionPackagePinAcquisitionBinding>());
        assert!(binding.retained_bytes() >= binding.eligibility().retained_bytes());
    }

    #[test]
    fn runtime_split_preserves_exact_generation_pin_and_recombination() {
        let eligibility = runtime_eligibility();
        let fixture = EntryFixture::from_eligibility(&eligibility);
        let entry = fixture.acquisition();
        let generation = ExtensionRuntimeGeneration::new(31).unwrap();
        let binding = ExtensionPackagePinAcquisitionBinding::mint(&entry, eligibility).unwrap();
        let parts = binding.into_runtime_parts(generation);

        assert!(parts.retained_bytes() >= size_of::<ExtensionPackagePinRuntimeParts>());
        assert_eq!(
            format!("{parts:?}"),
            "ExtensionPackagePinRuntimeParts { held_pin: \"<redacted>\", operation_authority: \"<redacted>\" }"
        );
        let (held, operation_authority) = parts.into_held_binding_and_operation_authority();
        assert_eq!(held.key(), entry.key());
        assert_eq!(held.package(), entry.package());
        assert_eq!(held.runtime_backend(), entry.runtime_backend());
        assert_eq!(held.journal_entry_cas(), entry.cas());
        assert_eq!(
            held.retained_bytes(),
            size_of::<ExtensionPackagePinHeldBinding>()
        );
        assert_eq!(
            operation_authority.fingerprint().instance().generation(),
            generation
        );
        assert_eq!(
            operation_authority.fingerprint().instance().profile(),
            entry.key().profile()
        );
        assert_eq!(operation_authority.fingerprint().package(), entry.package());

        let recombined = held.try_recombine(operation_authority).unwrap();
        assert_eq!(recombined.key(), entry.key());
        assert_eq!(recombined.package(), entry.package());
        assert_eq!(recombined.eligibility().profile(), entry.key().profile());
        assert_eq!(recombined.eligibility().package(), entry.package());
    }

    #[test]
    fn runtime_recombination_refusal_retains_both_exact_capabilities() {
        let eligibility = runtime_eligibility();
        let fixture = EntryFixture::from_eligibility(&eligibility);
        let entry = fixture.acquisition();
        let binding = ExtensionPackagePinAcquisitionBinding::mint(&entry, eligibility).unwrap();
        let (held, _exact_authority) = binding
            .into_runtime_parts(ExtensionRuntimeGeneration::new(37).unwrap())
            .into_held_binding_and_operation_authority();
        let wrong_eligibility =
            runtime_eligibility_for(ProfileId::from(41), ExtensionInstallId::from(13));
        let wrong_fixture = EntryFixture::from_eligibility(&wrong_eligibility);
        let wrong_entry = wrong_fixture.acquisition();
        let wrong_authority =
            ExtensionPackagePinAcquisitionBinding::mint(&wrong_entry, wrong_eligibility)
                .unwrap()
                .into_runtime_parts(ExtensionRuntimeGeneration::new(43).unwrap())
                .into_held_binding_and_operation_authority()
                .1;

        let refusal = held.try_recombine(wrong_authority).unwrap_err();
        assert_eq!(
            refusal.reason(),
            ExtensionPackagePinAcquisitionDenial::OperationAuthorityLineageMismatch
        );
        assert!(refusal.retained_bytes() >= size_of::<ExtensionPackagePinRecombineRefusal>());
        assert_eq!(
            format!("{refusal:?}"),
            "ExtensionPackagePinRecombineRefusal { reason: OperationAuthorityLineageMismatch, authority: \"<redacted>\" }"
        );
        let (held, wrong_authority) = refusal.into_parts();
        assert_eq!(held.key(), entry.key());
        assert_eq!(
            wrong_authority.fingerprint().instance().profile(),
            ProfileId::from(41)
        );
    }

    #[test]
    fn same_runtime_fingerprint_cannot_cross_package_pin_operation_lineage() {
        let first_eligibility = runtime_eligibility();
        let fixture = EntryFixture::from_eligibility(&first_eligibility);
        let first_entry = fixture.entry_with_operation(
            1,
            1,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
        );
        let (first_held, first_authority) =
            ExtensionPackagePinAcquisitionBinding::mint(&first_entry, first_eligibility)
                .unwrap()
                .into_runtime_parts(ExtensionRuntimeGeneration::new(47).unwrap())
                .into_held_binding_and_operation_authority();

        let second_eligibility = runtime_eligibility();
        let second_entry = fixture.entry_with_operation(
            1,
            2,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
        );
        let (second_held, second_authority) =
            ExtensionPackagePinAcquisitionBinding::mint(&second_entry, second_eligibility)
                .unwrap()
                .into_runtime_parts(ExtensionRuntimeGeneration::new(47).unwrap())
                .into_held_binding_and_operation_authority();

        assert_eq!(
            first_authority.fingerprint(),
            second_authority.fingerprint()
        );
        assert!(!second_authority.matches_native_ownership_lineage(&first_entry));
        let refusal = first_held.try_recombine(second_authority).unwrap_err();
        assert_eq!(
            refusal.reason(),
            ExtensionPackagePinAcquisitionDenial::OperationAuthorityLineageMismatch
        );
        let (first_held, second_authority) = refusal.into_parts();
        assert!(second_held.try_recombine(second_authority).is_ok());
        assert!(first_held.try_recombine(first_authority).is_ok());
    }

    #[test]
    fn acquisition_binding_rejects_every_independent_eligibility_field_mismatch() {
        fn assert_denial(
            mutate: impl FnOnce(&mut EntryFixture),
            expected: ExtensionPackagePinAcquisitionDenial,
        ) {
            let eligibility = runtime_eligibility();
            let mut fixture = EntryFixture::from_eligibility(&eligibility);
            mutate(&mut fixture);
            let entry = fixture.acquisition();
            assert_eq!(
                ExtensionPackagePinAcquisitionBinding::mint(&entry, eligibility).unwrap_err(),
                expected
            );
        }

        assert_denial(
            |fixture| {
                fixture.key = super::super::ExtensionNativeOwnershipKey::new(
                    ProfileId::from(31),
                    fixture.key.install_id(),
                    fixture.key.browsing_context(),
                );
            },
            ExtensionPackagePinAcquisitionDenial::EligibilityProfileMismatch,
        );
        assert_denial(
            |fixture| {
                fixture.key = super::super::ExtensionNativeOwnershipKey::new(
                    fixture.key.profile(),
                    ExtensionInstallId::from(37),
                    fixture.key.browsing_context(),
                );
            },
            ExtensionPackagePinAcquisitionDenial::EligibilityInstallMismatch,
        );
        assert_denial(
            |fixture| {
                fixture.key = super::super::ExtensionNativeOwnershipKey::new(
                    fixture.key.profile(),
                    fixture.key.install_id(),
                    ExtensionGrantBrowsingContext::Private,
                );
            },
            ExtensionPackagePinAcquisitionDenial::EligibilityBrowsingContextMismatch,
        );
        assert_denial(
            |fixture| fixture.package = package(41),
            ExtensionPackagePinAcquisitionDenial::EligibilityPackageMismatch,
        );
        assert_denial(
            |fixture| {
                fixture.catalog_revision = fixture.catalog_revision.next().unwrap();
            },
            ExtensionPackagePinAcquisitionDenial::EligibilityCatalogRevisionMismatch,
        );
        assert_denial(
            |fixture| fixture.install_revision = fixture.install_revision.next().unwrap(),
            ExtensionPackagePinAcquisitionDenial::EligibilityInstallRevisionMismatch,
        );
        assert_denial(
            |fixture| fixture.grant_revision = fixture.grant_revision.next().unwrap(),
            ExtensionPackagePinAcquisitionDenial::EligibilityGrantRevisionMismatch,
        );
        assert_denial(
            |fixture| fixture.grant_digest = ExtensionGrantDigest::from_bytes([43; 32]),
            ExtensionPackagePinAcquisitionDenial::EligibilityGrantDigestMismatch,
        );
    }

    #[test]
    fn acquisition_binding_requires_the_exact_safe_frontier() {
        let eligibility = runtime_eligibility();
        let fixture = EntryFixture::from_eligibility(&eligibility);
        let release = fixture.release_pending();
        assert_eq!(
            ExtensionPackagePinAcquisitionBinding::mint(&release, eligibility).unwrap_err(),
            ExtensionPackagePinAcquisitionDenial::OwnershipIntentMismatch
        );

        let eligibility = runtime_eligibility();
        let fixture = EntryFixture::from_eligibility(&eligibility);
        let may_own = fixture.entry(
            2,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        );
        assert_eq!(
            ExtensionPackagePinAcquisitionBinding::mint(&may_own, eligibility).unwrap_err(),
            ExtensionPackagePinAcquisitionDenial::OwnershipPhaseMismatch
        );
    }

    #[test]
    fn release_binding_is_cleanup_only_and_carries_the_exact_row() {
        let eligibility = runtime_eligibility();
        let mut fixture = EntryFixture::from_eligibility(&eligibility);
        fixture.catalog_role = ExtensionCatalogGenerationRole::Rollback;
        fixture.backend = ExtensionRuntimeBackendTarget::MacosCompatibility;
        let entry = fixture.release_pending();
        let binding = ExtensionPackagePinReleaseBinding::mint(&entry).unwrap();

        assert_eq!(binding.key(), entry.key());
        assert_eq!(binding.profile(), entry.key().profile());
        assert_eq!(binding.install_id(), entry.key().install_id());
        assert_eq!(binding.browsing_context(), entry.key().browsing_context());
        assert_eq!(binding.package(), entry.package());
        assert_eq!(binding.catalog_set_digest(), entry.catalog_set_digest());
        assert_eq!(
            binding.catalog_role(),
            ExtensionCatalogGenerationRole::Rollback
        );
        assert_eq!(
            binding.store_catalog_revision(),
            entry.store_catalog_revision()
        );
        assert_eq!(
            binding.store_install_revision(),
            entry.store_install_revision()
        );
        assert_eq!(binding.store_grant_revision(), entry.store_grant_revision());
        assert_eq!(binding.grant_digest(), entry.grant_digest());
        assert_eq!(binding.runtime_backend(), entry.runtime_backend());
        assert_eq!(binding.native_identity(), entry.native_identity());
        assert_eq!(binding.native_incarnation(), entry.native_incarnation());
        assert_eq!(binding.journal_operation(), entry.operation());
        assert_eq!(binding.journal_entry_revision(), entry.revision());
        assert_eq!(binding.journal_entry_cas(), entry.cas());
    }

    #[test]
    fn release_binding_rejects_wrong_intent_and_phase_but_preserves_any_partition() {
        let eligibility = runtime_eligibility();
        let fixture = EntryFixture::from_eligibility(&eligibility);
        assert_eq!(
            ExtensionPackagePinReleaseBinding::mint(&fixture.acquisition()).unwrap_err(),
            ExtensionPackagePinReleaseDenial::OwnershipIntentMismatch
        );
        let release_may_own = fixture.entry(
            3,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        );
        assert_eq!(
            ExtensionPackagePinReleaseBinding::mint(&release_may_own).unwrap_err(),
            ExtensionPackagePinReleaseDenial::OwnershipPhaseMismatch
        );

        let mut private = fixture;
        private.key = super::super::ExtensionNativeOwnershipKey::new(
            private.key.profile(),
            private.key.install_id(),
            ExtensionGrantBrowsingContext::Private,
        );
        let private_entry = private.release_pending();
        let private_binding = ExtensionPackagePinReleaseBinding::mint(&private_entry).unwrap();
        assert_eq!(
            private_binding.browsing_context(),
            ExtensionGrantBrowsingContext::Private
        );
    }

    #[test]
    fn binding_debug_is_bounded_and_redacts_owner_package_catalog_and_journal() {
        let eligibility = runtime_eligibility();
        let fixture = EntryFixture::from_eligibility(&eligibility);
        let acquisition =
            ExtensionPackagePinAcquisitionBinding::mint(&fixture.acquisition(), eligibility)
                .unwrap();
        assert_eq!(
            format!("{acquisition:?}"),
            "ExtensionPackagePinAcquisitionBinding { owner: \"<redacted>\", package: \"<redacted>\", catalog: \"<redacted>\", role: Active, backend: LinuxCompatibility, journal: \"<redacted>\", eligibility: \"<redacted>\" }"
        );

        let release = ExtensionPackagePinReleaseBinding::mint(&fixture.release_pending()).unwrap();
        assert_eq!(
            format!("{release:?}"),
            "ExtensionPackagePinReleaseBinding { owner: \"<redacted>\", package: \"<redacted>\", catalog: \"<redacted>\", role: Active, backend: LinuxCompatibility, journal: \"<redacted>\" }"
        );
    }

    #[test]
    fn malformed_entry_revisions_are_rejected_before_a_binding_can_be_minted() {
        let eligibility = runtime_eligibility();
        let fixture = EntryFixture::from_eligibility(&eligibility);
        assert!(matches!(
            ExtensionNativeOwnershipEntry::from_persisted(
                fixture.key,
                ExtensionNativeOwnershipOperation::INITIAL,
                ExtensionNativeOwnershipEntryRevision::new(2).unwrap(),
                fixture.package,
                fixture.catalog_set_digest,
                fixture.catalog_role,
                fixture.catalog_revision,
                fixture.install_revision,
                fixture.grant_revision,
                fixture.grant_digest,
                fixture.backend,
                ExtensionNativeIncarnation::INITIAL,
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
            ),
            Err(super::super::ExtensionNativeOwnershipJournalError::InvalidEntryRevision { .. })
        ));
    }

    #[test]
    fn malformed_native_identity_and_incarnation_are_rejected_before_binding_minting() {
        let eligibility = runtime_eligibility();
        let mut fixture = EntryFixture::from_eligibility(&eligibility);
        fixture.backend = ExtensionRuntimeBackendTarget::MacosNative;
        let native_identity =
            ExtensionNativeOwnershipIdentity::parse(fixture.backend, &"a".repeat(32)).unwrap();
        assert!(matches!(
            ExtensionNativeOwnershipEntry::from_persisted_with_native_identity(
                fixture.key,
                ExtensionNativeOwnershipOperation::INITIAL,
                ExtensionNativeOwnershipEntryRevision::INITIAL,
                fixture.package.clone(),
                fixture.catalog_set_digest,
                fixture.catalog_role,
                fixture.catalog_revision,
                fixture.install_revision,
                fixture.grant_revision,
                fixture.grant_digest,
                fixture.backend,
                Some(native_identity),
                ExtensionNativeIncarnation::INITIAL,
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
            ),
            Err(super::super::ExtensionNativeOwnershipJournalError::InvalidNativeIdentity)
        ));
        assert!(matches!(
            ExtensionNativeOwnershipEntry::from_persisted(
                fixture.key,
                ExtensionNativeOwnershipOperation::INITIAL,
                ExtensionNativeOwnershipEntryRevision::INITIAL,
                fixture.package,
                fixture.catalog_set_digest,
                fixture.catalog_role,
                fixture.catalog_revision,
                fixture.install_revision,
                fixture.grant_revision,
                fixture.grant_digest,
                fixture.backend,
                ExtensionNativeIncarnation::new(2).unwrap(),
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
            ),
            Err(super::super::ExtensionNativeOwnershipJournalError::OperationIncarnationMismatch)
        ));
    }

    #[test]
    fn bindings_can_cross_the_serialized_service_worker_boundary() {
        fn requires_send<T: Send>() {}
        requires_send::<ExtensionPackagePinAcquisitionBinding>();
        requires_send::<ExtensionPackagePinReleaseBinding>();
    }
}
