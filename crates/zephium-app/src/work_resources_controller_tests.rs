//! Real scoped worker + common controller + private original Work owner.
use super::*;
use crate::work_provider_fixture::{fixture_provider_responses, response_stream};
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};
use zephium_agent_controller::*;
use zephium_agent_runtime::*;

struct Clock(AtomicU64);
impl TerraControllerClock for Clock {
    fn now(&self) -> Result<AgentPolicyInstant, TerraControllerClockError> {
        Ok(AgentPolicyInstant::from_millis(
            self.0.load(Ordering::Acquire),
        ))
    }
}
struct Audit(bool);
impl AgentAuditPort for Audit {
    fn append(
        &self,
        delivery: AgentAuditDelivery,
        completion: AgentAuditCompletion,
    ) -> AgentAuditDispatch {
        let proof = delivery.proof();
        if !self.0 {
            completion(proof.settle(AgentAuditDeliveryOutcome::Committed));
        }
        AgentAuditDispatch::Accepted(proof)
    }
}
#[derive(Default)]
struct Native {
    reporters: Arc<Mutex<BTreeMap<ContextId, WorkBrowserResourceHealthReporter>>>,
    gate: Arc<AtomicBool>,
    tasks: Mutex<Vec<std::thread::JoinHandle<()>>>,
    read: Mutex<
        Option<(
            WorkBrowserObservationRequest,
            WorkBrowserObservationCompletionCallback,
        )>,
    >,
    hold_read: AtomicBool,
    reads: AtomicUsize,
    acquisitions: AtomicUsize,
    destructions: AtomicUsize,
    arm_notification: Mutex<Option<Arc<AtomicBool>>>,
    before_publication: Mutex<Option<mpsc::Receiver<()>>>,
}
impl Native {
    fn read_result(
        request: WorkBrowserObservationRequest,
        callback: WorkBrowserObservationCompletionCallback,
    ) {
        let (invocation, completion) = request.into_parts();
        let wire = format!(
            r#"{{"v":1,"i":{},"g":{},"c":"complete","n":[{{"k":1,"r":"paragraph","t":"Fixture result"}}]}}"#,
            invocation.invocation().get(),
            invocation.snapshot_generation().get()
        );
        let snapshot = invocation.decode_result(wire.as_bytes()).unwrap();
        callback(completion.settle(Ok(snapshot)));
    }
    fn join(&self) {
        for task in self.tasks.lock().unwrap().drain(..) {
            task.join().unwrap();
        }
    }
}
impl AgentBrowserPort for Native {
    fn work_resource_lifecycle(
        &self,
        mut request: WorkBrowserResourceRequest,
        callback: WorkBrowserResourceCompletionCallback,
    ) -> WorkBrowserResourceDispatch {
        if self.gate.load(Ordering::Acquire)
            && matches!(
                request.operation(),
                WorkBrowserResourceOperation::Acquire | WorkBrowserResourceOperation::Destroy
            )
        {
            drop(callback);
            return WorkBrowserResourceDispatch::Rejected {
                request: Box::new(request),
                failure: ContextPortFailure::ResourceExhausted,
            };
        }
        let outcome = match request.operation() {
            WorkBrowserResourceOperation::Construct => {
                let reporter = request.take_resource_health_reporter().unwrap();
                assert!(reporter.install(request.resource()));
                self.reporters
                    .lock()
                    .unwrap()
                    .insert(request.resource().identity().context(), reporter);
                WorkBrowserResourceNativeOutcome::Constructed
            }
            WorkBrowserResourceOperation::Acquire => {
                self.acquisitions.fetch_add(1, Ordering::AcqRel);
                WorkBrowserResourceNativeOutcome::Acquired
            }
            WorkBrowserResourceOperation::Destroy => {
                self.destructions.fetch_add(1, Ordering::AcqRel);
                let reporter = self
                    .reporters
                    .lock()
                    .unwrap()
                    .remove(&request.resource().identity().context());
                drop(reporter);
                WorkBrowserResourceNativeOutcome::Destroyed
            }
            WorkBrowserResourceOperation::Revoke => {
                self.gate.store(true, Ordering::Release);
                let gate = self.gate.clone();
                let reporters = self.reporters.clone();
                let context = request.resource().identity().context();
                let arm = self.arm_notification.lock().unwrap().clone();
                let before_publication = self.before_publication.lock().unwrap().take();
                let mut delivery = request.take_lease_delivery_completion().unwrap();
                self.tasks.lock().unwrap().push(std::thread::spawn(move || {
                    let notification = delivery.take_notification().unwrap();
                    callback(request.complete(WorkBrowserResourceNativeOutcome::Revoked {
                        debt: WorkBrowserLeaseNativeDebt::default(),
                        resource_retained: true,
                    }));
                    if let Some(ready) = before_publication {
                        ready.recv_timeout(Duration::from_secs(10)).unwrap();
                    }
                    assert!(delivery.publish_returned());
                    if let Some(arm) = arm {
                        arm.store(true, Ordering::Release);
                    }
                    if !notification.notify() {
                        if let Some(reporter) = reporters.lock().unwrap().get(&context) {
                            reporter.invalidate();
                        }
                    }
                    gate.store(false, Ordering::Release);
                }));
                return WorkBrowserResourceDispatch::Scheduled;
            }
        };
        callback(request.complete(outcome));
        WorkBrowserResourceDispatch::Scheduled
    }
    fn work_resource_observe(
        &self,
        request: WorkBrowserObservationRequest,
        callback: WorkBrowserObservationCompletionCallback,
    ) -> WorkBrowserObservationDispatch {
        self.reads.fetch_add(1, Ordering::AcqRel);
        if self.hold_read.load(Ordering::Acquire) {
            *self.read.lock().unwrap() = Some((request, callback));
        } else {
            Self::read_result(request, callback);
        }
        WorkBrowserObservationDispatch::Scheduled
    }
    fn dispatch(&self, _: ContextNativeRequest) -> ContextDispatch {
        panic!("retained path has no legacy context capability")
    }
    fn invoke_semantic(&self, _: SemanticRuntimeInvocation) -> ContextDispatch {
        panic!("retained path must use original Work request")
    }
    fn execute_semantic_action(
        &self,
        _: SemanticActionNativeRequest,
        _: SemanticActionNativeCompletion,
    ) -> ContextDispatch {
        panic!("no retained action authority")
    }
    fn transfer_cookies(&self, _: ContextCookieTransferRequest) -> ContextDispatch {
        panic!("no retained cookie authority")
    }
    fn audit_resources(&self, _: ContextResourceAuditId) -> ContextDispatch {
        ContextDispatch::Unsupported
    }
    fn capture_semantic_screenshot(
        &self,
        _: SemanticScreenshotNativeRequest,
        _: SemanticScreenshotNativeCompletion,
    ) -> ContextDispatch {
        panic!("no retained screenshot authority")
    }
    fn seal_for_shutdown(&self, _: ContextResourceAuditId) -> ContextShutdownDispatch {
        // This fixture never manufactures a global native shutdown proof.
        ContextShutdownDispatch::SealedWithoutAudit(ContextPortFailure::NativeRefused)
    }
}
fn now() -> AgentPolicyInstant {
    AgentPolicyInstant::from_millis(2)
}
fn setup() -> (
    WorkResourceOwner,
    Arc<Native>,
    WorkBrowserResourceJoin,
    RetainedBrowser,
) {
    setup_with_storage(ContextProfileStorageClass::Ephemeral)
}
fn setup_with_storage(
    storage: ContextProfileStorageClass,
) -> (
    WorkResourceOwner,
    Arc<Native>,
    WorkBrowserResourceJoin,
    RetainedBrowser,
) {
    let native = Arc::new(Native::default());
    let port = native.clone();
    let owner = WorkResourceOwner::new(
        WorkId::generate(),
        ProfileId::generate(),
        Arc::new(|| true),
        Box::new(move |_| Some(port)),
    )
    .unwrap();
    let mut construct = owner
        .construct(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            storage,
            ContextNavigationTarget::parse("https://retained-fixture.invalid/frozen").unwrap(),
            now(),
        )
        .unwrap();
    let Some(LifecycleResult::Event(WorkBrowserResourceEvent::Retained(resource))) =
        construct.poll(now()).unwrap()
    else {
        panic!("original construction")
    };
    let mut acquire = owner
        .acquire(
            &resource,
            ContextRunId::generate(),
            now(),
            AgentPolicyInstant::from_millis(600_002),
        )
        .unwrap();
    let Some(LifecycleResult::Event(WorkBrowserResourceEvent::Acquired(lease))) =
        acquire.poll(now()).unwrap()
    else {
        panic!("original acquisition")
    };
    let browser = owner.retained_browser(lease, now()).unwrap();
    (owner, native, resource, browser)
}
fn input(binding: &WorkBrowserReadBinding, clock: Arc<Clock>) -> AgentWorkRunInput {
    input_with_source(
        binding,
        clock,
        binding.storage(),
        binding.document().clone(),
    )
}
fn input_with_source(
    binding: &WorkBrowserReadBinding,
    clock: Arc<Clock>,
    storage: ContextProfileStorageClass,
    target: ContextNavigationTarget,
) -> AgentWorkRunInput {
    let identity = binding.frame().context().identity();
    let origin = binding.frame().origin().clone();
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).unwrap();
    let budget = AgentRunBudget::try_new(24, 1_000_000, 1_000_000, 1).unwrap();
    let node = AgentPlanNodeId::generate();
    let expires = AgentPolicyInstant::from_millis(600_002);
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
        identity.owner(),
        AgentRunScope::try_new(
            vec![identity.profile()],
            vec![AgentAccountScope::Anonymous],
            vec![origin.clone()],
            SemanticSensitivity::Public,
            effects,
            Vec::new(),
        )
        .unwrap(),
        budget,
        AgentPolicyInstant::from_millis(1),
        expires,
        vec![AgentPlanNodeScope::new(
            node,
            AgentPlanNodeAuthority::try_new(
                vec![identity.profile()],
                vec![AgentAccountScope::Anonymous],
                vec![origin],
                SemanticSensitivity::Public,
                effects,
            )
            .unwrap(),
            budget,
            expires,
        )],
    )
    .unwrap();
    let ids = TerraControllerIds::try_new(
        AgentSupervisorId::new(1).unwrap(),
        AgentSupervisorAttemptId::new(1).unwrap(),
        AgentSupervisorCancellationId::new(1).unwrap(),
        AgentModelCallId::new(1).unwrap(),
        [1, 2, 3, 4].map(|id| AgentAuditEventId::new(id).unwrap()),
        AgentAuditDeliveryId::new(1).unwrap(),
    )
    .unwrap();
    AgentWorkRunInput::try_new(
        manifest,
        AgentPlanLeaseBinding::new(AgentPlanLeaseId::generate(), node),
        AgentWorkContextSpec::try_new(identity, storage, target).unwrap(),
        "Read the current page and extract its label with source evidence.".into(),
        AgentWorkRunSettings::new(
            AgentBrowserModel::Luna,
            ids,
            clock,
            Instant::now() + Duration::from_secs(600),
        ),
    )
    .unwrap()
}

