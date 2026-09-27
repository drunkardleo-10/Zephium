//! Immutable, non-authorizing provenance for an on-device extension artifact.

use super::{
    ApiPermissionName, ExtensionArchiveDigest, ExtensionAuthorityId,
    ExtensionCompatibilityTargetId, ExtensionManifestDescriptor, ExtensionManifestDigest,
    ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackagePayloadIdentity,
    ExtensionPackageRevision, ExtensionTreeDigest, ExtensionUpstreamCheckpoint,
    EXTENSION_UPSTREAM_CHECKPOINT_BYTES,
};

/// Byte ceiling for an encoded provenance record and its logical retained size.
pub const MAX_EXTENSION_INSTALL_PROVENANCE_BYTES: usize = 1024;
/// Bounded publisher high-water history retained even after uninstall.
pub const MAX_EXTENSION_UPSTREAM_HISTORY_PER_PROFILE: usize = 128;

// Maximum encoded fixed fields plus the provider, transform and runtime
// tokens. Keep the one-byte token codec and Store's immutable bound honest.
const _: () = assert!(super::MAX_EXTENSION_API_PERMISSION_NAME_BYTES <= u8::MAX as usize);
const _: () = assert!(
    524 + 3 * super::MAX_EXTENSION_API_PERMISSION_NAME_BYTES
        <= MAX_EXTENSION_INSTALL_PROVENANCE_BYTES
);

/// Approved acquisition provider identity; never a caller-selected fetch URL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionProvenanceSource {
    /// Original signed Chrome Web Store package.
    ChromeWebStore,
    /// A separately approved compiled publisher-provider profile.
    Publisher(ApiPermissionName),
}

/// Original authenticated closed-tree identities before any transformation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionSourceTreeIdentity {
    /// Original manifest SHA-256.
    pub manifest: [u8; 32],
    /// Original canonical resource-tree SHA-256.
    pub tree: [u8; 32],
    /// Original canonical tree-index SHA-256.
    pub index: [u8; 32],
}

/// Exact local transformation, independent of publisher package authentication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionTransformProvenance {
    /// No transformation; input and output identities must agree.
    Identity,
    /// A compiled transform with a positive revision and exact asset digest.
    Compiled {
        /// Compiled transformation profile, not a remotely supplied program.
        target: ExtensionCompatibilityTargetId,
        /// Exact compiled transformation revision.
        revision: std::num::NonZeroU32,
        /// SHA-256 of the compiled transform assets/algorithm descriptor.
        sha256: [u8; 32],
    },
}

/// Historical policy evidence; these fields never authenticate remote policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionProvenancePolicy {
    /// Positive revision of the authenticated shared policy used for admission.
    pub revision: std::num::NonZeroU64,
    /// Exact shared policy payload SHA-256.
    pub sha256: [u8; 32],
}

/// Complete immutable source/transform/output/compatibility join.
///
/// This is structural data, not Verified or Beta admission authority. Store
/// binds it atomically to install and grant rows; the native ownership journal
/// already records their exact package and revision identities. Activation
/// still requires reauthenticated package bytes and a separate runtime witness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionInstallProvenance {
    source: ExtensionProvenanceSource,
    upstream: ExtensionUpstreamCheckpoint,
    original: ExtensionSourceTreeIdentity,
    transform: ExtensionTransformProvenance,
    package: ExtensionPackageIdentity,
    output_index: [u8; 32],
    runtime_target: ExtensionCompatibilityTargetId,
    compatibility: [u8; 32],
    policy: ExtensionProvenancePolicy,
}

/// Exact reauthenticated current and replacement provenance for an update.
#[derive(Clone, Debug)]
pub struct ExtensionProvenanceUpdate {
    current: std::sync::Arc<ExtensionInstallProvenance>,
    replacement: std::sync::Arc<ExtensionInstallProvenance>,
}

impl ExtensionProvenanceUpdate {
    /// Binds one provider/publisher/update line and non-regressing policy.
    /// Source-provider migration requires a separate explicit admission path.
    pub fn new(
        current: std::sync::Arc<ExtensionInstallProvenance>,
        replacement: std::sync::Arc<ExtensionInstallProvenance>,
    ) -> Option<Self> {
        if current.source != replacement.source
            || current.upstream.publisher() != replacement.upstream.publisher()
            || current.package.update_line() != replacement.package.update_line()
            || replacement.policy.revision < current.policy.revision
            || (replacement.policy.revision == current.policy.revision
                && replacement.policy.sha256 != current.policy.sha256)
        {
            return None;
        }
        Some(Self {
            current,
            replacement,
        })
    }
    /// Exact expected current provenance; Store compares every field.
    pub fn current(&self) -> &ExtensionInstallProvenance {
        &self.current
    }
    /// Exact replacement provenance committed with the package and grants.
    pub fn replacement(&self) -> &ExtensionInstallProvenance {
        &self.replacement
    }
    /// Conservative charge independent of shared Arc aliases.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.current.retained_bytes()
            + self.replacement.retained_bytes()
    }
}

