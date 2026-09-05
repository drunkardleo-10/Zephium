//! Synthetic native fixtures on the real application/runtime/controller paths.

use super::*;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use zephium_agent_controller::{
    AgentBrowserModel, AgentWorkContextSpec, AgentWorkRunSettings, AgentWorkTaskProgress,
    TerraControllerClock, TerraControllerClockError, TerraControllerIds,
};

static SERIAL: Mutex<()> = Mutex::new(());

struct Clock(AtomicU64);
impl TerraControllerClock for Clock {
    fn now(&self) -> Result<AgentPolicyInstant, TerraControllerClockError> {
        Ok(AgentPolicyInstant::from_millis(
            self.0.fetch_add(1, Ordering::Relaxed),
        ))
    }
}

struct Task;
impl AgentWorkTask for Task {
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        assert_eq!(observation.frames().len(), 1);
        Ok(AgentWorkTaskProgress::Complete)
    }
    fn assess(
        &self,
        _: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        Ok(AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            AgentAccountScope::Anonymous,
            now,
        ))
    }
}

fn input() -> AgentWorkRunInput {
    input_with_deadline(Instant::now() + Duration::from_secs(10))
}

fn input_with_deadline(deadline: Instant) -> AgentWorkRunInput {
    let profile = 1_u128.into();
    let context = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        profile,
        ContextKind::Owned,
    );
    let origin = SemanticOrigin::parse("https://application-fixture.invalid/").unwrap();
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).unwrap();
    let budget = AgentRunBudget::try_new(24, 1_000_000, 1_000_000, 1).unwrap();
    let node = AgentPlanNodeId::generate();
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
        context.owner(),
        AgentRunScope::try_new(
            vec![profile],
            vec![AgentAccountScope::Anonymous],
            vec![origin.clone()],
            SemanticSensitivity::Public,
            effects,
            Vec::new(),
        )
        .unwrap(),
        budget,
        AgentPolicyInstant::from_millis(1),
        AgentPolicyInstant::from_millis(100_000),
        vec![AgentPlanNodeScope::new(
            node,
            AgentPlanNodeAuthority::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin],
                SemanticSensitivity::Public,
                effects,
            )
            .unwrap(),
            budget,
            AgentPolicyInstant::from_millis(99_999),
        )],
    )
    .unwrap();
    let ids = TerraControllerIds::try_new(
        AgentSupervisorId::new(1).unwrap(),
        AgentSupervisorAttemptId::new(1).unwrap(),
        AgentSupervisorCancellationId::new(1).unwrap(),
        AgentModelCallId::new(1).unwrap(),
        [1, 2, 3, 4].map(|id| AgentAuditEventId::new(id).unwrap()),
        AgentAuditDeliveryId::new(1).unwrap(),
    )
    .unwrap();
    AgentWorkRunInput::try_new(
        manifest,
        AgentPlanLeaseBinding::new(AgentPlanLeaseId::generate(), node),
        AgentWorkContextSpec::try_new(
            context,
            ContextProfileStorageClass::Ephemeral,
            ContextNavigationTarget::parse("https://application-fixture.invalid/").unwrap(),
        )
        .unwrap(),
        "Verify the synthetic application fixture.".into(),
        AgentWorkRunSettings::new(
            AgentBrowserModel::Luna,
            ids,
            Arc::new(Clock(AtomicU64::new(2))),
            deadline,
        ),
    )
    .unwrap()
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Fault {
    None,
    ObservationLost,
    CloseLost,
    ObservationRefused,
}

