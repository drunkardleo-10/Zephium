use super::*;
use crate::*;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    thread,
};
use zephium_decision::Question;

#[derive(Default)]
struct Accounting {
    reject: bool,
    active: Vec<AgentProviderCallIdentity>,
    receipts: Vec<(AgentModelCallReceipt, AgentProviderInputMetricReceipt)>,
}

impl DecisionCallAccounting for Accounting {
    fn activated(&mut self, call: AgentProviderCallIdentity) -> bool {
        self.active.push(call);
        !self.reject
    }
    fn settled(&mut self, receipt: AgentModelCallReceipt, input: AgentProviderInputMetricReceipt) {
        self.receipts.push((receipt, input));
    }
}

fn admitted_fixture() -> (
    AgentRunPolicy,
    AgentModelCallRequest,
    SemanticObservation,
    DecisionObservation,
) {
    admitted_fixture_with_actions(false)
}

fn admitted_fixture_with_actions(
    actions: bool,
) -> (
    AgentRunPolicy,
    AgentModelCallRequest,
    SemanticObservation,
    DecisionObservation,
) {
    let identity = ContextIdentity::new(
        ContextId::from_raw(11),
        ContextRunId::from_raw(12),
        zephium_core::ids::ProfileId::from(13),
        ContextKind::Owned,
    );
    let mut contexts = ContextRegistry::new();
    contexts
        .reserve(
            identity,
            ContextCapabilities::try_new(ContextKind::Owned, &[ContextCapability::Observe])
                .unwrap(),
        )
        .unwrap();
    let op = contexts
        .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
        .unwrap();
    contexts
        .settle_construction(identity.id(), op, ContextSettlement::Applied)
        .unwrap();
    let context = contexts.join(identity.id()).unwrap();
    let source = SemanticOrigin::parse("https://example.test/").unwrap();
    let frame = SemanticFrameJoin::try_new(
        context,
        FrameId::MAIN,
        FrameGeneration::INITIAL,
        source.clone(),
        SemanticFrameTrust::SameOrigin,
    )
    .unwrap();
    let nodes = if actions {
        json!([{"k":1,"r":"document","o":16},{"k":2,"p":0,"r":"button","n":"Details","o":1}])
    } else {
        json!([{"k":1,"r":"document"}])
    };
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(1).unwrap(),
            frame,
            SemanticSnapshotGeneration::new(1).unwrap(),
        ),
        &serde_json::to_vec(
            &json!({"v":SEMANTIC_WIRE_VERSION,"i":1,"g":1,"c":"complete","n":nodes}),
        )
        .unwrap(),
    )
    .unwrap();
    let observation = SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(1).unwrap(),
            context,
            SemanticObservationBudget::try_new(8, 4096, 1).unwrap(),
        ),
        snapshot,
    )
    .unwrap()
    .finish()
    .unwrap();
    let account = AgentContextAccountBinding::new(
        AgentAccountAttestationId::from_raw(14),
        context,
        AgentAccountScope::Anonymous,
        AgentPolicyInstant::from_millis(100),
    );
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).unwrap();
    let budget = AgentRunBudget::try_new(3, 100_000, 10_000, 1).unwrap();
    let node = AgentPlanNodeId::from_raw(15);
    let lease = AgentPlanLeaseId::from_raw(16);
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::from_raw(17),
        ContextRunId::from_raw(12),
        AgentRunScope::try_new(
            vec![identity.profile()],
            vec![AgentAccountScope::Anonymous],
            vec![source.clone()],
            SemanticSensitivity::Public,
            effects,
            vec![],
        )
        .unwrap(),
        budget,
        AgentPolicyInstant::from_millis(1),
        AgentPolicyInstant::from_millis(10000),
        vec![AgentPlanNodeScope::new(
            node,
            AgentPlanNodeAuthority::try_new(
                vec![identity.profile()],
                vec![AgentAccountScope::Anonymous],
                vec![source],
                SemanticSensitivity::Public,
                effects,
            )
            .unwrap(),
            budget,
            AgentPolicyInstant::from_millis(9999),
        )],
    )
    .unwrap();
    let policy =
        AgentRunPolicy::try_new(manifest, vec![AgentPlanLeaseBinding::new(lease, node)]).unwrap();
    let call = AgentModelCallRequest::new(
        AgentModelCallId::new(1).unwrap(),
        lease,
        account,
        JevDecisionClient::call_budget().unwrap(),
        AgentPolicyInstant::from_millis(101),
    );
    let objective = AgentProviderObjective::try_admit_conservative_utf8(
        "Read this public document".into(),
        &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
    )
    .unwrap();
    let entries: Vec<_> = observation
        .frames()
        .iter()
        .flat_map(SemanticSnapshot::nodes)
        .filter(|node| !node.operations().is_empty())
        .map(|node| (node.reference(), node.operations()))
        .collect();
    let authority = AgentProviderActionAuthority::try_new(&observation, &entries).unwrap();
    let projection =
        DecisionObservation::try_new(&observation, &objective, &authority, account).unwrap();
    (policy, call, observation, projection)
}

