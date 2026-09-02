//! Bounded single-owner routing for semantic action settlement.
//!
//! The coordinator retains applied policy authority while an action waits for
//! exact shell facts. It allocates nothing while unused, admits at most one
//! settlement per logical context and four process-wide, and exposes one
//! move-only replaceable wake reservation. It owns no timer, callback, worker,
//! native object, snapshot, or retry policy.

use std::fmt;

use thiserror::Error;

use crate::semantic_execute::SemanticActionCoordinatorKey;
use crate::{
    AgentActiveEffect, SemanticActionExecutionApplied, SemanticActionFailure,
    SemanticActionSettlementStart, SemanticSettleError, SemanticSettleEvent, SemanticSettleFact,
    SemanticSettleInstant, SemanticSettleStatus, SemanticSettleTracker, SemanticSnapshot,
    MAX_AGENT_PENDING_EFFECTS,
};

/// Process-wide ceiling for applied actions awaiting independent verification.
pub const MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS: usize = MAX_AGENT_PENDING_EFFECTS;

struct SettlementEntry {
    key: SemanticActionCoordinatorKey,
    start: SemanticActionSettlementStart,
}

/// Move-only authority to route one event or exact wake to a pending action.
#[must_use]
pub struct SemanticActionSettlementReservation {
    key: SemanticActionCoordinatorKey,
    next_wake: SemanticSettleInstant,
}

impl SemanticActionSettlementReservation {
    /// Exact action-attempt identity retained without page content.
    pub const fn attempt(&self) -> crate::SemanticActionAttemptId {
        self.key.attempt()
    }

    /// Earliest exact monotonic wake for the sole shell-owned timer.
    pub const fn next_wake(&self) -> SemanticSettleInstant {
        self.next_wake
    }
}

impl fmt::Debug for SemanticActionSettlementReservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionSettlementReservation")
            .field("effect", &self.key.effect())
            .field("attempt", &self.key.attempt())
            .field("next_wake", &self.next_wake)
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Content-free current settlement debt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticActionSettlementCoordinatorStatus {
    pending: u8,
    sealed: bool,
}

impl SemanticActionSettlementCoordinatorStatus {
    /// Applied actions still awaiting a terminal settle condition.
    pub const fn pending(self) -> u8 {
        self.pending
    }

    /// Whether shutdown permanently closed new pending admission.
    pub const fn sealed(self) -> bool {
        self.sealed
    }
}

/// Exact result of admitting or advancing one settlement.
#[must_use]
pub enum SemanticActionSettlementUpdate {
    /// Settlement remains pending under one replacement wake reservation.
    Pending(SemanticActionSettlementReservation),
    /// Settlement is ready for verification or carries one typed failure.
    Terminal(Box<SemanticActionSettlementTerminal>),
}

/// Complete exact settlement state ready for verification or typed failure.
#[must_use]
pub struct SemanticActionSettlementTerminal {
    start: SemanticActionSettlementStart,
}

impl SemanticActionSettlementTerminal {
    fn new(start: SemanticActionSettlementStart) -> Self {
        Self { start }
    }

    /// Exact dispatched policy authority retained through settlement.
    pub const fn active(&self) -> &AgentActiveEffect {
        self.start.active()
    }

    /// Content-free fixed-backend attribution and execution timing.
    pub const fn execution(&self) -> SemanticActionExecutionApplied {
        self.start.execution()
    }

    /// Exact terminal tracker for independent effect verification.
    pub const fn tracker(&self) -> &SemanticSettleTracker {
        self.start.tracker()
    }

    /// Separates terminal policy authority, execution metrics, and verification state.
    pub(crate) fn into_parts(
        self,
    ) -> (
        AgentActiveEffect,
        SemanticActionExecutionApplied,
        SemanticSettleTracker,
    ) {
        self.start.into_parts()
    }
}

impl fmt::Debug for SemanticActionSettlementTerminal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionSettlementTerminal")
            .field("start", &self.start)
            .finish()
    }
}

impl fmt::Debug for SemanticActionSettlementUpdate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending(reservation) => {
                formatter.debug_tuple("Pending").field(reservation).finish()
            }
            Self::Terminal(start) => formatter.debug_tuple("Terminal").field(start).finish(),
        }
    }
}

