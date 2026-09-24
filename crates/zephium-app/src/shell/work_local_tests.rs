use super::*;
use std::path::{Path, PathBuf};

struct Home {
    dir: tempfile::TempDir,
    previous: Option<std::ffi::OsString>,
}
impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", dir.path());
        std::fs::create_dir(dir.path().join("project")).unwrap();
        Self { dir, previous }
    }
    fn project(&self) -> PathBuf {
        self.dir.path().join("project").canonicalize().unwrap()
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var("HOME", previous);
        } else {
            std::env::remove_var("HOME");
        }
    }
}
fn local_begin(work: WorkId, expected: WorkRevision, root: &Path) -> WorkCommandV1 {
    let mut command = begin(work, expected, 60);
    if let WorkRuntimeIntent::BeginAgent { grant, .. } = &mut command.intent {
        grant.folders = vec![root.to_string_lossy().into_owned()];
    }
    command
}
fn run(root: &Path, command: &str, timeout: Option<u32>) -> WorkAgentFetch {
    WorkAgentFetch::RunCommand {
        cwd: root.to_string_lossy().into_owned(),
        command: command.into(),
        timeout_secs: timeout,
    }
}
fn finish_local() -> WorkAgentTurnOutput {
    let mut result = finishing();
    result.artifacts[0].evidence = vec![0];
    result
}

#[test]
fn work_local_file_and_command_approval_journal() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(work_local_file_and_command_approval_journal_inner());
}

