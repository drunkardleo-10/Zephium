//! Suspended runtime-worker ownership and fail-closed lifecycle shell.

use std::fmt;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crossbeam_queue::ArrayQueue;
use thiserror::Error;
use tokio::sync::Notify;
use zephium_agentic::{AgentBrowserLifecycle, AgentBrowserPort, AgentBrowserShutdownOutcome};

use crate::mailbox::{
    AgentAuditSink, AgentRuntimeMailbox, AgentRuntimeMailboxWake, SemanticActionSink,
};
use crate::{AgentRuntimeMailboxConfig, AgentRuntimeMailboxFault, NativeEventSink};

const RUN_IDLE: u8 = 0;
const RUN_ACTIVE: u8 = 1;
const RUN_SEALED: u8 = 2;
const STAGED_STOP_NONE: u8 = 0;
const STAGED_STOP_UNEXPECTED_NATIVE_EVENT: u8 = 1;
const STAGED_STOP_TERMINAL_OVERFLOW: u8 = 2;
const STAGED_STOP_SIGNAL_OVERFLOW: u8 = 3;
const STAGED_STOP_CLOSED_MAILBOX: u8 = 4;

static RUNTIME_WORKER_HELD: AtomicBool = AtomicBool::new(false);
// Only one worker can exist process-wide, so one retained ownership bundle is
// a complete, bounded fallback when the best-effort reaper cannot start.
static EMERGENCY_WORKER_REAP: Mutex<Option<RuntimeWorkerOwnership>> = Mutex::new(None);

#[cfg(test)]
static FORCE_REAPER_SPAWN_FAILURE: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static WORKER_EXIT_GATE: WorkerExitGate = WorkerExitGate::new();

/// Minimum bounded control-command capacity for one staged run admission.
pub const MIN_AGENT_RUNTIME_COMMAND_CAPACITY: usize = 1;
/// Largest bounded control-command capacity accepted by this shell.
pub const MAX_AGENT_RUNTIME_COMMAND_CAPACITY: usize = 32;

/// Fixed configuration for one shell-owned agent runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentRuntimeConfig {
    mailbox: AgentRuntimeMailboxConfig,
    command_capacity: usize,
}

impl AgentRuntimeConfig {
    /// Conservative fixed configuration suitable for one production runtime.
    pub const STANDARD: Self = Self {
        mailbox: AgentRuntimeMailboxConfig::STANDARD,
        command_capacity: 4,
    };

    /// Creates one fixed, nonzero mailbox and command capacity configuration.
    pub const fn try_new(
        mailbox: AgentRuntimeMailboxConfig,
        command_capacity: usize,
    ) -> Option<Self> {
        if command_capacity < MIN_AGENT_RUNTIME_COMMAND_CAPACITY
            || command_capacity > MAX_AGENT_RUNTIME_COMMAND_CAPACITY
        {
            None
        } else {
            Some(Self {
                mailbox,
                command_capacity,
            })
        }
    }

    /// Native terminal/signal mailbox capacity configuration.
    pub const fn mailbox(self) -> AgentRuntimeMailboxConfig {
        self.mailbox
    }

    /// Maximum locally admitted control commands retained by the worker.
    pub const fn command_capacity(self) -> usize {
        self.command_capacity
    }
}

/// Content-free failure while starting the suspended worker.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum RuntimeSpawnError {
    /// Another Zephium agent runtime worker has not yet exited.
    #[error("an agent runtime worker is already running")]
    AlreadyRunning,
    /// The worker thread or current-thread Tokio runtime could not start.
    #[error("agent runtime worker is unavailable")]
    WorkerUnavailable,
}

/// Content-free reason this staged shell fail-stopped before controller wiring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentRuntimeStagedStopReason {
    /// A native callback arrived even though this shell dispatched no work.
    UnexpectedNativeEvent,
    /// An obligated native callback could not enter its reserved lane.
    TerminalMailboxOverflow,
    /// An unsolicited native signal could not enter its lane.
    SignalMailboxOverflow,
    /// Callback intake closed before the staged worker could reconcile it.
    ClosedMailbox,
}

impl AgentRuntimeStagedStopReason {
    const fn code(self) -> u8 {
        match self {
            Self::UnexpectedNativeEvent => STAGED_STOP_UNEXPECTED_NATIVE_EVENT,
            Self::TerminalMailboxOverflow => STAGED_STOP_TERMINAL_OVERFLOW,
            Self::SignalMailboxOverflow => STAGED_STOP_SIGNAL_OVERFLOW,
            Self::ClosedMailbox => STAGED_STOP_CLOSED_MAILBOX,
        }
    }

    const fn from_code(code: u8) -> Option<Self> {
        match code {
            STAGED_STOP_UNEXPECTED_NATIVE_EVENT => Some(Self::UnexpectedNativeEvent),
            STAGED_STOP_TERMINAL_OVERFLOW => Some(Self::TerminalMailboxOverflow),
            STAGED_STOP_SIGNAL_OVERFLOW => Some(Self::SignalMailboxOverflow),
            STAGED_STOP_CLOSED_MAILBOX => Some(Self::ClosedMailbox),
            _ => None,
        }
    }

