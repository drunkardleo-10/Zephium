//! Exact local projection of the Store-owned native-ownership journal.

use std::time::Instant;

use zephium_core::extensions::{
    ExtensionNativeOwnershipJournal, ExtensionNativeOwnershipJournalMutation,
};
use zephium_core::ports::store::{
    ExtensionNativeOwnershipJournalLoadOutcome, ExtensionNativeOwnershipJournalMutationApplied,
    ExtensionNativeOwnershipJournalMutationOutcome,
};
use zephium_store::{
    ExtensionNativeOwnershipStoreAuthority, ExtensionNativeOwnershipStoreCallOutcome,
};

pub(crate) trait JournalBackend {
    fn load_until(
        &self,
        deadline: Instant,
    ) -> ExtensionNativeOwnershipStoreCallOutcome<ExtensionNativeOwnershipJournalLoadOutcome>;

    fn mutate_until(
        &self,
        journal: &ExtensionNativeOwnershipJournal,
        mutation: ExtensionNativeOwnershipJournalMutation,
        deadline: Instant,
    ) -> ExtensionNativeOwnershipStoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome>;
}

impl JournalBackend for ExtensionNativeOwnershipStoreAuthority {
    fn load_until(
        &self,
        deadline: Instant,
    ) -> ExtensionNativeOwnershipStoreCallOutcome<ExtensionNativeOwnershipJournalLoadOutcome> {
        self.load_until(deadline)
    }

    fn mutate_until(
        &self,
        journal: &ExtensionNativeOwnershipJournal,
        mutation: ExtensionNativeOwnershipJournalMutation,
        deadline: Instant,
    ) -> ExtensionNativeOwnershipStoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome>
    {
        self.mutate_until(journal.revision(), mutation, deadline)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JournalLoadFailure {
    NotAdmitted,
    TimedOutAfterAdmission,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JournalMutationFailure {
    NotAdmitted,
    ReloadRequired,
    ProjectionMismatch,
    InvalidLocalTransition,
    StoreInvariant,
}

pub(crate) struct JournalProjection {
    journal: Option<ExtensionNativeOwnershipJournal>,
}

impl JournalProjection {
    pub(crate) const fn unknown() -> Self {
        Self { journal: None }
    }

    pub(crate) const fn known(&self) -> Option<&ExtensionNativeOwnershipJournal> {
        self.journal.as_ref()
    }

    pub(crate) fn invalidate(&mut self) {
        self.journal = None;
    }

    pub(crate) fn reload(
        &mut self,
        backend: &impl JournalBackend,
        deadline: Instant,
    ) -> Result<&ExtensionNativeOwnershipJournal, JournalLoadFailure> {
        self.journal = None;
        match backend.load_until(deadline) {
            ExtensionNativeOwnershipStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal),
            ) => {
                self.journal = Some(journal);
                Ok(self.journal.as_ref().expect("journal was just installed"))
            }
            ExtensionNativeOwnershipStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalLoadOutcome::Failed,
            ) => Err(JournalLoadFailure::Failed),
            ExtensionNativeOwnershipStoreCallOutcome::NotAdmitted => {
                Err(JournalLoadFailure::NotAdmitted)
            }
            ExtensionNativeOwnershipStoreCallOutcome::TimedOutAfterAdmission => {
                Err(JournalLoadFailure::TimedOutAfterAdmission)
            }
        }
    }

