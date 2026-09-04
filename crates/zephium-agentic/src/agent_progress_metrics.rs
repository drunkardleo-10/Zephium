//! Run-scoped progress and duration metrics over canonical semantic audit events.
//!
//! This optional streaming functional core owns no telemetry port, persistence,
//! clock, task, worker, channel, browser context, or native resource. A trusted
//! shell may construct it for an active run and explicitly feed canonical audit
//! events. It retains only opaque identities, closed enum counts, and bounded
//! monotonic duration aggregates. Missing duration evidence remains absent.

use std::fmt;

use thiserror::Error;

use crate::{
    AgentAuditEvent, AgentAuditEventId, AgentEffectId, AgentEffectSettlement, AgentModelCallId,
    AgentModelCallSettlement, AgentNeedsHumanReason, AgentPlanNodeId, AgentPolicyInstant,
    AgentProgressBlocker, AgentProgressOperation, AgentProgressResource, AgentProgressResult,
    AgentProgressState, AgentRunManifest, AgentRunManifestId, AgentRunSupervisor,
    AgentSemanticProgress, AgentSupervisorCancellationId, AgentSupervisorCancellationReason,
    AgentSupervisorContextReleaseOutcome, AgentSupervisorExecutionOutcome, AgentSupervisorFailure,
    AgentSupervisorId, AgentSupervisorNodeStatus, AgentSupervisorWait, MAX_AGENT_PENDING_EFFECTS,
    MAX_AGENT_PENDING_MODEL_CALLS, MAX_AGENT_PLAN_NODES,
};

/// Aggregate over one or more exact observed monotonic durations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentDurationMetrics {
    samples: u32,
    total_millis: u64,
    max_millis: u64,
}

impl AgentDurationMetrics {
    /// Exact number of observed start-to-terminal samples.
    pub const fn samples(self) -> u32 {
        self.samples
    }

    /// Checked sum of all observed durations.
    pub const fn total_millis(self) -> u64 {
        self.total_millis
    }

    /// Largest exact observed duration.
    pub const fn max_millis(self) -> u64 {
        self.max_millis
    }

    const fn empty() -> Self {
        Self {
            samples: 0,
            total_millis: 0,
            max_millis: 0,
        }
    }

    fn checked_record(self, millis: u64) -> Result<Self, AgentProgressMetricError> {
        Ok(Self {
            samples: self
                .samples
                .checked_add(1)
                .ok_or(AgentProgressMetricError::Overflow)?,
            total_millis: self
                .total_millis
                .checked_add(millis)
                .ok_or(AgentProgressMetricError::Overflow)?,
            max_millis: self.max_millis.max(millis),
        })
    }

    const fn observed(self) -> Option<Self> {
        if self.samples == 0 {
            None
        } else {
            Some(self)
        }
    }
}

/// Exact closed counts of policy-derived human pauses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentNeedsHumanMetrics {
    total: u32,
    reasons: [u32; 5],
}

impl AgentNeedsHumanMetrics {
    /// Total canonical `NeedsHuman` transitions observed in the run.
    pub const fn total(self) -> u32 {
        self.total
    }

    /// Exact transition count for one closed policy reason.
    pub const fn reason(self, reason: AgentNeedsHumanReason) -> u32 {
        self.reasons[needs_human_reason_index(reason)]
    }

    const fn empty() -> Self {
        Self {
            total: 0,
            reasons: [0; 5],
        }
    }

    fn checked_record(
        self,
        reason: AgentNeedsHumanReason,
    ) -> Result<Self, AgentProgressMetricError> {
        let mut next = self;
        next.total = next
            .total
            .checked_add(1)
            .ok_or(AgentProgressMetricError::Overflow)?;
        let index = needs_human_reason_index(reason);
        next.reasons[index] = next.reasons[index]
            .checked_add(1)
            .ok_or(AgentProgressMetricError::Overflow)?;
        Ok(next)
    }
}

/// Exact terminal outcome of the root responsibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentRunProgressOutcome {
    /// The root and every activated descendant succeeded.
    Succeeded,
    /// The root failed under one closed supervisor failure.
    Failed(AgentSupervisorFailure),
    /// The root was cancelled under one closed reason.
    Cancelled(AgentSupervisorCancellationReason),
}

/// Immutable content-free aggregate view of one audit stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentRunProgressSnapshot {
    manifest: AgentRunManifestId,
    supervisor: AgentSupervisorId,
    events: u64,
    activated_nodes: u32,
    terminal_nodes: u32,
    needs_human: AgentNeedsHumanMetrics,
    human_takeovers: u32,
    queue_wait: AgentDurationMetrics,
    model: AgentDurationMetrics,
    effect: AgentDurationMetrics,
    human_wait: AgentDurationMetrics,
    outcome: Option<AgentRunProgressOutcome>,
    total_elapsed_millis: Option<u64>,
}