#[test]
fn retained_admission_requires_exact_frozen_target_and_original_storage_class() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for storage in [
        ContextProfileStorageClass::Ephemeral,
        ContextProfileStorageClass::Durable,
    ] {
        let other = if storage == ContextProfileStorageClass::Ephemeral {
            ContextProfileStorageClass::Durable
        } else {
            ContextProfileStorageClass::Ephemeral
        };
        for (url, requested_storage, accepted) in [
            ("https://retained-fixture.invalid/frozen", storage, true),
            ("https://retained-fixture.invalid/other", storage, false),
            (
                "https://retained-fixture.invalid/frozen?other=1",
                storage,
                false,
            ),
            (
                "https://retained-fixture.invalid/frozen#other",
                storage,
                false,
            ),
            ("https://retained-fixture.invalid/frozen", other, false),
        ] {
            let (owner, native, resource, browser) = setup_with_storage(storage);
            let target = ContextNavigationTarget::parse(url).unwrap();
            // Manifest and account can legitimately match this same origin;
            // only the original descriptive row closes the exact-source join.
            assert_eq!(
                &SemanticOrigin::parse(url).unwrap(),
                browser.binding().frame().origin()
            );
            let input = input_with_source(
                browser.binding(),
                Arc::new(Clock(AtomicU64::new(2))),
                requested_storage,
                target,
            );
            let result = AgentWorkRetainedController::try_new(
                input,
                Box::new(browser),
                AgentProviderTransportConfig::STANDARD,
                AgentProviderCredential::try_new(
                    AgentProviderKind::OpenAiResponses,
                    "fixture-not-a-secret".into(),
                )
                .unwrap(),
                Arc::new(Audit(false)),
                Box::new(task()),
            );
            assert_eq!(result.is_ok(), accepted);
            if let Err(error) = &result {
                assert_eq!(*error, AgentWorkFailure::Contract);
            }
            drop(result);
            assert_eq!(native.reads.load(Ordering::Acquire), 0);
            assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
            assert!(native.tasks.lock().unwrap().is_empty());
            assert_eq!(native.destructions.load(Ordering::Acquire), 0);
            let mut destroy = owner.destroy(&resource).unwrap();
            assert!(matches!(
                destroy.poll(now()).unwrap(),
                Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
                    _
                )))
            ));
            owner.seal_resources().unwrap();
            assert!(owner.locally_retired());
        }
    }
}
fn task() -> AgentWorkExtractionTask {
    AgentWorkExtractionTask::try_new(
        vec![SemanticExtractionFieldSchema::try_text("label".into(), true, 64).unwrap()],
        AgentAccountScope::Anonymous,
    )
    .unwrap()
}
fn prepared(
    browser: Box<dyn AgentWorkRetainedBrowser>,
    responses: Vec<String>,
    audit_lost: bool,
) -> (
    AgentWorkRetainedController,
    AgentWorkRetainedHandle,
    AgentRuntimeScopedBinding,
    std::thread::JoinHandle<usize>,
) {
    prepared_with_audit(browser, responses, Arc::new(Audit(audit_lost)))
}
fn prepared_with_audit(
    browser: Box<dyn AgentWorkRetainedBrowser>,
    responses: Vec<String>,
    audit: Arc<dyn AgentAuditPort>,
) -> (
    AgentWorkRetainedController,
    AgentWorkRetainedHandle,
    AgentRuntimeScopedBinding,
    std::thread::JoinHandle<usize>,
) {
    let input = input(browser.binding(), Arc::new(Clock(AtomicU64::new(2))));
    let (transport, server) = fixture_provider_responses(responses);
    let (controller, handle, scope) = AgentWorkRetainedController::try_new_for_probe(
        input,
        browser,
        transport,
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-not-a-secret".into(),
        )
        .unwrap(),
        audit,
        Box::new(task()),
    )
    .unwrap();
    (controller, handle, scope, server)
}

