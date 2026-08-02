//! Structural identity for one immutable extension package release.

use std::fmt;

use sha2::{Digest, Sha256};

/// Exact byte length of every SHA-256 value in an extension package identity.
pub const EXTENSION_SHA256_BYTES: usize = 32;

/// Largest acquired extension archive admitted by the package boundary.
///
/// This limit is part of the durable profile schema. Raising it requires a
/// new migration and a matching package-admission review.
pub const MAX_EXTENSION_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

/// Largest revision representable by the durable SQLite adapters.
const MAX_DURABLE_EXTENSION_REVISION: u64 = i64::MAX as u64;

macro_rules! sha256_identity {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name([u8; EXTENSION_SHA256_BYTES]);

        impl $name {
            /// Constructs an exact structural value.
            ///
            /// This performs no authentication. Only the package-authority
            /// boundary may decide whether bytes identified by this value are
            /// trusted and available.
            pub const fn from_bytes(bytes: [u8; EXTENSION_SHA256_BYTES]) -> Self {
                Self(bytes)
            }

            /// Returns the exact bytes without allocating.
            pub const fn bytes(self) -> [u8; EXTENSION_SHA256_BYTES] {
                self.0
            }

            /// Borrows the exact bytes without allocating.
            pub const fn as_bytes(&self) -> &[u8; EXTENSION_SHA256_BYTES] {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                // Identity digests are not user-facing diagnostics. A short,
                // fixed-size prefix distinguishes values in tests and bounded
                // logs without repeatedly formatting 64 attacker-influenced
                // hexadecimal characters.
                write!(
                    formatter,
                    concat!(stringify!($name), "({:02x}{:02x}{:02x}{:02x}…)"),
                    self.0[0], self.0[1], self.0[2], self.0[3]
                )
            }
        }
    };
}

sha256_identity!(
    ExtensionAuthorityId,
    "Stable identity of the repository trust domain and its epoch."
);
sha256_identity!(
    ExtensionPackageKey,
    "Stable authenticated product/publisher key within one authority."
);
sha256_identity!(
    ExtensionArchiveDigest,
    "SHA-256 of an exact acquired extension-package ZIP payload."
);
sha256_identity!(
    ExtensionManifestDigest,
    "SHA-256 of the exact admitted extension manifest."
);
sha256_identity!(
    ExtensionTreeDigest,
    "SHA-256 of the canonical materialized extension resource tree."
);

/// Strictly positive authority sequence for one package release.
///
/// This is not the extension manifest's display `version`. Rollback and
/// equivocation checks are performed by the package authority against this
/// authenticated sequence and the complete identity below.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionPackageRevision(u64);

impl ExtensionPackageRevision {
    /// First valid durable revision.
    pub const INITIAL: Self = Self(1);

    /// Creates a revision accepted by every durable adapter.
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 || value > MAX_DURABLE_EXTENSION_REVISION {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Returns the durable integer value.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next revision without wrapping or crossing SQLite's signed
    /// integer boundary.
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Self::new(next),
            None => None,
        }
    }
}

/// Exact, bounded byte length of one acquired extension-package ZIP.
///
/// The length is part of payload identity rather than merely accounting
/// metadata: it prevents a digest from being accepted with incomplete or
/// ambiguously framed acquisition evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionArchiveLength(u64);

impl ExtensionArchiveLength {
    /// Constructs an acquired-ZIP length within the package admission bound.
    pub const fn new(bytes: u64) -> Option<Self> {
        if bytes == 0 || bytes > MAX_EXTENSION_ARCHIVE_BYTES {
            None
        } else {
            Some(Self(bytes))
        }
    }

    /// Returns the exact archive byte length.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Exact representation identity of one admitted package payload.
///
/// Bundled release trees have no acquired archive and must never carry
/// synthetic archive evidence. A future network-acquired ZIP is bound by
/// both its bounded byte length and SHA-256 digest. The variants' canonical
/// digest encoding is centralized in [`Self::update_sha256`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExtensionPackagePayloadIdentity {
    /// An authenticated canonical tree shipped as part of Zephium's release.
    BundledTree,
    /// An exact acquired ZIP payload, before validated materialization.
    AcquiredZip {
        length: ExtensionArchiveLength,
        sha256: ExtensionArchiveDigest,
    },
}

impl ExtensionPackagePayloadIdentity {
    const BUNDLED_TREE_DIGEST_TAG: u8 = 1;
    const ACQUIRED_ZIP_DIGEST_TAG: u8 = 2;

