//! Bounded worker-owned runtime slots.

use std::mem::size_of;

use zephium_core::extensions::{
    ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipIdentity,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPhase,
    ExtensionRuntimeEligibility, ExtensionRuntimeGeneration,
};
use zephium_extension_runtime_api::{
    ExtensionPackageAccess, ExtensionRuntimeAbsenceEvidence, ExtensionRuntimeActivationRequest,
    ExtensionRuntimeFailure, ExtensionRuntimeOwner, ExtensionRuntimePendingPublication,
    ExtensionRuntimePublicationGrantRebindRefusal, ExtensionRuntimePublicationReceipt,
    ExtensionRuntimePublicationReclaimRefusal, ExtensionRuntimePublicationRefusal,
    ExtensionRuntimePublicationRequest, ExtensionRuntimeRetirementRequest,
    ExtensionRuntimeUncertainOwner,
};

use crate::journal_store::{
    JournalActivationConflict, JournalActivationDiverged, JournalActivationReloadRequired,
    RowFenceConflict, RowFenceDiverged, RowFenceReloadRequired,
    MAX_JOURNAL_ACTIVATION_AMBIGUITY_ADDITIONAL_RETAINED_BYTES,
    MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES,
};
use crate::repository::{
    ServiceRuntimeAcquisitionError, ServiceRuntimeAcquisitionPlan,
    ServiceRuntimeHostActivationBindingRefusal, ServiceRuntimeLease, ServiceRuntimePackageAccess,
    ServiceRuntimePackageAccessBuildRefusal, ServiceRuntimePackageAccessReleaseRefusal,
    ServiceRuntimeRecovery, ServiceRuntimeRejoinRefusal, ServiceRuntimeRelease,
};

use super::outcome::{RuntimeActivationRejectionReason, RuntimeCoordinatorFailureReason};

const HEAP_ALLOCATION_OVERHEAD_BYTES: usize = 2 * size_of::<usize>();

const fn max4(first: usize, second: usize, third: usize, fourth: usize) -> usize {
    let first_pair = if first > second { first } else { second };
    let second_pair = if third > fourth { third } else { fourth };
    if first_pair > second_pair {
        first_pair
    } else {
        second_pair
    }
}

const fn max6(
    first: usize,
    second: usize,
    third: usize,
    fourth: usize,
    fifth: usize,
    sixth: usize,
) -> usize {
    let first_four = max4(first, second, third, fourth);
    let last_two = max2(fifth, sixth);
    if first_four > last_two {
        first_four
    } else {
        last_two
    }
}

const fn max2(first: usize, second: usize) -> usize {
    if first > second {
        first
    } else {
        second
    }
}

/// Conservative coordinator-owned charge retained beside the host's already
/// admitted lifecycle, publication, and repository-recovery authorities.
///
/// This intentionally double-counts the largest control struct's inline
/// authority fields. The small fixed over-reservation buys a stable bound
/// across every resumable state, both coordinator-owned box allocations, and
/// the largest post-call row fence without relying on enum niche layout.
pub(super) const MAX_COORDINATOR_HOST_COMPANION_RETAINED_BYTES: usize = size_of::<RuntimeSlot>()
    + max6(
        size_of::<NativeActivationState>(),
        size_of::<NativeUncertainState>(),
        size_of::<OwnedRuntimeState>(),
        size_of::<ReleaseRuntimeState>() + size_of::<PostHostReleaseAuthority>(),
        size_of::<NativeRetirementState>(),
        size_of::<NativeRetirementUncertainState>(),
    )
    + max2(
        size_of::<RuntimeRowFenceConflictState>(),
        size_of::<RuntimeRowFenceAmbiguousState>(),
    )
    + max4(
        size_of::<PublicationFailStopAuthority>(),
        size_of::<PublicationReclaimFailStopAuthority>(),
        size_of::<NativeCallPanicAuthority>(),
        size_of::<NativeRetirementCallPanicAuthority>(),
    )
    + max2(
        size_of::<PublicationCallPanicAuthority>(),
        size_of::<PublicationReclaimCallPanicAuthority>(),
    )
    + max2(
        size_of::<GrantRebindFailStopAuthority>(),
        size_of::<GrantRebindCallPanicAuthority>(),
    )
    + (2 * HEAP_ALLOCATION_OVERHEAD_BYTES)
    + MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES
    + zephium_extension_repository::MAX_BUNDLED_RUNTIME_PRE_HOST_REFUSAL_ADDITIONAL_RETAINED_BYTES
    + crate::repository::MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES;

