use super::*;
use crate::{
    AgentProviderModelRevision, AgentProviderPricingProfile, AgentProviderPricingRevision,
    AgentProviderReasoningEffort, AgentProviderStreamBudget, SemanticTokenizerRevision,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
};
fn config() -> WorkPlanningConfig {
    WorkPlanningConfig::try_new(
        AgentProviderCallConfig::try_for_test(
            AgentProviderKind::OpenAiResponses,
            AgentProviderModelRevision::try_new("gpt-5.6-terra".into()).unwrap(),
            AgentProviderReasoningEffort::Medium,
            SemanticTokenizerRevision::try_new("planning-fixture:v1".into()).unwrap(),
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
        100_000,
    )
    .unwrap()
}
fn disclosure() -> WorkPlanningDisclosure {
    WorkPlanningDisclosure::from_snapshot(
        &zephium_core::work::WorkSnapshot::create(
            100.into(),
            200.into(),
            "Compare two public libraries".into(),
        )
        .unwrap(),
    )
    .unwrap()
}
fn response() -> Value {
    json!({"object":"response","status":"completed","model":"gpt-5.6-terra","service_tier":"default","error":null,"incomplete_details":null,
            "output":[{"type":"reasoning","summary":[]},{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"{\"proposal\":{\"kind\":\"clarify\",\"prompt\":\"Which audience?\",\"options\":[]}}"}]}],
            "usage":{"input_tokens":100,"output_tokens":200,"total_tokens":300,"input_tokens_details":{"cached_tokens":10},"output_tokens_details":{"reasoning_tokens":50}}})
}
#[test]
fn planning_decoder_rejects_protocol_identity_usage_and_proposal_confusion() {
    let config = config();
    assert!(
        decode(&serde_json::to_vec(&response()).unwrap(), 100, &config)
            .unwrap()
            .is_ok()
    );
    for (pointer, value) in [
        ("/status", json!("incomplete")),
        ("/model", json!("unreviewed-model")),
        ("/service_tier", json!("priority")),
        ("/usage/input_tokens", json!(101)),
        ("/usage/total_tokens", json!(301)),
        ("/usage/output_tokens_details/reasoning_tokens", json!(201)),
        ("/usage/input_tokens_details/cached_tokens", json!(101)),
        ("/output/1/type", json!("function_call")),
        ("/output/1/role", json!("user")),
        ("/output/1/status", json!("in_progress")),
        (
            "/output/1/content/0/text",
            json!("{\"proposal\":{\"kind\":\"draft\",\"plan\":{\"nodes\":[]}}}"),
        ),
        (
            "/output/1/content/0/text",
            json!(
                "{\"proposal\":{\"kind\":\"clarify\",\"prompt\":\"?\",\"options\":[],\"approved\":true}}"
            ),
        ),
    ] {
        let mut changed = response();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            decode(&serde_json::to_vec(&changed).unwrap(), 100, &config).is_none(),
            "accepted {pointer}"
        );
    }
}
#[test]
fn planning_refusals_preserve_usage_and_duplicate_messages_are_rejected() {
    let mut refused = response();
    refused["output"][1]["content"] = json!([{"type":"refusal","refusal":"Cannot comply"}]);
    let result = decode(&serde_json::to_vec(&refused).unwrap(), 100, &config()).unwrap();
    assert!(matches!(
        result,
        Err(WorkPlanningUsage {
            input_tokens: 100,
            ..
        })
    ));
    let message = response()["output"][1].clone();
    for outputs in [
        json!([message, message]),
        json!([refused["output"][1], message]),
        json!([message,{"type":"reasoning"}]),
    ] {
        let mut changed = response();
        changed["output"] = outputs;
        assert!(decode(&serde_json::to_vec(&changed).unwrap(), 100, &config()).is_none());
    }
    let bytes = serde_json::to_string(&response()).unwrap().replace(
        "\"status\":\"completed\"",
        "\"status\":\"completed\",\"status\":\"incomplete\"",
    );
    assert!(decode(bytes.as_bytes(), 100, &config()).is_none());
}
struct Server {
    endpoint: String,
    thread: std::thread::JoinHandle<Vec<(String, Value, Vec<u8>)>>,
    hold: Arc<AtomicBool>,
    seen: Arc<std::sync::atomic::AtomicUsize>,
}
impl Server {
    fn new(responses: Vec<Vec<u8>>) -> Self {
        Self::with_hold(responses, false)
    }
    fn with_hold(responses: Vec<Vec<u8>>, hold_generation: bool) -> Self {
        let hold = Arc::new(AtomicBool::new(hold_generation));
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread_hold = hold.clone();
        let thread_seen = seen.clone();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let thread = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(180);
            let mut requests = vec![];
            for body in responses {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "fixture accept timeout");
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                // Normalize the accepted socket independently of the polling listener.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(120)))
                    .unwrap();
                let mut bytes = vec![];
                let mut buf = [0u8; 4096];
                let (head_end, length) = loop {
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    assert!(bytes.len() < MAX_REQUEST + 8192);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let head = std::str::from_utf8(&bytes[..end]).unwrap();
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(|v| v.parse::<usize>().unwrap())
                            })
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                assert!(length <= MAX_REQUEST);
                while bytes.len() < head_end + length {
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                }
                let path = std::str::from_utf8(&bytes[..head_end])
                    .unwrap()
                    .lines()
                    .next()
                    .unwrap()
                    .to_owned();
                requests.push((
                    path,
                    serde_json::from_slice(&bytes[head_end..]).unwrap(),
                    bytes[head_end..].to_vec(),
                ));
                thread_seen.store(requests.len(), Ordering::SeqCst);
                while requests.len() == 2 && thread_hold.load(Ordering::SeqCst) {
                    assert!(Instant::now() < deadline, "fixture release timeout");
                    std::thread::sleep(Duration::from_millis(1));
                }
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
            requests
        });
        Self {
            endpoint,
            thread,
            hold,
            seen,
        }
    }
    fn transport(&self) -> AgentProviderTransport {
        // macOS may cold-load system networking libraries during the first
        // request. Assertions synchronize on actual dispatch, not startup speed.
        AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::try_new(
                Duration::from_secs(150),
                Duration::from_secs(30),
                Duration::from_secs(120),
            )
            .unwrap(),
            &self.endpoint,
            &self.endpoint.replace("/responses", "/messages"),
        )
        .unwrap()
    }
}
fn planner(transport: AgentProviderTransport) -> OpenAiWorkPlanner {
    OpenAiWorkPlanner::try_new(
        transport,
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-only-key".into(),
        )
        .unwrap(),
        config(),
    )
    .unwrap()
}

