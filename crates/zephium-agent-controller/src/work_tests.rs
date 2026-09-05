//! Deterministic actor fixtures; loopback only, no native handles or real page data.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use zephium_agent_runtime::{
    AgentRuntimeConfig, AgentRuntimeHandle, NativeEventSink, PendingAgentRuntime,
};

static SERIAL: Mutex<()> = Mutex::new(());

#[test]
fn main_document_keeps_unadmitted_frames_explicit_without_child_native_calls() {
    struct MainTask;
    impl AgentWorkTask for MainTask {
        fn evaluate(
            &mut self,
            observation: &SemanticObservation,
        ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
            assert_eq!(observation.frames().len(), 1);
            let [boundary] = observation.frame_boundaries() else {
                panic!("missing boundary disposition");
            };
            assert_eq!(
                boundary.status(),
                SemanticFrameBoundaryStatus::Unsupported(SemanticFrameUnsupported::PolicyBlocked)
            );
            assert_eq!(boundary.parent_frame(), FrameId::MAIN);
            assert_eq!(
                observation
                    .resolve_node(boundary.reference(), observation.frames()[0].frame())
                    .unwrap()
                    .role(),
                SemanticRole::FrameBoundary
            );
            let read = read_semantic_observation(
                observation,
                SemanticReadAuthority::Initial,
                SemanticCaptureInstant::from_millis(5),
                SemanticReadSensitivityLimit::PublicOnly,
                SemanticReadBudget::STANDARD,
            )
            .unwrap();
            assert!(read
                .omissions()
                .contains(SemanticReadOmission::SourceIncomplete));
            assert_eq!(read.stats().incomplete_frames(), 1);
            Ok(AgentWorkTaskProgress::Complete)
        }
        fn assess(
            &self,
            _: &SemanticPreparedAction,
        ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
            panic!("no action authority");
        }
        fn attest_account(
            &self,
            context: ContextJoin,
            now: AgentPolicyInstant,
        ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
            Task.attest_account(context, now)
        }
    }
    let _serial = lock(&SERIAL);
    let (controller, handle) = AgentWorkController::try_new(
        input(),
        AgentProviderTransportConfig::STANDARD,
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-not-a-secret".into(),
        )
        .unwrap(),
        Arc::new(Audit(Fault::None)),
        Box::new(MainTask),
    )
    .unwrap();
    let (outcome, shutdown, calls, events) = drive(controller, handle, Fault::EmbeddedFrame);
    assert!(matches!(outcome, AgentWorkOutcome::Succeeded(_)));
    assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
    assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
    assert!(!events.iter().any(|event| matches!(
        event.kind(),
        AgentWorkEventKind::ModelActive | AgentWorkEventKind::ActionActive
    )));
}

#[test]
fn baseline_read_capability_is_frozen_before_any_provider_or_native_action() {
    struct MutatingTask(bool);
    impl AgentWorkTask for MutatingTask {
        fn allows_baseline_read(&self) -> bool {
            self.0
        }
        fn evaluate(
            &mut self,
            _: &SemanticObservation,
        ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
            self.0 = false;
            Ok(AgentWorkTaskProgress::Continue)
        }
        fn assess(
            &self,
            _: &SemanticPreparedAction,
        ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
            panic!("mutated contract")
        }
        fn attest_account(
            &self,
            context: ContextJoin,
            now: AgentPolicyInstant,
        ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
            Task.attest_account(context, now)
        }
    }
    let _serial = lock(&SERIAL);
    let (controller, handle) = AgentWorkController::try_new(
        input(),
        AgentProviderTransportConfig::STANDARD,
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-not-a-secret".into(),
        )
        .unwrap(),
        Arc::new(Audit(Fault::None)),
        Box::new(MutatingTask(true)),
    )
    .unwrap();
    let (outcome, shutdown, calls, _) = drive(controller, handle, Fault::None);
    let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(closed.failure(), AgentWorkFailure::Contract);
    assert_eq!(closed.policy_settlement().closure().model_calls(), 0);
    assert_eq!(closed.policy_settlement().closure().effects(), 0);
    assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
    assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
}

#[test]
fn production_form_initial_completion_or_missing_field_never_calls_provider() {
    let _serial = lock(&SERIAL);
    for name in ["Field", "Missing"] {
        let input = input();
        let task = crate::AgentWorkFormTask::try_new_local_preparation(
            input.context.identity,
            input.context.origin.clone(),
            AgentAccountScope::Anonymous,
            vec![
                crate::AgentWorkFormPhase::try_new(vec![crate::AgentWorkFormGoal::fill(
                    Some(name.into()),
                    String::new(),
                )
                .unwrap()])
                .unwrap(),
            ],
        )
        .unwrap();
        let (controller, handle) = AgentWorkController::try_new(
            input,
            AgentProviderTransportConfig::STANDARD,
            AgentProviderCredential::try_new(
                AgentProviderKind::OpenAiResponses,
                "fixture-not-a-secret".into(),
            )
            .unwrap(),
            Arc::new(Audit(Fault::None)),
            Box::new(task),
        )
        .unwrap();
        let (outcome, shutdown, calls, events) = drive(controller, handle, Fault::None);
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
        assert!(!events
            .iter()
            .any(|event| matches!(event.kind(), AgentWorkEventKind::ModelActive)));
        let closure = if name == "Field" {
            let AgentWorkOutcome::Succeeded(success) = outcome else {
                panic!("already satisfied: {outcome:?}")
            };
            success.closure()
        } else {
            let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
                panic!("missing field: {outcome:?}")
            };
            assert_eq!(closed.failure(), AgentWorkFailure::Contract);
            closed.policy_settlement().closure()
        };
        assert_eq!(closure.effects(), 0);
        assert_eq!(closure.model_calls(), 0);
    }
}
#[cfg(feature = "probe-harness")]
#[path = "work_combined_tests.rs"]
mod combined_tests;
#[cfg(feature = "probe-harness")]
use combined_tests::{CombinedFault, CombinedTask};
#[cfg(feature = "probe-harness")]
#[path = "work_scoped_tests.rs"]
mod scoped_tests;
#[cfg(feature = "probe-harness")]
use scoped_tests::{ScopedFault, ScopedTask};
#[cfg(feature = "probe-harness")]
#[path = "work_read_tests.rs"]
mod read_tests;
#[cfg(feature = "probe-harness")]
use read_tests::{ReadFault, ReadTask};
#[cfg(feature = "probe-harness")]
#[path = "work_account_tests.rs"]
mod account_tests;
#[cfg(feature = "probe-harness")]
use account_tests::{AccountFault, AccountTask};
const FIXTURE_POLICY_NOW_MILLIS: u64 = 2;

#[test]
fn missing_schema_cannot_admit_combined_mode_or_certify_result_readiness() {
    let extraction = AgentWorkExtractionTask::try_new(
        vec![SemanticExtractionFieldSchema::try_text("label".into(), true, 64).unwrap()],
        AgentAccountScope::Anonymous,
    )
    .unwrap();
    assert!(!extraction.allows_subtree_extraction());
    let schema = extraction.extraction_schema().cloned();
    let extraction = extraction.with_subtree_extraction();
    assert!(extraction.allows_subtree_extraction());
    assert!(!extraction.allows_actions_before_extraction());
    assert_eq!(extraction.extraction_schema(), schema.as_ref());
    struct MissingSchema(bool, bool);
    impl AgentWorkTask for MissingSchema {
        fn allows_subtree_extraction(&self) -> bool {
            self.1
        }
        fn allows_actions_before_extraction(&self) -> bool {
            self.0
        }
        fn evaluate(
            &mut self,
            _: &SemanticObservation,
        ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
            Ok(AgentWorkTaskProgress::ReadyForExtraction)
        }
        fn assess(
            &self,
            _: &SemanticPreparedAction,
        ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
            panic!("missing schema cannot authorize an action")
        }
        fn attest_account(
            &self,
            context: ContextJoin,
            now: AgentPolicyInstant,
        ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
            Task.attest_account(context, now)
        }
    }
    let _serial = lock(&SERIAL);
    for (combined, subtree) in [(true, false), (false, true), (false, false)] {
        let admitted = AgentWorkController::try_new(
            input(),
            AgentProviderTransportConfig::STANDARD,
            AgentProviderCredential::try_new(
                AgentProviderKind::OpenAiResponses,
                "synthetic-not-a-secret".into(),
            )
            .unwrap(),
            Arc::new(Audit(Fault::None)),
            Box::new(MissingSchema(combined, subtree)),
        );
        if combined || subtree {
            assert!(matches!(admitted, Err(AgentWorkFailure::Contract)));
        } else {
            let (controller, handle) = admitted.unwrap();
            let (outcome, shutdown, calls, _) = drive(controller, handle, Fault::None);
            let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
                panic!("readiness without a schema cannot succeed: {outcome:?}");
            };
            assert_eq!(closed.failure(), AgentWorkFailure::Contract);
            assert_eq!(closed.policy_settlement().closure().model_calls(), 0);
            assert_eq!(closed.policy_settlement().closure().effects(), 0);
            assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
            assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        }
    }
}