impl ExtensionInstallProvenance {
    /// Binds structural provenance to the exact admitted output descriptor.
    /// The caller must separately prove original CRX and transform execution.
    pub fn new(
        source: ExtensionProvenanceSource,
        upstream: ExtensionUpstreamCheckpoint,
        original: ExtensionSourceTreeIdentity,
        transform: ExtensionTransformProvenance,
        manifest: &ExtensionManifestDescriptor,
        output_index: [u8; 32],
        policy: ExtensionProvenancePolicy,
    ) -> Option<Self> {
        let value = Self {
            source,
            upstream,
            original,
            transform,
            package: manifest.package().clone(),
            output_index,
            runtime_target: manifest.compatibility_target().clone(),
            compatibility: manifest.compatibility_digest().bytes(),
            policy,
        };
        value.is_valid().then_some(value)
    }

    fn is_valid(&self) -> bool {
        self.policy.revision.get() <= i64::MAX as u64
            && self.retained_bytes() <= MAX_EXTENSION_INSTALL_PROVENANCE_BYTES
            && self.beta_fields_are_consistent()
            && (!matches!(self.transform, ExtensionTransformProvenance::Identity)
                || (self.original.manifest == self.package.manifest_sha256().bytes()
                    && self.original.tree == self.package.tree_sha256().bytes()
                    && self.original.index == self.output_index
                    && self.package.payload().acquired_zip_evidence().is_some_and(
                        |(_, digest)| digest.bytes() == self.upstream.archive_sha256(),
                    )))
    }

    fn beta_fields_are_consistent(&self) -> bool {
        if !super::is_beta_extension_authority(self.package.authority()) {
            return true;
        }
        self.upstream.publisher() == self.package.key()
            && self
                .package
                .payload()
                .acquired_zip_evidence()
                .is_some_and(|(_, digest)| digest.bytes() == self.upstream.archive_sha256())
            && [
                super::ExtensionBetaRuntimeTarget::MacosNative,
                super::ExtensionBetaRuntimeTarget::WindowsNative,
            ]
            .into_iter()
            .any(|runtime| {
                (runtime.target_id() == self.runtime_target.as_str()
                    || (self.package.authority() == runtime.authority(super::ExtensionBetaChannel::Local)
                        && self.runtime_target.as_str() == runtime.bounded_storage_target_id()
                        && matches!(&self.transform, ExtensionTransformProvenance::Compiled { target, .. }
                            if target.as_str() == "local.bounded-storage.v1"))
                    || (runtime == super::ExtensionBetaRuntimeTarget::MacosNative
                        && self.package.authority() == runtime.authority(super::ExtensionBetaChannel::Local)
                        && matches!(&self.transform, ExtensionTransformProvenance::Compiled { target, .. }
                            if (self.runtime_target.as_str() == super::LOCAL_MACOS_HISTORY_V3_COMPATIBILITY_TARGET && target.as_str() == "local.webkit-history.v3")
                            || (self.runtime_target.as_str() == super::LOCAL_MACOS_CAPABILITIES_V1_COMPATIBILITY_TARGET && target.as_str() == "local.webkit-capabilities.v1")
                            || (self.runtime_target.as_str() == super::LOCAL_MACOS_CAPABILITIES_V2_COMPATIBILITY_TARGET && target.as_str() == "local.webkit-capabilities.v2")
                            || (self.runtime_target.as_str() == super::LOCAL_MACOS_IDENTITY_V1_COMPATIBILITY_TARGET && target.as_str() == "local.webkit-identity.v1")
                            || (self.runtime_target.as_str() == super::LOCAL_MACOS_MAIN_DOCUMENT_GLOBS_V1_COMPATIBILITY_TARGET && target.as_str() == "local.webkit-main-document-globs.v1")
                            || (self.runtime_target.as_str() == super::LOCAL_MACOS_HISTORY_V2_COMPATIBILITY_TARGET && target.as_str() == "local.webkit-history.v2")
                            || (self.runtime_target.as_str() == super::MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET
                                && target.as_str() == "local.webkit-brokered.v1")
                            || (self.runtime_target.as_str() == super::LOCAL_MACOS_ADAPTED_COMPATIBILITY_TARGET
                                && target.as_str() == "local.webkit-adapted.v1"))))
                    && [
                        super::ExtensionBetaChannel::Stable,
                        super::ExtensionBetaChannel::Staging,
                        super::ExtensionBetaChannel::Local,
                    ]
                    .into_iter()
                    .any(|channel| runtime.authority(channel) == self.package.authority())
            })
    }

