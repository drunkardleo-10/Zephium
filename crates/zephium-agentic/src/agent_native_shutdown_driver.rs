//! Terminal imperative driver for the native browser shutdown protocol.
//!
//! The functional coordinator owns protocol correctness. This driver supplies
//! the bounded orchestration needed by an application lifecycle: it consumes
//! an already-admitted shutdown coordinator, atomically seals the native port,
//! waits only through a caller-owned event source, retries read-only audits at
//! a fixed bounded cadence, and returns only the coordinator-closed zero proof.
//! It creates no worker, timer, channel, page, native object, or background
//! task. A Work or Shell composition remains responsible for its event queue
//! and for settling run, provider, policy, and durable-audit ownership before
//! admitting an [`AgentNativeShutdownCoordinator`].

use std::time::{Duration, Instant};

use thiserror::Error;

use crate::{
    AgentBrowserPort, AgentNativeShutdownCoordinator, AgentNativeShutdownError,
    AgentNativeShutdownProof, AgentNativeShutdownStage, ContextNativeEvent, ContextResourceAuditId,
};

/// Delay before the first post-seal native resource-audit retry.
pub const AGENT_NATIVE_SHUTDOWN_RETRY_BASE_MILLIS: u64 = 100;

/// Maximum delay between post-seal native resource-audit retries.
pub const AGENT_NATIVE_SHUTDOWN_RETRY_MAX_MILLIS: u64 = 1_000;

/// One exact result from a trusted, caller-owned native-event wait.
#[must_use]
pub enum AgentNativeShutdownWait {
    /// One native event became available before the requested wake time.
    Event(Box<ContextNativeEvent>),
    /// The requested wake time elapsed without an event.
    Elapsed,
    /// The event source closed before an event or the requested wake time.
    Closed,
}

impl std::fmt::Debug for AgentNativeShutdownWait {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Event(_) => formatter.write_str("AgentNativeShutdownWait::Event([redacted])"),
            Self::Elapsed => formatter.write_str("AgentNativeShutdownWait::Elapsed"),
            Self::Closed => formatter.write_str("AgentNativeShutdownWait::Closed"),
        }
    }
}

/// Trusted blocking event source owned by the concrete application runtime.
///
/// `wait_until` must return [`AgentNativeShutdownWait::Elapsed`] only after the
/// supplied monotonic instant has elapsed. It may wake earlier only with one
/// event or a permanently closed source. The implementation must not dispatch
/// native UI work on the waiting thread.
pub trait AgentNativeShutdownEventSource: Send {
    /// Waits for one exact native event or the caller-selected monotonic wake.
    fn wait_until(&mut self, wake: Instant) -> AgentNativeShutdownWait;
}

/// Closed terminal failure from the bounded native shutdown driver.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentNativeShutdownDriveError {
    /// The caller-owned absolute shutdown deadline elapsed.
    #[error("agent native shutdown deadline elapsed")]
    Deadline,
    /// The native event source closed while one exact audit was outstanding.
    #[error("agent native shutdown event source closed")]
    EventSourceClosed,
    /// A non-audit or wrong-class event remained after logical drain.
    #[error("agent native shutdown received an unexpected event")]
    UnexpectedEvent,
    /// A strictly newer nonzero audit identity could not be represented.
    #[error("agent native shutdown audit identity exhausted")]
    AuditIdentityExhausted,
    /// The fixed native resource-audit attempt ceiling was exhausted.
    #[error("agent native shutdown audit attempts were exhausted")]
    AttemptsExhausted,
    /// The native port or event source violated the exact coordinator protocol.
    #[error("agent native shutdown protocol failed: {0}")]
    Protocol(AgentNativeShutdownError),
}

