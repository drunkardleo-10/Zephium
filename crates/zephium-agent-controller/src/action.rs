//! Policy-authorized native action continuation shared by production and qualifiers.

use std::fmt;
use zephium_agentic::*;

/// Exact verified browser transition eligible for one provider replay.
#[must_use]
pub struct AgentBrowserVerifiedTransition {
    pub(crate) continuation: AgentProviderContinuation,
    pub(crate) diff: Box<SemanticDiff>,
    pub(crate) terminal: Option<SemanticActionBatchResult>,
}

impl AgentBrowserVerifiedTransition {
    /// Exact policy-accounted native terminal for audit and metric closure.
    /// Diagnostic synthetic bridges cannot supply this production evidence.
    pub fn batch_result(&self) -> Option<&SemanticActionBatchResult> {
        self.terminal.as_ref()
    }

    pub(crate) fn into_parts(self) -> (AgentProviderContinuation, Box<SemanticDiff>) {
        (self.continuation, self.diff)
    }
}

impl fmt::Debug for AgentBrowserVerifiedTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentBrowserVerifiedTransition")
            .field("diff_stats", &self.diff.stats())
            .finish_non_exhaustive()
    }
}

/// Bound action awaiting independently assessed policy authorization.
#[must_use]
pub struct AgentBrowserActionProposal {
    action: SemanticPreparedAction,
    continuation: AgentProviderContinuation,
    batch: SemanticActionBatchExecution,
}

impl AgentBrowserActionProposal {
    pub(crate) fn bind(
        turn: AgentProviderSettledToolTurn,
        observation: &SemanticObservation,
        frames: &[SemanticFrameJoin],
        batch: SemanticActionBatchId,
    ) -> Result<Self, AgentBrowserActionError> {
        let (proposal, continuation) = turn.into_parts();
        let AgentBrowserToolProposal::Act(actions) = proposal else {
            return Err(AgentBrowserActionError::Tool);
        };
        if actions.actions().len() != 1 {
            return Err(AgentBrowserActionError::ActionCount);
        }
        let batch = SemanticActionBatch::bind(batch, observation, frames, actions.into_actions())
            .map_err(AgentBrowserActionError::Binding)?;
        let bound = batch
            .actions()
            .first()
            .ok_or(AgentBrowserActionError::ActionCount)?;
        let snapshot = observation
            .frames()
            .iter()
            .find(|snapshot| snapshot.frame() == bound.frame())
            .ok_or(AgentBrowserActionError::Tool)?;
        let action = bound
            .prepare(snapshot)
            .map_err(AgentBrowserActionError::Checkpoint)?;
        let batch =
            SemanticActionBatchExecution::new(&batch).map_err(AgentBrowserActionError::Batch)?;
        // This vertical admits only independently snapshot-verifiable effects.
        // Native navigation/dialog/scroll evidence needs its own host adapter.
        if matches!(
            action.verification(),
            SemanticVerification::NavigationCommitted
                | SemanticVerification::Dialog(_)
                | SemanticVerification::ScrollPositionChanged
        ) {
            return Err(AgentBrowserActionError::EvidenceRequired);
        }
        if !matches!(
            action.wait(),
            SemanticWaitCondition::Immediate | SemanticWaitCondition::MutationQuiet(_)
        ) {
            return Err(AgentBrowserActionError::EvidenceRequired);
        }
        Ok(Self {
            action,
            continuation,
            batch,
        })
    }

    /// The exact prepared action for a trusted effect classifier.
    pub const fn action(&self) -> &SemanticPreparedAction {
        &self.action
    }

