//! Single-owner bounded scheduling over a validated delegation topology.

use std::fmt;
use std::num::NonZeroU64;

use sha2::{Digest, Sha256};
use thiserror::Error;

use super::{
    AgentDelegationTopology, MAX_AGENT_EXECUTING_SUPERVISOR_NODES, MAX_AGENT_LIVE_SUPERVISOR_NODES,
};
use crate::{AgentPlanNodeId, SemanticActionFailure};

/// Process-local identity for one exact mutable supervisor incarnation.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentSupervisorId(NonZeroU64);

impl AgentSupervisorId {
    /// Constructs one nonzero shell-minted supervisor identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Numeric value for exact process-local correlation.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for AgentSupervisorId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentSupervisorId([redacted])")
    }
}

/// Strictly increasing identity for one admitted node execution turn.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentSupervisorAttemptId(NonZeroU64);

impl AgentSupervisorAttemptId {
    /// Constructs one nonzero shell-minted execution identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Numeric value for exact process-local correlation.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for AgentSupervisorAttemptId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentSupervisorAttemptId([redacted])")
    }
}

/// Why a node released its execution slot without becoming terminal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentSupervisorWait {
    /// At least one activated descendant must finish first.
    Descendants,
    /// The shell deliberately yielded a turn with no retained async work.
    Yielded,
}

/// Closed terminal supervisor-node failure without provider or page text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentSupervisorFailure {
    /// Exact run/node budget was exhausted.
    BudgetExhausted,
    /// Provider transport or typed provider response terminally failed.
    ProviderFailed,
    /// Model output failed the closed parser or policy contract.
    InvalidModelOutput,
    /// Deterministic policy refused continuation.
    PolicyDenied,
    /// A semantic action ended in one exact typed failure.
    Action(SemanticActionFailure),
    /// A hard scheduler/context/payload resource ceiling was reached.
    ResourceExhausted,
}

/// Strictly increasing identity for one admitted cancellation-tree update.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentSupervisorCancellationId(NonZeroU64);

impl AgentSupervisorCancellationId {
    /// Constructs one nonzero shell-minted cancellation identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Numeric value for exact process-local correlation.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for AgentSupervisorCancellationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentSupervisorCancellationId([redacted])")
    }
}

/// Closed reason for cancelling one activated run-tree branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentSupervisorCancellationReason {
    /// The person explicitly stopped the run or branch.
    UserRequested,
    /// Human takeover revoked automation authority.
    HumanTakeover,
    /// An ancestor failed and no longer needs this branch.
    ParentTerminated,
    /// The branch reached its trusted monotonic deadline.
    DeadlineExceeded,
    /// A hard run or node budget was exhausted.
    BudgetExhausted,
    /// Deterministic policy revoked continuation authority.
    PolicyRevoked,
    /// Process shutdown requires bounded terminal cleanup.
    Shutdown,
}

/// Content-free identity and reason for one cancellation-tree update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentSupervisorCancellation {
    id: AgentSupervisorCancellationId,
    reason: AgentSupervisorCancellationReason,
}

impl AgentSupervisorCancellation {
    /// Exact one-shot cancellation update.
    pub const fn id(self) -> AgentSupervisorCancellationId {
        self.id
    }

    /// Closed cancellation reason.
    pub const fn reason(self) -> AgentSupervisorCancellationReason {
        self.reason
    }
}

/// Public cancellation state for one node, including retained execution work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentSupervisorNodeCancellation {
    cancellation: AgentSupervisorCancellation,
    draining_attempt: Option<AgentSupervisorAttemptId>,
}

impl AgentSupervisorNodeCancellation {
    /// Exact cancellation update affecting the node.
    pub const fn cancellation(self) -> AgentSupervisorCancellation {
        self.cancellation
    }

    /// Exact attempt retaining an execution slot until terminal drain.
    pub const fn draining_attempt(self) -> Option<AgentSupervisorAttemptId> {
        self.draining_attempt
    }
}

