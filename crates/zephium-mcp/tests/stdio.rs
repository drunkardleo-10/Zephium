//! The client against the fixture server as a real child process.
use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;
use zephium_mcp::{Endpoint, McpSession, StdioServer, CALL};

#[tokio::test]
async fn a_stdio_server_runs_in_its_own_process() {
    let endpoint = Endpoint::Stdio(StdioServer {
        program: PathBuf::from(env!("CARGO_BIN_EXE_zephium-mcp-fixture")),
        args: vec![],
        env: vec![("PATH".into(), "/usr/bin:/bin".into())],
        cwd: None,
    });
    let session = McpSession::connect(&endpoint).await.expect("connects");
    assert_eq!(session.info().name.as_deref(), Some("fixture"));
    assert!(!session.info().protocol.is_empty());
    let tools = session.tools().await.unwrap();
    assert_eq!(tools.len(), 4);
    let echo = session
        .call(
            "echo",
            json!({"text": "över"}).as_object().cloned().unwrap(),
            CALL,
        )
        .await
        .unwrap();
    assert_eq!(echo.text, "över");
    let started = std::time::Instant::now();
    session.close().await;
    assert!(started.elapsed() < Duration::from_secs(3));
}
