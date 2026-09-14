use super::*;
use crate::{
    AgentProviderModelRevision, AgentProviderPricingProfile, AgentProviderPricingRevision,
    AgentProviderReasoningEffort, AgentProviderStreamBudget, SemanticTokenizerRevision,
};
use serde_json::json;
fn config() -> OpenAiPublicSearchConfig {
    config_for("gpt-4.1-mini")
}
fn config_for(model: &str) -> OpenAiPublicSearchConfig {
    OpenAiPublicSearchConfig::try_new(
        AgentProviderCallConfig::try_for_test(
            AgentProviderKind::OpenAiResponses,
            AgentProviderModelRevision::try_new(model.into()).unwrap(),
            AgentProviderReasoningEffort::Medium,
            SemanticTokenizerRevision::try_new("search-fixture:v1".into()).unwrap(),
            AgentProviderPricingProfile::try_new(
                AgentProviderPricingRevision::new(1).unwrap(),
                (SEARCH_CONTEXT_TOKENS + MAX_REQUEST_BYTES).into(),
            )
            .unwrap(),
            1,
            4096,
            AgentProviderStreamBudget::STANDARD,
        )
        .unwrap(),
    )
    .unwrap()
}
fn limits() -> WorkExecutionLimits {
    WorkExecutionLimits {
        model_tokens: 300_000,
        cost_micro_usd: 1_000_000,
        operations: 1,
        timeout_seconds: 30,
        max_workers: 1,
    }
}
fn response() -> Value {
    json!({"id":"resp_public","object":"response","status":"completed","model":"gpt-4.1-mini","service_tier":"default","error":null,"incomplete_details":null,
"output":[{"type":"web_search_call","id":"ws_public","status":"completed","action":{"type":"search","queries":["public query"]}},{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Public answer [1].","annotations":[{"type":"url_citation","url":"https://example.com/source","title":"Public source","start_index":14,"end_index":17}]}]}],
"usage":{"input_tokens":9000,"output_tokens":100,"total_tokens":9100,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}})
}
#[test]
fn request_contains_only_public_query_and_closed_native_search_controls() {
    let b = request(&config(), "public query", &[]).unwrap();
    let b: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(b["input"][0]["content"][0]["text"], "public query");
    assert_eq!(
        b["tools"],
        json!([{"type":"web_search","search_context_size":"medium"}])
    );
    assert_eq!(b["max_tool_calls"], 1);
    assert_eq!(b["tool_choice"], "required");
    for key in ["store", "stream", "background", "parallel_tool_calls"] {
        assert_eq!(b[key], false);
    }
    for key in [
        "previous_response_id",
        "conversation",
        "text",
        "metadata",
        "include",
        "reasoning",
    ] {
        assert!(b.get(key).is_none(), "{key}");
    }
    for query in ["", "\nprivate", &"x".repeat(513)] {
        assert!(request(&config(), query, &[]).is_err());
    }
}
#[test]
fn citations_and_fee_are_distinct_from_extraction_and_tokens() {
    let cfg = config();
    let got = decode(&serde_json::to_vec(&response()).unwrap(), &cfg, 1000)
        .unwrap()
        .unwrap();
    assert_eq!(got.response_id, "resp_public");
    assert_eq!(got.search_call_id, "ws_public");
    assert_eq!(got.citations[0].url, "https://example.com/source");
    assert_eq!(got.usage.model_tokens, 9100);
    assert_eq!(
        u64::from(got.usage.cost_micro_usd),
        cfg.call.planning_cost_ceiling(9000, 100).unwrap() + 10_000
    );
    assert!(got.usage.within(limits()));
}
#[test]
fn malformed_identity_tools_and_usage_cannot_become_evidence() {
    for (path, value) in [
        ("/model", json!("different")),
        ("/status", json!("incomplete")),
        (
            "/usage/input_tokens",
            json!(SEARCH_CONTEXT_TOKENS + MAX_REQUEST_BYTES + 1),
        ),
        ("/usage/total_tokens", json!(1)),
        ("/output/0/action/type", json!("open_page")),
        ("/output/1/role", json!("user")),
    ] {
        let mut r = response();
        *r.pointer_mut(path).unwrap() = value;
        assert!(
            decode(&serde_json::to_vec(&r).unwrap(), &config(), 1000).is_none(),
            "{path}"
        );
    }
    let mut r = response();
    let search = r["output"][0].clone();
    r["output"].as_array_mut().unwrap().insert(0, search);
    assert!(decode(&serde_json::to_vec(&r).unwrap(), &config(), 1000).is_none());
    assert!(decode(&vec![b' '; MAX_BODY as usize + 1], &config(), 1000).is_none());
}
#[test]
fn refused_search_retains_verified_fee_and_no_search_refusal_has_no_fee() {
    for searched in [true, false] {
        let mut r = response();
        r["output"][1]["content"] = json!([{"type":"refusal"}]);
        if !searched {
            r["output"].as_array_mut().unwrap().remove(0);
            r["usage"]["input_tokens"] = json!(1000);
            r["usage"]["total_tokens"] = json!(1100);
        }
        let Err(WorkPublicSearchError::Rejected(usage)) =
            decode(&serde_json::to_vec(&r).unwrap(), &config(), 1000).unwrap()
        else {
            panic!("not refusal")
        };
        assert_eq!(
            u64::from(usage.cost_micro_usd),
            config()
                .call
                .planning_cost_ceiling(if searched { 9000 } else { 1000 }, 100)
                .unwrap()
                + if searched { 10000 } else { 0 }
        );
    }
}
fn adapter(endpoint: &str) -> OpenAiPublicSearch {
    let transport = AgentProviderTransport::try_new_loopback(
        AgentProviderTransportConfig::try_new(
            Duration::from_secs(30),
            Duration::from_secs(10),
            Duration::from_secs(20),
        )
        .unwrap(),
        endpoint,
        &endpoint.replace("responses", "messages"),
    )
    .unwrap();
    OpenAiPublicSearch::try_new(
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
#[tokio::test]
async fn insufficient_budget_is_not_dispatched() {
    let provider = adapter("http://127.0.0.1:9/v1/responses");
    let mut low = limits();
    low.model_tokens = 1000;
    assert!(matches!(
        provider.search("public query", &[], low).await,
        Err(WorkPublicSearchError::NotDispatched(WorkError::Capacity))
    ));
    assert!(!provider.transport.shared.shutdown.is_cancelled());
}
#[tokio::test]
async fn dispatched_transport_failure_is_unknown_and_seals_transport() {
    let provider = adapter("http://127.0.0.1:9/v1/responses");
    assert!(matches!(
        provider.search("public query", &[], limits()).await,
        Err(WorkPublicSearchError::OutcomeUnknown)
    ));
    assert!(provider.transport.shared.shutdown.is_cancelled());
}
fn server(
    hold: bool,
) -> (
    String,
    std::thread::JoinHandle<Vec<u8>>,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::Sender<()>,
) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        let (end, len) = loop {
            let n = socket.read(&mut buffer).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&bytes[..end]);
                let len = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(|s| s.parse::<usize>().unwrap())
                    })
                    .unwrap();
                break (end + 4, len);
            }
        };
        while bytes.len() < end + len {
            let n = socket.read(&mut buffer).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
        }
        seen_tx.send(()).unwrap();
        if hold {
            release_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        }
        let response = serde_json::to_vec(&response()).unwrap();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        let _ = socket.write_all(head.as_bytes());
        let _ = socket.write_all(&response);
        bytes[end..end + len].to_vec()
    });
    (endpoint, thread, seen_rx, release_tx)
}
#[tokio::test]
async fn exact_frozen_public_bytes_reach_fixed_transport_and_citations_return() {
    let (endpoint, server, _seen, _release) = server(false);
    let provider = adapter(&endpoint);
    let expected = request(&provider.config, "public query", &[]).unwrap();
    let result = provider
        .search("public query", &[], limits())
        .await
        .unwrap();
    assert_eq!(server.join().unwrap(), expected);
    assert_eq!(result.citations.len(), 1);
    assert!(!provider.transport.shared.shutdown.is_cancelled());
}
#[tokio::test]
async fn dropping_after_dispatch_seals_instead_of_refunding() {
    let (endpoint, server, seen, release) = server(true);
    let provider = Arc::new(adapter(&endpoint));
    let task_provider = provider.clone();
    let task =
        tokio::spawn(async move { task_provider.search("public query", &[], limits()).await });
    let deadline = Instant::now() + Duration::from_secs(30);
    while seen.try_recv().is_err() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert!(provider.transport.shared.shutdown.is_cancelled());
    release.send(()).unwrap();
    server.join().unwrap();
}

