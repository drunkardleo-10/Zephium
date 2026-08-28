//! Exact, backend-authenticated native ownership evidence.

use std::fmt;
use std::num::NonZeroU64;

use zephium_core::extensions::{
    ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipEntryCas, ExtensionRuntimeBackendTarget,
};

use crate::{ExtensionRuntimeHostRegistryGeneration, ExtensionRuntimeTarget};

/// Exact byte length of a canonical native extension-owner identifier.
pub const EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES: usize = 32;

/// A canonical, bounded, non-authorizing identifier for one platform-native
/// extension owner.
///
/// Construction proves only the closed lowercase `a` through `p` shape. This
/// public structural value is not evidence that an owner exists, and callers
/// must not treat a caller-created value as authority. It becomes meaningful
/// only when a service-selected trusted lifecycle port returns it and the
/// service joins it to the exact backend, native incarnation, and durable
/// ownership row. It is deliberately distinct from extension install and
/// package identity. Adapters must never derive it from JavaScript, a native
/// object address, or untrusted package data.
///
/// The value retains no string allocation and its debug representation never
/// reveals the identifier.
///
/// Persistence must use the explicit canonical byte boundary; implicit
/// serialization is intentionally unavailable:
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeNativeOwnerId;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeNativeOwnerId>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeNativeOwnerId;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeNativeOwnerId>();
/// ```
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionRuntimeNativeOwnerId([u8; EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES]);

impl ExtensionRuntimeNativeOwnerId {
    /// Parses one exact canonical identifier without retaining the source
    /// string.
    pub fn parse_exact(value: &str) -> Result<Self, ExtensionRuntimeNativeOwnerIdError> {
        let bytes: [u8; EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES] = value
            .as_bytes()
            .try_into()
            .map_err(|_| ExtensionRuntimeNativeOwnerIdError::InvalidLength)?;
        Self::from_encoded_bytes(bytes)
    }

    /// Reconstructs an identifier from its exact bounded representation.
    pub fn from_encoded_bytes(
        bytes: [u8; EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES],
    ) -> Result<Self, ExtensionRuntimeNativeOwnerIdError> {
        if !bytes.iter().all(|byte| matches!(byte, b'a'..=b'p')) {
            return Err(ExtensionRuntimeNativeOwnerIdError::NonCanonical);
        }
        Ok(Self(bytes))
    }

    /// Returns the exact canonical bytes for a trusted persistence or native
    /// adapter boundary.
    #[must_use]
    pub const fn encoded_bytes(self) -> [u8; EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES] {
        self.0
    }
}

impl fmt::Debug for ExtensionRuntimeNativeOwnerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExtensionRuntimeNativeOwnerId([redacted])")
    }
}

/// Structural refusal to construct a native owner identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeNativeOwnerIdError {
    /// The encoded identifier is not exactly the required byte length.
    InvalidLength,
    /// At least one byte is outside the exact lowercase `a` through `p`
    /// alphabet.
    NonCanonical,
}

impl fmt::Display for ExtensionRuntimeNativeOwnerIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLength => "extension runtime native owner identifier has invalid length",
            Self::NonCanonical => {
                "extension runtime native owner identifier is not canonically encoded"
            }
        })
    }
}

impl std::error::Error for ExtensionRuntimeNativeOwnerIdError {}

/// Closed class of a trusted backend's definite-native-absence proof.
///
/// The class is descriptive rather than authorizing. A value becomes usable
/// only after the runtime API joins the complete evidence to the exact host
/// reservation which issued it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeAbsenceProofKind {
    /// The engine proved that the fresh activation attempt never entered its
    /// ownership-changing native boundary. This proof never establishes
    /// absence for retirement or crash recovery of a possible prior owner.
    ActivationNeverEntered,
    /// macOS proved all four grant dictionaries empty, private/all-host flags
    /// false, captured permission and pattern statuses non-granted,
    /// `isLoaded == false`, and exact controller-context absence.
    MacosZeroGrantsAndUnloaded,
    /// macOS re-opened the deterministic persistent profile controller,
    /// pointer-attested its controller/store identity, and observed both the
    /// complete context and extension inventories empty. With no context in
    /// the namespace, no per-context grant surface or native runtime owner
    /// exists to clear.
    MacosControllerNamespaceAbsent,
    /// Windows enumerated the complete bounded WebView2 extension snapshot for
    /// the exact attested profile after a completed removal or during crash
    /// recovery and observed the expected native owner absent. The profile and
    /// environment remain retained through the observation.
    WindowsProfileOwnerAbsent,
    /// The compatibility-runtime registry proved the exact owner absent, all
    /// owned native resources and injected content removed, and every owned
    /// callback/task drained.
    CompatibilityRegistryAbsentAndQuiescent,
}

