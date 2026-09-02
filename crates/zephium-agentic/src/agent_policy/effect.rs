//! Bounded semantic effect authorization over committed model-context taint.
//!
//! This child of the mutable run policy can reserve one operation per exact
//! prepared action and mint a non-cloneable permit. It owns no browser, page,
//! timer, provider, worker, or native input. The imperative shell must still
//! resolve/revalidate each action, dispatch a fixed backend with its permit,
//! settle one absolute deadline, and independently verify the effect.

use std::fmt;
use std::num::NonZeroU64;

use sha2::{Digest, Sha256};

use super::*;
use crate::{
    AgentRunManifestId, ContextAutomationState, ContextControl, SemanticActionExecutionApplied,
    SemanticActionExecutionCoordinatorRefusal, SemanticActionFailure,
    SemanticActionSettlementAdmissionRefusal, SemanticActionSettlementRefusal,
    SemanticActionVerificationRefusal, SemanticActionVerifiedTerminal, SemanticEffectProofKind,
    SemanticPreparedAction, SemanticSettleTracker, SemanticVerificationError,
    SemanticVerifiedAction,
};

/// Maximum prepared or dispatched semantic effects in one run policy.
pub const MAX_AGENT_PENDING_EFFECTS: usize = 4;

/// Monotonic shell-minted identity for one exact effect-authorization attempt.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentEffectId(NonZeroU64);

impl AgentEffectId {
    /// Constructs one nonzero process-local effect identity.
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

impl fmt::Debug for AgentEffectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentEffectId([redacted])")
    }
}

/// Trusted fixed-classifier result for one exact prepared semantic action.
///
/// Construction is deliberately separate from the model's declared effect.
/// The selected native/service adapter must derive the actual effect and
/// canonical destination independently before policy sees this value.
#[derive(Clone, Eq, PartialEq)]
pub struct AgentEffectAssessment {
    action_guard: [u8; 32],
    source_context: ContextJoin,
    destination_origin: SemanticOrigin,
    actual_effect: SemanticEffectClass,
}

impl AgentEffectAssessment {
    /// Binds independently derived destination/effect facts to one prepared action.
    pub fn new(
        action: &SemanticPreparedAction,
        destination_origin: SemanticOrigin,
        actual_effect: SemanticEffectClass,
    ) -> Self {
        Self {
            action_guard: action.verification_guard(),
            source_context: action.frame().context(),
            destination_origin,
            actual_effect,
        }
    }

    /// Exact pre-execution context assessed by the fixed adapter.
    pub const fn source_context(&self) -> ContextJoin {
        self.source_context
    }

    /// Canonical independently derived effect destination.
    pub const fn destination_origin(&self) -> &SemanticOrigin {
        &self.destination_origin
    }

    /// Independently derived actual effect class.
    pub const fn actual_effect(&self) -> SemanticEffectClass {
        self.actual_effect
    }

    /// Whether this assessment belongs to the exact prepared action guard.
    pub fn matches_action(&self, action: &SemanticPreparedAction) -> bool {
        self.source_context == action.frame().context()
            && self.action_guard == action.verification_guard()
    }
}

impl fmt::Debug for AgentEffectAssessment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentEffectAssessment")
            .field("action_guard", &"[redacted]")
            .field("source_context", &"[redacted]")
            .field("destination_origin", &"[redacted]")
            .field("actual_effect", &self.actual_effect)
            .finish()
    }
}

/// Complete trusted-shell request for one semantic effect decision.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentEffectRequest {
    id: AgentEffectId,
    lease: AgentPlanLeaseId,
    account: AgentContextAccountBinding,
    automation: ContextAutomationState,
    now: AgentPolicyInstant,
}

impl AgentEffectRequest {
    /// Joins exact effect, lease, account, current context state, and policy time.
    pub const fn new(
        id: AgentEffectId,
        lease: AgentPlanLeaseId,
        account: AgentContextAccountBinding,
        automation: ContextAutomationState,
        now: AgentPolicyInstant,
    ) -> Self {
        Self {
            id,
            lease,
            account,
            automation,
            now,
        }
    }

    /// Monotonic exact effect identity.
    pub const fn id(self) -> AgentEffectId {
        self.id
    }

    /// Mutable plan lease charged by this effect.
    pub const fn lease(self) -> AgentPlanLeaseId {
        self.lease
    }

    /// Exact current context/account attestation.
    pub const fn account(self) -> AgentContextAccountBinding {
        self.account
    }

    /// Atomically joined current lifecycle/control/freshness state.
    pub const fn automation(self) -> ContextAutomationState {
        self.automation
    }

    /// Trusted monotonic authorization time.
    pub const fn now(self) -> AgentPolicyInstant {
        self.now
    }
}

impl fmt::Debug for AgentEffectRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentEffectRequest")
            .field("id", &self.id)
            .field("lease", &self.lease)
            .field("account", &self.account)
            .field("automation", &self.automation)
            .field("now", &self.now)
            .finish()
    }
}

/// Trusted-shell facts sampled immediately before fixed-backend dispatch.
///
/// Construction is not lifecycle or account proof. The serialized context
/// actor must source the complete joined state and account attestation at the
/// last pre-native boundary; policy re-checks both against the permit.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentEffectDispatchRequest {
    attempt: SemanticActionAttemptId,
    account: AgentContextAccountBinding,
    automation: ContextAutomationState,
    now: AgentPolicyInstant,
}

