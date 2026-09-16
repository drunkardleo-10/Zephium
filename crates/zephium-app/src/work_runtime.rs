//! Product-owned dispatch and exact settlement over the ordinary Shell/Store.
//! Construction is dormant. Deserialized facts can never construct an attempt.
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use zephium_core::{
    ids::ProfileId,
    work::{artifact::*, port::*, runtime::*, *},
};

static ACTIVE: Mutex<Vec<(ProfileId, WorkId, WorkPlanNodeId)>> = Mutex::new(Vec::new());
struct Permit((ProfileId, WorkId, WorkPlanNodeId));
impl Permit {
    fn acquire(key: (ProfileId, WorkId, WorkPlanNodeId)) -> Result<Self, WorkError> {
        let mut active = ACTIVE.lock().map_err(|_| WorkError::Unavailable)?;
        if active.len() >= 4 || active.contains(&key) {
            return Err(WorkError::Capacity);
        }
        active.push(key);
        Ok(Self(key))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock() {
            active.retain(|key| *key != self.0);
        }
    }
}

/// Compact responsibility, exact approved scope and completion contract. This
/// is deliberately move-only and not serializable. Executors disclose only the
/// selected textual fields through their own provider policy boundary.
#[must_use]
pub struct WorkNodeAttempt {
    browser_session: zephium_agentic::WorkBrowserSession,
    basis_revision: Arc<Mutex<WorkRevision>>,
    owner: WorkRuntimeSessionId,
    progress: Arc<Mutex<Option<zephium_ipc::work::WorkSignalV1>>>,
    handle: crate::Handle,
    profile: ProfileId,
    work: WorkId,
    execution: WorkExecutionId,
    attempt: WorkAttemptId,
    node: WorkPlanNode,
    decisions: Vec<zephium_core::work::planning::PlanningAnswer>,
    dependencies: Vec<WorkArtifactV1>,
    spec: WorkNodeExecutionSpec,
    deadline: Instant,
    settled: bool,
    _permit: Permit,
}
impl WorkNodeAttempt {
    pub(crate) async fn runtime_projection(&self) -> Result<WorkRuntimeProjection, WorkError> {
        read(&self.handle, self.profile, self.work).await
    }

    pub(crate) async fn begin_child(
        &self,
        node: WorkPlanNodeId,
    ) -> Result<WorkNodeAttempt, WorkError> {
        if self.cancellation_requested().await? || Instant::now() >= self.deadline {
            return Err(WorkError::Unavailable);
        }
        let state = self.runtime_projection().await?;
        WorkRuntimeService::new(self.handle.clone())
            .begin(
                self.profile,
                self.work,
                state.work.revision,
                self.execution,
                node,
                Some((self.attempt, self.deadline)),
            )
            .await
    }

