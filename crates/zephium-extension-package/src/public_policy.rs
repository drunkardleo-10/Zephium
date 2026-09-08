//! Shared public metadata; parsing never grants Verified or Beta authority.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zephium_core::extensions::{ExtensionCompatibilityTargetId, ExtensionUpstreamVersion};

use crate::{
    parse_bounded_json, BoundedJsonLimits, ChromiumExtensionId, ChromiumManifestKeyDigest,
};

/// Fixed TUF target fetched by every client, independent of installed packages.
pub const EXTENSION_PUBLIC_POLICY_TARGET: &str = "extension-policy-v1.json";
/// Largest shared policy payload admitted before parsing.
pub const MAX_EXTENSION_PUBLIC_POLICY_BYTES: usize = 256 * 1024;
/// Largest recommendation or revocation cohort in this schema.
pub const MAX_EXTENSION_PUBLIC_POLICY_ENTRIES: usize = 256;
/// Largest signed policy lifetime; clients still enforce TUF role expiries.
pub const MAX_EXTENSION_PUBLIC_POLICY_LIFETIME_SECONDS: u64 = 7 * 24 * 60 * 60;

/// Release trust channel, independent of an extension's compatibility tier.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionPolicyChannel {
    /// Public shipping metadata.
    Stable,
    /// Separately signed non-shipping integration metadata.
    Staging,
}

/// One remote opt-in to an already compiled Beta policy. It cannot define APIs
/// or executable adapters; an unknown target/version is never runnable.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExtensionBetaTargetPolicy {
    target: String,
    policy_version: u32,
}

impl ExtensionBetaTargetPolicy {
    /// Returns the exact native target token.
    pub fn target(&self) -> &str {
        &self.target
    }
    /// Returns the compiled policy revision requested by this record.
    pub const fn policy_version(&self) -> u32 {
        self.policy_version
    }
}

/// Historical workflow evidence for one exact package/artifact/runtime.
/// It does not promote any package to the existing Verified manifest authority.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExtensionTestedVersion {
    upstream_version: String,
    original_crx_sha256: String,
    runtime_target: String,
    transform_id: String,
    transformed_tree_sha256: String,
    zephium_version: String,
    platform_runtime: String,
    tested_unix: u64,
    workflows: Vec<String>,
    limitations: Vec<String>,
}

impl ExtensionTestedVersion {
    /// Original upstream version text, not a claim about newer versions.
    pub fn upstream_version(&self) -> &str {
        &self.upstream_version
    }
    /// Exact original CRX identity used in this test campaign.
    pub fn original_crx_sha256(&self) -> &str {
        &self.original_crx_sha256
    }
    /// Exact native backend used in the campaign.
    pub fn runtime_target(&self) -> &str {
        &self.runtime_target
    }
    /// Exact compiled adaptation, or `identity.v1` for unmodified content.
    pub fn transform_id(&self) -> &str {
        &self.transform_id
    }
    /// Exact tested native tree, distinct from original package identity.
    pub fn transformed_tree_sha256(&self) -> &str {
        &self.transformed_tree_sha256
    }
    /// Browser build version used in the campaign.
    pub fn zephium_version(&self) -> &str {
        &self.zephium_version
    }
    /// OS and engine runtime evidence label.
    pub fn platform_runtime(&self) -> &str {
        &self.platform_runtime
    }
    /// Time of the completed test campaign.
    pub const fn tested_unix(&self) -> u64 {
        self.tested_unix
    }
    /// Bounded stable workflow labels exercised by the campaign.
    pub fn workflows(&self) -> &[String] {
        &self.workflows
    }
    /// Bounded plain-text limitations, always rendered as text.
    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }
}

/// A recommended publisher identity with exact, separately scoped test history.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExtensionRecommendation {
    extension_id: String,
    developer_key_sha256: String,
    tested_versions: Vec<ExtensionTestedVersion>,
}

impl ExtensionRecommendation {
    /// Canonical Chromium id used to derive its real store listing.
    pub fn extension_id(&self) -> &str {
        &self.extension_id
    }
    /// Complete publisher key digest, not just its truncated Chromium id.
    pub fn developer_key_sha256(&self) -> &str {
        &self.developer_key_sha256
    }
    /// Exact evidence; newer upstream packages never inherit these records.
    pub fn tested_versions(&self) -> &[ExtensionTestedVersion] {
        &self.tested_versions
    }
}

