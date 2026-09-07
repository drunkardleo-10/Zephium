//! Process-local Work scheduling over the original approved supervisor tree.
//!
//! This is an explicit-transition functional core, not a worker pool or model
//! router. It creates no task, timer, channel or native resource. The original
//! AgentRunPolicy still exclusively admits and accounts model/tool operations;
//! a scheduling token is never an effect, input-delivery or approval permit.
//! Checkpoints and opaque outputs are facts, not restart or publication proof.
//!
//! Compatibility: this new API accepts Work-bound evidence references only.
//! Legacy journal/artifact records have no WorkId. Enabling durable artifact
//! handoff requires a later WorkId-aware journal/artifact migration and an
//! original-owner publication join, not a caller-supplied label. Existing stored
//! artifacts and their publication/retrieval APIs are unchanged by this slice.

use super::*;
use crate::{
    AgentPolicyInstant, AgentRunBudget, AgentRunManifest, AgentRunManifestId,
    AgentWorkArtifactDescriptor, AgentWorkProfileId, ContextRunId, WorkEvidenceDescriptor, WorkId,
};

/// Cumulative scheduling turns, including yielded turns; never renewed by a child.
/// This structural bound is independent of provider/operation/cost accounting.
pub const MAX_AGENT_WORK_ORCHESTRATION_TURNS: u32 = 256;
/// Maximum opaque output references in one successful node result.
pub const MAX_AGENT_WORK_NODE_OUTPUTS: usize = 8;
/// Cumulative output references across the entire tree, including handed-off results.
pub const MAX_AGENT_WORK_ORCHESTRATION_OUTPUTS: usize = 64;

/// Opaque data identity, not payload, execution authority or publication proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWorkOutputReference {
    /// Immutable in-memory evidence with its original Work/profile/run ownership.
    Evidence(WorkEvidenceDescriptor),
    /// Legacy artifact metadata has no Work identity and is therefore refused by
    /// completion with `ArtifactWorkBindingRequired`, even if profile and run
    /// match. Admission needs a future original-owner Work/publication join;
    /// supplying a Work label or decoded metadata cannot manufacture that proof.
    UnboundArtifact(AgentWorkArtifactDescriptor),
}

impl AgentWorkOutputReference {
    fn same_identity(self, other: Self) -> bool {
        match (self, other) {
            (Self::Evidence(a), Self::Evidence(b)) => a.id() == b.id(),
            (Self::UnboundArtifact(a), Self::UnboundArtifact(b)) => a.id() == b.id(),
            _ => false,
        }
    }
}

/// Move-only scheduling turn. Dropping it does not drain or free its original slot.
#[must_use]
pub struct AgentWorkExecution {
    execution: AgentNodeExecution,
    work: WorkId,
    profile: AgentWorkProfileId,
}

impl AgentWorkExecution {
    /// Existing approved plan-node identity.
    pub const fn node(&self) -> AgentPlanNodeId {
        self.execution.node()
    }
    /// Exact process-local attempt, never a persistent execution permission.
    pub const fn attempt(&self) -> AgentSupervisorAttemptId {
        self.execution.attempt()
    }

    // Keep the caller's original move-only token on refusal. This private copy
    // is passed only to the original supervisor settlement, never to a worker
    // or public consumer. The original is dropped only after accepted mutation.
    fn settlement_token(&self) -> AgentNodeExecution {
        AgentNodeExecution {
            supervisor: self.execution.supervisor,
            node: self.execution.node,
            attempt: self.execution.attempt,
            guard: self.execution.guard,
        }
    }
}

/// Refusal retains the exact execution for cancellation or explicit correction.
#[must_use]
#[derive(Debug)]
pub struct AgentWorkExecutionRefusal {
    execution: AgentWorkExecution,
    error: AgentWorkOrchestrationError,
}

impl AgentWorkExecutionRefusal {
    /// Closed reason; no provider, page or error text.
    pub const fn error(&self) -> AgentWorkOrchestrationError {
        self.error
    }
    /// Recover the original token without renewing its identity or authority.
    pub fn into_execution(self) -> AgentWorkExecution {
        self.execution
    }
}

/// Immutable one-shot successful direct-child result delivery to one parent turn.
/// References carry no transcript or content and must be separately resolved.
#[must_use]
pub struct AgentWorkChildOutput {
    work: WorkId,
    profile: AgentWorkProfileId,
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    run: ContextRunId,
    supervisor: AgentSupervisorId,
    parent: AgentPlanNodeId,
    child: AgentPlanNodeId,
    producer: AgentSupervisorAttemptId,
    recipient: AgentSupervisorAttemptId,
    outputs: Vec<AgentWorkOutputReference>,
}

