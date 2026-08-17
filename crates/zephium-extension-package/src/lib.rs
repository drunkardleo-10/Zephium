//! Authentication-adjacent parsing for immutable extension release packages.
//!
//! This crate owns bounded upstream byte formats which do not belong in
//! `zephium-core`: portable package-relative paths, duplicate-key-safe JSON,
//! canonical resource-tree indexes, release catalog metadata, Chromium CRX3
//! signature verification, and manifest-key-derived extension identity. It deliberately performs no
//! filesystem I/O, archive extraction, network access, native mutation, or
//! profile persistence.
//!
//! The values produced here remain structural until a release or repository
//! authority authenticates their exact bytes. In particular, parsing a
//! catalog never makes its packages trusted by itself. Structural manifest
//! admission additionally requires an exact release/tree binding and an
//! explicit compatibility decision for every typed or unmodeled declaration.
//! Its public policy trait is intentionally suitable for analysis and tests:
//! neither a caller-selected policy nor [`AdmittedExtensionManifest`] is
//! product authentication or native-activation authority. The sealed authority
//! crate must authenticate the catalog and apply Zephium-owned exact backend
//! policy; native activation additionally requires an exact sealed
//! materialization receipt and live lease. Unknown top-level declarations remain
//! runtime-blocking authority; they are never silently treated as compatible.
//! Manifest UI strings remain typed as literal-or-localized until the exact
//! admitted default-locale resource is supplied to the bounded resolver.

#![deny(unsafe_code)]
#![deny(missing_docs)]

mod chromium;
mod crx3;
mod digest;
mod json;
mod limits;
mod manifest;
mod relative_path;
mod release;
mod tree;

pub use chromium::{
    ChromiumExtensionId, ChromiumManifestKey, ChromiumManifestKeyDigest, ChromiumManifestKeyError,
};
pub use crx3::{Crx3PackageError, Crx3SigningRequest, VerifiedCrx3Package};
pub use json::{parse_bounded_json, BoundedJsonError, BoundedJsonLimits, BoundedJsonValue};
pub use limits::*;
pub use manifest::{
    admit_extension_manifest, resolve_extension_default_locale,
    resolve_extension_metadata_default_locale, AdmittedExtensionManifest,
    ExtensionContentScriptResources, ExtensionDeclaredResourcePattern,
    ExtensionDefaultLocaleResolutionError, ExtensionLocalizedMessageKey,
    ExtensionManifestAdmissionDigest, ExtensionManifestAdmissionError,
    ExtensionManifestCompatibilityPolicy, ExtensionManifestCompatibilitySubject,
    ExtensionManifestIcon, ExtensionManifestMetadata, ExtensionManifestResource,
    ExtensionManifestResourcePlan, ExtensionOverrideResource, ExtensionResolvedMetadataDigest,
    ExtensionUnresolvedDisplayText, ExtensionWebAccessibleAudience,
    ExtensionWebAccessibleResourceGroup, ResolvedExtensionManifestMetadata,
    TrustedExtensionDisplayText,
};
pub use relative_path::{PortableRelativePath, PortableRelativePathError};
pub use release::{
    ExpectedChromiumIdentity, ExtensionCompatibilityReceiptDigest,
    ExtensionPackageAdmissionPolicyDigest, ExtensionReleaseAdmissionPolicy,
    ExtensionReleaseAdmissionPolicyError, ExtensionReleaseCatalog, ExtensionReleaseCatalogDigest,
    ExtensionReleaseCatalogError, ExtensionReleaseCatalogRevision,
    ExtensionReleaseCompatibilityReceipt, ExtensionReleaseLegalArtifactKind,
    ExtensionReleaseLegalNotice, ExtensionReleaseLicenseRule, ExtensionReleasePackage,
    ExtensionReleasePackageProvenance, ExtensionReleasePolicyBinding,
    ExtensionReleaseSourceReference, ExtensionReleaseTreeBinding,
};
pub use tree::{
    CanonicalExtensionTreeIndex, ExtensionTreeFile, ExtensionTreeIndexDigest,
    ExtensionTreeIndexError,
};
pub use zephium_core::extensions::MAX_EXTENSION_ARCHIVE_BYTES;
