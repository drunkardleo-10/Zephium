//! Closed Terra controller composition.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use thiserror::Error;
use zephium_agent_model_catalog::{
    settle_luna_provider_terminal, try_luna_provider_exact_call_config,
    LunaProviderTerminalSettlement, LUNA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS,
    LUNA_MAX_OUTPUT_TOKENS, LUNA_MODEL_REVISION, LUNA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS,
    LUNA_STANDARD_RATE_MAX_INPUT_TOKENS, TERRA_MODEL_REVISION,
};
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
    encode_semantic_diff, encode_semantic_locate_result, encode_semantic_observation,
    encode_semantic_runtime_invocation, locate_semantic_observation, AgentAuditDeliveryId,
    AgentAuditDispatch, AgentAuditEventId, AgentAuditLedger, AgentAuditPort,
    AgentContextAccountBinding, AgentDelegationSpec, AgentDelegationTopology, AgentModelCallBudget,
    AgentModelCallId, AgentModelCallReceipt, AgentModelCallRequest, AgentModelCallSettlement,
    AgentNodeExecution, AgentPlanLeaseBinding, AgentPolicyInstant, AgentPreparedObservationRequest,
    AgentProviderBatchDisposition, AgentProviderCallConfig, AgentProviderCancellation,
    AgentProviderDiffRequestDraft, AgentProviderDisclosureStage, AgentProviderFailureClass,
    AgentProviderImmediateSettlement, AgentProviderLocateRequestDraft, AgentProviderObjective,
    AgentProviderPolicySettlement, AgentProviderProtocolError, AgentProviderProtocolEvent,
    AgentProviderRequestSettlement, AgentProviderSettledToolTurn, AgentProviderStopReason,
    AgentProviderStreamConclusion, AgentProviderTransportInput, AgentProviderTransportOutcome,
    AgentProviderTransportResult, AgentRunAccountingMetrics, AgentRunActionPerformanceMetrics,
    AgentRunMetricClosure, AgentRunPolicy, AgentRunProgressMetrics, AgentRunProviderInputMetrics,
    AgentRunSupervisor, AgentSupervisorAttemptId, AgentSupervisorCancellationId,
    AgentSupervisorCancellationReason, AgentSupervisorCompletion, AgentSupervisorFailure,
    AgentSupervisorId, ContextDispatch, ContextNativeEvent, SemanticInvocationId,
    SemanticLocateBudget, SemanticLocateId, SemanticLocateRequest, SemanticModelEncodingBudget,
    SemanticModelEncodingError, SemanticObservationAssembler, SemanticObservationRequest,
    SemanticRuntimeBudget, SemanticSnapshotGeneration, MAX_AGENT_AUDIT_DELIVERY_EVENTS,
};

use crate::action::AgentBrowserVerifiedTransition;

#[path = "work.rs"]
pub(crate) mod work;
pub use work::{
    AgentWorkClosedUnsuccessfully, AgentWorkContextSpec, AgentWorkController, AgentWorkEvent,
    AgentWorkEventKind, AgentWorkExtractionTask, AgentWorkFailure, AgentWorkHandle,
    AgentWorkInitialReadiness, AgentWorkOutcome, AgentWorkRecovery, AgentWorkRetainedBrowser,
    AgentWorkRetainedController, AgentWorkRetainedHandle, AgentWorkRetainedOutcome,
    AgentWorkRetainedRecovery, AgentWorkRetainedResourceSpec, AgentWorkRunInput,
    AgentWorkRunSettings, AgentWorkSuccess, AgentWorkTask, AgentWorkTaskProgress,
    MAX_AGENT_WORK_EVENTS,
};

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
const MAX_BROWSER_MODEL_TURNS: u8 = 8;
const MAX_BROWSER_ACTIONS: u64 = 8;
const MAX_WORK_MODEL_CALLS: u8 = 64;
const MAX_WORK_ACTIONS: u64 = 64;
const LUNA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD: u64 = 77_830;

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
    {
        assert!(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS < LUNA_MAX_OUTPUT_TOKENS);
        assert!(
            LUNA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD
                == LUNA_STANDARD_RATE_MAX_INPUT_TOKENS
                    * LUNA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS
                    / 1_000_000
                    + (TERRA_CONTROLLER_MAX_OUTPUT_TOKENS as u64)
                        * LUNA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS
                        / 1_000_000
        );
    }
};

/// Closed catalog-backed model adapters for the shared browser driver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentBrowserModel {
    /// Balanced-intelligence GPT-5.6 Terra qualification baseline.
    Terra,
    /// Cost-sensitive GPT-5.6 Luna qualification target.
    Luna,
}

impl AgentBrowserModel {
    /// Exact provider model alias used by this adapter.
    pub const fn revision(self) -> &'static str {
        match self {
            Self::Terra => TERRA_MODEL_REVISION,
            Self::Luna => LUNA_MODEL_REVISION,
        }
    }
}

/// Provider retention policy; release graphs expose only the stateless variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentBrowserRetention {
    /// Preserve the production stateless `store:false` request contract.
    Stateless,
    /// Retain an explicitly public qualification request for dashboard review.
    #[cfg(feature = "probe-harness")]
    InspectablePublicData,
}

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
    max_model_calls: u8,
    max_actions: u64,
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
        let config = try_terra_provider_exact_call_config(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS)
            .map_err(|_| TerraControllerConstructionError::Catalog)?;
        Self::try_new_with_config(manifest, lease, turn, ids, clock, deadline, &config)
    }

    /// Builds a run input for an explicitly selected catalog-backed model.
    pub fn try_new_for_model(
        manifest: zephium_agentic::AgentRunManifest,
        lease: AgentPlanLeaseBinding,
        turn: TerraControllerTurnInput,
        ids: TerraControllerIds,
        clock: Arc<dyn TerraControllerClock>,
        deadline: Instant,
        model: AgentBrowserModel,
    ) -> Result<Self, TerraControllerConstructionError> {
        let config = match model {
            AgentBrowserModel::Terra => {
                try_terra_provider_exact_call_config(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS)
                    .map_err(|_| TerraControllerConstructionError::Catalog)?
            }
            AgentBrowserModel::Luna => {
                try_luna_provider_exact_call_config(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS)
                    .map_err(|_| TerraControllerConstructionError::Catalog)?
            }
        };
        Self::try_new_with_config(manifest, lease, turn, ids, clock, deadline, &config)
    }

    /// Compatibility constructor for existing release-excluded qualifiers.
    #[cfg(feature = "probe-harness")]
    pub fn try_new_for_probe_model(
        manifest: zephium_agentic::AgentRunManifest,
        lease: AgentPlanLeaseBinding,
        turn: TerraControllerTurnInput,
        ids: TerraControllerIds,
        clock: Arc<dyn TerraControllerClock>,
        deadline: Instant,
        model: AgentBrowserModel,
    ) -> Result<Self, TerraControllerConstructionError> {
        Self::try_new_for_model(manifest, lease, turn, ids, clock, deadline, model)
    }

    fn try_new_with_config(
        manifest: zephium_agentic::AgentRunManifest,
        lease: AgentPlanLeaseBinding,
        turn: TerraControllerTurnInput,
        ids: TerraControllerIds,
        clock: Arc<dyn TerraControllerClock>,
        deadline: Instant,
        config: &AgentProviderCallConfig,
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
            max_model_calls: MAX_BROWSER_MODEL_TURNS,
            max_actions: MAX_BROWSER_ACTIONS,
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

/// Exact synchronous terminal evidence, retained independently of policy time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AgentBrowserNavigationDispatchRefusal {
    operation: zephium_agentic::ContextOperationJoin,
    failure: zephium_agentic::ContextPortFailure,
}

/// Bounded locate/act session sharing the production policy and provider ports.
///
/// The host owns this session before polling any provider work. Dropping a
/// pending future leaves its attempt and accounting debt in this owner. Native
/// lifecycle, durable audit and the application supervisor remain host duties;
/// this driver alone is not a fully closed Work run.
#[must_use]
pub struct AgentBrowserSession {
    navigation: Option<zephium_agentic::AgentActiveNavigation>,
    navigation_refusal: Option<AgentBrowserNavigationDispatchRefusal>,
    navigation_receipt: Option<zephium_agentic::AgentNavigationReceipt>,
    journal: Option<work::WorkJournal>,
    journal_receipts: usize,
    policy: AgentRunPolicy,
    transport: BrowserSessionTransport,
    credential: Option<AgentProviderCredential>,
    attempt: Option<AgentProviderAttempt>,
    retained_terminal: Option<BrowserUnsettledTerminal>,
    extraction_output: Option<zephium_agentic::AgentProviderExtractionOutputCollector>,
    model_receipts: Vec<(
        AgentModelCallReceipt,
        zephium_agentic::AgentProviderInputMetricReceipt,
    )>,
    config: AgentProviderCallConfig,
    objective: Option<AgentProviderObjective>,
    model: AgentBrowserModel,
    lease: AgentPlanLeaseBinding,
    account: AgentContextAccountBinding,
    account_attestations: Vec<zephium_agentic::AgentAccountAttestationId>,
    next_call: u64,
    clock: Arc<dyn TerraControllerClock>,
    last_policy_at: AgentPolicyInstant,
    cancellation: AgentProviderCancellation,
    next_action: u64,
    action: Option<crate::AgentBrowserAction>,
    action_executions: zephium_agentic::SemanticActionExecutionCoordinator,
    action_settlements: zephium_agentic::SemanticActionSettlementCoordinator,
    action_refusal: Option<crate::AgentBrowserActionFinalizationRefusal>,
    action_admission_failure: Option<zephium_agentic::AgentFailedSemanticEffect>,
    action_proposal_failure: Option<crate::action::AgentBrowserActionProposalRefusal>,
    action_terminal: Option<zephium_agentic::SemanticActionBatchResult>,
    failure: Option<AgentBrowserProviderError>,
    deadline: Instant,
    turns: u8,
    max_model_calls: u8,
    max_actions: u64,
    max_account_attestations: usize,
    finished: bool,
}

impl AgentBrowserSession {
    /// Accepts a new independently sourced account sample at an idle boundary.
    /// The caller must use its trusted account adapter, never update a previous
    /// sample's timestamp. This grants no account/context switch, retry, renewed
    /// deadline, observation or policy budget. A refusal permanently stops new
    /// work while preserving every existing provider/native/accounting owner.
    pub fn refresh_account(
        &mut self,
        account: AgentContextAccountBinding,
    ) -> Result<(), AgentBrowserProviderError> {
        self.check_live()?;
        let result = self.validate_account_refresh(account);
        if let Err(error) = result {
            self.failure = Some(error);
            return Err(error);
        }
        if account != self.account {
            self.account_attestations.push(account.attestation());
        }
        self.account = account;
        Ok(())
    }

