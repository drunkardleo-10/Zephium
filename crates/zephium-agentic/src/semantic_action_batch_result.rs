//! Bounded terminal aggregation for sequential semantic action batches.
//!
//! Each action is still prepared, settled, independently verified, and
//! finalized against the exact acknowledged model baseline. This layer admits
//! those already-final results in strict batch order, retains only content-free
//! completion summaries plus the latest state update, and makes continuation,
//! mandatory stop, completion, or terminal failure explicit. It never executes,
//! retries, authorizes, or fabricates browser state.

use std::fmt;

use thiserror::Error;

use crate::{
    ContextJoin, SemanticActionAttemptId, SemanticActionBatch, SemanticActionBatchId,
    SemanticActionFailure, SemanticActionKind, SemanticActionNextState, SemanticActionRecoveryHint,
    SemanticActionResult, SemanticEffectClass, SemanticEffectProofKind,
    SemanticFreshSnapshotReason, SemanticObservationGeneration, SemanticObservationId,
    SemanticPreparedAction, SemanticSettleInstant, MAX_SEMANTIC_ACTIONS_PER_BATCH,
};

/// Why a successfully verified prefix must not execute the remaining actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticActionBatchStopReason {
    /// A verified navigation replaced the exact main document.
    Navigation,
    /// A verified browser/page dialog transition created a new effect boundary.
    Dialog,
    /// Context/document/cancellation authority changed after success.
    ContextChanged,
    /// Trusted observation found meaningful state outside the planned prefix.
    UnpredictedState,
}

/// Safe next imperative step after one admitted successful action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticActionBatchContinuation {
    /// Prepare only this next one-based action against the complete current snapshot.
    Continue {
        /// Exact one-based action position that may be prepared next.
        next_ordinal: u8,
    },
    /// Return the successful prefix and latest state without executing a remainder.
    StopRequired(SemanticActionBatchStopReason),
    /// Every bound action has independently completed.
    Complete,
}

/// Content-free completion summary for one independently finalized action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticActionBatchCompletion {
    ordinal: u8,
    kind: SemanticActionKind,
    attempt: SemanticActionAttemptId,
    proof: SemanticEffectProofKind,
    observed_at: SemanticSettleInstant,
    current_context: ContextJoin,
    next_state: SemanticActionNextState,
}

impl SemanticActionBatchCompletion {
    /// One-based action position in the exact batch.
    pub const fn ordinal(self) -> u8 {
        self.ordinal
    }

    /// Closed action class.
    pub const fn kind(self) -> SemanticActionKind {
        self.kind
    }

    /// Exact independently verified native attempt.
    pub const fn attempt(self) -> SemanticActionAttemptId {
        self.attempt
    }

    /// Closed independent effect-proof class.
    pub const fn proof(self) -> SemanticEffectProofKind {
        self.proof
    }

    /// Monotonic independent-evidence time.
    pub const fn observed_at(self) -> SemanticSettleInstant {
        self.observed_at
    }

    /// Exact post-proof context/document/cancellation authority.
    pub const fn current_context(self) -> ContextJoin {
        self.current_context
    }

    /// Diff or full-snapshot state update class produced by finalization.
    pub const fn next_state(self) -> SemanticActionNextState {
        self.next_state
    }
}

/// Terminal batch outcome; none of these variants authorizes a retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticActionBatchOutcome {
    /// Every bound action independently finalized.
    Complete,
    /// A safe verified prefix completed and the remainder was deliberately skipped.
    Stopped {
        /// Closed stop boundary.
        reason: SemanticActionBatchStopReason,
        /// Number of bound actions deliberately not executed.
        remaining: u8,
    },
    /// The next exact action failed; prior state is not represented as current.
    Failed {
        /// One-based action that failed before a successful final result.
        ordinal: u8,
        /// Closed pipeline failure.
        failure: SemanticActionFailure,
        /// Non-authorizing minimum recovery boundary.
        recovery: SemanticActionRecoveryHint,
    },
}

/// Final bounded result for one exact action batch.
///
/// Complete/stopped outcomes retain only the latest action's diff or fresh
/// snapshot. Failed outcomes deliberately retain no state update because the
/// failed attempt may have changed the page without independently proving it.
#[must_use]
pub struct SemanticActionBatchResult {
    batch: SemanticActionBatchId,
    effect: SemanticEffectClass,
    total: u8,
    completions: Vec<SemanticActionBatchCompletion>,
    outcome: SemanticActionBatchOutcome,
    final_state: Option<SemanticActionResult>,
}

