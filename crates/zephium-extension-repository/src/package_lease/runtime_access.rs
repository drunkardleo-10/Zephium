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
use zephium_core::extensions::{
    ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest, ExtensionNativeIncarnation,
    ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipKey, ExtensionNativeOwnershipOperation,
    ExtensionPackagePinAcquisitionBinding, ExtensionPackagePinHeldBinding,
    ExtensionPackagePinRecombineRefusal, ExtensionRuntimeBackendTarget,
    ExtensionRuntimeFingerprint, ExtensionRuntimeGeneration, ExtensionRuntimeOperationAuthority,
};
use zephium_extension_authority::ProductExtensionRuntimeTarget;
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, ChromiumManifestKey, MAX_EXTENSION_MANIFEST_BYTES,
    MAX_EXTENSION_PATH_COMPONENT_BYTES, MAX_EXTENSION_RELATIVE_PATH_BYTES,
    MAX_EXTENSION_RELATIVE_PATH_DEPTH, MAX_EXTENSION_TREE_FILES, MAX_EXTENSION_TREE_FILE_BYTES,
    MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES,
};
use zephium_extension_runtime_api::{
    ExtensionPackageAccess, ExtensionPackageAccessBuildError, ExtensionPackageAccessBuildRefusal,
    ExtensionPackageAccessError, ExtensionPackageAccessPort, ExtensionRuntimeHostActivation,
    ExtensionRuntimeHostActivationBinding, ExtensionRuntimeHostActivationBindingError,
    ExtensionRuntimeHostBindError, ExtensionRuntimeHostFactory,
    ExtensionRuntimeNativeIdentityExpectation, ExtensionRuntimeNativeOwnerId,
    ExtensionRuntimeNativeRootLeasePort, ExtensionRuntimeNativeRootVisitor,
    ExtensionRuntimeResource, ExtensionRuntimeResourceBinding, ExtensionRuntimeResourceBuildError,
    ExtensionRuntimeResourcePlan, ExtensionRuntimeResourcePlanBuildError,
    ExtensionRuntimeResourceVisitor, ExtensionRuntimeTarget,
    EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES, MAX_EXTENSION_RUNTIME_MANIFEST_BYTES,
    MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES, MAX_EXTENSION_RUNTIME_RESOURCE_BYTES,
    MAX_EXTENSION_RUNTIME_RESOURCE_PATH_BYTES, MAX_EXTENSION_RUNTIME_RESOURCE_PATH_COMPONENT_BYTES,
    MAX_EXTENSION_RUNTIME_RESOURCE_PATH_DEPTH, MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES,
    MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_RETAINED_BYTES,
};
use zephium_private_fs::SealedPrivateDirectory;

use super::api::{
    ActiveBundledPackageLease, ActiveBundledPackageReleaseRequest, BundledPackageResourceError,
    PackageLeaseCore, RollbackBundledPackageLease, RollbackBundledPackageReleaseRequest,
};
use super::resource;
use super::runtime::{LeasePresence, LeasePresenceBinding, RepositoryOpenEpoch};
use crate::materialization::{
    OwnerPackagePinIdentity, PackageLeaseRepositoryIdentity, VerifiedActivePackageSnapshot,
    VerifiedRollbackPackageSnapshot,
};
use crate::operation::{with_external_callback, RepositoryOperationError, RepositoryRuntime};
use crate::BundledCatalogSetIdentity;

const RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES: usize = 2 * size_of::<usize>();
const RETAINED_ARC_COUNTER_BYTES: usize = 2 * size_of::<usize>();
const _: () = assert!(MAX_EXTENSION_RUNTIME_RESOURCE_BYTES == MAX_EXTENSION_TREE_FILE_BYTES);
const _: () =
    assert!(MAX_EXTENSION_RUNTIME_MANIFEST_BYTES as usize == MAX_EXTENSION_MANIFEST_BYTES);
const _: () = assert!(EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES == 32);

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
    /// Combined package and operation authority accounting overflowed.
    #[error("the pre-host extension runtime authority accounting overflowed")]
    RetainedBytesOverflow,
    /// Combined package and operation authority exceed the per-owner ceiling.
    #[error("the pre-host extension runtime authority exceeds the retained-memory limit")]
    RetainedBytesExceeded,
    /// A native target has no catalog-authenticated manifest-key identity.
    #[error("the authenticated extension package has no native identity")]
    NativeIdentityUnavailable,
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

/// Why authenticated repository authority could not join a host activation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BundledRuntimeHostActivationBindingError {
    /// The supplied durable row does not continue the exact held repository-pin
    /// lineage, including its catalog role, set, operation, and incarnation.
    #[error("the native ownership row does not match the authenticated package pin")]
    RepositoryBindingMismatch,
    /// The runtime host rejected the otherwise exact atomic binding.
    #[error("the native runtime host rejected activation authority")]
    RuntimeHost(#[source] ExtensionRuntimeHostActivationBindingError),
    /// The trusted engine factory refused to reserve the exact host owner.
    #[error("the native runtime host factory refused activation authority")]
    RuntimeHostFactory(#[source] ExtensionRuntimeHostBindError),
    /// Aggregate host and repository recovery accounting overflowed.
    #[error("the complete extension runtime owner accounting overflowed")]
    RetainedBytesOverflow,
    /// Aggregate host and repository recovery state exceed the owner ceiling.
    #[error("the complete extension runtime owner exceeds the retained-memory limit")]
    RetainedBytesExceeded,
}

/// Atomic active-generation runtime package and operation authority.
///
/// The repository constructs this value only after fresh product admission,
/// exact durable-pin acquisition, complete resource-plan binding, and native
/// identity projection have all succeeded. Package access and Store-derived
/// operation authority cannot be extracted independently; a trusted host
/// binding must consume the complete value in one step.
///
/// ```compile_fail
/// use zephium_extension_repository::ActiveBundledRuntimePackageAccess;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ActiveBundledRuntimePackageAccess>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::ActiveBundledRuntimePackageAccess;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<ActiveBundledRuntimePackageAccess>();
/// ```
///
/// Active authority cannot cross into rollback release settlement:
///
/// ```compile_fail
/// use zephium_extension_repository::{
///     ActiveBundledRuntimePackageAccess, RollbackBundledPackageReleaseRequest,
/// };
/// fn cannot_cross_role(access: ActiveBundledRuntimePackageAccess) {
///     let _ = RollbackBundledPackageReleaseRequest::try_from_runtime_package_access(access);
/// }
/// ```
#[must_use = "active runtime package authority must settle or remain pinned for cleanup"]
pub struct ActiveBundledRuntimePackageAccess {
    access: ExtensionPackageAccess,
    operation_authority: ExtensionRuntimeOperationAuthority,
    binding: RuntimePackageBinding,
}

/// Atomic rollback-generation runtime package and operation authority.
///
/// This is nominally distinct from active authority. A returned rollback
/// value can become only a rollback release request, preventing catalog-role
/// confusion at the runtime settlement boundary.
///
/// ```compile_fail
/// use zephium_extension_repository::RollbackBundledRuntimePackageAccess;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<RollbackBundledRuntimePackageAccess>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::RollbackBundledRuntimePackageAccess;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<RollbackBundledRuntimePackageAccess>();
/// ```
///
/// Rollback authority cannot cross into active release settlement:
///
/// ```compile_fail
/// use zephium_extension_repository::{
///     ActiveBundledPackageReleaseRequest, RollbackBundledRuntimePackageAccess,
/// };
/// fn cannot_cross_role(access: RollbackBundledRuntimePackageAccess) {
///     let _ = ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(access);
/// }
/// ```
#[must_use = "rollback runtime package authority must settle or remain pinned for cleanup"]
pub struct RollbackBundledRuntimePackageAccess {
    access: ExtensionPackageAccess,
    operation_authority: ExtensionRuntimeOperationAuthority,
    binding: RuntimePackageBinding,
}

/// Move-only active-role recovery authority retained across native host work.
///
/// A successful host assembly returns this token beside the host binding. The
/// native lifecycle may temporarily separate package access from operation
/// authority, but only this exact token can join their returned values back to
/// the active repository provider and produce active release authority.
///
/// ```compile_fail
/// use zephium_extension_repository::ActiveBundledRuntimePackageRecoveryToken;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ActiveBundledRuntimePackageRecoveryToken>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::ActiveBundledRuntimePackageRecoveryToken;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<ActiveBundledRuntimePackageRecoveryToken>();
/// ```
#[must_use = "active recovery authority must remain paired with its native host owner"]
pub struct ActiveBundledRuntimePackageRecoveryToken {
    binding: RuntimePackageBinding,
}

/// Move-only rollback-role recovery authority retained across native host work.
///
/// This token is nominally distinct from active recovery authority. It can
/// produce only a rollback release request after the exact package access and
/// operation authority return from proven native absence.
///
/// ```compile_fail
/// use zephium_extension_repository::RollbackBundledRuntimePackageRecoveryToken;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<RollbackBundledRuntimePackageRecoveryToken>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::RollbackBundledRuntimePackageRecoveryToken;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<RollbackBundledRuntimePackageRecoveryToken>();
/// ```
#[must_use = "rollback recovery authority must remain paired with its native host owner"]
pub struct RollbackBundledRuntimePackageRecoveryToken {
    binding: RuntimePackageBinding,
}

/// Atomic active-role host activation and repository recovery authority.
///
/// Construction accounts for the engine activation's largest future control
/// state together with the role-specific repository recovery token. The two
/// capabilities can be separated only after that aggregate admission succeeds.
///
/// ```compile_fail
/// use zephium_extension_repository::ActiveBundledRuntimeHostActivation;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ActiveBundledRuntimeHostActivation>();
/// ```
#[must_use = "active host activation and recovery authority must settle together"]
pub struct ActiveBundledRuntimeHostActivation {
    activation: ExtensionRuntimeHostActivation,
    recovery: ActiveBundledRuntimePackageRecoveryToken,
}

/// Atomic rollback-role host activation and repository recovery authority.
///
/// This value is nominally distinct from active host authority and can yield
/// only a rollback recovery token after aggregate owner admission succeeds.
///
/// ```compile_fail
/// use zephium_extension_repository::RollbackBundledRuntimeHostActivation;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<RollbackBundledRuntimeHostActivation>();
/// ```
#[must_use = "rollback host activation and recovery authority must settle together"]
pub struct RollbackBundledRuntimeHostActivation {
    activation: ExtensionRuntimeHostActivation,
    recovery: RollbackBundledRuntimePackageRecoveryToken,
}

macro_rules! impl_runtime_package_recovery_token_debug {
    ($token:ident) => {
        impl $token {
            /// Conservative inline charge for this exact recovery token.
            #[must_use]
            pub const fn retained_bytes(&self) -> usize {
                size_of::<Self>()
            }
        }

        impl fmt::Debug for $token {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($token))
                    .field("authority", &"[redacted]")
                    .finish()
            }
        }
    };
}

