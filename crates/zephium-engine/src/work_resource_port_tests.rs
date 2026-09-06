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
fn move_only_revocation_receipt_transfers_at_callback_entry_but_global_permit_waits() {
    let (mut rows, admission, guard) = setup();
    let lease = leased(&mut rows, &guard);
    let request = rows.revoke(&lease).unwrap();
    guard.admit_lifecycle(&request, tick(1)).unwrap();
    let in_callback = guard.clone();
    let port_admission = admission.clone();
    WorkLifecycleTask {
        request: Some(request),
        guard: guard.clone(),
        permit: admission.reserve().unwrap(),
        completion: Some(Box::new(move |completion| {
            assert_eq!(in_callback.state.lock().unwrap().callbacks, 0);
            assert!(!in_callback.execution_reserved());
            assert_eq!(
                port_admission.pending(),
                Some(1),
                "terminal delivery still blocks global zero"
            );
            assert!(matches!(
                rows.settle_at(completion, tick(1)).unwrap(),
                WorkBrowserResourceEvent::LeaseEnded(_)
            ));
            let next = rows
                .acquire(
                    in_callback.resource(),
                    ContextRunId::generate(),
                    tick(1),
                    tick(1000),
                )
                .unwrap();
            in_callback.admit_lifecycle(&next, tick(1)).unwrap();
            assert!(in_callback.acquisition_current(next.lease().unwrap(), tick(1)));
        })),
    }
    .complete(drained());
    assert_eq!(admission.pending(), Some(0));
    assert!(guard.is_healthy());
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