#[tokio::test]
async fn admitted_decision_settles_actual_usage_and_dropped_dispatch_charges_the_ceiling() {
    let (endpoint, server) = server(vec![success()]);
    let client = client(endpoint);
    let server = server();
    let (mut policy, call, observation, projection) = admitted_fixture();
    let mut accounting = Accounting::default();
    let output = client
        .evaluate_observation_accounted(
            &mut policy,
            call,
            &observation,
            &projection,
            Instant::now() + Duration::from_secs(4),
            &AgentProviderCancellation::new(),
            Some(&mut accounting),
        )
        .await
        .unwrap();
    assert_eq!(server.join().unwrap(), 1);
    assert_eq!(output.receipt.input_tokens(), 123);
    assert_eq!(output.receipt.output_tokens(), 7);
    assert_eq!(accounting.active.len(), 1);
    assert_eq!(accounting.receipts, vec![(output.receipt, output.input)]);
    assert_eq!(output.input.call(), output.receipt.id());
    let tokens = output.input.metrics().structured_input_tokens().unwrap();
    assert_eq!(tokens.tokens(), 123);
    assert_eq!(tokens.quality(), SemanticTokenCountQuality::ProviderExact);
    assert_eq!(policy.pending_model_calls(), 0);

    let (mut policy, call, observation, projection) = admitted_fixture();
    let admission = policy
        .prepare_decision_input(
            call,
            &observation,
            &projection,
            u64::from(MAX_JEV_INPUT_TOKENS),
        )
        .unwrap();
    let ack = crate::semantic_diff::SemanticObservationAcknowledgement::from_fingerprint(
        crate::semantic_diff::SemanticObservationFingerprint::from_observation(&observation),
    );
    let active = policy.commit_observation_input(admission, &ack).unwrap();
    let input = AgentProviderInputMetricReceipt::from_decision(
        &active,
        projection.input_stats(),
        projection.request().encode().unwrap().len() as u32,
        MAX_JEV_INPUT_TOKENS,
        SemanticTokenCountQuality::Conservative,
    );
    let mut cancelled = Accounting::default();
    drop(DecisionPolicyGuard {
        policy: &mut policy,
        active: Some(active),
        input,
        accounting: Some(&mut cancelled),
    });
    assert_eq!(cancelled.receipts.len(), 1);
    let (receipt, input) = cancelled.receipts[0];
    assert_eq!(receipt.id(), input.call());
    assert_eq!(receipt.input_tokens(), u64::from(MAX_JEV_INPUT_TOKENS));
    assert_eq!(
        input.metrics().structured_input_tokens().unwrap().quality(),
        SemanticTokenCountQuality::Conservative
    );
    assert_eq!(policy.pending_model_calls(), 0);
    assert_eq!(
        policy.accounting().consumed_model_tokens(),
        u64::from(MAX_JEV_INPUT_TOKENS + MAX_JEV_OUTPUT_TOKENS)
    );
    assert_eq!(
        policy.accounting().consumed_cost_micro_usd(),
        jev_cost(MAX_JEV_INPUT_TOKENS)
    );
}