#[test]
fn actual_context_and_fixed_billed_content_have_separate_reservations() {
    let config = config();
    let bytes = request(&config, "public query", &[]).unwrap().len() as u32;
    let reserve = config.reservation(bytes).unwrap();
    assert_eq!(
        reserve.model_tokens,
        SEARCH_CONTEXT_TOKENS + bytes + config.call.max_output_tokens()
    );
    assert_eq!(
        u64::from(reserve.cost_micro_usd),
        config
            .call
            .planning_cost_ceiling(bytes + 8000, config.call.max_output_tokens())
            .unwrap()
            + 10000
    );
    assert!(config.reservation(MAX_REQUEST_BYTES + 1).is_err());
    let mut actual = response();
    actual["usage"]["input_tokens"] = json!(100000);
    actual["usage"]["total_tokens"] = json!(100100);
    let result = decode(&serde_json::to_vec(&actual).unwrap(), &config, bytes)
        .unwrap()
        .unwrap();
    assert_eq!(result.usage.model_tokens, 100100);
    assert_eq!(result.actual_input_tokens, 100000);
    assert!(result.usage.cost_micro_usd <= reserve.cost_micro_usd);
    actual["usage"]["output_tokens_details"]["reasoning_tokens"] = json!(1);
    assert!(decode(&serde_json::to_vec(&actual).unwrap(), &config, bytes).is_none());
}

