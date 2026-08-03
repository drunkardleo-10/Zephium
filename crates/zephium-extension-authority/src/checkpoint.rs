//! Structural durable checkpoint and monotonic admission classification.

use zephium_core::extensions::ExtensionAuthorityId;
use zephium_extension_package::{ExtensionReleaseCatalogDigest, ExtensionReleaseCatalogRevision};

use crate::BundledCatalogInventoryDigest;

/// Durable structural high-water mark for a bundled catalog authority.
///
/// Adapters may reconstruct this value from authenticated durable state. The
/// constructor performs no authentication: only an admitted catalog witness
/// may originate a new trusted checkpoint for persistence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BundledCatalogCheckpoint {
    authority: ExtensionAuthorityId,
    revision: ExtensionReleaseCatalogRevision,
    catalog_digest: ExtensionReleaseCatalogDigest,
    inventory_digest: BundledCatalogInventoryDigest,
}

impl BundledCatalogCheckpoint {
    /// Reconstructs structural checkpoint fields read from durable state.
    ///
    /// Possession of this value is not catalog or package authority.
    pub const fn from_parts(
        authority: ExtensionAuthorityId,
        revision: ExtensionReleaseCatalogRevision,
        catalog_digest: ExtensionReleaseCatalogDigest,
        inventory_digest: BundledCatalogInventoryDigest,
    ) -> Self {
        Self {
            authority,
            revision,
            catalog_digest,
            inventory_digest,
        }
    }

    /// Returns the trust-domain and epoch identity.
    pub const fn authority(self) -> ExtensionAuthorityId {
        self.authority
    }

    /// Returns the durable release-catalog revision.
    pub const fn revision(self) -> ExtensionReleaseCatalogRevision {
        self.revision
    }

    /// Returns SHA-256 of exact canonical catalog bytes.
    pub const fn catalog_digest(self) -> ExtensionReleaseCatalogDigest {
        self.catalog_digest
    }

    /// Returns the redundant deterministic package-inventory digest.
    pub const fn inventory_digest(self) -> BundledCatalogInventoryDigest {
        self.inventory_digest
    }

    pub(crate) fn classify(self, candidate: &Self) -> BundledCatalogDisposition {
        if self.authority != candidate.authority {
            return BundledCatalogDisposition::AuthorityMismatch;
        }
        if candidate.revision < self.revision {
            return BundledCatalogDisposition::Rollback;
        }
        if candidate.revision > self.revision {
            return BundledCatalogDisposition::Candidate;
        }
        if self.catalog_digest == candidate.catalog_digest
            && self.inventory_digest == candidate.inventory_digest
        {
            BundledCatalogDisposition::IdempotentReplay
        } else {
            BundledCatalogDisposition::Equivocation
        }
    }
}

/// Monotonic classification against a durable bundled-catalog checkpoint.
///
/// Only [`Self::Candidate`] and [`Self::IdempotentReplay`] are non-rejection
/// outcomes. Persistence and candidate/current transitions belong to a later
/// repository layer and are not claimed by this status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "rollback, equivocation, and authority mismatches must be handled"]
pub enum BundledCatalogDisposition {
    /// No checkpoint exists, or the admitted revision is strictly higher.
    Candidate,
    /// Revision and complete catalog identity exactly match the checkpoint.
    IdempotentReplay,
    /// The admitted revision is below the durable floor.
    Rollback,
    /// The revision is equal but catalog identity differs.
    Equivocation,
    /// The checkpoint belongs to a different authority epoch.
    AuthorityMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkpoint(
        authority: u8,
        revision: u64,
        catalog: u8,
        inventory: u8,
    ) -> BundledCatalogCheckpoint {
        BundledCatalogCheckpoint::from_parts(
            ExtensionAuthorityId::from_bytes([authority; 32]),
            ExtensionReleaseCatalogRevision::new(revision).unwrap(),
            ExtensionReleaseCatalogDigest::from_bytes([catalog; 32]),
            BundledCatalogInventoryDigest::from_bytes([inventory; 32]),
        )
    }

    #[test]
    fn checkpoint_classification_is_monotonic_and_identity_complete() {
        let floor = checkpoint(1, 5, 2, 3);
        assert_eq!(
            floor.classify(&checkpoint(1, 4, 2, 3)),
            BundledCatalogDisposition::Rollback
        );
        assert_eq!(
            floor.classify(&checkpoint(1, 5, 4, 3)),
            BundledCatalogDisposition::Equivocation
        );
        assert_eq!(
            floor.classify(&checkpoint(1, 5, 2, 4)),
            BundledCatalogDisposition::Equivocation
        );
        assert_eq!(
            floor.classify(&checkpoint(1, 5, 2, 3)),
            BundledCatalogDisposition::IdempotentReplay
        );
        assert_eq!(
            floor.classify(&checkpoint(1, 6, 9, 9)),
            BundledCatalogDisposition::Candidate
        );
        assert_eq!(
            floor.classify(&checkpoint(9, 6, 2, 3)),
            BundledCatalogDisposition::AuthorityMismatch
        );
    }
}
