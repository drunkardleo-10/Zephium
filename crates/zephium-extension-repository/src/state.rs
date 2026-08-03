//! Canonical durable state, journal, and complete package-row identity.

use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use zephium_core::extensions::{ExtensionPackagePayloadIdentity, ExtensionPackageRevision};
use zephium_extension_authority::BundledCatalogCheckpoint;
use zephium_extension_package::{
    ExtensionReleaseCatalog, ExtensionReleaseCatalogRevision, ExtensionReleaseLegalArtifactKind,
    ExtensionReleasePackage, MAX_EXTENSION_RELEASE_CATALOG_BYTES,
};

use crate::ExtensionRepositoryError;

pub(crate) const STATE_SCHEMA_VERSION: u32 = 1;
pub(crate) const JOURNAL_SCHEMA_VERSION: u32 = 1;
pub(crate) const CHECKPOINT_SCHEMA_VERSION: u32 = 1;
pub(crate) const MAX_STATE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_CHECKPOINT_BYTES: usize = 64 * 1024;
pub(crate) const MAX_JOURNAL_BYTES: usize = 128 * 1024;
pub(crate) const MAX_PACKAGE_LINE_HIGH_WATERS: usize = 32;

const MAX_DURABLE_GENERATION: u64 = i64::MAX as u64;
const PACKAGE_ROW_DIGEST_DOMAIN: &[u8] = b"zephium:extension-repository-package-row:v1\0";

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Digest32([u8; 32]);

impl Digest32 {
    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn bytes(self) -> [u8; 32] {
        self.0
    }

    pub(crate) fn to_hex(self) -> String {
        let mut encoded = String::with_capacity(64);
        for byte in self.0 {
            use std::fmt::Write as _;
            write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
        }
        encoded
    }

    pub(crate) fn from_lower_hex(value: &str) -> Option<Self> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return None;
        }
        let mut decoded = [0_u8; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            decoded[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
        }
        Some(Self(decoded))
    }
}

impl fmt::Debug for Digest32 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Digest32({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

impl Serialize for Digest32 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Digest32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_lower_hex(&value).ok_or_else(|| D::Error::custom("invalid lowercase SHA-256"))
    }
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepositoryState {
    pub(crate) schema_version: u32,
    pub(crate) generation: u64,
    pub(crate) authority_id: Option<Digest32>,
    pub(crate) catalog_high_water: Option<StoredCatalogCheckpoint>,
    pub(crate) package_line_high_waters: Vec<PackageLineHighWater>,
}

impl Default for RepositoryState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            generation: 0,
            authority_id: None,
            catalog_high_water: None,
            package_line_high_waters: Vec::new(),
        }
    }
}

impl RepositoryState {
    pub(crate) fn validate(&self) -> Result<(), ExtensionRepositoryError> {
        if self.schema_version != STATE_SCHEMA_VERSION || self.generation > MAX_DURABLE_GENERATION {
            return Err(ExtensionRepositoryError::StateCorrupt);
        }
        if self.generation == 0 {
            if self.authority_id.is_some()
                || self.catalog_high_water.is_some()
                || !self.package_line_high_waters.is_empty()
            {
                return Err(ExtensionRepositoryError::StateCorrupt);
            }
            return Ok(());
        }

        let authority = self
            .authority_id
            .ok_or(ExtensionRepositoryError::StateCorrupt)?;
        let catalog = self
            .catalog_high_water
            .as_ref()
            .ok_or(ExtensionRepositoryError::StateCorrupt)?;
        if catalog.authority_id != authority
            || ExtensionReleaseCatalogRevision::new(catalog.revision).is_none()
            || catalog.catalog_length == 0
            || catalog.catalog_length > MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64
            || self.package_line_high_waters.is_empty()
            || self.package_line_high_waters.len() > MAX_PACKAGE_LINE_HIGH_WATERS
        {
            return Err(ExtensionRepositoryError::StateCorrupt);
        }
        let mut previous = None;
        for line in &self.package_line_high_waters {
            if ExtensionPackageRevision::new(line.revision).is_none()
                || previous.is_some_and(|key: Digest32| key >= line.package_key)
            {
                return Err(ExtensionRepositoryError::StateCorrupt);
            }
            previous = Some(line.package_key);
        }
        Ok(())
    }

    pub(crate) fn next_generation(&self) -> Result<u64, ExtensionRepositoryError> {
        let next = self
            .generation
            .checked_add(1)
            .ok_or(ExtensionRepositoryError::GenerationExhausted)?;
        if next > MAX_DURABLE_GENERATION {
            return Err(ExtensionRepositoryError::GenerationExhausted);
        }
        Ok(next)
    }

    pub(crate) fn checkpoint(&self) -> Option<&StoredCatalogCheckpoint> {
        self.catalog_high_water.as_ref()
    }