/// Zero-idle owner of bounded post-native settlement state.
#[derive(Default)]
pub struct SemanticActionSettlementCoordinator {
    pending: Vec<SettlementEntry>,
    sealed: bool,
}

impl SemanticActionSettlementCoordinator {
    /// Constructs an empty coordinator without allocating.
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
            sealed: false,
        }
    }

    /// Admits one exact applied action into event-driven settlement.
    ///
    /// An immediately-ready action never enters the vector and remains
    /// terminally drainable after sealing. Any pending refusal returns the
    /// complete start owner so policy authority cannot be leaked.
    pub fn begin(
        &mut self,
        start: SemanticActionSettlementStart,
    ) -> Result<SemanticActionSettlementUpdate, SemanticActionSettlementAdmissionRefusal> {
        if start.tracker().status().is_terminal() {
            return Ok(SemanticActionSettlementUpdate::Terminal(Box::new(
                SemanticActionSettlementTerminal::new(start),
            )));
        }

        let key = start.coordinator_key();
        let error =
            if self.sealed {
                Some(SemanticActionSettlementCoordinatorError::Shutdown)
            } else if self.pending.iter().any(|entry| {
                entry.key.effect() == key.effect() || entry.key.attempt() == key.attempt()
            }) {
                Some(SemanticActionSettlementCoordinatorError::DuplicateSettlement)
            } else if self
                .pending
                .iter()
                .any(|entry| entry.key.context() == key.context())
            {
                Some(SemanticActionSettlementCoordinatorError::ContextBusy)
            } else if self.pending.len() >= MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS {
                Some(SemanticActionSettlementCoordinatorError::Capacity)
            } else {
                None
            };
        if let Some(error) = error {
            return Err(SemanticActionSettlementAdmissionRefusal::new(start, error));
        }

        let next_wake = pending_wake(&start);
        self.pending.push(SettlementEntry { key, start });
        Ok(SemanticActionSettlementUpdate::Pending(
            SemanticActionSettlementReservation { key, next_wake },
        ))
    }

    /// Routes one exact coalesced shell fact without polling.
    ///
    /// A malformed fact returns the still-current submitted handle.
    pub fn observe(
        &mut self,
        reservation: SemanticActionSettlementReservation,
        event: SemanticSettleEvent,
    ) -> Result<SemanticActionSettlementUpdate, SemanticActionSettlementAdvanceRefusal> {
        let index = match self.reservation_index(&reservation) {
            Ok(index) => index,
            Err(error) => {
                return Err(SemanticActionSettlementAdvanceRefusal::new(
                    reservation,
                    error,
                ));
            }
        };
        match self.pending[index].start.tracker_mut().observe(event) {
            Ok(status) => Ok(self.update(index, status)),
            Err(error) => Err(SemanticActionSettlementAdvanceRefusal::new(
                reservation,
                SemanticActionSettlementCoordinatorError::Settlement(error),
            )),
        }
    }

    /// Routes one borrowed exact snapshot for a target-state settle condition.
    ///
    /// The snapshot is never retained. An invalid snapshot returns the
    /// still-current submitted handle.
    pub fn observe_snapshot(
        &mut self,
        reservation: SemanticActionSettlementReservation,
        observed_at: SemanticSettleInstant,
        snapshot: &SemanticSnapshot,
    ) -> Result<SemanticActionSettlementUpdate, SemanticActionSettlementAdvanceRefusal> {
        let index = match self.reservation_index(&reservation) {
            Ok(index) => index,
            Err(error) => {
                return Err(SemanticActionSettlementAdvanceRefusal::new(
                    reservation,
                    error,
                ));
            }
        };
        match self.pending[index].start.tracker_mut().observe_snapshot(
            reservation.key.attempt(),
            observed_at,
            snapshot,
        ) {
            Ok(status) => Ok(self.update(index, status)),
            Err(error) => Err(SemanticActionSettlementAdvanceRefusal::new(
                reservation,
                SemanticActionSettlementCoordinatorError::Settlement(error),
            )),
        }
    }

    /// Delivers the sole scheduled wake no earlier than its current boundary.
    pub fn wake(
        &mut self,
        reservation: SemanticActionSettlementReservation,
        now: SemanticSettleInstant,
    ) -> Result<SemanticActionSettlementUpdate, SemanticActionSettlementAdvanceRefusal> {
        let index = match self.reservation_index(&reservation) {
            Ok(index) => index,
            Err(error) => {
                return Err(SemanticActionSettlementAdvanceRefusal::new(
                    reservation,
                    error,
                ));
            }
        };
        if now < reservation.next_wake {
            return Err(SemanticActionSettlementAdvanceRefusal::new(
                reservation,
                SemanticActionSettlementCoordinatorError::PrematureWake,
            ));
        }
        let event =
            SemanticSettleEvent::new(reservation.key.attempt(), now, SemanticSettleFact::Tick);
        match self.pending[index].start.tracker_mut().observe(event) {
            Ok(status) => Ok(self.update(index, status)),
            Err(error) => Err(SemanticActionSettlementAdvanceRefusal::new(
                reservation,
                SemanticActionSettlementCoordinatorError::Settlement(error),
            )),
        }
    }

    /// Permanently refuses new pending settlements while retaining admitted debt.
    pub fn seal(&mut self) -> SemanticActionSettlementCoordinatorStatus {
        self.sealed = true;
        self.status()
    }

    /// Content-free current capacity and shutdown accounting.
    pub fn status(&self) -> SemanticActionSettlementCoordinatorStatus {
        SemanticActionSettlementCoordinatorStatus {
            pending: u8::try_from(self.pending.len()).unwrap_or(u8::MAX),
            sealed: self.sealed,
        }
    }

    fn reservation_index(
        &self,
        reservation: &SemanticActionSettlementReservation,
    ) -> Result<usize, SemanticActionSettlementCoordinatorError> {
        let Some(index) = self.pending.iter().position(|entry| {
            entry.key.effect() == reservation.key.effect()
                || entry.key.attempt() == reservation.key.attempt()
        }) else {
            return Err(SemanticActionSettlementCoordinatorError::UnknownSettlement);
        };
        if self.pending[index].key != reservation.key {
            return Err(SemanticActionSettlementCoordinatorError::ReservationMismatch);
        }
        if self.pending[index].start.tracker().next_wake() != Some(reservation.next_wake) {
            return Err(SemanticActionSettlementCoordinatorError::ScheduleMismatch);
        }
        Ok(index)
    }

    fn update(
        &mut self,
        index: usize,
        status: SemanticSettleStatus,
    ) -> SemanticActionSettlementUpdate {
        if status.is_terminal() {
            return SemanticActionSettlementUpdate::Terminal(Box::new(
                SemanticActionSettlementTerminal::new(self.pending.remove(index).start),
            ));
        }
        let entry = &self.pending[index];
        SemanticActionSettlementUpdate::Pending(SemanticActionSettlementReservation {
            key: entry.key,
            next_wake: pending_wake(&entry.start),
        })
    }
}