struct GatedAudit {
    dispatched: mpsc::SyncSender<(AgentAuditDeliveryProof, AgentAuditCompletion)>,
    release: Mutex<mpsc::Receiver<()>>,
    calls: AtomicUsize,
}
impl AgentAuditPort for GatedAudit {
    fn append(
        &self,
        delivery: AgentAuditDelivery,
        completion: AgentAuditCompletion,
    ) -> AgentAuditDispatch {
        self.calls.fetch_add(1, Ordering::AcqRel);
        let proof = delivery.proof();
        self.dispatched.send((proof, completion)).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        AgentAuditDispatch::Accepted(proof)
    }
}

#[test]
fn queued_original_audit_is_accounted_after_cancelled_initial_retained_close() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for (cancel, deliver) in [(false, true), (true, true), (true, false)] {
        let (owner, native, resource, browser) = setup();
        let (dispatched, dispatch) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let audit = Arc::new(GatedAudit {
            dispatched,
            release: Mutex::new(released),
            calls: AtomicUsize::new(0),
        });
        let (controller, mut result, scope, server) = prepared_with_audit(
            Box::new(browser),
            vec![response_stream(1), response_stream(2)],
            audit.clone(),
        );
        let (handle, lifecycle) = start(controller, scope);
        // append can only start after the exact session/provider/delivery owners
        // entered WorkDrained and supervisor completion was recorded. Hold the
        // worker here so control necessarily wins ahead of its queued terminal.
        let (proof, completion) = dispatch.recv_timeout(Duration::from_secs(5)).unwrap();
        if cancel {
            handle.stop_and_seal(AgentRuntimeStopReason::Cancelled);
        }
        if deliver {
            completion(proof.settle(AgentAuditDeliveryOutcome::Committed));
        } else {
            drop(completion);
        }
        release.send(()).unwrap();
        let mut outcome = None;
        wait_until(|| {
            while result.take_event().is_some() {}
            outcome = result.take_outcome();
            outcome.is_some()
        });
        if cancel {
            let AgentWorkRetainedOutcome::Recovery(mut recovery) = outcome.unwrap() else {
                panic!("late cancellation must not relabel prior supervisor success");
            };
            let status = recovery.audit_status().unwrap();
            assert!(status.shutdown_sealed());
            assert!(!status.fail_stopped());
            if deliver {
                assert_eq!(status.in_flight(), 0);
                assert_eq!(status.pending(), 0);
                assert_eq!(status.committed(), u64::from(proof.events()));
            } else {
                assert_eq!(status.in_flight(), proof.events());
                assert_eq!(status.pending(), proof.events());
                assert_eq!(status.committed(), 0);
            }
            assert!(matches!(
                lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
                AgentRuntimeScopedDrain::Unproven
            ));
        } else {
            assert!(matches!(
                outcome,
                Some(AgentWorkRetainedOutcome::Accepted { .. })
            ));
            assert!(matches!(
                lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
                AgentRuntimeScopedDrain::Drained(_)
            ));
        }
        native.join();
        assert_eq!(audit.calls.load(Ordering::Acquire), 1);
        assert_eq!(native.reads.load(Ordering::Acquire), 1);
        assert_eq!(native.tasks.lock().unwrap().len(), 0);
        assert_eq!(native.destructions.load(Ordering::Acquire), 0);
        assert_eq!(native.reporters.lock().unwrap().len(), 1);
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(matches!(
            destroy.poll(now()).unwrap(),
            Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
                _
            )))
        ));
        owner.seal_resources().unwrap();
        assert!(owner.locally_retired());
        assert_eq!(server.join().unwrap(), 2);
    }
}
fn start(
    controller: AgentWorkRetainedController,
    scope: AgentRuntimeScopedBinding,
) -> (AgentRuntimeHandle, AgentRuntimeScopedLifecycle) {
    let (handle, _completion, lifecycle) = PendingScopedAgentRuntime::spawn_suspended(
        AgentRuntimeConfig::STANDARD,
        scope,
        Box::new(controller),
    )
    .unwrap()
    .bind()
    .into_parts();
    handle.start_run().unwrap();
    (handle, lifecycle)
}
fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !predicate() {
        assert!(Instant::now() < deadline, "bounded retained fixture");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn actual_common_controller_returns_source_bound_result_before_original_resource_destruction() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    let lease = browser.binding().lease().clone();
    let (controller, mut result, scope, server) = prepared(
        Box::new(browser),
        vec![response_stream(1), response_stream(2)],
        false,
    );
    let (_handle, lifecycle) = start(controller, scope);
    let mut outcome = None;
    wait_until(|| {
        while result.take_event().is_some() {}
        outcome = result.take_outcome();
        outcome.is_some()
    });
    let AgentWorkRetainedOutcome::Accepted {
        settlement,
        extraction,
    } = outcome.unwrap()
    else {
        panic!("actual scoped result not accepted")
    };
    assert_eq!(extraction.stats().source_edges(), 1);
    assert_eq!(settlement.closure().model_calls(), 2);
    assert_eq!(native.reads.load(Ordering::Acquire), 1);
    assert_eq!(native.destructions.load(Ordering::Acquire), 0);
    assert_eq!(native.reporters.lock().unwrap().len(), 1);
    let AgentRuntimeScopedDrain::Drained(drained) =
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2))
    else {
        panic!("original worker did not drain")
    };
    assert_eq!(drained.lease(), &lease);
    assert!(!owner.locally_retired());
    assert!(matches!(
        native.seal_for_shutdown(ContextResourceAuditId::new(1).unwrap()),
        ContextShutdownDispatch::SealedWithoutAudit(_)
    ));
    native.join();
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    owner.reap_absent(&resource).unwrap();
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert_eq!(server.join().unwrap(), 2);
}

