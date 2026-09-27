use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tough::schema::Role;
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, ExtensionPolicyChannel, ExtensionPublicPolicy,
};

use super::ExtensionPolicyError;

pub(super) const MAX_CHECKPOINT_BYTES: usize = 20 * 1024;

/// Structural durable high-water state. Decoding does not authenticate policy
/// or grant install authority. Store owners must persist this atomically with
/// the exact authenticated target and reject a mismatching prior checkpoint.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionPolicyCheckpoint {
    schema: u32,
    trust_sha256: [u8; 32],
    channel: ExtensionPolicyChannel,
    roles: [RoleCheckpoint; 4],
    policy_revision: u64,
    policy_sha256: [u8; 32],
    pub(super) trusted_unix: u64,
    publisher_revocations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RoleCheckpoint {
    version: u64,
    // Digest the canonical signed role, not the randomized signature envelope.
    sha256: [u8; 32],
}

pub(super) fn role<T: Role>(role: &T) -> Result<RoleCheckpoint, ExtensionPolicyError> {
    Ok(RoleCheckpoint {
        version: role.version().get(),
        sha256: Sha256::digest(
            role.canonical_form()
                .map_err(|_| ExtensionPolicyError::Authentication)?,
        )
        .into(),
    })
}

impl ExtensionPolicyCheckpoint {
    pub(super) fn matches_policy(&self, bytes: &[u8]) -> Result<(), ExtensionPolicyError> {
        let policy =
            ExtensionPublicPolicy::parse(bytes).map_err(|_| ExtensionPolicyError::Checkpoint)?;
        if policy.sha256() != self.policy_sha256
            || policy.revision() != self.policy_revision
            || policy.channel() != self.channel
        {
            return Err(ExtensionPolicyError::Checkpoint);
        }
        // Includes exact publisher-revocation preservation and validates that
        // the checkpoint's recorded set equals this authenticated payload.
        let reproduced = Self::candidate(
            self.trust_sha256,
            self.channel,
            self.roles.clone(),
            &policy,
            self.trusted_unix,
            None,
        )?;
        if &reproduced != self {
            return Err(ExtensionPolicyError::Checkpoint);
        }
        Ok(())
    }
    /// Encodes bounded, versioned structural state for durable storage.
    pub fn encode(&self) -> Result<Vec<u8>, ExtensionPolicyError> {
        let bytes = serde_json::to_vec(self).map_err(|_| ExtensionPolicyError::Checkpoint)?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(ExtensionPolicyError::Checkpoint);
        }
        Ok(bytes)
    }

    /// Reads structural state with duplicate-key and allocation limits. The
    /// compiled trust domain is checked separately on every refresh.
    pub fn decode(bytes: &[u8]) -> Result<Self, ExtensionPolicyError> {
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(ExtensionPolicyError::Checkpoint);
        }
        let value = parse_bounded_json(bytes, BoundedJsonLimits::public_extension_policy())
            .map_err(|_| ExtensionPolicyError::Checkpoint)?;
        let state: Self = serde_json::from_value(value.into_value())
            .map_err(|_| ExtensionPolicyError::Checkpoint)?;
        if state.schema != 1
            || state.policy_revision == 0
            || state.policy_revision > i64::MAX as u64
            || state.trusted_unix == 0
            || state.trusted_unix > 7_258_118_400
            || state
                .roles
                .iter()
                .any(|role| role.version == 0 || role.version > i64::MAX as u64)
            || state.publisher_revocations.len() > 256
            || state.publisher_revocations.iter().any(|key| {
                key.len() != 64
                    || !key
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            || state
                .publisher_revocations
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(ExtensionPolicyError::Checkpoint);
        }
        Ok(state)
    }

    pub(super) fn validate_domain(
        &self,
        trust: [u8; 32],
        channel: ExtensionPolicyChannel,
        now: u64,
    ) -> Result<(), ExtensionPolicyError> {
        if self.trust_sha256 != trust || self.channel != channel {
            return Err(ExtensionPolicyError::Checkpoint);
        }
        if now < self.trusted_unix {
            return Err(ExtensionPolicyError::Time);
        }
        Ok(())
    }

    pub(super) fn candidate(
        trust_sha256: [u8; 32],
        channel: ExtensionPolicyChannel,
        roles: [RoleCheckpoint; 4],
        policy: &ExtensionPublicPolicy,
        trusted_unix: u64,
        previous: Option<&Self>,
    ) -> Result<Self, ExtensionPolicyError> {
        let mut publisher_revocations = policy
            .revocations()
            .iter()
            .filter(|row| row.original_crx_sha256().is_none())
            .map(|row| row.developer_key_sha256().to_owned())
            .collect::<Vec<_>>();
        publisher_revocations.sort_unstable();
        let candidate = Self {
            schema: 1,
            trust_sha256,
            channel,
            roles,
            policy_revision: policy.revision(),
            policy_sha256: policy.sha256(),
            trusted_unix,
            publisher_revocations,
        };
        if let Some(previous) = previous {
            previous.validate_domain(trust_sha256, channel, trusted_unix)?;
            if candidate.policy_revision < previous.policy_revision
                || (candidate.policy_revision == previous.policy_revision
                    && candidate.policy_sha256 != previous.policy_sha256)
                || candidate
                    .roles
                    .iter()
                    .zip(&previous.roles)
                    .any(|(new, old)| {
                        new.version < old.version
                            || (new.version == old.version && new.sha256 != old.sha256)
                    })
            {
                return Err(ExtensionPolicyError::Rollback);
            }
            if previous
                .publisher_revocations
                .iter()
                .any(|key| candidate.publisher_revocations.binary_search(key).is_err())
            {
                return Err(ExtensionPolicyError::RevocationRemoved);
            }
        }
        // Ensure every accepted checkpoint can be losslessly persisted and read.
        Self::decode(&candidate.encode()?)?;
        Ok(candidate)
    }
}