/// Fixed coordinator state retained beside raw repository authority before
/// host assembly. Raw planning separately admits the larger of the actual plan
/// and its exact projected role-specific lease; raw access construction admits
/// its actual delegated provider. This companion covers the slot, the largest
/// resumable wrapper, both journal ambiguity families, and all outer Boxes.
pub(super) const MAX_COORDINATOR_PRE_HOST_COMPANION_RETAINED_BYTES: usize =
    size_of::<RuntimeSlot>()
        + max6(
            size_of::<AcquireLeaseState>(),
            size_of::<PreparingLeaseState>(),
            size_of::<PackageAccessBuildRefusalState>(),
            size_of::<PreparingAccessState>(),
            size_of::<MayOwnConflictState>(),
            size_of::<MayOwnAccessState>(),
        )
        + max4(
            size_of::<ReleaseRuntimeState>(),
            size_of::<RuntimeRowFenceOperation>(),
            size_of::<RuntimeRowFenceConflictState>(),
            size_of::<RuntimeRowFenceAmbiguousState>(),
        )
        + MAX_JOURNAL_ACTIVATION_AMBIGUITY_ADDITIONAL_RETAINED_BYTES
        + MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES
        + (4 * HEAP_ALLOCATION_OVERHEAD_BYTES);

pub(super) struct RuntimeSlot {
    key: ExtensionNativeOwnershipKey,
    generation: Option<ExtensionRuntimeGeneration>,
    state: Option<RuntimeSlotState>,
}

pub(super) enum RuntimeSlotState {
    Planning,
    Planned(Box<ServiceRuntimeAcquisitionPlan>),
    BeginConflict(Box<JournalActivationConflict<ServiceRuntimeAcquisitionPlan>>),
    BeginAmbiguous(Box<JournalActivationReloadRequired<ServiceRuntimeAcquisitionPlan>>),
    AcquireLease(Box<AcquireLeaseState>),
    PreparingLease(Box<PreparingLeaseState>),
    PackageAccessBuildRefusal(Box<PackageAccessBuildRefusalState>),
    PreparingAccess(Box<PreparingAccessState>),
    MayOwnConflict(Box<MayOwnConflictState>),
    MayOwnAmbiguous(Box<JournalActivationReloadRequired<ServiceRuntimePackageAccess>>),
    MayOwnAccess(Box<MayOwnAccessState>),
    HostBindingRefusal(Box<ServiceRuntimeHostActivationBindingRefusal>),
    NativeActivation(Box<NativeActivationState>),
    NativeUncertain(Box<NativeUncertainState>),
    NativeRetirement(Box<NativeRetirementState>),
    NativeRetirementUncertain(Box<NativeRetirementUncertainState>),
    Owned(Box<OwnedRuntimeState>),
    Release(Box<ReleaseRuntimeState>),
    GrantRebindPending(Box<RuntimeRowFenceOperation>),
    RowFenceConflict(Box<RuntimeRowFenceConflictState>),
    RowFenceAmbiguous(Box<RuntimeRowFenceAmbiguousState>),
    FailStop(Box<RuntimeFailStopState>),
}

pub(super) enum RuntimeRowFenceMutation {
    Transition {
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
    },
    AttachNativeIdentity(ExtensionNativeOwnershipIdentity),
    RebindGrants(Box<ExtensionRuntimeEligibility>),
    Clear,
}

impl RuntimeRowFenceMutation {
    pub(super) fn additional_retained_bytes(&self) -> usize {
        match self {
            Self::RebindGrants(eligibility) => eligibility
                .retained_bytes()
                .saturating_add(HEAP_ALLOCATION_OVERHEAD_BYTES),
            Self::Transition { .. } | Self::AttachNativeIdentity(_) | Self::Clear => 0,
        }
    }
}

