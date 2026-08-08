//! Explicit runtime ownership settlement.

use std::fmt;
use std::time::Instant;

use crate::{
    ExtensionPackageAccess, ExtensionPackageAccessError, ExtensionRuntimeAbsenceEvidence,
    ExtensionRuntimeNativeRootLease, ExtensionRuntimeOwnershipEvidence,
    ExtensionRuntimeRecoveryExpectation, ExtensionRuntimeResource, ExtensionRuntimeResourcePlan,
    ExtensionRuntimeResourceVisitor, ExtensionRuntimeTarget, ExtensionRuntimeVisitorError,
    MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
};

/// A closed, redacted runtime lifecycle failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeFailure {
    /// The runtime target is unsupported by this backend.
    UnsupportedTarget,
    /// The runtime backend is temporarily unavailable.
    BackendUnavailable,
    /// A bounded runtime capacity was exhausted.
    CapacityExceeded,
    /// Native validation rejected the package.
    PackageRejected,
    /// The native operation did not settle before its deadline.
    TimedOut,
    /// The backend failed without a safe, more-specific classification.
    Internal,
}

impl fmt::Display for ExtensionRuntimeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedTarget => "extension runtime target is unsupported",
            Self::BackendUnavailable => "extension runtime backend is unavailable",
            Self::CapacityExceeded => "extension runtime capacity was exceeded",
            Self::PackageRejected => "extension runtime rejected the package",
            Self::TimedOut => "extension runtime operation timed out",
            Self::Internal => "extension runtime operation failed internally",
        })
    }
}

impl std::error::Error for ExtensionRuntimeFailure {}

/// A backend's closed activation outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeActivationDisposition {
    /// The runtime definitely owns the package activation.
    Activated(ExtensionRuntimeOwnershipEvidence),
    /// The runtime definitely does not own an activation and the same request
    /// may be retried.
    Retryable {
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
        /// Exact reservation- and attempt-bound proof of absence.
        absence: ExtensionRuntimeAbsenceEvidence,
    },
    /// The runtime definitely does not own an activation and the request must be
    /// rejected back to the package service.
    Rejected {
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
        /// Exact reservation- and attempt-bound proof of absence.
        absence: ExtensionRuntimeAbsenceEvidence,
    },
    /// Native ownership could not be determined.
    OwnershipUncertain {
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
        /// Newly authenticated structural evidence, when native work reached
        /// the point where the adapter could identify the possible owner.
        evidence: Option<ExtensionRuntimeOwnershipEvidence>,
    },
}

/// A backend's closed retirement outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeRetirementDisposition {
    /// Native ownership is definitely absent.
    Retired(ExtensionRuntimeAbsenceEvidence),
    /// Native ownership definitely remains active.
    Retained(ExtensionRuntimeFailure),
    /// Native ownership could not be determined.
    OwnershipUncertain {
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
        /// Newly authenticated structural evidence, when available.
        evidence: Option<ExtensionRuntimeOwnershipEvidence>,
    },
}

/// A backend's closed ownership-reconciliation outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeOwnershipDisposition {
    /// Native ownership definitely exists.
    Owned(ExtensionRuntimeOwnershipEvidence),
    /// Native ownership is definitely absent.
    Absent(ExtensionRuntimeAbsenceEvidence),
    /// Native ownership remains indeterminate.
    StillUncertain {
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
        /// Newly authenticated structural evidence, when available.
        evidence: Option<ExtensionRuntimeOwnershipEvidence>,
    },
}

/// Temporary, exclusively borrowed package access exposed to a lifecycle port.
///
/// The view is non-owning and cannot be constructed outside this crate. It
/// cannot release or serialize the underlying access capability.
pub struct ExtensionPackageAccessView<'access> {
    access: &'access mut ExtensionPackageAccess,
}

impl ExtensionPackageAccessView<'_> {
    /// Returns the runtime family selected for this package.
    #[must_use]
    pub const fn target(&self) -> ExtensionRuntimeTarget {
        self.access.target()
    }

    /// Returns the manifest descriptor.
    #[must_use]
    pub fn manifest(&self) -> ExtensionRuntimeResource {
        self.access.manifest()
    }

    /// Borrows the complete canonical package-resource plan.
    #[must_use]
    pub const fn resources(&self) -> &ExtensionRuntimeResourcePlan {
        self.access.resources()
    }

    /// Returns bounded retained bytes attributed to package access.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.access.retained_bytes()
    }

    /// Visits the package manifest synchronously.
    pub fn visit_manifest(
        &mut self,
        visitor: &mut dyn ExtensionRuntimeResourceVisitor,
    ) -> Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError> {
        self.access.visit_manifest(visitor)
    }

    /// Visits one package resource synchronously.
    pub fn visit_resource(
        &mut self,
        resource: ExtensionRuntimeResource,
        visitor: &mut dyn ExtensionRuntimeResourceVisitor,
    ) -> Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError> {
        self.access.visit_resource(resource, visitor)
    }

    /// Transfers the preallocated native package-root lease exactly once.
    ///
    /// This succeeds only for [`ExtensionRuntimeTarget::NativeWebExtension`].
    /// The lease's retained state is already charged to [`Self::retained_bytes`]
    /// and its port-owned box contents/shared allocations must not be reported
    /// again by the lifecycle adapter. The adapter still charges its own
    /// predeclared inline lease destination through normal struct accounting.
    pub fn take_native_root_lease(
        &mut self,
    ) -> Result<ExtensionRuntimeNativeRootLease, ExtensionPackageAccessError> {
        self.access.take_native_root_lease()
    }
}

impl fmt::Debug for ExtensionPackageAccessView<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionPackageAccessView")
            .field("access", &"[redacted]")
            .finish()
    }
}

/// A passive ownership-control proxy for one exact native runtime incarnation.
///
/// Every callback is synchronous. Implementations must return a disposition
/// that describes native ownership after the attempted operation, not merely
/// whether an API call returned success. One port instance must remain bound to
/// one installation/runtime generation and native incarnation; it cannot be
/// reused to describe another owner.
///
/// Every definitely-owned or definitely-absent result is also a settlement
/// fence: the backend must prove that no accepted earlier work, pending callback,
/// or late completion can later invert that result. If such work cannot be
/// cancelled, invalidated, or joined, the only valid result is an uncertain
/// disposition. A timeout or postvalidation failure is never evidence of
/// absence by itself.
///
/// Optional evidence on an uncertain disposition must already be authenticated
/// by this trusted adapter. Observations are monotonic: omitting evidence
/// preserves prior knowledge, the first compatible value attaches it, and an
/// incompatible target or different value irreversibly poisons positive
/// ownership settlement for this process. After a conflict, only a definite
/// absence result can discharge the uncertainty.
///
/// This object is a proxy into an engine-owned native registry, not the COM,
/// Objective-C, webview, worker, or process owner itself. Destruction must be
/// passive: `Drop` must not perform native I/O, release native handles, mutate
/// native ownership, or claim that ownership has settled. Methods must not
/// panic; a panic is an adapter invariant failure and Zephium release builds
/// terminate instead of continuing with partially mutated native state. The
/// service must durably record conservative ownership before entering a
/// lifecycle call so startup reconciliation covers process termination at any
/// boundary.
pub trait ExtensionRuntimeOwnershipPort: Send {
    /// Reports a stable upper bound for deterministic control-plane memory
    /// retained by this proxy for this owner.
    ///
    /// The bound must include the concrete adapter allocation and exclusive
    /// host allocations, but not the API's trait-object pointer, package-access
    /// wrapper, engine-owned native handles, webview/worker/process memory, or a
    /// separate native resource reservation. It must remain valid through
    /// activation, retirement, and reconciliation. The service/engine must hold
    /// that separate hard count/reservation for every request, owner, and
    /// uncertainty state. A proxy that cannot provide its deterministic bound
    /// must report `usize::MAX`, causing construction to fail closed. The query
    /// must be side-effect-free so construction refusal retains unchanged state.
    fn retained_bytes(&self) -> usize;

    /// Rejoins copied absence evidence to this exact host reservation.
    ///
    /// The default refuses every proof. Trusted host adapters must override it
    /// and compare the complete owner, backend, runtime family, registry
    /// generation, native attempt, proof kind, and native identity lineage.
    /// A false result converts the claimed absence into uncertainty and keeps
    /// all package/native authority captive.
    fn accepts_absence_evidence(&self, _evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
        false
    }

    /// Attempts to retire the exact native owner before `deadline`.
    ///
    /// `deadline` is the caller's absolute monotonic deadline and must not be
    /// extended, rounded, or replaced. Package bytes and delegated resource
    /// access are deliberately unavailable at this boundary.
    fn retire_until(&mut self, deadline: Instant) -> ExtensionRuntimeRetirementDisposition;

    /// Reconciles native ownership before the exact absolute `deadline`.
    ///
    /// Package bytes and delegated resource access are deliberately
    /// unavailable. Expiration alone is never evidence of absence.
    fn reconcile_ownership_until(
        &mut self,
        deadline: Instant,
    ) -> ExtensionRuntimeOwnershipDisposition;
}

/// Activation-capable extension of an ownership-only runtime proxy.
///
/// Only a fresh, already-authorized activation request exposes temporary
/// package access. Once activation has settled, retirement and reconciliation
/// use only [`ExtensionRuntimeOwnershipPort`]. Persisted recovery therefore
/// cannot read package bytes or accidentally recreate activation authority.
pub trait ExtensionRuntimeLifecyclePort: ExtensionRuntimeOwnershipPort {
    /// Attempts to activate the package before the exact absolute `deadline`.
    ///
    /// Implementations must propagate the deadline unchanged to lower native
    /// layers and return a disposition describing ownership after the attempt.
    fn activate_until(
        &mut self,
        access: &mut ExtensionPackageAccessView<'_>,
        deadline: Instant,
    ) -> ExtensionRuntimeActivationDisposition;
}

/// Why a runtime activation request could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeActivationBuildError {
    /// Combined retained-memory accounting overflowed `usize`.
    RetainedBytesOverflow,
    /// Combined package, lifecycle, and wrapper memory exceeds the per-owner
    /// ceiling.
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionRuntimeActivationBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RetainedBytesOverflow => "extension runtime owner accounting overflowed",
            Self::RetainedBytesExceeded => {
                "extension runtime owner exceeds the retained-memory limit"
            }
        })
    }
}

impl std::error::Error for ExtensionRuntimeActivationBuildError {}

/// A failed activation-request construction that preserves both inputs.
#[must_use = "the refusal retains package access and its lifecycle proxy"]
pub struct ExtensionRuntimeActivationBuildRefusal {
    reason: ExtensionRuntimeActivationBuildError,
    access: ExtensionPackageAccess,
    lifecycle: Box<dyn ExtensionRuntimeLifecyclePort>,
}

impl ExtensionRuntimeActivationBuildRefusal {
    /// Returns the stable refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimeActivationBuildError {
        self.reason
    }

    /// Returns the same package access and lifecycle adapter supplied to the
    /// failed constructor.
    #[must_use = "both returned values retain delegated runtime state"]
    pub fn into_parts(
        self,
    ) -> (
        ExtensionPackageAccess,
        Box<dyn ExtensionRuntimeLifecyclePort>,
    ) {
        (self.access, self.lifecycle)
    }
}

impl fmt::Debug for ExtensionRuntimeActivationBuildRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeActivationBuildRefusal")
            .field("reason", &self.reason)
            .field("access", &"[redacted]")
            .field("lifecycle", &"[redacted]")
            .finish()
    }
}

/// Why persisted native-ownership uncertainty could not be reconstructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeRecoveryBuildError {
    /// Recovery-proxy and wrapper accounting overflowed `usize`.
    RetainedBytesOverflow,
    /// The cleanup-only recovery owner exceeds the per-owner ceiling.
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionRuntimeRecoveryBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RetainedBytesOverflow => "extension runtime recovery accounting overflowed",
            Self::RetainedBytesExceeded => {
                "extension runtime recovery exceeds the retained-memory limit"
            }
        })
    }
}

impl std::error::Error for ExtensionRuntimeRecoveryBuildError {}

/// A failed reconstruction of one conservatively persisted native owner.
///
/// No package access, resource plan, provider, target, or activation-capable
/// port participates in this value. It preserves only the passive ownership
/// proxy supplied by the service. No ownership callback has run. The caller
/// must keep its external native reservation and durable package pin held on
/// both success and refusal. A refusal is an opaque fail-closed quarantine:
/// dropping it destroys only the process-local proxy, whose destructor must be
/// passive, while durable cleanup authority survives for a later process.
///
/// The refusal cannot be cloned:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryBuildRefusal;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeRecoveryBuildRefusal>();
/// ```
///
/// It cannot be shared across threads:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryBuildRefusal;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeRecoveryBuildRefusal>();
/// ```
///
/// It cannot cross a serialization boundary:
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryBuildRefusal;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeRecoveryBuildRefusal>();
/// ```
///
/// It cannot be reconstructed from serialized data:
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryBuildRefusal;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeRecoveryBuildRefusal>();
/// ```
///
/// Its captive ownership proxy cannot be extracted:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryBuildRefusal;
/// fn escape(refusal: ExtensionRuntimeRecoveryBuildRefusal) {
///     let _ = refusal.into_parts();
/// }
/// ```
#[must_use = "the refusal retains cleanup-only ownership-recovery state"]
pub struct ExtensionRuntimeRecoveryBuildRefusal {
    reason: ExtensionRuntimeRecoveryBuildError,
    _ownership: Box<dyn ExtensionRuntimeOwnershipPort>,
}

impl ExtensionRuntimeRecoveryBuildRefusal {
    /// Returns the stable construction refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimeRecoveryBuildError {
        self.reason
    }
}

impl fmt::Debug for ExtensionRuntimeRecoveryBuildRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeRecoveryBuildRefusal")
            .field("reason", &self.reason)
            .field("ownership", &"[redacted]")
            .finish()
    }
}

struct RuntimeOwnershipCore {
    access: ExtensionPackageAccess,
    lifecycle: Box<dyn ExtensionRuntimeLifecyclePort>,
    retained_bytes: usize,
}

#[derive(Clone, Copy)]
struct RuntimeEvidenceObservation {
    evidence: Option<ExtensionRuntimeOwnershipEvidence>,
    poisoned: bool,
}

impl RuntimeEvidenceObservation {
    const fn unknown() -> Self {
        Self {
            evidence: None,
            poisoned: false,
        }
    }

    const fn known(evidence: ExtensionRuntimeOwnershipEvidence) -> Self {
        Self {
            evidence: Some(evidence),
            poisoned: false,
        }
    }

    /// Monotonically incorporates one trusted adapter observation.
    ///
    /// `true` means this observation history is irreversibly conflicted. Once
    /// poisoned, only a later definite-absence settlement may discharge the
    /// owner; no positive evidence can restore definite ownership.
    fn merge(
        &mut self,
        target: ExtensionRuntimeTarget,
        evidence: Option<ExtensionRuntimeOwnershipEvidence>,
    ) -> bool {
        if self.poisoned {
            return true;
        }
        let Some(evidence) = evidence else {
            return false;
        };
        if evidence.target() != target {
            self.poisoned = true;
            return true;
        }
        match self.evidence {
            None => self.evidence = Some(evidence),
            Some(previous) if previous == evidence => {}
            Some(_) => self.poisoned = true,
        }
        self.poisoned
    }
}

impl RuntimeOwnershipCore {
    fn with_activation_view<T>(
        &mut self,
        operation: impl FnOnce(
            &mut dyn ExtensionRuntimeLifecyclePort,
            &mut ExtensionPackageAccessView<'_>,
        ) -> T,
    ) -> T {
        let mut view = ExtensionPackageAccessView {
            access: &mut self.access,
        };
        operation(self.lifecycle.as_mut(), &mut view)
    }
}

