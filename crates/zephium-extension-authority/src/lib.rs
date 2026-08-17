//! Product-sealed authority for exact extension catalog metadata.
//!
//! This crate is the product authentication boundary above structurally parsed
//! release catalogs and manifests. It deliberately owns no filesystem,
//! archive, network, profile, native-webview, or runtime code. Catalog
//! admission proves that one exact canonical catalog matches a compiled active
//! or explicitly approved rollback generation and its exact policy. Active
//! bundled-tree and acquired-CRX catalogs mint nominally distinct witnesses;
//! neither can cross into the other's materializer. Active and rollback
//! witnesses are also intentionally different capabilities; only an active
//! witness can advance the monotonic repository. Manifest admission
//! additionally binds the exact catalog generation, package, tree index,
//! manifest, backend target, and reviewed compatibility matrix. Neither
//! witness grants access to package bytes or authorizes native activation:
//! later code must also retain an exact sealed materialization receipt and lease.
//!
//! The production authority is intentionally unprovisioned until exact
//! reviewed package and redistribution artifacts are available. There is no
//! runtime trust-provider hook or configurable bypass.

#![deny(unsafe_code)]
#![deny(missing_docs)]

// Optimized fixture authority exists only for the non-shipping product
// measurement binary. Application code independently rejects the base fixture
// cfg, so this exception cannot authorize a Zephium application build.
#[cfg(all(
    zephium_internal_repository_e2e,
    not(debug_assertions),
    not(zephium_extension_product_measurement)
))]
compile_error!("the internal repository E2E authority is forbidden in optimized builds");
#[cfg(all(
    zephium_internal_acquired_repository_e2e,
    not(zephium_internal_repository_e2e)
))]
compile_error!("the acquired repository E2E authority requires the base internal authority");
#[cfg(all(
    zephium_internal_repository_e2e,
    not(any(target_os = "macos", target_os = "linux", target_os = "windows"))
))]
compile_error!("the internal repository E2E authority has no unsupported-host profile");

mod checkpoint;
mod digest;
mod error;
mod inventory;
mod manifest;
mod product;
#[cfg(zephium_internal_repository_e2e)]
mod repository_e2e_fixture;

pub use checkpoint::{
    BundledCatalogCheckpoint, BundledCatalogDisposition, BundledCatalogGenerationAnchor,
};
pub use digest::BundledCatalogInventoryDigest;
pub use error::BundledCatalogAdmissionError;
pub use manifest::{
    ProductAdmittedExtensionManifest, ProductAdmittedRollbackExtensionManifest,
    ProductExtensionManifestAdmissionError, ProductExtensionManifestAuthority,
    ProductExtensionManifestAuthorityError, ProductExtensionManifestAuthorityStatus,
    ProductExtensionRuntimeSelectionError, ProductExtensionRuntimeTarget,
    MAX_PRODUCT_ADMITTED_EXTENSION_MANIFEST_RETAINED_BYTES,
    MAX_PRODUCT_EXTENSION_MANIFEST_AUTHORITY_RETAINED_BYTES,
    MAX_PRODUCT_EXTENSION_MANIFEST_GENERATION_RETAINED_BYTES,
    MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES,
    MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_ACROSS_GENERATIONS,
    MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION,
};
pub use product::{
    AdmittedAcquiredCatalog, AdmittedActiveCatalog, AdmittedBundledCatalog,
    AdmittedRollbackBundledCatalog, BundledPackageAuthority, BundledProductAuthorityStatus,
    ProductBundledCatalogGenerationRole, MAX_ADMITTED_ACQUIRED_CATALOG_RETAINED_BYTES,
    MAX_ADMITTED_BUNDLED_CATALOG_RETAINED_BYTES,
    MAX_ADMITTED_ROLLBACK_BUNDLED_CATALOG_RETAINED_BYTES,
    MAX_BUNDLED_PACKAGE_AUTHORITY_RETAINED_BYTES, MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS,
    MAX_PRODUCT_ROLLBACK_BUNDLED_CATALOGS,
};
