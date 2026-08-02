//! Pure, bounded extension package, manifest-policy, installation, and grant
//! contracts.
//!
//! This module deliberately contains no manifest/archive parser, filesystem
//! path, package bytes, native adapter, or UI state. Every digest and package
//! identity is only structural: possession is not proof that package bytes
//! were authenticated or remain available. A package-authority adapter must
//! establish that proof and bind backend-native identifiers before activation.

mod cohort;
mod grants;
mod identity;
mod install;
mod manifest;

pub use cohort::{
    ExtensionGrantCohort, ExtensionGrantCohortError, ExtensionGrantInitializationState,
    ExtensionGrantManifestBinding, ExtensionGrantManifestBindings,
    MAX_EXTENSION_GRANT_COHORT_RETAINED_BYTES,
    MAX_EXTENSION_GRANT_MANIFEST_BINDINGS_RETAINED_BYTES,
};
pub use grants::{
    ExtensionApiGrantDecision, ExtensionGrantApplication, ExtensionGrantApplyError,
    ExtensionGrantAuthority, ExtensionGrantAuthorityError, ExtensionGrantBrowsingContext,
    ExtensionGrantDenial, ExtensionGrantDigest, ExtensionGrantMutation,
    ExtensionGrantPersistenceProjection, ExtensionGrantRevision, ExtensionUrlScopeDecision,
    MAX_EXTENSION_GRANT_RETAINED_BYTES, MAX_EXTENSION_HOST_GRANTS,
};
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
pub use manifest::{
    ApiPermissionName, ApiPermissionNameError, ExtensionActionDeclaration,
    ExtensionApiPermissionSet, ExtensionBackgroundDeclaration, ExtensionBackgroundWorkerType,
    ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
    ExtensionCompatibilityProfileDigest, ExtensionCompatibilityTargetId,
    ExtensionContentScriptDeclaration, ExtensionContentScriptDescriptorDigest,
    ExtensionContentScriptGlobDeclaration, ExtensionContentScriptResourceDigest,
    ExtensionContentScriptRunAt, ExtensionContentScriptWorld,
    ExtensionContentSecurityPolicyDeclaration, ExtensionHostPermissionSet,
    ExtensionManifestDeclaration, ExtensionManifestDeclarations, ExtensionManifestDescriptor,
    ExtensionManifestError, ExtensionManifestExecutionSurfaces, ExtensionManifestResourceDigest,
    ExtensionManifestVersion, ExtensionOverrideTarget, ExtensionPackageLineIdentity,
    ExtensionSandboxDeclaration, ExtensionUnmodeledDeclarationName,
    ExtensionWebAccessibleResourceDeclaration, MAX_EXTENSION_API_PERMISSIONS,
    MAX_EXTENSION_API_PERMISSION_NAME_BYTES, MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS,
    MAX_EXTENSION_CONTENT_SCRIPT_FILES, MAX_EXTENSION_CONTENT_SCRIPT_GLOBS,
    MAX_EXTENSION_CONTENT_SCRIPT_PATTERNS, MAX_EXTENSION_HOST_PERMISSION_CANONICAL_BYTES,
    MAX_EXTENSION_HOST_PERMISSION_PATTERNS, MAX_EXTENSION_MANIFEST_DECLARATIONS,
    MAX_EXTENSION_MANIFEST_RETAINED_BYTES, MAX_EXTENSION_OVERRIDES,
    MAX_EXTENSION_SANDBOX_RESOURCES, MAX_EXTENSION_UNMODELED_DECLARATIONS,
    MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS, MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES,
};
