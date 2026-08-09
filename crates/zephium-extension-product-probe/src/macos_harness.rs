//! Bounded AppKit/main-loop harness for the real Zephium engine.

use std::ffi::c_void;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::MainThreadOnly as _;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSDate, NSPoint, NSProcessInfo, NSRect, NSRunLoop, NSSize,
};
use raw_window_handle::{AppKitWindowHandle, RawWindowHandle};
use zephium_core::blocker::ContentPolicyGeneration;
use zephium_core::extensions::ExtensionBrowserRequest;
use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::{
    ContentRuleSettlement, Engine, EngineEvent, UserContent, UserContentGeneration,
};
use zephium_core::runtime_security::RuntimeSecurityAdvisories;
use zephium_engine::{InitialUserContent, WebviewEngine};
use zephium_extension_runtime_api::ExtensionRuntimeHostFactory;

const MINIMUM_MACOS_MAJOR: isize = 15;
const MINIMUM_MACOS_MINOR: isize = 4;
const MAIN_TASK_CAPACITY: usize = 512;
const EVENT_CAPACITY: usize = 512;
const MAX_TASKS_PER_PUMP: usize = 64;
const RUN_LOOP_SLICE: Duration = Duration::from_millis(5);
const HTML_OBSERVATION_INTERVAL: Duration = Duration::from_millis(100);
const EXTENSION_MARKER: &str = "data-zephium-extension-product-probe";
const EXTENSION_MARKER_VALUE: &str = "ready:1";

type MainTask = Box<dyn FnOnce() + Send + 'static>;

pub(crate) fn operating_system_version() -> String {
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    format!(
        "{}.{}.{}",
        version.majorVersion, version.minorVersion, version.patchVersion
    )
}

fn supported_runtime() -> bool {
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    version.majorVersion > MINIMUM_MACOS_MAJOR
        || (version.majorVersion == MINIMUM_MACOS_MAJOR
            && version.minorVersion >= MINIMUM_MACOS_MINOR)
}

pub(crate) struct MacosEngineHarness {
    engine: Arc<WebviewEngine>,
    tasks: mpsc::Receiver<MainTask>,
    events: mpsc::Receiver<EngineEvent>,
    fatal_failures: mpsc::Receiver<&'static str>,
    ingress_failed: Arc<AtomicBool>,
    run_loop: Retained<NSRunLoop>,
    window: Option<Retained<NSWindow>>,
    _content_view: Retained<NSView>,
}

