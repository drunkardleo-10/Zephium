//! Source-free authentication of one marker-committed package build.
//!
//! The package-record final is an implicit commit marker only. This module
//! reconstructs non-serializable active or rollback authority from product-
//! sealed catalog and manifest policy, then freshly verifies every durable
//! package object before returning a typed completion capability. It never
//! accepts a byte-source callback and never repairs a committed closure.

use std::sync::Arc;

use thiserror::Error;
use zephium_core::extensions::ExtensionPackageKey;
use zephium_extension_authority::{
    BundledCatalogAdmissionError, BundledPackageAuthority, ProductBundledCatalogGenerationRole,
};
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, PortableRelativePath, MAX_EXTENSION_MANIFEST_BYTES,
    MAX_EXTENSION_TREE_INDEX_BYTES,
};

use super::names::{package_record, tree_index_object, tree_object};
use super::objects::{
    preflight_package_object_capacity, verify_interrupted_active_package,
    verify_interrupted_rollback_package, PackageObjectError, PackageObjectIntentDisposition,
    VerifiedActivePackageClosure, VerifiedRollbackPackageClosure,
};
use super::prepare::{
    open_product_manifest_authority, prepare_active_package_from_preparsed,
    prepare_rollback_package_from_preparsed, PreparationError,
};
use super::records::{PackageRecord, MAX_PACKAGE_RECORD_BYTES};
use super::runtime::MaterializationRuntime;
use super::storage::read_required_sealed_record;
use super::tree_reader::{with_verified_tree_resource, TreeResourceError};
use crate::ExtensionRepositoryError;

/// Exact typed package closure recovered without an external byte source.
#[must_use = "an interrupted package closure must be consumed by typed completion"]
pub(crate) enum VerifiedInterruptedPackageClosure {
    /// Ordinary active-generation completion authority.
    Active(VerifiedActivePackageClosure),
    /// Explicit rollback-generation completion authority.
    Rollback(VerifiedRollbackPackageClosure),
}