/// Signed disablement of one original CRX or every package from an exact key.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExtensionPolicyRevocation {
    extension_id: String,
    developer_key_sha256: String,
    // Missing must not silently become a publisher-wide revocation.
    #[serde(deserialize_with = "required_nullable_digest")]
    original_crx_sha256: Option<String>,
    reason: String,
}

impl ExtensionPolicyRevocation {
    /// Canonical extension id.
    pub fn extension_id(&self) -> &str {
        &self.extension_id
    }
    /// Exact publisher identity.
    pub fn developer_key_sha256(&self) -> &str {
        &self.developer_key_sha256
    }
    /// `None` revokes all versions for the exact publisher identity.
    pub fn original_crx_sha256(&self) -> Option<&str> {
        self.original_crx_sha256.as_deref()
    }
    /// Bounded plain-text reason, never HTML or a navigation target.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct PolicyDocument {
    schema_version: u32,
    policy_revision: u64,
    channel: ExtensionPolicyChannel,
    issued_unix: u64,
    expires_unix: u64,
    beta_targets: Vec<ExtensionBetaTargetPolicy>,
    recommendations: Vec<ExtensionRecommendation>,
    revocations: Vec<ExtensionPolicyRevocation>,
}

/// Bounded structural policy. Requires authenticated TUF target bytes and a
/// durable policy/time checkpoint before a separate Beta authority may use it.
#[derive(Clone, Debug)]
pub struct ExtensionPublicPolicy {
    document: PolicyDocument,
    sha256: [u8; 32],
}

/// URL-free public metadata rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionPublicPolicyError {
    /// Malformed, duplicate-key, oversized, or unsupported JSON.
    Structure,
    /// Invalid version, channel-independent sequence, or validity period.
    Header,
    /// Invalid, duplicated, or unbounded policy/evidence rows.
    Records,
}

impl std::fmt::Display for ExtensionPublicPolicyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "extension public policy rejected: {self:?}")
    }
}
impl std::error::Error for ExtensionPublicPolicyError {}

impl ExtensionPublicPolicy {
    /// Parses shared JSON with duplicate-key and allocation bounds. TUF
    /// authenticates exact bytes, so this format does not require JSON key
    /// order or whitespace canonicalization by the backend.
    pub fn parse(bytes: &[u8]) -> Result<Self, ExtensionPublicPolicyError> {
        use ExtensionPublicPolicyError::{Header, Records, Structure};
        if bytes.len() > MAX_EXTENSION_PUBLIC_POLICY_BYTES {
            return Err(Structure);
        }
        let bounded = parse_bounded_json(bytes, BoundedJsonLimits::public_extension_policy())
            .map_err(|_| Structure)?;
        let document: PolicyDocument =
            serde_json::from_value(bounded.into_value()).map_err(|_| Structure)?;
        if document.schema_version != 1
            || document.policy_revision == 0
            || document.policy_revision > i64::MAX as u64
            || document.issued_unix == 0
            || document.expires_unix > 7_258_118_400
            || document.expires_unix <= document.issued_unix
            || document.expires_unix - document.issued_unix
                > MAX_EXTENSION_PUBLIC_POLICY_LIFETIME_SECONDS
        {
            return Err(Header);
        }
        if document.beta_targets.len() > 8
            || document.recommendations.len() > MAX_EXTENSION_PUBLIC_POLICY_ENTRIES
            || document.revocations.len() > MAX_EXTENSION_PUBLIC_POLICY_ENTRIES
        {
            return Err(Records);
        }
        let mut targets = std::collections::BTreeSet::new();
        for target in &document.beta_targets {
            if ExtensionCompatibilityTargetId::parse_exact(&target.target).is_err()
                || target.policy_version == 0
                || !targets.insert(&target.target)
            {
                return Err(Records);
            }
        }
        let mut recommended = std::collections::BTreeSet::new();
        for row in &document.recommendations {
            if !valid_identity(&row.extension_id, &row.developer_key_sha256)
                || !recommended.insert(&row.extension_id)
                || row.tested_versions.is_empty()
                || row.tested_versions.len() > 16
            {
                return Err(Records);
            }
            let mut tested = std::collections::BTreeSet::new();
            for test in &row.tested_versions {
                if ExtensionUpstreamVersion::parse(&test.upstream_version).is_none()
                    || digest(&test.original_crx_sha256).is_none()
                    || digest(&test.transformed_tree_sha256).is_none()
                    || ExtensionCompatibilityTargetId::parse_exact(&test.runtime_target).is_err()
                    || ExtensionCompatibilityTargetId::parse_exact(&test.transform_id).is_err()
                    || !bounded_text(&test.zephium_version, 128)
                    || !bounded_text(&test.platform_runtime, 128)
                    || test.tested_unix == 0
                    || test.tested_unix > document.issued_unix
                    || test.workflows.is_empty()
                    || test.workflows.len() > 32
                    || test.limitations.len() > 32
                    || test.workflows.iter().any(|value| !bounded_text(value, 128))
                    || test
                        .limitations
                        .iter()
                        .any(|value| !bounded_text(value, 512))
                    || !tested.insert((
                        &test.original_crx_sha256,
                        &test.runtime_target,
                        &test.transform_id,
                        &test.zephium_version,
                        &test.platform_runtime,
                    ))
                {
                    return Err(Records);
                }
            }
        }
        let mut revoked = std::collections::BTreeSet::new();
        for row in &document.revocations {
            if !valid_identity(&row.extension_id, &row.developer_key_sha256)
                || row
                    .original_crx_sha256
                    .as_deref()
                    .is_some_and(|value| digest(value).is_none())
                || !bounded_text(&row.reason, 512)
                || !revoked.insert((&row.extension_id, &row.original_crx_sha256))
            {
                return Err(Records);
            }
        }
        Ok(Self {
            document,
            sha256: Sha256::digest(bytes).into(),
        })
    }