impl AgentEffectDispatchRequest {
    /// Joins native attempt identity to current lifecycle/account policy facts.
    pub const fn new(
        attempt: SemanticActionAttemptId,
        account: AgentContextAccountBinding,
        automation: ContextAutomationState,
        now: AgentPolicyInstant,
    ) -> Self {
        Self {
            attempt,
            account,
            automation,
            now,
        }
    }

    /// Strictly increasing native action attempt identity.
    pub const fn attempt(self) -> SemanticActionAttemptId {
        self.attempt
    }

    /// Current exact context/account attestation.
    pub const fn account(self) -> AgentContextAccountBinding {
        self.account
    }

    /// Current atomically joined lifecycle and context authority.
    pub const fn automation(self) -> ContextAutomationState {
        self.automation
    }

    /// Trusted monotonic pre-dispatch time.
    pub const fn now(self) -> AgentPolicyInstant {
        self.now
    }
}

impl fmt::Debug for AgentEffectDispatchRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentEffectDispatchRequest")
            .field("attempt", &self.attempt)
            .field("account", &self.account)
            .field("automation", &self.automation)
            .field("now", &self.now)
            .finish()
    }
}

/// Closed reason why automation paused without minting effect authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentNeedsHumanReason {
    /// A person currently owns the exact context's input.
    HumanControl,
    /// The effect requires a typed capability adapter not yet automated.
    CapabilityBoundary,
    /// Current manifest or node scope must be explicitly expanded.
    ScopeExpansion,
    /// An exact non-secret source-to-sink rule is absent.
    DataFlowApproval,
    /// A non-read cross-origin action lacks destination-account proof.
    CrossOriginWrite,
}

/// Content-free non-authorizing supervisor transition to human review/control.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentNeedsHumanTransition {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    node: AgentPlanNodeId,
    context: ContextJoin,
    effect: SemanticEffectClass,
    reason: AgentNeedsHumanReason,
}

impl AgentNeedsHumanTransition {
    /// Exact immutable manifest revision that could not authorize automation.
    pub const fn manifest(self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact plan node that requested the transition.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }

    /// Exact context/document/cancellation authority at the decision.
    pub const fn context(self) -> ContextJoin {
        self.context
    }

    /// Independently derived actual effect class.
    pub const fn effect(self) -> SemanticEffectClass {
        self.effect
    }

    /// Closed reason requiring human/supervisor handling.
    pub const fn reason(self) -> AgentNeedsHumanReason {
        self.reason
    }

    /// Whether this transition belongs to one exact canonical manifest revision.
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
        node: AgentPlanNodeId,
        context: ContextJoin,
        effect: SemanticEffectClass,
        reason: AgentNeedsHumanReason,
    ) -> Self {
        Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            node,
            context,
            effect,
            reason,
        }
    }
}

impl fmt::Debug for AgentNeedsHumanTransition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentNeedsHumanTransition")
            .field("manifest", &self.manifest)
            .field("manifest_guard", &"[redacted]")
            .field("node", &self.node)
            .field("context", &"[redacted]")
            .field("effect", &self.effect)
            .field("reason", &self.reason)
            .finish()
    }
}

/// Permit or explicit non-authorizing human transition from policy.
#[must_use]
#[derive(Debug)]
pub enum AgentEffectAuthorization {
    /// Exact effect reservation may continue through revalidation to dispatch.
    Permit(AgentEffectPermit),
    /// Automation paused; no operation budget or execution authority was reserved.
    NeedsHuman(AgentNeedsHumanTransition),
}

/// Non-cloneable pre-dispatch operation reservation.
#[must_use]
pub struct AgentEffectPermit {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    id: AgentEffectId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    effect: SemanticEffectClass,
    action_guard: [u8; 32],
    guard: [u8; 32],
}

impl AgentEffectPermit {
    /// Exact immutable manifest revision governing this authorization.
    pub const fn manifest(&self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact effect identity.
    pub const fn id(&self) -> AgentEffectId {
        self.id
    }

    /// Exact mutable lease holding one operation reservation.
    pub const fn lease(&self) -> AgentPlanLeaseId {
        self.lease
    }

    /// Exact approved plan node.
    pub const fn node(&self) -> AgentPlanNodeId {
        self.node
    }

    /// Independently classified actual effect.
    pub const fn effect(&self) -> SemanticEffectClass {
        self.effect
    }

    /// Whether this permit belongs to the exact revalidated action checkpoint.
    pub fn matches_action(&self, action: &SemanticPreparedAction) -> bool {
        self.action_guard == action.verification_guard()
    }

    /// Whether this permit belongs to one exact canonical manifest revision.
    pub(crate) fn matches_manifest_revision(
        &self,
        manifest: AgentRunManifestId,
        manifest_guard: [u8; 32],
    ) -> bool {
        self.manifest == manifest && self.manifest_guard == manifest_guard
    }

    #[cfg(test)]
    pub(crate) const fn for_progress_test(
        manifest: &AgentRunManifest,
        id: AgentEffectId,
        lease: AgentPlanLeaseId,
        node: AgentPlanNodeId,
        effect: SemanticEffectClass,
    ) -> Self {
        Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            id,
            lease,
            node,
            effect,
            action_guard: [0; 32],
            guard: [0; 32],
        }
    }
}

impl fmt::Debug for AgentEffectPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentEffectPermit")
            .field("manifest", &self.manifest)
            .field("manifest_guard", &"[redacted]")
            .field("id", &self.id)
            .field("lease", &self.lease)
            .field("node", &self.node)
            .field("effect", &self.effect)
            .field("action_guard", &"[redacted]")
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Non-cloneable exact effect after final policy revalidation accepted dispatch.
#[must_use]
pub struct AgentActiveEffect {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    id: AgentEffectId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    effect: SemanticEffectClass,
    attempt: SemanticActionAttemptId,
    action_guard: [u8; 32],
    guard: [u8; 32],
}