/// Validated observations required to prove one compatibility runtime absent.
///
/// Compatibility runtimes have no platform extension identifier, so their
/// engine registry is authoritative only after every subordinate native and
/// asynchronous obligation has also been discharged. This witness prevents a
/// trusted adapter from omitting one of those checks when minting evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionRuntimeCompatibilityAbsenceAudit {
    _validated: (),
}

impl ExtensionRuntimeCompatibilityAbsenceAudit {
    /// Validates the complete compatibility-runtime absence observation set.
    #[must_use]
    pub const fn try_from_observations(
        owner_registry_absent: bool,
        owned_native_resources_absent: bool,
        injected_content_absent: bool,
        callbacks_and_tasks_drained: bool,
    ) -> Option<Self> {
        if owner_registry_absent
            && owned_native_resources_absent
            && injected_content_absent
            && callbacks_and_tasks_drained
        {
            Some(Self { _validated: () })
        } else {
            None
        }
    }
}

/// Validated raw macOS observations required before an adapter may claim
/// post-native absence.
///
/// This zero-sized witness carries no authority and no lineage. It merely
/// prevents the trusted raw WebKit boundary from accidentally omitting one of
/// the required readbacks when asking a reservation-bound issuer to mint the
/// actual evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionRuntimeMacosAbsenceAudit {
    _validated: (),
}

/// Validated raw observations for controller-wide macOS owner absence.
///
/// This proof is deliberately distinct from [`ExtensionRuntimeMacosAbsenceAudit`]:
/// crash recovery has no context from which it could honestly read grant
/// dictionaries or an independent identifier. Instead, the adapter reopens
/// the exact deterministic persistent controller/store pair and proves that
/// its complete context and extension inventories are empty.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionRuntimeMacosControllerAbsenceAudit {
    _validated: (),
}

/// Validated WebView2 observations required to prove one expected owner absent
/// from an exact profile.
///
/// The witness does not claim the whole profile inventory is empty. Other
/// independently journaled extensions may coexist in the same profile; the
/// startup content-view gate compares that complete cohort separately.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionRuntimeWindowsAbsenceAudit {
    _validated: (),
}

impl ExtensionRuntimeWindowsAbsenceAudit {
    /// Validates the complete Windows owner-absence observation set.
    #[must_use]
    pub const fn try_from_observations(
        environment_profile_binding_exact: bool,
        inventory_bounded_and_complete: bool,
        expected_owner_absent: bool,
    ) -> Option<Self> {
        if environment_profile_binding_exact
            && inventory_bounded_and_complete
            && expected_owner_absent
        {
            Some(Self { _validated: () })
        } else {
            None
        }
    }
}

impl ExtensionRuntimeMacosControllerAbsenceAudit {
    /// Validates the complete controller-namespace observation set.
    #[must_use]
    pub const fn try_from_observations(
        persistent_store_identity_exact: bool,
        persistent_controller_identity_exact: bool,
        controller_store_binding_exact: bool,
        extension_contexts_empty: bool,
        extensions_empty: bool,
    ) -> Option<Self> {
        if persistent_store_identity_exact
            && persistent_controller_identity_exact
            && controller_store_binding_exact
            && extension_contexts_empty
            && extensions_empty
        {
            Some(Self { _validated: () })
        } else {
            None
        }
    }
}

impl ExtensionRuntimeMacosAbsenceAudit {
    /// Validates the complete closed macOS absence observation set.
    ///
    /// Each `*_empty`, `*_non_granted`, `unloaded`, and `controller_absent`
    /// argument must be true. The private/all-host access observations must be
    /// false. Any omitted, contradictory, or future-unrecognized observation
    /// therefore fails closed.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn try_from_observations(
        granted_permissions_empty: bool,
        denied_permissions_empty: bool,
        granted_patterns_empty: bool,
        denied_patterns_empty: bool,
        private_access: bool,
        all_hosts_access: bool,
        captured_permission_status_non_granted: bool,
        captured_pattern_status_non_granted: bool,
        unloaded: bool,
        controller_absent: bool,
    ) -> Option<Self> {
        if granted_permissions_empty
            && denied_permissions_empty
            && granted_patterns_empty
            && denied_patterns_empty
            && !private_access
            && !all_hosts_access
            && captured_permission_status_non_granted
            && captured_pattern_status_non_granted
            && unloaded
            && controller_absent
        {
            Some(Self { _validated: () })
        } else {
            None
        }
    }
}

