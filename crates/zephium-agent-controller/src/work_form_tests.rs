//! Synthetic semantics only; exercise real decode/bind/prepare contracts.
use super::*;

#[test]
fn static_task_scopes_never_renew_their_initial_account_sample() {
    let frame = frame();
    let form = AgentWorkFormTask::try_new_local_preparation(
        frame.context().identity(),
        frame.origin().clone(),
        AgentAccountScope::Anonymous,
        vec![AgentWorkFormPhase::try_new(vec![
            AgentWorkFormGoal::fill(None, "fixture".into()).unwrap()
        ])
        .unwrap()],
    )
    .unwrap();
    let extraction = crate::AgentWorkExtractionTask::try_new(
        vec![SemanticExtractionFieldSchema::try_text("label".into(), true, 32).unwrap()],
        AgentAccountScope::Anonymous,
    )
    .unwrap();
    for task in [
        &form as &dyn AgentWorkTask,
        &extraction as &dyn AgentWorkTask,
    ] {
        let first = task
            .attest_account(frame.context(), AgentPolicyInstant::from_millis(1))
            .unwrap();
        let old = task
            .attest_account(frame.context(), AgentPolicyInstant::from_millis(60_001))
            .unwrap();
        assert_eq!(first, old, "a static scope is not live account evidence");
    }
}

fn frame() -> SemanticFrameJoin {
    let identity = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        1_u128.into(),
        ContextKind::Owned,
    );
    let mut registry = ContextRegistry::new();
    registry
        .reserve(
            identity,
            ContextCapabilities::try_new(
                ContextKind::Owned,
                &[ContextCapability::Observe, ContextCapability::Act],
            )
            .unwrap(),
        )
        .unwrap();
    let operation = registry
        .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
        .unwrap();
    registry
        .settle_construction(identity.id(), operation, ContextSettlement::Applied)
        .unwrap();
    let context = registry.join(identity.id()).unwrap();
    SemanticFrameJoin::try_new(
        context,
        FrameId::MAIN,
        context.frame_generation(),
        SemanticOrigin::parse("https://fixture.example.test/").unwrap(),
        SemanticFrameTrust::SameOrigin,
    )
    .unwrap()
}
fn observe(
    frame: &SemanticFrameJoin,
    generation: u64,
    value: &str,
    selected: bool,
    extra: &str,
) -> SemanticObservation {
    let wire = format!(
        r#"{{"v":1,"i":{generation},"g":{generation},"c":"complete","n":[{{"k":1,"r":"document","o":16}},{{"k":2,"p":0,"r":"textbox","n":"Query","s":64,"o":3,"v":{{"k":"text","value":"{value}"}},"b":{{"x":10,"y":20,"w":120,"h":30}}}},{{"k":3,"p":0,"r":"combobox","n":"Language","o":4,"b":{{"x":10,"y":60,"w":120,"h":30}}}},{{"k":4,"p":2,"r":"option","n":"German","s":{},"o":0}},{{"k":5,"p":2,"r":"option","n":"English","s":{},"o":0}}{extra}]}}"#,
        if selected { 2 } else { 0 },
        0
    );
    decode(frame, generation, &wire)
}
fn decode(frame: &SemanticFrameJoin, generation: u64, wire: &str) -> SemanticObservation {
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(generation).unwrap(),
            frame.clone(),
            SemanticSnapshotGeneration::new(generation).unwrap(),
        ),
        wire.as_bytes(),
    )
    .unwrap();
    SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(generation).unwrap(),
            frame.context(),
            SemanticObservationBudget::INITIAL_FILTERED,
        ),
        snapshot,
    )
    .unwrap()
    .finish()
    .unwrap()
}
fn phase(value: &str, select: bool) -> AgentWorkFormPhase {
    let mut goals = vec![AgentWorkFormGoal::fill(Some("Query".into()), value.into()).unwrap()];
    if select {
        goals.push(AgentWorkFormGoal::select(Some("Language".into()), "German".into()).unwrap());
    }
    AgentWorkFormPhase::try_new(goals).unwrap()
}
fn task(frame: &SemanticFrameJoin, phases: Vec<AgentWorkFormPhase>) -> AgentWorkFormTask {
    AgentWorkFormTask::try_new_local_preparation(
        frame.context().identity(),
        frame.origin().clone(),
        AgentAccountScope::Anonymous,
        phases,
    )
    .unwrap()
}
fn external_task(frame: &SemanticFrameJoin, phases: Vec<AgentWorkFormPhase>) -> AgentWorkFormTask {
    AgentWorkFormTask::try_new_external_update(
        frame.context().identity(),
        frame.origin().clone(),
        AgentAccountScope::Anonymous,
        phases,
    )
    .unwrap()
}
fn action(
    observation: &SemanticObservation,
    intent: SemanticActionIntent,
    effect: SemanticEffectClass,
) -> SemanticPreparedAction {
    let verification = match intent {
        SemanticActionIntent::Fill { .. } => SemanticVerification::TargetValueMatchesInput,
        SemanticActionIntent::Select { .. } => SemanticVerification::TargetSelectionMatchesOption,
        _ => SemanticVerification::TargetState {
            state: SemanticState::Invalid,
            present: true,
        },
    };
    let proposal = SemanticActionProposal::try_new(
        intent,
        effect,
        SemanticWaitCondition::Immediate,
        verification,
        SemanticSettleBudget::try_new(2000).unwrap(),
    )
    .unwrap();
    let snapshot = &observation.frames()[0];
    let batch = SemanticActionBatch::bind(
        SemanticActionBatchId::new(1).unwrap(),
        observation,
        &[snapshot.frame().clone()],
        vec![proposal],
    )
    .unwrap();
    batch.actions()[0].prepare(snapshot).unwrap()
}
fn fill(observation: &SemanticObservation, value: &str) -> SemanticPreparedAction {
    action(
        observation,
        SemanticActionIntent::Fill {
            target: SemanticReferenceId::new(2).unwrap(),
            value: SemanticActionText::try_new(value.into()).unwrap(),
        },
        SemanticEffectClass::LocalWrite,
    )
}
fn external_fill(observation: &SemanticObservation, value: &str) -> SemanticPreparedAction {
    action(
        observation,
        SemanticActionIntent::Fill {
            target: SemanticReferenceId::new(2).unwrap(),
            value: SemanticActionText::try_new(value.into()).unwrap(),
        },
        SemanticEffectClass::ExternalWrite,
    )
}
fn select(observation: &SemanticObservation, option: u16) -> SemanticPreparedAction {
    action(
        observation,
        SemanticActionIntent::Select {
            target: SemanticReferenceId::new(3).unwrap(),
            option: SemanticReferenceId::new(option).unwrap(),
        },
        SemanticEffectClass::LocalWrite,
    )
}

