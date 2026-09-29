//! Bounded single-owner coordination for native semantic action debt.
//!
//! The coordinator retains policy authority while a move-only request is in a
//! trusted native port. It allocates nothing while unused, admits at most one
//! request per logical context and four process-wide, has no worker or timer,
//! and never drops a reservation merely because a malformed terminal arrived.

use std::fmt;

use thiserror::Error;

use crate::semantic_execute::SemanticActionCoordinatorKey;
use crate::{
    prepare_semantic_action_execution, AgentActiveEffect, ContextDispatch, ContextPortFailure,
    SemanticActionExecutionInstant, SemanticActionExecutionOutcome, SemanticActionExecutionPending,
    SemanticActionExecutionPreparationError, SemanticActionFailure, SemanticActionNativeRequest,
    SemanticActionNativeSettlement, SemanticFrameJoin, SemanticPreparedAction,
    MAX_AGENT_PENDING_EFFECTS,
};

/// Process-wide ceiling for native actions executing or awaiting settlement.
pub const MAX_PENDING_SEMANTIC_ACTION_EXECUTIONS: usize = MAX_AGENT_PENDING_EFFECTS;

/// Move-only cancellation/refusal handle for one coordinator-owned request.
#[must_use]
pub struct SemanticActionExecutionReservation {
    key: SemanticActionCoordinatorKey,
    deadline: SemanticActionExecutionInstant,
}

impl SemanticActionExecutionReservation {
    /// Exact action-attempt identity retained without page content.
    pub const fn attempt(&self) -> crate::SemanticActionAttemptId {
        self.key.attempt()
    }

    /// Absolute native-execution wake retained after the request moves away.
    pub const fn deadline(&self) -> SemanticActionExecutionInstant {
        self.deadline
    }
}

impl fmt::Debug for SemanticActionExecutionReservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionExecutionReservation")
            .field("effect", &self.key.effect())
            .field("attempt", &self.key.attempt())
            .field("deadline", &self.deadline)
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Content-free current native-action debt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticActionExecutionCoordinatorStatus {
    pending: u8,
    sealed: bool,
}

/// Result of reconciling synchronous native-port admission.
#[must_use]
pub enum SemanticActionExecutionDispatch {
    /// The port accepted the request and now owes one exact callback.
    Scheduled,
    /// The port refused synchronously and policy authority was recovered.
    Refused(Box<SemanticActionExecutionOutcome>),
}

impl fmt::Debug for SemanticActionExecutionDispatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scheduled => formatter.write_str("Scheduled"),
            Self::Refused(outcome) => formatter.debug_tuple("Refused").field(outcome).finish(),
        }
    }
}

impl SemanticActionExecutionCoordinatorStatus {
    /// Native requests executing or awaiting exact settlement.
    pub const fn pending(self) -> u8 {
        self.pending
    }

    /// Whether shutdown permanently closed new admission.
    pub const fn sealed(self) -> bool {
        self.sealed
    }
}

/// Zero-idle owner of bounded native semantic action reservations.
#[derive(Default)]
pub struct SemanticActionExecutionCoordinator {
    pending: Vec<SemanticActionExecutionPending>,
    sealed: bool,
}

