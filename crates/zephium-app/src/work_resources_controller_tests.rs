//! Real scoped worker + common controller + private original Work owner.
use super::*;
use crate::work_provider_fixture::{fixture_provider_responses, response_stream};
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};
use zephium_agent_controller::*;
use zephium_agent_runtime::*;

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "work_resources_durable_tests.rs"]
mod durable_tests;

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "work_resources_application_tests.rs"]
mod application_tests;

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "work_resources_product_tests.rs"]
mod product_tests;

#[path = "work_resources_navigation_tests.rs"]
mod navigation_tests;

#[path = "work_resources_action_tests.rs"]
mod action_tests;

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
    hold_construct: AtomicBool,
    construction: Mutex<
        Option<(
            WorkBrowserResourceRequest,
            WorkBrowserResourceCompletionCallback,
        )>,
    >,
    resource_sink: Mutex<Option<NativeSink>>,
    allow_global_shutdown: AtomicBool,
    global_sealed: AtomicBool,
    global_audits: AtomicUsize,
    hold_global_audit: AtomicBool,
    pending_global_audit: Mutex<Option<ContextNativeEvent>>,
    global_queued_debt: AtomicU8,
    wrong_global_audit_kind: AtomicBool,
    wrong_global_audit_identity: AtomicBool,
    synchronous_global_seal: AtomicBool,
    reject_global_audit: AtomicBool,
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
    hold_expansion: AtomicBool,
    region_root: AtomicBool,
    dense_expansion: AtomicBool,
    reject_expansion: AtomicBool,
    not_ready: AtomicBool,
    reads: AtomicUsize,
    acquisitions: AtomicUsize,
    destructions: AtomicUsize,
    arm_notification: Mutex<Option<Arc<AtomicBool>>>,
    before_publication: Mutex<Option<mpsc::Receiver<()>>>,
    final_document: Mutex<Option<ContextNavigationTarget>>,
    discovery: AtomicBool,
    form_actions: AtomicBool,
    form_applied: AtomicBool,
    actions: AtomicUsize,
    hold_action: AtomicBool,
    reject_action: AtomicBool,
    before_action_return: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    action: Mutex<
        Option<(
            WorkBrowserActionRequest,
            WorkBrowserActionCompletionCallback,
        )>,
    >,
    navigation_count: AtomicUsize,
    hold_navigation: AtomicBool,
    reject_navigation: AtomicBool,
    after_navigation: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    navigation: Mutex<
        Option<(
            WorkBrowserNavigationRequest,
            WorkBrowserNavigationCompletionCallback,
        )>,
    >,
}
impl Native {
    fn release_construction(&self) {
        self.hold_construct.store(false, Ordering::Release);
        let (request, callback) = self.construction.lock().unwrap().take().unwrap();
        assert!(matches!(
            self.work_resource_lifecycle(request, callback),
            WorkBrowserResourceDispatch::Scheduled
        ));
    }
    fn global_audit(&self, audit: ContextResourceAuditId, shutdown: bool) {
        let audit = if self.wrong_global_audit_identity.load(Ordering::Acquire) {
            ContextResourceAuditId::new(audit.get() + 1).unwrap()
        } else {
            audit
        };
        let live = u8::try_from(self.reporters.lock().unwrap().len()).unwrap();
        let snapshot = ContextNativeResourceSnapshot::try_new(ContextNativeResourceCounts {
            known_bindings: live,
            resident_views: live,
            owned_reservations: live,
            borrowed_leases: 0,
            visible_surfaces: 0,
            suspended_views: 0,
            pending_operations: u8::from(self.read.lock().unwrap().is_some()),
            pending_captures: 0,
            queued_tasks: self.global_queued_debt.load(Ordering::Acquire),
        })
        .unwrap();
        self.global_audits.fetch_add(1, Ordering::AcqRel);
        let event = if shutdown && !self.wrong_global_audit_kind.load(Ordering::Acquire) {
            ContextNativeEvent::ShutdownAuditSettled(ContextShutdownAuditSettlement::new(
                audit,
                Ok(snapshot),
            ))
        } else {
            ContextNativeEvent::ResourceAuditSettled(ContextResourceAuditSettlement::new(
                audit,
                Ok(snapshot),
            ))
        };
        if self.hold_global_audit.load(Ordering::Acquire) {
            assert!(self
                .pending_global_audit
                .lock()
                .unwrap()
                .replace(event)
                .is_none());
        } else {
            self.resource_sink.lock().unwrap().as_ref().unwrap()(event);
        }
    }