#[test]
fn cancelled_read_drains_original_slot_but_lost_read_never_claims_scoped_completion() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for lost in [false, true] {
        let (owner, native, resource, browser) = setup();
        native.hold_read.store(true, Ordering::Release);
        let (controller, mut result, scope, server) =
            prepared(Box::new(browser), Vec::new(), false);
        let (handle, lifecycle) = start(controller, scope);
        wait_until(|| native.read.lock().unwrap().is_some());
        handle.stop_and_seal(AgentRuntimeStopReason::Cancelled);
        let (request, callback) = native.read.lock().unwrap().take().unwrap();
        if lost {
            drop((request, callback));
        } else {
            Native::read_result(request, callback);
        }
        let mut outcome = None;
        wait_until(|| {
            while result.take_event().is_some() {}
            outcome = result.take_outcome();
            outcome.is_some()
        });
        assert!(!matches!(
            outcome,
            Some(AgentWorkRetainedOutcome::Accepted { .. })
        ));
        let drain = lifecycle.drain_until(Instant::now() + Duration::from_secs(2));
        if lost {
            assert!(matches!(drain, AgentRuntimeScopedDrain::Unproven));
        } else {
            assert!(matches!(drain, AgentRuntimeScopedDrain::Drained(_)));
        }
        native.join();
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(matches!(
            destroy.poll(now()).unwrap(),
            Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
                _
            )))
        ));
        owner.seal_resources().unwrap();
        assert_eq!(
            owner.locally_retired(),
            !lost,
            "lost callback debt remains with original Work owner"
        );
        assert_eq!(server.join().unwrap(), 0);
    }
}

