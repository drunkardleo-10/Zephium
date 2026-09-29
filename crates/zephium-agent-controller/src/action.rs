//! Policy-authorized native action continuation shared by production and qualifiers.

use std::fmt;
use zephium_agentic::*;

/// Exact verified browser transition eligible for one provider replay.
#[must_use]
pub struct AgentBrowserVerifiedTransition {
    pub(crate) continuation: Option<AgentProviderContinuation>,
    #[cfg(feature = "probe-harness")]
    pub(crate) probe_diff: Option<Box<SemanticDiff>>,
    pub(crate) terminal: Option<SemanticActionBatchResult>,
}

pub(crate) enum AgentBrowserVerifiedState {
    Accounted(Box<SemanticActionResult>),
    #[cfg(feature = "probe-harness")]
    ProbeDiff(Box<SemanticDiff>),
}

impl AgentBrowserVerifiedState {
    pub(crate) fn diff(&self) -> Option<&SemanticDiff> {
        match self {
            Self::Accounted(result) => result.diff(),
            #[cfg(feature = "probe-harness")]
            Self::ProbeDiff(diff) => Some(diff),
        }
    }

    pub(crate) fn action_result(&self) -> Option<&SemanticActionResult> {
        match self {
            Self::Accounted(result) => Some(result),
            #[cfg(feature = "probe-harness")]
            Self::ProbeDiff(_) => None,
        }
    }
}

impl AgentBrowserVerifiedTransition {
    /// Exact policy-accounted native terminal for audit and metric closure.
    /// Diagnostic synthetic bridges cannot supply this production evidence.
    pub fn batch_result(&self) -> Option<&SemanticActionBatchResult> {
        self.terminal.as_ref()
    }

    pub(crate) fn into_parts(
        self,
    ) -> Option<(AgentProviderContinuation, AgentBrowserVerifiedState)> {
        #[cfg(feature = "probe-harness")]
        if let Some(diff) = self.probe_diff {
            return Some((
                self.continuation?,
                AgentBrowserVerifiedState::ProbeDiff(diff),
            ));
        }
        Some((
            self.continuation?,
            AgentBrowserVerifiedState::Accounted(Box::new(self.terminal?.into_final_state()?)),
        ))
    }
}

impl fmt::Debug for AgentBrowserVerifiedTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentBrowserVerifiedTransition")
            .field(
                "next_state",
                &self
                    .terminal
                    .as_ref()
                    .and_then(|terminal| terminal.final_state())
                    .map(SemanticActionResult::next_state),
            )
            .finish_non_exhaustive()
    }
}

/// Bound action awaiting independently assessed policy authorization.
#[must_use]
pub struct AgentBrowserActionProposal {
    action: SemanticPreparedAction,
    baseline: SemanticObservationAcknowledgement,
    continuation: Option<AgentProviderContinuation>,
    batch: SemanticActionBatchExecution,
    refusal_context: Option<AgentProviderActionRefusalContext>,
}

pub(crate) enum AgentBrowserActionBinding {
    Prepared(Box<AgentBrowserActionProposal>),
    Refused(Box<AgentProviderActionRefusal>),
}

impl AgentBrowserActionProposal {
    /// Declines a bound action before any permit: the continuation carries
    /// the refusal back to the model and nothing was issued.
    pub(crate) fn into_refusal(
        self,
        error: SemanticActionBindingError,
    ) -> Option<AgentProviderActionRefusal> {
        Some(AgentProviderActionRefusal::unissued(self.continuation?, error, self.refusal_context))
    }
}

/// Original refused proposal, including its non-replayable continuation. Only
/// the exact policy branch before permit/dispatch can mark it unissued.
pub(crate) struct AgentBrowserActionProposalRefusal {
    proposal: AgentBrowserActionProposal,
    human_review: Option<AgentNeedsHumanTransition>,
}

impl AgentBrowserActionProposalRefusal {
    pub(crate) const fn human_review(&self) -> Option<AgentNeedsHumanTransition> {
        self.human_review
    }

