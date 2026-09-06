use super::*;

fn run_id(value: u128) -> ContextRunId {
    ContextRunId::parse(&format!("{value:026}")).unwrap()
}

#[derive(Default)]
struct Native {
    sink: Mutex<Option<NativeSink>>,
    reporters: Mutex<BTreeMap<ContextId, WorkBrowserResourceHealthReporter>>,
    lifecycle: Mutex<
        Option<(
            WorkBrowserResourceRequest,
            WorkBrowserResourceCompletionCallback,
        )>,
    >,
    read: Mutex<
        Option<(
            WorkBrowserObservationRequest,
            WorkBrowserObservationCompletionCallback,
        )>,
    >,
    delivery: Mutex<Option<WorkBrowserLeaseDeliveryCompletion>>,
    hold_revoke: AtomicBool,
    hold_read: AtomicBool,
    constructions: AtomicUsize,
    acquisitions: AtomicUsize,
    observations: AtomicUsize,
    reject_construct: std::sync::atomic::AtomicU8,
    rejected_lifecycle_callback: Mutex<Option<WorkBrowserResourceCompletionCallback>>,
    reject_read: std::sync::atomic::AtomicU8,
    rejected_read_callback: Mutex<Option<WorkBrowserObservationCompletionCallback>>,
    read_failure: Mutex<Option<ContextPortFailure>>,
}
impl Native {
    fn finish(
        &self,
        mut request: WorkBrowserResourceRequest,
        callback: WorkBrowserResourceCompletionCallback,
    ) {
        let outcome = match request.operation() {
            WorkBrowserResourceOperation::Construct => {
                let reporter = request
                    .take_resource_health_reporter()
                    .expect("tracked native observer");
                assert!(reporter.install(request.resource()));
                self.reporters
                    .lock()
                    .unwrap()
                    .insert(request.resource().identity().context(), reporter);
                self.constructions.fetch_add(1, Ordering::SeqCst);
                WorkBrowserResourceNativeOutcome::Constructed
            }
            WorkBrowserResourceOperation::Acquire => {
                self.acquisitions.fetch_add(1, Ordering::SeqCst);
                WorkBrowserResourceNativeOutcome::Acquired
            }
            WorkBrowserResourceOperation::Revoke => {
                *self.delivery.lock().unwrap() = request.take_lease_delivery_completion();
                WorkBrowserResourceNativeOutcome::Revoked {
                    debt: WorkBrowserLeaseNativeDebt::default(),
                    resource_retained: true,
                }
            }
            WorkBrowserResourceOperation::Destroy => {
                self.reporters
                    .lock()
                    .unwrap()
                    .remove(&request.resource().identity().context());
                WorkBrowserResourceNativeOutcome::Destroyed
            }
        };
        callback(request.complete(outcome));
    }
}
impl AgentBrowserPort for Native {
    fn work_resource_lifecycle(
        &self,
        request: WorkBrowserResourceRequest,
        callback: WorkBrowserResourceCompletionCallback,
    ) -> WorkBrowserResourceDispatch {
        if request.operation() == WorkBrowserResourceOperation::Construct {
            match self.reject_construct.load(Ordering::Acquire) {
                1 => {
                    drop(callback);
                    return WorkBrowserResourceDispatch::Rejected {
                        request: Box::new(request),
                        failure: ContextPortFailure::Unsupported,
                    };
                }
                2 => {
                    *self.rejected_lifecycle_callback.lock().unwrap() = Some(callback);
                    return WorkBrowserResourceDispatch::Rejected {
                        request: Box::new(request),
                        failure: ContextPortFailure::Unsupported,
                    };
                }
                3 => {
                    let mut foreign =
                        WorkBrowserResources::new(WorkId::generate(), ProfileId::generate());
                    let foreign = foreign
                        .construct(
                            WorkBrowserResourceId::generate(),
                            ContextId::generate(),
                            ContextProfileStorageClass::Ephemeral,
                            tick(0),
                        )
                        .unwrap();
                    callback(foreign.complete(WorkBrowserResourceNativeOutcome::Constructed));
                    return WorkBrowserResourceDispatch::Rejected {
                        request: Box::new(request),
                        failure: ContextPortFailure::Unsupported,
                    };
                }
                _ => {}
            }
        }
        if request.operation() == WorkBrowserResourceOperation::Revoke
            && self.hold_revoke.load(Ordering::Acquire)
        {
            assert!(self
                .lifecycle
                .lock()
                .unwrap()
                .replace((request, callback))
                .is_none());
        } else {
            self.finish(request, callback);
        }
        WorkBrowserResourceDispatch::Scheduled
    }
    fn work_resource_observe(
        &self,
        request: WorkBrowserObservationRequest,
        callback: WorkBrowserObservationCompletionCallback,
    ) -> WorkBrowserObservationDispatch {
        let reject = self.reject_read.load(Ordering::Acquire);
        if reject != 0 {
            if reject == 1 {
                drop(callback);
            } else {
                *self.rejected_read_callback.lock().unwrap() = Some(callback);
            }
            return WorkBrowserObservationDispatch::Rejected {
                request: Box::new(request),
                failure: self
                    .read_failure
                    .lock()
                    .unwrap()
                    .unwrap_or(ContextPortFailure::Unsupported),
            };
        }
        self.observations.fetch_add(1, Ordering::SeqCst);
        if self.hold_read.load(Ordering::Acquire) {
            assert!(self
                .read
                .lock()
                .unwrap()
                .replace((request, callback))
                .is_none());
        } else {
            let (_, terminal) = request.into_parts();
            callback(terminal);
        }
        WorkBrowserObservationDispatch::Scheduled
    }
    fn dispatch(&self, _: ContextNativeRequest) -> ContextDispatch {
        panic!("no legacy context authority")
    }
    fn transfer_cookies(&self, _: ContextCookieTransferRequest) -> ContextDispatch {
        panic!("no cookie authority")
    }
    fn audit_resources(&self, audit: ContextResourceAuditId) -> ContextDispatch {
        (self.sink.lock().unwrap().as_ref().unwrap())(ContextNativeEvent::ResourceAuditSettled(
            ContextResourceAuditSettlement::new(audit, Err(ContextPortFailure::NativeRefused)),
        ));
        ContextDispatch::Scheduled
    }
    fn seal_for_shutdown(&self, _: ContextResourceAuditId) -> ContextShutdownDispatch {
        ContextShutdownDispatch::SealedWithoutAudit(ContextPortFailure::NativeRefused)
    }
    fn invoke_semantic(&self, _: SemanticRuntimeInvocation) -> ContextDispatch {
        panic!("no legacy semantic authority")
    }
    fn execute_semantic_action(
        &self,
        _: SemanticActionNativeRequest,
        _: SemanticActionNativeCompletion,
    ) -> ContextDispatch {
        panic!("no action authority")
    }
    fn capture_semantic_screenshot(
        &self,
        _: SemanticScreenshotNativeRequest,
        _: SemanticScreenshotNativeCompletion,
    ) -> ContextDispatch {
        panic!("no screenshot authority")
    }
}
fn tick(value: u64) -> AgentPolicyInstant {
    AgentPolicyInstant::from_millis(value)
}
fn setup(wake: WakeApplication) -> (WorkResourceOwner, Arc<Native>) {
    let native = Arc::new(Native::default());
    let original = native.clone();
    let owner = WorkResourceOwner::new(
        WorkId::generate(),
        ProfileId::generate(),
        wake,
        Box::new(move |sink| {
            assert!(original.sink.lock().unwrap().replace(sink).is_none());
            Some(original)
        }),
    )
    .unwrap();
    (owner, native)
}
fn construct(owner: &WorkResourceOwner) -> WorkBrowserResourceJoin {
    let mut pending = owner
        .construct(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            ContextProfileStorageClass::Ephemeral,
            ContextNavigationTarget::parse("https://example.test/frozen").unwrap(),
            tick(0),
        )
        .unwrap();
    match pending.poll(tick(0)).unwrap().unwrap() {
        LifecycleResult::Event(WorkBrowserResourceEvent::Retained(join)) => join,
        _ => panic!("exact construction"),
    }
}
fn acquire(owner: &WorkResourceOwner, join: &WorkBrowserResourceJoin, run: u128) -> LeaseBrowser {
    acquire_at(owner, join, run, 1)
}
fn acquire_at(
    owner: &WorkResourceOwner,
    join: &WorkBrowserResourceJoin,
    run: u128,
    now: u64,
) -> LeaseBrowser {
    let mut pending = owner
        .acquire(join, run_id(run), tick(now), tick(100))
        .unwrap();
    match pending.poll(tick(now)).unwrap().unwrap() {
        LifecycleResult::Event(WorkBrowserResourceEvent::Acquired(lease)) => {
            owner.browser(lease, tick(now)).unwrap()
        }
        _ => panic!("exact acquisition"),
    }
}

