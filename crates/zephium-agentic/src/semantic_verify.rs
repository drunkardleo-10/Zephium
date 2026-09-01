//! Independent, bounded post-settle verification for semantic actions.
//!
//! A settle notification is only a wake condition. This module admits one
//! separately sampled, action-correlated observation and mints an opaque proof
//! only when the exact declared postcondition is established. It owns no page
//! runtime, native input, timer, queue, retry, or model-facing value.

use std::fmt;

use thiserror::Error;

use crate::semantic_settle::exact_document_successor;
use crate::{
    AgentActiveEffect, ContextJoin, SemanticActionAttemptId, SemanticActionExecutionApplied,
    SemanticActionFailure, SemanticActionRevalidationError, SemanticActionSettlementTerminal,
    SemanticCompleteness, SemanticDialogState, SemanticInvocationId, SemanticPreparedAction,
    SemanticScrollAmount, SemanticScrollDirection, SemanticSettleInstant, SemanticSettleStatus,
    SemanticSettleTracker, SemanticSnapshot, SemanticSnapshotGeneration, SemanticState,
    SemanticVerification, MAX_SEMANTIC_ACTION_TEXT_BYTES,
};

/// Largest absolute independently sampled scroll coordinate.
pub const MAX_SEMANTIC_SCROLL_COORDINATE: u64 = 1_000_000_000;

/// Bounded scroll position sampled independently of input dispatch.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticScrollPosition {
    x: i64,
    y: i64,
}

impl SemanticScrollPosition {
    /// Validates both coordinates against the fixed absolute ceiling.
    pub const fn try_new(x: i64, y: i64) -> Result<Self, SemanticScrollPositionError> {
        if x.unsigned_abs() > MAX_SEMANTIC_SCROLL_COORDINATE
            || y.unsigned_abs() > MAX_SEMANTIC_SCROLL_COORDINATE
        {
            Err(SemanticScrollPositionError::OutOfRange)
        } else {
            Ok(Self { x, y })
        }
    }

    /// Independently sampled horizontal coordinate.
    pub const fn x(self) -> i64 {
        self.x
    }

    /// Independently sampled vertical coordinate.
    pub const fn y(self) -> i64 {
        self.y
    }
}

impl fmt::Debug for SemanticScrollPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticScrollPosition([redacted])")
    }
}

/// Refusal while constructing a bounded scroll sample.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticScrollPositionError {
    /// At least one coordinate exceeded the fixed absolute ceiling.
    #[error("semantic scroll position exceeds its coordinate ceiling")]
    OutOfRange,
}

enum SemanticEffectEvidenceKind<'a> {
    Snapshot(&'a SemanticSnapshot),
    ExactTargetValue {
        snapshot: &'a SemanticSnapshot,
        before: &'a str,
        after: &'a str,
    },
    Navigation {
        prior: ContextJoin,
        current: ContextJoin,
    },
    Dialog {
        context: ContextJoin,
        before: SemanticDialogState,
        after: SemanticDialogState,
    },
    Scroll {
        snapshot: &'a SemanticSnapshot,
        before: SemanticScrollPosition,
        after: SemanticScrollPosition,
        target_visible_after: bool,
    },
}

/// One transient observation sampled independently of action dispatch.
///
/// The trusted shell must source this from the fixed observation adapter, not
/// from the backend completion response. Constructors carry no authority and
/// do not attest that a caller followed that separation.
pub struct SemanticEffectEvidence<'a> {
    attempt: SemanticActionAttemptId,
    observed_at: SemanticSettleInstant,
    kind: SemanticEffectEvidenceKind<'a>,
}

impl<'a> SemanticEffectEvidence<'a> {
    /// Joins one exact fresh semantic snapshot to an action attempt.
    pub const fn snapshot(
        attempt: SemanticActionAttemptId,
        observed_at: SemanticSettleInstant,
        snapshot: &'a SemanticSnapshot,
    ) -> Self {
        Self {
            attempt,
            observed_at,
            kind: SemanticEffectEvidenceKind::Snapshot(snapshot),
        }
    }

    /// Joins transient exact before/after target values and fresh metadata.
    ///
    /// Values remain borrowed, are never exposed by this type, and are
    /// compared only against the already bounded fill input.
    pub const fn exact_target_value(
        attempt: SemanticActionAttemptId,
        observed_at: SemanticSettleInstant,
        snapshot: &'a SemanticSnapshot,
        before: &'a str,
        after: &'a str,
    ) -> Self {
        Self {
            attempt,
            observed_at,
            kind: SemanticEffectEvidenceKind::ExactTargetValue {
                snapshot,
                before,
                after,
            },
        }
    }

    /// Joins independently sampled prior/current navigation authority.
    pub const fn navigation(
        attempt: SemanticActionAttemptId,
        observed_at: SemanticSettleInstant,
        prior: ContextJoin,
        current: ContextJoin,
    ) -> Self {
        Self {
            attempt,
            observed_at,
            kind: SemanticEffectEvidenceKind::Navigation { prior, current },
        }
    }

    /// Joins an independently sampled dialog transition without dialog text.
    pub const fn dialog(
        attempt: SemanticActionAttemptId,
        observed_at: SemanticSettleInstant,
        context: ContextJoin,
        before: SemanticDialogState,
        after: SemanticDialogState,
    ) -> Self {
        Self {
            attempt,
            observed_at,
            kind: SemanticEffectEvidenceKind::Dialog {
                context,
                before,
                after,
            },
        }
    }

    /// Joins independently sampled scroll positions and fresh target metadata.
    pub const fn scroll(
        attempt: SemanticActionAttemptId,
        observed_at: SemanticSettleInstant,
        snapshot: &'a SemanticSnapshot,
        before: SemanticScrollPosition,
        after: SemanticScrollPosition,
        target_visible_after: bool,
    ) -> Self {
        Self {
            attempt,
            observed_at,
            kind: SemanticEffectEvidenceKind::Scroll {
                snapshot,
                before,
                after,
                target_visible_after,
            },
        }
    }

    /// Exact action-attempt correlation.
    pub const fn attempt(&self) -> SemanticActionAttemptId {
        self.attempt
    }

    /// Monotonic independent-observation instant.
    pub const fn observed_at(&self) -> SemanticSettleInstant {
        self.observed_at
    }
}