impl fmt::Debug for SemanticActionSettlementCoordinator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionSettlementCoordinator")
            .field("status", &self.status())
            .finish()
    }
}

fn pending_wake(start: &SemanticActionSettlementStart) -> SemanticSettleInstant {
    start
        .tracker()
        .next_wake()
        .unwrap_or_else(|| start.tracker().deadline())
}

/// Admission refusal that returns the complete applied settlement owner.
#[must_use]
pub struct SemanticActionSettlementAdmissionRefusal {
    start: Box<SemanticActionSettlementStart>,
    error: SemanticActionSettlementCoordinatorError,
}

impl SemanticActionSettlementAdmissionRefusal {
    fn new(
        start: SemanticActionSettlementStart,
        error: SemanticActionSettlementCoordinatorError,
    ) -> Self {
        Self {
            start: Box::new(start),
            error,
        }
    }

    /// Closed admission failure.
    pub const fn error(&self) -> SemanticActionSettlementCoordinatorError {
        self.error
    }

    /// Closed action failure derived from this exact consuming refusal.
    pub const fn action_failure(&self) -> SemanticActionFailure {
        match self.error {
            SemanticActionSettlementCoordinatorError::ContextBusy
            | SemanticActionSettlementCoordinatorError::Capacity => {
                SemanticActionFailure::ResourceExhausted
            }
            SemanticActionSettlementCoordinatorError::DuplicateSettlement
            | SemanticActionSettlementCoordinatorError::Shutdown
            | SemanticActionSettlementCoordinatorError::UnknownSettlement
            | SemanticActionSettlementCoordinatorError::ReservationMismatch
            | SemanticActionSettlementCoordinatorError::ScheduleMismatch
            | SemanticActionSettlementCoordinatorError::PrematureWake
            | SemanticActionSettlementCoordinatorError::Settlement(_) => {
                SemanticActionFailure::BackendRefused
            }
        }
    }

