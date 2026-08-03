use thiserror::Error;

/// Failure at the private filesystem trust boundary.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PrivateFsError {
    /// The exact descriptor-relative child was absent.
    #[error("private filesystem node was not found")]
    NotFound,
    /// A caller attempted to use one of the namespace's root lock components.
    #[error("private filesystem component is reserved by the namespace")]
    ReservedComponent,
    /// A configured path or observed node could not establish the boundary.
    #[error("private filesystem path or node is unsafe")]
    Unsafe,
    /// Another process owns the exact namespace lock, or locking failed.
    #[error("private filesystem namespace lock is unavailable")]
    LockUnavailable,
    /// A caller or observed file exceeded its fixed operation byte bound.
    #[error("private filesystem byte bound was exceeded")]
    BoundExceeded,
    /// A no-replace publication found an existing destination.
    #[error("private filesystem publication destination already exists")]
    AlreadyExists,
    /// Two capabilities do not belong to the same locked namespace lease.
    #[error("private filesystem capabilities belong to different namespaces")]
    NamespaceMismatch,
    /// A directory expected to be empty still contains at least one entry.
    #[error("private filesystem directory is not empty")]
    DirectoryNotEmpty,
    /// A namespace mutation was refused while one or more verified paths are pinned.
    #[error("private filesystem namespace is in use")]
    InUse,
    /// Pre-commit observations disagreed about one node's exact identity.
    ///
    /// Once a namespace lease exists, it is quarantined before this error is
    /// returned. Activation-time failures occur before a lease exists and
    /// instead fail that activation attempt closed.
    #[error("private filesystem identity became ambiguous")]
    IdentityAmbiguous,
    /// A mutation committed but its durable postconditions could not be proven.
    ///
    /// A partial or complete residue may exist. An established namespace is
    /// quarantined and must be closed before recovery can inspect it again;
    /// during activation, the unissued lease is abandoned and a fresh attempt
    /// must recover the reserved state.
    #[error("private filesystem mutation settlement is unknown")]
    SettlementUnknown,
    /// The shared namespace lease was previously quarantined.
    #[error("private filesystem namespace is quarantined")]
    Quarantined,
    /// The platform cannot provide a required durability or identity primitive.
    #[error("private filesystem primitive is unavailable on this platform")]
    PrimitiveUnavailable,
    /// A filesystem operation failed after its inputs passed boundary checks.
    ///
    /// Mutation methods use this only before their named commit point. Once a
    /// commit may have happened they return [`Self::SettlementUnknown`]
    /// instead, so callers never mistake possible residue for a clean failure.
    #[error("private filesystem I/O failed")]
    Io,
}
