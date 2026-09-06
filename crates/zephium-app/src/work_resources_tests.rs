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
            WorkBrowserResourceOperation::Acquire => WorkBrowserResourceNativeOutcome::Acquired,
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