fn synthesis_disclosure(tokens: u32) -> zephium_core::work::synthesis::WorkSynthesisDisclosure {
    use zephium_core::work::{runtime::*, synthesis::*, *};
    WorkSynthesisDisclosure::try_new(
        &WorkPlanNode {
            id: 1.into(),
            objective: "Prepare a release checklist for review".into(),
            dependencies: vec![],
            outputs: vec![WorkExpectedOutput {
                name: "checklist".into(),
                description: "Suggested release checks".into(),
                review: WorkOutputReview::UserAcceptance,
            }],
        },
        &[],
        &[],
        WorkExecutionLimits {
            model_tokens: tokens,
            cost_micro_usd: 100_000,
            operations: 2,
            timeout_seconds: 170,
            max_workers: 1,
        },
    )
    .unwrap()
}

#[tokio::test]
async fn synthesis_transport_counts_exact_context_and_returns_semantic_artifacts_without_authority()
{
    use zephium_core::work::{artifact::*, synthesis::*};
    let mut terminal = response();
    terminal["output"][1]["content"][0]["text"] = json!(
        r#"{"artifacts":[{"output":0,"title":"Release checks","data":{"kind":"checklist","value":{"items":[{"text":"Review release notes","completed":false}]}},"evidence":[]}]}"#
    );
    let server = Server::new(vec![
        br#"{"object":"response.input_tokens","input_tokens":100}"#.to_vec(),
        serde_json::to_vec(&terminal).unwrap(),
    ]);
    let transport = server.transport();
    let model = crate::OpenAiWorkSynthesizer::try_new(
        transport.clone(),
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-only-key".into(),
        )
        .unwrap(),
        config(),
    )
    .unwrap();
    let input = synthesis_disclosure(8000);
    let result = model.produce(&input).await.unwrap();
    assert_eq!(result.usage.model_tokens, 300);
    assert_eq!(result.usage.operations, 1);
    let artifacts = input.resolve(result.outputs).unwrap();
    assert_eq!(artifacts[0].output, "checklist");
    assert!(
        matches!(&artifacts[0].data, WorkArtifactDataV1::Checklist { items } if items.len() == 1 && !items[0].completed)
    );
    let captured = server.thread.join().unwrap();
    assert_eq!(captured.len(), 2);
    assert!(captured[0].0.contains("/responses/input_tokens"));
    let mut generation = captured[1].1.clone();
    assert_eq!(generation["text"]["format"]["name"], "work_synthesis");
    assert_eq!(generation["tools"], json!([]));
    assert_eq!(generation["store"], false);
    for field in [
        "max_output_tokens",
        "reasoning",
        "service_tier",
        "stream",
        "store",
    ] {
        generation.as_object_mut().unwrap().remove(field);
    }
    assert_eq!(captured[0].1, generation);
    assert_eq!(transport.snapshot().unwrap().active_attempts(), 0);
    assert!(!transport.snapshot().unwrap().is_sealed());
}

