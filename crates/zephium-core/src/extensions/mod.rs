//! Pure, bounded extension package, manifest-policy, installation, and grant
//! contracts.
//!
//! This module deliberately contains no manifest/archive parser, filesystem
//! path, package bytes, native adapter, or UI state. Every digest and package
//! identity is only structural: possession is not proof that package bytes
//! were authenticated or remain available. A package-authority adapter must
//! establish that proof and bind backend-native identifiers before activation.

mod action;
mod browser_surface;
mod cohort;
mod compatibility_broker;
mod grants;
mod identity;
mod install;
mod manifest;
mod native_grants;
mod native_namespace;
mod native_ownership;
mod package_pin;
mod runtime;
mod transient;

pub use action::{
    ExtensionActionError, ExtensionActionIcon, ExtensionActionRejection, ExtensionActionRequest,
    ExtensionActionRequestId, ExtensionActionRevision, ExtensionActionScope,
    ExtensionActionSettlement, ExtensionActionSnapshot, ExtensionActionSnapshotSettlement,
    ExtensionActionState, ExtensionPopupAnchor, EXTENSION_ACTION_ICON_HEIGHT,
    EXTENSION_ACTION_ICON_RGBA_BYTES, EXTENSION_ACTION_ICON_WIDTH,
    MAX_EXTENSION_ACTION_BADGE_BYTES, MAX_EXTENSION_ACTION_LABEL_BYTES, MAX_EXTENSION_POPUP_HEIGHT,
    MAX_EXTENSION_POPUP_WIDTH, MIN_EXTENSION_POPUP_HEIGHT, MIN_EXTENSION_POPUP_WIDTH,
};
pub use browser_surface::{
    ExtensionBrowserRequest, ExtensionBrowserRequestAction, ExtensionBrowserRequestError,
    ExtensionBrowserRequestId, ExtensionBrowserRequestRejection, ExtensionBrowserRequestResult,
    ExtensionBrowserRequestSettlement, ExtensionBrowserSurface, ExtensionBrowserSurfaceError,
    ExtensionBrowserSurfaceGeneration, ExtensionBrowserTab, ExtensionBrowserWindow,
    MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES, MAX_EXTENSION_BROWSER_TABS,
    MAX_EXTENSION_BROWSER_WINDOWS, MAX_PENDING_EXTENSION_BROWSER_REQUESTS,
    MAX_PENDING_EXTENSION_BROWSER_REQUESTS_PER_PROFILE,
};
pub use cohort::{
    ExtensionGrantCohort, ExtensionGrantCohortError, ExtensionGrantInitializationState,
    ExtensionGrantManifestBinding, ExtensionGrantManifestBindings,
    MAX_EXTENSION_GRANT_COHORT_RETAINED_BYTES,
    MAX_EXTENSION_GRANT_MANIFEST_BINDINGS_RETAINED_BYTES,
};
pub use compatibility_broker::{
    ExtensionCompatibilityBrokerOperation, ExtensionCompatibilityBrokerPurpose,
    ExtensionCompatibilityBrokerRejection, ExtensionCompatibilityBrokerRequest,
    ExtensionCompatibilityBrokerRequestError, ExtensionCompatibilityBrokerRequestId,
    ExtensionCompatibilityBrokerResult, ExtensionCompatibilityBrokerSettlement,
    ExtensionCompatibilityBrokerWitness, ExtensionCompatibilityHistoryEntry,
    ExtensionCompatibilitySearchDisposition, EXTENSION_COMPATIBILITY_BROKER_APPLICATION_ID,
    MAX_EXTENSION_COMPATIBILITY_BROKER_REQUEST_BYTES,
    MAX_EXTENSION_COMPATIBILITY_BROKER_RESPONSE_BYTES, MAX_EXTENSION_COMPATIBILITY_HISTORY_RESULTS,
    MAX_EXTENSION_COMPATIBILITY_SEARCH_QUERY_BYTES,
    MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS,
    MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS_PER_PROFILE,
};
pub use grants::{
    ExtensionApiGrantDecision, ExtensionGrantApplication, ExtensionGrantApplyError,
    ExtensionGrantAuthority, ExtensionGrantAuthorityError, ExtensionGrantBrowsingContext,
    ExtensionGrantDenial, ExtensionGrantDigest, ExtensionGrantMutation, ExtensionGrantPatch,
    ExtensionGrantPatchError, ExtensionGrantPersistenceProjection, ExtensionGrantRevision,
    ExtensionUrlScopeDecision, MAX_EXTENSION_GRANT_PATCH_CHANGES,
    MAX_EXTENSION_GRANT_PATCH_RETAINED_BYTES, MAX_EXTENSION_GRANT_RETAINED_BYTES,
    MAX_EXTENSION_HOST_GRANTS,
};
pub use identity::{
    ExtensionArchiveDigest, ExtensionArchiveLength, ExtensionAuthorityId, ExtensionManifestDigest,
    ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackagePayloadIdentity,
    ExtensionPackageRevision, ExtensionTreeDigest, EXTENSION_SHA256_BYTES,
    MAX_EXTENSION_ARCHIVE_BYTES,
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
    ExtensionCommandsDeclaration, ExtensionCompatibilityClassification,
    ExtensionCompatibilityLevel, ExtensionCompatibilityProfileDigest,
    ExtensionCompatibilityTargetId, ExtensionContentScriptDeclaration,
    ExtensionContentScriptDescriptorDigest, ExtensionContentScriptGlobDeclaration,
    ExtensionContentScriptResourceDigest, ExtensionContentScriptRunAt, ExtensionContentScriptWorld,
    ExtensionContentSecurityPolicyDeclaration, ExtensionHostPermissionSet,
    ExtensionManifestAdditionalDeclarations, ExtensionManifestDeclaration,
    ExtensionManifestDeclarations, ExtensionManifestDescriptor, ExtensionManifestError,
    ExtensionManifestExecutionSurfaces, ExtensionManifestResourceDigest, ExtensionManifestVersion,
    ExtensionMinimumChromiumVersion, ExtensionOverrideTarget, ExtensionPackageLineIdentity,
    ExtensionSandboxDeclaration, ExtensionUnmodeledDeclarationName,
    ExtensionWebAccessibleResourceDeclaration, MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET,
    MAX_EXTENSION_API_PERMISSIONS, MAX_EXTENSION_API_PERMISSION_NAME_BYTES, MAX_EXTENSION_COMMANDS,
    MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS, MAX_EXTENSION_CONTENT_SCRIPT_FILES,
    MAX_EXTENSION_CONTENT_SCRIPT_GLOBS, MAX_EXTENSION_CONTENT_SCRIPT_PATTERNS,
    MAX_EXTENSION_HOST_PERMISSION_CANONICAL_BYTES, MAX_EXTENSION_HOST_PERMISSION_PATTERNS,
    MAX_EXTENSION_MANIFEST_DECLARATIONS, MAX_EXTENSION_MANIFEST_RETAINED_BYTES,
    MAX_EXTENSION_OVERRIDES, MAX_EXTENSION_SANDBOX_RESOURCES, MAX_EXTENSION_UNMODELED_DECLARATIONS,
    MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS, MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES,
};
pub use native_grants::{
    ExtensionNativeApiGrant, ExtensionNativeApiGrantIter, ExtensionNativeGrantDecision,
    ExtensionNativeGrantProjection, ExtensionNativeGrantRequirement, ExtensionNativeGrantSnapshot,
    ExtensionNativeHostGrant, ExtensionNativeHostGrantIter,
};
pub use native_namespace::{
    ExtensionNativeNamespaceScope, MAX_EXTENSION_NATIVE_NAMESPACE_OBLIGATIONS,
};
pub use native_ownership::{
    ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
    ExtensionExpectedNativeOwnershipIdentity, ExtensionNativeIncarnation,
    ExtensionNativeOwnershipApplyError, ExtensionNativeOwnershipEntry,
    ExtensionNativeOwnershipEntryCas, ExtensionNativeOwnershipEntryRevision,
    ExtensionNativeOwnershipGrantRebindCount, ExtensionNativeOwnershipIdentity,
    ExtensionNativeOwnershipIdentityError, ExtensionNativeOwnershipIntent,
    ExtensionNativeOwnershipJournal, ExtensionNativeOwnershipJournalApplication,
    ExtensionNativeOwnershipJournalError, ExtensionNativeOwnershipJournalMutation,
    ExtensionNativeOwnershipJournalRevision, ExtensionNativeOwnershipKey,
    ExtensionNativeOwnershipMutationKind, ExtensionNativeOwnershipOperation,
    ExtensionNativeOwnershipPhase, ExtensionNativeOwnershipPreparation,
    ExtensionRuntimeBackendTarget, EXTENSION_NATIVE_OWNERSHIP_ID_BYTES,
    MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES,
    MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_RETAINED_BYTES,
    MAX_EXTENSION_NATIVE_OWNERSHIP_MUTATION_RETAINED_BYTES,
};
pub use package_pin::{
    ExtensionPackagePinAcquisitionBinding, ExtensionPackagePinAcquisitionDenial,
    ExtensionPackagePinHeldBinding, ExtensionPackagePinRecombineRefusal,
    ExtensionPackagePinReleaseBinding, ExtensionPackagePinReleaseDenial,
    ExtensionPackagePinRuntimeParts,
};
pub use runtime::{
    ExtensionActiveTabGrantWitness, ExtensionCommittedRuntimeEligibilityError,
    ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial,
    ExtensionRuntimeEligibility, ExtensionRuntimeEligibilityDenial,
    ExtensionRuntimeGrantRebindDenial, ExtensionRuntimeGrantRebindRefusal,
    ExtensionRuntimeOperationAuthority,
};
pub use transient::{
    ExtensionDocumentPurpose, ExtensionRuntimeFingerprint, ExtensionRuntimeGeneration,
    ExtensionRuntimeInstance, ExtensionUserInvocationKind,
};