#[test]
fn dormant_refusal_retains_original_identity_and_reconciles_only_exact_audit() {
    let _serial = lock(&SERIAL);
    let (controller, mut handle) = AgentWorkController::try_new(
        input(),
        AgentProviderTransportConfig::STANDARD,
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-not-a-secret".into(),
        )
        .unwrap(),
        Arc::new(Audit(Fault::None)),
        Box::new(Task),
    )
    .unwrap();
    let owner = AgentWorkIncarnation::generate();
    let admission = controller.journal_admission(owner).unwrap().next();
    drop(controller);
    let AgentWorkOutcome::Recovery(mut recovery) = handle.take_outcome().unwrap() else {
        panic!("dormant controller cannot succeed");
    };
    assert!(recovery.state.credential.is_none());
    assert!(recovery.state.transport.is_none());
    assert!(recovery.state.input.as_ref().unwrap().objective.is_none());
    assert_eq!(recovery.journal_admission(owner).unwrap().next(), admission);
    assert!(recovery.prepare_audit_reconciliation().unwrap().is_none());
    assert_eq!(recovery.failure(), AgentWorkFailure::Shutdown);
    let (outcome, _, _, _) = run(Fault::AuditLost);
    let AgentWorkOutcome::Recovery(mut recovery) = outcome else {
        panic!("lost audit cannot succeed");
    };
    let failure = recovery.failure();
    let delivery = recovery.prepare_audit_reconciliation().unwrap().unwrap();
    assert_eq!(
        recovery
            .prepare_audit_reconciliation()
            .unwrap()
            .unwrap()
            .proof(),
        delivery.proof()
    );
    recovery
        .settle_audit_reconciliation(
            delivery
                .proof()
                .settle(AgentAuditDeliveryOutcome::Committed),
        )
        .unwrap();
    while let Some(delivery) = recovery.prepare_audit_reconciliation().unwrap() {
        recovery
            .settle_audit_reconciliation(
                delivery
                    .proof()
                    .settle(AgentAuditDeliveryOutcome::Committed),
            )
            .unwrap();
    }
    let status = recovery.audit_reconciliation_status().unwrap();
    assert_eq!(status.pending(), 0);
    assert_eq!(status.in_flight(), 0);
    assert!(status.shutdown_sealed() && !status.fail_stopped());
    assert_eq!(recovery.failure(), failure);
}

struct Clock(AtomicU64);
impl TerraControllerClock for Clock {
    fn now(&self) -> Result<AgentPolicyInstant, super::super::TerraControllerClockError> {
        Ok(AgentPolicyInstant::from_millis(
            self.0.fetch_add(1, Ordering::Relaxed),
        ))
    }
}

struct Task;
impl AgentWorkTask for Task {
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        assert_eq!(observation.frames().len(), 1);
        Ok(AgentWorkTaskProgress::Complete)
    }
    fn assess(
        &self,
        _action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        Ok(AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            AgentAccountScope::Anonymous,
            now,
        ))
    }
}

fn input() -> AgentWorkRunInput {
    input_with_effects(&[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite])
}

fn input_with_effects(allowed: &[SemanticEffectClass]) -> AgentWorkRunInput {
    let profile = 1_u128.into();
    let context = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        profile,
        ContextKind::Owned,
    );
    let origin = SemanticOrigin::parse("https://work-fixture.invalid/").expect("origin");
    let effects = AgentEffectScope::try_new(allowed).expect("effects");
    let budget = AgentRunBudget::try_new(24, 1_000_000, 1_000_000, 1).expect("budget");
    let node = AgentPlanNodeId::generate();
    let policy_expires = FIXTURE_POLICY_NOW_MILLIS
        + zephium_agent_provider_transport::MAX_AGENT_PROVIDER_REQUEST_TIMEOUT_MILLIS;
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
        context.owner(),
        AgentRunScope::try_new(
            vec![profile],
            vec![AgentAccountScope::Anonymous],
            vec![origin.clone()],
            SemanticSensitivity::Public,
            effects,
            Vec::new(),
        )
        .expect("scope"),
        budget,
        AgentPolicyInstant::from_millis(1),
        AgentPolicyInstant::from_millis(policy_expires),
        vec![AgentPlanNodeScope::new(
            node,
            AgentPlanNodeAuthority::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin],
                SemanticSensitivity::Public,
                effects,
            )
            .expect("authority"),
            budget,
            AgentPolicyInstant::from_millis(policy_expires),
        )],
    )
    .expect("manifest");
    let ids = TerraControllerIds::try_new(
        AgentSupervisorId::new(1).expect("id"),
        AgentSupervisorAttemptId::new(1).expect("id"),
        AgentSupervisorCancellationId::new(1).expect("id"),
        AgentModelCallId::new(1).expect("id"),
        [1, 2, 3, 4].map(|raw| AgentAuditEventId::new(raw).expect("id")),
        AgentAuditDeliveryId::new(1).expect("id"),
    )
    .expect("ids");
    AgentWorkRunInput::try_new(
        manifest,
        AgentPlanLeaseBinding::new(AgentPlanLeaseId::generate(), node),
        AgentWorkContextSpec::try_new(
            context,
            ContextProfileStorageClass::Ephemeral,
            ContextNavigationTarget::parse("https://work-fixture.invalid/").expect("target"),
        )
        .expect("context"),
        "Verify a deterministic fixture.".to_owned(),
        AgentWorkRunSettings::new(
            AgentBrowserModel::Luna,
            ids,
            Arc::new(Clock(AtomicU64::new(FIXTURE_POLICY_NOW_MILLIS))),
            // Client construction can compete with other suites' TLS/client
            // setup. Use the production hard horizon for non-deadline fixtures;
            // explicit expiry cases install their own deadline after construction.
            Instant::now() + super::super::MAX_TERRA_CONTROLLER_HARD_DEADLINE,
        ),
    )
    .expect("input")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Fault {
    #[cfg(feature = "probe-harness")]
    Scoped(ScopedFault),
    None,
    EmbeddedFrame,
    ConstructDispatch,
    ConstructCallback,
    NavigateDispatch,
    NavigateTarget,
    Observe,
    NativeNonzero,
    AuditRefused,
    AuditLost,
    ObservationLost,
    CancellationLost,
    CancellationRefused,
    CancellationDuplicate,
    CleanupAuditRefused,
    CleanupAuditLost,
    CleanupAuditDuplicate,
    CleanupDeliveryRefused,
    CleanupDeliveryLost,
    CloseLost,
    CloseRefused,
    DeadlineObservation,
    ShutdownObservation,
    #[cfg(feature = "probe-harness")]
    ActionDispatch,
    #[cfg(feature = "probe-harness")]
    ActionCallback,
    #[cfg(feature = "probe-harness")]
    ActionVerification,
    #[cfg(feature = "probe-harness")]
    ActionApplied,
    #[cfg(feature = "probe-harness")]
    ActionBudget,
    #[cfg(feature = "probe-harness")]
    ActionCancel,
    #[cfg(feature = "probe-harness")]
    ActionLost,
    #[cfg(feature = "probe-harness")]
    ActionNeedsHuman,
    #[cfg(feature = "probe-harness")]
    ActionNeedsHumanAuditLost,
    #[cfg(feature = "probe-harness")]
    ActionNeedsHumanAuditRefused,
    #[cfg(feature = "probe-harness")]
    ActionNeedsHumanNativeLost,
    #[cfg(feature = "probe-harness")]
    ActionNeedsHumanTakeover,
    CancelObservation,
    TakeoverObservation,
    SuspendObservation,
    RevokeObservation,
    Readiness,
    RendererObservation,
    MailboxPressure,
}

#[cfg(feature = "probe-harness")]
impl Fault {
    fn unissued_human(self) -> bool {
        matches!(
            self,
            Self::ActionNeedsHuman
                | Self::ActionNeedsHumanAuditLost
                | Self::ActionNeedsHumanAuditRefused
                | Self::ActionNeedsHumanNativeLost
                | Self::ActionNeedsHumanTakeover
        )
    }
}

struct Audit(Fault);
impl AgentAuditPort for Audit {
    fn append(
        &self,
        batch: AgentAuditDelivery,
        completion: AgentAuditCompletion,
    ) -> AgentAuditDispatch {
        let proof = batch.proof();
        if matches!(self.0, Fault::AuditRefused | Fault::CleanupDeliveryRefused) {
            return AgentAuditDispatch::Refused(proof.settle(AgentAuditDeliveryOutcome::Refused(
                AgentAuditSinkFailure::AppendFailed,
            )));
        }
        if !matches!(self.0, Fault::AuditLost | Fault::CleanupDeliveryLost) {
            completion(proof.settle(AgentAuditDeliveryOutcome::Committed));
        }
        AgentAuditDispatch::Accepted(proof)
    }
}

struct Port {
    sink: NativeEventSink,
    fault: Fault,
    calls: Mutex<Vec<u8>>,
    control: Arc<Mutex<Option<AgentRuntimeHandle>>>,
}

impl Port {
    fn send(&self, event: ContextNativeEvent) {
        let _ = self.sink.publish(event);
    }
}