    pub(crate) fn retain_child_artifacts(
        &mut self,
        artifacts: Vec<WorkArtifactV1>,
    ) -> Result<(), WorkError> {
        let mut combined = self.dependencies.clone();
        for artifact in artifacts {
            if self.node.dependencies.contains(&artifact.node) {
                if combined.iter().any(|a| a.id == artifact.id) {
                    return Err(WorkError::Invalid);
                }
                combined.push(artifact);
            }
        }
        self.dependencies = compact_dependency_artifacts(combined)?;
        Ok(())
    }
    /// Pure observation; retains neither the attempt nor a browser/resource.
    pub fn observer(&self) -> WorkAttemptObserver {
        WorkAttemptObserver {
            attempt: self.attempt,
            progress: Arc::downgrade(&self.progress),
        }
    }
    pub fn record_activity(&self, activity: zephium_ipc::work::WorkActivityV1) {
        self.probe().record_activity(activity);
    }
    /// A cloneable observation of this live attempt for step runners: it can
    /// read cancellation, report activity and commit steps, never settle or
    /// restart the attempt.
    pub fn probe(&self) -> WorkAttemptProbe {
        WorkAttemptProbe {
            browser_session: self.browser_session.clone(),
            basis_revision: self.basis_revision.clone(),
            owner: self.owner,
            progress: self.progress.clone(),
            handle: self.handle.clone(),
            profile: self.profile,
            work: self.work,
            execution: self.execution,
            attempt: self.attempt,
            node: self.node.id,
            deadline: self.deadline,
        }
    }
    pub(crate) fn mint_artifact(
        &self,
        draft: WorkArtifactDraft,
    ) -> Result<WorkArtifactV1, WorkError> {
        let expected = self
            .node
            .outputs
            .iter()
            .find(|o| o.name == draft.output)
            .ok_or(WorkError::Invalid)?;
        let artifact = WorkArtifactV1 {
            version: 1,
            id: WorkArtifactId::generate(),
            execution: self.execution,
            node: self.node.id,
            attempt: self.attempt,
            output: draft.output,
            title: draft.title,
            data: draft.data,
            evidence: draft.evidence,
            review: expected.review,
            presentation: WorkArtifactPresentationV1::Automatic,
        };
        artifact.validate()?;
        Ok(artifact)
    }
    pub fn profile(&self) -> ProfileId {
        self.profile
    }
    pub fn work(&self) -> WorkId {
        self.work
    }
    pub fn execution(&self) -> WorkExecutionId {
        self.execution
    }
    pub fn attempt(&self) -> WorkAttemptId {
        self.attempt
    }
    pub fn node(&self) -> &WorkPlanNode {
        &self.node
    }
    pub fn decisions(&self) -> &[zephium_core::work::planning::PlanningAnswer] {
        &self.decisions
    }
    pub fn disclosure_objective(&self) -> Result<String, WorkError> {
        assignment_objective(&self.node.objective, &self.decisions)
    }
    /// Only successful outputs of this node's approved direct dependencies,
    /// from this exact execution. No sibling graph or provider transcript.
    pub fn dependency_artifacts(&self) -> &[WorkArtifactV1] {
        &self.dependencies
    }
    pub fn specification(&self) -> &WorkNodeExecutionSpec {
        &self.spec
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub(crate) async fn read_evidence(
        &self,
        link: WorkEvidenceLink,
    ) -> Result<WorkEvidencePreviewV1, WorkError> {
        match request(
            &self.handle,
            self.profile,
            WorkRequest::ReadEvidence {
                id: self.work,
                link,
            },
        )
        .await?
        {
            WorkReply::Evidence(preview) => Ok(preview),
            _ => Err(WorkError::Invalid),
        }
    }

    /// Authoritative cancellation comes from durable intent. Delayed transient
    /// signals cannot cancel or restart an attempt. Owner changes fail closed.
    pub async fn cancellation_requested(&self) -> Result<bool, WorkError> {
        self.probe().cancellation_requested().await
    }

    /// Called by a trusted typed adapter only after its original worker and
    /// required resource ownership have settled. Unknown outcomes have no
    /// apparent zero-cost refund. User IPC cannot call this operation.
    pub async fn settle(
        self,
        result: WorkAdapterResult,
    ) -> Result<WorkRuntimeProjection, WorkError> {
        self.settle_owned(result)
            .await
            .map(WorkNodeSettlement::into_projection)
    }

    /// Preserve the original acknowledged publication for an owning parent.
    /// Reads and decoded projections cannot construct this move-only receipt.
    pub async fn settle_owned(
        self,
        result: WorkAdapterResult,
    ) -> Result<WorkNodeSettlement, WorkError> {
        self.settle_with_provider_evidence(result, None).await
    }

    pub(crate) async fn settle_with_provider_evidence(
        mut self,
        result: WorkAdapterResult,
        mut provider_evidence: Option<WorkProviderSearchRecordV1>,
    ) -> Result<WorkNodeSettlement, WorkError> {
        if result.status == WorkAttemptStatus::Running {
            return Err(WorkError::Invalid);
        }
        let mut status = result.status;
        let mut artifacts = result
            .artifacts
            .into_iter()
            .map(|draft| self.mint_artifact(draft))
            .collect::<Result<Vec<_>, WorkError>>()?;
        // Retry only persistence after a definite CAS conflict. Never repeat
        // adapter execution, change the attempt, or retry an uncertain write.
        for _ in 0..4 {
            let state = read(&self.handle, self.profile, self.work).await?;
            let update = if let Some(evidence) = &provider_evidence {
                WorkRuntimeUpdate::SettleProviderSearch {
                    execution: self.execution,
                    attempt: self.attempt,
                    status,
                    usage: result.usage,
                    artifacts: artifacts.clone(),
                    evidence: Box::new(evidence.clone()),
                }
            } else {
                WorkRuntimeUpdate::Settle {
                    execution: self.execution,
                    attempt: self.attempt,
                    status,
                    usage: result.usage,
                    artifacts: artifacts.clone(),
                    intervention: result.intervention.clone(),
                }
            };
            match request(
                &self.handle,
                self.profile,
                WorkRequest::RuntimeUpdate {
                    id: self.work,
                    expected: state.work.revision,
                    update,
                },
            )
            .await
            {
                Ok(WorkReply::Runtime(state)) => {
                    self.settled = true;
                    return Ok(WorkNodeSettlement {
                        profile: self.profile,
                        work: self.work,
                        execution: self.execution,
                        attempt: self.attempt,
                        node: self.node.id,
                        projection: *state,
                    });
                }
                Err(WorkError::Conflict) => continue,
                // A definite capacity refusal rolled back publication. Preserve
                // the original worker's known usage with a small failed fact;
                // never repeat provider work or treat an uncertain write this way.
                Err(WorkError::Capacity)
                    if status == WorkAttemptStatus::Succeeded && result.usage.is_some() =>
                {
                    status = WorkAttemptStatus::Failed;
                    artifacts.clear();
                    provider_evidence = None;
                }
                Err(error) => return Err(error),
                Ok(_) => return Err(WorkError::Invalid),
            }
        }
        Err(WorkError::Conflict)
    }
}

/// Fresh result of the original attempt's Store settlement callback. This is
/// not an execution capability, and serialization of its projection loses the
/// publication ownership required by the orchestration handoff.
#[must_use]
pub struct WorkNodeSettlement {
    profile: ProfileId,
    work: WorkId,
    execution: WorkExecutionId,
    attempt: WorkAttemptId,
    node: WorkPlanNodeId,
    projection: WorkRuntimeProjection,
}
impl WorkNodeSettlement {
    pub fn profile(&self) -> ProfileId {
        self.profile
    }
    pub fn work(&self) -> WorkId {
        self.work
    }
    pub fn execution(&self) -> WorkExecutionId {
        self.execution
    }
    pub fn attempt(&self) -> WorkAttemptId {
        self.attempt
    }
    pub fn node(&self) -> WorkPlanNodeId {
        self.node
    }
    pub fn projection(&self) -> &WorkRuntimeProjection {
        &self.projection
    }
    pub fn into_projection(self) -> WorkRuntimeProjection {
        self.projection
    }
}
#[derive(Clone)]
pub struct WorkAttemptObserver {
    attempt: WorkAttemptId,
    progress: std::sync::Weak<Mutex<Option<zephium_ipc::work::WorkSignalV1>>>,
}
impl WorkAttemptObserver {
    pub fn attempt(&self) -> WorkAttemptId {
        self.attempt
    }
    pub fn is_alive(&self) -> bool {
        self.progress.strong_count() > 0
    }
    /// At most one replaceable transient signal; idle observers own no timer.
    pub fn latest(&self) -> Option<zephium_ipc::work::WorkSignalV1> {
        self.progress.upgrade()?.lock().ok()?.clone()
    }
}
#[derive(Clone)]
pub struct WorkAttemptProbe {
    browser_session: zephium_agentic::WorkBrowserSession,
    basis_revision: Arc<Mutex<WorkRevision>>,
    owner: WorkRuntimeSessionId,
    progress: Arc<Mutex<Option<zephium_ipc::work::WorkSignalV1>>>,
    handle: crate::Handle,
    profile: ProfileId,
    work: WorkId,
    execution: WorkExecutionId,
    attempt: WorkAttemptId,
    node: WorkPlanNodeId,
    deadline: Instant,
}
impl WorkAttemptProbe {
    pub fn browser_session(&self) -> &zephium_agentic::WorkBrowserSession {
        &self.browser_session
    }
    pub fn profile(&self) -> ProfileId {
        self.profile
    }
    pub fn work(&self) -> WorkId {
        self.work
    }
    pub fn execution(&self) -> WorkExecutionId {
        self.execution
    }
    pub fn attempt(&self) -> WorkAttemptId {
        self.attempt
    }
    pub fn node(&self) -> WorkPlanNodeId {
        self.node
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn record_activity(&self, activity: zephium_ipc::work::WorkActivityV1) {
        if let Ok(mut signal) = self.progress.lock() {
            *signal = Some(zephium_ipc::work::WorkSignalV1 {
                version: 1,
                owner: self.owner,
                profile: self.profile.to_string(),
                work: self.work,
                basis_revision: *self
                    .basis_revision
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()),
                execution: self.execution,
                node: self.node,
                attempt: self.attempt,
                activity,
            });
        }
    }
    pub async fn runtime_projection(&self) -> Result<WorkRuntimeProjection, WorkError> {
        read(&self.handle, self.profile, self.work).await
    }
    pub async fn cancellation_requested(&self) -> Result<bool, WorkError> {
        let state = read(&self.handle, self.profile, self.work).await?;
        let execution = state
            .executions
            .iter()
            .find(|e| e.id == self.execution)
            .ok_or(WorkError::NotFound)?;
        let owned = state
            .owners
            .iter()
            .any(|entry| entry.execution == self.execution && entry.owner == self.owner)
            && execution.attempts.iter().any(|attempt| {
                attempt.id == self.attempt
                    && attempt.node == self.node
                    && attempt.status == WorkAttemptStatus::Running
            });
        if owned && !state.interrupted.contains(&self.execution) {
            // Only the original attempt advances its observation basis after a
            // fresh durable read. Historical projections never mint an observer.
            if let Ok(mut basis) = self.basis_revision.lock() {
                *basis = state.work.revision;
            }
            if let Ok(mut signal) = self.progress.lock() {
                if let Some(signal) = signal.as_mut() {
                    signal.basis_revision = state.work.revision;
                }
            }
        }
        let cancelled = !owned
            || state.interrupted.contains(&self.execution)
            || execution.status == WorkExecutionStatus::CancelRequested
            || execution.status.terminal()
            || execution.attempts.iter().any(|attempt| {
                !matches!(
                    attempt.status,
                    WorkAttemptStatus::Running | WorkAttemptStatus::Succeeded
                )
            });
        if cancelled {
            self.browser_session.close();
        }
        Ok(cancelled)
    }
    pub(crate) async fn read_evidence(
        &self,
        link: WorkEvidenceLink,
    ) -> Result<WorkEvidencePreviewV1, WorkError> {
        match request(
            &self.handle,
            self.profile,
            WorkRequest::ReadEvidence {
                id: self.work,
                link,
            },
        )
        .await?
        {
            WorkReply::Evidence(preview) => Ok(preview),
            _ => Err(WorkError::Invalid),
        }
    }
    /// Commit one step under the live attempt. Persistence retries only a
    /// definite CAS conflict; a capacity refusal downgrades a successful step
    /// to a failed one without its payload rather than losing the record.
    pub(crate) async fn commit_step(
        &self,
        mut update: WorkRuntimeUpdate,
    ) -> Result<WorkRuntimeProjection, WorkError> {
        for _ in 0..4 {
            let state = read(&self.handle, self.profile, self.work).await?;
            match request(
                &self.handle,
                self.profile,
                WorkRequest::RuntimeUpdate {
                    id: self.work,
                    expected: state.work.revision,
                    update: update.clone(),
                },
            )
            .await
            {
                Ok(WorkReply::Runtime(state)) => return Ok(*state),
                Err(WorkError::Conflict) => continue,
                Err(WorkError::Capacity) => match &mut update {
                    WorkRuntimeUpdate::SettleStep {
                        status,
                        artifacts,
                        evidence,
                        ..
                    } if *status == WorkStepStatus::Succeeded => {
                        *status = WorkStepStatus::Failed;
                        artifacts.clear();
                        *evidence = None;
                    }
                    WorkRuntimeUpdate::BeginStep {
                        step,
                        artifacts,
                        evidence,
                        ..
                    } if !artifacts.is_empty() => {
                        step.status = WorkStepStatus::Failed;
                        step.artifacts.clear();
                        step.evidence = None;
                        artifacts.clear();
                        *evidence = None;
                    }
                    _ => return Err(WorkError::Capacity),
                },
                Err(error) => return Err(error),
                Ok(_) => return Err(WorkError::Invalid),
            }
        }
        Err(WorkError::Conflict)
    }
}
impl Drop for WorkNodeAttempt {
    fn drop(&mut self) {
        self.browser_session.close();
        if let Ok(mut progress) = self.progress.lock() {
            progress.take();
        }
        if !self.settled {
            // No detached future/thread. If admission itself fails, the retained
            // running fact still requires recovery; no success is manufactured.
            let _ = self.handle.submit_owned_work_runtime(
                WorkRequest::RuntimeAbandon {
                    id: self.work,
                    execution: self.execution,
                    attempt: self.attempt,
                },
                self.profile,
            );
        }
    }
}

/// Adapter result data. Evidence links must be obtained from the adapter's
/// original durable source publication. They must not be invented by a model.
pub struct WorkArtifactDraft {
    pub output: String,
    pub title: String,
    pub data: WorkArtifactDataV1,
    pub evidence: Vec<WorkEvidenceLink>,
}
pub struct WorkAdapterResult {
    pub status: WorkAttemptStatus,
    pub usage: Option<WorkUsage>,
    pub artifacts: Vec<WorkArtifactDraft>,
    /// Closed reason automation stopped for a person; never page text.
    pub intervention: Option<WorkInterventionV1>,
}

pub struct WorkRuntimeService {
    handle: crate::Handle,
}
struct PendingBegin<'a> {
    handle: &'a crate::Handle,
    profile: ProfileId,
    work: WorkId,
    execution: WorkExecutionId,
    attempt: WorkAttemptId,
    armed: bool,
}
impl Drop for PendingBegin<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.handle.submit_owned_work_runtime(
                WorkRequest::RuntimeAbandon {
                    id: self.work,
                    execution: self.execution,
                    attempt: self.attempt,
                },
                self.profile,
            );
        }
    }
}
impl WorkRuntimeService {
    pub fn new(handle: crate::Handle) -> Self {
        Self { handle }
    }
    /// The user has already approved the exact plan and concrete scopes. This
    /// is a fresh dispatch, not resumption by deserialization. Store independently
    /// checks its incarnation, deadline, dependency readiness and reservation.
    pub async fn begin_node(
        &self,
        profile: ProfileId,
        work: WorkId,
        expected: WorkRevision,
        execution: WorkExecutionId,
        node: WorkPlanNodeId,
    ) -> Result<WorkNodeAttempt, WorkError> {
        self.begin(profile, work, expected, execution, node, None)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn begin(
        &self,
        profile: ProfileId,
        work: WorkId,
        expected: WorkRevision,
        execution: WorkExecutionId,
        node: WorkPlanNodeId,
        parent: Option<(WorkAttemptId, Instant)>,
    ) -> Result<WorkNodeAttempt, WorkError> {
        let permit = Permit::acquire((profile, work, node))?;
        let attempt = WorkAttemptId::generate();
        let mut pending = PendingBegin {
            handle: &self.handle,
            profile,
            work,
            execution,
            attempt,
            armed: true,
        };
        let submitted = Instant::now();
        let reply = request(
            &self.handle,
            profile,
            WorkRequest::RuntimeUpdate {
                id: work,
                expected,
                update: match parent {
                    Some((parent, _)) => WorkRuntimeUpdate::BeginChild {
                        execution,
                        attempt,
                        node,
                        parent,
                    },
                    None => WorkRuntimeUpdate::Begin {
                        execution,
                        attempt,
                        node,
                    },
                },
            },
        )
        .await?;
        let WorkReply::RuntimeStarted {
            projection,
            remaining_millis,
        } = reply
        else {
            return Err(WorkError::Invalid);
        };
        let execution_fact = projection
            .executions
            .iter()
            .find(|e| e.id == execution)
            .ok_or(WorkError::Invalid)?;
        let spec = execution_fact
            .spec
            .nodes
            .iter()
            .find(|n| n.node == node)
            .cloned()
            .ok_or(WorkError::Invalid)?;
        let node_plan = projection
            .work
            .plan
            .as_ref()
            .and_then(|p| p.draft.nodes.iter().find(|n| n.id == node))
            .cloned()
            .ok_or(WorkError::Invalid)?;
        let dependencies = compact_dependency_artifacts(
            execution_fact
                .artifacts
                .iter()
                .filter(|artifact| node_plan.dependencies.contains(&artifact.node))
                .cloned()
                .collect(),
        )?;
        // Subtract queue/Store time conservatively by anchoring at submission;
        // neither callback latency nor a new worker renews the deadline.
        let deadline = submitted
            + Duration::from_millis(u64::from(remaining_millis))
                .min(Duration::from_secs(u64::from(spec.limits.timeout_seconds)));
        let deadline = parent.map_or(deadline, |(_, parent_deadline)| {
            deadline.min(parent_deadline)
        });
        let owned = WorkNodeAttempt {
            browser_session: zephium_agentic::WorkBrowserSession::new(profile, work, deadline),
            basis_revision: Arc::new(Mutex::new(projection.work.revision)),
            owner: projection
                .owners
                .iter()
                .find(|entry| entry.execution == execution)
                .ok_or(WorkError::Invalid)?
                .owner,
            progress: Arc::new(Mutex::new(None)),
            handle: self.handle.clone(),
            profile,
            work,
            execution,
            attempt,
            node: node_plan,
            decisions: decision_context(&projection.work),
            dependencies,
            spec,
            deadline,
            settled: false,
            _permit: permit,
        };
        pending.armed = false;
        if Instant::now() >= deadline {
            return Err(WorkError::Unavailable);
        }
        Ok(owned)
    }
}
async fn request(
    handle: &crate::Handle,
    profile: ProfileId,
    request: WorkRequest,
) -> Result<WorkReply, WorkError> {
    let response = tokio::time::timeout(
        Duration::from_secs(10),
        handle.submit_owned_work_runtime(request, profile)?,
    )
    .await
    .map_err(|_| WorkError::OutcomeUnknown)??;
    if response.profile != profile {
        return Err(WorkError::ProfileUnavailable);
    }
    Ok(response.reply)
}
pub(crate) fn decision_context(
    work: &WorkSnapshot,
) -> Vec<zephium_core::work::planning::PlanningAnswer> {
    work.current_questions()
        .filter_map(|q| {
            q.answer
                .as_ref()
                .map(|answer| zephium_core::work::planning::PlanningAnswer {
                    question: q.prompt.clone(),
                    answer: answer.clone(),
                })
        })
        .collect()
}

pub(crate) fn assignment_objective(
    objective: &str,
    decisions: &[zephium_core::work::planning::PlanningAnswer],
) -> Result<String, WorkError> {
    let mut text = objective.to_owned();
    if !decisions.is_empty() {
        text.push_str(
            "\nUser decisions for this Work (task context, not additional permissions):\n",
        );
        text.push_str(&serde_json::to_string(decisions).map_err(|_| WorkError::Invalid)?);
    }
    if text.len() > MAX_WORK_TEXT_BYTES {
        return Err(WorkError::Capacity);
    }
    Ok(text)
}

async fn read(
    handle: &crate::Handle,
    profile: ProfileId,
    work: WorkId,
) -> Result<WorkRuntimeProjection, WorkError> {
    match request(handle, profile, WorkRequest::RuntimeRead { id: work }).await? {
        WorkReply::Runtime(state) => Ok(*state),
        _ => Err(WorkError::Invalid),
    }
}

/// Model-facing dependency projection only. Original Store publications and
/// all identity/evidence joins stay intact; the notice makes lost text explicit.
fn compact_dependency_artifacts(
    original: Vec<WorkArtifactV1>,
) -> Result<Vec<WorkArtifactV1>, WorkError> {
    const LIMIT: usize = 6 * 1024;
    const NOTICE: &str =
        "\n[Host-truncated dependency summary; complete artifact retained in Work.]";
    let fits = |items: &[WorkArtifactV1]| -> Result<bool, WorkError> {
        Ok(serde_json::to_vec(items)
            .map_err(|_| WorkError::Invalid)?
            .len()
            <= LIMIT)
    };
    if fits(&original)? {
        return Ok(original);
    }
    // Include the notice in the sizing projection even for complete summaries:
    // this makes the serialized prefix bound monotonic as allowance grows.
    // The returned projection labels only actual truncation.
    let project = |allowance: usize, sizing: bool| {
        original
            .iter()
            .cloned()
            .map(|mut artifact| {
                if let WorkArtifactDataV1::EvidenceCollection { summary, .. } = &mut artifact.data {
                    let prefix: String = summary.chars().take(allowance).collect();
                    let truncated = prefix.len() < summary.len();
                    *summary = prefix;
                    if truncated || sizing {
                        summary.push_str(NOTICE);
                    }
                }
                artifact
            })
            .collect::<Vec<_>>()
    };
    if !fits(&project(128, true))? {
        return Err(WorkError::Capacity);
    }
    let mut low = 128usize;
    let mut high = LIMIT;
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if fits(&project(middle, true))? {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    Ok(project(low, false))
}
