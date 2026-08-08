//! Durable obligations for profile-scoped platform extension namespaces.
//!
//! An obligation is deliberately weaker than proof that a namespace exists:
//! once a platform call could have created durable native state, Store retains
//! the obligation until profile deletion joins an exact native-erasure proof.
//! False positives are safe; forgetting a possible namespace is not.

/// Maximum number of profile-scoped native extension namespace obligations
/// retained by Store.
///
/// The browser supports at most 64 active profiles and at most 64 concurrent
/// profile-deletion tombstones, so 128 covers the complete durable union
/// without making corrupt cohorts unbounded.
pub const MAX_EXTENSION_NATIVE_NAMESPACE_OBLIGATIONS: usize = 128;

/// Exact platform namespace that profile deletion must erase.
///
/// Unknown future durable versions must be rejected by older Store binaries;
/// they must never be projected onto a namespace whose erasure contract may
/// be weaker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExtensionNativeNamespaceScope {
    /// The deterministic, persistent WKWebExtensionController namespace and
    /// its associated WKWebsiteDataStore introduced by the first macOS native
    /// extension runtime.
    MacosControllerV1,
}

impl ExtensionNativeNamespaceScope {
    /// Durable Store representation for this exact erasure contract.
    pub const fn persisted_version(self) -> u8 {
        match self {
            Self::MacosControllerV1 => 1,
        }
    }

    /// Decodes only versions whose complete erasure semantics this binary
    /// understands. Unknown values fail closed.
    pub const fn from_persisted_version(version: u8) -> Option<Self> {
        match version {
            1 => Some(Self::MacosControllerV1),
            _ => None,
        }
    }

    /// Exact retained-memory contribution when embedded in bounded Store
    /// deletion work.
    pub const fn retained_bytes(self) -> usize {
        let _ = self;
        core::mem::size_of::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_versions_are_exact_and_unknown_versions_fail_closed() {
        let scope = ExtensionNativeNamespaceScope::MacosControllerV1;
        assert_eq!(scope.persisted_version(), 1);
        assert_eq!(
            ExtensionNativeNamespaceScope::from_persisted_version(1),
            Some(scope)
        );
        for unknown in [0, 2, u8::MAX] {
            assert_eq!(
                ExtensionNativeNamespaceScope::from_persisted_version(unknown),
                None
            );
        }
        assert_eq!(
            scope.retained_bytes(),
            core::mem::size_of::<ExtensionNativeNamespaceScope>()
        );
    }
}
