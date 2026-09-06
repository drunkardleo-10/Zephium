use super::*;

impl WorkBrowserResources {
    // Most schedules settle on their last supplied tick; timing adversaries
    // call the production settle_at boundary explicitly with a changed clock.
    fn settle(
        &mut self,
        completion: WorkBrowserResourceCompletion,
    ) -> Result<WorkBrowserResourceEvent, WorkBrowserResourceError> {
        let now = self
            .rows
            .get(&completion.operation.resource.identity.resource)
            .map(|row| row.last_tick)
            .unwrap_or(tick(0));
        self.settle_at(completion, now)
    }
}

fn tick(value: u64) -> AgentPolicyInstant {
    AgentPolicyInstant::from_millis(value)
}

#[derive(Default)]
struct HealthWake(std::sync::atomic::AtomicUsize);
impl std::task::Wake for HealthWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[test]
fn health_is_exact_private_resource_bound_and_finite_sticky_coalesced() {
    use WorkBrowserResourceHealthState as H;
    let mut rows = registry();
    let resource = create(&mut rows, 701);
    let (mut health, reporter) = health::track(resource.clone());
    let wake = Arc::new(HealthWake::default());
    assert_eq!(health.register(wake.clone().into()), H::Pending);
    assert!(reporter.install(&resource));
    assert_eq!(health.snapshot(), H::Current);
    assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    for _ in 0..10_000 {
        reporter.invalidate();
    }
    assert_eq!(health.snapshot(), H::Uncertain);
    assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(health.poll(), H::Uncertain);
    reporter.invalidate();
    assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    drop(reporter);
    assert_eq!(health.poll(), H::Uncertain);
    assert!(!rows.is_quiescent());

    let mut identical_ids = registry();
    let foreign = create(&mut identical_ids, 701);
    assert_eq!(resource.identity(), foreign.identity());
    assert_ne!(resource, foreign);
    let (mut health, reporter) = health::track(resource.clone());
    health.register(wake.clone().into());
    assert!(!reporter.install(&foreign));
    assert!(!reporter.is_current(&resource));
    assert_eq!(health.poll(), H::Uncertain);
}

#[test]
fn health_original_registration_cannot_be_rebound_and_retirement_is_not_absence() {
    use WorkBrowserResourceHealthState as H;
    let mut rows = registry();
    let resource = create(&mut rows, 702);
    let (mut health, reporter) = health::track(resource.clone());
    health.register(Arc::new(HealthWake::default()).into());
    assert!(reporter.install(&resource));
    health.poll();
    drop(reporter);
    assert_eq!(health.poll(), H::Retired);
    assert!(!rows.is_quiescent());
    let (mut health, reporter) = health::track(resource.clone());
    health.register(Arc::new(HealthWake::default()).into());
    assert!(reporter.install(&resource));
    assert_eq!(
        health.register(Arc::new(HealthWake::default()).into()),
        H::Uncertain
    );
    assert!(!reporter.is_current(&resource));
}

#[test]
fn health_missing_registration_receiver_or_reporter_and_duplicate_install_close_admission() {
    use WorkBrowserResourceHealthState as H;
    let mut rows = registry();
    let resource = create(&mut rows, 703);
    let (mut health, reporter) = health::track(resource.clone());
    assert!(!reporter.install(&resource));
    // Register-then-recheck observes a publication preceding registration;
    // it cannot heal the original missing delivery owner.
    let wake = Arc::new(HealthWake::default());
    assert_eq!(health.register(wake.clone().into()), H::Uncertain);
    assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    let (mut health, reporter) = health::track(resource.clone());
    health.register(wake.clone().into());
    assert!(reporter.install(&resource));
    assert!(!reporter.install(&resource));
    assert_eq!(health.poll(), H::Uncertain);
    let (mut health, reporter) = health::track(resource.clone());
    health.register(wake.into());
    assert!(reporter.install(&resource));
    drop(health);
    assert!(!reporter.is_current(&resource));
    let (health, reporter) = health::track(resource);
    drop(reporter);
    assert_eq!(health.snapshot(), H::Uncertain);
}

