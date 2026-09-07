//! One closed actual-app retained-controller witness. No second agent loop,
//! product admission, durable result, successor, or replacement native runtime.
use std::{
    cell::{Cell, RefCell},
    mem::ManuallyDrop,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use zephium_agent_controller::*;
use zephium_agent_runtime::*;
use zephium_agentic::*;
use zephium_app::retained_work_probe::RetainedWorkProbeOwner;
use zephium_core::{
    blocker::{ContentPolicyGeneration, ContentRuleDigest, ContentRules},
    ports::engine::{ContentRuleSettlement, Engine, EngineEvent, NativeDispatch},
};
use zephium_engine::{
    ForegroundAdmissionWake, ForegroundRenderingAdmission, ForegroundRenderingWitnessReport,
    WebviewEngine, WorkResourceRenderingProbe,
};
use zephium_store::SqliteStore;

#[path = "retained_qualification_task.rs"]
mod task;
const TOTAL: Duration = Duration::from_secs(150);
const CLEANUP: Duration = Duration::from_secs(5);
// Fixed grace for already-admitted cleanup only. Derived from the original
// cleanup deadline; neither repeated failures nor late events can renew it.
const HARD_CLEANUP_GRACE: Duration = Duration::from_secs(5);
type ReleaseCallback = Box<dyn FnOnce(bool) + Send>;
type ReleaseSlot = Arc<Mutex<Option<ReleaseCallback>>>;

// Intermediate weak-window drainage preserves the one original callback. The
// final invocation remains inside the exact native task's panic/debt boundary.
fn deliver_snapshot_retirement(
    slot: &ReleaseSlot,
    state: ForegroundRenderingState,
    signal: &Signal,
) {
    if state == ForegroundRenderingState::Retiring {
        return;
    }
    let (callback, healthy) = match slot.lock() {
        Ok(mut slot) => (slot.take(), true),
        Err(poisoned) => {
            signal.failed.store(true, Ordering::Release);
            (poisoned.into_inner().take(), false)
        }
    };
    if let Some(callback) = callback {
        callback(healthy && state == ForegroundRenderingState::Retired);
    } else {
        panic!("missing exact snapshot retirement callback");
    }
}

fn begin_cleanup_deadline(
    cleaning: &mut bool,
    deadline: &mut Instant,
    now: Instant,
) -> Result<(), &'static str> {
    if !*cleaning {
        *cleaning = true;
        *deadline = now.checked_add(CLEANUP).ok_or("cleanup_deadline")?;
    }
    Ok(())
}
/// Only reviewed content-free controller metadata and bounded witness counters.
pub enum RetainedProbeTrace {
    Configured,
    Event(AgentWorkEvent),
    Outcome {
        state: &'static str,
        failure: Option<AgentWorkFailure>,
    },
    Observation {
        nodes: u16,
        complete: bool,
        current_document: bool,
        frame_boundaries: usize,
        markers: [bool; 5],
    },
    Closure {
        accepted: bool,
        fixture_mapping_verified: bool,
        presentation_retired: bool,
        scoped_worker_drained: bool,
        original_resource_retired: bool,
    },
    Totals {
        model_calls: u32,
        input_tokens: u64,
        output_tokens: u64,
        cost_micro_usd: u64,
    },
}
#[derive(Default)]
struct Totals {
    calls: u32,
    last_call: u64,
    input: u64,
    output: u64,
    cost: u64,
}
impl Totals {
    fn observe(&mut self, kind: AgentWorkEventKind) -> Result<(), &'static str> {
        if let AgentWorkEventKind::ModelSettled {
            call,
            input_tokens,
            output_tokens,
            cost_micro_usd,
            ..
        } = kind
        {
            if call.get() <= self.last_call || self.calls >= 8 {
                return Err("trace_call_binding");
            }
            self.calls += 1;
            self.last_call = call.get();
            self.input = self
                .input
                .checked_add(input_tokens)
                .ok_or("trace_token_overflow")?;
            self.output = self
                .output
                .checked_add(output_tokens)
                .ok_or("trace_token_overflow")?;
            self.cost = self
                .cost
                .checked_add(cost_micro_usd)
                .ok_or("trace_cost_overflow")?;
        }
        Ok(())
    }
}
type Trace = Arc<dyn Fn(RetainedProbeTrace) -> bool + Send + Sync>;
#[derive(Clone, Copy, Eq, PartialEq)]
enum Render {
    Acquire,
    Poll,
    SnapshotRetire,
    CleanupRetire,
}
enum Reply {
    Policy(bool),
    Render(Render, ForegroundRenderingState),
    SnapshotRetire,
}
struct Signal {
    profile: AgentWorkProfileId,
    generation: ContentPolicyGeneration,
    tx: mpsc::SyncSender<Reply>,
    failed: AtomicBool,
    alive: AtomicBool,
    shutdown: AtomicBool,
}
impl Signal {
    fn send(&self, reply: Reply) -> bool {
        if !self.alive.load(Ordering::Acquire) {
            self.failed.store(true, Ordering::Release);
            return false;
        }
        if self.shutdown.load(Ordering::Acquire) {
            // The failed diagnostic has handed all original owners to ordinary
            // shutdown. These content-free notifications grant no authority and
            // have no further driver consumer; original native completions and
            // snapshot-release accounting still execute before this method.
            return true;
        }
        if self.tx.try_send(reply).is_err() {
            self.failed.store(true, Ordering::Release);
            false
        } else {
            true
        }
    }
}

/// One failed diagnostic may retain its exact owners through ordinary shutdown.
/// Unproven owners have no implicit destructor at thread exit: a late native
/// callback must never target a receiver already discarded by a failure report.
struct ShutdownRetention<T>(Option<ManuallyDrop<T>>);
impl<T> ShutdownRetention<T> {
    const fn new() -> Self {
        Self(None)
    }
    fn retain(&mut self, owner: T) {
        assert!(self.0.is_none(), "duplicate diagnostic shutdown owner");
        self.0 = Some(ManuallyDrop::new(owner));
    }
    fn release_if(&mut self, ready: impl FnOnce(&mut T) -> bool) -> bool {
        if self.0.as_mut().is_some_and(|owner| !ready(owner)) {
            return false;
        }
        if let Some(owner) = self.0.take() {
            drop(ManuallyDrop::into_inner(owner));
        }
        true
    }
}
static POLICY: Mutex<Option<Arc<Signal>>> = Mutex::new(None);
thread_local! {
    static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) };
    static USED: Cell<bool> = const { Cell::new(false) };
    static EXPECTED_RESOURCE: RefCell<Option<WorkBrowserResourceJoin>> = const { RefCell::new(None) };
    static SHUTDOWN_OWNER: RefCell<ShutdownRetention<NativeShutdownOwner>> = const { RefCell::new(ShutdownRetention::new()) };
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Phase {
    Preparing,
    Construct,
    Rendering,
    Acquire,
    Actor,
    Stopping,
    Joining,
    Retire,
    Destroy,
    NativeDrain,
    Seal,
    Done,
}

