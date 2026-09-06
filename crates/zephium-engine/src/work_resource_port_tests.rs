use super::*;
use zephium_agentic::{
    ContextRunId, WorkBrowserResourceEvent, WorkBrowserResourceId, WorkBrowserResources, WorkId,
};

fn tick(value: u64) -> AgentPolicyInstant {
    AgentPolicyInstant::from_millis(value)
}
fn admission() -> Arc<AgentPortAdmission> {
    Arc::new(AgentPortAdmission::new(Arc::new(|_| {})))
}
fn source() -> (WorkBrowserResources, WorkBrowserResourceRequest) {
    let mut rows =
        WorkBrowserResources::new(WorkId::generate(), zephium_core::ids::ProfileId::generate());
    let request = rows
        .construct_document(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            ContextProfileStorageClass::Ephemeral,
            ContextNavigationTarget::parse("https://example.test/frozen").unwrap(),
            tick(0),
        )
        .unwrap();
    (rows, request)
}
fn setup() -> (
    WorkBrowserResources,
    Arc<AgentPortAdmission>,
    Arc<WorkResourceGuard>,
) {
    let (mut rows, request) = source();
    let admission = admission();
    let guard = Arc::new(WorkResourceGuard::new(&request, &admission));
    admission
        .work
        .lock()
        .unwrap()
        .rows
        .insert(request.resource().identity().context(), guard.clone());
    guard.outcome(&request, Outcome::Constructed);
    let _ = rows
        .settle_at(request.complete(Outcome::Constructed), tick(0))
        .unwrap();
    (rows, admission, guard)
}
fn leased(
    rows: &mut WorkBrowserResources,
    guard: &Arc<WorkResourceGuard>,
) -> WorkBrowserExecutionLease {
    let request = rows
        .acquire(
            guard.resource(),
            ContextRunId::generate(),
            tick(1),
            tick(1_000_000),
        )
        .unwrap();
    guard.admit_lifecycle(&request, tick(1)).unwrap();
    let lease = request.lease().unwrap().clone();
    guard.outcome(&request, Outcome::Acquired);
    let _ = rows
        .settle_at(request.complete(Outcome::Acquired), tick(1))
        .unwrap();
    lease
}
fn drained() -> Outcome {
    Outcome::Revoked {
        debt: zephium_agentic::WorkBrowserLeaseNativeDebt::default(),
        resource_retained: true,
    }
}
fn port(
    admission: Arc<AgentPortAdmission>,
    dispatch: MainThreadDispatch,
) -> EngineAgentBrowserPort {
    EngineAgentBrowserPort {
        admission,
        dispatch,
        sink: Arc::new(|_| {}),
    }
}
fn zero() -> ContextNativeResourceSnapshot {
    ContextNativeResourceSnapshot::try_new(zephium_agentic::ContextNativeResourceCounts {
        known_bindings: 0,
        resident_views: 0,
        owned_reservations: 0,
        borrowed_leases: 0,
        visible_surfaces: 0,
        suspended_views: 0,
        pending_operations: 0,
        pending_captures: 0,
        queued_tasks: 0,
    })
    .unwrap()
}

#[test]
fn queue_seal_revokes_an_already_admitted_read_before_native_execution() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let read = rows.observe_initial(&lease, tick(1)).unwrap();
    guard.admit_read(&read, tick(1)).unwrap();
    assert!(guard.admits(&lease, tick(1)));
    let revoke = rows.revoke(&lease).unwrap();
    guard.admit_lifecycle(&revoke, tick(1)).unwrap();
    assert!(!guard.admits(&lease, tick(1)));
    assert!(!guard.lease_drained(&lease));
    guard.read_terminal_begin();
    assert!(!guard.lease_drained(&lease));
    guard.read_terminal_end();
    assert!(guard.lease_drained(&lease));
    assert_eq!(admission.pending(), Some(0));
}

