//! Verified action completion joined to one bounded semantic next-state update.
//!
//! An independently verified effect is necessary but not sufficient for action
//! success. This module also requires the exact model-acknowledged observation
//! that supplied the action references, proves that semantic verification came
//! from the assembled post-action observation, and returns either a complete
//! bounded diff or that exact observation as an explicit fresh-snapshot fallback.

use std::fmt;

use thiserror::Error;

use crate::{
    compute_semantic_diff, AgentEffectReceipt, AgentVerifiedSemanticEffect,
    SemanticActionExecutionApplied, SemanticActionFailure, SemanticDiff, SemanticDiffBudget,
    SemanticDiffOutcome, SemanticFreshSnapshotReason, SemanticObservation,
    SemanticObservationAcknowledgement, SemanticPreparedAction, SemanticSettleInstant,
    SemanticSettleTracker, SemanticVerifiedAction,
};

/// One complete current observation joined to its trusted-shell capture time.
///
/// Construction grants no authority and does not attest the clock or source.
/// The imperative shell must use the same monotonic clock domain as settlement
/// and source the observation from the fixed native semantic adapter.
pub struct SemanticPostActionObservation {
    observed_at: SemanticSettleInstant,
    observation: SemanticObservation,
}

impl SemanticPostActionObservation {
    /// Joins one complete assembled observation to its monotonic capture time.
    pub const fn new(observed_at: SemanticSettleInstant, observation: SemanticObservation) -> Self {
        Self {
            observed_at,
            observation,
        }
    }

    /// Trusted-shell monotonic capture time.
    pub const fn observed_at(&self) -> SemanticSettleInstant {
        self.observed_at
    }

    /// Complete bounded current observation.
    pub const fn observation(&self) -> &SemanticObservation {
        &self.observation
    }
}

impl fmt::Debug for SemanticPostActionObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticPostActionObservation")
            .field("observed_at", &self.observed_at)
            .field("frame_count", &self.observation.frames().len())
            .field("node_count", &self.observation.node_count())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed class of next-state update carried by one successful action result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticActionNextState {
    /// A complete bounded diff against the exact acknowledged baseline.
    Diff,
    /// Diff premises failed safely; the exact current observation must be sent in full.
    FreshSnapshot(SemanticFreshSnapshotReason),
}

enum SemanticActionStateUpdate {
    Diff(Box<SemanticDiff>),
    FreshSnapshot {
        reason: SemanticFreshSnapshotReason,
        observation: Box<SemanticObservation>,
    },
}

/// One independently verified action plus its exact bounded next browser state.
///
/// This value is not policy authority and cannot execute, retry, or acknowledge
/// model delivery. The contained diff or fresh observation must still pass the
/// existing token-admission and committed-delivery boundary.
pub struct SemanticActionResult {
    verified: SemanticVerifiedAction,
    update: SemanticActionStateUpdate,
}

impl SemanticActionResult {
    /// Independently established effect proof.
    pub const fn verified(&self) -> &SemanticVerifiedAction {
        &self.verified
    }

    /// Whether the next model update is a bounded diff or full fresh snapshot.
    pub const fn next_state(&self) -> SemanticActionNextState {
        match &self.update {
            SemanticActionStateUpdate::Diff(_) => SemanticActionNextState::Diff,
            SemanticActionStateUpdate::FreshSnapshot { reason, .. } => {
                SemanticActionNextState::FreshSnapshot(*reason)
            }
        }
    }

    /// Complete bounded diff, when every conservative diff premise held.
    pub const fn diff(&self) -> Option<&SemanticDiff> {
        match &self.update {
            SemanticActionStateUpdate::Diff(diff) => Some(diff),
            SemanticActionStateUpdate::FreshSnapshot { .. } => None,
        }
    }

    /// Exact current observation that must be encoded in full after diff fallback.
    pub const fn fresh_snapshot(&self) -> Option<&SemanticObservation> {
        match &self.update {
            SemanticActionStateUpdate::Diff(_) => None,
            SemanticActionStateUpdate::FreshSnapshot { observation, .. } => Some(observation),
        }
    }
}