impl AgentActiveEffect {
    /// Exact immutable manifest revision governing this dispatched effect.
    pub const fn manifest(&self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact effect identity.
    pub const fn id(&self) -> AgentEffectId {
        self.id
    }

    /// Exact mutable plan lease.
    pub const fn lease(&self) -> AgentPlanLeaseId {
        self.lease
    }

    /// Exact approved plan node.
    pub const fn node(&self) -> AgentPlanNodeId {
        self.node
    }

    /// Independently classified effect.
    pub const fn effect(&self) -> SemanticEffectClass {
        self.effect
    }

    /// Exact native execution attempt identity.
    pub const fn attempt(&self) -> SemanticActionAttemptId {
        self.attempt
    }

    /// Whether this dispatched policy authority belongs to one exact action checkpoint.
    pub fn matches_action(&self, action: &SemanticPreparedAction) -> bool {
        self.effect == action.effect() && self.action_guard == action.verification_guard()
    }

    /// Whether this effect belongs to one exact canonical manifest revision.
    pub(crate) fn matches_manifest_revision(
        &self,
        manifest: AgentRunManifestId,
        manifest_guard: [u8; 32],
    ) -> bool {
        self.manifest == manifest && self.manifest_guard == manifest_guard
    }

    #[cfg(test)]
    pub(crate) const fn for_progress_test(
        manifest: &AgentRunManifest,
        id: AgentEffectId,
        lease: AgentPlanLeaseId,
        node: AgentPlanNodeId,
        effect: SemanticEffectClass,
        attempt: SemanticActionAttemptId,
    ) -> Self {
        Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            id,
            lease,
            node,
            effect,
            attempt,
            action_guard: [0; 32],
            guard: [0; 32],
        }
    }

    #[cfg(test)]
    pub(crate) fn for_execution_test(
        action: &SemanticPreparedAction,
        attempt: SemanticActionAttemptId,
    ) -> Self {
        Self {
            manifest: AgentRunManifestId::from_raw(801),
            manifest_guard: [0x81; 32],
            id: AgentEffectId::new(attempt.get()).expect("test effect identity"),
            lease: AgentPlanLeaseId::from_raw(803),
            node: AgentPlanNodeId::from_raw(804),
            effect: action.effect(),
            attempt,
            action_guard: action.verification_guard(),
            guard: [0x82; 32],
        }
    }
}

impl fmt::Debug for AgentActiveEffect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentActiveEffect")
            .field("manifest", &self.manifest)
            .field("manifest_guard", &"[redacted]")
            .field("id", &self.id)
            .field("lease", &self.lease)
            .field("node", &self.node)
            .field("effect", &self.effect)
            .field("attempt", &self.attempt)
            .field("action_guard", &"[redacted]")
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Pre-dispatch terminal class that releases the reserved operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentEffectCancellation {
    /// Fixed executor refused before a native attempt began.
    Refused,
    /// Exact cancellation won before dispatch.
    Cancelled,
    /// Resolve/revalidation found the prepared authority stale.
    RevalidationFailed,
}

/// Terminal dispatched-effect result retained without page content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentEffectSettlement {
    /// Independent verifier proved the exact declared postcondition.
    Verified(SemanticEffectProofKind),
    /// Dispatched attempt terminated under the closed action failure taxonomy.
    Failed(SemanticActionFailure),
}

/// Content-free receipt after one dispatched effect consumes its operation.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentEffectReceipt {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    id: AgentEffectId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    effect: SemanticEffectClass,
    attempt: SemanticActionAttemptId,
    settlement: AgentEffectSettlement,
    action_guard: [u8; 32],
}

impl AgentEffectReceipt {
    /// Exact immutable manifest revision that accounted this effect.
    pub const fn manifest(self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact effect identity.
    pub const fn id(self) -> AgentEffectId {
        self.id
    }

    /// Exact consumed mutable lease.
    pub const fn lease(self) -> AgentPlanLeaseId {
        self.lease
    }

    /// Exact approved plan node.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }

    /// Independently classified actual effect.
    pub const fn effect(self) -> SemanticEffectClass {
        self.effect
    }

    /// Exact native action attempt.
    pub const fn attempt(self) -> SemanticActionAttemptId {
        self.attempt
    }

    /// Verified proof class or terminal typed failure.
    pub const fn settlement(self) -> AgentEffectSettlement {
        self.settlement
    }

    pub(crate) fn matches_action(self, action: &SemanticPreparedAction) -> bool {
        self.effect == action.effect() && self.action_guard == action.verification_guard()
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
        id: AgentEffectId,
        lease: AgentPlanLeaseId,
        node: AgentPlanNodeId,
        effect: SemanticEffectClass,
        attempt: SemanticActionAttemptId,
        settlement: AgentEffectSettlement,
    ) -> Self {
        Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            id,
            lease,
            node,
            effect,
            attempt,
            settlement,
            action_guard: [0; 32],
        }
    }
}

impl fmt::Debug for AgentEffectReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentEffectReceipt")
            .field("manifest", &self.manifest)
            .field("manifest_guard", &"[redacted]")
            .field("id", &self.id)
            .field("lease", &self.lease)
            .field("node", &self.node)
            .field("effect", &self.effect)
            .field("attempt", &self.attempt)
            .field("settlement", &self.settlement)
            .field("action_guard", &"[redacted]")
            .finish()
    }
}