#[test]
fn acquisition_and_reads_recheck_original_expiry_and_shared_shutdown_seal() {
    let (mut rows, admission, guard) = setup();
    let request = rows
        .acquire(
            guard.resource(),
            ContextRunId::generate(),
            tick(1),
            tick(10),
        )
        .unwrap();
    let lease = request.lease().unwrap().clone();
    guard.admit_lifecycle(&request, tick(1)).unwrap();
    assert!(guard.acquisition_current(&lease, tick(9)));
    assert!(!guard.acquisition_current(&lease, tick(10)));
    admission.seal();
    assert!(!guard.acquisition_current(&lease, tick(1)));
    assert!(!guard.admits(&lease, tick(1)));
}

#[test]
fn returned_acquire_has_no_fictitious_native_lease_and_cannot_unquarantine() {
    for quarantine in [false, true] {
        let (mut rows, _, guard) = setup();
        let request = rows
            .acquire(
                guard.resource(),
                ContextRunId::generate(),
                tick(1),
                tick(100),
            )
            .unwrap();
        guard.admit_lifecycle(&request, tick(1)).unwrap();
        if quarantine {
            guard.fail();
        }
        guard.not_admitted(&request);
        assert!(!guard.execution_reserved());
        assert_eq!(guard.is_healthy(), !quarantine);
        assert_eq!(
            guard.state.lock().unwrap().phase,
            if quarantine {
                Phase::Quarantined
            } else {
                Phase::Retained
            }
        );
    }
}

#[test]
fn synchronous_refusal_returns_original_request_and_accepted_discard_settles_once() {
    for accepted in [false, true] {
        let (mut rows, request) = source();
        let resource = request.resource().clone();
        let admission = admission();
        let completions = Arc::new(Mutex::new(Vec::new()));
        let out = completions.clone();
        let port = port(
            admission.clone(),
            Arc::new(move |task| {
                drop(task);
                accepted
            }),
        );
        let result = port.work_resource_lifecycle(
            request,
            Box::new(move |completion| out.lock().unwrap().push(completion)),
        );
        if accepted {
            assert!(matches!(result, WorkBrowserResourceDispatch::Scheduled));
            let mut out = completions.lock().unwrap();
            assert_eq!(out.len(), 1);
            assert!(matches!(
                rows.settle_at(out.remove(0), tick(1)).unwrap(),
                WorkBrowserResourceEvent::Quarantined(_)
            ));
            assert!(!admission.work_is_absent());
        } else {
            let WorkBrowserResourceDispatch::Rejected { request, failure } = result else {
                panic!("exact refusal");
            };
            assert_eq!(request.resource(), &resource);
            let _ = rows.dispatch_refused(*request, failure).unwrap();
            assert!(completions.lock().unwrap().is_empty());
            assert!(admission.work_is_absent());
        }
        assert_eq!(admission.pending(), Some(0));
    }
}

#[test]
fn saturation_seals_revoke_without_dispatch_or_poisoning_another_resource() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let permits: Vec<_> = (0..MAX_PENDING_NATIVE_CONTEXT_TASKS)
        .map(|_| admission.reserve().unwrap())
        .collect();
    let request = rows.revoke(&lease).unwrap();
    let port = port(admission.clone(), Arc::new(|_| panic!("no queue capacity")));
    assert!(matches!(
        port.work_resource_lifecycle(request, Box::new(|_| panic!("not admitted"))),
        WorkBrowserResourceDispatch::Rejected { .. }
    ));
    assert!(!guard.admits(&lease, tick(1)));
    assert!(!guard.is_healthy());
    assert!(!admission.state.lock().unwrap().invariant_failed);
    drop(permits);
    assert_eq!(admission.pending(), Some(0));
}

