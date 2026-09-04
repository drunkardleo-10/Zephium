//! Closed Terra controller composition.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use thiserror::Error;
use zephium_agent_model_catalog::{
    settle_terra_provider_terminal, try_terra_provider_exact_call_config,
    TerraProviderTerminalOwner, TerraProviderTerminalSettlement,
    TerraProviderTerminalSettlementError, TERRA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS,
    TERRA_MAX_OUTPUT_TOKENS, TERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS,
    TERRA_STANDARD_RATE_MAX_INPUT_TOKENS,
};
use zephium_agent_provider_transport::{
    AgentProviderAbortReason, AgentProviderAttempt, AgentProviderCredential,
    AgentProviderTransport, AgentProviderTransportConfig,
};
use zephium_agent_runtime::{
    AgentRuntimeBrowser, AgentRuntimeController, AgentRuntimeControllerFuture,
    AgentRuntimeControllerTerminalClass, AgentRuntimeEvent, AgentRuntimeWorker,
    MAX_AGENT_RUNTIME_SIGNAL_CAPACITY, MAX_AGENT_RUNTIME_TERMINAL_CAPACITY,
};
use zephium_agentic::{
    encode_semantic_diff, encode_semantic_observation, encode_semantic_runtime_invocation,
    AgentAuditDeliveryId, AgentAuditDispatch, AgentAuditEventId, AgentAuditLedger, AgentAuditPort,
    AgentContextAccountBinding, AgentDelegationSpec, AgentDelegationTopology, AgentModelCallBudget,
    AgentModelCallId, AgentModelCallReceipt, AgentModelCallRequest, AgentModelCallSettlement,
    AgentNodeExecution, AgentPlanLeaseBinding, AgentPolicyInstant, AgentPreparedObservationRequest,
    AgentProviderBatchDisposition, AgentProviderCallConfig, AgentProviderCancellation,
    AgentProviderDiffRequestDraft, AgentProviderDisclosureStage, AgentProviderFailureClass,
    AgentProviderImmediateSettlement, AgentProviderObjective, AgentProviderPolicySettlement,
    AgentProviderProtocolError, AgentProviderProtocolEvent, AgentProviderRequestSettlement,
    AgentProviderStopReason, AgentProviderStreamConclusion, AgentProviderTransportInput,
    AgentProviderTransportOutcome, AgentProviderTransportResult, AgentRunAccountingMetrics,
    AgentRunActionPerformanceMetrics, AgentRunMetricClosure, AgentRunPolicy,
    AgentRunProgressMetrics, AgentRunProviderInputMetrics, AgentRunSupervisor,
    AgentSupervisorAttemptId, AgentSupervisorCancellationId, AgentSupervisorCancellationReason,
    AgentSupervisorCompletion, AgentSupervisorFailure, AgentSupervisorId, ContextDispatch,
    ContextNativeEvent, SemanticInvocationId, SemanticModelEncodingBudget,
    SemanticModelEncodingError, SemanticObservationAssembler, SemanticObservationRequest,
    SemanticRuntimeBudget, SemanticSnapshotGeneration, MAX_AGENT_AUDIT_DELIVERY_EVENTS,
};

#[cfg(feature = "probe-harness")]
use crate::probe::TerraProbeVerifiedTransition;

/// This text-only/discarding vertical never needs Terra's catalog-wide 128k
/// output ceiling. Keeping its own 8k ceiling bounds wasted generation and
/// the conservative reservation without widening catalog authority.
const TERRA_CONTROLLER_MAX_OUTPUT_TOKENS: u32 = 8_192;
/// Fixed Terra worst-case model-call reservation, in micro-USD.
const TERRA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD: u64 = 778_304;
/// Controller-level cap aligned with the transport's fixed request ceiling.
const MAX_TERRA_CONTROLLER_HARD_DEADLINE: Duration = Duration::from_secs(10 * 60);
const MAX_DEFERRED_RUNTIME_EVENTS: usize =
    MAX_AGENT_RUNTIME_TERMINAL_CAPACITY + MAX_AGENT_RUNTIME_SIGNAL_CAPACITY;
#[cfg(feature = "probe-harness")]
const MAX_TERRA_PROBE_MODEL_TURNS: u8 = 2;

const _: () = {
    assert!(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS < TERRA_MAX_OUTPUT_TOKENS);
    assert!(
        TERRA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD
            == TERRA_STANDARD_RATE_MAX_INPUT_TOKENS
                * TERRA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS
                / 1_000_000
                + (TERRA_CONTROLLER_MAX_OUTPUT_TOKENS as u64)
                    * TERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS
                    / 1_000_000
    );
};

/// Exact shell-minted identifiers for one deterministic controller turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerraControllerIds {
    supervisor: AgentSupervisorId,
    attempt: AgentSupervisorAttemptId,
    cancellation: AgentSupervisorCancellationId,
    model_call: AgentModelCallId,
    audit_events: [AgentAuditEventId; 4],
    audit_delivery: AgentAuditDeliveryId,
}

impl TerraControllerIds {
    /// Joins the exact monotonic identifiers required by the single turn.
    ///
    /// The four event identities represent the root start, model-active,
    /// model-terminal, and root-terminal projections.  Their ordering is
    /// validated before any supervisor or policy authority is constructed.
    pub fn try_new(
        supervisor: AgentSupervisorId,
        attempt: AgentSupervisorAttemptId,
        cancellation: AgentSupervisorCancellationId,
        model_call: AgentModelCallId,
        audit_events: [AgentAuditEventId; 4],
        audit_delivery: AgentAuditDeliveryId,
    ) -> Result<Self, TerraControllerConstructionError> {
        if audit_events
            .iter()
            .zip(audit_events.iter().skip(1))
            .any(|(left, right)| left >= right)
        {
            return Err(TerraControllerConstructionError::AuditIdentifiers);
        }
        Ok(Self {
            supervisor,
            attempt,
            cancellation,
            model_call,
            audit_events,
            audit_delivery,
        })
    }

    /// Returns the model-call identity to the crate-internal probe composer.
    ///
    /// This remains unavailable outside the controller crate: the diagnostic
    /// runner may join the identifier to its provider settlement, but callers
    /// cannot use it to construct independent model-call authority.
    #[cfg(feature = "probe-harness")]
    pub(crate) const fn model_call(self) -> AgentModelCallId {
        self.model_call
    }
}

/// Closed failure from the trusted monotonic controller clock.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TerraControllerClockError {
    /// The trusted clock could not provide a value for this transition.
    #[error("Terra controller clock is unavailable")]
    Unavailable,
    /// The trusted clock regressed or fell outside approved policy lifetime.
    #[error("Terra controller clock returned an invalid policy time")]
    Invalid,
}

/// Narrow trusted source of policy-valid monotonic controller time.
///
/// The controller checks returned values for nondecreasing order and never
/// substitutes manifest timestamps, wall-clock durations, or provider time.
pub trait TerraControllerClock: Send + Sync + 'static {
    /// Returns the exact policy time for the next controller transition.
    fn now(&self) -> Result<AgentPolicyInstant, TerraControllerClockError>;
}

/// One bounded trusted observation and objective for a single Terra turn.
///
/// This owns the objective until the controller's fixed catalog encoder
/// accepts it. Its fields cannot be used to supply a provider configuration,
/// reservation, policy, or direct browser operation.
#[must_use]
pub struct TerraControllerTurnInput {
    account: AgentContextAccountBinding,
    observation: SemanticObservationRequest,
    frame: zephium_agentic::SemanticFrameJoin,
    invocation: SemanticInvocationId,
    snapshot_generation: SemanticSnapshotGeneration,
    objective: String,
}

impl TerraControllerTurnInput {
    /// Joins the caller's exact semantic observation authority and objective.
    pub fn try_new(
        account: AgentContextAccountBinding,
        observation: SemanticObservationRequest,
        frame: zephium_agentic::SemanticFrameJoin,
        invocation: SemanticInvocationId,
        snapshot_generation: SemanticSnapshotGeneration,
        objective: String,
    ) -> Result<Self, TerraControllerConstructionError> {
        if account.context() != observation.context() || frame.context() != observation.context() {
            return Err(TerraControllerConstructionError::Authority);
        }
        Ok(Self {
            account,
            observation,
            frame,
            invocation,
            snapshot_generation,
            objective,
        })
    }
}

impl fmt::Debug for TerraControllerTurnInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerraControllerTurnInput")
            .field("account", &self.account)
            .field("observation", &self.observation)
            .field("frame", &self.frame)
            .field("invocation", &self.invocation)
            .field("snapshot_generation", &self.snapshot_generation)
            .field("objective", &"[redacted]")
            .finish()
    }
}

/// Move-only construction input for one root-node Terra text turn.
#[must_use]
pub struct TerraControllerRunInput {
    manifest: zephium_agentic::AgentRunManifest,
    lease: AgentPlanLeaseBinding,
    account: AgentContextAccountBinding,
    observation: SemanticObservationRequest,
    frame: zephium_agentic::SemanticFrameJoin,
    invocation: SemanticInvocationId,
    snapshot_generation: SemanticSnapshotGeneration,
    objective: AgentProviderObjective,
    ids: TerraControllerIds,
    clock: Arc<dyn TerraControllerClock>,
    deadline: Instant,
}