    const fn from_mailbox_fault(fault: AgentRuntimeMailboxFault) -> Self {
        match fault {
            AgentRuntimeMailboxFault::Closed => Self::ClosedMailbox,
            AgentRuntimeMailboxFault::TerminalOverflow => Self::TerminalMailboxOverflow,
            AgentRuntimeMailboxFault::SignalOverflow => Self::SignalMailboxOverflow,
        }
    }
}

/// Strictly increasing, opaque local admission identity for one run.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentRunTicket(NonZeroU64);

impl AgentRunTicket {
    /// Numeric process-local identity for exact runtime joins.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for AgentRunTicket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentRunTicket([redacted])")
    }
}

/// Closed refusal when this staged shell cannot admit another run.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentRunAdmissionRefusal {
    /// A run is already active; the shell deliberately admits one at a time.
    #[error("an agent run is already active")]
    Active,
    /// Cancellation/shutdown permanently sealed run admission.
    #[error("agent runtime run admission is sealed")]
    Sealed,
    /// The fixed worker command mailbox is full.
    #[error("agent runtime command mailbox capacity was exhausted")]
    Capacity,
    /// Ticket identity exhausted and therefore sealed further admissions.
    #[error("agent runtime ticket identity was exhausted")]
    TicketExhausted,
}

/// Content-free projection of staged runtime state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentRunStatus {
    admitted: bool,
    cancelled: bool,
    sealed: bool,
    mailbox_fault: Option<AgentRuntimeMailboxFault>,
    staged_stop_reason: Option<AgentRuntimeStagedStopReason>,
}

impl AgentRunStatus {
    /// Whether the one staged run slot is currently admitted.
    pub const fn admitted(self) -> bool {
        self.admitted
    }

    /// Whether cancellation was requested for the exact current run tree.
    pub const fn cancelled(self) -> bool {
        self.cancelled
    }

    /// Whether future run admission is permanently sealed.
    pub const fn sealed(self) -> bool {
        self.sealed
    }

    /// Any sticky native callback intake fault.
    pub const fn mailbox_fault(self) -> Option<AgentRuntimeMailboxFault> {
        self.mailbox_fault
    }

    /// Why this pre-controller shell fail-stopped, if an unexpected callback arrived.
    pub const fn staged_stop_reason(self) -> Option<AgentRuntimeStagedStopReason> {
        self.staged_stop_reason
    }
}

enum WorkerCommand {
    Start(AgentRunTicket),
}

enum StartupState {
    Suspended,
    Bound(Arc<dyn AgentBrowserPort>),
    Stop,
}

struct StartupGate {
    state: Mutex<StartupState>,
    changed: Notify,
}

impl StartupGate {
    fn new() -> Self {
        Self {
            state: Mutex::new(StartupState::Suspended),
            changed: Notify::new(),
        }
    }

    fn bind(&self, browser: Arc<dyn AgentBrowserPort>) {
        let mut state = recover_lock(&self.state);
        *state = StartupState::Bound(browser);
        self.changed.notify_waiters();
    }

    fn stop(&self) {
        let mut state = recover_lock(&self.state);
        *state = StartupState::Stop;
        self.changed.notify_waiters();
    }

    async fn wait(&self) -> Option<Arc<dyn AgentBrowserPort>> {
        loop {
            let mut notified = std::pin::pin!(self.changed.notified());
            notified.as_mut().enable();
            let decision = {
                let state = recover_lock(&self.state);
                match &*state {
                    StartupState::Suspended => None,
                    StartupState::Bound(browser) => Some(Some(Arc::clone(browser))),
                    StartupState::Stop => Some(None),
                }
            };
            if let Some(startup) = decision {
                return startup;
            }
            notified.await;
        }
    }
}

struct CompletionState {
    stopped: Mutex<bool>,
    changed: Condvar,
}

impl CompletionState {
    fn new() -> Self {
        Self {
            stopped: Mutex::new(false),
            changed: Condvar::new(),
        }
    }

    fn mark_stopped(&self) {
        let mut stopped = recover_lock(&self.stopped);
        *stopped = true;
        self.changed.notify_all();
    }

    fn is_stopped(&self) -> bool {
        *recover_lock(&self.stopped)
    }

    fn wait_until(&self, deadline: Instant) -> bool {
        let mut stopped = recover_lock(&self.stopped);
        while !*stopped {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let timeout = deadline.saturating_duration_since(now);
            let result = match self.changed.wait_timeout(stopped, timeout) {
                Ok(result) => result,
                Err(error) => error.into_inner(),
            };
            stopped = result.0;
            if result.1.timed_out() && !*stopped {
                return false;
            }
        }
        true
    }
}

struct RuntimeInner {
    mailbox: AgentRuntimeMailbox,
    commands: ArrayQueue<WorkerCommand>,
    control_wake: Notify,
    run_state: AtomicU8,
    next_ticket: AtomicU64,
    cancelled: AtomicBool,
    shutdown_requested: AtomicBool,
    staged_stop_reason: AtomicU8,
    completion: CompletionState,
}

impl RuntimeInner {
    fn status(&self) -> AgentRunStatus {
        let run_state = self.run_state.load(Ordering::Acquire);
        AgentRunStatus {
            admitted: run_state == RUN_ACTIVE,
            cancelled: self.cancelled.load(Ordering::Acquire),
            sealed: run_state == RUN_SEALED || self.shutdown_requested.load(Ordering::Acquire),
            mailbox_fault: self.mailbox.fault(),
            staged_stop_reason: AgentRuntimeStagedStopReason::from_code(
                self.staged_stop_reason.load(Ordering::Acquire),
            ),
        }
    }

