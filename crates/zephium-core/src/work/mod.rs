//! Durable Work facts. Authoring and execution history remain distinct; neither
//! contains a live provider, worker, execution token or native reference.
//! An editable plan describes desired work; compilation is a separate boundary.

pub mod artifact;
mod ids;
pub mod planning;
pub mod port;
pub mod proposal;
pub mod runtime;
pub mod synthesis;
#[cfg(test)]
mod tests;
use crate::ids::ProfileId;
pub use ids::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const WORK_SCHEMA_VERSION: u16 = 2;
pub const MAX_ACTIVE_WORKS_PER_PROFILE: usize = 256;
pub const MAX_WORKS_PER_PROFILE: usize = 512;
pub const MAX_WORK_PLAN_REVISIONS: usize = 32;
pub const MAX_WORK_NODES: usize = 64;
pub const MAX_WORK_QUESTIONS: usize = 32;
pub const MAX_WORK_EVENTS: usize = 2048;
pub const MAX_WORK_TEXT_BYTES: usize = 8192;
pub const MAX_WORK_NODE_BYTES: usize = 16384;
pub const MAX_WORK_REQUEST_BYTES: usize = 262144;
pub const MAX_WORK_PROFILE_BYTES: usize = 41943040;
pub const MAX_WORK_PAGE_SIZE: usize = 32;

/// SQLite- and JavaScript-safe ordered revision. The wire representation is a
/// decimal string, so later revisions cannot be rounded by a frontend number.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[cfg_attr(feature = "ipc-types", specta(type = String))]
pub struct WorkRevision(u64);
impl WorkRevision {
    pub const INITIAL: Self = Self(1);
    pub const fn get(self) -> u64 {
        self.0
    }
    pub fn new(value: u64) -> Option<Self> {
        (value > 0 && value <= i64::MAX as u64).then_some(Self(value))
    }
    pub fn next(self) -> Result<Self, WorkError> {
        Self::new(self.0 + 1).ok_or(WorkError::Capacity)
    }
}
impl Serialize for WorkRevision {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for WorkRevision {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value
            .parse()
            .ok()
            .and_then(Self::new)
            .filter(|revision| revision.0.to_string() == value)
            .ok_or_else(|| serde::de::Error::custom("invalid Work revision"))
    }
}

/// Closed failures deliberately carry no user/model content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkError {
    Invalid,
    Capacity,
    Conflict,
    NotFound,
    ProfileUnavailable,
    Unavailable,
    Shutdown,
    OutcomeUnknown,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkAuthoringStatus {
    Draft,
    NeedsInput,
    PlanReady,
}

