//! Serialized recovery of durable possible-native-owner rows.

use std::time::Instant;

use zephium_core::extensions::{
    ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipIdentity,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipJournalMutation,
    ExtensionNativeOwnershipPhase, ExtensionRuntimeBackendTarget,
};
use zephium_core::ids::ProfileId;
use zephium_extension_runtime_api::{
    ExtensionRuntimeAbsenceEvidence, ExtensionRuntimeFailure, ExtensionRuntimeHostBindError,
    ExtensionRuntimeHostFactory, ExtensionRuntimeHostProfileAbsenceDisposition,
    ExtensionRuntimeHostProfileAbsenceEvidence, ExtensionRuntimeHostRecoveryBinding,
    ExtensionRuntimeOwnershipEvidence, ExtensionRuntimeRecoveryOwner,
    ExtensionRuntimeRecoveryRequest, ExtensionRuntimeRecoveryRetirementSettlement,
    ExtensionRuntimeRecoverySettlement,
};

use crate::cleanup::CancellationCheck;
use crate::journal_store::{
    JournalBackend, JournalLoadFailure, JournalMutationFailure, JournalProjection,
};

/// One bounded step made by the native recovery state machine.
pub(crate) enum NativeRecoveryStep {
    Progress,
    Reload,
    Unsupported,
    Unavailable(NativeRecoveryUnavailable),
    Failed(NativeRecoveryFailure),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeRecoveryUnavailable {
    Cancelled,
    DeadlineExpired,
    StoreNotAdmitted,
    StoreObservationPending,
    BackendUnavailable,
    CapacityExceeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeRecoveryFailure {
    StoreJournalLoadFailed,
    StoreMutationInvariant,
    StoreProjectionMismatch,
    InvalidJournalTransition,
    InvalidBinding,
    HostInvariant,
}

struct PendingTransition {
    before: ExtensionNativeOwnershipEntry,
    after: ExtensionNativeOwnershipEntry,
    mutation: ExtensionNativeOwnershipJournalMutation,
}

struct DurableRecoveryLineage {
    entry: ExtensionNativeOwnershipEntry,
    pending: Option<PendingTransition>,
}

impl DurableRecoveryLineage {
    fn new(entry: ExtensionNativeOwnershipEntry) -> Self {
        Self {
            entry,
            pending: None,
        }
    }
}

enum NativeRecoveryControl {
    Request {
        request: ExtensionRuntimeRecoveryRequest,
        attached: bool,
    },
    Owner(ExtensionRuntimeRecoveryOwner),
    AbsenceFence(ExtensionRuntimeAbsenceEvidence),
}

struct NativeRecoveryFrontier {
    durable: DurableRecoveryLineage,
    control: NativeRecoveryControl,
}

/// Unique host-factory owner plus at most one exact recovery frontier.
///
/// The engine recovery pool has capacity one. Keeping the factory and frontier
/// in the serialized worker prevents parallel cleanup and makes an attached
/// proxy impossible to lose between retry attempts.
pub(crate) struct NativeRecoveryState {
    factory: ExtensionRuntimeHostFactory,
    frontier: Option<NativeRecoveryFrontier>,
    native_call_panic_fence: bool,
}

impl NativeRecoveryState {
    pub(crate) const fn new(factory: ExtensionRuntimeHostFactory) -> Self {
        Self {
            factory,
            frontier: None,
            native_call_panic_fence: false,
        }
    }

    pub(crate) const fn has_frontier(&self) -> bool {
        self.frontier.is_some()
    }

    /// Borrows the unique host factory only while crash-recovery owns no
    /// frontier and no recovery call is crossing the native panic fence.
    ///
    /// Fresh runtime admission and persisted-owner recovery share one engine
    /// registry factory. Returning `None` keeps those two protocols serialized
    /// without exposing or moving the factory out of this worker-owned state.
    #[allow(dead_code)] // Consumed by the worker-private runtime coordinator before actor ingress is wired.
    pub(crate) fn idle_factory(&mut self) -> Option<&mut ExtensionRuntimeHostFactory> {
        if self.frontier.is_some() || self.native_call_panic_fence {
            None
        } else {
            Some(&mut self.factory)
        }
    }

    /// Returns the exact profile owned by the current cleanup frontier.
    ///
    /// Pending durable transitions must preserve the complete ownership key;
    /// disagreement is a host/service invariant failure rather than a profile
    /// that scoped retirement may guess around.
    pub(crate) fn frontier_profile(&self) -> Result<Option<ProfileId>, NativeRecoveryFailure> {
        let Some(frontier) = self.frontier.as_ref() else {
            return Ok(None);
        };
        let key = frontier.durable.entry.key();
        if frontier
            .durable
            .pending
            .as_ref()
            .is_some_and(|pending| pending.before.key() != key || pending.after.key() != key)
        {
            return Err(NativeRecoveryFailure::HostInvariant);
        }
        Ok(Some(key.profile()))
    }

    /// Joins the engine's complete profile registry fence with this worker's
    /// exact local recovery frontier.
    pub(crate) fn profile_absence_until(
        &mut self,
        profile: ProfileId,
        deadline: Instant,
    ) -> Result<
        ExtensionRuntimeHostProfileAbsenceEvidence<'_>,
        ExtensionRuntimeHostProfileAbsenceDisposition,
    > {
        match self.frontier_profile() {
            Err(_) => {
                return Err(ExtensionRuntimeHostProfileAbsenceDisposition::InvariantFailed);
            }
            Ok(Some(frontier_profile)) if frontier_profile == profile => {
                return Err(ExtensionRuntimeHostProfileAbsenceDisposition::ObligationsRemain);
            }
            Ok(Some(_) | None) => {}
        }
        self.native_call_panic_fence = true;
        let outcome = self.factory.profile_absence_until(profile, deadline);
        self.native_call_panic_fence = false;
        outcome
    }

    /// Whether dropping this state would abandon an attached engine registry
    /// obligation. A merely bound, never-invoked request remains provisional
    /// and its passive destructor may return that local reservation.
    pub(crate) fn has_attached_obligation(&self) -> bool {
        self.native_call_panic_fence
            || self.frontier.as_ref().is_some_and(|frontier| {
                matches!(
                    frontier.control,
                    NativeRecoveryControl::Request { attached: true, .. }
                        | NativeRecoveryControl::Owner(_)
                )
            })
    }

    pub(crate) fn begin(&mut self, entry: ExtensionNativeOwnershipEntry) -> NativeRecoveryStep {
        if self.frontier.is_some() {
            return NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant);
        }
        let binding = match ExtensionRuntimeHostRecoveryBinding::try_new(entry.clone()) {
            Ok(binding) => binding,
            Err(_) => {
                return NativeRecoveryStep::Failed(NativeRecoveryFailure::InvalidBinding);
            }
        };
        self.native_call_panic_fence = true;
        let binding = self.factory.bind_recovery(binding);
        self.native_call_panic_fence = false;
        match binding {
            Ok(request) => {
                self.frontier = Some(NativeRecoveryFrontier {
                    durable: DurableRecoveryLineage::new(entry),
                    control: NativeRecoveryControl::Request {
                        request,
                        attached: false,
                    },
                });
                NativeRecoveryStep::Progress
            }
            Err(refusal) => classify_bind_error(refusal.reason()),
        }
    }

    pub(crate) fn advance(
        &mut self,
        journal_backend: &impl JournalBackend,
        projection: &mut JournalProjection,
        cancellation: &impl CancellationCheck,
        deadline: Instant,
    ) -> NativeRecoveryStep {
        let Some(frontier) = self.frontier.take() else {
            return NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant);
        };
        let (frontier, step) = match frontier.control {
            NativeRecoveryControl::Request { request, attached } => self.advance_request(
                frontier.durable,
                request,
                attached,
                journal_backend,
                projection,
                cancellation,
                deadline,
            ),
            NativeRecoveryControl::Owner(owner) => self.advance_owner(
                frontier.durable,
                owner,
                journal_backend,
                projection,
                cancellation,
                deadline,
            ),
            NativeRecoveryControl::AbsenceFence(absence) => self.advance_absence(
                frontier.durable,
                absence,
                journal_backend,
                projection,
                deadline,
            ),
        };
        self.frontier = frontier;
        step
    }

