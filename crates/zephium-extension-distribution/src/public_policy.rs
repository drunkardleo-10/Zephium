//! Signed shared extension policy acquisition. Results are candidates for a
//! durable compare-and-swap, never Verified, Beta, installation, or native
//! authority. The shipping trust slot is intentionally empty until provisioned.

mod cache;
mod checkpoint;
#[cfg(test)]
pub(crate) mod tests;
mod transport;

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use tough::schema::Role;
use tough::{ExpirationEnforcement, IntoVec, Limits, RepositoryLoader, TargetName};
use zephium_extension_package::{
    ExtensionPolicyChannel, ExtensionPublicPolicy, EXTENSION_PUBLIC_POLICY_TARGET,
    MAX_EXTENSION_PUBLIC_POLICY_BYTES,
};
use zephium_update_transport::FixedOriginTransport;

pub use cache::{AcceptedExtensionPolicy, ExtensionPolicyCache};
pub use checkpoint::ExtensionPolicyCheckpoint;
use transport::{PolicyTransport, Source, MAX_METADATA_BYTES, MAX_ROOT_UPDATES};

/// Stable diagnostics, without URLs, server text, extension inventory, or keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ExtensionPolicyError {
    /// No reviewed root is compiled for this channel.
    #[error("extension policy trust is not provisioned")]
    TrustUnavailable,
    /// The bounded metadata, signature chain, or target failed verification.
    #[error("extension policy authentication failed")]
    Authentication,
    /// The policy is stale, not yet valid, or the wall clock moved backwards.
    #[error("extension policy time validation failed")]
    Time,
    /// Policy or role versions regressed, or an existing version changed.
    #[error("extension policy rollback or equivocation detected")]
    Rollback,
    /// A persisted checkpoint is malformed or belongs to another trust domain.
    #[error("extension policy checkpoint is invalid")]
    Checkpoint,
    /// A previously published publisher-wide revocation disappeared.
    #[error("extension publisher revocation was removed")]
    RevocationRemoved,
    /// Private storage is unavailable, corrupt, or requires reopening after failure.
    #[error("extension policy storage is unavailable")]
    Storage,
    /// Another accepted update advanced the exact durable state.
    #[error("extension policy candidate is stale")]
    StaleCandidate,
    /// This client already owns one bounded refresh operation.
    #[error("extension policy refresh is already running")]
    Busy,
}

/// Fully signature-verified candidate awaiting atomic durable acceptance.
/// It cannot be converted into either existing Verified or future Beta authority.
#[derive(Debug)]
#[must_use = "persist with an exact prior-checkpoint comparison before admitting new installs"]
pub struct ExtensionPolicyCandidate {
    bytes: Box<[u8]>,
    policy: ExtensionPublicPolicy,
    previous: Option<ExtensionPolicyCheckpoint>,
    checkpoint: ExtensionPolicyCheckpoint,
    expires_unix: u64,
    fresh_until: Instant,
}

impl ExtensionPolicyCandidate {
    /// Exact authenticated target bytes, for an atomic cache/checkpoint write.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Exact expected durable state. A concurrent advance must reject this candidate.
    pub fn previous_checkpoint(&self) -> Option<&ExtensionPolicyCheckpoint> {
        self.previous.as_ref()
    }

    /// Proposed high-water state, which must be committed with the target bytes.
    pub fn checkpoint(&self) -> &ExtensionPolicyCheckpoint {
        &self.checkpoint
    }

    /// Returns structural policy only while all signed deadlines remain fresh.
    /// The caller supplies its rollback-protected current time; this is not an
    /// installation witness and a successful read does not commit the candidate.
    pub fn policy_at(
        &self,
        trusted_unix: u64,
    ) -> Result<&ExtensionPublicPolicy, ExtensionPolicyError> {
        if trusted_unix < self.checkpoint.trusted_unix
            || trusted_unix >= self.expires_unix
            || !self.policy.is_fresh_at(trusted_unix)
            || Instant::now() >= self.fresh_until
        {
            return Err(ExtensionPolicyError::Time);
        }
        Ok(&self.policy)
    }
}

/// Product-sealed fixed-origin shared metadata client. It has no extension-id
/// parameter, per-user identity, install inventory, or configurable endpoint.
#[derive(Debug)]
pub struct ExtensionPolicyClient {
    root: Arc<[u8]>,
    channel: ExtensionPolicyChannel,
    transport: PolicyTransport,
    in_flight: tokio::sync::Semaphore,
}

impl ExtensionPolicyClient {
    /// Constructs without network I/O. Both channels currently fail closed.
    /// Installing staging infrastructure alone cannot populate this trust slot.
    pub fn product(channel: ExtensionPolicyChannel) -> Result<Self, ExtensionPolicyError> {
        let root = compiled_root(channel).ok_or(ExtensionPolicyError::TrustUnavailable)?;
        transport::preflight(root, transport::Object::Root)?;
        let (metadata, targets) = transport::bases(channel)?;
        let source = FixedOriginTransport::new(
            metadata.clone(),
            targets.clone(),
            Duration::from_secs(20),
            Duration::from_secs(5),
            "Zephium-Extension-Policy/1",
        )
        .map_err(|_| ExtensionPolicyError::Authentication)?;
        Ok(Self {
            root: Arc::from(root),
            channel,
            transport: PolicyTransport::new(metadata, targets, Source::Https(source)),
            in_flight: tokio::sync::Semaphore::new(1),
        })
    }

