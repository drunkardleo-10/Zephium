use super::super::tests::{create, draft, edit, session};
use super::*;

#[test]
fn coordination_requires_exact_live_parent_and_complete_children() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let mut graph = draft();
    graph.nodes[0].outputs[0].review = WorkOutputReview::UserAcceptance;
    let root = graph.nodes[0].id;
    let mut child = graph.nodes[0].clone();
    child.id = 13.into();
    graph.nodes[0].dependencies = vec![child.id];
    graph.nodes.push(child);
    let planned = edit(&mut hub, &initial, WorkEdit::ReplaceDraft { draft: graph }).unwrap();
    let mut execution_spec = spec(planned.plan.as_ref().unwrap());
    execution_spec.limits.max_workers = 2;
    execution_spec.limits.model_tokens *= 2;
    execution_spec.limits.cost_micro_usd *= 2;
    execution_spec.limits.operations *= 2;
    execution_spec.nodes[0].capability = WorkCapability::Coordinate {
        scope: WorkBrowseScope {
            start_url: "https://example.test/".into(),
            routes: vec![WorkBrowseRoute {
                origin: "https://example.test".into(),
                path_prefix: "/".into(),
            }],
            max_hops: 1,
        },
    };
    execution_spec.nodes[1].parent = Some(root);
    let WorkReply::RuntimeCommand { receipt, .. } = hub
        .work_document(
            planned.profile,
            WorkRequest::RuntimeCommand {
                id: planned.id,
                expected: planned.revision,
                command: 100.into(),
                intent: WorkRuntimeIntent::Approve {
                    spec: execution_spec,
                },
            },
        )
        .unwrap()
    else {
        panic!()
    };
    let execution = receipt.execution;
    let update = |hub: &mut Hub, update| {
        let state = read_runtime(hub, &planned);
        hub.work_document(
            planned.profile,
            WorkRequest::RuntimeUpdate {
                id: planned.id,
                expected: state.work.revision,
                update,
            },
        )
    };
    // Neither matching metadata nor a child-only Begin admits a child.
    assert!(matches!(
        update(
            &mut hub,
            WorkRuntimeUpdate::BeginChild {
                execution,
                attempt: 501.into(),
                node: 13.into(),
                parent: 500.into()
            }
        ),
        Err(WorkError::Unavailable)
    ));
    assert!(matches!(
        update(
            &mut hub,
            WorkRuntimeUpdate::Begin {
                execution,
                attempt: 501.into(),
                node: 13.into()
            }
        ),
        Err(WorkError::Unavailable)
    ));
    update(
        &mut hub,
        WorkRuntimeUpdate::Begin {
            execution,
            attempt: 500.into(),
            node: root,
        },
    )
    .unwrap();
    assert!(matches!(
        update(
            &mut hub,
            WorkRuntimeUpdate::BeginChild {
                execution,
                attempt: 501.into(),
                node: 13.into(),
                parent: 999.into()
            }
        ),
        Err(WorkError::Unavailable)
    ));
    let artifact = |node, attempt| artifact::WorkArtifactV1 {
        version: 1,
        id: WorkArtifactId::generate(),
        execution,
        node,
        attempt,
        output: "sources".into(),
        title: "Proposed checks".into(),
        data: artifact::WorkArtifactDataV1::Document {
            paragraphs: vec!["Review release changes".into()],
            formatted: None,
        },
        evidence: vec![],
        review: WorkOutputReview::UserAcceptance,
        presentation: artifact::WorkArtifactPresentationV1::Automatic,
    };
    assert!(matches!(
        update(
            &mut hub,
            WorkRuntimeUpdate::Settle {
                execution,
                attempt: 500.into(),
                status: WorkAttemptStatus::Succeeded,
                usage: Some(WorkUsage::default()),
                artifacts: vec![artifact(root, 500.into())]
            }
        ),
        Err(WorkError::Conflict)
    ));
    update(
        &mut hub,
        WorkRuntimeUpdate::BeginChild {
            execution,
            attempt: 501.into(),
            node: 13.into(),
            parent: 500.into(),
        },
    )
    .unwrap();
    update(
        &mut hub,
        WorkRuntimeUpdate::Settle {
            execution,
            attempt: 501.into(),
            status: WorkAttemptStatus::Succeeded,
            usage: Some(WorkUsage::default()),
            artifacts: vec![artifact(13.into(), 501.into())],
        },
    )
    .unwrap();
    update(
        &mut hub,
        WorkRuntimeUpdate::Settle {
            execution,
            attempt: 500.into(),
            status: WorkAttemptStatus::Succeeded,
            usage: Some(WorkUsage::default()),
            artifacts: vec![artifact(root, 500.into())],
        },
    )
    .unwrap();
    let state = read_runtime(&mut hub, &planned);
    assert_eq!(state.executions[0].status, WorkExecutionStatus::NeedsReview);
    // A canvas reference retains the exact historical result and does not
    // accept a valid artifact ID paired with another execution or profile.
    let original = &state.executions[0].artifacts[0];
    let conn = hub.profile_conn(planned.profile).unwrap();
    assert!(
        validate_artifact_reference(conn, planned.profile, planned.id, execution, original.id)
            .is_ok()
    );
    assert!(matches!(
        validate_artifact_reference(conn, planned.profile, planned.id, 999.into(), original.id),
        Err(WorkError::NotFound)
    ));
    assert!(matches!(
        validate_artifact_reference(conn, planned.profile, planned.id, execution, 999.into()),
        Err(WorkError::NotFound)
    ));
    // Profile authority selects the database at Hub admission; a caller cannot
    // choose a different profile label for an already-open connection.
    assert!(matches!(
        hub.work_document(999.into(), WorkRequest::RuntimeRead { id: planned.id }),
        Err(WorkError::ProfileUnavailable)
    ));
    // Durable decoding independently refuses an impossible parent success.
    let mut forged = state.executions[0].clone();
    forged.attempts.pop();
    forged.artifacts.retain(|a| a.node == root);
    assert!(forged
        .validate(planned.plan.as_ref().unwrap(), state.work.revision)
        .is_err());
}

