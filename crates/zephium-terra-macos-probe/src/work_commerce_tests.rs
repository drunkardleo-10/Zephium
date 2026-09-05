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
    let op = registry
        .begin_navigation(identity.id(), ContextOperationId::new(2).unwrap())
        .unwrap();
    registry
        .settle_navigation(identity.id(), op, ContextSettlement::Applied)
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
    completeness: &str,
    nodes: &str,
) -> SemanticObservation {
    let frame = SemanticFrameJoin::try_new(
        context,
        FrameId::MAIN,
        context.frame_generation(),
        SemanticOrigin::parse(origin).unwrap(),
        SemanticFrameTrust::SameOrigin,
    )
    .unwrap();
    let wire = format!(
        r#"{{"v":1,"i":1,"g":1,"c":"{completeness}","n":[{{"k":1,"r":"document","o":16}},{nodes}]}}"#
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
    let boundaries: Vec<_> = snapshot
        .nodes()
        .iter()
        .filter(|node| node.role() == SemanticRole::FrameBoundary)
        .map(|node| node.reference())
        .collect();
    let mut assembler = SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(id).unwrap(),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        ),
        snapshot,
    )
    .unwrap();
    for boundary in boundaries {
        assembler
            .mark_frame_unsupported(
                FrameId::MAIN,
                boundary,
                SemanticFrameUnsupported::PlatformIsolationUnavailable,
            )
            .unwrap();
    }
    assembler.finish().unwrap()
}

