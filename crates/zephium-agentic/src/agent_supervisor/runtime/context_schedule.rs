//! Exact supervisor ownership over the existing bounded context registry.
//!
//! This layer reserves no native resource itself. It joins an existing
//! `ContextRegistry` row to one exact running manifest node, then keeps that
//! node live until registry-proven queued cancellation or terminal reaping.

use super::*;
use crate::{
    AgentRunManifest, ContextCapabilities, ContextId, ContextIdentity, ContextRegistry,
    ContextResourceDisposition, ContextTerminal,
};

/// One browser context assigned to an exact activated plan node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentSupervisorContextAssignment {
    supervisor: AgentSupervisorId,
    node: AgentPlanNodeId,
    identity: ContextIdentity,
}

impl AgentSupervisorContextAssignment {
    /// Exact mutable supervisor incarnation owning the assignment.
    pub const fn supervisor(self) -> AgentSupervisorId {
        self.supervisor
    }

    /// Exact plan node responsible for the context.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }

    /// Immutable context/run/profile/kind identity.
    pub const fn identity(self) -> ContextIdentity {
        self.identity
    }
}

/// Assigned context reached by one sticky cancellation-tree update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentSupervisorContextCancellationTarget {
    assignment: AgentSupervisorContextAssignment,
    cancellation: AgentSupervisorCancellation,
}

impl AgentSupervisorContextCancellationTarget {
    /// Exact node/context ownership that must be cleaned up.
    pub const fn assignment(self) -> AgentSupervisorContextAssignment {
        self.assignment
    }

    /// Exact cancellation update affecting the owning node.
    pub const fn cancellation(self) -> AgentSupervisorCancellation {
        self.cancellation
    }
}

/// Proven registry disposition that released one supervisor context budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentSupervisorContextReleaseOutcome {
    /// A queued logical row was cancelled before native construction.
    QueuedCancelled,
    /// An active row reached one exact terminal native-resource disposition.
    Retired {
        /// Exact terminal lifecycle result.
        terminal: ContextTerminal,
        /// Proven physical resource disposition.
        resource: ContextResourceDisposition,
    },
}

/// Content-free receipt after one exact assignment leaves the supervisor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentSupervisorContextRelease {
    assignment: AgentSupervisorContextAssignment,
    outcome: AgentSupervisorContextReleaseOutcome,
}

impl AgentSupervisorContextRelease {
    /// Exact released node/context ownership.
    pub const fn assignment(self) -> AgentSupervisorContextAssignment {
        self.assignment
    }

    /// Registry-proven queued or terminal disposition.
    pub const fn outcome(self) -> AgentSupervisorContextReleaseOutcome {
        self.outcome
    }

    #[cfg(test)]
    pub(crate) fn for_profile_lease_test(
        identity: ContextIdentity,
        outcome: AgentSupervisorContextReleaseOutcome,
    ) -> Self {
        Self {
            assignment: AgentSupervisorContextAssignment {
                supervisor: AgentSupervisorId::new(1).expect("test supervisor id"),
                node: AgentPlanNodeId::from_raw(1),
                identity,
            },
            outcome,
        }
    }
}

pub(super) struct SupervisorContextRow {
    assignment: AgentSupervisorContextAssignment,
}

