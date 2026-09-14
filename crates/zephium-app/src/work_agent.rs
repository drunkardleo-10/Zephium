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

/// A browser read or discovery the loop admitted for one step. The host
/// compiles it into an anonymous, read-only public browsing task.
pub struct WorkAgentBrowseRequest {
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
}
pub struct WorkAgentProviders<'a> {
    pub turn: &'a dyn WorkAgentTurnProvider,
    pub search: &'a dyn WorkPublicSearchProvider,
}

/// Tokens a turn must still be able to spend before the loop stops itself.
const TURN_TOKEN_FLOOR: u32 = 12_000;
const ASK_POLL: Duration = Duration::from_millis(500);
const MAX_PREVIEWS: usize = 96;

/// Closed loop facts for development logs; never model, page or user text.
#[derive(Clone, Copy, Debug)]
pub enum WorkAgentDiagnostic {
    TurnAdmitted {
        turn: u8,
        artifacts: usize,
        fetches: usize,
        asks: bool,
        finish: bool,
    },
    TurnRefused {
        turn: u8,
    },
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
            previews: Vec::new(),
            used: WorkUsage::default(),
            steps: 0,
            turn: 0,
            failed_turns: 0,
            intervention: None,
            diagnostic: self.diagnostic,
        };
        let outcome = driver.drive(&attempt, &providers, &mut browser).await;
        let (status, usage) = match outcome {
            Ok(status) => (status, driver.settled_usage(status)),
            Err(WorkError::OutcomeUnknown) => (WorkAttemptStatus::OutcomeUnknown, None),
            Err(error) => return Err(error),
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
    private: Vec<String>,
    previews: Vec<WorkEvidencePreviewV1>,
    used: WorkUsage,
    steps: u32,
    turn: u8,
    failed_turns: u8,
    intervention: Option<WorkInterventionV1>,
    diagnostic: Option<fn(WorkAgentDiagnostic)>,
}

