use super::super::tests::{leased, port, setup, tick};
use super::*;
use zephium_agentic::*;

/// Real policy admission and semantic delivery, with no provider/network/native
/// view. Native callback schedules below consume only this original request.
pub(crate) fn request(
    rows: &mut WorkBrowserResources,
    guard: &Arc<WorkResourceGuard>,
    lease: &WorkBrowserExecutionLease,
) -> (
    WorkBrowserActionRequest,
    AgentRunPolicy,
    SemanticActionExecutionCoordinator,
    SemanticActionExecutionReservation,
) {
    let request = rows.observe_initial(lease, tick(2)).unwrap();
    guard.admit_read(&request, tick(2)).unwrap();
    let (invocation, owner) = request.into_parts();
    let frame = invocation.frame().clone();
    let bytes = serde_json::to_vec(&serde_json::json!({"v": SEMANTIC_WIRE_VERSION,"i": invocation.invocation().get(),"g": invocation.snapshot_generation().get(),"c":"complete","n":[{"k":1,"r":"document"},{"k":2,"p":0,"r":"textbox","n":"Draft","o":2,"v":{"k":"text","value":""},"b":{"x":10,"y":20,"w":100,"h":30}}]})).unwrap();
    let snapshot = invocation.decode_result(&bytes).unwrap();
    guard.read_terminal_begin();
    let event = rows
        .settle_observation(owner.settle(Ok(snapshot)), tick(2))
        .unwrap();
    guard.read_terminal_end();
    let WorkBrowserObservationEvent::Snapshot(snapshot) = event else {
        panic!("snapshot");
    };
    let context = frame.context();
    let observation = SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(1).unwrap(),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        ),
        *snapshot,
    )
    .unwrap()
    .finish()
    .unwrap();
    let origin = frame.origin().clone();
    let effects =
        AgentEffectScope::try_new(&[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite])
            .unwrap();
    let budget = AgentRunBudget::try_new(8, 100_000, 100_000, 1).unwrap();
    let profile = guard.resource().identity().profile();
    let node = AgentPlanNodeId::generate();
    let plan_lease = AgentPlanLeaseId::generate();
    let authority = AgentPlanNodeAuthority::try_new(
        vec![profile],
        vec![AgentAccountScope::Anonymous],
        vec![origin.clone()],
        SemanticSensitivity::Public,
        effects,
    )
    .unwrap();
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
        lease.run(),
        AgentRunScope::try_new(
            vec![profile],
            vec![AgentAccountScope::Anonymous],
            vec![origin],
            SemanticSensitivity::Public,
            effects,
            vec![],
        )
        .unwrap(),
        budget,
        tick(0),
        tick(900_000),
        vec![AgentPlanNodeScope::new(
            node,
            authority,
            budget,
            tick(899_999),
        )],
    )
    .unwrap();
    let mut policy =
        AgentRunPolicy::try_new(manifest, vec![AgentPlanLeaseBinding::new(plan_lease, node)])
            .unwrap();
    let account = AgentContextAccountBinding::new(
        AgentAccountAttestationId::generate(),
        context,
        AgentAccountScope::Anonymous,
        tick(2),
    );
    let payload = encode_semantic_observation(
        &observation,
        SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
    )
    .unwrap()
    .admit_conservative_utf8(&SemanticTokenizerRevision::try_new("native-test-v1".into()).unwrap())
    .unwrap();
    let tokens = payload.token_measurement().tokens();
    let admission = policy
        .prepare_observation_input(
            AgentModelCallRequest::new(
                AgentModelCallId::new(1).unwrap(),
                plan_lease,
                account,
                AgentModelCallBudget::try_new(0, 0, 0).unwrap(),
                tick(3),
            ),
            &observation,
            &payload,
        )
        .unwrap();
    let baseline = payload
        .settle_delivery(SemanticModelDeliverySettlement::Committed)
        .unwrap();
    let call = policy
        .commit_observation_input(admission, &baseline)
        .unwrap();
    policy
        .settle_model_call(
            call,
            AgentModelCallSettlement::Completed,
            tokens.into(),
            0,
            0,
        )
        .unwrap();
    let automation = rows.automation_state(lease, tick(3)).unwrap();
    let proposal = SemanticActionProposal::try_new(
        SemanticActionIntent::Fill {
            target: SemanticReferenceId::new(2).unwrap(),
            value: SemanticActionText::try_new("private draft value".to_owned()).unwrap(),
        },
        SemanticEffectClass::LocalWrite,
        SemanticWaitCondition::Immediate,
        SemanticVerification::TargetValueMatchesInput,
        SemanticSettleBudget::try_new(5_000).unwrap(),
    )
    .unwrap();
    let batch = SemanticActionBatch::bind(
        SemanticActionBatchId::new(1).unwrap(),
        &observation,
        &[frame.clone()],
        vec![proposal],
    )
    .unwrap();
    let action = batch.actions()[0]
        .prepare(&observation.frames()[0])
        .unwrap();
    // Trusted test fixture contract, never derived from a page label in production.
    let assessment = AgentEffectAssessment::new(
        &action,
        frame.origin().clone(),
        SemanticEffectClass::LocalWrite,
    );
    let AgentEffectAuthorization::Permit(permit) = policy
        .authorize_semantic_effect(
            AgentEffectRequest::new(
                AgentEffectId::new(1).unwrap(),
                plan_lease,
                account,
                automation,
                tick(3),
            ),
            &action,
            &assessment,
        )
        .unwrap()
    else {
        panic!("fixture effect authorized");
    };
    let active = policy
        .dispatch_semantic_effect(
            permit,
            &action,
            AgentEffectDispatchRequest::new(
                SemanticActionAttemptId::new(1).unwrap(),
                account,
                automation,
                tick(3),
            ),
        )
        .unwrap();
    let mut coordinator = SemanticActionExecutionCoordinator::new();
    let (reservation, native) = coordinator
        .begin(
            active,
            &action,
            SemanticActionExecutionInstant::from_millis(3),
        )
        .unwrap();
    (
        rows.prepare_action(lease, native, tick(3)).unwrap(),
        policy,
        coordinator,
        reservation,
    )
}