async fn work_local_file_and_command_approval_journal_inner() {
    let home = Home::new();
    let root = home.project();
    let file = root.join("version.txt").to_string_lossy().into_owned();
    let moved = root.join("renamed.txt").to_string_lossy().into_owned();
    std::fs::write(&file, "version=1\nname=demo\n").unwrap();
    let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
    let (mut shell, queue, handle, profile) = fixture(store);
    let work = new_work(
        &mut shell,
        &queue,
        &handle,
        "Update the local fixture and test it",
    )
    .await;
    let script = Script::default();
    script.play([
        output(vec![
            WorkAgentFetch::ReadFile {
                path: file.clone(),
                offset: Some(1),
                limit: Some(2),
            },
            WorkAgentFetch::List {
                path: root.to_string_lossy().into_owned(),
                depth: Some(3),
            },
            WorkAgentFetch::SearchFiles {
                path: root.to_string_lossy().into_owned(),
                query: "version=[0-9]".into(),
                glob: Some("*.txt".into()),
                regex: Some(true),
            },
            run(&root, "pwd", None),
        ]),
        output(vec![WorkAgentFetch::EditFile {
            path: file.clone(),
            old: String::new(),
            new: String::new(),
            replacements: vec![
                WorkFileReplacementV1 {
                    old: "version=1".into(),
                    new: "version=2".into(),
                },
                WorkFileReplacementV1 {
                    old: "name=demo".into(),
                    new: "name=fixture".into(),
                },
            ],
        }]),
        output(vec![WorkAgentFetch::MoveFile {
            from: file.clone(),
            to: moved.clone(),
        }]),
        output(vec![WorkAgentFetch::DeleteFile {
            path: moved.clone(),
        }]),
        output(vec![run(&root, "printf first > result.txt", None)]),
        output(vec![
            run(&root, "printf second >> result.txt", None),
            run(&root, "rm result.txt", None),
        ]),
        finish_local(),
    ]);
    let service = WorkAgentService::new(handle.clone());
    let sources = Sources::default();
    let decisions = async {
        let mut count = 0;
        while count < 5 {
            let state = projection(&handle, profile, work).await;
            if let Some(execution) = state.executions.last() {
                if let Some(step) = execution.steps.iter().find(|s| {
                    s.status == WorkStepStatus::Running
                        && s.kind.proposes_write()
                        && s.kind.file_decision().is_none()
                        && (!matches!(s.kind, WorkStepKindV1::RunCommand { .. })
                            || s.local
                                .as_ref()
                                .and_then(|l| l.policy.as_ref())
                                .is_some_and(|p| p.scope != WorkCommandApprovalScopeV1::None))
                }) {
                    let approve = !matches!(&step.kind,WorkStepKindV1::RunCommand{command,..} if command.starts_with("rm"));
                    if let WorkStepKindV1::EditFile { .. } = step.kind {
                        assert_eq!(
                            std::fs::read_to_string(&file).unwrap(),
                            "version=1\nname=demo\n"
                        );
                        assert!(step
                            .local
                            .as_ref()
                            .unwrap()
                            .proposal
                            .as_ref()
                            .unwrap()
                            .contains("version=2"));
                    }
                    command(
                        &handle,
                        profile,
                        work,
                        WorkRuntimeIntent::ApproveStep {
                            execution: execution.id,
                            step: step.id,
                            approve,
                        },
                    )
                    .await;
                    count += 1;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    let (result, ()) = drive(&mut shell, &queue, async {
        tokio::join!(
            service.run(
                profile,
                local_begin(work, WorkRevision::INITIAL, &root),
                None,
                WorkAgentProviders {
                    turn: &script,
                    search: &sources
                },
                |probe, request| page(probe, request, false),
                |_| {}
            ),
            decisions
        )
    })
    .await;
    let state = result.unwrap();
    let execution = &state.executions[0];
    assert_eq!(execution.status, WorkExecutionStatus::NeedsReview);
    assert_eq!(execution.folder_approvals.len(), 1);
    assert_eq!(execution.command_evidence.len(), 3);
    assert_eq!(execution.file_evidence.len(), 6);
    assert_eq!(
        std::fs::read_to_string(root.join("result.txt")).unwrap(),
        "firstsecond"
    );
    assert!(!Path::new(&moved).exists());
    assert!(execution
        .steps
        .iter()
        .any(|s| s.note.as_deref() == Some("Declined") && s.status == WorkStepStatus::Failed));
    assert!(execution
        .file_evidence
        .iter()
        .filter(|r| matches!(
            r.file.kind,
            WorkFileKindV1::Written | WorkFileKindV1::Moved | WorkFileKindV1::Deleted
        ))
        .all(|r| r.file.before_digest.is_some()));
    execution
        .validate(state.work.plan.as_ref().unwrap(), state.work.revision)
        .unwrap();
}

#[test]
fn work_local_command_timeout_and_stop_settle_with_output() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(work_local_command_timeout_and_stop_settle_with_output_inner());
}

async fn work_local_command_timeout_and_stop_settle_with_output_inner() {
    let home = Home::new();
    let root = home.project();
    for stop in [false, true] {
        let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
        let (mut shell, queue, handle, profile) = fixture(store);
        let work = new_work(&mut shell, &queue, &handle, "Run the fixture command").await;
        let script = Script::default();
        script.play([
            output(vec![run(
                &root,
                "printf started; trap '' TERM; sleep 30 & wait",
                Some(if stop { 30 } else { 1 }),
            )]),
            finish_local(),
        ]);
        let sources = Sources::default();
        let service = WorkAgentService::new(handle.clone());
        let decide = async {
            let (execution, step) = running_step(&handle, profile, work, |k| {
                matches!(k, WorkStepKindV1::RunCommand { .. })
            })
            .await;
            command(
                &handle,
                profile,
                work,
                WorkRuntimeIntent::ApproveStep {
                    execution,
                    step,
                    approve: true,
                },
            )
            .await;
            if stop {
                loop {
                    let state = projection(&handle, profile, work).await;
                    if state.executions[0].steps.iter().any(|s| {
                        s.local
                            .as_ref()
                            .and_then(|l| l.output.as_ref())
                            .is_some_and(|o| o.text.contains("started"))
                    }) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                let started = Instant::now();
                command(
                    &handle,
                    profile,
                    work,
                    WorkRuntimeIntent::Cancel {
                        execution,
                        intervention: None,
                    },
                )
                .await;
                loop {
                    let state = projection(&handle, profile, work).await;
                    if state.executions[0]
                        .steps
                        .iter()
                        .find(|s| s.id == step)
                        .unwrap()
                        .status
                        != WorkStepStatus::Running
                    {
                        break;
                    }
                    assert!(started.elapsed() < Duration::from_secs(3));
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
        };
        let (result, ()) = drive(&mut shell, &queue, async {
            tokio::join!(
                service.run(
                    profile,
                    local_begin(work, WorkRevision::INITIAL, &root),
                    None,
                    WorkAgentProviders {
                        turn: &script,
                        search: &sources
                    },
                    |probe, request| page(probe, request, false),
                    |_| {}
                ),
                decide
            )
        })
        .await;
        let state = result.unwrap();
        let execution = &state.executions[0];
        let step = execution
            .steps
            .iter()
            .find(|s| matches!(s.kind, WorkStepKindV1::RunCommand { .. }))
            .unwrap();
        assert_eq!(step.status, WorkStepStatus::Failed);
        assert_eq!(
            step.note.as_deref(),
            Some(if stop { "Stopped" } else { "Stopped after 1 s" })
        );
        assert!(execution.command_evidence[0]
            .command
            .text
            .contains("started"));
    }
}

#[test]
fn work_local_quit_recovers_command_as_failed_without_replay() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(work_local_quit_recovers_command_as_failed_without_replay_inner());
}

async fn work_local_quit_recovers_command_as_failed_without_replay_inner() {
    let home = Home::new();
    let root = home.project();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, profile) = fixture(store.clone());
    let work = new_work(&mut shell, &queue, &handle, "Run a local command").await;
    let script = Script::default();
    script.play([output(vec![run(
        &root,
        "printf started; sleep 30; touch replayed",
        None,
    )])]);
    let sources = Sources::default();
    let service = WorkAgentService::new(handle.clone());
    drive(&mut shell,&queue,async{
        tokio::select! {
            _=service.run(profile,local_begin(work,WorkRevision::INITIAL,&root),None,WorkAgentProviders{turn:&script,search:&sources},|probe,request|page(probe,request,false),|_|{})=>panic!("command should still be running"),
            _=async{
                let (execution,step)=running_step(&handle,profile,work,|k|matches!(k,WorkStepKindV1::RunCommand{..})).await;command(&handle,profile,work,WorkRuntimeIntent::ApproveStep{execution,step,approve:true}).await;
                loop {let state=projection(&handle,profile,work).await;if state.executions[0].steps.iter().any(|s|s.local.as_ref().and_then(|l|l.output.as_ref()).is_some_and(|o|o.text.contains("started"))){break;}tokio::time::sleep(Duration::from_millis(20)).await;}
            }=>{}
        }
    }).await;
    drop(service);
    drop(handle);
    drop(queue);
    drop(shell);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
    drop(store);
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, _) = fixture(store);
    let state = drive(&mut shell, &queue, projection(&handle, profile, work)).await;
    let execution = state.executions[0].id;
    drive(
        &mut shell,
        &queue,
        command(
            &handle,
            profile,
            work,
            WorkRuntimeIntent::AcknowledgeInterruption { execution },
        ),
    )
    .await;
    let state = drive(&mut shell, &queue, projection(&handle, profile, work)).await;
    let step = state.executions[0]
        .steps
        .iter()
        .find(|s| matches!(s.kind, WorkStepKindV1::RunCommand { .. }))
        .unwrap();
    assert_eq!(step.status, WorkStepStatus::Failed);
    assert_eq!(step.note.as_deref(), Some("Stopped when the app quit"));
    assert!(!root.join("replayed").exists());
}
