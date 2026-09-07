//! Nonterminal reads use the real actor, transport, runtime and form predicate.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReadFault {
    None,
    Extraction,
    AfterActionExtraction,
    Disabled,
    Subtree,
    Ceiling,
    CountRefused,
    StreamRefused,
    CancelCount,
    TakeoverStream,
    SuspendCount,
    ActionLost,
    AuditLost,
}
impl ReadFault {
    pub(super) fn requests(self) -> u8 {
        match self {
            Self::AfterActionExtraction => 8,
            Self::Disabled | Self::Subtree => 2,
            Self::Ceiling => 16,
            Self::CountRefused | Self::CancelCount | Self::SuspendCount => 3,
            Self::StreamRefused | Self::TakeoverStream => 4,
            _ => 6,
        }
    }
    pub(super) fn stop(self, turns: u8, count: bool) -> Option<AgentRuntimeStopReason> {
        match (self, turns, count) {
            (Self::CancelCount, 1, true) => Some(AgentRuntimeStopReason::Cancelled),
            (Self::TakeoverStream, 1, false) => Some(AgentRuntimeStopReason::HumanTakeover),
            (Self::SuspendCount, 1, true) => Some(AgentRuntimeStopReason::Suspend),
            _ => None,
        }
    }
    pub(super) fn refused(self, turns: u8, count: bool) -> bool {
        turns == 1
            && matches!(
                (self, count),
                (Self::CountRefused, true) | (Self::StreamRefused, false)
            )
    }
    pub(super) fn stream(self, turn: u8) -> String {
        if matches!(self, Self::Extraction | Self::AfterActionExtraction) {
            let turn_in_read = if self == Self::AfterActionExtraction {
                if turn == 1 {
                    return tool_stream(turn, true);
                }
                turn - 1
            } else {
                turn
            };
            return match turn_in_read {
                1 => named_tool_stream(turn, "read", r#"{\"scope\":{\"kind\":\"initial\"}}"#),
                2 => named_tool_stream(
                    turn,
                    "extract",
                    r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
                ),
                _ => extraction_stream(ExtractionFault::None)
                    .replace("resp_2", &format!("resp_{turn}"))
                    .replace("msg_2", &format!("msg_{turn}")),
            };
        }
        if self == Self::Subtree {
            named_tool_stream(
                turn,
                "read",
                r#"{\"scope\":{\"kind\":\"subtree\",\"target\":\"@a2\"}}"#,
            )
        } else if turn <= 2 || self == Self::Ceiling {
            named_tool_stream(turn, "read", r#"{\"scope\":{\"kind\":\"initial\"}}"#)
        } else {
            tool_stream(turn, true)
        }
    }
    pub(super) fn check_request(self, body: &[u8], turns: u8) {
        let body = std::str::from_utf8(body).unwrap();
        if turns == 0 {
            assert_eq!(body.contains("\"name\":\"read\""), self != Self::Disabled);
        }
        if turns > u8::from(self == Self::AfterActionExtraction) {
            assert!(body.contains("ZREAD3 content=untrusted"));
            assert!(body.contains("Field"));
        }
    }
}

pub(super) struct ReadTask {
    inner: Box<dyn AgentWorkTask>,
    enabled: bool,
}
impl ReadTask {
    pub(super) fn new(input: &AgentWorkRunInput, fault: ReadFault) -> Self {
        let form = crate::AgentWorkFormTask::try_new_local_preparation(
            input.context.identity,
            input.context.origin.clone(),
            AgentAccountScope::Anonymous,
            vec![
                crate::AgentWorkFormPhase::try_new(vec![crate::AgentWorkFormGoal::fill(
                    Some("Field".into()),
                    "fixture value".into(),
                )
                .unwrap()])
                .unwrap(),
            ],
        )
        .unwrap()
        .with_baseline_read();
        assert!(form.allows_baseline_read());
        let extraction = || {
            AgentWorkExtractionTask::try_new(
                vec![SemanticExtractionFieldSchema::try_text("label".into(), true, 64).unwrap()],
                AgentAccountScope::Anonymous,
            )
            .unwrap()
        };
        let inner: Box<dyn AgentWorkTask> = match fault {
            ReadFault::AfterActionExtraction => Box::new(CombinedTask {
                extraction: extraction(),
                fault: CombinedFault::None,
                ready: None,
            }),
            ReadFault::Extraction => Box::new(extraction().with_baseline_read()),
            _ => Box::new(form),
        };
        Self {
            inner,
            enabled: fault != ReadFault::Disabled,
        }
    }
}
impl AgentWorkTask for ReadTask {
    fn allows_actions_before_extraction(&self) -> bool {
        self.inner.allows_actions_before_extraction()
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.inner.extraction_schema()
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.inner.accept_extraction(result)
    }
    fn allows_baseline_read(&self) -> bool {
        self.enabled
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.inner.evaluate(observation)
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        self.inner.assess(action)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        self.inner.attest_account(context, now)
    }
}

#[test]
fn read_continuations_preserve_baseline_budget_stop_and_cleanup_ownership() {
    let _serial = lock(&SERIAL);
    for fault in [
        ReadFault::None,
        ReadFault::Extraction,
        ReadFault::AfterActionExtraction,
        ReadFault::Disabled,
        ReadFault::Subtree,
        ReadFault::Ceiling,
        ReadFault::CountRefused,
        ReadFault::StreamRefused,
        ReadFault::CancelCount,
        ReadFault::TakeoverStream,
        ReadFault::SuspendCount,
        ReadFault::ActionLost,
        ReadFault::AuditLost,
    ] {
        provider_fixture(ProviderFault::Read(fault));
    }
}
pub(super) fn assert_outcome(
    fault: ReadFault,
    outcome: AgentWorkOutcome,
    shutdown: AgentBrowserShutdownOutcome,
    calls: &[u8],
    events: &[AgentWorkEvent],
) {
    if matches!(
        fault,
        ReadFault::Extraction | ReadFault::AfterActionExtraction
    ) {
        let AgentWorkOutcome::Succeeded(mut success) = outcome else {
            panic!("{fault:?}: {outcome:?}")
        };
        let actions = u32::from(fault == ReadFault::AfterActionExtraction);
        assert_eq!(success.closure().effects(), actions);
        assert_eq!(success.closure().model_calls(), 3 + actions);
        let result = success.take_extraction().unwrap();
        if actions == 1 {
            assert_ne!(result.observation(), SemanticObservationId::new(1).unwrap());
        }
        assert_eq!(
            calls.iter().filter(|call| **call == 3).count(),
            1 + actions as usize
        );
        assert_eq!(
            calls.iter().filter(|call| **call == 7).count(),
            actions as usize
        );
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
    } else if fault == ReadFault::None {
        let AgentWorkOutcome::Succeeded(success) = outcome else {
            panic!("{fault:?}: {outcome:?}")
        };
        assert_eq!(success.closure().model_calls(), 3);
        assert_eq!(success.closure().effects(), 1);
        assert_eq!(calls, [1, 2, 3, 7, 3, 4, 5, 6]);
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind()
                    == AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Read))
                .count(),
            2
        );
    } else if matches!(fault, ReadFault::ActionLost | ReadFault::AuditLost) {
        let AgentWorkOutcome::Recovery(recovery) = outcome else {
            panic!("{fault:?}: {outcome:?}")
        };
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Unclean));
        assert!(!events
            .iter()
            .any(|event| event.kind() == AgentWorkEventKind::Terminal));
        if fault == ReadFault::ActionLost {
            assert!(
                recovery.retained_callbacks() > 0
                    || recovery
                        .state
                        .session
                        .as_ref()
                        .is_some_and(|session| session.action.is_some())
            );
        } else {
            assert!(
                recovery
                    .state
                    .drained
                    .as_ref()
                    .unwrap()
                    .journal
                    .as_ref()
                    .unwrap()
                    .audit
                    .status()
                    .in_flight()
                    > 0
            );
        }
    } else {
        let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
            panic!("{fault:?}: {outcome:?}")
        };
        assert_eq!(closed.policy_settlement().closure().effects(), 0);
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        assert_eq!(calls.iter().filter(|call| **call == 3).count(), 1);
        assert!(!calls.contains(&7));
        let stop = match fault {
            ReadFault::CancelCount => Some(AgentWorkFailure::Cancelled),
            ReadFault::TakeoverStream => Some(AgentWorkFailure::HumanTakeover),
            ReadFault::SuspendCount => Some(AgentWorkFailure::SuspendRequested),
            _ => None,
        };
        if let Some(stop) = stop {
            assert_eq!(closed.failure(), stop);
        }
        if fault == ReadFault::Ceiling {
            assert_eq!(
                closed.failure(),
                AgentWorkFailure::Browser(AgentBrowserProviderError::TurnLimit)
            );
            assert_eq!(closed.policy_settlement().closure().model_calls(), 8);
        }
    }
}
