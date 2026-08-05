//! Authenticated current-catalog manifest bindings for atomic Store reads.

use thiserror::Error;
use zephium_core::extensions::{ExtensionGrantManifestBindings, ExtensionInstallCatalog};
use zephium_extension_authority::{
    BundledCatalogAdmissionError, ProductExtensionManifestAdmissionError,
    ProductExtensionManifestAuthorityError,
};

use super::api::{BundledCatalogGenerationRole, BundledCurrentCatalogSet};
use super::policy::snapshot_error_requires_poison;
use crate::materialization::{
    current_catalog_set_projection, load_active_manifest_bindings, load_rollback_manifest_bindings,
    PackageObjectError, SnapshotLoadError, VerifiedCatalogRole,
};
use crate::{ExtensionRepository, ExtensionRepositoryError};

/// Exact current bundled selection plus one authenticated descriptor for every
/// row in a caller-supplied complete installation catalog.
///
/// This bounded, path-free value is input to an atomic Store grant-cohort read.
/// It grants no profile permission, package pin, resource access, or native
/// activation authority. Lease acquisition must later revalidate the returned
/// selection identity against Store eligibility. The repository does not
/// authenticate the supplied profile catalog's provenance or freshness; that
/// remains the Store's exact-revision responsibility.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use = "authenticated manifest bindings must be joined with an exact Store cohort or discarded"]
pub struct BundledCurrentManifestBindings {
    current: BundledCurrentCatalogSet,
    bindings: ExtensionGrantManifestBindings,
}

impl BundledCurrentManifestBindings {
    /// Returns the content-addressed current selection and its product role.
    pub const fn current_catalog_set(&self) -> BundledCurrentCatalogSet {
        self.current
    }

    /// Borrows the complete install-id-to-manifest binding set.
    pub const fn bindings(&self) -> &ExtensionGrantManifestBindings {
        &self.bindings
    }

    /// Consumes the projection and returns the complete Store input.
    pub fn into_bindings(self) -> ExtensionGrantManifestBindings {
        self.bindings
    }
}