    fn seal_and_cancel(&self) {
        self.run_state.store(RUN_SEALED, Ordering::Release);
        self.cancelled.store(true, Ordering::Release);
        self.control_wake.notify_waiters();
    }

    fn request_shutdown(&self) {
        self.seal_and_cancel();
        self.shutdown_requested.store(true, Ordering::Release);
        self.mailbox.close();
        self.control_wake.notify_waiters();
    }

    fn record_staged_stop(&self, reason: AgentRuntimeStagedStopReason) {
        let _ = self.staged_stop_reason.compare_exchange(
            STAGED_STOP_NONE,
            reason.code(),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// Cloneable observer for a runtime worker's eventual exit.
#[derive(Clone)]
pub struct AgentRuntimeCompletion {
    inner: Arc<RuntimeInner>,
}

impl AgentRuntimeCompletion {
    /// Whether the worker loop stopped and published its completion signal.
    pub fn is_stopped(&self) -> bool {
        self.inner.completion.is_stopped()
    }
}

impl fmt::Debug for AgentRuntimeCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRuntimeCompletion")
            .field("stopped", &self.is_stopped())
            .finish()
    }
}

/// Cloneable shell handle for admission, cancellation, and callback sinks.
#[derive(Clone)]
pub struct AgentRuntimeHandle {
    inner: Arc<RuntimeInner>,
}

impl AgentRuntimeHandle {
    /// Admits the sole staged run slot and returns its opaque ticket.
    pub fn start_run(&self) -> Result<AgentRunTicket, AgentRunAdmissionRefusal> {
        match self.inner.run_state.compare_exchange(
            RUN_IDLE,
            RUN_ACTIVE,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {}
            Err(RUN_ACTIVE) => return Err(AgentRunAdmissionRefusal::Active),
            Err(_) => return Err(AgentRunAdmissionRefusal::Sealed),
        }

        let ticket = match allocate_ticket(&self.inner.next_ticket) {
            Some(ticket) => ticket,
            None => {
                self.inner.run_state.store(RUN_SEALED, Ordering::Release);
                return Err(AgentRunAdmissionRefusal::TicketExhausted);
            }
        };
        if self
            .inner
            .commands
            .push(WorkerCommand::Start(ticket))
            .is_err()
        {
            let _ = self.inner.run_state.compare_exchange(
                RUN_ACTIVE,
                RUN_IDLE,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            return Err(AgentRunAdmissionRefusal::Capacity);
        }
        self.inner.control_wake.notify_one();
        Ok(ticket)
    }

    /// Permanently seals future admission and requests cancellation.
    pub fn cancel_and_seal(&self) {
        self.inner.seal_and_cancel();
    }

    /// Returns a content-free projection of the staged shell state.
    pub fn status(&self) -> AgentRunStatus {
        self.inner.status()
    }
}

impl fmt::Debug for AgentRuntimeHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRuntimeHandle")
            .field("status", &self.status())
            .finish()
    }
}

/// Move-only suspended runtime before the browser port is transferred.
///
/// The native event sink is available in this state while no browser port has
/// entered the runtime. Because this staged shell dispatches no native work,
/// any callback received before or after binding is consumed, recorded as an
/// unexpected event, and fail-stops the worker rather than becoming orphaned
/// work. A later controller replaces that deliberate safety boundary.
pub struct PendingAgentRuntime {
    inner: Arc<RuntimeInner>,
    gate: Arc<StartupGate>,
    worker: Option<RuntimeWorkerOwnership>,
    bound: bool,
}

impl PendingAgentRuntime {
    /// Starts one named current-thread Tokio worker behind a startup gate.
    pub fn spawn_suspended(config: AgentRuntimeConfig) -> Result<Self, RuntimeSpawnError> {
        let inner = Arc::new(RuntimeInner {
            mailbox: AgentRuntimeMailbox::new(config.mailbox),
            commands: ArrayQueue::new(config.command_capacity),
            control_wake: Notify::new(),
            run_state: AtomicU8::new(RUN_IDLE),
            next_ticket: AtomicU64::new(1),
            cancelled: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
            staged_stop_reason: AtomicU8::new(STAGED_STOP_NONE),
            completion: CompletionState::new(),
        });
        let permit = acquire_worker_permit()?;
        let gate = Arc::new(StartupGate::new());
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let worker_inner = Arc::clone(&inner);
        let worker_gate = Arc::clone(&gate);
        let worker_permit = Arc::clone(&permit);
        let worker = thread::Builder::new()
            .name("zephium-agent-runtime".to_owned())
            .spawn(move || worker_main(worker_inner, worker_gate, startup_sender, worker_permit))
            .map_err(|_| RuntimeSpawnError::WorkerUnavailable)?;
        match startup_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                inner,
                gate,
                worker: Some(RuntimeWorkerOwnership { worker, permit }),
                bound: false,
            }),
            Ok(Err(())) | Err(_) => {
                gate.stop();
                let _ = worker.join();
                Err(RuntimeSpawnError::WorkerUnavailable)
            }
        }
    }

    /// Returns the native event sink before ownership of the browser port moves.
    pub fn native_event_sink(&self) -> NativeEventSink {
        self.inner.mailbox.native_event_sink()
    }

    /// Returns the semantic-action callback sink for the in-crate controller.
    #[allow(dead_code)]
    pub(crate) fn semantic_action_sink(&self) -> SemanticActionSink {
        self.inner.mailbox.semantic_action_sink()
    }

    /// Returns the audit-delivery callback sink for the in-crate controller.
    #[allow(dead_code)]
    pub(crate) fn audit_sink(&self) -> AgentAuditSink {
        self.inner.mailbox.audit_sink()
    }

    /// Transfers the browser port exactly once and releases the worker gate.
    pub fn bind_browser_port(
        mut self,
        browser: Arc<dyn AgentBrowserPort>,
    ) -> AgentRuntimeComposition {
        self.gate.bind(browser);
        self.bound = true;
        let handle = AgentRuntimeHandle {
            inner: Arc::clone(&self.inner),
        };
        let completion = AgentRuntimeCompletion {
            inner: Arc::clone(&self.inner),
        };
        let lifecycle = RuntimeLifecycle {
            inner: Arc::clone(&self.inner),
            worker: self.worker.take(),
        };
        AgentRuntimeComposition {
            handle,
            completion,
            lifecycle: Box::new(lifecycle),
        }
    }
}