#[tokio::test]
async fn host_accounting_refusal_stops_transport_and_preserves_terminal_receipts() {
    let (endpoint, _) = server(vec![]);
    let client = client(endpoint);
    let (mut policy, call, observation, projection) = admitted_fixture();
    let mut accounting = Accounting {
        reject: true,
        ..Accounting::default()
    };
    let result = client
        .evaluate_observation_accounted(
            &mut policy,
            call,
            &observation,
            &projection,
            Instant::now() + Duration::from_secs(2),
            &AgentProviderCancellation::new(),
            Some(&mut accounting),
        )
        .await;
    assert!(matches!(result, Err(AgentPolicyError::Authority)));
    assert_eq!(accounting.active.len(), 1);
    assert_eq!(accounting.receipts.len(), 1);
    assert_eq!(policy.pending_model_calls(), 0);
    assert!(client.transport.snapshot().unwrap().is_idle());
}

#[tokio::test]
async fn over_ceiling_vendor_usage_never_becomes_exact_accounting() {
    let body = json!({"model":zephium_decision::JEV_MODEL,"answers":{},"usage":{"input_tokens":MAX_JEV_INPUT_TOKENS + 1,"output_tokens":7}});
    let (endpoint, server) = server(vec![response(200, "", &body.to_string())]);
    let client = client(endpoint);
    let server = server();
    let (mut policy, call, observation, projection) = admitted_fixture();
    let output = client
        .evaluate_observation(
            &mut policy,
            call,
            &observation,
            &projection,
            Instant::now() + Duration::from_secs(4),
            &AgentProviderCancellation::new(),
        )
        .await
        .unwrap();
    assert_eq!(server.join().unwrap(), 1);
    assert!(output.call.response.is_err());
    assert_eq!(output.call.diagnostic.input_tokens, None);
    assert_eq!(
        output.receipt.input_tokens(),
        u64::from(MAX_JEV_INPUT_TOKENS)
    );
    assert_eq!(policy.pending_model_calls(), 0);
}

fn emulation_config() -> WorkPlanningConfig {
    WorkPlanningConfig::try_new(
        AgentProviderCallConfig::try_for_test(
            AgentProviderKind::OpenAiResponses,
            AgentProviderModelRevision::try_new("gpt-5.6-terra".into()).unwrap(),
            AgentProviderReasoningEffort::Medium,
            SemanticTokenizerRevision::try_new("decision-fixture:v1".into()).unwrap(),
            AgentProviderPricingProfile::try_new(
                AgentProviderPricingRevision::new(1).unwrap(),
                32768,
            )
            .unwrap(),
            1,
            4096,
            AgentProviderStreamBudget::STANDARD,
        )
        .unwrap(),
        8192,
        10_000,
    )
    .unwrap()
}

fn fixture_answers(request: &DecisionRequest) -> serde_json::Value {
    let answers: BTreeMap<_, _> = request.questions().iter().map(|(key, question)| {
        let value = match question {
            Question::Noul { .. } => json!({"type":"noul","noul":0.01}),
            Question::Choice { criteria, .. } => {
                let probabilities: BTreeMap<_, _> = criteria.keys().map(|key| (key, if key == "none" { 1.0 } else { 0.0 })).collect();
                json!({"type":"choice","choice":"none","confidence":1.0,"probabilities":probabilities})
            }
            Question::Score { .. } => panic!("unexpected fixture kind"),
        };
        (key.clone(), value)
    }).collect();
    json!({"answers":answers})
}

