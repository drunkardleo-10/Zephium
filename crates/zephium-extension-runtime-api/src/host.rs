//! Trusted host-side bindings for native extension runtimes.
//!
//! This module is the linear authority boundary between the serialized
//! extension service, authenticated repository access, and an engine-owned
//! native runtime registry. Binding is side-effect free: it may reserve
//! bounded process-local state, but it must not dispatch to the UI thread or
//! call a native API. Native ownership-changing work remains behind the
//! lifecycle ports returned by the engine.

use std::error::Error;
use std::fmt;
use std::mem::size_of;

use zephium_core::extensions::{
    ExtensionActiveTabGrantWitness, ExtensionDocumentAuthorityWitness, ExtensionDocumentPurpose,
    ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipEntryCas,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipPhase,
    ExtensionOperationAuthorityDenial, ExtensionRuntimeBackendTarget, ExtensionRuntimeFingerprint,
    ExtensionRuntimeOperationAuthority, ExtensionUserInvocationKind,
};

use crate::{
    ExtensionPackageAccess, ExtensionRuntimeActivationBuildError,
    ExtensionRuntimeActivationRequest, ExtensionRuntimeLifecyclePort,
    ExtensionRuntimeNativeOwnerId, ExtensionRuntimeOwnershipEvidence,
    ExtensionRuntimeOwnershipPort, ExtensionRuntimeRecoveryBuildError,
    ExtensionRuntimeRecoveryExpectation, ExtensionRuntimeRecoveryRequest, ExtensionRuntimeTarget,
    MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
};

/// Hard envelope for one fail-stop authority-routing quarantine.
///
/// Ordinary owner state remains inside
/// [`MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES`]. A trusted-engine invariant
/// failure can transiently retain the original owner state plus one substituted
/// authority so neither capability escapes. The serialized service may retain
/// at most one such quarantine while entering its fatal-invariant path; it must
/// never treat this envelope as ordinary runtime capacity.
pub const MAX_EXTENSION_RUNTIME_HOST_QUARANTINE_RETAINED_BYTES: usize =
    (2 * MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES) + (64 * 1024);

fn checked_retained_sum(values: impl IntoIterator<Item = usize>) -> usize {
    values
        .into_iter()
        .try_fold(0_usize, usize::checked_add)
        .unwrap_or(usize::MAX)
}

fn authority_exclusive_retained_bytes(authority: &ExtensionRuntimeOperationAuthority) -> usize {
    authority
        .retained_bytes()
        .saturating_sub(size_of::<ExtensionRuntimeOperationAuthority>())
}

/// Expected native identity selected from authenticated package metadata.
///
/// Native backends require the exact catalog-authenticated Chromium ID.
/// Compatibility runtimes deliberately have no platform-native identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeNativeIdentityExpectation {
    /// Exact identifier Zephium must assign to and read back from WebKit.
    MacosWebExtension(ExtensionRuntimeNativeOwnerId),
    /// Exact identifier WebView2 must return from the activated extension.
    WindowsWebView2Extension(ExtensionRuntimeNativeOwnerId),
    /// Zephium compatibility runtime with no platform extension identifier.
    Compatibility,
}

impl ExtensionRuntimeNativeIdentityExpectation {
    /// Runtime family required by this expectation.
    #[must_use]
    pub const fn target(self) -> ExtensionRuntimeTarget {
        match self {
            Self::MacosWebExtension(_) | Self::WindowsWebView2Extension(_) => {
                ExtensionRuntimeTarget::NativeWebExtension
            }
            Self::Compatibility => ExtensionRuntimeTarget::Compatibility,
        }
    }

    /// Exact durable backend required by this expectation.
    #[must_use]
    pub const fn backend(self) -> Option<ExtensionRuntimeBackendTarget> {
        match self {
            Self::MacosWebExtension(_) => Some(ExtensionRuntimeBackendTarget::MacosNative),
            Self::WindowsWebView2Extension(_) => Some(ExtensionRuntimeBackendTarget::WindowsNative),
            Self::Compatibility => None,
        }
    }

    fn accepts_backend(self, backend: ExtensionRuntimeBackendTarget) -> bool {
        matches!(
            (self, backend),
            (
                Self::MacosWebExtension(_),
                ExtensionRuntimeBackendTarget::MacosNative
            ) | (
                Self::WindowsWebView2Extension(_),
                ExtensionRuntimeBackendTarget::WindowsNative
            ) | (
                Self::Compatibility,
                ExtensionRuntimeBackendTarget::MacosCompatibility
                    | ExtensionRuntimeBackendTarget::LinuxCompatibility,
            )
        )
    }

    fn accepts_evidence(self, evidence: ExtensionRuntimeOwnershipEvidence) -> bool {
        matches!(
            (self, evidence),
            (Self::MacosWebExtension(expected), ExtensionRuntimeOwnershipEvidence::MacosWebExtension(actual))
                | (Self::WindowsWebView2Extension(expected), ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(actual))
                if expected == actual
        ) || matches!(
            (self, evidence),
            (
                Self::Compatibility,
                ExtensionRuntimeOwnershipEvidence::Compatibility
            )
        )
    }

    fn recovery_expectation(
        backend: ExtensionRuntimeBackendTarget,
        entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<ExtensionRuntimeRecoveryExpectation, ExtensionRuntimeHostRecoveryBindingError> {
        let expected = entry
            .native_identity()
            .map(|identity| {
                ExtensionRuntimeNativeOwnerId::from_encoded_bytes(identity.bytes())
                    .map_err(|_| ExtensionRuntimeHostRecoveryBindingError::NativeIdentityMismatch)
            })
            .transpose()?;
        match backend {
            ExtensionRuntimeBackendTarget::MacosNative => {
                Ok(ExtensionRuntimeRecoveryExpectation::MacosWebExtension { expected })
            }
            ExtensionRuntimeBackendTarget::WindowsNative => {
                Ok(ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension { expected })
            }
            ExtensionRuntimeBackendTarget::MacosCompatibility
            | ExtensionRuntimeBackendTarget::LinuxCompatibility
                if expected.is_none() =>
            {
                Ok(ExtensionRuntimeRecoveryExpectation::Compatibility)
            }
            ExtensionRuntimeBackendTarget::MacosCompatibility
            | ExtensionRuntimeBackendTarget::LinuxCompatibility => {
                Err(ExtensionRuntimeHostRecoveryBindingError::NativeIdentityMismatch)
            }
        }
    }
}

/// Exact durable address of one native-owner registry row.
///
/// This copied structural value is non-authorizing. Authority requires the
/// matching engine proxy and, for live operation access, the published
/// move-only operation authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionRuntimeOwnerAddress {
    cas: ExtensionNativeOwnershipEntryCas,
    backend: ExtensionRuntimeBackendTarget,
}

impl ExtensionRuntimeOwnerAddress {
    fn from_entry(entry: &ExtensionNativeOwnershipEntry) -> Self {
        Self {
            cas: entry.cas(),
            backend: entry.runtime_backend(),
        }
    }

    /// Exact durable compare-and-swap address.
    #[must_use]
    pub const fn cas(self) -> ExtensionNativeOwnershipEntryCas {
        self.cas
    }

    /// Exact backend bound to this owner.
    #[must_use]
    pub const fn backend(self) -> ExtensionRuntimeBackendTarget {
        self.backend
    }
}

/// Nonzero process-local generation of one engine registry reservation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionRuntimeHostRegistryGeneration(u64);

impl ExtensionRuntimeHostRegistryGeneration {
    /// Constructs a nonzero process-local generation.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Returns the exact generation value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable refusal to construct a fresh activation binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeHostActivationBindingError {
    /// The durable row is not an acquisition operation.
    OwnershipIntentMismatch,
    /// The durable row is not at the exact pre-native-call frontier.
    OwnershipPhaseMismatch,
    /// The pre-native-call row is not revision two.
    OwnershipRevisionMismatch,
    /// A fresh activation row already carries native identity.
    NativeIdentityAlreadyPresent,
    /// Store-derived runtime authority does not match the durable row.
    RuntimeFingerprintMismatch,
    /// Runtime authority belongs to another package-pin operation lineage.
    RuntimeAuthorityLineageMismatch,
    /// Package access targets another runtime family.
    RuntimeTargetMismatch,
    /// The authenticated native identity expectation names another backend.
    NativeIdentityExpectationMismatch,
    /// Retained-memory accounting overflowed.
    RetainedBytesOverflow,
    /// The pre-host authority already exceeds the per-owner ceiling.
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionRuntimeHostActivationBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid extension runtime host activation binding: {self:?}"
        )
    }
}

impl Error for ExtensionRuntimeHostActivationBindingError {}