impl AgentWorkChildOutput {
    /// Original Work ownership label, not a new aggregate.
    pub const fn work(&self) -> WorkId {
        self.work
    }
    /// Exact profile privacy boundary.
    pub const fn profile(&self) -> AgentWorkProfileId {
        self.profile
    }
    /// Original approved execution-manifest identity.
    pub const fn manifest(&self) -> AgentRunManifestId {
        self.manifest
    }
    /// Exact immutable approval revision, not merely equal descriptive IDs.
    pub fn matches_manifest(&self, manifest: &AgentRunManifest) -> bool {
        self.manifest == manifest.id() && self.manifest_guard == manifest.guard()
    }
    /// Original run; no successor run can inherit this delivery as authority.
    pub const fn run(&self) -> ContextRunId {
        self.run
    }
    /// Original mutable supervisor incarnation.
    pub const fn supervisor(&self) -> AgentSupervisorId {
        self.supervisor
    }
    /// Exact direct parent receiving the child result.
    pub const fn parent(&self) -> AgentPlanNodeId {
        self.parent
    }
    /// Exact successfully completed child.
    pub const fn child(&self) -> AgentPlanNodeId {
        self.child
    }
    /// Exact child attempt that completed with these references.
    pub const fn producer(&self) -> AgentSupervisorAttemptId {
        self.producer
    }
    /// Exact current parent attempt that consumed this one-shot delivery.
    pub const fn recipient(&self) -> AgentSupervisorAttemptId {
        self.recipient
    }
    /// Bounded immutable data references, never model input or effect permission.
    pub fn outputs(&self) -> &[AgentWorkOutputReference] {
        &self.outputs
    }
}

struct WorkNode {
    node: AgentPlanNodeId,
    deadline: AgentPolicyInstant,
    approved_budget: AgentRunBudget,
    completed_attempt: Option<AgentSupervisorAttemptId>,
    outputs: Option<Vec<AgentWorkOutputReference>>,
    output_count: usize,
    handed_off: bool,
}

/// Immutable checkpoint of one activated responsibility, containing no payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentWorkNodeCheckpoint {
    node: AgentSupervisorNodeSnapshot,
    parent: Option<AgentPlanNodeId>,
    approved_budget: AgentRunBudget,
    deadline: AgentPolicyInstant,
    completed_attempt: Option<AgentSupervisorAttemptId>,
    output_count: usize,
    handed_off: bool,
}

impl AgentWorkNodeCheckpoint {
    /// Original scheduler state and typed progress, not a worker-drain proof.
    pub const fn node(self) -> AgentSupervisorNodeSnapshot {
        self.node
    }
    /// Approved delegation parent, absent for the root.
    pub const fn parent(self) -> Option<AgentPlanNodeId> {
        self.parent
    }
    /// Frozen policy ceiling, NOT remaining or consumed provider budget.
    pub const fn approved_budget(self) -> AgentRunBudget {
        self.approved_budget
    }
    /// Original monotonic admission expiry, not a portable wall-clock timestamp.
    pub const fn deadline(self) -> AgentPolicyInstant {
        self.deadline
    }
    /// Attempt that explicitly completed successfully with the output set.
    pub const fn completed_attempt(self) -> Option<AgentSupervisorAttemptId> {
        self.completed_attempt
    }
    /// Number of references originally completed, including delivered references.
    pub const fn output_count(self) -> usize {
        self.output_count
    }
    /// Whether the direct parent already consumed the one-shot output set.
    pub const fn handed_off(self) -> bool {
        self.handed_off
    }
}

/// Immutable bounded projection for a future durable-plan adapter.
/// No decoder, restored token, auto-resume or persistence claim is provided.
pub struct AgentWorkOrchestrationCheckpoint {
    work: WorkId,
    profile: AgentWorkProfileId,
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    run: ContextRunId,
    supervisor: AgentSupervisorId,
    turns: u32,
    output_count: usize,
    status: AgentSupervisorRuntimeStatus,
    nodes: Vec<AgentWorkNodeCheckpoint>,
}