impl TerraControllerRunInput {
    /// Builds one root-only input bundle and conservatively admits its objective.
    ///
    /// The fixed Terra configuration is constructed internally. Callers cannot
    /// provide a generic provider configuration, tokenizer, reservation, or
    /// prebuilt policy authority that could be mismatched with this manifest.
    pub fn try_new(
        manifest: zephium_agentic::AgentRunManifest,
        lease: AgentPlanLeaseBinding,
        turn: TerraControllerTurnInput,
        ids: TerraControllerIds,
        clock: Arc<dyn TerraControllerClock>,
        deadline: Instant,
    ) -> Result<Self, TerraControllerConstructionError> {
        let now = Instant::now();
        let Some(horizon) = deadline.checked_duration_since(now) else {
            return Err(TerraControllerConstructionError::Deadline);
        };
        if horizon.is_zero() || horizon > MAX_TERRA_CONTROLLER_HARD_DEADLINE {
            return Err(TerraControllerConstructionError::Deadline);
        }
        let Some(root) = manifest.plan_nodes().first() else {
            return Err(TerraControllerConstructionError::Root);
        };
        if manifest.plan_nodes().len() != 1 || lease.node() != root.id() {
            return Err(TerraControllerConstructionError::Authority);
        }
        let TerraControllerTurnInput {
            account,
            observation,
            frame,
            invocation,
            snapshot_generation,
            objective,
        } = turn;
        let config = try_terra_provider_exact_call_config(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS)
            .map_err(|_| TerraControllerConstructionError::Catalog)?;
        let objective =
            AgentProviderObjective::try_admit_conservative_utf8(objective, config.tokenizer())
                .map_err(|_| TerraControllerConstructionError::Objective)?;
        Ok(Self {
            manifest,
            lease,
            account,
            observation,
            frame,
            invocation,
            snapshot_generation,
            objective,
            ids,
            clock,
            deadline,
        })
    }
}

impl fmt::Debug for TerraControllerRunInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerraControllerRunInput")
            .field("manifest", &self.manifest)
            .field("lease", &self.lease)
            .field("account", &self.account)
            .field("observation", &self.observation)
            .field("frame", &self.frame)
            .field("invocation", &self.invocation)
            .field("snapshot_generation", &self.snapshot_generation)
            .field("objective", &"[redacted]")
            .field("ids", &self.ids)
            .field("clock", &"[trusted]")
            .field("deadline", &"[redacted]")
            .finish()
    }
}

/// Content-free construction refusal for a Terra controller turn.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TerraControllerConstructionError {
    /// The manifest did not contain exactly one root plan node.
    #[error("Terra controller requires one root plan node")]
    Root,
    /// Context, frame, lease, or manifest authority did not join exactly.
    #[error("Terra controller authority mismatched")]
    Authority,
    /// The pinned Terra catalog could not construct its fixed call.
    #[error("Terra controller catalog is unavailable")]
    Catalog,
    /// The bounded objective was not admissible.
    #[error("Terra controller objective is invalid")]
    Objective,
    /// The supplied absolute controller deadline was expired or too distant.
    #[error("Terra controller deadline is invalid")]
    Deadline,
    /// The shell-minted audit-event identities were not strictly increasing.
    #[error("Terra controller audit identifiers are invalid")]
    AuditIdentifiers,
    /// The dedicated production provider transport could not be constructed.
    #[error("Terra controller provider transport is unavailable")]
    Transport,
    /// The fixed recovery lane could not reserve its bounded storage.
    #[error("Terra controller recovery storage is unavailable")]
    RecoveryStorage,
}

/// One controller configured only through narrow injected ports and ownership.
#[must_use]
pub struct TerraTextOnlyController {
    state: Option<TerraControllerRunState>,
    completion: Arc<Mutex<TerraControllerCompletionSlot>>,
}

impl TerraTextOnlyController {
    /// Eagerly creates one controller and its sole move-only terminal observation.
    ///
    /// Construction completes before the controller can enter the runtime, so
    /// every controller future owns recoverable state from its first poll.
    pub fn try_new(
        input: TerraControllerRunInput,
        transport_config: AgentProviderTransportConfig,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
    ) -> Result<(Self, TerraControllerCompletion), TerraControllerConstructionError> {
        // This controller owns a fresh transport instance.  Accepting a
        // cloneable shared transport would make the final shutdown proof
        // meaningless because a retained caller clone could admit another
        // call after controller construction.
        let transport = AgentProviderTransport::try_new(transport_config)
            .map_err(|_| TerraControllerConstructionError::Transport)?;
        Self::try_new_with_dedicated_transport(input, transport, credential, audit)
    }

    /// Builds a diagnostic-only controller around a fixed loopback transport.
    ///
    /// This seam exists solely for the release-forbidden probe harness. The
    /// caller must create a fresh idle transport and must not retain clones;
    /// production callers use [`Self::try_new`], which constructs exclusive
    /// transport ownership internally.
    #[cfg(feature = "probe-harness")]
    pub fn try_new_for_probe(
        input: TerraControllerRunInput,
        transport: AgentProviderTransport,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
    ) -> Result<(Self, TerraControllerCompletion), TerraControllerConstructionError> {
        let snapshot = transport
            .snapshot()
            .map_err(|_| TerraControllerConstructionError::Transport)?;
        if snapshot.is_sealed() || !snapshot.is_idle() {
            return Err(TerraControllerConstructionError::Transport);
        }
        Self::try_new_with_dedicated_transport(input, transport, credential, audit)
    }

    fn try_new_with_dedicated_transport(
        input: TerraControllerRunInput,
        transport: AgentProviderTransport,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
    ) -> Result<(Self, TerraControllerCompletion), TerraControllerConstructionError> {
        let state = Self::build_run(input, transport, credential, audit)?;
        let completion = Arc::new(Mutex::new(TerraControllerCompletionSlot::Pending));
        Ok((
            Self {
                state: Some(state),
                completion: Arc::clone(&completion),
            },
            TerraControllerCompletion { inner: completion },
        ))
    }

    fn build_run(
        input: TerraControllerRunInput,
        transport: AgentProviderTransport,
        credential: AgentProviderCredential,
        audit_port: Arc<dyn AgentAuditPort>,
    ) -> Result<TerraControllerRunState, TerraControllerConstructionError> {
        let root = input.lease.node();
        let topology = AgentDelegationTopology::try_new(
            &input.manifest,
            vec![AgentDelegationSpec::new(root, None)],
        )
        .map_err(|_| TerraControllerConstructionError::Authority)?;
        let supervisor = AgentRunSupervisor::new(input.ids.supervisor, topology);
        let audit = AgentAuditLedger::try_new(&input.manifest, &supervisor)
            .map_err(|_| TerraControllerConstructionError::Authority)?;
        let accounting = AgentRunAccountingMetrics::try_new(&input.manifest, &supervisor)
            .map_err(|_| TerraControllerConstructionError::Authority)?;
        let progress = AgentRunProgressMetrics::try_new(&input.manifest, &supervisor)
            .map_err(|_| TerraControllerConstructionError::Authority)?;
        let actions = AgentRunActionPerformanceMetrics::try_new(&input.manifest, &supervisor)
            .map_err(|_| TerraControllerConstructionError::Authority)?;
        let inputs = AgentRunProviderInputMetrics::try_new(&input.manifest, &supervisor)
            .map_err(|_| TerraControllerConstructionError::Authority)?;
        let policy = AgentRunPolicy::try_new(input.manifest, vec![input.lease])
            .map_err(|_| TerraControllerConstructionError::Authority)?;
        let mut deferred_runtime_events = Vec::new();
        deferred_runtime_events
            .try_reserve_exact(MAX_DEFERRED_RUNTIME_EVENTS)
            .map_err(|_| TerraControllerConstructionError::RecoveryStorage)?;
        Ok(TerraControllerRunState {
            root,
            lease: input.lease.lease(),
            execution: None,
            policy,
            supervisor,
            audit,
            accounting,
            progress,
            actions,
            inputs,
            account: input.account,
            observation: input.observation,
            frame: input.frame,
            invocation: input.invocation,
            snapshot_generation: input.snapshot_generation,
            objective: Some(input.objective),
            ids: input.ids,
            clock: input.clock,
            deadline: input.deadline,
            last_policy_at: None,
            attempt: None,
            unsettled: None,
            final_input_receipt: None,
            final_model_receipt: None,
            final_terminal: None,
            dropped_text_bytes: 0,
            transport,
            credential: Some(credential),
            audit_port,
            cancellation: None,
            stop: None,
            deferred_runtime_events,
            closed_outcome: None,
        })
    }
}

impl fmt::Debug for TerraTextOnlyController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TerraTextOnlyController([authority, redacted])")
    }
}

impl Drop for TerraTextOnlyController {
    fn drop(&mut self) {
        let Some(mut state) = self.state.take() else {
            return;
        };
        // Neither the cleartext objective nor a still-unadmitted credential
        // can help deterministic reconciliation. Do not retain either in a
        // recovery owner merely because a controller returned early.
        drop(state.objective.take());
        drop(state.credential.take());
        // Seal first so recovery cannot accidentally race new admission while
        // the forced-drop path converts the state-owned attempt to its only
        // terminal settlement.
        state.transport.seal();
        if let Some(attempt) = state.attempt.take() {
            if let Ok(result) = attempt.abort(AgentProviderAbortReason::ControllerFault) {
                let input = result.input_metric_receipt();
                if state.final_input_receipt.is_none() {
                    state.final_input_receipt = Some(input);
                }
                if let Ok(facts) = settle_terminal_result(&mut state, result) {
                    if state.final_model_receipt.is_none() {
                        state.final_model_receipt = Some(facts.receipt);
                        state.final_terminal = Some(facts);
                    }
                }
            }
        }
        let mut slot = match self.completion.lock() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        if matches!(*slot, TerraControllerCompletionSlot::Pending) {
            *slot = TerraControllerCompletionSlot::Ready(TerraControllerCompletionState::Recovery(
                TerraControllerRecovery {
                    state: Box::new(state),
                },
            ));
        }
    }
}

impl AgentRuntimeController for TerraTextOnlyController {
    fn run(
        self: Box<Self>,
        mut worker: AgentRuntimeWorker,
        browser: AgentRuntimeBrowser,
    ) -> AgentRuntimeControllerFuture {
        Box::pin(async move {
            let mut controller = *self;
            let _ = controller.run_turn(&mut worker, &browser).await;
        })
    }
}

