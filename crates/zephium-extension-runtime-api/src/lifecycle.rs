//! Explicit runtime ownership settlement.

use std::fmt;
use std::time::Instant;

use crate::{
    ExtensionPackageAccess, ExtensionPackageAccessError, ExtensionRuntimeNativeRootVisitor,
    ExtensionRuntimeResource, ExtensionRuntimeResourcePlan, ExtensionRuntimeResourceVisitor,
    ExtensionRuntimeTarget, ExtensionRuntimeVisitorError,
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
    Activated,
    /// The runtime definitely does not own an activation and the same request
    /// may be retried.
    Retryable(ExtensionRuntimeFailure),
    /// The runtime definitely does not own an activation and the request must be
    /// rejected back to the package service.
    Rejected(ExtensionRuntimeFailure),
    /// Native ownership could not be determined.
    OwnershipUncertain(ExtensionRuntimeFailure),
}

/// A backend's closed retirement outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeRetirementDisposition {
    /// Native ownership is definitely absent.
    Retired,
    /// Native ownership definitely remains active.
    Retained(ExtensionRuntimeFailure),
    /// Native ownership could not be determined.
    OwnershipUncertain(ExtensionRuntimeFailure),
}

/// A backend's closed ownership-reconciliation outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeOwnershipDisposition {
    /// Native ownership definitely exists.
    Owned,
    /// Native ownership is definitely absent.
    Absent,
    /// Native ownership remains indeterminate.
    StillUncertain(ExtensionRuntimeFailure),
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

    /// Visits the native package root synchronously.
    pub fn visit_native_root(
        &mut self,
        visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
    ) -> Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError> {
        self.access.visit_native_root(visitor)
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
            };
        }
        let disposition = self
            .core
            .with_activation_view(|lifecycle, access| lifecycle.activate_until(access, deadline));
        match disposition {
            ExtensionRuntimeActivationDisposition::Activated => {
                ExtensionRuntimeActivationSettlement::Activated(ExtensionRuntimeOwner {
                    core: self.core,
                })
            }
            ExtensionRuntimeActivationDisposition::Retryable(failure) => {
                ExtensionRuntimeActivationSettlement::Retryable {
                    request: self,
                    failure,
                }
            }
            ExtensionRuntimeActivationDisposition::Rejected(failure) => {
                ExtensionRuntimeActivationSettlement::Rejected {
                    access: self.core.access,
                    failure,
                }
            }
            ExtensionRuntimeActivationDisposition::OwnershipUncertain(failure) => {
                ExtensionRuntimeActivationSettlement::OwnershipUncertain {
                    owner: ExtensionRuntimeUncertainOwner { core: self.core },
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
    },
    /// Native ownership is definitely absent and package access is returned to
    /// the package service.
    Rejected {
        /// The same delegated package-access capability.
        access: ExtensionPackageAccess,
        /// The redacted runtime failure.
        failure: ExtensionRuntimeFailure,
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
}

impl ExtensionRuntimeOwner {
    /// Returns the selected runtime family.
    #[must_use]
    pub const fn target(&self) -> ExtensionRuntimeTarget {
        self.core.access.target()
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
        ExtensionRuntimeRetirementRequest { core: self.core }
    }
}

impl fmt::Debug for ExtensionRuntimeOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeOwner")
            .field("target", &self.target())
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
}

impl ExtensionRuntimeRetirementRequest {
    /// Returns the stable total retained-memory charge for this owner.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.core.retained_bytes
    }

    /// Cancels retirement without changing definite native ownership.
    ///
    /// No lifecycle method is called; the original owner state is restored.
    #[must_use = "cancelled retirement still owns the native runtime"]
    pub fn cancel(self) -> ExtensionRuntimeOwner {
        ExtensionRuntimeOwner { core: self.core }
    }

    /// Attempts retirement before the caller's absolute monotonic `deadline`.
    ///
    /// An already-expired deadline performs no adapter call and restores the
    /// definitely-owned state. Expiration never proves native absence.
    pub fn settle_until(self, deadline: Instant) -> ExtensionRuntimeRetirementSettlement {
        if deadline <= Instant::now() {
            return ExtensionRuntimeRetirementSettlement::Retained {
                owner: ExtensionRuntimeOwner { core: self.core },
                failure: ExtensionRuntimeFailure::TimedOut,
            };
        }
        let mut core = self.core;
        let disposition = core.lifecycle.retire_until(deadline);
        match disposition {
            ExtensionRuntimeRetirementDisposition::Retired => {
                ExtensionRuntimeRetirementSettlement::Retired(core.access)
            }
            ExtensionRuntimeRetirementDisposition::Retained(failure) => {
                ExtensionRuntimeRetirementSettlement::Retained {
                    owner: ExtensionRuntimeOwner { core },
                    failure,
                }
            }
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain(failure) => {
                ExtensionRuntimeRetirementSettlement::OwnershipUncertain {
                    owner: ExtensionRuntimeUncertainOwner { core },
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
    Retired(ExtensionPackageAccess),
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
            Self::Retired(access) => access_settlement_retained_bytes::<Self>(access),
            Self::Retained { owner, .. } => owner.retained_bytes(),
            Self::OwnershipUncertain { owner, .. } => owner.retained_bytes(),
        }
    }
}

impl fmt::Debug for ExtensionRuntimeRetirementSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retired(_) => formatter.write_str("Retired([redacted])"),
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
}

impl ExtensionRuntimeUncertainOwner {
    /// Returns the selected runtime family without exposing package access.
    #[must_use]
    pub const fn target(&self) -> ExtensionRuntimeTarget {
        self.core.access.target()
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
            ExtensionRuntimeOwnershipDisposition::Owned => {
                ExtensionRuntimeReconciliationSettlement::Owned(ExtensionRuntimeOwner {
                    core: self.core,
                })
            }
            ExtensionRuntimeOwnershipDisposition::Absent => {
                ExtensionRuntimeReconciliationSettlement::Absent(self.core.access)
            }
            ExtensionRuntimeOwnershipDisposition::StillUncertain(failure) => {
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
    Absent(ExtensionPackageAccess),
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
            Self::Absent(access) => access_settlement_retained_bytes::<Self>(access),
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
            Self::Absent(_) => formatter.write_str("Absent([redacted])"),
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
    retained_bytes: usize,
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
    pub fn try_from_persisted_uncertainty(
        ownership: Box<dyn ExtensionRuntimeOwnershipPort>,
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
                retained_bytes,
            },
        })
    }

    /// Returns the exact conservative retained-memory charge for this owner.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.core.retained_bytes
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
            ExtensionRuntimeOwnershipDisposition::Owned => {
                ExtensionRuntimeRecoverySettlement::Owned(ExtensionRuntimeRecoveryOwner { core })
            }
            ExtensionRuntimeOwnershipDisposition::Absent => {
                ExtensionRuntimeRecoverySettlement::Absent
            }
            ExtensionRuntimeOwnershipDisposition::StillUncertain(failure) => {
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
    Absent,
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
            Self::Absent => std::mem::size_of::<Self>(),
            Self::StillUncertain { request, .. } => request.retained_bytes(),
        }
    }
}