/// Lossless refusal to construct a fresh activation binding.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeHostActivationBindingRefusal;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeHostActivationBindingRefusal>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeHostActivationBindingRefusal;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeHostActivationBindingRefusal>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeHostActivationBindingRefusal;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeHostActivationBindingRefusal>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeHostActivationBindingRefusal;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeHostActivationBindingRefusal>();
/// ```
#[must_use = "the refusal retains every move-only activation input"]
pub struct ExtensionRuntimeHostActivationBindingRefusal {
    reason: ExtensionRuntimeHostActivationBindingError,
    parts: Box<ExtensionRuntimeHostActivationBindingParts>,
}

struct ExtensionRuntimeHostActivationBindingParts {
    entry: ExtensionNativeOwnershipEntry,
    access: ExtensionPackageAccess,
    authority: ExtensionRuntimeOperationAuthority,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
}

impl ExtensionRuntimeHostActivationBindingRefusal {
    /// Stable, identity-free refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimeHostActivationBindingError {
        self.reason
    }

    /// Recovers every exact constructor input.
    #[must_use = "all returned values retain runtime and package authority"]
    pub fn into_parts(
        self,
    ) -> (
        ExtensionNativeOwnershipEntry,
        ExtensionPackageAccess,
        ExtensionRuntimeOperationAuthority,
        ExtensionRuntimeNativeIdentityExpectation,
    ) {
        let ExtensionRuntimeHostActivationBindingParts {
            entry,
            access,
            authority,
            expectation,
        } = *self.parts;
        (entry, access, authority, expectation)
    }
}

impl fmt::Debug for ExtensionRuntimeHostActivationBindingRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeHostActivationBindingRefusal")
            .field("reason", &self.reason)
            .field("authority", &"[redacted]")
            .finish()
    }
}

/// Atomic package, operation, and durable-row authority for fresh activation.
///
/// The value is move-only, non-serializable, and has no extraction path on
/// success. Only an engine-supplied [`ExtensionRuntimeHostFactory`] can consume
/// it.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeHostActivationBinding;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeHostActivationBinding>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeHostActivationBinding;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeHostActivationBinding>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeHostActivationBinding;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeHostActivationBinding>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeHostActivationBinding;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeHostActivationBinding>();
/// ```
#[must_use = "activation authority must be bound or explicitly recovered from a refusal"]
pub struct ExtensionRuntimeHostActivationBinding {
    entry: ExtensionNativeOwnershipEntry,
    access: ExtensionPackageAccess,
    authority: ExtensionRuntimeOperationAuthority,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
}

impl ExtensionRuntimeHostActivationBinding {
    /// Joins exact Store, repository, and native-identity authority.
    ///
    /// This low-level constructor is reserved for the authenticated repository
    /// bridge. [`ExtensionPackageAccess`] is intentionally opaque here, so this
    /// layer can validate its target and memory bound but cannot independently
    /// prove that its provider bytes belong to the Store row. Product callers
    /// must use the role-specific repository assembler, which first validates
    /// package pin, catalog set/role, operation, incarnation, backend, and
    /// resource-plan lineage. The workspace gate rejects additional shipping
    /// call sites.
    pub fn try_from_authenticated_repository(
        entry: ExtensionNativeOwnershipEntry,
        access: ExtensionPackageAccess,
        authority: ExtensionRuntimeOperationAuthority,
        expectation: ExtensionRuntimeNativeIdentityExpectation,
    ) -> Result<Self, ExtensionRuntimeHostActivationBindingRefusal> {
        let reason = activation_binding_error(&entry, &access, &authority, expectation);
        if let Some(reason) = reason {
            return Err(ExtensionRuntimeHostActivationBindingRefusal {
                reason,
                parts: Box::new(ExtensionRuntimeHostActivationBindingParts {
                    entry,
                    access,
                    authority,
                    expectation,
                }),
            });
        }
        Ok(Self {
            entry,
            access,
            authority,
            expectation,
        })
    }

    /// Exact non-authorizing runtime fingerprint retained by the binding.
    #[must_use]
    pub const fn fingerprint(&self) -> &ExtensionRuntimeFingerprint {
        self.authority.fingerprint()
    }

    /// Exact owner address retained by the binding.
    #[must_use]
    pub fn owner_address(&self) -> ExtensionRuntimeOwnerAddress {
        ExtensionRuntimeOwnerAddress::from_entry(&self.entry)
    }

    /// Conservative retained-memory charge before engine binding.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.access
                    .retained_bytes()
                    .saturating_sub(size_of::<ExtensionPackageAccess>()),
            )
            .saturating_add(
                self.authority
                    .retained_bytes()
                    .saturating_sub(size_of::<ExtensionRuntimeOperationAuthority>()),
            )
    }
}

impl fmt::Debug for ExtensionRuntimeHostActivationBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeHostActivationBinding")
            .field("backend", &self.entry.runtime_backend())
            .field("target", &self.access.target())
            .field("authority", &"[redacted]")
            .finish()
    }
}

fn activation_binding_error(
    entry: &ExtensionNativeOwnershipEntry,
    access: &ExtensionPackageAccess,
    authority: &ExtensionRuntimeOperationAuthority,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
) -> Option<ExtensionRuntimeHostActivationBindingError> {
    use ExtensionRuntimeHostActivationBindingError as Error;

    if entry.intent() != ExtensionNativeOwnershipIntent::Acquire {
        return Some(Error::OwnershipIntentMismatch);
    }
    if entry.phase() != ExtensionNativeOwnershipPhase::NativeMayOwn {
        return Some(Error::OwnershipPhaseMismatch);
    }
    if entry.native_identity().is_some() {
        return Some(Error::NativeIdentityAlreadyPresent);
    }
    if entry.revision().get() != 2 {
        return Some(Error::OwnershipRevisionMismatch);
    }
    if !entry_matches_fingerprint(entry, authority.fingerprint()) {
        return Some(Error::RuntimeFingerprintMismatch);
    }
    if !authority.matches_native_ownership_lineage(entry) {
        return Some(Error::RuntimeAuthorityLineageMismatch);
    }
    if access.target() != expectation.target() {
        return Some(Error::RuntimeTargetMismatch);
    }
    if !expectation.accepts_backend(entry.runtime_backend()) {
        return Some(Error::NativeIdentityExpectationMismatch);
    }
    let retained = size_of::<ExtensionRuntimeHostActivationBinding>()
        .checked_add(
            access
                .retained_bytes()
                .saturating_sub(size_of::<ExtensionPackageAccess>()),
        )
        .and_then(|value| {
            value.checked_add(
                authority
                    .retained_bytes()
                    .saturating_sub(size_of::<ExtensionRuntimeOperationAuthority>()),
            )
        });
    match retained {
        None => Some(Error::RetainedBytesOverflow),
        Some(value) if value > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES => {
            Some(Error::RetainedBytesExceeded)
        }
        Some(_) => None,
    }
}

fn entry_matches_fingerprint(
    entry: &ExtensionNativeOwnershipEntry,
    fingerprint: &ExtensionRuntimeFingerprint,
) -> bool {
    let instance = fingerprint.instance();
    entry.key().profile() == instance.profile()
        && entry.key().install_id() == instance.install_id()
        && entry.key().browsing_context() == fingerprint.browsing_context()
        && entry.package() == fingerprint.package()
        && entry.store_catalog_revision() == fingerprint.catalog_revision()
        && entry.store_install_revision() == fingerprint.install_revision()
        && entry.store_grant_revision() == fingerprint.grant_revision()
        && entry.grant_digest() == fingerprint.grant_digest()
}

/// Borrowed, non-authorizing activation description passed to the engine.
pub struct ExtensionRuntimeHostActivationContext<'binding> {
    owner: ExtensionRuntimeOwnerAddress,
    fingerprint: &'binding ExtensionRuntimeFingerprint,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
    target: ExtensionRuntimeTarget,
}

impl ExtensionRuntimeHostActivationContext<'_> {
    /// Exact durable owner address.
    #[must_use]
    pub const fn owner(&self) -> ExtensionRuntimeOwnerAddress {
        self.owner
    }

    /// Complete non-authorizing runtime fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> &ExtensionRuntimeFingerprint {
        self.fingerprint
    }

    /// Authenticated expected native identity.
    #[must_use]
    pub const fn identity_expectation(&self) -> ExtensionRuntimeNativeIdentityExpectation {
        self.expectation
    }

    /// Runtime family selected by authenticated package metadata.
    #[must_use]
    pub const fn target(&self) -> ExtensionRuntimeTarget {
        self.target
    }
}

impl fmt::Debug for ExtensionRuntimeHostActivationContext<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeHostActivationContext")
            .field("owner", &"[redacted]")
            .field("backend", &self.owner.backend())
            .field("target", &self.target)
            .finish()
    }
}