#[test]
fn form_goal_accepts_editable_combobox_without_granting_role_only_fill() {
    let frame = frame();
    for (operations, accepts) in [(3, true), (9, false), (13, false)] {
        let mut task = task(&frame, vec![phase("query", false)]);
        let initial = decode(
            &frame,
            1,
            &format!(
                r#"{{"v":1,"i":1,"g":1,"c":"complete","n":[{{"k":1,"r":"document","o":16}},{{"k":2,"p":0,"r":"combobox","n":"Query","o":{operations},"b":{{"x":10,"y":20,"w":120,"h":30}}}}]}}"#
            ),
        );
        assert_eq!(task.evaluate(&initial).is_ok(), accepts);
        if accepts {
            assert!(task.assess(&fill(&initial, "query")).is_ok());
        }
    }
}

#[test]
fn phased_form_is_variable_order_and_exact_not_action_count() {
    for language_first in [true, false] {
        let frame = frame();
        let mut task = task(&frame, vec![phase("first", true), phase("final", true)]);
        let initial = observe(&frame, 1, "", false, "");
        assert!(task.assess(&fill(&initial, "first")).is_err());
        assert_eq!(
            task.evaluate(&initial).unwrap(),
            AgentWorkTaskProgress::Continue
        );
        assert!(task.assess(&fill(&initial, "first")).is_ok());
        assert!(task.assess(&select(&initial, 4)).is_ok());
        assert!(task.assess(&fill(&initial, "final")).is_err());
        assert!(task.assess(&select(&initial, 5)).is_err());
        let mid = observe(
            &frame,
            2,
            if language_first { "" } else { "first" },
            language_first,
            "",
        );
        assert_eq!(
            task.evaluate(&mid).unwrap(),
            AgentWorkTaskProgress::Continue
        );
        assert!(
            task.assess(&fill(&initial, "first")).is_err(),
            "retired reference cannot authorize"
        );
        assert_eq!(
            task.assess(&fill(&mid, if language_first { "first" } else { "final" }))
                .is_ok(),
            language_first
        );
        assert_eq!(
            task.assess(&select(&mid, if language_first { 5 } else { 4 }))
                .is_ok(),
            !language_first
        );
        let prepared = observe(&frame, 3, "first", true, "");
        assert_eq!(
            task.evaluate(&prepared).unwrap(),
            AgentWorkTaskProgress::Continue
        );
        assert!(task.assess(&fill(&prepared, "unapproved")).is_err());
        assert!(task.assess(&fill(&prepared, "final")).is_ok());
        assert!(task.assess(&select(&prepared, 5)).is_err());
        let final_state = observe(&frame, 4, "final", true, "");
        assert_eq!(
            task.evaluate(&final_state).unwrap(),
            AgentWorkTaskProgress::Complete
        );
        assert!(task.assess(&fill(&final_state, "unapproved")).is_err());
        assert!(
            task.evaluate(&final_state).is_err(),
            "completion cannot be reused"
        );
    }
    let frame = frame();
    let mut task = task(&frame, vec![phase("ready", true)]);
    assert_eq!(
        task.evaluate(&observe(&frame, 1, "ready", true, ""))
            .unwrap(),
        AgentWorkTaskProgress::Complete,
        "already satisfied requires zero actions"
    );
}

