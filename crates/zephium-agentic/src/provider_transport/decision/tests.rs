use super::*;
use crate::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    thread,
};
use zephium_decision::Question;

#[derive(Default)]
struct Accounting {
    reject: bool,
    active: Vec<AgentProviderCallIdentity>,
    receipts: Vec<(AgentModelCallReceipt, AgentProviderInputMetricReceipt)>,
}

impl DecisionCallAccounting for Accounting {
    fn activated(&mut self, call: AgentProviderCallIdentity) -> bool {
        self.active.push(call);
        !self.reject
    }
    fn settled(&mut self, receipt: AgentModelCallReceipt, input: AgentProviderInputMetricReceipt) {
        self.receipts.push((receipt, input));
    }
}

fn admitted_fixture() -> (
    AgentRunPolicy,
    AgentModelCallRequest,
    SemanticObservation,
    DecisionObservation,
) {
    admitted_fixture_with_actions(false)
}

fn admitted_fixture_with_actions(
    actions: bool,
) -> (
    AgentRunPolicy,
    AgentModelCallRequest,
    SemanticObservation,
    DecisionObservation,
) {
    admitted_fixture_with_nodes(if actions {
        json!([{"k":1,"r":"document","o":16},{"k":2,"p":0,"r":"button","n":"Details","o":1}])
    } else {
        json!([{"k":1,"r":"document"}])
    })
}

fn admitted_fixture_with_nodes(
    nodes: Value,
) -> (
    AgentRunPolicy,
    AgentModelCallRequest,
    SemanticObservation,
    DecisionObservation,
) {
    let identity = ContextIdentity::new(
        ContextId::from_raw(11),
        ContextRunId::from_raw(12),
        zephium_core::ids::ProfileId::from(13),
        ContextKind::Owned,
    );
    let mut contexts = ContextRegistry::new();
    contexts
        .reserve(
            identity,
            ContextCapabilities::try_new(ContextKind::Owned, &[ContextCapability::Observe])
                .unwrap(),
        )
        .unwrap();
    let op = contexts
        .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
        .unwrap();
    contexts
        .settle_construction(identity.id(), op, ContextSettlement::Applied)
        .unwrap();
    let context = contexts.join(identity.id()).unwrap();
    let source = SemanticOrigin::parse("https://example.test/").unwrap();
    let frame = SemanticFrameJoin::try_new(
        context,
        FrameId::MAIN,
        FrameGeneration::INITIAL,
        source.clone(),
        SemanticFrameTrust::SameOrigin,
    )
    .unwrap();
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(1).unwrap(),
            frame,
            SemanticSnapshotGeneration::new(1).unwrap(),
        ),
        &serde_json::to_vec(
            &json!({"v":SEMANTIC_WIRE_VERSION,"i":1,"g":1,"c":"complete","n":nodes}),
        )
        .unwrap(),
    )
    .unwrap();
    let observation = SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(1).unwrap(),
            context,
            SemanticObservationBudget::try_new(8, 4096, 1).unwrap(),
        ),
        snapshot,
    )
    .unwrap()
    .finish()
    .unwrap();
    let account = AgentContextAccountBinding::new(
        AgentAccountAttestationId::from_raw(14),
        context,
        AgentAccountScope::Anonymous,
        AgentPolicyInstant::from_millis(100),
    );
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).unwrap();
    let budget = AgentRunBudget::try_new(3, 100_000, 10_000, 1).unwrap();
    let node = AgentPlanNodeId::from_raw(15);
    let lease = AgentPlanLeaseId::from_raw(16);
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::from_raw(17),
        ContextRunId::from_raw(12),
        AgentRunScope::try_new(
            vec![identity.profile()],
            vec![AgentAccountScope::Anonymous],
            vec![source.clone()],
            SemanticSensitivity::Public,
            effects,
            vec![],
        )
        .unwrap(),
        budget,
        AgentPolicyInstant::from_millis(1),
        AgentPolicyInstant::from_millis(10000),
        vec![AgentPlanNodeScope::new(
            node,
            AgentPlanNodeAuthority::try_new(
                vec![identity.profile()],
                vec![AgentAccountScope::Anonymous],
                vec![source],
                SemanticSensitivity::Public,
                effects,
            )
            .unwrap(),
            budget,
            AgentPolicyInstant::from_millis(9999),
        )],
    )
    .unwrap();
    let policy =
        AgentRunPolicy::try_new(manifest, vec![AgentPlanLeaseBinding::new(lease, node)]).unwrap();
    let call = AgentModelCallRequest::new(
        AgentModelCallId::new(1).unwrap(),
        lease,
        account,
        JevDecisionClient::call_budget().unwrap(),
        AgentPolicyInstant::from_millis(101),
    );
    let objective = AgentProviderObjective::try_admit_conservative_utf8(
        "Read this public document".into(),
        &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
    )
    .unwrap();
    let entries: Vec<_> = observation
        .frames()
        .iter()
        .flat_map(SemanticSnapshot::nodes)
        .filter(|node| !node.operations().is_empty())
        .map(|node| (node.reference(), node.operations()))
        .collect();
    let authority = AgentProviderActionAuthority::try_new(&observation, &entries).unwrap();
    let projection =
        DecisionObservation::try_new(&observation, &objective, &authority, account).unwrap();
    (policy, call, observation, projection)
}

#[tokio::test]
async fn admitted_decision_settles_actual_usage_and_dropped_dispatch_charges_the_ceiling() {
    let (endpoint, server) = server(vec![success()]);
    let client = client(endpoint);
    let server = server();
    let (mut policy, call, observation, projection) = admitted_fixture();
    let mut accounting = Accounting::default();
    let output = client
        .evaluate_observation_accounted(
            &mut policy,
            call,
            &observation,
            &projection,
            Instant::now() + Duration::from_secs(4),
            &AgentProviderCancellation::new(),
            Some(&mut accounting),
        )
        .await
        .unwrap();
    assert_eq!(server.join().unwrap(), 1);
    assert_eq!(output.receipt.input_tokens(), 123);
    assert_eq!(output.receipt.output_tokens(), 7);
    assert_eq!(accounting.active.len(), 1);
    assert_eq!(accounting.receipts, vec![(output.receipt, output.input)]);
    assert_eq!(output.input.call(), output.receipt.id());
    let tokens = output.input.metrics().structured_input_tokens().unwrap();
    assert_eq!(tokens.tokens(), 123);
    assert_eq!(tokens.quality(), SemanticTokenCountQuality::ProviderExact);
    assert_eq!(policy.pending_model_calls(), 0);

    let (mut policy, call, observation, projection) = admitted_fixture();
    let admission = policy
        .prepare_decision_input(
            call,
            &observation,
            &projection,
            u64::from(MAX_JEV_INPUT_TOKENS),
        )
        .unwrap();
    let ack = crate::semantic_diff::SemanticObservationAcknowledgement::from_fingerprint(
        crate::semantic_diff::SemanticObservationFingerprint::from_observation(&observation),
    );
    let active = policy.commit_observation_input(admission, &ack).unwrap();
    let input = AgentProviderInputMetricReceipt::from_decision(
        &active,
        projection.input_stats(),
        projection.request().encode().unwrap().len() as u32,
        MAX_JEV_INPUT_TOKENS,
        SemanticTokenCountQuality::Conservative,
    );
    let mut cancelled = Accounting::default();
    drop(DecisionPolicyGuard {
        policy: &mut policy,
        active: Some(active),
        input,
        accounting: Some(&mut cancelled),
    });
    assert_eq!(cancelled.receipts.len(), 1);
    let (receipt, input) = cancelled.receipts[0];
    assert_eq!(receipt.id(), input.call());
    assert_eq!(receipt.input_tokens(), u64::from(MAX_JEV_INPUT_TOKENS));
    assert_eq!(
        input.metrics().structured_input_tokens().unwrap().quality(),
        SemanticTokenCountQuality::Conservative
    );
    assert_eq!(policy.pending_model_calls(), 0);
    assert_eq!(
        policy.accounting().consumed_model_tokens(),
        u64::from(MAX_JEV_INPUT_TOKENS + MAX_JEV_OUTPUT_TOKENS)
    );
    assert_eq!(
        policy.accounting().consumed_cost_micro_usd(),
        jev_cost(MAX_JEV_INPUT_TOKENS)
    );
}

#[tokio::test]
async fn host_accounting_refusal_stops_transport_and_preserves_terminal_receipts() {
    let (endpoint, _) = server(vec![]);
    let client = client(endpoint);
    let (mut policy, call, observation, projection) = admitted_fixture();
    let mut accounting = Accounting {
        reject: true,
        ..Accounting::default()
    };
    let result = client
        .evaluate_observation_accounted(
            &mut policy,
            call,
            &observation,
            &projection,
            Instant::now() + Duration::from_secs(2),
            &AgentProviderCancellation::new(),
            Some(&mut accounting),
        )
        .await;
    assert!(matches!(result, Err(AgentPolicyError::Authority)));
    assert_eq!(accounting.active.len(), 1);
    assert_eq!(accounting.receipts.len(), 1);
    assert_eq!(policy.pending_model_calls(), 0);
    assert!(client.transport.snapshot().unwrap().is_idle());
}

#[tokio::test]
async fn over_ceiling_vendor_usage_never_becomes_exact_accounting() {
    let body = json!({"model":zephium_decision::JEV_MODEL,"answers":{},"usage":{"input_tokens":MAX_JEV_INPUT_TOKENS + 1,"output_tokens":7}});
    let (endpoint, server) = server(vec![response(200, "", &body.to_string())]);
    let client = client(endpoint);
    let server = server();
    let (mut policy, call, observation, projection) = admitted_fixture();
    let output = client
        .evaluate_observation(
            &mut policy,
            call,
            &observation,
            &projection,
            Instant::now() + Duration::from_secs(4),
            &AgentProviderCancellation::new(),
        )
        .await
        .unwrap();
    assert_eq!(server.join().unwrap(), 1);
    assert!(output.call.response.is_err());
    assert_eq!(output.call.diagnostic.input_tokens, None);
    assert_eq!(
        output.receipt.input_tokens(),
        u64::from(MAX_JEV_INPUT_TOKENS)
    );
    assert_eq!(policy.pending_model_calls(), 0);
}

fn emulation_config() -> WorkPlanningConfig {
    WorkPlanningConfig::try_new(
        AgentProviderCallConfig::try_for_test(
            AgentProviderKind::OpenAiResponses,
            AgentProviderModelRevision::try_new("gpt-5.6-terra".into()).unwrap(),
            AgentProviderReasoningEffort::Medium,
            SemanticTokenizerRevision::try_new("decision-fixture:v1".into()).unwrap(),
            AgentProviderPricingProfile::try_new(
                AgentProviderPricingRevision::new(1).unwrap(),
                32768,
            )
            .unwrap(),
            1,
            4096,
            AgentProviderStreamBudget::STANDARD,
        )
        .unwrap(),
        8192,
        10_000,
    )
    .unwrap()
}

fn fixture_answers(request: &DecisionRequest) -> serde_json::Value {
    let answers: BTreeMap<_, _> = request.questions().iter().map(|(key, question)| {
        let value = match question {
            Question::Noul { .. } => json!({"type":"noul","noul":0.01}),
            Question::Choice { criteria, .. } => {
                let probabilities: BTreeMap<_, _> = criteria.keys().map(|key| (key, if key == "none" { 1.0 } else { 0.0 })).collect();
                json!({"type":"choice","choice":"none","confidence":1.0,"probabilities":probabilities})
            }
            Question::Score { .. } => panic!("unexpected fixture kind"),
        };
        (key.clone(), value)
    }).collect();
    json!({"answers":answers})
}

fn located_fixture(
    generated: bool,
) -> (
    AgentModelCallRequest,
    SemanticObservation,
    SemanticExtractionSchema,
    DecisionObservation,
) {
    located_fixture_with(generated, &[])
}

fn located_fixture_with(
    generated: bool,
    extra: &[Value],
) -> (
    AgentModelCallRequest,
    SemanticObservation,
    SemanticExtractionSchema,
    DecisionObservation,
) {
    located_fixture_priced(generated, extra, true, 512)
}

fn located_fixture_priced(
    generated: bool,
    extra: &[Value],
    price_required: bool,
    price_bytes: usize,
) -> (
    AgentModelCallRequest,
    SemanticObservation,
    SemanticExtractionSchema,
    DecisionObservation,
) {
    let (_, call, previous, _) = admitted_fixture();
    let mut nodes = json!([
        {"k":1,"r":"document","fc":true},
        {"k":2,"p":0,"r":"heading","l":1,"n":"Public product","t":"Public product","fc":true},
        {"k":3,"p":0,"r":"paragraph","n":"Price","t":"$349.99","fc":true},
        {"k":4,"p":0,"r":"image","n":"Product","m":"https://example.test/product.webp","fc":true},
        {"k":5,"p":0,"r":"paragraph","t":"Product description","fc":true},
        {"k":6,"p":0,"r":"paragraph","t":"Nearby evidence","fc":true},
        {"k":7,"p":0,"r":"paragraph","t":"Personal value","q":"sensitive","fc":true},
        {"k":8,"p":0,"r":"paragraph","t":"Unrelated footer","fc":true}
    ]);
    nodes.as_array_mut().unwrap().extend(extra.iter().cloned());
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(1).unwrap(),
            previous.frames()[0].frame().clone(),
            SemanticSnapshotGeneration::new(1).unwrap(),
        ),
        &serde_json::to_vec(
            &json!({"v":SEMANTIC_WIRE_VERSION,"i":1,"g":1,"c":"complete","n":nodes}),
        )
        .unwrap(),
    )
    .unwrap();
    let observation = SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(1).unwrap(),
            call.account().context(),
            SemanticObservationBudget::try_new(16, 8192, 1).unwrap(),
        ),
        snapshot,
    )
    .unwrap()
    .finish()
    .unwrap();
    let mut fields = vec![
        SemanticExtractionFieldSchema::try_text("name".into(), true, 512)
            .unwrap()
            .with_verbatim_text()
            .unwrap(),
        SemanticExtractionFieldSchema::try_text("price".into(), price_required, price_bytes)
            .unwrap()
            .with_verbatim_text()
            .unwrap(),
        SemanticExtractionFieldSchema::try_image_url("picture".into(), true, 1024).unwrap(),
    ];
    if generated {
        fields.push(SemanticExtractionFieldSchema::try_text("summary".into(), true, 512).unwrap());
    }
    let schema = SemanticExtractionSchema::try_new(
        SemanticExtractionSchemaId::new(1).unwrap(),
        vec![SemanticExtractionFieldSchema::try_rows("output_0".into(), true, fields, 1).unwrap()],
    )
    .unwrap();
    let objective = AgentProviderObjective::try_admit_conservative_utf8(
        "Compare three product pages; read this page's columns".into(),
        &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
    )
    .unwrap();
    let authority = AgentProviderActionAuthority::try_new(&observation, &[]).unwrap();
    let projection = DecisionObservation::try_for_read(
        &observation,
        &objective,
        &authority,
        call.account(),
        Some(&schema),
    )
    .unwrap();
    (call, observation, schema, projection)
}