#[test]
fn original_work_owner_and_sink_survive_two_exact_lease_facades_without_global_relabeling() {
    let (owner, native) = setup(Arc::new(|| true));
    let resource = construct(&owner);
    let a = acquire(&owner, &resource, 10);
    assert!(matches!(
        a.observe_initial(tick(2)).unwrap().poll(tick(2)).unwrap(),
        Some(WorkBrowserObservationEvent::Refused(_))
    ));
    let mut revoke = a.revoke().unwrap();
    assert!(revoke.poll(tick(3)).unwrap().is_none());
    assert!(matches!(
        owner.acquire(&resource, run_id(11), tick(3), tick(100)),
        Err(Refusal::Busy)
    ));
    assert!(native
        .delivery
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .publish_returned());
    assert!(matches!(
        revoke.poll(tick(3)).unwrap(),
        Some(LifecycleResult::Delivered(_))
    ));
    assert!(a.observe_initial(tick(3)).is_err());
    drop(a);
    // Stable native sink remains callable between actors; no A receiver needed.
    let _ = native.audit_resources(ContextResourceAuditId::new(1).unwrap());
    assert!(matches!(
        owner.poll_native_event().unwrap(),
        Some(ContextNativeEvent::ResourceAuditSettled(_))
    ));
    let b = acquire_at(&owner, &resource, 11, 3);
    assert_eq!(b.lease.resource(), &resource);
    assert!(matches!(
        b.observe_initial(tick(4)).unwrap().poll(tick(4)).unwrap(),
        Some(WorkBrowserObservationEvent::Refused(_))
    ));
    let mut revoke = b.revoke().unwrap();
    assert!(native
        .delivery
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .publish_returned());
    assert!(matches!(
        revoke.poll(tick(5)).unwrap(),
        Some(LifecycleResult::Delivered(_))
    ));
    drop(b);
    assert_eq!(native.constructions.load(Ordering::SeqCst), 1);
    assert_eq!(native.observations.load(Ordering::SeqCst), 2);
    assert!(!owner.locally_retired());
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(tick(6)).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
    assert!(native.reporters.lock().unwrap().is_empty());
}