#[test]
fn idle_native_health_wakes_actual_worker_and_late_read_stays_with_original_work_owner() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.hold_read.store(true, Ordering::Release);
    let (controller, mut result, scope, server) = prepared(Box::new(browser), Vec::new(), false);
    let (_handle, lifecycle) = start(controller, scope);
    wait_until(|| native.read.lock().unwrap().is_some());
    native
        .reporters
        .lock()
        .unwrap()
        .get(&resource.identity().context())
        .unwrap()
        .invalidate();
    let mut outcome = None;
    wait_until(|| {
        while result.take_event().is_some() {}
        outcome = result.take_outcome();
        outcome.is_some()
    });
    assert!(matches!(
        outcome,
        Some(AgentWorkRetainedOutcome::Recovery(_))
    ));
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Unproven
    ));
    assert!(owner
        .acquire(
            &resource,
            ContextRunId::generate(),
            now(),
            AgentPolicyInstant::from_millis(600_002)
        )
        .is_err());
    assert!(!owner.locally_retired());
    let (request, callback) = native.read.lock().unwrap().take().unwrap();
    Native::read_result(request, callback);
    native.join();
    owner.drain_abandoned(now()).unwrap();
    // Run A's result cannot escape; the application still owns every exact
    // late slot and can destroy the one uncertain resource independently.
    assert!(result.take_outcome().is_none());
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
    assert_eq!(server.join().unwrap(), 0);
}

struct CountWake(AtomicUsize);
impl Wake for CountWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

struct HostileWake(Option<Weak<Notifications>>);
impl Wake for HostileWake {
    fn wake(self: Arc<Self>) {
        if let Some(notifications) = self.0.as_ref().and_then(Weak::upgrade) {
            notifications.publish();
        } else {
            panic!("injected original-listener panic");
        }
    }
}
#[test]
fn panicking_listener_fails_closed_and_reentrant_scalar_publication_coalesces() {
    for reentrant in [false, true] {
        let (owner, native, _resource, mut browser) = setup();
        let wake = HostileWake(reentrant.then(|| Arc::downgrade(&owner.shared.notifications)));
        browser.register_listener(Arc::new(wake).into()).unwrap();
        assert_eq!(browser.listener().unwrap().notify(), reentrant);
        assert_eq!(browser.check_health(now()).is_ok(), reentrant);
        if !reentrant {
            assert!(browser.begin_observation(now()).is_err());
        }
        assert_eq!(native.reads.load(Ordering::Acquire), 0);
    }
}

