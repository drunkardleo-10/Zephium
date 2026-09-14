//! On-demand product execution over the same original attempts used by native
//! qualification. The driver is dormant until polled and never widens a plan.
use crate::work_runtime::{
    WorkAttemptObserver, WorkNodeAttempt, WorkNodeSettlement, WorkRuntimeService,
};
use std::{collections::BTreeSet, future::Future, time::Duration};
use zephium_core::{
    ids::ProfileId,
    work::{port::WorkReply, runtime::*, synthesis::WorkSynthesisProvider, *},
};

pub struct WorkExecutionRequest {
    pub profile: ProfileId,
    pub work: WorkId,
    pub expected_revision: WorkRevision,
    pub execution: WorkExecutionId,
}

pub struct WorkExecutionService {
    handle: crate::Handle,
}
impl WorkExecutionService {
    /// Explicit public-read submission. Only this original fresh Store callback
    /// proceeds to Begin; receipt replay and read projections never dispatch.
    pub async fn read_public<F, Fut, O>(
        &self,
        profile: ProfileId,
        command: zephium_ipc::work::WorkCommandV1,
        execute: F,
        mut observe: O,
    ) -> Result<WorkRuntimeProjection, WorkError>
    where
        F: FnOnce(WorkNodeAttempt) -> Fut,
        Fut: Future<Output = Result<WorkNodeSettlement, WorkError>>,
        O: FnMut(WorkAttemptObserver),
    {
        if command.version != 1 || !matches!(command.intent, WorkRuntimeIntent::ReadPublic { .. }) {
            return Err(WorkError::Invalid);
        }
        let work = command.work;
        let command_id = command.command;
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            self.handle.work_command(profile, command)?,
        )
        .await
        .map_err(|_| WorkError::OutcomeUnknown)??;
        if response.profile != profile {
            return Err(WorkError::ProfileUnavailable);
        }
        let WorkReply::PublicReadAdmitted {
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
        if execution.authorization != WorkExecutionAuthorization::UserDirectedPublicRead
            || execution.spec.nodes.len() != 1
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
        let original = attempt.attempt();
        observe(attempt.observer());
        let settlement = execute(attempt).await?;
        if settlement.profile() != profile
            || settlement.work() != work
            || settlement.execution() != receipt.execution
            || settlement.node() != node
            || settlement.attempt() != original
        {
            return Err(WorkError::Invalid);
        }
        Ok(settlement.into_projection())
    }

