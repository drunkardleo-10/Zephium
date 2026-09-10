use super::*;
use std::{sync::Mutex, time::Duration};

fn identity() -> ContextIdentity {
    ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        1_u128.into(),
        ContextKind::Owned,
    )
}

fn objective() -> PublicReadWorkObjective {
    PublicReadWorkObjective {
        objective: "Find the release decisions that remain unresolved and cite the evidence."
            .into(),
        navigation: AgentNavigationDiscovery::try_new(
            ContextNavigationTarget::parse("https://example.test/project").unwrap(),
            "/project/".into(),
            2,
        )
        .unwrap(),
        output_fields: vec![
            SemanticExtractionFieldSchema::try_text("answer".into(), true, 2048).unwrap(),
        ],
    }
}

fn production_objective() -> PublicReadWorkObjective {
    let source = SemanticOrigin::parse("https://example.test").unwrap();
    let docs = SemanticOrigin::parse("https://docs.example.test").unwrap();
    PublicReadWorkObjective {
        objective: "Investigate the linked evidence and return a cited result.".into(),
        navigation: AgentNavigationDiscovery::try_new_production(
            ContextNavigationTarget::parse("https://example.test/project?q=open#today").unwrap(),
            vec![
                AgentNavigationOriginRule::try_new(source, "/project/".into(), true, true).unwrap(),
                AgentNavigationOriginRule::try_new(docs, "/guide/".into(), true, false).unwrap(),
            ],
            8,
            2,
        )
        .unwrap(),
        output_fields: vec![
            SemanticExtractionFieldSchema::try_text("answer".into(), true, 2048).unwrap(),
        ],
    }
}

fn settings(account: PublicReadWorkAccount) -> PublicReadWorkSettings {
    PublicReadWorkSettings {
        account,
        model: AgentBrowserModel::Luna,
        budget: AgentRunBudget::try_new(64, 100_000, 100_000, 1).unwrap(),
        max_model_calls: 24,
        deadline: Instant::now() + Duration::from_secs(120),
    }
}

fn join(identity: ContextIdentity) -> ContextJoin {
    let mut registry = ContextRegistry::new();
    registry
        .reserve(
            identity,
            ContextCapabilities::try_new(ContextKind::Owned, &[]).unwrap(),
        )
        .unwrap();
    registry
        .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
        .unwrap()
        .context()
}

#[test]
fn ordinary_admission_preserves_scope_storage_and_absolute_clock_without_a_route() {
    for storage in [
        ContextProfileStorageClass::Ephemeral,
        ContextProfileStorageClass::Durable,
    ] {
        let identity = identity();
        let definition = objective();
        let scope = definition.navigation.clone();
        let limits = settings(PublicReadWorkAccount::Anonymous);
        let deadline = limits.deadline;
        let (input, task) = assemble(identity, storage, definition, limits).unwrap();
        let resource = input.retained_resource_spec().unwrap();
        assert_eq!(resource.identity, identity);
        assert_eq!(resource.storage, storage);
        assert_eq!(&resource.target, scope.departure());
        assert_eq!(resource.document_policy, scope.document_policy());
        crate::native_work_clock::assert_native_timing(&input, deadline);
        assert_eq!(task.navigation_discovery(), Some(&scope));
        assert!(task.navigation_route().is_none());
        assert!(task.navigation_target().is_none());
        assert!(task.allows_baseline_read());
        assert!(task.allows_progressive_observation());
        assert!(!task.allows_actions_before_extraction());
        assert!(task.extraction_schema().is_some());
        assert_eq!(
            task.attest_account(join(identity), resource.clock.now().unwrap())
                .unwrap()
                .account(),
            AgentAccountScope::Anonymous
        );
    }
}

#[test]
fn production_admission_freezes_every_origin_and_enables_bounded_visual_wait_tools() {
    let identity = identity();
    let definition = production_objective();
    let expected = definition.navigation.clone();
    let (input, task) = assemble(
        identity,
        ContextProfileStorageClass::Durable,
        definition,
        settings(PublicReadWorkAccount::Anonymous),
    )
    .unwrap();
    let node = input.manifest.plan_node(input.lease.node()).unwrap();
    assert_eq!(node.origins().len(), 2);
    assert_eq!(input.manifest.scope().origins().len(), 2);
    assert_eq!(task.navigation_discovery(), Some(&expected));
    assert!(task.allows_standalone_wait());
    assert!(task.allows_viewport_screenshot());
    assert!(task.allows_human_request());
}

struct Samples(Arc<Mutex<Option<AgentContextAccountBinding>>>);
impl AgentWorkAccountSource for Samples {
    fn sample(&self, _: ContextJoin) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        self.0.lock().unwrap().ok_or(AgentWorkFailure::Contract)
    }
}