impl AgentBrowserPort for Port {
    fn dispatch(&self, request: ContextNativeRequest) -> ContextDispatch {
        let event = match request {
            ContextNativeRequest::Construct(request) => {
                lock(&self.calls).push(1);
                assert_eq!(request.source(), ContextConstructionSource::Owned);
                if self.fault == Fault::ConstructDispatch {
                    return ContextDispatch::Rejected(ContextPortFailure::NativeRefused);
                }
                ContextNativeEvent::ConstructionSettled(
                    ContextConstructionSettlement::try_new(
                        request.operation(),
                        if self.fault == Fault::ConstructCallback {
                            Err(ContextPortFailure::NativeRefused)
                        } else {
                            Ok(ContextConstructionProof::MacOsOwnedSelectedProfileExtensionFree)
                        },
                    )
                    .expect("construction"),
                )
            }
            ContextNativeRequest::Navigate(request) => {
                lock(&self.calls).push(2);
                if self.fault == Fault::NavigateDispatch {
                    return ContextDispatch::Unsupported;
                }
                let target = if self.fault == Fault::NavigateTarget {
                    ContextNavigationTarget::parse("https://other.invalid/").expect("target")
                } else {
                    request.target().clone()
                };
                ContextNativeEvent::NavigationSettled(
                    ContextNavigationSettlement::try_new(request.operation(), Ok(target))
                        .expect("navigation"),
                )
            }
            ContextNativeRequest::Cancel(request) => {
                lock(&self.calls).push(4);
                if self.fault == Fault::CancellationLost {
                    return ContextDispatch::Scheduled;
                }
                if self.fault == Fault::CancellationDuplicate {
                    self.send(ContextNativeEvent::CancellationSettled(
                        ContextCancellationSettlement::new(request.current(), Ok(())),
                    ));
                }
                ContextNativeEvent::CancellationSettled(ContextCancellationSettlement::new(
                    request.current(),
                    if self.fault == Fault::CancellationRefused {
                        Err(ContextPortFailure::NativeRefused)
                    } else {
                        Ok(())
                    },
                ))
            }
            ContextNativeRequest::Transition(request) => {
                lock(&self.calls).push(5);
                assert_eq!(request.operation().kind(), ContextOperationKind::Close);
                if self.fault == Fault::CloseLost {
                    return ContextDispatch::Scheduled;
                }
                ContextNativeEvent::TransitionSettled(
                    ContextTransitionSettlement::try_new(
                        request.operation(),
                        if self.fault == Fault::CloseRefused {
                            Err(ContextPortFailure::NativeRefused)
                        } else {
                            Ok(())
                        },
                    )
                    .expect("close"),
                )
            }
        };
        self.send(event);
        ContextDispatch::Scheduled
    }

    fn invoke_semantic(&self, invocation: SemanticRuntimeInvocation) -> ContextDispatch {
        lock(&self.calls).push(3);
        #[cfg(feature = "probe-harness")]
        if let Fault::Scoped(fault) = self.fault {
            return scoped_tests::capture(self, invocation, fault);
        }
        let correlation = invocation.correlation();
        assert_eq!(
            correlation.snapshot_generation().get(),
            if lock(&self.calls).contains(&7) { 2 } else { 1 }
        );
        let wire = format!("{{\"v\":1,\"i\":{},\"g\":{},\"c\":\"complete\",\"n\":[{{\"k\":1,\"r\":\"document\",\"o\":16}},{{\"k\":2,\"p\":0,\"r\":\"textbox\",\"n\":\"Field\",\"s\":64,\"o\":2,\"v\":{{\"k\":\"text\",\"value\":\"\"}},\"b\":{{\"x\":10,\"y\":20,\"w\":120,\"h\":30}}}}]}}", correlation.invocation().get(), correlation.snapshot_generation().get());
        let wire = if self.fault == Fault::EmbeddedFrame {
            wire.replace("]}", ",{\"k\":3,\"p\":0,\"r\":\"frame_boundary\"}]}")
        } else {
            wire
        };
        #[cfg(feature = "probe-harness")]
        let wire = if self.fault == Fault::ActionApplied && lock(&self.calls).contains(&7) {
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
        .expect("fixture snapshot");
        if matches!(
            self.fault,
            Fault::CancelObservation
                | Fault::ObservationLost
                | Fault::CancellationLost
                | Fault::CancellationRefused
                | Fault::CancellationDuplicate
                | Fault::CloseLost
                | Fault::CloseRefused
        ) {
            lock(&self.control)
                .as_ref()
                .expect("control")
                .cancel_and_seal();
        }
        if matches!(
            self.fault,
            Fault::ObservationLost | Fault::DeadlineObservation | Fault::ShutdownObservation
        ) {
            return ContextDispatch::Scheduled;
        }
        let reason = match self.fault {
            Fault::TakeoverObservation => Some(AgentRuntimeStopReason::HumanTakeover),
            Fault::SuspendObservation => Some(AgentRuntimeStopReason::Suspend),
            Fault::RevokeObservation => Some(AgentRuntimeStopReason::PolicyRevoked),
            _ => None,
        };
        if let Some(reason) = reason {
            let control = lock(&self.control);
            let control = control.as_ref().expect("control");
            control.stop_and_seal(reason);
            control.cancel_and_seal(); // Cannot overwrite the first reason.
        }
        if self.fault == Fault::RendererObservation {
            self.send(ContextNativeEvent::RendererLost(ContextRendererLoss::new(
                correlation.frame().context(),
            )));
        }
        if self.fault == Fault::MailboxPressure {
            for raw in 1..=32 {
                self.send(ContextNativeEvent::ResourceAuditSettled(
                    ContextResourceAuditSettlement::new(
                        ContextResourceAuditId::new(raw).expect("id"),
                        Err(ContextPortFailure::NativeRefused),
                    ),
                ));
            }
        }
        self.send(ContextNativeEvent::SemanticRuntimeSettled(Box::new(
            SemanticRuntimeSettlement::try_new(
                correlation,
                if matches!(
                    self.fault,
                    Fault::Observe
                        | Fault::CleanupAuditRefused
                        | Fault::CleanupAuditLost
                        | Fault::CleanupAuditDuplicate
                        | Fault::CleanupDeliveryRefused
                        | Fault::CleanupDeliveryLost
                ) {
                    Err(SemanticRuntimePortFailure::Transport)
                } else if self.fault == Fault::Readiness
                    && lock(&self.calls).iter().filter(|call| **call == 3).count() < 3
                {
                    Err(SemanticRuntimePortFailure::NotReady)
                } else {
                    Ok(snapshot)
                },
            )
            .expect("snapshot"),
        )));
        ContextDispatch::Scheduled
    }

    fn seal_for_shutdown(&self, audit: ContextResourceAuditId) -> ContextShutdownDispatch {
        lock(&self.calls).push(6);
        #[cfg(feature = "probe-harness")]
        if self.fault == Fault::ActionNeedsHumanNativeLost {
            return ContextShutdownDispatch::AuditScheduled;
        }
        #[cfg(feature = "probe-harness")]
        if self.fault == Fault::ActionNeedsHumanTakeover {
            lock(&self.control)
                .as_ref()
                .unwrap()
                .stop_and_seal(AgentRuntimeStopReason::HumanTakeover);
        }
        if self.fault == Fault::CleanupAuditLost {
            return ContextShutdownDispatch::AuditScheduled;
        }
        let snapshot = ContextNativeResourceSnapshot::try_new(ContextNativeResourceCounts {
            known_bindings: 0,
            resident_views: 0,
            owned_reservations: 0,
            borrowed_leases: 0,
            visible_surfaces: 0,
            suspended_views: 0,
            pending_operations: 0,
            pending_captures: 0,
            queued_tasks: u8::from(self.fault == Fault::NativeNonzero),
        })
        .expect("counts");
        if self.fault == Fault::CleanupAuditDuplicate {
            self.send(ContextNativeEvent::ShutdownAuditSettled(
                ContextShutdownAuditSettlement::new(audit, Ok(snapshot)),
            ));
        }
        self.send(ContextNativeEvent::ShutdownAuditSettled(
            ContextShutdownAuditSettlement::new(
                audit,
                if self.fault == Fault::CleanupAuditRefused {
                    Err(ContextPortFailure::NativeRefused)
                } else {
                    Ok(snapshot)
                },
            ),
        ));
        ContextShutdownDispatch::AuditScheduled
    }
    fn transfer_cookies(&self, _request: ContextCookieTransferRequest) -> ContextDispatch {
        ContextDispatch::Unsupported
    }
    fn audit_resources(&self, _audit: ContextResourceAuditId) -> ContextDispatch {
        ContextDispatch::Unsupported
    }
    #[cfg(not(feature = "probe-harness"))]
    fn execute_semantic_action(
        &self,
        _request: SemanticActionNativeRequest,
        _completion: SemanticActionNativeCompletion,
    ) -> ContextDispatch {
        panic!("completed task must not execute an action")
    }
    #[cfg(feature = "probe-harness")]
    fn execute_semantic_action(
        &self,
        request: SemanticActionNativeRequest,
        completion: SemanticActionNativeCompletion,
    ) -> ContextDispatch {
        lock(&self.calls).push(7);
        match self.fault {
            Fault::ActionDispatch => return ContextDispatch::Unsupported,
            Fault::ActionCallback => {}
            Fault::ActionVerification
            | Fault::ActionApplied
            | Fault::Scoped(ScopedFault::Combined) => {
                let now = request.requested_at();
                let geometry = request.expected_geometry();
                completion(request.complete(
                    SemanticActionExecutionBackend::PageWorldCompatibilityFill,
                    SemanticActionNativeReadiness::ExactConnectedWritableFormTarget,
                    SemanticActionNativeViewport::try_new(800, 600).unwrap(),
                    geometry,
                    now,
                    now,
                ));
                return ContextDispatch::Scheduled;
            }
            Fault::ActionCancel | Fault::ActionLost => {
                lock(&self.control)
                    .as_ref()
                    .expect("control")
                    .stop_and_seal(AgentRuntimeStopReason::HumanTakeover);
                if self.fault == Fault::ActionLost {
                    return ContextDispatch::Scheduled;
                }
            }
            _ => panic!("completed task must not execute an action"),
        }
        let now = request.requested_at();
        completion(request.fail(SemanticActionNativeFailure::AppliedUnverified, now));
        ContextDispatch::Scheduled
    }
    fn capture_semantic_screenshot(
        &self,
        _request: SemanticScreenshotNativeRequest,
        _completion: SemanticScreenshotNativeCompletion,
    ) -> ContextDispatch {
        ContextDispatch::Unsupported
    }
}

fn run(
    fault: Fault,
) -> (
    AgentWorkOutcome,
    AgentBrowserShutdownOutcome,
    Vec<u8>,
    Vec<AgentWorkEvent>,
) {
    let input = input();
    let credential = AgentProviderCredential::try_new(
        AgentProviderKind::OpenAiResponses,
        "fixture-not-a-secret".to_owned(),
    )
    .expect("credential");
    let (mut controller, handle) = AgentWorkController::try_new(
        input,
        AgentProviderTransportConfig::STANDARD,
        credential,
        Arc::new(Audit(fault)),
        Box::new(Task),
    )
    .expect("actor");
    if matches!(fault, Fault::AuditLost | Fault::DeadlineObservation) {
        // These test runtime callback expiry, not HTTP-client construction.
        let deadline = Instant::now() + Duration::from_millis(200);
        let state = controller.state.as_mut().unwrap();
        state.input.as_mut().unwrap().settings.deadline = deadline;
        state.native.deadline = deadline;
    }
    drive(controller, handle, fault)
}

fn drive(
    controller: AgentWorkController,
    handle: AgentWorkHandle,
    fault: Fault,
) -> (
    AgentWorkOutcome,
    AgentBrowserShutdownOutcome,
    Vec<u8>,
    Vec<AgentWorkEvent>,
) {
    drive_with_control(controller, handle, fault, Arc::new(Mutex::new(None)))
}

fn drive_with_control(
    controller: AgentWorkController,
    mut handle: AgentWorkHandle,
    fault: Fault,
    control: Arc<Mutex<Option<AgentRuntimeHandle>>>,
) -> (
    AgentWorkOutcome,
    AgentBrowserShutdownOutcome,
    Vec<u8>,
    Vec<AgentWorkEvent>,
) {
    let pending = PendingAgentRuntime::spawn_suspended_with_controller(
        AgentRuntimeConfig::STANDARD,
        Box::new(controller),
    )
    .expect("runtime");
    let port = Arc::new(Port {
        sink: pending.native_event_sink(),
        fault,
        calls: Mutex::new(Vec::new()),
        control,
    });
    let (runtime, completion, lifecycle) = pending.bind_browser_port(port.clone()).into_parts();
    let mut lifecycle = Some(lifecycle);
    let mut requested_shutdown = None;
    *lock(&port.control) = Some(runtime.clone());
    assert!(lock(&port.calls).is_empty());
    runtime.start_run().expect("run");
    let wait_deadline = Instant::now() + Duration::from_secs(12);
    let outcome = loop {
        // Never keep the diagnostic mutex across the blocking lifecycle join:
        // cleanup callbacks on the worker also append their call codes.
        let shutdown_ready = fault == Fault::ShutdownObservation && lock(&port.calls).contains(&3);
        if shutdown_ready {
            if let Some(lifecycle) = lifecycle.take() {
                requested_shutdown =
                    Some(lifecycle.shutdown_until(Instant::now() + Duration::from_secs(2)));
            }
        }
        if let Some(outcome) = handle.take_outcome() {
            break outcome;
        }
        if completion.is_stopped() {
            break handle
                .take_outcome()
                .expect("controller stopped without recovery ownership");
        }
        assert!(
            Instant::now() < wait_deadline,
            "actor fixture deadline: {fault:?}, calls={:?}",
            lock(&port.calls)
        );
        std::thread::sleep(Duration::from_millis(2));
    };
    let shutdown = requested_shutdown.unwrap_or_else(|| {
        lifecycle
            .expect("lifecycle")
            .shutdown_until(Instant::now() + Duration::from_secs(2))
    });
    let calls = lock(&port.calls).clone();
    let events = std::iter::from_fn(|| handle.take_event()).collect();
    (outcome, shutdown, calls, events)
}

#[test]
fn explicit_readiness_preserves_snapshot_generation_and_stop_reasons_are_first_wins() {
    let _guard = lock(&SERIAL);
    let (outcome, shutdown, calls, _) = run(Fault::Readiness);
    assert!(
        matches!(outcome, AgentWorkOutcome::Succeeded(_)),
        "{outcome:?}"
    );
    assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
    assert_eq!(calls, [1, 2, 3, 3, 3, 4, 5, 6]);
    for (fault, expected) in [
        (Fault::TakeoverObservation, AgentWorkFailure::HumanTakeover),
        (
            Fault::SuspendObservation,
            AgentWorkFailure::SuspendRequested,
        ),
        (Fault::RevokeObservation, AgentWorkFailure::PolicyRevoked),
    ] {
        let (outcome, shutdown, calls, _) = run(fault);
        let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
            panic!("settled stop must close unsuccessfully: {outcome:?}");
        };
        assert_eq!(closed.failure(), expected);
        assert!(matches!(
            closed.policy_settlement().closure().outcome(),
            AgentRunProgressOutcome::Cancelled(_)
        ));
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        assert_eq!(calls.iter().filter(|call| **call == 4).count(), 1);
        assert_eq!(calls.iter().filter(|call| **call == 5).count(), 1);
    }
}