#[tokio::test]
async fn borrowed_emulation_admits_disclosure_and_conservatively_settles_bad_envelopes() {
    for extra_item in [false, true] {
        let (mut policy, call, observation, projection) = admitted_fixture();
        let mut body = json!({"object":"response","status":"completed","model":"gpt-5.6-terra","service_tier":"default","error":null,"incomplete_details":null,
            "output":[{"type":"reasoning","summary":[]},{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":fixture_answers(projection.request()).to_string()}]}],
            "usage":{"input_tokens":100,"output_tokens":200,"total_tokens":300,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":50}}});
        if extra_item {
            body["output"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"reasoning","summary":[]}));
        }
        let (mut endpoint, server) = server_for_model(
            vec![
                response(
                    200,
                    "",
                    r#"{"object":"response.input_tokens","input_tokens":100}"#,
                ),
                response(200, "", &body.to_string()),
            ],
            "gpt-5.6-terra",
        );
        endpoint.set_path("/v1/responses");
        let transport = AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::STANDARD,
            endpoint.as_str(),
            endpoint.as_str(),
        )
        .unwrap();
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-key".into(),
        )
        .unwrap();
        let config = emulation_config();
        let client = OpenAiDecisionCall::try_new(&transport, &credential, &config).unwrap();
        let call = AgentModelCallRequest::new(
            call.id(),
            call.lease(),
            call.account(),
            client.call_budget().unwrap(),
            AgentPolicyInstant::from_millis(101),
        );
        let server = server();
        let mut accounting = Accounting::default();
        let output = client
            .evaluate_observation(
                &mut policy,
                call,
                &observation,
                &projection,
                Instant::now() + Duration::from_secs(4),
                &AgentProviderCancellation::new(),
                Some(&mut accounting),
            )
            .await
            .unwrap();
        assert_eq!(server.join().unwrap(), 2);
        assert_eq!(accounting.active.len(), 1);
        assert_eq!(accounting.receipts, vec![(output.receipt, output.input)]);
        assert_eq!(
            output
                .input
                .metrics()
                .structured_input_tokens()
                .unwrap()
                .tokens(),
            100
        );
        if extra_item {
            assert!(output.call.response.is_err());
            assert_eq!(
                output.call.diagnostic.envelope_failure,
                Some(DecisionEnvelopeFailure::ItemCount)
            );
            assert_eq!(
                output.receipt.usage_accounting(),
                AgentModelUsageAccounting::ReservationCeiling
            );
            assert_eq!(output.receipt.input_tokens(), 8192);
            assert_eq!(output.receipt.output_tokens(), 4096);
            assert_eq!(output.receipt.cost_micro_usd(), 10_000);
        } else {
            let answers = output.call.response.unwrap();
            assert!(answers.answers.values().all(Result::is_ok));
            assert_eq!(
                output.receipt.usage_accounting(),
                AgentModelUsageAccounting::Exact
            );
            assert_eq!(output.receipt.input_tokens(), 100);
            assert_eq!(output.receipt.output_tokens(), 200);
        }
        assert_eq!(policy.pending_model_calls(), 0);
        assert_eq!(credential.provider(), AgentProviderKind::OpenAiResponses);
        assert!(transport.snapshot().unwrap().is_idle());
    }
}

#[tokio::test]
async fn dropping_dispatched_emulation_retains_receipts_and_seals_its_shared_transport() {
    let hold = Arc::new(AtomicBool::new(true));
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (mut endpoint, server) = server_for_model_with_gate(
        vec![
            response(
                200,
                "",
                r#"{"object":"response.input_tokens","input_tokens":100}"#,
            ),
            response(200, "", "{}"),
        ],
        "gpt-5.6-terra",
        Some((hold.clone(), seen.clone())),
    );
    endpoint.set_path("/v1/responses");
    let transport = AgentProviderTransport::try_new_loopback(
        AgentProviderTransportConfig::STANDARD,
        endpoint.as_str(),
        endpoint.as_str(),
    )
    .unwrap();
    let credential =
        AgentProviderCredential::try_new(AgentProviderKind::OpenAiResponses, "fixture-key".into())
            .unwrap();
    let config = emulation_config();
    let client = OpenAiDecisionCall::try_new(&transport, &credential, &config).unwrap();
    let (mut policy, call, observation, projection) = admitted_fixture();
    let call = AgentModelCallRequest::new(
        call.id(),
        call.lease(),
        call.account(),
        client.call_budget().unwrap(),
        AgentPolicyInstant::from_millis(101),
    );
    let cancellation = AgentProviderCancellation::new();
    let mut accounting = Accounting::default();
    let server = server();
    let mut pending = Box::pin(client.evaluate_observation(
        &mut policy,
        call,
        &observation,
        &projection,
        Instant::now() + Duration::from_secs(5),
        &cancellation,
        Some(&mut accounting),
    ));
    while seen.load(Ordering::SeqCst) != 2 {
        tokio::select! {
            _ = &mut pending => panic!("call ended before generation was held"),
            _ = tokio::time::sleep(Duration::from_millis(2)) => {},
        }
    }
    drop(pending);
    hold.store(false, Ordering::SeqCst);
    assert_eq!(server.join().unwrap(), 2);
    assert_eq!(accounting.active.len(), 1);
    assert_eq!(accounting.receipts.len(), 1);
    let (receipt, input) = accounting.receipts[0];
    assert_eq!(receipt.id(), input.call());
    assert_eq!(
        receipt.usage_accounting(),
        AgentModelUsageAccounting::ReservationCeiling
    );
    assert_eq!(receipt.input_tokens(), 8192);
    assert_eq!(receipt.output_tokens(), 4096);
    assert_eq!(receipt.cost_micro_usd(), 10_000);
    assert_eq!(policy.pending_model_calls(), 0);
    assert!(transport.snapshot().unwrap().is_sealed());
}

#[test]
fn fallback_projection_preserves_binding_and_only_repeats_uncertain_heads() {
    let (_, call, observation, projection) = admitted_fixture();
    let mut body = fixture_answers(projection.request());
    body["answers"]["challenge"]["noul"] = json!(0.5);
    let primary = projection
        .request()
        .decode_emulation(
            &serde_json::to_vec(&body).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    let fallback = projection.route(Ok(primary)).unwrap();
    let subset = fallback.projection().unwrap();
    assert_eq!(subset.question_count(), 1);
    assert!(subset.matches(&observation, call.account()));
    assert!(subset.request().questions().contains_key("challenge"));
    let emulation = subset
        .request()
        .decode_emulation(
            &serde_json::to_vec(&fixture_answers(subset.request())).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    let mut results = fallback.finish(Some(emulation));
    let foreign_account = AgentContextAccountBinding::new(
        AgentAccountAttestationId::from_raw(999),
        call.account().context(),
        AgentAccountScope::Anonymous,
        call.account().observed_at(),
    );
    assert!(results
        .take_challenge(&observation, foreign_account)
        .is_err());
    assert_eq!(
        results
            .take_challenge(&observation, call.account())
            .unwrap(),
        Some(false)
    );
    assert_eq!(
        results
            .take_challenge(&observation, call.account())
            .unwrap(),
        None
    );
    assert_eq!(
        results
            .take_operation(&observation, call.account())
            .unwrap(),
        None
    );
}

#[test]
fn action_selection_consumes_only_the_compatible_target_and_cannot_be_replayed() {
    for select_click in [false, true] {
        let (_, call, observation, projection) = admitted_fixture_with_actions(true);
        let mut body = fixture_answers(projection.request());
        for (key, selection) in [
            ("operation", "click"),
            ("scroll_target", "@a1"),
            ("click_target", if select_click { "@a2" } else { "none" }),
        ] {
            let Question::Choice { criteria, .. } = &projection.request().questions()[key] else {
                panic!("choice expected");
            };
            let selection = if key == "scroll_target" || (key == "click_target" && select_click) {
                criteria
                    .keys()
                    .find(|key| key.as_str() != "none")
                    .unwrap()
                    .as_str()
            } else {
                selection
            };
            let probabilities: BTreeMap<_, _> = criteria
                .keys()
                .map(|key| (key, if key == selection { 1.0 } else { 0.0 }))
                .collect();
            body["answers"][key] = json!({"type":"choice","choice":selection,"confidence":1.0,"probabilities":probabilities});
        }
        let response = projection
            .request()
            .decode_emulation(
                &serde_json::to_vec(&body).unwrap(),
                DecisionUsage::default(),
            )
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        let selection = answers
            .take_operation(&observation, call.account())
            .unwrap();
        assert_eq!(
            selection,
            if select_click {
                Some(DecisionOperation::Click(
                    observation.frames()[0].nodes()[1].reference(),
                ))
            } else {
                None
            }
        );
        assert_eq!(
            answers
                .take_operation(&observation, call.account())
                .unwrap(),
            None
        );
    }
}

fn request() -> DecisionRequest {
    DecisionRequest::try_new(
        json!("public fixed fixture"),
        BTreeMap::from([(
            "challenge".into(),
            Question::noul(json!("A challenge?"), None),
        )]),
    )
    .unwrap()
}

fn limits() -> WorkExecutionLimits {
    WorkExecutionLimits {
        model_tokens: 100_000,
        cost_micro_usd: 100_000,
        operations: 2,
        timeout_seconds: 30,
        max_workers: 1,
    }
}

fn response(status: u16, headers: &str, body: &str) -> String {
    format!("HTTP/1.1 {status} fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len())
}

fn success() -> String {
    response(200, "", &json!({"model":zephium_decision::JEV_MODEL,"answers":{"challenge":{"type":"noul","noul":0.02}},"usage":{"input_tokens":123,"output_tokens":7}}).to_string())
}

fn server(responses: Vec<String>) -> (Url, impl FnOnce() -> thread::JoinHandle<usize>) {
    server_for_model(responses, zephium_decision::JEV_MODEL)
}

fn server_for_model(
    responses: Vec<String>,
    model: &'static str,
) -> (Url, impl FnOnce() -> thread::JoinHandle<usize>) {
    server_for_model_with_gate(responses, model, None)
}

type ServerGate = (Arc<AtomicBool>, Arc<std::sync::atomic::AtomicUsize>);

fn server_for_model_with_gate(
    responses: Vec<String>,
    model: &'static str,
    gate: Option<ServerGate>,
) -> (Url, impl FnOnce() -> thread::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = Url::parse(&format!(
        "http://127.0.0.1:{}/v1/systemone",
        listener.local_addr().unwrap().port()
    ))
    .unwrap();
    let join = move || {
        thread::spawn(move || {
            let mut count = 0;
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(2))
                        }
                        _ => return count,
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let length = socket.read(&mut chunk).unwrap();
                    assert!(length > 0 && bytes.len() + length < 128 * 1024);
                    bytes.extend_from_slice(&chunk[..length]);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                        let body_len: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(str::to_owned)
                            })
                            .unwrap()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + body_len {
                            let request: serde_json::Value =
                                serde_json::from_slice(&bytes[end + 4..]).unwrap();
                            assert!(request["model"] == model);
                            break;
                        }
                    }
                }
                count += 1;
                if let Some((hold, seen)) = &gate {
                    seen.store(count, Ordering::SeqCst);
                    while count == 2 && hold.load(Ordering::SeqCst) {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(2));
                    }
                    let _ = socket.write_all(response.as_bytes());
                } else {
                    socket.write_all(response.as_bytes()).unwrap();
                }
            }
            count
        })
    };
    (endpoint, join)
}