/// Stable refusal to project a cleanup-only recovery binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeHostRecoveryBindingError {
    /// The row is not one of the durable possible-owner states.
    OwnershipStateMismatch,
    /// Durable native identity is incompatible with the selected backend.
    NativeIdentityMismatch,
}

impl fmt::Display for ExtensionRuntimeHostRecoveryBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid extension runtime recovery binding: {self:?}"
        )
    }
}

impl Error for ExtensionRuntimeHostRecoveryBindingError {}

/// Lossless refusal to project a cleanup-only recovery binding.
#[must_use = "the refusal retains the exact durable possible-owner row"]
pub struct ExtensionRuntimeHostRecoveryBindingRefusal {
    reason: ExtensionRuntimeHostRecoveryBindingError,
    entry: Box<ExtensionNativeOwnershipEntry>,
}

impl ExtensionRuntimeHostRecoveryBindingRefusal {
    /// Stable, identity-free refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimeHostRecoveryBindingError {
        self.reason
    }

    /// Recovers the exact supplied durable row.
    #[must_use]
    pub fn into_entry(self) -> ExtensionNativeOwnershipEntry {
        *self.entry
    }
}

impl fmt::Debug for ExtensionRuntimeHostRecoveryBindingRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeHostRecoveryBindingRefusal")
            .field("reason", &self.reason)
            .field("entry", &"[redacted]")
            .finish()
    }
}

/// Cleanup-only projection of one durable possible-owner row.
///
/// Package identity, grants, resources, and operation authority are discarded
/// during construction. This value can reconcile or retire a possible owner,
/// but it can never activate one.
#[must_use = "persisted native ownership must be reconciled explicitly"]
pub struct ExtensionRuntimeHostRecoveryBinding {
    context: ExtensionRuntimeHostRecoveryContext,
}

impl ExtensionRuntimeHostRecoveryBinding {
    /// Projects one structurally valid possible-owner row into cleanup-only state.
    pub fn try_new(
        entry: ExtensionNativeOwnershipEntry,
    ) -> Result<Self, ExtensionRuntimeHostRecoveryBindingRefusal> {
        let valid_state = matches!(
            (entry.intent(), entry.phase()),
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeMayOwn
                    | ExtensionNativeOwnershipPhase::NativeOwned,
            ) | (
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
            )
        );
        if !valid_state {
            return Err(ExtensionRuntimeHostRecoveryBindingRefusal {
                reason: ExtensionRuntimeHostRecoveryBindingError::OwnershipStateMismatch,
                entry: Box::new(entry),
            });
        }
        let expectation = match ExtensionRuntimeNativeIdentityExpectation::recovery_expectation(
            entry.runtime_backend(),
            &entry,
        ) {
            Ok(expectation) => expectation,
            Err(reason) => {
                return Err(ExtensionRuntimeHostRecoveryBindingRefusal {
                    reason,
                    entry: Box::new(entry),
                });
            }
        };
        let context = ExtensionRuntimeHostRecoveryContext {
            owner: ExtensionRuntimeOwnerAddress::from_entry(&entry),
            expectation,
        };
        Ok(Self { context })
    }

    /// Exact cleanup-only context retained by this binding.
    #[must_use]
    pub const fn context(&self) -> &ExtensionRuntimeHostRecoveryContext {
        &self.context
    }
}

impl fmt::Debug for ExtensionRuntimeHostRecoveryBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeHostRecoveryBinding")
            .field("context", &self.context)
            .finish()
    }
}

/// Compact cleanup-only engine binding context.
#[derive(Clone, Copy)]
pub struct ExtensionRuntimeHostRecoveryContext {
    owner: ExtensionRuntimeOwnerAddress,
    expectation: ExtensionRuntimeRecoveryExpectation,
}

impl ExtensionRuntimeHostRecoveryContext {
    /// Exact durable possible-owner address.
    #[must_use]
    pub const fn owner(self) -> ExtensionRuntimeOwnerAddress {
        self.owner
    }

    /// Backend-bound cleanup expectation.
    #[must_use]
    pub const fn expectation(self) -> ExtensionRuntimeRecoveryExpectation {
        self.expectation
    }
}

impl fmt::Debug for ExtensionRuntimeHostRecoveryContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeHostRecoveryContext")
            .field("owner", &"[redacted]")
            .field("backend", &self.owner.backend())
            .field("expectation", &self.expectation)
            .finish()
    }
}

/// Closed engine-side binding or publication failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeHostBindError {
    /// The engine or selected native runtime is unavailable.
    Unavailable,
    /// The backend is unsupported on this platform.
    UnsupportedBackend,
    /// A bounded engine or native resource pool is full.
    CapacityExceeded,
    /// Another exact reservation already owns this durable address.
    OwnerConflict,
    /// The engine has sealed new extension-runtime ingress.
    Sealed,
    /// A process-local generation or operation identity was exhausted.
    IdentityExhausted,
    /// Combined retained-memory accounting overflowed.
    RetainedBytesOverflow,
    /// Combined host state exceeds the per-owner retained-memory ceiling.
    RetainedBytesExceeded,
    /// An exact registry, evidence, or lifecycle invariant failed.
    InternalInvariant,
}

impl fmt::Display for ExtensionRuntimeHostBindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "extension runtime host binding failed: {self:?}")
    }
}

impl Error for ExtensionRuntimeHostBindError {}

/// Marker for a trusted engine lifecycle proxy.
///
/// Implementations must obey all passive-`Drop`, deadline, ownership, and
/// retained-memory rules of [`ExtensionRuntimeLifecyclePort`]. The lifecycle
/// and publication proxies returned for one activation must share one local
/// provisional admission reservation. Before any lifecycle method begins, the
/// last proxy destructor must release only that process-local reservation
/// synchronously and infallibly; it must perform no dispatch, native I/O, or
/// ownership settlement. The first lifecycle method atomically attaches the
/// reservation to the engine registry before native work. After attachment,
/// proxy destruction never removes the registry row or settles ownership.
pub trait ExtensionRuntimeHostLifecyclePort: ExtensionRuntimeLifecyclePort {}

/// Marker for a trusted cleanup-only engine ownership proxy.
///
/// Implementations must obey all passive-`Drop`, deadline, ownership, and
/// retained-memory rules of [`ExtensionRuntimeOwnershipPort`]. A newly bound
/// recovery proxy owns a provisional process-local admission reservation. If
/// no ownership method begins, destruction must synchronously and infallibly
/// release only that reservation. The first method attaches it to the engine
/// registry; destruction after attachment cannot settle or remove ownership.
pub trait ExtensionRuntimeHostOwnershipPort: ExtensionRuntimeOwnershipPort {}

/// Trusted engine publication and operation-authority proxy.
///
/// A successful publication consumes the exact operation authority into the
/// engine registry. A successful reclaim may return it only after native
/// absence is definite and the caller supplies the exact durable
/// `Release/NativeAbsentReleasePending` row. Every destructor is passive.
pub trait ExtensionRuntimeHostPublicationPort: Send {
    /// Stable upper bound for memory retained exclusively by this proxy.
    ///
    /// The bound includes the concrete adapter allocation and all exclusive
    /// deterministic host allocations. It excludes the API trait-object
    /// pointer, engine-owned native handles, the operation-authority allocation
    /// separately charged by this API, and the separately hard-counted
    /// logical/native reservation. It must remain valid before publication,
    /// while published, and through reclaim. The query is side-effect-free and
    /// must report `usize::MAX` when no trustworthy bound is available.
    fn retained_bytes(&self) -> usize;

