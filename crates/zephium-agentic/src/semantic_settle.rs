//! Event-driven, bounded settlement for one already-dispatched semantic action.
//!
//! Settlement is only readiness to perform independent effect verification. It
//! is never authorization, backend success, or proof of the requested effect.
//! The imperative shell owns the sole deadline timer and delivers exact native
//! facts; this functional core owns no timer, queue, thread, or page callback.

use std::fmt;
use std::num::NonZeroU64;

use thiserror::Error;

use crate::semantic::SemanticNodeKey;
use crate::{
    ContextJoin, FrameId, SemanticActionRevalidationError, SemanticCompleteness,
    SemanticDialogState, SemanticFrameJoin, SemanticPreparedAction, SemanticSnapshot,
    SemanticSnapshotGeneration, SemanticWaitCondition,
};

/// Maximum exact facts or snapshots one action settlement may consume.
pub const MAX_SEMANTIC_SETTLE_EVENTS: u16 = 128;

/// Nonzero shell-minted identity for one exact action execution attempt.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticActionAttemptId(NonZeroU64);

impl SemanticActionAttemptId {
    /// Constructs one nonzero process-local attempt identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the process-local correlation value to trusted orchestration.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for SemanticActionAttemptId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticActionAttemptId([redacted])")
    }
}

/// Monotonic process-local time supplied by the imperative shell.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticSettleInstant(u64);

impl SemanticSettleInstant {
    /// Wraps a monotonic millisecond tick from one process-local clock domain.
    pub const fn from_millis(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw process-local tick only to the deadline/metrics owner.
    pub const fn millis(self) -> u64 {
        self.0
    }

    const fn checked_add(self, millis: u32) -> Option<Self> {
        match self.0.checked_add(millis as u64) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    const fn elapsed_since(self, earlier: Self) -> Option<u64> {
        self.0.checked_sub(earlier.0)
    }
}

impl fmt::Debug for SemanticSettleInstant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticSettleInstant([redacted])")
    }
}

/// Closed trusted-shell fact used only to decide when verification may run.
#[derive(Clone, Eq, PartialEq)]
pub enum SemanticSettleFact {
    /// Deadline/quiet-period wake without a browser claim.
    Tick,
    /// A new main-frame document committed under the exact context lineage.
    NavigationCommitted(ContextJoin),
    /// The current or directly succeeding document reached the fixed ready state.
    DocumentReady(ContextJoin),
    /// Native-attested committed URL changed; the URL itself is absent.
    UrlChanged(ContextJoin),
    /// Bounded document title state changed; the title itself is absent.
    TitleChanged(ContextJoin),
    /// Browser/page dialog state reached a closed expected class.
    Dialog {
        /// Exact context authority in which the dialog fact was observed.
        context: ContextJoin,
        /// Present or absent.
        state: SemanticDialogState,
    },
    /// The exact action frame emitted a bounded semantic-change signal.
    SemanticChange(SemanticFrameJoin),
    /// One coalesced mutation signal for the exact action frame.
    Mutation(SemanticFrameJoin),
    /// Independently sampled scroll position changed in the exact action frame.
    ScrollPositionChanged(SemanticFrameJoin),
    /// Run cancellation advanced exactly once from a current lineage join.
    Cancelled {
        /// Exact join current immediately before cancellation.
        prior: ContextJoin,
        /// Exact full-invalidation join returned by cancellation.
        current: ContextJoin,
    },
    /// Renderer loss advanced exactly once from a current lineage join.
    RendererLost {
        /// Exact join current immediately before renderer loss.
        prior: ContextJoin,
        /// Exact full-invalidation join returned by renderer loss.
        current: ContextJoin,
    },
    /// Explicit human-control takeover advanced exact context authority.
    HumanControlTaken {
        /// Exact join current immediately before takeover.
        prior: ContextJoin,
        /// Exact full-invalidation join returned by takeover.
        current: ContextJoin,
    },
}

impl fmt::Debug for SemanticSettleFact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tick => formatter.write_str("Tick"),
            Self::NavigationCommitted(context) => formatter
                .debug_tuple("NavigationCommitted")
                .field(context)
                .finish(),
            Self::DocumentReady(context) => formatter
                .debug_tuple("DocumentReady")
                .field(context)
                .finish(),
            Self::UrlChanged(context) => {
                formatter.debug_tuple("UrlChanged").field(context).finish()
            }
            Self::TitleChanged(context) => formatter
                .debug_tuple("TitleChanged")
                .field(context)
                .finish(),
            Self::Dialog { context, state } => formatter
                .debug_struct("Dialog")
                .field("context", context)
                .field("state", state)
                .finish(),
            Self::SemanticChange(frame) => formatter
                .debug_tuple("SemanticChange")
                .field(frame)
                .finish(),
            Self::Mutation(frame) => formatter.debug_tuple("Mutation").field(frame).finish(),
            Self::ScrollPositionChanged(frame) => formatter
                .debug_tuple("ScrollPositionChanged")
                .field(frame)
                .finish(),
            Self::Cancelled { prior, current } => formatter
                .debug_struct("Cancelled")
                .field("prior", prior)
                .field("current", current)
                .finish(),
            Self::RendererLost { prior, current } => formatter
                .debug_struct("RendererLost")
                .field("prior", prior)
                .field("current", current)
                .finish(),
            Self::HumanControlTaken { prior, current } => formatter
                .debug_struct("HumanControlTaken")
                .field("prior", prior)
                .field("current", current)
                .finish(),
        }
    }
}

