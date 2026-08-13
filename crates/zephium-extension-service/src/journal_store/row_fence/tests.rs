use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use super::*;
use crate::journal_store::JournalMutationFailure;
use zephium_core::extensions::{
    ExtensionAuthorityId, ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
    ExtensionExpectedNativeOwnershipIdentity, ExtensionGrantBrowsingContext, ExtensionGrantDigest,
    ExtensionGrantRevision, ExtensionInstallCatalogRevision, ExtensionInstallRevision,
    ExtensionManifestDigest, ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPreparation,
    ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackagePayloadIdentity,
    ExtensionPackageRevision, ExtensionRuntimeBackendTarget, ExtensionTreeDigest,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_core::ports::store::{
    ExtensionNativeOwnershipJournalLoadOutcome, ExtensionNativeOwnershipJournalMutationApplied,
};

type Call<T> = ExtensionServiceStoreCallOutcome<T>;

struct MutationStep {
    expected_journal: ExtensionNativeOwnershipJournal,
    expected_mutation: ExtensionNativeOwnershipJournalMutation,
    outcome: Call<ExtensionNativeOwnershipJournalMutationOutcome>,
}

struct Backend {
    loads: RefCell<VecDeque<Call<ExtensionNativeOwnershipJournalLoadOutcome>>>,
    mutations: RefCell<VecDeque<MutationStep>>,
    mutation_calls: Cell<usize>,
    expected_deadline: Option<Instant>,
}

impl Backend {
    fn new(
        loads: impl IntoIterator<Item = Call<ExtensionNativeOwnershipJournalLoadOutcome>>,
        mutations: impl IntoIterator<Item = MutationStep>,
    ) -> Self {
        Self {
            loads: RefCell::new(loads.into_iter().collect()),
            mutations: RefCell::new(mutations.into_iter().collect()),
            mutation_calls: Cell::new(0),
            expected_deadline: None,
        }
    }

    fn with_expected_deadline(mut self, deadline: Instant) -> Self {
        self.expected_deadline = Some(deadline);
        self
    }

    fn assert_drained(&self) {
        assert!(self.loads.borrow().is_empty(), "unused scripted load");
        assert!(
            self.mutations.borrow().is_empty(),
            "unused scripted mutation"
        );
    }
}

impl JournalBackend for Backend {
    fn load_until(&self, _deadline: Instant) -> Call<ExtensionNativeOwnershipJournalLoadOutcome> {
        self.loads
            .borrow_mut()
            .pop_front()
            .expect("unexpected journal load")
    }

    fn mutate_until(
        &self,
        journal: &ExtensionNativeOwnershipJournal,
        mutation: ExtensionNativeOwnershipJournalMutation,
        deadline: Instant,
    ) -> Call<ExtensionNativeOwnershipJournalMutationOutcome> {
        self.mutation_calls.set(self.mutation_calls.get() + 1);
        if let Some(expected_deadline) = self.expected_deadline {
            assert_eq!(deadline, expected_deadline);
        }
        let step = self
            .mutations
            .borrow_mut()
            .pop_front()
            .expect("unexpected row mutation");
        assert_eq!(journal, &step.expected_journal);
        assert_eq!(mutation, step.expected_mutation);
        step.outcome
    }
}

struct RejectingBackend {
    mutation_calls: Cell<usize>,
}

impl RejectingBackend {
    const fn new() -> Self {
        Self {
            mutation_calls: Cell::new(0),
        }
    }
}

impl JournalBackend for RejectingBackend {
    fn load_until(&self, _deadline: Instant) -> Call<ExtensionNativeOwnershipJournalLoadOutcome> {
        panic!("unexpected journal load")
    }

    fn mutate_until(
        &self,
        _journal: &ExtensionNativeOwnershipJournal,
        _mutation: ExtensionNativeOwnershipJournalMutation,
        _deadline: Instant,
    ) -> Call<ExtensionNativeOwnershipJournalMutationOutcome> {
        self.mutation_calls.set(self.mutation_calls.get() + 1);
        Call::NotAdmitted
    }
}

#[derive(Clone, Copy)]
enum State {
    Preparing,
    MayOwnUnidentified,
    MayOwnIdentified,
    Owned,
    ReleaseMayOwn,
    ReleaseAbsent,
}

fn preparation() -> ExtensionNativeOwnershipPreparation {
    ExtensionNativeOwnershipPreparation::new(
        ExtensionNativeOwnershipKey::new(
            ProfileId::from(1),
            ExtensionInstallId::from(1),
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
        ExtensionRuntimeBackendTarget::MacosNative,
    )
}

fn expected_identity() -> ExtensionExpectedNativeOwnershipIdentity {
    ExtensionExpectedNativeOwnershipIdentity::parse(
        ExtensionRuntimeBackendTarget::MacosNative,
        "abcdefghijklmnopabcdefghijklmnop",
    )
    .unwrap()
}

fn native_identity() -> ExtensionNativeOwnershipIdentity {
    ExtensionNativeOwnershipIdentity::parse(
        ExtensionRuntimeBackendTarget::MacosNative,
        "abcdefghijklmnopabcdefghijklmnop",
    )
    .unwrap()
}

fn apply(
    journal: ExtensionNativeOwnershipJournal,
    mutation: ExtensionNativeOwnershipJournalMutation,
) -> (
    ExtensionNativeOwnershipJournal,
    ExtensionNativeOwnershipJournalMutationApplied,
) {
    let revision = journal.revision();
    let application = journal.apply(revision, mutation).unwrap();
    let applied = ExtensionNativeOwnershipJournalMutationApplied {
        journal_revision: application.journal().revision(),
        operation_high_water: application.journal().operation_high_water(),
        native_incarnation_high_water: application.journal().native_incarnation_high_water(),
        grant_rebind_count: application.journal().grant_rebind_count(),
        entry: application.entry().cloned().map(Box::new),
    };
    (application.into_journal(), applied)
}

fn fixture(
    state: State,
) -> (
    ExtensionNativeOwnershipJournal,
    ExtensionNativeOwnershipEntry,
) {
    let (preparing, _) = apply(
        ExtensionNativeOwnershipJournal::empty(),
        ExtensionNativeOwnershipJournalMutation::begin(preparation()),
    );
    if matches!(state, State::Preparing) {
        return (preparing.clone(), preparing.entries()[0].clone());
    }
    let preparing_row = preparing.entries()[0].clone();
    let (may_own, _) = apply(
        preparing,
        ExtensionNativeOwnershipJournalMutation::transition_with_expected_native_identity(
            preparing_row.cas(),
            expected_identity(),
        ),
    );
    if matches!(state, State::MayOwnUnidentified) {
        return (may_own.clone(), may_own.entries()[0].clone());
    }
    let may_own_row = may_own.entries()[0].clone();
    let (identified, _) = apply(
        may_own,
        ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
            may_own_row.cas(),
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
            native_identity(),
        ),
    );
    if matches!(state, State::MayOwnIdentified) {
        return (identified.clone(), identified.entries()[0].clone());
    }
    let identified_row = identified.entries()[0].clone();
    let (owned, _) = apply(
        identified,
        ExtensionNativeOwnershipJournalMutation::transition(
            identified_row.cas(),
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeOwned,
        ),
    );
    if matches!(state, State::Owned) {
        return (owned.clone(), owned.entries()[0].clone());
    }
    let owned_row = owned.entries()[0].clone();
    let (release_may_own, _) = apply(
        owned,
        ExtensionNativeOwnershipJournalMutation::transition(
            owned_row.cas(),
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        ),
    );
    if matches!(state, State::ReleaseMayOwn) {
        return (
            release_may_own.clone(),
            release_may_own.entries()[0].clone(),
        );
    }
    let release_row = release_may_own.entries()[0].clone();
    let (release_absent, _) = apply(
        release_may_own,
        ExtensionNativeOwnershipJournalMutation::transition(
            release_row.cas(),
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
        ),
    );
    (release_absent.clone(), release_absent.entries()[0].clone())
}

fn loaded(
    journal: ExtensionNativeOwnershipJournal,
) -> Call<ExtensionNativeOwnershipJournalLoadOutcome> {
    Call::Completed(ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal))
}

