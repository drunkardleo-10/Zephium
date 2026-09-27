//! Product-owned admission of an authenticated original MV3 source for the
//! public Beta pipeline. This is separate from exact Verified catalogs.
//! Source admission does not authenticate acquisition-provider eligibility,
//! a transformed output, user grants, a durable package, or a native lease.

mod compatibility;
mod native_hosts;
mod preparation;
#[cfg(test)]
mod tests;
mod withheld;

use std::sync::Arc;
#[cfg(test)]
use zephium_core::extensions::ExtensionAuthorityId;

use sha2::{Digest, Sha256};
use zephium_core::extensions::{
    ExtensionCompatibilityLevel, ExtensionPackageIdentity, ExtensionPackageKey,
    ExtensionPackageRevision, ExtensionSourceTreeIdentity, ExtensionUpstreamCheckpoint,
};
use zephium_extension_acquisition::AcquiredExtensionTreeReceipt;
use zephium_extension_package::{
    assess_upstream_extension_manifest, AdmittedExtensionManifest, ExpectedChromiumIdentity,
    ExtensionPolicyChannel, ExtensionPublicPolicy,
};

use crate::public_policy::AcceptedExtensionPolicy;
pub use compatibility::{BetaCompatibilityLimitation, BetaRuntimeTarget};
pub use preparation::{
    BetaArtifactPreparationError, BetaPreparationWorkspace, PreparedBetaArtifact,
    MAX_PREPARED_BETA_ARTIFACT_RETAINED_BYTES,
};
#[cfg(feature = "capabilities-v2-qa")]
pub use preparation::signed_bitwarden_qa_transform_refresh;

/// Version of the compiled source-admission rules. Remote policy can opt into
/// this exact version, but cannot add APIs, transforms, or runtime capabilities.
pub const BETA_SOURCE_POLICY_VERSION: u32 = 1;

/// Hard retained-memory ceiling for one admitted source. The shared signed
/// policy has a separate worker/cache budget and is never copied into each source.
pub const MAX_BETA_SOURCE_RETAINED_BYTES: usize =
    zephium_extension_acquisition::MAX_ACQUIRED_TREE_RECEIPT_RETAINED_BYTES
        + zephium_core::extensions::MAX_EXTENSION_MANIFEST_RETAINED_BYTES
        + zephium_extension_package::MAX_EXTENSION_MANIFEST_PLAN_RETAINED_BYTES
        + 2 * zephium_extension_package::MAX_EXTENSION_MANIFEST_BYTES
        + 16 * 1024;

/// Bounded source-admission rejection. No package code is executed here.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BetaSourceAdmissionError {
    /// Signed policy is expired, superseded, or its durable owner closed.
    #[error("Beta policy is no longer current")]
    PolicyUnavailable,
    /// This exact compiled target and policy version is not enabled remotely.
    #[error("Beta runtime policy is not enabled")]
    TargetNotEnabled,
    /// The authenticated publisher or original CRX has been revoked.
    #[error("extension publisher or package is revoked")]
    Revoked,
    /// Original manifest, publisher key, or complete tree evidence disagrees.
    #[error("authenticated extension source is inconsistent")]
    SourceMismatch,
    /// The last durably recorded publisher version forbids this source.
    #[error("extension upstream rollback or equivocation detected")]
    UpstreamRollback,
    /// A declaration cannot be honored under the compiled source policy.
    #[error("extension declaration is unsupported in Beta")]
    Unsupported(zephium_core::ports::extensions::ExtensionUnsupportedFeatures),
    /// The source plan exceeded its conservative owned-memory budget.
    #[error("Beta source admission exceeds its memory budget")]
    Capacity,
}

