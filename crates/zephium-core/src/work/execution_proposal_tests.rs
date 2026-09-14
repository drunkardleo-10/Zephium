use super::*;
fn limits() -> WorkExecutionLimits {
    WorkExecutionLimits {
        model_tokens: 256_000,
        cost_micro_usd: 1_000_000,
        operations: 256,
        timeout_seconds: 900,
        max_workers: 4,
    }
}
fn fixture(count: u8, searches: u8, delegated: bool) -> (WorkPlanRevision, WorkExecutionProposal) {
    let plan = WorkPlanRevision {
        context: None,
        author: WorkAuthor::User,
        revision: WorkRevision::INITIAL,
        basis_revision: WorkRevision::INITIAL,
        draft: WorkPlanDraft {
            id: WorkPlanId::generate(),
            nodes: (0..count)
                .map(|key| WorkPlanNode {
                    id: u128::from(key + 1).into(),
                    objective: format!("Responsibility {key}"),
                    dependencies: vec![],
                    outputs: vec![WorkExpectedOutput {
                        name: "result".into(),
                        description: "Reviewable result".into(),
                        review: WorkOutputReview::SourceMappedNeedsReview,
                    }],
                })
                .collect(),
        },
    };
    let proposal = WorkExecutionProposal {
        nodes: (0..count)
            .map(|key| WorkResponsibilityProposal {
                key,
                parent: (delegated && key != count - 1).then_some(count - 1),
                capability: if delegated && key == count - 1 {
                    WorkCapabilityProposal::Coordinate
                } else if key < searches {
                    WorkCapabilityProposal::PublicSearch {
                        query: format!("Public query {key}"),
                    }
                } else {
                    WorkCapabilityProposal::Synthesize
                },
            })
            .collect(),
    };
    (plan, proposal)
}
#[test]
fn search_allocation_preserves_synthesis_share_and_is_visible_before_approval() {
    let (plan, proposal) = fixture(4, 1, false);
    assert!(matches!(
        proposal.clone().compile_diagnosed(&plan, limits()),
        Err(WorkExecutionProposalRefusal::TokenReservation {
            required: 339456,
            available: 256000
        })
    ));
    let proposed = proposal.proposed_limits(limits()).unwrap();
    assert_eq!(proposed.model_tokens, 339456);
    assert_eq!(proposed.cost_micro_usd, 1_000_000);
    let spec = proposal.compile(&plan, proposed).unwrap();
    assert_eq!(
        spec.nodes
            .iter()
            .map(|n| n.limits.model_tokens)
            .collect::<Vec<_>>(),
        vec![147456, 64000, 64000, 64000]
    );
    assert_eq!(spec.limits.max_workers, 4);
}
#[test]
fn search_coordinator_contains_largest_child_without_inflating_other_synthesis() {
    let (plan, proposal) = fixture(4, 2, true);
    let proposed = proposal.proposed_limits(limits()).unwrap();
    assert_eq!(proposed.model_tokens, 506368);
    let spec = proposal.compile(&plan, proposed).unwrap();
    assert_eq!(
        spec.nodes
            .iter()
            .map(|n| n.limits.model_tokens)
            .collect::<Vec<_>>(),
        vec![147456, 147456, 64000, 147456]
    );
    spec.validate(&plan).unwrap();
}
#[test]
fn search_allocation_refuses_production_ceiling_and_preserves_legacy_equal_shares() {
    let (_, proposal) = fixture(7, 7, false);
    assert!(matches!(
        proposal.proposed_limits(limits()),
        Err(WorkExecutionProposalRefusal::TokenReservation {
            available: 1_000_000,
            ..
        })
    ));
    let (plan, mut proposal) = fixture(4, 0, true);
    proposal.nodes[0].capability = WorkCapabilityProposal::PublicDiscovery {
        search_query: "Browser query".into(),
    };
    assert_eq!(proposal.proposed_limits(limits()).unwrap(), limits());
    let spec = proposal.compile(&plan, limits()).unwrap();
    assert!(spec.nodes.iter().all(|n| n.limits.model_tokens == 64000));
}