#[test]
fn core_terminal_transfer_cannot_admit_reentrant_native_acquire_before_delivery_returns() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (request, ticket) = rows.revoke_with_delivery(&lease).unwrap();
    let ticket = Arc::new(Mutex::new(ticket));
    let during_ticket = ticket.clone();
    let terminal = Arc::new(Mutex::new(None));
    let during_terminal = terminal.clone();
    guard.admit_lifecycle(&request, tick(1)).unwrap();
    let in_callback = guard.clone();
    let port_admission = admission.clone();
    WorkLifecycleTask {
        request: Some(request),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
        completion: Some(Box::new(move |completion| {
            assert_eq!(in_callback.state.lock().unwrap().callbacks, 0);
            assert!(in_callback.execution_reserved());
            assert!(!in_callback.callbacks_drained());
            assert!(during_ticket.lock().unwrap().try_take().unwrap().is_none());
            assert_eq!(
                port_admission.pending(),
                Some(1),
                "terminal delivery still blocks global zero"
            );
            let WorkBrowserResourceEvent::LeaseEnded(ended) =
                rows.settle_at(completion, tick(1)).unwrap()
            else {
                panic!("exact original lease ended");
            };
            *during_terminal.lock().unwrap() = Some(ended);
            let next = rows
                .acquire(
                    in_callback.resource(),
                    ContextRunId::generate(),
                    tick(1),
                    tick(1000),
                )
                .unwrap();
            assert_eq!(
                in_callback.admit_lifecycle(&next, tick(1)),
                Err(ContextPortFailure::Stale)
            );
            assert!(!in_callback.acquisition_current(next.lease().unwrap(), tick(1)));
            let _ = rows
                .dispatch_refused(next, ContextPortFailure::Stale)
                .unwrap();
            assert_eq!(
                rows.phase(in_callback.resource()).unwrap(),
                zephium_agentic::WorkBrowserResourcePhase::Retained
            );
        })),
    }
    .complete(drained());
    assert_eq!(admission.pending(), Some(0));
    assert!(guard.is_healthy());
    assert!(!guard.execution_reserved());
    let receipt = ticket.lock().unwrap().try_take().unwrap().unwrap();
    let proof = terminal
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .join_delivery(receipt)
        .unwrap();
    assert_eq!(proof.lease(), &lease);
    assert!(!admission.work_is_absent());
    admission.verify_native_shutdown(zero());
    assert!(!admission.state.lock().unwrap().native_shutdown_verified);
    assert_eq!(
        admission.retire_for_successor(),
        Err(ContextPortFailure::ProfileBusy)
    );
}

#[test]
fn enqueue_then_block_keeps_delivery_pending_and_successor_closed_until_physical_return() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (request, mut ticket) = rows.revoke_with_delivery(&lease).unwrap();
    guard.admit_lifecycle(&request, tick(1)).unwrap();
    let rows = Arc::new(Mutex::new(rows));
    let during_rows = rows.clone();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    let task = WorkLifecycleTask {
        request: Some(request),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
        completion: Some(Box::new(move |completion| {
            let WorkBrowserResourceEvent::LeaseEnded(ended) = during_rows
                .lock()
                .unwrap()
                .settle_at(completion, tick(2))
                .unwrap()
            else {
                panic!("core receipt precedes physical return");
            };
            entered_tx.send(ended).unwrap();
            resume_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        })),
    };
    let running = std::thread::spawn(move || task.complete(drained()));
    let ended = entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(ticket.try_take().unwrap().is_none());
    assert_eq!(admission.pending(), Some(1));
    assert!(guard.execution_reserved());
    assert!(!guard.callbacks_drained());
    let next = rows
        .lock()
        .unwrap()
        .acquire(
            guard.resource(),
            ContextRunId::generate(),
            tick(3),
            tick(1_000_000),
        )
        .unwrap();
    assert_eq!(
        guard.admit_lifecycle(&next, tick(3)),
        Err(ContextPortFailure::Stale)
    );
    assert!(!guard.acquisition_current(next.lease().unwrap(), tick(3)));
    let _ = rows
        .lock()
        .unwrap()
        .dispatch_refused(next, ContextPortFailure::Stale)
        .unwrap();
    admission.verify_native_shutdown(zero());
    assert!(!admission.state.lock().unwrap().native_shutdown_verified);
    assert_eq!(
        admission.retire_for_successor(),
        Err(ContextPortFailure::ProfileBusy)
    );
    resume_tx.send(()).unwrap();
    running.join().unwrap();
    let proof = ended
        .join_delivery(ticket.try_take().unwrap().unwrap())
        .unwrap();
    assert_eq!(proof.lease(), &lease);
    assert_eq!(admission.pending(), Some(0));
    assert!(guard.callbacks_drained());
    assert!(!guard.execution_reserved());
    let next = rows
        .lock()
        .unwrap()
        .acquire(
            guard.resource(),
            ContextRunId::generate(),
            tick(4),
            tick(1_000_000),
        )
        .unwrap();
    guard.admit_lifecycle(&next, tick(4)).unwrap();
    assert!(guard.acquisition_current(next.lease().unwrap(), tick(4)));
}

