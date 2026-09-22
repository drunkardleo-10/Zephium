use std::collections::BTreeMap;

use crate::{
    Answer, AnswerValue, ContractError, DecisionRequest, DecisionResponse, QuestionKind,
    NONE_OPTION,
};

/// Threshold revision is coupled to the pinned model and recorded evaluations.
pub const CONFIDENCE_POLICY_REVISION: &str = "jev-1.13.0-conservative-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionPurpose {
    Challenge,
    Action,
    Locate,
    Picture,
    Relevance,
    Wall,
    Completion,
    OrderedScore,
}

impl DecisionPurpose {
    fn kind(self) -> QuestionKind {
        match self {
            Self::Challenge | Self::Relevance | Self::Completion => QuestionKind::Noul,
            Self::Action | Self::Locate | Self::Picture | Self::Wall => QuestionKind::Choice,
            Self::OrderedScore => QuestionKind::Score,
        }
    }

    fn confident(self, answer: &Answer) -> bool {
        match (self, answer.value()) {
            (Self::Challenge, AnswerValue::Noul { noul }) => *noul <= 0.05 || *noul >= 0.95,
            (Self::Relevance, AnswerValue::Noul { noul }) => *noul <= 0.20 || *noul >= 0.80,
            (Self::Completion, AnswerValue::Noul { noul }) => *noul <= 0.05 || *noul >= 0.95,
            (
                purpose,
                AnswerValue::Choice {
                    choice,
                    confidence,
                    probabilities,
                },
            ) => {
                let threshold = match purpose {
                    Self::Action => 0.98,
                    Self::Locate | Self::Wall => 0.95,
                    Self::Picture => 0.80,
                    _ => return false,
                };
                *confidence >= threshold
                    && probabilities.get(choice).is_some_and(|p| *p >= threshold)
            }
            (Self::OrderedScore, AnswerValue::Score { confidence, .. }) => *confidence >= 0.95,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FallbackReason {
    Unavailable,
    RateLimited,
    InvalidAnswer,
    LowConfidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnswerBackend {
    Primary,
    Emulation,
}

pub enum ResolvedDecision {
    Answer {
        answer: Answer,
        backend: AnswerBackend,
    },
    Abstained {
        backend: AnswerBackend,
    },
    Unresolved {
        reason: FallbackReason,
    },
}

/// Owns accepted heads while only failed or uncertain questions are retried.
/// The final emulation failure remains unresolved; it cannot become an action.
pub struct DecisionFallback {
    fallback: Option<DecisionRequest>,
    purposes: BTreeMap<String, DecisionPurpose>,
    reasons: BTreeMap<String, FallbackReason>,
    resolved: BTreeMap<String, ResolvedDecision>,
}

impl DecisionFallback {
    pub fn assess(
        request: &DecisionRequest,
        purposes: BTreeMap<String, DecisionPurpose>,
        primary: Result<DecisionResponse, FallbackReason>,
    ) -> Result<Self, ContractError> {
        if !request.questions().keys().eq(purposes.keys())
            || purposes
                .iter()
                .any(|(key, purpose)| request.questions()[key].kind() != purpose.kind())
        {
            return Err(ContractError::Question);
        }
        let (mut answers, failure) = match primary {
            Ok(response) => (response.answers, FallbackReason::InvalidAnswer),
            Err(reason) => (BTreeMap::new(), reason),
        };
        let mut resolved = BTreeMap::new();
        let mut reasons = BTreeMap::new();
        for (key, purpose) in &purposes {
            match assess_answer(
                request,
                key,
                *purpose,
                answers.remove(key),
                failure,
                AnswerBackend::Primary,
            ) {
                Ok(decision) => {
                    resolved.insert(key.clone(), decision);
                }
                Err(reason) => {
                    reasons.insert(key.clone(), reason);
                }
            }
        }
        let fallback = if reasons.is_empty() {
            None
        } else {
            Some(request.subset(reasons.keys().map(String::as_str))?)
        };
        Ok(Self {
            fallback,
            purposes,
            reasons,
            resolved,
        })
    }

    pub fn request(&self) -> Option<&DecisionRequest> {
        self.fallback.as_ref()
    }
    pub fn reasons(&self) -> &BTreeMap<String, FallbackReason> {
        &self.reasons
    }

    pub fn finish(mut self, emulation: Option<DecisionResponse>) -> DecisionResults {
        let mut answers = emulation
            .map(|response| response.answers)
            .unwrap_or_default();
        if let Some(request) = self.fallback {
            for (key, reason) in self.reasons {
                let decision = assess_answer(
                    &request,
                    &key,
                    self.purposes[&key],
                    answers.remove(&key),
                    reason,
                    AnswerBackend::Emulation,
                )
                .unwrap_or_else(|reason| ResolvedDecision::Unresolved { reason });
                self.resolved.insert(key, decision);
            }
        }
        DecisionResults {
            answers: self.resolved,
        }
    }
}

/// Move-only results: consumption precedes a caller's subsequent mutation.
pub struct DecisionResults {
    answers: BTreeMap<String, ResolvedDecision>,
}

impl DecisionResults {
    pub fn take(&mut self, key: &str) -> Option<ResolvedDecision> {
        self.answers.remove(key)
    }
}

fn assess_answer(
    request: &DecisionRequest,
    key: &str,
    purpose: DecisionPurpose,
    answer: Option<Result<Answer, ContractError>>,
    missing: FallbackReason,
    backend: AnswerBackend,
) -> Result<ResolvedDecision, FallbackReason> {
    let answer = answer
        .ok_or(missing)?
        .map_err(|_| FallbackReason::InvalidAnswer)?;
    if !request.accepts(key, &answer) {
        return Err(FallbackReason::InvalidAnswer);
    }
    if !purpose.confident(&answer) {
        return Err(FallbackReason::LowConfidence);
    }
    if matches!(answer.value(), AnswerValue::Choice { choice, .. } if choice == NONE_OPTION) {
        Ok(ResolvedDecision::Abstained { backend })
    } else {
        Ok(ResolvedDecision::Answer { answer, backend })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DecisionUsage, Question};
    use serde_json::json;

    fn request() -> DecisionRequest {
        DecisionRequest::try_new(
            json!("public state"),
            BTreeMap::from([
                (
                    "challenge".into(),
                    Question::noul(json!("Challenge?"), None),
                ),
                (
                    "target".into(),
                    Question::choice(
                        json!("Target?"),
                        BTreeMap::from([("@a1".into(), json!("public button"))]),
                    )
                    .unwrap(),
                ),
            ]),
        )
        .unwrap()
    }
    fn purposes() -> BTreeMap<String, DecisionPurpose> {
        BTreeMap::from([
            ("challenge".into(), DecisionPurpose::Challenge),
            ("target".into(), DecisionPurpose::Action),
        ])
    }

    #[test]
    fn fallback_preserves_good_heads_and_consumes_answers_once() {
        let request = request();
        let first = request.decode_emulation(&serde_json::to_vec(&json!({"answers":{
            "challenge":{"type":"noul","noul":0.01},
            "target":{"type":"choice","choice":"@a1","confidence":0.99,"probabilities":{"@a1":0.6,"none":0.4}}
        }})).unwrap(), DecisionUsage::default()).unwrap();
        let fallback = DecisionFallback::assess(&request, purposes(), Ok(first)).unwrap();
        assert_eq!(
            fallback.reasons().get("target"),
            Some(&FallbackReason::LowConfidence)
        );
        let narrow = fallback.request().unwrap();
        assert_eq!(narrow.questions().len(), 1);
        assert_eq!(narrow.state(), request.state());
        let second = narrow.decode_emulation(&serde_json::to_vec(&json!({"answers":{
            "target":{"type":"choice","choice":"none","confidence":1.0,"probabilities":{"@a1":0.0,"none":1.0}}
        }})).unwrap(), DecisionUsage::default()).unwrap();
        let mut results = fallback.finish(Some(second));
        assert!(matches!(
            results.take("challenge"),
            Some(ResolvedDecision::Answer {
                backend: AnswerBackend::Primary,
                ..
            })
        ));
        assert!(matches!(
            results.take("target"),
            Some(ResolvedDecision::Abstained {
                backend: AnswerBackend::Emulation
            })
        ));
        assert!(results.take("target").is_none());
    }

    #[test]
    fn answer_from_another_state_cannot_bypass_validation_and_missing_fallback_stays_unresolved() {
        let request = request();
        let other =
            DecisionRequest::try_new(json!("another document"), request.questions().clone())
                .unwrap();
        let first = other
            .decode_emulation(
                br#"{"answers":{"challenge":{"type":"noul","noul":0.0}}}"#,
                DecisionUsage::default(),
            )
            .unwrap();
        let fallback = DecisionFallback::assess(&request, purposes(), Ok(first)).unwrap();
        assert_eq!(fallback.request().unwrap().questions().len(), 2);
        let mut results = fallback.finish(None);
        assert!(matches!(
            results.take("challenge"),
            Some(ResolvedDecision::Unresolved {
                reason: FallbackReason::InvalidAnswer
            })
        ));
    }
}
