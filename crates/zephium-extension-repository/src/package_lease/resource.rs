//! Shared resource-read policy for both nominal package-lease roles.

use std::io::Read;
use std::sync::Arc;

use zephium_extension_package::{CanonicalExtensionTreeIndex, PortableRelativePath};
use zephium_private_fs::SealedPrivateDirectory;

use super::{BundledPackageResourceError, RepositoryHealth};
use crate::materialization::{with_verified_tree_resource, TreeResourceError};

pub(super) fn with_resource<T, E>(
    health: &Arc<RepositoryHealth>,
    root: &Arc<SealedPrivateDirectory>,
    index: &CanonicalExtensionTreeIndex,
    path: &PortableRelativePath,
    callback: impl FnOnce(&mut dyn Read) -> Result<T, E>,
) -> Result<Result<T, E>, BundledPackageResourceError> {
    if !health.is_healthy() {
        return Err(BundledPackageResourceError::LeaseInactive);
    }
    let result = with_verified_tree_resource(root, index, path, callback).map_err(|error| {
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
            health.poison();
        }
        mapped
    })?;
    if !health.is_healthy() {
        return Err(BundledPackageResourceError::LeaseInactive);
    }
    Ok(result)
}