    fn validate_account_refresh(
        &mut self,
        account: AgentContextAccountBinding,
    ) -> Result<(), AgentBrowserProviderError> {
        self.validate_account_update(account, self.account.context())
    }

    fn validate_account_update(
        &mut self,
        account: AgentContextAccountBinding,
        expected_context: zephium_agentic::ContextJoin,
    ) -> Result<(), AgentBrowserProviderError> {
        use zephium_agentic::MAX_AGENT_ACCOUNT_ATTESTATION_AGE_MILLIS;
        let refusal = AgentBrowserProviderError::Account;
        if self.attempt.is_some()
            || self.retained_terminal.is_some()
            || self.action.is_some()
            || self.policy.accounting().reserved_operations() != 0
        {
            return Err(refusal(AgentBrowserAccountError::Pending));
        }
        if account.context() != expected_context {
            return Err(refusal(AgentBrowserAccountError::ContextChanged));
        }
        if account.account() != self.account.account() {
            return Err(refusal(AgentBrowserAccountError::AccountChanged));
        }
        let now = self.policy_now()?;
        if account.observed_at() > now || account.observed_at() < self.account.observed_at() {
            return Err(refusal(AgentBrowserAccountError::Clock));
        }
        if now.millis() - account.observed_at().millis() > MAX_AGENT_ACCOUNT_ATTESTATION_AGE_MILLIS
        {
            return Err(refusal(AgentBrowserAccountError::Stale));
        }
        if account.attestation() == self.account.attestation() && account != self.account {
            return Err(refusal(AgentBrowserAccountError::Rewritten));
        }
        if account != self.account {
            if self.account_attestations.contains(&account.attestation()) {
                return Err(refusal(AgentBrowserAccountError::Replayed));
            }
            if self.account_attestations.len() >= self.max_account_attestations {
                return Err(refusal(AgentBrowserAccountError::Limit));
            }
        }
        Ok(())
    }

    /// Starts one session and returns its first settled tool proposal.
    #[cfg(feature = "probe-harness")]
    pub async fn start(
        input: TerraControllerRunInput,
        transport_config: AgentProviderTransportConfig,
        credential: AgentProviderCredential,
        observation: &zephium_agentic::SemanticObservation,
    ) -> Result<(Self, AgentBrowserProviderTurn), AgentBrowserProviderError> {
        Self::start_with_model(
            input,
            transport_config,
            credential,
            observation,
            AgentBrowserModel::Terra,
            AgentBrowserRetention::Stateless,
        )
        .await
    }

    /// Diagnostic convenience wrapper; production hosts retain ownership first.
    #[cfg(feature = "probe-harness")]
    pub async fn start_with_model(
        input: TerraControllerRunInput,
        transport_config: AgentProviderTransportConfig,
        credential: AgentProviderCredential,
        observation: &zephium_agentic::SemanticObservation,
        model: AgentBrowserModel,
        retention: AgentBrowserRetention,
    ) -> Result<(Self, AgentBrowserProviderTurn), AgentBrowserProviderError> {
        let mut session = Self::try_new(input, transport_config, credential, model, retention)?;
        let turn = match session.start_initial(observation).await {
            Ok(turn) => turn,
            Err(error) => {
                let _ = session.try_finish();
                return Err(error);
            }
        };
        Ok((session, turn))
    }

    /// Constructs an undispatched owner. No policy budget is reserved yet.
    pub fn try_new(
        input: TerraControllerRunInput,
        transport_config: AgentProviderTransportConfig,
        credential: AgentProviderCredential,
        model: AgentBrowserModel,
        retention: AgentBrowserRetention,
    ) -> Result<Self, AgentBrowserProviderError> {
        let transport = AgentProviderTransport::try_new(transport_config)
            .map_err(|_| AgentBrowserProviderError::Transport)?;
        Self::try_new_with_transport(
            input,
            BrowserSessionTransport(transport),
            credential,
            model,
            retention,
        )
    }

    fn try_new_with_transport(
        input: TerraControllerRunInput,
        transport: BrowserSessionTransport,
        credential: AgentProviderCredential,
        model: AgentBrowserModel,
        retention: AgentBrowserRetention,
    ) -> Result<Self, AgentBrowserProviderError> {
        let TerraControllerRunInput {
            manifest,
            lease,
            account,
            objective,
            ids,
            clock,
            deadline,
            max_model_calls,
            max_actions,
            ..
        } = input;
        if Instant::now() >= deadline {
            return Err(AgentBrowserProviderError::Deadline);
        }
        let now = clock.now().map_err(|_| AgentBrowserProviderError::Clock)?;
        let config = match model {
            AgentBrowserModel::Terra => {
                try_terra_provider_exact_call_config(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS)
                    .map_err(|_| AgentBrowserProviderError::Catalog)?
            }
            AgentBrowserModel::Luna => {
                try_luna_provider_exact_call_config(TERRA_CONTROLLER_MAX_OUTPUT_TOKENS)
                    .map_err(|_| AgentBrowserProviderError::Catalog)?
            }
        };
        let config = config.restrict_to_locate_and_act();
        let config = match retention {
            AgentBrowserRetention::Stateless => config,
            #[cfg(feature = "probe-harness")]
            AgentBrowserRetention::InspectablePublicData => {
                config.retain_response_for_inspectable_probe()
            }
        };
        let policy = AgentRunPolicy::try_new(manifest, vec![lease])
            .map_err(|_| AgentBrowserProviderError::Authority)?;
        let next_call = ids.model_call().get();
        // Initial sample, up to two per model turn (before native inspection or
        // navigation and again at provider admission), plus action admission.
        // Cache hits consume no slot; refresh remains independently bounded.
        let max_account_attestations = 1 + 2 * usize::from(max_model_calls) + max_actions as usize;
        let mut account_attestations = Vec::with_capacity(max_account_attestations);
        account_attestations.push(account.attestation());
        Ok(Self {
            policy,
            navigation: None,
            navigation_refusal: None,
            navigation_receipt: None,
            journal: None,
            journal_receipts: 0,
            transport,
            credential: Some(credential),
            attempt: None,
            retained_terminal: None,
            extraction_output: None,
            model_receipts: Vec::with_capacity(usize::from(max_model_calls)),
            config,
            objective: Some(objective),
            model,
            lease,
            account,
            account_attestations,
            next_call,
            max_model_calls,
            max_actions,
            max_account_attestations,
            clock,
            last_policy_at: now,
            cancellation: AgentProviderCancellation::new(),
            next_action: 1,
            action: None,
            action_executions: zephium_agentic::SemanticActionExecutionCoordinator::new(),
            action_settlements: zephium_agentic::SemanticActionSettlementCoordinator::new(),
            action_refusal: None,
            action_admission_failure: None,
            action_proposal_failure: None,
            action_terminal: None,
            failure: None,
            deadline,
            turns: 0,
            finished: false,
        })
    }

    /// Starts the initial provider turn, keeping all asynchronous debt in self.
    pub async fn start_initial(
        &mut self,
        observation: &zephium_agentic::SemanticObservation,
    ) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
        self.check_live()?;
        if self.turns != 0 || self.objective.is_none() {
            return Err(AgentBrowserProviderError::Continuation);
        }
        let payload = encode_semantic_observation(
            observation,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .and_then(|encoded| encoded.admit_conservative_utf8(self.config.tokenizer()))
        .map_err(AgentBrowserProviderError::InitialEncoding)?;
        let call = self.next_model_call_request()?;
        let objective = self
            .objective
            .as_ref()
            .ok_or(AgentBrowserProviderError::Continuation)?;
        let prepared = AgentPreparedObservationRequest::try_openai_for_provider_exact_count(
            &mut self.policy,
            call,
            observation,
            payload,
            objective,
            self.config.clone(),
        )
        .map_err(|_| AgentBrowserProviderError::Authority)?;
        self.drive(prepared.into_transport_input()).await
    }

