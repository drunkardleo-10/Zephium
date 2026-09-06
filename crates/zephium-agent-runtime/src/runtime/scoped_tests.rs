//! Default-graph proof of scoped callback ingress. No transport/test proof mint.

use std::future::{poll_fn, Future};
use std::sync::mpsc;
use std::task::Poll;

use zephium_agentic::*;

use super::tests::{runtime_test_guard, wait_stopped};
use super::*;

fn tick(value: u64) -> AgentPolicyInstant {
    AgentPolicyInstant::from_millis(value)
}

fn acquired_scope() -> (
    WorkBrowserResources,
    WorkBrowserExecutionLease,
    AgentRuntimeScopedBinding,
) {
    let mut rows = WorkBrowserResources::new(WorkId::generate(), 31.into());
    let request = rows
        .construct(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            ContextProfileStorageClass::Ephemeral,
            tick(1),
        )
        .unwrap();
    let resource = request.resource().clone();
    assert!(matches!(
        rows.settle_at(
            request.complete(WorkBrowserResourceNativeOutcome::Constructed),
            tick(1),
        )
        .unwrap(),
        WorkBrowserResourceEvent::Retained(_)
    ));
    let request = rows
        .acquire(&resource, ContextRunId::generate(), tick(2), tick(99))
        .unwrap();
    let WorkBrowserResourceEvent::Acquired(lease) = rows
        .settle_at(
            request.complete(WorkBrowserResourceNativeOutcome::Acquired),
            tick(2),
        )
        .unwrap()
    else {
        panic!("exact acquisition")
    };
    let profile = resource.identity().profile();
    let origin = SemanticOrigin::parse("https://scope.invalid").unwrap();
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).unwrap();
    let budget = AgentRunBudget::try_new(1, 1, 1, 1).unwrap();
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
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
        tick(1),
        tick(100),
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
            tick(99),
        )],
    )
    .unwrap();
    let binding = AgentRuntimeScopedBinding::try_new(lease.clone(), &manifest).unwrap();
    (rows, lease, binding)
}

struct IngressController {
    entered: mpsc::Sender<()>,
    polled_pending: mpsc::Sender<()>,
    claimed: mpsc::Sender<Result<(), AgentRuntimeControllerTerminalRefusal>>,
    resume: tokio::sync::oneshot::Receiver<()>,
}

impl AgentRuntimeScopedController for IngressController {
    fn run(self: Box<Self>, mut worker: AgentRuntimeWorker) -> AgentRuntimeControllerFuture {
        Box::pin(async move {
            assert!(matches!(
                worker.next_event().await.unwrap(),
                AgentRuntimeEvent::RunStarted(_)
            ));
            self.entered.send(()).unwrap();
            self.resume.await.unwrap();
            assert!(matches!(
                worker
                    .try_claim_controller_terminal(AgentRuntimeControllerTerminalClass::Ordinary)
                    .await,
                Err(AgentRuntimeControllerTerminalRefusal::Scope)
            ));
            let mut claim = Box::pin(
                worker.try_claim_scoped_terminal(AgentRuntimeControllerTerminalClass::Ordinary),
            );
            poll_fn(|cx| {
                assert!(claim.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            self.polled_pending.send(()).unwrap();
            // Even an obtained claim is not closure: no native/provider/policy
            // operands are manufactured by this callback-ingress fixture.
            let result = claim.await.map(drop);
            self.claimed.send(result).unwrap();
        })
    }
}

#[test]
fn scoped_claim_waits_accepted_callback_and_rejects_queued_terminal_debt() {
    let _serial = runtime_test_guard();
    for queued in [false, true] {
        let (mut rows, lease, binding) = acquired_scope();
        let (entered, admission) = mpsc::channel();
        let (polled_pending, pending_observed) = mpsc::channel();
        let (claimed, claim_result) = mpsc::channel();
        let (resume, wait) = tokio::sync::oneshot::channel();
        let pending = PendingScopedAgentRuntime::spawn_suspended(
            AgentRuntimeConfig::STANDARD,
            binding,
            Box::new(IngressController {
                entered,
                polled_pending,
                claimed,
                resume: wait,
            }),
        )
        .unwrap();
        let (handle, completion, lifecycle) = pending.bind().into_parts();
        handle.start_run().unwrap();
        admission.recv_timeout(Duration::from_secs(2)).unwrap();
        let callback = handle.inner.mailbox.hold_test_callback().unwrap();
        if queued {
            handle.inner.mailbox.publish_test_terminal().unwrap();
        }
        resume.send(()).unwrap();
        pending_observed
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_eq!(claim_result.try_recv(), Err(mpsc::TryRecvError::Empty));
        assert!(!completion.is_stopped());
        drop(callback);
        assert_eq!(
            claim_result.recv_timeout(Duration::from_secs(2)).unwrap(),
            if queued {
                Err(AgentRuntimeControllerTerminalRefusal::TerminalDebt)
            } else {
                Ok(())
            }
        );
        wait_stopped(&completion);
        assert!(matches!(
            lifecycle.drain_until(Instant::now() + Duration::from_secs(1)),
            AgentRuntimeScopedDrain::Unproven
        ));
        assert!(rows.admits_lease(&lease, tick(3)).is_ok());
        assert!(!rows.is_quiescent());
        let request = rows.revoke(&lease).unwrap();
        assert!(matches!(
            rows.settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Revoked {
                    debt: WorkBrowserLeaseNativeDebt::default(),
                    resource_retained: true,
                }),
                tick(3),
            )
            .unwrap(),
            WorkBrowserResourceEvent::LeaseEnded(_)
        ));
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
}