fn completed(
    outcome: ExtensionNativeOwnershipJournalMutationOutcome,
) -> Call<ExtensionNativeOwnershipJournalMutationOutcome> {
    Call::Completed(outcome)
}

fn known_projection(journal: ExtensionNativeOwnershipJournal) -> JournalProjection {
    JournalProjection {
        journal: Some(journal),
    }
}

fn settled(result: Result<RowFenceSettlement, RowFencePreparationFailure>) -> RowFenceSettlement {
    match result {
        Ok(settlement) => settlement,
        Err(failure) => panic!("row-fence preparation failed: {:?}", failure.reason()),
    }
}

fn rejected(
    result: Result<RowFenceSettlement, RowFencePreparationFailure>,
) -> RowFencePreparationFailure {
    match result {
        Err(failure) => failure,
        Ok(_) => panic!("row-fence preparation unexpectedly succeeded"),
    }
}

fn transition_mutation(
    row: &ExtensionNativeOwnershipEntry,
    intent: ExtensionNativeOwnershipIntent,
    phase: ExtensionNativeOwnershipPhase,
) -> ExtensionNativeOwnershipJournalMutation {
    ExtensionNativeOwnershipJournalMutation::transition(row.cas(), intent, phase)
}

fn assert_transition_applied(
    state: State,
    next_intent: ExtensionNativeOwnershipIntent,
    next_phase: ExtensionNativeOwnershipPhase,
) {
    let (journal, row) = fixture(state);
    let mutation = transition_mutation(&row, next_intent, next_phase);
    let (predicted, applied) = apply(journal.clone(), mutation.clone());
    let deadline = Instant::now();
    let backend = Backend::new(
        [],
        [MutationStep {
            expected_journal: journal.clone(),
            expected_mutation: mutation,
            outcome: completed(ExtensionNativeOwnershipJournalMutationOutcome::Applied(
                applied,
            )),
        }],
    )
    .with_expected_deadline(deadline);
    let mut projection = known_projection(journal);

    let settlement = settled(projection.settle_exact_row_transition(
        &backend,
        &row,
        next_intent,
        next_phase,
        0,
        deadline,
    ));
    assert!(settlement.additional_retained_bytes() <= MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES);
    let RowFenceSettlement::Applied(settled) = settlement else {
        panic!("allowed transition did not apply");
    };
    assert_eq!(settled.entry(), predicted.entries().first());
    assert_eq!(settled.journal_revision(), predicted.revision());
    assert_eq!(projection.known(), Some(&predicted));
    backend.assert_drained();
}

