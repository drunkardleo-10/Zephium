use super::*;
use crate::{ContextNavigationSettlement, ContextNavigationTarget, ContextPortFailure};

fn route_fixture(operations: u32) -> (PolicyFixture, ContextRegistry, SemanticObservation) {
    route_fixture_with_targets(operations, 10_000, vec![target(), final_target()])
}

fn route_fixture_with_targets(
    operations: u32,
    tokens: u64,
    destinations: Vec<ContextNavigationTarget>,
) -> (PolicyFixture, ContextRegistry, SemanticObservation) {
    navigation_fixture(operations, tokens, destinations, false)
}
fn discovery_observation(context: ContextJoin, id: u64) -> SemanticObservation {
    observation(
        context,
        origin("source"),
        id,
        vec![
            json!({"k":1,"r":"link","n":"Next","u":target().as_url().as_str()}),
            json!({"k":2,"r":"link","n":"Final","u":final_target().as_url().as_str()}),
        ],
    )
}
fn navigation_fixture(
    operations: u32,
    tokens: u64,
    destinations: Vec<ContextNavigationTarget>,
    discovery: bool,
) -> (PolicyFixture, ContextRegistry, SemanticObservation) {
    navigation_fixture_with_document_policy(
        operations,
        tokens,
        destinations,
        discovery,
        crate::WorkBrowserDocumentPolicy::Exact,
    )
}
fn navigation_fixture_with_document_policy(
    operations: u32,
    tokens: u64,
    destinations: Vec<ContextNavigationTarget>,
    discovery: bool,
    document_policy: crate::WorkBrowserDocumentPolicy,
) -> (PolicyFixture, ContextRegistry, SemanticObservation) {
    let (mut registry, context) = make_context_registry(901, 902, 903);
    registry
        .acknowledge_observation(context.identity().id(), context)
        .unwrap();
    let source = origin("source");
    let observation = if discovery {
        discovery_observation(context, 1)
    } else {
        actionable_observation(context, source.clone(), 1)
    };
    let sensitivity = if discovery {
        SemanticSensitivity::Public
    } else {
        SemanticSensitivity::Sensitive
    };
    let budget = run_budget(operations, tokens, 10_000);
    let effects = effects(&[SemanticEffectClass::Read]);
    let route = crate::AgentNavigationRoute::try_new(
        ContextNavigationTarget::parse("https://source.example.test/start").unwrap(),
        destinations,
    )
    .unwrap();
    let authority = AgentPlanNodeAuthority::try_new(
        vec![profile(902)],
        vec![AgentAccountScope::Anonymous],
        vec![source.clone()],
        sensitivity,
        effects,
    )
    .unwrap();
    let authority = if discovery {
        authority
            .with_navigation_discovery(
                crate::AgentNavigationDiscovery::try_new_with_document_policy(
                    route.departure().clone(),
                    "/".into(),
                    2,
                    document_policy,
                )
                .unwrap(),
            )
            .unwrap()
    } else {
        authority.with_navigation_route(route).unwrap()
    };
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::from_raw(1),
        ContextRunId::from_raw(901),
        AgentRunScope::try_new(
            vec![profile(902)],
            vec![AgentAccountScope::Anonymous],
            vec![source],
            sensitivity,
            effects,
            vec![],
        )
        .unwrap(),
        budget,
        AgentPolicyInstant::from_millis(ISSUED_AT),
        AgentPolicyInstant::from_millis(EXPIRES_AT),
        vec![AgentPlanNodeScope::new(
            AgentPlanNodeId::from_raw(1),
            authority,
            budget,
            AgentPolicyInstant::from_millis(EXPIRES_AT - 1),
        )],
    )
    .unwrap();
    let lease = AgentPlanLeaseId::from_raw(1);
    let mut policy = AgentRunPolicy::try_new(
        manifest,
        vec![AgentPlanLeaseBinding::new(
            lease,
            AgentPlanNodeId::from_raw(1),
        )],
    )
    .unwrap();
    commit_observation_to_model(
        &mut policy,
        lease,
        1,
        account(context, NOW - 1),
        &observation,
    );
    (PolicyFixture { policy, lease }, registry, observation)
}

fn final_target() -> ContextNavigationTarget {
    ContextNavigationTarget::parse("https://source.example.test/final").unwrap()
}

#[test]
fn structured_product_window_reaches_third_provider_request_with_original_policy() {
    // Shape/route regression from retained commerce run twenty-second:
    // 128 / NodeLimit -> @a122 window(1000,5000) -> 87 / ScopeBoundary.
    // Page strings below are synthetic; no stored response or provider is used.
    let (mut f, _, initial) = navigation_fixture(6, 200_000, vec![target(), final_target()], true);
    let context = initial.request().context();
    let make = |request: SemanticObservationRequest, generation, completeness, nodes| {
        let wire = serde_json::to_vec(&json!({
            "v":1,"i":generation,"g":generation,"c":completeness,"n":nodes
        }))
        .unwrap();
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(generation).unwrap(),
                initial.frames()[0].frame().clone(),
                SemanticSnapshotGeneration::new(generation).unwrap(),
            ),
            &wire,
        )
        .unwrap();
        SemanticObservationAssembler::new(request, snapshot)
            .unwrap()
            .finish()
            .unwrap()
    };
    let mut nodes: Vec<_> = (1..=128)
        .map(|key| json!({"k":key,"r":"paragraph","t":"previous-only product inventory"}))
        .collect();
    nodes[121] = json!({"k":122,"r":"heading","l":1,"n":"Product title"});
    let previous = make(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(2).unwrap(),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        ),
        12,
        "node_limit",
        nodes,
    );
    let selected = tokenizer();
    let config = provider_exact_config(selected.clone(), 128, 64_000)
        .restrict_to_navigation_and_extraction()
        .with_baseline_read()
        .with_progressive_observation();
    let objective = AgentProviderObjective::try_admit_conservative_utf8(
        "Research the observed product without buying".into(),
        &selected,
    )
    .unwrap();
    let payload = |observation: &SemanticObservation| {
        encode_semantic_observation(
            observation,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .unwrap()
        .admit_conservative_utf8(&selected)
        .unwrap()
    };
    let binding = account(context, NOW);
    let committed = AgentPreparedObservationRequest::try_openai_for_provider_exact_count(
        &mut f.policy,
        call_request(2, f.lease, binding, 64_000, 128, 1_000, NOW),
        &previous,
        payload(&previous),
        &objective,
        config.clone(),
    )
    .unwrap()
    .into_transport_input()
    .commit(&mut f.policy)
    .unwrap();
    let (second, input, seed) = committed.into_parts();
    let (active, evidence) = input.into_parts();
    let old_ack = evidence.observation_acknowledgement().unwrap().clone();
    f.policy
        .settle_model_call(active, AgentModelCallSettlement::Completed, 10_000, 4, 80)
        .unwrap();
    let arguments = r#"{"scope":{"kind":"surrounding_text","after_bytes":5000,"before_bytes":1000,"target":"@a122"}}"#;
    let tool = crate::AgentBrowserToolCall::decode_openai(
        second.call(),
        "fc_window".into(),
        "call_window".into(),
        "snapshot",
        arguments.into(),
    )
    .unwrap();
    let completion = crate::AgentProviderCompletion::new(
        second.call(),
        crate::AgentProviderStopReason::ToolCalls,
        crate::AgentProviderUsage::try_new(10_000, 4, 0, 0, 0).unwrap(),
        crate::AgentProviderStreamStats::new(200, 8, 0, 1, arguments.len() as u32),
        true,
    );
    let checkpoint = seed
        .unwrap()
        .join_terminal_tool_for_test(completion, tool.into_continuation_parts_for_test().0)
        .unwrap()
        .retire_for_observation(&previous, &config)
        .unwrap();
    let request = checkpoint
        .request(&previous, SemanticObservationId::new(3).unwrap())
        .unwrap();
    let mut nodes = vec![json!({"k":122,"r":"heading","l":1,"n":"Product title"})];
    nodes.extend(
        (201..=286)
            .map(|key| json!({"k":key,"r":"paragraph","t":"new product dimension evidence"})),
    );
    let current = make(request, 13, "scope_boundary", nodes);
    assert_eq!(previous.node_count(), 128);
    assert_eq!(current.node_count(), 87);
    assert!(current.frames()[0]
        .nodes()
        .iter()
        .all(|node| node.parent().is_none()
            && node.operations().is_empty()
            && node.link_destination().is_none()));
    let prepared = checkpoint
        .prepare_successor(
            &mut f.policy,
            &previous,
            &current,
            call_request(3, f.lease, binding, 64_000, 128, 1_000, NOW),
            config,
            payload(&current),
            &objective,
        )
        .expect("structured window reaches original third-call admission");
    assert_eq!(
        prepared.request().call().call(),
        AgentModelCallId::new(3).unwrap()
    );
    let body = std::str::from_utf8(prepared.request().body()).unwrap();
    assert!(
        body.contains("ZEPHIUM_HOST_INSPECTION_PROGRESS_V1")
            && body.contains("new product dimension evidence")
    );
    assert!(!body.contains("previous-only product inventory") && !body.contains("@a122"));
    let (third, input, _) = prepared
        .into_transport_input()
        .commit(&mut f.policy)
        .unwrap()
        .into_parts();
    assert_eq!(third.call().call(), AgentModelCallId::new(3).unwrap());
    let (active, evidence) = input.into_parts();
    assert!(evidence
        .observation_acknowledgement()
        .unwrap()
        .matches(&current));
    assert!(!old_ack.matches(&current));
    f.policy
        .settle_model_call(active, AgentModelCallSettlement::Completed, 10_000, 4, 80)
        .unwrap();
    assert_eq!(f.policy.pending_model_calls(), 0);
}

