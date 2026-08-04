//! Public orchestration for durable atomic bundled-catalog selections.
//!
//! Selection publication re-admits every exact manifest and re-verifies every
//! completed sealed package. It deliberately stops at repository metadata: no
//! profile grant, runtime lease, filesystem path, or native activation escapes
//! this layer.

use std::sync::Arc;

use thiserror::Error;
use zephium_core::extensions::ExtensionPackageKey;
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, ProductExtensionRuntimeTarget,
};

use crate::materialization::{
    derive_active_catalog_set, derive_rollback_catalog_set, open_product_manifest_authority,
    preflight_package_object_capacity, prepare_active_package, prepare_rollback_package,
    promote_active_catalog_set, promote_rollback_catalog_set, rollback_to_previous_catalog_set,
    stage_active_catalog_set_candidate, stage_rollback_catalog_set_candidate,
    verify_completed_active_package, verify_completed_rollback_package, BundledReleaseByteSource,
    MaterializationRuntime, PackageObjectIntentDisposition, VerifiedActiveCatalogSet,
    VerifiedRollbackCatalogSet,
};
use crate::state::Digest32;
use crate::writer::{
    completed_error_requires_sealing, map_object_error, map_preparation_error,
    publication_error_requires_sealing,
};
use crate::{BundledPackageMaterializationError, ExtensionRepository, ExtensionRepositoryError};

/// One exact runtime backend choice for one authenticated catalog package.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BundledPackageRuntimeSelection {
    package_key: ExtensionPackageKey,
    runtime_target: ProductExtensionRuntimeTarget,
}

impl BundledPackageRuntimeSelection {
    /// Creates one package/backend choice. The complete slice is validated
    /// against the authenticated catalog before durable mutation.
    pub const fn new(
        package_key: ExtensionPackageKey,
        runtime_target: ProductExtensionRuntimeTarget,
    ) -> Self {
        Self {
            package_key,
            runtime_target,
        }
    }

    /// Returns the authenticated catalog package key.
    pub const fn package_key(self) -> ExtensionPackageKey {
        self.package_key
    }

    /// Returns the exact product-reviewed backend target.
    pub const fn runtime_target(self) -> ProductExtensionRuntimeTarget {
        self.runtime_target
    }
}

/// Opaque, path-free identity of one exact atomic catalog selection.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BundledCatalogSetIdentity([u8; 32]);

