//! The tools against a recording host: read-before-edit, exact and unique
//! edits, approvals that decide what is written, and command classes that
//! are exactly the policy's.
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::json;
use zephium_core::work::runtime::*;
use zephium_core::work::{WorkError, WorkStepId};

use super::*;
use crate::work_files::WorkFileGrant;

#[derive(Default)]
struct Recorder {
    begun: Mutex<Vec<(WorkStepKindV1, Option<WorkLocalStepV1>)>>,
    settled: Mutex<Vec<(WorkStepStatus, Option<String>)>>,
    decisions: Mutex<Vec<Decision>>,
    asked: Mutex<u32>,
}
impl Recorder {
    fn deciding(decisions: &[Decision]) -> Self {
        Self {
            decisions: Mutex::new(decisions.iter().rev().copied().collect()),
            ..Default::default()
        }
    }
    fn kinds(&self) -> Vec<WorkStepKindV1> {
        self.begun
            .lock()
            .unwrap()
            .iter()
            .map(|(k, _)| k.clone())
            .collect()
    }
}
impl ComputerHost for Recorder {
    fn begin(
        &self,
        kind: WorkStepKindV1,
        local: Option<WorkLocalStepV1>,
    ) -> HostFuture<'_, Result<WorkStepId, WorkError>> {
        self.begun.lock().unwrap().push((kind, local));
        Box::pin(async { Ok(WorkStepId::generate()) })
    }
    fn settle(
        &self,
        _: WorkStepId,
        status: WorkStepStatus,
        settled: Settled,
        note: Option<String>,
    ) -> HostFuture<'_, Result<Option<String>, WorkError>> {
        self.settled.lock().unwrap().push((status, note));
        let key = match settled {
            Settled::Nothing => None,
            _ => Some(format!("s{}", self.settled.lock().unwrap().len())),
        };
        Box::pin(async move { Ok(key) })
    }
    fn progress(&self, _: WorkStepId, _: WorkCommandOutputV1) -> HostFuture<'_, ()> {
        Box::pin(async {})
    }
    fn decision(&self, _: WorkStepId) -> HostFuture<'_, Result<Decision, WorkError>> {
        *self.asked.lock().unwrap() += 1;
        let decision = self
            .decisions
            .lock()
            .unwrap()
            .pop()
            .unwrap_or(Decision::Stopped);
        Box::pin(async move { Ok(decision) })
    }
    fn folder_approved<'a>(&'a self, _: &'a str) -> HostFuture<'a, bool> {
        Box::pin(async { false })
    }
    fn cancelled(&self) -> HostFuture<'_, bool> {
        Box::pin(async { false })
    }
}

fn project() -> (tempfile::TempDir, ComputerTools, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", home.path());
    let root = home.path().join("code/app");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join("target/out.rs"), "fn add() {}\n").unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn add(a: i32) -> i32 {\n    a + 2\n}\n\npub fn twice(a: i32) -> i32 {\n    a + 2 + a\n}\n",
    )
    .unwrap();
    let (grant, refused) = WorkFileGrant::admit(&[root.to_string_lossy().into_owned()]);
    assert!(refused.is_empty());
    let tools = ComputerTools::new(grant, &root.to_string_lossy()).unwrap();
    (home, tools, root)
}

/// Runs a test body while it owns $HOME, which the grant policy reads.
fn serially(body: impl std::future::Future<Output = ()>) {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(body);
}

#[test]
fn edits_need_a_read_a_unique_match_and_an_approval() {
    serially(async {
        let (_home, tools, root) = project();
        let host =
            Recorder::deciding(&[Decision::Declined, Decision::Approved, Decision::Approved]);
        let edit = |old: &str, new: &str| json!({"path": "src/lib.rs", "old": old, "new": new});

        let blind = tools
            .call(&host, "edit", &edit("a + 2\n}", "a + 1\n}"))
            .await;
        assert!(blind.is_error && blind.content.contains("Read src/lib.rs before editing"));

        let read = tools
            .call(&host, "read", &json!({"path": "src/lib.rs"}))
            .await;
        assert!(!read.is_error);
        assert!(read
            .content
            .starts_with("1: pub fn add(a: i32) -> i32 {\n2:     a + 2\n"));
        assert!(read.content.ends_with("(source s1)"), "{}", read.content);

        let twice = tools.call(&host, "edit", &edit("a + 2", "a + 1")).await;
        assert!(
            twice.is_error && twice.content.contains("occurs 2 times"),
            "{}",
            twice.content
        );
        let missing = tools.call(&host, "edit", &edit("a + 3", "a + 1")).await;
        assert!(missing.is_error && missing.content.contains("not found"));

        let declined = tools
            .call(&host, "edit", &edit("    a + 2\n}\n\n", "    a + 1\n}\n\n"))
            .await;
        assert!(declined.is_error && declined.content.contains("declined"));
        assert!(std::fs::read_to_string(root.join("src/lib.rs"))
            .unwrap()
            .contains("    a + 2\n}\n\npub"));

        let applied = tools
            .call(&host, "edit", &edit("    a + 2\n}\n\n", "    a + 1\n}\n\n"))
            .await;
        assert_eq!(applied.content, "Changed src/lib.rs (+1 −1).");
        // The applied change is the new known text: a second edit needs no re-read.
        let all = tools
            .call(&host, "edit", &json!({"path": "src/lib.rs", "old": "a: i32", "new": "value: i32", "replace_all": true}))
            .await;
        assert!(!all.is_error, "{}", all.content);
        let text = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        assert_eq!(text.matches("value: i32").count(), 2);
        assert!(text.contains("    a + 1\n}"));

        let kinds = host.kinds();
        assert!(matches!(
            kinds.last(),
            Some(WorkStepKindV1::WriteFile { .. })
        ));
        assert!(kinds
            .iter()
            .any(|k| matches!(k, WorkStepKindV1::EditFile { .. })));

        let diffs =
            tools.diffs(&[("src/lib.rs".to_owned(), "Adds one, not two".to_owned())].into());
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "src/lib.rs");
        assert_eq!(diffs[0].language, "rust");
        assert_eq!(diffs[0].summary, "Adds one, not two");
        assert_eq!((diffs[0].added, diffs[0].removed), (3, 3));
    });
}