#[test]
fn callback_enqueue_then_panic_cannot_publish_delivery_or_admit_successor() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let (request, mut ticket) = rows.revoke_with_delivery(&lease).unwrap();
    guard.admit_lifecycle(&request, tick(1)).unwrap();
    let terminal = Arc::new(Mutex::new(None));
    let during = terminal.clone();
    WorkLifecycleTask {
        request: Some(request),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
        completion: Some(Box::new(move |completion| {
            let WorkBrowserResourceEvent::LeaseEnded(ended) =
                rows.settle_at(completion, tick(2)).unwrap()
            else {
                panic!("lease ended");
            };
            *during.lock().unwrap() = Some(ended);
            panic!("terminal enqueued but receiver did not return normally");
        })),
    }
    .complete(drained());
    let receipt = ticket.try_take().unwrap().unwrap();
    assert!(!receipt.returned());
    assert!(terminal
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .join_delivery(receipt)
        .is_err());
    assert!(!guard.is_healthy());
    assert_eq!(admission.pending(), Some(0));
    assert!(
        guard.callbacks_drained(),
        "physical unwind still permits later destruction"
    );
    assert!(!admission.state.lock().unwrap().invariant_failed);
    assert_eq!(
        admission.retire_for_successor(),
        Err(ContextPortFailure::ProfileBusy)
    );
}

#[test]
fn accepted_discard_or_consumer_loss_cannot_publish_delivery_success() {
    for discard_task in [false, true] {
        let (mut rows, admission, guard) = setup();
        let lease = leased(&mut rows, &guard);
        let (request, ticket) = rows.revoke_with_delivery(&lease).unwrap();
        guard.admit_lifecycle(&request, tick(1)).unwrap();
        let task = WorkLifecycleTask {
            request: Some(request),
            guard: guard.clone(),
            permit: admission.reserve().unwrap(),
            completion: Some(Box::new(move |completion| {
                let _ = rows.settle_at(completion, tick(2)).unwrap();
            })),
        };
        if discard_task {
            drop(task);
            let mut ticket = ticket;
            assert!(!ticket.try_take().unwrap().unwrap().returned());
        } else {
            drop(ticket);
            task.complete(drained());
        }
        assert!(!guard.is_healthy());
        assert_eq!(admission.pending(), Some(0));
        assert!(guard.callbacks_drained());
        assert!(!admission.work_is_absent());
    }
}

