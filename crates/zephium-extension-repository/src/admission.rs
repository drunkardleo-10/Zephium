//! Monotonic authenticated-catalog admission and durable transitions.

use std::cmp::Ordering;
use std::collections::BTreeSet;
#[cfg(feature = "acquired-packages")]
use zephium_extension_authority::AdmittedAcquiredCatalog;
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackCatalog, BundledCatalogCheckpoint,
};
use zephium_extension_package::{ExtensionReleaseCatalog, MAX_EXTENSION_RELEASE_CATALOG_BYTES};
use zephium_private_fs::{FileIdentity, LockedPrivateNamespace, PrivateDirectory, PrivateFsError};

use crate::catalog_cache::ProductCatalogAdmissionCache;
use crate::codec;
use crate::materialization::{self, MaterializationRuntime};
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
use crate::materialization::{
    MaterializationGarbageCollectionIntent, MAX_MATERIALIZATION_STATE_BYTES,
};
use crate::names::{catalog_file, checkpoint_file, checkpoint_stage, state_file, state_stage};
use crate::operation::{reject_if_external_callback, RepositoryOperationGuard, RepositoryRuntime};
use crate::package_lease::PackageLeaseRuntime;
use crate::recovery::open_repository;
use crate::state::{
    validate_catalog_lines, Digest32, PackageLineHighWater, RecoveryCheckpoint, RepositoryState,
    StoredCatalogCheckpoint, TransitionJournal, JOURNAL_SCHEMA_VERSION, MAX_CHECKPOINT_BYTES,
    MAX_JOURNAL_BYTES, MAX_PACKAGE_LINE_HIGH_WATERS, MAX_STATE_BYTES,
};
use crate::storage::{
    atomic_write_control, ensure_catalog_object, forget_catalog_object_identity,
    insert_catalog_object_identity, parse_catalog_canonical, publish_journal, read_required,
    validate_catalog_object_inventory, write_checkpoint, CatalogObjectIdentities,
};
use crate::ExtensionRepositoryError;

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
use zephium_private_fs::PrivateComponent;

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
use crate::names::{self, catalog_stage, journal_file, journal_stage};
/// Result of durably recording one authenticated bundled catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "catalog replay and durable advancement have different effects"]
pub enum BundledCatalogRecordOutcome {
    /// The strictly newer catalog and package-line floors became durable.
    Recorded,
    /// Durable state already names this exact catalog and complete package rows.
    IdempotentReplay,
}

/// Exclusive crash-durable extension catalog authority repository.
///
/// This value owns the private namespace lock. Its internal materialization
/// metadata and sealed roots remain non-public and cannot issue a package,
/// profile, receipt, or native-runtime lease. A post-commit ambiguity seals
/// the instance until a fresh [`Self::open`] performs exact journal recovery.
pub struct ExtensionRepository {
    _namespace: LockedPrivateNamespace,
    catalogs: PrivateDirectory,
    journals: PrivateDirectory,
    catalog_object_ids: BTreeSet<Digest32>,
    catalog_object_identities: CatalogObjectIdentities,
    catalog_admission_cache: ProductCatalogAdmissionCache,
    state: RepositoryState,
    state_bytes: Vec<u8>,
    materialization: Option<MaterializationRuntime>,
    pub(crate) runtime: RepositoryRuntime,
    pub(crate) package_leases: PackageLeaseRuntime,
    sealed: bool,
}

impl ExtensionRepository {
    /// Opens an already-admitted private namespace and recovers one exact state.
    ///
    /// Unknown entries, corrupt objects, a forked or gapped journal chain, or
    /// disagreement between state and checkpoint fail closed. The caller must
    /// create the namespace at its product-owned application-data location.
    pub fn open(namespace: LockedPrivateNamespace) -> Result<Self, ExtensionRepositoryError> {
        reject_if_external_callback().map_err(|error| error.repository_error())?;
        let opened = open_repository(namespace)?;
        let mut repository = Self {
            _namespace: opened.namespace,
            catalogs: opened.catalogs,
            journals: opened.journals,
            catalog_object_ids: opened.catalog_object_ids,
            catalog_object_identities: opened.catalog_object_identities,
            catalog_admission_cache: opened.catalog_admission_cache,
            state: opened.state,
            state_bytes: opened.state_bytes,
            materialization: Some(opened.materialization),
            runtime: RepositoryRuntime::new(),
            package_leases: PackageLeaseRuntime::new(),
            sealed: false,
        };
        // Opening settles only an intent that was already durable. Avoid a
        // second catalog inventory and parse pass on the ordinary no-GC
        // startup path; fresh collection remains an explicit, schedulable
        // operation so startup cannot acquire unbounded maintenance latency.
        if repository
            .materialization
            .as_ref()
            .is_some_and(|runtime| runtime._gc_intent.is_some())
        {
            repository.settle_pending_garbage_collection()?;
        }
        Ok(repository)
    }

