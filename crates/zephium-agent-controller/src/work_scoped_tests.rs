//! Scoped extraction uses the shipping actor and exact native callback mailbox.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScopedFault {
    Ceiling,
    UnexpectedAction,
    None,
    ParagraphSelection,
    RoleMutation,
    AccountRoleMutation,
    ResultRoleMutation,
    EmbeddedFrame,
    Combined,
    UnknownRef,
    WrongSchema,
    NoGrant,
    RefusedCapture,
    StaleCapture,
    WrongRoot,
    LostCapture,
    TakeoverCapture,
    RendererCapture,
    CancelMapCount,
    CancelMapStream,
    ForeignSource,
    AuditLost,
    ModeMutation,
}

impl ScopedFault {
    pub(super) fn requests(self) -> u8 {
        match self {
            Self::Ceiling => 16,
            Self::UnexpectedAction => 2,
            Self::ModeMutation | Self::RoleMutation => 0,
            Self::AccountRoleMutation => 2,
            Self::Combined => 6,
            Self::UnknownRef
            | Self::WrongSchema
            | Self::NoGrant
            | Self::RefusedCapture
            | Self::StaleCapture
            | Self::WrongRoot
            | Self::LostCapture
            | Self::TakeoverCapture
            | Self::RendererCapture => 2,
            Self::CancelMapCount => 3,
            _ => 4,
        }
    }
    pub(super) fn cancelled(self, turns: u8, count: bool) -> bool {
        turns == 1
            && ((count && self == Self::CancelMapCount)
                || (!count && self == Self::CancelMapStream))
    }
    pub(super) fn stream(self, turns: u8) -> String {
        if self == Self::Ceiling && turns < 7 {
            return tool_stream(turns, false);
        }
        if self == Self::UnexpectedAction {
            return tool_stream(turns, true);
        }
        if self == Self::Combined && turns == 1 {
            return tool_stream(turns, true);
        }
        if (self == Self::Ceiling && turns == 7) || turns == 1 + u8::from(self == Self::Combined) {
            let arguments = match self {
                Self::UnknownRef => {
                    r#"{\"scope\":{\"kind\":\"subtree\",\"target\":\"@a99\"},\"schema_id\":1}"#
                }
                Self::WrongSchema => {
                    r#"{\"scope\":{\"kind\":\"subtree\",\"target\":\"@a3\"},\"schema_id\":2}"#
                }
                _ => r#"{\"scope\":{\"kind\":\"subtree\",\"target\":\"@a3\"},\"schema_id\":1}"#,
            };
            return named_tool_stream(turns, "extract", arguments);
        }
        extraction_stream(if self == Self::ForeignSource {
            ExtractionFault::ForeignSource
        } else {
            ExtractionFault::None
        })
        .replace("Field", "Expanded fixture result")
        .replace(
            "@r1",
            if self == Self::ParagraphSelection {
                "@r1"
            } else {
                "@r2"
            },
        )
        .replace("resp_2", &format!("resp_{turns}"))
        .replace("msg_2", &format!("msg_{turns}"))
    }
}

