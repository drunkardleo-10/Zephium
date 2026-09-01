//! Mutable bounded plan-lease accounting and committed model-input taint.
//!
//! This functional core reserves one model call before transport, records
//! source taint only after exact committed observation/read delivery, and
//! settles provider usage without retries. It owns no transport, provider,
//! timer, task, thread, page, or browser. Effect permits are deliberately a
//! later layer that consumes this state; the state itself cannot execute an
//! action.

use std::fmt;
use std::num::NonZeroU64;

use sha2::{Digest, Sha256};
use thiserror::Error;
use zephium_core::ids::ProfileId;

use crate::semantic_diff::SemanticObservationFingerprint;
use crate::{
    AgentAccountScope, AgentContextAccountBinding, AgentPlanLeaseId, AgentPlanNodeId,
    AgentPolicyInstant, AgentRunBudget, AgentRunManifest, ContextJoin, SemanticEffectClass,
    SemanticModelPayload, SemanticObservation, SemanticObservationAcknowledgement, SemanticOrigin,
    SemanticReadDeliveryReceipt, SemanticReadModelPayload, SemanticReadResult, SemanticSensitivity,
    SemanticTrust,
};

/// Maximum model calls reserved or active in one run policy.
pub const MAX_AGENT_PENDING_MODEL_CALLS: usize = 4;
/// Maximum distinct profile/account/origin taint cohorts retained per run.
pub const MAX_AGENT_TAINT_COHORTS: usize = 128;
/// Maximum age of an account attestation at model-input admission (30 seconds).
pub const MAX_AGENT_ACCOUNT_ATTESTATION_AGE_MILLIS: u64 = 30_000;

/// Monotonic shell-minted identity for one exact model-input attempt.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentModelCallId(NonZeroU64);

impl AgentModelCallId {
    /// Constructs one nonzero process-local call identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Numeric value for exact trusted-shell correlation.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for AgentModelCallId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentModelCallId([redacted])")
    }
}

/// Exact mutable plan-lease identity bound to one approved node.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentPlanLeaseBinding {
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
}

impl AgentPlanLeaseBinding {
    /// Joins one shell-minted consumption lease to one approved plan node.
    pub const fn new(lease: AgentPlanLeaseId, node: AgentPlanNodeId) -> Self {
        Self { lease, node }
    }

    /// Exact mutable lease identity.
    pub const fn lease(self) -> AgentPlanLeaseId {
        self.lease
    }

    /// Exact approved plan node consumed through this lease.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }
}

impl fmt::Debug for AgentPlanLeaseBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentPlanLeaseBinding")
            .field("lease", &self.lease)
            .field("node", &self.node)
            .finish()
    }
}

/// Additional full-call reservation beyond one measured semantic payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentModelCallBudget {
    additional_input_tokens: u32,
    output_tokens: u32,
    cost_micro_usd: u64,
}

impl AgentModelCallBudget {
    /// Bounds trusted envelope/input, output, and provider cost reservation.
    pub const fn try_new(
        additional_input_tokens: u32,
        output_tokens: u32,
        cost_micro_usd: u64,
    ) -> Result<Self, AgentPolicyError> {
        if additional_input_tokens as u64 > crate::MAX_AGENT_RUN_MODEL_TOKENS
            || output_tokens as u64 > crate::MAX_AGENT_RUN_MODEL_TOKENS
            || cost_micro_usd > crate::MAX_AGENT_RUN_COST_MICRO_USD
        {
            return Err(AgentPolicyError::Budget);
        }
        Ok(Self {
            additional_input_tokens,
            output_tokens,
            cost_micro_usd,
        })
    }

    /// Non-semantic prompt/envelope input-token ceiling.
    pub const fn additional_input_tokens(self) -> u32 {
        self.additional_input_tokens
    }

    /// Provider output-token ceiling.
    pub const fn output_tokens(self) -> u32 {
        self.output_tokens
    }

    /// Provider call ceiling in millionths of a US dollar.
    pub const fn cost_micro_usd(self) -> u64 {
        self.cost_micro_usd
    }
}

/// Trusted-shell intent to reserve one exact model call against one plan lease.
///
/// This value grants no authority. `AgentRunPolicy` revalidates every field
/// against the exact payload provenance, manifest revision, mutable budget,
/// account attestation, and monotonic time before minting an admission.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentModelCallRequest {
    id: AgentModelCallId,
    lease: AgentPlanLeaseId,
    account: AgentContextAccountBinding,
    budget: AgentModelCallBudget,
    now: AgentPolicyInstant,
}

impl AgentModelCallRequest {
    /// Captures the complete trusted-shell reservation request.
    pub const fn new(
        id: AgentModelCallId,
        lease: AgentPlanLeaseId,
        account: AgentContextAccountBinding,
        budget: AgentModelCallBudget,
        now: AgentPolicyInstant,
    ) -> Self {
        Self {
            id,
            lease,
            account,
            budget,
            now,
        }
    }

    /// Monotonic exact call identity.
    pub const fn id(self) -> AgentModelCallId {
        self.id
    }

    /// Mutable plan lease that must reserve this call.
    pub const fn lease(self) -> AgentPlanLeaseId {
        self.lease
    }

    /// Exact current context/account attestation.
    pub const fn account(self) -> AgentContextAccountBinding {
        self.account
    }

    /// Complete input/output/cost reservation beyond measured semantic input.
    pub const fn budget(self) -> AgentModelCallBudget {
        self.budget
    }

    /// Trusted monotonic admission time.
    pub const fn now(self) -> AgentPolicyInstant {
        self.now
    }
}

impl fmt::Debug for AgentModelCallRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentModelCallRequest")
            .field("id", &self.id)
            .field("lease", &self.lease)
            .field("account", &self.account)
            .field("budget", &self.budget)
            .field("now", &self.now)
            .finish()
    }
}

/// Content-free taint retained for everything committed to the model context.
#[derive(Clone, Eq, PartialEq)]
pub struct AgentTaintCohort {
    context: ContextJoin,
    account: AgentAccountScope,
    origin: SemanticOrigin,
    sensitivity: SemanticSensitivity,
    trust: SemanticTrust,
    attested_at: AgentPolicyInstant,
}

impl AgentTaintCohort {
    /// Exact source context/document/cancellation authority delivered to the model.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Exact browser profile from source context authority.
    pub const fn profile(&self) -> ProfileId {
        self.context.identity().profile()
    }

    /// Exact source account attested at input admission.
    pub const fn account(&self) -> AgentAccountScope {
        self.account
    }

    /// Canonical native-attested source origin.
    pub const fn origin(&self) -> &SemanticOrigin {
        &self.origin
    }

    /// Maximum non-secret sensitivity actually made visible in this cohort.
    pub const fn sensitivity(&self) -> SemanticSensitivity {
        self.sensitivity
    }

    /// Worst source trust made visible in this cohort.
    pub const fn trust(&self) -> SemanticTrust {
        self.trust
    }

    /// Oldest trusted account-attestation time merged into this source cohort.
    pub const fn attested_at(&self) -> AgentPolicyInstant {
        self.attested_at
    }

    fn same_source(&self, other: &Self) -> bool {
        self.context == other.context
            && self.account == other.account
            && self.origin == other.origin
    }

    fn merge(&mut self, other: &Self) {
        debug_assert!(self.same_source(other));
        self.sensitivity = self.sensitivity.max(other.sensitivity);
        if other.trust == SemanticTrust::UntrustedPage {
            self.trust = SemanticTrust::UntrustedPage;
        }
        self.attested_at = self.attested_at.min(other.attested_at);
    }
}

impl fmt::Debug for AgentTaintCohort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentTaintCohort")
            .field("context", &"[redacted]")
            .field("account", &"[redacted]")
            .field("origin", &"[redacted]")
            .field("sensitivity", &self.sensitivity)
            .field("trust", &self.trust)
            .field("attested_at", &self.attested_at)
            .finish()
    }
}

/// Consumed and currently reserved accounting for a run or node lease.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentPolicyAccounting {
    consumed_operations: u32,
    reserved_operations: u32,
    consumed_model_tokens: u64,
    reserved_model_tokens: u64,
    consumed_cost_micro_usd: u64,
    reserved_cost_micro_usd: u64,
}

impl AgentPolicyAccounting {
    /// Effect/model/tool operations terminally consumed.
    pub const fn consumed_operations(self) -> u32 {
        self.consumed_operations
    }

    /// Operations held by admitted in-flight calls.
    pub const fn reserved_operations(self) -> u32 {
        self.reserved_operations
    }

