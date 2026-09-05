//! Provider-free fixed witness on the existing application's normal main queue.
//! No NSApplication bootstrap, nested event loop or manual event dispatch.

use super::{
    agentic_foreground_probe::HumanForegroundGuard,
    agentic_semantic_probe::{sample_foreground_snapshot, RenderingDocumentState},
    ContentPolicyTimeout,
};
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

const OFFSETS_MS: [u64; 8] = [0, 50, 100, 200, 400, 800, 1600, 3200];
const TOTAL_BUDGET: Duration = Duration::from_secs(15);
const CLEANUP_BUDGET: Duration = Duration::from_secs(5);

/// Called only after normal application/engine shutdown; never forces release.
pub fn foreground_rendering_native_drain() -> Option<bool> {
    EXPECTED_DRAIN.with(|context| {
        context
            .get()
            .and_then(super::agentic_foreground_probe::native_witness_drained)
    })
}

/// Content-free actual-lifecycle result; full application teardown is separate.
#[derive(Debug)]
pub struct ForegroundRenderingWitnessReport {
    /// Closed measurement/deferral/failure stage.
    pub outcome: &'static str,
    /// Cleanup failure never overwrites the original measurement/failure stage.
    pub cleanup_failure: Option<&'static str>,
    /// Exact synthetic controls and RAF reveal, never raw page text.
    pub samples: Vec<ForegroundRenderingWitnessSample>,
    /// Exact port seal reported zero native cohort resources.
    pub native_cohort_clean: bool,
    /// Original human owner remained unchanged through the last native step.
    pub human_ownership_preserved: bool,
    /// Fixed loopback listener/worker were retired successfully.
    pub fixture_clean: bool,
    /// Elapsed time from admission, not application launch.
    pub elapsed_ms: u64,
}

/// One fresh exact current-document semantic sample.
#[derive(Debug)]
pub struct ForegroundRenderingWitnessSample {
    /// Milliseconds from admitted rendering acquisition.
    pub elapsed_ms: u64,
    /// Bounded retained semantic nodes.
    pub nodes: usize,
    /// Exact fixture document/load/microtask/timer markers all matched.
    pub controls: bool,
    /// Exact fixture RAF-gated paragraph matched.
    pub animation_frame: bool,
}

enum Reply {
    Native(ContextNativeEvent),
    Rendering(ForegroundRenderingProbeRequest, ForegroundRenderingState),
    Policy(bool),
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
    static EXPECTED_DRAIN: std::cell::Cell<Option<ContextJoin>> = const { std::cell::Cell::new(None) };
}

/// Intercepts only the exact diagnostic profile's fixed policy receipt.
pub fn foreground_rendering_policy_event(event: &EngineEvent) -> bool {
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
    Construct(ContextOperationJoin),
    Navigate(ContextOperationJoin),
    Rendering(ForegroundRenderingProbeRequest),
    WaitingSample,
    Observe(SemanticRuntimeCorrelation),
    Retiring(ForegroundRenderingProbeRequest),
    Close(ContextOperationJoin),
    Seal(ContextResourceAuditId),
    Done,
}

struct Driver {
    engine: Arc<crate::WebviewEngine>,
    port: Arc<dyn AgentBrowserPort>,
    mailbox: Arc<Mailbox>,
    rx: Receiver<Reply>,
    human: HumanForegroundGuard,
    registry: ContextRegistry,
    _profiles: ContextProfileLeaseRegistry,
    lease: ContextProfileLease,
    identity: ContextIdentity,
    target: ContextNavigationTarget,
    fixture: Option<FixtureServer>,
    phase: Phase,
    timer: Option<ContentPolicyTimeout>,
    started: Instant,
    deadline: Instant,
    rendering_started: Option<Instant>,
    cleanup_started: bool,
    acquired: bool,
    next_sample: usize,
    samples: Vec<ForegroundRenderingWitnessSample>,
    outcome: &'static str,
    cleanup_failure: Option<&'static str>,
    native_clean: bool,
    human_preserved: bool,
    completion: Option<Box<dyn FnOnce(ForegroundRenderingWitnessReport) + Send>>,
}

