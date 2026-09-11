//! Nonblocking Store contract. Refusal means no callback or mutation; acceptance
//! means exactly one settlement unless the process dies. Lost settlements are
//! uncertain: reconcile the same Work/revision, never blindly replay an edit.
use super::*;

#[derive(Clone)]
pub enum WorkRequest {
    Delete {
        id: WorkId,
        expected: WorkRevision,
    },
    Create {
        author: WorkAuthor,
        id: WorkId,
        objective: String,
    },
    Read {
        id: WorkId,
    },
    Edit {
        author: WorkAuthor,
        id: WorkId,
        expected: WorkRevision,
        edit: WorkEdit,
    },
    ListPlans {
        id: WorkId,
    },
    ReadPlan {
        id: WorkId,
        revision: WorkRevision,
    },
    /// Stable keyset order by Work identity. Concurrent inserts can be seen by
    /// restarting enumeration; this is not a cross-page snapshot transaction.
    List {
        after: Option<WorkId>,
        limit: usize,
    },
}
impl WorkRequest {
    pub fn validate(&self) -> Result<(), WorkError> {
        match self {
            Self::Create { objective, .. } => validate_text(objective, MAX_WORK_TEXT_BYTES),
            Self::Edit { edit, .. } => edit.validate(),
            Self::List { limit, .. } if *limit == 0 || *limit > MAX_WORK_PAGE_SIZE => {
                Err(WorkError::Invalid)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkSummary {
    pub lifecycle: WorkLifecycle,
    pub schema_version: u16,
    pub id: WorkId,
    pub revision: WorkRevision,
    pub status: WorkAuthoringStatus,
    pub objective: String,
}
impl WorkSummary {
    pub fn validate(&self) -> Result<(), WorkError> {
        if self.schema_version != WORK_SCHEMA_VERSION {
            return Err(WorkError::Invalid);
        }
        validate_text(&self.objective, MAX_WORK_TEXT_BYTES)
    }
}
impl std::fmt::Debug for WorkSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkSummary([content redacted])")
    }
}
#[derive(Clone, Debug)]
pub enum WorkReply {
    Deleted {
        id: WorkId,
    },
    Snapshot(Box<WorkSnapshot>),
    Plan(WorkPlanRevision),
    PlanHistory {
        revisions: Vec<WorkRevision>,
    },
    Page {
        works: Vec<WorkSummary>,
        next: Option<WorkId>,
    },
}
pub type WorkCompletion = Box<dyn FnOnce(Result<WorkReply, WorkError>) + Send>;

impl std::fmt::Debug for WorkRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkRequest([content redacted])")
    }
}
