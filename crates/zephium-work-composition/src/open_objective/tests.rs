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
            ContextNavigationTarget::parse("https://example.test/project/?q=open#today").unwrap(),
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
fn production_admission_preserves_navigation_and_enables_bounded_visual_wait_tools() {
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
    assert_eq!(input.retained_resource_spec().unwrap().identity, identity);
    assert_eq!(task.navigation_discovery(), Some(&expected));
    assert!(task.allows_standalone_wait());
    assert!(task.allows_viewport_screenshot());
    assert!(task.allows_human_request());
}

#[test]
fn public_discovery_product_budget_compiles_to_an_isolated_retained_resource() {
    let mut definition = objective();
    definition.navigation = AgentNavigationDiscovery::try_new_public_web(
        ContextNavigationTarget::parse("https://www.bing.com/search?q=Svelte+Flow+compatibility")
            .unwrap(),
        usize::from(zephium_core::work::runtime::WORK_PUBLIC_DISCOVERY_MAX_HOPS),
        2,
    )
    .unwrap();
    let mut limits = settings(PublicReadWorkAccount::Anonymous);
    limits.budget = AgentRunBudget::try_new(51, 51_200, 200_000, 1).unwrap();
    limits.deadline = Instant::now() + Duration::from_secs(600);
    let (input, _) = assemble(
        identity(),
        ContextProfileStorageClass::Durable,
        definition,
        limits,
    )
    .unwrap();
    assert!(input.retained_resource_spec().unwrap().isolated_public);
    assert!(input.persist_extraction_result().is_ok());
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
            read_only: false,
            commits: Vec::new(),
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
                max_actions,
                read_only: false,
                commits: Vec::new(),
            })
        )
        .is_err());
    }
}

#[cfg(feature = "durable-runtime")]
#[test]
fn public_interactions_preserve_anonymous_read_only_scope() {
    let mut definition = objective();
    definition.navigation = AgentNavigationDiscovery::try_new_public_page(
        ContextNavigationTarget::parse("https://example.com/catalog").unwrap(),
    )
    .unwrap();
    let (input, task) = assemble_with_actions(
        identity(),
        ContextProfileStorageClass::Durable,
        definition,
        settings(PublicReadWorkAccount::Anonymous),
        Some(LocalActions {
            policy: Box::new(read_interactions::ReadingInteractionPolicy),
            max_actions: 8,
            read_only: true,
            commits: Vec::new(),
        }),
    )
    .unwrap();
    assert!(task.allows_actions_before_extraction());
    assert_eq!(task.navigation_discovery().unwrap().max_hops(), 0);
    assert!(
        input
            .persist_extraction_result()
            .unwrap()
            .retained_resource_spec()
            .unwrap()
            .isolated_public
    );
}

#[cfg(feature = "durable-runtime")]
pub(super) fn reading_observation(
    nodes: serde_json::Value,
    completeness: &str,
) -> SemanticObservation {
    let context = join(identity());
    let frame = SemanticFrameJoin::try_new(
        context,
        FrameId::MAIN,
        context.frame_generation(),
        SemanticOrigin::parse("https://example.test/").unwrap(),
        SemanticFrameTrust::SameOrigin,
    )
    .unwrap();
    let wire = serde_json::json!({"v":1,"i":1,"g":1,"c":completeness,"n":nodes});
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(1).unwrap(),
            frame,
            SemanticSnapshotGeneration::new(1).unwrap(),
        ),
        &serde_json::to_vec(&wire).unwrap(),
    )
    .unwrap();
    SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(1).unwrap(),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        ),
        snapshot,
    )
    .unwrap()
    .finish()
    .unwrap()
}

