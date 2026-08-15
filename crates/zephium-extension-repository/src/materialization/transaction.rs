//! Typed crash-durable materialization state transitions.
//!
//! Every operation consumes the recovered runtime. A caller must perform a
//! fresh recovery after success, after a pre-journal failure that left bounded
//! residue, and after any ambiguous settlement. This prevents derived maps,
//! sealed-root handles, or build-stage projections from surviving a durable
//! state change.

use zephium_core::extensions::{
    ExtensionPackageIdentity, ExtensionPackagePayloadIdentity, ExtensionPackagePinReleaseBinding,
    ExtensionRuntimeBackendTarget,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_private_fs::{ByteLimit, DirectoryIdentity, FileIdentity, PrivateFsError};

use super::catalog_set::{
    CatalogSetTransitionProof, VerifiedActiveCatalogSet, VerifiedRollbackCatalogSet,
};
#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
use super::cleanup::{
    inspect_package_build_commit_marker, reconcile_build_stages_for_abort, PackageBuildCommitMarker,
};
use super::cleanup::{AbortablePackageBuild, BuildStagesAbsent, CleanupError};
use super::gc::{GarbageCollectionAbsenceProof, GarbageCollectionPlan};
use super::names::{self, RecordNameKind, TreeNameKind};
#[cfg(feature = "acquired-packages")]
use super::objects::VerifiedAcquiredActivePackageClosure;
use super::objects::{
    PackageObjectCapacity, PackageObjectError, VerifiedActivePackageClosure,
    VerifiedRollbackPackageClosure,
};
use super::package_lease::VerifiedPackagePinAdmission;
use super::policy::{
    next_durable_generation, validate_completed_tree_budget, validate_package_anchor_consistency,
};
#[cfg(feature = "acquired-packages")]
use super::prepare::PreparedAcquiredActivePackage;
use super::prepare::{PreparedActivePackage, PreparedRollbackPackage};
use super::records::{
    CatalogSetRecord, PackageIdentityAnchor, PackageRecord, StoredPayloadIdentity,
    StoredRuntimePlatformFamily,
};
use super::runtime::MaterializationRuntime;
use super::state::{
    DurablePackagePin, HistoricalCatalogRole, MaterializationBuildIntent,
    MaterializationCheckpoint, MaterializationJournal, MaterializationState, StoredBrowsingContext,
    MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION, MATERIALIZATION_JOURNAL_SCHEMA_VERSION,
    MAX_MATERIALIZATION_JOURNAL_BYTES, MAX_MATERIALIZATION_STATE_BYTES,
};
use super::storage::{read_required_control, remove_required_control, write_checkpoint};
use crate::codec;
use crate::state::Digest32;
use crate::storage::atomic_write_control;
use crate::ExtensionRepositoryError;

/// Failure phase for one consuming materialization transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaterializationTransitionError {
    /// The final journal was not submitted to the no-replace publication
    /// primitive. Fresh recovery is still required before another operation.
    Clean(ExtensionRepositoryError),
    /// The namespace was quarantined, the final journal may be durable, or a
    /// simulated process loss invalidated this live repository instance.
    MustSeal(ExtensionRepositoryError),
}

/// Linear acknowledgement that one transition fully settled.
///
/// The value carries no runtime authority. Its only purpose is to make callers
/// explicitly reopen rather than accidentally continue with stale projections.
#[must_use = "a settled materialization transition requires fresh recovery"]
pub(crate) struct MaterializationTransitionCommitted {
    _generation: u64,
    _seal: TransitionCommitSeal,
}

struct TransitionCommitSeal;

/// Writer-owned crash frontier used only by structural tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransitionFaultPoint {
    None,
    AfterJournalStage,
    AfterJournalPublication,
    AfterState,
    AfterCheckpoint,
    AfterJournalRetirement,
}

/// Durably starts one exact package build.
///
/// Two successor generations are reserved up front: one for the intent and
/// one for either completion or abort. The record is not a durable completion
/// root until a typed verified closure is consumed by a completion operation.
pub(crate) fn begin_active_package_build(
    runtime: MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: &PreparedActivePackage,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    begin_package_build_with_fault(
        runtime,
        capacity,
        prepared.record(),
        TransitionFaultPoint::None,
    )
}

/// Durably starts one acquired active-package build while binding the exact
/// disposable stage shape observed by its capacity proof.
#[cfg(feature = "acquired-packages")]
pub(crate) fn begin_acquired_active_package_build(
    runtime: MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: &PreparedAcquiredActivePackage,
    acquisition_stage: bool,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    begin_package_build_with_fault_kind(
        runtime,
        capacity,
        prepared.record(),
        true,
        acquisition_stage,
        TransitionFaultPoint::None,
    )
}

/// Publishes one exact bounded collection intent before physical mutation.
pub(crate) fn begin_garbage_collection(
    runtime: MaterializationRuntime,
    plan: GarbageCollectionPlan,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    validate_runtime(&runtime)?;
    require_no_object_stages(&runtime)?;
    if runtime._state.build_intent.is_some()
        || runtime._build_intent.is_some()
        || runtime._state.gc_intent.is_some()
        || runtime._gc_intent.is_some()
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }

    let next = plan.into_state();
    let intent = next
        .gc_intent
        .as_ref()
        .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    let mut reconstructed_completed = next.completed_package_record_ids.clone();
    reconstructed_completed.extend(&intent.package_record_ids);
    reconstructed_completed.sort_unstable();
    if reconstructed_completed
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    if next.generation != next_generation(runtime._state.generation)?
        || next.schema_version != runtime._state.schema_version
        || next.candidate_catalog_set_id != runtime._state.candidate_catalog_set_id
        || next.current_catalog_set_id != runtime._state.current_catalog_set_id
        || next.previous_catalog_set_id != runtime._state.previous_catalog_set_id
        || next.package_pins != runtime._state.package_pins
        || next.build_intent.is_some()
        || intent.generation != next.generation
        || reconstructed_completed != runtime._state.completed_package_record_ids
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    next.validate()
        .map_err(MaterializationTransitionError::Clean)?;
    commit_state_transition(runtime, next, TransitionFaultPoint::None)
}

/// Clears one durable collection intent after exact post-delete proof.
pub(crate) fn complete_garbage_collection(
    runtime: MaterializationRuntime,
    proof: GarbageCollectionAbsenceProof,
    catalogs_parent: DirectoryIdentity,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    validate_runtime_projection(&runtime)?;
    require_no_object_stages(&runtime)?;
    let intent = runtime
        ._state
        .gc_intent
        .as_ref()
        .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    if runtime._build_intent.is_some()
        || runtime._state.build_intent.is_some()
        || runtime._gc_intent.as_ref() != Some(intent)
        || proof.intent() != intent
        || proof.state_sha256() != codec::digest(&runtime._state_bytes)
        || proof.generation() != intent.generation
        || proof.records_parent() != runtime._records.identity()
        || proof.trees_parent() != runtime._trees.identity()
        || proof.catalogs_parent() != catalogs_parent
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }

    let mut next = runtime._state.clone();
    next.generation = next_generation(runtime._state.generation)?;
    next.gc_intent = None;
    next.validate()
        .map_err(MaterializationTransitionError::Clean)?;
    commit_state_transition(runtime, next, TransitionFaultPoint::None)
}

