//! Content-free semantic progress owned by one exact run supervisor.
//!
//! Progress is a current projection, not a transcript. It carries only closed
//! operation/state/result/blocker classes and opaque resource identities. It
//! cannot carry objectives, prompts, model output, page content, origins,
//! selectors, JavaScript, native handles, or error strings.

use super::*;
use crate::{
    AgentActiveEffect, AgentEffectPermit, AgentEffectReceipt, AgentModelCallReceipt,
    AgentModelCallSettlement, AgentNeedsHumanReason, AgentNeedsHumanTransition, ContextId,
    SemanticActionFailure, SemanticEffectClass,
};

/// Closed semantic operation currently owned by one supervisor node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProgressOperation {
    /// Scheduler admission, queueing, yielding, or terminalization.
    Scheduling,
    /// Local planning over already-approved state.
    Planning,
    /// Activation or supervision of one pre-approved child node.
    Delegation,
    /// One bounded model call.
    Model,
    /// One semantic observation or diff capture.
    Observation,
    /// One provenance-preserving semantic read or extraction.
    Read,
    /// One browser-context lifecycle operation.
    Context,
    /// One independently classified semantic effect.
    Effect(SemanticEffectClass),
    /// Independent post-effect verification.
    Verification,
    /// Human review or control at one effect boundary.
    Approval(SemanticEffectClass),
    /// One content-free persistence or audit handoff.
    Persistence,
}

/// Opaque active resource identity attached to semantic progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProgressResource {
    /// Exact executing-agent turn.
    Execution(AgentSupervisorAttemptId),
    /// Exact pre-approved delegated plan node.
    PlanNode(AgentPlanNodeId),
    /// Exact model-call correlation identity.
    ModelCall(crate::AgentModelCallId),
    /// Exact browser-context identity.
    Context(ContextId),
    /// Exact semantic-effect correlation identity.
    Effect(crate::AgentEffectId),
}

/// Closed current state of one semantic operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProgressState {
    /// Responsibility exists but has not acquired an execution slot.
    Queued,
    /// The exact operation is active.
    Active,
    /// The operation released execution or requires an external decision.
    Waiting,
    /// The operation reached a successful terminal result.
    Succeeded,
    /// The operation reached a typed unsuccessful terminal result.
    Failed,
    /// Cancellation won the operation race.
    Cancelled,
}

/// Typed content-free result retained by the current progress projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProgressResult {
    /// Exact scheduler turn result.
    Supervisor(AgentSupervisorExecutionOutcome),
    /// Exact provider-call terminal class.
    Model(AgentModelCallSettlement),
    /// Exact semantic-effect terminal class.
    Effect(crate::AgentEffectSettlement),
    /// Registry-proven browser-context release class.
    Context(AgentSupervisorContextReleaseOutcome),
}

/// Closed blocker for waiting, failed, or cancelled semantic progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProgressBlocker {
    /// Resource-free supervisor wait.
    Scheduler(AgentSupervisorWait),
    /// Deterministic policy requires human review or control.
    NeedsHuman(AgentNeedsHumanReason),
    /// Terminal scheduler failure.
    Supervisor(AgentSupervisorFailure),
    /// Terminal semantic action failure.
    Effect(SemanticActionFailure),
    /// Provider transport or typed response failed.
    Provider,
    /// Cancellation won before a model response completed.
    ModelCancelled,
    /// Exact supervisor cancellation update affecting this responsibility.
    Cancellation(AgentSupervisorCancellationReason),
    /// A queued context was cancelled before native construction.
    ContextCancelled,
}

/// Validated active operation/resource pair supplied by the trusted shell.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentProgressActivity {
    operation: AgentProgressOperation,
    resource: Option<AgentProgressResource>,
}

impl AgentProgressActivity {
    /// Validates the resource class required by one closed operation.
    pub fn try_new(
        operation: AgentProgressOperation,
        resource: Option<AgentProgressResource>,
    ) -> Result<Self, AgentSupervisorRuntimeError> {
        if !activity_resource_matches(operation, resource) {
            return Err(AgentSupervisorRuntimeError::ProgressResourceMismatch);
        }
        Ok(Self {
            operation,
            resource,
        })
    }