#[cfg(feature = "probe-harness")]
#[test]
fn variable_provider_turns_use_real_worker_io_and_stop_at_the_exact_ceiling() {
    let _guard = lock(&SERIAL);
    provider_fixture(ProviderFault::Ceiling);
}

#[cfg(feature = "probe-harness")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProviderFault {
    Read(ReadFault),
    Scoped(ScopedFault),
    Combined(CombinedFault),
    Ceiling,
    CountRefused,
    StreamRefused,
    CancelCount,
    CancelStream,
    Native(Fault),
    Extraction(ExtractionFault),
}

#[cfg(feature = "probe-harness")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExtractionFault {
    None,
    WrongSchema,
    ExpandedScope,
    WrongKind,
    ForeignSource,
    CancelCount,
    CancelStream,
    AuditLost,
}

#[cfg(feature = "probe-harness")]
#[test]
fn extraction_uses_the_same_worker_and_never_publishes_failed_or_unsettled_output() {
    let _guard = lock(&SERIAL);
    for fault in [
        ExtractionFault::None,
        ExtractionFault::WrongSchema,
        ExtractionFault::ExpandedScope,
        ExtractionFault::WrongKind,
        ExtractionFault::ForeignSource,
        ExtractionFault::CancelCount,
        ExtractionFault::CancelStream,
        ExtractionFault::AuditLost,
    ] {
        provider_fixture(ProviderFault::Extraction(fault));
    }
}

#[cfg(feature = "probe-harness")]
#[test]
fn native_action_refusal_takeover_and_callback_loss_keep_the_original_effect_owner() {
    let _guard = lock(&SERIAL);
    for fault in [
        Fault::ActionDispatch,
        Fault::ActionCallback,
        Fault::ActionVerification,
        Fault::ActionBudget,
        Fault::ActionCancel,
        Fault::ActionLost,
        Fault::ActionNeedsHuman,
        Fault::ActionNeedsHumanAuditLost,
        Fault::ActionNeedsHumanAuditRefused,
        Fault::ActionNeedsHumanNativeLost,
        Fault::ActionNeedsHumanTakeover,
    ] {
        provider_fixture(ProviderFault::Native(fault));
    }
}

#[cfg(feature = "probe-harness")]
#[test]
fn settled_provider_refusals_and_count_stream_cancellation_close_without_success() {
    let _guard = lock(&SERIAL);
    for fault in [
        ProviderFault::CountRefused,
        ProviderFault::StreamRefused,
        ProviderFault::CancelCount,
        ProviderFault::CancelStream,
    ] {
        provider_fixture(fault);
    }
}

#[cfg(feature = "probe-harness")]
fn provider_fixture(fault: ProviderFault) {
    provider_fixture_with_form(fault, None);
}