impl AgentRunProgressSnapshot {
    /// Exact immutable manifest identity for this aggregate.
    pub const fn manifest(self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact mutable supervisor incarnation for this aggregate.
    pub const fn supervisor(self) -> AgentSupervisorId {
        self.supervisor
    }

    /// Number of strictly ordered canonical audit events admitted.
    pub const fn events(self) -> u64 {
        self.events
    }

    /// Number of topology nodes whose initial queued event was observed.
    pub const fn activated_nodes(self) -> u32 {
        self.activated_nodes
    }

    /// Number of observed terminal supervisor-node events.
    pub const fn terminal_nodes(self) -> u32 {
        self.terminal_nodes
    }

    /// Exact policy-derived human-pause counts.
    pub const fn needs_human(self) -> AgentNeedsHumanMetrics {
        self.needs_human
    }

    /// Distinct human-takeover cancellation updates observed at terminal nodes.
    pub const fn human_takeovers(self) -> u32 {
        self.human_takeovers
    }

    /// Observed initial activation queue durations, or `None` when unmeasured.
    pub const fn queue_wait(self) -> Option<AgentDurationMetrics> {
        self.queue_wait.observed()
    }

    /// Observed model active-to-terminal durations, or `None` when unmeasured.
    pub const fn model(self) -> Option<AgentDurationMetrics> {
        self.model.observed()
    }

    /// Observed effect active-to-terminal durations, or `None` when unmeasured.
    pub const fn effect(self) -> Option<AgentDurationMetrics> {
        self.effect.observed()
    }

    /// Observed `NeedsHuman` wait durations, or `None` while absent or open.
    pub const fn human_wait(self) -> Option<AgentDurationMetrics> {
        self.human_wait.observed()
    }

    /// Exact root terminal outcome, absent until its terminal event is observed.
    pub const fn outcome(self) -> Option<AgentRunProgressOutcome> {
        self.outcome
    }

    /// Root queued-to-terminal elapsed time, absent until both events exist.
    pub const fn total_elapsed_millis(self) -> Option<u64> {
        self.total_elapsed_millis
    }
}

#[derive(Clone, Copy)]
struct NodeProgressMetricRow {
    node: AgentPlanNodeId,
    activated: bool,
    terminal: bool,
    last_progress: Option<AgentSemanticProgress>,
    queued_at: Option<AgentPolicyInstant>,
    human_wait_at: Option<AgentPolicyInstant>,
}

#[derive(Clone, Copy)]
struct ActiveModelMetric {
    id: AgentModelCallId,
    node: AgentPlanNodeId,
    started_at: AgentPolicyInstant,
}

#[derive(Clone, Copy)]
struct ActiveEffectMetric {
    id: AgentEffectId,
    node: AgentPlanNodeId,
    started_at: AgentPolicyInstant,
}

#[derive(Clone, Copy)]
enum ProjectionKind {
    Queued,
    SchedulingActive,
    ModelActive(AgentModelCallId),
    ModelTerminal(AgentModelCallId),
    EffectActive(AgentEffectId),
    EffectTerminal(AgentEffectId),
    NeedsHuman(AgentNeedsHumanReason),
    SupervisorWait,
    SupervisorTerminal(AgentSupervisorExecutionOutcome),
    Other,
}

#[derive(Clone, Copy)]
enum VectorAction<T> {
    None,
    Insert(usize, T),
    Remove(usize),
}

/// Optional bounded streaming reducer over exact semantic audit events.
#[must_use]
pub struct AgentRunProgressMetrics {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    starts_at: AgentPolicyInstant,
    expires_at: AgentPolicyInstant,
    supervisor: AgentSupervisorId,
    root: AgentPlanNodeId,
    nodes: Vec<NodeProgressMetricRow>,
    active_models: Vec<ActiveModelMetric>,
    active_effects: Vec<ActiveEffectMetric>,
    takeover_cancellations: Vec<AgentSupervisorCancellationId>,
    last_event: Option<AgentAuditEventId>,
    last_recorded_at: Option<AgentPolicyInstant>,
    root_queued_at: Option<AgentPolicyInstant>,
    events: u64,
    activated_nodes: u32,
    terminal_nodes: u32,
    needs_human: AgentNeedsHumanMetrics,
    queue_wait: AgentDurationMetrics,
    model: AgentDurationMetrics,
    effect: AgentDurationMetrics,
    human_wait: AgentDurationMetrics,
    outcome: Option<AgentRunProgressOutcome>,
    total_elapsed_millis: Option<u64>,
}

impl AgentRunProgressMetrics {
    /// Joins one queued supervisor to its exact canonical manifest revision.
    pub fn try_new(
        manifest: &AgentRunManifest,
        supervisor: &AgentRunSupervisor,
    ) -> Result<Self, AgentProgressMetricError> {
        let status = supervisor.status();
        let root = supervisor.topology().root();
        if !supervisor.topology().matches_manifest(manifest)
            || status.activated() != 1
            || status.live() != 1
            || status.queued() != 1
            || status.executing() != 0
            || status.contexts() != 0
            || supervisor.node_status(root) != Some(AgentSupervisorNodeStatus::Queued)
        {
            return Err(AgentProgressMetricError::StartState);
        }

        let topology_nodes = supervisor.topology().nodes();
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(topology_nodes.len())
            .map_err(|_| AgentProgressMetricError::Capacity)?;
        nodes.extend(topology_nodes.iter().map(|node| NodeProgressMetricRow {
            node: node.node(),
            activated: false,
            terminal: false,
            last_progress: None,
            queued_at: None,
            human_wait_at: None,
        }));

        Ok(Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            starts_at: manifest.issued_at(),
            expires_at: manifest.expires_at(),
            supervisor: supervisor.id(),
            root,
            nodes,
            active_models: Vec::new(),
            active_effects: Vec::new(),
            takeover_cancellations: Vec::new(),
            last_event: None,
            last_recorded_at: None,
            root_queued_at: None,
            events: 0,
            activated_nodes: 0,
            terminal_nodes: 0,
            needs_human: AgentNeedsHumanMetrics::empty(),
            queue_wait: AgentDurationMetrics::empty(),
            model: AgentDurationMetrics::empty(),
            effect: AgentDurationMetrics::empty(),
            human_wait: AgentDurationMetrics::empty(),
            outcome: None,
            total_elapsed_millis: None,
        })
    }

