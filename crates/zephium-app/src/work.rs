//! Optional application-owned Work admission, lifecycle and durable recovery.

#![deny(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

use crate::{CallbackHandle, Command};

#[cfg(feature = "work-execution-probe")]
#[path = "work_probe.rs"]
mod probe;
#[cfg(test)]
#[path = "work_tests.rs"]
mod tests;
use std::{
    collections::VecDeque,
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    task::{Wake, Waker},
    time::{Duration, Instant},
};
use zephium_agent_controller::{
    AgentWorkController, AgentWorkEvent, AgentWorkFailure, AgentWorkHandle, AgentWorkOutcome,
    AgentWorkRunInput, AgentWorkTask,
};
use zephium_agent_provider_transport::{AgentProviderCredential, AgentProviderTransportConfig};
use zephium_agent_runtime::{
    AgentRuntimeCompletion, AgentRuntimeConfig, AgentRuntimeHandle, AgentRuntimeStopReason,
    NativeEventSink, PendingAgentRuntime,
};
use zephium_agentic::*;

/// Trusted composition-root factory for the original one-shot engine port.
/// Invoked only after durable admission. It must not reopen a sealed port,
/// substitute ordinary tabs, or retain the sink as independent authority.
pub type AgentWorkNativeFactory =
    Box<dyn FnOnce(NativeEventSink) -> Option<Arc<dyn AgentBrowserPort>> + Send>;

/// Trusted non-UI execution ports; native factory is move-only.
pub struct AgentWorkApplicationPorts {
    engine: crate::SharedEngine,
    audit: Arc<dyn AgentAuditPort>,
    native: AgentWorkNativeFactory,
}

impl AgentWorkApplicationPorts {
    /// Binds the existing durable audit adapter and actual engine-port factory.
    pub fn new(
        engine: crate::SharedEngine,
        audit: Arc<dyn AgentAuditPort>,
        native: AgentWorkNativeFactory,
    ) -> Self {
        Self {
            engine,
            audit,
            native,
        }
    }
}

/// Existing runtime and provider settings, with no parallel model semantics.
pub struct AgentWorkApplicationConfig {
    runtime: AgentRuntimeConfig,
    provider: AgentProviderTransportConfig,
}

impl AgentWorkApplicationConfig {
    /// Uses the same bounded runtime and transport configuration as the actor.
    pub fn new(runtime: AgentRuntimeConfig, provider: AgentProviderTransportConfig) -> Self {
        Self { runtime, provider }
    }
}

/// Dormant controller ownership. Preparation starts no runtime or browser.
#[must_use]
pub struct PreparedAgentWork {
    engine: crate::SharedEngine,
    audit: Arc<dyn AgentAuditPort>,
    controller: Box<AgentWorkController>,
    handle: AgentWorkHandle,
    runtime: AgentRuntimeConfig,
    native: AgentWorkNativeFactory,
    deadline: Instant,
    run: ContextRunId,
}

impl PreparedAgentWork {
    /// Builds the shipping stateless actor from trusted product inputs.
    pub fn try_new(
        input: AgentWorkRunInput,
        config: AgentWorkApplicationConfig,
        credential: AgentProviderCredential,
        task: Box<dyn AgentWorkTask>,
        ports: AgentWorkApplicationPorts,
    ) -> Result<Self, AgentWorkFailure> {
        let (controller, handle) = AgentWorkController::try_new(
            input,
            config.provider,
            credential,
            ports.audit.clone(),
            task,
        )?;
        Self::from_controller(controller, handle, config.runtime, ports)
    }

    fn from_controller(
        controller: AgentWorkController,
        handle: AgentWorkHandle,
        runtime: AgentRuntimeConfig,
        ports: AgentWorkApplicationPorts,
    ) -> Result<Self, AgentWorkFailure> {
        let deadline = controller.deadline()?;
        if deadline <= Instant::now() {
            return Err(AgentWorkFailure::Deadline);
        }
        let run = controller.run_identity()?;
        Ok(Self {
            engine: ports.engine,
            audit: ports.audit,
            controller: Box::new(controller),
            handle,
            runtime,
            native: ports.native,
            deadline,
            run,
        })
    }
}

impl fmt::Debug for PreparedAgentWork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PreparedAgentWork([owned, redacted])")
    }
}

/// Typed application phase; success is published only after durable terminal CAS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWorkApplicationPhase {
    Loading,
    Ready,
    Admitting,
    Running,
    Closing,
    NeedsReview,
    Recovery,
    PersistenceUncertain,
    Succeeded,
}

