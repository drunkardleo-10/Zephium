use super::work_planning_tests::{drive, fixture};
use crate::{work_execution::*, work_runtime::*};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use zephium_core::{
    ports::store::Store,
    work::{artifact::*, port::*, proposal::*, runtime::*, synthesis::*, *},
};
use zephium_ipc::work::*;

struct Primary;
impl WorkSynthesisProvider for Primary {
    fn produce<'a>(&'a self, input: &'a WorkSynthesisDisclosure) -> WorkSynthesisFuture<'a> {
        Box::pin(async move {
            assert_eq!(input.context().sources.len(), 1);
            assert!(input.context().objective.contains("Desktop"));
            Ok(WorkSynthesisResult {
                outputs: vec![WorkSynthesisOutput {
                    output: 0,
                    title: "Summary".into(),
                    data: WorkArtifactDataV1::Document {
                        paragraphs: vec!["Review changes before release.".into()],
                        formatted: None,
                    },
                    evidence: vec![],
                }],
                usage: WorkUsage {
                    model_tokens: 400,
                    cost_micro_usd: 100,
                    operations: 1,
                    accounting: WorkUsageAccounting::ConservativeReservation,
                },
            })
        })
    }
}
fn projection(response: WorkResponseV1) -> WorkProjectionV1 {
    match response.reply {
        WorkReplyV1::Projection { projection }
        | WorkReplyV1::ExecutionApplied { projection, .. } => *projection,
        other => panic!("unexpected product reply: {other:?}"),
    }
}

