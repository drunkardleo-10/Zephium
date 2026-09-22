use super::*;

const MAX_UNCHANGED_DECISION_OBSERVATIONS: u8 = 3;

impl AgentWorkController {
    pub(super) async fn run_decision_actions(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        mut observation: SemanticObservation,
        mut captured_at: SemanticCaptureInstant,
        mut progress: AgentWorkTaskProgress,
    ) -> Result<
        (
            SemanticObservation,
            SemanticCaptureInstant,
            AgentWorkTaskProgress,
            bool,
        ),
        AgentWorkFailure,
    > {
        let mut unchanged = 0u8;
        loop {
            state.check_task_contract()?;
            state.native.check_control(worker, browser)?;
            let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
            if session.decisions.is_none()
                || !state.actions_before_extraction
                || progress != AgentWorkTaskProgress::Continue
                || session.next_action > session.max_actions
                || session
                    .policy
                    .remaining_operations(session.lease.lease())
                    .map_err(|_| AgentWorkFailure::Contract)?
                    < 3
            {
                break;
            }
            if state.decision_answers.is_none() {
                if !state.human_request {
                    if Self::classify_human_challenge(state, worker, browser, &observation).await? {
                        break;
                    }
                } else {
                    let (fresh, challenged) =
                        Self::settle_human_challenge(state, worker, browser, observation).await?;
                    observation = fresh;
                    captured_at = SemanticCaptureInstant::from_millis(
                        state
                            .session
                            .as_mut()
                            .ok_or(AgentWorkFailure::Contract)?
                            .policy_now()
                            .map_err(AgentWorkFailure::Browser)?
                            .millis(),
                    );
                    progress = state.task_progress(&observation)?;
                    if challenged {
                        return Ok((observation, captured_at, progress, true));
                    }
                }
                if progress != AgentWorkTaskProgress::Continue {
                    break;
                }
            }
            let Some(mut answers) = state.decision_answers.take() else {
                break;
            };
            let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
            let more_below = answers
                .take_more_below(&observation, session.account)
                .map_err(|_| AgentWorkFailure::Contract)?;
            let Some(selection) = answers
                .take_action_selection(&observation, session.account)
                .map_err(|_| AgentWorkFailure::Contract)?
            else {
                break;
            };
            if matches!(selection.operation(), DecisionOperation::Scroll(_))
                && more_below != Some(true)
            {
                break;
            }
            let Some(recipe) = state
                .task
                .decision_action_recipe(selection.operation(), &observation)?
            else {
                break;
            };
            let frames = observation
                .frames()
                .iter()
                .map(|frame| frame.frame().clone())
                .collect::<Vec<_>>();
            let batch = SemanticActionBatchId::new(session.next_action)
                .ok_or(AgentWorkFailure::Contract)?;
            let proposal = match crate::AgentBrowserActionProposal::bind_decision(
                selection,
                recipe,
                &observation,
                &frames,
                batch,
            ) {
                Ok(proposal) => proposal,
                // Nothing was dispatched; the full planner may supply a supported recipe.
                Err(_) => break,
            };
            let assessment = match state.task.assess_observed(proposal.action(), &observation) {
                Ok(assessment) => assessment,
                Err(AgentWorkFailure::ActionDenied) => break,
                Err(error) => return Err(error),
            };
            state
                .journal_mut()?
                .emit(AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Act))?;
            Self::retain_action_read_evidence(state, &proposal, &observation, captured_at)?;
            let (current, at, transition) = Self::execute_prepared_action(
                state,
                worker,
                browser,
                proposal,
                assessment,
                &observation,
            )
            .await?;
            let batch = transition
                .batch_result()
                .ok_or(AgentWorkFailure::Contract)?;
            unchanged = if batch
                .final_state()
                .and_then(SemanticActionResult::diff)
                .is_some_and(|diff| diff.entries().is_empty())
            {
                unchanged.saturating_add(1)
            } else {
                0
            };
            state.task.accept_verified_action(batch, &current)?;
            observation = current;
            captured_at = at;
            progress = state.task_progress(&observation)?;
            if unchanged >= MAX_UNCHANGED_DECISION_OBSERVATIONS {
                return Err(AgentWorkFailure::Browser(
                    AgentBrowserProviderError::NoProgress,
                ));
            }
            if state.journal_mut()?.audit.status().pending() >= 32 {
                Self::drain_audit(state, worker, browser, None).await?;
            }
        }
        Ok((observation, captured_at, progress, false))
    }

    pub(super) fn retain_action_read_evidence(
        state: &mut WorkState,
        proposal: &crate::AgentBrowserActionProposal,
        observation: &SemanticObservation,
        captured_at: SemanticCaptureInstant,
    ) -> Result<(), AgentWorkFailure> {
        if state.progressive_observation || state.navigation_discovery.is_some() {
            if let Some(schema) = &state.extraction_schema {
                state
                    .session
                    .as_ref()
                    .ok_or(AgentWorkFailure::Contract)?
                    .check_live()
                    .map_err(AgentWorkFailure::Browser)?;
                let read = read_semantic_observation_for_schema(
                    observation,
                    SemanticReadAuthority::Acknowledged(proposal.baseline()),
                    captured_at,
                    SemanticReadSensitivityLimit::PublicOnly,
                    SemanticReadBudget::STANDARD,
                    schema,
                )
                .map_err(|error| {
                    AgentWorkFailure::Browser(AgentBrowserProviderError::Read(error))
                })?;
                state
                    .retained_read_evidence
                    .retain(&read, proposal.baseline())
                    .map_err(|error| {
                        AgentWorkFailure::Browser(AgentBrowserProviderError::Read(error))
                    })?;
            }
        }
        Ok(())
    }

    pub(super) async fn execute_prepared_action(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        proposal: crate::AgentBrowserActionProposal,
        assessment: AgentEffectAssessment,
        baseline: &SemanticObservation,
    ) -> Result<
        (
            SemanticObservation,
            SemanticCaptureInstant,
            crate::AgentBrowserVerifiedTransition,
        ),
        AgentWorkFailure,
    > {
        state.refresh_account(worker, browser)?;
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let id = state.native.identity.id();
        let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
        let automation = if let Some(retained) = &state.native.retained {
            retained.automation_state(now)?
        } else {
            state
                .native
                .contexts()?
                .automation_state(id)
                .map_err(|_| AgentWorkFailure::Context)?
        };
        let request = session
            .authorize_action(
                proposal,
                &assessment,
                automation,
                SemanticActionExecutionInstant::from_millis(now.millis()),
            )
            .map_err(AgentWorkFailure::Browser)?;
        let action_deadline = request.deadline();
        let dispatch = if let Some(retained) = &mut state.native.retained {
            retained.dispatch_action(request, now)
        } else {
            browser.execute_semantic_action(request, worker.semantic_action_completion())
        };
        // Native ownership starts at dispatch, before fallible policy
        // accounting. Recovery must drain even if that accounting fails.
        state.native.action_pending = matches!(dispatch, ContextDispatch::Scheduled);
        session
            .account_action_dispatch(dispatch)
            .map_err(AgentWorkFailure::Browser)?;
        let terminal = match state
            .native
            .next_action_event(worker, browser, action_deadline)
            .await?
        {
            AgentRuntimeEvent::SemanticActionTerminal(terminal)
                if session.action.as_ref().is_some_and(|action| {
                    action.accepts_settlement(&session.action_executions, &terminal)
                }) =>
            {
                terminal
            }
            event => {
                state.native.retain(event)?;
                return Err(AgentWorkFailure::Mailbox);
            }
        };
        state.native.action_pending = false;
        state.native_terminal = Some(terminal);
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let terminal = state
            .native_terminal
            .take()
            .ok_or(AgentWorkFailure::Contract)?;
        let mut wake = session
            .begin_action_settlement(terminal)
            .map_err(AgentWorkFailure::Browser)?;
        // Native evidence cannot restore a revoked document or authorize
        // postcondition reads. Settle its original owner first, then fail
        // closed before any continuation, success, or retry.
        state.native.check_control(worker, browser)?;
        for _ in 0..8 {
            let Some(next_wake) = wake else {
                break;
            };
            let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
            let delay = Duration::from_millis(next_wake.millis().saturating_sub(now.millis()));
            tokio::select! {
                biased;
                event = state.native.next_event(worker, browser) => {
                    state.native.retain(event?)?;
                    return Err(AgentWorkFailure::Mailbox);
                }
                () = tokio::time::sleep(delay) => {}
            }
            let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
            wake = session
                .wake_action_settlement(SemanticSettleInstant::from_millis(now.millis()))
                .map_err(AgentWorkFailure::Browser)?;
        }
        if wake.is_some() {
            return Err(AgentWorkFailure::Contract);
        }
        state.journal_mut()?.emit(AgentWorkEventKind::Verifying)?;
        let current = Self::observe(state, worker, browser).await?;
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
        let (_, transition) = session
            .verify_action_settlement(
                baseline,
                &current,
                SemanticSettleInstant::from_millis(now.millis()),
            )
            .map_err(AgentWorkFailure::Browser)?;
        Ok((
            current,
            SemanticCaptureInstant::from_millis(now.millis()),
            transition,
        ))
    }
}