impl AgentRunSupervisor {
    /// Current assignments in canonical `ContextId` order without allocation.
    pub fn context_assignments(
        &self,
    ) -> impl ExactSizeIterator<Item = AgentSupervisorContextAssignment> + '_ {
        self.contexts.iter().map(|row| row.assignment)
    }

    /// Current context cleanup targets for cancellation-signal reconciliation.
    pub fn context_cancellation_targets(
        &self,
    ) -> impl Iterator<Item = AgentSupervisorContextCancellationTarget> + '_ {
        self.contexts.iter().filter_map(|row| {
            let AgentSupervisorNodeStatus::Cancelling(node) =
                self.node_status(row.assignment.node())?
            else {
                return None;
            };
            Some(AgentSupervisorContextCancellationTarget {
                assignment: row.assignment,
                cancellation: node.cancellation(),
            })
        })
    }

    /// Atomically reserves a registry row and assigns it to one running node.
    ///
    /// The exact manifest is rejoined on every admission. The context must
    /// belong to the same run, use a profile allowed by the node, and fit both
    /// run-global and node-local context ceilings before the registry mutates.
    pub fn reserve_context(
        &mut self,
        execution: &AgentNodeExecution,
        manifest: &AgentRunManifest,
        registry: &mut ContextRegistry,
        identity: ContextIdentity,
        capabilities: ContextCapabilities,
    ) -> Result<AgentSupervisorContextAssignment, AgentSupervisorRuntimeError> {
        let node_index = self.require_running_execution(execution)?;
        if !self.topology.matches_manifest(manifest) {
            return Err(AgentSupervisorRuntimeError::ManifestMismatch);
        }
        if identity.owner() != self.topology.run() {
            return Err(AgentSupervisorRuntimeError::ContextAuthority);
        }
        let node = manifest
            .plan_node(execution.node())
            .ok_or(AgentSupervisorRuntimeError::NodeMissing)?;
        if node.profiles().binary_search(&identity.profile()).is_err() {
            return Err(AgentSupervisorRuntimeError::ContextAuthority);
        }
        if self.context_index(identity.id()).is_some() {
            return Err(AgentSupervisorRuntimeError::ContextDuplicate);
        }
        if self.contexts.len() >= usize::from(manifest.budget().contexts()) {
            return Err(AgentSupervisorRuntimeError::RunContextLimit);
        }
        if self.context_count_for_node(execution.node()) >= usize::from(node.budget().contexts()) {
            return Err(AgentSupervisorRuntimeError::NodeContextLimit);
        }
        let progress_activity = AgentProgressActivity::try_new(
            AgentProgressOperation::Context,
            Some(AgentProgressResource::Context(identity.id())),
        )?;
        self.contexts
            .try_reserve_exact(1)
            .map_err(|_| AgentSupervisorRuntimeError::ContextCapacity)?;
        registry.reserve(identity, capabilities)?;
        let assignment = AgentSupervisorContextAssignment {
            supervisor: self.id,
            node: execution.node(),
            identity,
        };
        let index = self
            .contexts
            .binary_search_by_key(&identity.id(), |row| row.assignment.identity().id())
            .unwrap_or_else(|index| index);
        self.contexts
            .insert(index, SupervisorContextRow { assignment });
        self.nodes[node_index].progress = self.progress(
            execution.node(),
            progress_activity,
            AgentProgressState::Active,
            None,
            None,
        );
        Ok(assignment)
    }

    /// Cancels one never-started registry row and releases its exact budgets.
    ///
    /// Cleanup remains available after a fail-stop seal because it can only
    /// remove an already-bound row after the registry returns matching proof.
    pub fn cancel_queued_context(
        &mut self,
        registry: &mut ContextRegistry,
        context: ContextId,
    ) -> Result<AgentSupervisorContextRelease, AgentSupervisorRuntimeError> {
        let index = self
            .context_index(context)
            .ok_or(AgentSupervisorRuntimeError::ContextNotAssigned)?;
        let assignment = self.contexts[index].assignment;
        let progress_activity = AgentProgressActivity::try_new(
            AgentProgressOperation::Context,
            Some(AgentProgressResource::Context(context)),
        )?;
        let identity = registry.cancel_queued(context)?;
        if identity != assignment.identity() {
            self.sealed = true;
            return Err(AgentSupervisorRuntimeError::ContextIdentityMismatch);
        }
        self.contexts.remove(index);
        if let Some(node_index) = self.node_index(assignment.node()) {
            self.nodes[node_index].progress = self.progress(
                assignment.node(),
                progress_activity,
                AgentProgressState::Cancelled,
                Some(AgentProgressResult::Context(
                    AgentSupervisorContextReleaseOutcome::QueuedCancelled,
                )),
                Some(AgentProgressBlocker::ContextCancelled),
            );
        }
        self.finalize_drained_cancellations();
        Ok(AgentSupervisorContextRelease {
            assignment,
            outcome: AgentSupervisorContextReleaseOutcome::QueuedCancelled,
        })
    }

    /// Reaps one exact terminal registry row and releases its context budgets.
    ///
    /// The registry itself proves the native resource was destroyed,
    /// transferred, or retained according to context kind before this method
    /// removes supervisor ownership.
    pub fn reap_terminal_context(
        &mut self,
        registry: &mut ContextRegistry,
        context: ContextId,
    ) -> Result<AgentSupervisorContextRelease, AgentSupervisorRuntimeError> {
        let index = self
            .context_index(context)
            .ok_or(AgentSupervisorRuntimeError::ContextNotAssigned)?;
        let assignment = self.contexts[index].assignment;
        let progress_activity = AgentProgressActivity::try_new(
            AgentProgressOperation::Context,
            Some(AgentProgressResource::Context(context)),
        )?;
        let retired = registry.reap_terminal(context)?;
        if retired.identity() != assignment.identity() {
            self.sealed = true;
            return Err(AgentSupervisorRuntimeError::ContextIdentityMismatch);
        }
        self.contexts.remove(index);
        let outcome = AgentSupervisorContextReleaseOutcome::Retired {
            terminal: retired.terminal(),
            resource: retired.resource(),
        };
        if let Some(node_index) = self.node_index(assignment.node()) {
            self.nodes[node_index].progress = self.progress(
                assignment.node(),
                progress_activity,
                AgentProgressState::Succeeded,
                Some(AgentProgressResult::Context(outcome)),
                None,
            );
        }
        self.finalize_drained_cancellations();
        Ok(AgentSupervisorContextRelease {
            assignment,
            outcome,
        })
    }

    pub(super) fn context_cancellation_targets_for(
        &self,
        root: AgentPlanNodeId,
    ) -> Vec<AgentSupervisorContextCancellationTarget> {
        let mut targets = Vec::with_capacity(self.contexts.len());
        targets.extend(self.context_cancellation_targets().filter(|target| {
            let node = target.assignment().node();
            node == root || topology_descends_from(&self.topology, node, root)
        }));
        targets
    }

    pub(super) fn has_assigned_context(&self, node: AgentPlanNodeId) -> bool {
        self.contexts
            .iter()
            .any(|row| row.assignment.node() == node)
    }

    pub(super) fn owns_context_assignment(
        &self,
        node: AgentPlanNodeId,
        identity: ContextIdentity,
    ) -> bool {
        self.contexts.iter().any(|row| {
            row.assignment.supervisor() == self.id
                && row.assignment.node() == node
                && row.assignment.identity() == identity
        })
    }

    fn context_count_for_node(&self, node: AgentPlanNodeId) -> usize {
        self.contexts
            .iter()
            .filter(|row| row.assignment.node() == node)
            .count()
    }

    fn context_index(&self, context: ContextId) -> Option<usize> {
        self.contexts
            .binary_search_by_key(&context, |row| row.assignment.identity().id())
            .ok()
    }
}