#[cfg(feature = "durable-runtime")]
#[test]
fn reading_scroll_recipe_binds_with_position_change_verification() {
    let policy = read_interactions::ReadingInteractionPolicy;
    let observation = reading_observation(
        serde_json::json!([{"k":1,"r":"document","fc":true,"o":16,
            "b":{"x":0,"y":0,"w":800,"h":600}}]),
        "complete",
    );
    let snapshot = &observation.frames()[0];
    let target = snapshot.nodes()[0].reference();
    let recipe = policy
        .decision_action_recipe(&DecisionOperation::Scroll(target), &observation)
        .unwrap()
        .unwrap();
    assert_eq!(recipe.wait(), SemanticWaitCondition::Immediate);
    assert_eq!(
        recipe.verification(),
        SemanticVerification::ScrollPositionChanged
    );
    let batch = SemanticActionBatch::bind(
        SemanticActionBatchId::new(1).unwrap(),
        &observation,
        &[snapshot.frame().clone()],
        vec![recipe],
    )
    .unwrap();
    let action = batch.actions()[0].prepare(snapshot).unwrap();
    assert!(policy.assess(&action, &observation).is_ok());
}

#[cfg(feature = "durable-runtime")]
#[test]
fn reading_interactions_require_native_boundaries_and_intended_outcomes() {
    use serde_json::json;
    let policy = read_interactions::ReadingInteractionPolicy;
    assert_eq!(
        policy.model_action_effect(),
        Some(SemanticEffectClass::Read)
    );
    for (role, name, activation, dialog, allowed, proof) in [
        (
            "button",
            "Continue",
            1,
            true,
            true,
            SemanticVerification::PageDialogClosed,
        ),
        (
            "button",
            "Continue",
            2,
            true,
            false,
            SemanticVerification::PageDialogClosed,
        ),
        (
            "button",
            "Continue",
            3,
            true,
            false,
            SemanticVerification::PageDialogClosed,
        ),
        (
            "button",
            "Continue",
            4,
            true,
            false,
            SemanticVerification::PageDialogClosed,
        ),
        (
            "button",
            "Continue",
            5,
            true,
            false,
            SemanticVerification::PageDialogClosed,
        ),
        (
            "button",
            "Continue",
            1,
            false,
            false,
            SemanticVerification::PageDialogClosed,
        ),
        (
            "button",
            "Accept",
            1,
            true,
            false,
            SemanticVerification::PageDialogClosed,
        ),
        (
            "button",
            "Buy now",
            6,
            false,
            false,
            SemanticVerification::TargetState {
                state: SemanticState::Expanded,
                present: true,
            },
        ),
        (
            "button",
            "Specifications",
            6,
            false,
            true,
            SemanticVerification::TargetState {
                state: SemanticState::Expanded,
                present: true,
            },
        ),
        (
            "button",
            "Shop",
            6,
            false,
            true,
            SemanticVerification::TargetState {
                state: SemanticState::Expanded,
                present: false,
            },
        ),
        (
            "button",
            "Specifications",
            1,
            false,
            false,
            SemanticVerification::TargetState {
                state: SemanticState::Expanded,
                present: true,
            },
        ),
        (
            "tab",
            "Specifications",
            1,
            false,
            true,
            SemanticVerification::TargetState {
                state: SemanticState::Selected,
                present: true,
            },
        ),
    ] {
        let observation = reading_observation(
            json!([
                {"k":1,"r":if dialog {"dialog"} else {"document"},"fc":true},
                {"k":2,"p":0,"r":role,"n":name,"ak":activation,"o":9,"fc":true,
                 "s":if proof == (SemanticVerification::TargetState {state: SemanticState::Expanded, present: false}) {4} else {0},
                 "b":{"x":1,"y":1,"w":100,"h":30}}
            ]),
            "complete",
        );
        let snapshot = &observation.frames()[0];
        let node = &snapshot.nodes()[1];
        assert_eq!(
            policy
                .model_action_operations(node, &observation)
                .unwrap()
                .contains(SemanticOperationClass::Click),
            allowed,
            "{role}/{name}/{activation}/{dialog}"
        );
        let recipe = policy
            .decision_action_recipe(&DecisionOperation::Click(node.reference()), &observation)
            .unwrap();
        assert_eq!(recipe.is_some(), allowed);
        if let Some(recipe) = recipe {
            assert_eq!(recipe.verification(), proof);
            let batch = SemanticActionBatch::bind(
                SemanticActionBatchId::new(1).unwrap(),
                &observation,
                &[snapshot.frame().clone()],
                vec![recipe],
            )
            .unwrap();
            let action = batch.actions()[0].prepare(snapshot).unwrap();
            assert!(policy.assess(&action, &observation).is_ok());
        }
        for verification in [
            proof,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
        ] {
            let proposal = SemanticActionProposal::try_new(
                SemanticActionIntent::Click {
                    target: node.reference(),
                },
                SemanticEffectClass::Read,
                SemanticWaitCondition::Immediate,
                verification,
                SemanticSettleBudget::try_new(2000).unwrap(),
            )
            .unwrap();
            let batch = SemanticActionBatch::bind(
                SemanticActionBatchId::new(1).unwrap(),
                &observation,
                &[snapshot.frame().clone()],
                vec![proposal],
            );
            let batch = match batch {
                Ok(batch) => batch,
                Err(_) if !allowed => continue,
                Err(error) => panic!("allowed action failed binding: {error:?}"),
            };
            let action = batch.actions()[0].prepare(snapshot).unwrap();
            assert_eq!(
                policy.assess(&action, &observation).is_ok(),
                allowed && verification == proof
            );
        }
    }
}