/// Consumes an admitted coordinator and drives native shutdown to exact zero.
///
/// The caller must first use [`AgentNativeShutdownCoordinator::try_new`] and
/// retain its lossless refusal for continued exact cleanup. `first_audit` is a
/// shell-minted process-local identity. Any retry uses its checked strictly
/// increasing successor. The function performs at most the coordinator's
/// fixed eight total audits and waits between retries with a 100 ms exponential
/// cadence capped at one second. It never sleeps or spins; all blocking belongs
/// to `events`, and every wait is capped by `deadline`.
///
/// A successful return proves only native browser drain. The concrete
/// [`crate::AgentBrowserLifecycle`] must independently settle provider work,
/// durable audit delivery, mutable policy, and the run cancellation tree.
pub fn drive_agent_native_shutdown_until(
    mut coordinator: AgentNativeShutdownCoordinator,
    port: &dyn AgentBrowserPort,
    first_audit: ContextResourceAuditId,
    events: &mut dyn AgentNativeShutdownEventSource,
    deadline: Instant,
) -> Result<AgentNativeShutdownProof, AgentNativeShutdownDriveError> {
    if Instant::now() >= deadline {
        return Err(AgentNativeShutdownDriveError::Deadline);
    }

    coordinator
        .begin_port_seal(first_audit)
        .map_err(AgentNativeShutdownDriveError::Protocol)?;
    let dispatch = port.seal_for_shutdown(first_audit);
    coordinator
        .account_port_seal(first_audit, dispatch)
        .map_err(AgentNativeShutdownDriveError::Protocol)?;
    let mut last_audit = first_audit;

    loop {
        if Instant::now() >= deadline {
            return Err(AgentNativeShutdownDriveError::Deadline);
        }
        match coordinator.status().stage() {
            AgentNativeShutdownStage::ShutdownAuditPending => {
                let event = wait_for_event(events, deadline)?;
                let ContextNativeEvent::ShutdownAuditSettled(settlement) = event else {
                    return Err(AgentNativeShutdownDriveError::UnexpectedEvent);
                };
                coordinator
                    .settle_shutdown_audit(settlement)
                    .map_err(AgentNativeShutdownDriveError::Protocol)?;
            }
            AgentNativeShutdownStage::ResourceAuditPending => {
                let event = wait_for_event(events, deadline)?;
                let ContextNativeEvent::ResourceAuditSettled(settlement) = event else {
                    return Err(AgentNativeShutdownDriveError::UnexpectedEvent);
                };
                coordinator
                    .settle_resource_audit(settlement)
                    .map_err(AgentNativeShutdownDriveError::Protocol)?;
            }
            AgentNativeShutdownStage::ResourceAuditRequired => {
                let next = last_audit
                    .get()
                    .checked_add(1)
                    .and_then(ContextResourceAuditId::new)
                    .ok_or(AgentNativeShutdownDriveError::AuditIdentityExhausted)?;
                wait_for_retry(events, coordinator.status().attempts(), deadline)?;
                if Instant::now() >= deadline {
                    return Err(AgentNativeShutdownDriveError::Deadline);
                }
                coordinator
                    .begin_resource_audit(next)
                    .map_err(AgentNativeShutdownDriveError::Protocol)?;
                let dispatch = port.audit_resources(next);
                coordinator
                    .account_resource_audit(next, dispatch)
                    .map_err(AgentNativeShutdownDriveError::Protocol)?;
                last_audit = next;
            }
            AgentNativeShutdownStage::ZeroProven => {
                return coordinator.finish().map_err(|_| {
                    AgentNativeShutdownDriveError::Protocol(AgentNativeShutdownError::Stage)
                });
            }
            AgentNativeShutdownStage::Exhausted => {
                return Err(AgentNativeShutdownDriveError::AttemptsExhausted);
            }
            AgentNativeShutdownStage::ReadyToSeal
            | AgentNativeShutdownStage::SealDispatchPending
            | AgentNativeShutdownStage::ResourceAuditDispatchPending => {
                return Err(AgentNativeShutdownDriveError::Protocol(
                    AgentNativeShutdownError::Stage,
                ));
            }
        }
    }
}