#[test]
fn legacy_revoke_without_ticket_keeps_same_physical_delivery_admission_barrier() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let request = rows.revoke(&lease).unwrap();
    guard.admit_lifecycle(&request, tick(1)).unwrap();
    let rows = Arc::new(Mutex::new(rows));
    let during_rows = rows.clone();
    let during_guard = guard.clone();
    WorkLifecycleTask {
        request: Some(request),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
        completion: Some(Box::new(move |completion| {
            let mut rows = during_rows.lock().unwrap();
            assert!(matches!(
                rows.settle_at(completion, tick(2)).unwrap(),
                WorkBrowserResourceEvent::LeaseEnded(_)
            ));
            let next = rows
                .acquire(
                    during_guard.resource(),
                    ContextRunId::generate(),
                    tick(3),
                    tick(1_000_000),
                )
                .unwrap();
            assert_eq!(
                during_guard.admit_lifecycle(&next, tick(3)),
                Err(ContextPortFailure::Stale)
            );
            assert!(!during_guard.acquisition_current(next.lease().unwrap(), tick(3)));
            let _ = rows
                .dispatch_refused(next, ContextPortFailure::Stale)
                .unwrap();
            assert!(during_guard.execution_reserved());
        })),
    }
    .complete(drained());
    assert_eq!(admission.pending(), Some(0));
    assert!(guard.is_healthy());
    assert!(!guard.execution_reserved());
    let next = rows
        .lock()
        .unwrap()
        .acquire(
            guard.resource(),
            ContextRunId::generate(),
            tick(4),
            tick(1_000_000),
        )
        .unwrap();
    guard.admit_lifecycle(&next, tick(4)).unwrap();
    assert!(guard.acquisition_current(next.lease().unwrap(), tick(4)));
}

#[test]
fn post_receipt_shutdown_quarantine_or_permit_uncertainty_cannot_publish_delivery() {
    for failure in 0..3 {
        let (mut rows, admission, guard) = setup();
        let lease = leased(&mut rows, &guard);
        let (request, mut ticket) = rows.revoke_with_delivery(&lease).unwrap();
        guard.admit_lifecycle(&request, tick(1)).unwrap();
        let during_guard = guard.clone();
        let during_admission = admission.clone();
        WorkLifecycleTask {
            request: Some(request),
            guard: guard.clone(),
            permit: admission.reserve().unwrap(),
            completion: Some(Box::new(move |completion| {
                assert!(matches!(
                    rows.settle_at(completion, tick(2)).unwrap(),
                    WorkBrowserResourceEvent::LeaseEnded(_)
                ));
                match failure {
                    0 => during_admission.seal(),
                    1 => during_guard.fail(),
                    _ => during_admission.release(), // Adversarial lost original permit count.
                }
            })),
        }
        .complete(drained());
        assert!(!ticket.try_take().unwrap().unwrap().returned());
        assert!(!guard.is_healthy());
        assert!(!admission.work_is_absent());
        assert_eq!(
            admission.pending(),
            if failure == 2 { None } else { Some(0) }
        );
        assert!(!admission.state.lock().unwrap().native_shutdown_verified);
    }
}

#[test]
fn expired_abandoned_wait_keeps_original_callback_debt_until_late_return() {
    let (mut rows, admission, guard) = setup();
    let resource = guard.resource().clone();
    let lease = leased(&mut rows, &guard);
    let (request, mut ticket) = rows.revoke_with_delivery(&lease).unwrap();
    guard.admit_lifecycle(&request, tick(1)).unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    let task = WorkLifecycleTask {
        request: Some(request),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
        completion: Some(Box::new(move |completion| {
            assert!(matches!(
                rows.settle_at(completion, tick(1_000_001)).unwrap(),
                WorkBrowserResourceEvent::LeaseEnded(_)
            ));
            assert!(rows
                .acquire(
                    &resource,
                    ContextRunId::generate(),
                    tick(1_000_001),
                    tick(1_000_000)
                )
                .is_err());
            entered_tx.send(()).unwrap();
            resume_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        })),
    };
    let running = std::thread::spawn(move || task.complete(drained()));
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(ticket.try_take().unwrap().is_none());
    drop(ticket); // Owner's wait deadline/cancellation does not cancel native debt.
    assert_eq!(admission.pending(), Some(1));
    assert!(guard.execution_reserved());
    assert!(!guard.callbacks_drained());
    resume_tx.send(()).unwrap();
    running.join().unwrap();
    assert_eq!(admission.pending(), Some(0));
    assert!(guard.callbacks_drained());
    assert!(!guard.is_healthy());
    assert!(!admission.work_is_absent());
}