fn cleanup_window(now: Instant, deadline: Instant, cleaning: bool) -> Result<bool, &'static str> {
    if now < deadline {
        return Ok(false);
    }
    if !cleaning {
        return Err("deadline");
    }
    if now
        >= deadline
            .checked_add(HARD_CLEANUP_GRACE)
            .ok_or("hard_cleanup_deadline")?
    {
        return Err("hard_cleanup_deadline");
    }
    Ok(true)
}

fn record_cleanup_failure(
    phase: &mut Phase,
    failure: &mut Option<&'static str>,
    reason: &'static str,
) {
    failure.get_or_insert(reason);
    if reason == "hard_cleanup_deadline" {
        *phase = Phase::Done;
    }
}

// Same original application owner and single audit used by Driver. Factored so
// delayed reporter/audit order can be tested without starting AppKit or a model.
fn progress_native_close(
    owner: &mut RetainedWorkProbeOwner,
    phase: &mut Phase,
    now: AgentPolicyInstant,
) -> Result<bool, &'static str> {
    let mut clean = false;
    if let Some(event) = owner.poll_lifecycle(now)? {
        match (*phase, event) {
            (Phase::Destroy, WorkBrowserResourceEvent::Destroyed(_)) => *phase = Phase::NativeDrain,
            _ => return Err("lifecycle_phase"),
        }
    }
    while let Some(event) = owner.poll_native_event()? {
        let ContextNativeEvent::ShutdownAuditSettled(settlement) = event else {
            return Err("native_event_phase");
        };
        if *phase != Phase::Seal || settlement.audit().get() != 1 {
            return Err("native_audit_binding");
        }
        let counts = settlement.outcome().map_err(|_| "native_audit")?.counts();
        let empty = [
            counts.known_bindings,
            counts.resident_views,
            counts.owned_reservations,
            counts.borrowed_leases,
            counts.visible_surfaces,
            counts.suspended_views,
            counts.pending_operations,
            counts.pending_captures,
            counts.queued_tasks,
        ]
        .into_iter()
        .all(|count| count == 0);
        if !empty || !owner.locally_retired() {
            return Err("native_resource_debt");
        }
        *phase = Phase::Done;
        clean = true;
    }
    if *phase == Phase::NativeDrain
        && owner.poll_seal(ContextResourceAuditId::new(1).ok_or("audit")?)?
    {
        *phase = Phase::Seal;
    }
    Ok(clean)
}
struct Driver {
    engine: Arc<WebviewEngine>,
    store: Arc<SqliteStore>,
    owner: RetainedWorkProbeOwner,
    admission: ForegroundRenderingAdmission,
    fixture: Option<FixtureServer>,
    target: ContextNavigationTarget,
    signal: Arc<Signal>,
    rx: mpsc::Receiver<Reply>,
    timer: Option<ForegroundAdmissionWake>,
    credential_job: Option<JoinHandle<Result<AgentProviderCredential, &'static str>>>,
    credential: Option<AgentProviderCredential>,
    policy_ready: bool,
    phase: Phase,
    render: Option<Arc<WorkResourceRenderingProbe>>,
    render_pending: Option<Render>,
    render_next: Option<Render>,
    snapshot_release: ReleaseSlot,
    release_requested: Arc<AtomicBool>,
    started: Instant,
    deadline: Instant,
    issued: AgentPolicyInstant,
    expires: AgentPolicyInstant,
    run: ContextRunId,
    result: Option<AgentWorkRetainedHandle>,
    control: Option<AgentRuntimeHandle>,
    completion: Option<AgentRuntimeCompletion>,
    lifecycle: Option<AgentRuntimeScopedLifecycle>,
    join: Option<JoinHandle<AgentRuntimeScopedDrain>>,
    scoped: Option<AgentRuntimeScopedDrained>,
    outcome: Option<AgentWorkRetainedOutcome>,
    expected: Arc<Mutex<Option<task::Expected>>>,
    sample: Arc<Mutex<Option<task::Sample>>>,
    observed: bool,
    accepted: bool,
    verified: bool,
    native_clean: bool,
    human_preserved: bool,
    cleaning: bool,
    result_class: &'static str,
    cleanup_failure: Option<&'static str>,
    trace: Trace,
    totals: Totals,
    finish: Option<Box<dyn FnOnce(ForegroundRenderingWitnessReport) + Send>>,
}

/// Terminal fallback carries callback/worker/resource obligations only. Engine,
/// Store, rendering and fixture holders stay out so normal engine teardown can
/// actually run. This never manufactures the missing qualification audit.
struct NativeShutdownOwner {
    owner: RetainedWorkProbeOwner,
    signal: Arc<Signal>,
    _rx: mpsc::Receiver<Reply>,
    snapshot_release: ReleaseSlot,
    credential_job: Option<JoinHandle<Result<AgentProviderCredential, &'static str>>>,
    join: Option<JoinHandle<AgentRuntimeScopedDrain>>,
    lifecycle: Option<AgentRuntimeScopedLifecycle>,
    control: Option<AgentRuntimeHandle>,
    worker_drained: bool,
    completion: Option<AgentRuntimeCompletion>,
    _result: Option<AgentWorkRetainedHandle>,
    _outcome: Option<AgentWorkRetainedOutcome>,
}
impl NativeShutdownOwner {
    fn drain_after_engine_shutdown(&mut self) -> bool {
        if self
            .credential_job
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
            let _ = self.credential_job.take().map(JoinHandle::join);
        }
        if self.join.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(Ok(AgentRuntimeScopedDrain::Drained(proof))) =
                self.join.take().map(JoinHandle::join)
            {
                self.worker_drained = self
                    .control
                    .as_ref()
                    .is_some_and(|control| proof.matches_runtime(control))
                    && Some(proof.lease().resource()) == self.owner.resource();
            }
        }
        let native_drained = now().is_ok_and(|now| self.owner.drain_after_engine_shutdown(now));
        let drained = native_drained
            && self
                .snapshot_release
                .lock()
                .is_ok_and(|slot| slot.is_none())
            && self.credential_job.is_none()
            && self.join.is_none()
            && self.lifecycle.is_none()
            && self.worker_drained
            && self
                .completion
                .as_ref()
                .is_none_or(AgentRuntimeCompletion::is_stopped);
        if drained {
            self.signal.alive.store(false, Ordering::Release);
        }
        drained
    }
}