struct ConcurrentSignalWake {
    calls: AtomicUsize,
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    notifications: Weak<Notifications>,
    panic_after: bool,
}
impl Wake for ConcurrentSignalWake {
    fn wake(self: Arc<Self>) {
        // Genuine concurrent Shared publishers, without an owner lock across
        // user wake code. Only the first invocation is held; a rearmed second
        // publication is allowed to call the thread-safe Waker concurrently.
        assert!(self
            .notifications
            .upgrade()
            .unwrap()
            .actors
            .try_lock()
            .is_ok());
        if self.calls.fetch_add(1, Ordering::AcqRel) == 0 {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(!self.panic_after, "injected concurrent wake panic");
        }
    }
}

#[test]
fn concurrent_shared_publications_coalesce_or_rewake_after_rearm_without_quarantine() {
    for rearm in [false, true] {
        for panic_after in [false, true] {
            let (owner, native, _resource, mut browser) = setup();
            let (entered, entry) = mpsc::sync_channel(1);
            let (release, released) = mpsc::sync_channel(1);
            let wake = Arc::new(ConcurrentSignalWake {
                calls: AtomicUsize::new(0),
                entered,
                release: Mutex::new(released),
                notifications: Arc::downgrade(&owner.shared.notifications),
                panic_after,
            });
            browser.register_listener(wake.clone().into()).unwrap();
            let notifications = owner.shared.notifications.clone();
            let first = std::thread::spawn(move || notifications.publish());
            entry.recv_timeout(Duration::from_secs(5)).unwrap();
            if rearm {
                browser.listener().unwrap().rearm().unwrap();
            }
            let notifications = owner.shared.notifications.clone();
            let second = std::thread::spawn(move || notifications.publish());
            assert!(second.join().unwrap());
            assert_eq!(
                wake.calls.load(Ordering::Acquire),
                if rearm { 2 } else { 1 }
            );
            assert!(!browser.listener().unwrap().failed.load(Ordering::Acquire));
            assert!(!browser.browser.resource.failed.load(Ordering::Acquire));
            release.send(()).unwrap();
            assert!(first.join().unwrap());
            assert_eq!(browser.check_health(now()).is_err(), panic_after);
            assert_eq!(native.reads.load(Ordering::Acquire), 0);
        }
    }
}

#[test]
fn retired_a_listener_and_facade_cannot_poison_or_read_a_separately_acquired_b_lease() {
    let (owner, native, resource, mut a) = setup();
    let (entered, entry) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let wake = Arc::new(ConcurrentSignalWake {
        calls: AtomicUsize::new(0),
        entered,
        release: Mutex::new(released),
        notifications: Arc::downgrade(&owner.shared.notifications),
        panic_after: true,
    });
    a.register_listener(wake.into()).unwrap();
    let notifications = owner.shared.notifications.clone();
    let old_publisher = std::thread::spawn(move || notifications.publish());
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    a.begin_revocation().unwrap();
    let mut delivered = None;
    wait_until(|| {
        delivered = a.poll_revocation(now()).unwrap();
        delivered.is_some()
    });
    native.join();
    let mut acquire = owner
        .acquire(
            &resource,
            ContextRunId::generate(),
            now(),
            AgentPolicyInstant::from_millis(600_002),
        )
        .unwrap();
    let Some(LifecycleResult::Event(WorkBrowserResourceEvent::Acquired(lease))) =
        acquire.poll(now()).unwrap()
    else {
        panic!("primitive B acquisition")
    };
    let mut b = owner.retained_browser(lease, now()).unwrap();
    b.register_listener(Arc::new(CountWake(AtomicUsize::new(0))).into())
        .unwrap();
    assert_ne!(a.binding().lease(), b.binding().lease());
    assert!(a.check_health(now()).is_err());
    assert!(a.begin_observation(now()).is_err());
    assert!(a.listener().unwrap().notify());
    // This generic A publication entered before retirement and returns with a
    // panic after separate primitive B acquisition. It is not the original
    // native delivery notification (which returned and was joined above).
    release.send(()).unwrap();
    assert!(old_publisher.join().unwrap());
    assert!(a.listener().unwrap().failed.load(Ordering::Acquire));
    drop(a);
    b.check_health(now()).unwrap();
    b.begin_observation(now()).unwrap();
    assert!(b.poll_observation(now()).unwrap().is_some());
    b.begin_revocation().unwrap();
    wait_until(|| b.poll_revocation(now()).unwrap().is_some());
    native.join();
    drop(b);
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}
#[test]
fn lease_listener_is_exact_single_use_and_idle_health_wakes_without_replacing_app_sink() {
    for fault in 0..4 {
        let (owner, native, resource, mut browser) = setup();
        let wakes = Arc::new(CountWake(AtomicUsize::new(0)));
        browser.register_listener(wakes.clone().into()).unwrap();
        browser.check_health(now()).unwrap();
        match fault {
            0 => {
                // The stable application pending bit is deliberately still set.
                assert!(owner.shared.notifications.pending.load(Ordering::Acquire));
                native
                    .reporters
                    .lock()
                    .unwrap()
                    .get(&resource.identity().context())
                    .unwrap()
                    .invalidate();
                assert_eq!(wakes.0.load(Ordering::Acquire), 1);
            }
            1 => {
                assert!(browser.register_listener(wakes.clone().into()).is_err());
            }
            2 => {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _lock = owner.shared.notifications.actors.lock().unwrap();
                    panic!("poison exact listener lane");
                }));
            }
            _ => {
                drop(owner);
            }
        }
        assert!(browser.check_health(now()).is_err());
        assert!(browser.begin_observation(now()).is_err());
        assert_eq!(native.reads.load(Ordering::Acquire), 0);
    }
}