    fn release_global_audit(&self) {
        let event = self.pending_global_audit.lock().unwrap().take().unwrap();
        self.resource_sink.lock().unwrap().as_ref().unwrap()(event);
    }
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
        if request.operation() == WorkBrowserResourceOperation::Construct
            && self.hold_construct.load(Ordering::Acquire)
        {
            assert!(self
                .construction
                .lock()
                .unwrap()
                .replace((request, callback))
                .is_none());
            return WorkBrowserResourceDispatch::Scheduled;
        }
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
        callback(
            if outcome == WorkBrowserResourceNativeOutcome::Constructed {
                match self.final_document.lock().unwrap().take() {
                    Some(document) => request.complete_document(document),
                    None => request.complete(outcome),
                }
            } else {
                request.complete(outcome)
            },
        );
        WorkBrowserResourceDispatch::Scheduled
    }
    fn work_resource_observe(
        &self,
        request: WorkBrowserObservationRequest,
        callback: WorkBrowserObservationCompletionCallback,
    ) -> WorkBrowserObservationDispatch {
        self.reads.fetch_add(1, Ordering::AcqRel);
        let expansion = request.invocation().scope() != SemanticRuntimeScopeClass::Initial;
        if expansion && self.reject_expansion.load(Ordering::Acquire) {
            return WorkBrowserObservationDispatch::Rejected {
                request: Box::new(request),
                failure: ContextPortFailure::NativeRefused,
            };
        }
        if self.not_ready.swap(false, Ordering::AcqRel) {
            let (_, completion) = request.into_parts();
            callback(completion.settle(Err(SemanticRuntimePortFailure::NotReady)));
            return WorkBrowserObservationDispatch::Scheduled;
        }
        if self.hold_read.load(Ordering::Acquire)
            || (expansion && self.hold_expansion.load(Ordering::Acquire))
        {
            *self.read.lock().unwrap() = Some((request, callback));
        } else {
            if self.form_actions.load(Ordering::Acquire) {
                action_tests::read_result(self, request, callback);
            } else if self.discovery.load(Ordering::Acquire) {
                navigation_tests::read_result(self, request, callback);
            } else {
                Self::read_result(request, callback);
            }
        }
        WorkBrowserObservationDispatch::Scheduled
    }
    fn work_resource_act(
        &self,
        request: WorkBrowserActionRequest,
        callback: WorkBrowserActionCompletionCallback,
    ) -> WorkBrowserActionDispatch {
        assert!(self.form_actions.load(Ordering::Acquire));
        self.actions.fetch_add(1, Ordering::AcqRel);
        if self.reject_action.load(Ordering::Acquire) {
            drop(callback);
            return WorkBrowserActionDispatch::Rejected {
                request: Box::new(request),
                failure: ContextPortFailure::NativeRefused,
            };
        }
        if self.hold_action.load(Ordering::Acquire) {
            assert!(self
                .action
                .lock()
                .unwrap()
                .replace((request, callback))
                .is_none());
        } else {
            action_tests::action_result(self, request, callback);
        }
        WorkBrowserActionDispatch::Scheduled
    }
    fn work_resource_navigate(
        &self,
        request: WorkBrowserNavigationRequest,
        callback: WorkBrowserNavigationCompletionCallback,
    ) -> WorkBrowserNavigationDispatch {
        assert!(
            self.discovery.load(Ordering::Acquire),
            "only explicit discovery fixtures navigate"
        );
        self.navigation_count.fetch_add(1, Ordering::AcqRel);
        if self.reject_navigation.load(Ordering::Acquire) {
            drop(callback);
            if let Some(action) = self.after_navigation.lock().unwrap().take() {
                action();
            }
            return WorkBrowserNavigationDispatch::Rejected {
                request: Box::new(request),
                failure: ContextPortFailure::NativeRefused,
            };
        }
        if self.hold_navigation.load(Ordering::Acquire) {
            assert!(self
                .navigation
                .lock()
                .unwrap()
                .replace((request, callback))
                .is_none());
        } else {
            let target = request.navigation().target().clone();
            callback(request.into_completion().settle(Ok(target)));
        }
        if let Some(action) = self.after_navigation.lock().unwrap().take() {
            action();
        }
        WorkBrowserNavigationDispatch::Scheduled
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
    fn audit_resources(&self, audit: ContextResourceAuditId) -> ContextDispatch {
        if !self.allow_global_shutdown.load(Ordering::Acquire)
            || !self.global_sealed.load(Ordering::Acquire)
            || self.reject_global_audit.load(Ordering::Acquire)
        {
            return ContextDispatch::Unsupported;
        }
        self.global_audit(audit, false);
        ContextDispatch::Scheduled
    }
    fn capture_semantic_screenshot(
        &self,
        _: SemanticScreenshotNativeRequest,
        _: SemanticScreenshotNativeCompletion,
    ) -> ContextDispatch {
        panic!("no retained screenshot authority")
    }
    fn seal_for_shutdown(&self, audit: ContextResourceAuditId) -> ContextShutdownDispatch {
        if self.allow_global_shutdown.load(Ordering::Acquire)
            && !self.global_sealed.swap(true, Ordering::AcqRel)
        {
            if self.synchronous_global_seal.load(Ordering::Acquire) {
                return ContextShutdownDispatch::SealedWithoutAudit(
                    ContextPortFailure::NativeRefused,
                );
            }
            self.global_audit(audit, true);
            return ContextShutdownDispatch::AuditScheduled;
        }
        // Ordinary scoped fixtures do not opt into global native shutdown.
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
    setup_with_document_policy(
        storage,
        zephium_agentic::WorkBrowserDocumentPolicy::Exact,
        None,
    )
}
fn setup_with_document_policy(
    storage: ContextProfileStorageClass,
    policy: zephium_agentic::WorkBrowserDocumentPolicy,
    effective: Option<ContextNavigationTarget>,
) -> (
    WorkResourceOwner,
    Arc<Native>,
    WorkBrowserResourceJoin,
    RetainedBrowser,
) {
    let (owner, native, resource) = construct_fixture(storage, policy, effective);
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
fn construct_fixture(
    storage: ContextProfileStorageClass,
    policy: zephium_agentic::WorkBrowserDocumentPolicy,
    effective: Option<ContextNavigationTarget>,
) -> (WorkResourceOwner, Arc<Native>, WorkBrowserResourceJoin) {
    construct_fixture_with_wake(storage, policy, effective, Arc::new(|| true))
}
fn construct_fixture_with_wake(
    storage: ContextProfileStorageClass,
    policy: zephium_agentic::WorkBrowserDocumentPolicy,
    effective: Option<ContextNavigationTarget>,
    wake: WakeApplication,
) -> (WorkResourceOwner, Arc<Native>, WorkBrowserResourceJoin) {
    let native = Arc::new(Native::default());
    *native.final_document.lock().unwrap() = effective;
    let port = native.clone();
    let owner = WorkResourceOwner::new(
        WorkId::generate(),
        ProfileId::generate(),
        wake,
        Box::new(move |sink| {
            *port.resource_sink.lock().unwrap() = Some(sink);
            Some(port)
        }),
    )
    .unwrap();
    let mut construct = owner
        .construct_with_policy(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            storage,
            ContextNavigationTarget::parse("https://retained-fixture.invalid/frozen").unwrap(),
            policy,
            now(),
        )
        .unwrap();
    let Some(LifecycleResult::Event(WorkBrowserResourceEvent::Retained(resource))) =
        construct.poll(now()).unwrap()
    else {
        panic!("original construction")
    };
    (owner, native, resource)
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
    input_with_budget(
        binding,
        clock,
        storage,
        target,
        AgentRunBudget::try_new(24, 1_000_000, 1_000_000, 1).unwrap(),
    )
}
fn input_with_budget(
    binding: &WorkBrowserReadBinding,
    clock: Arc<Clock>,
    storage: ContextProfileStorageClass,
    target: ContextNavigationTarget,
    budget: AgentRunBudget,
) -> AgentWorkRunInput {
    let identity = binding.frame().context().identity();
    let origin = binding.frame().origin().clone();
    input_for_context(identity, origin, clock, storage, target, budget)
}
fn input_for_context(
    identity: ContextIdentity,
    origin: SemanticOrigin,
    clock: Arc<Clock>,
    storage: ContextProfileStorageClass,
    target: ContextNavigationTarget,
    budget: AgentRunBudget,
) -> AgentWorkRunInput {
    input_for_context_until(
        identity,
        origin,
        clock,
        storage,
        target,
        budget,
        Instant::now() + Duration::from_secs(600),
    )
}
fn input_for_context_until(
    identity: ContextIdentity,
    origin: SemanticOrigin,
    clock: Arc<Clock>,
    storage: ContextProfileStorageClass,
    target: ContextNavigationTarget,
    budget: AgentRunBudget,
    deadline: Instant,
) -> AgentWorkRunInput {
    input_for_context_authority(
        identity,
        origin,
        clock,
        storage,
        target,
        budget,
        (deadline, None),
    )
}
fn input_for_context_authority(
    identity: ContextIdentity,
    origin: SemanticOrigin,
    clock: Arc<dyn TerraControllerClock>,
    storage: ContextProfileStorageClass,
    target: ContextNavigationTarget,
    budget: AgentRunBudget,
    (deadline, discovery): (Instant, Option<AgentNavigationDiscovery>),
) -> AgentWorkRunInput {
    input_for_context_authority_with_document_policy(
        identity,
        origin,
        clock,
        storage,
        target,
        budget,
        (deadline, discovery),
        WorkBrowserDocumentPolicy::Exact,
    )
}
#[allow(clippy::too_many_arguments)]
fn input_for_context_authority_with_document_policy(
    identity: ContextIdentity,
    origin: SemanticOrigin,
    clock: Arc<dyn TerraControllerClock>,
    storage: ContextProfileStorageClass,
    target: ContextNavigationTarget,
    budget: AgentRunBudget,
    (deadline, discovery): (Instant, Option<AgentNavigationDiscovery>),
    document_policy: WorkBrowserDocumentPolicy,
) -> AgentWorkRunInput {
    input_for_context_effects(
        identity,
        origin,
        clock,
        storage,
        target,
        budget,
        (deadline, discovery),
        document_policy,
        AgentEffectScope::try_new(&[SemanticEffectClass::Read]).unwrap(),
    )
}
#[allow(clippy::too_many_arguments)]
fn input_for_context_effects(
    identity: ContextIdentity,
    origin: SemanticOrigin,
    clock: Arc<dyn TerraControllerClock>,
    storage: ContextProfileStorageClass,
    target: ContextNavigationTarget,
    budget: AgentRunBudget,
    (deadline, discovery): (Instant, Option<AgentNavigationDiscovery>),
    document_policy: WorkBrowserDocumentPolicy,
    effects: AgentEffectScope,
) -> AgentWorkRunInput {
    let node = AgentPlanNodeId::generate();
    let expires = AgentPolicyInstant::from_millis(600_002);
    let authority = AgentPlanNodeAuthority::try_new(
        vec![identity.profile()],
        vec![AgentAccountScope::Anonymous],
        vec![origin.clone()],
        SemanticSensitivity::Public,
        effects,
    )
    .unwrap();
    let authority = match discovery {
        Some(scope) => authority.with_navigation_discovery(scope).unwrap(),
        None => authority,
    };
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
        vec![AgentPlanNodeScope::new(node, authority, budget, expires)],
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
        AgentWorkContextSpec::try_new_with_document_policy(
            identity,
            storage,
            target,
            document_policy,
        )
        .unwrap(),
        "Read the current page and extract its label with source evidence.".into(),
        AgentWorkRunSettings::new(AgentBrowserModel::Luna, ids, clock, deadline),
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
#[test]
fn startup_finalized_binding_reaches_common_controller_without_rebasing_original_request() {
    let requested = "https://retained-fixture.invalid/frozen";
    let finalized = "https://retained-fixture.invalid/frozen?opaque=one";
    for (row_policy, effective_url, input_url, input_policy, invalidate, accepted) in [
        (
            WorkBrowserDocumentPolicy::InitialQueryFinalization,
            Some(finalized),
            requested,
            WorkBrowserDocumentPolicy::InitialQueryFinalization,
            false,
            true,
        ),
        (
            WorkBrowserDocumentPolicy::InitialQueryFinalization,
            Some("https://retained-fixture.invalid/frozen?opaque=two"),
            requested,
            WorkBrowserDocumentPolicy::InitialQueryFinalization,
            false,
            true,
        ),
        (
            WorkBrowserDocumentPolicy::InitialQueryFinalization,
            Some(finalized),
            finalized,
            WorkBrowserDocumentPolicy::Exact,
            false,
            false,
        ),
        (
            WorkBrowserDocumentPolicy::InitialQueryFinalization,
            Some(finalized),
            requested,
            WorkBrowserDocumentPolicy::Exact,
            false,
            false,
        ),
        (
            WorkBrowserDocumentPolicy::Exact,
            None,
            requested,
            WorkBrowserDocumentPolicy::InitialQueryFinalization,
            false,
            false,
        ),
        (
            WorkBrowserDocumentPolicy::InitialQueryFinalization,
            Some(finalized),
            requested,
            WorkBrowserDocumentPolicy::InitialQueryFinalization,
            true,
            false,
        ),
    ] {
        let (owner, native, resource, browser) = setup_with_document_policy(
            ContextProfileStorageClass::Ephemeral,
            row_policy,
            effective_url.map(|url| ContextNavigationTarget::parse(url).unwrap()),
        );
        assert_eq!(
            browser.binding().requested_document().as_url().as_str(),
            requested
        );
        assert_eq!(
            browser.binding().document().as_url().as_str(),
            effective_url.unwrap_or(requested)
        );
        let input = input_for_context_authority_with_document_policy(
            browser.binding().frame().context().identity(),
            browser.binding().frame().origin().clone(),
            Arc::new(Clock(AtomicU64::new(2))),
            ContextProfileStorageClass::Ephemeral,
            ContextNavigationTarget::parse(input_url).unwrap(),
            AgentRunBudget::try_new(24, 1_000_000, 1_000_000, 1).unwrap(),
            (Instant::now() + Duration::from_secs(600), None),
            input_policy,
        );
        if invalidate {
            native
                .reporters
                .lock()
                .unwrap()
                .get(&resource.identity().context())
                .unwrap()
                .invalidate();
        }
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
        drop(result);
        assert_eq!(native.reads.load(Ordering::Acquire), 0);
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
fn snapshot_probe_holds_original_observation_until_release_without_recapture() {
    use super::snapshot_probe::{SnapshotRelease, SnapshotReleaseBrowser};
    for result in [Some(true), Some(false), None] {
        let (owner, native, resource, browser) = setup();
        let (tx, rx) = mpsc::sync_channel(1);
        let release = SnapshotRelease::new(
            resource.clone(),
            Box::new(move |callback| {
                tx.send(callback).unwrap();
                true
            }),
        );
        let mut browser = SnapshotReleaseBrowser::new(browser, release.clone()).unwrap();
        browser
            .register_listener(Arc::new(CountWake(AtomicUsize::new(0))).into())
            .unwrap();
        browser.begin_observation(now()).unwrap();
        assert!(browser.poll_observation(now()).unwrap().is_none());
        assert_eq!(native.reads.load(Ordering::Acquire), 1);
        assert!(!release.returned().unwrap());
        let callback = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        if let Some(value) = result {
            callback(value);
        } else {
            drop(callback);
        }
        match result {
            Some(true) => {
                let observation = browser.poll_observation(now()).unwrap().unwrap();
                assert_eq!(observation.request().id().get(), 1);
                assert_eq!(
                    observation.request().context(),
                    browser.binding().frame().context()
                );
                assert!(release.returned().unwrap());
            }
            _ => assert!(browser.poll_observation(now()).is_err()),
        }
        assert!(browser.begin_observation(now()).is_err());
        assert_eq!(native.reads.load(Ordering::Acquire), 1);
        drop(browser);
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(matches!(
            destroy.poll(now()).unwrap(),
            Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
                _
            )))
        ));
    }
}

#[test]
fn snapshot_probe_release_is_resource_bound_and_owner_survives_actor_loss() {
    use super::snapshot_probe::{SnapshotRelease, SnapshotReleaseBrowser};
    let (owner, _, resource, browser) = setup();
    let (_, _, wrong_resource, wrong_browser) = setup();
    let (tx, rx) = mpsc::sync_channel(1);
    let release = SnapshotRelease::new(
        resource.clone(),
        Box::new(move |callback| {
            tx.send(callback).unwrap();
            true
        }),
    );
    assert!(SnapshotReleaseBrowser::from_capture(
        wrong_browser,
        resource.clone(),
        super::super::probe::RetainedProbeCapture::OneShot(Box::new(|_| panic!(
            "foreign resource cannot retire"
        ))),
    )
    .is_err());
    assert_ne!(resource, wrong_resource);
    let mut browser = SnapshotReleaseBrowser::new(browser, release.clone()).unwrap();
    browser
        .register_listener(Arc::new(CountWake(AtomicUsize::new(0))).into())
        .unwrap();
    browser.begin_observation(now()).unwrap();
    assert!(browser.poll_observation(now()).unwrap().is_none());
    let callback = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(browser);
    assert!(!release.returned().unwrap());
    callback(true);
    assert!(release.returned().unwrap());
    assert!(!owner.locally_retired());
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
}

#[test]
fn snapshot_probe_common_worker_cannot_start_model_until_native_release() {
    use super::super::probe::RetainedProbeCapture;
    use super::snapshot_probe::SnapshotReleaseBrowser;
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for first_not_ready in [false, true] {
        let (owner, native, resource, browser) = setup();
        native.not_ready.store(first_not_ready, Ordering::Release);
        let (tx, rx) = mpsc::sync_channel(1);
        let (browser, release) = SnapshotReleaseBrowser::from_capture(
            browser,
            resource.clone(),
            RetainedProbeCapture::BoundedReadiness(Box::new(move |callback| {
                tx.send(callback).unwrap();
                true
            })),
        )
        .unwrap();
        assert!(browser.allows_readiness_retry());
        let (controller, mut result, scope, server) = prepared(
            Box::new(browser),
            vec![response_stream(1), response_stream(2)],
            false,
        );
        let (_handle, lifecycle) = start(controller, scope);
        let callback = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        while let Some(event) = result.take_event() {
            assert!(!matches!(
                event.kind(),
                AgentWorkEventKind::ModelActive
                    | AgentWorkEventKind::ModelSettled { .. }
                    | AgentWorkEventKind::ToolProposed(_)
            ));
        }
        assert!(result.take_outcome().is_none());
        assert_eq!(
            native.reads.load(Ordering::Acquire),
            1 + usize::from(first_not_ready)
        );
        callback(true);
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
        assert!(release.returned().unwrap());
        assert!(matches!(
            lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
            AgentRuntimeScopedDrain::Drained(_)
        ));
        native.join();
        assert_eq!(native.destructions.load(Ordering::Acquire), 0);
        assert!(!owner.locally_retired());
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
        assert_eq!(server.join().unwrap(), 2);
    }
}

#[test]
fn snapshot_probe_not_ready_consumes_one_dispatch_and_drains_without_provider() {
    use super::super::probe::RetainedProbeCapture;
    use super::snapshot_probe::SnapshotReleaseBrowser;
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    native.not_ready.store(true, Ordering::Release);
    let (browser, release) = SnapshotReleaseBrowser::from_capture(
        browser,
        resource.clone(),
        RetainedProbeCapture::OneShot(Box::new(|_| panic!("no successful snapshot to retire"))),
    )
    .unwrap();
    assert!(!browser.allows_readiness_retry());
    let (controller, mut result, scope, server) = prepared(Box::new(browser), Vec::new(), false);
    let (_handle, lifecycle) = start(controller, scope);
    let mut outcome = None;
    wait_until(|| {
        while let Some(event) = result.take_event() {
            assert!(!matches!(
                event.kind(),
                AgentWorkEventKind::ModelActive | AgentWorkEventKind::ModelSettled { .. }
            ));
        }
        outcome = result.take_outcome();
        outcome.is_some()
    });
    let Some(AgentWorkRetainedOutcome::ClosedUnsuccessfully(closed)) = outcome else {
        panic!("NotReady must close before provider");
    };
    assert_eq!(
        closed.failure(),
        AgentWorkFailure::Observation(SemanticRuntimePortFailure::NotReady)
    );
    assert_eq!(closed.policy_settlement().closure().model_calls(), 0);
    assert_eq!(native.reads.load(Ordering::Acquire), 1);
    assert!(!release.returned().unwrap());
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Drained(_)
    ));
    native.join();
    let reporter = native
        .reporters
        .lock()
        .unwrap()
        .remove(&resource.identity().context())
        .unwrap();
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    assert_eq!(owner.reap_absent(&resource), Err(Refusal::Busy));
    drop(reporter);
    owner.reap_absent(&resource).unwrap();
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
    assert!(owner.shared.global_current());
    assert_eq!(server.join().unwrap(), 0);
}