/// A move-only activation request that retains both package access and the
/// selected native lifecycle adapter.
///
/// This crate invokes no lifecycle method when the value is dropped and does
/// not expose package access. Lifecycle ports are required to have passive
/// destructors. Callers must settle the request and persist fail-safe ownership
/// at a higher layer.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeActivationRequest;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeActivationRequest>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeActivationRequest;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeActivationRequest>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeActivationRequest;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeActivationRequest>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeActivationRequest;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeActivationRequest>();
/// ```
///
/// Every activation attempt requires an explicit absolute deadline:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeActivationRequest;
/// fn deadline_is_required(request: ExtensionRuntimeActivationRequest) {
///     let _ = request.settle();
/// }
/// ```
#[must_use = "activation requests must be settled to preserve package ownership"]
pub struct ExtensionRuntimeActivationRequest {
    core: RuntimeOwnershipCore,
}

impl ExtensionRuntimeActivationRequest {
    /// Attempts to create a request from one already-authorized package
    /// capability and one bounded runtime adapter.
    ///
    /// The single per-owner ceiling includes package access, lifecycle state,
    /// and API wrapper overhead. Package-access wrapper bytes are included only
    /// once. Overflow or excess returns both inputs unchanged in
    /// [`ExtensionRuntimeActivationBuildRefusal`].
    pub fn try_new(
        access: ExtensionPackageAccess,
        lifecycle: Box<dyn ExtensionRuntimeLifecyclePort>,
    ) -> Result<Self, ExtensionRuntimeActivationBuildRefusal> {
        let wrapper_bytes = match runtime_owner_wrapper_bytes() {
            Some(wrapper_bytes) => wrapper_bytes,
            None => {
                return Err(ExtensionRuntimeActivationBuildRefusal {
                    reason: ExtensionRuntimeActivationBuildError::RetainedBytesOverflow,
                    access,
                    lifecycle,
                });
            }
        };
        let retained_bytes = access
            .retained_bytes()
            .checked_add(wrapper_bytes)
            .and_then(|retained_bytes| retained_bytes.checked_add(lifecycle.retained_bytes()));
        let retained_bytes = match retained_bytes {
            Some(retained_bytes) => retained_bytes,
            None => {
                return Err(ExtensionRuntimeActivationBuildRefusal {
                    reason: ExtensionRuntimeActivationBuildError::RetainedBytesOverflow,
                    access,
                    lifecycle,
                });
            }
        };
        if retained_bytes > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES {
            return Err(ExtensionRuntimeActivationBuildRefusal {
                reason: ExtensionRuntimeActivationBuildError::RetainedBytesExceeded,
                access,
                lifecycle,
            });
        }

        Ok(Self {
            core: RuntimeOwnershipCore {
                access,
                lifecycle,
                retained_bytes,
            },
        })
    }

    /// Returns the selected runtime family without exposing package ownership.
    #[must_use]
    pub const fn target(&self) -> ExtensionRuntimeTarget {
        self.core.access.target()
    }

    /// Returns the stable total retained-memory charge for this owner.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.core.retained_bytes
    }

    /// Cancels a request that has never activated or was returned retryable.
    ///
    /// Both states prove native ownership absent, so the exact access and
    /// lifecycle adapter can safely return to the service without a native
    /// call. This method performs no I/O or settlement action.
    #[must_use = "cancelled activation inputs retain delegated package state"]
    pub fn cancel(
        self,
    ) -> (
        ExtensionPackageAccess,
        Box<dyn ExtensionRuntimeLifecyclePort>,
    ) {
        (self.core.access, self.core.lifecycle)
    }

    /// Attempts activation before the caller's absolute monotonic `deadline`.
    ///
    /// An already-expired deadline performs no adapter call. Because an
    /// unattempted activation is definitely absent, it returns the same
    /// retryable request with [`ExtensionRuntimeFailure::TimedOut`].
    pub fn settle_until(mut self, deadline: Instant) -> ExtensionRuntimeActivationSettlement {
        if deadline <= Instant::now() {
            return ExtensionRuntimeActivationSettlement::Retryable {
                request: self,
                failure: ExtensionRuntimeFailure::TimedOut,
                absence: None,
            };
        }
        let disposition = self
            .core
            .with_activation_view(|lifecycle, access| lifecycle.activate_until(access, deadline));
        match disposition {
            ExtensionRuntimeActivationDisposition::Activated(evidence)
                if evidence.target() == self.target() =>
            {
                ExtensionRuntimeActivationSettlement::Activated(ExtensionRuntimeOwner {
                    core: self.core,
                    evidence,
                })
            }
            ExtensionRuntimeActivationDisposition::Activated(evidence) => {
                // A trusted adapter reported a definite owner for a different
                // runtime family. Native ownership may exist, but it cannot be
                // joined to this package request as definite ownership.
                let mut observation = RuntimeEvidenceObservation::unknown();
                let conflicted = observation.merge(self.target(), Some(evidence));
                debug_assert!(conflicted);
                ExtensionRuntimeActivationSettlement::OwnershipUncertain {
                    owner: ExtensionRuntimeUncertainOwner {
                        core: self.core,
                        observation,
                    },
                    failure: ExtensionRuntimeFailure::Internal,
                }
            }
            ExtensionRuntimeActivationDisposition::Retryable { failure, absence } => {
                if absence.target() == self.target()
                    && self.core.lifecycle.accepts_absence_evidence(absence)
                {
                    ExtensionRuntimeActivationSettlement::Retryable {
                        request: self,
                        failure,
                        absence: Some(absence),
                    }
                } else {
                    ExtensionRuntimeActivationSettlement::OwnershipUncertain {
                        owner: ExtensionRuntimeUncertainOwner {
                            core: self.core,
                            observation: RuntimeEvidenceObservation::unknown(),
                        },
                        failure: ExtensionRuntimeFailure::Internal,
                    }
                }
            }
            ExtensionRuntimeActivationDisposition::Rejected { failure, absence } => {
                if absence.target() == self.target()
                    && self.core.lifecycle.accepts_absence_evidence(absence)
                {
                    ExtensionRuntimeActivationSettlement::Rejected {
                        access: self.core.access,
                        failure,
                        absence,
                    }
                } else {
                    ExtensionRuntimeActivationSettlement::OwnershipUncertain {
                        owner: ExtensionRuntimeUncertainOwner {
                            core: self.core,
                            observation: RuntimeEvidenceObservation::unknown(),
                        },
                        failure: ExtensionRuntimeFailure::Internal,
                    }
                }
            }
            ExtensionRuntimeActivationDisposition::OwnershipUncertain { failure, evidence } => {
                let mut observation = RuntimeEvidenceObservation::unknown();
                let failure = if observation.merge(self.target(), evidence) {
                    ExtensionRuntimeFailure::Internal
                } else {
                    failure
                };
                ExtensionRuntimeActivationSettlement::OwnershipUncertain {
                    owner: ExtensionRuntimeUncertainOwner {
                        core: self.core,
                        observation,
                    },
                    failure,
                }
            }
        }
    }
}

fn runtime_owner_wrapper_bytes() -> Option<usize> {
    // Settlements are public move-only states and may be retained without
    // immediate destructuring, so their discriminants and failure payloads
    // participate in the same hard per-owner ceiling.
    let largest_owner_state = [
        std::mem::size_of::<ExtensionRuntimeActivationRequest>(),
        std::mem::size_of::<ExtensionRuntimeOwner>(),
        std::mem::size_of::<ExtensionRuntimeRetirementRequest>(),
        std::mem::size_of::<ExtensionRuntimeUncertainOwner>(),
        std::mem::size_of::<ExtensionRuntimeActivationSettlement>(),
        std::mem::size_of::<ExtensionRuntimeRetirementSettlement>(),
        std::mem::size_of::<ExtensionRuntimeReconciliationSettlement>(),
    ]
    .into_iter()
    .max()?;
    largest_owner_state.checked_sub(std::mem::size_of::<ExtensionPackageAccess>())
}

impl fmt::Debug for ExtensionRuntimeActivationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeActivationRequest")
            .field("target", &self.target())
            .field("retained_bytes", &self.retained_bytes())
            .field("core", &"[redacted]")
            .finish()
    }
}

/// Ownership-safe result of an activation attempt.
#[must_use]
pub enum ExtensionRuntimeActivationSettlement {
    /// Native ownership definitely exists.
    Activated(ExtensionRuntimeOwner),
    /// Native ownership is definitely absent and the same request may be
    /// retried without rebuilding delegated access.
    Retryable {
        /// The original move-only request.
        request: ExtensionRuntimeActivationRequest,
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
        /// Exact proof which authorized the definite-absence settlement.
        absence: Option<ExtensionRuntimeAbsenceEvidence>,
    },
    /// Native ownership is definitely absent and package access is returned to
    /// the package service.
    Rejected {
        /// The same delegated package-access capability.
        access: ExtensionPackageAccess,
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
        /// Exact proof which authorized the definite-absence settlement.
        absence: ExtensionRuntimeAbsenceEvidence,
    },
    /// Native ownership may exist, so access remains captive until explicit
    /// reconciliation proves absence.
    OwnershipUncertain {
        /// The uncertainty capability retaining package access and lifecycle.
        owner: ExtensionRuntimeUncertainOwner,
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
    },
}

impl ExtensionRuntimeActivationSettlement {
    /// Returns the conservative retained-memory charge while this settlement
    /// remains queued or otherwise unconsumed.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::Activated(owner) => owner.retained_bytes(),
            Self::Retryable { request, .. } => request.retained_bytes(),
            Self::Rejected { access, .. } => access_settlement_retained_bytes::<Self>(access),
            Self::OwnershipUncertain { owner, .. } => owner.retained_bytes(),
        }
    }
}

impl fmt::Debug for ExtensionRuntimeActivationSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Activated(_) => formatter.write_str("Activated([redacted])"),
            Self::Retryable { failure, .. } => formatter
                .debug_struct("Retryable")
                .field("request", &"[redacted]")
                .field("failure", failure)
                .finish(),
            Self::Rejected { failure, .. } => formatter
                .debug_struct("Rejected")
                .field("access", &"[redacted]")
                .field("failure", failure)
                .finish(),
            Self::OwnershipUncertain { failure, .. } => formatter
                .debug_struct("OwnershipUncertain")
                .field("owner", &"[redacted]")
                .field("failure", failure)
                .finish(),
        }
    }
}

/// Move-only state reporting definite ownership from the trusted lifecycle port.
///
/// There is intentionally no release or unpin operation. Definite retirement
/// returns package access to the service; uncertain retirement keeps it captive.
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeOwner;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeOwner>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeOwner;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeOwner>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeOwner;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeOwner>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeOwner;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeOwner>();
/// ```
///
/// Definite ownership cannot extract the delegated provider:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeOwner;
/// fn escape(owner: ExtensionRuntimeOwner) {
///     let _ = owner.into_provider();
/// }
/// ```
#[must_use = "runtime ownership must be explicitly retired"]
pub struct ExtensionRuntimeOwner {
    core: RuntimeOwnershipCore,
    evidence: ExtensionRuntimeOwnershipEvidence,
}

impl ExtensionRuntimeOwner {
    /// Returns the selected runtime family.
    #[must_use]
    pub const fn target(&self) -> ExtensionRuntimeTarget {
        self.core.access.target()
    }

    /// Returns a copied structural description of the exact native ownership
    /// evidence authenticated by the service-selected lifecycle adapter.
    /// The copied value is non-authorizing outside this owner capability.
    #[must_use]
    pub const fn ownership_evidence(&self) -> ExtensionRuntimeOwnershipEvidence {
        self.evidence
    }

    /// Returns the authenticated manifest descriptor.
    #[must_use]
    pub fn manifest(&self) -> ExtensionRuntimeResource {
        self.core.access.manifest()
    }

    /// Borrows the complete canonical package-resource plan.
    #[must_use]
    pub const fn resources(&self) -> &ExtensionRuntimeResourcePlan {
        self.core.access.resources()
    }

    /// Returns the stable total retained-memory charge for this owner.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.core.retained_bytes
    }

    /// Visits one package resource while native ownership is definite.
    pub fn visit_resource(
        &mut self,
        resource: ExtensionRuntimeResource,
        visitor: &mut dyn ExtensionRuntimeResourceVisitor,
    ) -> Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError> {
        self.core.access.visit_resource(resource, visitor)
    }

    /// Visits the package manifest while native ownership is definite.
    pub fn visit_manifest(
        &mut self,
        visitor: &mut dyn ExtensionRuntimeResourceVisitor,
    ) -> Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError> {
        self.core.access.visit_manifest(visitor)
    }

    /// Consumes definite ownership into the only retirement entry point.
    pub fn into_retirement_request(self) -> ExtensionRuntimeRetirementRequest {
        ExtensionRuntimeRetirementRequest {
            core: self.core,
            evidence: self.evidence,
        }
    }
}

impl fmt::Debug for ExtensionRuntimeOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeOwner")
            .field("target", &self.target())
            .field("ownership_evidence", &self.evidence)
            .field("retained_bytes", &self.retained_bytes())
            .field("core", &"[redacted]")
            .finish()
    }
}

/// Move-only request to retire a definitely owned native activation.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRetirementRequest;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeRetirementRequest>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRetirementRequest;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeRetirementRequest>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeRetirementRequest;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeRetirementRequest>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeRetirementRequest;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeRetirementRequest>();
/// ```
///
/// Every retirement attempt requires an explicit absolute deadline:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRetirementRequest;
/// fn deadline_is_required(request: ExtensionRuntimeRetirementRequest) {
///     let _ = request.settle();
/// }
/// ```
#[must_use = "retirement requests must be settled to preserve package ownership"]
pub struct ExtensionRuntimeRetirementRequest {
    core: RuntimeOwnershipCore,
    evidence: ExtensionRuntimeOwnershipEvidence,
}

impl ExtensionRuntimeRetirementRequest {
    /// Returns the stable total retained-memory charge for this owner.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.core.retained_bytes
    }

    /// Returns a copied structural description of the exact native ownership
    /// evidence retained for this request. The copy is non-authorizing outside
    /// this ownership capability.
    #[must_use]
    pub const fn ownership_evidence(&self) -> ExtensionRuntimeOwnershipEvidence {
        self.evidence
    }

    /// Cancels retirement without changing definite native ownership.
    ///
    /// No lifecycle method is called; the original owner state is restored.
    #[must_use = "cancelled retirement still owns the native runtime"]
    pub fn cancel(self) -> ExtensionRuntimeOwner {
        ExtensionRuntimeOwner {
            core: self.core,
            evidence: self.evidence,
        }
    }

    /// Attempts retirement before the caller's absolute monotonic `deadline`.
    ///
    /// An already-expired deadline performs no adapter call and restores the
    /// definitely-owned state. Expiration never proves native absence.
    pub fn settle_until(self, deadline: Instant) -> ExtensionRuntimeRetirementSettlement {
        if deadline <= Instant::now() {
            return ExtensionRuntimeRetirementSettlement::Retained {
                owner: ExtensionRuntimeOwner {
                    core: self.core,
                    evidence: self.evidence,
                },
                failure: ExtensionRuntimeFailure::TimedOut,
            };
        }
        let mut core = self.core;
        let disposition = core.lifecycle.retire_until(deadline);
        match disposition {
            ExtensionRuntimeRetirementDisposition::Retired(absence) => {
                if !matches!(
                    absence.proof_kind(),
                    crate::ExtensionRuntimeAbsenceProofKind::ActivationNeverEntered
                ) && absence.target() == core.access.target()
                    && core.lifecycle.accepts_absence_evidence(absence)
                {
                    ExtensionRuntimeRetirementSettlement::Retired {
                        access: core.access,
                        absence,
                    }
                } else {
                    ExtensionRuntimeRetirementSettlement::OwnershipUncertain {
                        owner: ExtensionRuntimeUncertainOwner {
                            core,
                            observation: RuntimeEvidenceObservation::known(self.evidence),
                        },
                        failure: ExtensionRuntimeFailure::Internal,
                    }
                }
            }
            ExtensionRuntimeRetirementDisposition::Retained(failure) => {
                ExtensionRuntimeRetirementSettlement::Retained {
                    owner: ExtensionRuntimeOwner {
                        core,
                        evidence: self.evidence,
                    },
                    failure,
                }
            }
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain { failure, evidence } => {
                let mut observation = RuntimeEvidenceObservation::known(self.evidence);
                let failure = if observation.merge(core.access.target(), evidence) {
                    ExtensionRuntimeFailure::Internal
                } else {
                    failure
                };
                ExtensionRuntimeRetirementSettlement::OwnershipUncertain {
                    owner: ExtensionRuntimeUncertainOwner { core, observation },
                    failure,
                }
            }
        }
    }
}

