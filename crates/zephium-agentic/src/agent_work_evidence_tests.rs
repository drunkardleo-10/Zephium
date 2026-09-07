use super::*;
use serde_json::json;

fn fixture(
    work: WorkId,
    profile: AgentWorkProfileId,
    run: ContextRunId,
    document: &str,
    quote: &str,
    sensitive: bool,
) -> (WorkBrowserReadBinding, Arc<SemanticOwnedExtractionResult>) {
    let tick = AgentPolicyInstant::from_millis;
    let mut rows = WorkBrowserResources::new(work, profile);
    let request = rows
        .construct_document(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            ContextProfileStorageClass::Ephemeral,
            ContextNavigationTarget::parse(document).unwrap(),
            tick(0),
        )
        .unwrap();
    let resource = request.resource().clone();
    let _ = rows
        .settle_at(
            request.complete(WorkBrowserResourceNativeOutcome::Constructed),
            tick(0),
        )
        .unwrap();
    let request = rows.acquire(&resource, run, tick(1), tick(100)).unwrap();
    let lease = request.lease().unwrap().clone();
    let _ = rows
        .settle_at(
            request.complete(WorkBrowserResourceNativeOutcome::Acquired),
            tick(1),
        )
        .unwrap();
    let binding = rows.read_binding(&lease, tick(2)).unwrap();
    let request = rows.observe_initial(&lease, tick(2)).unwrap();
    let observation_request = request.observation().clone();
    let (invocation, completion) = request.into_parts();
    let wire = serde_json::to_vec(&json!({
        "v": SEMANTIC_WIRE_VERSION, "i": invocation.invocation().get(),
        "g": invocation.snapshot_generation().get(), "c": "scope_boundary",
        "n": [
            {"k": 1, "r": "document"},
            {"k": 2, "p": 0, "r": "paragraph", "t": quote,
             "q": if sensitive { "sensitive" } else { "public" }},
            {"k": 3, "p": 0, "r": "paragraph", "t": "Other evidence"}
        ]
    }))
    .unwrap();
    let snapshot = invocation.decode_result(&wire).unwrap();
    let WorkBrowserObservationEvent::Snapshot(snapshot) = rows
        .settle_observation(completion.settle(Ok(snapshot)), tick(2))
        .unwrap()
    else {
        panic!("snapshot");
    };
    let observation = SemanticObservationAssembler::new(observation_request, *snapshot)
        .unwrap()
        .finish()
        .unwrap();
    let read = read_semantic_observation(
        &observation,
        SemanticReadAuthority::Initial,
        SemanticCaptureInstant::from_millis(2),
        SemanticReadSensitivityLimit::Sensitive,
        SemanticReadBudget::STANDARD,
    )
    .unwrap();
    let revision = SemanticTokenizerRevision::try_new("evidence-fixture-v1".into()).unwrap();
    let delivery = encode_semantic_read(
        &read,
        SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
    )
    .unwrap()
    .admit_conservative_utf8(&revision)
    .unwrap()
    .settle_delivery(SemanticModelDeliverySettlement::Committed)
    .unwrap();
    let schema = SemanticExtractionSchema::try_new(
        SemanticExtractionSchemaId::new(1).unwrap(),
        vec![
            SemanticExtractionFieldSchema::try_text("summary".into(), true, 256).unwrap(),
            SemanticExtractionFieldSchema::try_text_list("claims".into(), true, 2, 256).unwrap(),
        ],
    )
    .unwrap();
    let output = serde_json::to_vec(
        &json!({"v": SEMANTIC_EXTRACTION_SCHEMA_VERSION, "schema": 1,
        "fields": [
            {"name":"summary", "value":{"k":"text", "value":"Mapped claim", "sources":["@r1"]}},
            {"name":"claims", "value":{"k":"text_list", "sources":["@r1","@r2"], "items":[
                {"value":"First claim", "sources":["@r1"]},
                {"value":"Second claim", "sources":["@r2"]}
            ]}}
        ]}),
    )
    .unwrap();
    let result = extract_semantic_read(
        &schema,
        &read,
        &delivery,
        SemanticReadSensitivityLimit::Sensitive,
        &output,
    )
    .unwrap()
    .into_owned()
    .unwrap();
    // Native registry and observation die here. Only historical source data survives.
    (binding, Arc::new(result))
}

fn budget() -> WorkEvidenceBudget {
    WorkEvidenceBudget::try_new(MAX_WORK_EVIDENCE_ENTRIES, MAX_WORK_EVIDENCE_CONTENT_BYTES).unwrap()
}