    /// Admits one canonical event in strict audit and monotonic-time order.
    ///
    /// All identity, projection, capacity, sequence, and arithmetic checks
    /// complete before logical state changes. A corrected event may therefore
    /// be retried after any refusal.
    pub fn record_event(&mut self, event: AgentAuditEvent) -> Result<(), AgentProgressMetricError> {
        if !event.matches_manifest_revision(self.manifest, self.manifest_guard, self.supervisor) {
            return Err(AgentProgressMetricError::Authority);
        }
        if self.outcome.is_some() {
            return Err(AgentProgressMetricError::RunComplete);
        }
        if self.last_event.is_some_and(|last| event.id() <= last) {
            return Err(AgentProgressMetricError::EventReplay);
        }
        let recorded_at = event.recorded_at();
        if recorded_at < self.starts_at
            || recorded_at > self.expires_at
            || self.last_recorded_at.is_some_and(|last| recorded_at < last)
        {
            return Err(AgentProgressMetricError::EventTime);
        }

        let progress = event.progress();
        let node = progress.responsibility();
        let node_index = self
            .nodes
            .binary_search_by_key(&node, |row| row.node)
            .map_err(|_| AgentProgressMetricError::Authority)?;
        let mut next_node = self.nodes[node_index];
        if next_node.terminal {
            return Err(AgentProgressMetricError::NodeSequence);
        }
        if next_node.last_progress == Some(progress) {
            return Err(AgentProgressMetricError::DuplicateProgress);
        }

        let projection = classify_projection(progress)?;
        if !next_node.activated {
            if !matches!(projection, ProjectionKind::Queued)
                || (self.events == 0 && node != self.root)
            {
                return Err(AgentProgressMetricError::NodeSequence);
            }
        } else if matches!(projection, ProjectionKind::Queued) {
            return Err(AgentProgressMetricError::NodeSequence);
        }

        let next_events = self
            .events
            .checked_add(1)
            .ok_or(AgentProgressMetricError::Overflow)?;
        let mut next_activated_nodes = self.activated_nodes;
        let mut next_terminal_nodes = self.terminal_nodes;
        let mut next_needs_human = self.needs_human;
        let mut next_queue_wait = self.queue_wait;
        let mut next_model = self.model;
        let mut next_effect = self.effect;
        let mut next_human_wait = self.human_wait;
        let mut next_root_queued_at = self.root_queued_at;
        let mut next_outcome = self.outcome;
        let mut next_total_elapsed_millis = self.total_elapsed_millis;
        let mut model_action = VectorAction::None;
        let mut effect_action = VectorAction::None;
        let mut takeover_insert = None;

        if !next_node.activated {
            next_node.activated = true;
            next_node.queued_at = Some(recorded_at);
            next_activated_nodes = next_activated_nodes
                .checked_add(1)
                .ok_or(AgentProgressMetricError::Overflow)?;
            if node == self.root {
                next_root_queued_at = Some(recorded_at);
            }
        }

        if matches!(projection, ProjectionKind::SchedulingActive) {
            if let Some(queued_at) = next_node.queued_at {
                next_queue_wait =
                    next_queue_wait.checked_record(duration_millis(recorded_at, queued_at)?)?;
                next_node.queued_at = None;
            }
        }

        if let Some(human_wait_at) = next_node.human_wait_at {
            next_human_wait =
                next_human_wait.checked_record(duration_millis(recorded_at, human_wait_at)?)?;
            next_node.human_wait_at = None;
        }

        match projection {
            ProjectionKind::ModelActive(id) => {
                let index = self
                    .active_models
                    .binary_search_by_key(&id, |active| active.id)
                    .map_or_else(|index| index, |_| usize::MAX);
                if index == usize::MAX {
                    return Err(AgentProgressMetricError::OperationSequence);
                }
                if self.active_models.len() >= MAX_AGENT_PENDING_MODEL_CALLS {
                    return Err(AgentProgressMetricError::ActiveLimit);
                }
                model_action = VectorAction::Insert(
                    index,
                    ActiveModelMetric {
                        id,
                        node,
                        started_at: recorded_at,
                    },
                );
            }
            ProjectionKind::ModelTerminal(id) => {
                let index = self
                    .active_models
                    .binary_search_by_key(&id, |active| active.id)
                    .map_err(|_| AgentProgressMetricError::OperationSequence)?;
                let active = self.active_models[index];
                if active.node != node {
                    return Err(AgentProgressMetricError::OperationSequence);
                }
                next_model =
                    next_model.checked_record(duration_millis(recorded_at, active.started_at)?)?;
                model_action = VectorAction::Remove(index);
            }
            ProjectionKind::EffectActive(id) => {
                let index = self
                    .active_effects
                    .binary_search_by_key(&id, |active| active.id)
                    .map_or_else(|index| index, |_| usize::MAX);
                if index == usize::MAX {
                    return Err(AgentProgressMetricError::OperationSequence);
                }
                if self.active_effects.len() >= MAX_AGENT_PENDING_EFFECTS {
                    return Err(AgentProgressMetricError::ActiveLimit);
                }
                effect_action = VectorAction::Insert(
                    index,
                    ActiveEffectMetric {
                        id,
                        node,
                        started_at: recorded_at,
                    },
                );
            }
            ProjectionKind::EffectTerminal(id) => {
                let index = self
                    .active_effects
                    .binary_search_by_key(&id, |active| active.id)
                    .map_err(|_| AgentProgressMetricError::OperationSequence)?;
                let active = self.active_effects[index];
                if active.node != node {
                    return Err(AgentProgressMetricError::OperationSequence);
                }
                next_effect =
                    next_effect.checked_record(duration_millis(recorded_at, active.started_at)?)?;
                effect_action = VectorAction::Remove(index);
            }
            ProjectionKind::NeedsHuman(reason) => {
                next_needs_human = next_needs_human.checked_record(reason)?;
                next_node.human_wait_at = Some(recorded_at);
            }
            ProjectionKind::SupervisorTerminal(outcome) => {
                if self.active_models.iter().any(|active| active.node == node)
                    || self.active_effects.iter().any(|active| active.node == node)
                {
                    return Err(AgentProgressMetricError::OperationSequence);
                }
                next_node.terminal = true;
                next_node.queued_at = None;
                next_terminal_nodes = next_terminal_nodes
                    .checked_add(1)
                    .ok_or(AgentProgressMetricError::Overflow)?;
                if let AgentSupervisorExecutionOutcome::Cancelled(cancellation) = outcome {
                    if cancellation.reason() == AgentSupervisorCancellationReason::HumanTakeover {
                        match self
                            .takeover_cancellations
                            .binary_search(&cancellation.id())
                        {
                            Ok(_) => {}
                            Err(index) => takeover_insert = Some((index, cancellation.id())),
                        }
                    }
                }
                if node == self.root {
                    let queued_at =
                        next_root_queued_at.ok_or(AgentProgressMetricError::NodeSequence)?;
                    next_total_elapsed_millis = Some(duration_millis(recorded_at, queued_at)?);
                    next_outcome = Some(run_outcome(outcome)?);
                }
            }
            ProjectionKind::Queued
            | ProjectionKind::SchedulingActive
            | ProjectionKind::SupervisorWait
            | ProjectionKind::Other => {}
        }

        if matches!(model_action, VectorAction::Insert(_, _)) {
            self.active_models
                .try_reserve_exact(1)
                .map_err(|_| AgentProgressMetricError::Capacity)?;
        }
        if matches!(effect_action, VectorAction::Insert(_, _)) {
            self.active_effects
                .try_reserve_exact(1)
                .map_err(|_| AgentProgressMetricError::Capacity)?;
        }
        if takeover_insert.is_some() {
            if self.takeover_cancellations.len() >= MAX_AGENT_PLAN_NODES {
                return Err(AgentProgressMetricError::ActiveLimit);
            }
            self.takeover_cancellations
                .try_reserve_exact(1)
                .map_err(|_| AgentProgressMetricError::Capacity)?;
        }

        match model_action {
            VectorAction::None => {}
            VectorAction::Insert(index, active) => self.active_models.insert(index, active),
            VectorAction::Remove(index) => {
                self.active_models.remove(index);
            }
        }
        match effect_action {
            VectorAction::None => {}
            VectorAction::Insert(index, active) => self.active_effects.insert(index, active),
            VectorAction::Remove(index) => {
                self.active_effects.remove(index);
            }
        }
        if let Some((index, cancellation)) = takeover_insert {
            self.takeover_cancellations.insert(index, cancellation);
        }

        next_node.last_progress = Some(progress);
        self.nodes[node_index] = next_node;
        self.last_event = Some(event.id());
        self.last_recorded_at = Some(recorded_at);
        self.root_queued_at = next_root_queued_at;
        self.events = next_events;
        self.activated_nodes = next_activated_nodes;
        self.terminal_nodes = next_terminal_nodes;
        self.needs_human = next_needs_human;
        self.queue_wait = next_queue_wait;
        self.model = next_model;
        self.effect = next_effect;
        self.human_wait = next_human_wait;
        self.outcome = next_outcome;
        self.total_elapsed_millis = next_total_elapsed_millis;
        Ok(())
    }