impl fmt::Debug for SemanticEffectEvidence<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            SemanticEffectEvidenceKind::Snapshot(_) => "snapshot",
            SemanticEffectEvidenceKind::ExactTargetValue { .. } => "exact_target_value",
            SemanticEffectEvidenceKind::Navigation { .. } => "navigation",
            SemanticEffectEvidenceKind::Dialog { .. } => "dialog",
            SemanticEffectEvidenceKind::Scroll { .. } => "scroll",
        };
        formatter
            .debug_struct("SemanticEffectEvidence")
            .field("attempt", &self.attempt)
            .field("observed_at", &self.observed_at)
            .field("kind", &kind)
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed class of independently established effect proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticEffectProofKind {
    /// Exact allowlisted target state transitioned in the adjacent snapshot.
    TargetState,
    /// Exact safe fill value matched the requested bounded input.
    ExactTargetValue,
    /// Safe target value changed after an admitted fixed key recipe.
    TargetValueChanged,
    /// Exact bound option became selected.
    ExactSelection,
    /// Selection state changed after an admitted fixed key recipe.
    SelectionChanged,
    /// Exact next main-document authority was independently observed.
    Navigation,
    /// Expected dialog transition was independently observed.
    Dialog,
    /// Scroll position moved in the declared direction.
    Scroll,
}

/// Opaque proof that one exact settled action met its declared postcondition.
///
/// This proof is not policy authority and cannot execute or retry an action.
#[derive(Eq, PartialEq)]
pub struct SemanticVerifiedAction {
    attempt: SemanticActionAttemptId,
    ordinal: u8,
    verification: SemanticVerification,
    proof: SemanticEffectProofKind,
    observed_at: SemanticSettleInstant,
    deadline: SemanticSettleInstant,
    current_context: ContextJoin,
    current_invocation: Option<SemanticInvocationId>,
    current_snapshot: Option<SemanticSnapshotGeneration>,
    action_guard: [u8; 32],
}

impl SemanticVerifiedAction {
    /// Exact verified action attempt.
    pub const fn attempt(&self) -> SemanticActionAttemptId {
        self.attempt
    }

    /// One-based action position in its bound batch.
    pub const fn ordinal(&self) -> u8 {
        self.ordinal
    }

    /// Declared postcondition that was independently established.
    pub const fn verification(&self) -> SemanticVerification {
        self.verification
    }

    /// Closed class of admitted independent proof.
    pub const fn proof(&self) -> SemanticEffectProofKind {
        self.proof
    }

    /// Monotonic instant at which the independent effect evidence was sampled.
    pub const fn observed_at(&self) -> SemanticSettleInstant {
        self.observed_at
    }

    /// Sole absolute action deadline that also bounds post-action observation.
    pub const fn deadline(&self) -> SemanticSettleInstant {
        self.deadline
    }

    /// Exact current context authority represented by the independent proof.
    pub const fn current_context(&self) -> ContextJoin {
        self.current_context
    }

    /// Native invocation carrying semantic evidence, when a snapshot proved the effect.
    pub const fn current_invocation(&self) -> Option<SemanticInvocationId> {
        self.current_invocation
    }

    /// Adjacent post-action snapshot generation, when semantic proof was used.
    pub const fn current_snapshot(&self) -> Option<SemanticSnapshotGeneration> {
        self.current_snapshot
    }

    /// Reports whether this proof was minted for the exact bound action.
    pub fn matches_action(&self, action: &SemanticPreparedAction) -> bool {
        self.ordinal == action.ordinal() && self.action_guard == action.verification_guard()
    }
}

impl fmt::Debug for SemanticVerifiedAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticVerifiedAction")
            .field("attempt", &self.attempt)
            .field("ordinal", &self.ordinal)
            .field("verification", &self.verification)
            .field("proof", &self.proof)
            .field("observed_at", &self.observed_at)
            .field("deadline", &self.deadline)
            .field("current_context", &self.current_context)
            .field("current_invocation", &self.current_invocation)
            .field("current_snapshot", &self.current_snapshot)
            .field("action_guard", &"[redacted]")
            .finish()
    }
}

/// Refusal to mint an independent action-effect proof.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticVerificationError {
    /// Settlement has not reached its declared wake condition.
    #[error("semantic action settlement is still pending")]
    SettlementPending,
    /// Settlement already ended in one typed action failure.
    #[error("semantic action settlement failed")]
    SettlementFailed(SemanticActionFailure),
    /// Evidence belongs to another action attempt.
    #[error("semantic verification attempt mismatched")]
    AttemptMismatch,
    /// Settler and verifier were given different bound action contracts.
    #[error("semantic verification action contract mismatched")]
    ActionMismatch,
    /// Evidence was sampled before the settle condition completed.
    #[error("semantic verification evidence predates settlement")]
    EvidenceBeforeSettlement,
    /// Evidence arrived after the action's sole absolute deadline.
    #[error("semantic verification evidence exceeded the action deadline")]
    EvidenceAfterDeadline,
    /// Evidence class cannot establish this declared postcondition.
    #[error("semantic verification evidence class mismatched")]
    EvidenceKindMismatch,
    /// Snapshot/context authority is stale, skipped, or cross-frame.
    #[error("semantic verification evidence authority is stale")]
    StaleEvidence,
    /// Complete target evidence was unavailable.
    #[error("semantic verification target is missing")]
    TargetMissing,
    /// Stable target semantics changed incompatibly.
    #[error("semantic verification target changed")]
    TargetChanged,
    /// Verification crossed into credential/secret data.
    #[error("semantic verification crossed a credential boundary")]
    CredentialBoundary,
    /// Semantic proof used an incomplete snapshot.
    #[error("semantic verification snapshot is incomplete")]
    IncompleteSnapshot,
    /// Bounded transient evidence exceeded its hard ceiling.
    #[error("semantic verification evidence exceeded its resource ceiling")]
    EvidenceLimit,
    /// Independent evidence did not establish the exact postcondition.
    #[error("semantic action postcondition was not observed")]
    OutcomeNotObserved,
}

impl SemanticVerificationError {
    /// Maps verification refusal to the closed action-pipeline failure taxonomy.
    pub const fn action_failure(self) -> SemanticActionFailure {
        match self {
            Self::SettlementFailed(failure) => failure,
            Self::AttemptMismatch | Self::ActionMismatch => SemanticActionFailure::LeaseViolation,
            Self::StaleEvidence | Self::TargetMissing => SemanticActionFailure::StaleReference,
            Self::TargetChanged => SemanticActionFailure::TargetChanged,
            Self::CredentialBoundary => SemanticActionFailure::CredentialBoundary,
            Self::EvidenceLimit => SemanticActionFailure::ResourceExhausted,
            Self::SettlementPending
            | Self::EvidenceBeforeSettlement
            | Self::EvidenceAfterDeadline
            | Self::EvidenceKindMismatch
            | Self::IncompleteSnapshot
            | Self::OutcomeNotObserved => SemanticActionFailure::VerificationFailed,
        }
    }
}

