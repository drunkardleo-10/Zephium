//! Fixed provider-free two-lease witness on the actual application loop.
//! No model, arbitrary script, credential, public site or event-loop replacement.
use super::agentic_foreground_driver::ForegroundRenderingWitnessSample;
use super::{
    agentic_foreground_probe::HumanForegroundGuard,
    agentic_semantic_probe::{sample_foreground_snapshot, RenderingDocumentState},
    ContentPolicyTimeout,
};
use crate::agent_context_port::resource_witness::{
    Evidence, Operation as RenderOp, Request as RenderRequest, ResourceWitnessPort, RetentionStamp,
};
use crate::{ForegroundRenderingAdmission, ForegroundRenderingWitnessReport};
use objc2_foundation::MainThreadMarker;
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use zephium_agentic::*;
use zephium_core::{
    blocker::{ContentPolicyGeneration, ContentRuleDigest, ContentRules},
    ports::engine::{ContentRuleSettlement, Engine, EngineEvent, NativeDispatch},
};

const SAMPLE_OFFSETS_MS: [u64; 7] = [0, 50, 100, 200, 400, 800, 1600];
const TOTAL_BUDGET: Duration = Duration::from_secs(15);
const CLEANUP_BUDGET: Duration = Duration::from_secs(5);

enum Reply {
    Policy(bool),
    Lifecycle(WorkBrowserResourceCompletion),
    Read(Box<WorkBrowserObservationCompletion>),
    Rendering(RenderRequest, Evidence),
    Native(ContextNativeEvent),
}
struct Mailbox {
    profile: AgentWorkProfileId,
    generation: ContentPolicyGeneration,
    tx: SyncSender<Reply>,
    failed: AtomicBool,
}
impl Mailbox {
    fn deliver(&self, reply: Reply) {
        if self.tx.try_send(reply).is_err() {
            self.failed.store(true, Ordering::Release);
        }
    }
}
static POLICY_MAILBOX: Mutex<Option<Arc<Mailbox>>> = Mutex::new(None);
thread_local! {
    static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) };
    static USED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static EXPECTED: RefCell<Option<WorkBrowserResourceJoin>> = const { RefCell::new(None) };
}
pub fn work_resource_native_drain() -> Option<bool> {
    EXPECTED.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(super::agentic_foreground_probe::resource_native_drain)
    })
}
pub fn work_resource_native_failures() -> Option<crate::ForegroundNativeFailures> {
    EXPECTED.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(super::agentic_foreground_probe::resource_native_failures)
    })
}
pub fn work_resource_policy_event(event: &EngineEvent) -> bool {
    let EngineEvent::ContentRulesSettled {
        profile,
        requested,
        settlement,
    } = event
    else {
        return false;
    };
    let Ok(mailbox) = POLICY_MAILBOX.lock() else {
        return false;
    };
    let Some(mailbox) = mailbox
        .as_ref()
        .filter(|m| m.profile == *profile && m.generation == *requested)
    else {
        return false;
    };
    mailbox.deliver(Reply::Policy(matches!(
        settlement,
        ContentRuleSettlement::Applied { .. }
    )));
    true
}
#[derive(Clone)]
enum Phase {
    Policy,
    Construct,
    Render(RenderRequest),
    Acquire(u8),
    WaitingSample,
    Read(u8, SemanticRuntimeCorrelation),
    Stamp(u8, RenderRequest),
    Revoke(u8),
    Retire(RenderRequest),
    Destroy,
    Seal(ContextResourceAuditId),
    Done,
}
struct Driver {
    engine: Arc<crate::WebviewEngine>,
    port: Arc<dyn AgentBrowserPort>,
    rendering: ResourceWitnessPort,
    mailbox: Arc<Mailbox>,
    rx: Receiver<Reply>,
    human: HumanForegroundGuard,
    rows: WorkBrowserResources,
    resource: Option<WorkBrowserResourceJoin>,
    lease: Option<WorkBrowserExecutionLease>,
    old_lease: Option<WorkBrowserExecutionLease>,
    target: ContextNavigationTarget,
    fixture: Option<FixtureServer>,
    phase: Phase,
    started: Instant,
    deadline: Instant,
    lease_deadline: AgentPolicyInstant,
    timer: Option<ContentPolicyTimeout>,
    rendering_started: Option<Instant>,
    render_attempted: bool,
    samples: Vec<ForegroundRenderingWitnessSample>,
    stamp: Option<RetentionStamp>,
    distinct_leases: bool,
    stale_core: bool,
    stale_native: bool,
    retained: bool,
    ended: u8,
    native_counts: [u16; 2],
    cleanup_started: bool,
    outcome: &'static str,
    cleanup_failure: Option<&'static str>,
    native_clean: bool,
    human_preserved: bool,
    completion: Option<Box<dyn FnOnce(ForegroundRenderingWitnessReport) + Send>>,
}
pub fn start_work_resource_witness(
    engine: Arc<crate::WebviewEngine>,
    admission: ForegroundRenderingAdmission,
    completion: impl FnOnce(ForegroundRenderingWitnessReport) + Send + 'static,
) -> Result<(), &'static str> {
    MainThreadMarker::new().ok_or("main_thread")?;
    if USED.with(|used| used.replace(true)) {
        return Err("already_used");
    }
    let ForegroundRenderingAdmission(human) = admission;
    if !human.is_current() {
        completion(ForegroundRenderingWitnessReport {
            outcome: "DeferredForeground",
            cleanup_failure: None,
            samples: Vec::new(),
            native_cohort_clean: true,
            human_ownership_preserved: false,
            fixture_clean: true,
            elapsed_ms: 0,
        });
        return Ok(());
    }
    let started = Instant::now();
    let deadline = started.checked_add(TOTAL_BUDGET).ok_or("deadline")?;
    let now = clock()?;
    let lease_deadline =
        AgentPolicyInstant::from_millis(now.millis().checked_add(15_000).ok_or("clock")?);
    let profile = AgentWorkProfileId::generate();
    let generation = ContentPolicyGeneration::new(1).ok_or("generation")?;
    let (tx, rx) = mpsc::sync_channel(4);
    let mailbox = Arc::new(Mailbox {
        profile,
        generation,
        tx,
        failed: AtomicBool::new(false),
    });
    let native = mailbox.clone();
    let port = engine
        .take_agent_browser_port(move |event| native.deliver(Reply::Native(event)))
        .ok_or("native_port")?;
    let rendering = engine
        .agent_context_port
        .resource_witness_port()
        .ok_or("render_port")?;
    let fixture = FixtureServer::start().map_err(|_| "fixture")?;
    let target = ContextNavigationTarget::parse(&fixture.url(FixtureRoute::SemanticRendering))
        .map_err(|_| "fixture_target")?;
    let rows = WorkBrowserResources::new(WorkId::generate(), profile);
    *POLICY_MAILBOX.lock().map_err(|_| "policy_mailbox")? = Some(mailbox.clone());
    DRIVER.with(|slot| {
        *slot.borrow_mut() = Some(Driver {
            engine,
            port,
            rendering,
            mailbox,
            rx,
            human,
            rows,
            resource: None,
            lease: None,
            old_lease: None,
            target,
            fixture: Some(fixture),
            phase: Phase::Policy,
            started,
            deadline,
            lease_deadline,
            timer: None,
            rendering_started: None,
            render_attempted: false,
            samples: Vec::with_capacity(8),
            stamp: None,
            distinct_leases: false,
            stale_core: false,
            stale_native: false,
            retained: false,
            ended: 0,
            native_counts: [0; 2],
            cleanup_started: false,
            outcome: "ControlsIncomplete",
            cleanup_failure: None,
            native_clean: false,
            human_preserved: true,
            completion: Some(Box::new(completion)),
        })
    });
    let admitted = DRIVER.with(|slot| {
        slot.borrow().as_ref().is_some_and(|d| {
            d.engine.install_content_rules(
                profile,
                generation,
                ContentRules::allow_all(ContentRuleDigest::from_bytes([0; 32])),
            ) == NativeDispatch::Scheduled
        })
    });
    if !admitted {
        fail_driver("policy_dispatch");
    }
    schedule_tick();
    Ok(())
}
fn clock() -> Result<AgentPolicyInstant, &'static str> {
    crate::work_browser_monotonic_now().ok_or("clock")
}
pub fn cancel_work_resource_witness() -> bool {
    if MainThreadMarker::new().is_none() {
        return false;
    }
    DRIVER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(d) = slot.as_mut() else {
            return false;
        };
        d.fail("Cancelled");
        true
    })
}
fn fail_driver(reason: &'static str) {
    DRIVER.with(|slot| {
        if let Some(d) = slot.borrow_mut().as_mut() {
            d.fail(reason);
        }
    });
}
fn schedule_tick() {
    let timer = super::schedule_content_policy_timeout(Duration::from_millis(25), tick);
    DRIVER.with(|slot| {
        if let Some(d) = slot.borrow_mut().as_mut() {
            d.timer = timer;
            if d.timer.is_none() {
                d.cleanup_failure = Some("timer_unavailable");
                d.phase = Phase::Done;
            }
        }
    });
    finish_if_done();
}
fn tick() {
    DRIVER.with(|slot| {
        if let Some(d) = slot.borrow_mut().as_mut() {
            d.timer = None;
            if let Err(reason) = d.advance() {
                d.fail(reason);
            }
        }
    });
    if !finish_if_done() {
        schedule_tick();
    }
}
fn finish_if_done() -> bool {
    if !DRIVER.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_none_or(|d| matches!(d.phase, Phase::Done))
    }) {
        return false;
    }
    if let Some(d) = DRIVER.with(|slot| slot.borrow_mut().take()) {
        d.finish();
    }
    true
}
impl Driver {
    fn join(&self) -> Result<&WorkBrowserResourceJoin, &'static str> {
        self.resource.as_ref().ok_or("resource")
    }
    fn lifecycle(&mut self, request: WorkBrowserResourceRequest) -> Result<(), &'static str> {
        let mailbox = self.mailbox.clone();
        match self.port.work_resource_lifecycle(
            request,
            Box::new(move |completion| mailbox.deliver(Reply::Lifecycle(completion))),
        ) {
            WorkBrowserResourceDispatch::Scheduled => Ok(()),
            WorkBrowserResourceDispatch::Rejected { request, failure } => {
                let _ = self
                    .rows
                    .dispatch_refused(*request, failure)
                    .map_err(|_| "lifecycle_refusal_owner")?;
                Err("lifecycle_dispatch")
            }
        }
    }
    fn render(&mut self, operation: RenderOp, stamp: Option<u8>) -> Result<(), &'static str> {
        let request = RenderRequest {
            resource: self.join()?.clone(),
            operation,
            document: crate::agent_context_port::resource_witness::Document::RenderingFixture,
        };
        self.phase = if operation == RenderOp::Retire {
            Phase::Retire(request.clone())
        } else if let Some(index) = stamp {
            Phase::Stamp(index, request.clone())
        } else {
            Phase::Render(request.clone())
        };
        if operation == RenderOp::Acquire {
            self.render_attempted = true;
            EXPECTED.with(|s| *s.borrow_mut() = Some(request.resource.clone()));
        }
        let mailbox = self.mailbox.clone();
        self.rendering
            .schedule(
                request,
                Box::new(move |request, evidence| {
                    mailbox.deliver(Reply::Rendering(request, evidence))
                }),
            )
            .then_some(())
            .ok_or("rendering_dispatch")
    }
    fn acquire(&mut self, index: u8) -> Result<(), &'static str> {
        let resource = self.join()?.clone();
        let request = self
            .rows
            .acquire(
                &resource,
                ContextRunId::generate(),
                clock()?,
                self.lease_deadline,
            )
            .map_err(|_| "acquire")?;
        let lease = request.lease().ok_or("lease")?.clone();
        if index == 1 {
            let old = self.old_lease.as_ref().ok_or("old_lease")?;
            self.distinct_leases =
                old != &lease && old.run() != lease.run() && old.resource() == lease.resource();
            if !self.distinct_leases {
                return Err("distinct_leases");
            }
        }
        self.lease = Some(lease);
        self.phase = Phase::Acquire(index);
        self.lifecycle(request)
    }
    fn observe(&mut self, index: u8) -> Result<(), &'static str> {
        let lease = self.lease.as_ref().ok_or("lease")?;
        let request = self
            .rows
            .observe_initial(lease, clock()?)
            .map_err(|_| "read_admission")?;
        self.phase = Phase::Read(index, request.invocation().correlation());
        let mailbox = self.mailbox.clone();
        match self.port.work_resource_observe(
            request,
            Box::new(move |completion| mailbox.deliver(Reply::Read(Box::new(completion)))),
        ) {
            WorkBrowserObservationDispatch::Scheduled => Ok(()),
            WorkBrowserObservationDispatch::Rejected { request, .. } => {
                self.rows
                    .observation_dispatch_refused(*request)
                    .map_err(|_| "read_refusal_owner")?;
                Err("read_dispatch")
            }
        }
    }
    fn revoke(&mut self, index: u8) -> Result<(), &'static str> {
        let lease = self.lease.as_ref().ok_or("lease")?.clone();
        let stale = if index == 0 {
            Some(
                self.rows
                    .observe_initial(&lease, clock()?)
                    .map_err(|_| "stale_read_owner")?,
            )
        } else {
            None
        };
        let request = self.rows.revoke(&lease).map_err(|_| "revoke")?;
        self.phase = Phase::Revoke(index);
        self.lifecycle(request)?;
        if let Some(stale) = stale {
            self.stale_core = self.rows.admits_lease(&lease, clock()?).is_err();
            let mailbox = self.mailbox.clone();
            match self.port.work_resource_observe(
                stale,
                Box::new(move |completion| mailbox.deliver(Reply::Read(Box::new(completion)))),
            ) {
                WorkBrowserObservationDispatch::Rejected {
                    request,
                    failure: ContextPortFailure::Stale,
                } => {
                    self.rows
                        .observation_dispatch_refused(*request)
                        .map_err(|_| "stale_refusal_owner")?;
                    self.stale_native = true;
                }
                WorkBrowserObservationDispatch::Rejected { request, .. } => {
                    self.rows
                        .observation_dispatch_refused(*request)
                        .map_err(|_| "stale_refusal_owner")?;
                    return Err("stale_refusal_class");
                }
                WorkBrowserObservationDispatch::Scheduled => return Err("stale_read_admitted"),
            }
        }
        Ok(())
    }
    fn advance(&mut self) -> Result<(), &'static str> {
        if Instant::now() >= self.deadline {
            if self.cleanup_started {
                self.cleanup_failure = Some("deadline");
                self.phase = Phase::Done;
            } else {
                self.fail("deadline");
            }
            return Ok(());
        }
        if self.mailbox.failed.load(Ordering::Acquire) {
            return Err("mailbox");
        }
        if !self.human.is_current() {
            self.human_preserved = false;
            if !self.cleanup_started {
                return Err("DeferredForeground");
            }
        }
        if let Ok(reply) = self.rx.try_recv() {
            self.reply(reply)?;
        }
        if matches!(self.phase, Phase::WaitingSample) {
            let index = self.samples.len();
            if index >= SAMPLE_OFFSETS_MS.len() {
                return Err("ControlsIncomplete");
            }
            if self.rendering_started.ok_or("rendering_clock")?.elapsed()
                >= Duration::from_millis(SAMPLE_OFFSETS_MS[index])
            {
                self.observe(0)?;
            }
        }
        Ok(())
    }
    fn reply(&mut self, reply: Reply) -> Result<(), &'static str> {
        if let Reply::Rendering(request, _) = &reply {
            if !rendering_reply_matches(&self.phase, request) {
                return Err("rendering_phase");
            }
        }
        match reply {
            Reply::Policy(true) if matches!(self.phase, Phase::Policy) => {
                let request = self
                    .rows
                    .construct_document(
                        WorkBrowserResourceId::generate(),
                        ContextId::generate(),
                        ContextProfileStorageClass::Ephemeral,
                        self.target.clone(),
                        clock()?,
                    )
                    .map_err(|_| "construct")?;
                self.resource = Some(request.resource().clone());
                self.phase = Phase::Construct;
                self.lifecycle(request)
            }
            Reply::Lifecycle(completion) => {
                let event = self
                    .rows
                    .settle_at(completion, clock()?)
                    .map_err(|_| "lifecycle_owner")?;
                if let WorkBrowserResourceEvent::Quarantined(failure) = &event {
                    let stage = lifecycle_stage(&self.phase);
                    crate::diagnostic!(
                        "work-resource-lifecycle-refused: stage={stage} failure={failure:?}"
                    );
                    return Err(stage);
                }
                match (self.phase.clone(), event) {
                    (Phase::Construct, WorkBrowserResourceEvent::Retained(resource))
                        if Some(&resource) == self.resource.as_ref() =>
                    {
                        self.rendering_started = Some(Instant::now());
                        self.render(RenderOp::Acquire, None)
                    }
                    (Phase::Acquire(index), WorkBrowserResourceEvent::Acquired(lease))
                        if Some(&lease) == self.lease.as_ref() =>
                    {
                        if index == 0 {
                            self.phase = Phase::WaitingSample;
                            Ok(())
                        } else {
                            self.observe(index)
                        }
                    }
                    (Phase::Revoke(index), WorkBrowserResourceEvent::LeaseEnded(ended))
                        if Some(ended.lease()) == self.lease.as_ref() =>
                    {
                        self.ended += 1;
                        if index == 0 {
                            self.old_lease = self.lease.take();
                            self.acquire(1)
                        } else {
                            if !self.retained
                                || !self.distinct_leases
                                || !self.stale_core
                                || !self.stale_native
                                || self.ended != 2
                            {
                                return Err("retention_contract");
                            }
                            self.outcome = "ResourceRetainedAcrossLeases";
                            self.begin_cleanup()
                        }
                    }
                    (Phase::Destroy, WorkBrowserResourceEvent::Destroyed(resource))
                        if Some(&resource) == self.resource.as_ref() =>
                    {
                        self.rows.seal();
                        if !self.rows.is_quiescent() {
                            return Err("resource_not_quiescent");
                        }
                        self.seal()
                    }
                    (_, WorkBrowserResourceEvent::DebtSettled(_)) if self.cleanup_started => Ok(()),
                    _ => Err("lifecycle_contract"),
                }
            }
            Reply::Read(completion) => {
                let event = self
                    .rows
                    .settle_observation(*completion, clock()?)
                    .map_err(|_| "read_owner")?;
                if self.cleanup_started && matches!(event, WorkBrowserObservationEvent::DebtSettled)
                {
                    return Ok(());
                }
                let Phase::Read(index, correlation) = self.phase.clone() else {
                    return Err("read_phase");
                };
                if let WorkBrowserObservationEvent::Refused(failure) = &event {
                    crate::diagnostic!(
                        "work-resource-read-refused: lease_index={index} failure={failure:?}"
                    );
                }
                let WorkBrowserObservationEvent::Snapshot(snapshot) = event else {
                    return Err("read_refused");
                };
                let origin =
                    SemanticOrigin::parse(self.target.as_url().as_str()).map_err(|_| "origin")?;
                let sample =
                    sample_foreground_snapshot(&snapshot, correlation.frame().context(), &origin)?;
                let controls = sample.document == Some(RenderingDocumentState::Complete)
                    && sample.load
                    && sample.microtask
                    && sample.timer;
                self.samples.push(ForegroundRenderingWitnessSample {
                    elapsed_ms: self
                        .rendering_started
                        .ok_or("rendering_clock")?
                        .elapsed()
                        .as_millis()
                        .try_into()
                        .map_err(|_| "clock")?,
                    nodes: sample.nodes,
                    controls,
                    animation_frame: sample.animation_frame,
                });
                if controls && sample.animation_frame {
                    self.render(RenderOp::Inspect, Some(index))
                } else if index == 0 {
                    self.phase = Phase::WaitingSample;
                    Ok(())
                } else {
                    Err("successor_sentinel")
                }
            }
            Reply::Rendering(request, evidence) => match self.phase.clone() {
                Phase::Render(expected) if request == expected => match evidence.state {
                    ForegroundRenderingState::Ready => self.acquire(0),
                    ForegroundRenderingState::Acquiring => self.render(RenderOp::Poll, None),
                    ForegroundRenderingState::DeferredForeground => Err("DeferredForeground"),
                    _ => Err("rendering_refused"),
                },
                Phase::Stamp(index, expected)
                    if request == expected && evidence.state == ForegroundRenderingState::Ready =>
                {
                    let stamp = evidence.stamp.ok_or("native_stamp")?;
                    self.native_counts[usize::from(index)] = stamp.completed();
                    if index == 0 {
                        if stamp.completed() == 0 {
                            return Err("native_counter");
                        }
                        self.stamp = Some(stamp);
                    } else {
                        self.retained = self
                            .stamp
                            .as_ref()
                            .is_some_and(|prior| prior.retained_after_one_read(&stamp));
                        if !self.retained {
                            return Err("native_retention");
                        }
                    }
                    self.revoke(index)
                }
                Phase::Retire(expected) if request == expected => match evidence.state {
                    ForegroundRenderingState::Retired => self.destroy(),
                    ForegroundRenderingState::Retiring => self.render(RenderOp::Retire, None),
                    _ => Err("rendering_retirement"),
                },
                _ => Err("rendering_phase"),
            },
            Reply::Native(ContextNativeEvent::ShutdownAuditSettled(settlement)) => {
                let Phase::Seal(expected) = self.phase else {
                    return Err("seal_phase");
                };
                if settlement.audit() != expected {
                    return Err("seal_owner");
                }
                let counts = settlement.outcome().map_err(|_| "seal_audit")?.counts();
                self.native_clean = [
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
                if !self.native_clean {
                    return Err("native_cohort_not_empty");
                }
                self.phase = Phase::Done;
                Ok(())
            }
            _ => Err("unexpected_reply"),
        }
    }
    fn begin_cleanup(&mut self) -> Result<(), &'static str> {
        if !self.cleanup_started {
            self.cleanup_started = true;
            self.deadline = Instant::now()
                .checked_add(CLEANUP_BUDGET)
                .ok_or("cleanup_deadline")?;
        }
        if self.render_attempted {
            self.render(RenderOp::Retire, None)
        } else {
            self.destroy()
        }
    }
    fn destroy(&mut self) -> Result<(), &'static str> {
        let Some(resource) = self.resource.as_ref() else {
            return self.seal();
        };
        let request = self.rows.destroy(resource).map_err(|_| "destroy")?;
        self.phase = Phase::Destroy;
        self.lifecycle(request)
    }
    fn seal(&mut self) -> Result<(), &'static str> {
        let audit = ContextResourceAuditId::new(1).ok_or("audit")?;
        self.phase = Phase::Seal(audit);
        (self.port.seal_for_shutdown(audit) == ContextShutdownDispatch::AuditScheduled)
            .then_some(())
            .ok_or("seal_dispatch")
    }
    fn fail(&mut self, reason: &'static str) {
        if self.cleanup_started {
            self.cleanup_failure = Some(reason);
            self.phase = Phase::Done;
        } else {
            self.outcome = reason;
            if let Some(resource) = self.resource.as_ref() {
                let _ = self.rows.quarantine(resource);
            }
            if let Err(cleanup) = self.begin_cleanup() {
                self.cleanup_failure = Some(cleanup);
                self.phase = Phase::Done;
            }
        }
    }
    fn finish(mut self) {
        self.timer = None;
        if let Ok(mut mailbox) = POLICY_MAILBOX.lock() {
            *mailbox = None;
        }
        let fixture_clean = self
            .fixture
            .take()
            .is_some_and(|fixture| fixture.shutdown().is_ok());
        self.human_preserved &= self.human.is_current();
        crate::diagnostic!("work-resource-retention: distinct_leases={} ended_leases={} unchanged_native_page_document_world={} stale_core_rejected={} stale_native_rejected={} native_completed_before={} native_completed_after={} resource_core_clean={}", self.distinct_leases, self.ended, self.retained, self.stale_core, self.stale_native, self.native_counts[0], self.native_counts[1], self.rows.is_quiescent());
        let report = ForegroundRenderingWitnessReport {
            outcome: self.outcome,
            cleanup_failure: self.cleanup_failure,
            samples: std::mem::take(&mut self.samples),
            native_cohort_clean: self.native_clean,
            human_ownership_preserved: self.human_preserved,
            fixture_clean,
            elapsed_ms: self
                .started
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        };
        let completion = self.completion.take();
        drop(self);
        if let Some(completion) = completion {
            completion(report);
        }
    }
}

