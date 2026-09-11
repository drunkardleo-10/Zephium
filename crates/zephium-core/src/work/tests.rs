use super::*;

fn draft() -> WorkPlanDraft {
    WorkPlanDraft {
        id: 2.into(),
        nodes: vec![WorkPlanNode {
            id: 3.into(),
            objective: "Compare sources".into(),
            dependencies: vec![],
            outputs: vec![WorkExpectedOutput {
                name: "comparison".into(),
                description: "Source-bound comparison with uncertainties".into(),
                review: WorkOutputReview::SourceMappedNeedsReview,
            }],
        }],
    }
}
fn work() -> WorkSnapshot {
    WorkSnapshot::create(1.into(), 4.into(), "Understand the alternatives".into()).unwrap()
}
#[test]
fn work_authoring_questions_invalidate_plans_and_stale_edits_leave_facts_unchanged() {
    let original = work();
    let (planned, _) = original
        .apply(
            original.revision,
            WorkEdit::ReplaceDraft { draft: draft() },
            WorkAuthor::User,
        )
        .unwrap();
    assert_eq!(planned.status, WorkAuthoringStatus::PlanReady);
    assert_eq!(
        planned.plan.as_ref().unwrap().basis_revision,
        original.revision
    );
    let (questioned, _) = planned
        .apply(
            planned.revision,
            WorkEdit::OpenQuestion {
                id: 9.into(),
                prompt: "Which constraints matter?".into(),
                options: vec![],
            },
            WorkAuthor::User,
        )
        .unwrap();
    assert!(questioned.plan.is_none());
    assert_eq!(questioned.status, WorkAuthoringStatus::NeedsInput);
    assert_eq!(
        questioned.apply(
            planned.revision,
            WorkEdit::ReplaceDraft { draft: draft() },
            WorkAuthor::User
        ),
        Err(WorkError::Conflict)
    );
    assert_eq!(
        questioned.apply(
            questioned.revision,
            WorkEdit::ReplaceDraft { draft: draft() },
            WorkAuthor::User
        ),
        Err(WorkError::Conflict)
    );
    let (answered, _) = questioned
        .apply(
            questioned.revision,
            WorkEdit::AnswerQuestion {
                id: 9.into(),
                answer: "Local processing".into(),
            },
            WorkAuthor::User,
        )
        .unwrap();
    assert_eq!(answered.status, WorkAuthoringStatus::Draft);
    assert!(answered.plan.is_none());
    assert!(questioned.questions[0].answer.is_none());
}
#[test]
fn work_authoring_graph_rejects_cycles_duplicates_dangling_edges_and_unbounded_outputs() {
    let first = draft().nodes.remove(0);
    let mut second = first.clone();
    second.id = 5.into();
    second.dependencies = vec![first.id];
    let mut graph = WorkPlanDraft {
        id: 2.into(),
        nodes: vec![second, first],
    };
    assert!(graph.validate().is_ok()); // view order is not execution order
    graph.nodes[1].dependencies = vec![5.into()];
    assert_eq!(graph.validate(), Err(WorkError::Invalid));
    graph.nodes[1].dependencies = vec![99.into()];
    assert_eq!(graph.validate(), Err(WorkError::Invalid));
    graph.nodes[1].dependencies.clear();
    graph.nodes[1].id = 5.into();
    assert_eq!(graph.validate(), Err(WorkError::Invalid));
    let mut oversized = draft();
    oversized.nodes[0].outputs = vec![oversized.nodes[0].outputs[0].clone(); 9];
    assert_eq!(oversized.validate(), Err(WorkError::Invalid));
}
#[test]
fn work_authoring_refuses_invalid_and_exhausted_values_without_wrapping() {
    assert!(WorkSnapshot::create(1.into(), 2.into(), " \n".into()).is_err());
    assert!(WorkSnapshot::create(1.into(), 2.into(), "x".repeat(MAX_WORK_TEXT_BYTES + 1)).is_err());
    assert_eq!(
        WorkRevision::new(i64::MAX as u64).unwrap().next(),
        Err(WorkError::Capacity)
    );
    assert!(WorkRevision::new(0).is_none());
    let mut invalid = work();
    invalid.schema_version = WORK_SCHEMA_VERSION + 1;
    assert_eq!(invalid.validate(), Err(WorkError::Invalid));
    let (mut invalid, _) = work()
        .apply(
            WorkRevision::INITIAL,
            WorkEdit::ReplaceDraft { draft: draft() },
            WorkAuthor::User,
        )
        .unwrap();
    invalid.context_revision = invalid.revision;
    assert_eq!(invalid.validate(), Err(WorkError::Invalid));
}
#[test]
fn work_identity_preserves_canonical_wire_and_debug_redaction() {
    let id = WorkId::from(0xabcd);
    assert_eq!(WorkId::parse(&id.to_string()), Some(id));
    assert!(WorkId::parse(&id.to_string().to_lowercase()).is_none());
    assert_eq!(id.bytes(), 0xabcd_u128.to_be_bytes());
    assert!(!format!("{:?}", work()).contains("alternatives"));
    assert!(!format!(
        "{:?}",
        port::WorkRequest::Create {
            author: WorkAuthor::User,
            id,
            objective: "private".into()
        }
    )
    .contains("private"));
}