impl SemanticActionBatchResult {
    /// Exact model-proposed batch identity.
    pub const fn batch(&self) -> SemanticActionBatchId {
        self.batch
    }

    /// Homogeneous pre-policy effect class.
    pub const fn effect(&self) -> SemanticEffectClass {
        self.effect
    }

    /// Number of bound actions in the original batch.
    pub const fn total(&self) -> u8 {
        self.total
    }

    /// Strictly ordered content-free successful prefix.
    pub fn completions(&self) -> &[SemanticActionBatchCompletion] {
        &self.completions
    }

    /// Complete, deliberately stopped, or failed terminal state.
    pub const fn outcome(&self) -> SemanticActionBatchOutcome {
        self.outcome
    }

    /// Latest verified state update only for complete or stopped outcomes.
    pub const fn final_state(&self) -> Option<&SemanticActionResult> {
        self.final_state.as_ref()
    }

    /// Moves the latest state update into model encoding, when safe.
    pub fn into_final_state(self) -> Option<SemanticActionResult> {
        self.final_state
    }
}

impl fmt::Debug for SemanticActionBatchResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionBatchResult")
            .field("batch", &self.batch)
            .field("effect", &self.effect)
            .field("total", &self.total)
            .field("completed", &self.completions.len())
            .field("outcome", &self.outcome)
            .field(
                "final_next_state",
                &self
                    .final_state
                    .as_ref()
                    .map(SemanticActionResult::next_state),
            )
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Active bounded aggregation state for one exact sequential batch.
#[must_use]
pub struct SemanticActionBatchExecution {
    batch: SemanticActionBatchId,
    source_observation: SemanticObservationId,
    source_observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    effect: SemanticEffectClass,
    total: u8,
    action_guards: [[u8; 32]; MAX_SEMANTIC_ACTIONS_PER_BATCH],
    completions: Vec<SemanticActionBatchCompletion>,
    latest: Option<SemanticActionResult>,
    required_stop: Option<SemanticActionBatchStopReason>,
}

impl SemanticActionBatchExecution {
    /// Starts zero-I/O aggregation for one already-bound batch.
    pub fn new(batch: &SemanticActionBatch) -> Result<Self, SemanticActionBatchExecutionError> {
        if batch.actions().is_empty() || batch.actions().len() > MAX_SEMANTIC_ACTIONS_PER_BATCH {
            return Err(SemanticActionBatchExecutionError::Invariant);
        }
        let total = u8::try_from(batch.actions().len())
            .map_err(|_| SemanticActionBatchExecutionError::Invariant)?;
        let mut action_guards = [[0_u8; 32]; MAX_SEMANTIC_ACTIONS_PER_BATCH];
        for (slot, action) in action_guards.iter_mut().zip(batch.actions()) {
            *slot = action.verification_guard();
        }
        Ok(Self {
            batch: batch.id(),
            source_observation: batch.observation(),
            source_observation_generation: batch.observation_generation(),
            context: batch.context(),
            effect: batch.effect(),
            total,
            action_guards,
            completions: Vec::with_capacity(batch.actions().len()),
            latest: None,
            required_stop: None,
        })
    }

    /// Exact batch identity under aggregation.
    pub const fn batch(&self) -> SemanticActionBatchId {
        self.batch
    }

    /// Original action count.
    pub const fn total(&self) -> u8 {
        self.total
    }

    /// Successfully finalized prefix length.
    pub fn completed(&self) -> u8 {
        u8::try_from(self.completions.len()).unwrap_or(u8::MAX)
    }

    /// Current safe continuation decision.
    pub fn continuation(&self) -> SemanticActionBatchContinuation {
        if self.completions.len() == usize::from(self.total) {
            return SemanticActionBatchContinuation::Complete;
        }
        if let Some(reason) = self.required_stop {
            return SemanticActionBatchContinuation::StopRequired(reason);
        }
        SemanticActionBatchContinuation::Continue {
            next_ordinal: self.completed().saturating_add(1),
        }
    }