/// Current content-free state of one activated supervisor node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentSupervisorNodeStatus {
    /// Activated and eligible for an execution slot.
    Queued,
    /// One exact execution token owns a slot.
    Running(AgentSupervisorAttemptId),
    /// No task or execution slot is retained while the node waits.
    Waiting(AgentSupervisorWait),
    /// Cancellation is sticky while execution or descendants still drain.
    Cancelling(AgentSupervisorNodeCancellation),
    /// Node completed successfully.
    Succeeded,
    /// Node terminated under the closed failure taxonomy.
    Failed(AgentSupervisorFailure),
    /// Node and every activated descendant terminally cancelled.
    Cancelled(AgentSupervisorCancellation),
}

impl AgentSupervisorNodeStatus {
    /// Whether the node still counts against the live-node ceiling.
    pub const fn is_live(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Running(_) | Self::Waiting(_) | Self::Cancelling(_)
        )
    }

    /// Whether the node owns one executing-agent slot.
    pub const fn is_executing(self) -> bool {
        match self {
            Self::Running(_) => true,
            Self::Cancelling(cancellation) => cancellation.draining_attempt().is_some(),
            Self::Queued
            | Self::Waiting(_)
            | Self::Succeeded
            | Self::Failed(_)
            | Self::Cancelled(_) => false,
        }
    }

    /// Whether no later execution may reopen this node.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed(_) | Self::Cancelled(_))
    }
}

#[derive(Clone, Copy)]
struct RunningState {
    attempt: AgentSupervisorAttemptId,
    guard: [u8; 32],
}

#[derive(Clone, Copy)]
struct CancellingState {
    cancellation: AgentSupervisorCancellation,
    running: Option<RunningState>,
}

impl CancellingState {
    const fn public(self) -> AgentSupervisorNodeCancellation {
        AgentSupervisorNodeCancellation {
            cancellation: self.cancellation,
            draining_attempt: match self.running {
                Some(running) => Some(running.attempt),
                None => None,
            },
        }
    }
}

#[derive(Clone, Copy)]
enum NodeState {
    Queued,
    Running(RunningState),
    Waiting(AgentSupervisorWait),
    Cancelling(CancellingState),
    Succeeded,
    Failed(AgentSupervisorFailure),
    Cancelled(AgentSupervisorCancellation),
}

impl NodeState {
    const fn public(self) -> AgentSupervisorNodeStatus {
        match self {
            Self::Queued => AgentSupervisorNodeStatus::Queued,
            Self::Running(running) => AgentSupervisorNodeStatus::Running(running.attempt),
            Self::Waiting(reason) => AgentSupervisorNodeStatus::Waiting(reason),
            Self::Cancelling(cancellation) => {
                AgentSupervisorNodeStatus::Cancelling(cancellation.public())
            }
            Self::Succeeded => AgentSupervisorNodeStatus::Succeeded,
            Self::Failed(failure) => AgentSupervisorNodeStatus::Failed(failure),
            Self::Cancelled(cancellation) => AgentSupervisorNodeStatus::Cancelled(cancellation),
        }
    }
}

struct SupervisorNodeRow {
    node: AgentPlanNodeId,
    state: NodeState,
}

/// Privacy-preserving snapshot of one activated node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentSupervisorNodeSnapshot {
    node: AgentPlanNodeId,
    status: AgentSupervisorNodeStatus,
}

impl AgentSupervisorNodeSnapshot {
    /// Exact pre-approved manifest plan node.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }

    /// Current bounded scheduler state.
    pub const fn status(self) -> AgentSupervisorNodeStatus {
        self.status
    }
}

/// Aggregate resource-accounting projection of one mutable supervisor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentSupervisorRuntimeStatus {
    activated: usize,
    live: usize,
    executing: usize,
    queued: usize,
    waiting: usize,
    cancelling: usize,
    terminal: usize,
    cancelled: usize,
    sealed: bool,
}

impl AgentSupervisorRuntimeStatus {
    /// Nodes activated at least once, including terminal history.
    pub const fn activated(self) -> usize {
        self.activated
    }

    /// Nonterminal nodes consuming the live-tree ceiling.
    pub const fn live(self) -> usize {
        self.live
    }