    /// Authorizes the separately assessed action and retains native authority.
    ///
    /// `automation` must be freshly sampled from the owning context registry.
    /// The assessment is a trusted adapter result, never a model declaration.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn authorize(
        self,
        policy: &mut AgentRunPolicy,
        request: AgentEffectRequest,
        assessment: &AgentEffectAssessment,
        dispatch: AgentEffectDispatchRequest,
        requested_at: SemanticActionExecutionInstant,
        execution: &mut SemanticActionExecutionCoordinator,
        mut journal: Option<&mut crate::terra::work::WorkJournal>,
        admission_failure: &mut Option<AgentFailedSemanticEffect>,
        proposal_failure: &mut Option<AgentBrowserActionProposal>,
    ) -> Result<AgentBrowserAction, AgentBrowserActionError> {
        let admission = (|| {
            let permit = match policy
                .authorize_semantic_effect(request, &self.action, assessment)
                .map_err(AgentBrowserActionError::Policy)?
            {
                AgentEffectAuthorization::Permit(permit) => permit,
                AgentEffectAuthorization::NeedsHuman(transition) => {
                    if let Some(journal) = journal.as_mut() {
                        journal
                            .needs_human(transition)
                            .map_err(|_| AgentBrowserActionError::State)?;
                    }
                    return Err(AgentBrowserActionError::NeedsHuman(transition.reason()));
                }
            };
            let active = policy
                .dispatch_semantic_effect(permit, &self.action, dispatch)
                .map_err(AgentBrowserActionError::Policy)?;
            let journal_failed =
                journal.is_some_and(|journal| journal.action_active(&active).is_err());
            let (reservation, native) = match execution.begin(active, &self.action, requested_at) {
                Ok(value) => value,
                Err(refusal) => {
                    let failed = policy
                        .settle_execution_admission_refusal(refusal, &self.action)
                        .map_err(AgentBrowserActionError::Policy)?;
                    let failure = failed.failure();
                    *admission_failure = Some(failed);
                    return Err(AgentBrowserActionError::Failed(failure));
                }
            };
            Ok((reservation, native, journal_failed))
        })();
        let (reservation, native, journal_failed) = match admission {
            Ok(value) => value,
            Err(error) => {
                // Keep the original batch and prepared action alongside any
                // charged failure; recovery must not fabricate a new batch.
                *proposal_failure = Some(self);
                return Err(error);
            }
        };
        Ok(AgentBrowserAction {
            proposal: self,
            reservation,
            native: Some(native),
            terminal: None,
            pending: None,
            finished: false,
            receipt: None,
            failed: None,
            journal_failed,
        })
    }
}

impl fmt::Debug for AgentBrowserActionProposal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentBrowserActionProposal([redacted])")
    }
}

/// One live, policy-authorized native action retained until exact settlement.
///
/// On refusal this owner stays in the session, along with its policy. A host
/// cannot turn dropped or malformed native work into successful continuation.
#[must_use]
pub struct AgentBrowserAction {
    journal_failed: bool,
    proposal: AgentBrowserActionProposal,
    reservation: SemanticActionExecutionReservation,
    native: Option<SemanticActionNativeRequest>,
    terminal: Option<Box<SemanticActionSettlementTerminal>>,
    pending: Option<SemanticActionSettlementReservation>,
    finished: bool,
    receipt: Option<AgentEffectReceipt>,
    failed: Option<AgentFailedSemanticEffect>,
}

impl AgentBrowserAction {
    #[cfg(all(test, feature = "probe-harness"))]
    pub(crate) fn retained_failure(&self) -> Option<&AgentFailedSemanticEffect> {
        self.failed.as_ref()
    }

    pub(crate) fn account_dispatch(
        &mut self,
        dispatch: ContextDispatch,
        policy: &mut AgentRunPolicy,
        execution: &mut SemanticActionExecutionCoordinator,
    ) -> Result<(), AgentBrowserActionError> {
        match execution
            .account_dispatch(&self.reservation, dispatch)
            .map_err(AgentBrowserActionError::Native)?
        {
            SemanticActionExecutionDispatch::Scheduled => Ok(()),
            SemanticActionExecutionDispatch::Refused(outcome) => {
                let refusal =
                    match begin_semantic_action_settlement(*outcome, &self.proposal.action) {
                        Err(refusal) => refusal,
                        Ok(_) => return Err(AgentBrowserActionError::State),
                    };
                let failed = policy
                    .settle_settlement_start_refusal(refusal, &self.proposal.action)
                    .map_err(AgentBrowserActionError::Policy)?;
                Err(self.retain_failure(failed))
            }
        }
    }

    fn retain_failure(&mut self, failed: AgentFailedSemanticEffect) -> AgentBrowserActionError {
        let failure = failed.failure();
        self.finished = true;
        self.receipt = Some(failed.receipt());
        self.failed = Some(failed);
        AgentBrowserActionError::Failed(failure)
    }
    pub(crate) const fn journal_failed(&self) -> bool {
        self.journal_failed
    }

    pub(crate) fn accepts_settlement(
        &self,
        execution: &SemanticActionExecutionCoordinator,
        terminal: &SemanticActionNativeSettlement,
    ) -> bool {
        execution.accepts_settlement(&self.reservation, terminal)
    }
    /// Moves the exact bounded request to an existing native browser port.
    pub fn take_native_request(
        &mut self,
    ) -> Result<SemanticActionNativeRequest, AgentBrowserActionError> {
        self.native.take().ok_or(AgentBrowserActionError::State)
    }