#[test]
fn initial_effective_metadata_requires_original_retained_binding_before_model_calls() {
    use crate::*;
    for fault in 0..4 {
        let (f, _, _) = navigation_fixture(5, 10_000, vec![target(), final_target()], true);
        let mut policy = if fault == 3 {
            f.policy
        } else {
            AgentRunPolicy::try_new(
                f.policy.manifest,
                vec![AgentPlanLeaseBinding::new(
                    f.lease,
                    AgentPlanNodeId::from_raw(1),
                )],
            )
            .unwrap()
        };
        let requested = ContextNavigationTarget::parse(if fault == 1 {
            "https://source.example.test/other"
        } else {
            "https://source.example.test/start"
        })
        .unwrap();
        let effective =
            ContextNavigationTarget::parse(&format!("{}?opaque=initial", requested.as_url()))
                .unwrap();
        let mut rows = WorkBrowserResources::new(WorkId::generate(), profile(902));
        let construction = rows
            .construct_document_with_policy(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                requested.clone(),
                WorkBrowserDocumentPolicy::DocumentQueryFinalization,
                AgentPolicyInstant::from_millis(0),
            )
            .unwrap();
        let resource = construction.resource().clone();
        let _ = rows
            .settle_at(
                construction.complete_document(effective.clone()),
                AgentPolicyInstant::from_millis(0),
            )
            .unwrap();
        let acquire = rows
            .acquire(
                &resource,
                ContextRunId::from_raw(if fault == 2 { 999 } else { 901 }),
                AgentPolicyInstant::from_millis(1),
                AgentPolicyInstant::from_millis(EXPIRES_AT),
            )
            .unwrap();
        let lease = acquire.lease().unwrap().clone();
        let _ = rows
            .settle_at(
                acquire.complete(WorkBrowserResourceNativeOutcome::Acquired),
                AgentPolicyInstant::from_millis(1),
            )
            .unwrap();
        let binding = rows
            .read_binding(&lease, AgentPolicyInstant::from_millis(2))
            .unwrap();
        let result = policy.bind_retained_initial_document(&binding);
        assert_eq!(result.is_ok(), fault == 0, "fault {fault}");
        if fault == 0 {
            let context = binding.frame().context();
            let observed = discovery_observation(context, 1);
            let request = call_request(1, f.lease, account(context, NOW), 0, 0, 0, NOW);
            let checkpoint = policy
                .provider_navigation_checkpoint(request, &observed)
                .unwrap()
                .unwrap();
            assert_eq!(checkpoint.current_document(), Some(&effective));
            assert_eq!(checkpoint.current_requested_document(), Some(&requested));
            assert!(policy.bind_retained_initial_document(&binding).is_err());
        }
    }
}

#[test]
fn trusted_finalization_preserves_requested_progress_and_binds_effective_document() {
    use crate::WorkBrowserDocumentPolicy as P;
    for (policy, effective, accepted) in [
        (
            P::Exact,
            "https://source.example.test/next?opaque=one",
            false,
        ),
        (
            P::DocumentQueryFinalization,
            "https://source.example.test/next?opaque=one",
            true,
        ),
        (
            P::DocumentQueryFinalization,
            "https://source.example.test/other?opaque=one",
            false,
        ),
        (
            P::DocumentQueryFinalization,
            "https://foreign.test/next?opaque=one",
            false,
        ),
        (
            P::DocumentQueryFinalization,
            "https://source.example.test/next?opaque=one#fragment",
            false,
        ),
    ] {
        let (mut f, mut registry, observed) = navigation_fixture_with_document_policy(
            5,
            10_000,
            vec![target(), final_target()],
            true,
            policy,
        );
        // The actual fixture link remains the sole requested destination.
        let destination = target();
        let effective = ContextNavigationTarget::parse(effective).unwrap();
        let request = route_request(
            &f,
            &registry,
            &observed,
            account(observed.request().context(), NOW - 1),
        );
        let permit = f
            .policy
            .authorize_navigation(request, &observed, &baseline(&observed), &destination)
            .unwrap();
        let operation = registry
            .begin_navigation(
                observed.request().context().identity().id(),
                ContextOperationId::new(2).unwrap(),
            )
            .unwrap();
        let active = f
            .policy
            .dispatch_navigation(permit, operation, AgentPolicyInstant::from_millis(NOW))
            .unwrap();
        let native = active.native_request().unwrap();
        assert_eq!(native.target(), &destination);
        assert_eq!(native.document_policy(), policy);
        assert!(native.redirect_policy().is_none());
        let result = f.policy.settle_navigation(
            &active,
            &ContextNavigationSettlement::try_new(operation, Ok(effective.clone())).unwrap(),
            AgentPolicyInstant::from_millis(NOW),
        );
        assert_eq!(result.is_ok(), accepted, "{effective:?}");
        if !accepted {
            continue;
        }
        let receipt = result.unwrap();
        assert!(receipt.matches_source(&baseline(&observed), &destination));
        assert!(!receipt.matches_source(&baseline(&observed), &effective));
        assert_eq!(receipt.progress_id(), active.progress_id());
        let successor = discovery_observation(operation.context(), 2);
        let request = call_request(2, f.lease, account(operation.context(), NOW), 0, 0, 0, NOW);
        let checkpoint = f
            .policy
            .provider_navigation_checkpoint(request, &successor)
            .unwrap()
            .unwrap();
        assert_eq!(checkpoint.current_document(), Some(&effective));
        assert_eq!(checkpoint.current_requested_document(), Some(&destination));
        f.policy.navigation_effective_destinations[0] = Some(destination.clone());
        assert!(
            f.policy
                .provider_navigation_checkpoint(request, &successor)
                .is_err(),
            "even an otherwise policy-valid effective substitution must fail the receipt hash"
        );
    }
}

fn committed_route() -> (
    PolicyFixture,
    Vec<(AgentActiveNavigation, AgentNavigationReceipt)>,
) {
    committed_navigation(false)
}
fn committed_navigation(
    discovery: bool,
) -> (
    PolicyFixture,
    Vec<(AgentActiveNavigation, AgentNavigationReceipt)>,
) {
    let (mut f, mut registry, mut current) =
        navigation_fixture(5, 10_000, vec![target(), final_target()], discovery);
    let mut terminals = vec![];
    for (hop, destination) in [target(), final_target()].into_iter().enumerate() {
        let binding = account(
            current.request().context(),
            if hop == 0 { NOW - 1 } else { NOW },
        );
        if hop > 0 {
            commit_observation_to_model(&mut f.policy, f.lease, 2, binding, &current);
        }
        let request = route_request(&f, &registry, &current, binding);
        let permit = f
            .policy
            .authorize_navigation(request, &current, &baseline(&current), &destination)
            .unwrap();
        let operation = registry
            .begin_navigation(
                current.request().context().identity().id(),
                ContextOperationId::new(2 + hop as u64).unwrap(),
            )
            .unwrap();
        let active = f
            .policy
            .dispatch_navigation(permit, operation, AgentPolicyInstant::from_millis(NOW))
            .unwrap();
        let receipt = f
            .policy
            .settle_navigation(
                &active,
                &ContextNavigationSettlement::try_new(operation, Ok(destination)).unwrap(),
                AgentPolicyInstant::from_millis(NOW),
            )
            .unwrap();
        registry
            .settle_navigation(
                operation.context().identity().id(),
                operation,
                ContextSettlement::Applied,
            )
            .unwrap();
        registry
            .acknowledge_observation(operation.context().identity().id(), operation.context())
            .unwrap();
        current = if discovery {
            discovery_observation(operation.context(), hop as u64 + 2)
        } else {
            actionable_observation(operation.context(), origin("source"), hop as u64 + 2)
        };
        terminals.push((active, receipt));
    }
    (f, terminals)
}

