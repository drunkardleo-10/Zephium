//! Monotonic authenticated-catalog admission and durable transitions.

use std::cmp::Ordering;

use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, BundledCatalogCheckpoint,
};
use zephium_extension_package::{ExtensionReleaseCatalog, MAX_EXTENSION_RELEASE_CATALOG_BYTES};
use zephium_private_fs::{LockedPrivateNamespace, PrivateDirectory, PrivateFsError};

use crate::codec;
use crate::materialization::{self, MaterializationRuntime};
use crate::names::{catalog_file, state_file, state_stage};
use crate::recovery::open_repository;
use crate::state::{
    validate_catalog_lines, Digest32, PackageLineHighWater, RecoveryCheckpoint, RepositoryState,
    StoredCatalogCheckpoint, TransitionJournal, JOURNAL_SCHEMA_VERSION, MAX_JOURNAL_BYTES,
    MAX_PACKAGE_LINE_HIGH_WATERS, MAX_STATE_BYTES,
};
use crate::storage::{
    atomic_write_control, ensure_catalog_object, publish_journal, read_required, write_checkpoint,
};
use crate::ExtensionRepositoryError;

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
use zephium_private_fs::PrivateComponent;

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
use crate::names::{self, catalog_stage, checkpoint_stage, journal_file, journal_stage};
#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
use crate::state::MAX_CHECKPOINT_BYTES;

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
    state: RepositoryState,
    state_bytes: Vec<u8>,
    materialization: Option<MaterializationRuntime>,
    sealed: bool,
}

impl ExtensionRepository {
    /// Opens an already-admitted private namespace and recovers one exact state.
    ///
    /// Unknown entries, corrupt objects, a forked or gapped journal chain, or
    /// disagreement between state and checkpoint fail closed. The caller must
    /// create the namespace at its product-owned application-data location.
    pub fn open(namespace: LockedPrivateNamespace) -> Result<Self, ExtensionRepositoryError> {
        let opened = open_repository(namespace)?;
        Ok(Self {
            _namespace: opened.namespace,
            catalogs: opened.catalogs,
            journals: opened.journals,
            state: opened.state,
            state_bytes: opened.state_bytes,
            materialization: Some(opened.materialization),
            sealed: false,
        })
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
        self.record_view(admitted, exact_catalog_bytes, FaultPoint::None)
    }

    pub(crate) const fn writer_is_sealed(&self) -> bool {
        self.sealed
    }

