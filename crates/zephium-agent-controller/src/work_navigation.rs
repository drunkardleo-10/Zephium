//! One task-authored document hop through the original Work owners.

use super::*;

impl AgentWorkController {
    pub(super) fn validate_navigation_target(
        target: &ContextNavigationTarget,
        context: &AgentWorkContextSpec,
    ) -> Result<(), AgentWorkFailure> {
        // Only the task-authored, admission-frozen URL is inspected here.
        // No page/model URL can widen this same-origin exact destination.
        if target == &context.target
            || target.as_url().fragment().is_some()
            || target.as_url().as_str().len() > MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES
            || SemanticOrigin::parse(target.as_url().as_str()).as_ref() != Ok(&context.origin)
        {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(())
    }

    pub(super) async fn navigate_current(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
        turn: AgentBrowserProviderTurn,
        observation: &SemanticObservation,
        progress: AgentWorkTaskProgress,
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
        if state.navigation_committed || progress != AgentWorkTaskProgress::ReadyForNavigation {
            return Err(AgentWorkFailure::TaskPhase {
                expected: progress,
                proposed: AgentBrowserToolKind::Navigate,
            });
        }
        let target = state
            .navigation_target
            .clone()
            .ok_or(AgentWorkFailure::Contract)?;
        if !matches!(turn.turn.proposal(), AgentBrowserToolProposal::Navigate(proposed) if proposed == &target)
        {
            return Err(AgentWorkFailure::Contract);
        }
        let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
        // Reserve room for a destination proposal and its terminal mapping;
        // navigation cannot reset or silently consume the final usable call.
        if session.turns >= super::super::MAX_BROWSER_MODEL_TURNS - 1 {
            return Err(AgentWorkFailure::Browser(
                AgentBrowserProviderError::TurnLimit,
            ));
        }
        state.refresh_account(worker, browser)?;
        let id = state.native.identity.id();
        let automation = state
            .native
            .contexts()?
            .automation_state(id)
            .map_err(|_| AgentWorkFailure::Context)?;
        let op = ContextOperationId::new(state.native.id()?).ok_or(AgentWorkFailure::Contract)?;
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let checkpoint = turn
            .into_tool_turn()
            .into_parts()
            .1
            .retire_for_navigation(observation, &target, &session.config)
            .map_err(|_| AgentWorkFailure::Browser(AgentBrowserProviderError::Continuation))?;
        let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
        let permit = session
            .policy
            .authorize_navigation(
                AgentNavigationAuthorizationRequest::new(
                    session.lease.lease(),
                    session.account,
                    automation,
                    now,
                ),
                observation,
                checkpoint.baseline(),
                &target,
            )
            .map_err(|error| {
                AgentWorkFailure::Browser(AgentBrowserProviderError::Navigation(error))
            })?;
        let operation = match state.native.contexts()?.begin_navigation(id, op) {
            Ok(operation) => operation,
            Err(_) => {
                state
                    .session
                    .as_mut()
                    .ok_or(AgentWorkFailure::Contract)?
                    .policy
                    .cancel_navigation(permit)
                    .map_err(|error| {
                        AgentWorkFailure::Browser(AgentBrowserProviderError::Navigation(error))
                    })?;
                return Err(AgentWorkFailure::Context);
            }
        };
        // The old registry join is now irrevocably stale, before native dispatch.
        state.native.snapshot_generation = None;
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let active = session
            .policy
            .dispatch_navigation(permit, operation, now)
            .map_err(|error| {
                AgentWorkFailure::Browser(AgentBrowserProviderError::Navigation(error))
            })?;
        session.navigation = Some(active);
        let active = session
            .navigation
            .as_ref()
            .ok_or(AgentWorkFailure::Contract)?;
        let request = active
            .native_request()
            .map_err(|_| AgentWorkFailure::Contract)?;
        session
            .journal
            .as_mut()
            .ok_or(AgentWorkFailure::Contract)?
            .navigation_active(active)?;
        state.native.operation = Some(operation);
        let dispatch = browser.dispatch(ContextNativeRequest::Navigate(request));
        let refusal = match dispatch {
            ContextDispatch::Rejected(failure) => Some(failure),
            ContextDispatch::Unsupported => Some(ContextPortFailure::Unsupported),
            ContextDispatch::Scheduled => None,
        };
        if let Some(failure) = refusal {
            state.native.operation = None;
            state
                .native
                .contexts()?
                .settle_navigation(id, operation, ContextSettlement::Refused)
                .map_err(|_| AgentWorkFailure::Context)?;
            state
                .session
                .as_mut()
                .ok_or(AgentWorkFailure::Contract)?
                .settle_navigation_refusal(failure)?;
            return Err(AgentWorkFailure::Native(failure));
        }
        let receipt = match state.native.next_event(worker, browser).await? {
            AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::NavigationSettled(terminal))
                if terminal.operation() == operation =>
            {
                let result = state
                    .session
                    .as_mut()
                    .ok_or(AgentWorkFailure::Contract)?
                    .settle_navigation_terminal(&terminal);
                let receipt = match result {
                    Ok(receipt) => receipt,
                    Err(failure) => {
                        state.native.retain(AgentRuntimeEvent::NativeTerminal(
                            ContextNativeEvent::NavigationSettled(terminal),
                        ))?;
                        return Err(failure);
                    }
                };
                state.native.operation = None;
                state
                    .native
                    .contexts()?
                    .settle_navigation(
                        id,
                        operation,
                        if terminal.outcome().is_ok() {
                            ContextSettlement::Applied
                        } else {
                            ContextSettlement::Refused
                        },
                    )
                    .map_err(|_| AgentWorkFailure::Context)?;
                if let AgentNavigationSettlement::Failed(failure) = receipt.settlement() {
                    return Err(AgentWorkFailure::Native(failure));
                }
                receipt
            }
            event => {
                state.native.retain(event)?;
                return Err(AgentWorkFailure::Mailbox);
            }
        };
        state.native.check_control(worker, browser)?;
        state.check_task_contract()?;
        state.navigation_committed = true;
        let fresh = Self::observe(state, worker, browser).await?;
        let captured_at = SemanticCaptureInstant::from_millis(
            state
                .journal_mut()?
                .clock
                .now()
                .map_err(|_| AgentWorkFailure::Contract)?
                .millis(),
        );
        let progress = state.task_progress(&fresh)?;
        let now = state
            .session
            .as_mut()
            .ok_or(AgentWorkFailure::Contract)?
            .policy_now()
            .map_err(AgentWorkFailure::Browser)?;
        let account = state.task.attest_account(operation.context(), now);
        state.native.check_control(worker, browser)?;
        state.check_task_contract()?;
        let account = account?;
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let turn = Self::provider(
            &mut state.native,
            worker,
            browser,
            session.cancellation.clone(),
            session.continue_after_navigation(checkpoint, receipt, &fresh, account),
        )
        .await?;
        Ok((fresh, captured_at, progress, turn))
    }
}

