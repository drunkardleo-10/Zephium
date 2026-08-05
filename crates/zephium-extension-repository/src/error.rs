//! Stable repository failure categories.

use thiserror::Error;
use zephium_private_fs::PrivateFsError;

/// Failure while opening or advancing the authenticated extension repository.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRepositoryError {
    /// Exact caller-supplied catalog bytes do not match the admitted witness.
    #[error("extension catalog bytes do not match their admitted witness")]
    CatalogBytesMismatch,
    /// The durable repository belongs to another authority epoch.
    #[error("extension catalog authority does not match the durable repository")]
    AuthorityMismatch,
    /// The catalog revision is below the durable authority high-water mark.
    #[error("extension catalog revision is below the durable high-water mark")]
    CatalogRollback,
    /// An explicit rollback catalog is newer than the durable authority floor.
    #[error("extension rollback catalog revision is above the durable high-water mark")]
    RollbackCatalogAboveHighWater,
    /// The catalog revision is unchanged but its authenticated identity differs.
    #[error("extension catalog revision has an equivocal identity")]
    CatalogEquivocation,
    /// A package update line moved below its durable revision high-water mark.
    #[error("extension package revision is below its durable high-water mark")]
    PackageRollback,
    /// A package update-line revision is unchanged but its complete row differs.
    #[error("extension package revision has an equivocal release row")]
    PackageEquivocation,
    /// The bounded historical package-line floor cannot admit another key.
    #[error("extension package-line high-water capacity is exhausted")]
    PackageLineLimit,
    /// The bounded content-addressed catalog namespace is full.
    #[error("extension catalog-object capacity is exhausted")]
    CatalogObjectLimit,
    /// The repository generation cannot advance without leaving its durable range.
    #[error("extension repository generation is exhausted")]
    GenerationExhausted,
    /// A package build must settle or be durably aborted before catalog advance.
    #[error("catalog advance is blocked by an extension package build")]
    CatalogAdvanceBlockedByBuild,
    /// A bounded garbage-collection intent must settle before repository mutation.
    #[error("extension repository garbage collection is in progress")]
    GarbageCollectionInProgress,
    /// A live selected or owner-pinned package is incompatible with catalog advance.
    #[error("catalog advance is blocked by a live extension generation")]
    CatalogAdvanceBlockedByLiveGeneration,
    /// Canonical durable state or a referenced catalog object is corrupt.
    #[error("extension repository durable state is corrupt")]
    StateCorrupt,
    /// The journal/checkpoint chain has no single provable recovery result.
    #[error("extension repository recovery is ambiguous")]
    RecoveryAmbiguous,
    /// A mutation may have committed and this process may perform no more work.
    #[error("extension repository mutation settlement is ambiguous")]
    SettlementAmbiguous,
    /// Trusted adapter code attempted to enter a repository from a callback.
    #[error("extension repository operations are forbidden from adapter callbacks")]
    CallbackReentry,
    /// This repository instance was sealed after an ambiguous mutation.
    #[error("extension repository instance is sealed")]
    Sealed,
    /// The private filesystem rejected or could not complete an operation.
    #[error("extension repository filesystem operation failed: {0}")]
    FileSystem(#[from] PrivateFsError),
    /// Test-only simulated process loss after a named durable transition.
    #[cfg(test)]
    #[error("simulated extension repository process loss")]
    InjectedCrash,
}