#[test]
fn health_wake_panic_is_contained_and_cannot_authorize_native_execution() {
    struct Panics;
    impl std::task::Wake for Panics {
        fn wake(self: Arc<Self>) {
            panic!("intentional health wake failure");
        }
    }
    let mut rows = registry();
    let resource = create(&mut rows, 704);
    let (mut health, reporter) = health::track(resource.clone());
    health.register(Arc::new(Panics).into());
    assert!(!reporter.install(&resource));
    assert_eq!(health.poll(), WorkBrowserResourceHealthState::Uncertain);
    assert!(!reporter.is_current(&resource));
}

#[test]
fn health_request_attachment_is_construct_only_move_only_and_unhandled_fails_closed() {
    let mut rows = registry();
    let request = rows
        .construct(
            WorkBrowserResourceId::from_raw(705),
            ContextId::from_raw(805),
            ContextProfileStorageClass::Ephemeral,
            tick(0),
        )
        .unwrap();
    let (request, mut health) = request.track_resource_health().unwrap();
    let request = *request.track_resource_health().unwrap_err();
    health.register(Arc::new(HealthWake::default()).into());
    let resource = request.resource().clone();
    let _ = rows
        .settle(request.complete(WorkBrowserResourceNativeOutcome::Constructed))
        .unwrap();
    assert_eq!(health.poll(), WorkBrowserResourceHealthState::Uncertain);
    let request = rows
        .acquire(&resource, ContextRunId::from_raw(706), tick(1), tick(100))
        .unwrap();
    assert!(request.track_resource_health().is_err());
}
fn registry() -> WorkBrowserResources {
    WorkBrowserResources::new(WorkId::from_raw(1), ProfileId::from(2))
}
fn create(registry: &mut WorkBrowserResources, id: u128) -> WorkBrowserResourceJoin {
    let request = registry
        .construct(
            WorkBrowserResourceId::from_raw(id),
            ContextId::from_raw(id + 100),
            ContextProfileStorageClass::Ephemeral,
            tick(0),
        )
        .unwrap();
    match registry
        .settle(request.complete(WorkBrowserResourceNativeOutcome::Constructed))
        .unwrap()
    {
        WorkBrowserResourceEvent::Retained(resource) => resource,
        _ => panic!("construction must retain resource"),
    }
}
fn acquire(
    registry: &mut WorkBrowserResources,
    resource: &WorkBrowserResourceJoin,
    run: u128,
) -> WorkBrowserExecutionLease {
    let request = registry
        .acquire(resource, ContextRunId::from_raw(run), tick(1), tick(100))
        .unwrap();
    match registry
        .settle(request.complete(WorkBrowserResourceNativeOutcome::Acquired))
        .unwrap()
    {
        WorkBrowserResourceEvent::Acquired(lease) => lease,
        _ => panic!("native lease binding"),
    }
}
fn drained() -> WorkBrowserResourceNativeOutcome {
    WorkBrowserResourceNativeOutcome::Revoked {
        debt: WorkBrowserLeaseNativeDebt::default(),
        resource_retained: true,
    }
}

fn ended(
    registry: &mut WorkBrowserResources,
    request: WorkBrowserResourceRequest,
) -> WorkBrowserLeaseEnded {
    match registry.settle(request.complete(drained())).unwrap() {
        WorkBrowserResourceEvent::LeaseEnded(ended) => ended,
        _ => panic!("exact lease ended"),
    }
}

#[test]
fn delivery_requires_normal_native_return_beside_exact_core_terminal_and_consumes_once() {
    let mut rows = registry();
    let resource = create(&mut rows, 301);
    let lease = acquire(&mut rows, &resource, 302);
    let (mut request, mut ticket) = rows.revoke_with_delivery(&lease).unwrap();
    let native = request.take_lease_delivery_completion().unwrap();
    assert!(request.take_lease_delivery_completion().is_none());
    let ended = ended(&mut rows, request);
    assert!(ticket.try_take().unwrap().is_none());
    assert!(!rows.is_quiescent());
    assert!(native.publish_returned());
    let receipt = ticket.try_take().unwrap().unwrap();
    assert!(receipt.returned());
    assert_eq!(
        ticket.try_take().unwrap_err(),
        WorkBrowserLeaseDeliveryPollError::Consumed
    );
    let proof = ended.join_delivery(receipt).unwrap();
    assert_eq!(proof.lease(), &lease);
    assert!(!rows.is_quiescent());
    assert!(rows.observe_initial(&lease, tick(2)).is_err());
    destroy(&mut rows, &resource);
}