pub(super) struct ScopedTask {
    extraction: AgentWorkExtractionTask,
    fault: ScopedFault,
    observed: bool,
    ready: Option<SemanticObservationId>,
    account_samples: std::cell::Cell<u8>,
    alternate_schema: SemanticExtractionSchema,
}
impl ScopedTask {
    pub(super) fn new(fault: ScopedFault) -> Self {
        let mut extraction = AgentWorkExtractionTask::try_new(
            vec![SemanticExtractionFieldSchema::try_text("label".into(), true, 64).unwrap()],
            AgentAccountScope::Anonymous,
        )
        .unwrap();
        let alternate_schema = extraction.schema.clone();
        // Keep one default-all schedule. Exercise the remaining adversarial
        // actor paths with a frozen selection, including existing debt owners.
        if fault != ScopedFault::None {
            extraction = extraction.with_source_roles(
                SemanticReadRoleSelection::try_new(if fault == ScopedFault::ParagraphSelection {
                    &[SemanticRole::Paragraph]
                } else {
                    &[SemanticRole::Group, SemanticRole::Paragraph]
                })
                .unwrap(),
            );
        }
        Self {
            extraction,
            fault,
            observed: false,
            ready: None,
            account_samples: std::cell::Cell::new(0),
            alternate_schema,
        }
    }
}
impl AgentWorkTask for ScopedTask {
    fn allows_subtree_extraction(&self) -> bool {
        self.fault != ScopedFault::NoGrant
            && !(self.fault == ScopedFault::ModeMutation && self.observed)
    }
    fn allows_actions_before_extraction(&self) -> bool {
        self.fault == ScopedFault::Combined
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        if self.fault == ScopedFault::AccountRoleMutation && self.account_samples.get() >= 3 {
            Some(&self.alternate_schema)
        } else {
            self.extraction.extraction_schema()
        }
    }
    fn model_action_operations(
        &self,
        node: &SemanticNode,
        _: &SemanticObservation,
    ) -> Result<SemanticOperations, AgentWorkFailure> {
        if self.fault == ScopedFault::Combined
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
        self.observed = true;
        if self.fault == ScopedFault::RoleMutation {
            self.extraction.schema = self.alternate_schema.clone();
        }
        assert!(
            !observation
                .frames()
                .iter()
                .flat_map(|frame| frame.nodes())
                .any(|node| {
                    node.text()
                        .is_some_and(|text| text.as_str() == "Expanded fixture result")
                }),
            "fresh subtree content must be absent from every initial capture"
        );
        let prepared = observation.frames().iter().flat_map(|frame| frame.nodes()).any(|node| {
            matches!(node.value(), Some(SemanticValueSummary::Text(value)) if value.preview().text() == "fixture value")
        });
        self.ready = Some(observation.request().id());
        Ok(if self.fault == ScopedFault::Combined && prepared {
            AgentWorkTaskProgress::ReadyForExtraction
        } else {
            AgentWorkTaskProgress::Continue
        })
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        assert_eq!(self.fault, ScopedFault::Combined);
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
        self.account_samples.set(self.account_samples.get() + 1);
        Task.attest_account(context, now)
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        assert!(self.ready.is_some());
        assert_ne!(Some(result.observation()), self.ready);
        if self.fault == ScopedFault::ResultRoleMutation {
            self.extraction.schema = self.alternate_schema.clone();
        }
        self.extraction.accept_extraction(result)
    }
}

pub(super) fn capture(
    port: &Port,
    invocation: SemanticRuntimeInvocation,
    fault: ScopedFault,
) -> ContextDispatch {
    let scoped = invocation.scope() == SemanticRuntimeScopeClass::Subtree;
    let correlation = invocation.correlation();
    if scoped {
        lock(&port.calls).push(8);
        if fault == ScopedFault::RefusedCapture {
            return ContextDispatch::Unsupported;
        }
        if matches!(
            fault,
            ScopedFault::LostCapture | ScopedFault::TakeoverCapture
        ) {
            lock(&port.control)
                .as_ref()
                .unwrap()
                .stop_and_seal(AgentRuntimeStopReason::HumanTakeover);
            if fault == ScopedFault::LostCapture {
                return ContextDispatch::Scheduled;
            }
        }
        if fault == ScopedFault::RendererCapture {
            port.send(ContextNativeEvent::RendererLost(ContextRendererLoss::new(
                correlation.frame().context(),
            )));
        }
    }
    let nodes = if scoped && fault != ScopedFault::WrongRoot {
        r#"{"k":3,"r":"group","n":"Details"},{"k":4,"p":0,"r":"paragraph","t":"Expanded fixture result"}"#
    } else {
        r#"{"k":1,"r":"document","o":16},{"k":2,"p":0,"r":"textbox","n":"Field","s":64,"o":2,"v":{"k":"text","value":""},"b":{"x":10,"y":20,"w":120,"h":30}},{"k":3,"p":0,"r":"group","n":"Details"}"#
    };
    let nodes = if scoped && fault == ScopedFault::EmbeddedFrame {
        format!("{nodes},{{\"k\":5,\"p\":0,\"r\":\"frame_boundary\"}}")
    } else {
        nodes.to_owned()
    };
    let wire = format!(
        "{{\"v\":1,\"i\":{},\"g\":{},\"c\":\"complete\",\"n\":[{nodes}]}}",
        correlation.invocation().get(),
        correlation.snapshot_generation().get()
    );
    let wire = if lock(&port.calls).contains(&7) {
        wire.replace("\"value\":\"\"", "\"value\":\"fixture value\"")
    } else {
        wire
    };
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            correlation.invocation(),
            correlation.frame().clone(),
            correlation.snapshot_generation(),
        ),
        wire.as_bytes(),
    )
    .unwrap();
    port.send(ContextNativeEvent::SemanticRuntimeSettled(Box::new(
        SemanticRuntimeSettlement::try_new(
            correlation,
            if scoped && fault == ScopedFault::StaleCapture {
                Err(SemanticRuntimePortFailure::Stale)
            } else {
                Ok(snapshot)
            },
        )
        .unwrap(),
    )));
    ContextDispatch::Scheduled
}