impl BundledCatalogSetIdentity {
    /// Returns the content identity bytes. They reveal no repository path.
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

impl From<Digest32> for BundledCatalogSetIdentity {
    fn from(value: Digest32) -> Self {
        Self(value.bytes())
    }
}

/// Result of exact-CAS publishing and candidate-staging one catalog set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[must_use = "catalog-set replay and candidate mutation have different effects"]
pub enum BundledCatalogSetStageOutcome {
    /// The exact set became the sole durable candidate.
    Staged(BundledCatalogSetIdentity),
    /// The exact set was already the candidate; its closure was reverified.
    IdempotentCandidate(BundledCatalogSetIdentity),
    /// The exact set is already current; no candidate was created.
    AlreadyCurrent(BundledCatalogSetIdentity),
    /// The exact set is already the rollback selection; no candidate was
    /// created.
    AlreadyPrevious(BundledCatalogSetIdentity),
}

/// Result of promoting one exact candidate selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[must_use = "catalog-set promotion and exact replay have different effects"]
pub enum BundledCatalogSetPromotionOutcome {
    /// The candidate became current and the old current became previous.
    Promoted(BundledCatalogSetIdentity),
    /// The exact selection was already current after a settled retry.
    IdempotentCurrent(BundledCatalogSetIdentity),
}

/// Result of swapping one exact rollback selection with current.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[must_use = "catalog-set rollback and exact replay have different effects"]
pub enum BundledCatalogSetRollbackOutcome {
    /// The explicitly approved previous selection became current.
    RolledBack(BundledCatalogSetIdentity),
    /// The exact current/previous swap had already settled.
    IdempotentRollback(BundledCatalogSetIdentity),
}

/// Stable, path-free failure while producing or staging an atomic selection.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BundledCatalogSetError {
    /// Exact package preparation or durable object validation failed.
    #[error("extension catalog-set package verification failed: {0}")]
    Package(#[from] BundledPackageMaterializationError),
    /// The caller did not provide exactly one canonically ordered choice for
    /// every authenticated catalog package.
    #[error("extension catalog-set selection is not an exact catalog projection")]
    InvalidProjection,
    /// At least one selected package has not completed durable materialization.
    #[error("extension catalog-set package is not durably materialized")]
    PackageNotMaterialized,
    /// The exact authenticated catalog object has not previously been produced
    /// by package materialization.
    #[error("extension catalog is not durably materialized")]
    CatalogNotMaterialized,
    /// The expected candidate/current/previous identities no longer match the
    /// recovered durable state.
    #[error("extension catalog-set durable selection changed")]
    StaleSelection,
}

impl ExtensionRepository {
    /// Re-admits and re-verifies a complete active-generation selection, then
    /// exact-CAS publishes it and durably stages it as candidate.
    pub fn stage_active_bundled_catalog_set<S: BundledReleaseByteSource>(
        &mut self,
        catalog: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
        selections: &[BundledPackageRuntimeSelection],
        source: &mut S,
    ) -> Result<BundledCatalogSetStageOutcome, BundledCatalogSetError> {
        let operation_gate = Arc::clone(self.package_leases.operation_gate());
        let health = Arc::clone(self.package_leases.health());
        let _operation = operation_gate
            .enter(&health)
            .map_err(|error| repository_package_error(error.repository_error()))?;
        validate_projection(catalog.catalog(), selections)?;
        if !self
            .writer_validate_active_catalog_materialized(catalog, exact_catalog_bytes)
            .map_err(repository_package_error)?
        {
            return Err(BundledCatalogSetError::CatalogNotMaterialized);
        }
        let (mut runtime, verified) =
            self.produce_active_catalog_set(catalog, exact_catalog_bytes, selections, source)?;
        let identity = verified.record_id();
        if runtime._state.candidate_catalog_set_id.is_some()
            && runtime._state.candidate_catalog_set_id != Some(identity)
        {
            return Err(self.finish_stale_catalog_set(runtime));
        }
        if let Some(outcome) = existing_stage_outcome(
            runtime._state.candidate_catalog_set_id,
            runtime._state.current_catalog_set_id,
            runtime._state.previous_catalog_set_id,
            identity,
        ) {
            if let Err(error) = verified.verify_stored(&runtime) {
                return Err(self.finish_catalog_set_object_failure(runtime, error));
            }
            drop(runtime);
            self.writer_recover_materialization()
                .map_err(repository_package_error)?;
            return Ok(outcome);
        }
        if let Err(error) = verified.publish(&mut runtime) {
            return Err(self.finish_catalog_set_object_failure(runtime, error));
        }
        self.finish_transition(stage_active_catalog_set_candidate(runtime, verified))
            .map_err(BundledCatalogSetError::Package)?;
        Ok(BundledCatalogSetStageOutcome::Staged(identity.into()))
    }