/// Exact policy, execution, settlement, and independent verification success.
#[must_use]
pub struct SemanticActionVerifiedTerminal {
    active: AgentActiveEffect,
    execution: SemanticActionExecutionApplied,
    settlement: SemanticSettleTracker,
    verified: SemanticVerifiedAction,
}

impl SemanticActionVerifiedTerminal {
    /// Exact dispatched policy authority retained through verification.
    pub const fn active(&self) -> &AgentActiveEffect {
        &self.active
    }

    /// Content-free backend attribution and native execution timing.
    pub const fn execution(&self) -> SemanticActionExecutionApplied {
        self.execution
    }

    /// Exact terminal settlement state consumed by verification.
    pub const fn settlement(&self) -> &SemanticSettleTracker {
        &self.settlement
    }

    /// Opaque independently established action proof.
    pub const fn verified(&self) -> &SemanticVerifiedAction {
        &self.verified
    }

    /// Separates exact policy authority, metrics, settlement, and proof.
    pub fn into_parts(
        self,
    ) -> (
        AgentActiveEffect,
        SemanticActionExecutionApplied,
        SemanticSettleTracker,
        SemanticVerifiedAction,
    ) {
        (self.active, self.execution, self.settlement, self.verified)
    }
}

impl fmt::Debug for SemanticActionVerifiedTerminal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionVerifiedTerminal")
            .field("active", &"[redacted]")
            .field("execution", &self.execution)
            .field("settlement", &self.settlement)
            .field("verified", &self.verified)
            .finish()
    }
}

/// One-shot verification refusal that returns terminal policy authority.
#[must_use]
pub struct SemanticActionVerificationRefusal {
    terminal: Box<SemanticActionSettlementTerminal>,
    error: SemanticVerificationError,
}

impl SemanticActionVerificationRefusal {
    /// Closed independent-verification failure.
    pub const fn error(&self) -> SemanticVerificationError {
        self.error
    }

    /// Recovers terminal policy authority for one typed failed settlement.
    pub fn into_parts(
        self,
    ) -> (
        AgentActiveEffect,
        SemanticActionExecutionApplied,
        SemanticSettleTracker,
        SemanticVerificationError,
    ) {
        let (active, execution, settlement) = self.terminal.into_parts();
        (active, execution, settlement, self.error)
    }
}

impl fmt::Debug for SemanticActionVerificationRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionVerificationRefusal")
            .field("terminal", &"[redacted]")
            .field("error", &self.error)
            .finish()
    }
}

/// Consumes one exact terminal settlement into one independent proof attempt.
///
/// Refusal consumes the proof opportunity and returns policy authority for a
/// typed failed settlement. It creates no retry, timer, queue, snapshot copy,
/// page callback, or content buffer.
pub fn verify_semantic_action_terminal(
    terminal: SemanticActionSettlementTerminal,
    action: &SemanticPreparedAction,
    evidence: SemanticEffectEvidence<'_>,
) -> Result<SemanticActionVerifiedTerminal, SemanticActionVerificationRefusal> {
    let verified = match verify_semantic_action(terminal.tracker(), action, evidence) {
        Ok(verified) => verified,
        Err(error) => {
            return Err(SemanticActionVerificationRefusal {
                terminal: Box::new(terminal),
                error,
            });
        }
    };
    let (active, execution, settlement) = terminal.into_parts();
    Ok(SemanticActionVerifiedTerminal {
        active,
        execution,
        settlement,
        verified,
    })
}