    /// Publishes operation authority into an exact already-activated registry row.
    ///
    /// `Err` must return the exact supplied authority and prove it never entered
    /// the registry. The wrapper validates the full runtime fingerprint and
    /// quarantines any substituted capability as an engine invariant failure.
    fn publish_operation_authority(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        owned_entry: &ExtensionNativeOwnershipEntry,
        evidence: ExtensionRuntimeOwnershipEvidence,
        authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<(), ExtensionRuntimeHostPublicationPortRefusal>;

    /// Reclaims the exact operation authority after proven native absence.
    ///
    /// `Err` must retain the exact authority inside this port without changing
    /// registry ownership. `Ok` must return that exact authority only after the
    /// addressed generation is absent and removed; the wrapper validates its
    /// full runtime fingerprint before exposing it to repository settlement.
    fn reclaim_operation_authority(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        release_entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimeHostBindError>;

    /// Mints a transient active-tab witness inside the published registry.
    fn mint_active_tab_grant_witness(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        invocation: ExtensionUserInvocationKind,
    ) -> Result<ExtensionActiveTabGrantWitness, ExtensionOperationAuthorityDenial>;

    /// Mints a document-operation witness inside the published registry.
    fn mint_document_authority_witness(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionDocumentPurpose,
    ) -> Result<ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial>;
}

/// Lossless low-level refusal to publish operation authority.
#[must_use = "publication refusal retains the exact operation authority"]
pub struct ExtensionRuntimeHostPublicationPortRefusal {
    reason: ExtensionRuntimeHostBindError,
    authority: Box<ExtensionRuntimeOperationAuthority>,
}

impl ExtensionRuntimeHostPublicationPortRefusal {
    /// Constructs a lossless refusal for a trusted engine adapter.
    ///
    /// Calling this is the adapter's assertion that the returned authority is
    /// the exact input and was not retained or published anywhere else.
    pub fn new(
        reason: ExtensionRuntimeHostBindError,
        authority: ExtensionRuntimeOperationAuthority,
    ) -> Self {
        Self {
            reason,
            authority: Box::new(authority),
        }
    }

    /// Stable refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimeHostBindError {
        self.reason
    }

    /// Recovers the exact unconsumed operation authority.
    pub fn into_authority(self) -> ExtensionRuntimeOperationAuthority {
        *self.authority
    }
}

impl fmt::Debug for ExtensionRuntimeHostPublicationPortRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeHostPublicationPortRefusal")
            .field("reason", &self.reason)
            .field("authority", &"[redacted]")
            .finish()
    }
}

/// Passive proxy set reserved by the engine for one fresh activation.
#[must_use = "all activation proxies must remain joined to the exact binding"]
pub struct ExtensionRuntimeHostActivationPorts {
    generation: ExtensionRuntimeHostRegistryGeneration,
    lifecycle: Box<dyn ExtensionRuntimeHostLifecyclePort>,
    publication: Box<dyn ExtensionRuntimeHostPublicationPort>,
}

impl ExtensionRuntimeHostActivationPorts {
    /// Joins one exact registry generation to its lifecycle and publication proxies.
    pub fn new(
        generation: ExtensionRuntimeHostRegistryGeneration,
        lifecycle: Box<dyn ExtensionRuntimeHostLifecyclePort>,
        publication: Box<dyn ExtensionRuntimeHostPublicationPort>,
    ) -> Self {
        Self {
            generation,
            lifecycle,
            publication,
        }
    }
}

impl fmt::Debug for ExtensionRuntimeHostActivationPorts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeHostActivationPorts")
            .field("generation", &self.generation)
            .field("proxies", &"[redacted]")
            .finish()
    }
}

/// Unique trusted engine factory implementation.
///
/// Binding methods must not dispatch UI work or call native APIs. They may
/// reserve bounded logical slots and construct passive proxies.
pub trait ExtensionRuntimeHostFactoryPort: Send {
    /// Binds one fresh provisional activation reservation and its exact proxy pair.
    ///
    /// Both proxies must share the reservation lifecycle specified by
    /// [`ExtensionRuntimeHostLifecyclePort`]. Returning `Err` reserves nothing.
    fn bind_activation(
        &mut self,
        context: &ExtensionRuntimeHostActivationContext<'_>,
    ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError>;

    /// Binds one cleanup-only provisional possible-owner reservation.
    ///
    /// The returned proxy must obey the pre-attachment release rule specified
    /// by [`ExtensionRuntimeHostOwnershipPort`]. Returning `Err` reserves nothing.
    fn bind_recovery(
        &mut self,
        context: ExtensionRuntimeHostRecoveryContext,
    ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError>;
}

/// Unique move-only engine host factory held by the serialized service.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeHostFactory;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeHostFactory>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeHostFactory;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeHostFactory>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeHostFactory;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeHostFactory>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeHostFactory;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeHostFactory>();
/// ```
#[must_use = "the unique native extension host factory must remain serialized"]
pub struct ExtensionRuntimeHostFactory {
    port: Box<dyn ExtensionRuntimeHostFactoryPort>,
}

impl ExtensionRuntimeHostFactory {
    /// Creates a unique factory around a trusted engine implementation.
    pub fn from_trusted_port(port: Box<dyn ExtensionRuntimeHostFactoryPort>) -> Self {
        Self { port }
    }

    /// Binds a fresh activation without invoking a native lifecycle method.
    pub fn bind_activation(
        &mut self,
        binding: ExtensionRuntimeHostActivationBinding,
    ) -> Result<ExtensionRuntimeHostActivation, ExtensionRuntimeHostActivationBindRefusal> {
        let context = ExtensionRuntimeHostActivationContext {
            owner: ExtensionRuntimeOwnerAddress::from_entry(&binding.entry),
            fingerprint: binding.authority.fingerprint(),
            expectation: binding.expectation,
            target: binding.access.target(),
        };
        let ExtensionRuntimeHostActivationPorts {
            generation,
            lifecycle,
            publication,
        } = match self.port.bind_activation(&context) {
            Ok(parts) => parts,
            Err(reason) => {
                return Err(ExtensionRuntimeHostActivationBindRefusal {
                    reason,
                    binding: Box::new(binding),
                });
            }
        };
        let host_retained = lifecycle
            .retained_bytes()
            .checked_add(publication.retained_bytes())
            .and_then(|value| value.checked_add(binding.retained_bytes()))
            .and_then(|value| value.checked_add(size_of::<ExtensionRuntimeHostActivation>()));
        let reason = match host_retained {
            None => Some(ExtensionRuntimeHostBindError::RetainedBytesOverflow),
            Some(value) if value > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES => {
                Some(ExtensionRuntimeHostBindError::RetainedBytesExceeded)
            }
            Some(_) => None,
        };
        if let Some(reason) = reason {
            // No lifecycle method ran. The trusted proxy pair shares a
            // provisional local reservation, released synchronously by the
            // last destructor under the host marker-trait contract.
            drop(lifecycle);
            drop(publication);
            return Err(ExtensionRuntimeHostActivationBindRefusal {
                reason,
                binding: Box::new(binding),
            });
        }

        let ExtensionRuntimeHostActivationBinding {
            entry,
            access,
            authority,
            expectation,
        } = binding;
        let lifecycle: Box<dyn ExtensionRuntimeLifecyclePort> = lifecycle;
        let request = match ExtensionRuntimeActivationRequest::try_new(access, lifecycle) {
            Ok(request) => request,
            Err(refusal) => {
                let reason = match refusal.reason() {
                    ExtensionRuntimeActivationBuildError::RetainedBytesOverflow => {
                        ExtensionRuntimeHostBindError::RetainedBytesOverflow
                    }
                    ExtensionRuntimeActivationBuildError::RetainedBytesExceeded => {
                        ExtensionRuntimeHostBindError::RetainedBytesExceeded
                    }
                };
                let (access, lifecycle) = refusal.into_parts();
                // Construction is still pre-attachment, so dropping both
                // trusted proxies returns the provisional local reservation.
                drop(lifecycle);
                drop(publication);
                return Err(ExtensionRuntimeHostActivationBindRefusal {
                    reason,
                    binding: Box::new(ExtensionRuntimeHostActivationBinding {
                        entry,
                        access,
                        authority,
                        expectation,
                    }),
                });
            }
        };
        let pending = ExtensionRuntimePendingPublication {
            initial_entry: entry,
            authority,
            expectation,
            generation,
            publication,
        };
        let future_retained = request
            .retained_bytes()
            .checked_add(pending.max_future_control_retained_bytes());
        let reason = match future_retained {
            None => Some(ExtensionRuntimeHostBindError::RetainedBytesOverflow),
            Some(value) if value > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES => {
                Some(ExtensionRuntimeHostBindError::RetainedBytesExceeded)
            }
            Some(_) => None,
        };
        if let Some(reason) = reason {
            let (access, lifecycle) = request.cancel();
            let ExtensionRuntimePendingPublication {
                initial_entry: entry,
                authority,
                expectation,
                generation: _,
                publication,
            } = pending;
            // Neither proxy was invoked, so their shared provisional
            // reservation returns when the final proxy is destroyed.
            drop(lifecycle);
            drop(publication);
            return Err(ExtensionRuntimeHostActivationBindRefusal {
                reason,
                binding: Box::new(ExtensionRuntimeHostActivationBinding {
                    entry,
                    access,
                    authority,
                    expectation,
                }),
            });
        }
        Ok(ExtensionRuntimeHostActivation { request, pending })
    }

    /// Binds cleanup-only persisted uncertainty without native work.
    pub fn bind_recovery(
        &mut self,
        binding: ExtensionRuntimeHostRecoveryBinding,
    ) -> Result<ExtensionRuntimeRecoveryRequest, ExtensionRuntimeHostRecoveryBindRefusal> {
        let context = binding.context;
        let ownership = match self.port.bind_recovery(context) {
            Ok(ownership) => ownership,
            Err(reason) => {
                return Err(ExtensionRuntimeHostRecoveryBindRefusal {
                    reason,
                    binding: Box::new(ExtensionRuntimeHostRecoveryBinding { context }),
                });
            }
        };
        let ownership: Box<dyn ExtensionRuntimeOwnershipPort> = ownership;
        match ExtensionRuntimeRecoveryRequest::try_from_persisted_uncertainty(
            ownership,
            context.expectation,
        ) {
            Ok(request) => Ok(request),
            Err(refusal) => {
                let reason = match refusal.reason() {
                    ExtensionRuntimeRecoveryBuildError::RetainedBytesOverflow => {
                        ExtensionRuntimeHostBindError::RetainedBytesOverflow
                    }
                    ExtensionRuntimeRecoveryBuildError::RetainedBytesExceeded => {
                        ExtensionRuntimeHostBindError::RetainedBytesExceeded
                    }
                };
                // The rejected request never invoked an ownership method;
                // dropping its proxy returns the provisional local slot.
                drop(refusal);
                Err(ExtensionRuntimeHostRecoveryBindRefusal {
                    reason,
                    binding: Box::new(ExtensionRuntimeHostRecoveryBinding { context }),
                })
            }
        }
    }
}

impl fmt::Debug for ExtensionRuntimeHostFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExtensionRuntimeHostFactory([redacted])")
    }
}

