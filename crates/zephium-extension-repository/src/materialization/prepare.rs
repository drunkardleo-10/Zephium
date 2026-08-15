//! Exact, authority-preserving preflight for bundled package materialization.

use std::io::Read;

use sha2::{Digest, Sha256};
use thiserror::Error;
use zephium_core::extensions::{ExtensionPackageKey, ExtensionPackagePayloadIdentity};
#[cfg(feature = "acquired-packages")]
use zephium_extension_acquisition::AcquiredExtensionTreeReceipt;
#[cfg(feature = "acquired-packages")]
use zephium_extension_authority::AdmittedAcquiredCatalog;
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, ProductAdmittedExtensionManifest,
    ProductAdmittedRollbackExtensionManifest, ProductExtensionManifestAdmissionError,
    ProductExtensionManifestAuthority, ProductExtensionManifestAuthorityError,
    ProductExtensionRuntimeTarget,
};
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, ExtensionReleaseCatalog, ExtensionReleaseCatalogError,
    ExtensionReleaseLegalArtifactKind, ExtensionReleasePackage, ExtensionTreeIndexError,
    MAX_EXTENSION_MANIFEST_BYTES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

use super::records::{
    CatalogAnchor, LegalArtifactAnchor, ManifestAnchor, PackageIdentityAnchor, PackageRecord,
    StoredLegalArtifactKind, StoredPayloadIdentity, StoredRuntimeTarget, TreeIndexAnchor,
    PACKAGE_RECORD_SCHEMA_VERSION,
};
use super::source::{
    BundledReleaseByteSource, BundledReleaseCatalogSourceIdentity,
    BundledReleasePackageSourceIdentity, BundledReleaseResource, BundledReleaseSourceError,
};
use crate::operation::with_external_callback;
use crate::state::{digest_package_row, Digest32};