#[tokio::test]
async fn typed_scope_yields_valid_durable_provider_evidence() {
    let (endpoint, server, _seen, _release) = server(false);
    let provider = adapter(&endpoint);
    let scope = WorkPublicSearchScope {
        provider: WorkSearchProvider::OpenAi,
        model: "gpt-4.1-mini".into(),
        query: "public query".into(),
    };
    let result = WorkPublicSearchProvider::search(&provider, &scope, &[], limits())
        .await
        .unwrap();
    result.evidence.validate().unwrap();
    assert_eq!(
        result.evidence.actual_input_tokens + result.evidence.actual_output_tokens,
        result.usage.model_tokens
    );
    assert_eq!(result.evidence.search_call_id, "ws_public");
    server.join().unwrap();
}

#[cfg(feature = "probe-harness")]
#[test]
fn public_qualification_retention_is_opt_in_and_counted_in_exact_body() {
    let provider = adapter("http://127.0.0.1:9/v1/responses");
    let ordinary = request(&provider.config, "public query", &[]).unwrap();
    let work = zephium_core::work::WorkId::generate();
    let execution = zephium_core::work::WorkExecutionId::generate();
    let attempt = zephium_core::work::WorkAttemptId::generate();
    let provider = provider.with_public_work_trace(work, execution, attempt);
    assert_eq!(
        request(&provider.config, "public query", &[]).unwrap(),
        ordinary
    );
    let provider = provider.with_public_response_retention();
    let retained = request(&provider.config, "public query", &[]).unwrap();
    let body: Value = serde_json::from_slice(&retained).unwrap();
    assert_eq!(body["store"], true);
    assert_eq!(body["metadata"]["phase"], "public_search");
    assert_eq!(body["metadata"]["work"], work.to_string());
    assert_eq!(body["metadata"]["execution"], execution.to_string());
    assert_eq!(body["metadata"]["attempt"], attempt.to_string());
    assert!(retained.len() > ordinary.len());
    assert!(
        provider
            .config
            .reservation(retained.len() as u32)
            .unwrap()
            .model_tokens
            > provider
                .config
                .reservation(ordinary.len() as u32)
                .unwrap()
                .model_tokens
    );
}

#[test]
fn unusable_answers_settle_only_verified_search_usage() {
    for (path, value) in [
        ("/output/1/content/0/text", json!("  ")),
        ("/output/1/content/0/text", json!("Public answer [1].\r")),
        ("/output/1/content/0/text", json!("Public answer [1].\0")),
        (
            "/output/1/content/0/text",
            json!("Public answer [1].\u{7f}"),
        ),
        ("/output/1/content/0/text", json!("x".repeat(32769))),
        ("/output/1/content/0/annotations", json!([])),
        (
            "/output/1/content/0/annotations/0",
            json!({"type":"url_citation"}),
        ),
        (
            "/output/1/content/0/annotations/0",
            json!({"type":"unknown_annotation"}),
        ),
        (
            "/output/1/content/0/annotations/0/url",
            json!("file:///private"),
        ),
        ("/output/1/content/0/annotations/0/url", json!("not a URL")),
        ("/output/1/content/0/annotations/0/title", json!("")),
        ("/output/1/content/0/annotations/0/end_index", json!(999)),
    ] {
        let mut r = response();
        *r.pointer_mut(path).unwrap() = value;
        let Err(WorkPublicSearchError::Rejected(usage)) =
            decode(&serde_json::to_vec(&r).unwrap(), &config(), 1000).unwrap()
        else {
            panic!("unusable answer accepted: {path}")
        };
        assert_eq!(usage.model_tokens, 9100);
        assert_eq!(
            usage.accounting,
            WorkUsageAccounting::ConservativeReservation
        );
        assert_eq!(
            u64::from(usage.cost_micro_usd),
            config().call.planning_cost_ceiling(9000, 100).unwrap() + 10000
        );

        // An unusable answer must not hide uncertain accounting or extra tools.
        let mut uncertain = r.clone();
        uncertain["usage"]["total_tokens"] = json!(1);
        assert!(decode(&serde_json::to_vec(&uncertain).unwrap(), &config(), 1000).is_none());
        let extra = r["output"][0].clone();
        r["output"].as_array_mut().unwrap().push(extra);
        assert!(decode(&serde_json::to_vec(&r).unwrap(), &config(), 1000).is_none());
    }
}