/// Selects one freshly authenticated active catalog set as the sole candidate.
pub(crate) fn stage_active_catalog_set_candidate(
    runtime: MaterializationRuntime,
    catalog_set: VerifiedActiveCatalogSet,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    stage_catalog_set_candidate_with_fault(
        runtime,
        catalog_set.into_transition_proof(),
        TransitionFaultPoint::None,
    )
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn stage_active_catalog_set_candidate_at_fault(
    runtime: MaterializationRuntime,
    catalog_set: VerifiedActiveCatalogSet,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    stage_catalog_set_candidate_with_fault(runtime, catalog_set.into_transition_proof(), fault)
}

/// Selects one freshly authenticated rollback catalog set as the sole
/// candidate without converting its rollback authority into active authority.
pub(crate) fn stage_rollback_catalog_set_candidate(
    runtime: MaterializationRuntime,
    catalog_set: VerifiedRollbackCatalogSet,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    stage_catalog_set_candidate_with_fault(
        runtime,
        catalog_set.into_transition_proof(),
        TransitionFaultPoint::None,
    )
}

fn stage_catalog_set_candidate_with_fault<Prepared>(
    runtime: MaterializationRuntime,
    catalog_set: CatalogSetTransitionProof<Prepared>,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    validate_runtime(&runtime)?;
    require_no_object_stages(&runtime)?;
    if runtime._state.build_intent.is_some() || runtime._build_intent.is_some() {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    validate_catalog_set_transition_proof(&runtime, &catalog_set)?;
    let slots = [
        runtime._state.candidate_catalog_set_id,
        runtime._state.current_catalog_set_id,
        runtime._state.previous_catalog_set_id,
    ];
    if runtime._state.candidate_catalog_set_id.is_some()
        || slots.contains(&Some(catalog_set.record_id))
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }

    let mut next = runtime._state.clone();
    next.generation = next_generation(runtime._state.generation)?;
    next.candidate_catalog_set_id = Some(catalog_set.record_id);
    next.validate()
        .map_err(MaterializationTransitionError::Clean)?;
    validate_next_state_references(&runtime, &next)?;
    commit_state_transition(runtime, next, fault)
}

/// Atomically promotes one exact active candidate to current.
pub(crate) fn promote_active_catalog_set(
    runtime: MaterializationRuntime,
    catalog_set: VerifiedActiveCatalogSet,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    promote_catalog_set_with_fault(
        runtime,
        catalog_set.into_transition_proof(),
        TransitionFaultPoint::None,
    )
}

/// Atomically promotes one exact explicitly approved rollback candidate while
/// retaining the nominal rollback witness through the commit.
pub(crate) fn promote_rollback_catalog_set(
    runtime: MaterializationRuntime,
    catalog_set: VerifiedRollbackCatalogSet,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    promote_catalog_set_with_fault(
        runtime,
        catalog_set.into_transition_proof(),
        TransitionFaultPoint::None,
    )
}

fn promote_catalog_set_with_fault<Prepared>(
    runtime: MaterializationRuntime,
    catalog_set: CatalogSetTransitionProof<Prepared>,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    validate_runtime(&runtime)?;
    require_no_object_stages(&runtime)?;
    if runtime._state.build_intent.is_some() || runtime._build_intent.is_some() {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    validate_catalog_set_transition_proof(&runtime, &catalog_set)?;
    if runtime._state.candidate_catalog_set_id != Some(catalog_set.record_id) {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }

    let mut next = runtime._state.clone();
    next.generation = next_generation(runtime._state.generation)?;
    next.candidate_catalog_set_id = None;
    next.previous_catalog_set_id = runtime._state.current_catalog_set_id;
    next.current_catalog_set_id = Some(catalog_set.record_id);
    next.validate()
        .map_err(MaterializationTransitionError::Clean)?;
    validate_next_state_references(&runtime, &next)?;
    commit_state_transition(runtime, next, fault)
}

/// Atomically swaps an explicitly approved rollback set with the current set.
/// A candidate must not exist, so rollback cannot accidentally discard a
/// separately prepared generation.
pub(crate) fn rollback_to_previous_catalog_set(
    runtime: MaterializationRuntime,
    rollback: VerifiedRollbackCatalogSet,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    rollback_to_previous_catalog_set_with_fault(runtime, rollback, TransitionFaultPoint::None)
}

fn rollback_to_previous_catalog_set_with_fault(
    runtime: MaterializationRuntime,
    rollback: VerifiedRollbackCatalogSet,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    validate_runtime(&runtime)?;
    require_no_object_stages(&runtime)?;
    if runtime._state.build_intent.is_some() || runtime._build_intent.is_some() {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    let rollback = rollback.into_transition_proof();
    validate_catalog_set_transition_proof(&runtime, &rollback)?;
    let current = runtime
        ._state
        .current_catalog_set_id
        .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    if runtime._state.candidate_catalog_set_id.is_some()
        || runtime._state.previous_catalog_set_id != Some(rollback.record_id)
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }

    let mut next = runtime._state.clone();
    next.generation = next_generation(runtime._state.generation)?;
    next.current_catalog_set_id = Some(rollback.record_id);
    next.previous_catalog_set_id = Some(current);
    next.validate()
        .map_err(MaterializationTransitionError::Clean)?;
    validate_next_state_references(&runtime, &next)?;
    commit_state_transition(runtime, next, fault)
}

/// Exact, non-serializable proof that a package is selected by the recovered
/// current catalog set. It is structural input to the later lease layer and is
/// not itself profile or activation authority.
#[must_use = "a current package proof must be pinned or deliberately discarded"]
#[allow(dead_code)]
pub(crate) struct CurrentCatalogPackagePinProof {
    state_generation: u64,
    records_parent: DirectoryIdentity,
    current_catalog_set_id: Digest32,
    package_key: Digest32,
    runtime_backend: ExtensionRuntimeBackendTarget,
    pin: DurablePackagePin,
}

impl CurrentCatalogPackagePinProof {
    pub(crate) fn pin_identity(&self) -> OwnerPackagePinIdentity {
        self.pin.into()
    }
}

/// Exact durable identity of one owner pin incarnation.
///
/// Store's native incarnation prevents owner ABA across repository reopen.
/// The consuming transition proof separately binds the current repository
/// generation and directory identity for in-process compare-and-swap safety.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct OwnerPackagePinIdentity(DurablePackagePin);

impl From<DurablePackagePin> for OwnerPackagePinIdentity {
    fn from(pin: DurablePackagePin) -> Self {
        Self(pin)
    }
}

impl OwnerPackagePinIdentity {
    /// Returns the process-local lease registry key for this exact pin.
    pub(crate) const fn lease_owner(self) -> (ProfileId, ExtensionInstallId) {
        (self.0.profile_id, self.0.install_id)
    }

    /// Compares the Store-owned portion of this exact repository pin.
    ///
    /// Package-record identity is deliberately resolved through the recovered
    /// catalog set by `verify_package_pin_release_admission`; it cannot be
    /// reconstructed from a Store package identity alone.
    pub(crate) fn matches_release_binding_identity(
        self,
        binding: &ExtensionPackagePinReleaseBinding,
    ) -> bool {
        self.0.profile_id == binding.profile()
            && self.0.install_id == binding.install_id()
            && self.0.browsing_context == StoredBrowsingContext::from(binding.browsing_context())
            && self.0.catalog_set_record_id.bytes() == binding.catalog_set_digest().bytes()
            && self.0.catalog_role == HistoricalCatalogRole::from(binding.catalog_role())
            && self.0.native_incarnation == binding.native_incarnation().get()
    }
}

/// Read-only disposition of an exact release binding against recovered pins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackagePinReleaseAdmission {
    /// The complete pin and its authenticated set/package/backend join exist.
    Present,
    /// No pin exists for the exact owner; no catalog-set object is required.
    AlreadyAbsent,
}

/// Exact durable pin resolved from a Store-owned release row after reopen.
///
/// The present variant is intentionally opaque: only this module can mint the
/// complete repository identity consumed by the removal transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveredPackagePinRelease {
    /// The Store row exactly names one coherent durable repository pin.
    Present(OwnerPackagePinIdentity),
    /// The owner has no durable pin, including a crash before pin commit.
    AlreadyAbsent,
}

/// Closed refusal while verifying one Store-authorized package-pin release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackagePinReleaseAdmissionError {
    /// Store and repository identify different owner/package/backend state.
    JournalPinMismatch,
    /// Recovered repository state is internally incoherent.
    DurableIncoherence,
}

/// Validates the complete materialization frontier before any release replay.
///
/// This preflight is intentionally separate from Store-binding resolution so
/// transient filesystem failures retain their clean retry classification.
/// Both same-process release and reopened reconciliation must run it before an
/// `AlreadyAbsent` fast path; otherwise physical object-stage residue could be
/// reported as settled without first proving the repository frontier clean.
pub(crate) fn preflight_package_pin_release(
    runtime: &MaterializationRuntime,
) -> Result<(), MaterializationTransitionError> {
    validate_runtime(runtime)?;
    require_no_object_stages(runtime)
}

/// Resolves a Store release binding against one complete repository pin.
///
/// Absence is checked before any catalog-set lookup so crash-before-pin
/// recovery remains a truthful `AlreadyAbsent`. Presence requires the complete
/// set row, package record, package identity, and backend join.
pub(crate) fn verify_package_pin_release_admission(
    runtime: &MaterializationRuntime,
    expected_pin: OwnerPackagePinIdentity,
    binding: &ExtensionPackagePinReleaseBinding,
) -> Result<PackagePinReleaseAdmission, PackagePinReleaseAdmissionError> {
    if !expected_pin.matches_release_binding_identity(binding) {
        return Err(PackagePinReleaseAdmissionError::JournalPinMismatch);
    }
    match resolve_recovered_package_pin_release(runtime, binding)? {
        RecoveredPackagePinRelease::Present(pin) if pin == expected_pin => {
            Ok(PackagePinReleaseAdmission::Present)
        }
        RecoveredPackagePinRelease::Present(_) => {
            Err(PackagePinReleaseAdmissionError::JournalPinMismatch)
        }
        RecoveredPackagePinRelease::AlreadyAbsent => Ok(PackagePinReleaseAdmission::AlreadyAbsent),
    }
}

