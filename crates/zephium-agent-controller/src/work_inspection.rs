//! Closed progressive inspection through the original native observation owner.
use super::*;

impl AgentWorkController {
    pub(super) fn fit_model_observation(
        observation: SemanticObservation,
    ) -> Result<SemanticObservation, AgentWorkFailure> {
        fit_semantic_observation_for_model(
            observation,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .map_err(|error| {
            AgentWorkFailure::Browser(AgentBrowserProviderError::InitialEncoding(error))
        })
    }

    pub(super) async fn inspect_current(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        mut checkpoint: AgentProviderObservationCheckpoint,
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
        if usize::from(session.turns) + 2 > usize::from(session.max_model_calls)
            || session
                .policy
                .remaining_operations(session.lease.lease())
                .map_err(|_| AgentWorkFailure::Browser(AgentBrowserProviderError::Authority))?
                < 2
        {
            return Err(AgentWorkFailure::Browser(
                AgentBrowserProviderError::TurnLimit,
            ));
        }
        state.native.check_control(worker, browser)?;
        state.refresh_account(worker, browser)?;
        state.journal_mut()?.emit(AgentWorkEventKind::ToolProposed(
            AgentBrowserToolKind::Snapshot,
        ))?;
        state.journal_mut()?.emit(AgentWorkEventKind::Observing)?;
        let current = if state.native.retained.is_some() {
            let capability = state.observation_capability();
            let expansion = checkpoint
                .expansion()
                .map(|(target, kind)| (previous, checkpoint.baseline(), target, kind));
            match state
                .native
                .observe_retained_scope(worker, expansion, capability)
                .await
            {
                Ok(current) => current,
                Err(AgentWorkFailure::InspectionAnchorLost) if checkpoint.expansion().is_some() => {
                    checkpoint = checkpoint
                        .after_anchor_loss()
                        .map_err(|_| AgentWorkFailure::Contract)?;
                    state
                        .journal_mut()?
                        .emit(AgentWorkEventKind::InspectionAnchorLost)?;
                    state.native.check_control(worker, browser)?;
                    state.refresh_account(worker, browser)?;
                    state
                        .native
                        .observe_retained_scope(worker, None, capability)
                        .await?
                }
                Err(error) => return Err(error),
            }
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
        let current = Self::fit_model_observation(current)?;
        let progress = state.task_progress(&current)?;
        state.refresh_account(worker, browser)?;
        let action_authority = state
            .session
            .as_ref()
            .ok_or(AgentWorkFailure::Contract)?
            .config
            .permits_tool(AgentBrowserToolKind::Act)
            .then(|| state.action_authority(&current))
            .transpose()?;
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let next = Self::provider(
            &mut state.native,
            worker,
            browser,
            session.cancellation.clone(),
            session.continue_after_observation(
                checkpoint,
                previous,
                &current,
                action_authority.as_ref(),
            ),
        )
        .await?;
        Ok((current, captured_at, progress, next))
    }
}

impl AgentBrowserSession {
    pub(super) async fn continue_after_navigation_refusal(
        &mut self,
        refusal: AgentProviderNavigationRefusal,
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
        let prepared =
            AgentPreparedObservationRequest::try_navigation_refusal_for_provider_exact_count(
                &mut self.policy,
                request,
                observation,
                payload,
                self.config.clone(),
                refusal,
            )
            .map_err(AgentBrowserProviderError::from_request)?;
        self.drive(prepared.into_transport_input()).await
    }

    pub(super) async fn continue_after_action_refusal(
        &mut self,
        refusal: AgentProviderActionRefusal,
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
        let prepared =
            AgentPreparedObservationRequest::try_action_refusal_for_provider_exact_count(
                &mut self.policy,
                request,
                observation,
                payload,
                self.config.clone(),
                refusal,
            )
            .map_err(AgentBrowserProviderError::from_request)?;
        self.drive(prepared.into_transport_input()).await
    }

    pub(super) async fn continue_after_scope_refusal(
        &mut self,
        refusal: AgentProviderObservationRefusal,
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
        let prepared = AgentPreparedObservationRequest::try_scope_refusal_for_provider_exact_count(
            &mut self.policy,
            request,
            observation,
            payload,
            self.config.clone(),
            refusal,
        )
        .map_err(AgentBrowserProviderError::from_request)?;
        self.drive(prepared.into_transport_input()).await
    }

    async fn continue_after_observation(
        &mut self,
        checkpoint: AgentProviderObservationCheckpoint,
        previous: &SemanticObservation,
        observation: &SemanticObservation,
        action_authority: Option<&AgentProviderActionAuthority>,
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
            .prepare_successor_with_action_authority(
                &mut self.policy,
                previous,
                observation,
                request,
                self.config.clone(),
                payload,
                objective,
                action_authority,
            )
            .map_err(AgentBrowserProviderError::from_request)?;
        self.drive(prepared.into_transport_input()).await
    }
}