impl_runtime_package_recovery_token_debug!(ActiveBundledRuntimePackageRecoveryToken);
impl_runtime_package_recovery_token_debug!(RollbackBundledRuntimePackageRecoveryToken);

macro_rules! impl_runtime_host_activation {
    ($activation:ident, $token:ident) => {
        impl $activation {
            /// Returns the conservative charge of the current atomic host and
            /// repository recovery state.
            #[must_use]
            pub fn retained_bytes(&self) -> usize {
                self.activation
                    .retained_bytes()
                    .saturating_add(self.recovery.retained_bytes())
            }

            /// Returns the admitted maximum charge across every future host
            /// control state while the recovery token remains live.
            #[must_use]
            pub fn maximum_future_retained_bytes(&self) -> usize {
                self.activation
                    .maximum_future_retained_bytes()
                    .saturating_add(self.recovery.retained_bytes())
            }

            /// Separates the already-admitted engine activation from its exact
            /// role-specific repository recovery token.
            #[must_use = "both values are required to settle native ownership safely"]
            pub fn into_parts(self) -> (ExtensionRuntimeHostActivation, $token) {
                (self.activation, self.recovery)
            }
        }

        impl fmt::Debug for $activation {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($activation))
                    .field("authority", &"[redacted]")
                    .finish()
            }
        }
    };
}

impl_runtime_host_activation!(
    ActiveBundledRuntimeHostActivation,
    ActiveBundledRuntimePackageRecoveryToken
);
impl_runtime_host_activation!(
    RollbackBundledRuntimeHostActivation,
    RollbackBundledRuntimePackageRecoveryToken
);

/// Lossless refusal to rejoin active package and operation authority for release.
#[must_use = "the refusal retains active recovery, package, and operation authority"]
pub struct ActiveBundledRuntimePackageRejoinRefusal {
    refusal: ActiveBundledRuntimePackageRecoveryRefusal,
}

impl ActiveBundledRuntimePackageRejoinRefusal {
    /// Returns the stable recovery refusal reason.
    pub const fn reason(&self) -> ActiveBundledRuntimePackageRecoveryError {
        self.refusal.reason()
    }

    /// Recovers every exact input after an ordinary role/provider mismatch.
    ///
    /// An internal binding or Core recombination mismatch remains quarantined
    /// and returns this refusal unchanged.
    pub fn try_into_parts(
        self,
    ) -> Result<
        (
            ActiveBundledRuntimePackageRecoveryToken,
            ExtensionPackageAccess,
            ExtensionRuntimeOperationAuthority,
        ),
        Self,
    > {
        match self.refusal.try_into_access() {
            Ok(access) => {
                let ActiveBundledRuntimePackageAccess {
                    access,
                    operation_authority,
                    binding,
                } = access;
                Ok((
                    ActiveBundledRuntimePackageRecoveryToken { binding },
                    access,
                    operation_authority,
                ))
            }
            Err(refusal) => Err(Self { refusal }),
        }
    }

    /// Whether this refusal is a non-retryable internal authority mismatch.
    #[must_use]
    pub fn requires_fail_stop(&self) -> bool {
        self.refusal.requires_fail_stop()
    }
}

impl fmt::Debug for ActiveBundledRuntimePackageRejoinRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActiveBundledRuntimePackageRejoinRefusal")
            .field("reason", &self.reason())
            .field("authority", &"[redacted]")
            .finish()
    }
}

/// Lossless refusal to rejoin rollback package and operation authority for release.
#[must_use = "the refusal retains rollback recovery, package, and operation authority"]
pub struct RollbackBundledRuntimePackageRejoinRefusal {
    refusal: RollbackBundledRuntimePackageRecoveryRefusal,
}

impl RollbackBundledRuntimePackageRejoinRefusal {
    /// Returns the stable recovery refusal reason.
    pub const fn reason(&self) -> RollbackBundledRuntimePackageRecoveryError {
        self.refusal.reason()
    }

    /// Recovers every exact input after an ordinary role/provider mismatch.
    ///
    /// An internal binding or Core recombination mismatch remains quarantined
    /// and returns this refusal unchanged.
    pub fn try_into_parts(
        self,
    ) -> Result<
        (
            RollbackBundledRuntimePackageRecoveryToken,
            ExtensionPackageAccess,
            ExtensionRuntimeOperationAuthority,
        ),
        Self,
    > {
        match self.refusal.try_into_access() {
            Ok(access) => {
                let RollbackBundledRuntimePackageAccess {
                    access,
                    operation_authority,
                    binding,
                } = access;
                Ok((
                    RollbackBundledRuntimePackageRecoveryToken { binding },
                    access,
                    operation_authority,
                ))
            }
            Err(refusal) => Err(Self { refusal }),
        }
    }

    /// Whether this refusal is a non-retryable internal authority mismatch.
    #[must_use]
    pub fn requires_fail_stop(&self) -> bool {
        self.refusal.requires_fail_stop()
    }
}

impl fmt::Debug for RollbackBundledRuntimePackageRejoinRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RollbackBundledRuntimePackageRejoinRefusal")
            .field("reason", &self.reason())
            .field("authority", &"[redacted]")
            .finish()
    }
}

enum ActiveHostBindingRefusalAuthority {
    Recoverable {
        access: ActiveBundledRuntimePackageAccess,
        entry: ExtensionNativeOwnershipEntry,
    },
    Quarantined {
        _entry: ExtensionNativeOwnershipEntry,
        _access: ExtensionPackageAccess,
        _operation_authority: ExtensionRuntimeOperationAuthority,
        _expectation: ExtensionRuntimeNativeIdentityExpectation,
        _binding: RuntimePackageBinding,
    },
}

/// Lossless refusal to join active package authority to a native host activation.
///
/// Ordinary host validation failures return the same nominal active capability
/// and durable row through [`Self::try_into_access_and_entry`]. An impossible
/// contract mismatch keeps every authority component captive instead of
/// guessing that the repository provider can be safely reconstructed.
#[must_use = "the refusal retains active package and operation authority"]
pub struct ActiveBundledRuntimeHostActivationBindingRefusal {
    reason: BundledRuntimeHostActivationBindingError,
    authority: Box<ActiveHostBindingRefusalAuthority>,
}

impl ActiveBundledRuntimeHostActivationBindingRefusal {
    /// Returns the stable host-binding refusal reason.
    pub const fn reason(&self) -> BundledRuntimeHostActivationBindingError {
        self.reason
    }

    /// Recovers the exact active capability and supplied ownership row.
    ///
    /// A violated runtime-API return contract leaves the refusal quarantined
    /// and returns it unchanged.
    pub fn try_into_access_and_entry(
        self,
    ) -> Result<
        (
            ActiveBundledRuntimePackageAccess,
            ExtensionNativeOwnershipEntry,
        ),
        Self,
    > {
        match *self.authority {
            ActiveHostBindingRefusalAuthority::Recoverable { access, entry } => Ok((access, entry)),
            authority => Err(Self {
                reason: self.reason,
                authority: Box::new(authority),
            }),
        }
    }
}

impl fmt::Debug for ActiveBundledRuntimeHostActivationBindingRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActiveBundledRuntimeHostActivationBindingRefusal")
            .field("reason", &self.reason)
            .field("authority", &"[redacted]")
            .finish()
    }
}

enum RollbackHostBindingRefusalAuthority {
    Recoverable {
        access: RollbackBundledRuntimePackageAccess,
        entry: ExtensionNativeOwnershipEntry,
    },
    Quarantined {
        _entry: ExtensionNativeOwnershipEntry,
        _access: ExtensionPackageAccess,
        _operation_authority: ExtensionRuntimeOperationAuthority,
        _expectation: ExtensionRuntimeNativeIdentityExpectation,
        _binding: RuntimePackageBinding,
    },
}

/// Lossless refusal to join rollback package authority to a native host activation.
///
/// The nominal rollback role is preserved across every recoverable refusal and
/// cannot be substituted with active-generation release authority.
#[must_use = "the refusal retains rollback package and operation authority"]
pub struct RollbackBundledRuntimeHostActivationBindingRefusal {
    reason: BundledRuntimeHostActivationBindingError,
    authority: Box<RollbackHostBindingRefusalAuthority>,
}

impl RollbackBundledRuntimeHostActivationBindingRefusal {
    /// Returns the stable host-binding refusal reason.
    pub const fn reason(&self) -> BundledRuntimeHostActivationBindingError {
        self.reason
    }