/// One exact action-correlated settlement fact.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticSettleEvent {
    attempt: SemanticActionAttemptId,
    observed_at: SemanticSettleInstant,
    fact: SemanticSettleFact,
}

impl SemanticSettleEvent {
    /// Joins a closed trusted-shell fact to one attempt and monotonic instant.
    pub const fn new(
        attempt: SemanticActionAttemptId,
        observed_at: SemanticSettleInstant,
        fact: SemanticSettleFact,
    ) -> Self {
        Self {
            attempt,
            observed_at,
            fact,
        }
    }

    /// Exact action attempt identity.
    pub const fn attempt(&self) -> SemanticActionAttemptId {
        self.attempt
    }

    /// Monotonic fact time.
    pub const fn observed_at(&self) -> SemanticSettleInstant {
        self.observed_at
    }

    /// Closed settlement fact.
    pub const fn fact(&self) -> &SemanticSettleFact {
        &self.fact
    }
}

impl fmt::Debug for SemanticSettleEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticSettleEvent")
            .field("attempt", &self.attempt)
            .field("observed_at", &self.observed_at)
            .field("fact", &self.fact)
            .finish()
    }
}

/// Terminal failure taxonomy shared by settlement and later pipeline stages.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionFailure {
    /// Snapshot/frame/node authority is no longer current.
    #[error("semantic action reference is stale")]
    StaleReference,
    /// Stable target semantics or required operation changed.
    #[error("semantic action target changed")]
    TargetChanged,
    /// Target is currently disabled.
    #[error("semantic action target is disabled")]
    TargetDisabled,
    /// Action requires a separately authorized credential capability.
    #[error("semantic action crossed a credential boundary")]
    CredentialBoundary,
    /// Current hit testing or visibility found another surface over the target.
    #[error("semantic action target is occluded")]
    TargetOccluded,
    /// No admitted fixed backend supports this interaction.
    #[error("semantic action interaction is unsupported")]
    UnsupportedInteraction,
    /// Current origin/destination policy denied the action.
    #[error("semantic action origin is blocked")]
    BlockedOrigin,
    /// Run/plan/profile/account authority was absent, stale, or exhausted.
    #[error("semantic action lease is invalid")]
    LeaseViolation,
    /// Safe continuation requires explicit human control.
    #[error("semantic action requires a person")]
    NeedsHuman,
    /// A person took exclusive browser input while the action was settling.
    #[error("semantic action lost exclusive input to a person")]
    HumanControlChanged,
    /// A navigation other than the admitted transition replaced the document.
    #[error("semantic action navigation was replaced")]
    NavigationReplaced,
    /// Renderer loss invalidated current native and semantic state.
    #[error("semantic action renderer was lost")]
    RendererLost,
    /// The one absolute deadline expired.
    #[error("semantic action timed out")]
    Timeout,
    /// Independent post-settle evidence did not prove the requested effect.
    #[error("semantic action verification failed")]
    VerificationFailed,
    /// Exact run cancellation preempted the action.
    #[error("semantic action was cancelled")]
    Cancelled,
    /// A hard event/queue/payload/resource ceiling refused the action.
    #[error("semantic action exhausted a resource ceiling")]
    ResourceExhausted,
    /// The selected backend terminally refused its fixed request.
    #[error("semantic action backend refused the request")]
    BackendRefused,
}

impl From<SemanticActionRevalidationError> for SemanticActionFailure {
    fn from(value: SemanticActionRevalidationError) -> Self {
        match value {
            SemanticActionRevalidationError::StaleAuthority
            | SemanticActionRevalidationError::TargetMissing => Self::StaleReference,
            SemanticActionRevalidationError::TargetChanged
            | SemanticActionRevalidationError::OperationDenied
            | SemanticActionRevalidationError::SelectionTarget => Self::TargetChanged,
            SemanticActionRevalidationError::TargetDisabled => Self::TargetDisabled,
            SemanticActionRevalidationError::CredentialBoundary => Self::CredentialBoundary,
        }
    }
}

/// Non-authorizing recovery hint; automatic retry is never implied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticActionRecoveryHint {
    /// Stop this action/run branch unless a new explicit plan changes authority.
    Abort,
    /// A supervisor may decide only after a new complete observation.
    FreshObservationRequired,
    /// A separate explicit capability/policy transition is required.
    ExplicitCapabilityRequired,
    /// Transfer through the explicit human-control workflow.
    HumanRequired,
}