struct NativeFixture {
    sink: NativeEventSink,
    fault: Fault,
    calls: Arc<Mutex<Vec<u8>>>,
}
impl AgentBrowserPort for NativeFixture {
    fn dispatch(&self, request: ContextNativeRequest) -> ContextDispatch {
        let event = match request {
            ContextNativeRequest::Construct(request) => {
                lock(&self.calls).push(1);
                ContextNativeEvent::ConstructionSettled(
                    ContextConstructionSettlement::try_new(
                        request.operation(),
                        Ok(ContextConstructionProof::MacOsOwnedSelectedProfileExtensionFree),
                    )
                    .unwrap(),
                )
            }
            ContextNativeRequest::Navigate(request) => {
                lock(&self.calls).push(2);
                ContextNativeEvent::NavigationSettled(
                    ContextNavigationSettlement::try_new(
                        request.operation(),
                        Ok(request.target().clone()),
                    )
                    .unwrap(),
                )
            }
            ContextNativeRequest::Cancel(request) => {
                lock(&self.calls).push(4);
                ContextNativeEvent::CancellationSettled(ContextCancellationSettlement::new(
                    request.current(),
                    Ok(()),
                ))
            }
            ContextNativeRequest::Transition(request) => {
                lock(&self.calls).push(5);
                assert_eq!(request.operation().kind(), ContextOperationKind::Close);
                if self.fault == Fault::CloseLost {
                    return ContextDispatch::Scheduled;
                }
                ContextNativeEvent::TransitionSettled(
                    ContextTransitionSettlement::try_new(request.operation(), Ok(())).unwrap(),
                )
            }
        };
        let _ = self.sink.publish(event);
        ContextDispatch::Scheduled
    }
    fn invoke_semantic(&self, invocation: SemanticRuntimeInvocation) -> ContextDispatch {
        lock(&self.calls).push(3);
        if matches!(self.fault, Fault::ObservationLost | Fault::CloseLost) {
            return ContextDispatch::Scheduled;
        }
        let correlation = invocation.correlation();
        let wire = format!("{{\"v\":1,\"i\":{},\"g\":{},\"c\":\"complete\",\"n\":[{{\"k\":1,\"r\":\"document\",\"o\":16}}]}}", correlation.invocation().get(), correlation.snapshot_generation().get());
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                correlation.invocation(),
                correlation.frame().clone(),
                correlation.snapshot_generation(),
            ),
            wire.as_bytes(),
        )
        .unwrap();
        let result = if self.fault == Fault::ObservationRefused {
            Err(SemanticRuntimePortFailure::Transport)
        } else {
            Ok(snapshot)
        };
        let _ = self
            .sink
            .publish(ContextNativeEvent::SemanticRuntimeSettled(Box::new(
                SemanticRuntimeSettlement::try_new(correlation, result).unwrap(),
            )));
        ContextDispatch::Scheduled
    }
    fn seal_for_shutdown(&self, audit: ContextResourceAuditId) -> ContextShutdownDispatch {
        lock(&self.calls).push(6);
        let snapshot = ContextNativeResourceSnapshot::try_new(ContextNativeResourceCounts {
            known_bindings: 0,
            resident_views: 0,
            owned_reservations: 0,
            borrowed_leases: 0,
            visible_surfaces: 0,
            suspended_views: 0,
            pending_operations: 0,
            pending_captures: 0,
            queued_tasks: 0,
        })
        .unwrap();
        let _ = self.sink.publish(ContextNativeEvent::ShutdownAuditSettled(
            ContextShutdownAuditSettlement::new(audit, Ok(snapshot)),
        ));
        ContextShutdownDispatch::AuditScheduled
    }
    fn transfer_cookies(&self, _: ContextCookieTransferRequest) -> ContextDispatch {
        ContextDispatch::Unsupported
    }
    fn audit_resources(&self, _: ContextResourceAuditId) -> ContextDispatch {
        ContextDispatch::Unsupported
    }
    fn execute_semantic_action(
        &self,
        _: SemanticActionNativeRequest,
        _: SemanticActionNativeCompletion,
    ) -> ContextDispatch {
        panic!("trusted complete fixture cannot propose actions")
    }
    fn capture_semantic_screenshot(
        &self,
        _: SemanticScreenshotNativeRequest,
        _: SemanticScreenshotNativeCompletion,
    ) -> ContextDispatch {
        ContextDispatch::Unsupported
    }
}

fn prepared(
    audit: Arc<dyn AgentAuditPort>,
    fault: Fault,
    calls: Arc<Mutex<Vec<u8>>>,
    factories: Arc<AtomicUsize>,
) -> PreparedAgentWork {
    PreparedAgentWork::try_new(
        input(),
        AgentWorkApplicationConfig::new(
            AgentRuntimeConfig::STANDARD,
            AgentProviderTransportConfig::STANDARD,
        ),
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-not-a-secret".into(),
        )
        .unwrap(),
        Box::new(Task),
        AgentWorkApplicationPorts::new(
            fixture_engine(),
            audit,
            Box::new(move |sink| {
                factories.fetch_add(1, Ordering::AcqRel);
                Some(Arc::new(NativeFixture { sink, fault, calls }))
            }),
        ),
    )
    .unwrap()
}