/// Stable, path-free failure while preparing exact bundled package bytes.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub(crate) enum PreparationError {
    #[error("exact catalog bytes differ from the admitted catalog length")]
    CatalogLengthMismatch,
    #[error("exact catalog bytes differ from the admitted catalog digest")]
    CatalogDigestMismatch,
    #[error("the requested package is absent from the admitted catalog")]
    PackageMissing,
    #[error("the requested package payload has the wrong representation")]
    UnsupportedPayload,
    #[error("the selected extension runtime target is unsupported")]
    UnsupportedRuntimeTarget,
    #[error("product extension manifest authority is unavailable: {0}")]
    ManifestAuthority(#[source] ProductExtensionManifestAuthorityError),
    #[error("the bundled release source refused an exact resource: {0}")]
    Source(#[source] BundledReleaseSourceError),
    #[error("reading an exact bundled release resource failed")]
    Read,
    #[error("a bundled release resource has a different exact length")]
    ResourceLengthMismatch,
    #[error("a bundled release resource has a different exact digest")]
    ResourceDigestMismatch,
    #[error("the canonical extension tree index is invalid: {0}")]
    TreeIndex(#[source] ExtensionTreeIndexError),
    #[error("the canonical extension tree index differs from the catalog package")]
    TreeIndexBinding,
    #[error("product extension manifest admission failed: {0}")]
    ManifestAdmission(#[source] ProductExtensionManifestAdmissionError),
    #[error("bounded extension preparation accounting overflowed")]
    AccountingOverflow,
    #[error("the prepared package metadata record is internally inconsistent")]
    InvalidRecord,
}

/// Exact active-package preflight output.
///
/// The product witness is deliberately retained in this non-`Clone` value and
/// cannot be confused with rollback admission at a type-correct call site.
#[must_use = "prepared active package authority must be materialized or discarded"]
pub(crate) struct PreparedActivePackage {
    data: PreparedPackageData,
    _manifest: ProductAdmittedExtensionManifest,
}

/// Exact active acquired-package preflight.
///
/// The disposable filesystem stage is deliberately not embedded in this
/// authority witness. Keeping it separate lets the writer remove the stage on
/// every clean pre-intent refusal instead of losing the only cleanup handle
/// inside a failed preparation.
#[cfg(feature = "acquired-packages")]
#[must_use = "prepared acquired package authority must be materialized or discarded"]
pub(crate) struct PreparedAcquiredActivePackage {
    data: PreparedPackageData,
    _manifest: ProductAdmittedExtensionManifest,
}

#[cfg(feature = "acquired-packages")]
impl PreparedAcquiredActivePackage {
    pub(crate) const fn manifest(&self) -> &ProductAdmittedExtensionManifest {
        &self._manifest
    }

    pub(super) const fn package_source(&self) -> BundledReleasePackageSourceIdentity {
        self.data.package_source
    }

    pub(super) const fn tree_index(&self) -> &CanonicalExtensionTreeIndex {
        &self.data.tree_index
    }

    pub(super) fn tree_index_bytes(&self) -> &[u8] {
        &self.data.tree_index_bytes
    }

    pub(super) fn manifest_bytes(&self) -> &[u8] {
        &self.data.manifest_bytes
    }

    pub(crate) const fn record(&self) -> &PackageRecord {
        &self.data.record
    }

    pub(crate) fn into_lease_parts(
        self,
    ) -> (
        CanonicalExtensionTreeIndex,
        ProductAdmittedExtensionManifest,
    ) {
        (self.data.tree_index, self._manifest)
    }
}

impl PreparedActivePackage {
    pub(crate) const fn manifest(&self) -> &ProductAdmittedExtensionManifest {
        &self._manifest
    }

    pub(super) const fn package_source(&self) -> BundledReleasePackageSourceIdentity {
        self.data.package_source
    }

    pub(super) const fn tree_index(&self) -> &CanonicalExtensionTreeIndex {
        &self.data.tree_index
    }

    pub(super) fn tree_index_bytes(&self) -> &[u8] {
        &self.data.tree_index_bytes
    }

    pub(super) fn manifest_bytes(&self) -> &[u8] {
        &self.data.manifest_bytes
    }

    pub(crate) const fn record(&self) -> &PackageRecord {
        &self.data.record
    }

    pub(crate) fn into_lease_parts(
        self,
    ) -> (
        CanonicalExtensionTreeIndex,
        ProductAdmittedExtensionManifest,
    ) {
        (self.data.tree_index, self._manifest)
    }
}

/// Exact explicitly approved rollback-package preflight output.
///
/// The rollback product witness remains distinct and non-`Clone`; this type
/// has no conversion to [`PreparedActivePackage`].
#[must_use = "prepared rollback package authority must be materialized or discarded"]
pub(crate) struct PreparedRollbackPackage {
    data: PreparedPackageData,
    _manifest: ProductAdmittedRollbackExtensionManifest,
}

impl PreparedRollbackPackage {
    pub(crate) const fn manifest(&self) -> &ProductAdmittedRollbackExtensionManifest {
        &self._manifest
    }

    pub(super) const fn package_source(&self) -> BundledReleasePackageSourceIdentity {
        self.data.package_source
    }

    pub(super) const fn tree_index(&self) -> &CanonicalExtensionTreeIndex {
        &self.data.tree_index
    }

    pub(super) fn tree_index_bytes(&self) -> &[u8] {
        &self.data.tree_index_bytes
    }

    pub(super) fn manifest_bytes(&self) -> &[u8] {
        &self.data.manifest_bytes
    }

    pub(crate) const fn record(&self) -> &PackageRecord {
        &self.data.record
    }

    pub(crate) fn into_lease_parts(
        self,
    ) -> (
        CanonicalExtensionTreeIndex,
        ProductAdmittedRollbackExtensionManifest,
    ) {
        (self.data.tree_index, self._manifest)
    }
}

struct PreparedPackageData {
    package_source: BundledReleasePackageSourceIdentity,
    tree_index: CanonicalExtensionTreeIndex,
    tree_index_bytes: Box<[u8]>,
    manifest_bytes: Box<[u8]>,
    record: PackageRecord,
}

struct LoadedPackageBytes {
    package_source: BundledReleasePackageSourceIdentity,
    tree_index: CanonicalExtensionTreeIndex,
    tree_index_bytes: Box<[u8]>,
    manifest_bytes: Box<[u8]>,
}

pub(crate) fn open_product_manifest_authority(
) -> Result<ProductExtensionManifestAuthority, PreparationError> {
    ProductExtensionManifestAuthority::product().map_err(PreparationError::ManifestAuthority)
}

pub(crate) fn prepare_active_package<S: BundledReleaseByteSource>(
    catalog: &AdmittedBundledCatalog,
    exact_catalog_bytes: &[u8],
    manifest_authority: &ProductExtensionManifestAuthority,
    runtime_target: ProductExtensionRuntimeTarget,
    package_key: ExtensionPackageKey,
    source: &mut S,
) -> Result<PreparedActivePackage, PreparationError> {
    verify_catalog_bytes(
        catalog.catalog_length(),
        catalog.catalog_digest().bytes(),
        exact_catalog_bytes,
    )?;
    let catalog_source =
        BundledReleaseCatalogSourceIdentity::from_generation(catalog.generation_anchor())
            .ok_or(PreparationError::AccountingOverflow)?;
    let loaded = load_package_bytes(catalog.catalog(), catalog_source, package_key, source)?;
    prepare_active_loaded(
        catalog,
        manifest_authority,
        runtime_target,
        package_key,
        loaded,
    )
}

pub(crate) fn prepare_active_package_from_preparsed(
    catalog: &AdmittedBundledCatalog,
    manifest_authority: &ProductExtensionManifestAuthority,
    runtime_target: ProductExtensionRuntimeTarget,
    package_key: ExtensionPackageKey,
    tree_index: CanonicalExtensionTreeIndex,
    tree_index_bytes: Box<[u8]>,
    manifest_bytes: Box<[u8]>,
) -> Result<PreparedActivePackage, PreparationError> {
    let catalog_source =
        BundledReleaseCatalogSourceIdentity::from_generation(catalog.generation_anchor())
            .ok_or(PreparationError::AccountingOverflow)?;
    let loaded = bind_preparsed_package_bytes(
        catalog.catalog(),
        catalog_source,
        package_key,
        ExpectedPayloadClass::BundledTree,
        tree_index,
        tree_index_bytes,
        manifest_bytes,
    )?;
    prepare_active_loaded(
        catalog,
        manifest_authority,
        runtime_target,
        package_key,
        loaded,
    )
}

fn prepare_active_loaded(
    catalog: &AdmittedBundledCatalog,
    manifest_authority: &ProductExtensionManifestAuthority,
    runtime_target: ProductExtensionRuntimeTarget,
    package_key: ExtensionPackageKey,
    loaded: LoadedPackageBytes,
) -> Result<PreparedActivePackage, PreparationError> {
    let catalog_source = loaded.package_source.catalog();
    let manifest = manifest_authority
        .admit_manifest(
            catalog,
            runtime_target,
            package_key,
            &loaded.tree_index,
            &loaded.manifest_bytes,
        )
        .map_err(PreparationError::ManifestAdmission)?;
    let package = catalog
        .catalog()
        .package(package_key)
        .ok_or(PreparationError::PackageMissing)?;
    validate_active_witness(catalog_source, package, &loaded, runtime_target, &manifest)?;
    let record = package_record(
        catalog_source,
        package,
        &loaded,
        manifest.runtime_target(),
        manifest.compatibility_target().as_str(),
        manifest.admission_digest().bytes(),
    )?;

    Ok(PreparedActivePackage {
        data: PreparedPackageData {
            package_source: loaded.package_source,
            tree_index: loaded.tree_index,
            tree_index_bytes: loaded.tree_index_bytes,
            manifest_bytes: loaded.manifest_bytes,
            record,
        },
        _manifest: manifest,
    })
}

#[cfg(feature = "acquired-packages")]
pub(crate) fn prepare_acquired_active_package(
    catalog: &AdmittedAcquiredCatalog,
    exact_catalog_bytes: &[u8],
    manifest_authority: &ProductExtensionManifestAuthority,
    runtime_target: ProductExtensionRuntimeTarget,
    package_key: ExtensionPackageKey,
    receipt: AcquiredExtensionTreeReceipt,
    manifest_bytes: Box<[u8]>,
) -> Result<PreparedAcquiredActivePackage, PreparationError> {
    verify_catalog_bytes(
        catalog.catalog_length(),
        catalog.catalog_digest().bytes(),
        exact_catalog_bytes,
    )?;
    let catalog_source =
        BundledReleaseCatalogSourceIdentity::from_generation(catalog.generation_anchor())
            .ok_or(PreparationError::AccountingOverflow)?;
    let package = catalog
        .catalog()
        .package(package_key)
        .ok_or(PreparationError::PackageMissing)?;
    let (tree_index_bytes, tree_index) = receipt
        .into_release_tree_artifacts(package)
        .map_err(|_| PreparationError::InvalidRecord)?;
    let loaded = bind_preparsed_package_bytes(
        catalog.catalog(),
        catalog_source,
        package_key,
        ExpectedPayloadClass::AcquiredZip,
        tree_index,
        tree_index_bytes,
        manifest_bytes,
    )?;
    prepare_acquired_loaded(
        catalog,
        manifest_authority,
        runtime_target,
        package_key,
        loaded,
    )
}

#[cfg(feature = "acquired-packages")]
pub(crate) fn prepare_acquired_active_package_from_preparsed(
    catalog: &AdmittedAcquiredCatalog,
    manifest_authority: &ProductExtensionManifestAuthority,
    runtime_target: ProductExtensionRuntimeTarget,
    package_key: ExtensionPackageKey,
    tree_index: CanonicalExtensionTreeIndex,
    tree_index_bytes: Box<[u8]>,
    manifest_bytes: Box<[u8]>,
) -> Result<PreparedAcquiredActivePackage, PreparationError> {
    let catalog_source =
        BundledReleaseCatalogSourceIdentity::from_generation(catalog.generation_anchor())
            .ok_or(PreparationError::AccountingOverflow)?;
    let loaded = bind_preparsed_package_bytes(
        catalog.catalog(),
        catalog_source,
        package_key,
        ExpectedPayloadClass::AcquiredZip,
        tree_index,
        tree_index_bytes,
        manifest_bytes,
    )?;
    prepare_acquired_loaded(
        catalog,
        manifest_authority,
        runtime_target,
        package_key,
        loaded,
    )
}

#[cfg(feature = "acquired-packages")]
fn prepare_acquired_loaded(
    catalog: &AdmittedAcquiredCatalog,
    manifest_authority: &ProductExtensionManifestAuthority,
    runtime_target: ProductExtensionRuntimeTarget,
    package_key: ExtensionPackageKey,
    loaded: LoadedPackageBytes,
) -> Result<PreparedAcquiredActivePackage, PreparationError> {
    let catalog_source = loaded.package_source.catalog();
    let package = catalog
        .catalog()
        .package(package_key)
        .ok_or(PreparationError::PackageMissing)?;
    let manifest = manifest_authority
        .admit_acquired_manifest(
            catalog,
            runtime_target,
            package_key,
            &loaded.tree_index,
            &loaded.manifest_bytes,
        )
        .map_err(PreparationError::ManifestAdmission)?;
    validate_active_witness(catalog_source, package, &loaded, runtime_target, &manifest)?;
    let record = package_record(
        catalog_source,
        package,
        &loaded,
        manifest.runtime_target(),
        manifest.compatibility_target().as_str(),
        manifest.admission_digest().bytes(),
    )?;
    let LoadedPackageBytes {
        package_source,
        tree_index,
        tree_index_bytes,
        manifest_bytes,
    } = loaded;
    Ok(PreparedAcquiredActivePackage {
        data: PreparedPackageData {
            package_source,
            tree_index,
            tree_index_bytes,
            manifest_bytes,
            record,
        },
        _manifest: manifest,
    })
}

pub(crate) fn prepare_rollback_package<S: BundledReleaseByteSource>(
    catalog: &AdmittedRollbackBundledCatalog,
    exact_catalog_bytes: &[u8],
    manifest_authority: &ProductExtensionManifestAuthority,
    runtime_target: ProductExtensionRuntimeTarget,
    package_key: ExtensionPackageKey,
    source: &mut S,
) -> Result<PreparedRollbackPackage, PreparationError> {
    verify_catalog_bytes(
        catalog.catalog_length(),
        catalog.catalog_digest().bytes(),
        exact_catalog_bytes,
    )?;
    let catalog_source =
        BundledReleaseCatalogSourceIdentity::from_generation(catalog.generation_anchor())
            .ok_or(PreparationError::AccountingOverflow)?;
    let loaded = load_package_bytes(catalog.catalog(), catalog_source, package_key, source)?;
    prepare_rollback_loaded(
        catalog,
        manifest_authority,
        runtime_target,
        package_key,
        loaded,
    )
}

pub(crate) fn prepare_rollback_package_from_preparsed(
    catalog: &AdmittedRollbackBundledCatalog,
    manifest_authority: &ProductExtensionManifestAuthority,
    runtime_target: ProductExtensionRuntimeTarget,
    package_key: ExtensionPackageKey,
    tree_index: CanonicalExtensionTreeIndex,
    tree_index_bytes: Box<[u8]>,
    manifest_bytes: Box<[u8]>,
) -> Result<PreparedRollbackPackage, PreparationError> {
    let catalog_source =
        BundledReleaseCatalogSourceIdentity::from_generation(catalog.generation_anchor())
            .ok_or(PreparationError::AccountingOverflow)?;
    let loaded = bind_preparsed_package_bytes(
        catalog.catalog(),
        catalog_source,
        package_key,
        ExpectedPayloadClass::BundledTree,
        tree_index,
        tree_index_bytes,
        manifest_bytes,
    )?;
    prepare_rollback_loaded(
        catalog,
        manifest_authority,
        runtime_target,
        package_key,
        loaded,
    )
}

fn prepare_rollback_loaded(
    catalog: &AdmittedRollbackBundledCatalog,
    manifest_authority: &ProductExtensionManifestAuthority,
    runtime_target: ProductExtensionRuntimeTarget,
    package_key: ExtensionPackageKey,
    loaded: LoadedPackageBytes,
) -> Result<PreparedRollbackPackage, PreparationError> {
    let catalog_source = loaded.package_source.catalog();
    let manifest = manifest_authority
        .admit_rollback_manifest(
            catalog,
            runtime_target,
            package_key,
            &loaded.tree_index,
            &loaded.manifest_bytes,
        )
        .map_err(PreparationError::ManifestAdmission)?;
    let package = catalog
        .catalog()
        .package(package_key)
        .ok_or(PreparationError::PackageMissing)?;
    validate_rollback_witness(catalog_source, package, &loaded, runtime_target, &manifest)?;
    let record = package_record(
        catalog_source,
        package,
        &loaded,
        manifest.runtime_target(),
        manifest.compatibility_target().as_str(),
        manifest.admission_digest().bytes(),
    )?;

    Ok(PreparedRollbackPackage {
        data: PreparedPackageData {
            package_source: loaded.package_source,
            tree_index: loaded.tree_index,
            tree_index_bytes: loaded.tree_index_bytes,
            manifest_bytes: loaded.manifest_bytes,
            record,
        },
        _manifest: manifest,
    })
}

fn verify_catalog_bytes(
    expected_length: u64,
    expected_digest: [u8; 32],
    exact_catalog_bytes: &[u8],
) -> Result<(), PreparationError> {
    if u64::try_from(exact_catalog_bytes.len()).ok() != Some(expected_length) {
        return Err(PreparationError::CatalogLengthMismatch);
    }
    if <[u8; 32]>::from(Sha256::digest(exact_catalog_bytes)) != expected_digest {
        return Err(PreparationError::CatalogDigestMismatch);
    }
    Ok(())
}

fn load_package_bytes<S: BundledReleaseByteSource>(
    catalog: &ExtensionReleaseCatalog,
    catalog_source: BundledReleaseCatalogSourceIdentity,
    package_key: ExtensionPackageKey,
    source: &mut S,
) -> Result<LoadedPackageBytes, PreparationError> {
    let (package, package_source) = exact_package_source(
        catalog,
        catalog_source,
        package_key,
        ExpectedPayloadClass::BundledTree,
    )?;

    let tree_index_bytes = read_exact_resource(
        source,
        BundledReleaseResource::tree_index(
            package_source,
            package.tree_index_length(),
            package.tree_index_sha256(),
        ),
        MAX_EXTENSION_TREE_INDEX_BYTES as u64,
    )?;
    let tree_index = CanonicalExtensionTreeIndex::parse_canonical(&tree_index_bytes)
        .map_err(PreparationError::TreeIndex)?;
    package
        .bind_tree_index(&tree_index)
        .map_err(map_tree_binding_error)?;

    let manifest = tree_index
        .files()
        .iter()
        .find(|file| file.path().as_str() == "manifest.json")
        .ok_or(PreparationError::TreeIndexBinding)?;
    let manifest_bytes = read_exact_resource(
        source,
        BundledReleaseResource::tree_file(
            package_source,
            manifest.path(),
            manifest.length(),
            manifest.sha256(),
        ),
        MAX_EXTENSION_MANIFEST_BYTES as u64,
    )?;

    Ok(LoadedPackageBytes {
        package_source,
        tree_index,
        tree_index_bytes,
        manifest_bytes,
    })
}

fn bind_preparsed_package_bytes(
    catalog: &ExtensionReleaseCatalog,
    catalog_source: BundledReleaseCatalogSourceIdentity,
    package_key: ExtensionPackageKey,
    expected_payload: ExpectedPayloadClass,
    tree_index: CanonicalExtensionTreeIndex,
    tree_index_bytes: Box<[u8]>,
    manifest_bytes: Box<[u8]>,
) -> Result<LoadedPackageBytes, PreparationError> {
    // Repository lease/bootstrap reads already parsed these exact canonical
    // bytes once. Rebind the retained bytes, parsed index, admitted catalog,
    // and manifest here without repeating parser work or accepting a public
    // caller-supplied witness.
    let (package, package_source) =
        exact_package_source(catalog, catalog_source, package_key, expected_payload)?;
    let index_length =
        u64::try_from(tree_index_bytes.len()).map_err(|_| PreparationError::AccountingOverflow)?;
    if index_length == 0
        || index_length > MAX_EXTENSION_TREE_INDEX_BYTES as u64
        || index_length != tree_index.index_bytes()
    {
        return Err(PreparationError::ResourceLengthMismatch);
    }
    if <[u8; 32]>::from(Sha256::digest(&tree_index_bytes)) != tree_index.index_sha256().bytes() {
        return Err(PreparationError::ResourceDigestMismatch);
    }
    package
        .bind_tree_index(&tree_index)
        .map_err(map_tree_binding_error)?;

    let manifest = tree_index
        .files()
        .iter()
        .find(|file| file.path().as_str() == "manifest.json")
        .ok_or(PreparationError::TreeIndexBinding)?;
    let manifest_length =
        u64::try_from(manifest_bytes.len()).map_err(|_| PreparationError::AccountingOverflow)?;
    if manifest_length == 0
        || manifest_length > MAX_EXTENSION_MANIFEST_BYTES as u64
        || manifest_length != manifest.length()
    {
        return Err(PreparationError::ResourceLengthMismatch);
    }
    if <[u8; 32]>::from(Sha256::digest(&manifest_bytes)) != manifest.sha256() {
        return Err(PreparationError::ResourceDigestMismatch);
    }

    Ok(LoadedPackageBytes {
        package_source,
        tree_index,
        tree_index_bytes,
        manifest_bytes,
    })
}

fn exact_package_source(
    catalog: &ExtensionReleaseCatalog,
    catalog_source: BundledReleaseCatalogSourceIdentity,
    package_key: ExtensionPackageKey,
    expected_payload: ExpectedPayloadClass,
) -> Result<
    (
        &ExtensionReleasePackage,
        BundledReleasePackageSourceIdentity,
    ),
    PreparationError,
> {
    let package = catalog
        .package(package_key)
        .ok_or(PreparationError::PackageMissing)?;
    if !expected_payload.matches(package.payload()) {
        return Err(PreparationError::UnsupportedPayload);
    }
    let package_row = digest_package_row(package)
        .map_err(|_| PreparationError::AccountingOverflow)?
        .bytes();
    let package_source = BundledReleasePackageSourceIdentity::from_package(
        catalog_source,
        package.identity(),
        package_row,
    )
    .ok_or(PreparationError::AccountingOverflow)?;
    Ok((package, package_source))
}

#[derive(Clone, Copy)]
enum ExpectedPayloadClass {
    BundledTree,
    #[cfg(feature = "acquired-packages")]
    AcquiredZip,
}

impl ExpectedPayloadClass {
    const fn matches(self, payload: ExtensionPackagePayloadIdentity) -> bool {
        match (self, payload) {
            (Self::BundledTree, ExtensionPackagePayloadIdentity::BundledTree) => true,
            #[cfg(feature = "acquired-packages")]
            (Self::AcquiredZip, ExtensionPackagePayloadIdentity::AcquiredZip { .. }) => true,
            _ => false,
        }
    }
}

fn read_exact_resource<S: BundledReleaseByteSource>(
    source: &mut S,
    resource: BundledReleaseResource<'_>,
    maximum_length: u64,
) -> Result<Box<[u8]>, PreparationError> {
    let expected_length = resource.kind().expected_length();
    let expected_digest = resource.kind().expected_sha256();
    if expected_length == 0 || expected_length > maximum_length {
        return Err(PreparationError::ResourceLengthMismatch);
    }
    with_external_callback(|| {
        source.with_resource(resource, |reader| {
            read_exact_bytes(reader, expected_length, expected_digest)
        })
    })
    .map_err(PreparationError::Source)?
}

fn read_exact_bytes(
    reader: &mut dyn Read,
    expected_length: u64,
    expected_digest: [u8; 32],
) -> Result<Box<[u8]>, PreparationError> {
    let length =
        usize::try_from(expected_length).map_err(|_| PreparationError::AccountingOverflow)?;
    let mut bytes = vec![0_u8; length].into_boxed_slice();
    let mut offset = 0_usize;
    while offset != length {
        match reader.read(&mut bytes[offset..]) {
            Ok(0) => return Err(PreparationError::ResourceLengthMismatch),
            Ok(read) if read <= length - offset => offset += read,
            Ok(_) => return Err(PreparationError::Read),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(PreparationError::Read),
        }
    }

    let mut eof_probe = [0_u8; 1];
    loop {
        match reader.read(&mut eof_probe) {
            Ok(0) => break,
            Ok(1) => return Err(PreparationError::ResourceLengthMismatch),
            Ok(_) => return Err(PreparationError::Read),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(PreparationError::Read),
        }
    }
    if <[u8; 32]>::from(Sha256::digest(&bytes)) != expected_digest {
        return Err(PreparationError::ResourceDigestMismatch);
    }
    Ok(bytes)
}

fn validate_active_witness(
    catalog: BundledReleaseCatalogSourceIdentity,
    package: &ExtensionReleasePackage,
    loaded: &LoadedPackageBytes,
    requested_target: ProductExtensionRuntimeTarget,
    witness: &ProductAdmittedExtensionManifest,
) -> Result<(), PreparationError> {
    let exact_manifest_digest = <[u8; 32]>::from(Sha256::digest(&loaded.manifest_bytes));
    let chromium_identity_matches = match (witness.chromium_key(), package.chromium()) {
        (None, None) => true,
        (Some(key), Some(expected)) => key.digest() == expected.manifest_key_sha256(),
        _ => false,
    };
    if witness.runtime_target() != requested_target
        || witness.catalog_authority() != catalog.authority()
        || witness.catalog_revision() != catalog.revision()
        || witness.catalog_length() != catalog.catalog_length()
        || witness.catalog_digest() != catalog.catalog_digest()
        || witness.catalog_inventory_digest() != catalog.inventory_digest()
        || witness.package_identity() != package.identity()
        || witness.tree_index_digest() != loaded.tree_index.index_sha256()
        || witness.tree_index_length() != loaded.tree_index.index_bytes()
        || witness.tree_digest() != loaded.tree_index.tree_sha256()
        || witness.manifest_digest() != loaded.tree_index.manifest_sha256()
        || witness.manifest_digest().bytes() != exact_manifest_digest
        || witness.compatibility_target().as_str() != requested_target.compatibility_target_id()
        || !chromium_identity_matches
    {
        return Err(PreparationError::InvalidRecord);
    }
    Ok(())
}

fn validate_rollback_witness(
    catalog: BundledReleaseCatalogSourceIdentity,
    package: &ExtensionReleasePackage,
    loaded: &LoadedPackageBytes,
    requested_target: ProductExtensionRuntimeTarget,
    witness: &ProductAdmittedRollbackExtensionManifest,
) -> Result<(), PreparationError> {
    let exact_manifest_digest = <[u8; 32]>::from(Sha256::digest(&loaded.manifest_bytes));
    let chromium_identity_matches = match (witness.chromium_key(), package.chromium()) {
        (None, None) => true,
        (Some(key), Some(expected)) => key.digest() == expected.manifest_key_sha256(),
        _ => false,
    };
    if witness.runtime_target() != requested_target
        || witness.catalog_authority() != catalog.authority()
        || witness.catalog_revision() != catalog.revision()
        || witness.catalog_length() != catalog.catalog_length()
        || witness.catalog_digest() != catalog.catalog_digest()
        || witness.catalog_inventory_digest() != catalog.inventory_digest()
        || witness.package_identity() != package.identity()
        || witness.tree_index_digest() != loaded.tree_index.index_sha256()
        || witness.tree_index_length() != loaded.tree_index.index_bytes()
        || witness.tree_digest() != loaded.tree_index.tree_sha256()
        || witness.manifest_digest() != loaded.tree_index.manifest_sha256()
        || witness.manifest_digest().bytes() != exact_manifest_digest
        || witness.compatibility_target().as_str() != requested_target.compatibility_target_id()
        || !chromium_identity_matches
    {
        return Err(PreparationError::InvalidRecord);
    }
    Ok(())
}

fn package_record(
    catalog: BundledReleaseCatalogSourceIdentity,
    package: &ExtensionReleasePackage,
    loaded: &LoadedPackageBytes,
    runtime_target: ProductExtensionRuntimeTarget,
    compatibility_target: &str,
    admission_sha256: [u8; 32],
) -> Result<PackageRecord, PreparationError> {
    let runtime_target = stored_runtime_target(runtime_target)?;
    let identity = package.identity();
    let tree_index_length = u64::try_from(loaded.tree_index_bytes.len())
        .map_err(|_| PreparationError::AccountingOverflow)?;
    let tree_index_digest = <[u8; 32]>::from(Sha256::digest(&loaded.tree_index_bytes));
    let manifest = loaded
        .tree_index
        .files()
        .iter()
        .find(|file| file.path().as_str() == "manifest.json")
        .ok_or(PreparationError::InvalidRecord)?;
    let manifest_length = u64::try_from(loaded.manifest_bytes.len())
        .map_err(|_| PreparationError::AccountingOverflow)?;
    let manifest_digest = <[u8; 32]>::from(Sha256::digest(&loaded.manifest_bytes));
    let stored_payload = stored_payload(identity.payload());
    if loaded.package_source.catalog() != catalog
        || loaded.package_source.authority() != identity.authority()
        || loaded.package_source.package_key() != identity.key()
        || loaded.package_source.package_revision() != identity.revision()
        || loaded.package_source.payload() != identity.payload()
        || loaded.package_source.manifest_digest() != identity.manifest_sha256()
        || loaded.package_source.tree_digest() != identity.tree_sha256()
        || loaded.package_source.package_row_sha256()
            != digest_package_row(package)
                .map_err(|_| PreparationError::AccountingOverflow)?
                .bytes()
        || tree_index_length != loaded.tree_index.index_bytes()
        || tree_index_digest != loaded.tree_index.index_sha256().bytes()
        || manifest_length != manifest.length()
        || manifest_digest != manifest.sha256()
    {
        return Err(PreparationError::InvalidRecord);
    }
    let file_count = u32::try_from(loaded.tree_index.files().len())
        .map_err(|_| PreparationError::AccountingOverflow)?;
    let directory_count = u32::try_from(loaded.tree_index.implicit_directory_count())
        .map_err(|_| PreparationError::AccountingOverflow)?;
    let total_entry_count = u32::try_from(loaded.tree_index.total_entry_count())
        .map_err(|_| PreparationError::AccountingOverflow)?;
    let notice = package.provenance().legal_notice();
    let legal_kind = match notice.kind() {
        ExtensionReleaseLegalArtifactKind::NoticeBundle => StoredLegalArtifactKind::NoticeBundle,
    };
    let record = PackageRecord {
        schema_version: PACKAGE_RECORD_SCHEMA_VERSION,
        catalog: CatalogAnchor {
            authority_id: Digest32::from_bytes(catalog.authority().bytes()),
            revision: catalog.revision().get(),
            catalog_length: catalog.catalog_length(),
            catalog_sha256: Digest32::from_bytes(catalog.catalog_digest().bytes()),
            inventory_sha256: Digest32::from_bytes(catalog.inventory_digest().bytes()),
        },
        package: PackageIdentityAnchor {
            authority_id: Digest32::from_bytes(identity.authority().bytes()),
            package_key: Digest32::from_bytes(identity.key().bytes()),
            revision: identity.revision().get(),
            payload: stored_payload,
            manifest_sha256: Digest32::from_bytes(identity.manifest_sha256().bytes()),
            tree_sha256: Digest32::from_bytes(identity.tree_sha256().bytes()),
            package_row_sha256: Digest32::from_bytes(loaded.package_source.package_row_sha256()),
            chromium_manifest_key_sha256: package
                .chromium()
                .map(|expected| Digest32::from_bytes(expected.manifest_key_sha256().bytes())),
        },
        tree_index: TreeIndexAnchor {
            index_sha256: Digest32::from_bytes(loaded.tree_index.index_sha256().bytes()),
            index_length: loaded.tree_index.index_bytes(),
            tree_sha256: Digest32::from_bytes(loaded.tree_index.tree_sha256().bytes()),
            file_count,
            directory_count,
            total_entry_count,
            tree_bytes: loaded.tree_index.total_bytes(),
        },
        manifest: ManifestAnchor {
            runtime_target,
            compatibility_target: compatibility_target.to_owned(),
            manifest_length,
            manifest_sha256: Digest32::from_bytes(identity.manifest_sha256().bytes()),
            admission_sha256: Digest32::from_bytes(admission_sha256),
        },
        legal: LegalArtifactAnchor {
            target: notice.target().as_str().to_owned(),
            kind: legal_kind,
            length: notice.length(),
            sha256: Digest32::from_bytes(notice.sha256()),
        },
    };
    record
        .validate()
        .map_err(|_| PreparationError::InvalidRecord)?;
    Ok(record)
}

fn stored_payload(payload: ExtensionPackagePayloadIdentity) -> StoredPayloadIdentity {
    match payload {
        ExtensionPackagePayloadIdentity::BundledTree => StoredPayloadIdentity::BundledTree,
        ExtensionPackagePayloadIdentity::AcquiredZip { length, sha256 } => {
            StoredPayloadIdentity::AcquiredZip {
                length: length.get(),
                sha256: Digest32::from_bytes(sha256.bytes()),
            }
        }
    }
}

fn stored_runtime_target(
    runtime_target: ProductExtensionRuntimeTarget,
) -> Result<StoredRuntimeTarget, PreparationError> {
    #[allow(unreachable_patterns)]
    match runtime_target {
        ProductExtensionRuntimeTarget::MacosNative => Ok(StoredRuntimeTarget::MacosNative),
        ProductExtensionRuntimeTarget::MacosNativeBrokered => {
            Ok(StoredRuntimeTarget::MacosNativeBrokered)
        }
        ProductExtensionRuntimeTarget::MacosCompatibility => {
            Ok(StoredRuntimeTarget::MacosCompatibility)
        }
        ProductExtensionRuntimeTarget::LinuxCompatibility => {
            Ok(StoredRuntimeTarget::LinuxCompatibility)
        }
        ProductExtensionRuntimeTarget::WindowsNative => Ok(StoredRuntimeTarget::WindowsNative),
        _ => Err(PreparationError::UnsupportedRuntimeTarget),
    }
}

fn map_tree_binding_error(_error: ExtensionReleaseCatalogError) -> PreparationError {
    PreparationError::TreeIndexBinding
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor};

    use super::*;

    #[test]
    fn exact_reader_rejects_short_long_and_digest_mismatched_resources() {
        let digest = <[u8; 32]>::from(Sha256::digest(b"data"));
        assert_eq!(
            read_exact_bytes(&mut Cursor::new(b"dat"), 4, digest),
            Err(PreparationError::ResourceLengthMismatch)
        );
        assert_eq!(
            read_exact_bytes(&mut Cursor::new(b"data!"), 4, digest),
            Err(PreparationError::ResourceLengthMismatch)
        );
        assert_eq!(
            read_exact_bytes(&mut Cursor::new(b"data"), 4, [0; 32]),
            Err(PreparationError::ResourceDigestMismatch)
        );
        assert_eq!(
            read_exact_bytes(&mut Cursor::new(b"data"), 4, digest)
                .unwrap()
                .as_ref(),
            b"data"
        );
    }

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("fixture failure"))
        }
    }

    #[test]
    fn exact_reader_does_not_leak_io_error_details() {
        assert_eq!(
            read_exact_bytes(&mut FailingReader, 1, [0; 32]),
            Err(PreparationError::Read)
        );
    }

    struct InterruptedChunkReader<'bytes> {
        bytes: &'bytes [u8],
        offset: usize,
        interrupt_next: bool,
        maximum_chunk: usize,
    }

    impl Read for InterruptedChunkReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.interrupt_next {
                self.interrupt_next = false;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            if self.offset == self.bytes.len() {
                return Ok(0);
            }
            let length = buffer
                .len()
                .min(self.maximum_chunk)
                .min(self.bytes.len() - self.offset);
            buffer[..length].copy_from_slice(&self.bytes[self.offset..self.offset + length]);
            self.offset += length;
            self.interrupt_next = true;
            Ok(length)
        }
    }

    struct OverReportingReader;

    impl Read for OverReportingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            Ok(buffer.len().saturating_add(1))
        }
    }

    struct OverReportingEofProbe {
        supplied_byte: bool,
    }

    impl Read for OverReportingEofProbe {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.supplied_byte {
                buffer[0] = b'x';
                self.supplied_byte = true;
                Ok(1)
            } else {
                Ok(buffer.len().saturating_add(1))
            }
        }
    }

    struct FailingEofProbe {
        supplied_byte: bool,
    }

    impl Read for FailingEofProbe {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.supplied_byte {
                buffer[0] = b'x';
                self.supplied_byte = true;
                Ok(1)
            } else {
                Err(io::Error::other("fixture EOF-probe failure"))
            }
        }
    }

    #[test]
    fn exact_reader_retries_interrupted_chunked_reads_and_eof_probe() {
        let bytes = b"chunked payload";
        let digest = <[u8; 32]>::from(Sha256::digest(bytes));
        let mut reader = InterruptedChunkReader {
            bytes,
            offset: 0,
            interrupt_next: true,
            maximum_chunk: 3,
        };
        assert_eq!(
            read_exact_bytes(&mut reader, bytes.len() as u64, digest)
                .unwrap()
                .as_ref(),
            bytes
        );
    }

    #[test]
    fn exact_reader_rejects_contract_violating_over_reported_reads() {
        assert_eq!(
            read_exact_bytes(&mut OverReportingReader, 1, [0; 32]),
            Err(PreparationError::Read)
        );
        assert_eq!(
            read_exact_bytes(
                &mut OverReportingEofProbe {
                    supplied_byte: false,
                },
                1,
                <[u8; 32]>::from(Sha256::digest(b"x")),
            ),
            Err(PreparationError::Read)
        );
        assert_eq!(
            read_exact_bytes(
                &mut FailingEofProbe {
                    supplied_byte: false,
                },
                1,
                <[u8; 32]>::from(Sha256::digest(b"x")),
            ),
            Err(PreparationError::Read)
        );
    }

    #[test]
    fn exact_reader_handles_a_zero_length_internal_boundary_without_growth() {
        assert_eq!(
            read_exact_bytes(
                &mut Cursor::new([]),
                0,
                <[u8; 32]>::from(Sha256::digest([])),
            )
            .unwrap()
            .as_ref(),
            b""
        );
        assert_eq!(
            read_exact_bytes(
                &mut Cursor::new([1]),
                0,
                <[u8; 32]>::from(Sha256::digest([])),
            ),
            Err(PreparationError::ResourceLengthMismatch)
        );
    }

    #[test]
    fn catalog_bytes_must_equal_both_witness_projections() {
        let digest = <[u8; 32]>::from(Sha256::digest(b"catalog"));
        assert_eq!(verify_catalog_bytes(7, digest, b"catalog"), Ok(()));
        assert_eq!(
            verify_catalog_bytes(8, digest, b"catalog"),
            Err(PreparationError::CatalogLengthMismatch)
        );
        assert_eq!(
            verify_catalog_bytes(7, [0; 32], b"catalog"),
            Err(PreparationError::CatalogDigestMismatch)
        );
    }

    #[test]
    fn every_current_product_target_has_one_closed_stored_mapping() {
        assert_eq!(
            stored_runtime_target(ProductExtensionRuntimeTarget::MacosNative).unwrap(),
            StoredRuntimeTarget::MacosNative
        );
        assert_eq!(
            stored_runtime_target(ProductExtensionRuntimeTarget::MacosNativeBrokered).unwrap(),
            StoredRuntimeTarget::MacosNativeBrokered
        );
        assert_eq!(
            stored_runtime_target(ProductExtensionRuntimeTarget::MacosCompatibility).unwrap(),
            StoredRuntimeTarget::MacosCompatibility
        );
        assert_eq!(
            stored_runtime_target(ProductExtensionRuntimeTarget::LinuxCompatibility).unwrap(),
            StoredRuntimeTarget::LinuxCompatibility
        );
        assert_eq!(
            stored_runtime_target(ProductExtensionRuntimeTarget::WindowsNative).unwrap(),
            StoredRuntimeTarget::WindowsNative
        );
    }
}
