//! Exact policy refusal through the production actor and durable application.
//! Synthetic provider/native data; all closure owners are the originals.
use super::*;
use zephium_agent_controller::AgentBrowserRetention;

struct RefusedTask;
impl AgentWorkTask for RefusedTask {
    fn model_action_operations(
        &self,
        node: &SemanticNode,
        _: &SemanticObservation,
    ) -> Result<SemanticOperations, AgentWorkFailure> {
        // Reach the exact effect-policy refusal under test. Advertising this
        // synthetic operation does not approve its LocalWrite effect.
        if node.name().is_some_and(|name| name.as_str() == "Field") {
            SemanticOperations::try_new(&[SemanticOperationClass::Fill])
                .map_err(|_| AgentWorkFailure::Contract)
        } else {
            Ok(SemanticOperations::NONE)
        }
    }
    fn evaluate(
        &mut self,
        _: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        Ok(AgentWorkTaskProgress::Continue)
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        Ok(AgentEffectAssessment::new(
            action,
            action.frame().origin().clone(),
            SemanticEffectClass::LocalWrite,
        ))
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        Task.attest_account(context, now)
    }
}

fn prepared_review(
    journal: Arc<Journal>,
    calls: Arc<Mutex<Vec<u8>>>,
) -> (PreparedAgentWork, std::thread::JoinHandle<usize>) {
    let response = artifact_tests::response_stream(1)
        .replace("\"extract\"", "\"act\"")
        .replace(r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
            r#"{\"actions\":[{\"kind\":\"fill\",\"target\":\"@a2\",\"value\":\"fixture value\",\"effect\":\"local_write\",\"wait\":{\"kind\":\"immediate\"},\"verification\":{\"kind\":\"target_value_matches_input\"},\"settle_millis\":2000}]}"#);
    let (transport, server) = artifact_tests::fixture_provider_responses(vec![response]);
    let (controller, handle) = AgentWorkController::try_new_for_probe(
        input(),
        transport,
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-not-a-secret".into(),
        )
        .unwrap(),
        journal.clone(),
        Box::new(RefusedTask),
        AgentBrowserRetention::Stateless,
    )
    .unwrap();
    let ports = AgentWorkApplicationPorts::new(
        fixture_engine(),
        journal,
        Box::new(move |sink| {
            Some(Arc::new(NativeFixture {
                sink,
                fault: Fault::Form,
                calls,
            }))
        }),
    );
    (
        PreparedAgentWork::from_controller(controller, handle, AgentRuntimeConfig::STANDARD, ports)
            .unwrap(),
        server,
    )
}

fn command(actor: &mut ApplicationWork, control: WorkControl) {
    actor.control(WorkCommand {
        projection: actor.projection.clone(),
        control,
    });
}