    /// Re-admits and re-verifies a complete explicitly approved rollback
    /// selection, then exact-CAS publishes it and stages it as candidate while
    /// preserving its nominal rollback authority.
    pub fn stage_rollback_bundled_catalog_set<S: BundledReleaseByteSource>(
        &mut self,
        catalog: &AdmittedRollbackBundledCatalog,
        exact_catalog_bytes: &[u8],
        selections: &[BundledPackageRuntimeSelection],
        source: &mut S,
    ) -> Result<BundledCatalogSetStageOutcome, BundledCatalogSetError> {
        let operation_gate = Arc::clone(self.package_leases.operation_gate());
        let health = Arc::clone(self.package_leases.health());
        let _operation = operation_gate
            .enter(&health)
            .map_err(|error| repository_package_error(error.repository_error()))?;
        validate_projection(catalog.catalog(), selections)?;
        if !self
            .writer_validate_rollback_catalog_materialized(catalog, exact_catalog_bytes)
            .map_err(repository_package_error)?
        {
            return Err(BundledCatalogSetError::CatalogNotMaterialized);
        }
        let (mut runtime, verified) =
            self.produce_rollback_catalog_set(catalog, exact_catalog_bytes, selections, source)?;
        let identity = verified.record_id();
        if runtime._state.candidate_catalog_set_id.is_some()
            && runtime._state.candidate_catalog_set_id != Some(identity)
        {
            return Err(self.finish_stale_catalog_set(runtime));
        }
        if let Some(outcome) = existing_stage_outcome(
            runtime._state.candidate_catalog_set_id,
            runtime._state.current_catalog_set_id,
            runtime._state.previous_catalog_set_id,
            identity,
        ) {
            if let Err(error) = verified.verify_stored(&runtime) {
                return Err(self.finish_catalog_set_object_failure(runtime, error));
            }
            drop(runtime);
            self.writer_recover_materialization()
                .map_err(repository_package_error)?;
            return Ok(outcome);
        }
        if let Err(error) = verified.publish(&mut runtime) {
            return Err(self.finish_catalog_set_object_failure(runtime, error));
        }
        self.finish_transition(stage_rollback_catalog_set_candidate(runtime, verified))
            .map_err(BundledCatalogSetError::Package)?;
        Ok(BundledCatalogSetStageOutcome::Staged(identity.into()))
    }

    /// Re-admits an exact active candidate and atomically promotes it to
    /// current. `expected_candidate` is a caller-visible stale-CAS guard.
    pub fn promote_active_bundled_catalog_set<S: BundledReleaseByteSource>(
        &mut self,
        catalog: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
        selections: &[BundledPackageRuntimeSelection],
        expected_candidate: BundledCatalogSetIdentity,
        source: &mut S,
    ) -> Result<BundledCatalogSetPromotionOutcome, BundledCatalogSetError> {
        let operation_gate = Arc::clone(self.package_leases.operation_gate());
        let health = Arc::clone(self.package_leases.health());
        let _operation = operation_gate
            .enter(&health)
            .map_err(|error| repository_package_error(error.repository_error()))?;
        validate_projection(catalog.catalog(), selections)?;
        if !self
            .writer_validate_active_catalog_materialized(catalog, exact_catalog_bytes)
            .map_err(repository_package_error)?
        {
            return Err(BundledCatalogSetError::CatalogNotMaterialized);
        }
        let expected = Digest32::from_bytes(expected_candidate.bytes());
        let (candidate, current) = {
            let runtime = self
                .writer_materialization()
                .map_err(repository_package_error)?;
            (
                runtime._state.candidate_catalog_set_id,
                runtime._state.current_catalog_set_id,
            )
        };
        if candidate != Some(expected) && !(candidate.is_none() && current == Some(expected)) {
            return Err(BundledCatalogSetError::StaleSelection);
        }
        let (runtime, verified) =
            self.produce_active_catalog_set(catalog, exact_catalog_bytes, selections, source)?;
        let identity = verified.record_id();
        if identity != expected {
            return Err(self.finish_stale_catalog_set(runtime));
        }
        if let Err(error) = verified.verify_stored(&runtime) {
            return Err(self.finish_catalog_set_object_failure(runtime, error));
        }
        if runtime._state.candidate_catalog_set_id.is_none()
            && runtime._state.current_catalog_set_id == Some(identity)
        {
            drop(runtime);
            self.writer_recover_materialization()
                .map_err(repository_package_error)?;
            return Ok(BundledCatalogSetPromotionOutcome::IdempotentCurrent(
                identity.into(),
            ));
        }
        if runtime._state.candidate_catalog_set_id != Some(identity) {
            return Err(self.finish_stale_catalog_set(runtime));
        }
        self.finish_transition(promote_active_catalog_set(runtime, verified))
            .map_err(BundledCatalogSetError::Package)?;
        Ok(BundledCatalogSetPromotionOutcome::Promoted(identity.into()))
    }

