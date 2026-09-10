use super::super::tests::{leased, port, setup, tick};
use super::*;
use zephium_agentic::*;

/// Real policy admission and semantic delivery, with no provider/network/native
/// view. Native callback schedules below consume only this original request.
fn request(
    rows: &mut WorkBrowserResources,
    guard: &Arc<WorkResourceGuard>,
    lease: &WorkBrowserExecutionLease,
) -> (
    WorkBrowserNavigationRequest,
    AgentRunPolicy,
    AgentActiveNavigation,
) {
    let request = rows.observe_initial(lease, tick(2)).unwrap();
    guard.admit_read(&request, tick(2)).unwrap();
    let (invocation, owner) = request.into_parts();
    let frame = invocation.frame().clone();
    let bytes = serde_json::to_vec(&serde_json::json!({"v": SEMANTIC_WIRE_VERSION,"i": invocation.invocation().get(),"g": invocation.snapshot_generation().get(),"c":"complete","n":[{"k":1,"r":"document","o":16}]})).unwrap();
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
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).unwrap();
    let budget = AgentRunBudget::try_new(8, 100_000, 100_000, 1).unwrap();
    let profile = guard.resource().identity().profile();
    let node = AgentPlanNodeId::generate();
    let plan_lease = AgentPlanLeaseId::generate();
    let target = ContextNavigationTarget::parse("https://example.test/next").unwrap();
    let route =
        AgentNavigationRoute::try_new(guard.document().unwrap().clone(), vec![target.clone()])
            .unwrap();
    let authority = AgentPlanNodeAuthority::try_new(
        vec![profile],
        vec![AgentAccountScope::Anonymous],
        vec![origin.clone()],
        SemanticSensitivity::Public,
        effects,
    )
    .unwrap()
    .with_navigation_route(route)
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
    let permit = policy
        .authorize_navigation(
            AgentNavigationAuthorizationRequest::new(plan_lease, account, automation, tick(3)),
            &observation,
            &baseline,
            &target,
        )
        .unwrap();
    let preparation = rows.prepare_navigation(lease, context, tick(3)).unwrap();
    let active = policy
        .dispatch_navigation(permit, preparation.operation(), tick(3))
        .unwrap();
    (preparation.bind(&active).unwrap(), policy, active)
}

#[test]
fn exact_policy_request_retains_native_debt_through_callback_return() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (request, mut policy, active) = request(&mut rows, &guard, &lease);
    let operation = request.navigation().operation();
    let target = request.navigation().target().clone();
    guard.admit_navigation(&request, tick(4)).unwrap();
    let revoke = rows.revoke(&lease).unwrap();
    guard.admit_lifecycle(&revoke, tick(4)).unwrap();
    assert!(!guard.lease_drained(&lease));
    assert!(!guard.callbacks_drained());
    let returned = Arc::new(Mutex::new(None));
    let output = returned.clone();
    let callback_guard = guard.clone();
    let callback_lease = lease.clone();
    let callback_admission = admission.clone();
    // No GUI/main-queue operation is required to attest this exact delivery.
    *guard.notification_dispatch.lock().unwrap() = Some(Arc::new(|task| {
        drop(task);
        true
    }));
    let task = WorkNavigationTask {
        request: Some(request),
        completion: Some(Box::new(move |terminal| {
            assert_eq!(
                callback_guard.state.lock().unwrap().navigation,
                Some(operation)
            );
            assert_eq!(callback_guard.state.lock().unwrap().callbacks, 1);
            assert!(!callback_guard.lease_drained(&callback_lease));
            assert!(!callback_guard.callbacks_drained());
            assert!(callback_admission.pending().unwrap() > 0);
            *output.lock().unwrap() = Some(terminal);
        })),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
    };
    task.complete(Ok(target));
    assert!(guard.lease_drained(&lease));
    assert_eq!(admission.pending(), Some(0));
    assert_eq!(guard.state.lock().unwrap().document_epoch, 2);
    let event = rows
        .settle_navigation(returned.lock().unwrap().take().unwrap(), tick(5))
        .unwrap();
    assert!(!event.is_current());
    let receipt = policy
        .settle_navigation(&active, event.terminal(), tick(5))
        .unwrap();
    assert_eq!(receipt.settlement(), AgentNavigationSettlement::Committed);
    assert_eq!(policy.pending_navigations(), 0);
    assert_eq!(policy.accounting().consumed_operations(), 2);
    assert!(policy.accounting().reserved_operations() == 0);
}