    /// Nodes holding exact execution slots.
    pub const fn executing(self) -> usize {
        self.executing
    }

    /// Runnable nodes not holding execution slots.
    pub const fn queued(self) -> usize {
        self.queued
    }

    /// Nodes deliberately holding neither tasks nor execution slots.
    pub const fn waiting(self) -> usize {
        self.waiting
    }

    /// Nodes awaiting an exact running drain or live descendants.
    pub const fn cancelling(self) -> usize {
        self.cancelling
    }

    /// Successfully, unsuccessfully, or cancelled terminal nodes.
    pub const fn terminal(self) -> usize {
        self.terminal
    }

    /// Terminally cancelled nodes.
    pub const fn cancelled(self) -> usize {
        self.cancelled
    }

    /// Whether token/settlement ambiguity sealed further mutation.
    pub const fn is_sealed(self) -> bool {
        self.sealed
    }
}

/// Non-cloneable ownership of one exact executing-agent slot.
#[must_use]
pub struct AgentNodeExecution {
    supervisor: AgentSupervisorId,
    node: AgentPlanNodeId,
    attempt: AgentSupervisorAttemptId,
    guard: [u8; 32],
}

impl AgentNodeExecution {
    /// Exact mutable supervisor incarnation.
    pub const fn supervisor(&self) -> AgentSupervisorId {
        self.supervisor
    }

    /// Exact executing plan node.
    pub const fn node(&self) -> AgentPlanNodeId {
        self.node
    }

    /// Exact one-shot execution attempt.
    pub const fn attempt(&self) -> AgentSupervisorAttemptId {
        self.attempt
    }
}

impl fmt::Debug for AgentNodeExecution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentNodeExecution")
            .field("supervisor", &self.supervisor)
            .field("node", &self.node)
            .field("attempt", &self.attempt)
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Terminal or slot-releasing result of one exact execution turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentSupervisorExecutionOutcome {
    /// Node released its slot and may later be admitted again.
    Waiting(AgentSupervisorWait),
    /// Node completed after every activated descendant was terminal.
    Succeeded,
    /// Node failed after every activated descendant was terminal.
    Failed(AgentSupervisorFailure),
    /// Cancellation won the race and the exact execution terminally drained.
    Cancelled(AgentSupervisorCancellation),
}

/// Content-free receipt for one exact settled execution token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentSupervisorExecutionReceipt {
    supervisor: AgentSupervisorId,
    node: AgentPlanNodeId,
    attempt: AgentSupervisorAttemptId,
    outcome: AgentSupervisorExecutionOutcome,
}

impl AgentSupervisorExecutionReceipt {
    /// Exact mutable supervisor incarnation.
    pub const fn supervisor(self) -> AgentSupervisorId {
        self.supervisor
    }

    /// Exact settled plan node.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }

    /// Exact one-shot execution attempt.
    pub const fn attempt(self) -> AgentSupervisorAttemptId {
        self.attempt
    }

    /// Closed terminal or wait transition.
    pub const fn outcome(self) -> AgentSupervisorExecutionOutcome {
        self.outcome
    }
}

/// Explicit terminal execution decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentSupervisorCompletion {
    /// Node succeeded and may never reopen.
    Succeeded,
    /// Node failed under the closed content-free taxonomy.
    Failed(AgentSupervisorFailure),
}

/// One running node that must receive a best-effort cancellation signal.
///
/// This projection carries no execution, action, provider, page, or native
/// authority. Only the original non-cloneable execution token can acknowledge
/// terminal drain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentSupervisorCancellationTarget {
    node: AgentPlanNodeId,
    attempt: AgentSupervisorAttemptId,
    cancellation: AgentSupervisorCancellation,
}

impl AgentSupervisorCancellationTarget {
    /// Exact activated plan node whose current work must stop.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }

    /// Exact executing attempt retaining the scheduler slot.
    pub const fn attempt(self) -> AgentSupervisorAttemptId {
        self.attempt
    }

    /// Exact cancellation update the attempt must drain under.
    pub const fn cancellation(self) -> AgentSupervisorCancellation {
        self.cancellation
    }
}