pub(super) enum RuntimeRowFenceAuthority {
    Owned(Box<OwnedRuntimeState>),
    Release(Box<ReleaseRuntimeState>),
}

pub(super) struct RuntimeRowFenceOperation {
    pub(super) mutation: RuntimeRowFenceMutation,
    pub(super) authority: RuntimeRowFenceAuthority,
}

impl RuntimeRowFenceOperation {
    pub(super) const fn is_owned_retirement_transition(&self) -> bool {
        matches!(
            (&self.authority, &self.mutation),
            (
                RuntimeRowFenceAuthority::Owned(_),
                RuntimeRowFenceMutation::Transition {
                    intent: ExtensionNativeOwnershipIntent::Release,
                    phase: ExtensionNativeOwnershipPhase::NativeMayOwn,
                }
            )
        )
    }

    pub(super) const fn is_live_grant_rebind(&self) -> bool {
        matches!(
            (&self.authority, &self.mutation),
            (
                RuntimeRowFenceAuthority::Owned(state),
                RuntimeRowFenceMutation::RebindGrants(_),
            ) if matches!(state.operation, RuntimeOperationControl::Published(_))
        )
    }
}

pub(super) struct RuntimeRowFenceConflictState {
    pub(super) conflict: RowFenceConflict,
    pub(super) operation: RuntimeRowFenceOperation,
}

pub(super) struct RuntimeRowFenceAmbiguousState {
    pub(super) ambiguity: RowFenceReloadRequired,
    pub(super) operation: RuntimeRowFenceOperation,
}

pub(super) struct AcquireLeaseState {
    pub(super) plan: ServiceRuntimeAcquisitionPlan,
    pub(super) preparing: ExtensionNativeOwnershipEntry,
}

pub(super) struct PreparingLeaseState {
    pub(super) lease: ServiceRuntimeLease,
    pub(super) preparing: ExtensionNativeOwnershipEntry,
}

pub(super) struct PackageAccessBuildRefusalState {
    pub(super) refusal: ServiceRuntimePackageAccessBuildRefusal,
    pub(super) preparing: ExtensionNativeOwnershipEntry,
}

pub(super) struct PreparingAccessState {
    pub(super) access: ServiceRuntimePackageAccess,
    pub(super) preparing: ExtensionNativeOwnershipEntry,
}

pub(super) struct MayOwnConflictState {
    pub(super) preparing: ExtensionNativeOwnershipEntry,
    pub(super) conflict: JournalActivationConflict<ServiceRuntimePackageAccess>,
}

pub(super) struct MayOwnAccessState {
    pub(super) access: ServiceRuntimePackageAccess,
    pub(super) entry: ExtensionNativeOwnershipEntry,
}

pub(super) struct NativeActivationState {
    pub(super) initial_entry: ExtensionNativeOwnershipEntry,
    pub(super) request: ExtensionRuntimeActivationRequest,
    /// Present only after an adapter call returned definite absence. `None`
    /// means the activation request has never crossed the native boundary.
    pub(super) absence: Option<ExtensionRuntimeAbsenceEvidence>,
    pub(super) pending: ExtensionRuntimePendingPublication,
    pub(super) recovery: ServiceRuntimeRecovery,
}

pub(super) struct NativeUncertainState {
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
    pub(super) owner: ExtensionRuntimeUncertainOwner,
    /// Latest retryable native failure. Reconciliation replaces this value
    /// whenever the adapter supplies fresher uncertainty evidence.
    pub(super) failure: ExtensionRuntimeFailure,
    pub(super) pending: ExtensionRuntimePendingPublication,
    pub(super) recovery: ServiceRuntimeRecovery,
}

pub(super) struct NativeRetirementState {
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
    pub(super) request: ExtensionRuntimeRetirementRequest,
    pub(super) operation: RuntimeOperationControl,
    pub(super) recovery: ServiceRuntimeRecovery,
    pub(super) completion: ReleaseCompletion,
}

pub(super) struct NativeRetirementUncertainState {
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
    pub(super) owner: ExtensionRuntimeUncertainOwner,
    pub(super) operation: RuntimeOperationControl,
    pub(super) recovery: ServiceRuntimeRecovery,
    pub(super) completion: ReleaseCompletion,
}

