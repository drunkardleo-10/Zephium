//! Recorded public observations. Live runners print aggregate facts only.

use std::collections::BTreeMap;

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
    let sources = [(
        "lego_product_01",
        include_str!("../evals/lego_product_01.json"),
    )];
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
}

impl EvalMetrics {
    pub fn accuracy(&self) -> f64 {
        f64::from(self.correct) / f64::from(self.total.max(1))
    }
    pub fn brier(&self) -> f64 {
        self.brier_sum / f64::from(self.valid.max(1))
    }
}

pub fn measure(fixture: &EvalFixture, response: &DecisionResponse) -> [EvalMetrics; 3] {
    let mut result = [EvalMetrics::default(); 3];
    for (key, expected) in &fixture.expected {
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
        let (correct, event_probability, confidence) = match (expected, answer.value()) {
            (Expected::Noul { positive }, AnswerValue::Noul { noul }) => (
                (*noul >= 0.5) == *positive,
                if *positive { *noul } else { 1.0 - noul },
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
                (score - f64::from(*level)).abs() <= 0.5,
                probabilities
                    .get(&level.to_string())
                    .copied()
                    .unwrap_or(0.0),
                *confidence,
            ),
            _ => continue,
        };
        metrics.valid += 1;
        metrics.correct += u32::from(correct);
        metrics.brier_sum += (1.0 - event_probability).powi(2);
        metrics.confidence_sum += confidence;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
