//! Beta-only delegation to the existing native host lifecycle. No Verified
//! witness is created. Refusals retain authority; destructors never settle it.

use super::super::StoredBetaPackage;
use super::{BetaNativeAdmissionError as Error, BetaNativePackagePin, Reservation};
use crate::operation::{with_external_callback, RepositoryOperationError, RepositoryRuntime};
use crate::tree_reader::{with_verified_tree_resource, TreeResourceError};
use std::mem::size_of;
use std::sync::{atomic::Ordering, Arc};
use zephium_core::extensions::{
    ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipPhase,
    ExtensionPackagePinAcquisitionBinding, ExtensionPackagePinHeldBinding,
    ExtensionRuntimeBackendTarget, ExtensionRuntimeGeneration, ExtensionRuntimeOperationAuthority,
};
use zephium_extension_runtime_api::{
    ExtensionPackageAccess, ExtensionPackageAccessError, ExtensionPackageAccessPort,
    ExtensionRuntimeHostActivation, ExtensionRuntimeHostActivationBinding,
    ExtensionRuntimeHostFactory, ExtensionRuntimeNativeIdentityExpectation,
    ExtensionRuntimeNativeOwnerId, ExtensionRuntimeNativeRootLeasePort,
    ExtensionRuntimeNativeRootVisitor, ExtensionRuntimeResource, ExtensionRuntimeResourceBinding,
    ExtensionRuntimeResourcePlan, ExtensionRuntimeResourceVisitor, ExtensionRuntimeTarget,
    MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
};
use zephium_private_fs::SealedPrivateDirectory;

// Fixed wrapper, reservation, gate, Arc, directory-chain and refusal allowance.
// Dynamic package/index/plan/eligibility bytes are charged separately below.
const CONTROL_BYTES: usize = 512 * 1024;

struct Snapshot {
    package: Arc<StoredBetaPackage>,
    root: Arc<SealedPrivateDirectory>,
    runtime: RepositoryRuntime,
    reservation: Arc<Reservation>,
}
impl Snapshot {
    fn active(&self) -> Result<(), ExtensionPackageAccessError> {
        if !self.runtime.is_healthy() || !self.reservation.active.load(Ordering::Acquire) {
            return Err(ExtensionPackageAccessError::Inactive);
        }
        Ok(())
    }
    fn fresh(&self) -> Result<(), ExtensionPackageAccessError> {
        self.active()?;
        self.package
            .artifact
            .revalidate()
            .map_err(|_| ExtensionPackageAccessError::Inactive)
    }
}

struct Provider {
    snapshot: Arc<Snapshot>,
    plan: [u8; 32],
    root_lease: Option<Box<NativeRoot>>,
    retained: usize,
}
struct NativeRoot {
    snapshot: Arc<Snapshot>,
    used: bool,
}

