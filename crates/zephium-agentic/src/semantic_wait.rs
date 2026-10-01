//! Proof-carrying standalone waits over fresh semantic observations.
//!
//! A wait never treats elapsed time as evidence. The model selects one closed
//! condition, the runtime binds it to the exact delivered baseline, and only
//! independently captured consecutive observations can satisfy it. A deadline
//! merely produces a typed timeout result containing the latest fresh state.

use std::fmt;

use thiserror::Error;

use crate::semantic::SemanticNodeKey;
use crate::{
    compute_semantic_diff, AgentBrowserWaitCondition, FrameId, SemanticCompleteness,
    SemanticDiffBudget, SemanticDiffOutcome, SemanticObservation,
    SemanticObservationAcknowledgement, SemanticReferenceError, SemanticRole, SemanticScope,
    SemanticState,
};

/// First delay before a standalone wait samples fresh semantic state.
pub const SEMANTIC_STANDALONE_WAIT_INITIAL_POLL_MILLIS: u32 = 50;
/// Maximum delay between standalone-wait semantic samples.
pub const SEMANTIC_STANDALONE_WAIT_MAX_POLL_MILLIS: u32 = 1_000;

/// Content-free bounded polling schedule for one standalone wait.
///
/// It owns no asynchronous timer. The controller creates and drops each
/// individual sleep around a cancellation-prioritized select, so closure
/// cannot retain timer debt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticStandaloneWaitBackoff {
    next_millis: u32,
    samples: u16,
}

impl SemanticStandaloneWaitBackoff {
    /// Starts at the fixed low-latency initial sampling interval.
    pub const fn new() -> Self {
        Self {
            next_millis: SEMANTIC_STANDALONE_WAIT_INITIAL_POLL_MILLIS,
            samples: 0,
        }
    }

    /// Returns this sample's delay and advances exponentially to the fixed cap.
    pub fn next_delay_millis(&mut self) -> u32 {
        let delay = self.next_millis;
        self.next_millis = self
            .next_millis
            .saturating_mul(2)
            .min(SEMANTIC_STANDALONE_WAIT_MAX_POLL_MILLIS);
        self.samples = self.samples.saturating_add(1);
        delay
    }

    /// Number of delays issued by this bounded schedule.
    pub const fn samples(self) -> u16 {
        self.samples
    }
}

impl Default for SemanticStandaloneWaitBackoff {
    fn default() -> Self {
        Self::new()
    }
}

/// Terminal result of one bounded standalone wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticStandaloneWaitOutcome {
    /// A fresh observation independently satisfied the bound condition.
    Satisfied,
    /// The host's absolute deadline elapsed without observed satisfaction.
    TimedOut,
}

/// A terminal wait proof and its latest independently captured observation.
#[must_use]
pub struct SemanticStandaloneWaitResult {
    baseline: SemanticObservationAcknowledgement,
    observation: SemanticObservation,
    outcome: SemanticStandaloneWaitOutcome,
}

impl SemanticStandaloneWaitResult {
    /// Exact model-delivered baseline against which the wait was admitted.
    pub const fn baseline(&self) -> &SemanticObservationAcknowledgement {
        &self.baseline
    }

    /// Latest fresh observation captured before the terminal outcome.
    pub const fn observation(&self) -> &SemanticObservation {
        &self.observation
    }

    /// Independently derived terminal state.
    pub const fn outcome(&self) -> SemanticStandaloneWaitOutcome {
        self.outcome
    }

    /// Moves out the latest observation after provider continuation is bound.
    pub fn into_observation(self) -> SemanticObservation {
        self.observation
    }

    /// Conservatively downgrades a satisfaction sampled at or after the host's
    /// absolute deadline. This can never manufacture successful evidence.
    pub fn into_timed_out(mut self) -> Self {
        self.outcome = SemanticStandaloneWaitOutcome::TimedOut;
        self
    }
}