#[tokio::test]
async fn synthesis_counts_before_refusing_an_insufficient_original_node_budget() {
    use zephium_core::work::{synthesis::*, WorkError};
    let server = Server::new(vec![
        br#"{"object":"response.input_tokens","input_tokens":100}"#.to_vec(),
    ]);
    let transport = server.transport();
    let model = crate::OpenAiWorkSynthesizer::try_new(
        transport.clone(),
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-only-key".into(),
        )
        .unwrap(),
        config(),
    )
    .unwrap();
    assert!(matches!(
        model.produce(&synthesis_disclosure(4100)).await,
        Err(WorkSynthesisError::NotDispatched(WorkError::Capacity))
    ));
    assert_eq!(server.thread.join().unwrap().len(), 1);
    assert_eq!(transport.snapshot().unwrap().active_attempts(), 0);
    assert!(!transport.snapshot().unwrap().is_sealed());
}
#[tokio::test]
async fn planning_transport_counts_identical_input_before_one_generation_and_drains() {
    let server = Server::new(vec![
        br#"{"object":"response.input_tokens","input_tokens":100}"#.to_vec(),
        serde_json::to_vec(&response()).unwrap(),
    ]);
    let transport = server.transport();
    let planner = planner(transport.clone());
    let admitted_body = encode(&planner.request(&disclosure()).unwrap()).unwrap();
    assert_eq!(transport.snapshot().unwrap().active_attempts(), 0);
    let result = planner.propose(disclosure()).await.unwrap();
    assert!(matches!(
        result.proposal,
        WorkPlanningProposal::Clarify { .. }
    ));
    let captured = server.thread.join().unwrap();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[1].2, admitted_body);
    assert!(captured[0].0.contains("/responses/input_tokens"));
    let mut generation = captured[1].1.clone();
    assert_eq!(generation["store"], false);
    assert_eq!(generation["stream"], false);
    assert_eq!(generation["tools"], json!([]));
    assert_eq!(generation["tool_choice"], "none");
    assert_eq!(generation["parallel_tool_calls"], false);
    assert_eq!(generation["truncation"], "disabled");
    assert!(generation.get("include").is_none());
    assert!(generation.get("previous_response_id").is_none());
    assert_eq!(generation["instructions"], INSTRUCTIONS);
    assert_eq!(generation["input"][0]["role"], "user");
    assert_eq!(generation["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(
        generation["input"][0]["content"][0]["text"],
        serde_json::to_string(&serde_json::to_value(disclosure().context()).unwrap()).unwrap()
    );
    for field in [
        "max_output_tokens",
        "reasoning",
        "service_tier",
        "stream",
        "store",
    ] {
        generation.as_object_mut().unwrap().remove(field);
    }
    assert_eq!(captured[0].1, generation);
    assert_eq!(transport.snapshot().unwrap().active_attempts(), 0);
    assert!(!transport.snapshot().unwrap().is_sealed());
    transport.seal();
    assert!(matches!(
        planner.propose(disclosure()).await,
        Err(WorkPlanningError::Unavailable)
    ));
}
#[tokio::test]
async fn planning_transport_refuses_budget_before_generation_and_seals_unknown_outcomes() {
    let server = Server::new(vec![
        br#"{"object":"response.input_tokens","input_tokens":8193}"#.to_vec(),
    ]);
    let transport = server.transport();
    assert!(matches!(
        planner(transport.clone()).propose(disclosure()).await,
        Err(WorkPlanningError::Capacity)
    ));
    assert_eq!(server.thread.join().unwrap().len(), 1);
    assert!(!transport.snapshot().unwrap().is_sealed());
    let server = Server::new(vec![
        br#"{"object":"response.input_tokens","input_tokens":100}"#.to_vec(),
        b"not-json".to_vec(),
    ]);
    let transport = server.transport();
    assert!(matches!(
        planner(transport.clone()).propose(disclosure()).await,
        Err(WorkPlanningError::ProviderOutcomeUnknown)
    ));
    assert_eq!(server.thread.join().unwrap().len(), 2);
    assert!(transport.snapshot().unwrap().is_sealed());
    assert_eq!(transport.snapshot().unwrap().active_attempts(), 0);
}
#[tokio::test]
async fn planning_drop_cancels_dispatched_generation_and_releases_shared_slot() {
    let server = Server::with_hold(
        vec![
            br#"{"object":"response.input_tokens","input_tokens":100}"#.to_vec(),
            serde_json::to_vec(&response()).unwrap(),
        ],
        true,
    );
    let transport = server.transport();
    let planner = planner(transport.clone());
    let mut future = planner.propose(disclosure());
    tokio::time::timeout(Duration::from_secs(180),async {
            while server.seen.load(Ordering::SeqCst)<2 {
                tokio::select! { _ = &mut future => panic!("generation settled before release"), _ = tokio::time::sleep(Duration::from_millis(1)) => {} }
            }
        }).await.unwrap();
    assert_eq!(transport.snapshot().unwrap().active_attempts(), 1);
    drop(future);
    assert_eq!(transport.snapshot().unwrap().active_attempts(), 0);
    assert!(transport.snapshot().unwrap().is_sealed());
    server.hold.store(false, Ordering::SeqCst);
    assert_eq!(server.thread.join().unwrap().len(), 2);
}

