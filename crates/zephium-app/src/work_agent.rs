//! The routine agent loop. Rust admits every turn's operations, commits each
//! step as it settles, and enforces budgets; the model only proposes.
use crate::work_runtime::*;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, Instant},
};
use zephium_core::{
    ids::ProfileId,
    work::{agent::*, artifact::*, port::*, runtime::*, search::*, synthesis::*, *},
};
use zephium_ipc::work::WorkActivityV1;

#[path = "work_agent_reads.rs"]
mod reads;

/// A browser read or discovery the loop admitted for one step. The host
/// compiles it into an anonymous, read-only public browsing task.
#[derive(Clone)]
pub struct WorkAgentBrowseRequest {
    pub construction_attempt: zephium_agentic::WorkBrowserConstructionAttempt,
    pub id: WorkStepId,
    pub step: WorkStepKindV1,
    pub hops: u8,
    pub objective: String,
    pub output: String,
    pub limits: WorkExecutionLimits,
}
pub struct WorkBrowserOutcome {
    pub status: WorkStepStatus,
    pub usage: Option<WorkUsage>,
    pub artifacts: Vec<WorkArtifactDraft>,
    pub intervention: Option<WorkInterventionV1>,
    /// Why the page gave nothing, in closed words the model and the person read.
    pub note: Option<String>,
    /// Closed wall time, provider call counts and exact accounting for this read.
    pub measurements: Option<WorkStepMeasurementsV1>,
}
pub struct WorkAgentProviders<'a> {
    pub turn: &'a dyn WorkAgentTurnProvider,
    pub search: &'a dyn WorkPublicSearchProvider,
}

/// Tokens a turn must still be able to spend before the loop stops itself.
const TURN_TOKEN_FLOOR: u32 = 12_000;
/// Consecutive refused, idle or dropped turns before the run gives up.
/// Cancellation polls the store may fail to answer in a row before a run stops.
const MAX_UNREADABLE_POLLS: u8 = 12;
const MAX_FAILED_TURNS: u8 = 3;
const STEPS_EXHAUSTED: &str = "The step budget is used up: no more searches or reads will run. Publish the result from the sources already collected and finish.";
const ASK_POLL: Duration = Duration::from_millis(500);
const MAX_PREVIEWS: usize = 96;
const INHERITED_PREVIEWS: usize = 64;
const INHERITED_ARTIFACTS: usize = 32;
/// Earlier executions whose objects a run inherits whole; older ones reach
/// the model through the thread, and by object only when the request names it.
const INHERITED_EXECUTIONS: usize = 3;
/// Time a run keeps to act on an answer. A question still open this close to
/// the deadline suspends the run on it instead of letting the deadline fail it.
const ASK_RESERVE: Duration = Duration::from_secs(180);
const ASK_SUSPENDED: &str = "Waiting for your answer";
const BUDGET_SPENT: &str = "The run used its turns, steps or budget before it could finish";
const CONTEXT_FULL: &str = "The work has grown too large for one turn";
const TURN_UNPREPARED: &str = "The turn could not be prepared";

/// Closed loop facts for development logs; never model, page or user text.
#[derive(Clone, Copy, Debug)]
pub enum WorkAgentDiagnostic {
    TurnAdmitted {
        turn: u8,
        artifacts: usize,
        dropped: usize,
        fetches: usize,
        asks: bool,
        finish: bool,
    },
    TurnRefused {
        turn: u8,
        /// None when the provider returned nothing to resolve.
        reason: Option<zephium_core::work::agent::WorkAgentTurnRefusal>,
    },
    /// A search answered but nothing in it could be admitted.
    SearchRefused { note: &'static str },
    /// The loop stopped because the durable state says so.
    Stopped {
        cause: crate::work_runtime::WorkCancelCause,
    },
    ArtifactRefused {
        turn: u8,
        reason: zephium_core::work::agent::WorkAgentArtifactRefusal,
    },
    /// A page whose check may have passed in the background is loaded once more.
    ReadRetried,
    /// One settled page read, measured: wall time, provider calls and exact cost.
    ReadMeasured(WorkStepMeasurementsV1),
    /// The loop ended on an error the run reports as interrupted.
    LoopFailed { error: WorkError },
    /// No turn could be disclosed, even after shedding older context.
    DisclosureRefused { error: WorkError },
    /// Turns, steps, tokens or cost left no room for another turn.
    BudgetSpent,
    /// A question was still open near the deadline; the run stops on it.
    AskSuspended,
    CommitRefused {
        kind: &'static str,
        error: WorkError,
    },
}

pub struct WorkAgentService {
    handle: crate::Handle,
    diagnostic: Option<fn(WorkAgentDiagnostic)>,
}
impl WorkAgentService {
    pub fn new(handle: crate::Handle) -> Self {
        Self {
            handle,
            diagnostic: None,
        }
    }
    pub fn with_diagnostic(mut self, diagnostic: fn(WorkAgentDiagnostic)) -> Self {
        self.diagnostic = Some(diagnostic);
        self
    }

    /// Admits the objective, begins the single attempt and runs turns until
    /// the agent finishes, a budget ends, the user stops it, or a provider
    /// outcome becomes unknown. Every step is durable before the next begins.
    pub async fn run<B, Fut, O>(
        &self,
        profile: ProfileId,
        command: zephium_ipc::work::WorkCommandV1,
        selection: Option<context::WorkContextSelectionV1>,
        providers: WorkAgentProviders<'_>,
        mut browser: B,
        mut observe: O,
    ) -> Result<WorkRuntimeProjection, WorkError>
    where
        B: FnMut(WorkAttemptProbe, WorkAgentBrowseRequest) -> Fut,
        Fut: Future<Output = Result<WorkBrowserOutcome, WorkError>>,
        O: FnMut(WorkAttemptObserver),
    {
        if command.version != 1 || !matches!(command.intent, WorkRuntimeIntent::BeginAgent { .. }) {
            return Err(WorkError::Invalid);
        }
        let work = command.work;
        let command_id = command.command;
        let (request, bodies, private) = match selection {
            Some(selection) => {
                let admitted = crate::work_context::WorkContextAdmission::new(self.handle.clone())
                    .admit(profile, context::WorkContextPurpose::Agent, &selection)
                    .await?;
                let private: Vec<String> = admitted
                    .disclosure
                    .items
                    .iter()
                    .zip(&admitted.bodies)
                    .filter(|(item, _)| item.visibility == context::WorkContextVisibility::Private)
                    .map(|(_, body)| body.text.clone())
                    .collect();
                let zephium_ipc::work::WorkCommandV1 {
                    work,
                    expected_revision,
                    command,
                    intent,
                    ..
                } = command;
                (
                    WorkRequest::RuntimeCommandDisclosed {
                        id: work,
                        expected: expected_revision,
                        command,
                        intent,
                        context: admitted.disclosure,
                    },
                    admitted.bodies,
                    private,
                )
            }
            None => (command.into_request()?, Vec::new(), Vec::new()),
        };
        request.validate()?;
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            self.handle.submit_work_document(request, Some(profile))?,
        )
        .await
        .map_err(|_| WorkError::OutcomeUnknown)??;
        if response.profile != profile {
            return Err(WorkError::ProfileUnavailable);
        }
        let WorkReply::AgentAdmitted {
            projection,
            receipt,
            replayed,
        } = response.reply
        else {
            return Err(WorkError::Invalid);
        };
        if receipt.command != command_id || projection.work.id != work {
            return Err(WorkError::Invalid);
        }
        if replayed {
            return Ok(*projection);
        }
        let execution = projection
            .executions
            .iter()
            .find(|entry| entry.id == receipt.execution)
            .ok_or(WorkError::Invalid)?;
        let grant = execution.agent_grant().cloned().ok_or(WorkError::Invalid)?;
        if execution.authorization != WorkExecutionAuthorization::UserDirectedAgent
            || projection.work.revision != receipt.applied_revision
        {
            return Err(WorkError::Invalid);
        }
        let node = execution.spec.nodes[0].node;
        let attempt = WorkRuntimeService::new(self.handle.clone())
            .begin_node(
                profile,
                work,
                receipt.applied_revision,
                receipt.execution,
                node,
            )
            .await?;
        observe(attempt.observer());
        let original = attempt.attempt();
        let mut driver = Driver {
            probe: attempt.probe(),
            grant,
            limits: attempt.specification().limits,
            output: attempt.node().outputs[0].name.clone(),
            objective: attempt.disclosure_objective()?,
            decisions: attempt.decisions().to_vec(),
            bodies,
            private,
            inherited: Vec::new(),
            kept: Vec::new(),
            thread: Vec::new(),
            unreadable_polls: std::sync::atomic::AtomicU8::new(0),
            files: None,
            previews: Vec::new(),
            used: WorkUsage::default(),
            steps: 0,
            turn: 0,
            failed_turns: 0,
            intervention: None,
            diagnostic: self.diagnostic,
            notices: Vec::new(),
            published: 0,
            finish_refusals: 0,
            pending_output_repair: false,
        };
        let (files, refused) = crate::work_files::WorkFileGrant::admit(&driver.grant.folders);
        for folder in refused {
            driver.notice(&format!(
                "Folder not granted: {folder}. It is outside the home folder, protected, or missing."
            ));
        }
        driver.files = (!files.is_empty()).then_some(files);
        driver.inherit(&projection, receipt.execution).await;
        let outcome = driver.drive(&attempt, &providers, &mut browser).await;
        let (status, usage) = match outcome {
            Ok(status) => (status, driver.settled_usage(status)),
            Err(WorkError::OutcomeUnknown) => (WorkAttemptStatus::OutcomeUnknown, None),
            Err(error) => {
                driver.report(WorkAgentDiagnostic::LoopFailed { error });
                return Err(error);
            }
        };
        let settlement = attempt
            .settle_owned(WorkAdapterResult {
                status,
                usage,
                artifacts: vec![],
                intervention: driver
                    .intervention
                    .take()
                    .filter(|_| status != WorkAttemptStatus::Succeeded),
            })
            .await?;
        if settlement.profile() != profile
            || settlement.work() != work
            || settlement.execution() != receipt.execution
            || settlement.attempt() != original
        {
            return Err(WorkError::Invalid);
        }
        Ok(settlement.into_projection())
    }
}

