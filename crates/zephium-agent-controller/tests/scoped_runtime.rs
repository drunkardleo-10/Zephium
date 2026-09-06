#![cfg(feature = "provider-transport")]

use std::future::Future;
use std::pin::Pin;
use std::sync::{mpsc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use zephium_agent_runtime::*;
use zephium_agentic::*;

static SERIAL: Mutex<()> = Mutex::new(());

fn runtime_test_guard() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn wait_stopped(completion: &AgentRuntimeCompletion) {
    for _ in 0..300 {
        if completion.is_stopped() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(completion.is_stopped());
}

fn spawn_after_true_worker_exit() -> PendingAgentRuntime {
    for _ in 0..100 {
        match PendingAgentRuntime::spawn_suspended(AgentRuntimeConfig::STANDARD) {
            Ok(pending) => return pending,
            Err(RuntimeSpawnError::AlreadyRunning) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => panic!("worker startup: {error:?}"),
        }
    }
    panic!("original worker permit remains held");
}

fn tick(value: u64) -> AgentPolicyInstant {
    AgentPolicyInstant::from_millis(value)
}

fn resource(
    run: ContextRunId,
    profile: u128,
) -> (
    WorkBrowserResources,
    WorkBrowserExecutionLease,
    WorkBrowserLeaseDeliveryProof,
) {
    let mut rows = WorkBrowserResources::new(WorkId::generate(), profile.into());
    let request = rows
        .construct(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            ContextProfileStorageClass::Ephemeral,
            tick(1),
        )
        .unwrap();
    let join = request.resource().clone();
    assert!(matches!(
        rows.settle_at(
            request.complete(WorkBrowserResourceNativeOutcome::Constructed),
            tick(1)
        )
        .unwrap(),
        WorkBrowserResourceEvent::Retained(_)
    ));
    let request = rows.acquire(&join, run, tick(2), tick(99)).unwrap();
    let WorkBrowserResourceEvent::Acquired(lease) = rows
        .settle_at(
            request.complete(WorkBrowserResourceNativeOutcome::Acquired),
            tick(2),
        )
        .unwrap()
    else {
        panic!("exact lease")
    };
    let (mut request, mut ticket) = rows.revoke_with_delivery(&lease).unwrap();
    let delivered = request.take_lease_delivery_completion().unwrap();
    let WorkBrowserResourceEvent::LeaseEnded(ended) = rows
        .settle_at(
            request.complete(WorkBrowserResourceNativeOutcome::Revoked {
                debt: WorkBrowserLeaseNativeDebt::default(),
                resource_retained: true,
            }),
            tick(3),
        )
        .unwrap()
    else {
        panic!("exact retirement")
    };
    assert!(delivered.publish_returned());
    let proof = ended
        .join_delivery(ticket.try_take().unwrap().unwrap())
        .unwrap();
    (rows, lease, proof)
}

fn manifest(
    lease: &WorkBrowserExecutionLease,
    id: AgentRunManifestId,
    calls: u32,
) -> AgentRunManifest {
    manifest_window(lease, id, calls, 1, 100)
}

fn manifest_window(
    lease: &WorkBrowserExecutionLease,
    id: AgentRunManifestId,
    calls: u32,
    issued: u64,
    expires: u64,
) -> AgentRunManifest {
    let profile = lease.resource().identity().profile();
    let origin = SemanticOrigin::parse("https://scope.invalid").unwrap();
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).unwrap();
    let budget = AgentRunBudget::try_new(calls, 1, 1, 1).unwrap();
    AgentRunManifest::try_new(
        id,
        lease.run(),
        AgentRunScope::try_new(
            vec![profile],
            vec![AgentAccountScope::Anonymous],
            vec![origin.clone()],
            SemanticSensitivity::Public,
            effects,
            Vec::new(),
        )
        .unwrap(),
        budget,
        tick(issued),
        tick(expires),
        vec![AgentPlanNodeScope::new(
            AgentPlanNodeId::generate(),
            AgentPlanNodeAuthority::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin],
                SemanticSensitivity::Public,
                effects,
            )
            .unwrap(),
            budget,
            tick(expires - 1),
        )],
    )
    .unwrap()
}