#[test]
fn proof_closed_review_requires_exact_ack_and_fresh_successor_owners() {
    let _serial = lock(&SERIAL);
    for decision in [
        AgentWorkReviewDecision::AcceptFreshAdmission,
        AgentWorkReviewDecision::Reject,
    ] {
        for lose_ack in [false, true] {
            let journal = Arc::new(Journal::default());
            let (mut actor, _owner, view) = coordinator(journal.clone());
            let calls = Arc::new(Mutex::new(Vec::new()));
            let (prepared, server) = prepared_review(journal.clone(), calls.clone());
            let run = prepared.run;
            start(&mut actor, &journal, prepared);
            pump(&mut actor, |actor| actor.flight.is_some());
            assert_eq!(server.join().unwrap(), 1);
            let running = actor.record.unwrap();
            let (AgentWorkJournalRequest::CompareAndSet(mutation), ack) =
                lock(&journal.pending).pop_front().unwrap()
            else {
                panic!()
            };
            let review = mutation.next();
            assert_eq!(review.disposition(), AgentWorkDisposition::NeedsApproval);
            assert_eq!(review.debt(), AgentWorkDebt::NONE);
            assert_eq!(*lock(&calls), [1, 2, 3, 4, 5, 6]);
            let active = actor.active.as_ref().unwrap();
            assert_eq!(active.lifecycle_clean, Some(true));
            let Some(AgentWorkOutcome::ClosedUnsuccessfully(closed)) = active.outcome.as_ref()
            else {
                panic!()
            };
            let native = active.native.as_ref().unwrap();
            assert_eq!(closed.policy_settlement().closure().effects(), 0);
            assert_eq!(
                closed.human_review().unwrap().reason(),
                AgentNeedsHumanReason::ScopeExpansion
            );
            for offset in [32, 48, 64] {
                let mut foreign = *running.as_bytes();
                foreign[offset] ^= 1;
                assert!(AgentWorkJournalMutation::needs_approval_closed(
                    AgentWorkRecord::decode(foreign).unwrap(),
                    closed.policy_settlement(),
                    native,
                    closed.human_review().unwrap()
                )
                .is_err());
            }
            assert!(AgentWorkJournalMutation::completed(
                running,
                closed.policy_settlement(),
                native
            )
            .is_err());
            let (mut successor, _next_owner, _) = coordinator(journal.clone());
            successor.predecessor = Some(WorkPredecessor {
                projection: actor.projection.clone(),
                record: review,
            });
            assert!(!successor.accepts_predecessor(Some(&actor)));
            if lose_ack {
                actor.flight.as_mut().unwrap().deadline = Instant::now();
                actor.poll();
                command(&mut actor, WorkControl::Reconcile);
                let (AgentWorkJournalRequest::CompareAndSet(repeated), fresh_ack) =
                    lock(&journal.pending).pop_front().unwrap()
                else {
                    panic!()
                };
                assert_eq!(repeated.expected(), mutation.expected());
                assert_eq!(repeated.next(), review);
                ack(Ok(AgentWorkJournalReply::Record(Some(review))));
                actor.poll();
                assert_eq!(
                    view.snapshot().phase,
                    AgentWorkApplicationPhase::PersistenceUncertain
                );
                assert!(!successor.accepts_predecessor(Some(&actor)));
                fresh_ack(Ok(AgentWorkJournalReply::Record(Some(review))));
            } else {
                ack(Ok(AgentWorkJournalReply::Record(Some(review))));
            }
            actor.poll();
            assert_eq!(
                view.snapshot().phase,
                AgentWorkApplicationPhase::NeedsReview
            );
            while view.take_event().is_some() {}
            assert!(
                !successor.accepts_predecessor(Some(&actor)),
                "clean drain is not review acceptance"
            );
            command(
                &mut actor,
                WorkControl::Review {
                    record: review,
                    decision,
                },
            );
            let (AgentWorkJournalRequest::CompareAndSet(decision_mutation), old_ack) =
                lock(&journal.pending).pop_front().unwrap()
            else {
                panic!()
            };
            let terminal = decision_mutation.next();
            assert_eq!(terminal.debt(), AgentWorkDebt::NONE);
            successor.predecessor.as_mut().unwrap().record = terminal;
            assert!(
                !successor.accepts_predecessor(Some(&actor)),
                "review ACK still outstanding"
            );
            command(
                &mut actor,
                WorkControl::Stop {
                    run,
                    reason: AgentRuntimeStopReason::HumanTakeover,
                },
            );
            assert!(
                !actor.stopping,
                "exact decision CAS already owns the terminal"
            );
            if lose_ack {
                actor.flight.as_mut().unwrap().deadline = Instant::now();
                actor.poll();
                assert_eq!(
                    view.snapshot().phase,
                    AgentWorkApplicationPhase::PersistenceUncertain
                );
                command(&mut actor, WorkControl::Reconcile);
                let (AgentWorkJournalRequest::CompareAndSet(repeated), fresh_ack) =
                    lock(&journal.pending).pop_front().unwrap()
                else {
                    panic!()
                };
                assert_eq!(repeated.expected(), decision_mutation.expected());
                assert_eq!(repeated.next(), terminal);
                old_ack(Ok(AgentWorkJournalReply::Record(Some(terminal))));
                actor.poll();
                assert!(
                    !successor.accepts_predecessor(Some(&actor)),
                    "old ACK cannot settle a replacement slot"
                );
                fresh_ack(Ok(AgentWorkJournalReply::Record(Some(terminal))));
            } else {
                old_ack(Ok(AgentWorkJournalReply::Record(Some(terminal))));
            }
            actor.poll();
            assert_eq!(view.snapshot().phase, AgentWorkApplicationPhase::Reviewed);
            assert_eq!(view.snapshot().last_review, Some(Ok(terminal)));
            assert!(successor.accepts_predecessor(Some(&actor)));
            command(
                &mut actor,
                WorkControl::Review {
                    record: review,
                    decision,
                },
            );
            assert_eq!(
                view.snapshot().last_review,
                Some(Err(AgentWorkJournalError::Conflict))
            );
            command(
                &mut actor,
                WorkControl::Review {
                    record: terminal,
                    decision,
                },
            );
            assert_eq!(
                view.snapshot().last_review,
                Some(Err(AgentWorkJournalError::Transition))
            );
            assert!(lock(&journal.pending).is_empty());
            let original = actor.active.as_mut().unwrap().native.take();
            assert!(!successor.accepts_predecessor(Some(&actor)));
            actor.active.as_mut().unwrap().native = original;
            actor.active.as_mut().unwrap().lifecycle_clean = Some(false);
            assert!(!successor.accepts_predecessor(Some(&actor)));
            actor.active.as_mut().unwrap().lifecycle_clean = Some(true);
            assert!(successor.accepts_predecessor(Some(&actor)));
            assert_eq!(
                *lock(&calls),
                [1, 2, 3, 4, 5, 6],
                "review never replays an action or native port"
            );
            assert!(actor.shutdown_until(Instant::now() + Duration::from_secs(1)));
        }
    }
}