impl fmt::Debug for SemanticActionResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionResult")
            .field("verified", &self.verified)
            .field("next_state", &self.next_state())
            .field("diff_stats", &self.diff().map(SemanticDiff::stats))
            .field(
                "fresh_snapshot_nodes",
                &self.fresh_snapshot().map(SemanticObservation::node_count),
            )
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Policy-accounted verified action plus its bounded next browser state.
#[must_use]
pub struct AgentAccountedSemanticActionResult {
    receipt: AgentEffectReceipt,
    execution: SemanticActionExecutionApplied,
    settlement: SemanticSettleTracker,
    result: SemanticActionResult,
}

impl AgentAccountedSemanticActionResult {
    /// Exact policy receipt charged before result finalization.
    pub const fn receipt(&self) -> AgentEffectReceipt {
        self.receipt
    }

    /// Content-free backend attribution and native execution timing.
    pub const fn execution(&self) -> SemanticActionExecutionApplied {
        self.execution
    }

    /// Exact terminal settlement state used by independent verification.
    pub const fn settlement(&self) -> &SemanticSettleTracker {
        &self.settlement
    }

    /// Complete bounded action result for model-input admission.
    pub const fn result(&self) -> &SemanticActionResult {
        &self.result
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        AgentEffectReceipt,
        SemanticActionExecutionApplied,
        SemanticSettleTracker,
        SemanticActionResult,
    ) {
        (self.receipt, self.execution, self.settlement, self.result)
    }
}

impl fmt::Debug for AgentAccountedSemanticActionResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentAccountedSemanticActionResult")
            .field("receipt", &self.receipt)
            .field("execution", &self.execution)
            .field("settlement", &self.settlement)
            .field("result", &self.result)
            .finish()
    }
}

/// Result-finalization refusal that preserves charged proof and current state.
#[must_use]
pub struct AgentAccountedSemanticActionResultRefusal {
    accounted: Box<AgentVerifiedSemanticEffect>,
    current: Box<SemanticPostActionObservation>,
    error: SemanticActionResultError,
}

impl AgentAccountedSemanticActionResultRefusal {
    /// Closed authority or observation mismatch.
    pub const fn error(&self) -> SemanticActionResultError {
        self.error
    }

    /// Recovers the charged proof owner and exact current observation.
    pub fn into_parts(
        self,
    ) -> (
        AgentVerifiedSemanticEffect,
        SemanticPostActionObservation,
        SemanticActionResultError,
    ) {
        (*self.accounted, *self.current, self.error)
    }
}

impl fmt::Debug for AgentAccountedSemanticActionResultRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentAccountedSemanticActionResultRefusal")
            .field("accounted", &"[redacted]")
            .field("current", &self.current)
            .field("error", &self.error)
            .finish()
    }
}

/// Refusal before a verified action may be represented as successful.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionResultError {
    /// Effect proof was minted for another prepared action contract.
    #[error("semantic action result proof mismatched the prepared action")]
    ActionMismatch,
    /// The supplied baseline was not the observation that supplied action references.
    #[error("semantic action result baseline mismatched the bound action")]
    BaselineMismatch,
    /// The exact baseline did not have committed model-delivery authority.
    #[error("semantic action result baseline was not acknowledged")]
    BaselineNotAcknowledged,
    /// Current observation context did not match the independent effect proof.
    #[error("semantic action result current context mismatched verification")]
    CurrentContextMismatch,
    /// Assembled current observation did not contain the exact verification snapshot.
    #[error("semantic action result observation mismatched verification evidence")]
    VerificationObservationMismatch,
    /// Current observation was captured before independent effect evidence.
    #[error("semantic action result observation predates verification")]
    ObservationBeforeVerification,
    /// Current observation was captured after the action's sole absolute deadline.
    #[error("semantic action result observation exceeded the action deadline")]
    ObservationAfterDeadline,
}

impl SemanticActionResultError {
    /// Maps result refusal to the closed action-pipeline failure taxonomy.
    pub const fn action_failure(self) -> SemanticActionFailure {
        match self {
            Self::ActionMismatch | Self::BaselineMismatch | Self::BaselineNotAcknowledged => {
                SemanticActionFailure::LeaseViolation
            }
            Self::CurrentContextMismatch
            | Self::VerificationObservationMismatch
            | Self::ObservationBeforeVerification
            | Self::ObservationAfterDeadline => SemanticActionFailure::VerificationFailed,
        }
    }
}

