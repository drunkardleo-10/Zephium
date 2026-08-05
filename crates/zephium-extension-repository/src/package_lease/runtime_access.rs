//! Repository-owned delegation of authenticated package leases to runtimes.

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
use std::cell::Cell;
use std::fmt;
use std::io::Read;
use std::mem::size_of;
use std::sync::Arc;

use thiserror::Error;
use zephium_extension_authority::ProductExtensionRuntimeTarget;
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, MAX_EXTENSION_MANIFEST_BYTES, MAX_EXTENSION_PATH_COMPONENT_BYTES,
    MAX_EXTENSION_RELATIVE_PATH_BYTES, MAX_EXTENSION_RELATIVE_PATH_DEPTH, MAX_EXTENSION_TREE_FILES,
    MAX_EXTENSION_TREE_FILE_BYTES, MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES,
};
use zephium_extension_runtime_api::{
    ExtensionPackageAccess, ExtensionPackageAccessBuildError, ExtensionPackageAccessBuildRefusal,
    ExtensionPackageAccessError, ExtensionPackageAccessPort, ExtensionRuntimeNativeRootVisitor,
    ExtensionRuntimeResource, ExtensionRuntimeResourceBinding, ExtensionRuntimeResourceBuildError,
    ExtensionRuntimeResourcePlan, ExtensionRuntimeResourcePlanBuildError,
    ExtensionRuntimeResourceVisitor, ExtensionRuntimeTarget, MAX_EXTENSION_RUNTIME_MANIFEST_BYTES,
    MAX_EXTENSION_RUNTIME_RESOURCE_BYTES, MAX_EXTENSION_RUNTIME_RESOURCE_PATH_BYTES,
    MAX_EXTENSION_RUNTIME_RESOURCE_PATH_COMPONENT_BYTES, MAX_EXTENSION_RUNTIME_RESOURCE_PATH_DEPTH,
    MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES,
    MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_RETAINED_BYTES,
};
use zephium_private_fs::SealedPrivateDirectory;

use super::api::{
    ActiveBundledPackageLease, ActiveBundledPackageReleaseRequest, BundledPackageResourceError,
    PackageLeaseCore, RollbackBundledPackageLease, RollbackBundledPackageReleaseRequest,
};
use super::resource;
use crate::materialization::{VerifiedActivePackageSnapshot, VerifiedRollbackPackageSnapshot};
use crate::operation::{with_external_callback, RepositoryOperationError};