struct Driver {
    probe: WorkAttemptProbe,
    grant: WorkAgentGrantV1,
    limits: WorkExecutionLimits,
    output: String,
    objective: String,
    decisions: Vec<planning::PlanningAnswer>,
    bodies: Vec<context::WorkContextBody>,
    /// Earlier executions of this work: their cards stay on the canvas and
    /// their sources stay citable.
    inherited: Vec<WorkArtifactV1>,
    /// Inherited objects the request names or the last run produced: never shed.
    kept: Vec<WorkArtifactId>,
    thread: Vec<WorkAgentThreadEntry>,
    /// Consecutive cancellation polls the store could not answer.
    unreadable_polls: std::sync::atomic::AtomicU8,
    /// Folders the person granted, once admitted by policy.
    files: Option<crate::work_files::WorkFileGrant>,
    private: Vec<String>,
    previews: Vec<WorkEvidencePreviewV1>,
    used: WorkUsage,
    steps: u32,
    turn: u8,
    failed_turns: u8,
    intervention: Option<WorkInterventionV1>,
    diagnostic: Option<fn(WorkAgentDiagnostic)>,
    /// Closed feedback for the next turn: what was refused and why.
    notices: Vec<String>,
    published: usize,
    finish_refusals: u8,
    pending_output_repair: bool,
}

enum Fetched {
    Search(WorkStepId, WorkSearchOutcomeOwned),
    Browse(WorkStepId, Result<WorkBrowserOutcome, WorkError>, Option<WorkUsage>),
}
struct WorkSearchOutcomeOwned {
    status: WorkAttemptStatus,
    usage: Option<WorkUsage>,
    note: Option<&'static str>,
    record: Option<WorkProviderSearchRecordV1>,
}

impl Driver {
    fn remaining(&self) -> WorkExecutionLimits {
        WorkExecutionLimits {
            model_tokens: self
                .limits
                .model_tokens
                .saturating_sub(self.used.model_tokens)
                .max(1),
            cost_micro_usd: self
                .limits
                .cost_micro_usd
                .saturating_sub(self.used.cost_micro_usd)
                .max(1),
            operations: self
                .limits
                .operations
                .saturating_sub(self.used.operations)
                .max(1),
            ..self.limits
        }
    }
    fn charge(&mut self, usage: WorkUsage) {
        self.used.model_tokens = self.used.model_tokens.saturating_add(usage.model_tokens);
        self.used.cost_micro_usd = self
            .used
            .cost_micro_usd
            .saturating_add(usage.cost_micro_usd);
        self.used.operations = self.used.operations.saturating_add(usage.operations.max(1));
        if usage.accounting == WorkUsageAccounting::ConservativeReservation {
            self.used.accounting = WorkUsageAccounting::ConservativeReservation;
        }
    }
    fn settled_usage(&self, status: WorkAttemptStatus) -> Option<WorkUsage> {
        if status == WorkAttemptStatus::OutcomeUnknown {
            return None;
        }
        let mut usage = self.used;
        usage.operations = usage.operations.max(self.steps);
        if !usage.within(self.limits) {
            usage = WorkUsage {
                model_tokens: self.limits.model_tokens,
                cost_micro_usd: self.limits.cost_micro_usd,
                operations: self.limits.operations,
                accounting: WorkUsageAccounting::ConservativeReservation,
            };
        }
        Some(usage)
    }
    fn budget_left(&self) -> bool {
        self.turn < self.grant.max_turns
            && self.steps + 2 <= u32::from(self.grant.max_steps)
            && self.remaining().model_tokens >= TURN_TOKEN_FLOOR
            && self.remaining().cost_micro_usd > 1
    }
    fn step(&self, kind: WorkStepKindV1, status: WorkStepStatus) -> WorkStepFact {
        WorkStepFact {
            id: WorkStepId::generate(),
            turn: self.turn.max(1),
            kind,
            status,
            usage: None,
            artifacts: vec![],
            evidence: None,
            note: None,
            measurements: None,
        }
    }
    async fn begin(
        &mut self,
        step: WorkStepFact,
        artifacts: Vec<WorkArtifactV1>,
        evidence: Option<WorkProviderSearchRecordV1>,
    ) -> Result<WorkStepId, WorkError> {
        let id = step.id;
        let kind = step_kind_label(&step.kind);
        if let Err(error) = self
            .probe
            .commit_step(WorkRuntimeUpdate::BeginStep {
                execution: self.probe.execution(),
                attempt: self.probe.attempt(),
                step,
                artifacts,
                evidence: evidence.map(Box::new),
                file: None,
            })
            .await
        {
            self.report(WorkAgentDiagnostic::CommitRefused { kind, error });
            return Err(error);
        }
        self.steps += 1;
        Ok(id)
    }
    /// Notices reach the model next turn: at most eight, each within the
    /// disclosure's limit, never repeated.
    fn notice(&mut self, text: &str) {
        let text = zephium_core::work::agent::clip_text(text, 512);
        if self.notices.contains(&text) {
            return;
        }
        if self.notices.len() == 8 {
            self.notices.remove(0);
        }
        self.notices.push(text);
    }
    /// Settles a file step with what it disclosed, or with why it failed.
    async fn settle_file(
        &mut self,
        step: WorkStepId,
        status: WorkStepStatus,
        file: Option<WorkFileRecordV1>,
        note: Option<String>,
    ) -> Result<(), WorkError> {
        if let Err(error) = self
            .probe
            .commit_step(WorkRuntimeUpdate::SettleStep {
                execution: self.probe.execution(),
                attempt: self.probe.attempt(),
                step,
                status,
                usage: None,
                artifacts: vec![],
                evidence: None,
                file: file.map(Box::new),
                note,
                measurements: None,
            })
            .await
        {
            self.report(WorkAgentDiagnostic::CommitRefused {
                kind: "settle_file",
                error,
            });
            return Err(error);
        }
        Ok(())
    }
    fn report(&self, event: WorkAgentDiagnostic) {
        if let Some(diagnostic) = self.diagnostic {
            diagnostic(event);
        }
    }
    /// Ends the run on a failed turn step that says why, when the grant still
    /// has room for the step; the attempt fails either way.
    async fn fail_turn(&mut self, note: &str) -> Result<WorkAttemptStatus, WorkError> {
        if self.steps < u32::from(self.grant.max_steps) {
            let mut step = self.step(WorkStepKindV1::Turn, WorkStepStatus::Failed);
            step.usage = Some(WorkUsage::default());
            step.note = Some(note.to_owned());
            let _ = self.begin(step, vec![], None).await;
        }
        Ok(WorkAttemptStatus::Failed)
    }
    #[allow(clippy::too_many_arguments)]
    async fn settle(
        &mut self,
        step: WorkStepId,
        status: WorkStepStatus,
        usage: Option<WorkUsage>,
        artifacts: Vec<WorkArtifactV1>,
        evidence: Option<WorkProviderSearchRecordV1>,
        note: Option<String>,
        measurements: Option<WorkStepMeasurementsV1>,
    ) -> Result<(), WorkError> {
        if let Err(error) = self
            .probe
            .commit_step(WorkRuntimeUpdate::SettleStep {
                execution: self.probe.execution(),
                attempt: self.probe.attempt(),
                step,
                status,
                usage,
                artifacts,
                evidence: evidence.map(Box::new),
                file: None,
                note,
                measurements,
            })
            .await
        {
            self.report(WorkAgentDiagnostic::CommitRefused {
                kind: "settle",
                error,
            });
            return Err(error);
        }
        Ok(())
    }
    async fn cancelled(&self) -> bool {
        use std::sync::atomic::Ordering;
        // One poll the store could not answer is not a stop; a run only ends
        // as unreadable when the answer stays out of reach.
        let cause = match self.probe.cancellation_cause().await {
            Ok(Some(cause)) => Some(cause),
            Ok(None) if Instant::now() >= self.probe.deadline() => {
                Some(crate::work_runtime::WorkCancelCause::Deadline)
            }
            Ok(None) => {
                self.unreadable_polls.store(0, Ordering::Relaxed);
                None
            }
            Err(_) => {
                let polls = self.unreadable_polls.fetch_add(1, Ordering::Relaxed) + 1;
                (polls >= MAX_UNREADABLE_POLLS)
                    .then_some(crate::work_runtime::WorkCancelCause::Unreadable)
            }
        };
        if let Some(cause) = cause {
            self.report(WorkAgentDiagnostic::Stopped { cause });
        }
        cause.is_some()
    }