fn located_answers(projection: &DecisionObservation) -> Value {
    let mut output = fixture_answers(projection.request());
    output["answers"]["done"]["noul"] = json!(0.99);
    for (index, target) in ["@a2", "@a3", "@a4", "@a5"].into_iter().enumerate() {
        let key = format!("locate_{index}");
        let Some(Question::Choice { criteria, .. }) = projection.request().questions().get(&key)
        else {
            continue;
        };
        let probabilities: BTreeMap<_, _> = criteria
            .keys()
            .map(|key| (key, if key == target { 1.0 } else { 0.0 }))
            .collect();
        output["answers"][&key] =
            json!({"type":"choice","choice":target,"confidence":1.0,"probabilities":probabilities});
    }
    output
}

#[test]
fn an_uncertain_read_head_never_reaches_emulation_but_an_unanswered_one_still_does() {
    let (_, _, _, projection) = located_fixture(false);
    let mut output = located_answers(&projection);
    output["answers"]["locate_1"]["confidence"] = json!(0.1);
    output["answers"]["operation"]["confidence"] = json!(0.2);
    output["answers"]["done"]["noul"] = json!(0.5);
    let response = projection
        .request()
        .decode_emulation(
            &serde_json::to_vec(&output).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    // Every uncertain head is dropped: no emulated batch is built at all.
    assert!(projection.route(Ok(response)).unwrap().projection().is_none());

    let (_, _, _, projection) = located_fixture(false);
    let asked = projection.question_count();
    let unanswered = projection
        .route(Err(super::DecisionCallFailure::Unavailable))
        .unwrap();
    let emulated = unanswered.projection().expect("unanswered heads emulate");
    assert_eq!(emulated.question_count(), asked);
}

#[test]
fn a_settled_value_batch_completes_the_read_without_the_completion_head() {
    for (done, ready) in [(0.99, true), (0.01, false)] {
        let (call, observation, schema, projection) = located_fixture(false);
        let mut output = located_answers(&projection);
        output["answers"]["done"]["noul"] = json!(done);
        let response = projection
            .request()
            .decode_emulation(
                &serde_json::to_vec(&output).unwrap(),
                DecisionUsage::default(),
            )
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        assert_eq!(
            answers
                .take_read_selection(&observation, call.account(), &schema)
                .unwrap()
                .is_some(),
            ready,
            "an accepted completion head decides"
        );
    }
    // An unsettled completion head leaves Rust's own structural check in charge.
    let (call, observation, schema, projection) = located_fixture(false);
    let mut output = located_answers(&projection);
    output["answers"]["done"]["noul"] = json!(0.5);
    let response = projection
        .request()
        .decode_emulation(
            &serde_json::to_vec(&output).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    let mut answers = projection.route(Ok(response)).unwrap().finish(None);
    assert!(answers
        .take_read_selection(&observation, call.account(), &schema)
        .unwrap()
        .is_some());
}

#[test]
fn located_read_copies_exact_sources_and_discards_unused_speculative_fallback() {
    let (call, observation, schema, projection) = located_fixture(false);
    let mut output = located_answers(&projection);
    output["answers"]["operation"]["confidence"] = json!(0.2);
    output["answers"]["relevant"]["noul"] = json!(0.5);
    let response = projection
        .request()
        .decode_emulation(
            &serde_json::to_vec(&output).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    let fallback = projection.route(Ok(response)).unwrap();
    assert!(fallback.projection().is_none());
    let mut answers = fallback.finish(None);
    let selection = answers
        .take_read_selection(&observation, call.account(), &schema)
        .unwrap()
        .unwrap();
    assert!(answers
        .take_read_selection(&observation, call.account(), &schema)
        .unwrap()
        .is_none());
    let located = selection
        .prepare(
            &observation,
            call.account(),
            SemanticCaptureInstant::from_millis(101),
            None,
        )
        .unwrap();
    assert!(located.generation().is_none());
    let result = located.finish(None).unwrap();
    let SemanticExtractedValue::Rows(rows) = result.fields()[0].value() else {
        panic!()
    };
    assert!(
        matches!(rows.items()[0].fields()[1].value(), SemanticExtractedValue::Text(text) if text.as_str() == "$349.99")
    );
    assert!(
        matches!(rows.items()[0].fields()[2].value(), SemanticExtractedValue::ImageUrl(text) if text.as_str() == "https://example.test/product.webp")
    );
    assert!(result
        .read_omissions()
        .contains(SemanticReadOmission::ReferenceSelection));
    assert_eq!(result.stats().values(), 3);
}

#[test]
fn a_located_copy_longer_than_its_column_is_unknown_when_optional_and_refused_when_required() {
    for required in [false, true] {
        let (call, observation, schema, projection) = located_fixture_priced(false, &[], required, 4);
        let response = projection
            .request()
            .decode_emulation(
                &serde_json::to_vec(&located_answers(&projection)).unwrap(),
                DecisionUsage::default(),
            )
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        let selection = answers
            .take_read_selection(&observation, call.account(), &schema)
            .unwrap()
            .unwrap();
        let prepared = selection.prepare(
            &observation,
            call.account(),
            SemanticCaptureInstant::from_millis(101),
            None,
        );
        if required {
            assert!(matches!(prepared, Err(SemanticExtractionError::TextLimit)));
            continue;
        }
        let result = prepared.unwrap().finish(None).unwrap();
        let SemanticExtractedValue::Rows(rows) = result.fields()[0].value() else {
            panic!()
        };
        let names: Vec<_> = rows.items()[0].fields().iter().map(|field| field.name()).collect();
        assert_eq!(names, ["name", "picture"]);
    }
}

#[test]
fn located_generation_is_focused_and_merges_only_exact_delivered_evidence() {
    let (call, observation, schema, projection) = located_fixture(true);
    let response = projection
        .request()
        .decode_emulation(
            &serde_json::to_vec(&located_answers(&projection)).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    let mut answers = projection.route(Ok(response)).unwrap().finish(None);
    let located = answers
        .take_read_selection(&observation, call.account(), &schema)
        .unwrap()
        .unwrap()
        .prepare(
            &observation,
            call.account(),
            SemanticCaptureInstant::from_millis(101),
            None,
        )
        .unwrap();
    let (generation_schema, read) = located.generation().unwrap();
    assert_eq!(generation_schema.fields().len(), 1);
    assert_eq!(generation_schema.fields()[0].name(), "summary");
    let encoded = encode_semantic_extraction_request(
        generation_schema,
        read,
        SemanticModelEncodingBudget::EXTRACTION_PROVIDER_EXACT_CONSERVATIVE,
    )
    .unwrap()
    .admit_conservative_utf8(&SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap())
    .unwrap();
    for excluded in [
        "Unrelated footer",
        "Personal value",
        "product.webp",
        "Public product",
    ] {
        assert!(!encoded.as_str().contains(excluded));
    }
    let source = read
        .fragments()
        .iter()
        .find(|fragment| {
            fragment.provenance().reference() == SemanticReferenceId::parse("@a5").unwrap()
        })
        .unwrap();
    let output = serde_json::to_vec(&json!({"v":1,"schema":1,"fields":[{"name":"summary","value":{"k":"text","value":"A product description","sources":[source.id().model_token().to_string()]}}]})).unwrap();
    let (_, _, delivery) = encoded.into_provider_parts();
    let generated = extract_delivered_semantic_read(
        generation_schema,
        read,
        &delivery.commit(),
        SemanticReadSensitivityLimit::PublicOnly,
        &output,
    )
    .unwrap();
    let result = located.finish(Some(generated)).unwrap();
    let SemanticExtractedValue::Rows(rows) = result.fields()[0].value() else {
        panic!()
    };
    assert_eq!(rows.items()[0].fields().len(), 4);
    let SemanticExtractedValue::Text(summary) = rows.items()[0].fields()[3].value() else {
        panic!()
    };
    let source = result.sources(summary.source_span()).unwrap()[0].fragment();
    assert_eq!(
        source.content().text().unwrap().as_str(),
        "Product description"
    );
}

#[test]
fn a_whole_page_findings_read_generates_from_ranked_evidence_or_leaves_the_planner() {
    let (call, observation, _, _) = located_fixture(false);
    let schema = SemanticExtractionSchema::try_new(
        SemanticExtractionSchemaId::new(1).unwrap(),
        vec![SemanticExtractionFieldSchema::try_text_list("output_0".into(), true, 16, 1024).unwrap()],
    )
    .unwrap();
    assert!(schema.is_whole_page_findings());
    let objective = AgentProviderObjective::try_admit_conservative_utf8(
        "List what this page says about the product".into(),
        &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
    )
    .unwrap();
    for relevant in [0.9, 0.1] {
        let projection = DecisionObservation::try_for_read(
            &observation,
            &objective,
            &AgentProviderActionAuthority::try_new(&observation, &[]).unwrap(),
            call.account(),
            Some(&schema),
        )
        .unwrap();
        assert!(!projection.request().questions().contains_key("locate_0"));
        let mut body = fixture_answers(projection.request());
        body["answers"]["any_0"] = json!({"type":"noul","noul":relevant});
        let Question::Choice { criteria, .. } = &projection.request().questions()["find_0"] else {
            panic!("choice expected");
        };
        let probabilities: BTreeMap<_, _> = criteria
            .keys()
            .map(|key| {
                let probability = match key.as_str() {
                    "@a5" => 0.5,
                    "@a6" => 0.3,
                    "@a8" => 0.03,
                    "none" => 0.17,
                    _ => 0.0,
                };
                (key.clone(), probability)
            })
            .collect();
        body["answers"]["find_0"] = json!({"type":"choice","choice":"@a5","confidence":0.5,"probabilities":probabilities});
        let response = projection
            .request()
            .decode_emulation(&serde_json::to_vec(&body).unwrap(), DecisionUsage::default())
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        let mut evidence = SemanticRetainedReadEvidence::default();
        let (selection, ready) = answers
            .take_read_progress_retaining_evidence(
                &observation,
                call.account(),
                &schema,
                SemanticCaptureInstant::from_millis(101),
                &mut evidence,
            )
            .unwrap()
            .unwrap();
        if relevant < 0.5 {
            // Nothing located: the page planner reads this page.
            assert!(!ready && selection.located() == 0);
            continue;
        }
        assert!(ready && selection.located() == 2);
        let located = selection
            .prepare(&observation, call.account(), SemanticCaptureInstant::from_millis(101), None)
            .unwrap();
        let (generation_schema, read) = located.generation().unwrap();
        assert_eq!(generation_schema.fields()[0].name(), "output_0");
        let texts: Vec<_> = read
            .fragments()
            .iter()
            .filter_map(|fragment| fragment.content().text().map(|text| text.as_str().to_owned()))
            .collect();
        assert!(texts.iter().any(|text| text == "Product description"));
        assert!(texts.iter().any(|text| text == "Nearby evidence"));
        assert!(!texts.iter().any(|text| text == "Unrelated footer" || text == "$349.99"));
    }
}

#[test]
fn located_form_values_copy_complete_public_values_without_substituting_labels() {
    for (name, value, sensitivity, offered, truncated) in [
        (None, "Warsaw".to_owned(), "public", true, false),
        (
            Some("Destination"),
            "Warsaw".to_owned(),
            "public",
            true,
            false,
        ),
        (
            Some("Destination"),
            "x".repeat(MAX_SEMANTIC_VALUE_PREVIEW_BYTES + 1),
            "public",
            true,
            true,
        ),
        (
            None,
            "Private location".to_owned(),
            "sensitive",
            false,
            false,
        ),
        (None, "Private location".to_owned(), "secret", false, false),
    ] {
        let (_, call, previous, _) = admitted_fixture();
        let mut node = json!({"k":2,"p":0,"r":"textbox","v":{"k":"text","value":value},"q":sensitivity,"fc":true});
        if let Some(name) = name {
            node["n"] = json!(name);
        }
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(SemanticInvocationId::new(1).unwrap(), previous.frames()[0].frame().clone(), SemanticSnapshotGeneration::new(1).unwrap()),
            &serde_json::to_vec(&json!({"v":SEMANTIC_WIRE_VERSION,"i":1,"g":1,"c":"complete","n":[{"k":1,"r":"document","fc":true},node,{"k":3,"p":0,"r":"heading","l":1,"n":"Travel search","fc":true}]})).unwrap(),
        ).unwrap();
        let observation = SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(1).unwrap(),
                call.account().context(),
                SemanticObservationBudget::try_new(16, 8192, 1).unwrap(),
            ),
            snapshot,
        )
        .unwrap()
        .finish()
        .unwrap();
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(1).unwrap(),
            vec![
                SemanticExtractionFieldSchema::try_text("destination".into(), true, 2048)
                    .unwrap()
                    .with_verbatim_text()
                    .unwrap(),
            ],
        )
        .unwrap();
        let objective = AgentProviderObjective::try_admit_conservative_utf8(
            "Read the public destination".into(),
            &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
        )
        .unwrap();
        let authority = AgentProviderActionAuthority::try_new(&observation, &[]).unwrap();
        let projection = DecisionObservation::try_for_read(
            &observation,
            &objective,
            &authority,
            call.account(),
            Some(&schema),
        )
        .unwrap();
        let Question::Choice { criteria, .. } = &projection.request().questions()["locate_0"]
        else {
            panic!()
        };
        assert_eq!(criteria.contains_key("@a2"), offered);
        if !offered {
            assert!(!projection
                .request()
                .state()
                .to_string()
                .contains("Private location"));
            continue;
        }
        let response = projection
            .request()
            .decode_emulation(
                &serde_json::to_vec(&located_answers(&projection)).unwrap(),
                DecisionUsage::default(),
            )
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        let selection = answers
            .take_read_selection(&observation, call.account(), &schema)
            .unwrap()
            .unwrap();
        let located = selection.prepare(
            &observation,
            call.account(),
            SemanticCaptureInstant::from_millis(101),
            None,
        );
        if truncated {
            assert!(matches!(
                located,
                Err(SemanticExtractionError::VerbatimMismatch)
            ));
        } else {
            let result = located.unwrap().finish(None).unwrap();
            assert!(
                matches!(result.fields()[0].value(), SemanticExtractedValue::Text(text) if text.as_str() == value)
            );
        }
    }
}