#[test]
fn outstanding_original_audit_cannot_be_cleared_by_review_or_decoded_terminal_facts() {
    let _serial = lock(&SERIAL);
    let journal = Arc::new(Journal::default());
    journal.lose_audit.store(true, Ordering::Release);
    let (mut actor, _owner, view) = coordinator(journal.clone());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (prepared, server) = prepared_review(journal.clone(), calls.clone());
    start(&mut actor, &journal, prepared);
    pump(&mut actor, |actor| actor.flight.is_some());
    assert_eq!(server.join().unwrap(), 1);
    journal.commit();
    actor.poll();
    let record = actor.record.unwrap();
    assert_eq!(record.disposition(), AgentWorkDisposition::NeedsApproval);
    assert_eq!(record.debt(), AgentWorkDebt::UNKNOWN);
    assert!(matches!(
        actor.active.as_ref().unwrap().outcome,
        Some(AgentWorkOutcome::Recovery(_))
    ));
    command(
        &mut actor,
        WorkControl::Review {
            record,
            decision: AgentWorkReviewDecision::AcceptFreshAdmission,
        },
    );
    journal.commit();
    actor.poll();
    let terminal = actor.record.unwrap();
    assert_eq!(terminal.debt(), AgentWorkDebt::UNKNOWN);
    assert_eq!(view.snapshot().phase, AgentWorkApplicationPhase::Recovery);
    while view.take_event().is_some() {}
    let (mut successor, _next_owner, _) = coordinator(journal);
    successor.predecessor = Some(WorkPredecessor {
        projection: actor.projection.clone(),
        record: terminal,
    });
    assert!(!successor.accepts_predecessor(Some(&actor)));
    // Decoder recognizes historical zero-debt facts; it is not a replacement
    // for this process's missing original audit/native/lifecycle owner join.
    let mut bytes = *terminal.as_bytes();
    bytes[2] = AgentWorkDebt::NONE.bits();
    let historical = AgentWorkRecord::decode(bytes).unwrap();
    actor.record = Some(historical);
    successor.predecessor.as_mut().unwrap().record = historical;
    lock(&actor.projection).snapshot.phase = AgentWorkApplicationPhase::Reviewed;
    assert!(!successor.accepts_predecessor(Some(&actor)));
    assert_eq!(*lock(&calls), [1, 2, 3, 4, 5, 6]);
}