    async fn drive<B, Fut>(
        &mut self,
        attempt: &WorkNodeAttempt,
        providers: &WorkAgentProviders<'_>,
        browser: &mut B,
    ) -> Result<WorkAttemptStatus, WorkError>
    where
        B: FnMut(WorkAttemptProbe, WorkAgentBrowseRequest) -> Fut,
        Fut: Future<Output = Result<WorkBrowserOutcome, WorkError>>,
    {
        loop {
            if self.cancelled().await {
                return Ok(WorkAttemptStatus::Cancelled);
            }
            if !self.budget_left() {
                self.report(WorkAgentDiagnostic::BudgetSpent);
                return self.fail_turn(BUDGET_SPENT).await;
            }
            self.turn += 1;
            self.probe.record_activity(WorkActivityV1::Planning);
            let state = self.probe.runtime_projection().await?;
            let execution = state
                .executions
                .iter()
                .find(|e| e.id == self.probe.execution())
                .ok_or(WorkError::NotFound)?;
            // Steps the person appended (steers) count against the grant too.
            self.steps = self
                .steps
                .max(u32::try_from(execution.steps.len()).unwrap_or(u32::MAX));
            let budget = WorkAgentBudget {
                turns_left: self.grant.max_turns.saturating_sub(self.turn),
                steps_left: u8::try_from(
                    u32::from(self.grant.max_steps).saturating_sub(self.steps + 2),
                )
                .unwrap_or(u8::MAX),
                browse_available: true,
            };
            let notices = std::mem::take(&mut self.notices);
            let view = TurnView {
                objective: &self.objective,
                decisions: &self.decisions,
                bodies: &self.bodies,
                steps: &execution.steps,
                current: &execution.artifacts,
                budget,
                remaining: self.remaining(),
                notices: &notices,
            };
            let disclosed = disclose(
                &view,
                &mut self.previews,
                &mut self.inherited,
                &self.kept,
                &mut self.thread,
            );
            let disclosure = match disclosed {
                Ok(disclosure) => disclosure,
                Err(error) => {
                    self.report(WorkAgentDiagnostic::DisclosureRefused { error });
                    return self
                        .fail_turn(if error == WorkError::Capacity {
                            CONTEXT_FULL
                        } else {
                            TURN_UNPREPARED
                        })
                        .await;
                }
            };
            let trace = WorkSynthesisTrace {
                work: self.probe.work(),
                execution: self.probe.execution(),
                attempt: self.probe.attempt(),
            };
            let result = tokio::time::timeout_at(
                self.probe.deadline().into(),
                providers.turn.turn(&disclosure, trace),
            )
            .await;
            let mut reported = false;
            let mut failed_note = "The model did not answer";
            let (turn, usage) = match result {
                Ok(Ok(result)) => {
                    if !result.usage.within(self.remaining()) {
                        return Err(WorkError::OutcomeUnknown);
                    }
                    self.charge(result.usage);
                    let turn = match disclosure.resolve(result.output) {
                        Ok(turn) => Some(turn),
                        Err(refusal) => {
                            self.notice(refusal.notice());
                            self.report(WorkAgentDiagnostic::TurnRefused {
                                turn: self.turn,
                                reason: Some(refusal),
                            });
                            reported = true;
                            failed_note = match refusal {
                                WorkAgentTurnRefusal::Empty => "The agent did nothing this turn",
                                WorkAgentTurnRefusal::Oversized => {
                                    "The agent's turn was too large to use"
                                }
                            };
                            None
                        }
                    };
                    (turn, result.usage)
                }
                Ok(Err(WorkSynthesisError::NotDispatched(error))) => {
                    failed_note = match error {
                        WorkError::Capacity => "The request grew too large for the model",
                        _ => "The model could not be reached",
                    };
                    (None, WorkUsage::default())
                }
                Ok(Err(WorkSynthesisError::Rejected(usage))) => {
                    failed_note = "The model's turn could not be used";
                    self.charge(usage);
                    (None, usage)
                }
                Ok(Err(WorkSynthesisError::Stalled(usage))) => {
                    self.charge(usage);
                    (None, usage)
                }
                Ok(Err(WorkSynthesisError::OutcomeUnknown)) | Err(_) => {
                    let mut step = self.step(WorkStepKindV1::Turn, WorkStepStatus::OutcomeUnknown);
                    step.note = None;
                    let _ = self.begin(step, vec![], None).await;
                    return Err(WorkError::OutcomeUnknown);
                }
            };
            let mut record = self.step(
                WorkStepKindV1::Turn,
                if turn.is_some() {
                    WorkStepStatus::Succeeded
                } else {
                    WorkStepStatus::Failed
                },
            );
            record.usage = Some(usage);
            record.note = match &turn {
                Some(turn) if turn.finish => None,
                Some(turn) => turn.say.clone(),
                None => Some(failed_note.to_owned()),
            };
            self.begin(record, vec![], None).await?;
            match &turn {
                Some(turn) => self.report(WorkAgentDiagnostic::TurnAdmitted {
                    turn: self.turn,
                    artifacts: turn.artifacts.len(),
                    dropped: turn.dropped,
                    fetches: turn.fetch.len(),
                    asks: turn.ask.is_some(),
                    finish: turn.finish,
                }),
                None => {
                    if !reported {
                        self.report(WorkAgentDiagnostic::TurnRefused {
                            turn: self.turn,
                            reason: None,
                        });
                    }
                }
            }
            let Some(mut turn) = turn else {
                self.failed_turns += 1;
                if self.failed_turns >= MAX_FAILED_TURNS {
                    return Ok(WorkAttemptStatus::Failed);
                }
                continue;
            };
            for notice in std::mem::take(&mut turn.notices) {
                self.notice(&notice);
            }
            // A turn that only proposed refused objects is idle: the refusal
            // notices go back, but idle turns end the run like refused ones.
            let idle = !turn.finish
                && turn.ask.is_none()
                && turn.fetch.is_empty()
                && turn.artifacts.is_empty();
            if idle {
                self.failed_turns += 1;
            } else {
                self.failed_turns = 0;
            }
            let proposed_artifacts = turn.artifacts.len() + turn.dropped;
            let mut published_this_turn = 0;
            let mut repeated = false;
            turn.fetch.retain(|kind| {
                if execution
                    .steps
                    .iter()
                    .any(|step| reuses_completed_read(kind, step))
                {
                    repeated = true;
                    false
                } else {
                    true
                }
            });
            if repeated {
                self.notice("Repeated read skipped: this page and record schema already produced the cited canvas results shown in artifacts. Use those results, follow an observed source link_destination for missing details, or finish. No new browser work was dispatched.");
            }
            for refusal in &turn.refusals {
                self.report(WorkAgentDiagnostic::ArtifactRefused {
                    turn: self.turn,
                    reason: *refusal,
                });
                self.notice(&format!(
                    "A proposed object was refused last turn: it {}.",
                    refusal.notice()
                ));
            }
            if idle && self.failed_turns >= MAX_FAILED_TURNS {
                return Ok(WorkAttemptStatus::Failed);
            }
            if !turn.artifacts.is_empty() {
                self.probe
                    .record_activity(WorkActivityV1::ProducingArtifact);
                let headroom = MAX_WORK_ARTIFACTS.saturating_sub(execution.artifacts.len());
                let artifacts: Vec<WorkArtifactV1> = turn
                    .artifacts
                    .into_iter()
                    .take(headroom)
                    .filter_map(|artifact| {
                        attempt
                            .mint_artifact(WorkArtifactDraft {
                                output: self.output.clone(),
                                title: artifact.title,
                                data: artifact.data,
                                evidence: artifact.evidence,
                            })
                            .ok()
                    })
                    .collect();
                if !artifacts.is_empty() {
                    let mut step = self.step(WorkStepKindV1::Publish, WorkStepStatus::Succeeded);
                    step.artifacts = artifacts.iter().map(|a| a.id).collect();
                    step.note = Some(publish_note(&artifacts));
                    self.published += artifacts.len();
                    published_this_turn = artifacts.len();
                    self.begin(step, artifacts, None).await?;
                }
            }
            if !turn.fetch.is_empty() {
                let leaked = turn.fetch.iter().any(|kind| match kind {
                    WorkStepKindV1::Search { query } | WorkStepKindV1::Discover { query, .. } => {
                        query_discloses(query, &self.private)
                    }
                    _ => false,
                });
                if leaked {
                    self.failed_turns += 1;
                    self.notice("A search query repeated private context and was not sent. Rephrase the query without that text.");
                    if self.failed_turns >= MAX_FAILED_TURNS {
                        return Ok(WorkAttemptStatus::Failed);
                    }
                    continue;
                }
                let status = self.fetch(attempt, providers, browser, turn.fetch).await?;
                if let Some(status) = status {
                    return Ok(status);
                }
            }
            if let Some(question) = turn.ask {
                let status = self.ask(question).await?;
                if let Some(status) = status {
                    return Ok(status);
                }
            }
            if proposed_artifacts > 0 {
                self.pending_output_repair = published_this_turn < proposed_artifacts;
            }
            if turn.finish && (self.published == 0 || self.pending_output_repair) {
                if self.finish_refusals >= 2 {
                    return Ok(WorkAttemptStatus::Failed);
                }
                self.finish_refusals += 1;
                self.notice("Finish was refused: the requested result has not been published successfully. Repair the refused object using the existing sources and publish it before finishing. Earlier partial results do not replace that object.");
                continue;
            }
            if turn.finish {
                self.probe.record_activity(WorkActivityV1::Finishing);
                let mut step = self.step(
                    WorkStepKindV1::Finish {
                        followups: turn.followups,
                    },
                    WorkStepStatus::Succeeded,
                );
                step.note = turn.say;
                self.begin(step, vec![], None).await?;
                return Ok(WorkAttemptStatus::Succeeded);
            }
        }
    }

