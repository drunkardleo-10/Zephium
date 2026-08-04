//! Authority-preserving orchestration for exact bundled package materialization.
//!
//! This is the only layer that coordinates the outer monotonic catalog floor
//! with the inner package-object and state-transition protocols. Supporting
//! modules deliberately expose linear, role-specific capabilities; this module
//! consumes them without turning a materialized tree into activation, profile,
//! path, receipt, or lease authority.

use thiserror::Error;
use zephium_core::extensions::ExtensionPackageKey;
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, ProductExtensionManifestAdmissionError,
    ProductExtensionManifestAuthorityError, ProductExtensionRuntimeTarget,
};
use zephium_extension_package::ExtensionTreeIndexError;
use zephium_private_fs::PrivateFsError;

use crate::materialization::{
    abort_package_build, begin_active_package_build, begin_rollback_package_build,
    complete_active_package, complete_rollback_package, open_product_manifest_authority,
    preflight_package_object_capacity, prepare_active_package, prepare_rollback_package,
    publish_or_reuse_active_package, publish_or_reuse_rollback_package,
    reconcile_build_stages_for_abort, verify_completed_active_package,
    verify_completed_rollback_package, BundledReleaseByteSource, BundledReleaseSourceError,
    CleanupError, MaterializationRuntime, MaterializationTransitionError, PackageObjectError,
    PackageObjectIntentDisposition, PreparationError, PreparedActivePackage,
    PreparedRollbackPackage,
};
use crate::{ExtensionRepository, ExtensionRepositoryError};

/// Result of exactly materializing one bundled extension package.
///
/// Neither result is activation authority. Both mean the complete package
/// closure was freshly verified before this operation returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[must_use = "new materialization and exact replay have different durable effects"]
pub enum BundledPackageMaterializationOutcome {
    /// This operation durably appended the package to the completed ledger.
    Materialized,
    /// The exact package was already complete and every final was reverified.
    IdempotentReplay,
}