fn catalog_nodes() -> String {
    format!(r#"{{"k":2,"p":0,"r":"link","n":"{CATALOG_LINK}"}}"#)
}

fn product_nodes() -> String {
    format!(
        r#"{{"k":2,"p":0,"r":"heading","l":1,"n":"{PRODUCT}"}},{{"k":3,"p":0,"r":"paragraph","t":"{PRICE}"}},{{"k":4,"p":0,"r":"paragraph","t":"{MATERIAL}"}},{{"k":5,"p":0,"r":"button","n":"Add To Cart"}}"#
    )
}

fn departed(source: ContextJoin) -> CommerceProductTask {
    let mut task = CommerceProductTask::new(source.identity()).unwrap();
    task.attest_account(source, AgentPolicyInstant::from_millis(10))
        .unwrap();
    assert_eq!(
        task.evaluate(&observation(
            source,
            1,
            ORIGIN,
            "complete",
            &catalog_nodes()
        ))
        .unwrap(),
        AgentWorkTaskProgress::ReadyForNavigation
    );
    task
}

fn ready(source: ContextJoin, destination: ContextJoin) -> CommerceProductTask {
    let mut task = departed(source);
    assert_eq!(
        task.evaluate(&observation(
            destination,
            2,
            ORIGIN,
            "complete",
            &product_nodes()
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
    values: [&str; 3],
    citations: [&str; 3],
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
        &SemanticTokenizerRevision::try_new("commerce-test-v1".into()).unwrap(),
    )
    .unwrap();
    let delivery = payload
        .settle_delivery(SemanticModelDeliverySettlement::Committed)
        .unwrap();
    let fields = FIELDS
        .iter()
        .enumerate()
        .map(|(index, (name, _, _, _))| {
            format!(
                r#"{{"name":"{name}","value":{{"k":"text","value":"{}","sources":[{}]}}}}"#,
                values[index], citations[index]
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let output = format!(r#"{{"v":1,"schema":1,"fields":[{fields}]}}"#);
    extract_semantic_read(
        schema,
        &read,
        &delivery,
        SemanticReadSensitivityLimit::PublicOnly,
        output.as_bytes(),
    )
    .unwrap()
}

const VALUES: [&str; 3] = [PRODUCT, PRICE, MATERIAL];
const CITATIONS: [&str; 3] = [r#""@r1""#, r#""@r2""#, r#""@r3""#];

#[test]
fn closed_commerce_profile_freezes_one_route_three_fields_and_roles() {
    let (_, source, _) = contexts();
    let site = super::super::work_sites::Site::parse("commerce-product")
        .ok()
        .unwrap();
    assert_eq!(site.url(), DEPARTURE);
    assert_eq!(site.navigation_proposals(), 1);
    let task = site.task(source.identity()).unwrap();
    assert!(task.navigation_target().is_none());
    let route = task.navigation_route().unwrap();
    assert_eq!(route, &super::route().unwrap());
    assert_eq!(route.departure().as_url().as_str(), DEPARTURE);
    assert_eq!(route.destinations().len(), 1);
    assert_eq!(route.destinations()[0].as_url().as_str(), DESTINATION);
    let schema = task.extraction_schema().unwrap();
    assert_eq!(
        schema.source_roles(),
        SemanticReadRoleSelection::try_new(&[SemanticRole::Heading, SemanticRole::Paragraph])
            .unwrap()
    );
    for (field, (name, value, _, _)) in schema.fields().iter().zip(FIELDS) {
        assert_eq!(field.name(), name);
        assert_eq!(field.max_text_bytes(), Some(value.len()));
        assert!(field.required());
    }
    assert!(!task.allows_actions_before_extraction());
    assert!(!task.allows_baseline_read());
    assert!(!task.allows_subtree_extraction());
    for parameter in [
        "commerce-product/other",
        "commerce-product?redirect=1",
        DESTINATION,
    ] {
        assert!(super::super::work_sites::Site::parse(parameter).is_err());
    }
    let borrowed = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        1_u128.into(),
        ContextKind::BorrowedTab,
    );
    assert!(CommerceProductTask::new(borrowed).is_err());
}

#[test]
fn departure_requires_exact_unique_public_price_bearing_link_and_complete_capture() {
    let (_, source, _) = contexts();
    let catalog = catalog_nodes();
    for (origin, completeness, nodes) in [
        (ORIGIN, "complete", product_nodes()),
        (ORIGIN, "complete", catalog.replace("link", "paragraph")),
        (ORIGIN, "complete", catalog.replace(CATALOG_LINK, PRODUCT)),
        (ORIGIN, "complete", catalog.replace("$20.00", "$21.00")),
        (
            ORIGIN,
            "complete",
            catalog.replace("\"r\":\"link\"", "\"r\":\"link\",\"q\":\"sensitive\""),
        ),
        (ORIGIN, "complete", catalog.replace("T-Shirt ", "T-Shirt")),
        (
            ORIGIN,
            "complete",
            format!("{catalog},{}", catalog.replace("\"k\":2", "\"k\":3")),
        ),
        (ORIGIN, "inspection_limit", catalog.clone()),
        ("https://example.test", "complete", catalog.clone()),
    ] {
        let mut task = CommerceProductTask::new(source.identity()).unwrap();
        task.attest_account(source, AgentPolicyInstant::from_millis(10))
            .unwrap();
        assert!(task
            .evaluate(&observation(source, 1, origin, completeness, &nodes))
            .is_err());
        assert!(task.departure.is_none());
    }
    let mut task = CommerceProductTask::new(source.identity()).unwrap();
    assert!(task
        .evaluate(&observation(source, 1, ORIGIN, "complete", &catalog))
        .is_err());
}

#[test]
fn arrival_requires_three_unique_role_and_field_exact_current_document_facts() {
    let (mut registry, source, destination) = contexts();
    let skipped = registry
        .begin_navigation(source.identity().id(), ContextOperationId::new(4).unwrap())
        .unwrap()
        .context();
    let nodes = product_nodes();
    for (context, id, origin, completeness, nodes) in [
        (source, 2, ORIGIN, "complete", nodes.clone()),
        (contexts().2, 2, ORIGIN, "complete", nodes.clone()),
        (skipped, 2, ORIGIN, "complete", nodes.clone()),
        (destination, 1, ORIGIN, "complete", nodes.clone()),
        (
            destination,
            2,
            "https://example.test",
            "complete",
            nodes.clone(),
        ),
        (destination, 2, ORIGIN, "node_limit", nodes.clone()),
        (
            destination,
            2,
            ORIGIN,
            "complete",
            nodes.replace("$20.00", "$21.00"),
        ),
        (
            destination,
            2,
            ORIGIN,
            "complete",
            nodes.replace("60%", "61%"),
        ),
        (
            destination,
            2,
            ORIGIN,
            "complete",
            nodes.replace(" USD", ""),
        ),
        (
            destination,
            2,
            ORIGIN,
            "complete",
            nodes.replace(
                "\"r\":\"paragraph\"",
                "\"r\":\"paragraph\",\"q\":\"sensitive\"",
            ),
        ),
        (
            destination,
            2,
            ORIGIN,
            "complete",
            format!(r#"{nodes},{{"k":6,"p":0,"r":"frame_boundary"}}"#),
        ),
        (
            destination,
            2,
            ORIGIN,
            "complete",
            nodes.replace("paragraph", "link"),
        ),
        (
            destination,
            2,
            ORIGIN,
            "complete",
            nodes.replace("\"t\":", "\"n\":"),
        ),
        (
            destination,
            2,
            ORIGIN,
            "complete",
            format!(r#"{nodes},{{"k":6,"p":0,"r":"paragraph","t":"{PRICE}"}}"#),
        ),
        (
            destination,
            2,
            ORIGIN,
            "complete",
            format!(r#"{nodes},{{"k":6,"p":0,"r":"paragraph","t":"{MATERIAL}"}}"#),
        ),
        (
            destination,
            2,
            ORIGIN,
            "complete",
            format!(r#"{nodes},{{"k":6,"p":0,"r":"heading","l":1,"n":"{PRODUCT}"}}"#),
        ),
    ] {
        let mut task = departed(source);
        assert!(task
            .evaluate(&observation(context, id, origin, completeness, &nodes))
            .is_err());
        assert!(task.arrival.is_none());
        assert!(task
            .attest_account(destination, AgentPolicyInstant::from_millis(20))
            .is_err());
    }
}

#[test]
fn account_samples_are_fresh_per_verified_document_and_cached_without_renewal() {
    let (mut registry, source, destination) = contexts();
    let mut task = departed(source);
    let first = task
        .attest_account(source, AgentPolicyInstant::from_millis(100))
        .unwrap();
    assert_eq!(first.observed_at(), AgentPolicyInstant::from_millis(10));
    assert!(task
        .attest_account(destination, AgentPolicyInstant::from_millis(20))
        .is_err());
    task.evaluate(&observation(
        destination,
        2,
        ORIGIN,
        "complete",
        &product_nodes(),
    ))
    .unwrap();
    assert!(task
        .attest_account(destination, AgentPolicyInstant::from_millis(9))
        .is_err());
    let second = task
        .attest_account(destination, AgentPolicyInstant::from_millis(20))
        .unwrap();
    assert_ne!(first.attestation(), second.attestation());
    assert_eq!(second.account(), AgentAccountScope::Anonymous);
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
            cancelled,
            3,
            ORIGIN,
            "complete",
            &product_nodes()
        ))
        .is_err());
    assert!(task.arrival.is_none());
}

#[test]
fn extraction_rejects_stale_foreign_changed_role_field_and_reference_substitution() {
    let (_, source, destination) = contexts();
    let nodes = product_nodes();
    for (context, id, origin, nodes) in [
        (source, 1, ORIGIN, nodes.clone()),
        (source, 2, ORIGIN, nodes.clone()),
        (contexts().2, 2, ORIGIN, nodes.clone()),
        (destination, 3, ORIGIN, nodes.clone()),
        (destination, 2, "https://example.test", nodes.clone()),
        (destination, 2, ORIGIN, nodes.replace("paragraph", "link")),
        (destination, 2, ORIGIN, nodes.replace("\"t\":", "\"n\":")),
        (destination, 2, ORIGIN, nodes.replace("60%", "61%")),
        (
            destination,
            2,
            ORIGIN,
            format!(
                r#"{{"k":2,"p":0,"r":"button","n":"noise"}},{{"k":3,"p":0,"r":"heading","l":1,"n":"{PRODUCT}"}},{{"k":4,"p":0,"r":"paragraph","t":"{PRICE}"}},{{"k":5,"p":0,"r":"paragraph","t":"{MATERIAL}"}}"#
            ),
        ),
    ] {
        let mut task = ready(source, destination);
        let observation = observation(context, id, origin, "complete", &nodes);
        let schema = task.extraction_schema().unwrap().clone().with_source_roles(
            SemanticReadRoleSelection::try_new(&[
                SemanticRole::Heading,
                SemanticRole::Paragraph,
                SemanticRole::Link,
            ])
            .unwrap(),
        );
        let result = mapped(&schema, &observation, VALUES, CITATIONS);
        assert!(task.accept_extraction(&result).is_err());
        assert!(!task.complete);
    }
}

#[test]
fn extraction_rejects_invention_multiple_sources_and_reordered_citations() {
    let (_, source, destination) = contexts();
    let observation = observation(destination, 2, ORIGIN, "complete", &product_nodes());
    for (values, citations) in [
        ([PRODUCT, "$21.00 USD", MATERIAL], CITATIONS),
        ([PRODUCT, PRICE, "cotton tee"], CITATIONS),
        (VALUES, [r#""@r1""#, r#""@r3""#, r#""@r2""#]),
        (VALUES, [r#""@r1""#, r#""@r2","@r3""#, r#""@r3""#]),
    ] {
        let mut task = ready(source, destination);
        let result = mapped(
            task.extraction_schema().unwrap(),
            &observation,
            values,
            citations,
        );
        assert!(task.accept_extraction(&result).is_err());
        assert!(!verify_owned(&result.into_owned().unwrap()));
    }
}

#[test]
fn only_current_verified_account_and_three_exact_citations_can_complete_once() {
    let (_, source, destination) = contexts();
    let observation = observation(destination, 2, ORIGIN, "complete", &product_nodes());
    let mut task = departed(source);
    let result = mapped(
        task.extraction_schema().unwrap(),
        &observation,
        VALUES,
        CITATIONS,
    );
    assert!(task.accept_extraction(&result).is_err());
    task.evaluate(&observation).unwrap();
    assert!(task.accept_extraction(&result).is_err());
    task.attest_account(destination, AgentPolicyInstant::from_millis(20))
        .unwrap();
    assert_eq!(
        task.accept_extraction(&result).unwrap(),
        AgentWorkTaskProgress::Complete
    );
    assert!(task.accept_extraction(&result).is_err());
    assert!(task.evaluate(&observation).is_err());
    assert!(task
        .attest_account(destination, AgentPolicyInstant::from_millis(30))
        .is_err());
    let owned = result.into_owned().unwrap();
    assert!(super::super::work_sites::Site::CommerceProduct.verify_owned(&owned));
}

#[test]
fn rejected_replacement_observation_revokes_previously_verified_arrival() {
    let (_, source, destination) = contexts();
    let mut task = ready(source, destination);
    let arrival = observation(destination, 2, ORIGIN, "complete", &product_nodes());
    let result = mapped(
        task.extraction_schema().unwrap(),
        &arrival,
        VALUES,
        CITATIONS,
    );
    assert!(task
        .evaluate(&observation(
            destination,
            3,
            ORIGIN,
            "complete",
            &catalog_nodes()
        ))
        .is_err());
    assert!(task.accept_extraction(&result).is_err());
    assert!(!task.complete);
}
