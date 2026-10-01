use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::*;

const MAX_KEY_BYTES: usize = 64;
const NONE_RUBRIC: &str = "None of these answers the question.";
const PROBABILITY_SUM_TOLERANCE: f64 = 0.02;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuestionKind {
    Noul,
    Choice,
    Score,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    Noul {
        instructions: Value,
        criteria: Option<NoulCriteria>,
    },
    Choice {
        instructions: Value,
        criteria: BTreeMap<String, Value>,
    },
    Score {
        instructions: Value,
        criteria: Vec<Value>,
    },
}

#[derive(Clone, Deserialize, Serialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub when_true: Value,
    #[serde(rename = "false")]
    pub when_false: Value,
}

impl Question {
    pub fn noul(instructions: Value, criteria: Option<NoulCriteria>) -> Self {
        Self::Noul {
            instructions,
            criteria,
        }
    }

    /// Reserves the abstention key; callers cannot redefine its meaning.
    pub fn choice(
        instructions: Value,
        mut criteria: BTreeMap<String, Value>,
    ) -> Result<Self, ContractError> {
        if criteria.contains_key(NONE_OPTION) {
            return Err(ContractError::Options);
        }
        criteria.insert(NONE_OPTION.into(), Value::String(NONE_RUBRIC.into()));
        let question = Self::Choice {
            instructions,
            criteria,
        };
        question.validate()?;
        Ok(question)
    }

    pub fn score(instructions: Value, criteria: Vec<Value>) -> Self {
        Self::Score {
            instructions,
            criteria,
        }
    }

    pub fn kind(&self) -> QuestionKind {
        match self {
            Self::Noul { .. } => QuestionKind::Noul,
            Self::Choice { .. } => QuestionKind::Choice,
            Self::Score { .. } => QuestionKind::Score,
        }
    }

    fn validate(&self) -> Result<(), ContractError> {
        let instructions = match self {
            Self::Noul {
                instructions,
                criteria,
            } => {
                if criteria
                    .as_ref()
                    .is_some_and(|rubric| !entry(&rubric.when_true) || !entry(&rubric.when_false))
                {
                    return Err(ContractError::Question);
                }
                instructions
            }
            Self::Choice {
                instructions,
                criteria,
            } => {
                if criteria.len() < 2
                    || criteria.len() > MAX_CHOICE_OPTIONS
                    || criteria.get(NONE_OPTION) != Some(&Value::String(NONE_RUBRIC.into()))
                    || criteria
                        .iter()
                        .any(|(key, value)| !valid_key(key) || !entry(value))
                {
                    return Err(ContractError::Options);
                }
                instructions
            }
            Self::Score {
                instructions,
                criteria,
            } => {
                if !(2..=MAX_SCORE_LEVELS).contains(&criteria.len())
                    || criteria.iter().any(|level| !entry(level))
                {
                    return Err(ContractError::Options);
                }
                instructions
            }
        };
        if !entry(instructions) || instructions.is_null() {
            return Err(ContractError::Question);
        }
        Ok(())
    }
}

fn entry(value: &Value) -> bool {
    matches!(
        value,
        Value::String(_) | Value::Object(_) | Value::Array(_) | Value::Null
    )
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_BYTES
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'@'))
}

/// Immutable bounded question set. Its caller must separately admit disclosure.
#[derive(Clone, Serialize)]
pub struct DecisionRequest {
    state: Value,
    model: &'static str,
    questions: BTreeMap<String, Question>,
    #[serde(skip)]
    digest: [u8; 32],
}