#[test]
fn incomplete_reads_retain_only_confident_located_evidence_for_later_mapping() {
    let (call, observation, schema, projection) = located_fixture(false);
    let mut output = fixture_answers(projection.request());
    output["answers"]["locate_1"] = located_answers(&projection)["answers"]["locate_1"].clone();
    let response = projection
        .request()
        .decode_emulation(
            &serde_json::to_vec(&output).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    let mut answers = projection.route(Ok(response)).unwrap().finish(None);
    let mut retained = SemanticRetainedReadEvidence::default();
    assert!(answers
        .take_read_selection_retaining_evidence(
            &observation,
            call.account(),
            &schema,
            SemanticCaptureInstant::from_millis(101),
            &mut retained,
        )
        .unwrap()
        .is_none());
    // The located price, and the name the page's own heading supplies.
    assert_eq!(retained.retained_items(), 4);
    assert_eq!(retained.retained_bytes(), 40);
    assert!(answers
        .take_read_selection(&observation, call.account(), &schema)
        .unwrap()
        .is_none());

    let mut later_nodes = vec![json!({"k":1,"r":"document","n":"New section","fc":true})];
    later_nodes.extend((2..=128).map(|key| json!({
        "k":key + 100,"p":0,"r":"paragraph","t":format!("Unrelated current fact {key}"),"fc":true,
    })));
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(2).unwrap(),
            observation.frames()[0].frame().clone(),
            SemanticSnapshotGeneration::new(2).unwrap(),
        ),
        &serde_json::to_vec(
            &json!({"v":SEMANTIC_WIRE_VERSION,"i":2,"g":2,"c":"complete","n":later_nodes}),
        )
        .unwrap(),
    )
    .unwrap();
    let later = SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(2).unwrap(),
            call.account().context(),
            SemanticObservationBudget::try_new(128, 32768, 1).unwrap(),
        ),
        snapshot,
    )
    .unwrap()
    .finish()
    .unwrap();
    let ack = SemanticObservationAcknowledgement::from_fingerprint(
        crate::semantic_diff::SemanticObservationFingerprint::from_observation(&later),
    );
    let current = read_semantic_observation_for_schema(
        &later,
        SemanticReadAuthority::Acknowledged(&ack),
        SemanticCaptureInstant::from_millis(102),
        SemanticReadSensitivityLimit::PublicOnly,
        SemanticReadBudget::STANDARD,
        &schema,
    )
    .unwrap();
    assert_eq!(current.fragments().len(), 128);
    let merged = retained.merge_for_extraction(current).unwrap();
    assert_eq!(merged.fragments().len(), 128);
    assert!(merged.fragments().iter().any(|fragment| fragment
        .content()
        .text()
        .is_some_and(|text| text.as_str() == "$349.99")
        && fragment.provenance().observation().get() == 1));
}

#[test]
fn optional_values_still_need_inspection_before_a_typed_read_can_finish() {
    for absent in [1, 2] {
        let (call, observation, _, _) = located_fixture(false);
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(1).unwrap(),
            vec![
                SemanticExtractionFieldSchema::try_text("name".into(), true, 512)
                    .unwrap()
                    .with_verbatim_text()
                    .unwrap(),
                SemanticExtractionFieldSchema::try_text("price".into(), false, 512)
                    .unwrap()
                    .with_verbatim_text()
                    .unwrap(),
                SemanticExtractionFieldSchema::try_image_url("picture".into(), false, 1024)
                    .unwrap(),
            ],
        )
        .unwrap();
        let objective = AgentProviderObjective::try_admit_conservative_utf8(
            "Read the displayed product price and picture".into(),
            &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
        )
        .unwrap();
        let authority = AgentProviderActionAuthority::try_new(&observation, &[]).unwrap();
        let projection = DecisionObservation::try_for_read(
            &observation,
            &objective,
            &authority,
            call.account(),
            Some(&schema),
        )
        .unwrap();
        let mut output = located_answers(&projection);
        let key = format!("locate_{absent}");
        output["answers"][&key] = fixture_answers(projection.request())["answers"][&key].clone();
        let response = projection
            .request()
            .decode_emulation(
                &serde_json::to_vec(&output).unwrap(),
                DecisionUsage::default(),
            )
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        assert_eq!(
            answers
                .take_read_selection(&observation, call.account(), &schema)
                .unwrap()
                .is_some(),
            absent == 2
        );
    }
}

#[test]
fn an_optional_value_absent_on_a_confirming_look_publishes_unknown_but_a_required_one_never_does() {
    for required in [false, true] {
        let (call, observation, _, _) = located_fixture(false);
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(1).unwrap(),
            vec![
                SemanticExtractionFieldSchema::try_text("name".into(), true, 512)
                    .unwrap()
                    .with_verbatim_text()
                    .unwrap(),
                SemanticExtractionFieldSchema::try_text("monthly_total".into(), required, 512)
                    .unwrap()
                    .with_verbatim_text()
                    .unwrap(),
            ],
        )
        .unwrap();
        let objective = AgentProviderObjective::try_admit_conservative_utf8(
            "Read the listing name and its displayed monthly total".into(),
            &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
        )
        .unwrap();
        let authority = AgentProviderActionAuthority::try_new(&observation, &[]).unwrap();
        let mut earlier = None;
        let mut evidence = SemanticRetainedReadEvidence::default();
        for look in 0..2 {
            let projection = DecisionObservation::try_for_read(
                &observation,
                &objective,
                &authority,
                call.account(),
                Some(&schema),
            )
            .unwrap();
            let mut output = located_answers(&projection);
            output["answers"]["done"]["noul"] = json!(0.01);
            output["answers"]["locate_1"] =
                fixture_answers(projection.request())["answers"]["locate_1"].clone();
            let response = projection
                .request()
                .decode_emulation(
                    &serde_json::to_vec(&output).unwrap(),
                    DecisionUsage::default(),
                )
                .unwrap();
            let mut answers = projection.route(Ok(response)).unwrap().finish(None);
            let (selection, ready) = answers
                .take_read_progress_retaining_evidence(
                    &observation,
                    call.account(),
                    &schema,
                    SemanticCaptureInstant::from_millis(101),
                    &mut evidence,
                )
                .unwrap()
                .unwrap();
            assert!(!ready);
            assert_eq!(selection.awaits_absence(), !required, "look {look}");
            let Some(first) = earlier.take() else {
                earlier = Some(selection);
                continue;
            };
            assert!(selection.confirms_absence(&first));
            if !required {
                let result = first
                    .prepare_confirmed(
                        &observation,
                        call.account(),
                        SemanticCaptureInstant::from_millis(101),
                        None,
                        &evidence,
                    )
                    .unwrap()
                    .finish(None)
                    .unwrap();
                let names: Vec<_> = result.fields().iter().map(|field| field.name()).collect();
                assert_eq!(names, ["name"]);
            }
        }
    }
}

#[test]
fn an_own_address_column_cites_only_the_admitted_document_address() {
    let (call, observation, _, _) = located_fixture(false);
    let schema = |document_address: bool| {
        let url = SemanticExtractionFieldSchema::try_url("listing_url".into(), true, 2048).unwrap();
        SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(1).unwrap(),
            vec![
                SemanticExtractionFieldSchema::try_text("name".into(), true, 512)
                    .unwrap()
                    .with_verbatim_text()
                    .unwrap(),
                if document_address {
                    url.with_document_address().unwrap()
                } else {
                    url
                },
            ],
        )
        .unwrap()
    };
    assert!(SemanticExtractionFieldSchema::try_text("name".into(), true, 8)
        .unwrap()
        .with_document_address()
        .is_err());
    let objective = AgentProviderObjective::try_admit_conservative_utf8(
        "Read the listing name and its address".into(),
        &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
    )
    .unwrap();
    let authority = AgentProviderActionAuthority::try_new(&observation, &[]).unwrap();
    let own = ContextNavigationTarget::parse("https://example.test/rooms/42").unwrap();
    let foreign = ContextNavigationTarget::parse("https://elsewhere.test/rooms/42").unwrap();
    let run = |schema: &SemanticExtractionSchema, document: Option<&ContextNavigationTarget>| {
        let projection = DecisionObservation::try_for_read(
            &observation,
            &objective,
            &authority,
            call.account(),
            Some(schema),
        )
        .unwrap();
        let asked = projection.request().questions().contains_key("locate_1");
        let mut output = located_answers(&projection);
        output["answers"]["done"]["noul"] = json!(0.5);
        let response = projection
            .request()
            .decode_emulation(
                &serde_json::to_vec(&output).unwrap(),
                DecisionUsage::default(),
            )
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        let (selection, ready) = answers
            .take_read_progress_retaining_evidence(
                &observation,
                call.account(),
                schema,
                SemanticCaptureInstant::from_millis(101),
                &mut SemanticRetainedReadEvidence::default(),
            )
            .unwrap()
            .unwrap();
        let settled = ready;
        let result = selection
            .prepare(
                &observation,
                call.account(),
                SemanticCaptureInstant::from_millis(101),
                document,
            )
            .and_then(|located| located.finish(None));
        (asked, settled, result.map(|result| {
            let SemanticExtractedValue::Url(url) = result.fields()[1].value() else {
                panic!("url");
            };
            url.as_str().to_owned()
        }))
    };
    // No locate head is asked; the read finishes with the admitted address.
    let (asked, settled, value) = run(&schema(true), Some(&own));
    assert!(!asked && settled);
    assert_eq!(value.unwrap(), "https://example.test/rooms/42");
    // Without an admitted address, or with one off the page's origin, a
    // required own-address column is not answered at all.
    assert!(run(&schema(true), None).2.is_err());
    assert!(run(&schema(true), Some(&foreign)).2.is_err());
    // An ordinary url column never cites the document address.
    let ack = SemanticObservationAcknowledgement::from_fingerprint(
        crate::semantic_diff::SemanticObservationFingerprint::from_observation(&observation),
    );
    for (document_address, admitted) in [(true, true), (false, false)] {
        let schema = schema(document_address);
        let read = crate::semantic_read::read_located_semantic_observation_at(
            &observation,
            &ack,
            SemanticCaptureInstant::from_millis(101),
            &schema,
            &std::collections::BTreeSet::new(),
            Some(&own),
        )
        .unwrap();
        let [fragment] = read.fragments() else {
            panic!("one document address fragment");
        };
        assert_eq!(fragment.field(), SemanticReadField::DocumentAddress);
        let output = json!({"v":1,"schema":1,"fields":[
            {"name":"listing_url","value":{"k":"url","sources":[fragment.id().model_token()]}}
        ]});
        let listing = crate::semantic_extract::extract_semantic_read_inner(
            &SemanticExtractionSchema::try_new(
                SemanticExtractionSchemaId::new(1).unwrap(),
                vec![schema.fields()[1].clone()],
            )
            .unwrap(),
            &read,
            SemanticReadSensitivityLimit::PublicOnly,
            &serde_json::to_vec(&output).unwrap(),
        );
        assert_eq!(listing.is_ok(), admitted);
    }
}