    /// Closed semantic operation class.
    pub const fn operation(self) -> AgentProgressOperation {
        self.operation
    }

    /// Exact active resource, when the operation owns one.
    pub const fn resource(self) -> Option<AgentProgressResource> {
        self.resource
    }

    const fn scheduling(resource: Option<AgentProgressResource>) -> Self {
        Self {
            operation: AgentProgressOperation::Scheduling,
            resource,
        }
    }
}

impl fmt::Debug for AgentProgressActivity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProgressActivity")
            .field("operation", &self.operation)
            .field("resource", &self.resource)
            .finish()
    }
}

/// Current semantic progress for one exact plan-node responsibility.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentSemanticProgress {
    manifest: crate::AgentRunManifestId,
    supervisor: AgentSupervisorId,
    responsibility: AgentPlanNodeId,
    activity: AgentProgressActivity,
    state: AgentProgressState,
    result: Option<AgentProgressResult>,
    blocker: Option<AgentProgressBlocker>,
}

impl AgentSemanticProgress {
    /// Exact immutable manifest revision governing this projection.
    pub const fn manifest(self) -> crate::AgentRunManifestId {
        self.manifest
    }

    /// Exact mutable supervisor incarnation.
    pub const fn supervisor(self) -> AgentSupervisorId {
        self.supervisor
    }

    /// Exact plan node responsible for this operation.
    pub const fn responsibility(self) -> AgentPlanNodeId {
        self.responsibility
    }

    /// Closed operation and exact opaque resource.
    pub const fn activity(self) -> AgentProgressActivity {
        self.activity
    }

    /// Current operation state.
    pub const fn state(self) -> AgentProgressState {
        self.state
    }

    /// Typed result when the operation settled or yielded.
    pub const fn result(self) -> Option<AgentProgressResult> {
        self.result
    }

    /// Closed blocker for waiting, failed, or cancelled progress.
    pub const fn blocker(self) -> Option<AgentProgressBlocker> {
        self.blocker
    }
}

impl fmt::Debug for AgentSemanticProgress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentSemanticProgress")
            .field("manifest", &self.manifest)
            .field("supervisor", &self.supervisor)
            .field("responsibility", &self.responsibility)
            .field("activity", &self.activity)
            .field("state", &self.state)
            .field("result", &self.result)
            .field("blocker", &self.blocker)
            .field("content", &"[redacted]")
            .finish()
    }
}

impl AgentRunSupervisor {
    /// Current semantic progress for one activated responsibility.
    pub fn semantic_progress(&self, node: AgentPlanNodeId) -> Option<AgentSemanticProgress> {
        self.node_index(node)
            .map(|index| self.nodes[index].progress)
    }

    /// Records one active closed operation under the exact current execution.
    ///
    /// This projection grants no model, browser, effect, persistence, or
    /// approval authority and retains no free-form content.
    pub fn record_progress_activity(
        &mut self,
        execution: &AgentNodeExecution,
        activity: AgentProgressActivity,
    ) -> Result<AgentSemanticProgress, AgentSupervisorRuntimeError> {
        let index = self.require_running_execution(execution)?;
        let progress = self.progress(
            execution.node(),
            activity,
            AgentProgressState::Active,
            None,
            None,
        );
        self.nodes[index].progress = progress;
        Ok(progress)
    }

    /// Records a trusted controller's guarded committed-model correlation after
    /// transport admission as the current active resource.
    ///
    /// The identity is non-authorizing: it correlates an already admitted call
    /// but does not itself prove model commitment or expose active policy
    /// authority to the controller.
    pub fn record_active_model_call(
        &mut self,
        execution: &AgentNodeExecution,
        call: crate::AgentProviderCallIdentity,
    ) -> Result<AgentSemanticProgress, AgentSupervisorRuntimeError> {
        if !call.matches_manifest_revision(self.topology.manifest(), self.topology.manifest_guard())
            || call.node() != execution.node()
        {
            return Err(AgentSupervisorRuntimeError::ProgressAuthority);
        }
        let activity = AgentProgressActivity::try_new(
            AgentProgressOperation::Model,
            Some(AgentProgressResource::ModelCall(call.call())),
        )?;
        self.record_progress_activity(execution, activity)
    }