fn rendering_reply_matches(phase: &Phase, request: &RenderRequest) -> bool {
    match phase {
        Phase::Render(expected) => {
            request == expected && matches!(request.operation, RenderOp::Acquire | RenderOp::Poll)
        }
        Phase::Stamp(index, expected) => {
            *index < 2 && request == expected && request.operation == RenderOp::Inspect
        }
        Phase::Retire(expected) => request == expected && request.operation == RenderOp::Retire,
        _ => false,
    }
}

fn lifecycle_stage(phase: &Phase) -> &'static str {
    match phase {
        Phase::Construct => "construct_refused",
        Phase::Acquire(0) => "acquire_a_refused",
        Phase::Acquire(1) => "acquire_b_refused",
        Phase::Revoke(0) => "revoke_a_refused",
        Phase::Revoke(1) => "revoke_b_refused",
        Phase::Destroy => "destroy_refused",
        _ => "lifecycle_phase_refused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn resource() -> WorkBrowserResourceJoin {
        WorkBrowserResources::new(WorkId::generate(), AgentWorkProfileId::generate())
            .construct(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                AgentPolicyInstant::from_millis(0),
            )
            .unwrap()
            .resource()
            .clone()
    }
    #[test]
    fn rendering_receipts_bind_exact_resource_operation_phase_and_lease_slot() {
        let request = RenderRequest {
            resource: resource(),
            operation: RenderOp::Inspect,
            document: crate::agent_context_port::resource_witness::Document::RenderingFixture,
        };
        assert!(rendering_reply_matches(
            &Phase::Stamp(0, request.clone()),
            &request
        ));
        assert!(rendering_reply_matches(
            &Phase::Stamp(1, request.clone()),
            &request
        ));
        for phase in [
            Phase::Stamp(2, request.clone()),
            Phase::Render(request.clone()),
            Phase::Retire(request.clone()),
            Phase::Construct,
            Phase::Acquire(0),
            Phase::Revoke(0),
            Phase::Destroy,
            Phase::Done,
        ] {
            assert!(!rendering_reply_matches(&phase, &request));
        }
        let foreign = RenderRequest {
            resource: resource(),
            operation: RenderOp::Inspect,
            document: crate::agent_context_port::resource_witness::Document::RenderingFixture,
        };
        assert!(!rendering_reply_matches(
            &Phase::Stamp(0, request.clone()),
            &foreign
        ));
        #[cfg(feature = "native-agentic-public-resource-probe")]
        {
            let changed = RenderRequest {
                document: crate::agent_context_port::resource_witness::Document::PublicProductBrief,
                ..request.clone()
            };
            assert!(!rendering_reply_matches(
                &Phase::Stamp(0, request.clone()),
                &changed
            ));
        }
        for operation in [RenderOp::Acquire, RenderOp::Poll, RenderOp::Retire] {
            let changed = RenderRequest {
                resource: request.resource.clone(),
                operation,
                document: request.document,
            };
            assert!(!rendering_reply_matches(
                &Phase::Stamp(0, request.clone()),
                &changed
            ));
        }
    }
}