fn policy(manifest: AgentRunManifest) -> AgentRunPolicySettlement {
    let root = manifest.plan_nodes()[0].id();
    let mut supervisor = AgentRunSupervisor::new(
        AgentSupervisorId::new(1).unwrap(),
        AgentDelegationTopology::try_new(&manifest, vec![AgentDelegationSpec::new(root, None)])
            .unwrap(),
    );
    let accounting = AgentRunAccountingMetrics::try_new(&manifest, &supervisor).unwrap();
    let mut progress = AgentRunProgressMetrics::try_new(&manifest, &supervisor).unwrap();
    let actions = AgentRunActionPerformanceMetrics::try_new(&manifest, &supervisor).unwrap();
    let inputs = AgentRunProviderInputMetrics::try_new(&manifest, &supervisor).unwrap();
    let mut audit = AgentAuditLedger::try_new(&manifest, &supervisor).unwrap();
    let mut record = |supervisor: &AgentRunSupervisor, value| {
        let event = audit
            .record_current(
                supervisor,
                root,
                AgentAuditEventId::new(value).unwrap(),
                tick(value),
            )
            .unwrap();
        progress.record_event(event).unwrap();
    };
    record(&supervisor, 1);
    let execution = supervisor
        .start(root, AgentSupervisorAttemptId::new(1).unwrap())
        .unwrap();
    record(&supervisor, 2);
    supervisor
        .complete(execution, AgentSupervisorCompletion::Succeeded)
        .unwrap();
    record(&supervisor, 3);
    let closure = AgentRunMetricClosure::try_close(
        &manifest,
        &supervisor,
        &accounting,
        &progress,
        &actions,
        &inputs,
    )
    .unwrap();
    audit.seal_for_shutdown().unwrap();
    let delivery = audit
        .begin_delivery(
            AgentAuditDeliveryId::new(1).unwrap(),
            MAX_AGENT_AUDIT_DELIVERY_EVENTS,
        )
        .unwrap();
    audit
        .settle_delivery(
            delivery
                .proof()
                .settle(AgentAuditDeliveryOutcome::Committed),
        )
        .unwrap();
    AgentRunPolicy::try_new(
        manifest,
        vec![AgentPlanLeaseBinding::new(
            AgentPlanLeaseId::generate(),
            root,
        )],
    )
    .unwrap()
    .settle_metric_closure(closure, &accounting, audit)
    .unwrap()
}

fn provider() -> AgentProviderShutdownProof {
    let transport =
        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD).unwrap();
    assert!(transport.try_prove_shutdown().is_err());
    transport.seal();
    transport.try_prove_shutdown().unwrap().into()
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    Return,
    Block,
    Panic,
    DropPanic,
    LostClaim,
    NoClaim,
    Mismatch,
    Cancelled,
    Shutdown,
}

struct Controller {
    mode: Mode,
    delivery: WorkBrowserLeaseDeliveryProof,
    policy: AgentRunPolicySettlement,
    provider: AgentProviderShutdownProof,
    entered: mpsc::Sender<()>,
    committed: mpsc::Sender<bool>,
    resume: tokio::sync::oneshot::Receiver<()>,
}

struct FutureWithDrop {
    inner: AgentRuntimeControllerFuture,
    panic_on_drop: bool,
}
impl Future for FutureWithDrop {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        self.inner.as_mut().poll(cx)
    }
}
impl Drop for FutureWithDrop {
    fn drop(&mut self) {
        assert!(!self.panic_on_drop, "injected future drop panic");
    }
}
impl AgentRuntimeScopedController for Controller {
    fn run(self: Box<Self>, mut worker: AgentRuntimeWorker) -> AgentRuntimeControllerFuture {
        assert_eq!(std::thread::current().name(), Some("zephium-agent-runtime"));
        let mode = self.mode;
        Box::pin(FutureWithDrop {
            panic_on_drop: matches!(mode, Mode::DropPanic),
            inner: Box::pin(async move {
                assert!(matches!(
                    worker.next_event().await.unwrap(),
                    AgentRuntimeEvent::RunStarted(_)
                ));
                self.entered.send(()).unwrap();
                self.resume.await.unwrap();
                if matches!(mode, Mode::NoClaim) {
                    self.committed.send(false).unwrap();
                    return;
                }
                let class = match mode {
                    Mode::Cancelled => {
                        assert!(matches!(
                            worker.next_event().await.unwrap(),
                            AgentRuntimeEvent::CancellationRequested
                        ));
                        AgentRuntimeControllerTerminalClass::Cancelled
                    }
                    Mode::Shutdown => {
                        assert!(matches!(
                            worker.next_event().await.unwrap(),
                            AgentRuntimeEvent::ShutdownRequested
                        ));
                        AgentRuntimeControllerTerminalClass::Shutdown
                    }
                    _ => AgentRuntimeControllerTerminalClass::Ordinary,
                };
                assert!(matches!(
                    worker.try_claim_controller_terminal(class).await,
                    Err(AgentRuntimeControllerTerminalRefusal::Scope)
                ));
                let claim = match worker.try_claim_scoped_terminal(class).await {
                    Ok(claim) => claim,
                    Err(_) => {
                        self.committed.send(false).unwrap();
                        return;
                    }
                };
                if matches!(mode, Mode::LostClaim) {
                    drop(claim);
                    self.committed.send(false).unwrap();
                    return;
                }
                let expected_lease = self.delivery.lease().clone();
                match claim.commit(self.delivery, self.policy, self.provider) {
                    Ok(()) => assert!(!matches!(mode, Mode::Mismatch)),
                    Err(refusal) => {
                        assert!(matches!(mode, Mode::Mismatch));
                        let (_claim, delivery, policy, _provider) = refusal.into_parts();
                        assert_eq!(delivery.lease(), &expected_lease);
                        assert_eq!(policy, self.policy);
                        self.committed.send(false).unwrap();
                        return;
                    }
                }
                self.committed.send(true).unwrap();
                match mode {
                    Mode::Block => std::future::pending::<()>().await,
                    Mode::Panic => panic!("injected post-claim panic"),
                    _ => {}
                }
            }),
        })
    }
}