/// Nominal product source-admission witness for the Beta pipeline. It is not
/// cloneable, serializable, or convertible into a Verified manifest witness.
/// The original stream receipt remains owned here until an independently
/// verified materializer/transform consumes the source.
///
/// Every use must call `revalidate`; changing policy or dropping its durable
/// cache invalidates this witness. Neither this type nor its structural
/// descriptor can be passed to an existing Verified native activation path.
///
/// ```compile_fail
/// fn require_verified(_: &zephium_extension_authority::ProductAdmittedExtensionManifest) {}
/// fn cannot_promote(beta: &zephium_extension_distribution::beta::ProductAdmittedBetaSource) {
///     require_verified(beta);
/// }
/// ```
pub struct ProductAdmittedBetaSource {
    policy: SourcePolicy,
    source: AcquiredExtensionTreeReceipt,
    manifest: AdmittedExtensionManifest,
    upstream: ExtensionUpstreamCheckpoint,
    previous: Option<ExtensionUpstreamCheckpoint>,
    runtime: BetaRuntimeTarget,
    limitations: Vec<BetaCompatibilityLimitation>,
    withheld: withheld::WithheldFeatures,
}

// This private discriminator prevents an expired signed-policy admission from
// being treated as a locally admitted store package.
enum SourcePolicy {
    Signed(Arc<AcceptedExtensionPolicy>),
    Local,
    LocalLegacyHistory,
    LocalHistoryV2,
    LocalHistoryV3,
    // No public source-admission path selects this until native offscreen
    // ownership, grants, and DOM erasure have live proof.
    LocalCapabilitiesV2Candidate,
}

/// Locally authenticated external package source. No remote approval is needed
/// and this value cannot be converted into the signed-policy Beta witness.
pub struct ProductAdmittedExternalSource {
    inner: ProductAdmittedBetaSource,
}
impl std::fmt::Debug for ProductAdmittedExternalSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProductAdmittedExternalSource")
            .finish_non_exhaustive()
    }
}
impl ProductAdmittedExternalSource {
    /// Authenticated original manifest and compiled compatibility result.
    pub fn manifest(&self) -> &AdmittedExtensionManifest {
        &self.inner.manifest
    }
    /// Local retained-memory charge; no network client or policy owner exists.
    pub fn retained_bytes(&self) -> usize {
        self.inner.retained_bytes()
    }
}

/// Authenticates local source evidence and upstream continuity under the
/// compiled native policy. Provider selection and user consent remain separate.
pub fn admit_external_source(
    source: AcquiredExtensionTreeReceipt,
    manifest_bytes: &[u8],
    runtime: BetaRuntimeTarget,
    revision: ExtensionPackageRevision,
    previous: Option<ExtensionUpstreamCheckpoint>,
) -> Result<ProductAdmittedExternalSource, BetaSourceAdmissionError> {
    admit_source(
        SourcePolicy::Local,
        source,
        manifest_bytes,
        runtime,
        revision,
        previous,
    )
    .map(|inner| ProductAdmittedExternalSource { inner })
}

#[cfg(test)]
pub(super) fn admit_external_source_candidate_v2(
    source: AcquiredExtensionTreeReceipt,
    manifest_bytes: &[u8],
    runtime: BetaRuntimeTarget,
    revision: ExtensionPackageRevision,
    previous: Option<ExtensionUpstreamCheckpoint>,
) -> Result<ProductAdmittedExternalSource, BetaSourceAdmissionError> {
    admit_source(
        SourcePolicy::LocalCapabilitiesV2Candidate,
        source,
        manifest_bytes,
        runtime,
        revision,
        previous,
    )
    .map(|inner| ProductAdmittedExternalSource { inner })
}

/// One exact authenticated Bitwarden 2026.9.2 archive for isolated product QA.
/// Neither the feature nor this function changes ordinary external admission.
#[cfg(feature = "capabilities-v2-qa")]
pub(crate) const SIGNED_BITWARDEN_QA_CRX_SHA256: [u8; 32] = [
    0x6d, 0x30, 0x87, 0xda, 0xb3, 0x0d, 0x21, 0x54, 0x94, 0x80, 0x58, 0xda, 0xe8, 0xe4, 0xc7,
    0xe4, 0x14, 0x20, 0xd2, 0xac, 0x81, 0x68, 0x51, 0xe7, 0xc6, 0x8c, 0xaa, 0x93, 0x24, 0x16,
    0xc9, 0x30,
];