    /// Sends independently verified action state as the exact tool result.
    pub async fn continue_after_verified_action(
        &mut self,
        transition: AgentBrowserVerifiedTransition,
    ) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
        self.check_live()?;
        if self.turns >= self.max_model_calls {
            return Err(AgentBrowserProviderError::TurnLimit);
        }
        if Instant::now() >= self.deadline {
            return Err(AgentBrowserProviderError::Deadline);
        }
        let request = self.next_model_call_request()?;
        let (continuation, result) = transition
            .into_parts()
            .ok_or(AgentBrowserProviderError::Continuation)?;
        let Some(diff) = result.diff() else {
            let result = result
                .action_result()
                .ok_or(AgentBrowserProviderError::Continuation)?;
            let observation = result
                .fresh_snapshot()
                .ok_or(AgentBrowserProviderError::Continuation)?;
            let payload = encode_semantic_observation(
                observation,
                SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
            )
            .and_then(|encoded| encoded.admit_conservative_utf8(self.config.tokenizer()))
            .map_err(AgentBrowserProviderError::InitialEncoding)?;
            let prepared =
                AgentPreparedObservationRequest::try_verified_action_for_provider_exact_count(
                    &mut self.policy,
                    request,
                    result,
                    payload,
                    self.config.clone(),
                    continuation,
                )
                .map_err(|_| AgentBrowserProviderError::Authority)?;
            return self.drive(prepared.into_transport_input()).await;
        };
        let payload = encode_semantic_diff(
            diff,
            SemanticModelEncodingBudget::ACTION_DIFF_PROVIDER_EXACT_CONSERVATIVE,
        )
        .and_then(|encoded| encoded.admit_conservative_utf8(self.config.tokenizer()))
        .map_err(AgentBrowserProviderError::DiffEncoding)?;
        let bound = continuation
            .bind_diff_request(request, &self.config, diff, payload)
            .map_err(|_| AgentBrowserProviderError::Continuation)?;
        let draft = AgentProviderDiffRequestDraft::try_new(bound)
            .map_err(|_| AgentBrowserProviderError::Continuation)?;
        let prepared = draft
            .try_prepare_for_provider_exact_count(&mut self.policy, request, diff)
            .map_err(|_| AgentBrowserProviderError::Authority)?;
        self.drive(prepared.into_transport_input()).await
    }

    /// Executes one bounded semantic locate requested by the prior model turn.
    ///
    /// The model query is never interpreted as a selector. It enters the fixed
    /// Rust matcher against the exact acknowledged observation, and only the
    /// bounded content-free locate projection is returned to the provider.
    pub async fn continue_after_locate(
        &mut self,
        turn: AgentProviderSettledToolTurn,
        observation: &zephium_agentic::SemanticObservation,
        locate_id: u64,
    ) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
        self.check_live()?;
        if self.turns >= self.max_model_calls {
            return Err(AgentBrowserProviderError::TurnLimit);
        }
        if Instant::now() >= self.deadline {
            return Err(AgentBrowserProviderError::Deadline);
        }
        let (proposal, continuation) = turn.into_parts();
        let zephium_agentic::AgentBrowserToolProposal::Locate { query, scope } = proposal else {
            return Err(AgentBrowserProviderError::LocateTool);
        };
        let frames = observation
            .frames()
            .iter()
            .map(|snapshot| snapshot.frame().clone())
            .collect::<Vec<_>>();
        let query = query.into_locate_query();
        let request = SemanticLocateRequest::bind(
            SemanticLocateId::new(locate_id).ok_or(AgentBrowserProviderError::Authority)?,
            observation,
            continuation.baseline(),
            &frames,
            query,
            scope
                .try_into_locate_scope()
                .map_err(|_| AgentBrowserProviderError::LocateTool)?,
            SemanticLocateBudget::STANDARD,
        )
        .map_err(|_| AgentBrowserProviderError::Locate)?;
        let result = locate_semantic_observation(observation, request)
            .map_err(|_| AgentBrowserProviderError::Locate)?;
        // A miss is a completed inspection of this bounded baseline. Deliver
        // it through the same authenticated, budgeted continuation as a match;
        // the model may simplify its query or request a fresh scoped snapshot.
        let payload = encode_semantic_locate_result(
            &result,
            SemanticModelEncodingBudget::LOCATE_RESULT_PROVIDER_EXACT_CONSERVATIVE,
        )
        .and_then(|encoded| encoded.admit_conservative_utf8(self.config.tokenizer()))
        .map_err(AgentBrowserProviderError::LocateEncoding)?;
        let model_request = self.next_model_call_request()?;
        let bound = continuation
            .bind_locate_request(model_request, &self.config, &result, payload)
            .map_err(|_| AgentBrowserProviderError::Continuation)?;
        let draft = AgentProviderLocateRequestDraft::try_new(bound)
            .map_err(|_| AgentBrowserProviderError::Continuation)?;
        let prepared = draft
            .try_prepare_for_provider_exact_count(&mut self.policy, model_request, &result)
            .map_err(|_| AgentBrowserProviderError::Authority)?;
        self.drive(prepared.into_transport_input()).await
    }

    fn next_model_call_request(
        &mut self,
    ) -> Result<AgentModelCallRequest, AgentBrowserProviderError> {
        let call_id =
            AgentModelCallId::new(self.next_call).ok_or(AgentBrowserProviderError::Authority)?;
        self.next_call = self
            .next_call
            .checked_add(1)
            .ok_or(AgentBrowserProviderError::Authority)?;
        let now = self.policy_now()?;
        Ok(AgentModelCallRequest::new(
            call_id,
            self.lease.lease(),
            self.account,
            browser_call_budget(self.model)?,
            now,
        ))
    }

    /// Continues one settled read proposal against the exact already-delivered
    /// initial baseline. The host supplies its original trusted capture time;
    /// this function never captures, refreshes or acknowledges a new document.
    /// Policy still admits the read's source cohort and exact whole request.
    pub async fn continue_after_read(
        &mut self,
        turn: AgentProviderSettledToolTurn,
        observation: &zephium_agentic::SemanticObservation,
        captured_at: zephium_agentic::SemanticCaptureInstant,
    ) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
        use zephium_agentic::*;
        self.check_live()?;
        if self.turns >= self.max_model_calls {
            return Err(AgentBrowserProviderError::TurnLimit);
        }
        let (proposal, continuation) = turn.into_parts();
        if !self.config.permits_baseline_read()
            || !matches!(
                proposal,
                AgentBrowserToolProposal::Read(AgentBrowserScopeProposal::Initial)
            )
        {
            return Err(AgentBrowserProviderError::UnsupportedTool(proposal.kind()));
        }
        let read = read_semantic_observation(
            observation,
            SemanticReadAuthority::Acknowledged(continuation.baseline()),
            captured_at,
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .map_err(AgentBrowserProviderError::Read)?;
        let payload = encode_semantic_read(
            &read,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .and_then(|encoded| encoded.admit_conservative_utf8(self.config.tokenizer()))
        .map_err(AgentBrowserProviderError::ReadEncoding)?;
        let request = self.next_model_call_request()?;
        let bound = continuation
            .bind_read_request(request, &self.config, &read, payload)
            .map_err(|_| AgentBrowserProviderError::Continuation)?;
        let prepared = AgentProviderReadContinuationRequestDraft::try_new(bound)
            .and_then(|draft| {
                draft.try_prepare_for_provider_exact_count(&mut self.policy, request, &read)
            })
            .map_err(|_| AgentBrowserProviderError::Authority)?;
        self.drive(prepared.into_transport_input()).await
    }

    /// Consumes one schema-bound terminal extraction proposal. The returned
    /// mapping remains model-mapped data with exact delivered provenance, not
    /// an independently verified fact or task-completion authority.
    pub async fn extract<'a>(
        &mut self,
        turn: AgentBrowserProviderTurn,
        observation: &'a zephium_agentic::SemanticObservation,
        frames: &[zephium_agentic::SemanticFrameJoin],
        captured_at: zephium_agentic::SemanticCaptureInstant,
        schema: &zephium_agentic::SemanticExtractionSchema,
    ) -> Result<zephium_agentic::SemanticExtractionResult<'a>, AgentBrowserProviderError> {
        self.extract_from(turn, observation, None, frames, captured_at, schema)
            .await
    }

    async fn extract_from<'a>(
        &mut self,
        turn: AgentBrowserProviderTurn,
        observation: &'a zephium_agentic::SemanticObservation,
        previous: Option<&zephium_agentic::SemanticObservation>,
        frames: &[zephium_agentic::SemanticFrameJoin],
        captured_at: zephium_agentic::SemanticCaptureInstant,
        schema: &zephium_agentic::SemanticExtractionSchema,
    ) -> Result<zephium_agentic::SemanticExtractionResult<'a>, AgentBrowserProviderError> {
        self.extract_from_with_evidence(
            turn,
            observation,
            previous,
            frames,
            captured_at,
            schema,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn extract_from_with_evidence<'a>(
        &mut self,
        turn: AgentBrowserProviderTurn,
        observation: &'a zephium_agentic::SemanticObservation,
        previous: Option<&zephium_agentic::SemanticObservation>,
        frames: &[zephium_agentic::SemanticFrameJoin],
        captured_at: zephium_agentic::SemanticCaptureInstant,
        schema: &zephium_agentic::SemanticExtractionSchema,
        evidence: Option<&'a zephium_agentic::SemanticRetainedReadEvidence>,
    ) -> Result<zephium_agentic::SemanticExtractionResult<'a>, AgentBrowserProviderError> {
        use zephium_agentic::*;
        self.check_live()?;
        if self.turns >= self.max_model_calls {
            return Err(AgentBrowserProviderError::TurnLimit);
        }
        if frames.len() != observation.frames().len()
            || observation
                .frames()
                .iter()
                .any(|frame| !frames.contains(frame.frame()))
            || schema.id().get() != 1
        {
            return Err(AgentBrowserProviderError::Authority);
        }
        let (proposal, continuation) = turn.into_tool_turn().into_parts();
        let permitted = match (&proposal, previous) {
            (
                AgentBrowserToolProposal::Extract {
                    scope: AgentBrowserScopeProposal::Initial,
                    schema: id,
                },
                None,
            ) => *id == schema.id(),
            (
                AgentBrowserToolProposal::Extract {
                    scope: AgentBrowserScopeProposal::Subtree(_),
                    schema: id,
                },
                Some(_),
            ) => *id == schema.id(),
            _ => false,
        };
        if !permitted {
            return Err(AgentBrowserProviderError::UnsupportedTool(proposal.kind()));
        }
        if let Some(journal) = &self.journal {
            journal
                .emit(work::AgentWorkEventKind::ToolProposed(
                    AgentBrowserToolKind::Extract,
                ))
                .map_err(|_| AgentBrowserProviderError::Journal)?;
        }
        let read = zephium_agentic::read_selected_semantic_observation(
            observation,
            match previous {
                Some(previous) => SemanticReadAuthority::AcknowledgedExpansion {
                    previous,
                    acknowledgement: continuation.baseline(),
                },
                None => SemanticReadAuthority::Acknowledged(continuation.baseline()),
            },
            captured_at,
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
            schema.source_roles(),
        )
        .map_err(AgentBrowserProviderError::Read)?;
        let read = match evidence {
            Some(evidence) => evidence
                .merge_for_extraction(read)
                .map_err(AgentBrowserProviderError::Read)?,
            None => read,
        };
        // Every mapped value requires a delivered source fragment. An empty
        // read cannot satisfy a required field, so do not spend a provider
        // request asking the model to manufacture an impossible result.
        if read.fragments().is_empty() && schema.fields().iter().any(|field| field.required()) {
            return Err(AgentBrowserProviderError::NoExtractionEvidence);
        }
        let payload = encode_semantic_extraction_request(
            schema,
            &read,
            SemanticModelEncodingBudget::EXTRACTION_PROVIDER_EXACT_CONSERVATIVE,
        )
        .and_then(|encoded| encoded.admit_conservative_utf8(self.config.tokenizer()))
        .map_err(AgentBrowserProviderError::InitialEncoding)?;
        let request = self.next_model_call_request()?;
        let bound = continuation
            .bind_extraction_request(request, &self.config, schema, &read, payload)
            .map_err(|_| AgentBrowserProviderError::Continuation)?;
        let prepared = AgentProviderExtractionRequestDraft::try_new(bound)
            .and_then(|draft| {
                draft.try_prepare_for_provider_exact_count(&mut self.policy, request, schema, &read)
            })
            .map_err(|_| AgentBrowserProviderError::Authority)?;
        let (input, output) = prepared.into_transport_parts();
        let (terminal, _, _) = self.drive_terminal(input, Some(output)).await?;
        self.extraction_output
            .take()
            .ok_or(AgentBrowserProviderError::Authority)?
            .finish(
                &terminal,
                schema,
                &read,
                SemanticReadSensitivityLimit::PublicOnly,
            )
            .map_err(AgentBrowserProviderError::Extraction)
    }

    async fn drive(
        &mut self,
        input: AgentProviderTransportInput,
    ) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
        let (terminal, input, provider_elapsed) = self.drive_terminal(input, None).await?;
        let receipt = terminal.receipt();
        let turn = terminal
            .into_tool_turn()
            .ok_or(AgentBrowserProviderError::Proposal)?;
        Ok(AgentBrowserProviderTurn {
            receipt,
            input,
            turn,
            provider_elapsed,
        })
    }

    async fn drive_terminal(
        &mut self,
        input: AgentProviderTransportInput,
        output: Option<zephium_agentic::AgentProviderExtractionOutputBinding>,
    ) -> Result<
        (
            zephium_agentic::AgentProviderSettledTerminal,
            zephium_agentic::AgentProviderInputMetricReceipt,
            Duration,
        ),
        AgentBrowserProviderError,
    > {
        let provider_started = Instant::now();
        if let Err(error) = self.check_live() {
            let _ = input.cancel(&mut self.policy);
            return Err(error);
        }
        if self.turns >= self.max_model_calls {
            let _ = input.cancel(&mut self.policy);
            return Err(AgentBrowserProviderError::TurnLimit);
        }
        let cancellation = self.cancellation.clone();
        if self.attempt.is_some()
            || self.retained_terminal.is_some()
            || self.extraction_output.is_some()
        {
            let _ = input.cancel(&mut self.policy);
            return Err(AgentBrowserProviderError::Transport);
        }
        let Some(credential) = self.credential.as_ref() else {
            let _ = input.cancel(&mut self.policy);
            return Err(AgentBrowserProviderError::Cancelled);
        };
        self.turns = self
            .turns
            .checked_add(1)
            .ok_or(AgentBrowserProviderError::TurnLimit)?;
        self.attempt = Some(
            self.transport
                .try_admit(input, &mut self.policy, credential, cancellation)
                .map_err(|_| AgentBrowserProviderError::Transport)?,
        );
        if let Some(output) = output {
            let evidence = self
                .attempt
                .as_ref()
                .and_then(AgentProviderAttempt::input_evidence)
                .ok_or(AgentBrowserProviderError::Authority)?;
            self.extraction_output = Some(
                output
                    .start(evidence)
                    .map_err(AgentBrowserProviderError::Extraction)?,
            );
        }
        if let Some(journal) = self.journal.as_mut() {
            let call = self
                .attempt
                .as_ref()
                .and_then(AgentProviderAttempt::call)
                .ok_or(AgentBrowserProviderError::Journal)?;
            journal
                .model_active(call)
                .map_err(|_| AgentBrowserProviderError::Journal)?;
        }
        let counted = tokio::time::timeout_at(
            tokio::time::Instant::from_std(self.deadline),
            self.attempt
                .as_mut()
                .ok_or(AgentBrowserProviderError::Transport)?
                .count_openai_input_tokens(),
        )
        .await;
        let result = match counted {
            Ok(zephium_agent_provider_transport::AgentProviderExactCountOutcome::Counted(
                counted,
            )) => match tokio::time::timeout_at(
                tokio::time::Instant::from_std(self.deadline),
                counted.execute(|batch| {
                    if self
                        .extraction_output
                        .as_mut()
                        .is_some_and(|output| output.push_batch(batch).is_err())
                    {
                        AgentProviderBatchDisposition::Cancel
                    } else {
                        AgentProviderBatchDisposition::Continue
                    }
                }),
            )
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(_)) | Err(_) => self
                    .attempt
                    .take()
                    .ok_or(AgentBrowserProviderError::Transport)?
                    .abort(AgentProviderAbortReason::HostDeadline)
                    .map_err(|_| AgentBrowserProviderError::Transport)?,
            },
            Ok(zephium_agent_provider_transport::AgentProviderExactCountOutcome::Failed(
                result,
            )) => result,
            Ok(zephium_agent_provider_transport::AgentProviderExactCountOutcome::Unavailable(
                _,
            )) => self
                .attempt
                .take()
                .ok_or(AgentBrowserProviderError::Transport)?
                .abort(AgentProviderAbortReason::ControllerFault)
                .map_err(|_| AgentBrowserProviderError::Transport)?,
            Err(_) => self
                .attempt
                .take()
                .ok_or(AgentBrowserProviderError::Transport)?
                .abort(AgentProviderAbortReason::HostDeadline)
                .map_err(|_| AgentBrowserProviderError::Transport)?,
        };
        drop(self.attempt.take());
        let input = result.input_metric_receipt();
        let disclosure = result.disclosure_stage();
        let terminal = settle_browser_terminal(
            result,
            disclosure,
            input,
            self.model,
            &mut self.policy,
            &mut self.retained_terminal,
            &mut self.model_receipts,
        );
        self.record_model_receipts()?;
        Ok((terminal?, input, provider_started.elapsed()))
    }

    /// Seals admission and returns all run owners, even when drain refuses.
    ///
    /// This terminal is not durable-audit or application shutdown proof.
    /// The application must keep it until its own run/audit lifecycle closes.
    pub fn try_finish(
        self,
    ) -> Result<AgentBrowserSessionTerminal, AgentBrowserSessionFinishRefusal> {
        self.seal_terminal(false)
    }

    // Resource drain is independent of the task outcome. This private actor
    // path preserves every original owner and failure; it cannot manufacture
    // successful completion or bypass the later policy/metric/audit closure.
    fn try_finish_unsuccessful(
        self,
    ) -> Result<AgentBrowserSessionTerminal, AgentBrowserSessionFinishRefusal> {
        self.seal_terminal(true)
    }

    fn seal_terminal(
        mut self,
        unsuccessful: bool,
    ) -> Result<AgentBrowserSessionTerminal, AgentBrowserSessionFinishRefusal> {
        // Partial model mappings are never recovery artifacts. Their attempt
        // and policy/accounting owners remain in the normal close path.
        self.extraction_output.take();
        self.transport.seal();
        self.cancellation.cancel();
        self.action_executions.seal();
        self.action_settlements.seal();
        drop(self.credential.take());
        drop(self.objective.take());
        let mut close_failure = None;
        if let Some(attempt) = self.attempt.take() {
            match attempt.abort(AgentProviderAbortReason::ControllerFault) {
                Ok(result) => {
                    let input = result.input_metric_receipt();
                    let disclosure = result.disclosure_stage();
                    let _ = settle_browser_result(
                        result,
                        disclosure,
                        input,
                        self.model,
                        &mut self.policy,
                        &mut self.retained_terminal,
                        &mut self.model_receipts,
                    );
                }
                Err(_) => close_failure = Some(AgentBrowserProviderError::Transport),
            }
        }
        if self.record_model_receipts().is_err() {
            close_failure = Some(AgentBrowserProviderError::Journal);
        }
        if let Some(failure) = close_failure {
            // A refused drain keeps the same sticky failure on its returned
            // session; a later public finish cannot forget the close error.
            self.failure = Some(failure);
        }
        let provider = self.transport.try_prove_shutdown();
        let error = if provider.is_err() {
            Some(AgentBrowserProviderError::Transport)
        } else if self.action.is_some()
            || self.navigation.is_some()
            || self.navigation_refusal.is_some()
            || self.navigation_receipt.is_some()
            || self.action_executions.status().pending() != 0
            || self.action_settlements.status().pending() != 0
            || self.action_refusal.is_some()
            || self.action_admission_failure.is_some()
            || self
                .action_proposal_failure
                .as_ref()
                .is_some_and(|refusal| !unsuccessful || refusal.human_review().is_none())
            || self.action_terminal.is_some()
            || self.retained_terminal.is_some()
            || self.policy.is_sealed()
            || self.policy.pending_model_calls() != 0
            || self.policy.pending_effects() != 0
            || self.policy.accounting().reserved_operations() != 0
            || self.policy.accounting().reserved_model_tokens() != 0
            || self.policy.accounting().reserved_cost_micro_usd() != 0
        {
            Some(AgentBrowserProviderError::ActionPending)
        } else if close_failure.is_some() {
            close_failure
        } else if !unsuccessful {
            self.failure
        } else {
            None
        };
        self.finished = true;
        match (error, provider) {
            (Some(error), _) => Err(AgentBrowserSessionFinishRefusal {
                error,
                session: Box::new(self),
            }),
            (None, Ok(provider)) => Ok(AgentBrowserSessionTerminal {
                session: Box::new(self),
                provider: provider.into(),
            }),
            (None, Err(_)) => Err(AgentBrowserSessionFinishRefusal {
                error: AgentBrowserProviderError::Transport,
                session: Box::new(self),
            }),
        }
    }

    /// Compatibility close for release-excluded standalone qualifiers.
    #[cfg(feature = "probe-harness")]
    pub fn finish(self) -> Result<(), AgentBrowserProviderError> {
        self.try_finish()
            .map(|_| ())
            .map_err(|refusal| refusal.error)
    }

    fn record_model_receipts(&mut self) -> Result<(), AgentBrowserProviderError> {
        if let Some(journal) = self.journal.as_mut() {
            for &(receipt, input) in &self.model_receipts[self.journal_receipts..] {
                journal
                    .model_settled(receipt, input)
                    .map_err(|_| AgentBrowserProviderError::Journal)?;
                self.journal_receipts += 1;
            }
        }
        Ok(())
    }

    fn policy_now(&mut self) -> Result<AgentPolicyInstant, AgentBrowserProviderError> {
        let now = self
            .clock
            .now()
            .map_err(|_| AgentBrowserProviderError::Clock)?;
        if now < self.last_policy_at
            || now < self.policy.manifest().issued_at()
            || now > self.policy.manifest().expires_at()
        {
            return Err(AgentBrowserProviderError::Clock);
        }
        self.last_policy_at = now;
        Ok(now)
    }

    fn check_live(&self) -> Result<(), AgentBrowserProviderError> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        if self.finished || self.cancellation.is_cancelled() {
            return Err(AgentBrowserProviderError::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(AgentBrowserProviderError::Deadline);
        }
        Ok(())
    }

    /// Permanently revokes model admission. The host must also revoke native input.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Runs bounded semantic locate turns until the model proposes one action.
    ///
    /// The host supplies the complete native-current frame cohort. No tool is
    /// retried on refusal. Every provider turn is exposed only as content-free
    /// accounting. Run allowance and transcript byte/input limits are checked
    /// independently before each admission.
    pub async fn next_action(
        &mut self,
        turn: AgentBrowserProviderTurn,
        observation: &zephium_agentic::SemanticObservation,
        current_frames: &[zephium_agentic::SemanticFrameJoin],
        record: impl FnMut(
            AgentModelCallReceipt,
            zephium_agentic::AgentProviderInputMetricReceipt,
            Duration,
        ),
    ) -> Result<crate::AgentBrowserActionProposal, AgentBrowserProviderError> {
        let turn = self
            .next_step(turn, observation, current_frames, None, false, record)
            .await?;
        match self.bind_action_turn(turn, observation, current_frames)? {
            crate::action::AgentBrowserActionBinding::Prepared(proposal) => Ok(proposal),
            crate::action::AgentBrowserActionBinding::Refused(refusal) => {
                Err(AgentBrowserProviderError::Action(
                    crate::AgentBrowserActionError::Binding(refusal.reason()),
                ))
            }
        }
    }

    // Shares exact locate/action ownership with the public action driver.
    // Extract retains its original settled turn until the trusted Work task
    // authorizes readiness against the same current observation.
    async fn next_step(
        &mut self,
        mut turn: AgentBrowserProviderTurn,
        observation: &zephium_agentic::SemanticObservation,
        current_frames: &[zephium_agentic::SemanticFrameJoin],
        captured_at: Option<zephium_agentic::SemanticCaptureInstant>,
        extraction: bool,
        mut record: impl FnMut(
            AgentModelCallReceipt,
            zephium_agentic::AgentProviderInputMetricReceipt,
            Duration,
        ),
    ) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
        loop {
            self.check_live()?;
            record(turn.receipt(), turn.input(), turn.provider_elapsed());
            if turn.turn.proposal().kind() == zephium_agentic::AgentBrowserToolKind::Act
                || (extraction
                    && turn.turn.proposal().kind()
                        == zephium_agentic::AgentBrowserToolKind::Extract)
            {
                return Ok(turn);
            }
            let tool = turn.into_tool_turn();
            turn = self
                .continue_inspection(tool, observation, current_frames, captured_at)
                .await?;
        }
    }

    // Exactly one non-native inspection turn. Work owns the outer turn loop so
    // it can resample trusted account authority before every provider admission.
    async fn continue_inspection(
        &mut self,
        tool: AgentProviderSettledToolTurn,
        observation: &zephium_agentic::SemanticObservation,
        current_frames: &[zephium_agentic::SemanticFrameJoin],
        captured_at: Option<zephium_agentic::SemanticCaptureInstant>,
    ) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
        self.check_live()?;
        if let Some(journal) = self.journal.as_ref() {
            journal
                .emit(work::AgentWorkEventKind::ToolProposed(
                    tool.proposal().kind(),
                ))
                .map_err(|_| AgentBrowserProviderError::Journal)?;
        }
        match tool.proposal().kind() {
            zephium_agentic::AgentBrowserToolKind::Locate
            | zephium_agentic::AgentBrowserToolKind::Read => {
                // Locate is observation-bound, and its ref inventory must
                // also remain current at the caller's registry boundary.
                if current_frames.len() != observation.frames().len()
                    || observation
                        .frames()
                        .iter()
                        .any(|frame| !current_frames.contains(frame.frame()))
                {
                    return Err(AgentBrowserProviderError::Authority);
                }
                if tool.proposal().kind() == zephium_agentic::AgentBrowserToolKind::Read {
                    self.continue_after_read(
                        tool,
                        observation,
                        captured_at.ok_or(AgentBrowserProviderError::Authority)?,
                    )
                    .await
                } else {
                    self.continue_after_locate(tool, observation, self.next_call)
                        .await
                }
            }
            kind => Err(AgentBrowserProviderError::UnsupportedTool(kind)),
        }
    }

    fn bind_action_turn(
        &self,
        turn: AgentBrowserProviderTurn,
        observation: &zephium_agentic::SemanticObservation,
        current_frames: &[zephium_agentic::SemanticFrameJoin],
    ) -> Result<crate::action::AgentBrowserActionBinding, AgentBrowserProviderError> {
        self.check_live()?;
        if self.next_action > self.max_actions {
            return Err(AgentBrowserProviderError::ActionLimit);
        }
        if self.action.is_some() {
            return Err(AgentBrowserProviderError::ActionPending);
        }
        let tool = turn.into_tool_turn();
        if let Some(journal) = &self.journal {
            journal
                .emit(work::AgentWorkEventKind::ToolProposed(
                    tool.proposal().kind(),
                ))
                .map_err(|_| AgentBrowserProviderError::Journal)?;
        }
        let batch = zephium_agentic::SemanticActionBatchId::new(self.next_action)
            .ok_or(AgentBrowserProviderError::Authority)?;
        crate::AgentBrowserActionProposal::bind(
            tool,
            observation,
            current_frames,
            batch,
            &self.config,
        )
        .map_err(AgentBrowserProviderError::Action)
    }

    /// Applies real policy to an independently assessed action and retains its debt.
    pub fn authorize_action(
        &mut self,
        proposal: crate::AgentBrowserActionProposal,
        assessment: &zephium_agentic::AgentEffectAssessment,
        automation: zephium_agentic::ContextAutomationState,
        requested_at: zephium_agentic::SemanticActionExecutionInstant,
    ) -> Result<zephium_agentic::SemanticActionNativeRequest, AgentBrowserProviderError> {
        self.check_live()?;
        if self.next_action > self.max_actions {
            return Err(AgentBrowserProviderError::ActionLimit);
        }
        if self.action.is_some() {
            return Err(AgentBrowserProviderError::ActionPending);
        }
        let now = self.policy_now()?;
        let effect = zephium_agentic::AgentEffectId::new(self.next_action)
            .ok_or(AgentBrowserProviderError::Authority)?;
        let attempt = zephium_agentic::SemanticActionAttemptId::new(self.next_action)
            .ok_or(AgentBrowserProviderError::Authority)?;
        self.next_action = self
            .next_action
            .checked_add(1)
            .ok_or(AgentBrowserProviderError::TurnLimit)?;
        let request = zephium_agentic::AgentEffectRequest::new(
            effect,
            self.lease.lease(),
            self.account,
            automation,
            now,
        );
        let dispatch = zephium_agentic::AgentEffectDispatchRequest::new(
            attempt,
            self.account,
            automation,
            now,
        );
        let mut action = proposal
            .authorize(
                &mut self.policy,
                request,
                assessment,
                dispatch,
                requested_at,
                &mut self.action_executions,
                self.journal.as_mut(),
                &mut self.action_admission_failure,
                &mut self.action_proposal_failure,
            )
            .map_err(|error| {
                let error = AgentBrowserProviderError::Action(error);
                self.failure = Some(error);
                error
            })?;
        if action.journal_failed() {
            self.action = Some(action);
            self.failure = Some(AgentBrowserProviderError::Journal);
            return Err(AgentBrowserProviderError::Journal);
        }
        let native = action
            .take_native_request()
            .map_err(AgentBrowserProviderError::Action)?;
        self.action = Some(action);
        Ok(native)
    }

    /// Accounts native admission; a fully accounted rejection stops the task
    /// while permitting the ordinary unsuccessful resource closure.
    pub(crate) fn account_action_dispatch(
        &mut self,
        dispatch: ContextDispatch,
    ) -> Result<(), AgentBrowserProviderError> {
        let result = self
            .action
            .as_mut()
            .ok_or(AgentBrowserProviderError::ActionPending)?
            .account_dispatch(dispatch, &mut self.policy, &mut self.action_executions);
        let Err(error) = result else {
            return Ok(());
        };
        let original = AgentBrowserProviderError::Action(error);
        self.failure = Some(original);
        // Rejected is the native port's synchronous non-admission contract.
        // Scheduled failures, absent/mismatched callbacks and pending settlement
        // retain their original action owner and can never enter this branch.
        if !matches!(dispatch, ContextDispatch::Rejected(_))
            || !matches!(error, crate::AgentBrowserActionError::Failed(_))
            || self.action_executions.status().pending() != 0
            || self.action_settlements.status().pending() != 0
            || self.action_terminal.is_some()
        {
            return Err(original);
        }
        let action = self
            .action
            .take()
            .ok_or(AgentBrowserProviderError::ActionPending)?;
        let terminal = match action.into_rejected_batch() {
            Ok(terminal) => terminal,
            Err(action) => {
                self.action = Some(*action);
                return Err(original);
            }
        };
        // Preserve the exact batch before any fallible reducer/audit work. A
        // partial journal update remains Recovery and is never replayed.
        self.action_terminal = Some(terminal);
        let recorded = self.journal.as_mut().is_some_and(|journal| {
            journal
                .action_rejected(self.action_terminal.as_ref().expect("retained terminal"))
                .is_ok()
        });
        if !recorded {
            self.failure = Some(AgentBrowserProviderError::Journal);
            return Err(AgentBrowserProviderError::Journal);
        }
        self.action_terminal.take();
        // Accounting a refusal closes debt, not the failed task. The sticky
        // failure prevents another provider turn or any automatic retry.
        Err(original)
    }

    /// Independently verifies and accounts the pending action before continuation.
    pub fn settle_action(
        &mut self,
        native: zephium_agentic::SemanticActionNativeSettlement,
        baseline: &zephium_agentic::SemanticObservation,
        current: &zephium_agentic::SemanticObservation,
        observed_at: zephium_agentic::SemanticSettleInstant,
    ) -> Result<
        (
            zephium_agentic::AgentEffectReceipt,
            AgentBrowserVerifiedTransition,
        ),
        AgentBrowserProviderError,
    > {
        let action = self
            .action
            .as_mut()
            .ok_or(AgentBrowserProviderError::ActionPending)?;
        let accounted = match action.settle(
            &mut self.policy,
            native,
            current,
            observed_at,
            &mut self.action_executions,
            &mut self.action_settlements,
        ) {
            Ok(accounted) => accounted,
            Err(error) => {
                let error = AgentBrowserProviderError::Action(error);
                if error
                    != AgentBrowserProviderError::Action(
                        crate::AgentBrowserActionError::SettlementPending,
                    )
                {
                    self.failure = Some(error);
                }
                return Err(error);
            }
        };
        self.finish_action(accounted, baseline, current, observed_at)
    }

    /// Moves the exact native terminal into its original settlement owner.
    /// A returned wake is a clock boundary, never evidence of task completion.
    pub fn begin_action_settlement(
        &mut self,
        native: zephium_agentic::SemanticActionNativeSettlement,
    ) -> Result<Option<zephium_agentic::SemanticSettleInstant>, AgentBrowserProviderError> {
        self.action
            .as_mut()
            .ok_or(AgentBrowserProviderError::ActionPending)?
            .begin_settlement(
                &mut self.policy,
                native,
                &mut self.action_executions,
                &mut self.action_settlements,
            )
            .map_err(AgentBrowserProviderError::Action)
    }

    /// Advances only the retained coordinator's exact scheduled clock wake.
    pub fn wake_action_settlement(
        &mut self,
        now: zephium_agentic::SemanticSettleInstant,
    ) -> Result<Option<zephium_agentic::SemanticSettleInstant>, AgentBrowserProviderError> {
        self.action
            .as_mut()
            .ok_or(AgentBrowserProviderError::ActionPending)?
            .wake_settlement(now, &mut self.action_settlements)
            .map_err(AgentBrowserProviderError::Action)
    }

    /// Verifies the single adjacent fresh snapshot after settlement, without
    /// issuing another native action or relaxing reference freshness.
    pub fn verify_action_settlement(
        &mut self,
        baseline: &zephium_agentic::SemanticObservation,
        current: &zephium_agentic::SemanticObservation,
        observed_at: zephium_agentic::SemanticSettleInstant,
    ) -> Result<
        (
            zephium_agentic::AgentEffectReceipt,
            AgentBrowserVerifiedTransition,
        ),
        AgentBrowserProviderError,
    > {
        let result = self
            .action
            .as_mut()
            .ok_or(AgentBrowserProviderError::ActionPending)?
            .verify_settlement(&mut self.policy, current, observed_at);
        match result {
            Ok(accounted) => self.finish_action(accounted, baseline, current, observed_at),
            Err(error) => {
                let error = AgentBrowserProviderError::Action(error);
                if error
                    != AgentBrowserProviderError::Action(
                        crate::AgentBrowserActionError::SettlementPending,
                    )
                {
                    self.failure = Some(error);
                }
                Err(error)
            }
        }
    }

    fn finish_action(
        &mut self,
        accounted: zephium_agentic::AgentVerifiedSemanticEffect,
        baseline: &zephium_agentic::SemanticObservation,
        current: &zephium_agentic::SemanticObservation,
        observed_at: zephium_agentic::SemanticSettleInstant,
    ) -> Result<
        (
            zephium_agentic::AgentEffectReceipt,
            AgentBrowserVerifiedTransition,
        ),
        AgentBrowserProviderError,
    > {
        let receipt = accounted.receipt();
        let action = self
            .action
            .take()
            .ok_or(AgentBrowserProviderError::ActionPending)?;
        match action.into_transition(accounted, baseline, current, observed_at) {
            Ok(mut transition) => {
                if let Some(journal) = self.journal.as_mut() {
                    let terminal = transition
                        .batch_result()
                        .ok_or(AgentBrowserProviderError::Journal)?;
                    if journal.action_settled(receipt, terminal).is_err() {
                        self.action_terminal = transition.terminal.take();
                        self.failure = Some(AgentBrowserProviderError::Journal);
                        return Err(AgentBrowserProviderError::Journal);
                    }
                }
                Ok((receipt, transition))
            }
            Err(refusal) => {
                self.action_refusal = Some(refusal);
                self.failure = Some(AgentBrowserProviderError::Continuation);
                Err(AgentBrowserProviderError::Continuation)
            }
        }
    }
}