#[test]
fn destroyed_terminal_cannot_reap_a_living_native_reporter_even_after_uncertainty() {
    for uncertain in [false, true] {
        let (owner, native) = setup(Arc::new(|| true));
        let resource = construct(&owner);
        // Model a surviving native delegate/guard after physical page removal.
        let reporter = native
            .reporters
            .lock()
            .unwrap()
            .remove(&resource.identity().context())
            .unwrap();
        if uncertain {
            reporter.invalidate();
        }
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(matches!(
            destroy.poll(tick(6)).unwrap(),
            Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
                _
            )))
        ));
        assert!(!owner.locally_retired());
        assert_eq!(owner.reap_absent(&resource), Err(Refusal::Busy));
        assert!(owner.shared.resource(&resource).is_ok());
        // Retirement still wakes the original live receiver. Uncertainty must
        // never masquerade as release of that reporting owner.
        drop(reporter);
        owner.seal_resources().unwrap();
        assert!(owner.locally_retired());
        assert_eq!(owner.reap_absent(&resource).unwrap(), resource.identity());
        assert!(owner.shared.global_current());
    }
}

#[test]
fn idle_native_uncertainty_quarantines_only_its_resource_and_cannot_be_rebound() {
    let (owner, native) = setup(Arc::new(|| true));
    let a = construct(&owner);
    let b = construct(&owner);
    native
        .reporters
        .lock()
        .unwrap()
        .get(&a.identity().context())
        .unwrap()
        .invalidate();
    assert!(matches!(
        owner.acquire(&a, run_id(1), tick(1), tick(100)),
        Err(Refusal::Uncertain)
    ));
    let _b = acquire(&owner, &b, 2);
    let (foreign, _) = setup(Arc::new(|| true));
    assert!(matches!(
        foreign.acquire(&a, run_id(1), tick(1), tick(100)),
        Err(Refusal::Core(WorkBrowserResourceError::Stale))
    ));
}

#[test]
fn queued_read_retirement_and_lost_receiver_never_reroute_to_a_successor() {
    let (owner, native) = setup(Arc::new(|| true));
    let resource = construct(&owner);
    let a = acquire(&owner, &resource, 1);
    native.hold_read.store(true, Ordering::Release);
    let mut read = a.observe_initial(tick(2)).unwrap();
    let mut revoke = a.revoke().unwrap();
    assert!(native
        .delivery
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .publish_returned());
    assert!(revoke.poll(tick(3)).unwrap().is_none());
    assert!(matches!(
        owner.acquire(&resource, run_id(2), tick(3), tick(100)),
        Err(Refusal::Busy)
    ));
    let (request, callback) = native.read.lock().unwrap().take().unwrap();
    let (_, terminal) = request.into_parts();
    callback(terminal);
    assert!(matches!(
        read.poll(tick(3)).unwrap(),
        Some(WorkBrowserObservationEvent::DebtSettled)
    ));
    assert!(matches!(
        revoke.poll(tick(3)).unwrap(),
        Some(LifecycleResult::Delivered(_))
    ));
    let b = acquire_at(&owner, &resource, 2, 3);
    let read = b.observe_initial(tick(4)).unwrap();
    drop(read);
    let (request, callback) = native.read.lock().unwrap().take().unwrap();
    callback(request.into_parts().1);
    assert!(b.health(tick(4)).is_err());
    assert!(!owner.locally_retired());
}

#[test]
fn lost_or_unproven_revocation_delivery_prevents_primitive_reacquisition() {
    for lost in [false, true] {
        let (owner, native) = setup(Arc::new(|| true));
        let resource = construct(&owner);
        let a = acquire(&owner, &resource, 1);
        let mut revoke = a.revoke().unwrap();
        if lost {
            drop(revoke);
        } else {
            drop(native.delivery.lock().unwrap().take());
            assert!(revoke.poll(tick(2)).is_err());
        }
        assert!(owner
            .acquire(&resource, run_id(2), tick(2), tick(100))
            .is_err());
    }
}

#[test]
fn global_event_overflow_and_wake_failure_are_sticky_not_actor_mailbox_failures() {
    let (owner, native) = setup(Arc::new(|| true));
    let resource = construct(&owner);
    for id in 1..=3 {
        let _ = native.audit_resources(ContextResourceAuditId::new(id).unwrap());
    }
    assert!(owner.shared.notifications.failed.load(Ordering::Acquire));
    assert!(owner
        .acquire(&resource, run_id(2), tick(2), tick(100))
        .is_err());
    owner.poll_native_event().unwrap();
    assert!(owner.shared.notifications.failed.load(Ordering::Acquire));
    let (owner, _) = setup(Arc::new(|| false));
    let request = owner.construct(
        WorkBrowserResourceId::generate(),
        ContextId::generate(),
        ContextProfileStorageClass::Ephemeral,
        ContextNavigationTarget::parse("https://example.test/frozen").unwrap(),
        tick(0),
    );
    assert!(request.is_ok());
    assert!(owner.shared.notifications.failed.load(Ordering::Acquire));
}

#[test]
fn application_retains_late_a_terminal_after_handle_drop_and_can_destroy_exact_resource() {
    let (owner, native) = setup(Arc::new(|| true));
    let resource = construct(&owner);
    let a = acquire(&owner, &resource, 1);
    native.hold_read.store(true, Ordering::Release);
    let read = a.observe_initial(tick(2)).unwrap();
    drop(read);
    drop(a);
    owner.drain_abandoned(tick(3)).unwrap();
    assert_eq!(
        owner
            .shared
            .resource(&resource)
            .unwrap()
            .flights
            .load(Ordering::Acquire),
        1
    );
    let (request, callback) = native.read.lock().unwrap().take().unwrap();
    callback(request.into_parts().1);
    owner.drain_abandoned(tick(3)).unwrap();
    assert_eq!(
        owner
            .shared
            .resource(&resource)
            .unwrap()
            .flights
            .load(Ordering::Acquire),
        0
    );
    assert!(owner
        .acquire(&resource, run_id(2), tick(3), tick(100))
        .is_err());
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(tick(4)).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
    assert!(native.reporters.lock().unwrap().is_empty());
}