#[test]
fn document_metadata_nodes_are_never_name_text_or_evidence_candidates() {
    let metadata = [
        json!({"k":9,"p":0,"r":"link","n":"Page address","u":"https://example.test/rooms/42","fc":true}),
        json!({"k":10,"p":0,"r":"image","n":"Page image","m":"https://example.test/og.jpg","fc":true}),
    ];
    let (call, observation, _, own_page) = located_fixture_with(true, &metadata);
    let offered = |projection: &DecisionObservation, key: &str| {
        let Question::Choice { criteria, .. } = &projection.request().questions()[key] else {
            panic!("choice expected");
        };
        ["@a9", "@a10"].map(|token| criteria.contains_key(token))
    };
    // name, price (verbatim) and summary (generated) never see either node;
    // the picture head keeps the page image.
    for key in ["locate_0", "locate_1", "locate_3"] {
        assert_eq!(offered(&own_page, key), [false, false], "{key}");
    }
    assert_eq!(offered(&own_page, "locate_2"), [false, true]);
    let objective = AgentProviderObjective::try_admit_conservative_utf8(
        "Collect the catalog's products".into(),
        &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
    )
    .unwrap();
    let authority = AgentProviderActionAuthority::try_new(&observation, &[]).unwrap();
    let catalog = SemanticExtractionSchema::try_new(
        SemanticExtractionSchemaId::new(1).unwrap(),
        vec![
            SemanticExtractionFieldSchema::try_text("name".into(), true, 512).unwrap(),
            SemanticExtractionFieldSchema::try_url("product_url".into(), false, 2048).unwrap(),
            SemanticExtractionFieldSchema::try_image_url("image".into(), false, 2048).unwrap(),
        ],
    )
    .unwrap();
    let catalog = DecisionObservation::try_for_read(
        &observation,
        &objective,
        &authority,
        call.account(),
        Some(&catalog),
    )
    .unwrap();
    assert_eq!(offered(&catalog, "locate_0"), [false, false]);
    assert_eq!(offered(&catalog, "locate_1"), [true, false]);
    let findings = SemanticExtractionSchema::try_new(
        SemanticExtractionSchemaId::new(1).unwrap(),
        vec![SemanticExtractionFieldSchema::try_text_list("output_0".into(), true, 16, 1024).unwrap()],
    )
    .unwrap();
    let findings = DecisionObservation::try_for_read(
        &observation,
        &objective,
        &authority,
        call.account(),
        Some(&findings),
    )
    .unwrap();
    let chunks = findings
        .request()
        .questions()
        .keys()
        .filter(|key| key.starts_with("find_"))
        .count();
    assert!(chunks > 0);
    for index in 0..chunks {
        assert_eq!(offered(&findings, &format!("find_{index}")), [false, false]);
    }
}

const TITLE: &str = "Tower Bridge 21067 | Architecture | Buy online at the Official LEGO® Shop US";

/// The recorded LEGO Tower Bridge product page (lego_product_01): its
/// set-number line, level-1 heading and price, with the page's metadata.
fn product_page(
    heading: Option<&str>,
    title: bool,
) -> (AgentModelCallRequest, SemanticObservation) {
    let (_, call, previous, _) = admitted_fixture();
    let mut nodes = vec![
        json!({"k":1,"r":"document","fc":true}),
        json!({"k":2,"p":0,"r":"image","n":"Page image","m":"https://www.lego.com/cdn/21067.png","fc":true}),
        json!({"k":3,"p":0,"r":"link","n":"Page address","u":"https://www.lego.com/en-us/product/tower-bridge-10214","fc":true}),
        json!({"k":4,"p":0,"r":"paragraph","t":"#21067","fc":true}),
        json!({"k":6,"p":0,"r":"paragraph","t":"$349.99","fc":true}),
        json!({"k":7,"p":0,"r":"paragraph","t":"Available now","fc":true}),
    ];
    if title {
        nodes.push(json!({"k":8,"p":0,"r":"paragraph","n":"Page title","t":TITLE,"fc":true}));
    }
    if let Some(heading) = heading {
        nodes.push(json!({"k":5,"p":0,"r":"heading","l":1,"n":heading,"fc":true}));
    }
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(1).unwrap(),
            previous.frames()[0].frame().clone(),
            SemanticSnapshotGeneration::new(1).unwrap(),
        ),
        &serde_json::to_vec(
            &json!({"v":SEMANTIC_WIRE_VERSION,"i":1,"g":1,"c":"complete","n":nodes}),
        )
        .unwrap(),
    )
    .unwrap();
    let observation = SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(1).unwrap(),
            call.account().context(),
            SemanticObservationBudget::try_new(16, 8192, 1).unwrap(),
        ),
        snapshot,
    )
    .unwrap()
    .finish()
    .unwrap();
    (call, observation)
}

/// The name head's offered nodes and the copied name, for one name answer
/// (None abstains) on a single-record read of the product page.
fn own_page_name(
    heading: Option<&str>,
    title: bool,
    answer: Option<&str>,
) -> (Vec<String>, Vec<String>, Option<String>) {
    let (call, observation) = product_page(heading, title);
    let schema = SemanticExtractionSchema::try_new(
        SemanticExtractionSchemaId::new(1).unwrap(),
        vec![SemanticExtractionFieldSchema::try_rows(
            "output_0".into(),
            true,
            vec![
                SemanticExtractionFieldSchema::try_text("name".into(), true, 512)
                    .unwrap()
                    .with_verbatim_text()
                    .unwrap(),
                SemanticExtractionFieldSchema::try_text("price".into(), false, 512)
                    .unwrap()
                    .with_verbatim_text()
                    .unwrap(),
            ],
            1,
        )
        .unwrap()],
    )
    .unwrap();
    let objective = AgentProviderObjective::try_admit_conservative_utf8(
        "Read this product page's name and displayed price".into(),
        &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
    )
    .unwrap();
    let authority = AgentProviderActionAuthority::try_new(&observation, &[]).unwrap();
    let projection = DecisionObservation::try_for_read(
        &observation,
        &objective,
        &authority,
        call.account(),
        Some(&schema),
    )
    .unwrap();
    // Nodes by their visible text, else their name.
    let label = |node: &SemanticNode| {
        node.text()
            .or(node.name())
            .map(|text| text.as_str().to_owned())
            .unwrap_or_default()
    };
    let nodes: Vec<_> = observation.frames()[0].nodes().iter().collect();
    let token = |text: &str| {
        nodes
            .iter()
            .find(|node| label(node) == text)
            .map(|node| node.reference().model_token().to_string())
            .unwrap()
    };
    let offered = |key: &str| {
        let Question::Choice { criteria, .. } = &projection.request().questions()[key] else {
            panic!("choice expected");
        };
        nodes
            .iter()
            .filter(|node| criteria.contains_key(&node.reference().model_token().to_string()))
            .map(|node| label(node))
            .collect::<Vec<_>>()
    };
    let (name_offered, price_offered) = (offered("locate_0"), offered("locate_1"));
    let mut output = fixture_answers(projection.request());
    output["answers"]["done"]["noul"] = json!(0.99);
    for (key, target) in [("locate_0", answer), ("locate_1", Some("$349.99"))] {
        let Some(target) = target.map(token) else {
            continue;
        };
        let target = target.as_str();
        let Question::Choice { criteria, .. } = &projection.request().questions()[key] else {
            panic!("choice expected");
        };
        let probabilities: BTreeMap<_, _> = criteria
            .keys()
            .map(|key| (key, if key == target { 1.0 } else { 0.0 }))
            .collect();
        output["answers"][key] =
            json!({"type":"choice","choice":target,"confidence":1.0,"probabilities":probabilities});
    }
    let response = projection
        .request()
        .decode_emulation(
            &serde_json::to_vec(&output).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    let mut answers = projection.route(Ok(response)).unwrap().finish(None);
    let (selection, ready) = answers
        .take_read_progress_retaining_evidence(
            &observation,
            call.account(),
            &schema,
            SemanticCaptureInstant::from_millis(101),
            &mut SemanticRetainedReadEvidence::default(),
        )
        .unwrap()
        .unwrap();
    if !ready {
        return (name_offered, price_offered, None);
    }
    let result = selection
        .prepare(
            &observation,
            call.account(),
            SemanticCaptureInstant::from_millis(101),
            None,
        )
        .unwrap()
        .finish(None)
        .unwrap();
    let SemanticExtractedValue::Rows(rows) = result.fields()[0].value() else {
        panic!("rows expected");
    };
    let name = rows.items()[0]
        .fields()
        .iter()
        .find(|field| field.name() == "name")
        .map(|field| {
            let SemanticExtractedValue::Text(text) = field.value() else {
                panic!("text expected");
            };
            text.as_str().to_owned()
        });
    (name_offered, price_offered, name)
}

#[test]
fn an_own_page_names_its_subject_from_its_heading_then_its_title() {
    let heading = Some("Tower Bridge");
    // The name head offers only the level-1 heading; price never sees the
    // page's metadata.
    let (offered, price, name) = own_page_name(heading, true, Some("Tower Bridge"));
    assert_eq!(offered, ["Tower Bridge"]);
    assert_eq!(
        price,
        ["#21067", "$349.99", "Available now", "Tower Bridge"]
    );
    assert_eq!(name.as_deref(), Some("Tower Bridge"));
    // An abstaining name head takes the heading.
    assert_eq!(
        own_page_name(heading, true, None).2.as_deref(),
        Some("Tower Bridge")
    );
    // Without a heading the page title is offered and taken.
    let (offered, _, name) = own_page_name(None, true, None);
    assert_eq!(offered, [TITLE]);
    assert_eq!(name.as_deref(), Some(TITLE));
    // A heading that is only the set number yields to the title.
    let (_, _, name) = own_page_name(Some("21067"), true, Some("21067"));
    assert_eq!(name.as_deref(), Some(TITLE));
    // With neither, ordinary text is offered; the set number stands only then.
    let (offered, _, name) = own_page_name(None, false, Some("#21067"));
    assert_eq!(offered, ["#21067", "$349.99", "Available now"]);
    assert_eq!(name.as_deref(), Some("#21067"));
    // And an abstaining head on such a page leaves the name missing.
    assert_eq!(own_page_name(None, false, None).2, None);
}

#[test]
fn an_own_page_read_cites_its_canonical_address_and_page_image() {
    let canonical = json!({"k":9,"p":0,"r":"link","n":"Page address","u":"https://example.test/rooms/42","fc":true});
    let page_image = json!({"k":10,"p":0,"r":"image","n":"Page image","m":"https://example.test/og.jpg","fc":true});
    let schema = SemanticExtractionSchema::try_new(
        SemanticExtractionSchemaId::new(1).unwrap(),
        vec![
            SemanticExtractionFieldSchema::try_text("name".into(), true, 512)
                .unwrap()
                .with_verbatim_text()
                .unwrap(),
            SemanticExtractionFieldSchema::try_url("listing_url".into(), true, 2048)
                .unwrap()
                .with_document_address()
                .unwrap(),
            SemanticExtractionFieldSchema::try_image_url("picture".into(), false, 2048).unwrap(),
        ],
    )
    .unwrap();
    let admitted = ContextNavigationTarget::parse(
        "https://example.test/rooms/42?adults=1&source_impression_id=p3_x&utm_source=openai",
    )
    .unwrap();
    let untracked = untracked_document_address(&admitted);
    assert_eq!(untracked.as_url().as_str(), "https://example.test/rooms/42?adults=1");
    let read = |extra: &[Value]| {
        let (call, observation, _, _) = located_fixture_with(false, extra);
        let own_image = !extra.is_empty();
        let objective = AgentProviderObjective::try_admit_conservative_utf8(
            "Read this listing".into(),
            &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
        )
        .unwrap();
        let authority = AgentProviderActionAuthority::try_new(&observation, &[]).unwrap();
        let projection = DecisionObservation::try_for_read(
            &observation,
            &objective,
            &authority,
            call.account(),
            Some(&schema),
        )
        .unwrap();
        let mut output = located_answers(&projection);
        if own_image {
            // An unsettled completion head and an uncertain picture head.
            output["answers"]["done"]["noul"] = json!(0.5);
            output["answers"]["locate_2"]["confidence"] = json!(0.3);
        } else {
            output["answers"]["locate_2"] =
                fixture_answers(projection.request())["answers"]["locate_2"].clone();
        }
        let response = projection
            .request()
            .decode_emulation(
                &serde_json::to_vec(&output).unwrap(),
                DecisionUsage::default(),
            )
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        let (selection, ready) = answers
            .take_read_progress_retaining_evidence(
                &observation,
                call.account(),
                &schema,
                SemanticCaptureInstant::from_millis(101),
                &mut SemanticRetainedReadEvidence::default(),
            )
            .unwrap()
            .unwrap();
        assert!(ready);
        let result = selection
            .prepare(
                &observation,
                call.account(),
                SemanticCaptureInstant::from_millis(101),
                Some(&untracked),
            )
            .unwrap()
            .finish(None)
            .unwrap();
        result
            .fields()
            .iter()
            .map(|field| match field.value() {
                SemanticExtractedValue::Url(value) | SemanticExtractedValue::ImageUrl(value) => {
                    (field.name().to_owned(), value.as_str().to_owned())
                }
                _ => (field.name().to_owned(), String::new()),
            })
            .collect::<Vec<_>>()
    };
    // Canonical metadata and the page image answer the page's own columns.
    let fields = read(&[canonical, page_image]);
    assert!(fields.contains(&("listing_url".into(), "https://example.test/rooms/42".into())));
    assert!(fields.contains(&("picture".into(), "https://example.test/og.jpg".into())));
    // Without them: the untracked admitted address, and no picture.
    let fields = read(&[]);
    assert!(fields.contains(&(
        "listing_url".into(),
        "https://example.test/rooms/42?adults=1".into()
    )));
    assert!(!fields.iter().any(|(name, _)| name == "picture"));
}

#[test]
fn read_completion_evals_match_the_production_questions_for_optional_columns() {
    for (required, fixture) in [
        (
            true,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../zephium-decision/evals/yc_read_01.json"
            )),
        ),
        (
            false,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../zephium-decision/evals/yc_read_optional_01.json"
            )),
        ),
    ] {
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(1).unwrap(),
            vec![
                SemanticExtractionFieldSchema::try_text("program_duration".into(), true, 1024)
                    .unwrap()
                    .with_verbatim_text()
                    .unwrap(),
                SemanticExtractionFieldSchema::try_text("office_hours".into(), required, 1024)
                    .unwrap(),
            ],
        )
        .unwrap();
        let read = super::read::ReadProjection::for_schema(&schema).unwrap();
        let fixture: Value = serde_json::from_str(fixture).unwrap();
        assert_eq!(
            serde_json::to_value(read.completion_question()).unwrap(),
            fixture["request"]["questions"]["done"]
        );
        let (_, observation, _, _) = located_fixture(false);
        let references = observation
            .frames()
            .iter()
            .flat_map(|frame| frame.nodes())
            .map(SemanticNode::reference)
            .collect();
        let mut questions = BTreeMap::new();
        read.questions(&observation, &references, &mut questions)
            .unwrap();
        for (key, question) in questions {
            assert_eq!(
                serde_json::to_value(question).unwrap()["instructions"],
                fixture["request"]["questions"][&key]["instructions"]
            );
        }
    }
}

