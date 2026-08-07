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