    /// Recovers applied policy authority and the bounded tracker.
    pub(crate) fn into_parts(
        self,
    ) -> (
        SemanticActionSettlementStart,
        SemanticActionSettlementCoordinatorError,
    ) {
        (*self.start, self.error)
    }
}

impl fmt::Debug for SemanticActionSettlementAdmissionRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionSettlementAdmissionRefusal")
            .field("start", &"[redacted]")
            .field("error", &self.error)
            .finish()
    }
}

/// Advance refusal that returns the submitted move-only reservation.
///
/// It remains current after a fact/snapshot rejection or premature wake. An
/// unknown, mismatched, or replaced reservation is returned only for explicit
/// disposal and cannot mutate retained debt.
#[must_use]
pub struct SemanticActionSettlementAdvanceRefusal {
    reservation: Box<SemanticActionSettlementReservation>,
    error: SemanticActionSettlementCoordinatorError,
}

impl SemanticActionSettlementAdvanceRefusal {
    fn new(
        reservation: SemanticActionSettlementReservation,
        error: SemanticActionSettlementCoordinatorError,
    ) -> Self {
        Self {
            reservation: Box::new(reservation),
            error,
        }
    }

    /// Closed routing or settlement failure.
    pub const fn error(&self) -> SemanticActionSettlementCoordinatorError {
        self.error
    }

    /// Recovers the submitted reservation after a refused event or wake.
    pub fn into_parts(
        self,
    ) -> (
        SemanticActionSettlementReservation,
        SemanticActionSettlementCoordinatorError,
    ) {
        (*self.reservation, self.error)
    }
}

impl fmt::Debug for SemanticActionSettlementAdvanceRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionSettlementAdvanceRefusal")
            .field("reservation", &self.reservation)
            .field("error", &self.error)
            .finish()
    }
}

