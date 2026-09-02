//! Run-scoped, content-free action timing metrics over exact batch terminals.
//!
//! This optional functional core owns no telemetry port, persistence, clock,
//! task, worker, channel, provider, browser context, or native resource. A
//! trusted shell explicitly feeds immutable batch terminals. Fixed histograms
//! retain distributions without retaining page, site, profile, or native-handle
//! data; exact receipt identities are stored only to reject replay.

use std::fmt;

use thiserror::Error;

use crate::{
    AgentEffectId, AgentEffectSettlement, AgentPlanNodeId, AgentRunManifest, AgentRunManifestId,
    AgentRunSupervisor, AgentSupervisorId, SemanticActionAttemptId, SemanticActionBatchCompletion,
    SemanticActionBatchFailure, SemanticActionBatchFailureStage, SemanticActionBatchId,
    SemanticActionBatchOutcome, SemanticActionBatchResult, SemanticActionExecutionApplied,
    SemanticActionExecutionBackend, SemanticActionFailure, SemanticVerificationError,
    MAX_AGENT_PLAN_NODES, MAX_SEMANTIC_ACTIONS_PER_BATCH,
};

/// Inclusive upper bounds for the fixed duration histogram, in milliseconds.
///
/// One final overflow bucket follows these bounds. The logarithmic/fine-grained
/// mix keeps the reducer small while retaining useful interactive-tail shape.
pub const AGENT_ACTION_DURATION_BUCKET_UPPER_BOUNDS_MILLIS: [u64; 17] = [
    0, 1, 2, 4, 8, 16, 32, 64, 128, 250, 500, 1_000, 2_000, 5_000, 10_000, 30_000, 60_000,
];

/// Number of fixed duration buckets including the final overflow bucket.
pub const AGENT_ACTION_DURATION_BUCKET_COUNT: usize =
    AGENT_ACTION_DURATION_BUCKET_UPPER_BOUNDS_MILLIS.len() + 1;

/// Maximum in-memory size of one copyable action-performance snapshot.
pub const MAX_AGENT_ACTION_PERFORMANCE_SNAPSHOT_BYTES: usize = 1_024;

/// Fixed, checked duration distribution for one action phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentActionDurationMetrics {
    samples: u32,
    total_millis: u64,
    max_millis: u64,
    buckets: [u32; AGENT_ACTION_DURATION_BUCKET_COUNT],
}

impl AgentActionDurationMetrics {
    const fn empty() -> Self {
        Self {
            samples: 0,
            total_millis: 0,
            max_millis: 0,
            buckets: [0; AGENT_ACTION_DURATION_BUCKET_COUNT],
        }
    }

    /// Number of exact observed durations.
    pub const fn samples(self) -> u32 {
        self.samples
    }

    /// Checked sum of exact durations.
    pub const fn total_millis(self) -> u64 {
        self.total_millis
    }

    /// Largest exact duration.
    pub const fn max_millis(self) -> u64 {
        self.max_millis
    }

    /// Fixed histogram counts; the final entry is greater than 60 seconds.
    pub const fn bucket_counts(self) -> [u32; AGENT_ACTION_DURATION_BUCKET_COUNT] {
        self.buckets
    }

    fn checked_record(self, millis: u64) -> Result<Self, AgentActionMetricError> {
        let mut next = self;
        next.samples = next
            .samples
            .checked_add(1)
            .ok_or(AgentActionMetricError::Overflow)?;
        next.total_millis = next
            .total_millis
            .checked_add(millis)
            .ok_or(AgentActionMetricError::Overflow)?;
        next.max_millis = next.max_millis.max(millis);
        let bucket = duration_bucket(millis);
        next.buckets[bucket] = next.buckets[bucket]
            .checked_add(1)
            .ok_or(AgentActionMetricError::Overflow)?;
        Ok(next)
    }
}

/// Checked settlement-fact count aggregate for applied actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentActionSettleEventMetrics {
    samples: u32,
    total: u64,
    max: u16,
}

impl AgentActionSettleEventMetrics {
    const fn empty() -> Self {
        Self {
            samples: 0,
            total: 0,
            max: 0,
        }
    }

    /// Applied actions with an exact settlement-event count.
    pub const fn samples(self) -> u32 {
        self.samples
    }

    /// Checked sum of coalesced settlement facts.
    pub const fn total(self) -> u64 {
        self.total
    }

    /// Largest per-action settlement-fact count.
    pub const fn max(self) -> u16 {
        self.max
    }

    fn checked_record(self, events: u16) -> Result<Self, AgentActionMetricError> {
        Ok(Self {
            samples: self
                .samples
                .checked_add(1)
                .ok_or(AgentActionMetricError::Overflow)?,
            total: self
                .total
                .checked_add(u64::from(events))
                .ok_or(AgentActionMetricError::Overflow)?,
            max: self.max.max(events),
        })
    }
}

/// Fixed backend attribution for applied action terminals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentActionBackendMetrics {
    counts: [u32; 3],
}

impl AgentActionBackendMetrics {
    const fn empty() -> Self {
        Self { counts: [0; 3] }
    }

    /// Applied actions attributed to one closed backend class.
    pub const fn count(self, backend: SemanticActionExecutionBackend) -> u32 {
        self.counts[backend_index(backend)]
    }

    fn checked_record(
        self,
        backend: SemanticActionExecutionBackend,
    ) -> Result<Self, AgentActionMetricError> {
        let mut next = self;
        let index = backend_index(backend);
        next.counts[index] = next.counts[index]
            .checked_add(1)
            .ok_or(AgentActionMetricError::Overflow)?;
        Ok(next)
    }
}