impl TerraTextOnlyController {
    async fn run_turn(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
    ) -> Result<(), ()> {
        self.await_run_start(worker).await?;
        // The progress reducer requires the queued projection before root
        // execution starts. The four identifiers then record model-active,
        // model-terminal, and root-terminal in that order.
        self.record_projection(0)?;
        {
            let state = self.state.as_mut().ok_or(())?;
            let execution = state
                .supervisor
                .start(state.root, state.ids.attempt)
                .map_err(|_| ())?;
            state.execution = Some(execution);
        }
        let observation = self.collect_observation(worker, browser).await?;
        self.prepare_provider_attempt(observation, worker)?;
        self.record_active_model_projection()?;
        let facts = self.drive_provider(worker).await?;
        self.record_terminal_model_projection(facts)?;
        self.complete_root_from_terminal(facts, worker)?;
        self.record_projection(3)?;
        self.deliver_terminal_audit(worker).await?;
        self.publish_terminal(worker).await
    }

    async fn await_run_start(&mut self, worker: &mut AgentRuntimeWorker) -> Result<(), ()> {
        let deadline = self.state_deadline(worker)?;
        match next_event_until(worker, deadline).await? {
            AgentRuntimeEvent::RunStarted(_) => Ok(()),
            AgentRuntimeEvent::CancellationRequested => {
                let state = self.state.as_mut().ok_or(())?;
                record_stop(state, TerraControllerStop::Cancellation);
                Err(())
            }
            AgentRuntimeEvent::ShutdownRequested => {
                let state = self.state.as_mut().ok_or(())?;
                record_stop(state, TerraControllerStop::Shutdown);
                Err(())
            }
            event => {
                let state = self.state.as_mut().ok_or(())?;
                retain_runtime_event(state, event)?;
                record_stop(state, TerraControllerStop::Fault);
                Err(())
            }
        }
    }

    fn state_deadline(&self, worker: &AgentRuntimeWorker) -> Result<Instant, ()> {
        let state = self.state.as_ref().ok_or(())?;
        let deadline = effective_deadline(state, worker);
        if Instant::now() >= deadline {
            return Err(());
        }
        Ok(deadline)
    }

    fn record_projection(&mut self, index: usize) -> Result<(), ()> {
        let state = self.state.as_mut().ok_or(())?;
        let now = checked_policy_now(state).map_err(|_| ())?;
        let event = state
            .audit
            .record_current(
                &state.supervisor,
                state.root,
                *state.ids.audit_events.get(index).ok_or(())?,
                now,
            )
            .map_err(|_| ())?;
        state.progress.record_event(event).map_err(|_| ())
    }

