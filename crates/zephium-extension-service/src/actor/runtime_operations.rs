//! Path-free settlements for owner-directed runtime work.

use zephium_core::extensions::{ExtensionRuntimeEligibilityDenial, ExtensionRuntimeGeneration};
use zephium_extension_runtime_api::{ExtensionRuntimeFailure, ExtensionRuntimeHostBindError};

use crate::runtime_coordinator::{
    RuntimeActivationOutcome, RuntimeActivationRejectionReason, RuntimeActivationUnavailableReason,
    RuntimeCoordinatorFailureReason, RuntimeRetirementOutcome, RuntimeRetirementUnavailableReason,
};
use crate::startup::ExtensionServiceStartupFailureReason;

/// Retryable reason one runtime activation attempt did not settle active.
///
/// Every variant is non-authorizing. A caller may retry the same ownership key
/// with a fresh bounded deadline while the unique service owner remains live.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionServiceRuntimeActivationUnavailableReason {
    /// The operation or observation deadline elapsed.
    DeadlineReached,
    /// The bounded ordinary mailbox had no free admission slot.
    RetryableNotAdmitted,
    /// Startup has not produced current readiness for this exact worker.
    ServiceNotReady,
    /// Service shutdown was requested before this command began native work.
    CancellationRequested,
    /// Store definitely did not admit the required operation.
    StoreNotAdmitted,
    /// Store settlement needs a later exact observation.
    StoreObservationPending,
    /// The private package repository is temporarily unavailable.
    RepositoryUnavailable,
    /// Persisted-owner recovery currently owns the native-host factory.
    NativeRecoveryInProgress,
    /// The native host could not currently bind the requested runtime.
    HostUnavailable(ExtensionRuntimeHostBindError),
    /// Native activation failed in a definitely retryable state.
    NativeRetryable(ExtensionRuntimeFailure),
    /// Native ownership requires exact reconciliation before retry.
    NativeOwnershipUncertain(ExtensionRuntimeFailure),
    /// Runtime publication has not settled yet.
    PublicationPending(ExtensionRuntimeHostBindError),
    /// Retirement for this exact runtime is already in progress.
    RetirementInProgress,
}

/// Closed policy reason a requested runtime activation was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionServiceRuntimeActivationRejectionReason {
    /// The profile is absent or otherwise ineligible in Store's exact snapshot.
    ProfileUnavailable,
    /// The installed extension is not eligible for a background runtime.
    InstallUnavailable(ExtensionRuntimeEligibilityDenial),
    /// The install/grant cohort changed before authority publication.
    StoreCohortChanged,
    /// The authenticated package generation is unavailable.
    PackageUnavailable,
    /// The native adapter rejected the exact activation request.
    NativeRejected(ExtensionRuntimeFailure),
    /// The selected platform host does not support this runtime.
    HostUnsupported,
    /// The runtime would exceed its pre-admitted retained-memory ceiling.
    RetainedBytesExceeded,
}

/// Sticky reason runtime coordination failed closed.
///
/// These reasons grant no cleanup or absence authority. Coordinator failures
/// make later runtime work fail closed for this worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionServiceRuntimeFailureReason {
    /// Startup itself failed closed before runtime ingress was legal.
    StartupFailed(ExtensionServiceStartupFailureReason),
    /// The worker mailbox or thread is no longer available.
    WorkerUnavailable,
    /// The process-local runtime generation counter was exhausted.
    GenerationExhausted,
    /// The durable ownership journal violated its bounded representation.
    JournalInvalid,
    /// Store's exact journal settlement diverged from retained evidence.
    JournalDiverged,
    /// Repository authority or settlement invariants were violated.
    RepositoryInvariant,
    /// Native-host identity or ownership invariants were violated.
    HostInvariant,
    /// A native adapter call crossed the service panic fence.
    NativeCallPanicked,
    /// Retained-memory accounting overflowed and could not authorize work.
    RetainedBytesOverflow,
    /// A worker-private ordering, identity, or state invariant was violated.
    InternalProtocolViolation,
}

