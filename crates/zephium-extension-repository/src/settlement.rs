//! Public source-free settlement for interrupted bundled package builds.
//!
//! Settlement is deliberately independent of package byte adapters. An
//! incomplete build may be aborted only while a freshly revalidated physical
//! package-record marker is absent. Once that marker exists, the exact active
//! or rollback package must be completed from authenticated repository objects
//! or remain durably pending.

use thiserror::Error;
use zephium_extension_authority::{
    BundledCatalogAdmissionError, ProductExtensionManifestAdmissionError,
    ProductExtensionManifestAuthorityError,
};
use zephium_extension_package::ExtensionTreeIndexError;
use zephium_private_fs::PrivateFsError;

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
use crate::materialization::publish_intent_package_record_marker_for_e2e;
use crate::materialization::{
    abort_package_build, authenticate_interrupted_package, complete_active_package,
    complete_rollback_package, inspect_package_build_commit_marker,
    reconcile_build_stages_for_abort, CleanupError, InterruptedPackageAuthenticationError,
    MaterializationTransitionError, PackageBuildCommitMarker, PackageObjectError, PreparationError,
    VerifiedInterruptedPackageClosure,
};
use crate::{ExtensionRepository, ExtensionRepositoryError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InterruptedBuildSettlementFaultPoint {
    None,
    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    PublishMarkerAfterFirstAbsenceObservation,
}

/// Durable result of settling the repository's one possible package build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[must_use = "interrupted build settlement changes whether fresh package work may begin"]
pub enum BundledPackageBuildSettlementOutcome {
    /// No durable package build existed.
    NoBuild,
    /// A package-record marker existed and its fully reauthenticated closure
    /// was durably added to the completed ledger.
    Completed,
    /// No package-record marker existed, so exact stage cleanup and durable
    /// intent abort completed.
    AbortedIncomplete,
}

/// Stable, path-free failure while settling one interrupted package build.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BundledPackageBuildSettlementError {
    /// Durable repository recovery, I/O, or transition settlement failed.
    #[error("extension package build repository settlement failed: {0}")]
    Repository(#[from] ExtensionRepositoryError),
    /// Product-sealed bundled catalog authority is unavailable or invalid.
    #[error("extension package build catalog authority is unavailable: {0}")]
    CatalogAuthority(#[source] BundledCatalogAdmissionError),
    /// Exact repository-owned catalog bytes are not admitted by product policy.
    #[error("extension package build catalog admission failed: {0}")]
    CatalogAdmission(#[source] BundledCatalogAdmissionError),
    /// Product-sealed manifest compatibility authority is unavailable or
    /// invalid.
    #[error("extension package build manifest authority is unavailable: {0}")]
    ManifestAuthority(#[source] ProductExtensionManifestAuthorityError),
    /// Exact repository-owned manifest bytes are not admitted by product
    /// compatibility policy.
    #[error("extension package build manifest admission failed: {0}")]
    ManifestAdmission(#[source] ProductExtensionManifestAdmissionError),
    /// The stored canonical tree index failed semantic validation.
    #[error("extension package build tree index is invalid: {0}")]
    TreeIndex(#[source] ExtensionTreeIndexError),
    /// The authenticated closure exceeds a fixed materialization capacity.
    #[error("extension package build materialization capacity is exhausted")]
    CapacityExhausted,
    /// Exact reconstruction accounting overflowed a fixed bound.
    #[error("extension package build reconstruction accounting overflowed")]
    AccountingOverflow,
    /// A durable marker, catalog, manifest, index, legal object, tree, or
    /// package-record binding was missing or different.
    #[error("extension package build durable object closure is not exact")]
    DurableObjectMismatch,
}

impl ExtensionRepository {
    /// Settles the repository's one possible interrupted bundled package build.
    ///
    /// This API accepts no package source and invokes no external callback. It
    /// must run before any new materialization request consults its source. A
    /// missing package-record commit marker permits exact stage cleanup and
    /// abort. A present marker permanently forbids abort and instead requires
    /// fresh product admission plus complete durable-object authentication.
    pub fn settle_interrupted_bundled_package_build(
        &mut self,
    ) -> Result<BundledPackageBuildSettlementOutcome, BundledPackageBuildSettlementError> {
        let runtime = self.runtime.clone();
        let _operation = runtime.enter().map_err(|error| error.repository_error())?;
        self.settle_interrupted_bundled_package_build_under_gate()
    }

    pub(crate) fn settle_interrupted_bundled_package_build_under_gate(
        &mut self,
    ) -> Result<BundledPackageBuildSettlementOutcome, BundledPackageBuildSettlementError> {
        self.settle_interrupted_bundled_package_build_under_gate_with_fault(
            InterruptedBuildSettlementFaultPoint::None,
        )
    }

    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    pub(crate) fn settle_interrupted_bundled_package_build_at_fault(
        &mut self,
        fault: InterruptedBuildSettlementFaultPoint,
    ) -> Result<BundledPackageBuildSettlementOutcome, BundledPackageBuildSettlementError> {
        let runtime = self.runtime.clone();
        let _operation = runtime.enter().map_err(|error| error.repository_error())?;
        self.settle_interrupted_bundled_package_build_under_gate_with_fault(fault)
    }

    fn settle_interrupted_bundled_package_build_under_gate_with_fault(
        &mut self,
        fault: InterruptedBuildSettlementFaultPoint,
    ) -> Result<BundledPackageBuildSettlementOutcome, BundledPackageBuildSettlementError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed.into());
        }
        self.writer_require_gc_idle()?;

        loop {
            let marker = {
                let runtime = self.writer_materialization()?;
                if runtime._build_intent.is_none() {
                    return Ok(BundledPackageBuildSettlementOutcome::NoBuild);
                }
                inspect_package_build_commit_marker(runtime)
            };
            let marker = match marker {
                Ok(marker) => marker,
                Err(error) => return Err(self.finish_settlement_cleanup_observation(error)),
            };

            match marker {
                PackageBuildCommitMarker::Absent(marker_absent) => {
                    self.publish_marker_for_settlement_fault(fault)?;
                    let mut runtime = self.writer_take_materialization()?;
                    let abortable =
                        match reconcile_build_stages_for_abort(&mut runtime, marker_absent) {
                            Ok(abortable) => abortable,
                            Err(CleanupError::CommitMarkerPresent) => {
                                // A marker that appeared after the first observation
                                // may never be raced by abort. Reopen the complete
                                // physical frontier and route through completion.
                                drop(runtime);
                                self.recover_settlement_runtime()?;
                                continue;
                            }
                            Err(error) => {
                                drop(runtime);
                                return Err(self.finish_consumed_settlement_cleanup(error));
                            }
                        };
                    self.finish_settlement_transition(abort_package_build(runtime, abortable))?;
                    return Ok(BundledPackageBuildSettlementOutcome::AbortedIncomplete);
                }
                PackageBuildCommitMarker::Present => {
                    let catalog_digest = self
                        .writer_materialization()?
                        ._build_intent
                        .as_ref()
                        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?
                        .package_record
                        .catalog
                        .catalog_sha256;
                    let catalog_bytes = self.read_authenticated_catalog_object(catalog_digest)?;
                    let closure = authenticate_interrupted_package(
                        self.writer_materialization()?,
                        &catalog_bytes,
                    );
                    let closure = match closure {
                        Ok(closure) => closure,
                        Err(error) => {
                            let seal = authentication_error_requires_sealing(&error);
                            let public = map_authentication_error(error);
                            if seal {
                                self.writer_seal();
                            }
                            return Err(public);
                        }
                    };
                    let runtime = self.writer_take_materialization()?;
                    let transition = match closure {
                        VerifiedInterruptedPackageClosure::Active(closure) => {
                            complete_active_package(runtime, closure)
                        }
                        VerifiedInterruptedPackageClosure::Rollback(closure) => {
                            complete_rollback_package(runtime, closure)
                        }
                    };
                    self.finish_settlement_transition(transition)?;
                    return Ok(BundledPackageBuildSettlementOutcome::Completed);
                }
            }
        }
    }

    fn publish_marker_for_settlement_fault(
        &mut self,
        fault: InterruptedBuildSettlementFaultPoint,
    ) -> Result<(), BundledPackageBuildSettlementError> {
        #[cfg(all(
            test,
            zephium_internal_repository_e2e,
            any(target_os = "macos", target_os = "linux")
        ))]
        if fault == InterruptedBuildSettlementFaultPoint::PublishMarkerAfterFirstAbsenceObservation
        {
            let result =
                publish_intent_package_record_marker_for_e2e(self.writer_materialization()?);
            if let Err(error) = result {
                let classified = InterruptedPackageAuthenticationError::Object(error);
                let seal = authentication_error_requires_sealing(&classified);
                let public = map_authentication_error(classified);
                if seal {
                    self.writer_seal();
                }
                return Err(public);
            }
        }
        #[cfg(not(all(
            test,
            zephium_internal_repository_e2e,
            any(target_os = "macos", target_os = "linux")
        )))]
        let _ = fault;
        Ok(())
    }

    fn finish_settlement_transition<Committed>(
        &mut self,
        transition: Result<Committed, MaterializationTransitionError>,
    ) -> Result<(), BundledPackageBuildSettlementError> {
        match transition {
            Ok(_committed) => self.recover_settlement_runtime(),
            Err(MaterializationTransitionError::Clean(error)) => {
                self.recover_settlement_runtime()?;
                Err(error.into())
            }
            Err(MaterializationTransitionError::MustSeal(error)) => {
                self.writer_seal();
                Err(error.into())
            }
        }
    }

    fn recover_settlement_runtime(&mut self) -> Result<(), BundledPackageBuildSettlementError> {
        match self.writer_recover_materialization() {
            Ok(()) => Ok(()),
            Err(error) => {
                self.writer_seal();
                Err(error.into())
            }
        }
    }

    fn finish_settlement_cleanup_observation(
        &mut self,
        error: CleanupError,
    ) -> BundledPackageBuildSettlementError {
        if cleanup_error_requires_sealing(error) {
            self.writer_seal();
        }
        map_cleanup_error(error)
    }

    fn finish_consumed_settlement_cleanup(
        &mut self,
        error: CleanupError,
    ) -> BundledPackageBuildSettlementError {
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
}