const _: () = assert!(MAX_EXTENSION_RUNTIME_RESOURCE_BYTES == MAX_EXTENSION_TREE_FILE_BYTES);
const _: () =
    assert!(MAX_EXTENSION_RUNTIME_MANIFEST_BYTES as usize == MAX_EXTENSION_MANIFEST_BYTES);

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
std::thread_local! {
    static PROVIDER_RETAINED_BYTES_OVERRIDE: Cell<usize> = const { Cell::new(0) };
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(super) fn arm_provider_retained_bytes_override(retained_bytes: usize) {
    assert_ne!(retained_bytes, 0);
    PROVIDER_RETAINED_BYTES_OVERRIDE.with(|slot| {
        assert_eq!(
            slot.replace(retained_bytes),
            0,
            "runtime access retained-byte hook was already armed"
        );
    });
}
const _: () = assert!(MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES == MAX_EXTENSION_TREE_FILES);
const _: () =
    assert!(MAX_EXTENSION_RUNTIME_RESOURCE_PATH_BYTES == MAX_EXTENSION_RELATIVE_PATH_BYTES);
const _: () = assert!(
    MAX_EXTENSION_RUNTIME_RESOURCE_PATH_COMPONENT_BYTES == MAX_EXTENSION_PATH_COMPONENT_BYTES
);
const _: () =
    assert!(MAX_EXTENSION_RUNTIME_RESOURCE_PATH_DEPTH == MAX_EXTENSION_RELATIVE_PATH_DEPTH);
const _: () = assert!(
    MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_RETAINED_BYTES == MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES
);

/// Why an authenticated package lease could not be delegated to the runtime API.
///
/// Variants contain no package identifiers or resource paths.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BundledRuntimePackageAccessBuildError {
    /// Product authority selected a runtime target unknown to this adapter.
    #[error("the authenticated extension runtime target is unsupported")]
    UnsupportedRuntimeTarget,
    /// A repository tree entry violated the mirrored runtime binding contract.
    #[error("an authenticated extension resource could not be bound at ordinal {ordinal}")]
    ResourceBinding {
        /// Bounded canonical inventory ordinal.
        ordinal: u32,
        /// Closed, path-free binding rejection.
        #[source]
        error: ExtensionRuntimeResourceBuildError,
    },
    /// The complete authenticated tree could not form a bounded runtime plan.
    #[error("the authenticated extension resource inventory could not be delegated")]
    ResourcePlan(#[source] ExtensionRuntimeResourcePlanBuildError),
    /// Runtime package-access accounting refused the delegated provider.
    #[error("the runtime package-access boundary refused the delegated lease")]
    PackageAccess(#[source] ExtensionPackageAccessBuildError),
    /// Repository and runtime bindings disagreed after construction.
    #[error("the delegated extension package binding was internally inconsistent")]
    InternalBindingMismatch,
}

/// Why returned runtime package access could not become an active release request.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum ActiveBundledRuntimePackageRecoveryError {
    /// The access was not created from this repository's active lease provider.
    #[error("runtime package access does not contain active package authority")]
    WrongProviderRole,
    /// The returned access no longer agreed with its captive provider binding.
    #[error("returned active runtime package access failed binding validation")]
    InternalBindingMismatch,
}

/// Why returned runtime package access could not become a rollback release request.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum RollbackBundledRuntimePackageRecoveryError {
    /// The access was not created from this repository's rollback lease provider.
    #[error("runtime package access does not contain rollback package authority")]
    WrongProviderRole,
    /// The returned access no longer agreed with its captive provider binding.
    #[error("returned rollback runtime package access failed binding validation")]
    InternalBindingMismatch,
}

enum ActiveBuildAuthority {
    Lease(ActiveBundledPackageLease),
    RuntimeRefusal {
        _refusal: ExtensionPackageAccessBuildRefusal,
    },
    BindingQuarantine {
        _target: ExtensionRuntimeTarget,
        _resources: ExtensionRuntimeResourcePlan,
        _provider: Box<ActiveLeasePackageAccessProvider>,
    },
}

/// Failed active-package delegation that keeps the exact lease authority captive.
#[must_use = "the refusal retains active package lease authority"]
pub struct ActiveBundledRuntimePackageAccessBuildRefusal {
    reason: BundledRuntimePackageAccessBuildError,
    authority: Box<ActiveBuildAuthority>,
}

impl ActiveBundledRuntimePackageAccessBuildRefusal {
    /// Returns the stable, path-free refusal reason.
    pub const fn reason(&self) -> BundledRuntimePackageAccessBuildError {
        self.reason
    }

    /// Recovers the original active lease when its exact identity is still proven.
    ///
    /// An internal type or binding mismatch returns this refusal unchanged and
    /// keeps authority captive instead of dropping, releasing, or unpinning it.
    pub fn try_into_lease(self) -> Result<ActiveBundledPackageLease, Self> {
        match *self.authority {
            ActiveBuildAuthority::Lease(lease) => Ok(lease),
            authority => Err(Self {
                reason: self.reason,
                authority: Box::new(authority),
            }),
        }
    }
}

impl fmt::Debug for ActiveBundledRuntimePackageAccessBuildRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActiveBundledRuntimePackageAccessBuildRefusal")
            .field("reason", &self.reason)
            .field("authority", &"[redacted]")
            .finish()
    }
}