pub(super) fn descriptor(
    work: WorkId,
    profile: AgentWorkProfileId,
    run: ContextRunId,
) -> WorkEvidenceDescriptor {
    let (binding, result) = fixture(
        work,
        profile,
        run,
        "https://handoff.example.test/source",
        "Source quote",
        false,
    );
    let entry = WorkEvidenceEntry::try_new(&binding, &result).unwrap();
    let mut builder = WorkEvidenceBuilder::new(work, profile, budget());
    builder.insert(&entry).unwrap();
    builder.finish().unwrap().descriptor()
}
fn span(result: &SemanticOwnedExtractionResult) -> SemanticExtractionSourceSpan {
    let SemanticExtractedValue::Text(text) = result.fields()[0].value() else {
        panic!("text");
    };
    text.source_span()
}

#[test]
fn multiple_resources_keep_local_citation_spaces_and_original_omissions() {
    let work = WorkId::generate();
    let profile = AgentWorkProfileId::generate();
    let run = ContextRunId::generate();
    let (a_binding, a) = fixture(
        work,
        profile,
        run,
        "https://a.example.test/one",
        "Quote one",
        false,
    );
    let (b_binding, b) = fixture(
        work,
        profile,
        run,
        "https://b.example.test/two",
        "Contradictory quote two",
        false,
    );
    let first = WorkEvidenceEntry::try_new(&a_binding, &a).unwrap();
    let second = WorkEvidenceEntry::try_new(&b_binding, &b).unwrap();
    assert_ne!(first.id(), second.id());
    assert!(a.sources(span(&b)).is_none());
    assert!(b.sources(span(&a)).is_none());
    assert_eq!(a.sources(span(&a)).unwrap().next().unwrap().id.get(), 1);
    assert_eq!(b.sources(span(&b)).unwrap().next().unwrap().id.get(), 1);
    let mut builder = WorkEvidenceBuilder::new(work, profile, budget());
    builder.insert(&first).unwrap();
    builder.insert(&second).unwrap();
    let set = builder.finish().unwrap();
    drop((a_binding, b_binding, a, b));
    assert_eq!(set.descriptor().run(), run);
    assert_eq!(set.descriptor().work(), work);
    assert_eq!(set.descriptor().profile(), profile);
    assert_eq!(set.descriptor().entries(), 2);
    assert_ne!(set.descriptor().digest(), [0; 32]);
    for entry in set.entries() {
        assert_eq!(entry.storage(), ContextProfileStorageClass::Ephemeral);
        assert_eq!(
            entry.extraction().trust(),
            SemanticExtractionTrust::ModelMapped
        );
        assert_eq!(entry.review(), WorkEvidenceReview::NeedsReview);
        assert!(entry
            .extraction()
            .read_omissions()
            .contains(SemanticReadOmission::SourceIncomplete));
        assert_eq!(entry.extraction().read_stats().items(), 2);
        let SemanticExtractedValue::TextList(list) = entry.extraction().fields()[1].value() else {
            panic!("list");
        };
        assert_eq!(
            entry
                .extraction()
                .sources(list.source_span())
                .unwrap()
                .count(),
            2
        );
        assert_eq!(
            entry
                .extraction()
                .sources(list.items()[1].source_span())
                .unwrap()
                .next()
                .unwrap()
                .id
                .get(),
            2
        );
    }
    let shared = set.clone();
    assert_eq!(shared.descriptor(), set.descriptor());
    assert!(Arc::ptr_eq(&shared.entries()[0], &set.entries()[0]));
    assert_eq!(
        set.entry(first.id()).unwrap().document(),
        "https://a.example.test/one"
    );
}

#[test]
fn exact_binding_and_public_only_admission_do_not_consume_results() {
    let work = WorkId::generate();
    let profile = AgentWorkProfileId::generate();
    let run = ContextRunId::generate();
    let (a_binding, a) = fixture(
        work,
        profile,
        run,
        "https://a.example.test/one",
        "Quote",
        false,
    );
    let (b_binding, sensitive) = fixture(
        work,
        profile,
        run,
        "https://a.example.test/two",
        "Private quote",
        true,
    );
    assert_eq!(
        WorkEvidenceEntry::try_new(&b_binding, &a).unwrap_err(),
        WorkEvidenceError::Source
    );
    assert_eq!(
        WorkEvidenceEntry::try_new(&a_binding, &sensitive).unwrap_err(),
        WorkEvidenceError::Source
    );
    assert_eq!(
        WorkEvidenceEntry::try_new(&b_binding, &sensitive).unwrap_err(),
        WorkEvidenceError::Source
    );
    assert_eq!(a.fields().len(), 2);
    assert_eq!(Arc::strong_count(&a), 1);
}

