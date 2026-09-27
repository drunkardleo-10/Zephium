//! Structural Beta namespace labels shared by admission and durable joins.
//! These labels are not source, provider, consent or runtime authority.

use super::ExtensionAuthorityId;
use sha2::{Digest, Sha256};

/// Local WebKit adaptation profile. A label alone grants no native-app access.
pub const LOCAL_MACOS_ADAPTED_COMPATIBILITY_TARGET: &str = "macos.wkwebextension-adapted.v1";

/// Query-aware, read-only history compatibility. Separate from the v1 artifact target.
pub const LOCAL_MACOS_HISTORY_V2_COMPATIBILITY_TARGET: &str = "macos.wkwebextension-history.v2";

/// Delivers the query-aware adapter; older v2 artifacts retain their original bytes.
pub const LOCAL_MACOS_HISTORY_V3_COMPATIBILITY_TARGET: &str = "macos.wkwebextension-history.v3";

/// Local broker profile for declared browser APIs without history coupling.
pub const LOCAL_MACOS_CAPABILITIES_V1_COMPATIBILITY_TARGET: &str =
    "macos.wkwebextension-capabilities.v1";

/// Composed on-demand offscreen storage and explicit sandbox withholding.
pub const LOCAL_MACOS_CAPABILITIES_V2_COMPATIBILITY_TARGET: &str =
    "macos.wkwebextension-capabilities.v2";

/// Local on-demand Chromium identity web-flow adapter. It provides no Chrome
/// account or cached-token emulation.
pub const LOCAL_MACOS_IDENTITY_V1_COMPATIBILITY_TARGET: &str = "macos.wkwebextension-identity.v1";

/// Exact-document relay for isolated, idle, main-frame glob-bearing scripts.
pub const LOCAL_MACOS_MAIN_DOCUMENT_GLOBS_V1_COMPATIBILITY_TARGET: &str =
    "macos.wkwebextension-main-document-globs.v1";

/// Separate external-package admission namespaces. Labels are not authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionBetaChannel {
    /// Public release policy channel.
    Stable,
    /// Separately rooted staging policy channel.
    Staging,
    /// On-device admission under the compiled browser policy, without a remote
    /// per-install approval. Never interpreted as a signed metadata channel.
    Local,
}

/// Compiled Beta backend domain, independent of the current build host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionBetaRuntimeTarget {
    /// Apple's WKWebExtension backend.
    MacosNative,
    /// Microsoft's WebView2 backend.
    WindowsNative,
}

impl ExtensionBetaRuntimeTarget {
    /// Local-only manifest adaptation retaining the platform's storage quota.
    pub const fn bounded_storage_target_id(self) -> &'static str {
        match self {
            Self::MacosNative => "macos.wkwebextension-bounded-storage.v1",
            Self::WindowsNative => "windows.webview2-bounded-storage.v1",
        }
    }
    /// Exact policy target; remote data cannot invent a backend.
    pub const fn target_id(self) -> &'static str {
        match self {
            Self::MacosNative => "macos.wkwebextension.v1",
            Self::WindowsNative => "windows.webview2.v1",
        }
    }

    /// Resolves a compiled local compatibility profile to its native engine.
    /// This does not admit packages or enable signed-policy targets.
    pub fn from_local_compatibility_target(target: &str) -> Option<Self> {
        match target {
            "macos.wkwebextension.v1"
            | super::MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET
            | LOCAL_MACOS_ADAPTED_COMPATIBILITY_TARGET
            | LOCAL_MACOS_HISTORY_V2_COMPATIBILITY_TARGET
            | LOCAL_MACOS_HISTORY_V3_COMPATIBILITY_TARGET
            | LOCAL_MACOS_CAPABILITIES_V1_COMPATIBILITY_TARGET
            | LOCAL_MACOS_CAPABILITIES_V2_COMPATIBILITY_TARGET
            | LOCAL_MACOS_IDENTITY_V1_COMPATIBILITY_TARGET
            | LOCAL_MACOS_MAIN_DOCUMENT_GLOBS_V1_COMPATIBILITY_TARGET
            | "macos.wkwebextension-bounded-storage.v1" => Some(Self::MacosNative),
            "windows.webview2.v1" | "windows.webview2-bounded-storage.v1" => {
                Some(Self::WindowsNative)
            }
            _ => None,
        }
    }

    /// Stable structural update-line namespace. Possession proves no admission.
    pub fn authority(self, channel: ExtensionBetaChannel) -> ExtensionAuthorityId {
        let mut hash = Sha256::new();
        hash.update(b"zephium:beta-source-authority:v1\0");
        hash.update(match channel {
            ExtensionBetaChannel::Stable => b"stable\0".as_slice(),
            ExtensionBetaChannel::Staging => b"staging\0".as_slice(),
            ExtensionBetaChannel::Local => b"local\0".as_slice(),
        });
        hash.update(self.target_id().as_bytes());
        ExtensionAuthorityId::from_bytes(hash.finalize().into())
    }
}