impl SemanticActionExecutionCoordinator {
    /// Constructs an empty coordinator without allocating.
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
            sealed: false,
        }
    }

    /// Prepares and admits one exact policy-dispatched native request.
    ///
    /// On any refusal the still-dispatched policy authority is returned. A
    /// successful admission moves that authority into this coordinator until
    /// exact settlement, synchronous refusal, cancellation, or timeout.
    pub fn begin(
        &mut self,
        active: AgentActiveEffect,
        action: &SemanticPreparedAction,
        requested_at: SemanticActionExecutionInstant,
    ) -> Result<
        (
            SemanticActionExecutionReservation,
            SemanticActionNativeRequest,
        ),
        SemanticActionExecutionCoordinatorRefusal,
    > {
        let error =
            if self.sealed {
                Some(SemanticActionExecutionCoordinatorError::Shutdown)
            } else if !active.matches_action(action) {
                Some(SemanticActionExecutionCoordinatorError::Preparation(
                    SemanticActionExecutionPreparationError::AuthorityMismatch,
                ))
            } else if self.pending.iter().any(|entry| {
                let admitted = entry.coordinator_key();
                admitted.effect() == active.id() || admitted.attempt() == active.attempt()
            }) {
                Some(SemanticActionExecutionCoordinatorError::DuplicateRequest)
            } else if self.pending.iter().any(|entry| {
                entry.coordinator_key().context() == action.frame().context().identity()
            }) {
                Some(SemanticActionExecutionCoordinatorError::ContextBusy)
            } else if self.pending.len() >= MAX_PENDING_SEMANTIC_ACTION_EXECUTIONS {
                Some(SemanticActionExecutionCoordinatorError::Capacity)
            } else {
                None
            };
        if let Some(error) = error {
            return Err(SemanticActionExecutionCoordinatorRefusal::new(
                active, error,
            ));
        }
        let (pending, native) = prepare_semantic_action_execution(active, action, requested_at)
            .map_err(|refusal| {
                let (active, error) = refusal.into_parts();
                SemanticActionExecutionCoordinatorRefusal::new(
                    active,
                    SemanticActionExecutionCoordinatorError::Preparation(error),
                )
            })?;
        let key = pending.coordinator_key();
        let deadline = pending.deadline();
        self.pending.push(pending);
        Ok((SemanticActionExecutionReservation { key, deadline }, native))
    }

    /// Checks an exact native terminal without consuming it or its reservation.
    /// Hosts use this to retain a mismatched callback for explicit recovery.
    pub fn accepts_settlement(
        &self,
        reservation: &SemanticActionExecutionReservation,
        settlement: &SemanticActionNativeSettlement,
    ) -> bool {
        self.reservation_index(reservation).is_ok_and(|index| {
            self.pending[index].coordinator_key() == settlement.coordinator_key()
                && self.pending[index].matches_native_settlement(settlement)
        })
    }

    /// Admits one exact native terminal and releases only its reservation.
    pub fn settle(
        &mut self,
        current_frame: &SemanticFrameJoin,
        settlement: SemanticActionNativeSettlement,
    ) -> Result<SemanticActionExecutionOutcome, SemanticActionExecutionCoordinatorError> {
        let key = settlement.coordinator_key();
        let Some(index) = self.pending.iter().position(|entry| {
            let admitted = entry.coordinator_key();
            admitted.effect() == key.effect() || admitted.attempt() == key.attempt()
        }) else {
            return Err(SemanticActionExecutionCoordinatorError::UnknownRequest);
        };
        if self.pending[index].coordinator_key() != key
            || !self.pending[index].matches_native_settlement(&settlement)
        {
            return Err(SemanticActionExecutionCoordinatorError::RequestMismatch);
        }
        Ok(self.pending.remove(index).settle(current_frame, settlement))
    }

    /// Reconciles one exact synchronous port dispatch result.
    ///
    /// `Scheduled` retains the reservation because the port owes one callback.
    /// Every other closed result releases it into one typed policy failure.
    pub fn account_dispatch(
        &mut self,
        reservation: &SemanticActionExecutionReservation,
        dispatch: ContextDispatch,
    ) -> Result<SemanticActionExecutionDispatch, SemanticActionExecutionCoordinatorError> {
        let Some(failure) = semantic_action_dispatch_failure(dispatch) else {
            self.reservation_index(reservation)?;
            return Ok(SemanticActionExecutionDispatch::Scheduled);
        };
        self.refuse(reservation, failure)
            .map(|outcome| SemanticActionExecutionDispatch::Refused(Box::new(outcome)))
    }

    /// Releases an exact reservation after a synchronous port refusal or cancellation.
    pub fn refuse(
        &mut self,
        reservation: &SemanticActionExecutionReservation,
        failure: SemanticActionFailure,
    ) -> Result<SemanticActionExecutionOutcome, SemanticActionExecutionCoordinatorError> {
        let index = self.reservation_index(reservation)?;
        Ok(self.pending.remove(index).refuse(failure))
    }

    /// Releases an exact reservation only after its absolute native deadline.
    pub fn expire(
        &mut self,
        reservation: &SemanticActionExecutionReservation,
        now: SemanticActionExecutionInstant,
    ) -> Result<SemanticActionExecutionOutcome, SemanticActionExecutionCoordinatorError> {
        let index = self.reservation_index(reservation)?;
        if now < self.pending[index].deadline() {
            return Err(SemanticActionExecutionCoordinatorError::PrematureTimeout);
        }
        Ok(self
            .pending
            .remove(index)
            .refuse(SemanticActionFailure::Timeout))
    }

    /// Permanently refuses new requests while retaining terminal debt.
    pub fn seal(&mut self) -> SemanticActionExecutionCoordinatorStatus {
        self.sealed = true;
        self.status()
    }

    /// Content-free current capacity and shutdown accounting.
    pub fn status(&self) -> SemanticActionExecutionCoordinatorStatus {
        SemanticActionExecutionCoordinatorStatus {
            pending: u8::try_from(self.pending.len()).unwrap_or(u8::MAX),
            sealed: self.sealed,
        }
    }

    fn reservation_index(
        &self,
        reservation: &SemanticActionExecutionReservation,
    ) -> Result<usize, SemanticActionExecutionCoordinatorError> {
        let Some(index) = self.pending.iter().position(|entry| {
            let admitted = entry.coordinator_key();
            admitted.effect() == reservation.key.effect()
                || admitted.attempt() == reservation.key.attempt()
        }) else {
            return Err(SemanticActionExecutionCoordinatorError::UnknownRequest);
        };
        if self.pending[index].coordinator_key() != reservation.key {
            return Err(SemanticActionExecutionCoordinatorError::RequestMismatch);
        }
        Ok(index)
    }
}

/// Release-excluded owner used to qualify the real semantic click pipeline.
///
/// This wrapper adds no action mechanism. It drives production binding,
/// preparation, production-shaped execution authority, native request,
/// terminal rejoin, settlement, and verification types so a platform probe
/// cannot manufacture a looser request. It does not perform a real policy
/// assessment or mint production policy authorization. The `probe-harness`
/// feature is mechanically rejected from optimized builds.
#[cfg(feature = "probe-harness")]
#[must_use]
pub struct SemanticClickQualificationExecution {
    execution: SemanticActionQualificationExecution,
}

/// Release-excluded owner for one model-supplied local semantic click.
///
/// This is deliberately narrower than the general execution coordinator. It
/// accepts one already-decoded proposal, requires the fixed local-click
/// contract, then uses the production observation binder, native request
/// contract, settlement, and verification pipeline. It never mints product
/// policy authority; production controller code must use the policy actor's
/// assessment and dispatch path instead.
#[cfg(feature = "probe-harness")]
#[must_use]
pub struct SemanticModelClickQualificationExecution {
    execution: SemanticActionQualificationExecution,
}

/// Release-excluded owner for one model-supplied snapshot-verifiable action.
///
/// This expands live qualification beyond the first click without weakening
/// the production action contract. It accepts only local writes whose
/// postcondition can be established from the adjacent semantic snapshot. A
/// navigation, dialog, scroll, or external effect still requires its distinct
/// production evidence source and is refused here.
#[cfg(feature = "probe-harness")]
#[must_use]
pub struct SemanticModelActionQualificationExecution {
    execution: SemanticActionQualificationExecution,
    expected_proof: crate::SemanticEffectProofKind,
}

#[cfg(feature = "probe-harness")]
#[must_use]
struct SemanticActionQualificationExecution {
    coordinator: SemanticActionExecutionCoordinator,
    reservation: SemanticActionExecutionReservation,
    action: SemanticPreparedAction,
    native: Option<SemanticActionNativeRequest>,
}

#[cfg(feature = "probe-harness")]
impl SemanticActionQualificationExecution {
    fn prepare(
        observation: &crate::SemanticObservation,
        proposal: crate::SemanticActionProposal,
        batch: u64,
        attempt: u64,
        requested_at: SemanticActionExecutionInstant,
    ) -> Result<Self, SemanticActionQualificationError> {
        let frames = observation
            .frames()
            .iter()
            .map(|snapshot| snapshot.frame().clone())
            .collect::<Vec<_>>();
        let batch = crate::SemanticActionBatch::bind(
            crate::SemanticActionBatchId::new(batch)
                .ok_or(SemanticActionQualificationError::Identity)?,
            observation,
            &frames,
            vec![proposal],
        )
        .map_err(SemanticActionQualificationError::Binding)?;
        let action = batch
            .actions()
            .first()
            .ok_or(SemanticActionQualificationError::Contract)?
            .prepare(
                observation
                    .frames()
                    .first()
                    .ok_or(SemanticActionQualificationError::Contract)?,
            )
            .map_err(SemanticActionQualificationError::Checkpoint)?;
        let attempt = crate::SemanticActionAttemptId::new(attempt)
            .ok_or(SemanticActionQualificationError::Identity)?;
        let active = AgentActiveEffect::for_execution_qualification(&action, attempt);
        let mut coordinator = SemanticActionExecutionCoordinator::new();
        let (reservation, native) =
            coordinator
                .begin(active, &action, requested_at)
                .map_err(|refusal| {
                    SemanticActionQualificationError::NativeAdmission(refusal.error())
                })?;
        Ok(Self {
            coordinator,
            reservation,
            action,
            native: Some(native),
        })
    }