enum Fetched {
    Search(WorkStepId, WorkSearchOutcomeOwned),
    Browse(WorkStepId, Result<WorkBrowserOutcome, WorkError>),
}
struct WorkSearchOutcomeOwned {
    status: WorkAttemptStatus,
    usage: Option<WorkUsage>,
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
            })
            .await
        {
            self.report(WorkAgentDiagnostic::CommitRefused { kind, error });
            return Err(error);
        }
        self.steps += 1;
        Ok(id)
    }
    fn report(&self, event: WorkAgentDiagnostic) {
        if let Some(diagnostic) = self.diagnostic {
            diagnostic(event);
        }
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
                note,
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
        self.probe.cancellation_requested().await.unwrap_or(true)
            || Instant::now() >= self.probe.deadline()
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
                return Ok(WorkAttemptStatus::Failed);
            }
            self.turn += 1;
            self.probe.record_activity(WorkActivityV1::Planning);
            let state = self.probe.runtime_projection().await?;
            let execution = state
                .executions
                .iter()
                .find(|e| e.id == self.probe.execution())
                .ok_or(WorkError::NotFound)?;
            let disclosure = match WorkAgentTurnDisclosure::try_new(
                &self.objective,
                self.decisions.clone(),
                self.bodies.clone(),
                &execution.steps,
                &self.previews,
                &execution.artifacts,
                WorkAgentBudget {
                    turns_left: self.grant.max_turns.saturating_sub(self.turn),
                    steps_left: u8::try_from(
                        u32::from(self.grant.max_steps).saturating_sub(self.steps + 2),
                    )
                    .unwrap_or(u8::MAX),
                    browse_available: true,
                },
                self.remaining(),
            ) {
                Ok(disclosure) => disclosure,
                Err(_) => return Ok(WorkAttemptStatus::Failed),
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
            let (turn, usage) = match result {
                Ok(Ok(result)) => {
                    if !result.usage.within(self.remaining()) {
                        return Err(WorkError::OutcomeUnknown);
                    }
                    self.charge(result.usage);
                    (disclosure.resolve(result.output).ok(), result.usage)
                }
                Ok(Err(WorkSynthesisError::NotDispatched(_))) => (None, WorkUsage::default()),
                Ok(Err(WorkSynthesisError::Rejected(usage))) => {
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
            record.note = turn.as_ref().and_then(|turn| turn.say.clone());
            self.begin(record, vec![], None).await?;
            match &turn {
                Some(turn) => self.report(WorkAgentDiagnostic::TurnAdmitted {
                    turn: self.turn,
                    artifacts: turn.artifacts.len(),
                    fetches: turn.fetch.len(),
                    asks: turn.ask.is_some(),
                    finish: turn.finish,
                }),
                None => self.report(WorkAgentDiagnostic::TurnRefused { turn: self.turn }),
            }
            let Some(turn) = turn else {
                self.failed_turns += 1;
                if self.failed_turns >= 2 {
                    return Ok(WorkAttemptStatus::Failed);
                }
                continue;
            };
            self.failed_turns = 0;
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
                    self.begin(step, artifacts, None).await?;
                }
            }
            if !turn.fetch.is_empty() {
                let leaked = turn.fetch.iter().any(|kind| match kind {
                    WorkStepKindV1::Search { query } | WorkStepKindV1::Discover { query } => {
                        query_discloses(query, &self.private)
                    }
                    _ => false,
                });
                if leaked {
                    self.failed_turns += 1;
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
            if turn.finish {
                self.probe.record_activity(WorkActivityV1::Finishing);
                let step = self.step(WorkStepKindV1::Finish, WorkStepStatus::Succeeded);
                self.begin(step, vec![], None).await?;
                return Ok(WorkAttemptStatus::Succeeded);
            }
        }
    }

    /// Searches run together; native reads run one at a time after them.
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
        let mut begun = Vec::new();
        for kind in fetches {
            if self.steps + 2 > u32::from(self.grant.max_steps) {
                break;
            }
            let step = self.step(kind.clone(), WorkStepStatus::Running);
            let id = self.begin(step, vec![], None).await?;
            begun.push((id, kind));
        }
        let mut searches: Vec<Pin<Box<dyn Future<Output = Fetched> + Send + '_>>> = Vec::new();
        let mut browses = Vec::new();
        let remaining = self.remaining();
        let per_search = WorkExecutionLimits {
            max_workers: 1,
            ..remaining
        };
        for (id, kind) in begun {
            match kind {
                WorkStepKindV1::Search { query } => {
                    let scope = WorkPublicSearchScope {
                        provider: self.grant.provider,
                        model: self.grant.model.clone(),
                        query,
                    };
                    let probe = self.probe.clone();
                    let search = providers.search;
                    searches.push(Box::pin(async move {
                        let outcome = probe.run_search(search, &scope, &[], per_search).await;
                        Fetched::Search(
                            id,
                            match outcome {
                                Ok(outcome) => WorkSearchOutcomeOwned {
                                    status: outcome.status,
                                    usage: outcome.usage,
                                    record: outcome.record,
                                },
                                Err(_) => WorkSearchOutcomeOwned {
                                    status: WorkAttemptStatus::OutcomeUnknown,
                                    usage: None,
                                    record: None,
                                },
                            },
                        )
                    }));
                }
                kind => browses.push((id, kind)),
            }
        }
        let mut results = join_all(searches).await;
        for (id, kind) in browses {
            if self.cancelled().await {
                results.push(Fetched::Browse(
                    id,
                    Ok(WorkBrowserOutcome {
                        status: WorkStepStatus::Cancelled,
                        usage: Some(WorkUsage::default()),
                        artifacts: vec![],
                        intervention: None,
                    }),
                ));
                continue;
            }
            self.probe.record_activity(WorkActivityV1::Reading);
            let request = WorkAgentBrowseRequest {
                step: kind,
                hops: self.grant.browse_hops,
                objective: self.objective.clone(),
                output: self.output.clone(),
                limits: WorkExecutionLimits {
                    max_workers: 1,
                    ..self.remaining()
                },
            };
            let outcome = browser(self.probe.clone(), request).await;
            results.push(Fetched::Browse(id, outcome));
        }
        let mut terminal = None;
        for fetched in results {
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
                            )
                            .await?;
                            terminal = Some(WorkAttemptStatus::OutcomeUnknown);
                        }
                        (status, _) => {
                            self.settle(
                                id,
                                step_status(status),
                                outcome.usage.or(Some(WorkUsage::default())),
                                vec![],
                                None,
                                None,
                            )
                            .await?;
                        }
                    }
                }
                Fetched::Browse(id, outcome) => match outcome {
                    Ok(outcome) => {
                        if let Some(usage) = outcome.usage {
                            self.charge(usage);
                        }
                        let usage = if outcome.status == WorkStepStatus::OutcomeUnknown {
                            None
                        } else {
                            outcome.usage.or(Some(WorkUsage::default()))
                        };
                        let artifacts: Vec<WorkArtifactV1> = outcome
                            .artifacts
                            .into_iter()
                            .filter_map(|draft| attempt.mint_artifact(draft).ok())
                            .collect();
                        let mut previews = Vec::new();
                        for link in artifacts.iter().flat_map(|a| &a.evidence).take(8) {
                            if let Ok(preview) = self.probe.read_evidence(link.clone()).await {
                                previews.push(preview);
                            }
                        }
                        let (status, artifacts) = if outcome.status == WorkStepStatus::Succeeded {
                            (WorkStepStatus::Succeeded, artifacts)
                        } else {
                            (outcome.status, vec![])
                        };
                        let note =
                            (status == WorkStepStatus::Succeeded).then(|| read_note(&artifacts));
                        self.settle(id, status, usage, artifacts, None, note)
                            .await?;
                        for preview in previews {
                            self.keep_preview(preview);
                        }
                        if outcome.status == WorkStepStatus::OutcomeUnknown {
                            terminal = Some(WorkAttemptStatus::OutcomeUnknown);
                        }
                        if let Some(intervention) = outcome.intervention {
                            self.intervention = Some(intervention);
                            terminal = terminal.or(Some(WorkAttemptStatus::Failed));
                        }
                    }
                    Err(WorkError::OutcomeUnknown) => {
                        self.settle(id, WorkStepStatus::OutcomeUnknown, None, vec![], None, None)
                            .await?;
                        terminal = Some(WorkAttemptStatus::OutcomeUnknown);
                    }
                    Err(_) => {
                        self.settle(
                            id,
                            WorkStepStatus::Failed,
                            Some(WorkUsage::default()),
                            vec![],
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
            if self.cancelled().await {
                let status = if Instant::now() >= self.probe.deadline() {
                    WorkStepStatus::Failed
                } else {
                    WorkStepStatus::Cancelled
                };
                self.settle(id, status, None, vec![], None, None).await?;
                return Ok(Some(if status == WorkStepStatus::Failed {
                    WorkAttemptStatus::Failed
                } else {
                    WorkAttemptStatus::Cancelled
                }));
            }
        }
    }

    fn remember(&mut self, record: &WorkProviderSearchRecordV1) {
        let mut seen = std::collections::BTreeSet::new();
        for (index, citation) in record.evidence.citations.iter().enumerate() {
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
        if self.previews.iter().any(|p| p.link == preview.link) {
            return;
        }
        if self.previews.len() >= MAX_PREVIEWS {
            self.previews.remove(0);
        }
        self.previews.push(preview);
    }
}

fn step_kind_label(kind: &WorkStepKindV1) -> &'static str {
    match kind {
        WorkStepKindV1::Turn => "turn",
        WorkStepKindV1::Search { .. } => "search",
        WorkStepKindV1::Read { .. } => "read",
        WorkStepKindV1::Discover { .. } => "discover",
        WorkStepKindV1::Publish => "publish",
        WorkStepKindV1::Ask { .. } => "ask",
        WorkStepKindV1::Finish => "finish",
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

#[cfg(test)]
mod tests {
    use super::*;
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
