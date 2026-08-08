//! Backend-neutral native extension-runtime contracts.
//!
//! This crate does not originate package authorization or own durable
//! repository pins. It defines move-only ownership and bounded I/O contracts,
//! then linearly transports already-minted Store/package authority across the
//! trusted serialized-service and native-engine boundary. A package service
//! must complete authorization and pinning before constructing delegated
//! access and may recover that access only from same-process settlements or
//! cancellations whose state already proves native ownership absent.
//! Persisted uncertainty is reconstructed only from a cleanup-only ownership
//! proxy: restart recovery carries no package access, resource plan, provider,
//! target, or activation-capable port. Public native-owner identifiers and
//! evidence enums are structural and non-authorizing; live operation authority
//! exists only through the exact Store-derived capability published into a
//! service-selected trusted engine port and joined to the backend, native
//! incarnation, registry generation, and current durable ownership row.

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod access;
mod capacity;
mod host;
mod lifecycle;
mod ownership_evidence;
mod resource_plan;
mod target;

pub use access::{
    ExtensionPackageAccess, ExtensionPackageAccessBuildError, ExtensionPackageAccessBuildRefusal,
    ExtensionPackageAccessError, ExtensionPackageAccessPort, ExtensionRuntimeNativeRootLease,
    ExtensionRuntimeNativeRootLeasePort, ExtensionRuntimeNativeRootVisitor,
    ExtensionRuntimeResourceVisitor, ExtensionRuntimeVisitorError,
    MAX_EXTENSION_RUNTIME_INTERRUPTED_READ_RETRIES, MAX_EXTENSION_RUNTIME_NATIVE_ROOT_BYTES,
    MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
};
pub use capacity::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES;
pub use host::{
    ExtensionRuntimeAbsenceEvidenceIssuer, ExtensionRuntimeBoundAbsenceEvidenceIssuer,
    ExtensionRuntimeHostActivation, ExtensionRuntimeHostActivationBindRefusal,
    ExtensionRuntimeHostActivationBinding, ExtensionRuntimeHostActivationBindingError,
    ExtensionRuntimeHostActivationBindingRefusal, ExtensionRuntimeHostActivationContext,
    ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError,
    ExtensionRuntimeHostFactory, ExtensionRuntimeHostFactoryPort,
    ExtensionRuntimeHostLifecyclePort, ExtensionRuntimeHostOwnershipPort,
    ExtensionRuntimeHostProfileAbsenceDisposition, ExtensionRuntimeHostProfileAbsenceEvidence,
    ExtensionRuntimeHostPublicationPort, ExtensionRuntimeHostPublicationPortRefusal,
    ExtensionRuntimeHostRecoveryBindRefusal, ExtensionRuntimeHostRecoveryBinding,
    ExtensionRuntimeHostRecoveryBindingError, ExtensionRuntimeHostRecoveryBindingRefusal,
    ExtensionRuntimeHostRecoveryContext, ExtensionRuntimeHostRegistryGeneration,
    ExtensionRuntimeNativeIdentityExpectation, ExtensionRuntimeOwnerAddress,
    ExtensionRuntimePendingPublication, ExtensionRuntimePendingRecoveryRefusal,
    ExtensionRuntimePublicationAuthorizationError, ExtensionRuntimePublicationAuthorizationRefusal,
    ExtensionRuntimePublicationReceipt, ExtensionRuntimePublicationReclaimError,
    ExtensionRuntimePublicationReclaimRefusal, ExtensionRuntimePublicationRefusal,
    ExtensionRuntimePublicationRequest, ExtensionRuntimeRequestRecoveryRefusal,
    MAX_EXTENSION_RUNTIME_HOST_QUARANTINE_RETAINED_BYTES,
};
pub use lifecycle::{
    ExtensionPackageAccessView, ExtensionRuntimeActivationBuildError,
    ExtensionRuntimeActivationBuildRefusal, ExtensionRuntimeActivationDisposition,
    ExtensionRuntimeActivationRequest, ExtensionRuntimeActivationSettlement,
    ExtensionRuntimeFailure, ExtensionRuntimeLifecyclePort, ExtensionRuntimeOwner,
    ExtensionRuntimeOwnershipDisposition, ExtensionRuntimeOwnershipPort,
    ExtensionRuntimeReconciliationSettlement, ExtensionRuntimeRecoveryBuildError,
    ExtensionRuntimeRecoveryBuildRefusal, ExtensionRuntimeRecoveryOwner,
    ExtensionRuntimeRecoveryRequest, ExtensionRuntimeRecoveryRetirementRequest,
    ExtensionRuntimeRecoveryRetirementSettlement, ExtensionRuntimeRecoverySettlement,
    ExtensionRuntimeRetirementDisposition, ExtensionRuntimeRetirementRequest,
    ExtensionRuntimeRetirementSettlement, ExtensionRuntimeUncertainOwner,
};
pub use ownership_evidence::{
    ExtensionRuntimeAbsenceEvidence, ExtensionRuntimeAbsenceProofKind,
    ExtensionRuntimeCompatibilityAbsenceAudit, ExtensionRuntimeMacosAbsenceAudit,
    ExtensionRuntimeNativeOwnerId, ExtensionRuntimeNativeOwnerIdError,
    ExtensionRuntimeOwnershipEvidence, ExtensionRuntimeRecoveryExpectation,
    EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES,
};
pub use resource_plan::{
    ExtensionRuntimeResource, ExtensionRuntimeResourceBinding, ExtensionRuntimeResourceBuildError,
    ExtensionRuntimeResourcePlan, ExtensionRuntimeResourcePlanBuildError,
    ExtensionRuntimeResourcePlanEntry, MAX_EXTENSION_RUNTIME_MANIFEST_BYTES,
    MAX_EXTENSION_RUNTIME_RESOURCE_BYTES, MAX_EXTENSION_RUNTIME_RESOURCE_PATH_BYTES,
    MAX_EXTENSION_RUNTIME_RESOURCE_PATH_COMPONENT_BYTES, MAX_EXTENSION_RUNTIME_RESOURCE_PATH_DEPTH,
    MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES,
    MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_RETAINED_BYTES,
};
pub use target::ExtensionRuntimeTarget;