/// Immutable content-free aggregate of exact action batch terminals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentRunActionPerformanceSnapshot {
    manifest: AgentRunManifestId,
    supervisor: AgentSupervisorId,
    batches: u32,
    complete_batches: u32,
    stopped_batches: u32,
    failed_batches: u32,
    actions: u32,
    verified_actions: u32,
    failed_actions: u32,
    before_verification_failures: u32,
    after_execution_failures: u32,
    unorderable_verification_observations: u32,
    backends: AgentActionBackendMetrics,
    native_execution: AgentActionDurationMetrics,
    settlement: AgentActionDurationMetrics,
    applied_to_settlement: AgentActionDurationMetrics,
    verification_observation: AgentActionDurationMetrics,
    settlement_events: AgentActionSettleEventMetrics,
}

const _: () = assert!(
    std::mem::size_of::<AgentRunActionPerformanceSnapshot>()
        <= MAX_AGENT_ACTION_PERFORMANCE_SNAPSHOT_BYTES
);

impl AgentRunActionPerformanceSnapshot {
    /// Exact immutable manifest identity.
    pub const fn manifest(self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact mutable supervisor incarnation.
    pub const fn supervisor(self) -> AgentSupervisorId {
        self.supervisor
    }

    /// Unique exact batch terminals recorded.
    pub const fn batches(self) -> u32 {
        self.batches
    }

    /// Fully completed batches.
    pub const fn complete_batches(self) -> u32 {
        self.complete_batches
    }

    /// Batches stopped after a verified prefix.
    pub const fn stopped_batches(self) -> u32 {
        self.stopped_batches
    }

    /// Batches ending in a typed failed action.
    pub const fn failed_batches(self) -> u32 {
        self.failed_batches
    }

    /// Exact terminal actions represented by all recorded batches.
    pub const fn actions(self) -> u32 {
        self.actions
    }

    /// Independently verified action terminals.
    pub const fn verified_actions(self) -> u32 {
        self.verified_actions
    }

    /// Policy-accounted typed failed action terminals.
    pub const fn failed_actions(self) -> u32 {
        self.failed_actions
    }

    /// Failures without applied execution/settlement timing.
    pub const fn before_verification_failures(self) -> u32 {
        self.before_verification_failures
    }

    /// Failures after applied execution reached independent verification.
    pub const fn after_execution_failures(self) -> u32 {
        self.after_execution_failures
    }

    /// Failed proof clocks predating native revalidation and therefore omitted.
    pub const fn unorderable_verification_observations(self) -> u32 {
        self.unorderable_verification_observations
    }

    /// Applied-action backend attribution.
    pub const fn backends(self) -> AgentActionBackendMetrics {
        self.backends
    }

    /// Native revalidation-to-backend-completion distribution.
    pub const fn native_execution(self) -> AgentActionDurationMetrics {
        self.native_execution
    }

    /// Backend-completion-to-settlement-terminal distribution.
    pub const fn settlement(self) -> AgentActionDurationMetrics {
        self.settlement
    }

    /// Native revalidation-to-settlement-terminal distribution.
    pub const fn applied_to_settlement(self) -> AgentActionDurationMetrics {
        self.applied_to_settlement
    }

    /// Native revalidation-to-independent-observation distribution when ordered.
    pub const fn verification_observation(self) -> AgentActionDurationMetrics {
        self.verification_observation
    }

    /// Coalesced settlement-fact aggregate for applied actions.
    pub const fn settlement_events(self) -> AgentActionSettleEventMetrics {
        self.settlement_events
    }

    fn empty(manifest: AgentRunManifestId, supervisor: AgentSupervisorId) -> Self {
        Self {
            manifest,
            supervisor,
            batches: 0,
            complete_batches: 0,
            stopped_batches: 0,
            failed_batches: 0,
            actions: 0,
            verified_actions: 0,
            failed_actions: 0,
            before_verification_failures: 0,
            after_execution_failures: 0,
            unorderable_verification_observations: 0,
            backends: AgentActionBackendMetrics::empty(),
            native_execution: AgentActionDurationMetrics::empty(),
            settlement: AgentActionDurationMetrics::empty(),
            applied_to_settlement: AgentActionDurationMetrics::empty(),
            verification_observation: AgentActionDurationMetrics::empty(),
            settlement_events: AgentActionSettleEventMetrics::empty(),
        }
    }

    fn checked_record_batch(
        mut self,
        outcome: SemanticActionBatchOutcome,
    ) -> Result<Self, AgentActionMetricError> {
        self.batches = add_u32(self.batches, 1)?;
        match outcome {
            SemanticActionBatchOutcome::Complete => {
                self.complete_batches = add_u32(self.complete_batches, 1)?;
            }
            SemanticActionBatchOutcome::Stopped { .. } => {
                self.stopped_batches = add_u32(self.stopped_batches, 1)?;
            }
            SemanticActionBatchOutcome::Failed { .. } => {
                self.failed_batches = add_u32(self.failed_batches, 1)?;
            }
        }
        Ok(self)
    }

    fn checked_record_completion(
        mut self,
        completion: SemanticActionBatchCompletion,
    ) -> Result<Self, AgentActionMetricError> {
        self.actions = add_u32(self.actions, 1)?;
        self.verified_actions = add_u32(self.verified_actions, 1)?;
        self.checked_record_applied(
            completion.execution(),
            completion.settlement_elapsed_millis(),
            completion.settlement_terminal_at().millis(),
            completion.settlement_event_count(),
            Some(completion.observed_at().millis()),
        )
    }

    fn checked_record_failure(
        mut self,
        failure: SemanticActionBatchFailure,
    ) -> Result<Self, AgentActionMetricError> {
        self.actions = add_u32(self.actions, 1)?;
        self.failed_actions = add_u32(self.failed_actions, 1)?;
        match failure.stage() {
            SemanticActionBatchFailureStage::BeforeVerification => {
                self.before_verification_failures = add_u32(self.before_verification_failures, 1)?;
                Ok(self)
            }
            SemanticActionBatchFailureStage::AfterExecution => {
                self.after_execution_failures = add_u32(self.after_execution_failures, 1)?;
                self.checked_record_applied(
                    failure
                        .execution()
                        .ok_or(AgentActionMetricError::Invariant)?,
                    failure
                        .settlement_elapsed_millis()
                        .ok_or(AgentActionMetricError::Invariant)?,
                    failure
                        .settlement_terminal_at()
                        .ok_or(AgentActionMetricError::Invariant)?
                        .millis(),
                    failure
                        .settlement_event_count()
                        .ok_or(AgentActionMetricError::Invariant)?,
                    Some(
                        failure
                            .verification_observed_at()
                            .ok_or(AgentActionMetricError::Invariant)?
                            .millis(),
                    ),
                )
            }
        }
    }

    fn checked_record_applied(
        mut self,
        execution: SemanticActionExecutionApplied,
        settlement_millis: u64,
        settlement_terminal_millis: u64,
        settlement_events: u16,
        verification_observed_millis: Option<u64>,
    ) -> Result<Self, AgentActionMetricError> {
        let revalidated = execution.revalidated_at().millis();
        let completed = execution.completed_at().millis();
        let native_millis = completed
            .checked_sub(revalidated)
            .ok_or(AgentActionMetricError::Invariant)?;
        if settlement_terminal_millis.checked_sub(settlement_millis) != Some(completed) {
            return Err(AgentActionMetricError::Invariant);
        }
        let applied_to_settlement = settlement_terminal_millis
            .checked_sub(revalidated)
            .ok_or(AgentActionMetricError::Invariant)?;
        self.backends = self.backends.checked_record(execution.backend())?;
        self.native_execution = self.native_execution.checked_record(native_millis)?;
        self.settlement = self.settlement.checked_record(settlement_millis)?;
        self.applied_to_settlement = self
            .applied_to_settlement
            .checked_record(applied_to_settlement)?;
        self.settlement_events = self.settlement_events.checked_record(settlement_events)?;
        if let Some(observed) = verification_observed_millis {
            if let Some(millis) = observed.checked_sub(revalidated) {
                self.verification_observation =
                    self.verification_observation.checked_record(millis)?;
            } else {
                self.unorderable_verification_observations =
                    add_u32(self.unorderable_verification_observations, 1)?;
            }
        }
        Ok(self)
    }
}

/// Closed refusal while reducing exact action terminals into local metrics.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentActionMetricError {
    /// Manifest, supervisor, revision, or plan-node authority did not match.
    #[error("agent action metric authority mismatched")]
    Authority,
    /// Metrics were not created against the queued root before execution.
    #[error("agent action metrics must start with the queued supervisor root")]
    StartState,
    /// One exact terminal batch was already recorded.
    #[error("agent action metric batch replayed")]
    BatchReplay,
    /// One effect or native attempt appeared in more than one terminal.
    #[error("agent action metric receipt replayed")]
    ReceiptReplay,
    /// Recorded terminal actions exceeded the manifest operation ceiling.
    #[error("agent action metric operation budget exceeded")]
    Budget,
    /// Bounded local metric storage allocation failed.
    #[error("agent action metric bounded storage is unavailable")]
    Capacity,
    /// Checked counter or duration arithmetic overflowed.
    #[error("agent action metric arithmetic overflowed")]
    Overflow,
    /// A supposedly finalized batch carried contradictory closed facts.
    #[error("agent action metric batch invariant failed")]
    Invariant,
}