#[test]
fn delivery_unhandled_adapter_or_dropped_native_half_is_never_returned() {
    for explicitly_taken in [false, true] {
        let mut rows = registry();
        let resource = create(&mut rows, 303);
        let lease = acquire(&mut rows, &resource, 304);
        let (mut request, mut ticket) = rows.revoke_with_delivery(&lease).unwrap();
        if explicitly_taken {
            drop(request.take_lease_delivery_completion());
        }
        let ended = ended(&mut rows, request);
        let receipt = ticket.try_take().unwrap().unwrap();
        assert!(!receipt.returned());
        let refusal = ended.join_delivery(receipt).unwrap_err();
        let (ended, receipt) = refusal.into_parts();
        assert_eq!(ended.lease(), &lease);
        assert!(!receipt.returned());
        assert!(!rows.is_quiescent());
    }
}

#[test]
fn delivery_consumer_loss_cannot_acknowledge_normal_return() {
    let mut rows = registry();
    let resource = create(&mut rows, 305);
    let lease = acquire(&mut rows, &resource, 306);
    let (mut request, ticket) = rows.revoke_with_delivery(&lease).unwrap();
    let native = request.take_lease_delivery_completion().unwrap();
    drop(ticket);
    assert!(!native.publish_returned());
    let _ended = ended(&mut rows, request);
    assert!(!rows.is_quiescent());
}

#[test]
fn delivery_same_public_lease_cannot_substitute_another_private_request_slot() {
    let mut rows = registry();
    let resource = create(&mut rows, 307);
    let lease = acquire(&mut rows, &resource, 308);
    let (mut request, mut ticket) = rows.revoke_with_delivery(&lease).unwrap();
    let native = request.take_lease_delivery_completion().unwrap();
    let (mut foreign, mut foreign_ticket) = delivery::track(lease.clone());
    assert!(foreign.completion.take().unwrap().publish_returned());
    let ended = ended(&mut rows, request);
    let foreign_receipt = foreign_ticket.try_take().unwrap().unwrap();
    let refusal = ended.join_delivery(foreign_receipt).unwrap_err();
    let (ended, _) = refusal.into_parts();
    assert!(native.publish_returned());
    let proof = ended
        .join_delivery(ticket.try_take().unwrap().unwrap())
        .unwrap();
    assert_eq!(proof.lease(), &lease);
}

#[test]
fn delivery_legacy_lease_terminal_cannot_join_a_tracked_slot() {
    let mut rows = registry();
    let resource = create(&mut rows, 309);
    let lease = acquire(&mut rows, &resource, 310);
    let request = rows.revoke(&lease).unwrap();
    let ended = ended(&mut rows, request);
    let (mut foreign, mut ticket) = delivery::track(lease);
    assert!(foreign.completion.take().unwrap().publish_returned());
    assert!(ended
        .join_delivery(ticket.try_take().unwrap().unwrap())
        .is_err());
}

#[test]
fn delivery_stale_lease_receipts_refuse_without_losing_either_exact_owner() {
    let mut rows = registry();
    let resource = create(&mut rows, 311);
    let lease_a = acquire(&mut rows, &resource, 312);
    let (mut request_a, mut ticket_a) = rows.revoke_with_delivery(&lease_a).unwrap();
    let native_a = request_a.take_lease_delivery_completion().unwrap();
    let ended_a = ended(&mut rows, request_a);
    assert!(native_a.publish_returned());
    let lease_b = acquire(&mut rows, &resource, 313);
    let (mut request_b, mut ticket_b) = rows.revoke_with_delivery(&lease_b).unwrap();
    let native_b = request_b.take_lease_delivery_completion().unwrap();
    let ended_b = ended(&mut rows, request_b);
    assert!(native_b.publish_returned());
    let refused = ended_a
        .join_delivery(ticket_b.try_take().unwrap().unwrap())
        .unwrap_err();
    let (ended_a, receipt_b) = refused.into_parts();
    assert_eq!(ended_b.join_delivery(receipt_b).unwrap().lease(), &lease_b);
    assert_eq!(
        ended_a
            .join_delivery(ticket_a.try_take().unwrap().unwrap())
            .unwrap()
            .lease(),
        &lease_a
    );
    assert!(!rows.is_quiescent());
}