    /// Searches and reads each use bounded batches after their prerequisites.
    /// Returns a terminal attempt status when the run cannot continue.
    async fn fetch<B, Fut>(
        &mut self,
        attempt: &WorkNodeAttempt,
        providers: &WorkAgentProviders<'_>,
        browser: &mut B,
        fetches: Vec<WorkStepKindV1>,
    ) -> Result<Option<WorkAttemptStatus>, WorkError>
    where
        B: FnMut(WorkAttemptProbe, WorkAgentBrowseRequest) -> Fut,
        Fut: Future<Output = Result<WorkBrowserOutcome, WorkError>>,
    {
        let cap = usize::from(self.limits.max_workers).max(1);
        let (searches, browses): (Vec<_>, Vec<_>) = fetches
            .into_iter()
            .partition(|kind| matches!(kind, WorkStepKindV1::Search { .. }));
        let mut offset = 0;
        while offset < searches.len() {
            let steps = usize::from(self.grant.max_steps).saturating_sub(self.steps as usize + 1);
            if steps == 0 {
                self.notice(STEPS_EXHAUSTED);
                break;
            }
            let Some(remaining) = reads::remaining_limits(self.limits, self.used) else {
                break;
            };
            let mut count = cap
                .min(searches.len() - offset)
                .min(steps)
                .min(remaining.model_tokens as usize)
                .min(remaining.cost_micro_usd as usize)
                .min(remaining.operations as usize);
            let shares =
                loop {
                    let Some(shares) = reads::budget_shares(self.limits, self.used, count) else {
                        break None;
                    };
                    let fits = searches[offset..offset + count].iter().zip(&shares).all(
                        |(kind, limits)| {
                            let WorkStepKindV1::Search { query } = kind else {
                                return false;
                            };
                            let scope = WorkPublicSearchScope {
                                provider: self.grant.provider,
                                model: self.grant.model.clone(),
                                query: query.clone(),
                            };
                            providers
                                .search
                                .minimum_reservation(&scope, &[])
                                .is_none_or(|usage| usage.within(*limits))
                        },
                    );
                    if fits || count == 1 {
                        break Some(shares);
                    }
                    count -= 1;
                };
            let Some(shares) = shares else {
                break;
            };
            let batch = &searches[offset..offset + count];
            let mut running = Vec::new();
            for (kind, limits) in batch.iter().zip(shares) {
                let WorkStepKindV1::Search { query } = kind else {
                    continue;
                };
                let step = self.step(kind.clone(), WorkStepStatus::Running);
                let id = self.begin(step, vec![], None).await?;
                running.push((
                    id,
                    WorkPublicSearchScope {
                        provider: self.grant.provider,
                        model: self.grant.model.clone(),
                        query: query.clone(),
                    },
                    limits,
                ));
            }
            let futures: Vec<Pin<Box<dyn Future<Output = Fetched> + Send + '_>>> = running
                .into_iter()
                .map(|(id, scope, limits)| {
                    let probe = self.probe.clone();
                    let search = providers.search;
                    Box::pin(async move {
                        let outcome = probe.run_search(search, &scope, &[], limits).await;
                        Fetched::Search(
                            id,
                            match outcome {
                                Ok(outcome) => WorkSearchOutcomeOwned {
                                    status: outcome.status,
                                    usage: outcome.usage,
                                    note: outcome.note,
                                    record: outcome.record,
                                },
                                // Nothing was sent: a failed step, not a lost one.
                                Err(_) => WorkSearchOutcomeOwned {
                                    status: WorkAttemptStatus::Failed,
                                    usage: Some(WorkUsage::default()),
                                    note: Some("The search could not be run"),
                                    record: None,
                                },
                            },
                        )
                    }) as Pin<Box<dyn Future<Output = Fetched> + Send + '_>>
                })
                .collect();
            let mut terminal = None;
            for fetched in join_all(futures).await {
                terminal = terminal.or(self.settle_fetched(attempt, fetched).await?);
            }
            if terminal.is_some() {
                return Ok(terminal);
            }
            offset += count;
        }
        let (file_steps, browses): (Vec<_>, Vec<_>) =
            browses.into_iter().partition(WorkStepKindV1::files);
        for kind in file_steps {
            if let Some(terminal) = self.file_step(kind).await? {
                return Ok(Some(terminal));
            }
        }
        self.fetch_browses(attempt, browser, browses).await
    }

