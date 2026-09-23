use super::*;
use zephium_agentic::{
    AgentPolicyError, AgentProviderCallIdentity, AgentProviderInputMetricReceipt,
    DecisionCallAccounting, DecisionCallFailure, DecisionCredentialProvider, DecisionObservation,
    DecisionObservationAnswers, DecisionProjectionError, JevDecisionClient, OpenAiDecisionCall,
    WorkPlanningConfig,
};

const EMULATION_INPUT_TOKENS: u32 = 32_768;
const EMULATION_OUTPUT_TOKENS: u32 = 4_096;
const EMULATION_COST_MICRO_USD: u64 = 100_000;
const REMAINING_PAGE_MODEL_CALLS: u8 = 2;

/// Trusted provider configuration; credentials remain provider-bound and zeroizing.
pub enum AgentBrowserDecisionProvider {
    /// Use the same typed questions through the existing OpenAI credential.
    Emulation,
    /// Direct TypeSafe BYOK loaded by the trusted Keychain adapter.
    TypeSafe(AgentProviderCredential<DecisionCredentialProvider>),
    /// Trusted cloud-session endpoint and its separately bound bearer.
    Cloud {
        /// HTTPS service base URL supplied by the cloud session, never a page.
        base_url: String,
        /// Zeroizing cloud-session credential; a BYOK key is refused.
        credential: AgentProviderCredential<DecisionCredentialProvider>,
    },
}

pub(super) struct BrowserDecisions {
    jev: Option<JevDecisionClient>,
    emulation: WorkPlanningConfig,
}

struct Accounting<'a> {
    journal: Option<&'a mut work::WorkJournal>,
    receipts: &'a mut Vec<(AgentModelCallReceipt, AgentProviderInputMetricReceipt)>,
    turns: &'a mut u8,
    maximum: u8,
    failure: &'a mut Option<AgentBrowserProviderError>,
}

impl DecisionCallAccounting for Accounting<'_> {
    fn activated(&mut self, call: AgentProviderCallIdentity) -> bool {
        if *self.turns >= self.maximum {
            *self.failure = Some(AgentBrowserProviderError::TurnLimit);
            return false;
        }
        *self.turns += 1;
        if let Some(journal) = self.journal.as_mut() {
            if let Err(failure) = journal.model_active(call) {
                journal.failure.get_or_insert(failure);
                *self.failure = Some(AgentBrowserProviderError::Journal);
                return false;
            }
        }
        true
    }

    fn settled(&mut self, receipt: AgentModelCallReceipt, input: AgentProviderInputMetricReceipt) {
        self.receipts.push((receipt, input));
    }
}

impl AgentBrowserSession {
    pub(super) fn configure_decisions(
        &mut self,
        provider: AgentBrowserDecisionProvider,
    ) -> Result<(), AgentBrowserProviderError> {
        self.check_live()?;
        if self.turns != 0 || self.decisions.is_some() {
            return Err(AgentBrowserProviderError::Continuation);
        }
        let jev = match provider {
            AgentBrowserDecisionProvider::Emulation => None,
            AgentBrowserDecisionProvider::TypeSafe(credential) => Some(JevDecisionClient::direct(
                self.transport.0.clone(),
                credential,
            )),
            AgentBrowserDecisionProvider::Cloud {
                base_url,
                credential,
            } => Some(JevDecisionClient::cloud(
                self.transport.0.clone(),
                credential,
                &base_url,
            )),
        }
        .transpose();
        let jev = match jev {
            Ok(client) => client,
            Err(DecisionCallFailure::Unavailable) => None,
            Err(_) => return Err(AgentBrowserProviderError::Transport),
        };
        let config = zephium_agent_model_catalog::try_gpt6_luna_decision_call_config(
            EMULATION_OUTPUT_TOKENS,
            zephium_agent_model_catalog::Gpt6LunaDecisionEffort::Low,
        )
            .map_err(|_| AgentBrowserProviderError::Catalog)?;
        let emulation =
            WorkPlanningConfig::try_new(config, EMULATION_INPUT_TOKENS, EMULATION_COST_MICRO_USD)
                .map_err(|_| AgentBrowserProviderError::Catalog)?;
        self.decisions = Some(BrowserDecisions { jev, emulation });
        Ok(())
    }