/// Closed settlement admission, routing, or wake failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionSettlementCoordinatorError {
    /// An effect or attempt identity is already settling.
    #[error("semantic action settlement identity is already pending")]
    DuplicateSettlement,
    /// The same logical context already has pending settlement debt.
    #[error("semantic action context already has a pending settlement")]
    ContextBusy,
    /// Process-wide settlement capacity is exhausted.
    #[error("semantic action settlement capacity is exhausted")]
    Capacity,
    /// Shutdown permanently sealed new pending settlement admission.
    #[error("semantic action settlement coordinator is shutting down")]
    Shutdown,
    /// No pending settlement matched the reservation identity.
    #[error("semantic action settlement is not pending")]
    UnknownSettlement,
    /// A reservation did not match retained exact authority.
    #[error("semantic action settlement reservation mismatched")]
    ReservationMismatch,
    /// A replaced wake reservation was replayed.
    #[error("semantic action settlement wake schedule was replaced")]
    ScheduleMismatch,
    /// The shell delivered a wake before its exact current boundary.
    #[error("semantic action settlement wake was premature")]
    PrematureWake,
    /// The exact settlement core rejected a routed fact or snapshot.
    #[error(transparent)]
    Settlement(SemanticSettleError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        begin_semantic_action_settlement, decode_semantic_snapshot,
        prepare_semantic_action_execution, AgentActiveEffect, ContextCapabilities,
        ContextCapability, ContextId, ContextIdentity, ContextKind, ContextOperationId,
        ContextRegistry, ContextRunId, ContextSettlement, FrameGeneration, FrameId,
        SemanticActionBatch, SemanticActionBatchId, SemanticActionExecutionBackend,
        SemanticActionExecutionInstant, SemanticActionIntent, SemanticActionNativeReadiness,
        SemanticActionNativeViewport, SemanticActionProposal, SemanticActionText,
        SemanticDecodeContext, SemanticEffectClass, SemanticFrameTrust, SemanticInvocationId,
        SemanticMutationQuietPeriod, SemanticObservation, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticObservationId, SemanticOrigin, SemanticPreparedAction,
        SemanticReferenceId, SemanticSettleBudget, SemanticSnapshotGeneration, SemanticState,
        SemanticVerification, SemanticWaitCondition, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn observation(seed: u128) -> SemanticObservation {
        let identity = ContextIdentity::new(
            ContextId::from_raw(seed + 1),
            ContextRunId::from_raw(seed + 2),
            ProfileId::from(seed + 3),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[ContextCapability::Observe, ContextCapability::Act],
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
            .expect("settlement");
        let context = registry.join(identity.id()).expect("context");
        let frame = crate::SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://settlement-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 7,
            "g": 9,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Private settlement action",
                 "s": 0, "o": 9, "b": {"x": 10, "y": 20, "w": 100, "h": 30}},
                {"k": 3, "p": 0, "r": "textbox", "n": "Private settlement field",
                 "v": {"k": "text", "value": "old"}, "o": 11,
                 "b": {"x": 10, "y": 60, "w": 200, "h": 30}}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(7).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(9).expect("generation"),
            ),
            &bytes,
        )
        .expect("snapshot");
        let request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    fn action(
        observation: &SemanticObservation,
        batch: u64,
        wait: SemanticWaitCondition,
        verification: SemanticVerification,
        budget_millis: u32,
    ) -> SemanticPreparedAction {
        let proposal = SemanticActionProposal::try_new(
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(2).expect("target"),
            },
            SemanticEffectClass::Read,
            wait,
            verification,
            SemanticSettleBudget::try_new(budget_millis).expect("budget"),
        )
        .expect("proposal");
        SemanticActionBatch::bind(
            SemanticActionBatchId::new(batch).expect("batch"),
            observation,
            &[observation.frames()[0].frame().clone()],
            vec![proposal],
        )
        .expect("batch")
        .actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare")
    }

    fn immediate_action(observation: &SemanticObservation, batch: u64) -> SemanticPreparedAction {
        action(
            observation,
            batch,
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            250,
        )
    }

    fn quiet_action(observation: &SemanticObservation, batch: u64) -> SemanticPreparedAction {
        action(
            observation,
            batch,
            SemanticWaitCondition::MutationQuiet(
                SemanticMutationQuietPeriod::try_new(100).expect("quiet"),
            ),
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            500,
        )
    }

    fn target_state_action(
        observation: &SemanticObservation,
        batch: u64,
    ) -> SemanticPreparedAction {
        action(
            observation,
            batch,
            SemanticWaitCondition::TargetState {
                state: SemanticState::Expanded,
                present: true,
            },
            SemanticVerification::TargetState {
                state: SemanticState::Expanded,
                present: true,
            },
            250,
        )
    }

    fn fill_action(observation: &SemanticObservation, batch: u64) -> SemanticPreparedAction {
        let proposal = SemanticActionProposal::try_new(
            SemanticActionIntent::Fill {
                target: SemanticReferenceId::new(3).expect("target"),
                value: SemanticActionText::try_new("private-settlement-value".to_owned())
                    .expect("text"),
            },
            SemanticEffectClass::LocalWrite,
            SemanticWaitCondition::MutationQuiet(
                SemanticMutationQuietPeriod::try_new(100).expect("quiet"),
            ),
            SemanticVerification::TargetValueMatchesInput,
            SemanticSettleBudget::try_new(500).expect("budget"),
        )
        .expect("proposal");
        SemanticActionBatch::bind(
            SemanticActionBatchId::new(batch).expect("batch"),
            observation,
            &[observation.frames()[0].frame().clone()],
            vec![proposal],
        )
        .expect("batch")
        .actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare")
    }

    fn start(
        action: &SemanticPreparedAction,
        attempt: u64,
        completed_at: u64,
    ) -> SemanticActionSettlementStart {
        let active = AgentActiveEffect::for_execution_test(
            action,
            crate::SemanticActionAttemptId::new(attempt).expect("attempt"),
        );
        let (pending, native) = prepare_semantic_action_execution(
            active,
            action,
            SemanticActionExecutionInstant::from_millis(completed_at - 20),
        )
        .expect("execution");
        let actual_geometry = native.expected_geometry();
        let outcome = pending.settle(
            action.frame(),
            native.complete(
                SemanticActionExecutionBackend::FixedSemanticRecipe,
                SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
                SemanticActionNativeViewport::try_new(800, 600).expect("viewport"),
                actual_geometry,
                SemanticActionExecutionInstant::from_millis(completed_at - 10),
                SemanticActionExecutionInstant::from_millis(completed_at),
            ),
        );
        begin_semantic_action_settlement(outcome, action).expect("settlement start")
    }

    fn pending(update: SemanticActionSettlementUpdate) -> SemanticActionSettlementReservation {
        let SemanticActionSettlementUpdate::Pending(reservation) = update else {
            panic!("settlement unexpectedly terminal");
        };
        reservation
    }

    fn terminal(update: SemanticActionSettlementUpdate) -> SemanticActionSettlementTerminal {
        let SemanticActionSettlementUpdate::Terminal(start) = update else {
            panic!("settlement unexpectedly pending");
        };
        *start
    }

    fn snapshot(
        observation: &SemanticObservation,
        generation: u64,
        button_states: u8,
        completeness: &str,
    ) -> crate::SemanticSnapshot {
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": generation + 10,
            "g": generation,
            "c": completeness,
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Private settlement action",
                 "s": button_states, "o": 9,
                 "b": {"x": 10, "y": 20, "w": 100, "h": 30}}
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
    fn immediate_terminal_never_enters_pending_storage() {
        let observation = observation(10);
        let action = immediate_action(&observation, 10);
        let mut coordinator = SemanticActionSettlementCoordinator::new();
        assert_eq!(coordinator.status().pending(), 0);
        assert!(!coordinator.status().sealed());

        let terminal = terminal(coordinator.begin(start(&action, 10, 1_000)).expect("begin"));
        assert_eq!(terminal.active().attempt().get(), 10);
        assert_eq!(
            terminal.tracker().status(),
            SemanticSettleStatus::ReadyForVerification
        );
        assert_eq!(terminal.tracker().event_count(), 0);
        assert_eq!(coordinator.status().pending(), 0);
    }

    #[test]
    fn mutation_replaces_one_wake_and_stale_or_premature_wakes_fail_closed() {
        let observation = observation(20);
        let action = quiet_action(&observation, 20);
        let mut coordinator = SemanticActionSettlementCoordinator::new();
        let reservation = pending(coordinator.begin(start(&action, 20, 1_000)).expect("begin"));
        assert_eq!(reservation.next_wake().millis(), 1_100);
        assert_eq!(coordinator.status().pending(), 1);

        let refusal = coordinator
            .wake(reservation, SemanticSettleInstant::from_millis(1_099))
            .expect_err("premature wake");
        assert_eq!(
            refusal.error(),
            SemanticActionSettlementCoordinatorError::PrematureWake
        );
        let (reservation, _) = refusal.into_parts();
        let stale = SemanticActionSettlementReservation {
            key: reservation.key,
            next_wake: reservation.next_wake,
        };
        let update = coordinator
            .observe(
                reservation,
                SemanticSettleEvent::new(
                    crate::SemanticActionAttemptId::new(20).expect("attempt"),
                    SemanticSettleInstant::from_millis(1_050),
                    SemanticSettleFact::Mutation(action.frame().clone()),
                ),
            )
            .expect("mutation");
        let reservation = pending(update);
        assert_eq!(reservation.next_wake().millis(), 1_150);

        let refusal = coordinator
            .wake(stale, SemanticSettleInstant::from_millis(1_100))
            .expect_err("stale wake");
        assert_eq!(
            refusal.error(),
            SemanticActionSettlementCoordinatorError::ScheduleMismatch
        );
        let terminal = terminal(
            coordinator
                .wake(reservation, SemanticSettleInstant::from_millis(1_150))
                .expect("quiet wake"),
        );
        assert_eq!(
            terminal.tracker().status(),
            SemanticSettleStatus::ReadyForVerification
        );
        assert_eq!(terminal.tracker().event_count(), 2);
        assert_eq!(terminal.tracker().elapsed_millis(), Some(150));
        assert_eq!(coordinator.status().pending(), 0);
    }

    #[test]
    fn snapshot_routing_returns_current_reservation_after_rejection() {
        let observation = observation(30);
        let action = target_state_action(&observation, 30);
        let mut coordinator = SemanticActionSettlementCoordinator::new();
        let reservation = pending(coordinator.begin(start(&action, 30, 1_000)).expect("begin"));

        let refusal = coordinator
            .observe(
                reservation,
                SemanticSettleEvent::new(
                    crate::SemanticActionAttemptId::new(31).expect("wrong attempt"),
                    SemanticSettleInstant::from_millis(1_010),
                    SemanticSettleFact::Tick,
                ),
            )
            .expect_err("wrong attempt");
        assert_eq!(
            refusal.error(),
            SemanticActionSettlementCoordinatorError::Settlement(
                SemanticSettleError::AttemptMismatch
            )
        );
        let (reservation, _) = refusal.into_parts();
        let incomplete = snapshot(&observation, 10, 4, "node_limit");
        let refusal = coordinator
            .observe_snapshot(
                reservation,
                SemanticSettleInstant::from_millis(1_020),
                &incomplete,
            )
            .expect_err("incomplete");
        assert_eq!(
            refusal.error(),
            SemanticActionSettlementCoordinatorError::Settlement(
                SemanticSettleError::IncompleteSnapshot
            )
        );
        let (reservation, _) = refusal.into_parts();
        let current = snapshot(&observation, 10, 4, "complete");
        let terminal = terminal(
            coordinator
                .observe_snapshot(
                    reservation,
                    SemanticSettleInstant::from_millis(1_030),
                    &current,
                )
                .expect("snapshot"),
        );
        assert_eq!(
            terminal.tracker().status(),
            SemanticSettleStatus::ReadyForVerification
        );
        assert_eq!(terminal.tracker().event_count(), 1);
        assert_eq!(coordinator.status().pending(), 0);
    }

    #[test]
    fn context_capacity_and_shutdown_refusals_return_complete_authority() {
        let mut coordinator = SemanticActionSettlementCoordinator::new();
        let first_observation = observation(100);
        let first_action = quiet_action(&first_observation, 100);
        let first = pending(
            coordinator
                .begin(start(&first_action, 100, 1_000))
                .expect("first"),
        );

        let busy = coordinator
            .begin(start(&quiet_action(&first_observation, 101), 101, 1_000))
            .expect_err("context busy");
        assert_eq!(
            busy.error(),
            SemanticActionSettlementCoordinatorError::ContextBusy
        );
        assert_eq!(busy.into_parts().0.active().attempt().get(), 101);

        let duplicate_observation = observation(200);
        let duplicate_action = quiet_action(&duplicate_observation, 102);
        let duplicate = coordinator
            .begin(start(&duplicate_action, 100, 1_000))
            .expect_err("duplicate identity");
        assert_eq!(
            duplicate.error(),
            SemanticActionSettlementCoordinatorError::DuplicateSettlement
        );
        assert_eq!(
            duplicate.action_failure(),
            SemanticActionFailure::BackendRefused
        );
        assert_eq!(duplicate.into_parts().0.active().attempt().get(), 100);

        let mut reservations = vec![first];
        for offset in 1..MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS {
            let observation = observation(300 + offset as u128 * 10);
            let action = quiet_action(&observation, 300 + offset as u64);
            reservations.push(pending(
                coordinator
                    .begin(start(&action, 300 + offset as u64, 1_000))
                    .expect("capacity admission"),
            ));
        }
        assert_eq!(
            coordinator.status().pending(),
            MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS as u8
        );
        let overflow_observation = observation(800);
        let overflow_action = quiet_action(&overflow_observation, 800);
        let overflow = coordinator
            .begin(start(&overflow_action, 800, 1_000))
            .expect_err("capacity");
        assert_eq!(
            overflow.error(),
            SemanticActionSettlementCoordinatorError::Capacity
        );
        assert_eq!(
            overflow.action_failure(),
            SemanticActionFailure::ResourceExhausted
        );
        assert_eq!(overflow.into_parts().0.active().attempt().get(), 800);

        assert_eq!(
            coordinator.seal().pending(),
            MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS as u8
        );
        assert!(coordinator.status().sealed());
        let shutdown_observation = observation(900);
        let shutdown_action = quiet_action(&shutdown_observation, 900);
        let shutdown = coordinator
            .begin(start(&shutdown_action, 900, 1_000))
            .expect_err("shutdown");
        assert_eq!(
            shutdown.error(),
            SemanticActionSettlementCoordinatorError::Shutdown
        );
        assert_eq!(
            shutdown.action_failure(),
            SemanticActionFailure::BackendRefused
        );
        assert_eq!(shutdown.into_parts().0.active().attempt().get(), 900);

        let immediate = immediate_action(&shutdown_observation, 901);
        let immediate_terminal = terminal(
            coordinator
                .begin(start(&immediate, 901, 1_000))
                .expect("terminal drain after seal"),
        );
        assert_eq!(immediate_terminal.active().attempt().get(), 901);

        for reservation in reservations {
            let terminal = terminal(
                coordinator
                    .wake(reservation, SemanticSettleInstant::from_millis(1_100))
                    .expect("drain"),
            );
            assert_eq!(
                terminal.tracker().status(),
                SemanticSettleStatus::ReadyForVerification
            );
        }
        assert_eq!(coordinator.status().pending(), 0);
        assert!(coordinator.status().sealed());
        let debug = format!("{coordinator:?}");
        assert!(!debug.contains("settlement-private"));
        assert!(!debug.contains("Private settlement action"));
    }

    #[test]
    fn cross_coordinator_reservation_cannot_release_retained_debt() {
        let first_observation = observation(1_000);
        let second_observation = observation(1_100);
        let first_action = quiet_action(&first_observation, 1_000);
        let second_action = quiet_action(&second_observation, 1_100);
        let mut first = SemanticActionSettlementCoordinator::new();
        let mut second = SemanticActionSettlementCoordinator::new();
        let first_reservation = pending(
            first
                .begin(start(&first_action, 1_000, 1_000))
                .expect("first"),
        );
        let second_reservation = pending(
            second
                .begin(start(&second_action, 1_000, 1_000))
                .expect("second"),
        );
        let second_recovery = SemanticActionSettlementReservation {
            key: second_reservation.key,
            next_wake: second_reservation.next_wake,
        };

        let refusal = first
            .wake(
                second_reservation,
                SemanticSettleInstant::from_millis(1_100),
            )
            .expect_err("cross coordinator");
        assert_eq!(
            refusal.error(),
            SemanticActionSettlementCoordinatorError::ReservationMismatch
        );
        assert_eq!(first.status().pending(), 1);
        assert_eq!(second.status().pending(), 1);
        let _first = terminal(
            first
                .wake(first_reservation, SemanticSettleInstant::from_millis(1_100))
                .expect("first drain"),
        );
        let _second = terminal(
            second
                .wake(second_recovery, SemanticSettleInstant::from_millis(1_100))
                .expect("second drain"),
        );
    }

    #[test]
    fn content_bearing_action_values_never_enter_coordinator_diagnostics() {
        let observation = observation(1_200);
        let action = fill_action(&observation, 1_200);
        let mut coordinator = SemanticActionSettlementCoordinator::new();
        let reservation = pending(
            coordinator
                .begin(start(&action, 1_200, 1_000))
                .expect("begin"),
        );
        let debug = format!("{coordinator:?} {reservation:?}");
        assert!(!debug.contains("private-settlement-value"));
        assert!(!debug.contains("settlement-private"));
        assert!(!debug.contains("Private settlement action"));
    }
}
