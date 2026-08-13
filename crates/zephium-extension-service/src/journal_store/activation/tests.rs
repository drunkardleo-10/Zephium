use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use super::*;
use zephium_core::extensions::{
    ApiPermissionName, ExtensionApiPermissionSet, ExtensionAuthorityId,
    ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
    ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
    ExtensionCompatibilityTargetId, ExtensionContentSecurityPolicyDeclaration,
    ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision,
    ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionManifestDeclarations,
    ExtensionManifestDigest, ExtensionManifestExecutionSurfaces, ExtensionManifestResourceDigest,
    ExtensionNativeOwnershipJournalMutation, ExtensionNativeOwnershipJournalRevision,
    ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPreparation, ExtensionPackageIdentity,
    ExtensionPackageKey, ExtensionPackagePayloadIdentity, ExtensionPackageRevision,
    ExtensionTreeDigest,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_core::ports::store::{
    ExtensionNativeOwnershipJournalLoadOutcome, ExtensionNativeOwnershipJournalMutationApplied,
    ExtensionNativeOwnershipJournalMutationOutcome,
};

type Call<T> = ExtensionServiceStoreCallOutcome<T>;

enum Step {
    Begin {
        expected: ExtensionNativeOwnershipJournalRevision,
        mutation: ExtensionNativeOwnershipJournalMutation,
        manifest: Arc<ExtensionManifestDescriptor>,
        outcome: Call<ExtensionNativeOwnershipActivationOutcome>,
    },
    MayOwn {
        expected: ExtensionNativeOwnershipJournalRevision,
        preparing: ExtensionNativeOwnershipEntryCas,
        identity: Option<ExtensionExpectedNativeOwnershipIdentity>,
        manifest: Arc<ExtensionManifestDescriptor>,
        outcome: Call<ExtensionNativeOwnershipActivationOutcome>,
    },
}

struct Backend {
    loads: RefCell<VecDeque<Call<ExtensionNativeOwnershipJournalLoadOutcome>>>,
    steps: RefCell<VecDeque<Step>>,
    expected_deadline: Option<Instant>,
}

impl Backend {
    fn new(
        loads: impl IntoIterator<Item = Call<ExtensionNativeOwnershipJournalLoadOutcome>>,
        steps: impl IntoIterator<Item = Step>,
    ) -> Self {
        Self {
            loads: RefCell::new(loads.into_iter().collect()),
            steps: RefCell::new(steps.into_iter().collect()),
            expected_deadline: None,
        }
    }

    fn with_expected_deadline(mut self, deadline: Instant) -> Self {
        self.expected_deadline = Some(deadline);
        self
    }

    fn assert_drained(&self) {
        assert!(self.loads.borrow().is_empty(), "unused scripted load");
        assert!(self.steps.borrow().is_empty(), "unused activation step");
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
        _journal: &ExtensionNativeOwnershipJournal,
        _mutation: ExtensionNativeOwnershipJournalMutation,
        _deadline: Instant,
    ) -> Call<ExtensionNativeOwnershipJournalMutationOutcome> {
        panic!("fenced activation tests must not use the raw mutation seam")
    }
}

impl FencedActivationJournalBackend for Backend {
    fn begin_until(
        &self,
        expected: ExtensionNativeOwnershipJournalRevision,
        mutation: ExtensionNativeOwnershipJournalMutation,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> Call<ExtensionNativeOwnershipActivationOutcome> {
        if let Some(expected_deadline) = self.expected_deadline {
            assert_eq!(deadline, expected_deadline);
        }
        let Step::Begin {
            expected: scripted_expected,
            mutation: scripted_mutation,
            manifest: scripted_manifest,
            outcome,
        } = self
            .steps
            .borrow_mut()
            .pop_front()
            .expect("unexpected fenced Begin")
        else {
            panic!("expected Preparing-to-MayOwn, observed Begin");
        };
        assert_eq!(expected, scripted_expected);
        assert_eq!(mutation, scripted_mutation);
        assert!(Arc::ptr_eq(&manifest, &scripted_manifest));
        outcome
    }

    fn transition_to_may_own_until(
        &self,
        expected: ExtensionNativeOwnershipJournalRevision,
        preparing: ExtensionNativeOwnershipEntryCas,
        identity: Option<ExtensionExpectedNativeOwnershipIdentity>,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> Call<ExtensionNativeOwnershipActivationOutcome> {
        if let Some(expected_deadline) = self.expected_deadline {
            assert_eq!(deadline, expected_deadline);
        }
        let Step::MayOwn {
            expected: scripted_expected,
            preparing: scripted_preparing,
            identity: scripted_identity,
            manifest: scripted_manifest,
            outcome,
        } = self
            .steps
            .borrow_mut()
            .pop_front()
            .expect("unexpected Preparing-to-MayOwn")
        else {
            panic!("expected Begin, observed Preparing-to-MayOwn");
        };
        assert_eq!(expected, scripted_expected);
        assert_eq!(preparing, scripted_preparing);
        assert_eq!(identity, scripted_identity);
        assert!(Arc::ptr_eq(&manifest, &scripted_manifest));
        outcome
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Authority(u8);

fn package(seed: u8) -> ExtensionPackageIdentity {
    ExtensionPackageIdentity::new(
        ExtensionAuthorityId::from_bytes([seed; 32]),
        ExtensionPackageKey::from_bytes([2; 32]),
        ExtensionPackageRevision::INITIAL,
        ExtensionPackagePayloadIdentity::BundledTree,
        ExtensionManifestDigest::from_bytes([3; 32]),
        ExtensionTreeDigest::from_bytes([4; 32]),
    )
}

fn preparation(
    install: u128,
    backend: ExtensionRuntimeBackendTarget,
) -> ExtensionNativeOwnershipPreparation {
    ExtensionNativeOwnershipPreparation::new(
        ExtensionNativeOwnershipKey::new(
            ProfileId::from(1),
            ExtensionInstallId::from(install),
            ExtensionGrantBrowsingContext::Regular,
        ),
        package(1),
        ExtensionCatalogSetDigest::from_bytes([5; 32]),
        ExtensionCatalogGenerationRole::Active,
        ExtensionInstallCatalogRevision::INITIAL,
        ExtensionInstallRevision::INITIAL,
        ExtensionGrantRevision::INITIAL,
        ExtensionGrantDigest::from_bytes([6; 32]),
        backend,
    )
}

fn begin(install: u128) -> ExtensionNativeOwnershipJournalMutation {
    ExtensionNativeOwnershipJournalMutation::begin(preparation(
        install,
        ExtensionRuntimeBackendTarget::MacosNative,
    ))
}

fn manifest_for(package: ExtensionPackageIdentity) -> Arc<ExtensionManifestDescriptor> {
    let declarations = ExtensionManifestDeclarations::new(
        ExtensionApiPermissionSet::new(vec![ApiPermissionName::parse_exact("storage").unwrap()])
            .unwrap(),
        ExtensionApiPermissionSet::new(Vec::new()).unwrap(),
        None,
        None,
        None,
        None,
        Vec::new(),
        ExtensionManifestExecutionSurfaces::new(
            Vec::new(),
            ExtensionContentSecurityPolicyDeclaration::new(
                ExtensionManifestResourceDigest::from_bytes([7; 32]),
            ),
            None,
            Vec::new(),
        )
        .unwrap(),
        Vec::new(),
    )
    .unwrap();
    let compatibility = declarations
        .declaration_keys()
        .into_iter()
        .map(|declaration| {
            ExtensionCompatibilityClassification::new(
                declaration,
                ExtensionCompatibilityLevel::Compatible,
            )
        })
        .collect();
    Arc::new(
        ExtensionManifestDescriptor::new(
            package,
            3,
            declarations,
            ExtensionCompatibilityTargetId::parse_exact("test.journal.activation.v1").unwrap(),
            compatibility,
        )
        .unwrap(),
    )
}

fn expected_identity(
    backend: ExtensionRuntimeBackendTarget,
) -> ExtensionExpectedNativeOwnershipIdentity {
    ExtensionExpectedNativeOwnershipIdentity::parse(backend, "abcdefghijklmnopabcdefghijklmnop")
        .unwrap()
}

fn apply(
    current: &ExtensionNativeOwnershipJournal,
    mutation: &ExtensionNativeOwnershipJournalMutation,
) -> (
    ExtensionNativeOwnershipJournal,
    ExtensionNativeOwnershipJournalMutationApplied,
) {
    let application = current
        .clone()
        .apply(current.revision(), mutation.clone())
        .unwrap();
    let applied = ExtensionNativeOwnershipJournalMutationApplied {
        journal_revision: application.journal().revision(),
        operation_high_water: application.journal().operation_high_water(),
        native_incarnation_high_water: application.journal().native_incarnation_high_water(),
        grant_rebind_count: application.journal().grant_rebind_count(),
        entry: application.entry().cloned().map(Box::new),
    };
    (application.into_journal(), applied)
}

fn loaded(
    journal: ExtensionNativeOwnershipJournal,
) -> Call<ExtensionNativeOwnershipJournalLoadOutcome> {
    Call::Completed(ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal))
}

fn completed(
    outcome: ExtensionNativeOwnershipActivationOutcome,
) -> Call<ExtensionNativeOwnershipActivationOutcome> {
    Call::Completed(outcome)
}

fn settled(
    result: Result<
        JournalActivationSettlement<Authority>,
        JournalActivationPreparationFailure<Authority>,
    >,
) -> JournalActivationSettlement<Authority> {
    match result {
        Ok(settlement) => settlement,
        Err(failure) => panic!("preparation failed: {:?}", failure.reason()),
    }
}

fn rejected(
    result: Result<
        JournalActivationSettlement<Authority>,
        JournalActivationPreparationFailure<Authority>,
    >,
) -> JournalActivationPreparationFailure<Authority> {
    match result {
        Err(failure) => failure,
        Ok(_) => panic!("preparation unexpectedly succeeded"),
    }
}

#[test]
fn typed_begin_facade_remains_reachable_until_coordinator_wiring() {
    fn invoke(
        projection: &mut JournalProjection,
        backend: &Backend,
        plan: ServiceRuntimeAcquisitionPlan,
        deadline: Instant,
    ) {
        drop(projection.settle_fenced_begin(backend, plan, deadline));
    }

    fn invoke_may_own(
        projection: &mut JournalProjection,
        backend: &Backend,
        access: ServiceRuntimePackageAccess,
        preparing: ExtensionNativeOwnershipEntry,
        deadline: Instant,
    ) {
        drop(projection.settle_preparing_to_may_own(backend, access, preparing, deadline));
    }

    let _ = (invoke, invoke_may_own);
}

#[test]
fn retained_admission_reserves_the_exact_worst_case_adapter_charge() {
    let adapter_bound =
        JournalActivationReloadRequired::<Authority>::maximum_additional_retained_bytes();
    let boundary = MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        .checked_sub(adapter_bound)
        .expect("activation adapter bound must fit the per-owner ceiling");

    assert_eq!(retained_budget_failure::<Authority>(boundary), None);
    assert_eq!(
        retained_budget_failure::<Authority>(boundary + 1),
        Some(JournalActivationPreparationFailureReason::RetainedBytesExceeded)
    );
    assert_eq!(
        retained_budget_failure::<Authority>(usize::MAX),
        Some(JournalActivationPreparationFailureReason::RetainedBytesOverflow)
    );
}

#[test]
fn exact_begin_and_may_own_apply_and_preserve_nominal_authority() {
    let empty = ExtensionNativeOwnershipJournal::empty();
    let begin = begin(1);
    let (preparing_journal, begin_applied) = apply(&empty, &begin);
    let preparing = preparing_journal.entries()[0].clone();
    let identity = expected_identity(ExtensionRuntimeBackendTarget::MacosNative);
    let may_own = ExtensionNativeOwnershipJournalMutation::transition_with_expected_native_identity(
        preparing.cas(),
        identity,
    );
    let (may_own_journal, may_own_applied) = apply(&preparing_journal, &may_own);
    let manifest = manifest_for(package(1));
    let deadline = Instant::now();
    let backend = Backend::new(
        [],
        [
            Step::Begin {
                expected: empty.revision(),
                mutation: begin.clone(),
                manifest: Arc::clone(&manifest),
                outcome: completed(ExtensionNativeOwnershipActivationOutcome::Applied(
                    begin_applied,
                )),
            },
            Step::MayOwn {
                expected: preparing_journal.revision(),
                preparing: preparing.cas(),
                identity: Some(identity),
                manifest: Arc::clone(&manifest),
                outcome: completed(ExtensionNativeOwnershipActivationOutcome::Applied(
                    may_own_applied,
                )),
            },
        ],
    )
    .with_expected_deadline(deadline);
    let mut projection = JournalProjection {
        journal: Some(empty),
    };

    let JournalActivationSettlement::Applied(applied) =
        settled(projection.settle_fenced_begin_with_authority(
            &backend,
            Authority(1),
            begin,
            Arc::clone(&manifest),
            deadline,
        ))
    else {
        panic!("Begin was not applied");
    };
    assert!(
        applied.additional_retained_bytes()
            <= JournalActivationReloadRequired::<Authority>::maximum_additional_retained_bytes()
    );
    let (authority, row) = applied.into_parts();
    assert_eq!(authority, Authority(1));
    assert_eq!(row, preparing);
    assert_eq!(projection.known(), Some(&preparing_journal));

    let JournalActivationSettlement::Applied(applied) =
        settled(projection.settle_preparing_to_may_own_with_authority(
            &backend,
            Authority(2),
            preparing,
            Some(identity),
            Arc::clone(&manifest),
            deadline,
        ))
    else {
        panic!("MayOwn was not applied");
    };
    assert_eq!(applied.entry().expected_native_identity(), Some(identity));
    assert_eq!(projection.known(), Some(&may_own_journal));
    let (authority, _) = applied.into_parts();
    assert_eq!(authority, Authority(2));
    backend.assert_drained();
}

#[test]
fn every_definite_refusal_and_non_admission_returns_authority_without_projection_loss() {
    let refusals = [
        (
            ExtensionNativeOwnershipActivationOutcome::NotRegistered,
            JournalActivationRefusalReason::NotRegistered,
        ),
        (
            ExtensionNativeOwnershipActivationOutcome::DegradedProfile,
            JournalActivationRefusalReason::DegradedProfile,
        ),
        (
            ExtensionNativeOwnershipActivationOutcome::SessionRecoveryRequired,
            JournalActivationRefusalReason::SessionRecoveryRequired,
        ),
        (
            ExtensionNativeOwnershipActivationOutcome::Stale(
                ExtensionNativeOwnershipActivationStale::GrantDigest,
            ),
            JournalActivationRefusalReason::Stale(
                ExtensionNativeOwnershipActivationStale::GrantDigest,
            ),
        ),
        (
            ExtensionNativeOwnershipActivationOutcome::EligibilityChanged(
                ExtensionRuntimeEligibilityDenial::Disabled,
            ),
            JournalActivationRefusalReason::EligibilityChanged(
                ExtensionRuntimeEligibilityDenial::Disabled,
            ),
        ),
        (
            ExtensionNativeOwnershipActivationOutcome::Invalid,
            JournalActivationRefusalReason::Invalid,
        ),
        (
            ExtensionNativeOwnershipActivationOutcome::LimitReached,
            JournalActivationRefusalReason::LimitReached,
        ),
        (
            ExtensionNativeOwnershipActivationOutcome::RevisionExhausted,
            JournalActivationRefusalReason::RevisionExhausted,
        ),
        (
            ExtensionNativeOwnershipActivationOutcome::Failed,
            JournalActivationRefusalReason::Failed,
        ),
    ];
    for (index, (outcome, reason)) in refusals.into_iter().enumerate() {
        let initial = ExtensionNativeOwnershipJournal::empty();
        let mutation = begin(1);
        let manifest = manifest_for(package(1));
        let backend = Backend::new(
            [],
            [Step::Begin {
                expected: initial.revision(),
                mutation: mutation.clone(),
                manifest: Arc::clone(&manifest),
                outcome: completed(outcome),
            }],
        );
        let mut projection = JournalProjection {
            journal: Some(initial.clone()),
        };
        let JournalActivationSettlement::Refused(refused) =
            settled(projection.settle_fenced_begin_with_authority(
                &backend,
                Authority(index as u8),
                mutation,
                Arc::clone(&manifest),
                Instant::now(),
            ))
        else {
            panic!("definite outcome was not a refusal");
        };
        assert_eq!(refused.reason(), reason);
        assert_eq!(
            refused.into_returned().into_authority(),
            Authority(index as u8)
        );
        assert_eq!(projection.known(), Some(&initial));
        backend.assert_drained();
    }

    let initial = ExtensionNativeOwnershipJournal::empty();
    let mutation = begin(1);
    let manifest = manifest_for(package(1));
    let backend = Backend::new(
        [],
        [Step::Begin {
            expected: initial.revision(),
            mutation: mutation.clone(),
            manifest: Arc::clone(&manifest),
            outcome: Call::NotAdmitted,
        }],
    );
    let mut projection = JournalProjection {
        journal: Some(initial.clone()),
    };
    let JournalActivationSettlement::NotAdmitted(returned) =
        settled(projection.settle_fenced_begin_with_authority(
            &backend,
            Authority(10),
            mutation,
            Arc::clone(&manifest),
            Instant::now(),
        ))
    else {
        panic!("non-admission was not preserved");
    };
    assert_eq!(returned.into_authority(), Authority(10));
    assert_eq!(projection.known(), Some(&initial));
    backend.assert_drained();
}

#[test]
fn conflict_returns_authority_but_invalidates_until_a_real_current_reload() {
    let initial = ExtensionNativeOwnershipJournal::empty();
    let mutation = begin(1);
    let (current, _) = apply(&initial, &begin(2));
    let manifest = manifest_for(package(1));
    let backend = Backend::new(
        [loaded(current.clone())],
        [Step::Begin {
            expected: initial.revision(),
            mutation: mutation.clone(),
            manifest: Arc::clone(&manifest),
            outcome: completed(ExtensionNativeOwnershipActivationOutcome::Conflict {
                current: current.revision(),
            }),
        }],
    );
    let mut projection = JournalProjection {
        journal: Some(initial),
    };

    let JournalActivationSettlement::Conflict(conflict) =
        settled(projection.settle_fenced_begin_with_authority(
            &backend,
            Authority(12),
            mutation,
            Arc::clone(&manifest),
            Instant::now(),
        ))
    else {
        panic!("pre-commit conflict was not classified distinctly");
    };
    assert_eq!(conflict.current(), current.revision());
    assert!(projection.known().is_none());
    assert_eq!(conflict.into_returned().into_authority(), Authority(12));

    assert_eq!(projection.reload(&backend, Instant::now()), Ok(&current));
    backend.assert_drained();
}

#[test]
fn ambiguous_classes_invalidate_and_reconcile_only_complete_exact_frontiers() {
    let initial = ExtensionNativeOwnershipJournal::empty();
    let mutation = begin(1);
    let (predicted, exact) = apply(&initial, &mutation);
    let mut mismatched = exact;
    mismatched.entry = None;
    let outcomes = [
        completed(ExtensionNativeOwnershipActivationOutcome::OutcomeUnknown),
        Call::TimedOutAfterAdmission,
        completed(ExtensionNativeOwnershipActivationOutcome::Applied(
            mismatched,
        )),
    ];
    for (index, outcome) in outcomes.into_iter().enumerate() {
        let manifest = manifest_for(package(1));
        let backend = Backend::new(
            [loaded(predicted.clone())],
            [Step::Begin {
                expected: initial.revision(),
                mutation: mutation.clone(),
                manifest: Arc::clone(&manifest),
                outcome,
            }],
        );
        let mut projection = JournalProjection {
            journal: Some(initial.clone()),
        };
        let JournalActivationSettlement::ReloadRequired(unsettled) =
            settled(projection.settle_fenced_begin_with_authority(
                &backend,
                Authority(index as u8),
                mutation.clone(),
                Arc::clone(&manifest),
                Instant::now(),
            ))
        else {
            panic!("ambiguous outcome did not require reload");
        };
        assert!(
            unsettled.additional_retained_bytes()
                <= JournalActivationReloadRequired::<Authority>::maximum_additional_retained_bytes(
                )
        );
        assert!(projection.known().is_none());
        assert_eq!(projection.reload(&backend, Instant::now()), Ok(&predicted));
        let JournalActivationReconciliation::Applied(applied) = unsettled.reconcile(&projection)
        else {
            panic!("exact after frontier did not reconcile");
        };
        let (authority, _) = applied.into_parts();
        assert_eq!(authority, Authority(index as u8));
        backend.assert_drained();
    }

    let manifest = manifest_for(package(1));
    let backend = Backend::new(
        [loaded(initial.clone())],
        [Step::Begin {
            expected: initial.revision(),
            mutation: mutation.clone(),
            manifest: Arc::clone(&manifest),
            outcome: completed(ExtensionNativeOwnershipActivationOutcome::OutcomeUnknown),
        }],
    );
    let mut projection = JournalProjection {
        journal: Some(initial.clone()),
    };
    let JournalActivationSettlement::ReloadRequired(unsettled) =
        settled(projection.settle_fenced_begin_with_authority(
            &backend,
            Authority(5),
            mutation.clone(),
            Arc::clone(&manifest),
            Instant::now(),
        ))
    else {
        unreachable!();
    };
    assert_eq!(projection.reload(&backend, Instant::now()), Ok(&initial));
    let JournalActivationReconciliation::NotApplied(returned) = unsettled.reconcile(&projection)
    else {
        panic!("exact before frontier did not reconcile");
    };
    assert_eq!(returned.into_authority(), Authority(5));
    backend.assert_drained();

    let (third_frontier, _) = apply(&initial, &begin(2));
    let backend = Backend::new(
        [loaded(third_frontier.clone())],
        [Step::Begin {
            expected: initial.revision(),
            mutation: mutation.clone(),
            manifest: Arc::clone(&manifest),
            outcome: completed(ExtensionNativeOwnershipActivationOutcome::OutcomeUnknown),
        }],
    );
    let mut projection = JournalProjection {
        journal: Some(initial),
    };
    let JournalActivationSettlement::ReloadRequired(unsettled) =
        settled(projection.settle_fenced_begin_with_authority(
            &backend,
            Authority(6),
            mutation,
            Arc::clone(&manifest),
            Instant::now(),
        ))
    else {
        unreachable!();
    };
    assert_eq!(
        projection.reload(&backend, Instant::now()),
        Ok(&third_frontier)
    );
    let JournalActivationReconciliation::Diverged(diverged) = unsettled.reconcile(&projection)
    else {
        panic!("third frontier did not remain fail-stop");
    };
    drop(diverged);
    backend.assert_drained();
}

#[test]
fn may_own_ambiguity_preserves_authority_and_reload_pending_is_distinct() {
    let empty = ExtensionNativeOwnershipJournal::empty();
    let begin = begin(1);
    let (preparing_journal, _) = apply(&empty, &begin);
    let preparing = preparing_journal.entries()[0].clone();
    let identity = expected_identity(ExtensionRuntimeBackendTarget::MacosNative);
    let transition =
        ExtensionNativeOwnershipJournalMutation::transition_with_expected_native_identity(
            preparing.cas(),
            identity,
        );
    let (predicted, _) = apply(&preparing_journal, &transition);
    let manifest = manifest_for(package(1));

    let backend = Backend::new(
        [loaded(predicted.clone())],
        [Step::MayOwn {
            expected: preparing_journal.revision(),
            preparing: preparing.cas(),
            identity: Some(identity),
            manifest: Arc::clone(&manifest),
            outcome: Call::TimedOutAfterAdmission,
        }],
    );
    let mut projection = JournalProjection {
        journal: Some(preparing_journal.clone()),
    };
    let JournalActivationSettlement::ReloadRequired(unsettled) =
        settled(projection.settle_preparing_to_may_own_with_authority(
            &backend,
            Authority(20),
            preparing.clone(),
            Some(identity),
            Arc::clone(&manifest),
            Instant::now(),
        ))
    else {
        panic!("MayOwn timeout did not retain ambiguity authority");
    };
    assert!(projection.known().is_none());
    assert_eq!(projection.reload(&backend, Instant::now()), Ok(&predicted));
    let JournalActivationReconciliation::Applied(applied) = unsettled.reconcile(&projection) else {
        panic!("MayOwn predicted frontier did not reconcile");
    };
    let (authority, row) = applied.into_parts();
    assert_eq!(authority, Authority(20));
    assert_eq!(row.phase(), ExtensionNativeOwnershipPhase::NativeMayOwn);
    backend.assert_drained();

    let backend = Backend::new(
        [],
        [Step::MayOwn {
            expected: preparing_journal.revision(),
            preparing: preparing.cas(),
            identity: Some(identity),
            manifest: Arc::clone(&manifest),
            outcome: completed(ExtensionNativeOwnershipActivationOutcome::OutcomeUnknown),
        }],
    );
    let mut projection = JournalProjection {
        journal: Some(preparing_journal),
    };
    let JournalActivationSettlement::ReloadRequired(unsettled) =
        settled(projection.settle_preparing_to_may_own_with_authority(
            &backend,
            Authority(21),
            preparing,
            Some(identity),
            manifest,
            Instant::now(),
        ))
    else {
        unreachable!();
    };
    let JournalActivationReconciliation::ReloadPending(pending) = unsettled.reconcile(&projection)
    else {
        panic!("unknown projection did not retain reload-pending authority");
    };
    assert!(
        pending.additional_retained_bytes()
            <= JournalActivationReloadRequired::<Authority>::maximum_additional_retained_bytes()
    );
    backend.assert_drained();
}

#[test]
fn local_checks_are_store_free_and_return_authority_unchanged() {
    let backend = Backend::new([], []);
    let manifest = manifest_for(package(1));
    let mutation = begin(1);
    let mut projection = JournalProjection::unknown();
    let failure = rejected(projection.settle_fenced_begin_with_authority(
        &backend,
        Authority(1),
        mutation.clone(),
        Arc::clone(&manifest),
        Instant::now(),
    ));
    assert_eq!(
        failure.reason(),
        JournalActivationPreparationFailureReason::ReloadRequired
    );
    assert_eq!(failure.into_authority(), Authority(1));

    let initial = ExtensionNativeOwnershipJournal::empty();
    let mut projection = JournalProjection {
        journal: Some(initial.clone()),
    };
    let wrong_manifest = manifest_for(package(9));
    assert_eq!(
        rejected(projection.settle_fenced_begin_with_authority(
            &backend,
            Authority(2),
            mutation.clone(),
            wrong_manifest,
            Instant::now(),
        ))
        .reason(),
        JournalActivationPreparationFailureReason::ManifestPackageMismatch
    );

    let (preparing_journal, _) = apply(&initial, &mutation);
    let preparing = preparing_journal.entries()[0].clone();
    let mut projection = JournalProjection {
        journal: Some(preparing_journal.clone()),
    };
    assert_eq!(
        rejected(projection.settle_preparing_to_may_own_with_authority(
            &backend,
            Authority(3),
            preparing,
            None,
            Arc::clone(&manifest),
            Instant::now(),
        ))
        .reason(),
        JournalActivationPreparationFailureReason::InvalidIdentityDirection
    );
    assert_eq!(projection.known(), Some(&preparing_journal));

    let malformed =
        ExtensionNativeOwnershipJournalMutation::clear(preparing_journal.entries()[0].cas());
    let mut projection = JournalProjection {
        journal: Some(initial),
    };
    assert_eq!(
        rejected(projection.settle_fenced_begin_with_authority(
            &backend,
            Authority(4),
            malformed,
            manifest,
            Instant::now(),
        ))
        .reason(),
        JournalActivationPreparationFailureReason::MutationKindMismatch
    );
    backend.assert_drained();
}