    /// One step on the person's files. Reads settle at once with a record
    /// the agent can cite; a proposed change waits for the person's decision.
    async fn file_step(
        &mut self,
        kind: WorkStepKindV1,
    ) -> Result<Option<WorkAttemptStatus>, WorkError> {
        let Some(files) = self.files.clone() else {
            self.notice("No folder is granted in this run. Ask the person to add a folder to the canvas before reading or changing files.");
            return Ok(None);
        };
        if self.steps + 2 > u32::from(self.grant.max_steps) {
            self.notice(STEPS_EXHAUSTED);
            return Ok(None);
        }
        let step = self.step(kind.clone(), WorkStepStatus::Running);
        let id = self.begin(step, vec![], None).await?;
        let outcome = match &kind {
            WorkStepKindV1::List { path } => {
                self.probe.record_activity(WorkActivityV1::Reading);
                files.list(path)
            }
            WorkStepKindV1::ReadFile { path } => {
                self.probe.record_activity(WorkActivityV1::Reading);
                files.read(path)
            }
            WorkStepKindV1::SearchFiles { path, query } => {
                self.probe.record_activity(WorkActivityV1::Searching);
                files.search(path, query)
            }
            WorkStepKindV1::WriteFile { path, content, .. } => {
                match files.propose_write(path, content) {
                    Ok(_) => return self.await_decision(id, &files, &kind).await,
                    Err(error) => Err(error),
                }
            }
            WorkStepKindV1::EditFile { path, old, new, .. } => {
                match files.propose_edit(path, old, new) {
                    Ok(_) => return self.await_decision(id, &files, &kind).await,
                    Err(error) => Err(error),
                }
            }
            _ => return Ok(None),
        };
        self.settle_file_outcome(id, outcome).await?;
        Ok(None)
    }
    async fn settle_file_outcome(
        &mut self,
        id: WorkStepId,
        outcome: Result<WorkFileEvidenceV1, crate::work_files::WorkFileError>,
    ) -> Result<(), WorkError> {
        match outcome {
            Ok(file) => {
                let record = WorkFileRecordV1 {
                    id: WorkArtifactId::generate(),
                    node: self.probe.node(),
                    attempt: self.probe.attempt(),
                    file,
                };
                self.keep_file_preview(&record);
                let note = Some(match record.file.kind {
                    WorkFileKindV1::Directory => format!("{} entries", record.file.bytes),
                    WorkFileKindV1::Search => format!("{} hits", record.file.bytes),
                    WorkFileKindV1::Written => "Applied".to_owned(),
                    WorkFileKindV1::Text | WorkFileKindV1::Binary => {
                        format!("{} bytes", record.file.bytes)
                    }
                });
                self.settle_file(id, WorkStepStatus::Succeeded, Some(record), note)
                    .await
            }
            Err(error) => {
                self.settle_file(
                    id,
                    WorkStepStatus::Failed,
                    None,
                    Some(error.note().to_owned()),
                )
                .await
            }
        }
    }
    /// Waits for the person's decision on a proposed change, then applies it.
    async fn await_decision(
        &mut self,
        id: WorkStepId,
        files: &crate::work_files::WorkFileGrant,
        kind: &WorkStepKindV1,
    ) -> Result<Option<WorkAttemptStatus>, WorkError> {
        self.probe.record_activity(WorkActivityV1::WaitingForHuman);
        loop {
            tokio::time::sleep(ASK_POLL).await;
            let state = self.probe.runtime_projection().await?;
            let execution = state
                .executions
                .iter()
                .find(|e| e.id == self.probe.execution())
                .ok_or(WorkError::NotFound)?;
            let proposed = execution
                .steps
                .iter()
                .find(|s| s.id == id)
                .ok_or(WorkError::NotFound)?;
            match proposed.kind.file_decision() {
                Some(true) => {
                    let outcome = match kind {
                        WorkStepKindV1::WriteFile { path, content, .. } => {
                            files.apply_write(path, content)
                        }
                        WorkStepKindV1::EditFile { path, old, new, .. } => {
                            files.apply_edit(path, old, new)
                        }
                        _ => return Ok(None),
                    };
                    self.settle_file_outcome(id, outcome).await?;
                    return Ok(None);
                }
                Some(false) => {
                    self.settle_file(
                        id,
                        WorkStepStatus::Failed,
                        None,
                        Some("Declined by the person".into()),
                    )
                    .await?;
                    return Ok(None);
                }
                None => {}
            }
            if self.cancelled().await {
                let status = if Instant::now() >= self.probe.deadline() {
                    WorkStepStatus::Failed
                } else {
                    WorkStepStatus::Cancelled
                };
                self.settle_file(id, status, None, None).await?;
                return Ok(Some(if status == WorkStepStatus::Failed {
                    WorkAttemptStatus::Failed
                } else {
                    WorkAttemptStatus::Cancelled
                }));
            }
        }
    }
    /// The disclosed excerpt is capped below the record so the turn stays
    /// within its preview bounds.
    fn keep_file_preview(&mut self, record: &WorkFileRecordV1) {
        const PREVIEW_BYTES: usize = 8192;
        let file = &record.file;
        let folder = std::path::Path::new(&file.path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut end = file.text.len().min(PREVIEW_BYTES);
        while !file.text.is_char_boundary(end) {
            end -= 1;
        }
        self.keep_preview(WorkEvidencePreviewV1 {
            link_destination: None,
            version: 1,
            link: WorkEvidenceLink {
                extraction_id: record.id,
                source_id: 1,
            },
            origin: format!("file://{folder}"),
            role: "file".into(),
            truncated: file.truncated || end < file.text.len(),
            text: file.text[..end].to_owned(),
            source_bytes: file.bytes.to_string(),
            source: WorkEvidenceSourceV1::File {
                path: file.path.clone(),
                name: file.name.clone(),
                file_kind: file.kind,
            },
        });
    }

    /// Commits one fetched outcome. Returns a terminal attempt status when the
    /// run cannot continue (unknown provider outcome, intervention).
    async fn settle_fetched(
        &mut self,
        attempt: &WorkNodeAttempt,
        fetched: Fetched,
    ) -> Result<Option<WorkAttemptStatus>, WorkError> {
        let mut terminal = None;
        if let Fetched::Browse(_, _, Some(prior)) = &fetched {
            self.charge(*prior);
        }
        {
            match fetched {
                Fetched::Search(id, outcome) => {
                    if let Some(usage) = outcome.usage {
                        self.charge(usage);
                    }
                    match (outcome.status, outcome.record) {
                        (WorkAttemptStatus::Succeeded, Some(record)) => {
                            let artifacts = if record.evidence.citations.is_empty() {
                                vec![]
                            } else {
                                attempt
                                    .mint_artifact(sources_draft(&self.output, &record))
                                    .map(|artifact| vec![artifact])
                                    .unwrap_or_default()
                            };
                            self.remember(&record);
                            let note = Some(sources_note(record.evidence.citations.len()));
                            self.settle(
                                id,
                                WorkStepStatus::Succeeded,
                                outcome.usage,
                                artifacts,
                                Some(record),
                                note,
                                None,
                            )
                            .await?;
                        }
                        (WorkAttemptStatus::OutcomeUnknown, _) => {
                            self.settle(
                                id,
                                WorkStepStatus::OutcomeUnknown,
                                None,
                                vec![],
                                None,
                                None,
                                None,
                            )
                            .await?;
                            terminal = Some(WorkAttemptStatus::OutcomeUnknown);
                        }
                        (status, _) => {
                            if let Some(note) = outcome.note {
                                self.report(WorkAgentDiagnostic::SearchRefused { note });
                                self.notice(&format!(
                                    "A search failed: {note}. Try one differently worded search or read a listed page."
                                ));
                            }
                            self.settle(
                                id,
                                step_status(status),
                                outcome.usage.or(Some(WorkUsage::default())),
                                vec![],
                                None,
                                outcome.note.map(str::to_owned),
                                None,
                            )
                            .await?;
                        }
                    }
                }
                Fetched::Browse(id, outcome, prior) => match outcome {
                    Ok(outcome) => {
                        if let Some(usage) = outcome.usage {
                            self.charge(usage);
                        }
                        let measurements = outcome.measurements;
                        let usage = if outcome.status == WorkStepStatus::OutcomeUnknown {
                            None
                        } else {
                            reads::combined_usage(prior, outcome.usage)
                        };
                        let artifacts: Vec<WorkArtifactV1> = outcome
                            .artifacts
                            .into_iter()
                            .filter_map(|draft| attempt.mint_artifact(draft).ok())
                            .collect();
                        let (status, artifacts) = if outcome.status == WorkStepStatus::Succeeded {
                            (WorkStepStatus::Succeeded, artifacts)
                        } else {
                            (outcome.status, vec![])
                        };
                        let note = if status == WorkStepStatus::Succeeded {
                            Some(read_note(&artifacts))
                        } else {
                            outcome.note.clone()
                        };
                        if status == WorkStepStatus::Failed {
                            self.notice(&format!(
                                "A page read failed: {}. Use another listed source or finish with what the canvas has; do not reopen that page.",
                                outcome.note.as_deref().unwrap_or("the page gave nothing")
                            ));
                        }
                        let links = browser_preview_links(&artifacts);
                        let published = artifacts.len();
                        if let Some(measured) = measurements {
                            self.report(WorkAgentDiagnostic::ReadMeasured(measured));
                        }
                        self.settle(id, status, usage, artifacts, None, note, measurements)
                            .await?;
                        self.published += published;
                        for link in links {
                            if self.cancelled().await {
                                break;
                            }
                            let cached = self.previews.iter().find(|p| p.link == link).cloned();
                            let preview = match cached {
                                Some(preview) => Ok(preview),
                                None => self.probe.read_evidence(link).await,
                            };
                            if let Ok(preview) = preview {
                                self.keep_preview(preview);
                            }
                        }
                        if outcome.status == WorkStepStatus::OutcomeUnknown {
                            terminal = Some(WorkAttemptStatus::OutcomeUnknown);
                        }
                        // An anonymous read that needs a person is one failed
                        // page, not the end of the run: its note says why.
                        let _ = outcome.intervention;
                    }
                    Err(WorkError::OutcomeUnknown) => {
                        self.settle(
                            id,
                            WorkStepStatus::OutcomeUnknown,
                            None,
                            vec![],
                            None,
                            None,
                            None,
                        )
                        .await?;
                        terminal = Some(WorkAttemptStatus::OutcomeUnknown);
                    }
                    Err(_) => {
                        self.settle(
                            id,
                            WorkStepStatus::Failed,
                            prior.or(Some(WorkUsage::default())),
                            vec![],
                            None,
                            None,
                            None,
                        )
                        .await?;
                    }
                },
            }
        }
        Ok(terminal)
    }

    async fn ask(
        &mut self,
        question: WorkAgentQuestion,
    ) -> Result<Option<WorkAttemptStatus>, WorkError> {
        self.probe.record_activity(WorkActivityV1::WaitingForHuman);
        let step = self.step(
            WorkStepKindV1::Ask {
                prompt: question.prompt,
                options: question.options,
                answer: None,
            },
            WorkStepStatus::Running,
        );
        let id = self.begin(step, vec![], None).await?;
        loop {
            tokio::time::sleep(ASK_POLL).await;
            let state = self.probe.runtime_projection().await?;
            let execution = state
                .executions
                .iter()
                .find(|e| e.id == self.probe.execution())
                .ok_or(WorkError::NotFound)?;
            let asked = execution
                .steps
                .iter()
                .find(|s| s.id == id)
                .ok_or(WorkError::NotFound)?;
            if asked.status == WorkStepStatus::Succeeded {
                if let WorkStepKindV1::Ask {
                    prompt,
                    answer: Some(answer),
                    ..
                } = &asked.kind
                {
                    self.decisions.push(planning::PlanningAnswer {
                        question: prompt.clone(),
                        answer: answer.clone(),
                    });
                }
                return Ok(None);
            }
            // An open question suspends the run rather than spending it: near
            // the deadline the run stops on the question, and an answer or
            // Continue resumes it as the next request.
            if Instant::now() + ASK_RESERVE >= self.probe.deadline() {
                self.report(WorkAgentDiagnostic::AskSuspended);
                if let Err(error) = self.probe.request_stop().await {
                    self.report(WorkAgentDiagnostic::CommitRefused {
                        kind: "suspend",
                        error,
                    });
                }
                self.settle(
                    id,
                    WorkStepStatus::Cancelled,
                    None,
                    vec![],
                    None,
                    Some(ASK_SUSPENDED.into()),
                    None,
                )
                .await?;
                return Ok(Some(WorkAttemptStatus::Cancelled));
            }
            if self.cancelled().await {
                self.settle(
                    id,
                    WorkStepStatus::Cancelled,
                    None,
                    vec![],
                    None,
                    None,
                    None,
                )
                .await?;
                return Ok(Some(WorkAttemptStatus::Cancelled));
            }
        }
    }

    /// Seeds the thread from this work's earlier executions. Objects come
    /// whole from the last few runs, plus older ones the request names; a
    /// question the last run stopped on is answered by this request.
    async fn inherit(&mut self, projection: &WorkRuntimeProjection, current: WorkExecutionId) {
        let earlier: Vec<&WorkExecutionFact> = projection
            .executions
            .iter()
            .filter(|execution| execution.id != current)
            .collect();
        let produced: Vec<&[WorkArtifactV1]> = earlier
            .iter()
            .map(|execution| execution.artifacts.as_slice())
            .collect();
        let (artifacts, kept) = inheritable(&produced, &self.objective, &self.bodies);
        let recent = earlier.len().saturating_sub(INHERITED_EXECUTIONS);
        for execution in earlier[recent..].iter().rev() {
            for record in &execution.provider_evidence {
                if self.previews.len() >= INHERITED_PREVIEWS {
                    break;
                }
                self.remember(record);
            }
        }
        for artifact in artifacts.iter().rev() {
            for link in &artifact.evidence {
                if self.previews.len() >= INHERITED_PREVIEWS {
                    break;
                }
                if self.previews.iter().any(|preview| preview.link == *link) {
                    continue;
                }
                if let Ok(preview) = self.probe.read_evidence(link.clone()).await {
                    self.keep_preview(preview);
                }
            }
        }
        self.inherited = artifacts;
        self.kept = kept;
        if let Some((question, _)) = earlier.last().and_then(|last| pending_question(last)) {
            let answer = projection
                .executions
                .iter()
                .find(|execution| execution.id == current)
                .and_then(|execution| execution.spec.request.clone())
                .unwrap_or_else(|| self.objective.clone());
            self.decisions.push(planning::PlanningAnswer {
                question: question.to_owned(),
                answer,
            });
        }
        let mut thread: Vec<WorkAgentThreadEntry> = Vec::new();
        for execution in &earlier {
            let Some(request) = &execution.spec.request else {
                continue;
            };
            if thread.last().is_some_and(|entry| entry.request == *request) {
                continue;
            }
            thread.push(WorkAgentThreadEntry {
                request: request.clone(),
                ended: execution_ended(execution.status),
                summary: execution_summary(execution),
            });
        }
        if thread.last().is_some_and(|entry| entry.request == self.objective) {
            thread.pop();
        }
        self.thread = thread.into_iter().rev().take(16).rev().collect();
    }
    fn remember(&mut self, record: &WorkProviderSearchRecordV1) {
        let preferred = record
            .ranking
            .as_ref()
            .map(|ranking| ranking.preferred.as_slice())
            .unwrap_or_default();
        let mut seen = std::collections::BTreeSet::new();
        let indices = preferred.iter().map(|id| usize::from(*id) - 1).chain(
            (0..record.evidence.citations.len())
                .filter(|index| !preferred.contains(&((*index + 1) as u16))),
        );
        for index in indices {
            let citation = &record.evidence.citations[index];
            if !seen.insert(source_key(&citation.url)) {
                continue;
            }
            let Ok(text) = record.evidence.citation_excerpt(index) else {
                continue;
            };
            let Ok(origin) = url::Url::parse(&citation.url) else {
                continue;
            };
            let source_bytes = record.evidence.answer.len();
            self.keep_preview(WorkEvidencePreviewV1 {
                link_destination: None,
                version: 1,
                link: WorkEvidenceLink {
                    extraction_id: record.id,
                    source_id: (index + 1) as u16,
                },
                origin: origin.origin().ascii_serialization(),
                role: "provider_search".into(),
                truncated: text.len() < source_bytes,
                text,
                source_bytes: source_bytes.to_string(),
                source: WorkEvidenceSourceV1::ProviderSearch {
                    provider: record.evidence.provider,
                    model: record.evidence.model.clone(),
                    url: citation.url.clone(),
                    title: citation.title.clone(),
                    response_id: record.evidence.response_id.clone(),
                    search_call_id: record.evidence.search_call_id.clone(),
                },
            });
        }
    }
    fn keep_preview(&mut self, preview: WorkEvidencePreviewV1) {
        if let Some(index) = self.previews.iter().position(|p| p.link == preview.link) {
            self.previews.remove(index);
        }
        if self.previews.len() >= MAX_PREVIEWS {
            self.previews.remove(0);
        }
        self.previews.push(preview);
    }
}

/// What one turn discloses besides the context it may shed.
struct TurnView<'a> {
    objective: &'a str,
    decisions: &'a [planning::PlanningAnswer],
    bodies: &'a [context::WorkContextBody],
    steps: &'a [WorkStepFact],
    current: &'a [WorkArtifactV1],
    budget: WorkAgentBudget,
    remaining: WorkExecutionLimits,
    notices: &'a [String],
}

