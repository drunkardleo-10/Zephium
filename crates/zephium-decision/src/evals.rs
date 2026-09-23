//! Recorded public observations. Live runners print aggregate facts only.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

use crate::{AnswerValue, ContractError, DecisionRequest, DecisionResponse, Question, JEV_MODEL};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expected {
    Noul { positive: bool },
    Choice { accepted: Vec<String> },
    Score { level: u16 },
}

pub struct EvalFixture {
    pub id: &'static str,
    pub request: DecisionRequest,
    pub expected: BTreeMap<String, Expected>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recorded {
    id: String,
    source_sha256: String,
    request: Request,
    expected: BTreeMap<String, Expected>,
    unscored: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    state: Value,
    model: String,
    questions: BTreeMap<String, Question>,
}

pub fn fixtures() -> Result<Vec<EvalFixture>, ContractError> {
    let sources = [
        (
            "lego_product_01",
            include_str!("../evals/lego_product_01.json"),
        ),
        ("yc_about_01", include_str!("../evals/yc_about_01.json")),
        (
            "yc_about_01_json",
            include_str!("../evals/yc_about_01_json.json"),
        ),
        (
            "airbnb_search_01",
            include_str!("../evals/airbnb_search_01.json"),
        ),
        (
            "cloudflare_government_01",
            include_str!("../evals/cloudflare_government_01.json"),
        ),
        ("yc_read_01", include_str!("../evals/yc_read_01.json")),
        (
            "yc_read_optional_01",
            include_str!("../evals/yc_read_optional_01.json"),
        ),
        (
            "search_flow_01",
            include_str!("../evals/search_flow_01.json"),
        ),
        (
            "search_flow_unrelated_01",
            include_str!("../evals/search_flow_unrelated_01.json"),
        ),
        (
            "search_mixed_01",
            include_str!("../evals/search_mixed_01.json"),
        ),
        (
            "catalog_tower_bridge_01",
            include_str!("../evals/catalog_tower_bridge_01.json"),
        ),
        (
            "catalog_absent_01",
            include_str!("../evals/catalog_absent_01.json"),
        ),
        (
            "book_product_01",
            include_str!("../evals/book_product_01.json"),
        ),
        (
            "jacket_product_01",
            include_str!("../evals/jacket_product_01.json"),
        ),
        (
            "phone_product_01",
            include_str!("../evals/phone_product_01.json"),
        ),
        (
            "airbnb_listing_01",
            include_str!("../evals/airbnb_listing_01.json"),
        ),
        ("lego_gate_01", include_str!("../evals/lego_gate_01.json")),
        (
            "ikea_consent_01",
            include_str!("../evals/ikea_consent_01.json"),
        ),
        (
            "zalando_consent_01",
            include_str!("../evals/zalando_consent_01.json"),
        ),
        (
            "lego_catalog_01",
            include_str!("../evals/lego_catalog_01.json"),
        ),
        (
            "lego_catalog_cells_01",
            include_str!("../evals/lego_catalog_cells_01.json"),
        ),
        (
            "book_catalog_01",
            include_str!("../evals/book_catalog_01.json"),
        ),
        (
            "lego_product_specs_01",
            include_str!("../evals/lego_product_specs_01.json"),
        ),
    ];
    sources
        .into_iter()
        .map(|(id, bytes)| {
            let recorded: Recorded =
                serde_json::from_str(bytes).map_err(|_| ContractError::Wire)?;
            if recorded.id != id
                || recorded.request.model != JEV_MODEL
                || recorded.source_sha256.len() != 64
                || !recorded
                    .source_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || recorded.expected.len() + recorded.unscored.len()
                    != recorded.request.questions.len()
                || recorded
                    .expected
                    .keys()
                    .chain(&recorded.unscored)
                    .any(|key| !recorded.request.questions.contains_key(key))
                || recorded
                    .unscored
                    .iter()
                    .any(|key| recorded.expected.contains_key(key))
                || recorded.unscored.iter().collect::<BTreeSet<_>>().len()
                    != recorded.unscored.len()
                || recorded.expected.iter().any(|(key, expected)| {
                    match (recorded.request.questions.get(key), expected) {
                        (Some(Question::Noul { .. }), Expected::Noul { .. }) => false,
                        (
                            Some(Question::Choice { criteria, .. }),
                            Expected::Choice { accepted },
                        ) => {
                            accepted.is_empty()
                                || accepted.iter().collect::<BTreeSet<_>>().len() != accepted.len()
                                || accepted.iter().any(|key| !criteria.contains_key(key))
                        }
                        (Some(Question::Score { criteria, .. }), Expected::Score { level }) => {
                            usize::from(*level) >= criteria.len()
                        }
                        _ => true,
                    }
                })
            {
                return Err(ContractError::Wire);
            }
            let request =
                DecisionRequest::try_new(recorded.request.state, recorded.request.questions)?;
            Ok(EvalFixture {
                id,
                request,
                expected: recorded.expected,
            })
        })
        .collect()
}

/// Separate accumulators are required for Noul, Choice and Score.
#[derive(Clone, Copy, Debug, Default)]
pub struct EvalMetrics {
    pub total: u32,
    pub valid: u32,
    pub correct: u32,
    /// Probability of the labeled event against its binary outcome.
    pub brier_sum: f64,
    pub confidence_sum: f64,
    /// Ten fixed bins over the probability of the selected answer.
    pub calibration: [CalibrationBin; 10],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CalibrationBin {
    pub count: u32,
    pub correct: u32,
    pub probability_sum: f64,
}

impl EvalMetrics {
    pub fn accuracy(&self) -> f64 {
        f64::from(self.correct) / f64::from(self.total.max(1))
    }
    pub fn brier(&self) -> f64 {
        self.brier_sum / f64::from(self.valid.max(1))
    }
    pub fn calibration_error(&self) -> f64 {
        self.calibration
            .iter()
            .map(|bin| (f64::from(bin.correct) - bin.probability_sum).abs())
            .sum::<f64>()
            / f64::from(self.valid.max(1))
    }
}

pub fn measure(fixture: &EvalFixture, response: &DecisionResponse) -> [EvalMetrics; 3] {
    measure_selected(fixture, response, |_| true)
}

pub fn measure_question(
    fixture: &EvalFixture,
    response: &DecisionResponse,
    key: &str,
) -> [EvalMetrics; 3] {
    measure_selected(fixture, response, |candidate| candidate == key)
}

fn measure_selected(
    fixture: &EvalFixture,
    response: &DecisionResponse,
    include: impl Fn(&str) -> bool,
) -> [EvalMetrics; 3] {
    let mut result = [EvalMetrics::default(); 3];
    for (key, expected) in &fixture.expected {
        if !include(key) {
            continue;
        }
        let index = match expected {
            Expected::Noul { .. } => 0,
            Expected::Choice { .. } => 1,
            Expected::Score { .. } => 2,
        };
        let metrics = &mut result[index];
        metrics.total += 1;
        let Some(Ok(answer)) = response.answers.get(key) else {
            continue;
        };
        if !fixture.request.accepts(key, answer) {
            continue;
        }
        let (correct, event_probability, confidence, selected_probability) =
            match (expected, answer.value()) {
                (Expected::Noul { positive }, AnswerValue::Noul { noul }) => (
                    (*noul >= 0.5) == *positive,
                    if *positive { *noul } else { 1.0 - noul },
                    noul.max(1.0 - noul),
                    noul.max(1.0 - noul),
                ),
                (
                    Expected::Choice { accepted },
                    AnswerValue::Choice {
                        choice,
                        confidence,
                        probabilities,
                    },
                ) => (
                    accepted.contains(choice),
                    accepted
                        .iter()
                        .filter_map(|key| probabilities.get(key))
                        .sum(),
                    *confidence,
                    probabilities.get(choice).copied().unwrap_or(0.0),
                ),
                (
                    Expected::Score { level },
                    AnswerValue::Score {
                        score,
                        confidence,
                        probabilities,
                        ..
                    },
                ) => (
                    score.round() == f64::from(*level),
                    probabilities
                        .get(&level.to_string())
                        .copied()
                        .unwrap_or(0.0),
                    *confidence,
                    probabilities
                        .get(&(score.round() as u16).to_string())
                        .copied()
                        .unwrap_or(0.0),
                ),
                _ => continue,
            };
        metrics.valid += 1;
        metrics.correct += u32::from(correct);
        metrics.brier_sum += (1.0 - event_probability).powi(2);
        metrics.confidence_sum += confidence;
        let bin = &mut metrics.calibration[((selected_probability * 10.0) as usize).min(9)];
        bin.count += 1;
        bin.correct += u32::from(correct);
        bin.probability_sum += selected_probability;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recorded_lego_request_retains_all_heads_and_explicit_abstention() {
        let fixtures = fixtures().unwrap();
        let lego = &fixtures[0];
        assert_eq!(lego.request.questions().len(), 7);
        assert_eq!(lego.expected.len(), 5);
        for question in lego.request.questions().values() {
            if let Question::Choice { criteria, .. } = question {
                assert!(criteria.contains_key("none"));
            }
        }
        assert!(lego.request.state_bytes() <= crate::MAX_STATE_BYTES);
    }

    #[test]
    fn calibration_uses_selected_probability_instead_of_distribution_confidence() {
        let request = DecisionRequest::try_new(
            json!("public fixture"),
            BTreeMap::from([(
                "target".into(),
                Question::choice(
                    json!("Target?"),
                    BTreeMap::from([("present".into(), Value::Null)]),
                )
                .unwrap(),
            )]),
        )
        .unwrap();
        let response = request.decode_emulation(br#"{"answers":{"target":{"type":"choice","choice":"present","confidence":1.0,"probabilities":{"present":0.6,"none":0.4}}}}"#, crate::DecisionUsage::default()).unwrap();
        let fixture = EvalFixture {
            id: "calibration",
            request,
            expected: BTreeMap::from([(
                "target".into(),
                Expected::Choice {
                    accepted: vec!["none".into()],
                },
            )]),
        };
        let metric = measure(&fixture, &response)[1];
        assert_eq!(metric.correct, 0);
        assert_eq!(metric.calibration[6].count, 1);
        assert_eq!(metric.calibration[9].count, 0);
        assert!((metric.calibration_error() - 0.6).abs() < 1e-9);
        assert!((metric.brier() - 0.36).abs() < 1e-9);
    }
}