// The transport's field owner seals even when a whole session is dropped.
// Keeping Drop on this leaf lets a proven-drained terminal move its original
// policy, journal and native coordinators into the product closure owner.
struct BrowserSessionTransport(AgentProviderTransport);

impl std::ops::Deref for BrowserSessionTransport {
    type Target = AgentProviderTransport;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for BrowserSessionTransport {
    fn drop(&mut self) {
        self.0.seal();
    }
}

impl fmt::Debug for AgentBrowserSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentBrowserSession")
            .field("turns", &self.turns)
            .field("finished", &self.finished)
            .field("content", &"[redacted]")
            .finish()
    }
}

enum BrowserUnsettledTerminal {
    Immediate(AgentProviderImmediateSettlement),
    Terra(TerraProviderTerminalOwner),
    Luna(zephium_agent_model_catalog::LunaProviderTerminalOwner),
}

impl BrowserUnsettledTerminal {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Immediate(value) => std::mem::size_of_val(value),
            Self::Terra(value) => std::mem::size_of_val(value),
            Self::Luna(value) => std::mem::size_of_val(value),
        }
    }
}

/// Sealed, provider-drained session retaining exact policy ownership for its host.
/// This is not durable audit, native teardown, or full run completion proof.
#[must_use]
pub struct AgentBrowserSessionTerminal {
    session: Box<AgentBrowserSession>,
    provider: zephium_agentic::AgentProviderShutdownProof,
}