    fn take_native_request(
        &mut self,
    ) -> Result<SemanticActionNativeRequest, SemanticActionQualificationError> {
        self.native
            .take()
            .ok_or(SemanticActionQualificationError::RequestAlreadyTaken)
    }

    fn settle_and_verify(
        mut self,
        settlement: SemanticActionNativeSettlement,
        snapshot: &crate::SemanticSnapshot,
        observed_at: crate::SemanticSettleInstant,
        expected_proof: crate::SemanticEffectProofKind,
    ) -> Result<crate::SemanticActionExecutionApplied, SemanticActionQualificationError> {
        if self.native.is_some() || self.coordinator.status().pending() != 1 {
            return Err(SemanticActionQualificationError::Terminal);
        }
        let outcome = self
            .coordinator
            .settle(self.action.frame(), settlement)
            .map_err(|_| SemanticActionQualificationError::Terminal)?;
        if self.coordinator.status().pending() != 0
            || outcome.active().attempt() != self.reservation.attempt()
        {
            return Err(SemanticActionQualificationError::Terminal);
        }
        let start = crate::begin_semantic_action_settlement(outcome, &self.action)
            .map_err(|_| SemanticActionQualificationError::Settlement)?;
        let mut coordinator = crate::SemanticActionSettlementCoordinator::new();
        let update = coordinator
            .begin(start)
            .map_err(|_| SemanticActionQualificationError::Settlement)?;
        let terminal = match update {
            crate::SemanticActionSettlementUpdate::Pending(reservation) => {
                match coordinator
                    .observe_snapshot(reservation, observed_at, snapshot)
                    .map_err(|_| SemanticActionQualificationError::Settlement)?
                {
                    crate::SemanticActionSettlementUpdate::Terminal(terminal) => terminal,
                    crate::SemanticActionSettlementUpdate::Pending(_) => {
                        return Err(SemanticActionQualificationError::Settlement);
                    }
                }
            }
            crate::SemanticActionSettlementUpdate::Terminal(terminal) => terminal,
        };
        if coordinator.status().pending() != 0 {
            return Err(SemanticActionQualificationError::Settlement);
        }
        let evidence = match expected_proof {
            crate::SemanticEffectProofKind::ExactTargetValue => {
                let (before, after) = self
                    .action
                    .verification_fill_values(snapshot)
                    .map_err(|_| SemanticActionQualificationError::Verification)?;
                crate::SemanticEffectEvidence::exact_target_value(
                    self.reservation.attempt(),
                    observed_at,
                    snapshot,
                    before,
                    after,
                )
            }
            _ => crate::SemanticEffectEvidence::snapshot(
                self.reservation.attempt(),
                observed_at,
                snapshot,
            ),
        };
        let verified = crate::verify_semantic_action_terminal(*terminal, &self.action, evidence)
            .map_err(|_| SemanticActionQualificationError::Verification)?;
        if !verified.verified().matches_action(&self.action)
            || verified.verified().proof() != expected_proof
        {
            return Err(SemanticActionQualificationError::Verification);
        }
        Ok(verified.execution())
    }
}

#[cfg(feature = "probe-harness")]
impl SemanticClickQualificationExecution {
    /// Binds one observed opaque button reference through the production core.
    pub fn prepare(
        observation: &crate::SemanticObservation,
        target: crate::SemanticReferenceId,
        batch: u64,
        attempt: u64,
        requested_at: SemanticActionExecutionInstant,
    ) -> Result<Self, SemanticActionQualificationError> {
        let proposal = crate::SemanticActionProposal::try_new(
            crate::SemanticActionIntent::Click { target },
            crate::SemanticEffectClass::LocalWrite,
            crate::SemanticWaitCondition::TargetState {
                state: crate::SemanticState::Expanded,
                present: true,
            },
            crate::SemanticVerification::TargetState {
                state: crate::SemanticState::Expanded,
                present: true,
            },
            crate::SemanticSettleBudget::try_new(1_000)
                .map_err(|_| SemanticActionQualificationError::Contract)?,
        )
        .map_err(|_| SemanticActionQualificationError::Contract)?;
        Ok(Self {
            execution: SemanticActionQualificationExecution::prepare(
                observation,
                proposal,
                batch,
                attempt,
                requested_at,
            )?,
        })
    }

    /// Moves the exact production native request to the platform adapter once.
    pub fn take_native_request(
        &mut self,
    ) -> Result<SemanticActionNativeRequest, SemanticActionQualificationError> {
        self.execution.take_native_request()
    }

    /// Rejoins, settles, and verifies one exact adjacent semantic snapshot.
    pub fn settle_and_verify(
        self,
        settlement: SemanticActionNativeSettlement,
        snapshot: &crate::SemanticSnapshot,
        observed_at: crate::SemanticSettleInstant,
    ) -> Result<crate::SemanticActionExecutionApplied, SemanticActionQualificationError> {
        self.execution.settle_and_verify(
            settlement,
            snapshot,
            observed_at,
            crate::SemanticEffectProofKind::TargetState,
        )
    }
}

#[cfg(feature = "probe-harness")]
impl SemanticModelClickQualificationExecution {
    /// Binds exactly one decoded model click proposal to one observation.
    ///
    /// The model's opaque reference and declared target-state contract travel
    /// unchanged into the production binding core. Anything other than one
    /// local click with target-state verification is refused before native
    /// request construction.
    pub fn prepare(
        observation: &crate::SemanticObservation,
        proposal: crate::SemanticActionProposal,
        batch: u64,
        attempt: u64,
        requested_at: SemanticActionExecutionInstant,
    ) -> Result<Self, SemanticActionQualificationError> {
        if !matches!(proposal.intent(), crate::SemanticActionIntent::Click { .. })
            || proposal.effect() != crate::SemanticEffectClass::LocalWrite
            || !matches!(
                proposal.verification(),
                crate::SemanticVerification::TargetState { .. }
            )
        {
            return Err(SemanticActionQualificationError::Contract);
        }
        Ok(Self {
            execution: SemanticActionQualificationExecution::prepare(
                observation,
                proposal,
                batch,
                attempt,
                requested_at,
            )?,
        })
    }