fn spec(plan: &WorkPlanRevision) -> WorkExecutionSpec {
    let limits = WorkExecutionLimits {
        model_tokens: 8000,
        cost_micro_usd: 10000,
        operations: 32,
        timeout_seconds: 300,
        max_workers: 1,
    };
    WorkExecutionSpec {
        context: None,
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
                limits,
            })
            .collect(),
    }
}
fn approve(
    hub: &mut Hub,
    work: &WorkSnapshot,
    command: WorkCommandId,
) -> (WorkRuntimeProjection, WorkCommandReceipt) {
    let WorkReply::RuntimeCommand {
        projection,
        receipt,
    } = hub
        .work_document(
            work.profile,
            WorkRequest::RuntimeCommand {
                id: work.id,
                expected: work.revision,
                command,
                intent: WorkRuntimeIntent::Approve {
                    spec: spec(work.plan.as_ref().unwrap()),
                },
            },
        )
        .unwrap()
    else {
        panic!()
    };
    (*projection, receipt)
}
fn read_runtime(hub: &mut Hub, work: &WorkSnapshot) -> WorkRuntimeProjection {
    let WorkReply::Runtime(projection) = hub
        .work_document(work.profile, WorkRequest::RuntimeRead { id: work.id })
        .unwrap()
    else {
        panic!()
    };
    *projection
}
#[test]
fn approved_revision_is_idempotent_profile_bound_and_retained_after_compaction() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let planned = edit(
        &mut hub,
        &initial,
        WorkEdit::ReplaceDraft { draft: draft() },
    )
    .unwrap();
    let (approved, receipt) = approve(&mut hub, &planned, 100.into());
    let link = artifact::WorkEvidenceLink {
        extraction_id: 42_u128.into(),
        source_id: 1,
    };
    // An arbitrary archive identity cannot disclose content through an unrelated
    // Work, even to another Work in the same profile.
    assert!(matches!(
        hub.work_document(
            planned.profile,
            WorkRequest::ReadEvidence {
                id: planned.id,
                link: link.clone(),
            }
        ),
        Err(WorkError::NotFound)
    ));
    assert!(matches!(
        hub.work_document(
            2.into(),
            WorkRequest::ReadEvidence {
                id: planned.id,
                link,
            }
        ),
        Err(WorkError::NotFound)
    ));
    let (replayed, replay) = approve(&mut hub, &planned, 100.into());
    assert_eq!(receipt, replay);
    assert_eq!(approved.work.revision, replayed.work.revision);
    assert!(matches!(
        hub.work_document(2.into(), WorkRequest::RuntimeRead { id: planned.id }),
        Err(WorkError::NotFound)
    ));
    assert!(matches!(
        edit(
            &mut hub,
            &approved.work,
            WorkEdit::SetObjective {
                objective: "Change authority".into()
            }
        ),
        Err(WorkError::Conflict)
    ));
    assert!(matches!(
        hub.work_document(
            planned.profile,
            WorkRequest::RuntimeCommand {
                id: planned.id,
                expected: approved.work.revision,
                command: 100.into(),
                intent: WorkRuntimeIntent::Cancel {
                    execution: receipt.execution
                }
            }
        ),
        Err(WorkError::Conflict)
    ));
    hub.work_document(
        planned.profile,
        WorkRequest::RuntimeCommand {
            id: planned.id,
            expected: approved.work.revision,
            command: 101.into(),
            intent: WorkRuntimeIntent::Cancel {
                execution: receipt.execution,
            },
        },
    )
    .unwrap();
    let cancelled = read_runtime(&mut hub, &planned);
    let changed = edit(
        &mut hub,
        &cancelled.work,
        WorkEdit::ReplaceDraft { draft: draft() },
    )
    .unwrap();
    let compacted = edit(&mut hub, &changed, WorkEdit::CompactHistory).unwrap();
    assert!(matches!(
        hub.work_document(
            planned.profile,
            WorkRequest::ReadPlan {
                id: planned.id,
                revision: planned.plan.as_ref().unwrap().revision
            }
        ),
        Ok(WorkReply::Plan(_))
    ));
    assert_eq!(
        read_runtime(&mut hub, &compacted).executions[0].status,
        WorkExecutionStatus::Cancelled
    );
    hub.work_document(
        planned.profile,
        WorkRequest::Delete {
            id: planned.id,
            expected: compacted.revision,
        },
    )
    .unwrap();
}
#[test]
fn restart_never_recreates_authority_and_preserves_unknown_reservation() {
    let dir = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(dir.path().into()).unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let planned = edit(
        &mut hub,
        &initial,
        WorkEdit::ReplaceDraft { draft: draft() },
    )
    .unwrap();
    let (approved, receipt) = approve(&mut hub, &planned, 100.into());
    let attempt = WorkAttemptId::from(500);
    hub.work_document(
        planned.profile,
        WorkRequest::RuntimeUpdate {
            id: planned.id,
            expected: approved.work.revision,
            update: WorkRuntimeUpdate::Begin {
                execution: receipt.execution,
                attempt,
                node: planned.plan.as_ref().unwrap().draft.nodes[0].id,
            },
        },
    )
    .unwrap();
    drop(hub);
    let mut hub = Hub::open(dir.path().into()).unwrap();
    let interrupted = read_runtime(&mut hub, &planned);
    assert_eq!(interrupted.interrupted, vec![receipt.execution]);
    assert!(matches!(
        hub.work_document(
            planned.profile,
            WorkRequest::RuntimeUpdate {
                id: planned.id,
                expected: interrupted.work.revision,
                update: WorkRuntimeUpdate::Settle {
                    execution: receipt.execution,
                    attempt,
                    status: WorkAttemptStatus::Failed,
                    usage: Some(WorkUsage::default()),
                    artifacts: vec![]
                }
            }
        ),
        Err(WorkError::Conflict)
    ));
    hub.work_document(
        planned.profile,
        WorkRequest::RuntimeCommand {
            id: planned.id,
            expected: interrupted.work.revision,
            command: 101.into(),
            intent: WorkRuntimeIntent::AcknowledgeInterruption {
                execution: receipt.execution,
            },
        },
    )
    .unwrap();
    let final_state = read_runtime(&mut hub, &planned);
    assert_eq!(
        final_state.executions[0].status,
        WorkExecutionStatus::Interrupted
    );
    assert_eq!(
        final_state.executions[0].attempts[0].status,
        WorkAttemptStatus::OutcomeUnknown
    );
    assert_eq!(final_state.executions[0].attempts[0].usage, None);
}
#[test]
fn lost_approval_ack_reconciles_same_receipt_without_renewing_budget() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let planned = edit(
        &mut hub,
        &initial,
        WorkEdit::ReplaceDraft { draft: draft() },
    )
    .unwrap();
    let request = WorkRequest::RuntimeCommand {
        id: planned.id,
        expected: planned.revision,
        command: 100.into(),
        intent: WorkRuntimeIntent::Approve {
            spec: spec(planned.plan.as_ref().unwrap()),
        },
    };
    super::super::tests::LOSE_COMMIT_ACK.with(|flag| flag.set(true));
    assert!(matches!(
        hub.work_document(planned.profile, request.clone()),
        Err(WorkError::OutcomeUnknown)
    ));
    let WorkReply::RuntimeCommand { projection, .. } =
        hub.work_document(planned.profile, request).unwrap()
    else {
        panic!()
    };
    assert_eq!(projection.executions.len(), 1);
    assert_eq!(projection.work.revision, planned.revision.next().unwrap());
}