impl WorkActionTask {
    pub(crate) fn native_for_test() -> SemanticActionNativeRequest {
        Self::retained_for_test().into_parts().0
    }
    pub(crate) fn retained_for_test() -> WorkBrowserActionRequest {
        let (mut rows, _, guard) = setup();
        let lease = leased(&mut rows, &guard);
        request(&mut rows, &guard, &lease).0
    }
}

#[test]
fn exact_action_debt_survives_callback_entry_and_drains_only_after_return() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (mut request, _, coordinator, reservation) = request(&mut rows, &guard, &lease);
    let ticket = Arc::new(Mutex::new(request.take_delivery_ticket().unwrap()));
    let during_ticket = ticket.clone();
    let attempt = request.action().attempt();
    guard.admit_action(&request, tick(4)).unwrap();
    assert!(!guard.admits(&lease, tick(4)));
    let revoke = rows.revoke(&lease).unwrap();
    guard.admit_lifecycle(&revoke, tick(4)).unwrap();
    assert!(!guard.action_current(&lease, attempt, tick(4)));
    let output = Arc::new(Mutex::new(None));
    let returned = output.clone();
    let during = guard.clone();
    let during_lease = lease.clone();
    *guard.notification_dispatch.lock().unwrap() = Some(Arc::new(|_| true));
    let mut task = WorkActionTask {
        request: Some(request),
        owner: None,
        delivery: None,
        attempt,
        completion: Some(Box::new(move |terminal| {
            assert_eq!(during.state.lock().unwrap().action, Some(attempt));
            assert_eq!(during.state.lock().unwrap().callbacks, 1);
            assert!(!during.lease_drained(&during_lease));
            assert!(!during.callbacks_drained());
            assert_eq!(
                during_ticket.lock().unwrap().try_take_returned().unwrap(),
                None
            );
            *returned.lock().unwrap() = Some(terminal);
        })),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
    };
    let native = task.take_native().unwrap();
    task.complete(native.fail(
        SemanticActionNativeFailure::Cancelled,
        SemanticActionExecutionInstant::from_millis(5),
    ));
    assert!(guard.lease_drained(&lease));
    assert_eq!(admission.pending(), Some(0));
    assert_eq!(
        ticket.lock().unwrap().try_take_returned().unwrap(),
        Some(true)
    );
    let event = rows
        .settle_action(output.lock().unwrap().take().unwrap(), tick(5))
        .unwrap();
    assert!(!event.is_current());
    assert!(coordinator.accepts_settlement(&reservation, &event.into_terminal()));
}

#[test]
fn synchronous_action_rejection_keeps_original_recipe_and_no_callback() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (request, _, _, _) = request(&mut rows, &guard, &lease);
    // Advance the actual admission clock to the fixture's 3 ms policy instant.
    let now = work_browser_monotonic_now().unwrap().millis();
    if now < 3 {
        std::thread::sleep(std::time::Duration::from_millis(3 - now));
    }
    let native = port(admission.clone(), Arc::new(|_| false));
    let WorkBrowserActionDispatch::Rejected { request, failure } =
        native.schedule_work_action(request, Box::new(|_| panic!("unadmitted callback")))
    else {
        panic!("expected rejection");
    };
    assert_eq!(failure, ContextPortFailure::Shutdown);
    let recipe = rows.action_dispatch_refused(*request).unwrap();
    assert_eq!(recipe.fill_text().unwrap().as_str(), "private draft value");
    assert!(guard.state.lock().unwrap().action.is_none());
    assert_eq!(admission.pending(), Some(0));
}