    /// Moves the exact production native request to the platform adapter once.
    pub fn take_native_request(
        &mut self,
    ) -> Result<SemanticActionNativeRequest, SemanticActionQualificationError> {
        self.execution.take_native_request()
    }

    /// Rejoins, settles, and verifies one exact fresh semantic snapshot.
    pub fn settle_and_verify(
        self,
        settlement: SemanticActionNativeSettlement,
        snapshot: &crate::SemanticSnapshot,
        observed_at: crate::SemanticSettleInstant,
    ) -> Result<crate::SemanticActionExecutionApplied, SemanticActionQualificationError> {
        self.execution.settle_and_verify(
            settlement,
            snapshot,
            observed_at,
            crate::SemanticEffectProofKind::TargetState,
        )
    }
}

#[cfg(feature = "probe-harness")]
impl SemanticModelActionQualificationExecution {
    /// Binds one decoded model action to the exact observation it references.
    pub fn prepare(
        observation: &crate::SemanticObservation,
        proposal: crate::SemanticActionProposal,
        batch: u64,
        attempt: u64,
        requested_at: SemanticActionExecutionInstant,
    ) -> Result<Self, SemanticActionQualificationError> {
        if proposal.effect() != crate::SemanticEffectClass::LocalWrite
            || !matches!(
                proposal.wait(),
                crate::SemanticWaitCondition::Immediate
                    | crate::SemanticWaitCondition::MutationQuiet(_)
                    | crate::SemanticWaitCondition::TargetState { .. }
            )
        {
            return Err(SemanticActionQualificationError::Contract);
        }
        let expected_proof = match proposal.verification() {
            crate::SemanticVerification::PageDialogOpened => {
                crate::SemanticEffectProofKind::PageDialogOpened
            }
            crate::SemanticVerification::PageDialogClosed => {
                crate::SemanticEffectProofKind::PageDialogClosed
            }
            crate::SemanticVerification::TargetState { .. } => {
                crate::SemanticEffectProofKind::TargetState
            }
            crate::SemanticVerification::TargetValueMatchesInput => {
                crate::SemanticEffectProofKind::ExactTargetValue
            }
            crate::SemanticVerification::TargetValueChanged => {
                crate::SemanticEffectProofKind::TargetValueChanged
            }
            crate::SemanticVerification::TargetSelectionMatchesOption => {
                crate::SemanticEffectProofKind::ExactSelection
            }
            crate::SemanticVerification::TargetSelectionChanged => {
                crate::SemanticEffectProofKind::SelectionChanged
            }
            crate::SemanticVerification::NavigationCommitted
            | crate::SemanticVerification::Dialog(_)
            | crate::SemanticVerification::PageChanged
            | crate::SemanticVerification::ScrollPositionChanged => {
                return Err(SemanticActionQualificationError::Contract);
            }
        };
        Ok(Self {
            execution: SemanticActionQualificationExecution::prepare(
                observation,
                proposal,
                batch,
                attempt,
                requested_at,
            )?,
            expected_proof,
        })
    }

    /// Moves the exact production native request to the platform adapter once.
    pub fn take_native_request(
        &mut self,
    ) -> Result<SemanticActionNativeRequest, SemanticActionQualificationError> {
        self.execution.take_native_request()
    }

    /// Typed settle condition selected by the model and bound by production validation.
    pub const fn wait(&self) -> crate::SemanticWaitCondition {
        self.execution.action.wait()
    }

    /// Hard relative settle budget bound into the prepared action.
    pub const fn settle_budget(&self) -> crate::SemanticSettleBudget {
        self.execution.action.settle_budget()
    }

    /// Rejoins and independently verifies one adjacent semantic snapshot.
    pub fn settle_and_verify(
        self,
        settlement: SemanticActionNativeSettlement,
        snapshot: &crate::SemanticSnapshot,
        observed_at: crate::SemanticSettleInstant,
    ) -> Result<crate::SemanticActionExecutionApplied, SemanticActionQualificationError> {
        self.execution
            .settle_and_verify(settlement, snapshot, observed_at, self.expected_proof)
    }
}

/// Release-excluded owner used to qualify the real semantic fill pipeline.
#[cfg(feature = "probe-harness")]
#[must_use]
pub struct SemanticFillQualificationExecution {
    execution: SemanticActionQualificationExecution,
}

#[cfg(feature = "probe-harness")]
impl SemanticFillQualificationExecution {
    /// Binds one observed opaque text-control reference through the production core.
    pub fn prepare(
        observation: &crate::SemanticObservation,
        target: crate::SemanticReferenceId,
        value: crate::SemanticActionText,
        batch: u64,
        attempt: u64,
        requested_at: SemanticActionExecutionInstant,
    ) -> Result<Self, SemanticActionQualificationError> {
        let proposal = crate::SemanticActionProposal::try_new(
            crate::SemanticActionIntent::Fill { target, value },
            crate::SemanticEffectClass::LocalWrite,
            crate::SemanticWaitCondition::MutationQuiet(
                crate::SemanticMutationQuietPeriod::try_new(100)
                    .map_err(|_| SemanticActionQualificationError::Contract)?,
            ),
            crate::SemanticVerification::TargetValueMatchesInput,
            crate::SemanticSettleBudget::try_new(1_000)
                .map_err(|_| SemanticActionQualificationError::Contract)?,
        )
        .map_err(|_| SemanticActionQualificationError::Contract)?;
        Ok(Self {
            execution: SemanticActionQualificationExecution::prepare(
                observation,
                proposal,
                batch,
                attempt,
                requested_at,
            )?,
        })
    }

    /// Moves the exact production native request to the platform adapter once.
    pub fn take_native_request(
        &mut self,
    ) -> Result<SemanticActionNativeRequest, SemanticActionQualificationError> {
        self.execution.take_native_request()
    }

    /// Rejoins, settles, and verifies one exact adjacent semantic snapshot.
    pub fn settle_and_verify(
        self,
        settlement: SemanticActionNativeSettlement,
        snapshot: &crate::SemanticSnapshot,
        observed_at: crate::SemanticSettleInstant,
    ) -> Result<crate::SemanticActionExecutionApplied, SemanticActionQualificationError> {
        self.execution.settle_and_verify(
            settlement,
            snapshot,
            observed_at,
            crate::SemanticEffectProofKind::ExactTargetValue,
        )
    }
}