    fn has_decision_capacity(&self) -> Result<bool, AgentBrowserProviderError> {
        Ok(
            self.turns.saturating_add(REMAINING_PAGE_MODEL_CALLS) < self.max_model_calls
                && self
                    .policy
                    .remaining_operations(self.lease.lease())
                    .map_err(AgentBrowserProviderError::RequestPolicy)?
                    > u32::from(REMAINING_PAGE_MODEL_CALLS),
        )
    }

    pub(super) async fn decide_observation(
        &mut self,
        observation: &zephium_agentic::SemanticObservation,
        authority: &zephium_agentic::AgentProviderActionAuthority,
        schema: Option<&zephium_agentic::SemanticExtractionSchema>,
        locate_only: bool,
    ) -> Result<Option<DecisionObservationAnswers>, AgentBrowserProviderError> {
        self.check_live()?;
        if self.decisions.is_none() || !self.has_decision_capacity()? {
            return Ok(None);
        }
        if self.attempt.is_some() || self.retained_terminal.is_some() || self.action.is_some() {
            return Err(AgentBrowserProviderError::ActionPending);
        }
        let objective = self
            .objective
            .as_ref()
            .ok_or(AgentBrowserProviderError::Continuation)?;
        let built = match schema.filter(|_| locate_only) {
            Some(schema) => DecisionObservation::try_for_locate(
                observation,
                objective,
                authority,
                self.account,
                schema,
            ),
            None => DecisionObservation::try_for_read(
                observation,
                objective,
                authority,
                self.account,
                schema,
            ),
        };
        let projection = match built {
            Ok(projection) => projection,
            Err(DecisionProjectionError::Capacity) => return Ok(None),
            Err(_) => return Err(AgentBrowserProviderError::Authority),
        };
        self.decide_projection(observation, projection).await
    }

    /// A catalog read's second batch: which node inside each found record
    /// holds each text column. It discloses only those records.
    pub(super) async fn decide_row_cells(
        &mut self,
        observation: &zephium_agentic::SemanticObservation,
        discovery: &zephium_agentic::DecisionRowDiscovery,
    ) -> Result<Option<DecisionObservationAnswers>, AgentBrowserProviderError> {
        self.check_live()?;
        if self.decisions.is_none() || !self.has_decision_capacity()? {
            return Ok(None);
        }
        if self.attempt.is_some() || self.retained_terminal.is_some() || self.action.is_some() {
            return Err(AgentBrowserProviderError::ActionPending);
        }
        let objective = self
            .objective
            .as_ref()
            .ok_or(AgentBrowserProviderError::Continuation)?;
        let projection = match DecisionObservation::try_for_row_cells(
            observation,
            objective,
            self.account,
            discovery,
        ) {
            Ok(projection) => projection,
            Err(DecisionProjectionError::Capacity) => return Ok(None),
            Err(_) => return Err(AgentBrowserProviderError::Authority),
        };
        self.decide_projection(observation, projection).await
    }