struct Running {
    handle: AgentRuntimeHandle,
    completion: AgentRuntimeCompletion,
    lifecycle: AgentRuntimeScopedLifecycle,
    committed: mpsc::Receiver<bool>,
    resume: tokio::sync::oneshot::Sender<()>,
}
fn start(
    binding: AgentRuntimeScopedBinding,
    delivery: WorkBrowserLeaseDeliveryProof,
    policy: AgentRunPolicySettlement,
    mode: Mode,
) -> Running {
    let (entered, admission) = mpsc::channel();
    let (committed, receipt) = mpsc::channel();
    let (resume, wait) = tokio::sync::oneshot::channel();
    let pending = PendingScopedAgentRuntime::spawn_suspended(
        AgentRuntimeConfig::STANDARD,
        binding,
        Box::new(Controller {
            mode,
            delivery,
            policy,
            provider: provider(),
            entered,
            committed,
            resume: wait,
        }),
    )
    .unwrap();
    let (handle, completion, lifecycle) = pending.bind().into_parts();
    handle.start_run().unwrap();
    admission.recv_timeout(Duration::from_secs(2)).unwrap();
    Running {
        handle,
        completion,
        lifecycle,
        committed: receipt,
        resume,
    }
}

#[test]
fn scoped_actual_worker_closure_requires_normal_return_drop_and_original_join() {
    let _serial = runtime_test_guard();
    for mode in [
        Mode::Return,
        Mode::Block,
        Mode::Panic,
        Mode::DropPanic,
        Mode::LostClaim,
        Mode::NoClaim,
    ] {
        let (mut rows, lease, delivery) = resource(ContextRunId::generate(), 31);
        let manifest = manifest(&lease, AgentRunManifestId::generate(), 1);
        let stamp = AgentRunPolicySettlementBinding::new(&manifest);
        let binding = AgentRuntimeScopedBinding::try_new(lease.clone(), &manifest).unwrap();
        let settled = policy(manifest);
        let running = start(binding, delivery, settled, mode);
        running.resume.send(()).unwrap();
        let committed = running
            .committed
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or_else(|error| {
                panic!(
                    "{mode:?}: {error:?}, status={:?}, stopped={}",
                    running.handle.status(),
                    running.completion.is_stopped()
                )
            });
        assert_eq!(committed, !matches!(mode, Mode::LostClaim | Mode::NoClaim));
        assert_eq!(
            rows.phase(lease.resource()).unwrap(),
            WorkBrowserResourcePhase::Retained
        );
        // The native lease is already retired. Actual actor/worker ownership
        // still blocks any second process executor until its exact join.
        assert!(matches!(
            PendingAgentRuntime::spawn_suspended(AgentRuntimeConfig::STANDARD),
            Err(RuntimeSpawnError::AlreadyRunning)
        ));
        if matches!(mode, Mode::Block) {
            assert!(!running.completion.is_stopped());
        } else {
            wait_stopped(&running.completion);
        }
        let outcome = running
            .lifecycle
            .drain_until(Instant::now() + Duration::from_millis(50));
        match (mode, outcome) {
            (Mode::Return, AgentRuntimeScopedDrain::Drained(proof)) => {
                assert!(proof.matches_runtime(&running.handle));
                assert_eq!(proof.ticket().get(), 1);
                assert_eq!(proof.lease(), &lease);
                assert!(stamp.matches(proof.policy()));
            }
            (Mode::Return, _) => panic!("normally joined scoped closure"),
            (_, AgentRuntimeScopedDrain::Unproven) => {}
            (_, _) => panic!("{mode:?} cannot establish drain"),
        }
        wait_stopped(&running.completion);
        drop(spawn_after_true_worker_exit());
        assert_eq!(
            rows.phase(lease.resource()).unwrap(),
            WorkBrowserResourcePhase::Retained
        );
        assert!(running.handle.start_run().is_err());
        // Only the retained original resource owner can destroy the actual row.
        let request = rows.destroy(lease.resource()).unwrap();
        let event = rows
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Destroyed),
                tick(4),
            )
            .unwrap();
        assert!(matches!(event, WorkBrowserResourceEvent::Destroyed(_)));
        rows.seal();
        assert!(rows.is_quiescent());
    }
}