/// Settlement of one owner-directed runtime activation command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "runtime activation settlement must be checked"]
pub enum ExtensionServiceRuntimeActivationOutcome {
    /// The exact runtime became active at this process-local generation.
    Activated(ExtensionRuntimeGeneration),
    /// The exact runtime was already active at this generation.
    AlreadyActive(ExtensionRuntimeGeneration),
    /// This attempt made no activation claim and may be retried.
    Unavailable(ExtensionServiceRuntimeActivationUnavailableReason),
    /// Store, package, grant, or native policy rejected activation.
    Rejected(ExtensionServiceRuntimeActivationRejectionReason),
    /// The independent background-runtime capacity pool is full.
    CapacityExceeded,
    /// The profile has a permanent worker-local retirement fence.
    ProfileFenced,
    /// Runtime coordination or its worker failed closed.
    FailedClosed(ExtensionServiceRuntimeFailureReason),
}

/// Retryable reason one exact runtime retirement did not settle absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionServiceRuntimeRetirementUnavailableReason {
    /// The operation or observation deadline elapsed.
    DeadlineReached,
    /// The bounded ordinary mailbox had no free admission slot.
    RetryableNotAdmitted,
    /// Startup has not produced current readiness for this exact worker.
    ServiceNotReady,
    /// Service shutdown was requested before this command began native work.
    CancellationRequested,
    /// Store definitely did not admit the required operation.
    StoreNotAdmitted,
    /// Store settlement needs a later exact observation.
    StoreObservationPending,
    /// The private package repository is temporarily unavailable.
    RepositoryUnavailable,
    /// The native runtime retained ownership after a bounded retirement call.
    NativeRetained(ExtensionRuntimeFailure),
    /// Native ownership requires exact reconciliation before retry.
    NativeOwnershipUncertain(ExtensionRuntimeFailure),
    /// Runtime publication reclaim has not settled yet.
    PublicationReclaimPending,
    /// An interrupted activation must be reconciled before retirement resumes.
    ActivationReconciliationPending,
}

/// Settlement of one owner-directed exact runtime retirement command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "runtime retirement settlement must be checked"]
pub enum ExtensionServiceRuntimeRetirementOutcome {
    /// The exact worker-owned runtime and package pin were retired.
    Retired,
    /// No coordinator-owned runtime exists for the exact key.
    NotPresent,
    /// This attempt made no absence claim and may be retried.
    Unavailable(ExtensionServiceRuntimeRetirementUnavailableReason),
    /// Runtime coordination or its worker failed closed.
    FailedClosed(ExtensionServiceRuntimeFailureReason),
}

impl From<RuntimeActivationOutcome> for ExtensionServiceRuntimeActivationOutcome {
    fn from(outcome: RuntimeActivationOutcome) -> Self {
        match outcome {
            RuntimeActivationOutcome::Activated(generation) => Self::Activated(generation),
            RuntimeActivationOutcome::AlreadyActive(generation) => Self::AlreadyActive(generation),
            RuntimeActivationOutcome::Unavailable(reason) => Self::Unavailable(reason.into()),
            RuntimeActivationOutcome::Rejected(reason) => Self::Rejected(reason.into()),
            RuntimeActivationOutcome::CapacityExceeded => Self::CapacityExceeded,
            RuntimeActivationOutcome::ProfileFenced => Self::ProfileFenced,
            RuntimeActivationOutcome::FailedClosed(reason) => Self::FailedClosed(reason.into()),
        }
    }
}

impl From<RuntimeActivationUnavailableReason>
    for ExtensionServiceRuntimeActivationUnavailableReason
{
    fn from(reason: RuntimeActivationUnavailableReason) -> Self {
        match reason {
            RuntimeActivationUnavailableReason::DeadlineReached => Self::DeadlineReached,
            RuntimeActivationUnavailableReason::StoreNotAdmitted => Self::StoreNotAdmitted,
            RuntimeActivationUnavailableReason::StoreObservationPending => {
                Self::StoreObservationPending
            }
            RuntimeActivationUnavailableReason::RepositoryUnavailable => {
                Self::RepositoryUnavailable
            }
            RuntimeActivationUnavailableReason::NativeRecoveryInProgress => {
                Self::NativeRecoveryInProgress
            }
            RuntimeActivationUnavailableReason::HostUnavailable(reason) => {
                Self::HostUnavailable(reason)
            }
            RuntimeActivationUnavailableReason::NativeRetryable(reason) => {
                Self::NativeRetryable(reason)
            }
            RuntimeActivationUnavailableReason::NativeOwnershipUncertain(reason) => {
                Self::NativeOwnershipUncertain(reason)
            }
            RuntimeActivationUnavailableReason::PublicationPending(reason) => {
                Self::PublicationPending(reason)
            }
            RuntimeActivationUnavailableReason::RetirementInProgress => Self::RetirementInProgress,
        }
    }
}

