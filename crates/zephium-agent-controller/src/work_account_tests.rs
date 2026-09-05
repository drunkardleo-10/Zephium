//! Trusted account sampling against the actual actor/transport/mailbox path.
use super::*;
use crate::{AgentBrowserAccountError, AgentBrowserProviderError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AccountFault {
    Slow,
    Static,
    Cached,
    Changed,
    Future,
    Regressed,
    Rewritten,
    ChangedAfterAction,
    Refused,
    Cancel,
    CancelRefused,
    Takeover,
    Suspend,
    Revoke,
}

pub(super) struct AccountTask {
    task: Box<dyn AgentWorkTask>,
    fault: AccountFault,
    sample: Mutex<(u8, Option<AgentContextAccountBinding>)>,
    control: Arc<Mutex<Option<AgentRuntimeHandle>>>,
}
impl AccountTask {
    pub(super) fn new(
        task: Box<dyn AgentWorkTask>,
        fault: AccountFault,
        control: Arc<Mutex<Option<AgentRuntimeHandle>>>,
    ) -> Self {
        Self {
            task,
            fault,
            sample: Mutex::new((0, None)),
            control,
        }
    }
}
impl AgentWorkTask for AccountTask {
    fn navigation_target(&self) -> Option<&ContextNavigationTarget> {
        self.task.navigation_target()
    }
    fn allows_baseline_read(&self) -> bool {
        self.task.allows_baseline_read()
    }
    fn allows_subtree_extraction(&self) -> bool {
        self.task.allows_subtree_extraction()
    }
    fn allows_actions_before_extraction(&self) -> bool {
        self.task.allows_actions_before_extraction()
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.task.extraction_schema()
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.task.evaluate(observation)
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        self.task.assess(action)
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.task.accept_extraction(result)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        let declared = self.task.attest_account(context, now)?;
        assert_eq!(declared.account(), AgentAccountScope::Anonymous);
        // This fixture is the trusted account source for its synthetic owned
        // anonymous document; the built-in predicate must not renew its scope.
        let current = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            AgentAccountScope::Anonymous,
            now,
        );
        let mut sample = lock(&self.sample);
        sample.0 += 1;
        if sample.0
            >= if self.fault == AccountFault::ChangedAfterAction {
                4
            } else {
                3
            }
        {
            let previous = sample.1.unwrap();
            let stop = match self.fault {
                AccountFault::Cancel | AccountFault::CancelRefused => {
                    Some(AgentRuntimeStopReason::Cancelled)
                }
                AccountFault::Takeover => Some(AgentRuntimeStopReason::HumanTakeover),
                AccountFault::Suspend => Some(AgentRuntimeStopReason::Suspend),
                AccountFault::Revoke => Some(AgentRuntimeStopReason::PolicyRevoked),
                _ => None,
            };
            if let Some(reason) = stop {
                lock(&self.control).as_ref().unwrap().stop_and_seal(reason);
            }
            return match self.fault {
                AccountFault::Cached => Ok(previous),
                AccountFault::Changed | AccountFault::ChangedAfterAction => {
                    Ok(AgentContextAccountBinding::new(
                        current.attestation(),
                        context,
                        AgentAccountScope::Authenticated(AgentAccountId::generate()),
                        now,
                    ))
                }
                AccountFault::Future => Ok(AgentContextAccountBinding::new(
                    current.attestation(),
                    context,
                    current.account(),
                    AgentPolicyInstant::from_millis(now.millis() + 1_000),
                )),
                AccountFault::Regressed => Ok(AgentContextAccountBinding::new(
                    current.attestation(),
                    context,
                    current.account(),
                    AgentPolicyInstant::from_millis(previous.observed_at().millis() - 1),
                )),
                AccountFault::Rewritten => Ok(AgentContextAccountBinding::new(
                    previous.attestation(),
                    context,
                    current.account(),
                    now,
                )),
                AccountFault::Refused | AccountFault::CancelRefused => {
                    Err(AgentWorkFailure::Contract)
                }
                _ => {
                    sample.1 = Some(current);
                    Ok(current)
                }
            };
        }
        sample.1 = Some(current);
        Ok(current)
    }
}

