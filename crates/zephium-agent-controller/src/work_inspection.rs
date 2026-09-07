//! Closed progressive inspection through the original native observation owner.
use super::*;

impl AgentWorkController {
    pub(super) async fn inspect_current(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        turn: AgentBrowserProviderTurn,
        previous: &SemanticObservation,
    ) -> Result<
        (
            SemanticObservation,
            SemanticCaptureInstant,
            AgentWorkTaskProgress,
            AgentBrowserProviderTurn,
        ),
        AgentWorkFailure,
    > {
        state.check_task_contract()?;
        if !state.progressive_observation {
            return Err(AgentWorkFailure::Contract);
        }
        let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
        session.check_live().map_err(AgentWorkFailure::Browser)?;
        // Inspection must leave room for the next decision and terminal mapper.
        // It never renews the original model-call or wall-clock budget.
        if usize::from(session.turns) + 2 > usize::from(super::super::MAX_BROWSER_MODEL_TURNS) {
            return Err(AgentWorkFailure::Browser(
                AgentBrowserProviderError::TurnLimit,
            ));
        }
        let checkpoint = turn
            .into_tool_turn()
            .into_parts()
            .1
            .retire_for_observation(previous, &session.config)
            .map_err(|_| AgentWorkFailure::Browser(AgentBrowserProviderError::Continuation))?;
        state.native.check_control(worker, browser)?;
        state.refresh_account(worker, browser)?;
        state.journal_mut()?.emit(AgentWorkEventKind::ToolProposed(
            AgentBrowserToolKind::Snapshot,
        ))?;
        state.journal_mut()?.emit(AgentWorkEventKind::Observing)?;
        let current = if state.native.retained.is_some() {
            let expansion = checkpoint
                .expansion()
                .map(|(target, kind)| (previous, checkpoint.baseline(), target, kind));
            state
                .native
                .observe_retained_scope(worker, expansion)
                .await?
        } else {
            let id =
                SemanticObservationId::new(state.native.id()?).ok_or(AgentWorkFailure::Contract)?;
            let request = checkpoint
                .request(previous, id)
                .map_err(|_| AgentWorkFailure::Browser(AgentBrowserProviderError::Continuation))?;
            let [frame] = previous.frames() else {
                return Err(AgentWorkFailure::Contract);
            };
            Self::capture_once(state, worker, browser, request, frame.frame().clone()).await?
        };
        state.native.check_control(worker, browser)?;
        state.check_task_contract()?;
        let captured_at = SemanticCaptureInstant::from_millis(
            state
                .journal_mut()?
                .clock
                .now()
                .map_err(|_| AgentWorkFailure::Contract)?
                .millis(),
        );
        let progress = state.task_progress(&current)?;
        state.refresh_account(worker, browser)?;
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let next = Self::provider(
            &mut state.native,
            worker,
            browser,
            session.cancellation.clone(),
            session.continue_after_observation(checkpoint, previous, &current),
        )
        .await?;
        Ok((current, captured_at, progress, next))
    }
}

impl AgentBrowserSession {
    async fn continue_after_observation(
        &mut self,
        checkpoint: AgentProviderObservationCheckpoint,
        previous: &SemanticObservation,
        observation: &SemanticObservation,
    ) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
        self.check_live()?;
        let request = self.next_model_call_request()?;
        let payload = encode_semantic_observation(
            observation,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .and_then(|encoded| encoded.admit_conservative_utf8(self.config.tokenizer()))
        .map_err(AgentBrowserProviderError::InitialEncoding)?;
        let objective = self
            .objective
            .as_ref()
            .ok_or(AgentBrowserProviderError::Continuation)?;
        let prepared = checkpoint
            .prepare_successor(
                &mut self.policy,
                previous,
                observation,
                request,
                self.config.clone(),
                payload,
                objective,
            )
            .map_err(|_| AgentBrowserProviderError::Authority)?;
        self.drive(prepared.into_transport_input()).await
    }
}