/// Resolves one authenticated Store release row after repository reopen.
///
/// Callers must first run `preflight_package_pin_release` and preserve its
/// transition-error classification. This function repeats the in-memory state
/// validation defensively but deliberately does not collapse filesystem
/// preflight failures into an admission error.
///
/// Absence is returned before catalog-set lookup so crash-before-pin replay is
/// independent of objects that were never retained. Presence requires the
/// exact owner, context, historical set and role, Store native incarnation,
/// package identity, selected backend, and coherent durable set/package join.
pub(crate) fn resolve_recovered_package_pin_release(
    runtime: &MaterializationRuntime,
    binding: &ExtensionPackagePinReleaseBinding,
) -> Result<RecoveredPackagePinRelease, PackagePinReleaseAdmissionError> {
    validate_runtime(runtime).map_err(|_| PackagePinReleaseAdmissionError::DurableIncoherence)?;
    let owner = (
        binding.profile(),
        binding.install_id(),
        StoredBrowsingContext::from(binding.browsing_context()),
    );
    let index = match runtime
        ._state
        .package_pins
        .binary_search_by_key(&owner, DurablePackagePin::owner_key)
    {
        Ok(index) => index,
        Err(_) => return Ok(RecoveredPackagePinRelease::AlreadyAbsent),
    };
    let pin = runtime._state.package_pins[index];
    let pin_identity = OwnerPackagePinIdentity::from(pin);
    if !pin_identity.matches_release_binding_identity(binding) {
        return Err(PackagePinReleaseAdmissionError::JournalPinMismatch);
    }
    validate_present_package_pin_release(runtime, pin, binding)?;
    Ok(RecoveredPackagePinRelease::Present(pin_identity))
}

fn validate_present_package_pin_release(
    runtime: &MaterializationRuntime,
    pin: DurablePackagePin,
    binding: &ExtensionPackagePinReleaseBinding,
) -> Result<(), PackagePinReleaseAdmissionError> {
    let set = runtime
        ._catalog_sets
        .get(&pin.catalog_set_record_id)
        .ok_or(PackagePinReleaseAdmissionError::DurableIncoherence)?;
    validate_catalog_set_completed_projection(runtime, set)
        .map_err(|_| PackagePinReleaseAdmissionError::DurableIncoherence)?;
    let row = set
        .packages
        .iter()
        .find(|row| row.package_record_id == pin.package_record_id)
        .ok_or(PackagePinReleaseAdmissionError::DurableIncoherence)?;
    let package = runtime
        ._package_records
        .get(&pin.package_record_id)
        .ok_or(PackagePinReleaseAdmissionError::DurableIncoherence)?;
    if package.catalog != set.catalog
        || package.package.package_key != row.package_key
        || package.manifest.runtime_target != row.runtime_target
    {
        return Err(PackagePinReleaseAdmissionError::DurableIncoherence);
    }
    if !package_identity_matches(&package.package, binding.package())
        || !row
            .runtime_target
            .matches_runtime_backend(binding.runtime_backend())
    {
        return Err(PackagePinReleaseAdmissionError::JournalPinMismatch);
    }
    Ok(())
}

/// Exact owner-pin admission result. A replay carries no transition proof, so
/// it cannot accidentally create a no-op durable generation.
#[allow(dead_code)]
pub(crate) enum OwnerPackagePinPlan {
    /// The owner is absent; consume this proof in the exact add transition.
    Add(CurrentCatalogPackagePinProof),
    /// The owner already names the same complete Store-bound pin tuple.
    IdempotentReplay { pin: OwnerPackagePinIdentity },
    /// The exact owner key names a different durable Store-bound tuple.
    OwnerConflict,
}

#[allow(dead_code)]
pub(crate) fn plan_current_catalog_package_pin(
    runtime: &MaterializationRuntime,
    admission: &VerifiedPackagePinAdmission,
) -> Result<OwnerPackagePinPlan, MaterializationTransitionError> {
    validate_runtime(runtime)?;
    require_no_object_stages(runtime)?;
    let repository = admission.repository();
    let expected_current = admission.current_catalog_set_id();
    if runtime._root.identity() != repository.root
        || runtime._records.identity() != repository.records
        || runtime._trees.identity() != repository.trees
        || runtime._state.current_catalog_set_id != Some(expected_current)
        || runtime._state.build_intent.is_some()
        || runtime._build_intent.is_some()
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    let set = runtime
        ._catalog_sets
        .get(&expected_current)
        .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    let key = admission.package_key();
    let row = set
        .packages
        .binary_search_by_key(&key, |row| row.package_key)
        .ok()
        .and_then(|index| set.packages.get(index))
        .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    validate_catalog_set_completed_projection(runtime, set)?;
    let pin = admission.pin();
    if row.package_record_id != pin.package_record_id
        || pin.catalog_set_record_id != expected_current
        || !row
            .runtime_target
            .matches_runtime_backend(admission.runtime_backend())
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    let owner = pin.owner_key();
    match runtime
        ._state
        .package_pins
        .binary_search_by_key(&owner, DurablePackagePin::owner_key)
    {
        Ok(index) if runtime._state.package_pins[index] == pin => {
            return Ok(OwnerPackagePinPlan::IdempotentReplay { pin: pin.into() });
        }
        Ok(_) => return Ok(OwnerPackagePinPlan::OwnerConflict),
        Err(_) => {}
    }
    Ok(OwnerPackagePinPlan::Add(CurrentCatalogPackagePinProof {
        state_generation: runtime._state.generation,
        records_parent: runtime._records.identity(),
        current_catalog_set_id: expected_current,
        package_key: key,
        runtime_backend: admission.runtime_backend(),
        pin,
    }))
}

fn package_identity_matches(
    stored: &PackageIdentityAnchor,
    expected: &ExtensionPackageIdentity,
) -> bool {
    let payload_matches = match (stored.payload, expected.payload()) {
        (StoredPayloadIdentity::BundledTree, ExtensionPackagePayloadIdentity::BundledTree) => true,
        (
            StoredPayloadIdentity::AcquiredZip { length, sha256 },
            ExtensionPackagePayloadIdentity::AcquiredZip {
                length: expected_length,
                sha256: expected_sha256,
            },
        ) => length == expected_length.get() && sha256.bytes() == expected_sha256.bytes(),
        _ => false,
    };
    stored.authority_id.bytes() == expected.authority().bytes()
        && stored.package_key.bytes() == expected.key().bytes()
        && stored.revision == expected.revision().get()
        && payload_matches
        && stored.manifest_sha256.bytes() == expected.manifest_sha256().bytes()
        && stored.tree_sha256.bytes() == expected.tree_sha256().bytes()
}

/// Adds one exact owner/package pin. Existing owners are never replaced in
/// place; callers must explicitly remove the exact old record after runtime
/// drain and then recover before adding another.
#[allow(dead_code)]
pub(crate) fn add_owner_package_pin(
    runtime: MaterializationRuntime,
    proof: CurrentCatalogPackagePinProof,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    add_owner_package_pin_with_fault(runtime, proof, TransitionFaultPoint::None)
}

fn add_owner_package_pin_with_fault(
    runtime: MaterializationRuntime,
    proof: CurrentCatalogPackagePinProof,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    validate_runtime(&runtime)?;
    require_no_object_stages(&runtime)?;
    if runtime._state.generation != proof.state_generation
        || runtime._records.identity() != proof.records_parent
        || runtime._state.current_catalog_set_id != Some(proof.current_catalog_set_id)
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    let set = runtime
        ._catalog_sets
        .get(&proof.current_catalog_set_id)
        .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    if !set.packages.iter().any(|row| {
        row.package_key == proof.package_key
            && row.package_record_id == proof.pin.package_record_id
            && row
                .runtime_target
                .matches_runtime_backend(proof.runtime_backend)
    }) {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    let owner = proof.pin.owner_key();
    let mut next = runtime._state.clone();
    let insertion = next
        .package_pins
        .binary_search_by_key(&owner, DurablePackagePin::owner_key)
        .map_or_else(Ok, |_| {
            Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous))
        })?;
    next.package_pins.insert(insertion, proof.pin);
    next.generation = next_generation(runtime._state.generation)?;
    next.validate()
        .map_err(MaterializationTransitionError::Clean)?;
    validate_next_state_references(&runtime, &next)?;
    commit_state_transition(runtime, next, fault)
}

/// Linear proof for removing exactly one owner/package mapping.
#[allow(dead_code)]
pub(crate) struct OwnerPackagePinRemovalProof {
    state_generation: u64,
    records_parent: DirectoryIdentity,
    pin: DurablePackagePin,
}

/// Read-only exact removal plan. Absence is a truthful replay and carries no
/// transition capability.
#[allow(dead_code)]
pub(crate) enum OwnerPackagePinRemovalPlan {
    /// The exact complete owner/set/package/native-incarnation mapping is present.
    Remove(OwnerPackagePinRemovalProof),
    /// The owner is already absent.
    IdempotentReplay,
    /// The owner was rebound to another durable tuple or Store incarnation.
    Stale,
}