    /// Recovers the exact rollback capability and supplied ownership row.
    ///
    /// A violated runtime-API return contract leaves the refusal quarantined
    /// and returns it unchanged.
    pub fn try_into_access_and_entry(
        self,
    ) -> Result<
        (
            RollbackBundledRuntimePackageAccess,
            ExtensionNativeOwnershipEntry,
        ),
        Self,
    > {
        match *self.authority {
            RollbackHostBindingRefusalAuthority::Recoverable { access, entry } => {
                Ok((access, entry))
            }
            authority => Err(Self {
                reason: self.reason,
                authority: Box::new(authority),
            }),
        }
    }
}

impl fmt::Debug for RollbackBundledRuntimeHostActivationBindingRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RollbackBundledRuntimeHostActivationBindingRefusal")
            .field("reason", &self.reason)
            .field("authority", &"[redacted]")
            .finish()
    }
}

macro_rules! impl_runtime_package_access {
    ($access:ident) => {
        impl $access {
            /// Conservative total charge for package, operation, and wrapper
            /// authority retained before native host assembly.
            #[must_use]
            pub fn retained_bytes(&self) -> usize {
                pre_host_runtime_access_retained_bytes::<Self>(
                    &self.access,
                    &self.operation_authority,
                )
                .unwrap_or(usize::MAX)
            }
        }

        impl fmt::Debug for $access {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($access))
                    .field("authority", &"[redacted]")
                    .finish()
            }
        }
    };
}

impl_runtime_package_access!(ActiveBundledRuntimePackageAccess);
impl_runtime_package_access!(RollbackBundledRuntimePackageAccess);

impl ActiveBundledRuntimePackageAccess {
    /// Atomically joins this active package capability to the exact Store row
    /// and trusted engine factory without invoking a native lifecycle method.
    /// The row is still identityless: the catalog-derived ID is an exact
    /// expectation that the native adapter must assign/read back before Store
    /// publication.
    ///
    /// Factory proxy construction is provisional and side-effect-free. Both
    /// the transient factory state and the engine's maximum future control
    /// charge are admitted together with the repository recovery token. Every
    /// refusal occurs before a lifecycle or publication call and either
    /// reconstructs this same nominal active capability with the exact supplied
    /// row or keeps every returned authority component quarantined after an
    /// impossible lossless-return violation.
    pub fn try_into_host_activation(
        self,
        entry: ExtensionNativeOwnershipEntry,
        factory: &mut ExtensionRuntimeHostFactory,
    ) -> Result<ActiveBundledRuntimeHostActivation, ActiveBundledRuntimeHostActivationBindingRefusal>
    {
        let Self {
            access,
            operation_authority,
            binding,
        } = self;
        if !binding.matches_ownership_entry(&entry, &operation_authority) {
            return Err(ActiveBundledRuntimeHostActivationBindingRefusal {
                reason: BundledRuntimeHostActivationBindingError::RepositoryBindingMismatch,
                authority: Box::new(ActiveHostBindingRefusalAuthority::Recoverable {
                    access: Self {
                        access,
                        operation_authority,
                        binding,
                    },
                    entry,
                }),
            });
        }
        let expected_entry = entry.clone();
        let expected_fingerprint = operation_authority.fingerprint().clone();
        let host_binding =
            match ExtensionRuntimeHostActivationBinding::try_from_authenticated_repository(
                entry,
                access,
                operation_authority,
                binding.native_identity,
            ) {
                Ok(host_binding) => host_binding,
                Err(refusal) => {
                    let reason =
                        BundledRuntimeHostActivationBindingError::RuntimeHost(refusal.reason());
                    let (entry, access, operation_authority, expectation) = refusal.into_parts();
                    return Err(active_host_activation_refusal_from_parts(
                        reason,
                        binding,
                        &expected_entry,
                        &expected_fingerprint,
                        entry,
                        access,
                        operation_authority,
                        expectation,
                    ));
                }
            };
        let activation = match factory.bind_activation_with_companion_retained_bytes(
            host_binding,
            size_of::<ActiveBundledRuntimePackageRecoveryToken>(),
        ) {
            Ok(activation) => activation,
            Err(refusal) => {
                let reason =
                    BundledRuntimeHostActivationBindingError::RuntimeHostFactory(refusal.reason());
                let (entry, access, operation_authority, expectation) = refusal.cancel_into_parts();
                return Err(active_host_activation_refusal_from_parts(
                    reason,
                    binding,
                    &expected_entry,
                    &expected_fingerprint,
                    entry,
                    access,
                    operation_authority,
                    expectation,
                ));
            }
        };
        let recovery = ActiveBundledRuntimePackageRecoveryToken { binding };
        let reason = match activation
            .maximum_future_retained_bytes()
            .checked_add(recovery.retained_bytes())
        {
            None => Some(BundledRuntimeHostActivationBindingError::RetainedBytesOverflow),
            Some(value) if value > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES => {
                Some(BundledRuntimeHostActivationBindingError::RetainedBytesExceeded)
            }
            Some(_) => None,
        };
        if let Some(reason) = reason {
            let (entry, access, operation_authority, expectation) =
                activation.cancel_before_attempt();
            return Err(active_host_activation_refusal_from_parts(
                reason,
                binding,
                &expected_entry,
                &expected_fingerprint,
                entry,
                access,
                operation_authority,
                expectation,
            ));
        }
        Ok(ActiveBundledRuntimeHostActivation {
            activation,
            recovery,
        })
    }
}