    /// Returns one allocation-free immutable aggregate snapshot.
    pub const fn snapshot(&self) -> AgentRunProgressSnapshot {
        AgentRunProgressSnapshot {
            manifest: self.manifest,
            supervisor: self.supervisor,
            events: self.events,
            activated_nodes: self.activated_nodes,
            terminal_nodes: self.terminal_nodes,
            needs_human: self.needs_human,
            human_takeovers: self.takeover_cancellations.len() as u32,
            queue_wait: self.queue_wait,
            model: self.model,
            effect: self.effect,
            human_wait: self.human_wait,
            outcome: self.outcome,
            total_elapsed_millis: self.total_elapsed_millis,
        }
    }

    pub(crate) fn matches_metric_scope(
        &self,
        manifest: &AgentRunManifest,
        supervisor: AgentSupervisorId,
    ) -> bool {
        self.manifest == manifest.id()
            && self.manifest_guard == manifest.guard()
            && self.supervisor == supervisor
    }
}

impl fmt::Debug for AgentRunProgressMetrics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunProgressMetrics")
            .field("manifest", &self.manifest)
            .field("supervisor", &self.supervisor)
            .field("snapshot", &self.snapshot())
            .field("active_models", &self.active_models.len())
            .field("active_effects", &self.active_effects.len())
            .field("manifest_guard", &"[redacted]")
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed refusal from the run-local progress reducer.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProgressMetricError {
    /// Manifest, supervisor, node, or audit-event revision did not match.
    #[error("progress metric authority mismatch")]
    Authority,
    /// The reducer was not joined before the queued root began execution.
    #[error("progress metric reducer requires the exact queued start state")]
    StartState,
    /// Audit event identity was repeated or regressed.
    #[error("progress metric event identity replayed")]
    EventReplay,
    /// Audit event time escaped the manifest lifetime or regressed.
    #[error("progress metric event time is invalid")]
    EventTime,
    /// An unchanged current projection was presented as a new event.
    #[error("progress metric projection is unchanged")]
    DuplicateProgress,
    /// A node was skipped, reopened, or advanced outside its audited sequence.
    #[error("progress metric node sequence is invalid")]
    NodeSequence,
    /// A model/effect terminal event lacked its exact observed active event.
    #[error("progress metric operation sequence is invalid")]
    OperationSequence,
    /// A private progress projection violated its canonical closed shape.
    #[error("progress metric projection shape is invalid")]
    Projection,
    /// An event arrived after the root terminal outcome.
    #[error("progress metric run is already complete")]
    RunComplete,
    /// An explicit active-operation or cancellation ceiling was reached.
    #[error("progress metric active ceiling reached")]
    ActiveLimit,
    /// A bounded allocation could not be reserved before mutation.
    #[error("progress metric bounded allocation failed")]
    Capacity,
    /// Checked event, count, or duration arithmetic overflowed.
    #[error("progress metric arithmetic overflow")]
    Overflow,
}

fn classify_projection(
    progress: AgentSemanticProgress,
) -> Result<ProjectionKind, AgentProgressMetricError> {
    let operation = progress.activity().operation();
    let resource = progress.activity().resource();
    let state = progress.state();
    let result = progress.result();
    let blocker = progress.blocker();

    match result {
        Some(AgentProgressResult::Model(settlement)) => {
            let Some(AgentProgressResource::ModelCall(id)) = resource else {
                return Err(AgentProgressMetricError::Projection);
            };
            if operation != AgentProgressOperation::Model
                || !model_terminal_shape(settlement, state, blocker)
            {
                return Err(AgentProgressMetricError::Projection);
            }
            Ok(ProjectionKind::ModelTerminal(id))
        }
        Some(AgentProgressResult::Effect(settlement)) => {
            let Some(AgentProgressResource::Effect(id)) = resource else {
                return Err(AgentProgressMetricError::Projection);
            };
            if !matches!(operation, AgentProgressOperation::Effect(_))
                || !effect_terminal_shape(settlement, state, blocker)
            {
                return Err(AgentProgressMetricError::Projection);
            }
            Ok(ProjectionKind::EffectTerminal(id))
        }
        Some(AgentProgressResult::Supervisor(AgentSupervisorExecutionOutcome::Waiting(wait))) => {
            if state != AgentProgressState::Waiting {
                return Err(AgentProgressMetricError::Projection);
            }
            match blocker {
                Some(AgentProgressBlocker::NeedsHuman(reason))
                    if wait == AgentSupervisorWait::Yielded
                        && matches!(operation, AgentProgressOperation::Approval(_))
                        && matches!(resource, Some(AgentProgressResource::Context(_))) =>
                {
                    Ok(ProjectionKind::NeedsHuman(reason))
                }
                Some(AgentProgressBlocker::Scheduler(reason)) if reason == wait => {
                    Ok(ProjectionKind::SupervisorWait)
                }
                _ => Err(AgentProgressMetricError::Projection),
            }
        }
        Some(AgentProgressResult::Supervisor(outcome)) => {
            if !supervisor_terminal_shape(outcome, state, blocker) {
                return Err(AgentProgressMetricError::Projection);
            }
            Ok(ProjectionKind::SupervisorTerminal(outcome))
        }
        Some(AgentProgressResult::Context(outcome)) => {
            if operation != AgentProgressOperation::Context
                || !matches!(resource, Some(AgentProgressResource::Context(_)))
                || !context_terminal_shape(outcome, state, blocker)
            {
                return Err(AgentProgressMetricError::Projection);
            }
            Ok(ProjectionKind::Other)
        }
        None => match (operation, resource, state, blocker) {
            (AgentProgressOperation::Scheduling, None, AgentProgressState::Queued, None) => {
                Ok(ProjectionKind::Queued)
            }
            (
                AgentProgressOperation::Scheduling,
                Some(AgentProgressResource::Execution(_)),
                AgentProgressState::Active,
                None,
            ) => Ok(ProjectionKind::SchedulingActive),
            (
                AgentProgressOperation::Model,
                Some(AgentProgressResource::ModelCall(id)),
                AgentProgressState::Active,
                None,
            ) => Ok(ProjectionKind::ModelActive(id)),
            (
                AgentProgressOperation::Effect(_),
                Some(AgentProgressResource::Effect(id)),
                AgentProgressState::Active,
                None,
            ) => Ok(ProjectionKind::EffectActive(id)),
            (_, _, AgentProgressState::Active, None) => Ok(ProjectionKind::Other),
            (_, _, AgentProgressState::Waiting, Some(AgentProgressBlocker::Cancellation(_))) => {
                Ok(ProjectionKind::Other)
            }
            _ => Err(AgentProgressMetricError::Projection),
        },
    }
}

