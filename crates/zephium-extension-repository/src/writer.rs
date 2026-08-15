//! Authority-preserving orchestration for exact bundled package materialization.
//!
//! This is the only layer that coordinates the outer monotonic catalog floor
//! with the inner package-object and state-transition protocols. Supporting
//! modules deliberately expose linear, role-specific capabilities; this module
//! consumes them without turning a materialized tree into activation, profile,
//! path, receipt, or lease authority.

use thiserror::Error;
use zephium_core::extensions::ExtensionPackageKey;
#[cfg(feature = "acquired-packages")]
use zephium_extension_acquisition::{AcquiredExtensionArchive, AcquiredExtensionArchiveError};
#[cfg(feature = "acquired-packages")]
use zephium_extension_authority::AdmittedAcquiredCatalog;
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, ProductExtensionManifestAdmissionError,
    ProductExtensionManifestAuthorityError, ProductExtensionRuntimeTarget,
};
use zephium_extension_package::ExtensionTreeIndexError;
use zephium_private_fs::PrivateFsError;

use crate::materialization::{
    abort_package_build, begin_active_package_build, begin_rollback_package_build,
    complete_active_package, complete_rollback_package, inspect_package_build_commit_marker,
    open_product_manifest_authority, preflight_package_object_capacity, prepare_active_package,
    prepare_rollback_package, publish_or_reuse_active_package, publish_or_reuse_rollback_package,
    reconcile_build_stages_for_abort, verify_completed_active_package,
    verify_completed_rollback_package, BundledReleaseByteSource, BundledReleaseSourceError,
    CleanupError, MaterializationRuntime, MaterializationTransitionError, PackageBuildCommitMarker,
    PackageObjectError, PackageObjectIntentDisposition, PreparationError, PreparedActivePackage,
    PreparedRollbackPackage,
};
#[cfg(feature = "acquired-packages")]
use crate::materialization::{
    begin_acquired_active_package_build, build_authenticated_acquired_tree_evidence,
    build_authenticated_acquired_tree_stage, cleanup_acquisition_tree_stage,
    complete_acquired_active_package, preflight_acquired_package_object_capacity,
    prepare_acquired_active_package, publish_or_reuse_acquired_active_package,
    tree_acquisition_stage, verify_completed_acquired_active_package, AcquiredReleaseLegalSource,
    AcquiredReleaseLegalSourceError, AuthenticatedTreeStage, TreeWriterError,
};
#[cfg(feature = "acquired-packages")]
use crate::state::Digest32;
use crate::{
    BundledPackageBuildSettlementError, BundledPackageBuildSettlementOutcome, ExtensionRepository,
    ExtensionRepositoryError,
};

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
    /// A previously interrupted package build could not be settled before this
    /// request consulted its external byte source.
    #[error("interrupted extension package build settlement failed: {0}")]
    InterruptedBuildSettlement(#[source] BundledPackageBuildSettlementError),
}

/// Result of exactly materializing one catalog-authenticated acquired package.
#[cfg(feature = "acquired-packages")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[must_use = "new materialization and exact replay have different durable effects"]
pub enum AcquiredPackageMaterializationOutcome {
    /// This operation durably appended the package to the completed ledger.
    Materialized,
    /// The exact package was already complete and every final was reverified.
    IdempotentReplay,
}