/// Independently proves one exact settled action postcondition.
///
/// No settle fact is accepted as effect evidence. Snapshot proofs require the
/// exact adjacent snapshot; navigation/dialog/scroll facts must come from the
/// separately sampled closed evidence vocabulary.
pub(crate) fn verify_semantic_action(
    settlement: &SemanticSettleTracker,
    action: &SemanticPreparedAction,
    evidence: SemanticEffectEvidence<'_>,
) -> Result<SemanticVerifiedAction, SemanticVerificationError> {
    match settlement.status() {
        SemanticSettleStatus::Pending => return Err(SemanticVerificationError::SettlementPending),
        SemanticSettleStatus::Failed(failure) => {
            return Err(SemanticVerificationError::SettlementFailed(failure));
        }
        SemanticSettleStatus::ReadyForVerification => {}
    }
    if !settlement.matches_action(action) {
        return Err(SemanticVerificationError::ActionMismatch);
    }
    if evidence.attempt != settlement.attempt() {
        return Err(SemanticVerificationError::AttemptMismatch);
    }
    let terminal_at = settlement
        .terminal_at()
        .ok_or(SemanticVerificationError::SettlementPending)?;
    if evidence.observed_at < terminal_at {
        return Err(SemanticVerificationError::EvidenceBeforeSettlement);
    }
    if evidence.observed_at > settlement.deadline() {
        return Err(SemanticVerificationError::EvidenceAfterDeadline);
    }

    let (proof, current_context, current_invocation, current_snapshot) =
        match (action.verification(), evidence.kind) {
            (
                SemanticVerification::TargetState { state, present },
                SemanticEffectEvidenceKind::Snapshot(snapshot),
            ) => {
                let (_, target) = verification_target(action, snapshot)?;
                if target.states().contains(state) != present {
                    return Err(SemanticVerificationError::OutcomeNotObserved);
                }
                (
                    SemanticEffectProofKind::TargetState,
                    snapshot.frame().context(),
                    Some(snapshot.invocation()),
                    Some(snapshot.generation()),
                )
            }
            (
                SemanticVerification::TargetValueMatchesInput,
                SemanticEffectEvidenceKind::ExactTargetValue {
                    snapshot,
                    before,
                    after,
                },
            ) => {
                let _ = verification_target(action, snapshot)?;
                let expected = action
                    .fill_text()
                    .ok_or(SemanticVerificationError::ActionMismatch)?;
                if before.len() > MAX_SEMANTIC_ACTION_TEXT_BYTES
                    || after.len() > MAX_SEMANTIC_ACTION_TEXT_BYTES
                {
                    return Err(SemanticVerificationError::EvidenceLimit);
                }
                if before == expected.as_str() || after != expected.as_str() {
                    return Err(SemanticVerificationError::OutcomeNotObserved);
                }
                (
                    SemanticEffectProofKind::ExactTargetValue,
                    snapshot.frame().context(),
                    Some(snapshot.invocation()),
                    Some(snapshot.generation()),
                )
            }
            (
                SemanticVerification::TargetValueChanged,
                SemanticEffectEvidenceKind::Snapshot(snapshot),
            ) => {
                let (_, target) = verification_target(action, snapshot)?;
                if target.value() == action.target_value() {
                    return Err(SemanticVerificationError::OutcomeNotObserved);
                }
                (
                    SemanticEffectProofKind::TargetValueChanged,
                    snapshot.frame().context(),
                    Some(snapshot.invocation()),
                    Some(snapshot.generation()),
                )
            }
            (
                SemanticVerification::TargetSelectionMatchesOption,
                SemanticEffectEvidenceKind::Snapshot(snapshot),
            ) => {
                let (target_index, _) = verification_target(action, snapshot)?;
                let (_, option) = action
                    .verification_option(snapshot, target_index)
                    .map_err(map_target_error)?;
                if !option.states().contains(SemanticState::Selected) {
                    return Err(SemanticVerificationError::OutcomeNotObserved);
                }
                (
                    SemanticEffectProofKind::ExactSelection,
                    snapshot.frame().context(),
                    Some(snapshot.invocation()),
                    Some(snapshot.generation()),
                )
            }
            (
                SemanticVerification::TargetSelectionChanged,
                SemanticEffectEvidenceKind::Snapshot(snapshot),
            ) => {
                let (_, target) = verification_target(action, snapshot)?;
                let selected_changed = target.states().contains(SemanticState::Selected)
                    != action.target_states().contains(SemanticState::Selected);
                if !selected_changed && target.value() == action.target_value() {
                    return Err(SemanticVerificationError::OutcomeNotObserved);
                }
                (
                    SemanticEffectProofKind::SelectionChanged,
                    snapshot.frame().context(),
                    Some(snapshot.invocation()),
                    Some(snapshot.generation()),
                )
            }
            (
                SemanticVerification::NavigationCommitted,
                SemanticEffectEvidenceKind::Navigation { prior, current },
            ) => {
                if prior != action.frame().context() || !exact_document_successor(prior, current) {
                    return Err(SemanticVerificationError::StaleEvidence);
                }
                (SemanticEffectProofKind::Navigation, current, None, None)
            }
            (
                SemanticVerification::Dialog(expected),
                SemanticEffectEvidenceKind::Dialog {
                    context,
                    before,
                    after,
                },
            ) => {
                if context != action.frame().context() {
                    return Err(SemanticVerificationError::StaleEvidence);
                }
                if before == after || after != expected {
                    return Err(SemanticVerificationError::OutcomeNotObserved);
                }
                (SemanticEffectProofKind::Dialog, context, None, None)
            }
            (
                SemanticVerification::ScrollPositionChanged,
                SemanticEffectEvidenceKind::Scroll {
                    snapshot,
                    before,
                    after,
                    target_visible_after,
                },
            ) => {
                let _ = verification_target(action, snapshot)?;
                let (direction, amount) = action
                    .scroll_recipe()
                    .ok_or(SemanticVerificationError::ActionMismatch)?;
                if !moved_in_direction(before, after, direction)
                    || (amount == SemanticScrollAmount::IntoView && !target_visible_after)
                {
                    return Err(SemanticVerificationError::OutcomeNotObserved);
                }
                (
                    SemanticEffectProofKind::Scroll,
                    snapshot.frame().context(),
                    Some(snapshot.invocation()),
                    Some(snapshot.generation()),
                )
            }
            _ => return Err(SemanticVerificationError::EvidenceKindMismatch),
        };

    Ok(SemanticVerifiedAction {
        attempt: evidence.attempt,
        ordinal: action.ordinal(),
        verification: action.verification(),
        proof,
        observed_at: evidence.observed_at,
        deadline: settlement.deadline(),
        current_context,
        current_invocation,
        current_snapshot,
        action_guard: action.verification_guard(),
    })
}

fn verification_target<'a>(
    action: &SemanticPreparedAction,
    snapshot: &'a SemanticSnapshot,
) -> Result<(usize, &'a crate::SemanticNode), SemanticVerificationError> {
    if snapshot.completeness() != SemanticCompleteness::Complete {
        return Err(SemanticVerificationError::IncompleteSnapshot);
    }
    action
        .verification_target(snapshot)
        .map_err(map_target_error)
}

fn map_target_error(error: SemanticActionRevalidationError) -> SemanticVerificationError {
    match error {
        SemanticActionRevalidationError::StaleAuthority => SemanticVerificationError::StaleEvidence,
        SemanticActionRevalidationError::TargetMissing => SemanticVerificationError::TargetMissing,
        SemanticActionRevalidationError::TargetChanged
        | SemanticActionRevalidationError::OperationDenied
        | SemanticActionRevalidationError::TargetDisabled
        | SemanticActionRevalidationError::SelectionTarget => {
            SemanticVerificationError::TargetChanged
        }
        SemanticActionRevalidationError::CredentialBoundary => {
            SemanticVerificationError::CredentialBoundary
        }
    }
}