#[test]
fn retained_resource_and_destroy_terminal_block_zero_audit_and_legacy_succession() {
    let (mut rows, admission, guard) = setup();
    let task_request = rows.destroy(guard.resource()).unwrap();
    guard.admit_lifecycle(&task_request, tick(1)).unwrap();
    let during = admission.clone();
    let task = WorkLifecycleTask {
        request: Some(task_request),
        guard,
        permit: admission.reserve().unwrap(),
        completion: Some(Box::new(move |_| {
            during.seal();
            during.verify_native_shutdown(zero());
            assert!(!during.state.lock().unwrap().native_shutdown_verified);
            assert_eq!(
                during.retire_for_successor(),
                Err(ContextPortFailure::ProfileBusy)
            );
            assert!(!during.work_is_absent());
        })),
    };
    task.complete(Outcome::Destroyed);
    assert!(admission.work_is_absent());
    assert_eq!(admission.pending(), Some(0));
    assert_eq!(
        admission.retire_for_successor(),
        Err(ContextPortFailure::ProfileBusy),
        "new exact audit still required"
    );
}

#[test]
fn original_resource_notifications_coalesce_and_prevent_false_destroy_drain() {
    let (_, _, guard) = setup();
    assert!(guard.begin_notification());
    assert!(!guard.begin_notification());
    assert!(!guard.callbacks_drained());
    guard.consume_notification();
    assert!(guard.callbacks_drained());
    guard.consume_notification();
    assert!(!guard.is_healthy());
}

#[test]
fn callback_panic_quarantines_only_exact_resource_and_releases_task_permit() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let request = rows.revoke(&lease).unwrap();
    guard.admit_lifecycle(&request, tick(1)).unwrap();
    WorkLifecycleTask {
        request: Some(request),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
        completion: Some(Box::new(|_| panic!("accepted terminal consumer failed"))),
    }
    .complete(drained());
    assert!(!guard.is_healthy());
    assert_eq!(admission.pending(), Some(0));
    assert!(!admission.state.lock().unwrap().invariant_failed);
}

#[test]
fn resource_notifications_share_the_original_queue_ceiling_and_cannot_reopen_destroyed_owner() {
    let (_, admission, guard) = setup();
    let permits: Vec<_> = (0..MAX_PENDING_NATIVE_CONTEXT_TASKS)
        .map(|_| admission.reserve().unwrap())
        .collect();
    assert!(guard.begin_notification());
    assert!(guard.notification_permit().is_none());
    assert!(!guard.is_healthy());
    assert_eq!(admission.pending(), Some(MAX_PENDING_NATIVE_CONTEXT_TASKS));
    assert!(!guard.state.lock().unwrap().notification_pending);
    drop(permits);
    guard.state.lock().unwrap().phase = Phase::Destroyed;
    assert!(!guard.begin_notification());
}

#[test]
fn foreign_private_incarnation_and_scoped_fault_cannot_substitute_current_resource() {
    let (mut rows, admission, guard) = setup();
    let identity = guard.resource().identity();
    let mut foreign = WorkBrowserResources::new(identity.work(), identity.profile());
    let request = foreign
        .construct_document(
            identity.resource(),
            identity.context(),
            ContextProfileStorageClass::Ephemeral,
            ContextNavigationTarget::parse("https://example.test/substitute").unwrap(),
            tick(0),
        )
        .unwrap();
    let other = Arc::new(WorkResourceGuard::new(&request, &admission));
    other.outcome(&request, Outcome::Constructed);
    let _ = foreign
        .settle_at(request.complete(Outcome::Constructed), tick(0))
        .unwrap();
    let foreign_lease = leased(&mut foreign, &other);
    let foreign_read = foreign.observe_initial(&foreign_lease, tick(1)).unwrap();
    let lease = leased(&mut rows, &guard);
    assert_eq!(
        guard.admit_read(&foreign_read, tick(1)),
        Err(ContextPortFailure::Stale)
    );
    let foreign_revoke = foreign.revoke(&foreign_lease).unwrap();
    assert_eq!(
        guard.admit_lifecycle(&foreign_revoke, tick(1)),
        Err(ContextPortFailure::Stale)
    );
    other.fail();
    assert!(guard.admits(&lease, tick(1)));
    assert!(guard.is_healthy());
    assert!(!admission.state.lock().unwrap().invariant_failed);
    assert!(!Arc::ptr_eq(&guard, &other));
}

