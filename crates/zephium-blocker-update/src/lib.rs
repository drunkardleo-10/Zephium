//! Authenticated, rollback-resistant filter-package acquisition.
//!
//! This crate deliberately has no browser, profile, page, or UI dependency.
//! It accepts one application-shipped TUF root and two fixed HTTPS repository
//! origins, verifies a bounded package, and publishes only immutable
//! [`zephium_blocker::StaticPolicyCatalog`] values.

#![deny(unsafe_code)]
#![deny(missing_docs)]

mod cache_lock;
mod file_identity;
mod manifest;
mod repository;
mod storage;
mod transport;
mod types;
mod worker;

#[cfg(test)]
mod tests;

pub use manifest::{
    CatalogManifest, LicenseMetadata, ManifestError, ManifestSource, ManifestSourceFormat,
    CATALOG_MANIFEST_TARGET, CATALOG_MANIFEST_VERSION,
};
pub use storage::{GarbageCollectionReport, StoreError};
pub use types::{
    ActivatedCatalog, CandidateCommitDispatch, CandidateCommitOutcome, CandidateRejectDispatch,
    CandidateRejectOutcome, CandidateRejectionReason, CandidateRepairDispatch,
    CandidateRepairOutcome, CatalogAvailability, CatalogIdentity, ConfigError, FailureKind,
    GcBudget, LicensePolicy, RefreshAdmission, RepositoryConfig, RepositoryId, ShutdownOutcome,
    StatusSnapshot, UnavailableReason, UpdateLimits, UpdateStatus, WaitForStatus,
};
pub use worker::CatalogUpdateWorker;