fn map_authentication_error(
    error: InterruptedPackageAuthenticationError,
) -> BundledPackageBuildSettlementError {
    match error {
        InterruptedPackageAuthenticationError::CatalogAuthority(error) => {
            BundledPackageBuildSettlementError::CatalogAuthority(error)
        }
        InterruptedPackageAuthenticationError::CatalogAdmission(error) => {
            BundledPackageBuildSettlementError::CatalogAdmission(error)
        }
        InterruptedPackageAuthenticationError::Repository(error) => error.into(),
        InterruptedPackageAuthenticationError::Preparation(error) => map_preparation_error(error),
        InterruptedPackageAuthenticationError::Object(error) => map_object_error(error),
        InterruptedPackageAuthenticationError::DurableMismatch => {
            BundledPackageBuildSettlementError::DurableObjectMismatch
        }
    }
}

fn map_preparation_error(error: PreparationError) -> BundledPackageBuildSettlementError {
    match error {
        PreparationError::ManifestAuthority(error) => {
            BundledPackageBuildSettlementError::ManifestAuthority(error)
        }
        PreparationError::ManifestAdmission(error) => {
            BundledPackageBuildSettlementError::ManifestAdmission(error)
        }
        PreparationError::TreeIndex(error) => BundledPackageBuildSettlementError::TreeIndex(error),
        PreparationError::AccountingOverflow => {
            BundledPackageBuildSettlementError::AccountingOverflow
        }
        PreparationError::CatalogLengthMismatch
        | PreparationError::CatalogDigestMismatch
        | PreparationError::PackageMissing
        | PreparationError::UnsupportedPayload
        | PreparationError::UnsupportedRuntimeTarget
        | PreparationError::Source(_)
        | PreparationError::Read
        | PreparationError::ResourceLengthMismatch
        | PreparationError::ResourceDigestMismatch
        | PreparationError::TreeIndexBinding
        | PreparationError::InvalidRecord => {
            BundledPackageBuildSettlementError::DurableObjectMismatch
        }
    }
}