/// Policy-accounted verified effect retaining its exact downstream proof state.
#[must_use]
pub struct AgentVerifiedSemanticEffect {
    receipt: AgentEffectReceipt,
    execution: SemanticActionExecutionApplied,
    settlement: SemanticSettleTracker,
    verified: SemanticVerifiedAction,
}

impl AgentVerifiedSemanticEffect {
    /// Exact charged policy receipt.
    pub const fn receipt(&self) -> AgentEffectReceipt {
        self.receipt
    }

    /// Content-free backend attribution and native execution timing.
    pub const fn execution(&self) -> SemanticActionExecutionApplied {
        self.execution
    }

    /// Exact terminal settlement state consumed by verification.
    pub const fn settlement(&self) -> &SemanticSettleTracker {
        &self.settlement
    }

    /// Opaque independently established proof for action-result finalization.
    pub const fn verified(&self) -> &SemanticVerifiedAction {
        &self.verified
    }

    #[cfg(test)]
    pub(crate) fn for_pipeline_test(
        terminal: SemanticActionVerifiedTerminal,
        action: &SemanticPreparedAction,
    ) -> Self {
        let (active, execution, settlement, verified) = terminal.into_parts();
        assert!(active.matches_action(action));
        assert_eq!(active.attempt, verified.attempt());
        assert_eq!(settlement.attempt(), verified.attempt());
        assert_eq!(
            settlement.status(),
            crate::SemanticSettleStatus::ReadyForVerification
        );
        assert!(settlement.matches_action(action));
        let receipt = AgentEffectReceipt {
            manifest: active.manifest,
            manifest_guard: active.manifest_guard,
            id: active.id,
            lease: active.lease,
            node: active.node,
            effect: active.effect,
            attempt: active.attempt,
            settlement: AgentEffectSettlement::Verified(verified.proof()),
            action_guard: active.action_guard,
        };
        Self {
            receipt,
            execution,
            settlement,
            verified,
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        AgentEffectReceipt,
        SemanticActionExecutionApplied,
        SemanticSettleTracker,
        SemanticVerifiedAction,
    ) {
        (self.receipt, self.execution, self.settlement, self.verified)
    }
}

impl fmt::Debug for AgentVerifiedSemanticEffect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentVerifiedSemanticEffect")
            .field("receipt", &self.receipt)
            .field("execution", &self.execution)
            .field("settlement", &self.settlement)
            .field("verified", &self.verified)
            .finish()
    }
}

enum AgentFailedSemanticEffectEvidence {
    AfterExecution {
        execution: SemanticActionExecutionApplied,
        settlement: SemanticSettleTracker,
        verification_error: SemanticVerificationError,
    },
}

/// Policy-accounted semantic action failure with optional terminal metrics.
///
/// Failures before applied native execution allocate no evidence box. A
/// verification refusal retains one bounded execution/settlement record until
/// batch terminalization consumes it.
#[must_use]
pub struct AgentFailedSemanticEffect {
    receipt: AgentEffectReceipt,
    failure: SemanticActionFailure,
    evidence: Option<Box<AgentFailedSemanticEffectEvidence>>,
}

impl AgentFailedSemanticEffect {
    /// Exact charged failed policy receipt.
    pub const fn receipt(&self) -> AgentEffectReceipt {
        self.receipt
    }

    /// Exact charged closed failure.
    pub const fn failure(&self) -> SemanticActionFailure {
        self.failure
    }

    /// Content-free backend attribution when native execution applied.
    pub fn execution(&self) -> Option<SemanticActionExecutionApplied> {
        self.evidence.as_deref().map(|evidence| match evidence {
            AgentFailedSemanticEffectEvidence::AfterExecution { execution, .. } => *execution,
        })
    }

    /// Exact terminal settlement retained for a verification refusal.
    pub fn settlement(&self) -> Option<&SemanticSettleTracker> {
        self.evidence.as_deref().map(|evidence| match evidence {
            AgentFailedSemanticEffectEvidence::AfterExecution { settlement, .. } => settlement,
        })
    }

    /// Closed verifier refusal when independent verification was reached.
    pub fn verification_error(&self) -> Option<SemanticVerificationError> {
        self.evidence.as_deref().map(|evidence| match evidence {
            AgentFailedSemanticEffectEvidence::AfterExecution {
                verification_error, ..
            } => *verification_error,
        })
    }

    pub(crate) fn matches_action(&self, action: &SemanticPreparedAction) -> bool {
        self.receipt.matches_action(action)
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        AgentEffectReceipt,
        SemanticActionFailure,
        Option<(
            SemanticActionExecutionApplied,
            SemanticSettleTracker,
            SemanticVerificationError,
        )>,
    ) {
        let evidence = self.evidence.map(|evidence| match *evidence {
            AgentFailedSemanticEffectEvidence::AfterExecution {
                execution,
                settlement,
                verification_error,
            } => (execution, settlement, verification_error),
        });
        (self.receipt, self.failure, evidence)
    }

    #[cfg(test)]
    pub(crate) fn for_batch_test(
        action: &SemanticPreparedAction,
        attempt: SemanticActionAttemptId,
        failure: SemanticActionFailure,
    ) -> Self {
        let active = AgentActiveEffect::for_execution_test(action, attempt);
        Self {
            receipt: AgentEffectReceipt {
                manifest: active.manifest,
                manifest_guard: active.manifest_guard,
                id: active.id,
                lease: active.lease,
                node: active.node,
                effect: active.effect,
                attempt: active.attempt,
                settlement: AgentEffectSettlement::Failed(failure),
                action_guard: active.action_guard,
            },
            failure,
            evidence: None,
        }
    }
}