/// Compact, non-authorizing proof of definite native-runtime absence.
///
/// Raw construction is deliberately unavailable. Trusted host bindings mint
/// evidence through an issuer which is itself derived from an authenticated,
/// move-only activation or recovery context. The complete durable owner CAS,
/// backend, runtime family, registry generation, and native attempt form the
/// ABA fence. A macOS post-native proof additionally carries the independently
/// adapter-observed native identity. That observation must agree with the
/// catalog expectation, or with the durable prior observation for a migrated
/// row which predates the catalog-expectation field. When both anchors exist,
/// they must agree before the proof can be minted.
///
/// Copying this structural value grants no package, Store, native, or release
/// authority. Every consumer must rejoin it to the exact live host reservation.
///
/// Bare construction is intentionally impossible:
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionRuntimeAbsenceEvidence;
/// fn fabricate() -> ExtensionRuntimeAbsenceEvidence {
///     ExtensionRuntimeAbsenceEvidence {}
/// }
/// ```
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ExtensionRuntimeAbsenceEvidence {
    lineage: ExtensionRuntimeAbsenceLineage,
    backend: ExtensionRuntimeBackendTarget,
    target: ExtensionRuntimeTarget,
    generation: ExtensionRuntimeHostRegistryGeneration,
    attempt: NonZeroU64,
    proof: ExtensionRuntimeAbsenceProofKind,
    expected_native_identity: Option<ExtensionRuntimeNativeOwnerId>,
    observed_native_identity: Option<ExtensionRuntimeNativeOwnerId>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ExtensionRuntimeAbsenceLineage {
    Host(ExtensionNativeOwnershipEntryCas),
    #[cfg(test)]
    Test(NonZeroU64),
}

impl ExtensionRuntimeAbsenceEvidence {
    // Keep every security-relevant claim explicit at this sole raw boundary;
    // grouping them would make omitted lineage fields easier to default.
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn from_trusted_host(
        owner: ExtensionNativeOwnershipEntryCas,
        backend: ExtensionRuntimeBackendTarget,
        target: ExtensionRuntimeTarget,
        generation: ExtensionRuntimeHostRegistryGeneration,
        attempt: NonZeroU64,
        proof: ExtensionRuntimeAbsenceProofKind,
        expected_native_identity: Option<ExtensionRuntimeNativeOwnerId>,
        observed_native_identity: Option<ExtensionRuntimeNativeOwnerId>,
    ) -> Self {
        Self {
            lineage: ExtensionRuntimeAbsenceLineage::Host(owner),
            backend,
            target,
            generation,
            attempt,
            proof,
            expected_native_identity,
            observed_native_identity,
        }
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn for_test_host_lineage(
        owner: ExtensionNativeOwnershipEntryCas,
        backend: ExtensionRuntimeBackendTarget,
        target: ExtensionRuntimeTarget,
        generation: ExtensionRuntimeHostRegistryGeneration,
        attempt: NonZeroU64,
        proof: ExtensionRuntimeAbsenceProofKind,
        expected_native_identity: Option<ExtensionRuntimeNativeOwnerId>,
        observed_native_identity: Option<ExtensionRuntimeNativeOwnerId>,
    ) -> Self {
        Self::from_trusted_host(
            owner,
            backend,
            target,
            generation,
            attempt,
            proof,
            expected_native_identity,
            observed_native_identity,
        )
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn for_test_lineage(
        lineage: NonZeroU64,
        backend: ExtensionRuntimeBackendTarget,
        target: ExtensionRuntimeTarget,
        generation: ExtensionRuntimeHostRegistryGeneration,
        attempt: NonZeroU64,
        proof: ExtensionRuntimeAbsenceProofKind,
        expected_native_identity: Option<ExtensionRuntimeNativeOwnerId>,
        observed_native_identity: Option<ExtensionRuntimeNativeOwnerId>,
    ) -> Self {
        Self {
            lineage: ExtensionRuntimeAbsenceLineage::Test(lineage),
            backend,
            target,
            generation,
            attempt,
            proof,
            expected_native_identity,
            observed_native_identity,
        }
    }

    /// Exact durable owner CAS to which this observation is bound.
    #[must_use]
    pub const fn owner(self) -> Option<ExtensionNativeOwnershipEntryCas> {
        match self.lineage {
            ExtensionRuntimeAbsenceLineage::Host(owner) => Some(owner),
            #[cfg(test)]
            ExtensionRuntimeAbsenceLineage::Test(_) => None,
        }
    }

    /// Exact durable runtime backend to which this observation is bound.
    #[must_use]
    pub const fn backend(self) -> ExtensionRuntimeBackendTarget {
        self.backend
    }

    /// Runtime family to which this observation is bound.
    #[must_use]
    pub const fn target(self) -> ExtensionRuntimeTarget {
        self.target
    }

    /// Exact process-local host-registry generation.
    #[must_use]
    pub const fn registry_generation(self) -> ExtensionRuntimeHostRegistryGeneration {
        self.generation
    }

    /// Exact nonzero native-attempt identity within the registry process.
    #[must_use]
    pub const fn attempt(self) -> NonZeroU64 {
        self.attempt
    }

    /// Closed native-absence proof class.
    #[must_use]
    pub const fn proof_kind(self) -> ExtensionRuntimeAbsenceProofKind {
        self.proof
    }

    /// Catalog-authenticated identity carried by a post-native proof.
    #[must_use]
    pub const fn expected_native_identity(self) -> Option<ExtensionRuntimeNativeOwnerId> {
        self.expected_native_identity
    }

    /// Independently adapter-observed identity carried by a post-native proof.
    #[must_use]
    pub const fn observed_native_identity(self) -> Option<ExtensionRuntimeNativeOwnerId> {
        self.observed_native_identity
    }

    /// Checks this structural observation against one exact durable lineage.
    ///
    /// This is deliberately non-authorizing: the caller must still validate
    /// the process-local registry generation and the live reservation which
    /// issued the evidence. It centralizes the durable ABA, backend, runtime
    /// family, and expected/observed identity checks shared by activation and
    /// crash-recovery consumers.
    #[must_use]
    pub fn structurally_matches_entry(self, entry: &ExtensionNativeOwnershipEntry) -> bool {
        let Some(owner) = self.owner() else {
            return false;
        };
        if owner.key() != entry.key()
            || owner.operation() != entry.operation()
            || owner.native_incarnation() != entry.native_incarnation()
            || owner.revision() > entry.revision()
            || self.backend != entry.runtime_backend()
            || !target_matches_backend(self.target, self.backend)
            || self
                .expected_native_identity
                .map(|identity| identity.encoded_bytes())
                != entry
                    .expected_native_identity()
                    .map(|identity| identity.bytes())
        {
            return false;
        }

        match self.proof {
            ExtensionRuntimeAbsenceProofKind::ActivationNeverEntered => {
                self.observed_native_identity
                    .map(|identity| identity.encoded_bytes())
                    == entry.native_identity().map(|identity| identity.bytes())
            }
            ExtensionRuntimeAbsenceProofKind::MacosZeroGrantsAndUnloaded => {
                let identity_anchor = self
                    .expected_native_identity
                    .or(self.observed_native_identity);
                self.backend == ExtensionRuntimeBackendTarget::MacosNative
                    && self.target == ExtensionRuntimeTarget::NativeWebExtension
                    && identity_anchor.is_some()
                    && self.observed_native_identity == identity_anchor
                    && entry.native_identity().is_none_or(|identity| {
                        self.observed_native_identity
                            .is_some_and(|observed| observed.encoded_bytes() == identity.bytes())
                    })
            }
            ExtensionRuntimeAbsenceProofKind::MacosControllerNamespaceAbsent => {
                self.backend == ExtensionRuntimeBackendTarget::MacosNative
                    && self.target == ExtensionRuntimeTarget::NativeWebExtension
                    && self
                        .expected_native_identity
                        .zip(self.observed_native_identity)
                        .is_none_or(|(expected, observed)| expected == observed)
                    && self
                        .observed_native_identity
                        .map(|identity| identity.encoded_bytes())
                        == entry.native_identity().map(|identity| identity.bytes())
            }
            ExtensionRuntimeAbsenceProofKind::WindowsProfileOwnerAbsent => {
                self.backend == ExtensionRuntimeBackendTarget::WindowsNative
                    && self.target == ExtensionRuntimeTarget::NativeWebExtension
                    && self
                        .expected_native_identity
                        .zip(self.observed_native_identity)
                        .is_none_or(|(expected, observed)| expected == observed)
                    && self
                        .observed_native_identity
                        .map(|identity| identity.encoded_bytes())
                        == entry.native_identity().map(|identity| identity.bytes())
            }
            ExtensionRuntimeAbsenceProofKind::CompatibilityRegistryAbsentAndQuiescent => {
                self.target == ExtensionRuntimeTarget::Compatibility
                    && matches!(
                        self.backend,
                        ExtensionRuntimeBackendTarget::MacosCompatibility
                            | ExtensionRuntimeBackendTarget::LinuxCompatibility
                    )
                    && self.expected_native_identity.is_none()
                    && self.observed_native_identity.is_none()
                    && entry.expected_native_identity().is_none()
                    && entry.native_identity().is_none()
            }
        }
    }
}

const fn target_matches_backend(
    target: ExtensionRuntimeTarget,
    backend: ExtensionRuntimeBackendTarget,
) -> bool {
    matches!(
        (target, backend),
        (
            ExtensionRuntimeTarget::NativeWebExtension,
            ExtensionRuntimeBackendTarget::MacosNative
                | ExtensionRuntimeBackendTarget::WindowsNative
        ) | (
            ExtensionRuntimeTarget::Compatibility,
            ExtensionRuntimeBackendTarget::MacosCompatibility
                | ExtensionRuntimeBackendTarget::LinuxCompatibility
        )
    )
}

impl fmt::Debug for ExtensionRuntimeAbsenceEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeAbsenceEvidence")
            .field("lineage", &"[redacted]")
            .field("backend", &self.backend)
            .field("target", &self.target)
            .field("generation", &self.generation)
            .field("attempt", &self.attempt)
            .field("proof", &self.proof)
            .field("native_identity", &"[redacted]")
            .finish()
    }
}

/// Closed, structural description of exact native runtime ownership evidence.
///
/// This enum is publicly constructible and is non-authorizing by itself. Its
/// variants describe what a trusted adapter can attest; they do not attest
/// anything merely by existing. Authority is established only when the value
/// is returned through the service-selected trusted lifecycle port and joined
/// to that port's exact backend/native incarnation and the matching durable
/// ownership row. Native variants then carry identifiers authenticated by the
/// corresponding platform adapter. The compatibility variant is exact only
/// under that same join; compatibility runtimes have no separate platform
/// extension identifier.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum ExtensionRuntimeOwnershipEvidence {
    /// `WKWebExtensionContext.uniqueIdentifier`, assigned and authenticated by
    /// Zephium's macOS native adapter.
    MacosWebExtension(ExtensionRuntimeNativeOwnerId),
    /// `CoreWebView2BrowserExtension.Id`, authenticated by the Windows native
    /// adapter.
    WindowsWebView2Extension(ExtensionRuntimeNativeOwnerId),
    /// Zephium's compatibility owner, which has no platform-native extension
    /// identifier.
    Compatibility,
}