#[test]
fn located_read_requires_complete_required_answers_and_exact_schema() {
    for mode in 0..4 {
        let (call, observation, schema, projection) = located_fixture(false);
        let mut output = located_answers(&projection);
        if mode == 0 {
            output["answers"]["done"]["noul"] = json!(0.01);
        }
        if mode == 1 {
            output["answers"]["locate_1"] =
                fixture_answers(projection.request())["answers"]["locate_1"].clone();
        }
        if mode == 2 {
            output["answers"]["locate_1"]["confidence"] = json!(0.1);
        }
        let response = projection
            .request()
            .decode_emulation(
                &serde_json::to_vec(&output).unwrap(),
                DecisionUsage::default(),
            )
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        let schema = if mode == 3 {
            SemanticExtractionSchema::try_new(
                schema.id(),
                vec![SemanticExtractionFieldSchema::try_text("foreign".into(), true, 100).unwrap()],
            )
            .unwrap()
        } else {
            schema
        };
        let result = answers.take_read_selection(&observation, call.account(), &schema);
        if mode == 3 {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_none());
        }
    }
}

#[test]
fn located_generation_requires_committed_decision_taint_and_admits_only_focused_input() {
    let (mut policy, _, _, _) = admitted_fixture();
    let (call, observation, schema, projection) = located_fixture(true);
    let admission = policy
        .prepare_decision_input(call, &observation, &projection, 1000)
        .unwrap();
    let ack = crate::semantic_diff::SemanticObservationAcknowledgement::from_fingerprint(
        crate::semantic_diff::SemanticObservationFingerprint::from_observation(&observation),
    );
    let active = policy.commit_observation_input(admission, &ack).unwrap();
    let prior = AgentProviderInputMetricReceipt::from_decision(
        &active,
        projection.input_stats(),
        projection.request().encode().unwrap().len() as u32,
        1000,
        SemanticTokenCountQuality::Conservative,
    );
    policy
        .settle_model_call(active, AgentModelCallSettlement::Completed, 1000, 100, 1)
        .unwrap();
    let response = projection
        .request()
        .decode_emulation(
            &serde_json::to_vec(&located_answers(&projection)).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    let mut answers = projection.route(Ok(response)).unwrap().finish(None);
    let located = answers
        .take_read_selection(&observation, call.account(), &schema)
        .unwrap()
        .unwrap()
        .prepare(
            &observation,
            call.account(),
            SemanticCaptureInstant::from_millis(101),
            None,
        )
        .unwrap();
    let (schema, read) = located.generation().unwrap();
    let revision = SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap();
    let config = AgentProviderPricingSchedule::try_for_test(
        AgentProviderKind::OpenAiResponses,
        AgentProviderModelRevision::try_new("gpt-fixture".into()).unwrap(),
        AgentProviderReasoningEffort::None,
        revision.clone(),
        AgentProviderPricingProfile::try_new(AgentProviderPricingRevision::new(1).unwrap(), 32768)
            .unwrap(),
        AgentProviderTokenRates::try_new(1, 1, 1, 1).unwrap(),
    )
    .unwrap()
    .try_provider_exact_call_config(1024, AgentProviderStreamBudget::STANDARD)
    .unwrap()
    .restrict_to_extraction();
    let request = AgentModelCallRequest::new(
        AgentModelCallId::new(2).unwrap(),
        call.lease(),
        call.account(),
        AgentModelCallBudget::try_new(32768, 1024, 1000).unwrap(),
        AgentPolicyInstant::from_millis(102),
    );
    let objective = AgentProviderObjective::try_admit_conservative_utf8(
        "Read the located summary".into(),
        &revision,
    )
    .unwrap();
    let draft = || {
        let payload = encode_semantic_extraction_request(
            schema,
            read,
            SemanticModelEncodingBudget::EXTRACTION_PROVIDER_EXACT_CONSERVATIVE,
        )
        .unwrap()
        .admit_conservative_utf8(&revision)
        .unwrap();
        AgentProviderExtractionRequestDraft::try_located(
            &located, prior, request, &config, &objective, payload,
        )
        .unwrap()
    };
    let (mut foreign, _, _, _) = admitted_fixture();
    assert!(matches!(
        draft().try_prepare_for_provider_exact_count(&mut foreign, request, schema, read),
        Err(AgentProviderRequestError::Policy(
            AgentPolicyError::ReadBaselineMissing
        ))
    ));
    assert_eq!(foreign.pending_model_calls(), 0);
    let draft = draft();
    let body: Value = serde_json::from_slice(draft.request().body()).unwrap();
    let text = serde_json::to_string(&body["input"]).unwrap();
    for excluded in [
        "Public product",
        "Unrelated footer",
        "Personal value",
        "product.webp",
    ] {
        assert!(!text.contains(excluded));
    }
    let prepared = draft
        .try_prepare_for_provider_exact_count(&mut policy, request, schema, read)
        .unwrap();
    assert_eq!(policy.pending_model_calls(), 1);
    let (input, _) = prepared.into_transport_parts();
    let _ = input.cancel(&mut policy).unwrap();
    assert_eq!(policy.pending_model_calls(), 0);
}

#[test]
fn consumed_read_can_renew_the_same_account_but_cannot_change_scope_or_reverse_time() {
    for mode in 0..3 {
        let (call, observation, schema, projection) = located_fixture(false);
        let response = projection
            .request()
            .decode_emulation(
                &serde_json::to_vec(&located_answers(&projection)).unwrap(),
                DecisionUsage::default(),
            )
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        let selection = answers
            .take_read_selection(&observation, call.account(), &schema)
            .unwrap()
            .unwrap();
        let account = AgentContextAccountBinding::new(
            AgentAccountAttestationId::from_raw(999),
            call.account().context(),
            if mode == 1 {
                AgentAccountScope::Authenticated(AgentAccountId::from_raw(1000))
            } else {
                AgentAccountScope::Anonymous
            },
            AgentPolicyInstant::from_millis(if mode == 2 { 99 } else { 102 }),
        );
        let prepared = selection.prepare(
            &observation,
            account,
            SemanticCaptureInstant::from_millis(101),
            None,
        );
        assert_eq!(prepared.is_ok(), mode == 0);
    }
}

#[tokio::test]
async fn borrowed_emulation_admits_disclosure_and_conservatively_settles_bad_envelopes() {
    for extra_item in [false, true] {
        let (mut policy, call, observation, projection) = admitted_fixture();
        let mut body = json!({"object":"response","status":"completed","model":"gpt-5.6-terra","service_tier":"default","error":null,"incomplete_details":null,
            "output":[{"type":"reasoning","summary":[]},{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":fixture_answers(projection.request()).to_string()}]}],
            "usage":{"input_tokens":100,"output_tokens":200,"total_tokens":300,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":50}}});
        if extra_item {
            body["output"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"reasoning","summary":[]}));
        }
        let (mut endpoint, server) = server_for_model(
            vec![
                response(
                    200,
                    "",
                    r#"{"object":"response.input_tokens","input_tokens":100}"#,
                ),
                response(200, "", &body.to_string()),
            ],
            "gpt-5.6-terra",
        );
        endpoint.set_path("/v1/responses");
        let transport = AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::STANDARD,
            endpoint.as_str(),
            endpoint.as_str(),
        )
        .unwrap();
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-key".into(),
        )
        .unwrap();
        let config = emulation_config();
        let client = OpenAiDecisionCall::try_new(&transport, &credential, &config).unwrap();
        let call = AgentModelCallRequest::new(
            call.id(),
            call.lease(),
            call.account(),
            client.call_budget().unwrap(),
            AgentPolicyInstant::from_millis(101),
        );
        let server = server();
        let mut accounting = Accounting::default();
        let output = client
            .evaluate_observation(
                &mut policy,
                call,
                &observation,
                &projection,
                Instant::now() + Duration::from_secs(4),
                &AgentProviderCancellation::new(),
                Some(&mut accounting),
            )
            .await
            .unwrap();
        assert_eq!(server.join().unwrap(), 2);
        assert_eq!(accounting.active.len(), 1);
        assert_eq!(accounting.receipts, vec![(output.receipt, output.input)]);
        assert_eq!(
            output
                .input
                .metrics()
                .structured_input_tokens()
                .unwrap()
                .tokens(),
            100
        );
        if extra_item {
            assert!(output.call.response.is_err());
            // Reasoning after the answer is a shape violation, not a count one.
            assert_eq!(
                output.call.diagnostic.envelope_failure,
                Some(DecisionEnvelopeFailure::OutputShape)
            );
            assert_eq!(
                output.receipt.usage_accounting(),
                AgentModelUsageAccounting::ReservationCeiling
            );
            let facts = output.call.diagnostic.rejected_envelope.unwrap();
            assert_eq!(facts.output_items, 3);
            assert_eq!(facts.reasoning_items, 2);
            assert_eq!(facts.message_items, 1);
            assert_eq!(facts.text_items, 1);
            assert_eq!(facts.refusal_items, 0);
            assert_eq!(facts.reported_output_tokens, 200);
            assert_eq!(facts.reported_reasoning_tokens, 50);
            assert_eq!(
                facts.text_bytes,
                fixture_answers(projection.request()).to_string().len()
            );
            assert_eq!(output.receipt.input_tokens(), 8192);
            assert_eq!(output.receipt.output_tokens(), 4096);
            assert_eq!(output.receipt.cost_micro_usd(), 10_000);
        } else {
            assert!(output.call.diagnostic.rejected_envelope.is_none());
            let answers = output.call.response.unwrap();
            assert!(answers.answers.values().all(Result::is_ok));
            assert_eq!(
                output.receipt.usage_accounting(),
                AgentModelUsageAccounting::Exact
            );
            assert_eq!(output.receipt.input_tokens(), 100);
            assert_eq!(output.receipt.output_tokens(), 200);
        }
        assert_eq!(policy.pending_model_calls(), 0);
        assert_eq!(credential.provider(), AgentProviderKind::OpenAiResponses);
        assert!(transport.snapshot().unwrap().is_idle());
    }
}

#[tokio::test]
async fn dropping_dispatched_emulation_retains_receipts_and_seals_its_shared_transport() {
    let hold = Arc::new(AtomicBool::new(true));
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (mut endpoint, server) = server_for_model_with_gate(
        vec![
            response(
                200,
                "",
                r#"{"object":"response.input_tokens","input_tokens":100}"#,
            ),
            response(200, "", "{}"),
        ],
        "gpt-5.6-terra",
        Some((hold.clone(), seen.clone())),
    );
    endpoint.set_path("/v1/responses");
    let transport = AgentProviderTransport::try_new_loopback(
        AgentProviderTransportConfig::STANDARD,
        endpoint.as_str(),
        endpoint.as_str(),
    )
    .unwrap();
    let credential =
        AgentProviderCredential::try_new(AgentProviderKind::OpenAiResponses, "fixture-key".into())
            .unwrap();
    let config = emulation_config();
    let client = OpenAiDecisionCall::try_new(&transport, &credential, &config).unwrap();
    let (mut policy, call, observation, projection) = admitted_fixture();
    let call = AgentModelCallRequest::new(
        call.id(),
        call.lease(),
        call.account(),
        client.call_budget().unwrap(),
        AgentPolicyInstant::from_millis(101),
    );
    let cancellation = AgentProviderCancellation::new();
    let mut accounting = Accounting::default();
    let server = server();
    let mut pending = Box::pin(client.evaluate_observation(
        &mut policy,
        call,
        &observation,
        &projection,
        Instant::now() + Duration::from_secs(5),
        &cancellation,
        Some(&mut accounting),
    ));
    while seen.load(Ordering::SeqCst) != 2 {
        tokio::select! {
            _ = &mut pending => panic!("call ended before generation was held"),
            _ = tokio::time::sleep(Duration::from_millis(2)) => {},
        }
    }
    drop(pending);
    hold.store(false, Ordering::SeqCst);
    assert_eq!(server.join().unwrap(), 2);
    assert_eq!(accounting.active.len(), 1);
    assert_eq!(accounting.receipts.len(), 1);
    let (receipt, input) = accounting.receipts[0];
    assert_eq!(receipt.id(), input.call());
    assert_eq!(
        receipt.usage_accounting(),
        AgentModelUsageAccounting::ReservationCeiling
    );
    assert_eq!(receipt.input_tokens(), 8192);
    assert_eq!(receipt.output_tokens(), 4096);
    assert_eq!(receipt.cost_micro_usd(), 10_000);
    assert_eq!(policy.pending_model_calls(), 0);
    assert!(transport.snapshot().unwrap().is_sealed());
}

#[test]
fn fallback_projection_preserves_binding_and_only_repeats_unanswered_heads() {
    // An answered but uncertain head is never emulated; its question is left
    // to re-observation, a recipe or the planner.
    let (_, call, observation, projection) = admitted_fixture();
    let account = call.account();
    let mut body = fixture_answers(projection.request());
    body["answers"]["challenge"]["noul"] = json!(0.5);
    let primary = projection
        .request()
        .decode_emulation(
            &serde_json::to_vec(&body).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    let fallback = projection.route(Ok(primary)).unwrap();
    assert!(fallback.projection().is_none());
    assert_eq!(
        fallback
            .finish(None)
            .take_challenge(&observation, account)
            .unwrap(),
        None
    );
    let (_, call, observation, projection) = admitted_fixture();
    let mut body = fixture_answers(projection.request());
    body["answers"]
        .as_object_mut()
        .unwrap()
        .remove("challenge");
    let primary = projection
        .request()
        .decode_emulation(
            &serde_json::to_vec(&body).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    let fallback = projection.route(Ok(primary)).unwrap();
    assert_eq!(fallback.fallback_counts(), [[0, 0, 1, 0], [0; 4], [0; 4]]);
    let subset = fallback.projection().unwrap();
    assert_eq!(subset.question_count(), 1);
    assert!(subset.matches(&observation, call.account()));
    assert!(subset.request().questions().contains_key("challenge"));
    let emulation = subset
        .request()
        .decode_emulation(
            &serde_json::to_vec(&fixture_answers(subset.request())).unwrap(),
            DecisionUsage::default(),
        )
        .unwrap();
    let mut results = fallback.finish(Some(emulation));
    let foreign_account = AgentContextAccountBinding::new(
        AgentAccountAttestationId::from_raw(999),
        call.account().context(),
        AgentAccountScope::Anonymous,
        call.account().observed_at(),
    );
    assert!(results
        .take_challenge(&observation, foreign_account)
        .is_err());
    assert_eq!(
        results
            .take_challenge(&observation, call.account())
            .unwrap(),
        Some(false)
    );
    assert_eq!(
        results
            .take_challenge(&observation, call.account())
            .unwrap(),
        None
    );
    assert_eq!(
        results
            .take_operation(&observation, call.account())
            .unwrap(),
        None
    );
}

#[test]
fn a_small_read_only_navigation_acts_at_the_measured_threshold_and_a_transaction_does_not() {
    for (name, acts) in [("Continue", true), ("Buy now", false)] {
        let (_, call, observation, projection) = admitted_fixture_with_nodes(
            json!([{"k":1,"r":"document"},{"k":2,"p":0,"r":"button","n":name,"o":1}]),
        );
        let mut body = fixture_answers(projection.request());
        for key in ["operation", "click_target"] {
            let Question::Choice { criteria, .. } = &projection.request().questions()[key] else {
                panic!("choice expected");
            };
            let selection = if key == "operation" {
                "click".to_owned()
            } else {
                criteria.keys().find(|key| key.as_str() != "none").unwrap().clone()
            };
            let probabilities: BTreeMap<_, _> = criteria
                .keys()
                .map(|key| (key.clone(), if *key == selection { 0.75 } else { 0.25 / (criteria.len() - 1) as f64 }))
                .collect();
            body["answers"][key] = json!({"type":"choice","choice":selection,"confidence":0.75,"probabilities":probabilities});
        }
        let response = projection
            .request()
            .decode_emulation(&serde_json::to_vec(&body).unwrap(), DecisionUsage::default())
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        assert_eq!(
            answers.take_operation(&observation, call.account()).unwrap(),
            acts.then(|| DecisionOperation::Click(observation.frames()[0].nodes()[1].reference())),
            "{name}"
        );
    }
}

#[test]
fn action_selection_consumes_only_the_compatible_target_and_cannot_be_replayed() {
    for select_click in [false, true] {
        let (_, call, observation, projection) = admitted_fixture_with_actions(true);
        let mut body = fixture_answers(projection.request());
        for (key, selection) in [
            ("operation", "click"),
            ("scroll_target", "@a1"),
            ("click_target", if select_click { "@a2" } else { "none" }),
        ] {
            let Question::Choice { criteria, .. } = &projection.request().questions()[key] else {
                panic!("choice expected");
            };
            let selection = if key == "scroll_target" || (key == "click_target" && select_click) {
                criteria
                    .keys()
                    .find(|key| key.as_str() != "none")
                    .unwrap()
                    .as_str()
            } else {
                selection
            };
            let probabilities: BTreeMap<_, _> = criteria
                .keys()
                .map(|key| (key, if key == selection { 1.0 } else { 0.0 }))
                .collect();
            body["answers"][key] = json!({"type":"choice","choice":selection,"confidence":1.0,"probabilities":probabilities});
        }
        let response = projection
            .request()
            .decode_emulation(
                &serde_json::to_vec(&body).unwrap(),
                DecisionUsage::default(),
            )
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        let selection = answers
            .take_operation(&observation, call.account())
            .unwrap();
        assert_eq!(
            selection,
            if select_click {
                Some(DecisionOperation::Click(
                    observation.frames()[0].nodes()[1].reference(),
                ))
            } else {
                None
            }
        );
        assert_eq!(
            answers
                .take_operation(&observation, call.account())
                .unwrap(),
            None
        );
    }
}

#[test]
fn consumed_decision_recipe_cannot_substitute_target_operation_or_observation() {
    for case in 0..5 {
        let (_, call, observation, projection) = admitted_fixture_with_actions(true);
        let mut body = fixture_answers(projection.request());
        for (key, selection) in [
            ("operation", "click".to_owned()),
            (
                "click_target",
                observation.frames()[0].nodes()[1]
                    .reference()
                    .model_token()
                    .to_string(),
            ),
        ] {
            let Question::Choice { criteria, .. } = &projection.request().questions()[key] else {
                panic!("choice");
            };
            let probabilities: BTreeMap<_, _> = criteria
                .keys()
                .map(|key| (key, if key == &selection { 1.0 } else { 0.0 }))
                .collect();
            body["answers"][key] = json!({"type":"choice","choice":selection,"confidence":1.0,"probabilities":probabilities});
        }
        let response = projection
            .request()
            .decode_emulation(
                &serde_json::to_vec(&body).unwrap(),
                DecisionUsage::default(),
            )
            .unwrap();
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        let selection = answers
            .take_action_selection(&observation, call.account())
            .unwrap()
            .unwrap();
        assert!(answers
            .take_action_selection(&observation, call.account())
            .unwrap()
            .is_none());
        let target = observation.frames()[0].nodes()[if case == 1 { 0 } else { 1 }].reference();
        let (intent, verification) = if case == 2 {
            (
                SemanticActionIntent::Scroll {
                    target,
                    direction: SemanticScrollDirection::Down,
                    amount: SemanticScrollAmount::HalfPage,
                },
                SemanticVerification::ScrollPositionChanged,
            )
        } else {
            (
                SemanticActionIntent::Click { target },
                SemanticVerification::TargetState {
                    state: SemanticState::Focused,
                    present: true,
                },
            )
        };
        let recipe = SemanticActionProposal::try_new(
            intent,
            SemanticEffectClass::Read,
            SemanticWaitCondition::Immediate,
            verification,
            SemanticSettleBudget::try_new(2000).unwrap(),
        )
        .unwrap();
        let frames = if case == 4 {
            vec![]
        } else {
            vec![observation.frames()[0].frame().clone()]
        };
        let current = if case == 3 {
            SemanticObservationAssembler::new(
                SemanticObservationRequest::initial(
                    SemanticObservationId::new(2).unwrap(),
                    observation.request().context(),
                    SemanticObservationBudget::INITIAL_FILTERED,
                ),
                observation.frames()[0].clone(),
            )
            .unwrap()
            .finish()
            .unwrap()
        } else {
            observation.clone()
        };
        let bound = selection.bind_action(
            recipe,
            &current,
            &frames,
            SemanticActionBatchId::new(1).unwrap(),
        );
        assert_eq!(bound.is_ok(), case == 0, "case {case}");
        if let Ok((_, baseline)) = bound {
            assert!(baseline.authenticates(&observation));
        }
    }
}

fn request() -> DecisionRequest {
    DecisionRequest::try_new(
        json!("public fixed fixture"),
        BTreeMap::from([(
            "challenge".into(),
            Question::noul(json!("A challenge?"), None),
        )]),
    )
    .unwrap()
}

fn limits() -> WorkExecutionLimits {
    WorkExecutionLimits {
        model_tokens: 100_000,
        cost_micro_usd: 100_000,
        operations: 2,
        timeout_seconds: 30,
        max_workers: 1,
    }
}

fn response(status: u16, headers: &str, body: &str) -> String {
    format!("HTTP/1.1 {status} fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len())
}

fn success() -> String {
    response(200, "", &json!({"model":zephium_decision::JEV_MODEL,"answers":{"challenge":{"type":"noul","noul":0.02}},"usage":{"input_tokens":123,"output_tokens":7}}).to_string())
}

fn server(responses: Vec<String>) -> (Url, impl FnOnce() -> thread::JoinHandle<usize>) {
    server_for_model(responses, zephium_decision::JEV_MODEL)
}

fn server_for_model(
    responses: Vec<String>,
    model: &'static str,
) -> (Url, impl FnOnce() -> thread::JoinHandle<usize>) {
    server_for_model_with_gate(responses, model, None)
}

type ServerGate = (Arc<AtomicBool>, Arc<std::sync::atomic::AtomicUsize>);

fn server_for_model_with_gate(
    responses: Vec<String>,
    model: &'static str,
    gate: Option<ServerGate>,
) -> (Url, impl FnOnce() -> thread::JoinHandle<usize>) {
    server_for_model_captured(responses, model, gate, None)
}

fn server_for_model_captured(
    responses: Vec<String>,
    model: &'static str,
    gate: Option<ServerGate>,
    capture: Option<Arc<std::sync::Mutex<Vec<Value>>>>,
) -> (Url, impl FnOnce() -> thread::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = Url::parse(&format!(
        "http://127.0.0.1:{}/v1/systemone",
        listener.local_addr().unwrap().port()
    ))
    .unwrap();
    let join = move || {
        thread::spawn(move || {
            let mut count = 0;
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(2))
                        }
                        _ => return count,
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let length = socket.read(&mut chunk).unwrap();
                    assert!(length > 0 && bytes.len() + length < 128 * 1024);
                    bytes.extend_from_slice(&chunk[..length]);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                        let body_len: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(str::to_owned)
                            })
                            .unwrap()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + body_len {
                            let request: serde_json::Value =
                                serde_json::from_slice(&bytes[end + 4..]).unwrap();
                            assert!(request["model"] == model);
                            if let Some(capture) = &capture {
                                capture.lock().unwrap().push(request);
                            }
                            break;
                        }
                    }
                }
                count += 1;
                if let Some((hold, seen)) = &gate {
                    seen.store(count, Ordering::SeqCst);
                    while count == 2 && hold.load(Ordering::SeqCst) {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(2));
                    }
                    let _ = socket.write_all(response.as_bytes());
                } else {
                    socket.write_all(response.as_bytes()).unwrap();
                }
            }
            count
        })
    };
    (endpoint, join)
}