impl fmt::Debug for SemanticStandaloneWaitResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticStandaloneWaitResult")
            .field("baseline", &self.baseline)
            .field("observation", self.observation.request())
            .field("outcome", &self.outcome)
            .finish()
    }
}

#[derive(Clone, Copy)]
enum BoundWaitCondition {
    SemanticChange,
    TargetState {
        frame: FrameId,
        key: SemanticNodeKey,
        role: SemanticRole,
        state: SemanticState,
        present: bool,
    },
}

/// Live bounded wait state. It owns exactly one latest observation and no timer.
#[must_use]
pub struct SemanticStandaloneWait {
    baseline: SemanticObservationAcknowledgement,
    latest: SemanticObservation,
    condition: BoundWaitCondition,
}

/// Result of supplying one fresh consecutive observation to a wait.
#[must_use]
pub enum SemanticStandaloneWaitStep {
    /// The condition remains false; this value owns the new exact checkpoint.
    Pending(SemanticStandaloneWait),
    /// The supplied fresh observation satisfied the bound condition.
    Satisfied(SemanticStandaloneWaitResult),
}

/// Refusal to bind or advance a standalone wait.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticStandaloneWaitError {
    /// The condition is not part of the deliberately narrow production surface.
    #[error("standalone wait condition is unsupported")]
    UnsupportedCondition,
    /// The supplied baseline was not the exact model-delivered observation.
    #[error("standalone wait baseline is not acknowledged")]
    Baseline,
    /// The target reference was stale, absent, or ambiguous.
    #[error("standalone wait target is invalid")]
    Target,
    /// An observation was incomplete or not the unique consecutive successor.
    #[error("standalone wait observation is not complete and consecutive")]
    Observation,
}

impl SemanticStandaloneWait {
    /// Binds a model proposal to one exact acknowledged initial observation.
    pub fn prepare(
        condition: AgentBrowserWaitCondition,
        observation: SemanticObservation,
        acknowledgement: &SemanticObservationAcknowledgement,
    ) -> Result<Self, SemanticStandaloneWaitError> {
        if !acknowledgement.matches(&observation)
            || !matches!(observation.request().scope(), SemanticScope::Initial)
            || !observation_complete(&observation)
        {
            return Err(SemanticStandaloneWaitError::Baseline);
        }
        let condition = match condition {
            AgentBrowserWaitCondition::SemanticChange => BoundWaitCondition::SemanticChange,
            AgentBrowserWaitCondition::TargetState {
                target,
                state,
                present,
            } => {
                let frame = observation
                    .reference_frame(target)
                    .map_err(map_reference_error)?;
                let node = observation
                    .resolve_node(target, frame)
                    .map_err(map_reference_error)?;
                BoundWaitCondition::TargetState {
                    frame: frame.frame(),
                    key: node.key(),
                    role: node.role(),
                    state,
                    present,
                }
            }
            _ => return Err(SemanticStandaloneWaitError::UnsupportedCondition),
        };
        Ok(Self {
            baseline: acknowledgement.clone(),
            latest: observation,
            condition,
        })
    }