#[test]
fn duplicates_scope_run_and_capacity_fail_atomically_without_eviction() {
    let work = WorkId::generate();
    let profile = AgentWorkProfileId::generate();
    let run = ContextRunId::generate();
    let (binding, result) = fixture(
        work,
        profile,
        run,
        "https://example.test/one",
        "Quote",
        false,
    );
    let first = WorkEvidenceEntry::try_new(&binding, &result).unwrap();
    let duplicate = WorkEvidenceEntry::try_new(&binding, &result).unwrap();
    let mut builder = WorkEvidenceBuilder::new(work, profile, budget());
    builder.insert(&first).unwrap();
    assert_eq!(builder.insert(&first), Err(WorkEvidenceError::Duplicate));
    assert_eq!(
        builder.insert(&duplicate),
        Err(WorkEvidenceError::Duplicate)
    );
    for (other_work, other_profile, other_run) in [
        (WorkId::generate(), profile, run),
        (work, AgentWorkProfileId::generate(), run),
        (work, profile, ContextRunId::generate()),
    ] {
        let (binding, result) = fixture(
            other_work,
            other_profile,
            other_run,
            "https://example.test/other",
            "Quote",
            false,
        );
        let entry = WorkEvidenceEntry::try_new(&binding, &result).unwrap();
        assert_eq!(builder.insert(&entry), Err(WorkEvidenceError::Scope));
    }
    assert_eq!(builder.finish().unwrap().descriptor().entries(), 1);
    let mut bytes = WorkEvidenceBuilder::new(
        work,
        profile,
        WorkEvidenceBudget::try_new(2, first.content_bytes() - 1).unwrap(),
    );
    assert_eq!(bytes.insert(&first), Err(WorkEvidenceError::Capacity));
    assert!(matches!(bytes.finish(), Err(WorkEvidenceError::Empty)));
    let mut one = WorkEvidenceBuilder::new(
        work,
        profile,
        WorkEvidenceBudget::try_new(1, first.content_bytes()).unwrap(),
    );
    one.insert(&first).unwrap();
    let (other_binding, other_result) = fixture(
        work,
        profile,
        run,
        "https://example.test/next",
        "Next",
        false,
    );
    let other = WorkEvidenceEntry::try_new(&other_binding, &other_result).unwrap();
    assert_eq!(one.insert(&other), Err(WorkEvidenceError::Capacity));
    assert_eq!(
        one.finish().unwrap().descriptor().content_bytes(),
        first.content_bytes()
    );
}

#[test]
fn bounds_and_all_diagnostics_are_content_free() {
    assert_eq!(
        WorkEvidenceBudget::try_new(0, 1),
        Err(WorkEvidenceError::Capacity)
    );
    assert_eq!(
        WorkEvidenceBudget::try_new(MAX_WORK_EVIDENCE_ENTRIES + 1, 1),
        Err(WorkEvidenceError::Capacity)
    );
    assert_eq!(
        WorkEvidenceBudget::try_new(1, MAX_WORK_EVIDENCE_CONTENT_BYTES + 1),
        Err(WorkEvidenceError::Capacity)
    );
    let work = WorkId::generate();
    let profile = AgentWorkProfileId::generate();
    let run = ContextRunId::generate();
    let (binding, result) = fixture(
        work,
        profile,
        run,
        "https://private-source.example.test/detail",
        "Unique hostile quote",
        false,
    );
    let entry = WorkEvidenceEntry::try_new(&binding, &result).unwrap();
    let mut builder = WorkEvidenceBuilder::new(work, profile, budget());
    builder.insert(&entry).unwrap();
    let debug = format!("{builder:?} {entry:?} {:?}", entry.id());
    let set = builder.finish().unwrap();
    let debug = format!(
        "{debug} {set:?} {:?} {:?}",
        set.descriptor(),
        entry.extraction()
    );
    for secret in [
        "private-source",
        "Unique hostile",
        "Mapped claim",
        "First claim",
        &profile.to_string(),
        &format!("{:?}", run.bytes()),
        &format!("{:?}", work.bytes()),
    ] {
        assert!(!debug.contains(secret));
    }
}