impl MacosEngineHarness {
    pub(crate) fn install(data_root: PathBuf) -> Result<Option<Self>, String> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "product probe must run on the process main thread".to_owned())?;
        if !supported_runtime() {
            return Ok(None);
        }

        fs::create_dir_all(&data_root)
            .map_err(|error| format!("cannot create engine data root: {error}"))?;
        fs::set_permissions(&data_root, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("cannot secure engine data root: {error}"))?;

        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let window = new_window(mtm);
        let content_view = window
            .contentView()
            .ok_or_else(|| "probe window has no content view".to_owned())?;
        let pointer = NonNull::from(&*content_view).cast::<c_void>();
        let parent = RawWindowHandle::AppKit(AppKitWindowHandle::new(pointer));

        let (task_tx, tasks) = mpsc::sync_channel::<MainTask>(MAIN_TASK_CAPACITY);
        let dispatch = Arc::new(move |task: MainTask| task_tx.try_send(task).is_ok());

        let ingress_failed = Arc::new(AtomicBool::new(false));
        let event_ingress_failed = Arc::clone(&ingress_failed);
        let (event_tx, events) = mpsc::sync_channel(EVENT_CAPACITY);
        let fatal_ingress_failed = Arc::clone(&ingress_failed);
        let (fatal_tx, fatal_failures) = mpsc::sync_channel(8);

        let initial_generation = UserContentGeneration::new(1)
            .ok_or_else(|| "initial user-content generation construction failed".to_owned())?;
        let engine = zephium_engine::install(
            parent,
            dispatch,
            data_root,
            RuntimeSecurityAdvisories::new(),
            InitialUserContent::new(initial_generation, UserContent::default()),
            move |event| {
                if event_tx.try_send(event).is_err() {
                    event_ingress_failed.store(true, Ordering::Release);
                }
            },
            move |failure| {
                fatal_ingress_failed.store(true, Ordering::Release);
                let _ = fatal_tx.try_send(failure);
            },
        )
        .map_err(|error| format!("cannot install real webview engine: {error}"))?;
        window.orderFrontRegardless();

        Ok(Some(Self {
            engine: Arc::new(engine),
            tasks,
            events,
            fatal_failures,
            ingress_failed,
            run_loop: NSRunLoop::mainRunLoop(),
            window: Some(window),
            _content_view: content_view,
        }))
    }

    pub(crate) fn engine(&self) -> &WebviewEngine {
        &self.engine
    }

    pub(crate) fn take_extension_runtime_host_factory(
        &self,
    ) -> Option<ExtensionRuntimeHostFactory> {
        self.engine.take_extension_runtime_host_factory()
    }

    pub(crate) fn pump_until(
        &mut self,
        phase: &'static str,
        deadline: Instant,
        mut settled: impl FnMut(&mut Self) -> Result<bool, String>,
    ) -> Result<(), String> {
        loop {
            self.check_ingress(phase)?;
            if settled(self)? {
                self.check_ingress(phase)?;
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!("{phase} exceeded its bounded deadline"));
            }
            self.pump_once();
        }
    }

    pub(crate) fn wait_for_content_policy(
        &mut self,
        profile: ProfileId,
        requested: ContentPolicyGeneration,
        deadline: Instant,
    ) -> Result<(), String> {
        self.pump_until("content-policy settlement", deadline, |harness| loop {
            match harness.events.try_recv() {
                Ok(EngineEvent::ContentRulesSettled {
                    profile: event_profile,
                    requested: event_requested,
                    settlement,
                }) if event_profile == profile && event_requested == requested => {
                    return match settlement {
                        ContentRuleSettlement::Applied { generation }
                            if generation == requested =>
                        {
                            Ok(true)
                        }
                        other => Err(format!("allow-all content policy did not apply: {other:?}")),
                    };
                }
                Ok(EngineEvent::ViewCreationFailed { id }) => {
                    return Err(format!(
                        "unexpected view-creation failure before policy settlement: {id}"
                    ));
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => return Ok(false),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("engine event ingress disconnected".to_owned())
                }
            }
        })
    }

    pub(crate) fn wait_for_view_commit(
        &mut self,
        item: ItemId,
        expected_url: &str,
        deadline: Instant,
    ) -> Result<(), String> {
        self.pump_until("profile view construction and navigation", deadline, |harness| {
            loop {
                match harness.events.try_recv() {
                    Ok(EngineEvent::UrlChanged { id, url }) if id == item => {
                        if url == expected_url {
                            return Ok(true);
                        }
                        return Err(format!(
                            "profile view committed unexpected URL {url:?}; expected {expected_url:?}"
                        ));
                    }
                    Ok(EngineEvent::ViewCreationFailed { id }) if id == item => {
                        return Err("profile view creation failed closed".to_owned())
                    }
                    Ok(EngineEvent::Crashed { id }) if id == item => {
                        return Err("profile view renderer crashed during construction".to_owned())
                    }
                    Ok(_) => {}
                    Err(mpsc::TryRecvError::Empty) => return Ok(false),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err("engine event ingress disconnected".to_owned())
                    }
                }
            }
        })
    }

    pub(crate) fn wait_for_executable_extension(
        &mut self,
        item: ItemId,
        deadline: Instant,
        mut handle_browser_request: impl FnMut(
            &WebviewEngine,
            ExtensionBrowserRequest,
        ) -> Result<(), String>,
    ) -> Result<(), String> {
        let expected = format!(r#"{EXTENSION_MARKER}="{EXTENSION_MARKER_VALUE}""#);
        let failure_prefix = format!(r#"{EXTENSION_MARKER}=""#);
        let mut next_observation = Instant::now();
        self.pump_until("executable MV3 extension observation", deadline, |harness| {
            loop {
                match harness.events.try_recv() {
                    Ok(EngineEvent::HtmlExtracted {
                        id,
                        html,
                        truncated,
                    }) if id == item => {
                        if truncated {
                            return Err(
                                "extension marker observation returned truncated HTML".to_owned()
                            );
                        }
                        if html.contains(&expected) {
                            return Ok(true);
                        }
                        if let Some(marker_start) = html.find(&failure_prefix) {
                            let value_start = marker_start + failure_prefix.len();
                            let value = html[value_start..]
                                .split('"')
                                .next()
                                .unwrap_or_default()
                                .chars()
                                .take(160)
                                .collect::<String>();
                            return Err(format!(
                                "authenticated extension reported marker {value:?}; expected {EXTENSION_MARKER_VALUE:?}"
                            ));
                        }
                    }
                    Ok(EngineEvent::ViewCreationFailed { id }) if id == item => {
                        return Err("profile view failed during extension execution".to_owned())
                    }
                    Ok(EngineEvent::Crashed { id }) if id == item => {
                        return Err("profile view crashed during extension execution".to_owned())
                    }
                    Ok(EngineEvent::ExtensionBrowserRequested { request }) => {
                        handle_browser_request(&harness.engine, request)?;
                    }
                    Ok(_) => {}
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err("engine event ingress disconnected".to_owned())
                    }
                }
            }

            let now = Instant::now();
            if now >= next_observation {
                if harness.engine.extract_html(item)
                    != zephium_core::ports::engine::NativeDispatch::Scheduled
                {
                    return Err("HTML observation was not admitted".to_owned());
                }
                next_observation = now
                    .checked_add(HTML_OBSERVATION_INTERVAL)
                    .ok_or_else(|| "HTML-observation deadline overflowed".to_owned())?;
            }
            Ok(false)
        })
    }

    pub(crate) fn shutdown(&mut self, deadline: Instant) -> Result<(), String> {
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        self.engine.shutdown(Box::new(move |clean| {
            let _ = done_tx.send(clean);
        }));
        let mut clean = None;
        self.pump_until("engine shutdown", deadline, |_| match done_rx.try_recv() {
            Ok(settled) => {
                clean = Some(settled);
                Ok(true)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(false),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("engine-shutdown completion disconnected".to_owned())
            }
        })?;
        if clean != Some(true) {
            return Err("engine shutdown retained a native cleanup obligation".to_owned());
        }
        if let Some(window) = self.window.take() {
            window.close();
        }
        self.drain_run_loop();
        Ok(())
    }

    fn check_ingress(&self, phase: &'static str) -> Result<(), String> {
        if let Ok(failure) = self.fatal_failures.try_recv() {
            return Err(format!(
                "engine failed terminally during {phase}: {failure}"
            ));
        }
        if self.ingress_failed.load(Ordering::Acquire) {
            return Err(format!(
                "bounded engine event or fatal ingress failed during {phase}"
            ));
        }
        Ok(())
    }

    fn pump_once(&self) {
        for _ in 0..MAX_TASKS_PER_PUMP {
            match self.tasks.try_recv() {
                Ok(task) => task(),
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            }
        }
        self.drain_run_loop();
    }

    fn drain_run_loop(&self) {
        objc2::rc::autoreleasepool(|_| {
            self.run_loop
                .runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(
                    RUN_LOOP_SLICE.as_secs_f64(),
                ));
        });
    }
}

fn new_window(mtm: MainThreadMarker) -> Retained<NSWindow> {
    // SAFETY: `mtm` proves AppKit affinity, and the retained window outlives
    // the engine plus every Wry child attached to its content view.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(720.0, 540.0)),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: Rust retains the window until explicit engine teardown and
    // close; AppKit must not release it independently at close time.
    unsafe { window.setReleasedWhenClosed(false) };
    window
}
