//! Closed, path-free settlements for worker-private runtime coordination.

use zephium_core::extensions::{ExtensionRuntimeEligibilityDenial, ExtensionRuntimeGeneration};
use zephium_extension_runtime_api::{ExtensionRuntimeFailure, ExtensionRuntimeHostBindError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeActivationUnavailableReason {
    DeadlineReached,
    StoreNotAdmitted,
    StoreObservationPending,
    RepositoryUnavailable,
    NativeRecoveryInProgress,
    HostUnavailable(ExtensionRuntimeHostBindError),
    NativeRetryable(ExtensionRuntimeFailure),
    NativeOwnershipUncertain(ExtensionRuntimeFailure),
    PublicationPending(ExtensionRuntimeHostBindError),
    RetirementInProgress,
}

impl RuntimeActivationUnavailableReason {
    pub(crate) const fn pending_reason(
        self,
    ) -> zephium_core::ports::extensions::ExtensionActivationPendingReason {
        use zephium_core::ports::extensions::ExtensionActivationPendingReason as Reason;
        if matches!(
            self,
            Self::NativeRetryable(ExtensionRuntimeFailure::RestartRequired)
        ) {
            Reason::RestartRequired
        } else {
            Reason::Unavailable
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeActivationRejectionReason {
    ProfileUnavailable,
    InstallUnavailable(ExtensionRuntimeEligibilityDenial),
    StoreCohortChanged,
    PackageUnavailable,
    NativeRejected(ExtensionRuntimeFailure),
    HostUnsupported,
    RetainedBytesExceeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeCoordinatorFailureReason {
    GenerationExhausted,
    JournalInvalid,
    JournalDiverged,
    RepositoryInvariant,
    HostInvariant,
    NativeCallPanicked,
    RetainedBytesOverflow,
    InternalProtocolViolation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) enum RuntimeActivationOutcome {
    Activated(ExtensionRuntimeGeneration),
    AlreadyActive(ExtensionRuntimeGeneration),
    Unavailable(RuntimeActivationUnavailableReason),
    Rejected(RuntimeActivationRejectionReason),
    CapacityExceeded,
    ProfileFenced,
    FailedClosed(RuntimeCoordinatorFailureReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeRetirementUnavailableReason {
    DeadlineReached,
    StoreNotAdmitted,
    StoreObservationPending,
    RepositoryUnavailable,
    NativeRetained(ExtensionRuntimeFailure),
    NativeOwnershipUncertain(ExtensionRuntimeFailure),
    PublicationReclaimPending,
    ActivationReconciliationPending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) enum RuntimeRetirementOutcome {
    Retired,
    NotPresent,
    Unavailable(RuntimeRetirementUnavailableReason),
    FailedClosed(RuntimeCoordinatorFailureReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) enum RuntimeDrainOutcome {
    Drained,
    Unavailable(RuntimeRetirementUnavailableReason),
    FailedClosed(RuntimeCoordinatorFailureReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeGrantRebindUnavailableReason {
    DeadlineReached,
    StoreNotAdmitted,
    StoreObservationPending,
}

/// Settlement of one durable-journal plus live-host grant authority rebind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) enum RuntimeGrantRebindOutcome {
    /// The exact generation now holds the supplied later grant authority.
    Rebound(zephium_core::extensions::ExtensionGrantRevision),
    /// No earlier rebind settlement was pending for this owner.
    NoPending,
    /// The addressed runtime or generation is no longer the live owner.
    Conflict,
    /// Settlement remains safely retryable with authority retained in-slot.
    Unavailable(RuntimeGrantRebindUnavailableReason),
    /// An invariant failed after authority may have crossed a durable seam.
    FailedClosed(RuntimeCoordinatorFailureReason),
}
