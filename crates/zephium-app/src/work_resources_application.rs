//! Application-owned single-resource admission behind the bounded Shell adapter.
//!
//! A retained native row is not successor permission. This owner alone sequences
//! original Store acknowledgements, the common controller and scoped worker,
//! and keeps the page/cleanup owner across fresh, explicitly supplied actors.

use super::shutdown::RetainedNativeShutdown;
use super::*;
use std::collections::VecDeque;
use std::task::Waker;
use std::time::{Duration, Instant};
use zephium_agent_controller::*;
use zephium_agent_provider_transport::{AgentProviderCredential, AgentProviderTransportConfig};
use zephium_agent_runtime::*;

#[cfg(feature = "work-execution-probe")]
#[path = "work_resources_public_actor.rs"]
mod public_qualification;

/// Trusted, dormant preparation. The callback receives only a newly acquired
/// read binding through the narrow facade, never the native resource owner.
pub(super) type PrepareActor = Box<
    dyn FnOnce(
            Box<dyn AgentWorkRetainedBrowser>,
            Arc<dyn AgentAuditPort>,
        ) -> Result<StagedActor, AgentWorkFailure>
        + Send,
>;

pub(super) struct ActorRequest {
    pub(super) run: ContextRunId,
    pub(super) deadline: AgentPolicyInstant,
    pub(super) prepare: PrepareActor,
}

