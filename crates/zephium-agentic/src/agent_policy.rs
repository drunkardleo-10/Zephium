//! Mutable bounded plan-lease accounting and committed model-input taint.
//!
//! This functional core reserves one model call before transport, records
//! source taint only after exact committed observation/diff/read/locate delivery, and
//! settles provider usage without retries. Its child effect policy reserves
//! one operation per bounded prepared action only after exact source-to-sink
//! checks and still cannot execute an action. The core owns no transport,
//! provider, timer, task, thread, page, or browser.

use std::fmt;
use std::num::NonZeroU64;

use sha2::{Digest, Sha256};
use thiserror::Error;
use zephium_core::ids::ProfileId;

mod effect;
use effect::AgentEffectRow;
pub use effect::{
    AgentActiveEffect, AgentEffectAssessment, AgentEffectAuthorization, AgentEffectCancellation,
    AgentEffectDispatchRequest, AgentEffectId, AgentEffectPermit, AgentEffectReceipt,
    AgentEffectRequest, AgentEffectSettlement, AgentFailedSemanticEffect, AgentNeedsHumanReason,
    AgentNeedsHumanTransition, AgentVerifiedSemanticEffect, MAX_AGENT_PENDING_EFFECTS,
};

use crate::semantic_diff::SemanticObservationFingerprint;
use crate::semantic_diff_model::SemanticDiffDeliveryAuthority;
use crate::semantic_extract_model::SemanticExtractionDeliveryAuthority;
use crate::semantic_locate_model::SemanticLocateDeliveryAuthority;
use crate::semantic_read_model::SemanticReadDeliveryAuthority;
use crate::semantic_screenshot::SemanticScreenshotDeliveryAuthority;
use crate::{
    AgentAccountScope, AgentAuditLedger, AgentContextAccountBinding, AgentPlanLeaseId,
    AgentPlanNodeId, AgentPolicyInstant, AgentProviderPricedUsage, AgentProviderPricingAttribution,
    AgentRunAccountingMetrics, AgentRunBudget, AgentRunManifest, AgentRunManifestId,
    AgentRunMetricClosure, ContextJoin, SemanticActionAttemptId, SemanticDiff,
    SemanticDiffDeliveryReceipt, SemanticDiffModelPayload, SemanticEffectClass,
    SemanticExtractionDeliveryReceipt, SemanticExtractionSchema, SemanticLocateDeliveryReceipt,
    SemanticLocateResult, SemanticModelPayload, SemanticObservation,
    SemanticObservationAcknowledgement, SemanticObservationGeneration, SemanticObservationId,
    SemanticOrigin, SemanticReadDeliveryReceipt, SemanticReadModelPayload, SemanticReadResult,
    SemanticReferenceId, SemanticScreenshotDeliveryReceipt, SemanticSensitivity, SemanticTrust,
};

/// Maximum model calls reserved or active in one run policy.
pub const MAX_AGENT_PENDING_MODEL_CALLS: usize = 4;
/// Maximum distinct profile/account/origin taint cohorts retained per run.
pub const MAX_AGENT_TAINT_COHORTS: usize = 128;
/// Maximum opaque actionable references retained across committed model input.
pub const MAX_AGENT_TAINT_REFERENCES: usize = 4_096;
/// Maximum age of an account attestation at model-input admission (30 seconds).
pub const MAX_AGENT_ACCOUNT_ATTESTATION_AGE_MILLIS: u64 = 30_000;
/// Maximum byte size of one copyable clean policy-settlement value.
pub const MAX_AGENT_RUN_POLICY_SETTLEMENT_BYTES: usize = 256;

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
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    source_guard: [u8; 32],
    account: AgentAccountScope,
    origin: SemanticOrigin,
    sensitivity: SemanticSensitivity,
    trust: SemanticTrust,
    attested_at: AgentPolicyInstant,
    references: Vec<SemanticReferenceId>,
}

impl AgentTaintCohort {
    /// Exact source context/document/cancellation authority delivered to the model.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Exact observation request delivered for this source cohort.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Exact progressive observation generation delivered for this cohort.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
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

    /// Number of opaque action references actually disclosed for this cohort.
    pub fn reference_count(&self) -> usize {
        self.references.len()
    }

    pub(crate) fn contains_reference(&self, reference: SemanticReferenceId) -> bool {
        self.references.binary_search(&reference).is_ok()
    }

    fn same_source(&self, other: &Self) -> bool {
        self.context == other.context
            && self.observation == other.observation
            && self.observation_generation == other.observation_generation
            && self.source_guard == other.source_guard
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
        self.references = merge_references(&self.references, &other.references);
    }
}

impl fmt::Debug for AgentTaintCohort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentTaintCohort")
            .field("context", &"[redacted]")
            .field("source_guard", &"[redacted]")
            .field("account", &"[redacted]")
            .field("origin", &"[redacted]")
            .field("sensitivity", &self.sensitivity)
            .field("trust", &self.trust)
            .field("attested_at", &self.attested_at)
            .field("references", &self.references.len())
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

/// Non-authorizing proof that run policy and durable audit drain were consumed.
///
/// Success consumes both the mutable policy and shutdown-quiescent audit
/// ledger, so neither admission nor durable delivery can reopen the settled
/// run. This value does not prove native-resource, site, provider, device, or
/// production qualification.
#[derive(Clone, Copy, Eq, PartialEq)]
#[must_use]
pub struct AgentRunPolicySettlement {
    closure: AgentRunMetricClosure,
    accounting: AgentPolicyAccounting,
}

const _: () = assert!(
    std::mem::size_of::<AgentRunPolicySettlement>() <= MAX_AGENT_RUN_POLICY_SETTLEMENT_BYTES
);

impl AgentRunPolicySettlement {
    /// Exact terminal metric closure joined before consuming the policy.
    pub const fn closure(self) -> AgentRunMetricClosure {
        self.closure
    }

    /// Final consumed accounting with every reservation proven zero.
    pub const fn accounting(self) -> AgentPolicyAccounting {
        self.accounting
    }
}

impl fmt::Debug for AgentRunPolicySettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunPolicySettlement")
            .field("closure", &self.closure)
            .field("accounting", &self.accounting)
            .field("authority", &"[none]")
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed reason a mutable policy could not be cleanly consumed.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentRunPolicySettlementError {
    /// Metric closure or accounting belonged to another manifest revision/run.
    #[error("agent run policy settlement authority mismatched")]
    Authority,
    /// Policy was previously sealed by ambiguous or mismatched state.
    #[error("agent run policy settlement found a sealed policy")]
    Sealed,
    /// Model/effect reservations or reserved accounting remain live.
    #[error("agent run policy settlement still has pending work")]
    Pending,
    /// Consumed run or plan-lease accounting contradicted receipt metrics.
    #[error("agent run policy settlement accounting mismatched")]
    Accounting,
    /// Checked final token arithmetic overflowed.
    #[error("agent run policy settlement arithmetic overflowed")]
    Overflow,
    /// Durable audit ledger belonged to another manifest revision/supervisor.
    #[error("agent run policy settlement audit authority mismatched")]
    AuditAuthority,
    /// Durable audit ledger must reject new records before terminal drain.
    #[error("agent run policy settlement audit ledger is not shutdown-sealed")]
    AuditUnsealed,
    /// Durable audit events or a delivery remain pending.
    #[error("agent run policy settlement audit drain is pending")]
    AuditPending,
    /// Ambiguous durable audit settlement fail-stopped the ledger.
    #[error("agent run policy settlement audit ledger is fail-stopped")]
    AuditFailStopped,
    /// Durable audit commits did not cover every metric-closure event.
    #[error("agent run policy settlement audit coverage mismatched")]
    AuditCoverage,
}

/// Recoverable refusal retaining complete policy and audit-drain ownership.
#[must_use]
pub struct AgentRunPolicySettlementRefusal {
    error: AgentRunPolicySettlementError,
    policy: AgentRunPolicy,
    audit: AgentAuditLedger,
}

impl AgentRunPolicySettlementRefusal {
    /// Closed refusal reason.
    pub const fn error(&self) -> AgentRunPolicySettlementError {
        self.error
    }

    /// Retained policy state for inspection without extracting authority.
    pub const fn policy(&self) -> &AgentRunPolicy {
        &self.policy
    }

    /// Retained durable audit ledger for exact drain or inspection.
    pub const fn audit(&self) -> &AgentAuditLedger {
        &self.audit
    }

    /// Recovers complete mutable policy and audit-drain ownership.
    pub fn into_parts(self) -> (AgentRunPolicy, AgentAuditLedger) {
        (self.policy, self.audit)
    }
}

impl fmt::Debug for AgentRunPolicySettlementRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunPolicySettlementRefusal")
            .field("error", &self.error)
            .field("manifest", &self.policy.manifest().id())
            .field("pending_model_calls", &self.policy.pending_model_calls())
            .field("pending_effects", &self.policy.pending_effects())
            .field("sealed", &self.policy.is_sealed())
            .field("audit", &self.audit.status())
            .field("authority", &"[retained]")
            .field("content", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModelInputKind {
    Observation,
    Diff,
    Read,
    Extraction,
    Screenshot,
    Locate,
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

#[derive(Clone, Copy)]
struct ModelInputTokenReservation {
    measured: u64,
    additional: u64,
}

/// Expected content-free coordinates for one provisional provider call.
#[derive(Clone, Copy)]
pub(crate) struct AgentModelCallExpectation {
    manifest: AgentRunManifestId,
    call: AgentModelCallId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
}

impl AgentModelCallExpectation {
    /// Captures the exact provisional coordinates checked before policy mutation.
    pub(crate) const fn new(
        manifest: AgentRunManifestId,
        call: AgentModelCallId,
        lease: AgentPlanLeaseId,
        node: AgentPlanNodeId,
    ) -> Self {
        Self {
            manifest,
            call,
            lease,
            node,
        }
    }
}

/// Exact purpose-bound extraction evidence awaiting policy reservation.
pub(crate) struct AgentProviderExtractionInput<'a, 'read> {
    baseline: &'a SemanticObservationAcknowledgement,
    schema: &'a SemanticExtractionSchema,
    read: &'a SemanticReadResult<'read>,
    delivery: &'a SemanticExtractionDeliveryAuthority,
    structured_input_tokens: u64,
}

impl<'a, 'read> AgentProviderExtractionInput<'a, 'read> {
    pub(crate) const fn new(
        baseline: &'a SemanticObservationAcknowledgement,
        schema: &'a SemanticExtractionSchema,
        read: &'a SemanticReadResult<'read>,
        delivery: &'a SemanticExtractionDeliveryAuthority,
        structured_input_tokens: u64,
    ) -> Self {
        Self {
            baseline,
            schema,
            read,
            delivery,
            structured_input_tokens,
        }
    }
}

/// Non-cloneable pre-transport reservation for one exact model input.
#[must_use]
pub struct AgentModelCallAdmission {
    manifest: AgentRunManifestId,
    id: AgentModelCallId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    kind: ModelInputKind,
    input_token_limit: u64,
    output_token_limit: u64,
    cost_limit_micro_usd: u64,
    guard: [u8; 32],
}

impl AgentModelCallAdmission {
    /// Exact immutable manifest revision reserving this call.
    pub const fn manifest(&self) -> AgentRunManifestId {
        self.manifest
    }

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

    /// Complete provider-accounted input-token reservation for this call.
    pub const fn input_token_limit(&self) -> u64 {
        self.input_token_limit
    }

    /// Provider-accounted output-token reservation for this call.
    pub const fn output_token_limit(&self) -> u64 {
        self.output_token_limit
    }

    /// Provider-cost reservation in millionths of a US dollar.
    pub const fn cost_limit_micro_usd(&self) -> u64 {
        self.cost_limit_micro_usd
    }
}

impl fmt::Debug for AgentModelCallAdmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentModelCallAdmission")
            .field("manifest", &self.manifest)
            .field("id", &self.id)
            .field("lease", &self.lease)
            .field("node", &self.node)
            .field("kind", &self.kind)
            .field("input_token_limit", &self.input_token_limit)
            .field("output_token_limit", &self.output_token_limit)
            .field("cost_limit_micro_usd", &self.cost_limit_micro_usd)
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Non-cloneable exact call whose model input committed and taint is retained.
#[must_use]
pub struct AgentActiveModelCall {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    id: AgentModelCallId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    guard: [u8; 32],
}

impl AgentActiveModelCall {
    /// Exact immutable manifest revision governing this call.
    pub const fn manifest(&self) -> AgentRunManifestId {
        self.manifest
    }

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

    /// Whether this call belongs to one exact canonical manifest revision.
    pub(crate) fn matches_manifest_revision(
        &self,
        manifest: AgentRunManifestId,
        manifest_guard: [u8; 32],
    ) -> bool {
        self.manifest == manifest && self.manifest_guard == manifest_guard
    }

    pub(crate) const fn manifest_guard_for_metrics(&self) -> [u8; 32] {
        self.manifest_guard
    }

    #[cfg(test)]
    pub(crate) const fn for_progress_test(
        manifest: &AgentRunManifest,
        id: AgentModelCallId,
        lease: AgentPlanLeaseId,
        node: AgentPlanNodeId,
    ) -> Self {
        Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            id,
            lease,
            node,
            guard: [0; 32],
        }
    }
}

impl fmt::Debug for AgentActiveModelCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentActiveModelCall")
            .field("manifest", &self.manifest)
            .field("manifest_guard", &"[redacted]")
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

/// Terminal class when committed provider usage cannot be determined.
///
/// A completed response must carry exact provider accounting and therefore is
/// deliberately absent. Failure and cancellation charge the entire admitted
/// token and cost reservation so an ambiguous network outcome can never
/// release budget that the provider may have consumed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentModelCallUnaccountedSettlement {
    /// Provider failed after input commitment without trustworthy usage.
    ProviderFailed,
    /// Cancellation won after input commitment without trustworthy usage.
    Cancelled,
}

impl AgentModelCallUnaccountedSettlement {
    const fn terminal(self) -> AgentModelCallSettlement {
        match self {
            Self::ProviderFailed => AgentModelCallSettlement::ProviderFailed,
            Self::Cancelled => AgentModelCallSettlement::Cancelled,
        }
    }
}

/// Source of the usage charged by a terminal model-call receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentModelUsageAccounting {
    /// Exact trusted token and cost values were available, including proven zero.
    Exact,
    /// Provider tokens are exact and cost is a checked trusted-catalog ceiling.
    PricedCeiling,
    /// Usage was unknowable, so the complete admitted ceilings were charged.
    ReservationCeiling,
}

#[derive(Clone, Copy)]
struct TerminalModelUsage {
    accounting: AgentModelUsageAccounting,
    pricing_attribution: Option<AgentProviderPricingAttribution>,
    input_tokens: u64,
    output_tokens: u64,
    cost_micro_usd: u64,
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct AgentModelReceiptTestUsage {
    accounting: AgentModelUsageAccounting,
    pricing_attribution: Option<AgentProviderPricingAttribution>,
    input_tokens: u64,
    output_tokens: u64,
    cost_micro_usd: u64,
}

#[cfg(test)]
impl AgentModelReceiptTestUsage {
    pub(crate) const fn new(
        accounting: AgentModelUsageAccounting,
        pricing_attribution: Option<AgentProviderPricingAttribution>,
        input_tokens: u64,
        output_tokens: u64,
        cost_micro_usd: u64,
    ) -> Self {
        Self {
            accounting,
            pricing_attribution,
            input_tokens,
            output_tokens,
            cost_micro_usd,
        }
    }
}

/// Content-free terminal model-call accounting receipt.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentModelCallReceipt {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    id: AgentModelCallId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    settlement: AgentModelCallSettlement,
    usage_accounting: AgentModelUsageAccounting,
    pricing_attribution: Option<AgentProviderPricingAttribution>,
    input_tokens: u64,
    output_tokens: u64,
    cost_micro_usd: u64,
}

impl AgentModelCallReceipt {
    /// Exact immutable manifest revision that accounted this call.
    pub const fn manifest(self) -> AgentRunManifestId {
        self.manifest
    }

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

    /// Whether charged usage is exact, catalog-priced, or reservation-ceiling.
    pub const fn usage_accounting(self) -> AgentModelUsageAccounting {
        self.usage_accounting
    }

    /// Checked provider billing, pricing, and usage-subset attribution, if available.
    pub const fn pricing_attribution(self) -> Option<AgentProviderPricingAttribution> {
        self.pricing_attribution
    }

    /// Provider-accounted or conservatively charged input tokens.
    pub const fn input_tokens(self) -> u64 {
        self.input_tokens
    }

    /// Provider-accounted or conservatively charged output tokens.
    pub const fn output_tokens(self) -> u64 {
        self.output_tokens
    }

    /// Provider-accounted or conservatively charged cost in micro-USD.
    pub const fn cost_micro_usd(self) -> u64 {
        self.cost_micro_usd
    }

    /// Whether this receipt belongs to one exact canonical manifest revision.
    pub(crate) fn matches_manifest_revision(
        self,
        manifest: AgentRunManifestId,
        manifest_guard: [u8; 32],
    ) -> bool {
        self.manifest == manifest && self.manifest_guard == manifest_guard
    }

    #[cfg(test)]
    pub(crate) const fn for_progress_test(
        manifest: &AgentRunManifest,
        id: AgentModelCallId,
        lease: AgentPlanLeaseId,
        node: AgentPlanNodeId,
        settlement: AgentModelCallSettlement,
    ) -> Self {
        Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            id,
            lease,
            node,
            settlement,
            usage_accounting: AgentModelUsageAccounting::Exact,
            pricing_attribution: None,
            input_tokens: 0,
            output_tokens: 0,
            cost_micro_usd: 0,
        }
    }

    #[cfg(test)]
    pub(crate) const fn for_metrics_test(
        manifest: &AgentRunManifest,
        id: AgentModelCallId,
        lease: AgentPlanLeaseId,
        node: AgentPlanNodeId,
        settlement: AgentModelCallSettlement,
        usage: AgentModelReceiptTestUsage,
    ) -> Self {
        Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            id,
            lease,
            node,
            settlement,
            usage_accounting: usage.accounting,
            pricing_attribution: usage.pricing_attribution,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cost_micro_usd: usage.cost_micro_usd,
        }
    }
}