struct InspectTask(AgentWorkExtractionTask);
impl AgentWorkTask for InspectTask {
    fn allows_baseline_read(&self) -> bool {
        true
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.0.extraction_schema()
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.0.evaluate(observation)
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        self.0.assess(action)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        self.0.attest_account(context, now)
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.0.accept_extraction(result)
    }
}
#[test]
fn model_selected_read_continues_the_same_session_without_another_native_read() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    let input = input(browser.binding(), Arc::new(Clock(AtomicU64::new(2))));
    let read = response_stream(1)
        .replace("extract", "read")
        .replace(r#",\"schema_id\":1"#, "");
    let extract = response_stream(1)
        .replace("resp_1", "resp_2")
        .replace("fc_1", "fc_2")
        .replace("call_1", "call_2");
    let mapping = response_stream(2)
        .replace("resp_2", "resp_3")
        .replace("msg_2", "msg_3");
    let (transport, server) = fixture_provider_responses(vec![read, extract, mapping]);
    let (controller, mut result, scope) = AgentWorkRetainedController::try_new_for_probe(
        input,
        Box::new(browser),
        transport,
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-not-a-secret".into(),
        )
        .unwrap(),
        Arc::new(Audit(false)),
        Box::new(InspectTask(task())),
    )
    .unwrap();
    let (_handle, lifecycle) = start(controller, scope);
    let mut outcome = None;
    wait_until(|| {
        while result.take_event().is_some() {}
        outcome = result.take_outcome();
        outcome.is_some()
    });
    let Some(AgentWorkRetainedOutcome::Accepted { settlement, .. }) = outcome else {
        panic!("common read/understand loop")
    };
    assert_eq!(settlement.closure().model_calls(), 3);
    assert_eq!(native.reads.load(Ordering::Acquire), 1);
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Drained(_)
    ));
    native.join();
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
    assert_eq!(server.join().unwrap(), 3);
}

#[test]
fn common_mapping_refuses_foreign_source_and_preserves_original_owner_on_audit_loss() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for audit_lost in [false, true] {
        let (owner, native, resource, browser) = setup();
        let mapping = if audit_lost {
            response_stream(2)
        } else {
            response_stream(2).replace("@r1", "@r999")
        };
        let (controller, mut result, scope, server) = prepared(
            Box::new(browser),
            vec![response_stream(1), mapping],
            audit_lost,
        );
        let (handle, lifecycle) = start(controller, scope);
        let mut outcome = None;
        if audit_lost {
            wait_until(|| {
                native.gate.load(Ordering::Acquire) || native.tasks.lock().unwrap().len() == 1
            });
            // Exact original audit callback is intentionally lost, not replaced.
            handle.stop_and_seal(AgentRuntimeStopReason::Cancelled);
        }
        wait_until(|| {
            while result.take_event().is_some() {}
            outcome = result.take_outcome();
            outcome.is_some()
        });
        assert!(!matches!(
            outcome,
            Some(AgentWorkRetainedOutcome::Accepted { .. })
        ));
        let drain = lifecycle.drain_until(Instant::now() + Duration::from_secs(2));
        if audit_lost {
            assert!(matches!(drain, AgentRuntimeScopedDrain::Unproven));
        } else {
            assert!(matches!(drain, AgentRuntimeScopedDrain::Drained(_)));
        }
        native.join();
        assert_eq!(native.destructions.load(Ordering::Acquire), 0);
        assert_eq!(native.reporters.lock().unwrap().len(), 1);
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(matches!(
            destroy.poll(now()).unwrap(),
            Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
                _
            )))
        ));
        owner.seal_resources().unwrap();
        assert!(owner.locally_retired());
        assert_eq!(server.join().unwrap(), 2);
    }
}