/// Called only by the isolated actual application after exact main admission.
pub fn start_retained_controller_witness(
    engine: Arc<WebviewEngine>,
    store: Arc<SqliteStore>,
    admission: ForegroundRenderingAdmission,
    trace: Trace,
    finish: impl FnOnce(ForegroundRenderingWitnessReport) + Send + 'static,
) -> Result<(), &'static str> {
    if !admission.remains_current() {
        return Err("DeferredForeground");
    }
    if USED.with(|used| used.replace(true)) {
        return Err("already_used");
    }
    let started = Instant::now();
    let issued = now()?;
    let deadline = started.checked_add(TOTAL).ok_or("deadline")?;
    let expires =
        AgentPolicyInstant::from_millis(issued.millis().checked_add(150_000).ok_or("clock")?);
    let profile = AgentWorkProfileId::generate();
    let generation = ContentPolicyGeneration::new(1).ok_or("generation")?;
    let (tx, rx) = mpsc::sync_channel(4);
    let signal = Arc::new(Signal {
        profile,
        generation,
        tx,
        failed: AtomicBool::new(false),
        alive: AtomicBool::new(true),
        shutdown: AtomicBool::new(false),
    });
    let native = engine.clone();
    let wake = signal.clone();
    let owner = RetainedWorkProbeOwner::new(
        profile,
        // Diagnostic mailbox failure stops the driver; it is not loss of this
        // independently retained application resource-health receiver.
        Arc::new(move || wake.alive.load(Ordering::Acquire)),
        Box::new(move |sink| native.take_agent_browser_port(move |event| sink(event))),
    )
    .ok_or("native_owner")?;
    let fixture = FixtureServer::start().map_err(|_| "fixture")?;
    let target = ContextNavigationTarget::parse(&fixture.url(FixtureRoute::SemanticRendering))
        .map_err(|_| "fixture_target")?;
    let credential_job = std::thread::Builder::new()
        .name("work-probe-credential".into())
        .spawn(|| load_macos_probe_openai_credential().map_err(|_| "credential_unavailable"))
        .map_err(|_| "credential_worker")?;
    *POLICY.lock().map_err(|_| "policy_owner")? = Some(signal.clone());
    DRIVER.with(|slot| {
        *slot.borrow_mut() = Some(Driver {
            engine: engine.clone(),
            store,
            owner,
            admission,
            fixture: Some(fixture),
            target,
            signal,
            rx,
            timer: None,
            credential_job: Some(credential_job),
            credential: None,
            policy_ready: false,
            phase: Phase::Preparing,
            render: None,
            render_pending: None,
            render_next: None,
            snapshot_release: Arc::new(Mutex::new(None)),
            release_requested: Arc::new(AtomicBool::new(false)),
            started,
            deadline,
            issued,
            expires,
            run: ContextRunId::generate(),
            result: None,
            control: None,
            completion: None,
            lifecycle: None,
            join: None,
            scoped: None,
            outcome: None,
            expected: Arc::new(Mutex::new(None)),
            sample: Arc::new(Mutex::new(None)),
            observed: false,
            accepted: false,
            verified: false,
            native_clean: false,
            human_preserved: true,
            cleaning: false,
            result_class: "not_started",
            cleanup_failure: None,
            trace,
            totals: Totals::default(),
            finish: Some(Box::new(finish)),
        })
    });
    let rules = ContentRules::allow_all(ContentRuleDigest::from_bytes([0; 32]));
    if engine.install_content_rules(profile, generation, rules) != NativeDispatch::Scheduled {
        fail("content_policy_dispatch");
    }
    schedule();
    Ok(())
}
/// Exact diagnostic profile/generation only; unrelated shell events pass through.
pub fn retained_controller_policy_event(event: &EngineEvent) -> bool {
    let EngineEvent::ContentRulesSettled {
        profile,
        requested,
        settlement,
    } = event
    else {
        return false;
    };
    let Ok(slot) = POLICY.lock() else {
        return false;
    };
    let Some(signal) = slot
        .as_ref()
        .filter(|s| s.profile == *profile && s.generation == *requested)
    else {
        return false;
    };
    signal.send(Reply::Policy(matches!(
        settlement,
        ContentRuleSettlement::Applied { .. }
    )));
    true
}
pub fn cancel_retained_controller_witness() -> bool {
    DRIVER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(driver) = slot.as_mut() else {
            return false;
        };
        driver.fail("Cancelled");
        true
    })
}
pub fn retained_controller_native_drain() -> Option<bool> {
    EXPECTED_RESOURCE.with(|r| {
        r.borrow()
            .as_ref()
            .and_then(zephium_engine::retained_resource_rendering_drain)
    })
}
/// Called by the actual desktop's ordinary Exit event. A failed handoff remains
/// retained unless any dispatched original audit and all local obligations closed.
/// False is unresolved ownership, never a replacement clean-shutdown proof.
pub fn retained_controller_shutdown_complete() -> bool {
    SHUTDOWN_OWNER.with(|slot| {
        slot.borrow_mut()
            .release_if(NativeShutdownOwner::drain_after_engine_shutdown)
    })
}
pub fn retained_controller_native_failures() -> Option<zephium_engine::ForegroundNativeFailures> {
    EXPECTED_RESOURCE.with(|r| {
        r.borrow()
            .as_ref()
            .and_then(zephium_engine::retained_resource_rendering_failures)
    })
}
fn now() -> Result<AgentPolicyInstant, &'static str> {
    zephium_engine::work_browser_monotonic_now().ok_or("clock")
}
fn fail(reason: &'static str) {
    DRIVER.with(|slot| {
        if let Some(driver) = slot.borrow_mut().as_mut() {
            driver.fail(reason);
        }
    });
}
fn schedule() {
    let timer = zephium_engine::schedule_foreground_admission_wake(tick);
    DRIVER.with(|slot| {
        if let Some(driver) = slot.borrow_mut().as_mut() {
            driver.timer = timer;
            if driver.timer.is_none() {
                driver.cleanup_failure = Some("main_wake");
                driver.phase = Phase::Done;
            }
        }
    });
    finish_if_done();
}
fn tick() {
    DRIVER.with(|slot| {
        if let Some(driver) = slot.borrow_mut().as_mut() {
            driver.timer = None;
            if let Err(reason) = driver.advance() {
                driver.fail(reason);
            }
        }
    });
    if !finish_if_done() {
        schedule();
    }
}
fn finish_if_done() -> bool {
    if !DRIVER.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_none_or(|driver| driver.phase == Phase::Done)
    }) {
        return false;
    }
    if let Some(driver) = DRIVER.with(|slot| slot.borrow_mut().take()) {
        driver.finish();
    }
    true
}
impl Driver {
    fn render(&mut self, kind: Render) -> Result<(), &'static str> {
        if self.render_pending.is_some() {
            return Err("render_busy");
        }
        let renderer = self.render.as_ref().ok_or("render_owner")?;
        self.render_pending = Some(kind);
        let signal = self.signal.clone();
        let release = self.snapshot_release.clone();
        let callback = move |state| {
            if kind == Render::SnapshotRetire {
                deliver_snapshot_retirement(&release, state, &signal);
            }
            signal.send(Reply::Render(kind, state));
        };
        let scheduled = match kind {
            Render::Acquire => renderer.acquire(callback),
            Render::Poll => renderer.poll(callback),
            _ => renderer.retire(callback),
        };
        scheduled.then_some(()).ok_or("render_dispatch")
    }
    fn advance(&mut self) -> Result<(), &'static str> {
        if cleanup_window(Instant::now(), self.deadline, self.cleaning)? {
            self.cleanup_failure.get_or_insert("deadline");
        }
        if self.signal.failed.load(Ordering::Acquire) {
            if !self.cleaning {
                return Err("diagnostic_mailbox");
            }
            self.cleanup_failure.get_or_insert("diagnostic_mailbox");
        }
        if !self.admission.remains_current() {
            self.human_preserved = false;
            if !self.cleaning {
                return Err("DeferredForeground");
            }
        }
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Policy(true) if self.phase == Phase::Preparing && !self.policy_ready => {
                    self.policy_ready = true
                }
                Reply::SnapshotRetire if matches!(self.phase, Phase::Actor | Phase::Stopping) => {
                    self.render_next = Some(Render::SnapshotRetire)
                }
                Reply::Render(kind, state) if self.render_pending == Some(kind) => {
                    self.render_pending = None;
                    if self.phase == Phase::Retire && matches!(kind, Render::Acquire | Render::Poll)
                    {
                        self.render_next = Some(Render::CleanupRetire);
                        continue;
                    }
                    match (kind, state) {
                        (Render::Acquire | Render::Poll, ForegroundRenderingState::Ready)
                            if self.phase == Phase::Rendering =>
                        {
                            self.owner.acquire(self.run, now()?, self.expires)?;
                            self.phase = Phase::Acquire;
                        }
                        (Render::Acquire | Render::Poll, ForegroundRenderingState::Acquiring)
                            if self.phase == Phase::Rendering =>
                        {
                            self.render_next = Some(Render::Poll)
                        }
                        (Render::SnapshotRetire, ForegroundRenderingState::Retired) => {
                            if self.phase == Phase::Retire {
                                self.destroy()?;
                            }
                        }
                        (Render::SnapshotRetire, ForegroundRenderingState::Retiring) => {
                            self.render_next = Some(Render::SnapshotRetire)
                        }
                        (Render::CleanupRetire, ForegroundRenderingState::Retiring) => {
                            self.render_next = Some(Render::CleanupRetire)
                        }
                        (Render::CleanupRetire, ForegroundRenderingState::Retired) => {
                            self.destroy()?
                        }
                        _ => return Err("rendering_refused"),
                    }
                }
                _ => return Err("diagnostic_phase"),
            }
        }
        if matches!(
            self.phase,
            Phase::Destroy | Phase::NativeDrain | Phase::Seal
        ) {
            self.native_clean = progress_native_close(&mut self.owner, &mut self.phase, now()?)?;
        } else {
            if let Some(event) = self.owner.poll_lifecycle(now()?)? {
                match (self.phase, event) {
                    (Phase::Construct, WorkBrowserResourceEvent::Retained(resource)) => {
                        EXPECTED_RESOURCE.with(|r| *r.borrow_mut() = Some(resource.clone()));
                        self.render = Some(Arc::new(
                            WorkResourceRenderingProbe::new(
                                &self.engine,
                                &self.admission,
                                resource,
                            )
                            .ok_or("render_admission")?,
                        ));
                        self.phase = Phase::Rendering;
                        self.render_next = Some(Render::Acquire);
                    }
                    (Phase::Acquire, WorkBrowserResourceEvent::Acquired(lease)) => {
                        self.start_actor(lease)?
                    }
                    _ => return Err("lifecycle_phase"),
                }
            }
            if self.owner.poll_native_event()?.is_some() {
                return Err("native_event_phase");
            }
        }
        if let Some(kind) = self.render_next.take() {
            self.render(kind)?;
        }
        match self.phase {
            Phase::Preparing
                if self.policy_ready
                    && self
                        .credential_job
                        .as_ref()
                        .is_some_and(JoinHandle::is_finished) =>
            {
                self.credential = Some(
                    self.credential_job
                        .take()
                        .ok_or("credential_owner")?
                        .join()
                        .map_err(|_| "credential_worker_panic")??,
                );
                self.owner.construct(self.target.clone(), now()?)?;
                self.phase = Phase::Construct;
            }
            Phase::Actor | Phase::Stopping => self.poll_actor()?,
            Phase::Joining if self.join.as_ref().is_some_and(JoinHandle::is_finished) => {
                let drained = self
                    .join
                    .take()
                    .ok_or("worker_join")?
                    .join()
                    .map_err(|_| "worker_join_panic")?;
                if let AgentRuntimeScopedDrain::Drained(proof) = drained {
                    if !self
                        .control
                        .as_ref()
                        .is_some_and(|control| proof.matches_runtime(control))
                        || proof.lease().run() != self.run
                        || Some(proof.lease().resource()) != self.owner.resource()
                        || proof.policy().closure().model_calls() != self.totals.calls
                    {
                        self.result_class = "scoped_binding";
                    } else {
                        self.scoped = Some(proof);
                    }
                } else {
                    self.result_class = "scoped_unproven";
                }
                self.control.take();
                self.completion.take();
                self.result.take();
                self.begin_native_cleanup()?;
            }
            _ => {}
        }
        Ok(())
    }
    fn start_actor(&mut self, lease: WorkBrowserExecutionLease) -> Result<(), &'static str> {
        if !(self.trace)(RetainedProbeTrace::Configured) {
            return Err("trace_output");
        }
        let resource = lease.resource().identity();
        let identity = ContextIdentity::new(
            resource.context(),
            lease.run(),
            resource.profile(),
            ContextKind::Owned,
        );
        let input = task::input(
            identity,
            self.target.clone(),
            self.issued,
            self.expires,
            self.deadline,
        )?;
        let task = task::Task::new(identity, self.expected.clone(), self.sample.clone())?;
        let signal = self.signal.clone();
        let slot = self.snapshot_release.clone();
        let requested = self.release_requested.clone();
        let retire = Box::new(move |callback| {
            if requested.swap(true, Ordering::AcqRel) {
                return false;
            }
            match slot.lock() {
                Ok(mut slot) if slot.is_none() => *slot = Some(callback),
                _ => return false,
            }
            signal.send(Reply::SnapshotRetire)
        });
        let (result, control, completion, lifecycle) = self.owner.start(
            input,
            self.credential.take().ok_or("credential")?,
            self.store.clone(),
            Box::new(task),
            now()?,
            retire,
        )?;
        self.result = Some(result);
        self.control = Some(control);
        self.completion = Some(completion);
        self.lifecycle = Some(lifecycle);
        self.phase = Phase::Actor;
        Ok(())
    }
    fn drain_actor_metadata(&mut self) -> Result<(), &'static str> {
        let result = self.result.as_mut().ok_or("actor_result")?;
        while let Some(event) = result.take_event() {
            self.totals.observe(event.kind())?;
            if !(self.trace)(RetainedProbeTrace::Event(event)) {
                return Err("trace_output");
            }
        }
        if !self.observed {
            let sample = *self.sample.lock().map_err(|_| "task_sample")?;
            if let Some(sample) = sample {
                self.observed = true;
                if !(self.trace)(RetainedProbeTrace::Observation {
                    nodes: sample.nodes,
                    complete: sample.complete,
                    current_document: sample.current,
                    frame_boundaries: sample.boundaries,
                    markers: sample.markers,
                }) {
                    return Err("trace_output");
                }
            }
        }
        if self.outcome.is_none() {
            self.outcome = result.take_outcome();
        }
        Ok(())
    }
    fn poll_actor(&mut self) -> Result<(), &'static str> {
        self.drain_actor_metadata()?;
        if !self
            .completion
            .as_ref()
            .is_some_and(AgentRuntimeCompletion::is_stopped)
        {
            return Ok(());
        }
        // Completion can race the first empty pop. It is now stable: drain the
        // original final events/outcome again before judging or moving owners.
        self.drain_actor_metadata()?;
        let (state, failure) = match &self.outcome {
            Some(AgentWorkRetainedOutcome::Accepted { .. }) => ("accepted", None),
            Some(AgentWorkRetainedOutcome::ClosedUnsuccessfully(closed)) => {
                ("closed_unsuccessfully", Some(closed.failure()))
            }
            Some(AgentWorkRetainedOutcome::Recovery(recovery)) => {
                ("recovery", Some(recovery.failure()))
            }
            None => ("unavailable", None),
        };
        if !(self.trace)(RetainedProbeTrace::Outcome { state, failure }) {
            return Err("trace_output");
        }
        if let Some(AgentWorkRetainedOutcome::Accepted { extraction, .. }) = &self.outcome {
            self.accepted = true;
            self.verified = self
                .expected
                .lock()
                .map_err(|_| "task_evidence")?
                .as_ref()
                .is_some_and(|expected| task::verify_owned(extraction, expected));
            if !self.cleaning && self.verified && self.owner.presentation_returned() {
                self.result_class = "RetainedControllerAccepted";
            } else if !self.cleaning {
                self.result_class = "result_contract";
            }
        } else if !self.cleaning {
            self.result_class = "controller_unsuccessful";
        }
        begin_cleanup_deadline(&mut self.cleaning, &mut self.deadline, Instant::now())?;
        let lifecycle = self.lifecycle.take().ok_or("scoped_owner")?;
        let deadline = self.deadline;
        self.join = Some(
            std::thread::Builder::new()
                .name("work-probe-join".into())
                .spawn(move || lifecycle.drain_until(deadline))
                .map_err(|_| "join_spawn")?,
        );
        self.phase = Phase::Joining;
        Ok(())
    }
    fn begin_native_cleanup(&mut self) -> Result<(), &'static str> {
        self.owner.abandon_pending();
        if let Err(reason) = self.owner.drain_abandoned(now()?) {
            // Keep destruction available, but never erase an original slot's
            // accounting failure from the independent cleanup verdict.
            self.cleanup_failure.get_or_insert(reason);
        }
        if self.render.is_some() {
            self.phase = Phase::Retire;
            if self.render_pending.is_none() && self.render_next != Some(Render::SnapshotRetire) {
                self.render_next = Some(Render::CleanupRetire);
            }
            Ok(())
        } else {
            self.destroy()
        }
    }
    fn destroy(&mut self) -> Result<(), &'static str> {
        if self.owner.resource().is_some() {
            self.owner.destroy()?;
            self.phase = Phase::Destroy;
        } else {
            self.phase = Phase::NativeDrain;
        }
        Ok(())
    }
    fn fail(&mut self, reason: &'static str) {
        if self.cleaning {
            record_cleanup_failure(&mut self.phase, &mut self.cleanup_failure, reason);
            return;
        }
        self.result_class = reason;
        if let Err(reason) =
            begin_cleanup_deadline(&mut self.cleaning, &mut self.deadline, Instant::now())
        {
            self.cleanup_failure = Some(reason);
            self.phase = Phase::Done;
            return;
        }
        if let Some(control) = &self.control {
            control.stop_and_seal(AgentRuntimeStopReason::Cancelled);
            self.phase = Phase::Stopping;
        } else if let Err(cleanup) = self.begin_native_cleanup() {
            self.cleanup_failure = Some(cleanup);
            self.phase = Phase::Done;
        }
    }
    fn finish(mut self) {
        self.timer.take();
        POLICY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let callback_debt = self
            .snapshot_release
            .lock()
            .map_or(true, |slot| slot.is_some());
        if callback_debt
            || self.credential_job.is_some()
            || self.join.is_some()
            || self.lifecycle.is_some()
        {
            self.cleanup_failure = Some("application_callback_worker_debt");
        }
        let closed = self.closure_ready();
        if !closed {
            self.cleanup_failure
                .get_or_insert("original_shutdown_owner_debt");
            if let Some(control) = &self.control {
                control.stop_and_seal(AgentRuntimeStopReason::Cancelled);
            }
            // Stop consuming diagnostic replies before reporting failure; keep
            // all source/callback/worker owners and the original health sink.
            self.signal.shutdown.store(true, Ordering::Release);
        }
        let fixture_clean = self
            .fixture
            .take()
            .is_some_and(|fixture| fixture.shutdown().is_ok());
        let scoped = self.scoped.is_some();
        if !(self.trace)(RetainedProbeTrace::Totals {
            model_calls: self.totals.calls,
            input_tokens: self.totals.input,
            output_tokens: self.totals.output,
            cost_micro_usd: self.totals.cost,
        }) {
            self.cleanup_failure = Some("trace_output");
        }
        if !(self.trace)(RetainedProbeTrace::Closure {
            accepted: self.accepted,
            fixture_mapping_verified: self.verified,
            presentation_retired: self.owner.presentation_returned(),
            scoped_worker_drained: scoped,
            original_resource_retired: self.owner.locally_retired(),
        }) {
            self.cleanup_failure = Some("trace_output");
        }
        if self.result_class == "RetainedControllerAccepted" && !scoped {
            self.result_class = "scoped_unproven";
        }
        let report = ForegroundRenderingWitnessReport {
            outcome: self.result_class,
            cleanup_failure: self.cleanup_failure,
            samples: Vec::new(),
            native_cohort_clean: self.native_clean,
            human_ownership_preserved: self.human_preserved && self.admission.remains_current(),
            fixture_clean,
            elapsed_ms: self
                .started
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        };
        let finish = self.finish.take();
        if closed {
            self.signal.alive.store(false, Ordering::Release);
            drop(self);
        } else {
            // A scoped lifecycle has its own original reaper on unsuccessful
            // drain; drive it off the UI thread under the same fixed hard bound.
            if let Some(lifecycle) = self.lifecycle.take() {
                let deadline = self
                    .deadline
                    .checked_add(HARD_CLEANUP_GRACE)
                    .unwrap_or(self.deadline);
                self.join = std::thread::Builder::new()
                    .name("work-probe-final-join".into())
                    .spawn(move || lifecycle.drain_until(deadline))
                    .ok();
            }
            let retained = NativeShutdownOwner {
                worker_drained: self.control.is_none() || self.scoped.is_some(),
                owner: self.owner,
                signal: self.signal,
                _rx: self.rx,
                snapshot_release: self.snapshot_release,
                credential_job: self.credential_job,
                join: self.join,
                lifecycle: self.lifecycle,
                control: self.control,
                completion: self.completion,
                _result: self.result,
                _outcome: self.outcome,
            };
            drop((self.engine, self.store, self.render));
            SHUTDOWN_OWNER.with(|slot| slot.borrow_mut().retain(retained));
        }
        if let Some(finish) = finish {
            finish(report);
        }
    }
    fn closure_ready(&self) -> bool {
        self.native_clean
            && self.owner.locally_retired()
            && self.render_pending.is_none()
            && self
                .snapshot_release
                .lock()
                .is_ok_and(|slot| slot.is_none())
            && self.credential_job.is_none()
            && self.join.is_none()
            && self.lifecycle.is_none()
            && self
                .completion
                .as_ref()
                .is_none_or(AgentRuntimeCompletion::is_stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type TestNativeSink = Arc<dyn Fn(ContextNativeEvent) + Send + Sync>;
    #[derive(Default)]
    struct NativeCloseFixture {
        sink: Mutex<Option<TestNativeSink>>,
        reporter: Mutex<Option<WorkBrowserResourceHealthReporter>>,
        audit: Mutex<Option<ContextResourceAuditId>>,
        refuse_audit: AtomicBool,
        hold_destroy: AtomicBool,
        destruction: Mutex<
            Option<(
                WorkBrowserResourceRequest,
                WorkBrowserResourceCompletionCallback,
            )>,
        >,
    }
    impl AgentBrowserPort for NativeCloseFixture {
        fn work_resource_lifecycle(
            &self,
            mut request: WorkBrowserResourceRequest,
            callback: WorkBrowserResourceCompletionCallback,
        ) -> WorkBrowserResourceDispatch {
            let outcome = match request.operation() {
                WorkBrowserResourceOperation::Construct => {
                    let reporter = request.take_resource_health_reporter().unwrap();
                    assert!(reporter.install(request.resource()));
                    *self.reporter.lock().unwrap() = Some(reporter);
                    WorkBrowserResourceNativeOutcome::Constructed
                }
                WorkBrowserResourceOperation::Destroy => {
                    if self.hold_destroy.load(Ordering::Acquire) {
                        *self.destruction.lock().unwrap() = Some((request, callback));
                        return WorkBrowserResourceDispatch::Scheduled;
                    }
                    WorkBrowserResourceNativeOutcome::Destroyed
                }
                _ => panic!("cleanup fixture has no actor"),
            };
            callback(request.complete(outcome));
            WorkBrowserResourceDispatch::Scheduled
        }
        fn seal_for_shutdown(&self, audit: ContextResourceAuditId) -> ContextShutdownDispatch {
            assert!(self.reporter.lock().unwrap().is_none());
            if self.refuse_audit.load(Ordering::Acquire) {
                return ContextShutdownDispatch::SealedWithoutAudit(ContextPortFailure::Shutdown);
            }
            assert!(self.audit.lock().unwrap().replace(audit).is_none());
            ContextShutdownDispatch::AuditScheduled
        }
        fn dispatch(&self, _: ContextNativeRequest) -> ContextDispatch {
            ContextDispatch::Unsupported
        }
        fn transfer_cookies(&self, _: ContextCookieTransferRequest) -> ContextDispatch {
            ContextDispatch::Unsupported
        }
        fn audit_resources(&self, _: ContextResourceAuditId) -> ContextDispatch {
            ContextDispatch::Unsupported
        }
        fn invoke_semantic(&self, _: SemanticRuntimeInvocation) -> ContextDispatch {
            ContextDispatch::Unsupported
        }
        fn execute_semantic_action(
            &self,
            _: SemanticActionNativeRequest,
            _: SemanticActionNativeCompletion,
        ) -> ContextDispatch {
            ContextDispatch::Unsupported
        }
        fn capture_semantic_screenshot(
            &self,
            _: SemanticScreenshotNativeRequest,
            _: SemanticScreenshotNativeCompletion,
        ) -> ContextDispatch {
            ContextDispatch::Unsupported
        }
    }
    fn closing_owner(signal: &Arc<Signal>) -> (RetainedWorkProbeOwner, Arc<NativeCloseFixture>) {
        closing_owner_with_pending_destroy(signal, false)
    }
    fn closing_owner_with_pending_destroy(
        signal: &Arc<Signal>,
        hold_destroy: bool,
    ) -> (RetainedWorkProbeOwner, Arc<NativeCloseFixture>) {
        let native = Arc::new(NativeCloseFixture::default());
        native.hold_destroy.store(hold_destroy, Ordering::Release);
        let port = native.clone();
        let wake = signal.clone();
        let mut owner = RetainedWorkProbeOwner::new(
            AgentWorkProfileId::generate(),
            Arc::new(move || wake.alive.load(Ordering::Acquire)),
            Box::new(move |sink| {
                *port.sink.lock().unwrap() = Some(sink);
                Some(port)
            }),
        )
        .unwrap();
        owner
            .construct(
                ContextNavigationTarget::parse("http://127.0.0.1:12345/semantic-rendering-v1.html")
                    .unwrap(),
                AgentPolicyInstant::from_millis(1),
            )
            .unwrap();
        assert!(matches!(
            owner
                .poll_lifecycle(AgentPolicyInstant::from_millis(2))
                .unwrap(),
            Some(WorkBrowserResourceEvent::Retained(_))
        ));
        owner.destroy().unwrap();
        (owner, native)
    }
    impl NativeCloseFixture {
        fn finish_destruction(&self) {
            let (request, callback) = self.destruction.lock().unwrap().take().unwrap();
            callback(request.complete(WorkBrowserResourceNativeOutcome::Destroyed));
        }
        fn finish_audit(&self, queued_tasks: u8) {
            // Mock adapter evidence exercises controller ordering only. Actual
            // native zero/accounting is separately tested by engine fixtures.
            let audit = self.audit.lock().unwrap().take().unwrap();
            let snapshot = ContextNativeResourceSnapshot::try_new(ContextNativeResourceCounts {
                known_bindings: 0,
                resident_views: 0,
                owned_reservations: 0,
                borrowed_leases: 0,
                visible_surfaces: 0,
                suspended_views: 0,
                pending_operations: 0,
                pending_captures: 0,
                queued_tasks,
            })
            .unwrap();
            self.sink.lock().unwrap().as_ref().unwrap()(ContextNativeEvent::ShutdownAuditSettled(
                ContextShutdownAuditSettlement::new(audit, Ok(snapshot)),
            ));
        }
    }

    #[test]
    fn timeout_and_notifier_failure_keep_native_drain_running_until_original_audit() {
        for queued_debt in [0, 1] {
            let signal = Arc::new(signal());
            let (mut owner, native) = closing_owner(&signal);
            let mut phase = Phase::Destroy;
            let mut failure = None;
            assert!(!progress_native_close(
                &mut owner,
                &mut phase,
                AgentPolicyInstant::from_millis(3)
            )
            .unwrap());
            assert!(phase == Phase::NativeDrain);
            let deadline = Instant::now();
            assert_eq!(cleanup_window(deadline, deadline, true), Ok(true));
            record_cleanup_failure(&mut phase, &mut failure, "deadline");
            assert!(!signal.send(Reply::Policy(false)));
            record_cleanup_failure(&mut phase, &mut failure, "diagnostic_mailbox");
            assert!(phase == Phase::NativeDrain);
            assert!(!progress_native_close(
                &mut owner,
                &mut phase,
                AgentPolicyInstant::from_millis(4)
            )
            .unwrap());
            assert!(native.audit.lock().unwrap().is_none());
            // The delayed final native owner wakes the still-live application.
            drop(native.reporter.lock().unwrap().take());
            assert!(!progress_native_close(
                &mut owner,
                &mut phase,
                AgentPolicyInstant::from_millis(5)
            )
            .unwrap());
            assert!(phase == Phase::Seal);
            assert!(signal.alive.load(Ordering::Acquire));
            native.finish_audit(queued_debt);
            let outcome =
                progress_native_close(&mut owner, &mut phase, AgentPolicyInstant::from_millis(6));
            if queued_debt == 0 {
                assert_eq!(outcome, Ok(true));
                assert!(phase == Phase::Done);
            } else {
                assert_eq!(outcome, Err("native_resource_debt"));
                assert!(phase == Phase::Seal);
            }
            assert_eq!(failure, Some("deadline"));
            assert_eq!(
                cleanup_window(deadline + HARD_CLEANUP_GRACE, deadline, true),
                Err("hard_cleanup_deadline")
            );
        }
    }

    #[test]
    fn hard_deadline_retains_narrow_owner_until_engine_releases_late_reporter() {
        let signal = Arc::new(signal());
        let (mut owner, native) = closing_owner(&signal);
        let mut phase = Phase::Destroy;
        assert!(
            !progress_native_close(&mut owner, &mut phase, AgentPolicyInstant::from_millis(3))
                .unwrap()
        );
        let mut failure = None;
        record_cleanup_failure(&mut phase, &mut failure, "hard_cleanup_deadline");
        assert!(phase == Phase::Done);
        signal.shutdown.store(true, Ordering::Release);
        let (_, rx) = mpsc::channel();
        let retained = NativeShutdownOwner {
            owner,
            signal: signal.clone(),
            _rx: rx,
            snapshot_release: Arc::new(Mutex::new(None)),
            credential_job: None,
            join: None,
            lifecycle: None,
            control: None,
            worker_drained: true,
            completion: None,
            _result: None,
            _outcome: None,
        };
        let mut retention = ShutdownRetention::new();
        retention.retain(retained);
        assert!(!retention.release_if(NativeShutdownOwner::drain_after_engine_shutdown));
        for _ in 0..8 {
            assert!(signal.send(Reply::Policy(false)));
        }
        assert!(signal.alive.load(Ordering::Acquire));
        drop(native.reporter.lock().unwrap().take());
        assert!(retention.release_if(NativeShutdownOwner::drain_after_engine_shutdown));
        assert!(!signal.alive.load(Ordering::Acquire));
        assert!(native.audit.lock().unwrap().is_none());
        assert_eq!(failure, Some("hard_cleanup_deadline"));
    }

    #[test]
    fn failed_audit_dispatch_keeps_original_row_and_receiver_for_terminal_drain() {
        let signal = Arc::new(signal());
        let (mut owner, native) = closing_owner(&signal);
        let mut phase = Phase::Destroy;
        assert!(
            !progress_native_close(&mut owner, &mut phase, AgentPolicyInstant::from_millis(3))
                .unwrap()
        );
        drop(native.reporter.lock().unwrap().take());
        native.refuse_audit.store(true, Ordering::Release);
        assert_eq!(
            progress_native_close(&mut owner, &mut phase, AgentPolicyInstant::from_millis(4)),
            Err("native_audit_dispatch")
        );
        assert!(owner.resource().is_some());
        assert!(owner.drain_after_engine_shutdown(AgentPolicyInstant::from_millis(5)));
        assert!(signal.alive.load(Ordering::Acquire));
    }

    #[test]
    fn engine_teardown_cannot_release_fallback_with_original_flight_or_audit_terminal_pending() {
        for hold_destroy in [false, true] {
            let signal = Arc::new(signal());
            let (mut owner, native) = closing_owner_with_pending_destroy(&signal, hold_destroy);
            let mut phase = Phase::Destroy;
            drop(native.reporter.lock().unwrap().take());
            assert!(!progress_native_close(
                &mut owner,
                &mut phase,
                AgentPolicyInstant::from_millis(3)
            )
            .unwrap());
            assert!(
                phase
                    == if hold_destroy {
                        Phase::Destroy
                    } else {
                        Phase::Seal
                    }
            );
            signal.shutdown.store(true, Ordering::Release);
            let (_, rx) = mpsc::channel();
            let mut retention = ShutdownRetention::new();
            retention.retain(NativeShutdownOwner {
                owner,
                signal: signal.clone(),
                _rx: rx,
                snapshot_release: Arc::new(Mutex::new(None)),
                credential_job: None,
                join: None,
                lifecycle: None,
                control: None,
                worker_drained: true,
                completion: None,
                _result: None,
                _outcome: None,
            });
            assert!(!retention.release_if(NativeShutdownOwner::drain_after_engine_shutdown));
            assert!(signal.alive.load(Ordering::Acquire));
            if hold_destroy {
                native.finish_destruction();
            } else {
                native.finish_audit(0);
            }
            assert!(retention.release_if(NativeShutdownOwner::drain_after_engine_shutdown));
            assert!(!signal.alive.load(Ordering::Acquire));
        }
    }
    fn signal() -> Signal {
        let (tx, _) = mpsc::sync_channel(4);
        Signal {
            profile: AgentWorkProfileId::generate(),
            generation: ContentPolicyGeneration::new(1).unwrap(),
            tx,
            failed: AtomicBool::new(false),
            alive: AtomicBool::new(true),
            shutdown: AtomicBool::new(false),
        }
    }
    #[test]
    fn retirement_continuation_retains_exact_callback_until_native_terminal() {
        let signal = signal();
        let (tx, rx) = mpsc::sync_channel(1);
        let slot: ReleaseSlot = Arc::new(Mutex::new(Some(Box::new(move |retired| {
            tx.send(retired).unwrap();
        }))));
        for _ in 0..3 {
            deliver_snapshot_retirement(&slot, ForegroundRenderingState::Retiring, &signal);
            assert!(rx.try_recv().is_err());
            assert!(slot.lock().unwrap().is_some());
        }
        deliver_snapshot_retirement(&slot, ForegroundRenderingState::Retired, &signal);
        assert_eq!(rx.try_recv(), Ok(true));
        assert!(slot.lock().unwrap().is_none());
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            deliver_snapshot_retirement(&slot, ForegroundRenderingState::Retired, &signal)
        }))
        .is_err());
    }
    #[test]
    fn retirement_refusal_poison_and_reentrant_callback_never_fake_success() {
        for poison in [false, true] {
            let signal = signal();
            let (tx, rx) = mpsc::sync_channel(1);
            let slot: ReleaseSlot = Arc::new(Mutex::new(None));
            let reentrant = Arc::downgrade(&slot);
            *slot.lock().unwrap() = Some(Box::new(move |retired| {
                assert!(reentrant
                    .upgrade()
                    .unwrap()
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_none());
                tx.send(retired).unwrap();
            }));
            if poison {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _guard = slot.lock().unwrap();
                    panic!("poison snapshot owner");
                }));
            }
            deliver_snapshot_retirement(
                &slot,
                if poison {
                    ForegroundRenderingState::Retired
                } else {
                    ForegroundRenderingState::Failed
                },
                &signal,
            );
            assert_eq!(rx.try_recv(), Ok(false));
            assert_eq!(signal.failed.load(Ordering::Acquire), poison);
        }
    }
    #[test]
    fn native_callback_panic_is_not_contained_as_healthy_delivery() {
        let slot: ReleaseSlot =
            Arc::new(Mutex::new(Some(Box::new(|_| panic!("hostile final wake")))));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            deliver_snapshot_retirement(&slot, ForegroundRenderingState::Retired, &signal())
        }))
        .is_err());
        assert!(slot.lock().unwrap().is_none());
    }
    #[test]
    fn cleanup_deadline_cannot_be_renewed_by_late_actor_terminal() {
        let now = Instant::now();
        let mut deadline = now + TOTAL;
        let mut cleaning = false;
        begin_cleanup_deadline(&mut cleaning, &mut deadline, now).unwrap();
        assert_eq!(deadline, now + CLEANUP);
        let original = deadline;
        begin_cleanup_deadline(&mut cleaning, &mut deadline, now + Duration::from_secs(4)).unwrap();
        assert_eq!(deadline, original);
    }
}