#[tokio::test]
async fn rig_mapped_generation_retains_response_byte_ceiling_and_unknown_settlement() {
    let server = Server::new(vec![
        br#"{"object":"response.input_tokens","input_tokens":100}"#.to_vec(),
        vec![b' '; MAX_BODY as usize + 1],
    ]);
    let transport = server.transport();
    assert!(matches!(
        planner(transport.clone()).propose(disclosure()).await,
        Err(WorkPlanningError::ProviderOutcomeUnknown)
    ));
    assert_eq!(server.thread.join().unwrap().len(), 2);
    assert!(transport.snapshot().unwrap().is_sealed());
    assert_eq!(transport.snapshot().unwrap().active_attempts(), 0);
}

#[tokio::test]
async fn planning_shares_the_existing_four_slot_admission_ceiling() {
    let transport =
        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD).unwrap();
    let slots: Vec<_> = (0..4)
        .map(|_| {
            transport
                .reserve_key(TransportSlotKey::Planning(ulid::Ulid::new()))
                .ok()
                .unwrap()
        })
        .collect();
    assert!(matches!(
        planner(transport.clone()).propose(disclosure()).await,
        Err(WorkPlanningError::Capacity)
    ));
    drop(slots);
    assert_eq!(transport.snapshot().unwrap().active_attempts(), 0);
}
#[test]
fn planning_request_rejects_secret_shaped_content_without_disclosure() {
    let planner =
        planner(AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD).unwrap());
    let work = zephium_core::work::WorkSnapshot::create(
        1.into(),
        2.into(),
        "Please use sk-development-secret-value".into(),
    )
    .unwrap();
    assert!(matches!(
        planner.request(&WorkPlanningDisclosure::from_snapshot(&work).unwrap()),
        Err(WorkPlanningError::Privacy)
    ));
}

#[tokio::test]
async fn synthesis_count_refusal_reports_closed_numeric_diagnostic_without_generation() {
    use zephium_core::work::{synthesis::*, WorkError};
    static DIAGNOSTIC: std::sync::Mutex<Option<WorkSynthesisDiagnostic>> =
        std::sync::Mutex::new(None);
    let server = Server::new(vec![
        br#"{"object":"response.input_tokens","input_tokens":8193}"#.to_vec(),
    ]);
    let transport = server.transport();
    let model = crate::OpenAiWorkSynthesizer::try_new(
        transport.clone(),
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-only-key".into(),
        )
        .unwrap(),
        config(),
    )
    .unwrap()
    .with_diagnostic(|event| *DIAGNOSTIC.lock().unwrap() = Some(event));
    assert!(matches!(
        model.produce(&synthesis_disclosure(20000)).await,
        Err(WorkSynthesisError::NotDispatched(WorkError::Capacity))
    ));
    let Some(WorkSynthesisDiagnostic::InputCounted {
        tokens,
        maximum,
        request_bytes,
    }) = *DIAGNOSTIC.lock().unwrap()
    else {
        panic!("missing counted admission diagnostic")
    };
    assert_eq!((tokens, maximum), (8193, 8192));
    assert!(request_bytes > 0 && request_bytes <= MAX_REQUEST);
    assert_eq!(server.thread.join().unwrap().len(), 1);
    assert!(!transport.snapshot().unwrap().is_sealed());
}