    pub(crate) fn discard_after_closure(self) -> Option<AgentNeedsHumanTransition> {
        drop(self.proposal);
        self.human_review
    }
}

impl AgentBrowserActionProposal {
    pub(crate) fn bind(
        turn: AgentProviderSettledToolTurn,
        observation: &SemanticObservation,
        frames: &[SemanticFrameJoin],
        batch: SemanticActionBatchId,
        config: &AgentProviderCallConfig,
    ) -> Result<AgentBrowserActionBinding, AgentBrowserActionError> {
        let AgentBrowserToolProposal::Act(actions) = turn.proposal() else {
            return Err(AgentBrowserActionError::Tool);
        };
        if actions.actions().len() != 1 {
            return Err(AgentBrowserActionError::ActionCount);
        }
        let (batch, continuation, context) =
            match turn.resolve_action(batch, observation, frames, config) {
                Ok(AgentProviderActionResolution::Bound(batch, continuation, context)) => {
                    (batch, continuation, context)
                }
                Ok(AgentProviderActionResolution::Refused(refusal)) => {
                    return Ok(AgentBrowserActionBinding::Refused(Box::new(refusal)));
                }
                Err(AgentProviderActionResolutionError::Binding(error)) => {
                    return Err(AgentBrowserActionError::Binding(error));
                }
                Err(AgentProviderActionResolutionError::Continuation(_)) => {
                    return Err(AgentBrowserActionError::State);
                }
            };
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
        // This vertical admits only independently snapshot-verifiable effects.
        // Native navigation/dialog evidence needs its own host adapter; the
        // model is told so and chooses again, nothing having been issued.
        let unsupported = !AGENT_BROWSER_SNAPSHOT_ACTION_KINDS.contains(&action.kind())
            || matches!(
                action.verification(),
                SemanticVerification::NavigationCommitted | SemanticVerification::Dialog(_)
            )
            || !matches!(
                action.wait(),
                SemanticWaitCondition::Immediate | SemanticWaitCondition::MutationQuiet(_)
            );
        if unsupported {
            return Ok(AgentBrowserActionBinding::Refused(Box::new(
                AgentProviderActionRefusal::unissued(
                    continuation,
                    SemanticActionBindingError::UnsupportedVerification,
                    context,
                ),
            )));
        }
        let batch =
            SemanticActionBatchExecution::new(&batch).map_err(AgentBrowserActionError::Batch)?;
        // A model cannot shorten the allowance below the current native
        // snapshot capability. Never extend a deadline after dispatch instead.
        if action.settle_budget().millis() < MIN_AGENT_BROWSER_SNAPSHOT_SETTLE_MILLIS {
            return Err(AgentBrowserActionError::SettleBudget);
        }
        Ok(AgentBrowserActionBinding::Prepared(Box::new(Self {
            action,
            baseline: continuation.baseline().clone(),
            continuation: Some(continuation),
            batch,
            refusal_context: context,
        })))
    }

    /// The exact prepared action for a trusted effect classifier.
    pub const fn action(&self) -> &SemanticPreparedAction {
        &self.action
    }

    pub(crate) fn baseline(&self) -> &SemanticObservationAcknowledgement {
        &self.baseline
    }

    pub(crate) fn bind_decision(
        selection: DecisionActionSelection,
        recipe: SemanticActionProposal,
        observation: &SemanticObservation,
        frames: &[SemanticFrameJoin],
        batch: SemanticActionBatchId,
    ) -> Result<Self, AgentBrowserActionError> {
        let (batch, baseline) = selection
            .bind_action(recipe, observation, frames, batch)
            .map_err(|_| AgentBrowserActionError::State)?;
        Self::bind_owned(batch, baseline, observation)
    }

