//! Native regression for controller-wide WebResourceRequested filter unions.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2_22, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
};
use windows_core::Interface as _;
use wry::{WebContext, WebViewBuilder, WebViewExtWindows as _};

use super::super::{
    attest_environment, browser_process_for_environment, install_browser_process_exit_observer,
    pump_browser_exit_callbacks, wait_for_browser_process_exit,
};
use super::ProbeHostWindow;

const TIMEOUT: Duration = Duration::from_secs(15);
const PAGE: &str = r#"<!doctype html><meta charset="utf-8">
<form id="form" method="post" action="/document"><input name="v" value="fixed"></form>
<script>fetch('/fetch',{method:'POST'}).then(r=>{if(r.ok)document.getElementById('form').submit();});</script>"#;

#[path = "work_backing_control.rs"]
mod work_backing_control;

struct OwnedUdf(Option<tempfile::TempDir>);
impl OwnedUdf {
    fn close(mut self) -> Result<(), &'static str> {
        self.0
            .take()
            .ok_or("directory_owner")?
            .close()
            .map_err(|_| "remove_directory")
    }
}
impl Drop for OwnedUdf {
    fn drop(&mut self) {
        // An early native failure must not delete a UDF without exit proof.
        if let Some(directory) = self.0.take() {
            let _retained = directory.keep();
        }
    }
}

struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    fetches: Arc<AtomicUsize>,
    documents: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<Result<(), &'static str>>>,
}

impl Server {
    fn start() -> Result<Self, &'static str> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|_| "bind")?;
        let port = listener.local_addr().map_err(|_| "address")?.port();
        listener.set_nonblocking(true).map_err(|_| "listener")?;
        let stop = Arc::new(AtomicBool::new(false));
        let fetches = Arc::new(AtomicUsize::new(0));
        let documents = Arc::new(AtomicUsize::new(0));
        let (worker_stop, worker_fetches, worker_documents) =
            (stop.clone(), fetches.clone(), documents.clone());
        let thread = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(45);
            let mut accepted = 0;
            while !worker_stop.load(Ordering::Acquire) && Instant::now() < deadline {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => return Err("accept"),
                };
                accepted += 1;
                if accepted > 32 {
                    return Err("request_bound");
                }
                stream.set_nonblocking(false).map_err(|_| "stream")?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .map_err(|_| "read_timeout")?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .map_err(|_| "write_timeout")?;
                let mut bytes = Vec::new();
                let mut chunk = [0; 1024];
                while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    let count = match stream.read(&mut chunk) {
                        Ok(count) => count,
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            break
                        }
                        Err(_) => return Err("read"),
                    };
                    if count == 0 {
                        break;
                    }
                    if bytes.len() + count > 8192 {
                        return Err("header_bound");
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                }
                // Chromium may preconnect without sending a request. Such a
                // bounded empty connection carries no fixture mutation.
                if !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    continue;
                }
                let first = bytes.split(|byte| *byte == b'\n').next().ok_or("request")?;
                let body = if first.starts_with(b"GET / HTTP/") {
                    PAGE
                } else {
                    if first.starts_with(b"POST /fetch HTTP/") {
                        worker_fetches.fetch_add(1, Ordering::AcqRel);
                    }
                    if first.starts_with(b"POST /document HTTP/") {
                        worker_documents.fetch_add(1, Ordering::AcqRel);
                    }
                    "fixed"
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).map_err(|_| "write")?;
            }
            Ok(())
        });
        Ok(Self {
            port,
            stop,
            fetches,
            documents,
            thread: Some(thread),
        })
    }
    fn finish(&mut self) -> bool {
        self.stop.store(true, Ordering::Release);
        self.thread
            .take()
            .is_some_and(|thread| thread.join().is_ok_and(|result| result.is_ok()))
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.finish();
        }
    }
}

#[test]
fn native_filter_union_preserves_fetch_post_and_refuses_unapproved_document_post() {
    assert_eq!(run(), Ok(()));
}