#[tokio::test]
async fn synthesis_phase_accepts_8529_count_only_within_original_attempt_budget() {
    use zephium_core::work::{synthesis::*, WorkError};
    let count = br#"{"object":"response.input_tokens","input_tokens":8529}"#.to_vec();
    let server = Server::new(vec![count.clone()]);
    assert!(matches!(
        planner(server.transport()).propose(disclosure()).await,
        Err(WorkPlanningError::Capacity)
    ));
    assert_eq!(server.thread.join().unwrap().len(), 1);
    for enough in [false, true] {
        let mut terminal = response();
        terminal["usage"]["input_tokens"] = json!(8529);
        terminal["usage"]["total_tokens"] = json!(8729);
        terminal["output"][1]["content"][0]["text"] = json!(
            r#"{"artifacts":[{"output":0,"title":"Release checks","data":{"kind":"checklist","value":{"items":[{"text":"Review release notes","completed":false}]}},"evidence":[]}]}"#
        );
        let responses = if enough {
            vec![count.clone(), serde_json::to_vec(&terminal).unwrap()]
        } else {
            vec![count.clone()]
        };
        let server = Server::new(responses);
        let transport = server.transport();
        let phase = WorkPlanningConfig::try_new(config().call, 32768, 100000).unwrap();
        let model = crate::OpenAiWorkSynthesizer::try_new(
            transport.clone(),
            AgentProviderCredential::try_new(
                AgentProviderKind::OpenAiResponses,
                "fixture-only-key".into(),
            )
            .unwrap(),
            phase,
        )
        .unwrap();
        let result = model
            .produce(&synthesis_disclosure(if enough {
                147456
            } else {
                8529 + 4096 - 1
            }))
            .await;
        if enough {
            assert_eq!(result.unwrap().usage.model_tokens, 8729);
        } else {
            assert!(matches!(
                result,
                Err(WorkSynthesisError::NotDispatched(WorkError::Capacity))
            ));
        }
        let captured = server.thread.join().unwrap();
        assert_eq!(captured.len(), if enough { 2 } else { 1 });
        assert!(captured[0].0.contains("/responses/input_tokens"));
        if enough {
            assert!(!captured[1].0.contains("/input_tokens"));
        }
        assert!(!transport.snapshot().unwrap().is_sealed());
    }
}

#[tokio::test]
#[cfg(feature = "probe-harness")]
async fn synthesis_trace_is_per_call_opt_in_and_excluded_from_counted_context() {
    use zephium_core::work::synthesis::*;
    for retain in [false, true] {
        let mut terminal = response();
        terminal["output"][1]["content"][0]["text"] = json!(
            r#"{"artifacts":[{"output":0,"title":"Release checks","data":{"kind":"checklist","value":{"items":[{"text":"Review release notes","completed":false}]}},"evidence":[]}]}"#
        );
        let mut responses = vec![];
        for _ in 0..3 {
            responses.push(br#"{"object":"response.input_tokens","input_tokens":100}"#.to_vec());
            responses.push(serde_json::to_vec(&terminal).unwrap());
        }
        let server = Server::new(responses);
        let model = crate::OpenAiWorkSynthesizer::try_new(
            server.transport(),
            AgentProviderCredential::try_new(
                AgentProviderKind::OpenAiResponses,
                "fixture-only-key".into(),
            )
            .unwrap(),
            config(),
        )
        .unwrap();
        let model = if retain {
            model.with_public_response_retention()
        } else {
            model
        };
        let input = synthesis_disclosure(8000);
        let traces = [
            WorkSynthesisTrace {
                work: 1.into(),
                execution: 2.into(),
                attempt: 3.into(),
            },
            WorkSynthesisTrace {
                work: 4.into(),
                execution: 5.into(),
                attempt: 6.into(),
            },
        ];
        for trace in traces {
            model.produce_owned(&input, trace).await.unwrap();
        }
        model.produce(&input).await.unwrap();
        let captured = server.thread.join().unwrap();
        assert_eq!(captured.len(), 6);
        for index in 0..3 {
            let count = &captured[index * 2].1;
            let generation = &captured[index * 2 + 1].1;
            assert!(count.get("metadata").is_none());
            assert_eq!(*count, captured[0].1);
            assert_eq!(generation["store"], retain);
            if retain && index < 2 {
                assert_eq!(generation["metadata"]["work"], json!(traces[index].work));
                assert_eq!(
                    generation["metadata"]["execution"],
                    json!(traces[index].execution)
                );
                assert_eq!(
                    generation["metadata"]["attempt"],
                    json!(traces[index].attempt)
                );
            } else {
                assert!(generation["metadata"].get("work").is_none());
                assert!(generation["metadata"].get("execution").is_none());
                assert!(generation["metadata"].get("attempt").is_none());
            }
        }
    }
}
