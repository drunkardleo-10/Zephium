use super::work_planning_tests::{drive, fixture};
use super::work_runtime_tests::SynthesisFixture;
use crate::{work_runtime::*, WorkIntent, WorkUserEdit};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use zephium_core::work::{artifact::*, port::*, proposal::*, runtime::*, synthesis::*, *};
use zephium_ipc::work::WorkCommandV1;

struct Primary;
impl WorkSynthesisProvider for Primary {
    fn produce<'a>(&'a self, input: &'a WorkSynthesisDisclosure) -> WorkSynthesisFuture<'a> {
        Box::pin(async move {
            assert_eq!(input.context().sources.len(), 1);
            assert_eq!(input.context().outputs[0].name, "summary");
            assert!(
                matches!(&input.context().sources[0].data, WorkArtifactDataV1::Checklist { items } if items[0].text == "Review changes")
            );
            Ok(WorkSynthesisResult {
                outputs: vec![WorkSynthesisOutput {
                    output: 0,
                    title: "Release summary".into(),
                    data: WorkArtifactDataV1::Document {
                        paragraphs: vec!["Review changes before release.".into()],
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

#[tokio::test]
async fn coordinator_joins_original_child_publication_and_fails_closed_on_loss() {
    let mut foreign = None;
    for mode in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
        let (mut shell, queue, handle, profile) = fixture(store.clone());
        let create = handle
            .work_document(WorkIntent::Create {
                objective: "Prepare release checks and a summary".into(),
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
                                objective: "Summarize the child's release checks".into(),
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
                                    name: "checklist".into(),
                                    description: "Suggested checks".into(),
                                    review: WorkOutputReview::UserAcceptance,
                                }],
                            },
                        ],
                    },
                },
            })
            .unwrap();
        let WorkReply::Snapshot(planned) = drive(&mut shell, &queue, edit).await.unwrap().reply
        else {
            panic!()
        };
        let plan = planned.plan.as_ref().unwrap();
        let root = plan
            .draft
            .nodes
            .iter()
            .find(|n| n.outputs[0].name == "summary")
            .unwrap()
            .id;
        let child = plan
            .draft
            .nodes
            .iter()
            .find(|n| n.outputs[0].name == "checklist")
            .unwrap()
            .id;
        let limits = WorkExecutionLimits {
            model_tokens: 8000,
            cost_micro_usd: 10000,
            operations: 2,
            timeout_seconds: 60,
            max_workers: 2,
        };
        let spec = WorkExecutionSpec {
            plan_revision: plan.revision,
            limits: WorkExecutionLimits {
                model_tokens: 16000,
                cost_micro_usd: 20000,
                operations: 4,
                ..limits
            },
            nodes: vec![
                WorkNodeExecutionSpec {
                    node: root,
                    parent: None,
                    capability: WorkCapability::Coordinate {
                        scope: WorkBrowseScope {
                            start_url: "https://example.test/".into(),
                            routes: vec![WorkBrowseRoute {
                                origin: "https://example.test".into(),
                                path_prefix: "/".into(),
                            }],
                            max_hops: 1,
                        },
                    },
                    limits,
                },
                WorkNodeExecutionSpec {
                    node: child,
                    parent: Some(root),
                    capability: WorkCapability::Synthesize,
                    limits,
                },
            ],
        };
        let command = handle
            .work_command(
                profile,
                WorkCommandV1 {
                    version: 1,
                    work,
                    expected_revision: planned.revision,
                    command: WorkCommandId::generate(),
                    intent: WorkRuntimeIntent::Approve { spec },
                },
            )
            .unwrap();
        let WorkReply::RuntimeCommand {
            projection,
            receipt,
        } = drive(&mut shell, &queue, command).await.unwrap().reply
        else {
            panic!()
        };
        let service = WorkRuntimeService::new(handle.clone());
        assert!(matches!(
            drive(
                &mut shell,
                &queue,
                service.begin_node(
                    profile,
                    work,
                    projection.work.revision,
                    receipt.execution,
                    child
                )
            )
            .await,
            Err(WorkError::Unavailable)
        ));
        let root = drive(
            &mut shell,
            &queue,
            service.begin_node(
                profile,
                work,
                projection.work.revision,
                receipt.execution,
                root,
            ),
        )
        .await
        .unwrap();
        let deadline = root.deadline();
        let mut coordinator = drive(&mut shell, &queue, root.coordinate()).await.unwrap();
        let model = SynthesisFixture(if mode == 1 { 2 } else { 0 }, None);
        if mode != 2 {
            let result = drive(
                &mut shell,
                &queue,
                coordinator.execute_child(child, |attempt| async {
                    assert!(attempt.deadline() <= deadline);
                    assert!(attempt.dependency_artifacts().is_empty());
                    if mode == 3 {
                        drop(attempt);
                        return Ok(foreign.take().unwrap());
                    }
                    if mode == 4 {
                        let WorkReply::Runtime(state) = handle
                            .work_projection(profile, work)
                            .unwrap()
                            .await
                            .unwrap()
                            .reply
                        else {
                            panic!()
                        };
                        handle
                            .work_command(
                                profile,
                                WorkCommandV1 {
                                    version: 1,
                                    work,
                                    expected_revision: state.work.revision,
                                    command: WorkCommandId::generate(),
                                    intent: WorkRuntimeIntent::Cancel {
                                        execution: receipt.execution,
                                    },
                                },
                            )
                            .unwrap()
                            .await
                            .unwrap();
                        std::future::pending::<()>().await;
                        drop(attempt);
                        unreachable!();
                    }
                    attempt.synthesize_owned(&model).await
                }),
            )
            .await;
            assert_eq!(result.is_ok(), mode == 0);
            if mode == 3 {
                assert_eq!(result, Err(WorkError::Invalid));
            }
            if mode == 4 {
                assert_eq!(result, Err(WorkError::OutcomeUnknown));
            }
            // One-shot delivery; this callback must never execute twice.
            assert!(drive(
                &mut shell,
                &queue,
                coordinator
                    .execute_child(child, |_| async { panic!("duplicate child dispatched") })
            )
            .await
            .is_err());
        }
        let settlement = drive(&mut shell, &queue, coordinator.finish(&Primary))
            .await
            .unwrap();
        let result = settlement.projection().clone();
        if mode == 0 {
            foreign = Some(settlement);
        }
        let fact = &result.executions[0];
        assert_eq!(
            fact.status,
            match mode {
                0 => WorkExecutionStatus::NeedsReview,
                1 | 3 | 4 => WorkExecutionStatus::Interrupted,
                _ => WorkExecutionStatus::Failed,
            }
        );
        assert_eq!(fact.artifacts.len(), if mode == 0 { 2 } else { 0 });
        if mode == 0 {
            assert_eq!(
                fact.attempts
                    .iter()
                    .map(|a| a.usage.unwrap().model_tokens)
                    .sum::<u32>(),
                700
            );
        } else {
            assert_eq!(fact.attempts[0].usage, Some(WorkUsage::default()));
        }
        drop(service);
        drop(handle);
        drop(shell);
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            zephium_core::ports::store::StoreShutdownOutcome::Clean
        );
        drop(store);
        let reopened = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
        let (mut shell, queue, handle, _) = fixture(reopened.clone());
        let WorkReply::Runtime(read) = drive(
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
        assert_eq!(*read, result);
        drop(handle);
        drop(shell);
        assert_eq!(
            reopened.shutdown_until(Instant::now() + Duration::from_secs(5)),
            zephium_core::ports::store::StoreShutdownOutcome::Clean
        );
    }
}