#[test]
fn shared_ingress_counts_acquiring_leases_from_distinct_work_registries() {
    let admission = admission();
    let port = port(
        admission.clone(),
        Arc::new(|_| panic!("capacity check must precede dispatch")),
    );
    let mut owners = Vec::new();
    for _ in 0..=zephium_agentic::MAX_EXECUTING_CONTEXTS {
        let (mut rows, construct) = source();
        let guard = Arc::new(WorkResourceGuard::new(&construct, &admission));
        guard.outcome(&construct, Outcome::Constructed);
        let _ = rows
            .settle_at(construct.complete(Outcome::Constructed), tick(0))
            .unwrap();
        admission
            .work
            .lock()
            .unwrap()
            .rows
            .insert(guard.resource().identity().context(), guard.clone());
        owners.push((rows, guard));
    }
    for (rows, guard) in owners
        .iter_mut()
        .take(zephium_agentic::MAX_EXECUTING_CONTEXTS)
    {
        let request = rows
            .acquire(
                guard.resource(),
                ContextRunId::generate(),
                tick(1),
                tick(1_000_000),
            )
            .unwrap();
        guard.admit_lifecycle(&request, tick(1)).unwrap();
        // An unacknowledged acquisition still consumes the original capacity.
    }
    let (rows, guard) = owners.last_mut().unwrap();
    let request = rows
        .acquire(
            guard.resource(),
            ContextRunId::generate(),
            tick(1),
            tick(1_000_000),
        )
        .unwrap();
    assert!(matches!(
        port.work_resource_lifecycle(request, Box::new(|_| panic!("not admitted"))),
        WorkBrowserResourceDispatch::Rejected {
            failure: ContextPortFailure::ResourceExhausted,
            ..
        }
    ));
    assert!(!guard.execution_reserved());
    assert_eq!(admission.pending(), Some(0));
}

#[test]
fn destroy_before_queued_construct_cannot_attest_absence_or_admit_late_allocation() {
    let (mut rows, request) = source();
    let admission = admission();
    let guard = Arc::new(WorkResourceGuard::new(&request, &admission));
    admission
        .work
        .lock()
        .unwrap()
        .rows
        .insert(guard.resource().identity().context(), guard.clone());
    // Construct has published ingress but has not yet called MainThreadDispatch.
    rows.quarantine(guard.resource()).unwrap();
    let destroy = rows.destroy(guard.resource()).unwrap();
    guard.admit_lifecycle(&destroy, tick(1)).unwrap();
    assert!(
        !guard.callbacks_drained(),
        "the original Construct task can still arrive"
    );
    assert!(
        guard.port_open(),
        "shared port admission alone was the insufficient construction guard"
    );
    assert!(!guard.construction_current());
    // The stale construction now refuses without a view/load. Its exact
    // terminal drains the original obligation, not a replacement operation.
    guard.outcome(&request, Outcome::Refused);
    assert!(matches!(
        rows.settle_at(request.complete(Outcome::Refused), tick(1))
            .unwrap(),
        WorkBrowserResourceEvent::DebtSettled(_)
    ));
    assert!(guard.callbacks_drained());
    assert!(guard.destruction_started());
    assert!(!guard.construction_current());
    assert!(!admission.work_is_absent());
    WorkLifecycleTask {
        request: Some(destroy),
        completion: Some(Box::new(move |completion| {
            assert!(matches!(
                rows.settle_at(completion, tick(1)).unwrap(),
                WorkBrowserResourceEvent::Destroyed(_)
            ));
            rows.seal();
            assert!(rows.is_quiescent());
        })),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
    }
    .complete(Outcome::Destroyed);
    assert!(!guard.construction_current());
    assert!(admission.work_is_absent());
    assert_eq!(admission.pending(), Some(0));
}