    /// Terminal provider tokens charged to this scope.
    pub const fn consumed_model_tokens(self) -> u64 {
        self.consumed_model_tokens
    }

    /// Maximum provider tokens held by admitted in-flight calls.
    pub const fn reserved_model_tokens(self) -> u64 {
        self.reserved_model_tokens
    }

    /// Terminal provider/tool cost in micro-USD.
    pub const fn consumed_cost_micro_usd(self) -> u64 {
        self.consumed_cost_micro_usd
    }

    /// Provider/tool cost held by admitted in-flight calls.
    pub const fn reserved_cost_micro_usd(self) -> u64 {
        self.reserved_cost_micro_usd
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModelInputKind {
    Observation,
    Read,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModelCallState {
    Prepared,
    Delivered,
}

struct ModelCallRow {
    id: AgentModelCallId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    kind: ModelInputKind,
    source_guard: [u8; 32],
    admission_guard: [u8; 32],
    input_token_limit: u64,
    output_token_limit: u64,
    cost_limit: u64,
    candidates: Vec<AgentTaintCohort>,
    state: ModelCallState,
}

#[derive(Clone, Copy, Default)]
struct ConsumedUsage {
    operations: u32,
    model_tokens: u64,
    cost_micro_usd: u64,
}

struct PlanLeaseState {
    binding: AgentPlanLeaseBinding,
    consumed: ConsumedUsage,
}

struct AdmissionGuardFacts<'a> {
    manifest_guard: [u8; 32],
    id: AgentModelCallId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    kind: ModelInputKind,
    context: ContextJoin,
    source_guard: [u8; 32],
    input_token_limit: u64,
    output_token_limit: u64,
    cost_limit: u64,
    candidates: &'a [AgentTaintCohort],
}

/// Non-cloneable pre-transport reservation for one exact model input.
#[must_use]
pub struct AgentModelCallAdmission {
    id: AgentModelCallId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    kind: ModelInputKind,
    guard: [u8; 32],
}

impl AgentModelCallAdmission {
    /// Exact model-call identity.
    pub const fn id(&self) -> AgentModelCallId {
        self.id
    }

    /// Exact mutable plan lease reserving this call.
    pub const fn lease(&self) -> AgentPlanLeaseId {
        self.lease
    }

    /// Exact approved plan node reserving this call.
    pub const fn node(&self) -> AgentPlanNodeId {
        self.node
    }
}

impl fmt::Debug for AgentModelCallAdmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentModelCallAdmission")
            .field("id", &self.id)
            .field("lease", &self.lease)
            .field("node", &self.node)
            .field("kind", &self.kind)
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Non-cloneable exact call whose model input committed and taint is retained.
#[must_use]
pub struct AgentActiveModelCall {
    id: AgentModelCallId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    guard: [u8; 32],
}

impl AgentActiveModelCall {
    /// Exact committed model-call identity.
    pub const fn id(&self) -> AgentModelCallId {
        self.id
    }

    /// Exact plan lease holding token/cost reservation.
    pub const fn lease(&self) -> AgentPlanLeaseId {
        self.lease
    }

    /// Exact approved plan node.
    pub const fn node(&self) -> AgentPlanNodeId {
        self.node
    }
}

impl fmt::Debug for AgentActiveModelCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentActiveModelCall")
            .field("id", &self.id)
            .field("lease", &self.lease)
            .field("node", &self.node)
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Pre-delivery terminal outcome that releases the complete reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentModelInputCancellation {
    /// Selected transport refused before committing the input.
    Refused,
    /// Exact run cancellation won before input commitment.
    Cancelled,
}

/// Terminal provider-call outcome; none authorizes a retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentModelCallSettlement {
    /// Provider returned one terminal response.
    Completed,
    /// Provider failed after input commitment.
    ProviderFailed,
    /// Cancellation won after input commitment.
    Cancelled,
}

/// Content-free terminal model-call accounting receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentModelCallReceipt {
    id: AgentModelCallId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    settlement: AgentModelCallSettlement,
    input_tokens: u64,
    output_tokens: u64,
    cost_micro_usd: u64,
}

impl AgentModelCallReceipt {
    /// Exact model-call identity.
    pub const fn id(self) -> AgentModelCallId {
        self.id
    }

    /// Exact consumed plan lease.
    pub const fn lease(self) -> AgentPlanLeaseId {
        self.lease
    }

    /// Exact approved plan node.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }

    /// Completed, provider-failed, or cancelled terminal class.
    pub const fn settlement(self) -> AgentModelCallSettlement {
        self.settlement
    }

    /// Actual provider-accounted input tokens.
    pub const fn input_tokens(self) -> u64 {
        self.input_tokens
    }

    /// Actual provider-accounted output tokens.
    pub const fn output_tokens(self) -> u64 {
        self.output_tokens
    }

    /// Actual provider-accounted cost in micro-USD.
    pub const fn cost_micro_usd(self) -> u64 {
        self.cost_micro_usd
    }
}

/// Mutable single-owner policy state for one exact manifest revision.
#[must_use]
pub struct AgentRunPolicy {
    manifest: AgentRunManifest,
    leases: Vec<PlanLeaseState>,
    consumed: ConsumedUsage,
    taints: Vec<AgentTaintCohort>,
    calls: Vec<ModelCallRow>,
    last_call: Option<AgentModelCallId>,
    sealed: bool,
}

impl AgentRunPolicy {
    /// Installs exactly one unique mutable lease for every approved plan node.
    pub fn try_new(
        manifest: AgentRunManifest,
        bindings: Vec<AgentPlanLeaseBinding>,
    ) -> Result<Self, AgentPolicyError> {
        if bindings.len() != manifest.plan_nodes().len() {
            return Err(AgentPolicyError::LeaseSet);
        }
        let mut bindings = bindings;
        bindings.sort_by_key(|binding| binding.node());
        let duplicate_node = bindings
            .windows(2)
            .any(|pair| pair[0].node() == pair[1].node());
        let duplicate_lease = bindings.iter().enumerate().any(|(index, binding)| {
            bindings[..index]
                .iter()
                .any(|prior| prior.lease() == binding.lease())
        });
        if duplicate_node || duplicate_lease {
            return Err(AgentPolicyError::LeaseSet);
        }
        if bindings
            .iter()
            .zip(manifest.plan_nodes())
            .any(|(binding, node)| binding.node() != node.id())
        {
            return Err(AgentPolicyError::LeaseSet);
        }
        Ok(Self {
            manifest,
            leases: bindings
                .into_iter()
                .map(|binding| PlanLeaseState {
                    binding,
                    consumed: ConsumedUsage::default(),
                })
                .collect(),
            consumed: ConsumedUsage::default(),
            taints: Vec::new(),
            calls: Vec::with_capacity(MAX_AGENT_PENDING_MODEL_CALLS),
            last_call: None,
            sealed: false,
        })
    }

    /// Immutable approved manifest facts.
    pub const fn manifest(&self) -> &AgentRunManifest {
        &self.manifest
    }

    /// Persistent committed-model-input taint for all later effect checks.
    pub fn taints(&self) -> &[AgentTaintCohort] {
        &self.taints
    }

    /// Calls currently reserved before or after input commitment.
    pub fn pending_model_calls(&self) -> usize {
        self.calls.len()
    }

    /// Whether a mismatched/ambiguous settlement terminally sealed this policy.
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// Consumed and reserved run-wide accounting.
    pub fn accounting(&self) -> AgentPolicyAccounting {
        accounting(self.consumed, self.calls.iter())
    }

    /// Consumed and reserved accounting for one exact plan lease.
    pub fn lease_accounting(&self, lease: AgentPlanLeaseId) -> Option<AgentPolicyAccounting> {
        let state = self
            .leases
            .iter()
            .find(|state| state.binding.lease() == lease)?;
        Some(accounting(
            state.consumed,
            self.calls.iter().filter(|call| call.lease == lease),
        ))
    }

    /// Reserves exact observation input before any model transport receives bytes.
    pub fn prepare_observation_input(
        &mut self,
        request: AgentModelCallRequest,
        observation: &SemanticObservation,
        payload: &SemanticModelPayload,
    ) -> Result<AgentModelCallAdmission, AgentPolicyError> {
        if !payload.matches_observation(observation) {
            return Err(AgentPolicyError::PayloadMismatch);
        }
        let context = observation.request().context();
        let candidates = observation_taints(observation, request.account())?;
        let source_guard = SemanticObservationFingerprint::from_observation(observation).digest();
        self.prepare_model_input(
            request,
            context,
            ModelInputKind::Observation,
            source_guard,
            candidates,
            u64::from(payload.token_measurement().tokens()),
        )
    }

