use super::*;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use zephium_core::work::search::*;
fn scope() -> WorkPublicSearchScope {
    WorkPublicSearchScope {
        provider: WorkSearchProvider::OpenAi,
        model: PUBLIC_SEARCH_MODEL.into(),
        query: "public release information".into(),
    }
}
struct SearchFixture {
    mode: u8,
    called: AtomicUsize,
    dropped: AtomicBool,
    cancel: Option<(crate::Handle, zephium_core::ids::ProfileId, WorkCommandV1)>,
}
struct DropNotice<'a>(&'a AtomicBool);
impl Drop for DropNotice<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
impl WorkPublicSearchProvider for SearchFixture {
    fn search<'a>(
        &'a self,
        actual: &'a WorkPublicSearchScope,
        _: WorkExecutionLimits,
    ) -> WorkPublicSearchFuture<'a> {
        Box::pin(async move {
            assert_eq!(actual, &scope());
            self.called.fetch_add(1, Ordering::AcqRel);
            let _notice = DropNotice(&self.dropped);
            if let Some((handle, profile, command)) = &self.cancel {
                handle
                    .work_command(*profile, command.clone())
                    .unwrap()
                    .await
                    .unwrap();
                return std::future::pending().await;
            }
            if self.mode == 5 {
                return std::future::pending().await;
            }
            Ok(WorkPublicSearchResult {
                evidence: WorkProviderSearchEvidenceV1 {
                    version: 1,
                    provider: WorkSearchProvider::OpenAi,
                    model: if self.mode == 1 {
                        "other-model".into()
                    } else {
                        PUBLIC_SEARCH_MODEL.into()
                    },
                    response_model: PUBLIC_SEARCH_MODEL.into(),
                    response_id: "resp_test".into(),
                    search_call_id: "ws_test".into(),
                    answer: if self.mode == 6 {
                        "x".repeat(32768)
                    } else if self.mode == 7 {
                        "界".repeat(4000)
                    } else {
                        "Public answer [1].".into()
                    },
                    citations: if self.mode == 7 {
                        (0..[7, 19, 11][self.called.load(Ordering::Acquire) - 1])
                            .map(|index| WorkProviderSearchCitation {
                                url: format!("https://example.com/source/{index}"),
                                title: "Public source".into(),
                                start_index: 100 + index * 150,
                                end_index: 120 + index * 150,
                            })
                            .collect()
                    } else {
                        vec![WorkProviderSearchCitation {
                            url: "https://example.com/source".into(),
                            title: "Source".into(),
                            start_index: 14,
                            end_index: 17,
                        }]
                    },
                    actual_input_tokens: 200,
                    actual_output_tokens: 100,
                },
                usage: WorkUsage {
                    model_tokens: match self.mode {
                        2 => 9000,
                        3 => 301,
                        _ => 300,
                    },
                    cost_micro_usd: 50,
                    operations: 1,
                    accounting: WorkUsageAccounting::ConservativeReservation,
                },
            })
        })
    }
}
#[tokio::test]
async fn original_search_worker_publishes_exact_sources_and_rejects_mismatched_results() {
    for mode in 0..6 {
        search_case(mode).await;
    }
}
async fn search_case(mode: u8) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, profile) = fixture(store.clone());
    let create = handle
        .work_document(WorkIntent::Create {
            objective: "Prepare a release checklist".into(),
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
                    nodes: vec![WorkNodeProposal {
                        key: 0,
                        objective: "Search public release information".into(),
                        dependencies: vec![],
                        outputs: vec![WorkExpectedOutput {
                            name: "checklist".into(),
                            description: "Suggested checks for user review".into(),
                            review: WorkOutputReview::UserAcceptance,
                        }],
                    }],
                },
            },
        })
        .unwrap();
    let WorkReply::Snapshot(planned) = drive(&mut shell, &queue, edit).await.unwrap().reply else {
        panic!()
    };
    let plan = planned.plan.as_ref().unwrap();
    let node = plan.draft.nodes[0].id;
    let limits = WorkExecutionLimits {
        model_tokens: 8000,
        cost_micro_usd: 10000,
        operations: 2,
        timeout_seconds: 60,
        max_workers: 1,
    };
    let approve = handle
        .work_command(
            profile,
            WorkCommandV1 {
                version: 1,
                work,
                expected_revision: planned.revision,
                command: WorkCommandId::generate(),
                intent: WorkRuntimeIntent::Approve {
                    spec: WorkExecutionSpec {
                        plan_revision: plan.revision,
                        limits,
                        nodes: vec![WorkNodeExecutionSpec {
                            node,
                            parent: None,
                            capability: WorkCapability::PublicSearch { scope: scope() },
                            limits,
                        }],
                    },
                },
            },
        )
        .unwrap();
    let WorkReply::RuntimeCommand {
        projection,
        receipt,
    } = drive(&mut shell, &queue, approve).await.unwrap().reply
    else {
        panic!()
    };
    let runtime = WorkRuntimeService::new(handle.clone());
    let attempt = drive(
        &mut shell,
        &queue,
        runtime.begin_node(
            profile,
            work,
            projection.work.revision,
            receipt.execution,
            node,
        ),
    )
    .await
    .unwrap();
    let model = SearchFixture {
        mode,
        called: AtomicUsize::new(0),
        dropped: AtomicBool::new(false),
        cancel: (mode == 4).then(|| {
            (
                handle.clone(),
                profile,
                WorkCommandV1 {
                    version: 1,
                    work,
                    expected_revision: projection.work.revision.next().unwrap(),
                    command: WorkCommandId::generate(),
                    intent: WorkRuntimeIntent::Cancel {
                        execution: receipt.execution,
                    },
                },
            )
        }),
    };
    let outcome = if mode == 5 {
        let mut operation = Box::pin(attempt.search_public_owned(&model));
        drive(
            &mut shell,
            &queue,
            std::future::poll_fn(|cx| {
                let polled = operation.as_mut().poll(cx);
                assert!(polled.is_pending());
                if model.called.load(Ordering::Acquire) > 0 {
                    std::task::Poll::Ready(())
                } else {
                    std::task::Poll::Pending
                }
            }),
        )
        .await;
        drop(operation);
        None
    } else {
        Some(drive(&mut shell, &queue, attempt.search_public_owned(&model)).await)
    };
    assert_eq!(model.called.load(Ordering::Acquire), 1);
    assert!(model.dropped.load(Ordering::Acquire));
    #[cfg(feature = "work-execution")]
    {
        assert!(shell.work.is_none());
        assert!(shell.retained_work.is_none());
    }
    let WorkReply::Runtime(state) = drive(
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
    let execution = &state.executions[0];
    if mode == 0 {
        let settlement = outcome.unwrap().unwrap();
        assert_eq!(settlement.work(), work);
        assert_eq!(settlement.node(), node);
        assert_eq!(settlement.execution(), receipt.execution);
        assert_eq!(execution.attempts[0].status, WorkAttemptStatus::Succeeded);
        assert_eq!(execution.provider_evidence.len(), 1);
        assert_eq!(execution.artifacts.len(), 1);
        let evidence = &execution.provider_evidence[0];
        let link = execution.artifacts[0].evidence[0].clone();
        assert_eq!(link.extraction_id, evidence.id);
        assert_eq!(link.source_id, 1);
        assert_eq!(evidence.attempt, execution.attempts[0].id);
        assert_eq!(evidence.node, node);
        let read = handle
            .submit_owned_work_runtime(
                WorkRequest::ReadEvidence {
                    id: work,
                    link: link.clone(),
                },
                profile,
            )
            .unwrap();
        let WorkReply::Evidence(preview) = drive(&mut shell, &queue, read).await.unwrap().reply
        else {
            panic!()
        };
        assert_eq!(preview.link, link);
        assert!(preview.text.contains("[1]"));
        let other = handle
            .work_document(WorkIntent::Create {
                objective: "Different objective".into(),
            })
            .unwrap();
        let other_work = other.work_id().unwrap();
        drive(&mut shell, &queue, other).await.unwrap();
        let foreign = handle
            .submit_owned_work_runtime(
                WorkRequest::ReadEvidence {
                    id: other_work,
                    link: link.clone(),
                },
                profile,
            )
            .unwrap();
        assert!(
            drive(&mut shell, &queue, foreign).await.is_err(),
            "a provider source cannot cross its original Work publication"
        );
        let invalid = handle
            .submit_owned_work_runtime(
                WorkRequest::ReadEvidence {
                    id: work,
                    link: WorkEvidenceLink {
                        source_id: 2,
                        ..link
                    },
                },
                profile,
            )
            .unwrap();
        assert!(drive(&mut shell, &queue, invalid).await.is_err());
    } else {
        assert!(execution.artifacts.is_empty());
        assert!(execution.provider_evidence.is_empty());
        assert_eq!(
            execution.attempts[0].status,
            WorkAttemptStatus::OutcomeUnknown
        );
        assert!(execution.attempts[0].usage.is_none());
    }
    assert!(store.flush());
    drop(runtime);
    drop(handle);
    drop(shell);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
    drop(store);
    let reopened_store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut reopened_shell, reopened_queue, reopened_handle, _) = fixture(reopened_store.clone());
    let WorkReply::Runtime(reopened) = drive(
        &mut reopened_shell,
        &reopened_queue,
        reopened_handle.work_projection(profile, work).unwrap(),
    )
    .await
    .unwrap()
    .reply
    else {
        panic!()
    };
    assert_eq!(*reopened, *state);
    drop(reopened_handle);
    drop(reopened_shell);
    assert_eq!(
        reopened_store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
}