impl DecisionRequest {
    pub fn try_new(
        state: Value,
        questions: BTreeMap<String, Question>,
    ) -> Result<Self, ContractError> {
        if !matches!(state, Value::String(_) | Value::Object(_) | Value::Array(_)) {
            return Err(ContractError::State);
        }
        let state_bytes = encoded_len(&state)?;
        if state_bytes > MAX_STATE_BYTES || questions.is_empty() || questions.len() > MAX_QUESTIONS
        {
            return Err(ContractError::Capacity);
        }
        for (key, question) in &questions {
            if !valid_key(key) {
                return Err(ContractError::Question);
            }
            question.validate()?;
            if state_bytes + encoded_len(question)? > MAX_STATE_AND_QUESTION_BYTES {
                return Err(ContractError::Capacity);
            }
        }
        let mut request = Self {
            state,
            model: JEV_MODEL,
            questions,
            digest: [0; 32],
        };
        let encoded = request.encode()?;
        if encoded.len() > MAX_REQUEST_BYTES {
            return Err(ContractError::Capacity);
        }
        request.digest = Sha256::digest(&encoded).into();
        Ok(request)
    }

    pub fn state(&self) -> &Value {
        &self.state
    }
    pub fn questions(&self) -> &BTreeMap<String, Question> {
        &self.questions
    }

    /// Narrows a fallback call without altering its state or any rubric.
    pub fn subset<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, ContractError> {
        let mut questions = BTreeMap::new();
        for key in keys {
            let question = self.questions.get(key).ok_or(ContractError::Question)?;
            if questions.insert(key.to_owned(), question.clone()).is_some() {
                return Err(ContractError::Question);
            }
        }
        Self::try_new(self.state.clone(), questions)
    }

    pub fn encode(&self) -> Result<Vec<u8>, ContractError> {
        serde_json::to_vec(self).map_err(|_| ContractError::Wire)
    }

    pub fn state_bytes(&self) -> usize {
        // Construction has already serialized this immutable JSON value.
        encoded_len(&self.state).unwrap_or(MAX_STATE_BYTES)
    }

    /// Exact per-question schema for OpenAI structured-output emulation.
    pub fn answer_schema(&self) -> Value {
        crate::schema::answers(&self.questions)
    }

    /// Envelope failures reject the call; malformed individual answers reject
    /// only that question, so good speculative heads do not need another call.
    pub fn decode(&self, bytes: &[u8]) -> Result<DecisionResponse, ContractError> {
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(ContractError::Capacity);
        }
        let wire: WireResponse =
            serde_json::from_value(crate::wire::decode(bytes)?).map_err(|_| ContractError::Wire)?;
        if wire.model != JEV_MODEL {
            return Err(ContractError::Model);
        }
        self.validate_answers(wire.answers, wire.usage)
    }

    /// Emulation returns the same answers without claiming a Jev model identity.
    /// Usage comes from the validated OpenAI transport terminal.
    pub fn decode_emulation(
        &self,
        bytes: &[u8],
        usage: DecisionUsage,
    ) -> Result<DecisionResponse, ContractError> {
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(ContractError::Capacity);
        }
        let wire: WireEmulation =
            serde_json::from_value(crate::wire::decode(bytes)?).map_err(|_| ContractError::Wire)?;
        self.validate_answers(wire.answers, usage)
    }

    fn validate_answers(
        &self,
        answers: BTreeMap<String, Value>,
        usage: DecisionUsage,
    ) -> Result<DecisionResponse, ContractError> {
        if answers.keys().any(|key| !self.questions.contains_key(key)) {
            return Err(ContractError::Question);
        }
        Ok(DecisionResponse {
            usage,
            answers: self
                .questions
                .iter()
                .map(|(key, question)| {
                    let answer = answers
                        .get(key)
                        .ok_or(ContractError::MissingAnswer)
                        .and_then(|value| validate_answer(question, value))
                        .map(|mut answer| {
                            answer.1 = self.answer_binding(key);
                            answer
                        });
                    (key.clone(), answer)
                })
                .collect(),
        })
    }

    fn answer_binding(&self, key: &str) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"zephium-decision-answer-v1\0");
        digest.update(self.digest);
        digest.update([0]);
        digest.update(key.as_bytes());
        digest.finalize().into()
    }

    /// An answer from another question, rubric, state or batch cannot be reused.
    pub fn accepts(&self, key: &str, answer: &Answer) -> bool {
        self.questions.contains_key(key) && answer.1 == self.answer_binding(key)
    }
}

