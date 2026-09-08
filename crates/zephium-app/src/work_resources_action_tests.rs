//! Full retained controller loop with real policy, isolated native/model fixtures.
use super::*;

pub(super) fn read_result(
    native: &Native,
    request: WorkBrowserObservationRequest,
    callback: WorkBrowserObservationCompletionCallback,
) {
    let value = if native.form_applied.load(Ordering::Acquire) {
        "fixture value"
    } else {
        ""
    };
    let (invocation, owner) = request.into_parts();
    let wire = serde_json::json!({"v":1,"i":invocation.invocation().get(),"g":invocation.snapshot_generation().get(),"c":"complete","n":[
        {"k":1,"r":"paragraph","t":"Fixture result"},
        {"k":2,"r":"textbox","n":"Draft","s":64,"o":2,"v":{"k":"text","value":value},"b":{"x":10,"y":20,"w":100,"h":30}}
    ]}).to_string();
    callback(owner.settle(Ok(invocation.decode_result(wire.as_bytes()).unwrap())));
}
pub(super) fn action_result(
    native: &Native,
    request: WorkBrowserActionRequest,
    callback: WorkBrowserActionCompletionCallback,
) {
    let (request, mut owner) = request.into_parts();
    let mut delivery = owner.take_delivery_completion().unwrap();
    let notification = delivery.take_notification().unwrap();
    let geometry = request.expected_geometry();
    native.form_applied.store(true, Ordering::Release);
    let terminal = request.complete(
        SemanticActionExecutionBackend::PageWorldCompatibilityFill,
        SemanticActionNativeReadiness::ExactConnectedWritableFormTarget,
        SemanticActionNativeViewport::try_new(800, 600).unwrap(),
        geometry,
        SemanticActionExecutionInstant::from_millis(2),
        SemanticActionExecutionInstant::from_millis(2),
    );
    callback(owner.settle(terminal).unwrap());
    if let Some(check) = native.before_action_return.lock().unwrap().take() {
        check();
    }
    assert!(delivery.publish_returned());
    assert!(notification.notify());
}
pub(super) fn act_stream(value: &str) -> String {
    let arguments = serde_json::json!({"actions":[{"kind":"fill","target":"@a2","value":value,"effect":"local_write","wait":{"kind":"immediate"},"verification":{"kind":"target_value_matches_input"},"settle_millis":2000}]}).to_string().replace('"', "\\\"");
    response_stream(1)
        .replace("\"extract\"", "\"act\"")
        .replace(
            r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
            &arguments,
        )
}
fn prepare(
    browser: RetainedBrowser,
    responses: Vec<String>,
    approved: bool,
) -> (
    AgentWorkRetainedController,
    AgentWorkRetainedHandle,
    AgentRuntimeScopedBinding,
    std::thread::JoinHandle<usize>,
) {
    prepare_with_audit(browser, responses, approved, false)
}
fn prepare_with_audit(
    browser: RetainedBrowser,
    responses: Vec<String>,
    approved: bool,
    lose_audit: bool,
) -> (
    AgentWorkRetainedController,
    AgentWorkRetainedHandle,
    AgentRuntimeScopedBinding,
    std::thread::JoinHandle<usize>,
) {
    let binding = browser.binding();
    let identity = binding.frame().context().identity();
    let origin = binding.frame().origin().clone();
    let effects = if approved {
        vec![SemanticEffectClass::Read, SemanticEffectClass::LocalWrite]
    } else {
        vec![SemanticEffectClass::Read]
    };
    let input = input_for_context_effects(
        identity,
        origin.clone(),
        Arc::new(Clock(AtomicU64::new(2))),
        binding.storage(),
        binding.document().clone(),
        AgentRunBudget::try_new(24, 1_000_000, 1_000_000, 1).unwrap(),
        (Instant::now() + Duration::from_secs(30), None),
        WorkBrowserDocumentPolicy::Exact,
        AgentEffectScope::try_new(&effects).unwrap(),
    );
    let task = AgentWorkFormTask::try_new_local_preparation(
        identity,
        origin,
        AgentAccountScope::Anonymous,
        vec![AgentWorkFormPhase::try_new(vec![AgentWorkFormGoal::fill(
            Some("Draft".into()),
            "fixture value".into(),
        )
        .unwrap()])
        .unwrap()],
    )
    .unwrap()
    .with_extraction(vec![SemanticExtractionFieldSchema::try_text(
        "label".into(),
        true,
        64,
    )
    .unwrap()])
    .unwrap();
    let (transport, server) = fixture_provider_responses(responses);
    let (controller, handle, scope) = AgentWorkRetainedController::try_new_for_probe(
        input,
        Box::new(browser),
        transport,
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-not-a-secret".into(),
        )
        .unwrap(),
        Arc::new(Audit(lose_audit)),
        Box::new(task),
    )
    .unwrap();
    (controller, handle, scope, server)
}