fn client(endpoint: Url) -> JevDecisionClient {
    JevDecisionClient::new(
        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD).unwrap(),
        AgentProviderCredential::try_new(
            DecisionCredentialProvider::TypeSafe,
            "fixture-key".into(),
        )
        .unwrap(),
        endpoint,
        DecisionBackendKind::Jev,
    )
    .unwrap()
}

#[tokio::test]
async fn retries_only_explicit_rate_limit_and_accounts_one_answer() {
    let (endpoint, join) = server(vec![response(429, "Retry-After: 0\r\n", "{}"), success()]);
    let client = client(endpoint);
    let join = join();
    let output = client
        .run(
            &request(),
            limits(),
            Instant::now() + Duration::from_secs(3),
            &AgentProviderCancellation::new(),
        )
        .await;
    assert!(output.response.is_ok(), "{:?}", output.diagnostic);
    assert_eq!(output.diagnostic.attempts, Some(2));
    assert_eq!(output.charged_usage.input_tokens, 123);
    assert_eq!(output.cost_micro_usd, 6);
    assert_eq!(join.join().unwrap(), 2);
    assert!(client.transport.snapshot().unwrap().is_idle());
}

#[tokio::test]
async fn invalid_answer_charges_ceiling_and_preserves_fallback_transport() {
    let (endpoint, join) = server(vec![response(200, "", "{\"unexpected\":true}")]);
    let client = client(endpoint);
    let join = join();
    let output = client
        .run(
            &request(),
            limits(),
            Instant::now() + Duration::from_secs(3),
            &AgentProviderCancellation::new(),
        )
        .await;
    assert!(
        matches!(output.response, Err(DecisionCallFailure::InvalidAnswer)),
        "{:?}",
        output.diagnostic
    );
    assert_eq!(output.charged_usage.input_tokens, MAX_JEV_INPUT_TOKENS);
    assert!(!client.transport.snapshot().unwrap().is_sealed());
    assert_eq!(join.join().unwrap(), 1);
}

#[tokio::test]
async fn refuses_retry_after_beyond_deadline_without_dispatching_again() {
    let (endpoint, join) = server(vec![response(529, "Retry-After: 100\r\n", "{}")]);
    let client = client(endpoint);
    let join = join();
    let output = client
        .run(
            &request(),
            limits(),
            Instant::now() + Duration::from_secs(2),
            &AgentProviderCancellation::new(),
        )
        .await;
    assert!(
        matches!(output.response, Err(DecisionCallFailure::Overloaded)),
        "{:?}",
        output.diagnostic
    );
    assert_eq!(output.diagnostic.attempts, Some(1));
    assert_eq!(output.cost_micro_usd, 0);
    assert_eq!(join.join().unwrap(), 1);
}

