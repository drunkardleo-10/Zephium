//! Pure, bounded contracts for authenticated extension-package identities and
//! per-profile installations.
//!
//! This module deliberately contains no manifest/archive parser, filesystem
//! path, permission grant, native runtime, or UI state. A package identity is
//! only a structural key: possession of one is not proof that package bytes
//! were authenticated or remain available. A later package-authority adapter
//! must establish that proof before the application can activate an install.

mod identity;
mod install;

pub use identity::{
    ExtensionArchiveDigest, ExtensionAuthorityId, ExtensionManifestDigest,
    ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackageRevision, ExtensionTreeDigest,
    EXTENSION_SHA256_BYTES,
};
pub use install::{
    ExtensionInstall, ExtensionInstallCatalog, ExtensionInstallCatalogApplication,
    ExtensionInstallCatalogApplyError, ExtensionInstallCatalogError,
    ExtensionInstallCatalogMutation, ExtensionInstallCatalogRevision, ExtensionInstallRevision,
    MAX_EXTENSION_INSTALLS_PER_PROFILE, MAX_EXTENSION_INSTALL_CATALOG_RETAINED_BYTES,
};
