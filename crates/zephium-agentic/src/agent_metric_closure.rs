//! Point-in-time closure for one complete set of run-local metric reducers.
//!
//! This optional functional core cross-checks the terminal supervisor against
//! the accounting, audit-progress, action-performance, and committed-input
//! reducers. It owns no telemetry, persistence, clock, task, channel, browser
//! context, provider, or native resource. The resulting value is descriptive
//! evidence only: it grants no policy, model, page, profile, or native authority
//! and is not a production-qualification verdict.

use std::fmt;

use thiserror::Error;

use crate::{
    AgentProviderInputKind, AgentRunAccountingMetrics, AgentRunActionPerformanceMetrics,
    AgentRunManifest, AgentRunManifestId, AgentRunProgressMetrics, AgentRunProgressOutcome,
    AgentRunProviderInputMetrics, AgentRunSupervisor, AgentSupervisorId, AgentSupervisorNodeStatus,
    SemanticActionExecutionBackend,
};

/// Maximum byte size of one copyable metric-closure value.
pub const MAX_AGENT_RUN_METRIC_CLOSURE_BYTES: usize = 192;

/// Content-free evidence that four reducers agreed at one terminal run state.
///
/// This value is non-authorizing and does not prove site, provider, device,
/// resource, latency-distribution, endurance, or release qualification.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentRunMetricClosure {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    supervisor: AgentSupervisorId,
    outcome: AgentRunProgressOutcome,
    events: u64,
    activated_nodes: u32,
    operations: u32,
    model_calls: u32,
    provider_inputs: u32,
    effects: u32,
    actions: u32,
    batches: u32,
    needs_human: u32,
    human_takeovers: u32,
    total_elapsed_millis: u64,
}

const _: () =
    assert!(std::mem::size_of::<AgentRunMetricClosure>() <= MAX_AGENT_RUN_METRIC_CLOSURE_BYTES);