/// Identifies compiled Beta namespaces so missing provenance cannot enter a
/// legacy install/grant/runtime path. This is a refusal rule, not admission.
pub fn is_beta_extension_authority(authority: ExtensionAuthorityId) -> bool {
    [
        ExtensionBetaRuntimeTarget::MacosNative,
        ExtensionBetaRuntimeTarget::WindowsNative,
    ]
    .into_iter()
    .any(|runtime| {
        [
            ExtensionBetaChannel::Stable,
            ExtensionBetaChannel::Staging,
            ExtensionBetaChannel::Local,
        ]
        .into_iter()
        .any(|channel| runtime.authority(channel) == authority)
    })
}

/// Structural content identity of one immutable Beta repository object. This
/// is deliberately distinct from a signed catalog-set digest and proves no
/// package admission by itself.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionBetaObjectDigest([u8; 32]);

impl ExtensionBetaObjectDigest {
    /// Decodes structural journal bytes; the Store/repository join must verify them.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    /// Exact fixed-width durable bytes.
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
    /// Reconstructs the immutable object address from complete persisted evidence.
    pub fn from_provenance(provenance: &super::ExtensionInstallProvenance) -> Self {
        Self::from_parts(
            provenance.package(),
            provenance.output_index(),
            provenance.upstream(),
            provenance.transform(),
            provenance.compatibility_digest(),
        )
    }
    /// Hashes complete structural identity. No provider or native authority is minted.
    pub fn from_parts(
        package: &super::ExtensionPackageIdentity,
        index: [u8; 32],
        upstream: super::ExtensionUpstreamCheckpoint,
        transform: &super::ExtensionTransformProvenance,
        compatibility: [u8; 32],
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"zephium:beta-repository-object:v1\0");
        digest.update(package.authority().bytes());
        digest.update(package.key().bytes());
        digest.update(package.revision().get().to_le_bytes());
        package.payload().update_sha256(&mut digest);
        digest.update(package.manifest_sha256().bytes());
        digest.update(package.tree_sha256().bytes());
        digest.update(index);
        digest.update(upstream.encode());
        digest.update(compatibility);
        match transform {
            super::ExtensionTransformProvenance::Identity => digest.update([0]),
            super::ExtensionTransformProvenance::Compiled {
                target,
                revision,
                sha256,
            } => {
                digest.update([1]);
                digest.update((target.as_str().len() as u32).to_le_bytes());
                digest.update(target.as_str().as_bytes());
                digest.update(revision.get().to_le_bytes());
                digest.update(sha256);
            }
        }
        Self(digest.finalize().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_compatibility_profiles_route_to_their_actual_native_engine() {
        assert_eq!(
            ExtensionBetaRuntimeTarget::from_local_compatibility_target(
                super::super::MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET
            ),
            Some(ExtensionBetaRuntimeTarget::MacosNative)
        );
        assert_eq!(
            ExtensionBetaRuntimeTarget::from_local_compatibility_target(
                LOCAL_MACOS_CAPABILITIES_V1_COMPATIBILITY_TARGET
            ),
            Some(ExtensionBetaRuntimeTarget::MacosNative)
        );
        assert_eq!(
            ExtensionBetaRuntimeTarget::from_local_compatibility_target("macos.wkwebextension.v1"),
            Some(ExtensionBetaRuntimeTarget::MacosNative)
        );
        assert_eq!(
            ExtensionBetaRuntimeTarget::from_local_compatibility_target("windows.webview2.v1"),
            Some(ExtensionBetaRuntimeTarget::WindowsNative)
        );
        assert_eq!(
            ExtensionBetaRuntimeTarget::from_local_compatibility_target("unreviewed.future.target"),
            None
        );
    }
    #[test]
    fn published_beta_namespace_labels_stay_exact_and_separate() {
        for (channel, runtime, expected) in [
            (
                ExtensionBetaChannel::Stable,
                ExtensionBetaRuntimeTarget::MacosNative,
                "e3da979db873ec00b4f1b8be4496b5428f961db187b5c4588f3d5b30d60426da",
            ),
            (
                ExtensionBetaChannel::Stable,
                ExtensionBetaRuntimeTarget::WindowsNative,
                "9378c48279f1efef79cd8fb2a8c0227214e13961084dd2c5f64585248c3d1368",
            ),
            (
                ExtensionBetaChannel::Staging,
                ExtensionBetaRuntimeTarget::MacosNative,
                "4e86c0e24ae61e3cc7078300975eaff4a296bcf1192df0285140e21715fc9239",
            ),
            (
                ExtensionBetaChannel::Staging,
                ExtensionBetaRuntimeTarget::WindowsNative,
                "b8d3da2d3adb78aaa3a4c44ae0402664bd6e9a2600de328b192bd7116076a71e",
            ),
        ] {
            let id = runtime.authority(channel);
            assert!(is_beta_extension_authority(id));
            let encoded = id
                .bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(encoded, expected);
        }
        assert!(!is_beta_extension_authority(
            ExtensionAuthorityId::from_bytes([0; 32])
        ));
        assert!(!is_beta_extension_authority(
            ExtensionAuthorityId::from_bytes([1; 32])
        ));
    }
}