impl AgentWorkOrchestrationCheckpoint {
    /// Caller-supplied existing Work identity.
    pub const fn work(&self) -> WorkId {
        self.work
    }
    /// Original owning profile.
    pub const fn profile(&self) -> AgentWorkProfileId {
        self.profile
    }
    /// Original approved execution identity.
    pub const fn manifest(&self) -> AgentRunManifestId {
        self.manifest
    }
    /// Checks the exact approved revision without reconstructing authority.
    pub fn matches_manifest(&self, manifest: &AgentRunManifest) -> bool {
        self.manifest == manifest.id() && self.manifest_guard == manifest.guard()
    }
    /// Exact execution run, independent of the persistent Work aggregate.
    pub const fn run(&self) -> ContextRunId {
        self.run
    }
    /// Process-local supervisor incarnation, not rehydration authority.
    pub const fn supervisor(&self) -> AgentSupervisorId {
        self.supervisor
    }
    /// Cumulative structural scheduling turns, not provider operations or usage.
    pub const fn turns(&self) -> u32 {
        self.turns
    }
    /// Cumulative reference count, never renewed after handoff.
    pub const fn output_count(&self) -> usize {
        self.output_count
    }
    /// Original scheduler status, including an ambiguous/sealed owner.
    pub const fn status(&self) -> AgentSupervisorRuntimeStatus {
        self.status
    }
    /// Bounded immutable activated-node state in canonical node order.
    pub fn nodes(&self) -> &[AgentWorkNodeCheckpoint] {
        &self.nodes
    }
}

/// Single-owner Work execution state over the existing supervisor, with no idle work.
/// Construction does not start an agent or authorize any model/browser operation.
#[must_use]
pub struct AgentWorkOrchestration {
    work: WorkId,
    profile: AgentWorkProfileId,
    supervisor: AgentRunSupervisor,
    nodes: Vec<WorkNode>,
    issued: AgentPolicyInstant,
    observed: AgentPolicyInstant,
    turns: u32,
    output_count: usize,
}

impl AgentWorkOrchestration {
    /// Binds a single-profile Work to an already-approved exact manifest/topology.
    /// The caller supplies the original Work identity; none is minted here.
    pub fn try_new(
        work: WorkId,
        profile: AgentWorkProfileId,
        id: AgentSupervisorId,
        manifest: &AgentRunManifest,
        topology: AgentDelegationTopology,
    ) -> Result<Self, AgentWorkOrchestrationError> {
        if !topology.matches_manifest(manifest) {
            return Err(AgentWorkOrchestrationError::Manifest);
        }
        if manifest.scope().profiles() != [profile] {
            return Err(AgentWorkOrchestrationError::OutputOwnership);
        }
        let nodes = manifest
            .plan_nodes()
            .iter()
            .filter(|node| topology.node(node.id()).is_some())
            .map(|node| WorkNode {
                node: node.id(),
                deadline: node.expires_at(),
                approved_budget: node.budget(),
                completed_attempt: None,
                outputs: None,
                output_count: 0,
                handed_off: false,
            })
            .collect();
        Ok(Self {
            work,
            profile,
            supervisor: AgentRunSupervisor::new(id, topology),
            nodes,
            issued: manifest.issued_at(),
            observed: manifest.issued_at(),
            turns: 0,
            output_count: 0,
        })
    }

    /// Original scheduler status. Queued/waiting state owns no worker or timer.
    pub fn status(&self) -> AgentSupervisorRuntimeStatus {
        self.supervisor.status()
    }

    /// Admit a bounded scheduling turn under the original monotonic node expiry.
    pub fn start(
        &mut self,
        node: AgentPlanNodeId,
        attempt: AgentSupervisorAttemptId,
        now: AgentPolicyInstant,
    ) -> Result<AgentWorkExecution, AgentWorkOrchestrationError> {
        self.check_time(node, now)?;
        if self.turns >= MAX_AGENT_WORK_ORCHESTRATION_TURNS {
            return Err(AgentWorkOrchestrationError::TurnLimit);
        }
        let execution = self.supervisor.start(node, attempt)?;
        self.turns += 1;
        Ok(AgentWorkExecution {
            execution,
            work: self.work,
            profile: self.profile,
        })
    }

    /// Activate only a pre-approved direct child of the exact current parent.
    pub fn delegate(
        &mut self,
        parent: &AgentWorkExecution,
        child: AgentPlanNodeId,
        now: AgentPolicyInstant,
    ) -> Result<(), AgentWorkOrchestrationError> {
        self.check_owner(parent)?;
        self.supervisor
            .require_running_execution(&parent.execution)?;
        self.check_time(parent.node(), now)?;
        self.check_time(child, now)?;
        self.supervisor.delegate(&parent.execution, child)?;
        Ok(())
    }