#[test]
fn answer_newline_and_tab_remain_usable() {
    let mut r = response();
    r["output"][1]["content"][0]["text"] = json!("Public answer [1].\n\t");
    assert!(decode(&serde_json::to_vec(&r).unwrap(), &config(), 1000)
        .unwrap()
        .is_ok());
}

#[test]
fn luna_search_preserves_reasoning_and_prices_actual_input_with_one_tool_fee() {
    let cfg = config_for("gpt-5.6-luna");
    let body: Value = serde_json::from_slice(&request(&cfg, "public query", &[]).unwrap()).unwrap();
    assert_eq!(body["model"], "gpt-5.6-luna");
    assert_eq!(body["reasoning"]["effort"], "medium");
    assert_eq!(body["max_tool_calls"], 1);
    assert_eq!(body["tools"].as_array().unwrap().len(), 1);
    let reserved = cfg.reservation(1000).unwrap();
    assert_eq!(reserved.model_tokens, SEARCH_CONTEXT_TOKENS + 1000 + 4096);
    assert_eq!(
        u64::from(reserved.cost_micro_usd),
        cfg.call
            .planning_cost_ceiling(SEARCH_CONTEXT_TOKENS + 1000, 4096)
            .unwrap()
            + 10000
    );
    let mut r = response();
    r["model"] = json!("gpt-5.6-luna");
    r["usage"]["output_tokens_details"]["reasoning_tokens"] = json!(50);
    r["output"]
        .as_array_mut()
        .unwrap()
        .insert(0, json!({"type":"reasoning","id":"rs_test","summary":[]}));
    let result = decode(&serde_json::to_vec(&r).unwrap(), &cfg, 1000)
        .unwrap()
        .unwrap();
    assert_eq!(result.response_model, "gpt-5.6-luna");
    assert_eq!(result.usage.model_tokens, 9100);
    assert_eq!(
        u64::from(result.usage.cost_micro_usd),
        cfg.call.planning_cost_ceiling(9000, 100).unwrap() + 10000
    );
    assert!(decode(&serde_json::to_vec(&r).unwrap(), &config(), 1000).is_none());
    r["usage"]["output_tokens_details"]["reasoning_tokens"] = json!(101);
    assert!(decode(&serde_json::to_vec(&r).unwrap(), &cfg, 1000).is_none());
    r["usage"]["output_tokens_details"]["reasoning_tokens"] = json!(50);
    let extra = r["output"][1].clone();
    r["output"].as_array_mut().unwrap().insert(2, extra);
    assert!(decode(&serde_json::to_vec(&r).unwrap(), &cfg, 1000).is_none());
}

#[test]
fn unicode_query_boundary_matches_core_and_preserves_exact_serialized_input() {
    for character in ['a', '—', '界', '😀', '"', '\\'] {
        let query = character.to_string().repeat(PUBLIC_SEARCH_MAX_QUERY_CHARS);
        assert!(validate_public_search_query(&query).is_ok());
        let cfg = config();
        let bytes = request(&cfg, &query, &[]).unwrap();
        assert!(bytes.len() <= MAX_REQUEST_BYTES as usize);
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["input"][0]["content"][0]["text"], query);
        assert!(cfg.reservation(bytes.len() as u32).is_ok());
        let over = format!("{query}{character}");
        assert!(validate_public_search_query(&over).is_err());
        assert!(request(&cfg, &over, &[]).is_err());
    }
}