/// Content-free refusal from a release-excluded semantic action qualifier.
#[cfg(feature = "probe-harness")]
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionQualificationError {
    /// A nonzero fixed qualification identity was invalid.
    #[error("semantic action qualification identity is invalid")]
    Identity,
    /// The release-excluded qualification contract rejected the proposal shape.
    #[error("semantic action qualification contract failed")]
    Contract,
    /// Production observation binding refused the proposal.
    #[error("semantic action qualification binding failed: {0}")]
    Binding(crate::SemanticActionBindingError),
    /// Production pre-execution checkpointing refused the bound action.
    #[error("semantic action qualification checkpoint failed: {0}")]
    Checkpoint(crate::SemanticActionPreparationError),
    /// Production native admission refused the prepared action.
    #[error("semantic action qualification native admission failed: {0}")]
    NativeAdmission(SemanticActionExecutionCoordinatorError),
    /// The move-only native request was requested twice.
    #[error("semantic action qualification request was already taken")]
    RequestAlreadyTaken,
    /// The exact production terminal failed or did not rejoin.
    #[error("semantic action qualification terminal failed")]
    Terminal,
    /// Production settlement refused the adjacent post-action snapshot.
    #[error("semantic action qualification settlement failed")]
    Settlement,
    /// Independent production postcondition verification failed.
    #[error("semantic action qualification verification failed")]
    Verification,
}

#[cfg(feature = "probe-harness")]
impl SemanticActionQualificationError {
    /// Stable content-free reason code for release-excluded qualification evidence.
    pub const fn diagnostic_code(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Contract => "contract",
            Self::Binding(error) => match error {
                crate::SemanticActionBindingError::Empty => "binding_empty",
                crate::SemanticActionBindingError::ActionLimit => "binding_action_limit",
                crate::SemanticActionBindingError::CurrentFrameCohort => {
                    "binding_current_frame_cohort"
                }
                crate::SemanticActionBindingError::CurrentFrameMissing => {
                    "binding_current_frame_missing"
                }
                crate::SemanticActionBindingError::Reference(
                    crate::SemanticReferenceError::Unknown,
                ) => "binding_reference_unknown",
                crate::SemanticActionBindingError::Reference(
                    crate::SemanticReferenceError::Stale,
                ) => "binding_reference_stale",
                crate::SemanticActionBindingError::Reference(
                    crate::SemanticReferenceError::OperationDenied,
                ) => "binding_reference_operation_denied",
                crate::SemanticActionBindingError::CredentialBoundary => {
                    "binding_credential_boundary"
                }
                crate::SemanticActionBindingError::SelectionTarget => "binding_selection_target",
                crate::SemanticActionBindingError::OutcomeAlreadySatisfied => {
                    "binding_outcome_already_satisfied"
                }
                crate::SemanticActionBindingError::OutcomeContract => "binding_outcome_contract",
                crate::SemanticActionBindingError::TaskEffectMismatch(_) => {
                    "binding_task_effect_mismatch"
                }
                crate::SemanticActionBindingError::TargetIncomplete => "binding_target_incomplete",
                crate::SemanticActionBindingError::TextLimit => "binding_text_limit",
                crate::SemanticActionBindingError::SettleLimit => "binding_settle_limit",
                crate::SemanticActionBindingError::MixedEffectBoundary => {
                    "binding_mixed_effect_boundary"
                }
                crate::SemanticActionBindingError::EffectBatchLimit => "binding_effect_batch_limit",
                crate::SemanticActionBindingError::UnsupportedVerification => {
                    "binding_unsupported_verification"
                }
                crate::SemanticActionBindingError::AssignmentDenied => "binding_assignment_denied",
                crate::SemanticActionBindingError::BudgetExhausted => "binding_budget_exhausted",
                crate::SemanticActionBindingError::DispatchRejected => "binding_dispatch_rejected",
                crate::SemanticActionBindingError::Unverified => "binding_unverified",
                crate::SemanticActionBindingError::TargetCovered => "binding_target_covered",
            },
            Self::Checkpoint(error) => match error {
                crate::SemanticActionPreparationError::IncompleteSnapshot => {
                    "checkpoint_incomplete_snapshot"
                }
                crate::SemanticActionPreparationError::Revalidation(error) => match error {
                    crate::SemanticActionRevalidationError::StaleAuthority => {
                        "checkpoint_stale_authority"
                    }
                    crate::SemanticActionRevalidationError::TargetMissing => {
                        "checkpoint_target_missing"
                    }
                    crate::SemanticActionRevalidationError::TargetChanged => {
                        "checkpoint_target_changed"
                    }
                    crate::SemanticActionRevalidationError::OperationDenied => {
                        "checkpoint_operation_denied"
                    }
                    crate::SemanticActionRevalidationError::TargetDisabled => {
                        "checkpoint_target_disabled"
                    }
                    crate::SemanticActionRevalidationError::CredentialBoundary => {
                        "checkpoint_credential_boundary"
                    }
                    crate::SemanticActionRevalidationError::SelectionTarget => {
                        "checkpoint_selection_target"
                    }
                },
                crate::SemanticActionPreparationError::OutcomeAlreadySatisfied => {
                    "checkpoint_outcome_already_satisfied"
                }
            },
            Self::NativeAdmission(error) => match error {
                SemanticActionExecutionCoordinatorError::Preparation(error) => match error {
                    crate::SemanticActionExecutionPreparationError::AuthorityMismatch => {
                        "native_admission_authority_mismatch"
                    }
                    crate::SemanticActionExecutionPreparationError::InvalidGeometry => {
                        "native_admission_invalid_geometry"
                    }
                    crate::SemanticActionExecutionPreparationError::DeadlineOverflow => {
                        "native_admission_deadline_overflow"
                    }
                    crate::SemanticActionExecutionPreparationError::Invariant => {
                        "native_admission_invariant"
                    }
                },
                SemanticActionExecutionCoordinatorError::DuplicateRequest => {
                    "native_admission_duplicate_request"
                }
                SemanticActionExecutionCoordinatorError::ContextBusy => {
                    "native_admission_context_busy"
                }
                SemanticActionExecutionCoordinatorError::Capacity => "native_admission_capacity",
                SemanticActionExecutionCoordinatorError::Shutdown => "native_admission_shutdown",
                SemanticActionExecutionCoordinatorError::UnknownRequest => {
                    "native_admission_unknown_request"
                }
                SemanticActionExecutionCoordinatorError::RequestMismatch => {
                    "native_admission_request_mismatch"
                }
                SemanticActionExecutionCoordinatorError::PrematureTimeout => {
                    "native_admission_premature_timeout"
                }
            },
            Self::RequestAlreadyTaken => "request_already_taken",
            Self::Terminal => "terminal",
            Self::Settlement => "settlement",
            Self::Verification => "verification",
        }
    }
}