    /// Re-admits an exact explicitly approved rollback candidate and promotes
    /// it without erasing its nominal rollback role.
    pub fn promote_rollback_bundled_catalog_set<S: BundledReleaseByteSource>(
        &mut self,
        catalog: &AdmittedRollbackBundledCatalog,
        exact_catalog_bytes: &[u8],
        selections: &[BundledPackageRuntimeSelection],
        expected_candidate: BundledCatalogSetIdentity,
        source: &mut S,
    ) -> Result<BundledCatalogSetPromotionOutcome, BundledCatalogSetError> {
        let operation_gate = Arc::clone(self.package_leases.operation_gate());
        let health = Arc::clone(self.package_leases.health());
        let _operation = operation_gate
            .enter(&health)
            .map_err(|error| repository_package_error(error.repository_error()))?;
        validate_projection(catalog.catalog(), selections)?;
        if !self
            .writer_validate_rollback_catalog_materialized(catalog, exact_catalog_bytes)
            .map_err(repository_package_error)?
        {
            return Err(BundledCatalogSetError::CatalogNotMaterialized);
        }
        let expected = Digest32::from_bytes(expected_candidate.bytes());
        let (candidate, current) = {
            let runtime = self
                .writer_materialization()
                .map_err(repository_package_error)?;
            (
                runtime._state.candidate_catalog_set_id,
                runtime._state.current_catalog_set_id,
            )
        };
        if candidate != Some(expected) && !(candidate.is_none() && current == Some(expected)) {
            return Err(BundledCatalogSetError::StaleSelection);
        }
        let (runtime, verified) =
            self.produce_rollback_catalog_set(catalog, exact_catalog_bytes, selections, source)?;
        let identity = verified.record_id();
        if identity != expected {
            return Err(self.finish_stale_catalog_set(runtime));
        }
        if let Err(error) = verified.verify_stored(&runtime) {
            return Err(self.finish_catalog_set_object_failure(runtime, error));
        }
        if runtime._state.candidate_catalog_set_id.is_none()
            && runtime._state.current_catalog_set_id == Some(identity)
        {
            drop(runtime);
            self.writer_recover_materialization()
                .map_err(repository_package_error)?;
            return Ok(BundledCatalogSetPromotionOutcome::IdempotentCurrent(
                identity.into(),
            ));
        }
        if runtime._state.candidate_catalog_set_id != Some(identity) {
            return Err(self.finish_stale_catalog_set(runtime));
        }
        self.finish_transition(promote_rollback_catalog_set(runtime, verified))
            .map_err(BundledCatalogSetError::Package)?;
        Ok(BundledCatalogSetPromotionOutcome::Promoted(identity.into()))
    }

    /// Re-admits an explicitly approved rollback selection and atomically
    /// swaps the exact expected current/previous pair. No candidate may exist.
    pub fn rollback_bundled_catalog_set<S: BundledReleaseByteSource>(
        &mut self,
        catalog: &AdmittedRollbackBundledCatalog,
        exact_catalog_bytes: &[u8],
        selections: &[BundledPackageRuntimeSelection],
        expected_current: BundledCatalogSetIdentity,
        expected_previous: BundledCatalogSetIdentity,
        source: &mut S,
    ) -> Result<BundledCatalogSetRollbackOutcome, BundledCatalogSetError> {
        let operation_gate = Arc::clone(self.package_leases.operation_gate());
        let health = Arc::clone(self.package_leases.health());
        let _operation = operation_gate
            .enter(&health)
            .map_err(|error| repository_package_error(error.repository_error()))?;
        validate_projection(catalog.catalog(), selections)?;
        if !self
            .writer_validate_rollback_catalog_materialized(catalog, exact_catalog_bytes)
            .map_err(repository_package_error)?
        {
            return Err(BundledCatalogSetError::CatalogNotMaterialized);
        }
        let current_id = Digest32::from_bytes(expected_current.bytes());
        let previous_id = Digest32::from_bytes(expected_previous.bytes());
        let (candidate, current, previous) = {
            let runtime = self
                .writer_materialization()
                .map_err(repository_package_error)?;
            (
                runtime._state.candidate_catalog_set_id,
                runtime._state.current_catalog_set_id,
                runtime._state.previous_catalog_set_id,
            )
        };
        if candidate.is_some()
            || !((current == Some(current_id) && previous == Some(previous_id))
                || (current == Some(previous_id) && previous == Some(current_id)))
        {
            return Err(BundledCatalogSetError::StaleSelection);
        }
        let (runtime, verified) =
            self.produce_rollback_catalog_set(catalog, exact_catalog_bytes, selections, source)?;
        let rollback_id = verified.record_id();
        if rollback_id != previous_id {
            return Err(self.finish_stale_catalog_set(runtime));
        }
        if let Err(error) = verified.verify_stored(&runtime) {
            return Err(self.finish_catalog_set_object_failure(runtime, error));
        }
        if runtime._state.current_catalog_set_id == Some(rollback_id)
            && runtime._state.previous_catalog_set_id == Some(current_id)
        {
            drop(runtime);
            self.writer_recover_materialization()
                .map_err(repository_package_error)?;
            return Ok(BundledCatalogSetRollbackOutcome::IdempotentRollback(
                rollback_id.into(),
            ));
        }
        if runtime._state.current_catalog_set_id != Some(current_id)
            || runtime._state.previous_catalog_set_id != Some(rollback_id)
        {
            return Err(self.finish_stale_catalog_set(runtime));
        }
        self.finish_transition(rollback_to_previous_catalog_set(runtime, verified))
            .map_err(BundledCatalogSetError::Package)?;
        Ok(BundledCatalogSetRollbackOutcome::RolledBack(
            rollback_id.into(),
        ))
    }