/// Admits only the signed source above into the separate v2 QA target.
#[cfg(feature = "capabilities-v2-qa")]
pub fn admit_signed_bitwarden_capabilities_v2_qa(
    source: AcquiredExtensionTreeReceipt,
    manifest_bytes: &[u8],
    runtime: BetaRuntimeTarget,
    revision: ExtensionPackageRevision,
    previous: Option<ExtensionUpstreamCheckpoint>,
) -> Result<ProductAdmittedExternalSource, BetaSourceAdmissionError> {
    let runtime_matches = runtime == BetaRuntimeTarget::MacosNative;
    let id_matches = source.extension_id().as_str() == "nngceckbapebfimnlniiiahkandclblb";
    let archive_matches = source.original_crx_sha256() == SIGNED_BITWARDEN_QA_CRX_SHA256;
    if std::env::var("ZEPHIUM_OFFSCREEN_QA_TRACE").as_deref() == Ok("1") {
        eprintln!(
            "bitwarden-qa-prepare: exact-source runtime={runtime_matches} id={id_matches} archive={archive_matches}"
        );
    }
    if !runtime_matches || !id_matches || !archive_matches {
        return Err(BetaSourceAdmissionError::SourceMismatch);
    }
    admit_source(
        SourcePolicy::LocalCapabilitiesV2Candidate,
        source,
        manifest_bytes,
        runtime,
        revision,
        previous,
    )
    .map(|inner| ProductAdmittedExternalSource { inner })
}

impl std::fmt::Debug for ProductAdmittedBetaSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductAdmittedBetaSource")
            .field("runtime", &self.runtime)
            .finish_non_exhaustive()
    }
}