impl ExtensionRuntimeOwnershipEvidence {
    /// Returns the runtime family consistent with this evidence.
    #[must_use]
    pub const fn target(self) -> ExtensionRuntimeTarget {
        match self {
            Self::MacosWebExtension(_) | Self::WindowsWebView2Extension(_) => {
                ExtensionRuntimeTarget::NativeWebExtension
            }
            Self::Compatibility => ExtensionRuntimeTarget::Compatibility,
        }
    }
}

impl fmt::Debug for ExtensionRuntimeOwnershipEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MacosWebExtension(_) => formatter
                .debug_tuple("MacosWebExtension")
                .field(&"[redacted]")
                .finish(),
            Self::WindowsWebView2Extension(_) => formatter
                .debug_tuple("WindowsWebView2Extension")
                .field(&"[redacted]")
                .finish(),
            Self::Compatibility => formatter.write_str("Compatibility"),
        }
    }
}

/// Closed, structural expectation used to reconcile one conservatively
/// persisted runtime owner after process restart.
///
/// A native ownership row can be committed before the platform has returned
/// its stable owner identifier. Native variants therefore retain the exact
/// backend class, the independently catalog-authenticated identity expected
/// before the native call, and the independently adapter-observed identity
/// persisted after it. Legacy rows may lack either fact. The two identities
/// are never inferred from one another. [`Self::Compatibility`] is exact
/// immediately, because the compatibility runtime has no separate platform
/// identifier.
///
/// Like [`ExtensionRuntimeOwnershipEvidence`], constructing this value grants
/// no authority. A package service must join it to the exact durable row,
/// native incarnation, and service-selected trusted ownership port. Native
/// identifiers remain canonically bounded by
/// [`ExtensionRuntimeNativeOwnerId`].
///
/// This structural boundary intentionally has no implicit persistence format:
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryExpectation;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionRuntimeRecoveryExpectation>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionRuntimeRecoveryExpectation;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionRuntimeRecoveryExpectation>();
/// ```
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExtensionRuntimeRecoveryExpectation {
    /// A macOS `WKWebExtensionContext` with independent durable identity
    /// claims.
    MacosWebExtension {
        /// Exact catalog-authenticated identity selected before native work,
        /// or `None` for a legacy row that predates that durable binding.
        catalog_expected: Option<ExtensionRuntimeNativeOwnerId>,
        /// Exact adapter-observed `uniqueIdentifier`, or `None` when no
        /// authenticated observation was durably persisted.
        adapter_observed: Option<ExtensionRuntimeNativeOwnerId>,
    },
    /// A Windows WebView2 browser extension with independent durable identity
    /// claims.
    WindowsWebView2Extension {
        /// Exact catalog-authenticated identity selected before native work,
        /// or `None` for a legacy row that predates that durable binding.
        catalog_expected: Option<ExtensionRuntimeNativeOwnerId>,
        /// Exact adapter-observed `ICoreWebView2BrowserExtension::Id`, or
        /// `None` when no authenticated observation was durably persisted.
        adapter_observed: Option<ExtensionRuntimeNativeOwnerId>,
    },
    /// Zephium's exact compatibility-runtime owner class.
    Compatibility,
}

