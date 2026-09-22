use super::*;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    thread,
};
use zephium_decision::Question;

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
                            assert!(request["model"] == zephium_decision::JEV_MODEL);
                            break;
                        }
                    }
                }
                socket.write_all(response.as_bytes()).unwrap();
                count += 1;
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
    assert_eq!(output.diagnostic.attempts, 2);
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
    assert_eq!(output.diagnostic.attempts, 1);
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
    assert_eq!(output.diagnostic.attempts, 0);
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