    fn produce_active_catalog_set<S: BundledReleaseByteSource>(
        &mut self,
        catalog: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
        selections: &[BundledPackageRuntimeSelection],
        source: &mut S,
    ) -> Result<(MaterializationRuntime, VerifiedActiveCatalogSet), BundledCatalogSetError> {
        let authority = open_product_manifest_authority()
            .map_err(|error| BundledCatalogSetError::Package(map_preparation_error(error)))?;
        let mut packages = Vec::with_capacity(selections.len());
        for selection in selections {
            let prepared = prepare_active_package(
                catalog,
                exact_catalog_bytes,
                &authority,
                selection.runtime_target,
                selection.package_key,
                source,
            )
            .map_err(|error| BundledCatalogSetError::Package(map_preparation_error(error)))?;
            let capacity = match preflight_package_object_capacity(
                self.writer_materialization()
                    .map_err(repository_package_error)?,
                prepared.record(),
            ) {
                Ok(capacity) => capacity,
                Err(error) => return Err(self.finish_completed_object_failure(error)),
            };
            if capacity.intent_disposition() != PackageObjectIntentDisposition::CompletedReplay {
                return Err(BundledCatalogSetError::PackageNotMaterialized);
            }
            let completed = match verify_completed_active_package(
                self.writer_materialization()
                    .map_err(repository_package_error)?,
                capacity,
                prepared,
            ) {
                Ok(completed) => completed,
                Err(error) => return Err(self.finish_completed_object_failure(error)),
            };
            packages.push(completed);
        }

        let runtime = self
            .writer_take_materialization()
            .map_err(repository_package_error)?;
        let verified = match derive_active_catalog_set(&runtime, catalog, packages) {
            Ok(verified) => verified,
            Err(error) => return Err(self.finish_catalog_set_object_failure(runtime, error)),
        };
        Ok((runtime, verified))
    }