impl fmt::Debug for AgentFailedSemanticEffect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentFailedSemanticEffect")
            .field("receipt", &self.receipt)
            .field("failure", &self.failure)
            .field("has_execution_evidence", &self.evidence.is_some())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AgentEffectRowState {
    Authorized,
    Dispatched(SemanticActionAttemptId),
}

pub(super) struct AgentEffectRow {
    id: AgentEffectId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    effect: SemanticEffectClass,
    destination_origin: SemanticOrigin,
    account: AgentContextAccountBinding,
    action_guard: [u8; 32],
    guard: [u8; 32],
    state: AgentEffectRowState,
}

impl AgentEffectRow {
    pub(super) const fn lease(&self) -> AgentPlanLeaseId {
        self.lease
    }

    fn serializes_origin(&self, origin: &SemanticOrigin) -> bool {
        requires_origin_serialization(self.effect) && self.destination_origin == *origin
    }

    pub(super) const fn requires_origin_serialization(&self) -> bool {
        requires_origin_serialization(self.effect)
    }

    fn matches_permit(&self, permit: &AgentEffectPermit) -> bool {
        self.id == permit.id
            && self.lease == permit.lease
            && self.node == permit.node
            && self.effect == permit.effect
            && self.action_guard == permit.action_guard
            && self.guard == permit.guard
    }

    fn matches_active(&self, active: &AgentActiveEffect) -> bool {
        self.id == active.id
            && self.lease == active.lease
            && self.node == active.node
            && self.effect == active.effect
            && self.action_guard == active.action_guard
            && self.guard == active.guard
            && self.state == AgentEffectRowState::Dispatched(active.attempt)
    }
}

impl AgentRunPolicy {
    /// Authorizes one exact prepared semantic effect or returns `NeedsHuman`.
    pub fn authorize_semantic_effect(
        &mut self,
        request: AgentEffectRequest,
        action: &SemanticPreparedAction,
        assessment: &AgentEffectAssessment,
    ) -> Result<AgentEffectAuthorization, AgentPolicyError> {
        if self.sealed {
            return Err(AgentPolicyError::Sealed);
        }
        if self.effects.len() >= MAX_AGENT_PENDING_EFFECTS {
            return Err(AgentPolicyError::PendingEffectLimit);
        }
        if !self.calls.is_empty() {
            return Err(AgentPolicyError::ModelCallPending);
        }
        if self.last_effect.is_some_and(|last| request.id() <= last) {
            return Err(AgentPolicyError::EffectReplay);
        }
        let lease_index = self
            .lease_index(request.lease())
            .ok_or(AgentPolicyError::Lease)?;
        let node_id = self.leases[lease_index].binding.node();
        self.last_effect = Some(request.id());

        let node = self
            .manifest
            .plan_node(node_id)
            .ok_or(AgentPolicyError::Invariant)?;
        validate_time(
            &self.manifest,
            node.expires_at(),
            request.account(),
            request.now(),
        )?;
        let context = action.frame().context();
        if request.automation().context() != context
            || request.account().context() != context
            || context.identity().owner() != self.manifest.run()
        {
            return Err(AgentPolicyError::Authority);
        }
        if !assessment.matches_action(action) || assessment.actual_effect() != action.effect() {
            return Err(AgentPolicyError::EffectMismatch);
        }
        if !request.automation().can_automate() {
            if request.automation().status().control() == ContextControl::Human {
                return Ok(self.needs_human(
                    node_id,
                    context,
                    assessment.actual_effect(),
                    AgentNeedsHumanReason::HumanControl,
                ));
            }
            return Err(AgentPolicyError::ContextNotAutomatable);
        }
        if !model_received_action(&self.taints, action) {
            return Err(AgentPolicyError::ModelSourceMissing);
        }

        let effect = assessment.actual_effect();
        let account = request.account().account();
        let profile = context.identity().profile();
        let destination = EffectDestination {
            profile,
            account,
            origin: assessment.destination_origin(),
            effect,
        };
        if !effect_scope_contains(&self.manifest, node, action, destination) {
            return Ok(self.needs_human(
                node_id,
                context,
                effect,
                AgentNeedsHumanReason::ScopeExpansion,
            ));
        }
        if effect == SemanticEffectClass::CapabilityBoundary {
            return Ok(self.needs_human(
                node_id,
                context,
                effect,
                AgentNeedsHumanReason::CapabilityBoundary,
            ));
        }
        if effect != SemanticEffectClass::Read
            && assessment.destination_origin() != action.frame().origin()
        {
            return Ok(self.needs_human(
                node_id,
                context,
                effect,
                AgentNeedsHumanReason::CrossOriginWrite,
            ));
        }
        if let Some(reason) = data_flow_decision(&self.manifest, node, &self.taints, destination)? {
            return Ok(self.needs_human(node_id, context, effect, reason));
        }
        if self
            .effects
            .iter()
            .any(|row| row.action_guard == action.verification_guard())
        {
            return Err(AgentPolicyError::EffectActionPending);
        }
        if requires_origin_serialization(effect)
            && self
                .effects
                .iter()
                .any(|row| row.serializes_origin(assessment.destination_origin()))
        {
            return Err(AgentPolicyError::OriginWritePending);
        }

        ensure_budget(self.manifest.budget(), self.accounting(), 0, 0)?;
        let lease_accounting = self
            .lease_accounting(request.lease())
            .ok_or(AgentPolicyError::Invariant)?;
        ensure_budget(node.budget(), lease_accounting, 0, 0)?;

        let guard = effect_guard(
            self.manifest.guard(),
            request,
            node_id,
            action,
            assessment,
            &self.taints,
        );
        self.effects.push(AgentEffectRow {
            id: request.id(),
            lease: request.lease(),
            node: node_id,
            effect,
            destination_origin: assessment.destination_origin().clone(),
            account: request.account(),
            action_guard: action.verification_guard(),
            guard,
            state: AgentEffectRowState::Authorized,
        });
        Ok(AgentEffectAuthorization::Permit(AgentEffectPermit {
            manifest: self.manifest.id(),
            manifest_guard: self.manifest.guard(),
            id: request.id(),
            lease: request.lease(),
            node: node_id,
            effect,
            action_guard: action.verification_guard(),
            guard,
        }))
    }