#[test]
fn scoped_lost_lifecycle_reaps_worker_but_preserves_original_resource_owner() {
    let _serial = runtime_test_guard();
    let (mut rows, lease, delivery) = resource(ContextRunId::generate(), 31);
    let manifest = manifest(&lease, AgentRunManifestId::generate(), 1);
    let binding = AgentRuntimeScopedBinding::try_new(lease.clone(), &manifest).unwrap();
    let running = start(binding, delivery, policy(manifest), Mode::Block);
    running.resume.send(()).unwrap();
    assert!(running
        .committed
        .recv_timeout(Duration::from_secs(2))
        .unwrap());
    assert!(!running.completion.is_stopped());
    // Lost original lifecycle ownership cannot yield its move-only proof. It
    // must still hand the exact running worker to the existing bounded reaper.
    drop(running.lifecycle);
    wait_stopped(&running.completion);
    drop(spawn_after_true_worker_exit());
    assert!(running.handle.start_run().is_err());
    assert_eq!(
        rows.phase(lease.resource()).unwrap(),
        WorkBrowserResourcePhase::Retained
    );
    assert!(!rows.is_quiescent());
    let request = rows.destroy(lease.resource()).unwrap();
    assert!(matches!(
        rows.settle_at(
            request.complete(WorkBrowserResourceNativeOutcome::Destroyed),
            tick(4),
        )
        .unwrap(),
        WorkBrowserResourceEvent::Destroyed(_)
    ));
    rows.seal();
    assert!(rows.is_quiescent());
}

#[test]
fn scoped_commit_rejects_foreign_lease_and_same_id_different_manifest_losslessly() {
    let _serial = runtime_test_guard();
    for foreign_lease in [false, true] {
        let run = ContextRunId::generate();
        let (_rows, lease, delivery) = resource(run, 31);
        let approved = manifest(&lease, AgentRunManifestId::generate(), 1);
        let binding = AgentRuntimeScopedBinding::try_new(lease.clone(), &approved).unwrap();
        let (delivery, settlement) = if foreign_lease {
            let (_foreign_rows, _foreign_lease, foreign_delivery) = resource(run, 31);
            (foreign_delivery, policy(approved))
        } else {
            let substituted = manifest(&lease, approved.id(), 2);
            assert_eq!(approved.id(), substituted.id());
            let settlement = policy(substituted);
            assert!(!AgentRunPolicySettlementBinding::new(&approved).matches(settlement));
            (delivery, settlement)
        };
        let running = start(binding, delivery, settlement, Mode::Mismatch);
        running.resume.send(()).unwrap();
        assert!(!running
            .committed
            .recv_timeout(Duration::from_secs(2))
            .unwrap());
        wait_stopped(&running.completion);
        assert!(matches!(
            running
                .lifecycle
                .drain_until(Instant::now() + Duration::from_secs(1)),
            AgentRuntimeScopedDrain::Unproven
        ));
    }
}