/// Bounded content-free result of applying one cancellation-tree update.
#[must_use]
pub struct AgentSupervisorCancellationBatch {
    supervisor: AgentSupervisorId,
    root: AgentPlanNodeId,
    cancellation: AgentSupervisorCancellation,
    affected: usize,
    terminal: usize,
    targets: Vec<AgentSupervisorCancellationTarget>,
}

impl AgentSupervisorCancellationBatch {
    /// Exact mutable supervisor incarnation.
    pub const fn supervisor(&self) -> AgentSupervisorId {
        self.supervisor
    }

    /// Root of the activated cancellation subtree.
    pub const fn root(&self) -> AgentPlanNodeId {
        self.root
    }

    /// Exact one-shot cancellation update.
    pub const fn cancellation(&self) -> AgentSupervisorCancellation {
        self.cancellation
    }

    /// Nodes newly made cancelling or cancelled by this update.
    pub const fn affected(&self) -> usize {
        self.affected
    }

    /// Affected nodes made terminal without a running drain callback.
    pub const fn terminal(&self) -> usize {
        self.terminal
    }

    /// Running attempts that retain execution capacity until terminal drain.
    pub fn targets(&self) -> impl ExactSizeIterator<Item = AgentSupervisorCancellationTarget> + '_ {
        self.targets.iter().copied()
    }
}