    /// Releases one exact pre-dispatch operation reservation.
    pub fn cancel_semantic_effect(
        &mut self,
        permit: AgentEffectPermit,
        _cancellation: AgentEffectCancellation,
    ) -> Result<(), AgentPolicyError> {
        let Some(index) = self.effect_index(permit.id()) else {
            self.sealed = true;
            return Err(AgentPolicyError::EffectMissing);
        };
        let row = &self.effects[index];
        if !permit.matches_manifest_revision(self.manifest.id(), self.manifest.guard())
            || row.state != AgentEffectRowState::Authorized
            || !row.matches_permit(&permit)
        {
            self.sealed = true;
            return Err(AgentPolicyError::EffectSettlementMismatch);
        }
        self.effects.remove(index);
        Ok(())
    }

    /// Performs final actor-owned revalidation and marks one permit dispatched.
    ///
    /// Expected pre-native lifecycle/account refusal releases the operation
    /// reservation while consuming the request's correlation identity. Token
    /// substitution or attempt replay seals the policy as ambiguous.
    pub fn dispatch_semantic_effect(
        &mut self,
        permit: AgentEffectPermit,
        action: &SemanticPreparedAction,
        request: AgentEffectDispatchRequest,
    ) -> Result<AgentActiveEffect, AgentPolicyError> {
        let Some(index) = self.effect_index(permit.id()) else {
            self.sealed = true;
            return Err(AgentPolicyError::EffectMissing);
        };
        let row = &self.effects[index];
        if row.state != AgentEffectRowState::Authorized
            || !permit.matches_manifest_revision(self.manifest.id(), self.manifest.guard())
            || !row.matches_permit(&permit)
            || permit.action_guard != action.verification_guard()
        {
            self.sealed = true;
            return Err(AgentPolicyError::EffectSettlementMismatch);
        }
        if self
            .last_action_attempt
            .is_some_and(|last| request.attempt() <= last)
        {
            self.sealed = true;
            return Err(AgentPolicyError::EffectAttemptReplay);
        }
        // Attempt identities are one-shot even when the last pre-native
        // lifecycle sample refuses dispatch. Reusing correlation identity
        // after an interrupted handoff would make later audit ambiguous.
        self.last_action_attempt = Some(request.attempt());
        let node_id = row.node;
        let expected_account = row.account;
        let node = self
            .manifest
            .plan_node(node_id)
            .ok_or(AgentPolicyError::Invariant)?;
        let context = action.frame().context();
        if request.account() != expected_account
            || request.account().context() != context
            || request.automation().context() != context
        {
            self.effects.remove(index);
            return Err(AgentPolicyError::Authority);
        }
        if !request.automation().can_automate() {
            self.effects.remove(index);
            return Err(AgentPolicyError::ContextNotAutomatable);
        }
        if let Err(error) = validate_time(
            &self.manifest,
            node.expires_at(),
            request.account(),
            request.now(),
        ) {
            self.effects.remove(index);
            return Err(error);
        }
        self.effects[index].state = AgentEffectRowState::Dispatched(request.attempt());
        Ok(AgentActiveEffect {
            manifest: permit.manifest,
            manifest_guard: permit.manifest_guard,
            id: permit.id,
            lease: permit.lease,
            node: permit.node,
            effect: permit.effect,
            attempt: request.attempt(),
            action_guard: permit.action_guard,
            guard: permit.guard,
        })
    }

    /// Consumes one exact verified pipeline terminal into a charged receipt.
    pub fn settle_verified_semantic_terminal(
        &mut self,
        terminal: SemanticActionVerifiedTerminal,
        action: &SemanticPreparedAction,
    ) -> Result<AgentVerifiedSemanticEffect, AgentPolicyError> {
        let (active, execution, settlement, verified) = terminal.into_parts();
        let receipt = self.settle_verified_semantic_effect(active, action, &verified)?;
        Ok(AgentVerifiedSemanticEffect {
            receipt,
            execution,
            settlement,
            verified,
        })
    }

    /// Consumes one exact refused proof opportunity into a charged failure.
    pub fn settle_refused_semantic_terminal(
        &mut self,
        refusal: SemanticActionVerificationRefusal,
    ) -> Result<AgentFailedSemanticEffect, AgentPolicyError> {
        let (active, execution, settlement, verification_error) = refusal.into_parts();
        let failure = verification_error.action_failure();
        let receipt =
            self.settle_semantic_effect(active, AgentEffectSettlement::Failed(failure))?;
        Ok(AgentFailedSemanticEffect {
            receipt,
            failure,
            evidence: Some(Box::new(
                AgentFailedSemanticEffectEvidence::AfterExecution {
                    execution,
                    settlement,
                    verification_error,
                },
            )),
        })
    }