impl ExtensionPackageAccessPort for Provider {
    fn publisher_native_host(
        &self,
    ) -> Option<&zephium_core::extensions::ExtensionPublisherNativeHostRequirement> {
        self.snapshot.package.artifact.publisher_native_host()
    }
    fn retained_bytes(&self) -> usize {
        self.retained
    }
    fn visit_resource(
        &mut self,
        resource: ExtensionRuntimeResource,
        visitor: &mut dyn ExtensionRuntimeResourceVisitor,
    ) -> Result<(), ExtensionPackageAccessError> {
        let snapshot = &self.snapshot;
        let operation = snapshot.runtime.enter().map_err(operation_error)?;
        snapshot.active()?;
        let index = snapshot.package.artifact.index();
        let file = index
            .files()
            .get(resource.ordinal() as usize)
            .ok_or(ExtensionPackageAccessError::ResourceNotDeclared)?;
        if !resource.authenticates(
            self.plan,
            resource.ordinal(),
            file.path().as_str(),
            file.length(),
            file.sha256(),
        ) {
            return Err(ExtensionPackageAccessError::ResourceNotDeclared);
        }
        let _barrier = snapshot
            .runtime
            .begin_delegated_callback()
            .map_err(operation_error)?;
        drop(operation);
        let result = with_verified_tree_resource(&snapshot.root, index, file.path(), |reader| {
            with_external_callback(|| visitor.visit(reader))
        });
        if let Err(error) = result {
            match error {
                TreeResourceError::Unavailable => {
                    return Err(ExtensionPackageAccessError::ResourceUnavailable)
                }
                TreeResourceError::NotDeclared => {
                    return Err(ExtensionPackageAccessError::ResourceNotDeclared)
                }
                _ => {
                    snapshot.runtime.poison();
                    return Err(ExtensionPackageAccessError::ResourceIdentityMismatch);
                }
            }
        }
        // The runtime API captures the visitor result. Integrity and owner
        // checks must finish even when the visitor rejected provisional bytes.
        snapshot.active()
    }
    fn take_native_root_lease(
        &mut self,
        target: ExtensionRuntimeTarget,
    ) -> Result<Box<dyn ExtensionRuntimeNativeRootLeasePort>, ExtensionPackageAccessError> {
        let _operation = self.snapshot.runtime.enter().map_err(operation_error)?;
        self.snapshot.fresh()?;
        if target != ExtensionRuntimeTarget::NativeWebExtension {
            return Err(ExtensionPackageAccessError::NativeRootUnavailable);
        }
        self.root_lease
            .take()
            .map(|root| root as Box<dyn ExtensionRuntimeNativeRootLeasePort>)
            .ok_or(ExtensionPackageAccessError::NativeRootUnavailable)
    }
}
impl ExtensionRuntimeNativeRootLeasePort for NativeRoot {
    fn visit_native_root(
        &mut self,
        visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
    ) -> Result<(), ExtensionPackageAccessError> {
        if self.used {
            return Err(ExtensionPackageAccessError::Inactive);
        }
        self.used = true;
        let operation = self.snapshot.runtime.enter().map_err(operation_error)?;
        self.snapshot.fresh()?;
        let _barrier = self
            .snapshot
            .runtime
            .begin_delegated_callback()
            .map_err(operation_error)?;
        drop(operation);
        let result = self
            .snapshot
            .root
            .with_verified_path(|path| with_external_callback(|| visitor.visit(path)));
        if result.is_err() {
            self.snapshot.runtime.poison();
            return Err(ExtensionPackageAccessError::NativeRootIdentityMismatch);
        }
        self.snapshot.fresh()
    }
}
fn operation_error(error: RepositoryOperationError) -> ExtensionPackageAccessError {
    match error {
        RepositoryOperationError::CallbackReentry => ExtensionPackageAccessError::CallbackReentry,
        _ => ExtensionPackageAccessError::Inactive,
    }
}

/// Lossless pre-native construction refusal. Dropping it leaves the Store
/// journal and same-process reservation unresolved.
#[must_use]
pub struct BetaRuntimeBuildRefusal {
    reason: Error,
    pin: Box<BetaNativePackagePin>,
}
impl BetaRuntimeBuildRefusal {
    /// Stable refusal category.
    pub const fn reason(&self) -> Error {
        self.reason
    }
    /// Recovers the exact pin for explicit pre-native release settlement.
    pub fn into_pin(self) -> BetaNativePackagePin {
        *self.pin
    }
}

/// Move-only Beta package access and its captive Store eligibility. Only the
/// authenticated host-binding operation below can split these authorities.
///
/// ```compile_fail
/// fn cloneable<T: Clone>() {}
/// cloneable::<zephium_extension_repository::beta::BetaRuntimePackageAccess>();
/// ```
#[must_use]
pub struct BetaRuntimePackageAccess {
    access: ExtensionPackageAccess,
    binding: ExtensionPackagePinAcquisitionBinding,
    snapshot: Arc<Snapshot>,
    expectation: zephium_core::extensions::ExtensionExpectedNativeOwnershipIdentity,
}