    async fn collect_observation(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
    ) -> Result<zephium_agentic::SemanticObservation, ()> {
        let (invocation, correlation) = {
            let state = self.state.as_mut().ok_or(())?;
            let invocation = encode_semantic_runtime_invocation(
                &state.observation,
                state.frame.clone(),
                state.invocation,
                state.snapshot_generation,
                SemanticRuntimeBudget::INITIAL_FILTERED,
            )
            .map_err(|_| ())?;
            let correlation = invocation.correlation();
            (invocation, correlation)
        };
        self.permit_new_effect(worker)?;
        if browser.invoke_semantic(invocation) != ContextDispatch::Scheduled {
            return Err(());
        }
        loop {
            let deadline = match self.state_deadline(worker) {
                Ok(deadline) => deadline,
                Err(()) => {
                    let state = self.state.as_mut().ok_or(())?;
                    record_stop(state, deadline_stop(state, worker));
                    return Err(());
                }
            };
            let timer = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline));
            tokio::pin!(timer);
            let event = tokio::select! {
                biased;
                event = worker.next_event_for_terminal_cleanup() => event,
                () = &mut timer => {
                    let state = self.state.as_mut().ok_or(())?;
                    record_stop(state, deadline_stop(state, worker));
                    return Err(());
                }
            };
            if worker.status().mailbox_fault().is_some() {
                let state = self.state.as_mut().ok_or(())?;
                record_stop(state, TerraControllerStop::Fault);
            }
            match event {
                AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::SemanticRuntimeSettled(
                    settlement,
                )) => {
                    if settlement.correlation() != &correlation {
                        let state = self.state.as_mut().ok_or(())?;
                        retain_runtime_event(
                            state,
                            AgentRuntimeEvent::NativeTerminal(
                                ContextNativeEvent::SemanticRuntimeSettled(settlement),
                            ),
                        )?;
                        record_stop(state, TerraControllerStop::Fault);
                        continue;
                    }
                    let snapshot = settlement.into_outcome().map_err(|_| ())?;
                    let state = self.state.as_ref().ok_or(())?;
                    return SemanticObservationAssembler::new(state.observation.clone(), snapshot)
                        .and_then(SemanticObservationAssembler::finish)
                        .map_err(|_| ());
                }
                AgentRuntimeEvent::CancellationRequested => {
                    let state = self.state.as_mut().ok_or(())?;
                    record_stop(state, TerraControllerStop::Cancellation);
                    continue;
                }
                AgentRuntimeEvent::ShutdownRequested => {
                    let state = self.state.as_mut().ok_or(())?;
                    record_stop(state, TerraControllerStop::Shutdown);
                    continue;
                }
                event => {
                    let state = self.state.as_mut().ok_or(())?;
                    retain_runtime_event(state, event)?;
                    record_stop(state, TerraControllerStop::Fault);
                    continue;
                }
            }
        }
    }

    fn prepare_provider_attempt(
        &mut self,
        observation: zephium_agentic::SemanticObservation,
        worker: &AgentRuntimeWorker,
    ) -> Result<(), ()> {
        let now = self.permit_new_effect(worker)?;
        let state = self.state.as_mut().ok_or(())?;
        let config = try_terra_provider_exact_call_config(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS)
            .map_err(|_| ())?;
        let additional = u32::try_from(TERRA_STANDARD_RATE_MAX_INPUT_TOKENS).map_err(|_| ())?;
        let budget = AgentModelCallBudget::try_new(
            additional,
            TERRA_CONTROLLER_MAX_OUTPUT_TOKENS,
            TERRA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD,
        )
        .map_err(|_| ())?;
        let call = AgentModelCallRequest::new(
            state.ids.model_call,
            state.lease,
            state.account,
            budget,
            now,
        );
        let payload = encode_semantic_observation(
            &observation,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .and_then(|encoded| encoded.admit_conservative_utf8(config.tokenizer()))
        .map_err(|_| ())?;
        // The request and continuation retain the one necessary encoded copy;
        // the mutable run state must never retain a second objective after
        // provider admission or recovery publication.
        let objective = state.objective.take().ok_or(())?;
        let prepared = AgentPreparedObservationRequest::try_openai_for_provider_exact_count(
            &mut state.policy,
            call,
            &observation,
            payload,
            &objective,
            config,
        )
        .map_err(|_| ())?;
        // `config` is catalog-owned and this controller constructed the
        // prepared request itself. A fallible post-reservation equality check
        // would drop `prepared` and its live policy reservation without a
        // settlement path, so no such branch is permitted here.
        // Provider admission commits semantic disclosure. Recheck after the
        // synchronous reservation/encoding work; if it lost the deadline or
        // control race, consume the prepared authority through its exact
        // pre-disclosure cancellation path rather than dropping it.
        if self.permit_new_effect(worker).is_err() {
            let state = self.state.as_mut().ok_or(())?;
            let _ = prepared.settle(&mut state.policy, AgentProviderRequestSettlement::Cancelled);
            return Err(());
        }
        let state = self.state.as_mut().ok_or(())?;
        let cancellation = AgentProviderCancellation::new();
        let credential = state.credential.as_ref().ok_or(())?;
        let admission = state.transport.try_admit(
            prepared.into_transport_input(),
            &mut state.policy,
            credential,
            cancellation.clone(),
        );
        // Transport copied the only header form needed for the live attempt.
        // Drop the outer key material immediately after *any* admission
        // outcome: recovery can reconcile terminal policy/audit state but must
        // never retain credentials for a later replay.
        drop(state.credential.take());
        let attempt = admission.map_err(|_| ())?;
        state.cancellation = Some(cancellation);
        state.attempt = Some(attempt);
        Ok(())
    }

    fn record_active_model_projection(&mut self) -> Result<(), ()> {
        {
            let state = self.state.as_mut().ok_or(())?;
            let execution = state.execution.as_ref().ok_or(())?;
            let call = state
                .attempt
                .as_ref()
                .and_then(AgentProviderAttempt::call)
                .ok_or(())?;
            state
                .supervisor
                .record_active_model_call(execution, call)
                .map_err(|_| ())?;
        }
        self.record_projection(1)
    }

    async fn drive_provider(
        &mut self,
        worker: &mut AgentRuntimeWorker,
    ) -> Result<TerraProviderTerminalFacts, ()> {
        let (cancellation, deadline) = {
            let state = self.state.as_mut().ok_or(())?;
            (
                state.cancellation.clone().ok_or(())?,
                effective_deadline(state, worker),
            )
        };
        let count = {
            let state = self.state.as_mut().ok_or(())?;
            state
                .attempt
                .as_mut()
                .ok_or(())?
                .count_openai_input_tokens()
        };
        let (count, count_stop) =
            match wait_provider_drive(worker, &cancellation, deadline, count).await {
                Ok(outcome) => (outcome.value, outcome.stop),
                Err(ProviderDriveStop::Deadline) => {
                    let state = self.state.as_mut().ok_or(())?;
                    record_stop(state, deadline_stop(state, worker));
                    return abort_and_settle(state, AgentProviderAbortReason::HostDeadline);
                }
                Err(ProviderDriveStop::Worker) => {
                    let state = self.state.as_mut().ok_or(())?;
                    record_stop(state, TerraControllerStop::Fault);
                    return abort_and_settle(state, AgentProviderAbortReason::ControllerFault);
                }
                Err(ProviderDriveStop::Unexpected(event)) => {
                    let state = self.state.as_mut().ok_or(())?;
                    retain_runtime_event(state, *event)?;
                    record_stop(state, TerraControllerStop::Fault);
                    return abort_and_settle(state, AgentProviderAbortReason::ControllerFault);
                }
            };
        match count {
            zephium_agentic::AgentProviderExactCountOutcome::Counted(counted) => {
                // The counted handle is a live exclusive borrow of the
                // operation. Retain this copy only across its single stream
                // drive, then require terminal receipt equality before the
                // reducer records the final ProviderExact metric.
                let counted_input_receipt = counted.input_metric_receipt();
                let mut dropped_bytes = 0_u32;
                let mut overflowed = false;
                let stream = counted.execute(|batch| {
                    for delta in batch.deltas() {
                        let Ok(bytes) = u32::try_from(delta.len()) else {
                            overflowed = true;
                            return AgentProviderBatchDisposition::Cancel;
                        };
                        let Some(next) = dropped_bytes.checked_add(bytes) else {
                            overflowed = true;
                            return AgentProviderBatchDisposition::Cancel;
                        };
                        dropped_bytes = next;
                    }
                    AgentProviderBatchDisposition::Continue
                });
                let result = match wait_provider_drive(worker, &cancellation, deadline, stream)
                    .await
                {
                    Ok(ProviderDriveResult {
                        value: Ok(result),
                        stop,
                    }) => (result, stop),
                    Ok(ProviderDriveResult {
                        value: Err(_),
                        stop,
                    }) => {
                        let state = self.state.as_mut().ok_or(())?;
                        if let Some(stop) = count_stop.or(stop) {
                            record_stop(state, stop);
                        }
                        return abort_and_settle(state, AgentProviderAbortReason::ControllerFault);
                    }
                    Err(ProviderDriveStop::Worker) => {
                        let state = self.state.as_mut().ok_or(())?;
                        record_stop(state, TerraControllerStop::Fault);
                        return abort_and_settle(state, AgentProviderAbortReason::ControllerFault);
                    }
                    Err(ProviderDriveStop::Deadline) => {
                        let state = self.state.as_mut().ok_or(())?;
                        record_stop(state, deadline_stop(state, worker));
                        return abort_and_settle(state, AgentProviderAbortReason::HostDeadline);
                    }
                    Err(ProviderDriveStop::Unexpected(event)) => {
                        let state = self.state.as_mut().ok_or(())?;
                        retain_runtime_event(state, *event)?;
                        record_stop(state, TerraControllerStop::Fault);
                        return abort_and_settle(state, AgentProviderAbortReason::ControllerFault);
                    }
                };
                let state = self.state.as_mut().ok_or(())?;
                if let Some(stop) = count_stop.or(result.1) {
                    record_stop(state, stop);
                }
                drop(state.attempt.take());
                let result = result.0;
                state.dropped_text_bytes = dropped_bytes;
                let receipt_matches = result.input_metric_receipt() == counted_input_receipt;
                let facts = settle_and_record_result(state, result)?;
                if overflowed || !receipt_matches {
                    return Err(());
                }
                Ok(facts)
            }
            zephium_agentic::AgentProviderExactCountOutcome::Failed(result) => {
                let state = self.state.as_mut().ok_or(())?;
                if let Some(stop) = count_stop {
                    record_stop(state, stop);
                }
                drop(state.attempt.take());
                settle_and_record_result(state, result)
            }
            zephium_agentic::AgentProviderExactCountOutcome::Unavailable(_) => {
                let state = self.state.as_mut().ok_or(())?;
                if let Some(stop) = count_stop {
                    record_stop(state, stop);
                }
                abort_and_settle(state, AgentProviderAbortReason::ControllerFault)
            }
        }
    }

    fn record_terminal_model_projection(
        &mut self,
        facts: TerraProviderTerminalFacts,
    ) -> Result<(), ()> {
        {
            let state = self.state.as_mut().ok_or(())?;
            state
                .accounting
                .record_model_receipt(facts.receipt)
                .map_err(|_| ())?;
            let execution = state.execution.as_ref().ok_or(())?;
            state
                .supervisor
                .record_model_call_result(execution, facts.receipt)
                .map_err(|_| ())?;
        }
        self.record_projection(2)
    }

    fn complete_root_from_terminal(
        &mut self,
        facts: TerraProviderTerminalFacts,
        worker: &AgentRuntimeWorker,
    ) -> Result<(), ()> {
        let state = self.state.as_mut().ok_or(())?;
        refresh_runtime_stop(state, worker);
        let bytes_match = facts.output_text_bytes() == Some(state.dropped_text_bytes);
        let completion = if state.stop.is_none() && facts.is_completed_text_only() && bytes_match {
            AgentSupervisorCompletion::Succeeded
        } else {
            match state.stop {
                Some(TerraControllerStop::Cancellation) => {
                    let _ = state
                        .supervisor
                        .cancel_subtree(
                            state.root,
                            state.ids.cancellation,
                            AgentSupervisorCancellationReason::UserRequested,
                        )
                        .map_err(|_| ())?;
                    AgentSupervisorCompletion::Failed(AgentSupervisorFailure::ProviderFailed)
                }
                Some(TerraControllerStop::Shutdown) => {
                    let _ = state
                        .supervisor
                        .cancel_subtree(
                            state.root,
                            state.ids.cancellation,
                            AgentSupervisorCancellationReason::Shutdown,
                        )
                        .map_err(|_| ())?;
                    AgentSupervisorCompletion::Failed(AgentSupervisorFailure::ProviderFailed)
                }
                Some(TerraControllerStop::Deadline) => {
                    let _ = state
                        .supervisor
                        .cancel_subtree(
                            state.root,
                            state.ids.cancellation,
                            AgentSupervisorCancellationReason::DeadlineExceeded,
                        )
                        .map_err(|_| ())?;
                    AgentSupervisorCompletion::Failed(AgentSupervisorFailure::ProviderFailed)
                }
                Some(TerraControllerStop::Fault) => {
                    AgentSupervisorCompletion::Failed(AgentSupervisorFailure::ProviderFailed)
                }
                None if facts.conclusion.is_none()
                    || matches!(
                        facts.conclusion,
                        Some(AgentProviderStreamConclusion::Failed(_))
                    ) =>
                {
                    AgentSupervisorCompletion::Failed(AgentSupervisorFailure::ProviderFailed)
                }
                None => {
                    AgentSupervisorCompletion::Failed(AgentSupervisorFailure::InvalidModelOutput)
                }
            }
        };
        let execution = state.execution.take().ok_or(())?;
        state
            .supervisor
            .complete(execution, completion)
            .map_err(|_| ())?;
        state.closed_outcome = Some(match state.stop {
            Some(TerraControllerStop::Cancellation)
            | Some(TerraControllerStop::Shutdown)
            | Some(TerraControllerStop::Deadline) => TerraControllerClosedOutcome::Cancelled,
            Some(TerraControllerStop::Fault) => TerraControllerClosedOutcome::Failed,
            None if facts.is_completed_text_only() && bytes_match => {
                TerraControllerClosedOutcome::Succeeded
            }
            None => TerraControllerClosedOutcome::Failed,
        });
        Ok(())
    }

    async fn deliver_terminal_audit(&mut self, worker: &mut AgentRuntimeWorker) -> Result<(), ()> {
        {
            let state = self.state.as_mut().ok_or(())?;
            checked_policy_now(state).map_err(|_| ())?;
            state.transport.seal();
            let _ = state.transport.try_prove_shutdown().map_err(|_| ())?;
        }
        self.permit_new_effect(worker)?;
        let (expected_delivery, dispatch) = {
            let state = self.state.as_mut().ok_or(())?;
            let delivery = state
                .audit
                .begin_delivery(state.ids.audit_delivery, MAX_AGENT_AUDIT_DELIVERY_EVENTS)
                .map_err(|_| ())?;
            let expected = delivery.proof();
            (
                expected,
                state.audit_port.append(delivery, worker.audit_completion()),
            )
        };
        let settlement = match dispatch {
            AgentAuditDispatch::Refused(settlement)
                if settlement.proof() == expected_delivery
                    && !matches!(
                        settlement.outcome(),
                        zephium_agentic::AgentAuditDeliveryOutcome::Committed
                    ) =>
            {
                settlement
            }
            AgentAuditDispatch::Refused(_) => return Err(()),
            AgentAuditDispatch::Accepted(proof) if proof == expected_delivery => loop {
                let deadline = self.state_deadline(worker)?;
                let timer = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline));
                tokio::pin!(timer);
                let event = tokio::select! {
                    biased;
                    event = worker.next_event_for_terminal_cleanup() => event,
                    () = &mut timer => {
                        let state = self.state.as_mut().ok_or(())?;
                        record_stop(state, deadline_stop(state, worker));
                        return Err(());
                    }
                };
                if worker.status().mailbox_fault().is_some() {
                    let state = self.state.as_mut().ok_or(())?;
                    record_stop(state, TerraControllerStop::Fault);
                }
                match event {
                    AgentRuntimeEvent::AuditTerminal(settlement)
                        if settlement.proof() == expected_delivery =>
                    {
                        break settlement;
                    }
                    AgentRuntimeEvent::AuditTerminal(settlement) => {
                        let state = self.state.as_mut().ok_or(())?;
                        retain_runtime_event(state, AgentRuntimeEvent::AuditTerminal(settlement))?;
                        record_stop(state, TerraControllerStop::Fault);
                        continue;
                    }
                    AgentRuntimeEvent::CancellationRequested => {
                        let state = self.state.as_mut().ok_or(())?;
                        record_stop(state, TerraControllerStop::Cancellation);
                    }
                    AgentRuntimeEvent::ShutdownRequested => {
                        let state = self.state.as_mut().ok_or(())?;
                        record_stop(state, TerraControllerStop::Shutdown);
                    }
                    event => {
                        let state = self.state.as_mut().ok_or(())?;
                        retain_runtime_event(state, event)?;
                        record_stop(state, TerraControllerStop::Fault);
                        // Accepted audit ownership remains exact and live.
                        // Keep draining until its own callback or the bounded
                        // cleanup deadline; an unrelated native event cannot
                        // strand it in the ledger.
                        continue;
                    }
                }
            },
            AgentAuditDispatch::Accepted(_) => return Err(()),
        };
        let state = self.state.as_mut().ok_or(())?;
        if !matches!(
            state.audit.settle_delivery(settlement).map_err(|_| ())?,
            zephium_agentic::AgentAuditDeliveryOutcome::Committed
        ) {
            return Err(());
        }
        // The exact callback is settled before a later clock/control check.
        // Cancellation may prevent a clean success, but it may not strand an
        // accepted durable delivery in the ledger.
        refresh_runtime_stop(state, worker);
        checked_policy_now(state).map_err(|_| ())?;
        state.audit.seal_for_shutdown().map_err(|_| ())?;
        state.audit.is_quiescent().then_some(()).ok_or(())
    }

    fn permit_new_effect(&mut self, worker: &AgentRuntimeWorker) -> Result<AgentPolicyInstant, ()> {
        let state = self.state.as_mut().ok_or(())?;
        refresh_runtime_stop(state, worker);
        if state.stop.is_some() || Instant::now() >= effective_deadline(state, worker) {
            if state.stop.is_none() {
                record_stop(state, deadline_stop(state, worker));
            }
            return Err(());
        }
        let now = checked_policy_now(state).map_err(|_| ())?;
        if Instant::now() >= effective_deadline(state, worker) {
            record_stop(state, deadline_stop(state, worker));
            return Err(());
        }
        Ok(now)
    }

    async fn publish_terminal(&mut self, worker: &mut AgentRuntimeWorker) -> Result<(), ()> {
        let state = self.state.as_ref().ok_or(())?;
        let root_outcome = state.closed_outcome.ok_or(())?;
        if state.unsettled.is_some() || !state.deferred_runtime_events.is_empty() {
            return Err(());
        }
        let closure = AgentRunMetricClosure::try_close(
            state.policy.manifest(),
            &state.supervisor,
            &state.accounting,
            &state.progress,
            &state.actions,
            &state.inputs,
        )
        .map_err(|_| ())?;
        let class = match state.stop {
            Some(TerraControllerStop::Cancellation) => {
                AgentRuntimeControllerTerminalClass::Cancelled
            }
            Some(TerraControllerStop::Shutdown) => AgentRuntimeControllerTerminalClass::Shutdown,
            Some(TerraControllerStop::Deadline) | Some(TerraControllerStop::Fault) | None => {
                AgentRuntimeControllerTerminalClass::Ordinary
            }
        };
        // This is the runtime-owned clean-terminal linearization. It closes
        // ingress and rejects queued native debt before policy closure becomes
        // irreversible. The same proof applies to clean failed/cancelled
        // roots; only the controller's closed outcome labels the business
        // result.
        let claim = match worker.try_claim_controller_terminal(class).await {
            Ok(claim) => claim,
            Err(_) => {
                self.retain_terminal_claim_refusal_events(worker)?;
                return Err(());
            }
        };
        let mut completion_slot = match self.completion.lock() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !matches!(*completion_slot, TerraControllerCompletionSlot::Pending) {
            return Err(());
        }
        let state = self.state.take().ok_or(())?;
        let TerraControllerRunState {
            root,
            lease,
            execution,
            policy,
            supervisor,
            audit,
            accounting,
            progress,
            actions,
            inputs,
            account,
            observation,
            frame,
            invocation,
            snapshot_generation,
            objective,
            ids,
            clock,
            deadline,
            last_policy_at,
            attempt,
            unsettled,
            final_input_receipt,
            final_model_receipt,
            final_terminal,
            dropped_text_bytes,
            transport,
            credential,
            audit_port,
            cancellation,
            stop,
            deferred_runtime_events,
            closed_outcome: retained_outcome,
        } = state;
        match policy.settle_metric_closure(closure, &accounting, audit) {
            Ok(_) => {
                // Once policy/audit closure consumes their authorities, both
                // the runtime proof and private completion publication are
                // typestate-total. No fallible operation may follow.
                claim.commit();
                let final_outcome = closed_outcome_after_stop(stop, root_outcome);
                *completion_slot = TerraControllerCompletionSlot::Ready(match final_outcome {
                    TerraControllerClosedOutcome::Succeeded => {
                        TerraControllerCompletionState::Succeeded(TerraControllerSuccess {
                            closure,
                            text_delta_bytes_dropped: dropped_text_bytes,
                        })
                    }
                    TerraControllerClosedOutcome::Cancelled => {
                        TerraControllerCompletionState::Cancelled(TerraControllerCancelled {
                            _private: (),
                        })
                    }
                    TerraControllerClosedOutcome::Failed => {
                        TerraControllerCompletionState::Failed(TerraControllerFailure {
                            _private: (),
                        })
                    }
                });
                Ok(())
            }
            Err(refusal) => {
                drop(completion_slot);
                let (policy, audit) = refusal.into_parts();
                self.state = Some(TerraControllerRunState {
                    root,
                    lease,
                    execution,
                    policy,
                    supervisor,
                    audit,
                    accounting,
                    progress,
                    actions,
                    inputs,
                    account,
                    observation,
                    frame,
                    invocation,
                    snapshot_generation,
                    objective,
                    ids,
                    clock,
                    deadline,
                    last_policy_at,
                    attempt,
                    unsettled,
                    final_input_receipt,
                    final_model_receipt,
                    final_terminal,
                    dropped_text_bytes,
                    transport,
                    credential,
                    audit_port,
                    cancellation,
                    stop,
                    deferred_runtime_events,
                    closed_outcome: retained_outcome,
                });
                Err(())
            }
        }
    }

    fn retain_terminal_claim_refusal_events(
        &mut self,
        worker: &mut AgentRuntimeWorker,
    ) -> Result<(), ()> {
        while let Some(event) = worker.try_drain_terminal_claim_refusal_event() {
            let state = self.state.as_mut().ok_or(())?;
            retain_runtime_event(state, event)?;
            record_stop(state, TerraControllerStop::Fault);
        }
        Ok(())
    }
}

