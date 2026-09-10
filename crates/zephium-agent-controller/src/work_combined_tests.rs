//! Combined tasks use the same fixture worker, native effect and provider ports.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CombinedFault {
    None,
    BoundaryAfterAction,
    Premature,
    ExtraAction,
    WrongSchema,
    ExpandedScope,
    ForeignSource,
    CancelMapCount,
    CancelMapStream,
    AuditLost,
    SchemaMutation,
    RoleMutation,
    ModeMutation,
    CompleteWithoutResult,
    Ceiling,
    ResultRefused,
    ActionLost,
}

#[test]
fn verified_action_with_changed_boundary_continues_with_fresh_evidence() {
    let _serial = lock(&SERIAL);
    provider_fixture(ProviderFault::Combined(CombinedFault::BoundaryAfterAction));
}

pub(super) struct CombinedTask {
    pub(super) extraction: AgentWorkExtractionTask,
    pub(super) fault: CombinedFault,
    pub(super) ready: Option<SemanticObservationId>,
}

impl AgentWorkTask for CombinedTask {
    fn allows_actions_before_extraction(&self) -> bool {
        self.fault != CombinedFault::ModeMutation || self.ready.is_none()
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.extraction.extraction_schema()
    }
    fn model_action_operations(
        &self,
        node: &SemanticNode,
        _: &SemanticObservation,
    ) -> Result<SemanticOperations, AgentWorkFailure> {
        if self.ready.is_none()
            && node.name().is_some_and(|name| name.as_str() == "Field")
            && node.operations().contains(SemanticOperationClass::Fill)
        {
            SemanticOperations::try_new(&[SemanticOperationClass::Fill])
                .map_err(|_| AgentWorkFailure::Contract)
        } else {
            Ok(SemanticOperations::NONE)
        }
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let prepared = observation.frames().iter().flat_map(|frame| frame.nodes()).any(|node| {
            matches!(node.value(), Some(SemanticValueSummary::Text(value)) if !value.preview().truncated() && value.preview().text() == "fixture value")
        });
        if !prepared {
            return Ok(AgentWorkTaskProgress::Continue);
        }
        if self.fault == CombinedFault::SchemaMutation {
            self.extraction = AgentWorkExtractionTask::try_new(
                vec![
                    SemanticExtractionFieldSchema::try_text("different".into(), true, 64).unwrap(),
                ],
                AgentAccountScope::Anonymous,
            )
            .unwrap();
        }
        if self.fault == CombinedFault::RoleMutation {
            // Same schema identity and fields, changed only after the verified
            // action. A fresh task phase cannot widen/narrow admitted evidence.
            self.extraction.schema = self.extraction.schema.clone().with_source_roles(
                SemanticReadRoleSelection::try_new(&[SemanticRole::Textbox]).unwrap(),
            );
        }
        self.ready = Some(observation.request().id());
        if self.fault == CombinedFault::CompleteWithoutResult {
            return Ok(AgentWorkTaskProgress::Complete);
        }
        Ok(AgentWorkTaskProgress::ReadyForExtraction)
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        assert!(self.ready.is_none(), "no action after readiness");
        Ok(AgentEffectAssessment::new(
            action,
            action.frame().origin().clone(),
            SemanticEffectClass::LocalWrite,
        ))
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        Task.attest_account(context, now)
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        assert_eq!(Some(result.observation()), self.ready);
        assert_ne!(result.observation(), SemanticObservationId::new(1).unwrap());
        if self.fault == CombinedFault::ResultRefused {
            return Err(AgentWorkFailure::Contract);
        }
        self.extraction.accept_extraction(result)
    }
}

#[test]
fn combined_actions_and_results_keep_fresh_phase_and_original_failure_owners() {
    let _serial = lock(&SERIAL);
    for fault in [
        CombinedFault::None,
        CombinedFault::Premature,
        CombinedFault::ExtraAction,
        CombinedFault::WrongSchema,
        CombinedFault::ExpandedScope,
        CombinedFault::ForeignSource,
        CombinedFault::CancelMapCount,
        CombinedFault::CancelMapStream,
        CombinedFault::AuditLost,
        CombinedFault::SchemaMutation,
        CombinedFault::RoleMutation,
        CombinedFault::ModeMutation,
        CombinedFault::CompleteWithoutResult,
        CombinedFault::Ceiling,
        CombinedFault::ResultRefused,
        CombinedFault::ActionLost,
    ] {
        provider_fixture(ProviderFault::Combined(fault));
    }
}

pub(super) fn assert_outcome(
    fault: CombinedFault,
    outcome: AgentWorkOutcome,
    shutdown: AgentBrowserShutdownOutcome,
    calls: &[u8],
    events: &[AgentWorkEvent],
) {
    let effects = u32::from(fault != CombinedFault::Premature);
    assert_eq!(
        calls.iter().filter(|call| **call == 7).count(),
        effects as usize
    );
    if matches!(
        fault,
        CombinedFault::None | CombinedFault::Ceiling | CombinedFault::BoundaryAfterAction
    ) {
        let AgentWorkOutcome::Succeeded(mut success) = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(success.closure().effects(), 1);
        assert_eq!(
            success.closure().model_calls(),
            if fault == CombinedFault::Ceiling {
                8
            } else {
                3
            }
        );
        let result = success.take_extraction().unwrap();
        assert_ne!(result.observation(), SemanticObservationId::new(1).unwrap());
        assert!(success.take_extraction().is_none());
        if fault == CombinedFault::None {
            assert_eq!(calls, [1, 2, 3, 7, 3, 4, 5, 6]);
        }
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
    } else if matches!(fault, CombinedFault::AuditLost | CombinedFault::ActionLost) {
        let AgentWorkOutcome::Recovery(recovery) = outcome else {
            panic!("{fault:?}: {outcome:?}");
        };
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Unclean));
        assert!(!events
            .iter()
            .any(|event| event.kind() == AgentWorkEventKind::Terminal));
        if fault == CombinedFault::ActionLost {
            assert!(recovery.state.extraction.is_none());
            assert!(
                recovery.retained_callbacks() > 0
                    || recovery
                        .state
                        .session
                        .as_ref()
                        .is_some_and(|session| session.action.is_some())
            );
        } else {
            assert!(recovery.state.extraction.is_some());
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
            panic!("{fault:?}: {outcome:?}");
        };
        assert_eq!(closed.policy_settlement().closure().effects(), effects);
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        match fault {
            CombinedFault::Premature => assert_eq!(
                closed.failure(),
                AgentWorkFailure::TaskPhase {
                    expected: AgentWorkTaskProgress::Continue,
                    proposed: AgentBrowserToolKind::Extract
                }
            ),
            CombinedFault::ExtraAction => assert_eq!(
                closed.failure(),
                AgentWorkFailure::TaskPhase {
                    expected: AgentWorkTaskProgress::ReadyForExtraction,
                    proposed: AgentBrowserToolKind::Act
                }
            ),
            CombinedFault::SchemaMutation
            | CombinedFault::RoleMutation
            | CombinedFault::ModeMutation
            | CombinedFault::CompleteWithoutResult
            | CombinedFault::ResultRefused => {
                assert_eq!(closed.failure(), AgentWorkFailure::Contract)
            }
            CombinedFault::CancelMapCount | CombinedFault::CancelMapStream => {
                assert_eq!(closed.failure(), AgentWorkFailure::HumanTakeover)
            }
            _ => assert!(matches!(closed.failure(), AgentWorkFailure::Browser(_))),
        }
    }
}