    /// Exact payload digest, including its JSON formatting.
    pub const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
    /// Strictly positive signed policy sequence.
    pub const fn revision(&self) -> u64 {
        self.document.policy_revision
    }
    /// Release channel to compare with the compiled trust domain.
    pub const fn channel(&self) -> ExtensionPolicyChannel {
        self.document.channel
    }
    /// Earliest time this policy may be admitted.
    pub const fn issued_unix(&self) -> u64 {
        self.document.issued_unix
    }
    /// Expiry; HTTP cache responses cannot extend this value.
    pub const fn expires_unix(&self) -> u64 {
        self.document.expires_unix
    }
    /// Remote opt-ins; runnable support is limited by compiled backend policy.
    pub fn beta_targets(&self) -> &[ExtensionBetaTargetPolicy] {
        &self.document.beta_targets
    }
    /// Publisher recommendations and historical tests.
    pub fn recommendations(&self) -> &[ExtensionRecommendation] {
        &self.document.recommendations
    }
    /// Revocations override recommendations.
    pub fn revocations(&self) -> &[ExtensionPolicyRevocation] {
        &self.document.revocations
    }
    /// Checks validity using the caller's rollback-protected trusted time.
    pub const fn is_fresh_at(&self, trusted_unix: u64) -> bool {
        trusted_unix >= self.issued_unix() && trusted_unix < self.expires_unix()
    }
}

fn digest(value: &str) -> Option<[u8; 32]> {
    crate::digest::decode_lower_hex_32(value).ok()
}

