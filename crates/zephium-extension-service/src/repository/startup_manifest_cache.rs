//! Bounded, attempt-local authentication reuse. Sibling package receipts can be
//! consumed once by their later activation, avoiding repeated CRX decompression
//! and transformation. Selection still verifies all sealed bytes; native
//! admission retains its independent checks. No grants or native leases live here.
use std::sync::Arc;
use zephium_core::extensions::{
    ExtensionBetaObjectDigest, ExtensionManifestDescriptor, ExtensionUpstreamCheckpoint,
};
use zephium_extension_repository::beta::StoredBetaPackage;

const MAX_ENTRIES: usize = 32;
const MAX_BYTES: usize = 1024 * 1024;
const MAX_PACKAGE_BYTES: usize = 8 * 1024 * 1024;

struct Entry {
    object: ExtensionBetaObjectDigest,
    high_water: ExtensionUpstreamCheckpoint,
    manifest: Arc<ExtensionManifestDescriptor>,
    package: Option<StoredBetaPackage>,
}

#[derive(Default)]
pub(super) struct StartupManifestCache {
    entries: Vec<Entry>,
    bytes: usize,
    package_bytes: usize,
}

impl StartupManifestCache {
    pub(super) fn take_package(
        &mut self,
        object: ExtensionBetaObjectDigest,
        high_water: ExtensionUpstreamCheckpoint,
    ) -> Option<StoredBetaPackage> {
        let package = self
            .entries
            .iter_mut()
            .find(|entry| entry.object == object && entry.high_water == high_water)?
            .package
            .take()?;
        self.package_bytes -= package.retained_bytes();
        Some(package)
    }

    pub(super) fn retain_package(
        &mut self,
        object: ExtensionBetaObjectDigest,
        high_water: ExtensionUpstreamCheckpoint,
        package: StoredBetaPackage,
    ) {
        let bytes = package.retained_bytes();
        if self.package_bytes.saturating_add(bytes) > MAX_PACKAGE_BYTES {
            return;
        }
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.object == object && entry.high_water == high_water)
        else {
            return;
        };
        if entry.package.is_none() {
            entry.package = Some(package);
            self.package_bytes += bytes;
        }
    }
    pub(super) fn get(
        &self,
        object: ExtensionBetaObjectDigest,
        high_water: ExtensionUpstreamCheckpoint,
    ) -> Option<Arc<ExtensionManifestDescriptor>> {
        self.entries
            .iter()
            .find(|entry| entry.object == object && entry.high_water == high_water)
            .map(|entry| Arc::clone(&entry.manifest))
    }

    pub(super) fn insert(
        &mut self,
        object: ExtensionBetaObjectDigest,
        high_water: ExtensionUpstreamCheckpoint,
        manifest: Arc<ExtensionManifestDescriptor>,
    ) {
        if self
            .entries
            .iter()
            .any(|entry| entry.object == object && entry.high_water == high_water)
        {
            return;
        }
        let bytes = manifest
            .retained_bytes()
            .saturating_add(std::mem::size_of::<Entry>())
            .saturating_add(2 * std::mem::size_of::<usize>()); // Arc counters.
        if self.entries.len() >= MAX_ENTRIES || self.bytes.saturating_add(bytes) > MAX_BYTES {
            return; // Fall back to full authentication; never relax admission.
        }
        // Reserve exactly, avoiding hidden geometric capacity outside the cap.
        self.entries.reserve_exact(1);
        self.entries.push(Entry {
            object,
            high_water,
            manifest,
            package: None,
        });
        self.bytes += bytes;
    }
}
