use super::super::tests::{create, draft, edit, session};
use super::*;

fn spec(plan: &WorkPlanRevision) -> WorkExecutionSpec {
    let limits = WorkExecutionLimits {
        model_tokens: 8000,
        cost_micro_usd: 10000,
        operations: 32,
        timeout_seconds: 300,
        max_workers: 1,
    };
    WorkExecutionSpec {
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