/// Builds the turn, shedding until it fits: the disclosure itself first drops
/// older object bodies and shortens source text; then uncited sources go,
/// then inherited objects oldest first (never the kept ones), then thread
/// summaries oldest first, and only then the oldest thread entries. The
/// canvas keeps everything; only the model's view shrinks.
fn disclose(
    view: &TurnView<'_>,
    previews: &mut Vec<WorkEvidencePreviewV1>,
    inherited: &mut Vec<WorkArtifactV1>,
    kept: &[WorkArtifactId],
    thread: &mut Vec<WorkAgentThreadEntry>,
) -> Result<WorkAgentTurnDisclosure, WorkError> {
    loop {
        let room = MAX_WORK_ARTIFACTS.saturating_sub(view.current.len());
        let artifacts: Vec<WorkArtifactV1> = inherited
            .iter()
            .rev()
            .take(room)
            .rev()
            .chain(view.current)
            .cloned()
            .collect();
        let disclosed = WorkAgentTurnDisclosure::try_new(
            view.objective,
            view.decisions.to_vec(),
            view.bodies.to_vec(),
            view.steps,
            previews,
            &artifacts,
            view.budget,
            view.remaining,
            view.notices.to_vec(),
        )
        .and_then(|disclosure| disclosure.with_thread(thread.clone()));
        match disclosed {
            Err(WorkError::Capacity) => {}
            other => return other,
        }
        if evict_previews(previews, &artifacts) {
            continue;
        }
        let before = inherited.len();
        let mut shed = 0;
        inherited.retain(|artifact| {
            if shed < 4 && !kept.contains(&artifact.id) {
                shed += 1;
                false
            } else {
                true
            }
        });
        if inherited.len() < before {
            continue;
        }
        if let Some(entry) = thread.iter_mut().find(|entry| entry.summary.is_some()) {
            entry.summary = None;
            continue;
        }
        if thread.len() > 1 {
            thread.remove(0);
            continue;
        }
        return Err(WorkError::Capacity);
    }
}

/// Drops the oldest sources no current object cites; false when none can go.
fn evict_previews(previews: &mut Vec<WorkEvidencePreviewV1>, artifacts: &[WorkArtifactV1]) -> bool {
    let cited: Vec<&WorkEvidenceLink> = artifacts
        .iter()
        .flat_map(|artifact| artifact.evidence.iter())
        .collect();
    let before = previews.len();
    let mut dropped = 0;
    previews.retain(|preview| {
        if dropped < 16 && !cited.iter().any(|link| **link == preview.link) {
            dropped += 1;
            false
        } else {
            true
        }
    });
    previews.len() < before
}

/// Objects a run inherits, oldest first, from earlier executions' objects
/// (oldest execution first): everything from the last few runs plus older
/// subjects and comparisons the request names. Kept: those named and those
/// the latest producing run placed.
fn inheritable(
    executions: &[&[WorkArtifactV1]],
    objective: &str,
    bodies: &[context::WorkContextBody],
) -> (Vec<WorkArtifactV1>, Vec<WorkArtifactId>) {
    let recent = executions.len().saturating_sub(INHERITED_EXECUTIONS);
    let latest = executions
        .iter()
        .rposition(|artifacts| !artifacts.is_empty());
    let mut artifacts = Vec::new();
    let mut kept = Vec::new();
    for (index, produced) in executions.iter().enumerate().rev() {
        for artifact in produced.iter().rev() {
            if artifacts.len() >= INHERITED_ARTIFACTS {
                break;
            }
            let named = referenced(artifact, objective, bodies);
            let last = Some(index) == latest;
            if index < recent && !named && !last {
                continue;
            }
            if named || last {
                kept.push(artifact.id);
            }
            artifacts.push(artifact.clone());
        }
    }
    artifacts.reverse();
    (artifacts, kept)
}