fn run() -> Result<(), &'static str> {
    let mut server = Server::start()?;
    let directory = tempfile::Builder::new()
        .prefix("zephium-native-network-")
        .tempdir()
        .map_err(|_| "directory")?;
    let path = directory.path().to_owned();
    let directory = OwnedUdf(Some(directory));
    let host = ProbeHostWindow::new().map_err(|_| "host")?;
    let mut context = WebContext::new(Some(path.clone()));
    let finished = std::rc::Rc::new(std::cell::Cell::new(false));
    let callback_finished = finished.clone();
    let target = format!("http://127.0.0.1:{}/document", server.port);
    let mut view = WebViewBuilder::new_with_web_context(&mut context)
        .with_url("about:blank")
        .with_incognito(false)
        .with_visible(false)
        .with_navigation_event_handler(move |event| {
            if event.url == target
                && matches!(
                    event.phase,
                    wry::NavigationEventPhase::Finished
                        | wry::NavigationEventPhase::Failed
                        | wry::NavigationEventPhase::Cancelled
                )
            {
                callback_finished.set(true);
            }
        })
        .build_as_child(&host)
        .map_err(|_| "view")?;
    let environment = view.environment();
    let process = browser_process_for_environment(&environment).map_err(|_| "process")?;
    let observer = install_browser_process_exit_observer(&environment, process.id(), |_| {})
        .map_err(|_| "observer")?;
    let core22 = view
        .webview()
        .cast::<ICoreWebView2_22>()
        .map_err(|_| "core")?;
    let mut policy = None;
    let outcome = (|| {
        attest_environment(&environment, &path).map_err(|_| "attestation")?;
        // SAFETY: exact owned controller; fixed filter simulates another owner's
        // native fetch/script/image filters in the same controller-wide union.
        unsafe {
            core22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                windows_core::w!("*://*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )
        }
        .map_err(|_| "filter")?;
        policy = Some(
            super::super::work_network::WorkNetworkPolicy::install(
                &view,
                crate::platform::work_document_navigation::WorkDocumentNavigation::default(),
            )
            .map_err(|_| "policy")?,
        );
        view.load_url(&format!("http://127.0.0.1:{}/", server.port))
            .map_err(|_| "load")?;
        let deadline = Instant::now() + TIMEOUT;
        while !finished.get() && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("pump");
            }
        }
        if !finished.get()
            || server.fetches.load(Ordering::Acquire) != 1
            || server.documents.load(Ordering::Acquire) != 0
        {
            return Err("request_boundary");
        }
        Ok(())
    })();
    let policy_retired = policy.as_mut().is_none_or(|policy| policy.retire());
    drop(policy);
    // SAFETY: remove the exact fixed test filter on the same owned controller.
    let filter_retired = unsafe {
        core22.RemoveWebResourceRequestedFilterWithRequestSourceKinds(
            windows_core::w!("*://*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
        )
    }
    .is_ok();
    drop(core22);
    let closed = view.close().is_ok();
    drop(view);
    drop(context);
    drop(environment);
    let exited =
        wait_for_browser_process_exit(&process, &observer.proof(), Instant::now() + TIMEOUT);
    drop(observer);
    drop(process);
    drop(host);
    let server_closed = server.finish();
    if let Err(failure) = outcome {
        eprintln!(
            "native-network: failure={failure}; fetch_count={}; document_count={}; document_terminal={}; policy_retired={policy_retired}; filter_retired={filter_retired}; controller_closed={closed}; process_exited={exited}; server_closed={server_closed}",
            server.fetches.load(Ordering::Acquire), server.documents.load(Ordering::Acquire), finished.get()
        );
        if closed && exited && directory.close().is_err() {
            eprintln!("native-network: directory_cleanup_failed=true");
        }
        return Err(failure);
    }
    if !closed || !exited {
        return Err("native_cleanup");
    }
    directory.close()?;
    if !policy_retired {
        return Err("policy_retirement");
    }
    if !filter_retired {
        return Err("filter_retirement");
    }
    if !server_closed {
        return Err("server_cleanup");
    }
    outcome
}

