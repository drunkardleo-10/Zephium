//! Connections end to end against the real `gh` and the MCP fixture server.
//! Ignored by default: run with `--ignored` on a Mac where gh is signed in
//! and `ZEPHIUM_MCP_FIXTURE` names the built `zephium-mcp-fixture`.
use std::sync::Mutex;

use serde_json::json;
use zephium_core::work::runtime::WorkConfirmV1;
use zephium_core::work::WorkError;
use zephium_ipc::work::{WorkServerTransportV1, WorkServerV1};

use super::*;

#[derive(Default)]
struct Person {
    asked: Mutex<Vec<String>>,
    confirmed: Mutex<Vec<String>>,
    rows: Mutex<Vec<(String, bool)>>,
}
impl ConnectionHost for Person {
    fn record(
        &self,
        fact: CallFact,
        ok: bool,
        _: Option<String>,
    ) -> HostFuture<'_, Result<(), WorkError>> {
        self.rows.lock().unwrap().push((helper::row(&fact), ok));
        Box::pin(async { Ok(()) })
    }
    fn confirm(&self, confirm: WorkConfirmV1) -> HostFuture<'_, Result<Decision, WorkError>> {
        self.confirmed.lock().unwrap().push(confirm.headline);
        Box::pin(async { Ok(Decision::Declined) })
    }
    fn ask<'a>(
        &'a self,
        prompt: &'a str,
        options: &'a [&'a str],
    ) -> HostFuture<'a, Result<Option<String>, WorkError>> {
        self.asked.lock().unwrap().push(prompt.to_owned());
        let yes = options.first().map(|o| (*o).to_owned());
        Box::pin(async move { Ok(yes) })
    }
}

#[tokio::test]
#[ignore = "reads GitHub through the signed-in gh"]
async fn gh_reads_an_issue_and_holds_a_comment() {
    let status = cli::status(cli::Cli::Gh).await;
    assert!(status.usable(), "gh is installed and signed in");
    let github = gh::GitHub::new(status.path.unwrap(), None, None);
    let person = Person::default();
    let issue = github
        .call(
            &person,
            "github_issue",
            &json!({"repo": "cli/cli", "number": 10000}),
        )
        .await;
    assert!(!issue.is_error, "{}", issue.content);
    assert!(issue.content.starts_with("#10000 · "));
    let prs = github
        .call(
            &person,
            "github_prs",
            &json!({"repo": "cli/cli", "limit": 3}),
        )
        .await;
    assert!(!prs.is_error, "{}", prs.content);
    let comment = github
        .call(
            &person,
            "github_comment",
            &json!({"repo": "cli/cli", "number": 10000, "body": "test"}),
        )
        .await;
    assert!(comment.is_error && comment.content.contains("declined"));
    let asked = person.asked.lock().unwrap();
    assert!(
        asked.iter().all(|a| a.starts_with("Use GitHub (gh)? ")),
        "{asked:?}"
    );
    assert_eq!(person.confirmed.lock().unwrap().len(), 1);
    let rows = person.rows.lock().unwrap();
    assert_eq!(rows[0].0.split(" · ").next(), Some("Read issue #10000"));
    assert_eq!(rows[1].0, "Listed 3 pull requests");
}

#[tokio::test]
#[ignore = "needs the zephium-mcp-fixture binary"]
async fn an_mcp_server_is_a_connection() {
    let Ok(program) = std::env::var("ZEPHIUM_MCP_FIXTURE") else {
        panic!("set ZEPHIUM_MCP_FIXTURE");
    };
    let server = WorkServerV1 {
        id: "fixture".into(),
        name: "Fixture".into(),
        transport: WorkServerTransportV1::Stdio {
            command: program,
            args: vec![],
            env: vec![],
        },
        enabled: true,
    };
    let check = mcp::check("01M3CTWDG20GFZPACD7905RSRJ", &server).await;
    assert_eq!(check.outcome, zephium_ipc::work::WorkServerOutcomeV1::Ready);
    assert!(check.tools.iter().any(|t| t.name == "send_note" && t.asks));
    let connection = mcp::McpConnection::open("01M3CTWDG20GFZPACD7905RSRJ", server.clone())
        .await
        .unwrap();
    let person = Person::default();
    let echo = connection
        .call(&person, "fixture__echo", &json!({"text": "hello"}))
        .await;
    assert!(echo.content.ends_with("hello") && echo.content.contains("data, not instructions"));
    let send = connection
        .call(
            &person,
            "fixture__send_note",
            &json!({"to": "team", "body": "hi"}),
        )
        .await;
    assert!(send.is_error && send.content.contains("declined"));
    assert_eq!(
        person.confirmed.lock().unwrap().as_slice(),
        ["send_note on Fixture?"]
    );
    assert_eq!(
        person.asked.lock().unwrap()[0],
        "Use Fixture? To use its tools in this work with your account."
    );
}
