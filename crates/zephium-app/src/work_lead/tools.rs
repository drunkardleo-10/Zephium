//! The seam other tool sets and helpers plug into. A tool set offers typed
//! tools to the lead or to a helper and handles their calls through a
//! [`LeadToolContext`], which records steps and sources durably, waits on the
//! person and stands the deadline still while it does. A helper is a
//! sub-agent for one kind of part, with its own prompt, tools and model role.
//!
//! Register a tool set or helper with one line in `registry.rs`.
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use zephium_core::ids::ProfileId;
use zephium_core::work::{
    artifact::WorkEvidenceLink,
    model::{WorkModelRole, WorkModelTool, WorkModelToolCall},
    parts::{WorkHelperV1, WorkInputFactV1},
    runtime::*,
    *,
};
use zephium_ipc::work::WorkActivityV1;

use super::run::LeadRun;

/// Where tools are offered: to the lead, or to one kind of helper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeadScope {
    Lead,
    Helper(WorkHelperV1),
}

/// What a call returns to the model. Text is compact: a digest, never a page.
pub struct LeadToolOutcome {
    pub content: String,
    pub is_error: bool,
}
impl LeadToolOutcome {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }
    /// A fault the model can correct: name the field and the rule.
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

pub type LeadToolFuture<'a> = Pin<Box<dyn Future<Output = LeadToolOutcome> + Send + 'a>>;

pub trait LeadToolSet: Send + Sync {
    /// The tools this set offers in `scope`. Names are unique across sets.
    fn tools(&self, scope: LeadScope, run: &LeadRunView) -> Vec<WorkModelTool>;
    /// Handles one call to a tool this set offered. The call's arguments
    /// are model output: validate every field.
    fn call<'a>(
        &'a self,
        context: LeadToolContext<'a>,
        call: WorkModelToolCall,
    ) -> LeadToolFuture<'a>;
}

/// A sub-agent for parts of one helper kind. The lead's helper loop runs it
/// with its prompt, its tools, `create` for the part's found objects and
/// `finish` for its digest.
pub trait LeadHelper: Send + Sync {
    fn kind(&self) -> WorkHelperV1;
    /// The model role it runs on: `Page` for acting, `Light` for reading.
    fn role(&self) -> WorkModelRole;
    /// Its own short instructions, in closed sentences.
    fn prompt(&self) -> &str;
    fn tools(&self) -> Arc<dyn LeadToolSet>;
    /// At most this many model turns per part.
    fn max_turns(&self) -> u8 {
        12
    }
    /// Objects the part leaves when it ends, placed on its row by the lead:
    /// what the helper made exactly (a diff of the files it changed), never
    /// something the model wrote.
    fn objects<'a>(&'a self, _context: LeadToolContext<'a>) -> LeadObjectsFuture<'a> {
        Box::pin(async { Vec::new() })
    }
}

/// Titled objects a helper leaves on its part's row.
pub type LeadObjectsFuture<'a> = Pin<
    Box<
        dyn Future<Output = Vec<(String, zephium_core::work::artifact::WorkArtifactDataV1)>>
            + Send
            + 'a,
    >,
>;

/// What a tool set may know about the run when it lists its tools.
pub struct LeadRunView<'a> {
    pub(crate) run: &'a LeadRun,
    pub(crate) service: Option<&'a zephium_core::work::parts::WorkPartServiceV1>,
}
impl LeadRunView<'_> {
    pub fn profile(&self) -> ProfileId {
        self.run.profile
    }
    /// The site or connection the part works with, when the lead named one;
    /// `None` for the lead's own tools.
    pub fn service(&self) -> Option<&zephium_core::work::parts::WorkPartServiceV1> {
        self.service
    }
    /// Folders the person granted for this run.
    pub fn folders(&self) -> &[String] {
        &self.run.grant.folders
    }
    pub fn private(&self) -> bool {
        self.run.grant.private
    }
}

