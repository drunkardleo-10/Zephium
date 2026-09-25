//! Nonblocking Store contract. Refusal means no callback or mutation; acceptance
//! means exactly one settlement unless the process dies. Lost settlements are
//! uncertain: reconcile the same Work/revision, never blindly replay an edit.
use super::*;

#[derive(Clone)]
pub enum WorkRequest {
    /// Host-only context read, bound to an existing resource revision and profile.
    ReadMediaContext {
        resource: String,
        revision: String,
    },
    Environment {
        call: environment::WorkEnvironmentCall,
        /// Application-actor observations, never decoded from IPC. Store checks
        /// them after receipt lookup so replay survives resource retirement.
        space_available: bool,
        browser_available: bool,
        /// A resource reference names a live note of this profile. Notes are
        /// files outside the store, so only the application actor can say.
        note_available: bool,
    },
    AuthoringCommand {
        command: WorkCommandId,
        intent: authoring::WorkAuthoringIntent,
    },
    /// On-demand historical evidence linked by this Work; never live authority.
    ReadEvidence {
        id: WorkId,
        link: artifact::WorkEvidenceLink,
    },
    /// Owner drop notification. Exact attempt matching replaces user CAS; this
    /// can only record uncertainty, never start work or publish a result.
    RuntimeAbandon {
        id: WorkId,
        execution: WorkExecutionId,
        attempt: WorkAttemptId,
    },
    RuntimeRead {
        id: WorkId,
    },
    RuntimeCommand {
        id: WorkId,
        expected: WorkRevision,
        command: WorkCommandId,
        intent: runtime::WorkRuntimeIntent,
    },
    /// A user-directed public read carrying the Rust-admitted context
    /// manifest it may disclose. Never constructed from IPC.
    RuntimeCommandDisclosed {
        id: WorkId,
        expected: WorkRevision,
        command: WorkCommandId,
        intent: runtime::WorkRuntimeIntent,
        context: context::WorkContextDisclosureV1,
    },
    /// Host-only, never decoded from IPC or model output.
    RuntimeUpdate {
        id: WorkId,
        expected: WorkRevision,
        update: runtime::WorkRuntimeUpdate,
    },
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
        if let Self::RuntimeUpdate {
            update: runtime::WorkRuntimeUpdate::SettleProviderSearch { evidence, .. },
            ..
        } = self
        {
            evidence.evidence.validate()?;
        }
        match self {
            Self::RuntimeUpdate {
                update: runtime::WorkRuntimeUpdate::PageTitle { title, .. },
                ..
            } => runtime::validate_page_title(title),
            Self::ReadMediaContext { resource, revision } => {
                validate_text(resource, 128)?;
                validate_text(revision, 128)
            }
            Self::Environment { call, .. } => call.validate(),
            Self::AuthoringCommand { intent, .. } => intent.validate(),
            Self::RuntimeCommand {
                intent: runtime::WorkRuntimeIntent::EditArtifact { data, evidence, .. },
                ..
            } => {
                if evidence.len() > 64 {
                    return Err(WorkError::Capacity);
                }
                data.validate(evidence.len())
            }
            Self::RuntimeCommand {
                intent: runtime::WorkRuntimeIntent::ReadPublic { scope, limits },
                ..
            } => search::validate_direct_public_read(scope, *limits),
            Self::RuntimeCommandDisclosed {
                intent, context, ..
            } => match intent {
                runtime::WorkRuntimeIntent::ReadPublic { scope, limits } => {
                    if context.purpose != context::WorkContextPurpose::PublicRead
                        || context.requires_review()
                    {
                        return Err(WorkError::Invalid);
                    }
                    context.validate()?;
                    search::validate_direct_public_read(scope, *limits)
                }
                runtime::WorkRuntimeIntent::BeginAgent { grant, limits } => {
                    if context.purpose != context::WorkContextPurpose::Agent {
                        return Err(WorkError::Invalid);
                    }
                    context.validate()?;
                    grant.validate()?;
                    limits.validate()
                }
                _ => Err(WorkError::Invalid),
            },
            Self::RuntimeCommand {
                intent: runtime::WorkRuntimeIntent::BeginAgent { grant, limits },
                ..
            } => {
                grant.validate()?;
                limits.validate()
            }
            Self::RuntimeCommand {
                intent: runtime::WorkRuntimeIntent::AnswerStep { answer: text, .. },
                ..
            }
            | Self::RuntimeCommand {
                intent: runtime::WorkRuntimeIntent::Steer { text, .. },
                ..
            } => validate_text(text, MAX_WORK_TEXT_BYTES),
            Self::RuntimeUpdate {
                update:
                    runtime::WorkRuntimeUpdate::BeginStep {
                        step,
                        artifacts,
                        evidence,
                        file,
                        ..
                    },
                ..
            } => {
                step.validate()?;
                validate_step_payload(artifacts, evidence.as_deref(), file.as_deref())
            }
            Self::RuntimeUpdate {
                update:
                    runtime::WorkRuntimeUpdate::SettleStep {
                        artifacts,
                        evidence,
                        file,
                        note,
                        ..
                    },
                ..
            } => {
                if let Some(note) = note {
                    validate_text(note, runtime::MAX_WORK_STEP_NOTE_BYTES)?;
                }
                validate_step_payload(artifacts, evidence.as_deref(), file.as_deref())
            }
            Self::RuntimeUpdate {
                update: runtime::WorkRuntimeUpdate::CommandProgress { output, .. },
                ..
            } => runtime::validate_local_text(&output.text),
            Self::RuntimeUpdate {
                update: runtime::WorkRuntimeUpdate::SettleCommand { record, note, .. },
                ..
            } => {
                record.command.validate()?;
                validate_text(note, runtime::MAX_WORK_STEP_NOTE_BYTES)
            }
            Self::RuntimeCommand {
                intent: runtime::WorkRuntimeIntent::Approve { spec },
                ..
            } => spec.validate_bounds(),
            Self::RuntimeUpdate {
                update:
                    runtime::WorkRuntimeUpdate::Settle { artifacts, .. }
                    | runtime::WorkRuntimeUpdate::SettleProviderSearch { artifacts, .. },
                ..
            } => {
                if artifacts.len() > artifact::MAX_WORK_ARTIFACTS {
                    return Err(WorkError::Capacity);
                }
                for artifact in artifacts {
                    artifact.validate()?;
                }
                Ok(())
            }
            Self::Create { objective, .. } => validate_text(objective, MAX_WORK_TEXT_BYTES),
            Self::Edit { edit, .. } => edit.validate(),
            Self::List { limit, .. } if *limit == 0 || *limit > MAX_WORK_PAGE_SIZE => {
                Err(WorkError::Invalid)
            }
            _ => Ok(()),
        }
    }
}

