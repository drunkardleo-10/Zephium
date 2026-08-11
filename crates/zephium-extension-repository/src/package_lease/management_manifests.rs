//! Authenticated, locale-resolved manifest identity for extension management UI.

use std::sync::Arc;

use thiserror::Error;
use zephium_core::extensions::{
    ExtensionGrantCohortError, ExtensionGrantManifestBinding, ExtensionGrantManifestBindings,
    ExtensionInstallCatalog,
};
use zephium_core::ids::ExtensionInstallId;
use zephium_extension_package::{
    ExtensionDefaultLocaleResolutionError, ResolvedExtensionManifestMetadata,
};

use super::api::{BundledCatalogGenerationRole, BundledCurrentCatalogSet};
use super::manifest_bindings::{
    manifest_bindings_error_requires_poison, map_snapshot_error, BundledManifestBindingsError,
};
use crate::materialization::{
    current_catalog_set_projection, load_active_management_manifests,
    load_rollback_management_manifests, AuthenticatedManagementManifest,
    ManagementManifestLoadError, SnapshotLoadError, VerifiedCatalogRole,
};
use crate::{ExtensionRepository, ExtensionRepositoryError};

/// One authenticated, locale-resolved installed-extension identity.
///
/// The value contains no package path, package bytes, resource handle, profile
/// grant, or runtime authority. Its text is safe for direct browser-owned UI
/// because it was resolved from the exact admitted manifest and, when needed,
/// the digest-bound default-locale resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundledManagementManifest {
    install_id: ExtensionInstallId,
    version: Box<str>,
    metadata: ResolvedExtensionManifestMetadata,
}

impl BundledManagementManifest {
    /// Returns the durable profile-local install identity.
    pub const fn install_id(&self) -> ExtensionInstallId {
        self.install_id
    }

    /// Returns the canonical manifest version string.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns resolved display metadata authenticated by the repository.
    pub const fn metadata(&self) -> &ResolvedExtensionManifestMetadata {
        &self.metadata
    }

    /// Returns the required trusted display name.
    pub fn name(&self) -> &str {
        self.metadata.name().as_str()
    }

    /// Returns the trusted description when declared.
    pub fn description(&self) -> Option<&str> {
        self.metadata.description().map(|text| text.as_str())
    }

    /// Returns the trusted author string when declared.
    pub fn author(&self) -> Option<&str> {
        self.metadata.author().map(|text| text.as_str())
    }
}

/// Exact current selection, complete Store input, and one UI identity for
/// every caller-supplied install.
///
/// The complete cohort either authenticates as a unit or is absent. It creates
/// no durable package pin and does not verify full resource-tree closure,
/// activate an extension, or construct a native controller.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use = "authenticated management manifests must be joined with Store or discarded"]
pub struct BundledCurrentManagementManifests {
    current: BundledCurrentCatalogSet,
    bindings: ExtensionGrantManifestBindings,
    manifests: Box<[BundledManagementManifest]>,
}

impl BundledCurrentManagementManifests {
    /// Returns the exact content-addressed repository selection.
    pub const fn current_catalog_set(&self) -> BundledCurrentCatalogSet {
        self.current
    }

    /// Borrows the complete descriptor cohort suitable for one atomic Store read.
    pub const fn bindings(&self) -> &ExtensionGrantManifestBindings {
        &self.bindings
    }

    /// Borrows canonical install-identity order metadata.
    pub fn manifests(&self) -> &[BundledManagementManifest] {
        &self.manifests
    }

    /// Consumes the repository projection into its Store input and UI identities.
    pub fn into_parts(
        self,
    ) -> (
        BundledCurrentCatalogSet,
        ExtensionGrantManifestBindings,
        Box<[BundledManagementManifest]>,
    ) {
        (self.current, self.bindings, self.manifests)
    }
}