    /// Update closed semantic progress without text or execution authority.
    pub fn record_progress(
        &mut self,
        execution: &AgentWorkExecution,
        activity: AgentProgressActivity,
    ) -> Result<AgentSemanticProgress, AgentWorkOrchestrationError> {
        self.check_owner(execution)?;
        Ok(self
            .supervisor
            .record_progress_activity(&execution.execution, activity)?)
    }

    /// Release a scheduling slot without retaining a worker or promise.
    pub fn wait(
        &mut self,
        execution: AgentWorkExecution,
        reason: AgentSupervisorWait,
    ) -> Result<AgentSupervisorExecutionReceipt, AgentWorkExecutionRefusal> {
        if let Err(error) = self.check_owner(&execution) {
            return Err(AgentWorkExecutionRefusal { execution, error });
        }
        self.supervisor
            .wait(execution.settlement_token(), reason)
            .map_err(|error| AgentWorkExecutionRefusal {
                execution,
                error: error.into(),
            })
    }

    /// Complete one exact turn with immutable data references. This is a trusted
    /// scheduler decision, NOT evidence of objective success, durable publication
    /// or native closure. Live children must first finish or explicitly cancel.
    /// Expired/invalid completion preserves the token for cancellation or correction.
    pub fn complete(
        &mut self,
        execution: AgentWorkExecution,
        completion: AgentSupervisorCompletion,
        outputs: &[AgentWorkOutputReference],
        now: AgentPolicyInstant,
    ) -> Result<AgentSupervisorExecutionReceipt, AgentWorkExecutionRefusal> {
        match self.complete_inner(&execution, completion, outputs, now) {
            Ok(receipt) => Ok(receipt),
            Err(error) => Err(AgentWorkExecutionRefusal { execution, error }),
        }
    }

    fn complete_inner(
        &mut self,
        execution: &AgentWorkExecution,
        completion: AgentSupervisorCompletion,
        outputs: &[AgentWorkOutputReference],
        now: AgentPolicyInstant,
    ) -> Result<AgentSupervisorExecutionReceipt, AgentWorkOrchestrationError> {
        self.check_owner(execution)?;
        let (_, cancellation) = self.supervisor.require_execution(&execution.execution)?;
        // Cancellation wins even over late successful output or expiry. Original
        // descendant/slot drain rules still decide when the node becomes terminal.
        if cancellation.is_some() {
            return Ok(self
                .supervisor
                .complete(execution.settlement_token(), completion)?);
        }
        self.check_time(execution.node(), now)?;
        if self.supervisor.has_live_descendant(execution.node()) {
            return Err(AgentSupervisorRuntimeError::DescendantsLive.into());
        }
        if outputs.len() > MAX_AGENT_WORK_NODE_OUTPUTS
            || self.output_count + outputs.len() > MAX_AGENT_WORK_ORCHESTRATION_OUTPUTS
        {
            return Err(AgentWorkOrchestrationError::OutputLimit);
        }
        if completion != AgentSupervisorCompletion::Succeeded && !outputs.is_empty() {
            return Err(AgentWorkOrchestrationError::OutputState);
        }
        for (index, output) in outputs.iter().enumerate() {
            self.validate_output(*output)?;
            if outputs[..index]
                .iter()
                .any(|other| other.same_identity(*output))
            {
                return Err(AgentWorkOrchestrationError::OutputDuplicate);
            }
        }
        // Allocate before consuming the original execution. No partial publication
        // on allocation refusal or a wrong token. Only scalar mutation follows.
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(outputs.len())
            .map_err(|_| AgentWorkOrchestrationError::Capacity)?;
        retained.extend_from_slice(outputs);
        let index = self.node_index(execution.node())?;
        let receipt = self
            .supervisor
            .complete(execution.settlement_token(), completion)?;
        if receipt.outcome() == AgentSupervisorExecutionOutcome::Succeeded {
            self.nodes[index].completed_attempt = Some(execution.attempt());
            self.nodes[index].outputs = Some(retained);
            self.nodes[index].output_count = outputs.len();
            self.output_count += outputs.len();
        }
        Ok(receipt)
    }