    /// Evaluates one fresh consecutive observation. No clock or timer can call
    /// this condition satisfied without semantic evidence.
    pub fn advance(
        self,
        current: SemanticObservation,
    ) -> Result<SemanticStandaloneWaitStep, SemanticStandaloneWaitError> {
        if !matches!(current.request().scope(), SemanticScope::Initial)
            || !observation_complete(&current)
        {
            return Err(SemanticStandaloneWaitError::Observation);
        }
        let local = SemanticObservationAcknowledgement::from_fingerprint(
            crate::semantic_diff::SemanticObservationFingerprint::from_observation(&self.latest),
        );
        let diff = match compute_semantic_diff(
            &self.latest,
            &local,
            &current,
            SemanticDiffBudget::try_new(crate::MAX_SEMANTIC_DIFF_ENTRIES)
                .map_err(|_| SemanticStandaloneWaitError::Observation)?,
        ) {
            SemanticDiffOutcome::Diff(diff) => diff,
            SemanticDiffOutcome::FreshSnapshot(_) => {
                return Err(SemanticStandaloneWaitError::Observation)
            }
        };
        let satisfied = match self.condition {
            BoundWaitCondition::SemanticChange => !diff.entries().is_empty(),
            BoundWaitCondition::TargetState {
                frame,
                key,
                role,
                state,
                present,
            } => current
                .frames()
                .iter()
                .find(|snapshot| snapshot.frame().frame() == frame)
                .and_then(|snapshot| {
                    snapshot
                        .nodes()
                        .iter()
                        .find(|node| node.key() == key && node.role() == role)
                        .map(|node| node.states().contains(state) == present)
                })
                .unwrap_or(false),
        };
        if satisfied {
            Ok(SemanticStandaloneWaitStep::Satisfied(
                SemanticStandaloneWaitResult {
                    baseline: self.baseline,
                    observation: current,
                    outcome: SemanticStandaloneWaitOutcome::Satisfied,
                },
            ))
        } else {
            Ok(SemanticStandaloneWaitStep::Pending(Self {
                baseline: self.baseline,
                latest: current,
                condition: self.condition,
            }))
        }
    }

    /// Closes the wait at a trusted host deadline. Timeout is explicitly not a
    /// successful condition and carries only the latest fresh observation.
    pub fn time_out(self) -> SemanticStandaloneWaitResult {
        SemanticStandaloneWaitResult {
            baseline: self.baseline,
            observation: self.latest,
            outcome: SemanticStandaloneWaitOutcome::TimedOut,
        }
    }
}

fn observation_complete(observation: &SemanticObservation) -> bool {
    observation
        .frames()
        .iter()
        .all(|frame| frame.completeness() == SemanticCompleteness::Complete)
}

