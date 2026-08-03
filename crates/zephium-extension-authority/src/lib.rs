//! Sealed authority for extension packages bundled with a Zephium release.
//!
//! This crate is the product authentication boundary above structurally parsed
//! release catalogs and manifests. It deliberately owns no filesystem,
//! archive, network, profile, native-webview, or runtime code. Catalog
//! admission proves that one exact canonical catalog matches a compiled active
//! or explicitly approved rollback generation and its exact policy. The active
//! and rollback witnesses are intentionally different capabilities; only the
//! active witness can advance the monotonic repository. Manifest admission
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

mod checkpoint;
mod digest;
mod error;
mod inventory;
mod manifest;
mod product;

pub use checkpoint::{
    BundledCatalogCheckpoint, BundledCatalogDisposition, BundledCatalogGenerationAnchor,
};
pub use digest::BundledCatalogInventoryDigest;
pub use error::BundledCatalogAdmissionError;
pub use manifest::{
    ProductAdmittedExtensionManifest, ProductAdmittedRollbackExtensionManifest,
    ProductExtensionManifestAdmissionError, ProductExtensionManifestAuthority,
    ProductExtensionManifestAuthorityError, ProductExtensionManifestAuthorityStatus,
    ProductExtensionRuntimeTarget, MAX_PRODUCT_ADMITTED_EXTENSION_MANIFEST_RETAINED_BYTES,
    MAX_PRODUCT_EXTENSION_MANIFEST_AUTHORITY_RETAINED_BYTES,
    MAX_PRODUCT_EXTENSION_MANIFEST_GENERATION_RETAINED_BYTES,
    MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES,
    MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_ACROSS_GENERATIONS,
    MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION,
};
pub use product::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, BundledPackageAuthority,
    BundledProductAuthorityStatus, ProductBundledCatalogGenerationRole,
    MAX_ADMITTED_BUNDLED_CATALOG_RETAINED_BYTES,
    MAX_ADMITTED_ROLLBACK_BUNDLED_CATALOG_RETAINED_BYTES,
    MAX_BUNDLED_PACKAGE_AUTHORITY_RETAINED_BYTES, MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS,
    MAX_PRODUCT_ROLLBACK_BUNDLED_CATALOGS,
};
