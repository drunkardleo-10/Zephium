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