#[test]
fn discovery_progress_urls_join_exact_committed_receipts_and_current_document() {
    for fault in 0..7 {
        let (mut f, terminals) = committed_navigation(true);
        let context = terminals[1].1.operation().context();
        let observed = discovery_observation(context, 3);
        match fault {
            1 => f.policy.navigation_destinations[0] = None,
            2 => f.policy.navigation_destinations.swap(0, 1),
            3 => f.policy.navigation_destinations[1] = Some(target()),
            4 => f.policy.navigation_receipts[1] = None,
            5 => {
                f.policy.navigation_destinations[0] =
                    Some(ContextNavigationTarget::parse("https://other.invalid/foreign").unwrap())
            }
            _ => {}
        }
        let request = call_request(
            3,
            f.lease,
            account(
                if fault == 6 {
                    terminals[0].1.operation().context()
                } else {
                    context
                },
                NOW,
            ),
            0,
            0,
            0,
            NOW,
        );
        let checkpoint = f.policy.provider_navigation_checkpoint(request, &observed);
        if fault != 0 {
            assert!(checkpoint.is_err(), "substituted URL/context {fault}");
            continue;
        }
        let checkpoint = checkpoint.unwrap().unwrap();
        assert_eq!(checkpoint.current_document(), Some(&final_target()));
        assert_eq!(
            checkpoint.prior_documents().collect::<Vec<_>>(),
            vec![
                &ContextNavigationTarget::parse("https://source.example.test/start").unwrap(),
                &target()
            ]
        );
        assert_eq!(checkpoint.completed_hops(), 2);
        assert!(checkpoint.next_target().is_none());
        assert!(!format!("{:?}", terminals[0].1).contains("https://"));
    }
    let (f, _, observed) = navigation_fixture(5, 10_000, vec![target(), final_target()], true);
    let checkpoint = f
        .policy
        .provider_navigation_checkpoint(
            call_request(
                2,
                f.lease,
                account(observed.request().context(), NOW),
                0,
                0,
                0,
                NOW,
            ),
            &observed,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        checkpoint.current_document().unwrap().as_url().as_str(),
        "https://source.example.test/start"
    );
    assert_eq!(checkpoint.prior_documents().count(), 0);
}

#[test]
fn historical_extraction_requires_the_exact_discovery_receipt_chain() {
    for fault in 0..6 {
        let (mut fixture, terminals) = committed_navigation(true);
        let current = terminals[1].1.operation().context();
        let request = call_request(
            3,
            if fault == 5 {
                AgentPlanLeaseId::generate()
            } else {
                fixture.lease
            },
            if fault == 3 {
                AgentContextAccountBinding::new(
                    AgentAccountAttestationId::generate(),
                    current,
                    AgentAccountScope::Authenticated(AgentAccountId::generate()),
                    AgentPolicyInstant::from_millis(NOW),
                )
            } else {
                account(current, NOW)
            },
            0,
            0,
            0,
            NOW,
        );
        match fault {
            1 => fixture.policy.navigation_receipts[0] = None,
            2 => fixture.policy.navigation_receipts.swap(0, 1),
            4 => fixture.policy.navigation_attempts = 1,
            _ => {}
        }
        let historical = fixture
            .policy
            .historical_extraction_contexts(request, current);
        if fault != 0 {
            assert!(historical.is_err(), "changed history {fault} must refuse");
            continue;
        }
        let historical = historical.unwrap();
        assert_eq!(
            historical,
            vec![terminals[1].1.source(), terminals[0].1.source()]
        );
        let node = fixture
            .policy
            .manifest()
            .plan_node(AgentPlanNodeId::from_raw(1))
            .unwrap();
        let candidates = fixture.policy.taints.clone();
        assert_eq!(
            validate_context_scope(
                fixture.policy.manifest(),
                node,
                current,
                request.account(),
                &candidates
            ),
            Err(AgentPolicyError::SourceOutsideScope)
        );
        assert!(validate_context_scope_with_history(
            fixture.policy.manifest(),
            node,
            current,
            request.account(),
            &candidates,
            &historical
        )
        .is_ok());
    }

    // Fixed-route qualifications still require their terminal document. They
    // cannot gain historical extraction merely because some receipts exist.
    let (fixture, terminals) = committed_navigation(false);
    let current = terminals[1].1.operation().context();
    let request = call_request(3, fixture.lease, account(current, NOW), 0, 0, 0, NOW);
    assert!(fixture
        .policy
        .historical_extraction_contexts(request, current)
        .unwrap()
        .is_empty());
}

#[test]
fn historical_extraction_departed_read_rejoins_original_provider_taint() {
    let (fixture, terminals) = committed_navigation(true);
    let first = discovery_observation(terminals[0].1.source(), 1);
    let last = discovery_observation(terminals[1].1.operation().context(), 3);
    let first_ack = baseline(&first);
    let last_ack = baseline(&last);
    let read = |observation, acknowledgement| {
        read_semantic_observation(
            observation,
            SemanticReadAuthority::Acknowledged(acknowledgement),
            SemanticCaptureInstant::from_millis(NOW),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .unwrap()
    };
    let mut evidence = crate::SemanticRetainedReadEvidence::default();
    evidence
        .retain(&read(&first, &first_ack), &first_ack)
        .unwrap();
    assert!(evidence
        .merge_for_extraction(read(&last, &last_ack))
        .is_err());
    assert!(evidence.advance_after_navigation(terminals[1].1).is_err());
    evidence.advance_after_navigation(terminals[0].1).unwrap();
    assert!(evidence.advance_after_navigation(terminals[0].1).is_err());
    evidence.advance_after_navigation(terminals[1].1).unwrap();
    let merged = evidence
        .merge_for_extraction(read(&last, &last_ack))
        .unwrap();
    assert!(merged
        .fragments()
        .iter()
        .any(|fragment| fragment.provenance().context() == first.request().context()));
    assert!(crate::encode_semantic_read(
        &merged,
        SemanticModelEncodingBudget::EXTRACTION_PROVIDER_EXACT_CONSERVATIVE
    )
    .is_ok());

    let binding = account(last.request().context(), NOW);
    let mut taints = fixture.policy.taints.clone();
    taints.extend(observation_taints(&last, binding).unwrap());
    assert!(provider_extraction_read_taints(&merged, &last_ack, binding, &taints).is_ok());
    assert_eq!(
        provider_read_taints(&merged, &last_ack, binding, &taints),
        Err(AgentPolicyError::Authority)
    );
    let current_only = observation_taints(&last, binding).unwrap();
    assert_eq!(
        provider_extraction_read_taints(&merged, &last_ack, binding, &current_only),
        Err(AgentPolicyError::ReadBaselineMissing)
    );
}

#[test]
fn provider_route_checkpoint_is_derived_from_exact_ordered_committed_receipts() {
    for fault in 0..13 {
        let (mut f, terminals) = committed_route();
        let source = terminals[1].1.operation().context();
        let prior = terminals[0].1.operation().context();
        let observation =
            actionable_observation(if fault == 6 { prior } else { source }, origin("source"), 3);
        let binding = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            if matches!(fault, 6 | 7) {
                prior
            } else {
                source
            },
            if fault == 8 {
                AgentAccountScope::Authenticated(AgentAccountId::generate())
            } else {
                AgentAccountScope::Anonymous
            },
            AgentPolicyInstant::from_millis(if fault == 9 { NOW - 1 } else { NOW }),
        );
        match fault {
            1 => f.policy.navigation_receipts.swap(0, 1),
            2 => f.policy.navigation_receipts[0] = None,
            3 => f.policy.navigation_receipts[1] = f.policy.navigation_receipts[0],
            4 => f.policy.navigation_attempts = 1,
            5 => f.policy.navigation_attempts = 3,
            11 => f.policy.manifest = route_fixture(6).0.policy.manifest,
            12 => {
                f.policy.manifest =
                    route_fixture_with_targets(5, 10_000, vec![final_target(), target()])
                        .0
                        .policy
                        .manifest
            }
            _ => {}
        }
        let request = call_request(
            3,
            if fault == 10 {
                AgentPlanLeaseId::generate()
            } else {
                f.lease
            },
            binding,
            0,
            0,
            0,
            NOW,
        );
        let result = f
            .policy
            .provider_navigation_checkpoint(request, &observation);
        if fault == 0 {
            let checkpoint = result.unwrap().unwrap();
            assert_eq!(checkpoint.completed_hops(), 2);
            assert_eq!(checkpoint.total_hops(), 2);
            assert!(checkpoint.next_target().is_none());
        } else {
            assert!(result.is_err(), "checkpoint substitution {fault}");
        }
        assert_eq!(f.policy.pending_model_calls(), 0);
        assert_eq!(f.policy.accounting().reserved_operations(), 0);
    }
    let (mut f, terminals) = committed_route();
    f.policy.navigation_attempts = 1;
    f.policy.navigation_receipts[1] = None;
    let middle = actionable_observation(terminals[0].1.operation().context(), origin("source"), 2);
    let checkpoint = f
        .policy
        .provider_navigation_checkpoint(
            call_request(
                3,
                f.lease,
                account(middle.request().context(), NOW),
                0,
                0,
                0,
                NOW,
            ),
            &middle,
        )
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.completed_hops(), 1);
    assert_eq!(checkpoint.next_target(), Some(&final_target()));
    let retained_middle = checkpoint.binding();
    f.policy.navigation_attempts = 2;
    f.policy.navigation_receipts[1] = Some(terminals[1].1);
    let fresh = call_request(
        3,
        f.lease,
        account(terminals[1].1.operation().context(), NOW),
        0,
        0,
        0,
        NOW,
    );
    assert!(
        f.policy
            .validate_provider_navigation_checkpoint(fresh, retained_middle)
            .is_err(),
        "current policy cannot admit a replay carrying a retired document checkpoint"
    );
    let (f, _, observation) = route_fixture(5);
    let checkpoint = f
        .policy
        .provider_navigation_checkpoint(
            call_request(
                2,
                f.lease,
                account(observation.request().context(), NOW),
                0,
                0,
                0,
                NOW,
            ),
            &observation,
        )
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.completed_hops(), 0);
    assert_eq!(checkpoint.next_target(), Some(&target()));
}