    /// Records an exact authenticated bundled catalog as the monotonic floor.
    ///
    /// `exact_catalog_bytes` are rehashed, reparsed canonically, and compared
    /// with the non-forgeable authority witness before any durable mutation.
    /// Success neither materializes a package nor authorizes activation.
    pub fn record_bundled_catalog(
        &mut self,
        admitted: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<BundledCatalogRecordOutcome, ExtensionRepositoryError> {
        let runtime = self.runtime.clone();
        let operation = runtime.enter().map_err(|error| error.repository_error())?;
        self.record_bundled_catalog_under_gate(&operation, admitted, exact_catalog_bytes)
    }

    /// Records one product-authenticated catalog whose package payloads are
    /// acquired CRX3 archives. Metadata follows the same monotonic authority
    /// floor as bundled catalogs; this operation acquires no package bytes.
    #[cfg(feature = "acquired-packages")]
    pub fn record_acquired_catalog(
        &mut self,
        admitted: &AdmittedAcquiredCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<BundledCatalogRecordOutcome, ExtensionRepositoryError> {
        let runtime = self.runtime.clone();
        let operation = runtime.enter().map_err(|error| error.repository_error())?;
        self.record_acquired_catalog_under_gate(&operation, admitted, exact_catalog_bytes)
    }

    #[cfg(feature = "acquired-packages")]
    pub(crate) fn record_acquired_catalog_under_gate(
        &mut self,
        _operation: &RepositoryOperationGuard<'_>,
        admitted: &AdmittedAcquiredCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<BundledCatalogRecordOutcome, ExtensionRepositoryError> {
        let outcome = self.record_view(admitted, exact_catalog_bytes, FaultPoint::None)?;
        self.writer_seed_active_acquired_catalog(admitted)?;
        Ok(outcome)
    }

    pub(crate) fn record_bundled_catalog_under_gate(
        &mut self,
        _operation: &RepositoryOperationGuard<'_>,
        admitted: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<BundledCatalogRecordOutcome, ExtensionRepositoryError> {
        let outcome = self.record_view(admitted, exact_catalog_bytes, FaultPoint::None)?;
        self.writer_seed_active_catalog(admitted)?;
        Ok(outcome)
    }

    pub(crate) fn writer_is_sealed(&self) -> bool {
        self.sealed || !self.runtime.is_healthy()
    }

    pub(crate) fn writer_catalogs(&self) -> &PrivateDirectory {
        &self.catalogs
    }

    pub(crate) fn writer_catalog_object_ids(&self) -> &BTreeSet<Digest32> {
        &self.catalog_object_ids
    }

    pub(crate) fn writer_validate_catalog_object_inventory(
        &self,
    ) -> Result<(), ExtensionRepositoryError> {
        validate_catalog_object_inventory(
            &self.catalogs,
            &self.catalog_object_ids,
            &self.catalog_object_identities,
        )
    }

    pub(crate) fn writer_forget_deleted_catalog_object(
        &mut self,
        digest: Digest32,
    ) -> Result<(), ExtensionRepositoryError> {
        if self
            .catalogs
            .regular_identity(&catalog_file(digest))?
            .is_some()
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        forget_catalog_object_identity(
            &mut self.catalog_object_ids,
            &mut self.catalog_object_identities,
            digest,
        )?;
        if let Err(error) = self.catalog_admission_cache.forget_digest(digest) {
            return Err(self.prejournal_error(error));
        }
        Ok(())
    }

    pub(crate) fn writer_catalog_high_water(&self) -> Option<&StoredCatalogCheckpoint> {
        self.state.checkpoint()
    }

    /// Fresh proof that the outer monotonic controls have no pending journal
    /// and still equal the live in-memory projection.
    ///
    /// Catalog bytes are fully authenticated by open/admission. Callers that
    /// are about to mutate the catalog namespace must separately join this to
    /// [`Self::writer_validate_catalog_object_inventory`], which rechecks the
    /// complete bounded content-addressed inventory without reparsing it.
    pub(crate) fn writer_validate_outer_controls(&self) -> Result<(), ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        self.state.validate()?;
        if self
            .state
            .checkpoint()
            .is_some_and(|checkpoint| !self.catalog_object_ids.contains(&checkpoint.catalog_sha256))
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let canonical = codec::encode(&self.state, MAX_STATE_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
        if canonical != self.state_bytes
            || read_required(self._namespace.directory(), &state_file(), MAX_STATE_BYTES)?
                != canonical
            || self._namespace.directory().regular_exists(&state_stage())?
            || self
                ._namespace
                .directory()
                .regular_exists(&checkpoint_stage())?
            || !self
                .journals
                .list_components(crate::names::MAX_JOURNAL_ENTRIES)
                .map_err(crate::storage::map_recovery_fs)?
                .is_empty()
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let checkpoint_bytes = read_required(
            self._namespace.directory(),
            &checkpoint_file(),
            MAX_CHECKPOINT_BYTES,
        )?;
        let checkpoint: RecoveryCheckpoint = codec::decode(&checkpoint_bytes, MAX_CHECKPOINT_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
        if checkpoint != RecoveryCheckpoint::new(self.state.generation, codec::digest(&canonical)) {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        Ok(())
    }

    pub(crate) fn writer_materialization(
        &mut self,
    ) -> Result<&MaterializationRuntime, ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        if self.materialization.is_none() {
            self.writer_recover_materialization()?;
        }
        self.materialization
            .as_ref()
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
    }

    pub(crate) fn writer_require_gc_idle(&mut self) -> Result<(), ExtensionRepositoryError> {
        let validation = self.writer_materialization()?.validate_gc_idle_projection();
        match validation {
            Ok(()) => Ok(()),
            Err(ExtensionRepositoryError::GarbageCollectionInProgress) => {
                Err(ExtensionRepositoryError::GarbageCollectionInProgress)
            }
            Err(error) => Err(self.prejournal_error(error)),
        }
    }

    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    pub(crate) fn writer_install_gc_projection_for_e2e(
        &mut self,
        intent: MaterializationGarbageCollectionIntent,
    ) -> Result<(), ExtensionRepositoryError> {
        let mut state = self.writer_materialization()?._state.clone();
        state.generation = intent.generation;
        state.gc_intent = Some(intent.clone());
        state.validate()?;
        let state_bytes = codec::encode(&state, MAX_MATERIALIZATION_STATE_BYTES)
            .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
        let mut runtime = self.writer_take_materialization()?;
        runtime._state = state;
        runtime._state_bytes = state_bytes;
        runtime._gc_intent = Some(intent);
        self.materialization = Some(runtime);
        Ok(())
    }

    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    pub(crate) fn writer_break_gc_projection_for_e2e(&mut self) {
        self.materialization.as_mut().unwrap()._gc_intent = None;
    }

    pub(crate) fn writer_take_materialization(
        &mut self,
    ) -> Result<MaterializationRuntime, ExtensionRepositoryError> {
        let _ = self.writer_materialization()?;
        self.materialization
            .take()
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
    }

    pub(crate) fn writer_recover_materialization(
        &mut self,
    ) -> Result<(), ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        self.materialization = None;
        let high_water = self.state.checkpoint().cloned();
        let catalog_recovery = materialization::ProductCatalogRecovery::new(
            &self.catalogs,
            &self.catalog_object_ids,
            &self.catalog_object_identities,
            &mut self.catalog_admission_cache,
            high_water.as_ref(),
        );
        match materialization::open_or_recover(
            self._namespace.directory(),
            catalog_recovery,
            true,
            materialization::FaultPoint::None,
        ) {
            Ok(runtime) => {
                self.materialization = Some(runtime);
                Ok(())
            }
            Err(error) => Err(self.prejournal_error(error)),
        }
    }

    pub(crate) fn writer_seal(&mut self) {
        self.materialization = None;
        self.catalog_admission_cache.clear();
        self.sealed = true;
        self.runtime.poison();
    }

    /// Freshly reads one digest-addressed authenticated catalog object.
    ///
    /// This neutral repository primitive is shared by package leases and
    /// interrupted-build settlement; neither caller may treat bytes as product
    /// authority until it independently re-admits them.
    pub(crate) fn read_authenticated_catalog_object(
        &mut self,
        digest: Digest32,
    ) -> Result<Vec<u8>, ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        let Some(expected_identity) = self.catalog_object_identities.get(&digest).copied() else {
            return Err(self.prejournal_error(ExtensionRepositoryError::StateCorrupt));
        };
        let name = catalog_file(digest);
        let observed_identity = self
            .catalogs
            .regular_identity(&name)
            .map_err(|error| self.prejournal_error(error.into()))?;
        if observed_identity != Some(expected_identity) {
            return Err(self.prejournal_error(ExtensionRepositoryError::StateCorrupt));
        }
        let bytes = read_required(&self.catalogs, &name, MAX_EXTENSION_RELEASE_CATALOG_BYTES)
            .map_err(|error| self.required_catalog_object_error(error))?;
        let observed_identity = self
            .catalogs
            .regular_identity(&name)
            .map_err(|error| self.prejournal_error(error.into()))?;
        if codec::digest(&bytes) != digest || observed_identity != Some(expected_identity) {
            return Err(self.prejournal_error(ExtensionRepositoryError::StateCorrupt));
        }
        Ok(bytes)
    }

    fn required_catalog_object_error(
        &mut self,
        error: ExtensionRepositoryError,
    ) -> ExtensionRepositoryError {
        if matches!(
            error,
            ExtensionRepositoryError::RecoveryAmbiguous
                | ExtensionRepositoryError::FileSystem(PrivateFsError::NotFound)
        ) {
            // A verified current catalog-set record names this object exactly.
            // Absence, including an exists/open race, is durable corruption.
            return self.prejournal_error(ExtensionRepositoryError::StateCorrupt);
        }
        self.prejournal_error(error)
    }

    pub(crate) fn writer_ensure_rollback_catalog(
        &mut self,
        admitted: &AdmittedRollbackCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<(), ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        self.writer_require_gc_idle()?;
        validate_exact_rollback_catalog(admitted, exact_catalog_bytes)?;
        if let Err(error) = self.state.validate() {
            return Err(self.prejournal_error(error));
        }
        let current = self
            .state
            .checkpoint()
            .ok_or(ExtensionRepositoryError::StateCorrupt)?;
        if current.authority_id != Digest32::from_bytes(admitted.authority().bytes()) {
            return Err(ExtensionRepositoryError::AuthorityMismatch);
        }
        if admitted.revision().get() > current.revision {
            return Err(ExtensionRepositoryError::RollbackCatalogAboveHighWater);
        }
        if admitted.revision().get() == current.revision
            && (Digest32::from_bytes(admitted.catalog_digest().bytes()) != current.catalog_sha256
                || Digest32::from_bytes(admitted.inventory_digest().bytes())
                    != current.inventory_sha256
                || admitted.catalog_length() != current.catalog_length)
        {
            return Err(ExtensionRepositoryError::CatalogEquivocation);
        }
        let digest = Digest32::from_bytes(admitted.catalog_digest().bytes());
        let identity = ensure_catalog_object(&self.catalogs, digest, exact_catalog_bytes)
            .map_err(|error| self.prejournal_error(error))?;
        self.writer_remember_catalog_object_identity(digest, identity)?;
        self.writer_seed_rollback_catalog(admitted)?;
        Ok(())
    }

    /// Read-only proof that the exact active catalog is already the durable
    /// high-water and its authenticated object was previously published.
    pub(crate) fn writer_validate_active_catalog_materialized(
        &mut self,
        admitted: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<bool, ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        validate_exact_catalog(admitted, exact_catalog_bytes)?;
        self.state.validate()?;
        let candidate = StoredCatalogCheckpoint::from_admitted(
            admitted.checkpoint(),
            exact_catalog_bytes.len(),
        )?;
        if self.state.checkpoint() != Some(&candidate) {
            return Ok(false);
        }
        validate_catalog_lines(&self.state, admitted.catalog())?;
        let materialized =
            self.writer_validate_catalog_object(candidate.catalog_sha256, exact_catalog_bytes)?;
        if materialized {
            self.writer_seed_active_catalog(admitted)?;
        }
        Ok(materialized)
    }

    /// Read-only proof that the exact acquired active catalog is already the
    /// durable high-water and its authenticated object was previously
    /// published.
    #[cfg(feature = "acquired-packages")]
    pub(crate) fn writer_validate_active_acquired_catalog_materialized(
        &mut self,
        admitted: &AdmittedAcquiredCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<bool, ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        validate_exact_catalog(admitted, exact_catalog_bytes)?;
        self.state.validate()?;
        let candidate = StoredCatalogCheckpoint::from_admitted(
            admitted.checkpoint(),
            exact_catalog_bytes.len(),
        )?;
        if self.state.checkpoint() != Some(&candidate) {
            return Ok(false);
        }
        validate_catalog_lines(&self.state, admitted.catalog())?;
        let materialized =
            self.writer_validate_catalog_object(candidate.catalog_sha256, exact_catalog_bytes)?;
        if materialized {
            self.writer_seed_active_acquired_catalog(admitted)?;
        }
        Ok(materialized)
    }

    /// Read-only proof that the exact explicitly approved rollback catalog was
    /// previously published beneath the active high-water.
    pub(crate) fn writer_validate_rollback_catalog_materialized(
        &mut self,
        admitted: &AdmittedRollbackCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<bool, ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        validate_exact_rollback_catalog(admitted, exact_catalog_bytes)?;
        self.state.validate()?;
        let Some(current) = self.state.checkpoint() else {
            return Ok(false);
        };
        if current.authority_id != Digest32::from_bytes(admitted.authority().bytes()) {
            return Err(ExtensionRepositoryError::AuthorityMismatch);
        }
        if admitted.revision().get() > current.revision {
            return Err(ExtensionRepositoryError::RollbackCatalogAboveHighWater);
        }
        if admitted.revision().get() == current.revision
            && (Digest32::from_bytes(admitted.catalog_digest().bytes()) != current.catalog_sha256
                || Digest32::from_bytes(admitted.inventory_digest().bytes())
                    != current.inventory_sha256
                || admitted.catalog_length() != current.catalog_length)
        {
            return Err(ExtensionRepositoryError::CatalogEquivocation);
        }
        let materialized = self.writer_validate_catalog_object(
            Digest32::from_bytes(admitted.catalog_digest().bytes()),
            exact_catalog_bytes,
        )?;
        if materialized {
            self.writer_seed_rollback_catalog(admitted)?;
        }
        Ok(materialized)
    }

    fn writer_validate_catalog_object(
        &mut self,
        digest: Digest32,
        exact_catalog_bytes: &[u8],
    ) -> Result<bool, ExtensionRepositoryError> {
        let name = catalog_file(digest);
        let actual_identity = self
            .catalogs
            .regular_identity(&name)
            .map_err(|error| self.prejournal_error(error.into()))?;
        let expected_identity = self.catalog_object_identities.get(&digest).copied();
        let identity = match (actual_identity, expected_identity) {
            (None, None) if !self.catalog_object_ids.contains(&digest) => return Ok(false),
            (Some(actual), Some(expected))
                if actual == expected && self.catalog_object_ids.contains(&digest) =>
            {
                expected
            }
            _ => return Err(self.prejournal_error(ExtensionRepositoryError::StateCorrupt)),
        };
        let stored = read_required(&self.catalogs, &name, MAX_EXTENSION_RELEASE_CATALOG_BYTES)
            .map_err(|error| self.prejournal_error(error))?;
        let observed_identity = self
            .catalogs
            .regular_identity(&name)
            .map_err(|error| self.prejournal_error(error.into()))?;
        if stored != exact_catalog_bytes || observed_identity != Some(identity) {
            return Err(self.prejournal_error(ExtensionRepositoryError::StateCorrupt));
        }
        Ok(true)
    }

    pub(crate) fn writer_stage_active_catalog_candidate(
        &mut self,
        admitted: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<(), ExtensionRepositoryError> {
        self.stage_catalog_candidate_view(admitted, exact_catalog_bytes)?;
        self.writer_seed_active_catalog(admitted)
    }

    #[cfg(feature = "acquired-packages")]
    pub(crate) fn writer_stage_active_acquired_catalog_candidate(
        &mut self,
        admitted: &AdmittedAcquiredCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<(), ExtensionRepositoryError> {
        self.stage_catalog_candidate_view(admitted, exact_catalog_bytes)?;
        self.writer_seed_active_acquired_catalog(admitted)
    }

    fn stage_catalog_candidate_view(
        &mut self,
        admitted: &impl CatalogWitnessView,
        exact_catalog_bytes: &[u8],
    ) -> Result<(), ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        self.writer_require_gc_idle()?;
        validate_exact_catalog(admitted, exact_catalog_bytes)?;
        let candidate = StoredCatalogCheckpoint::from_admitted(
            admitted.checkpoint(),
            exact_catalog_bytes.len(),
        )?;
        if let Err(error) = plan_record(&self.state, admitted.catalog(), candidate.clone()) {
            return Err(self.prejournal_error(error));
        }
        let identity = ensure_catalog_object(
            &self.catalogs,
            candidate.catalog_sha256,
            exact_catalog_bytes,
        )
        .map_err(|error| self.prejournal_error(error))?;
        self.writer_remember_catalog_object_identity(candidate.catalog_sha256, identity)?;
        Ok(())
    }

    fn record_view(
        &mut self,
        witness: &impl CatalogWitnessView,
        exact_catalog_bytes: &[u8],
        fault: FaultPoint,
    ) -> Result<BundledCatalogRecordOutcome, ExtensionRepositoryError> {
        if self.writer_is_sealed() {
            return Err(ExtensionRepositoryError::Sealed);
        }
        validate_exact_catalog(witness, exact_catalog_bytes)?;
        let candidate = StoredCatalogCheckpoint::from_admitted(
            witness.checkpoint(),
            exact_catalog_bytes.len(),
        )?;
        let catalog_digest = candidate.catalog_sha256;
        let plan = plan_record(&self.state, witness.catalog(), candidate.clone())?;

        match &plan {
            RecordPlan::Advance(_) => self.validate_materialization_advance(candidate)?,
            RecordPlan::Replay => {
                if let Some(runtime) = self.materialization.as_ref() {
                    let validation = runtime.validate_gc_idle_projection();
                    match validation {
                        Ok(()) => {}
                        Err(ExtensionRepositoryError::GarbageCollectionInProgress) => {
                            return Err(ExtensionRepositoryError::GarbageCollectionInProgress);
                        }
                        Err(error) => return Err(self.prejournal_error(error)),
                    }
                }
            }
        }

        let identity =
            match ensure_catalog_object(&self.catalogs, catalog_digest, exact_catalog_bytes) {
                Ok(identity) => identity,
                Err(error) => return Err(self.prejournal_error(error)),
            };
        self.writer_remember_catalog_object_identity(catalog_digest, identity)?;
        self.fail_if(fault, FaultPoint::AfterCatalogObject)?;

        let RecordPlan::Advance(next_state) = plan else {
            return Ok(BundledCatalogRecordOutcome::IdempotentReplay);
        };
        let next_bytes = codec::encode(&next_state, MAX_STATE_BYTES)
            .map_err(|_| ExtensionRepositoryError::StateCorrupt)?;
        let previous_digest = codec::digest(&self.state_bytes);
        let next_digest = codec::digest(&next_bytes);
        let journal = TransitionJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation: next_state.generation,
            previous_state_sha256: previous_digest,
            next_state_sha256: next_digest,
            next_state: next_state.clone(),
        };
        let journal_bytes = codec::encode(&journal, MAX_JOURNAL_BYTES)
            .map_err(|_| ExtensionRepositoryError::StateCorrupt)?;
        let journal_name = match publish_journal(&self.journals, &journal, &journal_bytes) {
            Ok(name) => name,
            Err(error) => return Err(self.prejournal_error(error)),
        };
        self.fail_if(fault, FaultPoint::AfterJournal)?;

        if atomic_write_control(
            self._namespace.directory(),
            &state_file(),
            &state_stage(),
            &next_bytes,
            MAX_STATE_BYTES,
        )
        .is_err()
        {
            return Err(self.seal_ambiguous());
        }
        self.fail_if(fault, FaultPoint::AfterState)?;

        let checkpoint = RecoveryCheckpoint::new(next_state.generation, next_digest);
        if write_checkpoint(self._namespace.directory(), &checkpoint).is_err() {
            return Err(self.seal_ambiguous());
        }
        self.fail_if(fault, FaultPoint::AfterCheckpoint)?;

        if self
            .journals
            .remove_verified_regular(&journal_name)
            .is_err()
        {
            return Err(self.seal_ambiguous());
        }
        self.fail_if(fault, FaultPoint::AfterJournalRetirement)?;
        self.state = next_state;
        self.state_bytes = next_bytes;
        // Materialization recovery is bound to one exact catalog checkpoint.
        // A newer catalog floor may change the package rows that are eligible
        // for activation, so retain no live runtime opened under the previous
        // checkpoint. The next materialization operation must recover and
        // rebind it against `self.state.checkpoint()` before doing any work.
        self.materialization = None;
        Ok(BundledCatalogRecordOutcome::Recorded)
    }

    fn validate_materialization_advance(
        &mut self,
        candidate: StoredCatalogCheckpoint,
    ) -> Result<(), ExtensionRepositoryError> {
        if self.materialization.is_none() {
            let high_water = self.state.checkpoint().cloned();
            let catalog_recovery = materialization::ProductCatalogRecovery::new(
                &self.catalogs,
                &self.catalog_object_ids,
                &self.catalog_object_identities,
                &mut self.catalog_admission_cache,
                high_water.as_ref(),
            );
            let reopened = materialization::open_or_recover(
                self._namespace.directory(),
                catalog_recovery,
                true,
                materialization::FaultPoint::None,
            );
            let reopened = match reopened {
                Ok(runtime) => runtime,
                Err(error) => return Err(self.prejournal_error(error)),
            };
            self.materialization = Some(reopened);
        }
        let runtime = self
            .materialization
            .as_ref()
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        let gc_validation = runtime.validate_gc_idle_projection();
        match gc_validation {
            Ok(()) => {}
            Err(ExtensionRepositoryError::GarbageCollectionInProgress) => {
                return Err(ExtensionRepositoryError::GarbageCollectionInProgress);
            }
            Err(error) => return Err(self.prejournal_error(error)),
        }
        let validation = materialization::validate_catalog_advance(runtime, &candidate);
        validation.map_err(|error| self.prejournal_error(error))
    }

    fn prejournal_error(&mut self, error: ExtensionRepositoryError) -> ExtensionRepositoryError {
        if matches!(
            error,
            ExtensionRepositoryError::StateCorrupt
                | ExtensionRepositoryError::FileSystem(
                    PrivateFsError::Unsafe | PrivateFsError::BoundExceeded
                )
        ) {
            // A live repository must not advance an unrelated catalog after
            // observing durable namespace damage. This is a clean pre-journal
            // failure, so preserve the precise diagnosis while sealing the
            // instance until a fresh open performs full read-only preflight.
            self.writer_seal();
            return error;
        }
        if matches!(
            error,
            ExtensionRepositoryError::RecoveryAmbiguous
                | ExtensionRepositoryError::SettlementAmbiguous
        ) || matches!(
            error,
            ExtensionRepositoryError::FileSystem(
                PrivateFsError::IdentityAmbiguous
                    | PrivateFsError::SettlementUnknown
                    | PrivateFsError::Quarantined
                    | PrivateFsError::AlreadyExists
            )
        ) {
            self.writer_seal();
            ExtensionRepositoryError::SettlementAmbiguous
        } else {
            error
        }
    }

    fn writer_remember_catalog_object_identity(
        &mut self,
        digest: Digest32,
        identity: FileIdentity,
    ) -> Result<(), ExtensionRepositoryError> {
        let result = insert_catalog_object_identity(
            &mut self.catalog_object_ids,
            &mut self.catalog_object_identities,
            digest,
            identity,
        );
        result.map_err(|error| self.prejournal_error(error))
    }

    fn writer_seed_active_catalog(
        &mut self,
        admitted: &AdmittedBundledCatalog,
    ) -> Result<(), ExtensionRepositoryError> {
        let digest = Digest32::from_bytes(admitted.catalog_digest().bytes());
        let identity = self
            .catalog_object_identities
            .get(&digest)
            .copied()
            .ok_or(ExtensionRepositoryError::StateCorrupt)
            .map_err(|error| self.prejournal_error(error))?;
        let result = self.catalog_admission_cache.seed_active(identity, admitted);
        result.map_err(|error| self.prejournal_error(error))
    }

    #[cfg(feature = "acquired-packages")]
    fn writer_seed_active_acquired_catalog(
        &mut self,
        admitted: &AdmittedAcquiredCatalog,
    ) -> Result<(), ExtensionRepositoryError> {
        let digest = Digest32::from_bytes(admitted.catalog_digest().bytes());
        let identity = self
            .catalog_object_identities
            .get(&digest)
            .copied()
            .ok_or(ExtensionRepositoryError::StateCorrupt)
            .map_err(|error| self.prejournal_error(error))?;
        let result = self
            .catalog_admission_cache
            .seed_active_acquired(identity, admitted);
        result.map_err(|error| self.prejournal_error(error))
    }

    fn writer_seed_rollback_catalog(
        &mut self,
        admitted: &AdmittedRollbackCatalog,
    ) -> Result<(), ExtensionRepositoryError> {
        let digest = Digest32::from_bytes(admitted.catalog_digest().bytes());
        let identity = self
            .catalog_object_identities
            .get(&digest)
            .copied()
            .ok_or(ExtensionRepositoryError::StateCorrupt)
            .map_err(|error| self.prejournal_error(error))?;
        let result = self
            .catalog_admission_cache
            .seed_rollback(identity, admitted);
        result.map_err(|error| self.prejournal_error(error))
    }

    fn seal_ambiguous(&mut self) -> ExtensionRepositoryError {
        self.writer_seal();
        ExtensionRepositoryError::SettlementAmbiguous
    }

    fn fail_if(
        &mut self,
        configured: FaultPoint,
        reached: FaultPoint,
    ) -> Result<(), ExtensionRepositoryError> {
        #[cfg(test)]
        if configured == reached {
            self.writer_seal();
            return Err(ExtensionRepositoryError::InjectedCrash);
        }
        #[cfg(not(test))]
        let _ = (configured, reached);
        Ok(())
    }

    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
    fn record_with_fault(
        &mut self,
        witness: &TestCatalogWitness,
        exact_catalog_bytes: &[u8],
        fault: FaultPoint,
    ) -> Result<BundledCatalogRecordOutcome, ExtensionRepositoryError> {
        self.record_view(witness, exact_catalog_bytes, fault)
    }

    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    pub(crate) fn writer_record_bundled_catalog_with_fault(
        &mut self,
        witness: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
        fault: FaultPoint,
    ) -> Result<BundledCatalogRecordOutcome, ExtensionRepositoryError> {
        let outcome = self.record_view(witness, exact_catalog_bytes, fault)?;
        self.writer_seed_active_catalog(witness)?;
        Ok(outcome)
    }
}

trait CatalogWitnessView {
    fn catalog(&self) -> &ExtensionReleaseCatalog;
    fn checkpoint(&self) -> BundledCatalogCheckpoint;
}

impl CatalogWitnessView for AdmittedBundledCatalog {
    fn catalog(&self) -> &ExtensionReleaseCatalog {
        AdmittedBundledCatalog::catalog(self)
    }

    fn checkpoint(&self) -> BundledCatalogCheckpoint {
        AdmittedBundledCatalog::checkpoint(self)
    }
}

#[cfg(feature = "acquired-packages")]
impl CatalogWitnessView for AdmittedAcquiredCatalog {
    fn catalog(&self) -> &ExtensionReleaseCatalog {
        AdmittedAcquiredCatalog::catalog(self)
    }

    fn checkpoint(&self) -> BundledCatalogCheckpoint {
        AdmittedAcquiredCatalog::checkpoint(self)
    }
}

enum RecordPlan {
    Replay,
    Advance(RepositoryState),
}

fn plan_record(
    state: &RepositoryState,
    catalog: &ExtensionReleaseCatalog,
    candidate: StoredCatalogCheckpoint,
) -> Result<RecordPlan, ExtensionRepositoryError> {
    state.validate()?;
    if let Some(authority) = state.authority_id {
        if authority != candidate.authority_id {
            return Err(ExtensionRepositoryError::AuthorityMismatch);
        }
        let current = state
            .checkpoint()
            .ok_or(ExtensionRepositoryError::StateCorrupt)?;
        match candidate.revision.cmp(&current.revision) {
            Ordering::Less => return Err(ExtensionRepositoryError::CatalogRollback),
            Ordering::Equal => {
                if candidate != *current {
                    return Err(ExtensionRepositoryError::CatalogEquivocation);
                }
                validate_catalog_lines(state, catalog)?;
                return Ok(RecordPlan::Replay);
            }
            Ordering::Greater => {}
        }
    }

    let mut lines = state.package_line_high_waters.clone();
    for package in catalog.packages() {
        let candidate_line = PackageLineHighWater::from_package(package)?;
        match lines.binary_search_by_key(&candidate_line.package_key, |line| line.package_key) {
            Ok(index) => match candidate_line.revision.cmp(&lines[index].revision) {
                Ordering::Less => return Err(ExtensionRepositoryError::PackageRollback),
                Ordering::Equal
                    if candidate_line.package_row_sha256 != lines[index].package_row_sha256 =>
                {
                    return Err(ExtensionRepositoryError::PackageEquivocation);
                }
                Ordering::Equal => {}
                Ordering::Greater => lines[index] = candidate_line,
            },
            Err(index) => {
                if lines.len() >= MAX_PACKAGE_LINE_HIGH_WATERS {
                    return Err(ExtensionRepositoryError::PackageLineLimit);
                }
                lines.insert(index, candidate_line);
            }
        }
    }

    let next = RepositoryState {
        schema_version: crate::state::STATE_SCHEMA_VERSION,
        generation: state.next_generation()?,
        authority_id: Some(candidate.authority_id),
        catalog_high_water: Some(candidate),
        package_line_high_waters: lines,
    };
    next.validate()?;
    validate_catalog_lines(&next, catalog)?;
    Ok(RecordPlan::Advance(next))
}

fn validate_exact_catalog(
    witness: &impl CatalogWitnessView,
    bytes: &[u8],
) -> Result<(), ExtensionRepositoryError> {
    if bytes.is_empty() || bytes.len() > MAX_EXTENSION_RELEASE_CATALOG_BYTES {
        return Err(ExtensionRepositoryError::CatalogBytesMismatch);
    }
    let parsed = parse_catalog_canonical(bytes)
        .map_err(|_| ExtensionRepositoryError::CatalogBytesMismatch)?;
    if &parsed != witness.catalog()
        || parsed.digest() != witness.checkpoint().catalog_digest()
        || parsed.authority() != witness.checkpoint().authority()
        || parsed.revision() != witness.checkpoint().revision()
    {
        return Err(ExtensionRepositoryError::CatalogBytesMismatch);
    }
    Ok(())
}

fn validate_exact_rollback_catalog(
    admitted: &AdmittedRollbackCatalog,
    bytes: &[u8],
) -> Result<(), ExtensionRepositoryError> {
    if bytes.is_empty()
        || bytes.len() > MAX_EXTENSION_RELEASE_CATALOG_BYTES
        || u64::try_from(bytes.len()).ok() != Some(admitted.catalog_length())
    {
        return Err(ExtensionRepositoryError::CatalogBytesMismatch);
    }
    let parsed = parse_catalog_canonical(bytes)
        .map_err(|_| ExtensionRepositoryError::CatalogBytesMismatch)?;
    if &parsed != admitted.catalog()
        || parsed.authority() != admitted.authority()
        || parsed.revision() != admitted.revision()
        || parsed.digest() != admitted.catalog_digest()
    {
        return Err(ExtensionRepositoryError::CatalogBytesMismatch);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FaultPoint {
    None,
    AfterCatalogObject,
    AfterJournal,
    AfterState,
    AfterCheckpoint,
    AfterJournalRetirement,
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
struct TestCatalogWitness {
    catalog: ExtensionReleaseCatalog,
    checkpoint: BundledCatalogCheckpoint,
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
impl CatalogWitnessView for TestCatalogWitness {
    fn catalog(&self) -> &ExtensionReleaseCatalog {
        &self.catalog
    }

    fn checkpoint(&self) -> BundledCatalogCheckpoint {
        self.checkpoint
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests;

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;

    #[test]
    fn unavailable_namespace_primitive_fails_before_repository_or_path_mutation() {
        let result = LockedPrivateNamespace::open_or_create("not-consulted")
            .map_err(ExtensionRepositoryError::from)
            .and_then(ExtensionRepository::open);
        let error = match result {
            Ok(_) => panic!("Windows private namespace unexpectedly became available"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            ExtensionRepositoryError::FileSystem(PrivateFsError::PrimitiveUnavailable)
        );
    }
}