#[test]
fn every_allowed_lifecycle_edge_is_fenced_and_deadline_exact() {
    use ExtensionNativeOwnershipIntent::{Acquire, Release};
    use ExtensionNativeOwnershipPhase::{NativeAbsentReleasePending, NativeMayOwn, NativeOwned};

    for (state, intent, phase) in [
        (State::MayOwnIdentified, Acquire, NativeOwned),
        (State::MayOwnUnidentified, Release, NativeMayOwn),
        (State::MayOwnIdentified, Release, NativeMayOwn),
        (State::Preparing, Release, NativeAbsentReleasePending),
        (
            State::MayOwnUnidentified,
            Release,
            NativeAbsentReleasePending,
        ),
        (State::MayOwnIdentified, Release, NativeAbsentReleasePending),
        (State::Owned, Release, NativeMayOwn),
        (State::ReleaseMayOwn, Release, NativeAbsentReleasePending),
    ] {
        assert_transition_applied(state, intent, phase);
    }

    let (journal, row) = fixture(State::MayOwnUnidentified);
    let mutation = ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
        row.cas(),
        row.intent(),
        row.phase(),
        native_identity(),
    );
    let (predicted, applied) = apply(journal.clone(), mutation.clone());
    let deadline = Instant::now();
    let backend = Backend::new(
        [],
        [MutationStep {
            expected_journal: journal.clone(),
            expected_mutation: mutation,
            outcome: completed(ExtensionNativeOwnershipJournalMutationOutcome::Applied(
                applied,
            )),
        }],
    )
    .with_expected_deadline(deadline);
    let mut projection = known_projection(journal);
    let RowFenceSettlement::Applied(applied) =
        settled(projection.settle_native_identity_attachment(
            &backend,
            &row,
            native_identity(),
            0,
            deadline,
        ))
    else {
        panic!("identity attachment did not apply");
    };
    assert_eq!(applied.entry(), predicted.entries().first());
    backend.assert_drained();

    let (journal, row) = fixture(State::ReleaseAbsent);
    let mutation = ExtensionNativeOwnershipJournalMutation::clear(row.cas());
    let (predicted, applied) = apply(journal.clone(), mutation.clone());
    let backend = Backend::new(
        [],
        [MutationStep {
            expected_journal: journal.clone(),
            expected_mutation: mutation,
            outcome: completed(ExtensionNativeOwnershipJournalMutationOutcome::Applied(
                applied,
            )),
        }],
    );
    let mut projection = known_projection(journal);
    let RowFenceSettlement::Applied(applied) =
        settled(projection.settle_exact_row_clear(&backend, &row, 0, Instant::now()))
    else {
        panic!("exact release-pending clear did not apply");
    };
    assert!(applied.entry().is_none());
    assert_eq!(projection.known(), Some(&predicted));
    backend.assert_drained();
}