impl ExtensionRuntimeRecoveryExpectation {
    /// Builds an observed-only expectation from authenticated adapter
    /// evidence. This never fabricates an independent catalog expectation.
    #[must_use]
    pub const fn from_exact_evidence(evidence: ExtensionRuntimeOwnershipEvidence) -> Self {
        match evidence {
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(observed) => {
                Self::MacosWebExtension {
                    catalog_expected: None,
                    adapter_observed: Some(observed),
                }
            }
            ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(observed) => {
                Self::WindowsWebView2Extension {
                    catalog_expected: None,
                    adapter_observed: Some(observed),
                }
            }
            ExtensionRuntimeOwnershipEvidence::Compatibility => Self::Compatibility,
        }
    }

    /// Returns the runtime family consistent with this expectation.
    #[must_use]
    pub const fn target(self) -> ExtensionRuntimeTarget {
        match self {
            Self::MacosWebExtension { .. } | Self::WindowsWebView2Extension { .. } => {
                ExtensionRuntimeTarget::NativeWebExtension
            }
            Self::Compatibility => ExtensionRuntimeTarget::Compatibility,
        }
    }

    /// Returns exact adapter evidence already present in the durable row.
    ///
    /// A catalog expectation alone is deliberately not ownership evidence. An
    /// observation-less native expectation returns `None`; a trusted matching
    /// adapter observation can attach that identifier during reconciliation.
    /// Compatibility is exact at construction.
    #[must_use]
    pub const fn known_evidence(self) -> Option<ExtensionRuntimeOwnershipEvidence> {
        match self {
            Self::MacosWebExtension {
                adapter_observed: Some(observed),
                ..
            } => Some(ExtensionRuntimeOwnershipEvidence::MacosWebExtension(
                observed,
            )),
            Self::WindowsWebView2Extension {
                adapter_observed: Some(observed),
                ..
            } => Some(ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(
                observed,
            )),
            Self::Compatibility => Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
            Self::MacosWebExtension {
                adapter_observed: None,
                ..
            }
            | Self::WindowsWebView2Extension {
                adapter_observed: None,
                ..
            } => None,
        }
    }