/// Compatibility name for the click qualifier's shared refusal vocabulary.
#[cfg(feature = "probe-harness")]
pub type SemanticClickQualificationError = SemanticActionQualificationError;

/// Compatibility name for the fill qualifier's shared refusal vocabulary.
#[cfg(feature = "probe-harness")]
pub type SemanticFillQualificationError = SemanticActionQualificationError;

/// Exhaustive fail-closed mapping from the shared native dispatch vocabulary.
pub(crate) const fn semantic_action_dispatch_failure(
    dispatch: ContextDispatch,
) -> Option<SemanticActionFailure> {
    match dispatch {
        ContextDispatch::Scheduled => None,
        ContextDispatch::Unsupported
        | ContextDispatch::Rejected(ContextPortFailure::Unsupported) => {
            Some(SemanticActionFailure::UnsupportedInteraction)
        }
        ContextDispatch::Rejected(ContextPortFailure::ResourceExhausted) => {
            Some(SemanticActionFailure::ResourceExhausted)
        }
        ContextDispatch::Rejected(ContextPortFailure::Cancelled) => {
            Some(SemanticActionFailure::Cancelled)
        }
        ContextDispatch::Rejected(ContextPortFailure::TimedOut) => {
            Some(SemanticActionFailure::Timeout)
        }
        ContextDispatch::Rejected(ContextPortFailure::Stale) => {
            Some(SemanticActionFailure::StaleReference)
        }
        ContextDispatch::Rejected(
            ContextPortFailure::ProfileUnavailable
            | ContextPortFailure::ProfileBusy
            | ContextPortFailure::ExtensionIsolationUnproven
            | ContextPortFailure::CookieTransferFailed
            | ContextPortFailure::NativeRefused
            | ContextPortFailure::Shutdown,
        ) => Some(SemanticActionFailure::BackendRefused),
    }
}

impl fmt::Debug for SemanticActionExecutionCoordinator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionExecutionCoordinator")
            .field("status", &self.status())
            .finish()
    }
}

/// Admission refusal that never loses dispatched policy authority.
#[must_use]
pub struct SemanticActionExecutionCoordinatorRefusal {
    active: Box<AgentActiveEffect>,
    error: SemanticActionExecutionCoordinatorError,
}

impl SemanticActionExecutionCoordinatorRefusal {
    fn new(active: AgentActiveEffect, error: SemanticActionExecutionCoordinatorError) -> Self {
        Self {
            active: Box::new(active),
            error,
        }
    }

    /// Closed admission failure.
    pub const fn error(&self) -> SemanticActionExecutionCoordinatorError {
        self.error
    }

    /// Closed action failure derived from this exact consuming refusal.
    pub const fn action_failure(&self) -> SemanticActionFailure {
        match self.error {
            SemanticActionExecutionCoordinatorError::Preparation(error) => error.action_failure(),
            SemanticActionExecutionCoordinatorError::ContextBusy
            | SemanticActionExecutionCoordinatorError::Capacity => {
                SemanticActionFailure::ResourceExhausted
            }
            SemanticActionExecutionCoordinatorError::DuplicateRequest
            | SemanticActionExecutionCoordinatorError::Shutdown
            | SemanticActionExecutionCoordinatorError::UnknownRequest
            | SemanticActionExecutionCoordinatorError::RequestMismatch
            | SemanticActionExecutionCoordinatorError::PrematureTimeout => {
                SemanticActionFailure::BackendRefused
            }
        }
    }

    /// Recovers dispatched authority for one terminal policy failure.
    pub(crate) fn into_parts(self) -> (AgentActiveEffect, SemanticActionExecutionCoordinatorError) {
        (*self.active, self.error)
    }
}

impl fmt::Debug for SemanticActionExecutionCoordinatorRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionExecutionCoordinatorRefusal")
            .field("active", &"[redacted]")
            .field("error", &self.error)
            .finish()
    }
}

