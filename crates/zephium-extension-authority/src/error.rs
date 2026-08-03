//! Stable bundled-catalog admission failures.

use thiserror::Error;
use zephium_extension_package::ExtensionReleaseCatalogError;

/// Stable fail-closed rejection from the bundled package authority.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum BundledCatalogAdmissionError {
    /// No reviewed signed-release package anchor is compiled into this build.
    #[error("bundled extension package authority is unprovisioned")]
    Unprovisioned,
    /// Compiled product anchor and product policy disagree or exceed a bound.
    #[error("bundled extension product authority configuration is invalid")]
    InvalidProductConfiguration,
    /// No explicitly approved rollback generation can match the request.
    #[error("bundled extension rollback catalog is not product-provisioned")]
    RollbackCatalogNotProvisioned,
    /// Exact catalog length differs from the signed-release anchor.
    #[error("bundled extension catalog length does not match the product anchor")]
    CatalogLengthMismatch,
    /// SHA-256 of exact catalog bytes differs from the signed-release anchor.
    #[error("bundled extension catalog digest does not match the product anchor")]
    CatalogDigestMismatch,
    /// Structural canonical catalog parsing or license-policy binding failed.
    #[error("bundled extension catalog is invalid: {0}")]
    Catalog(#[source] ExtensionReleaseCatalogError),
    /// Catalog trust-domain and epoch identity differs from the product anchor.
    #[error("bundled extension catalog authority does not match the product anchor")]
    AuthorityMismatch,
    /// Catalog release revision differs from the exact product release anchor.
    #[error("bundled extension catalog revision does not match the product anchor")]
    RevisionMismatch,
    /// Catalog policy digest differs from the exact compiled product policy.
    #[error("bundled extension catalog policy does not match the product anchor")]
    PolicyMismatch,
    /// A bundled release catalog named an acquired archive payload.
    #[error("bundled extension catalog contains a non-bundled payload")]
    UnsupportedPayload,
    /// Deterministic closed package inventory differs from the product anchor.
    #[error("bundled extension catalog inventory does not match the product anchor")]
    InventoryMismatch,
    /// Logical retained-memory accounting overflowed.
    #[error("bundled extension catalog memory accounting overflowed")]
    AccountingOverflow,
    /// Logical retained-memory charge exceeds the authority ceiling.
    #[error("bundled extension catalog retained-memory limit was exceeded")]
    RetainedBytesExceeded,
}