impl AgentRunMetricClosure {
    /// Cross-checks one exact terminal supervisor and its four run-local reducers.
    pub fn try_close(
        manifest: &AgentRunManifest,
        supervisor: &AgentRunSupervisor,
        accounting: &AgentRunAccountingMetrics,
        progress: &AgentRunProgressMetrics,
        actions: &AgentRunActionPerformanceMetrics,
        inputs: &AgentRunProviderInputMetrics,
    ) -> Result<Self, AgentRunMetricClosureError> {
        let supervisor_id = supervisor.id();
        if !supervisor.topology().matches_manifest(manifest)
            || !accounting.matches_metric_scope(manifest, supervisor_id)
            || !progress.matches_metric_scope(manifest, supervisor_id)
            || !actions.matches_metric_scope(manifest, supervisor_id)
            || !inputs.matches_metric_scope(manifest, supervisor_id)
        {
            return Err(AgentRunMetricClosureError::Authority);
        }

        let status = supervisor.status();
        if status.is_sealed()
            || status.live() != 0
            || status.executing() != 0
            || status.queued() != 0
            || status.waiting() != 0
            || status.cancelling() != 0
            || status.contexts() != 0
            || status.activated() == 0
            || status.terminal() != status.activated()
        {
            return Err(AgentRunMetricClosureError::SupervisorIncomplete);
        }

        let progress_snapshot = progress.snapshot();
        let activated_nodes =
            u32::try_from(status.activated()).map_err(|_| AgentRunMetricClosureError::Overflow)?;
        let terminal_nodes =
            u32::try_from(status.terminal()).map_err(|_| AgentRunMetricClosureError::Overflow)?;
        let outcome = progress_snapshot
            .outcome()
            .ok_or(AgentRunMetricClosureError::ProgressIncomplete)?;
        let total_elapsed_millis = progress_snapshot
            .total_elapsed_millis()
            .ok_or(AgentRunMetricClosureError::ProgressIncomplete)?;
        if progress_snapshot.activated_nodes() != activated_nodes
            || progress_snapshot.terminal_nodes() != terminal_nodes
        {
            return Err(AgentRunMetricClosureError::ProgressIncomplete);
        }
        let root = supervisor.topology().root();
        let supervisor_outcome = match supervisor.node_status(root) {
            Some(AgentSupervisorNodeStatus::Succeeded) => AgentRunProgressOutcome::Succeeded,
            Some(AgentSupervisorNodeStatus::Failed(failure)) => {
                AgentRunProgressOutcome::Failed(failure)
            }
            Some(AgentSupervisorNodeStatus::Cancelled(cancellation)) => {
                AgentRunProgressOutcome::Cancelled(cancellation.reason())
            }
            Some(
                AgentSupervisorNodeStatus::Queued
                | AgentSupervisorNodeStatus::Running(_)
                | AgentSupervisorNodeStatus::Waiting(_)
                | AgentSupervisorNodeStatus::Cancelling(_),
            )
            | None => return Err(AgentRunMetricClosureError::SupervisorIncomplete),
        };
        if outcome != supervisor_outcome {
            return Err(AgentRunMetricClosureError::OutcomeMismatch);
        }

        let accounting_snapshot = accounting.snapshot();
        let model = accounting_snapshot.model();
        let effects = accounting_snapshot.effects();
        let action_snapshot = actions.snapshot();
        let input_snapshot = inputs.snapshot();

        require_equal_sum(
            model.calls(),
            &[
                model.completed(),
                model.provider_failed(),
                model.cancelled(),
            ],
        )?;
        require_equal_sum(
            model.calls(),
            &[
                model.exact(),
                model.priced_ceiling(),
                model.reservation_ceiling(),
            ],
        )?;
        require_equal_sum(effects.attempts(), &[effects.verified(), effects.failed()])?;
        require_equal_sum(
            accounting_snapshot.operations(),
            &[model.calls(), effects.attempts()],
        )?;
        require_equal_sum(
            action_snapshot.batches(),
            &[
                action_snapshot.complete_batches(),
                action_snapshot.stopped_batches(),
                action_snapshot.failed_batches(),
            ],
        )?;
        require_equal_sum(
            action_snapshot.actions(),
            &[
                action_snapshot.verified_actions(),
                action_snapshot.failed_actions(),
            ],
        )?;
        require_equal_sum(
            action_snapshot.failed_actions(),
            &[
                action_snapshot.before_verification_failures(),
                action_snapshot.after_execution_failures(),
            ],
        )?;
        let applied_actions = checked_sum(&[
            action_snapshot.verified_actions(),
            action_snapshot.after_execution_failures(),
        ])?;
        require_action_sample_count(action_snapshot, applied_actions)?;
        require_accounting_node_totals(accounting)?;
        require_input_totals(inputs)?;

        if accounting.model_receipt_ids() != inputs.receipt_ids()
            || model.calls() != input_snapshot.calls()
        {
            return Err(AgentRunMetricClosureError::ModelCoverage);
        }
        let model_duration_samples = progress_snapshot.model().map_or(0, |value| value.samples());
        if model_duration_samples != model.calls() {
            return Err(AgentRunMetricClosureError::ModelCoverage);
        }

        if accounting.effect_receipt_ids() != actions.effect_receipt_ids()
            || accounting.effect_attempt_ids() != actions.effect_attempt_ids()
            || effects.attempts() != action_snapshot.actions()
            || effects.verified() != action_snapshot.verified_actions()
            || effects.failed() != action_snapshot.failed_actions()
        {
            return Err(AgentRunMetricClosureError::EffectCoverage);
        }
        let effect_duration_samples = progress_snapshot
            .effect()
            .map_or(0, |value| value.samples());
        if effect_duration_samples != effects.attempts() {
            return Err(AgentRunMetricClosureError::EffectCoverage);
        }

        Ok(Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            supervisor: supervisor_id,
            outcome,
            events: progress_snapshot.events(),
            activated_nodes,
            operations: accounting_snapshot.operations(),
            model_calls: model.calls(),
            provider_inputs: input_snapshot.calls(),
            effects: effects.attempts(),
            actions: action_snapshot.actions(),
            batches: action_snapshot.batches(),
            needs_human: progress_snapshot.needs_human().total(),
            human_takeovers: progress_snapshot.human_takeovers(),
            total_elapsed_millis,
        })
    }

    /// Exact immutable manifest identity at closure.
    pub const fn manifest(self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact mutable supervisor incarnation at closure.
    pub const fn supervisor(self) -> AgentSupervisorId {
        self.supervisor
    }

    /// Exact root terminal outcome cross-checked against the supervisor.
    pub const fn outcome(self) -> AgentRunProgressOutcome {
        self.outcome
    }

    /// Canonical audit events admitted by the progress reducer.
    pub const fn events(self) -> u64 {
        self.events
    }

    /// Activated nodes proven terminal and represented in progress metrics.
    pub const fn activated_nodes(self) -> u32 {
        self.activated_nodes
    }

    /// Terminal model calls plus dispatched effects.
    pub const fn operations(self) -> u32 {
        self.operations
    }

    /// Exact model-call receipts cross-checked against committed inputs.
    pub const fn model_calls(self) -> u32 {
        self.model_calls
    }

    /// Exact committed provider inputs cross-checked against model receipts.
    pub const fn provider_inputs(self) -> u32 {
        self.provider_inputs
    }

    /// Exact dispatched-effect receipts cross-checked against action terminals.
    pub const fn effects(self) -> u32 {
        self.effects
    }

    /// Exact action terminals cross-checked against effect receipts.
    pub const fn actions(self) -> u32 {
        self.actions
    }

    /// Exact action-batch terminals.
    pub const fn batches(self) -> u32 {
        self.batches
    }

    /// Policy-derived human-pause transitions observed in the audit stream.
    pub const fn needs_human(self) -> u32 {
        self.needs_human
    }

    /// Distinct human-takeover cancellation updates observed at terminal nodes.
    pub const fn human_takeovers(self) -> u32 {
        self.human_takeovers
    }

    /// Observed root queued-to-terminal duration.
    pub const fn total_elapsed_millis(self) -> u64 {
        self.total_elapsed_millis
    }
}