impl AgentBrowserSessionTerminal {
    /// Every charged provider terminal, including failures, retained for host audit.
    pub fn model_receipts(
        &self,
    ) -> &[(
        AgentModelCallReceipt,
        zephium_agentic::AgentProviderInputMetricReceipt,
    )] {
        &self.session.model_receipts
    }
    /// Exact consumed policy totals; every reservation was checked empty.
    pub fn accounting(&self) -> zephium_agentic::AgentPolicyAccounting {
        self.session.policy.accounting()
    }
}

impl fmt::Debug for AgentBrowserSessionTerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentBrowserSessionTerminal")
            .field("accounting", &self.accounting())
            .finish_non_exhaustive()
    }
}

/// Lossless close refusal retaining the complete session and outstanding debt.
#[must_use]
pub struct AgentBrowserSessionFinishRefusal {
    error: AgentBrowserProviderError,
    session: Box<AgentBrowserSession>,
}

impl AgentBrowserSessionFinishRefusal {
    /// Closed reason the session could not prove provider/policy drain.
    pub const fn error(&self) -> AgentBrowserProviderError {
        self.error
    }
    /// Recovers the sealed owner for exact terminal reconciliation, never retry.
    pub fn into_session(self) -> AgentBrowserSession {
        *self.session
    }
}