#[cfg(feature = "probe-harness")]
#[test]
fn production_form_task_keeps_actor_refusal_and_callback_owners() {
    let _serial = lock(&SERIAL);
    for fault in [
        ProviderFault::Native(Fault::ActionApplied),
        ProviderFault::Native(Fault::ActionDispatch),
        ProviderFault::Native(Fault::ActionCallback),
        ProviderFault::Native(Fault::ActionVerification),
        ProviderFault::Native(Fault::ActionCancel),
        ProviderFault::Native(Fault::ActionLost),
        ProviderFault::Native(Fault::ActionNeedsHuman),
        ProviderFault::Native(Fault::ActionBudget),
        ProviderFault::CountRefused,
        ProviderFault::StreamRefused,
        ProviderFault::CancelCount,
        ProviderFault::CancelStream,
    ] {
        provider_fixture_with_form(fault, Some("fixture value"));
    }
    provider_fixture_with_form(
        ProviderFault::Native(Fault::ActionApplied),
        Some("not authorized"),
    );
}

#[cfg(feature = "probe-harness")]
fn provider_fixture_with_form(fault: ProviderFault, form: Option<&str>) {
    provider_fixture_with_account(fault, form, None);
}

#[cfg(feature = "probe-harness")]
fn provider_fixture_with_account(
    fault: ProviderFault,
    form: Option<&str>,
    account: Option<AccountFault>,
) {
    use std::io::{Read as _, Write as _};
    struct Continue;
    impl AgentWorkTask for Continue {
        fn evaluate(
            &mut self,
            _: &SemanticObservation,
        ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
            Ok(AgentWorkTaskProgress::Continue)
        }
        fn assess(
            &self,
            action: &SemanticPreparedAction,
        ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
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
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    listener.set_nonblocking(true).expect("nonblocking");
    let endpoint = format!(
        "http://{}/v1/responses",
        listener.local_addr().expect("address")
    );
    let control = Arc::new(Mutex::new(None::<AgentRuntimeHandle>));
    let server_control = control.clone();
    let account_clock = account.map(|_| Arc::new(Clock(AtomicU64::new(FIXTURE_POLICY_NOW_MILLIS))));
    let server_clock = account_clock.clone();
    let requests = if account.is_some_and(|fault| fault != AccountFault::Slow) {
        2
    } else {
        match fault {
            ProviderFault::Read(fault) => fault.requests(),
            ProviderFault::Scoped(fault) => fault.requests(),
            ProviderFault::Combined(
                CombinedFault::Premature
                | CombinedFault::SchemaMutation
                | CombinedFault::ModeMutation
                | CombinedFault::CompleteWithoutResult
                | CombinedFault::ActionLost,
            ) => 2,
            ProviderFault::Combined(
                CombinedFault::ExtraAction
                | CombinedFault::WrongSchema
                | CombinedFault::ExpandedScope,
            ) => 4,
            ProviderFault::Combined(CombinedFault::CancelMapCount) => 5,
            ProviderFault::Combined(CombinedFault::Ceiling) => 16,
            ProviderFault::Combined(_) => 6,
            ProviderFault::Ceiling => 16,
            ProviderFault::CountRefused | ProviderFault::CancelCount => 1,
            ProviderFault::Extraction(ExtractionFault::CancelCount) => 3,
            ProviderFault::Extraction(
                ExtractionFault::WrongSchema | ExtractionFault::ExpandedScope,
            ) => 2,
            ProviderFault::Extraction(_) => 4,
            _ => 2,
        }
    };
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut turns = 0;
        for _ in 0..requests {
            let (mut socket, _) = loop {
                match listener.accept() {
                    Ok(socket) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "bounded provider fixture");
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => panic!("loopback accept"),
                }
            };
            // Darwin accept inherits O_NONBLOCK from the listener. The bounded
            // blocking fixture reader must opt out before installing its timeout.
            socket
                .set_nonblocking(false)
                .expect("blocking fixture reader");
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("timeout");
            let mut request = Vec::new();
            let mut bytes = [0; 4096];
            let (header, expected) = loop {
                let count = socket.read(&mut bytes).expect("bounded request");
                assert!(count > 0 && request.len() + count <= 512 * 1024);
                request.extend_from_slice(&bytes[..count]);
                if let Some(header) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                    let head = std::str::from_utf8(&request[..header]).expect("HTTP fixture");
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().expect("length"))
                        })
                        .expect("content length");
                    break (header, header + 4 + length);
                }
            };
            while request.len() < expected {
                let count = socket.read(&mut bytes).expect("request body");
                assert!(count > 0 && request.len() + count <= 512 * 1024);
                request.extend_from_slice(&bytes[..count]);
            }
            let is_count = request.starts_with(b"POST /v1/responses/input_tokens ");
            let read_stop = if let ProviderFault::Read(fault) = fault {
                fault.stop(turns, is_count)
            } else {
                None
            };
            let cancelled = read_stop.is_some()
                || matches!(fault, ProviderFault::Scoped(fault) if fault.cancelled(turns, is_count))
                || (turns == 2
                    && ((is_count
                        && fault == ProviderFault::Combined(CombinedFault::CancelMapCount))
                        || (!is_count
                            && fault == ProviderFault::Combined(CombinedFault::CancelMapStream))))
                || (is_count && fault == ProviderFault::CancelCount)
                || (!is_count && fault == ProviderFault::CancelStream)
                || (turns == 1
                    && ((is_count
                        && fault == ProviderFault::Extraction(ExtractionFault::CancelCount))
                        || (!is_count
                            && fault == ProviderFault::Extraction(ExtractionFault::CancelStream))));
            if cancelled {
                lock(&server_control)
                    .as_ref()
                    .expect("control")
                    .stop_and_seal(read_stop.unwrap_or(AgentRuntimeStopReason::HumanTakeover));
            }
            let refused = (is_count && fault == ProviderFault::CountRefused)
                || (!is_count && fault == ProviderFault::StreamRefused)
                || matches!(fault, ProviderFault::Read(fault) if fault.refused(turns, is_count));
            if let ProviderFault::Read(fault) = fault {
                fault.check_request(&request[header + 4..], turns);
            }
            let (kind, body) = if is_count {
                (
                    "application/json",
                    r#"{"object":"response.input_tokens","input_tokens":17}"#.to_owned(),
                )
            } else {
                assert!(std::str::from_utf8(&request[header + 4..])
                    .expect("synthetic request")
                    .contains("\"store\":false"));
                turns += 1;
                (
                    "text/event-stream",
                    if let ProviderFault::Read(fault) = fault {
                        fault.stream(turns)
                    } else if let ProviderFault::Scoped(fault) = fault {
                        fault.stream(turns)
                    } else if let ProviderFault::Combined(fault) = fault {
                        if fault == CombinedFault::Ceiling && turns < 7 {
                            tool_stream(turns, false)
                        } else if fault == CombinedFault::Ceiling && turns == 7 {
                            tool_stream(turns, true)
                        } else if fault == CombinedFault::Ceiling {
                            named_tool_stream(
                                turns,
                                "extract",
                                r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
                            )
                        } else if turns == 1 && fault != CombinedFault::Premature {
                            tool_stream(turns, true)
                        } else if turns <= 2 {
                            if fault == CombinedFault::ExtraAction {
                                tool_stream(turns, true)
                            } else {
                                let arguments = if fault == CombinedFault::WrongSchema {
                                    r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":2}"#
                                } else if fault == CombinedFault::ExpandedScope {
                                    r#"{\"scope\":{\"kind\":\"subtree\",\"target\":\"@a2\"},\"schema_id\":1}"#
                                } else {
                                    r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#
                                };
                                named_tool_stream(turns, "extract", arguments)
                            }
                        } else {
                            extraction_stream(if fault == CombinedFault::ForeignSource {
                                ExtractionFault::ForeignSource
                            } else {
                                ExtractionFault::None
                            })
                            .replace("resp_2", "resp_3")
                            .replace("msg_2", "msg_3")
                        }
                    } else if let ProviderFault::Extraction(fault) = fault {
                        if turns == 1 {
                            let arguments = match fault {
                                ExtractionFault::WrongSchema => {
                                    r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":2}"#
                                }
                                ExtractionFault::ExpandedScope => {
                                    r#"{\"scope\":{\"kind\":\"subtree\",\"target\":\"@a2\"},\"schema_id\":1}"#
                                }
                                _ => r#"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"#,
                            };
                            named_tool_stream(turns, "extract", arguments)
                        } else {
                            extraction_stream(fault)
                        }
                    } else {
                        let stream = tool_stream(turns, matches!(fault, ProviderFault::Native(_)));
                        if fault == ProviderFault::Native(Fault::ActionBudget) {
                            stream.replace("settle_millis\\\":2000", "settle_millis\\\":1000")
                        } else {
                            stream
                        }
                    },
                )
            };
            let status = if refused {
                "503 Service Unavailable"
            } else {
                "200 OK"
            };
            if !is_count {
                if let Some(clock) = &server_clock {
                    // A slow completed provider turn, without a wall-clock sleep.
                    clock.0.fetch_add(31_000, Ordering::Relaxed);
                }
            }
            let response = write!(socket, "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            assert!(cancelled || response.is_ok());
        }
        turns
    });
    let transport = AgentProviderTransport::try_new_loopback(
        AgentProviderTransportConfig::STANDARD,
        &endpoint,
        "http://127.0.0.1:9/v1/messages",
    )
    .expect("loopback transport");
    let credential = AgentProviderCredential::try_new(
        AgentProviderKind::OpenAiResponses,
        "fixture-not-a-secret".to_owned(),
    )
    .expect("credential");
    let mut approved = if matches!(fault, ProviderFault::Native(native) if native.unissued_human())
    {
        input_with_effects(&[SemanticEffectClass::Read])
    } else {
        input()
    };
    if let Some(clock) = account_clock {
        approved.settings.clock = clock;
    }
    let task: Box<dyn AgentWorkTask> = if let Some(value) = form {
        Box::new(
            crate::AgentWorkFormTask::try_new_local_preparation(
                approved.context.identity,
                approved.context.origin.clone(),
                AgentAccountScope::Anonymous,
                vec![
                    crate::AgentWorkFormPhase::try_new(vec![crate::AgentWorkFormGoal::fill(
                        Some("Field".into()),
                        value.into(),
                    )
                    .unwrap()])
                    .unwrap(),
                ],
            )
            .unwrap(),
        )
    } else if let ProviderFault::Read(fault) = fault {
        Box::new(ReadTask::new(&approved, fault))
    } else if let ProviderFault::Scoped(fault) = fault {
        Box::new(ScopedTask::new(fault))
    } else if let ProviderFault::Combined(fault) = fault {
        Box::new(CombinedTask {
            extraction: AgentWorkExtractionTask::try_new(
                vec![SemanticExtractionFieldSchema::try_text("label".into(), true, 64).unwrap()],
                AgentAccountScope::Anonymous,
            )
            .unwrap(),
            fault,
            ready: None,
        })
    } else if matches!(fault, ProviderFault::Extraction(_)) {
        Box::new(
            AgentWorkExtractionTask::try_new(
                vec![SemanticExtractionFieldSchema::try_text("label".into(), true, 64).unwrap()],
                AgentAccountScope::Anonymous,
            )
            .unwrap(),
        )
    } else {
        Box::new(Continue)
    };
    let task: Box<dyn AgentWorkTask> = match account {
        Some(AccountFault::Static) => task,
        Some(fault) => Box::new(AccountTask::new(task, fault, control.clone())),
        None => task,
    };
    let audit_fault = if matches!(
        fault,
        ProviderFault::Extraction(ExtractionFault::AuditLost)
            | ProviderFault::Combined(CombinedFault::AuditLost)
            | ProviderFault::Scoped(ScopedFault::AuditLost)
            | ProviderFault::Native(Fault::ActionNeedsHumanAuditLost)
            | ProviderFault::Read(ReadFault::AuditLost)
    ) {
        Fault::AuditLost
    } else if fault == ProviderFault::Native(Fault::ActionNeedsHumanAuditRefused) {
        Fault::AuditRefused
    } else {
        Fault::None
    };
    let (mut controller, handle) = AgentWorkController::try_new_for_probe(
        approved,
        transport,
        credential,
        Arc::new(Audit(audit_fault)),
        task,
        AgentBrowserRetention::Stateless,
    )
    .expect("actor");
    if audit_fault == Fault::AuditLost {
        // This case deliberately waits for missing audit delivery. Preserve its
        // original ten-second timeout, starting after all client construction.
        let deadline = Instant::now() + Duration::from_secs(10);
        let state = controller.state.as_mut().unwrap();
        state.input.as_mut().unwrap().settings.deadline = deadline;
        state.native.deadline = deadline;
    }
    let native_fault = if let ProviderFault::Read(fault) = fault {
        if fault == ReadFault::ActionLost {
            Fault::ActionLost
        } else {
            Fault::ActionApplied
        }
    } else if let ProviderFault::Scoped(fault) = fault {
        Fault::Scoped(fault)
    } else if fault == ProviderFault::Combined(CombinedFault::ActionLost) {
        Fault::ActionLost
    } else if matches!(fault, ProviderFault::Combined(_)) {
        Fault::ActionApplied
    } else if let ProviderFault::Native(fault) = fault {
        fault
    } else {
        Fault::None
    };
    let (outcome, shutdown, calls, events) =
        drive_with_control(controller, handle, native_fault, control);
    if let Some(account) = account.filter(|fault| *fault != AccountFault::Slow) {
        assert_eq!(server.join().expect("account fixture server"), 1);
        account_tests::assert_refusal(account, outcome, shutdown, &calls);
        return;
    }
    if let ProviderFault::Read(fault) = fault {
        assert_eq!(server.join().expect("read fixture server"), requests / 2);
        read_tests::assert_outcome(fault, outcome, shutdown, &calls, &events);
        return;
    }
    if form.is_some() && fault == ProviderFault::Native(Fault::ActionApplied) {
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        if form == Some("fixture value") {
            let AgentWorkOutcome::Succeeded(success) = outcome else {
                panic!("form must succeed: {outcome:?}")
            };
            assert_eq!(success.closure().effects(), 1);
            assert_eq!(success.closure().model_calls(), 1);
            assert_eq!(calls, [1, 2, 3, 7, 3, 4, 5, 6]);
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind() == AgentWorkEventKind::Verified)
                    .count(),
                1
            );
        } else {
            let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
                panic!("unapproved form value must close: {outcome:?}")
            };
            assert_eq!(closed.failure(), AgentWorkFailure::Contract);
            assert_eq!(closed.policy_settlement().closure().effects(), 0);
            assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
        }
        assert_eq!(server.join().unwrap(), 1);
        return;
    }
    if let ProviderFault::Scoped(fault) = fault {
        assert_eq!(server.join().expect("scoped fixture server"), requests / 2);
        scoped_tests::assert_outcome(fault, outcome, shutdown, &calls, &events);
        return;
    }
    if let ProviderFault::Combined(fault) = fault {
        assert_eq!(
            server.join().expect("combined fixture server"),
            requests / 2
        );
        combined_tests::assert_outcome(fault, outcome, shutdown, &calls, &events);
        return;
    }
    if let ProviderFault::Extraction(fault) = fault {
        assert_eq!(server.join().expect("fixture server"), requests / 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.kind(), AgentWorkEventKind::ModelSettled { .. }))
                .count(),
            if matches!(
                fault,
                ExtractionFault::WrongSchema | ExtractionFault::ExpandedScope
            ) {
                1
            } else {
                2
            }
        );
        if fault == ExtractionFault::None {
            let AgentWorkOutcome::Succeeded(mut success) = outcome else {
                panic!("{outcome:?}");
            };
            assert_eq!(success.closure().model_calls(), 2);
            assert_eq!(success.closure().effects(), 0);
            let result = success.take_extraction().expect("owned result");
            assert!(success.take_extraction().is_none());
            assert_eq!(result.trust(), SemanticExtractionTrust::ModelMapped);
            let SemanticExtractedValue::Text(value) = result.fields()[0].value() else {
                panic!();
            };
            assert_eq!(value.as_str(), "Field");
            assert!(
                matches!(&result.sources(value.source_span()).unwrap().next().unwrap().content, SemanticOwnedReadContent::Text(text) if text == "Field")
            );
            assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
            assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
        } else if fault == ExtractionFault::AuditLost {
            let AgentWorkOutcome::Recovery(recovery) = outcome else {
                panic!("audit debt cannot close: {outcome:?}");
            };
            if let Some(session) = recovery.state.session.as_ref() {
                assert!(session.credential.is_none());
                assert!(session.extraction_output.is_none());
                assert_eq!(session.policy.pending_model_calls(), 0);
                assert_eq!(session.policy.pending_effects(), 0);
                assert!(session.transport.snapshot().unwrap().is_idle());
            } else {
                assert_eq!(fault, ExtractionFault::AuditLost);
                let drained = recovery.state.drained.as_ref().unwrap();
                assert!(drained.provider.is_some());
                assert_eq!(drained.policy.as_ref().unwrap().pending_model_calls(), 0);
                assert!(drained.journal.as_ref().unwrap().audit.status().in_flight() > 0);
                assert!(recovery.state.extraction.is_some());
            }
            assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Unclean));
            assert!(!events
                .iter()
                .any(|event| event.kind() == AgentWorkEventKind::Terminal));
        } else {
            let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
                panic!("settled invalid result must close without publication: {outcome:?}");
            };
            assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
            assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
            assert_eq!(closed.policy_settlement().closure().effects(), 0);
            if matches!(
                fault,
                ExtractionFault::CancelCount | ExtractionFault::CancelStream
            ) {
                assert_eq!(closed.failure(), AgentWorkFailure::HumanTakeover);
                assert!(matches!(
                    closed.policy_settlement().closure().outcome(),
                    AgentRunProgressOutcome::Cancelled(_)
                ));
            } else if matches!(
                fault,
                ExtractionFault::WrongKind | ExtractionFault::ForeignSource
            ) {
                assert!(matches!(
                    closed.failure(),
                    AgentWorkFailure::Browser(AgentBrowserProviderError::Extraction(_))
                ));
            } else if matches!(
                fault,
                ExtractionFault::WrongSchema | ExtractionFault::ExpandedScope
            ) {
                assert_eq!(
                    closed.failure(),
                    AgentWorkFailure::Browser(AgentBrowserProviderError::UnsupportedTool(
                        AgentBrowserToolKind::Extract
                    ))
                );
            }
        }
        return;
    }
    if matches!(
        fault,
        ProviderFault::Native(
            Fault::ActionNeedsHumanAuditLost
                | Fault::ActionNeedsHumanAuditRefused
                | Fault::ActionNeedsHumanNativeLost
        )
    ) {
        let AgentWorkOutcome::Recovery(recovery) = outcome else {
            panic!("unsettled refusal closure: {outcome:?}")
        };
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Unclean));
        assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
        let drained = recovery
            .state
            .drained
            .as_ref()
            .expect("original drained cohort");
        assert!(drained
            .proposal_refusal
            .as_ref()
            .unwrap()
            .human_review()
            .is_some());
        assert!(
            drained.policy.is_some() && drained.journal.is_some() && drained.provider.is_some()
        );
        assert_eq!(
            drained.proof.is_none(),
            fault == ProviderFault::Native(Fault::ActionNeedsHumanNativeLost)
        );
        assert!(!events.iter().any(|event| matches!(
            event.kind(),
            AgentWorkEventKind::Verified | AgentWorkEventKind::Terminal
        )));
        assert_eq!(server.join().unwrap(), 1);
        return;
    }
    if !matches!(fault, ProviderFault::Native(native) if !matches!(native, Fault::ActionBudget | Fault::ActionNeedsHuman | Fault::ActionNeedsHumanTakeover))
    {
        let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
            panic!("settled provider refusal must close: {fault:?}: {outcome:?}");
        };
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
        assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
        let closure = closed.policy_settlement().closure();
        assert_eq!(closure.effects(), 0);
        if fault == ProviderFault::Native(Fault::ActionNeedsHuman) {
            assert_eq!(
                closure.outcome(),
                AgentRunProgressOutcome::Failed(AgentSupervisorFailure::PolicyDenied)
            );
            assert_eq!(
                closed
                    .human_review()
                    .expect("exact unissued refusal")
                    .reason(),
                AgentNeedsHumanReason::ScopeExpansion
            );
            assert!(events.iter().any(|event| event.kind()
                == AgentWorkEventKind::NeedsHuman(AgentNeedsHumanReason::ScopeExpansion)));
        } else {
            assert!(closed.human_review().is_none());
        }
        if fault == ProviderFault::Native(Fault::ActionNeedsHumanTakeover) {
            assert!(matches!(
                closure.outcome(),
                AgentRunProgressOutcome::Cancelled(_)
            ));
        }
        assert_eq!(
            closure.model_calls(),
            if fault == ProviderFault::Ceiling {
                8
            } else {
                1
            }
        );
        match fault {
            ProviderFault::Ceiling => assert_eq!(
                closed.failure(),
                AgentWorkFailure::Browser(AgentBrowserProviderError::TurnLimit)
            ),
            ProviderFault::CancelCount | ProviderFault::CancelStream => {
                assert_eq!(closed.failure(), AgentWorkFailure::HumanTakeover);
                assert!(matches!(
                    closure.outcome(),
                    AgentRunProgressOutcome::Cancelled(_)
                ));
            }
            ProviderFault::Native(Fault::ActionBudget) => assert_eq!(
                closed.failure(),
                AgentWorkFailure::Browser(AgentBrowserProviderError::Action(
                    crate::AgentBrowserActionError::SettleBudget
                ))
            ),
            _ => assert!(matches!(closed.failure(), AgentWorkFailure::Browser(_))),
        }
        assert_eq!(server.join().expect("fixture server"), requests / 2);
        return;
    }
    let AgentWorkOutcome::Recovery(recovery) = outcome else {
        panic!("native debt cannot close: {fault:?}: {outcome:?}");
    };
    if fault == ProviderFault::Ceiling {
        assert_eq!(
            recovery.failure(),
            AgentWorkFailure::Browser(AgentBrowserProviderError::TurnLimit)
        );
    } else if matches!(
        fault,
        ProviderFault::CancelCount
            | ProviderFault::CancelStream
            | ProviderFault::Native(Fault::ActionCancel | Fault::ActionLost)
    ) {
        assert_eq!(recovery.failure(), AgentWorkFailure::HumanTakeover);
    } else {
        assert!(matches!(recovery.failure(), AgentWorkFailure::Browser(_)));
    }
    let session = recovery.state.session.as_ref().expect("retained session");
    assert!(session.credential.is_none());
    assert_eq!(session.policy.pending_model_calls(), 0);
    assert!(session.transport.snapshot().expect("transport").is_idle());
    assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Unclean));
    if let ProviderFault::Native(fault) = fault {
        if fault == Fault::ActionBudget {
            assert_eq!(
                recovery.failure(),
                AgentWorkFailure::Browser(AgentBrowserProviderError::Action(
                    crate::AgentBrowserActionError::SettleBudget
                ))
            );
            assert_eq!(calls, [1, 2, 3, 4, 5]);
            assert!(session.action.is_none());
            assert_eq!(session.policy.pending_effects(), 0);
        } else {
            if fault == Fault::ActionVerification {
                assert_eq!(calls, [1, 2, 3, 7, 3, 4, 5]);
                assert_eq!(
                    recovery.failure(),
                    AgentWorkFailure::Browser(AgentBrowserProviderError::Action(
                        crate::AgentBrowserActionError::Verification(
                            SemanticVerificationError::OutcomeNotObserved
                        )
                    ))
                );
            } else {
                assert_eq!(calls, [1, 2, 3, 7, 4, 5]);
            }
            let action = session
                .action
                .as_ref()
                .expect("original action and batch retained");
            if matches!(
                fault,
                Fault::ActionDispatch | Fault::ActionCallback | Fault::ActionVerification
            ) {
                assert!(
                    action.retained_failure().is_some(),
                    "charged failed effect retained"
                );
                assert_eq!(session.policy.pending_effects(), 0);
            } else {
                assert_eq!(session.policy.pending_effects(), 1);
                assert_eq!(
                    recovery.state.native.action_pending,
                    fault == Fault::ActionLost
                );
            }
        }
        assert!(!events.iter().any(|event| matches!(
            event.kind(),
            AgentWorkEventKind::Verified | AgentWorkEventKind::Terminal
        )));
    } else {
        assert_eq!(calls, [1, 2, 3, 4, 5]);
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.kind(), AgentWorkEventKind::ModelSettled { .. }))
            .count(),
        if fault == ProviderFault::Ceiling {
            8
        } else {
            1
        }
    );
    assert_eq!(server.join().expect("fixture server"), requests / 2);
}