impl fmt::Debug for AgentModelCallReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentModelCallReceipt")
            .field("manifest", &self.manifest)
            .field("manifest_guard", &"[redacted]")
            .field("id", &self.id)
            .field("lease", &self.lease)
            .field("node", &self.node)
            .field("settlement", &self.settlement)
            .field("usage_accounting", &self.usage_accounting)
            .field("pricing_attribution", &self.pricing_attribution)
            .field("input_tokens", &self.input_tokens)
            .field("output_tokens", &self.output_tokens)
            .field("cost_micro_usd", &self.cost_micro_usd)
            .finish()
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
    effects: Vec<AgentEffectRow>,
    last_call: Option<AgentModelCallId>,
    last_effect: Option<AgentEffectId>,
    last_action_attempt: Option<SemanticActionAttemptId>,
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
            effects: Vec::with_capacity(MAX_AGENT_PENDING_EFFECTS),
            last_call: None,
            last_effect: None,
            last_action_attempt: None,
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

    /// Semantic effects currently reserved or dispatched.
    pub fn pending_effects(&self) -> usize {
        self.effects.len()
    }

    /// Durable effects currently holding a canonical origin serialization slot.
    pub fn pending_origin_writes(&self) -> usize {
        self.effects
            .iter()
            .filter(|effect| effect.requires_origin_serialization())
            .count()
    }

    /// Whether a mismatched/ambiguous settlement terminally sealed this policy.
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// Consumed and reserved run-wide accounting.
    pub fn accounting(&self) -> AgentPolicyAccounting {
        accounting(self.consumed, self.calls.iter(), self.effects.iter())
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
            self.effects.iter().filter(|effect| effect.lease() == lease),
        ))
    }

    /// Consumes one clean terminal policy and its exact durable audit drain.
    ///
    /// Every refusal retains the complete policy so pending or ambiguous state
    /// cannot be discarded as a false terminal. Success drops mutable taint,
    /// lease, and audit-delivery state only after all reservations are zero,
    /// consumed run/node accounting matches the already-closed receipt
    /// reducers, and every closure event is durably committed.
    pub fn settle_metric_closure(
        self,
        closure: AgentRunMetricClosure,
        metrics: &AgentRunAccountingMetrics,
        audit: AgentAuditLedger,
    ) -> Result<AgentRunPolicySettlement, Box<AgentRunPolicySettlementRefusal>> {
        match validate_metric_settlement(&self, closure, metrics, &audit) {
            Ok(accounting) => Ok(AgentRunPolicySettlement {
                closure,
                accounting,
            }),
            Err(error) => Err(Box::new(AgentRunPolicySettlementRefusal {
                error,
                policy: self,
                audit,
            })),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_single_lease_consumed_for_metric_settlement_test(
        &mut self,
        operations: u32,
        model_tokens: u64,
        cost_micro_usd: u64,
    ) {
        assert_eq!(self.leases.len(), 1);
        let consumed = ConsumedUsage {
            operations,
            model_tokens,
            cost_micro_usd,
        };
        self.consumed = consumed;
        self.leases[0].consumed = consumed;
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
        let additional_input_tokens = u64::from(request.budget().additional_input_tokens());
        self.prepare_model_input(
            request,
            context,
            ModelInputKind::Observation,
            source_guard,
            candidates,
            ModelInputTokenReservation {
                measured: u64::from(payload.token_measurement().tokens()),
                additional: additional_input_tokens,
            },
        )
    }

    /// Reserves exact semantic-diff input before any model transport receives bytes.
    ///
    /// The diff must extend an exact baseline already committed to this policy.
    /// Current references, including unchanged rebases, are conservatively
    /// joined to that baseline before the new reservation can exist.
    pub fn prepare_diff_input(
        &mut self,
        request: AgentModelCallRequest,
        diff: &SemanticDiff,
        payload: &SemanticDiffModelPayload,
    ) -> Result<AgentModelCallAdmission, AgentPolicyError> {
        if !payload.matches_diff(diff) {
            return Err(AgentPolicyError::PayloadMismatch);
        }
        let context = diff
            .frames()
            .first()
            .map(|frame| frame.frame().context())
            .ok_or(AgentPolicyError::EmptyDiff)?;
        let candidates = diff_taints(diff, request.account(), &self.taints)?;
        let additional_input_tokens = u64::from(request.budget().additional_input_tokens());
        self.prepare_model_input(
            request,
            context,
            ModelInputKind::Diff,
            diff.guard(),
            candidates,
            ModelInputTokenReservation {
                measured: u64::from(payload.token_measurement().tokens()),
                additional: additional_input_tokens,
            },
        )
    }

    /// Reserves one exact whole-provider-input measurement for a bound diff draft.
    ///
    /// This crate-private path is used only after the fixed provider codec and
    /// an exact local structured-input counter have both succeeded. Expected
    /// call coordinates prevent a provisional continuation identity from being
    /// rebound to a different policy, lease, or plan node.
    pub(crate) fn prepare_provider_diff_input(
        &mut self,
        request: AgentModelCallRequest,
        expected: AgentModelCallExpectation,
        diff: &SemanticDiff,
        delivery: &SemanticDiffDeliveryAuthority,
        structured_input_tokens: u64,
    ) -> Result<AgentModelCallAdmission, AgentPolicyError> {
        if request.id() != expected.call
            || request.lease() != expected.lease
            || self.manifest.id() != expected.manifest
            || self
                .lease_index(expected.lease)
                .and_then(|index| self.leases.get(index))
                .is_none_or(|lease| lease.binding.node() != expected.node)
        {
            return Err(AgentPolicyError::Authority);
        }
        if !delivery.matches_diff(diff) {
            return Err(AgentPolicyError::PayloadMismatch);
        }
        let context = diff
            .frames()
            .first()
            .map(|frame| frame.frame().context())
            .ok_or(AgentPolicyError::EmptyDiff)?;
        let candidates = diff_taints(diff, request.account(), &self.taints)?;
        self.prepare_model_input(
            request,
            context,
            ModelInputKind::Diff,
            diff.guard(),
            candidates,
            ModelInputTokenReservation {
                measured: structured_input_tokens,
                additional: 0,
            },
        )
    }

    /// Reserves one exact whole-provider-input measurement for a locate result.
    ///
    /// The locate result must be bound to an observation already committed to
    /// this policy. Candidate taint is copied only from that exact baseline, so
    /// the derived opaque-reference subset cannot widen origin, account,
    /// sensitivity, trust, or browser authority.
    pub(crate) fn prepare_provider_locate_input(
        &mut self,
        request: AgentModelCallRequest,
        expected: AgentModelCallExpectation,
        result: &SemanticLocateResult,
        delivery: &SemanticLocateDeliveryAuthority,
        structured_input_tokens: u64,
    ) -> Result<AgentModelCallAdmission, AgentPolicyError> {
        if request.id() != expected.call
            || request.lease() != expected.lease
            || self.manifest.id() != expected.manifest
            || self
                .lease_index(expected.lease)
                .and_then(|index| self.leases.get(index))
                .is_none_or(|lease| lease.binding.node() != expected.node)
        {
            return Err(AgentPolicyError::Authority);
        }
        if !delivery.matches_result(result) {
            return Err(AgentPolicyError::PayloadMismatch);
        }
        let candidates = locate_taints(result, request.account(), &self.taints)?;
        self.prepare_model_input(
            request,
            result.context(),
            ModelInputKind::Locate,
            result.guard(),
            candidates,
            ModelInputTokenReservation {
                measured: structured_input_tokens,
                additional: 0,
            },
        )
    }

    /// Reserves one exact whole-provider-input measurement for a read result.
    ///
    /// The result must come from the exact observation already present in the
    /// retained provider transcript. Rejoining that committed baseline keeps
    /// the prior origin/account/sensitivity/reference taint unchanged: a read
    /// can disclose more value text, but it cannot create browser authority or
    /// lower the sensitivity of the model context that already contained it.
    pub(crate) fn prepare_provider_read_input(
        &mut self,
        request: AgentModelCallRequest,
        expected: AgentModelCallExpectation,
        baseline: &SemanticObservationAcknowledgement,
        read: &SemanticReadResult<'_>,
        delivery: &SemanticReadDeliveryAuthority,
        structured_input_tokens: u64,
    ) -> Result<AgentModelCallAdmission, AgentPolicyError> {
        if request.id() != expected.call
            || request.lease() != expected.lease
            || self.manifest.id() != expected.manifest
            || self
                .lease_index(expected.lease)
                .and_then(|index| self.leases.get(index))
                .is_none_or(|lease| lease.binding.node() != expected.node)
        {
            return Err(AgentPolicyError::Authority);
        }
        if !delivery.matches_read(read) || !read.matches_acknowledgement(baseline) {
            return Err(AgentPolicyError::PayloadMismatch);
        }
        let candidates = provider_read_taints(read, baseline, request.account(), &self.taints)?;
        self.prepare_model_input(
            request,
            read.context(),
            ModelInputKind::Read,
            read.guard(),
            candidates,
            ModelInputTokenReservation {
                measured: structured_input_tokens,
                additional: 0,
            },
        )
    }

    /// Reserves one exact whole-provider-input measurement for extraction mapping.
    ///
    /// The schema and bounded read must match the exact move-only delivery
    /// authority, and the read must still derive from the committed provider
    /// baseline. Candidate taint remains the unchanged baseline projection.
    pub(crate) fn prepare_provider_extraction_input(
        &mut self,
        request: AgentModelCallRequest,
        expected: AgentModelCallExpectation,
        input: AgentProviderExtractionInput<'_, '_>,
    ) -> Result<AgentModelCallAdmission, AgentPolicyError> {
        if request.id() != expected.call
            || request.lease() != expected.lease
            || self.manifest.id() != expected.manifest
            || self
                .lease_index(expected.lease)
                .and_then(|index| self.leases.get(index))
                .is_none_or(|lease| lease.binding.node() != expected.node)
        {
            return Err(AgentPolicyError::Authority);
        }
        if !input.delivery.matches(input.schema, input.read)
            || !input.read.matches_acknowledgement(input.baseline)
        {
            return Err(AgentPolicyError::PayloadMismatch);
        }
        let candidates =
            provider_read_taints(input.read, input.baseline, request.account(), &self.taints)?;
        self.prepare_model_input(
            request,
            input.delivery.context(),
            ModelInputKind::Extraction,
            input.delivery.guard(),
            candidates,
            ModelInputTokenReservation {
                measured: input.structured_input_tokens,
                additional: 0,
            },
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
        let additional_input_tokens = u64::from(request.budget().additional_input_tokens());
        self.prepare_model_input(
            request,
            context,
            ModelInputKind::Read,
            read.guard(),
            candidates,
            ModelInputTokenReservation {
                measured: u64::from(payload.token_measurement().tokens()),
                additional: additional_input_tokens,
            },
        )
    }

    /// Reserves one exact whole-provider-input measurement for a visual result.
    ///
    /// This crate-private path accepts only a fixed provider draft holding the
    /// exact move-only screenshot delivery authority. The original semantic
    /// observation is rejoined so visual pixels cannot be rebound to another
    /// context, generation, origin inventory, or policy call.
    pub(crate) fn prepare_provider_screenshot_input(
        &mut self,
        request: AgentModelCallRequest,
        expected: AgentModelCallExpectation,
        observation: &SemanticObservation,
        delivery: &SemanticScreenshotDeliveryAuthority,
        structured_input_tokens: u64,
    ) -> Result<AgentModelCallAdmission, AgentPolicyError> {
        if request.id() != expected.call
            || request.lease() != expected.lease
            || self.manifest.id() != expected.manifest
            || self
                .lease_index(expected.lease)
                .and_then(|index| self.leases.get(index))
                .is_none_or(|lease| lease.binding.node() != expected.node)
        {
            return Err(AgentPolicyError::Authority);
        }
        if !delivery.matches_observation(observation) {
            return Err(AgentPolicyError::PayloadMismatch);
        }
        let context = delivery.context();
        let candidates = screenshot_taints(observation, request.account(), delivery.guard())?;
        self.prepare_model_input(
            request,
            context,
            ModelInputKind::Screenshot,
            delivery.guard(),
            candidates,
            ModelInputTokenReservation {
                measured: structured_input_tokens,
                additional: 0,
            },
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

    /// Commits exact diff taint after exact transport delivery proof.
    pub fn commit_diff_input(
        &mut self,
        admission: AgentModelCallAdmission,
        receipt: &SemanticDiffDeliveryReceipt,
    ) -> Result<AgentActiveModelCall, AgentPolicyError> {
        self.commit_model_input(admission, ModelInputKind::Diff, receipt.guard())
    }

    /// Commits one exact locate-result taint projection after transport delivery.
    pub fn commit_locate_input(
        &mut self,
        admission: AgentModelCallAdmission,
        receipt: &SemanticLocateDeliveryReceipt,
    ) -> Result<AgentActiveModelCall, AgentPolicyError> {
        self.commit_model_input(admission, ModelInputKind::Locate, receipt.guard())
    }

    /// Commits exact read taint after exact transport delivery receipt.
    pub fn commit_read_input(
        &mut self,
        admission: AgentModelCallAdmission,
        receipt: &SemanticReadDeliveryReceipt,
    ) -> Result<AgentActiveModelCall, AgentPolicyError> {
        self.commit_model_input(admission, ModelInputKind::Read, receipt.guard())
    }

    /// Commits exact extraction-mapping taint after provider disclosure.
    pub fn commit_extraction_input(
        &mut self,
        admission: AgentModelCallAdmission,
        receipt: &SemanticExtractionDeliveryReceipt,
    ) -> Result<AgentActiveModelCall, AgentPolicyError> {
        self.commit_model_input(admission, ModelInputKind::Extraction, receipt.guard())
    }

    /// Commits exact sensitive visual taint after transport disclosure.
    pub fn commit_screenshot_input(
        &mut self,
        admission: AgentModelCallAdmission,
        receipt: &SemanticScreenshotDeliveryReceipt,
    ) -> Result<AgentActiveModelCall, AgentPolicyError> {
        self.commit_model_input(admission, ModelInputKind::Screenshot, receipt.guard())
    }

    /// Releases a pre-delivery reservation after refusal or exact cancellation.
    pub fn cancel_prepared_input(
        &mut self,
        admission: AgentModelCallAdmission,
        _cancellation: AgentModelInputCancellation,
    ) -> Result<(), AgentPolicyError> {
        let index = self.call_index_or_seal(admission.id)?;
        if self.calls[index].state != ModelCallState::Prepared
            || admission.manifest != self.manifest.id()
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
        let index = self.validate_active_model_call(&active)?;
        self.settle_validated_model_call(
            index,
            settlement,
            TerminalModelUsage {
                accounting: AgentModelUsageAccounting::Exact,
                pricing_attribution: None,
                input_tokens,
                output_tokens,
                cost_micro_usd,
            },
        )
    }

    /// Settles exact provider tokens with an opaque checked catalog-cost ceiling.
    ///
    /// The priced value can be constructed only by the provider pricing
    /// contract after exact provider/model/tokenizer/billing/revision matching.
    /// Pricing details remain joined to the move-only terminal authority until
    /// this single policy transition.
    pub fn settle_model_call_priced(
        &mut self,
        active: AgentActiveModelCall,
        settlement: AgentModelCallSettlement,
        priced: AgentProviderPricedUsage,
    ) -> Result<AgentModelCallReceipt, AgentPolicyError> {
        let index = self.validate_active_model_call(&active)?;
        let (usage, cost_ceiling_micro_usd, pricing_attribution) = priced.into_policy_parts();
        self.settle_validated_model_call(
            index,
            settlement,
            TerminalModelUsage {
                accounting: AgentModelUsageAccounting::PricedCeiling,
                pricing_attribution: Some(pricing_attribution),
                input_tokens: usage.input_tokens(),
                output_tokens: usage.output_tokens(),
                cost_micro_usd: cost_ceiling_micro_usd,
            },
        )
    }

    /// Settles a committed call whose provider usage is unknowable.
    ///
    /// This is the only safe terminal path after an ambiguous send, stream, or
    /// cancellation outcome. It consumes the complete admitted input-token,
    /// output-token, and cost ceilings and therefore cannot make retry budget
    /// appear available after provider-side work may have occurred.
    pub fn settle_model_call_unaccounted(
        &mut self,
        active: AgentActiveModelCall,
        settlement: AgentModelCallUnaccountedSettlement,
    ) -> Result<AgentModelCallReceipt, AgentPolicyError> {
        let index = self.validate_active_model_call(&active)?;
        let call = &self.calls[index];
        let input_tokens = call.input_token_limit;
        let output_tokens = call.output_token_limit;
        let cost_micro_usd = call.cost_limit;
        self.settle_validated_model_call(
            index,
            settlement.terminal(),
            TerminalModelUsage {
                accounting: AgentModelUsageAccounting::ReservationCeiling,
                pricing_attribution: None,
                input_tokens,
                output_tokens,
                cost_micro_usd,
            },
        )
    }

    fn validate_active_model_call(
        &mut self,
        active: &AgentActiveModelCall,
    ) -> Result<usize, AgentPolicyError> {
        let index = self.call_index_or_seal(active.id)?;
        let call = &self.calls[index];
        if call.state != ModelCallState::Delivered
            || !active.matches_manifest_revision(self.manifest.id(), self.manifest.guard())
            || call.lease != active.lease
            || call.node != active.node
            || call.admission_guard != active.guard
        {
            self.sealed = true;
            return Err(AgentPolicyError::AdmissionMismatch);
        }
        Ok(index)
    }

    fn settle_validated_model_call(
        &mut self,
        index: usize,
        settlement: AgentModelCallSettlement,
        usage: TerminalModelUsage,
    ) -> Result<AgentModelCallReceipt, AgentPolicyError> {
        let call = &self.calls[index];
        if matches!(usage.accounting, AgentModelUsageAccounting::PricedCeiling)
            != usage.pricing_attribution.is_some()
        {
            self.sealed = true;
            return Err(AgentPolicyError::Invariant);
        }
        let provider_usage_exceeded = usage.input_tokens > call.input_token_limit
            || usage.output_tokens > call.output_token_limit
            || usage.cost_micro_usd > call.cost_limit;
        let total_tokens = match usage.input_tokens.checked_add(usage.output_tokens) {
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
            cost_micro_usd: usage.cost_micro_usd,
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
            manifest: self.manifest.id(),
            manifest_guard: self.manifest.guard(),
            id: call.id,
            lease: call.lease,
            node: call.node,
            settlement,
            usage_accounting: usage.accounting,
            pricing_attribution: usage.pricing_attribution,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cost_micro_usd: usage.cost_micro_usd,
        })
    }

    fn prepare_model_input(
        &mut self,
        request: AgentModelCallRequest,
        context: ContextJoin,
        kind: ModelInputKind,
        source_guard: [u8; 32],
        candidates: Vec<AgentTaintCohort>,
        token_reservation: ModelInputTokenReservation,
    ) -> Result<AgentModelCallAdmission, AgentPolicyError> {
        let id = request.id();
        let lease = request.lease();
        let account = request.account();
        let budget = request.budget();
        if self.sealed {
            return Err(AgentPolicyError::Sealed);
        }
        if !self.effects.is_empty() {
            return Err(AgentPolicyError::EffectPending);
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

        let input_token_limit = token_reservation
            .measured
            .checked_add(token_reservation.additional)
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
        let (projected_cohorts, projected_references) =
            projected_taint_usage(&self.taints, &self.calls, &candidates)?;
        if projected_cohorts > MAX_AGENT_TAINT_COHORTS {
            return Err(AgentPolicyError::TaintLimit);
        }
        if projected_references > MAX_AGENT_TAINT_REFERENCES {
            return Err(AgentPolicyError::TaintReferenceLimit);
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
            manifest: self.manifest.id(),
            id,
            lease,
            node: node_id,
            kind,
            input_token_limit,
            output_token_limit,
            cost_limit_micro_usd: budget.cost_micro_usd(),
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
            || admission.manifest != self.manifest.id()
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
            manifest: admission.manifest,
            manifest_guard: self.manifest.guard(),
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
            .field("pending_effects", &self.pending_effects())
            .field("sealed", &self.sealed)
            .field("content", &"[redacted]")
            .finish()
    }
}

fn validate_metric_settlement(
    policy: &AgentRunPolicy,
    closure: AgentRunMetricClosure,
    metrics: &AgentRunAccountingMetrics,
    audit: &AgentAuditLedger,
) -> Result<AgentPolicyAccounting, AgentRunPolicySettlementError> {
    if !closure.matches_manifest_revision(policy.manifest())
        || !metrics.matches_metric_scope(policy.manifest(), closure.supervisor())
    {
        return Err(AgentRunPolicySettlementError::Authority);
    }
    if !audit.matches_run_scope(policy.manifest(), closure.supervisor()) {
        return Err(AgentRunPolicySettlementError::AuditAuthority);
    }
    let snapshot = metrics.snapshot();
    if snapshot.operations() != closure.operations()
        || snapshot.model().calls() != closure.model_calls()
        || snapshot.effects().attempts() != closure.effects()
    {
        return Err(AgentRunPolicySettlementError::Authority);
    }
    if policy.is_sealed() {
        return Err(AgentRunPolicySettlementError::Sealed);
    }

    let accounting = policy.accounting();
    if policy.pending_model_calls() != 0
        || policy.pending_effects() != 0
        || policy.pending_origin_writes() != 0
        || accounting.reserved_operations() != 0
        || accounting.reserved_model_tokens() != 0
        || accounting.reserved_cost_micro_usd() != 0
    {
        return Err(AgentRunPolicySettlementError::Pending);
    }

    let model_tokens = snapshot
        .model()
        .input_tokens()
        .checked_add(snapshot.model().output_tokens())
        .ok_or(AgentRunPolicySettlementError::Overflow)?;
    if accounting.consumed_operations() != snapshot.operations()
        || accounting.consumed_model_tokens() != model_tokens
        || accounting.consumed_cost_micro_usd() != snapshot.model().cost_micro_usd()
        || policy.leases.len() != metrics.nodes().len()
    {
        return Err(AgentRunPolicySettlementError::Accounting);
    }

    for (lease, node) in policy.leases.iter().zip(metrics.nodes()) {
        let lease_accounting = policy
            .lease_accounting(lease.binding.lease())
            .ok_or(AgentRunPolicySettlementError::Accounting)?;
        if lease.binding.node() != node.node()
            || lease_accounting.reserved_operations() != 0
            || lease_accounting.reserved_model_tokens() != 0
            || lease_accounting.reserved_cost_micro_usd() != 0
            || lease_accounting.consumed_operations() != node.operations()
            || lease_accounting.consumed_model_tokens() != node.model_tokens()
            || lease_accounting.consumed_cost_micro_usd() != node.cost_micro_usd()
        {
            return Err(AgentRunPolicySettlementError::Accounting);
        }
    }

    let audit_status = audit.status();
    if audit_status.fail_stopped() {
        return Err(AgentRunPolicySettlementError::AuditFailStopped);
    }
    if !audit_status.shutdown_sealed() {
        return Err(AgentRunPolicySettlementError::AuditUnsealed);
    }
    if !audit.is_quiescent() || audit_status.pending() != 0 || audit_status.in_flight() != 0 {
        return Err(AgentRunPolicySettlementError::AuditPending);
    }
    if audit_status.committed() != closure.events() {
        return Err(AgentRunPolicySettlementError::AuditCoverage);
    }
    Ok(accounting)
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
    /// At least one semantic effect freezes additional model input.
    #[error("agent policy semantic effect is pending")]
    EffectPending,
    /// Four prepared/dispatched semantic effects already consume the ceiling.
    #[error("agent policy pending semantic-effect ceiling reached")]
    PendingEffectLimit,
    /// The exact prepared action already has one pending authorization.
    #[error("agent policy action already has a pending semantic effect")]
    EffectActionPending,
    /// A durable write already owns the same canonical destination origin.
    #[error("agent policy destination origin already has a pending write")]
    OriginWritePending,
    /// Model input is still reserved or active while an effect seeks authority.
    #[error("agent policy model call is pending at the effect boundary")]
    ModelCallPending,
    /// Effect identity was reused or regressed.
    #[error("agent policy semantic effect identity replay")]
    EffectReplay,
    /// Exact effect reservation was absent.
    #[error("agent policy semantic effect reservation is missing")]
    EffectMissing,
    /// Independent effect assessment or prepared action did not exactly join.
    #[error("agent policy semantic effect assessment mismatch")]
    EffectMismatch,
    /// Native action-attempt identity was reused or regressed.
    #[error("agent policy semantic action attempt replay")]
    EffectAttemptReplay,
    /// Current lifecycle/control/freshness state cannot automate.
    #[error("agent policy context is not currently automatable")]
    ContextNotAutomatable,
    /// Prepared action target was not present in committed model input.
    #[error("agent policy action source was not delivered to the model")]
    ModelSourceMissing,
    /// Model context contains data from another browser profile.
    #[error("agent policy cross-profile model context is forbidden")]
    CrossProfileData,
    /// Effect permit, dispatch, or terminal proof did not exactly join.
    #[error("agent policy semantic effect settlement mismatch")]
    EffectSettlementMismatch,
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
    /// The read result's exact observation or one source reference was not committed.
    #[error("agent policy semantic read baseline is not committed")]
    ReadBaselineMissing,
    /// A semantic diff had no frame from which to prove context authority.
    #[error("agent policy empty diff has no source authority")]
    EmptyDiff,
    /// The diff's exact prior source cohort was not committed to this policy.
    #[error("agent policy semantic diff baseline is not committed")]
    DiffBaselineMissing,
    /// The locate result's exact observation or one returned reference was not committed.
    #[error("agent policy semantic locate baseline is not committed")]
    LocateBaselineMissing,
    /// Projected persistent model-context taint exceeded its hard ceiling.
    #[error("agent policy taint cohort ceiling reached")]
    TaintLimit,
    /// Projected opaque disclosed-reference inventory exceeded its hard ceiling.
    #[error("agent policy taint reference ceiling reached")]
    TaintReferenceLimit,
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
    let observation_id = observation.request().id();
    let observation_generation = observation.request().generation();
    let source_guard = SemanticObservationFingerprint::from_observation(observation).digest();
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
                observation: observation_id,
                observation_generation,
                source_guard,
                account: account.account(),
                origin: frame.frame().origin().clone(),
                sensitivity,
                trust: SemanticTrust::UntrustedPage,
                attested_at: account.observed_at(),
                references: canonical_references(
                    frame.nodes().iter().map(|node| node.reference()).collect(),
                ),
            },
        );
    }
    Ok(cohorts)
}

fn screenshot_taints(
    observation: &SemanticObservation,
    account: AgentContextAccountBinding,
    source_guard: [u8; 32],
) -> Result<Vec<AgentTaintCohort>, AgentPolicyError> {
    let context = observation.request().context();
    if account.context() != context || observation.frames().is_empty() {
        return Err(AgentPolicyError::Authority);
    }
    let mut cohorts = Vec::with_capacity(observation.frames().len());
    for frame in observation.frames() {
        if frame.frame().context() != context {
            return Err(AgentPolicyError::Authority);
        }
        merge_taint(
            &mut cohorts,
            AgentTaintCohort {
                context,
                observation: observation.request().id(),
                observation_generation: observation.request().generation(),
                source_guard,
                account: account.account(),
                origin: frame.frame().origin().clone(),
                // Pixels can disclose canvas, image, video, and cross-frame
                // information absent from the semantic projection. Known
                // secrets were rejected before capture, but public taint can
                // never be inferred from that negative check.
                sensitivity: SemanticSensitivity::Sensitive,
                trust: SemanticTrust::UntrustedPage,
                attested_at: account.observed_at(),
                // A screenshot is evidence, never opaque node authority.
                references: Vec::new(),
            },
        );
    }
    Ok(cohorts)
}

fn diff_taints(
    diff: &SemanticDiff,
    account: AgentContextAccountBinding,
    retained: &[AgentTaintCohort],
) -> Result<Vec<AgentTaintCohort>, AgentPolicyError> {
    let context = diff
        .frames()
        .first()
        .map(|frame| frame.frame().context())
        .ok_or(AgentPolicyError::EmptyDiff)?;
    if account.context() != context
        || diff
            .frames()
            .iter()
            .any(|frame| frame.frame().context() != context)
    {
        return Err(AgentPolicyError::Authority);
    }

    let mut origins = Vec::with_capacity(diff.frames().len());
    for frame in diff.frames() {
        if !origins.contains(frame.frame().origin()) {
            origins.push(frame.frame().origin().clone());
        }
    }

    let mut candidates = Vec::with_capacity(origins.len());
    for origin in origins {
        let baseline_matches = |cohort: &AgentTaintCohort| {
            cohort.context == context
                && cohort.observation == diff.previous_observation()
                && cohort.observation_generation == diff.previous_generation()
                && cohort.source_guard == diff.baseline_guard()
                && cohort.account == account.account()
                && cohort.origin == origin
        };
        if !retained.iter().any(baseline_matches) {
            return Err(AgentPolicyError::DiffBaselineMissing);
        }

        let mut sensitivity = retained
            .iter()
            .filter(|cohort| baseline_matches(cohort))
            .map(|cohort| cohort.sensitivity)
            .max()
            .unwrap_or(SemanticSensitivity::Public);
        let attested_at = retained
            .iter()
            .filter(|cohort| baseline_matches(cohort))
            .map(|cohort| cohort.attested_at)
            .min()
            .unwrap_or(account.observed_at())
            .min(account.observed_at());
        let mut references = canonical_references(
            retained
                .iter()
                .filter(|cohort| baseline_matches(cohort))
                .flat_map(|cohort| cohort.references.iter().copied())
                .collect(),
        );
        for entry in diff
            .entries()
            .iter()
            .filter(|entry| entry.frame().origin() == &origin)
        {
            if let Some(previous) = entry.previous_reference() {
                retire_taint_reference(&mut references, previous.reference())?;
            }
        }
        for rebase in diff
            .reference_rebases()
            .iter()
            .filter(|rebase| rebase.frame().origin() == &origin)
        {
            retire_taint_reference(&mut references, rebase.previous_reference().reference())?;
        }
        for entry in diff
            .entries()
            .iter()
            .filter(|entry| entry.frame().origin() == &origin)
        {
            if let Some(node) = entry.current_node() {
                sensitivity = sensitivity.max(match node.sensitivity() {
                    SemanticSensitivity::Secret => SemanticSensitivity::Sensitive,
                    other => other,
                });
            }
            if let Some(reference) = entry.current_reference() {
                insert_taint_reference(&mut references, reference);
            }
        }
        for rebase in diff
            .reference_rebases()
            .iter()
            .filter(|rebase| rebase.frame().origin() == &origin)
        {
            insert_taint_reference(&mut references, rebase.current_reference());
        }
        merge_taint(
            &mut candidates,
            AgentTaintCohort {
                context,
                observation: diff.current_observation(),
                observation_generation: diff.current_generation(),
                source_guard: diff.current_guard(),
                account: account.account(),
                origin,
                sensitivity,
                trust: SemanticTrust::UntrustedPage,
                attested_at,
                references: canonical_references(references),
            },
        );
    }
    Ok(candidates)
}

fn retire_taint_reference(
    references: &mut Vec<SemanticReferenceId>,
    retired: SemanticReferenceId,
) -> Result<(), AgentPolicyError> {
    let index = references
        .binary_search(&retired)
        .map_err(|_| AgentPolicyError::DiffBaselineMissing)?;
    references.remove(index);
    Ok(())
}

fn insert_taint_reference(references: &mut Vec<SemanticReferenceId>, current: SemanticReferenceId) {
    if let Err(index) = references.binary_search(&current) {
        references.insert(index, current);
    }
}

fn locate_taints(
    result: &SemanticLocateResult,
    account: AgentContextAccountBinding,
    retained: &[AgentTaintCohort],
) -> Result<Vec<AgentTaintCohort>, AgentPolicyError> {
    if account.context() != result.context() {
        return Err(AgentPolicyError::Authority);
    }
    let candidates = retained
        .iter()
        .filter(|cohort| {
            cohort.context == result.context()
                && cohort.observation == result.observation()
                && cohort.observation_generation == result.observation_generation()
                && cohort.source_guard == result.observation_guard()
                && cohort.account == account.account()
        })
        .cloned()
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(AgentPolicyError::LocateBaselineMissing);
    }
    for matched in result.matches() {
        let source_count = candidates
            .iter()
            .filter(|cohort| cohort.contains_reference(matched.reference()))
            .count();
        if source_count != 1 {
            return Err(AgentPolicyError::LocateBaselineMissing);
        }
    }
    Ok(candidates)
}

fn provider_read_taints(
    read: &SemanticReadResult<'_>,
    baseline: &SemanticObservationAcknowledgement,
    account: AgentContextAccountBinding,
    retained: &[AgentTaintCohort],
) -> Result<Vec<AgentTaintCohort>, AgentPolicyError> {
    if account.context() != read.context() || !read.matches_acknowledgement(baseline) {
        return Err(AgentPolicyError::Authority);
    }
    let candidates = retained
        .iter()
        .filter(|cohort| {
            cohort.context == baseline.context()
                && cohort.observation == baseline.observation()
                && cohort.observation_generation == baseline.generation()
                && cohort.source_guard == baseline.guard()
                && cohort.account == account.account()
        })
        .cloned()
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(AgentPolicyError::ReadBaselineMissing);
    }
    for fragment in read.fragments() {
        let provenance = fragment.provenance();
        let mut sources = candidates
            .iter()
            .filter(|cohort| cohort.contains_reference(provenance.reference()));
        let Some(source) = sources.next() else {
            return Err(AgentPolicyError::ReadBaselineMissing);
        };
        if sources.next().is_some() || source.origin() != provenance.origin() {
            return Err(AgentPolicyError::ReadBaselineMissing);
        }
    }
    Ok(candidates)
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
                observation: provenance.observation(),
                observation_generation: provenance.observation_generation(),
                source_guard: read.guard(),
                account: account.account(),
                origin: provenance.origin().clone(),
                sensitivity: provenance.sensitivity(),
                trust: provenance.trust(),
                attested_at: account.observed_at(),
                references: vec![provenance.reference()],
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

fn merge_references(
    left: &[SemanticReferenceId],
    right: &[SemanticReferenceId],
) -> Vec<SemanticReferenceId> {
    let mut merged = Vec::with_capacity(left.len().saturating_add(right.len()));
    let mut left_index = 0;
    let mut right_index = 0;
    while left_index < left.len() || right_index < right.len() {
        let candidate = match (left.get(left_index), right.get(right_index)) {
            (Some(left_value), Some(right_value)) if left_value < right_value => {
                left_index += 1;
                *left_value
            }
            (Some(left_value), Some(right_value)) if right_value < left_value => {
                right_index += 1;
                *right_value
            }
            (Some(left_value), Some(_)) => {
                left_index += 1;
                right_index += 1;
                *left_value
            }
            (Some(left_value), None) => {
                left_index += 1;
                *left_value
            }
            (None, Some(right_value)) => {
                right_index += 1;
                *right_value
            }
            (None, None) => break,
        };
        if merged.last() != Some(&candidate) {
            merged.push(candidate);
        }
    }
    merged
}

fn canonical_references(mut references: Vec<SemanticReferenceId>) -> Vec<SemanticReferenceId> {
    references.sort_unstable();
    references.dedup();
    references
}

fn projected_taint_usage(
    committed: &[AgentTaintCohort],
    calls: &[ModelCallRow],
    candidates: &[AgentTaintCohort],
) -> Result<(usize, usize), AgentPolicyError> {
    let mut projected = Vec::with_capacity(
        committed.len()
            + candidates.len()
            + calls
                .iter()
                .map(|call| call.candidates.len())
                .sum::<usize>(),
    );
    projected.extend_from_slice(committed);
    for cohort in calls
        .iter()
        .flat_map(|call| call.candidates.iter())
        .chain(candidates)
        .cloned()
    {
        merge_taint(&mut projected, cohort);
    }
    let references = projected.iter().try_fold(0_usize, |total, cohort| {
        total
            .checked_add(cohort.references.len())
            .ok_or(AgentPolicyError::TaintReferenceLimit)
    })?;
    Ok((projected.len(), references))
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
    effects: impl Iterator<Item = &'a AgentEffectRow>,
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
    for _effect in effects {
        value.reserved_operations = value.reserved_operations.saturating_add(1);
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
        && admission.input_token_limit == call.input_token_limit
        && admission.output_token_limit == call.output_token_limit
        && admission.cost_limit_micro_usd == call.cost_limit
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
        ModelInputKind::Diff => 2,
        ModelInputKind::Read => 3,
        ModelInputKind::Screenshot => 4,
        ModelInputKind::Locate => 5,
        ModelInputKind::Extraction => 6,
    }]);
    hash_context(&mut hasher, facts.context);
    hasher.update(facts.source_guard);
    hasher.update(facts.input_token_limit.to_be_bytes());
    hasher.update(facts.output_token_limit.to_be_bytes());
    hasher.update(facts.cost_limit.to_be_bytes());
    hasher.update((facts.candidates.len() as u64).to_be_bytes());
    for candidate in facts.candidates {
        hash_context(&mut hasher, candidate.context);
        hasher.update(candidate.observation.get().to_be_bytes());
        hasher.update(candidate.observation_generation.get().to_be_bytes());
        hasher.update(candidate.source_guard);
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
        hasher.update((candidate.references.len() as u64).to_be_bytes());
        for reference in &candidate.references {
            hasher.update(reference.get().to_be_bytes());
        }
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
    use crate::semantic_screenshot::admitted_test_screenshot;
    use crate::{
        compute_semantic_diff, decode_semantic_snapshot, encode_semantic_diff,
        encode_semantic_extraction_request, encode_semantic_locate_result,
        encode_semantic_observation, encode_semantic_read, locate_semantic_observation,
        read_semantic_observation, AgentAccountAttestationId, AgentAccountId, AgentDataFlowRule,
        AgentDelegationSpec, AgentDelegationTopology, AgentEffectScope, AgentPlanNodeAuthority,
        AgentPlanNodeScope, AgentPreparedObservationRequest, AgentPreparedReadRequest,
        AgentProviderCallConfig, AgentProviderContractError, AgentProviderDiffRequestDraft,
        AgentProviderEndpoint, AgentProviderExtractionRequestDraft, AgentProviderInputEvidence,
        AgentProviderInputKind, AgentProviderInputOutcome, AgentProviderKind,
        AgentProviderLocalInputTokenCounter, AgentProviderLocateRequestDraft,
        AgentProviderModelRevision, AgentProviderObjective,
        AgentProviderReadContinuationRequestDraft, AgentProviderReasoningEffort,
        AgentProviderRequestSettlement, AgentProviderScreenshotRequestDraft,
        AgentProviderSemanticInputStats, AgentProviderStreamBatch, AgentProviderStreamBudget,
        AgentProviderStreamConclusion, AgentProviderStreamEvent, AgentProviderTextDelta,
        AgentRunManifestId, AgentRunProviderInputMetrics, AgentRunScope, AgentRunSupervisor,
        AgentSupervisorId, ContextAutomationState, ContextCapabilities, ContextCapability,
        ContextId, ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, FrameId, SemanticActionBatch, SemanticActionBatchId,
        SemanticActionFailure, SemanticActionIntent, SemanticActionProposal,
        SemanticCaptureInstant, SemanticDecodeContext, SemanticDiffBudget, SemanticDiffOutcome,
        SemanticEffectEvidence, SemanticExtractionFieldSchema, SemanticExtractionSchema,
        SemanticExtractionSchemaId, SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId,
        SemanticLocateBudget, SemanticLocateId, SemanticLocateQuery, SemanticLocateRequest,
        SemanticLocateScope, SemanticModelDeliverySettlement, SemanticModelEncodingBudget,
        SemanticObservationAssembler, SemanticObservationBudget, SemanticObservationId,
        SemanticObservationRequest, SemanticPreparedAction, SemanticReadAuthority,
        SemanticReadBudget, SemanticReadSensitivityLimit, SemanticSettleBudget,
        SemanticSettleInstant, SemanticSnapshot, SemanticSnapshotGeneration, SemanticState,
        SemanticTokenCountQuality, SemanticTokenCountRequirement, SemanticTokenCounter,
        SemanticTokenCounterError, SemanticTokenMeasurement, SemanticTokenizerRevision,
        SemanticVerification, SemanticWaitCondition, SEMANTIC_WIRE_VERSION,
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

    fn make_context_registry(
        run: u128,
        profile_value: u128,
        context_value: u128,
    ) -> (ContextRegistry, ContextJoin) {
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
        let context = registry.join(identity.id()).expect("join");
        (registry, context)
    }

    fn make_context(run: u128, profile_value: u128, context_value: u128) -> ContextJoin {
        make_context_registry(run, profile_value, context_value).1
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

    fn actionable_observation(
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
                json!({"k": 2, "p": 0, "r": "button", "n": "Save draft", "o": 9,
                       "b": {"x": 10, "y": 10, "w": 100, "h": 30}}),
            ],
        )
    }

    fn multi_origin_observation(context: ContextJoin, observation_id: u64) -> SemanticObservation {
        fn frame_snapshot(
            context: ContextJoin,
            frame: FrameId,
            origin: SemanticOrigin,
            trust: SemanticFrameTrust,
            invocation: u64,
            nodes: Value,
        ) -> SemanticSnapshot {
            let frame_generation = if frame == FrameId::MAIN {
                context.frame_generation()
            } else {
                FrameGeneration::new(frame.get()).expect("frame generation")
            };
            let join = SemanticFrameJoin::try_new(context, frame, frame_generation, origin, trust)
                .expect("frame");
            let bytes = serde_json::to_vec(&json!({
                "v": SEMANTIC_WIRE_VERSION,
                "i": invocation,
                "g": invocation,
                "c": "complete",
                "n": nodes,
            }))
            .expect("wire");
            decode_semantic_snapshot(
                SemanticDecodeContext::new(
                    SemanticInvocationId::new(invocation).expect("invocation"),
                    join,
                    SemanticSnapshotGeneration::new(invocation).expect("snapshot"),
                ),
                &bytes,
            )
            .expect("snapshot")
        }

        let main = frame_snapshot(
            context,
            FrameId::MAIN,
            origin("visual-main"),
            SemanticFrameTrust::SameOrigin,
            observation_id + 10,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "frame_boundary"},
                {"k": 3, "p": 0, "r": "frame_boundary"}
            ]),
        );
        let first_boundary = main.nodes()[1].reference();
        let second_boundary = main.nodes()[2].reference();
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(observation_id).expect("observation"),
            context,
            SemanticObservationBudget::try_new(16, 8_192, 3).expect("budget"),
        );
        let mut assembler = SemanticObservationAssembler::new(request, main).expect("assembler");
        assembler
            .attach_frame(
                FrameId::MAIN,
                first_boundary,
                frame_snapshot(
                    context,
                    FrameId::new(2).expect("child frame"),
                    origin("visual-child-a"),
                    SemanticFrameTrust::CrossOriginIsolated,
                    observation_id + 20,
                    json!([{"k": 1, "r": "paragraph", "t": "child a"}]),
                ),
            )
            .expect("first child");
        assembler
            .attach_frame(
                FrameId::MAIN,
                second_boundary,
                frame_snapshot(
                    context,
                    FrameId::new(3).expect("child frame"),
                    origin("visual-child-b"),
                    SemanticFrameTrust::CrossOriginIsolated,
                    observation_id + 30,
                    json!([{"k": 1, "r": "paragraph", "t": "child b"}]),
                ),
            )
            .expect("second child");
        assembler.finish().expect("multi-origin observation")
    }

    fn multi_actionable_observation(
        context: ContextJoin,
        source: SemanticOrigin,
        observation_id: u64,
        actions: u16,
    ) -> SemanticObservation {
        let mut nodes = vec![json!({"k": 1, "r": "document", "o": 16})];
        for target in 2..=actions.saturating_add(1) {
            nodes.push(json!({
                "k": target,
                "p": 0,
                "r": "button",
                "n": format!("Action {target}"),
                "o": 9,
                "b": {"x": 10, "y": i64::from(target) * 40, "w": 100, "h": 30}
            }));
        }
        observation(context, source, observation_id, nodes)
    }

    fn read_limited_actionable_observation(
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
                json!({"k": 2, "p": 0, "r": "paragraph", "t": "first disclosed value"}),
                json!({"k": 3, "p": 0, "r": "button", "n": "Undisclosed target", "o": 9,
                       "b": {"x": 10, "y": 10, "w": 100, "h": 30}}),
            ],
        )
    }

    fn click_batch(
        observation: &SemanticObservation,
        target: u16,
        effect: SemanticEffectClass,
    ) -> SemanticActionBatch {
        click_batch_with_wait(
            observation,
            target,
            effect,
            SemanticWaitCondition::Immediate,
        )
    }

    fn click_batch_with_wait(
        observation: &SemanticObservation,
        target: u16,
        effect: SemanticEffectClass,
        wait: SemanticWaitCondition,
    ) -> SemanticActionBatch {
        let frames = observation
            .frames()
            .iter()
            .map(|snapshot| snapshot.frame().clone())
            .collect::<Vec<_>>();
        let proposal = SemanticActionProposal::try_new(
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(target).expect("target"),
            },
            effect,
            wait,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            SemanticSettleBudget::try_new(250).expect("settle budget"),
        )
        .expect("proposal");
        SemanticActionBatch::bind(
            SemanticActionBatchId::new(1).expect("batch"),
            observation,
            &frames,
            vec![proposal],
        )
        .expect("batch")
    }

    fn prepared_click(
        observation: &SemanticObservation,
        target: u16,
        effect: SemanticEffectClass,
    ) -> SemanticPreparedAction {
        let batch = click_batch(observation, target, effect);
        batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepared action")
    }

    fn post_action_snapshot(observation: &SemanticObservation) -> SemanticSnapshot {
        let previous = &observation.frames()[0];
        let generation = previous
            .generation()
            .next()
            .expect("post-action generation");
        let invocation = SemanticInvocationId::new(previous.invocation().get() + 1)
            .expect("post-action invocation");
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation.get(),
            "g": generation.get(),
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Save draft", "s": 64, "o": 9,
                 "b": {"x": 10, "y": 10, "w": 100, "h": 30}}
            ]
        }))
        .expect("post-action wire");
        decode_semantic_snapshot(
            SemanticDecodeContext::new(invocation, previous.frame().clone(), generation),
            &bytes,
        )
        .expect("post-action snapshot")
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

    struct FixedProviderInputCounter {
        revision: SemanticTokenizerRevision,
        tokens: u32,
        quality: SemanticTokenCountQuality,
    }

    impl FixedProviderInputCounter {
        fn measurement(
            &self,
            tokenizer: &SemanticTokenizerRevision,
            request_body: &[u8],
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if tokenizer != &self.revision || request_body.is_empty() {
                return Err(SemanticTokenCounterError::InvalidResult);
            }
            SemanticTokenMeasurement::try_new(self.revision.clone(), self.tokens, self.quality)
                .map_err(|_| SemanticTokenCounterError::InvalidResult)
        }
    }

    impl AgentProviderLocalInputTokenCounter for FixedProviderInputCounter {
        fn count_openai_responses_input(
            &self,
            model: &AgentProviderModelRevision,
            tokenizer: &SemanticTokenizerRevision,
            request_body: &[u8],
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if model.as_str() != "gpt-5.6-terra" {
                return Err(SemanticTokenCounterError::Unavailable);
            }
            self.measurement(tokenizer, request_body)
        }

        fn count_anthropic_messages_input(
            &self,
            _model: &AgentProviderModelRevision,
            _tokenizer: &SemanticTokenizerRevision,
            _request_body: &[u8],
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            Err(SemanticTokenCounterError::Unavailable)
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

    fn diff_between(
        previous: &SemanticObservation,
        current: &SemanticObservation,
    ) -> Box<SemanticDiff> {
        let acknowledgement = observation_payload(previous, 10)
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("baseline delivery");
        match compute_semantic_diff(
            previous,
            &acknowledgement,
            current,
            SemanticDiffBudget::ACTION,
        ) {
            SemanticDiffOutcome::Diff(diff) => diff,
            SemanticDiffOutcome::FreshSnapshot(reason) => {
                panic!("unexpected fresh snapshot: {reason:?}")
            }
        }
    }

    fn diff_payload(diff: &SemanticDiff, tokens: u32) -> SemanticDiffModelPayload {
        let revision = tokenizer();
        encode_semantic_diff(diff, SemanticModelEncodingBudget::ACTION_DIFF_EXACT)
            .expect("encode diff")
            .admit(
                &FixedCounter {
                    revision: revision.clone(),
                    tokens,
                },
                &revision,
            )
            .expect("diff payload")
    }

    fn effects(values: &[SemanticEffectClass]) -> AgentEffectScope {
        AgentEffectScope::try_new(values).expect("effects")
    }

    fn provider_config(
        tokenizer: SemanticTokenizerRevision,
        fixed_input_tokens: u32,
        max_output_tokens: u32,
    ) -> AgentProviderCallConfig {
        provider_config_with_pricing_range(tokenizer, fixed_input_tokens, max_output_tokens, 16_384)
    }

    fn provider_config_with_pricing_range(
        tokenizer: SemanticTokenizerRevision,
        fixed_input_tokens: u32,
        max_output_tokens: u32,
        max_priced_input_tokens: u64,
    ) -> AgentProviderCallConfig {
        AgentProviderCallConfig::try_new(
            AgentProviderKind::OpenAiResponses,
            AgentProviderModelRevision::try_new("gpt-5.6-terra".to_owned()).expect("model"),
            AgentProviderReasoningEffort::Medium,
            tokenizer,
            crate::AgentProviderPricingProfile::try_new(
                crate::AgentProviderPricingRevision::new(1).expect("pricing revision"),
                max_priced_input_tokens,
            )
            .expect("pricing profile"),
            fixed_input_tokens,
            max_output_tokens,
            AgentProviderStreamBudget::STANDARD,
        )
        .expect("provider config")
    }

    fn anthropic_provider_config(
        tokenizer: SemanticTokenizerRevision,
        fixed_input_tokens: u32,
        max_output_tokens: u32,
    ) -> AgentProviderCallConfig {
        AgentProviderCallConfig::try_new(
            AgentProviderKind::AnthropicMessages,
            AgentProviderModelRevision::try_new("claude-opus-5".to_owned()).expect("model"),
            AgentProviderReasoningEffort::None,
            tokenizer,
            crate::AgentProviderPricingProfile::try_new(
                crate::AgentProviderPricingRevision::new(1).expect("pricing revision"),
                16_384,
            )
            .expect("pricing profile"),
            fixed_input_tokens,
            max_output_tokens,
            AgentProviderStreamBudget::STANDARD,
        )
        .expect("provider config")
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

    fn policy_fixture_with_flows(
        run: u128,
        profile_value: u128,
        origins: Vec<SemanticOrigin>,
        data_flows: Vec<AgentDataFlowRule>,
        effect_values: &[SemanticEffectClass],
        budget: AgentRunBudget,
    ) -> PolicyFixture {
        let effect_scope = effects(effect_values);
        let scope = AgentRunScope::try_new(
            vec![profile(profile_value)],
            vec![AgentAccountScope::Anonymous],
            origins.clone(),
            SemanticSensitivity::Sensitive,
            effect_scope,
            data_flows,
        )
        .expect("scope");
        let authority = AgentPlanNodeAuthority::try_new(
            vec![profile(profile_value)],
            vec![AgentAccountScope::Anonymous],
            origins,
            SemanticSensitivity::Sensitive,
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

    fn commit_observation_to_model(
        policy: &mut AgentRunPolicy,
        lease: AgentPlanLeaseId,
        call_id: u64,
        binding: AgentContextAccountBinding,
        observation: &SemanticObservation,
    ) {
        let payload = observation_payload(observation, 10);
        let admission = policy
            .prepare_observation_input(
                call_request(call_id, lease, binding, 0, 0, 0, NOW),
                observation,
                &payload,
            )
            .expect("model admission");
        let delivery = payload
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("model delivery");
        let active = policy
            .commit_observation_input(admission, &delivery)
            .expect("model commit");
        policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 10, 0, 0)
            .expect("model settlement");
    }

    fn prepare_openai_screenshot_result(
        fixture: &mut PolicyFixture,
        binding: AgentContextAccountBinding,
        observation: &SemanticObservation,
        objective: &AgentProviderObjective,
        config: &AgentProviderCallConfig,
        visual: (u64, u8, u32),
    ) -> crate::AgentPreparedScreenshotRequest {
        let (screenshot_id, pixel, structured_tokens) = visual;
        let initial = AgentPreparedObservationRequest::try_openai(
            &mut fixture.policy,
            call_request(1, fixture.lease, binding, 15, 20, 100, NOW),
            observation,
            observation_payload(observation, 50),
            objective,
            config.clone(),
        )
        .expect("initial visual request");
        let committed = initial
            .into_transport_input()
            .commit(&mut fixture.policy)
            .expect("initial visual commit");
        let (initial_request, input, continuation) = committed.into_parts();
        let (active, evidence) = input.into_parts();
        let acknowledgement = evidence
            .observation_acknowledgement()
            .expect("visual baseline")
            .clone();
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 65, 4, 80)
            .expect("initial visual settlement");

        let tool = crate::AgentBrowserToolCall::decode_openai(
            initial_request.call(),
            "fc_policy_screenshot_1".to_owned(),
            "call_policy_screenshot_1".to_owned(),
            "screenshot",
            "{}".to_owned(),
        )
        .expect("screenshot tool");
        let completion = crate::AgentProviderCompletion::new(
            initial_request.call(),
            crate::AgentProviderStopReason::ToolCalls,
            crate::AgentProviderUsage::try_new(65, 4, 0, 0, 0).expect("usage"),
            crate::AgentProviderStreamStats::new(200, 8, 0, 1, 2),
            true,
        );
        let continuation = continuation
            .expect("visual continuation seed")
            .join_terminal_tool(completion, tool.into_continuation_parts().0)
            .expect("visual terminal join");
        let screenshot =
            admitted_test_screenshot(observation, &acknowledgement, screenshot_id, pixel);
        let visual_request = call_request(2, fixture.lease, binding, 500, 20, 100, NOW);
        let bound = continuation
            .bind_screenshot_request(visual_request, config, screenshot)
            .expect("bound screenshot result");
        AgentProviderScreenshotRequestDraft::try_new(bound)
            .expect("fixed screenshot draft")
            .try_prepare(
                &mut fixture.policy,
                visual_request,
                observation,
                &FixedProviderInputCounter {
                    revision: config.tokenizer().clone(),
                    tokens: structured_tokens,
                    quality: SemanticTokenCountQuality::ExactLocal,
                },
            )
            .expect("prepared screenshot result")
    }

    fn effect_request(
        id: u64,
        lease: AgentPlanLeaseId,
        binding: AgentContextAccountBinding,
        automation: ContextAutomationState,
    ) -> AgentEffectRequest {
        AgentEffectRequest::new(
            AgentEffectId::new(id).expect("effect"),
            lease,
            binding,
            automation,
            AgentPolicyInstant::from_millis(NOW),
        )
    }

    fn effect_dispatch_request(
        attempt: u64,
        binding: AgentContextAccountBinding,
        automation: ContextAutomationState,
    ) -> AgentEffectDispatchRequest {
        AgentEffectDispatchRequest::new(
            SemanticActionAttemptId::new(attempt).expect("attempt"),
            binding,
            automation,
            AgentPolicyInstant::from_millis(NOW),
        )
    }

    fn dispatch_local_effect(
        fixture: &mut PolicyFixture,
        action: &SemanticPreparedAction,
        destination: &SemanticOrigin,
        binding: AgentContextAccountBinding,
        automation: ContextAutomationState,
        effect_id: u64,
        attempt: u64,
    ) -> AgentActiveEffect {
        let assessment = AgentEffectAssessment::new(action, destination.clone(), action.effect());
        let permit = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(effect_id, fixture.lease, binding, automation),
                action,
                &assessment,
            )
            .expect("effect authorization")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("local effect needed human"),
        };
        fixture
            .policy
            .dispatch_semantic_effect(
                permit,
                action,
                effect_dispatch_request(attempt, binding, automation),
            )
            .expect("effect dispatch")
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
        assert_eq!(admission.input_token_limit(), 60);
        assert_eq!(admission.output_token_limit(), 20);
        assert_eq!(admission.cost_limit_micro_usd(), 100);
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
        assert_eq!(taint.observation().get(), 1);
        assert_eq!(taint.observation_generation().get(), 1);
        assert_eq!(taint.profile(), profile(8));
        assert_eq!(taint.account(), AgentAccountScope::Anonymous);
        assert_eq!(taint.sensitivity(), SemanticSensitivity::Sensitive);
        assert_eq!(taint.trust(), SemanticTrust::UntrustedPage);
        assert_eq!(
            taint.attested_at(),
            AgentPolicyInstant::from_millis(NOW - 1)
        );
        assert_eq!(taint.reference_count(), 4);
        assert!(taint.contains_reference(SemanticReferenceId::new(4).expect("reference")));
        let taint_debug = format!("{taint:?}");

        let receipt = fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 55, 10, 80)
            .expect("settle");
        assert_eq!(receipt.id().get(), 1);
        assert_eq!(receipt.lease(), fixture.lease);
        assert_eq!(receipt.settlement(), AgentModelCallSettlement::Completed);
        assert_eq!(receipt.usage_accounting(), AgentModelUsageAccounting::Exact);
        assert_eq!(receipt.pricing_attribution(), None);
        assert_eq!(receipt.input_tokens(), 55);
        assert_eq!(receipt.output_tokens(), 10);
        assert_eq!(receipt.cost_micro_usd(), 80);
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
    fn semantic_diff_delivery_transforms_exact_current_reference_authority() {
        let source = origin("diff-chain");
        let context = make_context(7_101, 7_102, 7_103);
        let previous = actionable_observation(context, source.clone(), 1);
        let current = observation(
            context,
            source.clone(),
            2,
            vec![
                json!({"k": 1, "r": "document", "o": 16}),
                json!({"k": 3, "p": 0, "r": "paragraph", "t": "new public marker"}),
                json!({"k": 2, "p": 0, "r": "button", "n": "Save draft", "o": 9,
                       "b": {"x": 10, "y": 10, "w": 100, "h": 30}}),
            ],
        );
        let diff = diff_between(&previous, &current);
        assert_eq!(diff.stats().added(), 1);
        assert_eq!(diff.reference_rebases().len(), 1);
        let mut fixture = policy_fixture(
            7_101,
            7_102,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let binding = account(context, NOW - 1);
        commit_observation_to_model(&mut fixture.policy, fixture.lease, 1, binding, &previous);

        let payload = diff_payload(&diff, 30);
        let admission = fixture
            .policy
            .prepare_diff_input(
                call_request(2, fixture.lease, binding, 5, 10, 80, NOW),
                &diff,
                &payload,
            )
            .expect("diff admission");
        assert_eq!(admission.input_token_limit(), 35);
        assert_eq!(fixture.policy.taints().len(), 1);
        let receipt = payload
            .settle_delivery_receipt(SemanticModelDeliverySettlement::Committed)
            .expect("diff delivery");
        assert!(receipt.acknowledgement().matches(&current));
        let active = fixture
            .policy
            .commit_diff_input(admission, &receipt)
            .expect("diff commit");

        assert_eq!(fixture.policy.taints().len(), 2);
        let current_taint = fixture
            .policy
            .taints()
            .iter()
            .find(|taint| taint.observation() == current.request().id())
            .expect("current taint");
        assert_eq!(current_taint.reference_count(), 3);
        for reference in 1..=3 {
            assert!(current_taint.contains_reference(
                SemanticReferenceId::new(reference).expect("current reference")
            ));
        }
        assert_eq!(
            current_taint.attested_at(),
            AgentPolicyInstant::from_millis(NOW - 1)
        );
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 31, 4, 70)
            .expect("diff settlement");
        assert_eq!(fixture.policy.pending_model_calls(), 0);
        assert_eq!(fixture.policy.accounting().consumed_operations(), 2);
        assert_eq!(fixture.policy.accounting().consumed_model_tokens(), 45);
    }

    #[test]
    fn semantic_diff_requires_the_exact_committed_baseline_and_payload_pair() {
        let source = origin("diff-baseline");
        let context = make_context(7_201, 7_202, 7_203);
        let previous = actionable_observation(context, source.clone(), 1);
        let alternate_previous = document_only_observation(context, source.clone(), 1);
        let current = actionable_observation(context, source.clone(), 2);
        let exact_diff = diff_between(&previous, &current);
        let alternate_diff = diff_between(&alternate_previous, &current);
        assert_ne!(exact_diff.guard(), alternate_diff.guard());
        let binding = account(context, NOW - 1);
        let mut fixture = policy_fixture(
            7_201,
            7_202,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );

        let payload = diff_payload(&exact_diff, 20);
        assert_eq!(
            fixture
                .policy
                .prepare_diff_input(
                    call_request(1, fixture.lease, binding, 0, 0, 0, NOW),
                    &exact_diff,
                    &payload,
                )
                .expect_err("missing baseline"),
            AgentPolicyError::DiffBaselineMissing
        );
        assert_eq!(fixture.policy.pending_model_calls(), 0);

        commit_observation_to_model(
            &mut fixture.policy,
            fixture.lease,
            1,
            binding,
            &alternate_previous,
        );
        let payload = diff_payload(&exact_diff, 20);
        assert_eq!(
            fixture
                .policy
                .prepare_diff_input(
                    call_request(2, fixture.lease, binding, 0, 0, 0, NOW),
                    &exact_diff,
                    &payload,
                )
                .expect_err("wrong exact baseline"),
            AgentPolicyError::DiffBaselineMissing
        );
        let alternate_payload = diff_payload(&alternate_diff, 20);
        assert_eq!(
            fixture
                .policy
                .prepare_diff_input(
                    call_request(2, fixture.lease, binding, 0, 0, 0, NOW),
                    &exact_diff,
                    &alternate_payload,
                )
                .expect_err("payload substitution"),
            AgentPolicyError::PayloadMismatch
        );
        assert_eq!(fixture.policy.pending_model_calls(), 0);
        assert!(!fixture.policy.is_sealed());
    }

    #[test]
    fn semantic_diff_delivery_receipt_substitution_seals_ambiguity() {
        let source = origin("diff-receipt");
        let context = make_context(7_301, 7_302, 7_303);
        let previous = actionable_observation(context, source.clone(), 1);
        let alternate_previous = document_only_observation(context, source.clone(), 1);
        let current = actionable_observation(context, source.clone(), 2);
        let exact_diff = diff_between(&previous, &current);
        let alternate_diff = diff_between(&alternate_previous, &current);
        let binding = account(context, NOW - 1);
        let mut fixture = policy_fixture(
            7_301,
            7_302,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        commit_observation_to_model(&mut fixture.policy, fixture.lease, 1, binding, &previous);
        let payload = diff_payload(&exact_diff, 20);
        let admission = fixture
            .policy
            .prepare_diff_input(
                call_request(2, fixture.lease, binding, 0, 0, 0, NOW),
                &exact_diff,
                &payload,
            )
            .expect("diff admission");
        let wrong_receipt = diff_payload(&alternate_diff, 20)
            .settle_delivery_receipt(SemanticModelDeliverySettlement::Committed)
            .expect("alternate receipt");
        assert_eq!(
            fixture
                .policy
                .commit_diff_input(admission, &wrong_receipt)
                .expect_err("receipt substitution"),
            AgentPolicyError::AdmissionMismatch
        );
        assert!(fixture.policy.is_sealed());
        assert_eq!(fixture.policy.pending_model_calls(), 1);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 1);
    }

    #[test]
    fn unknowable_provider_usage_charges_every_reserved_ceiling() {
        let source = origin("unaccounted-provider");
        let context = make_context(8_007, 8_008, 8_009);
        let observation = mixed_observation(context, source.clone(), 1);
        let payload = observation_payload(&observation, 50);
        let mut fixture = policy_fixture(
            8_007,
            8_008,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let admission = fixture
            .policy
            .prepare_observation_input(
                call_request(1, fixture.lease, account(context, NOW), 10, 20, 100, NOW),
                &observation,
                &payload,
            )
            .expect("admission");
        let acknowledgement = payload
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("delivery");
        let active = fixture
            .policy
            .commit_observation_input(admission, &acknowledgement)
            .expect("commit");

        let receipt = fixture
            .policy
            .settle_model_call_unaccounted(
                active,
                AgentModelCallUnaccountedSettlement::ProviderFailed,
            )
            .expect("conservative settlement");

        assert_eq!(
            receipt.settlement(),
            AgentModelCallSettlement::ProviderFailed
        );
        assert_eq!(
            receipt.usage_accounting(),
            AgentModelUsageAccounting::ReservationCeiling
        );
        assert_eq!(receipt.pricing_attribution(), None);
        assert_eq!(receipt.input_tokens(), 60);
        assert_eq!(receipt.output_tokens(), 20);
        assert_eq!(receipt.cost_micro_usd(), 100);
        assert_eq!(fixture.policy.pending_model_calls(), 0);
        assert_eq!(fixture.policy.accounting().consumed_operations(), 1);
        assert_eq!(fixture.policy.accounting().consumed_model_tokens(), 80);
        assert_eq!(fixture.policy.accounting().consumed_cost_micro_usd(), 100);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 0);
        assert!(!fixture.policy.is_sealed());
    }

    #[test]
    fn fixed_provider_request_is_atomic_bounded_and_one_shot() {
        let source = origin("provider-request");
        let context = make_context(9_007, 9_008, 9_009);
        let observation = mixed_observation(context, source.clone(), 1);
        let selected = tokenizer();
        let objective = AgentProviderObjective::try_admit(
            "Submit the reviewed form".to_owned(),
            &FixedCounter {
                revision: selected.clone(),
                tokens: 5,
            },
            &selected,
        )
        .expect("objective");

        let mut outside_pricing_range = policy_fixture(
            9_007,
            9_008,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        assert!(matches!(
            AgentPreparedObservationRequest::try_openai(
                &mut outside_pricing_range.policy,
                call_request(
                    1,
                    outside_pricing_range.lease,
                    account(context, NOW),
                    15,
                    20,
                    100,
                    NOW,
                ),
                &observation,
                observation_payload(&observation, 50),
                &objective,
                provider_config_with_pricing_range(selected.clone(), 10, 20, 64),
            ),
            Err(crate::AgentProviderRequestError::Contract(
                AgentProviderContractError::AdmissionBudget
            ))
        ));
        assert_eq!(outside_pricing_range.policy.pending_model_calls(), 0);

        let mut insufficient = policy_fixture(
            9_007,
            9_008,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let insufficient_payload = observation_payload(&observation, 50);
        assert!(matches!(
            AgentPreparedObservationRequest::try_openai(
                &mut insufficient.policy,
                call_request(
                    1,
                    insufficient.lease,
                    account(context, NOW),
                    15,
                    20,
                    100,
                    NOW,
                ),
                &observation,
                insufficient_payload,
                &objective,
                provider_config(selected.clone(), 11, 20),
            ),
            Err(crate::AgentProviderRequestError::Contract(
                AgentProviderContractError::AdmissionBudget
            ))
        ));
        assert_eq!(insufficient.policy.pending_model_calls(), 0);
        assert_eq!(insufficient.policy.accounting().reserved_operations(), 0);

        let mut fixture = policy_fixture(
            9_007,
            9_008,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let supervisor = AgentRunSupervisor::new(
            AgentSupervisorId::new(1).expect("supervisor"),
            AgentDelegationTopology::try_new(
                fixture.policy.manifest(),
                vec![AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None)],
            )
            .expect("topology"),
        );
        let mut input_reducer =
            AgentRunProviderInputMetrics::try_new(fixture.policy.manifest(), &supervisor)
                .expect("input metrics reducer");
        let prepared = AgentPreparedObservationRequest::try_openai(
            &mut fixture.policy,
            call_request(1, fixture.lease, account(context, NOW), 15, 20, 100, NOW),
            &observation,
            observation_payload(&observation, 50),
            &objective,
            provider_config(selected.clone(), 10, 20),
        )
        .expect("prepared request");
        assert_eq!(fixture.policy.pending_model_calls(), 1);
        assert_eq!(fixture.policy.accounting().reserved_model_tokens(), 85);
        assert_eq!(
            prepared.request().endpoint(),
            AgentProviderEndpoint::OpenAiResponses
        );
        assert!(prepared.request().byte_len() < crate::MAX_AGENT_PROVIDER_REQUEST_BYTES);
        let wire: Value = serde_json::from_slice(prepared.request().body()).expect("request JSON");
        assert_eq!(wire["model"], "gpt-5.6-terra");
        assert_eq!(wire["store"], false);
        assert_eq!(wire["stream"], true);
        assert_eq!(wire["parallel_tool_calls"], false);
        assert_eq!(wire["truncation"], "disabled");
        assert_eq!(wire["service_tier"], "default");
        assert_eq!(wire["reasoning"]["effort"], "medium");
        assert_eq!(
            wire["include"],
            serde_json::json!(["reasoning.encrypted_content"])
        );
        assert_eq!(wire.as_object().expect("request object").len(), 13);
        assert!(wire.get("previous_response_id").is_none());
        assert!(wire.get("metadata").is_none());
        assert_eq!(wire["input"].as_array().expect("input").len(), 2);
        assert_eq!(wire["tools"].as_array().expect("tools").len(), 13);
        assert!(wire["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .all(|tool| tool["type"] == "function" && tool["strict"] == true));
        let debug = format!("{prepared:?}");
        assert!(!debug.contains("Submit the reviewed form"));
        assert!(!debug.contains("private marker"));

        let semantic_stats = prepared.semantic_stats();
        let serialized_request_bytes = prepared.request().byte_len() as u32;
        let transport_input = prepared.into_transport_input();
        assert_eq!(transport_input.request().call().call().get(), 1);
        let transcript_bytes = transport_input
            .continuation_transcript_bytes()
            .expect("normal observation transcript");
        assert!(transcript_bytes > objective.byte_len());
        assert!(
            transcript_bytes
                <= crate::MAX_AGENT_PROVIDER_OBJECTIVE_BYTES
                    + crate::MAX_AGENT_PROVIDER_CONTINUATION_INITIAL_OBSERVATION_BYTES
        );
        let transport_debug = format!("{transport_input:?}");
        assert!(!transport_debug.contains("Submit the reviewed form"));
        assert!(!transport_debug.contains("private marker"));
        let committed = transport_input
            .commit(&mut fixture.policy)
            .expect("transport commit");
        let input_metrics = committed.input_metrics();
        let metric_receipt = committed.input_metric_receipt();
        assert_eq!(metric_receipt.manifest(), committed.active().manifest());
        assert_eq!(metric_receipt.call(), committed.active().id());
        assert_eq!(metric_receipt.lease(), committed.active().lease());
        assert_eq!(metric_receipt.node(), committed.active().node());
        assert_eq!(metric_receipt.metrics(), input_metrics);
        input_reducer
            .record(metric_receipt)
            .expect("committed input metrics");
        let input_snapshot = input_reducer.snapshot();
        assert_eq!(input_snapshot.calls(), 1);
        assert_eq!(
            input_snapshot
                .kind(AgentProviderInputKind::Observation)
                .serialized_request_bytes(),
            u64::from(serialized_request_bytes)
        );
        assert_eq!(
            input_snapshot.shapes().observation_nodes(),
            u64::from(semantic_stats.nodes())
        );
        assert_eq!(
            input_metrics.serialized_request_bytes(),
            serialized_request_bytes
        );
        assert_eq!(
            input_metrics.semantic(),
            AgentProviderSemanticInputStats::Observation(semantic_stats)
        );
        assert_eq!(
            input_metrics.semantic().disclosed_bytes(),
            semantic_stats.bytes()
        );
        assert_eq!(
            input_metrics
                .semantic_payload_tokens()
                .expect("semantic token count")
                .tokens(),
            50
        );
        assert_eq!(
            input_metrics
                .semantic_payload_tokens()
                .expect("semantic token count")
                .quality(),
            SemanticTokenCountQuality::ExactLocal
        );
        assert_eq!(input_metrics.structured_input_tokens(), None);
        let metric_debug = format!("{metric_receipt:?}");
        assert!(metric_debug.contains("[redacted]"));
        assert!(!metric_debug.contains("Submit the reviewed form"));
        assert!(!metric_debug.contains("private marker"));
        assert_eq!(
            committed.continuation_transcript_bytes(),
            Some(transcript_bytes)
        );
        assert!(committed
            .request()
            .call()
            .matches_active(committed.active()));
        let AgentProviderInputEvidence::Observation(acknowledgement) = committed.input_evidence()
        else {
            panic!("observation evidence")
        };
        assert!(acknowledgement.matches(&observation));
        let (request, input, continuation) = committed.into_parts();
        assert!(continuation.is_some());
        let (active, evidence) = input.into_parts();
        assert!(matches!(
            evidence,
            AgentProviderInputEvidence::Observation(_)
        ));
        assert_eq!(request.endpoint(), AgentProviderEndpoint::OpenAiResponses);
        assert_eq!(fixture.policy.taints().len(), 1);
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 65, 5, 80)
            .expect("provider settlement");

        let mut refused = policy_fixture(
            9_007,
            9_008,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let prepared = AgentPreparedObservationRequest::try_openai(
            &mut refused.policy,
            call_request(1, refused.lease, account(context, NOW), 15, 20, 100, NOW),
            &observation,
            observation_payload(&observation, 50),
            &objective,
            provider_config(selected, 10, 20),
        )
        .expect("prepared request");
        assert!(matches!(
            prepared
                .into_transport_input()
                .refuse(&mut refused.policy)
                .expect("refusal"),
            AgentProviderInputOutcome::Refused
        ));
        assert_eq!(refused.policy.pending_model_calls(), 0);
        assert!(refused.policy.taints().is_empty());
    }

    #[test]
    fn stateless_diff_counts_whole_input_before_exact_policy_and_transport_commit() {
        let source = origin("provider-diff-request");
        let context = make_context(9_207, 9_208, 9_209);
        let previous = actionable_observation(context, source.clone(), 1);
        let current = observation(
            context,
            source.clone(),
            2,
            vec![
                json!({"k": 1, "r": "document", "o": 16}),
                json!({"k": 3, "p": 0, "r": "paragraph", "t": "new private marker"}),
                json!({"k": 2, "p": 0, "r": "button", "n": "Save draft", "o": 9,
                       "b": {"x": 10, "y": 10, "w": 100, "h": 30}}),
            ],
        );
        let diff = diff_between(&previous, &current);
        let selected = tokenizer();
        let config = provider_config(selected.clone(), 10, 20);
        let objective = AgentProviderObjective::try_admit(
            "Review the private marker".to_owned(),
            &FixedCounter {
                revision: selected.clone(),
                tokens: 5,
            },
            &selected,
        )
        .expect("objective");
        let mut fixture = policy_fixture(
            9_207,
            9_208,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 5_000, 10_000),
        );
        let binding = account(context, NOW - 1);
        let prepared = AgentPreparedObservationRequest::try_openai(
            &mut fixture.policy,
            call_request(1, fixture.lease, binding, 15, 20, 100, NOW),
            &previous,
            observation_payload(&previous, 50),
            &objective,
            config.clone(),
        )
        .expect("initial request");
        let committed = prepared
            .into_transport_input()
            .commit(&mut fixture.policy)
            .expect("initial commit");
        let (initial_request, input, continuation) = committed.into_parts();
        let (active, evidence) = input.into_parts();
        assert!(evidence
            .observation_acknowledgement()
            .is_some_and(|acknowledgement| acknowledgement.matches(&previous)));
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 65, 4, 80)
            .expect("initial settlement");
        let tool = crate::AgentBrowserToolCall::decode_openai(
            initial_request.call(),
            "fc_diff_request_1".to_owned(),
            "call_diff_request_1".to_owned(),
            "back",
            "{}".to_owned(),
        )
        .expect("tool call");
        let correlation = tool.into_continuation_parts().0;
        let completion = crate::AgentProviderCompletion::new(
            initial_request.call(),
            crate::AgentProviderStopReason::ToolCalls,
            crate::AgentProviderUsage::try_new(65, 4, 0, 0, 0).expect("usage"),
            crate::AgentProviderStreamStats::new(200, 8, 0, 1, 2),
            true,
        );
        let continuation = continuation
            .expect("continuation seed")
            .join_terminal_tool(completion, correlation)
            .expect("terminal tool join");

        let diff_request = call_request(2, fixture.lease, binding, 500, 20, 100, NOW);
        let diff_measurement = SemanticTokenMeasurement::try_new(
            selected.clone(),
            30,
            SemanticTokenCountQuality::ExactLocal,
        )
        .expect("diff measurement");
        let provider_preflight = SemanticTokenMeasurement::try_new(
            selected.clone(),
            120,
            SemanticTokenCountQuality::ProviderExact,
        )
        .expect("provider preflight");
        assert_eq!(
            config
                .validate_diff_request(diff_request, &diff_measurement, &provider_preflight)
                .expect_err("provider-backed count must need separate disclosure authority"),
            AgentProviderContractError::InputTokenQuality
        );
        let oversized = SemanticTokenMeasurement::try_new(
            selected.clone(),
            531,
            SemanticTokenCountQuality::ExactLocal,
        )
        .expect("oversized measurement");
        assert_eq!(
            config
                .validate_diff_request(diff_request, &diff_measurement, &oversized)
                .expect_err("whole input exceeds authorized replay ceiling"),
            AgentProviderContractError::AdmissionBudget
        );
        assert_eq!(fixture.policy.pending_model_calls(), 0);
        let bound = continuation
            .bind_diff_request(diff_request, &config, &diff, diff_payload(&diff, 30))
            .expect("bound diff request");
        let draft = AgentProviderDiffRequestDraft::try_new(bound).expect("fixed diff draft");
        assert_eq!(fixture.policy.pending_model_calls(), 0);
        let prepared = draft
            .try_prepare(
                &mut fixture.policy,
                diff_request,
                &diff,
                &FixedProviderInputCounter {
                    revision: selected,
                    tokens: 120,
                    quality: SemanticTokenCountQuality::ExactLocal,
                },
            )
            .expect("whole-input admission");
        assert_eq!(prepared.structured_input_measurement().tokens(), 120);
        assert_eq!(
            prepared.structured_input_measurement().quality(),
            SemanticTokenCountQuality::ExactLocal
        );
        assert_eq!(fixture.policy.pending_model_calls(), 1);
        assert_eq!(fixture.policy.accounting().reserved_model_tokens(), 140);
        let transcript_bytes = prepared.continuation_transcript_bytes();
        let semantic_stats = prepared.semantic_stats();
        let serialized_request_bytes = prepared.request().byte_len() as u32;
        let debug = format!("{prepared:?}");
        assert!(!debug.contains("private marker"));
        assert!(!debug.contains("call_diff_request_1"));

        let committed = prepared
            .into_transport_input()
            .commit(&mut fixture.policy)
            .expect("diff transport commit");
        let input_metrics = committed.input_metrics();
        assert_eq!(
            input_metrics.serialized_request_bytes(),
            serialized_request_bytes
        );
        assert_eq!(
            input_metrics.semantic(),
            AgentProviderSemanticInputStats::Diff(semantic_stats)
        );
        assert_eq!(
            input_metrics
                .semantic_payload_tokens()
                .expect("diff token count")
                .tokens(),
            30
        );
        assert_eq!(
            input_metrics
                .structured_input_tokens()
                .expect("structured token count")
                .tokens(),
            120
        );
        assert_eq!(
            committed.continuation_transcript_bytes(),
            Some(transcript_bytes)
        );
        assert!(committed
            .input_evidence()
            .diff_receipt()
            .is_some_and(|receipt| receipt.acknowledgement().matches(&current)));
        assert!(committed
            .input_evidence()
            .observation_acknowledgement()
            .is_some_and(|acknowledgement| acknowledgement.matches(&current)));
        assert!(committed.input_evidence().read_receipt().is_none());
        let (request, input, continuation) = committed.into_parts();
        assert_eq!(request.endpoint(), AgentProviderEndpoint::OpenAiResponses);
        let second_call = request.call();
        let continuation = continuation.expect("diff continuation seed");
        let (active, evidence) = input.into_parts();
        assert!(matches!(evidence, AgentProviderInputEvidence::Diff(_)));
        assert_eq!(fixture.policy.taints().len(), 2);
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 120, 4, 80)
            .expect("diff settlement");
        assert_eq!(fixture.policy.pending_model_calls(), 0);

        let next = observation(
            context,
            origin("provider-diff-request"),
            3,
            vec![
                json!({"k": 1, "r": "document", "o": 16}),
                json!({"k": 3, "p": 0, "r": "paragraph", "t": "latest private marker"}),
                json!({"k": 2, "p": 0, "r": "button", "n": "Save draft", "o": 9,
                       "b": {"x": 10, "y": 10, "w": 100, "h": 30}}),
            ],
        );
        let next_diff = diff_between(&current, &next);
        let tool = crate::AgentBrowserToolCall::decode_openai(
            second_call,
            "fc_diff_request_2".to_owned(),
            "call_diff_request_2".to_owned(),
            "reload",
            "{}".to_owned(),
        )
        .expect("second tool call");
        let completion = crate::AgentProviderCompletion::new(
            second_call,
            crate::AgentProviderStopReason::ToolCalls,
            crate::AgentProviderUsage::try_new(120, 4, 0, 0, 0).expect("usage"),
            crate::AgentProviderStreamStats::new(200, 8, 0, 1, 2),
            true,
        );
        let continuation = continuation
            .join_terminal_tool(completion, tool.into_continuation_parts().0)
            .expect("second terminal tool join");
        let third_request = call_request(3, fixture.lease, binding, 500, 20, 100, NOW);
        let draft = AgentProviderDiffRequestDraft::try_new(
            continuation
                .bind_diff_request(
                    third_request,
                    &config,
                    &next_diff,
                    diff_payload(&next_diff, 30),
                )
                .expect("second bound diff"),
        )
        .expect("second fixed draft");
        let wire: Value =
            serde_json::from_slice(draft.request().body()).expect("second draft JSON");
        assert_eq!(wire["input"].as_array().expect("replay inputs").len(), 6);
        let prepared = draft
            .try_prepare(
                &mut fixture.policy,
                third_request,
                &next_diff,
                &FixedProviderInputCounter {
                    revision: tokenizer(),
                    tokens: 160,
                    quality: SemanticTokenCountQuality::ExactLocal,
                },
            )
            .expect("second whole-input admission");
        assert_eq!(fixture.policy.accounting().reserved_model_tokens(), 180);
        assert!(matches!(
            prepared
                .settle(&mut fixture.policy, AgentProviderRequestSettlement::Refused)
                .expect("diff refusal"),
            AgentProviderInputOutcome::Refused
        ));
        assert_eq!(fixture.policy.pending_model_calls(), 0);
        assert_eq!(fixture.policy.taints().len(), 2);
    }

    #[test]
    fn locate_result_rejoins_committed_baseline_without_widening_taint() {
        let source = origin("provider-locate-request");
        let context = make_context(9_237, 9_238, 9_239);
        let observation = actionable_observation(context, source.clone(), 1);
        let selected = tokenizer();
        let config = provider_config(selected.clone(), 10, 20);
        let objective = AgentProviderObjective::try_admit(
            "Find the save control".to_owned(),
            &FixedCounter {
                revision: selected.clone(),
                tokens: 5,
            },
            &selected,
        )
        .expect("objective");
        let mut fixture = policy_fixture(
            9_237,
            9_238,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 5_000, 10_000),
        );
        let binding = account(context, NOW - 1);
        let committed = AgentPreparedObservationRequest::try_openai(
            &mut fixture.policy,
            call_request(1, fixture.lease, binding, 15, 20, 100, NOW),
            &observation,
            observation_payload(&observation, 50),
            &objective,
            config.clone(),
        )
        .expect("initial request")
        .into_transport_input()
        .commit(&mut fixture.policy)
        .expect("initial commit");
        let (initial_request, input, continuation) = committed.into_parts();
        let (active, evidence) = input.into_parts();
        let baseline = evidence
            .observation_acknowledgement()
            .expect("observation baseline")
            .clone();
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 65, 4, 80)
            .expect("initial settlement");
        assert_eq!(fixture.policy.taints().len(), 1);
        let initial_reference_count = fixture.policy.taints()[0].reference_count();

        let arguments = r#"{"semantic_query":"save draft","scope":{"kind":"initial"}}"#;
        let tool = crate::AgentBrowserToolCall::decode_openai(
            initial_request.call(),
            "fc_locate_request_1".to_owned(),
            "call_locate_request_1".to_owned(),
            "locate",
            arguments.to_owned(),
        )
        .expect("locate tool");
        let completion = crate::AgentProviderCompletion::new(
            initial_request.call(),
            crate::AgentProviderStopReason::ToolCalls,
            crate::AgentProviderUsage::try_new(65, 4, 0, 0, 0).expect("usage"),
            crate::AgentProviderStreamStats::new(
                200,
                8,
                0,
                1,
                u32::try_from(arguments.len()).expect("argument bytes"),
            ),
            true,
        );
        let continuation = continuation
            .expect("continuation seed")
            .join_terminal_tool(completion, tool.into_continuation_parts().0)
            .expect("locate terminal join");
        let frames = observation
            .frames()
            .iter()
            .map(|frame| frame.frame().clone())
            .collect::<Vec<_>>();
        let locate_request = SemanticLocateRequest::bind(
            SemanticLocateId::new(1).expect("locate id"),
            &observation,
            &baseline,
            &frames,
            SemanticLocateQuery::try_new("save draft".to_owned()).expect("query"),
            SemanticLocateScope::Initial,
            SemanticLocateBudget::STANDARD,
        )
        .expect("locate bind");
        let result = locate_semantic_observation(&observation, locate_request).expect("locate");
        assert_eq!(result.matches().len(), 1);
        let payload = encode_semantic_locate_result(
            &result,
            SemanticModelEncodingBudget::LOCATE_RESULT_EXACT,
        )
        .expect("encode locate")
        .admit(
            &FixedCounter {
                revision: selected.clone(),
                tokens: 30,
            },
            &selected,
        )
        .expect("admit locate");
        let next_request = call_request(2, fixture.lease, binding, 500, 20, 100, NOW);
        let provider_counted_result = SemanticTokenMeasurement::try_new(
            selected.clone(),
            30,
            SemanticTokenCountQuality::ProviderExact,
        )
        .expect("provider locate measurement");
        let local_structured = SemanticTokenMeasurement::try_new(
            selected.clone(),
            120,
            SemanticTokenCountQuality::ExactLocal,
        )
        .expect("local structured measurement");
        assert_eq!(
            config
                .validate_locate_request(next_request, &provider_counted_result, &local_structured,)
                .expect_err("locate result itself must be counted locally"),
            AgentProviderContractError::InputTokenQuality
        );
        let draft = AgentProviderLocateRequestDraft::try_new(
            continuation
                .bind_locate_request(next_request, &config, &result, payload)
                .expect("bind locate result"),
        )
        .expect("fixed locate draft");
        let prepared = draft
            .try_prepare(
                &mut fixture.policy,
                next_request,
                &result,
                &FixedProviderInputCounter {
                    revision: selected,
                    tokens: 120,
                    quality: SemanticTokenCountQuality::ExactLocal,
                },
            )
            .expect("locate whole-input admission");
        assert_eq!(prepared.structured_input_measurement().tokens(), 120);
        assert_eq!(fixture.policy.accounting().reserved_model_tokens(), 140);
        let transcript_bytes = prepared.continuation_transcript_bytes();
        let semantic_stats = prepared.semantic_stats();
        let committed = prepared
            .into_transport_input()
            .commit(&mut fixture.policy)
            .expect("locate transport commit");
        let input_metrics = committed.input_metrics();
        assert_eq!(
            input_metrics.semantic(),
            AgentProviderSemanticInputStats::Locate(semantic_stats)
        );
        assert_eq!(
            input_metrics
                .semantic_payload_tokens()
                .expect("locate token count")
                .tokens(),
            30
        );
        assert_eq!(
            input_metrics
                .structured_input_tokens()
                .expect("structured token count")
                .tokens(),
            120
        );
        assert_eq!(
            committed.continuation_transcript_bytes(),
            Some(transcript_bytes)
        );
        assert!(committed
            .input_evidence()
            .locate_receipt()
            .is_some_and(|receipt| receipt.matches_result(&result)));
        assert!(committed
            .input_evidence()
            .observation_acknowledgement()
            .is_some_and(|acknowledgement| acknowledgement.matches(&observation)));
        assert_eq!(fixture.policy.taints().len(), 1);
        assert_eq!(
            fixture.policy.taints()[0].reference_count(),
            initial_reference_count
        );
        let (_, input, continuation) = committed.into_parts();
        assert!(continuation.is_some());
        let (active, evidence) = input.into_parts();
        assert!(matches!(evidence, AgentProviderInputEvidence::Locate(_)));
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 120, 4, 80)
            .expect("locate settlement");
    }

    #[test]
    fn read_result_rejoins_exact_baseline_and_retains_continuation_without_taint_growth() {
        let source = origin("provider-read-request");
        let context = make_context(9_247, 9_248, 9_249);
        let observation = actionable_observation(context, source.clone(), 1);
        let selected = tokenizer();
        let config = provider_config(selected.clone(), 10, 20);
        let objective = AgentProviderObjective::try_admit(
            "Read the save control".to_owned(),
            &FixedCounter {
                revision: selected.clone(),
                tokens: 5,
            },
            &selected,
        )
        .expect("objective");
        let mut fixture = policy_fixture(
            9_247,
            9_248,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 5_000, 10_000),
        );
        let binding = account(context, NOW - 1);
        let committed = AgentPreparedObservationRequest::try_openai(
            &mut fixture.policy,
            call_request(1, fixture.lease, binding, 15, 20, 100, NOW),
            &observation,
            observation_payload(&observation, 50),
            &objective,
            config.clone(),
        )
        .expect("initial request")
        .into_transport_input()
        .commit(&mut fixture.policy)
        .expect("initial commit");
        let (initial_request, input, continuation) = committed.into_parts();
        let (active, evidence) = input.into_parts();
        let baseline = evidence
            .observation_acknowledgement()
            .expect("observation baseline")
            .clone();
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 65, 4, 80)
            .expect("initial settlement");
        assert_eq!(fixture.policy.taints().len(), 1);
        let initial_reference_count = fixture.policy.taints()[0].reference_count();

        let arguments = r#"{"scope":{"kind":"initial"}}"#;
        let tool = crate::AgentBrowserToolCall::decode_openai(
            initial_request.call(),
            "fc_read_request_1".to_owned(),
            "call_read_request_1".to_owned(),
            "read",
            arguments.to_owned(),
        )
        .expect("read tool");
        let completion = crate::AgentProviderCompletion::new(
            initial_request.call(),
            crate::AgentProviderStopReason::ToolCalls,
            crate::AgentProviderUsage::try_new(65, 4, 0, 0, 0).expect("usage"),
            crate::AgentProviderStreamStats::new(
                200,
                8,
                0,
                1,
                u32::try_from(arguments.len()).expect("argument bytes"),
            ),
            true,
        );
        let continuation = continuation
            .expect("continuation seed")
            .join_terminal_tool(completion, tool.into_continuation_parts().0)
            .expect("read terminal join");
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(NOW - 2),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read result");
        assert!(read.matches_acknowledgement(&baseline));
        let payload = encode_semantic_read(
            &read,
            SemanticModelEncodingBudget::try_new(
                16 * 1024,
                16 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("read encoding budget"),
        )
        .expect("encode read")
        .admit(
            &FixedCounter {
                revision: selected.clone(),
                tokens: 30,
            },
            &selected,
        )
        .expect("admit read");
        let next_request = call_request(2, fixture.lease, binding, 500, 20, 100, NOW);
        let provider_counted_result = SemanticTokenMeasurement::try_new(
            selected.clone(),
            30,
            SemanticTokenCountQuality::ProviderExact,
        )
        .expect("provider read measurement");
        let local_structured = SemanticTokenMeasurement::try_new(
            selected.clone(),
            120,
            SemanticTokenCountQuality::ExactLocal,
        )
        .expect("local structured measurement");
        assert_eq!(
            config
                .validate_read_continuation_request(
                    next_request,
                    &provider_counted_result,
                    &local_structured,
                )
                .expect_err("read result itself must be counted locally"),
            AgentProviderContractError::InputTokenQuality
        );
        let draft = AgentProviderReadContinuationRequestDraft::try_new(
            continuation
                .bind_read_request(next_request, &config, &read, payload)
                .expect("bind read result"),
        )
        .expect("fixed read draft");
        let prepared = draft
            .try_prepare(
                &mut fixture.policy,
                next_request,
                &read,
                &FixedProviderInputCounter {
                    revision: selected,
                    tokens: 120,
                    quality: SemanticTokenCountQuality::ExactLocal,
                },
            )
            .expect("read whole-input admission");
        assert_eq!(prepared.structured_input_measurement().tokens(), 120);
        assert_eq!(fixture.policy.accounting().reserved_model_tokens(), 140);
        let transcript_bytes = prepared.continuation_transcript_bytes();
        let semantic_stats = prepared.semantic_stats();
        let committed = prepared
            .into_transport_input()
            .commit(&mut fixture.policy)
            .expect("read transport commit");
        let input_metrics = committed.input_metrics();
        assert_eq!(
            input_metrics.semantic(),
            AgentProviderSemanticInputStats::Read(semantic_stats)
        );
        assert_eq!(
            input_metrics
                .semantic_payload_tokens()
                .expect("read token count")
                .tokens(),
            30
        );
        assert_eq!(
            input_metrics
                .structured_input_tokens()
                .expect("structured token count")
                .tokens(),
            120
        );
        assert_eq!(
            committed.continuation_transcript_bytes(),
            Some(transcript_bytes)
        );
        assert!(committed
            .input_evidence()
            .read_receipt()
            .is_some_and(|receipt| receipt.matches_read(&read)));
        assert!(
            committed
                .input_evidence()
                .observation_acknowledgement()
                .is_none(),
            "read evidence must not mint full-observation authority"
        );
        assert_eq!(fixture.policy.taints().len(), 1);
        assert_eq!(
            fixture.policy.taints()[0].reference_count(),
            initial_reference_count
        );
        let (read_request, input, continuation) = committed.into_parts();
        let (active, evidence) = input.into_parts();
        assert!(matches!(evidence, AgentProviderInputEvidence::Read(_)));
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 120, 4, 80)
            .expect("read settlement");

        let tool = crate::AgentBrowserToolCall::decode_openai(
            read_request.call(),
            "fc_after_read_1".to_owned(),
            "call_after_read_1".to_owned(),
            "back",
            "{}".to_owned(),
        )
        .expect("post-read tool");
        let completion = crate::AgentProviderCompletion::new(
            read_request.call(),
            crate::AgentProviderStopReason::ToolCalls,
            crate::AgentProviderUsage::try_new(120, 4, 0, 0, 0).expect("usage"),
            crate::AgentProviderStreamStats::new(200, 8, 0, 1, 2),
            true,
        );
        let next_continuation = continuation
            .expect("read retains baseline for continuation")
            .join_terminal_tool(completion, tool.into_continuation_parts().0)
            .expect("post-read terminal join");
        assert!(next_continuation.baseline().matches(&observation));
    }

    #[test]
    fn extraction_mapping_commits_exact_input_and_admits_only_its_bound_output() {
        let source = origin("provider-extraction-request");
        let context = make_context(9_271, 9_272, 9_273);
        let observation = actionable_observation(context, source.clone(), 1);
        let selected = tokenizer();
        let config = provider_config(selected.clone(), 10, 20);
        let objective = AgentProviderObjective::try_admit(
            "Extract the save-control label".to_owned(),
            &FixedCounter {
                revision: selected.clone(),
                tokens: 5,
            },
            &selected,
        )
        .expect("objective");
        let mut fixture = policy_fixture(
            9_271,
            9_272,
            source,
            SemanticSensitivity::Public,
            &[SemanticEffectClass::Read],
            run_budget(10, 5_000, 10_000),
        );
        let account_binding = account(context, NOW - 1);
        let committed = AgentPreparedObservationRequest::try_openai(
            &mut fixture.policy,
            call_request(1, fixture.lease, account_binding, 15, 20, 100, NOW),
            &observation,
            observation_payload(&observation, 50),
            &objective,
            config.clone(),
        )
        .expect("initial request")
        .into_transport_input()
        .commit(&mut fixture.policy)
        .expect("initial commit");
        let (initial_request, input, continuation) = committed.into_parts();
        let (active, evidence) = input.into_parts();
        let baseline = evidence
            .observation_acknowledgement()
            .expect("observation baseline")
            .clone();
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 65, 4, 80)
            .expect("initial settlement");
        let initial_reference_count = fixture.policy.taints()[0].reference_count();

        let arguments = r#"{"schema_id":71}"#;
        let tool = crate::AgentBrowserToolCall::decode_openai(
            initial_request.call(),
            "fc_extract_request_1".to_owned(),
            "call_extract_request_1".to_owned(),
            "extract",
            arguments.to_owned(),
        )
        .expect("extract tool");
        let completion = crate::AgentProviderCompletion::new(
            initial_request.call(),
            crate::AgentProviderStopReason::ToolCalls,
            crate::AgentProviderUsage::try_new(65, 4, 0, 0, 0).expect("usage"),
            crate::AgentProviderStreamStats::new(
                200,
                8,
                0,
                1,
                u32::try_from(arguments.len()).expect("argument bytes"),
            ),
            true,
        );
        let continuation = continuation
            .expect("continuation seed")
            .join_terminal_tool(completion, tool.into_continuation_parts().0)
            .expect("extract terminal join");
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(NOW - 2),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .expect("read result");
        assert!(read.matches_acknowledgement(&baseline));
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(71).expect("schema id"),
            vec![
                SemanticExtractionFieldSchema::try_text("title".to_owned(), true, 64)
                    .expect("title field"),
            ],
        )
        .expect("schema");
        let payload = encode_semantic_extraction_request(
            &schema,
            &read,
            SemanticModelEncodingBudget::try_new(
                16 * 1024,
                16 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("extraction encoding budget"),
        )
        .expect("encode extraction request")
        .admit(
            &FixedCounter {
                revision: selected.clone(),
                tokens: 45,
            },
            &selected,
        )
        .expect("admit extraction request");
        let extraction_request = call_request(2, fixture.lease, account_binding, 500, 20, 100, NOW);
        let draft = AgentProviderExtractionRequestDraft::try_new(
            continuation
                .bind_extraction_request(extraction_request, &config, &schema, &read, payload)
                .expect("bind extraction"),
        )
        .expect("fixed extraction draft");
        let prepared = draft
            .try_prepare(
                &mut fixture.policy,
                extraction_request,
                &schema,
                &read,
                &FixedProviderInputCounter {
                    revision: selected,
                    tokens: 120,
                    quality: SemanticTokenCountQuality::ExactLocal,
                },
            )
            .expect("extraction whole-input admission");
        assert_eq!(prepared.structured_input_measurement().tokens(), 120);
        let semantic_stats = prepared.semantic_stats();
        let (transport, output_binding) = prepared.into_transport_parts();
        let committed = transport
            .commit(&mut fixture.policy)
            .expect("extraction transport commit");
        let input_metrics = committed.input_metrics();
        assert_eq!(
            input_metrics.semantic(),
            AgentProviderSemanticInputStats::Extraction(semantic_stats)
        );
        assert_eq!(
            input_metrics
                .semantic_payload_tokens()
                .expect("extraction token count")
                .tokens(),
            45
        );
        assert_eq!(
            input_metrics
                .structured_input_tokens()
                .expect("structured token count")
                .tokens(),
            120
        );
        assert!(committed.continuation_transcript_bytes().is_none());
        assert!(committed
            .input_evidence()
            .extraction_receipt()
            .is_some_and(|receipt| receipt.matches(&schema, &read)));
        assert!(committed
            .input_evidence()
            .observation_acknowledgement()
            .is_none());
        assert_eq!(fixture.policy.taints().len(), 1);
        assert_eq!(
            fixture.policy.taints()[0].reference_count(),
            initial_reference_count
        );
        let (request, input, continuation) = committed.into_parts();
        assert!(continuation.is_none());
        let (active, evidence) = input.into_parts();
        let mut collector = output_binding
            .start(&evidence)
            .expect("committed extraction evidence");
        let output = r#"{"v":1,"schema":71,"fields":[{"name":"title","value":{"k":"text","value":"Save draft","sources":["@r1"]}}]}"#;
        let split = output.len() / 2;
        collector
            .push_batch(AgentProviderStreamBatch::new(
                request.call(),
                vec![AgentProviderStreamEvent::TextDelta(
                    AgentProviderTextDelta::new(output[..split].to_owned()),
                )],
            ))
            .expect("first extraction delta");
        collector
            .push_batch(AgentProviderStreamBatch::new(
                request.call(),
                vec![AgentProviderStreamEvent::TextDelta(
                    AgentProviderTextDelta::new(output[split..].to_owned()),
                )],
            ))
            .expect("second extraction delta");
        assert_eq!(collector.retained_bytes(), output.len());
        let completion = crate::AgentProviderCompletion::new(
            request.call(),
            crate::AgentProviderStopReason::Completed,
            crate::AgentProviderUsage::try_new(120, 4, 0, 0, 0).expect("usage"),
            crate::AgentProviderStreamStats::new(
                240,
                2,
                u32::try_from(output.len()).expect("output bytes"),
                0,
                0,
            ),
            false,
        );
        let result = collector
            .finish(
                AgentProviderStreamConclusion::Completed(completion),
                &schema,
                &read,
                SemanticReadSensitivityLimit::PublicOnly,
            )
            .expect("admit exact extraction output");
        assert_eq!(result.schema(), schema.id());
        assert_eq!(result.stats().fields(), 1);
        assert_eq!(result.stats().source_edges(), 1);
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 120, 4, 80)
            .expect("extraction settlement");
    }

    #[test]
    fn empty_read_rejoins_committed_baseline_without_new_reference_authority() {
        let source = origin("empty-provider-read");
        let context = make_context(9_251, 9_252, 9_253);
        let observation = document_only_observation(context, source, 1);
        let baseline = observation_payload(&observation, 10)
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("baseline acknowledgement");
        let binding = account(context, NOW - 1);
        let retained = observation_taints(&observation, binding).expect("observation taint");
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(NOW - 2),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .expect("empty read");
        assert!(read.fragments().is_empty());
        let candidates = provider_read_taints(&read, &baseline, binding, &retained)
            .expect("empty read baseline rejoin");
        assert_eq!(candidates, retained);
        assert!(candidates
            .iter()
            .all(|cohort| cohort.reference_count() == 1));
        assert_eq!(
            provider_read_taints(&read, &baseline, binding, &[])
                .expect_err("missing retained baseline"),
            AgentPolicyError::ReadBaselineMissing
        );
    }

    #[test]
    fn screenshot_taint_covers_every_observed_frame_without_reference_authority() {
        let context = make_context(9_257, 9_258, 9_259);
        let observation = multi_origin_observation(context, 1);
        let cohorts = screenshot_taints(&observation, account(context, NOW - 1), [0xa5; 32])
            .expect("screenshot taints");
        assert_eq!(cohorts.len(), 3);
        assert_eq!(
            cohorts
                .iter()
                .map(|cohort| cohort.origin().clone())
                .collect::<Vec<_>>(),
            vec![
                origin("visual-main"),
                origin("visual-child-a"),
                origin("visual-child-b"),
            ]
        );
        assert!(cohorts.iter().all(|cohort| {
            cohort.context() == context
                && cohort.observation() == observation.request().id()
                && cohort.sensitivity() == SemanticSensitivity::Sensitive
                && cohort.trust() == SemanticTrust::UntrustedPage
                && cohort.reference_count() == 0
        }));
    }

    #[test]
    fn screenshot_commit_seals_on_exact_pixel_receipt_substitution() {
        let source = origin("screenshot-receipt");
        let context = make_context(9_277, 9_278, 9_279);
        let observation = actionable_observation(context, source.clone(), 1);
        let acknowledgement = observation_payload(&observation, 10)
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("acknowledgement");
        let first = admitted_test_screenshot(&observation, &acknowledgement, 81, 0x11);
        let second = admitted_test_screenshot(&observation, &acknowledgement, 82, 0x22);
        let (_, _, first_delivery) = first.into_provider_parts();
        let (_, _, second_delivery) = second.into_provider_parts();
        let mut fixture = policy_fixture(
            9_277,
            9_278,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let request = call_request(1, fixture.lease, account(context, NOW - 1), 10, 0, 0, NOW);
        let manifest = fixture.policy.manifest().id();
        let admission = fixture
            .policy
            .prepare_provider_screenshot_input(
                request,
                AgentModelCallExpectation::new(
                    manifest,
                    request.id(),
                    fixture.lease,
                    AgentPlanNodeId::from_raw(1),
                ),
                &observation,
                &first_delivery,
                10,
            )
            .expect("screenshot admission");
        let substituted = second_delivery.commit();
        assert!(matches!(
            fixture
                .policy
                .commit_screenshot_input(admission, &substituted),
            Err(AgentPolicyError::AdmissionMismatch)
        ));
        assert!(fixture.policy.is_sealed());
        assert!(fixture.policy.taints().is_empty());
    }

    #[test]
    fn screenshot_result_counts_whole_body_commits_sensitive_zero_reference_taint_once() {
        let source = origin("provider-screenshot-request");
        let context = make_context(9_307, 9_308, 9_309);
        let observation = actionable_observation(context, source.clone(), 1);
        let selected = tokenizer();
        let config = provider_config(selected.clone(), 10, 20);
        let objective = AgentProviderObjective::try_admit(
            "Inspect the visible page state".to_owned(),
            &FixedCounter {
                revision: selected.clone(),
                tokens: 5,
            },
            &selected,
        )
        .expect("objective");
        let binding = account(context, NOW - 1);
        let visual_request =
            call_request(2, AgentPlanLeaseId::from_raw(1), binding, 500, 20, 100, NOW);
        let provider_count = SemanticTokenMeasurement::try_new(
            selected.clone(),
            120,
            SemanticTokenCountQuality::ProviderExact,
        )
        .expect("provider count");
        assert_eq!(
            config
                .validate_screenshot_request(visual_request, &provider_count)
                .expect_err("visual request requires local whole-body counting"),
            AgentProviderContractError::InputTokenQuality
        );
        let oversized = SemanticTokenMeasurement::try_new(
            selected.clone(),
            501,
            SemanticTokenCountQuality::ExactLocal,
        )
        .expect("oversized local count");
        assert_eq!(
            config
                .validate_screenshot_request(visual_request, &oversized)
                .expect_err("whole visual body exceeds its exact authorization"),
            AgentProviderContractError::AdmissionBudget
        );

        let mut fixture = policy_fixture(
            9_307,
            9_308,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 5_000, 10_000),
        );
        let prepared = prepare_openai_screenshot_result(
            &mut fixture,
            binding,
            &observation,
            &objective,
            &config,
            (71, 0x33, 120),
        );
        assert_eq!(prepared.structured_input_measurement().tokens(), 120);
        assert_eq!(
            prepared.structured_input_measurement().quality(),
            SemanticTokenCountQuality::ExactLocal
        );
        assert!(prepared.continuation_transcript_bytes() > 0);
        assert!(
            prepared.continuation_transcript_bytes()
                <= crate::MAX_AGENT_PROVIDER_SCREENSHOT_TRANSCRIPT_BYTES
        );
        assert_eq!(fixture.policy.pending_model_calls(), 1);
        assert_eq!(fixture.policy.taints().len(), 1);
        assert_eq!(fixture.policy.accounting().reserved_model_tokens(), 140);
        let prepared_debug = format!("{prepared:?}");
        assert!(!prepared_debug.contains("provider-screenshot-request"));
        assert!(!prepared_debug.contains("call_policy_screenshot_1"));
        assert!(prepared_debug.contains("[redacted]"));

        let screenshot_stats = prepared.screenshot_stats();
        let serialized_request_bytes = prepared.request().byte_len() as u32;
        let committed = prepared
            .into_transport_input()
            .commit(&mut fixture.policy)
            .expect("screenshot transport commit");
        let input_metrics = committed.input_metrics();
        assert_eq!(
            input_metrics.serialized_request_bytes(),
            serialized_request_bytes
        );
        assert_eq!(
            input_metrics.semantic(),
            AgentProviderSemanticInputStats::Screenshot(screenshot_stats)
        );
        assert_eq!(
            input_metrics.semantic().disclosed_bytes(),
            screenshot_stats.canonical_png_bytes()
        );
        assert_eq!(input_metrics.semantic_payload_tokens(), None);
        assert_eq!(
            input_metrics
                .structured_input_tokens()
                .expect("structured token count")
                .tokens(),
            120
        );
        assert_eq!(committed.continuation_transcript_bytes(), None);
        let receipt = committed
            .input_evidence()
            .screenshot_receipt()
            .expect("screenshot evidence");
        assert_eq!(receipt.id().get(), 71);
        assert_eq!(receipt.observation(), observation.request().id());
        assert_eq!(
            receipt.observation_generation(),
            observation.request().generation()
        );
        assert_eq!(receipt.context(), context);
        assert_eq!(receipt.captured_at().millis(), 1_200);
        assert!(committed
            .input_evidence()
            .observation_acknowledgement()
            .is_none());
        assert!(committed.input_evidence().diff_receipt().is_none());
        assert!(committed.input_evidence().read_receipt().is_none());
        assert_eq!(fixture.policy.taints().len(), 2);
        let visual_taint = fixture
            .policy
            .taints()
            .iter()
            .find(|taint| taint.reference_count() == 0)
            .expect("zero-reference screenshot taint");
        assert_eq!(visual_taint.context(), context);
        assert_eq!(visual_taint.origin(), &source);
        assert_eq!(visual_taint.sensitivity(), SemanticSensitivity::Sensitive);
        assert_eq!(visual_taint.trust(), SemanticTrust::UntrustedPage);
        assert_eq!(visual_taint.account(), AgentAccountScope::Anonymous);
        let (_, input, continuation) = committed.into_parts();
        assert!(
            continuation.is_none(),
            "visual pixels must never enter replay"
        );
        let (active, evidence) = input.into_parts();
        assert!(matches!(
            evidence,
            AgentProviderInputEvidence::Screenshot(_)
        ));
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 120, 4, 80)
            .expect("screenshot provider settlement");
        assert_eq!(fixture.policy.pending_model_calls(), 0);

        let cancelled_source = origin("provider-screenshot-cancelled");
        let cancelled_context = make_context(9_407, 9_408, 9_409);
        let cancelled_observation =
            actionable_observation(cancelled_context, cancelled_source.clone(), 1);
        let cancelled_binding = account(cancelled_context, NOW - 1);
        let mut cancelled = policy_fixture(
            9_407,
            9_408,
            cancelled_source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 5_000, 10_000),
        );
        let prepared = prepare_openai_screenshot_result(
            &mut cancelled,
            cancelled_binding,
            &cancelled_observation,
            &objective,
            &config,
            (72, 0x44, 120),
        );
        assert_eq!(cancelled.policy.pending_model_calls(), 1);
        assert_eq!(cancelled.policy.taints().len(), 1);
        assert!(matches!(
            prepared
                .settle(
                    &mut cancelled.policy,
                    AgentProviderRequestSettlement::Cancelled
                )
                .expect("visual cancellation"),
            AgentProviderInputOutcome::Cancelled
        ));
        assert_eq!(cancelled.policy.pending_model_calls(), 0);
        assert_eq!(cancelled.policy.taints().len(), 1);
        assert_eq!(cancelled.policy.accounting().reserved_model_tokens(), 0);
    }

    #[test]
    fn anthropic_request_uses_the_same_atomic_input_authority() {
        let source = origin("anthropic-request");
        let context = make_context(9_107, 9_108, 9_109);
        let observation = mixed_observation(context, source.clone(), 1);
        let selected = tokenizer();
        let objective = AgentProviderObjective::try_admit(
            "Read the reviewed result".to_owned(),
            &FixedCounter {
                revision: selected.clone(),
                tokens: 5,
            },
            &selected,
        )
        .expect("objective");
        let mut fixture = policy_fixture(
            9_107,
            9_108,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let prepared = AgentPreparedObservationRequest::try_anthropic(
            &mut fixture.policy,
            call_request(1, fixture.lease, account(context, NOW), 15, 20, 100, NOW),
            &observation,
            observation_payload(&observation, 50),
            &objective,
            anthropic_provider_config(selected, 10, 20),
        )
        .expect("prepared request");
        assert_eq!(fixture.policy.pending_model_calls(), 1);
        assert_eq!(
            prepared.request().endpoint(),
            AgentProviderEndpoint::AnthropicMessages
        );
        let wire: Value = serde_json::from_slice(prepared.request().body()).expect("request JSON");
        assert_eq!(wire["model"], "claude-opus-5");
        assert_eq!(wire["tool_choice"]["disable_parallel_tool_use"], true);
        assert_eq!(wire["messages"].as_array().expect("messages").len(), 1);
        assert_eq!(
            wire["messages"][0]["content"]
                .as_array()
                .expect("content blocks")
                .len(),
            2
        );
        assert!(matches!(
            prepared
                .into_transport_input()
                .cancel(&mut fixture.policy)
                .expect("cancel"),
            AgentProviderInputOutcome::Cancelled
        ));
        assert_eq!(fixture.policy.pending_model_calls(), 0);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 0);
        assert!(fixture.policy.taints().is_empty());
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
        let selected = tokenizer();
        let objective = AgentProviderObjective::try_admit(
            "Read the selected fields".to_owned(),
            &FixedCounter {
                revision: selected.clone(),
                tokens: 2,
            },
            &selected,
        )
        .expect("objective");
        let mut fixture = policy_fixture(
            17,
            18,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        let binding = account(context, NOW - 1);
        let prepared = AgentPreparedReadRequest::try_openai(
            &mut fixture.policy,
            call_request(1, fixture.lease, binding, 5, 15, 90, NOW),
            &read,
            payload,
            &objective,
            provider_config(selected, 3, 15),
        )
        .expect("prepared request");
        assert_eq!(prepared.semantic_stats().items(), read.stats().items());
        let semantic_stats = prepared.semantic_stats();
        let serialized_request_bytes = prepared.request().byte_len() as u32;
        let AgentProviderInputOutcome::Committed(committed) = prepared
            .settle(
                &mut fixture.policy,
                AgentProviderRequestSettlement::Committed,
            )
            .expect("commit")
        else {
            panic!("committed input");
        };
        let input_metrics = committed.metrics();
        assert_eq!(
            input_metrics.serialized_request_bytes(),
            serialized_request_bytes
        );
        assert_eq!(
            input_metrics.semantic(),
            AgentProviderSemanticInputStats::Read(semantic_stats)
        );
        assert_eq!(
            input_metrics
                .semantic_payload_tokens()
                .expect("read token count")
                .tokens(),
            40
        );
        assert_eq!(input_metrics.structured_input_tokens(), None);
        assert!(committed
            .evidence()
            .read_receipt()
            .is_some_and(|receipt| receipt.matches_read(&read)));
        assert!(committed.evidence().observation_acknowledgement().is_none());
        let (active, _) = committed.into_parts();
        assert_eq!(fixture.policy.taints().len(), 1);
        assert_eq!(
            fixture.policy.taints()[0].sensitivity(),
            SemanticSensitivity::Sensitive
        );
        assert_eq!(fixture.policy.taints()[0].reference_count(), 2);
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
            manifest: fixture.policy.manifest().id(),
            id: AgentModelCallId::new(99).expect("call"),
            lease: fixture.lease,
            node: AgentPlanNodeId::from_raw(1),
            kind: ModelInputKind::Observation,
            input_token_limit: 0,
            output_token_limit: 0,
            cost_limit_micro_usd: 0,
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
    fn semantic_effect_permit_is_verified_accounted_and_nonreplayable() {
        let source = origin("effect");
        let (mut registry, context) = make_context_registry(107, 108, 109);
        let observation = actionable_observation(context, source.clone(), 1);
        let unfresh = registry
            .automation_state(context.identity().id())
            .expect("unfresh state");
        registry
            .acknowledge_observation(context.identity().id(), context)
            .expect("observation current");
        let automation = registry
            .automation_state(context.identity().id())
            .expect("automation state");
        let binding = account(context, NOW - 1);
        let mut fixture = policy_fixture(
            107,
            108,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
            run_budget(10, 1_000, 10_000),
        );
        commit_observation_to_model(&mut fixture.policy, fixture.lease, 1, binding, &observation);
        let batch = click_batch(&observation, 2, SemanticEffectClass::LocalWrite);
        let action = batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepared action");
        let assessment =
            AgentEffectAssessment::new(&action, source.clone(), SemanticEffectClass::LocalWrite);
        let request = effect_request(1, fixture.lease, binding, automation);
        let permit = match fixture
            .policy
            .authorize_semantic_effect(request, &action, &assessment)
            .expect("effect decision")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("same-origin effect was in scope"),
        };
        assert_eq!(fixture.policy.pending_effects(), 1);
        assert!(permit.matches_action(&action));
        assert_eq!(permit.effect(), SemanticEffectClass::LocalWrite);
        assert_eq!(fixture.policy.accounting().consumed_operations(), 1);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 1);

        let payload = observation_payload(&observation, 10);
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(
                    call_request(2, fixture.lease, binding, 0, 0, 0, NOW),
                    &observation,
                    &payload,
                )
                .expect_err("effect freezes model context"),
            AgentPolicyError::EffectPending
        );

        let attempt = SemanticActionAttemptId::new(1).expect("attempt");
        let active = fixture
            .policy
            .dispatch_semantic_effect(
                permit,
                &action,
                effect_dispatch_request(1, binding, automation),
            )
            .expect("dispatch");
        let (pending, native) = crate::prepare_semantic_action_execution(
            active,
            &action,
            crate::SemanticActionExecutionInstant::from_millis(NOW - 20),
        )
        .expect("native execution");
        let actual_geometry = native.expected_geometry();
        let outcome = pending.settle(
            action.frame(),
            native.complete(
                crate::SemanticActionExecutionBackend::FixedSemanticRecipe,
                crate::SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
                crate::SemanticActionNativeViewport::try_new(800, 600).expect("viewport"),
                actual_geometry,
                crate::SemanticActionExecutionInstant::from_millis(NOW - 10),
                crate::SemanticActionExecutionInstant::from_millis(NOW),
            ),
        );
        let start =
            crate::begin_semantic_action_settlement(outcome, &action).expect("settlement start");
        let mut settlement_coordinator = crate::SemanticActionSettlementCoordinator::new();
        let crate::SemanticActionSettlementUpdate::Terminal(terminal) = settlement_coordinator
            .begin(start)
            .expect("immediate settlement")
        else {
            panic!("immediate settlement was pending");
        };
        let snapshot = post_action_snapshot(&observation);
        let verified_terminal = crate::verify_semantic_action_terminal(
            *terminal,
            &action,
            SemanticEffectEvidence::snapshot(
                attempt,
                SemanticSettleInstant::from_millis(NOW + 1),
                &snapshot,
            ),
        )
        .expect("independent proof");
        let accounted = fixture
            .policy
            .settle_verified_semantic_terminal(verified_terminal, &action)
            .expect("effect settlement");
        let receipt = accounted.receipt();
        assert_eq!(receipt.id().get(), 1);
        assert_eq!(receipt.attempt(), attempt);
        assert_eq!(
            receipt.settlement(),
            AgentEffectSettlement::Verified(crate::SemanticEffectProofKind::TargetState)
        );
        assert_eq!(accounted.verified().attempt(), attempt);
        assert_eq!(accounted.settlement().attempt(), attempt);
        assert_eq!(
            accounted.execution().backend(),
            crate::SemanticActionExecutionBackend::FixedSemanticRecipe
        );
        let debug = format!("{accounted:?}");
        assert!(!debug.contains("Save draft"));
        assert!(!debug.contains("effect.example.test"));
        let current_request = SemanticObservationRequest::initial(
            SemanticObservationId::new(99).expect("current observation"),
            observation.request().context(),
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let current_observation = SemanticObservationAssembler::new(current_request, snapshot)
            .expect("current assembler")
            .finish()
            .expect("current observation");
        let acknowledgement = SemanticObservationAcknowledgement::from_fingerprint(
            crate::semantic_diff::SemanticObservationFingerprint::from_observation(&observation),
        );
        let wrong_acknowledgement = SemanticObservationAcknowledgement::from_fingerprint(
            crate::semantic_diff::SemanticObservationFingerprint::from_observation(
                &current_observation,
            ),
        );
        let refusal = crate::finalize_accounted_semantic_action_result(
            &action,
            accounted,
            &observation,
            &wrong_acknowledgement,
            crate::SemanticPostActionObservation::new(
                SemanticSettleInstant::from_millis(NOW + 1),
                current_observation,
            ),
            SemanticDiffBudget::ACTION,
        )
        .expect_err("wrong baseline acknowledgement");
        assert_eq!(
            refusal.error(),
            crate::SemanticActionResultError::BaselineNotAcknowledged
        );
        assert!(!format!("{refusal:?}").contains("Save draft"));
        let (accounted, current, _) = refusal.into_parts();
        let finalized = crate::finalize_accounted_semantic_action_result(
            &action,
            accounted,
            &observation,
            &acknowledgement,
            current,
            SemanticDiffBudget::ACTION,
        )
        .expect("accounted action result");
        assert_eq!(finalized.receipt(), receipt);
        assert_eq!(finalized.settlement().attempt(), attempt);
        assert_eq!(finalized.result().verified().attempt(), attempt);
        assert!(!format!("{finalized:?}").contains("Save draft"));
        let mut batch_execution =
            crate::SemanticActionBatchExecution::new(&batch).expect("batch execution");
        assert_eq!(
            batch_execution
                .record_success(&action, finalized)
                .expect("accounted batch success"),
            crate::SemanticActionBatchContinuation::Complete
        );
        let batch_result = batch_execution.finish().expect("complete batch");
        assert_eq!(batch_result.completions().len(), 1);
        assert_eq!(batch_result.completions()[0].receipt(), receipt);
        assert_eq!(batch_result.completions()[0].attempt(), attempt);
        assert_eq!(
            batch_result.completions()[0].execution().backend(),
            crate::SemanticActionExecutionBackend::FixedSemanticRecipe
        );
        assert!(!format!("{batch_result:?}").contains("Save draft"));
        assert_eq!(fixture.policy.pending_effects(), 0);
        assert_eq!(fixture.policy.accounting().consumed_operations(), 2);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 0);
        assert_eq!(fixture.policy.taints().len(), 1);

        let cancel_request = effect_request(2, fixture.lease, binding, automation);
        let cancel_permit = match fixture
            .policy
            .authorize_semantic_effect(cancel_request, &action, &assessment)
            .expect("second effect decision")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("same-origin effect was in scope"),
        };
        fixture
            .policy
            .cancel_semantic_effect(cancel_permit, AgentEffectCancellation::Cancelled)
            .expect("cancel permit");
        assert_eq!(fixture.policy.accounting().consumed_operations(), 2);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 0);
        assert_eq!(
            fixture
                .policy
                .authorize_semantic_effect(cancel_request, &action, &assessment)
                .expect_err("effect id cannot reopen"),
            AgentPolicyError::EffectReplay
        );

        let failed_permit = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(3, fixture.lease, binding, automation),
                &action,
                &assessment,
            )
            .expect("third effect decision")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("same-origin effect was in scope"),
        };
        let failed_attempt = SemanticActionAttemptId::new(2).expect("attempt");
        let failed_active = fixture
            .policy
            .dispatch_semantic_effect(
                failed_permit,
                &action,
                effect_dispatch_request(2, binding, automation),
            )
            .expect("failed dispatch");
        let (pending, native) = crate::prepare_semantic_action_execution(
            failed_active,
            &action,
            crate::SemanticActionExecutionInstant::from_millis(NOW - 20),
        )
        .expect("failed-path native execution");
        let actual_geometry = native.expected_geometry();
        let outcome = pending.settle(
            action.frame(),
            native.complete(
                crate::SemanticActionExecutionBackend::FixedSemanticRecipe,
                crate::SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
                crate::SemanticActionNativeViewport::try_new(800, 600).expect("viewport"),
                actual_geometry,
                crate::SemanticActionExecutionInstant::from_millis(NOW - 10),
                crate::SemanticActionExecutionInstant::from_millis(NOW),
            ),
        );
        let start = crate::begin_semantic_action_settlement(outcome, &action)
            .expect("failed-path settlement start");
        let crate::SemanticActionSettlementUpdate::Terminal(terminal) = settlement_coordinator
            .begin(start)
            .expect("failed-path immediate settlement")
        else {
            panic!("immediate settlement was pending");
        };
        let verification_refusal = crate::verify_semantic_action_terminal(
            *terminal,
            &action,
            SemanticEffectEvidence::navigation(
                failed_attempt,
                SemanticSettleInstant::from_millis(NOW + 1),
                action.frame().context(),
                action.frame().context(),
            ),
        )
        .expect_err("wrong evidence class");
        assert_eq!(
            verification_refusal.error(),
            crate::SemanticVerificationError::EvidenceKindMismatch
        );
        let failed = fixture
            .policy
            .settle_refused_semantic_terminal(verification_refusal)
            .expect("failed settlement");
        assert_eq!(
            failed.receipt().settlement(),
            AgentEffectSettlement::Failed(SemanticActionFailure::VerificationFailed)
        );
        assert_eq!(failed.receipt().attempt(), failed_attempt);
        assert_eq!(
            failed.verification_error(),
            Some(crate::SemanticVerificationError::EvidenceKindMismatch)
        );
        assert_eq!(
            failed
                .verification_observed_at()
                .expect("verification observation")
                .millis(),
            NOW + 1
        );
        assert_eq!(
            failed.settlement().expect("failed settlement").attempt(),
            failed_attempt
        );
        let debug = format!("{failed:?}");
        assert!(!debug.contains("Save draft"));
        assert!(!debug.contains("effect.example.test"));
        let failed_batch = crate::SemanticActionBatchExecution::new(&batch)
            .expect("failed batch execution")
            .fail(&action, failed)
            .expect("accounted batch failure");
        assert_eq!(
            failed_batch.outcome(),
            crate::SemanticActionBatchOutcome::Failed {
                ordinal: 1,
                failure: SemanticActionFailure::VerificationFailed,
                recovery: crate::SemanticActionRecoveryHint::FreshObservationRequired,
            }
        );
        let failed_summary = failed_batch.failure().expect("failed summary");
        assert_eq!(failed_summary.receipt().attempt(), failed_attempt);
        assert_eq!(
            failed_summary.stage(),
            crate::SemanticActionBatchFailureStage::AfterExecution
        );
        assert_eq!(
            failed_summary.verification_error(),
            Some(crate::SemanticVerificationError::EvidenceKindMismatch)
        );
        assert_eq!(
            failed_summary
                .verification_observed_at()
                .expect("verification observation")
                .millis(),
            NOW + 1
        );
        assert!(!format!("{failed_batch:?}").contains("Save draft"));
        assert_eq!(fixture.policy.accounting().consumed_operations(), 3);
        assert!(!fixture.policy.is_sealed());

        let stale_permit = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(4, fixture.lease, binding, automation),
                &action,
                &assessment,
            )
            .expect("fourth effect decision")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("same-origin effect was in scope"),
        };
        assert_eq!(
            fixture
                .policy
                .dispatch_semantic_effect(
                    stale_permit,
                    &action,
                    effect_dispatch_request(3, binding, unfresh),
                )
                .expect_err("dispatch must resample automation state"),
            AgentPolicyError::ContextNotAutomatable
        );
        assert_eq!(fixture.policy.pending_effects(), 0);
        assert_eq!(fixture.policy.accounting().consumed_operations(), 3);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 0);
        assert!(!fixture.policy.is_sealed());
        let debug = format!("{:?} {receipt:?} {failed_batch:?}", fixture.policy);
        assert!(!debug.contains("effect.example.test"));
        assert!(!debug.contains("Save draft"));
    }

    #[test]
    fn preverification_refusals_map_and_charge_without_raw_failure_authority() {
        let source = origin("preverification-failure");
        let (mut registry, context) = make_context_registry(407, 408, 409);
        let observation = actionable_observation(context, source.clone(), 1);
        registry
            .acknowledge_observation(context.identity().id(), context)
            .expect("observation current");
        let automation = registry
            .automation_state(context.identity().id())
            .expect("automation state");
        let binding = account(context, NOW - 1);
        let mut fixture = policy_fixture(
            407,
            408,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
            run_budget(10, 1_000, 10_000),
        );
        commit_observation_to_model(&mut fixture.policy, fixture.lease, 1, binding, &observation);
        let batch = click_batch(&observation, 2, SemanticEffectClass::LocalWrite);
        let action = batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepared action");

        let active =
            dispatch_local_effect(&mut fixture, &action, &source, binding, automation, 1, 1);
        let mut execution_coordinator = crate::SemanticActionExecutionCoordinator::new();
        let (reservation, native) = execution_coordinator
            .begin(
                active,
                &action,
                crate::SemanticActionExecutionInstant::from_millis(NOW),
            )
            .expect("native admission");
        drop(native);
        let outcome = execution_coordinator
            .refuse(&reservation, SemanticActionFailure::TargetOccluded)
            .expect("native refusal");
        let refusal = crate::begin_semantic_action_settlement(outcome, &action)
            .expect_err("failed execution cannot settle");
        let failed = fixture
            .policy
            .settle_settlement_start_refusal(refusal, &action)
            .expect("charged native refusal");
        assert_eq!(
            failed.receipt().settlement(),
            AgentEffectSettlement::Failed(SemanticActionFailure::TargetOccluded)
        );
        let terminal = crate::SemanticActionBatchExecution::new(&batch)
            .expect("batch")
            .fail(&action, failed)
            .expect("failed terminal");
        assert_eq!(
            terminal.failure().expect("failure").stage(),
            crate::SemanticActionBatchFailureStage::BeforeVerification
        );

        let active =
            dispatch_local_effect(&mut fixture, &action, &source, binding, automation, 2, 2);
        let mut sealed_execution = crate::SemanticActionExecutionCoordinator::new();
        sealed_execution.seal();
        let refusal = sealed_execution
            .begin(
                active,
                &action,
                crate::SemanticActionExecutionInstant::from_millis(NOW),
            )
            .expect_err("shutdown admission");
        let failed = fixture
            .policy
            .settle_execution_admission_refusal(refusal, &action)
            .expect("charged admission refusal");
        assert_eq!(
            failed.receipt().settlement(),
            AgentEffectSettlement::Failed(SemanticActionFailure::BackendRefused)
        );
        let admission_terminal = crate::SemanticActionBatchExecution::new(&batch)
            .expect("batch")
            .fail(&action, failed)
            .expect("failed terminal");
        assert_eq!(
            admission_terminal.outcome(),
            crate::SemanticActionBatchOutcome::Failed {
                ordinal: 1,
                failure: SemanticActionFailure::BackendRefused,
                recovery: crate::SemanticActionRecoveryHint::FreshObservationRequired,
            }
        );

        let waiting_batch = click_batch_with_wait(
            &observation,
            2,
            SemanticEffectClass::LocalWrite,
            SemanticWaitCondition::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
        );
        let waiting_action = waiting_batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("waiting action");
        let active = dispatch_local_effect(
            &mut fixture,
            &waiting_action,
            &source,
            binding,
            automation,
            3,
            3,
        );
        let mut execution_coordinator = crate::SemanticActionExecutionCoordinator::new();
        let (_reservation, native) = execution_coordinator
            .begin(
                active,
                &waiting_action,
                crate::SemanticActionExecutionInstant::from_millis(NOW - 20),
            )
            .expect("native admission");
        let actual_geometry = native.expected_geometry();
        let outcome = execution_coordinator
            .settle(
                waiting_action.frame(),
                native.complete(
                    crate::SemanticActionExecutionBackend::FixedSemanticRecipe,
                    crate::SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
                    crate::SemanticActionNativeViewport::try_new(800, 600).expect("viewport"),
                    actual_geometry,
                    crate::SemanticActionExecutionInstant::from_millis(NOW - 10),
                    crate::SemanticActionExecutionInstant::from_millis(NOW),
                ),
            )
            .expect("native settlement");
        let start = crate::begin_semantic_action_settlement(outcome, &waiting_action)
            .expect("settlement start");
        let mut sealed_settlement = crate::SemanticActionSettlementCoordinator::new();
        sealed_settlement.seal();
        let refusal = sealed_settlement
            .begin(start)
            .expect_err("settlement admission after shutdown");
        let failed = fixture
            .policy
            .settle_settlement_admission_refusal(refusal, &waiting_action)
            .expect("charged settlement admission refusal");
        assert_eq!(
            failed.receipt().settlement(),
            AgentEffectSettlement::Failed(SemanticActionFailure::BackendRefused)
        );
        let settlement_terminal = crate::SemanticActionBatchExecution::new(&waiting_batch)
            .expect("waiting batch")
            .fail(&waiting_action, failed)
            .expect("failed terminal");
        assert_eq!(
            settlement_terminal.outcome(),
            crate::SemanticActionBatchOutcome::Failed {
                ordinal: 1,
                failure: SemanticActionFailure::BackendRefused,
                recovery: crate::SemanticActionRecoveryHint::FreshObservationRequired,
            }
        );

        assert_eq!(fixture.policy.pending_effects(), 0);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 0);
        assert_eq!(fixture.policy.accounting().consumed_operations(), 4);
        assert!(!fixture.policy.is_sealed());
        let debug = format!("{:?}", fixture.policy);
        assert!(!debug.contains("preverification-failure"));
        assert!(!debug.contains("Save draft"));
    }

    #[test]
    fn bounded_effect_ledger_admits_four_distinct_actions_and_refuses_duplicates() {
        let source = origin("parallel-effect");
        let (mut registry, context) = make_context_registry(207, 208, 209);
        registry
            .acknowledge_observation(context.identity().id(), context)
            .expect("observation current");
        let automation = registry
            .automation_state(context.identity().id())
            .expect("automation state");
        let binding = account(context, NOW - 1);
        let observation = multi_actionable_observation(context, source.clone(), 1, 5);
        let mut fixture = policy_fixture(
            207,
            208,
            source.clone(),
            SemanticSensitivity::Public,
            &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
            run_budget(20, 1_000, 10_000),
        );
        commit_observation_to_model(&mut fixture.policy, fixture.lease, 1, binding, &observation);

        let first = prepared_click(&observation, 2, SemanticEffectClass::LocalWrite);
        let first_assessment =
            AgentEffectAssessment::new(&first, source.clone(), SemanticEffectClass::LocalWrite);
        let first_permit = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(1, fixture.lease, binding, automation),
                &first,
                &first_assessment,
            )
            .expect("first effect")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("local write was in scope"),
        };
        assert_eq!(
            fixture
                .policy
                .authorize_semantic_effect(
                    effect_request(2, fixture.lease, binding, automation),
                    &first,
                    &first_assessment,
                )
                .expect_err("duplicate prepared action"),
            AgentPolicyError::EffectActionPending
        );

        let mut permits = vec![first_permit];
        for (effect_id, target) in [(3, 3), (4, 4), (5, 5)] {
            let action = prepared_click(&observation, target, SemanticEffectClass::LocalWrite);
            let assessment = AgentEffectAssessment::new(
                &action,
                source.clone(),
                SemanticEffectClass::LocalWrite,
            );
            let permit = match fixture
                .policy
                .authorize_semantic_effect(
                    effect_request(effect_id, fixture.lease, binding, automation),
                    &action,
                    &assessment,
                )
                .expect("independent effect")
            {
                AgentEffectAuthorization::Permit(permit) => permit,
                AgentEffectAuthorization::NeedsHuman(_) => panic!("local write was in scope"),
            };
            permits.push(permit);
        }
        assert_eq!(fixture.policy.pending_effects(), MAX_AGENT_PENDING_EFFECTS);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 4);

        let fifth = prepared_click(&observation, 6, SemanticEffectClass::LocalWrite);
        let fifth_assessment =
            AgentEffectAssessment::new(&fifth, source.clone(), SemanticEffectClass::LocalWrite);
        assert_eq!(
            fixture
                .policy
                .authorize_semantic_effect(
                    effect_request(6, fixture.lease, binding, automation),
                    &fifth,
                    &fifth_assessment,
                )
                .expect_err("bounded effect ceiling"),
            AgentPolicyError::PendingEffectLimit
        );
        for permit in permits {
            fixture
                .policy
                .cancel_semantic_effect(permit, AgentEffectCancellation::Cancelled)
                .expect("cancel exact permit");
        }
        assert_eq!(fixture.policy.pending_effects(), 0);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 0);

        let fifth_permit = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(6, fixture.lease, binding, automation),
                &fifth,
                &fifth_assessment,
            )
            .expect("capacity refusal did not consume id")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("local write was in scope"),
        };
        fixture
            .policy
            .cancel_semantic_effect(fifth_permit, AgentEffectCancellation::Cancelled)
            .expect("cancel fifth permit");
        assert!(!fixture.policy.is_sealed());
    }

    #[test]
    fn durable_effects_serialize_by_canonical_origin_without_blocking_local_work() {
        let source = origin("serialized-effect");
        let (mut registry, context) = make_context_registry(217, 218, 219);
        registry
            .acknowledge_observation(context.identity().id(), context)
            .expect("observation current");
        let automation = registry
            .automation_state(context.identity().id())
            .expect("automation state");
        let binding = account(context, NOW - 1);
        let observation = multi_actionable_observation(context, source.clone(), 1, 3);
        let mut fixture = policy_fixture(
            217,
            218,
            source.clone(),
            SemanticSensitivity::Public,
            &[
                SemanticEffectClass::Read,
                SemanticEffectClass::LocalWrite,
                SemanticEffectClass::ExternalWrite,
            ],
            run_budget(20, 1_000, 10_000),
        );
        commit_observation_to_model(&mut fixture.policy, fixture.lease, 1, binding, &observation);

        let durable = prepared_click(&observation, 2, SemanticEffectClass::ExternalWrite);
        let durable_assessment = AgentEffectAssessment::new(
            &durable,
            source.clone(),
            SemanticEffectClass::ExternalWrite,
        );
        let durable_permit = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(1, fixture.lease, binding, automation),
                &durable,
                &durable_assessment,
            )
            .expect("first durable write")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("same-origin write was in scope"),
        };

        let local = prepared_click(&observation, 3, SemanticEffectClass::LocalWrite);
        let local_assessment =
            AgentEffectAssessment::new(&local, source.clone(), SemanticEffectClass::LocalWrite);
        let local_permit = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(2, fixture.lease, binding, automation),
                &local,
                &local_assessment,
            )
            .expect("local work remains independent")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("local write was in scope"),
        };

        let conflicting = prepared_click(&observation, 4, SemanticEffectClass::ExternalWrite);
        let conflicting_assessment = AgentEffectAssessment::new(
            &conflicting,
            source.clone(),
            SemanticEffectClass::ExternalWrite,
        );
        assert_eq!(
            fixture
                .policy
                .authorize_semantic_effect(
                    effect_request(3, fixture.lease, binding, automation),
                    &conflicting,
                    &conflicting_assessment,
                )
                .expect_err("same-origin durable write must serialize"),
            AgentPolicyError::OriginWritePending
        );
        assert_eq!(fixture.policy.pending_effects(), 2);
        assert_eq!(fixture.policy.pending_origin_writes(), 1);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 2);
        fixture
            .policy
            .cancel_semantic_effect(durable_permit, AgentEffectCancellation::Cancelled)
            .expect("release origin write");
        let conflicting_permit = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(4, fixture.lease, binding, automation),
                &conflicting,
                &conflicting_assessment,
            )
            .expect("origin released")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("same-origin write was in scope"),
        };
        fixture
            .policy
            .cancel_semantic_effect(local_permit, AgentEffectCancellation::Cancelled)
            .expect("cancel local work");
        fixture
            .policy
            .cancel_semantic_effect(conflicting_permit, AgentEffectCancellation::Cancelled)
            .expect("cancel replacement write");
        assert_eq!(fixture.policy.pending_effects(), 0);
        let debug = format!("{:?}", fixture.policy);
        assert!(!debug.contains("serialized-effect"));
        assert!(!debug.contains("Action 2"));
    }

    #[test]
    fn approved_durable_writes_to_distinct_origins_can_progress_concurrently() {
        let origin_a = origin("parallel-origin-a");
        let origin_b = origin("parallel-origin-b");
        let (mut registry_a, context_a) = make_context_registry(227, 228, 229);
        let (mut registry_b, context_b) = make_context_registry(227, 228, 230);
        registry_a
            .acknowledge_observation(context_a.identity().id(), context_a)
            .expect("observation a current");
        registry_b
            .acknowledge_observation(context_b.identity().id(), context_b)
            .expect("observation b current");
        let automation_a = registry_a
            .automation_state(context_a.identity().id())
            .expect("automation a");
        let automation_b = registry_b
            .automation_state(context_b.identity().id())
            .expect("automation b");
        let binding_a = account(context_a, NOW - 2);
        let binding_b = account(context_b, NOW - 1);
        let observation_a = actionable_observation(context_a, origin_a.clone(), 1);
        let observation_b = actionable_observation(context_b, origin_b.clone(), 2);
        let flow_effects = effects(&[SemanticEffectClass::ExternalWrite]);
        let flows = vec![
            AgentDataFlowRule::try_new(
                origin_a.clone(),
                AgentAccountScope::Anonymous,
                origin_b.clone(),
                AgentAccountScope::Anonymous,
                SemanticSensitivity::Public,
                flow_effects,
            )
            .expect("a to b flow"),
            AgentDataFlowRule::try_new(
                origin_b.clone(),
                AgentAccountScope::Anonymous,
                origin_a.clone(),
                AgentAccountScope::Anonymous,
                SemanticSensitivity::Public,
                flow_effects,
            )
            .expect("b to a flow"),
        ];
        let mut fixture = policy_fixture_with_flows(
            227,
            228,
            vec![origin_a.clone(), origin_b.clone()],
            flows,
            &[
                SemanticEffectClass::Read,
                SemanticEffectClass::ExternalWrite,
            ],
            run_budget(20, 1_000, 10_000),
        );
        commit_observation_to_model(
            &mut fixture.policy,
            fixture.lease,
            1,
            binding_a,
            &observation_a,
        );
        commit_observation_to_model(
            &mut fixture.policy,
            fixture.lease,
            2,
            binding_b,
            &observation_b,
        );

        let action_a = prepared_click(&observation_a, 2, SemanticEffectClass::ExternalWrite);
        let assessment_a = AgentEffectAssessment::new(
            &action_a,
            origin_a.clone(),
            SemanticEffectClass::ExternalWrite,
        );
        let permit_a = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(1, fixture.lease, binding_a, automation_a),
                &action_a,
                &assessment_a,
            )
            .expect("origin a write")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("approved a write needed human"),
        };
        let action_b = prepared_click(&observation_b, 2, SemanticEffectClass::ExternalWrite);
        let assessment_b = AgentEffectAssessment::new(
            &action_b,
            origin_b.clone(),
            SemanticEffectClass::ExternalWrite,
        );
        let permit_b = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(2, fixture.lease, binding_b, automation_b),
                &action_b,
                &assessment_b,
            )
            .expect("origin b write")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("approved b write needed human"),
        };
        assert_eq!(fixture.policy.pending_effects(), 2);
        assert_eq!(fixture.policy.pending_origin_writes(), 2);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 2);
        let active_b = fixture
            .policy
            .dispatch_semantic_effect(
                permit_b,
                &action_b,
                effect_dispatch_request(1, binding_b, automation_b),
            )
            .expect("dispatch b out of authorization order");
        let active_a = fixture
            .policy
            .dispatch_semantic_effect(
                permit_a,
                &action_a,
                effect_dispatch_request(2, binding_a, automation_a),
            )
            .expect("dispatch a");
        let receipt_a = fixture
            .policy
            .settle_failed_semantic_effect(
                active_a,
                &action_a,
                SemanticActionFailure::BackendRefused,
            )
            .expect("settle a first");
        let receipt_b = fixture
            .policy
            .settle_failed_semantic_effect(
                active_b,
                &action_b,
                SemanticActionFailure::BackendRefused,
            )
            .expect("settle b second");
        assert_eq!(receipt_a.receipt().id().get(), 1);
        assert_eq!(receipt_b.receipt().id().get(), 2);
        assert_eq!(fixture.policy.pending_effects(), 0);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 0);
        assert_eq!(fixture.policy.accounting().consumed_operations(), 4);
        assert!(!fixture.policy.is_sealed());
        let debug = format!("{:?}", fixture.policy);
        assert!(!debug.contains("parallel-origin-a"));
        assert!(!debug.contains("parallel-origin-b"));
    }

    #[test]
    fn bounded_read_cannot_authorize_a_guessed_undisclosed_reference() {
        let source = origin("read-reference");
        let (mut registry, context) = make_context_registry(117, 118, 119);
        let observation = read_limited_actionable_observation(context, source.clone(), 1);
        registry
            .acknowledge_observation(context.identity().id(), context)
            .expect("observation current");
        let automation = registry
            .automation_state(context.identity().id())
            .expect("automation state");
        let binding = account(context, NOW - 1);
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(NOW - 2),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::try_new(1, 1_024).expect("read budget"),
        )
        .expect("bounded read");
        assert_eq!(read.fragments().len(), 1);
        assert_eq!(read.fragments()[0].provenance().reference().get(), 2);
        let payload = read_payload(&read, 10);
        let mut fixture = policy_fixture(
            117,
            118,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
            run_budget(10, 1_000, 10_000),
        );
        let admission = fixture
            .policy
            .prepare_read_input(
                call_request(1, fixture.lease, binding, 0, 0, 0, NOW),
                &read,
                &payload,
            )
            .expect("read admission");
        let delivery = payload
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("read delivery");
        let active = fixture
            .policy
            .commit_read_input(admission, &delivery)
            .expect("read commit");
        fixture
            .policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 10, 0, 0)
            .expect("model settlement");
        assert!(!fixture.policy.taints()[0]
            .contains_reference(SemanticReferenceId::new(3).expect("reference")));

        let action = prepared_click(&observation, 3, SemanticEffectClass::LocalWrite);
        let assessment =
            AgentEffectAssessment::new(&action, source, SemanticEffectClass::LocalWrite);
        assert_eq!(
            fixture
                .policy
                .authorize_semantic_effect(
                    effect_request(1, fixture.lease, binding, automation),
                    &action,
                    &assessment,
                )
                .expect_err("undisclosed reference"),
            AgentPolicyError::ModelSourceMissing
        );
        assert_eq!(fixture.policy.pending_effects(), 0);
        assert_eq!(fixture.policy.accounting().consumed_operations(), 1);
    }

    #[test]
    fn source_to_sink_flow_requires_an_exact_global_rule() {
        let source = origin("flow-source");
        let destination = origin("flow-destination");
        let source_context = make_context(127, 128, 129);
        let source_observation = mixed_observation(source_context, source.clone(), 1);
        let (mut destination_registry, destination_context) = make_context_registry(127, 128, 130);
        let destination_observation =
            actionable_observation(destination_context, destination.clone(), 2);
        destination_registry
            .acknowledge_observation(destination_context.identity().id(), destination_context)
            .expect("destination observed");
        let automation = destination_registry
            .automation_state(destination_context.identity().id())
            .expect("automation state");
        let source_binding = account(source_context, NOW - 1);
        let destination_binding = account(destination_context, NOW - 1);
        let action = prepared_click(&destination_observation, 2, SemanticEffectClass::LocalWrite);
        let assessment = AgentEffectAssessment::new(
            &action,
            destination.clone(),
            SemanticEffectClass::LocalWrite,
        );

        let mut denied = policy_fixture_with_flows(
            127,
            128,
            vec![source.clone(), destination.clone()],
            Vec::new(),
            &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
            run_budget(10, 1_000, 10_000),
        );
        commit_observation_to_model(
            &mut denied.policy,
            denied.lease,
            1,
            source_binding,
            &source_observation,
        );
        commit_observation_to_model(
            &mut denied.policy,
            denied.lease,
            2,
            destination_binding,
            &destination_observation,
        );
        let transition = match denied
            .policy
            .authorize_semantic_effect(
                effect_request(1, denied.lease, destination_binding, automation),
                &action,
                &assessment,
            )
            .expect("data-flow decision")
        {
            AgentEffectAuthorization::NeedsHuman(transition) => transition,
            AgentEffectAuthorization::Permit(_) => panic!("unapproved flow admitted"),
        };
        assert_eq!(transition.reason(), AgentNeedsHumanReason::DataFlowApproval);
        assert_eq!(denied.policy.pending_effects(), 0);
        assert_eq!(denied.policy.accounting().reserved_operations(), 0);

        let flow = AgentDataFlowRule::try_new(
            source.clone(),
            AgentAccountScope::Anonymous,
            destination.clone(),
            AgentAccountScope::Anonymous,
            SemanticSensitivity::Sensitive,
            effects(&[SemanticEffectClass::LocalWrite]),
        )
        .expect("flow");
        let mut allowed = policy_fixture_with_flows(
            127,
            128,
            vec![source, destination],
            vec![flow],
            &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
            run_budget(10, 1_000, 10_000),
        );
        commit_observation_to_model(
            &mut allowed.policy,
            allowed.lease,
            1,
            source_binding,
            &source_observation,
        );
        commit_observation_to_model(
            &mut allowed.policy,
            allowed.lease,
            2,
            destination_binding,
            &destination_observation,
        );
        let permit = match allowed
            .policy
            .authorize_semantic_effect(
                effect_request(1, allowed.lease, destination_binding, automation),
                &action,
                &assessment,
            )
            .expect("approved data flow")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("exact flow refused"),
        };
        assert_eq!(allowed.policy.accounting().reserved_operations(), 1);
        allowed
            .policy
            .cancel_semantic_effect(permit, AgentEffectCancellation::Refused)
            .expect("release flow permit");
    }

    #[test]
    fn capability_scope_and_unfresh_context_pause_without_execution_authority() {
        let source = origin("human-boundary");
        let (mut registry, context) = make_context_registry(137, 138, 139);
        let observation = actionable_observation(context, source.clone(), 1);
        let binding = account(context, NOW - 1);
        let mut fixture = policy_fixture(
            137,
            138,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[
                SemanticEffectClass::Read,
                SemanticEffectClass::CapabilityBoundary,
            ],
            run_budget(10, 1_000, 10_000),
        );
        commit_observation_to_model(&mut fixture.policy, fixture.lease, 1, binding, &observation);
        let action = prepared_click(&observation, 2, SemanticEffectClass::CapabilityBoundary);
        let assessment =
            AgentEffectAssessment::new(&action, source, SemanticEffectClass::CapabilityBoundary);
        let unfresh = registry
            .automation_state(context.identity().id())
            .expect("unfresh state");
        assert_eq!(
            fixture
                .policy
                .authorize_semantic_effect(
                    effect_request(1, fixture.lease, binding, unfresh),
                    &action,
                    &assessment,
                )
                .expect_err("fresh observation required"),
            AgentPolicyError::ContextNotAutomatable
        );
        registry
            .acknowledge_observation(context.identity().id(), context)
            .expect("observation current");
        let automation = registry
            .automation_state(context.identity().id())
            .expect("automation state");
        let transition = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(2, fixture.lease, binding, automation),
                &action,
                &assessment,
            )
            .expect("capability decision")
        {
            AgentEffectAuthorization::NeedsHuman(transition) => transition,
            AgentEffectAuthorization::Permit(_) => panic!("capability boundary admitted"),
        };
        assert_eq!(
            transition.reason(),
            AgentNeedsHumanReason::CapabilityBoundary
        );
        assert_eq!(fixture.policy.pending_effects(), 0);
        assert_eq!(fixture.policy.accounting().consumed_operations(), 1);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 0);
    }

    #[test]
    fn effect_boundary_blocks_pending_model_calls_and_exhausted_operation_budgets() {
        let source = origin("effect-budget");
        let (mut registry, context) = make_context_registry(142, 143, 144);
        let observation = actionable_observation(context, source.clone(), 1);
        registry
            .acknowledge_observation(context.identity().id(), context)
            .expect("observation current");
        let automation = registry
            .automation_state(context.identity().id())
            .expect("automation state");
        let binding = account(context, NOW - 1);
        let action = prepared_click(&observation, 2, SemanticEffectClass::LocalWrite);
        let assessment =
            AgentEffectAssessment::new(&action, source.clone(), SemanticEffectClass::LocalWrite);

        let mut pending = policy_fixture(
            142,
            143,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
            run_budget(10, 1_000, 10_000),
        );
        let payload = observation_payload(&observation, 10);
        let admission = pending
            .policy
            .prepare_observation_input(
                call_request(1, pending.lease, binding, 0, 0, 0, NOW),
                &observation,
                &payload,
            )
            .expect("pending model call");
        assert_eq!(
            pending
                .policy
                .authorize_semantic_effect(
                    effect_request(1, pending.lease, binding, automation),
                    &action,
                    &assessment,
                )
                .expect_err("model call must settle first"),
            AgentPolicyError::ModelCallPending
        );
        pending
            .policy
            .cancel_prepared_input(admission, AgentModelInputCancellation::Cancelled)
            .expect("cancel model call");

        let mut exhausted = policy_fixture(
            142,
            143,
            source,
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
            run_budget(1, 1_000, 10_000),
        );
        commit_observation_to_model(
            &mut exhausted.policy,
            exhausted.lease,
            1,
            binding,
            &observation,
        );
        assert_eq!(
            exhausted
                .policy
                .authorize_semantic_effect(
                    effect_request(1, exhausted.lease, binding, automation),
                    &action,
                    &assessment,
                )
                .expect_err("operation budget exhausted"),
            AgentPolicyError::Budget
        );
        assert_eq!(exhausted.policy.pending_effects(), 0);
        assert_eq!(exhausted.policy.accounting().consumed_operations(), 1);
        assert_eq!(exhausted.policy.accounting().reserved_operations(), 0);
    }

    #[test]
    fn effect_token_or_action_substitution_seals_and_retains_ambiguity() {
        let source = origin("effect-mismatch");
        let (mut registry, context) = make_context_registry(145, 146, 147);
        let observation = actionable_observation(context, source.clone(), 1);
        registry
            .acknowledge_observation(context.identity().id(), context)
            .expect("observation current");
        let automation = registry
            .automation_state(context.identity().id())
            .expect("automation state");
        let binding = account(context, NOW - 1);
        let mut fixture = policy_fixture(
            145,
            146,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite],
            run_budget(10, 1_000, 10_000),
        );
        commit_observation_to_model(&mut fixture.policy, fixture.lease, 1, binding, &observation);
        let action = prepared_click(&observation, 2, SemanticEffectClass::LocalWrite);
        let wrong_action = prepared_click(&observation, 2, SemanticEffectClass::Read);
        let assessment =
            AgentEffectAssessment::new(&action, source, SemanticEffectClass::LocalWrite);
        let permit = match fixture
            .policy
            .authorize_semantic_effect(
                effect_request(1, fixture.lease, binding, automation),
                &action,
                &assessment,
            )
            .expect("effect decision")
        {
            AgentEffectAuthorization::Permit(permit) => permit,
            AgentEffectAuthorization::NeedsHuman(_) => panic!("same-origin effect was in scope"),
        };
        assert_eq!(
            fixture
                .policy
                .dispatch_semantic_effect(
                    permit,
                    &wrong_action,
                    effect_dispatch_request(1, binding, automation),
                )
                .expect_err("action substitution"),
            AgentPolicyError::EffectSettlementMismatch
        );
        assert!(fixture.policy.is_sealed());
        assert_eq!(fixture.policy.pending_effects(), 1);
        assert_eq!(fixture.policy.accounting().reserved_operations(), 1);
    }

    #[test]
    fn taint_reference_inventory_has_a_run_global_hard_ceiling() {
        let source = origin("reference-limit");
        let context = make_context(147, 148, 149);
        let observation = actionable_observation(context, source.clone(), 2);
        let binding = account(context, NOW - 1);
        let mut fixture = policy_fixture(
            147,
            148,
            source.clone(),
            SemanticSensitivity::Sensitive,
            &[SemanticEffectClass::Read],
            run_budget(10, 1_000, 10_000),
        );
        fixture.policy.taints = (0..8_u128)
            .map(|index| AgentTaintCohort {
                context: make_context(147, 148, 1_000 + index),
                observation: SemanticObservationId::new(1).expect("observation"),
                observation_generation: SemanticObservationGeneration::INITIAL,
                source_guard: [index as u8; 32],
                account: AgentAccountScope::Anonymous,
                origin: source.clone(),
                sensitivity: SemanticSensitivity::Public,
                trust: SemanticTrust::BrowserDerived,
                attested_at: AgentPolicyInstant::from_millis(NOW - 1),
                references: (1..=crate::MAX_SEMANTIC_NODES as u16)
                    .map(|value| SemanticReferenceId::new(value).expect("reference"))
                    .collect(),
            })
            .collect();
        assert_eq!(
            fixture
                .policy
                .taints()
                .iter()
                .map(AgentTaintCohort::reference_count)
                .sum::<usize>(),
            MAX_AGENT_TAINT_REFERENCES
        );
        let payload = observation_payload(&observation, 10);
        assert_eq!(
            fixture
                .policy
                .prepare_observation_input(
                    call_request(1, fixture.lease, binding, 0, 0, 0, NOW),
                    &observation,
                    &payload,
                )
                .expect_err("reference ceiling"),
            AgentPolicyError::TaintReferenceLimit
        );
        assert_eq!(fixture.policy.pending_model_calls(), 0);
    }

    #[test]
    fn taint_union_is_exact_bounded_and_merges_conservatively() {
        let source = origin("taint");
        let mut committed = Vec::new();
        for id in 1..=MAX_AGENT_TAINT_COHORTS as u128 {
            committed.push(AgentTaintCohort {
                context: make_context(97, 98, id),
                observation: SemanticObservationId::new(id as u64).expect("observation"),
                observation_generation: SemanticObservationGeneration::INITIAL,
                source_guard: [id as u8; 32],
                account: AgentAccountScope::Anonymous,
                origin: source.clone(),
                sensitivity: SemanticSensitivity::Public,
                trust: SemanticTrust::BrowserDerived,
                attested_at: AgentPolicyInstant::from_millis(NOW),
                references: vec![SemanticReferenceId::new(1).expect("reference")],
            });
        }
        let extra = AgentTaintCohort {
            context: make_context(97, 98, 10_000),
            observation: SemanticObservationId::new(10_000).expect("observation"),
            observation_generation: SemanticObservationGeneration::INITIAL,
            source_guard: [0xff; 32],
            account: AgentAccountScope::Anonymous,
            origin: source.clone(),
            sensitivity: SemanticSensitivity::Public,
            trust: SemanticTrust::BrowserDerived,
            attested_at: AgentPolicyInstant::from_millis(NOW),
            references: vec![SemanticReferenceId::new(1).expect("reference")],
        };
        assert_eq!(
            projected_taint_usage(&committed, &[], std::slice::from_ref(&extra)),
            Ok((MAX_AGENT_TAINT_COHORTS + 1, MAX_AGENT_TAINT_COHORTS + 1))
        );
        assert_eq!(
            projected_taint_usage(&committed, &[], std::slice::from_ref(&committed[0])),
            Ok((MAX_AGENT_TAINT_COHORTS, MAX_AGENT_TAINT_COHORTS))
        );

        let exact_context = committed[0].context();
        let mut merged = vec![AgentTaintCohort {
            context: exact_context,
            observation: committed[0].observation(),
            observation_generation: committed[0].observation_generation(),
            source_guard: committed[0].source_guard,
            account: AgentAccountScope::Anonymous,
            origin: source.clone(),
            sensitivity: SemanticSensitivity::Public,
            trust: SemanticTrust::BrowserDerived,
            attested_at: AgentPolicyInstant::from_millis(NOW),
            references: vec![SemanticReferenceId::new(1).expect("reference")],
        }];
        merge_taint(
            &mut merged,
            AgentTaintCohort {
                context: exact_context,
                observation: committed[0].observation(),
                observation_generation: committed[0].observation_generation(),
                source_guard: committed[0].source_guard,
                account: AgentAccountScope::Anonymous,
                origin: source,
                sensitivity: SemanticSensitivity::Sensitive,
                trust: SemanticTrust::UntrustedPage,
                attested_at: AgentPolicyInstant::from_millis(NOW - 1),
                references: vec![
                    SemanticReferenceId::new(1).expect("reference"),
                    SemanticReferenceId::new(2).expect("reference"),
                ],
            },
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].sensitivity(), SemanticSensitivity::Sensitive);
        assert_eq!(merged[0].trust(), SemanticTrust::UntrustedPage);
        assert_eq!(merged[0].reference_count(), 2);
        assert_eq!(
            merged[0].attested_at(),
            AgentPolicyInstant::from_millis(NOW - 1)
        );
    }
}