/// A subject or comparison object the request names by its title or one of
/// its subjects, or selects as context.
fn referenced(
    artifact: &WorkArtifactV1,
    objective: &str,
    bodies: &[context::WorkContextBody],
) -> bool {
    let subjects: &[WorkSubject] = match &artifact.data {
        WorkArtifactDataV1::ComparisonMatrix { subjects, .. }
        | WorkArtifactDataV1::Findings { subjects, .. }
        | WorkArtifactDataV1::EvidenceCollection { subjects, .. } => subjects,
        WorkArtifactDataV1::Comparison { .. } => &[],
        _ => return false,
    };
    if subjects.is_empty() && !matches!(artifact.data, WorkArtifactDataV1::Comparison { .. }) {
        return false;
    }
    let objective = objective.to_lowercase();
    let named = |name: &str| {
        let name = name.trim().to_lowercase();
        name.chars().count() >= 3 && objective.contains(&name)
    };
    named(&artifact.title)
        || subjects.iter().any(|subject| named(&subject.name))
        || bodies.iter().any(|body| {
            matches!(
                body.kind,
                context::WorkContextItemKind::Artifact | context::WorkContextItemKind::Subject
            ) && (body.title == artifact.title
                || subjects.iter().any(|subject| subject.name == body.title))
        })
}

/// The question a settled execution stopped on, still unanswered.
fn pending_question(execution: &WorkExecutionFact) -> Option<(&str, &[String])> {
    if !execution.status.terminal() {
        return None;
    }
    execution
        .steps
        .iter()
        .rev()
        .find_map(|step| match &step.kind {
            WorkStepKindV1::Ask {
                prompt,
                options,
                answer: None,
            } if step.status != WorkStepStatus::Running => {
                Some((prompt.as_str(), options.as_slice()))
            }
            _ => None,
        })
}

fn reuses_completed_read(request: &WorkStepKindV1, step: &WorkStepFact) -> bool {
    if step.status != WorkStepStatus::Succeeded || step.artifacts.is_empty() {
        return false;
    }
    match (request, &step.kind) {
        (
            WorkStepKindV1::Read { url, collection },
            WorkStepKindV1::Read {
                url: previous_url,
                collection: previous,
            },
        ) if url == previous_url => match (collection, previous) {
            (None, None) => true,
            (Some(current), Some(previous)) => {
                current.max_items == previous.max_items
                    && current.columns.len() == previous.columns.len()
                    && current.columns.iter().all(|column| {
                        previous.columns.iter().any(|old| {
                            old.name == column.name
                                && old.value == column.value
                                && old.required == column.required
                        })
                    })
            }
            _ => false,
        },
        _ => false,
    }
}

fn browser_preview_links(artifacts: &[WorkArtifactV1]) -> Vec<WorkEvidenceLink> {
    let mut links = Vec::new();
    for index in 0..MAX_ARTIFACT_EVIDENCE {
        for artifact in artifacts {
            if let Some(link) = artifact.evidence.get(index) {
                if !links.contains(link) {
                    links.push(link.clone());
                    if links.len() == MAX_PREVIEWS {
                        return links;
                    }
                }
            }
        }
    }
    links
}

fn step_kind_label(kind: &WorkStepKindV1) -> &'static str {
    match kind {
        WorkStepKindV1::Turn => "turn",
        WorkStepKindV1::Search { .. } => "search",
        WorkStepKindV1::Read { .. } => "read",
        WorkStepKindV1::Discover { .. } => "discover",
        WorkStepKindV1::Publish => "publish",
        WorkStepKindV1::Ask { .. } => "ask",
        WorkStepKindV1::Steer { .. } => "person",
        WorkStepKindV1::List { .. } => "list",
        WorkStepKindV1::ReadFile { .. } => "read_file",
        WorkStepKindV1::SearchFiles { .. } => "search_files",
        WorkStepKindV1::WriteFile { .. } => "write_file",
        WorkStepKindV1::EditFile { .. } => "edit_file",
        WorkStepKindV1::Finish { .. } => "finish",
    }
}

fn step_status(status: WorkAttemptStatus) -> WorkStepStatus {
    match status {
        WorkAttemptStatus::Running => WorkStepStatus::Running,
        WorkAttemptStatus::Succeeded => WorkStepStatus::Succeeded,
        WorkAttemptStatus::Failed => WorkStepStatus::Failed,
        WorkAttemptStatus::Cancelled => WorkStepStatus::Cancelled,
        WorkAttemptStatus::OutcomeUnknown => WorkStepStatus::OutcomeUnknown,
    }
}

/// Provider citations become one sources object: every citation is an entry
/// the canvas can show and later work can cite.
fn sources_draft(output: &str, record: &WorkProviderSearchRecordV1) -> WorkArtifactDraft {
    // One card per page: the provider often cites the same page twice.
    let mut seen = std::collections::BTreeSet::new();
    let entries = record
        .evidence
        .citations
        .iter()
        .take(64)
        .enumerate()
        .filter(|(_, citation)| seen.insert(source_key(&citation.url)))
        .map(|(index, citation)| WorkSourceEntry {
            evidence: index as u16,
            title: source_title(&citation.title, &citation.url),
            role: "source".into(),
            subject: None,
        })
        .collect();
    let evidence = (1..=record.evidence.citations.len().min(64))
        .map(|index| WorkEvidenceLink {
            extraction_id: record.id,
            source_id: index as u16,
        })
        .collect();
    let summary: String = record.evidence.answer.chars().take(4000).collect();
    WorkArtifactDraft {
        output: output.to_owned(),
        title: "Sources".into(),
        data: WorkArtifactDataV1::EvidenceCollection {
            summary: if summary.trim().is_empty() {
                "Sources found".into()
            } else {
                summary
            },
            subjects: vec![],
            entries,
        },
        evidence,
    }
}
/// Provider tracking parameters do not make a different page.
fn source_key(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) => {
            let kept: Vec<(String, String)> = parsed
                .query_pairs()
                .filter(|(key, _)| key != "utm_source")
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
            parsed.set_query(None);
            if !kept.is_empty() {
                parsed.query_pairs_mut().extend_pairs(kept);
            }
            parsed.set_fragment(None);
            parsed.to_string().trim_end_matches('/').to_lowercase()
        }
        Err(_) => url.to_lowercase(),
    }
}
fn source_title(title: &str, url: &str) -> String {
    let title: String = title.trim().chars().take(200).collect();
    if !title.is_empty() {
        return title;
    }
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| "Source".into())
}
fn sources_note(count: usize) -> String {
    match count {
        0 => "No sources found".into(),
        1 => "Found 1 source".into(),
        n => format!("Found {n} sources"),
    }
}
fn read_note(artifacts: &[WorkArtifactV1]) -> String {
    if let [artifact] = artifacts {
        match &artifact.data {
            WorkArtifactDataV1::ComparisonMatrix { subjects, .. } => {
                return format!("Placed {} cited records on the canvas", subjects.len());
            }
            WorkArtifactDataV1::Findings { items, .. } => {
                return format!("Placed {} cited findings on the canvas", items.len());
            }
            _ => {}
        }
    }
    match artifacts.len() {
        0 => "Read the page".into(),
        _ => "Read the page and kept notes".into(),
    }
}
fn publish_note(artifacts: &[WorkArtifactV1]) -> String {
    let mut kinds: Vec<&str> = artifacts.iter().map(|a| artifact_kind(&a.data)).collect();
    kinds.sort_unstable();
    kinds.dedup();
    format!(
        "Placed {} on the canvas",
        kinds.join(", ").replace('_', " ")
    )
}

/// Search text may not repeat a private context body: a 24-character window
/// of the query found inside any private body refuses the whole turn.
pub fn query_discloses(query: &str, private: &[String]) -> bool {
    const WINDOW: usize = 24;
    let query: Vec<char> = query.to_lowercase().chars().collect();
    if query.len() < WINDOW || private.is_empty() {
        return false;
    }
    let bodies: Vec<String> = private.iter().map(|body| body.to_lowercase()).collect();
    query.windows(WINDOW).any(|window| {
        let window: String = window.iter().collect();
        bodies.iter().any(|body| body.contains(&window))
    })
}