enum RollbackBuildAuthority {
    Lease(RollbackBundledPackageLease),
    RuntimeRefusal {
        _refusal: ExtensionPackageAccessBuildRefusal,
    },
    BindingQuarantine {
        _target: ExtensionRuntimeTarget,
        _resources: ExtensionRuntimeResourcePlan,
        _provider: Box<RollbackLeasePackageAccessProvider>,
    },
}

/// Failed rollback-package delegation that keeps the exact lease authority captive.
#[must_use = "the refusal retains rollback package lease authority"]
pub struct RollbackBundledRuntimePackageAccessBuildRefusal {
    reason: BundledRuntimePackageAccessBuildError,
    authority: Box<RollbackBuildAuthority>,
}

impl RollbackBundledRuntimePackageAccessBuildRefusal {
    /// Returns the stable, path-free refusal reason.
    pub const fn reason(&self) -> BundledRuntimePackageAccessBuildError {
        self.reason
    }

    /// Recovers the original rollback lease when its exact identity is still proven.
    ///
    /// An internal type or binding mismatch returns this refusal unchanged and
    /// keeps authority captive instead of dropping, releasing, or unpinning it.
    pub fn try_into_lease(self) -> Result<RollbackBundledPackageLease, Self> {
        match *self.authority {
            RollbackBuildAuthority::Lease(lease) => Ok(lease),
            authority => Err(Self {
                reason: self.reason,
                authority: Box::new(authority),
            }),
        }
    }
}

impl fmt::Debug for RollbackBundledRuntimePackageAccessBuildRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RollbackBundledRuntimePackageAccessBuildRefusal")
            .field("reason", &self.reason)
            .field("authority", &"[redacted]")
            .finish()
    }
}

enum ActiveRecoveryAuthority {
    Access(ExtensionPackageAccess),
    BindingQuarantine {
        _provider: Box<ActiveLeasePackageAccessProvider>,
    },
}

/// Failed active-package recovery that never discards package authority.
#[must_use = "the refusal retains returned runtime package authority"]
pub struct ActiveBundledRuntimePackageRecoveryRefusal {
    reason: ActiveBundledRuntimePackageRecoveryError,
    authority: ActiveRecoveryAuthority,
}

impl ActiveBundledRuntimePackageRecoveryRefusal {
    /// Returns the stable recovery refusal reason.
    pub const fn reason(&self) -> ActiveBundledRuntimePackageRecoveryError {
        self.reason
    }

    /// Recovers the complete original runtime access after a role mismatch.
    ///
    /// A binding mismatch remains quarantined and returns this refusal unchanged.
    pub fn try_into_access(self) -> Result<ExtensionPackageAccess, Self> {
        match self.authority {
            ActiveRecoveryAuthority::Access(access) => Ok(access),
            authority => Err(Self {
                reason: self.reason,
                authority,
            }),
        }
    }
}

impl fmt::Debug for ActiveBundledRuntimePackageRecoveryRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActiveBundledRuntimePackageRecoveryRefusal")
            .field("reason", &self.reason)
            .field("authority", &"[redacted]")
            .finish()
    }
}

enum RollbackRecoveryAuthority {
    Access(ExtensionPackageAccess),
    BindingQuarantine {
        _provider: Box<RollbackLeasePackageAccessProvider>,
    },
}

/// Failed rollback-package recovery that never discards package authority.
#[must_use = "the refusal retains returned runtime package authority"]
pub struct RollbackBundledRuntimePackageRecoveryRefusal {
    reason: RollbackBundledRuntimePackageRecoveryError,
    authority: RollbackRecoveryAuthority,
}

impl RollbackBundledRuntimePackageRecoveryRefusal {
    /// Returns the stable recovery refusal reason.
    pub const fn reason(&self) -> RollbackBundledRuntimePackageRecoveryError {
        self.reason
    }