#[test]
fn provider_route_checkpoint_rejects_pending_cancelled_and_failed_navigation() {
    for fault in 0..3 {
        let (mut f, mut registry, observation) = route_fixture(5);
        let request = route_request(
            &f,
            &registry,
            &observation,
            account(observation.request().context(), NOW),
        );
        let permit = f
            .policy
            .authorize_navigation(request, &observation, &baseline(&observation), &target())
            .unwrap();
        let mut successor = None;
        match fault {
            0 => {} // Original reservation is still active, with no terminal.
            1 => f.policy.cancel_navigation(permit).unwrap(),
            2 => {
                let operation = registry
                    .begin_navigation(
                        observation.request().context().identity().id(),
                        ContextOperationId::new(2).unwrap(),
                    )
                    .unwrap();
                let active = f
                    .policy
                    .dispatch_navigation(permit, operation, AgentPolicyInstant::from_millis(NOW))
                    .unwrap();
                f.policy
                    .refuse_navigation_dispatch(
                        &active,
                        ContextPortFailure::NativeRefused,
                        AgentPolicyInstant::from_millis(NOW),
                    )
                    .unwrap();
                successor = Some(actionable_observation(
                    operation.context(),
                    origin("source"),
                    2,
                ));
            }
            _ => unreachable!(),
        }
        let current = successor.as_ref().unwrap_or(&observation);
        assert!(f
            .policy
            .provider_navigation_checkpoint(
                call_request(
                    2,
                    f.lease,
                    account(current.request().context(), NOW),
                    0,
                    0,
                    0,
                    NOW
                ),
                current,
            )
            .is_err());
        assert_eq!(f.policy.pending_model_calls(), 0);
        assert_eq!(
            f.policy.accounting().reserved_operations(),
            u32::from(fault == 0)
        );
    }
}

#[test]
fn route_provider_request_counts_trusted_checkpoint_and_refuses_unsupported_or_secret_input() {
    for fault in 0..5 {
        let next = if fault == 4 {
            ContextNavigationTarget::parse(
                "https://source.example.test/sk-abcdefghijklmnop1234567890",
            )
            .unwrap()
        } else {
            target()
        };
        let (mut f, _, observation) =
            route_fixture_with_targets(5, 100_000, vec![next, final_target()]);
        let selected = tokenizer();
        let payload = encode_semantic_observation(
            &observation,
            SemanticModelEncodingBudget::INITIAL_CONSERVATIVE,
        )
        .unwrap()
        .admit_conservative_utf8(&selected)
        .unwrap();
        let objective = AgentProviderObjective::try_admit_conservative_utf8(
            "Complete the reviewed workflow".to_owned(),
            &selected,
        )
        .unwrap();
        let request = call_request(
            2,
            f.lease,
            account(observation.request().context(), NOW),
            if fault == 3 { 0 } else { 64_000 },
            128,
            1_000,
            NOW,
        );
        let config = provider_exact_config(selected, 128, 64_000);
        let result = match fault {
            1 => AgentPreparedObservationRequest::try_openai(
                &mut f.policy,
                request,
                &observation,
                payload,
                &objective,
                config,
            ),
            2 => AgentPreparedObservationRequest::try_anthropic(
                &mut f.policy,
                request,
                &observation,
                payload,
                &objective,
                config,
            ),
            _ => AgentPreparedObservationRequest::try_openai_for_provider_exact_count(
                &mut f.policy,
                request,
                &observation,
                payload,
                &objective,
                config,
            ),
        };
        if fault == 0 {
            let prepared = result.unwrap();
            let body: Value = serde_json::from_slice(prepared.request().body()).unwrap();
            let input = body["input"].as_array().unwrap();
            assert_eq!(input.len(), 3);
            assert_eq!(input[0]["role"], "user");
            assert_eq!(
                input[0]["content"][0]["text"],
                "Complete the reviewed workflow"
            );
            assert_eq!(input[1]["role"], "user");
            assert_eq!(input[2]["role"], "developer");
            let checkpoint = input[2]["content"][0]["text"].as_str().unwrap();
            assert!(checkpoint.starts_with("ZEPHIUM_HOST_NAVIGATION_CHECKPOINT_V1\n"));
            let progress: Value = serde_json::from_str(checkpoint.lines().last().unwrap()).unwrap();
            assert_eq!(
                progress,
                json!({"completed_hops":0,"total_hops":2,"next_navigation_target":target().as_url().as_str()})
            );
            assert_eq!(
                f.policy.accounting().reserved_model_tokens(),
                prepared.request().body().len() as u64 + 128
            );
            assert!(!format!("{prepared:?}").contains("source.example.test"));
            let count: Value = serde_json::from_slice(
                prepared
                    .request()
                    .openai_input_token_request()
                    .unwrap()
                    .body(),
            )
            .unwrap();
            assert_eq!(
                count["input"], body["input"],
                "the exact count includes trusted progress"
            );
            let retained = input[0]["content"][0]["text"].as_str().unwrap().len()
                + input[1]["content"][0]["text"].as_str().unwrap().len()
                + checkpoint.len();
            let transport = prepared.into_transport_input();
            assert_eq!(transport.continuation_transcript_bytes(), Some(retained));
            let _ = transport.cancel(&mut f.policy).unwrap();
        } else {
            assert!(
                result.is_err(),
                "unsupported/accounting/privacy fault {fault}"
            );
        }
        assert_eq!(f.policy.pending_model_calls(), 0);
        assert_eq!(f.policy.accounting().reserved_operations(), 0);
    }
}

