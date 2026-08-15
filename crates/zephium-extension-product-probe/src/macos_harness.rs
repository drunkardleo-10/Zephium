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
use zephium_core::extensions::{
    ExtensionActionRejection, ExtensionActionRequest, ExtensionActionSettlement,
    ExtensionActionSnapshotSettlement, ExtensionActionState, ExtensionBrowserRequest,
    ExtensionCompatibilityBrokerRequest,
};
use zephium_core::geometry::Size;
use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::{
    ContentRuleSettlement, Engine, EngineEvent, UserContent, UserContentGeneration,
};
use zephium_core::ports::extensions::ExtensionRuntimeGrantPrompt;
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
const EXTENSION_POPUP_MARKER: &str = "data-zephium-extension-popup-probe";

type MainTask = Box<dyn FnOnce() + Send + 'static>;

pub(crate) trait ExecutableExtensionCoordinator {
    fn poll(&mut self, engine: &WebviewEngine) -> Result<(), String>;

    fn handle_browser_request(
        &mut self,
        engine: &WebviewEngine,
        request: ExtensionBrowserRequest,
    ) -> Result<(), String>;

    fn handle_compatibility_broker_request(
        &mut self,
        engine: &WebviewEngine,
        request: ExtensionCompatibilityBrokerRequest,
    ) -> Result<(), String>;
}

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

    pub(crate) fn wait_for_extension_armed(
        &mut self,
        item: ItemId,
        deadline: Instant,
    ) -> Result<(), String> {
        let expected = format!(r#"{EXTENSION_MARKER}="armed""#);
        let marker_prefix = format!(r#"{EXTENSION_MARKER}=""#);
        let mut next_observation = Instant::now();
        self.pump_until("extension action listener readiness", deadline, |harness| {
            loop {
                match harness.events.try_recv() {
                    Ok(EngineEvent::HtmlExtracted {
                        id,
                        html,
                        truncated,
                    }) if id == item => {
                        if truncated {
                            return Err(
                                "extension readiness observation returned truncated HTML".into()
                            );
                        }
                        if html.contains(&expected) {
                            return Ok(true);
                        }
                        if let Some(marker_start) = html.find(&marker_prefix) {
                            let value_start = marker_start + marker_prefix.len();
                            let value = html[value_start..]
                                .split('"')
                                .next()
                                .unwrap_or_default()
                                .chars()
                                .take(160)
                                .collect::<String>();
                            return Err(format!(
                                "extension reached unexpected pre-action marker {value:?}"
                            ));
                        }
                    }
                    Ok(EngineEvent::ViewCreationFailed { id }) if id == item => {
                        return Err("profile view failed before extension action readiness".into())
                    }
                    Ok(EngineEvent::Crashed { id }) if id == item => {
                        return Err("profile view crashed before extension action readiness".into())
                    }
                    Ok(EngineEvent::ExtensionBrowserRequested { request }) => {
                        return Err(format!(
                            "extension requested browser mutation before optional grants: {:?}",
                            request.action()
                        ));
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
                    return Err("extension readiness observation was not admitted".into());
                }
                next_observation = now
                    .checked_add(HTML_OBSERVATION_INTERVAL)
                    .ok_or_else(|| "readiness-observation deadline overflowed".to_owned())?;
            }
            Ok(false)
        })
    }

    pub(crate) fn request_extension_action(
        &mut self,
        profile: ProfileId,
        item: ItemId,
        surface_generation: zephium_core::extensions::ExtensionBrowserSurfaceGeneration,
        deadline: Instant,
    ) -> Result<ExtensionActionState, String> {
        if self
            .engine
            .request_extension_actions(profile, item, surface_generation)
            != zephium_core::ports::engine::NativeDispatch::Scheduled
        {
            return Err("extension action snapshot was not scheduled".into());
        }
        let mut action = None;
        self.pump_until("extension action snapshot", deadline, |harness| loop {
            match harness.events.try_recv() {
                Ok(EngineEvent::ExtensionActionsSnapshotSettled {
                    profile: event_profile,
                    tab,
                    surface_generation: event_generation,
                    settlement,
                }) if event_profile == profile
                    && tab == item
                    && event_generation == surface_generation =>
                {
                    let ExtensionActionSnapshotSettlement::Applied(snapshot) = settlement else {
                        return Err("extension action snapshot was rejected".into());
                    };
                    if snapshot.actions().len() != 1 {
                        return Err(format!(
                            "expected one authenticated extension action, observed {}",
                            snapshot.actions().len()
                        ));
                    }
                    action = snapshot.actions().first().cloned();
                    return Ok(true);
                }
                Ok(EngineEvent::ViewCreationFailed { id }) if id == item => {
                    return Err("profile view failed during extension action projection".into())
                }
                Ok(EngineEvent::Crashed { id }) if id == item => {
                    return Err("profile view crashed during extension action projection".into())
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => return Ok(false),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("engine event ingress disconnected".to_owned())
                }
            }
        })?;
        action.ok_or_else(|| "extension action snapshot completed without an action".to_owned())
    }

    pub(crate) fn invoke_action_for_runtime_grant(
        &mut self,
        request: ExtensionActionRequest,
        deadline: Instant,
    ) -> Result<ExtensionRuntimeGrantPrompt, String> {
        if self.engine.invoke_extension_action(request)
            != zephium_core::ports::engine::NativeDispatch::Scheduled
        {
            return Err("extension action invocation was not scheduled".into());
        }
        let mut dispatched = false;
        let mut prompt = None;
        self.pump_until("native optional permission request", deadline, |harness| loop {
            match harness.events.try_recv() {
                Ok(EngineEvent::ExtensionActionSettled {
                    profile,
                    request: event_request,
                    settlement,
                }) if profile == request.runtime().profile()
                    && event_request == request.id() =>
                {
                    if settlement != ExtensionActionSettlement::Dispatched {
                        return Err(format!(
                            "extension action did not dispatch its user gesture: {settlement:?}"
                        ));
                    }
                    dispatched = true;
                }
                Ok(EngineEvent::ExtensionRuntimeGrantRequested {
                    prompt: candidate,
                }) => {
                    if prompt.is_some() {
                        return Err("extension action produced duplicate grant prompts".into());
                    }
                    prompt = Some(*candidate);
                }
                Ok(EngineEvent::ExtensionRuntimeGrantCancelled { runtime, request }) => {
                    return Err(format!(
                        "native optional permission prompt was cancelled: runtime={runtime:?}, request={}",
                        request.get()
                    ));
                }
                Ok(EngineEvent::ExtensionBrowserRequested { request }) => {
                    return Err(format!(
                        "extension requested browser mutation before grant settlement: {:?}",
                        request.action()
                    ));
                }
                Ok(EngineEvent::ViewCreationFailed { .. }) => {
                    return Err("profile view failed during optional permission request".into())
                }
                Ok(EngineEvent::Crashed { .. }) => {
                    return Err("profile view crashed during optional permission request".into())
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => {
                    return Ok(dispatched && prompt.is_some())
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("engine event ingress disconnected".to_owned())
                }
            }
        })?;
        prompt.ok_or_else(|| "extension action completed without a grant prompt".to_owned())
    }

    pub(crate) fn invoke_popup_action(
        &mut self,
        request: ExtensionActionRequest,
        deadline: Instant,
    ) -> Result<Size, String> {
        if self.engine.invoke_extension_action(request)
            != zephium_core::ports::engine::NativeDispatch::Scheduled
        {
            return Err("extension popup invocation was not scheduled".into());
        }
        let mut presented = None;
        self.pump_until("native extension popup presentation", deadline, |harness| {
            loop {
                match harness.events.try_recv() {
                    Ok(EngineEvent::ExtensionActionSettled {
                        profile,
                        request: event_request,
                        settlement,
                    }) if profile == request.runtime().profile()
                        && event_request == request.id() =>
                    {
                        let ExtensionActionSettlement::PopupPresented(size) = settlement else {
                            return Err(format!(
                                "extension popup was not presented: {settlement:?}"
                            ));
                        };
                        presented = Some(size);
                        return Ok(true);
                    }
                    Ok(EngineEvent::ExtensionRuntimeGrantRequested { .. }) => {
                        return Err("extension popup unexpectedly requested runtime grants".into())
                    }
                    Ok(EngineEvent::ExtensionBrowserRequested { request }) => {
                        return Err(format!(
                            "extension popup unexpectedly requested a browser mutation: {:?}",
                            request.action()
                        ));
                    }
                    Ok(EngineEvent::ViewCreationFailed { .. }) => {
                        return Err("profile view failed during extension popup presentation".into())
                    }
                    Ok(EngineEvent::Crashed { .. }) => {
                        return Err(
                            "profile view crashed during extension popup presentation".into()
                        )
                    }
                    Ok(_) => {}
                    Err(mpsc::TryRecvError::Empty) => return Ok(false),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err("engine event ingress disconnected".to_owned())
                    }
                }
            }
        })?;
        presented.ok_or_else(|| "extension popup completed without a size".to_owned())
    }

    pub(crate) fn reject_parallel_popup(
        &mut self,
        request: ExtensionActionRequest,
        deadline: Instant,
    ) -> Result<(), String> {
        if self.engine.invoke_extension_action(request)
            != zephium_core::ports::engine::NativeDispatch::Scheduled
        {
            return Err("parallel extension popup invocation was not scheduled".into());
        }
        self.pump_until("extension popup capacity rejection", deadline, |harness| loop {
            match harness.events.try_recv() {
                Ok(EngineEvent::ExtensionActionSettled {
                    profile,
                    request: event_request,
                    settlement,
                }) if profile == request.runtime().profile()
                    && event_request == request.id() =>
                {
                    return match settlement {
                        ExtensionActionSettlement::Rejected(
                            ExtensionActionRejection::PopupCapacityExceeded,
                        ) => Ok(true),
                        other => Err(format!(
                            "parallel extension popup did not fail at the capacity boundary: {other:?}"
                        )),
                    };
                }
                Ok(EngineEvent::ViewCreationFailed { .. }) => {
                    return Err("profile view failed during popup capacity proof".into())
                }
                Ok(EngineEvent::Crashed { .. }) => {
                    return Err("profile view crashed during popup capacity proof".into())
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => return Ok(false),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("engine event ingress disconnected".to_owned())
                }
            }
        })
    }

    pub(crate) fn wait_for_popup_execution(
        &mut self,
        item: ItemId,
        expected_count: usize,
        deadline: Instant,
    ) -> Result<(), String> {
        let expected = format!(r#"{EXTENSION_POPUP_MARKER}="ready:{expected_count}""#);
        let marker_prefix = format!(r#"{EXTENSION_POPUP_MARKER}=""#);
        let mut next_observation = Instant::now();
        self.pump_until("extension popup script execution", deadline, |harness| {
            loop {
                match harness.events.try_recv() {
                    Ok(EngineEvent::HtmlExtracted {
                        id,
                        html,
                        truncated,
                    }) if id == item => {
                        if truncated {
                            return Err(
                                "extension popup observation returned truncated HTML".into()
                            );
                        }
                        if html.contains(&expected) {
                            return Ok(true);
                        }
                        if let Some(marker_start) = html.find(&marker_prefix) {
                            let value_start = marker_start + marker_prefix.len();
                            let value = html[value_start..].split('"').next().unwrap_or_default();
                            let observed = value
                                .strip_prefix("ready:")
                                .and_then(|count| count.parse::<usize>().ok());
                            if observed.is_none_or(|count| count >= expected_count) {
                                return Err(format!(
                                    "extension popup reported unexpected marker {value:?}"
                                ));
                            }
                        }
                    }
                    Ok(EngineEvent::ViewCreationFailed { id }) if id == item => {
                        return Err("profile view failed during popup execution".into())
                    }
                    Ok(EngineEvent::Crashed { id }) if id == item => {
                        return Err("profile view crashed during popup execution".into())
                    }
                    Ok(EngineEvent::ExtensionBrowserRequested { request }) => {
                        return Err(format!(
                            "popup execution requested an unexpected browser mutation: {:?}",
                            request.action()
                        ));
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
                    return Err("extension popup HTML observation was not admitted".into());
                }
                next_observation = now
                    .checked_add(HTML_OBSERVATION_INTERVAL)
                    .ok_or_else(|| "popup-observation deadline overflowed".to_owned())?;
            }
            Ok(false)
        })
    }

    pub(crate) fn wait_for_executable_extension(
        &mut self,
        item: ItemId,
        expected_marker_value: &str,
        deadline: Instant,
        coordinator: &mut impl ExecutableExtensionCoordinator,
    ) -> Result<(), String> {
        let expected = format!(r#"{EXTENSION_MARKER}="{expected_marker_value}""#);
        let failure_prefix = format!(r#"{EXTENSION_MARKER}=""#);
        let mut next_observation = Instant::now();
        self.pump_until("executable MV3 extension observation", deadline, |harness| {
            coordinator.poll(&harness.engine)?;
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
                            if value != "armed" {
                                return Err(format!(
                                    "authenticated extension reported marker {value:?}; expected {expected_marker_value:?}"
                                ));
                            }
                        }
                    }
                    Ok(EngineEvent::ViewCreationFailed { id }) if id == item => {
                        return Err("profile view failed during extension execution".to_owned())
                    }
                    Ok(EngineEvent::Crashed { id }) if id == item => {
                        return Err("profile view crashed during extension execution".to_owned())
                    }
                    Ok(EngineEvent::ExtensionBrowserRequested { request }) => {
                        coordinator.handle_browser_request(&harness.engine, request)?;
                    }
                    Ok(EngineEvent::ExtensionCompatibilityBrokerRequested { request }) => {
                        coordinator.handle_compatibility_broker_request(
                            &harness.engine,
                            *request,
                        )?;
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