#[tokio::test]
async fn original_search_worker_capacity_failure_retains_usage_and_first_publication() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, profile) = fixture(store.clone());
    let create = handle
        .work_document(WorkIntent::Create {
            objective: "Public research".into(),
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
                    nodes: (0..2)
                        .map(|key| WorkNodeProposal {
                            key,
                            objective: "Independent public search".into(),
                            dependencies: vec![],
                            outputs: (0..8)
                                .map(|index| WorkExpectedOutput {
                                    name: format!("source{index}"),
                                    description: "Public evidence".into(),
                                    review: WorkOutputReview::SourceMappedNeedsReview,
                                })
                                .collect(),
                        })
                        .collect(),
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
        operations: 2,
        timeout_seconds: 60,
        max_workers: 1,
    };
    let nodes: Vec<_> = plan.draft.nodes.iter().map(|node| node.id).collect();
    let approve = handle
        .work_command(
            profile,
            WorkCommandV1 {
                version: 1,
                work,
                expected_revision: planned.revision,
                command: WorkCommandId::generate(),
                intent: WorkRuntimeIntent::Approve {
                    spec: WorkExecutionSpec {
                        plan_revision: plan.revision,
                        limits,
                        nodes: nodes
                            .iter()
                            .map(|node| WorkNodeExecutionSpec {
                                node: *node,
                                parent: None,
                                capability: WorkCapability::PublicSearch { scope: scope() },
                                limits: WorkExecutionLimits {
                                    model_tokens: 4000,
                                    cost_micro_usd: 5000,
                                    operations: 1,
                                    ..limits
                                },
                            })
                            .collect(),
                    },
                },
            },
        )
        .unwrap();
    let WorkReply::RuntimeCommand {
        projection,
        receipt,
    } = drive(&mut shell, &queue, approve).await.unwrap().reply
    else {
        panic!()
    };
    let runtime = WorkRuntimeService::new(handle.clone());
    let model = SearchFixture {
        mode: 6,
        called: AtomicUsize::new(0),
        dropped: AtomicBool::new(false),
        cancel: None,
    };
    let mut state = projection;
    let mut original = None;
    for (index, node) in nodes.iter().enumerate() {
        let attempt = drive(
            &mut shell,
            &queue,
            runtime.begin_node(profile, work, state.work.revision, receipt.execution, *node),
        )
        .await
        .unwrap();
        let attempt_id = attempt.attempt();
        let settled = drive(&mut shell, &queue, attempt.search_public_owned(&model))
            .await
            .unwrap_or_else(|error| panic!("publication {index}: {error:?}"));
        *state = settled.into_projection();
        let execution = &state.executions[0];
        let fact = execution
            .attempts
            .iter()
            .find(|fact| fact.id == attempt_id)
            .unwrap();
        assert_eq!(
            fact.status,
            if index == 0 {
                WorkAttemptStatus::Succeeded
            } else {
                WorkAttemptStatus::Failed
            }
        );
        assert_eq!(
            fact.usage,
            Some(WorkUsage {
                model_tokens: 300,
                cost_micro_usd: 50,
                operations: 1,
                accounting: WorkUsageAccounting::ConservativeReservation
            })
        );
        assert_eq!(execution.artifacts.len(), 8);
        assert_eq!(execution.provider_evidence.len(), 1);
        if index == 0 {
            assert!(serde_json::to_vec(execution).unwrap().len() > 262144);
            original = Some((
                execution.artifacts.clone(),
                execution.provider_evidence.clone(),
            ));
        } else {
            let (artifacts, evidence) = original.as_ref().unwrap();
            assert!(execution.artifacts == *artifacts);
            assert!(execution.provider_evidence == *evidence);
        }
        assert_eq!(
            model.called.load(Ordering::Acquire),
            index + 1,
            "publication retry must never redispatch search"
        );
    }
    #[cfg(feature = "work-execution")]
    {
        assert!(shell.work.is_none());
        assert!(shell.retained_work.is_none());
    }
    assert!(store.flush());
    drop(runtime);
    drop(handle);
    drop(shell);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
    drop(store);
    let reopened_store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, _) = fixture(reopened_store.clone());
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
    assert_eq!(*reopened, *state);
    drop(handle);
    drop(shell);
    assert_eq!(
        reopened_store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
}