    /// Exact acquisition provider.
    pub const fn source(&self) -> &ExtensionProvenanceSource {
        &self.source
    }
    /// Compiled native compatibility target recorded for these exact bytes.
    pub const fn runtime_target(&self) -> &ExtensionCompatibilityTargetId {
        &self.runtime_target
    }
    /// Original authenticated upstream version and byte identities.
    pub const fn upstream(&self) -> ExtensionUpstreamCheckpoint {
        self.upstream
    }
    /// Original authenticated closed tree.
    pub const fn original(&self) -> ExtensionSourceTreeIdentity {
        self.original
    }
    /// Exact compiled transformation or identity path.
    pub const fn transform(&self) -> &ExtensionTransformProvenance {
        &self.transform
    }
    /// Exact output package joined by install, grant, and native journal rows.
    pub const fn package(&self) -> &ExtensionPackageIdentity {
        &self.package
    }
    /// Exact output tree-index SHA-256.
    pub const fn output_index(&self) -> [u8; 32] {
        self.output_index
    }
    /// Exact output compatibility classification digest, not an admission witness.
    pub const fn compatibility_digest(&self) -> [u8; 32] {
        self.compatibility
    }
    /// Historical signed-policy identity.
    pub const fn policy(&self) -> ExtensionProvenancePolicy {
        self.policy
    }
    /// Rejoins all output descriptor fields without creating authority.
    pub fn matches_manifest(&self, manifest: &ExtensionManifestDescriptor) -> bool {
        &self.package == manifest.package()
            && &self.runtime_target == manifest.compatibility_target()
            && self.compatibility == manifest.compatibility_digest().bytes()
    }
    /// Conservative retained bytes, including owned token strings.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.runtime_target.as_str().len()
            + match &self.source {
                ExtensionProvenanceSource::ChromeWebStore => 0,
                ExtensionProvenanceSource::Publisher(id) => id.as_str().len(),
            }
            + match &self.transform {
                ExtensionTransformProvenance::Identity => 0,
                ExtensionTransformProvenance::Compiled { target, .. } => target.as_str().len(),
            }
    }

    /// Encodes bounded structural data; no signature or native witness is encoded.
    pub fn encode(&self) -> Box<[u8]> {
        let mut out = Vec::with_capacity(MAX_EXTENSION_INSTALL_PROVENANCE_BYTES);
        out.push(1);
        match &self.source {
            ExtensionProvenanceSource::ChromeWebStore => out.push(1),
            ExtensionProvenanceSource::Publisher(id) => {
                out.push(2);
                token(&mut out, id.as_str());
            }
        }
        out.extend_from_slice(&self.upstream.encode());
        for digest in [
            self.original.manifest,
            self.original.tree,
            self.original.index,
        ] {
            out.extend_from_slice(&digest);
        }
        match &self.transform {
            ExtensionTransformProvenance::Identity => out.push(0),
            ExtensionTransformProvenance::Compiled {
                target,
                revision,
                sha256,
            } => {
                out.push(1);
                token(&mut out, target.as_str());
                out.extend_from_slice(&revision.get().to_be_bytes());
                out.extend_from_slice(sha256);
            }
        }
        out.extend_from_slice(self.package.authority().as_bytes());
        out.extend_from_slice(self.package.key().as_bytes());
        out.extend_from_slice(&self.package.revision().get().to_be_bytes());
        match self.package.payload() {
            ExtensionPackagePayloadIdentity::BundledTree => out.push(1),
            ExtensionPackagePayloadIdentity::AcquiredZip { length, sha256 } => {
                out.push(2);
                out.extend_from_slice(&length.get().to_be_bytes());
                out.extend_from_slice(sha256.as_bytes());
            }
        }
        out.extend_from_slice(self.package.manifest_sha256().as_bytes());
        out.extend_from_slice(self.package.tree_sha256().as_bytes());
        out.extend_from_slice(&self.output_index);
        token(&mut out, self.runtime_target.as_str());
        out.extend_from_slice(&self.compatibility);
        out.extend_from_slice(&self.policy.revision.get().to_be_bytes());
        out.extend_from_slice(&self.policy.sha256);
        out.into_boxed_slice()
    }

    /// Decodes exact bounded data and repeats semantic validation. Restored
    /// bytes must still be compared with independently reauthenticated evidence.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > MAX_EXTENSION_INSTALL_PROVENANCE_BYTES {
            return None;
        }
        let mut input = Input(bytes);
        if input.take::<1>()? != [1] {
            return None;
        }
        let source = match input.take::<1>()? {
            [1] => ExtensionProvenanceSource::ChromeWebStore,
            [2] => ExtensionProvenanceSource::Publisher(
                ApiPermissionName::parse_exact(input.token()?).ok()?,
            ),
            _ => return None,
        };
        let upstream = ExtensionUpstreamCheckpoint::decode(
            &input.take::<EXTENSION_UPSTREAM_CHECKPOINT_BYTES>()?,
        )?;
        let original = ExtensionSourceTreeIdentity {
            manifest: input.take()?,
            tree: input.take()?,
            index: input.take()?,
        };
        let transform = match input.take::<1>()? {
            [0] => ExtensionTransformProvenance::Identity,
            [1] => ExtensionTransformProvenance::Compiled {
                target: ExtensionCompatibilityTargetId::parse_exact(input.token()?).ok()?,
                revision: std::num::NonZeroU32::new(u32::from_be_bytes(input.take()?))?,
                sha256: input.take()?,
            },
            _ => return None,
        };
        let authority = ExtensionAuthorityId::from_bytes(input.take()?);
        let key = ExtensionPackageKey::from_bytes(input.take()?);
        let revision = ExtensionPackageRevision::new(u64::from_be_bytes(input.take()?))?;
        let payload = match input.take::<1>()? {
            [1] => ExtensionPackagePayloadIdentity::BundledTree,
            [2] => ExtensionPackagePayloadIdentity::acquired_zip(
                u64::from_be_bytes(input.take()?),
                ExtensionArchiveDigest::from_bytes(input.take()?),
            )?,
            _ => return None,
        };
        let package = ExtensionPackageIdentity::new(
            authority,
            key,
            revision,
            payload,
            ExtensionManifestDigest::from_bytes(input.take()?),
            ExtensionTreeDigest::from_bytes(input.take()?),
        );
        let value = Self {
            source,
            upstream,
            original,
            transform,
            package,
            output_index: input.take()?,
            runtime_target: ExtensionCompatibilityTargetId::parse_exact(input.token()?).ok()?,
            compatibility: input.take()?,
            policy: ExtensionProvenancePolicy {
                revision: std::num::NonZeroU64::new(u64::from_be_bytes(input.take()?))?,
                sha256: input.take()?,
            },
        };
        (input.0.is_empty() && value.is_valid()).then_some(value)
    }
}