/// Optional bounded reducer over exact content-free action batch terminals.
#[must_use]
pub struct AgentRunActionPerformanceMetrics {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    supervisor: AgentSupervisorId,
    operation_limit: u32,
    nodes: Vec<AgentPlanNodeId>,
    batches: Vec<SemanticActionBatchId>,
    effect_receipts: Vec<AgentEffectId>,
    effect_attempts: Vec<SemanticActionAttemptId>,
    snapshot: AgentRunActionPerformanceSnapshot,
}

impl AgentRunActionPerformanceMetrics {
    /// Joins one empty reducer to an exact queued supervisor and manifest revision.
    pub fn try_new(
        manifest: &AgentRunManifest,
        supervisor: &AgentRunSupervisor,
    ) -> Result<Self, AgentActionMetricError> {
        if !supervisor.topology().matches_manifest(manifest) {
            return Err(AgentActionMetricError::Authority);
        }
        if manifest.plan_nodes().len() > MAX_AGENT_PLAN_NODES {
            return Err(AgentActionMetricError::Invariant);
        }
        let status = supervisor.status();
        let root = supervisor.topology().root();
        if status.activated() != 1
            || status.live() != 1
            || status.queued() != 1
            || status.executing() != 0
            || status.contexts() != 0
            || supervisor
                .node_status(root)
                .is_none_or(|state| state != crate::AgentSupervisorNodeStatus::Queued)
        {
            return Err(AgentActionMetricError::StartState);
        }
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(manifest.plan_nodes().len())
            .map_err(|_| AgentActionMetricError::Capacity)?;
        nodes.extend(manifest.plan_nodes().iter().map(|node| node.id()));
        Ok(Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            supervisor: supervisor.id(),
            operation_limit: manifest.budget().operations(),
            nodes,
            batches: Vec::new(),
            effect_receipts: Vec::new(),
            effect_attempts: Vec::new(),
            snapshot: AgentRunActionPerformanceSnapshot::empty(manifest.id(), supervisor.id()),
        })
    }

    /// Atomically records one immutable exact batch terminal.
    pub fn record_batch_result(
        &mut self,
        result: &SemanticActionBatchResult,
    ) -> Result<(), AgentActionMetricError> {
        let batch_index = match self.batches.binary_search(&result.batch()) {
            Ok(_) => return Err(AgentActionMetricError::BatchReplay),
            Err(index) => index,
        };
        let executed = validate_batch_shape(result)?;
        let next_actions = self
            .snapshot
            .actions
            .checked_add(executed)
            .ok_or(AgentActionMetricError::Overflow)?;
        if next_actions > self.operation_limit {
            return Err(AgentActionMetricError::Budget);
        }

        let mut next = self.snapshot.checked_record_batch(result.outcome())?;
        let mut ids = [None; MAX_SEMANTIC_ACTIONS_PER_BATCH];
        let mut attempts = [None; MAX_SEMANTIC_ACTIONS_PER_BATCH];
        let mut admitted = 0_usize;
        for (index, completion) in result.completions().iter().copied().enumerate() {
            validate_completion(result, completion, index)?;
            self.validate_receipt(completion.receipt())?;
            admit_identity(
                &self.effect_receipts,
                &self.effect_attempts,
                &mut ids,
                &mut attempts,
                &mut admitted,
                completion.receipt().id(),
                completion.attempt(),
            )?;
            next = next.checked_record_completion(completion)?;
        }
        if let Some(failure) = result.failure() {
            let expected_failure = match result.outcome() {
                SemanticActionBatchOutcome::Failed { failure, .. } => failure,
                _ => return Err(AgentActionMetricError::Invariant),
            };
            validate_failure(result, failure, expected_failure)?;
            self.validate_receipt(failure.receipt())?;
            admit_identity(
                &self.effect_receipts,
                &self.effect_attempts,
                &mut ids,
                &mut attempts,
                &mut admitted,
                failure.receipt().id(),
                failure.receipt().attempt(),
            )?;
            next = next.checked_record_failure(failure)?;
        }
        if admitted != usize::try_from(executed).map_err(|_| AgentActionMetricError::Invariant)? {
            return Err(AgentActionMetricError::Invariant);
        }

        self.batches
            .try_reserve(1)
            .map_err(|_| AgentActionMetricError::Capacity)?;
        self.effect_receipts
            .try_reserve(admitted)
            .map_err(|_| AgentActionMetricError::Capacity)?;
        self.effect_attempts
            .try_reserve(admitted)
            .map_err(|_| AgentActionMetricError::Capacity)?;
        self.batches.insert(batch_index, result.batch());
        for index in 0..admitted {
            let id = ids[index].ok_or(AgentActionMetricError::Invariant)?;
            let insertion = self
                .effect_receipts
                .binary_search(&id)
                .expect_err("preflight excluded effect replay");
            self.effect_receipts.insert(insertion, id);
            let attempt = attempts[index].ok_or(AgentActionMetricError::Invariant)?;
            let insertion = self
                .effect_attempts
                .binary_search(&attempt)
                .expect_err("preflight excluded attempt replay");
            self.effect_attempts.insert(insertion, attempt);
        }
        self.snapshot = next;
        Ok(())
    }

    /// Current fixed content-free aggregate.
    pub const fn snapshot(&self) -> AgentRunActionPerformanceSnapshot {
        self.snapshot
    }

    fn validate_receipt(
        &self,
        receipt: crate::AgentEffectReceipt,
    ) -> Result<(), AgentActionMetricError> {
        if !receipt.matches_manifest_revision(self.manifest, self.manifest_guard)
            || self.nodes.binary_search(&receipt.node()).is_err()
        {
            return Err(AgentActionMetricError::Authority);
        }
        Ok(())
    }
}