#[test]
fn a_file_changed_since_it_was_read_is_read_again() {
    serially(async {
        let (_home, tools, root) = project();
        let host = Recorder::deciding(&[Decision::Approved]);
        tools
            .call(&host, "read", &json!({"path": "src/lib.rs"}))
            .await;
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn add(a: i32) -> i32 {\n    a + 5\n}\n",
        )
        .unwrap();
        let stale = tools
            .call(
                &host,
                "edit",
                &json!({"path": "src/lib.rs", "old": "a + 5", "new": "a + 1"}),
            )
            .await;
        assert!(
            stale.is_error && stale.content.contains("changed since you read it"),
            "{}",
            stale.content
        );
        assert_eq!(*host.asked.lock().unwrap(), 0);
    });
}

#[test]
fn writes_create_freely_but_replace_only_what_was_read() {
    serially(async {
        let (_home, tools, root) = project();
        let host = Recorder::deciding(&[Decision::Approved, Decision::Approved]);
        let created = tools
            .call(
                &host,
                "write",
                &json!({"path": "docs/NOTES.md", "text": "# Notes\n"}),
            )
            .await;
        assert!(created.is_error, "a missing parent folder is refused");
        let created = tools
            .call(
                &host,
                "write",
                &json!({"path": "NOTES.md", "text": "# Notes\n"}),
            )
            .await;
        assert_eq!(created.content, "Changed NOTES.md (+1 −0).");
        let overwrite = tools
            .call(
                &host,
                "write",
                &json!({"path": "src/lib.rs", "text": "x\n"}),
            )
            .await;
        assert!(overwrite.is_error && overwrite.content.contains("Read it before replacing it"));
        let outside = tools
            .call(&host, "write", &json!({"path": "/etc/hosts", "text": "x"}))
            .await;
        assert!(outside.is_error && outside.content.contains("outside the granted folders"));
        let escape = tools
            .call(&host, "read", &json!({"path": "../secret"}))
            .await;
        assert!(escape.is_error);
        assert_eq!(
            std::fs::read_to_string(root.join("NOTES.md")).unwrap(),
            "# Notes\n"
        );
    });
}

#[test]
fn finding_files_respects_gitignore() {
    serially(async {
        let (_home, tools, _root) = project();
        let host = Recorder::default();
        let found = tools.call(&host, "glob", &json!({"pattern": "*.rs"})).await;
        assert_eq!(found.content, "src/lib.rs");
        let lines = tools
            .call(
                &host,
                "grep",
                &json!({"pattern": "fn \\w+", "output": "lines"}),
            )
            .await;
        assert!(
            lines.content.contains("src/lib.rs:1:pub fn add"),
            "{}",
            lines.content
        );
        assert!(!lines.content.contains("target"));
        let none = tools
            .call(&host, "grep", &json!({"pattern": "nothing here"}))
            .await;
        assert_eq!(none.content, "No matches.");
        let bad = tools
            .call(&host, "grep", &json!({"pattern": "(unclosed"}))
            .await;
        assert!(bad.is_error);
    });
}

#[test]
fn commands_keep_their_approval_classes() {
    serially(async {
        let (_home, tools, root) = project();
        let host = Recorder::deciding(&[Decision::Declined]);
        let listed = tools
            .call(&host, "bash", &json!({"command": "ls src"}))
            .await;
        assert!(
            listed.content.starts_with("exit 0 · "),
            "{}",
            listed.content
        );
        assert!(listed.content.contains("lib.rs"));
        assert_eq!(
            *host.asked.lock().unwrap(),
            0,
            "reading commands run at once"
        );
        let removed = tools
            .call(&host, "bash", &json!({"command": "rm src/lib.rs"}))
            .await;
        assert!(removed.is_error && removed.content.contains("declined"));
        assert!(root.join("src/lib.rs").exists());
        let multi = tools
            .call(&host, "bash", &json!({"command": "ls\nrm -rf ."}))
            .await;
        assert!(multi.is_error && multi.content.contains("one line"));
        let begun = host.begun.lock().unwrap();
        for (kind, local) in begun.iter() {
            let WorkStepKindV1::RunCommand { command, cwd, .. } = kind else {
                continue;
            };
            let policy = local.as_ref().unwrap().policy.as_ref().unwrap();
            let roots = [std::fs::canonicalize(&root).unwrap()];
            let expected =
                crate::work_commands::policy::classify(command, std::path::Path::new(cwd), &roots);
            assert_eq!((policy.class, policy.reason), expected, "{command}");
        }
    });
}

#[test]
fn test_output_is_summarised() {
    serially(async {
        let (_home, tools, _root) = project();
        let host = Recorder::default();
        let ran = tools
            .call(
                &host,
                "bash",
                &json!({"command": "printf 'running 2 tests\\ntest a ... ok\\ntest b ... FAILED\\ntest result: FAILED. 1 passed; 1 failed; 0 ignored\\n'"}),
            )
            .await;
        assert!(
            ran.content.contains("tests: 1 passed, 1 failed (b)"),
            "{}",
            ran.content
        );
        assert_eq!(tools.tests().unwrap().failed, 1);
        assert_eq!(
            host.settled.lock().unwrap().last().unwrap().1.as_deref(),
            Some("1 failed")
        );
    });
}