fn wait_for_event(
    events: &mut dyn AgentNativeShutdownEventSource,
    deadline: Instant,
) -> Result<ContextNativeEvent, AgentNativeShutdownDriveError> {
    match events.wait_until(deadline) {
        AgentNativeShutdownWait::Event(event) => {
            if Instant::now() >= deadline {
                Err(AgentNativeShutdownDriveError::Deadline)
            } else {
                Ok(*event)
            }
        }
        AgentNativeShutdownWait::Elapsed => Err(AgentNativeShutdownDriveError::Deadline),
        AgentNativeShutdownWait::Closed => Err(AgentNativeShutdownDriveError::EventSourceClosed),
    }
}

fn wait_for_retry(
    events: &mut dyn AgentNativeShutdownEventSource,
    attempts: u8,
    deadline: Instant,
) -> Result<(), AgentNativeShutdownDriveError> {
    let now = Instant::now();
    if now >= deadline {
        return Err(AgentNativeShutdownDriveError::Deadline);
    }
    let retry_wake = now
        .checked_add(retry_delay(attempts))
        .unwrap_or(deadline)
        .min(deadline);
    match events.wait_until(retry_wake) {
        AgentNativeShutdownWait::Event(_) => Err(AgentNativeShutdownDriveError::UnexpectedEvent),
        AgentNativeShutdownWait::Elapsed if retry_wake == deadline => {
            Err(AgentNativeShutdownDriveError::Deadline)
        }
        AgentNativeShutdownWait::Elapsed => Ok(()),
        AgentNativeShutdownWait::Closed => Err(AgentNativeShutdownDriveError::EventSourceClosed),
    }
}