    #[allow(clippy::too_many_arguments)]
    fn advance_request(
        &mut self,
        mut durable: DurableRecoveryLineage,
        request: ExtensionRuntimeRecoveryRequest,
        attached: bool,
        journal_backend: &impl JournalBackend,
        projection: &mut JournalProjection,
        cancellation: &impl CancellationCheck,
        deadline: Instant,
    ) -> (Option<NativeRecoveryFrontier>, NativeRecoveryStep) {
        if durable.pending.is_some() {
            let step =
                settle_pending_transition(&mut durable, journal_backend, projection, deadline);
            return (Some(request_frontier(durable, request, attached)), step);
        }
        if let Some(step) =
            ensure_exact_durable_row(&durable.entry, journal_backend, projection, deadline)
        {
            return (Some(request_frontier(durable, request, attached)), step);
        }
        if let Some(evidence) = request.ownership_evidence() {
            match ensure_evidence_is_durable(&mut durable, evidence, projection) {
                EvidenceDurability::Exact => {}
                EvidenceDurability::TransitionScheduled => {
                    let step = settle_pending_transition(
                        &mut durable,
                        journal_backend,
                        projection,
                        deadline,
                    );
                    return (Some(request_frontier(durable, request, attached)), step);
                }
                EvidenceDurability::Invalid => {
                    return (
                        Some(request_frontier(durable, request, attached)),
                        NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant),
                    );
                }
            }
        }
        if let Some(step) = operation_refusal(cancellation, deadline) {
            return (Some(request_frontier(durable, request, attached)), step);
        }

        self.native_call_panic_fence = true;
        let settlement = request.reconcile_until(deadline);
        self.native_call_panic_fence = false;
        let (mut frontier, provisional_step) = match settlement {
            ExtensionRuntimeRecoverySettlement::Owned(owner) => (
                NativeRecoveryFrontier {
                    durable,
                    control: NativeRecoveryControl::Owner(owner),
                },
                NativeRecoveryStep::Progress,
            ),
            ExtensionRuntimeRecoverySettlement::Absent(absence)
                if absence.structurally_matches_entry(&durable.entry) =>
            {
                (
                    NativeRecoveryFrontier {
                        durable,
                        control: NativeRecoveryControl::AbsenceFence(absence),
                    },
                    NativeRecoveryStep::Progress,
                )
            }
            ExtensionRuntimeRecoverySettlement::Absent(absence) => (
                absence_frontier(durable, absence),
                NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant),
            ),
            ExtensionRuntimeRecoverySettlement::StillUncertain { request, failure } => (
                request_frontier(durable, request, true),
                classify_runtime_failure(failure),
            ),
        };
        if let Some(step) =
            settle_new_frontier_evidence(&mut frontier, journal_backend, projection, deadline)
        {
            // A successful evidence write must not erase the adapter's
            // settlement classification. Retryable uncertainty remains
            // retryable and an identity conflict remains terminal for this
            // startup attempt, now with its observation durably retained.
            let step = if step_is_progress(&step) {
                provisional_step
            } else {
                step
            };
            return (Some(frontier), step);
        }
        let post_step = ensure_exact_durable_row(
            &frontier.durable.entry,
            journal_backend,
            projection,
            deadline,
        );
        (Some(frontier), post_step.unwrap_or(provisional_step))
    }

    #[allow(clippy::too_many_arguments)]
    fn advance_owner(
        &mut self,
        mut durable: DurableRecoveryLineage,
        owner: ExtensionRuntimeRecoveryOwner,
        journal_backend: &impl JournalBackend,
        projection: &mut JournalProjection,
        cancellation: &impl CancellationCheck,
        deadline: Instant,
    ) -> (Option<NativeRecoveryFrontier>, NativeRecoveryStep) {
        if durable.pending.is_some() {
            let step =
                settle_pending_transition(&mut durable, journal_backend, projection, deadline);
            return (Some(owner_frontier(durable, owner)), step);
        }
        if let Some(step) =
            ensure_exact_durable_row(&durable.entry, journal_backend, projection, deadline)
        {
            return (Some(owner_frontier(durable, owner)), step);
        }
        match ensure_evidence_is_durable(&mut durable, owner.ownership_evidence(), projection) {
            EvidenceDurability::Exact => {}
            EvidenceDurability::TransitionScheduled => {
                let step =
                    settle_pending_transition(&mut durable, journal_backend, projection, deadline);
                return (Some(owner_frontier(durable, owner)), step);
            }
            EvidenceDurability::Invalid => {
                return (
                    Some(owner_frontier(durable, owner)),
                    NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant),
                );
            }
        }
        if !evidence_matches_durable_expectation(&durable.entry, owner.ownership_evidence()) {
            // A trusted runtime request is required to poison positive
            // settlement when catalog-expected and adapter-observed identities
            // disagree. Keep the owner capability retained and fail closed if
            // an engine implementation ever violates that contract. The
            // durable row remains NativeMayOwn and no retirement/release work
            // is authorized from the mismatched positive claim.
            return (
                Some(owner_frontier(durable, owner)),
                NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant),
            );
        }
        if durable.entry.intent() == ExtensionNativeOwnershipIntent::Acquire {
            let mutation = ExtensionNativeOwnershipJournalMutation::transition(
                durable.entry.cas(),
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
            );
            let step = schedule_and_settle_transition(
                &mut durable,
                mutation,
                journal_backend,
                projection,
                deadline,
            );
            return (Some(owner_frontier(durable, owner)), step);
        }
        if durable.entry.intent() != ExtensionNativeOwnershipIntent::Release
            || durable.entry.phase() != ExtensionNativeOwnershipPhase::NativeMayOwn
        {
            return (
                Some(owner_frontier(durable, owner)),
                NativeRecoveryStep::Failed(NativeRecoveryFailure::InvalidJournalTransition),
            );
        }
        if let Some(step) = operation_refusal(cancellation, deadline) {
            return (Some(owner_frontier(durable, owner)), step);
        }

        self.native_call_panic_fence = true;
        let settlement = owner.into_retirement_request().settle_until(deadline);
        self.native_call_panic_fence = false;
        let (frontier, provisional_step) = match settlement {
            ExtensionRuntimeRecoveryRetirementSettlement::Absent(absence)
                if absence.structurally_matches_entry(&durable.entry) =>
            {
                (
                    NativeRecoveryFrontier {
                        durable,
                        control: NativeRecoveryControl::AbsenceFence(absence),
                    },
                    NativeRecoveryStep::Progress,
                )
            }
            ExtensionRuntimeRecoveryRetirementSettlement::Absent(absence) => (
                absence_frontier(durable, absence),
                NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant),
            ),
            ExtensionRuntimeRecoveryRetirementSettlement::Retained { owner, failure } => (
                owner_frontier(durable, owner),
                classify_runtime_failure(failure),
            ),
            ExtensionRuntimeRecoveryRetirementSettlement::OwnershipUncertain {
                request,
                failure,
            } => (
                request_frontier(durable, request, true),
                classify_runtime_failure(failure),
            ),
        };
        let post_step = ensure_exact_durable_row(
            &frontier.durable.entry,
            journal_backend,
            projection,
            deadline,
        );
        (Some(frontier), post_step.unwrap_or(provisional_step))
    }

    fn advance_absence(
        &mut self,
        mut durable: DurableRecoveryLineage,
        absence: ExtensionRuntimeAbsenceEvidence,
        journal_backend: &impl JournalBackend,
        projection: &mut JournalProjection,
        deadline: Instant,
    ) -> (Option<NativeRecoveryFrontier>, NativeRecoveryStep) {
        if !absence.structurally_matches_entry(&durable.entry) {
            return (
                Some(absence_frontier(durable, absence)),
                NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant),
            );
        }
        if durable.pending.is_some() {
            let step =
                settle_pending_transition(&mut durable, journal_backend, projection, deadline);
            if step_is_progress(&step)
                && durable.entry.intent() == ExtensionNativeOwnershipIntent::Release
                && durable.entry.phase()
                    == ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
            {
                return (None, NativeRecoveryStep::Progress);
            }
            return (Some(absence_frontier(durable, absence)), step);
        }
        if let Some(step) =
            ensure_exact_durable_row(&durable.entry, journal_backend, projection, deadline)
        {
            return (Some(absence_frontier(durable, absence)), step);
        }
        if durable.entry.intent() == ExtensionNativeOwnershipIntent::Release
            && durable.entry.phase() == ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
        {
            return (None, NativeRecoveryStep::Progress);
        }
        let (intent, phase) = match (durable.entry.intent(), durable.entry.phase()) {
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeOwned,
            ) => (
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
            ),
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
            )
            | (
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
            ) => (
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            ),
            _ => {
                return (
                    Some(absence_frontier(durable, absence)),
                    NativeRecoveryStep::Failed(NativeRecoveryFailure::InvalidJournalTransition),
                );
            }
        };
        let mutation =
            ExtensionNativeOwnershipJournalMutation::transition(durable.entry.cas(), intent, phase);
        let step = schedule_and_settle_transition(
            &mut durable,
            mutation,
            journal_backend,
            projection,
            deadline,
        );
        if step_is_progress(&step)
            && durable.entry.intent() == ExtensionNativeOwnershipIntent::Release
            && durable.entry.phase() == ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
        {
            (None, NativeRecoveryStep::Progress)
        } else {
            (Some(absence_frontier(durable, absence)), step)
        }
    }
}

fn request_frontier(
    durable: DurableRecoveryLineage,
    request: ExtensionRuntimeRecoveryRequest,
    attached: bool,
) -> NativeRecoveryFrontier {
    NativeRecoveryFrontier {
        durable,
        control: NativeRecoveryControl::Request { request, attached },
    }
}