impl fmt::Debug for ExtensionRuntimeRecoverySettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Owned(_) => formatter.write_str("Owned([redacted])"),
            Self::Absent => formatter.write_str("Absent"),
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
    core: RecoveryOwnershipCore,
}

impl ExtensionRuntimeRecoveryOwner {
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
    core: RecoveryOwnershipCore,
}

impl ExtensionRuntimeRecoveryRetirementRequest {
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
            ExtensionRuntimeRetirementDisposition::Retired => {
                ExtensionRuntimeRecoveryRetirementSettlement::Absent
            }
            ExtensionRuntimeRetirementDisposition::Retained(failure) => {
                ExtensionRuntimeRecoveryRetirementSettlement::Retained {
                    owner: ExtensionRuntimeRecoveryOwner { core },
                    failure,
                }
            }
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain(failure) => {
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
            .field("retained_bytes", &self.retained_bytes())
            .field("ownership", &"[redacted]")
            .finish()
    }
}

/// Cleanup-only result of a persisted-owner retirement attempt.
#[must_use]
pub enum ExtensionRuntimeRecoveryRetirementSettlement {
    /// Native ownership is definitely absent.
    Absent,
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
            Self::Absent => std::mem::size_of::<Self>(),
            Self::Retained { owner, .. } => owner.retained_bytes(),
            Self::OwnershipUncertain { request, .. } => request.retained_bytes(),
        }
    }
}