    /// Admits one exact finalized action in strict batch/ordinal/time order.
    ///
    /// The shell must have prepared any next action from the complete rolling
    /// checkpoint before consuming that checkpoint during finalization. This
    /// method retains only the latest full state update, keeping aggregation
    /// memory O(one bounded diff/snapshot + eight summaries).
    pub fn record_success(
        &mut self,
        action: &SemanticPreparedAction,
        result: SemanticActionResult,
    ) -> Result<SemanticActionBatchContinuation, SemanticActionBatchExecutionError> {
        if self.completions.len() == usize::from(self.total) {
            return Err(SemanticActionBatchExecutionError::AlreadyComplete);
        }
        if self.required_stop.is_some() {
            return Err(SemanticActionBatchExecutionError::StopRequired);
        }
        let expected = self.completed().saturating_add(1);
        if action.batch() != self.batch
            || action.source_observation() != self.source_observation
            || action.source_observation_generation() != self.source_observation_generation
            || action.bound_action().effect() != self.effect
        {
            return Err(SemanticActionBatchExecutionError::BatchMismatch);
        }
        if action.ordinal() != expected || result.verified().ordinal() != expected {
            return Err(SemanticActionBatchExecutionError::OrdinalMismatch);
        }
        let action_index = usize::from(expected - 1);
        if self.action_guards[action_index] != action.bound_action().verification_guard() {
            return Err(SemanticActionBatchExecutionError::ActionMismatch);
        }
        if !result.verified().matches_action(action) {
            return Err(SemanticActionBatchExecutionError::ActionResultMismatch);
        }
        if self
            .completions
            .iter()
            .any(|completion| completion.attempt == result.verified().attempt())
        {
            return Err(SemanticActionBatchExecutionError::AttemptReplay);
        }
        if self
            .completions
            .last()
            .is_some_and(|completion| result.verified().observed_at() < completion.observed_at)
        {
            return Err(SemanticActionBatchExecutionError::ClockRegression);
        }
        if result.verified().proof() == SemanticEffectProofKind::Navigation
            && result.next_state()
                != SemanticActionNextState::FreshSnapshot(
                    SemanticFreshSnapshotReason::ContextChanged,
                )
        {
            return Err(SemanticActionBatchExecutionError::ActionResultMismatch);
        }

        let completion = SemanticActionBatchCompletion {
            ordinal: expected,
            kind: action.kind(),
            attempt: result.verified().attempt(),
            proof: result.verified().proof(),
            observed_at: result.verified().observed_at(),
            current_context: result.verified().current_context(),
            next_state: result.next_state(),
        };
        self.completions.push(completion);
        self.latest = Some(result);

        if self.completions.len() < usize::from(self.total) {
            self.required_stop = match completion.proof {
                SemanticEffectProofKind::Navigation => {
                    Some(SemanticActionBatchStopReason::Navigation)
                }
                SemanticEffectProofKind::Dialog => Some(SemanticActionBatchStopReason::Dialog),
                _ if completion.current_context != self.context
                    || completion.next_state
                        == SemanticActionNextState::FreshSnapshot(
                            SemanticFreshSnapshotReason::ContextChanged,
                        ) =>
                {
                    Some(SemanticActionBatchStopReason::ContextChanged)
                }
                _ => None,
            };
        }
        Ok(self.continuation())
    }

    /// Deliberately stops a nonempty successful prefix for trusted unpredicted state.
    pub fn stop(
        mut self,
        reason: SemanticActionBatchStopReason,
    ) -> Result<SemanticActionBatchResult, SemanticActionBatchExecutionError> {
        if self.completions.is_empty() || self.completions.len() == usize::from(self.total) {
            return Err(SemanticActionBatchExecutionError::StopNotAllowed);
        }
        if let Some(required) = self.required_stop {
            if required != reason {
                return Err(SemanticActionBatchExecutionError::StopReasonMismatch);
            }
        } else if reason != SemanticActionBatchStopReason::UnpredictedState {
            return Err(SemanticActionBatchExecutionError::StopReasonMismatch);
        }
        self.required_stop = Some(reason);
        self.finish()
    }

    /// Terminally returns the complete batch or an automatically required stop.
    pub fn finish(self) -> Result<SemanticActionBatchResult, SemanticActionBatchExecutionError> {
        let completed = self.completions.len();
        let total = usize::from(self.total);
        let outcome = if completed == total {
            SemanticActionBatchOutcome::Complete
        } else if let Some(reason) = self.required_stop {
            let remaining = u8::try_from(total - completed)
                .map_err(|_| SemanticActionBatchExecutionError::Invariant)?;
            SemanticActionBatchOutcome::Stopped { reason, remaining }
        } else {
            return Err(SemanticActionBatchExecutionError::Incomplete);
        };
        let final_state = self
            .latest
            .ok_or(SemanticActionBatchExecutionError::Invariant)?;
        Ok(SemanticActionBatchResult {
            batch: self.batch,
            effect: self.effect,
            total: self.total,
            completions: self.completions,
            outcome,
            final_state: Some(final_state),
        })
    }

