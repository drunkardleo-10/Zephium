use crate::work_planning::*;
use crate::{actor::CommandQueue, Command, Handle, Shell, WorkIntent, WorkUserEdit};
use std::{
    future::Future,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use zephium_core::work::proposal::{WorkNodeProposal, WorkPlanProposal};
use zephium_core::{
    ids::ProfileId,
    work::{planning::*, port::*, *},
};

struct Scripted {
    proposal: Mutex<Option<WorkPlanningProposal>>,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    calls: AtomicUsize,
}
impl WorkPlanningProvider for Scripted {
    fn propose(&self, _: WorkPlanningDisclosure) -> WorkPlanningFuture<'_> {
        Box::pin(async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let hook = self.hook.lock().unwrap().take();
            if let Some(hook) = hook {
                hook();
            }
            let proposal = self.proposal.lock().unwrap().take();
            let Some(proposal) = proposal else {
                return std::future::pending().await;
            };
            Ok(WorkPlanningResult {
                proposal,
                usage: WorkPlanningUsage {
                    input_tokens: 100,
                    output_tokens: 200,
                    cost_ceiling_micro_usd: 300,
                },
            })
        })
    }
}
fn scripted(proposal: Option<WorkPlanningProposal>) -> Arc<Scripted> {
    Arc::new(Scripted {
        proposal: Mutex::new(proposal),
        hook: Mutex::new(None),
        calls: AtomicUsize::new(0),
    })
}
fn draft() -> WorkPlanningProposal {
    WorkPlanningProposal::Draft {
        plan: WorkPlanProposal {
            nodes: vec![WorkNodeProposal {
                key: 7,
                objective: "Compare documented guarantees".into(),
                dependencies: vec![],
                outputs: vec![WorkExpectedOutput {
                    name: "comparison".into(),
                    description: "A cited comparison with uncertainties".into(),
                    review: WorkOutputReview::SourceMappedNeedsReview,
                }],
            }],
        },
    }
}
pub(super) fn fixture(
    store: Arc<zephium_store::SqliteStore>,
) -> (Shell, CommandQueue, Handle, ProfileId) {
    let mut shell = Shell::new(
        Arc::new(crate::shell::tests::FakeEngine::default()),
        store.clone(),
        Arc::new(crate::shell::tests::FakeChrome),
        Box::new(|_| {}),
    );
    shell.handle(Command::Bootstrap);
    assert!(store.flush());
    let profile = shell.windows.focused().unwrap().profile;
    let queue = CommandQueue::new();
    let handle = Handle::new(queue.clone());
    (shell, queue, handle, profile)
}
pub(super) async fn drive<F: Future>(
    shell: &mut Shell,
    queue: &CommandQueue,
    future: F,
) -> F::Output {
    drive_with(shell, queue, future, |_, _| {}).await
}
async fn drive_with<F: Future>(
    shell: &mut Shell,
    queue: &CommandQueue,
    future: F,
    mut before: impl FnMut(&mut Shell, &Command),
) -> F::Output {
    tokio::pin!(future);
    loop {
        while let Some(command) = queue.try_recv() {
            before(shell, &command);
            shell.handle(command);
        }
        tokio::select! { result = &mut future => return result, _ = tokio::time::sleep(Duration::from_millis(1)) => {} }
    }
}
async fn read(
    shell: &mut Shell,
    queue: &CommandQueue,
    handle: &Handle,
    id: WorkId,
) -> Box<WorkSnapshot> {
    let projection = drive(
        shell,
        queue,
        handle.work_document(WorkIntent::Read { id }).unwrap(),
    )
    .await
    .unwrap();
    let WorkReply::Snapshot(work) = projection.reply else {
        panic!()
    };
    work
}
#[tokio::test]
async fn planning_application_persists_questions_answers_and_agent_drafts_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, profile) = fixture(store.clone());
    let create = handle
        .work_document(WorkIntent::Create {
            objective: "Compare SQLite and PostgreSQL for a local desktop app".into(),
        })
        .unwrap();
    let id = create.work_id().unwrap();
    drive(&mut shell, &queue, create).await.unwrap();
    let provider = scripted(Some(WorkPlanningProposal::Clarify {
        prompt: "Must multiple computers write concurrently?".into(),
        options: vec!["Yes".into(), "No".into()],
    }));
    let service = WorkPlanningService::new(handle.clone(), provider);
    let result = drive(
        &mut shell,
        &queue,
        service.plan(profile, id, WorkRevision::INITIAL, None),
    )
    .await
    .unwrap();
    let WorkReply::Snapshot(questioned) = result.persistence.unwrap().reply else {
        panic!()
    };
    assert_eq!(questioned.status, WorkAuthoringStatus::NeedsInput);
    let question = &questioned.questions[0];
    assert_eq!(question.author, WorkAuthor::PrimaryAgent);
    let answer = handle
        .work_document(WorkIntent::Edit {
            id,
            expected: questioned.revision,
            edit: WorkUserEdit::AnswerQuestion {
                id: question.id,
                answer: "No".into(),
            },
        })
        .unwrap();
    drive(&mut shell, &queue, answer).await.unwrap();
    let work = read(&mut shell, &queue, &handle, id).await;
    assert_eq!(work.questions[0].answer_author, Some(WorkAuthor::User));
    let service = WorkPlanningService::new(handle.clone(), scripted(Some(draft())));
    let completion = drive(
        &mut shell,
        &queue,
        service.plan(profile, id, work.revision, None),
    )
    .await
    .unwrap();
    assert_eq!(completion.usage.output_tokens, 200);
    let WorkReply::Snapshot(planned) = completion.persistence.unwrap().reply else {
        panic!()
    };
    assert_eq!(planned.status, WorkAuthoringStatus::PlanReady);
    let plan = planned.plan.as_ref().unwrap();
    assert_eq!(plan.author, WorkAuthor::PrimaryAgent);
    assert_eq!(plan.basis_revision, work.revision);
    assert_ne!(plan.draft.nodes[0].id, WorkPlanNodeId::from(7));
    drop(service);
    drop(handle);
    drop(shell);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
    drop(store);
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, reopened_profile) = fixture(store.clone());
    assert_eq!(profile, reopened_profile);
    assert_eq!(read(&mut shell, &queue, &handle, id).await, planned);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
}
#[tokio::test]
async fn planning_application_refuses_stale_private_and_cancelled_results() {
    let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
    let (mut shell, queue, handle, profile) = fixture(store.clone());
    let create = handle
        .work_document(WorkIntent::Create {
            objective: "Compare storage engines".into(),
        })
        .unwrap();
    let id = create.work_id().unwrap();
    drive(&mut shell, &queue, create).await.unwrap();
    let provider = scripted(Some(draft()));
    let service = WorkPlanningService::new(handle.clone(), provider.clone());
    assert!(matches!(
        drive(
            &mut shell,
            &queue,
            service.plan(profile, id, WorkRevision::new(99).unwrap(), None)
        )
        .await,
        Err(WorkPlanningError::Stale)
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    // User edit is queued while generation is in progress. The final CAS
    // must conflict while preserving the provider's usage receipt.
    let user = handle.clone();
    *provider.hook.lock().unwrap() = Some(Box::new(move || {
        drop(
            user.work_document(WorkIntent::Edit {
                id,
                expected: WorkRevision::INITIAL,
                edit: WorkUserEdit::SetObjective {
                    objective: "A newer user objective".into(),
                },
            })
            .unwrap(),
        );
    }));
    let completion = drive(
        &mut shell,
        &queue,
        service.plan(profile, id, WorkRevision::INITIAL, None),
    )
    .await
    .unwrap();
    assert!(matches!(completion.persistence, Err(WorkError::Conflict)));
    assert_eq!(completion.usage.input_tokens, 100);
    let work = read(&mut shell, &queue, &handle, id).await;
    assert!(work.plan.is_none());
    assert_eq!(work.objective, "A newer user objective");
    let service = WorkPlanningService::new(handle.clone(), scripted(Some(draft())));
    let mut dispatched = 0;
    let completion = drive_with(
        &mut shell,
        &queue,
        service.plan(profile, id, work.revision, None),
        |shell, _| {
            dispatched += 1;
            if dispatched == 2 {
                let mut selected = shell.profiles.remove(profile).unwrap();
                selected.kind = zephium_core::profiles::ProfileKind::Incognito;
                assert!(shell.profiles.insert(selected));
            }
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        completion.persistence,
        Err(WorkError::ProfileUnavailable)
    ));
    let mut selected = shell.profiles.remove(profile).unwrap();
    selected.kind = zephium_core::profiles::ProfileKind::Default;
    assert!(shell.profiles.insert(selected));
    let provider = scripted(None);
    let service = WorkPlanningService::new(handle.clone(), provider.clone());
    let mut pending = Box::pin(service.plan(profile, id, work.revision, None));
    assert!(tokio::time::timeout(
        Duration::from_millis(50),
        drive(&mut shell, &queue, &mut pending)
    )
    .await
    .is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        service.plan(profile, id, work.revision, None).await,
        Err(WorkPlanningError::Capacity)
    ));
    // Drop the actual future, not just its Pin reference.
    drop(pending);
    let resumed = WorkPlanningService::new(handle.clone(), scripted(Some(draft())));
    let completion = drive(
        &mut shell,
        &queue,
        resumed.plan(profile, id, work.revision, None),
    )
    .await
    .unwrap();
    assert!(completion.persistence.is_ok());
    assert!(read(&mut shell, &queue, &handle, id).await.plan.is_some());
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
}

/// Explicit opt-in spends at most $0.10 using the fixed development Keychain
/// credential. Uses public fixture content and a temporary real Store only.
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "live OpenAI development credential qualification; run explicitly"]
async fn planning_live_openai_through_application_and_reopened_store() {
    use zephium_agentic::{
        load_macos_development_openai_credential, AgentProviderTransport,
        AgentProviderTransportConfig, OpenAiWorkPlanner, WorkPlanningConfig,
    };
    let config = WorkPlanningConfig::try_new(
        zephium_agent_model_catalog::try_luna_provider_exact_call_config(4096).unwrap(),
        8192,
        100_000,
    )
    .unwrap();
    let transport =
        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD).unwrap();
    let provider = OpenAiWorkPlanner::try_new(
        transport.clone(),
        load_macos_development_openai_credential().unwrap(),
        config,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, profile) = fixture(store.clone());
    let create=handle.work_document(WorkIntent::Create {objective:"Draft a plan to compare SQLite and PostgreSQL for a single-user offline macOS desktop application. Use only public documentation in the eventual research; no accounts or private files are needed. The output should be a short cited comparison, with durability and migration tradeoffs, for an engineering review. Planning only; make reasonable assumptions and do not ask clarification questions.".into()}).unwrap();
    let id = create.work_id().unwrap();
    drive(&mut shell, &queue, create).await.unwrap();
    let service = WorkPlanningService::new(handle.clone(), Arc::new(provider));
    let completion = drive(
        &mut shell,
        &queue,
        service.plan(profile, id, WorkRevision::INITIAL, None),
    )
    .await
    .unwrap();
    let WorkReply::Snapshot(planned) = completion.persistence.unwrap().reply else {
        panic!()
    };
    assert_eq!(planned.status, WorkAuthoringStatus::PlanReady);
    assert_eq!(
        planned.plan.as_ref().unwrap().author,
        WorkAuthor::PrimaryAgent
    );
    assert!(completion.usage.cost_ceiling_micro_usd <= 100_000);
    println!(
        "live planning: input={} output={} cost_ceiling_micro_usd={} nodes={}",
        completion.usage.input_tokens,
        completion.usage.output_tokens,
        completion.usage.cost_ceiling_micro_usd,
        planned.plan.as_ref().unwrap().draft.nodes.len()
    );
    drop(service);
    drop(handle);
    drop(shell);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
    drop(store);
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, reopened) = fixture(store.clone());
    assert_eq!(profile, reopened);
    assert_eq!(read(&mut shell, &queue, &handle, id).await, planned);
    transport.seal();
    assert_eq!(transport.snapshot().unwrap().active_attempts(), 0);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
}
