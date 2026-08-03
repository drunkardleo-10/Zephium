//! Sealed authority for extension packages bundled with a Zephium release.
//!
//! This crate is the authentication boundary between structurally parsed
//! release metadata and later package materialization. It deliberately owns
//! no filesystem, archive, network, profile, native-webview, or runtime code.
//! Successful admission proves only that one exact canonical catalog matches
//! the product's compiled release anchor and admission policy. It does not
//! grant access to package bytes or permission to activate an extension.
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
mod product;

pub use checkpoint::{BundledCatalogCheckpoint, BundledCatalogDisposition};
pub use digest::BundledCatalogInventoryDigest;
pub use error::BundledCatalogAdmissionError;
pub use product::{
    AdmittedBundledCatalog, BundledPackageAuthority, BundledProductAuthorityStatus,
    MAX_ADMITTED_BUNDLED_CATALOG_RETAINED_BYTES,
};