#[test]
fn exact_external_transition_updates_then_restores_without_widening_authority() {
    let frame = frame();
    let transition = |from: &str, to: &str| {
        AgentWorkFormPhase::try_new(vec![AgentWorkFormGoal::fill_transition(
            None,
            from.into(),
            to.into(),
        )
        .unwrap()])
        .unwrap()
    };
    let mut task = external_task(
        &frame,
        vec![
            transition("original", "qualification marker"),
            transition("qualification marker", "original"),
        ],
    );

    let initial = observe(&frame, 1, "original", false, "");
    assert_eq!(
        task.evaluate(&initial).unwrap(),
        AgentWorkTaskProgress::Continue
    );
    let update = external_fill(&initial, "qualification marker");
    let assessment = task.assess(&update).unwrap();
    assert_eq!(
        assessment.actual_effect(),
        SemanticEffectClass::ExternalWrite
    );
    assert!(task
        .assess(&fill(&initial, "qualification marker"))
        .is_err());
    assert!(task.assess(&external_fill(&initial, "unapproved")).is_err());

    let changed = observe(&frame, 2, "qualification marker", false, "");
    assert_eq!(
        task.evaluate(&changed).unwrap(),
        AgentWorkTaskProgress::Continue
    );
    assert!(task
        .assess(&external_fill(&initial, "qualification marker"))
        .is_err());
    assert!(task.assess(&external_fill(&changed, "original")).is_ok());

    let restored = observe(&frame, 3, "original", false, "");
    assert_eq!(
        task.evaluate(&restored).unwrap(),
        AgentWorkTaskProgress::Complete
    );
}

#[test]
fn external_transition_precondition_and_uniqueness_fail_closed() {
    let frame = frame();
    let transition = || {
        AgentWorkFormPhase::try_new(vec![AgentWorkFormGoal::fill_transition(
            None,
            "original".into(),
            "qualification marker".into(),
        )
        .unwrap()])
        .unwrap()
    };

    let mut wrong_initial = external_task(&frame, vec![transition()]);
    assert!(wrong_initial
        .evaluate(&observe(&frame, 1, "unexpected", false, ""))
        .is_err());

    let mut ambiguous = external_task(&frame, vec![transition()]);
    let extra = r#",{"k":6,"p":0,"r":"textbox","n":"Other","s":64,"o":3,"v":{"k":"text","value":"qualification marker"}}"#;
    assert!(ambiguous
        .evaluate(&observe(&frame, 1, "original", false, extra))
        .is_err());

    let mut already_final = external_task(&frame, vec![transition()]);
    assert_eq!(
        already_final
            .evaluate(&observe(&frame, 1, "qualification marker", false, ""))
            .unwrap(),
        AgentWorkTaskProgress::Complete
    );
}

