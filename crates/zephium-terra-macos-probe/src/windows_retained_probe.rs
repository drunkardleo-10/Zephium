//! Provider-free qualification of the original retained Work native lifecycle.
//! No model, credential, endpoint override or legacy context admission is used.

use std::{
    io::Write as _,
    sync::{mpsc, Arc, Mutex},
    task::{Wake, Waker},
    time::{Duration, Instant},
};
use zephium_agentic::*;
use zephium_core::{
    ids::{ProfileId, SpaceId},
    ports::store::Store,
    profiles::ProfileKind,
    session::{PersistedProfile, PersistedSpace, SessionState},
};

const PHASE_WINDOW: Duration = Duration::from_secs(45);
const TOTAL_BUDGET: Duration = Duration::from_secs(210);
const OPERATION_BUDGET: Duration = Duration::from_secs(20);

struct Notification;
impl Wake for Notification {
    fn wake(self: Arc<Self>) {}
}
fn wake() -> Waker {
    Waker::from(Arc::new(Notification))
}
fn clock() -> Result<AgentPolicyInstant, &'static str> {
    zephium_engine::work_browser_monotonic_now().ok_or("retained_clock")
}
fn receive<T>(receiver: &mpsc::Receiver<T>) -> Result<T, &'static str> {
    receiver
        .recv_timeout(OPERATION_BUDGET)
        .map_err(|_| "retained_callback_deadline")
}
fn lifecycle(
    rows: &mut WorkBrowserResources,
    port: &dyn AgentBrowserPort,
    request: WorkBrowserResourceRequest,
) -> Result<WorkBrowserResourceEvent, &'static str> {
    let (send, answer) = mpsc::sync_channel(1);
    let operation = request.operation();
    let terminal = match port.work_resource_lifecycle(
        request,
        Box::new(move |result| {
            let _ = send.try_send(result);
        }),
    ) {
        WorkBrowserResourceDispatch::Scheduled => rows
            .settle_at(receive(&answer)?, clock()?)
            .map_err(|_| "retained_terminal_join"),
        WorkBrowserResourceDispatch::Rejected { request, failure } => {
            let _ = writeln!(std::io::stderr().lock(), "windows-retained-lifecycle: operation={operation:?} dispatch_refused={failure:?}; content=redacted");
            let _ = rows.dispatch_refused(*request, failure);
            Err("retained_native_rejected")
        }
    };
    if let Ok(WorkBrowserResourceEvent::Quarantined(failure)) = &terminal {
        let _ = writeln!(std::io::stderr().lock(), "windows-retained-lifecycle: operation={operation:?} quarantined={failure:?}; content=redacted");
    }
    terminal
}
fn construct(
    engine: &zephium_engine::WebviewEngine,
    rows: &mut WorkBrowserResources,
    port: &dyn AgentBrowserPort,
    document: &ContextNavigationTarget,
) -> Result<(WorkBrowserResourceJoin, WorkBrowserResourceHealth), &'static str> {
    let request = rows
        .construct_document(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            ContextProfileStorageClass::Durable,
            document.clone(),
            clock()?,
        )
        .map_err(|_| "retained_construct_admission")?;
    let resource = request.resource().clone();
    let (request, mut health) = request
        .track_resource_health()
        .map_err(|_| "retained_health_owner")?;
    if health.register(wake()) != WorkBrowserResourceHealthState::Pending {
        return Err("retained_health_registration");
    }
    let terminal = lifecycle(rows, port, request);
    let health_state = health.snapshot();
    let (kind, joined) = match &terminal {
        Ok(WorkBrowserResourceEvent::Retained(join)) => ("retained", join == &resource),
        Ok(WorkBrowserResourceEvent::Quarantined(failure)) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "windows-retained-lifecycle: construct_core_failure={failure:?}; content=redacted"
            );
            ("quarantined", false)
        }
        Ok(WorkBrowserResourceEvent::AdmissionRefused { failure, .. }) => {
            let _=writeln!(std::io::stderr().lock(),"windows-retained-lifecycle: construct_dispatch_failure={failure:?}; content=redacted");
            ("admission_refused", false)
        }
        Ok(_) => ("unexpected", false),
        Err(_) => ("terminal_error", false),
    };
    let _=writeln!(std::io::stderr().lock(),"windows-retained-lifecycle: construct_terminal={kind} exact_join={joined} health={health_state:?}; content=redacted");
    if !matches!(&terminal,Ok(WorkBrowserResourceEvent::Retained(join)) if join==&resource) {
        let _ = writeln!(
            std::io::stderr().lock(),
            "windows-retained-lifecycle: construct_native_failure={:?}; content=redacted",
            engine.work_resource_failure_cause(&resource)
        );
    }
    match terminal? {
        WorkBrowserResourceEvent::Retained(join)
            if join == resource && health.poll() == WorkBrowserResourceHealthState::Current =>
        {
            Ok((resource, health))
        }
        _ => Err("retained_construct_terminal"),
    }
}