#[test]
fn route_locate_replay_revalidates_progress_before_new_policy_reservation() {
    for fault in 0..3 {
        let (mut f, mut registry, observation) =
            route_fixture_with_targets(6, 100_000, vec![target(), final_target()]);
        let selected = tokenizer();
        let config = provider_exact_config(selected.clone(), 128, 64_000);
        let binding = account(observation.request().context(), NOW);
        let objective = AgentProviderObjective::try_admit_conservative_utf8(
            "Complete the approved route".to_owned(),
            &selected,
        )
        .unwrap();
        let payload = encode_semantic_observation(
            &observation,
            SemanticModelEncodingBudget::INITIAL_CONSERVATIVE,
        )
        .unwrap()
        .admit_conservative_utf8(&selected)
        .unwrap();
        let committed = AgentPreparedObservationRequest::try_openai_for_provider_exact_count(
            &mut f.policy,
            call_request(2, f.lease, binding, 64_000, 128, 1_000, NOW),
            &observation,
            payload,
            &objective,
            config.clone(),
        )
        .unwrap()
        .into_transport_input()
        .commit(&mut f.policy)
        .unwrap();
        let (initial, input, seed) = committed.into_parts();
        let (active, evidence) = input.into_parts();
        let baseline = evidence.observation_acknowledgement().unwrap().clone();
        f.policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 65, 4, 80)
            .unwrap();
        let arguments = r#"{"semantic_query":"save draft","scope":{"kind":"initial"}}"#;
        let tool = crate::AgentBrowserToolCall::decode_openai(
            initial.call(),
            "fc_route_2".to_owned(),
            "call_route_2".to_owned(),
            "locate",
            arguments.to_owned(),
        )
        .unwrap();
        let completion = crate::AgentProviderCompletion::new(
            initial.call(),
            crate::AgentProviderStopReason::ToolCalls,
            crate::AgentProviderUsage::try_new(65, 4, 0, 0, 0).unwrap(),
            crate::AgentProviderStreamStats::new(200, 8, 0, 1, arguments.len() as u32),
            true,
        );
        let continuation = seed
            .unwrap()
            .join_terminal_tool_for_test(completion, tool.into_continuation_parts_for_test().0)
            .unwrap();
        let frames = observation
            .frames()
            .iter()
            .map(|frame| frame.frame().clone())
            .collect::<Vec<_>>();
        let locate = SemanticLocateRequest::bind(
            SemanticLocateId::new(1).unwrap(),
            &observation,
            &baseline,
            &frames,
            SemanticLocateQuery::try_new("save draft".to_owned()).unwrap(),
            SemanticLocateScope::Initial,
            SemanticLocateBudget::STANDARD,
        )
        .unwrap();
        let result = locate_semantic_observation(&observation, locate).unwrap();
        let payload = encode_semantic_locate_result(
            &result,
            SemanticModelEncodingBudget::LOCATE_RESULT_PROVIDER_EXACT_CONSERVATIVE,
        )
        .unwrap()
        .admit_conservative_utf8(&selected)
        .unwrap();
        let mut request = call_request(3, f.lease, binding, 64_000, 128, 1_000, NOW);
        let draft = AgentProviderLocateRequestDraft::try_new(
            continuation
                .bind_locate_request(request, &config, &result, payload)
                .unwrap(),
        )
        .unwrap();
        if fault > 0 {
            let auth = route_request(&f, &registry, &observation, binding);
            let permit = f
                .policy
                .authorize_navigation(auth, &observation, &baseline, &target())
                .unwrap();
            let operation = registry
                .begin_navigation(
                    observation.request().context().identity().id(),
                    ContextOperationId::new(2).unwrap(),
                )
                .unwrap();
            let active = f
                .policy
                .dispatch_navigation(permit, operation, AgentPolicyInstant::from_millis(NOW))
                .unwrap();
            f.policy
                .settle_navigation(
                    &active,
                    &ContextNavigationSettlement::try_new(operation, Ok(target())).unwrap(),
                    AgentPolicyInstant::from_millis(NOW),
                )
                .unwrap();
            registry
                .settle_navigation(
                    operation.context().identity().id(),
                    operation,
                    ContextSettlement::Applied,
                )
                .unwrap();
            if fault == 2 {
                request = call_request(
                    3,
                    f.lease,
                    account(operation.context(), NOW),
                    64_000,
                    128,
                    1_000,
                    NOW,
                );
            }
        }
        let prepared = draft.try_prepare_for_provider_exact_count(&mut f.policy, request, &result);
        if fault == 0 {
            let _ = prepared
                .unwrap()
                .into_transport_input()
                .cancel(&mut f.policy)
                .unwrap();
        } else {
            assert!(
                matches!(
                    prepared,
                    Err(crate::AgentProviderRequestError::Policy(
                        AgentPolicyError::Navigation
                    ))
                ),
                "old checkpoint cannot replay under either old or successor account context"
            );
        }
        assert_eq!(f.policy.pending_model_calls(), 0);
        assert_eq!(f.policy.accounting().reserved_operations(), 0);
        assert_eq!(f.policy.pending_navigations(), 0);
    }
}

#[test]
fn finite_navigation_route_metrics_require_two_distinct_ordered_exact_terminals() {
    for fault in 0..4 {
        let (f, terminals) = committed_route();
        let root = AgentPlanNodeId::from_raw(1);
        let mut supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(1).unwrap(),
            AgentDelegationTopology::try_new(
                f.policy.manifest(),
                vec![AgentDelegationSpec::new(root, None)],
            )
            .unwrap(),
        );
        let mut accounting =
            crate::AgentRunAccountingMetrics::try_new(f.policy.manifest(), &supervisor).unwrap();
        let mut progress =
            crate::AgentRunProgressMetrics::try_new(f.policy.manifest(), &supervisor).unwrap();
        let mut audit = crate::AgentAuditLedger::try_new(f.policy.manifest(), &supervisor).unwrap();
        let record = |audit: &mut crate::AgentAuditLedger,
                      supervisor: &AgentRunSupervisor,
                      progress: &mut crate::AgentRunProgressMetrics,
                      id| {
            let event = audit
                .record_current(
                    supervisor,
                    root,
                    crate::AgentAuditEventId::new(id).unwrap(),
                    AgentPolicyInstant::from_millis(NOW + id),
                )
                .unwrap();
            let bytes = event.persistence_record();
            assert_eq!(bytes.as_bytes().len(), 128);
            assert!(!bytes
                .as_bytes()
                .windows(7)
                .any(|bytes| bytes == b"https://"));
            progress.record_event(event)
        };
        record(&mut audit, &supervisor, &mut progress, 1).unwrap();
        let execution = supervisor
            .start(root, crate::AgentSupervisorAttemptId::new(1).unwrap())
            .unwrap();
        record(&mut audit, &supervisor, &mut progress, 2).unwrap();
        if fault == 1 {
            assert_eq!(
                accounting
                    .record_navigation_receipt(terminals[1].1)
                    .unwrap_err(),
                crate::AgentMetricError::ReceiptReplay
            );
            assert_eq!(accounting.snapshot().navigations(), 0);
        }
        for (hop, (active, receipt)) in terminals.iter().enumerate() {
            supervisor
                .record_active_navigation(
                    &execution,
                    if fault == 3 && hop == 1 {
                        &terminals[0].0
                    } else {
                        active
                    },
                )
                .unwrap();
            let result = record(&mut audit, &supervisor, &mut progress, 3 + hop as u64 * 2);
            if fault == 3 && hop == 1 {
                assert_eq!(
                    result.unwrap_err(),
                    crate::AgentProgressMetricError::OperationSequence
                );
                assert_eq!(progress.snapshot().navigation().unwrap().samples(), 1);
                break;
            }
            result.unwrap();
            supervisor
                .record_navigation_result(
                    &execution,
                    if fault == 2 { terminals[1].1 } else { *receipt },
                )
                .unwrap();
            let result = record(&mut audit, &supervisor, &mut progress, 4 + hop as u64 * 2);
            if fault == 2 {
                assert_eq!(
                    result.unwrap_err(),
                    crate::AgentProgressMetricError::OperationSequence
                );
                assert!(progress.snapshot().navigation().is_none());
                break;
            }
            result.unwrap();
            accounting.record_navigation_receipt(*receipt).unwrap();
            assert_eq!(
                accounting.record_navigation_receipt(*receipt).unwrap_err(),
                crate::AgentMetricError::ReceiptReplay
            );
            assert_eq!(accounting.snapshot().navigations(), hop as u32 + 1);
        }
        if fault < 2 {
            assert_eq!(accounting.snapshot().navigations(), 2);
            assert_eq!(accounting.snapshot().operations(), 2);
            assert_eq!(accounting.snapshot().effects().attempts(), 0);
            assert_eq!(
                accounting.snapshot().navigation(),
                None,
                "legacy accessor must not hide the second receipt"
            );
            assert_eq!(
                accounting.snapshot().navigation_receipts(),
                &f.policy.navigation_receipts
            );
            assert_eq!(progress.snapshot().navigation().unwrap().samples(), 2);
            assert_eq!(
                progress.snapshot().navigation_terminals(),
                &[
                    Some((terminals[0].1.progress_id(), terminals[0].1.settlement())),
                    Some((terminals[1].1.progress_id(), terminals[1].1.settlement()))
                ]
            );
        }
    }
}

fn route_request(
    f: &PolicyFixture,
    registry: &ContextRegistry,
    observation: &SemanticObservation,
    binding: AgentContextAccountBinding,
) -> AgentNavigationAuthorizationRequest {
    AgentNavigationAuthorizationRequest::new(
        f.lease,
        binding,
        registry
            .automation_state(observation.request().context().identity().id())
            .unwrap(),
        AgentPolicyInstant::from_millis(NOW),
    )
}