fn map_object_error(error: PackageObjectError) -> BundledPackageBuildSettlementError {
    match error {
        PackageObjectError::CapacityExhausted => {
            BundledPackageBuildSettlementError::CapacityExhausted
        }
        PackageObjectError::GenerationExhausted => {
            ExtensionRepositoryError::GenerationExhausted.into()
        }
        PackageObjectError::Filesystem(error) => ExtensionRepositoryError::FileSystem(error).into(),
        PackageObjectError::SettlementAmbiguous => {
            ExtensionRepositoryError::SettlementAmbiguous.into()
        }
        PackageObjectError::BuildStateMismatch => {
            ExtensionRepositoryError::RecoveryAmbiguous.into()
        }
        PackageObjectError::Collision
        | PackageObjectError::ExactMismatch
        | PackageObjectError::Source(_) => {
            BundledPackageBuildSettlementError::DurableObjectMismatch
        }
    }
}

fn map_cleanup_error(error: CleanupError) -> BundledPackageBuildSettlementError {
    match error {
        CleanupError::Filesystem(error) => ExtensionRepositoryError::FileSystem(error).into(),
        CleanupError::SettlementAmbiguous => ExtensionRepositoryError::SettlementAmbiguous.into(),
        CleanupError::BuildStateMismatch
        | CleanupError::ExactMismatch
        | CleanupError::CommitMarkerPresent => ExtensionRepositoryError::RecoveryAmbiguous.into(),
    }
}

fn authentication_error_requires_sealing(error: &InterruptedPackageAuthenticationError) -> bool {
    match error {
        InterruptedPackageAuthenticationError::Repository(
            ExtensionRepositoryError::FileSystem(error),
        ) => filesystem_error_requires_sealing(*error),
        InterruptedPackageAuthenticationError::Object(PackageObjectError::Filesystem(error)) => {
            filesystem_error_requires_sealing(*error)
        }
        // Source-free authentication cannot construct this variant. Treat any
        // future violation of that boundary as durable incoherence, never as a
        // retryable adapter failure.
        InterruptedPackageAuthenticationError::Object(PackageObjectError::Source(_)) => true,
        InterruptedPackageAuthenticationError::CatalogAuthority(_)
        | InterruptedPackageAuthenticationError::CatalogAdmission(_)
        | InterruptedPackageAuthenticationError::Repository(_)
        | InterruptedPackageAuthenticationError::Preparation(_)
        | InterruptedPackageAuthenticationError::Object(_)
        | InterruptedPackageAuthenticationError::DurableMismatch => true,
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

const fn filesystem_error_requires_sealing(error: PrivateFsError) -> bool {
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
