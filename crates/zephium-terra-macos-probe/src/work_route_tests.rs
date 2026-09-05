use super::*;

fn contexts() -> (ContextRegistry, [ContextJoin; 3]) {
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
    let operation = registry
        .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
        .unwrap();
    registry
        .settle_construction(identity.id(), operation, ContextSettlement::Applied)
        .unwrap();
    let documents = [2, 3, 4].map(|id| {
        let operation = registry
            .begin_navigation(identity.id(), ContextOperationId::new(id).unwrap())
            .unwrap();
        registry
            .settle_navigation(identity.id(), operation, ContextSettlement::Applied)
            .unwrap();
        operation.context()
    });
    (registry, documents)
}

fn observation(
    context: ContextJoin,
    id: u64,
    origin: &str,
    role: &str,
    headings: &[&str],
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
    let mut nodes = vec![r#"{"k":1,"r":"document","o":16}"#.to_owned()];
    nodes.extend(headings.iter().enumerate().map(|(i, heading)| {
        format!(
            r#"{{"k":{},"p":0,"r":"{role}"{level},"n":"{heading}"}}"#,
            i + 2
        )
    }));
    let wire = format!(
        r#"{{"v":1,"i":1,"g":1,"c":"complete","n":[{}]}}"#,
        nodes.join(",")
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

fn checkpoint(context: ContextJoin, index: usize) -> SemanticObservation {
    observation(
        context,
        index as u64 + 1,
        ORIGIN,
        "heading",
        if index == 1 {
            &[HEADINGS[1], MIDDLE_DEPARTURE]
        } else {
            &HEADINGS[index..=index]
        },
    )
}

fn ready(documents: [ContextJoin; 3], count: usize) -> ReactRouteTask {
    let mut task = ReactRouteTask::new(documents[0].identity()).unwrap();
    task.attest_account(documents[0], AgentPolicyInstant::from_millis(10))
        .unwrap();
    for (index, context) in documents.into_iter().enumerate().take(count) {
        assert_eq!(
            task.evaluate(&checkpoint(context, index)).unwrap(),
            if index == 2 {
                AgentWorkTaskProgress::ReadyForExtraction
            } else {
                AgentWorkTaskProgress::ReadyForNavigation
            }
        );
        task.attest_account(
            context,
            AgentPolicyInstant::from_millis((index as u64 + 1) * 10),
        )
        .unwrap();
    }
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
    let delivery = encode_semantic_read(
        &read,
        SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
    )
    .unwrap()
    .admit_conservative_utf8(&SemanticTokenizerRevision::try_new("route-test-v1".into()).unwrap())
    .unwrap()
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
fn closed_three_document_profile_has_only_frozen_exact_targets_and_heading_extraction() {
    let (_, documents) = contexts();
    let site = super::super::work_sites::Site::parse("react-route")
        .ok()
        .unwrap();
    let task = site.task(documents[0].identity()).unwrap();
    assert_eq!(site.navigation_proposals(), 2);
    assert_eq!(site.url(), URLS[0]);
    assert_eq!(task.navigation_route(), Some(&route().unwrap()));
    assert_eq!(
        task.navigation_route()
            .unwrap()
            .destinations()
            .iter()
            .map(|target| target.as_url().as_str())
            .collect::<Vec<_>>(),
        URLS[1..]
    );
    assert!(task.navigation_target().is_none());
    assert!(!task.allows_actions_before_extraction());
    assert!(!task.allows_baseline_read());
    assert!(!task.allows_subtree_extraction());
    assert_eq!(
        task.extraction_schema().unwrap().source_roles(),
        SemanticReadRoleSelection::try_new(&[SemanticRole::Heading]).unwrap()
    );
    for name in [
        "react-route/other",
        "react-route?redirect=1",
        "react-route#fragment",
        URLS[2],
    ] {
        assert!(super::super::work_sites::Site::parse(name).is_err());
    }
}

#[test]
fn every_document_requires_exact_order_and_middle_arrival_and_departure_independently() {
    let (_, documents) = contexts();
    let (_, foreign) = contexts();
    for index in 0..3 {
        for fault in 0..9 {
            let mut task = ready(documents, index);
            let context = match fault {
                1 => foreign[index],
                2 => documents[if index == 2 { 0 } else { 2 }],
                _ => documents[index],
            };
            let role = if fault == 3 { "link" } else { "heading" };
            let origin = if fault == 4 {
                "https://example.test"
            } else {
                ORIGIN
            };
            let id = if fault == 5 { 1 } else { index as u64 + 1 };
            let headings = match fault {
                6 => vec![HEADINGS[(index + 1) % 3]],
                7 => vec![HEADINGS[index], HEADINGS[index]],
                8 if index == 1 => vec![MIDDLE_DEPARTURE],
                _ if index == 1 && fault != 0 => vec![HEADINGS[1], MIDDLE_DEPARTURE],
                _ => vec![HEADINGS[index]],
            };
            // Mode zero specifically removes the middle departure predicate.
            if (fault == 0 && index != 1)
                || (fault == 5 && index == 0)
                || (fault == 8 && index != 1)
            {
                continue;
            }
            assert!(
                task.evaluate(&observation(context, id, origin, role, &headings))
                    .is_err(),
                "index {index} fault {fault}"
            );
            assert!(task.arrival.is_none());
            assert_eq!(task.next_document, index);
            if index > 0 {
                assert!(task
                    .attest_account(documents[index], AgentPolicyInstant::from_millis(100))
                    .is_err());
            }
        }
    }
    let task = ready(documents, 3);
    assert!(task.arrival.is_some());
    assert_eq!(task.next_document, 3);
}

#[test]
fn isolated_route_account_samples_are_fresh_once_document_bound_and_not_renewable() {
    let (mut registry, documents) = contexts();
    let mut task = ReactRouteTask::new(documents[0].identity()).unwrap();
    let first = task
        .attest_account(documents[0], AgentPolicyInstant::from_millis(10))
        .unwrap();
    assert_eq!(
        task.attest_account(documents[0], AgentPolicyInstant::from_millis(100))
            .unwrap(),
        first
    );
    for index in 0..3 {
        if index > 0 {
            assert!(task
                .attest_account(documents[index], AgentPolicyInstant::from_millis(100))
                .is_err());
        }
        task.evaluate(&checkpoint(documents[index], index)).unwrap();
        if index > 0 {
            assert!(task
                .attest_account(documents[index], AgentPolicyInstant::from_millis(1))
                .is_err());
        }
        let sample = task
            .attest_account(
                documents[index],
                AgentPolicyInstant::from_millis((index as u64 + 1) * 10),
            )
            .unwrap();
        assert_eq!(sample.context(), documents[index]);
        assert_eq!(sample.account(), AgentAccountScope::Anonymous);
        assert_eq!(
            task.attest_account(documents[index], AgentPolicyInstant::from_millis(100))
                .unwrap(),
            sample
        );
        for prior in &documents[..index] {
            assert!(task
                .attest_account(*prior, AgentPolicyInstant::from_millis(100))
                .is_err());
        }
    }
    let samples = task.samples.get().map(Option::unwrap);
    for i in 0..3 {
        for j in i + 1..3 {
            assert_ne!(samples[i].attestation(), samples[j].attestation());
        }
    }
    let cancelled = registry
        .cancel_run(documents[2].identity().id(), documents[2])
        .unwrap();
    assert!(task
        .attest_account(cancelled, AgentPolicyInstant::from_millis(100))
        .is_err());
    assert!(task.evaluate(&checkpoint(cancelled, 2)).is_err());
    assert!(task.arrival.is_none());
}

#[test]
fn route_result_requires_final_document_exact_heading_citation_and_cannot_reuse_prior_phases() {
    let (_, documents) = contexts();
    for (context, id, origin, role, text, value) in [
        (documents[0], 1, ORIGIN, "heading", HEADINGS[0], HEADINGS[2]),
        (documents[1], 2, ORIGIN, "heading", HEADINGS[1], HEADINGS[2]),
        (documents[1], 3, ORIGIN, "heading", HEADINGS[2], HEADINGS[2]),
        (
            contexts().1[2],
            3,
            ORIGIN,
            "heading",
            HEADINGS[2],
            HEADINGS[2],
        ),
        (documents[2], 4, ORIGIN, "heading", HEADINGS[2], HEADINGS[2]),
        (
            documents[2],
            3,
            "https://example.test",
            "heading",
            HEADINGS[2],
            HEADINGS[2],
        ),
        (documents[2], 3, ORIGIN, "link", HEADINGS[2], HEADINGS[2]),
        (documents[2], 3, ORIGIN, "heading", HEADINGS[1], HEADINGS[2]),
        (
            documents[2],
            3,
            ORIGIN,
            "heading",
            HEADINGS[2],
            "Importing components",
        ),
    ] {
        let mut task = ready(documents, 3);
        let observation = observation(context, id, origin, role, &[text]);
        let schema = task
            .extraction_schema()
            .unwrap()
            .clone()
            .with_source_roles(SemanticReadRoleSelection::ALL);
        let result = mapped(&schema, &observation, value);
        assert!(task.accept_extraction(&result).is_err());
        assert!(!task.complete);
    }
    let final_observation = checkpoint(documents[2], 2);
    let mut task = ready(documents, 2);
    let result = mapped(
        task.extraction_schema().unwrap(),
        &final_observation,
        HEADINGS[2],
    );
    assert!(
        task.accept_extraction(&result).is_err(),
        "cannot extract before final phase and account"
    );
    let mut task = ready(documents, 3);
    assert_eq!(
        task.accept_extraction(&result).unwrap(),
        AgentWorkTaskProgress::Complete
    );
    assert!(task.accept_extraction(&result).is_err());
    assert!(task
        .attest_account(documents[2], AgentPolicyInstant::from_millis(100))
        .is_err());
    assert!(verify_owned(&result.into_owned().unwrap()));
}