impl fmt::Debug for AgentRunActionPerformanceMetrics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunActionPerformanceMetrics")
            .field("manifest", &self.manifest)
            .field("manifest_guard", &"[redacted]")
            .field("supervisor", &self.supervisor)
            .field("snapshot", &self.snapshot)
            .field("nodes", &self.nodes.len())
            .field("batches", &self.batches.len())
            .field("effect_receipts", &self.effect_receipts.len())
            .field("effect_attempts", &self.effect_attempts.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

fn validate_batch_shape(result: &SemanticActionBatchResult) -> Result<u32, AgentActionMetricError> {
    let completed =
        u8::try_from(result.completions().len()).map_err(|_| AgentActionMetricError::Invariant)?;
    if result.total() == 0 || usize::from(result.total()) > MAX_SEMANTIC_ACTIONS_PER_BATCH {
        return Err(AgentActionMetricError::Invariant);
    }
    let executed = match result.outcome() {
        SemanticActionBatchOutcome::Complete
            if completed == result.total() && result.failure().is_none() =>
        {
            completed
        }
        SemanticActionBatchOutcome::Stopped { remaining, .. }
            if completed > 0
                && remaining > 0
                && result.failure().is_none()
                && completed.checked_add(remaining) == Some(result.total()) =>
        {
            completed
        }
        SemanticActionBatchOutcome::Failed {
            ordinal,
            failure,
            recovery,
        } if result.failure().is_some()
            && completed.checked_add(1) == Some(ordinal)
            && ordinal <= result.total()
            && recovery == failure.recovery_hint() =>
        {
            ordinal
        }
        _ => return Err(AgentActionMetricError::Invariant),
    };
    Ok(u32::from(executed))
}

fn validate_completion(
    result: &SemanticActionBatchResult,
    completion: SemanticActionBatchCompletion,
    index: usize,
) -> Result<(), AgentActionMetricError> {
    let ordinal = u8::try_from(index + 1).map_err(|_| AgentActionMetricError::Invariant)?;
    let receipt = completion.receipt();
    let execution = completion.execution();
    if completion.ordinal() != ordinal
        || receipt.effect() != result.effect()
        || receipt.attempt() != completion.attempt()
        || receipt.settlement() != AgentEffectSettlement::Verified(completion.proof())
        || execution.completed_at() < execution.revalidated_at()
        || completion.observed_at() < completion.settlement_terminal_at()
        || completion
            .settlement_terminal_at()
            .millis()
            .checked_sub(completion.settlement_elapsed_millis())
            != Some(execution.completed_at().millis())
    {
        return Err(AgentActionMetricError::Invariant);
    }
    Ok(())
}

fn validate_failure(
    result: &SemanticActionBatchResult,
    failure: SemanticActionBatchFailure,
    expected: SemanticActionFailure,
) -> Result<(), AgentActionMetricError> {
    if failure.receipt().effect() != result.effect()
        || failure.receipt().settlement() != AgentEffectSettlement::Failed(expected)
    {
        return Err(AgentActionMetricError::Invariant);
    }
    match failure.stage() {
        SemanticActionBatchFailureStage::BeforeVerification => {
            if failure.execution().is_some()
                || failure.settlement_event_count().is_some()
                || failure.settlement_elapsed_millis().is_some()
                || failure.settlement_terminal_at().is_some()
                || failure.verification_observed_at().is_some()
                || failure.verification_error().is_some()
            {
                return Err(AgentActionMetricError::Invariant);
            }
        }
        SemanticActionBatchFailureStage::AfterExecution => {
            let execution = failure
                .execution()
                .ok_or(AgentActionMetricError::Invariant)?;
            let elapsed = failure
                .settlement_elapsed_millis()
                .ok_or(AgentActionMetricError::Invariant)?;
            let terminal = failure
                .settlement_terminal_at()
                .ok_or(AgentActionMetricError::Invariant)?;
            let observed = failure
                .verification_observed_at()
                .ok_or(AgentActionMetricError::Invariant)?;
            let error = failure
                .verification_error()
                .ok_or(AgentActionMetricError::Invariant)?;
            if failure.settlement_event_count().is_none()
                || execution.completed_at() < execution.revalidated_at()
                || terminal.millis().checked_sub(elapsed) != Some(execution.completed_at().millis())
                || error.action_failure() != expected
                || !verification_clock_matches(error, observed.millis(), terminal.millis())
            {
                return Err(AgentActionMetricError::Invariant);
            }
        }
    }
    Ok(())
}

fn verification_clock_matches(
    error: SemanticVerificationError,
    observed_millis: u64,
    terminal_millis: u64,
) -> bool {
    match error {
        SemanticVerificationError::EvidenceBeforeSettlement => observed_millis < terminal_millis,
        _ => observed_millis >= terminal_millis,
    }
}

#[allow(clippy::too_many_arguments)]
fn admit_identity(
    existing_ids: &[AgentEffectId],
    existing_attempts: &[SemanticActionAttemptId],
    ids: &mut [Option<AgentEffectId>; MAX_SEMANTIC_ACTIONS_PER_BATCH],
    attempts: &mut [Option<SemanticActionAttemptId>; MAX_SEMANTIC_ACTIONS_PER_BATCH],
    admitted: &mut usize,
    id: AgentEffectId,
    attempt: SemanticActionAttemptId,
) -> Result<(), AgentActionMetricError> {
    if *admitted >= MAX_SEMANTIC_ACTIONS_PER_BATCH
        || existing_ids.binary_search(&id).is_ok()
        || existing_attempts.binary_search(&attempt).is_ok()
        || ids[..*admitted].contains(&Some(id))
        || attempts[..*admitted].contains(&Some(attempt))
    {
        return Err(AgentActionMetricError::ReceiptReplay);
    }
    ids[*admitted] = Some(id);
    attempts[*admitted] = Some(attempt);
    *admitted += 1;
    Ok(())
}

fn duration_bucket(millis: u64) -> usize {
    AGENT_ACTION_DURATION_BUCKET_UPPER_BOUNDS_MILLIS
        .iter()
        .position(|upper| millis <= *upper)
        .unwrap_or(AGENT_ACTION_DURATION_BUCKET_COUNT - 1)
}

fn add_u32(left: u32, right: u32) -> Result<u32, AgentActionMetricError> {
    left.checked_add(right)
        .ok_or(AgentActionMetricError::Overflow)
}

const fn backend_index(backend: SemanticActionExecutionBackend) -> usize {
    match backend {
        SemanticActionExecutionBackend::FixedSemanticRecipe => 0,
        SemanticActionExecutionBackend::EngineNativeInput => 1,
        SemanticActionExecutionBackend::InProcessAccessibility => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentAccountScope, AgentDelegationSpec, AgentDelegationTopology, AgentEffectReceipt,
        AgentEffectScope, AgentPlanLeaseId, AgentPlanNodeAuthority, AgentPlanNodeScope,
        AgentPolicyInstant, AgentRunBudget, AgentRunScope, ContextCapabilities, ContextCapability,
        ContextId, ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, SemanticActionBatchStopReason, SemanticActionExecutionInstant,
        SemanticActionKind, SemanticActionNextState, SemanticActionRecoveryHint,
        SemanticEffectClass, SemanticEffectProofKind, SemanticOrigin, SemanticSensitivity,
        SemanticSettleInstant, SemanticVerificationError,
    };
    use zephium_core::ids::ProfileId;

    fn make_manifest(id: u128, operations: u32) -> AgentRunManifest {
        let profile = ProfileId::from(31);
        let origin =
            SemanticOrigin::parse("https://action-metrics.example.test/private").expect("origin");
        let effects =
            AgentEffectScope::try_new(&[SemanticEffectClass::LocalWrite]).expect("effects");
        let budget = AgentRunBudget::try_new(operations, 1_000, 1_000, 1).expect("budget");
        AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(id),
            ContextRunId::from_raw(32),
            AgentRunScope::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin.clone()],
                SemanticSensitivity::Public,
                effects,
                Vec::new(),
            )
            .expect("scope"),
            budget,
            AgentPolicyInstant::from_millis(1),
            AgentPolicyInstant::from_millis(10_000),
            vec![AgentPlanNodeScope::new(
                AgentPlanNodeId::from_raw(33),
                AgentPlanNodeAuthority::try_new(
                    vec![profile],
                    vec![AgentAccountScope::Anonymous],
                    vec![origin],
                    SemanticSensitivity::Public,
                    effects,
                )
                .expect("authority"),
                budget,
                AgentPolicyInstant::from_millis(9_000),
            )],
        )
        .expect("manifest")
    }

    fn supervisor(manifest: &AgentRunManifest, id: u64) -> AgentRunSupervisor {
        AgentRunSupervisor::new(
            AgentSupervisorId::new(id).expect("supervisor"),
            AgentDelegationTopology::try_new(
                manifest,
                vec![AgentDelegationSpec::new(
                    AgentPlanNodeId::from_raw(33),
                    None,
                )],
            )
            .expect("topology"),
        )
    }

    fn context() -> crate::ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(40),
            ContextRunId::from_raw(32),
            ProfileId::from(31),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[ContextCapability::Observe, ContextCapability::Act],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settle");
        registry.join(identity.id()).expect("join")
    }

    fn receipt(
        manifest: &AgentRunManifest,
        id: u64,
        attempt: u64,
        settlement: AgentEffectSettlement,
    ) -> AgentEffectReceipt {
        AgentEffectReceipt::for_progress_test(
            manifest,
            AgentEffectId::new(id).expect("effect"),
            AgentPlanLeaseId::from_raw(41),
            AgentPlanNodeId::from_raw(33),
            SemanticEffectClass::LocalWrite,
            SemanticActionAttemptId::new(attempt).expect("attempt"),
            settlement,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn completion(
        manifest: &AgentRunManifest,
        ordinal: u8,
        id: u64,
        attempt: u64,
        backend: SemanticActionExecutionBackend,
        revalidated: u64,
        completed: u64,
        settlement: u64,
        events: u16,
        observed: u64,
    ) -> SemanticActionBatchCompletion {
        let proof = SemanticEffectProofKind::TargetState;
        SemanticActionBatchCompletion::for_action_metrics_test(
            ordinal,
            SemanticActionKind::Click,
            receipt(
                manifest,
                id,
                attempt,
                AgentEffectSettlement::Verified(proof),
            ),
            SemanticActionExecutionApplied::for_action_metrics_test(
                backend,
                SemanticActionExecutionInstant::from_millis(revalidated),
                SemanticActionExecutionInstant::from_millis(completed),
            ),
            events,
            settlement,
            SemanticSettleInstant::from_millis(completed + settlement),
            SemanticActionAttemptId::new(attempt).expect("attempt"),
            proof,
            SemanticSettleInstant::from_millis(observed),
            context(),
            SemanticActionNextState::Diff,
        )
    }

    fn before_failure(
        manifest: &AgentRunManifest,
        id: u64,
        attempt: u64,
        failure: SemanticActionFailure,
    ) -> SemanticActionBatchFailure {
        SemanticActionBatchFailure::for_action_metrics_test(
            receipt(
                manifest,
                id,
                attempt,
                AgentEffectSettlement::Failed(failure),
            ),
            SemanticActionBatchFailureStage::BeforeVerification,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn after_failure(
        manifest: &AgentRunManifest,
        id: u64,
        attempt: u64,
        backend: SemanticActionExecutionBackend,
        revalidated: u64,
        completed: u64,
        settlement: u64,
        events: u16,
        observed: u64,
        error: SemanticVerificationError,
    ) -> SemanticActionBatchFailure {
        let failure = error.action_failure();
        SemanticActionBatchFailure::for_action_metrics_test(
            receipt(
                manifest,
                id,
                attempt,
                AgentEffectSettlement::Failed(failure),
            ),
            SemanticActionBatchFailureStage::AfterExecution,
            Some(SemanticActionExecutionApplied::for_action_metrics_test(
                backend,
                SemanticActionExecutionInstant::from_millis(revalidated),
                SemanticActionExecutionInstant::from_millis(completed),
            )),
            Some(events),
            Some(settlement),
            Some(SemanticSettleInstant::from_millis(completed + settlement)),
            Some(SemanticSettleInstant::from_millis(observed)),
            Some(error),
        )
    }

    #[test]
    fn exact_batch_terminals_build_fixed_content_free_distributions() {
        let manifest = make_manifest(50, 10);
        let supervisor = supervisor(&manifest, 51);
        let mut metrics =
            AgentRunActionPerformanceMetrics::try_new(&manifest, &supervisor).expect("metrics");
        let complete = SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(1).expect("batch"),
            SemanticEffectClass::LocalWrite,
            2,
            vec![
                completion(
                    &manifest,
                    1,
                    1,
                    1,
                    SemanticActionExecutionBackend::FixedSemanticRecipe,
                    100,
                    104,
                    6,
                    2,
                    112,
                ),
                completion(
                    &manifest,
                    2,
                    2,
                    2,
                    SemanticActionExecutionBackend::EngineNativeInput,
                    200,
                    201,
                    9,
                    3,
                    220,
                ),
            ],
            SemanticActionBatchOutcome::Complete,
            None,
        );
        metrics.record_batch_result(&complete).expect("complete");

        let failure = before_failure(&manifest, 3, 3, SemanticActionFailure::TargetOccluded);
        let failed = SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(2).expect("batch"),
            SemanticEffectClass::LocalWrite,
            1,
            Vec::new(),
            SemanticActionBatchOutcome::Failed {
                ordinal: 1,
                failure: SemanticActionFailure::TargetOccluded,
                recovery: SemanticActionRecoveryHint::FreshObservationRequired,
            },
            Some(failure),
        );
        metrics.record_batch_result(&failed).expect("pre failure");

        let failure = after_failure(
            &manifest,
            4,
            4,
            SemanticActionExecutionBackend::InProcessAccessibility,
            300,
            305,
            15,
            4,
            325,
            SemanticVerificationError::OutcomeNotObserved,
        );
        let after = SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(3).expect("batch"),
            SemanticEffectClass::LocalWrite,
            1,
            Vec::new(),
            SemanticActionBatchOutcome::Failed {
                ordinal: 1,
                failure: SemanticActionFailure::VerificationFailed,
                recovery: SemanticActionRecoveryHint::FreshObservationRequired,
            },
            Some(failure),
        );
        metrics.record_batch_result(&after).expect("post failure");

        let failure = after_failure(
            &manifest,
            5,
            5,
            SemanticActionExecutionBackend::FixedSemanticRecipe,
            400,
            405,
            15,
            1,
            399,
            SemanticVerificationError::EvidenceBeforeSettlement,
        );
        let unorderable = SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(4).expect("batch"),
            SemanticEffectClass::LocalWrite,
            1,
            Vec::new(),
            SemanticActionBatchOutcome::Failed {
                ordinal: 1,
                failure: SemanticActionFailure::VerificationFailed,
                recovery: SemanticActionRecoveryHint::FreshObservationRequired,
            },
            Some(failure),
        );
        metrics
            .record_batch_result(&unorderable)
            .expect("unorderable proof clock");

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.manifest(), manifest.id());
        assert_eq!(snapshot.supervisor(), supervisor.id());
        assert_eq!(snapshot.batches(), 4);
        assert_eq!(snapshot.complete_batches(), 1);
        assert_eq!(snapshot.stopped_batches(), 0);
        assert_eq!(snapshot.failed_batches(), 3);
        assert_eq!(snapshot.actions(), 5);
        assert_eq!(snapshot.verified_actions(), 2);
        assert_eq!(snapshot.failed_actions(), 3);
        assert_eq!(snapshot.before_verification_failures(), 1);
        assert_eq!(snapshot.after_execution_failures(), 2);
        assert_eq!(snapshot.unorderable_verification_observations(), 1);
        assert_eq!(
            snapshot
                .backends()
                .count(SemanticActionExecutionBackend::FixedSemanticRecipe),
            2
        );
        assert_eq!(
            snapshot
                .backends()
                .count(SemanticActionExecutionBackend::EngineNativeInput),
            1
        );
        assert_eq!(
            snapshot
                .backends()
                .count(SemanticActionExecutionBackend::InProcessAccessibility),
            1
        );
        assert_eq!(snapshot.native_execution().samples(), 4);
        assert_eq!(snapshot.native_execution().total_millis(), 15);
        assert_eq!(snapshot.native_execution().max_millis(), 5);
        assert_eq!(snapshot.settlement().total_millis(), 45);
        assert_eq!(snapshot.applied_to_settlement().total_millis(), 60);
        assert_eq!(snapshot.verification_observation().samples(), 3);
        assert_eq!(snapshot.verification_observation().total_millis(), 57);
        assert_eq!(snapshot.settlement_events().samples(), 4);
        assert_eq!(snapshot.settlement_events().total(), 10);
        assert_eq!(snapshot.settlement_events().max(), 4);
        assert_eq!(
            snapshot.native_execution().bucket_counts()[duration_bucket(1)],
            1
        );
        assert_eq!(
            snapshot.native_execution().bucket_counts()[duration_bucket(4)],
            1
        );
        assert_eq!(
            snapshot.native_execution().bucket_counts()[duration_bucket(5)],
            2
        );
        let debug = format!("{metrics:?}");
        assert!(!debug.contains("action-metrics.example.test"));
        assert!(!debug.contains("private"));
    }

    #[test]
    fn replay_authority_shape_and_budget_refusals_are_transactional() {
        let manifest = make_manifest(60, 2);
        let supervisor = supervisor(&manifest, 61);
        let mut metrics =
            AgentRunActionPerformanceMetrics::try_new(&manifest, &supervisor).expect("metrics");
        let one = SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(10).expect("batch"),
            SemanticEffectClass::LocalWrite,
            1,
            vec![completion(
                &manifest,
                1,
                10,
                10,
                SemanticActionExecutionBackend::FixedSemanticRecipe,
                10,
                11,
                1,
                0,
                12,
            )],
            SemanticActionBatchOutcome::Complete,
            None,
        );
        metrics.record_batch_result(&one).expect("first");
        let before = metrics.snapshot();
        assert_eq!(
            metrics.record_batch_result(&one),
            Err(AgentActionMetricError::BatchReplay)
        );
        assert_eq!(metrics.snapshot(), before);

        let replayed_receipt = SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(11).expect("batch"),
            SemanticEffectClass::LocalWrite,
            1,
            vec![completion(
                &manifest,
                1,
                10,
                10,
                SemanticActionExecutionBackend::FixedSemanticRecipe,
                15,
                16,
                1,
                0,
                17,
            )],
            SemanticActionBatchOutcome::Complete,
            None,
        );
        assert_eq!(
            metrics.record_batch_result(&replayed_receipt),
            Err(AgentActionMetricError::ReceiptReplay)
        );
        assert_eq!(metrics.snapshot(), before);

        let replayed_attempt = SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(12).expect("batch"),
            SemanticEffectClass::LocalWrite,
            1,
            vec![completion(
                &manifest,
                1,
                11,
                10,
                SemanticActionExecutionBackend::FixedSemanticRecipe,
                18,
                19,
                1,
                0,
                20,
            )],
            SemanticActionBatchOutcome::Complete,
            None,
        );
        assert_eq!(
            metrics.record_batch_result(&replayed_attempt),
            Err(AgentActionMetricError::ReceiptReplay)
        );
        assert_eq!(metrics.snapshot(), before);

        let foreign = make_manifest(62, 2);
        let foreign_result = SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(11).expect("batch"),
            SemanticEffectClass::LocalWrite,
            1,
            vec![completion(
                &foreign,
                1,
                11,
                11,
                SemanticActionExecutionBackend::FixedSemanticRecipe,
                20,
                21,
                1,
                0,
                22,
            )],
            SemanticActionBatchOutcome::Complete,
            None,
        );
        assert_eq!(
            metrics.record_batch_result(&foreign_result),
            Err(AgentActionMetricError::Authority)
        );
        assert_eq!(metrics.snapshot(), before);

        let malformed = SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(12).expect("batch"),
            SemanticEffectClass::LocalWrite,
            2,
            vec![completion(
                &manifest,
                1,
                12,
                12,
                SemanticActionExecutionBackend::FixedSemanticRecipe,
                30,
                31,
                1,
                0,
                32,
            )],
            SemanticActionBatchOutcome::Complete,
            None,
        );
        assert_eq!(
            metrics.record_batch_result(&malformed),
            Err(AgentActionMetricError::Invariant)
        );
        assert_eq!(metrics.snapshot(), before);

        let stopped = SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(13).expect("batch"),
            SemanticEffectClass::LocalWrite,
            2,
            vec![completion(
                &manifest,
                1,
                13,
                13,
                SemanticActionExecutionBackend::FixedSemanticRecipe,
                40,
                41,
                1,
                0,
                42,
            )],
            SemanticActionBatchOutcome::Stopped {
                reason: SemanticActionBatchStopReason::UnpredictedState,
                remaining: 1,
            },
            None,
        );
        metrics
            .record_batch_result(&stopped)
            .expect("second action");
        let full = metrics.snapshot();
        assert_eq!(full.actions(), 2);
        assert_eq!(full.stopped_batches(), 1);

        let over = SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(14).expect("batch"),
            SemanticEffectClass::LocalWrite,
            1,
            vec![completion(
                &manifest,
                1,
                14,
                14,
                SemanticActionExecutionBackend::FixedSemanticRecipe,
                50,
                51,
                1,
                0,
                52,
            )],
            SemanticActionBatchOutcome::Complete,
            None,
        );
        assert_eq!(
            metrics.record_batch_result(&over),
            Err(AgentActionMetricError::Budget)
        );
        assert_eq!(metrics.snapshot(), full);
    }

    #[test]
    fn histogram_overflow_bucket_and_constructor_state_are_explicit() {
        let metrics = AgentActionDurationMetrics::empty()
            .checked_record(0)
            .expect("zero")
            .checked_record(60_001)
            .expect("overflow bucket");
        assert_eq!(metrics.samples(), 2);
        assert_eq!(metrics.bucket_counts()[0], 1);
        assert_eq!(
            metrics.bucket_counts()[AGENT_ACTION_DURATION_BUCKET_COUNT - 1],
            1
        );

        let manifest = make_manifest(70, 2);
        let foreign = make_manifest(71, 2);
        let foreign_supervisor = supervisor(&foreign, 72);
        assert!(matches!(
            AgentRunActionPerformanceMetrics::try_new(&manifest, &foreign_supervisor),
            Err(AgentActionMetricError::Authority)
        ));
        let mut running = supervisor(&manifest, 73);
        let _execution = running
            .start(
                AgentPlanNodeId::from_raw(33),
                crate::AgentSupervisorAttemptId::new(1).expect("attempt"),
            )
            .expect("start");
        assert!(matches!(
            AgentRunActionPerformanceMetrics::try_new(&manifest, &running),
            Err(AgentActionMetricError::StartState)
        ));
    }
}