/// Human review never resumes an old proposal or clears execution debt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWorkReviewDecision {
    AcceptFreshAdmission,
    Reject,
}

/// Compact product projection with no objective, model/page content or native handles.
#[derive(Clone, Copy, Debug)]
pub struct AgentWorkApplicationSnapshot {
    pub phase: AgentWorkApplicationPhase,
    pub run: Option<ContextRunId>,
    pub failure: Option<AgentWorkFailure>,
    pub persistence_failure: Option<AgentWorkJournalError>,
    pub last_review: Option<Result<AgentWorkRecord, AgentWorkJournalError>>,
    pub recovery_audit: Option<Result<AgentAuditLedgerStatus, AgentWorkFailure>>,
}

struct Projection {
    extraction: Option<SemanticOwnedExtractionResult>,
    snapshot: AgentWorkApplicationSnapshot,
    records: Vec<AgentWorkRecord>,
    events: VecDeque<AgentWorkEvent>,
}

/// Stable shell port for future Work state/UI adapters. No native authority leaks.
#[derive(Clone)]
pub struct AgentWorkApplicationHandle {
    callback: CallbackHandle,
    projection: Arc<Mutex<Projection>>,
    admission: Arc<AtomicBool>,
}

impl AgentWorkApplicationHandle {
    /// Moves one explicitly model-mapped result only after the original clean
    /// lifecycle and durable terminal ACK. This is user-result content, never
    /// diagnostic data, and is not yet a persisted artifact or factual proof.
    pub fn take_extraction(&self) -> Option<SemanticOwnedExtractionResult> {
        let mut projection = lock(&self.projection);
        if projection.snapshot.phase != AgentWorkApplicationPhase::Succeeded {
            return None;
        }
        projection.extraction.take()
    }
    /// Latest bounded content-free state.
    pub fn snapshot(&self) -> AgentWorkApplicationSnapshot {
        lock(&self.projection).snapshot
    }
    /// Explicit bounded durable recovery inventory; decoded facts are not authority.
    pub fn records(&self) -> Vec<AgentWorkRecord> {
        lock(&self.projection).records.clone()
    }
    /// Removes one exact ordered content-free execution event.
    pub fn take_event(&self) -> Option<AgentWorkEvent> {
        let event = lock(&self.projection).events.pop_front();
        if event.is_some() {
            let _ = self.callback.dispatch(Command::WorkWake);
        }
        event
    }
    /// Transfers at most one run to this application/native-port lifetime.
    pub fn admit(&self, run: PreparedAgentWork) -> Result<(), Box<PreparedAgentWork>> {
        if self.admission.swap(true, Ordering::AcqRel) {
            return Err(Box::new(run));
        }
        let submission = WorkSubmission(Arc::new(Mutex::new(Some(run))), self.projection.clone());
        if self
            .callback
            .dispatch(Command::AdmitWork(submission.clone()))
        {
            Ok(())
        } else if let Some(run) = lock(&submission.0).take() {
            self.admission.store(false, Ordering::Release);
            Err(Box::new(run))
        } else {
            Ok(())
        }
    }
    /// Requests exact-run revocation; this is not native takeover acknowledgement.
    pub fn stop(&self, run: ContextRunId, reason: AgentRuntimeStopReason) -> bool {
        self.send_control(WorkControl::Stop { run, reason })
    }
    /// Reviews an exact durable revision. Accepting requires fresh admission later.
    pub fn review(&self, record: AgentWorkRecord, decision: AgentWorkReviewDecision) -> bool {
        self.send_control(WorkControl::Review { record, decision })
    }
    /// Explicitly reconciles only a retained durable operation, never an action.
    pub fn reconcile(&self) -> bool {
        self.send_control(WorkControl::Reconcile)
    }

    fn send_control(&self, control: WorkControl) -> bool {
        self.callback
            .dispatch(Command::WorkControl(Box::new(WorkCommand {
                projection: self.projection.clone(),
                control,
            })))
    }
}

#[derive(Clone)]
pub struct WorkSubmission(
    Arc<Mutex<Option<PreparedAgentWork>>>,
    Arc<Mutex<Projection>>,
);
impl fmt::Debug for WorkSubmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkSubmission([one-shot])")
    }
}

#[derive(Clone, Copy, Debug)]
pub enum WorkControl {
    Stop {
        run: ContextRunId,
        reason: AgentRuntimeStopReason,
    },
    Review {
        record: AgentWorkRecord,
        decision: AgentWorkReviewDecision,
    },
    Reconcile,
}