const fn model_terminal_shape(
    settlement: AgentModelCallSettlement,
    state: AgentProgressState,
    blocker: Option<AgentProgressBlocker>,
) -> bool {
    match settlement {
        AgentModelCallSettlement::Completed => {
            matches!(state, AgentProgressState::Succeeded) && blocker.is_none()
        }
        AgentModelCallSettlement::ProviderFailed => {
            matches!(state, AgentProgressState::Failed)
                && matches!(blocker, Some(AgentProgressBlocker::Provider))
        }
        AgentModelCallSettlement::Cancelled => {
            matches!(state, AgentProgressState::Cancelled)
                && matches!(blocker, Some(AgentProgressBlocker::ModelCancelled))
        }
    }
}

fn effect_terminal_shape(
    settlement: AgentEffectSettlement,
    state: AgentProgressState,
    blocker: Option<AgentProgressBlocker>,
) -> bool {
    match settlement {
        AgentEffectSettlement::Verified(_) => {
            matches!(state, AgentProgressState::Succeeded) && blocker.is_none()
        }
        AgentEffectSettlement::Failed(failure) => {
            matches!(state, AgentProgressState::Failed)
                && blocker == Some(AgentProgressBlocker::Effect(failure))
        }
    }
}

fn context_terminal_shape(
    outcome: AgentSupervisorContextReleaseOutcome,
    state: AgentProgressState,
    blocker: Option<AgentProgressBlocker>,
) -> bool {
    match outcome {
        AgentSupervisorContextReleaseOutcome::QueuedCancelled => {
            state == AgentProgressState::Cancelled
                && blocker == Some(AgentProgressBlocker::ContextCancelled)
        }
        AgentSupervisorContextReleaseOutcome::Retired { .. } => {
            state == AgentProgressState::Succeeded && blocker.is_none()
        }
    }
}

fn supervisor_terminal_shape(
    outcome: AgentSupervisorExecutionOutcome,
    state: AgentProgressState,
    blocker: Option<AgentProgressBlocker>,
) -> bool {
    match outcome {
        AgentSupervisorExecutionOutcome::Succeeded => {
            matches!(state, AgentProgressState::Succeeded) && blocker.is_none()
        }
        AgentSupervisorExecutionOutcome::Failed(failure) => {
            matches!(state, AgentProgressState::Failed)
                && blocker == Some(AgentProgressBlocker::Supervisor(failure))
        }
        AgentSupervisorExecutionOutcome::Cancelled(cancellation) => {
            matches!(state, AgentProgressState::Cancelled)
                && blocker == Some(AgentProgressBlocker::Cancellation(cancellation.reason()))
        }
        AgentSupervisorExecutionOutcome::Waiting(_) => false,
    }
}

const fn run_outcome(
    outcome: AgentSupervisorExecutionOutcome,
) -> Result<AgentRunProgressOutcome, AgentProgressMetricError> {
    match outcome {
        AgentSupervisorExecutionOutcome::Succeeded => Ok(AgentRunProgressOutcome::Succeeded),
        AgentSupervisorExecutionOutcome::Failed(failure) => {
            Ok(AgentRunProgressOutcome::Failed(failure))
        }
        AgentSupervisorExecutionOutcome::Cancelled(cancellation) => {
            Ok(AgentRunProgressOutcome::Cancelled(cancellation.reason()))
        }
        AgentSupervisorExecutionOutcome::Waiting(_) => Err(AgentProgressMetricError::Projection),
    }
}

fn duration_millis(
    later: AgentPolicyInstant,
    earlier: AgentPolicyInstant,
) -> Result<u64, AgentProgressMetricError> {
    later
        .millis()
        .checked_sub(earlier.millis())
        .ok_or(AgentProgressMetricError::EventTime)
}