/// Finalizes one policy-accounted action with a bounded next-state update.
///
/// Authority refusal returns both the already-charged proof owner and the exact
/// current observation. The current observation is consumed only after every
/// join validates, so callers cannot lose post-action state on a bad baseline,
/// proof, or clock coordinate.
pub fn finalize_accounted_semantic_action_result(
    action: &SemanticPreparedAction,
    accounted: AgentVerifiedSemanticEffect,
    baseline: &SemanticObservation,
    acknowledgement: &SemanticObservationAcknowledgement,
    current: SemanticPostActionObservation,
    budget: SemanticDiffBudget,
) -> Result<AgentAccountedSemanticActionResult, AgentAccountedSemanticActionResultRefusal> {
    if let Err(error) = validate_semantic_action_result(
        action,
        accounted.verified(),
        baseline,
        acknowledgement,
        &current,
    ) {
        return Err(AgentAccountedSemanticActionResultRefusal {
            accounted: Box::new(accounted),
            current: Box::new(current),
            error,
        });
    }
    let (receipt, execution, settlement, verified) = accounted.into_parts();
    let result =
        finish_semantic_action_result(verified, baseline, acknowledgement, current, budget);
    Ok(AgentAccountedSemanticActionResult {
        receipt,
        execution,
        settlement,
        result,
    })
}

#[cfg(test)]
pub(crate) fn finalize_semantic_action_result(
    action: &SemanticPreparedAction,
    verified: SemanticVerifiedAction,
    baseline: &SemanticObservation,
    acknowledgement: &SemanticObservationAcknowledgement,
    current: SemanticPostActionObservation,
    budget: SemanticDiffBudget,
) -> Result<SemanticActionResult, SemanticActionResultError> {
    validate_semantic_action_result(action, &verified, baseline, acknowledgement, &current)?;
    Ok(finish_semantic_action_result(
        verified,
        baseline,
        acknowledgement,
        current,
        budget,
    ))
}

fn validate_semantic_action_result(
    action: &SemanticPreparedAction,
    verified: &SemanticVerifiedAction,
    baseline: &SemanticObservation,
    acknowledgement: &SemanticObservationAcknowledgement,
    current: &SemanticPostActionObservation,
) -> Result<(), SemanticActionResultError> {
    if !verified.matches_action(action) {
        return Err(SemanticActionResultError::ActionMismatch);
    }
    if action.source_observation() != baseline.request().id()
        || action.source_observation_generation() != baseline.request().generation()
        || action.frame().context() != baseline.request().context()
    {
        return Err(SemanticActionResultError::BaselineMismatch);
    }
    if !acknowledgement.matches(baseline) {
        return Err(SemanticActionResultError::BaselineNotAcknowledged);
    }
    if current.observed_at() < verified.observed_at() {
        return Err(SemanticActionResultError::ObservationBeforeVerification);
    }
    if current.observed_at() > verified.deadline() {
        return Err(SemanticActionResultError::ObservationAfterDeadline);
    }
    if current.observation().request().context() != verified.current_context() {
        return Err(SemanticActionResultError::CurrentContextMismatch);
    }
    match (verified.current_invocation(), verified.current_snapshot()) {
        (Some(invocation), Some(generation)) => {
            if verified.current_context() != action.frame().context()
                || !current.observation().frames().iter().any(|snapshot| {
                    snapshot.frame() == action.frame()
                        && snapshot.invocation() == invocation
                        && snapshot.generation() == generation
                })
            {
                return Err(SemanticActionResultError::VerificationObservationMismatch);
            }
        }
        (None, None) => {}
        _ => return Err(SemanticActionResultError::VerificationObservationMismatch),
    }
    Ok(())
}

