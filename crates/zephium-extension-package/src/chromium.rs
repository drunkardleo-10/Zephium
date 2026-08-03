use std::error::Error;
use std::fmt;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use sha2::{Digest, Sha256};

use crate::{MAX_CHROMIUM_MANIFEST_KEY_BASE64_BYTES, MAX_CHROMIUM_MANIFEST_KEY_BYTES};

/// SHA-256 of exact decoded Chromium manifest public-key bytes.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChromiumManifestKeyDigest([u8; 32]);

impl ChromiumManifestKeyDigest {
    /// Constructs an exact structural digest.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns exact digest bytes.
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    /// Borrows exact digest bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn derived_extension_id(self) -> ChromiumExtensionId {
        extension_id_from_sha256(&self.0)
    }
}

impl fmt::Debug for ChromiumManifestKeyDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ChromiumManifestKeyDigest({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

/// Canonical 32-character Chromium extension identifier.
///
/// Chromium maps hexadecimal nibbles to `a` through `p` so an extension ID
/// cannot be interpreted as an all-numeric host.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChromiumExtensionId(Box<str>);

impl ChromiumExtensionId {
    /// Parses an exact lowercase Chromium extension identifier.
    pub fn parse(value: &str) -> Result<Self, ChromiumManifestKeyError> {
        if value.len() != 32 || !value.bytes().all(|byte| matches!(byte, b'a'..=b'p')) {
            return Err(ChromiumManifestKeyError::InvalidExtensionId);
        }
        Ok(Self(value.into()))
    }

    /// Returns the exact identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ChromiumExtensionId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ChromiumExtensionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl fmt::Debug for ChromiumExtensionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ChromiumExtensionId")
            .field(&self.as_str())
            .finish()
    }
}

/// Canonically decoded Chromium `manifest.json` `key` value.
///
/// Zephium accepts only raw padded standard Base64 with no PEM wrapper or
/// whitespace. Chromium accepts a wider development grammar, but a curated
/// package must have one platform-independent byte representation before its
/// native identity is admitted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChromiumManifestKey {
    decoded: Box<[u8]>,
    digest: ChromiumManifestKeyDigest,
    extension_id: ChromiumExtensionId,
}

impl ChromiumManifestKey {
    /// Decodes and canonicalizes one exact manifest `key` string.
    pub fn parse_canonical(value: &str) -> Result<Self, ChromiumManifestKeyError> {
        if value.is_empty() {
            return Err(ChromiumManifestKeyError::Empty);
        }
        if value.len() > MAX_CHROMIUM_MANIFEST_KEY_BASE64_BYTES {
            return Err(ChromiumManifestKeyError::EncodedTooLarge {
                bytes: value.len(),
                max: MAX_CHROMIUM_MANIFEST_KEY_BASE64_BYTES,
            });
        }
        if !value.is_ascii() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(ChromiumManifestKeyError::NonCanonicalBase64);
        }
        let decoded = STANDARD
            .decode(value)
            .map_err(|_| ChromiumManifestKeyError::NonCanonicalBase64)?;
        if decoded.is_empty() || decoded.len() > MAX_CHROMIUM_MANIFEST_KEY_BYTES {
            return Err(ChromiumManifestKeyError::DecodedSize {
                bytes: decoded.len(),
                max: MAX_CHROMIUM_MANIFEST_KEY_BYTES,
            });
        }
        if STANDARD.encode(&decoded) != value {
            return Err(ChromiumManifestKeyError::NonCanonicalBase64);
        }

        let hash: [u8; 32] = Sha256::digest(&decoded).into();
        let digest = ChromiumManifestKeyDigest(hash);
        let extension_id = digest.derived_extension_id();
        Ok(Self {
            decoded: decoded.into_boxed_slice(),
            digest,
            extension_id,
        })
    }

    /// Borrows the exact decoded public-key bytes used by Chromium identity.
    pub fn decoded_bytes(&self) -> &[u8] {
        &self.decoded
    }

    /// Returns SHA-256 of the exact decoded key bytes.
    pub const fn digest(&self) -> ChromiumManifestKeyDigest {
        self.digest
    }

    /// Returns the extension ID Chromium derives from the decoded key.
    pub const fn extension_id(&self) -> &ChromiumExtensionId {
        &self.extension_id
    }

    /// Verifies the signed expected native ID without path-derived fallback.
    pub fn verify_expected(
        &self,
        expected: &ChromiumExtensionId,
    ) -> Result<(), ChromiumManifestKeyError> {
        if self.extension_id == *expected {
            Ok(())
        } else {
            Err(ChromiumManifestKeyError::ExpectedIdMismatch)
        }
    }
}