impl fmt::Debug for AgentRunMetricClosure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunMetricClosure")
            .field("manifest", &self.manifest)
            .field("manifest_guard", &"[redacted]")
            .field("supervisor", &self.supervisor)
            .field("outcome", &self.outcome)
            .field("events", &self.events)
            .field("activated_nodes", &self.activated_nodes)
            .field("operations", &self.operations)
            .field("model_calls", &self.model_calls)
            .field("provider_inputs", &self.provider_inputs)
            .field("effects", &self.effects)
            .field("actions", &self.actions)
            .field("batches", &self.batches)
            .field("needs_human", &self.needs_human)
            .field("human_takeovers", &self.human_takeovers)
            .field("total_elapsed_millis", &self.total_elapsed_millis)
            .field("authority", &"[none]")
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed refusal while cross-checking terminal run-local metric coverage.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentRunMetricClosureError {
    /// Manifest revision, topology, supervisor, or reducer scope did not match.
    #[error("agent metric closure authority mismatched")]
    Authority,
    /// The supervisor remains live, owns resources, is sealed, or is not terminal.
    #[error("agent metric closure requires an unsealed terminal supervisor")]
    SupervisorIncomplete,
    /// Audit progress did not cover every activated terminal node and root outcome.
    #[error("agent metric closure progress stream is incomplete")]
    ProgressIncomplete,
    /// Audit-derived root outcome contradicted the terminal supervisor.
    #[error("agent metric closure root outcome mismatched")]
    OutcomeMismatch,
    /// Model receipts, committed inputs, or model-duration samples were incomplete.
    #[error("agent metric closure model coverage mismatched")]
    ModelCoverage,
    /// Effect receipts, action terminals, attempts, or durations were incomplete.
    #[error("agent metric closure effect coverage mismatched")]
    EffectCoverage,
    /// One supposedly closed reducer carried contradictory aggregate partitions.
    #[error("agent metric closure aggregate invariant failed")]
    Invariant,
    /// Checked count conversion or arithmetic overflowed.
    #[error("agent metric closure arithmetic overflowed")]
    Overflow,
}

