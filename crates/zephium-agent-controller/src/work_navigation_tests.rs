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

#[test]
fn two_discovered_hops_settle_original_progress_accounting_and_extract() {
    let _serial = lock(&SERIAL);
    provider_fixture(ProviderFault::Navigation(NavigationFault::DiscoveryTwoHops));
}

#[test]
fn authenticated_discovery_uses_fresh_host_account_samples_across_two_documents() {
    let _serial = lock(&SERIAL);
    provider_fixture_with_discovery_account(
        ProviderFault::Navigation(NavigationFault::DiscoveryTwoHops),
        None,
        None,
        Some(AgentAccountScope::Authenticated(AgentAccountId::generate())),
    );
}

#[test]
fn discovery_budget_reserves_mapping_and_rejects_last_decision_inspection() {
    let _serial = lock(&SERIAL);
    for (limit, refusal) in [(8, false), (4, false), (8, true)] {
        provider_fixture(ProviderFault::Navigation(NavigationFault::DiscoveryBudget(
            limit, refusal, 24,
        )));
    }
}

#[test]
fn extended_run_budget_crosses_eight_turns_and_preserves_terminal_reservation() {
    let _serial = lock(&SERIAL);
    for refusal in [false, true] {
        provider_fixture(ProviderFault::Navigation(NavigationFault::DiscoveryBudget(
            12, refusal, 24,
        )));
    }
    // This exceeds the original 64-record pending audit capacity and must
    // stream committed batches without dropping events or reusing delivery IDs.
    provider_fixture(ProviderFault::Navigation(NavigationFault::DiscoveryBudget(
        40, false, 64,
    )));
}

#[test]
fn discovery_operation_budget_preserves_the_mapping_call_after_navigation() {
    let _serial = lock(&SERIAL);
    for refusal in [false, true] {
        provider_fixture(ProviderFault::Navigation(NavigationFault::DiscoveryBudget(
            8, refusal, 8,
        )));
    }
}

#[test]
fn discovery_merges_accounted_documents_after_empty_inspection_and_rejects_unknown_sources() {
    let _serial = lock(&SERIAL);
    for foreign in [false, true] {
        provider_fixture(ProviderFault::Navigation(
            NavigationFault::DiscoveryEvidence(foreign),
        ));
    }
}

#[test]
fn discovery_returns_invalid_search_scope_without_capture_and_preserves_budgets() {
    let _serial = lock(&SERIAL);
    for case in 0..4 {
        provider_fixture(ProviderFault::Navigation(
            NavigationFault::DiscoveryScopeRefusal(case),
        ));
    }
}

#[test]
fn per_run_model_call_allowance_cannot_widen_product_limits() {
    for limit in [0, 1, 65, u8::MAX] {
        assert!(input().settings.with_max_model_calls(limit).is_err());
    }
    for limit in [2, 4, 8, 24, 64] {
        assert_eq!(
            input()
                .settings
                .with_max_model_calls(limit)
                .unwrap()
                .max_model_calls,
            limit
        );
    }
    for limit in [65, u64::MAX] {
        assert!(input().settings.with_max_actions(limit).is_err());
    }
    for limit in [0, 1, 8, 24, 64] {
        assert_eq!(
            input()
                .settings
                .with_max_actions(limit)
                .unwrap()
                .max_actions,
            limit
        );
    }
}