impl SemanticActionFailure {
    /// Returns the minimum safe recovery boundary without authorizing a retry.
    pub const fn recovery_hint(self) -> SemanticActionRecoveryHint {
        match self {
            Self::StaleReference
            | Self::TargetChanged
            | Self::TargetDisabled
            | Self::TargetOccluded
            | Self::NavigationReplaced
            | Self::RendererLost
            | Self::Timeout
            | Self::VerificationFailed
            | Self::BackendRefused => SemanticActionRecoveryHint::FreshObservationRequired,
            Self::CredentialBoundary => SemanticActionRecoveryHint::ExplicitCapabilityRequired,
            Self::UnsupportedInteraction | Self::NeedsHuman | Self::HumanControlChanged => {
                SemanticActionRecoveryHint::HumanRequired
            }
            Self::BlockedOrigin
            | Self::LeaseViolation
            | Self::Cancelled
            | Self::ResourceExhausted => SemanticActionRecoveryHint::Abort,
        }
    }
}

/// Current bounded settlement state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticSettleStatus {
    /// Awaiting a matching exact fact before the deadline.
    Pending,
    /// Declared condition reached; independent verification must run next.
    ReadyForVerification,
    /// Terminal typed failure.
    Failed(SemanticActionFailure),
}

impl SemanticSettleStatus {
    /// Reports whether no more facts may mutate this settlement.
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

/// Contract/refusal at the trusted-shell settlement boundary.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticSettleError {
    /// Relative action budget overflowed the supplied monotonic clock domain.
    #[error("semantic settle deadline overflowed")]
    DeadlineOverflow,
    /// Fact/snapshot belongs to another action attempt.
    #[error("semantic settle attempt identity mismatched")]
    AttemptMismatch,
    /// Fact/snapshot did not carry exact current frame/context lineage.
    #[error("semantic settle authority mismatched")]
    AuthorityMismatch,
    /// Trusted shell supplied a timestamp older than one already consumed.
    #[error("semantic settle clock regressed")]
    ClockRegression,
    /// Settlement was already terminal.
    #[error("semantic settle state is already terminal")]
    AlreadyTerminal,
    /// Target-state settlement requires one complete adjacent snapshot.
    #[error("semantic settle snapshot is incomplete")]
    IncompleteSnapshot,
}

/// O(1)-memory settlement core for one already-dispatched action attempt.
pub struct SemanticSettleTracker {
    attempt: SemanticActionAttemptId,
    frame: SemanticFrameJoin,
    target: SemanticNodeKey,
    action_guard: [u8; 32],
    source_snapshot: SemanticSnapshotGeneration,
    wait: SemanticWaitCondition,
    started_at: SemanticSettleInstant,
    deadline: SemanticSettleInstant,
    last_observed_at: SemanticSettleInstant,
    last_mutation_at: SemanticSettleInstant,
    terminal_at: Option<SemanticSettleInstant>,
    event_count: u16,
    status: SemanticSettleStatus,
}

impl SemanticSettleTracker {
    /// Starts settlement after a backend has terminally applied one fixed action.
    ///
    /// This constructor remains crate-private so production callers must use
    /// the consuming execution-outcome bridge. Internal verification tests use
    /// it directly to exercise the settlement core in isolation.
    pub(crate) fn begin(
        attempt: SemanticActionAttemptId,
        action: &SemanticPreparedAction,
        completed_at: SemanticSettleInstant,
    ) -> Result<Self, SemanticSettleError> {
        let deadline = completed_at
            .checked_add(action.settle_budget().millis())
            .ok_or(SemanticSettleError::DeadlineOverflow)?;
        let status = if action.wait() == SemanticWaitCondition::Immediate {
            SemanticSettleStatus::ReadyForVerification
        } else {
            SemanticSettleStatus::Pending
        };
        Ok(Self {
            attempt,
            frame: action.frame().clone(),
            target: action.target_key(),
            action_guard: action.verification_guard(),
            source_snapshot: action.checkpoint_snapshot(),
            wait: action.wait(),
            started_at: completed_at,
            deadline,
            last_observed_at: completed_at,
            last_mutation_at: completed_at,
            terminal_at: status.is_terminal().then_some(completed_at),
            event_count: 0,
            status,
        })
    }

    /// Exact attempt identity.
    pub const fn attempt(&self) -> SemanticActionAttemptId {
        self.attempt
    }

    /// One absolute deadline owned by the imperative timer layer.
    pub const fn deadline(&self) -> SemanticSettleInstant {
        self.deadline
    }