#[test]
fn goals_are_bounded_and_overlapping_authority_is_rejected() {
    for (name, value) in [
        (Some("".into()), "ok".into()),
        (Some("x".repeat(513)), "ok".into()),
        (None, "x".repeat(1025)),
        (None, "bad\0value".into()),
    ] {
        assert!(AgentWorkFormGoal::fill(name, value).is_err());
    }
    assert!(AgentWorkFormGoal::select(None, "".into()).is_err());
    assert!(AgentWorkFormGoal::select(None, "x".repeat(513)).is_err());
    assert!(AgentWorkFormGoal::fill_transition(None, "same".into(), "same".into()).is_err());
    assert!(AgentWorkFormPhase::try_new(vec![]).is_err());
    assert!(AgentWorkFormPhase::try_new(vec![
        AgentWorkFormGoal::fill(None, "a".into()).unwrap(),
        AgentWorkFormGoal::fill(Some("Query".into()), "b".into()).unwrap()
    ])
    .is_err());
    let frame = frame();
    assert!(AgentWorkFormTask::try_new_local_preparation(
        frame.context().identity(),
        frame.origin().clone(),
        AgentAccountScope::Anonymous,
        vec![]
    )
    .is_err());
    assert!(AgentWorkFormTask::try_new_local_preparation(
        frame.context().identity(),
        frame.origin().clone(),
        AgentAccountScope::Anonymous,
        vec![phase("value", false); 9]
    )
    .is_err());
}

#[test]
fn ambiguity_missing_option_and_value_truncation_clear_all_bindings() {
    for extra in [
        r#",{"k":6,"p":0,"r":"textbox","n":"Query","o":2}"#,
        r#",{"k":6,"p":2,"r":"option","n":"German","o":0}"#,
        r#",{"k":6,"p":2,"r":"option","n":"Other","s":2,"o":0}"#,
    ] {
        let frame = frame();
        let mut task = task(&frame, vec![phase("wanted", true)]);
        let initial = observe(&frame, 1, "", false, "");
        task.evaluate(&initial).unwrap();
        assert!(task.evaluate(&observe(&frame, 2, "", true, extra)).is_err());
        assert!(task.bindings.is_empty());
        assert!(task.assess(&fill(&initial, "wanted")).is_err());
        assert!(
            task.evaluate(&observe(&frame, 3, "", false, "")).is_err(),
            "no retry after refused evidence"
        );
    }
    let frame = frame();
    let initial = observe(&frame, 1, "", false, "");
    let mut missing = task(
        &frame,
        vec![AgentWorkFormPhase::try_new(vec![
            AgentWorkFormGoal::select(None, "Missing".into()).unwrap()
        ])
        .unwrap()],
    );
    assert!(missing.evaluate(&initial).is_err());
    let mut missing = task(
        &frame,
        vec![AgentWorkFormPhase::try_new(vec![AgentWorkFormGoal::fill(
            Some("Missing".into()),
            "wanted".into(),
        )
        .unwrap()])
        .unwrap()],
    );
    assert!(missing.evaluate(&initial).is_err());
    let mut value = task(&frame, vec![phase(&"x".repeat(1024), false)]);
    assert_eq!(
        value
            .evaluate(&observe(&frame, 1, &"x".repeat(1025), false, ""))
            .unwrap(),
        AgentWorkTaskProgress::Continue,
        "prefix is not complete value"
    );
}