#[cfg(feature = "durable-runtime")]
#[test]
fn reading_disclosure_preserves_both_reveal_and_click_admission() {
    let policy = read_interactions::ReadingInteractionPolicy;
    for completeness in ["complete", "node_limit"] {
        let observed = reading_observation(
            serde_json::json!([
                {"k":1,"r":"document","o":16,"fc":true,"b":{"x":0,"y":0,"w":1280,"h":800}},
                {"k":2,"p":0,"r":"button","n":"Specifications","ak":6,"o":25,"fc":true,
                 "b":{"x":10,"y":5000,"w":160,"h":32}}
            ]),
            completeness,
        );
        let ops = policy
            .model_action_operations(&observed.frames()[0].nodes()[1], &observed)
            .unwrap();
        assert!(ops.contains(SemanticOperationClass::Scroll));
        assert!(ops.contains(SemanticOperationClass::Click));
    }
}

#[test]
fn reading_dismissal_requires_complete_nontransactional_dialog_context() {
    use serde_json::json;
    let policy = read_interactions::ReadingInteractionPolicy;
    for (text, completeness, allowed) in [
        ("You can shop, get support and more.", "complete", true),
        ("Confirm your purchase", "complete", false),
        ("You can shop, get support and more.", "node_limit", false),
    ] {
        let observation = reading_observation(
            json!([
                {"k":1,"r":"dialog","fc":true},
                {"k":2,"p":0,"r":"paragraph","t":text,"fc":true},
                {"k":3,"p":0,"r":"button","n":"Continue","ak":1,"o":9,"fc":true,"b":{"x":1,"y":1,"w":100,"h":30}}
            ]),
            completeness,
        );
        assert_eq!(
            policy
                .model_action_operations(&observation.frames()[0].nodes()[2], &observation)
                .unwrap()
                .contains(SemanticOperationClass::Click),
            allowed
        );
    }
}

#[cfg(feature = "durable-runtime")]
#[test]
fn human_account_attestation_keeps_isolated_storage_and_single_page_scope() {
    let account = AgentAccountId::generate();
    let identity = identity();
    let mut definition = objective();
    definition.navigation = AgentNavigationDiscovery::try_new_account_page(
        ContextNavigationTarget::parse("https://example.com/verified").unwrap(),
        WorkBrowserDocumentPolicy::PublicSameDocumentQuery,
    )
    .unwrap();
    let expected = definition.navigation.clone();
    let (input, task) = assemble(
        identity,
        ContextProfileStorageClass::Durable,
        definition,
        settings(PublicReadWorkAccount::Identified {
            account,
            source: Box::new(crate::account_scope::SessionAccount { account }),
        }),
    )
    .unwrap();
    let isolated = input.with_isolated_website_data();
    let spec = isolated.retained_resource_spec().unwrap();
    assert!(spec.isolated_public);
    assert_eq!(spec.identity, identity);
    assert_eq!(task.navigation_discovery(), Some(&expected));
    assert_eq!(expected.max_hops(), 0);
    assert!(!expected.is_public_web());
    assert_eq!(
        task.attest_account(join(identity), spec.clock.now().unwrap())
            .unwrap()
            .account(),
        AgentAccountScope::Authenticated(account)
    );
}

