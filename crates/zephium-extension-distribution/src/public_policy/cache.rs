use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use zephium_extension_package::{
    ExtensionPolicyChannel, ExtensionPublicPolicy, MAX_EXTENSION_PUBLIC_POLICY_BYTES,
};
use zephium_private_fs::{ByteLimit, LockedPrivateNamespace, PrivateComponent, PrivateDirectory};

use super::{
    now, ExtensionPolicyCandidate, ExtensionPolicyCheckpoint, ExtensionPolicyClient,
    ExtensionPolicyError,
};

const MAGIC: &[u8; 8] = b"ZEPOL001";
const MAX_STATE_BYTES: usize =
    12 + super::checkpoint::MAX_CHECKPOINT_BYTES + MAX_EXTENSION_PUBLIC_POLICY_BYTES;

/// Signature-verified shared policy whose exact target and high-water state
/// have been durably committed. This is policy evidence only: it grants no
/// Verified classification, package installation, permissions, or native lease.
#[derive(Debug)]
pub struct AcceptedExtensionPolicy {
    candidate: ExtensionPolicyCandidate,
    epoch: Arc<AtomicU64>,
    generation: u64,
}

impl AcceptedExtensionPolicy {
    /// Reads the policy only while every signed deadline remains fresh.
    pub fn policy(&self) -> Result<&ExtensionPublicPolicy, ExtensionPolicyError> {
        if self.epoch.load(Ordering::Acquire) != self.generation {
            return Err(ExtensionPolicyError::StaleCandidate);
        }
        self.candidate.policy_at(now()?)
    }
    /// Exact committed high-water checkpoint.
    pub fn checkpoint(&self) -> &ExtensionPolicyCheckpoint {
        &self.candidate.checkpoint
    }
}

/// Exclusive shared-policy cache, independent of accounts and profiles. The
/// caller supplies a browser-owned namespace, never a web-selected path.
/// Durable operations use the existing private-filesystem lease and atomic
/// replacement; unsupported platforms fail closed in that adapter.
///
/// Run this owner on the extension worker: bounded private-file I/O is
/// synchronous. No background thread or timer is created here.
pub struct ExtensionPolicyCache {
    directory: PrivateDirectory,
    trust: [u8; 32],
    channel: ExtensionPolicyChannel,
    checkpoint: Option<ExtensionPolicyCheckpoint>,
    poisoned: bool,
    epoch: Arc<AtomicU64>,
}

impl ExtensionPolicyCache {
    /// Opens one channel beneath an exclusively leased namespace. Existing
    /// corruption fails closed; it is never treated as an empty/new cache.
    /// An uncommitted staging file is discarded only after validating current
    /// state. A rename that completed before a crash is therefore retained.
    pub fn open(
        namespace: LockedPrivateNamespace,
        client: &ExtensionPolicyClient,
    ) -> Result<Self, ExtensionPolicyError> {
        let channel_name = match client.channel {
            ExtensionPolicyChannel::Stable => "stable",
            ExtensionPolicyChannel::Staging => "staging",
        };
        let directory = namespace
            .directory()
            .create_private_child(&name(channel_name)?)
            .map_err(|_| ExtensionPolicyError::Storage)?;
        let mut cache = Self {
            directory,
            trust: Sha256::digest(&client.root).into(),
            channel: client.channel,
            checkpoint: None,
            poisoned: false,
            epoch: Arc::new(AtomicU64::new(0)),
        };
        let entries = cache
            .directory
            .list_components(2)
            .map_err(|_| ExtensionPolicyError::Storage)?;
        if entries
            .iter()
            .any(|entry| !matches!(entry.as_str(), "current" | "staged"))
        {
            return Err(ExtensionPolicyError::Storage);
        }
        cache.checkpoint = cache.read()?;
        cache
            .directory
            .remove_verified_regular(&name("staged")?)
            .map_err(|_| ExtensionPolicyError::Storage)?;
        Ok(cache)
    }

    /// Last accepted structural checkpoint, including expired policy history.
    /// This is not a cached-policy admission path.
    pub fn checkpoint(&self) -> Option<&ExtensionPolicyCheckpoint> {
        self.checkpoint.as_ref()
    }

    /// Fetches and verifies fresh policy, then durably accepts it as one unit.
    /// Network failure preserves all prior state and cannot extend freshness.
    pub async fn refresh(
        &mut self,
        client: &ExtensionPolicyClient,
    ) -> Result<AcceptedExtensionPolicy, ExtensionPolicyError> {
        self.check_client(client)?;
        let candidate = client.refresh(self.checkpoint()).await?;
        self.accept(candidate)
    }