    pub(crate) fn writer_materialization(
        &mut self,
    ) -> Result<&MaterializationRuntime, ExtensionRepositoryError> {
        if self.sealed {
            return Err(ExtensionRepositoryError::Sealed);
        }
        if self.materialization.is_none() {
            self.writer_recover_materialization()?;
        }
        self.materialization
            .as_ref()
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
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
        if self.sealed {
            return Err(ExtensionRepositoryError::Sealed);
        }
        self.materialization = None;
        match materialization::open_or_recover(
            self._namespace.directory(),
            &self.catalogs,
            self.state.checkpoint(),
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
        self.sealed = true;
    }

    pub(crate) fn writer_ensure_rollback_catalog(
        &mut self,
        admitted: &AdmittedRollbackBundledCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<(), ExtensionRepositoryError> {
        if self.sealed {
            return Err(ExtensionRepositoryError::Sealed);
        }
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
        ensure_catalog_object(&self.catalogs, digest, exact_catalog_bytes)
            .map_err(|error| self.prejournal_error(error))
    }

    /// Read-only proof that the exact active catalog is already the durable
    /// high-water and its authenticated object was previously published.
    pub(crate) fn writer_validate_active_catalog_materialized(
        &mut self,
        admitted: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<bool, ExtensionRepositoryError> {
        if self.sealed {
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
        self.writer_validate_catalog_object(candidate.catalog_sha256, exact_catalog_bytes)
    }

    /// Read-only proof that the exact explicitly approved rollback catalog was
    /// previously published beneath the active high-water.
    pub(crate) fn writer_validate_rollback_catalog_materialized(
        &mut self,
        admitted: &AdmittedRollbackBundledCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<bool, ExtensionRepositoryError> {
        if self.sealed {
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
        self.writer_validate_catalog_object(
            Digest32::from_bytes(admitted.catalog_digest().bytes()),
            exact_catalog_bytes,
        )
    }

    fn writer_validate_catalog_object(
        &mut self,
        digest: Digest32,
        exact_catalog_bytes: &[u8],
    ) -> Result<bool, ExtensionRepositoryError> {
        let name = catalog_file(digest);
        let exists = self
            .catalogs
            .regular_exists(&name)
            .map_err(|error| self.prejournal_error(error.into()))?;
        if !exists {
            return Ok(false);
        }
        let stored = read_required(&self.catalogs, &name, MAX_EXTENSION_RELEASE_CATALOG_BYTES)
            .map_err(|error| self.prejournal_error(error))?;
        if stored != exact_catalog_bytes {
            return Err(self.prejournal_error(ExtensionRepositoryError::StateCorrupt));
        }
        Ok(true)
    }

    pub(crate) fn writer_stage_active_catalog_candidate(
        &mut self,
        admitted: &AdmittedBundledCatalog,
        exact_catalog_bytes: &[u8],
    ) -> Result<(), ExtensionRepositoryError> {
        self.stage_catalog_candidate_view(admitted, exact_catalog_bytes)
    }

    fn stage_catalog_candidate_view(
        &mut self,
        admitted: &impl CatalogWitnessView,
        exact_catalog_bytes: &[u8],
    ) -> Result<(), ExtensionRepositoryError> {
        if self.sealed {
            return Err(ExtensionRepositoryError::Sealed);
        }
        validate_exact_catalog(admitted, exact_catalog_bytes)?;
        let candidate = StoredCatalogCheckpoint::from_admitted(
            admitted.checkpoint(),
            exact_catalog_bytes.len(),
        )?;
        if let Err(error) = plan_record(&self.state, admitted.catalog(), candidate.clone()) {
            return Err(self.prejournal_error(error));
        }
        ensure_catalog_object(
            &self.catalogs,
            candidate.catalog_sha256,
            exact_catalog_bytes,
        )
        .map_err(|error| self.prejournal_error(error))
    }

    fn record_view(
        &mut self,
        witness: &impl CatalogWitnessView,
        exact_catalog_bytes: &[u8],
        fault: FaultPoint,
    ) -> Result<BundledCatalogRecordOutcome, ExtensionRepositoryError> {
        if self.sealed {
            return Err(ExtensionRepositoryError::Sealed);
        }
        validate_exact_catalog(witness, exact_catalog_bytes)?;
        let candidate = StoredCatalogCheckpoint::from_admitted(
            witness.checkpoint(),
            exact_catalog_bytes.len(),
        )?;
        let catalog_digest = candidate.catalog_sha256;
        let plan = plan_record(&self.state, witness.catalog(), candidate.clone())?;

        if matches!(&plan, RecordPlan::Advance(_)) {
            self.validate_materialization_advance(candidate)?;
        }

        if let Err(error) =
            ensure_catalog_object(&self.catalogs, catalog_digest, exact_catalog_bytes)
        {
            return Err(self.prejournal_error(error));
        }
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
            let reopened = materialization::open_or_recover(
                self._namespace.directory(),
                &self.catalogs,
                self.state.checkpoint(),
                true,
                materialization::FaultPoint::None,
            );
            let reopened = match reopened {
                Ok(runtime) => runtime,
                Err(error) => return Err(self.prejournal_error(error)),
            };
            self.materialization = Some(reopened);
        }
        let validation = self
            .materialization
            .as_ref()
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
            .and_then(|runtime| materialization::validate_catalog_advance(runtime, &candidate));
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
        self.record_view(witness, exact_catalog_bytes, fault)
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
    let parsed = ExtensionReleaseCatalog::parse_canonical(bytes)
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
    admitted: &AdmittedRollbackBundledCatalog,
    bytes: &[u8],
) -> Result<(), ExtensionRepositoryError> {
    if bytes.is_empty()
        || bytes.len() > MAX_EXTENSION_RELEASE_CATALOG_BYTES
        || u64::try_from(bytes.len()).ok() != Some(admitted.catalog_length())
    {
        return Err(ExtensionRepositoryError::CatalogBytesMismatch);
    }
    let parsed = ExtensionReleaseCatalog::parse_canonical(bytes)
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