    /// Earliest exact monotonic wake the shell must schedule, if still pending.
    ///
    /// Mutation-quiet settlement wakes at the current quiet boundary and all
    /// other waits wake at their sole absolute deadline. The value advances
    /// after every admitted mutation. This method creates no timer and retains
    /// no timer handle; it lets the imperative shell keep one cancel/replace
    /// wake instead of polling.
    pub const fn next_wake(&self) -> Option<SemanticSettleInstant> {
        if self.status.is_terminal() {
            return None;
        }
        let candidate = match self.wait {
            SemanticWaitCondition::MutationQuiet(quiet) => {
                self.last_mutation_at.checked_add(quiet.millis())
            }
            SemanticWaitCondition::Immediate
            | SemanticWaitCondition::NavigationCommitted
            | SemanticWaitCondition::DocumentReady
            | SemanticWaitCondition::TargetState { .. }
            | SemanticWaitCondition::UrlChanged
            | SemanticWaitCondition::TitleChanged
            | SemanticWaitCondition::Dialog(_)
            | SemanticWaitCondition::SemanticChange
            | SemanticWaitCondition::ScrollPositionChanged => Some(self.deadline),
        };
        match candidate {
            Some(candidate) if candidate.millis() < self.deadline.millis() => Some(candidate),
            Some(_) | None => Some(self.deadline),
        }
    }

    /// Current settlement state.
    pub const fn status(&self) -> SemanticSettleStatus {
        self.status
    }

    /// Number of exact coalesced facts/snapshots consumed.
    pub const fn event_count(&self) -> u16 {
        self.event_count
    }

    /// Terminal elapsed milliseconds for bounded metrics.
    pub const fn elapsed_millis(&self) -> Option<u64> {
        match self.terminal_at {
            Some(terminal) => terminal.elapsed_since(self.started_at),
            None => None,
        }
    }

    /// Monotonic terminal instant, once settlement is ready or failed.
    pub const fn terminal_at(&self) -> Option<SemanticSettleInstant> {
        self.terminal_at
    }

    pub(crate) fn matches_action(&self, action: &SemanticPreparedAction) -> bool {
        self.frame == *action.frame()
            && self.target == action.target_key()
            && self.source_snapshot == action.checkpoint_snapshot()
            && self.action_guard == action.verification_guard()
    }

    /// Consumes one exact coalesced native/shell fact.
    pub fn observe(
        &mut self,
        event: SemanticSettleEvent,
    ) -> Result<SemanticSettleStatus, SemanticSettleError> {
        self.preflight(event.attempt, event.observed_at)?;
        if !self.fact_has_exact_authority(&event.fact) {
            return Err(SemanticSettleError::AuthorityMismatch);
        }
        if event.observed_at > self.deadline {
            return Ok(self.fail(SemanticActionFailure::Timeout, self.deadline));
        }
        if !self.admit_event(event.observed_at) {
            return Ok(self.status);
        }

        match &event.fact {
            SemanticSettleFact::Cancelled { .. } => {
                return Ok(self.fail(SemanticActionFailure::Cancelled, event.observed_at));
            }
            SemanticSettleFact::RendererLost { .. } => {
                return Ok(self.fail(SemanticActionFailure::RendererLost, event.observed_at));
            }
            SemanticSettleFact::HumanControlTaken { .. } => {
                return Ok(self.fail(
                    SemanticActionFailure::HumanControlChanged,
                    event.observed_at,
                ));
            }
            SemanticSettleFact::Mutation(_) => {
                self.last_mutation_at = event.observed_at;
            }
            _ => {}
        }

        if self.fact_matches(&event.fact) || self.quiet_period_elapsed(event.observed_at) {
            return Ok(self.ready(event.observed_at));
        }
        if event.observed_at >= self.deadline {
            return Ok(self.fail(SemanticActionFailure::Timeout, self.deadline));
        }
        Ok(self.status)
    }

    /// Observes a fresh exact action-frame snapshot for a target-state wait.
    ///
    /// The snapshot is a settle condition only. A later verifier must compare
    /// pre/post evidence and prove the requested effect independently.
    pub fn observe_snapshot(
        &mut self,
        attempt: SemanticActionAttemptId,
        observed_at: SemanticSettleInstant,
        snapshot: &SemanticSnapshot,
    ) -> Result<SemanticSettleStatus, SemanticSettleError> {
        self.preflight(attempt, observed_at)?;
        if snapshot.frame() != &self.frame {
            return Err(SemanticSettleError::AuthorityMismatch);
        }
        if snapshot.completeness() != SemanticCompleteness::Complete {
            return Err(SemanticSettleError::IncompleteSnapshot);
        }
        if observed_at > self.deadline {
            return Ok(self.fail(SemanticActionFailure::Timeout, self.deadline));
        }
        if !self.admit_event(observed_at) {
            return Ok(self.status);
        }
        if self.source_snapshot.next() != Some(snapshot.generation()) {
            return Ok(self.fail(SemanticActionFailure::StaleReference, observed_at));
        }
        let Some(target) = snapshot
            .nodes()
            .iter()
            .find(|node| node.key() == self.target)
        else {
            return Ok(self.fail(SemanticActionFailure::StaleReference, observed_at));
        };
        if let SemanticWaitCondition::TargetState { state, present } = self.wait {
            if target.states().contains(state) == present {
                return Ok(self.ready(observed_at));
            }
        }
        if self.quiet_period_elapsed(observed_at) {
            return Ok(self.ready(observed_at));
        }
        if observed_at >= self.deadline {
            return Ok(self.fail(SemanticActionFailure::Timeout, self.deadline));
        }
        Ok(self.status)
    }

