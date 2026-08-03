//! Deterministic closed inventory encoding for authenticated catalogs.

use sha2::{Digest, Sha256};
use zephium_core::extensions::ExtensionPackagePayloadIdentity;
use zephium_extension_package::{ExtensionReleaseCatalog, ExtensionReleaseLegalArtifactKind};

use crate::BundledCatalogInventoryDigest;

const INVENTORY_DOMAIN: &[u8] = b"zephium.bundled-extension-inventory.v2\0";

pub(crate) fn digest_catalog_inventory(
    catalog: &ExtensionReleaseCatalog,
) -> Option<BundledCatalogInventoryDigest> {
    let mut digest = Sha256::new();
    digest.update(INVENTORY_DOMAIN);
    update_usize(&mut digest, catalog.packages().len())?;

    for package in catalog.packages() {
        let identity = package.identity();
        digest.update(identity.authority().as_bytes());
        digest.update(identity.key().as_bytes());
        update_u64(&mut digest, identity.revision().get());
        match identity.payload() {
            ExtensionPackagePayloadIdentity::BundledTree => digest.update([1]),
            ExtensionPackagePayloadIdentity::AcquiredZip { length, sha256 } => {
                digest.update([2]);
                update_u64(&mut digest, length.get());
                digest.update(sha256.as_bytes());
            }
        }
        digest.update(identity.manifest_sha256().as_bytes());
        digest.update(identity.tree_sha256().as_bytes());
        digest.update(package.tree_index_sha256().as_bytes());
        update_u64(&mut digest, package.tree_index_length());
        update_usize(&mut digest, package.tree_file_count())?;
        update_u64(&mut digest, package.tree_bytes());

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
        update_u64(&mut digest, notice.length());
        digest.update(notice.sha256());

        match provenance.corresponding_source() {
            Some(source) => {
                digest.update([1]);
                update_str(&mut digest, source.url())?;
                update_str(&mut digest, source.revision())?;
            }
            None => digest.update([0]),
        }
    }

    Some(BundledCatalogInventoryDigest::from_bytes(
        digest.finalize().into(),
    ))
}

fn update_str(digest: &mut Sha256, value: &str) -> Option<()> {
    update_usize(digest, value.len())?;
    digest.update(value.as_bytes());
    Some(())
}

fn update_usize(digest: &mut Sha256, value: usize) -> Option<()> {
    update_u64(digest, u64::try_from(value).ok()?);
    Some(())
}

fn update_u64(digest: &mut Sha256, value: u64) {
    digest.update(value.to_be_bytes());
}