impl RollbackBundledRuntimePackageAccess {
    /// Atomically joins this rollback package capability to the exact Store row
    /// and trusted engine factory without invoking a native lifecycle method.
    /// The row is still identityless: the catalog-derived ID is an exact
    /// expectation that the native adapter must assign/read back before Store
    /// publication.
    ///
    /// A recoverable refusal preserves the rollback nominal role. An impossible
    /// lossless-return mismatch remains captive and cannot be converted into a
    /// release request for either catalog role.
    pub fn try_into_host_activation(
        self,
        entry: ExtensionNativeOwnershipEntry,
        factory: &mut ExtensionRuntimeHostFactory,
    ) -> Result<
        RollbackBundledRuntimeHostActivation,
        RollbackBundledRuntimeHostActivationBindingRefusal,
    > {
        let Self {
            access,
            operation_authority,
            binding,
        } = self;
        if !binding.matches_ownership_entry(&entry, &operation_authority) {
            return Err(RollbackBundledRuntimeHostActivationBindingRefusal {
                reason: BundledRuntimeHostActivationBindingError::RepositoryBindingMismatch,
                authority: Box::new(RollbackHostBindingRefusalAuthority::Recoverable {
                    access: Self {
                        access,
                        operation_authority,
                        binding,
                    },
                    entry,
                }),
            });
        }
        let expected_entry = entry.clone();
        let expected_fingerprint = operation_authority.fingerprint().clone();
        let host_binding =
            match ExtensionRuntimeHostActivationBinding::try_from_authenticated_repository(
                entry,
                access,
                operation_authority,
                binding.native_identity,
            ) {
                Ok(host_binding) => host_binding,
                Err(refusal) => {
                    let reason =
                        BundledRuntimeHostActivationBindingError::RuntimeHost(refusal.reason());
                    let (entry, access, operation_authority, expectation) = refusal.into_parts();
                    return Err(rollback_host_activation_refusal_from_parts(
                        reason,
                        binding,
                        &expected_entry,
                        &expected_fingerprint,
                        entry,
                        access,
                        operation_authority,
                        expectation,
                    ));
                }
            };
        let activation = match factory.bind_activation_with_companion_retained_bytes(
            host_binding,
            size_of::<RollbackBundledRuntimePackageRecoveryToken>(),
        ) {
            Ok(activation) => activation,
            Err(refusal) => {
                let reason =
                    BundledRuntimeHostActivationBindingError::RuntimeHostFactory(refusal.reason());
                let (entry, access, operation_authority, expectation) = refusal.cancel_into_parts();
                return Err(rollback_host_activation_refusal_from_parts(
                    reason,
                    binding,
                    &expected_entry,
                    &expected_fingerprint,
                    entry,
                    access,
                    operation_authority,
                    expectation,
                ));
            }
        };
        let recovery = RollbackBundledRuntimePackageRecoveryToken { binding };
        let reason = match activation
            .maximum_future_retained_bytes()
            .checked_add(recovery.retained_bytes())
        {
            None => Some(BundledRuntimeHostActivationBindingError::RetainedBytesOverflow),
            Some(value) if value > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES => {
                Some(BundledRuntimeHostActivationBindingError::RetainedBytesExceeded)
            }
            Some(_) => None,
        };
        if let Some(reason) = reason {
            let (entry, access, operation_authority, expectation) =
                activation.cancel_before_attempt();
            return Err(rollback_host_activation_refusal_from_parts(
                reason,
                binding,
                &expected_entry,
                &expected_fingerprint,
                entry,
                access,
                operation_authority,
                expectation,
            ));
        }
        Ok(RollbackBundledRuntimeHostActivation {
            activation,
            recovery,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn active_host_activation_refusal_from_parts(
    reason: BundledRuntimeHostActivationBindingError,
    binding: RuntimePackageBinding,
    expected_entry: &ExtensionNativeOwnershipEntry,
    expected_fingerprint: &ExtensionRuntimeFingerprint,
    entry: ExtensionNativeOwnershipEntry,
    access: ExtensionPackageAccess,
    operation_authority: ExtensionRuntimeOperationAuthority,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
) -> ActiveBundledRuntimeHostActivationBindingRefusal {
    let returned_exactly = &entry == expected_entry
        && expectation == binding.native_identity
        && operation_authority.fingerprint() == expected_fingerprint
        && operation_authority.matches_native_ownership_lineage(&entry)
        && binding.matches_plan(access.target(), access.resources());
    let authority = if returned_exactly {
        ActiveHostBindingRefusalAuthority::Recoverable {
            access: ActiveBundledRuntimePackageAccess {
                access,
                operation_authority,
                binding,
            },
            entry,
        }
    } else {
        ActiveHostBindingRefusalAuthority::Quarantined {
            _entry: entry,
            _access: access,
            _operation_authority: operation_authority,
            _expectation: expectation,
            _binding: binding,
        }
    };
    ActiveBundledRuntimeHostActivationBindingRefusal {
        reason,
        authority: Box::new(authority),
    }
}

#[allow(clippy::too_many_arguments)]
fn rollback_host_activation_refusal_from_parts(
    reason: BundledRuntimeHostActivationBindingError,
    binding: RuntimePackageBinding,
    expected_entry: &ExtensionNativeOwnershipEntry,
    expected_fingerprint: &ExtensionRuntimeFingerprint,
    entry: ExtensionNativeOwnershipEntry,
    access: ExtensionPackageAccess,
    operation_authority: ExtensionRuntimeOperationAuthority,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
) -> RollbackBundledRuntimeHostActivationBindingRefusal {
    let returned_exactly = &entry == expected_entry
        && expectation == binding.native_identity
        && operation_authority.fingerprint() == expected_fingerprint
        && operation_authority.matches_native_ownership_lineage(&entry)
        && binding.matches_plan(access.target(), access.resources());
    let authority = if returned_exactly {
        RollbackHostBindingRefusalAuthority::Recoverable {
            access: RollbackBundledRuntimePackageAccess {
                access,
                operation_authority,
                binding,
            },
            entry,
        }
    } else {
        RollbackHostBindingRefusalAuthority::Quarantined {
            _entry: entry,
            _access: access,
            _operation_authority: operation_authority,
            _expectation: expectation,
            _binding: binding,
        }
    };
    RollbackBundledRuntimeHostActivationBindingRefusal {
        reason,
        authority: Box::new(authority),
    }
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
macro_rules! impl_runtime_package_access_test_view {
    ($access:ident) => {
        impl $access {
            pub(super) const fn target(&self) -> ExtensionRuntimeTarget {
                self.access.target()
            }

            pub(super) const fn fingerprint(&self) -> &ExtensionRuntimeFingerprint {
                self.operation_authority.fingerprint()
            }

            pub(super) const fn resources(&self) -> &ExtensionRuntimeResourcePlan {
                self.access.resources()
            }

            pub(super) const fn package_access_retained_bytes(&self) -> usize {
                self.access.retained_bytes()
            }

            pub(super) fn operation_authority_retained_bytes(&self) -> usize {
                self.operation_authority.retained_bytes()
            }

            pub(super) fn visit_manifest(
                &mut self,
                visitor: &mut dyn ExtensionRuntimeResourceVisitor,
            ) -> Result<
                Result<(), zephium_extension_runtime_api::ExtensionRuntimeVisitorError>,
                ExtensionPackageAccessError,
            > {
                self.access.visit_manifest(visitor)
            }

            pub(super) fn visit_resource(
                &mut self,
                resource: ExtensionRuntimeResource,
                visitor: &mut dyn ExtensionRuntimeResourceVisitor,
            ) -> Result<
                Result<(), zephium_extension_runtime_api::ExtensionRuntimeVisitorError>,
                ExtensionPackageAccessError,
            > {
                self.access.visit_resource(resource, visitor)
            }
        }
    };
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
impl_runtime_package_access_test_view!(ActiveBundledRuntimePackageAccess);

struct HeldPackageLeaseCore<Snapshot> {
    open_epoch: Arc<RepositoryOpenEpoch>,
    runtime: RepositoryRuntime,
    repository: PackageLeaseRepositoryIdentity,
    current_set: BundledCatalogSetIdentity,
    pin: OwnerPackagePinIdentity,
    held: ExtensionPackagePinHeldBinding,
    presence: Arc<LeasePresence>,
    snapshot: Arc<Snapshot>,
}

struct HeldPackageLeaseRecombineRefusal<Snapshot> {
    _open_epoch: Arc<RepositoryOpenEpoch>,
    _runtime: RepositoryRuntime,
    _repository: PackageLeaseRepositoryIdentity,
    _current_set: BundledCatalogSetIdentity,
    _pin: OwnerPackagePinIdentity,
    _refusal: ExtensionPackagePinRecombineRefusal,
    _presence: Arc<LeasePresence>,
    _snapshot: Arc<Snapshot>,
}

impl<Snapshot> HeldPackageLeaseCore<Snapshot> {
    fn from_lease_core(
        core: PackageLeaseCore<Snapshot>,
        generation: ExtensionRuntimeGeneration,
    ) -> (Self, ExtensionRuntimeOperationAuthority) {
        let PackageLeaseCore {
            open_epoch,
            runtime,
            repository,
            current_set,
            pin,
            acquisition,
            presence,
            snapshot,
        } = core;
        let (held, operation_authority) = (*acquisition)
            .into_runtime_parts(generation)
            .into_held_binding_and_operation_authority();
        (
            Self {
                open_epoch,
                runtime,
                repository,
                current_set,
                pin,
                held,
                presence,
                snapshot,
            },
            operation_authority,
        )
    }

    fn try_into_lease_core(
        self,
        operation_authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<PackageLeaseCore<Snapshot>, Box<HeldPackageLeaseRecombineRefusal<Snapshot>>> {
        let Self {
            open_epoch,
            runtime,
            repository,
            current_set,
            pin,
            held,
            presence,
            snapshot,
        } = self;
        match held.try_recombine(operation_authority) {
            Ok(acquisition) => Ok(PackageLeaseCore {
                open_epoch,
                runtime,
                repository,
                current_set,
                pin,
                acquisition: Box::new(acquisition),
                presence,
                snapshot,
            }),
            Err(refusal) => Err(Box::new(HeldPackageLeaseRecombineRefusal {
                _open_epoch: open_epoch,
                _runtime: runtime,
                _repository: repository,
                _current_set: current_set,
                _pin: pin,
                _refusal: refusal,
                _presence: presence,
                _snapshot: snapshot,
            })),
        }
    }
}

enum ActiveBuildAuthority {
    Lease(ActiveBundledPackageLease),
    AccessQuarantine {
        _access: ExtensionPackageAccess,
        _operation_authority: ExtensionRuntimeOperationAuthority,
        _binding: RuntimePackageBinding,
    },
    AcceptedBindingQuarantine {
        _provider: Box<ActiveLeasePackageAccessProvider>,
        _operation_authority: ExtensionRuntimeOperationAuthority,
        _binding: RuntimePackageBinding,
    },
    RuntimeRefusal {
        _refusal: ExtensionPackageAccessBuildRefusal,
        _operation_authority: ExtensionRuntimeOperationAuthority,
    },
    RecombineQuarantine {
        _refusal: Box<HeldPackageLeaseRecombineRefusal<VerifiedActivePackageSnapshot>>,
    },
    BindingQuarantine {
        _target: ExtensionRuntimeTarget,
        _resources: ExtensionRuntimeResourcePlan,
        _provider: Box<ActiveLeasePackageAccessProvider>,
        _operation_authority: ExtensionRuntimeOperationAuthority,
        _binding: Option<RuntimePackageBinding>,
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
    AccessQuarantine {
        _access: ExtensionPackageAccess,
        _operation_authority: ExtensionRuntimeOperationAuthority,
        _binding: RuntimePackageBinding,
    },
    AcceptedBindingQuarantine {
        _provider: Box<RollbackLeasePackageAccessProvider>,
        _operation_authority: ExtensionRuntimeOperationAuthority,
        _binding: RuntimePackageBinding,
    },
    RuntimeRefusal {
        _refusal: ExtensionPackageAccessBuildRefusal,
        _operation_authority: ExtensionRuntimeOperationAuthority,
    },
    RecombineQuarantine {
        _refusal: Box<HeldPackageLeaseRecombineRefusal<VerifiedRollbackPackageSnapshot>>,
    },
    BindingQuarantine {
        _target: ExtensionRuntimeTarget,
        _resources: ExtensionRuntimeResourcePlan,
        _provider: Box<RollbackLeasePackageAccessProvider>,
        _operation_authority: ExtensionRuntimeOperationAuthority,
        _binding: Option<RuntimePackageBinding>,
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
    Access(Box<ActiveBundledRuntimePackageAccess>),
    RecombineQuarantine {
        _refusal: Box<HeldPackageLeaseRecombineRefusal<VerifiedActivePackageSnapshot>>,
    },
    BindingQuarantine {
        _provider: Box<ActiveLeasePackageAccessProvider>,
        _operation_authority: Box<ExtensionRuntimeOperationAuthority>,
    },
}

/// Failed active-package recovery that never discards package authority.
#[must_use = "the refusal retains returned runtime package authority"]
pub struct ActiveBundledRuntimePackageRecoveryRefusal {
    reason: ActiveBundledRuntimePackageRecoveryError,
    authority: Box<ActiveRecoveryAuthority>,
}

impl ActiveBundledRuntimePackageRecoveryRefusal {
    /// Returns the stable recovery refusal reason.
    pub const fn reason(&self) -> ActiveBundledRuntimePackageRecoveryError {
        self.reason
    }

    /// Recovers the complete original runtime access after a role mismatch.
    ///
    /// A binding mismatch remains quarantined and returns this refusal unchanged.
    pub fn try_into_access(self) -> Result<ActiveBundledRuntimePackageAccess, Self> {
        match *self.authority {
            ActiveRecoveryAuthority::Access(access) => Ok(*access),
            authority => Err(Self {
                reason: self.reason,
                authority: Box::new(authority),
            }),
        }
    }

    /// Whether this refusal is a non-retryable internal authority mismatch.
    #[must_use]
    pub fn requires_fail_stop(&self) -> bool {
        !matches!(self.authority.as_ref(), ActiveRecoveryAuthority::Access(_))
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
    Access(Box<RollbackBundledRuntimePackageAccess>),
    RecombineQuarantine {
        _refusal: Box<HeldPackageLeaseRecombineRefusal<VerifiedRollbackPackageSnapshot>>,
    },
    BindingQuarantine {
        _provider: Box<RollbackLeasePackageAccessProvider>,
        _operation_authority: Box<ExtensionRuntimeOperationAuthority>,
    },
}

/// Failed rollback-package recovery that never discards package authority.
#[must_use = "the refusal retains returned runtime package authority"]
pub struct RollbackBundledRuntimePackageRecoveryRefusal {
    reason: RollbackBundledRuntimePackageRecoveryError,
    authority: Box<RollbackRecoveryAuthority>,
}

impl RollbackBundledRuntimePackageRecoveryRefusal {
    /// Returns the stable recovery refusal reason.
    pub const fn reason(&self) -> RollbackBundledRuntimePackageRecoveryError {
        self.reason
    }

    /// Recovers the complete original runtime access after a role mismatch.
    ///
    /// A binding mismatch remains quarantined and returns this refusal unchanged.
    pub fn try_into_access(self) -> Result<RollbackBundledRuntimePackageAccess, Self> {
        match *self.authority {
            RollbackRecoveryAuthority::Access(access) => Ok(*access),
            authority => Err(Self {
                reason: self.reason,
                authority: Box::new(authority),
            }),
        }
    }

    /// Whether this refusal is a non-retryable internal authority mismatch.
    #[must_use]
    pub fn requires_fail_stop(&self) -> bool {
        !matches!(
            self.authority.as_ref(),
            RollbackRecoveryAuthority::Access(_)
        )
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

#[derive(Clone, Copy, Eq, PartialEq)]
struct RuntimePackageBinding {
    target: ExtensionRuntimeTarget,
    product_target: ProductExtensionRuntimeTarget,
    native_identity: ExtensionRuntimeNativeIdentityExpectation,
    owner: ExtensionNativeOwnershipKey,
    catalog_set_digest: ExtensionCatalogSetDigest,
    catalog_role: ExtensionCatalogGenerationRole,
    runtime_backend: ExtensionRuntimeBackendTarget,
    native_incarnation: ExtensionNativeIncarnation,
    journal_operation: ExtensionNativeOwnershipOperation,
    plan_digest: [u8; 32],
    resource_count: u32,
}

impl RuntimePackageBinding {
    fn try_new<Snapshot: RuntimePackageSnapshot>(
        snapshot: &Snapshot,
        resources: &ExtensionRuntimeResourcePlan,
        acquisition: &ExtensionPackagePinAcquisitionBinding,
    ) -> Result<Self, BundledRuntimePackageAccessBuildError> {
        let target = runtime_target(snapshot.runtime_target())?;
        let resource_count = u32::try_from(resources.entries().len())
            .map_err(|_| BundledRuntimePackageAccessBuildError::InternalBindingMismatch)?;
        let binding = Self {
            target,
            product_target: snapshot.runtime_target(),
            native_identity: runtime_native_identity(
                snapshot.runtime_target(),
                snapshot.chromium_key(),
            )?,
            owner: acquisition.key(),
            catalog_set_digest: acquisition.catalog_set_digest(),
            catalog_role: acquisition.catalog_role(),
            runtime_backend: acquisition.runtime_backend(),
            native_incarnation: acquisition.native_incarnation(),
            journal_operation: acquisition.journal_operation(),
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
            && runtime_native_identity(snapshot.runtime_target(), snapshot.chromium_key()).ok()
                == Some(self.native_identity)
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

    fn matches_ownership_entry(
        &self,
        entry: &ExtensionNativeOwnershipEntry,
        operation_authority: &ExtensionRuntimeOperationAuthority,
    ) -> bool {
        let fingerprint = operation_authority.fingerprint();
        let instance = fingerprint.instance();
        entry.key() == self.owner
            && entry.key().profile() == instance.profile()
            && entry.key().install_id() == instance.install_id()
            && entry.key().browsing_context() == fingerprint.browsing_context()
            && entry.package() == fingerprint.package()
            && entry.catalog_set_digest() == self.catalog_set_digest
            && entry.catalog_role() == self.catalog_role
            && entry.store_catalog_revision() == fingerprint.catalog_revision()
            && entry.store_install_revision() == fingerprint.install_revision()
            && entry.store_grant_revision() == fingerprint.grant_revision()
            && entry.grant_digest() == fingerprint.grant_digest()
            && entry.runtime_backend() == self.runtime_backend
            && entry.native_identity().is_none()
            && entry.native_incarnation() == self.native_incarnation
            && entry.operation() == self.journal_operation
            && operation_authority.matches_native_ownership_lineage(entry)
    }

    fn matches_held_pin(&self, held: &ExtensionPackagePinHeldBinding) -> bool {
        held.key() == self.owner
            && held.catalog_set_digest() == self.catalog_set_digest
            && held.catalog_role() == self.catalog_role
            && held.runtime_backend() == self.runtime_backend
            && held.native_incarnation() == self.native_incarnation
            && held.journal_operation() == self.journal_operation
    }
}

trait RuntimePackageSnapshot {
    fn runtime_target(&self) -> ProductExtensionRuntimeTarget;
    fn chromium_key(&self) -> Option<&ChromiumManifestKey>;
    fn index(&self) -> &CanonicalExtensionTreeIndex;
    fn root(&self) -> &Arc<SealedPrivateDirectory>;
}

macro_rules! impl_runtime_snapshot {
    ($snapshot:ty) => {
        impl RuntimePackageSnapshot for $snapshot {
            fn runtime_target(&self) -> ProductExtensionRuntimeTarget {
                self.runtime_target()
            }

            fn chromium_key(&self) -> Option<&ChromiumManifestKey> {
                self.chromium_key()
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
    core: HeldPackageLeaseCore<VerifiedActivePackageSnapshot>,
    binding: RuntimePackageBinding,
    native_root_lease: Option<Box<RepositoryNativeRootLease<VerifiedActivePackageSnapshot>>>,
    retained_bytes: usize,
}

struct RollbackLeasePackageAccessProvider {
    core: HeldPackageLeaseCore<VerifiedRollbackPackageSnapshot>,
    binding: RuntimePackageBinding,
    native_root_lease: Option<Box<RepositoryNativeRootLease<VerifiedRollbackPackageSnapshot>>>,
    retained_bytes: usize,
}

/// Preallocated, passively dropped root authority transferred into a trusted
/// native lifecycle adapter. Its shared allocations are charged exactly once
/// by the originating provider's stable retained-byte bound.
struct RepositoryNativeRootLease<Snapshot> {
    open_epoch: Arc<RepositoryOpenEpoch>,
    runtime: RepositoryRuntime,
    repository: PackageLeaseRepositoryIdentity,
    current_set: BundledCatalogSetIdentity,
    pin: OwnerPackagePinIdentity,
    presence: Arc<LeasePresence>,
    snapshot: Arc<Snapshot>,
    binding: RuntimePackageBinding,
}

impl<Snapshot: RuntimePackageSnapshot> RepositoryNativeRootLease<Snapshot> {
    fn new(core: &HeldPackageLeaseCore<Snapshot>, binding: RuntimePackageBinding) -> Self {
        Self {
            open_epoch: Arc::clone(&core.open_epoch),
            runtime: core.runtime.clone(),
            repository: core.repository,
            current_set: core.current_set,
            pin: core.pin,
            presence: Arc::clone(&core.presence),
            snapshot: Arc::clone(&core.snapshot),
            binding,
        }
    }

    fn matches_core(
        &self,
        core: &HeldPackageLeaseCore<Snapshot>,
        binding: &RuntimePackageBinding,
    ) -> bool {
        self.binding == *binding
            && Arc::ptr_eq(&self.open_epoch, &core.open_epoch)
            && self.repository == core.repository
            && self.current_set == core.current_set
            && self.pin == core.pin
            && Arc::ptr_eq(&self.presence, &core.presence)
            && Arc::ptr_eq(&self.snapshot, &core.snapshot)
            && binding.matches_held_pin(&core.held)
            && self.is_valid()
    }

    fn is_valid(&self) -> bool {
        let (profile, install) = self.pin.lease_owner();
        Arc::ptr_eq(&self.open_epoch, &self.presence.open_epoch)
            && self.presence.repository == self.repository
            && self.presence.profile == profile
            && self.presence.install == install
            && self.presence.binding == LeasePresenceBinding::DurablePin(self.pin)
            && self.current_set.bytes() == self.binding.catalog_set_digest.bytes()
            && self.binding.target == ExtensionRuntimeTarget::NativeWebExtension
            && self.binding.owner.profile() == profile
            && self.binding.owner.install_id() == install
            && self.binding.matches_snapshot(self.snapshot.as_ref())
    }
}

impl<Snapshot: RuntimePackageSnapshot + Send + Sync + 'static> ExtensionRuntimeNativeRootLeasePort
    for RepositoryNativeRootLease<Snapshot>
{
    fn visit_native_root(
        &mut self,
        visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
    ) -> Result<(), ExtensionPackageAccessError> {
        visit_retained_native_root(self, visitor)
    }
}

macro_rules! impl_runtime_provider {
    ($provider:ident, $lease:ident, $snapshot:ty) => {
        impl $provider {
            fn new(core: HeldPackageLeaseCore<$snapshot>, binding: RuntimePackageBinding) -> Self {
                let native_root_lease = (binding.target
                    == ExtensionRuntimeTarget::NativeWebExtension)
                    .then(|| Box::new(RepositoryNativeRootLease::new(&core, binding)));
                let retained_bytes =
                    provider_retained_bytes::<Self, RepositoryNativeRootLease<$snapshot>>(
                        core.snapshot.retained_bytes(),
                        native_root_lease.is_some(),
                    );
                Self {
                    core,
                    binding,
                    native_root_lease,
                    retained_bytes,
                }
            }

            fn try_into_lease(
                self,
                operation_authority: ExtensionRuntimeOperationAuthority,
            ) -> Result<$lease, Box<HeldPackageLeaseRecombineRefusal<$snapshot>>> {
                self.core
                    .try_into_lease_core(operation_authority)
                    .map(|core| $lease { core })
            }

            fn matches_access(
                &self,
                target: ExtensionRuntimeTarget,
                resources: &ExtensionRuntimeResourcePlan,
            ) -> bool {
                self.binding.matches_plan(target, resources)
                    && self.binding.matches_snapshot(self.core.snapshot.as_ref())
                    && self.binding.matches_held_pin(&self.core.held)
            }
        }

        impl ExtensionPackageAccessPort for $provider {
            fn retained_bytes(&self) -> usize {
                self.retained_bytes
            }

            fn visit_resource(
                &mut self,
                descriptor: ExtensionRuntimeResource,
                visitor: &mut dyn ExtensionRuntimeResourceVisitor,
            ) -> Result<(), ExtensionPackageAccessError> {
                visit_resource(&self.core, &self.binding, descriptor, visitor)
            }

            fn take_native_root_lease(
                &mut self,
                target: ExtensionRuntimeTarget,
            ) -> Result<Box<dyn ExtensionRuntimeNativeRootLeasePort>, ExtensionPackageAccessError>
            {
                take_native_root_lease(
                    &self.core,
                    &self.binding,
                    &mut self.native_root_lease,
                    target,
                )
            }
        }
    };
}

impl_runtime_provider!(
    ActiveLeasePackageAccessProvider,
    ActiveBundledPackageLease,
    VerifiedActivePackageSnapshot
);
impl_runtime_provider!(
    RollbackLeasePackageAccessProvider,
    RollbackBundledPackageLease,
    VerifiedRollbackPackageSnapshot
);

impl ActiveBundledPackageLease {
    /// Delegates this authenticated active lease as bounded runtime package access.
    ///
    /// The complete canonical tree is bound once. The supplied generation is
    /// joined to the Store eligibility exactly once, while the provider retains
    /// only compact held-pin authority. Destructors remain passive; only
    /// recovery from the complete returned value can produce role-correct
    /// durable release authority.
    pub fn into_runtime_package_access(
        self,
        generation: ExtensionRuntimeGeneration,
    ) -> Result<ActiveBundledRuntimePackageAccess, ActiveBundledRuntimePackageAccessBuildRefusal>
    {
        let resources = match build_resource_plan(self.core.snapshot.index()) {
            Ok(resources) => resources,
            Err(reason) => {
                return Err(ActiveBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(ActiveBuildAuthority::Lease(self)),
                });
            }
        };
        let binding = match RuntimePackageBinding::try_new(
            self.core.snapshot.as_ref(),
            &resources,
            self.core.acquisition.as_ref(),
        ) {
            Ok(binding) => binding,
            Err(reason) => {
                return Err(ActiveBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(ActiveBuildAuthority::Lease(self)),
                });
            }
        };
        let target = binding.target;
        let (core, operation_authority) =
            HeldPackageLeaseCore::from_lease_core(self.core, generation);
        let provider = Box::new(ActiveLeasePackageAccessProvider::new(core, binding));
        match ExtensionPackageAccess::from_delegated_provider(target, resources, provider) {
            Ok(access) => {
                let access = ActiveBundledRuntimePackageAccess {
                    access,
                    operation_authority,
                    binding,
                };
                match pre_host_runtime_access_retained_bytes::<ActiveBundledRuntimePackageAccess>(
                    &access.access,
                    &access.operation_authority,
                ) {
                    None => Err(active_pre_host_accounting_refusal(
                        access,
                        BundledRuntimePackageAccessBuildError::RetainedBytesOverflow,
                    )),
                    Some(retained) if retained > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES => {
                        Err(active_pre_host_accounting_refusal(
                            access,
                            BundledRuntimePackageAccessBuildError::RetainedBytesExceeded,
                        ))
                    }
                    Some(_) => Ok(access),
                }
            }
            Err(refusal) => Err(active_build_refusal(refusal, operation_authority)),
        }
    }
}

impl RollbackBundledPackageLease {
    /// Delegates this authenticated rollback lease as bounded runtime package access.
    ///
    /// Active and rollback providers and result types are distinct, preventing
    /// a returned capability from crossing catalog-generation roles. The
    /// supplied generation is joined to Store eligibility exactly once.
    pub fn into_runtime_package_access(
        self,
        generation: ExtensionRuntimeGeneration,
    ) -> Result<RollbackBundledRuntimePackageAccess, RollbackBundledRuntimePackageAccessBuildRefusal>
    {
        let resources = match build_resource_plan(self.core.snapshot.index()) {
            Ok(resources) => resources,
            Err(reason) => {
                return Err(RollbackBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(RollbackBuildAuthority::Lease(self)),
                });
            }
        };
        let binding = match RuntimePackageBinding::try_new(
            self.core.snapshot.as_ref(),
            &resources,
            self.core.acquisition.as_ref(),
        ) {
            Ok(binding) => binding,
            Err(reason) => {
                return Err(RollbackBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(RollbackBuildAuthority::Lease(self)),
                });
            }
        };
        let target = binding.target;
        let (core, operation_authority) =
            HeldPackageLeaseCore::from_lease_core(self.core, generation);
        let provider = Box::new(RollbackLeasePackageAccessProvider::new(core, binding));
        match ExtensionPackageAccess::from_delegated_provider(target, resources, provider) {
            Ok(access) => {
                let access = RollbackBundledRuntimePackageAccess {
                    access,
                    operation_authority,
                    binding,
                };
                match pre_host_runtime_access_retained_bytes::<RollbackBundledRuntimePackageAccess>(
                    &access.access,
                    &access.operation_authority,
                ) {
                    None => Err(rollback_pre_host_accounting_refusal(
                        access,
                        BundledRuntimePackageAccessBuildError::RetainedBytesOverflow,
                    )),
                    Some(retained) if retained > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES => {
                        Err(rollback_pre_host_accounting_refusal(
                            access,
                            BundledRuntimePackageAccessBuildError::RetainedBytesExceeded,
                        ))
                    }
                    Some(_) => Ok(access),
                }
            }
            Err(refusal) => Err(rollback_build_refusal(refusal, operation_authority)),
        }
    }
}

impl ActiveBundledRuntimePackageRecoveryToken {
    /// Rejoins exact post-absence package and operation authority into an active
    /// repository release request.
    ///
    /// The native lifecycle must first return package access only after definite
    /// absence, and unpublished or published host state must return the exact
    /// operation authority under its release-frontier protocol. This repository
    /// boundary then validates the private active provider, the complete binding,
    /// held pin, resource plan, and Core operation-authority lineage before any
    /// durable release can occur.
    pub fn try_into_release_request(
        self,
        access: ExtensionPackageAccess,
        operation_authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<ActiveBundledPackageReleaseRequest, ActiveBundledRuntimePackageRejoinRefusal> {
        ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(
            ActiveBundledRuntimePackageAccess {
                access,
                operation_authority,
                binding: self.binding,
            },
        )
        .map_err(|refusal| ActiveBundledRuntimePackageRejoinRefusal { refusal })
    }
}

impl RollbackBundledRuntimePackageRecoveryToken {
    /// Rejoins exact post-absence package and operation authority into a rollback
    /// repository release request.
    ///
    /// Active providers cannot cross this nominal boundary. A mismatched input
    /// is returned whole when it is safely recoverable; an internal binding or
    /// Core recombination mismatch remains captive for fail-stop handling.
    pub fn try_into_release_request(
        self,
        access: ExtensionPackageAccess,
        operation_authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<RollbackBundledPackageReleaseRequest, RollbackBundledRuntimePackageRejoinRefusal>
    {
        RollbackBundledPackageReleaseRequest::try_from_runtime_package_access(
            RollbackBundledRuntimePackageAccess {
                access,
                operation_authority,
                binding: self.binding,
            },
        )
        .map_err(|refusal| RollbackBundledRuntimePackageRejoinRefusal { refusal })
    }
}

impl ActiveBundledPackageReleaseRequest {
    /// Recovers active durable release authority from runtime-returned package access.
    ///
    /// Wrong-role or foreign access is returned whole. An impossible internal
    /// binding mismatch remains captive and never releases or unpins authority.
    pub fn try_from_runtime_package_access(
        access: ActiveBundledRuntimePackageAccess,
    ) -> Result<Self, ActiveBundledRuntimePackageRecoveryRefusal> {
        let ActiveBundledRuntimePackageAccess {
            access,
            operation_authority,
            binding,
        } = access;
        let target = access.target();
        let plan_digest = access.resources().digest();
        let resource_count = access.resources().entries().len();
        match access.try_into_delegated_provider::<ActiveLeasePackageAccessProvider>() {
            Err(access) => Err(ActiveBundledRuntimePackageRecoveryRefusal {
                reason: ActiveBundledRuntimePackageRecoveryError::WrongProviderRole,
                authority: Box::new(ActiveRecoveryAuthority::Access(Box::new(
                    ActiveBundledRuntimePackageAccess {
                        access,
                        operation_authority,
                        binding,
                    },
                ))),
            }),
            Ok(provider) => {
                if provider.binding != binding
                    || provider.binding.target != target
                    || provider.binding.plan_digest != plan_digest
                    || usize::try_from(provider.binding.resource_count).ok() != Some(resource_count)
                    || !provider
                        .binding
                        .matches_snapshot(provider.core.snapshot.as_ref())
                    || !provider.binding.matches_held_pin(&provider.core.held)
                {
                    return Err(ActiveBundledRuntimePackageRecoveryRefusal {
                        reason: ActiveBundledRuntimePackageRecoveryError::InternalBindingMismatch,
                        authority: Box::new(ActiveRecoveryAuthority::BindingQuarantine {
                            _provider: provider,
                            _operation_authority: Box::new(operation_authority),
                        }),
                    });
                }
                match provider.try_into_lease(operation_authority) {
                    Ok(lease) => Ok(lease.into_release_request()),
                    Err(refusal) => Err(ActiveBundledRuntimePackageRecoveryRefusal {
                        reason: ActiveBundledRuntimePackageRecoveryError::InternalBindingMismatch,
                        authority: Box::new(ActiveRecoveryAuthority::RecombineQuarantine {
                            _refusal: refusal,
                        }),
                    }),
                }
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
        access: RollbackBundledRuntimePackageAccess,
    ) -> Result<Self, RollbackBundledRuntimePackageRecoveryRefusal> {
        let RollbackBundledRuntimePackageAccess {
            access,
            operation_authority,
            binding,
        } = access;
        let target = access.target();
        let plan_digest = access.resources().digest();
        let resource_count = access.resources().entries().len();
        match access.try_into_delegated_provider::<RollbackLeasePackageAccessProvider>() {
            Err(access) => Err(RollbackBundledRuntimePackageRecoveryRefusal {
                reason: RollbackBundledRuntimePackageRecoveryError::WrongProviderRole,
                authority: Box::new(RollbackRecoveryAuthority::Access(Box::new(
                    RollbackBundledRuntimePackageAccess {
                        access,
                        operation_authority,
                        binding,
                    },
                ))),
            }),
            Ok(provider) => {
                if provider.binding != binding
                    || provider.binding.target != target
                    || provider.binding.plan_digest != plan_digest
                    || usize::try_from(provider.binding.resource_count).ok() != Some(resource_count)
                    || !provider
                        .binding
                        .matches_snapshot(provider.core.snapshot.as_ref())
                    || !provider.binding.matches_held_pin(&provider.core.held)
                {
                    return Err(RollbackBundledRuntimePackageRecoveryRefusal {
                        reason: RollbackBundledRuntimePackageRecoveryError::InternalBindingMismatch,
                        authority: Box::new(RollbackRecoveryAuthority::BindingQuarantine {
                            _provider: provider,
                            _operation_authority: Box::new(operation_authority),
                        }),
                    });
                }
                match provider.try_into_lease(operation_authority) {
                    Ok(lease) => Ok(lease.into_release_request()),
                    Err(refusal) => Err(RollbackBundledRuntimePackageRecoveryRefusal {
                        reason: RollbackBundledRuntimePackageRecoveryError::InternalBindingMismatch,
                        authority: Box::new(RollbackRecoveryAuthority::RecombineQuarantine {
                            _refusal: refusal,
                        }),
                    }),
                }
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

fn runtime_native_identity(
    target: ProductExtensionRuntimeTarget,
    chromium_key: Option<&ChromiumManifestKey>,
) -> Result<ExtensionRuntimeNativeIdentityExpectation, BundledRuntimePackageAccessBuildError> {
    // Product policy deliberately assigns the catalog-authenticated Chromium
    // ID to both native backends. WebView2 reports that ID directly; the macOS
    // adapter must set WKWebExtensionContext.uniqueIdentifier to this exact
    // value before load and verify readback. This does not infer WebKit's
    // default identity. Compatibility runtimes have no platform-native owner.
    let native_id = || {
        let extension_id = chromium_key
            .ok_or(BundledRuntimePackageAccessBuildError::NativeIdentityUnavailable)?
            .extension_id();
        let bytes: [u8; EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES] = extension_id
            .as_str()
            .as_bytes()
            .try_into()
            .map_err(|_| BundledRuntimePackageAccessBuildError::InternalBindingMismatch)?;
        ExtensionRuntimeNativeOwnerId::from_encoded_bytes(bytes)
            .map_err(|_| BundledRuntimePackageAccessBuildError::InternalBindingMismatch)
    };
    match target {
        ProductExtensionRuntimeTarget::MacosNative => {
            Ok(ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(native_id()?))
        }
        ProductExtensionRuntimeTarget::WindowsNative => {
            Ok(ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(native_id()?))
        }
        ProductExtensionRuntimeTarget::MacosCompatibility
        | ProductExtensionRuntimeTarget::LinuxCompatibility => {
            Ok(ExtensionRuntimeNativeIdentityExpectation::Compatibility)
        }
        _ => Err(BundledRuntimePackageAccessBuildError::UnsupportedRuntimeTarget),
    }
}

fn pre_host_runtime_access_retained_bytes<Access>(
    access: &ExtensionPackageAccess,
    operation_authority: &ExtensionRuntimeOperationAuthority,
) -> Option<usize> {
    let access_exclusive = access
        .retained_bytes()
        .checked_sub(size_of::<ExtensionPackageAccess>())?;
    let authority_exclusive = operation_authority
        .retained_bytes()
        .checked_sub(size_of::<ExtensionRuntimeOperationAuthority>())?;
    size_of::<Access>()
        .checked_add(access_exclusive)
        .and_then(|bytes| bytes.checked_add(authority_exclusive))
}

fn provider_retained_bytes<Provider, NativeRootLease>(
    snapshot_retained_bytes: usize,
    retains_native_root_lease: bool,
) -> usize {
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
    // Three exclusive allocations are always retained by delegated access: the
    // outer Box<Provider>, the shared snapshot Arc allocation, and the
    // owner-presence Arc allocation. A native target also preallocates one
    // root-lease Box. Its Arc handles are included in `size_of`, while the
    // snapshot and presence allocations remain counted exactly once below.
    // RepositoryRuntime and RepositoryOpenEpoch allocations are shared and
    // deliberately excluded.
    let baseline = size_of::<Provider>()
        .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
        .saturating_add(snapshot_retained_bytes)
        .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
        .saturating_add(RETAINED_ARC_COUNTER_BYTES)
        .saturating_add(size_of::<LeasePresence>())
        .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
        .saturating_add(RETAINED_ARC_COUNTER_BYTES);
    if retains_native_root_lease {
        baseline
            .saturating_add(size_of::<NativeRootLease>())
            .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
    } else {
        baseline
    }
}

fn visit_resource<Snapshot: RuntimePackageSnapshot>(
    core: &HeldPackageLeaseCore<Snapshot>,
    binding: &RuntimePackageBinding,
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

fn take_native_root_lease<Snapshot>(
    core: &HeldPackageLeaseCore<Snapshot>,
    binding: &RuntimePackageBinding,
    lease: &mut Option<Box<RepositoryNativeRootLease<Snapshot>>>,
    target: ExtensionRuntimeTarget,
) -> Result<Box<dyn ExtensionRuntimeNativeRootLeasePort>, ExtensionPackageAccessError>
where
    Snapshot: RuntimePackageSnapshot + Send + Sync + 'static,
{
    let _operation = core.runtime.enter().map_err(map_operation_error)?;
    if target != binding.target || !binding.matches_snapshot(core.snapshot.as_ref()) {
        core.runtime.poison();
        return Err(ExtensionPackageAccessError::NativeRootIdentityMismatch);
    }
    if target != ExtensionRuntimeTarget::NativeWebExtension {
        return Err(ExtensionPackageAccessError::NativeRootUnavailable);
    }
    let Some(candidate) = lease.as_ref() else {
        return Err(ExtensionPackageAccessError::NativeRootUnavailable);
    };
    if !candidate.matches_core(core, binding) {
        core.runtime.poison();
        return Err(ExtensionPackageAccessError::NativeRootIdentityMismatch);
    }
    let Some(candidate) = lease.take() else {
        core.runtime.poison();
        return Err(ExtensionPackageAccessError::NativeRootIdentityMismatch);
    };
    if !candidate.matches_core(core, binding) || !core.runtime.is_healthy() {
        *lease = Some(candidate);
        core.runtime.poison();
        return Err(ExtensionPackageAccessError::NativeRootIdentityMismatch);
    }
    Ok(candidate)
}

fn visit_retained_native_root<Snapshot: RuntimePackageSnapshot>(
    lease: &RepositoryNativeRootLease<Snapshot>,
    visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
) -> Result<(), ExtensionPackageAccessError> {
    let operation = lease.runtime.enter().map_err(map_operation_error)?;
    if !lease.is_valid() {
        lease.runtime.poison();
        return Err(ExtensionPackageAccessError::NativeRootIdentityMismatch);
    }

    let _callback_barrier = lease
        .runtime
        .begin_delegated_callback()
        .map_err(map_operation_error)?;
    drop(operation);

    let result = lease
        .snapshot
        .root()
        .with_verified_path(|path| with_external_callback(|| visitor.visit(path)));
    if !lease.is_valid() || !lease.runtime.is_healthy() {
        lease.runtime.poison();
        return Err(ExtensionPackageAccessError::NativeRootIdentityMismatch);
    }
    match result {
        Ok(_visitor_result) => Ok(()),
        Err(_error) => {
            lease.runtime.poison();
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

fn active_pre_host_accounting_refusal(
    access: ActiveBundledRuntimePackageAccess,
    reason: BundledRuntimePackageAccessBuildError,
) -> ActiveBundledRuntimePackageAccessBuildRefusal {
    let ActiveBundledRuntimePackageAccess {
        access,
        operation_authority,
        binding,
    } = access;
    let target = access.target();
    let plan_digest = access.resources().digest();
    let resource_count = access.resources().entries().len();
    match access.try_into_delegated_provider::<ActiveLeasePackageAccessProvider>() {
        Err(access) => ActiveBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(ActiveBuildAuthority::AccessQuarantine {
                _access: access,
                _operation_authority: operation_authority,
                _binding: binding,
            }),
        },
        Ok(provider)
            if provider.binding == binding
                && provider.binding.target == target
                && provider.binding.plan_digest == plan_digest
                && usize::try_from(provider.binding.resource_count).ok()
                    == Some(resource_count)
                && provider
                    .binding
                    .matches_snapshot(provider.core.snapshot.as_ref())
                && provider.binding.matches_held_pin(&provider.core.held) =>
        {
            match provider.try_into_lease(operation_authority) {
                Ok(lease) => ActiveBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(ActiveBuildAuthority::Lease(lease)),
                },
                Err(refusal) => ActiveBundledRuntimePackageAccessBuildRefusal {
                    reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
                    authority: Box::new(ActiveBuildAuthority::RecombineQuarantine {
                        _refusal: refusal,
                    }),
                },
            }
        }
        Ok(provider) => ActiveBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(ActiveBuildAuthority::AcceptedBindingQuarantine {
                _provider: provider,
                _operation_authority: operation_authority,
                _binding: binding,
            }),
        },
    }
}

fn rollback_pre_host_accounting_refusal(
    access: RollbackBundledRuntimePackageAccess,
    reason: BundledRuntimePackageAccessBuildError,
) -> RollbackBundledRuntimePackageAccessBuildRefusal {
    let RollbackBundledRuntimePackageAccess {
        access,
        operation_authority,
        binding,
    } = access;
    let target = access.target();
    let plan_digest = access.resources().digest();
    let resource_count = access.resources().entries().len();
    match access.try_into_delegated_provider::<RollbackLeasePackageAccessProvider>() {
        Err(access) => RollbackBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(RollbackBuildAuthority::AccessQuarantine {
                _access: access,
                _operation_authority: operation_authority,
                _binding: binding,
            }),
        },
        Ok(provider)
            if provider.binding == binding
                && provider.binding.target == target
                && provider.binding.plan_digest == plan_digest
                && usize::try_from(provider.binding.resource_count).ok()
                    == Some(resource_count)
                && provider
                    .binding
                    .matches_snapshot(provider.core.snapshot.as_ref())
                && provider.binding.matches_held_pin(&provider.core.held) =>
        {
            match provider.try_into_lease(operation_authority) {
                Ok(lease) => RollbackBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(RollbackBuildAuthority::Lease(lease)),
                },
                Err(refusal) => RollbackBundledRuntimePackageAccessBuildRefusal {
                    reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
                    authority: Box::new(RollbackBuildAuthority::RecombineQuarantine {
                        _refusal: refusal,
                    }),
                },
            }
        }
        Ok(provider) => RollbackBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(RollbackBuildAuthority::AcceptedBindingQuarantine {
                _provider: provider,
                _operation_authority: operation_authority,
                _binding: binding,
            }),
        },
    }
}

fn active_build_refusal(
    refusal: ExtensionPackageAccessBuildRefusal,
    operation_authority: ExtensionRuntimeOperationAuthority,
) -> ActiveBundledRuntimePackageAccessBuildRefusal {
    let reason = BundledRuntimePackageAccessBuildError::PackageAccess(refusal.reason());
    match refusal.try_into_delegated_provider::<ActiveLeasePackageAccessProvider>() {
        Err(refusal) => ActiveBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(ActiveBuildAuthority::RuntimeRefusal {
                _refusal: refusal,
                _operation_authority: operation_authority,
            }),
        },
        Ok((target, resources, provider)) if provider.matches_access(target, &resources) => {
            match provider.try_into_lease(operation_authority) {
                Ok(lease) => ActiveBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(ActiveBuildAuthority::Lease(lease)),
                },
                Err(refusal) => ActiveBundledRuntimePackageAccessBuildRefusal {
                    reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
                    authority: Box::new(ActiveBuildAuthority::RecombineQuarantine {
                        _refusal: refusal,
                    }),
                },
            }
        }
        Ok((target, resources, provider)) => ActiveBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(ActiveBuildAuthority::BindingQuarantine {
                _target: target,
                _resources: resources,
                _provider: provider,
                _operation_authority: operation_authority,
                _binding: None,
            }),
        },
    }
}

fn rollback_build_refusal(
    refusal: ExtensionPackageAccessBuildRefusal,
    operation_authority: ExtensionRuntimeOperationAuthority,
) -> RollbackBundledRuntimePackageAccessBuildRefusal {
    let reason = BundledRuntimePackageAccessBuildError::PackageAccess(refusal.reason());
    match refusal.try_into_delegated_provider::<RollbackLeasePackageAccessProvider>() {
        Err(refusal) => RollbackBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(RollbackBuildAuthority::RuntimeRefusal {
                _refusal: refusal,
                _operation_authority: operation_authority,
            }),
        },
        Ok((target, resources, provider)) if provider.matches_access(target, &resources) => {
            match provider.try_into_lease(operation_authority) {
                Ok(lease) => RollbackBundledRuntimePackageAccessBuildRefusal {
                    reason,
                    authority: Box::new(RollbackBuildAuthority::Lease(lease)),
                },
                Err(refusal) => RollbackBundledRuntimePackageAccessBuildRefusal {
                    reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
                    authority: Box::new(RollbackBuildAuthority::RecombineQuarantine {
                        _refusal: refusal,
                    }),
                },
            }
        }
        Ok((target, resources, provider)) => RollbackBundledRuntimePackageAccessBuildRefusal {
            reason: BundledRuntimePackageAccessBuildError::InternalBindingMismatch,
            authority: Box::new(RollbackBuildAuthority::BindingQuarantine {
                _target: target,
                _resources: resources,
                _provider: provider,
                _operation_authority: operation_authority,
                _binding: None,
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: &str) -> ChromiumManifestKey {
        ChromiumManifestKey::parse_canonical(value).unwrap()
    }

    fn encoded_id(expectation: ExtensionRuntimeNativeIdentityExpectation) -> [u8; 32] {
        match expectation {
            ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(id)
            | ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(id) => {
                id.encoded_bytes()
            }
            ExtensionRuntimeNativeIdentityExpectation::Compatibility => {
                panic!("compatibility runtimes have no native owner identifier")
            }
        }
    }

    #[test]
    fn native_targets_project_the_catalog_authenticated_chromium_id_exactly() {
        let key = key("Xw==");
        let expected = *b"ncocknphbhhlhkikpnnlmbcnbgdempcd";

        assert_eq!(
            encoded_id(
                runtime_native_identity(ProductExtensionRuntimeTarget::MacosNative, Some(&key))
                    .unwrap()
            ),
            expected
        );
        assert_eq!(
            encoded_id(
                runtime_native_identity(ProductExtensionRuntimeTarget::WindowsNative, Some(&key))
                    .unwrap()
            ),
            expected
        );
    }

    #[test]
    fn native_identity_projection_is_target_exact_and_fails_closed_without_a_key() {
        for target in [
            ProductExtensionRuntimeTarget::MacosNative,
            ProductExtensionRuntimeTarget::WindowsNative,
        ] {
            assert_eq!(
                runtime_native_identity(target, None),
                Err(BundledRuntimePackageAccessBuildError::NativeIdentityUnavailable)
            );
        }

        let key = key("Xw==");
        assert!(matches!(
            runtime_native_identity(ProductExtensionRuntimeTarget::MacosNative, Some(&key)),
            Ok(ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(_))
        ));
        assert!(matches!(
            runtime_native_identity(ProductExtensionRuntimeTarget::WindowsNative, Some(&key)),
            Ok(ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(_))
        ));
    }

    #[test]
    fn compatibility_targets_never_gain_a_platform_native_identity() {
        let key = key("Xw==");
        for target in [
            ProductExtensionRuntimeTarget::MacosCompatibility,
            ProductExtensionRuntimeTarget::LinuxCompatibility,
        ] {
            assert_eq!(
                runtime_native_identity(target, None),
                Ok(ExtensionRuntimeNativeIdentityExpectation::Compatibility)
            );
            assert_eq!(
                runtime_native_identity(target, Some(&key)),
                Ok(ExtensionRuntimeNativeIdentityExpectation::Compatibility)
            );
        }
    }

    #[test]
    fn distinct_admitted_manifest_keys_cannot_match_the_same_expected_owner() {
        let first = runtime_native_identity(
            ProductExtensionRuntimeTarget::WindowsNative,
            Some(&key("Xw==")),
        )
        .unwrap();
        let second = runtime_native_identity(
            ProductExtensionRuntimeTarget::WindowsNative,
            Some(&key("WA==")),
        )
        .unwrap();

        assert_ne!(first, second);
        for byte in encoded_id(first) {
            assert!(matches!(byte, b'a'..=b'p'));
        }
    }

    #[test]
    fn provider_charge_includes_preallocated_native_lease_without_double_counting_shared_arcs() {
        struct ProviderShape {
            _bytes: [u8; 3],
        }
        struct NativeRootLeaseShape {
            _snapshot: Arc<()>,
            _presence: Arc<()>,
        }

        let snapshot_retained_bytes = 17;
        let expected = size_of::<ProviderShape>()
            .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
            .saturating_add(snapshot_retained_bytes)
            .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
            .saturating_add(RETAINED_ARC_COUNTER_BYTES)
            .saturating_add(size_of::<LeasePresence>())
            .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
            .saturating_add(RETAINED_ARC_COUNTER_BYTES);

        assert_eq!(
            provider_retained_bytes::<ProviderShape, NativeRootLeaseShape>(
                snapshot_retained_bytes,
                false,
            ),
            expected
        );
        assert_eq!(
            provider_retained_bytes::<ProviderShape, NativeRootLeaseShape>(
                snapshot_retained_bytes,
                true,
            ),
            expected
                .saturating_add(size_of::<NativeRootLeaseShape>())
                .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
        );
    }
}