pub(super) struct StagedActor {
    controller: Box<AgentWorkRetainedController>,
    handle: AgentWorkRetainedHandle,
    scope: AgentRuntimeScopedBinding,
    lease: WorkBrowserExecutionLease,
    audit: Arc<dyn AgentAuditPort>,
    config: AgentRuntimeConfig,
    deadline: Instant,
}
impl StagedActor {
    #[cfg(all(test, feature = "work-execution-probe"))]
    pub(super) fn for_probe(
        input: AgentWorkRunInput,
        browser: Box<dyn AgentWorkRetainedBrowser>,
        transport: zephium_agent_provider_transport::AgentProviderTransport,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
    ) -> Result<Self, AgentWorkFailure> {
        let lease = browser.binding().lease().clone();
        let (controller, handle, scope) = AgentWorkRetainedController::try_new_for_probe(
            input,
            browser,
            transport,
            credential,
            audit.clone(),
            task,
        )?;
        let deadline = controller.deadline()?;
        Ok(Self {
            controller: Box::new(controller),
            handle,
            scope,
            lease,
            audit,
            config: AgentRuntimeConfig::STANDARD,
            deadline,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn try_new(
        input: AgentWorkRunInput,
        browser: Box<dyn AgentWorkRetainedBrowser>,
        config: AgentRuntimeConfig,
        provider: AgentProviderTransportConfig,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
    ) -> Result<Self, AgentWorkFailure> {
        let lease = browser.binding().lease().clone();
        let (controller, handle, scope) = AgentWorkRetainedController::try_new(
            input,
            browser,
            provider,
            credential,
            audit.clone(),
            task,
        )?;
        let deadline = controller.deadline()?;
        Ok(Self {
            controller: Box::new(controller),
            handle,
            scope,
            lease,
            audit,
            config,
            deadline,
        })
    }
}

struct ActiveActor {
    handle: AgentWorkRetainedHandle,
    runtime: AgentRuntimeHandle,
    completion: AgentRuntimeCompletion,
    lifecycle: Option<AgentRuntimeScopedLifecycle>,
    drained: Option<AgentRuntimeScopedDrained>,
    outcome: Option<AgentWorkRetainedOutcome>,
    accepted: Option<AgentRunPolicySettlement>,
    extraction: Option<Box<SemanticOwnedExtractionResult>>,
    pending_event: Option<AgentWorkEvent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AdmissionPhase {
    Constructing,
    Loading,
    Ready,
    Acquiring,
    Admitting,
    Starting,
    Running,
    Closing,
    Terminal,
    Uncertain,
}

/// Original request and callback slot stay owned even after timeout/refusal.
/// There is no read-back shortcut, automatic retry or decoded-record admission.
struct JournalFlight {
    request: AgentWorkJournalRequest,
    result: Arc<Mutex<Option<Result<AgentWorkJournalReply, AgentWorkJournalError>>>>,
    deadline: Instant,
    phase: AdmissionPhase,
    uncertain: bool,
    reconciliations: u8,
}

pub(super) struct RetainedWork {
    owner: WorkResourceOwner,
    resource: WorkBrowserResourceJoin,
    journal: Arc<dyn AgentWorkJournalPort>,
    audit: Arc<dyn AgentAuditPort>,
    waker: Waker,
    incarnation: Option<AgentWorkIncarnation>,
    inventory: Vec<AgentWorkRecord>,
    phase: AdmissionPhase,
    failure: Option<AgentWorkFailure>,
    persistence_failure: Option<AgentWorkJournalError>,
    request: Option<ActorRequest>,
    acquisition: Option<PendingLifecycle>,
    construction: Option<PendingLifecycle>,
    staged: Option<StagedActor>,
    unstarted: Option<AgentWorkRetainedOutcome>,
    active: Option<ActiveActor>,
    flight: Option<JournalFlight>,
    record: Option<AgentWorkRecord>,
    events: VecDeque<AgentWorkEvent>,
    stopping: bool,
    destruction: Option<PendingLifecycle>,
    destruction_settled: bool,
    destroyed: bool,
    unexpected_native: Option<ContextNativeEvent>,
    native_shutdown: Option<RetainedNativeShutdown>,
}
impl RetainedWork {
    /// Consumes the original owner, not a replacement port or a decoded row.
    /// Store and audit must be the same original adapter allocation. Construction
    /// and selected-profile admission still belong to the trusted application.
    pub(super) fn new(
        owner: WorkResourceOwner,
        resource: WorkBrowserResourceJoin,
        journal: Arc<dyn AgentWorkJournalPort>,
        audit: Arc<dyn AgentAuditPort>,
    ) -> Result<Self, (WorkResourceOwner, Refusal)> {
        let valid = std::ptr::addr_eq(Arc::as_ptr(&journal), Arc::as_ptr(&audit))
            && owner
                .shared
                .lock_resources()
                .is_ok_and(|rows| rows.len() == 1)
            && owner.shared.resource(&resource).is_ok_and(|row| {
                owner.shared.current(&row).is_ok()
                    && row.reusable.load(Ordering::Acquire)
                    && row.flights.load(Ordering::Acquire) == 0
            })
            && owner
                .shared
                .lock_rows()
                .is_ok_and(|rows| rows.phase(&resource) == Ok(WorkBrowserResourcePhase::Retained));
        if !valid {
            return Err((owner, Refusal::Uncertain));
        }
        // Journal/progress use the same original application wake as native
        // resource notifications; no second callback route is substituted.
        let waker = owner.shared.notifications.clone().into();
        let mut work = Self::dormant(owner, resource, journal, audit, waker);
        work.rearm();
        if work.failure.is_some() {
            return Err((work.owner, Refusal::Uncertain));
        }
        work.dispatch(AgentWorkJournalRequest::Claim);
        Ok(work)
    }

    fn dormant(
        owner: WorkResourceOwner,
        resource: WorkBrowserResourceJoin,
        journal: Arc<dyn AgentWorkJournalPort>,
        audit: Arc<dyn AgentAuditPort>,
        waker: Waker,
    ) -> Self {
        Self {
            owner,
            resource,
            journal,
            audit,
            waker,
            incarnation: None,
            inventory: Vec::new(),
            phase: AdmissionPhase::Loading,
            failure: None,
            persistence_failure: None,
            request: None,
            acquisition: None,
            construction: None,
            staged: None,
            unstarted: None,
            active: None,
            flight: None,
            record: None,
            events: VecDeque::with_capacity(MAX_AGENT_WORK_EVENTS),
            stopping: false,
            destruction: None,
            destruction_settled: false,
            destroyed: false,
            unexpected_native: None,
            native_shutdown: None,
        }
    }

    /// Shell has already checked original Store/Engine/profile before native
    /// construction. Retain its original operation through stop and timeout.
    pub(super) fn constructing(
        owner: WorkResourceOwner,
        pending: PendingLifecycle,
        journal: Arc<dyn AgentWorkJournalPort>,
        audit: Arc<dyn AgentAuditPort>,
    ) -> Self {
        let resource = pending.resource.join.clone();
        let exact = Arc::ptr_eq(&owner.shared, &pending.shared)
            && std::ptr::addr_eq(Arc::as_ptr(&journal), Arc::as_ptr(&audit))
            && owner
                .shared
                .lock_resources()
                .is_ok_and(|rows| rows.len() == 1)
            && owner
                .shared
                .resource(&resource)
                .is_ok_and(|row| Arc::ptr_eq(&row, &pending.resource));
        let waker = owner.shared.notifications.clone().into();
        let mut work = Self::dormant(owner, resource, journal, audit, waker);
        work.phase = AdmissionPhase::Constructing;
        work.construction = Some(pending);
        if !exact {
            work.fail(AgentWorkFailure::Contract);
        }
        work
    }

    pub(super) fn ready(&self) -> bool {
        if self.stopping
            || self.failure.is_some()
            || self.persistence_failure.is_some()
            || self.flight.is_some()
            || self.request.is_some()
            || self.acquisition.is_some()
            || self.staged.is_some()
            || self.unstarted.is_some()
            || !self.events.is_empty()
            || self.inventory.len() == MAX_DURABLE_AGENT_WORK_RUNS
        {
            return false;
        }
        match self.phase {
            AdmissionPhase::Ready => self.active.is_none() && self.record.is_none(),
            AdmissionPhase::Terminal => self.active.as_ref().is_some_and(|active| {
                active.drained.is_some()
                    && active.lifecycle.is_none()
                    && active.completion.is_stopped()
                    && (active.outcome.is_some() || active.accepted.is_some())
                    && active.extraction.is_none()
                    && active.pending_event.is_none()
                    && !active.handle.has_pending_events()
                    && self
                        .record
                        .is_some_and(|record| record.debt() == AgentWorkDebt::NONE)
            }),
            _ => false,
        }
    }

    /// Exactly one fresh explicit request; no automatic actor restart or replay.
    /// A refusal returns the original task/credential closure without consuming it.
    pub(super) fn submit(
        &mut self,
        request: ActorRequest,
        now: AgentPolicyInstant,
    ) -> Result<(), ActorRequest> {
        if !self.ready()
            || request.deadline <= now
            || self
                .inventory
                .iter()
                .any(|prior| prior.as_bytes()[48..64] == request.run.bytes())
        {
            return Err(request);
        }
        let acquisition =
            match self
                .owner
                .acquire(&self.resource, request.run, now, request.deadline)
            {
                Ok(acquisition) => acquisition,
                Err(_) => return Err(request),
            };
        // Only this exact durable/scoped join retires the previous actor owner.
        self.active.take();
        self.record = None;
        self.acquisition = Some(acquisition);
        self.request = Some(request);
        self.phase = AdmissionPhase::Acquiring;
        Ok(())
    }

    fn dispatch(&mut self, request: AgentWorkJournalRequest) {
        self.dispatch_attempt(request, self.phase, 0);
    }

    fn dispatch_attempt(
        &mut self,
        request: AgentWorkJournalRequest,
        phase: AdmissionPhase,
        reconciliations: u8,
    ) {
        debug_assert!(self.flight.is_none());
        let result = Arc::new(Mutex::new(None));
        let sink = result.clone();
        let wake = self.waker.clone();
        self.flight = Some(JournalFlight {
            request,
            result: result.clone(),
            deadline: Instant::now() + Duration::from_secs(2),
            phase,
            uncertain: false,
            reconciliations,
        });
        if let Err(error) = self.journal.dispatch(
            request,
            Box::new(move |reply| {
                match sink.lock() {
                    Ok(mut slot) => *slot = Some(reply),
                    Err(_) => return,
                }
                wake.wake();
            }),
        ) {
            if let Ok(mut slot) = result.lock() {
                *slot = Some(Err(error));
            }
        }
    }

    fn fail(&mut self, failure: AgentWorkFailure) {
        self.failure.get_or_insert(failure);
        self.phase = AdmissionPhase::Uncertain;
        self.stopping = true;
        self.request.take();
        if let Some(staged) = self.staged.take() {
            let StagedActor {
                controller,
                mut handle,
                ..
            } = staged;
            drop(controller);
            self.unstarted = handle.take_outcome();
        }
        if let Some(active) = &self.active {
            active
                .runtime
                .stop_and_seal(AgentRuntimeStopReason::Cancelled);
        }
    }

    fn acknowledge(
        &mut self,
        request: &AgentWorkJournalRequest,
        reply: AgentWorkJournalReply,
        phase: AdmissionPhase,
    ) -> Result<(), AgentWorkJournalError> {
        match (request, reply) {
            (AgentWorkJournalRequest::Claim, AgentWorkJournalReply::Claimed { owner, records }) => {
                if records.len() > MAX_DURABLE_AGENT_WORK_RUNS
                    || records
                        .windows(2)
                        .any(|pair| pair[0].key() >= pair[1].key())
                    || records.iter().any(|record| {
                        !record.disposition().is_terminal() || record.debt() != AgentWorkDebt::NONE
                    })
                {
                    // Recovery inventory needs the existing explicit recovery
                    // workflow, not a new resource coordinator clearing debt.
                    self.inventory = records;
                    #[cfg(feature = "work-execution-probe")]
                    self.public_claim_refusal_diagnostic();
                    return Err(AgentWorkJournalError::Conflict);
                }
                self.incarnation = Some(owner);
                self.inventory = records;
                self.phase = if self.stopping {
                    AdmissionPhase::Uncertain
                } else {
                    AdmissionPhase::Ready
                };
            }
            (
                AgentWorkJournalRequest::CompareAndSet(mutation),
                AgentWorkJournalReply::Record(Some(record)),
            ) if record == mutation.next() => {
                if let Some(previous) = self
                    .inventory
                    .iter_mut()
                    .find(|old| old.key() == record.key())
                {
                    *previous = record;
                } else if self.inventory.len() < MAX_DURABLE_AGENT_WORK_RUNS {
                    self.inventory.push(record);
                } else {
                    return Err(AgentWorkJournalError::Capacity);
                }
                self.record = Some(record);
                match (phase, record.disposition()) {
                    (AdmissionPhase::Admitting, AgentWorkDisposition::Admitted) => {
                        self.phase = if self.stopping {
                            AdmissionPhase::Closing
                        } else {
                            AdmissionPhase::Starting
                        };
                        self.dispatch(AgentWorkJournalRequest::CompareAndSet(
                            AgentWorkJournalMutation::transition(
                                record,
                                if self.stopping {
                                    AgentWorkDisposition::FailedClosed
                                } else {
                                    AgentWorkDisposition::Running
                                },
                            )?,
                        ));
                    }
                    (AdmissionPhase::Starting, AgentWorkDisposition::Running) => {
                        if self.stopping {
                            self.phase = AdmissionPhase::Closing;
                            self.dispatch(AgentWorkJournalRequest::CompareAndSet(
                                AgentWorkJournalMutation::transition(
                                    record,
                                    AgentWorkDisposition::FailedClosed,
                                )?,
                            ));
                        } else {
                            self.activate();
                        }
                    }
                    (
                        AdmissionPhase::Closing,
                        AgentWorkDisposition::Succeeded
                        | AgentWorkDisposition::Failed
                        | AgentWorkDisposition::Cancelled,
                    ) => {
                        self.phase = AdmissionPhase::Terminal;
                    }
                    (AdmissionPhase::Closing, AgentWorkDisposition::FailedClosed) => {
                        // Immutable no-replay classification, not scoped closure.
                        self.phase = AdmissionPhase::Uncertain;
                    }
                    (AdmissionPhase::Closing, AgentWorkDisposition::RecoveryRequired) => {
                        // The exact scoped controller ended in Recovery, so the
                        // durable row must remain nonterminal with its original
                        // conservative debt. This acknowledgement classifies
                        // the interruption; it is not a clean actor closure.
                        self.phase = AdmissionPhase::Uncertain;
                    }
                    _ => return Err(AgentWorkJournalError::Transition),
                }
            }
            _ => return Err(AgentWorkJournalError::Conflict),
        }
        Ok(())
    }

    fn activate(&mut self) {
        let Some(staged) = self.staged.take() else {
            self.fail(AgentWorkFailure::Contract);
            return;
        };
        if self.stopping || staged.deadline <= Instant::now() {
            self.staged = Some(staged);
            self.fail(AgentWorkFailure::Deadline);
            return;
        }
        let StagedActor {
            controller,
            mut handle,
            scope,
            config,
            ..
        } = staged;
        handle.set_waker(self.waker.clone());
        let pending = match PendingScopedAgentRuntime::spawn_suspended(config, scope, controller) {
            Ok(pending) => pending,
            Err(_) => {
                self.unstarted = handle.take_outcome();
                self.fail(AgentWorkFailure::Contract);
                return;
            }
        };
        let (runtime, completion, lifecycle) = pending.bind().into_parts();
        completion.set_waker(self.waker.clone());
        let started = runtime.start_run().is_ok();
        self.active = Some(ActiveActor {
            handle,
            runtime,
            completion,
            lifecycle: Some(lifecycle),
            drained: None,
            outcome: None,
            accepted: None,
            extraction: None,
            pending_event: None,
        });
        self.phase = AdmissionPhase::Running;
        if !started {
            self.fail(AgentWorkFailure::Contract);
        }
    }

    pub(super) fn poll(&mut self, now: AgentPolicyInstant) {
        self.poll_before(now, None);
    }

    fn poll_before(&mut self, now: AgentPolicyInstant, deadline: Option<Instant>) {
        self.rearm();
        if let Some(mut construction) = self.construction.take() {
            match construction.poll(now) {
                Ok(None) => self.construction = Some(construction),
                Ok(Some(LifecycleResult::Event(WorkBrowserResourceEvent::Retained(resource))))
                    if resource == self.resource =>
                {
                    if !self.stopping {
                        self.phase = AdmissionPhase::Loading;
                        self.dispatch(AgentWorkJournalRequest::Claim);
                    }
                }
                _ => self.fail(AgentWorkFailure::ContextLost),
            }
        }
        // Check both original horizons before consuming a raced Running ACK.
        if self.staged.as_ref().is_some_and(|staged| {
            staged.deadline <= Instant::now() || staged.lease.deadline() <= now
        }) {
            self.fail(AgentWorkFailure::Deadline);
        }
        if let Some(mut flight) = self.flight.take() {
            let reply = flight
                .result
                .lock()
                .map(|mut slot| slot.take())
                .map_err(|_| ());
            let mut retain = false;
            let result = match reply {
                Ok(Some(Ok(reply))) => self.acknowledge(&flight.request, reply, flight.phase),
                Ok(Some(Err(error))) => Err(error),
                Err(_) => Err(AgentWorkJournalError::Uncertain),
                _ if !flight.uncertain && Instant::now() >= flight.deadline => {
                    Err(AgentWorkJournalError::Uncertain)
                }
                _ => {
                    retain = true;
                    Ok(())
                }
            };
            if let Err(error) = result {
                self.persistence_failure.get_or_insert(error);
                self.fail(AgentWorkFailure::Contract);
                flight.uncertain = true;
                // Retain the exact unsettled request even after a malformed ACK.
                retain = true;
            }
            if retain {
                self.flight = Some(flight);
            } else {
                self.persistence_failure = None;
            }
        }
        if let Some(mut acquisition) = self.acquisition.take() {
            match acquisition.poll(now) {
                Ok(None) => self.acquisition = Some(acquisition),
                Ok(Some(LifecycleResult::Event(WorkBrowserResourceEvent::Acquired(lease)))) => {
                    if let Some(request) = self.request.take() {
                        let prepared = self
                            .owner
                            .retained_browser(lease.clone(), now)
                            .map_err(|_| AgentWorkFailure::ContextLost)
                            .and_then(|browser| {
                                (request.prepare)(Box::new(browser), self.audit.clone())
                            });
                        match prepared {
                            Ok(staged)
                                if staged.lease == lease
                                    && std::ptr::addr_eq(
                                        Arc::as_ptr(&staged.audit),
                                        Arc::as_ptr(&self.audit),
                                    ) =>
                            {
                                let admission = self
                                    .incarnation
                                    .ok_or(AgentWorkFailure::Contract)
                                    .and_then(|owner| staged.controller.journal_admission(owner));
                                self.staged = Some(staged);
                                match admission {
                                    Ok(admission) => {
                                        self.phase = AdmissionPhase::Admitting;
                                        self.dispatch(AgentWorkJournalRequest::CompareAndSet(
                                            admission,
                                        ));
                                    }
                                    Err(error) => self.fail(error),
                                }
                            }
                            Ok(staged) => {
                                self.staged = Some(staged);
                                self.fail(AgentWorkFailure::Contract);
                            }
                            Err(error) => self.fail(error),
                        }
                    }
                }
                _ => self.fail(AgentWorkFailure::ContextLost),
            }
        }
        let mut failure = None;
        if let Some(active) = &mut self.active {
            while let Some(event) = active
                .pending_event
                .take()
                .or_else(|| active.handle.take_event())
            {
                if self.events.len() == MAX_AGENT_WORK_EVENTS {
                    active.pending_event = Some(event);
                    failure = Some(AgentWorkFailure::Backpressure);
                    break;
                }
                self.events.push_back(event);
            }
            if active.completion.is_stopped() {
                if let Some(lifecycle) = active.lifecycle.take() {
                    let drain_deadline =
                        deadline.unwrap_or_else(|| Instant::now() + Duration::from_millis(100));
                    match lifecycle.drain_until(drain_deadline) {
                        AgentRuntimeScopedDrain::Drained(drained) => active.drained = Some(drained),
                        AgentRuntimeScopedDrain::Unproven => {
                            failure = Some(AgentWorkFailure::Contract)
                        }
                    }
                }
                if active.outcome.is_none() && active.accepted.is_none() {
                    match active.handle.take_outcome() {
                        Some(AgentWorkRetainedOutcome::Accepted {
                            settlement,
                            extraction,
                        }) => {
                            active.accepted = Some(settlement);
                            active.extraction = Some(extraction);
                        }
                        outcome => active.outcome = outcome,
                    }
                }
                if let Some(AgentWorkRetainedOutcome::Recovery(recovery)) = &active.outcome {
                    failure = Some(recovery.failure());
                }
            }
        }
        if let Some(failure) = failure {
            self.fail(failure);
        }
        if self.phase == AdmissionPhase::Running && self.flight.is_none() {
            if let (Some(active), Some(record)) = (&mut self.active, self.record) {
                if let Some(drained) = &active.drained {
                    let policy = match &active.outcome {
                        Some(AgentWorkRetainedOutcome::ClosedUnsuccessfully(closed)) => {
                            Some(closed.policy_settlement())
                        }
                        _ => active.accepted,
                    };
                    let terminal = if policy == Some(drained.policy())
                        && drained.lease().resource() == &self.resource
                    {
                        drained.work_terminal(&active.runtime, record)
                    } else {
                        Err(AgentWorkJournalError::Transition)
                    };
                    match terminal {
                        Ok(terminal) => {
                            self.phase = AdmissionPhase::Closing;
                            self.dispatch(AgentWorkJournalRequest::CompareAndSet(terminal));
                        }
                        Err(_) => self.fail(AgentWorkFailure::Contract),
                    }
                }
            }
        }
        // A scoped Recovery is the controller's final move-only outcome, but
        // carries no policy settlement that could authorize Failed, Cancelled
        // or Succeeded. Classify the exact Running row conservatively before
        // Store shutdown. The transition preserves UNKNOWN debt and remains
        // nonterminal, so it cannot be mistaken for a clean run or successor
        // admission merely because physical resource cleanup later succeeds.
        if self.flight.is_none()
            && self.active.as_ref().is_some_and(|active| {
                active.completion.is_stopped()
                    && active.lifecycle.is_none()
                    && matches!(active.outcome, Some(AgentWorkRetainedOutcome::Recovery(_)))
            })
        {
            if let Some(record) = self
                .record
                .filter(|record| record.disposition() == AgentWorkDisposition::Running)
            {
                match AgentWorkJournalMutation::transition(
                    record,
                    AgentWorkDisposition::RecoveryRequired,
                ) {
                    Ok(mutation) => {
                        self.phase = AdmissionPhase::Closing;
                        self.dispatch(AgentWorkJournalRequest::CompareAndSet(mutation));
                    }
                    Err(error) => {
                        self.persistence_failure.get_or_insert(error);
                    }
                }
            }
        }
        // A hard-deadline/spawn refusal can occur after Running ACK but before
        // an ActiveActor exists. Persist no-replay classification, not success
        // or revocation, from that exact acknowledged predecessor too.
        if self.stopping && self.active.is_none() && self.flight.is_none() {
            if let Some(record) = self.record.filter(|record| {
                matches!(
                    record.disposition(),
                    AgentWorkDisposition::Admitted | AgentWorkDisposition::Running
                )
            }) {
                match AgentWorkJournalMutation::transition(
                    record,
                    AgentWorkDisposition::FailedClosed,
                ) {
                    Ok(mutation) => {
                        self.phase = AdmissionPhase::Closing;
                        self.dispatch(AgentWorkJournalRequest::CompareAndSet(mutation));
                    }
                    Err(error) => {
                        self.persistence_failure = Some(error);
                    }
                }
            }
        }
    }

    fn rearm(&mut self) {
        if self.unexpected_native.is_some() {
            return;
        }
        loop {
            match self.owner.poll_native_event() {
                Ok(None) => return,
                Ok(Some(event)) => {
                    let event = if let Some(shutdown) = &mut self.native_shutdown {
                        match shutdown.settle(event) {
                            Ok(()) => continue,
                            Err(event) => *event,
                        }
                    } else {
                        event
                    };
                    // Preserve any unadmitted/mismatched original global terminal.
                    self.unexpected_native = Some(event);
                    self.fail(AgentWorkFailure::ContextLost);
                    return;
                }
                Err(_) => {
                    self.fail(AgentWorkFailure::ContextLost);
                    return;
                }
            }
        }
    }

    pub(super) fn take_event(&mut self) -> Option<AgentWorkEvent> {
        self.events.pop_front()
    }

    pub(super) fn phase(&self) -> AdmissionPhase {
        self.phase
    }

    pub(super) fn failures(&self) -> (Option<AgentWorkFailure>, Option<AgentWorkJournalError>) {
        // A proved unsuccessful actor outcome is not coordinator uncertainty.
        // Project its original cause without calling fail(), changing phase,
        // cancelling a drained actor, or interfering with the terminal CAS.
        let failure = self
            .failure
            .or_else(|| match self.active.as_ref()?.outcome.as_ref()? {
                AgentWorkRetainedOutcome::ClosedUnsuccessfully(closed) => Some(closed.failure()),
                _ => None,
            });
        (failure, self.persistence_failure)
    }

    pub(super) fn record(&self) -> Option<AgentWorkRecord> {
        self.record
    }

    pub(super) fn take_extraction(&mut self) -> Option<Box<SemanticOwnedExtractionResult>> {
        if self.phase != AdmissionPhase::Terminal
            || self.failure.is_some()
            || self.persistence_failure.is_some()
            || self.record?.disposition() != AgentWorkDisposition::Succeeded
        {
            return None;
        }
        self.active.as_mut()?.extraction.take()
    }

    /// Revokes actor execution only. It grants no rendering or human-input right.
    pub(super) fn cancel(&mut self) {
        if self.active.is_none()
            || self.request.is_some()
            || self.acquisition.is_some()
            || self.staged.is_some()
        {
            self.fail(AgentWorkFailure::Cancelled);
        }
        if let Some(active) = &self.active {
            active
                .runtime
                .stop_and_seal(AgentRuntimeStopReason::Cancelled);
        }
    }

    /// Explicit bounded reconciliation of the original exact idempotent request.
    /// Old callbacks retain their own disconnected slots; none can select a new
    /// worker or clear the sticky stop. A Store read is not used as an ACK.
    pub(super) fn reconcile(&mut self) -> bool {
        let Some(flight) = self.flight.take() else {
            return false;
        };
        if !flight.uncertain || flight.reconciliations >= 4 {
            self.flight = Some(flight);
            return false;
        }
        self.dispatch_attempt(flight.request, flight.phase, flight.reconciliations + 1);
        true
    }

    /// Cleanup remains possible after any admission failure. This is not human
    /// takeover, persistence settlement, or a global native-zero proof.
    pub(super) fn begin_shutdown(&mut self) {
        self.stopping = true;
        self.request.take();
        self.cancel();
        if self.staged.is_some() {
            self.fail(AgentWorkFailure::Cancelled);
        }
    }

    pub(super) fn poll_shutdown(&mut self, now: AgentPolicyInstant) -> Result<bool, Refusal> {
        self.poll_shutdown_before(now, None)
    }

    fn poll_shutdown_before(
        &mut self,
        now: AgentPolicyInstant,
        deadline: Option<Instant>,
    ) -> Result<bool, Refusal> {
        if !self.stopping {
            return Err(Refusal::Busy);
        }
        self.poll_before(now, deadline);
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Ok(false);
        }
        // A controller can end in Recovery while an exact native callback is
        // still owned by its abandoned operation slot. Drain only callbacks
        // that have actually arrived; an outstanding callback remains counted
        // and therefore continues to block reap/global shutdown below.
        self.owner.drain_abandoned(now)?;
        if self.destroyed {
            let native_clean = self.poll_native_shutdown(deadline)?;
            if native_clean && self.final_scoped_recovery_is_classified() {
                return Err(Refusal::Uncertain);
            }
            return Ok(native_clean && self.local_shutdown_settled());
        }
        if self.construction.is_some()
            || self.acquisition.is_some()
            || self.staged.is_some()
            || self
                .active
                .as_ref()
                .is_some_and(|active| !active.completion.is_stopped() || active.lifecycle.is_some())
        {
            return Ok(false);
        }
        if self.destruction.is_none() && !self.destruction_settled {
            self.destruction = Some(self.owner.destroy(&self.resource)?);
        }
        if !self.destruction_settled {
            match self
                .destruction
                .as_mut()
                .ok_or(Refusal::Uncertain)?
                .poll(now)?
            {
                None => return Ok(false),
                Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(_))) => {
                    self.destruction_settled = true;
                    self.destruction.take();
                }
                Some(_) => return Err(Refusal::Uncertain),
            }
        }
        match self.owner.reap_absent(&self.resource) {
            Ok(_) => {}
            Err(Refusal::Busy) => return Ok(false),
            Err(error) => return Err(error),
        }
        self.owner.seal_resources()?;
        self.destroyed = true;
        let native_clean = self.poll_native_shutdown(deadline)?;
        if native_clean && self.final_scoped_recovery_is_classified() {
            return Err(Refusal::Uncertain);
        }
        Ok(native_clean && self.local_shutdown_settled())
    }

    /// A final scoped Recovery cannot gain a policy settlement or a different
    /// runtime outcome inside this coordinator. Once its conservative Store
    /// classification is acknowledged there is no logical event left to wait
    /// for. Physical/native cleanup still runs first; the caller then receives
    /// an immediate unclean result instead of losing the remaining global
    /// shutdown budget to a notification that cannot make the run clean.
    fn final_scoped_recovery_is_classified(&self) -> bool {
        self.flight.is_none()
            && self.persistence_failure.is_none()
            && self.record.is_some_and(|record| {
                record.disposition() == AgentWorkDisposition::RecoveryRequired
            })
            && self.active.as_ref().is_some_and(|active| {
                active.completion.is_stopped()
                    && active.lifecycle.is_none()
                    && matches!(active.outcome, Some(AgentWorkRetainedOutcome::Recovery(_)))
            })
    }

    fn local_shutdown_settled(&self) -> bool {
        self.destroyed
            && self.owner.locally_retired()
            && self.flight.is_none()
            && self.persistence_failure.is_none()
            && self.unexpected_native.is_none()
            && self.unstarted.is_none()
            && self
                .record
                .is_none_or(|record| record.disposition().is_terminal())
            && self
                .active
                .as_ref()
                .is_none_or(|active| active.drained.is_some())
    }

    fn poll_native_shutdown(&mut self, deadline: Option<Instant>) -> Result<bool, Refusal> {
        let local_ready = self.local_shutdown_settled()
            || (self.destroyed
                && self.owner.locally_retired()
                && self.unexpected_native.is_none()
                && self.unstarted.is_none()
                && self.final_scoped_recovery_is_classified());
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) || !local_ready {
            return Ok(false);
        }
        if self.native_shutdown.is_none() {
            self.native_shutdown = Some(RetainedNativeShutdown::new(&self.owner)?);
        }
        self.native_shutdown
            .as_mut()
            .ok_or(Refusal::Uncertain)?
            .poll()
    }