#[test]
fn scoped_mapping_retains_exact_source_capture_and_all_original_failure_owners() {
    let _serial = lock(&SERIAL);
    for fault in [
        ScopedFault::Ceiling,
        ScopedFault::UnexpectedAction,
        ScopedFault::None,
        ScopedFault::ParagraphSelection,
        ScopedFault::RoleMutation,
        ScopedFault::AccountRoleMutation,
        ScopedFault::ResultRoleMutation,
        ScopedFault::EmbeddedFrame,
        ScopedFault::Combined,
        ScopedFault::UnknownRef,
        ScopedFault::WrongSchema,
        ScopedFault::NoGrant,
        ScopedFault::RefusedCapture,
        ScopedFault::StaleCapture,
        ScopedFault::WrongRoot,
        ScopedFault::LostCapture,
        ScopedFault::TakeoverCapture,
        ScopedFault::RendererCapture,
        ScopedFault::CancelMapCount,
        ScopedFault::CancelMapStream,
        ScopedFault::ForeignSource,
        ScopedFault::AuditLost,
        ScopedFault::ModeMutation,
    ] {
        provider_fixture(ProviderFault::Scoped(fault));
    }
}

pub(super) fn assert_outcome(
    fault: ScopedFault,
    outcome: AgentWorkOutcome,
    shutdown: AgentBrowserShutdownOutcome,
    calls: &[u8],
    events: &[AgentWorkEvent],
) {
    let captures = usize::from(!matches!(
        fault,
        ScopedFault::ModeMutation
            | ScopedFault::RoleMutation
            | ScopedFault::UnexpectedAction
            | ScopedFault::NoGrant
            | ScopedFault::UnknownRef
            | ScopedFault::WrongSchema
    ));
    assert_eq!(
        calls.iter().filter(|call| **call == 8).count(),
        captures,
        "{fault:?}"
    );
    assert_eq!(
        calls.iter().filter(|call| **call == 7).count(),
        usize::from(fault == ScopedFault::Combined)
    );
    match outcome {
        AgentWorkOutcome::Succeeded(mut success) => {
            assert!(matches!(
                fault,
                ScopedFault::None
                    | ScopedFault::ParagraphSelection
                    | ScopedFault::Combined
                    | ScopedFault::EmbeddedFrame
                    | ScopedFault::Ceiling
            ));
            assert_eq!(
                success.closure().model_calls(),
                if fault == ScopedFault::Ceiling {
                    8
                } else if fault == ScopedFault::Combined {
                    3
                } else {
                    2
                }
            );
            assert_eq!(
                success.closure().effects(),
                u32::from(fault == ScopedFault::Combined)
            );
            let result = success.take_extraction().unwrap();
            assert!(success.take_extraction().is_none());
            let SemanticExtractedValue::Text(value) = result.fields()[0].value() else {
                panic!()
            };
            assert_eq!(value.as_str(), "Expanded fixture result");
            assert!(
                matches!(&result.sources(value.source_span()).unwrap().next().unwrap().content, SemanticOwnedReadContent::Text(text) if text == "Expanded fixture result")
            );
            assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        }
        AgentWorkOutcome::Recovery(recovery) => {
            assert!(
                matches!(
                    fault,
                    ScopedFault::LostCapture
                        | ScopedFault::AuditLost
                        | ScopedFault::RendererCapture
                        | ScopedFault::Ceiling
                ),
                "{fault:?}: {:?}",
                recovery.failure()
            );
            assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Unclean));
            assert!(!events
                .iter()
                .any(|event| event.kind() == AgentWorkEventKind::Terminal));
            if let Some(session) = recovery.state.session.as_ref() {
                assert!(session.credential.is_none());
                assert_eq!(session.policy.pending_model_calls(), 0);
                assert_eq!(session.policy.pending_effects(), 0);
                assert!(session.transport.snapshot().unwrap().is_idle());
            }
            if fault == ScopedFault::LostCapture {
                assert!(recovery.state.native.observation.is_some());
                assert!(recovery.state.extraction.is_none());
            }
            if fault == ScopedFault::RendererCapture {
                assert_eq!(recovery.failure(), AgentWorkFailure::ContextLost);
                assert!(recovery.state.extraction.is_none());
            }
        }
        AgentWorkOutcome::ClosedUnsuccessfully(closed) => {
            assert!(
                !matches!(
                    fault,
                    ScopedFault::None
                        | ScopedFault::ParagraphSelection
                        | ScopedFault::EmbeddedFrame
                        | ScopedFault::Combined
                        | ScopedFault::LostCapture
                        | ScopedFault::AuditLost
                        | ScopedFault::RendererCapture
                ),
                "{fault:?}: {:?}",
                closed.failure()
            );
            assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
            assert_eq!(closed.policy_settlement().closure().effects(), 0);
            if matches!(
                fault,
                ScopedFault::CancelMapCount
                    | ScopedFault::CancelMapStream
                    | ScopedFault::TakeoverCapture
            ) {
                assert_eq!(closed.failure(), AgentWorkFailure::HumanTakeover);
            }
            match fault {
                ScopedFault::WrongRoot => assert_eq!(
                    closed.failure(),
                    AgentWorkFailure::Browser(AgentBrowserProviderError::Read(
                        SemanticReadError::ExpansionMismatch
                    ))
                ),
                ScopedFault::UnexpectedAction => {
                    assert_eq!(closed.failure(), AgentWorkFailure::Contract)
                }
                ScopedFault::UnknownRef | ScopedFault::WrongSchema => assert_eq!(
                    closed.failure(),
                    AgentWorkFailure::Browser(AgentBrowserProviderError::Continuation)
                ),
                ScopedFault::NoGrant => assert_eq!(
                    closed.failure(),
                    AgentWorkFailure::Browser(AgentBrowserProviderError::UnsupportedTool(
                        AgentBrowserToolKind::Extract
                    ))
                ),
                ScopedFault::RefusedCapture => {
                    assert_eq!(closed.failure(), AgentWorkFailure::Context)
                }
                ScopedFault::StaleCapture => assert_eq!(
                    closed.failure(),
                    AgentWorkFailure::Observation(SemanticRuntimePortFailure::Stale)
                ),
                ScopedFault::ModeMutation
                | ScopedFault::RoleMutation
                | ScopedFault::AccountRoleMutation
                | ScopedFault::ResultRoleMutation => {
                    assert_eq!(closed.failure(), AgentWorkFailure::Contract)
                }
                ScopedFault::ForeignSource => assert!(matches!(
                    closed.failure(),
                    AgentWorkFailure::Browser(AgentBrowserProviderError::Extraction(_))
                )),
                _ => {}
            }
        }
    }
}
