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
    let nodes = if actions {
        json!([{"k":1,"r":"document","o":16},{"k":2,"p":0,"r":"button","n":"Details","o":1}])
    } else {
        json!([{"k":1,"r":"document"}])
    };
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
    let (_, call, previous, _) = admitted_fixture();
    let nodes = json!([
        {"k":1,"r":"document","fc":true},
        {"k":2,"p":0,"r":"heading","l":1,"n":"Public product","t":"Public product","fc":true},
        {"k":3,"p":0,"r":"paragraph","n":"Price","t":"$349.99","fc":true},
        {"k":4,"p":0,"r":"image","n":"Product","m":"https://example.test/product.webp","fc":true},
        {"k":5,"p":0,"r":"paragraph","t":"Product description","fc":true},
        {"k":6,"p":0,"r":"paragraph","t":"Nearby evidence","fc":true},
        {"k":7,"p":0,"r":"paragraph","t":"Personal value","q":"sensitive","fc":true},
        {"k":8,"p":0,"r":"paragraph","t":"Unrelated footer","fc":true}
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
        SemanticExtractionFieldSchema::try_text("price".into(), true, 512)
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
            assert_eq!(
                output.call.diagnostic.envelope_failure,
                Some(DecisionEnvelopeFailure::ItemCount)
            );
            assert_eq!(
                output.receipt.usage_accounting(),
                AgentModelUsageAccounting::ReservationCeiling
            );
            assert_eq!(output.receipt.input_tokens(), 8192);
            assert_eq!(output.receipt.output_tokens(), 4096);
            assert_eq!(output.receipt.cost_micro_usd(), 10_000);
        } else {
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
fn fallback_projection_preserves_binding_and_only_repeats_uncertain_heads() {
    let (_, call, observation, projection) = admitted_fixture();
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
    assert_eq!(fallback.fallback_counts(), [[0, 0, 0, 1], [0; 4], [0; 4]]);
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