fn owner_frontier(
    durable: DurableRecoveryLineage,
    owner: ExtensionRuntimeRecoveryOwner,
) -> NativeRecoveryFrontier {
    NativeRecoveryFrontier {
        durable,
        control: NativeRecoveryControl::Owner(owner),
    }
}

fn absence_frontier(
    durable: DurableRecoveryLineage,
    absence: ExtensionRuntimeAbsenceEvidence,
) -> NativeRecoveryFrontier {
    NativeRecoveryFrontier {
        durable,
        control: NativeRecoveryControl::AbsenceFence(absence),
    }
}

fn operation_refusal(
    cancellation: &impl CancellationCheck,
    deadline: Instant,
) -> Option<NativeRecoveryStep> {
    if cancellation.is_cancelled() {
        Some(NativeRecoveryStep::Unavailable(
            NativeRecoveryUnavailable::Cancelled,
        ))
    } else if cancellation.deadline_reached(deadline) {
        Some(NativeRecoveryStep::Unavailable(
            NativeRecoveryUnavailable::DeadlineExpired,
        ))
    } else {
        None
    }
}

fn ensure_exact_durable_row(
    expected: &ExtensionNativeOwnershipEntry,
    journal_backend: &impl JournalBackend,
    projection: &mut JournalProjection,
    deadline: Instant,
) -> Option<NativeRecoveryStep> {
    let journal = match projection.reload(journal_backend, deadline) {
        Ok(journal) => journal,
        Err(error) => return Some(classify_load_error(error)),
    };
    match journal.get(expected.key()) {
        Some(current) if current == expected => None,
        _ => Some(NativeRecoveryStep::Failed(
            NativeRecoveryFailure::StoreProjectionMismatch,
        )),
    }
}

enum EvidenceDurability {
    Exact,
    TransitionScheduled,
    Invalid,
}

fn ensure_evidence_is_durable(
    durable: &mut DurableRecoveryLineage,
    evidence: ExtensionRuntimeOwnershipEvidence,
    projection: &JournalProjection,
) -> EvidenceDurability {
    let identity = match evidence {
        ExtensionRuntimeOwnershipEvidence::MacosWebExtension(owner) => {
            if durable.entry.runtime_backend() != ExtensionRuntimeBackendTarget::MacosNative {
                return EvidenceDurability::Invalid;
            }
            ExtensionNativeOwnershipIdentity::from_encoded_bytes(
                ExtensionRuntimeBackendTarget::MacosNative,
                owner.encoded_bytes(),
            )
            .ok()
        }
        ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(owner) => {
            if durable.entry.runtime_backend() != ExtensionRuntimeBackendTarget::WindowsNative {
                return EvidenceDurability::Invalid;
            }
            ExtensionNativeOwnershipIdentity::from_encoded_bytes(
                ExtensionRuntimeBackendTarget::WindowsNative,
                owner.encoded_bytes(),
            )
            .ok()
        }
        ExtensionRuntimeOwnershipEvidence::Compatibility => {
            return if matches!(
                durable.entry.runtime_backend(),
                ExtensionRuntimeBackendTarget::MacosCompatibility
                    | ExtensionRuntimeBackendTarget::LinuxCompatibility
            ) && durable.entry.native_identity().is_none()
            {
                EvidenceDurability::Exact
            } else {
                EvidenceDurability::Invalid
            };
        }
        _ => return EvidenceDurability::Invalid,
    };
    let Some(identity) = identity else {
        return EvidenceDurability::Invalid;
    };
    match durable.entry.native_identity() {
        Some(current) if current == identity => EvidenceDurability::Exact,
        Some(_) => EvidenceDurability::Invalid,
        None if durable.entry.phase() == ExtensionNativeOwnershipPhase::NativeMayOwn
            && schedule_transition(
                durable,
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    durable.entry.cas(),
                    durable.entry.intent(),
                    durable.entry.phase(),
                    identity,
                ),
                projection,
            ) =>
        {
            EvidenceDurability::TransitionScheduled
        }
        None => EvidenceDurability::Invalid,
    }
}

fn evidence_matches_durable_expectation(
    entry: &ExtensionNativeOwnershipEntry,
    evidence: ExtensionRuntimeOwnershipEvidence,
) -> bool {
    let observed = match evidence {
        ExtensionRuntimeOwnershipEvidence::MacosWebExtension(owner)
            if entry.runtime_backend() == ExtensionRuntimeBackendTarget::MacosNative =>
        {
            ExtensionNativeOwnershipIdentity::from_encoded_bytes(
                ExtensionRuntimeBackendTarget::MacosNative,
                owner.encoded_bytes(),
            )
            .ok()
        }
        ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(owner)
            if entry.runtime_backend() == ExtensionRuntimeBackendTarget::WindowsNative =>
        {
            ExtensionNativeOwnershipIdentity::from_encoded_bytes(
                ExtensionRuntimeBackendTarget::WindowsNative,
                owner.encoded_bytes(),
            )
            .ok()
        }
        ExtensionRuntimeOwnershipEvidence::Compatibility => {
            return matches!(
                entry.runtime_backend(),
                ExtensionRuntimeBackendTarget::MacosCompatibility
                    | ExtensionRuntimeBackendTarget::LinuxCompatibility
            ) && entry.expected_native_identity().is_none()
                && entry.native_identity().is_none();
        }
        _ => None,
    };
    let Some(observed) = observed else {
        return false;
    };
    entry
        .expected_native_identity()
        .is_none_or(|expected| expected.matches_observed(observed))
}

fn settle_new_frontier_evidence(
    frontier: &mut NativeRecoveryFrontier,
    journal_backend: &impl JournalBackend,
    projection: &mut JournalProjection,
    deadline: Instant,
) -> Option<NativeRecoveryStep> {
    let evidence = match &frontier.control {
        NativeRecoveryControl::Request { request, .. } => request.ownership_evidence(),
        NativeRecoveryControl::Owner(owner) => Some(owner.ownership_evidence()),
        NativeRecoveryControl::AbsenceFence(_) => None,
    }?;
    match ensure_evidence_is_durable(&mut frontier.durable, evidence, projection) {
        EvidenceDurability::Exact => None,
        EvidenceDurability::TransitionScheduled => Some(settle_pending_transition(
            &mut frontier.durable,
            journal_backend,
            projection,
            deadline,
        )),
        EvidenceDurability::Invalid => Some(NativeRecoveryStep::Failed(
            NativeRecoveryFailure::HostInvariant,
        )),
    }
}

fn schedule_and_settle_transition(
    durable: &mut DurableRecoveryLineage,
    mutation: ExtensionNativeOwnershipJournalMutation,
    journal_backend: &impl JournalBackend,
    projection: &mut JournalProjection,
    deadline: Instant,
) -> NativeRecoveryStep {
    if !schedule_transition(durable, mutation, projection) {
        return NativeRecoveryStep::Failed(NativeRecoveryFailure::InvalidJournalTransition);
    }
    settle_pending_transition(durable, journal_backend, projection, deadline)
}

fn schedule_transition(
    durable: &mut DurableRecoveryLineage,
    mutation: ExtensionNativeOwnershipJournalMutation,
    projection: &JournalProjection,
) -> bool {
    if durable.pending.is_some() {
        return false;
    }
    let Some(journal) = projection.known() else {
        return false;
    };
    if journal.get(durable.entry.key()) != Some(&durable.entry) {
        return false;
    }
    let Ok(application) = journal.clone().apply(journal.revision(), mutation.clone()) else {
        return false;
    };
    let Some(after) = application.entry().cloned() else {
        return false;
    };
    durable.pending = Some(PendingTransition {
        before: durable.entry.clone(),
        after,
        mutation,
    });
    true
}

fn settle_pending_transition(
    durable: &mut DurableRecoveryLineage,
    journal_backend: &impl JournalBackend,
    projection: &mut JournalProjection,
    deadline: Instant,
) -> NativeRecoveryStep {
    let Some(pending) = durable.pending.as_ref() else {
        return NativeRecoveryStep::Failed(NativeRecoveryFailure::InvalidJournalTransition);
    };
    let journal = match projection.reload(journal_backend, deadline) {
        Ok(journal) => journal,
        Err(error) => return classify_load_error(error),
    };
    match journal.get(pending.before.key()) {
        Some(current) if current == &pending.after => {
            durable.entry = pending.after.clone();
            durable.pending = None;
            return NativeRecoveryStep::Progress;
        }
        Some(current) if current == &pending.before => {}
        _ => {
            return NativeRecoveryStep::Failed(NativeRecoveryFailure::StoreProjectionMismatch);
        }
    }
    let mutation = pending.mutation.clone();
    match projection.mutate(journal_backend, mutation, deadline) {
        Ok(journal) => match journal.get(pending.before.key()) {
            Some(current) if current == &pending.after => {
                durable.entry = pending.after.clone();
                durable.pending = None;
                NativeRecoveryStep::Progress
            }
            _ => NativeRecoveryStep::Failed(NativeRecoveryFailure::StoreProjectionMismatch),
        },
        Err(error) => classify_mutation_error(error),
    }
}

