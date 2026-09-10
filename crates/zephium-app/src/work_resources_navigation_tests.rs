//! Existing controller/provider/policy loop on the retained application owner.
use super::*;

const FIRST: &str = "https://retained-fixture.invalid/one";
const SECOND: &str = "https://retained-fixture.invalid/two";
fn assert_host_checkpoint(request: &str, current: &str, prior: &[&str]) {
    assert!(request.contains("ZEPHIUM_HOST_LINK_DISCOVERY_V1"));
    let prior = prior
        .iter()
        .map(|url| format!(r#"\"{url}\""#))
        .collect::<Vec<_>>()
        .join(",");
    assert!(request.contains(&format!(
        r#"\"current_document_url\":\"{current}\",\"prior_document_urls\":[{prior}]"#
    )));
    assert!(request.contains(r#"\"next_navigation_target\":null"#));
}
pub(super) fn read_result(
    native: &Native,
    request: WorkBrowserObservationRequest,
    callback: WorkBrowserObservationCompletionCallback,
) {
    let stage = native.navigation_count.load(Ordering::Acquire);
    let expanded = request.invocation().scope() != SemanticRuntimeScopeClass::Initial;
    let (invocation, completion) = request.into_parts();
    let wire = format!(
        r#"{{"v":1,"i":{},"g":{},"c":"complete","n":[{{"k":1,"r":"paragraph","t":"Fixture result document_marker_{stage}"}},{{"k":2,"r":"link","n":"First source","u":"{FIRST}"}},{{"k":3,"r":"link","n":"Second source","u":"{SECOND}"}},{{"k":4,"r":"link","n":"Out of scope source","u":"https://other.invalid/source"}},{{"k":5,"r":"link","n":"Original source","u":"https://retained-fixture.invalid/frozen"}}]}}"#,
        invocation.invocation().get(),
        invocation.snapshot_generation().get()
    );
    let wire = if expanded {
        if invocation.scope() == SemanticRuntimeScopeClass::SurroundingText {
            format!(
                r#"{{"v":1,"i":{},"g":{},"c":"complete","n":[{{"k":1,"r":"paragraph","t":"Fixture result new_scoped_evidence"}}]}}"#,
                invocation.invocation().get(),
                invocation.snapshot_generation().get()
            )
        } else {
            wire.replace(&format!("document_marker_{stage}"), "new_scoped_evidence")
                .replace("\"r\":\"link\"", "\"p\":0,\"r\":\"link\"")
        }
    } else {
        wire
    };
    let wire = if expanded && native.dense_expansion.load(Ordering::Acquire) {
        let mut nodes = vec![serde_json::json!({"k":1,"r":"landmark"})];
        for index in 0..117 {
            let length = if index == 116 { 89 } else { 78 };
            let prefix = format!("Fixture result item {index}: ");
            let text = format!("{prefix}{}", "x".repeat(length - prefix.len()));
            nodes.push(serde_json::json!({"k":index+2,"p":0,"r":"paragraph","t":text}));
        }
        serde_json::json!({"v":1,"i":invocation.invocation().get(),"g":invocation.snapshot_generation().get(),"c":"scope_boundary","n":nodes}).to_string()
    } else if native.region_root.load(Ordering::Acquire) {
        let wire = wire.replacen("\"r\":\"paragraph\"", "\"r\":\"landmark\"", 1);
        if expanded {
            wire.replace("\"c\":\"complete\"", "\"c\":\"node_limit\"")
        } else {
            wire
        }
    } else {
        wire
    };
    callback(completion.settle(Ok(invocation.decode_result(wire.as_bytes()).unwrap())));
}
fn navigation_stream(turn: usize, url: &str) -> String {
    let arguments = format!(r#"{{\"url\":\"{url}\"}}"#);
    response_stream(1)
        .replace("\"extract\"", "\"navigate\"")
        .replace(
            r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
            &arguments,
        )
        .replace("resp_1", &format!("resp_{turn}"))
        .replace("fc_1", &format!("fc_{turn}"))
        .replace("call_1", &format!("call_{turn}"))
}
fn final_streams() -> Vec<String> {
    vec![
        response_stream(1)
            .replace("resp_1", "resp_3")
            .replace("fc_1", "fc_3")
            .replace("call_1", "call_3"),
        response_stream(2)
            .replace("resp_2", "resp_4")
            .replace("msg_2", "msg_4"),
    ]
}
fn inspection_stream(scope: &str) -> String {
    let args = format!("{{\"scope\":{scope}}}").replace('"', "\\\"");
    response_stream(1)
        .replace("\"extract\"", "\"snapshot\"")
        .replace(
            r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
            &args,
        )
}
type PreparedDiscovery = (
    AgentWorkRetainedController,
    AgentWorkRetainedHandle,
    AgentRuntimeScopedBinding,
    std::thread::JoinHandle<usize>,
    Arc<Mutex<Vec<String>>>,
);
fn prepare(browser: RetainedBrowser, responses: Vec<String>) -> PreparedDiscovery {
    prepare_with_clock(browser, responses, Arc::new(Clock(AtomicU64::new(2))))
}
fn prepare_with_clock(
    browser: RetainedBrowser,
    responses: Vec<String>,
    clock: Arc<dyn TerraControllerClock>,
) -> PreparedDiscovery {
    let binding = browser.binding();
    let scope =
        AgentNavigationDiscovery::try_new(binding.document().clone(), "/".into(), 2).unwrap();
    let input = input_for_context_authority(
        binding.frame().context().identity(),
        binding.frame().origin().clone(),
        clock,
        binding.storage(),
        binding.document().clone(),
        AgentRunBudget::try_new(24, 1_000_000, 1_000_000, 1).unwrap(),
        (
            Instant::now() + Duration::from_secs(30),
            Some(scope.clone()),
        ),
    );
    let task = AgentWorkDiscoveryTask::try_new(
        binding.frame().context().identity(),
        scope,
        vec![SemanticExtractionFieldSchema::try_text("label".into(), true, 64).unwrap()],
    )
    .unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let (transport, server) =
        crate::work_provider_fixture::fixture_provider_inspect(responses, move |_, request| {
            captured.lock().unwrap().push(request.to_owned())
        });
    let (controller, handle, scope) = AgentWorkRetainedController::try_new_for_probe(
        input,
        Box::new(browser),
        transport,
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-not-a-secret".into(),
        )
        .unwrap(),
        Arc::new(Audit(false)),
        Box::new(task),
    )
    .unwrap();
    (controller, handle, scope, server, requests)
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
fn retained_surrounding_inspection_is_citable_only_after_fresh_delivery() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    let context = browser.binding().frame().context().identity();
    native.discovery.store(true, Ordering::Release);
    let mut responses = vec![inspection_stream(
        r#"{"kind":"surrounding_text","target":"@a1","before_bytes":0,"after_bytes":1024}"#,
    )];
    responses.extend(final_streams());
    // Cite the prior capture, which is delivered only in the terminal inventory.
    // The current scoped capture owns @r1; the retained same-document quote owns @r2.
    *responses.last_mut().unwrap() = responses.last().unwrap().replace("@r1", "@r2");
    let (controller, mut result, scope, server, requests) = prepare(browser, responses);
    let (_, lifecycle) = start(controller, scope);
    let AgentWorkRetainedOutcome::Accepted {
        settlement,
        extraction,
    } = finish(&mut result)
    else {
        panic!("scoped extraction failed")
    };
    assert_eq!(settlement.closure().model_calls(), 3);
    assert_eq!(extraction.stats().source_edges(), 1);
    let zephium_agentic::SemanticExtractedValue::Text(value) = extraction.fields()[0].value()
    else {
        panic!("expected cited text");
    };
    let source = extraction
        .sources(value.source_span())
        .unwrap()
        .next()
        .unwrap();
    assert_eq!(source.id.get(), 2);
    assert!(source.observation < extraction.observation());
    assert_eq!(source.frame.context().identity(), context);
    assert!(
        matches!(&source.content, zephium_agentic::SemanticOwnedReadContent::Text(text)
        if text == "Fixture result document_marker_0")
    );
    assert_eq!(native.reads.load(Ordering::Acquire), 2);
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Drained(_)
    ));
    native.join();
    assert_eq!(server.join().unwrap(), 3);
    let requests = requests.lock().unwrap();
    assert!(!requests[0].contains("new_scoped_evidence"));
    assert!(
        requests[1].contains("new_scoped_evidence") && !requests[1].contains("document_marker_0")
    );
    assert!(
        !requests[1].contains("First source"),
        "old actionable links are retired"
    );
    let mapper: serde_json::Value =
        serde_json::from_str(requests[2].split_once("\r\n\r\n").unwrap().1).unwrap();
    let evidence = mapper["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["output"].as_str())
        .find(|text| text.starts_with("ZEXTRACT"))
        .unwrap();
    assert!(evidence.contains("refs=historical_read_only provenance=cohorts_v1"));
    assert!(evidence.contains("R @r1 @a1 text paragraph \"Fixture result new_scoped_evidence\"\n"));
    assert!(
        evidence.contains("R @r2 @a1 text paragraph \"Fixture result document_marker_0\" p=p2\n")
    );
    assert!(evidence.contains(&format!(
        "historical_observation={} generation={} captured_at_ms={} invocation={} snapshot={}",
        source.observation.get(),
        source.observation_generation.get(),
        source.captured_at.millis(),
        source.invocation.get(),
        source.snapshot.get()
    )));
    assert!(
        mapper.get("tools").is_none(),
        "historical refs grant no model action surface"
    );
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(destroy.poll(now()).unwrap().is_some());
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

#[test]
fn retained_history_does_not_restore_inspection_authority_for_old_refs() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.discovery.store(true, Ordering::Release);
    // @a2 was a link in the original capture. The new surrounding capture has
    // only @a1; retaining the original evidence must not make @a2 live again.
    let responses = vec![
        inspection_stream(
            r#"{"kind":"surrounding_text","target":"@a1","before_bytes":0,"after_bytes":1024}"#,
        ),
        inspection_stream(r#"{"kind":"subtree","target":"@a2"}"#)
            .replace("resp_1", "resp_2")
            .replace("fc_1", "fc_2")
            .replace("call_1", "call_2"),
    ];
    let (controller, mut result, scope, server, _) = prepare(browser, responses);
    let (_, lifecycle) = start(controller, scope);
    assert!(!matches!(
        finish(&mut result),
        AgentWorkRetainedOutcome::Accepted { .. }
    ));
    assert_eq!(
        native.reads.load(Ordering::Acquire),
        2,
        "stale ref must refuse before native dispatch"
    );
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Drained(_)
    ));
    native.join();
    assert_eq!(server.join().unwrap(), 2);
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(destroy.poll(now()).unwrap().is_some());
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

#[test]
fn retained_anchor_loss_refreshes_truthful_state_and_continues_original_run() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.discovery.store(true, Ordering::Release);
    native
        .missing_expansion_anchor
        .store(true, Ordering::Release);
    let mut responses = vec![inspection_stream(r#"{"kind":"subtree","target":"@a1"}"#)];
    responses.extend(final_streams());
    let (controller, mut result, scope, server, requests) = prepare(browser, responses);
    let (_, lifecycle) = start(controller, scope);
    let mut events = Vec::new();
    let mut outcome = None;
    wait_until(|| {
        while let Some(event) = result.take_event() {
            events.push(event.kind());
        }
        outcome = result.take_outcome();
        outcome.is_some()
    });
    let AgentWorkRetainedOutcome::Accepted { settlement, .. } = outcome.unwrap() else {
        panic!("fresh same-document capture should permit continuation")
    };
    assert_eq!(settlement.closure().model_calls(), 3);
    assert_eq!(native.reads.load(Ordering::Acquire), 3);
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    assert_eq!(
        events
            .iter()
            .filter(|kind| **kind == AgentWorkEventKind::InspectionAnchorLost)
            .count(),
        1
    );
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Drained(_)
    ));
    native.join();
    assert_eq!(server.join().unwrap(), 3);
    let requests = requests.lock().unwrap();
    assert!(requests[1].contains("failed_anchor_missing"));
    assert!(requests[1].contains("document_marker_0"));
    assert!(!requests[1].contains("new_scoped_evidence"));
    assert!(!requests[1].contains("call_1"));
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(destroy.poll(now()).unwrap().is_some());
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

#[test]
fn retained_anchor_loss_after_document_invalidation_does_not_refresh() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (_owner, native, resource, mut browser) = setup();
    browser
        .register_listener(Arc::new(CountWake(AtomicUsize::new(0))).into())
        .unwrap();
    native.discovery.store(true, Ordering::Release);
    browser.begin_observation(now()).unwrap();
    let previous = browser.poll_observation(now()).unwrap().unwrap();
    let acknowledgement = encode_semantic_observation(
        &previous,
        SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
    )
    .unwrap()
    .admit_conservative_utf8(
        &SemanticTokenizerRevision::try_new("test:anchor-loss:v1".into()).unwrap(),
    )
    .unwrap()
    .settle_delivery(SemanticModelDeliverySettlement::Committed)
    .unwrap();
    native.hold_expansion.store(true, Ordering::Release);
    browser
        .begin_expansion(
            &previous,
            &acknowledgement,
            SemanticReferenceId::new(1).unwrap(),
            SemanticExpansionKind::Subtree,
            now(),
        )
        .unwrap();
    let (request, callback) = native.read.lock().unwrap().take().unwrap();
    native
        .reporters
        .lock()
        .unwrap()
        .get(&resource.identity().context())
        .unwrap()
        .invalidate();
    let (_, completion) = request.into_parts();
    callback(completion.settle(Err(SemanticRuntimePortFailure::Result(
        SemanticRuntimeResultError::Runtime(SemanticRuntimeFault::AnchorMissing),
    ))));
    // Retire the test reporter while its invalidation wake is still coalesced;
    // it must not outlive the deliberately failed application listener.
    native
        .reporters
        .lock()
        .unwrap()
        .remove(&resource.identity().context());
    assert!(
        matches!(browser.poll_observation(now()), Err(error) if error != AgentWorkFailure::InspectionAnchorLost)
    );
    assert!(browser.begin_observation(now()).is_err());
    assert_eq!(native.reads.load(Ordering::Acquire), 2);
    native.join();
}

#[test]
fn retained_expansion_cancellation_and_lost_callback_keep_original_native_owners() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for (reason, lost) in [
        (AgentRuntimeStopReason::Cancelled, false),
        (AgentRuntimeStopReason::HumanTakeover, false),
        (AgentRuntimeStopReason::PolicyRevoked, false),
        (AgentRuntimeStopReason::Cancelled, true),
    ] {
        let (owner, native, resource, browser) = setup();
        native.discovery.store(true, Ordering::Release);
        native.hold_expansion.store(true, Ordering::Release);
        let responses = vec![inspection_stream(r#"{"kind":"subtree","target":"@a1"}"#)];
        let (controller, mut result, scope, server, _) = prepare(browser, responses);
        let (handle, lifecycle) = start(controller, scope);
        wait_until(|| native.read.lock().unwrap().is_some());
        assert_eq!(native.reads.load(Ordering::Acquire), 2);
        handle.stop_and_seal(reason);
        let (request, callback) = native.read.lock().unwrap().take().unwrap();
        if lost {
            drop((request, callback));
        } else {
            read_result(&native, request, callback);
        }
        assert!(!matches!(
            finish(&mut result),
            AgentWorkRetainedOutcome::Accepted { .. }
        ));
        let drain = lifecycle.drain_until(Instant::now() + Duration::from_secs(2));
        assert_eq!(matches!(drain, AgentRuntimeScopedDrain::Drained(_)), !lost);
        native.join();
        assert_eq!(server.join().unwrap(), 1);
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(destroy.poll(now()).unwrap().is_some());
        owner.seal_resources().unwrap();
        assert_eq!(owner.locally_retired(), !lost);
    }
}

#[test]
fn retained_inspection_rejects_bad_refs_scopes_and_synchronous_refusal_without_retry() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for (scope_json, reject, reads) in [
        (r#"{"kind":"subtree","target":"@a99"}"#, false, 1),
        (r#"{"kind":"region","target":"@a2"}"#, false, 1),
        (r#"{"kind":"frame","target":"@a1"}"#, false, 1),
        (
            r#"{"kind":"surrounding_text","target":"@a1","before_bytes":8192,"after_bytes":1}"#,
            false,
            1,
        ),
        (r#"{"kind":"subtree","target":"@a1"}"#, true, 2),
    ] {
        let (owner, native, resource, browser) = setup();
        native.discovery.store(true, Ordering::Release);
        native.reject_expansion.store(reject, Ordering::Release);
        let (controller, mut result, scope, server, _) =
            prepare(browser, vec![inspection_stream(scope_json)]);
        let (_, lifecycle) = start(controller, scope);
        assert!(!matches!(
            finish(&mut result),
            AgentWorkRetainedOutcome::Accepted { .. }
        ));
        assert_eq!(native.reads.load(Ordering::Acquire), reads, "{scope_json}");
        assert!(matches!(
            lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
            AgentRuntimeScopedDrain::Drained(_)
        ));
        native.join();
        assert_eq!(server.join().unwrap(), 1);
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(destroy.poll(now()).unwrap().is_some());
        owner.seal_resources().unwrap();
        assert!(owner.locally_retired());
    }
}

#[test]
fn retained_progressive_capture_retires_old_replay_and_keeps_navigation_live() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.discovery.store(true, Ordering::Release);
    let lease = browser.binding().lease().clone();
    let inspect = response_stream(1)
        .replace("\"extract\"", "\"snapshot\"")
        .replace(
            r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
            r#"{\"scope\":{\"kind\":\"subtree\",\"target\":\"@a1\"}}"#,
        );
    let read = response_stream(1)
        .replace("\"extract\"", "\"read\"")
        .replace(
            r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
            r#"{\"scope\":{\"kind\":\"initial\"}}"#,
        )
        .replace("resp_1", "resp_2")
        .replace("fc_1", "fc_2")
        .replace("call_1", "call_2");
    let mut responses = vec![inspect, read, navigation_stream(3, FIRST)];
    responses.extend(final_streams());
    let (controller, mut result, scope, server, requests) = prepare(browser, responses);
    let (_, lifecycle) = start(controller, scope);
    let AgentWorkRetainedOutcome::Accepted { settlement, .. } = finish(&mut result) else {
        panic!("progressive capture did not accept")
    };
    assert_eq!(settlement.closure().navigations(), 1);
    assert_eq!(settlement.closure().model_calls(), 5);
    assert_eq!(native.reads.load(Ordering::Acquire), 3);
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    assert_eq!(native.destructions.load(Ordering::Acquire), 0);
    assert!(
        matches!(lifecycle.drain_until(Instant::now() + Duration::from_secs(2)), AgentRuntimeScopedDrain::Drained(ref proof) if proof.lease() == &lease)
    );
    native.join();
    assert_eq!(server.join().unwrap(), 5);
    let requests = requests.lock().unwrap();
    assert!(requests[0].contains("document_marker_0"));
    assert!(
        requests[1].contains("new_scoped_evidence") && !requests[1].contains("document_marker_0")
    );
    assert!(!requests[1].contains("call_1") && !requests[1].contains("fc_1"));
    assert!(requests[1].contains("ZEPHIUM_HOST_INSPECTION_PROGRESS_V1"));
    assert!(requests[2].contains("new_scoped_evidence") && requests[2].contains("ZREAD3"));
    assert!(requests[2].contains("ZEPHIUM_HOST_INSPECTION_PROGRESS_V1"));
    assert!(
        requests[3].contains("document_marker_1") && !requests[3].contains("new_scoped_evidence")
    );
    assert_host_checkpoint(&requests[1], "https://retained-fixture.invalid/frozen", &[]);
    assert!(!requests[3].contains("ZEPHIUM_HOST_INSPECTION_PROGRESS_V1"));
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    owner.reap_absent(&resource).unwrap();
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

#[test]
fn retained_dense_region_reaches_counted_mapping_with_exact_sources_and_cleanup() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.discovery.store(true, Ordering::Release);
    native.region_root.store(true, Ordering::Release);
    native.dense_expansion.store(true, Ordering::Release);
    let mut responses = vec![
        navigation_stream(1, FIRST),
        inspection_stream(r#"{"kind":"region","target":"@a1"}"#)
            .replace("resp_1", "resp_2")
            .replace("fc_1", "fc_2")
            .replace("call_1", "call_2"),
    ];
    responses.extend(final_streams());
    let (controller, mut result, scope, server, requests) = prepare(browser, responses);
    let (_, lifecycle) = start(controller, scope);
    let AgentWorkRetainedOutcome::Accepted {
        settlement,
        extraction,
    } = finish(&mut result)
    else {
        panic!("legal dense scoped evidence did not reach source-backed mapping")
    };
    assert_eq!(settlement.closure().model_calls(), 4);
    assert_eq!(settlement.closure().navigations(), 1);
    assert_eq!(extraction.stats().source_edges(), 1);
    assert_eq!(native.reads.load(Ordering::Acquire), 3);
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Drained(_)
    ));
    native.join();
    assert_eq!(server.join().unwrap(), 4);
    let requests = requests.lock().unwrap();
    let mapper_body = requests[3].split_once("\r\n\r\n").unwrap().1;
    let mapper: serde_json::Value = serde_json::from_str(mapper_body).unwrap();
    let evidence = mapper["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["output"].as_str())
        .find(|text| text.starts_with("ZEXTRACT"))
        .unwrap();
    assert!(evidence.len() <= 16 * 1024);
    assert!(evidence.contains("ZREAD3 content=untrusted"));
    assert_eq!(
        evidence
            .lines()
            .filter(|line| line.starts_with("P "))
            .count(),
        2
    );
    assert_eq!(
        evidence
            .lines()
            .filter(|line| line.starts_with("R @r"))
            .count(),
        122
    );
    assert_eq!(
        evidence
            .lines()
            .filter(|line| line.starts_with("R @r") && !line.contains(" p="))
            .count(),
        117
    );
    assert!(evidence.contains("R @r117 @a118 text paragraph"));
    assert!(
        !evidence.contains("document_marker_0"),
        "departure document is never retained across navigation"
    );
    // The five initial arrival-page sources remain historical evidence beside
    // the 117 expanded-region sources, under the same 16 KiB ceiling.
    assert!(
        evidence.contains("R @r118 @a1 text landmark \"Fixture result document_marker_1\" p=p2\n")
    );
    for (index, name) in [
        "First source",
        "Second source",
        "Out of scope source",
        "Original source",
    ]
    .iter()
    .enumerate()
    {
        assert!(evidence.contains(&format!(
            "R @r{} @a{} name link \"{}\" p=p2\n",
            index + 119,
            index + 2,
            name
        )));
    }
    assert!(mapper.get("tools").is_none());
    eprintln!("retained dense mapping: snapshot_request_bytes={} mapper_request_bytes={} extraction_payload_bytes={}",requests[2].split_once("\r\n\r\n").unwrap().1.len(),mapper_body.len(),evidence.len());
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(destroy.poll(now()).unwrap().is_some());
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

#[test]
fn retained_region_initial_oscillation_preserves_progress_not_old_authority() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.discovery.store(true, Ordering::Release);
    native.region_root.store(true, Ordering::Release);
    let mut responses = Vec::new();
    for (index, scope) in [
        r#"{"kind":"region","target":"@a1"}"#,
        r#"{"kind":"initial"}"#,
        r#"{"kind":"region","target":"@a1"}"#,
        r#"{"kind":"initial"}"#,
    ]
    .into_iter()
    .enumerate()
    {
        responses.push(
            inspection_stream(scope)
                .replace("resp_1", &format!("resp_{}", index + 1))
                .replace("fc_1", &format!("fc_{}", index + 1))
                .replace("call_1", &format!("call_{}", index + 1)),
        );
    }
    responses.push(navigation_stream(5, FIRST));
    responses.extend(final_streams());
    let (controller, mut result, scope, server, requests) = prepare(browser, responses);
    let (_, lifecycle) = start(controller, scope);
    let AgentWorkRetainedOutcome::Accepted { settlement, .. } = finish(&mut result) else {
        panic!("bounded inspection history did not survive the observed scope cycle")
    };
    assert_eq!(settlement.closure().model_calls(), 7);
    assert_eq!(settlement.closure().navigations(), 1);
    assert_eq!(native.reads.load(Ordering::Acquire), 6);
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Drained(_)
    ));
    native.join();
    assert_eq!(server.join().unwrap(), 7);
    let requests = requests.lock().unwrap();
    assert!(!requests[0].contains("ZEPHIUM_HOST_INSPECTION_PROGRESS_V1"));
    for (index, request) in requests.iter().enumerate().take(5).skip(1) {
        assert!(request.contains("ZEPHIUM_HOST_INSPECTION_PROGRESS_V1"));
        assert!(request.contains(&format!(r#"\"completed_inspections\":{index}"#)));
        assert!(request.contains(r#"\"current_target\":\"@a1\""#));
        assert!(request.contains(r#"\"incomplete\":true"#));
        assert!(!request.contains("fc_1") && !request.contains("call_1"));
    }
    assert!(
        requests[2].contains("document_marker_0") && !requests[2].contains("new_scoped_evidence")
    );
    assert!(
        requests[3].contains("new_scoped_evidence") && !requests[3].contains("document_marker_0")
    );
    assert!(!requests[5].contains("ZEPHIUM_HOST_INSPECTION_PROGRESS_V1"));
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(destroy.poll(now()).unwrap().is_some());
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

#[test]
fn retained_two_selected_hops_use_original_policy_and_retire_previous_transcripts() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.discovery.store(true, Ordering::Release);
    let lease = browser.binding().lease().clone();
    let mut responses = vec![navigation_stream(1, FIRST), navigation_stream(2, SECOND)];
    responses.extend(final_streams());
    let (controller, mut result, scope, server, requests) = prepare(browser, responses);
    let (_, lifecycle) = start(controller, scope);
    let outcome = finish(&mut result);
    let AgentWorkRetainedOutcome::Accepted {
        settlement,
        extraction,
    } = outcome
    else {
        panic!("two-hop retained controller did not accept");
    };
    assert_eq!(settlement.closure().navigations(), 2);
    assert_eq!(settlement.closure().model_calls(), 4);
    assert_eq!(extraction.stats().source_edges(), 1);
    assert_eq!(native.navigation_count.load(Ordering::Acquire), 2);
    assert_eq!(native.reads.load(Ordering::Acquire), 3);
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    assert_eq!(native.destructions.load(Ordering::Acquire), 0);
    assert!(
        matches!(lifecycle.drain_until(Instant::now() + Duration::from_secs(2)), AgentRuntimeScopedDrain::Drained(ref proof) if proof.lease() == &lease)
    );
    native.join();
    assert_eq!(server.join().unwrap(), 4);
    let requests = requests.lock().unwrap();
    assert!(requests[0].contains("document_marker_0"));
    assert!(requests[1].contains("document_marker_1"));
    assert!(!requests[1].contains("document_marker_0"));
    assert!(requests[2].contains("document_marker_2"));
    assert!(!requests[2].contains("document_marker_1"));
    assert!(!requests[2].contains("document_marker_0"));
    for (index, current, prior) in [
        (0, "https://retained-fixture.invalid/frozen", vec![]),
        (1, FIRST, vec!["https://retained-fixture.invalid/frozen"]),
        (
            2,
            SECOND,
            vec!["https://retained-fixture.invalid/frozen", FIRST],
        ),
    ] {
        assert_host_checkpoint(&requests[index], current, &prior);
    }
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    owner.reap_absent(&resource).unwrap();
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

#[test]
fn retained_read_between_selected_hops_preserves_current_navigation_authority() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.discovery.store(true, Ordering::Release);
    let read = response_stream(1)
        .replace("\"extract\"", "\"read\"")
        .replace(
            r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
            r#"{\"scope\":{\"kind\":\"initial\"}}"#,
        )
        .replace("resp_1", "resp_2")
        .replace("fc_1", "fc_2")
        .replace("call_1", "call_2");
    let mut responses = vec![
        navigation_stream(1, FIRST),
        read,
        navigation_stream(3, SECOND),
    ];
    responses.extend(
        final_streams()
            .into_iter()
            .enumerate()
            .map(|(index, response)| {
                let (from, to) = (index + 3, index + 4);
                response
                    .replace(&format!("resp_{from}"), &format!("resp_{to}"))
                    .replace(&format!("fc_{from}"), &format!("fc_{to}"))
                    .replace(&format!("call_{from}"), &format!("call_{to}"))
                    .replace(&format!("msg_{from}"), &format!("msg_{to}"))
            }),
    );
    let (controller, mut result, scope, server, requests) = prepare(browser, responses);
    let (_, lifecycle) = start(controller, scope);
    let AgentWorkRetainedOutcome::Accepted { settlement, .. } = finish(&mut result) else {
        panic!("baseline read must not invalidate the current observed-link authority");
    };
    assert_eq!(settlement.closure().navigations(), 2);
    assert_eq!(settlement.closure().model_calls(), 5);
    assert_eq!(native.reads.load(Ordering::Acquire), 3);
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Drained(_)
    ));
    native.join();
    assert_eq!(server.join().unwrap(), 5);
    assert!(requests.lock().unwrap()[2].contains("document_marker_1"));
    assert_host_checkpoint(
        &requests.lock().unwrap()[2],
        FIRST,
        &["https://retained-fixture.invalid/frozen"],
    );
    assert!(!requests.lock().unwrap()[3].contains("document_marker_1"));
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(destroy.poll(now()).unwrap().is_some());
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

#[test]
fn invalid_selected_destinations_and_hop_exhaustion_never_dispatch_extra_navigation() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for (targets, dispatched) in [
        (vec!["https://other.invalid/source"], 0),
        (vec!["https://retained-fixture.invalid/unobserved"], 0),
        (vec!["https://retained-fixture.invalid/frozen"], 0),
        (vec![FIRST, FIRST], 1),
        (vec![FIRST, SECOND, FIRST], 2),
    ] {
        let (owner, native, resource, browser) = setup();
        native.discovery.store(true, Ordering::Release);
        let responses = targets
            .iter()
            .enumerate()
            .map(|(index, target)| navigation_stream(index + 1, target))
            .collect();
        let (controller, mut result, scope, server, _) = prepare(browser, responses);
        let (_, lifecycle) = start(controller, scope);
        let AgentWorkRetainedOutcome::ClosedUnsuccessfully(closed) = finish(&mut result) else {
            panic!("invalid navigation must close unsuccessfully");
        };
        assert_eq!(
            closed.policy_settlement().closure().navigations(),
            dispatched as u32
        );
        assert_eq!(native.navigation_count.load(Ordering::Acquire), dispatched);
        assert!(matches!(
            lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
            AgentRuntimeScopedDrain::Drained(_)
        ));
        native.join();
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(matches!(
            destroy.poll(now()).unwrap(),
            Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
                _
            )))
        ));
        owner.seal_resources().unwrap();
        assert!(owner.locally_retired());
        assert_eq!(server.join().unwrap(), targets.len());
    }
}

#[test]
fn stop_during_either_hop_accounts_native_terminal_and_original_policy_before_lease_delivery() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for (hop, reason) in [
        (1, AgentRuntimeStopReason::Cancelled),
        (2, AgentRuntimeStopReason::Cancelled),
        (1, AgentRuntimeStopReason::HumanTakeover),
        (1, AgentRuntimeStopReason::Suspend),
        (1, AgentRuntimeStopReason::PolicyRevoked),
    ] {
        let (owner, native, resource, browser) = setup();
        native.discovery.store(true, Ordering::Release);
        native.hold_navigation.store(true, Ordering::Release);
        let lease = browser.binding().lease().clone();
        let responses = [FIRST, SECOND]
            .into_iter()
            .take(hop)
            .enumerate()
            .map(|(index, target)| navigation_stream(index + 1, target))
            .collect();
        let (controller, mut result, scope, server, _) = prepare(browser, responses);
        let (handle, lifecycle) = start(controller, scope);
        for prior in 1..hop {
            wait_until(|| native.navigation.lock().unwrap().is_some());
            let (request, callback) = native.navigation.lock().unwrap().take().unwrap();
            assert_eq!(
                request
                    .navigation()
                    .operation()
                    .context()
                    .navigation_epoch()
                    .get(),
                prior as u64 + 1
            );
            let target = request.navigation().target().clone();
            callback(request.into_completion().settle(Ok(target)));
        }
        wait_until(|| native.navigation.lock().unwrap().is_some());
        handle.stop_and_seal(reason);
        wait_until(|| {
            owner.shared.lock_rows().unwrap().phase(&resource).unwrap()
                == WorkBrowserResourcePhase::Revoking
        });
        assert!(result.take_outcome().is_none());
        assert!(owner
            .shared
            .lock_rows()
            .unwrap()
            .read_binding(&lease, now())
            .is_err());
        assert_eq!(
            owner
                .shared
                .resource(&resource)
                .unwrap()
                .navigations
                .load(Ordering::Acquire),
            1
        );
        let (request, callback) = native.navigation.lock().unwrap().take().unwrap();
        let target = request.navigation().target().clone();
        callback(request.into_completion().settle(Ok(target)));
        let AgentWorkRetainedOutcome::ClosedUnsuccessfully(closed) = finish(&mut result) else {
            panic!("accounted stopped navigation must close without success");
        };
        assert_eq!(
            closed.policy_settlement().closure().navigations(),
            hop as u32
        );
        assert_eq!(
            closed.policy_settlement().closure().model_calls(),
            hop as u32
        );
        assert_eq!(native.reads.load(Ordering::Acquire), hop);
        assert!(matches!(
            lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
            AgentRuntimeScopedDrain::Drained(_)
        ));
        native.join();
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(matches!(
            destroy.poll(now()).unwrap(),
            Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
                _
            )))
        ));
        owner.seal_resources().unwrap();
        assert!(owner.locally_retired());
        assert_eq!(server.join().unwrap(), hop);
    }
}

#[test]
fn synchronous_native_refusal_reconciles_both_owners_without_restoring_old_reads() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.discovery.store(true, Ordering::Release);
    native.reject_navigation.store(true, Ordering::Release);
    let lease = browser.binding().lease().clone();
    let (controller, mut result, scope, server, _) =
        prepare(browser, vec![navigation_stream(1, FIRST)]);
    let (_, lifecycle) = start(controller, scope);
    let outcome = finish(&mut result);
    assert!(!matches!(
        outcome,
        AgentWorkRetainedOutcome::Accepted { .. }
    ));
    assert!(owner
        .shared
        .lock_rows()
        .unwrap()
        .read_binding(&lease, now())
        .is_err());
    assert_eq!(
        owner
            .shared
            .resource(&resource)
            .unwrap()
            .navigations
            .load(Ordering::Acquire),
        0
    );
    assert_eq!(native.reads.load(Ordering::Acquire), 1);
    let _ = lifecycle.drain_until(Instant::now() + Duration::from_secs(2));
    native.join();
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
    assert_eq!(server.join().unwrap(), 1);
}