    /// Take a successfully completed direct child's outputs exactly once, bound
    /// to this currently executing parent attempt. Siblings and old turns refuse.
    pub fn take_child_output(
        &mut self,
        parent: &AgentWorkExecution,
        child: AgentPlanNodeId,
        now: AgentPolicyInstant,
    ) -> Result<AgentWorkChildOutput, AgentWorkOrchestrationError> {
        self.check_owner(parent)?;
        self.supervisor
            .require_running_execution(&parent.execution)?;
        self.check_time(parent.node(), now)?;
        if self
            .supervisor
            .topology()
            .node(child)
            .is_none_or(|node| node.parent() != Some(parent.node()))
        {
            return Err(AgentSupervisorRuntimeError::DelegationMismatch.into());
        }
        if self.supervisor.node_status(child) != Some(AgentSupervisorNodeStatus::Succeeded) {
            return Err(AgentWorkOrchestrationError::OutputState);
        }
        let index = self.node_index(child)?;
        let producer = self.nodes[index]
            .completed_attempt
            .ok_or(AgentWorkOrchestrationError::OutputState)?;
        let outputs = self.nodes[index]
            .outputs
            .take()
            .ok_or(AgentWorkOrchestrationError::AlreadyDelivered)?;
        self.nodes[index].handed_off = true;
        Ok(AgentWorkChildOutput {
            work: self.work,
            profile: self.profile,
            manifest: self.supervisor.topology().manifest(),
            manifest_guard: self.supervisor.topology().manifest_guard(),
            run: self.supervisor.topology().run(),
            supervisor: self.supervisor.id(),
            parent: parent.node(),
            child,
            producer,
            recipient: parent.attempt(),
            outputs,
        })
    }

    /// Request sticky subtree cancellation. No slot is freed until its exact
    /// original token drains; this only returns the existing bounded targets.
    pub fn cancel_subtree(
        &mut self,
        root: AgentPlanNodeId,
        id: AgentSupervisorCancellationId,
        reason: AgentSupervisorCancellationReason,
    ) -> Result<AgentSupervisorCancellationBatch, AgentWorkOrchestrationError> {
        Ok(self.supervisor.cancel_subtree(root, id, reason)?)
    }

    /// Explicit original-token drain, not dropping a future or forgetting a child.
    pub fn drain_cancelled(
        &mut self,
        execution: AgentWorkExecution,
        cancellation: AgentSupervisorCancellationId,
    ) -> Result<AgentSupervisorExecutionReceipt, AgentWorkExecutionRefusal> {
        if let Err(error) = self.check_owner(&execution) {
            return Err(AgentWorkExecutionRefusal { execution, error });
        }
        self.supervisor
            .drain_cancelled(execution.settlement_token(), cancellation)
            .map_err(|error| AgentWorkExecutionRefusal {
                execution,
                error: error.into(),
            })
    }

    /// Snapshot current bounded facts on demand. This allocates only on request,
    /// never on idle or per model token, and cannot recreate execution authority.
    pub fn checkpoint(
        &self,
    ) -> Result<AgentWorkOrchestrationCheckpoint, AgentWorkOrchestrationError> {
        let nodes = self
            .supervisor
            .nodes()
            .map(|node| {
                let state = &self.nodes[self.node_index(node.node())?];
                let topology = self
                    .supervisor
                    .topology()
                    .node(node.node())
                    .ok_or(AgentSupervisorRuntimeError::NodeMissing)?;
                Ok(AgentWorkNodeCheckpoint {
                    node,
                    parent: topology.parent(),
                    approved_budget: state.approved_budget,
                    deadline: state.deadline,
                    completed_attempt: state.completed_attempt,
                    output_count: state.output_count,
                    handed_off: state.handed_off,
                })
            })
            .collect::<Result<Vec<_>, AgentWorkOrchestrationError>>()?;
        Ok(AgentWorkOrchestrationCheckpoint {
            work: self.work,
            profile: self.profile,
            manifest: self.supervisor.topology().manifest(),
            manifest_guard: self.supervisor.topology().manifest_guard(),
            run: self.supervisor.topology().run(),
            supervisor: self.supervisor.id(),
            turns: self.turns,
            output_count: self.output_count,
            status: self.status(),
            nodes,
        })
    }

    /// Root's immutable references after explicit successful completion. Borrowing
    /// metadata does not publish an artifact or discharge external runtime debt.
    pub fn root_outputs(&self) -> Option<&[AgentWorkOutputReference]> {
        let root = self.supervisor.topology().root();
        if self.supervisor.node_status(root) != Some(AgentSupervisorNodeStatus::Succeeded) {
            return None;
        }
        self.nodes
            .iter()
            .find(|row| row.node == root)?
            .outputs
            .as_deref()
    }