#[test]
fn abandoned_revocation_accounts_exact_delivery_without_fictitious_flight_debt() {
    for returned in [false, true] {
        let (owner, native) = setup(Arc::new(|| true));
        let resource = construct(&owner);
        let a = acquire(&owner, &resource, 1);
        let revoke = a.revoke().unwrap();
        drop(revoke);
        drop(a);
        let native_delivery = native.delivery.lock().unwrap().take().unwrap();
        if returned {
            assert!(native_delivery.publish_returned());
        } else {
            drop(native_delivery);
        }
        owner.drain_abandoned(tick(2)).unwrap();
        assert_eq!(
            owner
                .shared
                .resource(&resource)
                .unwrap()
                .flights
                .load(Ordering::Acquire),
            0
        );
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(destroy.poll(tick(3)).unwrap().is_some());
        owner.seal_resources().unwrap();
        assert!(owner.locally_retired());
        assert_eq!(owner.reap_absent(&resource).unwrap(), resource.identity());
    }
}

#[test]
fn failure_claim_and_facade_drop_linearize_against_retirement_and_b_admission() {
    for failure_first in [true, false] {
        let (owner, native) = setup(Arc::new(|| true));
        let resource = construct(&owner);
        let a = acquire(&owner, &resource, 1);
        let state = a.resource.clone();
        let retirement = a.retired.clone();
        let mut revoke = a.revoke().unwrap();
        assert!(native
            .delivery
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .publish_returned());
        let (entered, entry) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let failing_actor = std::thread::spawn(move || {
            // Same failure claim used by LeaseSignal and LeaseBrowser::Drop.
            // Pause precisely after winning the former check→act gap, before
            // publication. No test-only hook changes production behavior.
            let claim = failure_first.then(|| a.retired.claim_failure().unwrap());
            entered.send(()).unwrap();
            released
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            drop(claim);
            drop(a);
        });
        entry
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let mut b = None;
        if failure_first {
            assert_eq!(
                retirement.state.load(Ordering::Acquire),
                LeaseRetirement::FAILED
            );
            assert!(!state.failed.load(Ordering::Acquire));
            assert_eq!(
                owner
                    .acquire(&resource, run_id(2), tick(2), tick(100))
                    .err(),
                Some(Refusal::Busy)
            );
            assert_eq!(revoke.poll(tick(2)).err(), Some(Refusal::Uncertain));
            assert!(state.failed.load(Ordering::Acquire));
            assert!(!state.reusable.load(Ordering::Acquire));
            assert_eq!(state.flights.load(Ordering::Acquire), 0);
            assert_eq!(
                owner
                    .acquire(&resource, run_id(2), tick(2), tick(100))
                    .err(),
                Some(Refusal::Uncertain)
            );
            assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
        } else {
            assert!(matches!(
                revoke.poll(tick(2)).unwrap(),
                Some(LifecycleResult::Delivered(_))
            ));
            assert!(retirement.is_retired());
            b = Some(acquire_at(&owner, &resource, 2, 2));
            b.as_ref().unwrap().health(tick(2)).unwrap();
            assert_eq!(native.acquisitions.load(Ordering::Acquire), 2);
        }
        release.send(()).unwrap();
        failing_actor.join().unwrap();
        if let Some(b) = b {
            b.health(tick(2)).unwrap();
            assert!(!state.failed.load(Ordering::Acquire));
            let mut revoke_b = b.revoke().unwrap();
            assert!(native
                .delivery
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .publish_returned());
            assert!(revoke_b.poll(tick(2)).unwrap().is_some());
            drop(b);
        }
        assert_eq!(native.observations.load(Ordering::Acquire), 0);
        let mut destroy = owner.destroy(&resource).unwrap();
        assert!(destroy.poll(tick(2)).unwrap().is_some());
        owner.seal_resources().unwrap();
        assert!(owner.locally_retired());
        assert!(native.reporters.lock().unwrap().is_empty());
    }
}