#[test]
fn cancellation_before_or_after_review_classification_ack_closes_without_replay() {
    let _serial = lock(&SERIAL);
    for before_ack in [false, true] {
        let journal = Arc::new(Journal::default());
        let (mut actor, _owner, view) = coordinator(journal.clone());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (prepared, server) = prepared_review(journal.clone(), calls.clone());
        let run = prepared.run;
        start(&mut actor, &journal, prepared);
        pump(&mut actor, |actor| actor.flight.is_some());
        assert_eq!(server.join().unwrap(), 1);
        if !before_ack {
            journal.commit();
            actor.poll();
        }
        command(
            &mut actor,
            WorkControl::Stop {
                run,
                reason: AgentRuntimeStopReason::Cancelled,
            },
        );
        assert!(actor.stopping);
        if before_ack {
            journal.commit();
            actor.poll();
        }
        let (AgentWorkJournalRequest::CompareAndSet(mutation), ack) =
            lock(&journal.pending).pop_front().unwrap()
        else {
            panic!()
        };
        assert_eq!(
            mutation.next().disposition(),
            AgentWorkDisposition::FailedClosed
        );
        assert_eq!(mutation.next().debt(), AgentWorkDebt::NONE);
        ack(Ok(AgentWorkJournalReply::Record(Some(mutation.next()))));
        actor.poll();
        assert_eq!(view.snapshot().phase, AgentWorkApplicationPhase::Reviewed);
        assert_eq!(*lock(&calls), [1, 2, 3, 4, 5, 6]);
        assert!(actor.shutdown_until(Instant::now() + Duration::from_secs(1)));
    }
}

#[test]
fn shutdown_reconciles_both_sides_of_the_review_classification_ack() {
    let _serial = lock(&SERIAL);
    for before_ack in [false, true] {
        let journal = Arc::new(Journal::default());
        let (mut actor, _owner, view) = coordinator(journal.clone());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (prepared, server) = prepared_review(journal.clone(), calls.clone());
        start(&mut actor, &journal, prepared);
        pump(&mut actor, |actor| actor.flight.is_some());
        assert_eq!(server.join().unwrap(), 1);
        if !before_ack {
            journal.commit();
            actor.poll();
        }
        let writer = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut acknowledged = 0;
            loop {
                if let Some((AgentWorkJournalRequest::CompareAndSet(mutation), ack)) =
                    lock(&journal.pending).pop_front()
                {
                    acknowledged += 1;
                    let record = mutation.next();
                    ack(Ok(AgentWorkJournalReply::Record(Some(record))));
                    if record.disposition() == AgentWorkDisposition::FailedClosed {
                        break;
                    }
                }
                assert!(Instant::now() < deadline, "bounded shutdown fixture writer");
                std::thread::sleep(Duration::from_millis(1));
            }
            acknowledged
        });
        assert!(actor.shutdown_until(Instant::now() + Duration::from_secs(3)));
        assert_eq!(writer.join().unwrap(), if before_ack { 2 } else { 1 });
        assert_eq!(view.snapshot().phase, AgentWorkApplicationPhase::Reviewed);
        assert_eq!(
            actor.record.unwrap().disposition(),
            AgentWorkDisposition::FailedClosed
        );
        assert_eq!(actor.record.unwrap().debt(), AgentWorkDebt::NONE);
        assert_eq!(*lock(&calls), [1, 2, 3, 4, 5, 6]);
    }
}