#[test]
fn work_authoring_supersedes_context_and_preserves_attribution_without_reactivating_answers() {
    let first = work();
    let (asked, _) = first
        .apply(
            first.revision,
            WorkEdit::OpenQuestion {
                id: 90.into(),
                prompt: "Travel budget?".into(),
                options: vec![],
            },
            WorkAuthor::PrimaryAgent,
        )
        .unwrap();
    assert_eq!(asked.questions[0].basis_revision, Some(first.revision));
    let (answered, _) = asked
        .apply(
            asked.revision,
            WorkEdit::AnswerQuestion {
                id: 90.into(),
                answer: "2000".into(),
            },
            WorkAuthor::User,
        )
        .unwrap();
    let (changed, _) = answered
        .apply(
            answered.revision,
            WorkEdit::SetObjective {
                objective: "Research database indexes".into(),
            },
            WorkAuthor::User,
        )
        .unwrap();
    assert_eq!(changed.status, WorkAuthoringStatus::Draft);
    assert_eq!(changed.current_questions().count(), 0);
    assert_eq!(changed.questions[0].state, WorkQuestionState::Superseded);
    assert_eq!(changed.questions[0].answer.as_deref(), Some("2000"));
    assert_eq!(changed.questions[0].author, WorkAuthor::PrimaryAgent);
    assert_eq!(changed.questions[0].answer_author, Some(WorkAuthor::User));
    assert_eq!(
        changed.apply(
            changed.revision,
            WorkEdit::AnswerQuestion {
                id: 90.into(),
                answer: "Stale response".into(),
            },
            WorkAuthor::User
        ),
        Err(WorkError::Conflict)
    );
    let (fresh, _) = changed
        .apply(
            changed.revision,
            WorkEdit::OpenQuestion {
                id: 91.into(),
                prompt: "Which database?".into(),
                options: vec![],
            },
            WorkAuthor::PrimaryAgent,
        )
        .unwrap();
    let (dismissed, _) = fresh
        .apply(
            fresh.revision,
            WorkEdit::DismissQuestion { id: 91.into() },
            WorkAuthor::User,
        )
        .unwrap();
    let (planned, _) = dismissed
        .apply(
            dismissed.revision,
            WorkEdit::ReplaceDraft { draft: draft() },
            WorkAuthor::PrimaryAgent,
        )
        .unwrap();
    assert_eq!(
        planned.plan.as_ref().unwrap().author,
        WorkAuthor::PrimaryAgent
    );
    let (archived, _) = planned
        .apply(planned.revision, WorkEdit::Archive, WorkAuthor::User)
        .unwrap();
    assert_eq!(archived.plan, planned.plan);
    assert_eq!(
        archived.apply(
            archived.revision,
            WorkEdit::SetObjective {
                objective: "Cannot edit archive".into()
            },
            WorkAuthor::User
        ),
        Err(WorkError::Conflict)
    );
    let (compacted, _) = archived
        .apply(
            archived.revision,
            WorkEdit::CompactHistory,
            WorkAuthor::User,
        )
        .unwrap();
    assert!(compacted.questions.is_empty());
    assert_eq!(compacted.plan, planned.plan);
    assert!(compacted
        .apply(compacted.revision, WorkEdit::Restore, WorkAuthor::User)
        .is_ok());
}

#[test]
fn work_proposals_have_temporary_keys_and_cannot_smuggle_durable_identity_or_authority() {
    use proposal::*;
    let mut proposed = WorkPlanProposal {
        nodes: vec![WorkNodeProposal {
            key: 1,
            objective: "Read evidence".into(),
            dependencies: vec![],
            outputs: draft().nodes.remove(0).outputs,
        }],
    };
    let one = proposed.mint().unwrap();
    let two = proposed.mint().unwrap();
    assert_ne!(one.id, two.id);
    assert_ne!(one.nodes[0].id, two.nodes[0].id);
    proposed.nodes[0].dependencies = vec![2];
    assert_eq!(proposed.validate(), Err(WorkError::Invalid));
    proposed.nodes[0].dependencies = vec![1];
    assert_eq!(proposed.validate(), Err(WorkError::Invalid));
    proposed.nodes[0].dependencies.clear();
    let mut wire = serde_json::to_value(proposed).unwrap();
    wire["author"] = serde_json::json!("user");
    assert!(serde_json::from_value::<WorkPlanProposal>(wire).is_err());
}