    /// Recovers the complete original runtime access after a role mismatch.
    ///
    /// A binding mismatch remains quarantined and returns this refusal unchanged.
    pub fn try_into_access(self) -> Result<ExtensionPackageAccess, Self> {
        match self.authority {
            RollbackRecoveryAuthority::Access(access) => Ok(access),
            authority => Err(Self {
                reason: self.reason,
                authority,
            }),
        }
    }
}

impl fmt::Debug for RollbackBundledRuntimePackageRecoveryRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RollbackBundledRuntimePackageRecoveryRefusal")
            .field("reason", &self.reason)
            .field("authority", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Copy)]
struct RuntimePackageBinding {
    target: ExtensionRuntimeTarget,
    product_target: ProductExtensionRuntimeTarget,
    plan_digest: [u8; 32],
    resource_count: u32,
}

impl RuntimePackageBinding {
    fn try_new<Snapshot: RuntimePackageSnapshot>(
        snapshot: &Snapshot,
        resources: &ExtensionRuntimeResourcePlan,
    ) -> Result<Self, BundledRuntimePackageAccessBuildError> {
        let target = runtime_target(snapshot.runtime_target())?;
        let resource_count = u32::try_from(resources.entries().len())
            .map_err(|_| BundledRuntimePackageAccessBuildError::InternalBindingMismatch)?;
        let binding = Self {
            target,
            product_target: snapshot.runtime_target(),
            plan_digest: resources.digest(),
            resource_count,
        };
        if !binding.matches_snapshot(snapshot) || !binding.matches_plan(target, resources) {
            return Err(BundledRuntimePackageAccessBuildError::InternalBindingMismatch);
        }
        Ok(binding)
    }

    fn matches_snapshot<Snapshot: RuntimePackageSnapshot>(&self, snapshot: &Snapshot) -> bool {
        snapshot.runtime_target() == self.product_target
            && runtime_target(snapshot.runtime_target()).ok() == Some(self.target)
            && usize::try_from(self.resource_count).ok() == Some(snapshot.index().files().len())
    }

    fn matches_plan(
        &self,
        target: ExtensionRuntimeTarget,
        resources: &ExtensionRuntimeResourcePlan,
    ) -> bool {
        self.target == target
            && self.plan_digest == resources.digest()
            && usize::try_from(self.resource_count).ok() == Some(resources.entries().len())
    }
}

trait RuntimePackageSnapshot {
    fn runtime_target(&self) -> ProductExtensionRuntimeTarget;
    fn index(&self) -> &CanonicalExtensionTreeIndex;
    fn root(&self) -> &Arc<SealedPrivateDirectory>;
}

macro_rules! impl_runtime_snapshot {
    ($snapshot:ty) => {
        impl RuntimePackageSnapshot for $snapshot {
            fn runtime_target(&self) -> ProductExtensionRuntimeTarget {
                self.runtime_target()
            }

            fn index(&self) -> &CanonicalExtensionTreeIndex {
                self.index()
            }

            fn root(&self) -> &Arc<SealedPrivateDirectory> {
                self.root()
            }
        }
    };
}

impl_runtime_snapshot!(VerifiedActivePackageSnapshot);
impl_runtime_snapshot!(VerifiedRollbackPackageSnapshot);

struct ActiveLeasePackageAccessProvider {
    lease: ActiveBundledPackageLease,
    binding: RuntimePackageBinding,
}

struct RollbackLeasePackageAccessProvider {
    lease: RollbackBundledPackageLease,
    binding: RuntimePackageBinding,
}