#[test]
fn never_dispatched_constructor_cannot_remove_an_already_admitted_destruction_owner() {
    let (mut rows, request) = source();
    let admission = admission();
    let guard = Arc::new(WorkResourceGuard::new(&request, &admission));
    admission
        .work
        .lock()
        .unwrap()
        .rows
        .insert(guard.resource().identity().context(), guard.clone());
    let queue = Arc::new(Mutex::new(Vec::<Box<dyn FnOnce() + Send>>::new()));
    let queued = queue.clone();
    *guard.notification_dispatch.lock().unwrap() = Some(Arc::new(move |task| {
        queued.lock().unwrap().push(task);
        true
    }));
    assert!(guard.construction_current());
    rows.quarantine(guard.resource()).unwrap();
    let destroy = rows.destroy(guard.resource()).unwrap();
    guard.admit_lifecycle(&destroy, tick(1)).unwrap();
    // This decision is made under the same ingress mutex as Destroy admission.
    // Returning Construct must retain the cleanup row, not erase its authority.
    admission.work_construction_returned(&guard);
    assert!(!admission.work_is_absent());
    assert!(!guard.callbacks_drained());
    assert!(!guard.construction_current());
    assert!(guard.destruction_started());
    assert_eq!(admission.pending(), Some(1));
    let _ = rows
        .dispatch_refused(request, ContextPortFailure::Shutdown)
        .unwrap();
    queue.lock().unwrap().pop().unwrap()();
    assert!(guard.callbacks_drained());
    WorkLifecycleTask {
        request: Some(destroy),
        completion: Some(Box::new(move |completion| {
            assert!(matches!(
                rows.settle_at(completion, tick(1)).unwrap(),
                WorkBrowserResourceEvent::Destroyed(_)
            ));
            rows.seal();
            assert!(rows.is_quiescent());
        })),
        guard,
        permit: admission.reserve().unwrap(),
    }
    .complete(Outcome::Destroyed);
    assert!(admission.work_is_absent());
    assert_eq!(admission.pending(), Some(0));
}

// Test-only construction of the real move-only native envelopes. The queued
// resource notification remains explicit so adversarial tests can hold/release
// the same callback barrier without running AppKit or a replacement event loop.
impl WorkLifecycleTask {
    pub(crate) fn construction_for_test(
        request: WorkBrowserResourceRequest,
        completion: WorkBrowserResourceCompletionCallback,
        dispatch: MainThreadDispatch,
    ) -> Self {
        let admission = admission();
        let guard = Arc::new(WorkResourceGuard::new(&request, &admission));
        *guard.notification_dispatch.lock().unwrap() = Some(dispatch);
        admission
            .work
            .lock()
            .unwrap()
            .rows
            .insert(guard.resource().identity().context(), guard.clone());
        Self {
            request: Some(request),
            completion: Some(completion),
            guard,
            permit: admission.reserve().unwrap(),
        }
    }
    pub(crate) fn followup_for_test(
        &self,
        request: WorkBrowserResourceRequest,
        completion: WorkBrowserResourceCompletionCallback,
    ) -> Self {
        self.guard.admit_lifecycle(&request, tick(1)).unwrap();
        Self {
            request: Some(request),
            completion: Some(completion),
            guard: self.guard.clone(),
            permit: self.permit.admission.reserve().unwrap(),
        }
    }
    pub(crate) fn closure_for_test(&self) -> impl Fn() -> (bool, Option<usize>) {
        let admission = self.permit.admission.clone();
        move || (admission.work_is_absent(), admission.pending())
    }
}