    /// Reserves exact bounded-read input before any model transport receives bytes.
    pub fn prepare_read_input(
        &mut self,
        request: AgentModelCallRequest,
        read: &SemanticReadResult<'_>,
        payload: &SemanticReadModelPayload,
    ) -> Result<AgentModelCallAdmission, AgentPolicyError> {
        if !payload.matches_read(read) {
            return Err(AgentPolicyError::PayloadMismatch);
        }
        let Some(first) = read.fragments().first() else {
            return Err(AgentPolicyError::EmptyRead);
        };
        let context = first.provenance().context();
        let candidates = read_taints(read, request.account())?;
        self.prepare_model_input(
            request,
            context,
            ModelInputKind::Read,
            read.guard(),
            candidates,
            u64::from(payload.token_measurement().tokens()),
        )
    }

    /// Commits exact observation taint after exact transport acknowledgement.
    pub fn commit_observation_input(
        &mut self,
        admission: AgentModelCallAdmission,
        acknowledgement: &SemanticObservationAcknowledgement,
    ) -> Result<AgentActiveModelCall, AgentPolicyError> {
        self.commit_model_input(
            admission,
            ModelInputKind::Observation,
            acknowledgement.guard(),
        )
    }

    /// Commits exact read taint after exact transport delivery receipt.
    pub fn commit_read_input(
        &mut self,
        admission: AgentModelCallAdmission,
        receipt: &SemanticReadDeliveryReceipt,
    ) -> Result<AgentActiveModelCall, AgentPolicyError> {
        self.commit_model_input(admission, ModelInputKind::Read, receipt.guard())
    }

    /// Releases a pre-delivery reservation after refusal or exact cancellation.
    pub fn cancel_prepared_input(
        &mut self,
        admission: AgentModelCallAdmission,
        _cancellation: AgentModelInputCancellation,
    ) -> Result<(), AgentPolicyError> {
        let index = self.call_index_or_seal(admission.id)?;
        if self.calls[index].state != ModelCallState::Prepared
            || !admission_matches(&admission, &self.calls[index])
        {
            self.sealed = true;
            return Err(AgentPolicyError::AdmissionMismatch);
        }
        self.calls.remove(index);
        Ok(())
    }

    /// Settles actual provider usage and releases unused reservation.
    pub fn settle_model_call(
        &mut self,
        active: AgentActiveModelCall,
        settlement: AgentModelCallSettlement,
        input_tokens: u64,
        output_tokens: u64,
        cost_micro_usd: u64,
    ) -> Result<AgentModelCallReceipt, AgentPolicyError> {
        let index = self.call_index_or_seal(active.id)?;
        let call = &self.calls[index];
        if call.state != ModelCallState::Delivered
            || call.lease != active.lease
            || call.node != active.node
            || call.admission_guard != active.guard
        {
            self.sealed = true;
            return Err(AgentPolicyError::AdmissionMismatch);
        }
        let provider_usage_exceeded = input_tokens > call.input_token_limit
            || output_tokens > call.output_token_limit
            || cost_micro_usd > call.cost_limit;
        let total_tokens = match input_tokens.checked_add(output_tokens) {
            Some(total) => total,
            None => {
                self.sealed = true;
                return Err(AgentPolicyError::Invariant);
            }
        };
        let Some(lease_index) = self.lease_index(call.lease) else {
            self.sealed = true;
            return Err(AgentPolicyError::Invariant);
        };
        let consumed = ConsumedUsage {
            operations: 1,
            model_tokens: total_tokens,
            cost_micro_usd,
        };
        let run_consumed = match add_usage(self.consumed, consumed) {
            Ok(usage) => usage,
            Err(error) => {
                self.sealed = true;
                return Err(error);
            }
        };
        let lease_consumed = match add_usage(self.leases[lease_index].consumed, consumed) {
            Ok(usage) => usage,
            Err(error) => {
                self.sealed = true;
                return Err(error);
            }
        };
        self.consumed = run_consumed;
        self.leases[lease_index].consumed = lease_consumed;
        let call = self.calls.remove(index);
        if provider_usage_exceeded {
            self.sealed = true;
            return Err(AgentPolicyError::ProviderUsageExceeded);
        }
        Ok(AgentModelCallReceipt {
            id: call.id,
            lease: call.lease,
            node: call.node,
            settlement,
            input_tokens,
            output_tokens,
            cost_micro_usd,
        })
    }

    fn prepare_model_input(
        &mut self,
        request: AgentModelCallRequest,
        context: ContextJoin,
        kind: ModelInputKind,
        source_guard: [u8; 32],
        candidates: Vec<AgentTaintCohort>,
        measured_input_tokens: u64,
    ) -> Result<AgentModelCallAdmission, AgentPolicyError> {
        let id = request.id();
        let lease = request.lease();
        let account = request.account();
        let budget = request.budget();
        if self.sealed {
            return Err(AgentPolicyError::Sealed);
        }
        if self.calls.len() >= MAX_AGENT_PENDING_MODEL_CALLS {
            return Err(AgentPolicyError::PendingCallLimit);
        }
        if self.last_call.is_some_and(|last| id <= last) {
            return Err(AgentPolicyError::CallReplay);
        }
        let lease_index = self.lease_index(lease).ok_or(AgentPolicyError::Lease)?;
        let node_id = self.leases[lease_index].binding.node();
        let node = self
            .manifest
            .plan_node(node_id)
            .ok_or(AgentPolicyError::Invariant)?;
        validate_time(&self.manifest, node.expires_at(), account, request.now())?;
        validate_context_scope(&self.manifest, node, context, account, &candidates)?;

        let input_token_limit = measured_input_tokens
            .checked_add(u64::from(budget.additional_input_tokens()))
            .ok_or(AgentPolicyError::Budget)?;
        let output_token_limit = u64::from(budget.output_tokens());
        let reserved_tokens = input_token_limit
            .checked_add(output_token_limit)
            .ok_or(AgentPolicyError::Budget)?;
        let run_accounting = self.accounting();
        ensure_budget(
            self.manifest.budget(),
            run_accounting,
            reserved_tokens,
            budget.cost_micro_usd(),
        )?;
        let lease_accounting = self
            .lease_accounting(lease)
            .ok_or(AgentPolicyError::Invariant)?;
        ensure_budget(
            node.budget(),
            lease_accounting,
            reserved_tokens,
            budget.cost_micro_usd(),
        )?;
        if projected_taint_count(&self.taints, &self.calls, &candidates) > MAX_AGENT_TAINT_COHORTS {
            return Err(AgentPolicyError::TaintLimit);
        }

        let admission_guard = admission_guard(AdmissionGuardFacts {
            manifest_guard: self.manifest.guard(),
            id,
            lease,
            node: node_id,
            kind,
            context,
            source_guard,
            input_token_limit,
            output_token_limit,
            cost_limit: budget.cost_micro_usd(),
            candidates: &candidates,
        });
        self.calls.push(ModelCallRow {
            id,
            lease,
            node: node_id,
            kind,
            source_guard,
            admission_guard,
            input_token_limit,
            output_token_limit,
            cost_limit: budget.cost_micro_usd(),
            candidates,
            state: ModelCallState::Prepared,
        });
        self.last_call = Some(id);
        Ok(AgentModelCallAdmission {
            id,
            lease,
            node: node_id,
            kind,
            guard: admission_guard,
        })
    }

    fn commit_model_input(
        &mut self,
        admission: AgentModelCallAdmission,
        kind: ModelInputKind,
        source_guard: [u8; 32],
    ) -> Result<AgentActiveModelCall, AgentPolicyError> {
        let index = self.call_index_or_seal(admission.id)?;
        if self.calls[index].state != ModelCallState::Prepared
            || self.calls[index].kind != kind
            || self.calls[index].source_guard != source_guard
            || !admission_matches(&admission, &self.calls[index])
        {
            self.sealed = true;
            return Err(AgentPolicyError::AdmissionMismatch);
        }
        let candidates = std::mem::take(&mut self.calls[index].candidates);
        for candidate in candidates {
            merge_taint(&mut self.taints, candidate);
        }
        self.calls[index].state = ModelCallState::Delivered;
        Ok(AgentActiveModelCall {
            id: admission.id,
            lease: admission.lease,
            node: admission.node,
            guard: admission.guard,
        })
    }

    fn lease_index(&self, lease: AgentPlanLeaseId) -> Option<usize> {
        self.leases
            .iter()
            .position(|state| state.binding.lease() == lease)
    }