macro_rules! impl_runtime_provider {
    ($provider:ty, $lease:ty) => {
        impl $provider {
            fn new(lease: $lease, binding: RuntimePackageBinding) -> Self {
                Self { lease, binding }
            }

            fn into_lease(self) -> $lease {
                self.lease
            }

            fn matches_access(
                &self,
                target: ExtensionRuntimeTarget,
                resources: &ExtensionRuntimeResourcePlan,
            ) -> bool {
                self.binding.matches_plan(target, resources)
                    && self
                        .binding
                        .matches_snapshot(self.lease.core.snapshot.as_ref())
            }
        }

        impl ExtensionPackageAccessPort for $provider {
            fn retained_bytes(&self) -> usize {
                provider_retained_bytes::<Self, $lease>(self.lease.retained_bytes())
            }

            fn visit_resource(
                &mut self,
                descriptor: ExtensionRuntimeResource,
                visitor: &mut dyn ExtensionRuntimeResourceVisitor,
            ) -> Result<(), ExtensionPackageAccessError> {
                visit_resource(&self.lease.core, self.binding, descriptor, visitor)
            }

            fn visit_native_root(
                &mut self,
                target: ExtensionRuntimeTarget,
                visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
            ) -> Result<(), ExtensionPackageAccessError> {
                visit_native_root(&self.lease.core, self.binding, target, visitor)
            }
        }
    };
}

impl_runtime_provider!(ActiveLeasePackageAccessProvider, ActiveBundledPackageLease);
impl_runtime_provider!(
    RollbackLeasePackageAccessProvider,
    RollbackBundledPackageLease
);

impl ActiveBundledPackageLease {
    /// Delegates this authenticated active lease as bounded runtime package access.
    ///
    /// The complete canonical tree is bound once. The provider owns this lease,
    /// and its destructor remains passive; only recovery from returned access can
    /// produce role-correct durable release authority.
    pub fn into_runtime_package_access(
        self,
    ) -> Result<ExtensionPackageAccess, ActiveBundledRuntimePackageAccessBuildRefusal> {
        let resources = match build_resource_plan(self.core.snapshot.index()) {
            Ok(resources) => resources,
            Err(reason) => {
                return Err(ActiveBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(ActiveBuildAuthority::Lease(self)),
                });
            }
        };
        let binding = match RuntimePackageBinding::try_new(self.core.snapshot.as_ref(), &resources)
        {
            Ok(binding) => binding,
            Err(reason) => {
                return Err(ActiveBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(ActiveBuildAuthority::Lease(self)),
                });
            }
        };
        let target = binding.target;
        let provider = Box::new(ActiveLeasePackageAccessProvider::new(self, binding));
        match ExtensionPackageAccess::from_delegated_provider(target, resources, provider) {
            Ok(access) => Ok(access),
            Err(refusal) => Err(active_build_refusal(refusal)),
        }
    }
}

impl RollbackBundledPackageLease {
    /// Delegates this authenticated rollback lease as bounded runtime package access.
    ///
    /// Active and rollback providers are distinct concrete types, preventing a
    /// returned capability from crossing catalog-generation roles.
    pub fn into_runtime_package_access(
        self,
    ) -> Result<ExtensionPackageAccess, RollbackBundledRuntimePackageAccessBuildRefusal> {
        let resources = match build_resource_plan(self.core.snapshot.index()) {
            Ok(resources) => resources,
            Err(reason) => {
                return Err(RollbackBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(RollbackBuildAuthority::Lease(self)),
                });
            }
        };
        let binding = match RuntimePackageBinding::try_new(self.core.snapshot.as_ref(), &resources)
        {
            Ok(binding) => binding,
            Err(reason) => {
                return Err(RollbackBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(RollbackBuildAuthority::Lease(self)),
                });
            }
        };
        let target = binding.target;
        let provider = Box::new(RollbackLeasePackageAccessProvider::new(self, binding));
        match ExtensionPackageAccess::from_delegated_provider(target, resources, provider) {
            Ok(access) => Ok(access),
            Err(refusal) => Err(rollback_build_refusal(refusal)),
        }
    }
}