#[test]
fn scoped_cancel_and_shutdown_require_their_exact_control_class() {
    let _serial = runtime_test_guard();
    for mode in [Mode::Cancelled, Mode::Shutdown, Mode::Return] {
        let (_rows, lease, delivery) = resource(ContextRunId::generate(), 31);
        let manifest = manifest(&lease, AgentRunManifestId::generate(), 1);
        let binding = AgentRuntimeScopedBinding::try_new(lease, &manifest).unwrap();
        let settlement = policy(manifest);
        let running = start(binding, delivery, settlement, mode);
        let (drain_thread, lifecycle) = if matches!(mode, Mode::Shutdown) {
            (
                Some(std::thread::spawn(move || {
                    running
                        .lifecycle
                        .drain_until(Instant::now() + Duration::from_secs(2))
                })),
                None,
            )
        } else {
            running.handle.cancel_and_seal();
            (None, Some(running.lifecycle))
        };
        running.resume.send(()).unwrap();
        assert_eq!(
            running
                .committed
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            !matches!(mode, Mode::Return)
        );
        wait_stopped(&running.completion);
        let outcome = match drain_thread {
            Some(thread) => thread.join().unwrap(),
            None => lifecycle
                .unwrap()
                .drain_until(Instant::now() + Duration::from_secs(1)),
        };
        assert_eq!(
            matches!(outcome, AgentRuntimeScopedDrain::Drained(_)),
            !matches!(mode, Mode::Return)
        );
    }
}

#[test]
fn scoped_binding_preserves_run_profile_and_original_deadline_intersection() {
    let run = ContextRunId::generate();
    let (_rows, lease, _delivery) = resource(run, 31);
    let (_other_rows, other_run, _other_delivery) = resource(ContextRunId::generate(), 31);
    let (_profile_rows, other_profile, _profile_delivery) = resource(run, 32);
    for (manifest, expected) in [
        (
            manifest(&other_run, AgentRunManifestId::generate(), 1),
            AgentRuntimeScopedBindingRefusal::Run,
        ),
        (
            manifest(&other_profile, AgentRunManifestId::generate(), 1),
            AgentRuntimeScopedBindingRefusal::Profile,
        ),
        (
            manifest_window(&lease, AgentRunManifestId::generate(), 1, 1, 98),
            AgentRuntimeScopedBindingRefusal::Deadline,
        ),
        (
            manifest_window(&lease, AgentRunManifestId::generate(), 1, 100, 200),
            AgentRuntimeScopedBindingRefusal::Deadline,
        ),
    ] {
        assert_eq!(
            AgentRuntimeScopedBinding::try_new(lease.clone(), &manifest).err(),
            Some(expected)
        );
    }
    let original = manifest(&lease, AgentRunManifestId::generate(), 1);
    let stamp = AgentRunPolicySettlementBinding::new(&original);
    let binding = AgentRuntimeScopedBinding::try_new(lease, &original).unwrap();
    // Approval remains move-only and available to its actual policy, not copied
    // into the runtime just to make the two closure operands coexist.
    assert!(stamp.matches(policy(original)));
    assert!(format!("{binding:?}").contains("redacted"));
    assert!(!format!("{stamp:?}").contains("scope.invalid"));
}

#[test]
fn scoped_proofs_and_old_controls_cannot_cross_distinct_worker_allocations() {
    let _serial = runtime_test_guard();
    let mut previous: Option<(AgentRuntimeHandle, AgentRuntimeScopedDrained)> = None;
    for _ in 0..2 {
        let (rows, lease, delivery) = resource(ContextRunId::generate(), 31);
        let manifest = manifest(&lease, AgentRunManifestId::generate(), 1);
        let binding = AgentRuntimeScopedBinding::try_new(lease.clone(), &manifest).unwrap();
        let running = start(binding, delivery, policy(manifest), Mode::Return);
        if let Some((old_handle, old_proof)) = &previous {
            assert!(!old_proof.matches_runtime(&running.handle));
            old_handle.cancel_and_seal();
            assert!(!running.handle.status().cancelled());
        }
        running.resume.send(()).unwrap();
        assert!(running
            .committed
            .recv_timeout(Duration::from_secs(2))
            .unwrap());
        wait_stopped(&running.completion);
        let AgentRuntimeScopedDrain::Drained(proof) = running
            .lifecycle
            .drain_until(Instant::now() + Duration::from_secs(1))
        else {
            panic!("exact worker closure")
        };
        assert_eq!(proof.ticket().get(), 1); // Numeric ticket reuse grants nothing.
        assert_eq!(
            rows.phase(lease.resource()).unwrap(),
            WorkBrowserResourcePhase::Retained
        );
        if let Some((old_handle, _)) = &previous {
            assert!(!proof.matches_runtime(old_handle));
        }
        previous = Some((running.handle, proof));
    }
}
