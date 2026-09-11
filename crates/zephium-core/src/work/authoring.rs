//! Idempotent user authoring vocabulary. New entity identities and attribution
//! are chosen by Rust only after durable command replay has been checked.
use super::{proposal::WorkPlanProposal, *};

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkAuthoringIntent {
    Create {
        objective: String,
    },
    Edit {
        work: WorkId,
        expected_revision: WorkRevision,
        edit: WorkUserEdit,
    },
    Delete {
        work: WorkId,
        expected_revision: WorkRevision,
    },
}
impl WorkAuthoringIntent {
    pub fn validate(&self) -> Result<(), WorkError> {
        match self {
            Self::Create { objective } => validate_text(objective, MAX_WORK_TEXT_BYTES),
            Self::Edit { edit, .. } => edit.validate(),
            Self::Delete { .. } => Ok(()),
        }
    }
    pub fn work(&self) -> Option<WorkId> {
        match self {
            Self::Create { .. } => None,
            Self::Edit { work, .. } | Self::Delete { work, .. } => Some(*work),
        }
    }
}
impl std::fmt::Debug for WorkAuthoringIntent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkAuthoringIntent([content redacted])")
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkAuthoringReceipt {
    pub command: WorkCommandId,
    pub work: WorkId,
    pub applied_revision: WorkRevision,
    pub deleted: bool,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkUserEdit {
    SetObjective {
        objective: String,
    },
    OpenQuestion {
        prompt: String,
        options: Vec<String>,
    },
    AnswerQuestion {
        id: WorkQuestionId,
        answer: String,
    },
    DismissQuestion {
        id: WorkQuestionId,
    },
    ReplaceDraft {
        proposal: WorkPlanProposal,
    },
    Archive,
    Restore,
    CompactHistory,
}
impl WorkUserEdit {
    pub fn validate(&self) -> Result<(), WorkError> {
        match self {
            Self::ReplaceDraft { proposal } => proposal.validate(),
            Self::OpenQuestion { prompt, options } => WorkEdit::OpenQuestion {
                id: WorkQuestionId::from(0),
                prompt: prompt.clone(),
                options: options.clone(),
            }
            .validate(),
            _ => self.clone().into_edit()?.validate(),
        }
    }
    pub fn into_edit(self) -> Result<WorkEdit, WorkError> {
        let edit = self;
        Ok(match edit {
            WorkUserEdit::SetObjective { objective } => WorkEdit::SetObjective { objective },
            WorkUserEdit::OpenQuestion { prompt, options } => WorkEdit::OpenQuestion {
                id: WorkQuestionId::generate(),
                prompt,
                options,
            },
            WorkUserEdit::AnswerQuestion { id, answer } => WorkEdit::AnswerQuestion { id, answer },
            WorkUserEdit::DismissQuestion { id } => WorkEdit::DismissQuestion { id },
            WorkUserEdit::ReplaceDraft { proposal } => WorkEdit::ReplaceDraft {
                draft: proposal.mint()?,
            },
            WorkUserEdit::Archive => WorkEdit::Archive,
            WorkUserEdit::Restore => WorkEdit::Restore,
            WorkUserEdit::CompactHistory => WorkEdit::CompactHistory,
        })
    }
}
impl std::fmt::Debug for WorkUserEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkUserEdit([redacted])")
    }
}
