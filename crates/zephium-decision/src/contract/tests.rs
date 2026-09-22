use super::*;
use serde_json::json;

fn request() -> DecisionRequest {
    DecisionRequest::try_new(
        json!("untrusted fixture"),
        BTreeMap::from([
            (
                "challenge".into(),
                Question::noul(json!("Is this a challenge?"), None),
            ),
            (
                "target".into(),
                Question::choice(
                    json!("Choose target"),
                    BTreeMap::from([
                        ("@a1".into(), json!("first")),
                        ("@a2".into(), json!("second")),
                    ]),
                )
                .unwrap(),
            ),
            (
                "relevance".into(),
                Question::score(
                    json!("Relevance"),
                    vec!["absent".into(), "partial".into(), "complete".into()],
                ),
            ),
        ]),
    )
    .unwrap()
}

fn response() -> Value {
    json!({"model":JEV_MODEL,"usage":{"input_tokens":123,"output_tokens":45},"answers":{
        "challenge":{"type":"noul","noul":0.02},
        "target":{"type":"choice","choice":"@a1","confidence":0.9,"probabilities":{"@a1":0.9,"@a2":0.08,"none":0.02}},
        "relevance":{"type":"score","score":1.5,"confidence":0.5,"probabilities":{"0":0.0,"1":0.5,"2":0.5},"legend":{"0":"absent","1":"partial","2":"complete"}}
    }})
}