/// Stable, path-free failure while authenticating a complete manifest-binding
/// cohort from the exact current bundled selection.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BundledManifestBindingsError {
    /// Repository recovery, health, callback policy, or private storage refused
    /// the operation.
    #[error("extension manifest-binding repository operation failed: {0}")]
    Repository(#[source] ExtensionRepositoryError),
    /// No current atomic bundled catalog set exists.
    #[error("extension repository has no current bundled catalog set")]
    NoCurrentSelection,
    /// A coherent crash-resumable package build must settle before verifying a
    /// complete binding cohort.
    #[error("an extension package build is in progress")]
    BuildInProgress,
    /// The current selection changed while its exact package records were read.
    #[error("extension bundled catalog selection changed")]
    StaleSelection,
    /// A supplied install package is absent from the current selection.
    #[error("an installed extension package is not selected by the current catalog set")]
    PackageNotSelected,
    /// A supplied install package identity differs from the freshly admitted
    /// package with the same update-line key.
    #[error("an installed extension package differs from the authenticated package")]
    InstallPackageMismatch,
    /// Product-sealed catalog authority is unavailable.
    #[error("bundled extension catalog authority is unavailable: {0}")]
    CatalogAuthority(#[source] BundledCatalogAdmissionError),
    /// Exact repository-owned catalog bytes failed fresh product admission.
    #[error("bundled extension catalog admission failed: {0}")]
    CatalogAdmission(#[source] BundledCatalogAdmissionError),
    /// Product manifest authority is unavailable.
    #[error("extension manifest authority is unavailable: {0}")]
    ManifestAuthority(#[source] ProductExtensionManifestAuthorityError),
    /// An exact repository-owned manifest failed fresh product admission.
    #[error("extension manifest admission failed: {0}")]
    ManifestAdmission(#[source] ProductExtensionManifestAdmissionError),
    /// A required repository-owned object differs from its authenticated identity.
    #[error("extension package durable objects are not exact")]
    DurableObjectMismatch,
    /// A bounded repository or retained-memory inventory is exhausted.
    #[error("extension manifest-binding capacity is exhausted")]
    CapacityExhausted,
}

impl From<ExtensionRepositoryError> for BundledManifestBindingsError {
    fn from(error: ExtensionRepositoryError) -> Self {
        Self::Repository(error)
    }
}

impl ExtensionRepository {
    /// Freshly authenticates one descriptor for every install in the supplied
    /// complete catalog against the exact current bundled selection.
    ///
    /// The current catalog is parsed once and the manifest authority is opened
    /// once for the complete bounded cohort. Disabled installs are deliberately
    /// included: filtering by desired state would make a later atomic Store read
    /// incomplete. Any stale, absent, mixed-generation, missing required
    /// bootstrap object, or corrupt package rejects the whole result without
    /// creating an owner pin.
    /// This bootstrap verifies the exact catalog, package records, canonical
    /// tree indexes, and manifests needed for Store input; full resource-tree
    /// closure verification remains an independent lease-acquisition gate.
    ///
    /// Calling this method synchronously from a package-resource callback is
    /// rejected before lock acquisition. Package callbacks must also never
    /// delegate repository operations to another thread.
    pub fn authenticate_current_bundled_manifest_bindings(
        &mut self,
        installs: &ExtensionInstallCatalog,
    ) -> Result<BundledCurrentManifestBindings, BundledManifestBindingsError> {
        let runtime = self.runtime.clone();
        let _operation = runtime
            .enter()
            .map_err(|error| BundledManifestBindingsError::Repository(error.repository_error()))?;
        let current = current_catalog_set_projection(self.writer_materialization()?)
            .map_err(|error| self.finish_manifest_bindings_snapshot_error(error))?
            .ok_or(BundledManifestBindingsError::NoCurrentSelection)?;
        if current.build_in_progress() {
            return Err(BundledManifestBindingsError::BuildInProgress);
        }
        let exact_catalog = self.read_authenticated_catalog_object(current.catalog_digest())?;
        let (role, bindings) = match current.role() {
            VerifiedCatalogRole::Active => (
                BundledCatalogGenerationRole::Active,
                load_active_manifest_bindings(
                    self.writer_materialization()?,
                    &current,
                    &exact_catalog,
                    installs,
                )
                .map_err(|error| self.finish_manifest_bindings_snapshot_error(error))?,
            ),
            VerifiedCatalogRole::Rollback => (
                BundledCatalogGenerationRole::Rollback,
                load_rollback_manifest_bindings(
                    self.writer_materialization()?,
                    &current,
                    &exact_catalog,
                    installs,
                )
                .map_err(|error| self.finish_manifest_bindings_snapshot_error(error))?,
            ),
        };
        Ok(BundledCurrentManifestBindings {
            current: BundledCurrentCatalogSet {
                identity: current.identity().into(),
                role,
            },
            bindings,
        })
    }

    fn finish_manifest_bindings_snapshot_error(
        &mut self,
        error: SnapshotLoadError,
    ) -> BundledManifestBindingsError {
        let poison = manifest_bindings_error_requires_poison(&error);
        let mapped = map_snapshot_error(error);
        if poison {
            self.writer_seal();
        }
        mapped
    }
}

// Dispatch selects this loader directly from the freshly authenticated
// current role, so observing the opposite role here is internal incoherence.
// The shared lease policy leaves WrongRole clean because it is also used by
// public role-specific lease entry points where a caller can choose wrongly.
fn manifest_bindings_error_requires_poison(error: &SnapshotLoadError) -> bool {
    matches!(error, SnapshotLoadError::WrongRole) || snapshot_error_requires_poison(error)
}

fn map_snapshot_error(error: SnapshotLoadError) -> BundledManifestBindingsError {
    match error {
        SnapshotLoadError::Repository(error) => BundledManifestBindingsError::Repository(error),
        SnapshotLoadError::CatalogAuthority(error) => {
            BundledManifestBindingsError::CatalogAuthority(error)
        }
        SnapshotLoadError::CatalogAdmission(error) => {
            BundledManifestBindingsError::CatalogAdmission(error)
        }
        SnapshotLoadError::ManifestAuthority(error) => {
            BundledManifestBindingsError::ManifestAuthority(error)
        }
        SnapshotLoadError::ManifestAdmission(error) => {
            BundledManifestBindingsError::ManifestAdmission(error)
        }
        SnapshotLoadError::StaleSelection => BundledManifestBindingsError::StaleSelection,
        SnapshotLoadError::PackageNotSelected => BundledManifestBindingsError::PackageNotSelected,
        SnapshotLoadError::InstallPackageMismatch | SnapshotLoadError::EligibilityMismatch => {
            BundledManifestBindingsError::InstallPackageMismatch
        }
        SnapshotLoadError::AccountingOverflow => BundledManifestBindingsError::CapacityExhausted,
        SnapshotLoadError::WrongRole
        | SnapshotLoadError::PackageNotMaterialized
        | SnapshotLoadError::Preparation(_)
        | SnapshotLoadError::DurableMismatch => BundledManifestBindingsError::DurableObjectMismatch,
        SnapshotLoadError::Object { error, .. } => map_object_error(error),
    }
}

fn map_object_error(error: PackageObjectError) -> BundledManifestBindingsError {
    match error {
        PackageObjectError::BuildStateMismatch => {
            BundledManifestBindingsError::Repository(ExtensionRepositoryError::RecoveryAmbiguous)
        }
        PackageObjectError::CapacityExhausted => BundledManifestBindingsError::CapacityExhausted,
        PackageObjectError::GenerationExhausted => {
            BundledManifestBindingsError::Repository(ExtensionRepositoryError::GenerationExhausted)
        }
        PackageObjectError::Collision
        | PackageObjectError::ExactMismatch
        | PackageObjectError::Source(_) => BundledManifestBindingsError::DurableObjectMismatch,
        PackageObjectError::Filesystem(error) => {
            BundledManifestBindingsError::Repository(ExtensionRepositoryError::FileSystem(error))
        }
        PackageObjectError::SettlementAmbiguous => {
            BundledManifestBindingsError::Repository(ExtensionRepositoryError::SettlementAmbiguous)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_catalog_mismatches_are_non_poisoning_and_distinct() {
        for (source, expected) in [
            (
                SnapshotLoadError::PackageNotSelected,
                BundledManifestBindingsError::PackageNotSelected,
            ),
            (
                SnapshotLoadError::InstallPackageMismatch,
                BundledManifestBindingsError::InstallPackageMismatch,
            ),
        ] {
            assert!(!snapshot_error_requires_poison(&source));
            let mapped = map_snapshot_error(source);
            assert_eq!(
                std::mem::discriminant(&mapped),
                std::mem::discriminant(&expected)
            );
        }
    }

    #[test]
    fn impossible_bootstrap_role_mismatch_is_poisoning() {
        let error = SnapshotLoadError::WrongRole;
        assert!(!snapshot_error_requires_poison(&error));
        assert!(manifest_bindings_error_requires_poison(&error));
        assert!(matches!(
            map_snapshot_error(error),
            BundledManifestBindingsError::DurableObjectMismatch
        ));
    }
}
