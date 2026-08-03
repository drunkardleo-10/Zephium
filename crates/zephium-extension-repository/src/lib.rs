//! Crash-durable authority state for authenticated extension release catalogs.
//!
//! This crate owns the private repository namespace and monotonic catalog and
//! package-line high-water marks. It deliberately does not materialize package
//! trees, grant profile authority, expose filesystem paths, or mutate a native
//! extension runtime. Recording an authenticated catalog proves only that an
//! equal or older catalog can no longer be accepted in this authority epoch.

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod admission;
mod codec;
mod error;
mod names;
mod recovery;
mod state;
mod storage;

pub use admission::{BundledCatalogRecordOutcome, ExtensionRepository};
pub use error::ExtensionRepositoryError;