    /// Constructs exact acquired-ZIP identity after enforcing the byte bound.
    pub const fn acquired_zip(length: u64, sha256: ExtensionArchiveDigest) -> Option<Self> {
        match ExtensionArchiveLength::new(length) {
            Some(length) => Some(Self::AcquiredZip { length, sha256 }),
            None => None,
        }
    }

    /// Returns exact ZIP evidence, or `None` for a bundled tree.
    pub const fn acquired_zip_evidence(
        self,
    ) -> Option<(ExtensionArchiveLength, ExtensionArchiveDigest)> {
        match self {
            Self::BundledTree => None,
            Self::AcquiredZip { length, sha256 } => Some((length, sha256)),
        }
    }

    /// Appends the stable, unambiguous payload identity to a SHA-256 input.
    ///
    /// Encoding is one variant tag, followed for acquired ZIPs by an
    /// eight-byte big-endian length and the exact 32-byte archive digest.
    pub fn update_sha256(self, digest: &mut Sha256) {
        match self {
            Self::BundledTree => digest.update([Self::BUNDLED_TREE_DIGEST_TAG]),
            Self::AcquiredZip { length, sha256 } => {
                digest.update([Self::ACQUIRED_ZIP_DIGEST_TAG]);
                digest.update(length.get().to_be_bytes());
                digest.update(sha256.as_bytes());
            }
        }
    }
}

/// Immutable structural identity of one exact extension package release.
///
/// `authority` and `key` identify the update line. `revision` orders releases
/// within that line. The tagged payload identity distinguishes bundled trees
/// from acquired ZIPs without synthetic evidence. The manifest and tree
/// digests independently bind admitted authority and canonical materialized
/// resources so a native adapter can never treat a path or manifest version
/// string as package authority.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionPackageIdentity {
    authority: ExtensionAuthorityId,
    key: ExtensionPackageKey,
    revision: ExtensionPackageRevision,
    payload: ExtensionPackagePayloadIdentity,
    manifest_sha256: ExtensionManifestDigest,
    tree_sha256: ExtensionTreeDigest,
}

impl ExtensionPackageIdentity {
    /// Constructs one structurally complete package identity.
    ///
    /// This does not authenticate any bytes. The package authority must match
    /// all six fields to its durable current/previous/candidate state before
    /// returning an activation-capable package lease.
    pub const fn new(
        authority: ExtensionAuthorityId,
        key: ExtensionPackageKey,
        revision: ExtensionPackageRevision,
        payload: ExtensionPackagePayloadIdentity,
        manifest_sha256: ExtensionManifestDigest,
        tree_sha256: ExtensionTreeDigest,
    ) -> Self {
        Self {
            authority,
            key,
            revision,
            payload,
            manifest_sha256,
            tree_sha256,
        }
    }

    pub const fn authority(&self) -> ExtensionAuthorityId {
        self.authority
    }

    pub const fn key(&self) -> ExtensionPackageKey {
        self.key
    }

    pub const fn revision(&self) -> ExtensionPackageRevision {
        self.revision
    }

    pub const fn payload(&self) -> ExtensionPackagePayloadIdentity {
        self.payload
    }

    pub const fn manifest_sha256(&self) -> ExtensionManifestDigest {
        self.manifest_sha256
    }

    pub const fn tree_sha256(&self) -> ExtensionTreeDigest {
        self.tree_sha256
    }

