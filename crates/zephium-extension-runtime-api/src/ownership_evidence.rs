//! Exact, backend-authenticated native ownership evidence.

use std::fmt;

use crate::ExtensionRuntimeTarget;

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
/// backend class and optionally the identifier already authenticated and
/// persisted by the service. [`Self::Compatibility`] is exact immediately,
/// because the compatibility runtime has no separate platform identifier.
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
    /// A macOS `WKWebExtensionContext`, optionally with its already persisted
    /// `uniqueIdentifier`.
    MacosWebExtension {
        /// Exact durable owner identifier, or `None` when native creation may
        /// have started but no authenticated identifier was persisted.
        expected: Option<ExtensionRuntimeNativeOwnerId>,
    },
    /// A Windows WebView2 browser extension, optionally with its already
    /// persisted `ICoreWebView2BrowserExtension::Id`.
    WindowsWebView2Extension {
        /// Exact durable owner identifier, or `None` when native creation may
        /// have started but no authenticated identifier was persisted.
        expected: Option<ExtensionRuntimeNativeOwnerId>,
    },
    /// Zephium's exact compatibility-runtime owner class.
    Compatibility,
}

impl ExtensionRuntimeRecoveryExpectation {
    /// Builds the exact expectation corresponding to authenticated ownership
    /// evidence.
    #[must_use]
    pub const fn from_exact_evidence(evidence: ExtensionRuntimeOwnershipEvidence) -> Self {
        match evidence {
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(expected) => {
                Self::MacosWebExtension {
                    expected: Some(expected),
                }
            }
            ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(expected) => {
                Self::WindowsWebView2Extension {
                    expected: Some(expected),
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

    /// Returns exact evidence already present in the durable expectation.
    ///
    /// An identityless native expectation returns `None`; a trusted matching
    /// adapter observation can attach that identifier during reconciliation.
    /// Compatibility is exact at construction.
    #[must_use]
    pub const fn known_evidence(self) -> Option<ExtensionRuntimeOwnershipEvidence> {
        match self {
            Self::MacosWebExtension {
                expected: Some(expected),
            } => Some(ExtensionRuntimeOwnershipEvidence::MacosWebExtension(
                expected,
            )),
            Self::WindowsWebView2Extension {
                expected: Some(expected),
            } => Some(ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(
                expected,
            )),
            Self::Compatibility => Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
            Self::MacosWebExtension { expected: None }
            | Self::WindowsWebView2Extension { expected: None } => None,
        }
    }

    pub(crate) fn accepts(self, evidence: ExtensionRuntimeOwnershipEvidence) -> bool {
        match (self, evidence) {
            (
                Self::MacosWebExtension { expected },
                ExtensionRuntimeOwnershipEvidence::MacosWebExtension(actual),
            )
            | (
                Self::WindowsWebView2Extension { expected },
                ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(actual),
            ) => expected.is_none_or(|expected| expected == actual),
            (Self::Compatibility, ExtensionRuntimeOwnershipEvidence::Compatibility) => true,
            _ => false,
        }
    }
}

impl fmt::Debug for ExtensionRuntimeRecoveryExpectation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let expected = |expected: &Option<ExtensionRuntimeNativeOwnerId>| {
            expected.as_ref().map(|_| "[redacted]")
        };
        match self {
            Self::MacosWebExtension { expected: owner_id } => formatter
                .debug_struct("MacosWebExtension")
                .field("expected", &expected(owner_id))
                .finish(),
            Self::WindowsWebView2Extension { expected: owner_id } => formatter
                .debug_struct("WindowsWebView2Extension")
                .field("expected", &expected(owner_id))
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
        let macos = ExtensionRuntimeRecoveryExpectation::MacosWebExtension { expected: Some(id) };
        let identityless_macos =
            ExtensionRuntimeRecoveryExpectation::MacosWebExtension { expected: None };
        let windows =
            ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension { expected: Some(id) };
        let identityless_windows =
            ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension { expected: None };

        assert_eq!(
            macos,
            ExtensionRuntimeRecoveryExpectation::from_exact_evidence(
                ExtensionRuntimeOwnershipEvidence::MacosWebExtension(id)
            )
        );
        assert_eq!(
            windows,
            ExtensionRuntimeRecoveryExpectation::from_exact_evidence(
                ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(id)
            )
        );
        assert_eq!(
            ExtensionRuntimeRecoveryExpectation::Compatibility,
            ExtensionRuntimeRecoveryExpectation::from_exact_evidence(
                ExtensionRuntimeOwnershipEvidence::Compatibility
            )
        );

        for expectation in [macos, identityless_macos, windows, identityless_windows] {
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
        assert_eq!(identityless_macos.known_evidence(), None);
        assert_eq!(identityless_windows.known_evidence(), None);
        assert_eq!(
            ExtensionRuntimeRecoveryExpectation::Compatibility.known_evidence(),
            Some(ExtensionRuntimeOwnershipEvidence::Compatibility)
        );
        assert!(std::mem::size_of::<ExtensionRuntimeRecoveryExpectation>() <= 40);
    }

    #[test]
    fn recovery_expectation_debug_never_reveals_native_identifiers() {
        let id = ExtensionRuntimeNativeOwnerId::parse_exact(CANONICAL).expect("canonical id");
        for debug in [
            format!(
                "{:?}",
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension { expected: Some(id) }
            ),
            format!(
                "{:?}",
                ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
                    expected: Some(id)
                }
            ),
        ] {
            assert!(debug.contains("redacted"));
            assert!(!debug.contains(CANONICAL));
        }
        assert_eq!(
            format!(
                "{:?}",
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension { expected: None }
            ),
            "MacosWebExtension { expected: None }"
        );
    }
}