    fn call_index_or_seal(&mut self, id: AgentModelCallId) -> Result<usize, AgentPolicyError> {
        match self.calls.iter().position(|call| call.id == id) {
            Some(index) => Ok(index),
            None => {
                self.sealed = true;
                Err(AgentPolicyError::CallMissing)
            }
        }
    }
}

impl fmt::Debug for AgentRunPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunPolicy")
            .field("manifest", &self.manifest.id())
            .field("leases", &self.leases.len())
            .field("accounting", &self.accounting())
            .field("taints", &self.taints.len())
            .field("pending_model_calls", &self.calls.len())
            .field("sealed", &self.sealed)
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed refusal from mutable plan-lease/model-input policy.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentPolicyError {
    /// Lease set did not name every approved node exactly once.
    #[error("agent policy plan-lease set is invalid")]
    LeaseSet,
    /// Exact plan lease was absent.
    #[error("agent policy plan lease is missing")]
    Lease,
    /// Policy was terminally sealed by an ambiguous/mismatched result.
    #[error("agent policy is sealed")]
    Sealed,
    /// Model-call identity was reused or regressed.
    #[error("agent policy model-call identity replay")]
    CallReplay,
    /// Pending model-call ceiling was reached.
    #[error("agent policy pending model-call ceiling reached")]
    PendingCallLimit,
    /// Model call was not pending in this exact policy.
    #[error("agent policy model call is missing")]
    CallMissing,
    /// Manifest or plan node was not currently live.
    #[error("agent policy approval is expired")]
    Expired,
    /// Context/run/profile/account authority did not exactly join.
    #[error("agent policy source authority mismatch")]
    Authority,
    /// Account observation was older than its hard freshness bound.
    #[error("agent policy account attestation is stale")]
    AccountStale,
    /// Source origin/account/profile was outside approved scope.
    #[error("agent policy model input is outside approved scope")]
    SourceOutsideScope,
    /// Semantic read effect was absent from run or plan-node authority.
    #[error("agent policy model input read effect is outside approved scope")]
    EffectOutsideScope,
    /// Model-visible input exceeded approved sensitivity.
    #[error("agent policy model input exceeds sensitivity scope")]
    Sensitivity,
    /// Payload was not encoded from the exact supplied source projection.
    #[error("agent policy model payload mismatched its source")]
    PayloadMismatch,
    /// Empty read could not prove source context authority.
    #[error("agent policy empty read has no source authority")]
    EmptyRead,
    /// Projected persistent model-context taint exceeded its hard ceiling.
    #[error("agent policy taint cohort ceiling reached")]
    TaintLimit,
    /// Run or node operation/token/cost budget could not reserve the call.
    #[error("agent policy budget is exhausted")]
    Budget,
    /// Admission/active token, source receipt, or call state mismatched.
    #[error("agent policy model-call admission mismatch")]
    AdmissionMismatch,
    /// Provider reported usage above its pre-transport reservation.
    #[error("agent policy provider usage exceeded reservation")]
    ProviderUsageExceeded,
    /// Monotonic policy time regressed.
    #[error("agent policy clock regressed")]
    ClockRegression,
    /// A bounded internal representation/counter invariant failed.
    #[error("agent policy internal invariant failed")]
    Invariant,
}

fn validate_time(
    manifest: &AgentRunManifest,
    node_expiry: AgentPolicyInstant,
    account: AgentContextAccountBinding,
    now: AgentPolicyInstant,
) -> Result<(), AgentPolicyError> {
    if now < manifest.issued_at() || now < account.observed_at() {
        return Err(AgentPolicyError::ClockRegression);
    }
    if now >= manifest.expires_at() || now >= node_expiry {
        return Err(AgentPolicyError::Expired);
    }
    if now.millis() - account.observed_at().millis() > MAX_AGENT_ACCOUNT_ATTESTATION_AGE_MILLIS {
        return Err(AgentPolicyError::AccountStale);
    }
    Ok(())
}

fn validate_context_scope(
    manifest: &AgentRunManifest,
    node: &crate::AgentPlanNodeScope,
    context: ContextJoin,
    account: AgentContextAccountBinding,
    candidates: &[AgentTaintCohort],
) -> Result<(), AgentPolicyError> {
    if account.context() != context || context.identity().owner() != manifest.run() {
        return Err(AgentPolicyError::Authority);
    }
    if !manifest
        .scope()
        .effects()
        .contains(SemanticEffectClass::Read)
        || !node.effects().contains(SemanticEffectClass::Read)
    {
        return Err(AgentPolicyError::EffectOutsideScope);
    }
    let profile = context.identity().profile();
    if manifest.scope().profiles().binary_search(&profile).is_err()
        || node.profiles().binary_search(&profile).is_err()
        || manifest
            .scope()
            .accounts()
            .binary_search(&account.account())
            .is_err()
        || node.accounts().binary_search(&account.account()).is_err()
    {
        return Err(AgentPolicyError::SourceOutsideScope);
    }
    for candidate in candidates {
        if candidate.context != context
            || candidate.profile() != profile
            || candidate.account != account.account()
            || manifest
                .scope()
                .origins()
                .binary_search(&candidate.origin)
                .is_err()
            || node.origins().binary_search(&candidate.origin).is_err()
        {
            return Err(AgentPolicyError::SourceOutsideScope);
        }
        if candidate.sensitivity > manifest.scope().max_sensitivity()
            || candidate.sensitivity > node.max_sensitivity()
            || candidate.sensitivity == SemanticSensitivity::Secret
        {
            return Err(AgentPolicyError::Sensitivity);
        }
    }
    Ok(())
}

fn observation_taints(
    observation: &SemanticObservation,
    account: AgentContextAccountBinding,
) -> Result<Vec<AgentTaintCohort>, AgentPolicyError> {
    let context = observation.request().context();
    if account.context() != context {
        return Err(AgentPolicyError::Authority);
    }
    let mut cohorts = Vec::with_capacity(observation.frames().len());
    for frame in observation.frames() {
        if frame.frame().context() != context {
            return Err(AgentPolicyError::Authority);
        }
        let sensitivity = frame
            .nodes()
            .iter()
            .map(|node| match node.sensitivity() {
                // Secret values are mechanically absent/redacted. Remaining
                // page labels/metadata are conservatively private taint.
                SemanticSensitivity::Secret => SemanticSensitivity::Sensitive,
                other => other,
            })
            .max()
            .unwrap_or(SemanticSensitivity::Public);
        merge_taint(
            &mut cohorts,
            AgentTaintCohort {
                context,
                account: account.account(),
                origin: frame.frame().origin().clone(),
                sensitivity,
                trust: SemanticTrust::UntrustedPage,
                attested_at: account.observed_at(),
            },
        );
    }
    Ok(cohorts)
}

fn read_taints(
    read: &SemanticReadResult<'_>,
    account: AgentContextAccountBinding,
) -> Result<Vec<AgentTaintCohort>, AgentPolicyError> {
    let mut cohorts = Vec::new();
    for fragment in read.fragments() {
        let provenance = fragment.provenance();
        if provenance.context() != account.context()
            || provenance.sensitivity() == SemanticSensitivity::Secret
        {
            return Err(AgentPolicyError::Authority);
        }
        merge_taint(
            &mut cohorts,
            AgentTaintCohort {
                context: provenance.context(),
                account: account.account(),
                origin: provenance.origin().clone(),
                sensitivity: provenance.sensitivity(),
                trust: provenance.trust(),
                attested_at: account.observed_at(),
            },
        );
    }
    Ok(cohorts)
}

fn merge_taint(cohorts: &mut Vec<AgentTaintCohort>, candidate: AgentTaintCohort) {
    if let Some(existing) = cohorts
        .iter_mut()
        .find(|existing| existing.same_source(&candidate))
    {
        existing.merge(&candidate);
    } else {
        cohorts.push(candidate);
    }
}

fn projected_taint_count(
    committed: &[AgentTaintCohort],
    calls: &[ModelCallRow],
    candidates: &[AgentTaintCohort],
) -> usize {
    let mut distinct: Vec<&AgentTaintCohort> = Vec::with_capacity(
        committed.len()
            + candidates.len()
            + calls
                .iter()
                .map(|call| call.candidates.len())
                .sum::<usize>(),
    );
    for cohort in committed
        .iter()
        .chain(calls.iter().flat_map(|call| call.candidates.iter()))
        .chain(candidates)
    {
        if !distinct.iter().any(|existing| existing.same_source(cohort)) {
            distinct.push(cohort);
        }
    }
    distinct.len()
}