#[test]
fn every_other_state_edge_identity_attachment_and_clear_are_store_free() {
    use ExtensionNativeOwnershipIntent::{Acquire, Release};
    use ExtensionNativeOwnershipPhase::{
        NativeAbsentPreparing, NativeAbsentReleasePending, NativeMayOwn, NativeOwned,
    };

    let backend = RejectingBackend::new();
    let states = [
        State::Preparing,
        State::MayOwnUnidentified,
        State::MayOwnIdentified,
        State::Owned,
        State::ReleaseMayOwn,
        State::ReleaseAbsent,
    ];
    let targets = [
        (Acquire, NativeAbsentPreparing),
        (Acquire, NativeMayOwn),
        (Acquire, NativeOwned),
        (Acquire, NativeAbsentReleasePending),
        (Release, NativeAbsentPreparing),
        (Release, NativeMayOwn),
        (Release, NativeOwned),
        (Release, NativeAbsentReleasePending),
    ];
    for state in states {
        let (journal, row) = fixture(state);
        for (intent, phase) in targets {
            if allowed_row_transition(&row, intent, phase) {
                continue;
            }
            let before_calls = backend.mutation_calls.get();
            let mut projection = known_projection(journal.clone());
            let failure = rejected(projection.settle_exact_row_transition(
                &backend,
                &row,
                intent,
                phase,
                0,
                Instant::now(),
            ));
            assert_eq!(
                failure.reason(),
                RowFencePreparationFailureReason::ForbiddenLifecycleEdge
            );
            assert_eq!(backend.mutation_calls.get(), before_calls);
            assert_eq!(projection.known(), Some(&journal));
        }
    }

    for state in [
        State::Preparing,
        State::Owned,
        State::ReleaseMayOwn,
        State::ReleaseAbsent,
    ] {
        let (journal, row) = fixture(state);
        let mut projection = known_projection(journal.clone());
        assert_eq!(
            rejected(projection.settle_native_identity_attachment(
                &backend,
                &row,
                native_identity(),
                0,
                Instant::now(),
            ))
            .reason(),
            RowFencePreparationFailureReason::ForbiddenLifecycleEdge
        );
        assert_eq!(projection.known(), Some(&journal));
    }

    for state in [
        State::Preparing,
        State::MayOwnIdentified,
        State::Owned,
        State::ReleaseMayOwn,
    ] {
        let (journal, row) = fixture(state);
        let mut projection = known_projection(journal.clone());
        assert_eq!(
            rejected(projection.settle_exact_row_clear(&backend, &row, 0, Instant::now(),))
                .reason(),
            RowFencePreparationFailureReason::ForbiddenLifecycleEdge
        );
        assert_eq!(projection.known(), Some(&journal));
    }
    assert_eq!(backend.mutation_calls.get(), 0);
}