#[derive(Clone)]
pub struct WorkCommand {
    projection: Arc<Mutex<Projection>>,
    control: WorkControl,
}
impl fmt::Debug for WorkCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.control.fmt(f)
    }
}

#[derive(Clone)]
pub struct WorkAttachment(Arc<Mutex<Option<ApplicationWork>>>);
impl fmt::Debug for WorkAttachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkAttachment([one-shot])")
    }
}

impl CallbackHandle {
    /// Lazily attaches the durable Work coordinator without creating an agent.
    pub fn attach_work(
        &self,
        journal: Arc<dyn AgentWorkJournalPort>,
        engine: crate::SharedEngine,
    ) -> Option<AgentWorkApplicationHandle> {
        let projection = Arc::new(Mutex::new(Projection {
            extraction: None,
            snapshot: AgentWorkApplicationSnapshot {
                phase: AgentWorkApplicationPhase::Loading,
                run: None,
                failure: None,
                persistence_failure: None,
                last_review: None,
                recovery_audit: None,
            },
            records: Vec::new(),
            events: VecDeque::new(),
        }));
        let handle = AgentWorkApplicationHandle {
            callback: self.clone(),
            projection: projection.clone(),
            admission: Arc::new(AtomicBool::new(false)),
        };
        let actor = ApplicationWork::new(journal, engine, projection, self.clone());
        if self.dispatch(Command::AttachWork(WorkAttachment(Arc::new(Mutex::new(
            Some(actor),
        ))))) {
            Some(handle)
        } else {
            None
        }
    }
}

struct ApplicationWake(CallbackHandle);
impl Wake for ApplicationWake {
    fn wake(self: Arc<Self>) {
        let _ = self.0.dispatch(Command::WorkWake);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.0.dispatch(Command::WorkWake);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

struct ActiveWork {
    pending_event: Option<AgentWorkEvent>,
    needs_review: bool,
    handle: AgentWorkHandle,
    runtime: AgentRuntimeHandle,
    completion: AgentRuntimeCompletion,
    lifecycle: Option<Box<dyn AgentBrowserLifecycle>>,
    outcome: Option<AgentWorkOutcome>,
    native: Option<AgentNativeShutdownProof>,
    lifecycle_clean: Option<bool>,
}

struct RecoveryAuditFlight {
    proof: AgentAuditDeliveryProof,
    result: Arc<Mutex<Option<AgentAuditDeliverySettlement>>>,
    deadline: Instant,
    uncertain: bool,
}

type DurableResult = Result<AgentWorkJournalReply, AgentWorkJournalError>;

struct DurableSlot {
    result: Mutex<Option<DurableResult>>,
    ready: std::sync::Condvar,
}
impl DurableSlot {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: std::sync::Condvar::new(),
        }
    }
    fn put(&self, result: DurableResult) {
        *lock(&self.result) = Some(result);
        self.ready.notify_all();
    }
    fn wait_until(&self, deadline: Instant) {
        let mut result = lock(&self.result);
        while result.is_none() && Instant::now() < deadline {
            result = self
                .ready
                .wait_timeout(result, deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
    }
}

struct DurableFlight {
    request: AgentWorkJournalRequest,
    result: Arc<DurableSlot>,
    deadline: Instant,
    uncertain: bool,
    reconciliations: u8,
    purpose: DurablePurpose,
}

#[derive(Clone, Copy)]
enum DurablePurpose {
    Claim,
    Admit,
    Start,
    Terminal,
    Review,
}

pub(crate) struct ApplicationWork {
    engine: crate::SharedEngine,
    audit: Option<Arc<dyn AgentAuditPort>>,
    unstarted: Option<AgentWorkOutcome>,
    recovery_audit: Option<RecoveryAuditFlight>,
    audit_attempts: u8,
    journal: Arc<dyn AgentWorkJournalPort>,
    projection: Arc<Mutex<Projection>>,
    waker: Waker,
    owner: Option<AgentWorkIncarnation>,
    staged: Option<PreparedAgentWork>,
    active: Option<ActiveWork>,
    flight: Option<DurableFlight>,
    record: Option<AgentWorkRecord>,
    stopping: bool,
    used: bool,
}

impl ApplicationWork {
    pub(crate) fn belongs_to_engine(&self, engine: &crate::SharedEngine) -> bool {
        Arc::ptr_eq(&self.engine, engine)
    }
    pub(crate) fn belongs_to_store(&self, store: &crate::SharedStore) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(&self.journal), Arc::as_ptr(store))
    }
    fn new(
        journal: Arc<dyn AgentWorkJournalPort>,
        engine: crate::SharedEngine,
        projection: Arc<Mutex<Projection>>,
        callback: CallbackHandle,
    ) -> Self {
        Self {
            engine,
            audit: None,
            unstarted: None,
            recovery_audit: None,
            audit_attempts: 0,
            journal,
            projection,
            waker: Waker::from(Arc::new(ApplicationWake(callback))),
            owner: None,
            staged: None,
            active: None,
            flight: None,
            record: None,
            stopping: false,
            used: false,
        }
    }