#[test]
fn revocation_closes_undispatched_navigation_but_preserves_only_completion_debt() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (request, _, _) = request(&mut rows, &guard, &lease);
    let operation = request.navigation().operation();
    guard.admit_navigation(&request, tick(4)).unwrap();
    assert!(guard.navigation_dispatch_current(&lease, operation, tick(4)));

    let revoke = rows.revoke(&lease).unwrap();
    guard.admit_lifecycle(&revoke, tick(4)).unwrap();
    assert!(
        !guard.navigation_dispatch_current(&lease, operation, tick(4)),
        "a PARK acknowledgement arriving after revoke owns no native dispatch"
    );
    assert!(
        guard.navigation_completion_current(&lease, operation, tick(4)),
        "only an operation already dispatched before revoke may drain"
    );

    let callbacks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let callback_count = callbacks.clone();
    *guard.notification_dispatch.lock().unwrap() = Some(Arc::new(|task| {
        drop(task);
        true
    }));
    let task = WorkNavigationTask {
        request: Some(request),
        completion: Some(Box::new(move |_| {
            callback_count.fetch_add(1, Ordering::SeqCst);
        })),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
    };
    task.complete(Err(ContextPortFailure::Stale));
    assert_eq!(callbacks.load(Ordering::SeqCst), 1);
    assert!(guard.state.lock().unwrap().navigation.is_none());
    assert_eq!(admission.pending(), Some(0));
}

#[test]
fn synchronous_navigation_refusal_returns_original_without_callback() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (request, mut policy, active) = request(&mut rows, &guard, &lease);
    let operation = request.navigation().operation();
    let native = port(
        admission.clone(),
        Arc::new(|task| {
            drop(task);
            false
        }),
    );
    let result = native.schedule_work_navigation(
        request,
        Box::new(|_| panic!("non-dispatch must not callback")),
    );
    let WorkBrowserNavigationDispatch::Rejected { request, failure } = result else {
        panic!("refused");
    };
    assert_eq!(request.navigation().operation(), operation);
    assert_eq!(failure, ContextPortFailure::Shutdown);
    rows.navigation_dispatch_refused(*request).unwrap();
    policy
        .refuse_navigation_dispatch(&active, failure, tick(5))
        .unwrap();
    assert!(guard.state.lock().unwrap().navigation.is_none());
    assert_eq!(admission.pending(), Some(0));
    assert!(rows.read_binding(&lease, tick(5)).is_err());
}

#[test]
fn native_navigation_admission_checks_original_lease_source_deadline_and_physical_slots() {
    for fault in 0..6 {
        let (mut rows, admission, guard) = setup();
        let lease = leased(&mut rows, &guard);
        let (request, _, _) = request(&mut rows, &guard, &lease);
        match fault {
            0 => guard.state.lock().unwrap().reads = 1,
            1 => guard.state.lock().unwrap().callbacks = 1,
            2 => guard.state.lock().unwrap().observed = None,
            3 => guard.state.lock().unwrap().document_epoch = 2,
            4 => guard.state.lock().unwrap().phase = Phase::Revoking,
            _ => {}
        }
        let result = guard.admit_navigation(
            &request,
            if fault == 5 {
                lease.deadline()
            } else {
                tick(4)
            },
        );
        assert!(result.is_err(), "fault {fault}");
        assert!(guard.state.lock().unwrap().navigation.is_none());
        assert_eq!(admission.pending(), Some(0));
        rows.navigation_dispatch_refused(request).unwrap();
    }
}

#[test]
fn lost_failed_or_panicking_navigation_keeps_exact_terminal_debt_and_quarantines() {
    for fault in 0..4 {
        let (mut rows, admission, guard) = setup();
        let lease = leased(&mut rows, &guard);
        let (request, mut policy, active) = request(&mut rows, &guard, &lease);
        let target = request.navigation().target().clone();
        guard.admit_navigation(&request, tick(4)).unwrap();
        *guard.notification_dispatch.lock().unwrap() = Some(Arc::new(|task| {
            drop(task);
            true
        }));
        let returned = Arc::new(Mutex::new(None));
        let output = returned.clone();
        let called = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let callback_called = called.clone();
        let task = WorkNavigationTask {
            request: Some(request),
            completion: Some(Box::new(move |terminal| {
                callback_called.fetch_add(1, Ordering::SeqCst);
                // A consumer may enqueue then unwind. Physical native debt
                // drains, but its sticky resource failure cannot be undone by
                // accepting the already-enqueued successful core terminal.
                *output.lock().unwrap() = Some(terminal);
                assert_ne!(fault, 3, "consumer unwind");
            })),
            guard: guard.clone(),
            permit: admission.reserve().unwrap(),
        };
        match fault {
            0 => drop(task),
            1 => task.complete(Err(ContextPortFailure::TimedOut)),
            2 => task.complete(Ok(ContextNavigationTarget::parse(
                "https://example.test/wrong",
            )
            .unwrap())),
            _ => task.complete(Ok(target)),
        }
        assert_eq!(called.load(Ordering::SeqCst), 1);
        assert_eq!(admission.pending(), Some(0));
        assert!(!guard.is_healthy());
        assert!(guard.state.lock().unwrap().navigation.is_none());
        assert_eq!(guard.state.lock().unwrap().callbacks, 0);
        assert_eq!(
            guard.state.lock().unwrap().document_epoch,
            if fault == 3 { 2 } else { 1 }
        );
        assert!(rows.read_binding(&lease, tick(5)).is_err());
        let event = rows
            .settle_navigation(returned.lock().unwrap().take().unwrap(), tick(5))
            .unwrap();
        let _ = policy
            .settle_navigation(&active, event.terminal(), tick(5))
            .unwrap();
        assert_eq!(policy.pending_navigations(), 0);
        assert!(!guard.admits(&lease, tick(5)));
    }
}