/// Stable, path-free failure while materializing one bundled package.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BundledPackageMaterializationError {
    /// The repository rejected, could not recover, or ambiguously settled an
    /// operation.
    #[error("extension package repository operation failed: {0}")]
    Repository(#[from] ExtensionRepositoryError),
    /// Product-sealed manifest compatibility authority is unavailable or
    /// internally invalid.
    #[error("extension manifest product authority is unavailable: {0}")]
    ManifestAuthority(#[source] ProductExtensionManifestAuthorityError),
    /// Exact manifest bytes or their reviewed compatibility profile were not
    /// admitted by product policy.
    #[error("extension manifest admission failed: {0}")]
    ManifestAdmission(#[source] ProductExtensionManifestAdmissionError),
    /// Exact catalog bytes differ from the authenticated catalog capability.
    #[error("extension catalog bytes do not match their admitted capability")]
    CatalogBytesMismatch,
    /// The authenticated catalog does not contain the requested package key.
    #[error("extension package is absent from the authenticated catalog")]
    PackageMissing,
    /// The selected package is not a bundled resource-tree payload.
    #[error("extension package payload cannot be materialized from bundled resources")]
    UnsupportedPayload,
    /// The requested native or compatibility runtime target is unsupported.
    #[error("extension package runtime target is unsupported")]
    UnsupportedRuntimeTarget,
    /// The fixed bundled-resource adapter refused an exact resource.
    #[error("bundled extension package source failed: {0}")]
    Source(#[source] BundledReleaseSourceError),
    /// Reading an exact resource failed without exposing adapter details.
    #[error("reading an exact bundled extension resource failed")]
    ResourceRead,
    /// An exact resource was shorter or longer than its authenticated bound.
    #[error("bundled extension resource length does not match its authenticated identity")]
    ResourceLengthMismatch,
    /// Exact resource bytes differ from their authenticated SHA-256 identity.
    #[error("bundled extension resource digest does not match its authenticated identity")]
    ResourceDigestMismatch,
    /// A streamed or retained package resource lost its exact identity while
    /// building an otherwise authenticated closure.
    #[error("bundled extension package resource integrity changed during materialization")]
    ResourceIntegrityMismatch,
    /// Canonical extension tree-index validation failed.
    #[error("extension tree index is invalid: {0}")]
    TreeIndex(#[source] ExtensionTreeIndexError),
    /// The canonical tree index differs from the authenticated catalog row.
    #[error("extension tree index does not match the authenticated package")]
    TreeIndexBinding,
    /// A bounded logical or physical object inventory cannot admit the package.
    #[error("extension package materialization capacity is exhausted")]
    CapacityExhausted,
    /// Exact preparation accounting overflowed a fixed production bound.
    #[error("extension package materialization accounting overflowed")]
    AccountingOverflow,
    /// Prepared authority projections disagreed before any durable mutation.
    #[error("extension package preparation produced an inconsistent identity")]
    PreparationInvariant,
    /// A content-addressed durable final was missing, collided, or differed
    /// from its authenticated identity.
    #[error("extension package durable object closure is not exact")]
    DurableObjectMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExistingBuild {
    None,
    MatchingWithoutStages,
    MatchingWithStages,
    Different,
}

impl ExtensionRepository {
    /// Materializes one exact package from the ordinary active product catalog.
    ///
    /// Read-only package preparation occurs first. The authenticated catalog is
    /// then recorded through the repository's monotonic high-water protocol,
    /// which may remain durable if a later clean package-source error occurs.
    /// Retrying the same request is exact and idempotent.
    pub fn materialize_active_bundled_package<S: BundledReleaseByteSource>(
        &mut self,
        catalog: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
        runtime_target: ProductExtensionRuntimeTarget,
        package_key: ExtensionPackageKey,
        source: &mut S,
    ) -> Result<BundledPackageMaterializationOutcome, BundledPackageMaterializationError> {
        self.require_writer_open()?;
        let manifest_authority =
            open_product_manifest_authority().map_err(map_preparation_error)?;
        let prepared = prepare_active_package(
            catalog,
            exact_catalog_bytes,
            &manifest_authority,
            runtime_target,
            package_key,
            source,
        )
        .map_err(map_preparation_error)?;

        // Authenticate, monotonic-plan, and exact-CAS-stage the candidate
        // before an unrelated resumable intent may be aborted. The catalog
        // object is inert until `record_bundled_catalog` advances authority.
        self.writer_stage_active_catalog_candidate(catalog, exact_catalog_bytes)?;

        let existing = {
            let runtime = self.writer_materialization()?;
            match runtime._build_intent.as_ref() {
                None => ExistingBuild::None,
                Some(intent) if &intent.package_record != prepared.record() => {
                    ExistingBuild::Different
                }
                Some(_) if runtime._build_stage.is_some() || !runtime._record_stages.is_empty() => {
                    ExistingBuild::MatchingWithStages
                }
                Some(_) => ExistingBuild::MatchingWithoutStages,
            }
        };
        self.reconcile_existing_build(existing)?;

        // This call owns every outer high-water and package-line interlock. A
        // writer must never publish an active package around that authority.
        let _catalog_record = self.record_bundled_catalog(catalog, exact_catalog_bytes)?;
        self.drive_active_materialization(prepared, source)
    }

    /// Materializes one exact package from an explicitly approved rollback
    /// catalog without lowering or otherwise mutating the outer high-water.
    ///
    /// The rollback catalog object is exact-CAS-published for deterministic
    /// restart recovery only after its relation to the existing active floor
    /// has been validated. Success still grants no activation authority.
    pub fn materialize_rollback_bundled_package<S: BundledReleaseByteSource>(
        &mut self,
        catalog: &AdmittedRollbackBundledCatalog,
        exact_catalog_bytes: &[u8],
        runtime_target: ProductExtensionRuntimeTarget,
        package_key: ExtensionPackageKey,
        source: &mut S,
    ) -> Result<BundledPackageMaterializationOutcome, BundledPackageMaterializationError> {
        self.require_writer_open()?;
        let manifest_authority =
            open_product_manifest_authority().map_err(map_preparation_error)?;
        let prepared = prepare_rollback_package(
            catalog,
            exact_catalog_bytes,
            &manifest_authority,
            runtime_target,
            package_key,
            source,
        )
        .map_err(map_preparation_error)?;

        // Rollback catalog publication is inert and does not lower the outer
        // floor. Complete it before replacing an unrelated resumable intent.
        self.writer_ensure_rollback_catalog(catalog, exact_catalog_bytes)?;

        let existing = {
            let runtime = self.writer_materialization()?;
            match runtime._build_intent.as_ref() {
                None => ExistingBuild::None,
                Some(intent) if &intent.package_record != prepared.record() => {
                    ExistingBuild::Different
                }
                Some(_) if runtime._build_stage.is_some() || !runtime._record_stages.is_empty() => {
                    ExistingBuild::MatchingWithStages
                }
                Some(_) => ExistingBuild::MatchingWithoutStages,
            }
        };
        self.reconcile_existing_build(existing)?;
        self.drive_rollback_materialization(prepared, source)
    }

    fn drive_active_materialization<S: BundledReleaseByteSource>(
        &mut self,
        prepared: PreparedActivePackage,
        source: &mut S,
    ) -> Result<BundledPackageMaterializationOutcome, BundledPackageMaterializationError> {
        loop {
            let (capacity, had_intent) = {
                let runtime = self.writer_materialization()?;
                (
                    preflight_package_object_capacity(runtime, prepared.record()),
                    runtime._build_intent.is_some(),
                )
            };
            let capacity = match capacity {
                Ok(capacity) => capacity,
                Err(error) => return Err(self.handle_preflight_error(error, had_intent)),
            };

            match capacity.intent_disposition() {
                PackageObjectIntentDisposition::RequiresCommit => {
                    let runtime = self.writer_take_materialization()?;
                    self.finish_transition(begin_active_package_build(
                        runtime, capacity, &prepared,
                    ))?;
                    // A capacity proof may not cross a durable transition. The
                    // next loop iteration preflights the freshly recovered
                    // runtime and must observe `AlreadyCommitted`.
                }
                PackageObjectIntentDisposition::AlreadyCommitted => {
                    let mut runtime = self.writer_take_materialization()?;
                    let closure = match publish_or_reuse_active_package(
                        &mut runtime,
                        capacity,
                        prepared,
                        source,
                    ) {
                        Ok(closure) => closure,
                        Err(error) if publication_error_requires_sealing(error) => {
                            drop(runtime);
                            return Err(self.seal_object_error(error));
                        }
                        Err(error) => {
                            let public_error = map_publication_error(error);
                            self.abort_after_clean_publication_failure(runtime)?;
                            return Err(public_error);
                        }
                    };
                    self.finish_transition(complete_active_package(runtime, closure))?;
                    return Ok(BundledPackageMaterializationOutcome::Materialized);
                }
                PackageObjectIntentDisposition::CompletedReplay => {
                    let verified = {
                        let runtime = self.writer_materialization()?;
                        verify_completed_active_package(runtime, capacity, prepared)
                    };
                    match verified {
                        Ok(verified) => {
                            let (
                                _record_id,
                                _record,
                                _tree_root,
                                _records_parent,
                                _trees_parent,
                                _active_authority,
                            ) = verified.into_parts();
                            return Ok(BundledPackageMaterializationOutcome::IdempotentReplay);
                        }
                        Err(error) if completed_error_requires_sealing(error) => {
                            return Err(self.seal_object_error(error));
                        }
                        Err(error) => return Err(map_object_error(error)),
                    }
                }
            }
        }
    }

    fn drive_rollback_materialization<S: BundledReleaseByteSource>(
        &mut self,
        prepared: PreparedRollbackPackage,
        source: &mut S,
    ) -> Result<BundledPackageMaterializationOutcome, BundledPackageMaterializationError> {
        loop {
            let (capacity, had_intent) = {
                let runtime = self.writer_materialization()?;
                (
                    preflight_package_object_capacity(runtime, prepared.record()),
                    runtime._build_intent.is_some(),
                )
            };
            let capacity = match capacity {
                Ok(capacity) => capacity,
                Err(error) => return Err(self.handle_preflight_error(error, had_intent)),
            };

            match capacity.intent_disposition() {
                PackageObjectIntentDisposition::RequiresCommit => {
                    let runtime = self.writer_take_materialization()?;
                    self.finish_transition(begin_rollback_package_build(
                        runtime, capacity, &prepared,
                    ))?;
                }
                PackageObjectIntentDisposition::AlreadyCommitted => {
                    let mut runtime = self.writer_take_materialization()?;
                    let closure = match publish_or_reuse_rollback_package(
                        &mut runtime,
                        capacity,
                        prepared,
                        source,
                    ) {
                        Ok(closure) => closure,
                        Err(error) if publication_error_requires_sealing(error) => {
                            drop(runtime);
                            return Err(self.seal_object_error(error));
                        }
                        Err(error) => {
                            let public_error = map_publication_error(error);
                            self.abort_after_clean_publication_failure(runtime)?;
                            return Err(public_error);
                        }
                    };
                    self.finish_transition(complete_rollback_package(runtime, closure))?;
                    return Ok(BundledPackageMaterializationOutcome::Materialized);
                }
                PackageObjectIntentDisposition::CompletedReplay => {
                    let verified = {
                        let runtime = self.writer_materialization()?;
                        verify_completed_rollback_package(runtime, capacity, prepared)
                    };
                    match verified {
                        Ok(verified) => {
                            let (
                                _record_id,
                                _record,
                                _tree_root,
                                _records_parent,
                                _trees_parent,
                                _rollback_authority,
                            ) = verified.into_parts();
                            return Ok(BundledPackageMaterializationOutcome::IdempotentReplay);
                        }
                        Err(error) if completed_error_requires_sealing(error) => {
                            return Err(self.seal_object_error(error));
                        }
                        Err(error) => return Err(map_object_error(error)),
                    }
                }
            }
        }
    }

    fn require_writer_open(&self) -> Result<(), BundledPackageMaterializationError> {
        if self.writer_is_sealed() {
            Err(ExtensionRepositoryError::Sealed.into())
        } else {
            Ok(())
        }
    }

    fn reconcile_existing_build(
        &mut self,
        existing: ExistingBuild,
    ) -> Result<(), BundledPackageMaterializationError> {
        match existing {
            ExistingBuild::None | ExistingBuild::MatchingWithoutStages => return Ok(()),
            ExistingBuild::MatchingWithStages | ExistingBuild::Different => {}
        }

        let mut runtime = self.writer_take_materialization()?;
        let stages_absent = match reconcile_build_stages_for_abort(&mut runtime) {
            Ok(proof) => proof,
            Err(error) => {
                drop(runtime);
                return Err(self.finish_cleanup_failure(error));
            }
        };

        if existing == ExistingBuild::MatchingWithStages {
            // Partial stage bytes are not authority and are intentionally
            // discarded. The durable exact intent remains resumable.
            let _ = stages_absent;
            drop(runtime);
            return self.recover_consumed_runtime();
        }

        self.finish_transition(abort_package_build(runtime, stages_absent))
    }

    fn abort_after_clean_publication_failure(
        &mut self,
        mut runtime: MaterializationRuntime,
    ) -> Result<(), BundledPackageMaterializationError> {
        let stages_absent = match reconcile_build_stages_for_abort(&mut runtime) {
            Ok(proof) => proof,
            Err(error) => {
                drop(runtime);
                return Err(self.finish_cleanup_failure(error));
            }
        };
        self.finish_transition(abort_package_build(runtime, stages_absent))
    }

    pub(crate) fn finish_transition<Committed>(
        &mut self,
        transition: Result<Committed, MaterializationTransitionError>,
    ) -> Result<(), BundledPackageMaterializationError> {
        match transition {
            Ok(_committed) => self.recover_consumed_runtime(),
            Err(MaterializationTransitionError::Clean(error)) => {
                self.recover_consumed_runtime()?;
                Err(error.into())
            }
            Err(MaterializationTransitionError::MustSeal(error)) => {
                self.writer_seal();
                Err(error.into())
            }
        }
    }

    fn recover_consumed_runtime(&mut self) -> Result<(), BundledPackageMaterializationError> {
        match self.writer_recover_materialization() {
            Ok(()) => Ok(()),
            Err(error) => {
                self.writer_seal();
                Err(error.into())
            }
        }
    }

    fn finish_cleanup_failure(
        &mut self,
        error: CleanupError,
    ) -> BundledPackageMaterializationError {
        if cleanup_error_requires_sealing(error) {
            self.writer_seal();
            return map_cleanup_error(error);
        }
        match self.writer_recover_materialization() {
            Ok(()) => map_cleanup_error(error),
            Err(recovery) => {
                self.writer_seal();
                recovery.into()
            }
        }
    }

    fn handle_preflight_error(
        &mut self,
        error: PackageObjectError,
        had_intent: bool,
    ) -> BundledPackageMaterializationError {
        if preflight_error_requires_sealing(error, had_intent) {
            self.writer_seal();
        }
        map_object_error(error)
    }

    fn seal_object_error(
        &mut self,
        error: PackageObjectError,
    ) -> BundledPackageMaterializationError {
        self.writer_seal();
        map_object_error(error)
    }
}

pub(crate) fn map_preparation_error(error: PreparationError) -> BundledPackageMaterializationError {
    match error {
        PreparationError::CatalogLengthMismatch | PreparationError::CatalogDigestMismatch => {
            BundledPackageMaterializationError::CatalogBytesMismatch
        }
        PreparationError::PackageMissing => BundledPackageMaterializationError::PackageMissing,
        PreparationError::UnsupportedPayload => {
            BundledPackageMaterializationError::UnsupportedPayload
        }
        PreparationError::UnsupportedRuntimeTarget => {
            BundledPackageMaterializationError::UnsupportedRuntimeTarget
        }
        PreparationError::ManifestAuthority(error) => {
            BundledPackageMaterializationError::ManifestAuthority(error)
        }
        PreparationError::Source(error) => BundledPackageMaterializationError::Source(error),
        PreparationError::Read => BundledPackageMaterializationError::ResourceRead,
        PreparationError::ResourceLengthMismatch => {
            BundledPackageMaterializationError::ResourceLengthMismatch
        }
        PreparationError::ResourceDigestMismatch => {
            BundledPackageMaterializationError::ResourceDigestMismatch
        }
        PreparationError::TreeIndex(error) => BundledPackageMaterializationError::TreeIndex(error),
        PreparationError::TreeIndexBinding => BundledPackageMaterializationError::TreeIndexBinding,
        PreparationError::ManifestAdmission(error) => {
            BundledPackageMaterializationError::ManifestAdmission(error)
        }
        PreparationError::AccountingOverflow => {
            BundledPackageMaterializationError::AccountingOverflow
        }
        PreparationError::InvalidRecord => BundledPackageMaterializationError::PreparationInvariant,
    }
}

pub(crate) fn map_object_error(error: PackageObjectError) -> BundledPackageMaterializationError {
    match error {
        PackageObjectError::CapacityExhausted => {
            BundledPackageMaterializationError::CapacityExhausted
        }
        PackageObjectError::GenerationExhausted => {
            ExtensionRepositoryError::GenerationExhausted.into()
        }
        PackageObjectError::Source(error) => BundledPackageMaterializationError::Source(error),
        PackageObjectError::Filesystem(error) => ExtensionRepositoryError::FileSystem(error).into(),
        PackageObjectError::SettlementAmbiguous => {
            ExtensionRepositoryError::SettlementAmbiguous.into()
        }
        PackageObjectError::BuildStateMismatch => {
            ExtensionRepositoryError::RecoveryAmbiguous.into()
        }
        PackageObjectError::Collision | PackageObjectError::ExactMismatch => {
            BundledPackageMaterializationError::DurableObjectMismatch
        }
    }
}

fn map_publication_error(error: PackageObjectError) -> BundledPackageMaterializationError {
    match error {
        PackageObjectError::ExactMismatch => {
            BundledPackageMaterializationError::ResourceIntegrityMismatch
        }
        other => map_object_error(other),
    }
}

fn map_cleanup_error(error: CleanupError) -> BundledPackageMaterializationError {
    match error {
        CleanupError::Filesystem(error) => ExtensionRepositoryError::FileSystem(error).into(),
        CleanupError::SettlementAmbiguous => ExtensionRepositoryError::SettlementAmbiguous.into(),
        CleanupError::BuildStateMismatch | CleanupError::ExactMismatch => {
            ExtensionRepositoryError::RecoveryAmbiguous.into()
        }
    }
}

const fn preflight_error_requires_sealing(error: PackageObjectError, had_intent: bool) -> bool {
    match error {
        PackageObjectError::BuildStateMismatch
        | PackageObjectError::Collision
        | PackageObjectError::ExactMismatch
        | PackageObjectError::SettlementAmbiguous => true,
        PackageObjectError::GenerationExhausted => had_intent,
        PackageObjectError::Filesystem(error) => filesystem_error_requires_sealing(error),
        PackageObjectError::CapacityExhausted | PackageObjectError::Source(_) => false,
    }
}

pub(crate) const fn publication_error_requires_sealing(error: PackageObjectError) -> bool {
    match error {
        PackageObjectError::BuildStateMismatch
        | PackageObjectError::CapacityExhausted
        | PackageObjectError::GenerationExhausted
        | PackageObjectError::Collision
        | PackageObjectError::SettlementAmbiguous => true,
        PackageObjectError::Filesystem(error) => filesystem_error_requires_sealing(error),
        PackageObjectError::ExactMismatch | PackageObjectError::Source(_) => false,
    }
}

pub(crate) const fn completed_error_requires_sealing(error: PackageObjectError) -> bool {
    match error {
        PackageObjectError::Filesystem(error) => filesystem_error_requires_sealing(error),
        PackageObjectError::Source(_) => false,
        PackageObjectError::BuildStateMismatch
        | PackageObjectError::CapacityExhausted
        | PackageObjectError::GenerationExhausted
        | PackageObjectError::Collision
        | PackageObjectError::ExactMismatch
        | PackageObjectError::SettlementAmbiguous => true,
    }
}

const fn cleanup_error_requires_sealing(error: CleanupError) -> bool {
    match error {
        CleanupError::BuildStateMismatch
        | CleanupError::ExactMismatch
        | CleanupError::SettlementAmbiguous => true,
        CleanupError::Filesystem(error) => filesystem_error_requires_sealing(error),
    }
}

const fn filesystem_error_requires_sealing(error: PrivateFsError) -> bool {
    matches!(
        error,
        PrivateFsError::NotFound
            | PrivateFsError::ReservedComponent
            | PrivateFsError::Unsafe
            | PrivateFsError::BoundExceeded
            | PrivateFsError::AlreadyExists
            | PrivateFsError::NamespaceMismatch
            | PrivateFsError::DirectoryNotEmpty
            | PrivateFsError::IdentityAmbiguous
            | PrivateFsError::SettlementUnknown
            | PrivateFsError::Quarantined
    )
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
#[path = "writer/tests.rs"]
mod repository_e2e_tests;