fn client(endpoint: Url) -> JevDecisionClient {
    JevDecisionClient::new(
        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD).unwrap(),
        AgentProviderCredential::try_new(
            DecisionCredentialProvider::TypeSafe,
            "fixture-key".into(),
        )
        .unwrap(),
        endpoint,
        DecisionBackendKind::Jev,
    )
    .unwrap()
}

#[tokio::test]
async fn retries_only_explicit_rate_limit_and_accounts_one_answer() {
    let (endpoint, join) = server(vec![response(429, "Retry-After: 0\r\n", "{}"), success()]);
    let client = client(endpoint);
    let join = join();
    let output = client
        .run(
            &request(),
            limits(),
            Instant::now() + Duration::from_secs(3),
            &AgentProviderCancellation::new(),
        )
        .await;
    assert!(output.response.is_ok(), "{:?}", output.diagnostic);
    assert_eq!(output.diagnostic.attempts, Some(2));
    assert_eq!(output.charged_usage.input_tokens, 123);
    assert_eq!(output.cost_micro_usd, 6);
    assert_eq!(join.join().unwrap(), 2);
    assert!(client.transport.snapshot().unwrap().is_idle());
}

#[tokio::test]
async fn invalid_answer_charges_ceiling_and_preserves_fallback_transport() {
    let (endpoint, join) = server(vec![response(200, "", "{\"unexpected\":true}")]);
    let client = client(endpoint);
    let join = join();
    let output = client
        .run(
            &request(),
            limits(),
            Instant::now() + Duration::from_secs(3),
            &AgentProviderCancellation::new(),
        )
        .await;
    assert!(
        matches!(output.response, Err(DecisionCallFailure::InvalidAnswer)),
        "{:?}",
        output.diagnostic
    );
    assert_eq!(output.charged_usage.input_tokens, MAX_JEV_INPUT_TOKENS);
    assert!(!client.transport.snapshot().unwrap().is_sealed());
    assert_eq!(join.join().unwrap(), 1);
}

