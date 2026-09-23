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
                intervention: None,
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
            intervention: None,
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
            intervention: None,
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
        request: None,
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
                    execution: receipt.execution,
                    intervention: None,
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
                intervention: None,
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
                    intervention: None,
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
                intervention: None,
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
#[test]
fn takeover_persists_its_reason_and_success_cannot_carry_one() {
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
    let running = read_runtime(&mut hub, &planned);
    let cancel = |hub: &mut Hub, command: u128, intervention| {
        hub.work_document(
            planned.profile,
            WorkRequest::RuntimeCommand {
                id: planned.id,
                expected: running.work.revision,
                command: command.into(),
                intent: WorkRuntimeIntent::Cancel {
                    execution: receipt.execution,
                    intervention,
                },
            },
        )
    };
    assert!(matches!(
        cancel(
            &mut hub,
            101,
            Some(WorkInterventionV1 {
                kind: WorkInterventionKindV1::HumanTakeover,
                origin: Some("https://example.test/page".into()),
            })
        ),
        Err(WorkError::Invalid)
    ));
    cancel(
        &mut hub,
        102,
        Some(WorkInterventionV1 {
            kind: WorkInterventionKindV1::HumanTakeover,
            origin: Some("https://example.test".into()),
        }),
    )
    .unwrap();
    let taken = read_runtime(&mut hub, &planned);
    assert_eq!(
        taken.executions[0].status,
        WorkExecutionStatus::CancelRequested
    );
    assert_eq!(
        taken.executions[0].intervention,
        Some(WorkInterventionV1 {
            kind: WorkInterventionKindV1::HumanTakeover,
            origin: Some("https://example.test".into()),
        })
    );
    let settle = |hub: &mut Hub, status, intervention| {
        let state = read_runtime(hub, &planned);
        hub.work_document(
            planned.profile,
            WorkRequest::RuntimeUpdate {
                id: planned.id,
                expected: state.work.revision,
                update: WorkRuntimeUpdate::Settle {
                    execution: receipt.execution,
                    attempt,
                    status,
                    usage: Some(WorkUsage::default()),
                    artifacts: vec![],
                    intervention,
                },
            },
        )
    };
    assert!(matches!(
        settle(
            &mut hub,
            WorkAttemptStatus::Succeeded,
            Some(WorkInterventionV1 {
                kind: WorkInterventionKindV1::SignIn,
                origin: None,
            })
        ),
        Err(WorkError::Invalid)
    ));
    settle(&mut hub, WorkAttemptStatus::Cancelled, None).unwrap();
    let settled = read_runtime(&mut hub, &planned);
    assert_eq!(
        settled.executions[0].intervention.as_ref().map(|i| i.kind),
        Some(WorkInterventionKindV1::HumanTakeover)
    );
}