#[test]
fn exact_facade_drop_closes_only_its_unretired_lease_and_owner_drop_seals_all_actor_authority() {
    let (owner, native) = setup(Arc::new(|| true));
    let resource = construct(&owner);
    let a = acquire(&owner, &resource, 1);
    assert!(owner.browser(a.lease.clone(), tick(1)).is_err());
    let mut revoke = a.revoke().unwrap();
    assert!(native
        .delivery
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .publish_returned());
    assert!(revoke.poll(tick(2)).unwrap().is_some());
    let b = acquire_at(&owner, &resource, 2, 2);
    assert!(a.health(tick(2)).is_err());
    assert!(b.health(tick(2)).is_ok());
    drop(a);
    assert!(b.health(tick(2)).is_ok());
    assert!(b.health(tick(100)).is_err());
    drop(owner);
    assert!(b.health(tick(2)).is_err());
    assert!(b.observe_initial(tick(2)).is_err());

    let (owner, _) = setup(Arc::new(|| true));
    let resource = construct(&owner);
    drop(acquire(&owner, &resource, 1));
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(destroy.poll(tick(2)).unwrap().is_some());
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

#[test]
fn exact_construct_non_admission_closes_but_retained_callback_contradiction_cannot_disappear() {
    for retained in [false, true] {
        let (owner, native) = setup(Arc::new(|| true));
        native
            .reject_construct
            .store(if retained { 2 } else { 1 }, Ordering::Release);
        let mut pending = owner
            .construct(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("https://example.test/frozen").unwrap(),
                tick(0),
            )
            .unwrap();
        if retained {
            assert!(pending.resource.failed.load(Ordering::Acquire));
            assert!(pending.poll(tick(0)).unwrap().is_none());
            owner.seal_resources().unwrap();
            assert!(!owner.locally_retired());
            drop(native.rejected_lifecycle_callback.lock().unwrap().take());
            assert_eq!(pending.poll(tick(0)).err(), Some(Refusal::Uncertain));
        } else {
            assert!(matches!(
                pending.poll(tick(0)).unwrap(),
                Some(LifecycleResult::Event(
                    WorkBrowserResourceEvent::AdmissionRefused {
                        operation: WorkBrowserResourceOperation::Construct,
                        ..
                    }
                ))
            ));
        }
        owner.seal_resources().unwrap();
        assert!(owner.locally_retired());
    }
}

#[test]
fn resource_and_global_wakes_rearm_only_after_draining_their_published_state() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let (owner, native) = setup(Arc::new(move || {
        counter.fetch_add(1, Ordering::AcqRel);
        true
    }));
    let resource = construct(&owner);
    owner.poll_health(&resource).unwrap();
    assert!(owner.poll_native_event().unwrap().is_none());
    let baseline = wakes.load(Ordering::Acquire);
    for id in 1..=2 {
        let _ = native.audit_resources(ContextResourceAuditId::new(id).unwrap());
    }
    assert_eq!(wakes.load(Ordering::Acquire), baseline + 1);
    assert!(owner.poll_native_event().unwrap().is_some());
    assert!(owner.poll_native_event().unwrap().is_some());
    assert!(owner.poll_native_event().unwrap().is_none());
    native
        .reporters
        .lock()
        .unwrap()
        .get(&resource.identity().context())
        .unwrap()
        .invalidate();
    assert_eq!(wakes.load(Ordering::Acquire), baseline + 2);
    assert_eq!(
        owner.poll_health(&resource).unwrap(),
        WorkBrowserResourceHealthState::Uncertain
    );
}

#[test]
fn poisoned_first_observer_does_not_suppress_later_resources_idle_failure_wake() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let (owner, native) = setup(Arc::new(move || {
        counter.fetch_add(1, Ordering::AcqRel);
        true
    }));
    let _ = construct(&owner);
    let _ = construct(&owner);
    // Select the actual ordered-map first entry; generated IDs or insertion
    // order must not decide whether this adversarial schedule exercises the bug.
    let resources: Vec<_> = owner
        .shared
        .lock_resources()
        .unwrap()
        .values()
        .cloned()
        .collect();
    let [a, b] = resources.as_slice() else {
        panic!("exactly two resources")
    };
    assert!(a.join.identity().resource() < b.join.identity().resource());
    assert_eq!(wakes.load(Ordering::Acquire), 1);
    assert!(owner.shared.notifications.pending.load(Ordering::Acquire));
    assert_eq!(a.flights.load(Ordering::Acquire), 0);
    assert_eq!(b.flights.load(Ordering::Acquire), 0);
    poison(&a.health);
    assert_eq!(owner.poll_native_event().err(), Some(Refusal::Uncertain));
    assert!(a.failed.load(Ordering::Acquire));
    assert!(!b.failed.load(Ordering::Acquire));
    assert!(!owner.shared.notifications.failed.load(Ordering::Acquire));
    assert!(!owner.shared.notifications.pending.load(Ordering::Acquire));
    assert!(owner.shared.current(b).is_ok());
    let baseline = wakes.load(Ordering::Acquire);
    native
        .reporters
        .lock()
        .unwrap()
        .get(&b.join.identity().context())
        .unwrap()
        .invalidate();
    assert_eq!(wakes.load(Ordering::Acquire), baseline + 1);
    assert!(owner.shared.notifications.pending.load(Ordering::Acquire));
    // Even repeated failure of A cannot prevent accounting B's published fault.
    assert_eq!(owner.poll_native_event().err(), Some(Refusal::Uncertain));
    assert!(b.failed.load(Ordering::Acquire));
    assert!(!owner.shared.notifications.failed.load(Ordering::Acquire));
    assert_eq!(a.flights.load(Ordering::Acquire), 0);
    assert_eq!(b.flights.load(Ordering::Acquire), 0);
    assert!(!owner.locally_retired());
    // A local observer error must not consume/discard original global terminals.
    for id in 1..=2 {
        let _ = native.audit_resources(ContextResourceAuditId::new(id).unwrap());
    }
    assert!(matches!(
        owner.poll_native_event().unwrap(),
        Some(ContextNativeEvent::ResourceAuditSettled(_))
    ));
    assert!(matches!(
        owner.poll_native_event().unwrap(),
        Some(ContextNativeEvent::ResourceAuditSettled(_))
    ));
    assert_eq!(owner.poll_native_event().err(), Some(Refusal::Uncertain));
    assert!(!owner.shared.notifications.failed.load(Ordering::Acquire));
}

#[test]
fn late_a_read_after_exact_destruction_still_settles_its_original_owner_slot() {
    let (owner, native) = setup(Arc::new(|| true));
    let resource = construct(&owner);
    let a = acquire(&owner, &resource, 1);
    native.hold_read.store(true, Ordering::Release);
    drop(a.observe_initial(tick(2)).unwrap());
    drop(a);
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(destroy.poll(tick(3)).unwrap().is_some());
    owner.seal_resources().unwrap();
    assert!(!owner.locally_retired());
    let (request, callback) = native.read.lock().unwrap().take().unwrap();
    callback(request.into_parts().1);
    owner.drain_abandoned(tick(4)).unwrap();
    assert!(owner.locally_retired());
    assert!(matches!(
        owner
            .shutdown_audit(ContextResourceAuditId::new(1).unwrap())
            .unwrap(),
        ContextShutdownDispatch::SealedWithoutAudit(_)
    ));
}

