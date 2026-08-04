//! Backend-neutral, non-authorizing native extension-runtime contracts.
//!
//! This crate defines only move-only ownership and bounded I/O contracts. It
//! neither authorizes extension packages nor owns durable repository pins. A
//! package service must complete those checks before constructing delegated
//! access and may recover that access only from settlements or cancellations
//! whose state already proves native ownership absent.

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod access;
mod lifecycle;
mod target;

pub use access::{
    ExtensionPackageAccess, ExtensionPackageAccessBuildError, ExtensionPackageAccessBuildRefusal,
    ExtensionPackageAccessError, ExtensionPackageAccessPort, ExtensionRuntimeNativeRootVisitor,
    ExtensionRuntimeResource, ExtensionRuntimeResourceBuildError, ExtensionRuntimeResourceVisitor,
    ExtensionRuntimeVisitorError, MAX_EXTENSION_RUNTIME_INTERRUPTED_READ_RETRIES,
    MAX_EXTENSION_RUNTIME_MANIFEST_BYTES, MAX_EXTENSION_RUNTIME_NATIVE_ROOT_BYTES,
    MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES, MAX_EXTENSION_RUNTIME_RESOURCE_BYTES,
};
pub use lifecycle::{
    ExtensionPackageAccessView, ExtensionRuntimeActivationBuildError,
    ExtensionRuntimeActivationBuildRefusal, ExtensionRuntimeActivationDisposition,
    ExtensionRuntimeActivationRequest, ExtensionRuntimeActivationSettlement,
    ExtensionRuntimeFailure, ExtensionRuntimeLifecyclePort, ExtensionRuntimeOwner,
    ExtensionRuntimeOwnershipDisposition, ExtensionRuntimeReconciliationSettlement,
    ExtensionRuntimeRetirementDisposition, ExtensionRuntimeRetirementRequest,
    ExtensionRuntimeRetirementSettlement, ExtensionRuntimeUncertainOwner,
};
pub use target::ExtensionRuntimeTarget;