#[cfg(feature = "probe-harness")]
fn tool_stream(turn: u8, native: bool) -> String {
    let name = if native { "act" } else { "locate" };
    let arguments = if native {
        r#"{\"actions\":[{\"kind\":\"fill\",\"target\":\"@a2\",\"value\":\"fixture value\",\"effect\":\"local_write\",\"wait\":{\"kind\":\"immediate\"},\"verification\":{\"kind\":\"target_value_matches_input\"},\"settle_millis\":2000}]}"#
    } else {
        r#"{\"semantic_query\":\"document\",\"scope\":{\"kind\":\"initial\"}}"#
    };
    named_tool_stream(turn, name, arguments)
}

#[cfg(feature = "probe-harness")]
fn named_tool_stream(turn: u8, name: &str, arguments: &str) -> String {
    let item = format!(
        r#"{{"type":"function_call","id":"fc_{turn}","call_id":"call_{turn}","name":"{name}","arguments":"{arguments}","status":"completed"}}"#
    );
    let pending = format!(
        r#"{{"type":"function_call","id":"fc_{turn}","call_id":"call_{turn}","name":"{name}","arguments":"","status":"in_progress"}}"#
    );
    let events = [
        (
            "response.created",
            format!(
                r#"{{"type":"response.created","response":{{"id":"resp_{turn}","status":"in_progress","model":"gpt-5.6-luna","service_tier":"default"}}}}"#
            ),
        ),
        (
            "response.output_item.added",
            format!(r#"{{"type":"response.output_item.added","output_index":0,"item":{pending}}}"#),
        ),
        (
            "response.function_call_arguments.delta",
            format!(
                r#"{{"type":"response.function_call_arguments.delta","item_id":"fc_{turn}","delta":"{arguments}"}}"#
            ),
        ),
        (
            "response.function_call_arguments.done",
            format!(
                r#"{{"type":"response.function_call_arguments.done","item_id":"fc_{turn}","name":"{name}","arguments":"{arguments}"}}"#
            ),
        ),
        (
            "response.output_item.done",
            format!(r#"{{"type":"response.output_item.done","output_index":0,"item":{item}}}"#),
        ),
        (
            "response.completed",
            format!(
                r#"{{"type":"response.completed","response":{{"id":"resp_{turn}","status":"completed","model":"gpt-5.6-luna","service_tier":"default","output":[{item}],"usage":{{"input_tokens":17,"output_tokens":3,"total_tokens":20,"input_tokens_details":{{"cached_tokens":0}},"output_tokens_details":{{"reasoning_tokens":0}}}}}}}}"#
            ),
        ),
    ];
    events
        .into_iter()
        .map(|(event, body)| format!("event: {event}\ndata: {body}\n\n"))
        .collect::<String>()
        + "data: [DONE]\n\n"
}

#[test]
fn trusted_completion_closes_original_context_audit_policy_provider_and_runtime() {
    let _guard = lock(&SERIAL);
    let (outcome, shutdown, calls, events) = run(Fault::None);
    let AgentWorkOutcome::Succeeded(settlement) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(settlement.closure().model_calls(), 0);
    assert_eq!(settlement.closure().effects(), 0);
    assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
    assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
    assert_eq!(
        events.last().expect("terminal").kind(),
        AgentWorkEventKind::Terminal
    );
    for pair in events.windows(2) {
        assert_eq!(pair[0].sequence() + 1, pair[1].sequence());
    }
}

#[test]
fn unsuccessful_cleanup_refuses_lost_duplicate_and_refused_native_receipts() {
    let _guard = lock(&SERIAL);
    for fault in [
        Fault::CancellationRefused,
        Fault::CancellationDuplicate,
        Fault::CleanupAuditRefused,
        Fault::CleanupAuditLost,
        Fault::CleanupAuditDuplicate,
        Fault::CleanupDeliveryRefused,
        Fault::CleanupDeliveryLost,
    ] {
        let (outcome, shutdown, calls, _) = run(fault);
        let AgentWorkOutcome::Recovery(recovery) = outcome else {
            panic!("{fault:?}: {outcome:?}")
        };
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Unclean));
        assert_eq!(calls.iter().filter(|call| **call == 4).count(), 1);
        assert_eq!(calls.iter().filter(|call| **call == 5).count(), 1);
        assert!(recovery.state.extraction.is_none());
        if matches!(
            fault,
            Fault::CancellationRefused | Fault::CancellationDuplicate
        ) {
            assert!(recovery.retained_callbacks() > 0);
            assert!(!calls.contains(&6));
        } else {
            assert_eq!(calls.iter().filter(|call| **call == 6).count(), 1);
            assert!(recovery.state.drained.is_some());
            if matches!(
                fault,
                Fault::CleanupDeliveryRefused | Fault::CleanupDeliveryLost
            ) {
                let drained = recovery.state.drained.as_ref().unwrap();
                assert!(drained.proof.is_some());
                assert!(!drained.journal.as_ref().unwrap().audit.is_quiescent());
                assert!(drained.policy.is_some());
                assert!(drained.provider.is_some());
            }
        }
    }
}