#[tokio::test]
async fn cancellation_and_insufficient_budget_do_not_dispatch() {
    let client = client(Url::parse("http://127.0.0.1:9/v1/systemone").unwrap());
    let cancelled = AgentProviderCancellation::new();
    cancelled.cancel();
    let output = client
        .run(
            &request(),
            limits(),
            Instant::now() + Duration::from_secs(2),
            &cancelled,
        )
        .await;
    assert!(matches!(
        output.response,
        Err(DecisionCallFailure::Cancelled)
    ));
    assert_eq!(output.diagnostic.attempts, Some(0));
    let mut limited = limits();
    limited.model_tokens = 1;
    let output = client
        .run(
            &request(),
            limited,
            Instant::now() + Duration::from_secs(2),
            &AgentProviderCancellation::new(),
        )
        .await;
    assert!(matches!(
        output.response,
        Err(DecisionCallFailure::Capacity)
    ));
    assert_eq!(output.cost_micro_usd, 0);
}

#[test]
fn retry_after_honors_dates_and_refuses_malformed_values() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let mut headers = HeaderMap::new();
    headers.insert(
        RETRY_AFTER,
        HeaderValue::from_str(&httpdate::fmt_http_date(now + Duration::from_secs(12))).unwrap(),
    );
    assert_eq!(retry_delay(&headers, 1, now), Some(Duration::from_secs(12)));
    headers.insert(RETRY_AFTER, HeaderValue::from_static("invalid"));
    assert_eq!(retry_delay(&headers, 1, now), None);
    headers.insert(
        RETRY_AFTER,
        HeaderValue::from_static("18446744073709551616"),
    );
    assert_eq!(retry_delay(&headers, 1, now), None);
}

#[test]
fn cloud_endpoint_and_credentials_are_bound_independently_of_page_data() {
    assert_eq!(
        cloud_endpoint("https://cloud.zephium.app/decisions/")
            .unwrap()
            .path(),
        "/decisions/v1/systemone"
    );
    for base in [
        "http://cloud.zephium.app",
        "https://user@cloud.zephium.app",
        "https://cloud.zephium.app?token=secret",
        "https://cloud.zephium.app/#fragment",
        "https://127.0.0.1",
        "https://localhost",
        "https://cloud.zephium.app:8443",
    ] {
        assert!(cloud_endpoint(base).is_err());
    }
    let credential = AgentProviderCredential::try_new(
        DecisionCredentialProvider::ZephiumCloud,
        "fixture-secret".into(),
    )
    .unwrap();
    assert!(!format!("{credential:?}").contains("fixture-secret"));
    assert!(JevDecisionClient::direct(
        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD).unwrap(),
        credential
    )
    .is_err());
}

fn search_ranking_fixture(
    count: usize,
) -> (
    zephium_core::work::search::WorkPublicSearchScope,
    zephium_core::work::search::WorkProviderSearchEvidenceV1,
) {
    use zephium_core::work::search::*;
    let scope = WorkPublicSearchScope {
        provider: WorkSearchProvider::OpenAi,
        model: "gpt-5.6-luna".into(),
        query: "public source".into(),
    };
    let evidence = WorkProviderSearchEvidenceV1 {
        version: 1,
        provider: scope.provider,
        model: scope.model.clone(),
        response_model: scope.model.clone(),
        response_id: "resp_ranking".into(),
        search_call_id: "ws_ranking".into(),
        answer: "Public provider evidence.".into(),
        citations: (0..count)
            .map(|index| WorkProviderSearchCitation {
                url: format!("https://example.test/{index}?private_query=excluded"),
                title: format!("Source {index}"),
                start_index: 0,
                end_index: 6,
            })
            .collect(),
        actual_input_tokens: 100,
        actual_output_tokens: 10,
    };
    (scope, evidence)
}

#[test]
fn search_ranking_projection_is_bounded_and_uses_only_admitted_public_candidates() {
    let (scope, evidence) = search_ranking_fixture(64);
    let projection = super::search::search_projection(&scope, &evidence).unwrap();
    assert_eq!(projection.questions().len(), 16);
    let encoded = projection.encode().unwrap();
    let text = std::str::from_utf8(&encoded).unwrap();
    assert!(!text.contains("private_query"));
    assert!(text.contains("untrusted_provider_search"));
    assert!(projection
        .questions()
        .values()
        .all(|question| question.kind() == zephium_decision::QuestionKind::Noul));
    let mut different = scope.clone();
    different.model = "gpt-4.1-mini".into();
    assert!(super::search::search_projection(&different, &evidence).is_err());
    let mut secret = evidence.clone();
    secret.answer = "Authorization: Bearer do-not-disclose-this-secret".into();
    assert!(super::search::search_projection(&scope, &secret).is_err());
    let mut duplicate = evidence.clone();
    for source in &mut duplicate.citations {
        source.url = evidence.citations[0].url.clone();
    }
    assert!(super::search::search_projection(&scope, &duplicate).is_err());
}