impl Drop for PendingAgentRuntime {
    fn drop(&mut self) {
        if self.bound {
            return;
        }
        self.gate.stop();
        self.inner.request_shutdown();
        if let Some(worker) = self.worker.take() {
            worker.join();
        }
    }
}

/// Completed runtime assembly after exact browser-port transfer.
pub struct AgentRuntimeComposition {
    handle: AgentRuntimeHandle,
    completion: AgentRuntimeCompletion,
    lifecycle: Box<dyn AgentBrowserLifecycle>,
}

impl AgentRuntimeComposition {
    /// Consumes the constructor-closed assembly into its three owned ports.
    pub fn into_parts(
        self,
    ) -> (
        AgentRuntimeHandle,
        AgentRuntimeCompletion,
        Box<dyn AgentBrowserLifecycle>,
    ) {
        (self.handle, self.completion, self.lifecycle)
    }
}

impl fmt::Debug for AgentRuntimeComposition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRuntimeComposition")
            .field("handle", &self.handle)
            .field("completion", &self.completion)
            .field("lifecycle", &"AgentBrowserLifecycle([redacted])")
            .finish()
    }
}

struct RuntimeLifecycle {
    inner: Arc<RuntimeInner>,
    worker: Option<RuntimeWorkerOwnership>,
}

impl AgentBrowserLifecycle for RuntimeLifecycle {
    fn shutdown_until(mut self: Box<Self>, deadline: Instant) -> AgentBrowserShutdownOutcome {
        self.inner.request_shutdown();
        if !self.inner.completion.wait_until(deadline) {
            if let Some(worker) = self.worker.take() {
                schedule_reap(worker);
            }
            return AgentBrowserShutdownOutcome::Unclean;
        }
        if let Some(worker) = self.worker.take() {
            worker.join();
        }
        // This is intentionally fail-closed staged integration. The shell has
        // no controller-owned native shutdown/audit proof yet, so it must not
        // claim Clean merely because its worker exited.
        AgentBrowserShutdownOutcome::Unclean
    }
}

impl Drop for RuntimeLifecycle {
    fn drop(&mut self) {
        // If an application drops this staged lifecycle rather than consuming
        // it, it still cannot leave new work or native callback intake open.
        // The reaper joins the cancelled worker, but this remains Unclean:
        // only a later controller can establish the native-zero audit proof
        // required to claim a clean browser shutdown.
        self.inner.request_shutdown();
        if let Some(worker) = self.worker.take() {
            schedule_reap(worker);
        }
    }
}

fn schedule_reap(worker: RuntimeWorkerOwnership) {
    // Deadline expiry must not make the UI-thread caller wait indefinitely.
    // The reaper owns only a worker which has already been cancelled, sealed,
    // and disconnected from callback intake; the public outcome remains
    // Unclean until a later controller can offer an audited native-zero proof.
    let handoff = Arc::new(Mutex::new(Some(worker)));
    let reaper_handoff = Arc::clone(&handoff);
    if spawn_reaper(reaper_handoff).is_err() {
        // Do not trade the caller's deadline for join ownership. The singleton
        // worker permit guarantees this global slot is empty here: a later
        // spawn will join this retained handle only after it is finished, and
        // otherwise refuses admission without starting another worker.
        let worker = take_worker_ownership(&handoff);
        let mut emergency = recover_lock(&EMERGENCY_WORKER_REAP);
        *emergency = worker;
    }
}

fn spawn_reaper(handoff: Arc<Mutex<Option<RuntimeWorkerOwnership>>>) -> Result<(), ()> {
    #[cfg(test)]
    if FORCE_REAPER_SPAWN_FAILURE.load(Ordering::Acquire) {
        return Err(());
    }
    thread::Builder::new()
        .name("zephium-agent-runtime-reaper".to_owned())
        .spawn(move || {
            if let Some(worker) = take_worker_ownership(&handoff) {
                worker.join();
            }
        })
        .map(|_| ())
        .map_err(|_| ())
}