struct LargeSearchPrimary;
impl zephium_core::work::synthesis::WorkSynthesisProvider for LargeSearchPrimary {
    fn produce<'a>(
        &'a self,
        input: &'a zephium_core::work::synthesis::WorkSynthesisDisclosure,
    ) -> zephium_core::work::synthesis::WorkSynthesisFuture<'a> {
        Box::pin(async move {
            use zephium_core::work::synthesis::*;
            assert_eq!(input.context().sources.len(), 3);
            assert_eq!(input.context().evidence.len(), 37);
            assert!(
                serde_json::to_vec(input.context()).unwrap().len() <= MAX_SYNTHESIS_CONTEXT_BYTES
            );
            for (source, count) in input.context().sources.iter().zip([7, 19, 11]) {
                assert_eq!(source.evidence.len(), count);
                assert!(
                    matches!(&source.data, WorkArtifactDataV1::EvidenceCollection { summary, .. } if summary.contains("Host-truncated dependency summary"))
                );
            }
            assert!(input
                .context()
                .evidence
                .iter()
                .all(|item| item.truncated && item.text.chars().count() >= 128));
            Ok(WorkSynthesisResult {
                outputs: vec![WorkSynthesisOutput {
                    output: 0,
                    title: "Combined evidence".into(),
                    data: WorkArtifactDataV1::Document {
                        paragraphs: vec!["Reviewed public evidence".into()],
                        formatted: None,
                    },
                    evidence: (0..37).collect(),
                }],
                usage: WorkUsage {
                    model_tokens: 300,
                    cost_micro_usd: 50,
                    operations: 1,
                    accounting: WorkUsageAccounting::ConservativeReservation,
                },
            })
        })
    }
}
#[tokio::test]
async fn delegated_search_large_child_results_keep_exact_sources_and_bounded_synthesis() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
    let (mut shell, queue, handle, profile) = fixture(store.clone());
    let create = handle
        .work_document(WorkIntent::Create {
            objective: "Public research".into(),
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
                    nodes: (0..4)
                        .map(|key| WorkNodeProposal {
                            key,
                            objective: "Independent public search".into(),
                            dependencies: if key == 3 { vec![0, 1, 2] } else { vec![] },
                            outputs: (0..1)
                                .map(|index| WorkExpectedOutput {
                                    name: format!("source{index}"),
                                    description: "Public evidence".into(),
                                    review: WorkOutputReview::SourceMappedNeedsReview,
                                })
                                .collect(),
                        })
                        .collect(),
                },
            },
        })
        .unwrap();
    let WorkReply::Snapshot(planned) = drive(&mut shell, &queue, edit).await.unwrap().reply else {
        panic!()
    };
    let plan = planned.plan.as_ref().unwrap();
    let limits = WorkExecutionLimits {
        model_tokens: 32000,
        cost_micro_usd: 40000,
        operations: 8,
        timeout_seconds: 60,
        max_workers: 4,
    };
    let nodes: Vec<_> = plan.draft.nodes.iter().map(|node| node.id).collect();
    let approve = handle
        .work_command(
            profile,
            WorkCommandV1 {
                version: 1,
                work,
                expected_revision: planned.revision,
                command: WorkCommandId::generate(),
                intent: WorkRuntimeIntent::Approve {
                    spec: WorkExecutionSpec {
                        plan_revision: plan.revision,
                        limits,
                        nodes: nodes
                            .iter()
                            .map(|node| WorkNodeExecutionSpec {
                                node: *node,
                                parent: (*node != nodes[3]).then_some(nodes[3]),
                                capability: if *node == nodes[3] {
                                    WorkCapability::CoordinatePublicResearch {
                                        provider: WorkSearchProvider::OpenAi,
                                        model: PUBLIC_SEARCH_MODEL.into(),
                                        max_hops: 1,
                                    }
                                } else {
                                    WorkCapability::PublicSearch { scope: scope() }
                                },
                                limits: WorkExecutionLimits {
                                    model_tokens: 4000,
                                    cost_micro_usd: 5000,
                                    operations: 1,
                                    ..limits
                                },
                            })
                            .collect(),
                    },
                },
            },
        )
        .unwrap();
    let WorkReply::RuntimeCommand {
        projection,
        receipt,
    } = drive(&mut shell, &queue, approve).await.unwrap().reply
    else {
        panic!()
    };

    let runtime = WorkRuntimeService::new(handle.clone());
    let root = drive(
        &mut shell,
        &queue,
        runtime.begin_node(
            profile,
            work,
            projection.work.revision,
            receipt.execution,
            nodes[3],
        ),
    )
    .await
    .unwrap();
    let mut coordinator = drive(&mut shell, &queue, root.coordinate()).await.unwrap();
    let model = SearchFixture {
        mode: 7,
        called: AtomicUsize::new(0),
        dropped: AtomicBool::new(false),
        cancel: None,
    };
    for child in &nodes[..3] {
        drive(
            &mut shell,
            &queue,
            coordinator.execute_child(*child, |attempt| attempt.search_public_owned(&model)),
        )
        .await
        .unwrap();
    }
    let settled = drive(&mut shell, &queue, coordinator.finish(&LargeSearchPrimary))
        .await
        .unwrap();
    let fact = &settled.projection().executions[0];
    assert_eq!(model.called.load(Ordering::Acquire), 3);
    assert!(fact
        .attempts
        .iter()
        .all(|attempt| attempt.status == WorkAttemptStatus::Succeeded));
    assert_eq!(fact.provider_evidence.len(), 3);
    for (record, count) in fact.provider_evidence.iter().zip([7, 19, 11]) {
        assert_eq!(record.evidence.answer, "界".repeat(4000));
        let artifact = fact
            .artifacts
            .iter()
            .find(|artifact| artifact.attempt == record.attempt)
            .unwrap();
        assert!(
            matches!(&artifact.data, WorkArtifactDataV1::EvidenceCollection { summary, .. } if *summary == record.evidence.answer)
        );
        assert_eq!(artifact.evidence.len(), count);
        assert!(artifact
            .evidence
            .iter()
            .all(|link| link.extraction_id == record.id));
    }
    let parent = fact
        .artifacts
        .iter()
        .find(|artifact| artifact.node == nodes[3])
        .unwrap();
    assert_eq!(parent.evidence.len(), 37);
    assert!(store.flush());
}

