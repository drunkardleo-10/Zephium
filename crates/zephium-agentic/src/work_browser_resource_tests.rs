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