/// Starts a suspended runtime without transferring a browser port.
pub fn spawn_suspended(
    config: AgentRuntimeConfig,
) -> Result<PendingAgentRuntime, RuntimeSpawnError> {
    PendingAgentRuntime::spawn_suspended(config)
}

fn worker_main(
    inner: Arc<RuntimeInner>,
    gate: Arc<StartupGate>,
    startup_sender: mpsc::SyncSender<Result<(), ()>>,
    permit: Arc<WorkerPermit>,
) {
    // This is intentionally scoped across every actual worker operation,
    // including startup failure and post-loop thread teardown. The permit
    // therefore cannot release merely because lifecycle completion became
    // observable; it releases only when this worker thread returns.
    let _permit_until_worker_exit = permit;
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            let _ = startup_sender.send(Err(()));
            inner.completion.mark_stopped();
            return;
        }
    };
    let _ = startup_sender.send(Ok(()));
    runtime.block_on(worker_loop(inner, gate));
}

async fn worker_loop(inner: Arc<RuntimeInner>, gate: Arc<StartupGate>) {
    // Keeping the move-only browser authority in this one worker is the
    // ownership seam. The controller added in the next stage performs every
    // port operation. Until then, every callback is unexpected and forces a
    // recorded fail-stop after the mailbox has consumed its retained authority.
    let mut browser: Option<Arc<dyn AgentBrowserPort>> = None;
    loop {
        let mut notified = std::pin::pin!(inner.control_wake.notified());
        notified.as_mut().enable();
        if inner.shutdown_requested.load(Ordering::Acquire) {
            inner.mailbox.close_and_drain().await;
            break;
        }
        while let Some(command) = inner.commands.pop() {
            match command {
                WorkerCommand::Start(ticket) => {
                    let _ticket = ticket;
                }
            }
        }
        if inner.shutdown_requested.load(Ordering::Acquire) {
            inner.mailbox.close_and_drain().await;
            break;
        }
        let mailbox_wait = inner.mailbox.wait_for_work();
        tokio::pin!(mailbox_wait);
        if browser.is_none() {
            let startup_wait = gate.wait();
            tokio::pin!(startup_wait);
            tokio::select! {
                biased;
                _ = &mut notified => {}
                wake = &mut mailbox_wait => {
                    fail_staged_mailbox_wake(&inner, wake).await;
                    break;
                }
                startup = &mut startup_wait => match startup {
                    Some(port) => browser = Some(port),
                    None => {
                        inner.request_shutdown();
                        inner.mailbox.close_and_drain().await;
                        break;
                    }
                },
            }
        } else {
            tokio::select! {
                biased;
                _ = &mut notified => {}
                wake = &mut mailbox_wait => {
                    fail_staged_mailbox_wake(&inner, wake).await;
                    break;
                }
            }
        }
    }
    drop(browser);
    #[cfg(test)]
    WORKER_EXIT_GATE.wait();
    inner.completion.mark_stopped();
}

async fn fail_staged_mailbox_wake(inner: &RuntimeInner, wake: AgentRuntimeMailboxWake) {
    // `wait_for_work` deliberately reports terminal debt before a sticky
    // fault so the eventual drain remains terminal-first. Its wake label is
    // not, however, the complete diagnostic state: saturation may already
    // have made successful execution impossible. Prefer that sticky fault so
    // a terminal overflow is never misreported as an ordinary callback.
    let reason = match inner.mailbox.fault() {
        Some(fault) => AgentRuntimeStagedStopReason::from_mailbox_fault(fault),
        None => match wake {
            AgentRuntimeMailboxWake::TerminalReady | AgentRuntimeMailboxWake::SignalReady => {
                AgentRuntimeStagedStopReason::UnexpectedNativeEvent
            }
            AgentRuntimeMailboxWake::Fault(fault) => {
                AgentRuntimeStagedStopReason::from_mailbox_fault(fault)
            }
        },
    };
    inner.record_staged_stop(reason);
    inner.request_shutdown();
    inner.mailbox.close_and_drain().await;
}

fn allocate_ticket(counter: &AtomicU64) -> Option<AgentRunTicket> {
    loop {
        let current = counter.load(Ordering::Acquire);
        let ticket = NonZeroU64::new(current)?;
        let next = current.checked_add(1)?;
        match counter.compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Some(AgentRunTicket(ticket)),
            Err(_) => continue,
        }
    }
}

fn recover_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(value) => value,
        Err(error) => error.into_inner(),
    }
}

fn take_worker_ownership(
    handoff: &Mutex<Option<RuntimeWorkerOwnership>>,
) -> Option<RuntimeWorkerOwnership> {
    recover_lock(handoff).take()
}

struct RuntimeWorkerOwnership {
    worker: JoinHandle<()>,
    permit: Arc<WorkerPermit>,
}

impl RuntimeWorkerOwnership {
    fn is_finished(&self) -> bool {
        self.worker.is_finished()
    }

    fn join(self) {
        let _ = self.worker.join();
        // Keep the lifecycle owner's permit alive until the actual worker has
        // been joined. The worker itself holds the other Arc until its thread
        // returns, so either ordering preserves process-wide exclusion.
        let _permit_until_joined = self.permit;
    }
}