impl fmt::Debug for AgentBrowserSessionFinishRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentBrowserSessionFinishRefusal")
            .field("error", &self.error)
            .field(
                "retained_terminal_bytes",
                &self
                    .session
                    .retained_terminal
                    .as_ref()
                    .map(BrowserUnsettledTerminal::retained_bytes),
            )
            .finish_non_exhaustive()
    }
}

fn browser_call_budget(
    model: AgentBrowserModel,
) -> Result<AgentModelCallBudget, AgentBrowserProviderError> {
    let (input_ceiling, reservation) = match model {
        AgentBrowserModel::Terra => (
            TERRA_STANDARD_RATE_MAX_INPUT_TOKENS,
            TERRA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD,
        ),
        AgentBrowserModel::Luna => (
            LUNA_STANDARD_RATE_MAX_INPUT_TOKENS,
            LUNA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD,
        ),
    };
    let input_ceiling =
        u32::try_from(input_ceiling).map_err(|_| AgentBrowserProviderError::Catalog)?;
    AgentModelCallBudget::try_new(
        input_ceiling,
        TERRA_CONTROLLER_MAX_OUTPUT_TOKENS,
        reservation,
    )
    .map_err(|_| AgentBrowserProviderError::Catalog)
}

fn settle_browser_result(
    result: AgentProviderTransportResult,
    disclosure: AgentProviderDisclosureStage,
    input: zephium_agentic::AgentProviderInputMetricReceipt,
    model: AgentBrowserModel,
    policy: &mut AgentRunPolicy,
    retained: &mut Option<BrowserUnsettledTerminal>,
    receipts: &mut Vec<(
        AgentModelCallReceipt,
        zephium_agentic::AgentProviderInputMetricReceipt,
    )>,
) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
    let terminal =
        settle_browser_terminal(result, disclosure, input, model, policy, retained, receipts)?;
    let receipt = terminal.receipt();
    let turn = terminal
        .into_tool_turn()
        .ok_or(AgentBrowserProviderError::Proposal)?;
    Ok(AgentBrowserProviderTurn {
        receipt,
        input,
        turn,
        provider_elapsed: Duration::ZERO,
    })
}

fn settle_browser_terminal(
    result: AgentProviderTransportResult,
    disclosure: AgentProviderDisclosureStage,
    input: zephium_agentic::AgentProviderInputMetricReceipt,
    model: AgentBrowserModel,
    policy: &mut AgentRunPolicy,
    retained: &mut Option<BrowserUnsettledTerminal>,
    receipts: &mut Vec<(
        AgentModelCallReceipt,
        zephium_agentic::AgentProviderInputMetricReceipt,
    )>,
) -> Result<zephium_agentic::AgentProviderSettledTerminal, AgentBrowserProviderError> {
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
            let receipt = settlement.settle(policy).map_err(|error| {
                *retained = error
                    .into_unsettled()
                    .map(BrowserUnsettledTerminal::Immediate);
                AgentBrowserProviderError::Settlement
            })?;
            receipts.push((receipt, input));
            Err(match terminal_failure {
                Some(failure) => match disclosure {
                    AgentProviderDisclosureStage::NotDispatched => {
                        AgentBrowserProviderError::PreDispatchTerminal(failure.class())
                    }
                    AgentProviderDisclosureStage::InputTokenCountDisclosed => {
                        AgentBrowserProviderError::CountTerminal(failure.class())
                    }
                    AgentProviderDisclosureStage::ModelRequestMayHaveDispatched => {
                        match failure.protocol_error() {
                            Some(error) => AgentBrowserProviderError::ModelProtocol(
                                error,
                                failure.protocol_event(),
                            ),
                            None => AgentBrowserProviderError::ModelTerminal(failure.class()),
                        }
                    }
                },
                None => AgentBrowserProviderError::Proposal,
            })
        }
        AgentProviderPolicySettlement::PricingRequired(settlement) => {
            let terminal = match model {
                AgentBrowserModel::Terra => {
                    match settle_terra_provider_terminal(*settlement, policy).map_err(|error| {
                        *retained = error.into_retained().map(BrowserUnsettledTerminal::Terra);
                        AgentBrowserProviderError::Settlement
                    })? {
                        TerraProviderTerminalSettlement::Priced(terminal) => terminal,
                        TerraProviderTerminalSettlement::ReservationCeiling(receipt) => {
                            receipts.push((*receipt, input));
                            return Err(AgentBrowserProviderError::Settlement);
                        }
                    }
                }
                AgentBrowserModel::Luna => {
                    match settle_luna_provider_terminal(*settlement, policy).map_err(|error| {
                        *retained = error.into_retained().map(BrowserUnsettledTerminal::Luna);
                        AgentBrowserProviderError::Settlement
                    })? {
                        LunaProviderTerminalSettlement::Priced(terminal) => terminal,
                        LunaProviderTerminalSettlement::ReservationCeiling(receipt) => {
                            receipts.push((*receipt, input));
                            return Err(AgentBrowserProviderError::Settlement);
                        }
                    }
                }
            };
            let receipt = terminal.receipt();
            receipts.push((receipt, input));
            Ok(*terminal)
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
) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
    let (session, turn) =
        AgentBrowserSession::start(input, transport_config, credential, observation).await?;
    session.finish()?;
    Ok(turn)
}