    /// Routes through the production runtime browser without exposing native handles.
    pub fn dispatch(
        &mut self,
        browser: &zephium_agent_runtime::AgentRuntimeBrowser,
        completion: SemanticActionNativeCompletion,
    ) -> Result<ContextDispatch, AgentBrowserActionError> {
        let request = self.take_native_request()?;
        Ok(browser.execute_semantic_action(request, completion))
    }

    /// Settles the exact native terminal and independently verifies fresh state.
    pub(crate) fn settle(
        &mut self,
        policy: &mut AgentRunPolicy,
        native: SemanticActionNativeSettlement,
        current: &SemanticObservation,
        observed_at: SemanticSettleInstant,
        execution: &mut SemanticActionExecutionCoordinator,
        settlement: &mut SemanticActionSettlementCoordinator,
    ) -> Result<AgentVerifiedSemanticEffect, AgentBrowserActionError> {
        self.begin_settlement(policy, native, execution, settlement)?;
        if let Some(pending) = self.pending.take() {
            let snapshot = current
                .frames()
                .iter()
                .find(|snapshot| snapshot.frame() == self.proposal.action.frame())
                .ok_or(AgentBrowserActionError::State)?;
            match settlement.observe_snapshot(pending, observed_at, snapshot) {
                Ok(update) => {
                    self.retain_update(update);
                }
                Err(refusal) => {
                    self.pending = Some(refusal.into_parts().0);
                    return Err(AgentBrowserActionError::Settlement);
                }
            }
        }
        self.verify_settlement(policy, current, observed_at)
    }

    pub(crate) fn begin_settlement(
        &mut self,
        policy: &mut AgentRunPolicy,
        native: SemanticActionNativeSettlement,
        execution: &mut SemanticActionExecutionCoordinator,
        settlement: &mut SemanticActionSettlementCoordinator,
    ) -> Result<Option<SemanticSettleInstant>, AgentBrowserActionError> {
        if self.native.is_some()
            || self.finished
            || self.pending.is_some()
            || self.terminal.is_some()
        {
            return Err(AgentBrowserActionError::State);
        }
        let outcome = execution
            .settle(self.proposal.action.frame(), native)
            .map_err(AgentBrowserActionError::Native)?;
        let start = match begin_semantic_action_settlement(outcome, &self.proposal.action) {
            Ok(start) => start,
            Err(refusal) => {
                let failed = policy
                    .settle_settlement_start_refusal(refusal, &self.proposal.action)
                    .map_err(AgentBrowserActionError::Policy)?;
                return Err(self.retain_failure(failed));
            }
        };
        let update = match settlement.begin(start) {
            Ok(update) => update,
            Err(refusal) => {
                let failed = policy
                    .settle_settlement_admission_refusal(refusal, &self.proposal.action)
                    .map_err(AgentBrowserActionError::Policy)?;
                return Err(self.retain_failure(failed));
            }
        };
        Ok(self.retain_update(update))
    }

    fn retain_update(
        &mut self,
        update: SemanticActionSettlementUpdate,
    ) -> Option<SemanticSettleInstant> {
        match update {
            SemanticActionSettlementUpdate::Terminal(terminal) => {
                self.terminal = Some(terminal);
                None
            }
            SemanticActionSettlementUpdate::Pending(pending) => {
                let wake = pending.next_wake();
                self.pending = Some(pending);
                Some(wake)
            }
        }
    }

    pub(crate) fn wake_settlement(
        &mut self,
        observed_at: SemanticSettleInstant,
        settlement: &mut SemanticActionSettlementCoordinator,
    ) -> Result<Option<SemanticSettleInstant>, AgentBrowserActionError> {
        let pending = self.pending.take().ok_or(AgentBrowserActionError::State)?;
        match settlement.wake(pending, observed_at) {
            Ok(update) => Ok(self.retain_update(update)),
            Err(refusal) => {
                self.pending = Some(refusal.into_parts().0);
                Err(AgentBrowserActionError::Settlement)
            }
        }
    }

    pub(crate) fn verify_settlement(
        &mut self,
        policy: &mut AgentRunPolicy,
        current: &SemanticObservation,
        observed_at: SemanticSettleInstant,
    ) -> Result<AgentVerifiedSemanticEffect, AgentBrowserActionError> {
        if self.pending.is_some() {
            return Err(AgentBrowserActionError::SettlementPending);
        }
        let snapshot = current
            .frames()
            .iter()
            .find(|snapshot| snapshot.frame() == self.proposal.action.frame())
            .ok_or(AgentBrowserActionError::State)?;
        self.verify_terminal(policy, snapshot, observed_at)
    }