/// Stable, path-free management-manifest failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BundledManagementManifestsError {
    /// Catalog, manifest, repository, or bounded-cohort authentication failed.
    #[error("extension management manifest authentication failed: {0}")]
    Authentication(#[source] BundledManifestBindingsError),
    /// Exact admitted locale data could not produce safe browser UI text.
    #[error("extension management display metadata resolution failed: {0}")]
    Metadata(#[source] ExtensionDefaultLocaleResolutionError),
}

impl From<ExtensionRepositoryError> for BundledManagementManifestsError {
    fn from(error: ExtensionRepositoryError) -> Self {
        Self::Authentication(BundledManifestBindingsError::Repository(error))
    }
}

impl ExtensionRepository {
    /// Freshly authenticates and resolves display identity for every supplied install.
    ///
    /// This method is deliberately separate from runtime manifest bootstrap so
    /// ordinary activation and process startup never pay for locale-resource
    /// reads. Call it only for an explicit privileged management projection.
    /// Disabled installs remain included, and any row failure rejects the whole
    /// result rather than presenting a filtered or partly unauthenticated list.
    pub fn authenticate_current_bundled_management_manifests(
        &mut self,
        installs: &ExtensionInstallCatalog,
    ) -> Result<BundledCurrentManagementManifests, BundledManagementManifestsError> {
        let runtime = self.runtime.clone();
        let _operation = runtime.enter().map_err(|error| {
            BundledManagementManifestsError::Authentication(
                BundledManifestBindingsError::Repository(error.repository_error()),
            )
        })?;
        let current = current_catalog_set_projection(self.writer_materialization()?)
            .map_err(|error| self.finish_management_manifest_snapshot_error(error))?
            .ok_or(BundledManagementManifestsError::Authentication(
                BundledManifestBindingsError::NoCurrentSelection,
            ))?;
        if current.build_in_progress() {
            return Err(BundledManagementManifestsError::Authentication(
                BundledManifestBindingsError::BuildInProgress,
            ));
        }
        let exact_catalog = self.read_authenticated_catalog_object(current.catalog_digest())?;
        let (role, manifests) = match current.role() {
            VerifiedCatalogRole::Active => (
                BundledCatalogGenerationRole::Active,
                load_active_management_manifests(
                    self.writer_materialization()?,
                    &current,
                    &exact_catalog,
                    installs,
                )
                .map_err(|error| self.finish_management_manifest_load_error(error))?,
            ),
            VerifiedCatalogRole::Rollback => (
                BundledCatalogGenerationRole::Rollback,
                load_rollback_management_manifests(
                    self.writer_materialization()?,
                    &current,
                    &exact_catalog,
                    installs,
                )
                .map_err(|error| self.finish_management_manifest_load_error(error))?,
            ),
        };
        let (bindings, manifests) = finish_management_cohort(manifests)?;
        Ok(BundledCurrentManagementManifests {
            current: BundledCurrentCatalogSet {
                identity: current.identity().into(),
                role,
            },
            bindings,
            manifests,
        })
    }

    fn finish_management_manifest_load_error(
        &mut self,
        error: ManagementManifestLoadError,
    ) -> BundledManagementManifestsError {
        match error {
            ManagementManifestLoadError::Snapshot(error) => {
                self.finish_management_manifest_snapshot_error(error)
            }
            ManagementManifestLoadError::Metadata(error) => {
                BundledManagementManifestsError::Metadata(error)
            }
        }
    }

    fn finish_management_manifest_snapshot_error(
        &mut self,
        error: SnapshotLoadError,
    ) -> BundledManagementManifestsError {
        let poison = manifest_bindings_error_requires_poison(&error);
        let mapped = map_snapshot_error(error);
        if poison {
            self.writer_seal();
        }
        BundledManagementManifestsError::Authentication(mapped)
    }
}

fn finish_management_cohort(
    manifests: Box<[AuthenticatedManagementManifest]>,
) -> Result<
    (
        ExtensionGrantManifestBindings,
        Box<[BundledManagementManifest]>,
    ),
    BundledManagementManifestsError,
> {
    let mut bindings = Vec::with_capacity(manifests.len());
    let mut presentation = Vec::with_capacity(manifests.len());
    for manifest in manifests {
        let (install_id, descriptor, version, metadata) = manifest.into_parts();
        bindings.push(ExtensionGrantManifestBinding::new(
            install_id,
            Arc::clone(&descriptor),
        ));
        presentation.push(BundledManagementManifest {
            install_id,
            version,
            metadata,
        });
    }
    let bindings = ExtensionGrantManifestBindings::new(bindings).map_err(map_cohort_error)?;
    Ok((bindings, presentation.into_boxed_slice()))
}

fn map_cohort_error(error: ExtensionGrantCohortError) -> BundledManagementManifestsError {
    let authentication = match error {
        ExtensionGrantCohortError::TooManyBindings { .. }
        | ExtensionGrantCohortError::AccountingOverflow
        | ExtensionGrantCohortError::RetainedBytesExceeded { .. } => {
            BundledManifestBindingsError::CapacityExhausted
        }
        ExtensionGrantCohortError::DuplicateBinding(_)
        | ExtensionGrantCohortError::IncompleteBindings
        | ExtensionGrantCohortError::ManifestPackageMismatch(_)
        | ExtensionGrantCohortError::TooManyAuthorities { .. }
        | ExtensionGrantCohortError::DuplicateAuthority(_)
        | ExtensionGrantCohortError::UnknownAuthority
        | ExtensionGrantCohortError::AuthorityMismatch(_) => {
            BundledManifestBindingsError::DurableObjectMismatch
        }
    };
    BundledManagementManifestsError::Authentication(authentication)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_failure_does_not_claim_repository_corruption() {
        let error = BundledManagementManifestsError::Metadata(
            ExtensionDefaultLocaleResolutionError::MissingLocaleMessages,
        );
        assert!(matches!(
            error,
            BundledManagementManifestsError::Metadata(_)
        ));
    }
}