#[test]
fn finite_navigation_route_requires_each_exact_prior_checkpoint_and_retains_original_budget() {
    for fault in 0..12 {
        let (mut f, mut registry, source) = route_fixture(if fault == 11 { 3 } else { 5 });
        let request = route_request(
            &f,
            &registry,
            &source,
            account(source.request().context(), NOW - 1),
        );
        assert!(
            f.policy
                .authorize_navigation(request, &source, &baseline(&source), &final_target())
                .is_err(),
            "cannot skip first"
        );
        assert_eq!(f.policy.pending_navigations(), 0);
        let permit = f
            .policy
            .authorize_navigation(request, &source, &baseline(&source), &target())
            .unwrap();
        if fault == 1 {
            f.policy.cancel_navigation(permit).unwrap();
            assert!(f
                .policy
                .authorize_navigation(request, &source, &baseline(&source), &target())
                .is_err());
            assert!(f
                .policy
                .authorize_navigation(request, &source, &baseline(&source), &final_target())
                .is_err());
            continue;
        }
        let operation = registry
            .begin_navigation(
                source.request().context().identity().id(),
                ContextOperationId::new(2).unwrap(),
            )
            .unwrap();
        assert!(registry
            .acknowledge_observation(
                source.request().context().identity().id(),
                source.request().context()
            )
            .is_err());
        let active = f
            .policy
            .dispatch_navigation(permit, operation, AgentPolicyInstant::from_millis(NOW))
            .unwrap();
        assert!(active.native_request().unwrap().redirect_policy().is_none());
        let terminal = ContextNavigationSettlement::try_new(
            operation,
            if fault == 2 {
                Err(ContextPortFailure::Cancelled)
            } else {
                Ok(target())
            },
        )
        .unwrap();
        let receipt = f
            .policy
            .settle_navigation(&active, &terminal, AgentPolicyInstant::from_millis(NOW))
            .unwrap();
        registry
            .settle_navigation(
                operation.context().identity().id(),
                operation,
                ContextSettlement::Applied,
            )
            .unwrap();
        let fresh = actionable_observation(operation.context(), origin("source"), 2);
        registry
            .acknowledge_observation(operation.context().identity().id(), operation.context())
            .unwrap();
        let binding = if fault == 5 {
            account(source.request().context(), NOW)
        } else if fault == 6 {
            AgentContextAccountBinding::new(
                AgentAccountAttestationId::generate(),
                operation.context(),
                AgentAccountScope::Authenticated(AgentAccountId::generate()),
                AgentPolicyInstant::from_millis(NOW),
            )
        } else {
            account(operation.context(), if fault == 7 { NOW - 1 } else { NOW })
        };
        if !matches!(fault, 5 | 6 | 8) {
            commit_observation_to_model(&mut f.policy, f.lease, 2, binding, &fresh);
        }
        let mut request = route_request(&f, &registry, &fresh, binding);
        if fault == 9 {
            request = AgentNavigationAuthorizationRequest::new(
                f.lease,
                binding,
                registry
                    .automation_state(operation.context().identity().id())
                    .unwrap(),
                AgentPolicyInstant::from_millis(EXPIRES_AT),
            );
        }
        if fault == 10 {
            registry
                .observe_navigation_replacement(
                    operation.context().identity().id(),
                    operation.context(),
                )
                .unwrap();
            request = route_request(&f, &registry, &fresh, binding);
        }
        let departure = if fault == 4 { &source } else { &fresh };
        let destination = if fault == 3 { target() } else { final_target() };
        let result =
            f.policy
                .authorize_navigation(request, departure, &baseline(departure), &destination);
        if fault != 0 {
            assert!(result.is_err(), "second checkpoint fault {fault}");
            assert_eq!(f.policy.pending_navigations(), 0);
            assert_eq!(f.policy.navigation_receipts, [Some(receipt), None]);
            continue;
        }
        let permit = result.unwrap();
        let final_operation = registry
            .begin_navigation(
                operation.context().identity().id(),
                ContextOperationId::new(3).unwrap(),
            )
            .unwrap();
        assert!(registry
            .acknowledge_observation(operation.context().identity().id(), operation.context())
            .is_err());
        let second = f
            .policy
            .dispatch_navigation(
                permit,
                final_operation,
                AgentPolicyInstant::from_millis(NOW),
            )
            .unwrap();
        let second_receipt = f
            .policy
            .settle_navigation(
                &second,
                &ContextNavigationSettlement::try_new(final_operation, Ok(final_target())).unwrap(),
                AgentPolicyInstant::from_millis(NOW),
            )
            .unwrap();
        assert_eq!(receipt.hop(), 0);
        assert_eq!(second_receipt.hop(), 1);
        assert_ne!(receipt.progress_id(), second_receipt.progress_id());
        assert_eq!(second_receipt.source(), receipt.operation().context());
        assert_eq!(
            f.policy.navigation_receipts,
            [Some(receipt), Some(second_receipt)]
        );
        assert_eq!(f.policy.accounting().consumed_operations(), 4);
        assert_eq!(
            f.policy
                .lease_accounting(f.lease)
                .unwrap()
                .consumed_operations(),
            4
        );
        assert_eq!(f.policy.accounting().reserved_operations(), 0);
        assert_eq!(f.policy.taints().len(), 2);
        assert!(
            f.policy
                .authorize_navigation(request, &fresh, &baseline(&fresh), &final_target())
                .is_err(),
            "route exhausted"
        );
        assert!(
            f.policy
                .authorize_navigation(request, &fresh, &baseline(&fresh), &target())
                .is_err(),
            "cannot repeat"
        );
    }
}

fn target() -> ContextNavigationTarget {
    ContextNavigationTarget::parse("https://source.example.test/next").unwrap()
}

#[test]
fn navigation_audit_identity_binds_full_authority_not_recyclable_operation_number() {
    fn active_fixture(
        target: ContextNavigationTarget,
    ) -> (PolicyFixture, AgentActiveNavigation, AgentNavigationReceipt) {
        let (mut f, mut registry, observation, auth) = fixture(true, 5);
        let permit = f
            .policy
            .authorize_navigation(auth, &observation, &baseline(&observation), &target)
            .unwrap();
        let operation = registry
            .begin_navigation(
                observation.request().context().identity().id(),
                ContextOperationId::new(2).unwrap(),
            )
            .unwrap();
        let active = f
            .policy
            .dispatch_navigation(permit, operation, AgentPolicyInstant::from_millis(NOW))
            .unwrap();
        let receipt = f
            .policy
            .settle_navigation(
                &active,
                &ContextNavigationSettlement::try_new(operation, Ok(target)).unwrap(),
                AgentPolicyInstant::from_millis(NOW),
            )
            .unwrap();
        (f, active, receipt)
    }
    for substitute in [false, true] {
        let (f, active, receipt) = active_fixture(target());
        let (_, alternate, other_receipt) = active_fixture(
            ContextNavigationTarget::parse("https://source.example.test/substitution").unwrap(),
        );
        assert_eq!(active.operation(), alternate.operation());
        assert_ne!(active.progress_id(), alternate.progress_id());
        assert_eq!(active.progress_id(), receipt.progress_id());
        let root = AgentPlanNodeId::from_raw(1);
        let mut supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(1).unwrap(),
            AgentDelegationTopology::try_new(
                f.policy.manifest(),
                vec![AgentDelegationSpec::new(root, None)],
            )
            .unwrap(),
        );
        let mut accounting =
            crate::AgentRunAccountingMetrics::try_new(f.policy.manifest(), &supervisor).unwrap();
        let mut progress =
            crate::AgentRunProgressMetrics::try_new(f.policy.manifest(), &supervisor).unwrap();
        let mut audit = crate::AgentAuditLedger::try_new(f.policy.manifest(), &supervisor).unwrap();
        let event = audit
            .record_current(
                &supervisor,
                root,
                crate::AgentAuditEventId::new(1).unwrap(),
                AgentPolicyInstant::from_millis(NOW),
            )
            .unwrap();
        progress.record_event(event).unwrap();
        let execution = supervisor
            .start(root, crate::AgentSupervisorAttemptId::new(1).unwrap())
            .unwrap();
        let event = audit
            .record_current(
                &supervisor,
                root,
                crate::AgentAuditEventId::new(2).unwrap(),
                AgentPolicyInstant::from_millis(NOW),
            )
            .unwrap();
        progress.record_event(event).unwrap();
        supervisor
            .record_active_navigation(&execution, &active)
            .unwrap();
        let event = audit
            .record_current(
                &supervisor,
                root,
                crate::AgentAuditEventId::new(3).unwrap(),
                AgentPolicyInstant::from_millis(NOW),
            )
            .unwrap();
        assert!(!event
            .persistence_record()
            .as_bytes()
            .windows(7)
            .any(|bytes| bytes == b"https://"));
        progress.record_event(event).unwrap();
        let final_receipt = if substitute { other_receipt } else { receipt };
        supervisor
            .record_navigation_result(&execution, final_receipt)
            .unwrap();
        let event = audit
            .record_current(
                &supervisor,
                root,
                crate::AgentAuditEventId::new(4).unwrap(),
                AgentPolicyInstant::from_millis(NOW + 1),
            )
            .unwrap();
        let result = progress.record_event(event);
        if substitute {
            assert_eq!(
                result.unwrap_err(),
                crate::AgentProgressMetricError::OperationSequence
            );
            assert!(progress.snapshot().navigation().is_none());
        } else {
            result.unwrap();
            assert_eq!(progress.snapshot().navigation().unwrap().samples(), 1);
            accounting.record_navigation_receipt(receipt).unwrap();
            assert_eq!(
                accounting.record_navigation_receipt(receipt).unwrap_err(),
                crate::AgentMetricError::ReceiptReplay
            );
            assert_eq!(accounting.snapshot().operations(), 1);
            assert_eq!(accounting.snapshot().effects().attempts(), 0);
            assert_eq!(accounting.snapshot().navigations(), 1);
        }
    }
}