#[test]
fn pre_provider_budget_refusal_drains_worker_and_preserves_late_original_reporter() {
    use super::snapshot_probe::{SnapshotRelease, SnapshotReleaseBrowser};
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (owner, native, resource, browser) = setup();
    let input = input_with_budget(
        browser.binding(),
        Arc::new(Clock(AtomicU64::new(2))),
        browser.binding().storage(),
        browser.binding().document().clone(),
        AgentRunBudget::try_new(8, 100_000, 50_000, 1).unwrap(),
    );
    let (tx, rx) = mpsc::sync_channel(1);
    let release = SnapshotRelease::new(
        resource.clone(),
        Box::new(move |callback| {
            tx.send(callback).unwrap();
            true
        }),
    );
    let browser = SnapshotReleaseBrowser::new(browser, release.clone()).unwrap();
    let (transport, server) = fixture_provider_responses(Vec::new());
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
        Box::new(task()),
    )
    .unwrap();
    let (_handle, lifecycle) = start(controller, scope);
    rx.recv_timeout(Duration::from_secs(5)).unwrap()(true);
    let mut outcome = None;
    wait_until(|| {
        while let Some(event) = result.take_event() {
            assert!(!matches!(
                event.kind(),
                AgentWorkEventKind::ModelActive | AgentWorkEventKind::ModelSettled { .. }
            ));
        }
        outcome = result.take_outcome();
        outcome.is_some()
    });
    let Some(AgentWorkRetainedOutcome::ClosedUnsuccessfully(closed)) = outcome else {
        panic!("budget must fail before provider dispatch");
    };
    assert_eq!(
        closed.failure(),
        AgentWorkFailure::Browser(AgentBrowserProviderError::Authority)
    );
    assert_eq!(closed.policy_settlement().closure().model_calls(), 0);
    assert!(release.returned().unwrap());
    assert!(matches!(
        lifecycle.drain_until(Instant::now() + Duration::from_secs(2)),
        AgentRuntimeScopedDrain::Drained(_)
    ));
    native.join();
    let reporter = native
        .reporters
        .lock()
        .unwrap()
        .remove(&resource.identity().context())
        .unwrap();
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    assert_eq!(owner.reap_absent(&resource), Err(Refusal::Busy));
    assert!(!owner.locally_retired());
    drop(reporter);
    owner.reap_absent(&resource).unwrap();
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
    assert!(owner.shared.global_current());
    assert_eq!(server.join().unwrap(), 0);
}

