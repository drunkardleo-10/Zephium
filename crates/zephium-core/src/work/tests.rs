use super::*;

#[test]
fn coordination_rejects_combined_completion_deadlocks_and_scope_widening() {
    use super::runtime::*;
    let mut graph = draft();
    graph.nodes = (1..=4)
        .map(|id| {
            let mut node = graph.nodes[0].clone();
            node.id = id.into();
            node
        })
        .collect();
    let planned = work()
        .apply(
            WorkRevision::INITIAL,
            WorkEdit::ReplaceDraft { draft: graph },
            WorkAuthor::User,
        )
        .unwrap()
        .0;
    let mut plan = planned.plan.unwrap();
    let limits = WorkExecutionLimits {
        model_tokens: 1000,
        cost_micro_usd: 1000,
        operations: 2,
        timeout_seconds: 60,
        max_workers: 2,
    };
    let scope = WorkBrowseScope {
        start_url: "https://example.test/".into(),
        routes: vec![WorkBrowseRoute {
            origin: "https://example.test".into(),
            path_prefix: "/".into(),
        }],
        max_hops: 1,
    };
    let mut spec = WorkExecutionSpec {
        plan_revision: plan.revision,
        limits: WorkExecutionLimits {
            model_tokens: 4000,
            cost_micro_usd: 4000,
            operations: 8,
            ..limits
        },
        nodes: (1..=4)
            .map(|id| WorkNodeExecutionSpec {
                node: id.into(),
                parent: match id {
                    2 => Some(1.into()),
                    4 => Some(3.into()),
                    _ => None,
                },
                capability: WorkCapability::Coordinate {
                    scope: scope.clone(),
                },
                limits,
            })
            .collect(),
    };
    assert!(spec.validate(&plan).is_ok());
    plan.draft.nodes[1].dependencies = vec![3.into()];
    plan.draft.nodes[3].dependencies = vec![1.into()];
    assert!(plan.draft.validate().is_ok());
    assert_eq!(spec.validate(&plan), Err(WorkError::Invalid));
    plan.draft.nodes[1].dependencies.clear();
    plan.draft.nodes[3].dependencies.clear();
    spec.nodes[1].capability = WorkCapability::PublicBrowse {
        scope: WorkBrowseScope {
            max_hops: 2,
            ..scope
        },
    };
    assert_eq!(spec.validate(&plan), Err(WorkError::Invalid));
}

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
fn execution_proposal_preserves_plan_and_bounds_public_browser_responsibilities() {
    use super::{execution_proposal::*, runtime::*};
    let plan = work()
        .apply(
            WorkRevision::INITIAL,
            WorkEdit::ReplaceDraft { draft: draft() },
            WorkAuthor::User,
        )
        .unwrap()
        .0
        .plan
        .unwrap();
    let limits = WorkExecutionLimits {
        model_tokens: 256_000,
        cost_micro_usd: 1_000_000,
        operations: 256,
        timeout_seconds: 900,
        max_workers: 4,
    };
    let proposal = WorkExecutionProposal {
        nodes: vec![WorkResponsibilityProposal {
            key: 0,
            parent: None,
            capability: WorkCapabilityProposal::PublicDiscovery {
                search_query: "Svelte graph performance".into(),
            },
        }],
    };
    let spec = proposal.clone().compile(&plan, limits).unwrap();
    assert_eq!(spec.plan_revision, plan.revision);
    assert_eq!(spec.nodes[0].node, plan.draft.nodes[0].id);
    assert_eq!(spec.nodes[0].limits.timeout_seconds, 600);
    assert!(
        matches!(&spec.nodes[0].capability, WorkCapability::PublicDiscovery { scope } if scope.max_hops == WORK_PUBLIC_DISCOVERY_MAX_HOPS && scope.search_query == "Svelte graph performance")
    );
    for invalid in 0..3 {
        let mut changed = proposal.clone();
        match invalid {
            0 => changed.nodes[0].key = 1,
            1 => changed.nodes[0].parent = Some(0),
            _ => changed.nodes.push(changed.nodes[0].clone()),
        }
        let reason = changed
            .clone()
            .compile_diagnosed(&plan, limits)
            .err()
            .unwrap();
        assert_eq!(
            reason,
            match invalid {
                0 => WorkExecutionProposalRefusal::UnknownKey { key: 1 },
                1 => WorkExecutionProposalRefusal::DelegationTopology,
                _ => WorkExecutionProposalRefusal::NodeCount {
                    expected: 1,
                    actual: 2
                },
            }
        );
        assert!(changed.compile(&plan, limits).is_err());
    }
    let mut wrong_review = plan.clone();
    wrong_review.draft.nodes[0].outputs[0].review = WorkOutputReview::UserAcceptance;
    assert_eq!(
        proposal
            .clone()
            .compile_diagnosed(&wrong_review, limits)
            .err(),
        Some(WorkExecutionProposalRefusal::OutputReview { key: 0 })
    );
    let mut invalid_parent = proposal.clone();
    invalid_parent.nodes[0].parent = Some(4);
    assert_eq!(
        invalid_parent.compile_diagnosed(&plan, limits).err(),
        Some(WorkExecutionProposalRefusal::UnknownParent { key: 0, parent: 4 })
    );
    let mut invalid_query = proposal;
    invalid_query.nodes[0].capability = WorkCapabilityProposal::PublicDiscovery {
        search_query: "".into(),
    };
    assert_eq!(
        invalid_query.compile_diagnosed(&plan, limits).err(),
        Some(WorkExecutionProposalRefusal::Capability { key: 0 })
    );
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
