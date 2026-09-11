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
    basis_revision: WorkRevision,
    progress: Arc<Mutex<Option<zephium_ipc::work::WorkSignalV1>>>,
    handle: crate::Handle,
    profile: ProfileId,
    work: WorkId,
    execution: WorkExecutionId,
    attempt: WorkAttemptId,
    node: WorkPlanNode,
    dependencies: Vec<WorkArtifactV1>,
    spec: WorkNodeExecutionSpec,
    deadline: Instant,
    settled: bool,
    _permit: Permit,
}
impl WorkNodeAttempt {
    /// Pure observation; retains neither the attempt nor a browser/resource.
    pub fn observer(&self) -> WorkAttemptObserver {
        WorkAttemptObserver(self.progress.clone())
    }
    pub fn record_activity(&self, activity: zephium_ipc::work::WorkActivityV1) {
        if let Ok(mut signal) = self.progress.lock() {
            *signal = Some(zephium_ipc::work::WorkSignalV1 {
                version: 1,
                profile: self.profile.to_string(),
                work: self.work,
                basis_revision: self.basis_revision,
                execution: self.execution,
                node: self.node.id,
                attempt: self.attempt,
                activity,
            });
        }
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

    /// Authoritative cancellation comes from durable intent. Delayed transient
    /// signals cannot cancel or restart an attempt. Owner changes fail closed.
    pub async fn cancellation_requested(&self) -> Result<bool, WorkError> {
        let state = read(&self.handle, self.profile, self.work).await?;
        let execution = state
            .executions
            .iter()
            .find(|e| e.id == self.execution)
            .ok_or(WorkError::NotFound)?;
        Ok(state.interrupted.contains(&self.execution)
            || execution.status == WorkExecutionStatus::CancelRequested
            || execution.status.terminal())
    }

    /// Called by a trusted typed adapter only after its original worker and
    /// required resource ownership have settled. Unknown outcomes have no
    /// apparent zero-cost refund. User IPC cannot call this operation.
    pub async fn settle(
        mut self,
        result: WorkAdapterResult,
    ) -> Result<WorkRuntimeProjection, WorkError> {
        if result.status == WorkAttemptStatus::Running {
            return Err(WorkError::Invalid);
        }
        let artifacts = result
            .artifacts
            .into_iter()
            .map(|draft| {
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
            })
            .collect::<Result<Vec<_>, WorkError>>()?;
        // Retry only persistence after a definite CAS conflict. Never repeat
        // adapter execution, change the attempt, or retry an uncertain write.
        for _ in 0..4 {
            let state = read(&self.handle, self.profile, self.work).await?;
            let update = WorkRuntimeUpdate::Settle {
                execution: self.execution,
                attempt: self.attempt,
                status: result.status,
                usage: result.usage,
                artifacts: artifacts.clone(),
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
                    return Ok(*state);
                }
                Err(WorkError::Conflict) => continue,
                Err(error) => return Err(error),
                Ok(_) => return Err(WorkError::Invalid),
            }
        }
        Err(WorkError::Conflict)
    }
}
#[derive(Clone)]
pub struct WorkAttemptObserver(Arc<Mutex<Option<zephium_ipc::work::WorkSignalV1>>>);
impl WorkAttemptObserver {
    /// At most one replaceable transient signal; idle observers own no timer.
    pub fn latest(&self) -> Option<zephium_ipc::work::WorkSignalV1> {
        self.0.lock().ok()?.clone()
    }
}
impl Drop for WorkNodeAttempt {
    fn drop(&mut self) {
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
                update: WorkRuntimeUpdate::Begin {
                    execution,
                    attempt,
                    node,
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
        let mut dependencies = Vec::new();
        let mut dependency_bytes = 0;
        for artifact in &execution_fact.artifacts {
            if node_plan.dependencies.contains(&artifact.node) {
                dependency_bytes += serde_json::to_vec(artifact)
                    .map_err(|_| WorkError::Invalid)?
                    .len();
                if dependency_bytes > 6 * 1024 {
                    return Err(WorkError::Capacity);
                }
                dependencies.push(artifact.clone());
            }
        }
        // Subtract queue/Store time conservatively by anchoring at submission;
        // neither callback latency nor a new worker renews the deadline.
        let deadline = submitted
            + Duration::from_millis(u64::from(remaining_millis))
                .min(Duration::from_secs(u64::from(spec.limits.timeout_seconds)));
        let owned = WorkNodeAttempt {
            basis_revision: projection.work.revision,
            progress: Arc::new(Mutex::new(None)),
            handle: self.handle.clone(),
            profile,
            work,
            execution,
            attempt,
            node: node_plan,
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