impl fmt::Debug for ExtensionRuntimeRetirementRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeRetirementRequest")
            .field("ownership_evidence", &self.evidence)
            .field("retained_bytes", &self.retained_bytes())
            .field("core", &"[redacted]")
            .finish()
    }
}

/// Ownership-safe result of a retirement attempt.
#[must_use]
pub enum ExtensionRuntimeRetirementSettlement {
    /// Native ownership is definitely absent, so package access returns to the
    /// package service.
    Retired {
        /// The same delegated package-access capability.
        access: ExtensionPackageAccess,
        /// Exact proof which authorized the definite-absence settlement.
        absence: ExtensionRuntimeAbsenceEvidence,
    },
    /// Native ownership definitely remains active.
    Retained {
        /// Restored definite runtime ownership.
        owner: ExtensionRuntimeOwner,
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
    },
    /// Native ownership may remain, so access stays captive.
    OwnershipUncertain {
        /// The uncertainty capability retaining access and lifecycle.
        owner: ExtensionRuntimeUncertainOwner,
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
    },
}

impl ExtensionRuntimeRetirementSettlement {
    /// Returns the conservative retained-memory charge while this settlement
    /// remains queued or otherwise unconsumed.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::Retired { access, .. } => access_settlement_retained_bytes::<Self>(access),
            Self::Retained { owner, .. } => owner.retained_bytes(),
            Self::OwnershipUncertain { owner, .. } => owner.retained_bytes(),
        }
    }
}

impl fmt::Debug for ExtensionRuntimeRetirementSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retired { .. } => formatter.write_str("Retired([redacted])"),
            Self::Retained { failure, .. } => formatter
                .debug_struct("Retained")
                .field("owner", &"[redacted]")
                .field("failure", failure)
                .finish(),
            Self::OwnershipUncertain { failure, .. } => formatter
                .debug_struct("OwnershipUncertain")
                .field("owner", &"[redacted]")
                .field("failure", failure)
                .finish(),
        }
    }
}

/// Move-only state retaining access while trusted native ownership is unresolved.
///
/// This type exposes no package access, visitor, release, or unpin path. Only a
/// reconciliation result of [`ExtensionRuntimeOwnershipDisposition::Absent`]
/// returns access to the package service.
///
/// Authenticated evidence observations are monotonic. If the adapter ever
/// reports an incompatible target or a value different from previously known
/// evidence, this state is irreversibly poisoned: later positive evidence
/// cannot restore definite ownership during this process, even if it matches
/// the original value. Definite absence remains the sole discharge path.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeUncertainOwner;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeUncertainOwner>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeUncertainOwner;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeUncertainOwner>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeUncertainOwner;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeUncertainOwner>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeUncertainOwner;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeUncertainOwner>();
/// ```
///
/// Uncertain ownership cannot extract the delegated provider:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeUncertainOwner;
/// fn escape(owner: ExtensionRuntimeUncertainOwner) {
///     let _ = owner.into_provider();
/// }
/// ```
///
/// Every reconciliation attempt requires an explicit absolute deadline:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeUncertainOwner;
/// fn deadline_is_required(owner: ExtensionRuntimeUncertainOwner) {
///     let _ = owner.reconcile();
/// }
/// ```
#[must_use = "uncertain ownership must be reconciled before access can return"]
pub struct ExtensionRuntimeUncertainOwner {
    core: RuntimeOwnershipCore,
    observation: RuntimeEvidenceObservation,
}

impl ExtensionRuntimeUncertainOwner {
    /// Returns the selected runtime family without exposing package access.
    #[must_use]
    pub const fn target(&self) -> ExtensionRuntimeTarget {
        self.core.access.target()
    }

    /// Returns a copied structural description of exact native ownership
    /// evidence when one has already been authenticated. The copy is
    /// non-authorizing outside this ownership capability. Activation can
    /// become uncertain before an identifier is available, so absence here
    /// does not prove native ownership absent.
    #[must_use]
    pub const fn ownership_evidence(&self) -> Option<ExtensionRuntimeOwnershipEvidence> {
        self.observation.evidence
    }

    /// Returns the stable total retained-memory charge for this owner.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.core.retained_bytes
    }

    /// Reconciles native ownership before the caller's absolute monotonic
    /// `deadline` and returns access only on definite absence.
    ///
    /// An already-expired deadline performs no adapter call and preserves
    /// uncertainty.
    pub fn reconcile_until(
        mut self,
        deadline: Instant,
    ) -> ExtensionRuntimeReconciliationSettlement {
        if deadline <= Instant::now() {
            return ExtensionRuntimeReconciliationSettlement::StillUncertain {
                owner: self,
                failure: ExtensionRuntimeFailure::TimedOut,
            };
        }
        let disposition = self.core.lifecycle.reconcile_ownership_until(deadline);
        match disposition {
            ExtensionRuntimeOwnershipDisposition::Owned(evidence) => {
                let target = self.target();
                if self.observation.merge(target, Some(evidence)) {
                    ExtensionRuntimeReconciliationSettlement::StillUncertain {
                        owner: self,
                        failure: ExtensionRuntimeFailure::Internal,
                    }
                } else {
                    ExtensionRuntimeReconciliationSettlement::Owned(ExtensionRuntimeOwner {
                        core: self.core,
                        evidence,
                    })
                }
            }
            ExtensionRuntimeOwnershipDisposition::Absent(absence) => {
                if absence.target() == self.target()
                    && self.core.lifecycle.accepts_absence_evidence(absence)
                {
                    ExtensionRuntimeReconciliationSettlement::Absent {
                        access: self.core.access,
                        absence,
                    }
                } else {
                    ExtensionRuntimeReconciliationSettlement::StillUncertain {
                        owner: self,
                        failure: ExtensionRuntimeFailure::Internal,
                    }
                }
            }
            ExtensionRuntimeOwnershipDisposition::StillUncertain { failure, evidence } => {
                let target = self.target();
                let failure = if self.observation.merge(target, evidence) {
                    ExtensionRuntimeFailure::Internal
                } else {
                    failure
                };
                ExtensionRuntimeReconciliationSettlement::StillUncertain {
                    owner: self,
                    failure,
                }
            }
        }
    }
}

impl fmt::Debug for ExtensionRuntimeUncertainOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeUncertainOwner")
            .field("target", &self.target())
            .field("ownership_evidence", &self.observation.evidence)
            .field("evidence_conflicted", &self.observation.poisoned)
            .field("retained_bytes", &self.retained_bytes())
            .field("core", &"[redacted]")
            .finish()
    }
}

/// Ownership-safe result of uncertainty reconciliation.
#[must_use]
pub enum ExtensionRuntimeReconciliationSettlement {
    /// Native ownership is now definitely present.
    Owned(ExtensionRuntimeOwner),
    /// Native ownership is definitely absent, so package access returns to the
    /// package service.
    Absent {
        /// The same delegated package-access capability.
        access: ExtensionPackageAccess,
        /// Exact proof which authorized the definite-absence settlement.
        absence: ExtensionRuntimeAbsenceEvidence,
    },
    /// Native ownership remains unresolved and access stays captive.
    StillUncertain {
        /// The renewed uncertainty capability.
        owner: ExtensionRuntimeUncertainOwner,
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
    },
}

impl ExtensionRuntimeReconciliationSettlement {
    /// Returns the conservative retained-memory charge while this settlement
    /// remains queued or otherwise unconsumed.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::Owned(owner) => owner.retained_bytes(),
            Self::Absent { access, .. } => access_settlement_retained_bytes::<Self>(access),
            Self::StillUncertain { owner, .. } => owner.retained_bytes(),
        }
    }
}

fn access_settlement_retained_bytes<Settlement>(access: &ExtensionPackageAccess) -> usize {
    access.retained_bytes().saturating_add(
        std::mem::size_of::<Settlement>()
            .saturating_sub(std::mem::size_of::<ExtensionPackageAccess>()),
    )
}

impl fmt::Debug for ExtensionRuntimeReconciliationSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Owned(_) => formatter.write_str("Owned([redacted])"),
            Self::Absent { .. } => formatter.write_str("Absent([redacted])"),
            Self::StillUncertain { failure, .. } => formatter
                .debug_struct("StillUncertain")
                .field("owner", &"[redacted]")
                .field("failure", failure)
                .finish(),
        }
    }
}

struct RecoveryOwnershipCore {
    ownership: Box<dyn ExtensionRuntimeOwnershipPort>,
    observation: RecoveryEvidenceObservation,
    retained_bytes: usize,
}

impl RecoveryOwnershipCore {
    fn merge_evidence(&mut self, evidence: Option<ExtensionRuntimeOwnershipEvidence>) -> bool {
        self.observation.merge(evidence)
    }
}

#[derive(Clone, Copy)]
struct RecoveryEvidenceObservation {
    expectation: ExtensionRuntimeRecoveryExpectation,
    evidence: Option<ExtensionRuntimeOwnershipEvidence>,
    poisoned: bool,
}

impl RecoveryEvidenceObservation {
    fn new(expectation: ExtensionRuntimeRecoveryExpectation) -> Self {
        Self {
            evidence: expectation.known_evidence(),
            expectation,
            poisoned: expectation.has_identity_conflict(),
        }
    }

    fn merge(&mut self, evidence: Option<ExtensionRuntimeOwnershipEvidence>) -> bool {
        if self.poisoned {
            return true;
        }
        let Some(evidence) = evidence else {
            return false;
        };
        if !self.expectation.accepts_evidence_backend(evidence) {
            self.poisoned = true;
            return true;
        }
        match self.evidence {
            None => {
                self.evidence = Some(evidence);
                if !self.expectation.accepts(evidence) {
                    self.poisoned = true;
                }
            }
            Some(previous) if previous == evidence => {}
            Some(_) => self.poisoned = true,
        }
        self.poisoned
    }

    fn accepts_absence(self, absence: ExtensionRuntimeAbsenceEvidence) -> bool {
        use zephium_core::extensions::ExtensionRuntimeBackendTarget;

        match self.expectation {
            ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                catalog_expected,
                adapter_observed,
            } => {
                absence.target() == ExtensionRuntimeTarget::NativeWebExtension
                    && absence.backend() == ExtensionRuntimeBackendTarget::MacosNative
                    && absence.expected_native_identity() == catalog_expected
                    && match absence.proof_kind() {
                        crate::ExtensionRuntimeAbsenceProofKind::MacosZeroGrantsAndUnloaded => {
                            let identity_anchor = catalog_expected.or(adapter_observed);
                            identity_anchor.is_some()
                                && absence.observed_native_identity() == identity_anchor
                                && adapter_observed.is_none_or(|observed| {
                                    absence.observed_native_identity() == Some(observed)
                                })
                        }
                        crate::ExtensionRuntimeAbsenceProofKind::ActivationNeverEntered
                        | crate::ExtensionRuntimeAbsenceProofKind::CompatibilityRegistryAbsentAndQuiescent => false,
                    }
            }
            ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension { .. } => false,
            ExtensionRuntimeRecoveryExpectation::Compatibility => {
                absence.target() == ExtensionRuntimeTarget::Compatibility
                    && matches!(
                        absence.backend(),
                        ExtensionRuntimeBackendTarget::MacosCompatibility
                            | ExtensionRuntimeBackendTarget::LinuxCompatibility
                    )
                    && absence.expected_native_identity().is_none()
                    && absence.observed_native_identity().is_none()
                    && matches!(
                        absence.proof_kind(),
                        crate::ExtensionRuntimeAbsenceProofKind::CompatibilityRegistryAbsentAndQuiescent
                    )
            }
        }
    }
}

struct RecoveryOwnedCore {
    ownership: Box<dyn ExtensionRuntimeOwnershipPort>,
    expectation: ExtensionRuntimeRecoveryExpectation,
    evidence: ExtensionRuntimeOwnershipEvidence,
    retained_bytes: usize,
}

impl RecoveryOwnedCore {
    fn into_uncertain(self) -> RecoveryOwnershipCore {
        RecoveryOwnershipCore {
            ownership: self.ownership,
            observation: RecoveryEvidenceObservation {
                expectation: self.expectation,
                evidence: Some(self.evidence),
                poisoned: false,
            },
            retained_bytes: self.retained_bytes,
        }
    }
}

/// Cleanup-only reconstruction of one conservatively persisted native owner.
///
/// The caller must first join an unresolved durable ownership row to the exact
/// native incarnation and hold the separate native-resource reservation. This
/// request carries no package access, resource plan, provider, target, or
/// activation-capable port. Definite absence releases only this process-local
/// control proxy; the service may then settle its separately retained durable
/// package pin.
///
/// A request may begin with only the exact native backend class for a legacy
/// row, with a catalog expectation but no adapter observation, or with an
/// adapter observation but no legacy catalog expectation. The first matching
/// trusted observation attaches evidence when none was persisted. Any adapter
/// evidence inconsistent with the durable backend class, catalog expectation,
/// already-observed identifier, or a previously attached identifier
/// irreversibly poisons positive settlement in this process. A mismatch already
/// present between the two durable identity claims starts poisoned. Later
/// matching evidence cannot heal the conflict; only definite absence can
/// discharge it.
///
/// Construction calls only the ownership port's side-effect-free accounting
/// query. Every destructor remains passive.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRequest;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeRecoveryRequest>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRequest;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeRecoveryRequest>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRequest;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeRecoveryRequest>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRequest;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeRecoveryRequest>();
/// ```
///
/// Cleanup recovery cannot extract package access, recover package metadata,
/// activate a runtime, or omit a deadline:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRequest;
/// fn escape(request: ExtensionRuntimeRecoveryRequest) {
///     let _ = request.into_provider();
/// }
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRequest;
/// fn no_target(request: &ExtensionRuntimeRecoveryRequest) {
///     let _ = request.target();
/// }
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRequest;
/// fn no_resource_plan(request: &ExtensionRuntimeRecoveryRequest) {
///     let _ = request.resources();
/// }
/// ```
///
/// ```compile_fail
/// use std::time::Instant;
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRequest;
/// fn no_activation(request: ExtensionRuntimeRecoveryRequest) {
///     let _ = request.settle_until(Instant::now());
/// }
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRequest;
/// fn deadline_is_required(request: ExtensionRuntimeRecoveryRequest) {
///     let _ = request.reconcile();
/// }
/// ```
#[must_use = "persisted ownership must be reconciled before durable cleanup"]
pub struct ExtensionRuntimeRecoveryRequest {
    core: RecoveryOwnershipCore,
}