#[derive(Default)]
struct Journal {
    audit_proofs: Mutex<Vec<AgentAuditDeliveryProof>>,
    lose_audit: AtomicBool,
    pending: Mutex<VecDeque<(AgentWorkJournalRequest, AgentWorkJournalCompletion)>>,
}
impl AgentWorkJournalPort for Journal {
    fn dispatch(
        &self,
        request: AgentWorkJournalRequest,
        callback: AgentWorkJournalCompletion,
    ) -> Result<(), AgentWorkJournalError> {
        lock(&self.pending).push_back((request, callback));
        Ok(())
    }
}
impl AgentAuditPort for Journal {
    fn append(
        &self,
        delivery: AgentAuditDelivery,
        completion: AgentAuditCompletion,
    ) -> AgentAuditDispatch {
        let proof = delivery.proof();
        lock(&self.audit_proofs).push(proof);
        if !self.lose_audit.load(Ordering::Acquire) {
            completion(proof.settle(AgentAuditDeliveryOutcome::Committed));
        }
        AgentAuditDispatch::Accepted(proof)
    }
}
impl Journal {
    fn settle(
        &self,
        reply: impl FnOnce(AgentWorkJournalRequest) -> DurableResult,
    ) -> AgentWorkJournalRequest {
        let (request, callback) = lock(&self.pending).pop_front().unwrap();
        callback(reply(request));
        request
    }
    fn commit(&self) {
        self.settle(|request| match request {
            AgentWorkJournalRequest::CompareAndSet(mutation) => {
                Ok(AgentWorkJournalReply::Record(Some(mutation.next())))
            }
            _ => panic!("expected mutation"),
        });
    }
}

fn fixture_engine() -> Arc<crate::shell::tests::FakeEngine> {
    static ENGINE: std::sync::OnceLock<Arc<crate::shell::tests::FakeEngine>> =
        std::sync::OnceLock::new();
    ENGINE
        .get_or_init(|| Arc::new(crate::shell::tests::FakeEngine::default()))
        .clone()
}

fn coordinator(
    journal: Arc<Journal>,
) -> (
    ApplicationWork,
    crate::actor::Handle,
    AgentWorkApplicationHandle,
) {
    let queue = crate::actor::CommandQueue::new();
    let owner = crate::actor::Handle::new(queue.clone());
    let handle = owner
        .callback_handle()
        .attach_work(journal, fixture_engine())
        .unwrap();
    let Command::AttachWork(attachment) = queue.try_recv().unwrap() else {
        panic!()
    };
    (
        ApplicationWork::take_attachment(&attachment).unwrap(),
        owner,
        handle,
    )
}

