//! Shared resource-read policy for both nominal package-lease roles.

use std::io::Read;
use std::sync::Arc;

use zephium_extension_package::{CanonicalExtensionTreeIndex, PortableRelativePath};
use zephium_private_fs::SealedPrivateDirectory;

use super::api::BundledPackageResourceError;
use crate::materialization::{with_verified_tree_resource, TreeResourceError};
use crate::operation::{with_external_callback, RepositoryRuntime};

pub(super) fn with_resource<T, E>(
    runtime: &RepositoryRuntime,
    root: &Arc<SealedPrivateDirectory>,
    index: &CanonicalExtensionTreeIndex,
    path: &PortableRelativePath,
    callback: impl FnOnce(&mut dyn Read) -> Result<T, E>,
) -> Result<Result<T, E>, BundledPackageResourceError> {
    if !runtime.is_healthy() {
        return Err(BundledPackageResourceError::LeaseInactive);
    }
    let result = with_verified_tree_resource(root, index, path, |reader| {
        with_external_callback(|| callback(reader))
    })
    .map_err(|error| {
        let mapped = match error {
            TreeResourceError::NotDeclared => BundledPackageResourceError::ResourceNotDeclared,
            TreeResourceError::Unavailable => BundledPackageResourceError::ResourceReadUnavailable,
            TreeResourceError::Quarantined => BundledPackageResourceError::NamespaceQuarantined,
            TreeResourceError::Missing | TreeResourceError::Mismatch => {
                BundledPackageResourceError::DurableResourceMismatch
            }
        };
        if !matches!(
            mapped,
            BundledPackageResourceError::ResourceNotDeclared
                | BundledPackageResourceError::ResourceReadUnavailable
        ) {
            runtime.poison();
        }
        mapped
    })?;
    if !runtime.is_healthy() {
        return Err(BundledPackageResourceError::LeaseInactive);
    }
    Ok(result)
}