#[cfg(test)]
struct WorkerExitGate {
    held: Mutex<bool>,
    changed: Condvar,
}

#[cfg(test)]
impl WorkerExitGate {
    const fn new() -> Self {
        Self {
            held: Mutex::new(false),
            changed: Condvar::new(),
        }
    }

    fn hold(&self) {
        *recover_lock(&self.held) = true;
    }

    fn release(&self) {
        *recover_lock(&self.held) = false;
        self.changed.notify_all();
    }

    fn wait(&self) {
        let mut held = recover_lock(&self.held);
        while *held {
            held = match self.changed.wait(held) {
                Ok(held) => held,
                Err(error) => error.into_inner(),
            };
        }
    }
}

struct WorkerPermit;

impl Drop for WorkerPermit {
    fn drop(&mut self) {
        RUNTIME_WORKER_HELD.store(false, Ordering::Release);
    }
}

fn acquire_worker_permit() -> Result<Arc<WorkerPermit>, RuntimeSpawnError> {
    let completed_worker = {
        let mut emergency = recover_lock(&EMERGENCY_WORKER_REAP);
        match emergency.as_ref() {
            Some(worker) if worker.is_finished() => emergency.take(),
            Some(_) => {
                // The retained lifecycle permit remains held while this old
                // worker finishes. Preserve its join ownership and reject this
                // attempt; callers may retry without a second worker starting.
                return Err(RuntimeSpawnError::AlreadyRunning);
            }
            None => None,
        }
    };
    if let Some(worker) = completed_worker {
        worker.join();
    }
    if RUNTIME_WORKER_HELD
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(RuntimeSpawnError::AlreadyRunning);
    }
    Ok(Arc::new(WorkerPermit))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use crate::{MIN_AGENT_RUNTIME_SIGNAL_CAPACITY, MIN_AGENT_RUNTIME_TERMINAL_CAPACITY};
    use zephium_agentic::{
        ContextCookieTransferRequest, ContextDispatch, ContextNativeEvent, ContextNativeRequest,
        ContextPortFailure, ContextResourceAuditId, ContextResourceAuditSettlement,
        ContextShutdownDispatch, SemanticActionNativeCompletion, SemanticActionNativeRequest,
        SemanticRuntimeInvocation, SemanticScreenshotNativeCompletion,
        SemanticScreenshotNativeRequest,
    };

    use super::*;

    static RUNTIME_TEST_SERIALIZER: Mutex<()> = Mutex::new(());

    struct RecordingPort {
        calls: AtomicUsize,
    }

    impl RecordingPort {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::Acquire)
        }
    }

    impl AgentBrowserPort for RecordingPort {
        fn dispatch(&self, _request: ContextNativeRequest) -> ContextDispatch {
            self.calls.fetch_add(1, Ordering::AcqRel);
            ContextDispatch::Unsupported
        }

        fn transfer_cookies(&self, _request: ContextCookieTransferRequest) -> ContextDispatch {
            self.calls.fetch_add(1, Ordering::AcqRel);
            ContextDispatch::Unsupported
        }

        fn audit_resources(&self, _audit: ContextResourceAuditId) -> ContextDispatch {
            self.calls.fetch_add(1, Ordering::AcqRel);
            ContextDispatch::Unsupported
        }

        fn seal_for_shutdown(&self, _audit: ContextResourceAuditId) -> ContextShutdownDispatch {
            self.calls.fetch_add(1, Ordering::AcqRel);
            ContextShutdownDispatch::SealedWithoutAudit(ContextPortFailure::Shutdown)
        }

        fn invoke_semantic(&self, _invocation: SemanticRuntimeInvocation) -> ContextDispatch {
            self.calls.fetch_add(1, Ordering::AcqRel);
            ContextDispatch::Unsupported
        }

        fn execute_semantic_action(
            &self,
            _request: SemanticActionNativeRequest,
            _completion: SemanticActionNativeCompletion,
        ) -> ContextDispatch {
            self.calls.fetch_add(1, Ordering::AcqRel);
            ContextDispatch::Unsupported
        }

        fn capture_semantic_screenshot(
            &self,
            _request: SemanticScreenshotNativeRequest,
            _completion: SemanticScreenshotNativeCompletion,
        ) -> ContextDispatch {
            self.calls.fetch_add(1, Ordering::AcqRel);
            ContextDispatch::Unsupported
        }
    }

    fn runtime_test_guard() -> MutexGuard<'static, ()> {
        recover_lock(&RUNTIME_TEST_SERIALIZER)
    }

    struct ForcedReaperSpawnFailure;

    impl ForcedReaperSpawnFailure {
        fn enable() -> Self {
            FORCE_REAPER_SPAWN_FAILURE.store(true, Ordering::Release);
            Self
        }
    }

    impl Drop for ForcedReaperSpawnFailure {
        fn drop(&mut self) {
            FORCE_REAPER_SPAWN_FAILURE.store(false, Ordering::Release);
        }
    }

    struct HeldWorkerExitGate;

    impl HeldWorkerExitGate {
        fn enable() -> Self {
            WORKER_EXIT_GATE.hold();
            Self
        }

        fn release(self) {
            WORKER_EXIT_GATE.release();
        }
    }

    impl Drop for HeldWorkerExitGate {
        fn drop(&mut self) {
            WORKER_EXIT_GATE.release();
        }
    }

    fn wait_stopped(completion: &AgentRuntimeCompletion) {
        for _ in 0..40 {
            if completion.is_stopped() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(completion.is_stopped());
    }

    fn spawn_after_true_worker_exit() -> PendingAgentRuntime {
        for _ in 0..40 {
            match spawn_suspended(AgentRuntimeConfig::STANDARD) {
                Ok(next) => return next,
                Err(RuntimeSpawnError::AlreadyRunning) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(RuntimeSpawnError::WorkerUnavailable) => {
                    panic!("test worker infrastructure is unavailable")
                }
            }
        }
        panic!("worker permit did not release after the worker exited")
    }

    #[test]
    fn suspended_worker_does_not_touch_a_port_and_drop_before_bind_joins() {
        let _guard = runtime_test_guard();
        let start = Instant::now();
        let pending = spawn_suspended(AgentRuntimeConfig::STANDARD).expect("worker starts");
        let native = pending.native_event_sink();
        drop(native);
        drop(pending);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn ownership_admits_one_run_and_shutdown_never_claims_clean() {
        let _guard = runtime_test_guard();
        let port = Arc::new(RecordingPort {
            calls: AtomicUsize::new(0),
        });
        let pending = spawn_suspended(AgentRuntimeConfig::STANDARD).expect("worker starts");
        let composition = pending.bind_browser_port(Arc::clone(&port) as Arc<dyn AgentBrowserPort>);
        let (handle, completion, lifecycle) = composition.into_parts();
        let ticket = handle.start_run().expect("first run admission");
        assert_eq!(ticket.get(), 1);
        assert_eq!(handle.start_run(), Err(AgentRunAdmissionRefusal::Active));
        handle.cancel_and_seal();
        let status = handle.status();
        assert!(!status.admitted());
        assert!(status.cancelled());
        assert!(status.sealed());
        assert_eq!(port.calls(), 0);
        let outcome = lifecycle.shutdown_until(Instant::now() + Duration::from_secs(1));
        assert!(matches!(outcome, AgentBrowserShutdownOutcome::Unclean));
        assert!(completion.is_stopped());
        assert_eq!(port.calls(), 0);
    }

    #[test]
    fn expired_deadline_is_unclean_and_stop_request_remains_sticky() {
        let _guard = runtime_test_guard();
        let port = Arc::new(RecordingPort {
            calls: AtomicUsize::new(0),
        });
        let pending = spawn_suspended(AgentRuntimeConfig::STANDARD).expect("worker starts");
        let composition = pending.bind_browser_port(port as Arc<dyn AgentBrowserPort>);
        let (handle, completion, lifecycle) = composition.into_parts();
        let outcome = lifecycle.shutdown_until(Instant::now());
        assert!(matches!(outcome, AgentBrowserShutdownOutcome::Unclean));
        assert!(handle.status().sealed());
        wait_stopped(&completion);
        let next = spawn_after_true_worker_exit();
        drop(next);
    }

    #[test]
    fn failed_reaper_spawn_retains_the_worker_without_extending_the_deadline() {
        let _guard = runtime_test_guard();
        let _forced_failure = ForcedReaperSpawnFailure::enable();
        let worker_exit_gate = HeldWorkerExitGate::enable();
        let port = Arc::new(RecordingPort {
            calls: AtomicUsize::new(0),
        });
        let pending = spawn_suspended(AgentRuntimeConfig::STANDARD).expect("worker starts");
        let composition = pending.bind_browser_port(port as Arc<dyn AgentBrowserPort>);
        let (_handle, completion, lifecycle) = composition.into_parts();
        let start = Instant::now();
        let outcome = lifecycle.shutdown_until(Instant::now());
        assert!(matches!(outcome, AgentBrowserShutdownOutcome::Unclean));
        assert!(start.elapsed() < Duration::from_secs(1));
        // The lifecycle's Arc travels into the emergency bundle before this
        // method returns, so admission remains closed across the handoff even
        // if the worker races to exit immediately afterwards.
        assert!(RUNTIME_WORKER_HELD.load(Ordering::Acquire));
        assert!(recover_lock(&EMERGENCY_WORKER_REAP).is_some());
        assert!(!completion.is_stopped());

        worker_exit_gate.release();
        wait_stopped(&completion);
        assert!(RUNTIME_WORKER_HELD.load(Ordering::Acquire));
        let next = spawn_after_true_worker_exit();
        assert!(recover_lock(&EMERGENCY_WORKER_REAP).is_none());
        drop(next);
    }

    #[test]
    fn public_debug_is_content_free() {
        let _guard = runtime_test_guard();
        let pending = spawn_suspended(AgentRuntimeConfig::STANDARD).expect("worker starts");
        let debug = format!("{:#?}", AgentRuntimeConfig::STANDARD);
        assert!(!debug.contains("http"));
        drop(pending);
    }

    #[test]
    fn process_wide_worker_permit_covers_pending_bound_and_true_worker_exit() {
        let _guard = runtime_test_guard();
        let pending = spawn_suspended(AgentRuntimeConfig::STANDARD).expect("first worker starts");
        assert!(matches!(
            spawn_suspended(AgentRuntimeConfig::STANDARD),
            Err(RuntimeSpawnError::AlreadyRunning)
        ));

        let port = Arc::new(RecordingPort {
            calls: AtomicUsize::new(0),
        });
        let composition = pending.bind_browser_port(port as Arc<dyn AgentBrowserPort>);
        let (_handle, completion, lifecycle) = composition.into_parts();
        assert!(matches!(
            spawn_suspended(AgentRuntimeConfig::STANDARD),
            Err(RuntimeSpawnError::AlreadyRunning)
        ));
        let outcome = lifecycle.shutdown_until(Instant::now() + Duration::from_secs(1));
        assert!(matches!(outcome, AgentBrowserShutdownOutcome::Unclean));
        assert!(completion.is_stopped());

        let next = spawn_after_true_worker_exit();
        drop(next);
    }

    #[test]
    fn terminal_overflow_reason_wins_while_terminal_debt_is_ready() {
        let config = AgentRuntimeMailboxConfig::try_new(
            MIN_AGENT_RUNTIME_TERMINAL_CAPACITY,
            MIN_AGENT_RUNTIME_SIGNAL_CAPACITY,
        )
        .expect("published test capacities are valid");
        let mailbox = AgentRuntimeMailbox::new(config);
        let sink = mailbox.native_event_sink();
        for value in 1..=MIN_AGENT_RUNTIME_TERMINAL_CAPACITY {
            let audit =
                ContextResourceAuditId::new(value as u64).expect("test audit identity is nonzero");
            let event = ContextNativeEvent::ResourceAuditSettled(
                ContextResourceAuditSettlement::new(audit, Err(ContextPortFailure::Shutdown)),
            );
            assert!(sink.publish(event).is_ok());
        }
        let overflow_audit =
            ContextResourceAuditId::new((MIN_AGENT_RUNTIME_TERMINAL_CAPACITY + 1) as u64)
                .expect("test overflow audit identity is nonzero");
        let overflow_event = ContextNativeEvent::ResourceAuditSettled(
            ContextResourceAuditSettlement::new(overflow_audit, Err(ContextPortFailure::Shutdown)),
        );
        assert_eq!(
            sink.publish(overflow_event),
            Err(AgentRuntimeMailboxFault::TerminalOverflow)
        );

        let inner = RuntimeInner {
            mailbox,
            commands: ArrayQueue::new(MIN_AGENT_RUNTIME_COMMAND_CAPACITY),
            control_wake: Notify::new(),
            run_state: AtomicU8::new(RUN_IDLE),
            next_ticket: AtomicU64::new(1),
            cancelled: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
            staged_stop_reason: AtomicU8::new(STAGED_STOP_NONE),
            completion: CompletionState::new(),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("test runtime starts");
        runtime.block_on(fail_staged_mailbox_wake(
            &inner,
            AgentRuntimeMailboxWake::TerminalReady,
        ));

        assert_eq!(
            inner.status().staged_stop_reason(),
            Some(AgentRuntimeStagedStopReason::TerminalMailboxOverflow)
        );
        assert!(inner.status().cancelled());
        assert!(inner.status().sealed());
        assert!(inner.mailbox.try_pop().is_none());
    }

    #[test]
    fn unexpected_real_native_event_is_consumed_and_fail_stops_the_staged_shell() {
        let _guard = runtime_test_guard();
        let port = Arc::new(RecordingPort {
            calls: AtomicUsize::new(0),
        });
        let pending = spawn_suspended(AgentRuntimeConfig::STANDARD).expect("worker starts");
        let sink = pending.native_event_sink();
        let audit = ContextResourceAuditId::new(1).expect("nonzero audit identity");
        let event = ContextNativeEvent::ResourceAuditSettled(ContextResourceAuditSettlement::new(
            audit,
            Err(ContextPortFailure::Shutdown),
        ));
        let result = sink.publish(event);
        assert!(result.is_ok() || result == Err(AgentRuntimeMailboxFault::Closed));
        let composition = pending.bind_browser_port(Arc::clone(&port) as Arc<dyn AgentBrowserPort>);
        let (handle, completion, lifecycle) = composition.into_parts();
        wait_stopped(&completion);
        let status = handle.status();
        assert!(status.cancelled());
        assert!(status.sealed());
        assert_eq!(
            status.staged_stop_reason(),
            Some(AgentRuntimeStagedStopReason::UnexpectedNativeEvent)
        );
        assert_eq!(port.calls(), 0);
        drop(lifecycle);
        let next = spawn_after_true_worker_exit();
        drop(next);
    }

    #[test]
    fn dropping_lifecycle_reaps_the_worker_and_releases_the_permit() {
        let _guard = runtime_test_guard();
        let port = Arc::new(RecordingPort {
            calls: AtomicUsize::new(0),
        });
        let pending = spawn_suspended(AgentRuntimeConfig::STANDARD).expect("worker starts");
        let composition = pending.bind_browser_port(port as Arc<dyn AgentBrowserPort>);
        let (_handle, completion, lifecycle) = composition.into_parts();
        drop(lifecycle);
        wait_stopped(&completion);
        let next = spawn_after_true_worker_exit();
        drop(next);
    }
}