impl ProductAdmittedBetaSource {
    fn rules(&self) -> compatibility::CompiledBetaPolicy {
        match self.policy {
            SourcePolicy::Signed(_) => compatibility::CompiledBetaPolicy::new(self.runtime),
            SourcePolicy::Local
            | SourcePolicy::LocalCapabilitiesV2Candidate
            | SourcePolicy::LocalLegacyHistory
            | SourcePolicy::LocalHistoryV2
            | SourcePolicy::LocalHistoryV3 => {
                compatibility::CompiledBetaPolicy::for_external(self.runtime)
                    .with_brokered(self.uses_brokered_transform())
                    .with_capability_broker(self.uses_capability_transform())
                    .with_history_v2(self.uses_history_v2())
                    .with_history_v3(self.uses_history_v3())
                    .with_adapted(
                        self.uses_adapted_transform(),
                        self.uses_publisher_transform(),
                    )
                    .with_identity(
                        self.uses_identity_transform()
                            || (self.uses_main_document_globs_transform()
                                && self
                                    .manifest
                                    .descriptor()
                                    .declarations()
                                    .required_api()
                                    .contains_exact("identity")),
                    )
                    .with_main_document_globs(self.uses_main_document_globs_transform())
                    .with_capabilities_v2(self.uses_capabilities_v2_transform())
                    .with_bounded_storage(self.uses_bounded_storage_transform())
            }
        }
    }
    fn uses_brokered_transform(&self) -> bool {
        matches!(
            self.policy,
            SourcePolicy::Local
                | SourcePolicy::LocalCapabilitiesV2Candidate
                | SourcePolicy::LocalLegacyHistory
                | SourcePolicy::LocalHistoryV2
                | SourcePolicy::LocalHistoryV3
        ) && self.runtime == BetaRuntimeTarget::MacosNative
            && matches!(
                self.manifest.descriptor().compatibility_target().as_str(),
                zephium_core::extensions::MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET
                    | zephium_core::extensions::LOCAL_MACOS_HISTORY_V2_COMPATIBILITY_TARGET
                    | zephium_core::extensions::LOCAL_MACOS_HISTORY_V3_COMPATIBILITY_TARGET
            )
    }
    fn uses_history_v2(&self) -> bool {
        self.uses_history_v3()
            || self.manifest.descriptor().compatibility_target().as_str()
                == zephium_core::extensions::LOCAL_MACOS_HISTORY_V2_COMPATIBILITY_TARGET
    }
    fn uses_capability_transform(&self) -> bool {
        matches!(self.policy, SourcePolicy::Local)
            && self.runtime == BetaRuntimeTarget::MacosNative
            && self.manifest.descriptor().compatibility_target().as_str()
                == zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V1_COMPATIBILITY_TARGET
    }
    fn uses_capabilities_v2_transform(&self) -> bool {
        matches!(self.policy, SourcePolicy::LocalCapabilitiesV2Candidate)
            && self.runtime == BetaRuntimeTarget::MacosNative
            && self.manifest.descriptor().compatibility_target().as_str()
                == zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V2_COMPATIBILITY_TARGET
    }
    fn uses_identity_transform(&self) -> bool {
        matches!(self.policy, SourcePolicy::Local)
            && self.runtime == BetaRuntimeTarget::MacosNative
            && self.manifest.descriptor().compatibility_target().as_str()
                == zephium_core::extensions::LOCAL_MACOS_IDENTITY_V1_COMPATIBILITY_TARGET
    }
    fn uses_main_document_globs_transform(&self) -> bool {
        matches!(self.policy, SourcePolicy::Local)
            && self.runtime == BetaRuntimeTarget::MacosNative
            && self.manifest.descriptor().compatibility_target().as_str()
                == zephium_core::extensions::LOCAL_MACOS_MAIN_DOCUMENT_GLOBS_V1_COMPATIBILITY_TARGET
    }
    fn uses_history_v3(&self) -> bool {
        self.manifest.descriptor().compatibility_target().as_str()
            == zephium_core::extensions::LOCAL_MACOS_HISTORY_V3_COMPATIBILITY_TARGET
    }
    fn uses_adapted_transform(&self) -> bool {
        matches!(
            self.policy,
            SourcePolicy::Local
                | SourcePolicy::LocalCapabilitiesV2Candidate
                | SourcePolicy::LocalLegacyHistory
                | SourcePolicy::LocalHistoryV2
                | SourcePolicy::LocalHistoryV3
        ) && self.runtime == BetaRuntimeTarget::MacosNative
            && self.manifest.descriptor().compatibility_target().as_str()
                == zephium_core::extensions::LOCAL_MACOS_ADAPTED_COMPATIBILITY_TARGET
    }
    fn uses_publisher_transform(&self) -> bool {
        self.uses_adapted_transform()
            && self
                .manifest
                .descriptor()
                .declarations()
                .required_api()
                .contains_exact("nativeMessaging")
            && native_hosts::supports(self.upstream.publisher().bytes())
    }
    fn uses_bounded_storage_transform(&self) -> bool {
        matches!(
            self.policy,
            SourcePolicy::Local
                | SourcePolicy::LocalCapabilitiesV2Candidate
                | SourcePolicy::LocalLegacyHistory
                | SourcePolicy::LocalHistoryV2
                | SourcePolicy::LocalHistoryV3
        ) && (self.manifest.descriptor().compatibility_target().as_str()
            == self.runtime.bounded_storage_target_id()
            || self
                .limitations
                .contains(&BetaCompatibilityLimitation::BoundedStorageQuota))
    }
    /// Rechecks live policy ownership, expiry, compiled opt-in, and revocation.
    /// No caller-supplied clock or policy implementation crosses this boundary.
    pub fn revalidate(&self) -> Result<(), BetaSourceAdmissionError> {
        match &self.policy {
            SourcePolicy::Signed(policy) => check_policy(
                policy
                    .policy()
                    .map_err(|_| BetaSourceAdmissionError::PolicyUnavailable)?,
                self.runtime,
                &self.source,
            ),
            SourcePolicy::Local
            | SourcePolicy::LocalCapabilitiesV2Candidate
            | SourcePolicy::LocalLegacyHistory
            | SourcePolicy::LocalHistoryV2
            | SourcePolicy::LocalHistoryV3 => Ok(()),
        }
    }

    /// Fresh, exact source descriptor and resource plan. This describes the
    /// original tree; transformations must be authenticated independently.
    pub fn manifest(&self) -> Result<&AdmittedExtensionManifest, BetaSourceAdmissionError> {
        self.revalidate()?;
        Ok(&self.manifest)
    }