#[test]
fn synchronous_rejected_action_closes_exact_failed_receipt_without_retry() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for lose_audit in [false, true] {
        let (owner, native, resource, browser) = setup();
        native.form_actions.store(true, Ordering::Release);
        native.reject_action.store(true, Ordering::Release);
        let (controller, mut result, scope, server) =
            prepare_with_audit(browser, vec![act_stream("fixture value")], true, lose_audit);
        let (_, lifecycle) = start(controller, scope);
        let mut outcome = None;
        let mut events = Vec::new();
        wait_until(|| {
            while let Some(event) = result.take_event() {
                events.push(event.kind());
            }
            outcome = result.take_outcome();
            outcome.is_some()
        });
        let drained = lifecycle.drain_until(Instant::now() + Duration::from_secs(2));
        if lose_audit {
            assert!(matches!(
                outcome,
                Some(AgentWorkRetainedOutcome::Recovery(_))
            ));
            assert!(matches!(drained, AgentRuntimeScopedDrain::Unproven));
        } else {
            let Some(AgentWorkRetainedOutcome::ClosedUnsuccessfully(closed)) = outcome else {
                panic!("proved non-admission must close as failed");
            };
            assert_eq!(
                closed.failure(),
                AgentWorkFailure::Browser(AgentBrowserProviderError::Action(
                    AgentBrowserActionError::Failed(SemanticActionFailure::BackendRefused)
                ))
            );
            let closure = closed.policy_settlement().closure();
            assert_eq!(closure.model_calls(), 1);
            assert_eq!(closure.effects(), 1);
            assert_eq!(closure.actions(), 1);
            assert!(matches!(
                closure.outcome(),
                AgentRunProgressOutcome::Failed(_)
            ));
            assert!(matches!(drained, AgentRuntimeScopedDrain::Drained(_)));
            assert_eq!(
                events
                    .iter()
                    .filter(|kind| **kind == AgentWorkEventKind::Terminal)
                    .count(),
                1
            );
        }
        assert_eq!(
            events
                .iter()
                .filter(|kind| matches!(kind, AgentWorkEventKind::ActionRejected(_)))
                .count(),
            1
        );
        assert!(!events.contains(&AgentWorkEventKind::Verified));
        assert_eq!(native.actions.load(Ordering::Acquire), 1);
        assert_eq!(native.reads.load(Ordering::Acquire), 1);
        assert!(!native.form_applied.load(Ordering::Acquire));
        native.join();
        assert_eq!(server.join().unwrap(), 1);
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(destroy.poll(now()).unwrap().is_some());
        owner.seal_resources().unwrap();
    }
}
fn finish(result: &mut AgentWorkRetainedHandle) -> AgentWorkRetainedOutcome {
    let mut outcome = None;
    wait_until(|| {
        while result.take_event().is_some() {}
        outcome = result.take_outcome();
        outcome.is_some()
    });
    outcome.unwrap()
}
#[test]
fn retained_authorized_form_action_settles_from_fresh_observation_before_extraction() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.form_actions.store(true, Ordering::Release);
    let shared = owner.shared.clone();
    let original = shared.resource(&resource).unwrap();
    *native.before_action_return.lock().unwrap() = Some(Box::new(move || {
        let slot = original
            .slots
            .lock()
            .unwrap()
            .iter()
            .find_map(|slot| match slot {
                OwnedSlot::Action(slot) => Some(slot.clone()),
                _ => None,
            })
            .unwrap();
        assert!(slot
            .lock()
            .unwrap()
            .poll(&shared, &original, now())
            .unwrap()
            .is_none());
        assert_eq!(original.actions.load(Ordering::Acquire), 1);
    }));
    let responses = vec![
        act_stream("fixture value"),
        response_stream(1)
            .replace("resp_1", "resp_2")
            .replace("fc_1", "fc_2")
            .replace("call_1", "call_2"),
        response_stream(2)
            .replace("resp_2", "resp_3")
            .replace("msg_2", "msg_3"),
    ];
    let (controller, mut result, scope, server) = prepare(browser, responses, true);
    let (_, lifecycle) = start(controller, scope);
    let outcome = finish(&mut result);
    let AgentWorkRetainedOutcome::Accepted {
        settlement,
        extraction,
    } = outcome
    else {
        match outcome {
            AgentWorkRetainedOutcome::Recovery(recovery) => {
                panic!("retained form recovery: {:?}", recovery.failure())
            }
            AgentWorkRetainedOutcome::ClosedUnsuccessfully(closed) => {
                panic!("retained form refused: {:?}", closed.failure())
            }
            _ => panic!("retained form did not produce a verified artifact"),
        }
    };
    assert_eq!(settlement.closure().model_calls(), 3);
    assert_eq!(extraction.stats().source_edges(), 1);
    assert_eq!(native.actions.load(Ordering::Acquire), 1);
    assert_eq!(native.reads.load(Ordering::Acquire), 2);
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Drained(_)
    ));
    native.join();
    assert_eq!(server.join().unwrap(), 3);
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(destroy.poll(now()).unwrap().is_some());
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}
#[test]
fn retained_action_requires_both_plan_authority_and_exact_trusted_goal() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for (approved, value, reject, dispatches) in [
        (false, "fixture value", false, 0),
        (true, "other value", false, 0),
        (true, "fixture value", true, 1),
    ] {
        let (owner, native, resource, browser) = setup();
        native.form_actions.store(true, Ordering::Release);
        native.reject_action.store(reject, Ordering::Release);
        let (controller, mut result, scope, server) =
            prepare(browser, vec![act_stream(value)], approved);
        let (_, lifecycle) = start(controller, scope);
        assert!(!matches!(
            finish(&mut result),
            AgentWorkRetainedOutcome::Accepted { .. }
        ));
        assert_eq!(native.actions.load(Ordering::Acquire), dispatches);
        assert!(!native.form_applied.load(Ordering::Acquire));
        assert_eq!(native.reads.load(Ordering::Acquire), 1);
        let _ = lifecycle.drain_until(Instant::now() + Duration::from_secs(2));
        native.join();
        assert_eq!(server.join().unwrap(), 1);
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(destroy.poll(now()).unwrap().is_some());
        owner.seal_resources().unwrap();
    }
}