fn classify_bind_error(error: ExtensionRuntimeHostBindError) -> NativeRecoveryStep {
    match error {
        ExtensionRuntimeHostBindError::UnsupportedBackend => NativeRecoveryStep::Unsupported,
        ExtensionRuntimeHostBindError::Unavailable | ExtensionRuntimeHostBindError::Sealed => {
            NativeRecoveryStep::Unavailable(NativeRecoveryUnavailable::BackendUnavailable)
        }
        ExtensionRuntimeHostBindError::CapacityExceeded => {
            NativeRecoveryStep::Unavailable(NativeRecoveryUnavailable::CapacityExceeded)
        }
        ExtensionRuntimeHostBindError::OwnerConflict
        | ExtensionRuntimeHostBindError::IdentityExhausted
        | ExtensionRuntimeHostBindError::RetainedBytesOverflow
        | ExtensionRuntimeHostBindError::RetainedBytesExceeded
        | ExtensionRuntimeHostBindError::InternalInvariant => {
            NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant)
        }
        _ => NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant),
    }
}

fn classify_runtime_failure(failure: ExtensionRuntimeFailure) -> NativeRecoveryStep {
    match failure {
        ExtensionRuntimeFailure::UnsupportedTarget
        | ExtensionRuntimeFailure::BackendUnavailable
        | ExtensionRuntimeFailure::RestartRequired => {
            NativeRecoveryStep::Unavailable(NativeRecoveryUnavailable::BackendUnavailable)
        }
        ExtensionRuntimeFailure::CapacityExceeded => {
            NativeRecoveryStep::Unavailable(NativeRecoveryUnavailable::CapacityExceeded)
        }
        ExtensionRuntimeFailure::TimedOut => {
            NativeRecoveryStep::Unavailable(NativeRecoveryUnavailable::DeadlineExpired)
        }
        ExtensionRuntimeFailure::PackageRejected | ExtensionRuntimeFailure::Internal => {
            NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant)
        }
        _ => NativeRecoveryStep::Failed(NativeRecoveryFailure::HostInvariant),
    }
}

fn classify_load_error(error: JournalLoadFailure) -> NativeRecoveryStep {
    match error {
        JournalLoadFailure::NotAdmitted => {
            NativeRecoveryStep::Unavailable(NativeRecoveryUnavailable::StoreNotAdmitted)
        }
        JournalLoadFailure::TimedOutAfterAdmission => {
            NativeRecoveryStep::Unavailable(NativeRecoveryUnavailable::StoreObservationPending)
        }
        JournalLoadFailure::Failed => {
            NativeRecoveryStep::Failed(NativeRecoveryFailure::StoreJournalLoadFailed)
        }
    }
}

fn classify_mutation_error(error: JournalMutationFailure) -> NativeRecoveryStep {
    match error {
        JournalMutationFailure::NotAdmitted => {
            NativeRecoveryStep::Unavailable(NativeRecoveryUnavailable::StoreNotAdmitted)
        }
        JournalMutationFailure::ReloadRequired => NativeRecoveryStep::Reload,
        JournalMutationFailure::ProjectionMismatch => {
            NativeRecoveryStep::Failed(NativeRecoveryFailure::StoreProjectionMismatch)
        }
        JournalMutationFailure::InvalidLocalTransition => {
            NativeRecoveryStep::Failed(NativeRecoveryFailure::InvalidJournalTransition)
        }
        JournalMutationFailure::StoreInvariant => {
            NativeRecoveryStep::Failed(NativeRecoveryFailure::StoreMutationInvariant)
        }
    }
}

