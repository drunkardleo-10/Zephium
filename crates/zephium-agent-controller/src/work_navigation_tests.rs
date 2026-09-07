//! Two documents through the shipping actor, provider transport and mailbox.
use super::*;
use std::cell::Cell;
use std::sync::atomic::AtomicBool;

#[path = "work_route_tests.rs"]
pub(super) mod route_tests;

pub(super) fn discovery_scope() -> AgentNavigationDiscovery {
    AgentNavigationDiscovery::try_new(
        ContextNavigationTarget::parse("https://work-fixture.invalid/").unwrap(),
        "/".into(),
        2,
    )
    .unwrap()
}

#[test]
fn discovered_link_workflow_uses_original_controller_and_refuses_unobserved_urls() {
    let _serial = lock(&SERIAL);
    for fault in [
        NavigationFault::Discovery,
        NavigationFault::DiscoveryMissingLink,
    ] {
        provider_fixture(ProviderFault::Navigation(fault));
    }
}

pub(super) struct NavigationSchedule {
    pub(super) events: Mutex<Option<Arc<Mutex<WorkEvents>>>>,
    native_started: AtomicBool,
    native_clock_calls: AtomicU64,
    audit_faulted: AtomicBool,
    fault: NavigationFault,
}
impl NavigationSchedule {
    pub(super) fn new(fault: NavigationFault) -> Self {
        Self {
            events: Mutex::new(None),
            native_started: AtomicBool::new(false),
            native_clock_calls: AtomicU64::new(0),
            audit_faulted: AtomicBool::new(false),
            fault,
        }
    }
}
pub(super) struct NavigationClock {
    ticks: Clock,
    schedule: Arc<NavigationSchedule>,
}
impl NavigationClock {
    pub(super) fn new(schedule: Arc<NavigationSchedule>) -> Self {
        Self {
            ticks: Clock(AtomicU64::new(FIXTURE_POLICY_NOW_MILLIS)),
            schedule,
        }
    }
}
impl TerraControllerClock for NavigationClock {
    fn now(&self) -> Result<AgentPolicyInstant, super::super::super::TerraControllerClockError> {
        if let NavigationFault::Route(fault) = self.schedule.fault {
            return route_tests::clock(self, fault);
        }
        if self.schedule.native_started.load(Ordering::Relaxed) {
            let call = self
                .schedule
                .native_clock_calls
                .fetch_add(1, Ordering::Relaxed)
                + 1;
            match self.schedule.fault {
                NavigationFault::RefusalClock if call == 2 => {
                    return Err(super::super::super::TerraControllerClockError::Invalid);
                }
                NavigationFault::RefusalClockStuck if call >= 2 => {
                    return Err(super::super::super::TerraControllerClockError::Invalid);
                }
                NavigationFault::AuditRefusedClock if call <= 2 => {
                    return Err(super::super::super::TerraControllerClockError::Invalid);
                }
                NavigationFault::RefusalClockRegressed if call >= 2 => {
                    return Ok(AgentPolicyInstant::from_millis(
                        FIXTURE_POLICY_NOW_MILLIS - 1,
                    ));
                }
                _ => {}
            }
        }
        if matches!(
            self.schedule.fault,
            NavigationFault::AuditActive | NavigationFault::AuditRefused
        ) && self.schedule.native_started.load(Ordering::Relaxed)
            && !self.schedule.audit_faulted.swap(true, Ordering::Relaxed)
        {
            return Err(super::super::super::TerraControllerClockError::Invalid);
        }
        self.ticks.now()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NavigationFault {
    Discovery,
    DiscoveryMissingLink,
    Route(route_tests::RouteFault),
    None,
    PriorLocate,
    Ceiling,
    WrongTarget,
    CrossOrigin,
    Premature,
    Repeat,
    DepartureMissing,
    ArrivalMissing,
    TargetMutation,
    AccountMutation,
    ChangedAccount,
    StaleAccount,
    CachedAccount,
    Dispatch,
    NativeFailure,
    Redirect,
    Lost,
    Takeover,
    Renderer,
    StaleCapture,
    LostCapture,
    CancelCapture,
    CancelMapCount,
    ForeignSource,
    OldCitation,
    ResultMutation,
    AuditLost,
    Backpressure,
    AuditActive,
    AuditRefused,
    RefusalClock,
    RefusalClockStuck,
    RefusalClockRegressed,
    AuditRefusedClock,
}

impl NavigationFault {
    pub(super) fn requests(self) -> u8 {
        match self {
            Self::Route(fault) => fault.requests(),
            Self::PriorLocate => 8,
            Self::Ceiling => 14,
            Self::DepartureMissing | Self::TargetMutation | Self::AccountMutation => 0,
            Self::None
            | Self::Discovery
            | Self::ForeignSource
            | Self::OldCitation
            | Self::ResultMutation
            | Self::AuditLost => 6,
            Self::Repeat => 4,
            Self::CancelMapCount => 5,
            _ => 2,
        }
    }
    pub(super) fn cancelled(self, turns: u8, count: bool) -> bool {
        if let Self::Route(fault) = self {
            return fault.cancelled(turns, count);
        }
        self == Self::CancelMapCount && turns == 2 && count
    }
    pub(super) fn stream(self, turn: u8) -> String {
        if let Self::Route(fault) = self {
            return fault.stream(turn);
        }
        if (self == Self::PriorLocate && turn == 1) || (self == Self::Ceiling && turn < 7) {
            return tool_stream(turn, false);
        }
        if self == Self::PriorLocate {
            return Self::None
                .stream(turn - 1)
                .replace(&format!("resp_{}", turn - 1), &format!("resp_{turn}"))
                .replace(&format!("msg_{}", turn - 1), &format!("msg_{turn}"))
                .replace(&format!("fc_{}", turn - 1), &format!("fc_{turn}"))
                .replace(&format!("call_{}", turn - 1), &format!("call_{turn}"));
        }
        if self == Self::Ceiling {
            return named_tool_stream(
                turn,
                "navigate",
                r#"{\"url\":\"https://work-fixture.invalid/arrival\"}"#,
            );
        }
        if turn == 1 && self == Self::Premature || turn == 2 && self != Self::Repeat {
            named_tool_stream(
                turn,
                "extract",
                r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
            )
        } else if turn < 3 {
            let target = match self {
                Self::WrongTarget => "https://work-fixture.invalid/substitution",
                Self::CrossOrigin => "https://other.invalid/arrival",
                _ => "https://work-fixture.invalid/arrival",
            };
            named_tool_stream(turn, "navigate", &format!(r#"{{\"url\":\"{target}\"}}"#))
        } else {
            extraction_stream(if self == Self::ForeignSource {
                ExtractionFault::ForeignSource
            } else {
                ExtractionFault::None
            })
            .replace(
                "Field",
                if self == Self::OldCitation {
                    "Departure certificate"
                } else {
                    "Arrival certificate"
                },
            )
            .replace("resp_2", "resp_3")
            .replace("msg_2", "msg_3")
        }
    }
    pub(super) fn check_request(self, bytes: &[u8], turns: u8) {
        if let Self::Route(fault) = self {
            return fault.check_request(bytes, turns);
        }
        let text = std::str::from_utf8(bytes).unwrap();
        if matches!(self, Self::Discovery | Self::DiscoveryMissingLink) {
            assert!(text.contains("ZEPHIUM_HOST_LINK_DISCOVERY_V1"));
            assert!(!text.contains("ZEPHIUM_HOST_NAVIGATION_CHECKPOINT_V1"));
            assert!(text.contains(r#"\"next_navigation_target\":null"#));
        }
        assert!(
            text.contains("Verify a deterministic fixture."),
            "admitted objective survives"
        );
        if turns == 0 || self == Self::Ceiling || (self == Self::PriorLocate && turns == 1) {
            assert!(text.contains("Departure certificate"));
            assert!(!text.contains("Arrival certificate"));
        } else {
            assert!(
                !text.contains("Departure certificate"),
                "old page transcript is retired"
            );
            assert!(!text.contains("call_1"), "old tool correlation is retired");
            assert!(!text.contains("resp_1"));
            assert!(text.contains("Arrival certificate"));
        }
    }
}

pub(super) struct NavigationTask {
    extraction: AgentWorkExtractionTask,
    target: ContextNavigationTarget,
    alternate: ContextNavigationTarget,
    fault: NavigationFault,
    departed: Option<ContextJoin>,
    arrival: Option<SemanticObservationId>,
    mutate: Cell<bool>,
    prior_account: Cell<Option<AgentContextAccountBinding>>,
    account_samples: Cell<u8>,
    schedule: Arc<NavigationSchedule>,
}
impl NavigationTask {
    pub(super) fn new(fault: NavigationFault, schedule: Arc<NavigationSchedule>) -> Self {
        Self {
            extraction: AgentWorkExtractionTask::try_new(
                vec![SemanticExtractionFieldSchema::try_text("label".into(), true, 64).unwrap()],
                AgentAccountScope::Anonymous,
            )
            .unwrap()
            .with_source_roles(
                SemanticReadRoleSelection::try_new(&[SemanticRole::Heading]).unwrap(),
            ),
            target: ContextNavigationTarget::parse("https://work-fixture.invalid/arrival").unwrap(),
            alternate: ContextNavigationTarget::parse("https://work-fixture.invalid/substitution")
                .unwrap(),
            fault,
            departed: None,
            arrival: None,
            mutate: Cell::new(false),
            prior_account: Cell::new(None),
            account_samples: Cell::new(0),
            schedule,
        }
    }
}
impl AgentWorkTask for NavigationTask {
    fn navigation_target(&self) -> Option<&ContextNavigationTarget> {
        Some(if self.mutate.get() {
            &self.alternate
        } else {
            &self.target
        })
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.extraction.extraction_schema()
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let has = |text: &str| {
            observation
                .frames()
                .iter()
                .flat_map(|frame| frame.nodes())
                .any(|node| node.name().is_some_and(|name| name.as_str() == text))
        };
        if let Some(prior) = self.departed {
            if prior == observation.request().context()
                || !has("Arrival certificate")
                || self.fault == NavigationFault::ArrivalMissing
            {
                return Err(AgentWorkFailure::Contract);
            }
            self.arrival = Some(observation.request().id());
            Ok(AgentWorkTaskProgress::ReadyForExtraction)
        } else {
            if !has("Departure certificate") || self.fault == NavigationFault::DepartureMissing {
                return Err(AgentWorkFailure::Contract);
            }
            self.departed = Some(observation.request().context());
            if self.fault == NavigationFault::TargetMutation {
                self.mutate.set(true);
            }
            Ok(AgentWorkTaskProgress::ReadyForNavigation)
        }
    }
    fn assess(
        &self,
        _: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        panic!("navigation cannot become an action");
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        let current = Task.attest_account(context, now)?;
        self.account_samples.set(self.account_samples.get() + 1);
        if self.fault == NavigationFault::Backpressure && self.account_samples.get() == 3 {
            let events = lock(&self.schedule.events).as_ref().unwrap().clone();
            let mut events = lock(&events);
            while events.queue.len() < MAX_AGENT_WORK_EVENTS {
                events.publish(AgentWorkEventKind::Observing).unwrap();
            }
        }
        if let Some(prior) = self.prior_account.get() {
            if self.fault == NavigationFault::AccountMutation && self.departed.is_some() {
                self.mutate.set(true);
            }
            if context != prior.context() {
                return Ok(match self.fault {
                    NavigationFault::CachedAccount => prior,
                    NavigationFault::StaleAccount => AgentContextAccountBinding::new(
                        current.attestation(),
                        context,
                        current.account(),
                        prior.observed_at(),
                    ),
                    NavigationFault::ChangedAccount => AgentContextAccountBinding::new(
                        current.attestation(),
                        context,
                        AgentAccountScope::Authenticated(AgentAccountId::generate()),
                        now,
                    ),
                    _ => current,
                });
            }
        }
        self.prior_account.set(Some(current));
        Ok(current)
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        assert_eq!(Some(result.observation()), self.arrival);
        if self.fault == NavigationFault::ResultMutation {
            self.mutate.set(true);
        }
        let SemanticExtractedValue::Text(value) = result.fields()[0].value() else {
            return Err(AgentWorkFailure::Contract);
        };
        if value.as_str() != "Arrival certificate" {
            return Err(AgentWorkFailure::Contract);
        }
        self.extraction.accept_extraction(result)
    }
}

pub(super) fn navigate(
    port: &Port,
    request: ContextNavigationRequest,
    fault: NavigationFault,
) -> ContextDispatch {
    if let NavigationFault::Route(fault) = fault {
        return route_tests::navigate(port, request, fault);
    }
    lock(&port.calls).push(9);
    port.navigation_schedule
        .as_ref()
        .unwrap()
        .native_started
        .store(true, Ordering::Relaxed);
    assert_eq!(
        request.target().as_url().as_str(),
        "https://work-fixture.invalid/arrival"
    );
    assert!(request.redirect_policy().is_none());
    if matches!(
        fault,
        NavigationFault::Dispatch
            | NavigationFault::AuditRefused
            | NavigationFault::RefusalClock
            | NavigationFault::RefusalClockStuck
            | NavigationFault::RefusalClockRegressed
            | NavigationFault::AuditRefusedClock
    ) {
        return ContextDispatch::Rejected(ContextPortFailure::NativeRefused);
    }
    if matches!(fault, NavigationFault::Lost | NavigationFault::Takeover) {
        lock(&port.control)
            .as_ref()
            .unwrap()
            .stop_and_seal(AgentRuntimeStopReason::HumanTakeover);
        if fault == NavigationFault::Lost {
            return ContextDispatch::Scheduled;
        }
    }
    if fault == NavigationFault::Renderer {
        port.send(ContextNativeEvent::RendererLost(ContextRendererLoss::new(
            request.operation().context(),
        )));
    }
    let outcome = if fault == NavigationFault::Redirect {
        Ok(ContextNavigationTarget::parse("https://work-fixture.invalid/redirect").unwrap())
    } else if fault == NavigationFault::NativeFailure {
        Err(ContextPortFailure::NativeRefused)
    } else {
        Ok(request.target().clone())
    };
    port.send(ContextNativeEvent::NavigationSettled(
        ContextNavigationSettlement::try_new(request.operation(), outcome).unwrap(),
    ));
    ContextDispatch::Scheduled
}

pub(super) fn capture(
    port: &Port,
    invocation: SemanticRuntimeInvocation,
    fault: NavigationFault,
) -> ContextDispatch {
    if let NavigationFault::Route(fault) = fault {
        return route_tests::capture(port, invocation, fault);
    }
    let arrived = lock(&port.calls).contains(&9);
    let correlation = invocation.correlation();
    assert_eq!(
        correlation.snapshot_generation().get(),
        1,
        "new document rotates the semantic world"
    );
    if arrived
        && matches!(
            fault,
            NavigationFault::LostCapture | NavigationFault::CancelCapture
        )
    {
        lock(&port.control)
            .as_ref()
            .unwrap()
            .stop_and_seal(AgentRuntimeStopReason::HumanTakeover);
        if fault == NavigationFault::LostCapture {
            return ContextDispatch::Scheduled;
        }
    }
    let heading = if arrived {
        "Arrival certificate"
    } else {
        "Departure certificate"
    };
    let wire = format!(
        r#"{{"v":1,"i":{},"g":{},"c":"complete","n":[{{"k":1,"r":"document","o":16}},{{"k":2,"p":0,"r":"heading","l":1,"n":"{heading}"}}]}}"#,
        correlation.invocation().get(),
        correlation.snapshot_generation().get()
    );
    let wire = if !arrived && fault == NavigationFault::Discovery {
        wire.replace("]}", r#",{"k":3,"p":0,"r":"link","n":"A relevant source","u":"https://work-fixture.invalid/arrival"}]}"#)
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
            if arrived && fault == NavigationFault::StaleCapture {
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
fn two_document_workflow_retires_provider_replay_and_preserves_original_run_owners() {
    let _serial = lock(&SERIAL);
    for fault in [
        NavigationFault::None,
        NavigationFault::PriorLocate,
        NavigationFault::Ceiling,
        NavigationFault::WrongTarget,
        NavigationFault::CrossOrigin,
        NavigationFault::Premature,
        NavigationFault::Repeat,
        NavigationFault::DepartureMissing,
        NavigationFault::ArrivalMissing,
        NavigationFault::TargetMutation,
        NavigationFault::AccountMutation,
        NavigationFault::ChangedAccount,
        NavigationFault::StaleAccount,
        NavigationFault::CachedAccount,
        NavigationFault::Dispatch,
        NavigationFault::NativeFailure,
        NavigationFault::Redirect,
        NavigationFault::Lost,
        NavigationFault::Takeover,
        NavigationFault::Renderer,
        NavigationFault::StaleCapture,
        NavigationFault::LostCapture,
        NavigationFault::CancelCapture,
        NavigationFault::CancelMapCount,
        NavigationFault::ForeignSource,
        NavigationFault::OldCitation,
        NavigationFault::ResultMutation,
        NavigationFault::AuditLost,
        NavigationFault::Backpressure,
        NavigationFault::AuditActive,
        NavigationFault::AuditRefused,
        NavigationFault::RefusalClock,
        NavigationFault::RefusalClockStuck,
        NavigationFault::RefusalClockRegressed,
        NavigationFault::AuditRefusedClock,
    ] {
        provider_fixture(ProviderFault::Navigation(fault));
    }
}

pub(super) fn assert_outcome(
    fault: NavigationFault,
    outcome: AgentWorkOutcome,
    shutdown: AgentBrowserShutdownOutcome,
    calls: &[u8],
    events: &[AgentWorkEvent],
) {
    if let NavigationFault::Route(fault) = fault {
        return route_tests::assert_outcome(fault, outcome, shutdown, calls, events);
    }
    assert!(!calls.contains(&7), "navigation never dispatches an action");
    assert!(!events
        .iter()
        .any(|event| event.kind() == AgentWorkEventKind::Verified));
    if matches!(
        fault,
        NavigationFault::None | NavigationFault::PriorLocate | NavigationFault::Discovery
    ) {
        let AgentWorkOutcome::Succeeded(mut success) = outcome else {
            panic!("navigation success: {outcome:?}");
        };
        assert_eq!(
            success.closure().model_calls(),
            if fault == NavigationFault::PriorLocate {
                4
            } else {
                3
            }
        );
        assert_eq!(success.closure().navigations(), 1);
        assert_eq!(success.closure().effects(), 0);
        assert_eq!(
            success.closure().operations(),
            success.closure().model_calls() + 1
        );
        let result = success.take_extraction().unwrap();
        assert!(success.take_extraction().is_none());
        let SemanticExtractedValue::Text(value) = result.fields()[0].value() else {
            panic!();
        };
        assert_eq!(value.as_str(), "Arrival certificate");
        assert!(
            matches!(&result.sources(value.source_span()).unwrap().next().unwrap().content, SemanticOwnedReadContent::Text(text) if text == "Arrival certificate")
        );
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        assert_eq!(calls, [1, 2, 3, 2, 9, 3, 4, 5, 6]);
    } else if matches!(
        fault,
        NavigationFault::Redirect
            | NavigationFault::Lost
            | NavigationFault::Renderer
            | NavigationFault::LostCapture
            | NavigationFault::AuditLost
            | NavigationFault::AuditActive
            | NavigationFault::AuditRefused
            | NavigationFault::RefusalClockStuck
            | NavigationFault::RefusalClockRegressed
            | NavigationFault::AuditRefusedClock
    ) {
        let AgentWorkOutcome::Recovery(recovery) = outcome else {
            panic!("{fault:?} must retain debt: {outcome:?}");
        };
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Unclean));
        assert!(!events
            .iter()
            .any(|event| event.kind() == AgentWorkEventKind::Terminal));
        if let Some(session) = &recovery.state.session {
            assert!(session.credential.is_none());
            assert_eq!(session.policy.pending_effects(), 0);
            assert_eq!(session.policy.pending_model_calls(), 0);
            if matches!(
                fault,
                NavigationFault::RefusalClockStuck | NavigationFault::RefusalClockRegressed
            ) {
                assert_eq!(calls.iter().filter(|call| **call == 9).count(), 1);
                assert_eq!(session.policy.pending_navigations(), 1);
                assert_eq!(session.policy.accounting().reserved_operations(), 1);
                let active = session.navigation.as_ref().expect("original policy owner");
                let refusal = session
                    .navigation_refusal
                    .expect("exact native refusal survives");
                assert_eq!(refusal.operation, active.operation());
                assert_eq!(refusal.failure, ContextPortFailure::NativeRefused);
                assert!(session.navigation_receipt.is_none());
                assert!(recovery.state.native.operation.is_none());
            }
            if matches!(fault, NavigationFault::Redirect | NavigationFault::Lost) {
                assert_eq!(session.policy.pending_navigations(), 1);
                assert!(session.navigation.is_some());
            }
            if matches!(
                fault,
                NavigationFault::AuditActive
                    | NavigationFault::AuditRefused
                    | NavigationFault::AuditRefusedClock
            ) {
                assert!(calls.contains(&9));
                assert_eq!(session.policy.pending_navigations(), 0);
                assert!(session.navigation.is_none());
                assert!(session.navigation_refusal.is_none());
                let receipt = session
                    .navigation_receipt
                    .expect("accounted terminal retains audit debt, not a native reservation");
                assert_eq!(
                    receipt.settlement(),
                    if fault == NavigationFault::AuditActive {
                        AgentNavigationSettlement::Committed
                    } else {
                        AgentNavigationSettlement::Failed(ContextPortFailure::NativeRefused)
                    }
                );
                assert!(recovery.state.native.operation.is_none());
                if fault == NavigationFault::AuditRefusedClock {
                    assert_eq!(recovery.failure, AgentWorkFailure::Contract);
                }
            }
        }
    } else {
        let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
            panic!("{fault:?} must close without result: {outcome:?}");
        };
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        let closure = closed.policy_settlement().closure();
        if fault == NavigationFault::RefusalClock {
            assert_eq!(
                closed.failure(),
                AgentWorkFailure::Browser(AgentBrowserProviderError::Clock)
            );
            assert_eq!(calls.iter().filter(|call| **call == 9).count(), 1);
            assert_eq!(
                closed
                    .policy_settlement()
                    .accounting()
                    .reserved_operations(),
                0
            );
            assert_eq!(closure.model_calls(), 1);
            assert_eq!(closure.navigations(), 1);
        }
        if fault == NavigationFault::Backpressure {
            assert_eq!(closed.failure(), AgentWorkFailure::Backpressure);
            assert!(!calls.contains(&9));
            assert_eq!(calls.iter().filter(|call| **call == 2).count(), 1);
            assert_eq!(
                closed
                    .policy_settlement()
                    .accounting()
                    .reserved_operations(),
                0
            );
            assert_eq!(closure.navigations(), 0);
            assert_eq!(closure.model_calls(), 1);
        }
        assert_eq!(closure.effects(), 0);
        assert_eq!(closure.navigations(), u32::from(calls.contains(&9)));
        assert_eq!(
            closure.operations(),
            closure.model_calls() + closure.navigations()
        );
    }
}