#[test]
fn delivery_synchronous_non_admission_retains_refusal_without_a_callback_proof() {
    let mut rows = registry();
    let resource = create(&mut rows, 314);
    let lease = acquire(&mut rows, &resource, 315);
    let (request, mut ticket) = rows.revoke_with_delivery(&lease).unwrap();
    assert!(matches!(
        rows.dispatch_refused(request, ContextPortFailure::Unsupported)
            .unwrap(),
        WorkBrowserResourceEvent::AdmissionRefused {
            operation: WorkBrowserResourceOperation::Revoke,
            failure: ContextPortFailure::Unsupported,
            ..
        }
    ));
    assert_eq!(
        rows.phase(&resource).unwrap(),
        WorkBrowserResourcePhase::Quarantined
    );
    assert!(!ticket.try_take().unwrap().unwrap().returned());
    assert!(rows
        .acquire(&resource, ContextRunId::from_raw(316), tick(2), tick(100))
        .is_err());
    assert!(!rows.is_quiescent());
}
fn destroy(registry: &mut WorkBrowserResources, resource: &WorkBrowserResourceJoin) {
    let request = registry.destroy(resource).unwrap();
    assert!(matches!(
        registry
            .settle(request.complete(WorkBrowserResourceNativeOutcome::Destroyed))
            .unwrap(),
        WorkBrowserResourceEvent::Destroyed(_)
    ));
}

#[test]
fn work_identity_persists_across_distinct_run_leases_without_page_relabeling() {
    let mut registry = registry();
    let resource = create(&mut registry, 3);
    let persisted = serde_json::to_string(&resource.identity()).unwrap();
    let restored: WorkBrowserResourceIdentity = serde_json::from_str(&persisted).unwrap();
    assert_eq!(restored, resource.identity());
    assert!(!persisted.contains("run"));
    let first = acquire(&mut registry, &resource, 4);
    assert!(registry.admits_lease(&first, tick(1)).is_ok());
    let request = registry.revoke(&first).unwrap();
    assert_eq!(
        registry.admits_lease(&first, tick(1)),
        Err(WorkBrowserResourceError::Phase)
    );
    let ended = match registry.settle(request.complete(drained())).unwrap() {
        WorkBrowserResourceEvent::LeaseEnded(receipt) => receipt,
        _ => panic!("exact lease-only drain"),
    };
    assert_eq!(ended.lease(), &first);
    assert_eq!(
        registry.phase(&resource).unwrap(),
        WorkBrowserResourcePhase::Retained
    );
    assert!(!registry.is_quiescent());
    let second = acquire(&mut registry, &resource, 5);
    assert_eq!(first.resource(), second.resource());
    assert_ne!(first.run(), second.run());
    assert_eq!(
        registry.admits_lease(&first, tick(1)),
        Err(WorkBrowserResourceError::Stale)
    );
    assert!(registry.admits_lease(&second, tick(1)).is_ok());
    let request = registry.revoke(&second).unwrap();
    let _ = registry.settle(request.complete(drained())).unwrap();
    registry.seal();
    assert!(!registry.is_quiescent());
    destroy(&mut registry, &resource);
    assert!(registry.is_quiescent());
}

#[test]
fn restored_identical_ids_cannot_substitute_registry_or_native_receipt_authority() {
    let mut original = registry();
    let resource = create(&mut original, 3);
    let mut restored = registry();
    let replacement = create(&mut restored, 3);
    assert_eq!(resource.identity(), replacement.identity());
    assert_ne!(resource, replacement);
    let request = original
        .acquire(&resource, ContextRunId::from_raw(4), tick(1), tick(100))
        .unwrap();
    let completion = request.complete(WorkBrowserResourceNativeOutcome::Acquired);
    assert_eq!(
        restored.settle(completion).unwrap_err(),
        WorkBrowserResourceError::Stale
    );
    assert_eq!(
        restored.phase(&replacement).unwrap(),
        WorkBrowserResourcePhase::Retained
    );
    assert_eq!(
        original.phase(&resource).unwrap(),
        WorkBrowserResourcePhase::Acquiring
    );
}