/// Lossless refusal to bind fresh activation into the engine.
#[must_use = "the refusal retains complete activation authority"]
pub struct ExtensionRuntimeHostActivationBindRefusal {
    reason: ExtensionRuntimeHostBindError,
    binding: Box<ExtensionRuntimeHostActivationBinding>,
}

impl ExtensionRuntimeHostActivationBindRefusal {
    /// Stable refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimeHostBindError {
        self.reason
    }

    /// Recovers the complete unchanged activation binding.
    pub fn into_binding(self) -> ExtensionRuntimeHostActivationBinding {
        *self.binding
    }

    /// Cancels the refused, still-unattached activation and returns every
    /// exact input needed by the authenticated repository to recover its
    /// role-specific package lease.
    ///
    /// A bind refusal is produced only before any lifecycle or publication
    /// proxy is invoked. Therefore this extraction never claims that a native
    /// owner was removed or that the durable ownership row was settled; the
    /// caller must settle the returned row independently before releasing the
    /// recovered package pin.
    #[must_use = "all returned values retain runtime, package, and durable-row authority"]
    pub fn cancel_into_parts(
        self,
    ) -> (
        ExtensionNativeOwnershipEntry,
        ExtensionPackageAccess,
        ExtensionRuntimeOperationAuthority,
        ExtensionRuntimeNativeIdentityExpectation,
    ) {
        let ExtensionRuntimeHostActivationBinding {
            entry,
            access,
            authority,
            expectation,
        } = *self.binding;
        (entry, access, authority, expectation)
    }
}

impl fmt::Debug for ExtensionRuntimeHostActivationBindRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeHostActivationBindRefusal")
            .field("reason", &self.reason)
            .field("binding", &"[redacted]")
            .finish()
    }
}

/// Lossless refusal to bind cleanup-only persisted uncertainty.
#[must_use = "the refusal retains the cleanup-only binding"]
pub struct ExtensionRuntimeHostRecoveryBindRefusal {
    reason: ExtensionRuntimeHostBindError,
    binding: Box<ExtensionRuntimeHostRecoveryBinding>,
}

impl ExtensionRuntimeHostRecoveryBindRefusal {
    /// Stable refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimeHostBindError {
        self.reason
    }

    /// Recovers the complete cleanup-only binding.
    pub fn into_binding(self) -> ExtensionRuntimeHostRecoveryBinding {
        *self.binding
    }
}

impl fmt::Debug for ExtensionRuntimeHostRecoveryBindRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeHostRecoveryBindRefusal")
            .field("reason", &self.reason)
            .field("binding", &"[redacted]")
            .finish()
    }
}

/// Fresh lifecycle request paired with its unpublished operation authority.
#[must_use = "activation and publication authority must settle together"]
pub struct ExtensionRuntimeHostActivation {
    request: ExtensionRuntimeActivationRequest,
    pending: ExtensionRuntimePendingPublication,
}

impl ExtensionRuntimeHostActivation {
    /// Separates the lifecycle request from its exact pending-publication token.
    #[must_use = "both values are required to settle activation safely"]
    pub fn into_parts(
        self,
    ) -> (
        ExtensionRuntimeActivationRequest,
        ExtensionRuntimePendingPublication,
    ) {
        (self.request, self.pending)
    }

    /// Conservative retained-memory charge while both capabilities are queued.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.request
            .retained_bytes()
            .saturating_add(self.pending.retained_bytes())
    }
}

impl fmt::Debug for ExtensionRuntimeHostActivation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExtensionRuntimeHostActivation([redacted])")
    }
}

/// Unpublished operation authority for one exact activated owner attempt.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePendingPublication;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimePendingPublication>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePendingPublication;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimePendingPublication>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimePendingPublication;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimePendingPublication>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimePendingPublication;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimePendingPublication>();
/// ```
#[must_use = "operation authority must publish only after the exact Store CAS"]
pub struct ExtensionRuntimePendingPublication {
    initial_entry: ExtensionNativeOwnershipEntry,
    authority: ExtensionRuntimeOperationAuthority,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
    generation: ExtensionRuntimeHostRegistryGeneration,
    publication: Box<dyn ExtensionRuntimeHostPublicationPort>,
}

impl ExtensionRuntimePendingPublication {
    /// Exact engine registry generation reserved for this attempt.
    #[must_use]
    pub const fn registry_generation(&self) -> ExtensionRuntimeHostRegistryGeneration {
        self.generation
    }

    /// Exact initial durable owner address.
    #[must_use]
    pub fn owner_address(&self) -> ExtensionRuntimeOwnerAddress {
        ExtensionRuntimeOwnerAddress::from_entry(&self.initial_entry)
    }

    /// Conservative retained-memory charge while authority is unpublished.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(authority_exclusive_retained_bytes(&self.authority))
            .saturating_add(self.publication.retained_bytes())
    }

    fn max_future_control_retained_bytes(&self) -> usize {
        let authority_exclusive = authority_exclusive_retained_bytes(&self.authority);
        let authority_total = self.authority.retained_bytes();
        let publication = self.publication.retained_bytes();
        let pending = self.retained_bytes();
        let request = checked_retained_sum([
            size_of::<ExtensionRuntimePublicationRequest>(),
            authority_exclusive,
            publication,
        ]);
        let receipt = checked_retained_sum([
            size_of::<ExtensionRuntimePublicationReceipt>(),
            authority_total,
            publication,
        ]);
        let authorization_refusal = checked_retained_sum([
            size_of::<ExtensionRuntimePublicationAuthorizationRefusal>(),
            size_of::<ExtensionRuntimePublicationAuthorizationParts>()
                .saturating_sub(size_of::<ExtensionRuntimePendingPublication>()),
            pending,
        ]);
        [
            pending,
            checked_retained_sum([size_of::<ExtensionRuntimePendingRecoveryRefusal>(), pending]),
            authorization_refusal,
            request,
            checked_retained_sum([size_of::<ExtensionRuntimeRequestRecoveryRefusal>(), request]),
            checked_retained_sum([size_of::<ExtensionRuntimePublicationRefusal>(), request]),
            receipt,
            checked_retained_sum([
                size_of::<ExtensionRuntimePublicationReclaimRefusal>(),
                receipt,
            ]),
        ]
        .into_iter()
        .max()
        .unwrap_or(usize::MAX)
    }

    /// Authorizes publication from an exact Store-shaped owned row and
    /// authenticated activation evidence.
    ///
    /// The row is structural, cloneable data rather than freshness authority.
    /// The serialized service must read and CAS-revalidate the current Store
    /// row immediately before this call.
    pub fn authorize(
        self,
        owned_entry: ExtensionNativeOwnershipEntry,
        evidence: ExtensionRuntimeOwnershipEvidence,
    ) -> Result<ExtensionRuntimePublicationRequest, ExtensionRuntimePublicationAuthorizationRefusal>
    {
        if let Some(reason) = publication_authorization_error(
            &self.initial_entry,
            &owned_entry,
            self.expectation,
            evidence,
            self.authority.fingerprint(),
        ) {
            return Err(ExtensionRuntimePublicationAuthorizationRefusal {
                reason,
                parts: Box::new(ExtensionRuntimePublicationAuthorizationParts {
                    pending: self,
                    owned_entry,
                    evidence,
                }),
            });
        }
        Ok(ExtensionRuntimePublicationRequest {
            initial_entry: self.initial_entry,
            owned_entry,
            fingerprint: self.authority.fingerprint().clone(),
            authority_retained_bytes: self.authority.retained_bytes(),
            authority: self.authority,
            expectation: self.expectation,
            evidence,
            generation: self.generation,
            publication: self.publication,
        })
    }

    /// Recovers unpublished operation authority after an exact structural
    /// post-absence row has been revalidated against Store.
    ///
    /// The engine publication proxy is dropped passively. A caller cannot use
    /// this path while Store still records possible or definite ownership. The
    /// serialized service must first durably reach, then immediately
    /// CAS-revalidate, `Release/NativeAbsentReleasePending` for the same
    /// operation lineage. This borrowed row does not itself prove freshness.
    pub fn recover_after_absence(
        self,
        release_entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimePendingRecoveryRefusal> {
        if !valid_release_frontier(&self.initial_entry, release_entry, self.expectation) {
            return Err(ExtensionRuntimePendingRecoveryRefusal {
                reason: ExtensionRuntimePublicationAuthorizationError::ReleaseFrontierMismatch,
                pending: Box::new(self),
            });
        }
        let Self {
            initial_entry: _,
            authority,
            expectation: _,
            generation: _,
            publication,
        } = self;
        drop(publication);
        Ok(authority)
    }
}

