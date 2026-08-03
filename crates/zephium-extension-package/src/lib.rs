//! Authentication-adjacent parsing for immutable extension release packages.
//!
//! This crate owns bounded upstream byte formats which do not belong in
//! `zephium-core`: portable package-relative paths, duplicate-key-safe JSON,
//! canonical resource-tree indexes, release catalog metadata, and Chromium's
//! manifest-key-derived extension identity. It deliberately performs no
//! filesystem I/O, archive extraction, network access, native mutation, or
//! profile persistence.
//!
//! The values produced here remain structural until a release or repository
//! authority authenticates their exact bytes. In particular, parsing a
//! catalog never makes its packages trusted by itself.

#![deny(unsafe_code)]
#![deny(missing_docs)]

mod chromium;
mod digest;
mod json;
mod limits;
mod relative_path;
mod release;
mod tree;

pub use chromium::{
    ChromiumExtensionId, ChromiumManifestKey, ChromiumManifestKeyDigest, ChromiumManifestKeyError,
};
pub use json::{parse_bounded_json, BoundedJsonError, BoundedJsonLimits, BoundedJsonValue};
pub use limits::*;
pub use relative_path::{PortableRelativePath, PortableRelativePathError};
pub use release::{
    ExpectedChromiumIdentity, ExtensionPackageAdmissionPolicyDigest,
    ExtensionReleaseAdmissionPolicy, ExtensionReleaseAdmissionPolicyError, ExtensionReleaseCatalog,
    ExtensionReleaseCatalogDigest, ExtensionReleaseCatalogError, ExtensionReleaseCatalogRevision,
    ExtensionReleaseLegalArtifactKind, ExtensionReleaseLegalNotice, ExtensionReleaseLicenseRule,
    ExtensionReleasePackage, ExtensionReleasePackageProvenance, ExtensionReleasePolicyBinding,
    ExtensionReleaseSourceReference, ExtensionReleaseTreeBinding,
};
pub use tree::{
    CanonicalExtensionTreeIndex, ExtensionTreeFile, ExtensionTreeIndexDigest,
    ExtensionTreeIndexError,
};
pub use zephium_core::extensions::MAX_EXTENSION_ARCHIVE_BYTES;
