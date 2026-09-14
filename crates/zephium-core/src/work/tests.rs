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
        context: None,
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

#[test]
fn account_scoped_specs_bind_one_attested_page_and_a_single_step_plan() {
    use super::runtime::*;
    let plan = WorkPlanRevision {
        context: None,
        author: WorkAuthor::User,
        revision: WorkRevision::INITIAL,
        basis_revision: WorkRevision::INITIAL,
        draft: draft(),
    };
    let limits = WorkExecutionLimits {
        model_tokens: 128_000,
        cost_micro_usd: 500_000,
        operations: 64,
        timeout_seconds: 600,
        max_workers: 4,
    };
    let scope = WorkAccountScope {
        tab: crate::ids::ItemId::generate(),
        url: "https://app.notion.com/p/Sprint-42".into(),
        origin: "https://app.notion.com".into(),
        account: "01JACCOUNT0000000000000000".into(),
    };
    let read = WorkExecutionSpec::account_scoped(
        &plan,
        limits,
        WorkCapability::AccountRead {
            scope: scope.clone(),
        },
    )
    .unwrap();
    assert_eq!(read.nodes.len(), 1);
    assert_eq!(read.nodes[0].limits.max_workers, 1);
    let update = WorkFieldUpdateV1 {
        field: Some("Title".into()),
        from: "Sprint 42".into(),
        to: "Sprint 42 (probe)".into(),
    };
    WorkExecutionSpec::account_scoped(
        &plan,
        limits,
        WorkCapability::AccountUpdate {
            scope: scope.clone(),
            update: update.clone(),
        },
    )
    .unwrap();
    for refused in [
        WorkCapability::AccountRead {
            scope: WorkAccountScope {
                origin: "https://www.notion.so".into(),
                ..scope.clone()
            },
        },
        WorkCapability::AccountRead {
            scope: WorkAccountScope {
                url: "http://app.notion.com/p/Sprint-42".into(),
                origin: "http://app.notion.com".into(),
                ..scope.clone()
            },
        },
        WorkCapability::AccountRead {
            scope: WorkAccountScope {
                account: "not an id".into(),
                ..scope.clone()
            },
        },
        WorkCapability::AccountUpdate {
            scope: scope.clone(),
            update: WorkFieldUpdateV1 {
                to: "Sprint 42".into(),
                ..update.clone()
            },
        },
        WorkCapability::AccountUpdate {
            scope: scope.clone(),
            update: WorkFieldUpdateV1 {
                from: " ".into(),
                ..update.clone()
            },
        },
        WorkCapability::PublicBrowse {
            scope: WorkBrowseScope {
                start_url: "https://app.notion.com/p/Sprint-42".into(),
                routes: vec![WorkBrowseRoute {
                    origin: "https://app.notion.com".into(),
                    path_prefix: "/".into(),
                }],
                max_hops: 1,
            },
        },
    ] {
        assert!(WorkExecutionSpec::account_scoped(&plan, limits, refused).is_err());
    }
    let mut two_steps = plan.clone();
    let mut second = two_steps.draft.nodes[0].clone();
    second.id = WorkPlanNodeId::generate();
    two_steps.draft.nodes.push(second);
    assert!(WorkExecutionSpec::account_scoped(
        &two_steps,
        limits,
        WorkCapability::AccountRead { scope },
    )
    .is_err());
    assert!(WorkInterventionV1 {
        kind: WorkInterventionKindV1::SignIn,
        origin: Some("https://app.notion.com/p".into()),
    }
    .validate()
    .is_err());
}