fn validate_step_payload(
    artifacts: &[artifact::WorkArtifactV1],
    evidence: Option<&runtime::WorkProviderSearchRecordV1>,
    file: Option<&runtime::WorkFileRecordV1>,
) -> Result<(), WorkError> {
    if artifacts.len() > artifact::MAX_WORK_ARTIFACTS {
        return Err(WorkError::Capacity);
    }
    for artifact in artifacts {
        artifact.validate()?;
    }
    if evidence.is_some() && file.is_some() {
        return Err(WorkError::Invalid);
    }
    if let Some(record) = evidence {
        record.evidence.validate()?;
    }
    if let Some(record) = file {
        record.file.validate()?;
    }
    Ok(())
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
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
    MediaContext(WorkMediaContext),
    Environment(environment::WorkEnvironmentReply),
    AuthoringCommand(authoring::WorkAuthoringReceipt),
    Evidence(artifact::WorkEvidencePreviewV1),
    /// Returned only to the original successful Begin observer, never by read
    /// or command replay. This is data; the application owns the live attempt.
    RuntimeStarted {
        projection: Box<runtime::WorkRuntimeProjection>,
        remaining_millis: u32,
    },
    Runtime(Box<runtime::WorkRuntimeProjection>),
    PublicReadAdmitted {
        projection: Box<runtime::WorkRuntimeProjection>,
        receipt: runtime::WorkCommandReceipt,
        replayed: bool,
    },
    /// The agent execution exists and is approved; only a fresh (not replayed)
    /// admission may begin its attempt.
    AgentAdmitted {
        projection: Box<runtime::WorkRuntimeProjection>,
        receipt: runtime::WorkCommandReceipt,
        replayed: bool,
    },
    RuntimeCommand {
        projection: Box<runtime::WorkRuntimeProjection>,
        receipt: runtime::WorkCommandReceipt,
    },
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

/// Native context bytes, never an IPC projection or diagnostic body.
#[derive(Clone)]
pub struct WorkMediaContext(pub Vec<u8>);
impl std::fmt::Debug for WorkMediaContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkMediaContext")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl std::fmt::Debug for WorkRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkRequest([content redacted])")
    }
}