#[test]
fn every_native_debt_class_and_absent_page_refuses_lease_only_retirement() {
    for index in 0..8 {
        let mut registry = registry();
        let resource = create(&mut registry, 3);
        let lease = acquire(&mut registry, &resource, 4);
        let request = registry.revoke(&lease).unwrap();
        let mut debt = WorkBrowserLeaseNativeDebt::default();
        match index {
            0 => debt.queued_tasks = 1,
            1 => debt.observations = 1,
            2 => debt.actions = 1,
            3 => debt.navigations = 1,
            4 => debt.captures = 1,
            5 => debt.callbacks = 1,
            6 => debt.callbacks = u8::MAX,
            _ => {}
        }
        let result = WorkBrowserResourceNativeOutcome::Revoked {
            debt,
            resource_retained: index != 7,
        };
        assert!(matches!(
            registry.settle(request.complete(result)).unwrap(),
            WorkBrowserResourceEvent::Quarantined(WorkBrowserResourceFailure::DrainUnproven)
        ));
        assert_eq!(
            registry.admits_lease(&lease, tick(1)),
            Err(WorkBrowserResourceError::Quarantined)
        );
        assert!(registry
            .acquire(&resource, ContextRunId::from_raw(5), tick(1), tick(100))
            .is_err());
        assert!(!registry.is_quiescent());
        destroy(&mut registry, &resource);
        registry.seal();
        assert!(registry.is_quiescent());
    }
}

#[test]
fn uncertain_callback_never_blocks_reserved_destruction_or_disappears_as_clean() {
    for destruction_first in [false, true] {
        let mut registry = registry();
        let resource = create(&mut registry, 3);
        let request = registry
            .acquire(&resource, ContextRunId::from_raw(4), tick(1), tick(100))
            .unwrap();
        registry.quarantine(&resource).unwrap();
        let close = registry.destroy(&resource).unwrap();
        registry.seal();
        if destruction_first {
            let _ = registry
                .settle(close.complete(WorkBrowserResourceNativeOutcome::Destroyed))
                .unwrap();
            assert!(!registry.is_quiescent());
            assert_eq!(
                registry.reap(&resource),
                Err(WorkBrowserResourceError::Pending)
            );
            assert!(matches!(
                registry
                    .settle(request.complete(WorkBrowserResourceNativeOutcome::Acquired))
                    .unwrap(),
                WorkBrowserResourceEvent::DebtSettled(_)
            ));
        } else {
            assert!(matches!(
                registry
                    .settle(request.complete(WorkBrowserResourceNativeOutcome::Acquired))
                    .unwrap(),
                WorkBrowserResourceEvent::DebtSettled(_)
            ));
            assert_eq!(
                registry.phase(&resource).unwrap(),
                WorkBrowserResourcePhase::Destroying
            );
            assert!(!registry.is_quiescent());
            let _ = registry
                .settle(close.complete(WorkBrowserResourceNativeOutcome::Destroyed))
                .unwrap();
        }
        assert!(registry.is_quiescent());
        assert_eq!(registry.reap(&resource).unwrap(), resource.identity());
    }
}

#[test]
fn quarantine_is_scoped_sticky_and_retains_execution_capacity() {
    let mut registry = registry();
    let resources: Vec<_> = (3..8).map(|id| create(&mut registry, id)).collect();
    let mut leases = Vec::new();
    for (index, resource) in resources.iter().take(MAX_EXECUTING_CONTEXTS).enumerate() {
        leases.push(acquire(&mut registry, resource, 20 + index as u128));
    }
    registry.quarantine(&resources[0]).unwrap();
    assert_eq!(
        registry.admits_lease(&leases[0], tick(1)),
        Err(WorkBrowserResourceError::Quarantined)
    );
    assert!(registry.admits_lease(&leases[1], tick(1)).is_ok());
    assert_eq!(
        registry
            .acquire(
                &resources[4],
                ContextRunId::from_raw(25),
                tick(1),
                tick(100)
            )
            .unwrap_err(),
        WorkBrowserResourceError::Capacity
    );
    destroy(&mut registry, &resources[0]);
    let lease = acquire(&mut registry, &resources[4], 25);
    assert!(registry.admits_lease(&lease, tick(1)).is_ok());
}

