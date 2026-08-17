//! Product-authenticated acquisition of curated extension release objects.
//!
//! This crate is the narrow network-to-service bridge for acquired CRX3
//! packages. It fetches one exact product-sealed catalog from a fixed HTTPS
//! metadata directory, derives package and legal-object paths only from
//! authenticated identities, and retains at most one bounded package request
//! at a time. It neither exposes a generic fetch API nor grants installation,
//! profile, repository, or native-runtime authority.
//!
//! Every result remains path-free and is reauthenticated by
//! `zephium-extension-service` immediately before durable materialization.
//! No endpoint, catalog, package, or legal byte is compiled into this crate.

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod authentication;
mod client;
mod layout;
mod session;

pub use client::{
    ExtensionDistributionClient, ExtensionDistributionClientError, ExtensionDistributionError,
};
pub use session::{
    ExtensionDistributionSession, MAX_EXTENSION_DISTRIBUTION_SESSION_RETAINED_BYTES,
};