impl fmt::Debug for AgentSupervisorCancellationBatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentSupervisorCancellationBatch")
            .field("supervisor", &self.supervisor)
            .field("root", &self.root)
            .field("cancellation", &self.cancellation)
            .field("affected", &self.affected)
            .field("terminal", &self.terminal)
            .field("drain_targets", &self.targets.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Single-owner bounded scheduler for one exact delegation topology.
#[must_use]
pub struct AgentRunSupervisor {
    id: AgentSupervisorId,
    topology: AgentDelegationTopology,
    nodes: Vec<SupervisorNodeRow>,
    last_attempt: Option<AgentSupervisorAttemptId>,
    last_cancellation: Option<AgentSupervisorCancellationId>,
    sealed: bool,
}

impl AgentRunSupervisor {
    /// Activates only the topology root without creating tasks or workers.
    pub fn new(id: AgentSupervisorId, topology: AgentDelegationTopology) -> Self {
        let root = topology.root();
        Self {
            id,
            topology,
            nodes: vec![SupervisorNodeRow {
                node: root,
                state: NodeState::Queued,
            }],
            last_attempt: None,
            last_cancellation: None,
            sealed: false,
        }
    }

    /// Exact mutable supervisor incarnation.
    pub const fn id(&self) -> AgentSupervisorId {
        self.id
    }

    /// Exact immutable topology revision.
    pub const fn topology(&self) -> &AgentDelegationTopology {
        &self.topology
    }

    /// Aggregate bounded scheduler/resource state.
    pub fn status(&self) -> AgentSupervisorRuntimeStatus {
        let mut status = AgentSupervisorRuntimeStatus {
            activated: self.nodes.len(),
            live: 0,
            executing: 0,
            queued: 0,
            waiting: 0,
            cancelling: 0,
            terminal: 0,
            cancelled: 0,
            sealed: self.sealed,
        };
        for row in &self.nodes {
            match row.state.public() {
                AgentSupervisorNodeStatus::Queued => {
                    status.live += 1;
                    status.queued += 1;
                }
                AgentSupervisorNodeStatus::Running(_) => {
                    status.live += 1;
                    status.executing += 1;
                }
                AgentSupervisorNodeStatus::Waiting(_) => {
                    status.live += 1;
                    status.waiting += 1;
                }
                AgentSupervisorNodeStatus::Cancelling(cancellation) => {
                    status.live += 1;
                    status.cancelling += 1;
                    if cancellation.draining_attempt().is_some() {
                        status.executing += 1;
                    }
                }
                AgentSupervisorNodeStatus::Succeeded | AgentSupervisorNodeStatus::Failed(_) => {
                    status.terminal += 1;
                }
                AgentSupervisorNodeStatus::Cancelled(_) => {
                    status.terminal += 1;
                    status.cancelled += 1;
                }
            }
        }
        status
    }

    /// Current state for one activated node.
    pub fn node_status(&self, node: AgentPlanNodeId) -> Option<AgentSupervisorNodeStatus> {
        self.node_index(node)
            .map(|index| self.nodes[index].state.public())
    }

    /// Canonical activated-node projections without allocating a snapshot list.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = AgentSupervisorNodeSnapshot> + '_ {
        self.nodes.iter().map(|row| AgentSupervisorNodeSnapshot {
            node: row.node,
            status: row.state.public(),
        })
    }

    /// All running cancellation targets for reconciliation after signal loss.
    pub fn cancellation_targets(
        &self,
    ) -> impl Iterator<Item = AgentSupervisorCancellationTarget> + '_ {
        self.nodes.iter().filter_map(|row| match row.state {
            NodeState::Cancelling(CancellingState {
                cancellation,
                running: Some(running),
            }) => Some(AgentSupervisorCancellationTarget {
                node: row.node,
                attempt: running.attempt,
                cancellation,
            }),
            NodeState::Queued
            | NodeState::Running(_)
            | NodeState::Waiting(_)
            | NodeState::Cancelling(_)
            | NodeState::Succeeded
            | NodeState::Failed(_)
            | NodeState::Cancelled(_) => None,
        })
    }

    /// Activates one pre-approved direct child while its exact parent executes.
    pub fn delegate(
        &mut self,
        parent: &AgentNodeExecution,
        child: AgentPlanNodeId,
    ) -> Result<(), AgentSupervisorRuntimeError> {
        self.require_running_execution(parent)?;
        let topology_child = self
            .topology
            .node(child)
            .ok_or(AgentSupervisorRuntimeError::NodeMissing)?;
        if topology_child.parent() != Some(parent.node()) {
            return Err(AgentSupervisorRuntimeError::DelegationMismatch);
        }
        if self.node_index(child).is_some() {
            return Err(AgentSupervisorRuntimeError::AlreadyActivated);
        }
        if self.status().live >= MAX_AGENT_LIVE_SUPERVISOR_NODES {
            return Err(AgentSupervisorRuntimeError::LiveLimit);
        }
        let index = self
            .nodes
            .binary_search_by_key(&child, |row| row.node)
            .unwrap_or_else(|index| index);
        self.nodes.insert(
            index,
            SupervisorNodeRow {
                node: child,
                state: NodeState::Queued,
            },
        );
        Ok(())
    }

    /// Applies one sticky cancellation update to an activated subtree.
    ///
    /// Queued and resource-free waiting nodes are synchronously cancelled once
    /// their activated descendants are terminal. Running attempts remain live
    /// and executing until their original non-cloneable token settles or
    /// explicitly acknowledges terminal drain.
    pub fn cancel_subtree(
        &mut self,
        root: AgentPlanNodeId,
        id: AgentSupervisorCancellationId,
        reason: AgentSupervisorCancellationReason,
    ) -> Result<AgentSupervisorCancellationBatch, AgentSupervisorRuntimeError> {
        if self.sealed {
            return Err(AgentSupervisorRuntimeError::Sealed);
        }
        if self.last_cancellation.is_some_and(|last| id <= last) {
            return Err(AgentSupervisorRuntimeError::CancellationReplay);
        }
        let root_index = self
            .node_index(root)
            .ok_or(AgentSupervisorRuntimeError::NodeMissing)?;
        match self.nodes[root_index].state {
            NodeState::Queued | NodeState::Running(_) | NodeState::Waiting(_) => {}
            NodeState::Cancelling(_) => {
                return Err(AgentSupervisorRuntimeError::CancellationPending);
            }
            NodeState::Succeeded | NodeState::Failed(_) | NodeState::Cancelled(_) => {
                return Err(AgentSupervisorRuntimeError::NotLive);
            }
        }

        let cancellation = AgentSupervisorCancellation { id, reason };
        let terminal_before = self.status().cancelled;
        let mut affected = 0;
        for index in 0..self.nodes.len() {
            let node = self.nodes[index].node;
            if node != root && !topology_descends_from(&self.topology, node, root) {
                continue;
            }
            match self.nodes[index].state {
                NodeState::Queued | NodeState::Waiting(_) => {
                    self.nodes[index].state = NodeState::Cancelling(CancellingState {
                        cancellation,
                        running: None,
                    });
                    affected += 1;
                }
                NodeState::Running(running) => {
                    self.nodes[index].state = NodeState::Cancelling(CancellingState {
                        cancellation,
                        running: Some(running),
                    });
                    affected += 1;
                }
                NodeState::Cancelling(_)
                | NodeState::Succeeded
                | NodeState::Failed(_)
                | NodeState::Cancelled(_) => {}
            }
        }
        debug_assert!(affected > 0);
        self.last_cancellation = Some(id);
        self.finalize_drained_cancellations();
        let mut targets = Vec::with_capacity(self.status().executing);
        targets.extend(self.cancellation_targets().filter(|target| {
            target.node() == root || topology_descends_from(&self.topology, target.node(), root)
        }));
        debug_assert!(targets.len() <= MAX_AGENT_EXECUTING_SUPERVISOR_NODES);
        let terminal = self.status().cancelled.saturating_sub(terminal_before);
        Ok(AgentSupervisorCancellationBatch {
            supervisor: self.id,
            root,
            cancellation,
            affected,
            terminal,
            targets,
        })
    }

    /// Admits one queued/yielded node into an executing-agent slot.
    pub fn start(
        &mut self,
        node: AgentPlanNodeId,
        attempt: AgentSupervisorAttemptId,
    ) -> Result<AgentNodeExecution, AgentSupervisorRuntimeError> {
        if self.sealed {
            return Err(AgentSupervisorRuntimeError::Sealed);
        }
        if self.last_attempt.is_some_and(|last| attempt <= last) {
            return Err(AgentSupervisorRuntimeError::AttemptReplay);
        }
        let index = self
            .node_index(node)
            .ok_or(AgentSupervisorRuntimeError::NodeMissing)?;
        match self.nodes[index].state {
            NodeState::Queued | NodeState::Waiting(AgentSupervisorWait::Yielded) => {}
            NodeState::Waiting(AgentSupervisorWait::Descendants) => {
                if self.has_live_descendant(node) {
                    return Err(AgentSupervisorRuntimeError::DescendantsLive);
                }
            }
            NodeState::Running(_)
            | NodeState::Cancelling(_)
            | NodeState::Succeeded
            | NodeState::Failed(_)
            | NodeState::Cancelled(_) => {
                return Err(AgentSupervisorRuntimeError::NotRunnable);
            }
        }
        if self.status().executing >= MAX_AGENT_EXECUTING_SUPERVISOR_NODES {
            return Err(AgentSupervisorRuntimeError::ExecutionLimit);
        }
        let guard = execution_guard(self.id, self.topology.guard(), node, attempt);
        self.nodes[index].state = NodeState::Running(RunningState { attempt, guard });
        self.last_attempt = Some(attempt);
        Ok(AgentNodeExecution {
            supervisor: self.id,
            node,
            attempt,
            guard,
        })
    }

    /// Releases one execution slot into a content-free nonterminal wait state.
    pub fn wait(
        &mut self,
        execution: AgentNodeExecution,
        reason: AgentSupervisorWait,
    ) -> Result<AgentSupervisorExecutionReceipt, AgentSupervisorRuntimeError> {
        let (index, cancellation) = self.require_execution(&execution)?;
        if let Some(cancellation) = cancellation {
            return Ok(self.settle_cancelled_execution(index, execution, cancellation));
        }
        self.nodes[index].state = NodeState::Waiting(reason);
        Ok(execution_receipt(
            execution,
            AgentSupervisorExecutionOutcome::Waiting(reason),
        ))
    }

    /// Settles one exact execution, or safely waits when descendants remain.
    pub fn complete(
        &mut self,
        execution: AgentNodeExecution,
        completion: AgentSupervisorCompletion,
    ) -> Result<AgentSupervisorExecutionReceipt, AgentSupervisorRuntimeError> {
        let (index, cancellation) = self.require_execution(&execution)?;
        if let Some(cancellation) = cancellation {
            return Ok(self.settle_cancelled_execution(index, execution, cancellation));
        }
        if self.has_live_descendant(execution.node()) {
            self.nodes[index].state = NodeState::Waiting(AgentSupervisorWait::Descendants);
            return Ok(execution_receipt(
                execution,
                AgentSupervisorExecutionOutcome::Waiting(AgentSupervisorWait::Descendants),
            ));
        }
        let outcome = match completion {
            AgentSupervisorCompletion::Succeeded => {
                self.nodes[index].state = NodeState::Succeeded;
                AgentSupervisorExecutionOutcome::Succeeded
            }
            AgentSupervisorCompletion::Failed(failure) => {
                self.nodes[index].state = NodeState::Failed(failure);
                AgentSupervisorExecutionOutcome::Failed(failure)
            }
        };
        Ok(execution_receipt(execution, outcome))
    }

    /// Acknowledges that one cancellation target and all work it owned drained.
    ///
    /// A wrong cancellation identity is ambiguous: mutation seals and the
    /// executing slot remains retained rather than risking oversubscription.
    pub fn drain_cancelled(
        &mut self,
        execution: AgentNodeExecution,
        cancellation: AgentSupervisorCancellationId,
    ) -> Result<AgentSupervisorExecutionReceipt, AgentSupervisorRuntimeError> {
        let (index, pending) = self.require_execution(&execution)?;
        let Some(pending) = pending else {
            self.sealed = true;
            return Err(AgentSupervisorRuntimeError::CancellationMismatch);
        };
        if pending.id() != cancellation {
            self.sealed = true;
            return Err(AgentSupervisorRuntimeError::CancellationMismatch);
        }
        Ok(self.settle_cancelled_execution(index, execution, pending))
    }

    fn require_execution(
        &mut self,
        execution: &AgentNodeExecution,
    ) -> Result<(usize, Option<AgentSupervisorCancellation>), AgentSupervisorRuntimeError> {
        if self.sealed {
            return Err(AgentSupervisorRuntimeError::Sealed);
        }
        let Some(index) = self.node_index(execution.node) else {
            self.sealed = true;
            return Err(AgentSupervisorRuntimeError::ExecutionMismatch);
        };
        let (running, cancellation) = match self.nodes[index].state {
            NodeState::Running(running) => (Some(running), None),
            NodeState::Cancelling(cancelling) => {
                (cancelling.running, Some(cancelling.cancellation))
            }
            NodeState::Queued
            | NodeState::Waiting(_)
            | NodeState::Succeeded
            | NodeState::Failed(_)
            | NodeState::Cancelled(_) => (None, None),
        };
        let matches = execution.supervisor == self.id
            && running.is_some_and(|running| {
                running.attempt == execution.attempt && running.guard == execution.guard
            });
        if !matches {
            self.sealed = true;
            return Err(AgentSupervisorRuntimeError::ExecutionMismatch);
        }
        Ok((index, cancellation))
    }

    fn require_running_execution(
        &mut self,
        execution: &AgentNodeExecution,
    ) -> Result<usize, AgentSupervisorRuntimeError> {
        let (index, cancellation) = self.require_execution(execution)?;
        if cancellation.is_some() {
            return Err(AgentSupervisorRuntimeError::CancellationPending);
        }
        Ok(index)
    }

    fn settle_cancelled_execution(
        &mut self,
        index: usize,
        execution: AgentNodeExecution,
        cancellation: AgentSupervisorCancellation,
    ) -> AgentSupervisorExecutionReceipt {
        self.nodes[index].state = NodeState::Cancelling(CancellingState {
            cancellation,
            running: None,
        });
        self.finalize_drained_cancellations();
        execution_receipt(
            execution,
            AgentSupervisorExecutionOutcome::Cancelled(cancellation),
        )
    }

    fn finalize_drained_cancellations(&mut self) {
        loop {
            let candidate = self.nodes.iter().position(|row| {
                matches!(
                    row.state,
                    NodeState::Cancelling(CancellingState { running: None, .. })
                ) && !self.has_live_descendant(row.node)
            });
            let Some(index) = candidate else {
                return;
            };
            let NodeState::Cancelling(cancelling) = self.nodes[index].state else {
                unreachable!("candidate was proven cancelling");
            };
            self.nodes[index].state = NodeState::Cancelled(cancelling.cancellation);
        }
    }

    fn node_index(&self, node: AgentPlanNodeId) -> Option<usize> {
        self.nodes.binary_search_by_key(&node, |row| row.node).ok()
    }

    fn has_live_descendant(&self, ancestor: AgentPlanNodeId) -> bool {
        self.nodes.iter().any(|row| {
            row.node != ancestor
                && row.state.public().is_live()
                && topology_descends_from(&self.topology, row.node, ancestor)
        })
    }
}