impl AgentBrowserSession {
    pub(super) fn settle_navigation_terminal(
        &mut self,
        terminal: &ContextNavigationSettlement,
    ) -> Result<AgentNavigationReceipt, AgentWorkFailure> {
        let now = self.policy_now().map_err(AgentWorkFailure::Browser)?;
        let active = self.navigation.as_ref().ok_or(AgentWorkFailure::Contract)?;
        let receipt = self
            .policy
            .settle_navigation(active, terminal, now)
            .map_err(|error| {
                AgentWorkFailure::Browser(AgentBrowserProviderError::Navigation(error))
            })?;
        self.record_navigation_terminal(receipt)
    }

    fn settle_navigation_refusal(
        &mut self,
        failure: ContextPortFailure,
    ) -> Result<AgentNavigationReceipt, AgentWorkFailure> {
        let now = self.policy_now().map_err(AgentWorkFailure::Browser)?;
        let active = self.navigation.as_ref().ok_or(AgentWorkFailure::Contract)?;
        let receipt = self
            .policy
            .refuse_navigation_dispatch(active, failure, now)
            .map_err(|error| {
                AgentWorkFailure::Browser(AgentBrowserProviderError::Navigation(error))
            })?;
        self.record_navigation_terminal(receipt)
    }

    fn record_navigation_terminal(
        &mut self,
        receipt: AgentNavigationReceipt,
    ) -> Result<AgentNavigationReceipt, AgentWorkFailure> {
        self.navigation_receipt = Some(receipt);
        self.journal
            .as_mut()
            .ok_or(AgentWorkFailure::Contract)?
            .navigation_settled(receipt)?;
        self.navigation.take();
        self.navigation_receipt.take();
        Ok(receipt)
    }

    async fn continue_after_navigation(
        &mut self,
        checkpoint: AgentProviderNavigationCheckpoint,
        receipt: AgentNavigationReceipt,
        observation: &SemanticObservation,
        account: AgentContextAccountBinding,
    ) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
        self.check_live()?;
        if self.turns >= super::super::MAX_BROWSER_MODEL_TURNS {
            return Err(AgentBrowserProviderError::TurnLimit);
        }
        if receipt.source() != self.account.context()
            || account.observed_at() < receipt.settled_at()
            || account.attestation() == self.account.attestation()
        {
            return Err(AgentBrowserProviderError::Account(
                super::super::AgentBrowserAccountError::ContextChanged,
            ));
        }
        self.validate_account_update(account, receipt.operation().context())?;
        let provisional = self.next_model_call_request()?;
        let request = AgentModelCallRequest::new(
            provisional.id(),
            provisional.lease(),
            account,
            provisional.budget(),
            provisional.now(),
        );
        checkpoint
            .validate_successor(receipt, observation, request, &self.config)
            .map_err(|_| AgentBrowserProviderError::Continuation)?;
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
        let prepared = AgentPreparedObservationRequest::try_openai_for_provider_exact_count(
            &mut self.policy,
            request,
            observation,
            payload,
            objective,
            self.config.clone(),
        )
        .map_err(|_| AgentBrowserProviderError::Authority)?;
        self.account_attestations.push(account.attestation());
        self.account = account;
        self.drive(prepared.into_transport_input()).await
    }
}

impl WorkJournal {
    fn navigation_active(
        &mut self,
        active: &AgentActiveNavigation,
    ) -> Result<(), AgentWorkFailure> {
        self.supervisor
            .record_active_navigation(
                self.execution.as_ref().ok_or(AgentWorkFailure::Contract)?,
                active,
            )
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.record()?;
        self.emit(AgentWorkEventKind::ToolProposed(
            AgentBrowserToolKind::Navigate,
        ))
    }

    fn navigation_settled(
        &mut self,
        receipt: AgentNavigationReceipt,
    ) -> Result<(), AgentWorkFailure> {
        self.accounting
            .record_navigation_receipt(receipt)
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.supervisor
            .record_navigation_result(
                self.execution.as_ref().ok_or(AgentWorkFailure::Contract)?,
                receipt,
            )
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.record()
    }
}