#[test]
fn contradictory_read_callback_is_retained_and_accounted_after_refusal() {
    let (owner, native) = setup(Arc::new(|| true));
    let resource = construct(&owner);
    let a = acquire(&owner, &resource, 1);
    native.reject_read.store(2, Ordering::Release);
    let mut pending = a.observe_initial(tick(2)).unwrap();
    assert!(pending.poll(tick(2)).unwrap().is_none());
    assert!(a.health(tick(2)).is_err());
    drop(native.rejected_read_callback.lock().unwrap().take());
    assert_eq!(pending.poll(tick(2)).err(), Some(Refusal::Uncertain));
    assert_eq!(pending.resource.flights.load(Ordering::Acquire), 0);
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(destroy.poll(tick(3)).unwrap().is_some());
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

#[test]
fn synchronous_foreign_callback_plus_returned_request_is_explicit_uncertainty() {
    let (owner, native) = setup(Arc::new(|| true));
    native.reject_construct.store(3, Ordering::Release);
    let mut pending = owner
        .construct(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            ContextProfileStorageClass::Ephemeral,
            ContextNavigationTarget::parse("https://example.test/frozen").unwrap(),
            tick(0),
        )
        .unwrap();
    assert_eq!(pending.poll(tick(0)).err(), Some(Refusal::Uncertain));
    assert!(pending.resource.failed.load(Ordering::Acquire));
    assert_eq!(pending.resource.flights.load(Ordering::Acquire), 0);
    assert!(owner
        .acquire(&pending.resource.join, run_id(1), tick(1), tick(100))
        .is_err());
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
}

#[test]
fn synchronous_read_non_admission_preserves_its_exact_native_failure() {
    for failure in [
        ContextPortFailure::Unsupported,
        ContextPortFailure::ResourceExhausted,
        ContextPortFailure::Stale,
        ContextPortFailure::Cancelled,
        ContextPortFailure::TimedOut,
        ContextPortFailure::Shutdown,
        ContextPortFailure::NativeRefused,
    ] {
        let (owner, native) = setup(Arc::new(|| true));
        let resource = construct(&owner);
        let a = acquire(&owner, &resource, 1);
        native.reject_read.store(1, Ordering::Release);
        *native.read_failure.lock().unwrap() = Some(failure);
        let mut read = a.observe_initial(tick(2)).unwrap();
        assert_eq!(
            read.poll(tick(2)).err(),
            Some(Refusal::NativeAdmission(failure))
        );
        assert_eq!(read.resource.flights.load(Ordering::Acquire), 0);
        assert!(a.health(tick(2)).is_ok());
    }
}

fn poison<T>(mutex: &Mutex<T>) {
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = mutex.lock().unwrap();
        panic!("injected owner-state poison");
    }))
    .is_err());
    assert!(mutex.is_poisoned());
}

#[test]
fn refused_construction_retires_waking_reporter_before_any_owner_slot_or_rows_lock() {
    // Preflight the exact locks before actual reentry, so regressing the drop
    // location fails an assertion instead of hanging the test process forever.
    for schedule in 0..4 {
        let destination = Arc::new(Mutex::new(std::sync::Weak::<Shared>::new()));
        let attempted = Arc::new(AtomicUsize::new(0));
        let reentered = Arc::new(AtomicUsize::new(0));
        let blocked = Arc::new(AtomicBool::new(false));
        let wake_destination = destination.clone();
        let wake_attempted = attempted.clone();
        let wake_reentered = reentered.clone();
        let wake_blocked = blocked.clone();
        let (owner, native) = setup(Arc::new(move || {
            let Some(shared) = wake_destination.lock().unwrap().upgrade() else {
                return true;
            };
            wake_attempted.fetch_add(1, Ordering::AcqRel);
            let rows_available = shared.rows.try_lock().is_ok();
            let resources = shared
                .resources
                .try_lock()
                .ok()
                .map(|rows| rows.values().cloned().collect::<Vec<_>>());
            let locks_available = rows_available
                && resources.as_ref().is_some_and(|rows| {
                    rows.iter().all(|row| {
                        row.health.try_lock().is_ok()
                            && row.facade.try_lock().is_ok()
                            && row.slots.try_lock().is_ok_and(|slots| {
                                slots.iter().all(|slot| match slot {
                                    OwnedSlot::Lifecycle(slot) => slot.try_lock().is_ok(),
                                    OwnedSlot::Read(slot) => slot.try_lock().is_ok(),
                                })
                            })
                    })
                });
            if !locks_available {
                wake_blocked.store(true, Ordering::Release);
                return true;
            }
            // This is the real application-owned drain, not a second owner or
            // a simulated mutex-only wake. It must remain callable synchronously.
            shared.drain_abandoned(tick(0)).unwrap();
            for resource in resources.unwrap() {
                assert!(Arc::ptr_eq(
                    &shared.resource(&resource.join).unwrap(),
                    &resource
                ));
                let _ = shared.current(&resource);
            }
            wake_reentered.fetch_add(1, Ordering::AcqRel);
            true
        }));
        *destination.lock().unwrap() = Arc::downgrade(&owner.shared);
        native.reject_construct.store(1, Ordering::Release);
        let mut pending = owner
            .construct(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("https://example.test/frozen").unwrap(),
                tick(0),
            )
            .unwrap();
        let resource = pending.resource.clone();
        match schedule {
            0 => {
                assert!(pending.poll(tick(0)).unwrap().is_some());
                drop(pending);
            }
            1 => {
                drop(pending);
                owner.drain_abandoned(tick(0)).unwrap();
            }
            2 => {
                poison(&owner.shared.rows);
                assert_eq!(pending.poll(tick(0)).err(), Some(Refusal::Uncertain));
                drop(pending);
            }
            3 => {
                poison(&pending.slot);
                assert_eq!(pending.poll(tick(0)).err(), Some(Refusal::Uncertain));
                drop(pending);
                assert_eq!(owner.drain_abandoned(tick(0)), Err(Refusal::Uncertain));
            }
            _ => unreachable!(),
        }
        assert!(
            !blocked.load(Ordering::Acquire),
            "wake held an owner lock: {schedule}"
        );
        assert_eq!(attempted.load(Ordering::Acquire), 1, "{schedule}");
        assert_eq!(reentered.load(Ordering::Acquire), 1, "{schedule}");
        assert_eq!(native.constructions.load(Ordering::Acquire), 0);
        if schedule < 2 {
            assert_eq!(resource.flights.load(Ordering::Acquire), 0);
            owner.seal_resources().unwrap();
            assert!(owner.locally_retired());
        } else {
            assert_eq!(resource.flights.load(Ordering::Acquire), 1);
            assert!(!owner.locally_retired());
        }
        // Do not exercise the unrelated owner-Drop wake in this schedule.
        *destination.lock().unwrap() = std::sync::Weak::new();
    }
}