/// Starts once, only on the actual application's main thread and foreground.
/// The composition root must independently identify its exact main surface.
pub fn start_foreground_rendering_witness(
    engine: Arc<crate::WebviewEngine>,
    completion: impl FnOnce(ForegroundRenderingWitnessReport) + Send + 'static,
) -> Result<(), &'static str> {
    MainThreadMarker::new().ok_or("main_thread")?;
    if USED.with(|used| used.replace(true)) {
        return Err("already_used");
    }
    let Some(human) = HumanForegroundGuard::capture() else {
        completion(ForegroundRenderingWitnessReport {
            outcome: "DeferredForeground",
            cleanup_failure: None,
            samples: Vec::new(),
            native_cohort_clean: true,
            human_ownership_preserved: true,
            fixture_clean: true,
            elapsed_ms: 0,
        });
        return Ok(());
    };
    let started = Instant::now();
    let deadline = started.checked_add(TOTAL_BUDGET).ok_or("deadline")?;
    let identity = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        AgentWorkProfileId::generate(),
        ContextKind::Owned,
    );
    let generation = ContentPolicyGeneration::new(1).ok_or("policy_generation")?;
    let (tx, rx) = mpsc::sync_channel(4);
    let mailbox = Arc::new(Mailbox {
        profile: identity.profile(),
        generation,
        tx,
        failed: AtomicBool::new(false),
    });
    let native_mailbox = mailbox.clone();
    let port = engine
        .take_agent_browser_port(move |event| native_mailbox.deliver(Reply::Native(event)))
        .ok_or("native_port")?;
    let fixture = FixtureServer::start().map_err(|_| "fixture")?;
    let target = ContextNavigationTarget::parse(&fixture.url(FixtureRoute::SemanticRendering))
        .map_err(|_| "fixture_target")?;
    let mut profiles = ContextProfileLeaseRegistry::new();
    let lease = profiles
        .acquire(
            ContextProfileLeaseId::new(1).ok_or("profile_id")?,
            identity,
            ContextProfileStorageClass::Ephemeral,
            ContextProfileLeasePurpose::Owned,
        )
        .map_err(|_| "profile_lease")?;
    let mut registry = ContextRegistry::new();
    registry
        .reserve(identity, capabilities()?)
        .map_err(|_| "context_reserve")?;
    *POLICY_MAILBOX.lock().map_err(|_| "policy_mailbox")? = Some(mailbox.clone());
    let driver = Driver {
        engine,
        port,
        mailbox,
        rx,
        human,
        registry,
        _profiles: profiles,
        lease,
        identity,
        target,
        fixture: Some(fixture),
        phase: Phase::Policy,
        timer: None,
        started,
        deadline,
        rendering_started: None,
        cleanup_started: false,
        acquired: false,
        next_sample: 0,
        samples: Vec::with_capacity(8),
        outcome: "ControlsIncomplete",
        cleanup_failure: None,
        native_clean: false,
        human_preserved: true,
        completion: Some(Box::new(completion)),
    };
    DRIVER.with(|slot| *slot.borrow_mut() = Some(driver));
    let admitted = DRIVER.with(|slot| {
        slot.borrow().as_ref().is_some_and(|d| {
            d.engine.install_content_rules(
                d.identity.profile(),
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

/// Requests cleanup without activating the application or reacquiring authority.
pub fn cancel_foreground_rendering_witness() -> bool {
    if MainThreadMarker::new().is_none() {
        return false;
    }
    DRIVER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(driver) = slot.as_mut() else {
            return false;
        };
        driver.fail("Cancelled");
        true
    })
}

fn capabilities() -> Result<ContextCapabilities, &'static str> {
    ContextCapabilities::try_new(
        ContextKind::Owned,
        &[ContextCapability::Observe, ContextCapability::Navigate],
    )
    .map_err(|_| "capabilities")
}

fn fail_driver(reason: &'static str) {
    DRIVER.with(|slot| {
        if let Some(driver) = slot.borrow_mut().as_mut() {
            driver.fail(reason);
        }
    });
}

fn schedule_tick() {
    let timer = super::schedule_content_policy_timeout(Duration::from_millis(25), tick);
    DRIVER.with(|slot| {
        if let Some(driver) = slot.borrow_mut().as_mut() {
            driver.timer = timer;
            if driver.timer.is_none() {
                if driver.cleanup_started {
                    driver.cleanup_failure = Some("timer_unavailable");
                } else {
                    driver.outcome = "timer_unavailable";
                }
                driver.phase = Phase::Done;
            }
        }
    });
    finish_if_done();
}

fn tick() {
    DRIVER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(driver) = slot.as_mut() else {
            return;
        };
        driver.timer = None;
        if let Err(reason) = driver.advance() {
            driver.fail(reason);
        }
    });
    if !finish_if_done() {
        schedule_tick();
    }
}