fn encoded_len(value: &impl Serialize) -> Result<usize, ContractError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| ContractError::Wire)
}

impl fmt::Debug for DecisionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecisionRequest")
            .field("model", &self.model)
            .field("state_bytes", &self.state_bytes())
            .field("question_count", &self.questions.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DecisionUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

pub struct DecisionResponse {
    pub usage: DecisionUsage,
    pub answers: BTreeMap<String, Result<Answer, ContractError>>,
}

/// Constructed only after validation against the exact offered question.
pub struct Answer(AnswerValue, [u8; 32]);

impl Answer {
    pub fn value(&self) -> &AnswerValue {
        &self.0
    }
    pub fn kind(&self) -> QuestionKind {
        match self.0 {
            AnswerValue::Noul { .. } => QuestionKind::Noul,
            AnswerValue::Choice { .. } => QuestionKind::Choice,
            AnswerValue::Score { .. } => QuestionKind::Score,
        }
    }
}

// Page-derived keys and score legends deliberately have no Debug implementation.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnswerValue {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        confidence: f64,
        probabilities: BTreeMap<String, f64>,
    },
    Score {
        score: f64,
        confidence: f64,
        probabilities: BTreeMap<String, f64>,
        legend: BTreeMap<String, Value>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireResponse {
    model: String,
    answers: BTreeMap<String, Value>,
    usage: DecisionUsage,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireEmulation {
    answers: BTreeMap<String, Value>,
}

fn probability(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn distribution<'a>(
    probabilities: &BTreeMap<String, f64>,
    keys: impl Iterator<Item = &'a String>,
) -> bool {
    probabilities.keys().eq(keys)
        && probabilities.values().all(|value| probability(*value))
        && (probabilities.values().sum::<f64>() - 1.0).abs() <= PROBABILITY_SUM_TOLERANCE
}

fn validate_answer(question: &Question, value: &Value) -> Result<Answer, ContractError> {
    let answer: AnswerValue =
        serde_json::from_value(value.clone()).map_err(|_| ContractError::Answer)?;
    let valid = match (question, &answer) {
        (Question::Noul { .. }, AnswerValue::Noul { noul }) => probability(*noul),
        (
            Question::Choice { criteria, .. },
            AnswerValue::Choice {
                choice,
                confidence,
                probabilities,
            },
        ) => {
            probability(*confidence)
                && distribution(probabilities, criteria.keys())
                && probabilities
                    .get(choice)
                    .is_some_and(|selected| probabilities.values().all(|p| p <= selected))
        }
        (
            Question::Score { criteria, .. },
            AnswerValue::Score {
                score,
                confidence,
                probabilities,
                legend,
            },
        ) => {
            let expected: BTreeMap<_, _> = criteria
                .iter()
                .enumerate()
                .map(|(i, text)| (i.to_string(), text.clone()))
                .collect();
            let weighted = criteria
                .iter()
                .enumerate()
                .map(|(i, _)| probabilities.get(&i.to_string()).copied().unwrap_or(0.0) * i as f64)
                .sum::<f64>();
            probability(*confidence)
                && score.is_finite()
                && *score >= 0.0
                && *score <= (criteria.len() - 1) as f64
                && legend == &expected
                && distribution(probabilities, expected.keys())
                && (*score - weighted).abs() <= PROBABILITY_SUM_TOLERANCE
        }
        _ => false,
    };
    if valid {
        Ok(Answer(answer, [0; 32]))
    } else {
        Err(ContractError::Answer)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ContractError {
    #[error("decision state is invalid")]
    State,
    #[error("decision question is invalid")]
    Question,
    #[error("decision options are invalid")]
    Options,
    #[error("decision budget exceeded")]
    Capacity,
    #[error("decision wire contract is invalid")]
    Wire,
    #[error("decision model is mismatched")]
    Model,
    #[error("decision answer is missing")]
    MissingAnswer,
    #[error("decision answer is invalid")]
    Answer,
}

#[cfg(test)]
mod tests;