fn finish_semantic_action_result(
    verified: SemanticVerifiedAction,
    baseline: &SemanticObservation,
    acknowledgement: &SemanticObservationAcknowledgement,
    current: SemanticPostActionObservation,
    budget: SemanticDiffBudget,
) -> SemanticActionResult {
    let current = current.observation;
    let update = match compute_semantic_diff(baseline, acknowledgement, &current, budget) {
        SemanticDiffOutcome::Diff(diff) => SemanticActionStateUpdate::Diff(diff),
        SemanticDiffOutcome::FreshSnapshot(reason) => {
            debug_assert_ne!(reason, SemanticFreshSnapshotReason::NotAcknowledged);
            SemanticActionStateUpdate::FreshSnapshot {
                reason,
                observation: Box::new(current),
            }
        }
    };
    SemanticActionResult { verified, update }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_diff::SemanticObservationFingerprint;
    use crate::{
        decode_semantic_snapshot, verify_semantic_action, ContextCapabilities, ContextCapability,
        ContextId, ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameId, SemanticActionBatch, SemanticActionBatchId,
        SemanticActionIntent, SemanticActionProposal, SemanticDecodeContext, SemanticEffectClass,
        SemanticEffectEvidence, SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId,
        SemanticObservationAssembler, SemanticObservationBudget, SemanticObservationId,
        SemanticOperationClass, SemanticOrigin, SemanticReferenceId, SemanticSettleBudget,
        SemanticSettleEvent, SemanticSettleFact, SemanticSettleInstant, SemanticSettleTracker,
        SemanticSnapshotGeneration, SemanticState, SemanticVerification, SemanticWaitCondition,
        SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn context_with_registry() -> (crate::ContextJoin, ContextRegistry) {
        let identity = ContextIdentity::new(
            ContextId::from_raw(91),
            ContextRunId::from_raw(92),
            ProfileId::from(93),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[
                ContextCapability::Observe,
                ContextCapability::Act,
                ContextCapability::Navigate,
            ],
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
        let context = registry.join(identity.id()).expect("context");
        (context, registry)
    }

    fn context() -> crate::ContextJoin {
        context_with_registry().0
    }

    fn observation(
        context: crate::ContextJoin,
        id: u64,
        invocation: u64,
        generation: u64,
        checked: bool,
        status: &str,
    ) -> SemanticObservation {
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://result-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let states = u16::from(checked);
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": generation,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "checkbox", "n": "Private toggle",
                 "s": states, "o": 1},
                {"k": 3, "p": 0, "r": "status", "n": status}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(invocation).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(generation).expect("generation"),
            ),
            &bytes,
        )
        .expect("snapshot");
        let request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(id).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    fn acknowledge(observation: &SemanticObservation) -> SemanticObservationAcknowledgement {
        SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(observation),
        )
    }

    fn post(observation: SemanticObservation, millis: u64) -> SemanticPostActionObservation {
        SemanticPostActionObservation::new(SemanticSettleInstant::from_millis(millis), observation)
    }

    fn prepared(observation: &SemanticObservation, batch: u64) -> SemanticPreparedAction {
        let proposal = SemanticActionProposal::try_new(
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(2).expect("target"),
            },
            SemanticEffectClass::LocalWrite,
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetState {
                state: SemanticState::Checked,
                present: true,
            },
            SemanticSettleBudget::try_new(250).expect("budget"),
        )
        .expect("proposal");
        let batch = SemanticActionBatch::bind(
            SemanticActionBatchId::new(batch).expect("batch"),
            observation,
            &[observation.frames()[0].frame().clone()],
            vec![proposal],
        )
        .expect("bind");
        batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare")
    }

    fn verify(
        action: &SemanticPreparedAction,
        current: &SemanticObservation,
        attempt: u64,
    ) -> SemanticVerifiedAction {
        let attempt = crate::SemanticActionAttemptId::new(attempt).expect("attempt");
        let tracker =
            SemanticSettleTracker::begin(attempt, action, SemanticSettleInstant::from_millis(100))
                .expect("settlement");
        verify_semantic_action(
            &tracker,
            action,
            SemanticEffectEvidence::snapshot(
                attempt,
                SemanticSettleInstant::from_millis(101),
                &current.frames()[0],
            ),
        )
        .expect("verify")
    }

    #[test]
    fn finalizes_only_acknowledged_verified_state_as_a_complete_diff() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, "Private old status");
        let acknowledgement = acknowledge(&baseline);
        let action = prepared(&baseline, 1);
        let current = observation(context, 2, 2, 2, true, "Private old status");
        let verified = verify(&action, &current, 1);

        let result = finalize_semantic_action_result(
            &action,
            verified,
            &baseline,
            &acknowledgement,
            post(current, 101),
            SemanticDiffBudget::ACTION,
        )
        .expect("result");

        assert_eq!(result.next_state(), SemanticActionNextState::Diff);
        assert_eq!(result.diff().expect("diff").stats().changed(), 1);
        assert!(result.fresh_snapshot().is_none());
        assert_eq!(
            result
                .verified()
                .current_invocation()
                .expect("invocation")
                .get(),
            2
        );
        assert_eq!(
            result
                .verified()
                .current_snapshot()
                .expect("snapshot")
                .get(),
            2
        );
        let debug = format!("{result:?}");
        assert!(!debug.contains("Private old status"));
        assert!(!debug.contains("Private toggle"));
    }

    #[test]
    fn carries_the_exact_current_observation_when_diff_budget_requires_fresh_state() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, "Private old status");
        let acknowledgement = acknowledge(&baseline);
        let action = prepared(&baseline, 1);
        let current = observation(context, 2, 2, 2, true, "Private new status");
        let post_debug = format!("{:?}", post(current.clone(), 101));
        assert!(!post_debug.contains("Private new status"));
        assert!(!post_debug.contains("Private toggle"));
        let verified = verify(&action, &current, 1);

        let result = finalize_semantic_action_result(
            &action,
            verified,
            &baseline,
            &acknowledgement,
            post(current, 101),
            SemanticDiffBudget::try_new(1).expect("budget"),
        )
        .expect("fresh result");

        assert_eq!(
            result.next_state(),
            SemanticActionNextState::FreshSnapshot(SemanticFreshSnapshotReason::DiffLimit)
        );
        assert!(result.diff().is_none());
        let fresh = result.fresh_snapshot().expect("fresh observation");
        assert_eq!(fresh.request().id().get(), 2);
        assert_eq!(fresh.node_count(), 3);
        let debug = format!("{result:?}");
        assert!(!debug.contains("Private new status"));
    }

    #[test]
    fn refuses_unacknowledged_or_wrong_source_baselines() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, "Private old status");
        let altered = observation(context, 1, 1, 1, false, "Private altered status");
        let action = prepared(&baseline, 1);
        let current = observation(context, 2, 2, 2, true, "Private old status");
        let verified = verify(&action, &current, 1);
        assert_eq!(
            finalize_semantic_action_result(
                &action,
                verified,
                &baseline,
                &acknowledge(&altered),
                post(current, 101),
                SemanticDiffBudget::ACTION,
            )
            .expect_err("acknowledgement must bind content"),
            SemanticActionResultError::BaselineNotAcknowledged
        );

        let wrong_source = observation(context, 9, 1, 1, false, "Private old status");
        let current = observation(context, 2, 2, 2, true, "Private old status");
        let verified = verify(&action, &current, 2);
        assert_eq!(
            finalize_semantic_action_result(
                &action,
                verified,
                &wrong_source,
                &acknowledge(&wrong_source),
                post(current, 101),
                SemanticDiffBudget::ACTION,
            )
            .expect_err("source must match"),
            SemanticActionResultError::BaselineMismatch
        );
    }

    #[test]
    fn refuses_a_current_observation_that_did_not_supply_verification_evidence() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, "Private old status");
        let action = prepared(&baseline, 1);
        let evidence_observation = observation(context, 2, 2, 2, true, "Private old status");
        let verified = verify(&action, &evidence_observation, 1);
        let substituted = observation(context, 3, 3, 2, true, "Private old status");

        assert_eq!(
            finalize_semantic_action_result(
                &action,
                verified,
                &baseline,
                &acknowledge(&baseline),
                post(substituted, 101),
                SemanticDiffBudget::ACTION,
            )
            .expect_err("invocation substitution must fail"),
            SemanticActionResultError::VerificationObservationMismatch
        );
    }

    #[test]
    fn post_action_observation_must_follow_proof_within_the_same_deadline() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, "Private old status");
        let action = prepared(&baseline, 1);
        let current = observation(context, 2, 2, 2, true, "Private new status");
        let verified = verify(&action, &current, 1);
        assert_eq!(verified.observed_at().millis(), 101);
        assert_eq!(verified.deadline().millis(), 350);
        assert_eq!(
            finalize_semantic_action_result(
                &action,
                verified,
                &baseline,
                &acknowledge(&baseline),
                post(current.clone(), 100),
                SemanticDiffBudget::ACTION,
            )
            .expect_err("pre-verification observation must fail"),
            SemanticActionResultError::ObservationBeforeVerification
        );

        let verified = verify(&action, &current, 2);
        assert_eq!(
            finalize_semantic_action_result(
                &action,
                verified,
                &baseline,
                &acknowledge(&baseline),
                post(current, 351),
                SemanticDiffBudget::ACTION,
            )
            .expect_err("late observation must fail"),
            SemanticActionResultError::ObservationAfterDeadline
        );
        assert_eq!(
            SemanticActionResultError::ObservationBeforeVerification.action_failure(),
            SemanticActionFailure::VerificationFailed
        );
        assert_eq!(
            SemanticActionResultError::BaselineNotAcknowledged.action_failure(),
            SemanticActionFailure::LeaseViolation
        );
    }

    #[test]
    fn action_guard_includes_batch_and_source_authority() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, "Private old status");
        let first = prepared(&baseline, 1);
        let second = prepared(&baseline, 2);
        let current = observation(context, 2, 2, 2, true, "Private old status");
        let verified = verify(&first, &current, 1);

        assert_eq!(
            finalize_semantic_action_result(
                &second,
                verified,
                &baseline,
                &acknowledge(&baseline),
                post(current, 101),
                SemanticDiffBudget::ACTION,
            )
            .expect_err("batch substitution must fail"),
            SemanticActionResultError::ActionMismatch
        );
        assert_eq!(first.source_observation().get(), 1);
        assert_eq!(first.source_observation_generation().get(), 1);
        assert_eq!(first.batch().get(), 1);
        assert_eq!(
            SemanticOperationClass::Click,
            first.bound_action().kind().operation()
        );
    }

    #[test]
    fn navigation_proof_returns_the_exact_successor_as_fresh_state() {
        let (context, mut registry) = context_with_registry();
        let baseline = observation(context, 1, 1, 1, false, "Private old status");
        let proposal = SemanticActionProposal::try_new(
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(2).expect("target"),
            },
            SemanticEffectClass::LocalWrite,
            SemanticWaitCondition::NavigationCommitted,
            SemanticVerification::NavigationCommitted,
            SemanticSettleBudget::try_new(250).expect("budget"),
        )
        .expect("proposal");
        let batch = SemanticActionBatch::bind(
            SemanticActionBatchId::new(1).expect("batch"),
            &baseline,
            &[baseline.frames()[0].frame().clone()],
            vec![proposal],
        )
        .expect("bind");
        let action = batch.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("prepare");
        let attempt = crate::SemanticActionAttemptId::new(1).expect("attempt");
        let mut tracker =
            SemanticSettleTracker::begin(attempt, &action, SemanticSettleInstant::from_millis(100))
                .expect("settlement");
        let successor = registry
            .begin_navigation(
                context.identity().id(),
                ContextOperationId::new(2).expect("operation"),
            )
            .expect("navigation")
            .context();
        tracker
            .observe(SemanticSettleEvent::new(
                attempt,
                SemanticSettleInstant::from_millis(110),
                SemanticSettleFact::NavigationCommitted(successor),
            ))
            .expect("settle");
        let verified = verify_semantic_action(
            &tracker,
            &action,
            SemanticEffectEvidence::navigation(
                attempt,
                SemanticSettleInstant::from_millis(111),
                context,
                successor,
            ),
        )
        .expect("verify");
        let current = observation(successor, 2, 2, 1, false, "Private new document");

        let result = finalize_semantic_action_result(
            &action,
            verified,
            &baseline,
            &acknowledge(&baseline),
            post(current, 112),
            SemanticDiffBudget::ACTION,
        )
        .expect("navigation result");

        assert_eq!(result.verified().current_context(), successor);
        assert_eq!(result.verified().current_invocation(), None);
        assert_eq!(result.verified().current_snapshot(), None);
        assert_eq!(
            result.next_state(),
            SemanticActionNextState::FreshSnapshot(SemanticFreshSnapshotReason::ContextChanged)
        );
        assert_eq!(
            result
                .fresh_snapshot()
                .expect("fresh successor")
                .request()
                .context(),
            successor
        );
    }
}