#[allow(dead_code)]
pub(crate) fn plan_owner_package_pin_removal(
    runtime: &MaterializationRuntime,
    expected_pin: OwnerPackagePinIdentity,
) -> Result<OwnerPackagePinRemovalPlan, MaterializationTransitionError> {
    validate_runtime(runtime)?;
    require_no_object_stages(runtime)?;
    if runtime._state.build_intent.is_some() || runtime._build_intent.is_some() {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    let owner = expected_pin.0.owner_key();
    let index = match runtime
        ._state
        .package_pins
        .binary_search_by_key(&owner, DurablePackagePin::owner_key)
    {
        Ok(index) => index,
        Err(_) => return Ok(OwnerPackagePinRemovalPlan::IdempotentReplay),
    };
    let pin = runtime._state.package_pins[index];
    if OwnerPackagePinIdentity::from(pin) != expected_pin {
        return Ok(OwnerPackagePinRemovalPlan::Stale);
    }
    Ok(OwnerPackagePinRemovalPlan::Remove(
        OwnerPackagePinRemovalProof {
            state_generation: runtime._state.generation,
            records_parent: runtime._records.identity(),
            pin,
        },
    ))
}

#[allow(dead_code)]
pub(crate) fn remove_owner_package_pin(
    runtime: MaterializationRuntime,
    proof: OwnerPackagePinRemovalProof,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    remove_owner_package_pin_with_fault(runtime, proof, TransitionFaultPoint::None)
}

fn remove_owner_package_pin_with_fault(
    runtime: MaterializationRuntime,
    proof: OwnerPackagePinRemovalProof,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    validate_runtime(&runtime)?;
    require_no_object_stages(&runtime)?;
    if runtime._state.build_intent.is_some()
        || runtime._build_intent.is_some()
        || runtime._state.generation != proof.state_generation
        || runtime._records.identity() != proof.records_parent
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    let owner = proof.pin.owner_key();
    let mut next = runtime._state.clone();
    let index = next
        .package_pins
        .binary_search_by_key(&owner, DurablePackagePin::owner_key)
        .map_err(|_| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    if next.package_pins[index] != proof.pin {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    next.package_pins.remove(index);
    next.generation = next_generation(runtime._state.generation)?;
    next.validate()
        .map_err(MaterializationTransitionError::Clean)?;
    validate_next_state_references(&runtime, &next)?;
    commit_state_transition(runtime, next, fault)
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn promote_active_catalog_set_at_fault(
    runtime: MaterializationRuntime,
    catalog_set: VerifiedActiveCatalogSet,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    promote_catalog_set_with_fault(runtime, catalog_set.into_transition_proof(), fault)
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn rollback_to_previous_catalog_set_at_fault(
    runtime: MaterializationRuntime,
    rollback: VerifiedRollbackCatalogSet,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    rollback_to_previous_catalog_set_with_fault(runtime, rollback, fault)
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn add_owner_package_pin_at_fault(
    runtime: MaterializationRuntime,
    proof: CurrentCatalogPackagePinProof,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    add_owner_package_pin_with_fault(runtime, proof, fault)
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn remove_owner_package_pin_at_fault(
    runtime: MaterializationRuntime,
    proof: OwnerPackagePinRemovalProof,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    remove_owner_package_pin_with_fault(runtime, proof, fault)
}

fn validate_catalog_set_transition_proof<Prepared>(
    runtime: &MaterializationRuntime,
    proof: &CatalogSetTransitionProof<Prepared>,
) -> Result<(), MaterializationTransitionError> {
    if proof.state_generation != runtime._state.generation
        || proof.records_parent != runtime._records.identity()
        || proof.record.record_id().ok() != Some(proof.record_id)
        || runtime._catalog_sets.get(&proof.record_id) != Some(&proof.record)
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    validate_catalog_set_completed_projection(runtime, &proof.record)
}

fn validate_catalog_set_completed_projection(
    runtime: &MaterializationRuntime,
    catalog_set: &CatalogSetRecord,
) -> Result<(), MaterializationTransitionError> {
    catalog_set
        .validate()
        .map_err(|_| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    for row in &catalog_set.packages {
        if runtime
            ._state
            .completed_package_record_ids
            .binary_search(&row.package_record_id)
            .is_err()
        {
            return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
        }
        let package = runtime
            ._package_records
            .get(&row.package_record_id)
            .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
        if package.catalog != catalog_set.catalog
            || package.package.package_key != row.package_key
            || package.manifest.runtime_target != row.runtime_target
        {
            return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
        }
    }
    Ok(())
}

fn validate_next_state_references(
    runtime: &MaterializationRuntime,
    state: &MaterializationState,
) -> Result<(), MaterializationTransitionError> {
    state
        .validate()
        .map_err(|_| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    let mut live_family = None;
    for catalog_set_id in state.catalog_pin_ids() {
        let set = runtime
            ._catalog_sets
            .get(&catalog_set_id)
            .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
        validate_catalog_set_completed_projection(runtime, set)?;
        for row in &set.packages {
            bind_transition_platform_family(
                &mut live_family,
                row.runtime_target.platform_family(),
            )?;
        }
    }

    for pin in &state.package_pins {
        let set = runtime
            ._catalog_sets
            .get(&pin.catalog_set_record_id)
            .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
        let row = set
            .packages
            .iter()
            .find(|row| row.package_record_id == pin.package_record_id)
            .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
        let package = runtime
            ._package_records
            .get(&pin.package_record_id)
            .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
        if package.catalog != set.catalog
            || package.package.package_key != row.package_key
            || package.manifest.runtime_target != row.runtime_target
        {
            return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
        }
        bind_transition_platform_family(&mut live_family, row.runtime_target.platform_family())?;
    }
    Ok(())
}

fn bind_transition_platform_family(
    observed: &mut Option<StoredRuntimePlatformFamily>,
    candidate: StoredRuntimePlatformFamily,
) -> Result<(), MaterializationTransitionError> {
    if observed.is_some_and(|family| family != candidate) {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    *observed = Some(candidate);
    Ok(())
}

pub(crate) fn begin_rollback_package_build(
    runtime: MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared: &PreparedRollbackPackage,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    begin_package_build_with_fault(
        runtime,
        capacity,
        prepared.record(),
        TransitionFaultPoint::None,
    )
}

fn begin_package_build_with_fault(
    runtime: MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared_record: &PackageRecord,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    begin_package_build_with_fault_kind(runtime, capacity, prepared_record, false, false, fault)
}

fn begin_package_build_with_fault_kind(
    runtime: MaterializationRuntime,
    capacity: PackageObjectCapacity,
    prepared_record: &PackageRecord,
    acquired_package: bool,
    acquisition_stage: bool,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    validate_runtime(&runtime)?;
    if acquisition_stage {
        require_exact_acquisition_stage(&runtime, prepared_record.tree_index.tree_sha256)?;
    } else {
        require_no_object_stages(&runtime)?;
    }
    if runtime._state.build_intent.is_some() {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    let (preflight_generation, records_parent, trees_parent, record) = capacity
        .into_begin_parts(acquired_package, acquisition_stage)
        .map_err(map_begin_proof_error)?;
    if runtime._state.generation != preflight_generation
        || runtime._records.identity() != records_parent
        || runtime._trees.identity() != trees_parent
        || &record != prepared_record
    {
        return Err(must_seal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    record
        .validate()
        .map_err(MaterializationTransitionError::Clean)?;
    let record_id = record
        .record_id()
        .map_err(MaterializationTransitionError::Clean)?;
    if runtime
        ._state
        .completed_package_record_ids
        .binary_search(&record_id)
        .is_ok()
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    validate_candidate_policy(&runtime, &record)?;

    let successor = next_generation(runtime._state.generation)?;
    let mut next = runtime._state.clone();
    next.generation = successor;
    next.build_intent = Some(MaterializationBuildIntent {
        schema_version: MATERIALIZATION_BUILD_INTENT_SCHEMA_VERSION,
        generation: successor,
        package_record_id: record_id,
        package_record: record,
    });
    next.validate()
        .map_err(MaterializationTransitionError::Clean)?;
    commit_state_transition(runtime, next, fault)
}

/// Durably clears one interrupted build after all writer stages were removed.
///
/// Partially published content-addressed finals remain inert and may be reused
/// by a later exact build. This operation never removes or rewrites a final.
pub(crate) fn abort_package_build(
    runtime: MaterializationRuntime,
    abortable: AbortablePackageBuild,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    abort_package_build_with_fault(runtime, abortable, TransitionFaultPoint::None)
}

pub(super) fn abort_package_build_with_fault(
    runtime: MaterializationRuntime,
    abortable: AbortablePackageBuild,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    validate_runtime(&runtime)?;
    abortable.validate(&runtime).map_err(map_cleanup_error)?;
    if runtime._state.build_intent.is_none() {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    let mut next = runtime._state.clone();
    next.generation = next_generation(runtime._state.generation)?;
    next.build_intent = None;
    next.validate()
        .map_err(MaterializationTransitionError::Clean)?;
    commit_state_transition(runtime, next, fault)
}

/// Completes an ordinary active package without erasing its authority type.
pub(crate) fn complete_active_package(
    runtime: MaterializationRuntime,
    closure: VerifiedActivePackageClosure,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    complete_active_package_with_fault(runtime, closure, TransitionFaultPoint::None)
}

/// Completes an acquired active package without erasing its nominal package
/// representation at the transaction boundary.
#[cfg(feature = "acquired-packages")]
pub(crate) fn complete_acquired_active_package(
    runtime: MaterializationRuntime,
    closure: VerifiedAcquiredActivePackageClosure,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    let (
        generation,
        record_id,
        record,
        _tree_root,
        records_parent,
        trees_parent,
        stages_absent,
        _active_authority,
    ) = closure.into_completion_parts();
    complete_package_build(
        runtime,
        CompletionPlan {
            intent_generation: generation,
            record_id,
            record,
            records_parent,
            trees_parent,
            stages_absent,
        },
        TransitionFaultPoint::None,
    )
}

pub(crate) fn complete_active_package_with_fault(
    runtime: MaterializationRuntime,
    closure: VerifiedActivePackageClosure,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    let (
        generation,
        record_id,
        record,
        _tree_root,
        records_parent,
        trees_parent,
        stages_absent,
        _active_authority,
    ) = closure.into_completion_parts();
    complete_package_build(
        runtime,
        CompletionPlan {
            intent_generation: generation,
            record_id,
            record,
            records_parent,
            trees_parent,
            stages_absent,
        },
        fault,
    )
}

/// Completes an explicitly authorized rollback package without converting its
/// witness into an ordinary active capability.
pub(crate) fn complete_rollback_package(
    runtime: MaterializationRuntime,
    closure: VerifiedRollbackPackageClosure,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    complete_rollback_package_with_fault(runtime, closure, TransitionFaultPoint::None)
}

pub(super) fn complete_rollback_package_with_fault(
    runtime: MaterializationRuntime,
    closure: VerifiedRollbackPackageClosure,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    let (
        generation,
        record_id,
        record,
        _tree_root,
        records_parent,
        trees_parent,
        stages_absent,
        _rollback_authority,
    ) = closure.into_completion_parts();
    complete_package_build(
        runtime,
        CompletionPlan {
            intent_generation: generation,
            record_id,
            record,
            records_parent,
            trees_parent,
            stages_absent,
        },
        fault,
    )
}

struct CompletionPlan {
    intent_generation: u64,
    record_id: Digest32,
    record: PackageRecord,
    records_parent: DirectoryIdentity,
    trees_parent: DirectoryIdentity,
    stages_absent: BuildStagesAbsent,
}

fn complete_package_build(
    runtime: MaterializationRuntime,
    plan: CompletionPlan,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    let CompletionPlan {
        intent_generation,
        record_id,
        record,
        records_parent,
        trees_parent,
        stages_absent,
    } = plan;
    validate_runtime(&runtime)?;
    if runtime._records.identity() != records_parent || runtime._trees.identity() != trees_parent {
        return Err(must_seal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    stages_absent
        .validate(&runtime)
        .map_err(map_cleanup_error)?;
    let intent = runtime
        ._state
        .build_intent
        .as_ref()
        .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    if runtime._state.generation != intent_generation
        || intent.generation != intent_generation
        || intent.package_record_id != record_id
        || intent.package_record != record
        || record.record_id().ok() != Some(record_id)
        || runtime
            ._state
            .completed_package_record_ids
            .binary_search(&record_id)
            .is_ok()
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    validate_candidate_policy(&runtime, &record)?;

    let mut next = runtime._state.clone();
    next.generation = next_generation(runtime._state.generation)?;
    let insertion = next
        .completed_package_record_ids
        .binary_search(&record_id)
        .map_or_else(Ok, |_| {
            Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous))
        })?;
    next.completed_package_record_ids
        .insert(insertion, record_id);
    next.build_intent = None;
    next.validate()
        .map_err(MaterializationTransitionError::Clean)?;
    commit_state_transition(runtime, next, fault)
}

fn validate_candidate_policy(
    runtime: &MaterializationRuntime,
    candidate: &PackageRecord,
) -> Result<(), MaterializationTransitionError> {
    let mut completed = Vec::with_capacity(
        runtime
            ._state
            .completed_package_record_ids
            .len()
            .saturating_add(1),
    );
    for record_id in &runtime._state.completed_package_record_ids {
        completed.push(
            runtime
                ._package_records
                .get(record_id)
                .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?,
        );
    }
    completed.push(candidate);
    validate_package_anchor_consistency(completed.iter().copied())
        .map_err(|_| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    validate_completed_tree_budget(completed)
        .map_err(|_| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))
}

fn validate_runtime(
    runtime: &MaterializationRuntime,
) -> Result<(), MaterializationTransitionError> {
    validate_runtime_projection(runtime)?;
    if runtime._state.gc_intent.is_some() {
        return Err(before_journal(
            ExtensionRepositoryError::GarbageCollectionInProgress,
        ));
    }
    Ok(())
}

fn validate_runtime_projection(
    runtime: &MaterializationRuntime,
) -> Result<(), MaterializationTransitionError> {
    runtime
        ._state
        .validate()
        .map_err(MaterializationTransitionError::Clean)?;
    let canonical = codec::encode(&runtime._state, MAX_MATERIALIZATION_STATE_BYTES)
        .map_err(|_| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    if canonical != runtime._state_bytes
        || runtime._state.build_intent != runtime._build_intent
        || runtime._state.gc_intent != runtime._gc_intent
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    Ok(())
}

fn require_no_object_stages(
    runtime: &MaterializationRuntime,
) -> Result<(), MaterializationTransitionError> {
    if runtime._build_stage.is_some() || !runtime._record_stages.is_empty() {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    for entry in runtime
        ._trees
        .list_components(names::MAX_TREE_ENTRIES)
        .map_err(map_prepublication_fs)?
    {
        let (_, kind) = names::parse_tree_name(entry.as_str())
            .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
        if matches!(kind, TreeNameKind::Stage(_) | TreeNameKind::Acquisition) {
            return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
        }
    }
    for entry in runtime
        ._records
        .list_components(names::MAX_RECORD_ENTRIES)
        .map_err(map_prepublication_fs)?
    {
        let (_, kind) = names::parse_record_name(entry.as_str())
            .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
        if matches!(
            kind,
            RecordNameKind::Package { stage: true }
                | RecordNameKind::CatalogSet { stage: true }
                | RecordNameKind::TreeIndex { stage: true }
                | RecordNameKind::Legal { stage: true }
        ) {
            return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
        }
    }
    Ok(())
}

fn require_exact_acquisition_stage(
    runtime: &MaterializationRuntime,
    expected_tree: Digest32,
) -> Result<(), MaterializationTransitionError> {
    if runtime._build_stage.is_some()
        || runtime._acquisition_stage.is_none()
        || !runtime._record_stages.is_empty()
    {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    let mut observed = false;
    for entry in runtime
        ._trees
        .list_components(names::MAX_TREE_ENTRIES)
        .map_err(map_prepublication_fs)?
    {
        let (digest, kind) = names::parse_tree_name(entry.as_str())
            .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
        match kind {
            TreeNameKind::Acquisition if digest == expected_tree && !observed => observed = true,
            TreeNameKind::Acquisition | TreeNameKind::Stage(_) => {
                return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
            }
            TreeNameKind::Object | TreeNameKind::Retired(_) => {}
        }
    }
    if !observed {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    for entry in runtime
        ._records
        .list_components(names::MAX_RECORD_ENTRIES)
        .map_err(map_prepublication_fs)?
    {
        let (_, kind) = names::parse_record_name(entry.as_str())
            .ok_or_else(|| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
        if kind.is_stage() {
            return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
        }
    }
    Ok(())
}

fn next_generation(current: u64) -> Result<u64, MaterializationTransitionError> {
    next_durable_generation(current)
        .ok_or_else(|| before_journal(ExtensionRepositoryError::GenerationExhausted))
}

fn commit_state_transition(
    runtime: MaterializationRuntime,
    next_state: MaterializationState,
    fault: TransitionFaultPoint,
) -> Result<MaterializationTransitionCommitted, MaterializationTransitionError> {
    let prepared = prepare_transition(&runtime._state, &runtime._state_bytes, next_state)?;

    require_empty_journal_inventory(&runtime)?;
    let journal_stage_identity = write_journal_stage(&runtime, &prepared)?;
    verify_journal_stage(&runtime, &prepared)?;
    fail_before_publication(fault, TransitionFaultPoint::AfterJournalStage)?;

    // From this call onward, the final journal may be durable. No error can
    // truthfully authorize continued use of this repository instance.
    let published_identity = runtime
        ._journals
        .publish_noreplace_verified_regular(&prepared.stage_name, &prepared.journal_name)
        .map_err(|_| must_seal(ExtensionRepositoryError::SettlementAmbiguous))?;
    if published_identity != journal_stage_identity {
        return Err(must_seal(ExtensionRepositoryError::SettlementAmbiguous));
    }
    verify_published_journal(&runtime, &prepared)
        .map_err(|_| must_seal(ExtensionRepositoryError::SettlementAmbiguous))?;
    fail_after_publication(fault, TransitionFaultPoint::AfterJournalPublication)?;

    atomic_write_control(
        &runtime._root,
        &names::state_file(),
        &names::state_stage(),
        &prepared.next_state_bytes,
        MAX_MATERIALIZATION_STATE_BYTES,
    )
    .map_err(|_| must_seal(ExtensionRepositoryError::SettlementAmbiguous))?;
    fail_after_publication(fault, TransitionFaultPoint::AfterState)?;

    write_checkpoint(
        &runtime._root,
        MaterializationCheckpoint::new(prepared.next_state.generation, prepared.next_state_sha256),
    )
    .map_err(|_| must_seal(ExtensionRepositoryError::SettlementAmbiguous))?;
    fail_after_publication(fault, TransitionFaultPoint::AfterCheckpoint)?;

    remove_required_control(&runtime._journals, &prepared.journal_name)
        .map_err(|_| must_seal(ExtensionRepositoryError::SettlementAmbiguous))?;
    fail_after_publication(fault, TransitionFaultPoint::AfterJournalRetirement)?;
    if !runtime
        ._journals
        .list_components(names::MAX_MATERIALIZATION_JOURNAL_ENTRIES)
        .map_err(|_| must_seal(ExtensionRepositoryError::SettlementAmbiguous))?
        .is_empty()
    {
        return Err(must_seal(ExtensionRepositoryError::SettlementAmbiguous));
    }

    Ok(MaterializationTransitionCommitted {
        _generation: prepared.next_state.generation,
        _seal: TransitionCommitSeal,
    })
}

#[derive(Debug)]
struct PreparedTransition {
    next_state: MaterializationState,
    next_state_bytes: Vec<u8>,
    next_state_sha256: Digest32,
    journal_bytes: Vec<u8>,
    stage_name: zephium_private_fs::PrivateComponent,
    journal_name: zephium_private_fs::PrivateComponent,
}

fn prepare_transition(
    current_state: &MaterializationState,
    current_state_bytes: &[u8],
    next_state: MaterializationState,
) -> Result<PreparedTransition, MaterializationTransitionError> {
    current_state
        .validate()
        .map_err(MaterializationTransitionError::Clean)?;
    let canonical_current = codec::encode(current_state, MAX_MATERIALIZATION_STATE_BYTES)
        .map_err(|_| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    if canonical_current != current_state_bytes {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    next_state
        .validate()
        .map_err(MaterializationTransitionError::Clean)?;
    if next_state.generation != next_generation(current_state.generation)? {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }

    let next_state_bytes = codec::encode(&next_state, MAX_MATERIALIZATION_STATE_BYTES)
        .map_err(|_| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    let next_state_sha256 = codec::digest(&next_state_bytes);
    let journal = MaterializationJournal {
        schema_version: MATERIALIZATION_JOURNAL_SCHEMA_VERSION,
        generation: next_state.generation,
        previous_state_sha256: codec::digest(current_state_bytes),
        next_state_sha256,
        next_state: next_state.clone(),
    };
    journal
        .validate()
        .map_err(MaterializationTransitionError::Clean)?;
    let journal_bytes = codec::encode(&journal, MAX_MATERIALIZATION_JOURNAL_BYTES)
        .map_err(|_| before_journal(ExtensionRepositoryError::RecoveryAmbiguous))?;
    let journal_sha256 = codec::digest(&journal_bytes);
    let stage_name = names::journal_stage(next_state.generation, journal_sha256)
        .map_err(MaterializationTransitionError::Clean)?;
    let journal_name = names::journal_file(next_state.generation, journal_sha256)
        .map_err(MaterializationTransitionError::Clean)?;

    Ok(PreparedTransition {
        next_state,
        next_state_bytes,
        next_state_sha256,
        journal_bytes,
        stage_name,
        journal_name,
    })
}

fn require_empty_journal_inventory(
    runtime: &MaterializationRuntime,
) -> Result<(), MaterializationTransitionError> {
    let entries = runtime
        ._journals
        .list_components(names::MAX_MATERIALIZATION_JOURNAL_ENTRIES)
        .map_err(map_prepublication_fs)?;
    if !entries.is_empty() {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    Ok(())
}

fn write_journal_stage(
    runtime: &MaterializationRuntime,
    prepared: &PreparedTransition,
) -> Result<FileIdentity, MaterializationTransitionError> {
    runtime
        ._journals
        .write_new_synced(
            &prepared.stage_name,
            &prepared.journal_bytes,
            ByteLimit::new(MAX_MATERIALIZATION_JOURNAL_BYTES).map_err(map_prepublication_fs)?,
        )
        .map_err(map_journal_stage_fs)
}

fn verify_journal_stage(
    runtime: &MaterializationRuntime,
    prepared: &PreparedTransition,
) -> Result<(), MaterializationTransitionError> {
    let stored = read_required_control(
        &runtime._journals,
        &prepared.stage_name,
        MAX_MATERIALIZATION_JOURNAL_BYTES,
    )
    .map_err(|error| match error {
        ExtensionRepositoryError::FileSystem(error) if fs_error_is_terminal(error) => {
            must_seal(ExtensionRepositoryError::FileSystem(error))
        }
        other => before_journal(other),
    })?;
    if stored != prepared.journal_bytes {
        return Err(before_journal(ExtensionRepositoryError::RecoveryAmbiguous));
    }
    Ok(())
}

fn verify_published_journal(
    runtime: &MaterializationRuntime,
    prepared: &PreparedTransition,
) -> Result<(), ExtensionRepositoryError> {
    let stored = read_required_control(
        &runtime._journals,
        &prepared.journal_name,
        MAX_MATERIALIZATION_JOURNAL_BYTES,
    )?;
    if stored != prepared.journal_bytes {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

fn fail_before_publication(
    configured: TransitionFaultPoint,
    reached: TransitionFaultPoint,
) -> Result<(), MaterializationTransitionError> {
    #[cfg(test)]
    if configured == reached {
        return Err(must_seal(ExtensionRepositoryError::InjectedCrash));
    }
    #[cfg(not(test))]
    let _ = (configured, reached);
    Ok(())
}

fn fail_after_publication(
    configured: TransitionFaultPoint,
    reached: TransitionFaultPoint,
) -> Result<(), MaterializationTransitionError> {
    #[cfg(test)]
    if configured == reached {
        return Err(must_seal(ExtensionRepositoryError::InjectedCrash));
    }
    #[cfg(not(test))]
    let _ = (configured, reached);
    Ok(())
}

const fn before_journal(error: ExtensionRepositoryError) -> MaterializationTransitionError {
    MaterializationTransitionError::Clean(error)
}

const fn must_seal(error: ExtensionRepositoryError) -> MaterializationTransitionError {
    MaterializationTransitionError::MustSeal(error)
}

fn map_prepublication_fs(error: PrivateFsError) -> MaterializationTransitionError {
    if fs_error_is_terminal(error) {
        must_seal(ExtensionRepositoryError::FileSystem(error))
    } else {
        before_journal(match error {
            PrivateFsError::AlreadyExists
            | PrivateFsError::BoundExceeded
            | PrivateFsError::Unsafe => ExtensionRepositoryError::RecoveryAmbiguous,
            other => ExtensionRepositoryError::FileSystem(other),
        })
    }
}

fn map_journal_stage_fs(error: PrivateFsError) -> MaterializationTransitionError {
    // A terminal create/write result may have left durable stage residue and a
    // quarantined lease. Consuming the runtime plus this category forces a
    // fresh recovery instead of relying on every caller to notice the subtype.
    map_prepublication_fs(error)
}

fn map_begin_proof_error(_error: PackageObjectError) -> MaterializationTransitionError {
    // A capacity token is produced by an immediately preceding bounded
    // preflight. Any disagreement at this private handoff is stale-runtime or
    // programmer corruption, never a user/source failure.
    must_seal(ExtensionRepositoryError::RecoveryAmbiguous)
}

fn map_cleanup_error(error: CleanupError) -> MaterializationTransitionError {
    match error {
        CleanupError::BuildStateMismatch
        | CleanupError::ExactMismatch
        | CleanupError::CommitMarkerPresent => {
            must_seal(ExtensionRepositoryError::RecoveryAmbiguous)
        }
        CleanupError::SettlementAmbiguous => {
            must_seal(ExtensionRepositoryError::SettlementAmbiguous)
        }
        CleanupError::Filesystem(error) if fs_error_is_terminal(error) => {
            must_seal(ExtensionRepositoryError::FileSystem(error))
        }
        CleanupError::Filesystem(error) => {
            before_journal(ExtensionRepositoryError::FileSystem(error))
        }
    }
}

const fn fs_error_is_terminal(error: PrivateFsError) -> bool {
    matches!(
        error,
        PrivateFsError::IdentityAmbiguous
            | PrivateFsError::SettlementUnknown
            | PrivateFsError::Quarantined
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materialization::state::MAX_DURABLE_GENERATION;

    fn encoded(state: &MaterializationState) -> Vec<u8> {
        codec::encode(state, MAX_MATERIALIZATION_STATE_BYTES).unwrap()
    }

    #[test]
    fn preparation_binds_one_exact_successor_and_both_state_digests() {
        let current = MaterializationState::default();
        let current_bytes = encoded(&current);
        let next = MaterializationState {
            generation: 1,
            ..MaterializationState::default()
        };

        let prepared = prepare_transition(&current, &current_bytes, next.clone()).unwrap();
        let journal: MaterializationJournal = codec::decode_materialization(
            &prepared.journal_bytes,
            MAX_MATERIALIZATION_JOURNAL_BYTES,
        )
        .unwrap();
        assert_eq!(prepared.next_state, next);
        assert_eq!(journal.previous_state_sha256, codec::digest(&current_bytes));
        assert_eq!(
            journal.next_state_sha256,
            codec::digest(&prepared.next_state_bytes)
        );
        assert_eq!(journal.next_state, next);
    }

    #[test]
    fn preparation_rejects_stale_runtime_bytes_and_non_successors() {
        let current = MaterializationState::default();
        let stale_bytes = encoded(&MaterializationState {
            generation: 1,
            ..MaterializationState::default()
        });
        let successor = MaterializationState {
            generation: 1,
            ..MaterializationState::default()
        };
        assert_eq!(
            prepare_transition(&current, &stale_bytes, successor).unwrap_err(),
            before_journal(ExtensionRepositoryError::RecoveryAmbiguous)
        );

        let skipped = MaterializationState {
            generation: 2,
            ..MaterializationState::default()
        };
        assert_eq!(
            prepare_transition(&current, &encoded(&current), skipped).unwrap_err(),
            before_journal(ExtensionRepositoryError::RecoveryAmbiguous)
        );
    }

    #[test]
    fn generation_exhaustion_is_a_clean_prepublication_failure() {
        let current = MaterializationState {
            generation: MAX_DURABLE_GENERATION,
            ..MaterializationState::default()
        };
        let next = current.clone();
        assert_eq!(
            prepare_transition(&current, &encoded(&current), next).unwrap_err(),
            before_journal(ExtensionRepositoryError::GenerationExhausted)
        );
    }

    #[test]
    fn terminal_stage_failures_are_structurally_ambiguous() {
        for error in [
            PrivateFsError::IdentityAmbiguous,
            PrivateFsError::SettlementUnknown,
            PrivateFsError::Quarantined,
        ] {
            assert_eq!(
                map_journal_stage_fs(error),
                must_seal(ExtensionRepositoryError::FileSystem(error))
            );
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    mod native {
        use std::collections::BTreeSet;
        use std::fs;
        use std::os::unix::fs::PermissionsExt as _;

        use tempfile::TempDir;
        use zephium_private_fs::{LockedPrivateNamespace, PrivateDirectory};

        use super::*;
        use crate::materialization::names::{
            self, MAX_MATERIALIZATION_JOURNAL_ENTRIES, MAX_MATERIALIZATION_ROOT_ENTRIES,
            MAX_RECORD_ENTRIES, MAX_TREE_ENTRIES,
        };
        use crate::materialization::objects::{
            preflight_package_object_capacity, PackageObjectIntentDisposition,
        };
        use crate::materialization::records::tests::package_record_fixture;
        use crate::materialization::recovery::{
            open_or_recover_test_fixture, FaultPoint as RecoveryFaultPoint,
        };
        use crate::materialization::state::MAX_MATERIALIZATION_CHECKPOINT_BYTES;
        use crate::materialization::storage::read_required_control;

        struct NativeHarness {
            namespace: Option<LockedPrivateNamespace>,
            temporary: TempDir,
        }

        impl NativeHarness {
            fn new() -> (Self, MaterializationRuntime) {
                #[cfg(target_os = "macos")]
                let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
                #[cfg(target_os = "linux")]
                let temporary = tempfile::tempdir_in("/tmp").unwrap();
                fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
                let namespace =
                    LockedPrivateNamespace::open_or_create(temporary.path().join("repository"))
                        .unwrap();
                let runtime = open_or_recover_test_fixture(
                    namespace.directory(),
                    false,
                    RecoveryFaultPoint::None,
                )
                .unwrap();
                (
                    Self {
                        namespace: Some(namespace),
                        temporary,
                    },
                    runtime,
                )
            }

            fn repository_root(&self) -> &PrivateDirectory {
                self.namespace
                    .as_ref()
                    .expect("native test namespace remains open")
                    .directory()
            }

            fn recover(&self) -> MaterializationRuntime {
                open_or_recover_test_fixture(self.repository_root(), true, RecoveryFaultPoint::None)
                    .unwrap()
            }
        }

        impl Drop for NativeHarness {
            fn drop(&mut self) {
                drop(self.namespace.take());
                make_fixture_tree_removable(self.temporary.path());
            }
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        struct DurableFrontier {
            state_generation: u64,
            checkpoint_generation: u64,
            intent_generation: Option<u64>,
            journal_stages: usize,
            journal_finals: usize,
        }

        fn observe_frontier(harness: &NativeHarness) -> DurableFrontier {
            let root = harness
                .repository_root()
                .open_private_child(&names::materialization_directory())
                .unwrap();
            assert!(!root.regular_exists(&names::state_stage()).unwrap());
            assert!(!root.regular_exists(&names::checkpoint_stage()).unwrap());

            let state_bytes =
                read_required_control(&root, &names::state_file(), MAX_MATERIALIZATION_STATE_BYTES)
                    .unwrap();
            let state: MaterializationState =
                codec::decode_materialization(&state_bytes, MAX_MATERIALIZATION_STATE_BYTES)
                    .unwrap();
            assert_eq!(state_bytes, encoded(&state));

            let checkpoint_bytes = read_required_control(
                &root,
                &names::checkpoint_file(),
                MAX_MATERIALIZATION_CHECKPOINT_BYTES,
            )
            .unwrap();
            let checkpoint: MaterializationCheckpoint = codec::decode_materialization(
                &checkpoint_bytes,
                MAX_MATERIALIZATION_CHECKPOINT_BYTES,
            )
            .unwrap();

            let trees = root.open_private_child(&names::trees_directory()).unwrap();
            let records = root
                .open_private_child(&names::records_directory())
                .unwrap();
            let journals = root
                .open_private_child(&names::journals_directory())
                .unwrap();
            assert!(trees.list_components(MAX_TREE_ENTRIES).unwrap().is_empty());
            assert!(records
                .list_components(MAX_RECORD_ENTRIES)
                .unwrap()
                .is_empty());

            let mut journal_stages = 0_usize;
            let mut journal_finals = 0_usize;
            for entry in journals
                .list_components(MAX_MATERIALIZATION_JOURNAL_ENTRIES)
                .unwrap()
            {
                let (_, _, stage) = names::parse_journal_name(entry.as_str()).unwrap();
                if stage {
                    journal_stages += 1;
                } else {
                    journal_finals += 1;
                }
            }

            DurableFrontier {
                state_generation: state.generation,
                checkpoint_generation: checkpoint.generation,
                intent_generation: state.build_intent.as_ref().map(|intent| intent.generation),
                journal_stages,
                journal_finals,
            }
        }

        fn assert_recovered_shape(
            runtime: &MaterializationRuntime,
            record: &PackageRecord,
            generation: u64,
            intent_generation: Option<u64>,
            disposition: PackageObjectIntentDisposition,
        ) {
            assert_eq!(runtime._state.generation, generation);
            assert_eq!(runtime._state_bytes, encoded(&runtime._state));
            assert_eq!(runtime._state.build_intent, runtime._build_intent);
            match (runtime._build_intent.as_ref(), intent_generation) {
                (None, None) => {}
                (Some(intent), Some(expected_generation)) => {
                    assert_eq!(intent.generation, expected_generation);
                    assert_eq!(intent.package_record, *record);
                    assert_eq!(intent.package_record_id, record.record_id().unwrap());
                }
                (observed, expected) => {
                    panic!("intent mismatch: observed={observed:?}, expected={expected:?}")
                }
            }
            assert!(runtime
                ._journals
                .list_components(MAX_MATERIALIZATION_JOURNAL_ENTRIES)
                .unwrap()
                .is_empty());
            assert!(runtime
                ._trees
                .list_components(MAX_TREE_ENTRIES)
                .unwrap()
                .is_empty());
            assert!(runtime
                ._records
                .list_components(MAX_RECORD_ENTRIES)
                .unwrap()
                .is_empty());
            assert!(runtime._build_stage.is_none());
            assert!(runtime._record_stages.is_empty());
            assert!(!runtime._root.regular_exists(&names::state_stage()).unwrap());
            assert!(!runtime
                ._root
                .regular_exists(&names::checkpoint_stage())
                .unwrap());

            let root_entries = runtime
                ._root
                .list_components(MAX_MATERIALIZATION_ROOT_ENTRIES)
                .unwrap()
                .into_iter()
                .map(|entry| entry.as_str().to_owned())
                .collect::<BTreeSet<_>>();
            assert_eq!(
                root_entries,
                BTreeSet::from([
                    "journals".to_owned(),
                    "records".to_owned(),
                    "recovery-checkpoint.json".to_owned(),
                    "state.json".to_owned(),
                    "trees".to_owned(),
                ])
            );
            assert_eq!(
                preflight_package_object_capacity(runtime, record)
                    .unwrap()
                    .intent_disposition(),
                disposition
            );
        }

        fn injected_crash(
            result: Result<MaterializationTransitionCommitted, MaterializationTransitionError>,
        ) {
            match result {
                Err(MaterializationTransitionError::MustSeal(
                    ExtensionRepositoryError::InjectedCrash,
                )) => {}
                Err(error) => panic!("unexpected transition error: {error:?}"),
                Ok(_) => panic!("fault boundary unexpectedly committed without interruption"),
            }
        }

        #[test]
        fn begin_transition_recovers_every_native_crash_frontier() {
            let record = package_record_fixture(41);
            let cases = [
                (
                    TransitionFaultPoint::AfterJournalStage,
                    DurableFrontier {
                        state_generation: 0,
                        checkpoint_generation: 0,
                        intent_generation: None,
                        journal_stages: 1,
                        journal_finals: 0,
                    },
                    0,
                    None,
                    PackageObjectIntentDisposition::RequiresCommit,
                ),
                (
                    TransitionFaultPoint::AfterJournalPublication,
                    DurableFrontier {
                        state_generation: 0,
                        checkpoint_generation: 0,
                        intent_generation: None,
                        journal_stages: 0,
                        journal_finals: 1,
                    },
                    1,
                    Some(1),
                    PackageObjectIntentDisposition::AlreadyCommitted,
                ),
                (
                    TransitionFaultPoint::AfterState,
                    DurableFrontier {
                        state_generation: 1,
                        checkpoint_generation: 0,
                        intent_generation: Some(1),
                        journal_stages: 0,
                        journal_finals: 1,
                    },
                    1,
                    Some(1),
                    PackageObjectIntentDisposition::AlreadyCommitted,
                ),
                (
                    TransitionFaultPoint::AfterCheckpoint,
                    DurableFrontier {
                        state_generation: 1,
                        checkpoint_generation: 1,
                        intent_generation: Some(1),
                        journal_stages: 0,
                        journal_finals: 1,
                    },
                    1,
                    Some(1),
                    PackageObjectIntentDisposition::AlreadyCommitted,
                ),
                (
                    TransitionFaultPoint::AfterJournalRetirement,
                    DurableFrontier {
                        state_generation: 1,
                        checkpoint_generation: 1,
                        intent_generation: Some(1),
                        journal_stages: 0,
                        journal_finals: 0,
                    },
                    1,
                    Some(1),
                    PackageObjectIntentDisposition::AlreadyCommitted,
                ),
            ];

            for (fault, expected_frontier, generation, intent, disposition) in cases {
                let (harness, runtime) = NativeHarness::new();
                let capacity = preflight_package_object_capacity(&runtime, &record).unwrap();
                injected_crash(begin_package_build_with_fault(
                    runtime, capacity, &record, fault,
                ));
                assert_eq!(observe_frontier(&harness), expected_frontier);

                let recovered = harness.recover();
                assert_recovered_shape(&recovered, &record, generation, intent, disposition);
                drop(recovered);
            }
        }

        #[cfg(feature = "acquired-packages")]
        #[test]
        fn acquired_begin_transition_preserves_only_its_exact_stage() {
            use std::sync::Arc;

            use crate::materialization::objects::preflight_acquired_package_object_capacity;
            use crate::materialization::records::StoredPayloadIdentity;
            use crate::materialization::runtime::MaterializationTreeCapability;

            let (harness, mut runtime) = NativeHarness::new();
            let mut record = package_record_fixture(49);
            record.package.payload = StoredPayloadIdentity::AcquiredZip {
                length: 19,
                sha256: Digest32::from_bytes([49; 32]),
            };
            let stage_name = names::tree_acquisition_stage(record.tree_index.tree_sha256);
            let stage = runtime
                ._trees
                .create_new_private_child(&stage_name)
                .unwrap();
            let sealed = stage.seal().unwrap();
            runtime._acquisition_stage = Some(MaterializationTreeCapability::Sealed {
                _directory: Arc::new(sealed),
            });
            let capacity =
                preflight_acquired_package_object_capacity(&runtime, &record, true).unwrap();

            let _ = begin_package_build_with_fault_kind(
                runtime,
                capacity,
                &record,
                true,
                true,
                TransitionFaultPoint::None,
            )
            .unwrap();

            let mut recovered = harness.recover();
            assert_eq!(recovered._state.generation, 1);
            assert_eq!(
                recovered._build_intent.as_ref().unwrap().package_record,
                record
            );
            assert!(recovered._build_stage.is_none());
            assert!(recovered._acquisition_stage.is_some());
            assert!(recovered
                ._trees
                .open_sealed_private_child(&stage_name)
                .is_ok());

            let PackageBuildCommitMarker::Absent(marker_absent) =
                inspect_package_build_commit_marker(&recovered).unwrap()
            else {
                panic!("acquired fixture unexpectedly contains its commit marker");
            };
            let abortable =
                reconcile_build_stages_for_abort(&mut recovered, marker_absent).unwrap();
            let _ = abort_package_build(recovered, abortable).unwrap();
            let settled = harness.recover();
            assert!(settled._build_intent.is_none());
            assert!(settled._acquisition_stage.is_none());
            assert_eq!(
                settled
                    ._trees
                    .open_private_child_any_mode(&stage_name)
                    .err(),
                Some(PrivateFsError::NotFound)
            );
        }

        #[test]
        fn abort_transition_recovers_every_native_crash_frontier() {
            let record = package_record_fixture(51);
            let cases = [
                (
                    TransitionFaultPoint::AfterJournalStage,
                    DurableFrontier {
                        state_generation: 1,
                        checkpoint_generation: 1,
                        intent_generation: Some(1),
                        journal_stages: 1,
                        journal_finals: 0,
                    },
                    1,
                    Some(1),
                    PackageObjectIntentDisposition::AlreadyCommitted,
                ),
                (
                    TransitionFaultPoint::AfterJournalPublication,
                    DurableFrontier {
                        state_generation: 1,
                        checkpoint_generation: 1,
                        intent_generation: Some(1),
                        journal_stages: 0,
                        journal_finals: 1,
                    },
                    2,
                    None,
                    PackageObjectIntentDisposition::RequiresCommit,
                ),
                (
                    TransitionFaultPoint::AfterState,
                    DurableFrontier {
                        state_generation: 2,
                        checkpoint_generation: 1,
                        intent_generation: None,
                        journal_stages: 0,
                        journal_finals: 1,
                    },
                    2,
                    None,
                    PackageObjectIntentDisposition::RequiresCommit,
                ),
                (
                    TransitionFaultPoint::AfterCheckpoint,
                    DurableFrontier {
                        state_generation: 2,
                        checkpoint_generation: 2,
                        intent_generation: None,
                        journal_stages: 0,
                        journal_finals: 1,
                    },
                    2,
                    None,
                    PackageObjectIntentDisposition::RequiresCommit,
                ),
                (
                    TransitionFaultPoint::AfterJournalRetirement,
                    DurableFrontier {
                        state_generation: 2,
                        checkpoint_generation: 2,
                        intent_generation: None,
                        journal_stages: 0,
                        journal_finals: 0,
                    },
                    2,
                    None,
                    PackageObjectIntentDisposition::RequiresCommit,
                ),
            ];

            for (fault, expected_frontier, generation, intent, disposition) in cases {
                let (harness, initial) = NativeHarness::new();
                let capacity = preflight_package_object_capacity(&initial, &record).unwrap();
                let committed = begin_package_build_with_fault(
                    initial,
                    capacity,
                    &record,
                    TransitionFaultPoint::None,
                )
                .unwrap();
                drop(committed);

                let mut durable_intent = harness.recover();
                assert_recovered_shape(
                    &durable_intent,
                    &record,
                    1,
                    Some(1),
                    PackageObjectIntentDisposition::AlreadyCommitted,
                );
                let PackageBuildCommitMarker::Absent(marker_absent) =
                    inspect_package_build_commit_marker(&durable_intent).unwrap()
                else {
                    panic!("abort fixture unexpectedly contains a commit marker");
                };
                let abortable =
                    reconcile_build_stages_for_abort(&mut durable_intent, marker_absent).unwrap();
                injected_crash(abort_package_build_with_fault(
                    durable_intent,
                    abortable,
                    fault,
                ));
                assert_eq!(observe_frontier(&harness), expected_frontier);

                let recovered = harness.recover();
                assert_recovered_shape(&recovered, &record, generation, intent, disposition);
                drop(recovered);
            }
        }

        fn make_fixture_tree_removable(path: &std::path::Path) {
            let Ok(metadata) = fs::symlink_metadata(path) else {
                return;
            };
            if metadata.is_dir() {
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
                if let Ok(entries) = fs::read_dir(path) {
                    for entry in entries.flatten() {
                        make_fixture_tree_removable(&entry.path());
                    }
                }
            } else if !metadata.file_type().is_symlink() {
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
            }
        }
    }
}