#[test]
fn native_identity_invariants_are_validated_before_store_admission() {
    let backend = RejectingBackend::new();

    let (unidentified_journal, unidentified) = fixture(State::MayOwnUnidentified);
    let mut projection = known_projection(unidentified_journal.clone());
    assert_eq!(
        rejected(projection.settle_exact_row_transition(
            &backend,
            &unidentified,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeOwned,
            0,
            Instant::now(),
        ))
        .reason(),
        RowFencePreparationFailureReason::InvalidLocalTransition
    );
    assert_eq!(projection.known(), Some(&unidentified_journal));

    let (identified_journal, identified) = fixture(State::MayOwnIdentified);
    let mut projection = known_projection(identified_journal.clone());
    assert_eq!(
        rejected(projection.settle_native_identity_attachment(
            &backend,
            &identified,
            native_identity(),
            0,
            Instant::now(),
        ))
        .reason(),
        RowFencePreparationFailureReason::InvalidLocalTransition
    );
    assert_eq!(projection.known(), Some(&identified_journal));
    assert_eq!(backend.mutation_calls.get(), 0);
}

#[test]
fn retained_limit_overflow_unknown_and_row_mismatch_are_pre_store() {
    let (journal, row) = fixture(State::Owned);
    let boundary = MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        .checked_sub(MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES)
        .unwrap();
    let mutation = transition_mutation(
        &row,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
    );
    let backend = Backend::new(
        [],
        [MutationStep {
            expected_journal: journal.clone(),
            expected_mutation: mutation,
            outcome: Call::NotAdmitted,
        }],
    );
    let mut projection = known_projection(journal.clone());
    assert!(matches!(
        settled(projection.settle_exact_row_transition(
            &backend,
            &row,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
            boundary,
            Instant::now(),
        )),
        RowFenceSettlement::NotAdmitted
    ));
    assert_eq!(projection.known(), Some(&journal));
    backend.assert_drained();

    let backend = RejectingBackend::new();
    for (retained, reason) in [
        (
            boundary + 1,
            RowFencePreparationFailureReason::RetainedBytesExceeded,
        ),
        (
            usize::MAX,
            RowFencePreparationFailureReason::RetainedBytesOverflow,
        ),
    ] {
        let mut projection = known_projection(journal.clone());
        assert_eq!(
            rejected(projection.settle_exact_row_transition(
                &backend,
                &row,
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
                retained,
                Instant::now(),
            ))
            .reason(),
            reason
        );
        assert_eq!(projection.known(), Some(&journal));
    }

    let mut unknown = JournalProjection::unknown();
    assert_eq!(
        rejected(unknown.settle_exact_row_transition(
            &backend,
            &row,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
            0,
            Instant::now(),
        ))
        .reason(),
        RowFencePreparationFailureReason::ReloadRequired
    );

    let (other_journal, other_row) = fixture(State::Preparing);
    let mut projection = known_projection(other_journal.clone());
    assert_eq!(
        rejected(projection.settle_exact_row_transition(
            &backend,
            &row,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
            0,
            Instant::now(),
        ))
        .reason(),
        RowFencePreparationFailureReason::CurrentRowMismatch
    );
    assert_ne!(row, other_row);
    assert_eq!(projection.known(), Some(&other_journal));
    assert_eq!(backend.mutation_calls.get(), 0);
}

