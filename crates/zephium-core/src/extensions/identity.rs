//! Structural identity for one immutable extension package release.

use std::fmt;

/// Exact byte length of every SHA-256 value in an extension package identity.
pub const EXTENSION_SHA256_BYTES: usize = 32;

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
    "SHA-256 of the exact acquired extension-package archive."
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

/// Immutable structural identity of one exact extension package release.
///
/// `authority` and `key` identify the update line. `revision` orders releases
/// within that line. The three digests bind the acquired archive, admitted
/// manifest, and canonical materialized resource tree independently so a
/// native adapter can never treat a path or manifest version string as
/// package authority.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionPackageIdentity {
    authority: ExtensionAuthorityId,
    key: ExtensionPackageKey,
    revision: ExtensionPackageRevision,
    archive_sha256: ExtensionArchiveDigest,
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
        archive_sha256: ExtensionArchiveDigest,
        manifest_sha256: ExtensionManifestDigest,
        tree_sha256: ExtensionTreeDigest,
    ) -> Self {
        Self {
            authority,
            key,
            revision,
            archive_sha256,
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

    pub const fn archive_sha256(&self) -> ExtensionArchiveDigest {
        self.archive_sha256
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
            archive_digest(3),
            manifest_digest(4),
            tree_digest(5),
        );
        let changed_tree = ExtensionPackageIdentity::new(
            base.authority(),
            base.key(),
            base.revision(),
            base.archive_sha256(),
            base.manifest_sha256(),
            tree_digest(6),
        );
        assert_ne!(base, changed_tree);
        assert_eq!(base.update_line(), changed_tree.update_line());
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