#[test]
fn two_discovered_hops_preserve_policy_blocked_final_page_frames() {
    let _serial = lock(&SERIAL);
    provider_fixture(ProviderFault::Navigation(
        NavigationFault::DiscoveryTwoHopsBlockedFrame,
    ));
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
    DiscoveryBudget(u8, bool, u32),
    DiscoveryEvidence(bool),
    DiscoveryScopeRefusal(u8),
    DiscoveryTwoHops,
    DiscoveryTwoHopsBlockedFrame,
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
    pub(super) fn two_discovery_hops(self) -> bool {
        matches!(
            self,
            Self::DiscoveryTwoHops | Self::DiscoveryTwoHopsBlockedFrame
        )
    }
    pub(super) fn requests(self) -> u8 {
        match self {
            Self::DiscoveryScopeRefusal(_) => 10,
            Self::DiscoveryEvidence(_) => 8,
            Self::DiscoveryBudget(limit, refusal, operations) => {
                2 * (limit.min((operations - 1) as u8) - u8::from(refusal))
            }
            Self::DiscoveryTwoHops | Self::DiscoveryTwoHopsBlockedFrame => {
                route_tests::RouteFault::None.requests()
            }
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
        if let Self::DiscoveryScopeRefusal(case) = self {
            return match turn {
                1 => Self::Discovery.stream(1),
                2 => named_tool_stream(
                    turn,
                    "snapshot",
                    r#"{\"scope\":{\"kind\":\"surrounding_text\",\"target\":\"@a2\",\"before_bytes\":0,\"after_bytes\":1024}}"#,
                ),
                3 => named_tool_stream(
                    turn,
                    "snapshot",
                    &format!(
                        r#"{{\"scope\":{{\"kind\":\"text_search\",\"target\":\"{}\",\"query\":\"$\"}}}}"#,
                        if case == 1 { "@a99" } else { "@a1" }
                    ),
                ),
                4 | 5 if case == 2 => named_tool_stream(
                    turn,
                    "snapshot",
                    r#"{\"scope\":{\"kind\":\"text_search\",\"target\":\"@a1\",\"query\":\"$\"}}"#,
                ),
                4 => named_tool_stream(
                    turn,
                    "extract",
                    r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
                ),
                _ => Self::Discovery
                    .stream(3)
                    .replace("resp_3", "resp_5")
                    .replace("msg_3", "msg_5"),
            };
        }
        if let Self::DiscoveryEvidence(foreign) = self {
            return match turn {
                1 => Self::Discovery.stream(1),
                2 => named_tool_stream(turn, "snapshot", r#"{\"scope\":{\"kind\":\"initial\"}}"#),
                3 => named_tool_stream(
                    turn,
                    "extract",
                    r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
                ),
                _ => Self::Discovery
                    .stream(3)
                    .replace("resp_3", "resp_4")
                    .replace("msg_3", "msg_4")
                    .replace("Arrival certificate", "Arrival and departure certificates")
                    .replace("@r1", if foreign { "@r99" } else { r#"@r1\",\"@r2"# }),
            };
        }
        if let Self::DiscoveryBudget(limit, refusal, operations) = self {
            let limit = limit.min((operations - 1) as u8);
            if turn == 1 {
                return Self::Discovery.stream(turn);
            }
            if turn == 2 {
                return named_tool_stream(turn, "read", r#"{\"scope\":{\"kind\":\"initial\"}}"#);
            }
            if turn < limit - 1 {
                return named_tool_stream(
                    turn,
                    "locate",
                    r#"{\"semantic_query\":\"missing specifications\",\"scope\":{\"kind\":\"initial\"}}"#,
                );
            }
            if turn == limit - 1 {
                return if refusal {
                    if operations <= u32::from(limit) + 1 {
                        named_tool_stream(
                            turn,
                            "locate",
                            r#"{\"semantic_query\":\"missing specifications\",\"scope\":{\"kind\":\"initial\"}}"#,
                        )
                    } else {
                        named_tool_stream(turn, "snapshot", r#"{\"scope\":{\"kind\":\"initial\"}}"#)
                    }
                } else {
                    named_tool_stream(
                        turn,
                        "extract",
                        r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
                    )
                };
            }
            return Self::Discovery
                .stream(3)
                .replace("resp_3", &format!("resp_{turn}"))
                .replace("msg_3", &format!("msg_{turn}"));
        }
        if self.two_discovery_hops() {
            return route_tests::RouteFault::None.stream(turn);
        }
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
        if let Self::DiscoveryScopeRefusal(case) = self {
            if turns == 3 || (case == 2 && turns == 4) {
                let body: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                let results: Vec<_> = body["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|item| item["type"] == "function_call_output")
                    .collect();
                let result = results.last().unwrap();
                assert_eq!(result["call_id"], format!("call_{turns}"));
                let error: serde_json::Value =
                    serde_json::from_str(result["output"].as_str().unwrap()).unwrap();
                assert_eq!(error["code"], "invalid_snapshot_scope");
                assert_eq!(error["executed"], false);
                assert_eq!(error["observation_unchanged"], true);
                assert!(error.get("observation").is_none());
                let observation = body["input"][1]["content"][0]["text"].as_str().unwrap();
                assert!(observation.contains("scope=surrounding_text"));
                assert!(observation.contains("r=heading"));
                assert!(observation.contains("Arrival certificate"));
                assert_eq!(
                    std::str::from_utf8(bytes).unwrap().matches("ZSEM3").count(),
                    1,
                    "refusal replays the current observation exactly once"
                );
                assert!(std::str::from_utf8(bytes).unwrap().contains(&format!(
                    "decision_calls_remaining_including_this={}",
                    if case == 3 { 1 } else { 5 - turns }
                )));
                if case == 3 {
                    let tools = body["tools"].as_array().unwrap();
                    assert_eq!(
                        tools.len(),
                        1,
                        "operation budget still reserves terminal mapping"
                    );
                    assert_eq!(tools[0]["name"], "extract");
                }
            }
            return;
        }
        if let Self::DiscoveryEvidence(_) = self {
            let text = std::str::from_utf8(bytes).unwrap();
            if turns == 2 {
                assert!(
                    !text.contains("Arrival certificate"),
                    "fresh empty decision has no old actionable baseline"
                );
            }
            if turns == 3 {
                assert!(
                    text.contains("Arrival certificate"),
                    "terminal mapping receives retained evidence"
                );
                assert!(text.contains("historical_observation="));
                assert!(
                    text.contains("Departure certificate"),
                    "accounted navigation retains historical read evidence for extraction"
                );
            }
            return;
        }
        if let Self::DiscoveryBudget(configured_limit, _, operations) = self {
            let limit = configured_limit.min((operations - 1) as u8);
            let text = std::str::from_utf8(bytes).unwrap();
            assert!(text.contains("ZEPHIUM_HOST_LINK_DISCOVERY_V1"));
            assert_eq!(
                text.matches("ZEPHIUM_HOST_DECISION_BUDGET_V1").count(),
                usize::from(turns < limit - 1)
            );
            if turns < limit - 1 {
                assert!(text.contains(&format!(
                    "decision_calls_remaining_including_this={}",
                    u32::from(configured_limit - turns - 1)
                        .min(operations - u32::from(turns) - u32::from(turns > 0) - 1)
                )));
                assert!(text.contains("terminal_mapping_calls_reserved=1"));
                let tools = text.split("\"tools\":").nth(1).unwrap();
                if turns == limit - 2 {
                    assert_eq!(tools.matches("\"name\":").count(), 1);
                    assert!(tools.contains("\"name\":\"extract\""));
                } else {
                    assert!(tools.contains("\"name\":\"locate\""));
                }
            }
            if turns == 2 {
                assert!(text.contains("ZREAD3 content=untrusted"));
            }
            if turns > 2 && turns < limit - 1 {
                assert!(text.contains("matches=0 matched=0"));
            }
            return;
        }
        if self.two_discovery_hops() {
            let text = std::str::from_utf8(bytes).unwrap();
            assert!(
                text.contains("frames=1"),
                "only the main frame is disclosed"
            );
            if self == Self::DiscoveryTwoHopsBlockedFrame && turns >= 2 {
                assert!(text.contains("unsupported:policy_blocked"));
                assert!(text.contains("frame_boundary"));
            }
            assert!(text.contains("ZEPHIUM_HOST_LINK_DISCOVERY_V1"));
            assert!(!text.contains("ZEPHIUM_HOST_NAVIGATION_CHECKPOINT_V1"));
            assert!(text.contains(&format!(
                r#"\"completed_hops\":{},\"total_hops\":2,\"next_navigation_target\":null"#,
                turns.min(2)
            )));
            for prior in 1..=turns.min(2) {
                assert!(!text.contains(&format!("call_{prior}")));
                assert!(!text.contains(&format!("resp_{prior}")));
            }
            return;
        }
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
                (self == Self::Discovery && turns == 2) || !text.contains("Departure certificate"),
                "only terminal discovery extraction may include prior document evidence"
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
    if fault.two_discovery_hops() {
        return route_tests::navigate(port, request, route_tests::RouteFault::None);
    }
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
    if fault.two_discovery_hops() {
        return route_tests::capture(port, invocation, route_tests::RouteFault::None);
    }
    if let NavigationFault::Route(fault) = fault {
        return route_tests::capture(port, invocation, fault);
    }
    let arrived = lock(&port.calls).contains(&9);
    let correlation = invocation.correlation();
    if !matches!(
        fault,
        NavigationFault::DiscoveryEvidence(_) | NavigationFault::DiscoveryScopeRefusal(_)
    ) {
        assert_eq!(
            correlation.snapshot_generation().get(),
            1,
            "new document rotates the semantic world"
        );
    }
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
    let wire = if !arrived
        && matches!(
            fault,
            NavigationFault::Discovery
                | NavigationFault::DiscoveryBudget(..)
                | NavigationFault::DiscoveryEvidence(_)
                | NavigationFault::DiscoveryScopeRefusal(_)
        ) {
        wire.replace("]}", r#",{"k":3,"p":0,"r":"link","n":"A relevant source","u":"https://work-fixture.invalid/arrival"}]}"#)
    } else {
        wire
    };
    let wire = if matches!(fault, NavigationFault::DiscoveryEvidence(_))
        && correlation.snapshot_generation().get() > 1
    {
        format!(
            r#"{{"v":1,"i":{},"g":{},"c":"complete","n":[{{"k":1,"r":"document","o":16}}]}}"#,
            correlation.invocation().get(),
            correlation.snapshot_generation().get()
        )
    } else {
        wire
    };
    let wire = if matches!(fault, NavigationFault::DiscoveryScopeRefusal(_))
        && correlation.snapshot_generation().get() > 1
    {
        format!(
            r#"{{"v":1,"i":{},"g":{},"c":"complete","n":[{{"k":2,"r":"heading","l":1,"n":"Arrival certificate"}},{{"k":4,"r":"paragraph","t":"Visible product detail"}}]}}"#,
            correlation.invocation().get(),
            correlation.snapshot_generation().get()
        )
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
    if let NavigationFault::DiscoveryScopeRefusal(case) = fault {
        if case == 2 {
            let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
                panic!("{outcome:?}");
            };
            assert_eq!(
                closed.failure(),
                AgentWorkFailure::Browser(AgentBrowserProviderError::TurnLimit)
            );
            assert_eq!(closed.policy_settlement().closure().model_calls(), 5);
            assert_eq!(closed.policy_settlement().closure().operations(), 6);
        } else {
            let AgentWorkOutcome::Succeeded(mut success) = outcome else {
                panic!("{outcome:?}");
            };
            assert_eq!(success.closure().model_calls(), 5);
            assert_eq!(success.closure().operations(), 6);
            assert!(success.take_extraction().is_some());
        }
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind() == AgentWorkEventKind::InspectionRefused)
                .count(),
            if case == 2 { 2 } else { 1 }
        );
        assert_eq!(
            calls,
            [1, 2, 3, 2, 9, 3, 3, 4, 5, 6],
            "invalid target never dispatches a native capture"
        );
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        return;
    }
    if let NavigationFault::DiscoveryEvidence(foreign) = fault {
        if foreign {
            let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
                panic!("{outcome:?}");
            };
            assert!(matches!(
                closed.failure(),
                AgentWorkFailure::Browser(AgentBrowserProviderError::Extraction(_))
            ));
        } else {
            let AgentWorkOutcome::Succeeded(mut success) = outcome else {
                panic!("{outcome:?}");
            };
            assert_eq!(success.closure().model_calls(), 4);
            let result = success.take_extraction().unwrap();
            let SemanticExtractedValue::Text(value) = result.fields()[0].value() else {
                panic!();
            };
            let sources = result
                .sources(value.source_span())
                .unwrap()
                .collect::<Vec<_>>();
            assert_eq!(sources.len(), 2);
            let source = sources[0];
            assert!(source.observation < result.observation());
            assert!(source.captured_at.millis() < result.captured_at().millis());
            assert!(
                matches!(&source.content, SemanticOwnedReadContent::Text(text) if text == "Arrival certificate")
            );
            assert!(
                matches!(&sources[1].content, SemanticOwnedReadContent::Text(text) if text == "Departure certificate")
            );
            assert_ne!(sources[0].frame.context(), sources[1].frame.context());
            assert_eq!(
                sources[0].frame.context().identity(),
                sources[1].frame.context().identity()
            );
        }
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        assert!(!calls.contains(&7));
        return;
    }
    if let NavigationFault::DiscoveryBudget(limit, refusal, operations) = fault {
        let limit = limit.min((operations - 1) as u8);
        let calls_used = if refusal {
            let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
                panic!("{outcome:?}");
            };
            assert_eq!(
                closed.failure(),
                AgentWorkFailure::Browser(AgentBrowserProviderError::TurnLimit)
            );
            assert_eq!(
                closed.policy_settlement().closure().operations(),
                u32::from(limit)
            );
            assert!(closed.policy_settlement().closure().operations() < operations);
            closed.policy_settlement().closure().model_calls()
        } else {
            let AgentWorkOutcome::Succeeded(mut success) = outcome else {
                panic!("{outcome:?}");
            };
            assert_eq!(success.closure().navigations(), 1);
            assert_eq!(success.closure().operations(), u32::from(limit) + 1);
            assert!(success.closure().operations() <= operations);
            assert!(success.take_extraction().is_some());
            success.closure().model_calls()
        };
        assert_eq!(calls_used, u32::from(limit - u8::from(refusal)));
        assert_eq!(calls, [1, 2, 3, 2, 9, 3, 4, 5, 6]);
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        return;
    }
    if fault.two_discovery_hops() {
        return route_tests::assert_outcome(
            route_tests::RouteFault::None,
            outcome,
            shutdown,
            calls,
            events,
        );
    }
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