struct HeldWake {
    original: Waker,
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    armed: Arc<AtomicBool>,
    notifying: Arc<AtomicBool>,
}
impl Wake for HeldWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if self.armed.swap(false, Ordering::AcqRel) {
            self.notifying.store(true, Ordering::Release);
            self.original.wake_by_ref();
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        } else {
            self.original.wake_by_ref();
        }
    }
}
struct HeldBrowser {
    browser: RetainedBrowser,
    entered: Option<mpsc::SyncSender<()>>,
    release: Option<mpsc::Receiver<()>>,
    armed: Arc<AtomicBool>,
    rearmed: Option<mpsc::SyncSender<()>>,
    notifying: Arc<AtomicBool>,
}
impl AgentWorkRetainedBrowser for HeldBrowser {
    fn binding(&self) -> &WorkBrowserReadBinding {
        self.browser.binding()
    }
    fn register_listener(&mut self, original: Waker) -> Result<(), AgentWorkFailure> {
        self.browser.register_listener(
            Arc::new(HeldWake {
                original,
                entered: self.entered.take().unwrap(),
                release: Mutex::new(self.release.take().unwrap()),
                armed: self.armed.clone(),
                notifying: self.notifying.clone(),
            })
            .into(),
        )
    }
    fn check_health(&self, now: AgentPolicyInstant) -> Result<(), AgentWorkFailure> {
        self.browser.check_health(now)
    }
    fn begin_observation(&mut self, now: AgentPolicyInstant) -> Result<(), AgentWorkFailure> {
        self.browser.begin_observation(now)
    }
    fn poll_observation(
        &mut self,
        now: AgentPolicyInstant,
    ) -> Result<Option<SemanticObservation>, AgentWorkFailure> {
        self.browser.poll_observation(now)
    }
    fn begin_revocation(&mut self) -> Result<(), AgentWorkFailure> {
        // Hold the original native notification, not the earlier terminal-slot
        // publication. The fixture arms just before that exact notifier call.
        self.browser.begin_revocation()
    }
    fn poll_revocation(
        &mut self,
        now: AgentPolicyInstant,
    ) -> Result<Option<WorkBrowserLeaseDeliveryProof>, AgentWorkFailure> {
        if !self.notifying.load(Ordering::Acquire) {
            if self.rearmed.is_some() {
                assert!(self.browser.poll_revocation(now)?.is_none());
                let ended = self
                    .browser
                    .revoke
                    .as_ref()
                    .unwrap()
                    .slot
                    .lock()
                    .unwrap()
                    .ended
                    .is_some();
                if ended {
                    self.rearmed.take().unwrap().send(()).unwrap();
                }
            } else {
                self.browser.listener().unwrap().rearm().unwrap();
            }
            return Ok(None);
        }
        self.browser.poll_revocation(now)
    }
}

#[test]
fn scoped_worker_drain_is_not_held_native_notification_or_successor_or_destroy_permission() {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    let (entered, entry) = mpsc::sync_channel(1);
    let (release, waiting) = mpsc::sync_channel(1);
    let armed = Arc::new(AtomicBool::new(false));
    let (rearmed, ready) = mpsc::sync_channel(1);
    *native.arm_notification.lock().unwrap() = Some(armed.clone());
    *native.before_publication.lock().unwrap() = Some(ready);
    let browser = HeldBrowser {
        browser,
        entered: Some(entered),
        release: Some(waiting),
        armed,
        rearmed: Some(rearmed),
        notifying: Arc::new(AtomicBool::new(false)),
    };
    let (controller, mut result, scope, server) = prepared(
        Box::new(browser),
        vec![response_stream(1), response_stream(2)],
        false,
    );
    let (_handle, lifecycle) = start(controller, scope);
    entry.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut outcome = None;
    wait_until(|| {
        while result.take_event().is_some() {}
        outcome = result.take_outcome();
        outcome.is_some()
    });
    assert!(matches!(
        outcome,
        Some(AgentWorkRetainedOutcome::Accepted { .. })
    ));
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Drained(_)
    ));
    assert!(native.gate.load(Ordering::Acquire));
    assert!(!owner.locally_retired());
    assert_eq!(native.destructions.load(Ordering::Acquire), 0);
    // Primitive B acquisition is refused by the original native reservation;
    // no public product/successor admission exists in this slice.
    let mut attempted = owner
        .acquire(
            &resource,
            ContextRunId::generate(),
            now(),
            AgentPolicyInstant::from_millis(600_002),
        )
        .unwrap();
    assert!(matches!(
        attempted.poll(now()).unwrap(),
        Some(LifecycleResult::Event(
            WorkBrowserResourceEvent::AdmissionRefused { .. }
        ))
    ));
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(
            WorkBrowserResourceEvent::AdmissionRefused { .. }
        ))
    ));
    assert_eq!(native.destructions.load(Ordering::Acquire), 0);
    release.send(()).unwrap();
    native.join();
    // The refused destruction is single-shot; this schedule proves refusal,
    // never invents successful cleanup after a terminal failed destroy attempt.
    assert!(!owner.locally_retired());
    assert_eq!(server.join().unwrap(), 2);
}