pub(super) enum RuntimeOperationControl {
    Pending(Box<ExtensionRuntimePendingPublication>),
    PublicationRequest(Box<ExtensionRuntimePublicationRequest>),
    Published(Box<ExtensionRuntimePublicationReceipt>),
}

pub(super) struct OwnedRuntimeState {
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
    pub(super) owner: ExtensionRuntimeOwner,
    pub(super) operation: RuntimeOperationControl,
    pub(super) recovery: ServiceRuntimeRecovery,
}

pub(super) enum ReleaseAuthority {
    NoRepositoryPin(Option<Box<ServiceRuntimeAcquisitionPlan>>),
    Lease(Box<ServiceRuntimeLease>),
    PreHost(Box<ServiceRuntimePackageAccess>),
    PreHostRefusal(Box<ServiceRuntimePackageAccessReleaseRefusal>),
    PostHost(Box<PostHostReleaseAuthority>),
    Repository(Box<ServiceRuntimeRelease>),
    RejoinRefusal(Box<ServiceRuntimeRejoinRefusal>),
}

pub(super) struct PostHostReleaseAuthority {
    pub(super) access: ExtensionPackageAccess,
    pub(super) operation: RuntimeOperationControl,
    pub(super) recovery: ServiceRuntimeRecovery,
    pub(super) absence: PostHostAbsenceBasis,
}

// Absence evidence is deliberately inline and Copy. This state already lives
// inside a bounded boxed release authority; another allocation would add a
// fallible teardown edge without reducing any steady-state live-runtime slot.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy)]
pub(super) enum PostHostAbsenceBasis {
    /// The bound activation request never crossed the lifecycle port. This is
    /// not post-native evidence and may only accompany an unpublished pending
    /// operation.
    ActivationUnentered,
    /// Exact evidence returned after a lifecycle-port call.
    Proven(ExtensionRuntimeAbsenceEvidence),
}