const fn needs_human_reason_index(reason: AgentNeedsHumanReason) -> usize {
    match reason {
        AgentNeedsHumanReason::HumanControl => 0,
        AgentNeedsHumanReason::CapabilityBoundary => 1,
        AgentNeedsHumanReason::ScopeExpansion => 2,
        AgentNeedsHumanReason::DataFlowApproval => 3,
        AgentNeedsHumanReason::CrossOriginWrite => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentAccountScope, AgentActiveEffect, AgentActiveModelCall, AgentAuditLedger,
        AgentDelegationSpec, AgentDelegationTopology, AgentEffectReceipt, AgentEffectScope,
        AgentModelCallReceipt, AgentNeedsHumanTransition, AgentPlanLeaseId, AgentPlanNodeAuthority,
        AgentPlanNodeScope, AgentRunBudget, AgentRunScope, AgentSupervisorAttemptId,
        AgentSupervisorCancellationId, AgentSupervisorCompletion, ContextCapabilities,
        ContextCapability, ContextId, ContextIdentity, ContextKind, ContextOperationId,
        ContextRegistry, ContextRunId, ContextSettlement, SemanticActionAttemptId,
        SemanticEffectClass, SemanticEffectProofKind, SemanticOrigin, SemanticSensitivity,
    };
    use zephium_core::ids::ProfileId;

    fn make_manifest(id: u128, operations: u32, child: bool) -> AgentRunManifest {
        let profile = ProfileId::from(1);
        let origin = SemanticOrigin::parse(
            "https://progress-metrics.example.test/private?credential=hidden",
        )
        .expect("origin");
        let effects = AgentEffectScope::try_new(&[
            SemanticEffectClass::Read,
            SemanticEffectClass::LocalWrite,
        ])
        .expect("effects");
        let budget = AgentRunBudget::try_new(operations, 1_000, 1_000, 1).expect("budget");
        let authority = || {
            AgentPlanNodeAuthority::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin.clone()],
                SemanticSensitivity::Public,
                effects,
            )
            .expect("authority")
        };
        let mut nodes = vec![AgentPlanNodeScope::new(
            AgentPlanNodeId::from_raw(1),
            authority(),
            budget,
            AgentPolicyInstant::from_millis(9_000),
        )];
        if child {
            nodes.push(AgentPlanNodeScope::new(
                AgentPlanNodeId::from_raw(2),
                authority(),
                budget,
                AgentPolicyInstant::from_millis(8_000),
            ));
        }
        AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(id),
            ContextRunId::from_raw(2),
            AgentRunScope::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin],
                SemanticSensitivity::Public,
                effects,
                Vec::new(),
            )
            .expect("scope"),
            budget,
            AgentPolicyInstant::from_millis(100),
            AgentPolicyInstant::from_millis(10_000),
            nodes,
        )
        .expect("manifest")
    }

    fn make_supervisor(manifest: &AgentRunManifest, id: u64, child: bool) -> AgentRunSupervisor {
        let mut specs = vec![AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None)];
        if child {
            specs.push(AgentDelegationSpec::new(
                AgentPlanNodeId::from_raw(2),
                Some(AgentPlanNodeId::from_raw(1)),
            ));
        }
        AgentRunSupervisor::new(
            AgentSupervisorId::new(id).expect("supervisor"),
            AgentDelegationTopology::try_new(manifest, specs).expect("topology"),
        )
    }

    fn event(value: u64) -> AgentAuditEventId {
        AgentAuditEventId::new(value).expect("event")
    }

    fn attempt(value: u64) -> AgentSupervisorAttemptId {
        AgentSupervisorAttemptId::new(value).expect("attempt")
    }

    fn cancellation(value: u64) -> AgentSupervisorCancellationId {
        AgentSupervisorCancellationId::new(value).expect("cancellation")
    }

    fn record(
        ledger: &mut AgentAuditLedger,
        metrics: &mut AgentRunProgressMetrics,
        supervisor: &AgentRunSupervisor,
        node: AgentPlanNodeId,
        id: u64,
        millis: u64,
    ) -> AgentAuditEvent {
        let event = ledger
            .record_current(
                supervisor,
                node,
                event(id),
                AgentPolicyInstant::from_millis(millis),
            )
            .expect("canonical audit event");
        metrics.record_event(event).expect("progress event");
        event
    }

    #[test]
    fn canonical_stream_records_only_observed_queue_model_effect_and_run_durations() {
        let manifest = make_manifest(1, 100, false);
        let root = AgentPlanNodeId::from_raw(1);
        let mut supervisor = make_supervisor(&manifest, 1, false);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        let mut metrics =
            AgentRunProgressMetrics::try_new(&manifest, &supervisor).expect("metrics");

        record(&mut ledger, &mut metrics, &supervisor, root, 1, 100);
        assert_eq!(metrics.snapshot().queue_wait(), None);
        assert_eq!(metrics.snapshot().model(), None);
        assert_eq!(metrics.snapshot().effect(), None);
        assert_eq!(metrics.snapshot().total_elapsed_millis(), None);

        let execution = supervisor.start(root, attempt(1)).expect("start");
        record(&mut ledger, &mut metrics, &supervisor, root, 2, 110);

        let active_model = AgentActiveModelCall::for_progress_test(
            &manifest,
            AgentModelCallId::new(1).expect("model"),
            AgentPlanLeaseId::from_raw(10),
            root,
        );
        supervisor
            .record_active_model_call(
                &execution,
                crate::AgentProviderCallIdentity::from_active(&active_model),
            )
            .expect("active model");
        record(&mut ledger, &mut metrics, &supervisor, root, 3, 120);
        let model_receipt = AgentModelCallReceipt::for_progress_test(
            &manifest,
            AgentModelCallId::new(1).expect("model"),
            AgentPlanLeaseId::from_raw(10),
            root,
            AgentModelCallSettlement::Completed,
        );
        supervisor
            .record_model_call_result(&execution, model_receipt)
            .expect("model result");
        record(&mut ledger, &mut metrics, &supervisor, root, 4, 150);

        let active_effect = AgentActiveEffect::for_progress_test(
            &manifest,
            AgentEffectId::new(1).expect("effect"),
            AgentPlanLeaseId::from_raw(10),
            root,
            SemanticEffectClass::Read,
            SemanticActionAttemptId::new(1).expect("action attempt"),
        );
        supervisor
            .record_active_effect(&execution, &active_effect)
            .expect("active effect");
        record(&mut ledger, &mut metrics, &supervisor, root, 5, 160);
        let effect_receipt = AgentEffectReceipt::for_progress_test(
            &manifest,
            AgentEffectId::new(1).expect("effect"),
            AgentPlanLeaseId::from_raw(10),
            root,
            SemanticEffectClass::Read,
            SemanticActionAttemptId::new(1).expect("action attempt"),
            AgentEffectSettlement::Verified(SemanticEffectProofKind::TargetState),
        );
        supervisor
            .record_effect_result(&execution, effect_receipt)
            .expect("effect result");
        record(&mut ledger, &mut metrics, &supervisor, root, 6, 180);

        supervisor
            .complete(execution, AgentSupervisorCompletion::Succeeded)
            .expect("complete root");
        record(&mut ledger, &mut metrics, &supervisor, root, 7, 200);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.events(), 7);
        assert_eq!(snapshot.activated_nodes(), 1);
        assert_eq!(snapshot.terminal_nodes(), 1);
        assert_eq!(snapshot.outcome(), Some(AgentRunProgressOutcome::Succeeded));
        assert_eq!(snapshot.total_elapsed_millis(), Some(100));
        assert_eq!(
            snapshot.queue_wait(),
            Some(AgentDurationMetrics {
                samples: 1,
                total_millis: 10,
                max_millis: 10,
            })
        );
        assert_eq!(snapshot.model().expect("model").samples(), 1);
        assert_eq!(snapshot.model().expect("model").total_millis(), 30);
        assert_eq!(snapshot.model().expect("model").max_millis(), 30);
        assert_eq!(snapshot.effect().expect("effect").total_millis(), 20);
        assert_eq!(snapshot.human_wait(), None);
        assert_eq!(snapshot.human_takeovers(), 0);
        assert_eq!(snapshot.needs_human().total(), 0);
        let debug = format!("{metrics:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("progress-metrics.example.test"));
        assert!(!debug.contains("credential"));
        assert!(!debug.contains("hidden"));
    }

    #[test]
    fn replay_time_and_same_id_foreign_revision_refusals_do_not_mutate() {
        let manifest = make_manifest(2, 100, false);
        let root = AgentPlanNodeId::from_raw(1);
        let mut supervisor = make_supervisor(&manifest, 2, false);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        let mut metrics =
            AgentRunProgressMetrics::try_new(&manifest, &supervisor).expect("metrics");
        let queued = record(&mut ledger, &mut metrics, &supervisor, root, 1, 100);
        let execution = supervisor.start(root, attempt(1)).expect("start");
        let active = record(&mut ledger, &mut metrics, &supervisor, root, 2, 110);
        let before = metrics.snapshot();

        assert_eq!(
            metrics.record_event(active).expect_err("replay"),
            AgentProgressMetricError::EventReplay
        );
        assert_eq!(metrics.snapshot(), before);

        let exact_time_supervisor = make_supervisor(&manifest, 2, false);
        let mut exact_time_ledger =
            AgentAuditLedger::try_new(&manifest, &exact_time_supervisor).expect("time ledger");
        let earlier = exact_time_ledger
            .record_current(
                &exact_time_supervisor,
                root,
                event(3),
                AgentPolicyInstant::from_millis(105),
            )
            .expect("earlier exact event");
        assert_eq!(
            metrics.record_event(earlier).expect_err("time regression"),
            AgentProgressMetricError::EventTime
        );
        assert_eq!(metrics.snapshot(), before);

        let changed = make_manifest(2, 99, false);
        assert_eq!(changed.id(), manifest.id());
        assert!(!changed.matches_revision(&manifest));
        let changed_supervisor = make_supervisor(&changed, 2, false);
        let mut changed_ledger =
            AgentAuditLedger::try_new(&changed, &changed_supervisor).expect("foreign ledger");
        let foreign = changed_ledger
            .record_current(
                &changed_supervisor,
                root,
                event(3),
                AgentPolicyInstant::from_millis(115),
            )
            .expect("foreign event");
        assert_eq!(
            metrics.record_event(foreign).expect_err("foreign revision"),
            AgentProgressMetricError::Authority
        );
        assert_eq!(metrics.snapshot(), before);

        supervisor
            .record_progress_activity(
                &execution,
                crate::AgentProgressActivity::try_new(AgentProgressOperation::Planning, None)
                    .expect("planning"),
            )
            .expect("planning progress");
        record(&mut ledger, &mut metrics, &supervisor, root, 3, 120);
        assert_eq!(metrics.snapshot().events(), 3);
        assert_ne!(metrics.snapshot(), before);
        assert_eq!(queued.id(), event(1));
    }

    #[test]
    fn human_wait_closes_only_on_a_later_exact_event() {
        let manifest = make_manifest(3, 100, false);
        let root = AgentPlanNodeId::from_raw(1);
        let mut supervisor = make_supervisor(&manifest, 3, false);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        let mut metrics =
            AgentRunProgressMetrics::try_new(&manifest, &supervisor).expect("metrics");
        record(&mut ledger, &mut metrics, &supervisor, root, 1, 100);
        let execution = supervisor.start(root, attempt(1)).expect("start");
        record(&mut ledger, &mut metrics, &supervisor, root, 2, 110);

        let mut registry = ContextRegistry::new();
        let identity = ContextIdentity::new(
            ContextId::from_raw(1),
            ContextRunId::from_raw(2),
            ProfileId::from(1),
            ContextKind::Owned,
        );
        supervisor
            .reserve_context(
                &execution,
                &manifest,
                &mut registry,
                identity,
                ContextCapabilities::try_new(
                    ContextKind::Owned,
                    &[
                        ContextCapability::Navigate,
                        ContextCapability::Observe,
                        ContextCapability::Act,
                        ContextCapability::Suspend,
                        ContextCapability::Recover,
                    ],
                )
                .expect("capabilities"),
            )
            .expect("reserve context");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("begin context");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settle context");
        let join = registry.join(identity.id()).expect("join");
        supervisor
            .wait_for_human(
                &execution,
                AgentNeedsHumanTransition::for_progress_test(
                    &manifest,
                    root,
                    join,
                    SemanticEffectClass::LocalWrite,
                    AgentNeedsHumanReason::HumanControl,
                ),
            )
            .expect("wait for human");
        record(&mut ledger, &mut metrics, &supervisor, root, 3, 130);
        let waiting = metrics.snapshot();
        assert_eq!(waiting.human_wait(), None);
        assert_eq!(waiting.needs_human().total(), 1);
        assert_eq!(
            waiting
                .needs_human()
                .reason(AgentNeedsHumanReason::HumanControl),
            1
        );

        let _resumed = supervisor.start(root, attempt(2)).expect("resume");
        record(&mut ledger, &mut metrics, &supervisor, root, 4, 170);
        let observed = metrics.snapshot().human_wait().expect("human duration");
        assert_eq!(observed.samples(), 1);
        assert_eq!(observed.total_millis(), 40);
    }

    #[test]
    fn terminal_operation_without_its_active_event_is_refused_without_mutation() {
        let manifest = make_manifest(5, 100, false);
        let root = AgentPlanNodeId::from_raw(1);
        let mut supervisor = make_supervisor(&manifest, 5, false);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        let mut metrics =
            AgentRunProgressMetrics::try_new(&manifest, &supervisor).expect("metrics");
        record(&mut ledger, &mut metrics, &supervisor, root, 1, 100);
        let execution = supervisor.start(root, attempt(1)).expect("start");
        record(&mut ledger, &mut metrics, &supervisor, root, 2, 110);

        let receipt = AgentModelCallReceipt::for_progress_test(
            &manifest,
            AgentModelCallId::new(1).expect("model"),
            AgentPlanLeaseId::from_raw(10),
            root,
            AgentModelCallSettlement::Completed,
        );
        supervisor
            .record_model_call_result(&execution, receipt)
            .expect("model result");
        let terminal_without_start = ledger
            .record_current(
                &supervisor,
                root,
                event(3),
                AgentPolicyInstant::from_millis(130),
            )
            .expect("canonical terminal event");
        let before = metrics.snapshot();
        assert_eq!(
            metrics
                .record_event(terminal_without_start)
                .expect_err("missing active model"),
            AgentProgressMetricError::OperationSequence
        );
        assert_eq!(metrics.snapshot(), before);

        let active = AgentActiveModelCall::for_progress_test(
            &manifest,
            AgentModelCallId::new(1).expect("model"),
            AgentPlanLeaseId::from_raw(10),
            root,
        );
        supervisor
            .record_active_model_call(
                &execution,
                crate::AgentProviderCallIdentity::from_active(&active),
            )
            .expect("active model");
        record(&mut ledger, &mut metrics, &supervisor, root, 4, 140);
        supervisor
            .record_model_call_result(&execution, receipt)
            .expect("model result retry");
        record(&mut ledger, &mut metrics, &supervisor, root, 5, 160);
        assert_eq!(
            metrics.snapshot().model().expect("model").total_millis(),
            20
        );
    }

    #[test]
    fn supervisor_terminal_with_an_active_operation_is_refused_without_mutation() {
        let manifest = make_manifest(6, 100, false);
        let root = AgentPlanNodeId::from_raw(1);
        let mut supervisor = make_supervisor(&manifest, 6, false);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        let mut metrics =
            AgentRunProgressMetrics::try_new(&manifest, &supervisor).expect("metrics");
        record(&mut ledger, &mut metrics, &supervisor, root, 1, 100);
        let execution = supervisor.start(root, attempt(1)).expect("start");
        record(&mut ledger, &mut metrics, &supervisor, root, 2, 110);

        let active = AgentActiveModelCall::for_progress_test(
            &manifest,
            AgentModelCallId::new(1).expect("model"),
            AgentPlanLeaseId::from_raw(10),
            root,
        );
        supervisor
            .record_active_model_call(
                &execution,
                crate::AgentProviderCallIdentity::from_active(&active),
            )
            .expect("active model");
        record(&mut ledger, &mut metrics, &supervisor, root, 3, 120);
        let before = metrics.snapshot();
        assert_eq!(metrics.active_models.len(), 1);

        supervisor
            .complete(execution, AgentSupervisorCompletion::Succeeded)
            .expect("complete supervisor");
        let terminal = ledger
            .record_current(
                &supervisor,
                root,
                event(4),
                AgentPolicyInstant::from_millis(130),
            )
            .expect("canonical supervisor terminal");
        assert_eq!(
            metrics
                .record_event(terminal)
                .expect_err("active operation must block terminal metrics"),
            AgentProgressMetricError::OperationSequence
        );
        assert_eq!(metrics.snapshot(), before);
        assert_eq!(metrics.active_models.len(), 1);
    }

    #[test]
    fn supervisor_terminal_with_an_active_effect_is_refused_without_mutation() {
        let manifest = make_manifest(7, 100, false);
        let root = AgentPlanNodeId::from_raw(1);
        let mut supervisor = make_supervisor(&manifest, 7, false);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        let mut metrics =
            AgentRunProgressMetrics::try_new(&manifest, &supervisor).expect("metrics");
        record(&mut ledger, &mut metrics, &supervisor, root, 1, 100);
        let execution = supervisor.start(root, attempt(1)).expect("start");
        record(&mut ledger, &mut metrics, &supervisor, root, 2, 110);

        let active = AgentActiveEffect::for_progress_test(
            &manifest,
            AgentEffectId::new(1).expect("effect"),
            AgentPlanLeaseId::from_raw(10),
            root,
            SemanticEffectClass::Read,
            SemanticActionAttemptId::new(1).expect("action attempt"),
        );
        supervisor
            .record_active_effect(&execution, &active)
            .expect("active effect");
        record(&mut ledger, &mut metrics, &supervisor, root, 3, 120);
        let before = metrics.snapshot();
        assert_eq!(metrics.active_effects.len(), 1);

        supervisor
            .complete(execution, AgentSupervisorCompletion::Succeeded)
            .expect("complete supervisor");
        let terminal = ledger
            .record_current(
                &supervisor,
                root,
                event(4),
                AgentPolicyInstant::from_millis(130),
            )
            .expect("canonical supervisor terminal");
        assert_eq!(
            metrics
                .record_event(terminal)
                .expect_err("active effect must block terminal metrics"),
            AgentProgressMetricError::OperationSequence
        );
        assert_eq!(metrics.snapshot(), before);
        assert_eq!(metrics.active_effects.len(), 1);
    }

    #[test]
    fn one_human_takeover_cancellation_is_deduplicated_across_terminal_nodes() {
        let manifest = make_manifest(4, 100, true);
        let root = AgentPlanNodeId::from_raw(1);
        let child = AgentPlanNodeId::from_raw(2);
        let mut supervisor = make_supervisor(&manifest, 4, true);
        let mut ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
        let mut metrics =
            AgentRunProgressMetrics::try_new(&manifest, &supervisor).expect("metrics");
        record(&mut ledger, &mut metrics, &supervisor, root, 1, 100);
        let execution = supervisor.start(root, attempt(1)).expect("start");
        record(&mut ledger, &mut metrics, &supervisor, root, 2, 110);
        supervisor.delegate(&execution, child).expect("delegate");
        record(&mut ledger, &mut metrics, &supervisor, child, 3, 120);

        let _batch = supervisor
            .cancel_subtree(
                root,
                cancellation(1),
                AgentSupervisorCancellationReason::HumanTakeover,
            )
            .expect("cancel tree");
        record(&mut ledger, &mut metrics, &supervisor, child, 4, 130);
        supervisor
            .drain_cancelled(execution, cancellation(1))
            .expect("drain root");
        record(&mut ledger, &mut metrics, &supervisor, root, 5, 140);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.activated_nodes(), 2);
        assert_eq!(snapshot.terminal_nodes(), 2);
        assert_eq!(snapshot.human_takeovers(), 1);
        assert_eq!(
            snapshot.outcome(),
            Some(AgentRunProgressOutcome::Cancelled(
                AgentSupervisorCancellationReason::HumanTakeover
            ))
        );
        assert_eq!(snapshot.total_elapsed_millis(), Some(40));
        let queue = snapshot.queue_wait().expect("root queue");
        assert_eq!(queue.samples(), 1);
        assert_eq!(queue.total_millis(), 10);
    }
}