impl ActiveBundledPackageReleaseRequest {
    /// Recovers active durable release authority from runtime-returned package access.
    ///
    /// Wrong-role or foreign access is returned whole. An impossible internal
    /// binding mismatch remains captive and never releases or unpins authority.
    pub fn try_from_runtime_package_access(
        access: ExtensionPackageAccess,
    ) -> Result<Self, ActiveBundledRuntimePackageRecoveryRefusal> {
        let target = access.target();
        let plan_digest = access.resources().digest();
        let resource_count = access.resources().entries().len();
        match access.try_into_delegated_provider::<ActiveLeasePackageAccessProvider>() {
            Err(access) => Err(ActiveBundledRuntimePackageRecoveryRefusal {
                reason: ActiveBundledRuntimePackageRecoveryError::WrongProviderRole,
                authority: ActiveRecoveryAuthority::Access(access),
            }),
            Ok(provider) => {
                if provider.binding.target != target
                    || provider.binding.plan_digest != plan_digest
                    || usize::try_from(provider.binding.resource_count).ok() != Some(resource_count)
                    || !provider
                        .binding
                        .matches_snapshot(provider.lease.core.snapshot.as_ref())
                {
                    return Err(ActiveBundledRuntimePackageRecoveryRefusal {
                        reason: ActiveBundledRuntimePackageRecoveryError::InternalBindingMismatch,
                        authority: ActiveRecoveryAuthority::BindingQuarantine {
                            _provider: provider,
                        },
                    });
                }
                Ok(provider.into_lease().into_release_request())
            }
        }
    }
}

impl RollbackBundledPackageReleaseRequest {
    /// Recovers rollback durable release authority from runtime-returned package access.
    ///
    /// Wrong-role or foreign access is returned whole. An impossible internal
    /// binding mismatch remains captive and never releases or unpins authority.
    pub fn try_from_runtime_package_access(
        access: ExtensionPackageAccess,
    ) -> Result<Self, RollbackBundledRuntimePackageRecoveryRefusal> {
        let target = access.target();
        let plan_digest = access.resources().digest();
        let resource_count = access.resources().entries().len();
        match access.try_into_delegated_provider::<RollbackLeasePackageAccessProvider>() {
            Err(access) => Err(RollbackBundledRuntimePackageRecoveryRefusal {
                reason: RollbackBundledRuntimePackageRecoveryError::WrongProviderRole,
                authority: RollbackRecoveryAuthority::Access(access),
            }),
            Ok(provider) => {
                if provider.binding.target != target
                    || provider.binding.plan_digest != plan_digest
                    || usize::try_from(provider.binding.resource_count).ok() != Some(resource_count)
                    || !provider
                        .binding
                        .matches_snapshot(provider.lease.core.snapshot.as_ref())
                {
                    return Err(RollbackBundledRuntimePackageRecoveryRefusal {
                        reason: RollbackBundledRuntimePackageRecoveryError::InternalBindingMismatch,
                        authority: RollbackRecoveryAuthority::BindingQuarantine {
                            _provider: provider,
                        },
                    });
                }
                Ok(provider.into_lease().into_release_request())
            }
        }
    }
}

fn build_resource_plan(
    index: &CanonicalExtensionTreeIndex,
) -> Result<ExtensionRuntimeResourcePlan, BundledRuntimePackageAccessBuildError> {
    let mut bindings = Vec::with_capacity(index.files().len());
    for (ordinal, file) in index.files().iter().enumerate() {
        let binding = ExtensionRuntimeResourceBinding::try_new(
            file.path().as_str(),
            file.length(),
            file.sha256(),
        )
        .map_err(
            |error| BundledRuntimePackageAccessBuildError::ResourceBinding {
                ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
                error,
            },
        )?;
        bindings.push(binding);
    }
    ExtensionRuntimeResourcePlan::try_new(bindings)
        .map_err(BundledRuntimePackageAccessBuildError::ResourcePlan)
}