/// One settled, exactly-priced tool proposal and content-free receipts.
#[must_use]
pub struct AgentBrowserProviderTurn {
    receipt: AgentModelCallReceipt,
    input: zephium_agentic::AgentProviderInputMetricReceipt,
    turn: zephium_agentic::AgentProviderSettledToolTurn,
    provider_elapsed: Duration,
}

impl AgentBrowserProviderTurn {
    /// Trusted local elapsed time for exact counting and model generation.
    pub const fn provider_elapsed(&self) -> Duration {
        self.provider_elapsed
    }
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

impl fmt::Debug for AgentBrowserProviderTurn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentBrowserProviderTurn")
            .field("receipt", &self.receipt)
            .field("input", &self.input)
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed refusal of a trusted account sample; no account data is retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentBrowserAccountError {
    /// An original provider/effect reservation still owns its frozen sample.
    Pending,
    /// A sample attempted to replace the exact document/context authority.
    ContextChanged,
    /// A sample attempted to switch the admitted account, even within scope.
    AccountChanged,
    /// The sample predates its predecessor or is ahead of trusted policy time.
    Clock,
    /// The account sample exceeded the existing policy freshness ceiling.
    Stale,
    /// An existing attestation identity was reused with different facts.
    Rewritten,
    /// A prior sample identity was replayed after a newer sample was accepted.
    Replayed,
    /// The fixed model/effect-derived sample inventory was exhausted.
    Limit,
}

/// Closed content-free session refusal. No variant authorizes a blind retry.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentBrowserProviderError {
    /// Exact task-authorized native navigation policy refused its checkpoint.
    #[error("browser document navigation was refused")]
    Navigation(zephium_agentic::AgentPolicyError),
    /// The trusted host could not maintain exact fresh account authority.
    #[error("browser account re-attestation was refused")]
    Account(AgentBrowserAccountError),
    /// A bounded read exceeded its encoding/admission ceiling.
    #[error("browser semantic read encoding was refused")]
    ReadEncoding(zephium_agentic::SemanticModelEncodingError),
    /// Fresh bounded read did not join its exact source authority.
    #[error("browser semantic read authority was refused")]
    Read(zephium_agentic::SemanticReadError),
    /// Purpose-bound output or its exact delivered sources were refused.
    #[error("browser extraction output was refused")]
    Extraction(zephium_agentic::AgentProviderExtractionOutputError),
    /// The authorized read has no sources for a required extraction field.
    #[error("browser extraction has no source evidence for its required fields")]
    NoExtractionEvidence,
    /// Product projection or exact durable accounting refused a transition.
    #[error("browser session journal refused a transition")]
    Journal,
    /// Caller revoked this session; resuming requires a new observation and run.
    #[error("browser session was cancelled")]
    Cancelled,
    /// One native action or its recovery owner remains live.
    #[error("browser session retains native action debt")]
    ActionPending,
    /// The independent native-effect ceiling was reached.
    #[error("browser session action ceiling was exhausted")]
    ActionLimit,
    /// The exact action policy or native pipeline refused execution.
    #[error("browser action failed")]
    Action(crate::AgentBrowserActionError),
    /// This driver has no adapter for the proposed bounded tool.
    #[error("browser tool is not supported by this driver")]
    UnsupportedTool(zephium_agentic::AgentBrowserToolKind),
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
    /// The provider locate proposal was not compatible with the fixed matcher.
    #[error("Terra probe locate tool contract failed")]
    LocateTool,
    /// The fixed semantic matcher refused the acknowledged observation.
    #[error("Terra probe semantic locate failed")]
    Locate,
    /// The bounded semantic locate result could not be encoded.
    #[error("Terra probe locate result encoding failed")]
    LocateEncoding(SemanticModelEncodingError),
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
    use super::*;
    use super::{
        TerraControllerConstructionError, TerraControllerIds, TERRA_CONTROLLER_MAX_OUTPUT_TOKENS,
        TERRA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD,
    };
    use zephium_agent_model_catalog::{
        TERRA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS, TERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS,
        TERRA_STANDARD_RATE_MAX_INPUT_TOKENS,
    };
    use zephium_agentic::*;
    use zephium_agentic::{
        AgentAuditDeliveryId, AgentAuditEventId, AgentModelCallId, AgentSupervisorAttemptId,
        AgentSupervisorCancellationId, AgentSupervisorId,
    };

    fn event(value: u64) -> AgentAuditEventId {
        AgentAuditEventId::new(value).expect("test event id")
    }

    struct FixedBrowserClock(u64);
    impl TerraControllerClock for FixedBrowserClock {
        fn now(&self) -> Result<AgentPolicyInstant, TerraControllerClockError> {
            Ok(AgentPolicyInstant::from_millis(self.0))
        }
    }

    fn browser_fixture() -> (AgentBrowserSession, SemanticObservation) {
        browser_fixture_with_budget(AgentRunBudget::try_new(16, 300_000, 1_000_000, 1).unwrap())
    }

    fn browser_fixture_with_budget(
        budget: AgentRunBudget,
    ) -> (AgentBrowserSession, SemanticObservation) {
        browser_fixture_with_limits(budget, MAX_BROWSER_MODEL_TURNS, MAX_BROWSER_ACTIONS)
    }