    fn preflight(
        &self,
        attempt: SemanticActionAttemptId,
        observed_at: SemanticSettleInstant,
    ) -> Result<(), SemanticSettleError> {
        if self.status.is_terminal() {
            return Err(SemanticSettleError::AlreadyTerminal);
        }
        if attempt != self.attempt {
            return Err(SemanticSettleError::AttemptMismatch);
        }
        if observed_at < self.last_observed_at {
            return Err(SemanticSettleError::ClockRegression);
        }
        Ok(())
    }

    fn admit_event(&mut self, observed_at: SemanticSettleInstant) -> bool {
        self.last_observed_at = observed_at;
        if self.event_count >= MAX_SEMANTIC_SETTLE_EVENTS {
            self.fail(SemanticActionFailure::ResourceExhausted, observed_at);
            return false;
        }
        self.event_count += 1;
        true
    }

    fn fact_has_exact_authority(&self, fact: &SemanticSettleFact) -> bool {
        let context = self.frame.context();
        match fact {
            SemanticSettleFact::Tick => true,
            SemanticSettleFact::NavigationCommitted(current) => {
                exact_document_successor(context, *current)
            }
            SemanticSettleFact::DocumentReady(current)
            | SemanticSettleFact::UrlChanged(current)
            | SemanticSettleFact::TitleChanged(current) => {
                same_or_document_successor(context, *current)
            }
            SemanticSettleFact::Dialog {
                context: current, ..
            } => *current == context,
            SemanticSettleFact::SemanticChange(frame)
            | SemanticSettleFact::Mutation(frame)
            | SemanticSettleFact::ScrollPositionChanged(frame) => frame == &self.frame,
            SemanticSettleFact::Cancelled { prior, current }
            | SemanticSettleFact::RendererLost { prior, current }
            | SemanticSettleFact::HumanControlTaken { prior, current } => {
                same_or_document_successor(context, *prior)
                    && exact_full_invalidation_successor(*prior, *current)
            }
        }
    }

    fn fact_matches(&self, fact: &SemanticSettleFact) -> bool {
        match (self.wait, fact) {
            (
                SemanticWaitCondition::NavigationCommitted,
                SemanticSettleFact::NavigationCommitted(_),
            )
            | (SemanticWaitCondition::DocumentReady, SemanticSettleFact::DocumentReady(_))
            | (SemanticWaitCondition::UrlChanged, SemanticSettleFact::UrlChanged(_))
            | (SemanticWaitCondition::TitleChanged, SemanticSettleFact::TitleChanged(_))
            | (SemanticWaitCondition::SemanticChange, SemanticSettleFact::SemanticChange(_))
            | (
                SemanticWaitCondition::ScrollPositionChanged,
                SemanticSettleFact::ScrollPositionChanged(_),
            ) => true,
            (SemanticWaitCondition::Dialog(expected), SemanticSettleFact::Dialog { state, .. }) => {
                expected == *state
            }
            _ => false,
        }
    }

    fn quiet_period_elapsed(&self, observed_at: SemanticSettleInstant) -> bool {
        let SemanticWaitCondition::MutationQuiet(quiet) = self.wait else {
            return false;
        };
        observed_at
            .elapsed_since(self.last_mutation_at)
            .is_some_and(|elapsed| elapsed >= u64::from(quiet.millis()))
    }

    fn ready(&mut self, observed_at: SemanticSettleInstant) -> SemanticSettleStatus {
        self.status = SemanticSettleStatus::ReadyForVerification;
        self.terminal_at = Some(observed_at);
        self.status
    }

    fn fail(
        &mut self,
        failure: SemanticActionFailure,
        observed_at: SemanticSettleInstant,
    ) -> SemanticSettleStatus {
        self.status = SemanticSettleStatus::Failed(failure);
        self.terminal_at = Some(observed_at);
        self.status
    }
}

impl fmt::Debug for SemanticSettleTracker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticSettleTracker")
            .field("attempt", &self.attempt)
            .field("frame", &self.frame)
            .field("target", &"[redacted]")
            .field("action_guard", &"[redacted]")
            .field("source_snapshot", &self.source_snapshot)
            .field("wait", &self.wait)
            .field("deadline", &self.deadline)
            .field("event_count", &self.event_count)
            .field("status", &self.status)
            .finish()
    }
}

fn same_or_document_successor(previous: ContextJoin, current: ContextJoin) -> bool {
    previous == current || exact_document_successor(previous, current)
}

pub(crate) fn exact_document_successor(previous: ContextJoin, current: ContextJoin) -> bool {
    current.identity() == previous.identity()
        && current.context_generation() == previous.context_generation()
        && current.cancellation_generation() == previous.cancellation_generation()
        && previous.navigation_epoch().next() == Some(current.navigation_epoch())
        && current.frame() == FrameId::MAIN
        && previous.frame_generation().next() == Some(current.frame_generation())
}

