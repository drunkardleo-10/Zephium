//! Tests the selected live wrapper, not a bare discovery task or a site oracle.
use super::*;

fn context() -> ContextJoin {
    let identity = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        zephium_core::ids::ProfileId::generate(),
        ContextKind::Owned,
    );
    let mut registry = ContextRegistry::new();
    registry
        .reserve(
            identity,
            ContextCapabilities::try_new(
                ContextKind::Owned,
                &[ContextCapability::Observe, ContextCapability::Navigate],
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
    registry.join(identity.id()).unwrap()
}

fn observation(
    context: ContextJoin,
    origin: &str,
    id: u64,
    status: &str,
    text: &str,
) -> SemanticObservation {
    let frame = SemanticFrameJoin::try_new(
        context,
        FrameId::MAIN,
        context.frame_generation(),
        SemanticOrigin::parse(origin).unwrap(),
        SemanticFrameTrust::SameOrigin,
    )
    .unwrap();
    // Deliberately generic public evidence. Neither a product nor the answer
    // from the live objective appears in this structural fixture.
    let wire = format!(
        r#"{{"v":1,"i":{id},"g":{id},"c":"{status}","n":[
        {{"k":1,"r":"document","o":16}},
        {{"k":2,"p":0,"r":"heading","l":1,"n":"Public information"}},
        {{"k":3,"p":0,"r":"paragraph","t":"{text}"}},
        {{"k":4,"p":0,"r":"button","n":"Add to cart","o":1}}
    ]}}"#
    );
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(id).unwrap(),
            frame,
            SemanticSnapshotGeneration::new(id).unwrap(),
        ),
        wire.as_bytes(),
    )
    .unwrap();
    SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(id).unwrap(),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        ),
        snapshot,
    )
    .unwrap()
    .finish()
    .unwrap()
}

fn mapped<'a>(
    schema: &SemanticExtractionSchema,
    observation: &'a SemanticObservation,
) -> SemanticExtractionResult<'a> {
    let read = read_selected_semantic_observation(
        observation,
        SemanticReadAuthority::Initial,
        SemanticCaptureInstant::from_millis(30),
        SemanticReadSensitivityLimit::PublicOnly,
        SemanticReadBudget::STANDARD,
        schema.source_roles(),
    )
    .unwrap();
    let delivery = encode_semantic_read(
        &read,
        SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
    )
    .unwrap()
    .admit_conservative_utf8(
        &SemanticTokenizerRevision::try_new("objective-test-v1".into()).unwrap(),
    )
    .unwrap()
    .settle_delivery(SemanticModelDeliverySettlement::Committed)
    .unwrap();
    let output = br#"{"v":1,"schema":1,"fields":[
        {"name":"summary","value":{"k":"text","value":"Information is insufficient for a recommendation.","sources":["@r2"]}},
        {"name":"important_claims","value":{"k":"text_list","items":[{"value":"The current page does not establish a fit.","sources":["@r2"]}],"sources":["@r2"]}},
        {"name":"caveats","value":{"k":"text_list","items":[{"value":"A human should inspect missing information.","sources":["@r2"]}],"sources":["@r2"]}}
    ]}"#;
    extract_semantic_read(
        schema,
        &read,
        &delivery,
        SemanticReadSensitivityLimit::PublicOnly,
        output,
    )
    .unwrap()
}

