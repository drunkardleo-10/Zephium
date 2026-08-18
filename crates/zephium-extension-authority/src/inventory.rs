//! Deterministic closed inventory encoding for authenticated catalogs.

use zephium_extension_package::ExtensionReleaseCatalog;

use crate::BundledCatalogInventoryDigest;

pub(crate) fn digest_catalog_inventory(
    catalog: &ExtensionReleaseCatalog,
) -> Option<BundledCatalogInventoryDigest> {
    matches!(catalog.schema_version(), 1 | 2)
        .then(|| BundledCatalogInventoryDigest::from_bytes(catalog.inventory_sha256()))
}