fn checked_sum(values: &[u32]) -> Result<u32, AgentRunMetricClosureError> {
    values.iter().copied().try_fold(0_u32, |total, value| {
        total
            .checked_add(value)
            .ok_or(AgentRunMetricClosureError::Overflow)
    })
}

fn require_equal_sum(expected: u32, values: &[u32]) -> Result<(), AgentRunMetricClosureError> {
    if checked_sum(values)? != expected {
        return Err(AgentRunMetricClosureError::Invariant);
    }
    Ok(())
}

fn require_action_sample_count(
    snapshot: crate::AgentRunActionPerformanceSnapshot,
    applied_actions: u32,
) -> Result<(), AgentRunMetricClosureError> {
    let backend_samples = checked_sum(&[
        snapshot
            .backends()
            .count(SemanticActionExecutionBackend::FixedSemanticRecipe),
        snapshot
            .backends()
            .count(SemanticActionExecutionBackend::EngineNativeInput),
        snapshot
            .backends()
            .count(SemanticActionExecutionBackend::InProcessAccessibility),
    ])?;
    let verification_samples = checked_sum(&[
        snapshot.verification_observation().samples(),
        snapshot.unorderable_verification_observations(),
    ])?;
    if backend_samples != applied_actions
        || snapshot.native_execution().samples() != applied_actions
        || snapshot.settlement().samples() != applied_actions
        || snapshot.applied_to_settlement().samples() != applied_actions
        || snapshot.settlement_events().samples() != applied_actions
        || verification_samples != applied_actions
        || checked_sum(&snapshot.native_execution().bucket_counts())? != applied_actions
        || checked_sum(&snapshot.settlement().bucket_counts())? != applied_actions
        || checked_sum(&snapshot.applied_to_settlement().bucket_counts())? != applied_actions
        || checked_sum(&snapshot.verification_observation().bucket_counts())?
            != snapshot.verification_observation().samples()
    {
        return Err(AgentRunMetricClosureError::Invariant);
    }
    Ok(())
}

fn require_accounting_node_totals(
    accounting: &AgentRunAccountingMetrics,
) -> Result<(), AgentRunMetricClosureError> {
    let snapshot = accounting.snapshot();
    let mut operations = 0_u32;
    let mut model_calls = 0_u32;
    let mut effects = 0_u32;
    for node in accounting.nodes() {
        operations = operations
            .checked_add(node.operations())
            .ok_or(AgentRunMetricClosureError::Overflow)?;
        model_calls = model_calls
            .checked_add(node.model_calls())
            .ok_or(AgentRunMetricClosureError::Overflow)?;
        effects = effects
            .checked_add(node.effects())
            .ok_or(AgentRunMetricClosureError::Overflow)?;
    }
    let mut priced_calls = 0_u32;
    for schedule in accounting.pricing_schedules() {
        priced_calls = priced_calls
            .checked_add(schedule.calls())
            .ok_or(AgentRunMetricClosureError::Overflow)?;
    }
    if operations != snapshot.operations()
        || model_calls != snapshot.model().calls()
        || effects != snapshot.effects().attempts()
        || priced_calls != snapshot.model().priced_ceiling()
    {
        return Err(AgentRunMetricClosureError::Invariant);
    }
    Ok(())
}

