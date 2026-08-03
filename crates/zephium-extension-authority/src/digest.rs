//! Stable digest identities owned by the bundled authority.

use std::fmt;

/// SHA-256 of the deterministic, closed package inventory in one catalog.
///
/// This digest is structural. Constructing one from durable bytes does not
/// authenticate a catalog and cannot create an admission or activation
/// capability.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BundledCatalogInventoryDigest([u8; 32]);

impl BundledCatalogInventoryDigest {
    /// Reconstructs an exact structural digest from durable bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the exact digest bytes.
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    /// Borrows the exact digest bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for BundledCatalogInventoryDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "BundledCatalogInventoryDigest({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}