impl ExtensionRuntimeRecoveryRequest {
    /// Constructs one cleanup-only request from an exact-incarnation ownership
    /// proxy after the service has validated its durable identity join.
    ///
    /// `expectation` must come from that exact durable row. Legacy native rows
    /// may omit either independently persisted identity claim while they
    /// conservatively record that the matching backend class may own the
    /// runtime. Constructing the public structural expectation does not itself
    /// authorize cleanup or prove that a native owner exists.
    pub fn try_from_persisted_uncertainty(
        ownership: Box<dyn ExtensionRuntimeOwnershipPort>,
        expectation: ExtensionRuntimeRecoveryExpectation,
    ) -> Result<Self, ExtensionRuntimeRecoveryBuildRefusal> {
        let retained_bytes = ownership
            .retained_bytes()
            .checked_add(recovery_owner_wrapper_bytes());
        let retained_bytes = match retained_bytes {
            Some(retained_bytes) => retained_bytes,
            None => {
                return Err(ExtensionRuntimeRecoveryBuildRefusal {
                    reason: ExtensionRuntimeRecoveryBuildError::RetainedBytesOverflow,
                    _ownership: ownership,
                });
            }
        };
        if retained_bytes > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES {
            return Err(ExtensionRuntimeRecoveryBuildRefusal {
                reason: ExtensionRuntimeRecoveryBuildError::RetainedBytesExceeded,
                _ownership: ownership,
            });
        }
        Ok(Self {
            core: RecoveryOwnershipCore {
                ownership,
                observation: RecoveryEvidenceObservation::new(expectation),
                retained_bytes,
            },
        })
    }

    /// Returns the exact conservative retained-memory charge for this owner.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.core.retained_bytes
    }

    /// Returns the closed backend and independent identity claims loaded from
    /// the durable ownership row. The copy is non-authorizing outside this
    /// cleanup capability.
    #[must_use]
    pub const fn expectation(&self) -> ExtensionRuntimeRecoveryExpectation {
        self.core.observation.expectation
    }

    /// Returns exact ownership evidence already persisted or monotonically
    /// attached from a trusted adapter observation.
    ///
    /// Native recovery without adapter-observed evidence returns `None` until
    /// the first matching observation; a catalog expectation alone is not
    /// evidence. Compatibility recovery is exact from construction. The copy
    /// remains non-authorizing outside this cleanup capability.
    #[must_use]
    pub const fn ownership_evidence(&self) -> Option<ExtensionRuntimeOwnershipEvidence> {
        self.core.observation.evidence
    }

    /// Reconciles native ownership before the exact absolute `deadline`.
    ///
    /// An already-expired deadline performs no callback and preserves
    /// uncertainty. Definite absence returns no package or release capability.
    pub fn reconcile_until(self, deadline: Instant) -> ExtensionRuntimeRecoverySettlement {
        if deadline <= Instant::now() {
            return ExtensionRuntimeRecoverySettlement::StillUncertain {
                request: self,
                failure: ExtensionRuntimeFailure::TimedOut,
            };
        }
        let mut core = self.core;
        match core.ownership.reconcile_ownership_until(deadline) {
            ExtensionRuntimeOwnershipDisposition::Owned(evidence) => {
                if core.merge_evidence(Some(evidence)) {
                    ExtensionRuntimeRecoverySettlement::StillUncertain {
                        request: Self { core },
                        failure: ExtensionRuntimeFailure::Internal,
                    }
                } else {
                    match core.observation.evidence {
                        Some(evidence) => ExtensionRuntimeRecoverySettlement::Owned(
                            ExtensionRuntimeRecoveryOwner {
                                core: RecoveryOwnedCore {
                                    ownership: core.ownership,
                                    expectation: core.observation.expectation,
                                    evidence,
                                    retained_bytes: core.retained_bytes,
                                },
                            },
                        ),
                        None => ExtensionRuntimeRecoverySettlement::StillUncertain {
                            request: Self { core },
                            failure: ExtensionRuntimeFailure::Internal,
                        },
                    }
                }
            }
            ExtensionRuntimeOwnershipDisposition::Absent(absence) => {
                if core.observation.accepts_absence(absence)
                    && core.ownership.accepts_absence_evidence(absence)
                {
                    ExtensionRuntimeRecoverySettlement::Absent(absence)
                } else {
                    ExtensionRuntimeRecoverySettlement::StillUncertain {
                        request: Self { core },
                        failure: ExtensionRuntimeFailure::Internal,
                    }
                }
            }
            ExtensionRuntimeOwnershipDisposition::StillUncertain { failure, evidence } => {
                let failure = if core.merge_evidence(evidence) {
                    ExtensionRuntimeFailure::Internal
                } else {
                    failure
                };
                ExtensionRuntimeRecoverySettlement::StillUncertain {
                    request: Self { core },
                    failure,
                }
            }
        }
    }
}

impl fmt::Debug for ExtensionRuntimeRecoveryRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeRecoveryRequest")
            .field("expectation", &self.core.observation.expectation)
            .field("ownership_evidence", &self.core.observation.evidence)
            .field("evidence_conflicted", &self.core.observation.poisoned)
            .field("retained_bytes", &self.retained_bytes())
            .field("ownership", &"[redacted]")
            .finish()
    }
}

/// Cleanup-only result of persisted ownership reconciliation.
#[must_use]
pub enum ExtensionRuntimeRecoverySettlement {
    /// The exact native owner definitely exists.
    Owned(ExtensionRuntimeRecoveryOwner),
    /// Native ownership is definitely absent. No package capability is
    /// created; the service may proceed with its separate durable cleanup.
    Absent(ExtensionRuntimeAbsenceEvidence),
    /// Native ownership remains indeterminate.
    StillUncertain {
        /// The same cleanup-only request.
        request: ExtensionRuntimeRecoveryRequest,
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
    },
}

impl ExtensionRuntimeRecoverySettlement {
    /// Returns the exact conservative retained-memory charge of this value.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        match self {
            Self::Owned(owner) => owner.retained_bytes(),
            Self::Absent(_) => std::mem::size_of::<Self>(),
            Self::StillUncertain { request, .. } => request.retained_bytes(),
        }
    }
}

impl fmt::Debug for ExtensionRuntimeRecoverySettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Owned(_) => formatter.write_str("Owned([redacted])"),
            Self::Absent(_) => formatter.write_str("Absent([redacted])"),
            Self::StillUncertain { failure, .. } => formatter
                .debug_struct("StillUncertain")
                .field("request", &"[redacted]")
                .field("failure", failure)
                .finish(),
        }
    }
}

/// Cleanup-only proof that the exact persisted native owner exists.
///
/// This move-only value exposes no package data or activation operation.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryOwner;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeRecoveryOwner>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryOwner;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeRecoveryOwner>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryOwner;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeRecoveryOwner>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryOwner;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeRecoveryOwner>();
/// ```
///
/// Definite recovered ownership still cannot expose package data:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryOwner;
/// fn no_package_access(mut owner: ExtensionRuntimeRecoveryOwner) {
///     let _ = owner.visit_manifest(&mut |_| Ok(()));
/// }
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryOwner;
/// fn no_provider(owner: ExtensionRuntimeRecoveryOwner) {
///     let _ = owner.into_provider();
/// }
/// ```
#[must_use = "recovered native ownership must be explicitly retired"]
pub struct ExtensionRuntimeRecoveryOwner {
    core: RecoveryOwnedCore,
}

impl ExtensionRuntimeRecoveryOwner {
    /// Returns a copied structural description of the exact native ownership
    /// evidence confirmed against the durable ownership row. The copy is
    /// non-authorizing outside this cleanup owner capability.
    #[must_use]
    pub const fn ownership_evidence(&self) -> ExtensionRuntimeOwnershipEvidence {
        self.core.evidence
    }

    /// Returns the exact conservative retained-memory charge for this owner.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.core.retained_bytes
    }

    /// Consumes definite ownership into its only retirement entry point.
    pub fn into_retirement_request(self) -> ExtensionRuntimeRecoveryRetirementRequest {
        ExtensionRuntimeRecoveryRetirementRequest { core: self.core }
    }
}

impl fmt::Debug for ExtensionRuntimeRecoveryOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeRecoveryOwner")
            .field("ownership_evidence", &self.core.evidence)
            .field("retained_bytes", &self.retained_bytes())
            .field("ownership", &"[redacted]")
            .finish()
    }
}

/// Cleanup-only request to retire a definitely present persisted owner.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRetirementRequest;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeRecoveryRetirementRequest>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRetirementRequest;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeRecoveryRetirementRequest>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRetirementRequest;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeRecoveryRetirementRequest>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRetirementRequest;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeRecoveryRetirementRequest>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryRetirementRequest;
/// fn deadline_is_required(request: ExtensionRuntimeRecoveryRetirementRequest) {
///     let _ = request.settle();
/// }
/// ```
#[must_use = "persisted retirement must settle before durable cleanup"]
pub struct ExtensionRuntimeRecoveryRetirementRequest {
    core: RecoveryOwnedCore,
}

impl ExtensionRuntimeRecoveryRetirementRequest {
    /// Returns a copied structural description of the exact native ownership
    /// evidence retained for this request. The copy is non-authorizing outside
    /// this cleanup ownership capability.
    #[must_use]
    pub const fn ownership_evidence(&self) -> ExtensionRuntimeOwnershipEvidence {
        self.core.evidence
    }

    /// Returns the exact conservative retained-memory charge for this owner.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.core.retained_bytes
    }

    /// Cancels retirement without calling the adapter or changing ownership.
    #[must_use = "cancelled recovery retirement still owns the native runtime"]
    pub fn cancel(self) -> ExtensionRuntimeRecoveryOwner {
        ExtensionRuntimeRecoveryOwner { core: self.core }
    }

    /// Attempts retirement before the exact absolute `deadline`.
    ///
    /// An already-expired deadline performs no callback and restores definite
    /// ownership. Expiration never proves native absence.
    pub fn settle_until(self, deadline: Instant) -> ExtensionRuntimeRecoveryRetirementSettlement {
        if deadline <= Instant::now() {
            return ExtensionRuntimeRecoveryRetirementSettlement::Retained {
                owner: ExtensionRuntimeRecoveryOwner { core: self.core },
                failure: ExtensionRuntimeFailure::TimedOut,
            };
        }
        let mut core = self.core;
        match core.ownership.retire_until(deadline) {
            ExtensionRuntimeRetirementDisposition::Retired(absence) => {
                if RecoveryEvidenceObservation::new(core.expectation).accepts_absence(absence)
                    && core.ownership.accepts_absence_evidence(absence)
                {
                    ExtensionRuntimeRecoveryRetirementSettlement::Absent(absence)
                } else {
                    ExtensionRuntimeRecoveryRetirementSettlement::OwnershipUncertain {
                        request: ExtensionRuntimeRecoveryRequest {
                            core: core.into_uncertain(),
                        },
                        failure: ExtensionRuntimeFailure::Internal,
                    }
                }
            }
            ExtensionRuntimeRetirementDisposition::Retained(failure) => {
                ExtensionRuntimeRecoveryRetirementSettlement::Retained {
                    owner: ExtensionRuntimeRecoveryOwner { core },
                    failure,
                }
            }
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain { failure, evidence } => {
                let mut core = core.into_uncertain();
                let failure = if core.merge_evidence(evidence) {
                    ExtensionRuntimeFailure::Internal
                } else {
                    failure
                };
                ExtensionRuntimeRecoveryRetirementSettlement::OwnershipUncertain {
                    request: ExtensionRuntimeRecoveryRequest { core },
                    failure,
                }
            }
        }
    }
}

impl fmt::Debug for ExtensionRuntimeRecoveryRetirementRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeRecoveryRetirementRequest")
            .field("ownership_evidence", &self.core.evidence)
            .field("retained_bytes", &self.retained_bytes())
            .field("ownership", &"[redacted]")
            .finish()
    }
}

/// Cleanup-only result of a persisted-owner retirement attempt.
#[must_use]
pub enum ExtensionRuntimeRecoveryRetirementSettlement {
    /// Native ownership is definitely absent.
    Absent(ExtensionRuntimeAbsenceEvidence),
    /// Native ownership definitely remains present.
    Retained {
        /// Restored cleanup-only definite ownership.
        owner: ExtensionRuntimeRecoveryOwner,
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
    },
    /// Native ownership became indeterminate.
    OwnershipUncertain {
        /// The cleanup-only reconciliation request.
        request: ExtensionRuntimeRecoveryRequest,
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
    },
}

impl ExtensionRuntimeRecoveryRetirementSettlement {
    /// Returns the exact conservative retained-memory charge of this value.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        match self {
            Self::Absent(_) => std::mem::size_of::<Self>(),
            Self::Retained { owner, .. } => owner.retained_bytes(),
            Self::OwnershipUncertain { request, .. } => request.retained_bytes(),
        }
    }
}

impl fmt::Debug for ExtensionRuntimeRecoveryRetirementSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent(_) => formatter.write_str("Absent([redacted])"),
            Self::Retained { failure, .. } => formatter
                .debug_struct("Retained")
                .field("owner", &"[redacted]")
                .field("failure", failure)
                .finish(),
            Self::OwnershipUncertain { failure, .. } => formatter
                .debug_struct("OwnershipUncertain")
                .field("request", &"[redacted]")
                .field("failure", failure)
                .finish(),
        }
    }
}