impl BetaNativePackagePin {
    /// Builds the complete resource provider and preallocates its exact-once
    /// native root lease off the UI thread. Performs complete byte verification.
    pub fn into_runtime_access(self) -> Result<BetaRuntimePackageAccess, BetaRuntimeBuildRefusal> {
        let build = (|| {
            self.package.verify().map_err(|_| Error::Package)?;
            if !self.runtime.is_healthy() || !self.reservation.active.load(Ordering::Acquire) {
                return Err(Error::Package);
            }
            self.root
                .with_verified_path(|_| ())
                .map_err(|_| Error::Package)?;
            let files = self.package.artifact.index().files();
            let entries = files
                .iter()
                .map(|file| {
                    ExtensionRuntimeResourceBinding::try_new(
                        file.path().as_str(),
                        file.length(),
                        file.sha256(),
                    )
                    .map_err(|_| Error::Package)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let plan =
                ExtensionRuntimeResourcePlan::try_new(entries).map_err(|_| Error::Package)?;
            let retained = self
                .package
                .retained_bytes()
                .checked_add(CONTROL_BYTES)
                .ok_or(Error::Capacity)?;
            let complete = retained
                .checked_add(plan.retained_bytes())
                .and_then(|bytes| bytes.checked_add(self.binding.retained_bytes()))
                .ok_or(Error::Capacity)?;
            if complete > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES {
                return Err(Error::Capacity);
            }
            Ok((plan, retained))
        })();
        let (plan, retained) = match build {
            Ok(value) => value,
            Err(reason) => {
                return Err(BetaRuntimeBuildRefusal {
                    reason,
                    pin: Box::new(self),
                })
            }
        };
        let snapshot = Arc::new(Snapshot {
            package: Arc::clone(&self.package),
            root: Arc::clone(&self.root),
            runtime: self.runtime.clone(),
            reservation: Arc::clone(&self.reservation),
        });
        let provider = Box::new(Provider {
            snapshot: Arc::clone(&snapshot),
            plan: plan.digest(),
            root_lease: Some(Box::new(NativeRoot {
                snapshot: Arc::clone(&snapshot),
                used: false,
            })),
            retained,
        });
        let access = match ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            plan,
            provider,
        ) {
            Ok(access) => access,
            Err(refusal) => {
                drop(refusal);
                return Err(BetaRuntimeBuildRefusal {
                    reason: Error::Capacity,
                    pin: Box::new(self),
                });
            }
        };
        Ok(BetaRuntimePackageAccess {
            access,
            binding: self.binding,
            snapshot,
            expectation: self.expectation,
        })
    }
}

/// A host binding refusal keeps every source, row and operation capability
/// captive. An impossible routing mismatch is never projected as native absence.
#[must_use]
pub struct BetaRuntimeHostRefusal {
    reason: Error,
    host_reason: Option<zephium_extension_runtime_api::ExtensionRuntimeHostBindError>,
    authority: HostRefused,
}
enum HostRefused {
    Before(Box<(BetaRuntimePackageAccess, ExtensionNativeOwnershipEntry)>),
    After(
        Box<(
            BetaRuntimeRecoveryToken,
            ExtensionNativeOwnershipEntry,
            ExtensionPackageAccess,
            ExtensionRuntimeOperationAuthority,
        )>,
    ),
}
impl BetaRuntimeHostRefusal {
    /// Original trusted-host refusal, when binding reached the host factory.
    pub const fn host_reason(
        &self,
    ) -> Option<zephium_extension_runtime_api::ExtensionRuntimeHostBindError> {
        self.host_reason
    }