    /// Whether independently persisted native identity claims already
    /// conflict.
    #[must_use]
    pub fn has_identity_conflict(self) -> bool {
        match self {
            Self::MacosWebExtension {
                catalog_expected: Some(expected),
                adapter_observed: Some(observed),
            }
            | Self::WindowsWebView2Extension {
                catalog_expected: Some(expected),
                adapter_observed: Some(observed),
            } => expected.encoded_bytes() != observed.encoded_bytes(),
            Self::MacosWebExtension { .. }
            | Self::WindowsWebView2Extension { .. }
            | Self::Compatibility => false,
        }
    }

    pub(crate) const fn accepts_evidence_backend(
        self,
        evidence: ExtensionRuntimeOwnershipEvidence,
    ) -> bool {
        matches!(
            (self, evidence),
            (
                Self::MacosWebExtension { .. },
                ExtensionRuntimeOwnershipEvidence::MacosWebExtension(_)
            ) | (
                Self::WindowsWebView2Extension { .. },
                ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(_)
            ) | (
                Self::Compatibility,
                ExtensionRuntimeOwnershipEvidence::Compatibility
            )
        )
    }

    pub(crate) fn accepts(self, evidence: ExtensionRuntimeOwnershipEvidence) -> bool {
        if !self.accepts_evidence_backend(evidence) {
            return false;
        }
        match (self, evidence) {
            (
                Self::MacosWebExtension {
                    catalog_expected,
                    adapter_observed,
                },
                ExtensionRuntimeOwnershipEvidence::MacosWebExtension(actual),
            )
            | (
                Self::WindowsWebView2Extension {
                    catalog_expected,
                    adapter_observed,
                },
                ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(actual),
            ) => {
                catalog_expected.is_none_or(|expected| expected == actual)
                    && adapter_observed.is_none_or(|observed| observed == actual)
            }
            (Self::Compatibility, ExtensionRuntimeOwnershipEvidence::Compatibility) => true,
            _ => false,
        }
    }
}