#[tokio::test]
async fn direct_public_read_dispatches_only_after_fresh_receipt_and_never_on_replay() {
    use crate::work_execution::WorkExecutionService;
    for dormant in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
        let (mut shell, queue, handle, profile) = fixture(store.clone());
        let create = handle
            .work_document(WorkIntent::Create {
                objective: scope().query,
            })
            .unwrap();
        let work = create.work_id().unwrap();
        drive(&mut shell, &queue, create).await.unwrap();
        let command = WorkCommandV1 {
            version: 1,
            work,
            expected_revision: WorkRevision::INITIAL,
            command: WorkCommandId::generate(),
            intent: WorkRuntimeIntent::ReadPublic {
                scope: scope(),
                limits: WorkExecutionLimits {
                    model_tokens: 147456,
                    cost_micro_usd: 100000,
                    operations: 1,
                    timeout_seconds: 180,
                    max_workers: 1,
                },
            },
        };
        let service = WorkExecutionService::new(handle.clone());
        let model = SearchFixture {
            mode: 0,
            called: AtomicUsize::new(0),
            dropped: AtomicBool::new(false),
            cancel: None,
        };
        if dormant {
            // Simulate durable admission whose original callback was lost before Begin.
            drive(
                &mut shell,
                &queue,
                handle.work_command(profile, command.clone()).unwrap(),
            )
            .await
            .unwrap();
        }
        let state = drive(
            &mut shell,
            &queue,
            service.read_public(
                profile,
                command.clone(),
                |attempt| attempt.search_public_owned(&model),
                |_| {},
            ),
        )
        .await
        .unwrap();
        assert_eq!(model.called.load(Ordering::Acquire), usize::from(!dormant));
        assert_eq!(state.executions[0].attempts.len(), usize::from(!dormant));
        let replay = drive(
            &mut shell,
            &queue,
            service.read_public(
                profile,
                command.clone(),
                |_| async { panic!("replay cannot authorize a worker") },
                |_| {},
            ),
        )
        .await
        .unwrap();
        assert_eq!(state, replay);
        if dormant {
            let refused = drive(
                &mut shell,
                &queue,
                service.execute(
                    crate::work_execution::WorkExecutionRequest {
                        profile,
                        work,
                        expected_revision: state.work.revision,
                        execution: state.executions[0].id,
                    },
                    &SynthesisFixture(0, None),
                    |_| async { panic!("ordinary Start cannot dispatch direct read") },
                    |_| {},
                ),
            )
            .await;
            assert!(refused.is_err());
            drive(
                &mut shell,
                &queue,
                handle
                    .work_command(
                        profile,
                        WorkCommandV1 {
                            version: 1,
                            work,
                            expected_revision: state.work.revision,
                            command: WorkCommandId::generate(),
                            intent: WorkRuntimeIntent::Cancel {
                                execution: state.executions[0].id,
                            },
                        },
                    )
                    .unwrap(),
            )
            .await
            .unwrap();
        }
        drop(service);
        drop(handle);
        drop(shell);
        assert!(matches!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            zephium_core::ports::store::StoreShutdownOutcome::Clean
        ));
        let reopened_store = Arc::new(zephium_store::SqliteStore::open(dir.path()).unwrap());
        let (mut shell, queue, handle, _) = fixture(reopened_store.clone());
        let service = WorkExecutionService::new(handle.clone());
        let replay = drive(
            &mut shell,
            &queue,
            service.read_public(
                profile,
                command,
                |_| async { panic!("restart cannot recreate original dispatch") },
                |_| {},
            ),
        )
        .await
        .unwrap();
        assert_eq!(replay.executions.len(), 1);
        assert_eq!(replay.executions[0].attempts.len(), usize::from(!dormant));
        drop(service);
        drop(handle);
        drop(shell);
        assert!(matches!(
            reopened_store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            zephium_core::ports::store::StoreShutdownOutcome::Clean
        ));
    }
}