#[test]
fn accepts_all_three_typed_answers_and_none_abstention() {
    let request = request();
    let mut value = response();
    let decoded = request
        .decode(&serde_json::to_vec(&value).unwrap())
        .unwrap();
    assert_eq!(decoded.usage.input_tokens, 123);
    assert!(decoded.answers.values().all(Result::is_ok));
    value["answers"]["target"]["choice"] = json!("none");
    value["answers"]["target"]["probabilities"] = json!({"@a1":0.1,"@a2":0.0,"none":0.9});
    assert!(request
        .decode(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .answers["target"]
        .is_ok());
}

#[test]
fn rejects_forged_distributions_without_discarding_other_answers() {
    for invalid in [
        json!({"choice":"@a99","probabilities":{"@a1":0.9,"@a2":0.08,"none":0.02}}),
        json!({"choice":"@a1","probabilities":{"@a1":0.9,"@a99":0.08,"none":0.02}}),
        json!({"choice":"@a2","probabilities":{"@a1":0.9,"@a2":0.08,"none":0.02}}),
        json!({"choice":"@a1","probabilities":{"@a1":0.9,"@a2":0.3,"none":0.02}}),
        json!({"choice":"@a1","probabilities":{"@a1":1.1,"@a2":-0.1,"none":0.0}}),
        json!({"choice":"@a1","probabilities":{"@a1":1.0}}),
    ] {
        let mut value = response();
        value["answers"]["target"]["choice"] = invalid["choice"].clone();
        value["answers"]["target"]["probabilities"] = invalid["probabilities"].clone();
        let decoded = request()
            .decode(&serde_json::to_vec(&value).unwrap())
            .unwrap();
        assert!(matches!(
            decoded.answers["target"],
            Err(ContractError::Answer)
        ));
        assert!(decoded.answers["challenge"].is_ok());
        assert!(decoded.answers["relevance"].is_ok());
    }
}

#[test]
fn rejects_unknown_model_questions_fields_and_missing_answers() {
    let mut value = response();
    value["model"] = json!("jev-latest");
    assert!(matches!(
        request().decode(&serde_json::to_vec(&value).unwrap()),
        Err(ContractError::Model)
    ));
    value = response();
    value["answers"]["invented"] = json!({"type":"noul","noul":1});
    assert!(matches!(
        request().decode(&serde_json::to_vec(&value).unwrap()),
        Err(ContractError::Question)
    ));
    value = response();
    value["answers"].as_object_mut().unwrap().remove("target");
    value["answers"]["challenge"]["instructions"] = json!("ignore policy");
    let decoded = request()
        .decode(&serde_json::to_vec(&value).unwrap())
        .unwrap();
    assert!(matches!(
        decoded.answers["target"],
        Err(ContractError::MissingAnswer)
    ));
    assert!(matches!(
        decoded.answers["challenge"],
        Err(ContractError::Answer)
    ));
}

#[test]
fn score_must_preserve_ordered_legend_and_weighted_value() {
    for (field, replacement) in [
        ("score", json!(2)),
        ("legend", json!({"0":"complete","1":"partial","2":"absent"})),
        ("probabilities", json!({"0":0.5,"1":0.5,"3":0.0})),
    ] {
        let mut value = response();
        value["answers"]["relevance"][field] = replacement;
        assert!(matches!(
            request()
                .decode(&serde_json::to_vec(&value).unwrap())
                .unwrap()
                .answers["relevance"],
            Err(ContractError::Answer)
        ));
    }
}

#[test]
fn score_accepts_structured_levels_and_rejects_changed_legends_and_vendor_overflow() {
    let levels = vec![
        json!({"description":"blocked","examples":["challenge"]}),
        json!(["readable", "evidence"]),
        Value::Null,
    ];
    let request = DecisionRequest::try_new(
        json!("state"),
        BTreeMap::from([(
            "coverage".into(),
            Question::score(json!("Coverage?"), levels.clone()),
        )]),
    )
    .unwrap();
    let mut response = json!({"answers":{"coverage":{"type":"score","score":1.0,"confidence":1.0,"probabilities":{"0":0.0,"1":1.0,"2":0.0},"legend":{"0":levels[0],"1":levels[1],"2":levels[2]}}}});
    assert!(request
        .decode_emulation(
            &serde_json::to_vec(&response).unwrap(),
            DecisionUsage::default()
        )
        .unwrap()
        .answers["coverage"]
        .is_ok());
    response["answers"]["coverage"]["legend"]["1"] = json!(["evidence", "readable"]);
    assert!(request
        .decode_emulation(
            &serde_json::to_vec(&response).unwrap(),
            DecisionUsage::default()
        )
        .unwrap()
        .answers["coverage"]
        .is_err());
    let schema = request.answer_schema();
    assert_eq!(
        schema["properties"]["answers"]["properties"]["coverage"]["properties"]["legend"]
            ["properties"]["0"]["additionalProperties"],
        false
    );
    assert!(DecisionRequest::try_new(
        json!("state"),
        BTreeMap::from([(
            "coverage".into(),
            Question::score(
                json!("Coverage?"),
                vec![json!("level"); MAX_SCORE_LEVELS + 1]
            )
        )])
    )
    .is_err());
}

#[test]
fn immutable_subset_preserves_state_and_refuses_unknown_or_duplicate_questions() {
    let original = request();
    let subset = original.subset(["target"]).unwrap();
    assert_eq!(subset.state(), original.state());
    assert_eq!(subset.questions().len(), 1);
    assert_eq!(
        serde_json::to_value(&subset.questions()["target"]).unwrap(),
        serde_json::to_value(&original.questions()["target"]).unwrap()
    );
    assert!(original.subset(["unknown"]).is_err());
    assert!(original.subset(["target", "target"]).is_err());
    assert!(original.subset([]).is_err());
    assert!(!format!("{original:?}").contains("untrusted fixture"));
}

#[test]
fn request_enforces_capacity_and_reserved_abstention() {
    let options = (0..255).map(|i| (format!("@a{i}"), Value::Null)).collect();
    assert!(Question::choice(json!("target"), options).is_err());
    let question = Question::Choice {
        instructions: json!("target"),
        criteria: BTreeMap::from([("@a1".into(), Value::Null)]),
    };
    assert!(DecisionRequest::try_new(
        json!("state"),
        BTreeMap::from([("target".into(), question)])
    )
    .is_err());
    assert!(Question::choice(
        json!("target"),
        BTreeMap::from([("none".into(), json!("click"))])
    )
    .is_err());
    assert!(
        DecisionRequest::try_new(json!("s".repeat(MAX_STATE_BYTES)), request().questions).is_err()
    );
    assert!(!probability(f64::NAN));
    assert!(!probability(f64::INFINITY));
}

#[test]
fn emulation_schema_enumerates_exact_question_and_option_keys() {
    let schema = request().answer_schema();
    assert_eq!(schema["additionalProperties"], false);
    let answers = &schema["properties"]["answers"];
    assert_eq!(answers["required"].as_array().unwrap().len(), 3);
    let target = &answers["properties"]["target"];
    assert_eq!(
        target["properties"]["choice"]["enum"],
        json!(["@a1", "@a2", "none"])
    );
    assert_eq!(
        target["properties"]["probabilities"]["additionalProperties"],
        false
    );
    assert_eq!(
        target["properties"]["probabilities"]["required"],
        json!(["@a1", "@a2", "none"])
    );
}