    fn browser_fixture_with_limits(
        budget: AgentRunBudget,
        max_model_calls: u8,
        max_actions: u64,
    ) -> (AgentBrowserSession, SemanticObservation) {
        let profile = 13_u128.into();
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            profile,
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
        let operation = registry
            .begin_context(identity.id(), ContextOperationId::new(1).expect("id"))
            .expect("construct");
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .expect("settle");
        let context = registry.join(identity.id()).expect("context");
        let origin = SemanticOrigin::parse("https://fixture.example.test/").expect("origin");
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            origin.clone(),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(1).expect("invocation"),
                frame.clone(),
                SemanticSnapshotGeneration::INITIAL,
            ),
            br#"{"v":1,"i":1,"g":1,"c":"complete","n":[{"k":1,"r":"document","o":16}]}"#,
        )
        .expect("snapshot");
        let observation = SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(1).expect("observation"),
                context,
                SemanticObservationBudget::INITIAL_FILTERED,
            ),
            snapshot,
        )
        .expect("assembler")
        .finish()
        .expect("observation");
        let effects = AgentEffectScope::try_new(&[
            SemanticEffectClass::Read,
            SemanticEffectClass::LocalWrite,
        ])
        .expect("effects");
        let account_scope = AgentAccountScope::Anonymous;
        let scope = AgentRunScope::try_new(
            vec![profile],
            vec![account_scope],
            vec![origin.clone()],
            SemanticSensitivity::Sensitive,
            effects,
            vec![],
        )
        .expect("scope");
        let node = AgentPlanNodeId::generate();
        let authority = AgentPlanNodeAuthority::try_new(
            vec![profile],
            vec![account_scope],
            vec![origin],
            SemanticSensitivity::Sensitive,
            effects,
        )
        .expect("authority");
        let manifest = AgentRunManifest::try_new(
            AgentRunManifestId::generate(),
            identity.owner(),
            scope,
            budget,
            AgentPolicyInstant::from_millis(1000),
            AgentPolicyInstant::from_millis(200_000),
            vec![AgentPlanNodeScope::new(
                node,
                authority,
                budget,
                AgentPolicyInstant::from_millis(199_999),
            )],
        )
        .expect("manifest");
        let lease = AgentPlanLeaseBinding::new(AgentPlanLeaseId::generate(), node);
        let account = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            account_scope,
            AgentPolicyInstant::from_millis(1000),
        );
        let ids = TerraControllerIds::try_new(
            AgentSupervisorId::new(1).expect("id"),
            AgentSupervisorAttemptId::new(1).expect("id"),
            AgentSupervisorCancellationId::new(1).expect("id"),
            AgentModelCallId::new(1).expect("id"),
            [event(1), event(2), event(3), event(4)],
            AgentAuditDeliveryId::new(1).expect("id"),
        )
        .expect("ids");
        let turn = TerraControllerTurnInput::try_new(
            account,
            observation.request().clone(),
            frame,
            SemanticInvocationId::new(1).expect("id"),
            SemanticSnapshotGeneration::INITIAL,
            "Prepare a fixture field".to_owned(),
        )
        .expect("turn");
        let mut input = TerraControllerRunInput::try_new_for_model(
            manifest,
            lease,
            turn,
            ids,
            Arc::new(FixedBrowserClock(1001)),
            Instant::now() + Duration::from_secs(60),
            AgentBrowserModel::Luna,
        )
        .expect("input");
        input.max_model_calls = max_model_calls;
        input.max_actions = max_actions;
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-not-a-credential".to_owned(),
        )
        .expect("fixture credential");
        let session = AgentBrowserSession::try_new(
            input,
            AgentProviderTransportConfig::STANDARD,
            credential,
            AgentBrowserModel::Luna,
            AgentBrowserRetention::Stateless,
        )
        .expect("session");
        (session, observation)
    }

    fn reserved_browser_input(
        session: &mut AgentBrowserSession,
        observation: &SemanticObservation,
    ) -> AgentProviderTransportInput {
        let request = session.next_model_call_request().expect("request");
        let payload = encode_semantic_observation(
            observation,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .expect("encoding")
        .admit_conservative_utf8(session.config.tokenizer())
        .expect("admission");
        AgentPreparedObservationRequest::try_openai_for_provider_exact_count(
            &mut session.policy,
            request,
            observation,
            payload,
            session.objective.as_ref().expect("objective"),
            session.config.clone(),
        )
        .expect("prepare")
        .into_transport_input()
    }

    #[test]
    fn luna_initial_reservation_requires_sufficient_frozen_run_and_node_budget() {
        use zephium_agentic::{AgentPolicyError, AgentProviderRequestError};
        for cost in [
            50_000,
            LUNA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD - 1,
            LUNA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD,
            100_000,
        ] {
            let budget = AgentRunBudget::try_new(8, 100_000, cost, 1).unwrap();
            let (mut session, observation) = browser_fixture_with_budget(budget);
            let request = session.next_model_call_request().unwrap();
            let payload = encode_semantic_observation(
                &observation,
                SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
            )
            .unwrap()
            .admit_conservative_utf8(session.config.tokenizer())
            .unwrap();
            let prepared = AgentPreparedObservationRequest::try_openai_for_provider_exact_count(
                &mut session.policy,
                request,
                &observation,
                payload,
                session.objective.as_ref().unwrap(),
                session.config.clone(),
            );
            if cost < LUNA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD {
                assert!(matches!(
                    prepared,
                    Err(AgentProviderRequestError::Policy(AgentPolicyError::Budget))
                ));
                assert_eq!(session.policy.pending_model_calls(), 0);
            } else {
                let prepared = prepared.unwrap();
                assert_eq!(session.policy.pending_model_calls(), 1);
                assert_eq!(
                    session.policy.accounting().reserved_cost_micro_usd(),
                    LUNA_PROVIDER_EXACT_RESERVATION_COST_MICRO_USD
                );
                let _outcome = prepared
                    .into_transport_input()
                    .cancel(&mut session.policy)
                    .unwrap();
                assert_eq!(session.policy.pending_model_calls(), 0);
            }
            // Preparation never dispatches counting or model generation.
            assert!(session.try_finish_unsuccessful().is_ok());
        }
    }

    #[test]
    fn account_refresh_preserves_exact_identity_time_replay_and_bounded_storage() {
        use zephium_agentic::{AgentAccountAttestationId, AgentAccountId, AgentAccountScope};
        for (case, expected) in [
            (0, AgentBrowserAccountError::Stale),
            (1, AgentBrowserAccountError::Clock),
            (2, AgentBrowserAccountError::Clock),
            (3, AgentBrowserAccountError::AccountChanged),
            (4, AgentBrowserAccountError::ContextChanged),
            (5, AgentBrowserAccountError::Rewritten),
            (6, AgentBrowserAccountError::Replayed),
            (7, AgentBrowserAccountError::Limit),
        ] {
            let (mut session, _) = browser_fixture();
            let original = session.account;
            session.clock = Arc::new(FixedBrowserClock(32_000));
            let mut account = AgentContextAccountBinding::new(
                AgentAccountAttestationId::generate(),
                original.context(),
                original.account(),
                AgentPolicyInstant::from_millis(32_000),
            );
            match case {
                0 => account = original,
                1 => {
                    account = AgentContextAccountBinding::new(
                        account.attestation(),
                        account.context(),
                        account.account(),
                        AgentPolicyInstant::from_millis(32_001),
                    )
                }
                2 => {
                    account = AgentContextAccountBinding::new(
                        account.attestation(),
                        account.context(),
                        account.account(),
                        AgentPolicyInstant::from_millis(999),
                    )
                }
                3 => {
                    account = AgentContextAccountBinding::new(
                        account.attestation(),
                        account.context(),
                        AgentAccountScope::Authenticated(AgentAccountId::generate()),
                        account.observed_at(),
                    )
                }
                4 => {
                    let (other, _) = browser_fixture();
                    account = AgentContextAccountBinding::new(
                        account.attestation(),
                        other.account.context(),
                        account.account(),
                        account.observed_at(),
                    );
                }
                5 => {
                    account = AgentContextAccountBinding::new(
                        original.attestation(),
                        account.context(),
                        account.account(),
                        account.observed_at(),
                    )
                }
                6 => {
                    session.refresh_account(account).unwrap();
                    account = AgentContextAccountBinding::new(
                        original.attestation(),
                        original.context(),
                        original.account(),
                        account.observed_at(),
                    );
                }
                7 => {
                    for _ in 1..session.max_account_attestations {
                        session
                            .refresh_account(AgentContextAccountBinding::new(
                                AgentAccountAttestationId::generate(),
                                original.context(),
                                original.account(),
                                account.observed_at(),
                            ))
                            .unwrap();
                    }
                    let current = session.account;
                    session.refresh_account(current).unwrap();
                    assert_eq!(
                        session.account_attestations.len(),
                        session.max_account_attestations
                    );
                }
                _ => unreachable!(),
            }
            let retained = session.account;
            assert_eq!(
                session.refresh_account(account),
                Err(AgentBrowserProviderError::Account(expected)),
                "case {case}"
            );
            assert_eq!(session.account, retained);
            assert_eq!(
                session.refresh_account(retained),
                Err(AgentBrowserProviderError::Account(expected)),
                "refusal cannot be retried"
            );
            assert_eq!(session.policy.pending_model_calls(), 0);
            assert_eq!(session.next_action, 1);
            assert!(session.try_finish_unsuccessful().is_ok());
        }
    }

    #[test]
    fn extended_session_retains_old_account_replay_protection_without_renewing_authority() {
        let budget = AgentRunBudget::try_new(80, 300_000, 1_000_000, 1).unwrap();
        let (mut session, _) = browser_fixture_with_limits(budget, 24, 12);
        let original = session.account;
        session.clock = Arc::new(FixedBrowserClock(1001));
        for _ in 0..32 {
            session
                .refresh_account(AgentContextAccountBinding::new(
                    AgentAccountAttestationId::generate(),
                    original.context(),
                    original.account(),
                    original.observed_at(),
                ))
                .unwrap();
        }
        assert_eq!(session.policy.accounting().reserved_operations(), 0);
        assert_eq!(
            session
                .policy
                .remaining_operations(session.lease.lease())
                .unwrap(),
            80
        );
        assert_eq!(
            session.refresh_account(original),
            Err(AgentBrowserProviderError::Account(
                AgentBrowserAccountError::Replayed
            ))
        );
        assert!(session.try_finish_unsuccessful().is_ok());
    }

    #[test]
    fn account_refresh_does_not_rebind_or_discard_an_original_provider_reservation() {
        let (mut session, observation) = browser_fixture();
        let input = reserved_browser_input(&mut session, &observation);
        let original = session.account;
        assert_eq!(
            session.refresh_account(original),
            Err(AgentBrowserProviderError::Account(
                AgentBrowserAccountError::Pending
            ))
        );
        assert_eq!(session.account, original);
        assert_eq!(session.policy.pending_model_calls(), 1);
        let refusal = session.try_finish_unsuccessful().unwrap_err();
        let mut session = refusal.into_session();
        assert_eq!(session.policy.pending_model_calls(), 1);
        let _outcome = input.cancel(&mut session.policy).unwrap();
        assert!(session.try_finish_unsuccessful().is_ok());
    }

    #[test]
    fn startup_account_expiry_remains_enforced_without_a_new_trusted_sample() {
        let (mut session, observation) = browser_fixture();
        session.clock = Arc::new(FixedBrowserClock(32_000));
        let request = session.next_model_call_request().unwrap();
        let payload = encode_semantic_observation(
            &observation,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .unwrap()
        .admit_conservative_utf8(session.config.tokenizer())
        .unwrap();
        let result = AgentPreparedObservationRequest::try_openai_for_provider_exact_count(
            &mut session.policy,
            request,
            &observation,
            payload,
            session.objective.as_ref().unwrap(),
            session.config.clone(),
        );
        assert!(matches!(
            result,
            Err(zephium_agentic::AgentProviderRequestError::Policy(
                zephium_agentic::AgentPolicyError::AccountStale
            ))
        ));
        assert_eq!(session.policy.pending_model_calls(), 0);
    }

    #[test]
    fn browser_session_constructor_is_undispatched_and_shutdown_is_content_free() {
        let (session, _) = browser_fixture();
        assert_eq!(session.policy.pending_model_calls(), 0);
        assert!(!format!("{session:?}").contains("Prepare a fixture"));
        let terminal = session.try_finish().expect("drained");
        assert!(terminal.session.credential.is_none());
        assert_eq!(terminal.accounting().reserved_model_tokens(), 0);
    }

    #[test]
    fn browser_pre_dispatch_stops_release_the_exact_model_reservation() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");
        for case in 0..4 {
            let (mut session, observation) = browser_fixture();
            let input = reserved_browser_input(&mut session, &observation);
            assert_eq!(session.policy.pending_model_calls(), 1);
            assert!(session.policy.accounting().reserved_model_tokens() > 0);
            let expected = match case {
                0 => {
                    session.cancel();
                    AgentBrowserProviderError::Cancelled
                }
                1 => {
                    session.turns = MAX_BROWSER_MODEL_TURNS;
                    AgentBrowserProviderError::TurnLimit
                }
                2 => {
                    session.deadline = Instant::now();
                    AgentBrowserProviderError::Deadline
                }
                _ => {
                    session.credential.take();
                    AgentBrowserProviderError::Cancelled
                }
            };
            assert_eq!(
                runtime.block_on(session.drive(input)).expect_err("stop"),
                expected
            );
            assert_eq!(session.policy.pending_model_calls(), 0);
            assert_eq!(session.policy.accounting().reserved_model_tokens(), 0);
            assert!(session.attempt.is_none());
            let _terminal = session.try_finish().expect("drained stop");
        }
    }

    #[test]
    fn browser_shutdown_preserves_unsettled_budget_and_never_claims_clean() {
        let (mut session, observation) = browser_fixture();
        let input = reserved_browser_input(&mut session, &observation);
        let refused = session.try_finish().expect_err("reservation outstanding");
        assert_eq!(refused.error(), AgentBrowserProviderError::ActionPending);
        let mut owner = refused.into_session();
        assert_eq!(owner.policy.pending_model_calls(), 1);
        assert_eq!(
            owner.check_live(),
            Err(AgentBrowserProviderError::Cancelled)
        );
        let _outcome = input
            .cancel(&mut owner.policy)
            .expect("reconcile exact input");
        let _terminal = owner.try_finish().expect("reconciled");
    }

    #[test]
    fn browser_clock_is_real_monotonic_and_ids_do_not_wrap() {
        let (mut session, _) = browser_fixture();
        session.clock = Arc::new(FixedBrowserClock(1000));
        assert_eq!(session.policy_now(), Err(AgentBrowserProviderError::Clock));
        session.clock = Arc::new(FixedBrowserClock(200_001));
        assert_eq!(session.policy_now(), Err(AgentBrowserProviderError::Clock));
        session.next_call = u64::MAX;
        assert_eq!(
            session.next_model_call_request().expect_err("overflow"),
            AgentBrowserProviderError::Authority
        );
    }

    #[test]
    fn browser_cancelled_initial_turn_never_reserves_or_dispatches() {
        let (mut session, observation) = browser_fixture();
        session.cancel();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        assert_eq!(
            runtime
                .block_on(session.start_initial(&observation))
                .expect_err("cancelled"),
            AgentBrowserProviderError::Cancelled
        );
        assert_eq!(session.turns, 0);
        assert_eq!(session.policy.pending_model_calls(), 0);
        assert!(session.objective.is_some());
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