struct TerraControllerRunState {
    root: zephium_agentic::AgentPlanNodeId,
    lease: zephium_agentic::AgentPlanLeaseId,
    execution: Option<AgentNodeExecution>,
    policy: AgentRunPolicy,
    supervisor: AgentRunSupervisor,
    audit: AgentAuditLedger,
    accounting: AgentRunAccountingMetrics,
    progress: AgentRunProgressMetrics,
    actions: AgentRunActionPerformanceMetrics,
    inputs: AgentRunProviderInputMetrics,
    account: AgentContextAccountBinding,
    observation: SemanticObservationRequest,
    frame: zephium_agentic::SemanticFrameJoin,
    invocation: SemanticInvocationId,
    snapshot_generation: SemanticSnapshotGeneration,
    objective: Option<AgentProviderObjective>,
    ids: TerraControllerIds,
    clock: Arc<dyn TerraControllerClock>,
    deadline: Instant,
    last_policy_at: Option<AgentPolicyInstant>,
    attempt: Option<AgentProviderAttempt>,
    unsettled: Option<TerraControllerUnsettledTerminal>,
    final_input_receipt: Option<zephium_agentic::AgentProviderInputMetricReceipt>,
    final_model_receipt: Option<AgentModelCallReceipt>,
    final_terminal: Option<TerraProviderTerminalFacts>,
    dropped_text_bytes: u32,
    // These ports are deliberately part of the recovery bundle. A forced
    // controller drop must not discard the ability to prove transport drain
    // or reconcile an accepted-but-not-yet-callback audit delivery.
    transport: AgentProviderTransport,
    credential: Option<AgentProviderCredential>,
    audit_port: Arc<dyn AgentAuditPort>,
    cancellation: Option<AgentProviderCancellation>,
    stop: Option<TerraControllerStop>,
    // An unexpected terminal-lane value can itself carry move-only callback
    // authority. Retain it for opaque recovery rather than losing it under a
    // wildcard controller event branch.
    deferred_runtime_events: Vec<AgentRuntimeEvent>,
    closed_outcome: Option<TerraControllerClosedOutcome>,
}

#[derive(Clone, Copy)]
enum TerraControllerStop {
    Cancellation,
    Shutdown,
    Deadline,
    Fault,
}

enum ProviderDriveStop {
    Deadline,
    Worker,
    Unexpected(Box<AgentRuntimeEvent>),
}

struct ProviderDriveResult<T> {
    value: T,
    stop: Option<TerraControllerStop>,
}

async fn next_event_until(
    worker: &mut AgentRuntimeWorker,
    deadline: Instant,
) -> Result<AgentRuntimeEvent, ()> {
    if Instant::now() >= deadline {
        return Err(());
    }
    let timer = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline));
    tokio::pin!(timer);
    tokio::select! {
        event = worker.next_event() => event.map_err(|_| ()),
        () = &mut timer => Err(()),
    }
}

async fn wait_provider_drive<T>(
    worker: &mut AgentRuntimeWorker,
    cancellation: &AgentProviderCancellation,
    controller_deadline: Instant,
    drive: impl std::future::Future<Output = T>,
) -> Result<ProviderDriveResult<T>, ProviderDriveStop> {
    tokio::pin!(drive);
    let mut stop = None;
    loop {
        let deadline = worker
            .shutdown_deadline()
            .filter(|shutdown| *shutdown < controller_deadline)
            .unwrap_or(controller_deadline);
        if Instant::now() >= deadline {
            return Err(ProviderDriveStop::Deadline);
        }
        let timer = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline));
        tokio::pin!(timer);
        tokio::select! {
            biased;
            event = worker.next_event() => match event {
                Ok(AgentRuntimeEvent::CancellationRequested) => {
                    if stop.is_none() {
                        stop = Some(TerraControllerStop::Cancellation);
                    }
                    cancellation.cancel();
                }
                Ok(AgentRuntimeEvent::ShutdownRequested) => {
                    stop = Some(TerraControllerStop::Shutdown);
                    cancellation.cancel();
                }
                Ok(event) => {
                    // A terminal-lane value can own exact settlement
                    // authority. Stop borrowing this operation so the caller
                    // can abort it, and retain the event for recovery rather
                    // than silently dropping it in a wildcard branch.
                    cancellation.cancel();
                    return Err(ProviderDriveStop::Unexpected(Box::new(event)));
                }
                Err(_) => {
                    cancellation.cancel();
                    return Err(ProviderDriveStop::Worker);
                }
            },
            result = &mut drive => return Ok(ProviderDriveResult { value: result, stop }),
            () = &mut timer => return Err(ProviderDriveStop::Deadline),
        }
    }
}