    fn produce_rollback_catalog_set<S: BundledReleaseByteSource>(
        &mut self,
        catalog: &AdmittedRollbackBundledCatalog,
        exact_catalog_bytes: &[u8],
        selections: &[BundledPackageRuntimeSelection],
        source: &mut S,
    ) -> Result<(MaterializationRuntime, VerifiedRollbackCatalogSet), BundledCatalogSetError> {
        let authority = open_product_manifest_authority()
            .map_err(|error| BundledCatalogSetError::Package(map_preparation_error(error)))?;
        let mut packages = Vec::with_capacity(selections.len());
        for selection in selections {
            let prepared = prepare_rollback_package(
                catalog,
                exact_catalog_bytes,
                &authority,
                selection.runtime_target,
                selection.package_key,
                source,
            )
            .map_err(|error| BundledCatalogSetError::Package(map_preparation_error(error)))?;
            let capacity = match preflight_package_object_capacity(
                self.writer_materialization()
                    .map_err(repository_package_error)?,
                prepared.record(),
            ) {
                Ok(capacity) => capacity,
                Err(error) => return Err(self.finish_completed_object_failure(error)),
            };
            if capacity.intent_disposition() != PackageObjectIntentDisposition::CompletedReplay {
                return Err(BundledCatalogSetError::PackageNotMaterialized);
            }
            let completed = match verify_completed_rollback_package(
                self.writer_materialization()
                    .map_err(repository_package_error)?,
                capacity,
                prepared,
            ) {
                Ok(completed) => completed,
                Err(error) => return Err(self.finish_completed_object_failure(error)),
            };
            packages.push(completed);
        }
        let runtime = self
            .writer_take_materialization()
            .map_err(repository_package_error)?;
        let verified = match derive_rollback_catalog_set(&runtime, catalog, packages) {
            Ok(verified) => verified,
            Err(error) => return Err(self.finish_catalog_set_object_failure(runtime, error)),
        };
        Ok((runtime, verified))
    }

    fn finish_catalog_set_object_failure(
        &mut self,
        runtime: crate::materialization::MaterializationRuntime,
        error: crate::materialization::PackageObjectError,
    ) -> BundledCatalogSetError {
        drop(runtime);
        if publication_error_requires_sealing(error)
            || matches!(
                error,
                crate::materialization::PackageObjectError::ExactMismatch
            )
        {
            self.writer_seal();
        } else if let Err(recovery) = self.writer_recover_materialization() {
            self.writer_seal();
            return BundledCatalogSetError::Package(repository_materialization_error(recovery));
        }
        BundledCatalogSetError::Package(map_object_error(error))
    }

    fn finish_completed_object_failure(
        &mut self,
        error: crate::materialization::PackageObjectError,
    ) -> BundledCatalogSetError {
        if completed_error_requires_sealing(error) {
            self.writer_seal();
        }
        BundledCatalogSetError::Package(map_object_error(error))
    }

    fn finish_stale_catalog_set(
        &mut self,
        runtime: MaterializationRuntime,
    ) -> BundledCatalogSetError {
        drop(runtime);
        if let Err(error) = self.writer_recover_materialization() {
            self.writer_seal();
            return repository_package_error(error);
        }
        BundledCatalogSetError::StaleSelection
    }
}

fn validate_projection(
    catalog: &zephium_extension_package::ExtensionReleaseCatalog,
    selections: &[BundledPackageRuntimeSelection],
) -> Result<(), BundledCatalogSetError> {
    if selections.len() != catalog.packages().len()
        || selections
            .iter()
            .zip(catalog.packages())
            .any(|(selection, package)| {
                selection.package_key.bytes() != package.identity().key().bytes()
            })
    {
        return Err(BundledCatalogSetError::InvalidProjection);
    }
    Ok(())
}

fn existing_stage_outcome(
    candidate: Option<Digest32>,
    current: Option<Digest32>,
    previous: Option<Digest32>,
    identity: Digest32,
) -> Option<BundledCatalogSetStageOutcome> {
    let public_identity = BundledCatalogSetIdentity::from(identity);
    if candidate == Some(identity) {
        Some(BundledCatalogSetStageOutcome::IdempotentCandidate(
            public_identity,
        ))
    } else if current == Some(identity) {
        Some(BundledCatalogSetStageOutcome::AlreadyCurrent(
            public_identity,
        ))
    } else if previous == Some(identity) {
        Some(BundledCatalogSetStageOutcome::AlreadyPrevious(
            public_identity,
        ))
    } else {
        None
    }
}

fn repository_package_error(error: ExtensionRepositoryError) -> BundledCatalogSetError {
    BundledCatalogSetError::Package(repository_materialization_error(error))
}

fn repository_materialization_error(
    error: ExtensionRepositoryError,
) -> BundledPackageMaterializationError {
    BundledPackageMaterializationError::Repository(error)
}