#[test]
fn slow_provider_turns_resample_each_shipping_inspection_action_and_mapping_boundary() {
    let _serial = lock(&SERIAL);
    for (fault, form) in [
        (
            ProviderFault::Native(Fault::ActionApplied),
            Some("fixture value"),
        ),
        (ProviderFault::Read(ReadFault::None), None),
        (ProviderFault::Read(ReadFault::AfterActionExtraction), None),
        (ProviderFault::Combined(CombinedFault::None), None),
        (ProviderFault::Scoped(ScopedFault::None), None),
        (ProviderFault::Scoped(ScopedFault::ParagraphSelection), None),
        (ProviderFault::Ceiling, None),
        (ProviderFault::Read(ReadFault::ActionLost), None),
        (ProviderFault::Combined(CombinedFault::AuditLost), None),
    ] {
        provider_fixture_with_account(fault, form, Some(AccountFault::Slow));
    }
}

#[test]
fn fresh_account_refusal_or_control_during_sampling_cannot_dispatch_an_action() {
    let _serial = lock(&SERIAL);
    for fault in [
        AccountFault::Cached,
        AccountFault::Static,
        AccountFault::Changed,
        AccountFault::Future,
        AccountFault::Regressed,
        AccountFault::Rewritten,
        AccountFault::Refused,
        AccountFault::Cancel,
        AccountFault::CancelRefused,
        AccountFault::Takeover,
        AccountFault::Suspend,
        AccountFault::Revoke,
    ] {
        provider_fixture_with_account(
            ProviderFault::Native(Fault::ActionApplied),
            Some("fixture value"),
            Some(fault),
        );
    }
    for fault in [
        ProviderFault::Read(ReadFault::None),
        ProviderFault::Extraction(ExtractionFault::None),
    ] {
        provider_fixture_with_account(fault, None, Some(AccountFault::Cached));
    }
    provider_fixture_with_account(
        ProviderFault::Combined(CombinedFault::None),
        None,
        Some(AccountFault::ChangedAfterAction),
    );
    for fault in [
        AccountFault::Changed,
        AccountFault::Cancel,
        AccountFault::Revoke,
    ] {
        provider_fixture_with_account(
            ProviderFault::Scoped(ScopedFault::ParagraphSelection),
            None,
            Some(fault),
        );
    }
}

pub(super) fn assert_refusal(
    fault: AccountFault,
    outcome: AgentWorkOutcome,
    shutdown: AgentBrowserShutdownOutcome,
    calls: &[u8],
    subtree: bool,
) {
    let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
        panic!("{fault:?}: {outcome:?}")
    };
    let expected = match fault {
        AccountFault::Cached | AccountFault::Static => AgentWorkFailure::Browser(
            AgentBrowserProviderError::Account(AgentBrowserAccountError::Stale),
        ),
        AccountFault::Changed | AccountFault::ChangedAfterAction => AgentWorkFailure::Browser(
            AgentBrowserProviderError::Account(AgentBrowserAccountError::AccountChanged),
        ),
        AccountFault::Future | AccountFault::Regressed => AgentWorkFailure::Browser(
            AgentBrowserProviderError::Account(AgentBrowserAccountError::Clock),
        ),
        AccountFault::Rewritten => AgentWorkFailure::Browser(AgentBrowserProviderError::Account(
            AgentBrowserAccountError::Rewritten,
        )),
        AccountFault::Refused => AgentWorkFailure::Contract,
        AccountFault::Cancel | AccountFault::CancelRefused => AgentWorkFailure::Cancelled,
        AccountFault::Takeover => AgentWorkFailure::HumanTakeover,
        AccountFault::Suspend => AgentWorkFailure::SuspendRequested,
        AccountFault::Revoke => AgentWorkFailure::PolicyRevoked,
        AccountFault::Slow => unreachable!(),
    };
    assert_eq!(closed.failure(), expected, "{fault:?}");
    if matches!(
        expected,
        AgentWorkFailure::Browser(AgentBrowserProviderError::Account(_))
    ) {
        assert_eq!(
            closed.policy_settlement().closure().outcome(),
            AgentRunProgressOutcome::Failed(AgentSupervisorFailure::PolicyDenied)
        );
    }
    assert_eq!(closed.policy_settlement().closure().model_calls(), 1);
    if fault == AccountFault::ChangedAfterAction {
        assert_eq!(closed.policy_settlement().closure().effects(), 1);
        assert_eq!(calls, [1, 2, 3, 7, 3, 4, 5, 6]);
    } else {
        assert_eq!(closed.policy_settlement().closure().effects(), 0);
        if subtree {
            assert_eq!(calls, [1, 2, 3, 3, 8, 4, 5, 6]);
        } else {
            assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
        }
    }
    assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
}