#[cfg(feature = "durable-runtime")]
#[test]
fn a_recorded_consent_dialog_is_dismissed_by_its_refusing_control_only_by_recipe() {
    use serde_json::json;
    let policy = read_interactions::ReadingInteractionPolicy;
    let button = |key: u32, name: &str, y: u32| json!({"k":key,"p":0,"r":"button","n":name,"ak":1,"o":9,"fc":true,"b":{"x":10,"y":y,"w":300,"h":40}});
    // IKEA's Polish consent dialog as recorded on 2026-09-23: accept all,
    // accept only necessary, and a settings control.
    let consent = |title: &str, topic: &str| {
        reading_observation(
            json!([
                {"k":1,"r":"dialog","n":title,"fc":true,"b":{"x":0,"y":0,"w":800,"h":600}},
                {"k":2,"p":0,"r":"paragraph","t":topic,"fc":true},
                button(3, "Akceptuj wszystkie cookies", 400),
                button(4, "Akceptuj tylko niezbędne", 450),
                button(5, "Ustawienia cookies, otwiera okno dialogowe centrum preferencji", 500)
            ]),
            "complete",
        )
    };
    let observation = consent(
        "Ty decydujesz jakich ciasteczek użyjemy",
        "My, IKEA, korzystamy na naszych stronach z plików cookies (ciasteczek).",
    );
    let nodes = observation.frames()[0].nodes();
    assert_eq!(
        policy.consent_dismissal(&observation),
        Some(nodes[3].reference())
    );
    for node in &nodes[2..] {
        assert!(!policy
            .model_action_operations(node, &observation)
            .unwrap()
            .contains(SemanticOperationClass::Click));
    }
    let recipe = policy
        .decision_action_recipe(
            &DecisionOperation::Click(nodes[3].reference()),
            &observation,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        recipe.verification(),
        SemanticVerification::PageDialogClosed
    );
    let snapshot = &observation.frames()[0];
    let batch = SemanticActionBatch::bind(
        SemanticActionBatchId::new(1).unwrap(),
        &observation,
        &[snapshot.frame().clone()],
        vec![recipe],
    )
    .unwrap();
    let action = batch.actions()[0].prepare(snapshot).unwrap();
    assert!(policy.assess(&action, &observation).is_ok());
    assert!(policy
        .decision_action_recipe(
            &DecisionOperation::Click(nodes[4].reference()),
            &observation
        )
        .unwrap()
        .is_none());
    // The same controls in a dialog that is not about cookies stay refused.
    let other = reading_observation(
        json!([
            {"k":1,"r":"dialog","n":"Twoje zamówienie","fc":true,"b":{"x":0,"y":0,"w":800,"h":600}},
            {"k":2,"p":0,"r":"paragraph","t":"Potwierdź zamówienie.","fc":true},
            button(3, "Akceptuj", 400)
        ]),
        "complete",
    );
    assert_eq!(policy.consent_dismissal(&other), None);
    // LEGO's entry gate names cookies and offers Continue, which the model
    // may also choose; the recipe picks it too.
    let gate = reading_observation(
        json!([
            {"k":1,"r":"dialog","fc":true,"b":{"x":0,"y":0,"w":800,"h":600}},
            {"k":2,"p":0,"r":"paragraph","t":"We want to let you know that we are placing cookies on your device.","fc":true},
            button(3, "Continue", 300)
        ]),
        "complete",
    );
    let continue_button = gate.frames()[0].nodes()[2].reference();
    assert_eq!(policy.consent_dismissal(&gate), Some(continue_button));
    assert!(policy
        .model_action_operations(&gate.frames()[0].nodes()[2], &gate)
        .unwrap()
        .contains(SemanticOperationClass::Click));
}