#[cfg(feature = "probe-harness")]
fn extraction_stream(fault: ExtractionFault) -> String {
    let source = if fault == ExtractionFault::ForeignSource {
        "@r999"
    } else {
        "@r1"
    };
    let value = if fault == ExtractionFault::WrongKind {
        format!(r#"{{"k":"boolean","value":true,"sources":["{source}"]}}"#)
    } else {
        format!(r#"{{"k":"text","value":"Field","sources":["{source}"]}}"#)
    };
    // Closed ASCII fixture values only; never a production JSON encoder.
    let output = format!(r#"{{"v":1,"schema":1,"fields":[{{"name":"label","value":{value}}}]}}"#)
        .replace('"', "\\\"");
    let events = [
        ("response.created", r#"{"type":"response.created","response":{"id":"resp_2","status":"in_progress","model":"gpt-5.6-luna","service_tier":"default"}}"#.to_owned()),
        ("response.output_item.added", r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg_2","status":"in_progress","role":"assistant"}}"#.to_owned()),
        ("response.content_part.added", r#"{"type":"response.content_part.added","item_id":"msg_2","output_index":0,"content_index":0,"part":{"type":"output_text"}}"#.to_owned()),
        ("response.output_text.delta", format!(r#"{{"type":"response.output_text.delta","item_id":"msg_2","output_index":0,"content_index":0,"delta":"{output}"}}"#)),
        ("response.output_text.done", format!(r#"{{"type":"response.output_text.done","item_id":"msg_2","output_index":0,"content_index":0,"text":"{output}"}}"#)),
        ("response.content_part.done", r#"{"type":"response.content_part.done","item_id":"msg_2","output_index":0,"content_index":0,"part":{"type":"output_text"}}"#.to_owned()),
        ("response.output_item.done", r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"message","id":"msg_2","status":"completed","role":"assistant"}}"#.to_owned()),
        ("response.completed", r#"{"type":"response.completed","response":{"id":"resp_2","status":"completed","model":"gpt-5.6-luna","service_tier":"default","output":[{"type":"message","id":"msg_2","status":"completed","role":"assistant","content":[{"type":"output_text"}]}],"usage":{"input_tokens":17,"output_tokens":3,"total_tokens":20,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}}}"#.to_owned()),
    ];
    events
        .into_iter()
        .map(|(event, body)| format!("event: {event}\ndata: {body}\n\n"))
        .collect::<String>()
        + "data: [DONE]\n\n"
}

#[test]
fn native_audit_cancellation_renderer_and_mailbox_faults_never_claim_success() {
    let _guard = lock(&SERIAL);
    for fault in [
        Fault::ConstructDispatch,
        Fault::ConstructCallback,
        Fault::NavigateDispatch,
        Fault::NavigateTarget,
        Fault::Observe,
        Fault::NativeNonzero,
        Fault::AuditRefused,
        Fault::AuditLost,
        Fault::CancelObservation,
        Fault::RendererObservation,
        Fault::MailboxPressure,
    ] {
        let (outcome, shutdown, calls, _) = run(fault);
        if matches!(fault, Fault::Observe | Fault::CancelObservation) {
            let AgentWorkOutcome::ClosedUnsuccessfully(closed) = outcome else {
                panic!("settled read/refusal must close: {fault:?}: {outcome:?}");
            };
            assert_eq!(closed.policy_settlement().closure().model_calls(), 0);
            assert_eq!(closed.policy_settlement().closure().effects(), 0);
            assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Clean(_)));
            assert_eq!(calls, [1, 2, 3, 4, 5, 6]);
            continue;
        }
        assert!(
            matches!(outcome, AgentWorkOutcome::Recovery(_)),
            "{fault:?}: {outcome:?}"
        );
        assert!(
            matches!(shutdown, AgentBrowserShutdownOutcome::Unclean),
            "{fault:?}"
        );
        if matches!(
            fault,
            Fault::Observe | Fault::CancelObservation | Fault::RendererObservation
        ) {
            assert!(
                calls.contains(&5),
                "{fault:?}: recovery must close the owned page"
            );
        }
    }
}

#[test]
fn lost_callbacks_and_close_refusal_preserve_debt_without_leaving_cleanup_unscheduled() {
    let _guard = lock(&SERIAL);
    for fault in [
        Fault::ObservationLost,
        Fault::CancellationLost,
        Fault::CloseLost,
        Fault::CloseRefused,
        Fault::DeadlineObservation,
        Fault::ShutdownObservation,
    ] {
        let (outcome, shutdown, calls, _) = run(fault);
        let AgentWorkOutcome::Recovery(recovery) = outcome else {
            panic!("lost debt cannot complete");
        };
        assert!(matches!(shutdown, AgentBrowserShutdownOutcome::Unclean));
        assert_eq!(
            calls.iter().filter(|call| **call == 4).count(),
            1,
            "{fault:?}"
        );
        assert_eq!(
            calls.iter().filter(|call| **call == 5).count(),
            1,
            "{fault:?}"
        );
        assert!(
            !calls.contains(&6),
            "unreconciled resources cannot be sealed into a clean proof"
        );
        let native = &recovery.state.native;
        match fault {
            Fault::ObservationLost | Fault::DeadlineObservation | Fault::ShutdownObservation => {
                assert!(native.observation.is_some())
            }
            Fault::CancellationLost => assert!(native.cancellation.is_some()),
            Fault::CloseLost => assert!(native.recovery_close.is_some()),
            Fault::CloseRefused => assert!(native.profile.is_some()),
            _ => unreachable!(),
        }
        if matches!(
            fault,
            Fault::ObservationLost
                | Fault::DeadlineObservation
                | Fault::CancellationLost
                | Fault::ShutdownObservation
        ) {
            assert!(
                native.profile.is_none(),
                "exact close callback releases only the context/profile owner"
            );
        }
        if fault == Fault::DeadlineObservation {
            assert_eq!(recovery.failure(), AgentWorkFailure::Deadline);
        }
        if fault == Fault::ShutdownObservation {
            assert_eq!(recovery.failure(), AgentWorkFailure::Shutdown);
        }
    }
}

#[test]
fn product_backpressure_is_bounded_sticky_and_content_free() {
    let run = ContextRunId::generate();
    let mut events = WorkEvents::new(run).expect("events");
    for _ in 0..MAX_AGENT_WORK_EVENTS {
        events
            .publish(AgentWorkEventKind::Observing)
            .expect("capacity");
    }
    assert_eq!(
        events.publish(AgentWorkEventKind::Verified),
        Err(AgentWorkFailure::Backpressure)
    );
    assert_eq!(events.queue.len(), MAX_AGENT_WORK_EVENTS);
    events.queue.pop_front();
    assert_eq!(
        events.publish(AgentWorkEventKind::Verified),
        Err(AgentWorkFailure::Backpressure)
    );
    assert!(!format!("{:?}", events.queue).contains("work-fixture"));
}
