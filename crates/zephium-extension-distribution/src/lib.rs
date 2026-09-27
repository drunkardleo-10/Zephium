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
//! The default curated path compiles no endpoint, catalog, package, or legal
//! bytes. Separate opt-in `public-policy` and `beta-admission` modules own
//! fixed shared-policy endpoints and Beta source eligibility; they do not
//! turn curated catalogs into public-install authority.

#![deny(missing_docs)]
#![deny(unsafe_code)]

// Exercise the actual repository implementation against private signed-policy
// fixtures without exporting any test root or witness constructor to consumers.
#[cfg(all(test, feature = "beta-admission"))]
extern crate self as zephium_extension_distribution;

// Shared repository I/O primitives used by the real signed Beta fixture. No
// substitute runtime provider or test authority is exported from either crate.
#[cfg(all(test, feature = "beta-admission"))]
#[path = "../../zephium-extension-repository/src/operation.rs"]
mod operation;
#[cfg(all(test, feature = "beta-admission"))]
#[path = "../../zephium-extension-repository/src/tree_reader.rs"]
mod tree_reader;

mod authentication;
#[cfg(feature = "beta-admission")]
pub mod beta;
#[cfg(feature = "beta-admission")]
pub mod chrome_store;
mod client;
mod coordinator;
mod layout;
#[cfg(feature = "local-extension-lab")]
mod local_lab;
#[cfg(feature = "public-policy")]
pub mod public_policy;
mod session;
#[cfg(feature = "staging-extension-catalog")]
mod staging;

#[cfg(all(feature = "staging-extension-catalog", feature = "local-extension-lab"))]
compile_error!("staging-extension-catalog and local-extension-lab are mutually exclusive");

pub use client::{
    ExtensionDistributionClient, ExtensionDistributionClientError, ExtensionDistributionError,
};
pub use coordinator::{
    ExtensionDistributionCompletion, ExtensionDistributionCoordinator,
    ExtensionDistributionFailure, ExtensionDistributionFailurePhase,
    ExtensionDistributionFailureReason, ExtensionDistributionServicePort,
    MAX_EXTENSION_DISTRIBUTION_COORDINATOR_RETAINED_BYTES,
};
pub use session::{
    ExtensionDistributionSession, MAX_EXTENSION_DISTRIBUTION_SESSION_RETAINED_BYTES,
};