    pub async fn prepare_public_approval(
        &self,
        profile: ProfileId,
        request: zephium_ipc::work::WorkApprovalRequestV1,
    ) -> Result<zephium_ipc::work::WorkResponseV1, WorkError> {
        if request.version != 1 {
            return Err(WorkError::Invalid);
        }
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            self.handle.work_projection(profile, request.work)?,
        )
        .await
        .map_err(|_| WorkError::Unavailable)??;
        let WorkReply::Runtime(state) = response.reply else {
            return Err(WorkError::Invalid);
        };
        if response.profile != profile
            || state.work.revision != request.expected_revision
            || state.work.lifecycle != WorkLifecycle::Active
            || state.work.status != WorkAuthoringStatus::PlanReady
            || state.executions.iter().any(|e| !e.status.terminal())
        {
            return Err(WorkError::Conflict);
        }
        let spec = WorkExecutionSpec::public_research(
            state.work.plan.as_ref().ok_or(WorkError::Invalid)?,
            request.limits,
            request.scope,
            request.primary,
        )?;
        let decisions = crate::work_runtime::decision_context(&state.work);
        for node in &state
            .work
            .plan
            .as_ref()
            .ok_or(WorkError::Invalid)?
            .draft
            .nodes
        {
            crate::work_runtime::assignment_objective(&node.objective, &decisions)?;
        }
        supported_primary(&spec)?;
        Ok(zephium_ipc::work::WorkResponseV1 {
            version: 1,
            profile: profile.to_string(),
            reply: zephium_ipc::work::WorkReplyV1::ApprovalDraft {
                work: request.work,
                expected_revision: request.expected_revision,
                spec,
            },
        })
    }

    pub async fn execute_request<F, Fut, O>(
        &self,
        profile: ProfileId,
        request: zephium_ipc::work::WorkStartRequestV1,
        provider: &dyn WorkSynthesisProvider,
        browser: F,
        observe: O,
    ) -> Result<WorkRuntimeProjection, WorkError>
    where
        F: FnMut(WorkNodeAttempt) -> Fut,
        Fut: Future<Output = Result<WorkNodeSettlement, WorkError>>,
        O: FnMut(WorkAttemptObserver),
    {
        if request.version != 1 {
            return Err(WorkError::Invalid);
        }
        self.execute(
            WorkExecutionRequest {
                profile,
                work: request.work,
                expected_revision: request.expected_revision,
                execution: request.execution,
            },
            provider,
            browser,
            observe,
        )
        .await
    }
    pub fn new(handle: crate::Handle) -> Self {
        Self { handle }
    }

    /// The host supplies the native adapter and configured provider. The UI
    /// supplies only the displayed Work/execution/revision. Unsupported topology
    /// is rejected before any attempt or model is started. Lost observers never
    /// recreate attempts: callers reconcile the durable projection.
    pub async fn execute<F, Fut, O>(
        &self,
        request: WorkExecutionRequest,
        provider: &dyn WorkSynthesisProvider,
        mut browser: F,
        mut observe: O,
    ) -> Result<WorkRuntimeProjection, WorkError>
    where
        F: FnMut(WorkNodeAttempt) -> Fut,
        Fut: Future<Output = Result<WorkNodeSettlement, WorkError>>,
        O: FnMut(WorkAttemptObserver),
    {
        let WorkExecutionRequest {
            profile,
            work,
            expected_revision,
            execution,
        } = request;
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            self.handle.work_projection(profile, work)?,
        )
        .await
        .map_err(|_| WorkError::Unavailable)??;
        let WorkReply::Runtime(state) = response.reply else {
            return Err(WorkError::Invalid);
        };
        if response.profile != profile
            || state.work.revision != expected_revision
            || state.interrupted.contains(&execution)
        {
            return Err(WorkError::Conflict);
        }
        let fact = state
            .executions
            .iter()
            .find(|e| e.id == execution)
            .ok_or(WorkError::NotFound)?;
        if fact.status != WorkExecutionStatus::Approved
            || fact.authorization == WorkExecutionAuthorization::UserDirectedPublicRead
        {
            return Err(WorkError::Conflict);
        }
        let plan = state.work.plan.as_ref().ok_or(WorkError::Invalid)?.clone();
        let spec = fact.spec.clone();
        spec.validate(&plan)?;
        let primary = supported_primary(&spec)?;
        let runtime = WorkRuntimeService::new(self.handle.clone());
        if let Some(primary) = primary {
            let attempt = runtime
                .begin_node(profile, work, expected_revision, execution, primary)
                .await?;
            attempt.record_activity(zephium_ipc::work::WorkActivityV1::Delegating);
            observe(attempt.observer());
            let mut coordinator = attempt.coordinate().await?;
            let mut completed = BTreeSet::new();
            while completed.len() + 1 < plan.draft.nodes.len() {
                let node = plan
                    .draft
                    .nodes
                    .iter()
                    .find(|n| {
                        n.id != primary
                            && !completed.contains(&n.id)
                            && n.dependencies.iter().all(|d| completed.contains(d))
                    })
                    .ok_or(WorkError::Invalid)?
                    .id;
                let result = coordinator
                    .execute_child(node, |attempt| {
                        observe(attempt.observer());
                        execute_node(attempt, provider, &mut browser)
                    })
                    .await;
                if result.is_err() {
                    // Finish the original poisoned primary without another
                    // model call; the child keeps its own failure/unknown facts.
                    return coordinator
                        .finish(provider)
                        .await
                        .map(WorkNodeSettlement::into_projection);
                }
                completed.insert(node);
            }
            return coordinator
                .finish(provider)
                .await
                .map(WorkNodeSettlement::into_projection);
        }
        let mut state = *state;
        let mut completed = BTreeSet::new();
        while completed.len() < plan.draft.nodes.len() {
            let node = plan
                .draft
                .nodes
                .iter()
                .find(|n| {
                    !completed.contains(&n.id)
                        && n.dependencies.iter().all(|d| completed.contains(d))
                })
                .ok_or(WorkError::Invalid)?
                .id;
            let attempt = runtime
                .begin_node(profile, work, state.work.revision, execution, node)
                .await?;
            observe(attempt.observer());
            let original_attempt = attempt.attempt();
            let settlement = execute_node(attempt, provider, &mut browser).await?;
            if settlement.profile() != profile
                || settlement.work() != work
                || settlement.execution() != execution
                || settlement.node() != node
                || settlement.attempt() != original_attempt
            {
                return Err(WorkError::Invalid);
            }
            let id = settlement.attempt();
            state = settlement.into_projection();
            if !state
                .executions
                .iter()
                .flat_map(|e| &e.attempts)
                .any(|a| a.id == id && a.status == WorkAttemptStatus::Succeeded)
            {
                return Ok(state);
            }
            completed.insert(node);
        }
        Ok(state)
    }
}

fn supported_primary(spec: &WorkExecutionSpec) -> Result<Option<WorkPlanNodeId>, WorkError> {
    let roots: Vec<_> = spec
        .nodes
        .iter()
        .filter(|n| {
            matches!(
                n.capability,
                WorkCapability::Coordinate { .. }
                    | WorkCapability::CoordinatePublicDiscovery { .. }
                    | WorkCapability::CoordinatePublicResearch { .. }
            )
        })
        .collect();
    match roots.as_slice() {
        [] if spec.nodes.iter().all(|n| n.parent.is_none()) => Ok(None),
        [primary]
            if primary.parent.is_none()
                && spec
                    .nodes
                    .iter()
                    .all(|n| n.node == primary.node || n.parent == Some(primary.node))
                && (spec.nodes.len() == 1 || spec.limits.max_workers >= 2) =>
        {
            Ok(Some(primary.node))
        }
        _ => Err(WorkError::Invalid),
    }
}

async fn execute_node<F, Fut>(
    attempt: WorkNodeAttempt,
    provider: &dyn WorkSynthesisProvider,
    browser: &mut F,
) -> Result<WorkNodeSettlement, WorkError>
where
    F: FnMut(WorkNodeAttempt) -> Fut,
    Fut: Future<Output = Result<WorkNodeSettlement, WorkError>>,
{
    match attempt.specification().capability {
        WorkCapability::PublicBrowse { .. }
        | WorkCapability::PublicDiscovery { .. }
        | WorkCapability::PublicSearch { .. } => browser(attempt).await,
        WorkCapability::Synthesize => attempt.synthesize_owned(provider).await,
        WorkCapability::Coordinate { .. }
        | WorkCapability::CoordinatePublicDiscovery { .. }
        | WorkCapability::CoordinatePublicResearch { .. } => Err(WorkError::Invalid),
    }
}