fn moved_in_direction(
    before: SemanticScrollPosition,
    after: SemanticScrollPosition,
    direction: SemanticScrollDirection,
) -> bool {
    match direction {
        SemanticScrollDirection::Up => after.y < before.y,
        SemanticScrollDirection::Down => after.y > before.y,
        SemanticScrollDirection::Left => after.x < before.x,
        SemanticScrollDirection::Right => after.x > before.x,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, FrameId, SemanticActionBatch, SemanticActionBatchId,
        SemanticActionBindingError, SemanticActionIntent, SemanticActionProposal,
        SemanticActionText, SemanticDecodeContext, SemanticEffectClass, SemanticFrameJoin,
        SemanticFrameTrust, SemanticInvocationId, SemanticObservation,
        SemanticObservationAssembler, SemanticObservationBudget, SemanticObservationId,
        SemanticOrigin, SemanticPressKey, SemanticReferenceId, SemanticScrollAmount,
        SemanticScrollDirection, SemanticSettleBudget, SemanticSettleEvent, SemanticSettleFact,
        SemanticVerification, SemanticWaitCondition, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn observation() -> (SemanticObservation, ContextRegistry) {
        let identity = ContextIdentity::new(
            ContextId::from_raw(71),
            ContextRunId::from_raw(72),
            ProfileId::from(73),
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
        let context = registry.join(identity.id()).expect("join");
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://verify-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let snapshot = snapshot_for_frame(
            frame,
            1,
            1,
            "complete",
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Private submit", "o": 1},
                {"k": 3, "p": 0, "r": "textbox", "n": "Private title",
                 "v": {"k": "text", "value": "old"}, "o": 10,
                 "b": {"x": 10, "y": 20, "w": 200, "h": 30}},
                {"k": 4, "p": 0, "r": "checkbox", "n": "Private toggle", "o": 1},
                {"k": 5, "p": 0, "r": "combobox", "n": "Private priority",
                 "v": {"k": "ordinal", "value": 0}, "o": 12},
                {"k": 6, "p": 4, "r": "option", "n": "Private high", "o": 1}
            ]),
        );
        let request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let observation = SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation");
        (observation, registry)
    }

    fn snapshot_for_frame(
        frame: SemanticFrameJoin,
        invocation: u64,
        generation: u64,
        completeness: &str,
        nodes: serde_json::Value,
    ) -> SemanticSnapshot {
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": generation,
            "c": completeness,
            "n": nodes,
        }))
        .expect("wire");
        decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(invocation).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(generation).expect("generation"),
            ),
            &bytes,
        )
        .expect("snapshot")
    }

    fn current(observation: &SemanticObservation, nodes: serde_json::Value) -> SemanticSnapshot {
        snapshot_for_frame(
            observation.frames()[0].frame().clone(),
            2,
            2,
            "complete",
            nodes,
        )
    }

    fn bind(
        observation: &SemanticObservation,
        intent: SemanticActionIntent,
        wait: SemanticWaitCondition,
        verification: SemanticVerification,
    ) -> Result<SemanticActionBatch, SemanticActionBindingError> {
        let proposal = SemanticActionProposal::try_new(
            intent,
            SemanticEffectClass::LocalWrite,
            wait,
            verification,
            SemanticSettleBudget::try_new(250).expect("budget"),
        )
        .expect("proposal");
        SemanticActionBatch::bind(
            SemanticActionBatchId::new(1).expect("batch"),
            observation,
            &[observation.frames()[0].frame().clone()],
            vec![proposal],
        )
    }

    fn immediate(action: &SemanticPreparedAction, attempt: u64) -> SemanticSettleTracker {
        SemanticSettleTracker::begin(
            SemanticActionAttemptId::new(attempt).expect("attempt"),
            action,
            SemanticSettleInstant::from_millis(100),
        )
        .expect("tracker")
    }

    fn execution_terminal(
        action: &SemanticPreparedAction,
        attempt: u64,
    ) -> SemanticActionSettlementTerminal {
        let active = AgentActiveEffect::for_execution_test(
            action,
            SemanticActionAttemptId::new(attempt).expect("attempt"),
        );
        let (pending, native) = crate::prepare_semantic_action_execution(
            active,
            action,
            crate::SemanticActionExecutionInstant::from_millis(80),
        )
        .expect("execution");
        let actual_geometry = native.expected_geometry();
        let outcome = pending.settle(
            action.frame(),
            native.complete(
                crate::SemanticActionExecutionBackend::FixedSemanticRecipe,
                crate::SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
                crate::SemanticActionNativeViewport::try_new(800, 600).expect("viewport"),
                actual_geometry,
                crate::SemanticActionExecutionInstant::from_millis(90),
                crate::SemanticActionExecutionInstant::from_millis(100),
            ),
        );
        let start = crate::begin_semantic_action_settlement(outcome, action).expect("settlement");
        let mut coordinator = crate::SemanticActionSettlementCoordinator::new();
        let crate::SemanticActionSettlementUpdate::Terminal(terminal) =
            coordinator.begin(start).expect("terminal")
        else {
            panic!("immediate settlement was pending");
        };
        *terminal
    }

    #[test]
    fn terminal_verification_consumes_one_exact_proof_opportunity() {
        let (observation, _) = observation();
        let batch = bind(
            &observation,
            SemanticActionIntent::Fill {
                target: SemanticReferenceId::new(3).expect("target"),
                value: SemanticActionText::try_new("new private title".to_owned()).expect("text"),
            },
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetValueMatchesInput,
        )
        .expect("batch");
        let action = batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        let snapshot = current(
            &observation,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 3, "p": 0, "r": "textbox", "n": "Private title",
                 "v": {"k": "text", "value": "new private title"}, "o": 10}
            ]),
        );

        let success = verify_semantic_action_terminal(
            execution_terminal(&action, 21),
            &action,
            SemanticEffectEvidence::exact_target_value(
                SemanticActionAttemptId::new(21).expect("attempt"),
                SemanticSettleInstant::from_millis(101),
                &snapshot,
                "old",
                "new private title",
            ),
        )
        .expect("verification");
        assert_eq!(success.active().attempt().get(), 21);
        assert_eq!(
            success.execution().backend(),
            crate::SemanticActionExecutionBackend::FixedSemanticRecipe
        );
        assert_eq!(
            success.settlement().status(),
            SemanticSettleStatus::ReadyForVerification
        );
        assert_eq!(
            success.verified().proof(),
            SemanticEffectProofKind::ExactTargetValue
        );
        let debug = format!("{success:?}");
        assert!(!debug.contains("new private title"));
        let (active, _execution, settlement, verified) = success.into_parts();
        assert_eq!(active.attempt().get(), 21);
        assert_eq!(settlement.attempt().get(), 21);
        assert_eq!(verified.attempt().get(), 21);

        let refusal = verify_semantic_action_terminal(
            execution_terminal(&action, 22),
            &action,
            SemanticEffectEvidence::exact_target_value(
                SemanticActionAttemptId::new(22).expect("attempt"),
                SemanticSettleInstant::from_millis(101),
                &snapshot,
                "old",
                "wrong private title",
            ),
        )
        .expect_err("unproven outcome");
        assert_eq!(
            refusal.error(),
            SemanticVerificationError::OutcomeNotObserved
        );
        assert!(!format!("{refusal:?}").contains("wrong private title"));
        let (active, _execution, settlement, error) = refusal.into_parts();
        assert_eq!(active.attempt().get(), 22);
        assert_eq!(settlement.attempt().get(), 22);
        assert_eq!(
            error.action_failure(),
            SemanticActionFailure::VerificationFailed
        );

        let substituted_batch = bind(
            &observation,
            SemanticActionIntent::Fill {
                target: SemanticReferenceId::new(3).expect("target"),
                value: SemanticActionText::try_new("different private title".to_owned())
                    .expect("text"),
            },
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetValueMatchesInput,
        )
        .expect("batch");
        let substituted = substituted_batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        let refusal = verify_semantic_action_terminal(
            execution_terminal(&action, 23),
            &substituted,
            SemanticEffectEvidence::exact_target_value(
                SemanticActionAttemptId::new(23).expect("attempt"),
                SemanticSettleInstant::from_millis(101),
                &snapshot,
                "old",
                "new private title",
            ),
        )
        .expect_err("substituted action");
        assert_eq!(refusal.error(), SemanticVerificationError::ActionMismatch);
        assert_eq!(refusal.into_parts().0.attempt().get(), 23);
    }

    #[test]
    fn exact_fill_uses_transient_unmodified_value_and_adjacent_metadata() {
        let (observation, _) = observation();
        let batch = bind(
            &observation,
            SemanticActionIntent::Fill {
                target: SemanticReferenceId::new(3).expect("target"),
                value: SemanticActionText::try_new("new private title".to_owned()).expect("text"),
            },
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetValueMatchesInput,
        )
        .expect("batch");
        let prepared = batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        let action = &prepared;
        let attempt = SemanticActionAttemptId::new(1).expect("attempt");
        let tracker = immediate(action, 1);
        let snapshot = current(
            &observation,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 3, "p": 0, "r": "textbox", "n": "Private title",
                 "v": {"k": "text", "value": "new private title"}, "o": 10}
            ]),
        );

        assert_eq!(
            verify_semantic_action(
                &tracker,
                action,
                SemanticEffectEvidence::exact_target_value(
                    attempt,
                    SemanticSettleInstant::from_millis(101),
                    &snapshot,
                    "old",
                    "wrong private title",
                ),
            ),
            Err(SemanticVerificationError::OutcomeNotObserved)
        );
        let oversized = "x".repeat(MAX_SEMANTIC_ACTION_TEXT_BYTES + 1);
        assert_eq!(
            verify_semantic_action(
                &tracker,
                action,
                SemanticEffectEvidence::exact_target_value(
                    attempt,
                    SemanticSettleInstant::from_millis(101),
                    &snapshot,
                    &oversized,
                    "new private title",
                ),
            ),
            Err(SemanticVerificationError::EvidenceLimit)
        );
        let proof = verify_semantic_action(
            &tracker,
            action,
            SemanticEffectEvidence::exact_target_value(
                attempt,
                SemanticSettleInstant::from_millis(101),
                &snapshot,
                "old",
                "new private title",
            ),
        )
        .expect("proof");
        assert_eq!(proof.proof(), SemanticEffectProofKind::ExactTargetValue);
        assert_eq!(proof.current_snapshot(), snapshot.generation().into());
        assert!(proof.matches_action(action));
        let debug = format!("{proof:?}");
        assert!(!debug.contains("new private title"));
        assert!(!format!(
            "{:?}",
            SemanticEffectEvidence::exact_target_value(
                attempt,
                SemanticSettleInstant::from_millis(101),
                &snapshot,
                "old",
                "new private title"
            )
        )
        .contains("new private title"));
        assert_eq!(
            verify_semantic_action(
                &tracker,
                action,
                SemanticEffectEvidence::exact_target_value(
                    attempt,
                    SemanticSettleInstant::from_millis(101),
                    &snapshot,
                    "new private title",
                    "new private title",
                ),
            ),
            Err(SemanticVerificationError::OutcomeNotObserved)
        );

        let other_frame = SemanticFrameJoin::try_new(
            observation.request().context(),
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://other-verify.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let other_snapshot = snapshot_for_frame(
            other_frame,
            11,
            1,
            "complete",
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 3, "p": 0, "r": "textbox", "n": "Private title",
                 "v": {"k": "text", "value": "old"}, "o": 10}
            ]),
        );
        let other_request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(2).expect("observation"),
            observation.request().context(),
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let other_observation = SemanticObservationAssembler::new(other_request, other_snapshot)
            .expect("assembler")
            .finish()
            .expect("observation");
        let other_batch = bind(
            &other_observation,
            SemanticActionIntent::Fill {
                target: SemanticReferenceId::new(2).expect("target"),
                value: SemanticActionText::try_new("new private title".to_owned()).expect("text"),
            },
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetValueMatchesInput,
        )
        .expect("batch");
        let other_prepared = other_batch.actions()[0]
            .prepare(&other_observation.frames()[0])
            .expect("prepare");
        assert!(!proof.matches_action(&other_prepared));
    }

    #[test]
    fn target_state_requires_a_real_transition_and_exact_adjacent_snapshot() {
        let (observation, _) = observation();
        assert_eq!(
            bind(
                &observation,
                SemanticActionIntent::Click {
                    target: SemanticReferenceId::new(2).expect("target"),
                },
                SemanticWaitCondition::Immediate,
                SemanticVerification::TargetState {
                    state: SemanticState::Focused,
                    present: false,
                },
            ),
            Err(SemanticActionBindingError::OutcomeAlreadySatisfied)
        );

        let batch = bind(
            &observation,
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(4).expect("target"),
            },
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetState {
                state: SemanticState::Checked,
                present: true,
            },
        )
        .expect("batch");
        let prepared = batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        let action = &prepared;
        let tracker = immediate(action, 2);
        let snapshot = current(
            &observation,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 4, "p": 0, "r": "checkbox", "n": "Private toggle", "s": 1, "o": 1}
            ]),
        );
        let proof = verify_semantic_action(
            &tracker,
            action,
            SemanticEffectEvidence::snapshot(
                SemanticActionAttemptId::new(2).expect("attempt"),
                SemanticSettleInstant::from_millis(101),
                &snapshot,
            ),
        )
        .expect("proof");
        assert_eq!(proof.proof(), SemanticEffectProofKind::TargetState);

        let skipped = snapshot_for_frame(
            observation.frames()[0].frame().clone(),
            3,
            3,
            "complete",
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 4, "p": 0, "r": "checkbox", "n": "Private toggle", "s": 1, "o": 1}
            ]),
        );
        assert_eq!(
            verify_semantic_action(
                &tracker,
                action,
                SemanticEffectEvidence::snapshot(
                    SemanticActionAttemptId::new(2).expect("attempt"),
                    SemanticSettleInstant::from_millis(102),
                    &skipped,
                ),
            ),
            Err(SemanticVerificationError::StaleEvidence)
        );
        let incomplete = snapshot_for_frame(
            observation.frames()[0].frame().clone(),
            4,
            2,
            "node_limit",
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 4, "p": 0, "r": "checkbox", "n": "Private toggle", "s": 1, "o": 1}
            ]),
        );
        assert_eq!(
            verify_semantic_action(
                &tracker,
                action,
                SemanticEffectEvidence::snapshot(
                    SemanticActionAttemptId::new(2).expect("attempt"),
                    SemanticSettleInstant::from_millis(102),
                    &incomplete,
                ),
            ),
            Err(SemanticVerificationError::IncompleteSnapshot)
        );
    }

    #[test]
    fn selection_proof_requires_the_exact_bound_option() {
        let (observation, _) = observation();
        let batch = bind(
            &observation,
            SemanticActionIntent::Select {
                target: SemanticReferenceId::new(5).expect("target"),
                option: SemanticReferenceId::new(6).expect("option"),
            },
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetSelectionMatchesOption,
        )
        .expect("batch");
        let prepared = batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        let action = &prepared;
        let tracker = immediate(action, 3);
        let unselected = current(
            &observation,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 5, "p": 0, "r": "combobox", "n": "Private priority",
                 "v": {"k": "ordinal", "value": 0}, "o": 12},
                {"k": 6, "p": 1, "r": "option", "n": "Private high", "o": 1}
            ]),
        );
        assert_eq!(
            verify_semantic_action(
                &tracker,
                action,
                SemanticEffectEvidence::snapshot(
                    SemanticActionAttemptId::new(3).expect("attempt"),
                    SemanticSettleInstant::from_millis(101),
                    &unselected,
                ),
            ),
            Err(SemanticVerificationError::OutcomeNotObserved)
        );
        let selected = current(
            &observation,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 5, "p": 0, "r": "combobox", "n": "Private priority",
                 "v": {"k": "ordinal", "value": 1}, "o": 12},
                {"k": 6, "p": 1, "r": "option", "n": "Private high", "s": 2, "o": 1}
            ]),
        );
        let proof = verify_semantic_action(
            &tracker,
            action,
            SemanticEffectEvidence::snapshot(
                SemanticActionAttemptId::new(3).expect("attempt"),
                SemanticSettleInstant::from_millis(101),
                &selected,
            ),
        )
        .expect("proof");
        assert_eq!(proof.proof(), SemanticEffectProofKind::ExactSelection);
    }

    #[test]
    fn fixed_key_value_change_uses_pre_and_post_target_semantics() {
        let (observation, _) = observation();
        let batch = bind(
            &observation,
            SemanticActionIntent::Press {
                target: SemanticReferenceId::new(3).expect("target"),
                key: SemanticPressKey::Backspace,
            },
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetValueChanged,
        )
        .expect("batch");
        let prepared = batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        let action = &prepared;
        let tracker = immediate(action, 4);
        let unchanged = current(
            &observation,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 3, "p": 0, "r": "textbox", "n": "Private title",
                 "v": {"k": "text", "value": "old"}, "o": 10}
            ]),
        );
        assert_eq!(
            verify_semantic_action(
                &tracker,
                action,
                SemanticEffectEvidence::snapshot(
                    SemanticActionAttemptId::new(4).expect("attempt"),
                    SemanticSettleInstant::from_millis(101),
                    &unchanged,
                ),
            ),
            Err(SemanticVerificationError::OutcomeNotObserved)
        );
        let changed = current(
            &observation,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 3, "p": 0, "r": "textbox", "n": "Private title",
                 "v": {"k": "text", "value": "ol"}, "o": 10}
            ]),
        );
        assert!(verify_semantic_action(
            &tracker,
            action,
            SemanticEffectEvidence::snapshot(
                SemanticActionAttemptId::new(4).expect("attempt"),
                SemanticSettleInstant::from_millis(101),
                &changed,
            ),
        )
        .is_ok());
    }

    #[test]
    fn navigation_and_dialog_require_independent_exact_transitions() {
        let (observation, mut registry) = observation();
        let navigation_batch = bind(
            &observation,
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(2).expect("target"),
            },
            SemanticWaitCondition::NavigationCommitted,
            SemanticVerification::NavigationCommitted,
        )
        .expect("batch");
        let navigation_prepared = navigation_batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        let navigation_action = &navigation_prepared;
        let navigation_attempt = SemanticActionAttemptId::new(5).expect("attempt");
        let mut navigation_tracker = SemanticSettleTracker::begin(
            navigation_attempt,
            navigation_action,
            SemanticSettleInstant::from_millis(100),
        )
        .expect("tracker");
        let navigation = registry
            .begin_navigation(
                observation.request().context().identity().id(),
                ContextOperationId::new(2).expect("operation"),
            )
            .expect("navigation")
            .context();
        navigation_tracker
            .observe(SemanticSettleEvent::new(
                navigation_attempt,
                SemanticSettleInstant::from_millis(110),
                SemanticSettleFact::NavigationCommitted(navigation),
            ))
            .expect("settle");
        let navigation_proof = verify_semantic_action(
            &navigation_tracker,
            navigation_action,
            SemanticEffectEvidence::navigation(
                navigation_attempt,
                SemanticSettleInstant::from_millis(111),
                navigation_action.frame().context(),
                navigation,
            ),
        )
        .expect("proof");
        assert_eq!(
            navigation_proof.proof(),
            SemanticEffectProofKind::Navigation
        );

        let dialog_batch = bind(
            &observation,
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(2).expect("target"),
            },
            SemanticWaitCondition::Dialog(SemanticDialogState::Present),
            SemanticVerification::Dialog(SemanticDialogState::Present),
        )
        .expect("batch");
        let dialog_prepared = dialog_batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        let dialog_action = &dialog_prepared;
        let dialog_attempt = SemanticActionAttemptId::new(6).expect("attempt");
        let mut dialog_tracker = SemanticSettleTracker::begin(
            dialog_attempt,
            dialog_action,
            SemanticSettleInstant::from_millis(100),
        )
        .expect("tracker");
        dialog_tracker
            .observe(SemanticSettleEvent::new(
                dialog_attempt,
                SemanticSettleInstant::from_millis(110),
                SemanticSettleFact::Dialog {
                    context: dialog_action.frame().context(),
                    state: SemanticDialogState::Present,
                },
            ))
            .expect("settle");
        assert_eq!(
            verify_semantic_action(
                &dialog_tracker,
                dialog_action,
                SemanticEffectEvidence::dialog(
                    dialog_attempt,
                    SemanticSettleInstant::from_millis(111),
                    dialog_action.frame().context(),
                    SemanticDialogState::Present,
                    SemanticDialogState::Present,
                ),
            ),
            Err(SemanticVerificationError::OutcomeNotObserved)
        );
        assert!(verify_semantic_action(
            &dialog_tracker,
            dialog_action,
            SemanticEffectEvidence::dialog(
                dialog_attempt,
                SemanticSettleInstant::from_millis(111),
                dialog_action.frame().context(),
                SemanticDialogState::Absent,
                SemanticDialogState::Present,
            ),
        )
        .is_ok());
    }

    #[test]
    fn scroll_proof_checks_direction_visibility_and_coordinate_bounds() {
        assert_eq!(
            SemanticScrollPosition::try_new(MAX_SEMANTIC_SCROLL_COORDINATE as i64 + 1, 0),
            Err(SemanticScrollPositionError::OutOfRange)
        );
        let (observation, _) = observation();
        let batch = bind(
            &observation,
            SemanticActionIntent::Scroll {
                target: SemanticReferenceId::new(1).expect("target"),
                direction: SemanticScrollDirection::Down,
                amount: SemanticScrollAmount::IntoView,
            },
            SemanticWaitCondition::Immediate,
            SemanticVerification::ScrollPositionChanged,
        )
        .expect("batch");
        let prepared = batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        let action = &prepared;
        let tracker = immediate(action, 7);
        let snapshot = current(&observation, json!([{ "k": 1, "r": "document", "o": 16 }]));
        let before = SemanticScrollPosition::try_new(0, 10).expect("position");
        let after = SemanticScrollPosition::try_new(0, 30).expect("position");
        let evidence = |after, visible| {
            SemanticEffectEvidence::scroll(
                SemanticActionAttemptId::new(7).expect("attempt"),
                SemanticSettleInstant::from_millis(101),
                &snapshot,
                before,
                after,
                visible,
            )
        };
        assert_eq!(
            verify_semantic_action(&tracker, action, evidence(after, false)),
            Err(SemanticVerificationError::OutcomeNotObserved)
        );
        assert_eq!(
            verify_semantic_action(
                &tracker,
                action,
                evidence(
                    SemanticScrollPosition::try_new(0, 0).expect("position"),
                    true
                ),
            ),
            Err(SemanticVerificationError::OutcomeNotObserved)
        );
        let proof = verify_semantic_action(&tracker, action, evidence(after, true)).expect("proof");
        assert_eq!(proof.proof(), SemanticEffectProofKind::Scroll);
    }

    #[test]
    fn verifier_rejects_pending_wrong_attempt_wrong_action_and_deadline() {
        let (observation, _) = observation();
        let first = bind(
            &observation,
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(4).expect("target"),
            },
            SemanticWaitCondition::SemanticChange,
            SemanticVerification::TargetState {
                state: SemanticState::Checked,
                present: true,
            },
        )
        .expect("batch");
        let first_prepared = first.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        let first_action = &first_prepared;
        let pending = SemanticSettleTracker::begin(
            SemanticActionAttemptId::new(8).expect("attempt"),
            first_action,
            SemanticSettleInstant::from_millis(100),
        )
        .expect("tracker");
        let snapshot = current(
            &observation,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 4, "p": 0, "r": "checkbox", "n": "Private toggle", "s": 1, "o": 1}
            ]),
        );
        assert_eq!(
            verify_semantic_action(
                &pending,
                first_action,
                SemanticEffectEvidence::snapshot(
                    SemanticActionAttemptId::new(8).expect("attempt"),
                    SemanticSettleInstant::from_millis(101),
                    &snapshot,
                ),
            ),
            Err(SemanticVerificationError::SettlementPending)
        );

        let immediate_first = bind(
            &observation,
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(4).expect("target"),
            },
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetState {
                state: SemanticState::Checked,
                present: true,
            },
        )
        .expect("batch");
        let immediate_prepared = immediate_first.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        let first_action = &immediate_prepared;
        let tracker = immediate(first_action, 9);
        assert_eq!(
            verify_semantic_action(
                &tracker,
                first_action,
                SemanticEffectEvidence::snapshot(
                    SemanticActionAttemptId::new(10).expect("attempt"),
                    SemanticSettleInstant::from_millis(101),
                    &snapshot,
                ),
            ),
            Err(SemanticVerificationError::AttemptMismatch)
        );
        assert_eq!(
            verify_semantic_action(
                &tracker,
                first_action,
                SemanticEffectEvidence::snapshot(
                    SemanticActionAttemptId::new(9).expect("attempt"),
                    SemanticSettleInstant::from_millis(99),
                    &snapshot,
                ),
            ),
            Err(SemanticVerificationError::EvidenceBeforeSettlement)
        );
        assert_eq!(
            verify_semantic_action(
                &tracker,
                first_action,
                SemanticEffectEvidence::dialog(
                    SemanticActionAttemptId::new(9).expect("attempt"),
                    SemanticSettleInstant::from_millis(101),
                    first_action.frame().context(),
                    SemanticDialogState::Absent,
                    SemanticDialogState::Present,
                ),
            ),
            Err(SemanticVerificationError::EvidenceKindMismatch)
        );

        let other = bind(
            &observation,
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(2).expect("target"),
            },
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
        )
        .expect("batch");
        let other_prepared = other.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare");
        assert_eq!(
            verify_semantic_action(
                &tracker,
                &other_prepared,
                SemanticEffectEvidence::snapshot(
                    SemanticActionAttemptId::new(9).expect("attempt"),
                    SemanticSettleInstant::from_millis(101),
                    &snapshot,
                ),
            ),
            Err(SemanticVerificationError::ActionMismatch)
        );
        assert_eq!(
            verify_semantic_action(
                &tracker,
                first_action,
                SemanticEffectEvidence::snapshot(
                    SemanticActionAttemptId::new(9).expect("attempt"),
                    SemanticSettleInstant::from_millis(351),
                    &snapshot,
                ),
            ),
            Err(SemanticVerificationError::EvidenceAfterDeadline)
        );
        assert_eq!(
            SemanticVerificationError::EvidenceLimit.action_failure(),
            SemanticActionFailure::ResourceExhausted
        );
    }
}
