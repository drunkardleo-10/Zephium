use super::*;

const MAX_UNCHANGED_DECISION_OBSERVATIONS: u8 = 3;
/// Cheap typed re-observations of one page before the read stops asking the
/// recommended backend. Three covers the recorded product pages: expanding one
/// collapsed specification disclosure plus two screens of scrolling, and it
/// bounds the added work at three native operations and three sub-second
/// batches, well inside one read's existing action and call allowances.
const MAX_READ_REOBSERVATIONS: u8 = 3;

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
        let mut reobservations = 0u8;
        let mut pending: Option<Box<zephium_agentic::DecisionReadSelection>> = None;
        // The address the one-document gate admitted, never one from page text.
        let document = state
            .native
            .retained
            .as_ref()
            .map(|browser| zephium_agentic::untracked_document_address(browser.binding().document()));
        loop {
            state.check_task_contract()?;
            state.native.check_control(worker, browser)?;
            let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
            if session.decisions.is_none() || progress == AgentWorkTaskProgress::Complete {
                break;
            }
            if state.decision_answers.is_none() {
                if !state.human_request {
                    if Self::classify_human_challenge(
                        state,
                        worker,
                        browser,
                        &observation,
                        reobservations > 0,
                    )
                    .await?
                    {
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
                if progress == AgentWorkTaskProgress::Complete {
                    break;
                }
            }
            let Some(mut answers) = state.decision_answers.take() else {
                break;
            };
            let mut gap: Option<Box<zephium_agentic::DecisionReadSelection>> = None;
            let mut candidate: Option<Box<zephium_agentic::DecisionReadSelection>> = None;
            if let Some(schema) = state.extraction_schema.clone() {
                let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
                if let Some((selection, ready)) = answers
                    .take_read_progress_retaining_evidence(
                        &observation,
                        session.account,
                        &schema,
                        captured_at,
                        &mut state.retained_read_evidence,
                    )
                    .map_err(AgentWorkFailure::DecisionRead)?
                {
                    // A look that located everything except optional values it
                    // confidently found absent finishes once a later look
                    // confirms each absence; those columns publish unknown.
                    // A required column never settles this way.
                    let confirmed =
                        pending.take_if(|earlier| !ready && selection.confirms_absence(earlier));
                    if ready || confirmed.is_some() {
                        state.refresh_account(worker, browser)?;
                        let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
                        // The earlier look's values come from the evidence it
                        // retained, cited under the current observation.
                        let evidence = std::mem::take(&mut state.retained_read_evidence);
                        // A confirming look that itself located the rest is
                        // read directly, with focused generation if needed;
                        // otherwise the earlier look's copies are cited.
                        let prepared = match confirmed.filter(|_| !selection.awaits_absence()) {
                            Some(earlier) => earlier.prepare_confirmed(
                                &observation,
                                session.account,
                                captured_at,
                                document.as_ref(),
                                &evidence,
                            ),
                            None => selection.prepare(
                                &observation,
                                session.account,
                                captured_at,
                                document.as_ref(),
                            ),
                        };
                        // A clipped or unavailable exact source leaves the normal planner available.
                        let located = match prepared {
                            Ok(located) => located,
                            Err(_) => {
                                state.retained_read_evidence = evidence;
                                break;
                            }
                        };
                        // Boxed: the read loop's future must stay well inside the
                        // runtime worker's stack.
                        Box::pin(Self::finish_located_read(state, worker, browser, located))
                            .await?;
                        state.retained_read_evidence = evidence;
                        return Ok((
                            observation,
                            captured_at,
                            AgentWorkTaskProgress::Complete,
                            false,
                        ));
                    }
                    if selection.awaits_absence() {
                        candidate = Some(Box::new(selection));
                    } else {
                        gap = Some(Box::new(selection));
                    }
                }
            }
            let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
            let more_below = answers
                .take_more_below(&observation, session.account)
                .map_err(AgentWorkFailure::DecisionMoreBelow)?;
            let actionable = state.actions_before_extraction
                && progress == AgentWorkTaskProgress::Continue
                && session.next_action <= session.max_actions
                && session
                    .policy
                    .remaining_operations(session.lease.lease())
                    .map_err(|_| AgentWorkFailure::Contract)?
                    >= 3;
            // A settled action head is already paid for, so it is preferred.
            // Otherwise one cheap typed re-observation runs before any generated
            // value: Rust scrolls an already offered region and asks the
            // recommended backend again over value heads only.
            let proposed = if actionable {
                answers
                    .take_action_selection(&observation, session.account)
                    .map_err(AgentWorkFailure::DecisionOperation)?
            } else {
                None
            };
            if proposed
                .as_ref()
                .is_some_and(|selection| matches!(selection.operation(), DecisionOperation::Scroll(_)))
                && more_below != Some(true)
            {
                break;
            }
            // Rust's own re-observation is optimistic: if the page refuses it,
            // the read continues on the existing planner path instead of ending.
            let mut reobserving = false;
            let selection = match proposed {
                Some(selection) => selection,
                None => {
                    let reobserve = actionable
                        && reobservations < MAX_READ_REOBSERVATIONS
                        && (gap.is_some() || candidate.is_some() || pending.is_some());
                    let scroll = if reobserve {
                        answers
                            .take_reobservation_scroll(&observation, session.account)
                            .map_err(AgentWorkFailure::DecisionOperation)?
                    } else {
                        None
                    };
                    match scroll {
                        Some(scroll) => {
                            reobservations = reobservations.saturating_add(1);
                            reobserving = true;
                            gap = None;
                            scroll
                        }
                        None => {
                            // The recommended backend stopped short. One focused
                            // generation over the located neighbourhood finishes
                            // the read; the general planner is not asked here.
                            if let Some(gap) = gap.filter(|gap| {
                                gap.unresolved() > 0
                                    && gap.located() > 0
                                    && gap.unresolved_are_generated()
                            }) {
                                state.refresh_account(worker, browser)?;
                                let session =
                                    state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
                                let located = match gap.prepare_completing(
                                    &observation,
                                    session.account,
                                    captured_at,
                                    document.as_ref(),
                                ) {
                                    Ok(located) => located,
                                    Err(_) => break,
                                };
                                // Boxed: the read loop's future must stay well
                                // inside the runtime worker's stack.
                                Box::pin(Self::finish_located_read(
                                    state, worker, browser, located,
                                ))
                                .await?;
                                return Ok((
                                    observation,
                                    captured_at,
                                    AgentWorkTaskProgress::Complete,
                                    false,
                                ));
                            }
                            break;
                        }
                    }
                }
            };
            let _ = gap;
            let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
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
            let executed = Self::execute_prepared_action(
                state,
                worker,
                browser,
                proposal,
                assessment,
                &observation,
            )
            .await;
            let (current, at, transition) = match executed {
                Ok(settled) => settled,
                Err(AgentWorkFailure::Browser(AgentBrowserProviderError::Action(_)))
                    if reobserving =>
                {
                    break
                }
                Err(error) => return Err(error),
            };
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
            if let Some(selection) = candidate.take() {
                pending = Some(selection);
            }
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

    /// One extraction from located sources. Focused generation is the only
    /// provider call it can make, and it never reopens the general planner.
    async fn finish_located_read(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        located: zephium_agentic::DecisionLocatedRead<'_>,
    ) -> Result<(), AgentWorkFailure> {
        state
            .journal_mut()?
            .emit(AgentWorkEventKind::ToolProposed(
                AgentBrowserToolKind::Extract,
            ))?;
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let result = Self::provider(
            &mut state.native,
            worker,
            browser,
            session.cancellation.clone(),
            session.extract_located(located),
        )
        .await?;
        if state.task.accept_extraction(&result)? != AgentWorkTaskProgress::Complete {
            return Err(AgentWorkFailure::Contract);
        }
        state.check_task_contract()?;
        state.extraction = Some(
            result
                .into_owned()
                .map_err(|_| AgentWorkFailure::Contract)?,
        );
        Ok(())
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