fn require_input_totals(
    inputs: &AgentRunProviderInputMetrics,
) -> Result<(), AgentRunMetricClosureError> {
    let snapshot = inputs.snapshot();
    let mut node_calls = 0_u32;
    for node in inputs.nodes() {
        node_calls = node_calls
            .checked_add(node.calls())
            .ok_or(AgentRunMetricClosureError::Overflow)?;
    }
    let mut kind_calls = 0_u32;
    let mut serialized_request_bytes = 0_u64;
    let mut disclosed_bytes = 0_u64;
    for kind in AgentProviderInputKind::ALL {
        let metrics = snapshot.kind(kind);
        kind_calls = kind_calls
            .checked_add(metrics.calls())
            .ok_or(AgentRunMetricClosureError::Overflow)?;
        serialized_request_bytes = serialized_request_bytes
            .checked_add(metrics.serialized_request_bytes())
            .ok_or(AgentRunMetricClosureError::Overflow)?;
        disclosed_bytes = disclosed_bytes
            .checked_add(metrics.disclosed_bytes())
            .ok_or(AgentRunMetricClosureError::Overflow)?;
    }
    if node_calls != snapshot.calls()
        || kind_calls != snapshot.calls()
        || serialized_request_bytes != snapshot.serialized_request_bytes()
        || disclosed_bytes != snapshot.disclosed_bytes()
    {
        return Err(AgentRunMetricClosureError::Invariant);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentAccountScope, AgentActiveEffect, AgentActiveModelCall, AgentAuditEventId,
        AgentAuditLedger, AgentDelegationSpec, AgentDelegationTopology, AgentEffectId,
        AgentEffectReceipt, AgentEffectScope, AgentEffectSettlement, AgentModelCallId,
        AgentModelCallReceipt, AgentModelCallSettlement, AgentPlanLeaseId, AgentPlanNodeAuthority,
        AgentPlanNodeId, AgentPlanNodeScope, AgentPolicyInstant, AgentProviderInputMetricReceipt,
        AgentProviderInputMetrics, AgentProviderSemanticInputStats, AgentRunBudget, AgentRunScope,
        AgentSupervisorAttemptId, AgentSupervisorCompletion, AgentSupervisorFailure,
        ContextCapabilities, ContextCapability, ContextId, ContextIdentity, ContextKind,
        ContextOperationId, ContextRegistry, ContextRunId, ContextSettlement,
        SemanticActionAttemptId, SemanticActionBatchCompletion, SemanticActionBatchId,
        SemanticActionBatchOutcome, SemanticActionBatchResult, SemanticActionExecutionApplied,
        SemanticActionExecutionInstant, SemanticActionKind, SemanticActionNextState,
        SemanticEffectClass, SemanticEffectProofKind, SemanticEncodingStats, SemanticOrigin,
        SemanticSensitivity, SemanticSettleInstant, SemanticTokenCountQuality,
    };
    use zephium_core::ids::ProfileId;

    const ROOT: AgentPlanNodeId = AgentPlanNodeId::from_raw(1);

    fn manifest(id: u128, operations: u32) -> AgentRunManifest {
        let profile = ProfileId::from(1);
        let origin =
            SemanticOrigin::parse("https://metric-closure.example.test/private?credential=hidden")
                .expect("origin");
        let effects =
            AgentEffectScope::try_new(&[SemanticEffectClass::LocalWrite]).expect("effects");
        let budget = AgentRunBudget::try_new(operations, 1_000, 1_000, 1).expect("budget");
        AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(id),
            ContextRunId::from_raw(2),
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
            AgentPolicyInstant::from_millis(100),
            AgentPolicyInstant::from_millis(10_000),
            vec![AgentPlanNodeScope::new(
                ROOT,
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
            AgentDelegationTopology::try_new(manifest, vec![AgentDelegationSpec::new(ROOT, None)])
                .expect("topology"),
        )
    }

    fn event(value: u64) -> AgentAuditEventId {
        AgentAuditEventId::new(value).expect("event")
    }

    fn attempt(value: u64) -> AgentSupervisorAttemptId {
        AgentSupervisorAttemptId::new(value).expect("attempt")
    }

    fn record(
        ledger: &mut AgentAuditLedger,
        progress: &mut AgentRunProgressMetrics,
        supervisor: &AgentRunSupervisor,
        id: u64,
        millis: u64,
    ) {
        let event = ledger
            .record_current(
                supervisor,
                ROOT,
                event(id),
                AgentPolicyInstant::from_millis(millis),
            )
            .expect("audit event");
        progress.record_event(event).expect("progress event");
    }

    fn input_receipt(manifest: &AgentRunManifest, call: u64) -> AgentProviderInputMetricReceipt {
        AgentProviderInputMetricReceipt::for_reducer_test(
            manifest,
            AgentModelCallId::new(call).expect("call"),
            AgentPlanLeaseId::from_raw(10),
            ROOT,
            AgentProviderInputMetrics::for_reducer_test(
                1_000,
                AgentProviderSemanticInputStats::Observation(
                    SemanticEncodingStats::for_input_metrics_test(100, 8, 2, 7, 2),
                ),
                Some((20, SemanticTokenCountQuality::ExactLocal)),
                None,
            ),
        )
    }

    fn model_receipt(manifest: &AgentRunManifest, call: u64) -> AgentModelCallReceipt {
        AgentModelCallReceipt::for_progress_test(
            manifest,
            AgentModelCallId::new(call).expect("call"),
            AgentPlanLeaseId::from_raw(10),
            ROOT,
            AgentModelCallSettlement::Completed,
        )
    }

    fn effect_receipt(manifest: &AgentRunManifest, action_attempt: u64) -> AgentEffectReceipt {
        AgentEffectReceipt::for_progress_test(
            manifest,
            AgentEffectId::new(1).expect("effect"),
            AgentPlanLeaseId::from_raw(10),
            ROOT,
            SemanticEffectClass::LocalWrite,
            SemanticActionAttemptId::new(action_attempt).expect("action attempt"),
            AgentEffectSettlement::Verified(SemanticEffectProofKind::TargetState),
        )
    }

    fn context() -> crate::ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(20),
            ContextRunId::from_raw(2),
            ProfileId::from(1),
            ContextKind::Owned,
        );
        let mut registry = ContextRegistry::new();
        registry
            .reserve(
                identity,
                ContextCapabilities::try_new(
                    ContextKind::Owned,
                    &[ContextCapability::Observe, ContextCapability::Act],
                )
                .expect("capabilities"),
            )
            .expect("reserve");
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

    fn action_batch(manifest: &AgentRunManifest, action_attempt: u64) -> SemanticActionBatchResult {
        let receipt = effect_receipt(manifest, action_attempt);
        let completion = SemanticActionBatchCompletion::for_action_metrics_test(
            1,
            SemanticActionKind::Click,
            receipt,
            SemanticActionExecutionApplied::for_action_metrics_test(
                SemanticActionExecutionBackend::EngineNativeInput,
                SemanticActionExecutionInstant::from_millis(140),
                SemanticActionExecutionInstant::from_millis(142),
            ),
            2,
            3,
            SemanticSettleInstant::from_millis(145),
            SemanticActionAttemptId::new(action_attempt).expect("action attempt"),
            SemanticEffectProofKind::TargetState,
            SemanticSettleInstant::from_millis(149),
            context(),
            SemanticActionNextState::Diff,
        );
        SemanticActionBatchResult::for_action_metrics_test(
            SemanticActionBatchId::new(1).expect("batch"),
            SemanticEffectClass::LocalWrite,
            1,
            vec![completion],
            SemanticActionBatchOutcome::Complete,
            None,
        )
    }

    struct Fixture {
        manifest: AgentRunManifest,
        supervisor: AgentRunSupervisor,
        accounting: AgentRunAccountingMetrics,
        progress: AgentRunProgressMetrics,
        actions: AgentRunActionPerformanceMetrics,
        inputs: AgentRunProviderInputMetrics,
        ledger: AgentAuditLedger,
    }

    impl Fixture {
        fn new(id: u128) -> Self {
            Self::with_operations(id, 8)
        }

        fn with_operations(id: u128, operations: u32) -> Self {
            let manifest = manifest(id, operations);
            let supervisor = supervisor(&manifest, id as u64);
            let accounting =
                AgentRunAccountingMetrics::try_new(&manifest, &supervisor).expect("accounting");
            let progress =
                AgentRunProgressMetrics::try_new(&manifest, &supervisor).expect("progress");
            let actions =
                AgentRunActionPerformanceMetrics::try_new(&manifest, &supervisor).expect("actions");
            let inputs =
                AgentRunProviderInputMetrics::try_new(&manifest, &supervisor).expect("inputs");
            let ledger = AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
            Self {
                manifest,
                supervisor,
                accounting,
                progress,
                actions,
                inputs,
                ledger,
            }
        }

        fn run_success(&mut self, input_call: u64, action_attempt: u64) {
            record(
                &mut self.ledger,
                &mut self.progress,
                &self.supervisor,
                1,
                100,
            );
            let execution = self.supervisor.start(ROOT, attempt(1)).expect("start");
            record(
                &mut self.ledger,
                &mut self.progress,
                &self.supervisor,
                2,
                110,
            );

            let active_model = AgentActiveModelCall::for_progress_test(
                &self.manifest,
                AgentModelCallId::new(1).expect("model"),
                AgentPlanLeaseId::from_raw(10),
                ROOT,
            );
            self.supervisor
                .record_active_model_call(&execution, &active_model)
                .expect("active model");
            record(
                &mut self.ledger,
                &mut self.progress,
                &self.supervisor,
                3,
                120,
            );
            let model = model_receipt(&self.manifest, 1);
            self.supervisor
                .record_model_call_result(&execution, model)
                .expect("model result");
            record(
                &mut self.ledger,
                &mut self.progress,
                &self.supervisor,
                4,
                130,
            );
            self.accounting
                .record_model_receipt(model)
                .expect("model accounting");
            self.inputs
                .record(input_receipt(&self.manifest, input_call))
                .expect("input accounting");

            let active_effect = AgentActiveEffect::for_progress_test(
                &self.manifest,
                AgentEffectId::new(1).expect("effect"),
                AgentPlanLeaseId::from_raw(10),
                ROOT,
                SemanticEffectClass::LocalWrite,
                SemanticActionAttemptId::new(1).expect("action attempt"),
            );
            self.supervisor
                .record_active_effect(&execution, &active_effect)
                .expect("active effect");
            record(
                &mut self.ledger,
                &mut self.progress,
                &self.supervisor,
                5,
                140,
            );
            let effect = effect_receipt(&self.manifest, 1);
            self.supervisor
                .record_effect_result(&execution, effect)
                .expect("effect result");
            record(
                &mut self.ledger,
                &mut self.progress,
                &self.supervisor,
                6,
                150,
            );
            self.accounting
                .record_effect_receipt(effect)
                .expect("effect accounting");
            self.actions
                .record_batch_result(&action_batch(&self.manifest, action_attempt))
                .expect("action accounting");

            self.supervisor
                .complete(execution, AgentSupervisorCompletion::Succeeded)
                .expect("complete");
            record(
                &mut self.ledger,
                &mut self.progress,
                &self.supervisor,
                7,
                160,
            );
        }

        fn close(&self) -> Result<AgentRunMetricClosure, AgentRunMetricClosureError> {
            AgentRunMetricClosure::try_close(
                &self.manifest,
                &self.supervisor,
                &self.accounting,
                &self.progress,
                &self.actions,
                &self.inputs,
            )
        }
    }

    #[test]
    fn exact_nonzero_receipt_and_audit_coverage_closes_without_authority() {
        let mut fixture = Fixture::new(1);
        fixture.run_success(1, 1);
        let closure = fixture.close().expect("metric closure");

        assert_eq!(closure.manifest(), fixture.manifest.id());
        assert_eq!(closure.supervisor(), fixture.supervisor.id());
        assert_eq!(closure.outcome(), AgentRunProgressOutcome::Succeeded);
        assert_eq!(closure.events(), 7);
        assert_eq!(closure.activated_nodes(), 1);
        assert_eq!(closure.operations(), 2);
        assert_eq!(closure.model_calls(), 1);
        assert_eq!(closure.provider_inputs(), 1);
        assert_eq!(closure.effects(), 1);
        assert_eq!(closure.actions(), 1);
        assert_eq!(closure.batches(), 1);
        assert_eq!(closure.needs_human(), 0);
        assert_eq!(closure.human_takeovers(), 0);
        assert_eq!(closure.total_elapsed_millis(), 60);
        assert!(std::mem::size_of::<AgentRunMetricClosure>() <= MAX_AGENT_RUN_METRIC_CLOSURE_BYTES);

        let debug = format!("{closure:?}");
        assert!(debug.contains("[redacted]"));
        assert!(debug.contains("[none]"));
        assert!(!debug.contains("metric-closure.example.test"));
        assert!(!debug.contains("credential"));
        assert!(!debug.contains("hidden"));
    }

    #[test]
    fn active_supervisor_and_incomplete_progress_refuse_closure() {
        let mut active = Fixture::new(2);
        assert_eq!(
            active.close(),
            Err(AgentRunMetricClosureError::SupervisorIncomplete)
        );

        record(
            &mut active.ledger,
            &mut active.progress,
            &active.supervisor,
            1,
            100,
        );
        let execution = active.supervisor.start(ROOT, attempt(1)).expect("start");
        record(
            &mut active.ledger,
            &mut active.progress,
            &active.supervisor,
            2,
            110,
        );
        active
            .supervisor
            .complete(execution, AgentSupervisorCompletion::Succeeded)
            .expect("complete");
        assert_eq!(
            active.close(),
            Err(AgentRunMetricClosureError::ProgressIncomplete)
        );
    }

    #[test]
    fn same_public_id_foreign_revision_refuses_before_terminal_state() {
        let fixture = Fixture::new(3);
        let changed = manifest(3, 7);
        assert_eq!(changed.id(), fixture.manifest.id());
        assert!(!changed.matches_revision(&fixture.manifest));
        let changed_supervisor = supervisor(&changed, 3);

        assert_eq!(
            AgentRunMetricClosure::try_close(
                &changed,
                &changed_supervisor,
                &fixture.accounting,
                &fixture.progress,
                &fixture.actions,
                &fixture.inputs,
            ),
            Err(AgentRunMetricClosureError::Authority)
        );
    }

    #[test]
    fn closure_equality_retains_the_private_manifest_revision() {
        let mut first = Fixture::with_operations(7, 8);
        first.run_success(1, 1);
        let mut changed = Fixture::with_operations(7, 7);
        changed.run_success(1, 1);

        let first = first.close().expect("first closure");
        let changed = changed.close().expect("changed closure");
        assert_eq!(first.manifest(), changed.manifest());
        assert_ne!(first, changed);
    }

    #[test]
    fn exact_model_and_effect_identity_coverage_is_required() {
        let mut wrong_call = Fixture::new(4);
        wrong_call.run_success(2, 1);
        assert_eq!(
            wrong_call.close(),
            Err(AgentRunMetricClosureError::ModelCoverage)
        );

        let mut wrong_attempt = Fixture::new(5);
        wrong_attempt.run_success(1, 2);
        assert_eq!(
            wrong_attempt.close(),
            Err(AgentRunMetricClosureError::EffectCoverage)
        );
    }

    #[test]
    fn audit_outcome_must_match_the_exact_terminal_supervisor() {
        let mut fixture = Fixture::new(6);
        fixture.run_success(1, 1);

        let mut failed = supervisor(&fixture.manifest, 6);
        let execution = failed.start(ROOT, attempt(1)).expect("start failed run");
        failed
            .complete(
                execution,
                AgentSupervisorCompletion::Failed(AgentSupervisorFailure::ProviderFailed),
            )
            .expect("fail root");
        assert_eq!(
            AgentRunMetricClosure::try_close(
                &fixture.manifest,
                &failed,
                &fixture.accounting,
                &fixture.progress,
                &fixture.actions,
                &fixture.inputs,
            ),
            Err(AgentRunMetricClosureError::OutcomeMismatch)
        );
    }
}