#[test]
fn opaque_probe_owner_uses_original_rows_and_refuses_second_resource_actor_or_early_seal() {
    use super::super::probe::RetainedWorkProbeOwner;
    let native = Arc::new(Native::default());
    let port = native.clone();
    let mut owner = RetainedWorkProbeOwner::new(
        ProfileId::generate(),
        Arc::new(|| true),
        Box::new(move |_| Some(port)),
    )
    .unwrap();
    let target = ContextNavigationTarget::parse("https://retained-fixture.invalid/frozen").unwrap();
    owner.construct(target.clone(), now()).unwrap();
    let resource = owner.resource().unwrap().clone();
    assert!(owner.construct(target, now()).is_err());
    assert!(
        matches!(owner.poll_lifecycle(now()).unwrap(), Some(WorkBrowserResourceEvent::Retained(join)) if join == resource)
    );
    assert!(owner
        .poll_seal(ContextResourceAuditId::new(1).unwrap())
        .is_err());
    assert!(!owner.locally_retired());
    owner
        .acquire(
            ContextRunId::generate(),
            now(),
            AgentPolicyInstant::from_millis(100),
        )
        .unwrap();
    assert!(owner
        .acquire(
            ContextRunId::generate(),
            now(),
            AgentPolicyInstant::from_millis(100)
        )
        .is_err());
    assert!(
        matches!(owner.poll_lifecycle(now()).unwrap(), Some(WorkBrowserResourceEvent::Acquired(lease)) if lease.resource() == &resource)
    );
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    assert_eq!(native.reads.load(Ordering::Acquire), 0);
    let reporter = native
        .reporters
        .lock()
        .unwrap()
        .remove(&resource.identity().context())
        .unwrap();
    owner.destroy().unwrap();
    assert!(
        matches!(owner.poll_lifecycle(now()).unwrap(), Some(WorkBrowserResourceEvent::Destroyed(join)) if join == resource)
    );
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert_eq!(
        owner.poll_seal(ContextResourceAuditId::new(1).unwrap()),
        Ok(false)
    );
    assert!(!owner.locally_retired());
    drop(reporter);
    // This fixture intentionally cannot mint a native global audit. The opaque
    // bridge preserves that refusal instead of converting local owner closure.
    assert!(owner
        .poll_seal(ContextResourceAuditId::new(1).unwrap())
        .is_err());
    assert!(owner.locally_retired());
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
    let lanes = owner.shared.notifications.actors.lock().unwrap().len();
    assert!(a
        .register_listener(Arc::new(CountWake(AtomicUsize::new(0))).into())
        .is_err());
    assert_eq!(
        owner.shared.notifications.actors.lock().unwrap().len(),
        lanes
    );
    b.check_health(now()).unwrap();
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