/// Stable, path-free failure while materializing one acquired package.
#[cfg(feature = "acquired-packages")]
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum AcquiredPackageMaterializationError {
    /// Repository recovery, I/O, or durable transition failed.
    #[error("acquired extension package repository operation failed: {0}")]
    Repository(#[from] ExtensionRepositoryError),
    /// CRX signature, developer identity, ZIP framing, or archive policy failed.
    #[error("acquired extension package authentication failed: {0}")]
    Archive(#[source] AcquiredExtensionArchiveError),
    /// Product manifest authority is unavailable or invalid.
    #[error("extension manifest product authority is unavailable: {0}")]
    ManifestAuthority(#[source] ProductExtensionManifestAuthorityError),
    /// Exact manifest bytes are not admitted for the requested runtime.
    #[error("extension manifest admission failed: {0}")]
    ManifestAdmission(#[source] ProductExtensionManifestAdmissionError),
    /// Exact catalog bytes differ from their admitted capability.
    #[error("extension catalog bytes do not match their admitted capability")]
    CatalogBytesMismatch,
    /// The selected package row is absent.
    #[error("extension package is absent from the authenticated catalog")]
    PackageMissing,
    /// The selected row is not an acquired ZIP payload.
    #[error("extension package payload is not an acquired archive")]
    UnsupportedPayload,
    /// The requested native or compatibility runtime is unsupported.
    #[error("extension package runtime target is unsupported")]
    UnsupportedRuntimeTarget,
    /// The fixed legal-resource adapter refused an exact notice resource.
    #[error("extension legal resource source failed: {0}")]
    LegalSource(#[source] AcquiredReleaseLegalSourceError),
    /// Reading an exact retained resource failed.
    #[error("reading an exact extension package resource failed")]
    ResourceRead,
    /// Exact resource length differs from authenticated metadata.
    #[error("extension package resource length does not match its identity")]
    ResourceLengthMismatch,
    /// Exact resource digest differs from authenticated metadata.
    #[error("extension package resource digest does not match its identity")]
    ResourceDigestMismatch,
    /// Archive output or a staged resource changed before durable closure.
    #[error("acquired extension package resource integrity changed during materialization")]
    ResourceIntegrityMismatch,
    /// The canonical tree index failed semantic validation.
    #[error("extension tree index is invalid: {0}")]
    TreeIndex(#[source] ExtensionTreeIndexError),
    /// The canonical tree index differs from the authenticated package row.
    #[error("extension tree index does not match the authenticated package")]
    TreeIndexBinding,
    /// A bounded logical or physical inventory cannot admit the package.
    #[error("extension package materialization capacity is exhausted")]
    CapacityExhausted,
    /// Exact preparation accounting exceeded a fixed production bound.
    #[error("extension package materialization accounting overflowed")]
    AccountingOverflow,
    /// Prepared authority projections disagreed before durable mutation.
    #[error("extension package preparation produced an inconsistent identity")]
    PreparationInvariant,
    /// A durable final was missing, collided, or differed from its identity.
    #[error("extension package durable object closure is not exact")]
    DurableObjectMismatch,
    /// A prior interrupted package build could not settle source-free.
    #[error("interrupted extension package build settlement failed: {0}")]
    InterruptedBuildSettlement(#[source] BundledPackageBuildSettlementError),
}

#[cfg(feature = "acquired-packages")]
struct AcquiredPackageMaterializationRequest<'a, S: AcquiredReleaseLegalSource> {
    catalog: &'a AdmittedAcquiredCatalog,
    exact_catalog_bytes: &'a [u8],
    runtime_target: ProductExtensionRuntimeTarget,
    package_key: ExtensionPackageKey,
    crx3_bytes: &'a [u8],
    legal_source: &'a mut S,
    fault: crate::materialization::ObjectPublicationFaultPoint,
}

impl ExtensionRepository {
    /// Materializes one exact package from the ordinary active product catalog.
    ///
    /// Any durable prior build settles source-free before package preparation
    /// can invoke the supplied adapter. The authenticated catalog is then
    /// recorded through the repository's monotonic high-water protocol, which
    /// may remain durable if a later clean package-source error occurs. Retrying
    /// the same request is exact and idempotent.
    pub fn materialize_active_bundled_package<S: BundledReleaseByteSource>(
        &mut self,
        catalog: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
        runtime_target: ProductExtensionRuntimeTarget,
        package_key: ExtensionPackageKey,
        source: &mut S,
    ) -> Result<BundledPackageMaterializationOutcome, BundledPackageMaterializationError> {
        let runtime = self.runtime.clone();
        let operation = runtime.enter().map_err(|error| error.repository_error())?;
        self.require_writer_open()?;
        self.writer_require_gc_idle()?;
        let interrupted_record = self
            .writer_materialization()?
            ._build_intent
            .as_ref()
            .map(|intent| intent.package_record.clone());
        let settlement = self
            .settle_interrupted_bundled_package_build_under_gate()
            .map_err(BundledPackageMaterializationError::InterruptedBuildSettlement)?;
        if settlement == BundledPackageBuildSettlementOutcome::Completed
            && interrupted_record.as_ref().is_some_and(|record| {
                record.catalog.generation_anchor().ok() == Some(catalog.generation_anchor())
                    && record.package.package_key.bytes() == package_key.bytes()
                    && record.manifest.runtime_target.product_target() == runtime_target
            })
        {
            // Settlement freshly re-admitted the repository-owned catalog and
            // completed this exact closure without consulting `source`. Bind
            // the caller's independent authority witness and exact bytes before
            // returning so a forged or equivocated retry can never inherit it.
            self.writer_stage_active_catalog_candidate(catalog, exact_catalog_bytes)?;
            let _catalog_record =
                self.record_bundled_catalog_under_gate(&operation, catalog, exact_catalog_bytes)?;
            return Ok(BundledPackageMaterializationOutcome::Materialized);
        }
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
        // Authenticate, monotonic-plan, and exact-CAS-stage the candidate after
        // settlement has proved no prior build remains. The catalog object is
        // inert until `record_bundled_catalog` advances authority.
        self.writer_stage_active_catalog_candidate(catalog, exact_catalog_bytes)?;

        // This call owns every outer high-water and package-line interlock. A
        // writer must never publish an active package around that authority.
        let _catalog_record =
            self.record_bundled_catalog_under_gate(&operation, catalog, exact_catalog_bytes)?;
        self.drive_active_materialization(prepared, source)
    }

    /// Materializes one exact CRX3 package from an authenticated active
    /// acquired catalog.
    ///
    /// The method performs no network access and accepts no filesystem path.
    /// The borrowed CRX bytes are signature checked, catalog-bound, bounded,
    /// streamed once into either a private tree stage or zero-retention sinks,
    /// and admitted before any package intent becomes durable. Only the legal
    /// notice uses the fixed release-resource callback.
    #[cfg(feature = "acquired-packages")]
    pub fn materialize_active_acquired_package<S: AcquiredReleaseLegalSource>(
        &mut self,
        catalog: &AdmittedAcquiredCatalog,
        exact_catalog_bytes: &[u8],
        runtime_target: ProductExtensionRuntimeTarget,
        package_key: ExtensionPackageKey,
        crx3_bytes: &[u8],
        legal_source: &mut S,
    ) -> Result<AcquiredPackageMaterializationOutcome, AcquiredPackageMaterializationError> {
        self.materialize_active_acquired_package_request(AcquiredPackageMaterializationRequest {
            catalog,
            exact_catalog_bytes,
            runtime_target,
            package_key,
            crx3_bytes,
            legal_source,
            fault: crate::materialization::ObjectPublicationFaultPoint::None,
        })
    }

    #[cfg(feature = "acquired-packages")]
    fn materialize_active_acquired_package_request<S: AcquiredReleaseLegalSource>(
        &mut self,
        request: AcquiredPackageMaterializationRequest<'_, S>,
    ) -> Result<AcquiredPackageMaterializationOutcome, AcquiredPackageMaterializationError> {
        let AcquiredPackageMaterializationRequest {
            catalog,
            exact_catalog_bytes,
            runtime_target,
            package_key,
            crx3_bytes,
            legal_source,
            fault,
        } = request;
        let runtime = self.runtime.clone();
        let operation = runtime.enter().map_err(|error| error.repository_error())?;
        self.require_writer_open()
            .map_err(map_bundled_to_acquired_error)?;
        self.writer_require_gc_idle()?;
        let interrupted_record = self
            .writer_materialization()?
            ._build_intent
            .as_ref()
            .map(|intent| intent.package_record.clone());
        let settlement = self
            .settle_interrupted_bundled_package_build_under_gate()
            .map_err(AcquiredPackageMaterializationError::InterruptedBuildSettlement)?;
        if settlement == BundledPackageBuildSettlementOutcome::Completed
            && interrupted_record.as_ref().is_some_and(|record| {
                record.catalog.generation_anchor().ok() == Some(catalog.generation_anchor())
                    && record.package.package_key.bytes() == package_key.bytes()
                    && record.manifest.runtime_target.product_target() == runtime_target
            })
        {
            // Recovery re-admitted repository-owned authority and completed
            // this exact closure without the CRX or legal source. Rebind the
            // caller's independent admitted catalog and exact bytes before
            // returning so an equivocated retry cannot inherit that result.
            self.writer_stage_active_acquired_catalog_candidate(catalog, exact_catalog_bytes)?;
            let _catalog_record =
                self.record_acquired_catalog_under_gate(&operation, catalog, exact_catalog_bytes)?;
            return Ok(AcquiredPackageMaterializationOutcome::Materialized);
        }

        // The outer catalog transition reopens the inner materialization
        // namespace. Advance it before creating an unowned acquisition stage,
        // otherwise correct restart cleanup would discard that stage.
        self.writer_stage_active_acquired_catalog_candidate(catalog, exact_catalog_bytes)?;
        let _catalog_record =
            self.record_acquired_catalog_under_gate(&operation, catalog, exact_catalog_bytes)?;

        let package = catalog
            .catalog()
            .package(package_key)
            .ok_or(AcquiredPackageMaterializationError::PackageMissing)?;
        let mut archive =
            AcquiredExtensionArchive::authenticate_release_package_crx3(crx3_bytes, package)
                .map_err(AcquiredPackageMaterializationError::Archive)?;
        let tree_id = Digest32::from_bytes(package.identity().tree_sha256().bytes());
        let tree_exists = self
            .writer_materialization()?
            ._tree_object_ids
            .contains(&tree_id);
        let stage_name = tree_acquisition_stage(tree_id);

        let evidence = if tree_exists {
            build_authenticated_acquired_tree_evidence(package, &mut archive).map(|evidence| {
                let (receipt, manifest) = evidence.into_parts();
                (None, receipt, manifest)
            })
        } else {
            build_authenticated_acquired_tree_stage(
                &self.writer_materialization()?._trees,
                &stage_name,
                package,
                &mut archive,
            )
            .map(|evidence| {
                let (stage, receipt, manifest) = evidence.into_parts();
                (Some(stage), receipt, manifest)
            })
        };
        let (mut tree_stage, receipt, manifest_bytes) = match evidence {
            Ok(evidence) => evidence,
            Err(error) => {
                self.settle_failed_preintent_acquisition_stage(&stage_name, !tree_exists, error)?;
                return Err(map_acquired_tree_error(error));
            }
        };

        let manifest_authority = match open_product_manifest_authority() {
            Ok(authority) => authority,
            Err(error) => {
                self.remove_owned_preintent_acquisition_stage(&stage_name, tree_stage.take())?;
                return Err(map_acquired_preparation_error(error));
            }
        };
        let prepared = prepare_acquired_active_package(
            catalog,
            exact_catalog_bytes,
            &manifest_authority,
            runtime_target,
            package_key,
            receipt,
            manifest_bytes,
        );
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.remove_owned_preintent_acquisition_stage(&stage_name, tree_stage.take())?;
                return Err(map_acquired_preparation_error(error));
            }
        };

        self.drive_acquired_materialization(prepared, tree_stage, legal_source, fault)
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
        let runtime = self.runtime.clone();
        let _operation = runtime.enter().map_err(|error| error.repository_error())?;
        self.require_writer_open()?;
        self.writer_require_gc_idle()?;
        let interrupted_record = self
            .writer_materialization()?
            ._build_intent
            .as_ref()
            .map(|intent| intent.package_record.clone());
        let settlement = self
            .settle_interrupted_bundled_package_build_under_gate()
            .map_err(BundledPackageMaterializationError::InterruptedBuildSettlement)?;
        if settlement == BundledPackageBuildSettlementOutcome::Completed
            && interrupted_record.as_ref().is_some_and(|record| {
                record.catalog.generation_anchor().ok() == Some(catalog.generation_anchor())
                    && record.package.package_key.bytes() == package_key.bytes()
                    && record.manifest.runtime_target.product_target() == runtime_target
            })
        {
            // Rollback catalog publication remains inert and cannot lower the
            // active floor. This also revalidates the caller's exact bytes
            // before returning the source-free completion result.
            self.writer_ensure_rollback_catalog(catalog, exact_catalog_bytes)?;
            return Ok(BundledPackageMaterializationOutcome::Materialized);
        }
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
        // floor. Source-free settlement above proved no prior intent remains.
        self.writer_ensure_rollback_catalog(catalog, exact_catalog_bytes)?;

        self.drive_rollback_materialization(prepared, source)
    }

    #[cfg(feature = "acquired-packages")]
    fn drive_acquired_materialization<S: AcquiredReleaseLegalSource>(
        &mut self,
        prepared: crate::materialization::PreparedAcquiredActivePackage,
        mut tree_stage: Option<AuthenticatedTreeStage>,
        legal_source: &mut S,
        fault: crate::materialization::ObjectPublicationFaultPoint,
    ) -> Result<AcquiredPackageMaterializationOutcome, AcquiredPackageMaterializationError> {
        loop {
            let stage_present = tree_stage.is_some();
            let (capacity, had_intent) = {
                let runtime = self.writer_materialization()?;
                (
                    preflight_acquired_package_object_capacity(
                        runtime,
                        prepared.record(),
                        stage_present,
                    ),
                    runtime._build_intent.is_some(),
                )
            };
            let capacity = match capacity {
                Ok(capacity) => capacity,
                Err(error) => {
                    if !had_intent && tree_stage.is_some() {
                        if preflight_error_requires_sealing(error, false) {
                            drop(tree_stage.take());
                            self.writer_seal();
                        } else {
                            let stage_name =
                                tree_acquisition_stage(prepared.record().tree_index.tree_sha256);
                            self.remove_owned_preintent_acquisition_stage(
                                &stage_name,
                                tree_stage.take(),
                            )?;
                        }
                    } else if preflight_error_requires_sealing(error, had_intent) {
                        self.writer_seal();
                    }
                    return Err(map_acquired_object_error(error));
                }
            };

            match capacity.intent_disposition() {
                PackageObjectIntentDisposition::RequiresCommit => {
                    let runtime = self.writer_take_materialization()?;
                    let transition = begin_acquired_active_package_build(
                        runtime,
                        capacity,
                        &prepared,
                        stage_present,
                    );
                    if transition.is_err() {
                        drop(tree_stage.take());
                    }
                    self.finish_transition(transition)
                        .map_err(map_bundled_to_acquired_error)?;
                }
                PackageObjectIntentDisposition::AlreadyCommitted => {
                    let mut runtime = self.writer_take_materialization()?;
                    let closure = match publish_or_reuse_acquired_active_package(
                        &mut runtime,
                        capacity,
                        prepared,
                        tree_stage.take(),
                        legal_source,
                        fault,
                    ) {
                        Ok(closure) => closure,
                        Err(error) if publication_error_requires_sealing(error) => {
                            drop(runtime);
                            self.writer_seal();
                            return Err(map_acquired_object_error(error));
                        }
                        Err(error) => {
                            let public = map_acquired_publication_error(error);
                            if self
                                .settle_after_clean_publication_failure(runtime)
                                .map_err(map_bundled_to_acquired_error)?
                            {
                                return Ok(AcquiredPackageMaterializationOutcome::Materialized);
                            }
                            return Err(public);
                        }
                    };
                    self.finish_transition(complete_acquired_active_package(runtime, closure))
                        .map_err(map_bundled_to_acquired_error)?;
                    return Ok(AcquiredPackageMaterializationOutcome::Materialized);
                }
                PackageObjectIntentDisposition::CompletedReplay => {
                    let verified = {
                        let runtime = self.writer_materialization()?;
                        verify_completed_acquired_active_package(runtime, capacity, prepared)
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
                            return Ok(AcquiredPackageMaterializationOutcome::IdempotentReplay);
                        }
                        Err(error) if completed_error_requires_sealing(error) => {
                            self.writer_seal();
                            return Err(map_acquired_object_error(error));
                        }
                        Err(error) => return Err(map_acquired_object_error(error)),
                    }
                }
            }
        }
    }

    #[cfg(feature = "acquired-packages")]
    fn settle_failed_preintent_acquisition_stage(
        &mut self,
        stage_name: &zephium_private_fs::PrivateComponent,
        stage_was_attempted: bool,
        error: TreeWriterError,
    ) -> Result<(), AcquiredPackageMaterializationError> {
        if !stage_was_attempted {
            return Ok(());
        }
        if matches!(
            error,
            TreeWriterError::TransitionAmbiguous
                | TreeWriterError::Filesystem(
                    PrivateFsError::AlreadyExists
                        | PrivateFsError::IdentityAmbiguous
                        | PrivateFsError::SettlementUnknown
                        | PrivateFsError::Quarantined
                )
        ) {
            self.writer_seal();
            return Err(ExtensionRepositoryError::SettlementAmbiguous.into());
        }
        match cleanup_acquisition_tree_stage(&self.writer_materialization()?._trees, stage_name) {
            Ok(_) => Ok(()),
            Err(_) => {
                self.writer_seal();
                Err(ExtensionRepositoryError::SettlementAmbiguous.into())
            }
        }
    }

    #[cfg(feature = "acquired-packages")]
    fn remove_owned_preintent_acquisition_stage(
        &mut self,
        stage_name: &zephium_private_fs::PrivateComponent,
        stage: Option<AuthenticatedTreeStage>,
    ) -> Result<(), AcquiredPackageMaterializationError> {
        let Some(stage) = stage else {
            return Ok(());
        };
        drop(stage);
        match cleanup_acquisition_tree_stage(&self.writer_materialization()?._trees, stage_name) {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => {
                self.writer_seal();
                Err(ExtensionRepositoryError::SettlementAmbiguous.into())
            }
        }
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
                            if self.settle_after_clean_publication_failure(runtime)? {
                                return Ok(BundledPackageMaterializationOutcome::Materialized);
                            }
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
                            if self.settle_after_clean_publication_failure(runtime)? {
                                return Ok(BundledPackageMaterializationOutcome::Materialized);
                            }
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

    fn settle_after_clean_publication_failure(
        &mut self,
        mut runtime: MaterializationRuntime,
    ) -> Result<bool, BundledPackageMaterializationError> {
        let marker = match inspect_package_build_commit_marker(&runtime) {
            Ok(marker) => marker,
            Err(error) => {
                drop(runtime);
                return Err(self.finish_cleanup_failure(error));
            }
        };
        match marker {
            PackageBuildCommitMarker::Absent(marker_absent) => {
                let abortable = match reconcile_build_stages_for_abort(&mut runtime, marker_absent)
                {
                    Ok(proof) => proof,
                    Err(CleanupError::CommitMarkerPresent) => {
                        drop(runtime);
                        self.recover_consumed_runtime()?;
                        return self
                            .settle_interrupted_bundled_package_build_under_gate()
                            .map(|outcome| {
                                outcome == BundledPackageBuildSettlementOutcome::Completed
                            })
                            .map_err(
                                BundledPackageMaterializationError::InterruptedBuildSettlement,
                            );
                    }
                    Err(error) => {
                        drop(runtime);
                        return Err(self.finish_cleanup_failure(error));
                    }
                };
                self.finish_transition(abort_package_build(runtime, abortable))?;
                Ok(false)
            }
            PackageBuildCommitMarker::Present => {
                drop(runtime);
                self.recover_consumed_runtime()?;
                self.settle_interrupted_bundled_package_build_under_gate()
                    .map(|outcome| outcome == BundledPackageBuildSettlementOutcome::Completed)
                    .map_err(BundledPackageMaterializationError::InterruptedBuildSettlement)
            }
        }
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

#[cfg(feature = "acquired-packages")]
fn map_acquired_preparation_error(error: PreparationError) -> AcquiredPackageMaterializationError {
    match error {
        PreparationError::CatalogLengthMismatch | PreparationError::CatalogDigestMismatch => {
            AcquiredPackageMaterializationError::CatalogBytesMismatch
        }
        PreparationError::PackageMissing => AcquiredPackageMaterializationError::PackageMissing,
        PreparationError::UnsupportedPayload => {
            AcquiredPackageMaterializationError::UnsupportedPayload
        }
        PreparationError::UnsupportedRuntimeTarget => {
            AcquiredPackageMaterializationError::UnsupportedRuntimeTarget
        }
        PreparationError::ManifestAuthority(error) => {
            AcquiredPackageMaterializationError::ManifestAuthority(error)
        }
        PreparationError::ManifestAdmission(error) => {
            AcquiredPackageMaterializationError::ManifestAdmission(error)
        }
        PreparationError::Source(_) => AcquiredPackageMaterializationError::PreparationInvariant,
        PreparationError::Read => AcquiredPackageMaterializationError::ResourceRead,
        PreparationError::ResourceLengthMismatch => {
            AcquiredPackageMaterializationError::ResourceLengthMismatch
        }
        PreparationError::ResourceDigestMismatch => {
            AcquiredPackageMaterializationError::ResourceDigestMismatch
        }
        PreparationError::TreeIndex(error) => AcquiredPackageMaterializationError::TreeIndex(error),
        PreparationError::TreeIndexBinding => AcquiredPackageMaterializationError::TreeIndexBinding,
        PreparationError::AccountingOverflow => {
            AcquiredPackageMaterializationError::AccountingOverflow
        }
        PreparationError::InvalidRecord => {
            AcquiredPackageMaterializationError::PreparationInvariant
        }
    }
}

#[cfg(feature = "acquired-packages")]
fn map_acquired_tree_error(error: TreeWriterError) -> AcquiredPackageMaterializationError {
    match error {
        TreeWriterError::AcquiredArchive(_)
        | TreeWriterError::AcquiredTree(_)
        | TreeWriterError::ExactMismatch => {
            AcquiredPackageMaterializationError::ResourceIntegrityMismatch
        }
        TreeWriterError::Source(_) => AcquiredPackageMaterializationError::PreparationInvariant,
        TreeWriterError::Filesystem(error) => AcquiredPackageMaterializationError::Repository(
            ExtensionRepositoryError::FileSystem(error),
        ),
        TreeWriterError::TransitionAmbiguous => AcquiredPackageMaterializationError::Repository(
            ExtensionRepositoryError::SettlementAmbiguous,
        ),
    }
}

#[cfg(feature = "acquired-packages")]
fn map_acquired_object_error(error: PackageObjectError) -> AcquiredPackageMaterializationError {
    match error {
        PackageObjectError::CapacityExhausted => {
            AcquiredPackageMaterializationError::CapacityExhausted
        }
        PackageObjectError::GenerationExhausted => {
            ExtensionRepositoryError::GenerationExhausted.into()
        }
        PackageObjectError::AcquiredLegalSource(error) => {
            AcquiredPackageMaterializationError::LegalSource(error)
        }
        PackageObjectError::Source(_) => AcquiredPackageMaterializationError::PreparationInvariant,
        PackageObjectError::Filesystem(error) => ExtensionRepositoryError::FileSystem(error).into(),
        PackageObjectError::SettlementAmbiguous => {
            ExtensionRepositoryError::SettlementAmbiguous.into()
        }
        PackageObjectError::BuildStateMismatch => {
            ExtensionRepositoryError::RecoveryAmbiguous.into()
        }
        PackageObjectError::Collision | PackageObjectError::ExactMismatch => {
            AcquiredPackageMaterializationError::DurableObjectMismatch
        }
    }
}

#[cfg(feature = "acquired-packages")]
fn map_acquired_publication_error(
    error: PackageObjectError,
) -> AcquiredPackageMaterializationError {
    match error {
        PackageObjectError::ExactMismatch => {
            AcquiredPackageMaterializationError::ResourceIntegrityMismatch
        }
        other => map_acquired_object_error(other),
    }
}

#[cfg(feature = "acquired-packages")]
fn map_bundled_to_acquired_error(
    error: BundledPackageMaterializationError,
) -> AcquiredPackageMaterializationError {
    match error {
        BundledPackageMaterializationError::Repository(error) => error.into(),
        BundledPackageMaterializationError::ManifestAuthority(error) => {
            AcquiredPackageMaterializationError::ManifestAuthority(error)
        }
        BundledPackageMaterializationError::ManifestAdmission(error) => {
            AcquiredPackageMaterializationError::ManifestAdmission(error)
        }
        BundledPackageMaterializationError::CatalogBytesMismatch => {
            AcquiredPackageMaterializationError::CatalogBytesMismatch
        }
        BundledPackageMaterializationError::PackageMissing => {
            AcquiredPackageMaterializationError::PackageMissing
        }
        BundledPackageMaterializationError::UnsupportedPayload => {
            AcquiredPackageMaterializationError::UnsupportedPayload
        }
        BundledPackageMaterializationError::UnsupportedRuntimeTarget => {
            AcquiredPackageMaterializationError::UnsupportedRuntimeTarget
        }
        BundledPackageMaterializationError::Source(_) => {
            AcquiredPackageMaterializationError::PreparationInvariant
        }
        BundledPackageMaterializationError::ResourceRead => {
            AcquiredPackageMaterializationError::ResourceRead
        }
        BundledPackageMaterializationError::ResourceLengthMismatch => {
            AcquiredPackageMaterializationError::ResourceLengthMismatch
        }
        BundledPackageMaterializationError::ResourceDigestMismatch => {
            AcquiredPackageMaterializationError::ResourceDigestMismatch
        }
        BundledPackageMaterializationError::ResourceIntegrityMismatch => {
            AcquiredPackageMaterializationError::ResourceIntegrityMismatch
        }
        BundledPackageMaterializationError::TreeIndex(error) => {
            AcquiredPackageMaterializationError::TreeIndex(error)
        }
        BundledPackageMaterializationError::TreeIndexBinding => {
            AcquiredPackageMaterializationError::TreeIndexBinding
        }
        BundledPackageMaterializationError::CapacityExhausted => {
            AcquiredPackageMaterializationError::CapacityExhausted
        }
        BundledPackageMaterializationError::AccountingOverflow => {
            AcquiredPackageMaterializationError::AccountingOverflow
        }
        BundledPackageMaterializationError::PreparationInvariant => {
            AcquiredPackageMaterializationError::PreparationInvariant
        }
        BundledPackageMaterializationError::DurableObjectMismatch => {
            AcquiredPackageMaterializationError::DurableObjectMismatch
        }
        BundledPackageMaterializationError::InterruptedBuildSettlement(error) => {
            AcquiredPackageMaterializationError::InterruptedBuildSettlement(error)
        }
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
        #[cfg(feature = "acquired-packages")]
        PackageObjectError::AcquiredLegalSource(_) => {
            BundledPackageMaterializationError::DurableObjectMismatch
        }
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
        CleanupError::BuildStateMismatch
        | CleanupError::ExactMismatch
        | CleanupError::CommitMarkerPresent => ExtensionRepositoryError::RecoveryAmbiguous.into(),
    }
}

pub(crate) const fn preflight_error_requires_sealing(
    error: PackageObjectError,
    had_intent: bool,
) -> bool {
    match error {
        PackageObjectError::BuildStateMismatch
        | PackageObjectError::Collision
        | PackageObjectError::ExactMismatch
        | PackageObjectError::SettlementAmbiguous => true,
        PackageObjectError::GenerationExhausted => had_intent,
        PackageObjectError::Filesystem(error) => filesystem_error_requires_sealing(error),
        PackageObjectError::CapacityExhausted | PackageObjectError::Source(_) => false,
        #[cfg(feature = "acquired-packages")]
        PackageObjectError::AcquiredLegalSource(_) => false,
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
        #[cfg(feature = "acquired-packages")]
        PackageObjectError::AcquiredLegalSource(_) => false,
    }
}

pub(crate) const fn completed_error_requires_sealing(error: PackageObjectError) -> bool {
    match error {
        PackageObjectError::Filesystem(error) => filesystem_error_requires_sealing(error),
        PackageObjectError::Source(_) => false,
        #[cfg(feature = "acquired-packages")]
        PackageObjectError::AcquiredLegalSource(_) => false,
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
        | CleanupError::CommitMarkerPresent
        | CleanupError::SettlementAmbiguous => true,
        CleanupError::Filesystem(error) => filesystem_error_requires_sealing(error),
    }
}

pub(crate) const fn filesystem_error_requires_sealing(error: PrivateFsError) -> bool {
    match error {
        PrivateFsError::NotFound
        | PrivateFsError::ReservedComponent
        | PrivateFsError::Unsafe
        | PrivateFsError::BoundExceeded
        | PrivateFsError::AlreadyExists
        | PrivateFsError::NamespaceMismatch
        | PrivateFsError::DirectoryNotEmpty
        | PrivateFsError::IdentityAmbiguous
        | PrivateFsError::SettlementUnknown
        | PrivateFsError::Quarantined => true,
        PrivateFsError::LockUnavailable
        | PrivateFsError::InUse
        | PrivateFsError::PrimitiveUnavailable
        | PrivateFsError::Io => false,
    }
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
#[path = "writer/tests.rs"]
mod repository_e2e_tests;

#[cfg(all(
    test,
    feature = "acquired-packages",
    zephium_internal_repository_e2e,
    zephium_internal_acquired_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
#[path = "writer/acquired_tests.rs"]
mod acquired_repository_e2e_tests;
