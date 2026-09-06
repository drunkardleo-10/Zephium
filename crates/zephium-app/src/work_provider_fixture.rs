//! Shared loopback-only wire fixture; no provider/kernel closure is fabricated.
use std::io::{Read, Write};
use std::time::{Duration, Instant};
use zephium_agent_provider_transport::{AgentProviderTransport, AgentProviderTransportConfig};

pub(crate) fn fixture_provider_responses(
    responses: Vec<String>,
) -> (AgentProviderTransport, std::thread::JoinHandle<usize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut turns = 0;
        for _ in 0..responses.len() * 2 {
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "bounded fixture listener");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => panic!("fixture listener"),
                }
            };
            // Darwin inherits the listener's nonblocking flag at accept.
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut part = [0; 4096];
            loop {
                let read = socket.read(&mut part).unwrap();
                assert!(read > 0);
                request.extend_from_slice(&part[..read]);
                assert!(request.len() < 128 * 1024);
                if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&request[..header_end]).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|value| value.parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if request.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            let count = request.starts_with(b"POST /v1/responses/input_tokens ");
            let (kind, body) = if count {
                (
                    "application/json",
                    "{\"object\":\"response.input_tokens\",\"input_tokens\":17}".to_owned(),
                )
            } else {
                turns += 1;
                ("text/event-stream", responses[turns - 1].clone())
            };
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        turns
    });
    (
        AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::STANDARD,
            &endpoint,
            "http://127.0.0.1:9/v1/messages",
        )
        .unwrap(),
        server,
    )
}

pub(crate) fn response_stream(turn: usize) -> String {
    let created = format!(
        r#"{{"type":"response.created","response":{{"id":"resp_{turn}","status":"in_progress","model":"gpt-5.6-luna","service_tier":"default"}}}}"#
    );
    let mut events = vec![("response.created", created)];
    let terminal_item = if turn == 1 {
        let item = r#"{"type":"function_call","id":"fc_1","call_id":"call_1","name":"extract","arguments":"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}","status":"completed"}"#;
        events.push(("response.output_item.added", r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"extract","arguments":"","status":"in_progress"}}"#.into()));
        events.push(("response.function_call_arguments.delta", r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"}"#.into()));
        events.push(("response.function_call_arguments.done", r#"{"type":"response.function_call_arguments.done","item_id":"fc_1","output_index":0,"arguments":"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"}"#.into()));
        events.push((
            "response.output_item.done",
            format!(r#"{{"type":"response.output_item.done","output_index":0,"item":{item}}}"#),
        ));
        item.to_owned()
    } else {
        let output = r#"{\"v\":1,\"schema\":1,\"fields\":[{\"name\":\"label\",\"value\":{\"k\":\"text\",\"value\":\"Fixture result\",\"sources\":[\"@r1\"]}}]}"#;
        events.push(("response.output_item.added", r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg_2","status":"in_progress","role":"assistant"}}"#.into()));
        events.push(("response.content_part.added", r#"{"type":"response.content_part.added","item_id":"msg_2","output_index":0,"content_index":0,"part":{"type":"output_text"}}"#.into()));
        events.push(("response.output_text.delta", format!(r#"{{"type":"response.output_text.delta","item_id":"msg_2","output_index":0,"content_index":0,"delta":"{output}"}}"#)));
        events.push(("response.output_text.done", format!(r#"{{"type":"response.output_text.done","item_id":"msg_2","output_index":0,"content_index":0,"text":"{output}"}}"#)));
        events.push(("response.content_part.done", r#"{"type":"response.content_part.done","item_id":"msg_2","output_index":0,"content_index":0,"part":{"type":"output_text"}}"#.into()));
        events.push(("response.output_item.done", r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"message","id":"msg_2","status":"completed","role":"assistant"}}"#.into()));
        r#"{"type":"message","id":"msg_2","status":"completed","role":"assistant","content":[{"type":"output_text"}]}"#.into()
    };
    events.push(("response.completed", format!(r#"{{"type":"response.completed","response":{{"id":"resp_{turn}","status":"completed","model":"gpt-5.6-luna","service_tier":"default","output":[{terminal_item}],"usage":{{"input_tokens":17,"output_tokens":3,"total_tokens":20,"input_tokens_details":{{"cached_tokens":0}},"output_tokens_details":{{"reasoning_tokens":0}}}}}}}}"#)));
    events
        .into_iter()
        .map(|(event, body)| format!("event: {event}\ndata: {body}\n\n"))
        .collect::<String>()
        + "data: [DONE]\n\n"
}