impl From<RuntimeActivationRejectionReason> for ExtensionServiceRuntimeActivationRejectionReason {
    fn from(reason: RuntimeActivationRejectionReason) -> Self {
        match reason {
            RuntimeActivationRejectionReason::ProfileUnavailable => Self::ProfileUnavailable,
            RuntimeActivationRejectionReason::InstallUnavailable(reason) => {
                Self::InstallUnavailable(reason)
            }
            RuntimeActivationRejectionReason::StoreCohortChanged => Self::StoreCohortChanged,
            RuntimeActivationRejectionReason::PackageUnavailable => Self::PackageUnavailable,
            RuntimeActivationRejectionReason::NativeRejected(reason) => {
                Self::NativeRejected(reason)
            }
            RuntimeActivationRejectionReason::HostUnsupported => Self::HostUnsupported,
            RuntimeActivationRejectionReason::RetainedBytesExceeded => Self::RetainedBytesExceeded,
        }
    }
}

impl From<RuntimeCoordinatorFailureReason> for ExtensionServiceRuntimeFailureReason {
    fn from(reason: RuntimeCoordinatorFailureReason) -> Self {
        match reason {
            RuntimeCoordinatorFailureReason::GenerationExhausted => Self::GenerationExhausted,
            RuntimeCoordinatorFailureReason::JournalInvalid => Self::JournalInvalid,
            RuntimeCoordinatorFailureReason::JournalDiverged => Self::JournalDiverged,
            RuntimeCoordinatorFailureReason::RepositoryInvariant => Self::RepositoryInvariant,
            RuntimeCoordinatorFailureReason::HostInvariant => Self::HostInvariant,
            RuntimeCoordinatorFailureReason::NativeCallPanicked => Self::NativeCallPanicked,
            RuntimeCoordinatorFailureReason::RetainedBytesOverflow => Self::RetainedBytesOverflow,
            RuntimeCoordinatorFailureReason::InternalProtocolViolation => {
                Self::InternalProtocolViolation
            }
        }
    }
}

impl From<RuntimeRetirementOutcome> for ExtensionServiceRuntimeRetirementOutcome {
    fn from(outcome: RuntimeRetirementOutcome) -> Self {
        match outcome {
            RuntimeRetirementOutcome::Retired => Self::Retired,
            RuntimeRetirementOutcome::NotPresent => Self::NotPresent,
            RuntimeRetirementOutcome::Unavailable(reason) => Self::Unavailable(reason.into()),
            RuntimeRetirementOutcome::FailedClosed(reason) => Self::FailedClosed(reason.into()),
        }
    }
}

impl From<RuntimeRetirementUnavailableReason>
    for ExtensionServiceRuntimeRetirementUnavailableReason
{
    fn from(reason: RuntimeRetirementUnavailableReason) -> Self {
        match reason {
            RuntimeRetirementUnavailableReason::DeadlineReached => Self::DeadlineReached,
            RuntimeRetirementUnavailableReason::StoreNotAdmitted => Self::StoreNotAdmitted,
            RuntimeRetirementUnavailableReason::StoreObservationPending => {
                Self::StoreObservationPending
            }
            RuntimeRetirementUnavailableReason::RepositoryUnavailable => {
                Self::RepositoryUnavailable
            }
            RuntimeRetirementUnavailableReason::NativeRetained(reason) => {
                Self::NativeRetained(reason)
            }
            RuntimeRetirementUnavailableReason::NativeOwnershipUncertain(reason) => {
                Self::NativeOwnershipUncertain(reason)
            }
            RuntimeRetirementUnavailableReason::PublicationReclaimPending => {
                Self::PublicationReclaimPending
            }
            RuntimeRetirementUnavailableReason::ActivationReconciliationPending => {
                Self::ActivationReconciliationPending
            }
        }
    }
}