    /// Records one exact terminal model-call accounting receipt.
    pub fn record_model_call_result(
        &mut self,
        execution: &AgentNodeExecution,
        receipt: AgentModelCallReceipt,
    ) -> Result<AgentSemanticProgress, AgentSupervisorRuntimeError> {
        let index = self.require_running_execution(execution)?;
        if !receipt
            .matches_manifest_revision(self.topology.manifest(), self.topology.manifest_guard())
            || receipt.node() != execution.node()
        {
            return Err(AgentSupervisorRuntimeError::ProgressAuthority);
        }
        let state = match receipt.settlement() {
            AgentModelCallSettlement::Completed => AgentProgressState::Succeeded,
            AgentModelCallSettlement::ProviderFailed => AgentProgressState::Failed,
            AgentModelCallSettlement::Cancelled => AgentProgressState::Cancelled,
        };
        let blocker = match receipt.settlement() {
            AgentModelCallSettlement::Completed => None,
            AgentModelCallSettlement::ProviderFailed => Some(AgentProgressBlocker::Provider),
            AgentModelCallSettlement::Cancelled => Some(AgentProgressBlocker::ModelCancelled),
        };
        let progress = self.progress(
            execution.node(),
            AgentProgressActivity::try_new(
                AgentProgressOperation::Model,
                Some(AgentProgressResource::ModelCall(receipt.id())),
            )?,
            state,
            Some(AgentProgressResult::Model(receipt.settlement())),
            blocker,
        );
        self.nodes[index].progress = progress;
        Ok(progress)
    }

    /// Records one exact authorized effect before native dispatch.
    pub fn record_effect_permit(
        &mut self,
        execution: &AgentNodeExecution,
        permit: &AgentEffectPermit,
    ) -> Result<AgentSemanticProgress, AgentSupervisorRuntimeError> {
        if !permit
            .matches_manifest_revision(self.topology.manifest(), self.topology.manifest_guard())
            || permit.node() != execution.node()
        {
            return Err(AgentSupervisorRuntimeError::ProgressAuthority);
        }
        let activity = AgentProgressActivity::try_new(
            AgentProgressOperation::Effect(permit.effect()),
            Some(AgentProgressResource::Effect(permit.id())),
        )?;
        self.record_progress_activity(execution, activity)
    }

    /// Records one exact dispatched effect as the current active resource.
    pub fn record_active_effect(
        &mut self,
        execution: &AgentNodeExecution,
        active: &AgentActiveEffect,
    ) -> Result<AgentSemanticProgress, AgentSupervisorRuntimeError> {
        if !active
            .matches_manifest_revision(self.topology.manifest(), self.topology.manifest_guard())
            || active.node() != execution.node()
        {
            return Err(AgentSupervisorRuntimeError::ProgressAuthority);
        }
        let activity = AgentProgressActivity::try_new(
            AgentProgressOperation::Effect(active.effect()),
            Some(AgentProgressResource::Effect(active.id())),
        )?;
        self.record_progress_activity(execution, activity)
    }

    /// Records one exact terminal semantic-effect receipt.
    pub fn record_effect_result(
        &mut self,
        execution: &AgentNodeExecution,
        receipt: AgentEffectReceipt,
    ) -> Result<AgentSemanticProgress, AgentSupervisorRuntimeError> {
        let index = self.require_running_execution(execution)?;
        if !receipt
            .matches_manifest_revision(self.topology.manifest(), self.topology.manifest_guard())
            || receipt.node() != execution.node()
        {
            return Err(AgentSupervisorRuntimeError::ProgressAuthority);
        }
        let (state, blocker) = match receipt.settlement() {
            crate::AgentEffectSettlement::Verified(_) => (AgentProgressState::Succeeded, None),
            crate::AgentEffectSettlement::Failed(failure) => (
                AgentProgressState::Failed,
                Some(AgentProgressBlocker::Effect(failure)),
            ),
        };
        let progress = self.progress(
            execution.node(),
            AgentProgressActivity::try_new(
                AgentProgressOperation::Effect(receipt.effect()),
                Some(AgentProgressResource::Effect(receipt.id())),
            )?,
            state,
            Some(AgentProgressResult::Effect(receipt.settlement())),
            blocker,
        );
        self.nodes[index].progress = progress;
        Ok(progress)
    }