#[test]
fn incomplete_secret_disabled_readonly_and_partial_scope_are_not_task_evidence() {
    let frame = frame();
    let valid = observe(&frame, 1, "", false, "");
    for (completeness, field) in [
        ("node_limit", r#""o":2"#),
        ("complete", r#""o":2,"q":"secret""#),
        ("complete", r#""o":0,"s":8"#),
        ("complete", r#""o":0"#),
    ] {
        let mut task = task(&frame, vec![phase("wanted", false)]);
        task.evaluate(&valid).unwrap();
        let wire = format!(
            r#"{{"v":1,"i":2,"g":2,"c":"{completeness}","n":[{{"k":1,"r":"document","o":16}},{{"k":2,"p":0,"r":"textbox","n":"Query",{field}}}]}}"#
        );
        assert!(task.evaluate(&decode(&frame, 2, &wire)).is_err());
        assert!(task.assess(&fill(&valid, "wanted")).is_err());
        assert!(task.bindings.is_empty());
    }
    let request = valid
        .begin_expansion(
            SemanticObservationId::new(2).unwrap(),
            SemanticReferenceId::new(2).unwrap(),
            &frame,
            SemanticExpansionKind::Subtree,
            SemanticObservationBudget::INITIAL_FILTERED,
        )
        .unwrap();
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(2).unwrap(),
            frame.clone(),
            SemanticSnapshotGeneration::new(2).unwrap(),
        ),
        br#"{"v":1,"i":2,"g":2,"c":"complete","n":[{"k":2,"r":"textbox","n":"Query","o":2}]}"#,
    )
    .unwrap();
    let partial = SemanticObservationAssembler::new(request, snapshot)
        .unwrap()
        .finish()
        .unwrap();
    let mut task = task(&frame, vec![phase("", false)]);
    assert!(
        task.evaluate(&partial).is_err(),
        "complete subtree cannot prove global target uniqueness"
    );
}

#[test]
fn unique_unnamed_fields_clear_exactly_but_foreign_named_targets_cannot_write() {
    let frame = frame();
    let mut clearing = task(&frame, vec![phase("", false)]);
    assert_eq!(
        clearing
            .evaluate(&observe(&frame, 1, "", false, ""))
            .unwrap(),
        AgentWorkTaskProgress::Complete
    );
    let mut unnamed = task(
        &frame,
        vec![AgentWorkFormPhase::try_new(vec![
            AgentWorkFormGoal::fill(None, "wanted".into()).unwrap()
        ])
        .unwrap()],
    );
    let observation = observe(&frame, 1, "", false, "");
    unnamed.evaluate(&observation).unwrap();
    assert!(unnamed.assess(&fill(&observation, "wanted")).is_ok());
    let other = observe(
        &frame,
        2,
        "",
        false,
        r#",{"k":6,"p":0,"r":"textbox","n":"Other","o":2,"b":{"x":10,"y":100,"w":120,"h":30}}"#,
    );
    assert!(unnamed.evaluate(&other).is_err());
    let mut named = task(&frame, vec![phase("wanted", false)]);
    named.evaluate(&other).unwrap();
    let foreign = action(
        &other,
        SemanticActionIntent::Fill {
            target: SemanticReferenceId::new(6).unwrap(),
            value: SemanticActionText::try_new("wanted".into()).unwrap(),
        },
        SemanticEffectClass::LocalWrite,
    );
    assert!(named.assess(&foreign).is_err());
}

#[test]
fn cross_context_origin_stale_generation_and_wrong_effect_never_assess() {
    let frame = frame();
    let initial = observe(&frame, 1, "", false, "");
    for changed in [
        initial.clone(),
        observe(&frame, 1, "changed", false, ""),
        observe(&super::tests::frame(), 2, "", false, ""),
    ] {
        let mut task = task(&frame, vec![phase("wanted", false)]);
        task.evaluate(&initial).unwrap();
        assert!(task.evaluate(&changed).is_err());
        assert!(task.assess(&fill(&initial, "wanted")).is_err());
    }
    let mut task = task(&frame, vec![phase("wanted", false)]);
    assert!(task
        .attest_account(
            super::tests::frame().context(),
            AgentPolicyInstant::from_millis(1)
        )
        .is_err());
    task.evaluate(&initial).unwrap();
    let wrong_effect = action(
        &initial,
        SemanticActionIntent::Fill {
            target: SemanticReferenceId::new(2).unwrap(),
            value: SemanticActionText::try_new("wanted".into()).unwrap(),
        },
        SemanticEffectClass::ExternalWrite,
    );
    assert!(task.assess(&wrong_effect).is_err());
    let click = action(
        &initial,
        SemanticActionIntent::Click {
            target: SemanticReferenceId::new(2).unwrap(),
        },
        SemanticEffectClass::LocalWrite,
    );
    assert!(task.assess(&click).is_err());
    let foreign = SemanticFrameJoin::try_new(
        frame.context(),
        FrameId::MAIN,
        frame.frame_generation(),
        SemanticOrigin::parse("https://other.example.test").unwrap(),
        SemanticFrameTrust::SameOrigin,
    )
    .unwrap();
    assert!(task
        .evaluate(&observe(&foreign, 2, "wanted", false, ""))
        .is_err());
}