/// A visible committed page whose load terminal is held by one synthetic image.
/// The fixture releases it only after a picture arrives without any semantic read.
struct EarlyPreviewFixture {
    url: String,
    document_release: Arc<std::sync::atomic::AtomicBool>,
    release: Arc<std::sync::atomic::AtomicBool>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl EarlyPreviewFixture {
    fn start() -> Result<Self, &'static str> {
        Self::start_with_document_held(false)
    }
    fn start_with_document_held(held: bool) -> Result<Self, &'static str> {
        use std::io::Read;
        use std::sync::atomic::{AtomicBool, Ordering};
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").map_err(|_| "early_preview_bind")?;
        let port = listener
            .local_addr()
            .map_err(|_| "early_preview_address")?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|_| "early_preview_nonblocking")?;
        let release = Arc::new(AtomicBool::new(false));
        let document_release = Arc::new(AtomicBool::new(!held));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_release = release.clone();
        let worker_document_release = document_release.clone();
        let worker_stop = stop.clone();
        let worker = std::thread::Builder::new().name("early-preview-fixture".into()).spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                let Ok((mut stream, _)) = listener.accept() else { std::thread::sleep(Duration::from_millis(5)); continue; };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                let mut request = [0; 2048];
                let Ok(count) = stream.read(&mut request) else { continue; };
                let slow = request[..count].starts_with(b"GET /late.svg ");
                let (mime, body) = if slow {
                    while !worker_release.load(Ordering::Acquire) && !worker_stop.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    ("image/svg+xml", "<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'/>")
                } else {
                    while !worker_document_release.load(Ordering::Acquire) && !worker_stop.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    ("text/html", "<!doctype html><html><head><meta name='color-scheme' content='light'><style>html,body{margin:0;background:#2476b5;color:white;font:32px sans-serif}h1{padding:40px}img{width:1px;height:1px}</style></head><body><h1>Committed preview fixture</h1><img src='/late.svg'></body></html>")
                };
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        }).map_err(|_| "early_preview_worker")?;
        Ok(Self {
            url: format!("http://127.0.0.1:{port}/early.html"),
            release,
            document_release,
            stop,
            worker: Some(worker),
        })
    }
}
impl Drop for EarlyPreviewFixture {
    fn drop(&mut self) {
        self.document_release
            .store(true, std::sync::atomic::Ordering::Release);
        self.release
            .store(true, std::sync::atomic::Ordering::Release);
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn qualify_first_preview(
    rows: &mut WorkBrowserResources,
    port: &dyn AgentBrowserPort,
) -> Result<(), &'static str> {
    let fixture = EarlyPreviewFixture::start()?;
    let document =
        ContextNavigationTarget::parse(&fixture.url).map_err(|_| "early_preview_document")?;
    let request = rows
        .construct_document(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            ContextProfileStorageClass::Durable,
            document,
            clock()?,
        )
        .map_err(|_| "early_preview_admission")?;
    let resource = request.resource().clone();
    let (request, mut health) = request
        .track_resource_health()
        .map_err(|_| "early_preview_health")?;
    let _ = health.register(wake());
    let (send, answer) = mpsc::sync_channel(1);
    let started = Instant::now();
    if !matches!(
        port.work_resource_lifecycle(
            request,
            Box::new(move |result| {
                let _ = send.try_send(result);
            })
        ),
        WorkBrowserResourceDispatch::Scheduled
    ) {
        return Err("early_preview_dispatch");
    }
    let mut early_terminal = None;
    let proof = (|| {
        loop {
            if let Ok(terminal) = answer.try_recv() {
                early_terminal = Some(terminal);
                return Err("early_preview_constructed_before_first_picture");
            }
            if port.latest_work_frame(&resource).is_some() {
                break;
            }
            if started.elapsed() >= Duration::from_secs(10) {
                return Err("early_preview_picture_deadline");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let first_ms = started.elapsed().as_millis();
        let first = verify_frame(port, &resource)?;
        let pixels =
            image::load_from_memory_with_format(first.png.as_slice(), image::ImageFormat::Png)
                .map_err(|_| "early_preview_decode")?
                .to_rgb8();
        if pixels.width() != first.width || pixels.height() != first.height {
            return Err("early_preview_decoded_dimensions");
        }
        for (x, y) in [
            (first.width / 2, first.height / 2),
            (first.width / 4, first.height * 3 / 4),
            (first.width * 3 / 4, first.height * 3 / 4),
        ] {
            if !pixels
                .get_pixel(x, y)
                .0
                .iter()
                .zip([36u8, 118, 181])
                .all(|(observed, expected)| observed.abs_diff(expected) <= 8)
            {
                return Err("early_preview_committed_pixels");
            }
        }
        if let Ok(terminal) = answer.try_recv() {
            early_terminal = Some(terminal);
            return Err("early_preview_no_pending_load");
        }
        Ok((first_ms, first))
    })();
    fixture
        .release
        .store(true, std::sync::atomic::Ordering::Release);
    let terminal = rows
        .settle_at(
            early_terminal.map_or_else(|| receive(&answer), Ok)?,
            clock()?,
        )
        .map_err(|_| "early_preview_terminal_join")?;
    let retained = matches!(terminal, WorkBrowserResourceEvent::Retained(ref join) if join == &resource)
        && health.poll() == WorkBrowserResourceHealthState::Current;
    let total_ms = started.elapsed().as_millis();
    destroy_with_uncertain_health(rows, port, &resource, &mut health, !retained)?;
    let (first_ms, first) = proof?;
    if !retained {
        return Err("early_preview_construct_terminal");
    }
    let _ = writeln!(std::io::stdout().lock(), "windows-first-preview: before_semantic_completion=true semantic_reads=0 first_frame_ms={first_ms} construction_ms={total_ms} frame_generation={} width={} height={} image_bytes={} cleanup=true; content=synthetic", first.generation, first.width, first.height, first.png.len());
    Ok(())
}

fn qualify_cancelled_preview(
    engine: &zephium_engine::WebviewEngine,
    rows: &mut WorkBrowserResources,
    port: &dyn AgentBrowserPort,
) -> Result<(), &'static str> {
    let fixture = EarlyPreviewFixture::start_with_document_held(true)?;
    let request = rows
        .construct_document_with_isolation(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            ContextProfileStorageClass::Durable,
            ContextNavigationTarget::parse(&fixture.url).map_err(|_| "cancel_preview_document")?,
            WorkBrowserDocumentPolicy::Exact,
            true,
            clock()?,
        )
        .map_err(|_| "cancel_preview_admission")?;
    let resource = request.resource().clone();
    let session = WorkBrowserSession::new(
        resource.identity().profile(),
        resource.identity().work(),
        Instant::now() + OPERATION_BUDGET,
    );
    let request = request
        .with_anonymous_session(session.clone())
        .map_err(|_| "cancel_preview_session")?;
    let (request, mut health) = request
        .track_resource_health()
        .map_err(|_| "cancel_preview_health")?;
    let _ = health.register(wake());
    let (send, answer) = mpsc::sync_channel(1);
    if !matches!(
        port.work_resource_lifecycle(
            request,
            Box::new(move |result| {
                let _ = send.try_send(result);
            })
        ),
        WorkBrowserResourceDispatch::Scheduled
    ) {
        return Err("cancel_preview_dispatch");
    }
    let (capture_send, capture_answer) = mpsc::sync_channel(1);
    let capture_session = session.clone();
    let registered = engine.work_resource_on_frame_capture_dispatched(
        &resource,
        Box::new(move || {
            // This one-shot runs after successful native dispatch while the
            // ORIGINAL in-flight flag is set, without waiting on the UI thread.
            capture_session.close();
            let _ = capture_send.try_send(());
        }),
    );
    // Prevent even a fast first commit/capture from outrunning registration.
    fixture
        .document_release
        .store(true, std::sync::atomic::Ordering::Release);
    let proof = if registered {
        capture_answer
            .recv_timeout(OPERATION_BUDGET)
            .map_err(|_| "cancel_preview_capture_not_inflight")
    } else {
        Err("cancel_preview_capture_registration")
    };
    // Idempotent cleanup also closes the original session on a failed proof.
    session.close();
    fixture
        .release
        .store(true, std::sync::atomic::Ordering::Release);
    let terminal = rows
        .settle_at(receive(&answer)?, clock()?)
        .map_err(|_| "cancel_preview_terminal_join")?;
    let refused = matches!(terminal, WorkBrowserResourceEvent::Quarantined(_));
    let late_picture = port.latest_work_frame(&resource).is_some();
    destroy_with_uncertain_health(rows, port, &resource, &mut health, true)?;
    proof?;
    if !refused {
        return Err("cancel_preview_not_refused");
    }
    if late_picture {
        return Err("cancel_preview_late_picture");
    }
    if engine.work_resource_frame_capture_pending(&resource) == Some(true) {
        return Err("cancel_preview_capture_debt");
    }
    let _ = writeln!(std::io::stdout().lock(), "windows-first-preview-cancel: original_capture_inflight=true original_session_closed=true constructor_refused=true late_picture_discarded=true original_capture_drained=true cleanup=true; content=synthetic");
    Ok(())
}
fn observe(
    rows: &mut WorkBrowserResources,
    port: &dyn AgentBrowserPort,
    lease: &WorkBrowserExecutionLease,
) -> Result<u16, &'static str> {
    let snapshot = observe_snapshot(rows, port, lease)?;
    if snapshot.completeness() != SemanticCompleteness::Complete {
        return Err("retained_observation_terminal");
    }
    let mut progress = None;
    for node in snapshot.nodes() {
        if node.role() != SemanticRole::Heading {
            continue;
        }
        for value in [node.name(), node.text()].into_iter().flatten() {
            if let Some(counter) = value.as_str().strip_prefix("Retained native progress ") {
                let counter = counter
                    .parse::<u16>()
                    .map_err(|_| "retained_progress_shape")?;
                if counter > 4096 || progress.is_some_and(|prior| prior != counter) {
                    return Err("retained_progress_shape");
                }
                progress = Some(counter);
            }
        }
    }
    progress.ok_or("retained_progress_absent")
}
fn observe_snapshot(
    rows: &mut WorkBrowserResources,
    port: &dyn AgentBrowserPort,
    lease: &WorkBrowserExecutionLease,
) -> Result<SemanticSnapshot, &'static str> {
    let request = rows
        .observe_initial(lease, clock()?)
        .map_err(|_| "retained_observation_admission")?;
    let (send, answer) = mpsc::sync_channel(1);
    match port.work_resource_observe(
        request,
        Box::new(move |result| {
            let _ = send.try_send(result);
        }),
    ) {
        WorkBrowserObservationDispatch::Scheduled => {}
        WorkBrowserObservationDispatch::Rejected { request, failure } => {
            let _ = writeln!(std::io::stderr().lock(), "windows-retained-observation: dispatch=rejected failure={failure:?}; content=redacted");
            let _ = rows.observation_dispatch_refused(*request);
            return Err("retained_observation_rejected");
        }
    }
    let terminal = rows.settle_observation(receive(&answer)?, clock()?);
    match &terminal {
        Ok(WorkBrowserObservationEvent::Snapshot(snapshot)) => {
            let _ = writeln!(std::io::stderr().lock(), "windows-retained-observation: terminal=snapshot completeness={:?} node_count={}; content=redacted", snapshot.completeness(), snapshot.nodes().len());
        }
        Ok(WorkBrowserObservationEvent::DebtSettled) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "windows-retained-observation: terminal=debt_settled; content=redacted"
            );
        }
        Ok(WorkBrowserObservationEvent::Refused(failure)) => {
            let _ = writeln!(std::io::stderr().lock(), "windows-retained-observation: terminal=refused failure={failure:?}; content=redacted");
        }
        Err(failure) => {
            let _ = writeln!(std::io::stderr().lock(), "windows-retained-observation: terminal=join_error failure={failure:?}; content=redacted");
        }
    }
    match terminal.map_err(|_| "retained_observation_join")? {
        WorkBrowserObservationEvent::Snapshot(snapshot) if !snapshot.nodes().is_empty() => {
            Ok(*snapshot)
        }
        _ => Err("retained_observation_terminal"),
    }
}
fn phase_window(
    phase: &'static str,
    health: &mut WorkBrowserResourceHealth,
) -> Result<(), &'static str> {
    let _ = writeln!(std::io::stdout().lock(),
        "windows-retained-phase: ready={phase} pid={} window_seconds=45 observation_verified=true frame_verified=true frame_rate=unmeasured; content=redacted", std::process::id());
    let deadline = Instant::now() + PHASE_WINDOW;
    while Instant::now() < deadline {
        if health.poll() != WorkBrowserResourceHealthState::Current {
            return Err("retained_phase_health");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = writeln!(
        std::io::stdout().lock(),
        "windows-retained-phase: completed={phase}; content=redacted"
    );
    Ok(())
}
fn verify_frame(
    port: &dyn AgentBrowserPort,
    resource: &WorkBrowserResourceJoin,
) -> Result<Arc<WorkBrowserFrame>, &'static str> {
    let deadline = Instant::now() + OPERATION_BUDGET;
    let frame = loop {
        if let Some(frame) = port.latest_work_frame(resource) {
            break frame;
        }
        if Instant::now() >= deadline {
            return Err("retained_frame_deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let budget = SemanticScreenshotBudget::STANDARD;
    if frame.generation == 0
        || frame.width == 0
        || frame.height == 0
        || frame.width > u32::from(budget.max_width())
        || frame.height > u32::from(budget.max_height())
        || u64::from(frame.width) * u64::from(frame.height) > u64::from(budget.max_pixels())
        || frame.png.len() > budget.max_png_bytes() as usize
        || frame.png.len() < 33
        || frame.png.get(..8) != Some(b"\x89PNG\r\n\x1a\n")
        || frame.png.get(12..16) != Some(b"IHDR")
    {
        return Err("retained_frame_shape");
    }
    let width = u32::from_be_bytes(
        frame
            .png
            .get(16..20)
            .ok_or("retained_frame_width")?
            .try_into()
            .map_err(|_| "retained_frame_width")?,
    );
    let height = u32::from_be_bytes(
        frame
            .png
            .get(20..24)
            .ok_or("retained_frame_height")?
            .try_into()
            .map_err(|_| "retained_frame_height")?,
    );
    if width != frame.width || height != frame.height {
        return Err("retained_frame_dimensions");
    }
    let _=writeln!(std::io::stdout().lock(),"windows-retained-frame: generation={} width={} height={} png_bytes={} bounded_png=true; content=redacted",frame.generation,width,height,frame.png.len());
    Ok(frame)
}
fn destroy(
    rows: &mut WorkBrowserResources,
    port: &dyn AgentBrowserPort,
    resource: &WorkBrowserResourceJoin,
    health: &mut WorkBrowserResourceHealth,
) -> Result<(), &'static str> {
    destroy_with_uncertain_health(rows, port, resource, health, false)
}
fn destroy_with_uncertain_health(
    rows: &mut WorkBrowserResources,
    port: &dyn AgentBrowserPort,
    resource: &WorkBrowserResourceJoin,
    health: &mut WorkBrowserResourceHealth,
    allow_uncertain: bool,
) -> Result<(), &'static str> {
    let request = rows
        .destroy(resource)
        .map_err(|_| "retained_destroy_admission")?;
    if !matches!(lifecycle(rows, port, request)?, WorkBrowserResourceEvent::Destroyed(join) if &join==resource)
    {
        return Err("retained_destroy_terminal");
    }
    let deadline = Instant::now() + OPERATION_BUDGET;
    while !health.reporter_retired() {
        if Instant::now() >= deadline {
            return Err("retained_health_retirement_deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let state = health.poll();
    if state != WorkBrowserResourceHealthState::Retired
        && !(allow_uncertain && state == WorkBrowserResourceHealthState::Uncertain)
    {
        return Err("retained_health_retirement");
    }
    if port.latest_work_frame(resource).is_some() {
        return Err("retained_frame_retirement");
    }
    Ok(())
}
struct NativeEvents(mpsc::Receiver<ContextNativeEvent>);
impl AgentNativeShutdownEventSource for NativeEvents {
    fn wait_until(&mut self, wake: Instant) -> AgentNativeShutdownWait {
        match self
            .0
            .recv_timeout(wake.saturating_duration_since(Instant::now()))
        {
            Ok(event) => AgentNativeShutdownWait::Event(Box::new(event)),
            Err(mpsc::RecvTimeoutError::Timeout) => AgentNativeShutdownWait::Elapsed,
            Err(mpsc::RecvTimeoutError::Disconnected) => AgentNativeShutdownWait::Closed,
        }
    }
}
fn workflow(
    engine: &zephium_engine::WebviewEngine,
    shell: &zephium_app::Handle,
    profile: ProfileId,
    cookie_session: Option<bool>,
    wikipedia_read: bool,
    first_preview: bool,
) -> Result<(), &'static str> {
    let startup_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let readiness = shell.work_profile_binding();
        let binding = loop {
            if let Some(answer) = readiness.try_recv() {
                break answer;
            }
            if Instant::now() >= startup_deadline {
                return Err("retained_profile_deadline");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        match binding {
            zephium_app::AgentWorkProfileReadiness::Ready(binding)
                if binding.profile() == profile =>
            {
                break
            }
            zephium_app::AgentWorkProfileReadiness::PolicyPending(_)
            | zephium_app::AgentWorkProfileReadiness::PolicyMissing
            | zephium_app::AgentWorkProfileReadiness::ProfileMissing => {}
            _ => return Err("retained_profile_policy"),
        }
        if Instant::now() >= startup_deadline {
            return Err("retained_profile_deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if let Some(observation) = cookie_session {
        return cookie_workflow(engine, profile, observation);
    }
    if wikipedia_read {
        return wikipedia_workflow(engine, profile);
    }
    if first_preview {
        let mut factory = engine
            .take_agent_browser_lifetime_factory()
            .ok_or("early_preview_factory")?;
        let work = WorkId::generate();
        let (events, answer) = mpsc::sync_channel(2);
        let port = factory
            .begin_work_page(work, 1, move |event| {
                let _ = events.try_send(event);
            })
            .map_err(|_| "early_preview_port")?;
        let mut rows = WorkBrowserResources::new(work, profile);
        let proof = qualify_first_preview(&mut rows, port.as_ref())
            .and_then(|_| qualify_cancelled_preview(engine, &mut rows, port.as_ref()));
        rows.seal();
        let coordinator = rows
            .begin_native_shutdown()
            .map_err(|_| "early_preview_scope_close")?;
        let _zero = drive_agent_native_shutdown_until(
            coordinator,
            port.as_ref(),
            ContextResourceAuditId::new(1).ok_or("early_preview_audit")?,
            &mut NativeEvents(answer),
            Instant::now() + OPERATION_BUDGET,
        )
        .map_err(|_| "early_preview_native_zero")?;
        return proof;
    }
    let fixture = FixtureServer::start().map_err(|_| "retained_fixture")?;
    let document = ContextNavigationTarget::parse(&format!(
        "{}#windows-retained-progress",
        fixture.url(FixtureRoute::SemanticRendering)
    ))
    .map_err(|_| "retained_fixture_document")?;
    let work = WorkId::generate();
    let (events, answer) = mpsc::sync_channel(2);
    let mut factory = engine
        .take_agent_browser_lifetime_factory()
        .ok_or("retained_factory")?;
    let port = factory
        .begin_work_page(work, 1, move |event| {
            let _ = events.try_send(event);
        })
        .map_err(|_| "retained_port")?;
    let mut rows = WorkBrowserResources::new(work, profile);
    let (resource, mut health) = construct(engine, &mut rows, port.as_ref(), &document)?;
    let deadline =
        zephium_engine::work_browser_monotonic_deadline(Instant::now() + Duration::from_secs(110))
            .ok_or("retained_lease_deadline")?;
    let request = rows
        .acquire(&resource, ContextRunId::generate(), clock()?, deadline)
        .map_err(|_| "retained_acquire_admission")?;
    let lease = match lifecycle(&mut rows, port.as_ref(), request)? {
        WorkBrowserResourceEvent::Acquired(lease) if lease.resource() == &resource => lease,
        _ => return Err("retained_acquire_terminal"),
    };
    let progress_before = observe(&mut rows, port.as_ref(), &lease)?;
    let initial_frame = verify_frame(port.as_ref(), &resource)?;
    phase_window("leased_covered", &mut health)?;
    let progress_after = observe(&mut rows, port.as_ref(), &lease)?;
    if progress_after <= progress_before {
        return Err("retained_progress_not_advanced");
    }
    let _ = writeln!(
        std::io::stdout().lock(),
        "windows-retained-progress: advanced_while_leased=true counter_delta={}; content=redacted",
        progress_after - progress_before
    );
    // Burst settled reads while the asynchronous thumbnail may still be
    // finishing. The final demand must survive both the single-capture gate
    // and 250ms throttle, then drain with the original revocation owner.
    let _ = observe(&mut rows, port.as_ref(), &lease)?;
    let before_final = port
        .latest_work_frame(&resource)
        .ok_or("retained_final_frame_absent")?
        .generation;
    let _ = observe(&mut rows, port.as_ref(), &lease)?;
    let (request, mut ticket) = rows
        .revoke_with_delivery(&lease)
        .map_err(|_| "retained_revoke_admission")?;
    ticket
        .register_waker(wake())
        .map_err(|_| "retained_delivery_registration")?;
    let ended = match lifecycle(&mut rows, port.as_ref(), request)? {
        WorkBrowserResourceEvent::LeaseEnded(ended) if ended.lease() == &lease => ended,
        _ => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "windows-retained-lifecycle: revoke_native_failure={:?}; content=redacted",
                engine.work_resource_failure_cause(&resource)
            );
            return Err("retained_revoke_terminal");
        }
    };
    let delivery_deadline = Instant::now() + OPERATION_BUDGET;
    let receipt = loop {
        if let Some(receipt) = ticket
            .try_take()
            .map_err(|_| "retained_delivery_terminal")?
        {
            break receipt;
        }
        if Instant::now() >= delivery_deadline {
            return Err("retained_delivery_deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _proof = ended
        .join_delivery(receipt)
        .map_err(|_| "retained_delivery_join")?;
    let final_frame = verify_frame(port.as_ref(), &resource)?;
    if final_frame.generation <= before_final || final_frame.png == initial_frame.png {
        return Err("retained_final_frame_stale");
    }
    let _ = writeln!(std::io::stdout().lock(),
        "windows-retained-frame: latest_settled_generation={} prior_generation={} changed_pixels=true final_demand_drained_before_revoke=true; content=synthetic",
        final_frame.generation, before_final);
    phase_window("idle_retained", &mut health)?;
    if port
        .latest_work_frame(&resource)
        .is_none_or(|frame| frame.generation != final_frame.generation)
    {
        return Err("retained_idle_frame_changed");
    }
    destroy(&mut rows, port.as_ref(), &resource, &mut health)?;
    // A second original Durable construction must rejoin the same origin's
    // native store and seed metadata. No cache/observer terminal is substituted.
    let (reopened, mut reopened_health) = construct(engine, &mut rows, port.as_ref(), &document)?;
    destroy(&mut rows, port.as_ref(), &reopened, &mut reopened_health)?;
    rows.seal();
    let coordinator = rows
        .begin_native_shutdown()
        .map_err(|_| "retained_native_shutdown_admission")?;
    let _native_zero = drive_agent_native_shutdown_until(
        coordinator,
        port.as_ref(),
        ContextResourceAuditId::new(1).ok_or("retained_audit_identity")?,
        &mut NativeEvents(answer),
        Instant::now() + OPERATION_BUDGET,
    )
    .map_err(|_| "retained_native_zero")?;
    fixture
        .shutdown()
        .map_err(|_| "retained_fixture_shutdown")?;
    let _ = writeln!(std::io::stdout().lock(),
        "windows-retained-lifecycle: durable_constructed=true leased=true observation_verified=true progress_advanced_while_leased=true frame_verified=true frame_rate=unmeasured revoke_callback_returned=true resource_retained=true same_durable_origin_reopened=true original_health_owners_retired=true frame_retired=true native_zero=true; content=redacted");
    Ok(())
}

pub(super) fn run() -> Result<(), super::ProbeFailure> {
    run_inner(None, false, false)
}
pub(super) fn run_first_preview() -> Result<(), super::ProbeFailure> {
    run_inner(None, false, true)
}
pub(super) fn run_cookie_session() -> Result<(), super::ProbeFailure> {
    run_inner(Some(true), false, false)
}
pub(super) fn run_cookie_session_without_observation() -> Result<(), super::ProbeFailure> {
    run_inner(Some(false), false, false)
}
pub(super) fn run_wikipedia_read() -> Result<(), super::ProbeFailure> {
    run_inner(None, true, false)
}
fn run_inner(
    cookie_session: Option<bool>,
    wikipedia_read: bool,
    first_preview: bool,
) -> Result<(), super::ProbeFailure> {
    let data = tempfile::Builder::new()
        .prefix("zephium-windows-retained-")
        .tempdir()
        .map_err(|_| super::ProbeFailure::Runtime)?;
    let store = Arc::new(
        zephium_store::SqliteStore::open(data.path()).map_err(|_| super::ProbeFailure::Runtime)?,
    );
    let profile = ProfileId::generate();
    let space = SpaceId::generate();
    store.save_session(SessionState {
        profiles: vec![PersistedProfile {
            id: profile,
            name: "Retained native qualification".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: space,
            profile,
            name: "Fixed native fixture".into(),
        }],
        active_space: Some(space),
        ..SessionState::default()
    });
    if !store.flush() {
        return Err(super::ProbeFailure::Runtime);
    }
    let blocker = super::loopback_support::seeded_blocker(data.path())?;
    let relay = Arc::new(Mutex::new(None::<zephium_app::CallbackHandle>));
    let events = relay.clone();
    let (send, receive) = mpsc::sync_channel(1);
    let owner_store = store.clone();
    zephium_engine::run_windows_work_application_with_input_probe(
        profile,
        zephium_engine::MacosWorkProbeInput::LifecycleOnly,
        TOTAL_BUDGET,
        move |event| {
            if let Ok(relay) = events.lock() {
                if let Some(handle) = relay.as_ref() {
                    handle.dispatch(zephium_app::Command::Engine(event));
                }
            }
        },
        move |engine| {
            let shell = zephium_app::spawn_suspended(
                engine.clone(),
                owner_store,
                blocker,
                Box::new(|_| {}),
                Arc::new(super::loopback_support::NoChrome),
                Box::new(|_| {}),
            )
            .map_err(|_| "retained_shell")?;
            *relay.lock().map_err(|_| "retained_event_relay")? = Some(shell.callback_handle());
            if !shell.admit_startup() || !shell.dispatch(zephium_app::Command::Bootstrap) {
                return Err("retained_startup");
            }
            let worker_shell = shell.clone();
            let failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker_failed = failed.clone();
            let worker = std::thread::Builder::new()
                .name("windows-retained-qualification".into())
                .spawn(move || {
                    let result = workflow(engine.as_ref(), &worker_shell, profile, cookie_session, wikipedia_read, first_preview);
                    if let Err(reason)=result {
                        worker_failed.store(true, std::sync::atomic::Ordering::Release);
                        let _=writeln!(std::io::stderr().lock(),"windows-retained-lifecycle: original_worker_failure={reason}; content=redacted");
                    }
                    let _ = send.send(result);
                })
                .map_err(|_| "retained_worker")?;
            let mut worker = Some(worker);
            let mut shutdown = None;
            Ok(Box::new(move |native_failed| {
                let finished = worker.as_ref().is_some_and(|join| join.is_finished());
                if finished && worker.take().is_none_or(|join| join.join().is_err()) {
                    return Some(Err("retained_worker_panic"));
                }
                if (native_failed || finished) && shutdown.is_none() {
                    let request =
                        shell.shutdown_with_deadline(Instant::now() + Duration::from_secs(10));
                    shutdown = std::thread::Builder::new()
                        .name("windows-retained-shutdown".into())
                        .spawn(move || request.recv_until_deadline())
                        .ok();
                    if shutdown.is_none() {
                        return Some(Err("retained_shutdown_thread"));
                    }
                }
                if shutdown.as_ref().is_some_and(|join| join.is_finished()) {
                    let clean = shutdown.take().is_some_and(|join| {
                        matches!(join.join(), Ok(Ok(zephium_app::ShutdownOutcome::Clean)))
                    });
                    return Some(if clean && !native_failed && !failed.load(std::sync::atomic::Ordering::Acquire) {
                        Ok(())
                    } else {
                        Err("retained_shell_shutdown")
                    });
                }
                None
            }))
        },
    )
    .map_err(|reason| {
        let _ = writeln!(
            std::io::stderr().lock(),
            "windows-retained-lifecycle: host_failure={reason}; content=redacted"
        );
        super::ProbeFailure::Runtime
    })?;
    receive
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| super::ProbeFailure::Runtime)?
        .map_err(|reason| {
            let _ = writeln!(
                std::io::stderr().lock(),
                "windows-retained-lifecycle: failure={reason}; content=redacted"
            );
            super::ProbeFailure::Verification
        })?;
    if store.shutdown_until(Instant::now() + Duration::from_secs(5))
        != zephium_core::ports::store::StoreShutdownOutcome::Clean
    {
        return Err(super::ProbeFailure::Runtime);
    }
    let _ = writeln!(
        std::io::stdout().lock(),
        "windows-retained-lifecycle: shell_store_shutdown_clean=true; content=redacted"
    );
    Ok(())
}

/// Fixed public reads isolate native installation/observation on each origin.
/// Each page gets its own original resource; this does not qualify an in-place
/// cross-origin navigation or manufacture any navigation/action authority.
fn wikipedia_workflow(
    engine: &zephium_engine::WebviewEngine,
    profile: ProfileId,
) -> Result<(), &'static str> {
    let work = WorkId::generate();
    let (events, answer) = mpsc::sync_channel(2);
    let mut factory = engine
        .take_agent_browser_lifetime_factory()
        .ok_or("wikipedia_factory")?;
    let port = factory
        .begin_work_page(work, 1, move |event| {
            let _ = events.try_send(event);
        })
        .map_err(|_| "wikipedia_port")?;
    let mut rows = WorkBrowserResources::new(work, profile);
    for (stage, url, origin) in [
        (
            "portal",
            "https://www.wikipedia.org/",
            "https://www.wikipedia.org",
        ),
        (
            "english_article",
            "https://en.wikipedia.org/wiki/Wikipedia",
            "https://en.wikipedia.org",
        ),
    ] {
        let document = ContextNavigationTarget::parse(url).map_err(|_| "wikipedia_target")?;
        let origin = SemanticOrigin::parse(origin).map_err(|_| "wikipedia_origin")?;
        let _ = writeln!(
            std::io::stdout().lock(),
            "windows-wikipedia-read: stage={stage} phase=construct model_calls=0; content=redacted"
        );
        let (resource, mut health) = construct(engine, &mut rows, port.as_ref(), &document)?;
        let deadline = zephium_engine::work_browser_monotonic_deadline(
            Instant::now() + Duration::from_secs(65),
        )
        .ok_or("wikipedia_deadline")?;
        let request = rows
            .acquire(&resource, ContextRunId::generate(), clock()?, deadline)
            .map_err(|_| "wikipedia_acquire")?;
        let lease = match lifecycle(&mut rows, port.as_ref(), request)? {
            WorkBrowserResourceEvent::Acquired(lease) if lease.resource() == &resource => lease,
            _ => return Err("wikipedia_acquire_terminal"),
        };
        let first = observe_snapshot(&mut rows, port.as_ref(), &lease)?;
        let second = observe_snapshot(&mut rows, port.as_ref(), &lease)?;
        let public_heading = |snapshot: &SemanticSnapshot| {
            snapshot.nodes().iter().any(|node| {
                node.role() == SemanticRole::Heading
                    && [node.name(), node.text()]
                        .into_iter()
                        .flatten()
                        .any(|value| value.as_str().contains("Wikipedia"))
            })
        };
        if first.frame().origin() != &origin
            || second.frame() != first.frame()
            || second.generation().get() <= first.generation().get()
            || !public_heading(&first)
            || !public_heading(&second)
        {
            return Err("wikipedia_public_read_verification");
        }
        let _ = writeln!(std::io::stdout().lock(), "windows-wikipedia-read: stage={stage} phase=read origin_verified=true heading_verified=true fresh_snapshot=true completeness={:?} nodes={} model_calls=0; content=redacted", second.completeness(), second.nodes().len());
        let (request, mut ticket) = rows
            .revoke_with_delivery(&lease)
            .map_err(|_| "wikipedia_revoke")?;
        ticket
            .register_waker(wake())
            .map_err(|_| "wikipedia_delivery")?;
        let ended = match lifecycle(&mut rows, port.as_ref(), request)? {
            WorkBrowserResourceEvent::LeaseEnded(ended) if ended.lease() == &lease => ended,
            _ => return Err("wikipedia_revoke_terminal"),
        };
        let until = Instant::now() + OPERATION_BUDGET;
        let receipt = loop {
            if let Some(receipt) = ticket.try_take().map_err(|_| "wikipedia_receipt")? {
                break receipt;
            }
            if Instant::now() >= until {
                return Err("wikipedia_receipt_deadline");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let _proof = ended
            .join_delivery(receipt)
            .map_err(|_| "wikipedia_delivery_join")?;
        destroy(&mut rows, port.as_ref(), &resource, &mut health)?;
    }
    rows.seal();
    let coordinator = rows
        .begin_native_shutdown()
        .map_err(|_| "wikipedia_shutdown")?;
    let _zero = drive_agent_native_shutdown_until(
        coordinator,
        port.as_ref(),
        ContextResourceAuditId::new(1).ok_or("wikipedia_audit")?,
        &mut NativeEvents(answer),
        Instant::now() + OPERATION_BUDGET,
    )
    .map_err(|_| "wikipedia_native_zero")?;
    let _ = writeln!(std::io::stdout().lock(), "windows-wikipedia-read: pages=2 snapshots=4 resources_destroyed=true native_zero=true same_resource_navigation=unqualified model_calls=0; content=redacted");
    Ok(())
}

/// The full Engine/Shell host admits and retires both pages; HTTP ingress and
/// later server-observed authentication use the same exact native origin.
fn cookie_workflow(
    engine: &zephium_engine::WebviewEngine,
    profile: ProfileId,
    observation: bool,
) -> Result<(), &'static str> {
    let fixture = CookieFixture::start()?;
    let work = WorkId::generate();
    let (events, answer) = mpsc::sync_channel(2);
    let mut factory = engine
        .take_agent_browser_lifetime_factory()
        .ok_or("cookie_host_factory")?;
    let port = factory
        .begin_work_page(work, 1, move |e| {
            let _ = events.try_send(e);
        })
        .map_err(|_| "cookie_host_port")?;
    let mut rows = WorkBrowserResources::new(work, profile);
    for route in ["seed", "read"] {
        let document =
            ContextNavigationTarget::parse(&format!("http://127.0.0.1:{}/{route}", fixture.port))
                .map_err(|_| "cookie_host_target")?;
        let (resource, mut health) = construct(engine, &mut rows, port.as_ref(), &document)?;
        let deadline =
            zephium_engine::work_browser_monotonic_deadline(Instant::now() + OPERATION_BUDGET)
                .ok_or("cookie_host_deadline")?;
        let request = rows
            .acquire(&resource, ContextRunId::generate(), clock()?, deadline)
            .map_err(|_| "cookie_host_acquire")?;
        let lease = match lifecycle(&mut rows, port.as_ref(), request)? {
            WorkBrowserResourceEvent::Acquired(lease) if lease.resource() == &resource => lease,
            _ => return Err("cookie_host_acquire_terminal"),
        };
        let observed = observation
            .then(|| observe(&mut rows, port.as_ref(), &lease))
            .transpose()?;
        let _ = writeln!(
            std::io::stdout().lock(),
            "windows-cookie-host: stage={route} observation_authenticated={observed:?} content=synthetic"
        );
        let (request, mut ticket) = rows
            .revoke_with_delivery(&lease)
            .map_err(|_| "cookie_host_revoke")?;
        ticket
            .register_waker(wake())
            .map_err(|_| "cookie_host_delivery")?;
        let ended = match lifecycle(&mut rows, port.as_ref(), request)? {
            WorkBrowserResourceEvent::LeaseEnded(ended) if ended.lease() == &lease => ended,
            _ => return Err("cookie_host_revoke_terminal"),
        };
        let until = Instant::now() + OPERATION_BUDGET;
        let receipt = loop {
            if let Some(receipt) = ticket.try_take().map_err(|_| "cookie_host_receipt")? {
                break receipt;
            }
            if Instant::now() >= until {
                return Err("cookie_host_receipt_deadline");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let _proof = ended
            .join_delivery(receipt)
            .map_err(|_| "cookie_host_delivery_join")?;
        destroy(&mut rows, port.as_ref(), &resource, &mut health)?;
        if observed.is_some_and(|observed| observed != 1) {
            return Err("cookie_host_authentication_lost");
        }
    }
    rows.seal();
    let coordinator = rows
        .begin_native_shutdown()
        .map_err(|_| "cookie_host_shutdown")?;
    let _zero = drive_agent_native_shutdown_until(
        coordinator,
        port.as_ref(),
        ContextResourceAuditId::new(1).ok_or("cookie_host_audit")?,
        &mut NativeEvents(answer),
        Instant::now() + OPERATION_BUDGET,
    )
    .map_err(|_| "cookie_host_native_zero")?;
    let signed_reads = fixture.finish()?;
    if signed_reads != 1 {
        return Err("cookie_host_server_authentication");
    }
    let _=writeln!(std::io::stdout().lock(),"windows-cookie-host: native_zero=true signed_server_read=true model_calls=0; content=synthetic");
    Ok(())
}

struct CookieFixture {
    port: u16,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<Result<u32, &'static str>>>,
}
impl CookieFixture {
    fn start() -> Result<Self, &'static str> {
        use std::{
            io::Read as _,
            net::TcpListener,
            sync::atomic::{AtomicBool, Ordering},
        };
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| "cookie_host_listener")?;
        let port = listener
            .local_addr()
            .map_err(|_| "cookie_host_address")?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|_| "cookie_host_nonblocking")?;
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::Builder::new()
            .name("windows-cookie-host-http".into())
            .spawn(move || {
                let mut reads = 0;
                let mut requests = 0;
                let until = Instant::now() + Duration::from_secs(90);
                while !done.load(Ordering::Acquire) && Instant::now() < until {
                    let (mut stream, _) = match listener.accept() {
                        Ok(stream) => stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(_) => return Err("cookie_host_accept"),
                    };
                    requests += 1;
                    if requests > 32 {
                        return Err("cookie_host_request_limit");
                    }
                    stream.set_nonblocking(false).map_err(|_| "cookie_host_stream_mode")?;
                    stream.set_read_timeout(Some(Duration::from_millis(500)))
                        .map_err(|_| "cookie_host_read_timeout")?;
                    stream.set_write_timeout(Some(Duration::from_millis(500)))
                        .map_err(|_| "cookie_host_write_timeout")?;
                    let mut bytes = [0; 4096];
                    let mut len = 0;
                    while len < bytes.len() && !bytes[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut bytes[len..]) {
                            Ok(0) => break,
                            Ok(count) => len += count,
                            Err(error) if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => break,
                            Err(_) => return Err("cookie_host_request_read"),
                        }
                    }
                    if len == 0 {
                        continue;
                    }
                    let request = std::str::from_utf8(&bytes[..len])
                        .map_err(|_| "cookie_host_request_encoding")?;
                    let seed = request.starts_with("GET /seed HTTP/");
                    let read = request.starts_with("GET /read HTTP/");
                    let authenticated = request.lines().any(|line| {
                        line.split_once(':').is_some_and(|(key, value)| {
                            key.eq_ignore_ascii_case("Cookie")
                                && value.split(';').any(|cookie| cookie.trim() == "cookie-host=retained")
                        })
                    });
                    if read && authenticated {
                        reads += 1;
                    }
                    let yes = seed || (read && authenticated);
                    let body = format!(
                        "<!doctype html><html><body><h1>Retained native progress {}</h1></body></html>",
                        u8::from(yes),
                    );
                    let cookie = if seed {
                        "Set-Cookie: cookie-host=retained; Path=/; Max-Age=3600; HttpOnly\r\n"
                    } else {
                        ""
                    };
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n{cookie}\r\n{body}", body.len())
                        .map_err(|_| "cookie_host_response")?;
                }
                Ok(reads)
            })
            .map_err(|_| "cookie_host_thread")?;
        Ok(Self {
            port,
            stop,
            thread: Some(thread),
        })
    }
    fn finish(mut self) -> Result<u32, &'static str> {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        self.thread
            .take()
            .ok_or("cookie_host_thread_missing")?
            .join()
            .map_err(|_| "cookie_host_thread_panic")?
    }
}
impl Drop for CookieFixture {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