fn exact_full_invalidation_successor(previous: ContextJoin, current: ContextJoin) -> bool {
    current.identity() == previous.identity()
        && previous.context_generation().next() == Some(current.context_generation())
        && previous.navigation_epoch().next() == Some(current.navigation_epoch())
        && previous.frame_generation().next() == Some(current.frame_generation())
        && previous.cancellation_generation().next() == Some(current.cancellation_generation())
        && current.frame() == FrameId::MAIN
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, SemanticActionBatch, SemanticActionBatchId,
        SemanticActionIntent, SemanticActionProposal, SemanticDecodeContext, SemanticEffectClass,
        SemanticFrameTrust, SemanticInvocationId, SemanticMutationQuietPeriod, SemanticObservation,
        SemanticObservationAssembler, SemanticObservationBudget, SemanticObservationId,
        SemanticOrigin, SemanticReferenceId, SemanticSettleBudget, SemanticState,
        SemanticVerification, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn observation() -> (SemanticObservation, ContextRegistry) {
        let identity = ContextIdentity::new(
            ContextId::from_raw(41),
            ContextRunId::from_raw(42),
            ProfileId::from(43),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[
                ContextCapability::Observe,
                ContextCapability::Act,
                ContextCapability::Navigate,
                ContextCapability::HumanControl,
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
            .expect("construct");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settle");
        let context = registry.join(identity.id()).expect("join");
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://settle-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 1,
            "g": 1,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Sensitive button label", "o": 9}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(1).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(1).expect("generation"),
            ),
            &bytes,
        )
        .expect("snapshot");
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

    fn action(
        observation: &SemanticObservation,
        wait: SemanticWaitCondition,
        verification: SemanticVerification,
        budget_millis: u32,
    ) -> SemanticPreparedAction {
        let proposal = SemanticActionProposal::try_new(
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(2).expect("button"),
            },
            SemanticEffectClass::Read,
            wait,
            verification,
            SemanticSettleBudget::try_new(budget_millis).expect("budget"),
        )
        .expect("proposal");
        let batch = SemanticActionBatch::bind(
            SemanticActionBatchId::new(1).expect("batch"),
            observation,
            &[observation.frames()[0].frame().clone()],
            vec![proposal],
        )
        .expect("batch");
        batch.actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare")
    }

    fn event(
        attempt: SemanticActionAttemptId,
        millis: u64,
        fact: SemanticSettleFact,
    ) -> SemanticSettleEvent {
        SemanticSettleEvent::new(attempt, SemanticSettleInstant::from_millis(millis), fact)
    }

    fn snapshot(
        observation: &SemanticObservation,
        generation: u64,
        button_states: u8,
        completeness: &str,
    ) -> SemanticSnapshot {
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": generation + 10,
            "g": generation,
            "c": completeness,
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Sensitive button label", "s": button_states, "o": 9}
            ]
        }))
        .expect("wire");
        decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(generation + 10).expect("invocation"),
                observation.frames()[0].frame().clone(),
                SemanticSnapshotGeneration::new(generation).expect("generation"),
            ),
            &bytes,
        )
        .expect("snapshot")
    }

    #[test]
    fn immediate_settlement_is_ready_without_allocating_or_consuming_events() {
        let (observation, _) = observation();
        let action = action(
            &observation,
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            250,
        );
        let tracker = SemanticSettleTracker::begin(
            SemanticActionAttemptId::new(1).expect("attempt"),
            &action,
            SemanticSettleInstant::from_millis(100),
        )
        .expect("tracker");
        assert_eq!(tracker.status(), SemanticSettleStatus::ReadyForVerification);
        assert_eq!(tracker.event_count(), 0);
        assert_eq!(tracker.elapsed_millis(), Some(0));
        assert_eq!(tracker.deadline().millis(), 350);
        assert_eq!(tracker.next_wake(), None);
    }

    #[test]
    fn mutation_quiet_restarts_and_completes_only_under_the_absolute_deadline() {
        let (observation, _) = observation();
        let action = action(
            &observation,
            SemanticWaitCondition::MutationQuiet(
                SemanticMutationQuietPeriod::try_new(100).expect("quiet"),
            ),
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            500,
        );
        let attempt = SemanticActionAttemptId::new(2).expect("attempt");
        let mut tracker = SemanticSettleTracker::begin(
            attempt,
            &action,
            SemanticSettleInstant::from_millis(1_000),
        )
        .expect("tracker");
        assert_eq!(
            tracker.next_wake().map(SemanticSettleInstant::millis),
            Some(1_100)
        );
        let frame = action.frame().clone();
        assert_eq!(
            tracker
                .observe(event(attempt, 1_050, SemanticSettleFact::Mutation(frame)))
                .expect("mutation"),
            SemanticSettleStatus::Pending
        );
        assert_eq!(
            tracker.next_wake().map(SemanticSettleInstant::millis),
            Some(1_150)
        );
        assert_eq!(
            tracker
                .observe(event(attempt, 1_149, SemanticSettleFact::Tick))
                .expect("tick"),
            SemanticSettleStatus::Pending
        );
        assert_eq!(
            tracker.next_wake().map(SemanticSettleInstant::millis),
            Some(1_150)
        );
        assert_eq!(
            tracker
                .observe(event(attempt, 1_150, SemanticSettleFact::Tick))
                .expect("tick"),
            SemanticSettleStatus::ReadyForVerification
        );
        assert_eq!(tracker.next_wake(), None);
        assert_eq!(tracker.elapsed_millis(), Some(150));
    }

    #[test]
    fn target_state_uses_private_stable_identity_in_an_exact_fresh_snapshot() {
        let (observation, _) = observation();
        let action = action(
            &observation,
            SemanticWaitCondition::TargetState {
                state: SemanticState::Expanded,
                present: true,
            },
            SemanticVerification::TargetState {
                state: SemanticState::Expanded,
                present: true,
            },
            250,
        );
        let attempt = SemanticActionAttemptId::new(3).expect("attempt");
        let stale_attempt = SemanticActionAttemptId::new(30).expect("attempt");
        let mut stale_tracker = SemanticSettleTracker::begin(
            stale_attempt,
            &action,
            SemanticSettleInstant::from_millis(50),
        )
        .expect("tracker");
        assert_eq!(
            stale_tracker
                .observe_snapshot(
                    stale_attempt,
                    SemanticSettleInstant::from_millis(60),
                    &observation.frames()[0],
                )
                .expect("snapshot"),
            SemanticSettleStatus::Failed(SemanticActionFailure::StaleReference)
        );
        let mut incomplete_tracker =
            SemanticSettleTracker::begin(attempt, &action, SemanticSettleInstant::from_millis(50))
                .expect("tracker");
        let incomplete = snapshot(&observation, 2, 4, "node_limit");
        assert_eq!(
            incomplete_tracker.observe_snapshot(
                attempt,
                SemanticSettleInstant::from_millis(70),
                &incomplete,
            ),
            Err(SemanticSettleError::IncompleteSnapshot)
        );
        assert_eq!(incomplete_tracker.event_count(), 0);
        let mut tracker =
            SemanticSettleTracker::begin(attempt, &action, SemanticSettleInstant::from_millis(50))
                .expect("tracker");
        let current = snapshot(&observation, 2, 4, "complete");
        assert_eq!(
            tracker
                .observe_snapshot(attempt, SemanticSettleInstant::from_millis(75), &current,)
                .expect("snapshot"),
            SemanticSettleStatus::ReadyForVerification
        );
        assert_eq!(tracker.event_count(), 1);
    }

    #[test]
    fn navigation_requires_the_exact_next_context_document_join() {
        let (observation, mut registry) = observation();
        let action = action(
            &observation,
            SemanticWaitCondition::NavigationCommitted,
            SemanticVerification::NavigationCommitted,
            250,
        );
        let attempt = SemanticActionAttemptId::new(4).expect("attempt");
        let mut tracker =
            SemanticSettleTracker::begin(attempt, &action, SemanticSettleInstant::from_millis(10))
                .expect("tracker");
        assert_eq!(
            tracker.observe(event(
                attempt,
                11,
                SemanticSettleFact::NavigationCommitted(action.frame().context()),
            )),
            Err(SemanticSettleError::AuthorityMismatch)
        );
        assert_eq!(tracker.event_count(), 0);

        let navigation = registry
            .begin_navigation(
                observation.request().context().identity().id(),
                ContextOperationId::new(2).expect("operation"),
            )
            .expect("navigation");
        assert_eq!(
            tracker
                .observe(event(
                    attempt,
                    12,
                    SemanticSettleFact::NavigationCommitted(navigation.context()),
                ))
                .expect("event"),
            SemanticSettleStatus::ReadyForVerification
        );
    }

    #[test]
    fn timeout_clock_attempt_and_terminality_are_exact() {
        let (observation, _) = observation();
        let action = action(
            &observation,
            SemanticWaitCondition::SemanticChange,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            100,
        );
        let attempt = SemanticActionAttemptId::new(5).expect("attempt");
        let mut tracker =
            SemanticSettleTracker::begin(attempt, &action, SemanticSettleInstant::from_millis(500))
                .expect("tracker");
        assert_eq!(
            tracker.next_wake().map(SemanticSettleInstant::millis),
            Some(600)
        );
        assert_eq!(
            tracker.observe(event(
                SemanticActionAttemptId::new(6).expect("other"),
                510,
                SemanticSettleFact::Tick,
            )),
            Err(SemanticSettleError::AttemptMismatch)
        );
        assert_eq!(
            tracker
                .observe(event(attempt, 600, SemanticSettleFact::Tick))
                .expect("deadline"),
            SemanticSettleStatus::Failed(SemanticActionFailure::Timeout)
        );
        assert_eq!(tracker.next_wake(), None);
        assert_eq!(tracker.elapsed_millis(), Some(100));
        assert_eq!(
            tracker.observe(event(attempt, 601, SemanticSettleFact::Tick)),
            Err(SemanticSettleError::AlreadyTerminal)
        );
    }

    #[test]
    fn cancellation_requires_the_exact_full_invalidation_join() {
        let (observation, mut registry) = observation();
        let action = action(
            &observation,
            SemanticWaitCondition::SemanticChange,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            250,
        );
        let attempt = SemanticActionAttemptId::new(6).expect("attempt");
        let mut tracker =
            SemanticSettleTracker::begin(attempt, &action, SemanticSettleInstant::from_millis(10))
                .expect("tracker");
        assert_eq!(
            tracker.observe(event(
                attempt,
                11,
                SemanticSettleFact::Cancelled {
                    prior: action.frame().context(),
                    current: action.frame().context(),
                },
            )),
            Err(SemanticSettleError::AuthorityMismatch)
        );
        let cancelled = registry
            .cancel_run(
                observation.request().context().identity().id(),
                observation.request().context(),
            )
            .expect("cancel");
        assert_eq!(
            tracker
                .observe(event(
                    attempt,
                    12,
                    SemanticSettleFact::Cancelled {
                        prior: action.frame().context(),
                        current: cancelled,
                    },
                ))
                .expect("cancel event"),
            SemanticSettleStatus::Failed(SemanticActionFailure::Cancelled)
        );
        assert_eq!(tracker.elapsed_millis(), Some(2));
    }

    #[test]
    fn explicit_human_takeover_preempts_settlement_with_exact_authority() {
        let (observation, mut registry) = observation();
        let action = action(
            &observation,
            SemanticWaitCondition::SemanticChange,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            250,
        );
        let attempt = SemanticActionAttemptId::new(9).expect("attempt");
        let mut tracker =
            SemanticSettleTracker::begin(attempt, &action, SemanticSettleInstant::from_millis(10))
                .expect("tracker");
        let takeover = registry
            .begin_human_control(
                observation.request().context().identity().id(),
                ContextOperationId::new(3).expect("operation"),
            )
            .expect("takeover");
        assert_eq!(
            tracker
                .observe(event(
                    attempt,
                    11,
                    SemanticSettleFact::HumanControlTaken {
                        prior: action.frame().context(),
                        current: takeover.context(),
                    },
                ))
                .expect("takeover event"),
            SemanticSettleStatus::Failed(SemanticActionFailure::HumanControlChanged)
        );
        assert_eq!(
            SemanticActionFailure::HumanControlChanged.recovery_hint(),
            SemanticActionRecoveryHint::HumanRequired
        );
    }

    #[test]
    fn event_ceiling_failure_mapping_and_debug_are_closed() {
        let (observation, _) = observation();
        let action = action(
            &observation,
            SemanticWaitCondition::SemanticChange,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            30_000,
        );
        let attempt = SemanticActionAttemptId::new(7).expect("attempt");
        let mut tracker =
            SemanticSettleTracker::begin(attempt, &action, SemanticSettleInstant::from_millis(0))
                .expect("tracker");
        for index in 0..MAX_SEMANTIC_SETTLE_EVENTS {
            assert_eq!(
                tracker
                    .observe(event(
                        attempt,
                        u64::from(index) + 1,
                        SemanticSettleFact::Tick,
                    ))
                    .expect("tick"),
                SemanticSettleStatus::Pending
            );
        }
        assert_eq!(
            tracker
                .observe(event(
                    attempt,
                    u64::from(MAX_SEMANTIC_SETTLE_EVENTS) + 1,
                    SemanticSettleFact::Tick,
                ))
                .expect("ceiling"),
            SemanticSettleStatus::Failed(SemanticActionFailure::ResourceExhausted)
        );
        assert_eq!(
            SemanticActionFailure::from(SemanticActionRevalidationError::CredentialBoundary)
                .recovery_hint(),
            SemanticActionRecoveryHint::ExplicitCapabilityRequired
        );
        assert_eq!(
            SemanticActionFailure::Cancelled.recovery_hint(),
            SemanticActionRecoveryHint::Abort
        );
        let debug = format!("{tracker:?}");
        assert!(!debug.contains("settle-private"));
        assert!(!debug.contains("Sensitive button label"));
        assert!(!debug.contains("1000"));
        assert!(debug.contains("ResourceExhausted"));
    }

    #[test]
    fn deadline_overflow_is_refused_before_tracking_starts() {
        let (observation, _) = observation();
        let action = action(
            &observation,
            SemanticWaitCondition::SemanticChange,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            250,
        );
        assert!(matches!(
            SemanticSettleTracker::begin(
                SemanticActionAttemptId::new(8).expect("attempt"),
                &action,
                SemanticSettleInstant::from_millis(u64::MAX),
            ),
            Err(SemanticSettleError::DeadlineOverflow)
        ));
        assert_eq!(
            SemanticActionFailure::TargetChanged.recovery_hint(),
            SemanticActionRecoveryHint::FreshObservationRequired
        );
    }
}