fn recovery_owner_wrapper_bytes() -> usize {
    [
        std::mem::size_of::<ExtensionRuntimeRecoveryRequest>(),
        std::mem::size_of::<ExtensionRuntimeRecoveryOwner>(),
        std::mem::size_of::<ExtensionRuntimeRecoveryRetirementRequest>(),
        std::mem::size_of::<ExtensionRuntimeRecoverySettlement>(),
        std::mem::size_of::<ExtensionRuntimeRecoveryRetirementSettlement>(),
    ]
    .into_iter()
    .max()
    .expect("recovery owner layout set is nonempty")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::{Cursor, Read};
    use std::num::NonZeroU64;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::{
        ExtensionPackageAccessPort, ExtensionRuntimeHostRegistryGeneration,
        ExtensionRuntimeNativeRootLeasePort, ExtensionRuntimeNativeRootVisitor,
        ExtensionRuntimeResourceBinding, ExtensionRuntimeResourcePlan,
    };

    struct IdentityProvider {
        identity: u64,
        recovered: Arc<Mutex<Vec<u64>>>,
        native_root_lease: Option<Box<IdentityNativeRootLease>>,
    }

    struct IdentityNativeRootLease;

    impl ExtensionRuntimeNativeRootLeasePort for IdentityNativeRootLease {
        fn visit_native_root(
            &mut self,
            visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
        ) -> Result<(), ExtensionPackageAccessError> {
            #[cfg(unix)]
            let root = Path::new("/private/var/zephium/extensions/package");
            #[cfg(windows)]
            let root = Path::new(r"C:\Zephium\extensions\package");
            let _ = visitor.visit(root);
            Ok(())
        }
    }

    impl Drop for IdentityProvider {
        fn drop(&mut self) {
            self.recovered.lock().expect("lock").push(self.identity);
        }
    }

    impl ExtensionPackageAccessPort for IdentityProvider {
        fn retained_bytes(&self) -> usize {
            0
        }

        fn visit_resource(
            &mut self,
            resource: ExtensionRuntimeResource,
            visitor: &mut dyn ExtensionRuntimeResourceVisitor,
        ) -> Result<(), ExtensionPackageAccessError> {
            let bytes = vec![0_u8; resource.declared_bytes() as usize];
            let _ = visitor.visit(&mut Cursor::new(bytes));
            Ok(())
        }

        fn take_native_root_lease(
            &mut self,
            target: ExtensionRuntimeTarget,
        ) -> Result<Box<dyn ExtensionRuntimeNativeRootLeasePort>, ExtensionPackageAccessError>
        {
            if target != ExtensionRuntimeTarget::NativeWebExtension {
                return Err(ExtensionPackageAccessError::NativeRootUnavailable);
            }
            self.native_root_lease
                .take()
                .map(|lease| lease as Box<dyn ExtensionRuntimeNativeRootLeasePort>)
                .ok_or(ExtensionPackageAccessError::NativeRootUnavailable)
        }
    }

    struct ScriptedLifecycle {
        expected_target: ExtensionRuntimeTarget,
        absence_evidence: ExtensionRuntimeAbsenceEvidence,
        activations: VecDeque<ExtensionRuntimeActivationDisposition>,
        retirements: VecDeque<ExtensionRuntimeRetirementDisposition>,
        reconciliations: VecDeque<ExtensionRuntimeOwnershipDisposition>,
        calls: Arc<Mutex<Vec<&'static str>>>,
        retained_bytes: usize,
    }

    type RecoveredIdentities = Arc<Mutex<Vec<u64>>>;
    type LifecycleCalls = Arc<Mutex<Vec<&'static str>>>;
    type TestRequest = (
        ExtensionRuntimeActivationRequest,
        RecoveredIdentities,
        LifecycleCalls,
    );

    impl ExtensionRuntimeOwnershipPort for ScriptedLifecycle {
        fn retained_bytes(&self) -> usize {
            self.retained_bytes
        }

        fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
            evidence == self.absence_evidence
        }

        fn retire_until(&mut self, _deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
            self.calls.lock().expect("lock").push("retire");
            self.retirements.pop_front().expect("retirement scripted")
        }

        fn reconcile_ownership_until(
            &mut self,
            _deadline: Instant,
        ) -> ExtensionRuntimeOwnershipDisposition {
            self.calls.lock().expect("lock").push("reconcile");
            self.reconciliations
                .pop_front()
                .expect("reconciliation scripted")
        }
    }

    impl ExtensionRuntimeLifecyclePort for ScriptedLifecycle {
        fn activate_until(
            &mut self,
            access: &mut ExtensionPackageAccessView<'_>,
            _deadline: Instant,
        ) -> ExtensionRuntimeActivationDisposition {
            assert_eq!(access.target(), self.expected_target);
            self.calls.lock().expect("lock").push("activate");
            self.activations.pop_front().expect("activation scripted")
        }
    }

    struct BudgetLifecycle {
        identity: u64,
        retained_bytes: usize,
        recovered: Arc<Mutex<Vec<u64>>>,
    }

    impl Drop for BudgetLifecycle {
        fn drop(&mut self) {
            self.recovered.lock().expect("lock").push(self.identity);
        }
    }

    impl ExtensionRuntimeOwnershipPort for BudgetLifecycle {
        fn retained_bytes(&self) -> usize {
            self.retained_bytes
        }

        fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
            evidence == test_absence(ExtensionRuntimeTarget::Compatibility)
        }

        fn retire_until(&mut self, _deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
            retired()
        }

        fn reconcile_ownership_until(
            &mut self,
            _deadline: Instant,
        ) -> ExtensionRuntimeOwnershipDisposition {
            ExtensionRuntimeOwnershipDisposition::Owned(
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            )
        }
    }

    impl ExtensionRuntimeLifecyclePort for BudgetLifecycle {
        fn activate_until(
            &mut self,
            _access: &mut ExtensionPackageAccessView<'_>,
            _deadline: Instant,
        ) -> ExtensionRuntimeActivationDisposition {
            ExtensionRuntimeActivationDisposition::Activated(
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            )
        }
    }

    struct RecoveryConstructionProbeOwnership {
        retained_queries: Arc<AtomicUsize>,
        ownership_callbacks: Arc<AtomicUsize>,
    }

    impl ExtensionRuntimeOwnershipPort for RecoveryConstructionProbeOwnership {
        fn retained_bytes(&self) -> usize {
            self.retained_queries.fetch_add(1, Ordering::Relaxed);
            0
        }

        fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
            evidence == test_absence(ExtensionRuntimeTarget::Compatibility)
        }

        fn retire_until(&mut self, _deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
            self.ownership_callbacks.fetch_add(1, Ordering::Relaxed);
            retired()
        }

        fn reconcile_ownership_until(
            &mut self,
            _deadline: Instant,
        ) -> ExtensionRuntimeOwnershipDisposition {
            self.ownership_callbacks.fetch_add(1, Ordering::Relaxed);
            absent()
        }
    }

    struct ScriptedOwnership {
        identity: u64,
        absence_evidence: ExtensionRuntimeAbsenceEvidence,
        retirements: VecDeque<ExtensionRuntimeRetirementDisposition>,
        reconciliations: VecDeque<ExtensionRuntimeOwnershipDisposition>,
        calls: Arc<Mutex<Vec<(&'static str, Instant)>>>,
        dropped: Arc<Mutex<Vec<u64>>>,
        retained_bytes: usize,
    }

    impl Drop for ScriptedOwnership {
        fn drop(&mut self) {
            self.dropped.lock().expect("lock").push(self.identity);
        }
    }

    impl ExtensionRuntimeOwnershipPort for ScriptedOwnership {
        fn retained_bytes(&self) -> usize {
            self.retained_bytes
        }

        fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
            evidence == self.absence_evidence
        }

        fn retire_until(&mut self, deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
            self.calls.lock().expect("lock").push(("retire", deadline));
            self.retirements.pop_front().expect("retirement scripted")
        }

        fn reconcile_ownership_until(
            &mut self,
            deadline: Instant,
        ) -> ExtensionRuntimeOwnershipDisposition {
            self.calls
                .lock()
                .expect("lock")
                .push(("reconcile", deadline));
            self.reconciliations
                .pop_front()
                .expect("reconciliation scripted")
        }
    }

    type OwnershipCalls = Arc<Mutex<Vec<(&'static str, Instant)>>>;

    struct DeadlineLifecycle {
        calls: OwnershipCalls,
    }

    impl ExtensionRuntimeOwnershipPort for DeadlineLifecycle {
        fn retained_bytes(&self) -> usize {
            0
        }

        fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
            evidence == test_absence(ExtensionRuntimeTarget::Compatibility)
        }

        fn retire_until(&mut self, deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
            self.calls.lock().expect("lock").push(("retire", deadline));
            retired()
        }

        fn reconcile_ownership_until(
            &mut self,
            deadline: Instant,
        ) -> ExtensionRuntimeOwnershipDisposition {
            self.calls
                .lock()
                .expect("lock")
                .push(("reconcile", deadline));
            ExtensionRuntimeOwnershipDisposition::Owned(
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            )
        }
    }

    impl ExtensionRuntimeLifecyclePort for DeadlineLifecycle {
        fn activate_until(
            &mut self,
            _access: &mut ExtensionPackageAccessView<'_>,
            deadline: Instant,
        ) -> ExtensionRuntimeActivationDisposition {
            self.calls
                .lock()
                .expect("lock")
                .push(("activate", deadline));
            activation_uncertain(ExtensionRuntimeFailure::TimedOut, None)
        }
    }

    fn scripted_ownership(
        identity: u64,
        retirements: impl IntoIterator<Item = ExtensionRuntimeRetirementDisposition>,
        reconciliations: impl IntoIterator<Item = ExtensionRuntimeOwnershipDisposition>,
        retained_bytes: usize,
    ) -> (
        Box<dyn ExtensionRuntimeOwnershipPort>,
        OwnershipCalls,
        RecoveredIdentities,
    ) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let dropped = Arc::new(Mutex::new(Vec::new()));
        let retirements: VecDeque<_> = retirements.into_iter().collect();
        let reconciliations: VecDeque<_> = reconciliations.into_iter().collect();
        let absence_evidence = retirements
            .iter()
            .find_map(|disposition| match disposition {
                ExtensionRuntimeRetirementDisposition::Retired(absence) => Some(*absence),
                _ => None,
            })
            .or_else(|| {
                reconciliations
                    .iter()
                    .find_map(|disposition| match disposition {
                        ExtensionRuntimeOwnershipDisposition::Absent(absence) => Some(*absence),
                        _ => None,
                    })
            })
            .unwrap_or_else(|| test_absence(ExtensionRuntimeTarget::Compatibility));
        (
            Box::new(ScriptedOwnership {
                identity,
                absence_evidence,
                retirements,
                reconciliations,
                calls: Arc::clone(&calls),
                dropped: Arc::clone(&dropped),
                retained_bytes,
            }),
            calls,
            dropped,
        )
    }

    fn request(
        identity: u64,
        activations: impl IntoIterator<Item = ExtensionRuntimeActivationDisposition>,
        retirements: impl IntoIterator<Item = ExtensionRuntimeRetirementDisposition>,
        reconciliations: impl IntoIterator<Item = ExtensionRuntimeOwnershipDisposition>,
    ) -> TestRequest {
        request_for_target(
            identity,
            ExtensionRuntimeTarget::Compatibility,
            activations,
            retirements,
            reconciliations,
        )
    }

    fn request_for_target(
        identity: u64,
        target: ExtensionRuntimeTarget,
        activations: impl IntoIterator<Item = ExtensionRuntimeActivationDisposition>,
        retirements: impl IntoIterator<Item = ExtensionRuntimeRetirementDisposition>,
        reconciliations: impl IntoIterator<Item = ExtensionRuntimeOwnershipDisposition>,
    ) -> TestRequest {
        let recovered = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let resources = identity_resource_plan(identity);
        let access = ExtensionPackageAccess::from_delegated_provider(
            target,
            resources,
            Box::new(IdentityProvider {
                identity,
                recovered: Arc::clone(&recovered),
                native_root_lease: Some(Box::new(IdentityNativeRootLease)),
            }),
        )
        .expect("access");
        let request = ExtensionRuntimeActivationRequest::try_new(
            access,
            Box::new(ScriptedLifecycle {
                expected_target: target,
                absence_evidence: test_absence(target),
                activations: activations.into_iter().collect(),
                retirements: retirements.into_iter().collect(),
                reconciliations: reconciliations.into_iter().collect(),
                calls: Arc::clone(&calls),
                retained_bytes: 1024,
            }),
        )
        .expect("bounded activation request");
        (request, recovered, calls)
    }

    fn identity_provider_inputs(
        identity: u64,
        recovered: &RecoveredIdentities,
    ) -> (
        ExtensionRuntimeTarget,
        ExtensionRuntimeResourcePlan,
        Box<dyn ExtensionPackageAccessPort>,
    ) {
        (
            ExtensionRuntimeTarget::Compatibility,
            identity_resource_plan(identity),
            Box::new(IdentityProvider {
                identity,
                recovered: Arc::clone(recovered),
                native_root_lease: Some(Box::new(IdentityNativeRootLease)),
            }),
        )
    }

    fn identity_resource_plan(identity: u64) -> ExtensionRuntimeResourcePlan {
        ExtensionRuntimeResourcePlan::try_new(vec![ExtensionRuntimeResourceBinding::try_new(
            "manifest.json",
            1,
            [identity as u8; 32],
        )
        .expect("manifest binding")])
        .expect("resource plan")
    }

    fn identity_access(identity: u64, recovered: &RecoveredIdentities) -> ExtensionPackageAccess {
        let (target, resources, provider) = identity_provider_inputs(identity, recovered);
        ExtensionPackageAccess::from_delegated_provider(target, resources, provider)
            .expect("access")
    }

    fn future_deadline() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    const TEST_OWNERSHIP_EVIDENCE: ExtensionRuntimeOwnershipEvidence =
        ExtensionRuntimeOwnershipEvidence::Compatibility;

    fn test_absence(target: ExtensionRuntimeTarget) -> ExtensionRuntimeAbsenceEvidence {
        let (backend, proof, expected, observed) = match target {
            ExtensionRuntimeTarget::Compatibility => (
                zephium_core::extensions::ExtensionRuntimeBackendTarget::MacosCompatibility,
                crate::ExtensionRuntimeAbsenceProofKind::CompatibilityRegistryAbsentAndQuiescent,
                None,
                None,
            ),
            ExtensionRuntimeTarget::NativeWebExtension => (
                zephium_core::extensions::ExtensionRuntimeBackendTarget::MacosNative,
                crate::ExtensionRuntimeAbsenceProofKind::MacosZeroGrantsAndUnloaded,
                Some(native_owner_id(b'a')),
                Some(native_owner_id(b'a')),
            ),
        };
        ExtensionRuntimeAbsenceEvidence::for_test_lineage(
            NonZeroU64::new(1).expect("nonzero test lineage"),
            backend,
            target,
            ExtensionRuntimeHostRegistryGeneration::new(1).expect("nonzero test generation"),
            NonZeroU64::new(1).expect("nonzero test attempt"),
            proof,
            expected,
            observed,
        )
    }

    fn retired() -> ExtensionRuntimeRetirementDisposition {
        retired_for(ExtensionRuntimeTarget::Compatibility)
    }

    fn retired_for(target: ExtensionRuntimeTarget) -> ExtensionRuntimeRetirementDisposition {
        ExtensionRuntimeRetirementDisposition::Retired(test_absence(target))
    }

    fn absent() -> ExtensionRuntimeOwnershipDisposition {
        absent_for(ExtensionRuntimeTarget::Compatibility)
    }

    fn absent_for(target: ExtensionRuntimeTarget) -> ExtensionRuntimeOwnershipDisposition {
        ExtensionRuntimeOwnershipDisposition::Absent(test_absence(target))
    }

    fn test_recovery_absence(
        expectation: ExtensionRuntimeRecoveryExpectation,
    ) -> ExtensionRuntimeAbsenceEvidence {
        let (backend, target, proof, expected, observed) = match expectation {
            ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                catalog_expected,
                adapter_observed,
            } => (
                zephium_core::extensions::ExtensionRuntimeBackendTarget::MacosNative,
                ExtensionRuntimeTarget::NativeWebExtension,
                crate::ExtensionRuntimeAbsenceProofKind::MacosZeroGrantsAndUnloaded,
                catalog_expected,
                catalog_expected.or(adapter_observed),
            ),
            ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
                catalog_expected,
                adapter_observed,
            } => (
                zephium_core::extensions::ExtensionRuntimeBackendTarget::WindowsNative,
                ExtensionRuntimeTarget::NativeWebExtension,
                crate::ExtensionRuntimeAbsenceProofKind::ActivationNeverEntered,
                catalog_expected,
                adapter_observed,
            ),
            ExtensionRuntimeRecoveryExpectation::Compatibility => (
                zephium_core::extensions::ExtensionRuntimeBackendTarget::MacosCompatibility,
                ExtensionRuntimeTarget::Compatibility,
                crate::ExtensionRuntimeAbsenceProofKind::CompatibilityRegistryAbsentAndQuiescent,
                None,
                None,
            ),
        };
        ExtensionRuntimeAbsenceEvidence::for_test_lineage(
            NonZeroU64::new(1).expect("nonzero test lineage"),
            backend,
            target,
            ExtensionRuntimeHostRegistryGeneration::new(1).expect("nonzero test generation"),
            NonZeroU64::new(1).expect("nonzero test attempt"),
            proof,
            expected,
            observed,
        )
    }

    fn recovery_absent(
        expectation: ExtensionRuntimeRecoveryExpectation,
    ) -> ExtensionRuntimeOwnershipDisposition {
        ExtensionRuntimeOwnershipDisposition::Absent(test_recovery_absence(expectation))
    }

    fn recovery_retired(
        expectation: ExtensionRuntimeRecoveryExpectation,
    ) -> ExtensionRuntimeRetirementDisposition {
        ExtensionRuntimeRetirementDisposition::Retired(test_recovery_absence(expectation))
    }

    fn retryable(failure: ExtensionRuntimeFailure) -> ExtensionRuntimeActivationDisposition {
        ExtensionRuntimeActivationDisposition::Retryable {
            failure,
            absence: test_absence(ExtensionRuntimeTarget::Compatibility),
        }
    }

    fn rejected(failure: ExtensionRuntimeFailure) -> ExtensionRuntimeActivationDisposition {
        ExtensionRuntimeActivationDisposition::Rejected {
            failure,
            absence: test_absence(ExtensionRuntimeTarget::Compatibility),
        }
    }

    const fn activated() -> ExtensionRuntimeActivationDisposition {
        ExtensionRuntimeActivationDisposition::Activated(TEST_OWNERSHIP_EVIDENCE)
    }

    const fn owned() -> ExtensionRuntimeOwnershipDisposition {
        ExtensionRuntimeOwnershipDisposition::Owned(TEST_OWNERSHIP_EVIDENCE)
    }

    const fn activation_uncertain(
        failure: ExtensionRuntimeFailure,
        evidence: Option<ExtensionRuntimeOwnershipEvidence>,
    ) -> ExtensionRuntimeActivationDisposition {
        ExtensionRuntimeActivationDisposition::OwnershipUncertain { failure, evidence }
    }

    const fn retirement_uncertain(
        failure: ExtensionRuntimeFailure,
        evidence: Option<ExtensionRuntimeOwnershipEvidence>,
    ) -> ExtensionRuntimeRetirementDisposition {
        ExtensionRuntimeRetirementDisposition::OwnershipUncertain { failure, evidence }
    }

    const fn still_uncertain(
        failure: ExtensionRuntimeFailure,
        evidence: Option<ExtensionRuntimeOwnershipEvidence>,
    ) -> ExtensionRuntimeOwnershipDisposition {
        ExtensionRuntimeOwnershipDisposition::StillUncertain { failure, evidence }
    }

    fn native_owner_id(byte: u8) -> crate::ExtensionRuntimeNativeOwnerId {
        crate::ExtensionRuntimeNativeOwnerId::from_encoded_bytes(
            [byte; crate::EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES],
        )
        .expect("test native owner id must use the canonical alphabet")
    }

    fn macos_evidence(byte: u8) -> ExtensionRuntimeOwnershipEvidence {
        ExtensionRuntimeOwnershipEvidence::MacosWebExtension(native_owner_id(byte))
    }

    fn windows_evidence(byte: u8) -> ExtensionRuntimeOwnershipEvidence {
        ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(native_owner_id(byte))
    }

    fn recovery_request(
        ownership: Box<dyn ExtensionRuntimeOwnershipPort>,
    ) -> Result<ExtensionRuntimeRecoveryRequest, ExtensionRuntimeRecoveryBuildRefusal> {
        recovery_request_for(
            ownership,
            ExtensionRuntimeRecoveryExpectation::Compatibility,
        )
    }

    fn recovery_request_for(
        ownership: Box<dyn ExtensionRuntimeOwnershipPort>,
        expectation: ExtensionRuntimeRecoveryExpectation,
    ) -> Result<ExtensionRuntimeRecoveryRequest, ExtensionRuntimeRecoveryBuildRefusal> {
        ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership, expectation)
    }

    fn recover_identity_provider(access: ExtensionPackageAccess) -> Box<IdentityProvider> {
        let provider = access.into_provider();
        let payload: Box<dyn std::any::Any + Send> = provider;
        match payload.downcast::<IdentityProvider>() {
            Ok(provider) => provider,
            Err(payload) => {
                drop(payload);
                panic!("lease-backed provider type changed")
            }
        }
    }

    #[test]
    fn owner_accounting_accepts_exact_ceiling_without_double_counting_access() {
        let recovered_access = Arc::new(Mutex::new(Vec::new()));
        let recovered_lifecycle = Arc::new(Mutex::new(Vec::new()));
        let access = identity_access(21, &recovered_access);
        let wrapper_bytes = runtime_owner_wrapper_bytes().expect("owner wrapper accounting");
        let lifecycle_bytes =
            MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES - access.retained_bytes() - wrapper_bytes;
        let request = ExtensionRuntimeActivationRequest::try_new(
            access,
            Box::new(BudgetLifecycle {
                identity: 31,
                retained_bytes: lifecycle_bytes,
                recovered: Arc::clone(&recovered_lifecycle),
            }),
        )
        .expect("exact owner ceiling must be accepted");
        assert_eq!(
            request.retained_bytes(),
            MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        );

        let activation = request.settle_until(future_deadline());
        assert_eq!(
            activation.retained_bytes(),
            MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        );
        let owner = match activation {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(
            owner.retained_bytes(),
            MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        );
        let retirement = owner.into_retirement_request();
        assert_eq!(
            retirement.retained_bytes(),
            MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        );
        let settlement = retirement.settle_until(future_deadline());
        assert!(settlement.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
        let access = match settlement {
            ExtensionRuntimeRetirementSettlement::Retired { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(*recovered_lifecycle.lock().expect("lock"), vec![31]);
        drop(access);
        assert_eq!(*recovered_access.lock().expect("lock"), vec![21]);
    }

    #[test]
    fn owner_accounting_covers_every_persistent_public_state_layout() {
        let charged_layout = std::mem::size_of::<ExtensionPackageAccess>()
            + runtime_owner_wrapper_bytes().expect("owner wrapper accounting");
        for actual in [
            std::mem::size_of::<ExtensionRuntimeActivationRequest>(),
            std::mem::size_of::<ExtensionRuntimeOwner>(),
            std::mem::size_of::<ExtensionRuntimeRetirementRequest>(),
            std::mem::size_of::<ExtensionRuntimeUncertainOwner>(),
            std::mem::size_of::<ExtensionRuntimeActivationSettlement>(),
            std::mem::size_of::<ExtensionRuntimeRetirementSettlement>(),
            std::mem::size_of::<ExtensionRuntimeReconciliationSettlement>(),
        ] {
            assert!(actual <= charged_layout);
        }
    }

    #[test]
    fn recovery_accounting_covers_every_persistent_public_state_layout() {
        let charged_layout = recovery_owner_wrapper_bytes();
        for actual in [
            std::mem::size_of::<ExtensionRuntimeRecoveryRequest>(),
            std::mem::size_of::<ExtensionRuntimeRecoveryOwner>(),
            std::mem::size_of::<ExtensionRuntimeRecoveryRetirementRequest>(),
            std::mem::size_of::<ExtensionRuntimeRecoverySettlement>(),
            std::mem::size_of::<ExtensionRuntimeRecoveryRetirementSettlement>(),
        ] {
            assert!(actual <= charged_layout);
        }
    }

    #[test]
    fn exact_evidence_survives_every_same_process_ownership_state() {
        let (request, recovered, calls) = request(
            70,
            [activated()],
            [
                ExtensionRuntimeRetirementDisposition::Retained(
                    ExtensionRuntimeFailure::BackendUnavailable,
                ),
                retirement_uncertain(
                    ExtensionRuntimeFailure::TimedOut,
                    Some(TEST_OWNERSHIP_EVIDENCE),
                ),
                retired(),
            ],
            [owned()],
        );
        let retained_bytes = request.retained_bytes();
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.ownership_evidence(), TEST_OWNERSHIP_EVIDENCE);
        assert_eq!(owner.retained_bytes(), retained_bytes);

        let retirement = owner.into_retirement_request();
        assert_eq!(retirement.ownership_evidence(), TEST_OWNERSHIP_EVIDENCE);
        let owner = match retirement.settle_until(future_deadline()) {
            ExtensionRuntimeRetirementSettlement::Retained { owner, .. } => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.ownership_evidence(), TEST_OWNERSHIP_EVIDENCE);

        let uncertain = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::OwnershipUncertain { owner, .. } => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(
            uncertain.ownership_evidence(),
            Some(TEST_OWNERSHIP_EVIDENCE)
        );
        assert_eq!(uncertain.retained_bytes(), retained_bytes);

        let owner = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.ownership_evidence(), TEST_OWNERSHIP_EVIDENCE);
        let access = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::Retired { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![70]);
        assert_eq!(
            *calls.lock().expect("lock"),
            vec!["activate", "retire", "retire", "reconcile", "retire"]
        );
    }

    #[test]
    fn activation_target_mismatch_is_conservatively_uncertain() {
        let mismatched_evidence = macos_evidence(b'a');
        let (request, recovered, calls) = request(
            71,
            [ExtensionRuntimeActivationDisposition::Activated(
                mismatched_evidence,
            )],
            [],
            [owned(), absent()],
        );
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.target(), ExtensionRuntimeTarget::Compatibility);
        assert_eq!(owner.ownership_evidence(), None);
        let owner = match owner.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.ownership_evidence(), None);
        let access = match owner.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::Absent { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(
            *calls.lock().expect("lock"),
            vec!["activate", "reconcile", "reconcile"]
        );
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![71]);
    }

    #[test]
    fn known_evidence_mismatch_cannot_reconcile_to_a_different_owner() {
        let exact_evidence = macos_evidence(b'b');
        let different_evidence = windows_evidence(b'b');
        let (request, recovered, _) = request_for_target(
            72,
            ExtensionRuntimeTarget::NativeWebExtension,
            [ExtensionRuntimeActivationDisposition::Activated(
                exact_evidence,
            )],
            [retirement_uncertain(
                ExtensionRuntimeFailure::TimedOut,
                Some(exact_evidence),
            )],
            [
                still_uncertain(ExtensionRuntimeFailure::TimedOut, Some(different_evidence)),
                still_uncertain(
                    ExtensionRuntimeFailure::BackendUnavailable,
                    Some(exact_evidence),
                ),
                ExtensionRuntimeOwnershipDisposition::Owned(exact_evidence),
                absent_for(ExtensionRuntimeTarget::NativeWebExtension),
            ],
        );
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let uncertain = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::OwnershipUncertain { owner, .. } => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let uncertain = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.ownership_evidence(), Some(exact_evidence));
        let uncertain = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.ownership_evidence(), Some(exact_evidence));
        let uncertain = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.ownership_evidence(), Some(exact_evidence));
        let access = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::Absent { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![72]);
    }

    #[test]
    fn identityless_native_recovery_attaches_exact_macos_and_windows_evidence() {
        for (identity, expectation, evidence) in [
            (
                79,
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                    catalog_expected: None,
                    adapter_observed: None,
                },
                macos_evidence(b'j'),
            ),
            (
                80,
                ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
                    catalog_expected: None,
                    adapter_observed: None,
                },
                windows_evidence(b'k'),
            ),
        ] {
            let (ownership, _, dropped) = scripted_ownership(
                identity,
                [],
                [
                    still_uncertain(ExtensionRuntimeFailure::TimedOut, Some(evidence)),
                    ExtensionRuntimeOwnershipDisposition::Owned(evidence),
                ],
                1024,
            );
            let request = recovery_request_for(ownership, expectation)
                .expect("identityless native recovery must fit the owner budget");
            assert_eq!(request.expectation(), expectation);
            assert_eq!(request.ownership_evidence(), None);

            let request = match request.reconcile_until(future_deadline()) {
                ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                    assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                    request
                }
                settlement => panic!("unexpected settlement: {settlement:?}"),
            };
            assert_eq!(request.ownership_evidence(), Some(evidence));

            let owner = match request.reconcile_until(future_deadline()) {
                ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
                settlement => panic!("unexpected settlement: {settlement:?}"),
            };
            assert_eq!(owner.ownership_evidence(), evidence);
            drop(owner);
            assert_eq!(*dropped.lock().expect("lock"), vec![identity]);
        }
    }

    #[test]
    fn catalog_expected_identity_is_not_evidence_and_survives_retirement_uncertainty() {
        let evidence = macos_evidence(b'f');
        let expected_id = match evidence {
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(identity) => identity,
            _ => unreachable!("test evidence is macOS"),
        };
        let expectation = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: Some(expected_id),
            adapter_observed: None,
        };
        let (ownership, _, dropped) = scripted_ownership(
            86,
            [retirement_uncertain(
                ExtensionRuntimeFailure::TimedOut,
                Some(evidence),
            )],
            [
                ExtensionRuntimeOwnershipDisposition::Owned(evidence),
                ExtensionRuntimeOwnershipDisposition::Owned(evidence),
            ],
            1024,
        );
        let request = recovery_request_for(ownership, expectation)
            .expect("catalog-bound recovery must fit the owner budget");
        assert_eq!(request.expectation(), expectation);
        assert_eq!(request.ownership_evidence(), None);

        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let request = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRecoveryRetirementSettlement::OwnershipUncertain {
                request,
                failure,
            } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.expectation(), expectation);
        assert_eq!(request.ownership_evidence(), Some(evidence));
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(owner);
        assert_eq!(*dropped.lock().expect("lock"), vec![86]);
    }

    #[test]
    fn conflicting_same_backend_observation_is_retained_but_never_authorizes_ownership() {
        let expected = macos_evidence(b'i');
        let observed = macos_evidence(b'j');
        let expected_id = match expected {
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(identity) => identity,
            _ => unreachable!("test evidence is macOS"),
        };
        let expectation = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: Some(expected_id),
            adapter_observed: None,
        };
        let (ownership, _, dropped) = scripted_ownership(
            88,
            [],
            [
                still_uncertain(ExtensionRuntimeFailure::TimedOut, Some(observed)),
                ExtensionRuntimeOwnershipDisposition::Owned(expected),
                ExtensionRuntimeOwnershipDisposition::Owned(observed),
                recovery_absent(expectation),
            ],
            1024,
        );
        let request = recovery_request_for(ownership, expectation)
            .expect("catalog-bound recovery must fit the owner budget");
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("conflicting evidence must stay uncertain: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(observed));

        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("expected evidence cannot heal the conflict: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(observed));
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("observed evidence cannot heal the conflict: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(observed));
        assert!(matches!(
            request.reconcile_until(future_deadline()),
            ExtensionRuntimeRecoverySettlement::Absent(_)
        ));
        assert_eq!(*dropped.lock().expect("lock"), vec![88]);
    }

    #[test]
    fn persisted_expected_observed_mismatch_never_settles_owned_or_absent() {
        let expected = macos_evidence(b'g');
        let observed = macos_evidence(b'h');
        let expected_id = match expected {
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(identity) => identity,
            _ => unreachable!("test evidence is macOS"),
        };
        let observed_id = match observed {
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(identity) => identity,
            _ => unreachable!("test evidence is macOS"),
        };
        let expectation = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: Some(expected_id),
            adapter_observed: Some(observed_id),
        };
        let (ownership, _, dropped) = scripted_ownership(
            87,
            [],
            [
                ExtensionRuntimeOwnershipDisposition::Owned(observed),
                ExtensionRuntimeOwnershipDisposition::Owned(expected),
                recovery_absent(expectation),
            ],
            1024,
        );
        let request = recovery_request_for(ownership, expectation)
            .expect("mismatched durable recovery remains cleanup-capable");
        assert!(request.expectation().has_identity_conflict());
        assert_eq!(request.ownership_evidence(), Some(observed));

        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("mismatch must not settle positively: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(observed));
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("later matching evidence must not heal: {settlement:?}"),
        };
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("mismatched identities cannot authorize absence: {settlement:?}"),
        };
        drop(request);
        assert_eq!(*dropped.lock().expect("lock"), vec![87]);
    }

    #[test]
    fn identityless_recovery_wrong_backend_poisons_and_cannot_be_healed() {
        let matching = macos_evidence(b'l');
        let conflicting = windows_evidence(b'l');
        let expectation = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: None,
            adapter_observed: None,
        };
        let (ownership, _, dropped) = scripted_ownership(
            81,
            [],
            [
                still_uncertain(
                    ExtensionRuntimeFailure::BackendUnavailable,
                    Some(conflicting),
                ),
                ExtensionRuntimeOwnershipDisposition::Owned(matching),
                still_uncertain(ExtensionRuntimeFailure::TimedOut, Some(matching)),
                recovery_absent(expectation),
            ],
            1024,
        );
        let request = recovery_request_for(ownership, expectation)
            .expect("identityless native recovery must fit the owner budget");
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), None);
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), None);
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("identityless recovery cannot authorize absence: {settlement:?}"),
        };
        drop(request);
        assert_eq!(*dropped.lock().expect("lock"), vec![81]);
    }

    #[test]
    fn attached_native_identifier_conflict_without_durable_identity_stays_uncertain() {
        let first = macos_evidence(b'm');
        let conflicting = macos_evidence(b'n');
        let expectation = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: None,
            adapter_observed: None,
        };
        let (ownership, _, dropped) = scripted_ownership(
            82,
            [],
            [
                still_uncertain(ExtensionRuntimeFailure::TimedOut, Some(first)),
                ExtensionRuntimeOwnershipDisposition::Owned(conflicting),
                ExtensionRuntimeOwnershipDisposition::Owned(first),
                recovery_absent(expectation),
            ],
            1024,
        );
        let request = recovery_request_for(ownership, expectation)
            .expect("identityless native recovery must fit the owner budget");
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(first));
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(first));
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(first));
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("identity conflict cannot authorize absence: {settlement:?}"),
        };
        drop(request);
        assert_eq!(*dropped.lock().expect("lock"), vec![82]);
    }

    #[test]
    fn compatibility_recovery_is_exact_from_construction() {
        let (ownership, _, dropped) = scripted_ownership(
            83,
            [retired()],
            [
                still_uncertain(ExtensionRuntimeFailure::BackendUnavailable, None),
                owned(),
            ],
            1024,
        );
        let request = recovery_request(ownership).expect("bounded compatibility recovery");
        assert_eq!(
            request.ownership_evidence(),
            Some(ExtensionRuntimeOwnershipEvidence::Compatibility)
        );
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(
            owner.ownership_evidence(),
            ExtensionRuntimeOwnershipEvidence::Compatibility
        );
        assert!(matches!(
            owner
                .into_retirement_request()
                .settle_until(future_deadline()),
            ExtensionRuntimeRecoveryRetirementSettlement::Absent(_)
        ));
        assert_eq!(*dropped.lock().expect("lock"), vec![83]);
    }

    #[test]
    fn recovery_none_and_expired_deadlines_preserve_identityless_uncertainty() {
        let expectation = ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
            catalog_expected: None,
            adapter_observed: None,
        };
        let (ownership, calls, dropped) = scripted_ownership(
            84,
            [],
            [still_uncertain(
                ExtensionRuntimeFailure::BackendUnavailable,
                None,
            )],
            1024,
        );
        let request = recovery_request_for(ownership, expectation)
            .expect("identityless native recovery must fit the owner budget");
        let request = match request.reconcile_until(Instant::now()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(calls.lock().expect("lock").is_empty());
        assert_eq!(request.expectation(), expectation);
        assert_eq!(request.ownership_evidence(), None);
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), None);
        drop(request);
        assert_eq!(calls.lock().expect("lock").len(), 1);
        assert_eq!(*dropped.lock().expect("lock"), vec![84]);
    }

    #[test]
    fn attached_native_evidence_remains_exact_through_recovery_retirement() {
        let evidence = macos_evidence(b'o');
        let ExtensionRuntimeOwnershipEvidence::MacosWebExtension(native_id) = evidence else {
            unreachable!("test evidence is macOS")
        };
        let expectation = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: Some(native_id),
            adapter_observed: Some(native_id),
        };
        let (ownership, calls, dropped) = scripted_ownership(
            85,
            [
                ExtensionRuntimeRetirementDisposition::Retained(
                    ExtensionRuntimeFailure::BackendUnavailable,
                ),
                retirement_uncertain(ExtensionRuntimeFailure::TimedOut, None),
                recovery_retired(expectation),
            ],
            [
                ExtensionRuntimeOwnershipDisposition::Owned(evidence),
                ExtensionRuntimeOwnershipDisposition::Owned(evidence),
            ],
            1024,
        );
        let request = recovery_request_for(ownership, expectation)
            .expect("identityless native recovery must fit the owner budget");
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.ownership_evidence(), evidence);
        let owner = match owner.into_retirement_request().settle_until(Instant::now()) {
            ExtensionRuntimeRecoveryRetirementSettlement::Retained { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(calls.lock().expect("lock").len(), 1);
        let owner = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRecoveryRetirementSettlement::Retained { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.ownership_evidence(), evidence);
        let request = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRecoveryRetirementSettlement::OwnershipUncertain {
                request,
                failure,
            } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(
            request.expectation(),
            ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                catalog_expected: Some(native_id),
                adapter_observed: Some(native_id),
            }
        );
        assert_eq!(request.ownership_evidence(), Some(evidence));
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.ownership_evidence(), evidence);
        assert!(matches!(
            owner
                .into_retirement_request()
                .settle_until(future_deadline()),
            ExtensionRuntimeRecoveryRetirementSettlement::Absent(_)
        ));
        assert_eq!(calls.lock().expect("lock").len(), 5);
        assert_eq!(*dropped.lock().expect("lock"), vec![85]);
    }

    #[test]
    fn recovery_evidence_conflict_is_irreversible_until_absence() {
        let expected = macos_evidence(b'c');
        let unexpected = macos_evidence(b'd');
        let ExtensionRuntimeOwnershipEvidence::MacosWebExtension(expected_id) = expected else {
            unreachable!("test evidence is macOS")
        };
        let expectation = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: Some(expected_id),
            adapter_observed: Some(expected_id),
        };
        let (ownership, _, dropped) = scripted_ownership(
            73,
            [],
            [
                ExtensionRuntimeOwnershipDisposition::Owned(unexpected),
                still_uncertain(ExtensionRuntimeFailure::BackendUnavailable, Some(expected)),
                ExtensionRuntimeOwnershipDisposition::Owned(expected),
                recovery_absent(expectation),
            ],
            1024,
        );
        let request =
            ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership, expectation)
                .expect("bounded recovery request");
        assert_eq!(request.ownership_evidence(), Some(expected));
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(expected));
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(expected));
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(expected));
        let settlement = request.reconcile_until(future_deadline());
        assert!(matches!(
            settlement,
            ExtensionRuntimeRecoverySettlement::Absent(_)
        ));
        assert_eq!(*dropped.lock().expect("lock"), vec![73]);
    }

    #[test]
    fn recovery_retains_idempotent_evidence_through_every_state() {
        let expected = macos_evidence(b'e');
        let (ownership, _, dropped) = scripted_ownership(
            74,
            [
                ExtensionRuntimeRetirementDisposition::Retained(
                    ExtensionRuntimeFailure::BackendUnavailable,
                ),
                retirement_uncertain(ExtensionRuntimeFailure::TimedOut, Some(expected)),
            ],
            [
                still_uncertain(ExtensionRuntimeFailure::BackendUnavailable, Some(expected)),
                ExtensionRuntimeOwnershipDisposition::Owned(expected),
                ExtensionRuntimeOwnershipDisposition::Owned(expected),
            ],
            1024,
        );
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(
            ownership,
            ExtensionRuntimeRecoveryExpectation::from_exact_evidence(expected),
        )
        .expect("bounded recovery request");
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(expected));
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.ownership_evidence(), expected);
        let retirement = owner.into_retirement_request();
        assert_eq!(retirement.ownership_evidence(), expected);
        let owner = match retirement.settle_until(future_deadline()) {
            ExtensionRuntimeRecoveryRetirementSettlement::Retained { owner, .. } => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.ownership_evidence(), expected);
        let request = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRecoveryRetirementSettlement::OwnershipUncertain {
                request, ..
            } => request,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.ownership_evidence(), Some(expected));
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.ownership_evidence(), expected);
        drop(owner);
        assert_eq!(*dropped.lock().expect("lock"), vec![74]);
    }

    #[test]
    fn uncertain_results_attach_compatible_evidence_monotonically() {
        let evidence = macos_evidence(b'f');
        let (request, recovered, _) = request_for_target(
            75,
            ExtensionRuntimeTarget::NativeWebExtension,
            [activation_uncertain(
                ExtensionRuntimeFailure::TimedOut,
                None,
            )],
            [retired_for(ExtensionRuntimeTarget::NativeWebExtension)],
            [
                still_uncertain(ExtensionRuntimeFailure::BackendUnavailable, Some(evidence)),
                still_uncertain(ExtensionRuntimeFailure::TimedOut, None),
                still_uncertain(ExtensionRuntimeFailure::BackendUnavailable, Some(evidence)),
                ExtensionRuntimeOwnershipDisposition::Owned(evidence),
            ],
        );
        let uncertain = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.ownership_evidence(), None);
        let uncertain = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.ownership_evidence(), Some(evidence));
        let uncertain = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.ownership_evidence(), Some(evidence));
        let uncertain = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let owner = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.ownership_evidence(), evidence);
        let access = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::Retired { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![75]);
    }

    #[test]
    fn activation_uncertainty_can_authenticate_owner_evidence_early() {
        let evidence = macos_evidence(b'g');
        let (request, recovered, _) = request_for_target(
            76,
            ExtensionRuntimeTarget::NativeWebExtension,
            [activation_uncertain(
                ExtensionRuntimeFailure::TimedOut,
                Some(evidence),
            )],
            [retired_for(ExtensionRuntimeTarget::NativeWebExtension)],
            [ExtensionRuntimeOwnershipDisposition::Owned(evidence)],
        );
        let uncertain = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.ownership_evidence(), Some(evidence));
        let owner = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let access = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::Retired { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![76]);
    }

    #[test]
    fn retirement_evidence_conflict_is_irreversible_until_absence() {
        let expected = macos_evidence(b'h');
        let unexpected = windows_evidence(b'h');
        let (request, recovered, _) = request_for_target(
            77,
            ExtensionRuntimeTarget::NativeWebExtension,
            [ExtensionRuntimeActivationDisposition::Activated(expected)],
            [retirement_uncertain(
                ExtensionRuntimeFailure::TimedOut,
                Some(unexpected),
            )],
            [
                ExtensionRuntimeOwnershipDisposition::Owned(expected),
                absent_for(ExtensionRuntimeTarget::NativeWebExtension),
            ],
        );
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let uncertain = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::OwnershipUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.ownership_evidence(), Some(expected));
        let uncertain = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let access = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::Absent { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![77]);
    }

    #[test]
    fn recovery_retirement_conflict_cannot_be_healed_by_expected_evidence() {
        let expected = macos_evidence(b'i');
        let unexpected = windows_evidence(b'i');
        let ExtensionRuntimeOwnershipEvidence::MacosWebExtension(expected_id) = expected else {
            unreachable!("test evidence is macOS")
        };
        let expectation = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: Some(expected_id),
            adapter_observed: Some(expected_id),
        };
        let (ownership, _, dropped) = scripted_ownership(
            78,
            [retirement_uncertain(
                ExtensionRuntimeFailure::TimedOut,
                Some(unexpected),
            )],
            [
                ExtensionRuntimeOwnershipDisposition::Owned(expected),
                ExtensionRuntimeOwnershipDisposition::Owned(expected),
                recovery_absent(expectation),
            ],
            1024,
        );
        let request =
            ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership, expectation)
                .expect("bounded recovery request");
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let request = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRecoveryRetirementSettlement::OwnershipUncertain {
                request,
                failure,
            } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::Internal);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(matches!(
            request.reconcile_until(future_deadline()),
            ExtensionRuntimeRecoverySettlement::Absent(_)
        ));
        assert_eq!(*dropped.lock().expect("lock"), vec![78]);
    }

    #[test]
    fn definitely_absent_activation_requests_can_be_cancelled_without_native_work() {
        let (request, recovered, calls) = self::request(41, [activated()], [], []);
        let (access, lifecycle) = request.cancel();
        assert!(calls.lock().expect("lock").is_empty());
        let provider = recover_identity_provider(access);
        assert_eq!(provider.identity, 41);
        drop(lifecycle);
        drop(provider);
        assert_eq!(*recovered.lock().expect("lock"), vec![41]);

        let (request, recovered, calls) = self::request(
            42,
            [retryable(ExtensionRuntimeFailure::BackendUnavailable)],
            [],
            [],
        );
        let request = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Retryable { request, .. } => request,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let (access, lifecycle) = request.cancel();
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);
        let provider = recover_identity_provider(access);
        assert_eq!(provider.identity, 42);
        drop(lifecycle);
        drop(provider);
        assert_eq!(*recovered.lock().expect("lock"), vec![42]);
    }

    #[test]
    fn retirement_cancellation_restores_the_exact_owner_without_native_work() {
        let (request, recovered, calls) = request(43, [activated()], [retired()], []);
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let owner = owner.into_retirement_request().cancel();
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);
        let access = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::Retired { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let provider = recover_identity_provider(access);
        assert_eq!(provider.identity, 43);
        drop(provider);
        assert_eq!(*recovered.lock().expect("lock"), vec![43]);
        assert_eq!(*calls.lock().expect("lock"), vec!["activate", "retire"]);
    }

    #[test]
    fn same_process_lifecycle_forwards_every_absolute_deadline_unchanged() {
        let recovered = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let request = ExtensionRuntimeActivationRequest::try_new(
            identity_access(58, &recovered),
            Box::new(DeadlineLifecycle {
                calls: Arc::clone(&calls),
            }),
        )
        .expect("bounded activation request");
        let base = Instant::now() + Duration::from_secs(60);
        let activation_deadline = base;
        let reconciliation_deadline = base + Duration::from_secs(1);
        let retirement_deadline = base + Duration::from_secs(2);

        let uncertain = match request.settle_until(activation_deadline) {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let owner = match uncertain.reconcile_until(reconciliation_deadline) {
            ExtensionRuntimeReconciliationSettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let access = match owner
            .into_retirement_request()
            .settle_until(retirement_deadline)
        {
            ExtensionRuntimeRetirementSettlement::Retired { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(
            *calls.lock().expect("lock"),
            vec![
                ("activate", activation_deadline),
                ("reconcile", reconciliation_deadline),
                ("retire", retirement_deadline),
            ]
        );
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![58]);
    }

    #[test]
    fn expired_deadlines_make_no_callback_and_preserve_the_known_state() {
        let (request, recovered, calls) = self::request(59, [activated()], [], []);
        let request = match request.settle_until(Instant::now()) {
            ExtensionRuntimeActivationSettlement::Retryable {
                request, failure, ..
            } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(calls.lock().expect("lock").is_empty());
        let (access, lifecycle) = request.cancel();
        drop(lifecycle);
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![59]);

        let (request, recovered, calls) = self::request(60, [activated()], [retired()], []);
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let owner = match owner.into_retirement_request().settle_until(Instant::now()) {
            ExtensionRuntimeRetirementSettlement::Retained { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);
        drop(owner);
        assert_eq!(*recovered.lock().expect("lock"), vec![60]);

        let (request, recovered, calls) = self::request(
            61,
            [activation_uncertain(
                ExtensionRuntimeFailure::TimedOut,
                None,
            )],
            [],
            [absent()],
        );
        let uncertain = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, .. } => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let uncertain = match uncertain.reconcile_until(Instant::now()) {
            ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);
        drop(uncertain);
        assert_eq!(*recovered.lock().expect("lock"), vec![61]);

        let (ownership, calls, dropped) = scripted_ownership(62, [], [absent()], 1024);
        let request = recovery_request(ownership).expect("bounded recovery request");
        let request = match request.reconcile_until(Instant::now()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(calls.lock().expect("lock").is_empty());
        drop(request);
        assert_eq!(*dropped.lock().expect("lock"), vec![62]);

        let (ownership, calls, dropped) = scripted_ownership(63, [retired()], [owned()], 1024);
        let request = recovery_request(ownership).expect("bounded recovery request");
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let owner = match owner.into_retirement_request().settle_until(Instant::now()) {
            ExtensionRuntimeRecoveryRetirementSettlement::Retained { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(calls.lock().expect("lock").len(), 1);
        assert_eq!(calls.lock().expect("lock")[0].0, "reconcile");
        drop(owner);
        assert_eq!(*dropped.lock().expect("lock"), vec![63]);
    }

    #[test]
    fn owner_accounting_refusal_returns_both_inputs_unchanged() {
        for (case, lifecycle_bytes, expected) in [
            (
                0_u64,
                MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
                ExtensionRuntimeActivationBuildError::RetainedBytesExceeded,
            ),
            (
                1_u64,
                usize::MAX,
                ExtensionRuntimeActivationBuildError::RetainedBytesOverflow,
            ),
        ] {
            let recovered_access = Arc::new(Mutex::new(Vec::new()));
            let recovered_lifecycle = Arc::new(Mutex::new(Vec::new()));
            let access = identity_access(22 + case, &recovered_access);
            let refusal = ExtensionRuntimeActivationRequest::try_new(
                access,
                Box::new(BudgetLifecycle {
                    identity: 32 + case,
                    retained_bytes: lifecycle_bytes,
                    recovered: Arc::clone(&recovered_lifecycle),
                }),
            )
            .expect_err("owner budget must fail closed");
            assert_eq!(refusal.reason(), expected);
            assert!(format!("{refusal:?}").contains("[redacted]"));
            assert!(recovered_access.lock().expect("lock").is_empty());
            assert!(recovered_lifecycle.lock().expect("lock").is_empty());

            let (access, lifecycle) = refusal.into_parts();
            assert!(recovered_access.lock().expect("lock").is_empty());
            assert!(recovered_lifecycle.lock().expect("lock").is_empty());
            drop(access);
            drop(lifecycle);
            assert_eq!(*recovered_access.lock().expect("lock"), vec![22 + case]);
            assert_eq!(*recovered_lifecycle.lock().expect("lock"), vec![32 + case]);
        }
    }

    #[test]
    fn rejection_returns_the_same_access_only_after_definite_absence() {
        let (request, recovered, calls) = request(
            11,
            [rejected(ExtensionRuntimeFailure::PackageRejected)],
            [],
            [],
        );

        let access = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Rejected {
                access, failure, ..
            } => {
                assert_eq!(failure, ExtensionRuntimeFailure::PackageRejected);
                access
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(recovered.lock().expect("lock").is_empty());
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![11]);
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);
    }

    #[test]
    fn retryable_activation_preserves_the_same_request() {
        let (request, recovered, calls) = request(
            12,
            [
                retryable(ExtensionRuntimeFailure::BackendUnavailable),
                activated(),
            ],
            [retired()],
            [],
        );

        let request = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Retryable {
                request, failure, ..
            } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(recovered.lock().expect("lock").is_empty());
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let access = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::Retired { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(recovered.lock().expect("lock").is_empty());
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![12]);
        assert_eq!(
            *calls.lock().expect("lock"),
            vec!["activate", "activate", "retire"]
        );
    }

    #[test]
    fn retained_retirement_restores_definite_owner() {
        let (request, recovered, calls) = request(
            13,
            [activated()],
            [
                ExtensionRuntimeRetirementDisposition::Retained(
                    ExtensionRuntimeFailure::BackendUnavailable,
                ),
                retired(),
            ],
            [],
        );
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let owner = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::Retained { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(recovered.lock().expect("lock").is_empty());
        let access = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::Retired { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![13]);
        assert_eq!(
            *calls.lock().expect("lock"),
            vec!["activate", "retire", "retire"]
        );
    }

    #[test]
    fn activation_uncertainty_retains_access_until_absence() {
        let (request, recovered, calls) = request(
            14,
            [activation_uncertain(
                ExtensionRuntimeFailure::TimedOut,
                None,
            )],
            [],
            [
                still_uncertain(ExtensionRuntimeFailure::TimedOut, None),
                absent(),
            ],
        );
        let retained_bytes = request.retained_bytes();
        let uncertain = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.retained_bytes(), retained_bytes);
        assert!(recovered.lock().expect("lock").is_empty());
        let uncertain = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.retained_bytes(), retained_bytes);
        assert!(recovered.lock().expect("lock").is_empty());
        let access = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::Absent { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(recovered.lock().expect("lock").is_empty());
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![14]);
        assert_eq!(
            *calls.lock().expect("lock"),
            vec!["activate", "reconcile", "reconcile"]
        );
    }

    #[test]
    fn persisted_recovery_covers_every_state_and_forwards_exact_deadlines() {
        let (ownership, calls, dropped) = scripted_ownership(
            51,
            [
                ExtensionRuntimeRetirementDisposition::Retained(
                    ExtensionRuntimeFailure::BackendUnavailable,
                ),
                retirement_uncertain(ExtensionRuntimeFailure::TimedOut, None),
            ],
            [
                still_uncertain(ExtensionRuntimeFailure::BackendUnavailable, None),
                owned(),
                absent(),
            ],
            1024,
        );
        let request =
            recovery_request(ownership).expect("bounded persisted recovery must be constructible");
        let retained_bytes = request.retained_bytes();
        assert!(calls.lock().expect("lock").is_empty());
        assert!(dropped.lock().expect("lock").is_empty());

        let base = Instant::now() + Duration::from_secs(60);
        let reconciliation_one = base;
        let reconciliation_two = base + Duration::from_secs(1);
        let retirement_one = base + Duration::from_secs(2);
        let retirement_two = base + Duration::from_secs(3);
        let reconciliation_three = base + Duration::from_secs(4);

        let request = match request.reconcile_until(reconciliation_one) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(request.retained_bytes(), retained_bytes);
        let owner = match request.reconcile_until(reconciliation_two) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(owner.retained_bytes(), retained_bytes);
        let owner = match owner.into_retirement_request().settle_until(retirement_one) {
            ExtensionRuntimeRecoveryRetirementSettlement::Retained { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let request = match owner.into_retirement_request().settle_until(retirement_two) {
            ExtensionRuntimeRecoveryRetirementSettlement::OwnershipUncertain {
                request,
                failure,
            } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let settlement = request.reconcile_until(reconciliation_three);
        assert!(settlement.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
        assert!(matches!(
            settlement,
            ExtensionRuntimeRecoverySettlement::Absent(_)
        ));

        assert_eq!(
            *calls.lock().expect("lock"),
            vec![
                ("reconcile", reconciliation_one),
                ("reconcile", reconciliation_two),
                ("retire", retirement_one),
                ("retire", retirement_two),
                ("reconcile", reconciliation_three),
            ]
        );
        assert_eq!(*dropped.lock().expect("lock"), vec![51]);
    }

    #[test]
    fn persisted_recovery_construction_only_queries_accounting() {
        let retained_queries = Arc::new(AtomicUsize::new(0));
        let ownership_callbacks = Arc::new(AtomicUsize::new(0));
        let request = recovery_request(Box::new(RecoveryConstructionProbeOwnership {
            retained_queries: Arc::clone(&retained_queries),
            ownership_callbacks: Arc::clone(&ownership_callbacks),
        }))
        .expect("bounded persisted recovery must be constructible");
        assert_eq!(retained_queries.load(Ordering::Relaxed), 1);
        assert_eq!(ownership_callbacks.load(Ordering::Relaxed), 0);
        drop(request);
        assert_eq!(ownership_callbacks.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn persisted_recovery_accounting_accepts_the_ceiling_and_refuses_excess() {
        let wrapper_bytes = recovery_owner_wrapper_bytes();
        let exact_proxy_bytes = MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
            .checked_sub(wrapper_bytes)
            .expect("recovery wrapper must fit the owner ceiling");
        let (ownership, calls, dropped) =
            scripted_ownership(52, [retired()], [owned()], exact_proxy_bytes);
        let request =
            recovery_request(ownership).expect("the exact recovery-owner ceiling must be accepted");
        assert_eq!(
            request.retained_bytes(),
            MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        );
        let settlement = request.reconcile_until(future_deadline());
        assert_eq!(
            settlement.retained_bytes(),
            MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        );
        let owner = match settlement {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let retirement = owner.into_retirement_request();
        assert_eq!(
            retirement.retained_bytes(),
            MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        );
        let settlement = retirement.settle_until(future_deadline());
        assert!(settlement.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
        assert!(matches!(
            settlement,
            ExtensionRuntimeRecoveryRetirementSettlement::Absent(_)
        ));
        assert_eq!(calls.lock().expect("lock").len(), 2);
        assert_eq!(*dropped.lock().expect("lock"), vec![52]);

        let exceeded_proxy_bytes = exact_proxy_bytes
            .checked_add(1)
            .expect("the configured owner ceiling leaves one excess byte");
        let (ownership, calls, dropped) = scripted_ownership(53, [], [], exceeded_proxy_bytes);
        let refusal = recovery_request(ownership)
            .expect_err("one byte above the recovery ceiling must fail closed");
        assert_eq!(
            refusal.reason(),
            ExtensionRuntimeRecoveryBuildError::RetainedBytesExceeded
        );
        let debug = format!("{refusal:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("ScriptedOwnership"));
        assert!(calls.lock().expect("lock").is_empty());
        assert!(dropped.lock().expect("lock").is_empty());
        drop(refusal);
        assert_eq!(*dropped.lock().expect("lock"), vec![53]);

        let (ownership, calls, dropped) = scripted_ownership(54, [], [], usize::MAX);
        let refusal = recovery_request(ownership)
            .expect_err("overflowing recovery accounting must fail closed");
        assert_eq!(
            refusal.reason(),
            ExtensionRuntimeRecoveryBuildError::RetainedBytesOverflow
        );
        assert!(calls.lock().expect("lock").is_empty());
        assert!(dropped.lock().expect("lock").is_empty());
        drop(refusal);
        assert_eq!(*dropped.lock().expect("lock"), vec![54]);
    }

    #[test]
    fn retirement_uncertainty_can_reconcile_back_to_owned() {
        let (request, recovered, calls) = request(
            15,
            [activated()],
            [
                retirement_uncertain(ExtensionRuntimeFailure::TimedOut, None),
                retired(),
            ],
            [owned()],
        );
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let uncertain = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::OwnershipUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(recovered.lock().expect("lock").is_empty());
        let owner = match uncertain.reconcile_until(future_deadline()) {
            ExtensionRuntimeReconciliationSettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let access = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::Retired { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![15]);
        assert_eq!(
            *calls.lock().expect("lock"),
            vec!["activate", "retire", "reconcile", "retire"]
        );
    }

    #[test]
    fn definite_absence_recovers_exact_provider_after_both_ownership_paths() {
        let (request, recovered, _) = self::request(44, [activated()], [retired()], []);
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let access = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::Retired { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let provider = recover_identity_provider(access);
        assert_eq!(provider.identity, 44);
        assert!(recovered.lock().expect("lock").is_empty());
        drop(provider);
        assert_eq!(*recovered.lock().expect("lock"), vec![44]);

        let (request, recovered, _) = self::request(
            45,
            [activation_uncertain(
                ExtensionRuntimeFailure::TimedOut,
                None,
            )],
            [],
            [absent()],
        );
        let uncertain = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, .. } => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let settlement = uncertain.reconcile_until(future_deadline());
        assert!(settlement.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
        let access = match settlement {
            ExtensionRuntimeReconciliationSettlement::Absent { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let provider = recover_identity_provider(access);
        assert_eq!(provider.identity, 45);
        assert!(recovered.lock().expect("lock").is_empty());
        drop(provider);
        assert_eq!(*recovered.lock().expect("lock"), vec![45]);
    }

    #[test]
    fn dropping_capabilities_never_invokes_lifecycle_callbacks() {
        let (request, _, calls) = self::request(46, [activated()], [], []);
        drop(request);
        assert!(calls.lock().expect("lock").is_empty());

        let (request, _, calls) = self::request(47, [activated()], [], []);
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(owner);
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);

        let (request, _, calls) = self::request(48, [activated()], [], []);
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(owner.into_retirement_request());
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);

        let (request, _, calls) = self::request(
            49,
            [activation_uncertain(
                ExtensionRuntimeFailure::TimedOut,
                None,
            )],
            [],
            [],
        );
        let uncertain = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, .. } => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(uncertain);
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);

        let (ownership, calls, dropped) = scripted_ownership(50, [], [], 1024);
        let recovery =
            recovery_request(ownership).expect("persisted recovery must fit the owner budget");
        drop(recovery);
        assert!(calls.lock().expect("lock").is_empty());
        assert_eq!(*dropped.lock().expect("lock"), vec![50]);
    }

    #[test]
    fn dropping_every_recovery_capability_is_passive() {
        let (ownership, calls, dropped) = scripted_ownership(64, [], [owned()], 1024);
        let request = recovery_request(ownership).expect("bounded recovery request");
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(calls.lock().expect("lock").len(), 1);
        drop(owner);
        assert_eq!(calls.lock().expect("lock").len(), 1);
        assert_eq!(*dropped.lock().expect("lock"), vec![64]);

        let (ownership, calls, dropped) = scripted_ownership(65, [], [owned()], 1024);
        let request = recovery_request(ownership).expect("bounded recovery request");
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(owner.into_retirement_request());
        assert_eq!(calls.lock().expect("lock").len(), 1);
        assert_eq!(*dropped.lock().expect("lock"), vec![65]);

        let (ownership, calls, dropped) = scripted_ownership(69, [], [owned()], 1024);
        let request = recovery_request(ownership).expect("bounded recovery request");
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let owner = owner.into_retirement_request().cancel();
        drop(owner);
        assert_eq!(calls.lock().expect("lock").len(), 1);
        assert_eq!(*dropped.lock().expect("lock"), vec![69]);

        let (ownership, calls, dropped) = scripted_ownership(
            66,
            [],
            [still_uncertain(
                ExtensionRuntimeFailure::BackendUnavailable,
                None,
            )],
            1024,
        );
        let request = recovery_request(ownership).expect("bounded recovery request");
        let request = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(request);
        assert_eq!(calls.lock().expect("lock").len(), 1);
        assert_eq!(*dropped.lock().expect("lock"), vec![66]);

        let (ownership, calls, dropped) = scripted_ownership(
            67,
            [ExtensionRuntimeRetirementDisposition::Retained(
                ExtensionRuntimeFailure::BackendUnavailable,
            )],
            [owned()],
            1024,
        );
        let request = recovery_request(ownership).expect("bounded recovery request");
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let owner = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRecoveryRetirementSettlement::Retained { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(owner);
        assert_eq!(calls.lock().expect("lock").len(), 2);
        assert_eq!(*dropped.lock().expect("lock"), vec![67]);

        let (ownership, calls, dropped) = scripted_ownership(
            68,
            [retirement_uncertain(
                ExtensionRuntimeFailure::TimedOut,
                None,
            )],
            [owned()],
            1024,
        );
        let request = recovery_request(ownership).expect("bounded recovery request");
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let request = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRecoveryRetirementSettlement::OwnershipUncertain {
                request,
                failure,
            } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(request);
        assert_eq!(calls.lock().expect("lock").len(), 2);
        assert_eq!(*dropped.lock().expect("lock"), vec![68]);
    }

    #[test]
    fn owner_can_use_bounded_access_without_extracting_it() {
        let (request, recovered, _) = request(16, [activated()], [retired()], []);
        let mut owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let mut calls = 0;
        assert_eq!(
            owner.visit_manifest(&mut |reader: &mut dyn Read| {
                calls += 1;
                assert_eq!(reader.read(&mut [0_u8; 1]).expect("read"), 1);
                Ok(())
            }),
            Ok(Ok(()))
        );
        assert_eq!(calls, 1);
        let access = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::Retired { access, .. } => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(access);
        assert_eq!(*recovered.lock().expect("lock"), vec![16]);
    }

    #[test]
    fn lifecycle_capabilities_and_port_are_send_and_object_safe() {
        fn assert_send<T: Send>() {}
        assert_send::<ExtensionRuntimeActivationRequest>();
        assert_send::<ExtensionRuntimeOwner>();
        assert_send::<ExtensionRuntimeRetirementRequest>();
        assert_send::<ExtensionRuntimeUncertainOwner>();
        assert_send::<ExtensionRuntimeRecoveryRequest>();
        assert_send::<ExtensionRuntimeRecoveryOwner>();
        assert_send::<ExtensionRuntimeRecoveryRetirementRequest>();
        assert_send::<ExtensionRuntimeRecoverySettlement>();
        assert_send::<ExtensionRuntimeRecoveryRetirementSettlement>();
        let _: Option<Box<dyn ExtensionRuntimeOwnershipPort>> = None;
        let _: Option<Box<dyn ExtensionRuntimeLifecyclePort>> = None;
    }

    #[test]
    fn capability_debug_output_is_redacted() {
        let (request, _, _) = request(17, [activated()], [], []);
        let debug = format!("{request:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("IdentityProvider"));
    }
}