fn ensure_budget(
    budget: AgentRunBudget,
    accounting: AgentPolicyAccounting,
    reserved_tokens: u64,
    reserved_cost: u64,
) -> Result<(), AgentPolicyError> {
    let operations = accounting
        .consumed_operations
        .checked_add(accounting.reserved_operations)
        .and_then(|value| value.checked_add(1))
        .ok_or(AgentPolicyError::Budget)?;
    let tokens = accounting
        .consumed_model_tokens
        .checked_add(accounting.reserved_model_tokens)
        .and_then(|value| value.checked_add(reserved_tokens))
        .ok_or(AgentPolicyError::Budget)?;
    let cost = accounting
        .consumed_cost_micro_usd
        .checked_add(accounting.reserved_cost_micro_usd)
        .and_then(|value| value.checked_add(reserved_cost))
        .ok_or(AgentPolicyError::Budget)?;
    if operations > budget.operations()
        || tokens > budget.model_tokens()
        || cost > budget.cost_micro_usd()
    {
        return Err(AgentPolicyError::Budget);
    }
    Ok(())
}

fn accounting<'a>(
    consumed: ConsumedUsage,
    calls: impl Iterator<Item = &'a ModelCallRow>,
) -> AgentPolicyAccounting {
    let mut value = AgentPolicyAccounting {
        consumed_operations: consumed.operations,
        reserved_operations: 0,
        consumed_model_tokens: consumed.model_tokens,
        reserved_model_tokens: 0,
        consumed_cost_micro_usd: consumed.cost_micro_usd,
        reserved_cost_micro_usd: 0,
    };
    for call in calls {
        value.reserved_operations = value.reserved_operations.saturating_add(1);
        value.reserved_model_tokens = value
            .reserved_model_tokens
            .saturating_add(call.input_token_limit)
            .saturating_add(call.output_token_limit);
        value.reserved_cost_micro_usd = value
            .reserved_cost_micro_usd
            .saturating_add(call.cost_limit);
    }
    value
}

fn add_usage(
    current: ConsumedUsage,
    added: ConsumedUsage,
) -> Result<ConsumedUsage, AgentPolicyError> {
    Ok(ConsumedUsage {
        operations: current
            .operations
            .checked_add(added.operations)
            .ok_or(AgentPolicyError::Invariant)?,
        model_tokens: current
            .model_tokens
            .checked_add(added.model_tokens)
            .ok_or(AgentPolicyError::Invariant)?,
        cost_micro_usd: current
            .cost_micro_usd
            .checked_add(added.cost_micro_usd)
            .ok_or(AgentPolicyError::Invariant)?,
    })
}

fn admission_matches(admission: &AgentModelCallAdmission, call: &ModelCallRow) -> bool {
    admission.id == call.id
        && admission.lease == call.lease
        && admission.node == call.node
        && admission.kind == call.kind
        && admission.guard == call.admission_guard
}

fn admission_guard(facts: AdmissionGuardFacts<'_>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-AGENT-MODEL-CALL-ADMISSION-1\0");
    hasher.update(facts.manifest_guard);
    hasher.update(facts.id.get().to_be_bytes());
    hasher.update(facts.lease.bytes());
    hasher.update(facts.node.bytes());
    hasher.update([match facts.kind {
        ModelInputKind::Observation => 1,
        ModelInputKind::Read => 2,
    }]);
    hash_context(&mut hasher, facts.context);
    hasher.update(facts.source_guard);
    hasher.update(facts.input_token_limit.to_be_bytes());
    hasher.update(facts.output_token_limit.to_be_bytes());
    hasher.update(facts.cost_limit.to_be_bytes());
    hasher.update((facts.candidates.len() as u64).to_be_bytes());
    for candidate in facts.candidates {
        hash_context(&mut hasher, candidate.context);
        hash_account(&mut hasher, candidate.account);
        let origin = candidate.origin.as_url().as_str().as_bytes();
        hasher.update((origin.len() as u64).to_be_bytes());
        hasher.update(origin);
        hasher.update([match candidate.sensitivity {
            SemanticSensitivity::Public => 1,
            SemanticSensitivity::Sensitive => 2,
            SemanticSensitivity::Secret => 3,
        }]);
        hasher.update([match candidate.trust {
            SemanticTrust::UntrustedPage => 1,
            SemanticTrust::BrowserDerived => 2,
        }]);
        hasher.update(candidate.attested_at.millis().to_be_bytes());
    }
    hasher.finalize().into()
}

fn hash_account(hasher: &mut Sha256, account: AgentAccountScope) {
    match account {
        AgentAccountScope::Anonymous => hasher.update([0]),
        AgentAccountScope::Authenticated(id) => {
            hasher.update([1]);
            hasher.update(id.bytes());
        }
    }
}