#[test]
fn provider_navigation_checkpoint_is_exact_and_new_document_uses_original_policy() {
    for fault in 0..13 {
        let (mut f, mut registry, previous, auth) = fixture(false, 5);
        let config = provider_config(tokenizer(), 10, 20).restrict_to_navigation_and_extraction();
        let objective = AgentProviderObjective::try_admit(
            "Trusted objective survives document retirement".to_owned(),
            &FixedCounter {
                revision: tokenizer(),
                tokens: 5,
            },
            &tokenizer(),
        )
        .unwrap();
        let committed = AgentPreparedObservationRequest::try_openai(
            &mut f.policy,
            call_request(
                1,
                f.lease,
                account(previous.request().context(), NOW - 1),
                15,
                20,
                100,
                NOW,
            ),
            &previous,
            observation_payload(&previous, 50),
            &objective,
            config.clone(),
        )
        .unwrap()
        .into_transport_input()
        .commit(&mut f.policy)
        .unwrap();
        let (first, input, seed) = committed.into_parts();
        let (active, _) = input.into_parts();
        f.policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 65, 4, 80)
            .unwrap();
        let arguments = if fault == 12 {
            r#"{"semantic_query":"document","scope":{"kind":"initial"}}"#
        } else {
            r#"{"url":"https://source.example.test/next"}"#
        };
        let tool = crate::AgentBrowserToolCall::decode_openai(
            first.call(),
            "fc_old".into(),
            "call_old".into(),
            if fault == 12 { "locate" } else { "navigate" },
            arguments.to_owned(),
        )
        .unwrap();
        let completion = crate::AgentProviderCompletion::new(
            first.call(),
            crate::AgentProviderStopReason::ToolCalls,
            crate::AgentProviderUsage::try_new(65, 4, 0, 0, 0).unwrap(),
            crate::AgentProviderStreamStats::new(200, 8, 0, 1, arguments.len() as u32),
            true,
        );
        let continuation = seed
            .unwrap()
            .join_terminal_tool_for_test(completion, tool.into_continuation_parts_for_test().0)
            .unwrap();
        let changed = actionable_observation(previous.request().context(), origin("source"), 99);
        let checkpoint = continuation.retire_for_navigation(
            if fault == 11 { &changed } else { &previous },
            &if fault == 10 {
                ContextNavigationTarget::parse("https://source.example.test/other").unwrap()
            } else {
                target()
            },
            &config,
        );
        if fault >= 10 {
            assert!(checkpoint.is_err(), "retirement fault {fault}");
            continue;
        }
        let checkpoint = checkpoint.unwrap();
        assert!(!format!("{checkpoint:?}").contains("example.test"));
        let permit = f
            .policy
            .authorize_navigation(auth, &previous, checkpoint.baseline(), &target())
            .unwrap();
        let operation = registry
            .begin_navigation(
                previous.request().context().identity().id(),
                ContextOperationId::new(2).unwrap(),
            )
            .unwrap();
        let active = f
            .policy
            .dispatch_navigation(permit, operation, AgentPolicyInstant::from_millis(NOW))
            .unwrap();
        let receipt = f
            .policy
            .settle_navigation(
                &active,
                &ContextNavigationSettlement::try_new(
                    operation,
                    if fault == 9 {
                        Err(ContextPortFailure::Cancelled)
                    } else {
                        Ok(target())
                    },
                )
                .unwrap(),
                AgentPolicyInstant::from_millis(NOW + 1),
            )
            .unwrap();
        let fresh = actionable_observation(
            operation.context(),
            origin("source"),
            if fault == 5 { 1 } else { 2 },
        );
        let account = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            if fault == 6 {
                previous.request().context()
            } else {
                operation.context()
            },
            if fault == 7 {
                AgentAccountScope::Authenticated(AgentAccountId::generate())
            } else {
                AgentAccountScope::Anonymous
            },
            AgentPolicyInstant::from_millis(if fault == 8 { NOW } else { NOW + 1 }),
        );
        let request = call_request(
            if fault == 2 { 1 } else { 2 },
            if fault == 3 {
                AgentPlanLeaseId::generate()
            } else {
                f.lease
            },
            account,
            15,
            20,
            100,
            NOW + 2,
        );
        let altered_config = provider_config(tokenizer(), 10, 20);
        let result = checkpoint.validate_successor(
            receipt,
            if fault == 4 { &previous } else { &fresh },
            request,
            if fault == 1 { &altered_config } else { &config },
        );
        if fault != 0 {
            assert!(result.is_err(), "successor fault {fault}");
            continue;
        }
        result.unwrap();
        assert_eq!(f.policy.accounting().consumed_operations(), 2);
        assert_eq!(
            f.policy.taints().len(),
            1,
            "old disclosure remains a run taint"
        );
        let committed = AgentPreparedObservationRequest::try_openai(
            &mut f.policy,
            request,
            &fresh,
            observation_payload(&fresh, 50),
            &objective,
            config,
        )
        .unwrap()
        .into_transport_input()
        .commit(&mut f.policy)
        .unwrap();
        let (_, input, _) = committed.into_parts();
        let (active, _) = input.into_parts();
        f.policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 65, 4, 80)
            .unwrap();
        assert_eq!(f.policy.accounting().consumed_operations(), 3);
        assert_eq!(
            f.policy.taints().len(),
            2,
            "successor disclosure does not erase original taint"
        );
    }
}

fn baseline(observation: &SemanticObservation) -> SemanticObservationAcknowledgement {
    SemanticObservationAcknowledgement::from_fingerprint(
        SemanticObservationFingerprint::from_observation(observation),
    )
}

fn fixture(
    delivered: bool,
    operations: u32,
) -> (
    PolicyFixture,
    ContextRegistry,
    SemanticObservation,
    AgentNavigationAuthorizationRequest,
) {
    let (mut registry, context) = make_context_registry(901, 902, 903);
    let source = origin("source");
    let observation = actionable_observation(context, source.clone(), 1);
    registry
        .acknowledge_observation(context.identity().id(), context)
        .unwrap();
    let binding = account(context, NOW - 1);
    let request = AgentNavigationAuthorizationRequest::new(
        AgentPlanLeaseId::from_raw(1),
        binding,
        registry.automation_state(context.identity().id()).unwrap(),
        AgentPolicyInstant::from_millis(NOW),
    );
    let mut fixture = policy_fixture(
        901,
        902,
        source,
        SemanticSensitivity::Sensitive,
        &[SemanticEffectClass::Read],
        run_budget(operations, 10_000, 10_000),
    );
    if delivered {
        commit_observation_to_model(&mut fixture.policy, fixture.lease, 1, binding, &observation);
    }
    (fixture, registry, observation, request)
}