    /// Consumes an exact native-execution admission refusal into one charged failure.
    pub fn settle_execution_admission_refusal(
        &mut self,
        refusal: SemanticActionExecutionCoordinatorRefusal,
        action: &SemanticPreparedAction,
    ) -> Result<AgentFailedSemanticEffect, AgentPolicyError> {
        let failure = refusal.action_failure();
        let (active, _error) = refusal.into_parts();
        self.settle_failed_semantic_effect(active, action, failure)
    }

    /// Consumes an exact native-to-settlement refusal into one charged failure.
    pub fn settle_settlement_start_refusal(
        &mut self,
        refusal: SemanticActionSettlementRefusal,
        action: &SemanticPreparedAction,
    ) -> Result<AgentFailedSemanticEffect, AgentPolicyError> {
        let failure = refusal.action_failure();
        let (active, _error) = refusal.into_parts();
        self.settle_failed_semantic_effect(active, action, failure)
    }

    /// Consumes an exact settlement-coordinator admission refusal into one charged failure.
    pub fn settle_settlement_admission_refusal(
        &mut self,
        refusal: SemanticActionSettlementAdmissionRefusal,
        action: &SemanticPreparedAction,
    ) -> Result<AgentFailedSemanticEffect, AgentPolicyError> {
        let failure = refusal.action_failure();
        let (start, _error) = refusal.into_parts();
        let (active, _execution, _tracker) = start.into_parts();
        self.settle_failed_semantic_effect(active, action, failure)
    }

    /// Settles a dispatched effect only with its exact independent proof.
    pub(crate) fn settle_verified_semantic_effect(
        &mut self,
        active: AgentActiveEffect,
        action: &SemanticPreparedAction,
        verified: &SemanticVerifiedAction,
    ) -> Result<AgentEffectReceipt, AgentPolicyError> {
        if verified.attempt() != active.attempt
            || active.action_guard != action.verification_guard()
            || !verified.matches_action(action)
        {
            self.sealed = true;
            return Err(AgentPolicyError::EffectSettlementMismatch);
        }
        self.settle_semantic_effect(active, AgentEffectSettlement::Verified(verified.proof()))
    }

    /// Settles a dispatched effect under the closed action failure taxonomy.
    pub(crate) fn settle_failed_semantic_effect(
        &mut self,
        active: AgentActiveEffect,
        action: &SemanticPreparedAction,
        failure: SemanticActionFailure,
    ) -> Result<AgentFailedSemanticEffect, AgentPolicyError> {
        if !active.matches_action(action) {
            self.sealed = true;
            return Err(AgentPolicyError::EffectSettlementMismatch);
        }
        let receipt =
            self.settle_semantic_effect(active, AgentEffectSettlement::Failed(failure))?;
        Ok(AgentFailedSemanticEffect {
            receipt,
            failure,
            evidence: None,
        })
    }

    fn settle_semantic_effect(
        &mut self,
        active: AgentActiveEffect,
        settlement: AgentEffectSettlement,
    ) -> Result<AgentEffectReceipt, AgentPolicyError> {
        let Some(effect_index) = self.effect_index(active.id()) else {
            self.sealed = true;
            return Err(AgentPolicyError::EffectMissing);
        };
        let row = &self.effects[effect_index];
        if !active.matches_manifest_revision(self.manifest.id(), self.manifest.guard())
            || !row.matches_active(&active)
        {
            self.sealed = true;
            return Err(AgentPolicyError::EffectSettlementMismatch);
        }
        let Some(lease_index) = self.lease_index(row.lease) else {
            self.sealed = true;
            return Err(AgentPolicyError::Invariant);
        };
        let consumed = ConsumedUsage {
            operations: 1,
            model_tokens: 0,
            cost_micro_usd: 0,
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
        let row = self.effects.remove(effect_index);
        Ok(AgentEffectReceipt {
            manifest: self.manifest.id(),
            manifest_guard: active.manifest_guard,
            id: row.id,
            lease: row.lease,
            node: row.node,
            effect: row.effect,
            attempt: active.attempt,
            settlement,
            action_guard: active.action_guard,
        })
    }

    fn needs_human(
        &self,
        node: AgentPlanNodeId,
        context: ContextJoin,
        effect: SemanticEffectClass,
        reason: AgentNeedsHumanReason,
    ) -> AgentEffectAuthorization {
        AgentEffectAuthorization::NeedsHuman(AgentNeedsHumanTransition {
            manifest: self.manifest.id(),
            manifest_guard: self.manifest.guard(),
            node,
            context,
            effect,
            reason,
        })
    }

    fn effect_index(&self, id: AgentEffectId) -> Option<usize> {
        self.effects.iter().position(|row| row.id == id)
    }
}

fn model_received_action(taints: &[AgentTaintCohort], action: &SemanticPreparedAction) -> bool {
    taints.iter().any(|taint| {
        taint.context == action.frame().context()
            && taint.observation == action.source_observation()
            && taint.observation_generation == action.source_observation_generation()
            && taint.origin == *action.frame().origin()
            && taint.contains_reference(action.target_reference())
            && action
                .bound_action()
                .option_reference()
                .is_none_or(|reference| taint.contains_reference(reference))
    })
}

const fn requires_origin_serialization(effect: SemanticEffectClass) -> bool {
    matches!(
        effect,
        SemanticEffectClass::ExternalWrite
            | SemanticEffectClass::Communication
            | SemanticEffectClass::Purchase
            | SemanticEffectClass::Destructive
    )
}

#[derive(Clone, Copy)]
struct EffectDestination<'a> {
    profile: ProfileId,
    account: AgentAccountScope,
    origin: &'a SemanticOrigin,
    effect: SemanticEffectClass,
}