#[test]
fn non_admission_and_every_definite_refusal_retain_projection() {
    let refusals = [
        (
            ExtensionNativeOwnershipJournalMutationOutcome::NotRegistered,
            RowFenceRefusalReason::NotRegistered,
        ),
        (
            ExtensionNativeOwnershipJournalMutationOutcome::DegradedProfile,
            RowFenceRefusalReason::DegradedProfile,
        ),
        (
            ExtensionNativeOwnershipJournalMutationOutcome::SessionRecoveryRequired,
            RowFenceRefusalReason::SessionRecoveryRequired,
        ),
        (
            ExtensionNativeOwnershipJournalMutationOutcome::Invalid,
            RowFenceRefusalReason::Invalid,
        ),
        (
            ExtensionNativeOwnershipJournalMutationOutcome::LimitReached,
            RowFenceRefusalReason::LimitReached,
        ),
        (
            ExtensionNativeOwnershipJournalMutationOutcome::RevisionExhausted,
            RowFenceRefusalReason::RevisionExhausted,
        ),
        (
            ExtensionNativeOwnershipJournalMutationOutcome::Failed,
            RowFenceRefusalReason::Failed,
        ),
    ];
    for (outcome, reason) in refusals {
        let (journal, row) = fixture(State::Owned);
        let mutation = transition_mutation(
            &row,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        );
        let backend = Backend::new(
            [],
            [MutationStep {
                expected_journal: journal.clone(),
                expected_mutation: mutation,
                outcome: completed(outcome),
            }],
        );
        let mut projection = known_projection(journal.clone());
        let RowFenceSettlement::Refused(refused) = settled(projection.settle_exact_row_transition(
            &backend,
            &row,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
            0,
            Instant::now(),
        )) else {
            panic!("definite Store refusal was not preserved");
        };
        assert_eq!(refused.reason(), reason);
        assert_eq!(projection.known(), Some(&journal));
        backend.assert_drained();
    }
}

fn assert_ambiguous_outcome_reconciles_after(
    outcome: Call<ExtensionNativeOwnershipJournalMutationOutcome>,
) {
    let (journal, row) = fixture(State::Owned);
    let mutation = transition_mutation(
        &row,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
    );
    let (after, _) = apply(journal.clone(), mutation.clone());
    let backend = Backend::new(
        [loaded(after.clone())],
        [MutationStep {
            expected_journal: journal.clone(),
            expected_mutation: mutation,
            outcome,
        }],
    );
    let mut projection = known_projection(journal);
    let RowFenceSettlement::ReloadRequired(unsettled) =
        settled(projection.settle_exact_row_transition(
            &backend,
            &row,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
            0,
            Instant::now(),
        ))
    else {
        panic!("ambiguous Store outcome did not retain evidence");
    };
    assert!(projection.known().is_none());
    assert!(
        unsettled.additional_retained_bytes()
            <= RowFenceReloadRequired::maximum_additional_retained_bytes()
    );
    assert_eq!(projection.reload(&backend, Instant::now()), Ok(&after));
    let reconciliation = unsettled.reconcile(&projection);
    assert!(reconciliation.additional_retained_bytes() <= MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES);
    let RowFenceReconciliation::Applied(applied) = reconciliation else {
        panic!("exact after frontier did not reconcile");
    };
    assert_eq!(applied.entry(), after.entries().first());
    backend.assert_drained();
}

#[test]
fn mismatched_applied_outcome_unknown_and_timeout_are_true_ambiguity() {
    let (journal, row) = fixture(State::Owned);
    let mutation = transition_mutation(
        &row,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
    );
    let (_, mut mismatched) = apply(journal, mutation);
    mismatched.entry = None;

    for outcome in [
        completed(ExtensionNativeOwnershipJournalMutationOutcome::Applied(
            mismatched,
        )),
        completed(ExtensionNativeOwnershipJournalMutationOutcome::OutcomeUnknown),
        Call::TimedOutAfterAdmission,
    ] {
        assert_ambiguous_outcome_reconciles_after(outcome);
    }
}