    /// A step Rust chose on the exact observation it read it from (a
    /// consent banner's refusal), with no model or decision in between. It
    /// still needs the task's own assessment before any dispatch.
    pub(crate) fn bind_code_owned(
        recipe: SemanticActionProposal,
        observation: &SemanticObservation,
        frames: &[SemanticFrameJoin],
        batch: SemanticActionBatchId,
    ) -> Result<Self, AgentBrowserActionError> {
        let (baseline, _) = SemanticObservationAcknowledgement::whole_page_scope(observation)
            .ok_or(AgentBrowserActionError::State)?;
        let batch = SemanticActionBatch::bind(batch, observation, frames, vec![recipe])
            .map_err(|_| AgentBrowserActionError::State)?;
        Self::bind_owned(batch, baseline, observation)
    }

    fn bind_owned(
        batch: SemanticActionBatch,
        baseline: SemanticObservationAcknowledgement,
        observation: &SemanticObservation,
    ) -> Result<Self, AgentBrowserActionError> {
        let bound = batch
            .actions()
            .first()
            .ok_or(AgentBrowserActionError::ActionCount)?;
        let snapshot = observation
            .frames()
            .iter()
            .find(|snapshot| snapshot.frame() == bound.frame())
            .ok_or(AgentBrowserActionError::State)?;
        let action = bound
            .prepare(snapshot)
            .map_err(AgentBrowserActionError::Checkpoint)?;
        if !AGENT_BROWSER_SNAPSHOT_ACTION_KINDS.contains(&action.kind())
            || matches!(
                action.verification(),
                SemanticVerification::NavigationCommitted | SemanticVerification::Dialog(_)
            )
            || !matches!(
                action.wait(),
                SemanticWaitCondition::Immediate | SemanticWaitCondition::MutationQuiet(_)
            )
        {
            return Err(AgentBrowserActionError::Binding(
                SemanticActionBindingError::UnsupportedVerification,
            ));
        }
        if action.settle_budget().millis() < MIN_AGENT_BROWSER_SNAPSHOT_SETTLE_MILLIS {
            return Err(AgentBrowserActionError::SettleBudget);
        }
        Ok(Self {
            action,
            baseline,
            continuation: None,
            batch: SemanticActionBatchExecution::new(&batch)
                .map_err(AgentBrowserActionError::Batch)?,
            refusal_context: None,
        })
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
        proposal_failure: &mut Option<AgentBrowserActionProposalRefusal>,
    ) -> Result<AgentBrowserAction, AgentBrowserActionError> {
        let mut human_review = None;
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
                        human_review = Some(transition);
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
                *proposal_failure = Some(AgentBrowserActionProposalRefusal {
                    proposal: self,
                    human_review,
                });
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
            native_unverified: false,
            reinspection_owner: None,
            reinspection_result: None,
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
    native_unverified: bool,
    reinspection_owner: Option<std::sync::Arc<()>>,
    reinspection_result: Option<crate::AgentWorkEffectReobservation>,
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
    pub(crate) fn prepare_reinspection(
        &mut self,
        account: AgentContextAccountBinding,
        resources: &mut WorkBrowserResources,
        lease: &WorkBrowserExecutionLease,
        target: crate::AgentWorkEffectReadTarget,
        now: AgentPolicyInstant,
    ) -> Result<
        (
            crate::AgentWorkEffectReinspection,
            WorkBrowserObservationRequest,
        ),
        crate::AgentWorkEffectReinspectionError,
    > {
        use crate::AgentWorkEffectReinspectionError as Error;
        if !self.native_unverified
            || !self.finished
            || self.native.is_some()
            || self.pending.is_some()
            || self.terminal.is_some()
            || self.journal_failed
        {
            return Err(Error::Unavailable);
        }
        let failed = self
            .failed
            .as_ref()
            .filter(|failed| {
                failed.failure() == SemanticActionFailure::NeedsHuman
                    && self.receipt == Some(failed.receipt())
            })
            .ok_or(Error::Unavailable)?;
        if self.reinspection_owner.is_some() {
            return Err(Error::AlreadyIssued);
        }
        let owner = std::sync::Arc::new(());
        let prepared = crate::work_reinspection::AgentWorkEffectReinspection::prepare(
            owner.clone(),
            failed.receipt(),
            &self.proposal.action,
            account,
            resources,
            lease,
            target,
            now,
        )?;
        self.reinspection_owner = Some(owner);
        Ok(prepared)
    }

    pub(crate) fn record_reinspection(
        &mut self,
        result: crate::AgentWorkEffectReobservation,
    ) -> Result<(), Box<crate::AgentWorkEffectReobservation>> {
        if self.reinspection_result.is_some()
            || self
                .reinspection_owner
                .as_ref()
                .is_none_or(|owner| !std::sync::Arc::ptr_eq(owner, &result.owner))
            || self.receipt != Some(result.original_effect())
        {
            return Err(Box::new(result));
        }
        self.reinspection_result = Some(result);
        Ok(())
    }

    pub(crate) fn reinspection_result(&self) -> Option<&crate::AgentWorkEffectReobservation> {
        self.reinspection_result.as_ref()
    }

    #[cfg(all(test, feature = "probe-harness"))]
    pub(crate) fn retained_failure(&self) -> Option<&AgentFailedSemanticEffect> {
        self.failed.as_ref()
    }

    #[cfg(all(test, feature = "probe-harness"))]
    pub(crate) fn reinspection_test_action(&self) -> &SemanticPreparedAction {
        &self.proposal.action
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

    /// Consumes only the exact fully-accounted synchronous refusal. The caller
    /// proves native non-admission; a native failure callback is insufficient.
    pub(crate) fn into_rejected_batch(self) -> Result<SemanticActionBatchResult, Box<Self>> {
        if !self.finished
            || self.native.is_some()
            || self.pending.is_some()
            || self.terminal.is_some()
            || self.journal_failed
            || self.failed.as_ref().is_none_or(|failed| {
                failed.execution().is_some() || self.receipt != Some(failed.receipt())
            })
        {
            return Err(Box::new(self));
        }
        self.into_failed_batch()
    }

    /// A rejected read-effect batch keeps its continuation: the model hears
    /// the refusal and chooses again on a fresh observation.
    pub(crate) fn into_rejected_refusal(
        self,
    ) -> Result<(SemanticActionBatchResult, AgentProviderActionRefusal), Box<Self>> {
        if self.proposal.action.effect() != SemanticEffectClass::Read || self.proposal.continuation.is_none() {
            return Err(Box::new(self));
        }
        let mut this = *self.into_rejected_batch_keeping()?;
        let failed = this.failed.take().expect("checked original failed owner");
        match this.proposal.batch.fail(&this.proposal.action, failed) {
            Ok(terminal) => Ok((
                terminal,
                AgentProviderActionRefusal::unissued(
                    this.proposal.continuation.take().expect("checked provider continuation"),
                    SemanticActionBindingError::DispatchRejected,
                    this.proposal.refusal_context,
                ),
            )),
            Err(refusal) => {
                let (batch, failed, _) = refusal.into_parts();
                this.proposal.batch = batch;
                this.failed = Some(failed);
                Err(Box::new(this))
            }
        }
    }
    /// A read-effect action that ran but whose outcome was not observed:
    /// its batch fails with that reason and the model hears why.
    pub(crate) fn into_unverified_refusal(
        self,
        local_writes: bool,
    ) -> Result<(SemanticActionBatchResult, AgentProviderActionRefusal), Box<Self>> {
        let effect = self.proposal.action.effect();
        if !self.finished
            || self.native.is_some()
            || self.pending.is_some()
            || self.terminal.is_some()
            || self.journal_failed
            || !(effect == SemanticEffectClass::Read
                || (local_writes && effect == SemanticEffectClass::LocalWrite))
            || self.proposal.continuation.is_none()
            || self.failed.as_ref().is_none_or(|failed| {
                self.receipt != Some(failed.receipt()) || failed.verification_error().is_none()
            })
        {
            return Err(Box::new(self));
        }
        let mut this = self;
        let failed = this.failed.take().expect("checked original failed owner");
        match this.proposal.batch.fail(&this.proposal.action, failed) {
            Ok(terminal) => Ok((
                terminal,
                AgentProviderActionRefusal::unissued(
                    this.proposal.continuation.take().expect("checked provider continuation"),
                    SemanticActionBindingError::Unverified,
                    this.proposal.refusal_context,
                ),
            )),
            Err(refusal) => {
                let (batch, failed, _) = refusal.into_parts();
                this.proposal.batch = batch;
                this.failed = Some(failed);
                Err(Box::new(this))
            }
        }
    }
    /// An action the page refused before it acted (its target covered or
    /// out of view): nothing was pressed or typed, so a read or a draft keeps
    /// its continuation and the model hears why.
    pub(crate) fn into_covered_refusal(
        self,
    ) -> Result<(SemanticActionBatchResult, AgentProviderActionRefusal), Box<Self>> {
        let effect = self.proposal.action.effect();
        if !self.finished
            || self.native.is_some()
            || self.pending.is_some()
            || self.terminal.is_some()
            || self.journal_failed
            || !matches!(
                effect,
                SemanticEffectClass::Read | SemanticEffectClass::LocalWrite
            )
            || self.proposal.continuation.is_none()
            || self.failed.as_ref().is_none_or(|failed| {
                self.receipt != Some(failed.receipt())
                    || failed.failure() != SemanticActionFailure::TargetOccluded
            })
        {
            return Err(Box::new(self));
        }
        let mut this = self;
        let failed = this.failed.take().expect("checked original failed owner");
        match this.proposal.batch.fail(&this.proposal.action, failed) {
            Ok(terminal) => Ok((
                terminal,
                AgentProviderActionRefusal::unissued(
                    this.proposal
                        .continuation
                        .take()
                        .expect("checked provider continuation"),
                    SemanticActionBindingError::TargetCovered,
                    this.proposal.refusal_context,
                ),
            )),
            Err(refusal) => {
                let (batch, failed, _) = refusal.into_parts();
                this.proposal.batch = batch;
                this.failed = Some(failed);
                Err(Box::new(this))
            }
        }
    }

    /// A failed code-owned read (no model continuation) closes its batch so
    /// the page goes on to the model's own first look.
    pub(crate) fn into_failed_owned_read(self) -> Result<SemanticActionBatchResult, Box<Self>> {
        if !self.finished
            || self.native.is_some()
            || self.pending.is_some()
            || self.terminal.is_some()
            || self.journal_failed
            || self.proposal.action.effect() != SemanticEffectClass::Read
            || self.proposal.continuation.is_some()
            || self
                .failed
                .as_ref()
                .is_none_or(|failed| self.receipt != Some(failed.receipt()))
        {
            return Err(Box::new(self));
        }
        self.into_failed_batch()
    }

    fn into_rejected_batch_keeping(self) -> Result<Box<Self>, Box<Self>> {
        if !self.finished
            || self.native.is_some()
            || self.pending.is_some()
            || self.terminal.is_some()
            || self.journal_failed
            || self.failed.as_ref().is_none_or(|failed| {
                failed.execution().is_some() || self.receipt != Some(failed.receipt())
            })
        {
            return Err(Box::new(self));
        }
        Ok(Box::new(self))
    }

    pub(crate) fn into_failed_read_scroll_batch(
        self,
    ) -> Result<SemanticActionBatchResult, Box<Self>> {
        if !self.finished
            || self.native.is_some()
            || self.pending.is_some()
            || self.terminal.is_some()
            || self.journal_failed
            || self.proposal.action.kind() != SemanticActionKind::Scroll
            || self.proposal.action.effect() != SemanticEffectClass::Read
            || self.failed.as_ref().is_none_or(|failed| {
                self.receipt != Some(failed.receipt())
                    || failed.verification_error()
                        != Some(SemanticVerificationError::OutcomeNotObserved)
                    || failed.execution().is_none_or(|execution| {
                        execution.backend() != SemanticActionExecutionBackend::FixedSemanticRecipe
                    })
            })
        {
            return Err(Box::new(self));
        }
        self.into_failed_batch()
    }

    pub(crate) fn into_failed_decision_read_batch(
        self,
    ) -> Result<SemanticActionBatchResult, Box<Self>> {
        if !self.finished
            || self.native.is_some()
            || self.pending.is_some()
            || self.terminal.is_some()
            || self.journal_failed
            || self.proposal.continuation.is_some()
            || self.proposal.action.effect() != SemanticEffectClass::Read
            || self.failed.as_ref().is_none_or(|failed| {
                self.receipt != Some(failed.receipt())
                    || failed.verification_error().is_none()
                    || failed.execution().is_none_or(|execution| {
                        execution.backend() != SemanticActionExecutionBackend::FixedSemanticRecipe
                    })
            })
        {
            return Err(Box::new(self));
        }
        self.into_failed_batch()
    }

    fn into_failed_batch(mut self) -> Result<SemanticActionBatchResult, Box<Self>> {
        let failed = self.failed.take().expect("checked original failed owner");
        match self.proposal.batch.fail(&self.proposal.action, failed) {
            Ok(terminal) => Ok(terminal),
            Err(refusal) => {
                let (batch, failed, _) = refusal.into_parts();
                self.proposal.batch = batch;
                self.failed = Some(failed);
                Err(Box::new(self))
            }
        }
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
        let native_unverified = native.is_applied_unverified();
        let outcome = execution
            .settle(self.proposal.action.frame(), native)
            .map_err(AgentBrowserActionError::Native)?;
        self.native_unverified = native_unverified;
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
        let evidence = match prepare_semantic_action_snapshot_evidence(
            &self.proposal.action,
            self.reservation.attempt(),
            observed_at,
            snapshot,
        ) {
            Ok(evidence) => Ok(evidence),
            // The target left the page or changed after the action ran: the
            // terminal settles as refused with that exact reason.
            Err(SemanticSnapshotEvidenceError::Revalidation(error)) => Err(error),
            Err(SemanticSnapshotEvidenceError::NonSnapshotEvidenceRequired) => {
                return Err(AgentBrowserActionError::EvidenceRequired);
            }
        };
        let terminal = self.terminal.take().ok_or(AgentBrowserActionError::State)?;
        let verified = match evidence {
            Ok(evidence) => {
                verify_semantic_action_terminal(*terminal, &self.proposal.action, evidence)
            }
            Err(error) => Err(SemanticActionVerificationRefusal::unobserved(
                *terminal,
                observed_at,
                error,
            )),
        };
        let verified =
            match verified {
                Ok(verified) => verified,
                Err(refusal) => {
                    let reason = refusal.error();
                    let failed = policy
                        .settle_refused_semantic_terminal(refusal)
                        .map_err(AgentBrowserActionError::Policy)?;
                    self.retain_failure(failed);
                    return Err(AgentBrowserActionError::Verification(reason));
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
            &self.proposal.baseline,
            SemanticPostActionObservation::new(observed_at, current.clone()),
            SemanticDiffBudget::ACTION,
        )
        .map_err(|refusal| {
            AgentBrowserActionFinalizationRefusal::Finalization(Box::new(refusal))
        })?;
        // Both a delta and a fresh observation carry the same independently
        // verified effect. Record its batch/accounting before provider delivery.
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
            #[cfg(feature = "probe-harness")]
            probe_diff: None,
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
    /// The retained native adapters do not dispatch this interaction kind.
    UnsupportedInteraction,
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
    /// Total native/settlement/verification allowance is below host capability.
    SettleBudget,
    /// One effect was charged as a typed failure.
    Failed(SemanticActionFailure),
    /// Independent observation verification failed; the charged failed effect
    /// remains owned exactly as for other failures. Contains no page content.
    Verification(SemanticVerificationError),
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
    /// Complete accounted result remains owned after batch correlation refusal.
    BatchAdmission(Box<SemanticActionBatchAdmissionRefusal>),
    /// Closed batch invariant failure; no successful continuation exists.
    Batch(SemanticActionBatchExecutionError),
}