#[tokio::test]
async fn refuses_retry_after_beyond_deadline_without_dispatching_again() {
    let (endpoint, join) = server(vec![response(529, "Retry-After: 100\r\n", "{}")]);
    let client = client(endpoint);
    let join = join();
    let output = client
        .run(
            &request(),
            limits(),
            Instant::now() + Duration::from_secs(2),
            &AgentProviderCancellation::new(),
        )
        .await;
    assert!(
        matches!(output.response, Err(DecisionCallFailure::Overloaded)),
        "{:?}",
        output.diagnostic
    );
    assert_eq!(output.diagnostic.attempts, Some(1));
    assert_eq!(output.cost_micro_usd, 0);
    assert_eq!(join.join().unwrap(), 1);
}

#[tokio::test]
async fn cancellation_and_insufficient_budget_do_not_dispatch() {
    let client = client(Url::parse("http://127.0.0.1:9/v1/systemone").unwrap());
    let cancelled = AgentProviderCancellation::new();
    cancelled.cancel();
    let output = client
        .run(
            &request(),
            limits(),
            Instant::now() + Duration::from_secs(2),
            &cancelled,
        )
        .await;
    assert!(matches!(
        output.response,
        Err(DecisionCallFailure::Cancelled)
    ));
    assert_eq!(output.diagnostic.attempts, Some(0));
    let mut limited = limits();
    limited.model_tokens = 1;
    let output = client
        .run(
            &request(),
            limited,
            Instant::now() + Duration::from_secs(2),
            &AgentProviderCancellation::new(),
        )
        .await;
    assert!(matches!(
        output.response,
        Err(DecisionCallFailure::Capacity)
    ));
    assert_eq!(output.cost_micro_usd, 0);
}