fn abort_and_settle(
    state: &mut TerraControllerRunState,
    reason: AgentProviderAbortReason,
) -> Result<TerraProviderTerminalFacts, ()> {
    let attempt = state.attempt.take().ok_or(())?;
    let result = attempt.abort(reason).map_err(|_| ())?;
    settle_and_record_result(state, result)
}

fn settle_and_record_result(
    state: &mut TerraControllerRunState,
    result: AgentProviderTransportResult,
) -> Result<TerraProviderTerminalFacts, ()> {
    // A terminal result still owns its active policy authority. Copy only its
    // bounded receipt, settle/retain that authority, and retain terminal facts
    // before any reducer can refuse. Reducers are descriptive and must never
    // be able to destroy an unsettled provider terminal.
    let input = result.input_metric_receipt();
    let input_conflict = state
        .final_input_receipt
        .is_some_and(|existing| existing != input);
    if state.final_input_receipt.is_none() {
        state.final_input_receipt = Some(input);
    }
    let facts = settle_terminal_result(state, result)?;
    if state.final_model_receipt.is_some() || state.final_terminal.is_some() {
        return Err(());
    }
    state.final_model_receipt = Some(facts.receipt);
    state.final_terminal = Some(facts);
    if input_conflict {
        return Err(());
    }
    state.inputs.record(input).map_err(|_| ())?;
    Ok(facts)
}

enum TerraControllerUnsettledTerminal {
    Immediate(AgentProviderImmediateSettlement),
    Pricing(TerraProviderTerminalOwner),
}

impl TerraControllerUnsettledTerminal {
    const fn label(&self) -> &'static str {
        match self {
            Self::Immediate(settlement) => {
                let _ = settlement;
                "immediate"
            }
            Self::Pricing(terminal) => {
                let _ = terminal;
                "pricing"
            }
        }
    }
}

/// Content-free terminal facts retained until root closure determines the
/// text-only result. Provider text and any proposal remain unreachable here.
#[derive(Clone, Copy)]
struct TerraProviderTerminalFacts {
    receipt: AgentModelCallReceipt,
    conclusion: Option<AgentProviderStreamConclusion>,
    has_tool_turn: bool,
}

impl TerraProviderTerminalFacts {
    fn output_text_bytes(self) -> Option<u32> {
        self.conclusion.map(|conclusion| match conclusion {
            AgentProviderStreamConclusion::Completed(completion) => {
                completion.stats().output_text_bytes()
            }
            AgentProviderStreamConclusion::Failed(failure) => failure.stats().output_text_bytes(),
        })
    }

    fn is_completed_text_only(self) -> bool {
        matches!(
            self.conclusion,
            Some(AgentProviderStreamConclusion::Completed(completion))
                if completion.stop() == AgentProviderStopReason::Completed
                    && completion.stats().output_text_bytes() > 0
        ) && self.receipt.settlement() == AgentModelCallSettlement::Completed
            && !self.has_tool_turn
    }
}

fn checked_policy_now(
    state: &mut TerraControllerRunState,
) -> Result<AgentPolicyInstant, TerraControllerClockError> {
    let now = state.clock.now()?;
    let manifest = state.policy.manifest();
    let node = manifest
        .plan_nodes()
        .iter()
        .find(|node| node.id() == state.root)
        .ok_or(TerraControllerClockError::Invalid)?;
    if now < manifest.issued_at()
        || now > manifest.expires_at()
        || now > node.expires_at()
        || state.last_policy_at.is_some_and(|last| now < last)
    {
        return Err(TerraControllerClockError::Invalid);
    }
    state.last_policy_at = Some(now);
    Ok(now)
}

fn effective_deadline(state: &TerraControllerRunState, worker: &AgentRuntimeWorker) -> Instant {
    match worker.shutdown_deadline() {
        Some(shutdown) if shutdown < state.deadline => shutdown,
        Some(_) | None => state.deadline,
    }
}

fn deadline_stop(
    state: &TerraControllerRunState,
    worker: &AgentRuntimeWorker,
) -> TerraControllerStop {
    match worker.shutdown_deadline() {
        Some(shutdown) if shutdown <= state.deadline => TerraControllerStop::Shutdown,
        Some(_) | None => TerraControllerStop::Deadline,
    }
}

fn record_stop(state: &mut TerraControllerRunState, observed: TerraControllerStop) {
    // Lifecycle shutdown is stronger than a prior user cancellation. All
    // other first observations remain sticky so EOF cannot erase them.
    if state.stop.is_none()
        || matches!(
            (state.stop, observed),
            (
                Some(TerraControllerStop::Cancellation),
                TerraControllerStop::Shutdown
            )
        )
    {
        state.stop = Some(observed);
    }
}

fn refresh_runtime_stop(state: &mut TerraControllerRunState, worker: &AgentRuntimeWorker) {
    if worker.shutdown_deadline().is_some() {
        record_stop(state, TerraControllerStop::Shutdown);
    } else if worker.status().cancelled() {
        record_stop(state, TerraControllerStop::Cancellation);
    } else if Instant::now() >= state.deadline {
        record_stop(state, TerraControllerStop::Deadline);
    }
}

fn closed_outcome_after_stop(
    stop: Option<TerraControllerStop>,
    root_outcome: TerraControllerClosedOutcome,
) -> TerraControllerClosedOutcome {
    match stop {
        Some(TerraControllerStop::Cancellation)
        | Some(TerraControllerStop::Shutdown)
        | Some(TerraControllerStop::Deadline) => TerraControllerClosedOutcome::Cancelled,
        Some(TerraControllerStop::Fault) => TerraControllerClosedOutcome::Failed,
        None => root_outcome,
    }
}

fn retain_runtime_event(
    state: &mut TerraControllerRunState,
    event: AgentRuntimeEvent,
) -> Result<(), ()> {
    if state.deferred_runtime_events.len() >= MAX_DEFERRED_RUNTIME_EVENTS {
        return Err(());
    }
    state.deferred_runtime_events.push(event);
    Ok(())
}

fn settle_terminal_result(
    state: &mut TerraControllerRunState,
    result: AgentProviderTransportResult,
) -> Result<TerraProviderTerminalFacts, ()> {
    if state.unsettled.is_some() {
        return Err(());
    }
    let conclusion = match result.outcome() {
        AgentProviderTransportOutcome::Stream(conclusion) => Some(*conclusion),
        AgentProviderTransportOutcome::Failed(_) => None,
    };
    match result.into_policy_settlement() {
        AgentProviderPolicySettlement::Immediate(settlement) => match settlement
            .settle(&mut state.policy)
        {
            Ok(receipt) => Ok(TerraProviderTerminalFacts {
                receipt,
                conclusion,
                has_tool_turn: conclusion.is_some_and(|terminal| {
                    matches!(
                        terminal,
                        AgentProviderStreamConclusion::Completed(completion)
                            if completion.tool_only_output()
                    )
                }),
            }),
            Err(error) => {
                if let Some(unsettled) = error.into_unsettled() {
                    state.unsettled = Some(TerraControllerUnsettledTerminal::Immediate(unsettled));
                }
                Err(())
            }
        },
        AgentProviderPolicySettlement::PricingRequired(settlement) => {
            match settle_terra_provider_terminal(*settlement, &mut state.policy) {
                Ok(TerraProviderTerminalSettlement::Priced(terminal)) => {
                    let receipt = terminal.receipt();
                    let conclusion = terminal.conclusion();
                    let has_tool_turn = terminal.has_tool_turn();
                    drop(terminal);
                    Ok(TerraProviderTerminalFacts {
                        receipt,
                        conclusion: Some(conclusion),
                        has_tool_turn,
                    })
                }
                Ok(TerraProviderTerminalSettlement::ReservationCeiling(receipt)) => {
                    Ok(TerraProviderTerminalFacts {
                        receipt: *receipt,
                        conclusion,
                        has_tool_turn: conclusion.is_some_and(|terminal| {
                            matches!(
                                terminal,
                                AgentProviderStreamConclusion::Completed(completion)
                                    if completion.tool_only_output()
                            )
                        }),
                    })
                }
                Err(TerraProviderTerminalSettlementError::PolicyPrecondition {
                    retained, ..
                })
                | Err(TerraProviderTerminalSettlementError::Fallback { retained }) => {
                    state.unsettled = Some(TerraControllerUnsettledTerminal::Pricing(retained));
                    Err(())
                }
                Err(TerraProviderTerminalSettlementError::Policy(_)) => Err(()),
            }
        }
    }
}

/// Move-only external observation of a controller terminal state.
#[must_use]
pub struct TerraControllerCompletion {
    inner: Arc<Mutex<TerraControllerCompletionSlot>>,
}

impl TerraControllerCompletion {
    /// Whether the controller has published its sole terminal observation.
    ///
    /// A `false` result retains this handle unchanged; it never consumes or
    /// tombstones eventual success or recovery authority.
    pub fn is_ready(&self) -> bool {
        let slot = match self.inner.lock() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        matches!(*slot, TerraControllerCompletionSlot::Ready(_))
    }

    /// Consumes the sole terminal observation only after it is ready.
    ///
    /// A pending handle is returned unchanged so its owner can later observe
    /// the exact success or retained recovery authority.
    pub fn try_into_state(self) -> Result<TerraControllerCompletionState, Self> {
        let mut slot = match self.inner.lock() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !matches!(*slot, TerraControllerCompletionSlot::Ready(_)) {
            drop(slot);
            return Err(self);
        }
        match std::mem::replace(&mut *slot, TerraControllerCompletionSlot::Taken) {
            TerraControllerCompletionSlot::Ready(state) => Ok(state),
            TerraControllerCompletionSlot::Pending | TerraControllerCompletionSlot::Taken => {
                drop(slot);
                Err(self)
            }
        }
    }
}