fn runtime_target(
    target: ProductExtensionRuntimeTarget,
) -> Result<ExtensionRuntimeTarget, BundledRuntimePackageAccessBuildError> {
    match target {
        ProductExtensionRuntimeTarget::MacosNative
        | ProductExtensionRuntimeTarget::WindowsNative => {
            Ok(ExtensionRuntimeTarget::NativeWebExtension)
        }
        ProductExtensionRuntimeTarget::MacosCompatibility
        | ProductExtensionRuntimeTarget::LinuxCompatibility => {
            Ok(ExtensionRuntimeTarget::Compatibility)
        }
        _ => Err(BundledRuntimePackageAccessBuildError::UnsupportedRuntimeTarget),
    }
}

fn provider_retained_bytes<Provider, Lease>(lease_retained_bytes: usize) -> usize {
    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    {
        let retained_bytes = PROVIDER_RETAINED_BYTES_OVERRIDE.with(|slot| slot.replace(0));
        if retained_bytes != 0 {
            return retained_bytes;
        }
    }
    size_of::<Provider>()
        .checked_sub(size_of::<Lease>())
        .and_then(|wrapper_bytes| lease_retained_bytes.checked_add(wrapper_bytes))
        .unwrap_or(usize::MAX)
}

fn visit_resource<Snapshot: RuntimePackageSnapshot>(
    core: &PackageLeaseCore<Snapshot>,
    binding: RuntimePackageBinding,
    descriptor: ExtensionRuntimeResource,
    visitor: &mut dyn ExtensionRuntimeResourceVisitor,
) -> Result<(), ExtensionPackageAccessError> {
    let operation = core.runtime.enter().map_err(map_operation_error)?;
    if !binding.matches_snapshot(core.snapshot.as_ref()) {
        core.runtime.poison();
        return Err(ExtensionPackageAccessError::ResourceIdentityMismatch);
    }
    let ordinal = usize::try_from(descriptor.ordinal())
        .map_err(|_| ExtensionPackageAccessError::ResourceNotDeclared)?;
    let file = core
        .snapshot
        .index()
        .files()
        .get(ordinal)
        .ok_or(ExtensionPackageAccessError::ResourceNotDeclared)?;
    let canonical_ordinal =
        u32::try_from(ordinal).map_err(|_| ExtensionPackageAccessError::ResourceNotDeclared)?;
    if !descriptor.authenticates(
        binding.plan_digest,
        canonical_ordinal,
        file.path().as_str(),
        file.length(),
        file.sha256(),
    ) {
        return Err(ExtensionPackageAccessError::ResourceNotDeclared);
    }

    let _callback_barrier = core
        .runtime
        .begin_delegated_callback()
        .map_err(map_operation_error)?;
    drop(operation);

    let result = resource::with_resource(
        &core.runtime,
        core.snapshot.root(),
        core.snapshot.index(),
        file.path(),
        |reader: &mut dyn Read| visitor.visit(reader),
    )
    .map_err(map_resource_error);
    if !binding.matches_snapshot(core.snapshot.as_ref()) {
        core.runtime.poison();
        return Err(ExtensionPackageAccessError::ResourceIdentityMismatch);
    }
    match result {
        Ok(_visitor_result) if core.runtime.is_healthy() => Ok(()),
        Ok(_visitor_result) => Err(ExtensionPackageAccessError::Inactive),
        Err(error) => Err(error),
    }
}