impl fmt::Debug for ExtensionRuntimeRecoveryRetirementSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => formatter.write_str("Absent"),
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
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::{
        ExtensionPackageAccessPort, ExtensionRuntimeResourceBinding, ExtensionRuntimeResourcePlan,
    };

    struct IdentityProvider {
        identity: u64,
        recovered: Arc<Mutex<Vec<u64>>>,
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

        fn visit_native_root(
            &mut self,
            _target: ExtensionRuntimeTarget,
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

    struct ScriptedLifecycle {
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
            assert_eq!(access.target(), ExtensionRuntimeTarget::Compatibility);
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

        fn retire_until(&mut self, _deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
            ExtensionRuntimeRetirementDisposition::Retired
        }

        fn reconcile_ownership_until(
            &mut self,
            _deadline: Instant,
        ) -> ExtensionRuntimeOwnershipDisposition {
            ExtensionRuntimeOwnershipDisposition::Owned
        }
    }

    impl ExtensionRuntimeLifecyclePort for BudgetLifecycle {
        fn activate_until(
            &mut self,
            _access: &mut ExtensionPackageAccessView<'_>,
            _deadline: Instant,
        ) -> ExtensionRuntimeActivationDisposition {
            ExtensionRuntimeActivationDisposition::Activated
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

        fn retire_until(&mut self, _deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
            self.ownership_callbacks.fetch_add(1, Ordering::Relaxed);
            ExtensionRuntimeRetirementDisposition::Retired
        }

        fn reconcile_ownership_until(
            &mut self,
            _deadline: Instant,
        ) -> ExtensionRuntimeOwnershipDisposition {
            self.ownership_callbacks.fetch_add(1, Ordering::Relaxed);
            ExtensionRuntimeOwnershipDisposition::Absent
        }
    }

    struct ScriptedOwnership {
        identity: u64,
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

        fn retire_until(&mut self, deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
            self.calls.lock().expect("lock").push(("retire", deadline));
            ExtensionRuntimeRetirementDisposition::Retired
        }

        fn reconcile_ownership_until(
            &mut self,
            deadline: Instant,
        ) -> ExtensionRuntimeOwnershipDisposition {
            self.calls
                .lock()
                .expect("lock")
                .push(("reconcile", deadline));
            ExtensionRuntimeOwnershipDisposition::Owned
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
            ExtensionRuntimeActivationDisposition::OwnershipUncertain(
                ExtensionRuntimeFailure::TimedOut,
            )
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
        (
            Box::new(ScriptedOwnership {
                identity,
                retirements: retirements.into_iter().collect(),
                reconciliations: reconciliations.into_iter().collect(),
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
        let recovered = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let resources = identity_resource_plan(identity);
        let access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::Compatibility,
            resources,
            Box::new(IdentityProvider {
                identity,
                recovered: Arc::clone(&recovered),
            }),
        )
        .expect("access");
        let request = ExtensionRuntimeActivationRequest::try_new(
            access,
            Box::new(ScriptedLifecycle {
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
            ExtensionRuntimeRetirementSettlement::Retired(access) => access,
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
    fn definitely_absent_activation_requests_can_be_cancelled_without_native_work() {
        let (request, recovered, calls) = self::request(
            41,
            [ExtensionRuntimeActivationDisposition::Activated],
            [],
            [],
        );
        let (access, lifecycle) = request.cancel();
        assert!(calls.lock().expect("lock").is_empty());
        let provider = recover_identity_provider(access);
        assert_eq!(provider.identity, 41);
        drop(lifecycle);
        drop(provider);
        assert_eq!(*recovered.lock().expect("lock"), vec![41]);

        let (request, recovered, calls) = self::request(
            42,
            [ExtensionRuntimeActivationDisposition::Retryable(
                ExtensionRuntimeFailure::BackendUnavailable,
            )],
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
        let (request, recovered, calls) = request(
            43,
            [ExtensionRuntimeActivationDisposition::Activated],
            [ExtensionRuntimeRetirementDisposition::Retired],
            [],
        );
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
            ExtensionRuntimeRetirementSettlement::Retired(access) => access,
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
            ExtensionRuntimeRetirementSettlement::Retired(access) => access,
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
        let (request, recovered, calls) = self::request(
            59,
            [ExtensionRuntimeActivationDisposition::Activated],
            [],
            [],
        );
        let request = match request.settle_until(Instant::now()) {
            ExtensionRuntimeActivationSettlement::Retryable { request, failure } => {
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

        let (request, recovered, calls) = self::request(
            60,
            [ExtensionRuntimeActivationDisposition::Activated],
            [ExtensionRuntimeRetirementDisposition::Retired],
            [],
        );
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
            [ExtensionRuntimeActivationDisposition::OwnershipUncertain(
                ExtensionRuntimeFailure::TimedOut,
            )],
            [],
            [ExtensionRuntimeOwnershipDisposition::Absent],
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

        let (ownership, calls, dropped) =
            scripted_ownership(62, [], [ExtensionRuntimeOwnershipDisposition::Absent], 1024);
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
            .expect("bounded recovery request");
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

        let (ownership, calls, dropped) = scripted_ownership(
            63,
            [ExtensionRuntimeRetirementDisposition::Retired],
            [ExtensionRuntimeOwnershipDisposition::Owned],
            1024,
        );
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
            .expect("bounded recovery request");
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
            [ExtensionRuntimeActivationDisposition::Rejected(
                ExtensionRuntimeFailure::PackageRejected,
            )],
            [],
            [],
        );

        let access = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Rejected { access, failure } => {
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
                ExtensionRuntimeActivationDisposition::Retryable(
                    ExtensionRuntimeFailure::BackendUnavailable,
                ),
                ExtensionRuntimeActivationDisposition::Activated,
            ],
            [ExtensionRuntimeRetirementDisposition::Retired],
            [],
        );

        let request = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Retryable { request, failure } => {
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
            ExtensionRuntimeRetirementSettlement::Retired(access) => access,
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
            [ExtensionRuntimeActivationDisposition::Activated],
            [
                ExtensionRuntimeRetirementDisposition::Retained(
                    ExtensionRuntimeFailure::BackendUnavailable,
                ),
                ExtensionRuntimeRetirementDisposition::Retired,
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
            ExtensionRuntimeRetirementSettlement::Retired(access) => access,
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
            [ExtensionRuntimeActivationDisposition::OwnershipUncertain(
                ExtensionRuntimeFailure::TimedOut,
            )],
            [],
            [
                ExtensionRuntimeOwnershipDisposition::StillUncertain(
                    ExtensionRuntimeFailure::TimedOut,
                ),
                ExtensionRuntimeOwnershipDisposition::Absent,
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
            ExtensionRuntimeReconciliationSettlement::Absent(access) => access,
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
                ExtensionRuntimeRetirementDisposition::OwnershipUncertain(
                    ExtensionRuntimeFailure::TimedOut,
                ),
            ],
            [
                ExtensionRuntimeOwnershipDisposition::StillUncertain(
                    ExtensionRuntimeFailure::BackendUnavailable,
                ),
                ExtensionRuntimeOwnershipDisposition::Owned,
                ExtensionRuntimeOwnershipDisposition::Absent,
            ],
            1024,
        );
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
            .expect("bounded persisted recovery must be constructible");
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
            ExtensionRuntimeRecoverySettlement::Absent
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
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(Box::new(
            RecoveryConstructionProbeOwnership {
                retained_queries: Arc::clone(&retained_queries),
                ownership_callbacks: Arc::clone(&ownership_callbacks),
            },
        ))
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
        let (ownership, calls, dropped) = scripted_ownership(
            52,
            [ExtensionRuntimeRetirementDisposition::Retired],
            [ExtensionRuntimeOwnershipDisposition::Owned],
            exact_proxy_bytes,
        );
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
            .expect("the exact recovery-owner ceiling must be accepted");
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
            ExtensionRuntimeRecoveryRetirementSettlement::Absent
        ));
        assert_eq!(calls.lock().expect("lock").len(), 2);
        assert_eq!(*dropped.lock().expect("lock"), vec![52]);

        let exceeded_proxy_bytes = exact_proxy_bytes
            .checked_add(1)
            .expect("the configured owner ceiling leaves one excess byte");
        let (ownership, calls, dropped) = scripted_ownership(53, [], [], exceeded_proxy_bytes);
        let refusal = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
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
        let refusal = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
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
            [ExtensionRuntimeActivationDisposition::Activated],
            [
                ExtensionRuntimeRetirementDisposition::OwnershipUncertain(
                    ExtensionRuntimeFailure::TimedOut,
                ),
                ExtensionRuntimeRetirementDisposition::Retired,
            ],
            [ExtensionRuntimeOwnershipDisposition::Owned],
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
            ExtensionRuntimeRetirementSettlement::Retired(access) => access,
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
        let (request, recovered, _) = self::request(
            44,
            [ExtensionRuntimeActivationDisposition::Activated],
            [ExtensionRuntimeRetirementDisposition::Retired],
            [],
        );
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let access = match owner
            .into_retirement_request()
            .settle_until(future_deadline())
        {
            ExtensionRuntimeRetirementSettlement::Retired(access) => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let provider = recover_identity_provider(access);
        assert_eq!(provider.identity, 44);
        assert!(recovered.lock().expect("lock").is_empty());
        drop(provider);
        assert_eq!(*recovered.lock().expect("lock"), vec![44]);

        let (request, recovered, _) = self::request(
            45,
            [ExtensionRuntimeActivationDisposition::OwnershipUncertain(
                ExtensionRuntimeFailure::TimedOut,
            )],
            [],
            [ExtensionRuntimeOwnershipDisposition::Absent],
        );
        let uncertain = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, .. } => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let settlement = uncertain.reconcile_until(future_deadline());
        assert!(settlement.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
        let access = match settlement {
            ExtensionRuntimeReconciliationSettlement::Absent(access) => access,
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
        let (request, _, calls) = self::request(
            46,
            [ExtensionRuntimeActivationDisposition::Activated],
            [],
            [],
        );
        drop(request);
        assert!(calls.lock().expect("lock").is_empty());

        let (request, _, calls) = self::request(
            47,
            [ExtensionRuntimeActivationDisposition::Activated],
            [],
            [],
        );
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(owner);
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);

        let (request, _, calls) = self::request(
            48,
            [ExtensionRuntimeActivationDisposition::Activated],
            [],
            [],
        );
        let owner = match request.settle_until(future_deadline()) {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(owner.into_retirement_request());
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);

        let (request, _, calls) = self::request(
            49,
            [ExtensionRuntimeActivationDisposition::OwnershipUncertain(
                ExtensionRuntimeFailure::TimedOut,
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
        let recovery = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
            .expect("persisted recovery must fit the owner budget");
        drop(recovery);
        assert!(calls.lock().expect("lock").is_empty());
        assert_eq!(*dropped.lock().expect("lock"), vec![50]);
    }

    #[test]
    fn dropping_every_recovery_capability_is_passive() {
        let (ownership, calls, dropped) =
            scripted_ownership(64, [], [ExtensionRuntimeOwnershipDisposition::Owned], 1024);
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
            .expect("bounded recovery request");
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(calls.lock().expect("lock").len(), 1);
        drop(owner);
        assert_eq!(calls.lock().expect("lock").len(), 1);
        assert_eq!(*dropped.lock().expect("lock"), vec![64]);

        let (ownership, calls, dropped) =
            scripted_ownership(65, [], [ExtensionRuntimeOwnershipDisposition::Owned], 1024);
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
            .expect("bounded recovery request");
        let owner = match request.reconcile_until(future_deadline()) {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(owner.into_retirement_request());
        assert_eq!(calls.lock().expect("lock").len(), 1);
        assert_eq!(*dropped.lock().expect("lock"), vec![65]);

        let (ownership, calls, dropped) =
            scripted_ownership(69, [], [ExtensionRuntimeOwnershipDisposition::Owned], 1024);
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
            .expect("bounded recovery request");
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
            [ExtensionRuntimeOwnershipDisposition::StillUncertain(
                ExtensionRuntimeFailure::BackendUnavailable,
            )],
            1024,
        );
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
            .expect("bounded recovery request");
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
            [ExtensionRuntimeOwnershipDisposition::Owned],
            1024,
        );
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
            .expect("bounded recovery request");
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
            [ExtensionRuntimeRetirementDisposition::OwnershipUncertain(
                ExtensionRuntimeFailure::TimedOut,
            )],
            [ExtensionRuntimeOwnershipDisposition::Owned],
            1024,
        );
        let request = ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(ownership)
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
        let (request, recovered, _) = request(
            16,
            [ExtensionRuntimeActivationDisposition::Activated],
            [ExtensionRuntimeRetirementDisposition::Retired],
            [],
        );
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
            ExtensionRuntimeRetirementSettlement::Retired(access) => access,
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
        let (request, _, _) = request(
            17,
            [ExtensionRuntimeActivationDisposition::Activated],
            [],
            [],
        );
        let debug = format!("{request:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("IdentityProvider"));
    }
}