fn retry_delay(attempts: u8) -> Duration {
    let shift = u32::from(attempts.saturating_sub(1).min(6));
    Duration::from_millis(
        AGENT_NATIVE_SHUTDOWN_RETRY_BASE_MILLIS
            .saturating_mul(1_u64 << shift)
            .min(AGENT_NATIVE_SHUTDOWN_RETRY_MAX_MILLIS),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::{
        AgentNativeShutdownResources, ContextCookieTransferRegistry, ContextDispatch,
        ContextNativeRequest, ContextNativeResourceCounts, ContextNativeResourceSnapshot,
        ContextPortFailure, ContextResourceAuditSettlement, ContextShutdownAuditSettlement,
        ContextShutdownDispatch, SemanticActionExecutionCoordinator,
        SemanticActionNativeCompletion, SemanticActionNativeRequest,
        SemanticActionSettlementCoordinator, SemanticRuntimeInvocation,
        SemanticScreenshotCoordinator, SemanticScreenshotNativeCompletion,
        SemanticScreenshotNativeRequest,
    };
    use crate::{ContextProfileLeaseRegistry, ContextRegistry};

    #[derive(Clone, Copy)]
    enum AuditStep {
        ScheduledZero,
        ScheduledNonzero,
        ScheduledWrongIdentity,
        Rejected,
    }

    struct ScriptedPort {
        steps: Mutex<VecDeque<AuditStep>>,
        events: Arc<Mutex<VecDeque<ContextNativeEvent>>>,
        calls: AtomicUsize,
    }

    impl ScriptedPort {
        fn new(
            steps: impl IntoIterator<Item = AuditStep>,
            events: Arc<Mutex<VecDeque<ContextNativeEvent>>>,
        ) -> Self {
            Self {
                steps: Mutex::new(steps.into_iter().collect()),
                events,
                calls: AtomicUsize::new(0),
            }
        }

        fn next(&self) -> AuditStep {
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.steps
                .lock()
                .expect("script lock")
                .pop_front()
                .expect("scripted audit step")
        }

        fn push(&self, event: ContextNativeEvent) {
            self.events.lock().expect("event lock").push_back(event);
        }
    }

    impl AgentBrowserPort for ScriptedPort {
        fn dispatch(&self, _request: ContextNativeRequest) -> ContextDispatch {
            ContextDispatch::Unsupported
        }

        fn transfer_cookies(
            &self,
            _request: crate::ContextCookieTransferRequest,
        ) -> ContextDispatch {
            ContextDispatch::Unsupported
        }

        fn audit_resources(&self, audit: ContextResourceAuditId) -> ContextDispatch {
            match self.next() {
                AuditStep::ScheduledZero => {
                    self.push(ContextNativeEvent::ResourceAuditSettled(
                        ContextResourceAuditSettlement::new(audit, Ok(zero_snapshot())),
                    ));
                    ContextDispatch::Scheduled
                }
                AuditStep::ScheduledNonzero => {
                    self.push(ContextNativeEvent::ResourceAuditSettled(
                        ContextResourceAuditSettlement::new(audit, Ok(nonzero_snapshot())),
                    ));
                    ContextDispatch::Scheduled
                }
                AuditStep::ScheduledWrongIdentity => {
                    let wrong = ContextResourceAuditId::new(audit.get() + 1).expect("wrong audit");
                    self.push(ContextNativeEvent::ResourceAuditSettled(
                        ContextResourceAuditSettlement::new(wrong, Ok(zero_snapshot())),
                    ));
                    ContextDispatch::Scheduled
                }
                AuditStep::Rejected => {
                    ContextDispatch::Rejected(ContextPortFailure::ResourceExhausted)
                }
            }
        }

        fn seal_for_shutdown(&self, audit: ContextResourceAuditId) -> ContextShutdownDispatch {
            match self.next() {
                AuditStep::ScheduledZero => {
                    self.push(ContextNativeEvent::ShutdownAuditSettled(
                        ContextShutdownAuditSettlement::new(audit, Ok(zero_snapshot())),
                    ));
                    ContextShutdownDispatch::AuditScheduled
                }
                AuditStep::ScheduledNonzero => {
                    self.push(ContextNativeEvent::ShutdownAuditSettled(
                        ContextShutdownAuditSettlement::new(audit, Ok(nonzero_snapshot())),
                    ));
                    ContextShutdownDispatch::AuditScheduled
                }
                AuditStep::ScheduledWrongIdentity => {
                    let wrong = ContextResourceAuditId::new(audit.get() + 1).expect("wrong audit");
                    self.push(ContextNativeEvent::ShutdownAuditSettled(
                        ContextShutdownAuditSettlement::new(wrong, Ok(zero_snapshot())),
                    ));
                    ContextShutdownDispatch::AuditScheduled
                }
                AuditStep::Rejected => ContextShutdownDispatch::SealedWithoutAudit(
                    ContextPortFailure::ResourceExhausted,
                ),
            }
        }

        fn invoke_semantic(&self, _invocation: SemanticRuntimeInvocation) -> ContextDispatch {
            ContextDispatch::Unsupported
        }

        fn execute_semantic_action(
            &self,
            _request: SemanticActionNativeRequest,
            _completion: SemanticActionNativeCompletion,
        ) -> ContextDispatch {
            ContextDispatch::Unsupported
        }

        fn capture_semantic_screenshot(
            &self,
            _request: SemanticScreenshotNativeRequest,
            _completion: SemanticScreenshotNativeCompletion,
        ) -> ContextDispatch {
            ContextDispatch::Unsupported
        }
    }

    struct QueueEvents {
        events: Arc<Mutex<VecDeque<ContextNativeEvent>>>,
        close_when_empty: bool,
    }

    impl AgentNativeShutdownEventSource for QueueEvents {
        fn wait_until(&mut self, _wake: Instant) -> AgentNativeShutdownWait {
            if let Some(event) = self.events.lock().expect("event lock").pop_front() {
                AgentNativeShutdownWait::Event(Box::new(event))
            } else if self.close_when_empty {
                AgentNativeShutdownWait::Closed
            } else {
                AgentNativeShutdownWait::Elapsed
            }
        }
    }

    fn coordinator() -> AgentNativeShutdownCoordinator {
        let mut contexts = ContextRegistry::new();
        let mut profile_leases = ContextProfileLeaseRegistry::new();
        let mut cookie_transfers = ContextCookieTransferRegistry::new();
        let mut action_executions = SemanticActionExecutionCoordinator::new();
        let mut action_settlements = SemanticActionSettlementCoordinator::new();
        let mut screenshots = SemanticScreenshotCoordinator::new();
        contexts.seal_for_shutdown().expect("context seal");
        profile_leases
            .seal_for_shutdown()
            .expect("profile lease seal");
        cookie_transfers
            .seal_for_shutdown()
            .expect("cookie transfer seal");
        action_executions.seal();
        action_settlements.seal();
        screenshots.seal_for_shutdown();
        let resources = AgentNativeShutdownResources::new(
            contexts,
            profile_leases,
            cookie_transfers,
            action_executions,
            action_settlements,
            screenshots,
        );
        AgentNativeShutdownCoordinator::try_new(resources).expect("admitted coordinator")
    }

    fn counts(queued_tasks: u8) -> ContextNativeResourceCounts {
        ContextNativeResourceCounts {
            known_bindings: 0,
            resident_views: 0,
            owned_reservations: 0,
            borrowed_leases: 0,
            visible_surfaces: 0,
            suspended_views: 0,
            pending_operations: 0,
            pending_captures: 0,
            queued_tasks,
        }
    }

    fn zero_snapshot() -> ContextNativeResourceSnapshot {
        ContextNativeResourceSnapshot::try_new(counts(0)).expect("zero snapshot")
    }

    fn nonzero_snapshot() -> ContextNativeResourceSnapshot {
        ContextNativeResourceSnapshot::try_new(counts(1)).expect("nonzero snapshot")
    }

    fn setup(steps: impl IntoIterator<Item = AuditStep>) -> (ScriptedPort, QueueEvents) {
        let events = Arc::new(Mutex::new(VecDeque::new()));
        (
            ScriptedPort::new(steps, Arc::clone(&events)),
            QueueEvents {
                events,
                close_when_empty: false,
            },
        )
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }

    #[test]
    fn exact_shutdown_audit_produces_the_only_clean_proof() {
        let (port, mut events) = setup([AuditStep::ScheduledZero]);
        let proof = drive_agent_native_shutdown_until(
            coordinator(),
            &port,
            ContextResourceAuditId::new(40).expect("audit"),
            &mut events,
            deadline(),
        )
        .expect("zero proof");
        assert_eq!(proof.audit().get(), 40);
        assert_eq!(proof.attempts(), 1);
        assert_eq!(proof.snapshot(), zero_snapshot());
        assert_eq!(port.calls.load(Ordering::Acquire), 1);
    }

    #[test]
    fn nonzero_and_rejected_audits_retry_with_strictly_new_identities() {
        let (port, mut events) = setup([
            AuditStep::ScheduledNonzero,
            AuditStep::Rejected,
            AuditStep::ScheduledZero,
        ]);
        let proof = drive_agent_native_shutdown_until(
            coordinator(),
            &port,
            ContextResourceAuditId::new(50).expect("audit"),
            &mut events,
            deadline(),
        )
        .expect("retried zero proof");
        assert_eq!(proof.audit().get(), 52);
        assert_eq!(proof.attempts(), 3);
        assert_eq!(port.calls.load(Ordering::Acquire), 3);
    }

    #[test]
    fn deadline_source_and_protocol_fail_closed() {
        let (port, mut events) = setup([AuditStep::ScheduledZero]);
        assert_eq!(
            drive_agent_native_shutdown_until(
                coordinator(),
                &port,
                ContextResourceAuditId::new(61).expect("audit"),
                &mut events,
                Instant::now(),
            )
            .expect_err("elapsed deadline"),
            AgentNativeShutdownDriveError::Deadline
        );
        assert_eq!(port.calls.load(Ordering::Acquire), 0);

        let (port, mut events) = setup([AuditStep::ScheduledWrongIdentity]);
        assert_eq!(
            drive_agent_native_shutdown_until(
                coordinator(),
                &port,
                ContextResourceAuditId::new(62).expect("audit"),
                &mut events,
                deadline(),
            )
            .expect_err("wrong audit identity"),
            AgentNativeShutdownDriveError::Protocol(AgentNativeShutdownError::AuditMismatch)
        );

        let (port, mut events) = setup([AuditStep::ScheduledNonzero]);
        events.close_when_empty = true;
        assert_eq!(
            drive_agent_native_shutdown_until(
                coordinator(),
                &port,
                ContextResourceAuditId::new(63).expect("audit"),
                &mut events,
                deadline(),
            )
            .expect_err("closed event source"),
            AgentNativeShutdownDriveError::EventSourceClosed
        );
    }

    #[test]
    fn retry_bounds_identity_overflow_and_unexpected_events() {
        let (port, mut events) = setup([
            AuditStep::Rejected,
            AuditStep::Rejected,
            AuditStep::Rejected,
            AuditStep::Rejected,
            AuditStep::Rejected,
            AuditStep::Rejected,
            AuditStep::Rejected,
            AuditStep::Rejected,
        ]);
        assert_eq!(
            drive_agent_native_shutdown_until(
                coordinator(),
                &port,
                ContextResourceAuditId::new(70).expect("audit"),
                &mut events,
                deadline(),
            )
            .expect_err("audit ceiling"),
            AgentNativeShutdownDriveError::AttemptsExhausted
        );
        assert_eq!(port.calls.load(Ordering::Acquire), 8);

        let (port, mut events) = setup([AuditStep::ScheduledNonzero]);
        assert_eq!(
            drive_agent_native_shutdown_until(
                coordinator(),
                &port,
                ContextResourceAuditId::new(u64::MAX).expect("audit"),
                &mut events,
                deadline(),
            )
            .expect_err("audit identity overflow"),
            AgentNativeShutdownDriveError::AuditIdentityExhausted
        );

        let (port, mut events) = setup([AuditStep::ScheduledNonzero]);
        events.events.lock().expect("event lock").push_back(
            ContextNativeEvent::ResourceAuditSettled(ContextResourceAuditSettlement::new(
                ContextResourceAuditId::new(99).expect("audit"),
                Ok(zero_snapshot()),
            )),
        );
        assert_eq!(
            drive_agent_native_shutdown_until(
                coordinator(),
                &port,
                ContextResourceAuditId::new(80).expect("audit"),
                &mut events,
                deadline(),
            )
            .expect_err("wrong event class"),
            AgentNativeShutdownDriveError::UnexpectedEvent
        );
    }

    #[test]
    fn wait_diagnostics_do_not_project_event_payloads() {
        let event = ContextNativeEvent::ResourceAuditSettled(ContextResourceAuditSettlement::new(
            ContextResourceAuditId::new(1).expect("audit"),
            Ok(nonzero_snapshot()),
        ));
        assert_eq!(
            format!("{:?}", AgentNativeShutdownWait::Event(Box::new(event))),
            "AgentNativeShutdownWait::Event([redacted])"
        );
        assert_eq!(retry_delay(1), Duration::from_millis(100));
        assert_eq!(retry_delay(2), Duration::from_millis(200));
        assert_eq!(retry_delay(5), Duration::from_millis(1_000));
        assert_eq!(retry_delay(u8::MAX), Duration::from_millis(1_000));
    }
}