/// Attribution is descriptive history, never authorization. Legacy content has
/// unknown attribution; application callers cannot impersonate agent authors.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkAuthor {
    User,
    PrimaryAgent,
    OtherAgent,
    LegacyUnknown,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkLifecycle {
    Active,
    Archived,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkQuestionState {
    Active,
    Answered,
    Superseded,
    Dismissed,
}

/// Requested output review level, never evidence that a requirement was met.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkOutputReview {
    Mechanical,
    SourceMappedNeedsReview,
    UserAcceptance,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkExpectedOutput {
    pub name: String,
    pub description: String,
    pub review: WorkOutputReview,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPlanNode {
    pub id: WorkPlanNodeId,
    pub objective: String,
    pub dependencies: Vec<WorkPlanNodeId>,
    pub outputs: Vec<WorkExpectedOutput>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPlanDraft {
    pub id: WorkPlanId,
    pub nodes: Vec<WorkPlanNode>,
}
impl WorkPlanDraft {
    pub fn validate(&self) -> Result<(), WorkError> {
        if self.nodes.is_empty() || self.nodes.len() > MAX_WORK_NODES {
            return Err(WorkError::Invalid);
        }
        let ids: BTreeSet<_> = self.nodes.iter().map(|n| n.id).collect();
        if ids.len() != self.nodes.len() {
            return Err(WorkError::Invalid);
        }
        let mut bytes = 0usize;
        for node in &self.nodes {
            validate_text(&node.objective, MAX_WORK_TEXT_BYTES)?;
            if node.dependencies.len() > 16 || node.outputs.is_empty() || node.outputs.len() > 8 {
                return Err(WorkError::Invalid);
            }
            let deps: BTreeSet<_> = node.dependencies.iter().copied().collect();
            if deps.len() != node.dependencies.len()
                || deps.contains(&node.id)
                || !deps.is_subset(&ids)
            {
                return Err(WorkError::Invalid);
            }
            bytes += validate_node_content(&node.objective, &node.outputs)?;
        }
        if bytes > MAX_WORK_REQUEST_BYTES / 2 {
            return Err(WorkError::Capacity);
        }
        // Bounded topological validation permits arbitrary presentation order.
        let mut settled = BTreeSet::new();
        loop {
            let before = settled.len();
            for node in &self.nodes {
                if node.dependencies.iter().all(|id| settled.contains(id)) {
                    settled.insert(node.id);
                }
            }
            if settled.len() == ids.len() {
                return Ok(());
            }
            if before == settled.len() {
                return Err(WorkError::Invalid);
            }
        }
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPlanRevision {
    pub author: WorkAuthor,
    pub revision: WorkRevision,
    /// Exact Work context from which the proposal was accepted.
    pub basis_revision: WorkRevision,
    pub draft: WorkPlanDraft,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkQuestion {
    pub basis_revision: Option<WorkRevision>,
    pub objective_revision: Option<WorkRevision>,
    pub state: WorkQuestionState,
    pub author: WorkAuthor,
    pub answer_author: Option<WorkAuthor>,
    pub id: WorkQuestionId,
    pub prompt: String,
    pub options: Vec<String>,
    pub answer: Option<String>,
}
impl WorkQuestion {
    pub fn is_current(&self) -> bool {
        matches!(
            self.state,
            WorkQuestionState::Active | WorkQuestionState::Answered
        )
    }
    pub fn validate(&self) -> Result<(), WorkError> {
        if self.basis_revision.is_none() != self.objective_revision.is_none()
            || (self.basis_revision.is_none()
                && (self.is_current() || self.author != WorkAuthor::LegacyUnknown))
            || self.objective_revision > self.basis_revision
            || self.answer.is_some() != self.answer_author.is_some()
            || (self.state == WorkQuestionState::Active && self.answer.is_some())
            || (self.state == WorkQuestionState::Answered && self.answer.is_none())
        {
            return Err(WorkError::Invalid);
        }
        validate_text(&self.prompt, MAX_WORK_TEXT_BYTES)?;
        if self.options.len() > 8 {
            return Err(WorkError::Invalid);
        }
        let mut options = BTreeSet::new();
        for option in &self.options {
            validate_text(option, 512)?;
            if !options.insert(option) {
                return Err(WorkError::Invalid);
            }
        }
        if let Some(answer) = &self.answer {
            validate_text(answer, MAX_WORK_TEXT_BYTES)?;
        }
        Ok(())
    }
}

/// Bounded full-resynchronization projection. Serialized facts grant nothing.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkSnapshot {
    pub lifecycle: WorkLifecycle,
    pub objective_revision: WorkRevision,
    pub context_revision: WorkRevision,
    pub objective_author: WorkAuthor,
    pub schema_version: u16,
    pub id: WorkId,
    pub profile: ProfileId,
    pub revision: WorkRevision,
    pub objective: String,
    pub status: WorkAuthoringStatus,
    pub plan: Option<WorkPlanRevision>,
    pub questions: Vec<WorkQuestion>,
}
impl WorkSnapshot {
    pub fn create(id: WorkId, profile: ProfileId, objective: String) -> Result<Self, WorkError> {
        validate_text(&objective, MAX_WORK_TEXT_BYTES)?;
        Ok(Self {
            lifecycle: WorkLifecycle::Active,
            objective_revision: WorkRevision::INITIAL,
            context_revision: WorkRevision::INITIAL,
            objective_author: WorkAuthor::User,
            schema_version: WORK_SCHEMA_VERSION,
            id,
            profile,
            objective,
            revision: WorkRevision::INITIAL,
            status: WorkAuthoringStatus::Draft,
            plan: None,
            questions: vec![],
        })
    }
    pub fn validate(&self) -> Result<(), WorkError> {
        if self.schema_version != WORK_SCHEMA_VERSION
            || self.questions.len() > MAX_WORK_QUESTIONS
            || self.objective_revision > self.context_revision
            || self.context_revision > self.revision
        {
            return Err(WorkError::Invalid);
        }
        validate_text(&self.objective, MAX_WORK_TEXT_BYTES)?;
        let mut ids = BTreeSet::new();
        for question in &self.questions {
            question.validate()?;
            if !ids.insert(question.id)
                || question
                    .basis_revision
                    .is_some_and(|basis| basis >= self.revision)
                || (question.is_current()
                    && question.objective_revision != Some(self.objective_revision))
            {
                return Err(WorkError::Invalid);
            }
        }
        if let Some(plan) = &self.plan {
            plan.draft.validate()?;
            if plan.revision > self.revision
                || plan.basis_revision < self.context_revision
                || plan.basis_revision.next()? != plan.revision
                || self
                    .questions
                    .iter()
                    .any(|q| q.state == WorkQuestionState::Active)
            {
                return Err(WorkError::Invalid);
            }
        }
        if self.status != self.derived_status() {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
    fn derived_status(&self) -> WorkAuthoringStatus {
        if self
            .questions
            .iter()
            .any(|q| q.state == WorkQuestionState::Active)
        {
            WorkAuthoringStatus::NeedsInput
        } else if self.plan.is_some() {
            WorkAuthoringStatus::PlanReady
        } else {
            WorkAuthoringStatus::Draft
        }
    }
    /// Only this explicit set is eligible for a future planning request.
    pub fn current_questions(&self) -> impl Iterator<Item = &WorkQuestion> {
        self.questions.iter().filter(|q| q.is_current())
    }
    /// Pure compare-and-set transition. Refusal leaves the original untouched.
    pub fn apply(
        &self,
        expected: WorkRevision,
        edit: WorkEdit,
        author: WorkAuthor,
    ) -> Result<(Self, WorkEventKind), WorkError> {
        self.validate()?;
        if self.revision != expected {
            return Err(WorkError::Conflict);
        }
        edit.validate()?;
        if self.lifecycle == WorkLifecycle::Archived
            && !matches!(edit, WorkEdit::Restore | WorkEdit::CompactHistory)
        {
            return Err(WorkError::Conflict);
        }
        let mut next = self.clone();
        next.revision = self.revision.next()?;
        let event = match edit {
            WorkEdit::SetObjective { objective } => {
                next.objective = objective;
                next.objective_author = author;
                next.objective_revision = next.revision;
                for question in &mut next.questions {
                    if question.is_current() {
                        question.state = WorkQuestionState::Superseded;
                    }
                }
                next.plan = None;
                WorkEventKind::ObjectiveEdited
            }
            WorkEdit::OpenQuestion {
                id,
                prompt,
                options,
            } => {
                if next.questions.len() >= MAX_WORK_QUESTIONS {
                    return Err(WorkError::Capacity);
                }
                if next.questions.iter().any(|q| q.id == id) {
                    return Err(WorkError::Conflict);
                }
                next.questions.push(WorkQuestion {
                    basis_revision: Some(self.revision),
                    objective_revision: Some(self.objective_revision),
                    state: WorkQuestionState::Active,
                    author,
                    answer_author: None,
                    id,
                    prompt,
                    options,
                    answer: None,
                });
                next.plan = None;
                WorkEventKind::QuestionOpened
            }
            WorkEdit::AnswerQuestion { id, answer } => {
                let question = next
                    .questions
                    .iter_mut()
                    .find(|q| q.id == id)
                    .ok_or(WorkError::NotFound)?;
                if !question.is_current() {
                    return Err(WorkError::Conflict);
                }
                question.answer = Some(answer);
                question.answer_author = Some(author);
                question.state = WorkQuestionState::Answered;
                next.plan = None;
                WorkEventKind::QuestionAnswered
            }
            WorkEdit::ReplaceDraft { draft } => {
                if next
                    .questions
                    .iter()
                    .any(|q| q.state == WorkQuestionState::Active)
                {
                    return Err(WorkError::Conflict);
                }
                next.plan = Some(WorkPlanRevision {
                    author,
                    revision: next.revision,
                    basis_revision: self.revision,
                    draft,
                });
                WorkEventKind::DraftReplaced
            }
            WorkEdit::DismissQuestion { id } => {
                let question = next
                    .questions
                    .iter_mut()
                    .find(|q| q.id == id)
                    .ok_or(WorkError::NotFound)?;
                if !question.is_current() {
                    return Err(WorkError::Conflict);
                }
                question.state = WorkQuestionState::Dismissed;
                next.plan = None;
                WorkEventKind::QuestionDismissed
            }
            WorkEdit::Archive => {
                next.lifecycle = WorkLifecycle::Archived;
                WorkEventKind::Archived
            }
            WorkEdit::Restore => {
                if self.lifecycle != WorkLifecycle::Archived {
                    return Err(WorkError::Conflict);
                }
                next.lifecycle = WorkLifecycle::Active;
                WorkEventKind::Restored
            }
            WorkEdit::CompactHistory => {
                next.questions.retain(WorkQuestion::is_current);
                WorkEventKind::HistoryCompacted
            }
        };
        if matches!(
            event,
            WorkEventKind::ObjectiveEdited
                | WorkEventKind::QuestionOpened
                | WorkEventKind::QuestionAnswered
                | WorkEventKind::QuestionDismissed
        ) {
            next.context_revision = next.revision;
        }
        next.status = next.derived_status();
        next.validate()?;
        Ok((next, event))
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkEdit {
    Archive,
    Restore,
    CompactHistory,
    DismissQuestion {
        id: WorkQuestionId,
    },
    SetObjective {
        objective: String,
    },
    OpenQuestion {
        id: WorkQuestionId,
        prompt: String,
        options: Vec<String>,
    },
    AnswerQuestion {
        id: WorkQuestionId,
        answer: String,
    },
    ReplaceDraft {
        draft: WorkPlanDraft,
    },
}
impl WorkEdit {
    pub fn validate(&self) -> Result<(), WorkError> {
        match self {
            Self::SetObjective { objective } => validate_text(objective, MAX_WORK_TEXT_BYTES),
            Self::OpenQuestion {
                prompt, options, ..
            } => {
                validate_text(prompt, MAX_WORK_TEXT_BYTES)?;
                if options.len() > 8 {
                    return Err(WorkError::Invalid);
                }
                let mut unique = BTreeSet::new();
                for option in options {
                    validate_text(option, 512)?;
                    if !unique.insert(option) {
                        return Err(WorkError::Invalid);
                    }
                }
                Ok(())
            }
            Self::AnswerQuestion { answer, .. } => validate_text(answer, MAX_WORK_TEXT_BYTES),
            Self::ReplaceDraft { draft } => draft.validate(),
            Self::Archive | Self::Restore | Self::CompactHistory | Self::DismissQuestion { .. } => {
                Ok(())
            }
        }
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkEventKind {
    RuntimeChanged,
    Archived,
    Restored,
    HistoryCompacted,
    QuestionDismissed,
    Created,
    ObjectiveEdited,
    QuestionOpened,
    QuestionAnswered,
    DraftReplaced,
}

fn validate_node_content(
    objective: &str,
    outputs: &[WorkExpectedOutput],
) -> Result<usize, WorkError> {
    validate_text(objective, MAX_WORK_TEXT_BYTES)?;
    if outputs.is_empty() || outputs.len() > 8 {
        return Err(WorkError::Invalid);
    }
    let mut names = BTreeSet::new();
    let mut bytes = objective.len();
    for output in outputs {
        validate_text(&output.name, 128)?;
        validate_text(&output.description, 2048)?;
        if !names.insert(&output.name) {
            return Err(WorkError::Invalid);
        }
        bytes += output.name.len() + output.description.len();
    }
    if bytes > MAX_WORK_NODE_BYTES {
        return Err(WorkError::Capacity);
    }
    Ok(bytes)
}

pub(crate) fn validate_text(text: &str, max: usize) -> Result<(), WorkError> {
    if text.len() > max
        || text.trim().is_empty()
        || text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(WorkError::Invalid);
    }
    Ok(())
}

macro_rules! redacted_debug {
    ($($name:ident),+ $(,)?) => { $(impl std::fmt::Debug for $name {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(concat!(stringify!($name), "([content redacted])"))
        }
    })+ };
}
redacted_debug!(
    WorkExpectedOutput,
    WorkSnapshot,
    WorkPlanDraft,
    WorkPlanNode,
    WorkPlanRevision,
    WorkQuestion,
    WorkEdit
);