    fn verify_terminal(
        &mut self,
        policy: &mut AgentRunPolicy,
        snapshot: &SemanticSnapshot,
        observed_at: SemanticSettleInstant,
    ) -> Result<AgentVerifiedSemanticEffect, AgentBrowserActionError> {
        let evidence = prepare_semantic_action_snapshot_evidence(
            &self.proposal.action,
            self.reservation.attempt(),
            observed_at,
            snapshot,
        )
        .map_err(|_| AgentBrowserActionError::EvidenceRequired)?;
        let terminal = self.terminal.take().ok_or(AgentBrowserActionError::State)?;
        let verified =
            match verify_semantic_action_terminal(*terminal, &self.proposal.action, evidence) {
                Ok(verified) => verified,
                Err(refusal) => {
                    let failed = policy
                        .settle_refused_semantic_terminal(refusal)
                        .map_err(AgentBrowserActionError::Policy)?;
                    return Err(self.retain_failure(failed));
                }
            };
        self.finished = true;
        policy
            .settle_verified_semantic_terminal(verified, &self.proposal.action)
            .map_err(AgentBrowserActionError::Policy)
    }

    pub(crate) fn into_transition(
        mut self,
        accounted: AgentVerifiedSemanticEffect,
        baseline: &SemanticObservation,
        current: &SemanticObservation,
        observed_at: SemanticSettleInstant,
    ) -> Result<AgentBrowserVerifiedTransition, AgentBrowserActionFinalizationRefusal> {
        let result = finalize_accounted_semantic_action_result(
            &self.proposal.action,
            accounted,
            baseline,
            self.proposal.continuation.baseline(),
            SemanticPostActionObservation::new(observed_at, current.clone()),
            SemanticDiffBudget::ACTION,
        )
        .map_err(|refusal| {
            AgentBrowserActionFinalizationRefusal::Finalization(Box::new(refusal))
        })?;
        // The finalizer already enforces the complete baseline/proof/current join.
        // A fallback remains a typed stop; it cannot be replayed as an action diff.
        let Some(diff) = result.result().diff().cloned() else {
            return Err(AgentBrowserActionFinalizationRefusal::FreshSnapshot(
                Box::new(result),
            ));
        };
        self.proposal
            .batch
            .record_success(&self.proposal.action, result)
            .map_err(|refusal| {
                AgentBrowserActionFinalizationRefusal::BatchAdmission(Box::new(refusal))
            })?;
        let terminal = self
            .proposal
            .batch
            .finish()
            .map_err(AgentBrowserActionFinalizationRefusal::Batch)?;
        Ok(AgentBrowserVerifiedTransition {
            continuation: self.proposal.continuation,
            diff: Box::new(diff),
            terminal: Some(terminal),
        })
    }
}

impl fmt::Debug for AgentBrowserAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentBrowserAction")
            .field("native_requested", &self.native.is_none())
            .field("settlement_pending", &self.pending.is_some())
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

/// Closed, content-free action refusal. No variant authorizes retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentBrowserActionError {
    /// Tool is outside this vertical.
    Tool,
    /// This vertical accepts exactly one action per tool turn.
    ActionCount,
    /// Exact observed references failed binding.
    Binding(SemanticActionBindingError),
    /// Fresh action checkpoint failed.
    Checkpoint(SemanticActionPreparationError),
    /// The host must supply a distinct non-snapshot evidence class.
    EvidenceRequired,
    /// A policy decision refused the action.
    Policy(AgentPolicyError),
    /// Explicit human control or authorization is required.
    NeedsHuman(AgentNeedsHumanReason),
    /// An exact native terminal failed to rejoin.
    Native(SemanticActionExecutionCoordinatorError),
    /// Settlement rejected its event or snapshot.
    Settlement,
    /// The declared settlement condition is still pending.
    SettlementPending,
    /// One effect was charged as a typed failure.
    Failed(SemanticActionFailure),
    /// State or callback replay was rejected.
    State,
    /// Exact accounted batch admission refused.
    Batch(SemanticActionBatchExecutionError),
}

/// Lossless state-update refusal after an effect was already accounted.
#[derive(Debug)]
#[must_use]
pub enum AgentBrowserActionFinalizationRefusal {
    /// The full charged owner and observation remain available for reconciliation.
    Finalization(Box<AgentAccountedSemanticActionResultRefusal>),
    /// This exact action requires a new full-observation turn.
    FreshSnapshot(Box<AgentAccountedSemanticActionResult>),
    /// Complete accounted result remains owned after batch correlation refusal.
    BatchAdmission(Box<SemanticActionBatchAdmissionRefusal>),
    /// Closed batch invariant failure; no successful continuation exists.
    Batch(SemanticActionBatchExecutionError),
}
