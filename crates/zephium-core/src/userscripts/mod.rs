//! Pure, bounded userscript source and catalog contracts.
//!
//! This module deliberately stops before native activation, permissions, or a
//! GM API bridge. Source text is the durable authority; parsed metadata and a
//! compatibility assessment are derived locally under fixed allocation
//! budgets.

mod catalog;
mod compatibility;
mod metadata;
mod runtime;

pub use catalog::{
    Userscript, UserscriptCatalog, UserscriptCatalogError, UserscriptCatalogMutation,
    UserscriptCatalogRevision, UserscriptRevision, CURRENT_USERSCRIPT_METADATA_FORMAT,
    MAX_USERSCRIPTS_PER_PROFILE, MAX_USERSCRIPT_CATALOG_SOURCE_BYTES,
};
pub use compatibility::{
    assess_userscript_compatibility, assess_userscript_runtime_eligibility,
    UserscriptCompatibility, UserscriptCompatibilityIssue, UserscriptRuntimeEligibility,
};
pub use metadata::{
    parse_userscript_metadata, DeclaredGrant, DeclaredRunAt, ParsedUserscriptMetadata,
    UnsupportedDirective, UserscriptMetadataError, UserscriptSourceDigest,
    MAX_USERSCRIPT_DESCRIPTION_BYTES, MAX_USERSCRIPT_DIRECTIVES, MAX_USERSCRIPT_GRANTS,
    MAX_USERSCRIPT_METADATA_BYTES, MAX_USERSCRIPT_METADATA_LINES,
    MAX_USERSCRIPT_METADATA_LINE_BYTES, MAX_USERSCRIPT_NAMESPACE_BYTES, MAX_USERSCRIPT_NAME_BYTES,
    MAX_USERSCRIPT_VERSION_BYTES,
};
pub use runtime::{
    prepare_userscript_runtime, PreparedUserscriptAccounting, PreparedUserscriptEnvironment,
    PreparedUserscriptFrameScope, PreparedUserscriptRunAt, PreparedUserscriptRuntime,
    UserscriptRuntimePreparationError, MAX_PREPARED_USERSCRIPT_RETAINED_BYTES,
};
