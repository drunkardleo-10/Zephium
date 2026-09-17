//! Provider-neutral, one-call authoring. Disclosure contains only explicit Work
//! context; the result is a proposal and cannot approve or execute anything.
use super::{proposal::*, *};
use std::{future::Future, pin::Pin};

pub const MAX_PLANNING_CONTEXT_BYTES: usize = 32 * 1024;
pub const MAX_PLANNING_OUTPUT_BYTES: usize = 128 * 1024;

#[derive(Clone, Serialize)]
pub struct PlanningAnswer {
    pub question: String,
    pub answer: String,
}
#[derive(Clone, Serialize)]
pub struct PlanningContext {
    pub objective: String,
    pub answers: Vec<PlanningAnswer>,
    pub current_draft: Option<WorkPlanProposal>,
    /// Admitted canvas objects the user selected as context, bodies included.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<super::context::WorkContextBody>,
}
/// Rust owns the source join. Only `context()` is provider-facing; no profile,
/// Work, plan, question, or node identity is serialized into a model request.
pub struct WorkPlanningDisclosure {
    profile: ProfileId,
    work: WorkId,
    revision: WorkRevision,
    context: PlanningContext,
    admitted: Option<super::context::WorkContextDisclosureV1>,
}
impl WorkPlanningDisclosure {
    pub fn from_snapshot(snapshot: &WorkSnapshot) -> Result<Self, WorkPlanningError> {
        Self::from_snapshot_with_context(snapshot, None)
    }
    /// Adds admitted canvas context inside the same byte ceiling.
    pub fn from_snapshot_with_context(
        snapshot: &WorkSnapshot,
        admitted: Option<&super::context::WorkAdmittedContext>,
    ) -> Result<Self, WorkPlanningError> {
        snapshot.validate().map_err(WorkPlanningError::Store)?;
        if snapshot.lifecycle != WorkLifecycle::Active {
            return Err(WorkPlanningError::Stale);
        }
        if snapshot.status == WorkAuthoringStatus::NeedsInput {
            return Err(WorkPlanningError::NeedsInput);
        }
        let mut bytes = snapshot.objective.len() + admitted.map_or(0, |a| a.bytes());
        for question in snapshot.current_questions() {
            if let Some(answer) = &question.answer {
                bytes += question.prompt.len() + answer.len();
            }
        }
        if let Some(plan) = &snapshot.plan {
            for node in &plan.draft.nodes {
                bytes += node.objective.len();
                for output in &node.outputs {
                    bytes += output.name.len() + output.description.len();
                }
            }
        }
        if bytes > MAX_PLANNING_CONTEXT_BYTES {
            return Err(WorkPlanningError::Capacity);
        }
        let answers: Vec<_> = snapshot
            .current_questions()
            .filter_map(|q| {
                q.answer.as_ref().map(|answer| PlanningAnswer {
                    question: q.prompt.clone(),
                    answer: answer.clone(),
                })
            })
            .collect();
        let current_draft = snapshot
            .plan
            .as_ref()
            .map(|plan| {
                let nodes = &plan.draft.nodes;
                let nodes = nodes
                    .iter()
                    .enumerate()
                    .map(|(key, node)| {
                        Ok(WorkNodeProposal {
                            key: key as u8,
                            objective: node.objective.clone(),
                            outputs: node.outputs.clone(),
                            dependencies: node
                                .dependencies
                                .iter()
                                .map(|id| {
                                    plan.draft
                                        .nodes
                                        .iter()
                                        .position(|n| n.id == *id)
                                        .map(|p| p as u8)
                                        .ok_or(WorkPlanningError::Invalid)
                                })
                                .collect::<Result<_, _>>()?,
                        })
                    })
                    .collect::<Result<_, WorkPlanningError>>()?;
                Ok::<_, WorkPlanningError>(WorkPlanProposal { nodes })
            })
            .transpose()?;
        let context = PlanningContext {
            objective: snapshot.objective.clone(),
            answers,
            current_draft,
            context: admitted.map(|a| a.bodies.clone()).unwrap_or_default(),
        };
        Ok(Self {
            profile: snapshot.profile,
            work: snapshot.id,
            revision: snapshot.revision,
            context,
            admitted: admitted.map(|a| a.disclosure.clone()),
        })
    }
    pub fn context(&self) -> &PlanningContext {
        &self.context
    }
    /// The manifest to persist with whatever this disclosure produced.
    pub fn admitted(&self) -> Option<&super::context::WorkContextDisclosureV1> {
        self.admitted.as_ref()
    }
    pub fn profile(&self) -> ProfileId {
        self.profile
    }
    pub fn work(&self) -> WorkId {
        self.work
    }
    pub fn revision(&self) -> WorkRevision {
        self.revision
    }
}

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkPlanningProposal {
    Clarify {
        prompt: String,
        options: Vec<String>,
    },
    Draft {
        plan: WorkPlanProposal,
    },
}
impl WorkPlanningProposal {
    pub fn validate(&self) -> Result<(), WorkPlanningError> {
        match self {
            Self::Clarify { prompt, options } => {
                validate_text(prompt, MAX_WORK_TEXT_BYTES).map_err(WorkPlanningError::Store)?;
                if options.len() > 8 {
                    return Err(WorkPlanningError::Invalid);
                }
                let mut unique = BTreeSet::new();
                for option in options {
                    validate_text(option, 512).map_err(WorkPlanningError::Store)?;
                    if !unique.insert(option) {
                        return Err(WorkPlanningError::Invalid);
                    }
                }
                Ok(())
            }
            Self::Draft { plan } => plan.validate().map_err(WorkPlanningError::Store),
        }
    }
    /// Trusted application use only, after the provider result is validated.
    pub fn into_edit(self) -> Result<WorkEdit, WorkPlanningError> {
        self.into_edit_disclosed(None)
    }
    /// Binds the admitted-context manifest to an accepted draft.
    pub fn into_edit_disclosed(
        self,
        context: Option<super::context::WorkContextDisclosureV1>,
    ) -> Result<WorkEdit, WorkPlanningError> {
        self.validate()?;
        Ok(match self {
            Self::Clarify { prompt, options } => WorkEdit::OpenQuestion {
                id: WorkQuestionId::generate(),
                prompt,
                options,
            },
            Self::Draft { plan } => {
                let draft = plan.mint().map_err(WorkPlanningError::Store)?;
                match context {
                    Some(context) => WorkEdit::ReplaceDraftDisclosed { draft, context },
                    None => WorkEdit::ReplaceDraft { draft },
                }
            }
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkPlanningError {
    Invalid,
    Capacity,
    Unavailable,
    Cancelled,
    Timeout,
    Stale,
    NeedsInput,
    Privacy,
    /// Generation may have been billed. No proposal is accepted and no retry
    /// happens automatically, including after lost or malformed responses.
    ProviderOutcomeUnknown,
    /// The call was dispatched and no readable terminal arrived in time. Its
    /// ceiling is charged, so a caller may try again within its budget.
    ProviderStalled(WorkPlanningUsage),
    ProviderRefused(WorkPlanningUsage),
    Store(WorkError),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkPlanningUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost_ceiling_micro_usd: u64,
}
pub struct WorkPlanningResult {
    pub proposal: WorkPlanningProposal,
    pub usage: WorkPlanningUsage,
}
pub type WorkPlanningFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WorkPlanningResult, WorkPlanningError>> + Send + 'a>>;
/// Implementations own configured routing and budgets. Dropping the future
/// must stop their local I/O; they must never auto-retry a generation.
pub type WorkExecutionPlanningFuture<'a> = Pin<
    Box<
        dyn Future<
                Output = Result<
                    super::execution_proposal::WorkExecutionPlanningResult,
                    WorkPlanningError,
                >,
            > + Send
            + 'a,
    >,
>;
pub trait WorkPlanningProvider: Send + Sync {
    fn propose_execution(&self, _input: WorkPlanningDisclosure) -> WorkExecutionPlanningFuture<'_> {
        Box::pin(async { Err(WorkPlanningError::Unavailable) })
    }
    fn propose(&self, input: WorkPlanningDisclosure) -> WorkPlanningFuture<'_>;
}
impl std::fmt::Debug for WorkPlanningDisclosure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkPlanningDisclosure([redacted])")
    }
}
impl std::fmt::Debug for WorkPlanningProposal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkPlanningProposal([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn work() -> WorkSnapshot {
        WorkSnapshot::create(10.into(), 11.into(), "Compare libraries".into()).unwrap()
    }
    fn apply(work: WorkSnapshot, edit: WorkEdit) -> WorkSnapshot {
        work.apply(work.revision, edit, WorkAuthor::User).unwrap().0
    }
    #[test]
    fn planning_discloses_only_current_answers_and_remaps_durable_graph_ids() {
        let work = apply(
            work(),
            WorkEdit::OpenQuestion {
                id: 12.into(),
                prompt: "Which language?".into(),
                options: vec![],
            },
        );
        assert!(matches!(
            WorkPlanningDisclosure::from_snapshot(&work),
            Err(WorkPlanningError::NeedsInput)
        ));
        let work = apply(
            work,
            WorkEdit::AnswerQuestion {
                id: 12.into(),
                answer: "Rust".into(),
            },
        );
        let disclosure = WorkPlanningDisclosure::from_snapshot(&work).unwrap();
        assert_eq!(disclosure.context().answers[0].answer, "Rust");
        let work = apply(
            work,
            WorkEdit::SetObjective {
                objective: "Design a garden".into(),
            },
        );
        let work = apply(
            work,
            WorkEdit::ReplaceDraft {
                draft: WorkPlanDraft {
                    id: 13.into(),
                    nodes: vec![WorkPlanNode {
                        id: 14.into(),
                        objective: "Measure".into(),
                        dependencies: vec![],
                        outputs: vec![WorkExpectedOutput {
                            name: "dimensions".into(),
                            description: "Measured boundaries".into(),
                            review: WorkOutputReview::UserAcceptance,
                        }],
                    }],
                },
            },
        );
        let disclosure = WorkPlanningDisclosure::from_snapshot(&work).unwrap();
        assert!(disclosure.context().answers.is_empty());
        assert_eq!(
            disclosure.context().current_draft.as_ref().unwrap().nodes[0].key,
            0
        );
        let encoded = serde_json::to_string(disclosure.context()).unwrap();
        for forbidden in [
            "profile",
            "revision",
            "schema_version",
            "author",
            "Which language?",
            "Rust",
        ] {
            assert!(!encoded.contains(forbidden));
        }
        let work = apply(work, WorkEdit::Archive);
        assert!(matches!(
            WorkPlanningDisclosure::from_snapshot(&work),
            Err(WorkPlanningError::Stale)
        ));
    }
    #[test]
    fn planning_rejects_oversized_context_and_malformed_proposals_before_minting() {
        let mut work = work();
        for n in 0..5 {
            work = apply(
                work,
                WorkEdit::OpenQuestion {
                    id: (100 + n).into(),
                    prompt: "Details?".into(),
                    options: vec![],
                },
            );
            work = apply(
                work,
                WorkEdit::AnswerQuestion {
                    id: (100 + n).into(),
                    answer: "a".repeat(MAX_WORK_TEXT_BYTES),
                },
            );
        }
        assert!(matches!(
            WorkPlanningDisclosure::from_snapshot(&work),
            Err(WorkPlanningError::Capacity)
        ));
        for proposal in [
            WorkPlanningProposal::Clarify {
                prompt: "?".into(),
                options: vec!["same".into(); 2],
            },
            WorkPlanningProposal::Draft {
                plan: WorkPlanProposal { nodes: vec![] },
            },
        ] {
            assert!(proposal.into_edit().is_err());
        }
        let proposal = WorkPlanningProposal::Clarify {
            prompt: "Which audience?".into(),
            options: vec![],
        };
        let WorkEdit::OpenQuestion { id: first, .. } = proposal.clone().into_edit().unwrap() else {
            panic!()
        };
        let WorkEdit::OpenQuestion { id: second, .. } = proposal.into_edit().unwrap() else {
            panic!()
        };
        assert_ne!(first, second);
    }
}