    pub(crate) fn line(&self, key: Digest32) -> Option<&PackageLineHighWater> {
        self.package_line_high_waters
            .binary_search_by_key(&key, |line| line.package_key)
            .ok()
            .map(|index| &self.package_line_high_waters[index])
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredCatalogCheckpoint {
    pub(crate) authority_id: Digest32,
    pub(crate) revision: u64,
    pub(crate) catalog_length: u64,
    pub(crate) catalog_sha256: Digest32,
    pub(crate) inventory_sha256: Digest32,
}

impl StoredCatalogCheckpoint {
    pub(crate) fn from_admitted(
        checkpoint: BundledCatalogCheckpoint,
        catalog_length: usize,
    ) -> Result<Self, ExtensionRepositoryError> {
        let catalog_length = u64::try_from(catalog_length)
            .map_err(|_| ExtensionRepositoryError::CatalogBytesMismatch)?;
        Ok(Self {
            authority_id: Digest32::from_bytes(checkpoint.authority().bytes()),
            revision: checkpoint.revision().get(),
            catalog_length,
            catalog_sha256: Digest32::from_bytes(checkpoint.catalog_digest().bytes()),
            inventory_sha256: Digest32::from_bytes(checkpoint.inventory_digest().bytes()),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageLineHighWater {
    pub(crate) package_key: Digest32,
    pub(crate) revision: u64,
    pub(crate) package_row_sha256: Digest32,
}

impl PackageLineHighWater {
    pub(crate) fn from_package(
        package: &ExtensionReleasePackage,
    ) -> Result<Self, ExtensionRepositoryError> {
        Ok(Self {
            package_key: Digest32::from_bytes(package.identity().key().bytes()),
            revision: package.identity().revision().get(),
            package_row_sha256: digest_package_row(package)?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransitionJournal {
    pub(crate) schema_version: u32,
    pub(crate) generation: u64,
    pub(crate) previous_state_sha256: Digest32,
    pub(crate) next_state_sha256: Digest32,
    pub(crate) next_state: RepositoryState,
}

impl TransitionJournal {
    pub(crate) fn validate(&self) -> Result<(), ExtensionRepositoryError> {
        if self.schema_version != JOURNAL_SCHEMA_VERSION
            || self.generation != self.next_state.generation
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        self.next_state
            .validate()
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryCheckpoint {
    pub(crate) schema_version: u32,
    pub(crate) generation: u64,
    pub(crate) state_sha256: Digest32,
}

impl RecoveryCheckpoint {
    pub(crate) const fn new(generation: u64, state_sha256: Digest32) -> Self {
        Self {
            schema_version: CHECKPOINT_SCHEMA_VERSION,
            generation,
            state_sha256,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), ExtensionRepositoryError> {
        if self.schema_version != CHECKPOINT_SCHEMA_VERSION
            || self.generation > MAX_DURABLE_GENERATION
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }
}

pub(crate) fn digest_package_row(
    package: &ExtensionReleasePackage,
) -> Result<Digest32, ExtensionRepositoryError> {
    let mut digest = Sha256::new();
    digest.update(PACKAGE_ROW_DIGEST_DOMAIN);
    let identity = package.identity();
    digest.update(identity.authority().as_bytes());
    digest.update(identity.key().as_bytes());
    digest.update(identity.revision().get().to_be_bytes());
    match identity.payload() {
        ExtensionPackagePayloadIdentity::BundledTree => digest.update([1]),
        ExtensionPackagePayloadIdentity::AcquiredZip { length, sha256 } => {
            digest.update([2]);
            digest.update(length.get().to_be_bytes());
            digest.update(sha256.as_bytes());
        }
    }
    digest.update(identity.manifest_sha256().as_bytes());
    digest.update(identity.tree_sha256().as_bytes());
    digest.update(package.tree_index_sha256().as_bytes());
    digest.update(package.tree_index_length().to_be_bytes());
    update_usize(&mut digest, package.tree_file_count())?;
    digest.update(package.tree_bytes().to_be_bytes());

    match package.chromium() {
        Some(chromium) => {
            digest.update([1]);
            digest.update(chromium.manifest_key_sha256().as_bytes());
        }
        None => digest.update([0]),
    }

    let provenance = package.provenance();
    update_str(&mut digest, provenance.source_url())?;
    update_str(&mut digest, provenance.upstream_version())?;
    update_str(&mut digest, provenance.upstream_revision())?;
    update_str(&mut digest, provenance.license_expression())?;
    update_str(&mut digest, provenance.attribution())?;
    update_str(&mut digest, provenance.redistribution())?;
    let notice = provenance.legal_notice();
    update_str(&mut digest, notice.target().as_str())?;
    match notice.kind() {
        ExtensionReleaseLegalArtifactKind::NoticeBundle => digest.update([1]),
    }
    digest.update(notice.length().to_be_bytes());
    digest.update(notice.sha256());
    match provenance.corresponding_source() {
        Some(source) => {
            digest.update([1]);
            update_str(&mut digest, source.url())?;
            update_str(&mut digest, source.revision())?;
        }
        None => digest.update([0]),
    }
    Ok(Digest32::from_bytes(digest.finalize().into()))
}

pub(crate) fn validate_catalog_lines(
    state: &RepositoryState,
    catalog: &ExtensionReleaseCatalog,
) -> Result<(), ExtensionRepositoryError> {
    for package in catalog.packages() {
        let candidate = PackageLineHighWater::from_package(package)?;
        let durable = state
            .line(candidate.package_key)
            .ok_or(ExtensionRepositoryError::StateCorrupt)?;
        if durable.revision != candidate.revision
            || durable.package_row_sha256 != candidate.package_row_sha256
        {
            return Err(ExtensionRepositoryError::StateCorrupt);
        }
    }
    Ok(())
}

fn update_str(digest: &mut Sha256, value: &str) -> Result<(), ExtensionRepositoryError> {
    update_usize(digest, value.len())?;
    digest.update(value.as_bytes());
    Ok(())
}

fn update_usize(digest: &mut Sha256, value: usize) -> Result<(), ExtensionRepositoryError> {
    let value = u64::try_from(value).map_err(|_| ExtensionRepositoryError::StateCorrupt)?;
    digest.update(value.to_be_bytes());
    Ok(())
}