impl fmt::Debug for ExtensionRuntimePendingPublication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimePendingPublication")
            .field("generation", &self.generation)
            .field("authority", &"[redacted]")
            .finish()
    }
}

/// Lossless refusal to recover authority before engine publication.
#[must_use = "the refusal retains the exact pending publication authority"]
pub struct ExtensionRuntimePendingRecoveryRefusal {
    reason: ExtensionRuntimePublicationAuthorizationError,
    pending: Box<ExtensionRuntimePendingPublication>,
}

impl ExtensionRuntimePendingRecoveryRefusal {
    /// Stable refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimePublicationAuthorizationError {
        self.reason
    }

    /// Recovers the exact pending publication token.
    pub fn into_pending(self) -> ExtensionRuntimePendingPublication {
        *self.pending
    }

    /// Stable upper bound retained by this recoverable refusal.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        checked_retained_sum([size_of::<Self>(), self.pending.retained_bytes()])
    }
}

impl fmt::Debug for ExtensionRuntimePendingRecoveryRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimePendingRecoveryRefusal")
            .field("reason", &self.reason)
            .field("pending", &"[redacted]")
            .finish()
    }
}

/// Stable failure to authorize publication against the fresh Store row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimePublicationAuthorizationError {
    /// The Store row is not an acquired, definitely-owned row.
    OwnershipStateMismatch,
    /// The Store row does not continue the exact original operation lineage.
    OwnershipLineageMismatch,
    /// Native evidence differs from the authenticated package expectation.
    NativeEvidenceMismatch,
    /// Persisted native identity differs from authenticated native evidence.
    NativeIdentityMismatch,
    /// Operation authority no longer matches the durable owner row.
    RuntimeFingerprintMismatch,
    /// A release row is not the exact post-absence cleanup frontier.
    ReleaseFrontierMismatch,
}

impl fmt::Display for ExtensionRuntimePublicationAuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "extension runtime publication was not authorized: {self:?}"
        )
    }
}

impl Error for ExtensionRuntimePublicationAuthorizationError {}

/// Lossless publication-authorization refusal.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationAuthorizationRefusal;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimePublicationAuthorizationRefusal>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationAuthorizationRefusal;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimePublicationAuthorizationRefusal>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationAuthorizationRefusal;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimePublicationAuthorizationRefusal>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationAuthorizationRefusal;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimePublicationAuthorizationRefusal>();
/// ```
#[must_use = "the refusal retains pending authority and the supplied Store row"]
pub struct ExtensionRuntimePublicationAuthorizationRefusal {
    reason: ExtensionRuntimePublicationAuthorizationError,
    parts: Box<ExtensionRuntimePublicationAuthorizationParts>,
}

struct ExtensionRuntimePublicationAuthorizationParts {
    pending: ExtensionRuntimePendingPublication,
    owned_entry: ExtensionNativeOwnershipEntry,
    evidence: ExtensionRuntimeOwnershipEvidence,
}

impl ExtensionRuntimePublicationAuthorizationRefusal {
    /// Stable refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimePublicationAuthorizationError {
        self.reason
    }

    /// Recovers every exact input unchanged.
    pub fn into_parts(
        self,
    ) -> (
        ExtensionRuntimePendingPublication,
        ExtensionNativeOwnershipEntry,
        ExtensionRuntimeOwnershipEvidence,
    ) {
        let ExtensionRuntimePublicationAuthorizationParts {
            pending,
            owned_entry,
            evidence,
        } = *self.parts;
        (pending, owned_entry, evidence)
    }

    /// Stable upper bound retained by this recoverable refusal.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        checked_retained_sum([
            size_of::<Self>(),
            size_of::<ExtensionRuntimePublicationAuthorizationParts>()
                .saturating_sub(size_of::<ExtensionRuntimePendingPublication>()),
            self.parts.pending.retained_bytes(),
        ])
    }
}

impl fmt::Debug for ExtensionRuntimePublicationAuthorizationRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimePublicationAuthorizationRefusal")
            .field("reason", &self.reason)
            .field("authority", &"[redacted]")
            .finish()
    }
}

fn publication_authorization_error(
    initial: &ExtensionNativeOwnershipEntry,
    owned: &ExtensionNativeOwnershipEntry,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
    evidence: ExtensionRuntimeOwnershipEvidence,
    fingerprint: &ExtensionRuntimeFingerprint,
) -> Option<ExtensionRuntimePublicationAuthorizationError> {
    use ExtensionRuntimePublicationAuthorizationError as Error;

    if owned.intent() != ExtensionNativeOwnershipIntent::Acquire
        || owned.phase() != ExtensionNativeOwnershipPhase::NativeOwned
    {
        return Some(Error::OwnershipStateMismatch);
    }
    if !same_owner_lineage(initial, owned) || owned.revision() <= initial.revision() {
        return Some(Error::OwnershipLineageMismatch);
    }
    if !entry_matches_fingerprint(owned, fingerprint) {
        return Some(Error::RuntimeFingerprintMismatch);
    }
    if !expectation.accepts_evidence(evidence) {
        return Some(Error::NativeEvidenceMismatch);
    }
    let persisted_matches = match evidence {
        ExtensionRuntimeOwnershipEvidence::MacosWebExtension(actual)
        | ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(actual) => owned
            .native_identity()
            .is_some_and(|identity| identity.bytes() == actual.encoded_bytes()),
        ExtensionRuntimeOwnershipEvidence::Compatibility => owned.native_identity().is_none(),
    };
    (!persisted_matches).then_some(Error::NativeIdentityMismatch)
}

fn same_owner_lineage(
    initial: &ExtensionNativeOwnershipEntry,
    current: &ExtensionNativeOwnershipEntry,
) -> bool {
    initial.key() == current.key()
        && initial.operation() == current.operation()
        && initial.native_incarnation() == current.native_incarnation()
        && initial.package() == current.package()
        && initial.catalog_set_digest() == current.catalog_set_digest()
        && initial.catalog_role() == current.catalog_role()
        && initial.store_catalog_revision() == current.store_catalog_revision()
        && initial.store_install_revision() == current.store_install_revision()
        && initial.store_grant_revision() == current.store_grant_revision()
        && initial.grant_digest() == current.grant_digest()
        && initial.runtime_backend() == current.runtime_backend()
}

/// Exact post-Store publication request.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationRequest;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimePublicationRequest>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationRequest;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimePublicationRequest>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationRequest;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimePublicationRequest>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationRequest;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimePublicationRequest>();
/// ```
#[must_use = "authorized publication must be attempted or retained for cleanup"]
pub struct ExtensionRuntimePublicationRequest {
    initial_entry: ExtensionNativeOwnershipEntry,
    owned_entry: ExtensionNativeOwnershipEntry,
    fingerprint: ExtensionRuntimeFingerprint,
    authority_retained_bytes: usize,
    authority: ExtensionRuntimeOperationAuthority,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
    evidence: ExtensionRuntimeOwnershipEvidence,
    generation: ExtensionRuntimeHostRegistryGeneration,
    publication: Box<dyn ExtensionRuntimeHostPublicationPort>,
}

