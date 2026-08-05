//! Backend-neutral, non-authorizing native extension-runtime contracts.
//!
//! This crate defines only move-only ownership and bounded I/O contracts. It
//! neither authorizes extension packages nor owns durable repository pins. A
//! package service must complete those checks before constructing delegated
//! access and may recover that access only from same-process settlements or
//! cancellations whose state already proves native ownership absent.
//! Persisted uncertainty is reconstructed only from a cleanup-only ownership
//! proxy: restart recovery carries no package access, resource plan, provider,
//! target, or activation-capable port. Public native-owner identifiers and
//! evidence enums are structural and non-authorizing; authority exists only
//! through a service-selected trusted lifecycle port joined to the exact
//! backend, native incarnation, and durable ownership row.

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod access;
mod lifecycle;
mod ownership_evidence;
mod resource_plan;
mod target;

pub use access::{
    ExtensionPackageAccess, ExtensionPackageAccessBuildError, ExtensionPackageAccessBuildRefusal,
    ExtensionPackageAccessError, ExtensionPackageAccessPort, ExtensionRuntimeNativeRootVisitor,
    ExtensionRuntimeResourceVisitor, ExtensionRuntimeVisitorError,
    MAX_EXTENSION_RUNTIME_INTERRUPTED_READ_RETRIES, MAX_EXTENSION_RUNTIME_NATIVE_ROOT_BYTES,
    MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
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