fn effect_scope_contains(
    manifest: &AgentRunManifest,
    node: &crate::AgentPlanNodeScope,
    action: &SemanticPreparedAction,
    destination: EffectDestination<'_>,
) -> bool {
    manifest
        .scope()
        .profiles()
        .binary_search(&destination.profile)
        .is_ok()
        && node.profiles().binary_search(&destination.profile).is_ok()
        && manifest
            .scope()
            .accounts()
            .binary_search(&destination.account)
            .is_ok()
        && node.accounts().binary_search(&destination.account).is_ok()
        && manifest
            .scope()
            .origins()
            .binary_search(action.frame().origin())
            .is_ok()
        && node
            .origins()
            .binary_search(action.frame().origin())
            .is_ok()
        && manifest
            .scope()
            .origins()
            .binary_search(destination.origin)
            .is_ok()
        && node.origins().binary_search(destination.origin).is_ok()
        && manifest.scope().effects().contains(destination.effect)
        && node.effects().contains(destination.effect)
        && action.target_sensitivity() != SemanticSensitivity::Secret
        && action.target_sensitivity() <= manifest.scope().max_sensitivity()
        && action.target_sensitivity() <= node.max_sensitivity()
}

fn data_flow_decision(
    manifest: &AgentRunManifest,
    node: &crate::AgentPlanNodeScope,
    taints: &[AgentTaintCohort],
    destination: EffectDestination<'_>,
) -> Result<Option<AgentNeedsHumanReason>, AgentPolicyError> {
    for taint in taints {
        if taint.profile() != destination.profile {
            return Err(AgentPolicyError::CrossProfileData);
        }
        if taint.sensitivity() == SemanticSensitivity::Secret {
            return Err(AgentPolicyError::Invariant);
        }
        if node.profiles().binary_search(&taint.profile()).is_err()
            || node.accounts().binary_search(&taint.account()).is_err()
            || node.origins().binary_search(taint.origin()).is_err()
            || taint.sensitivity() > node.max_sensitivity()
        {
            return Ok(Some(AgentNeedsHumanReason::ScopeExpansion));
        }
        if destination.effect == SemanticEffectClass::Read
            || (taint.account() == destination.account && taint.origin() == destination.origin)
        {
            continue;
        }
        let allowed = manifest.scope().data_flows().iter().any(|flow| {
            flow.source_origin() == taint.origin()
                && flow.source_account() == taint.account()
                && flow.destination_origin() == destination.origin
                && flow.destination_account() == destination.account
                && taint.sensitivity() <= flow.max_sensitivity()
                && flow.effects().contains(destination.effect)
        });
        if !allowed {
            return Ok(Some(AgentNeedsHumanReason::DataFlowApproval));
        }
    }
    Ok(None)
}

fn effect_guard(
    manifest_guard: [u8; 32],
    request: AgentEffectRequest,
    node: AgentPlanNodeId,
    action: &SemanticPreparedAction,
    assessment: &AgentEffectAssessment,
    taints: &[AgentTaintCohort],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-AGENT-SEMANTIC-EFFECT-PERMIT-1\0");
    hasher.update(manifest_guard);
    hasher.update(request.id().get().to_be_bytes());
    hasher.update(request.lease().bytes());
    hasher.update(node.bytes());
    hasher.update(request.account().attestation().bytes());
    hash_account(&mut hasher, request.account().account());
    hasher.update(request.account().observed_at().millis().to_be_bytes());
    hash_context(&mut hasher, request.automation().context());
    hasher.update(action.verification_guard());
    hash_effect_class(&mut hasher, assessment.actual_effect());
    hash_origin(&mut hasher, assessment.destination_origin());
    hasher.update((taints.len() as u64).to_be_bytes());
    for taint in taints {
        hash_context(&mut hasher, taint.context());
        hasher.update(taint.observation().get().to_be_bytes());
        hasher.update(taint.observation_generation().get().to_be_bytes());
        hash_account(&mut hasher, taint.account());
        hash_origin(&mut hasher, taint.origin());
        hasher.update([match taint.sensitivity() {
            SemanticSensitivity::Public => 1,
            SemanticSensitivity::Sensitive => 2,
            SemanticSensitivity::Secret => 3,
        }]);
        hasher.update([match taint.trust() {
            SemanticTrust::UntrustedPage => 1,
            SemanticTrust::BrowserDerived => 2,
        }]);
        hasher.update(taint.attested_at().millis().to_be_bytes());
        hasher.update((taint.references.len() as u64).to_be_bytes());
        for reference in &taint.references {
            hasher.update(reference.get().to_be_bytes());
        }
    }
    hasher.finalize().into()
}

fn hash_origin(hasher: &mut Sha256, origin: &SemanticOrigin) {
    let value = origin.as_url().as_str().as_bytes();
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn hash_effect_class(hasher: &mut Sha256, effect: SemanticEffectClass) {
    hasher.update([match effect {
        SemanticEffectClass::Read => 1,
        SemanticEffectClass::LocalWrite => 2,
        SemanticEffectClass::ExternalWrite => 3,
        SemanticEffectClass::Communication => 4,
        SemanticEffectClass::Purchase => 5,
        SemanticEffectClass::Destructive => 6,
        SemanticEffectClass::CapabilityBoundary => 7,
    }]);
}