impl ExtensionRuntimePublicationRequest {
    /// Publishes operation authority into the exact engine registry generation.
    pub fn publish(
        self,
    ) -> Result<ExtensionRuntimePublicationReceipt, ExtensionRuntimePublicationRefusal> {
        let Self {
            initial_entry,
            owned_entry,
            fingerprint,
            authority_retained_bytes,
            authority,
            expectation,
            evidence,
            generation,
            mut publication,
        } = self;
        let owner = ExtensionRuntimeOwnerAddress::from_entry(&initial_entry);
        match publication.publish_operation_authority(
            owner,
            generation,
            &owned_entry,
            evidence,
            authority,
        ) {
            Ok(()) => Ok(ExtensionRuntimePublicationReceipt {
                initial_entry,
                owned_entry,
                fingerprint,
                authority_retained_bytes,
                expectation,
                evidence,
                generation,
                publication,
            }),
            Err(refusal) => {
                let reason = refusal.reason();
                let returned_authority = refusal.into_authority();
                if returned_authority.fingerprint() == &fingerprint
                    && returned_authority.matches_native_ownership_lineage(&initial_entry)
                {
                    Err(ExtensionRuntimePublicationRefusal {
                        reason,
                        state: ExtensionRuntimePublicationRefusalState::Recoverable(Box::new(
                            Self {
                                initial_entry,
                                owned_entry,
                                fingerprint,
                                authority_retained_bytes,
                                authority: returned_authority,
                                expectation,
                                evidence,
                                generation,
                                publication,
                            },
                        )),
                    })
                } else {
                    Err(ExtensionRuntimePublicationRefusal {
                        reason: ExtensionRuntimeHostBindError::InternalInvariant,
                        state: ExtensionRuntimePublicationRefusalState::Quarantined {
                            _quarantine: Box::new(ExtensionRuntimePublicationQuarantine {
                                _initial_entry: initial_entry,
                                _owned_entry: owned_entry,
                                _fingerprint: fingerprint,
                                _authority_retained_bytes: authority_retained_bytes,
                                _returned_authority: returned_authority,
                                _expectation: expectation,
                                _evidence: evidence,
                                _generation: generation,
                                _publication: publication,
                            }),
                        },
                    })
                }
            }
        }
    }

    /// Recovers operation authority after an exact post-absence row was
    /// durably committed and immediately revalidated, before the authority
    /// entered the engine registry. The borrowed row is structural rather than
    /// Store-freshness authority.
    pub fn recover_after_absence(
        self,
        release_entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimeRequestRecoveryRefusal> {
        if !valid_release_frontier(&self.owned_entry, release_entry, self.expectation) {
            return Err(ExtensionRuntimeRequestRecoveryRefusal {
                reason: ExtensionRuntimePublicationAuthorizationError::ReleaseFrontierMismatch,
                request: Box::new(self),
            });
        }
        let Self {
            initial_entry: _,
            owned_entry: _,
            fingerprint: _,
            authority_retained_bytes: _,
            authority,
            expectation: _,
            evidence: _,
            generation: _,
            publication,
        } = self;
        drop(publication);
        Ok(authority)
    }

    /// Stable upper bound while this authorized request remains unpublished.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        debug_assert_eq!(
            self.authority_retained_bytes,
            self.authority.retained_bytes()
        );
        checked_retained_sum([
            size_of::<Self>(),
            authority_exclusive_retained_bytes(&self.authority),
            self.publication.retained_bytes(),
        ])
    }
}

impl fmt::Debug for ExtensionRuntimePublicationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimePublicationRequest")
            .field("generation", &self.generation)
            .field("authority", &"[redacted]")
            .finish()
    }
}

/// Lossless refusal to recover authority from an unpublished authorized request.
#[must_use = "the refusal retains the exact authorized publication request"]
pub struct ExtensionRuntimeRequestRecoveryRefusal {
    reason: ExtensionRuntimePublicationAuthorizationError,
    request: Box<ExtensionRuntimePublicationRequest>,
}

impl ExtensionRuntimeRequestRecoveryRefusal {
    /// Stable refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimePublicationAuthorizationError {
        self.reason
    }

    /// Recovers the exact authorized request.
    pub fn into_request(self) -> ExtensionRuntimePublicationRequest {
        *self.request
    }

    /// Stable upper bound retained by this recoverable refusal.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        checked_retained_sum([size_of::<Self>(), self.request.retained_bytes()])
    }
}

impl fmt::Debug for ExtensionRuntimeRequestRecoveryRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeRequestRecoveryRefusal")
            .field("reason", &self.reason)
            .field("request", &"[redacted]")
            .finish()
    }
}

/// Lossless engine refusal to publish an authorized request.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationRefusal;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimePublicationRefusal>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationRefusal;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimePublicationRefusal>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationRefusal;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimePublicationRefusal>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationRefusal;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimePublicationRefusal>();
/// ```
#[must_use = "the refusal retains the exact publication request"]
pub struct ExtensionRuntimePublicationRefusal {
    reason: ExtensionRuntimeHostBindError,
    state: ExtensionRuntimePublicationRefusalState,
}

enum ExtensionRuntimePublicationRefusalState {
    Recoverable(Box<ExtensionRuntimePublicationRequest>),
    Quarantined {
        _quarantine: Box<ExtensionRuntimePublicationQuarantine>,
    },
}

struct ExtensionRuntimePublicationQuarantine {
    _initial_entry: ExtensionNativeOwnershipEntry,
    _owned_entry: ExtensionNativeOwnershipEntry,
    _fingerprint: ExtensionRuntimeFingerprint,
    _authority_retained_bytes: usize,
    _returned_authority: ExtensionRuntimeOperationAuthority,
    _expectation: ExtensionRuntimeNativeIdentityExpectation,
    _evidence: ExtensionRuntimeOwnershipEvidence,
    _generation: ExtensionRuntimeHostRegistryGeneration,
    _publication: Box<dyn ExtensionRuntimeHostPublicationPort>,
}

impl ExtensionRuntimePublicationRefusal {
    /// Stable engine refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimeHostBindError {
        self.reason
    }

    /// Recovers the exact authorized request after an ordinary host refusal.
    ///
    /// A violated authority-return contract remains quarantined and returns
    /// this refusal unchanged.
    pub fn try_into_request(self) -> Result<ExtensionRuntimePublicationRequest, Self> {
        match self.state {
            ExtensionRuntimePublicationRefusalState::Recoverable(request) => Ok(*request),
            state => Err(Self {
                reason: self.reason,
                state,
            }),
        }
    }

    /// Whether this refusal represents a trusted-engine invariant failure.
    ///
    /// A quarantined refusal is not retryable. The serialized service must
    /// retain at most one while entering its fatal-invariant path.
    #[must_use]
    pub const fn requires_fail_stop(&self) -> bool {
        matches!(
            &self.state,
            ExtensionRuntimePublicationRefusalState::Quarantined { .. }
        )
    }

    /// Stable upper bound retained by this refusal or fail-stop quarantine.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        let retained = match &self.state {
            ExtensionRuntimePublicationRefusalState::Recoverable(request) => {
                checked_retained_sum([size_of::<Self>(), request.retained_bytes()])
            }
            ExtensionRuntimePublicationRefusalState::Quarantined { _quarantine } => {
                checked_retained_sum([
                    size_of::<Self>(),
                    size_of::<ExtensionRuntimePublicationQuarantine>(),
                    _quarantine._authority_retained_bytes,
                    authority_exclusive_retained_bytes(&_quarantine._returned_authority),
                    _quarantine._publication.retained_bytes(),
                ])
            }
        };
        debug_assert!(
            !self.requires_fail_stop()
                || retained <= MAX_EXTENSION_RUNTIME_HOST_QUARANTINE_RETAINED_BYTES
        );
        retained
    }
}

impl fmt::Debug for ExtensionRuntimePublicationRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimePublicationRefusal")
            .field("reason", &self.reason)
            .field("state", &"[redacted]")
            .finish()
    }
}

/// Move-only control receipt for one published native runtime.
///
/// The receipt provides trusted operation ingress without returning operation
/// authority to the service. Only exact post-absence release settlement may
/// reclaim that authority for repository pin recombination.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationReceipt;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimePublicationReceipt>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationReceipt;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimePublicationReceipt>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationReceipt;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimePublicationReceipt>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationReceipt;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimePublicationReceipt>();
/// ```
#[must_use = "published authority must remain controlled until exact post-absence reclaim"]
pub struct ExtensionRuntimePublicationReceipt {
    initial_entry: ExtensionNativeOwnershipEntry,
    owned_entry: ExtensionNativeOwnershipEntry,
    fingerprint: ExtensionRuntimeFingerprint,
    authority_retained_bytes: usize,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
    evidence: ExtensionRuntimeOwnershipEvidence,
    generation: ExtensionRuntimeHostRegistryGeneration,
    publication: Box<dyn ExtensionRuntimeHostPublicationPort>,
}

impl ExtensionRuntimePublicationReceipt {
    /// Exact engine registry generation controlling this published owner.
    #[must_use]
    pub const fn registry_generation(&self) -> ExtensionRuntimeHostRegistryGeneration {
        self.generation
    }

