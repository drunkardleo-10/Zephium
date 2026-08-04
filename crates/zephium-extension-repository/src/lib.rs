//! Crash-durable authority and materialization metadata for extensions.
//!
//! This crate owns the private repository namespace, monotonic catalog and
//! package-line high-water marks, exact bounded package objects, materialization
//! records, and live opaque sealed-tree identities. Materialization deliberately
//! grants no profile authority, exposes no filesystem path, issues no activation
//! receipt, and mutates no native extension runtime. Recording an authenticated
//! catalog proves only that an equal or older catalog can no longer be accepted
//! in this authority epoch.

#![deny(missing_docs)]
#![deny(unsafe_code)]

#[cfg(all(zephium_internal_repository_e2e, not(debug_assertions)))]
compile_error!("the internal repository E2E authority is forbidden in optimized builds");

mod admission;
mod codec;
mod error;
mod materialization;
mod names;
mod recovery;
mod state;
mod storage;
mod writer;

pub use admission::{BundledCatalogRecordOutcome, ExtensionRepository};
pub use error::ExtensionRepositoryError;
pub use materialization::{
    BundledReleaseByteSource, BundledReleaseCatalogSourceIdentity,
    BundledReleasePackageSourceIdentity, BundledReleaseResource, BundledReleaseResourceKind,
    BundledReleaseSourceError,
};
pub use writer::{BundledPackageMaterializationError, BundledPackageMaterializationOutcome};