    /// The caller's one absolute process deadline covers scoped worker, Store,
    /// resource callbacks and global native audit retries. The Shell command
    /// queue need not run: wait on the original callback epoch instead. Timeout
    /// never consumes unresolved owners or authorizes a fresh actor.
    pub(super) fn shutdown_until(
        &mut self,
        clock: &dyn TerraControllerClock,
        deadline: Instant,
    ) -> bool {
        self.begin_shutdown();
        let notifications = self.owner.shared.notifications.clone();
        loop {
            if Instant::now() >= deadline {
                return false;
            }
            // Snapshot before polling, not after: a final callback can race
            // the empty poll and the beginning of the blocking wait.
            let Ok(epoch) = notifications.epoch.snapshot() else {
                self.fail(AgentWorkFailure::ContextLost);
                return false;
            };
            let Ok(now) = clock.now() else {
                self.fail(AgentWorkFailure::Contract);
                return false;
            };
            if Instant::now() >= deadline {
                return false;
            }
            match self.poll_shutdown_before(now, Some(deadline)) {
                Ok(true) => return Instant::now() < deadline,
                Err(_) => return false,
                Ok(false) => {}
            }
            let wake_at = self
                .next_deadline()
                .map_or(deadline, |next| next.min(deadline));
            if notifications
                .epoch
                .wait_until_changed(epoch, wake_at)
                .is_err()
            {
                self.fail(AgentWorkFailure::ContextLost);
                return false;
            }
        }
    }

    pub(super) fn resource_destroyed(&self) -> bool {
        self.destroyed
    }

    /// Joins the existing application timer; no extra worker or polling thread.
    pub(super) fn next_deadline(&self) -> Option<Instant> {
        self.flight
            .as_ref()
            .filter(|flight| !flight.uncertain)
            .map(|flight| flight.deadline)
            .into_iter()
            .chain(self.staged.as_ref().map(|staged| staged.deadline))
            .chain(
                self.native_shutdown
                    .as_ref()
                    .and_then(RetainedNativeShutdown::next_deadline),
            )
            .min()
    }
}