    async fn decide_projection(
        &mut self,
        observation: &zephium_agentic::SemanticObservation,
        projection: DecisionObservation,
    ) -> Result<Option<DecisionObservationAnswers>, AgentBrowserProviderError> {
        #[cfg(feature = "probe-harness")]
        projection.record_anonymous_eval_request();
        let primary = if self
            .decisions
            .as_ref()
            .is_some_and(|decisions| decisions.jev.is_some())
        {
            let call = self.next_model_call_request_with_budget(
                JevDecisionClient::call_budget()
                    .map_err(AgentBrowserProviderError::RequestPolicy)?,
            )?;
            let decisions = self
                .decisions
                .as_ref()
                .ok_or(AgentBrowserProviderError::Authority)?;
            let client = decisions
                .jev
                .as_ref()
                .ok_or(AgentBrowserProviderError::Authority)?;
            let mut accounting = Accounting {
                journal: self.journal.as_mut(),
                receipts: &mut self.model_receipts,
                turns: &mut self.turns,
                maximum: self.max_model_calls,
                failure: &mut self.failure,
            };
            let output = client
                .evaluate_observation_accounted(
                    &mut self.policy,
                    call,
                    observation,
                    &projection,
                    self.deadline,
                    &self.cancellation,
                    Some(&mut accounting),
                )
                .await;
            self.settle_decision(output)?
        } else {
            Err(DecisionCallFailure::Unavailable)
        };
        let fallback = projection
            .route(primary)
            .map_err(|_| AgentBrowserProviderError::Authority)?;
        let can_emulate = self.has_decision_capacity()?;
        if fallback.projection().is_some() {
            if let Some(journal) = &self.journal {
                journal
                    .emit(work::AgentWorkEventKind::DecisionFallback {
                        counts: fallback.fallback_counts(),
                        purposes: fallback.fallback_purposes(),
                        capacity: can_emulate,
                    })
                    .map_err(|_| AgentBrowserProviderError::Journal)?;
            }
        }
        let emulation = if let Some(projection) = fallback.projection().filter(|_| can_emulate) {
            self.check_live()?;
            let decisions = self
                .decisions
                .as_ref()
                .ok_or(AgentBrowserProviderError::Authority)?;
            let credential = self
                .credential
                .as_ref()
                .ok_or(AgentBrowserProviderError::Cancelled)?;
            let client =
                OpenAiDecisionCall::try_new(&self.transport, credential, &decisions.emulation)
                    .map_err(|_| AgentBrowserProviderError::Catalog)?;
            let budget = client
                .call_budget()
                .map_err(AgentBrowserProviderError::RequestPolicy)?;
            let call = self.next_model_call_request_with_budget(budget)?;
            let decisions = self
                .decisions
                .as_ref()
                .ok_or(AgentBrowserProviderError::Authority)?;
            let credential = self
                .credential
                .as_ref()
                .ok_or(AgentBrowserProviderError::Cancelled)?;
            let client =
                OpenAiDecisionCall::try_new(&self.transport, credential, &decisions.emulation)
                    .map_err(|_| AgentBrowserProviderError::Catalog)?;
            let mut accounting = Accounting {
                journal: self.journal.as_mut(),
                receipts: &mut self.model_receipts,
                turns: &mut self.turns,
                maximum: self.max_model_calls,
                failure: &mut self.failure,
            };
            let output = client
                .evaluate_observation(
                    &mut self.policy,
                    call,
                    observation,
                    projection,
                    self.deadline,
                    &self.cancellation,
                    Some(&mut accounting),
                )
                .await;
            self.settle_decision(output)?.ok()
        } else {
            None
        };
        self.check_live()?;
        Ok(Some(fallback.finish(emulation)))
    }

    fn settle_decision(
        &mut self,
        output: Result<zephium_agentic::AdmittedDecisionOutput, AgentPolicyError>,
    ) -> Result<
        Result<zephium_decision::DecisionResponse, DecisionCallFailure>,
        AgentBrowserProviderError,
    > {
        self.record_model_receipts()?;
        self.check_live()?;
        match output {
            Ok(output) => {
                if let Some(journal) = &self.journal {
                    journal
                        .emit(work::AgentWorkEventKind::DecisionSettled(
                            output.call.diagnostic,
                        ))
                        .map_err(|_| AgentBrowserProviderError::Journal)?;
                }
                Ok(output.call.response)
            }
            Err(AgentPolicyError::Budget) => Ok(Err(DecisionCallFailure::Capacity)),
            #[cfg(feature = "probe-harness")]
            Err(AgentPolicyError::ModelInputBudget { .. }) => {
                Ok(Err(DecisionCallFailure::Capacity))
            }
            Err(error) => Err(AgentBrowserProviderError::RequestPolicy(error)),
        }
    }
}