    /// Stable update-line key, deliberately excluding release revision and
    /// content digests.
    pub const fn update_line(&self) -> (ExtensionAuthorityId, ExtensionPackageKey) {
        (self.authority, self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn archive_digest(byte: u8) -> ExtensionArchiveDigest {
        ExtensionArchiveDigest::from_bytes([byte; EXTENSION_SHA256_BYTES])
    }

    fn manifest_digest(byte: u8) -> ExtensionManifestDigest {
        ExtensionManifestDigest::from_bytes([byte; EXTENSION_SHA256_BYTES])
    }

    fn tree_digest(byte: u8) -> ExtensionTreeDigest {
        ExtensionTreeDigest::from_bytes([byte; EXTENSION_SHA256_BYTES])
    }

    fn acquired_zip(byte: u8) -> ExtensionPackagePayloadIdentity {
        ExtensionPackagePayloadIdentity::acquired_zip(u64::from(byte) + 1, archive_digest(byte))
            .expect("bounded archive fixture")
    }

    #[test]
    fn durable_revision_rejects_zero_and_sqlite_overflow() {
        assert_eq!(ExtensionPackageRevision::new(0), None);
        assert_eq!(
            ExtensionPackageRevision::new(i64::MAX as u64),
            Some(ExtensionPackageRevision(i64::MAX as u64))
        );
        assert_eq!(ExtensionPackageRevision::new(i64::MAX as u64 + 1), None);
        assert_eq!(ExtensionPackageRevision(i64::MAX as u64).next(), None);
    }

    #[test]
    fn package_representations_are_independent_parts_of_identity() {
        let base = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::INITIAL,
            acquired_zip(3),
            manifest_digest(4),
            tree_digest(5),
        );
        let changed_tree = ExtensionPackageIdentity::new(
            base.authority(),
            base.key(),
            base.revision(),
            base.payload(),
            base.manifest_sha256(),
            tree_digest(6),
        );
        assert_ne!(base, changed_tree);
        assert_eq!(base.update_line(), changed_tree.update_line());
    }

    #[test]
    fn archive_length_is_strictly_positive_and_bounded() {
        assert_eq!(ExtensionArchiveLength::new(0), None);
        assert_eq!(
            ExtensionArchiveLength::new(MAX_EXTENSION_ARCHIVE_BYTES)
                .map(ExtensionArchiveLength::get),
            Some(MAX_EXTENSION_ARCHIVE_BYTES)
        );
        assert_eq!(
            ExtensionArchiveLength::new(MAX_EXTENSION_ARCHIVE_BYTES + 1),
            None
        );
    }

    #[test]
    fn payload_identity_preserves_exact_zip_evidence() {
        let digest = archive_digest(9);
        let payload = ExtensionPackagePayloadIdentity::acquired_zip(17, digest)
            .expect("bounded exact archive evidence");
        assert_eq!(
            payload.acquired_zip_evidence(),
            Some((ExtensionArchiveLength::new(17).unwrap(), digest))
        );
        assert_eq!(
            ExtensionPackagePayloadIdentity::BundledTree.acquired_zip_evidence(),
            None
        );
    }

    #[test]
    fn payload_digest_encoding_is_tagged_and_length_bound() {
        fn encoded(payload: ExtensionPackagePayloadIdentity) -> [u8; 32] {
            let mut digest = Sha256::new();
            payload.update_sha256(&mut digest);
            digest.finalize().into()
        }

        let archive = archive_digest(3);
        let first = ExtensionPackagePayloadIdentity::acquired_zip(1, archive).unwrap();
        let second = ExtensionPackagePayloadIdentity::acquired_zip(2, archive).unwrap();
        assert_ne!(
            encoded(ExtensionPackagePayloadIdentity::BundledTree),
            encoded(first)
        );
        assert_ne!(encoded(first), encoded(second));
    }

    proptest! {
        #[test]
        fn sha256_wrappers_preserve_every_byte(bytes in any::<[u8; 32]>()) {
            let authority = ExtensionAuthorityId::from_bytes(bytes);
            let key = ExtensionPackageKey::from_bytes(bytes);
            let archive = ExtensionArchiveDigest::from_bytes(bytes);
            let manifest = ExtensionManifestDigest::from_bytes(bytes);
            let tree = ExtensionTreeDigest::from_bytes(bytes);
            prop_assert_eq!(authority.bytes(), bytes);
            prop_assert_eq!(key.bytes(), bytes);
            prop_assert_eq!(archive.bytes(), bytes);
            prop_assert_eq!(manifest.bytes(), bytes);
            prop_assert_eq!(tree.bytes(), bytes);
            prop_assert_eq!(authority.as_bytes(), &bytes);
            prop_assert_eq!(key.as_bytes(), &bytes);
            prop_assert_eq!(archive.as_bytes(), &bytes);
            prop_assert_eq!(manifest.as_bytes(), &bytes);
            prop_assert_eq!(tree.as_bytes(), &bytes);
        }

        #[test]
        fn valid_revision_round_trips(value in 1_u64..=i64::MAX as u64) {
            let revision = ExtensionPackageRevision::new(value)
                .expect("generated durable revision");
            prop_assert_eq!(revision.get(), value);
            if value < i64::MAX as u64 {
                prop_assert_eq!(revision.next().map(ExtensionPackageRevision::get), Some(value + 1));
            } else {
                prop_assert_eq!(revision.next(), None);
            }
        }
    }
}