    /// Exact authenticated ownership evidence published to the engine.
    #[must_use]
    pub const fn ownership_evidence(&self) -> ExtensionRuntimeOwnershipEvidence {
        self.evidence
    }

    /// Mints one active-tab witness inside the published registry.
    pub fn mint_active_tab_grant_witness(
        &mut self,
        runtime: &ExtensionRuntimeFingerprint,
        invocation: ExtensionUserInvocationKind,
    ) -> Result<ExtensionActiveTabGrantWitness, ExtensionOperationAuthorityDenial> {
        if runtime != &self.fingerprint {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        let witness = self.publication.mint_active_tab_grant_witness(
            ExtensionRuntimeOwnerAddress::from_entry(&self.initial_entry),
            self.generation,
            runtime,
            invocation,
        )?;
        if !witness.matches(runtime, invocation) {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        Ok(witness)
    }

    /// Mints one document-operation witness inside the published registry.
    pub fn mint_document_authority_witness(
        &mut self,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionDocumentPurpose,
    ) -> Result<ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial> {
        if runtime != &self.fingerprint {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        let witness = self.publication.mint_document_authority_witness(
            ExtensionRuntimeOwnerAddress::from_entry(&self.initial_entry),
            self.generation,
            runtime,
            purpose,
        )?;
        if !witness.matches(runtime, purpose) {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        Ok(witness)
    }

    /// Reclaims operation authority only at an exact, immediately revalidated
    /// post-absence Store frontier. The borrowed row is structural and the
    /// serialized service remains responsible for current-row/CAS freshness.
    pub fn reclaim_after_absence(
        mut self,
        release_entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimePublicationReclaimRefusal> {
        if !valid_release_frontier(&self.owned_entry, release_entry, self.expectation) {
            return Err(ExtensionRuntimePublicationReclaimRefusal {
                reason: ExtensionRuntimePublicationReclaimError::Authorization(
                    ExtensionRuntimePublicationAuthorizationError::ReleaseFrontierMismatch,
                ),
                state: ExtensionRuntimePublicationReclaimRefusalState::Recoverable(Box::new(self)),
            });
        }
        match self.publication.reclaim_operation_authority(
            ExtensionRuntimeOwnerAddress::from_entry(&self.initial_entry),
            self.generation,
            release_entry,
        ) {
            Ok(authority)
                if authority.fingerprint() == &self.fingerprint
                    && authority.matches_native_ownership_lineage(&self.initial_entry) =>
            {
                Ok(authority)
            }
            Ok(returned_authority) => Err(ExtensionRuntimePublicationReclaimRefusal {
                reason: ExtensionRuntimePublicationReclaimError::Host(
                    ExtensionRuntimeHostBindError::InternalInvariant,
                ),
                state: ExtensionRuntimePublicationReclaimRefusalState::Quarantined {
                    _quarantine: Box::new(ExtensionRuntimePublicationReclaimQuarantine {
                        _receipt: self,
                        _returned_authority: returned_authority,
                    }),
                },
            }),
            Err(reason) => Err(ExtensionRuntimePublicationReclaimRefusal {
                reason: ExtensionRuntimePublicationReclaimError::Host(reason),
                state: ExtensionRuntimePublicationReclaimRefusalState::Recoverable(Box::new(self)),
            }),
        }
    }

    /// Conservative retained-memory charge for the service-side control receipt.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        checked_retained_sum([
            size_of::<Self>(),
            self.authority_retained_bytes,
            self.publication.retained_bytes(),
        ])
    }
}

/// Closed reason post-absence authority reclaim did not settle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimePublicationReclaimError {
    /// The supplied structural row did not match the exact release frontier.
    Authorization(ExtensionRuntimePublicationAuthorizationError),
    /// The engine could not return authority from the exact registry row.
    Host(ExtensionRuntimeHostBindError),
}

impl fmt::Display for ExtensionRuntimePublicationReclaimError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "extension runtime authority reclaim failed: {self:?}"
        )
    }
}

impl Error for ExtensionRuntimePublicationReclaimError {}

/// Lossless refusal to reclaim published operation authority.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationReclaimRefusal;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimePublicationReclaimRefusal>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationReclaimRefusal;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimePublicationReclaimRefusal>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationReclaimRefusal;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimePublicationReclaimRefusal>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimePublicationReclaimRefusal;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimePublicationReclaimRefusal>();
/// ```
#[must_use = "the refusal retains control of the published registry authority"]
pub struct ExtensionRuntimePublicationReclaimRefusal {
    reason: ExtensionRuntimePublicationReclaimError,
    state: ExtensionRuntimePublicationReclaimRefusalState,
}

enum ExtensionRuntimePublicationReclaimRefusalState {
    Recoverable(Box<ExtensionRuntimePublicationReceipt>),
    Quarantined {
        _quarantine: Box<ExtensionRuntimePublicationReclaimQuarantine>,
    },
}

struct ExtensionRuntimePublicationReclaimQuarantine {
    _receipt: ExtensionRuntimePublicationReceipt,
    _returned_authority: ExtensionRuntimeOperationAuthority,
}

impl ExtensionRuntimePublicationReclaimRefusal {
    /// Stable reclaim refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionRuntimePublicationReclaimError {
        self.reason
    }

    /// Recovers the exact published control receipt after an ordinary refusal.
    ///
    /// A violated authority-return contract remains quarantined and returns
    /// this refusal unchanged.
    pub fn try_into_receipt(self) -> Result<ExtensionRuntimePublicationReceipt, Self> {
        match self.state {
            ExtensionRuntimePublicationReclaimRefusalState::Recoverable(receipt) => Ok(*receipt),
            state => Err(Self {
                reason: self.reason,
                state,
            }),
        }
    }

    /// Whether this refusal represents a trusted-engine invariant failure.
    ///
    /// A quarantined refusal is not retryable. The serialized service must
    /// retain at most one while entering its fatal-invariant path.
    #[must_use]
    pub const fn requires_fail_stop(&self) -> bool {
        matches!(
            &self.state,
            ExtensionRuntimePublicationReclaimRefusalState::Quarantined { .. }
        )
    }

    /// Stable upper bound retained by this refusal or fail-stop quarantine.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        let retained = match &self.state {
            ExtensionRuntimePublicationReclaimRefusalState::Recoverable(receipt) => {
                checked_retained_sum([size_of::<Self>(), receipt.retained_bytes()])
            }
            ExtensionRuntimePublicationReclaimRefusalState::Quarantined { _quarantine } => {
                checked_retained_sum([
                    size_of::<Self>(),
                    size_of::<ExtensionRuntimePublicationReclaimQuarantine>()
                        .saturating_sub(size_of::<ExtensionRuntimePublicationReceipt>())
                        .saturating_sub(size_of::<ExtensionRuntimeOperationAuthority>()),
                    _quarantine._receipt.retained_bytes(),
                    _quarantine._returned_authority.retained_bytes(),
                ])
            }
        };
        debug_assert!(
            !self.requires_fail_stop()
                || retained <= MAX_EXTENSION_RUNTIME_HOST_QUARANTINE_RETAINED_BYTES
        );
        retained
    }
}

impl fmt::Debug for ExtensionRuntimePublicationReclaimRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimePublicationReclaimRefusal")
            .field("reason", &self.reason)
            .field("state", &"[redacted]")
            .finish()
    }
}

impl fmt::Debug for ExtensionRuntimePublicationReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimePublicationReceipt")
            .field("generation", &self.generation)
            .field("evidence", &self.evidence)
            .field("expectation", &self.expectation)
            .field("authority", &"[engine-owned]")
            .finish()
    }
}

fn valid_release_frontier(
    owned: &ExtensionNativeOwnershipEntry,
    release: &ExtensionNativeOwnershipEntry,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
) -> bool {
    same_owner_lineage(owned, release)
        && release.intent() == ExtensionNativeOwnershipIntent::Release
        && release.phase() == ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
        && release.revision() > owned.revision()
        // A native activation may observe and durably attach its deterministic
        // owner ID after the initial `NativeMayOwn` row but before an
        // unpublished owner is retired. Identity is immutable once present,
        // so cleanup may add it but can never replace or erase it.
        && release_identity_continues(owned, release, expectation)
}

fn release_identity_continues(
    owned: &ExtensionNativeOwnershipEntry,
    release: &ExtensionNativeOwnershipEntry,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
) -> bool {
    if let Some(identity) = owned.native_identity() {
        return release.native_identity() == Some(identity);
    }
    match expectation {
        ExtensionRuntimeNativeIdentityExpectation::Compatibility => {
            release.native_identity().is_none()
        }
        ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(expected)
        | ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(expected) => release
            .native_identity()
            .is_none_or(|identity| identity.bytes() == expected.encoded_bytes()),
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