    /// Commits a verified candidate only if its expected prior checkpoint is
    /// still exact. Target bytes and checkpoint share a single atomic record.
    /// No usable receipt is returned until the private-file commit has settled.
    pub fn accept(
        &mut self,
        mut candidate: ExtensionPolicyCandidate,
    ) -> Result<AcceptedExtensionPolicy, ExtensionPolicyError> {
        if self.poisoned {
            return Err(ExtensionPolicyError::Storage);
        }
        let current = self.read()?;
        if current != self.checkpoint || current.as_ref() != candidate.previous_checkpoint() {
            return Err(ExtensionPolicyError::StaleCandidate);
        }
        let time = now()?;
        candidate
            .checkpoint
            .validate_domain(self.trust, self.channel, time)?;
        candidate.policy_at(time)?;
        candidate.checkpoint.trusted_unix = time;
        let checkpoint = candidate.checkpoint.encode()?;
        let mut bytes = Vec::with_capacity(12 + checkpoint.len() + candidate.bytes.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&(checkpoint.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&checkpoint);
        bytes.extend_from_slice(&candidate.bytes);
        // From the first mutation onwards any error requires reopening and
        // recovery; a partially settled rename must never authorize a receipt.
        self.poisoned = true;
        let generation = self
            .epoch
            .load(Ordering::Acquire)
            .checked_add(1)
            .filter(|generation| *generation < u64::MAX)
            .ok_or(ExtensionPolicyError::Storage)?;
        self.epoch.store(generation, Ordering::Release);
        self.directory
            .write_new_synced(&name("staged")?, &bytes, limit()?)
            .map_err(|_| ExtensionPolicyError::Storage)?;
        if current.is_some() {
            self.directory
                .replace_verified_regular(&name("staged")?, &name("current")?)
                .map_err(|_| ExtensionPolicyError::Storage)?;
        } else {
            self.directory
                .publish_noreplace_verified_regular(&name("staged")?, &name("current")?)
                .map_err(|_| ExtensionPolicyError::Storage)?;
        }
        // A deadline could elapse during disk synchronization. Persisted
        // high-water still stands; refuse a fresh receipt in that case.
        self.checkpoint = Some(candidate.checkpoint.clone());
        self.poisoned = false;
        candidate.policy_at(now()?)?;
        Ok(AcceptedExtensionPolicy {
            candidate,
            epoch: Arc::clone(&self.epoch),
            generation,
        })
    }

    fn check_client(&self, client: &ExtensionPolicyClient) -> Result<(), ExtensionPolicyError> {
        if self.poisoned {
            return Err(ExtensionPolicyError::Storage);
        }
        if self.trust != <[u8; 32]>::from(Sha256::digest(&client.root))
            || self.channel != client.channel
        {
            return Err(ExtensionPolicyError::Checkpoint);
        }
        Ok(())
    }

    fn read(&self) -> Result<Option<ExtensionPolicyCheckpoint>, ExtensionPolicyError> {
        let Some(bytes) = self
            .directory
            .read_bounded_regular(&name("current")?, limit()?)
            .map_err(|_| ExtensionPolicyError::Storage)?
        else {
            return Ok(None);
        };
        if bytes.len() < 12 || &bytes[..8] != MAGIC {
            return Err(ExtensionPolicyError::Storage);
        }
        let length = u32::from_be_bytes(
            bytes[8..12]
                .try_into()
                .map_err(|_| ExtensionPolicyError::Storage)?,
        ) as usize;
        let end = 12usize
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or(ExtensionPolicyError::Storage)?;
        let checkpoint = ExtensionPolicyCheckpoint::decode(&bytes[12..end])?;
        checkpoint.validate_domain(self.trust, self.channel, now()?)?;
        checkpoint.matches_policy(&bytes[end..])?;
        Ok(Some(checkpoint))
    }
}

impl Drop for ExtensionPolicyCache {
    fn drop(&mut self) {
        self.epoch.store(u64::MAX, Ordering::Release);
    }
}

fn name(value: &str) -> Result<PrivateComponent, ExtensionPolicyError> {
    PrivateComponent::new(value).map_err(|_| ExtensionPolicyError::Storage)
}
fn limit() -> Result<ByteLimit, ExtensionPolicyError> {
    ByteLimit::new(MAX_STATE_BYTES).map_err(|_| ExtensionPolicyError::Storage)
}