#[test]
fn original_owner_poison_closes_preexisting_facades_without_a_map_lookup() {
    for poison_rows in [false, true] {
        let (owner, native) = setup(Arc::new(|| true));
        let a_join = construct(&owner);
        let b_join = construct(&owner);
        let a = acquire(&owner, &a_join, 1);
        let b = acquire(&owner, &b_join, 2);
        if poison_rows {
            poison(&owner.shared.rows);
        } else {
            poison(&owner.shared.resources);
        }
        // Neither facade traverses the resources map; both must nevertheless
        // latch and observe its poison before any owner lookup/drain does so.
        assert_eq!(a.health(tick(2)), Err(Refusal::Uncertain));
        assert!(owner.shared.notifications.failed.load(Ordering::Acquire));
        assert_eq!(b.observe_initial(tick(2)).err(), Some(Refusal::Uncertain));
        assert_eq!(native.observations.load(Ordering::Acquire), 0);
        assert!(!owner.locally_retired());
    }
}

#[test]
fn original_map_entry_points_latch_global_poison_before_returning() {
    for entry in 0..7 {
        let (owner, native) = setup(Arc::new(|| true));
        let join = construct(&owner);
        let browser = acquire(&owner, &join, 1);
        poison(&owner.shared.resources);
        match entry {
            0 => assert_eq!(owner.poll_native_event().err(), Some(Refusal::Uncertain)),
            1 => assert_eq!(owner.poll_health(&join).err(), Some(Refusal::Uncertain)),
            2 => assert_eq!(owner.drain_abandoned(tick(2)), Err(Refusal::Uncertain)),
            3 => assert_eq!(
                owner
                    .construct(
                        WorkBrowserResourceId::generate(),
                        ContextId::generate(),
                        ContextProfileStorageClass::Ephemeral,
                        ContextNavigationTarget::parse("https://example.test/frozen").unwrap(),
                        tick(2),
                    )
                    .err(),
                Some(Refusal::Uncertain)
            ),
            4 => assert!(!owner.locally_retired()),
            5 => assert_eq!(owner.reap_absent(&join).err(), Some(Refusal::Uncertain)),
            6 => assert_eq!(
                owner.browser(browser.lease.clone(), tick(2)).err(),
                Some(Refusal::Uncertain)
            ),
            _ => unreachable!(),
        }
        assert!(
            owner.shared.notifications.failed.load(Ordering::Acquire),
            "{entry}"
        );
        assert_eq!(browser.health(tick(2)), Err(Refusal::Uncertain));
        assert!(browser.observe_initial(tick(2)).is_err());
        assert_eq!(native.observations.load(Ordering::Acquire), 0);
        assert_eq!(native.constructions.load(Ordering::Acquire), 1);
    }
}

#[test]
fn resource_local_poison_closes_only_its_exact_existing_facade() {
    for local in 0..3 {
        let (owner, native) = setup(Arc::new(|| true));
        let a_join = construct(&owner);
        let b_join = construct(&owner);
        let a = acquire(&owner, &a_join, 1);
        let b = acquire(&owner, &b_join, 2);
        match local {
            0 => poison(&a.resource.health),
            1 => poison(&a.resource.slots),
            2 => poison(&a.resource.facade),
            _ => unreachable!(),
        }
        assert_eq!(a.health(tick(2)), Err(Refusal::Uncertain));
        assert!(a.resource.failed.load(Ordering::Acquire));
        assert!(!owner.shared.notifications.failed.load(Ordering::Acquire));
        if local == 1 {
            assert_eq!(owner.drain_abandoned(tick(2)), Err(Refusal::Uncertain));
        }
        assert!(a.observe_initial(tick(2)).is_err());
        assert!(b.health(tick(2)).is_ok());
        assert!(b
            .observe_initial(tick(2))
            .unwrap()
            .poll(tick(2))
            .unwrap()
            .is_some());
        assert_eq!(native.observations.load(Ordering::Acquire), 1);
    }
}