#[tokio::test]
async fn product_commands_execute_review_edit_and_reopen_without_mutating_original_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, profile) = fixture(store.clone());
    let create: WorkAuthoringCommandV1 = serde_json::from_value(serde_json::json!({
        "version": 1, "command": WorkCommandId::generate(),
        "intent": { "kind": "create", "objective": "Prepare release checks and a summary" }
    }))
    .unwrap();
    let created = drive(
        &mut shell,
        &queue,
        handle
            .work_authoring_command(profile, create)
            .unwrap()
            .response(profile),
    )
    .await;
    let WorkReplyV1::AuthoringApplied { receipt } = created.reply else {
        panic!()
    };
    let work = receipt.work;
    let author = |intent| {
        handle
            .work_authoring_command(
                profile,
                WorkAuthoringCommandV1 {
                    version: 1,
                    command: WorkCommandId::generate(),
                    intent,
                },
            )
            .unwrap()
            .response(profile)
    };
    let read = || {
        handle
            .work_query(
                profile,
                WorkQueryV1 {
                    version: 1,
                    query: WorkQueryKindV1::Projection { work },
                },
            )
            .unwrap()
            .response(profile)
    };
    drive(
        &mut shell,
        &queue,
        author(WorkAuthoringIntent::Edit {
            work,
            expected_revision: receipt.applied_revision,
            edit: WorkUserEdit::OpenQuestion {
                prompt: "Which deployment?".into(),
                options: vec!["Desktop".into()],
            },
        }),
    )
    .await;
    let state = projection(drive(&mut shell, &queue, read()).await);
    drive(
        &mut shell,
        &queue,
        author(WorkAuthoringIntent::Edit {
            work,
            expected_revision: state.work.revision,
            edit: WorkUserEdit::AnswerQuestion {
                id: state.work.questions[0].id,
                answer: "Desktop".into(),
            },
        }),
    )
    .await;
    let state = projection(drive(&mut shell, &queue, read()).await);
    drive(
        &mut shell,
        &queue,
        author(WorkAuthoringIntent::Edit {
            work,
            expected_revision: state.work.revision,
            edit: WorkUserEdit::ReplaceDraft {
                proposal: WorkPlanProposal {
                    nodes: vec![
                        WorkNodeProposal {
                            key: 0,
                            objective: "Summarize the child's checks".into(),
                            dependencies: vec![1],
                            outputs: vec![WorkExpectedOutput {
                                name: "summary".into(),
                                description: "Summary for review".into(),
                                review: WorkOutputReview::UserAcceptance,
                            }],
                        },
                        WorkNodeProposal {
                            key: 1,
                            objective: "Suggest release checks".into(),
                            dependencies: vec![],
                            outputs: vec![WorkExpectedOutput {
                                name: "checks".into(),
                                description: "Checks for review".into(),
                                review: WorkOutputReview::UserAcceptance,
                            }],
                        },
                    ],
                },
            },
        }),
    )
    .await;
    let planned = projection(drive(&mut shell, &queue, read()).await);
    let primary = planned.work.plan.as_ref().unwrap().draft.nodes[0].id;
    let driver = WorkExecutionService::new(handle.clone());
    let preview = drive(
        &mut shell,
        &queue,
        driver.prepare_public_approval(
            profile,
            WorkApprovalRequestV1 {
                version: 1,
                work,
                expected_revision: planned.work.revision,
                limits: WorkExecutionLimits {
                    model_tokens: 16000,
                    cost_micro_usd: 20000,
                    operations: 4,
                    timeout_seconds: 60,
                    max_workers: 2,
                },
                scope: WorkBrowseScope {
                    start_url: "https://example.test/".into(),
                    routes: vec![WorkBrowseRoute {
                        origin: "https://example.test".into(),
                        path_prefix: "/".into(),
                    }],
                    max_hops: 2,
                },
                primary: Some(primary),
            },
        ),
    )
    .await
    .unwrap();
    assert!(projection(drive(&mut shell, &queue, read()).await)
        .executions
        .is_empty());
    let WorkReplyV1::ApprovalDraft { spec, .. } = preview.reply else {
        panic!()
    };
    let command = |state: &WorkProjectionV1, intent| {
        handle
            .work_command(
                profile,
                WorkCommandV1 {
                    version: 1,
                    work,
                    expected_revision: state.work.revision,
                    command: WorkCommandId::generate(),
                    intent,
                },
            )
            .unwrap()
            .response(profile)
    };
    let approved = projection(
        drive(
            &mut shell,
            &queue,
            command(&planned, WorkRuntimeIntent::Approve { spec }),
        )
        .await,
    );
    let execution = approved.executions[0].id;
    let start = WorkStartRequestV1 {
        version: 1,
        work,
        expected_revision: approved.work.revision,
        execution,
    };
    let mut calls = 0;
    let mut observers = Vec::new();
    let mut state = drive(
        &mut shell,
        &queue,
        driver.execute_request(
            profile,
            start.clone(),
            &Primary,
            |attempt| {
                calls += 1;
                assert_eq!(attempt.decisions().len(), 1);
                assert_eq!(attempt.decisions()[0].answer, "Desktop");
                async move {
                    let output = attempt.node().outputs[0].name.clone();
                    attempt
                        .settle_owned(WorkAdapterResult {
                            status: WorkAttemptStatus::Succeeded,
                            intervention: None,
                            usage: Some(WorkUsage {
                                operations: 1,
                                ..WorkUsage::default()
                            }),
                            artifacts: vec![WorkArtifactDraft {
                                output,
                                title: "Checks".into(),
                                data: WorkArtifactDataV1::Checklist {
                                    items: vec![WorkChecklistItem {
                                        text: "Review changes".into(),
                                        completed: false,
                                    }],
                                },
                                evidence: vec![],
                            }],
                        })
                        .await
                }
            },
            |observer| observers.push(observer),
        ),
    )
    .await
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(observers.len(), 2);
    assert!(observers.iter().all(|o| o.latest().is_none()));
    assert_eq!(state.executions[0].status, WorkExecutionStatus::NeedsReview);
    assert_eq!(state.executions[0].artifacts.len(), 2);
    assert!(matches!(
        drive(
            &mut shell,
            &queue,
            driver.execute_request(
                profile,
                start,
                &Primary,
                |_| async { panic!("replay must not dispatch a browser") },
                |_| {}
            )
        )
        .await,
        Err(WorkError::Conflict)
    ));
    let original = state.executions[0].artifacts.clone();
    let artifact = original.last().unwrap().id;
    state = projection(
        drive(
            &mut shell,
            &queue,
            command(
                &state,
                WorkRuntimeIntent::ReviewArtifact {
                    execution,
                    artifact,
                    decision: WorkArtifactDecision::Rejected,
                },
            ),
        )
        .await,
    );
    assert_eq!(state.executions[0].status, WorkExecutionStatus::NeedsReview);
    let bad_edit = command(
        &state,
        WorkRuntimeIntent::EditArtifact {
            execution,
            artifact,
            data: WorkArtifactDataV1::Document {
                paragraphs: vec!["User correction".into()],
                formatted: None,
            },
            evidence: vec![WorkEvidenceLink {
                extraction_id: WorkArtifactId::generate(),
                source_id: 1,
            }],
        },
    );
    assert!(matches!(
        drive(&mut shell, &queue, bad_edit).await.reply,
        WorkReplyV1::Error {
            error: WorkFailureV1::Invalid
        }
    ));
    assert_eq!(projection(drive(&mut shell, &queue, read()).await), state);
    state = projection(
        drive(
            &mut shell,
            &queue,
            command(
                &state,
                WorkRuntimeIntent::EditArtifact {
                    execution,
                    artifact,
                    data: WorkArtifactDataV1::Document {
                        paragraphs: vec!["User correction".into()],
                        formatted: None,
                    },
                    evidence: vec![],
                },
            ),
        )
        .await,
    );
    assert_eq!(state.executions[0].user_artifacts[0].decision, None);
    for artifact in &original {
        state = projection(
            drive(
                &mut shell,
                &queue,
                command(
                    &state,
                    WorkRuntimeIntent::ReviewArtifact {
                        execution,
                        artifact: artifact.id,
                        decision: WorkArtifactDecision::Accepted,
                    },
                ),
            )
            .await,
        );
    }
    assert_eq!(state.executions[0].status, WorkExecutionStatus::Completed);
    assert_eq!(state.executions[0].artifacts, original);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
    let reopened = zephium_store::SqliteStore::open(dir.path()).unwrap();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    reopened
        .work_document(
            profile,
            WorkRequest::RuntimeRead { id: work },
            Box::new(move |reply| {
                tx.send(reply).unwrap();
            }),
        )
        .unwrap();
    let WorkReply::Runtime(restored) = rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap()
    else {
        panic!()
    };
    assert_eq!(*restored, state);
    assert_eq!(
        reopened.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
}