#[test]
fn action_admission_rejects_foreign_stale_expired_busy_and_revoked_owners() {
    for fault in 0..9 {
        let (mut rows, _, guard) = setup();
        let lease = leased(&mut rows, &guard);
        let (request, _, _, _) = request(&mut rows, &guard, &lease);
        match fault {
            0 => guard.state.lock().unwrap().reads = 1,
            1 => guard.state.lock().unwrap().callbacks = 1,
            2 => guard.state.lock().unwrap().observed = None,
            3 => guard.state.lock().unwrap().document_epoch = 2,
            4 => guard.state.lock().unwrap().phase = Phase::Revoking,
            5 => {
                guard.state.lock().unwrap().action = Some(SemanticActionAttemptId::new(9).unwrap())
            }
            6 => guard.state.lock().unwrap().uncertain = true,
            _ => {}
        }
        let now = match fault {
            7 => tick(request.action().deadline().millis()),
            8 => tick(2),
            _ => tick(4),
        };
        assert!(guard.admit_action(&request, now).is_err(), "fault {fault}");
    }
}

#[test]
fn losing_dispatched_recipe_keeps_action_debt_and_quarantines() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (request, _, _, _) = request(&mut rows, &guard, &lease);
    let attempt = request.action().attempt();
    guard.admit_action(&request, tick(4)).unwrap();
    let mut task = WorkActionTask {
        request: Some(request),
        owner: None,
        delivery: None,
        attempt,
        completion: Some(Box::new(|_| panic!("cannot invent a terminal"))),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
    };
    let _original = task.take_native().unwrap();
    drop(task);
    assert!(!guard.is_healthy());
    assert_eq!(guard.state.lock().unwrap().action, Some(attempt));
    assert!(!guard.callbacks_drained());
}

#[test]
fn quarantine_keeps_only_exact_action_drain_and_callback_return_debt() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (mut request, _, _, _) = request(&mut rows, &guard, &lease);
    let mut ticket = request.take_delivery_ticket().unwrap();
    let attempt = request.action().attempt();
    guard.admit_action(&request, tick(4)).unwrap();
    guard.fail();
    assert!(!guard.action_current(&lease, attempt, tick(4)));
    assert!(!guard.admits(&lease, tick(4)));
    assert!(guard.action_drain_current(&lease, attempt, tick(4)));
    assert!(!guard.action_drain_current(
        &lease,
        SemanticActionAttemptId::new(attempt.get() + 1).unwrap(),
        tick(4)
    ));
    assert!(!guard.action_drain_current(&lease, attempt, lease.deadline()));
    let during = guard.clone();
    *guard.notification_dispatch.lock().unwrap() = Some(Arc::new(|_| true));
    let mut task = WorkActionTask {
        request: Some(request),
        owner: None,
        delivery: None,
        attempt,
        completion: Some(Box::new(move |_| {
            assert!(!during.callbacks_drained());
            assert_eq!(during.state.lock().unwrap().action, Some(attempt));
        })),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
    };
    let native = task.take_native().unwrap();
    task.complete(native.fail(
        SemanticActionNativeFailure::AppliedUnverified,
        SemanticActionExecutionInstant::from_millis(5),
    ));
    assert_eq!(ticket.try_take_returned().unwrap(), Some(true));
    assert!(!guard.action_drain_current(&lease, attempt, tick(5)));
    assert!(!guard.is_healthy());
    assert!(guard.state.lock().unwrap().action.is_none());
    assert_eq!(guard.state.lock().unwrap().callbacks, 0);
    assert_eq!(admission.pending(), Some(0));
}

#[test]
fn explicit_retirement_stops_quarantined_action_drain_even_when_lifecycle_is_refused() {
    let (mut rows, _admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (request, _, _, _) = request(&mut rows, &guard, &lease);
    let attempt = request.action().attempt();
    guard.admit_action(&request, tick(4)).unwrap();
    guard.fail();
    assert!(guard.action_drain_current(&lease, attempt, tick(4)));
    let revoke = rows.revoke(&lease).unwrap();
    assert!(guard.admit_lifecycle(&revoke, tick(4)).is_err());
    assert!(!guard.action_drain_current(&lease, attempt, tick(4)));
    assert_eq!(guard.state.lock().unwrap().action, Some(attempt));
}

#[test]
fn panicking_action_consumer_never_publishes_successful_return() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (mut request, _, _, _) = request(&mut rows, &guard, &lease);
    let mut ticket = request.take_delivery_ticket().unwrap();
    let attempt = request.action().attempt();
    guard.admit_action(&request, tick(4)).unwrap();
    *guard.notification_dispatch.lock().unwrap() = Some(Arc::new(|_| true));
    let mut task = WorkActionTask {
        request: Some(request),
        owner: None,
        delivery: None,
        attempt,
        completion: Some(Box::new(|_| panic!("consumer unwind"))),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
    };
    let native = task.take_native().unwrap();
    task.complete(native.fail(
        SemanticActionNativeFailure::Cancelled,
        SemanticActionExecutionInstant::from_millis(5),
    ));
    assert_eq!(ticket.try_take_returned().unwrap(), Some(false));
    assert!(!guard.is_healthy());
    assert!(guard.state.lock().unwrap().action.is_none());
    assert_eq!(guard.state.lock().unwrap().callbacks, 0);
    assert_eq!(admission.pending(), Some(0));
}