#[test]
fn selected_definition_binds_same_scope_in_task_and_read_only_manifest() {
    let context = context();
    let task = (DEFINITION.task)(context.identity()).unwrap();
    let scope = task.navigation_discovery().unwrap();
    assert_eq!(scope.departure().as_url().as_str(), INITIAL);
    assert_eq!(scope.origin(), &SemanticOrigin::parse(ORIGIN).unwrap());
    assert_eq!(scope.path_prefix(), PATH_PREFIX);
    assert_eq!(scope.max_hops(), 2);
    let authority = (DEFINITION.authority)(
        AgentPlanNodeAuthority::try_new(
            vec![context.identity().profile()],
            vec![AgentAccountScope::Anonymous],
            vec![SemanticOrigin::parse(ORIGIN).unwrap()],
            SemanticSensitivity::Public,
            AgentEffectScope::try_new(&[SemanticEffectClass::Read]).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(authority.navigation_discovery(), Some(scope));
    assert_eq!(authority.effects().len(), 1);
    assert!(authority.effects().contains(SemanticEffectClass::Read));
    assert!(!task.allows_actions_before_extraction());
    assert!(task.navigation_route().is_none());
    assert!(task.navigation_target().is_none());
    let within = format!("{ORIGIN}{PATH_PREFIX}unfamiliar-fixture-page");
    assert!(scope.admits(&ContextNavigationTarget::parse(&within).unwrap()));
    for target in [
        INITIAL.to_owned(),
        format!("{within}?variant=1"),
        format!("{within}#details"),
        format!("{ORIGIN}/outside-scope"),
        format!(
            "http://{}{PATH_PREFIX}item",
            scope.origin().as_url().host_str().unwrap()
        ),
        format!("https://outside.example.test{PATH_PREFIX}item"),
        format!("{ORIGIN}{PATH_PREFIX}%2fitem"),
    ] {
        assert!(
            !scope.admits(&ContextNavigationTarget::parse(&target).unwrap()),
            "{target}"
        );
    }
    // Scope membership alone never makes these invented test URLs observed
    // links; native dispatch still needs the original controller policy proof.
}

#[test]
fn selected_wrapper_accepts_incomplete_changed_content_without_answer_predicates() {
    let context = context();
    let mut task = (DEFINITION.task)(context.identity()).unwrap();
    let account = task
        .attest_account(context, AgentPolicyInstant::from_millis(10))
        .unwrap();
    for (id, status, text) in [
        (1, "complete", "The catalog is loading."),
        (
            2,
            "scope_boundary",
            "No suitable item is established by this evidence.",
        ),
        (
            3,
            "node_limit",
            "Some public information remains outside this bounded observation.",
        ),
    ] {
        assert_eq!(
            task.evaluate(&observation(context, ORIGIN, id, status, text))
                .unwrap(),
            AgentWorkTaskProgress::Continue
        );
        let current_account = task
            .attest_account(context, AgentPolicyInstant::from_millis(20 + id))
            .unwrap();
        assert_eq!(
            current_account, account,
            "inspection must not renew account age"
        );
    }
    assert!(task
        .evaluate(&observation(
            context,
            "https://outside.example.test",
            4,
            "complete",
            "Different origin."
        ))
        .is_err());
    assert!(task
        .evaluate(&observation(
            self::context(),
            ORIGIN,
            5,
            "complete",
            "Different resource."
        ))
        .is_err());
}

#[test]
fn selected_result_verifier_checks_current_source_shape_not_a_preselected_recommendation() {
    let context = context();
    let mut task = (DEFINITION.task)(context.identity()).unwrap();
    let schema = task.extraction_schema().unwrap().clone();
    let current = observation(
        context,
        ORIGIN,
        1,
        "scope_boundary",
        "The necessary details are unavailable.",
    );
    task.attest_account(context, AgentPolicyInstant::from_millis(10))
        .unwrap();
    task.evaluate(&current).unwrap();
    let result = mapped(&schema, &current);
    assert_eq!(
        task.accept_extraction(&result).unwrap(),
        AgentWorkTaskProgress::Complete
    );
    assert!(
        task.accept_extraction(&result).is_err(),
        "completion is once-only"
    );
    assert!((DEFINITION.verify_owned)(&result.into_owned().unwrap()));
    let foreign = observation(
        context,
        "https://outside.example.test",
        2,
        "complete",
        "The necessary details are unavailable.",
    );
    assert!(!(DEFINITION.verify_owned)(
        &mapped(&schema, &foreign).into_owned().unwrap()
    ));
    assert!(
        !ApplicationObserver::default().report().accepted,
        "shape-valid current sources do not prove durable, multi-page or useful execution"
    );
}

#[cfg(feature = "retained-commerce-qualification")]
#[test]
fn commerce_selection_is_exact_and_cannot_silently_be_the_docs_witness() {
    assert_eq!(INITIAL, "https://www.lego.com/en-us/themes/architecture");
    assert_eq!(ORIGIN, "https://www.lego.com");
    assert_eq!(PATH_PREFIX, "/en-us/");
    assert_eq!(TASK_NAME, "retained-commerce-gift-v1");
    assert!(OBJECTIVE.contains(INITIAL));
    assert!(!OBJECTIVE.contains("svelte"));
    assert!(
        !OBJECTIVE.contains("/product/"),
        "do not preselect a product destination"
    );
    assert!(configuration_diagnostic().contains("rendering=observation_owned"));
    assert!(configuration_diagnostic().contains(TASK_NAME));
}