fn required_nullable_digest<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}
fn valid_identity(id: &str, key: &str) -> bool {
    let (Ok(id), Some(key)) = (ChromiumExtensionId::parse(id), digest(key)) else {
        return false;
    };
    ChromiumManifestKeyDigest::from_bytes(key).derived_extension_id() == id
}
fn bounded_text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.chars().all(|character| !character.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> serde_json::Value {
        serde_json::from_slice(include_bytes!(
            "../fixtures/public-policy/empty-staging.json"
        ))
        .unwrap()
    }
    fn parse(
        value: &serde_json::Value,
    ) -> Result<ExtensionPublicPolicy, ExtensionPublicPolicyError> {
        ExtensionPublicPolicy::parse(&serde_json::to_vec(value).unwrap())
    }
    #[test]
    fn accepts_empty_disabled_policy_and_preserves_exact_digest() {
        let bytes = serde_json::to_vec_pretty(&empty()).unwrap();
        let policy = ExtensionPublicPolicy::parse(&bytes).unwrap();
        assert!(policy.beta_targets().is_empty());
        assert!(!policy.is_fresh_at(99));
        assert!(policy.is_fresh_at(100));
        assert!(!policy.is_fresh_at(200));
        assert_eq!(policy.sha256(), <[u8; 32]>::from(Sha256::digest(&bytes)));
    }
    #[test]
    fn rejects_duplicate_keys_unknown_fields_invalid_time_and_revision() {
        let duplicate =
            serde_json::to_string(&empty())
                .unwrap()
                .replacen('{', "{\"schema_version\":1,", 1);
        assert!(ExtensionPublicPolicy::parse(duplicate.as_bytes()).is_err());
        for (field, value) in [
            ("schema_version", serde_json::json!(2)),
            ("policy_revision", serde_json::json!(0)),
            ("expires_unix", serde_json::json!(100)),
            ("channel", serde_json::json!("beta")),
            ("execute", serde_json::json!("native")),
        ] {
            let mut document = empty();
            document[field] = value;
            assert!(parse(&document).is_err(), "{field}");
        }
        let mut document = empty();
        document["expires_unix"] =
            serde_json::json!(101 + MAX_EXTENSION_PUBLIC_POLICY_LIFETIME_SECONDS);
        assert!(parse(&document).is_err());
    }
    #[test]
    fn publisher_keys_must_derive_the_declared_id_and_revocations_are_bounded() {
        let mut document = empty();
        let row = serde_json::json!({"extension_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","developer_key_sha256":"00".repeat(32),"original_crx_sha256":null,"reason":"Compromised publisher"});
        document["revocations"] = serde_json::json!([row.clone()]);
        assert!(parse(&document).is_ok());
        document["revocations"][0]
            .as_object_mut()
            .unwrap()
            .remove("original_crx_sha256");
        assert!(parse(&document).is_err());
        document["revocations"] = serde_json::json!([row.clone()]);
        document["revocations"][0]["developer_key_sha256"] = serde_json::json!("11".repeat(32));
        assert!(parse(&document).is_err());
        document["revocations"] = serde_json::json!([row.clone(), row]);
        assert!(parse(&document).is_err());
    }
    #[test]
    fn rejects_duplicate_targets_and_cannot_receive_remote_executable_policy() {
        let mut document = empty();
        let row = serde_json::json!({"target":"macos.wkwebextension.v1","policy_version":1});
        document["beta_targets"] = serde_json::json!([row.clone()]);
        assert!(parse(&document).is_ok());
        document["beta_targets"] = serde_json::json!([row.clone(), row]);
        assert!(parse(&document).is_err());
        document["beta_targets"] = serde_json::json!([{"target":"macos.wkwebextension.v1","policy_version":1,"script":"nativeAccess()"}]);
        assert!(parse(&document).is_err());
    }

    #[test]
    fn recommendation_evidence_is_exact_bounded_and_never_implies_a_new_version() {
        let mut document = empty();
        let record = serde_json::json!({
            "upstream_version":"1.10", "original_crx_sha256":"11".repeat(32),
            "runtime_target":"macos.wkwebextension.v1", "transform_id":"identity.v1",
            "transformed_tree_sha256":"22".repeat(32), "zephium_version":"0.1.0",
            "platform_runtime":"macOS fixture", "tested_unix":99,
            "workflows":["keyboard navigation"], "limitations":["Fixture evidence only"]
        });
        document["recommendations"] = serde_json::json!([{
            "extension_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "developer_key_sha256":"00".repeat(32),
            "tested_versions":[record.clone()]
        }]);
        let policy = parse(&document).unwrap();
        let evidence = &policy.recommendations()[0].tested_versions()[0];
        assert_eq!(evidence.upstream_version(), "1.10");
        assert_eq!(evidence.original_crx_sha256(), "11".repeat(32));
        document["recommendations"][0]["tested_versions"] =
            serde_json::json!([record.clone(), record.clone()]);
        assert!(parse(&document).is_err());
        for (field, value) in [
            ("upstream_version", serde_json::json!("1.01")),
            ("original_crx_sha256", serde_json::json!("AA".repeat(32))),
            ("tested_unix", serde_json::json!(101)),
            ("workflows", serde_json::json!([])),
            ("limitations", serde_json::json!(["unsafe\ntext"])),
            ("execute", serde_json::json!("native()")),
        ] {
            document["recommendations"][0]["tested_versions"] = serde_json::json!([record.clone()]);
            document["recommendations"][0]["tested_versions"][0][field] = value;
            assert!(parse(&document).is_err(), "{field}");
        }
    }

    #[test]
    fn oversized_payloads_and_cohorts_fail_before_becoming_policy() {
        assert!(
            ExtensionPublicPolicy::parse(&vec![b' '; MAX_EXTENSION_PUBLIC_POLICY_BYTES + 1])
                .is_err()
        );
        let mut document = empty();
        document["beta_targets"] = serde_json::json!((0..9).map(|index| serde_json::json!({"target":format!("fixture.runtime{index}.v1"),"policy_version":1})).collect::<Vec<_>>());
        assert!(parse(&document).is_err());
    }
}