#[test]
fn retained_stop_preserves_dispatched_action_terminal_without_claiming_verified_effect() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for reason in [
        AgentRuntimeStopReason::Cancelled,
        AgentRuntimeStopReason::HumanTakeover,
    ] {
        let (owner, native, resource, browser) = setup();
        native.form_actions.store(true, Ordering::Release);
        native.hold_action.store(true, Ordering::Release);
        let (controller, mut result, scope, server) =
            prepare(browser, vec![act_stream("fixture value")], true);
        let (handle, lifecycle) = start(controller, scope);
        wait_until(|| native.action.lock().unwrap().is_some());
        handle.stop_and_seal(reason);
        // Native may already have applied before stop reached it. Its original
        // receipt must survive, but revoked work must not take a fresh action
        // or turn that receipt alone into verified success.
        let (request, callback) = native.action.lock().unwrap().take().unwrap();
        action_result(&native, request, callback);
        assert!(matches!(
            finish(&mut result),
            AgentWorkRetainedOutcome::Recovery(_)
        ));
        assert_eq!(native.actions.load(Ordering::Acquire), 1);
        assert_eq!(native.reads.load(Ordering::Acquire), 1);
        let _ = lifecycle.drain_until(Instant::now() + Duration::from_secs(2));
        native.join();
        assert_eq!(server.join().unwrap(), 1);
        // Resource destruction and policy verification are independent debts.
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(destroy.poll(now()).unwrap().is_some());
        owner.seal_resources().unwrap();
    }
}