    fn node_index(&self, node: AgentPlanNodeId) -> Result<usize, AgentWorkOrchestrationError> {
        self.nodes
            .binary_search_by_key(&node, |row| row.node)
            .map_err(|_| AgentSupervisorRuntimeError::NodeMissing.into())
    }

    fn check_owner(
        &self,
        execution: &AgentWorkExecution,
    ) -> Result<(), AgentWorkOrchestrationError> {
        if execution.work != self.work || execution.profile != self.profile {
            return Err(AgentWorkOrchestrationError::OutputOwnership);
        }
        Ok(())
    }

    fn check_time(
        &mut self,
        node: AgentPlanNodeId,
        now: AgentPolicyInstant,
    ) -> Result<(), AgentWorkOrchestrationError> {
        let index = self.node_index(node)?;
        if now < self.issued || now < self.observed {
            return Err(AgentWorkOrchestrationError::Clock);
        }
        self.observed = now;
        if now >= self.nodes[index].deadline {
            return Err(AgentWorkOrchestrationError::Expired);
        }
        Ok(())
    }

    fn validate_output(
        &self,
        output: AgentWorkOutputReference,
    ) -> Result<(), AgentWorkOrchestrationError> {
        match output {
            AgentWorkOutputReference::Evidence(descriptor) => {
                if descriptor.work() == self.work
                    && descriptor.profile() == self.profile
                    && descriptor.run() == self.supervisor.topology().run()
                {
                    Ok(())
                } else {
                    Err(AgentWorkOrchestrationError::OutputOwnership)
                }
            }
            AgentWorkOutputReference::UnboundArtifact(_) => {
                // The original descriptor/publication binds only profile and
                // manifest/run, not Work. One manifest can label multiple Work
                // owners here. Never turn those weaker facts into a Work-bound
                // child result; storage resolution alone cannot repair this join.
                Err(AgentWorkOrchestrationError::ArtifactWorkBindingRequired)
            }
        }
    }
}

impl fmt::Debug for AgentWorkOrchestration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWorkOrchestration")
            .field("status", &self.status())
            .field("turns", &self.turns)
            .field("output_count", &self.output_count)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for AgentWorkExecution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWorkExecution")
            .field("execution", &self.execution)
            .field("owner", &"[redacted]")
            .finish()
    }
}

impl fmt::Debug for AgentWorkChildOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWorkChildOutput")
            .field("outputs", &self.outputs.len())
            .field("identity", &"[redacted]")
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for AgentWorkOrchestrationCheckpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWorkOrchestrationCheckpoint")
            .field("status", &self.status)
            .field("turns", &self.turns)
            .field("output_count", &self.output_count)
            .field("identity", &"[redacted]")
            .finish_non_exhaustive()
    }
}

/// Closed orchestration refusal; no native, provider or content-bearing text.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentWorkOrchestrationError {
    /// Topology and original manifest revision differ.
    #[error("work orchestration manifest mismatch")]
    Manifest,
    /// Clock regressed or predates the original approval.
    #[error("work orchestration clock regressed")]
    Clock,
    /// Admission expired; original tokens remain available for cancellation drain.
    #[error("work orchestration node expired")]
    Expired,
    /// Original cumulative structural turn budget exhausted.
    #[error("work orchestration scheduling turn ceiling reached")]
    TurnLimit,
    /// Per-node or whole-tree output bound reached.
    #[error("work orchestration output ceiling reached")]
    OutputLimit,
    /// Reference does not belong to the exact Work/profile/run destination.
    #[error("work orchestration output ownership mismatch")]
    OutputOwnership,
    /// Legacy artifact metadata cannot prove which Work owns its publication.
    /// No label, descriptor decode or equal manifest/run may substitute for it.
    #[error("work orchestration artifact requires an original Work ownership binding")]
    ArtifactWorkBindingRequired,
    /// Same output identity appeared twice in one immutable result.
    #[error("work orchestration output identity duplicated")]
    OutputDuplicate,
    /// Output is unavailable or supplied for a failed node.
    #[error("work orchestration output requires successful completion")]
    OutputState,
    /// Parent already consumed this child's immutable output set.
    #[error("work orchestration child output already delivered")]
    AlreadyDelivered,
    /// Bounded metadata allocation refused before mutation.
    #[error("work orchestration metadata capacity unavailable")]
    Capacity,
    /// Original supervisor refused ownership, bounds or lifecycle mutation.
    #[error(transparent)]
    Supervisor(#[from] AgentSupervisorRuntimeError),
}

#[cfg(test)]
mod tests;