impl fmt::Debug for AgentRunSupervisor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunSupervisor")
            .field("id", &self.id)
            .field("manifest", &self.topology.manifest())
            .field("status", &self.status())
            .field("last_attempt", &self.last_attempt)
            .field("last_cancellation", &self.last_cancellation)
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed mutable-supervisor refusal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentSupervisorRuntimeError {
    /// Token ambiguity terminally sealed mutation.
    #[error("agent supervisor is sealed")]
    Sealed,
    /// Node was absent from the topology or not yet activated.
    #[error("agent supervisor node is missing")]
    NodeMissing,
    /// Requested child did not name the exact executing direct parent.
    #[error("agent supervisor delegation parent mismatched")]
    DelegationMismatch,
    /// Node identity had already been activated and cannot reopen.
    #[error("agent supervisor node was already activated")]
    AlreadyActivated,
    /// Eight live nodes already consume the initial tree ceiling.
    #[error("agent supervisor live-node ceiling reached")]
    LiveLimit,
    /// Four exact execution tokens already consume the execution ceiling.
    #[error("agent supervisor execution ceiling reached")]
    ExecutionLimit,
    /// Execution attempt identity was reused or regressed.
    #[error("agent supervisor execution attempt replay")]
    AttemptReplay,
    /// Cancellation identity was reused or regressed.
    #[error("agent supervisor cancellation replay")]
    CancellationReplay,
    /// The requested cancellation root is already draining.
    #[error("agent supervisor cancellation is already pending")]
    CancellationPending,
    /// The requested cancellation root is already terminal.
    #[error("agent supervisor node is not live")]
    NotLive,
    /// Node is running or terminal and cannot start another turn.
    #[error("agent supervisor node is not runnable")]
    NotRunnable,
    /// Parent cannot settle/resume while an activated descendant is live.
    #[error("agent supervisor descendant is still live")]
    DescendantsLive,
    /// Non-cloneable execution identity/state/guard did not exactly match.
    #[error("agent supervisor execution token mismatched")]
    ExecutionMismatch,
    /// Drain acknowledgement did not name the exact pending cancellation.
    #[error("agent supervisor cancellation drain mismatched")]
    CancellationMismatch,
}

fn topology_descends_from(
    topology: &AgentDelegationTopology,
    node: AgentPlanNodeId,
    ancestor: AgentPlanNodeId,
) -> bool {
    let mut current = topology.node(node);
    while let Some(value) = current {
        let Some(parent) = value.parent() else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        current = topology.node(parent);
    }
    false
}

fn execution_guard(
    supervisor: AgentSupervisorId,
    topology_guard: [u8; 32],
    node: AgentPlanNodeId,
    attempt: AgentSupervisorAttemptId,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-AGENT-SUPERVISOR-EXECUTION-1\0");
    hasher.update(supervisor.get().to_be_bytes());
    hasher.update(topology_guard);
    hasher.update(node.bytes());
    hasher.update(attempt.get().to_be_bytes());
    hasher.finalize().into()
}

fn execution_receipt(
    execution: AgentNodeExecution,
    outcome: AgentSupervisorExecutionOutcome,
) -> AgentSupervisorExecutionReceipt {
    AgentSupervisorExecutionReceipt {
        supervisor: execution.supervisor,
        node: execution.node,
        attempt: execution.attempt,
        outcome,
    }
}
