//! Task-scoped document checkpoints through the original Work owners.

use super::*;

pub(super) struct NavigationTerminal {
    receipt: AgentNavigationReceipt,
    journal_failure: Option<AgentWorkFailure>,
}

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
        browser: &WorkBrowser<'_>,
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
        let discovery = state.navigation_discovery.as_ref();
        let expected = if discovery.is_some() {
            AgentWorkTaskProgress::Continue
        } else {
            AgentWorkTaskProgress::ReadyForNavigation
        };
        if state.navigation_complete() || progress != expected {
            return Err(AgentWorkFailure::TaskPhase {
                expected: progress,
                proposed: AgentBrowserToolKind::Navigate,
            });
        }
        let target = if let Some(scope) = discovery {
            let AgentBrowserToolProposal::Navigate(target) = turn.turn.proposal() else {
                return Err(AgentWorkFailure::Contract);
            };
            if !scope.admits(target) {
                return Err(AgentWorkFailure::Contract);
            }
            target.clone()
        } else {
            state
                .current_navigation_target()
                .cloned()
                .ok_or(AgentWorkFailure::Contract)?
        };
        if !matches!(turn.turn.proposal(), AgentBrowserToolProposal::Navigate(proposed) if proposed == &target)
        {
            return Err(AgentWorkFailure::Contract);
        }
        let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
        // Keep the original total ceiling and reserve the remaining exact route
        // proposals plus final extraction/mapping; no hop receives a new budget.
        let remaining_hops = if discovery.is_some() {
            1
        } else {
            state
                .navigation_length()
                .checked_sub(state.navigation_hops)
                .ok_or(AgentWorkFailure::Contract)?
        };
        if usize::from(session.turns) + remaining_hops + 1 > usize::from(session.max_model_calls) {
            return Err(AgentWorkFailure::Browser(
                AgentBrowserProviderError::TurnLimit,
            ));
        }
        state.refresh_account(worker, browser)?;
        state.journal_mut()?.emit(AgentWorkEventKind::ToolProposed(
            AgentBrowserToolKind::Navigate,
        ))?;
        state.native.check_control(worker, browser)?;
        state.check_task_contract()?;
        let id = state.native.identity.id();
        let automation = if let Some(retained) = &state.native.retained {
            let now = state
                .session
                .as_mut()
                .ok_or(AgentWorkFailure::Contract)?
                .policy_now()
                .map_err(AgentWorkFailure::Browser)?;
            retained.automation_state(now)?
        } else {
            state
                .native
                .contexts()?
                .automation_state(id)
                .map_err(|_| AgentWorkFailure::Context)?
        };
        let legacy_operation = if state.native.retained.is_none() {
            Some(ContextOperationId::new(state.native.id()?).ok_or(AgentWorkFailure::Contract)?)
        } else {
            None
        };
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
        let prepared = if let Some(retained) = &mut state.native.retained {
            retained.prepare_navigation(automation.context(), now)
        } else {
            let op = legacy_operation.ok_or(AgentWorkFailure::Contract)?;
            state
                .native
                .contexts()?
                .begin_navigation(id, op)
                .map_err(|_| AgentWorkFailure::Context)
        };
        let operation = match prepared {
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
            });
        let active = match active {
            Ok(active) => active,
            Err(error) => {
                if let Some(retained) = &mut state.native.retained {
                    let _ = retained.cancel_navigation_preparation();
                }
                return Err(error);
            }
        };
        session.navigation = Some(active);
        let active = session
            .navigation
            .as_ref()
            .ok_or(AgentWorkFailure::Contract)?;
        state.native.operation = Some(operation);
        let dispatch = if let Some(retained) = &mut state.native.retained {
            retained.dispatch_navigation(active)
        } else {
            let request = active
                .native_request()
                .map_err(|_| AgentWorkFailure::Contract)?;
            browser.dispatch(ContextNativeRequest::Navigate(request))
        };
        let refusal = match dispatch {
            ContextDispatch::Rejected(failure) => Some(failure),
            ContextDispatch::Unsupported => Some(ContextPortFailure::Unsupported),
            ContextDispatch::Scheduled => None,
        };
        if let Some(failure) = refusal {
            // The port's exact decision is terminal evidence even when a later
            // clock/audit operation fails. It is not a callback or new permit.
            session.navigation_refusal =
                Some(super::super::AgentBrowserNavigationDispatchRefusal { operation, failure });
        }
        // Both original owners exist before dispatch. Audit/progress recording
        // is fallible, so do it before processing callbacks, but only after the
        // port has either accepted a real callback debt or explicitly refused.
        // A local journal error must never strand an undispatched successor.
        let journal_failure = session
            .journal
            .as_mut()
            .ok_or(AgentWorkFailure::Contract)
            .and_then(|journal| journal.navigation_active(active))
            .err();
        if let Some(failure) = refusal {
            state.native.operation = None;
            if state.native.retained.is_none() {
                state
                    .native
                    .contexts()?
                    .settle_navigation(id, operation, ContextSettlement::Refused)
                    .map_err(|_| AgentWorkFailure::Context)?;
            }
            let terminal = state
                .session
                .as_mut()
                .ok_or(AgentWorkFailure::Contract)?
                .settle_navigation_refusal();
            let terminal = match terminal {
                Ok(terminal) => terminal,
                Err(failure) => return Err(journal_failure.unwrap_or(failure)),
            };
            return Err(journal_failure
                .or(terminal.journal_failure)
                .unwrap_or(AgentWorkFailure::Native(failure)));
        }
        if let Some(failure) = journal_failure {
            return Err(failure);
        }
        let receipt = match state.native.next_navigation_event(worker, browser).await? {
            AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::NavigationSettled(terminal))
                if terminal.operation() == operation =>
            {
                let result = state
                    .session
                    .as_mut()
                    .ok_or(AgentWorkFailure::Contract)?
                    .settle_navigation_terminal(&terminal);
                let terminal_record = match result {
                    Ok(terminal_record) => terminal_record,
                    Err(failure) => {
                        state.native.retain(AgentRuntimeEvent::NativeTerminal(
                            ContextNativeEvent::NavigationSettled(terminal),
                        ))?;
                        return Err(failure);
                    }
                };
                state.native.operation = None;
                if state.native.retained.is_none() {
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
                }
                if let Some(failure) = terminal_record.journal_failure {
                    return Err(failure);
                }
                let receipt = terminal_record.receipt;
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
        if receipt.hop() != state.navigation_hops {
            return Err(AgentWorkFailure::Contract);
        }
        state.navigation_hops += 1;
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
    ) -> Result<NavigationTerminal, AgentWorkFailure> {
        if self.navigation_refusal.is_some() {
            return Err(AgentWorkFailure::Contract);
        }
        let now = self.policy_now().map_err(AgentWorkFailure::Browser)?;
        let active = self.navigation.as_ref().ok_or(AgentWorkFailure::Contract)?;
        let receipt = self
            .policy
            .settle_navigation(active, terminal, now)
            .map_err(|error| {
                AgentWorkFailure::Browser(AgentBrowserProviderError::Navigation(error))
            })?;
        Ok(self.record_navigation_terminal(receipt))
    }

    pub(super) fn settle_navigation_refusal(
        &mut self,
    ) -> Result<NavigationTerminal, AgentWorkFailure> {
        let refusal = self.navigation_refusal.ok_or(AgentWorkFailure::Contract)?;
        if self
            .navigation
            .as_ref()
            .map(AgentActiveNavigation::operation)
            != Some(refusal.operation)
        {
            return Err(AgentWorkFailure::Contract);
        }
        let now = self.policy_now().map_err(AgentWorkFailure::Browser)?;
        let active = self.navigation.as_ref().ok_or(AgentWorkFailure::Contract)?;
        let receipt = self
            .policy
            .refuse_navigation_dispatch(active, refusal.failure, now)
            .map_err(|error| {
                AgentWorkFailure::Browser(AgentBrowserProviderError::Navigation(error))
            })?;
        self.navigation_refusal.take();
        Ok(self.record_navigation_terminal(receipt))
    }

    fn record_navigation_terminal(
        &mut self,
        receipt: AgentNavigationReceipt,
    ) -> NavigationTerminal {
        self.navigation_receipt = Some(receipt);
        self.navigation.take();
        let journal_failure = self
            .journal
            .as_mut()
            .ok_or(AgentWorkFailure::Contract)
            .and_then(|journal| journal.navigation_settled(receipt))
            .err();
        if journal_failure.is_none() {
            self.navigation_receipt.take();
        }
        NavigationTerminal {
            receipt,
            journal_failure,
        }
    }

    async fn continue_after_navigation(
        &mut self,
        checkpoint: AgentProviderNavigationCheckpoint,
        receipt: AgentNavigationReceipt,
        observation: &SemanticObservation,
        account: AgentContextAccountBinding,
    ) -> Result<AgentBrowserProviderTurn, AgentBrowserProviderError> {
        self.check_live()?;
        if self.turns >= self.max_model_calls {
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
        self.record()
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
