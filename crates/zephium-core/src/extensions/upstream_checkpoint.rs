//! Non-authorizing durable upstream update high-water state.

use super::{ExtensionPackageKey, ExtensionUpstreamVersion};

/// Exact byte length of the structural version-one upstream checkpoint codec.
pub const EXTENSION_UPSTREAM_CHECKPOINT_BYTES: usize = 105;

/// Publisher and original artifact at the highest accepted upstream version.
///
/// This record must survive package rollback. Reading it from storage is not
/// authentication: candidates must originate from CRX and manifest validation,
/// and a successful comparison must be committed atomically with admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionUpstreamCheckpoint {
    publisher: ExtensionPackageKey,
    version: ExtensionUpstreamVersion,
    original_crx_sha256: [u8; 32],
    archive_sha256: [u8; 32],
}

/// Result of comparing an authenticated candidate with a durable high-water mark.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "an upstream update must handle rollback, equivocation, and publisher continuity"]
pub enum ExtensionUpstreamUpdateDisposition {
    /// Exact same upstream version and original bytes.
    Unchanged,
    /// Strictly newer version under the same complete publisher key.
    Advance,
    /// Publisher continuity failed.
    PublisherChanged,
    /// A lower upstream version cannot advance the update line.
    Rollback,
    /// A previously accepted version now names different bytes.
    Equivocation,
}

impl ExtensionUpstreamCheckpoint {
    /// Reconstructs structural evidence. Only authenticated acquisition may
    /// originate a new accepted checkpoint; this constructor grants no authority.
    pub const fn from_parts(
        publisher: ExtensionPackageKey,
        version: ExtensionUpstreamVersion,
        original_crx_sha256: [u8; 32],
        archive_sha256: [u8; 32],
    ) -> Self {
        Self {
            publisher,
            version,
            original_crx_sha256,
            archive_sha256,
        }
    }

    /// Full SHA-256 publisher identity, not the truncated Chromium identifier.
    pub const fn publisher(self) -> ExtensionPackageKey {
        self.publisher
    }
    /// Highest accepted upstream version.
    pub const fn version(self) -> ExtensionUpstreamVersion {
        self.version
    }
    /// Exact complete original CRX digest.
    pub const fn original_crx_sha256(self) -> [u8; 32] {
        self.original_crx_sha256
    }
    /// Exact original ZIP digest, distinct from a transformed tree.
    pub const fn archive_sha256(self) -> [u8; 32] {
        self.archive_sha256
    }

    /// Classifies without mutating or lowering the durable checkpoint.
    pub fn classify(self, candidate: Self) -> ExtensionUpstreamUpdateDisposition {
        use ExtensionUpstreamUpdateDisposition::*;
        if self.publisher != candidate.publisher {
            return PublisherChanged;
        }
        match candidate.version.cmp(&self.version) {
            std::cmp::Ordering::Less => Rollback,
            std::cmp::Ordering::Greater => Advance,
            std::cmp::Ordering::Equal if self == candidate => Unchanged,
            std::cmp::Ordering::Equal => Equivocation,
        }
    }

    /// Encodes fixed-size structural data for a bounded durable adapter.
    pub fn encode(self) -> [u8; EXTENSION_UPSTREAM_CHECKPOINT_BYTES] {
        let mut bytes = [0; EXTENSION_UPSTREAM_CHECKPOINT_BYTES];
        bytes[0] = 1;
        bytes[1..33].copy_from_slice(self.publisher.as_bytes());
        for (index, component) in self.version.components().into_iter().enumerate() {
            bytes[33 + index * 2..35 + index * 2].copy_from_slice(&component.to_be_bytes());
        }
        bytes[41..73].copy_from_slice(&self.original_crx_sha256);
        bytes[73..105].copy_from_slice(&self.archive_sha256);
        bytes
    }

    /// Decodes an exact versioned record; trailing bytes and zero versions fail.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != EXTENSION_UPSTREAM_CHECKPOINT_BYTES || bytes[0] != 1 {
            return None;
        }
        let mut version = [0; 4];
        for (index, component) in version.iter_mut().enumerate() {
            *component = u16::from_be_bytes(bytes[33 + index * 2..35 + index * 2].try_into().ok()?);
        }
        Some(Self::from_parts(
            ExtensionPackageKey::from_bytes(bytes[1..33].try_into().ok()?),
            ExtensionUpstreamVersion::from_components(version)?,
            bytes[41..73].try_into().ok()?,
            bytes[73..105].try_into().ok()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn checkpoint(version: &str) -> ExtensionUpstreamCheckpoint {
        ExtensionUpstreamCheckpoint::from_parts(
            ExtensionPackageKey::from_bytes([1; 32]),
            ExtensionUpstreamVersion::parse(version).unwrap(),
            [2; 32],
            [3; 32],
        )
    }
    #[test]
    fn refuses_downgrade_reissued_version_and_publisher_changes() {
        use ExtensionUpstreamUpdateDisposition::*;
        let accepted = checkpoint("1.10");
        assert_eq!(accepted.classify(checkpoint("1.9")), Rollback);
        assert_eq!(accepted.classify(checkpoint("1.10.0.0")), Unchanged);
        assert_eq!(accepted.classify(checkpoint("1.11")), Advance);
        let mut reissue = accepted;
        reissue.original_crx_sha256 = [4; 32];
        assert_eq!(accepted.classify(reissue), Equivocation);
        let mut replacement = checkpoint("2");
        replacement.publisher = ExtensionPackageKey::from_bytes([4; 32]);
        assert_eq!(accepted.classify(replacement), PublisherChanged);
        // Running an older local rollback generation must not replace accepted.
        assert_eq!(accepted.classify(checkpoint("1.9.1")), Rollback);
    }
    #[test]
    fn exact_durable_codec_rejects_unknown_truncated_and_zero_version_records() {
        let value = checkpoint("65535.2.0.1");
        let bytes = value.encode();
        assert_eq!(ExtensionUpstreamCheckpoint::decode(&bytes), Some(value));
        for length in 0..bytes.len() {
            assert!(ExtensionUpstreamCheckpoint::decode(&bytes[..length]).is_none());
        }
        let mut unknown = bytes;
        unknown[0] = 2;
        assert!(ExtensionUpstreamCheckpoint::decode(&unknown).is_none());
        let mut zero = bytes;
        zero[33..41].fill(0);
        assert!(ExtensionUpstreamCheckpoint::decode(&zero).is_none());
        assert!(ExtensionUpstreamCheckpoint::decode(&[0; 106]).is_none());
    }
}