#[test]
fn agent_executions_commit_steps_incrementally_and_finish_explicitly() {
    use super::{agent::*, artifact::*, runtime::*, search::*};
    let plan = WorkPlanRevision {
        context: None,
        author: WorkAuthor::User,
        revision: WorkRevision::new(2).unwrap(),
        basis_revision: WorkRevision::INITIAL,
        draft: draft(),
    };
    let limits = WorkExecutionLimits {
        model_tokens: 600_000,
        cost_micro_usd: 1_000_000,
        operations: 64,
        timeout_seconds: 1200,
        max_workers: 2,
    };
    let grant = WorkAgentGrantV1 {
        provider: WorkSearchProvider::OpenAi,
        model: PUBLIC_SEARCH_MODEL.into(),
        max_turns: 8,
        max_steps: 24,
        browse_hops: 4,
    };
    assert!(WorkAgentGrantV1 {
        max_turns: 0,
        ..grant.clone()
    }
    .validate()
    .is_err());
    let spec = WorkExecutionSpec::agent(&plan, limits, grant.clone()).unwrap();
    let node = plan.draft.nodes[0].id;
    let attempt = WorkAttemptId::from(500);
    let revision = WorkRevision::new(9).unwrap();
    let mut fact = WorkExecutionFact {
        authorization: WorkExecutionAuthorization::UserDirectedAgent,
        id: WorkExecutionId::from(7),
        approved_revision: WorkRevision::new(3).unwrap(),
        spec,
        status: WorkExecutionStatus::Approved,
        attempts: vec![],
        artifacts: vec![],
        provider_evidence: vec![],
        user_artifacts: vec![],
        intervention: None,
        steps: vec![],
    };
    fact.validate(&plan, revision).unwrap();
    let mut reviewed = fact.clone();
    reviewed.authorization = WorkExecutionAuthorization::ReviewedPlan;
    assert!(reviewed.validate(&plan, revision).is_err());
    fact.attempts.push(WorkAttemptFact {
        id: attempt,
        node,
        status: WorkAttemptStatus::Running,
        usage: None,
    });
    fact.status = WorkExecutionStatus::Running;
    let step = |id: u128, turn, kind, status| WorkStepFact {
        id: id.into(),
        turn,
        kind,
        status,
        usage: None,
        artifacts: vec![],
        evidence: None,
        note: None,
    };
    let usage = WorkUsage {
        model_tokens: 1200,
        cost_micro_usd: 300,
        operations: 1,
        accounting: WorkUsageAccounting::Exact,
    };
    let mut turn = step(1, 1, WorkStepKindV1::Turn, WorkStepStatus::Succeeded);
    turn.usage = Some(usage);
    turn.note = Some("Looking for current options.".into());
    fact.steps.push(turn);
    fact.steps.push(step(
        2,
        1,
        WorkStepKindV1::Search {
            query: "best canvas libraries".into(),
        },
        WorkStepStatus::Running,
    ));
    fact.validate(&plan, revision).unwrap();
    // A settled search binds its provider record and usage.
    let record = WorkProviderSearchRecordV1 {
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
    let sources = WorkArtifactV1 {
        version: 1,
        id: WorkArtifactId::from(41),
        execution: fact.id,
        node,
        attempt,
        output: "comparison".into(),
        title: "Sources".into(),
        data: artifact::WorkArtifactDataV1::EvidenceCollection {
            summary: "Svelte Flow is a canvas library".into(),
            subjects: vec![],
            entries: vec![],
        },
        evidence: vec![WorkEvidenceLink {
            extraction_id: record.id,
            source_id: 1,
        }],
        review: WorkOutputReview::SourceMappedNeedsReview,
        presentation: artifact::WorkArtifactPresentationV1::Automatic,
    };
    fact.steps[1].status = WorkStepStatus::Succeeded;
    fact.steps[1].usage = Some(usage);
    fact.steps[1].evidence = Some(record.id);
    fact.steps[1].artifacts = vec![sources.id];
    assert!(fact.validate(&plan, revision).is_err());
    fact.provider_evidence.push(record);
    fact.artifacts.push(sources);
    fact.validate(&plan, revision).unwrap();
    // Unclaimed artifacts and unknown questions never validate.
    let mut orphan = fact.clone();
    orphan.steps[1].artifacts.clear();
    assert!(orphan.validate(&plan, revision).is_err());
    let mut answered_running = fact.clone();
    answered_running.steps.push(step(
        3,
        2,
        WorkStepKindV1::Ask {
            prompt: "Which budget?".into(),
            options: vec!["Low".into(), "High".into()],
            answer: Some("Low".into()),
        },
        WorkStepStatus::Running,
    ));
    assert!(answered_running.validate(&plan, revision).is_err());
    // Success requires a final Finish step and no running steps.
    let mut early = fact.clone();
    early.attempts[0].status = WorkAttemptStatus::Succeeded;
    early.attempts[0].usage = Some(usage);
    early.status = WorkExecutionStatus::NeedsReview;
    assert!(early.validate(&plan, revision).is_err());
    fact.steps.push(step(
        4,
        2,
        WorkStepKindV1::Finish,
        WorkStepStatus::Succeeded,
    ));
    fact.attempts[0].status = WorkAttemptStatus::Succeeded;
    fact.attempts[0].usage = Some(usage);
    fact.status = WorkExecutionStatus::NeedsReview;
    fact.validate(&plan, revision).unwrap();
    let mut after_finish = fact.clone();
    after_finish.attempts[0].status = WorkAttemptStatus::Running;
    after_finish.attempts[0].usage = None;
    after_finish.status = WorkExecutionStatus::Running;
    after_finish
        .steps
        .push(step(5, 3, WorkStepKindV1::Turn, WorkStepStatus::Failed));
    assert!(after_finish.validate(&plan, revision).is_err());

    // Turn resolution admits only shown sources and a complete vocabulary.
    let preview = WorkEvidencePreviewV1 {
        version: 1,
        link: WorkEvidenceLink {
            extraction_id: WorkArtifactId::from(40),
            source_id: 1,
        },
        origin: "https://svelteflow.dev".into(),
        role: "provider_search".into(),
        text: "Svelte Flow is a canvas library".into(),
        truncated: false,
        source_bytes: "31".into(),
        source: WorkEvidenceSourceV1::ProviderSearch {
            provider: WorkSearchProvider::OpenAi,
            model: PUBLIC_SEARCH_MODEL.into(),
            url: "https://svelteflow.dev/docs".into(),
            title: "Svelte Flow".into(),
            response_id: "resp_1".into(),
            search_call_id: "ws_1".into(),
        },
    };
    let disclosure = WorkAgentTurnDisclosure::try_new(
        "Compare canvas libraries",
        vec![],
        vec![],
        &fact.steps,
        &[preview],
        &fact.artifacts,
        WorkAgentBudget {
            turns_left: 6,
            steps_left: 20,
            browse_available: true,
        },
        limits,
    )
    .unwrap();
    assert_eq!(
        disclosure.context().sources[0].url,
        "https://svelteflow.dev/docs"
    );
    let output = |fetch, ask, finish| WorkAgentTurnOutput {
        say: Some("Comparing.".into()),
        artifacts: vec![WorkAgentArtifactOutput {
            title: "Svelte Flow".into(),
            data: artifact::WorkArtifactDataV1::Findings {
                subjects: vec![],
                items: vec![artifact::WorkFinding {
                    claim: "Svelte Flow is a canvas library".into(),
                    subject: None,
                    evidence: vec![0],
                    confidence: artifact::WorkConfidence::Supported,
                    detail: None,
                    general_knowledge: false,
                }],
            },
            evidence: vec![0],
        }],
        fetch,
        ask,
        finish,
    };
    let turn = disclosure
        .resolve(output(
            vec![
                WorkAgentFetch::Read {
                    url: "https://svelteflow.dev/docs".into(),
                },
                WorkAgentFetch::Search {
                    query: "svelte flow bundle size".into(),
                },
            ],
            None,
            false,
        ))
        .unwrap();
    assert_eq!(turn.fetch.len(), 2);
    assert_eq!(turn.artifacts[0].evidence[0].source_id, 1);
    assert!(disclosure
        .resolve(output(
            vec![WorkAgentFetch::Read {
                url: "https://example.test/unseen".into(),
            }],
            None,
            false,
        ))
        .is_err());
    assert!(disclosure
        .resolve(output(
            vec![WorkAgentFetch::Search {
                query: "more".into()
            }],
            None,
            true,
        ))
        .is_err());
    let mut uncited = output(vec![], None, true);
    uncited.artifacts[0].evidence.clear();
    assert!(disclosure.resolve(uncited).is_err());
    assert!(disclosure
        .resolve(WorkAgentTurnOutput {
            say: None,
            artifacts: vec![],
            fetch: vec![],
            ask: None,
            finish: false,
        })
        .is_err());
}