fn pump(actor: &mut ApplicationWork, predicate: impl Fn(&ApplicationWork) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        actor.poll();
        if predicate(actor) {
            break;
        }
        assert!(Instant::now() < deadline, "bounded application fixture");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn start(actor: &mut ApplicationWork, journal: &Journal, staged: PreparedAgentWork) {
    actor.initialize();
    actor.admit(WorkSubmission(
        Arc::new(Mutex::new(Some(staged))),
        actor.projection.clone(),
    ));
    journal.settle(|_| {
        Ok(AgentWorkJournalReply::Claimed {
            owner: AgentWorkIncarnation::generate(),
            records: Vec::new(),
        })
    });
    actor.poll();
    journal.commit();
    actor.poll();
    journal.commit();
    actor.poll();
}

#[test]
fn native_creation_waits_for_both_exact_durable_admission_acknowledgements() {
    let _serial = lock(&SERIAL);
    let journal = Arc::new(Journal::default());
    let (mut actor, _owner, _view) = coordinator(journal.clone());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let factories = Arc::new(AtomicUsize::new(0));
    let staged = prepared(
        journal.clone(),
        Fault::None,
        calls.clone(),
        factories.clone(),
    );
    assert_eq!(
        format!("{staged:?}"),
        "PreparedAgentWork([owned, redacted])"
    );
    actor.initialize();
    actor.admit(WorkSubmission(
        Arc::new(Mutex::new(Some(staged))),
        actor.projection.clone(),
    ));
    assert_eq!(factories.load(Ordering::Acquire), 0);
    journal.settle(|_| {
        Ok(AgentWorkJournalReply::Claimed {
            owner: AgentWorkIncarnation::generate(),
            records: Vec::new(),
        })
    });
    actor.poll();
    assert_eq!(factories.load(Ordering::Acquire), 0);
    journal.commit();
    actor.poll();
    assert_eq!(factories.load(Ordering::Acquire), 0);
    journal.commit();
    actor.poll();
    assert_eq!(factories.load(Ordering::Acquire), 1);
    pump(&mut actor, |actor| {
        actor
            .flight
            .as_ref()
            .is_some_and(|flight| matches!(flight.purpose, DurablePurpose::Terminal))
    });
    assert_ne!(
        lock(&actor.projection).snapshot.phase,
        AgentWorkApplicationPhase::Succeeded
    );
    journal.commit();
    actor.poll();
    assert_eq!(
        lock(&actor.projection).snapshot.phase,
        AgentWorkApplicationPhase::Succeeded
    );
    assert_eq!(*lock(&calls), [1, 2, 3, 4, 5, 6]);
    assert!(actor.shutdown_until(Instant::now() + Duration::from_secs(1)));
}

#[test]
fn uncertain_admission_retains_exact_intent_and_never_starts_without_explicit_reconciliation() {
    let journal = Arc::new(Journal::default());
    let (mut actor, _owner, _) = coordinator(journal.clone());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let factories = Arc::new(AtomicUsize::new(0));
    actor.initialize();
    actor.admit(WorkSubmission(
        Arc::new(Mutex::new(Some(prepared(
            journal.clone(),
            Fault::None,
            calls,
            factories.clone(),
        )))),
        actor.projection.clone(),
    ));
    journal.settle(|_| {
        Ok(AgentWorkJournalReply::Claimed {
            owner: AgentWorkIncarnation::generate(),
            records: Vec::new(),
        })
    });
    actor.poll();
    let original = journal.settle(|_| Err(AgentWorkJournalError::Uncertain));
    actor.poll();
    for _ in 0..8 {
        actor.poll();
    }
    assert!(lock(&journal.pending).is_empty());
    assert_eq!(factories.load(Ordering::Acquire), 0);
    actor.stopping = true;
    actor.control(WorkCommand {
        projection: actor.projection.clone(),
        control: WorkControl::Reconcile,
    });
    let replay = journal.settle(|request| {
        let AgentWorkJournalRequest::CompareAndSet(mutation) = request else {
            panic!()
        };
        Ok(AgentWorkJournalReply::Record(Some(mutation.next())))
    });
    let (
        AgentWorkJournalRequest::CompareAndSet(first),
        AgentWorkJournalRequest::CompareAndSet(replayed),
    ) = (original, replay)
    else {
        panic!()
    };
    assert_eq!(first.next(), replayed.next());
    actor.poll();
    journal.commit();
    actor.poll();
    assert_eq!(factories.load(Ordering::Acquire), 0);
    assert_eq!(
        actor.record.unwrap().disposition(),
        AgentWorkDisposition::FailedClosed
    );
}

#[test]
fn lost_native_callbacks_and_takeover_persist_recovery_without_success_or_replay() {
    let _serial = lock(&SERIAL);
    for fault in [
        Fault::ObservationLost,
        Fault::CloseLost,
        Fault::ObservationRefused,
    ] {
        let journal = Arc::new(Journal::default());
        let (mut actor, _owner, _) = coordinator(journal.clone());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let factories = Arc::new(AtomicUsize::new(0));
        let staged = prepared(journal.clone(), fault, calls.clone(), factories.clone());
        let run = staged.run;
        start(&mut actor, &journal, staged);
        pump(&mut actor, |_| lock(&calls).contains(&3));
        actor.control(WorkCommand {
            projection: actor.projection.clone(),
            control: WorkControl::Stop {
                run,
                reason: AgentRuntimeStopReason::HumanTakeover,
            },
        });
        pump(&mut actor, |actor| actor.flight.is_some());
        journal.commit();
        actor.poll();
        assert_eq!(
            actor.record.unwrap().disposition(),
            AgentWorkDisposition::RecoveryRequired
        );
        assert_eq!(actor.record.unwrap().debt(), AgentWorkDebt::UNKNOWN);
        assert_eq!(factories.load(Ordering::Acquire), 1);
        assert!(matches!(
            actor.active.as_ref().unwrap().outcome,
            Some(AgentWorkOutcome::Recovery(_))
        ));
        let native_before = lock(&calls).clone();
        journal.lose_audit.store(true, Ordering::Release);
        actor.control(WorkCommand {
            projection: actor.projection.clone(),
            control: WorkControl::Reconcile,
        });
        let first = *lock(&journal.audit_proofs).last().unwrap();
        actor.recovery_audit.as_mut().unwrap().deadline = Instant::now();
        actor.poll();
        journal.lose_audit.store(false, Ordering::Release);
        actor.control(WorkCommand {
            projection: actor.projection.clone(),
            control: WorkControl::Reconcile,
        });
        pump(&mut actor, |actor| actor.recovery_audit.is_none());
        let proofs = lock(&journal.audit_proofs);
        assert!(proofs
            .windows(2)
            .any(|pair| pair[0] == first && pair[1] == first));
        assert!(lock(&actor.projection)
            .snapshot
            .recovery_audit
            .unwrap()
            .unwrap()
            .shutdown_sealed());
        assert_eq!(
            lock(&actor.projection)
                .snapshot
                .recovery_audit
                .unwrap()
                .unwrap()
                .pending(),
            0
        );
        assert_eq!(*lock(&calls), native_before);
        drop(proofs);
        assert!(!actor.shutdown_until(Instant::now() + Duration::from_secs(1)));
    }
}

#[test]
fn unclean_lifecycle_handoff_consumes_the_original_late_worker_outcome() {
    let _serial = lock(&SERIAL);
    let journal = Arc::new(Journal::default());
    let (mut actor, _owner, _) = coordinator(journal.clone());
    let calls = Arc::new(Mutex::new(Vec::new()));
    start(
        &mut actor,
        &journal,
        prepared(
            journal.clone(),
            Fault::ObservationLost,
            calls.clone(),
            Arc::new(AtomicUsize::new(0)),
        ),
    );
    pump(&mut actor, |_| lock(&calls).contains(&3));
    let active = actor.active.as_mut().unwrap();
    assert!(active.outcome.is_none());
    // The original lifecycle's Drop seals and hands the original worker to
    // its reaper. It does not construct a replacement lifecycle or proof.
    drop(active.lifecycle.take().unwrap());
    active.lifecycle_clean = Some(false);
    pump(&mut actor, |actor| {
        actor.active.as_ref().unwrap().outcome.is_some()
    });
    assert!(matches!(
        actor.active.as_ref().unwrap().outcome,
        Some(AgentWorkOutcome::Recovery(_))
    ));
    journal.commit();
    actor.poll();
    assert_eq!(
        actor.record.unwrap().disposition(),
        AgentWorkDisposition::RecoveryRequired
    );
    assert_eq!(actor.record.unwrap().debt(), AgentWorkDebt::UNKNOWN);
    assert!(!actor.shutdown_until(Instant::now() + Duration::from_millis(1)));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn actual_shell_command_path_closes_the_same_sqlite_store_after_durable_success() {
    let _serial = lock(&SERIAL);
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(directory.path()).unwrap());
    let queue = crate::actor::CommandQueue::new();
    let owner = crate::actor::Handle::new(queue.clone());
    let engine = fixture_engine();
    let mut shell = crate::Shell::new(
        engine.clone(),
        store.clone(),
        Arc::new(crate::shell::tests::FakeChrome),
        Box::new(|_| {}),
    );
    shell.attach_queue(queue.clone());
    let wrong = owner
        .callback_handle()
        .attach_work(
            store.clone(),
            Arc::new(crate::shell::tests::FakeEngine::default()),
        )
        .unwrap();
    shell.handle(queue.try_recv().unwrap());
    assert_eq!(wrong.snapshot().failure, Some(AgentWorkFailure::Contract));
    let view = owner
        .callback_handle()
        .attach_work(store.clone(), engine.clone())
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let factories = Arc::new(AtomicUsize::new(0));
    view.admit(prepared(
        store.clone(),
        Fault::None,
        calls.clone(),
        factories.clone(),
    ))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while view.snapshot().phase != AgentWorkApplicationPhase::Succeeded {
        if let Some(command) = queue.try_recv() {
            shell.handle(command);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            Instant::now() < deadline,
            "actual shell fixture deadline: {:?}",
            view.snapshot()
        );
    }
    assert_eq!(view.records().len(), 1);
    assert_eq!(
        view.records()[0].disposition(),
        AgentWorkDisposition::Succeeded
    );
    assert_eq!(factories.load(Ordering::Acquire), 1);
    assert_eq!(*lock(&calls), [1, 2, 3, 4, 5, 6]);
    let second = owner
        .callback_handle()
        .attach_work(store.clone(), engine)
        .unwrap();
    while second.snapshot().phase == AgentWorkApplicationPhase::Loading {
        shell.handle(queue.try_recv().unwrap());
    }
    assert_eq!(second.snapshot().phase, AgentWorkApplicationPhase::Recovery);
    let second_factories = Arc::new(AtomicUsize::new(0));
    second
        .admit(prepared(
            store.clone(),
            Fault::None,
            Arc::new(Mutex::new(Vec::new())),
            second_factories.clone(),
        ))
        .unwrap();
    while let Some(command) = queue.try_recv() {
        shell.handle(command);
    }
    assert_eq!(second_factories.load(Ordering::Acquire), 0);
    assert_eq!(view.snapshot().phase, AgentWorkApplicationPhase::Succeeded);
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    shell.handle(Command::Shutdown {
        deadline: Instant::now() + Duration::from_secs(2),
        ack: tx,
    });
    assert_eq!(rx.recv().unwrap(), crate::ShutdownOutcome::Clean);
}

fn historical(
    owner: AgentWorkIncarnation,
    run: ContextRunId,
    disposition: AgentWorkDisposition,
) -> AgentWorkRecord {
    let mut bytes = [0; AGENT_WORK_RECORD_BYTES];
    bytes[0] = 1;
    bytes[1] = disposition as u8;
    bytes[2] = 63;
    bytes[15] = 3;
    bytes[16..32].copy_from_slice(&owner.bytes());
    bytes[47] = 1;
    bytes[48..64].copy_from_slice(&run.bytes());
    AgentWorkRecord::decode(bytes).unwrap()
}

#[test]
fn recovered_review_accept_reject_stale_replay_and_foreign_handle_are_nonexecuting() {
    for decision in [
        AgentWorkReviewDecision::AcceptFreshAdmission,
        AgentWorkReviewDecision::Reject,
    ] {
        let journal = Arc::new(Journal::default());
        let (mut actor, _owner, _) = coordinator(journal.clone());
        actor.initialize();
        let owner = AgentWorkIncarnation::generate();
        let record = historical(
            owner,
            ContextRunId::generate(),
            AgentWorkDisposition::Interrupted,
        );
        journal.settle(|_| {
            Ok(AgentWorkJournalReply::Claimed {
                owner,
                records: vec![record],
            })
        });
        actor.poll();
        let control = WorkControl::Review { record, decision };
        actor.control(WorkCommand {
            projection: actor.projection.clone(),
            control,
        });
        actor.control(WorkCommand {
            projection: actor.projection.clone(),
            control,
        });
        assert_eq!(lock(&journal.pending).len(), 1);
        journal.commit();
        actor.poll();
        let terminal = lock(&actor.projection)
            .snapshot
            .last_review
            .unwrap()
            .unwrap();
        assert!(terminal.disposition().is_terminal());
        assert_eq!(terminal.debt(), AgentWorkDebt::UNKNOWN);
        assert!(actor.active.is_none() && actor.staged.is_none());
        actor.control(WorkCommand {
            projection: actor.projection.clone(),
            control,
        });
        assert_eq!(
            lock(&actor.projection).snapshot.last_review,
            Some(Err(AgentWorkJournalError::Conflict))
        );
        actor.control(WorkCommand {
            projection: actor.projection.clone(),
            control: WorkControl::Review {
                record: terminal,
                decision,
            },
        });
        assert_eq!(
            lock(&actor.projection).snapshot.last_review,
            Some(Err(AgentWorkJournalError::Transition))
        );
        let (_other, _other_owner, other_view) = coordinator(Arc::new(Journal::default()));
        actor.control(WorkCommand {
            projection: other_view.projection,
            control,
        });
        assert!(lock(&journal.pending).is_empty());
    }
}

#[test]
fn cancellation_and_review_are_ordered_by_the_original_record_cas() {
    for stop_first in [true, false] {
        let journal = Arc::new(Journal::default());
        let (mut actor, _owner, _) = coordinator(journal.clone());
        actor.initialize();
        let owner = AgentWorkIncarnation::generate();
        let run = ContextRunId::generate();
        let record = historical(owner, run, AgentWorkDisposition::NeedsApproval);
        journal.settle(|_| {
            Ok(AgentWorkJournalReply::Claimed {
                owner,
                records: vec![record],
            })
        });
        actor.poll();
        actor.record = Some(record);
        lock(&actor.projection).snapshot.run = Some(run);
        let review = WorkControl::Review {
            record,
            decision: AgentWorkReviewDecision::AcceptFreshAdmission,
        };
        let stop = WorkControl::Stop {
            run,
            reason: AgentRuntimeStopReason::Cancelled,
        };
        for control in if stop_first {
            [stop, review]
        } else {
            [review, stop]
        } {
            actor.control(WorkCommand {
                projection: actor.projection.clone(),
                control,
            });
        }
        assert_eq!(lock(&journal.pending).len(), 1);
        journal.commit();
        actor.poll();
        assert_eq!(
            actor.record.unwrap().disposition(),
            if stop_first {
                AgentWorkDisposition::FailedClosed
            } else {
                AgentWorkDisposition::FreshAdmissionRequired
            }
        );
        assert!(actor.active.is_none());
    }
}

#[test]
fn cancelled_or_expired_admission_never_invokes_the_native_factory() {
    for reason in [
        None,
        Some(AgentRuntimeStopReason::Cancelled),
        Some(AgentRuntimeStopReason::HumanTakeover),
    ] {
        let journal = Arc::new(Journal::default());
        let (mut actor, _owner, _) = coordinator(journal.clone());
        actor.initialize();
        let factories = Arc::new(AtomicUsize::new(0));
        let mut staged = prepared(
            journal.clone(),
            Fault::None,
            Arc::new(Mutex::new(Vec::new())),
            factories.clone(),
        );
        let run = staged.run;
        if reason.is_none() {
            staged.deadline = Instant::now();
        }
        actor.admit(WorkSubmission(
            Arc::new(Mutex::new(Some(staged))),
            actor.projection.clone(),
        ));
        if let Some(reason) = reason {
            actor.control(WorkCommand {
                projection: actor.projection.clone(),
                control: WorkControl::Stop { run, reason },
            });
        }
        journal.settle(|_| {
            Ok(AgentWorkJournalReply::Claimed {
                owner: AgentWorkIncarnation::generate(),
                records: Vec::new(),
            })
        });
        actor.poll();
        while !lock(&journal.pending).is_empty() {
            journal.commit();
            actor.poll();
        }
        assert_eq!(factories.load(Ordering::Acquire), 0);
        assert_eq!(
            actor.record.unwrap().disposition(),
            AgentWorkDisposition::FailedClosed
        );
        assert!(actor.active.is_none());
        assert!(matches!(
            actor.unstarted,
            Some(AgentWorkOutcome::Recovery(_))
        ));
        assert!(!actor.shutdown_until(Instant::now() + Duration::from_millis(1)));
    }
}

#[test]
fn native_factory_refusal_panic_and_runtime_refusal_retain_original_recovery() {
    let _serial = lock(&SERIAL);
    for fault in 0..3 {
        let journal = Arc::new(Journal::default());
        let (mut actor, _owner, _) = coordinator(journal.clone());
        let mut staged = prepared(
            journal.clone(),
            Fault::None,
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(AtomicUsize::new(0)),
        );
        staged.native = Box::new(move |_| {
            assert_ne!(fault, 1, "synthetic factory fault");
            None
        });
        let pending = (fault == 2)
            .then(|| PendingAgentRuntime::spawn_suspended(AgentRuntimeConfig::STANDARD).unwrap());
        start(&mut actor, &journal, staged);
        actor.poll();
        assert!(actor.active.is_none());
        assert!(matches!(
            actor.unstarted,
            Some(AgentWorkOutcome::Recovery(_))
        ));
        journal.commit();
        actor.poll();
        assert_eq!(
            actor.record.unwrap().disposition(),
            AgentWorkDisposition::FailedClosed
        );
        actor.control(WorkCommand {
            projection: actor.projection.clone(),
            control: WorkControl::Reconcile,
        });
        pump(&mut actor, |actor| actor.recovery_audit.is_none());
        assert!(
            lock(&actor.projection)
                .snapshot
                .recovery_audit
                .unwrap()
                .unwrap()
                .pending()
                == 0
        );
        assert!(!actor.shutdown_until(Instant::now() + Duration::from_millis(1)));
        drop(pending);
    }
}

#[test]
fn uncertain_terminal_ack_reconciles_exactly_once_without_reentering_runtime() {
    let _serial = lock(&SERIAL);
    let journal = Arc::new(Journal::default());
    let (mut actor, _owner, _) = coordinator(journal.clone());
    let factories = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(Mutex::new(Vec::new()));
    start(
        &mut actor,
        &journal,
        prepared(
            journal.clone(),
            Fault::None,
            calls.clone(),
            factories.clone(),
        ),
    );
    pump(&mut actor, |actor| actor.flight.is_some());
    let request = journal.settle(|_| Err(AgentWorkJournalError::Uncertain));
    actor.poll();
    assert_eq!(
        lock(&actor.projection).snapshot.phase,
        AgentWorkApplicationPhase::PersistenceUncertain
    );
    assert!(actor.active.as_ref().unwrap().native.is_some());
    let native_before = lock(&calls).clone();
    actor.control(WorkCommand {
        projection: actor.projection.clone(),
        control: WorkControl::Reconcile,
    });
    let replay = journal.settle(|request| {
        let AgentWorkJournalRequest::CompareAndSet(mutation) = request else {
            panic!()
        };
        Ok(AgentWorkJournalReply::Record(Some(mutation.next())))
    });
    let (
        AgentWorkJournalRequest::CompareAndSet(first),
        AgentWorkJournalRequest::CompareAndSet(second),
    ) = (request, replay)
    else {
        panic!()
    };
    assert_eq!(first.next(), second.next());
    actor.poll();
    assert_eq!(
        lock(&actor.projection).snapshot.phase,
        AgentWorkApplicationPhase::Succeeded
    );
    for _ in 0..8 {
        actor.poll();
    }
    assert!(lock(&journal.pending).is_empty());
    assert_eq!(*lock(&calls), native_before);
    assert_eq!(factories.load(Ordering::Acquire), 1);
}

#[test]
fn mailbox_pressure_coalesces_work_wakes_across_user_barriers() {
    let queue = crate::actor::CommandQueue::new();
    let _owner = crate::actor::Handle::new(queue.clone());
    for _ in 0..100 {
        assert!(queue.try_push(Command::Open).is_ok());
        assert!(queue.try_push(Command::WorkWake).is_ok());
    }
    let mut wakes = 0;
    while let Some(command) = queue.try_recv() {
        if matches!(command, Command::WorkWake) {
            wakes += 1;
        }
    }
    assert_eq!(wakes, 1);
    while queue.try_push(Command::Open).is_ok() {}
    assert!(queue.try_push(Command::WorkWake).is_ok());
    let mut wakes = 0;
    while let Some(command) = queue.try_recv() {
        wakes += usize::from(matches!(command, Command::WorkWake));
    }
    assert_eq!(
        wakes, 1,
        "ordinary FIFO saturation cannot exclude a Work wake"
    );
}

#[test]
fn callback_deadline_and_shutdown_keep_uncertain_admission_owned() {
    let journal = Arc::new(Journal::default());
    let (mut actor, _owner, _) = coordinator(journal.clone());
    actor.initialize();
    actor.flight.as_mut().unwrap().deadline = Instant::now();
    actor.poll();
    assert_eq!(
        lock(&actor.projection).snapshot.phase,
        AgentWorkApplicationPhase::PersistenceUncertain
    );
    assert!(!actor.shutdown_until(Instant::now() + Duration::from_millis(1)));
    assert!(actor.flight.is_some());
    assert_eq!(lock(&journal.pending).len(), 1);
    journal.settle(|_| {
        Ok(AgentWorkJournalReply::Claimed {
            owner: AgentWorkIncarnation::generate(),
            records: Vec::new(),
        })
    });
    actor.poll();
    assert!(actor.shutdown_until(Instant::now() + Duration::from_millis(1)));
}

#[test]
fn mismatched_durable_and_audit_owners_are_refused_before_runtime_creation() {
    for foreign_engine in [false, true] {
        let journal = Arc::new(Journal::default());
        let (mut actor, _owner, _) = coordinator(journal.clone());
        let other = if foreign_engine {
            journal.clone()
        } else {
            Arc::new(Journal::default())
        };
        let factories = Arc::new(AtomicUsize::new(0));
        let mut staged = prepared(
            other,
            Fault::None,
            Arc::new(Mutex::new(Vec::new())),
            factories.clone(),
        );
        if foreign_engine {
            staged.engine = Arc::new(crate::shell::tests::FakeEngine::default());
        }
        actor.admit(WorkSubmission(
            Arc::new(Mutex::new(Some(staged))),
            actor.projection.clone(),
        ));
        assert_eq!(
            lock(&actor.projection).snapshot.failure,
            Some(AgentWorkFailure::Contract)
        );
        assert!(actor.active.is_none());
        assert_eq!(factories.load(Ordering::Acquire), 0);
        assert!(lock(&journal.pending).is_empty());
    }
}

#[test]
fn expired_preparation_never_claims_journal_or_native_and_fresh_preparation_survives() {
    let journal = Arc::new(Journal::default());
    let factories = Arc::new(AtomicUsize::new(0));
    let factory_calls = factories.clone();
    let deadline = Instant::now() + Duration::from_millis(100);
    let stale = input_with_deadline(deadline);
    std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
    let rejected = PreparedAgentWork::try_new(
        stale,
        AgentWorkApplicationConfig::new(
            AgentRuntimeConfig::STANDARD,
            AgentProviderTransportConfig::STANDARD,
        ),
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-not-a-secret".into(),
        )
        .unwrap(),
        Box::new(Task),
        AgentWorkApplicationPorts::new(
            fixture_engine(),
            journal.clone(),
            Box::new(move |_| {
                factory_calls.fetch_add(1, Ordering::AcqRel);
                None
            }),
        ),
    );
    assert!(matches!(rejected, Err(AgentWorkFailure::Deadline)));
    assert_eq!(factories.load(Ordering::Acquire), 0);
    assert!(lock(&journal.pending).is_empty());
    let fresh = prepared(
        journal.clone(),
        Fault::None,
        Arc::new(Mutex::new(Vec::new())),
        factories.clone(),
    );
    assert!(Arc::ptr_eq(
        &fresh.engine,
        &(fixture_engine() as crate::SharedEngine)
    ));
    assert_eq!(factories.load(Ordering::Acquire), 0);
    assert!(lock(&journal.pending).is_empty());
}