pub(super) struct ReleaseRuntimeState {
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
    pub(super) authority: ReleaseAuthority,
    pub(super) completion: ReleaseCompletion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReleaseCompletion {
    Retired,
    ActivationRejected(RuntimeActivationRejectionReason),
    ActivationUnavailable(super::outcome::RuntimeActivationUnavailableReason),
}

pub(super) struct RuntimeFailStopState {
    pub(super) reason: RuntimeCoordinatorFailureReason,
    pub(super) retained: RuntimeFailStopRetained,
}

// These payloads are deliberately captive. Once the coordinator fail-stops,
// it must retain every move-only authority whose native or durable ownership
// may still be attached; inspecting or dropping that authority would invent a
// settlement the protocol cannot prove.
#[allow(dead_code)]
pub(super) enum RuntimeFailStopRetained {
    Prior(Box<RuntimeSlotState>),
    BeginDiverged(Box<JournalActivationDiverged<ServiceRuntimeAcquisitionPlan>>),
    MayOwnDiverged(Box<JournalActivationDiverged<ServiceRuntimePackageAccess>>),
    RepositoryAcquisition(Box<ServiceRuntimeAcquisitionError>),
    Publication(Box<PublicationFailStopAuthority>),
    PreHostAccess(Box<ServiceRuntimePackageAccess>),
    NativeCallPanicked(Box<NativeCallPanicAuthority>),
    NativeRetirementCallPanicked(Box<NativeRetirementCallPanicAuthority>),
    RowFenceDiverged(Box<RuntimeRowFenceDivergedAuthority>),
    PublicationReclaim(Box<PublicationReclaimFailStopAuthority>),
    PublicationCallPanicked(Box<PublicationCallPanicAuthority>),
    PublicationReclaimCallPanicked(Box<PublicationReclaimCallPanicAuthority>),
    GrantRebind(Box<GrantRebindFailStopAuthority>),
    GrantRebindCallPanicked(Box<GrantRebindCallPanicAuthority>),
}

#[allow(dead_code)] // Captive authority; see `RuntimeFailStopRetained` above.
pub(super) struct RuntimeRowFenceDivergedAuthority {
    pub(super) diverged: RowFenceDiverged,
    pub(super) operation: RuntimeRowFenceOperation,
}

#[allow(dead_code)] // Captive authority; see `RuntimeFailStopRetained` above.
pub(super) struct PublicationFailStopAuthority {
    pub(super) owner: ExtensionRuntimeOwner,
    pub(super) refusal: ExtensionRuntimePublicationRefusal,
    pub(super) recovery: ServiceRuntimeRecovery,
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
}

#[allow(dead_code)] // Captive authority; see `RuntimeFailStopRetained` above.
pub(super) struct NativeCallPanicAuthority {
    pub(super) pending: ExtensionRuntimePendingPublication,
    pub(super) recovery: ServiceRuntimeRecovery,
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
}

#[allow(dead_code)] // Captive authority; see `RuntimeFailStopRetained` above.
pub(super) struct NativeRetirementCallPanicAuthority {
    pub(super) operation: RuntimeOperationControl,
    pub(super) recovery: ServiceRuntimeRecovery,
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
}

#[allow(dead_code)] // Captive authority; see `RuntimeFailStopRetained` above.
pub(super) struct PublicationReclaimFailStopAuthority {
    pub(super) access: ExtensionPackageAccess,
    pub(super) refusal: ExtensionRuntimePublicationReclaimRefusal,
    pub(super) recovery: ServiceRuntimeRecovery,
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
    pub(super) absence: PostHostAbsenceBasis,
}

#[allow(dead_code)] // Captive authority; see `RuntimeFailStopRetained` above.
pub(super) struct PublicationCallPanicAuthority {
    pub(super) owner: ExtensionRuntimeOwner,
    pub(super) recovery: ServiceRuntimeRecovery,
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
}

#[allow(dead_code)] // Captive authority; see `RuntimeFailStopRetained` above.
pub(super) struct PublicationReclaimCallPanicAuthority {
    pub(super) access: ExtensionPackageAccess,
    pub(super) recovery: ServiceRuntimeRecovery,
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
    pub(super) absence: PostHostAbsenceBasis,
}

#[allow(dead_code)] // Captive authority; see `RuntimeFailStopRetained` above.
pub(super) struct GrantRebindFailStopAuthority {
    pub(super) owner: ExtensionRuntimeOwner,
    pub(super) refusal: ExtensionRuntimePublicationGrantRebindRefusal,
    pub(super) recovery: ServiceRuntimeRecovery,
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
}

#[allow(dead_code)] // Engine authority remains captive after an unwinding host call.
pub(super) struct GrantRebindCallPanicAuthority {
    pub(super) owner: ExtensionRuntimeOwner,
    pub(super) recovery: ServiceRuntimeRecovery,
    pub(super) current_entry: ExtensionNativeOwnershipEntry,
}

impl RuntimeSlot {
    pub(super) const fn planning(key: ExtensionNativeOwnershipKey) -> Self {
        Self {
            key,
            generation: None,
            state: Some(RuntimeSlotState::Planning),
        }
    }

    pub(super) const fn key(&self) -> ExtensionNativeOwnershipKey {
        self.key
    }

    pub(super) const fn generation(&self) -> Option<ExtensionRuntimeGeneration> {
        self.generation
    }

    pub(super) fn set_generation(&mut self, generation: ExtensionRuntimeGeneration) -> bool {
        if self.generation.is_some() {
            return false;
        }
        self.generation = Some(generation);
        true
    }

    pub(super) fn take_state(&mut self) -> Option<RuntimeSlotState> {
        self.state.take()
    }

    pub(super) fn install_state(&mut self, state: RuntimeSlotState) -> bool {
        if self.state.is_some() {
            return false;
        }
        self.state = Some(state);
        true
    }

