use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::sync::Mutex;

use serde_json::{json, Value};

use super::*;

/// Runs one recorded stream through the wire's decoder, split into odd-sized
/// chunks so framing across reads is exercised too.
fn decode(
    wire: WorkModelWire,
    fixture: &str,
) -> (Vec<WorkModelEvent>, Result<Decoded, WorkModelError>) {
    let mut decoder = WireDecoder::new(wire);
    let mut framer = sse::SseFramer::default();
    let mut frames = Vec::new();
    let mut out = Vec::new();
    for chunk in fixture.as_bytes().chunks(37) {
        framer.push(chunk, &mut frames).unwrap();
    }
    framer.finish(&mut frames).unwrap();
    for frame in frames.drain(..) {
        match decoder.event(&frame, &mut out) {
            Ok(Flow::Continue) => {}
            Ok(Flow::Done) => break,
            Err(error) => return (out, Err(error)),
        }
    }
    let decoded = decoder.finish(&mut out);
    (out, decoded)
}

fn sse(events: &[(&str, Value)]) -> String {
    events
        .iter()
        .map(|(name, data)| format!("event: {name}\ndata: {data}\n\n"))
        .collect()
}

pub(super) fn request(wire: WorkModelWire, model: &str) -> WorkModelRequest {
    WorkModelRequest {
        model: WorkModelRef {
            provider: WorkModelProvider::OpenAi,
            wire,
            model: model.into(),
        },
        system: vec![
            WorkModelSystemBlock {
                text: "Core prompt.".into(),
                cache: true,
            },
            WorkModelSystemBlock {
                text: "Work context.".into(),
                cache: false,
            },
        ],
        tools: vec![WorkModelTool {
            name: "web_fetch".into(),
            description: "Read a page.".into(),
            schema: json!({"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}),
        }],
        messages: vec![WorkModelMessage::User(vec![
            WorkModelPart::Text("Plan my trip".into()),
            WorkModelPart::Image {
                media_type: "image/png".into(),
                bytes: vec![1, 2, 3],
            },
        ])],
        max_output_tokens: 4_096,
        reasoning: Some(WorkModelReasoning::Medium),
        native_search: true,
        parallel_tools: true,
    }
}

fn texts(events: &[WorkModelEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            WorkModelEvent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn calls(parts: &[WorkModelPart]) -> Vec<(String, String, Value)> {
    parts
        .iter()
        .filter_map(|part| match part {
            WorkModelPart::ToolCall(call) => {
                Some((call.id.clone(), call.name.clone(), call.arguments.clone()))
            }
            _ => None,
        })
        .collect()
}

type Hits = Vec<(String, String, String)>;

fn searches(events: &[WorkModelEvent]) -> Vec<(String, Hits)> {
    events
        .iter()
        .filter_map(|event| match event {
            WorkModelEvent::Search { query, hits } => Some((
                query.clone(),
                hits.iter()
                    .map(|hit| (hit.url.clone(), hit.title.clone(), hit.snippet.clone()))
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn openai_streams_text_parallel_calls_reasoning_and_search() {
    let reasoning =
        json!({"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAA"});
    let search = json!({"type": "web_search_call", "id": "ws_1", "status": "completed",
        "action": {"type": "search", "query": "YC batch dates 2027",
            "sources": [{"type": "url", "url": "https://www.ycombinator.com/apply"}]}});
    let fixture = sse(&[
        (
            "response.created",
            json!({"type": "response.created", "response": {"status": "in_progress"}}),
        ),
        (
            "response.output_item.done",
            json!({"type": "response.output_item.done", "output_index": 0, "item": reasoning}),
        ),
        (
            "response.output_item.done",
            json!({"type": "response.output_item.done", "output_index": 1, "item": search}),
        ),
        (
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "delta": "Batch starts "}),
        ),
        (
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "delta": "in January."}),
        ),
        (
            "response.output_item.done",
            json!({"type": "response.output_item.done", "output_index": 2, "item": {
            "type": "message", "role": "assistant", "content": [{"type": "output_text",
            "text": "Batch starts in January.", "annotations": [{"type": "url_citation",
            "url": "https://www.ycombinator.com/apply", "title": "Apply to Y Combinator",
            "start_index": 0, "end_index": 24}]}]}}),
        ),
        (
            "response.function_call_arguments.delta",
            json!({"type": "response.function_call_arguments.delta", "delta": "{\"url\""}),
        ),
        (
            "response.output_item.done",
            json!({"type": "response.output_item.done", "output_index": 3, "item": {
            "type": "function_call", "call_id": "call_a", "name": "web_fetch", "arguments": "{\"url\":\"https://a.example\"}"}}),
        ),
        (
            "response.output_item.done",
            json!({"type": "response.output_item.done", "output_index": 4, "item": {
            "type": "function_call", "call_id": "call_b", "name": "web_fetch", "arguments": "{\"url\":\"https://b.example\"}"}}),
        ),
        (
            "response.completed",
            json!({"type": "response.completed", "response": {"status": "completed", "usage": {
            "input_tokens": 12000, "input_tokens_details": {"cached_tokens": 10000},
            "output_tokens": 900, "output_tokens_details": {"reasoning_tokens": 600}}}}),
        ),
    ]);
    let (events, decoded) = decode(WorkModelWire::OpenAiResponses, &fixture);
    let decoded = decoded.unwrap();
    assert_eq!(texts(&events), "Batch starts in January.");
    assert_eq!(
        searches(&events),
        vec![(
            "YC batch dates 2027".to_owned(),
            vec![(
                "https://www.ycombinator.com/apply".to_owned(),
                "Apply to Y Combinator".to_owned(),
                "Batch starts in January.".to_owned()
            )]
        )]
    );
    assert_eq!(decoded.stop, DecodedStop::End(WorkModelStop::ToolUse));
    assert_eq!(decoded.searches, 1);
    assert_eq!(
        calls(&decoded.assistant),
        vec![
            (
                "call_a".into(),
                "web_fetch".into(),
                json!({"url": "https://a.example"})
            ),
            (
                "call_b".into(),
                "web_fetch".into(),
                json!({"url": "https://b.example"})
            ),
        ]
    );
    let tool_events = events
        .iter()
        .filter(|event| matches!(event, WorkModelEvent::ToolCall(_)))
        .count();
    assert_eq!(tool_events, 2);
    assert_eq!(decoded.usage.input_tokens, 12_000);
    assert_eq!(decoded.usage.cached_input_tokens, 10_000);
    assert_eq!(decoded.usage.reasoning_tokens, 600);
    // The reasoning and search items come back first, verbatim, on the next call.
    assert!(matches!(&decoded.assistant[0], part if replayed(part, "openai") == Some(&reasoning)));
    let mut next = request(WorkModelWire::OpenAiResponses, "gpt-6-sol");
    next.messages
        .push(WorkModelMessage::Assistant(decoded.assistant.clone()));
    next.messages.push(WorkModelMessage::ToolResults(vec![
        WorkModelToolResult {
            call: "call_a".into(),
            content: "A".into(),
            is_error: false,
        },
        WorkModelToolResult {
            call: "call_b".into(),
            content: "gone".into(),
            is_error: true,
        },
    ]));
    let target = LeadTarget::direct(WorkModelProvider::OpenAi).unwrap();
    let body = openai::body(&next, &target).unwrap();
    let input = body["input"].as_array().unwrap();
    let kinds: Vec<&str> = input
        .iter()
        .map(|item| item["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "message",
            "reasoning",
            "web_search_call",
            "message",
            "function_call",
            "function_call",
            "function_call_output",
            "function_call_output"
        ]
    );
    assert_eq!(input[1]["encrypted_content"], "gAAA");
    assert_eq!(
        input[0]["content"][1]["image_url"],
        "data:image/png;base64,AQID"
    );
    assert_eq!(input[7]["output"], "Error: gone");
    assert_eq!(body["store"], false);
    assert_eq!(body["instructions"], "Core prompt.\n\nWork context.");
    assert_eq!(body["reasoning"]["effort"], "medium");
    assert_eq!(body["tools"][1]["type"], "web_search");
    assert_eq!(
        body["include"],
        json!([
            "reasoning.encrypted_content",
            "web_search_call.action.sources"
        ])
    );
}

#[test]
fn openai_limits_and_failures_close() {
    let incomplete = sse(&[(
        "response.incomplete",
        json!({"type": "response.incomplete", "response": {"incomplete_details": {"reason": "max_output_tokens"},
            "usage": {"input_tokens": 5, "output_tokens": 9}}}),
    )]);
    let (_, decoded) = decode(WorkModelWire::OpenAiResponses, &incomplete);
    assert_eq!(
        decoded.unwrap().stop,
        DecodedStop::End(WorkModelStop::MaxTokens)
    );

    let failed = sse(&[(
        "response.failed",
        json!({"type": "response.failed", "response": {"error": {"code": "server_error", "message": "secret words"}}}),
    )]);
    let (_, decoded) = decode(WorkModelWire::OpenAiResponses, &failed);
    assert_eq!(decoded.err(), Some(WorkModelError::Overloaded));

    let refused = sse(&[
        (
            "response.output_item.done",
            json!({"type": "response.output_item.done", "item": {
            "type": "message", "content": [{"type": "refusal", "refusal": "no"}]}}),
        ),
        (
            "response.completed",
            json!({"type": "response.completed", "response": {"usage": {}}}),
        ),
    ]);
    let (_, decoded) = decode(WorkModelWire::OpenAiResponses, &refused);
    assert_eq!(
        decoded.unwrap().stop,
        DecodedStop::End(WorkModelStop::Refused)
    );

    let truncated = sse(&[(
        "response.output_text.delta",
        json!({"type": "response.output_text.delta", "delta": "half"}),
    )]);
    let (_, decoded) = decode(WorkModelWire::OpenAiResponses, &truncated);
    assert_eq!(decoded.err(), Some(WorkModelError::Protocol));

    let malformed = sse(&[
        (
            "response.output_item.done",
            json!({"type": "response.output_item.done", "item": {"type": "function_call",
            "call_id": "c", "name": "web_fetch", "arguments": "{\"url\": "}}),
        ),
        (
            "response.completed",
            json!({"type": "response.completed", "response": {}}),
        ),
    ]);
    let (_, decoded) = decode(WorkModelWire::OpenAiResponses, &malformed);
    // The loop's schema check hands the model a fault it can correct.
    assert_eq!(calls(&decoded.unwrap().assistant)[0].2, json!("{\"url\": "));
}

#[test]
fn http_failures_classify_without_provider_text() {
    let body = br#"{"error":{"type":"invalid_request_error","message":"prompt is too long: 1200000 tokens"}}"#;
    assert_eq!(
        classify(StatusCode::BAD_REQUEST, body),
        WorkModelError::ContextTooLong
    );
    assert_eq!(
        classify(StatusCode::BAD_REQUEST, b"{}"),
        WorkModelError::BadRequest
    );
    assert_eq!(
        classify(StatusCode::UNAUTHORIZED, b""),
        WorkModelError::Unauthorized
    );
    assert_eq!(
        classify(StatusCode::from_u16(529).unwrap(), b""),
        WorkModelError::Overloaded
    );
    let quota = br#"{"error":{"code":"insufficient_quota"}}"#;
    assert_eq!(
        classify(StatusCode::TOO_MANY_REQUESTS, quota),
        WorkModelError::OverBudget
    );
    let credit = br#"{"error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the API."}}"#;
    assert_eq!(
        classify(StatusCode::BAD_REQUEST, credit),
        WorkModelError::OverBudget
    );
    assert_eq!(
        stream_error("credit_balance_exhausted"),
        WorkModelError::OverBudget
    );
    let mut headers = HeaderMap::new();
    headers.insert(RETRY_AFTER, HeaderValue::from_static("7"));
    assert_eq!(retry_after(&headers), Some(Duration::from_secs(7)));
    headers.insert("retry-after-ms", HeaderValue::from_static("250"));
    assert_eq!(retry_after(&headers), Some(Duration::from_millis(250)));
}

#[test]
fn cost_follows_the_catalog_price_and_search_fees() {
    let client = LeadClient::new(
        LeadTarget::direct(WorkModelProvider::Anthropic).unwrap(),
        Arc::new(LeadStaticCredential::new(
            LeadSecret::new("k".into()).unwrap(),
        )),
        models::builtin_price(WorkModelProvider::Anthropic, "claude-opus-5-5"),
    );
    let decoded = Decoded {
        usage: WorkModelUsage {
            input_tokens: 1_000_000,
            cached_input_tokens: 500_000,
            output_tokens: 100_000,
            ..Default::default()
        },
        cache_write_tokens: 100_000,
        searches: 2,
        ..Default::default()
    };
    // 400k fresh × $4 + 500k cached × $0.20 + 100k written × $5 + 100k out × $20 + 2 × $0.01
    let expected = 1_600_000 + 100_000 + 500_000 + 2_000_000 + 20_000;
    assert_eq!(client.usage(&decoded).cost_micros, Some(expected));
}

#[test]
fn targets_build_exact_urls_for_direct_and_cloud() {
    let gemini = LeadTarget::direct(WorkModelProvider::Google).unwrap();
    assert_eq!(
        gemini.call_url("gemini-3.8-flash").unwrap().as_str(),
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.8-flash:streamGenerateContent?alt=sse"
    );
    assert!(gemini.call_url("../../x").is_err());
    let cloud = LeadTarget::cloud(ZEPHIUM_CLOUD_BASE, WorkModelProvider::DeepSeek).unwrap();
    assert_eq!(
        cloud.call_url("deepseek-flash").unwrap().as_str(),
        "https://api.zephium.app/deepseek/v1/chat/completions"
    );
    let cloud = LeadTarget::cloud(ZEPHIUM_CLOUD_BASE, WorkModelProvider::OpenAi).unwrap();
    assert_eq!(
        cloud.call_url("gpt-6-sol").unwrap().as_str(),
        "https://api.zephium.app/openai/v1/responses"
    );
    assert!(LeadTarget::compatible("http://example.com/v1").is_err());
    assert!(LeadTarget::compatible("http://localhost:11434/v1").is_ok());
    assert!(LeadTarget::compatible("https://user:pw@example.com/v1").is_err());
    let mut headers = HeaderMap::new();
    cloud
        .authorize(&mut headers, &LeadSecret::new("tok".into()).unwrap())
        .unwrap();
    assert_eq!(headers["authorization"], "Bearer tok");
    assert!(headers["authorization"].is_sensitive());
}

/// A loopback server answering each connection with the next canned reply.
fn serve(replies: Vec<String>) -> (String, std::sync::Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/v1", listener.local_addr().unwrap());
    let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for reply in replies {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 65_536];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if body.len() >= length {
                        log.lock().unwrap().push(text);
                        break;
                    }
                }
            }
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    (address, seen)
}

fn http_reply(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n{headers}\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn a_rate_limited_call_retries_after_the_hint_then_streams() {
    let stream = sse(&[
        (
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "delta": "Hi"}),
        ),
        (
            "response.output_item.done",
            json!({"type": "response.output_item.done", "item": {
            "type": "message", "content": [{"type": "output_text", "text": "Hi", "annotations": []}]}}),
        ),
        (
            "response.completed",
            json!({"type": "response.completed", "response": {"usage": {
            "input_tokens": 1000, "output_tokens": 10}}}),
        ),
    ]);
    let (base, seen) = serve(vec![
        http_reply(
            "429 Too Many Requests",
            "retry-after: 1\r\n",
            "{\"error\":{\"message\":\"slow\"}}",
        ),
        http_reply("200 OK", "content-type: text/event-stream\r\n", &stream),
    ]);
    let target = LeadTarget {
        wire: WorkModelWire::OpenAiResponses,
        upstream: WorkModelProvider::OpenAi,
        base: Url::parse(&base).unwrap(),
        cloud: false,
    };
    let client = LeadClient::new(
        target,
        Arc::new(LeadStaticCredential::new(
            LeadSecret::new("sk-test".into()).unwrap(),
        )),
        models::builtin_price(WorkModelProvider::OpenAi, "gpt-6-sol"),
    );
    let events = Mutex::new(Vec::new());
    let sink = |event: WorkModelEvent| {
        if let WorkModelEvent::Text(text) = event {
            events.lock().unwrap().push(text);
        }
    };
    let outcome = client
        .call(request(WorkModelWire::OpenAiResponses, "gpt-6-sol"), &sink)
        .await
        .unwrap();
    assert_eq!(outcome.stop, WorkModelStop::EndTurn);
    assert_eq!(*events.lock().unwrap(), vec!["Hi".to_owned()]);
    // 1,000 in × $2 + 10 out × $10, rounded up to whole micro-dollars.
    assert_eq!(outcome.usage.cost_micros, Some(2_100));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(seen[1].starts_with("POST /v1/responses "));
    assert!(seen[1]
        .to_ascii_lowercase()
        .contains("authorization: bearer sk-test"));
}

#[tokio::test]
async fn an_unauthorized_call_fails_closed_without_retrying() {
    let (base, seen) = serve(vec![http_reply("401 Unauthorized", "", "{}")]);
    let target = LeadTarget {
        wire: WorkModelWire::OpenAiResponses,
        upstream: WorkModelProvider::OpenAi,
        base: Url::parse(&base).unwrap(),
        cloud: false,
    };
    let client = LeadClient::new(
        target,
        Arc::new(LeadStaticCredential::new(
            LeadSecret::new("sk-test".into()).unwrap(),
        )),
        None,
    );
    let error = client
        .call(
            request(WorkModelWire::OpenAiResponses, "gpt-6-sol"),
            &|_| {},
        )
        .await
        .err();
    assert_eq!(error, Some(WorkModelError::Unauthorized));
    assert_eq!(seen.lock().unwrap().len(), 1);
}