impl fmt::Debug for TerraControllerCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TerraControllerCompletion([move-only, redacted])")
    }
}

enum TerraControllerCompletionSlot {
    Pending,
    Ready(TerraControllerCompletionState),
    Taken,
}

#[derive(Clone, Copy)]
enum TerraControllerClosedOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

/// Closed terminal controller observation.
#[must_use]
pub enum TerraControllerCompletionState {
    /// The root succeeded and every controller-owned closure check passed.
    Succeeded(TerraControllerSuccess),
    /// The root closed as a deterministic content-free failure.
    Failed(TerraControllerFailure),
    /// The root closed after cancellation or lifecycle shutdown drained.
    Cancelled(TerraControllerCancelled),
    /// Reconciliation did not establish a clean terminal and retained state.
    Recovery(TerraControllerRecovery),
}

impl fmt::Debug for TerraControllerCompletionState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Succeeded(_) => formatter.write_str("TerraControllerCompletionState::Succeeded"),
            Self::Failed(_) => formatter.write_str("TerraControllerCompletionState::Failed"),
            Self::Cancelled(_) => formatter.write_str("TerraControllerCompletionState::Cancelled"),
            Self::Recovery(_) => formatter.write_str("TerraControllerCompletionState::Recovery"),
        }
    }
}

/// Content-free evidence of a fully closed logical controller turn.
///
/// This proves provider, reducer, durable-audit, policy, and runtime mailbox
/// closure owned by this one turn. The shared browser cohort's native resource
/// shutdown is deliberately owned by the outer lifecycle's existing native
/// shutdown coordinator, not by this text-only observer controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerraControllerSuccess {
    closure: AgentRunMetricClosure,
    text_delta_bytes_dropped: u32,
}

/// Content-free evidence that the root reached a closed failed terminal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerraControllerFailure {
    _private: (),
}

/// Content-free evidence that cancellation reached a closed terminal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerraControllerCancelled {
    _private: (),
}

impl TerraControllerSuccess {
    /// Cross-checked reducer closure established before policy clean settlement.
    pub const fn closure(self) -> AgentRunMetricClosure {
        self.closure
    }

    /// Model text bytes counted and discarded without retention or release.
    pub const fn text_delta_bytes_dropped(self) -> u32 {
        self.text_delta_bytes_dropped
    }
}

/// Opaque move-only state retained when the controller cannot claim success.
#[must_use]
pub struct TerraControllerRecovery {
    state: Box<TerraControllerRunState>,
}

impl fmt::Debug for TerraControllerRecovery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerraControllerRecovery")
            .field(
                "unsettled",
                &self
                    .state
                    .unsettled
                    .as_ref()
                    .map(TerraControllerUnsettledTerminal::label),
            )
            .field("active_attempt", &self.state.attempt.is_some())
            .field("input_receipt", &self.state.final_input_receipt.is_some())
            .field("model_receipt", &self.state.final_model_receipt.is_some())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Release-excluded owner for one bounded Terra continuation session.
///
/// The session retains one policy, credential, and transport across the exact
/// initial observation and at most one verified diff continuation. It is not
/// product orchestration: native effects still pass through the dedicated
/// probe qualifier rather than the shipping policy actor.
#[cfg(feature = "probe-harness")]
#[must_use]
pub struct TerraProbeSession {
    policy: AgentRunPolicy,
    transport: AgentProviderTransport,
    credential: AgentProviderCredential,
    config: AgentProviderCallConfig,
    lease: AgentPlanLeaseBinding,
    account: AgentContextAccountBinding,
    next_call: u64,
    next_policy_millis: u64,
    deadline: Instant,
    turns: u8,
    finished: bool,
}

#[cfg(feature = "probe-harness")]
impl TerraProbeSession {
    /// Starts one session and returns its first settled tool proposal.
    pub async fn start(
        input: TerraControllerRunInput,
        transport_config: AgentProviderTransportConfig,
        credential: AgentProviderCredential,
        observation: &zephium_agentic::SemanticObservation,
    ) -> Result<(Self, TerraProbeProviderTurn), TerraProbeProviderError> {
        let TerraControllerRunInput {
            manifest,
            lease,
            account,
            objective,
            ids,
            clock,
            deadline,
            ..
        } = input;
        if Instant::now() >= deadline {
            return Err(TerraProbeProviderError::Deadline);
        }
        let now = clock.now().map_err(|_| TerraProbeProviderError::Clock)?;
        let config = try_terra_provider_exact_call_config(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS)
            .map_err(|_| TerraProbeProviderError::Catalog)?;
        let budget = terra_probe_call_budget()?;
        let call =
            AgentModelCallRequest::new(ids.model_call(), lease.lease(), account, budget, now);
        let payload = encode_semantic_observation(
            observation,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .and_then(|encoded| encoded.admit_conservative_utf8(config.tokenizer()))
        .map_err(TerraProbeProviderError::InitialEncoding)?;
        let mut policy = AgentRunPolicy::try_new(manifest, vec![lease])
            .map_err(|_| TerraProbeProviderError::Authority)?;
        let prepared = AgentPreparedObservationRequest::try_openai_for_provider_exact_count(
            &mut policy,
            call,
            observation,
            payload,
            &objective,
            config.clone(),
        )
        .map_err(|_| TerraProbeProviderError::Authority)?;
        let transport = AgentProviderTransport::try_new(transport_config)
            .map_err(|_| TerraProbeProviderError::Transport)?;
        let next_call = ids
            .model_call()
            .get()
            .checked_add(1)
            .ok_or(TerraProbeProviderError::Authority)?;
        let next_policy_millis = now
            .millis()
            .checked_add(1)
            .ok_or(TerraProbeProviderError::Clock)?;
        let mut session = Self {
            policy,
            transport,
            credential,
            config,
            lease,
            account,
            next_call,
            next_policy_millis,
            deadline,
            turns: 0,
            finished: false,
        };
        let turn = match session.drive(prepared.into_transport_input()).await {
            Ok(turn) => turn,
            Err(error) => {
                let _ = session.finish();
                return Err(error);
            }
        };
        Ok((session, turn))
    }

    /// Sends one independently verified action diff as the exact tool result.
    pub async fn continue_after_verified_action(
        &mut self,
        transition: TerraProbeVerifiedTransition,
    ) -> Result<TerraProbeProviderTurn, TerraProbeProviderError> {
        if self.finished || self.turns >= MAX_TERRA_PROBE_MODEL_TURNS {
            return Err(TerraProbeProviderError::TurnLimit);
        }
        if Instant::now() >= self.deadline {
            return Err(TerraProbeProviderError::Deadline);
        }
        let call_id =
            AgentModelCallId::new(self.next_call).ok_or(TerraProbeProviderError::Authority)?;
        self.next_call = self
            .next_call
            .checked_add(1)
            .ok_or(TerraProbeProviderError::Authority)?;
        let now = AgentPolicyInstant::from_millis(self.next_policy_millis);
        self.next_policy_millis = self
            .next_policy_millis
            .checked_add(1)
            .ok_or(TerraProbeProviderError::Clock)?;
        let request = AgentModelCallRequest::new(
            call_id,
            self.lease.lease(),
            self.account,
            terra_probe_call_budget()?,
            now,
        );
        let (continuation, diff) = transition.into_parts();
        let payload = encode_semantic_diff(
            &diff,
            SemanticModelEncodingBudget::ACTION_DIFF_PROVIDER_EXACT_CONSERVATIVE,
        )
        .and_then(|encoded| encoded.admit_conservative_utf8(self.config.tokenizer()))
        .map_err(TerraProbeProviderError::DiffEncoding)?;
        let bound = continuation
            .bind_diff_request(request, &self.config, &diff, payload)
            .map_err(|_| TerraProbeProviderError::Continuation)?;
        let draft = AgentProviderDiffRequestDraft::try_new(bound)
            .map_err(|_| TerraProbeProviderError::Continuation)?;
        let prepared = draft
            .try_prepare_for_provider_exact_count(&mut self.policy, request, &diff)
            .map_err(|_| TerraProbeProviderError::Authority)?;
        self.drive(prepared.into_transport_input()).await
    }

    async fn drive(
        &mut self,
        input: AgentProviderTransportInput,
    ) -> Result<TerraProbeProviderTurn, TerraProbeProviderError> {
        if self.finished || self.turns >= MAX_TERRA_PROBE_MODEL_TURNS {
            return Err(TerraProbeProviderError::TurnLimit);
        }
        let cancellation = AgentProviderCancellation::new();
        let mut attempt = self
            .transport
            .try_admit(input, &mut self.policy, &self.credential, cancellation)
            .map_err(|_| TerraProbeProviderError::Transport)?;
        let counted = tokio::time::timeout_at(
            tokio::time::Instant::from_std(self.deadline),
            attempt.count_openai_input_tokens(),
        )
        .await;
        let result = match counted {
            Ok(zephium_agent_provider_transport::AgentProviderExactCountOutcome::Counted(
                counted,
            )) => match tokio::time::timeout_at(
                tokio::time::Instant::from_std(self.deadline),
                counted.execute(|_| AgentProviderBatchDisposition::Continue),
            )
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(_)) | Err(_) => attempt
                    .abort(AgentProviderAbortReason::HostDeadline)
                    .map_err(|_| TerraProbeProviderError::Transport)?,
            },
            Ok(zephium_agent_provider_transport::AgentProviderExactCountOutcome::Failed(
                result,
            )) => result,
            Ok(zephium_agent_provider_transport::AgentProviderExactCountOutcome::Unavailable(
                _,
            )) => attempt
                .abort(AgentProviderAbortReason::ControllerFault)
                .map_err(|_| TerraProbeProviderError::Transport)?,
            Err(_) => attempt
                .abort(AgentProviderAbortReason::HostDeadline)
                .map_err(|_| TerraProbeProviderError::Transport)?,
        };
        let input = result.input_metric_receipt();
        let disclosure = result.disclosure_stage();
        let turn = settle_terra_probe_result(result, disclosure, input, &mut self.policy)?;
        self.turns = self
            .turns
            .checked_add(1)
            .ok_or(TerraProbeProviderError::TurnLimit)?;
        Ok(turn)
    }

    /// Seals the shared transport and proves every admitted call has drained.
    pub fn finish(mut self) -> Result<(), TerraProbeProviderError> {
        self.transport.seal();
        let _shutdown = self
            .transport
            .try_prove_shutdown()
            .map_err(|_| TerraProbeProviderError::Transport)?;
        self.finished = true;
        Ok(())
    }
}

