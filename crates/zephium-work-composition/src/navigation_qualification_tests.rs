//! Shared task regressions execute against the same authority used by both hosts.
use super::*;

fn contexts() -> (ContextRegistry, ContextJoin, ContextJoin) {
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
                &[ContextCapability::Observe, ContextCapability::Navigate],
            )
            .unwrap(),
        )
        .unwrap();
    let op = registry
        .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
        .unwrap();
    registry
        .settle_construction(identity.id(), op, ContextSettlement::Applied)
        .unwrap();
    let initial = registry
        .begin_navigation(identity.id(), ContextOperationId::new(2).unwrap())
        .unwrap();
    registry
        .settle_navigation(identity.id(), initial, ContextSettlement::Applied)
        .unwrap();
    let departure = registry.join(identity.id()).unwrap();
    let op = registry
        .begin_navigation(identity.id(), ContextOperationId::new(3).unwrap())
        .unwrap();
    registry
        .settle_navigation(identity.id(), op, ContextSettlement::Applied)
        .unwrap();
    (registry, departure, op.context())
}

fn observation(
    context: ContextJoin,
    id: u64,
    origin: &str,
    role: &str,
    heading: &str,
    duplicate: bool,
) -> SemanticObservation {
    let frame = SemanticFrameJoin::try_new(
        context,
        FrameId::MAIN,
        context.frame_generation(),
        SemanticOrigin::parse(origin).unwrap(),
        SemanticFrameTrust::SameOrigin,
    )
    .unwrap();
    let level = if role == "heading" { r#", "l":1"# } else { "" };
    let duplicate = if duplicate {
        format!(r#",{{"k":3,"p":0,"r":"{role}"{level},"n":"{heading}"}}"#)
    } else {
        String::new()
    };
    let wire = format!(
        r#"{{"v":1,"i":1,"g":1,"c":"complete","n":[{{"k":1,"r":"document","o":16}},{{"k":2,"p":0,"r":"{role}"{level},"n":"{heading}"}}{duplicate}]}}"#
    );
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(1).unwrap(),
            frame,
            SemanticSnapshotGeneration::INITIAL,
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

fn ready(source: ContextJoin, destination: ContextJoin) -> ReactNavigationTask {
    let mut task = ReactNavigationTask::new(source.identity()).unwrap();
    task.attest_account(source, AgentPolicyInstant::from_millis(10))
        .unwrap();
    assert_eq!(
        task.evaluate(&observation(source, 1, ORIGIN, "heading", DEPARTURE, false))
            .unwrap(),
        AgentWorkTaskProgress::ReadyForNavigation
    );
    assert_eq!(
        task.evaluate(&observation(
            destination,
            2,
            ORIGIN,
            "heading",
            ARRIVAL,
            false
        ))
        .unwrap(),
        AgentWorkTaskProgress::ReadyForExtraction
    );
    task.attest_account(destination, AgentPolicyInstant::from_millis(20))
        .unwrap();
    task
}

fn mapped<'a>(
    schema: &SemanticExtractionSchema,
    observation: &'a SemanticObservation,
    value: &str,
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
    let payload = encode_semantic_read(
        &read,
        SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
    )
    .unwrap()
    .admit_conservative_utf8(
        &SemanticTokenizerRevision::try_new("navigation-test-v1".into()).unwrap(),
    )
    .unwrap();
    let delivery = payload
        .settle_delivery(SemanticModelDeliverySettlement::Committed)
        .unwrap();
    let output = format!(
        r#"{{"v":1,"schema":1,"fields":[{{"name":"destination_heading","value":{{"k":"text","value":"{value}","sources":["@r1"]}}}}]}}"#
    );
    extract_semantic_read(
        schema,
        &read,
        &delivery,
        SemanticReadSensitivityLimit::PublicOnly,
        output.as_bytes(),
    )
    .unwrap()
}

#[test]
fn closed_navigation_profile_has_no_parameter_or_extra_capability() {
    let (_, source, _) = contexts();
    let task = task(source.identity()).unwrap();
    assert_eq!(
        task.navigation_target().unwrap().as_url().as_str(),
        DESTINATION
    );
    assert!(task
        .navigation_target()
        .unwrap()
        .as_url()
        .fragment()
        .is_none());
    assert_eq!(
        task.extraction_schema().unwrap().source_roles(),
        SemanticReadRoleSelection::try_new(&[SemanticRole::Heading]).unwrap()
    );
    assert!(!task.allows_actions_before_extraction());
    assert!(!task.allows_baseline_read());
    assert!(!task.allows_subtree_extraction());
}

#[test]
fn departure_arrival_require_exact_independent_document_heading_evidence() {
    let (mut registry, source, destination) = contexts();
    let foreign = contexts().2;
    let skipped = registry
        .begin_navigation(source.identity().id(), ContextOperationId::new(4).unwrap())
        .unwrap()
        .context();
    for (context, id, origin, role, heading, duplicate) in [
        (source, 1, ORIGIN, "heading", ARRIVAL, false),
        (source, 1, ORIGIN, "link", DEPARTURE, false),
        (source, 1, ORIGIN, "heading", DEPARTURE, true),
        (
            source,
            1,
            "https://example.test",
            "heading",
            DEPARTURE,
            false,
        ),
    ] {
        let mut task = ReactNavigationTask::new(source.identity()).unwrap();
        task.attest_account(source, AgentPolicyInstant::from_millis(10))
            .unwrap();
        assert!(task
            .evaluate(&observation(context, id, origin, role, heading, duplicate))
            .is_err());
        assert!(task.departure.is_none());
    }
    for (context, id, origin, role, heading, duplicate) in [
        (source, 2, ORIGIN, "heading", ARRIVAL, false),
        (foreign, 2, ORIGIN, "heading", ARRIVAL, false),
        (skipped, 2, ORIGIN, "heading", ARRIVAL, false),
        (destination, 1, ORIGIN, "heading", ARRIVAL, false),
        (destination, 2, ORIGIN, "heading", DEPARTURE, false),
        (destination, 2, ORIGIN, "link", ARRIVAL, false),
        (destination, 2, ORIGIN, "heading", ARRIVAL, true),
        (
            destination,
            2,
            "https://example.test",
            "heading",
            ARRIVAL,
            false,
        ),
    ] {
        let mut task = ReactNavigationTask::new(source.identity()).unwrap();
        task.attest_account(source, AgentPolicyInstant::from_millis(10))
            .unwrap();
        task.evaluate(&observation(source, 1, ORIGIN, "heading", DEPARTURE, false))
            .unwrap();
        assert!(task
            .evaluate(&observation(context, id, origin, role, heading, duplicate))
            .is_err());
        assert!(task.arrival.is_none());
        assert!(task
            .attest_account(destination, AgentPolicyInstant::from_millis(20))
            .is_err());
    }
}

#[test]
fn isolated_account_samples_are_document_bound_fresh_once_and_never_renewed() {
    let (mut registry, source, destination) = contexts();
    let mut task = ReactNavigationTask::new(source.identity()).unwrap();
    let first = task
        .attest_account(source, AgentPolicyInstant::from_millis(10))
        .unwrap();
    assert_eq!(
        task.attest_account(source, AgentPolicyInstant::from_millis(100))
            .unwrap(),
        first
    );
    assert!(task
        .attest_account(destination, AgentPolicyInstant::from_millis(20))
        .is_err());
    task.evaluate(&observation(source, 1, ORIGIN, "heading", DEPARTURE, false))
        .unwrap();
    assert!(task
        .attest_account(destination, AgentPolicyInstant::from_millis(20))
        .is_err());
    task.evaluate(&observation(
        destination,
        2,
        ORIGIN,
        "heading",
        ARRIVAL,
        false,
    ))
    .unwrap();
    assert!(task
        .attest_account(destination, AgentPolicyInstant::from_millis(9))
        .is_err());
    let second = task
        .attest_account(destination, AgentPolicyInstant::from_millis(20))
        .unwrap();
    assert_eq!(second.context(), destination);
    assert_eq!(second.account(), AgentAccountScope::Anonymous);
    assert_ne!(second.attestation(), first.attestation());
    assert_eq!(
        task.attest_account(destination, AgentPolicyInstant::from_millis(100))
            .unwrap(),
        second
    );
    assert!(task
        .attest_account(source, AgentPolicyInstant::from_millis(100))
        .is_err());
    let cancelled = registry
        .cancel_run(source.identity().id(), destination)
        .unwrap();
    assert!(task
        .attest_account(cancelled, AgentPolicyInstant::from_millis(100))
        .is_err());
    assert!(task
        .evaluate(&observation(
            cancelled, 3, ORIGIN, "heading", ARRIVAL, false
        ))
        .is_err());
}

#[test]
fn final_citation_must_be_the_exact_fresh_arrival_not_a_model_or_source_substitute() {
    let (_, source, destination) = contexts();
    for (context, id, origin, role, heading, value) in [
        (source, 1, ORIGIN, "heading", DEPARTURE, ARRIVAL),
        (source, 2, ORIGIN, "heading", ARRIVAL, ARRIVAL),
        (contexts().2, 2, ORIGIN, "heading", ARRIVAL, ARRIVAL),
        (destination, 3, ORIGIN, "heading", ARRIVAL, ARRIVAL),
        (
            destination,
            2,
            "https://example.test",
            "heading",
            ARRIVAL,
            ARRIVAL,
        ),
        (destination, 2, ORIGIN, "link", ARRIVAL, ARRIVAL),
        (destination, 2, ORIGIN, "heading", DEPARTURE, ARRIVAL),
        (
            destination,
            2,
            ORIGIN,
            "heading",
            ARRIVAL,
            "Your first component",
        ),
    ] {
        let mut task = ready(source, destination);
        let observation = observation(context, id, origin, role, heading, false);
        let schema = task
            .extraction_schema()
            .unwrap()
            .clone()
            .with_source_roles(SemanticReadRoleSelection::ALL);
        let result = mapped(&schema, &observation, value);
        assert!(task.accept_extraction(&result).is_err());
        assert!(!task.complete);
    }
    let mut task = ready(source, destination);
    let observation = observation(destination, 2, ORIGIN, "heading", ARRIVAL, false);
    let result = mapped(task.extraction_schema().unwrap(), &observation, ARRIVAL);
    assert_eq!(
        task.accept_extraction(&result).unwrap(),
        AgentWorkTaskProgress::Complete
    );
    assert!(task.accept_extraction(&result).is_err());
    assert!(verify_owned(&result.into_owned().unwrap()));
}