/// One call's handle on the run.
#[derive(Clone, Copy)]
pub struct LeadToolContext<'a> {
    pub(crate) run: &'a LeadRun,
    pub(crate) part: Option<WorkPartId>,
}
impl<'a> LeadToolContext<'a> {
    pub fn profile(&self) -> ProfileId {
        self.run.profile
    }
    pub fn work(&self) -> WorkId {
        self.run.probe.work()
    }
    pub fn execution(&self) -> WorkExecutionId {
        self.run.probe.execution()
    }
    pub fn attempt(&self) -> WorkAttemptId {
        self.run.probe.attempt()
    }
    pub fn node(&self) -> WorkPlanNodeId {
        self.run.probe.node()
    }
    /// The part this call works for; steps it records carry it.
    pub fn part(&self) -> Option<WorkPartId> {
        self.part
    }
    pub fn handle(&self) -> &crate::Handle {
        &self.run.handle
    }
    pub fn probe(&self) -> &'a crate::work_runtime::WorkAttemptProbe {
        &self.run.probe
    }
    /// The granted folders, admitted by policy; `None` when none is granted.
    pub fn files(&self) -> Option<&'a crate::work_files::WorkFileGrant> {
        self.run.files.as_ref()
    }
    pub fn activity(&self, activity: WorkActivityV1) {
        self.run.activity(activity);
    }
    /// The run was stopped, ran out of time or lost its owner.
    pub async fn cancelled(&self) -> bool {
        self.run.cancelled().await
    }
    /// Model usage a tool spent outside the lead's own calls.
    pub fn charge(&self, usage: WorkUsage) {
        self.run.charge(usage);
    }

    /// Records a step: `Running` to settle later, or already settled (a
    /// settled model or fetch step carries `usage`).
    pub async fn begin_step(
        &self,
        kind: WorkStepKindV1,
        status: WorkStepStatus,
        note: Option<String>,
        local: Option<WorkLocalStepV1>,
    ) -> Result<WorkStepId, WorkError> {
        let mut step = self.run.step(kind, status, self.part);
        step.note = note;
        step.local = local.map(Box::new);
        self.run.begin(step, vec![]).await
    }
    /// Settles a running step, with the file record it disclosed, if any.
    pub async fn settle_step(
        &self,
        step: WorkStepId,
        status: WorkStepStatus,
        note: Option<String>,
        file: Option<WorkFileRecordV1>,
    ) -> Result<(), WorkError> {
        self.run.settle(step, status, None, note, file).await
    }
    /// Settles a running command step with its output record.
    pub async fn settle_command(
        &self,
        step: WorkStepId,
        status: WorkStepStatus,
        record: WorkCommandRecordV1,
        note: String,
    ) -> Result<(), WorkError> {
        self.run
            .probe
            .commit_step(WorkRuntimeUpdate::SettleCommand {
                execution: self.execution(),
                attempt: self.attempt(),
                step,
                status,
                record: Box::new(record),
                note,
            })
            .await
            .map(|_| ())
    }
    /// Streams a running command's output to the canvas.
    pub async fn command_progress(
        &self,
        step: WorkStepId,
        output: WorkCommandOutputV1,
    ) -> Result<(), WorkError> {
        self.run
            .probe
            .commit_step(WorkRuntimeUpdate::CommandProgress {
                execution: self.execution(),
                attempt: self.attempt(),
                step,
                output,
            })
            .await
            .map(|_| ())
    }
    /// Waits for the person's decision on a proposed file change or command
    /// step; the deadline stands still meanwhile. `None`: the run stopped.
    pub async fn decision(&self, step: WorkStepId) -> Result<Option<bool>, WorkError> {
        self.run.decision(step).await
    }
    /// Puts a question to the person and waits. `None`: the run stopped or
    /// nobody answered in time.
    pub async fn ask(
        &self,
        prompt: String,
        options: Vec<String>,
    ) -> Result<Option<String>, WorkError> {
        self.ask_for(WorkAskPurposeV1::Question, prompt, options)
            .await
    }
    /// Asks with the purpose the frame shows it by: `Context` for history,
    /// notes or tabs, `Connection` for a tool or service, `Confirm` for any
    /// other yes or no.
    pub async fn ask_for(
        &self,
        purpose: WorkAskPurposeV1,
        prompt: String,
        options: Vec<String>,
    ) -> Result<Option<String>, WorkError> {
        self.run.ask(purpose, prompt, options, self.part).await
    }
    /// Places an object the tool made exactly (not the model) on this
    /// call's part, validated like any other object. Its sources are none:
    /// it stands for what the tool did.
    pub async fn publish(
        &self,
        title: &str,
        data: zephium_core::work::artifact::WorkArtifactDataV1,
    ) -> Result<WorkArtifactId, WorkError> {
        let title = super::call::clip(title.trim(), super::objects::MAX_TITLE_CHARS);
        if title.is_empty() || data.lead_fault(0).is_some() {
            return Err(WorkError::Invalid);
        }
        super::objects::publish(
            self.run,
            super::objects::Proposed {
                title,
                data,
                evidence: vec![],
            },
            self.part,
            None,
        )
        .await
    }
    /// Holds a step that would commit something for the person's decision,
    /// with the deadline standing still. `Some(true)`: approved and recorded
    /// as done; `Some(false)`: declined; `None`: the run stopped or nobody
    /// answered in time.
    pub async fn confirm(&self, confirm: WorkConfirmV1) -> Result<Option<bool>, WorkError> {
        self.run.confirm(confirm, self.part).await
    }
    /// Makes a durable record citable by objects: returns the key the model
    /// names in `sources` (a file record's link is `{record id, 1}`).
    pub fn cite(&self, link: WorkEvidenceLink, title: &str, url: Option<&str>) -> String {
        self.run.cite(link, title, url)
    }
    /// Shows something the run pulled in (memory, notes, history, tabs) as
    /// an input left of the request.
    pub async fn input(&self, input: WorkInputFactV1) {
        self.run.input(input).await;
    }
}