#[test]
fn expiry_clock_regression_and_seal_do_not_erase_cleanup_authority() {
    let mut registry = registry();
    let resource = create(&mut registry, 3);
    let lease = acquire(&mut registry, &resource, 4);
    assert_eq!(
        registry.admits_lease(&lease, tick(100)),
        Err(WorkBrowserResourceError::Expired)
    );
    let request = registry.revoke(&lease).unwrap();
    let _ = registry.settle(request.complete(drained())).unwrap();
    assert_eq!(
        registry
            .acquire(&resource, ContextRunId::from_raw(5), tick(99), tick(200))
            .unwrap_err(),
        WorkBrowserResourceError::Quarantined
    );
    registry.seal();
    assert_eq!(
        registry.admits_lease(&lease, tick(101)),
        Err(WorkBrowserResourceError::Sealed)
    );
    destroy(&mut registry, &resource);
    assert!(registry.is_quiescent());
}

#[test]
fn exhausted_acquisition_ids_cannot_block_revoke_or_reserved_destruction() {
    let mut registry = registry();
    let resource = create(&mut registry, 3);
    let lease = acquire(&mut registry, &resource, 4);
    registry.sequence = u64::MAX;
    let request = registry.revoke(&lease).unwrap();
    let _ = registry.settle(request.complete(drained())).unwrap();
    assert_eq!(
        registry
            .acquire(&resource, ContextRunId::from_raw(5), tick(1), tick(100))
            .unwrap_err(),
        WorkBrowserResourceError::Exhausted
    );
    destroy(&mut registry, &resource);
    registry.seal();
    assert!(registry.is_quiescent());
}

#[test]
fn wrong_phase_duplicate_and_old_incarnation_receipts_cannot_reopen_resources() {
    let mut registry = registry();
    let resource = create(&mut registry, 3);
    let request = registry
        .acquire(&resource, ContextRunId::from_raw(4), tick(1), tick(100))
        .unwrap();
    let duplicate = WorkBrowserResourceCompletion {
        operation: request.operation.clone(),
        outcome: WorkBrowserResourceNativeOutcome::Acquired,
        delivery: None,
    };
    assert!(matches!(
        registry
            .settle(request.complete(WorkBrowserResourceNativeOutcome::Constructed))
            .unwrap(),
        WorkBrowserResourceEvent::Quarantined(WorkBrowserResourceFailure::Contract)
    ));
    assert_eq!(
        registry.settle(duplicate).unwrap_err(),
        WorkBrowserResourceError::Stale
    );
    destroy(&mut registry, &resource);
    registry.reap(&resource).unwrap();
    let replacement = create(&mut registry, 3);
    assert_eq!(resource.identity(), replacement.identity());
    assert_ne!(resource, replacement);
    assert_eq!(
        registry.phase(&resource),
        Err(WorkBrowserResourceError::Stale)
    );
}

#[test]
fn bounds_duplicate_contexts_and_shutdown_remain_exact() {
    let mut registry = registry();
    for id in 3..(3 + MAX_LIVE_CONTEXTS as u128) {
        create(&mut registry, id);
    }
    assert_eq!(
        registry
            .construct(
                WorkBrowserResourceId::from_raw(99),
                ContextId::from_raw(103),
                ContextProfileStorageClass::Ephemeral,
                tick(0)
            )
            .unwrap_err(),
        WorkBrowserResourceError::Duplicate
    );
    assert_eq!(
        registry
            .construct(
                WorkBrowserResourceId::from_raw(99),
                ContextId::from_raw(199),
                ContextProfileStorageClass::Ephemeral,
                tick(0)
            )
            .unwrap_err(),
        WorkBrowserResourceError::Capacity
    );
    registry.seal();
    assert!(!registry.is_quiescent());
    assert_eq!(
        registry
            .construct(
                WorkBrowserResourceId::from_raw(99),
                ContextId::from_raw(199),
                ContextProfileStorageClass::Ephemeral,
                tick(0)
            )
            .unwrap_err(),
        WorkBrowserResourceError::Sealed
    );
}