/// Path-free failure while authenticating a marker-committed build.
#[derive(Debug, Error)]
pub(crate) enum InterruptedPackageAuthenticationError {
    /// Product-sealed catalog authority is unavailable or invalid.
    #[error("bundled extension catalog authority is unavailable: {0}")]
    CatalogAuthority(#[source] BundledCatalogAdmissionError),
    /// Exact stored catalog bytes are not admitted by product policy.
    #[error("stored extension catalog admission failed: {0}")]
    CatalogAdmission(#[source] BundledCatalogAdmissionError),
    /// Durable repository I/O or metadata recovery failed.
    #[error("extension package repository authentication failed: {0}")]
    Repository(#[source] ExtensionRepositoryError),
    /// Exact manifest, index, or catalog/package reconstruction failed.
    #[error("extension package preparation failed: {0}")]
    Preparation(#[source] PreparationError),
    /// The durable package object closure was not exact.
    #[error("extension package closure authentication failed: {0}")]
    Object(#[source] PackageObjectError),
    /// A durable marker or object binding disagreed with the build intent.
    #[error("extension package durable object binding is not exact")]
    DurableMismatch,
}

/// Reconstructs and verifies one marker-committed package from durable objects.
///
/// `exact_catalog_bytes` must come from the repository's authenticated catalog
/// object namespace. The live runtime map is intentionally not used as marker
/// authority: immediately after publication it may not yet contain the new
/// package record, while all physical finals are already durable.
pub(crate) fn authenticate_interrupted_package(
    runtime: &MaterializationRuntime,
    exact_catalog_bytes: &[u8],
) -> Result<VerifiedInterruptedPackageClosure, InterruptedPackageAuthenticationError> {
    let intent = runtime
        ._build_intent
        .as_ref()
        .ok_or(InterruptedPackageAuthenticationError::DurableMismatch)?;
    if runtime._state.build_intent.as_ref() != Some(intent)
        || runtime._state.generation != intent.generation
        || intent.package_record.record_id().ok() != Some(intent.package_record_id)
    {
        return Err(InterruptedPackageAuthenticationError::DurableMismatch);
    }

    let authority = BundledPackageAuthority::product()
        .map_err(InterruptedPackageAuthenticationError::CatalogAuthority)?;
    let generation = intent
        .package_record
        .catalog
        .generation_anchor()
        .map_err(InterruptedPackageAuthenticationError::Repository)?;
    let role = authority
        .recognize_generation(&generation)
        .ok_or(InterruptedPackageAuthenticationError::DurableMismatch)?;

    let marker_bytes = read_required_sealed_record(
        &runtime._records,
        &package_record(intent.package_record_id),
        MAX_PACKAGE_RECORD_BYTES,
    )
    .map_err(InterruptedPackageAuthenticationError::Repository)?;
    let marker = PackageRecord::decode(&marker_bytes)
        .map_err(InterruptedPackageAuthenticationError::Repository)?;
    if marker != intent.package_record || marker.record_id().ok() != Some(intent.package_record_id)
    {
        return Err(InterruptedPackageAuthenticationError::DurableMismatch);
    }

    let index_bytes = read_required_sealed_record(
        &runtime._records,
        &tree_index_object(marker.tree_index.index_sha256),
        MAX_EXTENSION_TREE_INDEX_BYTES,
    )
    .map_err(InterruptedPackageAuthenticationError::Repository)?;
    let index = CanonicalExtensionTreeIndex::parse_canonical(&index_bytes)
        .map_err(|_| InterruptedPackageAuthenticationError::DurableMismatch)?;
    if index.index_sha256().bytes() != marker.tree_index.index_sha256.bytes()
        || index.index_bytes() != marker.tree_index.index_length
        || index.tree_sha256().bytes() != marker.tree_index.tree_sha256.bytes()
        || index.files().len() != marker.tree_index.file_count as usize
        || index.implicit_directory_count() != marker.tree_index.directory_count as usize
        || index.total_entry_count() != marker.tree_index.total_entry_count as usize
        || index.total_bytes() != marker.tree_index.tree_bytes
    {
        return Err(InterruptedPackageAuthenticationError::DurableMismatch);
    }

    let root = Arc::new(
        runtime
            ._trees
            .open_sealed_private_child(&tree_object(marker.tree_index.tree_sha256))
            .map_err(ExtensionRepositoryError::FileSystem)
            .map_err(InterruptedPackageAuthenticationError::Repository)?,
    );
    let manifest_path = PortableRelativePath::parse("manifest.json")
        .map_err(|_| InterruptedPackageAuthenticationError::DurableMismatch)?;
    let manifest_length = index
        .file(&manifest_path)
        .ok_or(InterruptedPackageAuthenticationError::DurableMismatch)?
        .length();
    if manifest_length == 0
        || manifest_length > MAX_EXTENSION_MANIFEST_BYTES as u64
        || manifest_length != marker.manifest.manifest_length
    {
        return Err(InterruptedPackageAuthenticationError::DurableMismatch);
    }
    let manifest_capacity = usize::try_from(manifest_length)
        .map_err(|_| InterruptedPackageAuthenticationError::DurableMismatch)?;
    let manifest_bytes = with_verified_tree_resource(&root, &index, &manifest_path, |reader| {
        let mut bytes = Vec::with_capacity(manifest_capacity);
        reader.read_to_end(&mut bytes).map(|_| bytes)
    })
    .map_err(map_tree_resource)?
    .map_err(|_| InterruptedPackageAuthenticationError::DurableMismatch)?;

    let manifest_authority = open_product_manifest_authority()
        .map_err(InterruptedPackageAuthenticationError::Preparation)?;
    let package_key = ExtensionPackageKey::from_bytes(marker.package.package_key.bytes());
    let capacity = preflight_package_object_capacity(runtime, &marker)
        .map_err(InterruptedPackageAuthenticationError::Object)?;
    if capacity.intent_disposition() != PackageObjectIntentDisposition::AlreadyCommitted {
        return Err(InterruptedPackageAuthenticationError::DurableMismatch);
    }

    match role {
        ProductBundledCatalogGenerationRole::Active => {
            let catalog = authority
                .admit_catalog(exact_catalog_bytes)
                .map_err(InterruptedPackageAuthenticationError::CatalogAdmission)?;
            if catalog.generation_anchor() != generation {
                return Err(InterruptedPackageAuthenticationError::DurableMismatch);
            }
            let prepared = prepare_active_package_from_preparsed(
                &catalog,
                &manifest_authority,
                marker.manifest.runtime_target.product_target(),
                package_key,
                index,
                index_bytes.into_boxed_slice(),
                manifest_bytes.into_boxed_slice(),
            )
            .map_err(InterruptedPackageAuthenticationError::Preparation)?;
            if prepared.record() != &marker {
                return Err(InterruptedPackageAuthenticationError::DurableMismatch);
            }
            verify_interrupted_active_package(runtime, capacity, prepared)
                .map(VerifiedInterruptedPackageClosure::Active)
                .map_err(InterruptedPackageAuthenticationError::Object)
        }
        ProductBundledCatalogGenerationRole::Rollback => {
            let catalog = authority
                .admit_rollback_catalog(exact_catalog_bytes)
                .map_err(InterruptedPackageAuthenticationError::CatalogAdmission)?;
            if catalog.generation_anchor() != generation {
                return Err(InterruptedPackageAuthenticationError::DurableMismatch);
            }
            let prepared = prepare_rollback_package_from_preparsed(
                &catalog,
                &manifest_authority,
                marker.manifest.runtime_target.product_target(),
                package_key,
                index,
                index_bytes.into_boxed_slice(),
                manifest_bytes.into_boxed_slice(),
            )
            .map_err(InterruptedPackageAuthenticationError::Preparation)?;
            if prepared.record() != &marker {
                return Err(InterruptedPackageAuthenticationError::DurableMismatch);
            }
            verify_interrupted_rollback_package(runtime, capacity, prepared)
                .map(VerifiedInterruptedPackageClosure::Rollback)
                .map_err(InterruptedPackageAuthenticationError::Object)
        }
    }
}

fn map_tree_resource(error: TreeResourceError) -> InterruptedPackageAuthenticationError {
    match error {
        TreeResourceError::Unavailable => InterruptedPackageAuthenticationError::Repository(
            ExtensionRepositoryError::FileSystem(zephium_private_fs::PrivateFsError::Io),
        ),
        TreeResourceError::NotDeclared
        | TreeResourceError::Missing
        | TreeResourceError::Mismatch
        | TreeResourceError::Quarantined => InterruptedPackageAuthenticationError::DurableMismatch,
    }
}