    /// Bounded explicit limitations to present before installation consent.
    pub fn limitations(&self) -> &[BetaCompatibilityLimitation] {
        &self.limitations
    }

    /// Original authenticated version and CRX/ZIP digests for atomic Store joins.
    pub const fn upstream(&self) -> ExtensionUpstreamCheckpoint {
        self.upstream
    }

    /// Expected prior durable high-water mark. Store must compare it again at
    /// installation commit; source admission cannot mutate or replace it.
    pub const fn previous_upstream(&self) -> Option<ExtensionUpstreamCheckpoint> {
        self.previous
    }

    /// Original authenticated manifest, tree, and index provenance.
    pub fn original_tree(&self) -> ExtensionSourceTreeIdentity {
        ExtensionSourceTreeIdentity {
            manifest: self.source.index().manifest_sha256().bytes(),
            tree: self.source.index().tree_sha256().bytes(),
            index: self.source.index().index_sha256().bytes(),
        }
    }

    /// Historical signed policy identity for output provenance. It grants no
    /// authority after this source witness expires or becomes superseded.
    pub fn policy_evidence(
        &self,
    ) -> Result<zephium_core::extensions::ExtensionProvenancePolicy, BetaSourceAdmissionError> {
        self.revalidate()?;
        match &self.policy {
            SourcePolicy::Signed(policy) => {
                let policy = policy.policy().map_err(|_| BetaSourceAdmissionError::PolicyUnavailable)?;
                Ok(zephium_core::extensions::ExtensionProvenancePolicy { revision: std::num::NonZeroU64::new(policy.revision()).ok_or(BetaSourceAdmissionError::PolicyUnavailable)?, sha256: policy.sha256() })
            }
            SourcePolicy::Local | SourcePolicy::LocalCapabilitiesV2Candidate | SourcePolicy::LocalLegacyHistory | SourcePolicy::LocalHistoryV2 | SourcePolicy::LocalHistoryV3 => Ok(zephium_core::extensions::ExtensionProvenancePolicy { revision: std::num::NonZeroU64::MIN, sha256: Sha256::digest(b"zephium:local-external-admission:v1;authenticated-crx3;native-capabilities;explicit-grants;no-remote-approval").into() }),
        }
    }

    /// Reports a required identity adaptation, never silently inserting a key
    /// or treating the original publisher signature as an output signature.
    pub fn requires_manifest_key_transform(&self) -> bool {
        self.manifest.chromium_key().is_none()
    }

    /// Conservatively accounts owned source/manifest memory. Shared policy is
    /// charged separately by its single cache/worker owner.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.source.retained_bytes()
            + self.manifest.retained_bytes()
            + self.limitations.capacity() * std::mem::size_of::<BetaCompatibilityLimitation>()
            + self.withheld.retained_bytes()
    }
}

/// Admits original source capabilities using only compiled rules and a live,
/// durably accepted public policy. Complete CRX/tree evidence is consumed.
/// `revision` is a local Store sequence, not a lossy conversion of Chromium's
/// four-component version. The eventual Store transaction must recheck both
/// that sequence and `previous_upstream` against current durable state.
pub fn admit_beta_source(
    policy: Arc<AcceptedExtensionPolicy>,
    source: AcquiredExtensionTreeReceipt,
    manifest_bytes: &[u8],
    runtime: BetaRuntimeTarget,
    revision: ExtensionPackageRevision,
    previous_upstream: Option<ExtensionUpstreamCheckpoint>,
) -> Result<ProductAdmittedBetaSource, BetaSourceAdmissionError> {
    admit_source(
        SourcePolicy::Signed(policy),
        source,
        manifest_bytes,
        runtime,
        revision,
        previous_upstream,
    )
}