    pub(super) fn is_live(&self) -> bool {
        matches!(
            self.state,
            Some(RuntimeSlotState::Owned(ref state))
                if matches!(state.operation, RuntimeOperationControl::Published(_))
        ) || matches!(
            self.state,
            Some(RuntimeSlotState::GrantRebindPending(ref operation))
                if operation.is_live_grant_rebind()
        ) || matches!(
            self.state,
            Some(RuntimeSlotState::RowFenceConflict(ref state))
                if state.operation.is_live_grant_rebind()
        ) || matches!(
            self.state,
            Some(RuntimeSlotState::RowFenceAmbiguous(ref state))
                if state.operation.is_live_grant_rebind()
        )
    }

    pub(super) fn published_owned_entry(&self) -> Option<&ExtensionNativeOwnershipEntry> {
        match self.state.as_ref()? {
            RuntimeSlotState::Owned(state)
                if matches!(state.operation, RuntimeOperationControl::Published(_)) =>
            {
                Some(&state.current_entry)
            }
            _ => None,
        }
    }

    /// Borrows package bytes only while this exact slot has a definitely
    /// owned, fully published runtime. Pending rebind and retirement cannot
    /// authorize a new offscreen resource response.
    pub(super) fn published_owner_mut(&mut self) -> Option<&mut ExtensionRuntimeOwner> {
        match self.state.as_mut()? {
            RuntimeSlotState::Owned(state)
                if matches!(state.operation, RuntimeOperationControl::Published(_)) =>
            {
                Some(&mut state.owner)
            }
            _ => None,
        }
    }

    pub(super) fn has_attached_obligation(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(RuntimeSlotState::has_attached_obligation)
    }

    /// Whether this slot owns reconciliation evidence whose before/after
    /// frontier includes the shared journal clocks. While such evidence is
    /// captive, no other slot may drive: an unrelated durable mutation would
    /// turn a recoverable settlement into an artificial third frontier.
    pub(super) fn owns_journal_settlement_barrier(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(RuntimeSlotState::owns_journal_settlement_barrier)
    }

    pub(super) fn enter_fail_stop(
        &mut self,
        reason: RuntimeCoordinatorFailureReason,
        retained: RuntimeFailStopRetained,
    ) -> bool {
        if self.state.is_some() {
            return false;
        }
        self.state = Some(RuntimeSlotState::FailStop(Box::new(RuntimeFailStopState {
            reason,
            retained,
        })));
        true
    }
}

impl RuntimeSlotState {
    fn owns_journal_settlement_barrier(&self) -> bool {
        match self {
            Self::BeginConflict(_)
            | Self::BeginAmbiguous(_)
            | Self::MayOwnConflict(_)
            | Self::MayOwnAmbiguous(_)
            | Self::RowFenceConflict(_)
            | Self::RowFenceAmbiguous(_) => true,
            Self::Planning
            | Self::Planned(_)
            | Self::AcquireLease(_)
            | Self::PreparingLease(_)
            | Self::PackageAccessBuildRefusal(_)
            | Self::PreparingAccess(_)
            | Self::MayOwnAccess(_)
            | Self::HostBindingRefusal(_)
            | Self::NativeActivation(_)
            | Self::NativeUncertain(_)
            | Self::NativeRetirement(_)
            | Self::NativeRetirementUncertain(_)
            | Self::Owned(_)
            | Self::Release(_)
            | Self::GrantRebindPending(_)
            | Self::FailStop(_) => false,
        }
    }