#[test]
fn lost_navigation_callback_preserves_recovery_and_original_resource_debt() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.discovery.store(true, Ordering::Release);
    native.hold_navigation.store(true, Ordering::Release);
    let (controller, mut result, scope, server, _) =
        prepare(browser, vec![navigation_stream(1, FIRST)]);
    let (handle, lifecycle) = start(controller, scope);
    wait_until(|| native.navigation.lock().unwrap().is_some());
    handle.stop_and_seal(AgentRuntimeStopReason::Cancelled);
    drop(native.navigation.lock().unwrap().take());
    assert!(matches!(
        finish(&mut result),
        AgentWorkRetainedOutcome::Recovery(_)
    ));
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Unproven
    ));
    native.join();
    let mut destroy = owner.destroy(&resource).unwrap();
    let _ = destroy.poll(now());
    owner.seal_resources().unwrap();
    assert!(!owner.locally_retired());
    assert_eq!(
        owner
            .shared
            .resource(&resource)
            .unwrap()
            .navigations
            .load(Ordering::Acquire),
        1
    );
    assert_eq!(server.join().unwrap(), 1);
}

#[test]
fn undispatched_preparation_retires_old_reads_and_cancels_without_native_debt() {
    let (owner, native, resource, mut browser) = setup();
    browser
        .register_listener(Arc::new(CountWake(AtomicUsize::new(0))).into())
        .unwrap();
    let old = browser.binding().frame().context();
    let lease = browser.binding().lease().clone();
    assert!(browser.prepare_navigation(old, now()).is_err());
    browser.begin_observation(now()).unwrap();
    assert!(browser.poll_observation(now()).unwrap().is_some());
    let operation = browser.prepare_navigation(old, now()).unwrap();
    assert_ne!(operation.context(), old);
    assert!(browser.begin_observation(now()).is_err());
    assert!(owner
        .shared
        .lock_rows()
        .unwrap()
        .read_binding(&lease, now())
        .is_err());
    assert!(browser.prepare_navigation(old, now()).is_err());
    browser.cancel_navigation_preparation().unwrap();
    assert!(browser.begin_observation(now()).is_err());
    assert_eq!(native.navigation_count.load(Ordering::Acquire), 0);
    assert_eq!(
        owner
            .shared
            .resource(&resource)
            .unwrap()
            .navigations
            .load(Ordering::Acquire),
        0
    );
    // Preparation refusal leaves the document quarantined, not restored. The
    // resource owner must explicitly destroy it rather than grant a new lease.
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(destroy.poll(now()).unwrap().is_some());
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

struct FailNextClock(AtomicBool);
impl TerraControllerClock for FailNextClock {
    fn now(&self) -> Result<AgentPolicyInstant, TerraControllerClockError> {
        if self.0.swap(false, Ordering::AcqRel) {
            Err(TerraControllerClockError::Unavailable)
        } else {
            Ok(now())
        }
    }
}

#[test]
fn journal_clock_failure_after_native_decision_accounts_native_debt_but_refuses_closure() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for refused in [false, true] {
        let (owner, native, resource, browser) = setup();
        native.discovery.store(true, Ordering::Release);
        native.reject_navigation.store(refused, Ordering::Release);
        let clock = Arc::new(FailNextClock(AtomicBool::new(false)));
        let trigger = clock.clone();
        *native.after_navigation.lock().unwrap() = Some(Box::new(move || {
            trigger.0.store(true, Ordering::Release);
        }));
        let (controller, mut result, scope, server, _) =
            prepare_with_clock(browser, vec![navigation_stream(1, FIRST)], clock);
        let (_, lifecycle) = start(controller, scope);
        let AgentWorkRetainedOutcome::Recovery(mut recovery) = finish(&mut result) else {
            panic!("failed journal authority cannot become successful or clean closure");
        };
        assert_eq!(recovery.failure(), AgentWorkFailure::Contract);
        // Original journal/session owners survive in Recovery; physical native
        // terminal debt is not fabricated to hide a missing journal record.
        assert!(recovery.audit_status().is_ok());
        assert_eq!(native.reads.load(Ordering::Acquire), 1);
        assert_eq!(
            owner
                .shared
                .resource(&resource)
                .unwrap()
                .navigations
                .load(Ordering::Acquire),
            0
        );
        assert!(matches!(
            lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
            AgentRuntimeScopedDrain::Unproven
        ));
        native.join();
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(destroy.poll(now()).unwrap().is_some());
        owner.seal_resources().unwrap();
        assert!(owner.locally_retired());
        assert_eq!(server.join().unwrap(), 1);
    }
}