    /// Stable refusal category.
    pub const fn reason(&self) -> Error {
        self.reason
    }
    /// Recovers pre-native authority when Core proves the exact lineage.
    pub fn try_into_pin(
        self,
    ) -> Result<(BetaNativePackagePin, ExtensionNativeOwnershipEntry), BetaRuntimeRecoveryRefusal>
    {
        self.try_into_access()
            .map(|(access, entry)| (access.into_pin(), entry))
    }
    /// Restores the unchanged pre-native package-access wrapper, including its
    /// consumed flags, without rebuilding or re-reading resources.
    pub fn try_into_access(
        self,
    ) -> Result<(BetaRuntimePackageAccess, ExtensionNativeOwnershipEntry), BetaRuntimeRecoveryRefusal>
    {
        match self.authority {
            HostRefused::Before(parts) => Ok(*parts),
            HostRefused::After(parts) => {
                let (token, entry, access, operation) = *parts;
                token
                    .rejoin_access(access, operation)
                    .map(|access| (access, entry))
            }
        }
    }
}

/// Native activation and its separately retained, move-only Beta recovery token.
#[must_use]
pub struct BetaRuntimeHostActivation {
    activation: ExtensionRuntimeHostActivation,
    recovery: BetaRuntimeRecoveryToken,
}
impl BetaRuntimeHostActivation {
    /// Largest future owner charge including its retained recovery token.
    pub fn maximum_future_retained_bytes(&self) -> usize {
        self.activation
            .maximum_future_retained_bytes()
            .saturating_add(self.recovery.retained_bytes())
    }