#[tokio::test]
async fn search_ranking_falls_back_per_question_and_charges_both_backends() {
    let (scope, evidence) = search_ranking_fixture(2);
    let (endpoint, jev_server) = server(vec![response(
        200,
        "",
        &json!({
            "model":zephium_decision::JEV_MODEL, "answers":{
                "source_1":{"type":"noul","noul":0.01},
                "source_2":{"type":"noul","noul":0.5}
            }, "usage":{"input_tokens":123,"output_tokens":7}
        })
        .to_string(),
    )]);
    let jev = client(endpoint);
    let body = json!({"object":"response","status":"completed","model":"gpt-5.6-terra","service_tier":"default","error":null,"incomplete_details":null,
        "output":[{"type":"reasoning","summary":[]},{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":json!({"answers":{"source_2":{"type":"noul","noul":0.99}}}).to_string()}]}],
        "usage":{"input_tokens":100,"output_tokens":200,"total_tokens":300,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":50}}});
    let (mut endpoint, emulation_server) = server_for_model(
        vec![
            response(
                200,
                "",
                r#"{"object":"response.input_tokens","input_tokens":100}"#,
            ),
            response(200, "", &body.to_string()),
        ],
        "gpt-5.6-terra",
    );
    endpoint.set_path("/v1/responses");
    let transport = AgentProviderTransport::try_new_loopback(
        AgentProviderTransportConfig::STANDARD,
        endpoint.as_str(),
        endpoint.as_str(),
    )
    .unwrap();
    let credential =
        AgentProviderCredential::try_new(AgentProviderKind::OpenAiResponses, "fixture-key".into())
            .unwrap();
    let ranking = super::search::SearchDecisionRanking::new(Some(jev), emulation_config(), None);
    let jev_server = jev_server();
    let emulation_server = emulation_server();
    let output = ranking
        .rerank(
            &transport,
            &credential,
            &scope,
            &evidence,
            limits(),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(jev_server.join().unwrap(), 1);
    assert_eq!(emulation_server.join().unwrap(), 2);
    assert_eq!(output.preferred, vec![2]);
    assert_eq!(output.usage.model_tokens, 430);
    assert_eq!(output.usage.operations, 2);
    assert_eq!(
        output.usage.accounting,
        zephium_core::work::runtime::WorkUsageAccounting::Exact
    );
    assert!(transport.snapshot().unwrap().is_idle());
    output.validate(&evidence).unwrap();
}

#[test]
fn recorded_search_eval_requests_match_the_shipping_projection_and_emulation_body() {
    let transport =
        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD).unwrap();
    let credential =
        AgentProviderCredential::try_new(AgentProviderKind::OpenAiResponses, "fixture-key".into())
            .unwrap();
    let config = emulation_config();
    let client = OpenAiDecisionCall::try_new(&transport, &credential, &config).unwrap();
    for (source, raw_fixture) in [
        (
            include_str!("../../../../zephium-decision/evals/search_flow_source_01.json"),
            include_str!("../../../../zephium-decision/evals/search_flow_01.json"),
        ),
        (
            include_str!("../../../../zephium-decision/evals/search_flow_source_01.json"),
            include_str!("../../../../zephium-decision/evals/search_flow_unrelated_01.json"),
        ),
        (
            include_str!("../../../../zephium-decision/evals/search_mixed_source_01.json"),
            include_str!("../../../../zephium-decision/evals/search_mixed_01.json"),
        ),
    ] {
        let raw: Value = serde_json::from_str(source).unwrap();
        let mut scope: zephium_core::work::search::WorkPublicSearchScope =
            serde_json::from_value(raw["scope"].clone()).unwrap();
        let evidence: zephium_core::work::search::WorkProviderSearchEvidenceV1 =
            serde_json::from_value(raw["evidence"].clone()).unwrap();
        let fixture: Value = serde_json::from_str(raw_fixture).unwrap();
        use sha2::Digest as _;
        let digest = sha2::Sha256::digest(source.as_bytes());
        assert_eq!(fixture["source_sha256"], format!("{digest:x}"));
        scope.query = fixture["request"]["state"]["public_query"]
            .as_str()
            .unwrap()
            .to_owned();
        let actual = super::search::search_projection(&scope, &evidence).unwrap();
        assert!(actual.state() == &fixture["request"]["state"]);
        assert!(
            serde_json::to_value(actual.questions()).unwrap() == fixture["request"]["questions"]
        );
        let body = client.body(&actual).unwrap();
        assert_eq!(body["instructions"], EMULATION_INSTRUCTIONS);
        let content: Value =
            serde_json::from_str(body["input"][0]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            content,
            json!({"state": actual.state(), "questions": actual.questions()})
        );
        assert_eq!(body["text"]["format"]["schema"], actual.answer_schema());
        assert_eq!(body["store"], false);
        assert!(body.get("previous_response_id").is_none());
    }
}

#[tokio::test]
async fn search_ranking_deadline_keeps_interrupted_generation_unknown() {
    let hold = Arc::new(AtomicBool::new(true));
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (mut endpoint, server) = server_for_model_with_gate(
        vec![
            response(
                200,
                "",
                r#"{"object":"response.input_tokens","input_tokens":100}"#,
            ),
            response(200, "", "{}"),
        ],
        "gpt-5.6-terra",
        Some((hold.clone(), seen.clone())),
    );
    endpoint.set_path("/v1/responses");
    let transport = AgentProviderTransport::try_new_loopback(
        AgentProviderTransportConfig::STANDARD,
        endpoint.as_str(),
        endpoint.as_str(),
    )
    .unwrap();
    let credential =
        AgentProviderCredential::try_new(AgentProviderKind::OpenAiResponses, "fixture-key".into())
            .unwrap();
    let ranking = super::search::SearchDecisionRanking::new(None, emulation_config(), None);
    let (scope, evidence) = search_ranking_fixture(2);
    let server = server();
    let mut pending = Box::pin(ranking.rerank(
        &transport,
        &credential,
        &scope,
        &evidence,
        limits(),
        Instant::now() + Duration::from_secs(2),
    ));
    while seen.load(Ordering::SeqCst) != 2 {
        tokio::select! {
            _ = &mut pending => panic!("ranking ended before generation was held"),
            _ = tokio::time::sleep(Duration::from_millis(2)) => {},
        }
    }
    assert!(matches!(
        pending.await,
        Err(zephium_core::work::search::WorkPublicSearchError::OutcomeUnknown)
    ));
    hold.store(false, Ordering::SeqCst);
    assert_eq!(server.join().unwrap(), 2);
    assert!(transport.snapshot().unwrap().is_sealed());
}

fn catalog_previews() -> Vec<zephium_core::work::artifact::WorkEvidencePreviewV1> {
    serde_json::from_str(include_str!(
        "../../../../zephium-decision/evals/catalog_links_source_01.json"
    ))
    .unwrap()
}

fn catalog_disclosure() -> zephium_core::work::agent::WorkAgentTurnDisclosure {
    use zephium_core::work::agent::*;
    WorkAgentTurnDisclosure::try_new(
        "Read the Tower Bridge LEGO product page to obtain its displayed price and pieces count.",
        vec![],
        vec![],
        &[],
        &catalog_previews(),
        &[],
        WorkAgentBudget {
            turns_left: 3,
            steps_left: 8,
            browse_available: true,
        },
        WorkExecutionLimits {
            operations: 8,
            ..limits()
        },
        vec![],
    )
    .unwrap()
}

#[test]
fn catalog_link_projection_matches_recorded_evals_and_excludes_unadmitted_targets() {
    use super::link::link_projection;
    let input = catalog_disclosure();
    let context = input.context();
    for raw in [
        include_str!("../../../../zephium-decision/evals/catalog_tower_bridge_01.json"),
        include_str!("../../../../zephium-decision/evals/catalog_absent_01.json"),
    ] {
        let fixture: Value = serde_json::from_str(raw).unwrap();
        use sha2::Digest as _;
        let digest = sha2::Sha256::digest(include_bytes!(
            "../../../../zephium-decision/evals/catalog_links_source_01.json"
        ));
        assert_eq!(fixture["source_sha256"], format!("{digest:x}"));
        let actual = link_projection(
            fixture["request"]["state"]["objective"].as_str().unwrap(),
            &context.sources,
            &[],
            true,
        )
        .unwrap();
        assert!(actual.state() == &fixture["request"]["state"]);
        assert!(
            serde_json::to_value(actual.questions()).unwrap() == fixture["request"]["questions"]
        );
    }
    assert!(link_projection(&context.objective, &context.sources, &[], false).is_err());
    let mut sources = catalog_disclosure()
        .context()
        .sources
        .iter()
        .map(|source| zephium_core::work::agent::WorkAgentSourceView {
            command: None,
            key: source.key,
            acquired_by: source.acquired_by,
            title: source.title.clone(),
            url: source.url.clone(),
            link_destination: source.link_destination.clone(),
            text: source.text.clone(),
            truncated: source.truncated,
        })
        .collect::<Vec<_>>();
    for source in &mut sources {
        source.acquired_by = "provider_search";
    }
    assert!(link_projection("detail", &sources, &[], true).is_err());
    for source in &mut sources {
        source.acquired_by = "native_browser";
        source.truncated = true;
    }
    assert!(link_projection("detail", &sources, &[], true).is_err());
    for source in &mut sources {
        source.truncated = false;
        source.text = "forged destination".into();
    }
    assert!(link_projection("detail", &sources, &[], true).is_err());
    let steps = context
        .sources
        .iter()
        .map(|source| zephium_core::work::agent::WorkAgentStepView {
            turn: 1,
            kind: "read",
            detail: source.link_destination.clone().unwrap(),
            outcome: "failed",
            note: None,
        })
        .collect::<Vec<_>>();
    assert!(link_projection("detail", &context.sources, &steps, true).is_err());
    assert!(link_projection(
        "Authorization: Bearer do-not-disclose-this-secret",
        &context.sources,
        &[],
        true
    )
    .is_err());
}

#[tokio::test]
async fn catalog_link_fallback_reaches_the_work_turn_and_charges_every_call() {
    use zephium_core::work::agent::WorkAgentTurnProvider;
    let input = catalog_disclosure();
    let request = super::link::link_projection(
        &input.context().objective,
        &input.context().sources,
        &[],
        true,
    )
    .unwrap();
    let Question::Choice { criteria, .. } = &request.questions()["next_page"] else {
        panic!()
    };
    let selected = criteria
        .keys()
        .find(|key| key.as_str() != "none")
        .unwrap()
        .clone();
    let answer = |confident: bool| {
        let probabilities: BTreeMap<_, _> = criteria
            .keys()
            .map(|key| {
                (
                    key,
                    if confident {
                        if *key == selected {
                            1.0
                        } else {
                            0.0
                        }
                    } else {
                        1.0 / criteria.len() as f64
                    },
                )
            })
            .collect();
        json!({"next_page":{"type":"choice","choice":if confident { selected.as_str() } else { "none" },"confidence":if confident { 1.0 } else { 1.0 / criteria.len() as f64 },"probabilities":probabilities}})
    };
    let (endpoint, jev_server) = server(vec![response(200, "", &json!({"model":zephium_decision::JEV_MODEL,"answers":answer(false),"usage":{"input_tokens":123,"output_tokens":7}}).to_string())]);
    let body = |text: Value| {
        json!({"object":"response","status":"completed","model":"gpt-5.6-terra","service_tier":"default","error":null,"incomplete_details":null,
        "output":[{"type":"reasoning","summary":[]},{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text.to_string()}]}],
        "usage":{"input_tokens":100,"output_tokens":200,"total_tokens":300,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":50}}})
    };
    let count = response(
        200,
        "",
        r#"{"object":"response.input_tokens","input_tokens":100}"#,
    );
    let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (mut openai_endpoint, openai_server) = server_for_model_captured(vec![
        count.clone(), response(200, "", &body(json!({"answers":answer(true)})).to_string()),
        count, response(200, "", &body(json!({"say":null,"artifacts":[],"fetch":[],"ask":null,"finish":true,"followups":[]})).to_string()),
    ], "gpt-5.6-terra", None, Some(captured.clone()));
    openai_endpoint.set_path("/v1/responses");
    let transport = AgentProviderTransport::try_new_loopback(
        AgentProviderTransportConfig::STANDARD,
        openai_endpoint.as_str(),
        openai_endpoint.as_str(),
    )
    .unwrap();
    let credential =
        AgentProviderCredential::try_new(AgentProviderKind::OpenAiResponses, "fixture-key".into())
            .unwrap();
    let agent = super::super::agent::OpenAiWorkAgent::try_new(
        transport.clone(),
        credential,
        emulation_config(),
    )
    .unwrap()
    .with_link_decisions(Some(client(endpoint)), emulation_config(), None)
    .unwrap();
    let jev_server = jev_server();
    let openai_server = openai_server();
    let output = agent
        .turn(
            &input,
            zephium_core::work::synthesis::WorkSynthesisTrace {
                work: 1.into(),
                execution: 1.into(),
                attempt: 1.into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(jev_server.join().unwrap(), 1);
    assert_eq!(openai_server.join().unwrap(), 4);
    assert_eq!(output.usage.model_tokens, 730);
    assert_eq!(output.usage.operations, 3);
    assert!(output.output.finish);
    let captured = captured.lock().unwrap();
    let context: Value = serde_json::from_str(
        captured[3]["input"][0]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        context["next_page_candidate"]["source_key"],
        selected
            .strip_prefix("source_")
            .unwrap()
            .parse::<u16>()
            .unwrap()
    );
    assert!(transport.snapshot().unwrap().is_idle());
}

#[test]
fn a_catalog_read_copies_each_found_record_from_inside_it_or_leaves_the_planner() {
    let (_, call, previous, _) = admitted_fixture();
    // References are node keys: @a3, @a9 and @a15 are product records, @a21
    // and @a23 a navigation list of the same parent shape rule.
    let card = |k: u64, name: &str, text: &str, slug: &str| {
        let parent = k - 1;
        vec![
            json!({"k":k,"p":1,"r":"list_item","fc":true}),
            json!({"k":k+1,"p":parent,"r":"document","t":text,"fc":true}),
            json!({"k":k+2,"p":k,"r":"link","n":name,"u":format!("https://shop.example.test/{slug}"),"fc":true}),
            json!({"k":k+3,"p":k+1,"r":"image","n":"primary","m":format!("https://shop.example.test/{slug}.webp"),"fc":true}),
            json!({"k":k+4,"p":k,"r":"heading","l":3,"n":name,"fc":true}),
            json!({"k":k+5,"p":k,"r":"button","n":"Add to Bag","fc":true}),
        ]
    };
    let mut nodes = vec![
        json!({"k":1,"r":"document","fc":true}),
        json!({"k":2,"p":0,"r":"list","fc":true}),
    ];
    nodes.extend(card(3, "Tower Bridge", "Tower Bridge $349.99 New", "tower-bridge"));
    nodes.extend(card(9, "Paris", "Paris $79.99", "paris"));
    nodes.extend(card(15, "London", "$39.99", "london"));
    nodes.extend([
        json!({"k":21,"p":0,"r":"list","fc":true}),
        json!({"k":22,"p":20,"r":"list_item","fc":true}),
        json!({"k":23,"p":21,"r":"link","n":"Home","u":"https://shop.example.test/","fc":true}),
        json!({"k":24,"p":20,"r":"list_item","fc":true}),
        json!({"k":25,"p":23,"r":"link","n":"Help","u":"https://shop.example.test/help","fc":true}),
    ]);
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(1).unwrap(),
            previous.frames()[0].frame().clone(),
            SemanticSnapshotGeneration::new(1).unwrap(),
        ),
        &serde_json::to_vec(
            &json!({"v":SEMANTIC_WIRE_VERSION,"i":1,"g":1,"c":"complete","n":nodes}),
        )
        .unwrap(),
    )
    .unwrap();
    let observation = SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(1).unwrap(),
            call.account().context(),
            SemanticObservationBudget::try_new(32, 8192, 1).unwrap(),
        ),
        snapshot,
    )
    .unwrap()
    .finish()
    .unwrap();
    let columns = vec![
        SemanticExtractionFieldSchema::try_text("name".into(), true, 512).unwrap(),
        SemanticExtractionFieldSchema::try_text("price".into(), false, 512)
            .unwrap()
            .with_verbatim_text()
            .unwrap(),
        SemanticExtractionFieldSchema::try_url("product_url".into(), false, 1024).unwrap(),
        SemanticExtractionFieldSchema::try_image_url("image".into(), false, 1024).unwrap(),
    ];
    let schema = SemanticExtractionSchema::try_new(
        SemanticExtractionSchemaId::new(1).unwrap(),
        vec![SemanticExtractionFieldSchema::try_rows("output_0".into(), true, columns, 3).unwrap()],
    )
    .unwrap();
    assert!(schema.is_row_collection() && schema.reads_whole_page());
    let objective = AgentProviderObjective::try_admit_conservative_utf8(
        "Collect three sets with prices, links and pictures".into(),
        &SemanticTokenizerRevision::try_new("fixture-v1".into()).unwrap(),
    )
    .unwrap();
    let decide = |projection: &DecisionObservation, body: Value| {
        let response = projection
            .request()
            .decode_emulation(&serde_json::to_vec(&body).unwrap(), DecisionUsage::default())
            .unwrap();
        response
    };
    for subjects in [true, false] {
        let projection = DecisionObservation::try_for_read(
            &observation,
            &objective,
            &AgentProviderActionAuthority::try_new(&observation, &[]).unwrap(),
            call.account(),
            Some(&schema),
        )
        .unwrap();
        let Question::Choice { criteria, .. } = &projection.request().questions()["rows_0"] else {
            panic!("choice expected");
        };
        assert_eq!(
            criteria.keys().filter(|key| *key != "none").collect::<Vec<_>>(),
            ["@a15", "@a3", "@a9"]
        );
        assert!(projection.request().questions().contains_key("group_1"));
        let mut body = fixture_answers(projection.request());
        if subjects {
            body["answers"]["group_0"] = json!({"type":"noul","noul":0.95});
        }
        let response = decide(&projection, body);
        let mut answers = projection.route(Ok(response)).unwrap().finish(None);
        let discovery = answers
            .take_row_discovery(&observation, call.account(), &schema)
            .unwrap();
        let Some(discovery) = discovery else {
            // No group of subjects: the page planner reads this page.
            assert!(!subjects);
            continue;
        };
        assert_eq!((discovery.rows(), discovery.max_items()), (3, 3));
        assert!(discovery.needs_cells());
        let cells = DecisionObservation::try_for_row_cells(
            &observation,
            &objective,
            call.account(),
            &discovery,
        )
        .unwrap();
        let questions = cells.request().questions();
        assert_eq!(
            questions.keys().collect::<Vec<_>>(),
            ["cell_0_1", "cell_1_1", "cell_2_1"]
        );
        let Question::Choice { criteria, .. } = &questions["cell_2_1"] else {
            panic!("choice expected");
        };
        assert!(criteria.contains_key("@a16") && !criteria.contains_key("@a4"));
        // The second record's head stays unsettled; the settled records agree
        // on the position its value takes.
        let answer = |key: &str, shares: &[(&str, f64)]| {
            let Question::Choice { criteria, .. } = &questions[key] else {
                panic!("choice expected");
            };
            let probabilities: BTreeMap<_, _> = criteria
                .keys()
                .map(|option| {
                    let share = shares.iter().find(|(chosen, _)| chosen == option);
                    (option.clone(), share.map_or(0.0, |(_, share)| *share))
                })
                .collect();
            json!({"type":"choice","choice":shares[0].0,"confidence":shares[0].1,"probabilities":probabilities})
        };
        let body = json!({"answers":{
            "cell_0_1": answer("cell_0_1", &[("@a4", 0.95), ("none", 0.05)]),
            "cell_1_1": answer("cell_1_1", &[("@a13", 0.4), ("@a10", 0.35), ("none", 0.25)]),
            "cell_2_1": answer("cell_2_1", &[("@a16", 0.95), ("none", 0.05)]),
        }});
        let response = decide(&cells, body);
        let mut cells = cells.route(Ok(response)).unwrap().finish(None);
        let located = discovery
            .prepare(
                &observation,
                call.account(),
                SemanticCaptureInstant::from_millis(101),
                Some(&mut cells),
            )
            .unwrap();
        assert!(located.generation().is_none());
        let result = located.finish(None).unwrap();
        let SemanticExtractedValue::Rows(rows) = result.fields()[0].value() else {
            panic!("rows expected");
        };
        let text = |value: &SemanticExtractedValue| match value {
            SemanticExtractedValue::Text(text)
            | SemanticExtractedValue::Url(text)
            | SemanticExtractedValue::ImageUrl(text) => text.as_str().to_owned(),
            _ => panic!("text expected"),
        };
        let copied: Vec<Vec<String>> = rows
            .items()
            .iter()
            .map(|row| row.fields().iter().map(|field| text(field.value())).collect())
            .collect();
        assert_eq!(
            copied,
            [
                ["Tower Bridge", "$349.99", "https://shop.example.test/tower-bridge", "https://shop.example.test/tower-bridge.webp"],
                ["Paris", "$79.99", "https://shop.example.test/paris", "https://shop.example.test/paris.webp"],
                ["London", "$39.99", "https://shop.example.test/london", "https://shop.example.test/london.webp"],
            ]
        );
    }
}

#[test]
fn a_located_label_value_pair_copies_its_value_node() {
    let (call, observation, schema, projection) = located_fixture_with(
        false,
        &[
            json!({"k":9,"p":0,"r":"group","n":"Pieces:3745","fc":true}),
            json!({"k":10,"p":8,"r":"paragraph","t":"3745","fc":true}),
        ],
    );
    let mut output = located_answers(&projection);
    let Question::Choice { criteria, .. } = &projection.request().questions()["locate_1"] else {
        panic!("choice expected");
    };
    let probabilities: BTreeMap<_, _> = criteria
        .keys()
        .map(|key| (key, if key == "@a9" { 1.0 } else { 0.0 }))
        .collect();
    output["answers"]["locate_1"] =
        json!({"type":"choice","choice":"@a9","confidence":1.0,"probabilities":probabilities});
    let response = projection
        .request()
        .decode_emulation(&serde_json::to_vec(&output).unwrap(), DecisionUsage::default())
        .unwrap();
    let mut answers = projection.route(Ok(response)).unwrap().finish(None);
    let selection = answers
        .take_read_selection(&observation, call.account(), &schema)
        .unwrap()
        .unwrap();
    let result = selection
        .prepare(&observation, call.account(), SemanticCaptureInstant::from_millis(101), None)
        .unwrap()
        .finish(None)
        .unwrap();
    let SemanticExtractedValue::Rows(rows) = result.fields()[0].value() else {
        panic!("rows expected");
    };
    let SemanticExtractedValue::Text(value) = rows.items()[0].fields()[1].value() else {
        panic!("text expected");
    };
    assert_eq!(value.as_str(), "3745");
}