    pub(crate) fn mutate(
        &mut self,
        backend: &impl JournalBackend,
        mutation: ExtensionNativeOwnershipJournalMutation,
        deadline: Instant,
    ) -> Result<&ExtensionNativeOwnershipJournal, JournalMutationFailure> {
        let Some(current) = self.journal.take() else {
            return Err(JournalMutationFailure::ReloadRequired);
        };
        let application = match current.clone().apply(current.revision(), mutation.clone()) {
            Ok(application) => application,
            Err(_) => {
                self.journal = Some(current);
                return Err(JournalMutationFailure::InvalidLocalTransition);
            }
        };
        match backend.mutate_until(&current, mutation, deadline) {
            ExtensionNativeOwnershipStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalMutationOutcome::Applied(applied),
            ) if applied_matches(&application, &applied) => {
                self.journal = Some(application.into_journal());
                Ok(self.journal.as_ref().expect("journal was just installed"))
            }
            ExtensionNativeOwnershipStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalMutationOutcome::Applied(_),
            ) => Err(JournalMutationFailure::ProjectionMismatch),
            ExtensionNativeOwnershipStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalMutationOutcome::Conflict { .. }
                | ExtensionNativeOwnershipJournalMutationOutcome::OutcomeUnknown,
            )
            | ExtensionNativeOwnershipStoreCallOutcome::TimedOutAfterAdmission => {
                Err(JournalMutationFailure::ReloadRequired)
            }
            ExtensionNativeOwnershipStoreCallOutcome::NotAdmitted => {
                self.journal = Some(current);
                Err(JournalMutationFailure::NotAdmitted)
            }
            ExtensionNativeOwnershipStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalMutationOutcome::NotRegistered
                | ExtensionNativeOwnershipJournalMutationOutcome::DegradedProfile
                | ExtensionNativeOwnershipJournalMutationOutcome::SessionRecoveryRequired
                | ExtensionNativeOwnershipJournalMutationOutcome::Invalid
                | ExtensionNativeOwnershipJournalMutationOutcome::LimitReached
                | ExtensionNativeOwnershipJournalMutationOutcome::RevisionExhausted
                | ExtensionNativeOwnershipJournalMutationOutcome::Failed,
            ) => Err(JournalMutationFailure::StoreInvariant),
        }
    }
}