fn finish_if_done() -> bool {
    let done = DRIVER.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_none_or(|driver| matches!(driver.phase, Phase::Done))
    });
    if !done {
        return false;
    }
    let driver = DRIVER.with(|slot| slot.borrow_mut().take());
    if let Some(driver) = driver {
        driver.finish();
    }
    true
}

impl Driver {
    fn dispatch(&self, request: ContextNativeRequest) -> Result<(), &'static str> {
        (self.port.dispatch(request) == ContextDispatch::Scheduled)
            .then_some(())
            .ok_or("native_dispatch")
    }
    fn join(&self) -> Result<ContextJoin, &'static str> {
        self.registry
            .join(self.identity.id())
            .map_err(|_| "context_join")
    }

    fn render(&mut self, operation: ForegroundRenderingProbeOperation) -> Result<(), &'static str> {
        let request = ForegroundRenderingProbeRequest::new(self.join()?, operation);
        let mailbox = self.mailbox.clone();
        self.phase = if operation == ForegroundRenderingProbeOperation::Retire {
            Phase::Retiring(request)
        } else {
            Phase::Rendering(request)
        };
        let scheduled = self.port.probe_foreground_rendering(
            request,
            Box::new(move |request, state| mailbox.deliver(Reply::Rendering(request, state))),
        ) == ContextDispatch::Scheduled;
        if scheduled && operation == ForegroundRenderingProbeOperation::Acquire {
            self.acquired = true;
            EXPECTED_DRAIN.with(|context| context.set(Some(request.context())));
        }
        scheduled.then_some(()).ok_or("rendering_dispatch")
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
            let started = self.rendering_started.ok_or("rendering_clock")?;
            if self.next_sample >= OFFSETS_MS.len() {
                if self.samples.iter().any(|sample| sample.controls) {
                    self.outcome = "AnimationFrameNotObservedWithinWindow";
                }
                self.begin_cleanup()?;
            } else if started.elapsed() >= Duration::from_millis(OFFSETS_MS[self.next_sample]) {
                self.observe()?;
            }
        }
        Ok(())
    }

    fn reply(&mut self, reply: Reply) -> Result<(), &'static str> {
        if !reply_matches(&self.phase, &reply) {
            return Err("unexpected_reply");
        }
        match (self.phase.clone(), reply) {
            (Phase::Policy, Reply::Policy(true)) => {
                let operation = self
                    .registry
                    .begin_context(
                        self.identity.id(),
                        ContextOperationId::new(1).ok_or("operation")?,
                    )
                    .map_err(|_| "construct")?;
                let request = ContextConstructionRequest::try_new(
                    operation,
                    capabilities()?,
                    self.lease,
                    ContextConstructionSource::Owned,
                )
                .map_err(|_| "construct_request")?;
                self.phase = Phase::Construct(operation);
                self.dispatch(ContextNativeRequest::Construct(request))?;
            }
            (
                Phase::Construct(expected),
                Reply::Native(ContextNativeEvent::ConstructionSettled(settlement)),
            ) if settlement.operation() == expected => {
                let applied = settlement.outcome().is_ok();
                self.registry
                    .settle_construction(
                        self.identity.id(),
                        expected,
                        if applied {
                            ContextSettlement::Applied
                        } else {
                            ContextSettlement::Refused
                        },
                    )
                    .map_err(|_| "construct_settlement")?;
                if !applied {
                    return Err("construct_refused");
                }
                let operation = self
                    .registry
                    .begin_navigation(
                        self.identity.id(),
                        ContextOperationId::new(2).ok_or("operation")?,
                    )
                    .map_err(|_| "navigate")?;
                self.phase = Phase::Navigate(operation);
                self.dispatch(ContextNativeRequest::Navigate(
                    ContextNavigationRequest::try_new(operation, self.target.clone())
                        .map_err(|_| "navigate_request")?,
                ))?;
            }
            (
                Phase::Navigate(expected),
                Reply::Native(ContextNativeEvent::NavigationSettled(settlement)),
            ) if settlement.operation() == expected => {
                let applied = settlement.outcome() == &Ok(self.target.clone());
                self.registry
                    .settle_navigation(
                        self.identity.id(),
                        expected,
                        if applied {
                            ContextSettlement::Applied
                        } else {
                            ContextSettlement::Refused
                        },
                    )
                    .map_err(|_| "navigate_settlement")?;
                if !applied {
                    return Err("navigate_refused");
                }
                self.rendering_started = Some(Instant::now());
                self.render(ForegroundRenderingProbeOperation::Acquire)?;
            }
            (Phase::Rendering(expected), Reply::Rendering(request, state))
                if request == expected =>
            {
                match state {
                    ForegroundRenderingState::Ready => {
                        self.acquired = true;
                        self.phase = Phase::WaitingSample;
                    }
                    ForegroundRenderingState::Acquiring => {
                        self.acquired = true;
                        self.render(ForegroundRenderingProbeOperation::Poll)?;
                    }
                    ForegroundRenderingState::DeferredForeground => {
                        return Err("DeferredForeground")
                    }
                    ForegroundRenderingState::Expired => return Err("RenderingExpired"),
                    _ => return Err("rendering_refused"),
                }
            }
            (
                Phase::Observe(expected),
                Reply::Native(ContextNativeEvent::SemanticRuntimeSettled(settlement)),
            ) if settlement.correlation() == &expected => {
                let snapshot = settlement.into_outcome().map_err(|_| "semantic_refused")?;
                let origin =
                    SemanticOrigin::parse(self.target.as_url().as_str()).map_err(|_| "origin")?;
                let sample = sample_foreground_snapshot(&snapshot, self.join()?, &origin)?;
                let controls = sample.document == Some(RenderingDocumentState::Complete)
                    && sample.load
                    && sample.microtask
                    && sample.timer;
                let elapsed_ms = self
                    .rendering_started
                    .ok_or("rendering_clock")?
                    .elapsed()
                    .as_millis()
                    .try_into()
                    .map_err(|_| "clock")?;
                self.samples.push(ForegroundRenderingWitnessSample {
                    elapsed_ms,
                    nodes: sample.nodes,
                    controls,
                    animation_frame: sample.animation_frame,
                });
                self.next_sample += 1;
                if controls && sample.animation_frame {
                    self.outcome = "AnimationFrameObserved";
                    self.begin_cleanup()?;
                } else {
                    self.phase = Phase::WaitingSample;
                }
            }
            (Phase::Retiring(expected), Reply::Rendering(request, state))
                if request == expected =>
            {
                match state {
                    ForegroundRenderingState::Retired => self.close()?,
                    ForegroundRenderingState::Retiring => {
                        self.render(ForegroundRenderingProbeOperation::Retire)?
                    }
                    _ => return Err("rendering_retirement"),
                }
            }
            (
                Phase::Close(expected),
                Reply::Native(ContextNativeEvent::TransitionSettled(settlement)),
            ) if settlement.operation() == expected => {
                let applied = settlement.outcome().is_ok();
                self.registry
                    .settle_close(
                        self.identity.id(),
                        expected,
                        if applied {
                            ContextSettlement::Applied
                        } else {
                            ContextSettlement::Refused
                        },
                    )
                    .map_err(|_| "close_settlement")?;
                if !applied {
                    return Err("close_refused");
                }
                let audit = ContextResourceAuditId::new(1).ok_or("audit_id")?;
                self.phase = Phase::Seal(audit);
                if self.port.seal_for_shutdown(audit) != ContextShutdownDispatch::AuditScheduled {
                    return Err("seal_dispatch");
                }
            }
            (
                Phase::Seal(expected),
                Reply::Native(ContextNativeEvent::ShutdownAuditSettled(settlement)),
            ) if settlement.audit() == expected => {
                let counts = settlement.outcome().map_err(|_| "seal_audit")?.counts();
                self.native_clean = empty_counts(&counts);
                if !self.native_clean {
                    return Err("native_cohort_not_empty");
                }
                self.phase = Phase::Done;
            }
            _ => return Err("unexpected_reply"),
        }
        Ok(())
    }

    fn observe(&mut self) -> Result<(), &'static str> {
        let context = self.join()?;
        let id = u64::try_from(self.next_sample + 1).map_err(|_| "sample_id")?;
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse(self.target.as_url().as_str()).map_err(|_| "origin")?,
            SemanticFrameTrust::SameOrigin,
        )
        .map_err(|_| "frame")?;
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(id).ok_or("sample_id")?,
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let invocation = encode_semantic_runtime_invocation(
            &request,
            frame,
            SemanticInvocationId::new(id).ok_or("sample_id")?,
            SemanticSnapshotGeneration::new(id).ok_or("sample_id")?,
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .map_err(|_| "semantic_encode")?;
        self.phase = Phase::Observe(invocation.correlation());
        (self.port.invoke_semantic(invocation) == ContextDispatch::Scheduled)
            .then_some(())
            .ok_or("semantic_dispatch")
    }

    fn begin_cleanup(&mut self) -> Result<(), &'static str> {
        if !self.cleanup_started {
            self.cleanup_started = true;
            self.deadline = Instant::now()
                .checked_add(CLEANUP_BUDGET)
                .ok_or("cleanup_deadline")?;
        }
        if self.acquired {
            self.render(ForegroundRenderingProbeOperation::Retire)
        } else {
            self.close()
        }
    }

    fn close(&mut self) -> Result<(), &'static str> {
        let operation = self
            .registry
            .begin_close(
                self.identity.id(),
                ContextOperationId::new(3).ok_or("operation")?,
            )
            .map_err(|_| "close")?;
        self.phase = Phase::Close(operation);
        self.dispatch(ContextNativeRequest::Transition(
            ContextTransitionRequest::try_new(operation).map_err(|_| "close_request")?,
        ))
    }

    fn fail(&mut self, reason: &'static str) {
        if self.cleanup_started {
            self.cleanup_failure = Some(reason);
            self.phase = Phase::Done;
        } else {
            self.outcome = reason;
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

fn empty_counts(counts: &ContextNativeResourceCounts) -> bool {
    [
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
    .all(|count| count == 0)
}

fn reply_matches(phase: &Phase, reply: &Reply) -> bool {
    match (phase, reply) {
        (Phase::Policy, Reply::Policy(_)) => true,
        (
            Phase::Construct(expected),
            Reply::Native(ContextNativeEvent::ConstructionSettled(settlement)),
        ) => *expected == settlement.operation(),
        (
            Phase::Navigate(expected),
            Reply::Native(ContextNativeEvent::NavigationSettled(settlement)),
        ) => *expected == settlement.operation(),
        (Phase::Rendering(expected), Reply::Rendering(request, _))
        | (Phase::Retiring(expected), Reply::Rendering(request, _)) => expected == request,
        (
            Phase::Observe(expected),
            Reply::Native(ContextNativeEvent::SemanticRuntimeSettled(settlement)),
        ) => expected == settlement.correlation(),
        (
            Phase::Close(expected),
            Reply::Native(ContextNativeEvent::TransitionSettled(settlement)),
        ) => *expected == settlement.operation(),
        (
            Phase::Seal(expected),
            Reply::Native(ContextNativeEvent::ShutdownAuditSettled(settlement)),
        ) => *expected == settlement.audit(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined() -> (ContextRegistry, ContextIdentity, ContextJoin) {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            AgentWorkProfileId::generate(),
            ContextKind::Owned,
        );
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities().unwrap()).unwrap();
        let operation = registry
            .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
            .unwrap();
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .unwrap();
        let join = registry.join(identity.id()).unwrap();
        (registry, identity, join)
    }

    #[test]
    fn foreground_driver_never_accepts_wrong_phase_operation_or_document() {
        let (mut registry, identity, context) = joined();
        let request = ForegroundRenderingProbeRequest::new(
            context,
            ForegroundRenderingProbeOperation::Acquire,
        );
        let phase = Phase::Rendering(request);
        assert!(reply_matches(
            &phase,
            &Reply::Rendering(request, ForegroundRenderingState::DeferredForeground)
        ));
        let wrong = ForegroundRenderingProbeRequest::new(
            context,
            ForegroundRenderingProbeOperation::Retire,
        );
        assert!(!reply_matches(
            &phase,
            &Reply::Rendering(wrong, ForegroundRenderingState::Ready)
        ));
        let successor = registry
            .begin_navigation(identity.id(), ContextOperationId::new(2).unwrap())
            .unwrap()
            .context();
        let wrong = ForegroundRenderingProbeRequest::new(
            successor,
            ForegroundRenderingProbeOperation::Acquire,
        );
        assert!(!reply_matches(
            &phase,
            &Reply::Rendering(wrong, ForegroundRenderingState::Ready)
        ));
        for wrong_phase in [Phase::Policy, Phase::WaitingSample, Phase::Done] {
            assert!(!reply_matches(
                &wrong_phase,
                &Reply::Rendering(request, ForegroundRenderingState::Ready)
            ));
        }
    }

    #[test]
    fn foreground_driver_cannot_substitute_a_semantic_generation_or_an_ordinary_audit() {
        let (_, _, context) = joined();
        let invocation = |id| {
            let frame = SemanticFrameJoin::try_new(
                context,
                FrameId::MAIN,
                context.frame_generation(),
                SemanticOrigin::parse("http://127.0.0.1:12345").unwrap(),
                SemanticFrameTrust::SameOrigin,
            )
            .unwrap();
            encode_semantic_runtime_invocation(
                &SemanticObservationRequest::initial(
                    SemanticObservationId::new(id).unwrap(),
                    context,
                    SemanticObservationBudget::INITIAL_FILTERED,
                ),
                frame,
                SemanticInvocationId::new(id).unwrap(),
                SemanticSnapshotGeneration::new(id).unwrap(),
                SemanticRuntimeBudget::INITIAL_FILTERED,
            )
            .unwrap()
        };
        let expected = invocation(1).correlation();
        let reply = |correlation| {
            Reply::Native(ContextNativeEvent::SemanticRuntimeSettled(Box::new(
                SemanticRuntimeSettlement::try_new(
                    correlation,
                    Err(SemanticRuntimePortFailure::NotReady),
                )
                .unwrap(),
            )))
        };
        assert!(reply_matches(
            &Phase::Observe(expected.clone()),
            &reply(expected.clone())
        ));
        assert!(!reply_matches(
            &Phase::Observe(expected),
            &reply(invocation(2).correlation())
        ));
        let audit = ContextResourceAuditId::new(1).unwrap();
        assert!(!reply_matches(
            &Phase::Seal(audit),
            &Reply::Native(ContextNativeEvent::ResourceAuditSettled(
                ContextResourceAuditSettlement::new(audit, Err(ContextPortFailure::NativeRefused))
            ))
        ));
    }

    #[test]
    fn foreground_driver_mailbox_never_evicts_and_failure_is_sticky() {
        let (tx, rx) = mpsc::sync_channel(4);
        let mailbox = Mailbox {
            profile: AgentWorkProfileId::generate(),
            generation: ContentPolicyGeneration::new(1).unwrap(),
            tx,
            failed: AtomicBool::new(false),
        };
        for _ in 0..4 {
            mailbox.deliver(Reply::Policy(true));
        }
        assert!(!mailbox.failed.load(Ordering::Acquire));
        mailbox.deliver(Reply::Policy(false));
        assert!(mailbox.failed.load(Ordering::Acquire));
        for _ in 0..4 {
            assert!(matches!(rx.try_recv(), Ok(Reply::Policy(true))));
        }
        assert!(rx.try_recv().is_err());
        assert!(mailbox.failed.load(Ordering::Acquire));
    }

    #[test]
    fn foreground_driver_zero_requires_every_native_owner_class() {
        let zero = ContextNativeResourceCounts {
            known_bindings: 0,
            resident_views: 0,
            owned_reservations: 0,
            borrowed_leases: 0,
            visible_surfaces: 0,
            suspended_views: 0,
            pending_operations: 0,
            pending_captures: 0,
            queued_tasks: 0,
        };
        assert!(empty_counts(&zero));
        for change in [
            |c: &mut ContextNativeResourceCounts| c.known_bindings = 1,
            |c: &mut ContextNativeResourceCounts| c.resident_views = 1,
            |c: &mut ContextNativeResourceCounts| c.owned_reservations = 1,
            |c: &mut ContextNativeResourceCounts| c.borrowed_leases = 1,
            |c: &mut ContextNativeResourceCounts| c.visible_surfaces = 1,
            |c: &mut ContextNativeResourceCounts| c.suspended_views = 1,
            |c: &mut ContextNativeResourceCounts| c.pending_operations = 1,
            |c: &mut ContextNativeResourceCounts| c.pending_captures = 1,
            |c: &mut ContextNativeResourceCounts| c.queued_tasks = 1,
        ] {
            let mut nonzero = zero;
            change(&mut nonzero);
            assert!(!empty_counts(&nonzero));
        }
    }
}
