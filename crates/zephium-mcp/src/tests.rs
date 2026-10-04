use super::*;
use rmcp::ServiceExt;
use serde_json::json;
use tokio::io::AsyncReadExt;

/// A session over an in-process pipe to the fixture.
async fn in_process() -> McpSession {
    let (client, server) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        let (read, write) = tokio::io::split(server);
        if let Ok(service) = fixture::Fixture.serve((read, write)).await {
            let _ = service.waiting().await;
        }
    });
    let (read, write) = tokio::io::split(client);
    let service = client_config()
        .serve_with_lifecycle((stdio::Bounded::new(read), write), lifecycle())
        .await
        .expect("handshake");
    McpSession::adopt(service, None)
}

#[tokio::test]
async fn lists_calls_and_reads_within_bounds() {
    let session = in_process().await;
    let info = session.info();
    assert_eq!(info.name.as_deref(), Some("fixture"));
    assert_eq!(info.version.as_deref(), Some("1.2.3"));
    assert!(info.has_tools && info.has_resources);
    assert!(info
        .instructions
        .as_deref()
        .is_some_and(|i| i.contains("Fixture")));

    let tools = session.tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["echo", "add", "send_note", "flood"]);
    assert_eq!(tools[0].read_only, Some(true));
    assert_eq!(tools[0].title.as_deref(), Some("Echo"));
    assert_eq!(tools[2].read_only, Some(false));
    assert_eq!(tools[1].schema["required"], json!(["a", "b"]));

    let args = |value: Value| value.as_object().cloned().unwrap();
    let echo = session
        .call("echo", args(json!({"text": "hi"})), CALL)
        .await
        .unwrap();
    assert_eq!(
        echo,
        McpText {
            text: "hi".into(),
            is_error: false,
            truncated: false
        }
    );
    let sum = session
        .call("add", args(json!({"a": 2, "b": 3.5})), CALL)
        .await
        .unwrap();
    assert_eq!(sum.text, "5.5");
    let refused = session
        .call("add", args(json!({"a": "x"})), CALL)
        .await
        .unwrap();
    assert!(refused.is_error);
    let flood = session.call("flood", Map::new(), CALL).await.unwrap();
    assert!(flood.truncated);
    assert_eq!(flood.text.len(), MAX_RESULT_BYTES);
    assert_eq!(
        session.call("missing", Map::new(), CALL).await,
        Err(McpError::Unknown)
    );

    let resources = session.resources().await.unwrap();
    assert_eq!(resources[0].uri, "memo://readme");
    let memo = session.read("memo://readme").await.unwrap();
    assert_eq!(memo.text, "Hello from the fixture.");
    session.close().await;
}

#[tokio::test]
async fn an_endless_line_fails_the_stream() {
    let (mut writer, reader) = tokio::io::duplex(1024);
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let chunk = vec![b'x'; 512];
        for _ in 0..16 {
            if writer.write_all(&chunk).await.is_err() {
                break;
            }
        }
    });
    let mut bounded = stdio::Bounded::with_limit(reader, 4096);
    let mut sink = Vec::new();
    let error = bounded.read_to_end(&mut sink).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(sink.len() <= 4096 + 1024);
}

#[test]
fn programs_resolve_on_the_given_path() {
    let executable = std::env::current_exe().unwrap();
    let directory = executable.parent().unwrap();
    let path = std::env::join_paths([directory]).unwrap();
    let name = executable.file_name().unwrap().to_str().unwrap();
    assert_eq!(
        program_path(name, path.to_str().unwrap()),
        Some(executable.clone())
    );
    assert_eq!(
        program_path(executable.to_str().unwrap(), ""),
        Some(executable)
    );
    assert_eq!(program_path("./program", path.to_str().unwrap()), None);
    assert_eq!(
        program_path("no-such-program-zephium", path.to_str().unwrap()),
        None
    );
}
#[tokio::test]
async fn a_missing_program_is_a_closed_failure() {
    let endpoint = Endpoint::Stdio(StdioServer {
        program: PathBuf::from("/nonexistent/zephium-mcp"),
        args: vec![],
        env: vec![],
        cwd: None,
    });
    assert_eq!(
        McpSession::connect(&endpoint).await.err(),
        Some(McpError::Spawn)
    );
    let silent = Endpoint::Stdio(StdioServer {
        program: PathBuf::from("/bin/cat"),
        args: vec!["/dev/null".into()],
        env: vec![],
        cwd: None,
    });
    assert!(McpSession::connect(&silent).await.is_err());
}