#[cfg(feature = "durable-runtime")]
#[test]
fn a_recorded_specifications_disclosure_in_view_is_toggled_by_recipe() {
    use serde_json::json;
    let policy = read_interactions::ReadingInteractionPolicy;
    // LEGO's Tower Bridge page as recorded on 2026-09-24: the Specifications
    // tab in the section navigation, then the collapsed Specifications
    // disclosure beside Customer Reviews, below the viewport and in view.
    let page = |ops: u32, y: u32| {
        reading_observation(
            json!([
                {"k":1,"r":"document","fc":true,"o":16,"b":{"x":0,"y":0,"w":800,"h":600}},
                {"k":2,"p":0,"r":"button","n":"Specifications","ak":1,"fc":true,"b":{"x":10,"y":300,"w":120,"h":40}},
                {"k":3,"p":0,"r":"button","n":"Specifications","ak":6,"o":ops,"fc":true,"b":{"x":10,"y":y,"w":300,"h":40}},
                {"k":4,"p":0,"r":"button","n":"Customer Reviews Average rating 4.2 out of 5 stars (5)","ak":6,"o":ops,"fc":true,"b":{"x":10,"y":y + 60,"w":300,"h":40}}
            ]),
            "complete",
        )
    };
    assert_eq!(policy.detail_disclosure(&page(16, 2400)), None);
    let shown = page(9, 300);
    let disclosure = shown.frames()[0].nodes()[2].reference();
    assert_eq!(
        policy.detail_disclosure(&shown),
        Some(DecisionOperation::Click(disclosure))
    );
    let recipe = policy
        .decision_action_recipe(&DecisionOperation::Click(disclosure), &shown)
        .unwrap()
        .unwrap();
    assert_eq!(
        recipe.verification(),
        SemanticVerification::TargetState {
            state: SemanticState::Expanded,
            present: true,
        }
    );
    let snapshot = &shown.frames()[0];
    let batch = SemanticActionBatch::bind(
        SemanticActionBatchId::new(1).unwrap(),
        &shown,
        &[snapshot.frame().clone()],
        vec![recipe],
    )
    .unwrap();
    let action = batch.actions()[0].prepare(snapshot).unwrap();
    assert!(policy.assess(&action, &shown).is_ok());
}

#[cfg(feature = "durable-runtime")]
#[test]
fn a_page_in_the_persons_session_is_read_never_dismissed_or_consented() {
    use serde_json::json;
    let policy = read_interactions::SessionReadingPolicy;
    let observation = reading_observation(
        json!([
            {"k":1,"r":"document","fc":true,"o":16,"b":{"x":0,"y":0,"w":800,"h":600}},
            {"k":2,"p":0,"r":"button","n":"Specifications","ak":6,"o":25,"fc":true,
             "b":{"x":1,"y":1,"w":100,"h":30}},
            {"k":3,"p":0,"r":"tab","n":"Details","ak":1,"o":9,"fc":true,
             "b":{"x":1,"y":40,"w":100,"h":30}}
        ]),
        "complete",
    );
    let snapshot = &observation.frames()[0];
    let [document, disclosure, tab] = [0, 1, 2].map(|index| &snapshot.nodes()[index]);
    for (node, operation) in [
        (document, DecisionOperation::Scroll(document.reference())),
        (disclosure, DecisionOperation::Click(disclosure.reference())),
        (tab, DecisionOperation::Click(tab.reference())),
    ] {
        let _ = node;
        let recipe = policy
            .decision_action_recipe(&operation, &observation)
            .unwrap()
            .unwrap();
        let batch = SemanticActionBatch::bind(
            SemanticActionBatchId::new(1).unwrap(),
            &observation,
            &[snapshot.frame().clone()],
            vec![recipe],
        )
        .unwrap();
        assert!(policy
            .assess(&batch.actions()[0].prepare(snapshot).unwrap(), &observation)
            .is_ok());
    }
    assert!(policy
        .model_action_operations(tab, &observation)
        .unwrap()
        .contains(SemanticOperationClass::Click));
    assert!(policy.consent_dismissal(&observation).is_none());
}