    /// Terminally fails the next action and discards any now-stale prior state.
    pub fn fail(
        self,
        failure: SemanticActionFailure,
    ) -> Result<SemanticActionBatchResult, SemanticActionBatchExecutionError> {
        if self.completions.len() == usize::from(self.total) {
            return Err(SemanticActionBatchExecutionError::AlreadyComplete);
        }
        if self.required_stop.is_some() {
            return Err(SemanticActionBatchExecutionError::StopRequired);
        }
        let ordinal = u8::try_from(self.completions.len() + 1)
            .map_err(|_| SemanticActionBatchExecutionError::Invariant)?;
        Ok(SemanticActionBatchResult {
            batch: self.batch,
            effect: self.effect,
            total: self.total,
            completions: self.completions,
            outcome: SemanticActionBatchOutcome::Failed {
                ordinal,
                failure,
                recovery: failure.recovery_hint(),
            },
            final_state: None,
        })
    }
}

impl fmt::Debug for SemanticActionBatchExecution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionBatchExecution")
            .field("batch", &self.batch)
            .field("effect", &self.effect)
            .field("total", &self.total)
            .field("completed", &self.completions.len())
            .field("continuation", &self.continuation())
            .field(
                "latest",
                &self.latest.as_ref().map(SemanticActionResult::next_state),
            )
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed refusal while aggregating already-finalized actions.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionBatchExecutionError {
    /// Batch/source/effect authority did not match the active aggregation.
    #[error("semantic action batch aggregation authority mismatch")]
    BatchMismatch,
    /// Action/result ordinal was not the exact next batch position.
    #[error("semantic action batch aggregation ordinal mismatch")]
    OrdinalMismatch,
    /// Prepared action was not the exact bound action at this batch position.
    #[error("semantic action batch aggregation action mismatch")]
    ActionMismatch,
    /// Final result was not minted for the supplied prepared action.
    #[error("semantic action batch aggregation result mismatch")]
    ActionResultMismatch,
    /// One native attempt identity was reused by two completed actions.
    #[error("semantic action batch aggregation attempt replay")]
    AttemptReplay,
    /// Independent proof time regressed across the successful prefix.
    #[error("semantic action batch aggregation clock regressed")]
    ClockRegression,
    /// A navigation/dialog/context boundary already requires a terminal stop.
    #[error("semantic action batch aggregation must stop")]
    StopRequired,
    /// Every bound action has already completed.
    #[error("semantic action batch aggregation is already complete")]
    AlreadyComplete,
    /// A batch without a stop boundary still has unexecuted actions.
    #[error("semantic action batch aggregation is incomplete")]
    Incomplete,
    /// A stop requires a nonempty successful prefix and an unexecuted remainder.
    #[error("semantic action batch aggregation cannot stop here")]
    StopNotAllowed,
    /// Caller-declared stop reason disagreed with established evidence.
    #[error("semantic action batch aggregation stop reason mismatch")]
    StopReasonMismatch,
    /// A bounded count or state representation invariant failed.
    #[error("semantic action batch aggregation invariant failed")]
    Invariant,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_diff::SemanticObservationFingerprint;
    use crate::{
        decode_semantic_snapshot, finalize_semantic_action_result, verify_semantic_action,
        ContextCapabilities, ContextCapability, ContextId, ContextIdentity, ContextKind,
        ContextOperationId, ContextRegistry, ContextRunId, ContextSettlement, FrameId,
        SemanticActionIntent, SemanticActionProposal, SemanticDecodeContext, SemanticDialogState,
        SemanticDiffBudget, SemanticEffectEvidence, SemanticFrameJoin, SemanticFrameTrust,
        SemanticInvocationId, SemanticObservation, SemanticObservationAcknowledgement,
        SemanticObservationAssembler, SemanticObservationBudget, SemanticObservationId,
        SemanticOrigin, SemanticPostActionObservation, SemanticReferenceId, SemanticSettleBudget,
        SemanticSettleEvent, SemanticSettleFact, SemanticSettleTracker, SemanticSnapshotGeneration,
        SemanticState, SemanticVerification, SemanticWaitCondition, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn context_with_registry() -> (ContextJoin, ContextRegistry) {
        let identity = ContextIdentity::new(
            ContextId::from_raw(191),
            ContextRunId::from_raw(192),
            ProfileId::from(193),
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

    fn context() -> ContextJoin {
        context_with_registry().0
    }

    fn observation(
        context: ContextJoin,
        id: u64,
        invocation: u64,
        generation: u64,
        first_checked: bool,
        second_checked: bool,
        status: &str,
    ) -> SemanticObservation {
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://batch-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": generation,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "checkbox", "n": "Private first toggle",
                 "s": u16::from(first_checked), "o": 1},
                {"k": 3, "p": 0, "r": "checkbox", "n": "Private second toggle",
                 "s": u16::from(second_checked), "o": 1},
                {"k": 4, "p": 0, "r": "status", "n": status}
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

    fn acknowledgement(observation: &SemanticObservation) -> SemanticObservationAcknowledgement {
        SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(observation),
        )
    }

    fn checked_click(target: u16) -> SemanticActionProposal {
        SemanticActionProposal::try_new(
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(target).expect("target"),
            },
            SemanticEffectClass::LocalWrite,
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetState {
                state: SemanticState::Checked,
                present: true,
            },
            SemanticSettleBudget::try_new(250).expect("budget"),
        )
        .expect("proposal")
    }

    fn standard_batch(baseline: &SemanticObservation, id: u64) -> SemanticActionBatch {
        SemanticActionBatch::bind(
            SemanticActionBatchId::new(id).expect("batch"),
            baseline,
            &[baseline.frames()[0].frame().clone()],
            vec![checked_click(2), checked_click(3)],
        )
        .expect("batch")
    }

    fn snapshot_result(
        action: &SemanticPreparedAction,
        baseline: &SemanticObservation,
        current: SemanticObservation,
        attempt: u64,
        completed_at: u64,
        observed_at: u64,
    ) -> SemanticActionResult {
        let attempt = SemanticActionAttemptId::new(attempt).expect("attempt");
        let tracker = SemanticSettleTracker::begin(
            attempt,
            action,
            SemanticSettleInstant::from_millis(completed_at),
        )
        .expect("settlement");
        let verified = verify_semantic_action(
            &tracker,
            action,
            SemanticEffectEvidence::snapshot(
                attempt,
                SemanticSettleInstant::from_millis(observed_at),
                &current.frames()[0],
            ),
        )
        .expect("verification");
        finalize_semantic_action_result(
            action,
            verified,
            baseline,
            &acknowledgement(baseline),
            SemanticPostActionObservation::new(
                SemanticSettleInstant::from_millis(observed_at),
                current,
            ),
            SemanticDiffBudget::ACTION,
        )
        .expect("result")
    }

    #[test]
    fn aggregates_a_verified_batch_in_exact_order_with_only_the_latest_state() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, false, "Private original status");
        let batch = standard_batch(&baseline, 1);
        let first = batch.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("first");
        let after_first = observation(context, 2, 2, 2, true, false, "Private first status");
        let second = batch.actions()[1]
            .prepare(&after_first.frames()[0])
            .expect("second rolling checkpoint");
        let first_result = snapshot_result(&first, &baseline, after_first, 1, 100, 101);
        let after_second = observation(context, 3, 3, 3, true, true, "Private final status");
        let second_result = snapshot_result(&second, &baseline, after_second, 2, 200, 201);

        let mut execution = SemanticActionBatchExecution::new(&batch).expect("execution");
        assert_eq!(execution.completed(), 0);
        assert_eq!(
            execution.continuation(),
            SemanticActionBatchContinuation::Continue { next_ordinal: 1 }
        );
        assert_eq!(
            execution
                .record_success(&first, first_result)
                .expect("first success"),
            SemanticActionBatchContinuation::Continue { next_ordinal: 2 }
        );
        assert_eq!(execution.completed(), 1);
        assert_eq!(
            execution
                .record_success(&second, second_result)
                .expect("second success"),
            SemanticActionBatchContinuation::Complete
        );
        let repeated_result = snapshot_result(
            &second,
            &baseline,
            observation(context, 4, 3, 3, true, true, "Private repeated status"),
            3,
            300,
            301,
        );
        assert_eq!(
            execution.record_success(&second, repeated_result),
            Err(SemanticActionBatchExecutionError::AlreadyComplete)
        );

        let result = execution.finish().expect("complete batch");
        assert_eq!(result.batch(), batch.id());
        assert_eq!(result.effect(), SemanticEffectClass::LocalWrite);
        assert_eq!(result.total(), 2);
        assert_eq!(result.outcome(), SemanticActionBatchOutcome::Complete);
        assert_eq!(result.completions().len(), 2);
        assert_eq!(result.completions()[0].ordinal(), 1);
        assert_eq!(result.completions()[1].ordinal(), 2);
        assert_eq!(
            result.completions()[0].proof(),
            SemanticEffectProofKind::TargetState
        );
        assert_eq!(result.completions()[1].observed_at().millis(), 201);
        assert_eq!(result.completions()[1].current_context(), context);
        assert_eq!(
            result.completions()[1].next_state(),
            result.final_state().expect("latest state").next_state()
        );
        let debug = format!("{result:?}");
        assert!(!debug.contains("Private original status"));
        assert!(!debug.contains("Private first status"));
        assert!(!debug.contains("Private final status"));
        assert!(!debug.contains("Private first toggle"));
    }

    #[test]
    fn exact_guard_rejects_reused_batch_ids_and_out_of_order_results() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, false, "Private baseline");
        let original = standard_batch(&baseline, 9);
        let impostor = SemanticActionBatch::bind(
            SemanticActionBatchId::new(9).expect("reused batch id"),
            &baseline,
            &[baseline.frames()[0].frame().clone()],
            vec![checked_click(3), checked_click(2)],
        )
        .expect("impostor batch");
        let impostor_first = impostor.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("impostor action");
        let impostor_current = observation(context, 2, 2, 2, false, true, "Private changed");
        let impostor_result =
            snapshot_result(&impostor_first, &baseline, impostor_current, 1, 100, 101);
        let mut execution = SemanticActionBatchExecution::new(&original).expect("execution");
        assert_eq!(
            execution.record_success(&impostor_first, impostor_result),
            Err(SemanticActionBatchExecutionError::ActionMismatch)
        );
        assert_eq!(execution.completed(), 0);

        let after_first = observation(context, 3, 2, 2, true, false, "Private first");
        let second = original.actions()[1]
            .prepare(&after_first.frames()[0])
            .expect("second");
        let after_second = observation(context, 4, 3, 3, true, true, "Private second");
        let second_result = snapshot_result(&second, &baseline, after_second, 2, 200, 201);
        assert_eq!(
            execution.record_success(&second, second_result),
            Err(SemanticActionBatchExecutionError::OrdinalMismatch)
        );
        assert_eq!(execution.completed(), 0);
        assert!(matches!(
            execution.finish(),
            Err(SemanticActionBatchExecutionError::Incomplete)
        ));
    }

    #[test]
    fn result_must_match_the_exact_pre_execution_checkpoint() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, false, "Private baseline");
        let batch = standard_batch(&baseline, 15);
        let supplied = batch.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("supplied checkpoint");
        let later_checkpoint =
            observation(context, 2, 2, 2, false, false, "Private later checkpoint");
        let substituted = batch.actions()[0]
            .prepare(&later_checkpoint.frames()[0])
            .expect("substituted checkpoint");
        let substituted_result = snapshot_result(
            &substituted,
            &baseline,
            observation(context, 3, 3, 3, true, false, "Private changed"),
            1,
            100,
            101,
        );
        let mut execution = SemanticActionBatchExecution::new(&batch).expect("execution");
        assert_eq!(
            execution.record_success(&supplied, substituted_result),
            Err(SemanticActionBatchExecutionError::ActionResultMismatch)
        );
        assert_eq!(execution.completed(), 0);

        let other = standard_batch(&baseline, 16);
        let other_first = other.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("other action");
        let other_result = snapshot_result(
            &other_first,
            &baseline,
            observation(context, 4, 2, 2, true, false, "Private other"),
            2,
            200,
            201,
        );
        assert_eq!(
            execution.record_success(&other_first, other_result),
            Err(SemanticActionBatchExecutionError::BatchMismatch)
        );
        assert_eq!(execution.completed(), 0);
    }

    #[test]
    fn refuses_attempt_replay_and_regressing_independent_proof_time() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, false, "Private baseline");
        let batch = standard_batch(&baseline, 10);
        let first = batch.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("first");
        let after_first = observation(context, 2, 2, 2, true, false, "Private first");
        let second = batch.actions()[1]
            .prepare(&after_first.frames()[0])
            .expect("second");
        let first_result = snapshot_result(&first, &baseline, after_first, 7, 100, 101);
        let second_result = snapshot_result(
            &second,
            &baseline,
            observation(context, 3, 3, 3, true, true, "Private second"),
            7,
            200,
            201,
        );
        let mut replay = SemanticActionBatchExecution::new(&batch).expect("execution");
        replay
            .record_success(&first, first_result)
            .expect("first success");
        assert_eq!(
            replay.record_success(&second, second_result),
            Err(SemanticActionBatchExecutionError::AttemptReplay)
        );
        assert_eq!(replay.completed(), 1);

        let first = batch.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("first");
        let after_first = observation(context, 4, 2, 2, true, false, "Private first");
        let second = batch.actions()[1]
            .prepare(&after_first.frames()[0])
            .expect("second");
        let first_result = snapshot_result(&first, &baseline, after_first, 8, 200, 201);
        let second_result = snapshot_result(
            &second,
            &baseline,
            observation(context, 5, 3, 3, true, true, "Private second"),
            9,
            100,
            101,
        );
        let mut clock = SemanticActionBatchExecution::new(&batch).expect("execution");
        clock
            .record_success(&first, first_result)
            .expect("first success");
        assert_eq!(
            clock.record_success(&second, second_result),
            Err(SemanticActionBatchExecutionError::ClockRegression)
        );
        assert_eq!(clock.completed(), 1);
    }

    #[test]
    fn failure_discards_prior_state_and_returns_only_a_no_retry_recovery_boundary() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, false, "Private baseline");
        let batch = standard_batch(&baseline, 11);
        let first = batch.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("first");
        let first_result = snapshot_result(
            &first,
            &baseline,
            observation(context, 2, 2, 2, true, false, "Private changed"),
            1,
            100,
            101,
        );
        let mut execution = SemanticActionBatchExecution::new(&batch).expect("execution");
        execution
            .record_success(&first, first_result)
            .expect("first success");
        let result = execution
            .fail(SemanticActionFailure::TargetOccluded)
            .expect("failed batch");
        assert_eq!(
            result.outcome(),
            SemanticActionBatchOutcome::Failed {
                ordinal: 2,
                failure: SemanticActionFailure::TargetOccluded,
                recovery: SemanticActionRecoveryHint::FreshObservationRequired,
            }
        );
        assert_eq!(result.completions().len(), 1);
        assert!(result.final_state().is_none());
        assert!(result.into_final_state().is_none());

        let cancelled = SemanticActionBatchExecution::new(&batch)
            .expect("execution")
            .fail(SemanticActionFailure::Cancelled)
            .expect("cancelled batch");
        assert_eq!(
            cancelled.outcome(),
            SemanticActionBatchOutcome::Failed {
                ordinal: 1,
                failure: SemanticActionFailure::Cancelled,
                recovery: SemanticActionRecoveryHint::Abort,
            }
        );
        assert!(cancelled.completions().is_empty());
    }

    #[test]
    fn trusted_unpredicted_state_may_stop_only_a_nonempty_successful_prefix() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, false, "Private baseline");
        let batch = standard_batch(&baseline, 12);
        assert!(matches!(
            SemanticActionBatchExecution::new(&batch)
                .expect("execution")
                .stop(SemanticActionBatchStopReason::UnpredictedState),
            Err(SemanticActionBatchExecutionError::StopNotAllowed)
        ));

        let first = batch.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("first");
        let first_result = snapshot_result(
            &first,
            &baseline,
            observation(context, 2, 2, 2, true, false, "Private changed"),
            1,
            100,
            101,
        );
        let mut execution = SemanticActionBatchExecution::new(&batch).expect("execution");
        execution
            .record_success(&first, first_result)
            .expect("first success");
        let result = execution
            .stop(SemanticActionBatchStopReason::UnpredictedState)
            .expect("deliberate stop");
        assert_eq!(
            result.outcome(),
            SemanticActionBatchOutcome::Stopped {
                reason: SemanticActionBatchStopReason::UnpredictedState,
                remaining: 1,
            }
        );
        assert_eq!(result.completions().len(), 1);
        assert!(result.final_state().is_some());

        let first = batch.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("first");
        let first_result = snapshot_result(
            &first,
            &baseline,
            observation(context, 3, 2, 2, true, false, "Private changed again"),
            2,
            200,
            201,
        );
        let mut execution = SemanticActionBatchExecution::new(&batch).expect("execution");
        execution
            .record_success(&first, first_result)
            .expect("first success");
        assert!(matches!(
            execution.stop(SemanticActionBatchStopReason::Navigation),
            Err(SemanticActionBatchExecutionError::StopReasonMismatch)
        ));
    }

    #[test]
    fn verified_navigation_mandatorily_stops_the_remainder_at_new_context_authority() {
        let (context, mut registry) = context_with_registry();
        let baseline = observation(context, 1, 1, 1, false, false, "Private baseline");
        let navigation = SemanticActionProposal::try_new(
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(2).expect("target"),
            },
            SemanticEffectClass::LocalWrite,
            SemanticWaitCondition::NavigationCommitted,
            SemanticVerification::NavigationCommitted,
            SemanticSettleBudget::try_new(250).expect("budget"),
        )
        .expect("navigation proposal");
        let batch = SemanticActionBatch::bind(
            SemanticActionBatchId::new(13).expect("batch"),
            &baseline,
            &[baseline.frames()[0].frame().clone()],
            vec![navigation, checked_click(3)],
        )
        .expect("batch");
        let first = batch.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("first");
        let attempt = SemanticActionAttemptId::new(1).expect("attempt");
        let mut tracker =
            SemanticSettleTracker::begin(attempt, &first, SemanticSettleInstant::from_millis(100))
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
            .expect("navigation settle");
        let verified = verify_semantic_action(
            &tracker,
            &first,
            SemanticEffectEvidence::navigation(
                attempt,
                SemanticSettleInstant::from_millis(111),
                context,
                successor,
            ),
        )
        .expect("navigation verification");
        let result = finalize_semantic_action_result(
            &first,
            verified,
            &baseline,
            &acknowledgement(&baseline),
            SemanticPostActionObservation::new(
                SemanticSettleInstant::from_millis(112),
                observation(successor, 2, 2, 1, false, false, "Private successor"),
            ),
            SemanticDiffBudget::ACTION,
        )
        .expect("navigation result");

        let mut execution = SemanticActionBatchExecution::new(&batch).expect("execution");
        assert_eq!(
            execution
                .record_success(&first, result)
                .expect("navigation success"),
            SemanticActionBatchContinuation::StopRequired(
                SemanticActionBatchStopReason::Navigation
            )
        );
        let result = execution.finish().expect("mandatory stop");
        assert_eq!(
            result.outcome(),
            SemanticActionBatchOutcome::Stopped {
                reason: SemanticActionBatchStopReason::Navigation,
                remaining: 1,
            }
        );
        assert_eq!(
            result
                .final_state()
                .expect("successor state")
                .fresh_snapshot()
                .expect("successor snapshot")
                .request()
                .context(),
            successor
        );
    }

    #[test]
    fn independently_verified_dialog_transition_mandatorily_stops_the_remainder() {
        let context = context();
        let baseline = observation(context, 1, 1, 1, false, false, "Private baseline");
        let dialog = SemanticActionProposal::try_new(
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(2).expect("target"),
            },
            SemanticEffectClass::LocalWrite,
            SemanticWaitCondition::Dialog(SemanticDialogState::Present),
            SemanticVerification::Dialog(SemanticDialogState::Present),
            SemanticSettleBudget::try_new(250).expect("budget"),
        )
        .expect("dialog proposal");
        let batch = SemanticActionBatch::bind(
            SemanticActionBatchId::new(14).expect("batch"),
            &baseline,
            &[baseline.frames()[0].frame().clone()],
            vec![dialog, checked_click(3)],
        )
        .expect("batch");
        let first = batch.actions()[0]
            .prepare(&baseline.frames()[0])
            .expect("first");
        let attempt = SemanticActionAttemptId::new(1).expect("attempt");
        let mut tracker =
            SemanticSettleTracker::begin(attempt, &first, SemanticSettleInstant::from_millis(100))
                .expect("settlement");
        tracker
            .observe(SemanticSettleEvent::new(
                attempt,
                SemanticSettleInstant::from_millis(110),
                SemanticSettleFact::Dialog {
                    context,
                    state: SemanticDialogState::Present,
                },
            ))
            .expect("dialog settle");
        let verified = verify_semantic_action(
            &tracker,
            &first,
            SemanticEffectEvidence::dialog(
                attempt,
                SemanticSettleInstant::from_millis(111),
                context,
                SemanticDialogState::Absent,
                SemanticDialogState::Present,
            ),
        )
        .expect("dialog verification");
        let result = finalize_semantic_action_result(
            &first,
            verified,
            &baseline,
            &acknowledgement(&baseline),
            SemanticPostActionObservation::new(
                SemanticSettleInstant::from_millis(112),
                observation(context, 2, 2, 2, false, false, "Private dialog state"),
            ),
            SemanticDiffBudget::ACTION,
        )
        .expect("dialog result");

        let mut execution = SemanticActionBatchExecution::new(&batch).expect("execution");
        assert_eq!(
            execution
                .record_success(&first, result)
                .expect("dialog success"),
            SemanticActionBatchContinuation::StopRequired(SemanticActionBatchStopReason::Dialog)
        );
        let result = execution.finish().expect("mandatory stop");
        assert_eq!(
            result.outcome(),
            SemanticActionBatchOutcome::Stopped {
                reason: SemanticActionBatchStopReason::Dialog,
                remaining: 1,
            }
        );
        assert_eq!(
            result.completions()[0].proof(),
            SemanticEffectProofKind::Dialog
        );
    }
}
