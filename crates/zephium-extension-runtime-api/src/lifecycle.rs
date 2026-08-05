//! Explicit runtime ownership settlement.

use std::fmt;

use crate::{
    ExtensionPackageAccess, ExtensionPackageAccessBuildError, ExtensionPackageAccessError,
    ExtensionPackageAccessPort, ExtensionRuntimeNativeRootVisitor, ExtensionRuntimeResource,
    ExtensionRuntimeResourcePlan, ExtensionRuntimeResourceVisitor, ExtensionRuntimeTarget,
    ExtensionRuntimeVisitorError, MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
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

/// A passive control-plane proxy for one exact native runtime incarnation.
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
pub trait ExtensionRuntimeLifecyclePort: Send {
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
    /// must be side-effect-free so construction refusal returns unchanged state.
    fn retained_bytes(&self) -> usize;

    /// Attempts to activate the package.
    fn activate(
        &mut self,
        access: &mut ExtensionPackageAccessView<'_>,
    ) -> ExtensionRuntimeActivationDisposition;

    /// Attempts to retire the package.
    fn retire(
        &mut self,
        access: &mut ExtensionPackageAccessView<'_>,
    ) -> ExtensionRuntimeRetirementDisposition;

    /// Reconciles native ownership after an uncertain outcome.
    fn reconcile_ownership(
        &mut self,
        access: &mut ExtensionPackageAccessView<'_>,
    ) -> ExtensionRuntimeOwnershipDisposition;
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
    /// Delegated package access failed its bounded construction checks.
    PackageAccess(ExtensionPackageAccessBuildError),
    /// The combined recovery owner exceeded its bounded accounting contract.
    OwnerAccounting(ExtensionRuntimeActivationBuildError),
}

impl fmt::Display for ExtensionRuntimeRecoveryBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PackageAccess(error) => error.fmt(formatter),
            Self::OwnerAccounting(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ExtensionRuntimeRecoveryBuildError {}

/// A failed reconstruction of one conservatively persisted native owner.
///
/// No ordinary [`ExtensionPackageAccess`] escapes this refusal while native
/// ownership remains unresolved. It instead preserves the exact raw delegated
/// provider, target, complete resource plan, and passive lifecycle proxy
/// supplied by the service. No ownership-changing lifecycle callback has run and no
/// input is released or replaced. The caller must keep its external native
/// reservation and durable package pin held on both success and refusal. A
/// refusal is an opaque fail-closed quarantine: dropping it may destroy these
/// process-local wrappers, but provider and lifecycle destructors must remain
/// passive and the durable pin must survive for a later process to reconcile.
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
/// Its captive delegated provider cannot be extracted:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryBuildRefusal;
/// fn escape(refusal: ExtensionRuntimeRecoveryBuildRefusal) {
///     let _ = refusal.into_parts();
/// }
/// ```
#[must_use = "the refusal retains delegated package and recovery lifecycle state"]
pub struct ExtensionRuntimeRecoveryBuildRefusal {
    reason: ExtensionRuntimeRecoveryBuildError,
    target: ExtensionRuntimeTarget,
    _resources: ExtensionRuntimeResourcePlan,
    _provider: Box<dyn ExtensionPackageAccessPort>,
    _lifecycle: Box<dyn ExtensionRuntimeLifecyclePort>,
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
            .field("target", &self.target)
            .field("resources", &"[redacted]")
            .field("provider", &"[redacted]")
            .field("lifecycle", &"[redacted]")
            .finish()
    }
}

struct RuntimeOwnershipCore {
    access: ExtensionPackageAccess,
    lifecycle: Box<dyn ExtensionRuntimeLifecyclePort>,
    retained_bytes: usize,
}

impl RuntimeOwnershipCore {
    fn with_view<T>(
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

    /// Attempts activation and converts the backend disposition into an
    /// ownership-safe public settlement.
    pub fn settle(mut self) -> ExtensionRuntimeActivationSettlement {
        let disposition = self
            .core
            .with_view(|lifecycle, access| lifecycle.activate(access));
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

    /// Attempts retirement and returns access only when native absence is
    /// definite.
    pub fn settle(mut self) -> ExtensionRuntimeRetirementSettlement {
        let disposition = self
            .core
            .with_view(|lifecycle, access| lifecycle.retire(access));
        match disposition {
            ExtensionRuntimeRetirementDisposition::Retired => {
                ExtensionRuntimeRetirementSettlement::Retired(self.core.access)
            }
            ExtensionRuntimeRetirementDisposition::Retained(failure) => {
                ExtensionRuntimeRetirementSettlement::Retained {
                    owner: ExtensionRuntimeOwner { core: self.core },
                    failure,
                }
            }
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain(failure) => {
                ExtensionRuntimeRetirementSettlement::OwnershipUncertain {
                    owner: ExtensionRuntimeUncertainOwner { core: self.core },
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
#[must_use = "uncertain ownership must be reconciled before access can return"]
pub struct ExtensionRuntimeUncertainOwner {
    core: RuntimeOwnershipCore,
}

impl ExtensionRuntimeUncertainOwner {
    /// Reconstructs conservative native-ownership uncertainty after restart.
    ///
    /// This constructor is deliberately non-authorizing. The caller must have
    /// reauthenticated and durably pinned the exact package, loaded an
    /// unresolved ownership row from its bounded durable journal, and created
    /// a passive lifecycle proxy bound to that row's exact native incarnation.
    /// The separate service/engine native-resource reservation must already be
    /// held and must remain held for every returned state.
    ///
    /// Construction invokes only the provider and lifecycle ports'
    /// side-effect-free retained-memory queries; no ownership-changing
    /// lifecycle callback runs. The returned value cannot activate code or
    /// expose package access, and its only settlement operation is
    /// [`Self::reconcile`]. The external native reservation remains held on
    /// both success and refusal. A caller that cannot prove the persisted
    /// identity join must retain its durable package pin and fail closed
    /// instead of constructing this value.
    pub fn try_from_persisted_uncertainty(
        target: ExtensionRuntimeTarget,
        resources: ExtensionRuntimeResourcePlan,
        provider: Box<dyn ExtensionPackageAccessPort>,
        lifecycle: Box<dyn ExtensionRuntimeLifecyclePort>,
    ) -> Result<Self, ExtensionRuntimeRecoveryBuildRefusal> {
        let access =
            match ExtensionPackageAccess::from_delegated_provider(target, resources, provider) {
                Ok(access) => access,
                Err(refusal) => {
                    let reason =
                        ExtensionRuntimeRecoveryBuildError::PackageAccess(refusal.reason());
                    let (target, resources, provider) = refusal.into_parts();
                    return Err(ExtensionRuntimeRecoveryBuildRefusal {
                        reason,
                        target,
                        _resources: resources,
                        _provider: provider,
                        _lifecycle: lifecycle,
                    });
                }
            };
        match ExtensionRuntimeActivationRequest::try_new(access, lifecycle) {
            Ok(request) => Ok(Self { core: request.core }),
            Err(refusal) => {
                let reason = ExtensionRuntimeRecoveryBuildError::OwnerAccounting(refusal.reason());
                let (access, lifecycle) = refusal.into_parts();
                let (target, resources, provider) = access.into_parts();
                Err(ExtensionRuntimeRecoveryBuildRefusal {
                    reason,
                    target,
                    _resources: resources,
                    _provider: provider,
                    _lifecycle: lifecycle,
                })
            }
        }
    }

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

    /// Reconciles native ownership and returns access only on definite absence.
    pub fn reconcile(mut self) -> ExtensionRuntimeReconciliationSettlement {
        let disposition = self
            .core
            .with_view(|lifecycle, access| lifecycle.reconcile_ownership(access));
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::{Cursor, Read};
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

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

    struct OverflowProvider {
        identity: u64,
        recovered: Arc<Mutex<Vec<u64>>>,
    }

    impl Drop for OverflowProvider {
        fn drop(&mut self) {
            self.recovered.lock().expect("lock").push(self.identity);
        }
    }

    impl ExtensionPackageAccessPort for OverflowProvider {
        fn retained_bytes(&self) -> usize {
            usize::MAX
        }

        fn visit_resource(
            &mut self,
            _resource: ExtensionRuntimeResource,
            _visitor: &mut dyn ExtensionRuntimeResourceVisitor,
        ) -> Result<(), ExtensionPackageAccessError> {
            Err(ExtensionPackageAccessError::Inactive)
        }

        fn visit_native_root(
            &mut self,
            _target: ExtensionRuntimeTarget,
            _visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
        ) -> Result<(), ExtensionPackageAccessError> {
            Err(ExtensionPackageAccessError::Inactive)
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

    impl ExtensionRuntimeLifecyclePort for ScriptedLifecycle {
        fn retained_bytes(&self) -> usize {
            self.retained_bytes
        }

        fn activate(
            &mut self,
            access: &mut ExtensionPackageAccessView<'_>,
        ) -> ExtensionRuntimeActivationDisposition {
            assert_eq!(access.target(), ExtensionRuntimeTarget::Compatibility);
            self.calls.lock().expect("lock").push("activate");
            self.activations.pop_front().expect("activation scripted")
        }

        fn retire(
            &mut self,
            access: &mut ExtensionPackageAccessView<'_>,
        ) -> ExtensionRuntimeRetirementDisposition {
            assert_eq!(access.target(), ExtensionRuntimeTarget::Compatibility);
            self.calls.lock().expect("lock").push("retire");
            self.retirements.pop_front().expect("retirement scripted")
        }

        fn reconcile_ownership(
            &mut self,
            access: &mut ExtensionPackageAccessView<'_>,
        ) -> ExtensionRuntimeOwnershipDisposition {
            assert_eq!(access.target(), ExtensionRuntimeTarget::Compatibility);
            self.calls.lock().expect("lock").push("reconcile");
            self.reconciliations
                .pop_front()
                .expect("reconciliation scripted")
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

    impl ExtensionRuntimeLifecyclePort for BudgetLifecycle {
        fn retained_bytes(&self) -> usize {
            self.retained_bytes
        }

        fn activate(
            &mut self,
            _access: &mut ExtensionPackageAccessView<'_>,
        ) -> ExtensionRuntimeActivationDisposition {
            ExtensionRuntimeActivationDisposition::Activated
        }

        fn retire(
            &mut self,
            _access: &mut ExtensionPackageAccessView<'_>,
        ) -> ExtensionRuntimeRetirementDisposition {
            ExtensionRuntimeRetirementDisposition::Retired
        }

        fn reconcile_ownership(
            &mut self,
            _access: &mut ExtensionPackageAccessView<'_>,
        ) -> ExtensionRuntimeOwnershipDisposition {
            ExtensionRuntimeOwnershipDisposition::Owned
        }
    }

    struct RecoveryConstructionProbeLifecycle {
        retained_queries: Arc<AtomicUsize>,
        ownership_callbacks: Arc<AtomicUsize>,
    }

    impl ExtensionRuntimeLifecyclePort for RecoveryConstructionProbeLifecycle {
        fn retained_bytes(&self) -> usize {
            self.retained_queries.fetch_add(1, Ordering::Relaxed);
            0
        }

        fn activate(
            &mut self,
            _access: &mut ExtensionPackageAccessView<'_>,
        ) -> ExtensionRuntimeActivationDisposition {
            self.ownership_callbacks.fetch_add(1, Ordering::Relaxed);
            ExtensionRuntimeActivationDisposition::Activated
        }

        fn retire(
            &mut self,
            _access: &mut ExtensionPackageAccessView<'_>,
        ) -> ExtensionRuntimeRetirementDisposition {
            self.ownership_callbacks.fetch_add(1, Ordering::Relaxed);
            ExtensionRuntimeRetirementDisposition::Retired
        }

        fn reconcile_ownership(
            &mut self,
            _access: &mut ExtensionPackageAccessView<'_>,
        ) -> ExtensionRuntimeOwnershipDisposition {
            self.ownership_callbacks.fetch_add(1, Ordering::Relaxed);
            ExtensionRuntimeOwnershipDisposition::Absent
        }
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

        let activation = request.settle();
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
        let settlement = retirement.settle();
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
        let request = match request.settle() {
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
        let owner = match request.settle() {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let owner = owner.into_retirement_request().cancel();
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);
        let access = match owner.into_retirement_request().settle() {
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

        let access = match request.settle() {
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

        let request = match request.settle() {
            ExtensionRuntimeActivationSettlement::Retryable { request, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                request
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(recovered.lock().expect("lock").is_empty());
        let owner = match request.settle() {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let access = match owner.into_retirement_request().settle() {
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
        let owner = match request.settle() {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let owner = match owner.into_retirement_request().settle() {
            ExtensionRuntimeRetirementSettlement::Retained { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::BackendUnavailable);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(recovered.lock().expect("lock").is_empty());
        let access = match owner.into_retirement_request().settle() {
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
        let uncertain = match request.settle() {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.retained_bytes(), retained_bytes);
        assert!(recovered.lock().expect("lock").is_empty());
        let uncertain = match uncertain.reconcile() {
            ExtensionRuntimeReconciliationSettlement::StillUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(uncertain.retained_bytes(), retained_bytes);
        assert!(recovered.lock().expect("lock").is_empty());
        let access = match uncertain.reconcile() {
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
    fn persisted_uncertainty_can_only_reconcile_and_never_activates() {
        let recovered = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (target, manifest, provider) = identity_provider_inputs(51, &recovered);
        let uncertain = ExtensionRuntimeUncertainOwner::try_from_persisted_uncertainty(
            target,
            manifest,
            provider,
            Box::new(ScriptedLifecycle {
                activations: VecDeque::new(),
                retirements: [ExtensionRuntimeRetirementDisposition::Retired]
                    .into_iter()
                    .collect(),
                reconciliations: [ExtensionRuntimeOwnershipDisposition::Owned]
                    .into_iter()
                    .collect(),
                calls: Arc::clone(&calls),
                retained_bytes: 1024,
            }),
        )
        .expect("persisted uncertainty must fit the owner budget");
        assert!(calls.lock().expect("lock").is_empty());
        assert!(recovered.lock().expect("lock").is_empty());

        let owner = match uncertain.reconcile() {
            ExtensionRuntimeReconciliationSettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(*calls.lock().expect("lock"), vec!["reconcile"]);
        let access = match owner.into_retirement_request().settle() {
            ExtensionRuntimeRetirementSettlement::Retired(access) => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let provider = recover_identity_provider(access);
        assert_eq!(provider.identity, 51);
        drop(provider);
        assert_eq!(*recovered.lock().expect("lock"), vec![51]);
        assert_eq!(*calls.lock().expect("lock"), vec!["reconcile", "retire"]);

        let recovered = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (target, manifest, provider) = identity_provider_inputs(52, &recovered);
        let uncertain = ExtensionRuntimeUncertainOwner::try_from_persisted_uncertainty(
            target,
            manifest,
            provider,
            Box::new(ScriptedLifecycle {
                activations: VecDeque::new(),
                retirements: VecDeque::new(),
                reconciliations: [ExtensionRuntimeOwnershipDisposition::Absent]
                    .into_iter()
                    .collect(),
                calls: Arc::clone(&calls),
                retained_bytes: 1024,
            }),
        )
        .expect("persisted uncertainty must fit the owner budget");
        let access = match uncertain.reconcile() {
            ExtensionRuntimeReconciliationSettlement::Absent(access) => access,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert_eq!(*calls.lock().expect("lock"), vec!["reconcile"]);
        let provider = recover_identity_provider(access);
        assert_eq!(provider.identity, 52);
        drop(provider);
        assert_eq!(*recovered.lock().expect("lock"), vec![52]);
    }

    #[test]
    fn persisted_uncertainty_construction_only_queries_accounting() {
        let recovered = Arc::new(Mutex::new(Vec::new()));
        let (target, manifest, provider) = identity_provider_inputs(57, &recovered);
        let retained_queries = Arc::new(AtomicUsize::new(0));
        let ownership_callbacks = Arc::new(AtomicUsize::new(0));
        let uncertain = ExtensionRuntimeUncertainOwner::try_from_persisted_uncertainty(
            target,
            manifest,
            provider,
            Box::new(RecoveryConstructionProbeLifecycle {
                retained_queries: Arc::clone(&retained_queries),
                ownership_callbacks: Arc::clone(&ownership_callbacks),
            }),
        )
        .expect("bounded persisted uncertainty must be constructible");
        assert_eq!(retained_queries.load(Ordering::Relaxed), 1);
        assert_eq!(ownership_callbacks.load(Ordering::Relaxed), 0);
        drop(uncertain);
        assert_eq!(ownership_callbacks.load(Ordering::Relaxed), 0);
        assert_eq!(*recovered.lock().expect("lock"), vec![57]);
    }

    #[test]
    fn persisted_uncertainty_accounting_refusal_quarantines_both_inputs() {
        let recovered_access = Arc::new(Mutex::new(Vec::new()));
        let recovered_lifecycle = Arc::new(Mutex::new(Vec::new()));
        let (target, manifest, provider) = identity_provider_inputs(53, &recovered_access);
        let refusal = ExtensionRuntimeUncertainOwner::try_from_persisted_uncertainty(
            target,
            manifest,
            provider,
            Box::new(BudgetLifecycle {
                identity: 54,
                retained_bytes: usize::MAX,
                recovered: Arc::clone(&recovered_lifecycle),
            }),
        )
        .expect_err("overflowing recovery owner accounting must fail closed");
        assert_eq!(
            refusal.reason(),
            ExtensionRuntimeRecoveryBuildError::OwnerAccounting(
                ExtensionRuntimeActivationBuildError::RetainedBytesOverflow
            )
        );
        let debug = format!("{refusal:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("IdentityProvider"));
        assert!(recovered_access.lock().expect("lock").is_empty());
        assert!(recovered_lifecycle.lock().expect("lock").is_empty());
        drop(refusal);
        assert_eq!(*recovered_access.lock().expect("lock"), vec![53]);
        assert_eq!(*recovered_lifecycle.lock().expect("lock"), vec![54]);
    }

    #[test]
    fn persisted_uncertainty_access_accounting_refusal_keeps_provider_captive() {
        let recovered_access = Arc::new(Mutex::new(Vec::new()));
        let recovered_lifecycle = Arc::new(Mutex::new(Vec::new()));
        let refusal = ExtensionRuntimeUncertainOwner::try_from_persisted_uncertainty(
            ExtensionRuntimeTarget::Compatibility,
            identity_resource_plan(55),
            Box::new(OverflowProvider {
                identity: 55,
                recovered: Arc::clone(&recovered_access),
            }),
            Box::new(BudgetLifecycle {
                identity: 56,
                retained_bytes: 0,
                recovered: Arc::clone(&recovered_lifecycle),
            }),
        )
        .expect_err("overflowing package-access accounting must fail closed");
        assert_eq!(
            refusal.reason(),
            ExtensionRuntimeRecoveryBuildError::PackageAccess(
                ExtensionPackageAccessBuildError::RetainedBytesOverflow
            )
        );
        assert!(recovered_access.lock().expect("lock").is_empty());
        assert!(recovered_lifecycle.lock().expect("lock").is_empty());
        drop(refusal);
        assert_eq!(*recovered_access.lock().expect("lock"), vec![55]);
        assert_eq!(*recovered_lifecycle.lock().expect("lock"), vec![56]);
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
        let owner = match request.settle() {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let uncertain = match owner.into_retirement_request().settle() {
            ExtensionRuntimeRetirementSettlement::OwnershipUncertain { owner, failure } => {
                assert_eq!(failure, ExtensionRuntimeFailure::TimedOut);
                owner
            }
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        assert!(recovered.lock().expect("lock").is_empty());
        let owner = match uncertain.reconcile() {
            ExtensionRuntimeReconciliationSettlement::Owned(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let access = match owner.into_retirement_request().settle() {
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
        let owner = match request.settle() {
            ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let access = match owner.into_retirement_request().settle() {
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
        let uncertain = match request.settle() {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, .. } => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        let settlement = uncertain.reconcile();
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
        let owner = match request.settle() {
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
        let owner = match request.settle() {
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
        let uncertain = match request.settle() {
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { owner, .. } => owner,
            settlement => panic!("unexpected settlement: {settlement:?}"),
        };
        drop(uncertain);
        assert_eq!(*calls.lock().expect("lock"), vec!["activate"]);

        let recovered = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (target, manifest, provider) = identity_provider_inputs(50, &recovered);
        let uncertain = ExtensionRuntimeUncertainOwner::try_from_persisted_uncertainty(
            target,
            manifest,
            provider,
            Box::new(ScriptedLifecycle {
                activations: VecDeque::new(),
                retirements: VecDeque::new(),
                reconciliations: VecDeque::new(),
                calls: Arc::clone(&calls),
                retained_bytes: 1024,
            }),
        )
        .expect("persisted uncertainty must fit the owner budget");
        drop(uncertain);
        assert!(calls.lock().expect("lock").is_empty());
        assert_eq!(*recovered.lock().expect("lock"), vec![50]);
    }

    #[test]
    fn owner_can_use_bounded_access_without_extracting_it() {
        let (request, recovered, _) = request(
            16,
            [ExtensionRuntimeActivationDisposition::Activated],
            [ExtensionRuntimeRetirementDisposition::Retired],
            [],
        );
        let mut owner = match request.settle() {
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
        let access = match owner.into_retirement_request().settle() {
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