#[test]
fn synchronous_nonadmission_is_not_a_native_receipt_or_unknown_construction() {
    let mut registry = registry();
    let request = registry
        .construct(
            WorkBrowserResourceId::from_raw(3),
            ContextId::from_raw(103),
            ContextProfileStorageClass::Ephemeral,
            tick(0),
        )
        .unwrap();
    let resource = request.resource().clone();
    assert!(matches!(
        registry
            .dispatch_refused(request, ContextPortFailure::Unsupported)
            .unwrap(),
        WorkBrowserResourceEvent::AdmissionRefused {
            operation: WorkBrowserResourceOperation::Construct,
            ..
        }
    ));
    registry.seal();
    assert!(registry.is_quiescent());
    assert_eq!(registry.reap(&resource).unwrap(), resource.identity());
}

#[test]
fn refused_acquire_preserves_resource_but_refused_revoke_preserves_native_debt() {
    let mut registry = registry();
    let resource = create(&mut registry, 3);
    let request = registry
        .acquire(&resource, ContextRunId::from_raw(4), tick(1), tick(100))
        .unwrap();
    let unused = request.lease().unwrap().clone();
    let _ = registry
        .dispatch_refused(request, ContextPortFailure::Unsupported)
        .unwrap();
    assert_eq!(
        registry.phase(&resource).unwrap(),
        WorkBrowserResourcePhase::Retained
    );
    assert_eq!(
        registry.admits_lease(&unused, tick(1)),
        Err(WorkBrowserResourceError::Stale)
    );
    let lease = acquire(&mut registry, &resource, 5);
    let request = registry.revoke(&lease).unwrap();
    let _ = registry
        .dispatch_refused(request, ContextPortFailure::ResourceExhausted)
        .unwrap();
    assert_eq!(
        registry.phase(&resource).unwrap(),
        WorkBrowserResourcePhase::Quarantined
    );
    let close = registry.destroy(&resource).unwrap();
    let _ = registry
        .dispatch_refused(close, ContextPortFailure::Shutdown)
        .unwrap();
    assert!(registry.destroy(&resource).is_err());
    registry.seal();
    assert!(!registry.is_quiescent());
}

#[test]
fn late_or_shutdown_raced_acquisition_requires_revocation_without_ever_becoming_leased() {
    for sealed in [false, true] {
        let mut registry = registry();
        let resource = create(&mut registry, 3);
        let request = registry
            .acquire(&resource, ContextRunId::from_raw(4), tick(1), tick(100))
            .unwrap();
        if sealed {
            registry.seal();
        }
        let now = tick(if sealed { 2 } else { 100 });
        let lease = match registry
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Acquired),
                now,
            )
            .unwrap()
        {
            WorkBrowserResourceEvent::RevocationRequired(lease) => lease,
            _ => panic!("late acquisition must not activate"),
        };
        assert_eq!(
            registry.phase(&resource).unwrap(),
            WorkBrowserResourcePhase::RevocationRequired
        );
        assert!(registry.admits_lease(&lease, now).is_err());
        let request = registry.revoke(&lease).unwrap();
        assert!(matches!(
            registry
                .settle_at(request.complete(drained()), now)
                .unwrap(),
            WorkBrowserResourceEvent::LeaseEnded(_)
        ));
        destroy(&mut registry, &resource);
        registry.seal();
        assert!(registry.is_quiescent());
    }
}

#[test]
fn quarantine_during_acquiring_is_sticky_on_late_native_success() {
    let mut registry = registry();
    let resource = create(&mut registry, 3);
    let request = registry
        .acquire(&resource, ContextRunId::from_raw(4), tick(1), tick(100))
        .unwrap();
    let lease = request.lease().unwrap().clone();
    registry.quarantine(&resource).unwrap();
    assert!(matches!(
        registry
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Acquired),
                tick(2)
            )
            .unwrap(),
        WorkBrowserResourceEvent::Quarantined(WorkBrowserResourceFailure::CallbackUncertain)
    ));
    assert_eq!(
        registry.admits_lease(&lease, tick(2)),
        Err(WorkBrowserResourceError::Quarantined)
    );
    destroy(&mut registry, &resource);
}