    /// Transfers the existing host lifecycle and the matching cleanup token.
    pub fn into_parts(self) -> (ExtensionRuntimeHostActivation, BetaRuntimeRecoveryToken) {
        (self.activation, self.recovery)
    }
    /// Stable total including the companion recovery wrapper.
    pub fn retained_bytes(&self) -> usize {
        self.activation
            .retained_bytes()
            .saturating_add(size_of::<BetaRuntimeRecoveryToken>())
    }
}

/// Same-process cleanup token. It cannot read resources, activate code, clear
/// Store, or be reconstructed from disk. Native absence must return exact access.
///
/// ```compile_fail
/// fn cloneable<T: Clone>() {}
/// cloneable::<zephium_extension_repository::beta::BetaRuntimeRecoveryToken>();
/// ```
#[must_use]
pub struct BetaRuntimeRecoveryToken {
    held: ExtensionPackagePinHeldBinding,
    snapshot: Arc<Snapshot>,
    expectation: zephium_core::extensions::ExtensionExpectedNativeOwnershipIdentity,
}
/// A wrong-provider or wrong-lineage recovery retains all original capabilities.
#[must_use]
pub struct BetaRuntimeRecoveryRefusal {
    _authority: Box<RecoveryRefused>,
}
enum RecoveryRefused {
    Foreign {
        _token: BetaRuntimeRecoveryToken,
        _access: ExtensionPackageAccess,
        _operation: ExtensionRuntimeOperationAuthority,
    },
    Routing {
        _token: BetaRuntimeRecoveryToken,
        _access: ExtensionPackageAccess,
        _operation: ExtensionRuntimeOperationAuthority,
    },
    Lineage {
        _refusal: zephium_core::extensions::ExtensionPackagePinRecombineRefusal,
        _access: ExtensionPackageAccess,
        _snapshot: Arc<Snapshot>,
    },
}

impl BetaRuntimePackageAccess {
    /// Exact original pre-native row retained by this source reservation.
    pub fn matches_preparing_ownership_entry(&self, row: &ExtensionNativeOwnershipEntry) -> bool {
        row == &self.snapshot.reservation.entry
    }

    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "used by distribution's private signed-package integration fixture"
    )]
    pub(crate) fn test_access(&mut self) -> &mut ExtensionPackageAccess {
        &mut self.access
    }
    /// Conservative pre-host total, including bound eligibility and wrappers.
    pub fn retained_bytes(&self) -> usize {
        self.access
            .retained_bytes()
            .saturating_add(self.binding.retained_bytes())
            .saturating_add(size_of::<Self>())
    }
    /// Original authenticated native identifier to persist before native work.
    pub const fn expected_native_identity(
        &self,
    ) -> zephium_core::extensions::ExtensionExpectedNativeOwnershipIdentity {
        self.expectation
    }
    /// Binds only the exact first MayOwn row and its enabled Store eligibility.
    /// The service must perform its immediate Store freshness fence before this
    /// call. Factory reservation is non-native; activation remains lifecycle-owned.
    pub fn try_into_host_activation(
        self,
        entry: ExtensionNativeOwnershipEntry,
        generation: ExtensionRuntimeGeneration,
        factory: &mut ExtensionRuntimeHostFactory,
        companion_bytes: usize,
        transient_bytes: usize,
    ) -> Result<BetaRuntimeHostActivation, BetaRuntimeHostRefusal> {
        let before = |reason, access, entry| BetaRuntimeHostRefusal {
            reason,
            host_reason: None,
            authority: HostRefused::Before(Box::new((access, entry))),
        };
        let pin = &self.binding;
        let initial = &self.snapshot.reservation.entry;
        if self.snapshot.fresh().is_err() {
            return Err(before(Error::Package, self, entry));
        }
        if entry.key() != initial.key()
            || entry.package() != pin.package()
            || entry.source() != initial.source()
            || entry.operation() != initial.operation()
            || entry.native_incarnation() != initial.native_incarnation()
            || entry.runtime_backend() != initial.runtime_backend()
            || entry.store_catalog_revision() != pin.store_catalog_revision()
            || entry.store_install_revision() != pin.store_install_revision()
            || entry.store_grant_revision() != pin.store_grant_revision()
            || entry.grant_digest() != pin.grant_digest()
            || entry.intent() != ExtensionNativeOwnershipIntent::Acquire
            || entry.phase() != ExtensionNativeOwnershipPhase::NativeMayOwn
            || entry.revision().get() != 2
            || entry.expected_native_identity() != Some(self.expectation)
            || entry.native_identity().is_some()
        {
            return Err(before(Error::Ownership, self, entry));
        }
        let Some(companion) = companion_bytes.checked_add(size_of::<BetaRuntimeRecoveryToken>())
        else {
            return Err(before(Error::Capacity, self, entry));
        };
        if companion
            .checked_add(transient_bytes)
            .and_then(|bytes| bytes.checked_add(self.retained_bytes()))
            .is_none_or(|bytes| bytes > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES)
        {
            return Err(before(Error::Capacity, self, entry));
        }
        let identity =
            match ExtensionRuntimeNativeOwnerId::from_encoded_bytes(self.expectation.bytes()) {
                Ok(id) => id,
                Err(_) => return Err(before(Error::Ownership, self, entry)),
            };
        let expectation = match entry.runtime_backend() {
            ExtensionRuntimeBackendTarget::MacosNative => {
                ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(identity)
            }
            ExtensionRuntimeBackendTarget::WindowsNative => {
                ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(identity)
            }
            _ => return Err(before(Error::Backend, self, entry)),
        };
        let (held, operation) = self
            .binding
            .into_runtime_parts(generation)
            .into_held_binding_and_operation_authority();
        let token = BetaRuntimeRecoveryToken {
            held,
            snapshot: self.snapshot,
            expectation: self.expectation,
        };
        let after = |reason, token, entry, access, operation| BetaRuntimeHostRefusal {
            reason,
            host_reason: None,
            authority: HostRefused::After(Box::new((token, entry, access, operation))),
        };
        let binding = match ExtensionRuntimeHostActivationBinding::try_from_authenticated_repository(
            entry,
            self.access,
            operation,
            expectation,
        ) {
            Ok(binding) => binding,
            Err(refusal) => {
                let (entry, access, operation, _) = refusal.into_parts();
                return Err(after(Error::Ownership, token, entry, access, operation));
            }
        };
        match factory.bind_activation_with_retained_byte_charges(
            binding,
            companion,
            transient_bytes,
        ) {
            Ok(activation) => Ok(BetaRuntimeHostActivation {
                activation,
                recovery: token,
            }),
            Err(refusal) => {
                let host_reason = refusal.reason();
                let (entry, access, operation, _) = refusal.cancel_into_parts();
                let mut result = after(Error::Ownership, token, entry, access, operation);
                result.host_reason = Some(host_reason);
                Err(result)
            }
        }
    }
    /// Cancels a pre-native access value back to its reserved pin. A transferred
    /// native root cannot be obtained through this opaque pre-host wrapper.
    pub fn into_pin(self) -> BetaNativePackagePin {
        let pin = BetaNativePackagePin {
            package: Arc::clone(&self.snapshot.package),
            root: Arc::clone(&self.snapshot.root),
            runtime: self.snapshot.runtime.clone(),
            reservation: Arc::clone(&self.snapshot.reservation),
            binding: self.binding,
            expectation: self.expectation,
        };
        drop(self.access);
        pin
    }
}
impl BetaRuntimeRecoveryToken {
    /// Inline recovery charge; shared package state is charged by its provider.
    pub const fn retained_bytes(&self) -> usize {
        size_of::<Self>()
    }
    /// Rejoins lifecycle-returned access after native absence. Core preserves
    /// exact stable lineage across later Store grant rebindings.
    pub fn rejoin(
        self,
        access: ExtensionPackageAccess,
        operation: ExtensionRuntimeOperationAuthority,
    ) -> Result<BetaNativePackagePin, BetaRuntimeRecoveryRefusal> {
        self.rejoin_access(access, operation)
            .map(BetaRuntimePackageAccess::into_pin)
    }
    fn rejoin_access(
        self,
        access: ExtensionPackageAccess,
        operation: ExtensionRuntimeOperationAuthority,
    ) -> Result<BetaRuntimePackageAccess, BetaRuntimeRecoveryRefusal> {
        let Some(provider) = access.delegated_provider::<Provider>() else {
            return Err(BetaRuntimeRecoveryRefusal {
                _authority: Box::new(RecoveryRefused::Foreign {
                    _token: self,
                    _access: access,
                    _operation: operation,
                }),
            });
        };
        if !Arc::ptr_eq(&provider.snapshot, &self.snapshot)
            || access.target() != ExtensionRuntimeTarget::NativeWebExtension
            || provider.plan != access.resources().digest()
        {
            return Err(BetaRuntimeRecoveryRefusal {
                _authority: Box::new(RecoveryRefused::Routing {
                    _token: self,
                    _access: access,
                    _operation: operation,
                }),
            });
        }
        match self.held.try_recombine(operation) {
            Ok(binding) => Ok(BetaRuntimePackageAccess {
                access,
                binding,
                snapshot: self.snapshot,
                expectation: self.expectation,
            }),
            Err(refusal) => Err(BetaRuntimeRecoveryRefusal {
                _authority: Box::new(RecoveryRefused::Lineage {
                    _refusal: refusal,
                    _access: access,
                    _snapshot: self.snapshot,
                }),
            }),
        }
    }
}
impl BetaRuntimeRecoveryRefusal {
    /// Whether a same-provider ownership-routing invariant failed.
    pub fn requires_fail_stop(&self) -> bool {
        !matches!(&*self._authority, RecoveryRefused::Foreign { .. })
    }
    /// Wrong-provider refusals return every original input unchanged.
    pub fn try_into_parts(
        self,
    ) -> Result<
        (
            BetaRuntimeRecoveryToken,
            ExtensionPackageAccess,
            ExtensionRuntimeOperationAuthority,
        ),
        Self,
    > {
        match *self._authority {
            RecoveryRefused::Foreign {
                _token,
                _access,
                _operation,
            } => Ok((_token, _access, _operation)),
            other => Err(Self {
                _authority: Box::new(other),
            }),
        }
    }
}

macro_rules! opaque_debug { ($($name:ident),+ $(,)?) => { $(impl std::fmt::Debug for $name { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct(stringify!($name)).finish_non_exhaustive() } })+ }; }
opaque_debug!(
    BetaRuntimePackageAccess,
    BetaRuntimeBuildRefusal,
    BetaRuntimeHostActivation,
    BetaRuntimeHostRefusal,
    BetaRuntimeRecoveryToken,
    BetaRuntimeRecoveryRefusal
);