// Select implementations from declared capabilities, never extension names/IDs.
fn local_rules(
    runtime: BetaRuntimeTarget,
    bytes: &[u8],
    publisher: [u8; 32],
    compose_bounded: bool,
    history_v2: bool,
    history_v3: bool,
    pinned_history_recipe: bool,
    candidate_v2: bool,
) -> Result<compatibility::CompiledBetaPolicy, BetaSourceAdmissionError> {
    let root = zephium_extension_package::parse_bounded_json(
        bytes,
        zephium_extension_package::BoundedJsonLimits::extension_manifest(),
    )
    .map_err(|_| BetaSourceAdmissionError::SourceMismatch)?
    .into_value();
    let declares = |name: &str| {
        root.get("permissions")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|permissions| permissions.iter().any(|value| value.as_str() == Some(name)))
    };
    let macos = runtime == BetaRuntimeTarget::MacosNative;
    let bounded = ["permissions", "optional_permissions"]
        .into_iter()
        .any(|field| {
            root.get(field)
                .and_then(serde_json::Value::as_array)
                .is_some_and(|permissions| {
                    permissions
                        .iter()
                        .any(|value| value.as_str() == Some("unlimitedStorage"))
                })
        });
    if bounded && !compose_bounded && !candidate_v2 {
        // Preserve the exact recipe of previously admitted bounded packages.
        // A bounded, package-neutral native subset. Other missing capabilities
        // still reject; this does not combine unrelated adapters speculatively.
        return Ok(
            compatibility::CompiledBetaPolicy::for_external(runtime).with_bounded_storage(true)
        );
    }
    let history = declares("history");
    // New admissions retain the released history recipe whenever its source
    // prerequisites are present. Reopened v1/v2/v3 artifacts always replay
    // their pinned recipe, independent of what a newer release can compile.
    let brokered = macos
        && history
        && (pinned_history_recipe
            || (declares("storage")
                && root.get("background").is_some()
                && !declares("nativeMessaging")));
    let capability_broker =
        macos && !brokered && (history || declares("search") || declares("sessions"));
    let adapted = macos
        && !brokered
        && !capability_broker
        && (root
            .pointer("/background/type")
            .and_then(serde_json::Value::as_str)
            == Some("module")
            || root.pointer("/background/scripts").is_some()
            || root.pointer("/storage/managed_schema").is_some()
            || root
                .pointer("/content_security_policy/extension_pages")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|csp| csp.contains("'wasm-unsafe-eval'"))
            || [
                "nativeMessaging",
                "privacy",
                "notifications",
                "webNavigation",
                "cookies",
                "downloads",
                "idle",
                "management",
                "webRequest",
                "webRequestAuthProvider",
                "declarativeNetRequest",
                "declarativeNetRequestWithHostAccess",
                "declarativeNetRequestFeedback",
            ]
            .into_iter()
            .any(declares));
    let has_globs = root
        .get("content_scripts")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|scripts| {
            scripts.iter().any(|script| {
                script.get("include_globs").is_some() || script.get("exclude_globs").is_some()
            })
        });
    let main_document_globs = macos
        && !brokered
        && !capability_broker
        && has_globs
        && declares("scripting")
        && !declares("nativeMessaging")
        && root
            .pointer("/background/service_worker")
            .and_then(serde_json::Value::as_str)
            .is_some();
    let capabilities_v2 = candidate_v2
        && macos
        && declares("offscreen")
        && !declares("nativeMessaging")
        && root
            .pointer("/background/service_worker")
            .and_then(serde_json::Value::as_str)
            .is_some();
    Ok(compatibility::CompiledBetaPolicy::for_external(runtime)
        .with_brokered(brokered)
        .with_capability_broker(capability_broker)
        .with_history_v2(history_v2)
        .with_history_v3(history_v3)
        .with_adapted(
            adapted,
            declares("nativeMessaging") && native_hosts::supports(publisher),
        )
        .with_identity(
            macos
                && !brokered
                && !capability_broker
                && declares("identity")
                && !declares("nativeMessaging"),
        )
        .with_main_document_globs(main_document_globs)
        .with_capabilities_v2(capabilities_v2)
        .with_bounded_storage(bounded))
}

