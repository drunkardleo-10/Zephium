use super::*;
use crate::{ContextNavigationSettlement, ContextNavigationTarget, ContextPortFailure};

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