fn visit_native_root<Snapshot: RuntimePackageSnapshot>(
    core: &PackageLeaseCore<Snapshot>,
    binding: RuntimePackageBinding,
    target: ExtensionRuntimeTarget,
    visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
) -> Result<(), ExtensionPackageAccessError> {
    let operation = core.runtime.enter().map_err(map_operation_error)?;
    if target != binding.target || !binding.matches_snapshot(core.snapshot.as_ref()) {
        core.runtime.poison();
        return Err(ExtensionPackageAccessError::NativeRootIdentityMismatch);
    }
    if target != ExtensionRuntimeTarget::NativeWebExtension {
        return Err(ExtensionPackageAccessError::NativeRootUnavailable);
    }

    let _callback_barrier = core
        .runtime
        .begin_delegated_callback()
        .map_err(map_operation_error)?;
    drop(operation);

    let result = core
        .snapshot
        .root()
        .with_verified_path(|path| with_external_callback(|| visitor.visit(path)));
    if !binding.matches_snapshot(core.snapshot.as_ref()) || !core.runtime.is_healthy() {
        core.runtime.poison();
        return Err(ExtensionPackageAccessError::NativeRootIdentityMismatch);
    }
    match result {
        Ok(_visitor_result) => Ok(()),
        Err(_error) => {
            core.runtime.poison();
            Err(ExtensionPackageAccessError::NativeRootIdentityMismatch)
        }
    }
}

const fn map_operation_error(error: RepositoryOperationError) -> ExtensionPackageAccessError {
    match error {
        RepositoryOperationError::CallbackReentry => ExtensionPackageAccessError::CallbackReentry,
        RepositoryOperationError::Unhealthy | RepositoryOperationError::Poisoned => {
            ExtensionPackageAccessError::Inactive
        }
    }
}

const fn map_resource_error(error: BundledPackageResourceError) -> ExtensionPackageAccessError {
    match error {
        BundledPackageResourceError::LeaseInactive => ExtensionPackageAccessError::Inactive,
        BundledPackageResourceError::ResourceNotDeclared => {
            ExtensionPackageAccessError::ResourceNotDeclared
        }
        BundledPackageResourceError::ResourceReadUnavailable => {
            ExtensionPackageAccessError::ResourceUnavailable
        }
        BundledPackageResourceError::DurableResourceMismatch
        | BundledPackageResourceError::NamespaceQuarantined => {
            ExtensionPackageAccessError::ResourceIdentityMismatch
        }
        BundledPackageResourceError::CallbackReentry => {
            ExtensionPackageAccessError::CallbackReentry
        }
    }
}

fn active_build_refusal(
    refusal: ExtensionPackageAccessBuildRefusal,
) -> ActiveBundledRuntimePackageAccessBuildRefusal {
    let reason = BundledRuntimePackageAccessBuildError::PackageAccess(refusal.reason());
    match refusal.try_into_delegated_provider::<ActiveLeasePackageAccessProvider>() {
        Err(refusal) => ActiveBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(ActiveBuildAuthority::RuntimeRefusal { _refusal: refusal }),
        },
        Ok((target, resources, provider)) if provider.matches_access(target, &resources) => {
            ActiveBundledRuntimePackageAccessBuildRefusal {
                reason,
                authority: Box::new(ActiveBuildAuthority::Lease(provider.into_lease())),
            }
        }
        Ok((target, resources, provider)) => ActiveBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(ActiveBuildAuthority::BindingQuarantine {
                _target: target,
                _resources: resources,
                _provider: provider,
            }),
        },
    }
}

fn rollback_build_refusal(
    refusal: ExtensionPackageAccessBuildRefusal,
) -> RollbackBundledRuntimePackageAccessBuildRefusal {
    let reason = BundledRuntimePackageAccessBuildError::PackageAccess(refusal.reason());
    match refusal.try_into_delegated_provider::<RollbackLeasePackageAccessProvider>() {
        Err(refusal) => RollbackBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(RollbackBuildAuthority::RuntimeRefusal { _refusal: refusal }),
        },
        Ok((target, resources, provider)) if provider.matches_access(target, &resources) => {
            RollbackBundledRuntimePackageAccessBuildRefusal {
                reason,
                authority: Box::new(RollbackBuildAuthority::Lease(provider.into_lease())),
            }
        }
        Ok((target, resources, provider)) => RollbackBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(RollbackBuildAuthority::BindingQuarantine {
                _target: target,
                _resources: resources,
                _provider: provider,
            }),
        },
    }
}