#[test]
fn ambiguity_reconciles_before_pending_and_both_kinds_of_third_frontier() {
    let (before, row) = fixture(State::MayOwnIdentified);
    let mutation = transition_mutation(
        &row,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
    );
    let (after, _) = apply(before.clone(), mutation.clone());

    let make_unsettled = |loads| {
        let backend = Backend::new(
            loads,
            [MutationStep {
                expected_journal: before.clone(),
                expected_mutation: mutation.clone(),
                outcome: completed(ExtensionNativeOwnershipJournalMutationOutcome::OutcomeUnknown),
            }],
        );
        let mut projection = known_projection(before.clone());
        let RowFenceSettlement::ReloadRequired(unsettled) =
            settled(projection.settle_exact_row_transition(
                &backend,
                &row,
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
                0,
                Instant::now(),
            ))
        else {
            unreachable!();
        };
        (backend, projection, unsettled)
    };

    let (backend, mut projection, unsettled) = make_unsettled(vec![loaded(before.clone())]);
    assert_eq!(projection.reload(&backend, Instant::now()), Ok(&before));
    let RowFenceReconciliation::NotApplied(not_applied) = unsettled.reconcile(&projection) else {
        panic!("exact before frontier did not reconcile");
    };
    assert_eq!(not_applied.entry(), &row);
    backend.assert_drained();

    let (backend, projection, unsettled) = make_unsettled(Vec::new());
    let RowFenceReconciliation::Pending(pending) = unsettled.reconcile(&projection) else {
        panic!("unknown projection did not remain pending");
    };
    assert!(pending.additional_retained_bytes() <= MAX_ROW_FENCE_ADDITIONAL_RETAINED_BYTES);
    backend.assert_drained();

    // Same global clocks as predicted-after, but a different affected row.
    let alternative_mutation = transition_mutation(
        &row,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
    );
    let (same_clocks_different_row, _) = apply(before.clone(), alternative_mutation);
    assert_eq!(same_clocks_different_row.revision(), after.revision());
    let (backend, mut projection, unsettled) =
        make_unsettled(vec![loaded(same_clocks_different_row.clone())]);
    assert_eq!(
        projection.reload(&backend, Instant::now()),
        Ok(&same_clocks_different_row)
    );
    let RowFenceReconciliation::Diverged(diverged) = unsettled.reconcile(&projection) else {
        panic!("different affected row did not diverge");
    };
    drop(diverged);
    backend.assert_drained();

    // Same affected row as before, but a later global revision/high-water
    // frontier produced by an unrelated row.
    let unrelated_preparation = ExtensionNativeOwnershipPreparation::new(
        ExtensionNativeOwnershipKey::new(
            ProfileId::from(1),
            ExtensionInstallId::from(2),
            ExtensionGrantBrowsingContext::Regular,
        ),
        preparation().package().clone(),
        ExtensionCatalogSetDigest::from_bytes([5; 32]),
        ExtensionCatalogGenerationRole::Active,
        ExtensionInstallCatalogRevision::INITIAL,
        ExtensionInstallRevision::INITIAL,
        ExtensionGrantRevision::INITIAL,
        ExtensionGrantDigest::from_bytes([6; 32]),
        ExtensionRuntimeBackendTarget::MacosNative,
    );
    let (different_clocks_same_row, _) = apply(
        before.clone(),
        ExtensionNativeOwnershipJournalMutation::begin(unrelated_preparation),
    );
    assert_eq!(different_clocks_same_row.get(row.key()), Some(&row));
    assert_ne!(different_clocks_same_row.revision(), before.revision());
    let (backend, mut projection, unsettled) =
        make_unsettled(vec![loaded(different_clocks_same_row.clone())]);
    assert_eq!(
        projection.reload(&backend, Instant::now()),
        Ok(&different_clocks_same_row)
    );
    let RowFenceReconciliation::Diverged(diverged) = unsettled.reconcile(&projection) else {
        panic!("different clocks with the same row did not diverge");
    };
    drop(diverged);
    backend.assert_drained();
}

#[test]
fn clear_ambiguity_reconciles_exact_row_absence() {
    let (before, row) = fixture(State::ReleaseAbsent);
    let mutation = ExtensionNativeOwnershipJournalMutation::clear(row.cas());
    let (after, _) = apply(before.clone(), mutation.clone());
    let backend = Backend::new(
        [loaded(after.clone())],
        [MutationStep {
            expected_journal: before.clone(),
            expected_mutation: mutation,
            outcome: Call::TimedOutAfterAdmission,
        }],
    );
    let mut projection = known_projection(before);
    let RowFenceSettlement::ReloadRequired(unsettled) =
        settled(projection.settle_exact_row_clear(&backend, &row, 0, Instant::now()))
    else {
        unreachable!();
    };
    assert_eq!(projection.reload(&backend, Instant::now()), Ok(&after));
    let RowFenceReconciliation::Applied(applied) = unsettled.reconcile(&projection) else {
        panic!("exact clear frontier did not reconcile");
    };
    assert!(applied.entry().is_none());
    backend.assert_drained();
}