fn hash_context(hasher: &mut Sha256, context: ContextJoin) {
    let identity = context.identity();
    hasher.update(identity.id().bytes());
    hasher.update(identity.owner().bytes());
    hasher.update(identity.profile().bytes());
    hasher.update([match identity.kind() {
        crate::ContextKind::Owned => 1,
        crate::ContextKind::BorrowedTab => 2,
        crate::ContextKind::HumanSignInHandoff => 3,
    }]);
    hasher.update(context.context_generation().get().to_be_bytes());
    hasher.update(context.navigation_epoch().get().to_be_bytes());
    hasher.update(context.frame().get().to_be_bytes());
    hasher.update(context.frame_generation().get().to_be_bytes());
    hasher.update(context.cancellation_generation().get().to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, encode_semantic_observation, encode_semantic_read,
        read_semantic_observation, AgentAccountAttestationId, AgentAccountId, AgentEffectScope,
        AgentPlanNodeAuthority, AgentPlanNodeScope, AgentRunManifestId, AgentRunScope,
        ContextCapabilities, ContextCapability, ContextId, ContextIdentity, ContextKind,
        ContextOperationId, ContextRegistry, ContextRunId, ContextSettlement, FrameGeneration,
        FrameId, SemanticCaptureInstant, SemanticDecodeContext, SemanticFrameJoin,
        SemanticFrameTrust, SemanticInvocationId, SemanticModelDeliverySettlement,
        SemanticModelEncodingBudget, SemanticObservationAssembler, SemanticObservationBudget,
        SemanticObservationId, SemanticObservationRequest, SemanticReadAuthority,
        SemanticReadBudget, SemanticReadSensitivityLimit, SemanticSnapshotGeneration,
        SemanticTokenCountQuality, SemanticTokenCountRequirement, SemanticTokenCounter,
        SemanticTokenCounterError, SemanticTokenMeasurement, SemanticTokenizerRevision,
        SEMANTIC_WIRE_VERSION,
    };
    use serde_json::{json, Value};

    const ISSUED_AT: u64 = 1_000;
    const NOW: u64 = 2_000;
    const EXPIRES_AT: u64 = 100_000;

    fn profile(value: u128) -> ProfileId {
        ProfileId::from(value)
    }

    fn origin(host: &str) -> SemanticOrigin {
        SemanticOrigin::parse(&format!("https://{host}.example.test/private")).expect("origin")
    }

    fn make_context(run: u128, profile_value: u128, context_value: u128) -> ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(context_value),
            ContextRunId::from_raw(run),
            profile(profile_value),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[ContextCapability::Observe, ContextCapability::Act],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let operation = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("begin");
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .expect("settle");
        registry.join(identity.id()).expect("join")
    }

    fn observation(
        context: ContextJoin,
        source: SemanticOrigin,
        observation_id: u64,
        nodes: Vec<Value>,
    ) -> SemanticObservation {
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            source,
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let invocation = observation_id + 10;
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": invocation,
            "c": "complete",
            "n": nodes,
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(invocation).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(invocation).expect("snapshot generation"),
            ),
            &bytes,
        )
        .expect("snapshot");
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(observation_id).expect("observation"),
            context,
            SemanticObservationBudget::try_new(16, 8_192, 1).expect("budget"),
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    fn mixed_observation(
        context: ContextJoin,
        source: SemanticOrigin,
        observation_id: u64,
    ) -> SemanticObservation {
        observation(
            context,
            source,
            observation_id,
            vec![
                json!({"k": 1, "r": "document", "o": 16}),
                json!({"k": 2, "p": 0, "r": "paragraph", "t": "public marker"}),
                json!({
                    "k": 3,
                    "p": 0,
                    "r": "paragraph",
                    "t": "private marker",
                    "q": "sensitive"
                }),
                json!({
                    "k": 4,
                    "p": 0,
                    "r": "password",
                    "n": "Password",
                    "v": {"k": "text", "value": "must-never-escape"},
                    "q": "secret",
                    "o": 2
                }),
            ],
        )
    }

    fn document_only_observation(
        context: ContextJoin,
        source: SemanticOrigin,
        observation_id: u64,
    ) -> SemanticObservation {
        observation(
            context,
            source,
            observation_id,
            vec![json!({"k": 1, "r": "document", "o": 16})],
        )
    }

    struct FixedCounter {
        revision: SemanticTokenizerRevision,
        tokens: u32,
    }

    impl SemanticTokenCounter for FixedCounter {
        fn count_tokens(
            &self,
            input: &str,
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if input.is_empty() {
                return Err(SemanticTokenCounterError::InvalidResult);
            }
            SemanticTokenMeasurement::try_new(
                self.revision.clone(),
                self.tokens,
                SemanticTokenCountQuality::ExactLocal,
            )
            .map_err(|_| SemanticTokenCounterError::InvalidResult)
        }
    }

    fn tokenizer() -> SemanticTokenizerRevision {
        SemanticTokenizerRevision::try_new("agent-policy-test-v1".to_owned()).expect("revision")
    }

    fn observation_payload(observation: &SemanticObservation, tokens: u32) -> SemanticModelPayload {
        let revision = tokenizer();
        encode_semantic_observation(
            observation,
            SemanticModelEncodingBudget::try_new(
                16_384,
                1_000,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("encoding budget"),
        )
        .expect("encode")
        .admit(
            &FixedCounter {
                revision: revision.clone(),
                tokens,
            },
            &revision,
        )
        .expect("payload")
    }

    fn read_payload(read: &SemanticReadResult<'_>, tokens: u32) -> SemanticReadModelPayload {
        let revision = tokenizer();
        encode_semantic_read(
            read,
            SemanticModelEncodingBudget::try_new(
                16_384,
                1_000,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("encoding budget"),
        )
        .expect("encode")
        .admit(
            &FixedCounter {
                revision: revision.clone(),
                tokens,
            },
            &revision,
        )
        .expect("payload")
    }

    fn effects(values: &[SemanticEffectClass]) -> AgentEffectScope {
        AgentEffectScope::try_new(values).expect("effects")
    }

    fn run_budget(operations: u32, tokens: u64, cost: u64) -> AgentRunBudget {
        AgentRunBudget::try_new(operations, tokens, cost, 1).expect("budget")
    }

    struct PolicyFixture {
        policy: AgentRunPolicy,
        lease: AgentPlanLeaseId,
    }

    fn policy_fixture(
        run: u128,
        profile_value: u128,
        source: SemanticOrigin,
        max_sensitivity: SemanticSensitivity,
        effect_values: &[SemanticEffectClass],
        budget: AgentRunBudget,
    ) -> PolicyFixture {
        let effect_scope = effects(effect_values);
        let scope = AgentRunScope::try_new(
            vec![profile(profile_value)],
            vec![AgentAccountScope::Anonymous],
            vec![source.clone()],
            max_sensitivity,
            effect_scope,
            Vec::new(),
        )
        .expect("scope");
        let authority = AgentPlanNodeAuthority::try_new(
            vec![profile(profile_value)],
            vec![AgentAccountScope::Anonymous],
            vec![source],
            max_sensitivity,
            effect_scope,
        )
        .expect("authority");
        let node_id = AgentPlanNodeId::from_raw(1);
        let manifest = AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(1),
            ContextRunId::from_raw(run),
            scope,
            budget,
            AgentPolicyInstant::from_millis(ISSUED_AT),
            AgentPolicyInstant::from_millis(EXPIRES_AT),
            vec![AgentPlanNodeScope::new(
                node_id,
                authority,
                budget,
                AgentPolicyInstant::from_millis(EXPIRES_AT - 1),
            )],
        )
        .expect("manifest");
        let lease = AgentPlanLeaseId::from_raw(1);
        let policy =
            AgentRunPolicy::try_new(manifest, vec![AgentPlanLeaseBinding::new(lease, node_id)])
                .expect("policy");
        PolicyFixture { policy, lease }
    }

    fn account(context: ContextJoin, observed_at: u64) -> AgentContextAccountBinding {
        AgentContextAccountBinding::new(
            AgentAccountAttestationId::from_raw(u128::from(observed_at)),
            context,
            AgentAccountScope::Anonymous,
            AgentPolicyInstant::from_millis(observed_at),
        )
    }

    fn call_request(
        id: u64,
        lease: AgentPlanLeaseId,
        account: AgentContextAccountBinding,
        additional_input_tokens: u32,
        output_tokens: u32,
        cost: u64,
        now: u64,
    ) -> AgentModelCallRequest {
        AgentModelCallRequest::new(
            AgentModelCallId::new(id).expect("call"),
            lease,
            account,
            AgentModelCallBudget::try_new(additional_input_tokens, output_tokens, cost)
                .expect("call budget"),
            AgentPolicyInstant::from_millis(now),
        )
    }

    #[test]
    fn observation_delivery_reserves_commits_taint_and_settles_actual_usage() {
        let source = origin("source");
        let context = make_context(7, 8, 9);
        let observation = mixed_observation(context, source.clone(), 1);
        let payload = observation_payload(&observation, 50);
        let mut fixture = policy_fixture(
            7,
            8,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let binding = account(context, NOW - 1);
        let admission = fixture
            .policy
            .prepare_observation_input(
                call_request(1, fixture.lease, binding, 10, 20, 100, NOW),
                &observation,
                &payload,
            )
            .expect("admission");
        let admission_debug = format!("{admission:?}");

        assert_eq!(fixture.policy.pending_model_calls(), 1);
        assert_eq!(fixture.policy.taints(), &[]);
        assert_eq!(
            fixture.policy.accounting(),
            AgentPolicyAccounting {
                consumed_operations: 0,
                reserved_operations: 1,
                consumed_model_tokens: 0,
                reserved_model_tokens: 80,
                consumed_cost_micro_usd: 0,
                reserved_cost_micro_usd: 100,
            }
        );

        let acknowledgement = payload
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("delivery");
        let active = fixture
            .policy
            .commit_observation_input(admission, &acknowledgement)
            .expect("commit");
        assert_eq!(fixture.policy.taints().len(), 1);
        let taint = &fixture.policy.taints()[0];
        assert_eq!(taint.context(), context);
        assert_eq!(taint.profile(), profile(8));
        assert_eq!(taint.account(), AgentAccountScope::Anonymous);
        assert_eq!(taint.sensitivity(), SemanticSensitivity::Sensitive);
        assert_eq!(taint.trust(), SemanticTrust::UntrustedPage);
        assert_eq!(
            taint.attested_at(),
            AgentPolicyInstant::from_millis(NOW - 1)
        );
        let taint_debug = format!("{taint:?}");

        let receipt = fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 55, 10, 80)
            .expect("settle");
        assert_eq!(receipt.id().get(), 1);
        assert_eq!(receipt.lease(), fixture.lease);
        assert_eq!(receipt.settlement(), AgentModelCallSettlement::Completed);
        assert_eq!(fixture.policy.pending_model_calls(), 0);
        assert_eq!(
            fixture.policy.accounting(),
            AgentPolicyAccounting {
                consumed_operations: 1,
                reserved_operations: 0,
                consumed_model_tokens: 65,
                reserved_model_tokens: 0,
                consumed_cost_micro_usd: 80,
                reserved_cost_micro_usd: 0,
            }
        );
        assert_eq!(
            fixture.policy.lease_accounting(fixture.lease),
            Some(fixture.policy.accounting())
        );
        let debug = format!("{:?} {taint_debug} {admission_debug}", fixture.policy);
        assert!(!debug.contains("source.example.test"));
        assert!(!debug.contains("private marker"));
        assert!(!debug.contains("must-never-escape"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn committed_read_merges_exact_source_taint_and_provider_failure_does_not_erase_it() {
        let source = origin("read");
        let context = make_context(17, 18, 19);
        let observation = mixed_observation(context, source.clone(), 1);
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(NOW - 2),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");
        let payload = read_payload(&read, 40);
        let mut fixture = policy_fixture(
            17,
            18,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let binding = account(context, NOW - 1);
        let admission = fixture
            .policy
            .prepare_read_input(
                call_request(1, fixture.lease, binding, 5, 15, 90, NOW),
                &read,
                &payload,
            )
            .expect("admission");
        let receipt = payload
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("delivery");
        let active = fixture
            .policy
            .commit_read_input(admission, &receipt)
            .expect("commit");
        assert_eq!(fixture.policy.taints().len(), 1);
        assert_eq!(
            fixture.policy.taints()[0].sensitivity(),
            SemanticSensitivity::Sensitive
        );
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::ProviderFailed, 43, 0, 12)
            .expect("settle failure");
        assert_eq!(fixture.policy.taints().len(), 1);
        assert_eq!(fixture.policy.accounting().consumed_operations(), 1);
        assert_eq!(fixture.policy.accounting().consumed_model_tokens(), 43);
        assert_eq!(fixture.policy.accounting().consumed_cost_micro_usd(), 12);
    }

    #[test]
    fn pre_delivery_cancellation_releases_every_reservation_and_call_ids_never_reopen() {
        let source = origin("cancel");
        let context = make_context(27, 28, 29);
        let observation = mixed_observation(context, source.clone(), 1);
        let payload = observation_payload(&observation, 20);
        let mut fixture = policy_fixture(
            27,
            28,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let binding = account(context, NOW);
        let request = call_request(1, fixture.lease, binding, 5, 10, 50, NOW);
        let admission = fixture
            .policy
            .prepare_observation_input(request, &observation, &payload)
            .expect("admission");
        fixture
            .policy
            .cancel_prepared_input(admission, AgentModelInputCancellation::Cancelled)
            .expect("cancel");

        assert_eq!(fixture.policy.pending_model_calls(), 0);
        assert_eq!(fixture.policy.taints(), &[]);
        assert_eq!(
            fixture.policy.accounting(),
            AgentPolicyAccounting {
                consumed_operations: 0,
                reserved_operations: 0,
                consumed_model_tokens: 0,
                reserved_model_tokens: 0,
                consumed_cost_micro_usd: 0,
                reserved_cost_micro_usd: 0,
            }
        );
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(request, &observation, &payload)
                .expect_err("replay"),
            AgentPolicyError::CallReplay
        );
        assert!(!fixture.policy.is_sealed());
    }

    #[test]
    fn pending_call_and_aggregate_budget_limits_include_all_reservations() {
        let source = origin("bounded");
        let context = make_context(37, 38, 39);
        let observation = mixed_observation(context, source.clone(), 1);
        let payload = observation_payload(&observation, 10);
        let binding = account(context, NOW);
        let mut fixture = policy_fixture(
            37,
            38,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let mut admissions = Vec::new();
        for id in 1..=MAX_AGENT_PENDING_MODEL_CALLS as u64 {
            admissions.push(
                fixture
                    .policy
                    .prepare_observation_input(
                        call_request(id, fixture.lease, binding, 1, 1, 1, NOW),
                        &observation,
                        &payload,
                    )
                    .expect("bounded admission"),
            );
        }
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(
                    call_request(5, fixture.lease, binding, 1, 1, 1, NOW),
                    &observation,
                    &payload,
                )
                .expect_err("pending limit"),
            AgentPolicyError::PendingCallLimit
        );
        assert_eq!(fixture.policy.accounting().reserved_operations(), 4);
        assert_eq!(fixture.policy.accounting().reserved_model_tokens(), 48);
        for admission in admissions {
            fixture
                .policy
                .cancel_prepared_input(admission, AgentModelInputCancellation::Refused)
                .expect("release");
        }

        let mut budgeted = policy_fixture(
            37,
            38,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(2, 100, 100),
        );
        let first = budgeted
            .policy
            .prepare_observation_input(
                call_request(1, budgeted.lease, binding, 10, 20, 60, NOW),
                &observation,
                &observation_payload(&observation, 50),
            )
            .expect("first reservation");
        assert_eq!(budgeted.policy.accounting().reserved_model_tokens(), 80);
        assert_eq!(
            budgeted
                .policy
                .prepare_observation_input(
                    call_request(2, budgeted.lease, binding, 0, 0, 1, NOW),
                    &observation,
                    &observation_payload(&observation, 21),
                )
                .expect_err("aggregate token budget"),
            AgentPolicyError::Budget
        );
        budgeted
            .policy
            .cancel_prepared_input(first, AgentModelInputCancellation::Refused)
            .expect("release first");
        let cost_hold = budgeted
            .policy
            .prepare_observation_input(
                call_request(3, budgeted.lease, binding, 0, 0, 100, NOW),
                &observation,
                &payload,
            )
            .expect("cost reservation");
        assert_eq!(
            budgeted
                .policy
                .prepare_observation_input(
                    call_request(4, budgeted.lease, binding, 0, 0, 1, NOW),
                    &observation,
                    &payload,
                )
                .expect_err("aggregate cost budget"),
            AgentPolicyError::Budget
        );
        budgeted
            .policy
            .cancel_prepared_input(cost_hold, AgentModelInputCancellation::Refused)
            .expect("release cost");
    }

    #[test]
    fn admission_rejects_payload_authority_freshness_effect_and_sensitivity_mismatches() {
        let source = origin("authority");
        let context = make_context(47, 48, 49);
        let observation = mixed_observation(context, source.clone(), 1);
        let other_observation = mixed_observation(context, source.clone(), 2);
        let payload = observation_payload(&observation, 10);
        let binding = account(context, NOW);

        let mut fixture = policy_fixture(
            47,
            48,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(
                    call_request(1, fixture.lease, binding, 0, 0, 0, NOW),
                    &other_observation,
                    &payload,
                )
                .expect_err("payload substitution"),
            AgentPolicyError::PayloadMismatch
        );
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(
                    call_request(
                        1,
                        fixture.lease,
                        account(
                            context,
                            50_000 - MAX_AGENT_ACCOUNT_ATTESTATION_AGE_MILLIS - 1,
                        ),
                        0,
                        0,
                        0,
                        50_000,
                    ),
                    &observation,
                    &payload,
                )
                .expect_err("stale account"),
            AgentPolicyError::AccountStale
        );
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(
                    call_request(1, fixture.lease, account(context, NOW + 1), 0, 0, 0, NOW,),
                    &observation,
                    &payload,
                )
                .expect_err("future account"),
            AgentPolicyError::ClockRegression
        );
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(
                    call_request(1, fixture.lease, binding, 0, 0, 0, EXPIRES_AT),
                    &observation,
                    &payload,
                )
                .expect_err("expired"),
            AgentPolicyError::Expired
        );

        let wrong_context = make_context(99, 48, 50);
        let wrong_owner = mixed_observation(wrong_context, source.clone(), 3);
        let wrong_owner_payload = observation_payload(&wrong_owner, 10);
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(
                    call_request(1, fixture.lease, account(wrong_context, NOW), 0, 0, 0, NOW,),
                    &wrong_owner,
                    &wrong_owner_payload,
                )
                .expect_err("wrong run"),
            AgentPolicyError::Authority
        );

        let wrong_profile_context = make_context(47, 999, 51);
        let wrong_profile = mixed_observation(wrong_profile_context, source.clone(), 4);
        let wrong_profile_payload = observation_payload(&wrong_profile, 10);
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(
                    call_request(
                        1,
                        fixture.lease,
                        account(wrong_profile_context, NOW),
                        0,
                        0,
                        0,
                        NOW,
                    ),
                    &wrong_profile,
                    &wrong_profile_payload,
                )
                .expect_err("wrong profile"),
            AgentPolicyError::SourceOutsideScope
        );

        let authenticated = AgentContextAccountBinding::new(
            AgentAccountAttestationId::from_raw(999),
            context,
            AgentAccountScope::Authenticated(AgentAccountId::from_raw(999)),
            AgentPolicyInstant::from_millis(NOW),
        );
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(
                    call_request(1, fixture.lease, authenticated, 0, 0, 0, NOW),
                    &observation,
                    &payload,
                )
                .expect_err("wrong account"),
            AgentPolicyError::SourceOutsideScope
        );

        let other_origin_observation = mixed_observation(context, origin("other"), 5);
        let other_origin_payload = observation_payload(&other_origin_observation, 10);
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(
                    call_request(1, fixture.lease, binding, 0, 0, 0, NOW),
                    &other_origin_observation,
                    &other_origin_payload,
                )
                .expect_err("wrong origin"),
            AgentPolicyError::SourceOutsideScope
        );

        let mut no_read = policy_fixture(
            47,
            48,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::ExternalWrite],
            run_budget(10, 1_000, 10_000),
        );
        assert_eq!(
            no_read
                .policy
                .prepare_observation_input(
                    call_request(1, no_read.lease, binding, 0, 0, 0, NOW),
                    &observation,
                    &payload,
                )
                .expect_err("read effect"),
            AgentPolicyError::EffectOutsideScope
        );

        let mut public_only = policy_fixture(
            47,
            48,
            source,
            SemanticSensitivity::Public,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        assert_eq!(
            public_only
                .policy
                .prepare_observation_input(
                    call_request(1, public_only.lease, binding, 0, 0, 0, NOW),
                    &observation,
                    &payload,
                )
                .expect_err("sensitive metadata"),
            AgentPolicyError::Sensitivity
        );
    }

    #[test]
    fn empty_read_has_no_ambient_source_authority() {
        let source = origin("empty-read");
        let context = make_context(57, 58, 59);
        let observation = document_only_observation(context, source.clone(), 1);
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(NOW),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");
        assert!(read.fragments().is_empty());
        let payload = read_payload(&read, 1);
        let mut fixture = policy_fixture(
            57,
            58,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        assert_eq!(
            fixture
                .policy
                .prepare_read_input(
                    call_request(1, fixture.lease, account(context, NOW), 0, 0, 0, NOW,),
                    &read,
                    &payload,
                )
                .expect_err("empty read"),
            AgentPolicyError::EmptyRead
        );
        assert_eq!(fixture.policy.accounting().reserved_operations(), 0);
    }

    #[test]
    fn mismatched_delivery_retains_ambiguity_but_provider_overage_accounts_actual_usage() {
        let source = origin("seal");
        let context = make_context(67, 68, 69);
        let first_observation = mixed_observation(context, source.clone(), 1);
        let second_observation = mixed_observation(context, source.clone(), 2);
        let first_payload = observation_payload(&first_observation, 20);
        let binding = account(context, NOW);
        let mut mismatched = policy_fixture(
            67,
            68,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let admission = mismatched
            .policy
            .prepare_observation_input(
                call_request(1, mismatched.lease, binding, 0, 10, 50, NOW),
                &first_observation,
                &first_payload,
            )
            .expect("admission");
        let mismatched_payload = observation_payload(&second_observation, 20);
        let acknowledgement = mismatched_payload
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("delivery");
        assert_eq!(
            mismatched
                .policy
                .commit_observation_input(admission, &acknowledgement)
                .expect_err("mismatch"),
            AgentPolicyError::AdmissionMismatch
        );
        assert!(mismatched.policy.is_sealed());
        assert_eq!(mismatched.policy.pending_model_calls(), 1);
        assert_eq!(mismatched.policy.taints(), &[]);
        let second_payload = observation_payload(&second_observation, 20);
        assert_eq!(
            mismatched
                .policy
                .prepare_observation_input(
                    call_request(2, mismatched.lease, binding, 0, 0, 0, NOW),
                    &second_observation,
                    &second_payload,
                )
                .expect_err("sealed"),
            AgentPolicyError::Sealed
        );

        let payload = observation_payload(&first_observation, 20);
        let mut overage = policy_fixture(
            67,
            68,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let admission = overage
            .policy
            .prepare_observation_input(
                call_request(1, overage.lease, binding, 5, 10, 50, NOW),
                &first_observation,
                &payload,
            )
            .expect("admission");
        let acknowledgement = payload
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("delivery");
        let active = overage
            .policy
            .commit_observation_input(admission, &acknowledgement)
            .expect("commit");
        assert_eq!(
            overage
                .policy
                .settle_model_call(active, AgentModelCallSettlement::Completed, 26, 0, 1)
                .expect_err("overage"),
            AgentPolicyError::ProviderUsageExceeded
        );
        assert!(overage.policy.is_sealed());
        assert_eq!(overage.policy.pending_model_calls(), 0);
        assert_eq!(overage.policy.accounting().reserved_model_tokens(), 0);
        assert_eq!(overage.policy.accounting().consumed_operations(), 1);
        assert_eq!(overage.policy.accounting().consumed_model_tokens(), 26);
        assert_eq!(overage.policy.accounting().consumed_cost_micro_usd(), 1);
        assert_eq!(overage.policy.taints().len(), 1);
    }

    #[test]
    fn missing_admission_identity_seals_the_policy_as_ambiguous() {
        let source = origin("missing");
        let mut fixture = policy_fixture(
            77,
            78,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let missing = AgentModelCallAdmission {
            id: AgentModelCallId::new(99).expect("call"),
            lease: fixture.lease,
            node: AgentPlanNodeId::from_raw(1),
            kind: ModelInputKind::Observation,
            guard: [0; 32],
        };
        assert_eq!(
            fixture
                .policy
                .cancel_prepared_input(missing, AgentModelInputCancellation::Cancelled)
                .expect_err("missing"),
            AgentPolicyError::CallMissing
        );
        assert!(fixture.policy.is_sealed());
    }

    #[test]
    fn lease_set_rejects_nonadjacent_duplicate_mutable_authority() {
        let source = origin("lease");
        let run = ContextRunId::from_raw(87);
        let effect_scope = effects(&[SemanticEffectClass::Read]);
        let budget = run_budget(10, 1_000, 10_000);
        let scope = AgentRunScope::try_new(
            vec![profile(88)],
            vec![AgentAccountScope::Anonymous],
            vec![source.clone()],
            SemanticSensitivity::Sensitive,
            effect_scope,
            Vec::new(),
        )
        .expect("scope");
        let mut nodes = Vec::new();
        for id in 1..=3_u128 {
            let authority = AgentPlanNodeAuthority::try_new(
                vec![profile(88)],
                vec![AgentAccountScope::Anonymous],
                vec![source.clone()],
                SemanticSensitivity::Sensitive,
                effect_scope,
            )
            .expect("authority");
            nodes.push(AgentPlanNodeScope::new(
                AgentPlanNodeId::from_raw(id),
                authority,
                budget,
                AgentPolicyInstant::from_millis(EXPIRES_AT - 1),
            ));
        }
        let manifest = AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(87),
            run,
            scope,
            budget,
            AgentPolicyInstant::from_millis(ISSUED_AT),
            AgentPolicyInstant::from_millis(EXPIRES_AT),
            nodes,
        )
        .expect("manifest");
        let repeated = AgentPlanLeaseId::from_raw(1);
        assert_eq!(
            AgentRunPolicy::try_new(
                manifest,
                vec![
                    AgentPlanLeaseBinding::new(repeated, AgentPlanNodeId::from_raw(1)),
                    AgentPlanLeaseBinding::new(
                        AgentPlanLeaseId::from_raw(2),
                        AgentPlanNodeId::from_raw(2),
                    ),
                    AgentPlanLeaseBinding::new(repeated, AgentPlanNodeId::from_raw(3)),
                ],
            )
            .expect_err("duplicate lease"),
            AgentPolicyError::LeaseSet
        );
    }

    #[test]
    fn taint_union_is_exact_bounded_and_merges_conservatively() {
        let source = origin("taint");
        let mut committed = Vec::new();
        for id in 1..=MAX_AGENT_TAINT_COHORTS as u128 {
            committed.push(AgentTaintCohort {
                context: make_context(97, 98, id),
                account: AgentAccountScope::Anonymous,
                origin: source.clone(),
                sensitivity: SemanticSensitivity::Public,
                trust: SemanticTrust::BrowserDerived,
                attested_at: AgentPolicyInstant::from_millis(NOW),
            });
        }
        let extra = AgentTaintCohort {
            context: make_context(97, 98, 10_000),
            account: AgentAccountScope::Anonymous,
            origin: source.clone(),
            sensitivity: SemanticSensitivity::Public,
            trust: SemanticTrust::BrowserDerived,
            attested_at: AgentPolicyInstant::from_millis(NOW),
        };
        assert_eq!(
            projected_taint_count(&committed, &[], std::slice::from_ref(&extra)),
            MAX_AGENT_TAINT_COHORTS + 1
        );
        assert_eq!(
            projected_taint_count(&committed, &[], std::slice::from_ref(&committed[0])),
            MAX_AGENT_TAINT_COHORTS
        );

        let exact_context = committed[0].context();
        let mut merged = vec![AgentTaintCohort {
            context: exact_context,
            account: AgentAccountScope::Anonymous,
            origin: source.clone(),
            sensitivity: SemanticSensitivity::Public,
            trust: SemanticTrust::BrowserDerived,
            attested_at: AgentPolicyInstant::from_millis(NOW),
        }];
        merge_taint(
            &mut merged,
            AgentTaintCohort {
                context: exact_context,
                account: AgentAccountScope::Anonymous,
                origin: source,
                sensitivity: SemanticSensitivity::Sensitive,
                trust: SemanticTrust::UntrustedPage,
                attested_at: AgentPolicyInstant::from_millis(NOW - 1),
            },
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].sensitivity(), SemanticSensitivity::Sensitive);
        assert_eq!(merged[0].trust(), SemanticTrust::UntrustedPage);
        assert_eq!(
            merged[0].attested_at(),
            AgentPolicyInstant::from_millis(NOW - 1)
        );
    }
}