impl fmt::Debug for ExtensionRuntimeRecoveryExpectation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted = |identity: &Option<ExtensionRuntimeNativeOwnerId>| {
            identity.as_ref().map(|_| "[redacted]")
        };
        match self {
            Self::MacosWebExtension {
                catalog_expected,
                adapter_observed,
            } => formatter
                .debug_struct("MacosWebExtension")
                .field("catalog_expected", &redacted(catalog_expected))
                .field("adapter_observed", &redacted(adapter_observed))
                .finish(),
            Self::WindowsWebView2Extension {
                catalog_expected,
                adapter_observed,
            } => formatter
                .debug_struct("WindowsWebView2Extension")
                .field("catalog_expected", &redacted(catalog_expected))
                .field("adapter_observed", &redacted(adapter_observed))
                .finish(),
            Self::Compatibility => formatter.write_str("Compatibility"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANONICAL: &str = "abcdefghijklmnopabcdefghijklmnop";

    #[test]
    fn owner_id_accepts_only_the_exact_canonical_alphabet() {
        let id = ExtensionRuntimeNativeOwnerId::parse_exact(CANONICAL).expect("canonical id");
        assert_eq!(id.encoded_bytes(), *CANONICAL.as_bytes());
        assert_eq!(
            ExtensionRuntimeNativeOwnerId::from_encoded_bytes(id.encoded_bytes()),
            Ok(id)
        );

        for malformed in [
            "abcdefghijklmnopabcdefghijklmn",
            "abcdefghijklmnopabcdefghijklmnopq",
        ] {
            assert_eq!(
                ExtensionRuntimeNativeOwnerId::parse_exact(malformed),
                Err(ExtensionRuntimeNativeOwnerIdError::InvalidLength)
            );
        }

        for canonical_byte in b'a'..=b'p' {
            let mut bytes = [b'a'; EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES];
            bytes[17] = canonical_byte;
            assert!(ExtensionRuntimeNativeOwnerId::from_encoded_bytes(bytes).is_ok());
        }
        for byte in u8::MIN..=u8::MAX {
            if matches!(byte, b'a'..=b'p') {
                continue;
            }
            for index in [0, EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES / 2, 31] {
                let mut bytes = [b'a'; EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES];
                bytes[index] = byte;
                assert_eq!(
                    ExtensionRuntimeNativeOwnerId::from_encoded_bytes(bytes),
                    Err(ExtensionRuntimeNativeOwnerIdError::NonCanonical)
                );
            }
        }
        for malformed in [
            "Abcdefghijklmnopabcdefghijklmnop",
            "abcdefghijklmnopabcdefghijklmn0p",
            "abcdefghijklmnopabcdefghijklmnqp",
        ] {
            assert_eq!(
                ExtensionRuntimeNativeOwnerId::parse_exact(malformed),
                Err(ExtensionRuntimeNativeOwnerIdError::NonCanonical)
            );
        }
    }

    #[test]
    fn owner_id_and_evidence_debug_are_redacted() {
        let id = ExtensionRuntimeNativeOwnerId::parse_exact(CANONICAL).expect("canonical id");
        for debug in [
            format!("{id:?}"),
            format!(
                "{:?}",
                ExtensionRuntimeOwnershipEvidence::MacosWebExtension(id)
            ),
            format!(
                "{:?}",
                ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(id)
            ),
        ] {
            assert!(debug.contains("redacted"));
            assert!(!debug.contains(CANONICAL));
        }
    }

    #[test]
    fn evidence_has_a_closed_target_mapping_and_bounded_layout() {
        let id = ExtensionRuntimeNativeOwnerId::parse_exact(CANONICAL).expect("canonical id");
        assert_eq!(
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(id).target(),
            ExtensionRuntimeTarget::NativeWebExtension
        );
        assert_eq!(
            ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(id).target(),
            ExtensionRuntimeTarget::NativeWebExtension
        );
        assert_eq!(
            ExtensionRuntimeOwnershipEvidence::Compatibility.target(),
            ExtensionRuntimeTarget::Compatibility
        );
        assert_eq!(
            std::mem::size_of::<ExtensionRuntimeNativeOwnerId>(),
            EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES
        );
        assert!(std::mem::size_of::<ExtensionRuntimeOwnershipEvidence>() <= 40);
    }

    #[test]
    fn recovery_expectation_is_backend_exact_and_bounded() {
        let id = ExtensionRuntimeNativeOwnerId::parse_exact(CANONICAL).expect("canonical id");
        let macos = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: Some(id),
            adapter_observed: Some(id),
        };
        let catalog_only_macos = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: Some(id),
            adapter_observed: None,
        };
        let observed_only_macos = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: None,
            adapter_observed: Some(id),
        };
        let identityless_macos = ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: None,
            adapter_observed: None,
        };
        let windows = ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
            catalog_expected: Some(id),
            adapter_observed: Some(id),
        };
        let identityless_windows = ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
            catalog_expected: None,
            adapter_observed: None,
        };

        assert_eq!(
            ExtensionRuntimeRecoveryExpectation::from_exact_evidence(
                ExtensionRuntimeOwnershipEvidence::MacosWebExtension(id)
            ),
            observed_only_macos
        );
        assert_eq!(
            ExtensionRuntimeRecoveryExpectation::from_exact_evidence(
                ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(id)
            ),
            ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
                catalog_expected: None,
                adapter_observed: Some(id),
            }
        );
        assert_eq!(
            ExtensionRuntimeRecoveryExpectation::Compatibility,
            ExtensionRuntimeRecoveryExpectation::from_exact_evidence(
                ExtensionRuntimeOwnershipEvidence::Compatibility
            )
        );

        for expectation in [
            macos,
            catalog_only_macos,
            observed_only_macos,
            identityless_macos,
            windows,
            identityless_windows,
        ] {
            assert_eq!(
                expectation.target(),
                ExtensionRuntimeTarget::NativeWebExtension
            );
        }
        assert_eq!(
            ExtensionRuntimeRecoveryExpectation::Compatibility.target(),
            ExtensionRuntimeTarget::Compatibility
        );
        assert_eq!(
            macos.known_evidence(),
            Some(ExtensionRuntimeOwnershipEvidence::MacosWebExtension(id))
        );
        assert_eq!(catalog_only_macos.known_evidence(), None);
        assert_eq!(
            observed_only_macos.known_evidence(),
            Some(ExtensionRuntimeOwnershipEvidence::MacosWebExtension(id))
        );
        assert_eq!(identityless_macos.known_evidence(), None);
        assert_eq!(identityless_windows.known_evidence(), None);
        assert_eq!(
            ExtensionRuntimeRecoveryExpectation::Compatibility.known_evidence(),
            Some(ExtensionRuntimeOwnershipEvidence::Compatibility)
        );
        assert!(std::mem::size_of::<ExtensionRuntimeRecoveryExpectation>() <= 72);

        let other = ExtensionRuntimeNativeOwnerId::from_encoded_bytes(
            [b'p'; EXTENSION_RUNTIME_NATIVE_OWNER_ID_BYTES],
        )
        .expect("canonical different ID");
        assert!(ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
            catalog_expected: Some(id),
            adapter_observed: Some(other),
        }
        .has_identity_conflict());
        assert!(!catalog_only_macos.has_identity_conflict());
        assert!(!observed_only_macos.has_identity_conflict());
    }

    #[test]
    fn recovery_expectation_debug_never_reveals_native_identifiers() {
        let id = ExtensionRuntimeNativeOwnerId::parse_exact(CANONICAL).expect("canonical id");
        for debug in [
            format!(
                "{:?}",
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                    catalog_expected: Some(id),
                    adapter_observed: Some(id),
                }
            ),
            format!(
                "{:?}",
                ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
                    catalog_expected: Some(id),
                    adapter_observed: Some(id),
                }
            ),
        ] {
            assert!(debug.contains("redacted"));
            assert!(!debug.contains(CANONICAL));
        }
        assert_eq!(
            format!(
                "{:?}",
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                    catalog_expected: None,
                    adapter_observed: None,
                }
            ),
            "MacosWebExtension { catalog_expected: None, adapter_observed: None }"
        );
    }

    #[test]
    fn absence_audits_require_every_observation_without_exception() {
        let compatibility = [true; 4];
        assert!(
            ExtensionRuntimeCompatibilityAbsenceAudit::try_from_observations(
                compatibility[0],
                compatibility[1],
                compatibility[2],
                compatibility[3],
            )
            .is_some()
        );
        for flipped in 0..compatibility.len() {
            let mut observations = compatibility;
            observations[flipped] = false;
            assert!(
                ExtensionRuntimeCompatibilityAbsenceAudit::try_from_observations(
                    observations[0],
                    observations[1],
                    observations[2],
                    observations[3],
                )
                .is_none(),
                "compatibility observation {flipped} must be mandatory"
            );
        }

        let macos = [true, true, true, true, false, false, true, true, true, true];
        assert!(ExtensionRuntimeMacosAbsenceAudit::try_from_observations(
            macos[0], macos[1], macos[2], macos[3], macos[4], macos[5], macos[6], macos[7],
            macos[8], macos[9],
        )
        .is_some());
        for flipped in 0..macos.len() {
            let mut observations = macos;
            observations[flipped] = !observations[flipped];
            assert!(
                ExtensionRuntimeMacosAbsenceAudit::try_from_observations(
                    observations[0],
                    observations[1],
                    observations[2],
                    observations[3],
                    observations[4],
                    observations[5],
                    observations[6],
                    observations[7],
                    observations[8],
                    observations[9],
                )
                .is_none(),
                "macOS observation {flipped} must be mandatory"
            );
        }

        let controller = [true; 5];
        assert!(
            ExtensionRuntimeMacosControllerAbsenceAudit::try_from_observations(
                controller[0],
                controller[1],
                controller[2],
                controller[3],
                controller[4],
            )
            .is_some()
        );
        for flipped in 0..controller.len() {
            let mut observations = controller;
            observations[flipped] = false;
            assert!(
                ExtensionRuntimeMacosControllerAbsenceAudit::try_from_observations(
                    observations[0],
                    observations[1],
                    observations[2],
                    observations[3],
                    observations[4],
                )
                .is_none(),
                "macOS controller observation {flipped} must be mandatory"
            );
        }

        let windows = [true; 3];
        assert!(ExtensionRuntimeWindowsAbsenceAudit::try_from_observations(
            windows[0], windows[1], windows[2],
        )
        .is_some());
        for flipped in 0..windows.len() {
            let mut observations = windows;
            observations[flipped] = false;
            assert!(
                ExtensionRuntimeWindowsAbsenceAudit::try_from_observations(
                    observations[0],
                    observations[1],
                    observations[2],
                )
                .is_none(),
                "Windows observation {flipped} must be mandatory"
            );
        }
    }
}