    /// Atomically records a policy-derived human blocker and releases the
    /// current execution slot into a resource-free yielded wait. The borrowed
    /// execution token is invalid after success and must be dropped.
    pub fn wait_for_human(
        &mut self,
        execution: &AgentNodeExecution,
        transition: AgentNeedsHumanTransition,
    ) -> Result<AgentSupervisorExecutionReceipt, AgentSupervisorRuntimeError> {
        let index = self.require_running_execution(execution)?;
        if !transition
            .matches_manifest_revision(self.topology.manifest(), self.topology.manifest_guard())
            || transition.node() != execution.node()
            || !self.owns_context_assignment(execution.node(), transition.context().identity())
        {
            return Err(AgentSupervisorRuntimeError::ProgressAuthority);
        }
        let wait = AgentSupervisorWait::Yielded;
        self.nodes[index].progress = self.progress(
            execution.node(),
            AgentProgressActivity {
                operation: AgentProgressOperation::Approval(transition.effect()),
                resource: Some(AgentProgressResource::Context(
                    transition.context().identity().id(),
                )),
            },
            AgentProgressState::Waiting,
            Some(AgentProgressResult::Supervisor(
                AgentSupervisorExecutionOutcome::Waiting(wait),
            )),
            Some(AgentProgressBlocker::NeedsHuman(transition.reason())),
        );
        self.nodes[index].state = NodeState::Waiting(wait);
        Ok(AgentSupervisorExecutionReceipt {
            supervisor: execution.supervisor,
            node: execution.node,
            attempt: execution.attempt,
            outcome: AgentSupervisorExecutionOutcome::Waiting(wait),
        })
    }

    pub(super) fn progress(
        &self,
        responsibility: AgentPlanNodeId,
        activity: AgentProgressActivity,
        state: AgentProgressState,
        result: Option<AgentProgressResult>,
        blocker: Option<AgentProgressBlocker>,
    ) -> AgentSemanticProgress {
        AgentSemanticProgress {
            manifest: self.topology.manifest(),
            supervisor: self.id,
            responsibility,
            activity,
            state,
            result,
            blocker,
        }
    }

    pub(super) fn scheduled_progress(
        &self,
        node: AgentPlanNodeId,
        state: AgentProgressState,
        resource: Option<AgentProgressResource>,
    ) -> AgentSemanticProgress {
        self.progress(
            node,
            AgentProgressActivity::scheduling(resource),
            state,
            None,
            None,
        )
    }
}

const fn activity_resource_matches(
    operation: AgentProgressOperation,
    resource: Option<AgentProgressResource>,
) -> bool {
    matches!(
        (operation, resource),
        (AgentProgressOperation::Scheduling, None)
            | (
                AgentProgressOperation::Scheduling,
                Some(AgentProgressResource::Execution(_))
            )
            | (AgentProgressOperation::Planning, None)
            | (
                AgentProgressOperation::Delegation,
                Some(AgentProgressResource::PlanNode(_))
            )
            | (
                AgentProgressOperation::Model,
                Some(AgentProgressResource::ModelCall(_))
            )
            | (
                AgentProgressOperation::Observation,
                Some(AgentProgressResource::Context(_))
            )
            | (
                AgentProgressOperation::Read,
                Some(AgentProgressResource::Context(_))
            )
            | (
                AgentProgressOperation::Context,
                Some(AgentProgressResource::Context(_))
            )
            | (
                AgentProgressOperation::Effect(_),
                Some(AgentProgressResource::Effect(_))
            )
            | (
                AgentProgressOperation::Verification,
                Some(AgentProgressResource::Effect(_))
            )
            | (
                AgentProgressOperation::Approval(_),
                Some(AgentProgressResource::Context(_))
            )
            | (AgentProgressOperation::Persistence, None)
    )
}