    /// Fetches the same signed policy for every client in a channel. Uses the
    /// caller's last durably accepted checkpoint, preserving role, policy,
    /// clock, and publisher-revocation high-water marks across restarts.
    /// This does not mutate durable state or grant admission authority.
    pub async fn refresh(
        &self,
        previous: Option<&ExtensionPolicyCheckpoint>,
    ) -> Result<ExtensionPolicyCandidate, ExtensionPolicyError> {
        let _permit = self
            .in_flight
            .try_acquire()
            .map_err(|_| ExtensionPolicyError::Busy)?;
        let started = now()?;
        // An overall deadline bounds the complete root-update chain as well
        // as per-request transport deadlines. No background task is spawned.
        tokio::time::timeout(Duration::from_secs(60), self.verify(previous, started))
            .await
            .map_err(|_| ExtensionPolicyError::Authentication)?
    }

    async fn verify(
        &self,
        previous: Option<&ExtensionPolicyCheckpoint>,
        started: u64,
    ) -> Result<ExtensionPolicyCandidate, ExtensionPolicyError> {
        let started_instant = Instant::now();
        let trust = Sha256::digest(&self.root).into();
        if let Some(previous) = previous {
            previous.validate_domain(trust, self.channel, started)?;
        }
        transport::preflight(&self.root, transport::Object::Root)?;
        let transport = self.transport.fresh()?;
        let repository = RepositoryLoader::new(
            &self.root,
            transport.metadata.clone(),
            transport.targets.clone(),
        )
        .transport(transport.clone())
        .limits(Limits {
            max_root_size: MAX_METADATA_BYTES as u64,
            max_timestamp_size: MAX_METADATA_BYTES as u64,
            max_snapshot_size: MAX_METADATA_BYTES as u64,
            max_targets_size: MAX_METADATA_BYTES as u64,
            max_root_updates: MAX_ROOT_UPDATES,
        })
        .expiration_enforcement(ExpirationEnforcement::Safe)
        // Tough's ephemeral datastore is deliberately NOT treated as durable
        // rollback protection. Every result is compared to the caller's
        // complete checkpoint below, including across online-key rotation.
        .load()
        .await
        .map_err(|_| ExtensionPolicyError::Authentication)?;

        let name = TargetName::new(EXTENSION_PUBLIC_POLICY_TARGET)
            .map_err(|_| ExtensionPolicyError::Authentication)?;
        let targets = &repository.targets().signed;
        if targets.delegations.is_some() || targets.targets.len() != 1 {
            return Err(ExtensionPolicyError::Authentication);
        }
        let target = targets
            .targets
            .get(&name)
            .ok_or(ExtensionPolicyError::Authentication)?;
        if target.length == 0 || target.length > MAX_EXTENSION_PUBLIC_POLICY_BYTES as u64 {
            return Err(ExtensionPolicyError::Authentication);
        }
        let bytes = repository
            .read_target(&name)
            .await
            .map_err(|_| ExtensionPolicyError::Authentication)?
            .ok_or(ExtensionPolicyError::Authentication)?
            .into_vec()
            .await
            .map_err(|_| ExtensionPolicyError::Authentication)?;
        let policy = ExtensionPublicPolicy::parse(&bytes)
            .map_err(|_| ExtensionPolicyError::Authentication)?;
        if policy.channel() != self.channel {
            return Err(ExtensionPolicyError::Authentication);
        }
        let finished = now()?;
        if finished < started || !policy.is_fresh_at(finished) {
            return Err(ExtensionPolicyError::Time);
        }
        let roles = [
            checkpoint::role(&repository.root().signed)?,
            checkpoint::role(&repository.timestamp().signed)?,
            checkpoint::role(&repository.snapshot().signed)?,
            checkpoint::role(&repository.targets().signed)?,
        ];
        let mut expires_unix = policy.expires_unix();
        for expires in [
            repository.root().signed.expires(),
            repository.timestamp().signed.expires(),
            repository.snapshot().signed.expires(),
            repository.targets().signed.expires(),
        ] {
            let expires =
                u64::try_from(expires.as_second()).map_err(|_| ExtensionPolicyError::Time)?;
            expires_unix = expires_unix.min(expires);
        }
        if finished >= expires_unix {
            return Err(ExtensionPolicyError::Time);
        }
        let checkpoint = ExtensionPolicyCheckpoint::candidate(
            trust,
            self.channel,
            roles,
            &policy,
            finished,
            previous,
        )?;
        let fresh_until = started_instant
            .checked_add(Duration::from_secs(expires_unix - started))
            .ok_or(ExtensionPolicyError::Time)?;
        // Never replay hashes of unsigned, attacker-personalized response
        // bytes. Only this fully authenticated bounded chain may enter the
        // next refresh's conditional cache.
        self.transport.retain_verified(&transport)?;
        Ok(ExtensionPolicyCandidate {
            bytes: bytes.into_boxed_slice(),
            policy,
            previous: previous.cloned(),
            checkpoint,
            expires_unix,
            fresh_until,
        })
    }
}

fn compiled_root(_channel: ExtensionPolicyChannel) -> Option<&'static [u8]> {
    None
}

fn now() -> Result<u64, ExtensionPolicyError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| ExtensionPolicyError::Time)
}