#[test]
fn retry_after_honors_dates_and_refuses_malformed_values() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let mut headers = HeaderMap::new();
    headers.insert(
        RETRY_AFTER,
        HeaderValue::from_str(&httpdate::fmt_http_date(now + Duration::from_secs(12))).unwrap(),
    );
    assert_eq!(retry_delay(&headers, 1, now), Some(Duration::from_secs(12)));
    headers.insert(RETRY_AFTER, HeaderValue::from_static("invalid"));
    assert_eq!(retry_delay(&headers, 1, now), None);
    headers.insert(
        RETRY_AFTER,
        HeaderValue::from_static("18446744073709551616"),
    );
    assert_eq!(retry_delay(&headers, 1, now), None);
}

#[test]
fn cloud_endpoint_and_credentials_are_bound_independently_of_page_data() {
    assert_eq!(
        cloud_endpoint("https://cloud.zephium.app/decisions/")
            .unwrap()
            .path(),
        "/decisions/v1/systemone"
    );
    for base in [
        "http://cloud.zephium.app",
        "https://user@cloud.zephium.app",
        "https://cloud.zephium.app?token=secret",
        "https://cloud.zephium.app/#fragment",
        "https://127.0.0.1",
        "https://localhost",
        "https://cloud.zephium.app:8443",
    ] {
        assert!(cloud_endpoint(base).is_err());
    }
    let credential = AgentProviderCredential::try_new(
        DecisionCredentialProvider::ZephiumCloud,
        "fixture-secret".into(),
    )
    .unwrap();
    assert!(!format!("{credential:?}").contains("fixture-secret"));
    assert!(JevDecisionClient::direct(
        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD).unwrap(),
        credential
    )
    .is_err());
}
