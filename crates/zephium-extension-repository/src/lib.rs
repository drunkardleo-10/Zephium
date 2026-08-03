//! Crash-durable authority and materialization metadata for extensions.
//!
//! This crate owns the private repository namespace, monotonic catalog and
//! package-line high-water marks, bounded materialization records, and live
//! opaque sealed-tree identities. It deliberately does not materialize package
//! bytes yet, grant profile authority, expose filesystem paths, issue receipts,
//! or mutate a native extension runtime. Recording an authenticated catalog
//! proves only that an equal or older catalog can no longer be accepted in this
//! authority epoch.

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod admission;
mod codec;
mod error;
mod materialization;
mod names;
mod recovery;
mod state;
mod storage;

pub use admission::{BundledCatalogRecordOutcome, ExtensionRepository};
pub use error::ExtensionRepositoryError;