fn map_reference_error(_: SemanticReferenceError) -> SemanticStandaloneWaitError {
    SemanticStandaloneWaitError::Target
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, SemanticDecodeContext, SemanticFrameJoin,
        SemanticFrameTrust, SemanticInvocationId, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticObservationId, SemanticObservationRequest,
        SemanticOrigin, SemanticReferenceId, SemanticSnapshotGeneration, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn context() -> crate::ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(1),
            ContextRunId::from_raw(2),
            ProfileId::from(3),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[ContextCapability::Observe, ContextCapability::Act],
        )
        .unwrap();
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).unwrap();
        let operation = registry
            .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
            .unwrap();
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .unwrap();
        registry.join(identity.id()).unwrap()
    }

    fn observation(
        context: crate::ContextJoin,
        observation: u64,
        invocation: u64,
        generation: u64,
        checked: bool,
        label: &str,
    ) -> SemanticObservation {
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(observation).unwrap(),
            context,
            SemanticObservationBudget::try_new(16, 4096, 1).unwrap(),
        );
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://wait.example.test/").unwrap(),
            SemanticFrameTrust::SameOrigin,
        )
        .unwrap();
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": generation,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document"},
                {"k": 2, "p": 0, "r": "checkbox", "n": label, "s": u8::from(checked), "o": 1}
            ]
        }))
        .unwrap();
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(invocation).unwrap(),
                frame,
                SemanticSnapshotGeneration::new(generation).unwrap(),
            ),
            &wire,
        )
        .unwrap();
        SemanticObservationAssembler::new(request, snapshot)
            .unwrap()
            .finish()
            .unwrap()
    }

    fn acknowledge(observation: &SemanticObservation) -> SemanticObservationAcknowledgement {
        SemanticObservationAcknowledgement::from_fingerprint(
            crate::semantic_diff::SemanticObservationFingerprint::from_observation(observation),
        )
    }

    #[test]
    fn target_state_requires_fresh_consecutive_evidence() {
        let context = context();
        let baseline = observation(context, 1, 11, 21, false, "Private toggle");
        let acknowledgement = acknowledge(&baseline);
        let wait = SemanticStandaloneWait::prepare(
            AgentBrowserWaitCondition::TargetState {
                target: SemanticReferenceId::new(2).unwrap(),
                state: SemanticState::Checked,
                present: true,
            },
            baseline,
            &acknowledgement,
        )
        .unwrap();
        let unchanged = observation(context, 2, 12, 22, false, "Private toggle");
        let SemanticStandaloneWaitStep::Pending(wait) = wait.advance(unchanged).unwrap() else {
            panic!("unchanged state must remain pending");
        };
        let checked = observation(context, 3, 13, 23, true, "Private toggle");
        let SemanticStandaloneWaitStep::Satisfied(result) = wait.advance(checked).unwrap() else {
            panic!("checked state must satisfy the wait");
        };
        assert_eq!(result.outcome(), SemanticStandaloneWaitOutcome::Satisfied);
        assert_eq!(result.baseline(), &acknowledgement);
    }

    #[test]
    fn semantic_change_and_timeout_are_distinct_terminal_outcomes() {
        let context = context();
        let baseline = observation(context, 1, 11, 21, false, "Private before");
        let acknowledgement = acknowledge(&baseline);
        let wait = SemanticStandaloneWait::prepare(
            AgentBrowserWaitCondition::SemanticChange,
            baseline,
            &acknowledgement,
        )
        .unwrap();
        let unchanged = observation(context, 2, 12, 22, false, "Private before");
        let SemanticStandaloneWaitStep::Pending(wait) = wait.advance(unchanged).unwrap() else {
            panic!("unchanged semantics must remain pending");
        };
        assert_eq!(
            wait.time_out().outcome(),
            SemanticStandaloneWaitOutcome::TimedOut
        );

        let baseline = observation(context, 10, 31, 41, false, "Private before");
        let acknowledgement = acknowledge(&baseline);
        let wait = SemanticStandaloneWait::prepare(
            AgentBrowserWaitCondition::SemanticChange,
            baseline,
            &acknowledgement,
        )
        .unwrap();
        let changed = observation(context, 11, 32, 42, false, "Private after");
        assert!(matches!(
            wait.advance(changed).unwrap(),
            SemanticStandaloneWaitStep::Satisfied(_)
        ));
    }

    #[test]
    fn timer_only_and_nonconsecutive_waits_fail_closed() {
        let context = context();
        let baseline = observation(context, 1, 11, 21, false, "Private toggle");
        let acknowledgement = acknowledge(&baseline);
        assert!(matches!(
            SemanticStandaloneWait::prepare(
                AgentBrowserWaitCondition::Immediate,
                baseline.clone(),
                &acknowledgement,
            ),
            Err(SemanticStandaloneWaitError::UnsupportedCondition)
        ));
        let wait = SemanticStandaloneWait::prepare(
            AgentBrowserWaitCondition::SemanticChange,
            baseline,
            &acknowledgement,
        )
        .unwrap();
        let skipped = observation(context, 2, 12, 23, false, "Private toggle");
        assert!(matches!(
            wait.advance(skipped),
            Err(SemanticStandaloneWaitError::Observation)
        ));
    }

    #[test]
    fn polling_backoff_bounds_full_captures_over_maximum_wait() {
        let mut backoff = SemanticStandaloneWaitBackoff::new();
        assert_eq!(
            (0..6)
                .map(|_| backoff.next_delay_millis())
                .collect::<Vec<_>>(),
            vec![50, 100, 200, 400, 800, 1_000]
        );
        let mut backoff = SemanticStandaloneWaitBackoff::new();
        let mut elapsed = 0_u32;
        let mut captures = 0_u16;
        loop {
            let delay = backoff.next_delay_millis();
            let Some(next) = elapsed.checked_add(delay) else {
                panic!("bounded schedule overflowed");
            };
            if next > crate::MAX_SEMANTIC_ACTION_SETTLE_MILLIS {
                break;
            }
            elapsed = next;
            captures += 1;
        }
        assert_eq!(captures, 33);
        assert_eq!(backoff.samples(), 34);
        assert!(elapsed <= crate::MAX_SEMANTIC_ACTION_SETTLE_MILLIS);
    }
}