#[test]
fn settlement_clock_failure_keeps_exact_terminal_and_independent_cleanup_owner() {
    let mut registry = registry();
    let resource = create(&mut registry, 3);
    let request = registry
        .acquire(&resource, ContextRunId::from_raw(4), tick(1), tick(100))
        .unwrap();
    assert!(matches!(
        registry
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Acquired),
                tick(0)
            )
            .unwrap(),
        WorkBrowserResourceEvent::Quarantined(WorkBrowserResourceFailure::ClockRegression)
    ));
    let close = registry.destroy(&resource).unwrap();
    let _ = registry
        .settle_at(
            close.complete(WorkBrowserResourceNativeOutcome::Destroyed),
            tick(0),
        )
        .unwrap();
    assert_eq!(
        registry
            .rows
            .get(&resource.identity.resource)
            .unwrap()
            .failure,
        Some(WorkBrowserResourceFailure::ClockRegression)
    );
    registry.seal();
    assert!(registry.is_quiescent()); // native/local closure only, not a clock/audit success
}

#[test]
fn stale_lease_and_incompatible_acquire_cannot_mutate_the_current_clock_or_owner() {
    let mut registry = registry();
    let resource = create(&mut registry, 3);
    let old = acquire(&mut registry, &resource, 4);
    let revoke = registry.revoke(&old).unwrap();
    let _ = registry.settle(revoke.complete(drained())).unwrap();
    let current = acquire(&mut registry, &resource, 5);
    assert!(registry.admits_lease(&current, tick(10)).is_ok());
    assert_eq!(
        registry.admits_lease(&old, tick(0)),
        Err(WorkBrowserResourceError::Stale)
    );
    assert_eq!(
        registry
            .acquire(&resource, ContextRunId::from_raw(6), tick(0), tick(100))
            .unwrap_err(),
        WorkBrowserResourceError::Phase
    );
    assert!(registry.admits_lease(&current, tick(10)).is_ok());
}

#[test]
fn resource_protocol_payloads_have_fixed_bounded_width_and_redacted_debug() {
    assert!(std::mem::size_of::<WorkBrowserResourceRequest>() <= 256);
    assert!(std::mem::size_of::<WorkBrowserResourceCompletion>() <= 256);
    assert!(std::mem::size_of::<WorkBrowserResourceDispatch>() <= 32);
    let mut registry = registry();
    let resource = create(&mut registry, 3);
    let lease = acquire(&mut registry, &resource, 4);
    let serialized = serde_json::to_string(&resource.identity()).unwrap();
    assert!(serialized.len() <= 192);
    let debug = format!("{lease:?}");
    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("ProfileId("));
    assert!(!debug.contains(&serde_json::to_string(&lease.run()).unwrap()));
}

#[test]
fn exact_nonadmission_survives_quarantine_without_retaining_a_fictitious_native_lease() {
    let mut registry = registry();
    let request = registry
        .construct(
            WorkBrowserResourceId::from_raw(3),
            ContextId::from_raw(103),
            ContextProfileStorageClass::Ephemeral,
            tick(0),
        )
        .unwrap();
    let resource = request.resource().clone();
    registry.quarantine(&resource).unwrap();
    let _ = registry
        .dispatch_refused(request, ContextPortFailure::Unsupported)
        .unwrap();
    assert_eq!(
        registry.phase(&resource).unwrap(),
        WorkBrowserResourcePhase::Destroyed
    );
    registry.reap(&resource).unwrap();
    let resource = create(&mut registry, 3);
    let request = registry
        .acquire(&resource, ContextRunId::from_raw(4), tick(1), tick(100))
        .unwrap();
    registry.quarantine(&resource).unwrap();
    let _ = registry
        .dispatch_refused(request, ContextPortFailure::Unsupported)
        .unwrap();
    let row = registry.rows.get(&resource.identity.resource).unwrap();
    assert!(row.lease.is_none());
    assert_eq!(row.phase, WorkBrowserResourcePhase::Quarantined);
    assert_eq!(
        row.failure,
        Some(WorkBrowserResourceFailure::CallbackUncertain)
    );
    destroy(&mut registry, &resource);
    registry.seal();
    assert!(registry.is_quiescent());
}