    fn has_attached_obligation(&self) -> bool {
        match self {
            Self::NativeUncertain(_)
            | Self::NativeRetirement(_)
            | Self::NativeRetirementUncertain(_)
            | Self::Owned(_) => true,
            Self::Release(state) => state.has_attached_obligation(),
            Self::GrantRebindPending(operation) => operation.has_attached_obligation(),
            Self::RowFenceConflict(state) => state.operation.has_attached_obligation(),
            Self::RowFenceAmbiguous(state) => state.operation.has_attached_obligation(),
            Self::FailStop(state) => state.retained.has_attached_obligation(),
            Self::Planning
            | Self::Planned(_)
            | Self::BeginConflict(_)
            | Self::BeginAmbiguous(_)
            | Self::AcquireLease(_)
            | Self::PreparingLease(_)
            | Self::PackageAccessBuildRefusal(_)
            | Self::PreparingAccess(_)
            | Self::MayOwnConflict(_)
            | Self::MayOwnAmbiguous(_)
            | Self::MayOwnAccess(_)
            | Self::HostBindingRefusal(_)
            // An unattempted or retryable activation request is documented by
            // the runtime API as definitely native-absent.
            | Self::NativeActivation(_) => false,
        }
    }
}

impl ReleaseRuntimeState {
    fn has_attached_obligation(&self) -> bool {
        match &self.authority {
            ReleaseAuthority::PostHost(state) => state.operation.has_attached_obligation(),
            ReleaseAuthority::NoRepositoryPin(_)
            | ReleaseAuthority::Lease(_)
            | ReleaseAuthority::PreHost(_)
            | ReleaseAuthority::PreHostRefusal(_)
            | ReleaseAuthority::Repository(_)
            // Rejoin refusal is created only after native absence and
            // publication reclaim have both settled.
            | ReleaseAuthority::RejoinRefusal(_) => false,
        }
    }
}

impl RuntimeOperationControl {
    pub(super) const fn registry_generation(
        &self,
    ) -> zephium_extension_runtime_api::ExtensionRuntimeHostRegistryGeneration {
        match self {
            Self::Pending(pending) => pending.registry_generation(),
            Self::PublicationRequest(request) => request.registry_generation(),
            Self::Published(receipt) => receipt.registry_generation(),
        }
    }

    fn has_attached_obligation(&self) -> bool {
        matches!(self, Self::Published(_))
    }
}

impl PostHostReleaseAuthority {
    /// Rejoins non-authorizing host evidence to the exact durable/service
    /// authority that is about to consume it. This is the final local ABA
    /// fence before publication reclaim and repository release.
    pub(super) fn accepts_absence_for(&self, entry: &ExtensionNativeOwnershipEntry) -> bool {
        match self.absence {
            PostHostAbsenceBasis::ActivationUnentered => {
                matches!(self.operation, RuntimeOperationControl::Pending(_))
                    && runtime_target_matches_backend(self.access.target(), entry.runtime_backend())
            }
            PostHostAbsenceBasis::Proven(absence) => {
                absence.target() == self.access.target()
                    && absence.registry_generation() == self.operation.registry_generation()
                    && absence.structurally_matches_entry(entry)
            }
        }
    }
}

const fn runtime_target_matches_backend(
    target: zephium_extension_runtime_api::ExtensionRuntimeTarget,
    backend: zephium_core::extensions::ExtensionRuntimeBackendTarget,
) -> bool {
    use zephium_core::extensions::ExtensionRuntimeBackendTarget;
    use zephium_extension_runtime_api::ExtensionRuntimeTarget;
    matches!(
        (target, backend),
        (
            ExtensionRuntimeTarget::NativeWebExtension,
            ExtensionRuntimeBackendTarget::MacosNative
                | ExtensionRuntimeBackendTarget::WindowsNative
        ) | (
            ExtensionRuntimeTarget::Compatibility,
            ExtensionRuntimeBackendTarget::MacosCompatibility
                | ExtensionRuntimeBackendTarget::LinuxCompatibility
        )
    )
}

impl RuntimeRowFenceOperation {
    fn has_attached_obligation(&self) -> bool {
        match &self.authority {
            RuntimeRowFenceAuthority::Owned(_) => true,
            RuntimeRowFenceAuthority::Release(state) => state.has_attached_obligation(),
        }
    }
}

impl RuntimeFailStopRetained {
    fn has_attached_obligation(&self) -> bool {
        match self {
            Self::Prior(state) => state.has_attached_obligation(),
            Self::Publication(_)
            | Self::NativeCallPanicked(_)
            | Self::NativeRetirementCallPanicked(_)
            | Self::PublicationReclaim(_)
            | Self::PublicationCallPanicked(_)
            | Self::PublicationReclaimCallPanicked(_)
            | Self::GrantRebind(_)
            | Self::GrantRebindCallPanicked(_) => true,
            Self::RowFenceDiverged(state) => state.operation.has_attached_obligation(),
            Self::BeginDiverged(_)
            | Self::MayOwnDiverged(_)
            | Self::RepositoryAcquisition(_)
            | Self::PreHostAccess(_) => false,
        }
    }
}