#[test]
fn agent_admission_mints_the_plan_and_steps_commit_while_the_attempt_runs() {
    use zephium_core::work::search::*;
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let grant = WorkAgentGrantV1 {
        provider: WorkSearchProvider::OpenAi,
        model: PUBLIC_SEARCH_MODEL.into(),
        max_turns: 8,
        max_steps: 24,
        browse_hops: 4,
        folders: vec![],
    };
    let limits = WorkExecutionLimits {
        model_tokens: 600_000,
        cost_micro_usd: 1_000_000,
        operations: 64,
        timeout_seconds: 1200,
        max_workers: 2,
    };
    let begin = |hub: &mut Hub, command: u128, expected| {
        hub.work_document(
            initial.profile,
            WorkRequest::RuntimeCommand {
                id: initial.id,
                expected,
                command: command.into(),
                intent: WorkRuntimeIntent::BeginAgent {
                    grant: grant.clone(),
                    limits,
                },
            },
        )
    };
    let WorkReply::AgentAdmitted {
        projection,
        receipt,
        replayed: false,
    } = begin(&mut hub, 100, initial.revision).unwrap()
    else {
        panic!()
    };
    let execution = receipt.execution;
    let fact = &projection.executions[0];
    assert_eq!(
        fact.authorization,
        WorkExecutionAuthorization::UserDirectedAgent
    );
    assert!(fact.is_agent());
    assert_eq!(projection.work.plan.as_ref().unwrap().draft.nodes.len(), 1);
    assert!(matches!(
        begin(&mut hub, 100, initial.revision).unwrap(),
        WorkReply::AgentAdmitted { replayed: true, .. }
    ));
    assert!(matches!(
        begin(&mut hub, 101, projection.work.revision),
        Err(WorkError::Conflict)
    ));
    let node = projection.work.plan.as_ref().unwrap().draft.nodes[0].id;
    let attempt = WorkAttemptId::from(500);
    let update = |hub: &mut Hub, update| {
        let state = read_runtime(hub, &initial);
        hub.work_document(
            initial.profile,
            WorkRequest::RuntimeUpdate {
                id: initial.id,
                expected: state.work.revision,
                update,
            },
        )
    };
    let step = |id: u128, turn, kind, status| WorkStepFact {
        id: id.into(),
        turn,
        kind,
        status,
        usage: None,
        artifacts: vec![],
        evidence: None,
        note: None,
        measurements: None,
    };
    // Steps need a live attempt.
    assert!(update(
        &mut hub,
        WorkRuntimeUpdate::BeginStep {
            execution,
            attempt,
            step: step(1, 1, WorkStepKindV1::Turn, WorkStepStatus::Succeeded),
            artifacts: vec![],
            evidence: None,
            file: None,
        }
    )
    .is_err());
    update(
        &mut hub,
        WorkRuntimeUpdate::Begin {
            execution,
            attempt,
            node,
        },
    )
    .unwrap();
    let usage = WorkUsage {
        model_tokens: 1200,
        cost_micro_usd: 300,
        operations: 1,
        accounting: WorkUsageAccounting::Exact,
    };
    let mut turn = step(1, 1, WorkStepKindV1::Turn, WorkStepStatus::Succeeded);
    turn.usage = Some(usage);
    turn.note = Some("Searching for options.".into());
    update(
        &mut hub,
        WorkRuntimeUpdate::BeginStep {
            execution,
            attempt,
            step: turn,
            artifacts: vec![],
            evidence: None,
            file: None,
        },
    )
    .unwrap();
    update(
        &mut hub,
        WorkRuntimeUpdate::BeginStep {
            execution,
            attempt,
            step: step(
                2,
                1,
                WorkStepKindV1::Search {
                    query: "canvas libraries".into(),
                },
                WorkStepStatus::Running,
            ),
            artifacts: vec![],
            evidence: None,
            file: None,
        },
    )
    .unwrap();
    let record = WorkProviderSearchRecordV1 {
        ranking: None,
        id: WorkArtifactId::from(40),
        node,
        attempt,
        evidence: WorkProviderSearchEvidenceV1 {
            version: 1,
            provider: WorkSearchProvider::OpenAi,
            model: PUBLIC_SEARCH_MODEL.into(),
            response_model: PUBLIC_SEARCH_MODEL.into(),
            response_id: "resp_1".into(),
            search_call_id: "ws_1".into(),
            answer: "Svelte Flow is a canvas library".into(),
            citations: vec![WorkProviderSearchCitation {
                url: "https://svelteflow.dev".into(),
                title: "Svelte Flow".into(),
                start_index: 0,
                end_index: 11,
            }],
            actual_input_tokens: 1000,
            actual_output_tokens: 200,
        },
    };
    let sources = artifact::WorkArtifactV1 {
        version: 1,
        id: WorkArtifactId::from(41),
        execution,
        node,
        attempt,
        output: "Result".into(),
        title: "Sources".into(),
        data: artifact::WorkArtifactDataV1::EvidenceCollection {
            summary: "Svelte Flow is a canvas library".into(),
            subjects: vec![],
            entries: vec![],
        },
        evidence: vec![artifact::WorkEvidenceLink {
            extraction_id: record.id,
            source_id: 1,
        }],
        review: WorkOutputReview::SourceMappedNeedsReview,
        presentation: artifact::WorkArtifactPresentationV1::Automatic,
    };
    // A settled search publishes its sources and record atomically.
    assert!(update(
        &mut hub,
        WorkRuntimeUpdate::SettleStep {
            execution,
            attempt,
            step: 2.into(),
            status: WorkStepStatus::Succeeded,
            usage: Some(usage),
            artifacts: vec![sources.clone()],
            evidence: None,
            file: None,
            note: None,
            measurements: None,
        }
    )
    .is_err());
    update(
        &mut hub,
        WorkRuntimeUpdate::SettleStep {
            execution,
            attempt,
            step: 2.into(),
            status: WorkStepStatus::Succeeded,
            usage: Some(usage),
            artifacts: vec![sources],
            evidence: Some(Box::new(record)),
            file: None,
            note: Some("Found 1 source".into()),
            measurements: None,
        },
    )
    .unwrap();
    let state = read_runtime(&mut hub, &initial);
    assert_eq!(state.executions[0].artifacts.len(), 1);
    assert_eq!(state.executions[0].steps[1].artifacts.len(), 1);
    assert_eq!(state.executions[0].status, WorkExecutionStatus::Running);
    // Questions are answered while the attempt runs, exactly once.
    update(
        &mut hub,
        WorkRuntimeUpdate::BeginStep {
            execution,
            attempt,
            step: step(
                3,
                2,
                WorkStepKindV1::Ask {
                    prompt: "Which budget?".into(),
                    options: vec!["Low".into(), "High".into()],
                    answer: None,
                },
                WorkStepStatus::Running,
            ),
            artifacts: vec![],
            evidence: None,
            file: None,
        },
    )
    .unwrap();
    let answer = |hub: &mut Hub, command: u128| {
        let state = read_runtime(hub, &initial);
        hub.work_document(
            initial.profile,
            WorkRequest::RuntimeCommand {
                id: initial.id,
                expected: state.work.revision,
                command: command.into(),
                intent: WorkRuntimeIntent::AnswerStep {
                    execution,
                    step: 3.into(),
                    answer: "Low".into(),
                },
            },
        )
    };
    answer(&mut hub, 200).unwrap();
    assert!(matches!(answer(&mut hub, 201), Err(WorkError::Conflict)));
    let state = read_runtime(&mut hub, &initial);
    assert!(matches!(
        &state.executions[0].steps[2].kind,
        WorkStepKindV1::Ask { answer: Some(answer), .. } if answer == "Low"
    ));
    // A steer is a settled person step appended while the attempt runs.
    let steer = |hub: &mut Hub, command: u128| {
        let state = read_runtime(hub, &initial);
        hub.work_document(
            initial.profile,
            WorkRequest::RuntimeCommand {
                id: initial.id,
                expected: state.work.revision,
                command: command.into(),
                intent: WorkRuntimeIntent::Steer {
                    execution,
                    text: "Only official pricing pages".into(),
                },
            },
        )
    };
    steer(&mut hub, 210).unwrap();
    let state = read_runtime(&mut hub, &initial);
    assert!(matches!(
        &state.executions[0].steps[3],
        WorkStepFact { kind: WorkStepKindV1::Steer { text }, status: WorkStepStatus::Succeeded, usage: None, turn: 2, .. }
            if text == "Only official pricing pages"
    ));
    // A proposed change waits undecided, takes exactly one decision, and
    // settles with the file record it produced.
    update(
        &mut hub,
        WorkRuntimeUpdate::BeginStep {
            execution,
            attempt,
            step: step(
                7,
                2,
                WorkStepKindV1::WriteFile {
                    path: "/Users/me/Documents/project/notes.txt".into(),
                    content: "one\n".into(),
                    decision: None,
                },
                WorkStepStatus::Running,
            ),
            artifacts: vec![],
            evidence: None,
            file: None,
        },
    )
    .unwrap();
    let approve = |hub: &mut Hub, command: u128| {
        let state = read_runtime(hub, &initial);
        hub.work_document(
            initial.profile,
            WorkRequest::RuntimeCommand {
                id: initial.id,
                expected: state.work.revision,
                command: command.into(),
                intent: WorkRuntimeIntent::ApproveStep {
                    execution,
                    step: 7.into(),
                    approve: true,
                },
            },
        )
    };
    approve(&mut hub, 220).unwrap();
    assert!(matches!(approve(&mut hub, 221), Err(WorkError::Conflict)));
    let written = WorkFileRecordV1 {
        id: 71.into(),
        node,
        attempt,
        file: WorkFileEvidenceV1 {
            path: "/Users/me/Documents/project/notes.txt".into(),
            name: "notes.txt".into(),
            kind: WorkFileKindV1::Written,
            bytes: 4,
            digest: "a".repeat(64),
            text: "@@ -1,0 +1,1 @@\n+ one\n".into(),
            truncated: false,
        },
    };
    update(
        &mut hub,
        WorkRuntimeUpdate::SettleStep {
            execution,
            attempt,
            step: 7.into(),
            status: WorkStepStatus::Succeeded,
            usage: None,
            artifacts: vec![],
            evidence: None,
            file: Some(Box::new(written.clone())),
            note: Some("Applied".into()),
            measurements: None,
        },
    )
    .unwrap();
    let state = read_runtime(&mut hub, &initial);
    assert_eq!(state.executions[0].file_evidence, vec![written]);
    assert!(matches!(
        &state.executions[0].steps[4],
        WorkStepFact { kind: WorkStepKindV1::WriteFile { decision: Some(true), .. }, status: WorkStepStatus::Succeeded, evidence: Some(id), .. } if *id == 71.into()
    ));
    // Success needs the Finish step first.
    let settle = |hub: &mut Hub, status| {
        let state = read_runtime(hub, &initial);
        hub.work_document(
            initial.profile,
            WorkRequest::RuntimeUpdate {
                id: initial.id,
                expected: state.work.revision,
                update: WorkRuntimeUpdate::Settle {
                    execution,
                    attempt,
                    status,
                    usage: Some(usage),
                    artifacts: vec![],
                    intervention: None,
                },
            },
        )
    };
    assert!(settle(&mut hub, WorkAttemptStatus::Succeeded).is_err());
    update(
        &mut hub,
        WorkRuntimeUpdate::BeginStep {
            execution,
            attempt,
            step: step(
                4,
                3,
                WorkStepKindV1::Finish { followups: vec![] },
                WorkStepStatus::Succeeded,
            ),
            artifacts: vec![],
            evidence: None,
            file: None,
        },
    )
    .unwrap();
    settle(&mut hub, WorkAttemptStatus::Succeeded).unwrap();
    let state = read_runtime(&mut hub, &initial);
    assert_eq!(state.executions[0].status, WorkExecutionStatus::NeedsReview);
    assert_eq!(state.executions[0].steps.len(), 6);
    assert!(matches!(steer(&mut hub, 211), Err(WorkError::Conflict)));
}