fn applied_matches(
    application: &zephium_core::extensions::ExtensionNativeOwnershipJournalApplication,
    applied: &ExtensionNativeOwnershipJournalMutationApplied,
) -> bool {
    let predicted = application.journal();
    applied.journal_revision == predicted.revision()
        && applied.operation_high_water == predicted.operation_high_water()
        && applied.native_incarnation_high_water == predicted.native_incarnation_high_water()
        && applied.entry.as_deref() == application.entry()
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    use super::*;
    use zephium_core::extensions::{
        ExtensionAuthorityId, ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
        ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision,
        ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionManifestDigest,
        ExtensionNativeOwnershipJournalRevision, ExtensionNativeOwnershipKey,
        ExtensionNativeOwnershipPreparation, ExtensionPackageIdentity, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackageRevision, ExtensionRuntimeBackendTarget,
        ExtensionTreeDigest,
    };
    use zephium_core::ids::{ExtensionInstallId, ProfileId};

    type StoreCallOutcome<T> = ExtensionNativeOwnershipStoreCallOutcome<T>;

    struct MutationStep {
        expected_journal: ExtensionNativeOwnershipJournal,
        expected_mutation: ExtensionNativeOwnershipJournalMutation,
        outcome: StoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome>,
    }

    impl MutationStep {
        fn new(
            expected_journal: ExtensionNativeOwnershipJournal,
            expected_mutation: ExtensionNativeOwnershipJournalMutation,
            outcome: StoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome>,
        ) -> Self {
            Self {
                expected_journal,
                expected_mutation,
                outcome,
            }
        }
    }

    struct ScriptedBackend {
        loads: RefCell<VecDeque<StoreCallOutcome<ExtensionNativeOwnershipJournalLoadOutcome>>>,
        mutations: RefCell<VecDeque<MutationStep>>,
        load_calls: Cell<usize>,
        mutation_calls: Cell<usize>,
    }

    impl ScriptedBackend {
        fn new(
            loads: impl IntoIterator<
                Item = StoreCallOutcome<ExtensionNativeOwnershipJournalLoadOutcome>,
            >,
            mutations: impl IntoIterator<Item = MutationStep>,
        ) -> Self {
            Self {
                loads: RefCell::new(loads.into_iter().collect()),
                mutations: RefCell::new(mutations.into_iter().collect()),
                load_calls: Cell::new(0),
                mutation_calls: Cell::new(0),
            }
        }

        fn assert_drained(&self) {
            assert!(self.loads.borrow().is_empty(), "unused scripted load");
            assert!(
                self.mutations.borrow().is_empty(),
                "unused scripted mutation"
            );
        }
    }

    impl JournalBackend for ScriptedBackend {
        fn load_until(
            &self,
            _deadline: Instant,
        ) -> StoreCallOutcome<ExtensionNativeOwnershipJournalLoadOutcome> {
            self.load_calls.set(self.load_calls.get() + 1);
            self.loads
                .borrow_mut()
                .pop_front()
                .expect("unexpected journal load")
        }

        fn mutate_until(
            &self,
            journal: &ExtensionNativeOwnershipJournal,
            mutation: ExtensionNativeOwnershipJournalMutation,
            _deadline: Instant,
        ) -> StoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome> {
            self.mutation_calls.set(self.mutation_calls.get() + 1);
            let step = self
                .mutations
                .borrow_mut()
                .pop_front()
                .expect("unexpected journal mutation");
            assert_eq!(journal, &step.expected_journal);
            assert_eq!(mutation, step.expected_mutation);
            step.outcome
        }
    }

    fn preparation(value: u128) -> ExtensionNativeOwnershipPreparation {
        ExtensionNativeOwnershipPreparation::new(
            ExtensionNativeOwnershipKey::new(
                ProfileId::from(1),
                ExtensionInstallId::from(value),
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

    fn begin(value: u128) -> ExtensionNativeOwnershipJournalMutation {
        ExtensionNativeOwnershipJournalMutation::begin(preparation(value))
    }

    fn exact_applied(
        current: &ExtensionNativeOwnershipJournal,
        mutation: &ExtensionNativeOwnershipJournalMutation,
    ) -> (
        ExtensionNativeOwnershipJournal,
        ExtensionNativeOwnershipJournalMutationApplied,
    ) {
        let application = current
            .clone()
            .apply(current.revision(), mutation.clone())
            .expect("fixture mutation must be valid");
        let applied = ExtensionNativeOwnershipJournalMutationApplied {
            journal_revision: application.journal().revision(),
            operation_high_water: application.journal().operation_high_water(),
            native_incarnation_high_water: application.journal().native_incarnation_high_water(),
            entry: application.entry().cloned().map(Box::new),
        };
        (application.into_journal(), applied)
    }

    fn completed_load(
        journal: ExtensionNativeOwnershipJournal,
    ) -> StoreCallOutcome<ExtensionNativeOwnershipJournalLoadOutcome> {
        StoreCallOutcome::Completed(ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal))
    }

    fn completed_mutation(
        outcome: ExtensionNativeOwnershipJournalMutationOutcome,
    ) -> StoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome> {
        StoreCallOutcome::Completed(outcome)
    }

    fn assert_mutation_requires_reload(
        first_outcome: StoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome>,
        expected_failure: JournalMutationFailure,
    ) {
        let initial = ExtensionNativeOwnershipJournal::empty();
        let first_mutation = begin(1);
        let retry_mutation = begin(2);
        let (expected_after_retry, retry_applied) = exact_applied(&initial, &retry_mutation);
        let backend = ScriptedBackend::new(
            [
                completed_load(initial.clone()),
                completed_load(initial.clone()),
            ],
            [
                MutationStep::new(initial.clone(), first_mutation.clone(), first_outcome),
                MutationStep::new(
                    initial.clone(),
                    retry_mutation.clone(),
                    completed_mutation(ExtensionNativeOwnershipJournalMutationOutcome::Applied(
                        retry_applied,
                    )),
                ),
            ],
        );
        let mut projection = JournalProjection::unknown();

        assert_eq!(projection.reload(&backend, Instant::now()), Ok(&initial));
        assert_eq!(
            projection.mutate(&backend, first_mutation, Instant::now()),
            Err(expected_failure)
        );
        assert!(projection.known().is_none());

        assert_eq!(
            projection.mutate(&backend, retry_mutation.clone(), Instant::now()),
            Err(JournalMutationFailure::ReloadRequired)
        );
        assert_eq!(backend.mutation_calls.get(), 1);

        assert_eq!(projection.reload(&backend, Instant::now()), Ok(&initial));
        assert_eq!(
            projection.mutate(&backend, retry_mutation, Instant::now()),
            Ok(&expected_after_retry)
        );
        assert_eq!(projection.known(), Some(&expected_after_retry));
        assert_eq!(backend.load_calls.get(), 2);
        assert_eq!(backend.mutation_calls.get(), 2);
        backend.assert_drained();
    }

    #[test]
    fn exact_applied_result_installs_only_the_predicted_projection() {
        let initial = ExtensionNativeOwnershipJournal::empty();
        let mutation = begin(1);
        let (predicted, applied) = exact_applied(&initial, &mutation);
        let backend = ScriptedBackend::new(
            [completed_load(initial.clone())],
            [MutationStep::new(
                initial.clone(),
                mutation.clone(),
                completed_mutation(ExtensionNativeOwnershipJournalMutationOutcome::Applied(
                    applied,
                )),
            )],
        );
        let mut projection = JournalProjection::unknown();

        assert_eq!(projection.reload(&backend, Instant::now()), Ok(&initial));
        assert_eq!(
            projection.mutate(&backend, mutation, Instant::now()),
            Ok(&predicted)
        );
        assert_eq!(projection.known(), Some(&predicted));
        assert_eq!(backend.load_calls.get(), 1);
        assert_eq!(backend.mutation_calls.get(), 1);
        backend.assert_drained();
    }

    #[test]
    fn every_applied_field_mismatch_invalidates_until_a_full_reload() {
        let initial = ExtensionNativeOwnershipJournal::empty();
        let mutation = begin(1);
        let (_, exact) = exact_applied(&initial, &mutation);

        let mut revision_mismatch = exact.clone();
        revision_mismatch.journal_revision = revision_mismatch
            .journal_revision
            .next()
            .expect("fixture revision must advance");

        let mut operation_mismatch = exact.clone();
        operation_mismatch.operation_high_water = None;

        let mut incarnation_mismatch = exact.clone();
        incarnation_mismatch.native_incarnation_high_water = None;

        let mut entry_mismatch = exact;
        entry_mismatch.entry = None;

        for (field, applied) in [
            ("journal_revision", revision_mismatch),
            ("operation_high_water", operation_mismatch),
            ("native_incarnation_high_water", incarnation_mismatch),
            ("entry", entry_mismatch),
        ] {
            assert_ne!(
                applied,
                exact_applied(&initial, &mutation).1,
                "{field} fixture must differ from the predicted settlement"
            );
            assert_mutation_requires_reload(
                completed_mutation(ExtensionNativeOwnershipJournalMutationOutcome::Applied(
                    applied,
                )),
                JournalMutationFailure::ProjectionMismatch,
            );
        }
    }

    #[test]
    fn conflict_outcome_unknown_and_timeout_require_reload_before_retry() {
        let initial_revision = ExtensionNativeOwnershipJournalRevision::INITIAL;
        for outcome in [
            completed_mutation(ExtensionNativeOwnershipJournalMutationOutcome::Conflict {
                current: initial_revision,
            }),
            completed_mutation(ExtensionNativeOwnershipJournalMutationOutcome::OutcomeUnknown),
            StoreCallOutcome::TimedOutAfterAdmission,
        ] {
            assert_mutation_requires_reload(outcome, JournalMutationFailure::ReloadRequired);
        }
    }

    #[test]
    fn definite_non_admission_retains_projection_and_permits_retry_without_reload() {
        let initial = ExtensionNativeOwnershipJournal::empty();
        let mutation = begin(1);
        let (predicted, applied) = exact_applied(&initial, &mutation);
        let backend = ScriptedBackend::new(
            [completed_load(initial.clone())],
            [
                MutationStep::new(
                    initial.clone(),
                    mutation.clone(),
                    StoreCallOutcome::NotAdmitted,
                ),
                MutationStep::new(
                    initial.clone(),
                    mutation.clone(),
                    completed_mutation(ExtensionNativeOwnershipJournalMutationOutcome::Applied(
                        applied,
                    )),
                ),
            ],
        );
        let mut projection = JournalProjection::unknown();

        assert_eq!(projection.reload(&backend, Instant::now()), Ok(&initial));
        assert_eq!(
            projection.mutate(&backend, mutation.clone(), Instant::now()),
            Err(JournalMutationFailure::NotAdmitted)
        );
        assert_eq!(projection.known(), Some(&initial));
        assert_eq!(backend.load_calls.get(), 1);

        assert_eq!(
            projection.mutate(&backend, mutation, Instant::now()),
            Ok(&predicted)
        );
        assert_eq!(backend.load_calls.get(), 1);
        assert_eq!(backend.mutation_calls.get(), 2);
        backend.assert_drained();
    }

    #[test]
    fn every_unexpected_store_settlement_invalidates_until_reload() {
        for outcome in [
            ExtensionNativeOwnershipJournalMutationOutcome::NotRegistered,
            ExtensionNativeOwnershipJournalMutationOutcome::DegradedProfile,
            ExtensionNativeOwnershipJournalMutationOutcome::SessionRecoveryRequired,
            ExtensionNativeOwnershipJournalMutationOutcome::Invalid,
            ExtensionNativeOwnershipJournalMutationOutcome::LimitReached,
            ExtensionNativeOwnershipJournalMutationOutcome::RevisionExhausted,
            ExtensionNativeOwnershipJournalMutationOutcome::Failed,
        ] {
            assert_mutation_requires_reload(
                completed_mutation(outcome),
                JournalMutationFailure::StoreInvariant,
            );
        }
    }

    #[test]
    fn invalid_local_transition_never_reaches_store_and_retains_projection() {
        let initial = ExtensionNativeOwnershipJournal::empty();
        let existing_mutation = begin(1);
        let (known, _) = exact_applied(&initial, &existing_mutation);
        let valid_mutation = begin(2);
        let (predicted, applied) = exact_applied(&known, &valid_mutation);
        let backend = ScriptedBackend::new(
            [completed_load(known.clone())],
            [MutationStep::new(
                known.clone(),
                valid_mutation.clone(),
                completed_mutation(ExtensionNativeOwnershipJournalMutationOutcome::Applied(
                    applied,
                )),
            )],
        );
        let mut projection = JournalProjection::unknown();

        assert_eq!(projection.reload(&backend, Instant::now()), Ok(&known));
        assert_eq!(
            projection.mutate(&backend, existing_mutation, Instant::now()),
            Err(JournalMutationFailure::InvalidLocalTransition)
        );
        assert_eq!(projection.known(), Some(&known));
        assert_eq!(backend.mutation_calls.get(), 0);

        assert_eq!(
            projection.mutate(&backend, valid_mutation, Instant::now()),
            Ok(&predicted)
        );
        assert_eq!(backend.mutation_calls.get(), 1);
        backend.assert_drained();
    }

    #[test]
    fn unsuccessful_reload_clears_previously_known_projection() {
        for (outcome, expected_failure) in [
            (
                StoreCallOutcome::Completed(ExtensionNativeOwnershipJournalLoadOutcome::Failed),
                JournalLoadFailure::Failed,
            ),
            (
                StoreCallOutcome::NotAdmitted,
                JournalLoadFailure::NotAdmitted,
            ),
            (
                StoreCallOutcome::TimedOutAfterAdmission,
                JournalLoadFailure::TimedOutAfterAdmission,
            ),
        ] {
            let initial = ExtensionNativeOwnershipJournal::empty();
            let backend = ScriptedBackend::new(
                [completed_load(initial.clone()), outcome],
                std::iter::empty(),
            );
            let mut projection = JournalProjection::unknown();

            assert_eq!(projection.reload(&backend, Instant::now()), Ok(&initial));
            assert_eq!(
                projection.reload(&backend, Instant::now()),
                Err(expected_failure)
            );
            assert!(projection.known().is_none());
            assert_eq!(
                projection.mutate(&backend, begin(1), Instant::now()),
                Err(JournalMutationFailure::ReloadRequired)
            );
            assert_eq!(backend.load_calls.get(), 2);
            assert_eq!(backend.mutation_calls.get(), 0);
            backend.assert_drained();
        }
    }
}