fn admit_source(
    policy: SourcePolicy,
    source: AcquiredExtensionTreeReceipt,
    manifest_bytes: &[u8],
    runtime: BetaRuntimeTarget,
    revision: ExtensionPackageRevision,
    previous_upstream: Option<ExtensionUpstreamCheckpoint>,
) -> Result<ProductAdmittedBetaSource, BetaSourceAdmissionError> {
    if matches!(
        policy,
        SourcePolicy::Local
            | SourcePolicy::LocalCapabilitiesV2Candidate
            | SourcePolicy::LocalLegacyHistory
            | SourcePolicy::LocalHistoryV2
            | SourcePolicy::LocalHistoryV3
    ) {
        let document = zephium_extension_package::parse_bounded_json(
            manifest_bytes,
            zephium_extension_package::BoundedJsonLimits::extension_manifest(),
        )
        .map_err(|_| BetaSourceAdmissionError::SourceMismatch)?
        .into_value();
        if document.get("update_url").is_some_and(|value| {
            value.as_str() != Some("https://clients2.google.com/service/update2/crx")
        }) {
            return Err(BetaSourceAdmissionError::SourceMismatch);
        }
    }
    let channel = match &policy {
        SourcePolicy::Signed(policy) => {
            let authenticated = policy
                .policy()
                .map_err(|_| BetaSourceAdmissionError::PolicyUnavailable)?;
            check_policy(authenticated, runtime, &source)?;
            match authenticated.channel() {
                ExtensionPolicyChannel::Stable => {
                    zephium_core::extensions::ExtensionBetaChannel::Stable
                }
                ExtensionPolicyChannel::Staging => {
                    zephium_core::extensions::ExtensionBetaChannel::Staging
                }
            }
        }
        SourcePolicy::Local
        | SourcePolicy::LocalCapabilitiesV2Candidate
        | SourcePolicy::LocalLegacyHistory
        | SourcePolicy::LocalHistoryV2
        | SourcePolicy::LocalHistoryV3 => zephium_core::extensions::ExtensionBetaChannel::Local,
    };
    let upstream = source
        .upstream_checkpoint(manifest_bytes)
        .map_err(|_| BetaSourceAdmissionError::SourceMismatch)?;
    if let Some(previous) = previous_upstream {
        if !matches!(
            previous.classify(upstream),
            zephium_core::extensions::ExtensionUpstreamUpdateDisposition::Unchanged
                | zephium_core::extensions::ExtensionUpstreamUpdateDisposition::Advance
        ) {
            return Err(BetaSourceAdmissionError::UpstreamRollback);
        }
    }
    // Separate domain, channel, backend and full publisher key. No catalog
    // row or reviewed authority identifier is synthesized or reused.
    let authority = runtime.authority(channel);
    let package = ExtensionPackageIdentity::new(
        authority,
        ExtensionPackageKey::from_bytes(source.developer_key_sha256().bytes()),
        revision,
        source.payload_identity(),
        source.index().manifest_sha256(),
        source.index().tree_sha256(),
    );
    let mut rules = match &policy {
        SourcePolicy::Signed(_) => compatibility::CompiledBetaPolicy::new(runtime),
        SourcePolicy::Local
        | SourcePolicy::LocalCapabilitiesV2Candidate
        | SourcePolicy::LocalLegacyHistory
        | SourcePolicy::LocalHistoryV2
        | SourcePolicy::LocalHistoryV3 => local_rules(
            runtime,
            manifest_bytes,
            source.developer_key_sha256().bytes(),
            false,
            matches!(policy, SourcePolicy::Local | SourcePolicy::LocalHistoryV2),
            matches!(policy, SourcePolicy::Local | SourcePolicy::LocalHistoryV3),
            !matches!(
                policy,
                SourcePolicy::Local | SourcePolicy::LocalCapabilitiesV2Candidate
            ),
            matches!(policy, SourcePolicy::LocalCapabilitiesV2Candidate),
        )?,
    };
    let mut manifest = assess_upstream_extension_manifest(
        &package,
        source.index(),
        &ExpectedChromiumIdentity::from_manifest_key_digest(source.developer_key_sha256()),
        manifest_bytes,
        &rules,
    )
    .map_err(|_| BetaSourceAdmissionError::SourceMismatch)?;
    let mut withheld = if matches!(
        policy,
        SourcePolicy::Local
            | SourcePolicy::LocalCapabilitiesV2Candidate
            | SourcePolicy::LocalLegacyHistory
            | SourcePolicy::LocalHistoryV2
            | SourcePolicy::LocalHistoryV3
    ) {
        withheld::WithheldFeatures::plan(&manifest, manifest_bytes)?
    } else {
        Default::default()
    };
    let mut unsupported = unsupported_features(&manifest, &withheld);
    if matches!(
        policy,
        SourcePolicy::Local
            | SourcePolicy::LocalCapabilitiesV2Candidate
            | SourcePolicy::LocalLegacyHistory
            | SourcePolicy::LocalHistoryV2
            | SourcePolicy::LocalHistoryV3
    ) && !unsupported.declarations().is_empty()
    {
        // Try composition only after the historical recipe refuses. Working
        // installations keep the same target, bytes, digest and reopen path.
        rules = local_rules(
            runtime,
            manifest_bytes,
            source.developer_key_sha256().bytes(),
            true,
            matches!(
                policy,
                SourcePolicy::Local | SourcePolicy::LocalHistoryV2 | SourcePolicy::LocalHistoryV3
            ),
            matches!(policy, SourcePolicy::Local | SourcePolicy::LocalHistoryV3),
            !matches!(
                policy,
                SourcePolicy::Local | SourcePolicy::LocalCapabilitiesV2Candidate
            ),
            matches!(policy, SourcePolicy::LocalCapabilitiesV2Candidate),
        )?;
        manifest = assess_upstream_extension_manifest(
            &package,
            source.index(),
            &ExpectedChromiumIdentity::from_manifest_key_digest(source.developer_key_sha256()),
            manifest_bytes,
            &rules,
        )
        .map_err(|_| BetaSourceAdmissionError::SourceMismatch)?;
        withheld = withheld::WithheldFeatures::plan(&manifest, manifest_bytes)?;
        unsupported = unsupported_features(&manifest, &withheld);
    }
    if !unsupported.declarations().is_empty() {
        return Err(BetaSourceAdmissionError::Unsupported(unsupported));
    }
    let limitations = rules.limitations(&manifest);
    let admitted = ProductAdmittedBetaSource {
        policy,
        source,
        manifest,
        upstream,
        previous: previous_upstream,
        runtime,
        limitations,
        withheld,
    };
    if admitted.retained_bytes() > MAX_BETA_SOURCE_RETAINED_BYTES {
        return Err(BetaSourceAdmissionError::Capacity);
    }
    admitted.revalidate()?;
    Ok(admitted)
}