#[test]
fn navigation_revokes_source_before_dispatch_and_charges_one_distinct_attempt() {
    let (mut f, mut registry, observation, request) = fixture(true, 3);
    let source = observation.request().context();
    let baseline = baseline(&observation);
    let permit = f
        .policy
        .authorize_navigation(request, &observation, &baseline, &target())
        .unwrap();
    assert_eq!(f.policy.accounting().consumed_operations(), 1);
    assert_eq!(f.policy.accounting().reserved_operations(), 1);
    assert_eq!(f.policy.pending_effects(), 0);
    let payload = observation_payload(&observation, 10);
    assert_eq!(
        f.policy
            .prepare_observation_input(
                call_request(2, f.lease, account(source, NOW - 1), 0, 0, 0, NOW),
                &observation,
                &payload
            )
            .unwrap_err(),
        AgentPolicyError::Navigation
    );
    let operation = registry
        .begin_navigation(source.identity().id(), ContextOperationId::new(2).unwrap())
        .unwrap();
    assert_ne!(registry.join(source.identity().id()).unwrap(), source);
    assert!(registry
        .acknowledge_observation(source.identity().id(), source)
        .is_err());
    assert!(!registry
        .automation_state(source.identity().id())
        .unwrap()
        .can_automate());
    let active = f
        .policy
        .dispatch_navigation(permit, operation, AgentPolicyInstant::from_millis(NOW))
        .unwrap();
    let native = active.native_request().unwrap();
    assert_eq!(native.target(), &target());
    assert!(native.redirect_policy().is_none());
    let terminal = ContextNavigationSettlement::try_new(operation, Ok(target())).unwrap();
    let receipt = f
        .policy
        .settle_navigation(&active, &terminal, AgentPolicyInstant::from_millis(NOW))
        .unwrap();
    assert!(receipt.matches_source(&baseline, &target()));
    assert_eq!(receipt.source(), source);
    assert_eq!(receipt.operation(), operation);
    assert_eq!(f.policy.accounting().consumed_operations(), 2);
    assert_eq!(f.policy.accounting().reserved_operations(), 0);
    assert_eq!(f.policy.pending_navigations(), 0);
    assert_eq!(f.policy.pending_effects(), 0);
    assert_eq!(
        f.policy
            .authorize_navigation(request, &observation, &baseline, &target())
            .unwrap_err(),
        AgentPolicyError::Navigation
    );
    assert!(
        f.policy
            .settle_navigation(&active, &terminal, AgentPolicyInstant::from_millis(NOW))
            .is_err(),
        "receipt cannot replay"
    );
}

#[test]
fn navigation_rejects_undelivered_changed_cross_origin_fragment_and_exhausted_authority() {
    let (mut f, _, observation, request) = fixture(false, 3);
    assert_eq!(
        f.policy
            .authorize_navigation(request, &observation, &baseline(&observation), &target())
            .unwrap_err(),
        AgentPolicyError::ModelSourceMissing
    );
    for raw in [
        "https://other.example.test/next",
        "http://source.example.test/next",
        "https://source.example.test/next#part",
        "https://source.example.test:444/next",
    ] {
        let (mut f, _, observation, request) = fixture(true, 3);
        assert_eq!(
            f.policy
                .authorize_navigation(
                    request,
                    &observation,
                    &baseline(&observation),
                    &ContextNavigationTarget::parse(raw).unwrap()
                )
                .unwrap_err(),
            AgentPolicyError::Navigation
        );
        assert_eq!(f.policy.pending_navigations(), 0);
    }
    let (mut f, _, observation, request) = fixture(true, 1);
    assert!(f
        .policy
        .authorize_navigation(request, &observation, &baseline(&observation), &target())
        .is_err());
    let (mut f, _, observation, request) = fixture(true, 3);
    let changed = actionable_observation(observation.request().context(), origin("source"), 2);
    assert_eq!(
        f.policy
            .authorize_navigation(request, &changed, &baseline(&observation), &target())
            .unwrap_err(),
        AgentPolicyError::Navigation
    );
    assert_eq!(
        f.policy
            .authorize_navigation(request, &changed, &baseline(&changed), &target())
            .unwrap_err(),
        AgentPolicyError::ModelSourceMissing
    );
}

#[test]
fn navigation_rejects_pending_model_and_stale_lifecycle_or_account() {
    let (mut f, mut registry, observation, request) = fixture(true, 4);
    let payload = observation_payload(&observation, 10);
    let pending = f
        .policy
        .prepare_observation_input(
            call_request(
                2,
                f.lease,
                account(observation.request().context(), NOW - 1),
                0,
                0,
                0,
                NOW,
            ),
            &observation,
            &payload,
        )
        .unwrap();
    assert_eq!(
        f.policy
            .authorize_navigation(request, &observation, &baseline(&observation), &target())
            .unwrap_err(),
        AgentPolicyError::ModelCallPending
    );
    drop(pending); // Dropping admission does not silently retire its original policy debt.
    assert_eq!(f.policy.pending_model_calls(), 1);
    let (mut f2, _, _, _) = fixture(true, 4);
    let next = registry
        .begin_navigation(
            observation.request().context().identity().id(),
            ContextOperationId::new(2).unwrap(),
        )
        .unwrap();
    let request = AgentNavigationAuthorizationRequest::new(
        f2.lease,
        account(observation.request().context(), NOW - 1),
        registry
            .automation_state(next.context().identity().id())
            .unwrap(),
        AgentPolicyInstant::from_millis(NOW),
    );
    assert_eq!(
        f2.policy
            .authorize_navigation(request, &observation, &baseline(&observation), &target())
            .unwrap_err(),
        AgentPolicyError::Navigation
    );
    let (mut f, registry, observation, _) = fixture(true, 4);
    let request = AgentNavigationAuthorizationRequest::new(
        f.lease,
        account(next.context(), NOW - 1),
        registry
            .automation_state(observation.request().context().identity().id())
            .unwrap(),
        AgentPolicyInstant::from_millis(NOW),
    );
    assert_eq!(
        f.policy
            .authorize_navigation(request, &observation, &baseline(&observation), &target())
            .unwrap_err(),
        AgentPolicyError::Navigation
    );
}

#[test]
fn navigation_cancel_releases_only_undispatched_owner_and_cannot_mint_a_retry() {
    let (mut f, _, observation, request) = fixture(true, 3);
    let permit = f
        .policy
        .authorize_navigation(request, &observation, &baseline(&observation), &target())
        .unwrap();
    f.policy.cancel_navigation(permit).unwrap();
    assert_eq!(f.policy.pending_navigations(), 0);
    assert_eq!(f.policy.accounting().consumed_operations(), 1);
    assert!(f
        .policy
        .authorize_navigation(request, &observation, &baseline(&observation), &target())
        .is_err());
}

#[test]
fn navigation_foreign_terminal_and_redirect_leave_original_owner_but_failed_exact_attempt_settles()
{
    for mode in 0..5 {
        let (mut f, mut registry, observation, request) = fixture(true, 3);
        let permit = f
            .policy
            .authorize_navigation(request, &observation, &baseline(&observation), &target())
            .unwrap();
        let operation = registry
            .begin_navigation(
                observation.request().context().identity().id(),
                ContextOperationId::new(2).unwrap(),
            )
            .unwrap();
        let active = f
            .policy
            .dispatch_navigation(permit, operation, AgentPolicyInstant::from_millis(NOW))
            .unwrap();
        let receipt = if mode == 4 {
            f.policy.refuse_navigation_dispatch(
                &active,
                ContextPortFailure::Unsupported,
                AgentPolicyInstant::from_millis(NOW),
            )
        } else {
            let terminal_op = if mode == 1 {
                let (mut other, join) = make_context_registry(901, 902, 999);
                other
                    .begin_navigation(join.identity().id(), ContextOperationId::new(2).unwrap())
                    .unwrap()
            } else {
                operation
            };
            let outcome = match mode {
                0 => Ok(
                    ContextNavigationTarget::parse("https://source.example.test/redirect").unwrap(),
                ),
                2 => Err(ContextPortFailure::Cancelled),
                _ => Ok(target()),
            };
            f.policy.settle_navigation(
                &active,
                &ContextNavigationSettlement::try_new(terminal_op, outcome).unwrap(),
                AgentPolicyInstant::from_millis(if mode == 3 { NOW - 1 } else { NOW }),
            )
        };
        if matches!(mode, 0 | 1 | 3) {
            assert!(receipt.is_err());
            assert!(f.policy.is_sealed());
            assert_eq!(f.policy.pending_navigations(), 1);
            assert_eq!(f.policy.accounting().reserved_operations(), 1);
        } else {
            assert!(matches!(
                receipt.unwrap().settlement(),
                AgentNavigationSettlement::Failed(_)
            ));
            assert_eq!(f.policy.pending_navigations(), 0);
            assert_eq!(f.policy.accounting().consumed_operations(), 2);
        }
        assert_eq!(f.policy.pending_effects(), 0);
    }
}