#[test]
fn approved_deadline_uses_original_monotonic_store_incarnation() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let planned = edit(
        &mut hub,
        &initial,
        WorkEdit::ReplaceDraft { draft: draft() },
    )
    .unwrap();
    let (approved, receipt) = approve(&mut hub, &planned, 100.into());
    // Wall time has not elapsed. Only the original process's monotonic clock
    // decides admission; absolute timestamps remain descriptive history.
    hub.work_runtime_epoch = std::time::Instant::now() - std::time::Duration::from_secs(301);
    assert!(matches!(
        hub.work_document(
            planned.profile,
            WorkRequest::RuntimeUpdate {
                id: planned.id,
                expected: approved.work.revision,
                update: WorkRuntimeUpdate::Begin {
                    execution: receipt.execution,
                    attempt: 999.into(),
                    node: planned.plan.as_ref().unwrap().draft.nodes[0].id
                }
            }
        ),
        Err(WorkError::Conflict)
    ));
    assert!(read_runtime(&mut hub, &planned).executions[0]
        .attempts
        .is_empty());
}

fn direct_read_request(work: &WorkSnapshot, command: u128) -> WorkRequest {
    WorkRequest::RuntimeCommand {
        id: work.id,
        expected: work.revision,
        command: command.into(),
        intent: WorkRuntimeIntent::ReadPublic {
            scope: zephium_core::work::search::WorkPublicSearchScope {
                provider: zephium_core::work::search::WorkSearchProvider::OpenAi,
                model: zephium_core::work::search::PUBLIC_SEARCH_MODEL.into(),
                query: work.objective.clone(),
            },
            limits: WorkExecutionLimits {
                model_tokens: 147456,
                cost_micro_usd: 100000,
                operations: 1,
                timeout_seconds: 180,
                max_workers: 1,
            },
        },
    }
}
#[test]
fn direct_public_read_is_atomic_replayable_and_exactly_scoped() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    // A failure after the internal plan write rolls the entire command back.
    hub.profile_conn(initial.profile).unwrap().execute_batch("CREATE TRIGGER fail_direct_execution BEFORE INSERT ON work_executions BEGIN SELECT RAISE(ABORT, 'test'); END;").unwrap();
    assert!(hub
        .work_document(initial.profile, direct_read_request(&initial, 700))
        .is_err());
    let unchanged = read_runtime(&mut hub, &initial);
    assert_eq!(unchanged.work.revision, initial.revision);
    assert!(unchanged.work.plan.is_none());
    assert!(unchanged.executions.is_empty());
    hub.profile_conn(initial.profile)
        .unwrap()
        .execute_batch("DROP TRIGGER fail_direct_execution;")
        .unwrap();
    assert!(hub
        .work_document(2.into(), direct_read_request(&initial, 700))
        .is_err());
    let WorkReply::PublicReadAdmitted {
        projection,
        receipt,
        replayed,
    } = hub
        .work_document(initial.profile, direct_read_request(&initial, 700))
        .unwrap()
    else {
        panic!()
    };
    assert!(!replayed);
    assert_eq!(projection.executions.len(), 1);
    assert!(projection.executions[0].attempts.is_empty());
    assert_eq!(
        projection.executions[0].authorization,
        WorkExecutionAuthorization::UserDirectedPublicRead
    );
    assert_eq!(
        projection.work.plan.as_ref().unwrap().draft.nodes[0].outputs[0].name,
        "Research findings"
    );
    let WorkReply::PublicReadAdmitted {
        receipt: again,
        replayed,
        ..
    } = hub
        .work_document(initial.profile, direct_read_request(&initial, 700))
        .unwrap()
    else {
        panic!()
    };
    assert!(replayed);
    assert_eq!(receipt, again);
    let mut changed = initial.clone();
    changed.objective.push('!');
    assert!(matches!(
        hub.work_document(initial.profile, direct_read_request(&changed, 700)),
        Err(WorkError::Conflict)
    ));
    assert!(matches!(
        hub.work_document(initial.profile, direct_read_request(&initial, 701)),
        Err(WorkError::Conflict)
    ));
    assert!(hub
        .work_document(initial.profile, direct_read_request(&projection.work, 701))
        .is_err());
    hub.work_document(
        initial.profile,
        WorkRequest::RuntimeCommand {
            id: initial.id,
            expected: projection.work.revision,
            command: 702.into(),
            intent: WorkRuntimeIntent::Cancel {
                execution: receipt.execution,
            },
        },
    )
    .unwrap();
    let WorkReply::PublicReadAdmitted {
        projection,
        replayed,
        ..
    } = hub
        .work_document(initial.profile, direct_read_request(&initial, 700))
        .unwrap()
    else {
        panic!()
    };
    assert!(replayed);
    assert_eq!(projection.executions.len(), 1);
    assert!(projection.executions[0].attempts.is_empty());
}

#[test]
fn direct_public_read_refuses_unanswered_clarification_and_changed_disclosure() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let mut changed = initial.clone();
    changed.objective.push('!');
    assert!(matches!(
        hub.work_document(initial.profile, direct_read_request(&changed, 800)),
        Err(WorkError::Invalid)
    ));
    let asked = edit(
        &mut hub,
        &initial,
        WorkEdit::OpenQuestion {
            id: 99.into(),
            prompt: "Which public release?".into(),
            options: vec![],
        },
    )
    .unwrap();
    assert!(matches!(
        hub.work_document(initial.profile, direct_read_request(&asked, 801)),
        Err(WorkError::Invalid)
    ));
    let state = read_runtime(&mut hub, &asked);
    assert!(state.executions.is_empty());
    assert_eq!(state.work, asked);
}