/// Bounded fan-out without a futures dependency: polls every pending future
/// each wake and returns once all have settled.
async fn join_all<T>(mut futures: Vec<Pin<Box<dyn Future<Output = T> + Send + '_>>>) -> Vec<T> {
    let mut results: Vec<Option<T>> = futures.iter().map(|_| None).collect();
    std::future::poll_fn(|cx: &mut Context<'_>| {
        let mut pending = false;
        for (index, future) in futures.iter_mut().enumerate() {
            if results[index].is_some() {
                continue;
            }
            match future.as_mut().poll(cx) {
                Poll::Ready(value) => results[index] = Some(value),
                Poll::Pending => pending = true,
            }
        }
        if pending {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
    results.into_iter().flatten().collect()
}

fn execution_ended(status: WorkExecutionStatus) -> &'static str {
    match status {
        WorkExecutionStatus::Completed | WorkExecutionStatus::NeedsReview => "completed",
        WorkExecutionStatus::Cancelled | WorkExecutionStatus::CancelRequested => "stopped",
        WorkExecutionStatus::Failed => "failed",
        WorkExecutionStatus::Interrupted => "interrupted",
        WorkExecutionStatus::Approved | WorkExecutionStatus::Running => "running",
    }
}

/// The run's last line for the person, or the note of the step that ended
/// it, plus what it did; the model reads it, so it is clipped.
fn execution_summary(execution: &WorkExecutionFact) -> Option<String> {
    let last = execution
        .steps
        .iter()
        .rev()
        .find_map(|step| step.note.as_deref().filter(|note| !note.trim().is_empty()));
    let searches = execution
        .steps
        .iter()
        .filter(|step| matches!(step.kind, WorkStepKindV1::Search { .. }))
        .count();
    let reads = execution
        .steps
        .iter()
        .filter(|step| {
            matches!(
                step.kind,
                WorkStepKindV1::Read { .. } | WorkStepKindV1::Discover { .. }
            ) && step.status == WorkStepStatus::Succeeded
        })
        .count();
    // A question the run stopped on leads, so clipping never loses it.
    let mut summary = match pending_question(execution) {
        Some((prompt, [])) => format!("Stopped waiting for the person's answer to: {prompt}"),
        Some((prompt, options)) => format!(
            "Stopped waiting for the person's answer to: {prompt} Options: {}.",
            options.join("; ")
        ),
        None => last.map(str::to_owned).unwrap_or_default(),
    };
    if !summary.is_empty() {
        summary.push(' ');
    }
    summary.push_str(&format!(
        "({searches} searches, {reads} pages read, {} objects placed)",
        execution.artifacts.len()
    ));
    Some(zephium_core::work::agent::clip_text(
        &summary,
        MAX_WORK_STEP_NOTE_BYTES,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_read_reuses_results_but_failed_or_changed_requests_can_run() {
        let collection = serde_json::from_value(serde_json::json!({
            "title":"Products", "max_items":3, "columns":[
                {"name":"price","required":false,"value":{"kind":"text"}}
            ]
        }))
        .unwrap();
        let request = WorkStepKindV1::Read {
            url: "https://shop.example.test/catalog?q=sets".into(),
            collection: Some(collection),
        };
        let mut step = WorkStepFact {
            id: 1.into(),
            turn: 1,
            kind: request.clone(),
            status: WorkStepStatus::Succeeded,
            usage: Some(WorkUsage::default()),
            artifacts: vec![1.into()],
            evidence: None,
            note: None,
            measurements: None,
        };
        assert!(reuses_completed_read(&request, &step));
        let mut renamed = request.clone();
        if let WorkStepKindV1::Read {
            collection: Some(schema),
            ..
        } = &mut renamed
        {
            schema.title = "Compared products".into();
        }
        assert!(reuses_completed_read(&renamed, &step));
        let mut changed_requirement = request.clone();
        if let WorkStepKindV1::Read {
            collection: Some(schema),
            ..
        } = &mut changed_requirement
        {
            schema.columns[0].required = true;
        }
        assert!(!reuses_completed_read(&changed_requirement, &step));
        let mut stricter_previous = step.clone();
        stricter_previous.kind = changed_requirement;
        assert!(!reuses_completed_read(&request, &stricter_previous));
        if let WorkStepKindV1::Read {
            collection: Some(schema),
            ..
        } = &mut renamed
        {
            schema.max_items = 5;
        }
        assert!(!reuses_completed_read(&renamed, &step));
        let mut changed_url = request.clone();
        if let WorkStepKindV1::Read { url, .. } = &mut changed_url {
            *url = "https://shop.example.test/catalog?q=other".into();
        }
        assert!(!reuses_completed_read(&changed_url, &step));
        step.status = WorkStepStatus::Failed;
        assert!(!reuses_completed_read(&request, &step));
        step.status = WorkStepStatus::Succeeded;
        step.artifacts.clear();
        assert!(!reuses_completed_read(&request, &step));
    }

    #[test]
    fn browser_previews_cover_later_artifacts_and_deduplicate_within_the_budget() {
        let artifact = |id: u128| WorkArtifactV1 {
            version: 1,
            id: id.into(),
            execution: 1.into(),
            node: 1.into(),
            attempt: 1.into(),
            output: "results".into(),
            title: "Results".into(),
            data: WorkArtifactDataV1::Findings {
                subjects: vec![],
                items: vec![],
            },
            evidence: (1..=64)
                .map(|source_id| WorkEvidenceLink {
                    extraction_id: id.into(),
                    source_id,
                })
                .collect(),
            review: WorkOutputReview::SourceMappedNeedsReview,
            presentation: WorkArtifactPresentationV1::Automatic,
        };
        let first = artifact(10);
        let second = artifact(20);
        let links = browser_preview_links(&[first.clone(), first, second]);
        assert_eq!(links.len(), MAX_PREVIEWS);
        assert_eq!(
            links
                .iter()
                .filter(|link| link.extraction_id == 10.into())
                .count(),
            48
        );
        assert_eq!(
            links
                .iter()
                .filter(|link| link.extraction_id == 20.into())
                .count(),
            48
        );
        assert!(links.iter().any(|link| link.source_id == 48));
    }

    #[test]
    fn a_long_work_still_discloses_a_turn_after_shedding() {
        let artifact = |execution: u128, index: u128, data: WorkArtifactDataV1| WorkArtifactV1 {
            version: 1,
            id: (execution * 100 + index).into(),
            execution: execution.into(),
            node: 1.into(),
            attempt: execution.into(),
            output: "results".into(),
            title: format!("Result {execution}.{index}"),
            data,
            evidence: (1..=2)
                .map(|source_id| WorkEvidenceLink {
                    extraction_id: (execution * 100 + index).into(),
                    source_id,
                })
                .collect(),
            review: WorkOutputReview::SourceMappedNeedsReview,
            presentation: WorkArtifactPresentationV1::Automatic,
        };
        let document = || WorkArtifactDataV1::Document {
            paragraphs: vec!["x".repeat(3000)],
            formatted: None,
        };
        let executions: Vec<Vec<WorkArtifactV1>> = (1..=16)
            .map(|execution| {
                (1..=4)
                    .map(|index| {
                        let data = if execution == 2 && index == 1 {
                            WorkArtifactDataV1::Findings {
                                subjects: vec![WorkSubject {
                                    name: "Lisbon shortlist".into(),
                                    descriptor: None,
                                    homepage: None,
                                    image_candidates: vec![],
                                }],
                                items: vec![],
                            }
                        } else {
                            document()
                        };
                        artifact(execution, index, data)
                    })
                    .collect()
            })
            .collect();
        let slices: Vec<&[WorkArtifactV1]> = executions.iter().map(Vec::as_slice).collect();
        let objective = "Check the visa rules for the Lisbon shortlist as well";
        let (mut inherited, kept) = inheritable(&slices, objective, &[]);
        assert_eq!(inherited.len(), 3 * 4 + 1);
        assert!(inherited.iter().any(|a| a.title == "Result 2.1"));
        assert!(kept.contains(&WorkArtifactId::from(201)) && kept.contains(&1601.into()));
        let current: Vec<WorkArtifactV1> = (1..=4)
            .map(|index| artifact(17, index, document()))
            .collect();
        let mut previews: Vec<WorkEvidencePreviewV1> = inherited
            .iter()
            .chain(&current)
            .flat_map(|a| a.evidence.clone())
            .chain((1..=40).map(|source_id| WorkEvidenceLink {
                extraction_id: 9999.into(),
                source_id,
            }))
            .map(|link| WorkEvidencePreviewV1 {
                version: 1,
                link,
                origin: "https://example.test".into(),
                role: "page".into(),
                text: "y".repeat(8000),
                truncated: false,
                source_bytes: "8000".into(),
                link_destination: None,
                source: WorkEvidenceSourceV1::NativeExtraction,
            })
            .collect();
        let mut thread: Vec<WorkAgentThreadEntry> = (0..16)
            .map(|_| WorkAgentThreadEntry {
                request: "r".repeat(1500),
                ended: "completed",
                summary: Some("s".repeat(500)),
            })
            .collect();
        let view = TurnView {
            objective,
            decisions: &[],
            bodies: &[],
            steps: &[],
            current: &current,
            budget: WorkAgentBudget {
                turns_left: 4,
                steps_left: 8,
                browse_available: true,
            },
            remaining: WorkExecutionLimits {
                model_tokens: 100_000,
                cost_micro_usd: 100_000,
                operations: 32,
                timeout_seconds: 600,
                max_workers: 1,
            },
            notices: &[],
        };
        let disclosure = disclose(&view, &mut previews, &mut inherited, &kept, &mut thread)
            .expect("a long work still gets its turn");
        let context = disclosure.context();
        assert!(previews.len() < 74, "uncited sources went first");
        assert_eq!(context.thread.len(), 16);
        for title in ["Result 2.1", "Result 16.1", "Result 17.4"] {
            assert!(context.artifacts.iter().any(|a| a.title == title));
        }
    }

    #[test]
    fn private_context_never_rides_a_search_query() {
        let private = vec!["Our Q3 revenue target is 4.2M with a hiring freeze".to_owned()];
        assert!(query_discloses(
            "revenue target is 4.2M with a hiring freeze",
            &private
        ));
        assert!(!query_discloses("best canvas libraries 2026", &private));
        assert!(!query_discloses("hiring freeze", &private));
    }
}