/// Closed coordinator refusal or terminal-correlation error.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionExecutionCoordinatorError {
    /// Preparation failed before native admission.
    #[error(transparent)]
    Preparation(SemanticActionExecutionPreparationError),
    /// A pending effect or attempt identity was reused.
    #[error("semantic action native request identity is already pending")]
    DuplicateRequest,
    /// The same logical context already has native action debt.
    #[error("semantic action context already has a pending native request")]
    ContextBusy,
    /// Process-wide native action capacity is exhausted.
    #[error("semantic action native execution capacity is exhausted")]
    Capacity,
    /// Shutdown permanently sealed new admission.
    #[error("semantic action native coordinator is shutting down")]
    Shutdown,
    /// No pending reservation matched the terminal identity.
    #[error("semantic action native request is not pending")]
    UnknownRequest,
    /// A terminal or reservation did not match retained exact authority.
    #[error("semantic action native reservation mismatched")]
    RequestMismatch,
    /// The shell attempted to expire work before its one deadline.
    #[error("semantic action native timeout was premature")]
    PrematureTimeout,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, FrameId, SemanticActionBatch, SemanticActionBatchId,
        SemanticActionIntent, SemanticActionProposal, SemanticEffectClass, SemanticFrameTrust,
        SemanticInvocationId, SemanticObservation, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticObservationId, SemanticOrigin, SemanticReferenceId,
        SemanticSettleBudget, SemanticSnapshotGeneration, SemanticState, SemanticVerification,
        SemanticWaitCondition, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn button_observation(seed: u128, geometry: bool) -> SemanticObservation {
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
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://coordinator-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let button = if geometry {
            json!({
                "k": 2, "p": 0, "r": "button", "n": "Private coordinator action", "o": 9,
                "b": {"x": 10, "y": 20, "w": 100, "h": 30}
            })
        } else {
            json!({
                "k": 2, "p": 0, "r": "button", "n": "Private coordinator action", "o": 9
            })
        };
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 7,
            "g": 9,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                button
            ],
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            crate::SemanticDecodeContext::new(
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

    fn click_action(observation: &SemanticObservation, batch: u64) -> SemanticPreparedAction {
        let proposal = SemanticActionProposal::try_new(
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(2).expect("target"),
            },
            SemanticEffectClass::Read,
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            SemanticSettleBudget::try_new(250).expect("settle budget"),
        )
        .expect("proposal");
        SemanticActionBatch::bind(
            SemanticActionBatchId::new(batch).expect("batch"),
            observation,
            &[observation.frames()[0].frame().clone()],
            vec![proposal],
        )
        .expect("bind")
        .actions()[0]
            .prepare(&observation.frames()[0])
            .expect("prepare")
    }

    fn active(action: &SemanticPreparedAction, attempt: u64) -> AgentActiveEffect {
        AgentActiveEffect::for_execution_qualification(
            action,
            crate::SemanticActionAttemptId::new(attempt).expect("attempt"),
        )
    }

    fn begin(
        coordinator: &mut SemanticActionExecutionCoordinator,
        action: &SemanticPreparedAction,
        attempt: u64,
    ) -> (
        SemanticActionExecutionReservation,
        SemanticActionNativeRequest,
    ) {
        coordinator
            .begin(
                active(action, attempt),
                action,
                SemanticActionExecutionInstant::from_millis(1_000),
            )
            .expect("coordinator begin")
    }

    #[test]
    fn exact_terminals_release_once_and_deadlines_are_shell_driven() {
        let observation = button_observation(10, true);
        let action = click_action(&observation, 1);
        let mut coordinator = SemanticActionExecutionCoordinator::new();
        assert_eq!(coordinator.status().pending(), 0);
        assert!(!coordinator.status().sealed());

        let (reservation, native) = begin(&mut coordinator, &action, 11);
        assert_eq!(reservation.attempt().get(), 11);
        assert_eq!(reservation.deadline().millis(), 1_250);
        assert_eq!(reservation.deadline(), native.deadline());
        assert_eq!(coordinator.status().pending(), 1);
        assert_eq!(
            coordinator
                .expire(
                    &reservation,
                    SemanticActionExecutionInstant::from_millis(1_249),
                )
                .expect_err("premature timeout"),
            SemanticActionExecutionCoordinatorError::PrematureTimeout
        );
        let settlement = native.fail(
            crate::SemanticActionNativeFailure::TargetOccluded,
            SemanticActionExecutionInstant::from_millis(1_010),
        );
        let outcome = coordinator
            .settle(action.frame(), settlement)
            .expect("settlement");
        assert_eq!(
            outcome.disposition(),
            crate::SemanticActionExecutionDisposition::Failed(
                SemanticActionFailure::TargetOccluded
            )
        );
        assert_eq!(coordinator.status().pending(), 0);
        assert_eq!(
            coordinator
                .refuse(&reservation, SemanticActionFailure::Cancelled)
                .expect_err("reservation replay"),
            SemanticActionExecutionCoordinatorError::UnknownRequest
        );

        let (reservation, native) = begin(&mut coordinator, &action, 12);
        drop(native);
        let outcome = coordinator
            .refuse(&reservation, SemanticActionFailure::Cancelled)
            .expect("synchronous refusal");
        assert_eq!(
            outcome.disposition(),
            crate::SemanticActionExecutionDisposition::Failed(SemanticActionFailure::Cancelled)
        );

        let (reservation, native) = begin(&mut coordinator, &action, 13);
        drop(native);
        let outcome = coordinator
            .expire(
                &reservation,
                SemanticActionExecutionInstant::from_millis(1_250),
            )
            .expect("deadline");
        assert_eq!(
            outcome.disposition(),
            crate::SemanticActionExecutionDisposition::Failed(SemanticActionFailure::Timeout)
        );
    }

    #[test]
    fn synchronous_dispatch_mapping_is_exhaustive_and_releases_only_refusals() {
        let mappings = [
            (ContextDispatch::Scheduled, None),
            (
                ContextDispatch::Unsupported,
                Some(SemanticActionFailure::UnsupportedInteraction),
            ),
            (
                ContextDispatch::Rejected(ContextPortFailure::Unsupported),
                Some(SemanticActionFailure::UnsupportedInteraction),
            ),
            (
                ContextDispatch::Rejected(ContextPortFailure::ResourceExhausted),
                Some(SemanticActionFailure::ResourceExhausted),
            ),
            (
                ContextDispatch::Rejected(ContextPortFailure::Cancelled),
                Some(SemanticActionFailure::Cancelled),
            ),
            (
                ContextDispatch::Rejected(ContextPortFailure::TimedOut),
                Some(SemanticActionFailure::Timeout),
            ),
            (
                ContextDispatch::Rejected(ContextPortFailure::Stale),
                Some(SemanticActionFailure::StaleReference),
            ),
            (
                ContextDispatch::Rejected(ContextPortFailure::ProfileUnavailable),
                Some(SemanticActionFailure::BackendRefused),
            ),
            (
                ContextDispatch::Rejected(ContextPortFailure::ProfileBusy),
                Some(SemanticActionFailure::BackendRefused),
            ),
            (
                ContextDispatch::Rejected(ContextPortFailure::ExtensionIsolationUnproven),
                Some(SemanticActionFailure::BackendRefused),
            ),
            (
                ContextDispatch::Rejected(ContextPortFailure::CookieTransferFailed),
                Some(SemanticActionFailure::BackendRefused),
            ),
            (
                ContextDispatch::Rejected(ContextPortFailure::NativeRefused),
                Some(SemanticActionFailure::BackendRefused),
            ),
            (
                ContextDispatch::Rejected(ContextPortFailure::Shutdown),
                Some(SemanticActionFailure::BackendRefused),
            ),
        ];
        for (dispatch, expected) in mappings {
            assert_eq!(semantic_action_dispatch_failure(dispatch), expected);
        }

        let observation = button_observation(15, true);
        let action = click_action(&observation, 15);
        let mut coordinator = SemanticActionExecutionCoordinator::new();
        let (reservation, native) = begin(&mut coordinator, &action, 15);
        drop(native);
        assert!(matches!(
            coordinator
                .account_dispatch(&reservation, ContextDispatch::Scheduled)
                .expect("scheduled"),
            SemanticActionExecutionDispatch::Scheduled
        ));
        assert_eq!(coordinator.status().pending(), 1);
        let SemanticActionExecutionDispatch::Refused(outcome) = coordinator
            .account_dispatch(&reservation, ContextDispatch::Unsupported)
            .expect("unsupported")
        else {
            panic!("unsupported dispatch retained native debt");
        };
        assert_eq!(
            outcome.disposition(),
            crate::SemanticActionExecutionDisposition::Failed(
                SemanticActionFailure::UnsupportedInteraction
            )
        );
        assert_eq!(coordinator.status().pending(), 0);
    }

    #[test]
    fn duplicate_context_and_global_capacity_refuse_without_losing_authority() {
        let observation = button_observation(20, true);
        let action = click_action(&observation, 2);
        let mut coordinator = SemanticActionExecutionCoordinator::new();
        let (first, native) = begin(&mut coordinator, &action, 21);
        drop(native);

        let duplicate = coordinator
            .begin(
                active(&action, 21),
                &action,
                SemanticActionExecutionInstant::from_millis(1_000),
            )
            .expect_err("duplicate");
        assert_eq!(
            duplicate.error(),
            SemanticActionExecutionCoordinatorError::DuplicateRequest
        );
        assert_eq!(
            duplicate.action_failure(),
            SemanticActionFailure::BackendRefused
        );
        assert_eq!(duplicate.into_parts().0.attempt().get(), 21);

        let busy = coordinator
            .begin(
                active(&action, 22),
                &action,
                SemanticActionExecutionInstant::from_millis(1_000),
            )
            .expect_err("context busy");
        assert_eq!(
            busy.error(),
            SemanticActionExecutionCoordinatorError::ContextBusy
        );
        assert_eq!(
            busy.action_failure(),
            SemanticActionFailure::ResourceExhausted
        );
        assert_eq!(busy.into_parts().0.attempt().get(), 22);
        let _outcome = coordinator
            .refuse(&first, SemanticActionFailure::Cancelled)
            .expect("release first");

        let mut reservations = Vec::new();
        for offset in 0..MAX_PENDING_SEMANTIC_ACTION_EXECUTIONS {
            let observation = button_observation(100 + offset as u128 * 10, true);
            let action = click_action(&observation, 100 + offset as u64);
            let (reservation, native) = begin(&mut coordinator, &action, 100 + offset as u64);
            drop(native);
            reservations.push(reservation);
        }
        assert_eq!(
            coordinator.status().pending(),
            MAX_PENDING_SEMANTIC_ACTION_EXECUTIONS as u8
        );
        let overflow_observation = button_observation(900, true);
        let overflow_action = click_action(&overflow_observation, 900);
        let overflow = coordinator
            .begin(
                active(&overflow_action, 900),
                &overflow_action,
                SemanticActionExecutionInstant::from_millis(1_000),
            )
            .expect_err("capacity");
        assert_eq!(
            overflow.error(),
            SemanticActionExecutionCoordinatorError::Capacity
        );
        assert_eq!(
            overflow.action_failure(),
            SemanticActionFailure::ResourceExhausted
        );
        assert_eq!(overflow.into_parts().0.attempt().get(), 900);
        for reservation in reservations {
            let _outcome = coordinator
                .refuse(&reservation, SemanticActionFailure::Cancelled)
                .expect("release reservation");
        }
        assert_eq!(coordinator.status().pending(), 0);
    }

    #[test]
    fn malformed_cross_request_terminal_never_releases_retained_debt() {
        let observation = button_observation(30, true);
        let first_action = click_action(&observation, 31);
        let second_action = click_action(&observation, 32);
        let mut first = SemanticActionExecutionCoordinator::new();
        let mut second = SemanticActionExecutionCoordinator::new();
        let (first_reservation, first_native) = begin(&mut first, &first_action, 41);
        let (second_reservation, second_native) = begin(&mut second, &second_action, 41);
        drop(first_native);
        let substituted = second_native.fail(
            crate::SemanticActionNativeFailure::Transport,
            SemanticActionExecutionInstant::from_millis(1_010),
        );
        assert_eq!(
            first
                .settle(first_action.frame(), substituted)
                .expect_err("cross-request terminal"),
            SemanticActionExecutionCoordinatorError::RequestMismatch
        );
        assert_eq!(first.status().pending(), 1);
        assert_eq!(second.status().pending(), 1);
        let _first_outcome = first
            .refuse(&first_reservation, SemanticActionFailure::BackendRefused)
            .expect("first release");
        let _second_outcome = second
            .refuse(&second_reservation, SemanticActionFailure::BackendRefused)
            .expect("second release");
    }

    #[test]
    fn preparation_and_shutdown_refusals_are_recoverable_and_redacted() {
        let missing = button_observation(40, false);
        let missing_action = click_action(&missing, 41);
        let mut coordinator = SemanticActionExecutionCoordinator::new();
        let refusal = coordinator
            .begin(
                active(&missing_action, 51),
                &missing_action,
                SemanticActionExecutionInstant::from_millis(1_000),
            )
            .expect_err("geometry");
        assert_eq!(
            refusal.error(),
            SemanticActionExecutionCoordinatorError::Preparation(
                SemanticActionExecutionPreparationError::InvalidGeometry
            )
        );
        assert_eq!(
            refusal.action_failure(),
            SemanticActionFailure::TargetChanged
        );
        assert_eq!(refusal.into_parts().0.attempt().get(), 51);

        let observation = button_observation(50, true);
        let action = click_action(&observation, 51);
        let (reservation, native) = begin(&mut coordinator, &action, 52);
        drop(native);
        let status = coordinator.seal();
        assert!(status.sealed());
        assert_eq!(status.pending(), 1);
        let refusal = coordinator
            .begin(
                active(&action, 53),
                &action,
                SemanticActionExecutionInstant::from_millis(1_000),
            )
            .expect_err("shutdown");
        assert_eq!(
            refusal.error(),
            SemanticActionExecutionCoordinatorError::Shutdown
        );
        assert_eq!(
            refusal.action_failure(),
            SemanticActionFailure::BackendRefused
        );
        assert_eq!(refusal.into_parts().0.attempt().get(), 53);
        let debug = format!("{coordinator:?} {reservation:?}");
        assert!(!debug.contains("coordinator-private"));
        assert!(!debug.contains("Private coordinator action"));
        assert!(!debug.contains("ProfileId"));
        let _outcome = coordinator
            .refuse(&reservation, SemanticActionFailure::Cancelled)
            .expect("shutdown drain");
        assert_eq!(coordinator.status().pending(), 0);
        assert!(coordinator.status().sealed());
    }
}