const fn step_is_progress(step: &NativeRecoveryStep) -> bool {
    matches!(step, NativeRecoveryStep::Progress)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::panic::{self, AssertUnwindSafe};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use zephium_core::extensions::{
        ExtensionAuthorityId, ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
        ExtensionExpectedNativeOwnershipIdentity, ExtensionGrantBrowsingContext,
        ExtensionGrantDigest, ExtensionGrantRevision, ExtensionInstallCatalogRevision,
        ExtensionInstallRevision, ExtensionManifestDigest, ExtensionNativeOwnershipJournal,
        ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPreparation, ExtensionPackageIdentity,
        ExtensionPackageKey, ExtensionPackagePayloadIdentity, ExtensionPackagePinReleaseBinding,
        ExtensionPackageRevision, ExtensionTreeDigest,
    };
    use zephium_core::ids::{ExtensionInstallId, ProfileId};
    use zephium_core::ports::store::{
        ExtensionNativeOwnershipJournalLoadOutcome, ExtensionNativeOwnershipJournalMutationApplied,
        ExtensionNativeOwnershipJournalMutationOutcome,
    };
    use zephium_extension_repository::{
        BundledPackageLeaseReleaseError, BundledPackageLeaseReleaseOutcome,
    };
    use zephium_extension_runtime_api::{
        ExtensionRuntimeAbsenceEvidence, ExtensionRuntimeBoundAbsenceEvidenceIssuer,
        ExtensionRuntimeCompatibilityAbsenceAudit, ExtensionRuntimeHostActivationContext,
        ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostFactoryPort,
        ExtensionRuntimeHostOwnershipPort, ExtensionRuntimeHostRecoveryContext,
        ExtensionRuntimeHostRegistryGeneration, ExtensionRuntimeMacosAbsenceAudit,
        ExtensionRuntimeNativeOwnerId, ExtensionRuntimeOwnershipDisposition,
        ExtensionRuntimeOwnershipPort, ExtensionRuntimeRecoveryExpectation,
        ExtensionRuntimeRetirementDisposition,
    };
    use zephium_store::ExtensionServiceStoreCallOutcome;

    use super::*;
    use crate::cleanup::{
        reconcile_startup, CleanupRepositoryBackend, CleanupStartupOutcome, CleanupUnavailable,
    };
    use crate::repository::ServiceRepositoryOpenError;

    const TEST_DEADLINE: Duration = Duration::from_secs(30);

    enum LoadAction {
        Current,
        Replace(ExtensionNativeOwnershipJournal),
    }

    enum MutationAction {
        Apply,
        ApplyButReportUnknown,
    }

    struct FakeJournalBackend {
        durable: RefCell<ExtensionNativeOwnershipJournal>,
        loads: RefCell<VecDeque<LoadAction>>,
        mutation_actions: RefCell<VecDeque<MutationAction>>,
        mutations: RefCell<Vec<ExtensionNativeOwnershipEntry>>,
    }

    impl FakeJournalBackend {
        fn new(journal: ExtensionNativeOwnershipJournal) -> Self {
            Self {
                durable: RefCell::new(journal),
                loads: RefCell::new(VecDeque::new()),
                mutation_actions: RefCell::new(VecDeque::new()),
                mutations: RefCell::new(Vec::new()),
            }
        }

        fn with_loads(self, loads: impl IntoIterator<Item = LoadAction>) -> Self {
            self.loads.borrow_mut().extend(loads);
            self
        }

        fn durable(&self) -> ExtensionNativeOwnershipJournal {
            self.durable.borrow().clone()
        }

        fn with_mutation_actions(self, actions: impl IntoIterator<Item = MutationAction>) -> Self {
            self.mutation_actions.borrow_mut().extend(actions);
            self
        }

        fn mutation_entries(&self) -> Vec<ExtensionNativeOwnershipEntry> {
            self.mutations.borrow().clone()
        }
    }

    impl JournalBackend for FakeJournalBackend {
        fn load_until(
            &self,
            _deadline: Instant,
        ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipJournalLoadOutcome> {
            let journal = match self.loads.borrow_mut().pop_front() {
                None | Some(LoadAction::Current) => self.durable.borrow().clone(),
                Some(LoadAction::Replace(journal)) => {
                    *self.durable.borrow_mut() = journal.clone();
                    journal
                }
            };
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal),
            )
        }

        fn mutate_until(
            &self,
            journal: &ExtensionNativeOwnershipJournal,
            mutation: ExtensionNativeOwnershipJournalMutation,
            _deadline: Instant,
        ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome>
        {
            let durable = self.durable.borrow().clone();
            if &durable != journal {
                return ExtensionServiceStoreCallOutcome::Completed(
                    ExtensionNativeOwnershipJournalMutationOutcome::Conflict {
                        current: durable.revision(),
                    },
                );
            }
            let application = durable
                .apply(journal.revision(), mutation)
                .expect("scripted mutation must be valid");
            let applied = ExtensionNativeOwnershipJournalMutationApplied {
                journal_revision: application.journal().revision(),
                operation_high_water: application.journal().operation_high_water(),
                native_incarnation_high_water: application
                    .journal()
                    .native_incarnation_high_water(),
                grant_rebind_count: application.journal().grant_rebind_count(),
                entry: application.entry().cloned().map(Box::new),
            };
            if let Some(entry) = application.entry() {
                self.mutations.borrow_mut().push(entry.clone());
            }
            *self.durable.borrow_mut() = application.into_journal();
            match self
                .mutation_actions
                .borrow_mut()
                .pop_front()
                .unwrap_or(MutationAction::Apply)
            {
                MutationAction::Apply => ExtensionServiceStoreCallOutcome::Completed(
                    ExtensionNativeOwnershipJournalMutationOutcome::Applied(applied),
                ),
                MutationAction::ApplyButReportUnknown => {
                    ExtensionServiceStoreCallOutcome::Completed(
                        ExtensionNativeOwnershipJournalMutationOutcome::OutcomeUnknown,
                    )
                }
            }
        }
    }

    struct FakeRepository {
        open: bool,
        releases: usize,
    }

    impl FakeRepository {
        const fn new() -> Self {
            Self {
                open: false,
                releases: 0,
            }
        }
    }

    impl CleanupRepositoryBackend for FakeRepository {
        fn is_open(&self) -> bool {
            self.open
        }

        fn open(&mut self) -> Result<(), ServiceRepositoryOpenError> {
            self.open = true;
            Ok(())
        }

        fn reopen(&mut self) -> Result<(), ServiceRepositoryOpenError> {
            self.open = true;
            Ok(())
        }

        fn reconcile_release(
            &mut self,
            _binding: &ExtensionPackagePinReleaseBinding,
        ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
            self.releases += 1;
            Ok(BundledPackageLeaseReleaseOutcome::Released)
        }
    }

    struct NeverCancelled;

    impl CancellationCheck for NeverCancelled {
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    struct AlwaysCancelled;

    impl CancellationCheck for AlwaysCancelled {
        fn is_cancelled(&self) -> bool {
            true
        }
    }

    struct HostScript {
        bind_error: Option<ExtensionRuntimeHostBindError>,
        bind_calls: usize,
        profile_absence_calls: usize,
        reconcile_calls: usize,
        retire_calls: usize,
        recovery_expectations: Vec<ExtensionRuntimeRecoveryExpectation>,
        profile_absence: VecDeque<Result<(), ExtensionRuntimeHostProfileAbsenceDisposition>>,
        reconcile_deadlines: Vec<Instant>,
        retire_deadlines: Vec<Instant>,
        reconcile: VecDeque<ScriptedReconciliation>,
        retire: VecDeque<ScriptedRetirement>,
    }

    #[allow(clippy::large_enum_variant)] // Test script favors readable Copy-like values.
    enum ScriptedReconciliation {
        Disposition(ExtensionRuntimeOwnershipDisposition),
        Absent,
    }

    enum ScriptedRetirement {
        Retired,
    }

    impl HostScript {
        fn new() -> Self {
            Self {
                bind_error: None,
                bind_calls: 0,
                profile_absence_calls: 0,
                reconcile_calls: 0,
                retire_calls: 0,
                recovery_expectations: Vec::new(),
                profile_absence: VecDeque::new(),
                reconcile_deadlines: Vec::new(),
                retire_deadlines: Vec::new(),
                reconcile: VecDeque::new(),
                retire: VecDeque::new(),
            }
        }
    }

    struct ScriptedHostFactoryPort {
        script: Arc<Mutex<HostScript>>,
    }

    impl ExtensionRuntimeHostFactoryPort for ScriptedHostFactoryPort {
        fn bind_activation(
            &mut self,
            _context: ExtensionRuntimeHostActivationContext<'_>,
        ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
            Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
        }

        fn bind_recovery(
            &mut self,
            context: ExtensionRuntimeHostRecoveryContext,
        ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError>
        {
            let mut script = self.script.lock().unwrap();
            script.bind_calls += 1;
            script.recovery_expectations.push(context.expectation());
            if let Some(error) = script.bind_error {
                return Err(error);
            }
            drop(script);
            Ok(Box::new(ScriptedOwnershipPort {
                script: Arc::clone(&self.script),
                absence_issuer: context.absence_evidence_issuer().bind(
                    ExtensionRuntimeHostRegistryGeneration::new(1)
                        .expect("test registry generation is nonzero"),
                ),
                last_absence: None,
                next_absence_attempt: 1,
                recovery_expectation: context.expectation(),
            }))
        }

        fn profile_absence_until(
            &mut self,
            _profile: ProfileId,
            _deadline: Instant,
        ) -> Result<(), ExtensionRuntimeHostProfileAbsenceDisposition> {
            let mut script = self.script.lock().unwrap();
            script.profile_absence_calls += 1;
            script.profile_absence.pop_front().unwrap_or(Err(
                ExtensionRuntimeHostProfileAbsenceDisposition::Unavailable,
            ))
        }
    }

    struct ScriptedOwnershipPort {
        script: Arc<Mutex<HostScript>>,
        absence_issuer: ExtensionRuntimeBoundAbsenceEvidenceIssuer,
        last_absence: Option<ExtensionRuntimeAbsenceEvidence>,
        next_absence_attempt: u64,
        recovery_expectation: ExtensionRuntimeRecoveryExpectation,
    }

    impl ScriptedOwnershipPort {
        fn mint_absence(&mut self) -> Option<ExtensionRuntimeAbsenceEvidence> {
            let attempt = std::num::NonZeroU64::new(self.next_absence_attempt)
                .expect("test absence attempt remains nonzero");
            self.next_absence_attempt = self
                .next_absence_attempt
                .checked_add(1)
                .expect("test absence attempt does not exhaust");
            let absence = match self.recovery_expectation {
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                    catalog_expected,
                    adapter_observed,
                } if catalog_expected.or(adapter_observed).is_some_and(|anchor| {
                    catalog_expected.is_none_or(|expected| expected == anchor)
                        && adapter_observed.is_none_or(|observed| observed == anchor)
                }) =>
                {
                    let observed = catalog_expected
                        .or(adapter_observed)
                        .expect("guard established one exact native identity anchor");
                    let audit = ExtensionRuntimeMacosAbsenceAudit::try_from_observations(
                        true, true, true, true, false, false, true, true, true, true,
                    )
                    .expect("scripted macOS absence audit is complete");
                    self.absence_issuer
                        .mint_macos_zero_grants_and_unloaded(attempt, observed, audit)
                }
                ExtensionRuntimeRecoveryExpectation::Compatibility => {
                    let audit = ExtensionRuntimeCompatibilityAbsenceAudit::try_from_observations(
                        true, true, true, true,
                    )
                    .expect("scripted compatibility runtime is fully quiescent");
                    self.absence_issuer
                        .mint_compatibility_registry_absent_and_quiescent(attempt, audit)
                }
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension { .. }
                | ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension { .. } => None,
            };
            self.last_absence = absence;
            absence
        }
    }

    impl ExtensionRuntimeOwnershipPort for ScriptedOwnershipPort {
        fn retained_bytes(&self) -> usize {
            0
        }

        fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
            self.last_absence == Some(evidence)
                && self.absence_issuer.accepts(evidence, evidence.attempt())
        }

        fn retire_until(&mut self, deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
            let disposition = {
                let mut script = self.script.lock().unwrap();
                script.retire_calls += 1;
                script.retire_deadlines.push(deadline);
                script
                    .retire
                    .pop_front()
                    .expect("unexpected retirement call")
            };
            match disposition {
                ScriptedRetirement::Retired => self.mint_absence().map_or(
                    ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: None,
                    },
                    ExtensionRuntimeRetirementDisposition::Retired,
                ),
            }
        }

        fn reconcile_ownership_until(
            &mut self,
            deadline: Instant,
        ) -> ExtensionRuntimeOwnershipDisposition {
            let disposition = {
                let mut script = self.script.lock().unwrap();
                script.reconcile_calls += 1;
                script.reconcile_deadlines.push(deadline);
                script
                    .reconcile
                    .pop_front()
                    .expect("unexpected reconciliation call")
            };
            match disposition {
                ScriptedReconciliation::Disposition(disposition) => disposition,
                ScriptedReconciliation::Absent => self.mint_absence().map_or(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: None,
                    },
                    ExtensionRuntimeOwnershipDisposition::Absent,
                ),
            }
        }
    }

    impl ExtensionRuntimeHostOwnershipPort for ScriptedOwnershipPort {}

    fn factory(script: &Arc<Mutex<HostScript>>) -> ExtensionRuntimeHostFactory {
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(ScriptedHostFactoryPort {
            script: Arc::clone(script),
        }))
    }

    fn preparation(
        backend: ExtensionRuntimeBackendTarget,
        install: u128,
    ) -> ExtensionNativeOwnershipPreparation {
        ExtensionNativeOwnershipPreparation::new(
            ExtensionNativeOwnershipKey::new(
                ProfileId::from(1),
                ExtensionInstallId::from(install),
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionPackageIdentity::new(
                ExtensionAuthorityId::from_bytes([1; 32]),
                ExtensionPackageKey::from_bytes([2; 32]),
                ExtensionPackageRevision::INITIAL,
                ExtensionPackagePayloadIdentity::BundledTree,
                ExtensionManifestDigest::from_bytes([3; 32]),
                ExtensionTreeDigest::from_bytes([4; 32]),
            ),
            ExtensionCatalogSetDigest::from_bytes([5; 32]),
            ExtensionCatalogGenerationRole::Active,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
            ExtensionGrantRevision::INITIAL,
            ExtensionGrantDigest::from_bytes([6; 32]),
            backend,
        )
    }

    fn native_owner_bytes(install_id: ExtensionInstallId) -> [u8; 32] {
        let identifier_byte = b'a' + (install_id.bytes()[15] & 0x0f);
        [identifier_byte; 32]
    }

    fn expected_native_identity(
        backend: ExtensionRuntimeBackendTarget,
        install_id: ExtensionInstallId,
    ) -> ExtensionExpectedNativeOwnershipIdentity {
        ExtensionExpectedNativeOwnershipIdentity::from_encoded_bytes(
            backend,
            native_owner_bytes(install_id),
        )
        .unwrap()
    }

    fn possible_owner_transition(
        entry: &ExtensionNativeOwnershipEntry,
    ) -> ExtensionNativeOwnershipJournalMutation {
        match entry.runtime_backend() {
            ExtensionRuntimeBackendTarget::MacosNative
            | ExtensionRuntimeBackendTarget::WindowsNative => {
                ExtensionNativeOwnershipJournalMutation::transition_with_expected_native_identity(
                    entry.cas(),
                    expected_native_identity(entry.runtime_backend(), entry.key().install_id()),
                )
            }
            ExtensionRuntimeBackendTarget::MacosCompatibility
            | ExtensionRuntimeBackendTarget::LinuxCompatibility => {
                ExtensionNativeOwnershipJournalMutation::transition(
                    entry.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                )
            }
        }
    }

    fn possible_owner(backend: ExtensionRuntimeBackendTarget) -> ExtensionNativeOwnershipJournal {
        let journal = ExtensionNativeOwnershipJournal::empty();
        let revision = journal.revision();
        let journal = journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::begin(preparation(backend, 1)),
            )
            .unwrap()
            .into_journal();
        let entry = journal.entries()[0].clone();
        let revision = journal.revision();
        journal
            .apply(revision, possible_owner_transition(&entry))
            .unwrap()
            .into_journal()
    }

    fn possible_owner_with_observation(
        backend: ExtensionRuntimeBackendTarget,
        owner: ExtensionRuntimeNativeOwnerId,
    ) -> ExtensionNativeOwnershipJournal {
        let journal = possible_owner(backend);
        let entry = journal.entries()[0].clone();
        let observed =
            ExtensionNativeOwnershipIdentity::from_encoded_bytes(backend, owner.encoded_bytes())
                .unwrap();
        let revision = journal.revision();
        journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    entry.cas(),
                    entry.intent(),
                    entry.phase(),
                    observed,
                ),
            )
            .unwrap()
            .into_journal()
    }

    fn legacy_observed_only_owner(
        backend: ExtensionRuntimeBackendTarget,
        owner: ExtensionRuntimeNativeOwnerId,
    ) -> ExtensionNativeOwnershipJournal {
        let current = possible_owner_with_observation(backend, owner);
        let entry = &current.entries()[0];
        let legacy = ExtensionNativeOwnershipEntry::from_persisted_with_native_identities(
            entry.key(),
            entry.operation(),
            entry.revision(),
            entry.package().clone(),
            entry.catalog_set_digest(),
            entry.catalog_role(),
            entry.store_catalog_revision(),
            entry.store_install_revision(),
            entry.store_grant_revision(),
            entry.grant_digest(),
            entry.runtime_backend(),
            None,
            entry.native_identity(),
            entry.native_incarnation(),
            entry.intent(),
            entry.phase(),
        )
        .expect("legacy observed-only cleanup row must remain loadable");
        ExtensionNativeOwnershipJournal::from_persisted(
            current.revision(),
            current.operation_high_water(),
            current.native_incarnation_high_water(),
            vec![legacy],
        )
        .expect("legacy observed-only cleanup journal must remain loadable")
    }

    fn two_possible_owners(
        backend: ExtensionRuntimeBackendTarget,
    ) -> ExtensionNativeOwnershipJournal {
        let mut journal = ExtensionNativeOwnershipJournal::empty();
        for install in [1_u128, 2_u128] {
            let revision = journal.revision();
            journal = journal
                .apply(
                    revision,
                    ExtensionNativeOwnershipJournalMutation::begin(preparation(backend, install)),
                )
                .unwrap()
                .into_journal();
        }
        for install in [1_u128, 2_u128] {
            let key = preparation(backend, install).key();
            let entry = journal.get(key).unwrap().clone();
            let revision = journal.revision();
            journal = journal
                .apply(revision, possible_owner_transition(&entry))
                .unwrap()
                .into_journal();
        }
        journal
    }

    fn run(
        backend: &FakeJournalBackend,
        projection: &mut JournalProjection,
        repository: &mut FakeRepository,
        native: &mut NativeRecoveryState,
        cancellation: &impl CancellationCheck,
        deadline: Instant,
    ) -> CleanupStartupOutcome {
        reconcile_startup(
            backend,
            projection,
            repository,
            Some(native),
            crate::cleanup::CleanupAttempt::new(
                crate::cleanup::CleanupScope::All,
                cancellation,
                deadline,
            ),
            |_| {},
        )
    }

    #[test]
    fn profile_absence_wrapper_exposes_exact_frontier_and_never_skips_it() {
        let script = Arc::new(Mutex::new(HostScript::new()));
        let mut native = NativeRecoveryState::new(factory(&script));
        let journal = possible_owner(ExtensionRuntimeBackendTarget::MacosNative);
        let entry = journal.entries()[0].clone();
        let profile = entry.key().profile();

        assert!(matches!(native.begin(entry), NativeRecoveryStep::Progress));
        assert_eq!(native.frontier_profile(), Ok(Some(profile)));
        assert!(matches!(
            native.profile_absence_until(profile, Instant::now() + TEST_DEADLINE),
            Err(ExtensionRuntimeHostProfileAbsenceDisposition::ObligationsRemain)
        ));
        assert_eq!(script.lock().unwrap().profile_absence_calls, 0);
    }

    #[test]
    fn profile_absence_wrapper_fails_closed_on_inconsistent_pending_frontier_key() {
        let script = Arc::new(Mutex::new(HostScript::new()));
        let mut native = NativeRecoveryState::new(factory(&script));
        let journal = two_possible_owners(ExtensionRuntimeBackendTarget::MacosNative);
        let before = journal.entries()[0].clone();
        let after = journal.entries()[1].clone();
        let profile = before.key().profile();

        assert!(matches!(
            native.begin(before.clone()),
            NativeRecoveryStep::Progress
        ));
        native
            .frontier
            .as_mut()
            .expect("begin installs the exact frontier")
            .durable
            .pending = Some(PendingTransition {
            mutation: ExtensionNativeOwnershipJournalMutation::clear(before.cas()),
            before,
            after,
        });

        assert_eq!(
            native.frontier_profile(),
            Err(NativeRecoveryFailure::HostInvariant)
        );
        assert!(matches!(
            native.profile_absence_until(profile, Instant::now() + TEST_DEADLINE),
            Err(ExtensionRuntimeHostProfileAbsenceDisposition::InvariantFailed)
        ));
        assert_eq!(script.lock().unwrap().profile_absence_calls, 0);
    }

    #[test]
    fn profile_absence_wrapper_returns_factory_bound_evidence_without_a_frontier() {
        let profile = ProfileId::from(8);
        let script = Arc::new(Mutex::new(HostScript::new()));
        script.lock().unwrap().profile_absence.push_back(Ok(()));
        let mut native = NativeRecoveryState::new(factory(&script));

        let evidence = native
            .profile_absence_until(profile, Instant::now() + TEST_DEADLINE)
            .expect("empty local frontier and trusted host absence must mint evidence");
        assert!(evidence.is_for_profile(profile));
        assert_ne!(evidence.fence_generation(), 0);
        drop(evidence);
        assert_eq!(script.lock().unwrap().profile_absence_calls, 1);
    }

    #[test]
    fn panicking_native_call_leaves_a_sticky_fail_stop_obligation() {
        let script = Arc::new(Mutex::new(HostScript::new()));
        let journal = possible_owner(ExtensionRuntimeBackendTarget::LinuxCompatibility);
        let entry = journal.entries()[0].clone();
        let backend = FakeJournalBackend::new(journal);
        let mut projection = JournalProjection::unknown();
        let mut native = NativeRecoveryState::new(factory(&script));
        assert!(matches!(native.begin(entry), NativeRecoveryStep::Progress));

        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = native.advance(
                &backend,
                &mut projection,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            );
        }));

        assert!(result.is_err());
        assert!(native.has_attached_obligation());
    }

    #[test]
    fn scoped_cleanup_never_advances_an_unrelated_native_frontier() {
        let script = Arc::new(Mutex::new(HostScript::new()));
        let journal = possible_owner(ExtensionRuntimeBackendTarget::MacosNative);
        let entry = journal.entries()[0].clone();
        let backend = FakeJournalBackend::new(journal.clone());
        let mut projection = JournalProjection::unknown();
        let mut repository = FakeRepository::new();
        let mut native = NativeRecoveryState::new(factory(&script));
        assert!(matches!(native.begin(entry), NativeRecoveryStep::Progress));

        let outcome = reconcile_startup(
            &backend,
            &mut projection,
            &mut repository,
            Some(&mut native),
            crate::cleanup::CleanupAttempt::new(
                crate::cleanup::CleanupScope::Profile(ProfileId::from(2)),
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            |_| {},
        );

        assert_eq!(
            outcome,
            CleanupStartupOutcome::Ready {
                journal_revision: journal.revision(),
            }
        );
        assert!(native.has_frontier());
        assert_eq!(backend.durable(), journal);
        let script = script.lock().unwrap();
        assert_eq!(script.reconcile_calls, 0);
        assert_eq!(script.retire_calls, 0);
    }

    fn native_owner_id() -> ExtensionRuntimeNativeOwnerId {
        ExtensionRuntimeNativeOwnerId::from_encoded_bytes(native_owner_bytes(
            ExtensionInstallId::from(1),
        ))
        .unwrap()
    }

    fn conflicting_native_owner_id() -> ExtensionRuntimeNativeOwnerId {
        ExtensionRuntimeNativeOwnerId::from_encoded_bytes([b'p'; 32]).unwrap()
    }

    #[test]
    fn catalog_expectation_is_bound_but_never_persisted_as_adapter_observation() {
        let mut host = HostScript::new();
        host.reconcile
            .push_back(ScriptedReconciliation::Disposition(
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::TimedOut,
                    evidence: None,
                },
            ));
        let script = Arc::new(Mutex::new(host));
        let initial = possible_owner(ExtensionRuntimeBackendTarget::MacosNative);
        let expected = initial.entries()[0]
            .expected_native_identity()
            .expect("fresh native row carries the catalog expectation");
        let backend = FakeJournalBackend::new(initial);
        let mut projection = JournalProjection::unknown();
        let mut repository = FakeRepository::new();
        let mut native = NativeRecoveryState::new(factory(&script));

        assert_eq!(
            run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            CleanupStartupOutcome::Unavailable(CleanupUnavailable::DeadlineExpired)
        );

        let durable_journal = backend.durable();
        let durable = &durable_journal.entries()[0];
        assert_eq!(durable.expected_native_identity(), Some(expected));
        assert_eq!(durable.native_identity(), None);
        assert!(backend.mutation_entries().is_empty());
        let script = script.lock().unwrap();
        assert_eq!(
            script.recovery_expectations,
            vec![ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                catalog_expected: Some(native_owner_id()),
                adapter_observed: None,
            }]
        );
    }

    #[test]
    fn mismatched_observation_cannot_be_discharged_by_other_identity_absence() {
        let mismatched = conflicting_native_owner_id();
        let mut host = HostScript::new();
        host.reconcile.extend([
            ScriptedReconciliation::Disposition(
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::TimedOut,
                    evidence: Some(ExtensionRuntimeOwnershipEvidence::MacosWebExtension(
                        mismatched,
                    )),
                },
            ),
            ScriptedReconciliation::Absent,
        ]);
        let script = Arc::new(Mutex::new(host));
        let backend =
            FakeJournalBackend::new(possible_owner(ExtensionRuntimeBackendTarget::MacosNative));
        let mut projection = JournalProjection::unknown();
        let mut repository = FakeRepository::new();
        let mut native = NativeRecoveryState::new(factory(&script));

        assert_eq!(
            run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            CleanupStartupOutcome::Failed(crate::cleanup::CleanupFailure::NativeHostInvariant)
        );
        let after_durable_attachment = backend.durable();
        let entry = &after_durable_attachment.entries()[0];
        assert_eq!(
            entry.native_identity().map(|identity| identity.bytes()),
            Some(mismatched.encoded_bytes())
        );
        assert_eq!(entry.intent(), ExtensionNativeOwnershipIntent::Acquire);
        assert_eq!(entry.phase(), ExtensionNativeOwnershipPhase::NativeMayOwn);
        assert!(entry.expected_native_identity().is_some_and(|expected| {
            entry
                .native_identity()
                .is_some_and(|observed| !expected.matches_observed(observed))
        }));
        assert!(!evidence_matches_durable_expectation(
            entry,
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(mismatched),
        ));
        assert_eq!(repository.releases, 0);

        assert_eq!(
            run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            CleanupStartupOutcome::Failed(crate::cleanup::CleanupFailure::NativeHostInvariant)
        );
        assert_eq!(backend.durable(), after_durable_attachment);
        assert_eq!(repository.releases, 0);
        assert!(native.has_frontier());
        let script = script.lock().unwrap();
        assert_eq!(script.bind_calls, 1);
        assert_eq!(script.reconcile_calls, 2);
        assert_eq!(script.retire_calls, 0);
    }

    #[test]
    fn legacy_observed_only_native_row_remains_cleanup_capable() {
        let evidence = ExtensionRuntimeOwnershipEvidence::MacosWebExtension(native_owner_id());
        let mut host = HostScript::new();
        host.reconcile
            .push_back(ScriptedReconciliation::Disposition(
                ExtensionRuntimeOwnershipDisposition::Owned(evidence),
            ));
        host.retire.push_back(ScriptedRetirement::Retired);
        let script = Arc::new(Mutex::new(host));
        let initial = legacy_observed_only_owner(
            ExtensionRuntimeBackendTarget::MacosNative,
            native_owner_id(),
        );
        assert_eq!(initial.entries()[0].expected_native_identity(), None);
        assert!(initial.entries()[0].native_identity().is_some());
        let backend = FakeJournalBackend::new(initial);
        let mut projection = JournalProjection::unknown();
        let mut repository = FakeRepository::new();
        let mut native = NativeRecoveryState::new(factory(&script));

        assert!(matches!(
            run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            CleanupStartupOutcome::Ready { .. }
        ));
        assert!(backend.durable().entries().is_empty());
        assert_eq!(repository.releases, 1);
        let script = script.lock().unwrap();
        assert_eq!(script.bind_calls, 1);
        assert_eq!(script.reconcile_calls, 1);
        assert_eq!(script.retire_calls, 1);
        assert_eq!(
            script.recovery_expectations,
            vec![ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                catalog_expected: None,
                adapter_observed: Some(native_owner_id()),
            }]
        );
    }

    #[test]
    fn definite_absence_releases_the_exact_pin_and_clears_the_row() {
        let script = Arc::new(Mutex::new(HostScript::new()));
        script
            .lock()
            .unwrap()
            .reconcile
            .push_back(ScriptedReconciliation::Absent);
        let backend = FakeJournalBackend::new(possible_owner(
            ExtensionRuntimeBackendTarget::LinuxCompatibility,
        ));
        let mut projection = JournalProjection::unknown();
        let mut repository = FakeRepository::new();
        let mut native = NativeRecoveryState::new(factory(&script));

        let deadline = Instant::now() + TEST_DEADLINE;
        let outcome = run(
            &backend,
            &mut projection,
            &mut repository,
            &mut native,
            &NeverCancelled,
            deadline,
        );

        assert!(matches!(outcome, CleanupStartupOutcome::Ready { .. }));
        assert!(backend.durable().entries().is_empty());
        assert_eq!(repository.releases, 1);
        let script = script.lock().unwrap();
        assert_eq!(script.bind_calls, 1);
        assert_eq!(script.reconcile_calls, 1);
        assert_eq!(script.retire_calls, 0);
        assert!(!native.has_frontier());
    }

    #[test]
    fn multiple_rows_are_recovered_serially_with_one_host_reservation_each() {
        let mut host = HostScript::new();
        host.reconcile.extend([
            ScriptedReconciliation::Absent,
            ScriptedReconciliation::Absent,
        ]);
        let script = Arc::new(Mutex::new(host));
        let backend = FakeJournalBackend::new(two_possible_owners(
            ExtensionRuntimeBackendTarget::LinuxCompatibility,
        ));
        let mut projection = JournalProjection::unknown();
        let mut repository = FakeRepository::new();
        let mut native = NativeRecoveryState::new(factory(&script));

        assert!(matches!(
            run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            CleanupStartupOutcome::Ready { .. }
        ));

        assert!(backend.durable().entries().is_empty());
        assert_eq!(repository.releases, 2);
        let script = script.lock().unwrap();
        assert_eq!(script.bind_calls, 2);
        assert_eq!(script.reconcile_calls, 2);
        assert_eq!(script.retire_calls, 0);
    }

    #[test]
    fn ambiguous_observation_attachment_reloads_exact_after_state_without_rebinding() {
        let mut host = HostScript::new();
        host.reconcile
            .push_back(ScriptedReconciliation::Disposition(
                ExtensionRuntimeOwnershipDisposition::Owned(
                    ExtensionRuntimeOwnershipEvidence::MacosWebExtension(native_owner_id()),
                ),
            ));
        host.retire.push_back(ScriptedRetirement::Retired);
        let script = Arc::new(Mutex::new(host));
        let backend =
            FakeJournalBackend::new(possible_owner(ExtensionRuntimeBackendTarget::MacosNative))
                .with_mutation_actions([MutationAction::ApplyButReportUnknown]);
        let mut projection = JournalProjection::unknown();
        let mut repository = FakeRepository::new();
        let mut native = NativeRecoveryState::new(factory(&script));

        let deadline = Instant::now() + TEST_DEADLINE;
        let outcome = run(
            &backend,
            &mut projection,
            &mut repository,
            &mut native,
            &NeverCancelled,
            deadline,
        );

        assert!(matches!(outcome, CleanupStartupOutcome::Ready { .. }));
        let mutations = backend.mutation_entries();
        assert!(mutations.len() >= 3);
        assert_eq!(
            mutations[0]
                .native_identity()
                .map(|identity| identity.bytes()),
            Some(native_owner_id().encoded_bytes())
        );
        assert!(mutations[0]
            .expected_native_identity()
            .is_some_and(|expected| mutations[0]
                .native_identity()
                .is_some_and(|observed| expected.matches_observed(observed))));
        assert_eq!(
            mutations[0].intent(),
            ExtensionNativeOwnershipIntent::Acquire
        );
        assert_eq!(
            mutations[1].intent(),
            ExtensionNativeOwnershipIntent::Release
        );
        assert_eq!(
            mutations[1].phase(),
            ExtensionNativeOwnershipPhase::NativeMayOwn
        );
        let script = script.lock().unwrap();
        assert_eq!(script.bind_calls, 1);
        assert_eq!(script.reconcile_calls, 1);
        assert_eq!(script.retire_calls, 1);
        assert_eq!(script.reconcile_deadlines, vec![deadline]);
        assert_eq!(script.retire_deadlines, vec![deadline]);
    }

    #[test]
    fn uncertain_request_and_authenticated_identity_survive_retry_without_rebinding() {
        let mut host = HostScript::new();
        host.reconcile.extend([
            ScriptedReconciliation::Disposition(
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::TimedOut,
                    evidence: Some(ExtensionRuntimeOwnershipEvidence::MacosWebExtension(
                        native_owner_id(),
                    )),
                },
            ),
            ScriptedReconciliation::Absent,
        ]);
        let script = Arc::new(Mutex::new(host));
        let backend =
            FakeJournalBackend::new(possible_owner(ExtensionRuntimeBackendTarget::MacosNative));
        let mut projection = JournalProjection::unknown();
        let mut repository = FakeRepository::new();
        let mut native = NativeRecoveryState::new(factory(&script));

        assert_eq!(
            run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            CleanupStartupOutcome::Unavailable(CleanupUnavailable::DeadlineExpired)
        );
        assert!(native.has_attached_obligation());
        assert_eq!(
            backend.durable().entries()[0]
                .native_identity()
                .map(|identity| identity.bytes()),
            Some(native_owner_id().encoded_bytes())
        );

        assert_eq!(
            run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &AlwaysCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            CleanupStartupOutcome::Unavailable(CleanupUnavailable::Cancelled)
        );
        assert!(native.has_attached_obligation());
        assert_eq!(script.lock().unwrap().reconcile_calls, 1);

        assert!(matches!(
            run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            CleanupStartupOutcome::Ready { .. }
        ));
        let script = script.lock().unwrap();
        assert_eq!(script.bind_calls, 1);
        assert_eq!(script.reconcile_calls, 2);
        assert_eq!(
            backend
                .mutation_entries()
                .iter()
                .filter(|entry| {
                    entry.intent() == ExtensionNativeOwnershipIntent::Acquire
                        && entry.phase() == ExtensionNativeOwnershipPhase::NativeMayOwn
                        && entry.native_identity().is_some()
                })
                .count(),
            1
        );
    }

    #[test]
    fn stale_store_row_after_ambiguous_native_call_fails_closed() {
        let mut host = HostScript::new();
        host.reconcile
            .push_back(ScriptedReconciliation::Disposition(
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::BackendUnavailable,
                    evidence: None,
                },
            ));
        let script = Arc::new(Mutex::new(host));
        let backend = FakeJournalBackend::new(possible_owner(
            ExtensionRuntimeBackendTarget::LinuxCompatibility,
        ))
        .with_loads([
            LoadAction::Current,
            LoadAction::Current,
            LoadAction::Replace(ExtensionNativeOwnershipJournal::empty()),
        ]);
        let mut projection = JournalProjection::unknown();
        let mut repository = FakeRepository::new();
        let mut native = NativeRecoveryState::new(factory(&script));

        assert_eq!(
            run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            CleanupStartupOutcome::Failed(crate::cleanup::CleanupFailure::StoreProjectionMismatch)
        );
        assert!(native.has_attached_obligation());
        assert_eq!(script.lock().unwrap().reconcile_calls, 1);
        assert_eq!(repository.releases, 0);
    }

    #[test]
    fn unsupported_or_capacity_refusal_never_claims_absence() {
        for (error, unavailable) in [
            (ExtensionRuntimeHostBindError::UnsupportedBackend, false),
            (ExtensionRuntimeHostBindError::CapacityExceeded, true),
        ] {
            let mut host = HostScript::new();
            host.bind_error = Some(error);
            let script = Arc::new(Mutex::new(host));
            let initial = possible_owner(ExtensionRuntimeBackendTarget::LinuxCompatibility);
            let backend = FakeJournalBackend::new(initial.clone());
            let mut projection = JournalProjection::unknown();
            let mut repository = FakeRepository::new();
            let mut native = NativeRecoveryState::new(factory(&script));

            let outcome = run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            );
            if unavailable {
                assert_eq!(
                    outcome,
                    CleanupStartupOutcome::Unavailable(CleanupUnavailable::NativeRuntimeCapacity)
                );
            } else {
                assert!(matches!(
                    outcome,
                    CleanupStartupOutcome::CleanupRequired {
                        possible_owner_count: 1,
                        ..
                    }
                ));
            }
            assert_eq!(backend.durable(), initial);
            assert!(!native.has_frontier());
            assert_eq!(repository.releases, 0);
        }
    }

    #[test]
    fn cancellation_and_expired_deadline_do_not_bind_or_call_native_code() {
        for (cancelled, deadline) in [
            (true, Instant::now() + TEST_DEADLINE),
            (false, Instant::now()),
        ] {
            let script = Arc::new(Mutex::new(HostScript::new()));
            let backend = FakeJournalBackend::new(possible_owner(
                ExtensionRuntimeBackendTarget::LinuxCompatibility,
            ));
            let mut projection = JournalProjection::unknown();
            let mut repository = FakeRepository::new();
            let mut native = NativeRecoveryState::new(factory(&script));
            let outcome = if cancelled {
                run(
                    &backend,
                    &mut projection,
                    &mut repository,
                    &mut native,
                    &AlwaysCancelled,
                    deadline,
                )
            } else {
                run(
                    &backend,
                    &mut projection,
                    &mut repository,
                    &mut native,
                    &NeverCancelled,
                    deadline,
                )
            };
            assert!(matches!(outcome, CleanupStartupOutcome::Unavailable(_)));
            let script = script.lock().unwrap();
            assert_eq!(script.bind_calls, 0);
            assert_eq!(script.reconcile_calls, 0);
            assert_eq!(script.retire_calls, 0);
        }
    }

    #[test]
    fn shutdown_style_drain_settles_an_attached_request_or_retains_it_fail_closed() {
        let mut host = HostScript::new();
        host.reconcile.extend([
            ScriptedReconciliation::Disposition(
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::TimedOut,
                    evidence: None,
                },
            ),
            ScriptedReconciliation::Absent,
        ]);
        let script = Arc::new(Mutex::new(host));
        let backend = FakeJournalBackend::new(possible_owner(
            ExtensionRuntimeBackendTarget::LinuxCompatibility,
        ));
        let mut projection = JournalProjection::unknown();
        let mut repository = FakeRepository::new();
        let mut native = NativeRecoveryState::new(factory(&script));

        assert!(matches!(
            run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            CleanupStartupOutcome::Unavailable(_)
        ));
        assert!(native.has_attached_obligation());

        assert!(matches!(
            run(
                &backend,
                &mut projection,
                &mut repository,
                &mut native,
                &NeverCancelled,
                Instant::now() + TEST_DEADLINE,
            ),
            CleanupStartupOutcome::Ready { .. }
        ));
        assert!(!native.has_attached_obligation());
        assert_eq!(repository.releases, 1);
    }
}