    pub(crate) fn take_attachment(attachment: &WorkAttachment) -> Option<Self> {
        lock(&attachment.0).take()
    }

    pub(crate) fn initialize(&mut self) {
        self.dispatch(AgentWorkJournalRequest::Claim, DurablePurpose::Claim, 0);
    }

    pub(crate) fn refuse_attachment(&mut self) {
        self.fail(AgentWorkFailure::Contract);
    }

    pub(crate) fn admit(&mut self, submission: WorkSubmission) {
        if !Arc::ptr_eq(&self.projection, &submission.1) {
            return;
        }
        let Some(run) = lock(&submission.0).take() else {
            return;
        };
        if !Arc::ptr_eq(&run.engine, &self.engine)
            || !std::ptr::addr_eq(Arc::as_ptr(&run.audit), Arc::as_ptr(&self.journal))
        {
            self.fail(AgentWorkFailure::Contract);
            return;
        }
        if self.used || self.stopping {
            return;
        }
        self.used = true;
        self.audit = Some(run.audit.clone());
        lock(&self.projection).snapshot.run = Some(run.run);
        self.staged = Some(run);
        self.poll();
    }

    fn dispatch(
        &mut self,
        request: AgentWorkJournalRequest,
        purpose: DurablePurpose,
        reconciliations: u8,
    ) {
        let result = Arc::new(DurableSlot::new());
        let sink = result.clone();
        let waker = self.waker.clone();
        let callback = Box::new(move |value| {
            sink.put(value);
            waker.wake();
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let flight = DurableFlight {
            request,
            result: result.clone(),
            deadline,
            uncertain: false,
            reconciliations,
            purpose,
        };
        self.flight = Some(flight);
        if let Err(error) = self.journal.dispatch(request, callback) {
            result.put(Err(error));
        }
    }

    fn durable_uncertain(&mut self, error: AgentWorkJournalError) {
        if self.active.is_none() {
            self.stopping = true;
            self.abort_staged();
        }
        let mut projection = lock(&self.projection);
        projection.snapshot.phase = AgentWorkApplicationPhase::PersistenceUncertain;
        projection.snapshot.persistence_failure = Some(error);
        drop(projection);
        self.stop_active(AgentRuntimeStopReason::Cancelled);
    }

    fn remember(&mut self, record: AgentWorkRecord) -> bool {
        let mut projection = lock(&self.projection);
        if let Some(previous) = projection
            .records
            .iter_mut()
            .find(|previous| previous.key() == record.key())
        {
            *previous = record;
            true
        } else if projection.records.len() < MAX_DURABLE_AGENT_WORK_RUNS {
            projection.records.push(record);
            true
        } else {
            false
        }
    }

    fn acknowledge(
        &mut self,
        purpose: DurablePurpose,
        request: AgentWorkJournalRequest,
        reply: AgentWorkJournalReply,
    ) -> Result<(), AgentWorkJournalError> {
        match (purpose, request, reply) {
            (
                DurablePurpose::Claim,
                AgentWorkJournalRequest::Claim,
                AgentWorkJournalReply::Claimed { owner, records },
            ) => {
                if records.len() > MAX_DURABLE_AGENT_WORK_RUNS
                    || records
                        .windows(2)
                        .any(|pair| pair[0].key() >= pair[1].key())
                    || records.iter().any(|record| {
                        !record.disposition().is_terminal() && record.incarnation() != owner
                    })
                {
                    return Err(AgentWorkJournalError::Conflict);
                }
                self.owner = Some(owner);
                let mut projection = lock(&self.projection);
                projection.records = records;
                projection.snapshot.phase = if self.stopping {
                    AgentWorkApplicationPhase::Recovery
                } else {
                    AgentWorkApplicationPhase::Ready
                };
                Ok(())
            }
            (
                purpose,
                AgentWorkJournalRequest::CompareAndSet(mutation),
                AgentWorkJournalReply::Record(Some(record)),
            ) if record == mutation.next() => {
                if !self.remember(record) {
                    return Err(AgentWorkJournalError::Capacity);
                }
                if !matches!(purpose, DurablePurpose::Review) {
                    self.record = Some(record);
                }
                match purpose {
                    DurablePurpose::Admit => {
                        let disposition = if self.stopping {
                            AgentWorkDisposition::FailedClosed
                        } else {
                            AgentWorkDisposition::Running
                        };
                        let next = AgentWorkJournalMutation::transition(record, disposition)?;
                        self.dispatch(
                            AgentWorkJournalRequest::CompareAndSet(next),
                            if self.stopping {
                                DurablePurpose::Terminal
                            } else {
                                DurablePurpose::Start
                            },
                            0,
                        );
                    }
                    DurablePurpose::Start => self.activate(),
                    DurablePurpose::Terminal => {
                        if record.disposition() == AgentWorkDisposition::Succeeded {
                            if let Some(AgentWorkOutcome::Succeeded(success)) = self
                                .active
                                .as_mut()
                                .and_then(|active| active.outcome.as_mut())
                            {
                                lock(&self.projection).extraction = success.take_extraction();
                            }
                        }
                        lock(&self.projection).snapshot.phase = match record.disposition() {
                            AgentWorkDisposition::Succeeded => AgentWorkApplicationPhase::Succeeded,
                            AgentWorkDisposition::NeedsApproval => {
                                AgentWorkApplicationPhase::NeedsReview
                            }
                            _ => AgentWorkApplicationPhase::Recovery,
                        };
                        self.abort_staged();
                    }
                    DurablePurpose::Review => {
                        if self.record.is_some_and(|prior| prior.key() == record.key()) {
                            self.record = Some(record);
                        }
                        lock(&self.projection).snapshot.last_review = Some(Ok(record));
                    }
                    DurablePurpose::Claim => return Err(AgentWorkJournalError::Conflict),
                }
                Ok(())
            }
            _ => Err(AgentWorkJournalError::Conflict),
        }
    }

    pub(crate) fn poll(&mut self) {
        if self
            .staged
            .as_ref()
            .is_some_and(|staged| Instant::now() >= staged.deadline)
        {
            self.fail(AgentWorkFailure::Deadline);
            self.abort_staged();
        }
        let mut continue_audit = false;
        if let Some(mut flight) = self.flight.take() {
            let result = lock(&flight.result.result).take();
            match result {
                Some(Ok(reply)) => {
                    if let Err(error) = self.acknowledge(flight.purpose, flight.request, reply) {
                        flight.uncertain = true;
                        self.flight = Some(flight);
                        self.durable_uncertain(error);
                    } else {
                        lock(&self.projection).snapshot.persistence_failure = None;
                    }
                }
                Some(Err(error)) => {
                    flight.uncertain = true;
                    self.flight = Some(flight);
                    self.durable_uncertain(error);
                }
                None => {
                    if !flight.uncertain && Instant::now() >= flight.deadline {
                        flight.uncertain = true;
                        self.durable_uncertain(AgentWorkJournalError::Uncertain);
                    }
                    self.flight = Some(flight);
                }
            }
        }
        if let Some(mut flight) = self.recovery_audit.take() {
            let result = lock(&flight.result).take();
            if let Some(settlement) = result {
                let status = if settlement.proof() != flight.proof {
                    Err(AgentWorkFailure::Audit)
                } else if let Some(recovery) = self.recovery_mut() {
                    match recovery.settle_audit_reconciliation(settlement) {
                        Ok(AgentAuditDeliveryOutcome::Committed) => {
                            recovery.audit_reconciliation_status()
                        }
                        _ => Err(AgentWorkFailure::Audit),
                    }
                } else {
                    Err(AgentWorkFailure::Contract)
                };
                continue_audit = status.is_ok_and(|status| status.pending() > 0);
                lock(&self.projection).snapshot.recovery_audit = Some(status);
            } else {
                if Instant::now() >= flight.deadline {
                    flight.uncertain = true;
                    lock(&self.projection).snapshot.recovery_audit =
                        Some(Err(AgentWorkFailure::Audit));
                }
                self.recovery_audit = Some(flight);
            }
        }
        if let Some(active) = &mut self.active {
            while let Some(event) = active
                .pending_event
                .take()
                .or_else(|| active.handle.take_event())
            {
                if matches!(
                    event.kind(),
                    zephium_agent_controller::AgentWorkEventKind::NeedsHuman(_)
                ) {
                    active.needs_review = true;
                }
                let mut projection = lock(&self.projection);
                if projection.events.len() == zephium_agent_controller::MAX_AGENT_WORK_EVENTS {
                    active.pending_event = Some(event);
                    active
                        .runtime
                        .stop_and_seal(AgentRuntimeStopReason::Cancelled);
                    projection.snapshot.failure = Some(AgentWorkFailure::Backpressure);
                    break;
                }
                projection.events.push_back(event);
            }
            if active.completion.is_stopped() && active.lifecycle.is_some() {
                if let Some(lifecycle) = active.lifecycle.take() {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        lifecycle.shutdown_until(Instant::now() + Duration::from_millis(100))
                    }));
                    match outcome {
                        Ok(AgentBrowserShutdownOutcome::Clean(native)) => {
                            active.native = Some(native);
                            active.lifecycle_clean = Some(true);
                        }
                        _ => {
                            active.lifecycle_clean = Some(false);
                        }
                    }
                }
            }
            // An unclean, deadline-bounded lifecycle join can return before
            // the worker's final Drop publishes its recovery owner. Consume
            // that exact late outcome on completion; never replace or lose it.
            if active.completion.is_stopped()
                && active.lifecycle.is_none()
                && active.outcome.is_none()
            {
                active.outcome = active.handle.take_outcome();
                if let Some(AgentWorkOutcome::Recovery(recovery)) = &active.outcome {
                    lock(&self.projection).snapshot.failure = Some(recovery.failure());
                }
            }
        }
        if continue_audit {
            self.reconcile_audit();
        }
        if self.flight.is_some() {
            return;
        }
        if let Some(owner) = self.owner {
            if self.record.is_none() && (self.staged.is_some() || self.unstarted.is_some()) {
                let admission = if let Some(staged) = &self.staged {
                    staged.controller.journal_admission(owner)
                } else if let Some(AgentWorkOutcome::Recovery(recovery)) = &self.unstarted {
                    recovery.journal_admission(owner)
                } else {
                    Err(AgentWorkFailure::Contract)
                };
                match admission {
                    Ok(mutation) => {
                        lock(&self.projection).snapshot.phase =
                            AgentWorkApplicationPhase::Admitting;
                        self.dispatch(
                            AgentWorkJournalRequest::CompareAndSet(mutation),
                            DurablePurpose::Admit,
                            0,
                        );
                    }
                    Err(error) => self.fail(error),
                }
                return;
            }
        }
        self.persist_execution_end();
    }

    fn activate(&mut self) {
        if self.stopping {
            self.abort_staged();
            return;
        }
        let Some(mut staged) = self.staged.take() else {
            self.fail(AgentWorkFailure::Contract);
            return;
        };
        if self.stopping || Instant::now() >= staged.deadline {
            drop(staged.controller);
            self.unstarted = staged.handle.take_outcome();
            self.fail(AgentWorkFailure::Deadline);
            return;
        }
        staged.handle.set_waker(self.waker.clone());
        let pending = match PendingAgentRuntime::spawn_suspended_with_controller(
            staged.runtime,
            staged.controller,
        ) {
            Ok(pending) => pending,
            Err(_) => {
                self.unstarted = staged.handle.take_outcome();
                self.fail(AgentWorkFailure::Shutdown);
                return;
            }
        };
        let sink = pending.native_event_sink();
        let browser = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (staged.native)(sink)
        })) {
            Ok(Some(browser)) => browser,
            _ => {
                drop(pending);
                self.unstarted = staged.handle.take_outcome();
                self.fail(AgentWorkFailure::Context);
                return;
            }
        };
        let (runtime, completion, lifecycle) = pending.bind_browser_port(browser).into_parts();
        completion.set_waker(self.waker.clone());
        self.active = Some(ActiveWork {
            pending_event: None,
            needs_review: false,
            handle: staged.handle,
            runtime,
            completion,
            lifecycle: Some(lifecycle),
            outcome: None,
            native: None,
            lifecycle_clean: None,
        });
        if let Some(active) = &self.active {
            if active.runtime.start_run().is_err() {
                self.fail(AgentWorkFailure::Shutdown);
                return;
            }
        }
        lock(&self.projection).snapshot.phase = AgentWorkApplicationPhase::Running;
    }

    fn persist_execution_end(&mut self) {
        let Some(record) = self.record else {
            return;
        };
        if record.disposition() != AgentWorkDisposition::Running {
            return;
        }
        let mutation = if let Some(active) = &self.active {
            if active.lifecycle_clean.is_none() {
                return;
            }
            match (&active.outcome, &active.native, active.lifecycle_clean) {
                (Some(AgentWorkOutcome::Succeeded(policy)), Some(native), Some(true))
                    if lock(&self.projection).snapshot.failure.is_none() =>
                {
                    AgentWorkJournalMutation::completed(record, policy.policy_settlement(), native)
                }
                _ => AgentWorkJournalMutation::transition(
                    record,
                    if active.needs_review && !self.stopping {
                        AgentWorkDisposition::NeedsApproval
                    } else {
                        AgentWorkDisposition::RecoveryRequired
                    },
                ),
            }
        } else if self.stopping {
            AgentWorkJournalMutation::transition(record, AgentWorkDisposition::FailedClosed)
        } else {
            return;
        };
        match mutation {
            Ok(mutation) => {
                lock(&self.projection).snapshot.phase = AgentWorkApplicationPhase::Closing;
                self.dispatch(
                    AgentWorkJournalRequest::CompareAndSet(mutation),
                    DurablePurpose::Terminal,
                    0,
                );
            }
            Err(error) => self.durable_uncertain(error),
        }
    }

    fn fail(&mut self, failure: AgentWorkFailure) {
        self.stopping = true;
        let mut projection = lock(&self.projection);
        projection.snapshot.failure = Some(failure);
        projection.snapshot.phase = AgentWorkApplicationPhase::Recovery;
        drop(projection);
        self.stop_active(AgentRuntimeStopReason::Cancelled);
    }

    fn abort_staged(&mut self) {
        if let Some(staged) = self.staged.take() {
            let PreparedAgentWork {
                controller,
                mut handle,
                ..
            } = staged;
            // The existing controller Drop clears credentials/objective and
            // publishes its original recovery owner; do not discard that owner.
            drop(controller);
            self.unstarted = handle.take_outcome();
        }
    }

    fn recovery_mut(&mut self) -> Option<&mut zephium_agent_controller::AgentWorkRecovery> {
        let outcome = if let Some(active) = &mut self.active {
            active.outcome.as_mut()
        } else {
            self.unstarted.as_mut()
        };
        match outcome {
            Some(AgentWorkOutcome::Recovery(recovery)) => Some(recovery),
            _ => None,
        }
    }

    fn stop_active(&self, reason: AgentRuntimeStopReason) {
        if let Some(active) = &self.active {
            active.runtime.stop_and_seal(reason);
        }
    }

    pub(crate) fn control(&mut self, command: WorkCommand) {
        if !Arc::ptr_eq(&self.projection, &command.projection) {
            return;
        }
        match command.control {
            WorkControl::Stop { run, reason } => {
                if lock(&self.projection).snapshot.run == Some(run) {
                    self.stopping = true;
                    self.stop_active(reason);
                    self.abort_staged();
                    if self.active.is_none() {
                        lock(&self.projection)
                            .snapshot
                            .failure
                            .get_or_insert(match reason {
                                AgentRuntimeStopReason::Cancelled => AgentWorkFailure::Cancelled,
                                AgentRuntimeStopReason::HumanTakeover => {
                                    AgentWorkFailure::HumanTakeover
                                }
                                AgentRuntimeStopReason::Suspend => {
                                    AgentWorkFailure::SuspendRequested
                                }
                                AgentRuntimeStopReason::PolicyRevoked => {
                                    AgentWorkFailure::PolicyRevoked
                                }
                            });
                    }
                    if self.flight.is_none() {
                        if let Some(record) = self.record.filter(|record| {
                            record.disposition() == AgentWorkDisposition::NeedsApproval
                        }) {
                            if let Ok(mutation) = AgentWorkJournalMutation::transition(
                                record,
                                AgentWorkDisposition::FailedClosed,
                            ) {
                                self.dispatch(
                                    AgentWorkJournalRequest::CompareAndSet(mutation),
                                    DurablePurpose::Terminal,
                                    0,
                                );
                            }
                        }
                    }
                }
            }
            WorkControl::Review { record, decision } => {
                let result = if self.flight.is_some() || self.recovery_audit.is_some() {
                    Err(AgentWorkJournalError::Unavailable)
                } else if !lock(&self.projection).records.contains(&record)
                    || Some(record.incarnation()) != self.owner
                {
                    Err(AgentWorkJournalError::Conflict)
                } else {
                    AgentWorkJournalMutation::transition(
                        record,
                        match decision {
                            AgentWorkReviewDecision::AcceptFreshAdmission => {
                                AgentWorkDisposition::FreshAdmissionRequired
                            }
                            AgentWorkReviewDecision::Reject => AgentWorkDisposition::Rejected,
                        },
                    )
                };
                match result {
                    Ok(mutation) => self.dispatch(
                        AgentWorkJournalRequest::CompareAndSet(mutation),
                        DurablePurpose::Review,
                        0,
                    ),
                    Err(error) => lock(&self.projection).snapshot.last_review = Some(Err(error)),
                }
            }
            WorkControl::Reconcile => {
                if let Some(flight) = self.flight.take() {
                    if flight.uncertain && flight.reconciliations < 4 {
                        self.dispatch(flight.request, flight.purpose, flight.reconciliations + 1);
                    } else {
                        self.flight = Some(flight);
                    }
                } else {
                    self.reconcile_audit();
                }
            }
        }
        self.poll();
    }

    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        let durable = self
            .flight
            .as_ref()
            .filter(|flight| !flight.uncertain)
            .map(|flight| flight.deadline);
        let audit = self
            .recovery_audit
            .as_ref()
            .filter(|flight| !flight.uncertain)
            .map(|flight| flight.deadline);
        durable
            .into_iter()
            .chain(audit)
            .chain(self.staged.as_ref().map(|staged| staged.deadline))
            .min()
    }

    fn reconcile_audit(&mut self) {
        if self
            .recovery_audit
            .as_ref()
            .is_some_and(|flight| !flight.uncertain)
            || self.audit_attempts >= 4
        {
            return;
        }
        let Some(audit) = self.audit.clone() else {
            return;
        };
        let Some(recovery) = self.recovery_mut() else {
            return;
        };
        let delivery = match recovery.prepare_audit_reconciliation() {
            Ok(Some(delivery)) => delivery,
            Ok(None) => {
                lock(&self.projection).snapshot.recovery_audit =
                    Some(recovery.audit_reconciliation_status());
                return;
            }
            Err(error) => {
                lock(&self.projection).snapshot.recovery_audit = Some(Err(error));
                return;
            }
        };
        self.audit_attempts += 1;
        let proof = delivery.proof();
        let result = Arc::new(Mutex::new(None));
        let sink = result.clone();
        let wake = self.waker.clone();
        self.recovery_audit = Some(RecoveryAuditFlight {
            proof,
            result: result.clone(),
            deadline: Instant::now() + Duration::from_secs(2),
            uncertain: false,
        });
        match audit.append(
            delivery,
            Box::new(move |settlement| {
                *lock(&sink) = Some(settlement);
                wake.wake();
            }),
        ) {
            AgentAuditDispatch::Accepted(expected) if expected == proof => {}
            AgentAuditDispatch::Refused(settlement) => {
                *lock(&result) = Some(settlement);
            }
            _ => {
                if let Some(flight) = &mut self.recovery_audit {
                    flight.uncertain = true;
                }
                lock(&self.projection).snapshot.recovery_audit = Some(Err(AgentWorkFailure::Audit));
            }
        }
    }

    pub(crate) fn begin_shutdown(&mut self) {
        self.stopping = true;
        self.stop_active(AgentRuntimeStopReason::Cancelled);
        self.abort_staged();
    }

    pub(crate) fn shutdown_until(&mut self, deadline: Instant) -> bool {
        self.begin_shutdown();
        if let Some(active) = &mut self.active {
            if let Some(lifecycle) = active.lifecycle.take() {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    lifecycle.shutdown_until(deadline)
                })) {
                    Ok(AgentBrowserShutdownOutcome::Clean(native)) => {
                        active.native = Some(native);
                        active.lifecycle_clean = Some(true);
                    }
                    _ => active.lifecycle_clean = Some(false),
                }
                active.outcome = active.handle.take_outcome();
            }
        }
        // The accepted callback writes its own one-slot result before waking
        // the shell. Wait only on that slot, never on the shell's command queue.
        // This remains bounded by the original process-shutdown deadline.
        for _ in 0..4 {
            self.poll();
            let Some(flight) = &self.flight else {
                break;
            };
            if flight.uncertain || Instant::now() >= deadline {
                break;
            }
            flight.result.wait_until(deadline);
        }
        self.poll();
        self.flight.is_none()
            && self.recovery_audit.is_none()
            && self.unstarted.is_none()
            && self
                .active
                .as_ref()
                .is_none_or(|active| active.lifecycle_clean == Some(true))
    }
}
