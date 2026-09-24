use super::tests::browser_fixture;
use super::*;
use zephium_agentic::*;

#[tokio::test]
async fn decision_call_joins_the_shared_work_journal() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for count in [true, false] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let (header, length) = loop {
                let mut buffer = [0; 4096];
                let read = stream.read(&mut buffer).unwrap();
                assert_ne!(read, 0);
                bytes.extend_from_slice(&buffer[..read]);
                if let Some(end) = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while bytes.len() < header + length {
                let mut buffer = [0; 4096];
                let read = stream.read(&mut buffer).unwrap();
                assert_ne!(read, 0);
                bytes.extend_from_slice(&buffer[..read]);
            }
            let request: serde_json::Value = serde_json::from_slice(&bytes[header..]).unwrap();
            assert_eq!(request["model"], "gpt-6-luna");
            let body = if count {
                serde_json::json!({"object":"response.input_tokens","input_tokens":100})
            } else {
                let schema = &request["text"]["format"]["schema"]["properties"]["answers"]["properties"];
                let answers: serde_json::Map<_, _> = schema.as_object().unwrap().iter().map(|(key, schema)| {
                    let value = if schema["properties"]["noul"].is_object() {
                        serde_json::json!({"type":"noul","noul":0.01})
                    } else {
                        let probabilities: serde_json::Map<_, _> = schema["properties"]["probabilities"]["properties"].as_object().unwrap().keys()
                            .map(|key| (key.clone(), serde_json::json!(if key == "none" { 1.0 } else { 0.0 }))).collect();
                        serde_json::json!({"type":"choice","choice":"none","confidence":1.0,"probabilities":probabilities})
                    };
                    (key.clone(), value)
                }).collect();
                serde_json::json!({"object":"response","status":"completed","model":"gpt-6-luna","service_tier":"default","error":null,"incomplete_details":null,
                    "output":[{"type":"reasoning","summary":[]},{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":serde_json::json!({"answers":answers}).to_string()}]}],
                    "usage":{"input_tokens":100,"output_tokens":200,"total_tokens":300,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":50}}})
            }.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
    });
    let (mut session, observation) = browser_fixture();
    session.transport = BrowserSessionTransport(
        AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::STANDARD,
            &endpoint,
            &endpoint,
        )
        .unwrap(),
    );
    let events = Arc::new(std::sync::Mutex::new(
        work::WorkEvents::new(session.policy.manifest().run()).unwrap(),
    ));
    let mut journal = work::WorkJournal::new(
        session.policy.manifest(),
        session.lease.node(),
        AgentSupervisorId::new(1).unwrap(),
        AgentSupervisorCancellationId::new(1).unwrap(),
        session.clock.clone(),
        events,
    )
    .unwrap();
    journal
        .start(AgentSupervisorAttemptId::new(1).unwrap())
        .unwrap();
    session.journal = Some(journal);
    session
        .configure_decisions(AgentBrowserDecisionProvider::Emulation)
        .unwrap();
    let authority = AgentProviderActionAuthority::try_new(&observation, &[]).unwrap();
    let mut answers = session
        .decide_observation(&observation, &authority, None, false)
        .await
        .unwrap()
        .unwrap();
    server.join().unwrap();
    assert_eq!(
        answers
            .take_challenge(&observation, session.account)
            .unwrap(),
        Some(false)
    );
    assert_eq!(session.turns, 1);
    assert!(!session.initial_provider_started);
    assert_eq!(session.model_receipts.len(), 1);
    assert_eq!(session.journal_receipts, 1);
    assert_eq!(session.policy.pending_model_calls(), 0);
    assert_eq!(session.policy.accounting().consumed_model_tokens(), 300);
    let journal = session.journal.as_ref().unwrap();
    assert!(journal.failure.is_none());
    assert_eq!(journal.inputs.snapshot().calls(), 1);
    assert_eq!(
        journal
            .inputs
            .snapshot()
            .kind(AgentProviderInputKind::Decision)
            .calls(),
        1
    );
    assert!(session.transport.snapshot().unwrap().is_idle());
    assert!(session.try_finish().is_ok());
}