#[cfg(feature = "probe-harness")]
impl Drop for TerraProbeSession {
    fn drop(&mut self) {
        self.transport.seal();
    }
}

#[cfg(feature = "probe-harness")]
impl fmt::Debug for TerraProbeSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerraProbeSession")
            .field("turns", &self.turns)
            .field("finished", &self.finished)
            .field("content", &"[redacted]")
            .finish()
    }
}

#[cfg(feature = "probe-harness")]
fn terra_probe_call_budget() -> Result<AgentModelCallBudget, TerraProbeProviderError> {
    let input_ceiling = u32::try_from(TERRA_STANDARD_RATE_MAX_INPUT_TOKENS)
        .map_err(|_| TerraProbeProviderError::Catalog)?;
    AgentModelCallBudget::try_new(
        input_ceiling,
        TERRA_CONTROLLER_MAX_OUTPUT_TOKENS,
        TERRA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD,
    )
    .map_err(|_| TerraProbeProviderError::Catalog)
}

#[cfg(feature = "probe-harness")]
fn settle_terra_probe_result(
    result: AgentProviderTransportResult,
    disclosure: AgentProviderDisclosureStage,
    input: zephium_agentic::AgentProviderInputMetricReceipt,
    policy: &mut AgentRunPolicy,
) -> Result<TerraProbeProviderTurn, TerraProbeProviderError> {
    match result.into_policy_settlement() {
        AgentProviderPolicySettlement::Immediate(settlement) => {
            let terminal_failure = match settlement.outcome() {
                AgentProviderTransportOutcome::Failed(failure) => Some(failure),
                AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Failed(
                    failure,
                )) => Some(failure.failure()),
                AgentProviderTransportOutcome::Stream(
                    AgentProviderStreamConclusion::Completed(_),
                ) => None,
            };
            let _receipt = settlement
                .settle(policy)
                .map_err(|_| TerraProbeProviderError::Settlement)?;
            Err(match terminal_failure {
                Some(failure) => match disclosure {
                    AgentProviderDisclosureStage::NotDispatched => {
                        TerraProbeProviderError::PreDispatchTerminal(failure.class())
                    }
                    AgentProviderDisclosureStage::InputTokenCountDisclosed => {
                        TerraProbeProviderError::CountTerminal(failure.class())
                    }
                    AgentProviderDisclosureStage::ModelRequestMayHaveDispatched => {
                        match failure.protocol_error() {
                            Some(error) => TerraProbeProviderError::ModelProtocol(
                                error,
                                failure.protocol_event(),
                            ),
                            None => TerraProbeProviderError::ModelTerminal(failure.class()),
                        }
                    }
                },
                None => TerraProbeProviderError::Proposal,
            })
        }
        AgentProviderPolicySettlement::PricingRequired(settlement) => {
            match settle_terra_provider_terminal(*settlement, policy)
                .map_err(|_| TerraProbeProviderError::Settlement)?
            {
                TerraProviderTerminalSettlement::Priced(terminal) => {
                    let receipt = terminal.receipt();
                    let Some(turn) = terminal.into_tool_turn() else {
                        return Err(TerraProbeProviderError::Proposal);
                    };
                    Ok(TerraProbeProviderTurn {
                        receipt,
                        input,
                        turn,
                    })
                }
                TerraProviderTerminalSettlement::ReservationCeiling(_) => {
                    Err(TerraProbeProviderError::Settlement)
                }
            }
        }
    }
}

/// Runs one release-excluded provider-exact Terra proposal turn.
#[cfg(feature = "probe-harness")]
pub async fn run_initial_terra_probe(
    input: TerraControllerRunInput,
    transport_config: AgentProviderTransportConfig,
    credential: AgentProviderCredential,
    observation: &zephium_agentic::SemanticObservation,
) -> Result<TerraProbeProviderTurn, TerraProbeProviderError> {
    let (session, turn) =
        TerraProbeSession::start(input, transport_config, credential, observation).await?;
    session.finish()?;
    Ok(turn)
}

/// One settled, exactly-priced tool proposal and content-free receipts.
#[cfg(feature = "probe-harness")]
#[must_use]
pub struct TerraProbeProviderTurn {
    receipt: AgentModelCallReceipt,
    input: zephium_agentic::AgentProviderInputMetricReceipt,
    turn: zephium_agentic::AgentProviderSettledToolTurn,
}

#[cfg(feature = "probe-harness")]
impl TerraProbeProviderTurn {
    /// Exact model accounting receipt.
    pub const fn receipt(&self) -> AgentModelCallReceipt {
        self.receipt
    }
    /// Exact provider count receipt.
    pub const fn input(&self) -> zephium_agentic::AgentProviderInputMetricReceipt {
        self.input
    }
    /// Releases the single settled browser tool turn.
    pub fn into_tool_turn(self) -> zephium_agentic::AgentProviderSettledToolTurn {
        self.turn
    }
}

#[cfg(feature = "probe-harness")]
impl fmt::Debug for TerraProbeProviderTurn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TerraProbeProviderTurn")
            .field("receipt", &self.receipt)
            .field("input", &self.input)
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Content-free provider-probe refusal.
#[cfg(feature = "probe-harness")]
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TerraProbeProviderError {
    /// Supplied live context or policy authority did not join exactly.
    #[error("Terra probe authority was invalid")]
    Authority,
    /// The pinned Terra catalog configuration was unavailable.
    #[error("Terra probe catalog was unavailable")]
    Catalog,
    /// The injected policy clock could not provide an admissible instant.
    #[error("Terra probe clock was unavailable")]
    Clock,
    /// The one-turn absolute deadline elapsed.
    #[error("Terra probe deadline elapsed")]
    Deadline,
    /// The trusted initial semantic observation could not be encoded.
    #[error("Terra probe initial observation encoding failed")]
    InitialEncoding(SemanticModelEncodingError),
    /// The verified semantic action diff could not be encoded.
    #[error("Terra probe action diff encoding failed")]
    DiffEncoding(SemanticModelEncodingError),
    /// Exact-count or streaming provider transport failed.
    #[error("Terra probe transport failed")]
    Transport,
    /// Provider failed before either external request was committed.
    #[error("Terra probe provider failed before dispatch")]
    PreDispatchTerminal(AgentProviderFailureClass),
    /// Provider failed after the authenticated exact-count request was disclosed.
    #[error("Terra probe provider exact-count request failed")]
    CountTerminal(AgentProviderFailureClass),
    /// Provider failed after model generation may have been dispatched.
    #[error("Terra probe provider model request failed")]
    ModelTerminal(AgentProviderFailureClass),
    /// Provider model stream violated one closed decoder invariant.
    #[error("Terra probe provider model stream violated its typed protocol")]
    ModelProtocol(
        AgentProviderProtocolError,
        Option<AgentProviderProtocolEvent>,
    ),
    /// Exact provider terminal pricing or policy settlement failed.
    #[error("Terra probe settlement failed")]
    Settlement,
    /// The settled terminal did not release one tool proposal.
    #[error("Terra probe did not return exactly one tool proposal")]
    Proposal,
    /// The verified result could not bind to the exact prior provider turn.
    #[error("Terra probe continuation did not match the prior turn")]
    Continuation,
    /// The fixed qualification session exceeded its model-turn ceiling.
    #[error("Terra probe model-turn ceiling was exhausted")]
    TurnLimit,
}

#[cfg(test)]
mod tests {
    use super::{
        TerraControllerConstructionError, TerraControllerIds, TERRA_CONTROLLER_MAX_OUTPUT_TOKENS,
        TERRA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD,
    };
    use zephium_agent_model_catalog::{
        TERRA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS, TERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS,
        TERRA_STANDARD_RATE_MAX_INPUT_TOKENS,
    };
    use zephium_agentic::{
        AgentAuditDeliveryId, AgentAuditEventId, AgentModelCallId, AgentSupervisorAttemptId,
        AgentSupervisorCancellationId, AgentSupervisorId,
    };

    fn event(value: u64) -> AgentAuditEventId {
        AgentAuditEventId::new(value).expect("test event id")
    }

    #[test]
    fn controller_audit_ids_are_strictly_monotonic_before_any_run_mutation() {
        let error = TerraControllerIds::try_new(
            AgentSupervisorId::new(1).expect("supervisor"),
            AgentSupervisorAttemptId::new(1).expect("attempt"),
            AgentSupervisorCancellationId::new(1).expect("cancellation"),
            AgentModelCallId::new(1).expect("model"),
            [event(10), event(12), event(11), event(13)],
            AgentAuditDeliveryId::new(1).expect("delivery"),
        )
        .expect_err("out-of-order events");
        assert_eq!(error, TerraControllerConstructionError::AuditIdentifiers);
    }

    #[test]
    fn text_only_output_ceiling_and_reservation_remain_bounded() {
        assert_eq!(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS, 8_192);
        assert_eq!(
            TERRA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD,
            TERRA_STANDARD_RATE_MAX_INPUT_TOKENS * TERRA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS
                / 1_000_000
                + u64::from(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS)
                    * TERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS
                    / 1_000_000
        );
    }
}