#[test]
fn frame_retirement_revokes_input_then_preserves_successor_rendering() {
    assert_eq!(run_frame_retirement(false, (1280, 800)), Ok(()));
}

#[test]
fn ordinary_navigation_guard_hides_a_retained_presenter() {
    assert_eq!(run_frame_retirement(true, (1280, 800)), Ok(()));
}

#[test]
fn dpi_125_percent_native_frame_stays_small_and_retires_exactly() {
    assert_eq!(run_frame_retirement(false, (1600, 1000)), Ok(()));
}

#[test]
fn dpi_150_percent_native_frame_stays_small_and_retires_exactly() {
    assert_eq!(run_frame_retirement(false, (1920, 1200)), Ok(()));
}

fn run_frame_retirement(
    ordinary_navigation_guard: bool,
    physical: (u32, u32),
) -> Result<(), &'static str> {
    use super::super::work_presentation::{PresentationState, WorkObservationPresentation};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, ShowWindow, SW_SHOWNOACTIVATE};

    fn presentation(view: &wry::WebView) -> Result<WorkObservationPresentation, &'static str> {
        let deadline = Instant::now() + TIMEOUT;
        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
        let prepared = WorkObservationPresentation::prepare(view, deadline, |_| {});
        #[cfg(not(feature = "native-agentic-work-lifetime-diagnostic"))]
        let prepared = WorkObservationPresentation::prepare(view, deadline);
        prepared.map_err(|_| "frame_presentation")
    }

    fn frame(
        view: &wry::WebView,
        pending: Arc<AtomicBool>,
    ) -> Result<Rc<RefCell<Option<bool>>>, &'static str> {
        let received = Rc::new(RefCell::new(None));
        let callback = received.clone();
        let started = Instant::now();
        if !super::super::semantic_screenshot::capture_work_frame(view, move |image| {
            let accepted = image.as_ref().is_some_and(|(width, height, png)| {
                (*width, *height) == (640, 400) && png.len() <= 1024 * 1024
            });
            let _ =
                writeln!(std::io::stdout().lock(),
                "native-work-frame: total_ms={} width={} height={} encoded_bytes={} accepted={}",
                started.elapsed().as_millis(),
                image.as_ref().map_or(0, |frame| frame.0),
                image.as_ref().map_or(0, |frame| frame.1),
                image.as_ref().map_or(0, |frame| frame.2.len()), accepted);
            *callback.borrow_mut() = Some(accepted);
            pending.store(false, Ordering::Release);
        }) {
            return Err("frame_dispatch");
        }
        Ok(received)
    }

    fn wait_frame(received: &Rc<RefCell<Option<bool>>>) -> Result<(), &'static str> {
        let deadline = Instant::now() + TIMEOUT;
        while received.borrow().is_none() && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("frame_pump");
            }
        }
        if *received.borrow() != Some(true) {
            return Err("frame_completion");
        }
        Ok(())
    }

    let directory = tempfile::Builder::new()
        .prefix("zephium-native-frame-")
        .tempdir()
        .map_err(|_| "directory")?;
    let path = directory.path().to_owned();
    let directory = OwnedUdf(Some(directory));
    let host = ProbeHostWindow::new().map_err(|_| "host")?;
    // SAFETY: this provider-free fixture uniquely owns its top-level HWND.
    unsafe {
        let _ = ShowWindow(host.hwnd, SW_SHOWNOACTIVATE);
    }
    let mut context = WebContext::new(Some(path.clone()));
    let loaded = Rc::new(Cell::new(false));
    let callback_loaded = loaded.clone();
    let mut builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_html("<!doctype html><h1>Fixed native frame</h1>")
        .with_incognito(false)
        .with_visible(false)
        .with_bounds(wry::Rect {
            position: wry::dpi::PhysicalPosition::new(0, 0).into(),
            size: wry::dpi::PhysicalSize::new(physical.0, physical.1).into(),
        })
        .with_navigation_event_handler(move |event| {
            if event.phase == wry::NavigationEventPhase::Finished {
                callback_loaded.set(true);
            }
        });
    if ordinary_navigation_guard {
        builder = builder.with_navigation_presentation_guard(|| {});
    }
    let mut view = builder.build_as_child(&host).map_err(|_| "view")?;
    let environment = view.environment();
    let process = browser_process_for_environment(&environment).map_err(|_| "process")?;
    let observer = install_browser_process_exit_observer(&environment, process.id(), |_| {})
        .map_err(|_| "observer")?;
    let outcome = (|| {
        attest_environment(&environment, &path).map_err(|_| "attestation")?;
        let deadline = Instant::now() + TIMEOUT;
        while !loaded.get() && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("frame_load");
            }
        }
        if !loaded.get() {
            return Err("frame_load");
        }

        let mut old = presentation(&view)?;
        if old.present() != PresentationState::Ready {
            return Err("frame_present");
        }
        let fence = old.human_fence();
        let pending = Arc::new(AtomicBool::new(true));
        old.retain_frame_capture(pending.clone());
        let first = frame(&view, pending.clone())?;
        // Retirement revokes input immediately, but the exact pending native
        // preview continues with its rendering owner and original callback.
        if old.retire() != PresentationState::Retiring || fence()
            || !old.visible_for_audit()
            // SAFETY: exact controller-owned child on the fixture STA.
            || !unsafe { IsWindowVisible(view.hwnd()).as_bool() }
        {
            return Err("frame_retirement_fence");
        }
        wait_frame(&first)?;
        if pending.load(Ordering::Acquire) || old.retire() != PresentationState::Retired {
            return Err("frame_retirement_drain");
        }

        let mut successor = presentation(&view)?;
        if successor.present() != PresentationState::Ready {
            return Err("successor_present");
        }
        let successor_pending = Arc::new(AtomicBool::new(true));
        successor.retain_frame_capture(successor_pending.clone());
        let second = frame(&view, successor_pending)?;
        drop(old);
        // An already-retired presenter's destructor cannot hide a new owner
        // of the same exact child, or strand that owner's preview callback.
        // SAFETY: exact controller-owned child on the fixture STA.
        if !unsafe { IsWindowVisible(view.hwnd()).as_bool() } || !successor.human_current() {
            return Err("successor_hidden");
        }
        wait_frame(&second)?;
        loaded.set(false);
        view.load_html("<!doctype html><h1>Fixed second native frame document</h1>")
            .map_err(|_| "frame_navigation")?;
        let deadline = Instant::now() + TIMEOUT;
        while !loaded.get() && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("frame_navigation_pump");
            }
        }
        if !loaded.get() {
            return Err("frame_navigation_load");
        }
        // Reproduce Wry's ContentLoading hide without deliberately stranding
        // a native preview. Work omits this ordinary reveal hook, so the same
        // exact rendering owner can complete the next document's preview.
        // SAFETY: the retained controller and its exact child are fixture-owned.
        let visible = unsafe { IsWindowVisible(view.hwnd()).as_bool() };
        let mut controller_visible = windows::core::BOOL::default();
        // SAFETY: this retained controller is queried on its owning STA.
        unsafe { view.controller().IsVisible(&mut controller_visible) }
            .map_err(|_| "frame_controller_visibility")?;
        if visible == ordinary_navigation_guard
            || controller_visible.as_bool() == ordinary_navigation_guard
            || successor.poll() != PresentationState::Ready
        {
            return Err("frame_navigation_visibility");
        }
        if !ordinary_navigation_guard {
            let pending = Arc::new(AtomicBool::new(true));
            successor.retain_frame_capture(pending.clone());
            wait_frame(&frame(&view, pending)?)?;
        }
        if successor.retire() != PresentationState::Retired {
            return Err("successor_retirement");
        }
        Ok(())
    })();
    let closed = view.close().is_ok();
    drop(view);
    drop(context);
    drop(environment);
    let exited =
        wait_for_browser_process_exit(&process, &observer.proof(), Instant::now() + TIMEOUT);
    drop(observer);
    drop(process);
    drop(host);
    if !closed || !exited {
        return Err("frame_native_cleanup");
    }
    directory.close()?;
    outcome
}