#[test]
fn selected_profile_does_not_supply_authenticated_account_facts() {
    let identity = identity();
    let context = join(identity);
    let account = AgentAccountId::generate();
    let samples = Arc::new(Mutex::new(None));
    let (input, task) = assemble(
        identity,
        ContextProfileStorageClass::Durable,
        objective(),
        settings(PublicReadWorkAccount::Identified {
            account,
            source: Box::new(Samples(samples.clone())),
        }),
    )
    .unwrap();
    let now = input.retained_resource_spec().unwrap().clock.now().unwrap();
    assert_eq!(
        task.attest_account(context, now),
        Err(AgentWorkFailure::Contract)
    );
    let sample = AgentContextAccountBinding::new(
        AgentAccountAttestationId::generate(),
        context,
        AgentAccountScope::Authenticated(account),
        now,
    );
    *samples.lock().unwrap() = Some(sample);
    assert_eq!(task.attest_account(context, now), Ok(sample));
    // Composition passes through original host evidence and does not restamp it.
    assert_eq!(
        task.attest_account(
            context,
            AgentPolicyInstant::from_millis(now.millis() + 60_000)
        ),
        Ok(sample)
    );
    *samples.lock().unwrap() = Some(AgentContextAccountBinding::new(
        AgentAccountAttestationId::generate(),
        context,
        AgentAccountScope::Authenticated(AgentAccountId::generate()),
        now,
    ));
    assert_eq!(
        task.attest_account(context, now),
        Err(AgentWorkFailure::Contract)
    );
}

#[test]
fn admission_rejects_empty_intent_invalid_schema_and_invalid_limits() {
    for invalid in 0..5 {
        let mut objective = objective();
        let mut limits = settings(PublicReadWorkAccount::Anonymous);
        match invalid {
            0 => objective.objective = " \n ".into(),
            1 => objective.output_fields.clear(),
            2 => limits.max_model_calls = 65,
            3 => limits.budget = AgentRunBudget::try_new(64, 100_000, 100_000, 2).unwrap(),
            4 => limits.deadline = Instant::now() - Duration::from_secs(1),
            _ => unreachable!(),
        }
        assert!(assemble(
            identity(),
            ContextProfileStorageClass::Ephemeral,
            objective,
            limits
        )
        .is_err());
    }
}

struct DormantAudit;
impl AgentAuditPort for DormantAudit {
    fn append(&self, _: AgentAuditDelivery, _: AgentAuditCompletion) -> AgentAuditDispatch {
        panic!("preparation must not execute audit effects")
    }
}

#[test]
fn assembled_scope_and_task_pass_ordinary_controller_admission() {
    for account in [
        PublicReadWorkAccount::Anonymous,
        PublicReadWorkAccount::Identified {
            account: AgentAccountId::generate(),
            source: Box::new(Samples(Arc::new(Mutex::new(None)))),
        },
    ] {
        let (input, task) = assemble(
            identity(),
            ContextProfileStorageClass::Durable,
            objective(),
            settings(account),
        )
        .unwrap();
        let result = zephium_agent_controller::AgentWorkController::try_new(
            input,
            zephium_agent_provider_transport::AgentProviderTransportConfig::STANDARD,
            AgentProviderCredential::try_new(
                AgentProviderKind::OpenAiResponses,
                "unused-test-credential".into(),
            )
            .unwrap(),
            Arc::new(DormantAudit),
            task,
        );
        assert!(result.is_ok());
    }
}

struct RefuseLocalActions;
impl AgentWorkLocalActionPolicy for RefuseLocalActions {
    fn model_action_operations(
        &self,
        _: &SemanticNode,
        _: &SemanticObservation,
    ) -> Result<SemanticOperations, AgentWorkFailure> {
        Ok(SemanticOperations::NONE)
    }

    fn assess(
        &self,
        _: &SemanticPreparedAction,
        _: &SemanticObservation,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
}

#[test]
fn ordinary_local_action_objective_keeps_discovery_account_and_result_contract() {
    let identity = identity();
    let (input, task) = assemble_with_actions(
        identity,
        ContextProfileStorageClass::Durable,
        objective(),
        settings(PublicReadWorkAccount::Anonymous),
        Some(LocalActions {
            policy: Box::new(RefuseLocalActions),
            max_actions: 3,
        }),
    )
    .unwrap();
    assert!(task.allows_actions_before_extraction());
    assert!(task.navigation_discovery().is_some());
    assert!(task.navigation_route().is_none());
    assert!(task.allows_progressive_observation());
    let input = input.persist_extraction_result().unwrap();
    assert!(zephium_agent_controller::AgentWorkController::try_new(
        input,
        zephium_agent_provider_transport::AgentProviderTransportConfig::STANDARD,
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "unused-test-credential".into()
        )
        .unwrap(),
        Arc::new(DormantAudit),
        task
    )
    .is_ok());
    for max_actions in [0, 65, u64::MAX] {
        assert!(assemble_with_actions(
            identity,
            ContextProfileStorageClass::Durable,
            objective(),
            settings(PublicReadWorkAccount::Anonymous),
            Some(LocalActions {
                policy: Box::new(RefuseLocalActions),
                max_actions
            })
        )
        .is_err());
    }
}