fn extension_id_from_sha256(hash: &[u8; 32]) -> ChromiumExtensionId {
    let mut bytes = [0_u8; 32];
    for (index, byte) in hash[..16].iter().copied().enumerate() {
        bytes[index * 2] = b'a' + (byte >> 4);
        bytes[index * 2 + 1] = b'a' + (byte & 0x0f);
    }
    let value = std::str::from_utf8(&bytes).expect("a-through-p bytes are valid UTF-8");
    ChromiumExtensionId(value.into())
}

/// Stable Chromium manifest-key rejection reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChromiumManifestKeyError {
    /// The manifest key is empty.
    Empty,
    /// Encoded bytes exceed the hard ceiling.
    EncodedTooLarge {
        /// Observed bytes.
        bytes: usize,
        /// Maximum bytes.
        max: usize,
    },
    /// Decoded key bytes are empty or exceed the hard ceiling.
    DecodedSize {
        /// Observed bytes.
        bytes: usize,
        /// Maximum bytes.
        max: usize,
    },
    /// Text is not the one canonical standard-Base64 representation.
    NonCanonicalBase64,
    /// A separately supplied extension ID is not canonical.
    InvalidExtensionId,
    /// The derived ID differs from signed expected native identity.
    ExpectedIdMismatch,
}

impl fmt::Display for ChromiumManifestKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("Chromium manifest key is empty"),
            Self::EncodedTooLarge { bytes, max } => write!(
                formatter,
                "Chromium manifest key uses {bytes} encoded bytes; maximum is {max}"
            ),
            Self::DecodedSize { bytes, max } => write!(
                formatter,
                "Chromium manifest key uses {bytes} decoded bytes; maximum is {max}"
            ),
            Self::NonCanonicalBase64 => {
                formatter.write_str("Chromium manifest key is not canonical standard Base64")
            }
            Self::InvalidExtensionId => {
                formatter.write_str("Chromium extension ID is not canonical")
            }
            Self::ExpectedIdMismatch => {
                formatter.write_str("Chromium manifest key does not derive the expected ID")
            }
        }
    }
}

impl Error for ChromiumManifestKeyError {}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn matches_chromiums_generate_id_golden_vector() {
        // Chromium's id_util_unittest expects GenerateId("_") to produce
        // this value. `Xw==` is the canonical manifest representation of the
        // same one-byte public-key input.
        let key = ChromiumManifestKey::parse_canonical("Xw==").unwrap();
        assert_eq!(
            key.extension_id().as_str(),
            "ncocknphbhhlhkikpnnlmbcnbgdempcd"
        );
    }

    #[test]
    fn rejects_development_and_ambiguous_base64_forms() {
        for invalid in [
            "",
            "Xw",
            "Xw==\n",
            " Xw==",
            "-----BEGIN PUBLIC KEY-----Xw==-----END PUBLIC KEY-----",
            "_w==",
        ] {
            assert!(ChromiumManifestKey::parse_canonical(invalid).is_err());
        }
    }

    #[test]
    fn expected_id_check_is_exact() {
        let key = ChromiumManifestKey::parse_canonical("Xw==").unwrap();
        let other = ChromiumExtensionId::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        assert_eq!(
            key.verify_expected(&other),
            Err(ChromiumManifestKeyError::ExpectedIdMismatch)
        );
        assert_eq!(key.verify_expected(key.extension_id()), Ok(()));
    }

    proptest! {
        #[test]
        fn every_bounded_key_derives_a_canonical_id(bytes in prop::collection::vec(any::<u8>(), 1..=2048)) {
            let encoded = STANDARD.encode(&bytes);
            let key = ChromiumManifestKey::parse_canonical(&encoded).unwrap();
            prop_assert_eq!(key.decoded_bytes(), bytes.as_slice());
            prop_assert_eq!(key.extension_id().as_str().len(), 32);
            prop_assert!(key.extension_id().as_str().bytes().all(|byte| matches!(byte, b'a'..=b'p')));
        }
    }
}