fn check_policy(
    policy: &ExtensionPublicPolicy,
    runtime: BetaRuntimeTarget,
    source: &AcquiredExtensionTreeReceipt,
) -> Result<(), BetaSourceAdmissionError> {
    if !policy.beta_targets().iter().any(|entry| {
        entry.target() == runtime.target_id()
            && entry.policy_version() == BETA_SOURCE_POLICY_VERSION
    }) {
        return Err(BetaSourceAdmissionError::TargetNotEnabled);
    }
    let key = hex(source.developer_key_sha256().bytes());
    let crx = hex(source.original_crx_sha256());
    if policy.revocations().iter().any(|entry| {
        entry.extension_id() == source.extension_id().as_str()
            && entry.developer_key_sha256() == key
            && entry
                .original_crx_sha256()
                .is_none_or(|revoked| revoked == crx)
    }) {
        return Err(BetaSourceAdmissionError::Revoked);
    }
    Ok(())
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unsupported_features(
    manifest: &AdmittedExtensionManifest,
    withheld: &withheld::WithheldFeatures,
) -> zephium_core::ports::extensions::ExtensionUnsupportedFeatures {
    zephium_core::ports::extensions::ExtensionUnsupportedFeatures::new(
        manifest
            .descriptor()
            .compatibility()
            .iter()
            .filter(|row| {
                matches!(
                    row.level(),
                    ExtensionCompatibilityLevel::Unsupported
                        | ExtensionCompatibilityLevel::Unassessed
                )
            })
            .filter(|row| !withheld.removes(row.declaration(), manifest))
            .map(|row| row.declaration().clone()),
    )
}
