//! Dormant product scheduling for curated extension distribution.
//!
//! Construction is explicit and requires a product-authenticated distribution
//! client plus the Shell's non-owning callback ingress. Ordinary Zephium
//! builds do not depend on this crate, so they allocate no worker, runtime,
//! timer, transport, or status state. The worker contains no endpoint, trust
//! material, catalog selection, or automatic refresh policy.

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod port;
mod worker;

pub use port::ShellExtensionDistributionPort;
pub use worker::{
    ExtensionDistributionHandle, ExtensionDistributionRefreshAdmission,
    ExtensionDistributionShutdownOutcome, ExtensionDistributionWorker,
    ExtensionDistributionWorkerLaunchError,
};