#[test]
fn conflict_is_definite_non_application_but_requires_reload_for_inspection() {
    let (before, row) = fixture(State::MayOwnIdentified);
    let requested = transition_mutation(
        &row,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
    );
    let alternative = transition_mutation(
        &row,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
    );
    let (current, _) = apply(before.clone(), alternative);
    let backend = Backend::new(
        [loaded(current.clone())],
        [MutationStep {
            expected_journal: before.clone(),
            expected_mutation: requested,
            outcome: completed(ExtensionNativeOwnershipJournalMutationOutcome::Conflict {
                current: current.revision(),
            }),
        }],
    );
    let mut projection = known_projection(before.clone());
    let RowFenceSettlement::Conflict(conflict) = settled(projection.settle_exact_row_transition(
        &backend,
        &row,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
        0,
        Instant::now(),
    )) else {
        panic!("Store conflict was misclassified as possible application");
    };
    assert!(projection.known().is_none());
    assert_eq!(conflict.reported_current(), current.revision());
    assert_eq!(conflict.expected_before(), &row);
    assert!(matches!(
        conflict.inspect(&projection),
        RowFenceConflictInspection::Pending
    ));

    assert_eq!(projection.reload(&backend, Instant::now()), Ok(&current));
    let RowFenceConflictInspection::Reloaded {
        reported_revision_matches,
        expected_before_matches,
        current: observed,
    } = conflict.inspect(&projection)
    else {
        panic!("complete reload remained pending");
    };
    assert!(reported_revision_matches);
    assert!(!expected_before_matches);
    assert_eq!(observed, current.get(row.key()));
    backend.assert_drained();
}

#[test]
fn raw_mutation_and_row_fence_cannot_bypass_fenced_activation() {
    let backend = RejectingBackend::new();
    let empty = ExtensionNativeOwnershipJournal::empty();
    let mut projection = known_projection(empty.clone());
    assert_eq!(
        projection.mutate(
            &backend,
            ExtensionNativeOwnershipJournalMutation::begin(preparation()),
            Instant::now(),
        ),
        Err(JournalMutationFailure::InvalidLocalTransition)
    );
    assert_eq!(projection.known(), Some(&empty));

    let (preparing, row) = fixture(State::Preparing);
    let mut projection = known_projection(preparing.clone());
    assert_eq!(
        projection.mutate(
            &backend,
            ExtensionNativeOwnershipJournalMutation::transition_with_expected_native_identity(
                row.cas(),
                expected_identity(),
            ),
            Instant::now(),
        ),
        Err(JournalMutationFailure::InvalidLocalTransition)
    );
    assert_eq!(projection.known(), Some(&preparing));

    assert_eq!(
        rejected(projection.settle_exact_row_transition(
            &backend,
            &row,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
            0,
            Instant::now(),
        ))
        .reason(),
        RowFencePreparationFailureReason::ForbiddenLifecycleEdge
    );
    assert_eq!(backend.mutation_calls.get(), 0);
}

#[test]
fn runtime_coordinator_source_has_no_generic_mutation_bypass() {
    let sources = [
        include_str!("../../runtime_coordinator/mod.rs"),
        include_str!("../../runtime_coordinator/activation.rs"),
        include_str!("../../runtime_coordinator/outcome.rs"),
        include_str!("../../runtime_coordinator/reconciliation.rs"),
        include_str!("../../runtime_coordinator/retirement.rs"),
        include_str!("../../runtime_coordinator/slot.rs"),
    ];
    for source in sources {
        assert!(!source.contains(".mutate("));
        assert!(!source.contains("JournalProjection::mutate"));
    }
}