#[test]
fn poisoned_read_slot_retains_late_terminal_and_reports_abandoned_drain_failure() {
    let (owner, native) = setup(Arc::new(|| true));
    let a_join = construct(&owner);
    let b_join = construct(&owner);
    let a = acquire(&owner, &a_join, 1);
    let b = acquire(&owner, &b_join, 2);
    native.hold_read.store(true, Ordering::Release);
    let mut pending = a.observe_initial(tick(2)).unwrap();
    poison(&pending.slot);
    // Direct polling itself must latch exact failure, without a health preflight.
    assert_eq!(pending.poll(tick(2)).err(), Some(Refusal::Uncertain));
    assert!(a.resource.failed.load(Ordering::Acquire));
    drop(pending);
    let (request, callback) = native.read.lock().unwrap().take().unwrap();
    callback(request.into_parts().1);
    assert_eq!(owner.drain_abandoned(tick(3)), Err(Refusal::Uncertain));
    assert_eq!(a.resource.flights.load(Ordering::Acquire), 1);
    assert_eq!(a.resource.reads.load(Ordering::Acquire), 1);
    // Another exact resource's abandoned slot is still accounted even when
    // either map iteration order encounters the poisoned slot first.
    let b_read = b.observe_initial(tick(3)).unwrap();
    let (request, callback) = native.read.lock().unwrap().take().unwrap();
    drop(b_read);
    callback(request.into_parts().1);
    assert_eq!(owner.drain_abandoned(tick(4)), Err(Refusal::Uncertain));
    assert_eq!(b.resource.flights.load(Ordering::Acquire), 0);
    assert_eq!(a.resource.flights.load(Ordering::Acquire), 1);
    assert!(!owner.shared.notifications.failed.load(Ordering::Acquire));
    assert!(!owner.locally_retired());
}

#[test]
fn poisoned_revocation_slot_cannot_publish_delivery_or_readmit_its_lease() {
    let (owner, native) = setup(Arc::new(|| true));
    let join = construct(&owner);
    let browser = acquire(&owner, &join, 1);
    native.hold_revoke.store(true, Ordering::Release);
    let mut pending = browser.revoke().unwrap();
    poison(&pending.slot);
    assert_eq!(pending.poll(tick(2)).err(), Some(Refusal::Uncertain));
    assert!(browser.resource.failed.load(Ordering::Acquire));
    drop(pending);
    let (request, callback) = native.lifecycle.lock().unwrap().take().unwrap();
    native.finish(request, callback);
    assert!(native
        .delivery
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .publish_returned());
    assert_eq!(owner.drain_abandoned(tick(3)), Err(Refusal::Uncertain));
    assert_eq!(browser.resource.flights.load(Ordering::Acquire), 1);
    assert!(!browser.retired.is_retired());
    assert_eq!(
        owner.acquire(&join, run_id(2), tick(3), tick(100)).err(),
        Some(Refusal::Uncertain)
    );
    assert!(!owner.shared.notifications.failed.load(Ordering::Acquire));
    assert!(!owner.locally_retired());
}

#[test]
fn busy_slot_inspection_never_waits_or_mislabels_callback_ownership() {
    let (owner, native) = setup(Arc::new(|| true));
    let join = construct(&owner);
    let browser = acquire(&owner, &join, 1);
    native.hold_read.store(true, Ordering::Release);
    let pending = browser.observe_initial(tick(2)).unwrap();
    let _poll_guard = pending.slot.lock().unwrap();
    let slots = browser.resource.slots.lock().unwrap();
    assert!(!slots.iter().all(|slot| slot.finished(&browser.resource)));
    assert!(!browser.resource.failed.load(Ordering::Acquire));
    assert_eq!(browser.resource.flights.load(Ordering::Acquire), 1);
}

#[test]
fn poisoned_pending_operation_is_visible_before_its_handle_is_polled_or_dropped() {
    let (owner, native) = setup(Arc::new(|| true));
    let join = construct(&owner);
    let browser = acquire(&owner, &join, 1);
    native.hold_read.store(true, Ordering::Release);
    let pending = browser.observe_initial(tick(2)).unwrap();
    poison(&pending.slot);
    assert_eq!(browser.health(tick(2)), Err(Refusal::Uncertain));
    assert!(browser.resource.failed.load(Ordering::Acquire));
    assert!(browser.observe_initial(tick(2)).is_err());
    assert_eq!(native.observations.load(Ordering::Acquire), 1);
    assert_eq!(browser.resource.flights.load(Ordering::Acquire), 1);
    assert!(!owner.shared.notifications.failed.load(Ordering::Acquire));
}

#[test]
fn slot_retention_discovers_poison_before_dispatching_an_already_prepared_read() {
    let (owner, native) = setup(Arc::new(|| true));
    let join = construct(&owner);
    let browser = acquire(&owner, &join, 1);
    let mut prior = browser.observe_initial(tick(2)).unwrap();
    assert!(prior.poll(tick(2)).unwrap().is_some());
    // Exact trusted request preparation wins, then another operation's state
    // poisons before this request can reserve its application-owned slot.
    browser.health(tick(3)).unwrap();
    let request = owner
        .shared
        .lock_rows()
        .unwrap()
        .observe_initial(&browser.lease, tick(3))
        .unwrap();
    poison(&prior.slot);
    let mut pending =
        PendingRead::dispatch(owner.shared.clone(), browser.resource.clone(), request).unwrap();
    assert!(browser.resource.failed.load(Ordering::Acquire));
    assert_eq!(native.observations.load(Ordering::Acquire), 1);
    assert_eq!(
        pending.poll(tick(3)).err(),
        Some(Refusal::NativeAdmission(ContextPortFailure::Shutdown))
    );
    assert_eq!(browser.resource.flights.load(Ordering::Acquire), 0);
    assert!(!owner.shared.notifications.failed.load(Ordering::Acquire));
    assert!(browser.health(tick(3)).is_err());
}