fn token(out: &mut Vec<u8>, value: &str) {
    // Core permission/target tokens are capped at 96 bytes.
    out.push(value.len() as u8);
    out.extend_from_slice(value.as_bytes());
}
struct Input<'a>(&'a [u8]);
impl<'a> Input<'a> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, tail) = self.0.split_at_checked(N)?;
        self.0 = tail;
        head.try_into().ok()
    }
    fn token(&mut self) -> Option<&'a str> {
        let length = usize::from(self.take::<1>()?[0]);
        if length == 0 || length > super::MAX_EXTENSION_API_PERMISSION_NAME_BYTES {
            return None;
        }
        let (head, tail) = self.0.split_at_checked(length)?;
        self.0 = tail;
        std::str::from_utf8(head).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::*;

    fn manifest(revision: u64) -> ExtensionManifestDescriptor {
        let package = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::new(revision).unwrap(),
            ExtensionPackagePayloadIdentity::acquired_zip(
                17,
                ExtensionArchiveDigest::from_bytes([4; 32]),
            )
            .unwrap(),
            ExtensionManifestDigest::from_bytes([5; 32]),
            ExtensionTreeDigest::from_bytes([6; 32]),
        );
        let declarations = ExtensionManifestDeclarations::new(
            ExtensionApiPermissionSet::new(Vec::new()).unwrap(),
            ExtensionApiPermissionSet::new(Vec::new()).unwrap(),
            None,
            None,
            None,
            None,
            Vec::new(),
            ExtensionManifestExecutionSurfaces::new(
                Vec::new(),
                ExtensionContentSecurityPolicyDeclaration::new(
                    ExtensionManifestResourceDigest::from_bytes([7; 32]),
                ),
                None,
                Vec::new(),
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap();
        let compatibility = declarations
            .declaration_keys()
            .into_iter()
            .map(|declaration| {
                ExtensionCompatibilityClassification::new(
                    declaration,
                    ExtensionCompatibilityLevel::Compatible,
                )
            })
            .collect();
        ExtensionManifestDescriptor::new(
            package,
            3,
            declarations,
            ExtensionCompatibilityTargetId::parse_exact("macos.wkwebextension.v1").unwrap(),
            compatibility,
        )
        .unwrap()
    }

    fn provenance(manifest: &ExtensionManifestDescriptor) -> ExtensionInstallProvenance {
        ExtensionInstallProvenance::new(
            ExtensionProvenanceSource::ChromeWebStore,
            ExtensionUpstreamCheckpoint::from_parts(
                ExtensionPackageKey::from_bytes([3; 32]),
                ExtensionUpstreamVersion::parse("1.2.0").unwrap(),
                [8; 32],
                [4; 32],
            ),
            ExtensionSourceTreeIdentity {
                manifest: [5; 32],
                tree: [6; 32],
                index: [9; 32],
            },
            ExtensionTransformProvenance::Identity,
            manifest,
            [9; 32],
            ExtensionProvenancePolicy {
                revision: std::num::NonZeroU64::new(1).unwrap(),
                sha256: [10; 32],
            },
        )
        .unwrap()
    }

    #[test]
    fn codec_binds_original_output_and_policy_and_rejects_every_truncated_prefix() {
        let manifest = manifest(1);
        let mut value = provenance(&manifest);
        for compiled in [false, true] {
            if compiled {
                value.source = ExtensionProvenanceSource::Publisher(
                    ApiPermissionName::parse_exact(&"a".repeat(96)).unwrap(),
                );
                value.transform = ExtensionTransformProvenance::Compiled {
                    target: ExtensionCompatibilityTargetId::parse_exact(&"b".repeat(96)).unwrap(),
                    revision: std::num::NonZeroU32::new(1).unwrap(),
                    sha256: [11; 32],
                };
                value.runtime_target =
                    ExtensionCompatibilityTargetId::parse_exact(&"c".repeat(96)).unwrap();
            }
            let bytes = value.encode();
            assert!(bytes.len() <= MAX_EXTENSION_INSTALL_PROVENANCE_BYTES);
            if compiled {
                assert_eq!(bytes.len(), 812);
            }
            assert!(value.retained_bytes() <= MAX_EXTENSION_INSTALL_PROVENANCE_BYTES);
            assert_eq!(
                ExtensionInstallProvenance::decode(&bytes),
                Some(value.clone())
            );
            for length in 0..bytes.len() {
                assert!(ExtensionInstallProvenance::decode(&bytes[..length]).is_none());
            }
            let mut trailing = bytes.to_vec();
            trailing.push(0);
            assert!(ExtensionInstallProvenance::decode(&trailing).is_none());
            let mut unknown = bytes.to_vec();
            unknown[0] = 2;
            assert!(ExtensionInstallProvenance::decode(&unknown).is_none());
        }
    }

    #[test]
    fn identity_cannot_hide_a_transform_and_descriptor_revision_is_exact() {
        let value = provenance(&manifest(1));
        assert!(value.matches_manifest(&manifest(1)));
        assert!(!value.matches_manifest(&manifest(2)));
        let mut changed = value.clone();
        changed.original.tree = [99; 32];
        assert!(!changed.is_valid());
        assert!(ExtensionInstallProvenance::decode(&changed.encode()).is_none());
        let mut changed = value;
        changed.output_index = [99; 32];
        assert!(!changed.is_valid());
    }

    #[test]
    fn update_rejects_provider_switch_and_policy_rollback_or_equivocation() {
        use std::sync::Arc;
        let current = Arc::new(provenance(&manifest(1)));
        let replacement = provenance(&manifest(2));
        assert!(
            ExtensionProvenanceUpdate::new(current.clone(), Arc::new(replacement.clone()))
                .is_some()
        );
        let mut switched = replacement.clone();
        switched.source = ExtensionProvenanceSource::Publisher(
            ApiPermissionName::parse_exact("other.publisher").unwrap(),
        );
        assert!(ExtensionProvenanceUpdate::new(current.clone(), Arc::new(switched)).is_none());
        let mut different = replacement.clone();
        different.policy.sha256 = [99; 32];
        assert!(ExtensionProvenanceUpdate::new(current, Arc::new(different)).is_none());
        let mut newer = replacement.clone();
        newer.policy.revision = std::num::NonZeroU64::new(2).unwrap();
        assert!(ExtensionProvenanceUpdate::new(Arc::new(newer), Arc::new(replacement)).is_none());
    }
}
