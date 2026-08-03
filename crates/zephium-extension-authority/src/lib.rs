//! Sealed authority for extension packages bundled with a Zephium release.
//!
//! This crate is the product authentication boundary above structurally parsed
//! release catalogs and manifests. It deliberately owns no filesystem,
//! archive, network, profile, native-webview, or runtime code. Catalog
//! admission proves that one exact canonical catalog matches the compiled
//! release anchor and policy. Manifest admission additionally binds one exact
//! catalog package, tree index, manifest, backend target, and reviewed
//! compatibility matrix. Neither witness grants access to package bytes or
//! authorizes native activation: later code must also retain an exact sealed
//! materialization receipt and lease.
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

pub use checkpoint::{BundledCatalogCheckpoint, BundledCatalogDisposition};
pub use digest::BundledCatalogInventoryDigest;
pub use error::BundledCatalogAdmissionError;
pub use manifest::{
    ProductAdmittedExtensionManifest, ProductExtensionManifestAdmissionError,
    ProductExtensionManifestAuthority, ProductExtensionManifestAuthorityError,
    ProductExtensionManifestAuthorityStatus, ProductExtensionRuntimeTarget,
    MAX_PRODUCT_ADMITTED_EXTENSION_MANIFEST_RETAINED_BYTES,
    MAX_PRODUCT_EXTENSION_MANIFEST_AUTHORITY_RETAINED_BYTES,
    MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES,
};
pub use product::{
    AdmittedBundledCatalog, BundledPackageAuthority, BundledProductAuthorityStatus,
    MAX_ADMITTED_BUNDLED_CATALOG_RETAINED_BYTES,
};
