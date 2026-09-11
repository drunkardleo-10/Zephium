use super::work_planning_tests::{drive, fixture};
use crate::{work_runtime::*, WorkIntent, WorkUserEdit};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use zephium_core::work::{artifact::*, port::*, proposal::*, runtime::*, *};
use zephium_ipc::work::WorkCommandV1;

#[tokio::test]
async fn runtime_application_approves_exact_plan_settles_artifact_and_recovers_dropped_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, profile) = fixture(store.clone());
    let create = handle
        .work_document(WorkIntent::Create {
            objective: "Produce a local checklist".into(),
        })
        .unwrap();
    let work = create.work_id().unwrap();
    drive(&mut shell, &queue, create).await.unwrap();
    let edit = handle
        .work_document(WorkIntent::Edit {
            id: work,
            expected: WorkRevision::INITIAL,
            edit: WorkUserEdit::ReplaceDraft {
                proposal: WorkPlanProposal {
                    nodes: vec![
                        WorkNodeProposal {
                            key: 0,
                            objective: "Build the requested checklist".into(),
                            dependencies: vec![],
                            outputs: vec![WorkExpectedOutput {
                                name: "checklist".into(),
                                description: "The requested local task list".into(),
                                review: WorkOutputReview::Mechanical,
                            }],
                        },
                        WorkNodeProposal {
                            key: 1,
                            objective: "Consume the selected checklist result".into(),
                            dependencies: vec![0],
                            outputs: vec![WorkExpectedOutput {
                                name: "summary".into(),
                                description: "A derived local checklist".into(),
                                review: WorkOutputReview::Mechanical,
                            }],
                        },
                    ],
                },
            },
        })
        .unwrap();
    let WorkReply::Snapshot(planned) = drive(&mut shell, &queue, edit).await.unwrap().reply else {
        panic!()
    };
    let plan = planned.plan.as_ref().unwrap();
    let limits = WorkExecutionLimits {
        model_tokens: 8000,
        cost_micro_usd: 10000,
        operations: 32,
        timeout_seconds: 300,
        max_workers: 1,
    };
    let spec = WorkExecutionSpec {
        plan_revision: plan.revision,
        limits,
        nodes: plan
            .draft
            .nodes
            .iter()
            .map(|node| WorkNodeExecutionSpec {
                node: node.id,
                parent: None,
                capability: WorkCapability::Synthesize,
                limits: WorkExecutionLimits {
                    model_tokens: limits.model_tokens / 2,
                    cost_micro_usd: limits.cost_micro_usd / 2,
                    operations: limits.operations / 2,
                    ..limits
                },
            })
            .collect(),
    };
    let approve = handle
        .work_command(
            profile,
            WorkCommandV1 {
                version: 1,
                work,
                expected_revision: planned.revision,
                command: WorkCommandId::generate(),
                intent: WorkRuntimeIntent::Approve { spec: spec.clone() },
            },
        )
        .unwrap();
    let WorkReply::RuntimeCommand {
        projection: approved,
        receipt,
    } = drive(&mut shell, &queue, approve).await.unwrap().reply
    else {
        panic!()
    };
    let service = WorkRuntimeService::new(handle.clone());
    let attempt = drive(
        &mut shell,
        &queue,
        service.begin_node(
            profile,
            work,
            approved.work.revision,
            receipt.execution,
            plan.draft.nodes[0].id,
        ),
    )
    .await
    .unwrap();
    assert_eq!(attempt.node().objective, plan.draft.nodes[0].objective);
    assert!(attempt.deadline() > Instant::now());
    assert!(attempt.dependency_artifacts().is_empty());
    let observer = attempt.observer();
    assert!(observer.latest().is_none());
    attempt.record_activity(zephium_ipc::work::WorkActivityV1::ProducingArtifact);
    let signal = observer.latest().unwrap();
    assert_eq!(signal.work, work);
    assert_eq!(signal.attempt, attempt.attempt());
    // A focused private surface cannot read the regular Work, but cannot
    // redirect or prevent settlement of that Work's already-owned attempt.
    let private = zephium_core::ids::ProfileId::generate();
    assert!(shell.profiles.insert(zephium_core::profiles::Profile {
        id: private,
        name: "Private fixture".into(),
        kind: zephium_core::profiles::ProfileKind::Incognito
    }));
    shell.windows.focused_mut().unwrap().profile = private;
    assert!(matches!(
        drive(
            &mut shell,
            &queue,
            handle.work_projection(profile, work).unwrap()
        )
        .await,
        Err(WorkError::ProfileUnavailable)
    ));
    let result = WorkAdapterResult {
        status: WorkAttemptStatus::Succeeded,
        usage: Some(WorkUsage::default()),
        artifacts: vec![WorkArtifactDraft {
            output: "checklist".into(),
            title: "Tasks".into(),
            data: WorkArtifactDataV1::Checklist {
                items: vec![WorkChecklistItem {
                    text: "Review release notes".into(),
                    completed: false,
                }],
            },
            evidence: vec![],
        }],
    };
    let completed = drive(&mut shell, &queue, attempt.settle(result))
        .await
        .unwrap();
    assert_eq!(completed.executions[0].status, WorkExecutionStatus::Running);
    assert_eq!(completed.executions[0].artifacts.len(), 1);
    let mut missing_output = completed.executions[0].clone();
    missing_output.artifacts.clear();
    assert_eq!(
        missing_output.validate(plan, completed.work.revision),
        Err(WorkError::Invalid)
    );
    assert_eq!(completed.work.profile, profile);
    assert!(observer.latest().is_none());
    shell.windows.focused_mut().unwrap().profile = profile;
    shell.profiles.remove(private);
    let successor = drive(
        &mut shell,
        &queue,
        service.begin_node(
            profile,
            work,
            completed.work.revision,
            receipt.execution,
            plan.draft.nodes[1].id,
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        successor.dependency_artifacts(),
        completed.executions[0].artifacts
    );
    let completed = drive(
        &mut shell,
        &queue,
        successor.settle(WorkAdapterResult {
            status: WorkAttemptStatus::Succeeded,
            usage: Some(WorkUsage::default()),
            artifacts: vec![WorkArtifactDraft {
                output: "summary".into(),
                title: "Derived result".into(),
                data: WorkArtifactDataV1::Checklist {
                    items: vec![WorkChecklistItem {
                        text: "Review the selected predecessor result".into(),
                        completed: false,
                    }],
                },
                evidence: vec![],
            }],
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        completed.executions[0].status,
        WorkExecutionStatus::Completed
    );
    assert_eq!(completed.executions[0].artifacts.len(), 2);
    let approve = handle
        .work_command(
            profile,
            WorkCommandV1 {
                version: 1,
                work,
                expected_revision: completed.work.revision,
                command: WorkCommandId::generate(),
                intent: WorkRuntimeIntent::Approve { spec },
            },
        )
        .unwrap();
    let WorkReply::RuntimeCommand {
        projection: approved,
        receipt,
    } = drive(&mut shell, &queue, approve).await.unwrap().reply
    else {
        panic!()
    };
    let attempt = drive(
        &mut shell,
        &queue,
        service.begin_node(
            profile,
            work,
            approved.work.revision,
            receipt.execution,
            plan.draft.nodes[0].id,
        ),
    )
    .await
    .unwrap();
    let dropped = attempt.attempt();
    drop(attempt);
    let state = drive(
        &mut shell,
        &queue,
        handle.work_projection(profile, work).unwrap(),
    )
    .await
    .unwrap();
    let WorkReply::Runtime(state) = state.reply else {
        panic!()
    };
    let unknown = state
        .executions
        .iter()
        .find(|e| e.id == receipt.execution)
        .unwrap();
    assert_eq!(unknown.status, WorkExecutionStatus::Interrupted);
    assert_eq!(unknown.attempts[0].id, dropped);
    assert_eq!(unknown.attempts[0].usage, None);
    drop(service);
    drop(handle);
    drop(shell);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
    drop(store);
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, _) = fixture(store.clone());
    let WorkReply::Runtime(reopened) = drive(
        &mut shell,
        &queue,
        handle.work_projection(profile, work).unwrap(),
    )
    .await
    .unwrap()
    .reply
    else {
        panic!()
    };
    assert_eq!(reopened, state);
    drop(handle);
    drop(shell);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
}
