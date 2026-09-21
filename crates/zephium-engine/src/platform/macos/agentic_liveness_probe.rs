//! Public-site controls with no provider or automated input.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use block2::RcBlock;
use objc2::{rc::Retained, runtime::AnyObject, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDate, NSError, NSHTTPCookie, NSPoint, NSRect, NSRunLoop, NSSize,
    NSString,
};
use objc2_web_kit::WKContentWorld;
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};
use std::{
    cell::RefCell,
    ffi::c_void,
    ptr::NonNull,
    rc::Rc,
    time::{Duration, Instant},
};
use wry::WebViewBuilder;

const OBSERVATION_WINDOW: Duration = Duration::from_secs(30);
const SAMPLE_DEADLINE: Duration = Duration::from_secs(5);
const INTERACTIVE_OBSERVATION_WINDOW: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug)]
pub enum LivenessSite {
    Government,
    Cloudflare,
}
impl LivenessSite {
    fn url(self) -> &'static str {
        match self {
            Self::Government => "https://travel.state.gov/",
            Self::Cloudflare => "https://www.cloudflare.com/",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LivenessStage {
    Bare,
    Safari,
    BrowseScripts,
    DocumentGate,
    Hidden,
    OwnedWork,
}

enum ProbePage {
    Raw(wry::WebView),
    Owned(super::agent_context::AgentOwnedView),
}
impl ProbePage {
    fn view(&self) -> &wry::WebView {
        match self {
            Self::Raw(view) => view,
            Self::Owned(view) => view.view(),
        }
    }
}

struct Host(Retained<NSView>);
impl HasWindowHandle for Host {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(
            NonNull::from(&*self.0).cast::<c_void>(),
        ));
        // SAFETY: the host retains this content view longer than the child WebView.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

fn pump() {
    objc2::rc::autoreleasepool(|_| {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        if let Some(mtm) = MainThreadMarker::new() {
            let app = NSApplication::sharedApplication(mtm);
            if let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
                objc2_app_kit::NSEventMask::Any,
                None,
                objc2_foundation::ns_string!("NSDefaultRunLoopMode"),
                true,
            ) {
                app.sendEvent(&event);
            }
        }
    });
}

const SAMPLE: &str = r#"
const start = performance.now();
let raf = 0, timers = 0, timerMax = 0, timerTotal = 0, stopped = false;
function frame() { if (!stopped) { raf++; requestAnimationFrame(frame); } }
function timer() {
  const before = performance.now();
  setTimeout(() => {
    if (stopped) return;
    const dt = performance.now() - before;
    timers++; timerMax = Math.max(timerMax, dt); timerTotal += dt;
    if (timers < 512) timer();
  }, 0);
}
requestAnimationFrame(frame); timer();
return new Promise(resolve => setTimeout(() => {
  stopped = true;
  const text = (document.body?.innerText || '').slice(0, 65536).toLowerCase();
  const nav = performance.getEntriesByType('navigation')[0];
  resolve(JSON.stringify({
    complete: document.readyState === 'complete', visible: document.visibilityState === 'visible',
    government_content: text.includes('travel advisories') && text.includes('u.s. passports'),
    body_characters: text.length,
    challenge: ['performing security verification','verify you are human','checking your browser','just a moment'].some(x => text.includes(x)),
    light_dom_challenge_frames: Array.from(document.querySelectorAll('iframe')).slice(0, 256).filter(x => { try { return new URL(x.src).hostname === 'challenges.cloudflare.com'; } catch { return false; } }).length,
    light_dom_checkboxes: document.querySelectorAll('input[type=checkbox], [role=checkbox]').length,
    raf, timers, timer_max_ms: Math.round(timerMax), timer_mean_ms: timers ? Math.round(timerTotal / timers) : 0,
    sample_ms: Math.round(performance.now() - start), load_ms: Math.round(nav?.loadEventEnd || 0)
  }));
}, 2000));
"#;

pub fn run_liveness_probe(site: LivenessSite, stage: LivenessStage) -> Result<(), &'static str> {
    run(site, stage, OBSERVATION_WINDOW)
}

pub fn run_interactive_government_probe() -> Result<(), &'static str> {
    run(
        LivenessSite::Government,
        LivenessStage::Bare,
        INTERACTIVE_OBSERVATION_WINDOW,
    )
}

fn run(
    site: LivenessSite,
    stage: LivenessStage,
    observation_window: Duration,
) -> Result<(), &'static str> {
    let mtm = MainThreadMarker::new().ok_or("main_thread")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    // SAFETY: AppKit construction and all accesses are on the main thread.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(80.0, 80.0), NSSize::new(1200.0, 800.0)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: retained owner handles release after child teardown.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("Zephium Liveness Probe"));
    let host = Host(window.contentView().ok_or("content_view")?);
    let hidden = matches!(stage, LivenessStage::Hidden | LivenessStage::OwnedWork);
    let started = Instant::now();
    let gate = (stage == LivenessStage::DocumentGate)
        .then(crate::platform::work_document_navigation::WorkDocumentNavigation::default);
    let policy_gate = gate.clone();
    let event_gate = gate.clone();
    let mut builder = WebViewBuilder::new()
        .with_url(if gate.is_some() { "about:blank" } else { site.url() }).with_incognito(true).with_visible(!hidden).with_focused(!hidden)
        .with_devtools(true)
        .with_apple_navigation_action_handler(move |target, action| {
            let allowed = policy_gate.as_ref().is_none_or(|gate| gate.allows_apple_action(&target, action));
            let challenge = url::Url::parse(&target).is_ok_and(|url| url.host_str() == Some("challenges.cloudflare.com"));
            if challenge { eprintln!("liveness_probe site={site:?} stage={stage:?} frame=challenge requested=true allowed={allowed} main={:?}", action.target_is_main_frame); }
            if !allowed { eprintln!("liveness_probe site={site:?} stage={stage:?} navigation_refused=true"); }
            allowed
        })
        .with_navigation_event_handler(move |event| {
            eprintln!("liveness_probe site={site:?} stage={stage:?} frame=main phase={:?} elapsed_ms={}", event.phase, started.elapsed().as_millis());
            if let Some(gate) = &event_gate { let _ = gate.observe(event); }
        });
    if stage != LivenessStage::Bare {
        builder = builder.with_user_agent(super::safari_user_agent());
    }
    if hidden {
        builder = builder.with_background_throttling(wry::BackgroundThrottlingPolicy::Throttle);
    }
    if matches!(
        stage,
        LivenessStage::BrowseScripts | LivenessStage::DocumentGate | LivenessStage::Hidden
    ) {
        for (source, all_frames) in crate::host::protected_script_specs_for_native_probe() {
            builder = builder.with_initialization_script_for_main_only(source, !all_frames);
        }
    }
    let view = if stage == LivenessStage::OwnedWork {
        use super::agent_context::{build_owned_work_view, AgentOwnedViewCallbacks};
        let store = super::new_ephemeral_data_store().map_err(|_| "store")?;
        let mut owned = build_owned_work_view(
            &host,
            zephium_agentic::ContextOwnedViewport::STANDARD,
            zephium_core::ids::ProfileId::generate(),
            zephium_agentic::ContextProfileStorageClass::Ephemeral,
            Some(&store),
            AgentOwnedViewCallbacks::new(|_| {}, || {}, || {}, || {}, || {}),
        )
        .map_err(|_| "owned_construction")?;
        let navigation = owned.work_navigation().ok_or("owned_gate")?.clone();
        bootstrap(&navigation)?;
        owned
            .prepare_semantic_document_load()
            .map_err(|_| "semantic_load")?;
        arm(&navigation, site)?;
        owned
            .view()
            .load_url(site.url())
            .map_err(|_| "owned_load")?;
        ProbePage::Owned(owned)
    } else {
        let view = builder.build_as_child(&host).map_err(|_| "construction")?;
        if let Some(gate) = &gate {
            bootstrap(gate)?;
            arm(gate, site)?;
            view.load_url(site.url()).map_err(|_| "load")?;
        }
        ProbePage::Raw(view)
    };
    window.makeKeyAndOrderFront(None);
    app.activate();
    while started.elapsed() < observation_window {
        pump();
    }
    let gate_failed = match &view {
        ProbePage::Owned(owned) => owned.work_navigation().is_some_and(|gate| gate.failed()),
        ProbePage::Raw(_) => gate.as_ref().is_some_and(|gate| gate.failed()),
    };
    eprintln!("liveness_probe site={site:?} stage={stage:?} gate_failed={gate_failed}");
    let page = super::native_webview(view.view());
    let cookies = Rc::new(RefCell::new(None));
    let cookie_result = cookies.clone();
    let cookie_callback = RcBlock::new(move |values: NonNull<NSArray<NSHTTPCookie>>| {
        // SAFETY: WebKit retains the cookie array for this callback.
        let values = unsafe { values.as_ref() };
        let mut clearance = false;
        let mut bot_management = false;
        for cookie in values.iter().take(4096) {
            let name = cookie.name();
            clearance |= name.isEqualToString(objc2_foundation::ns_string!("cf_clearance"));
            bot_management |= name.isEqualToString(objc2_foundation::ns_string!("__cf_bm"));
        }
        cookie_result.replace(Some((values.len(), clearance, bot_management)));
    });
    // SAFETY: all retained WebKit objects are accessed on their main thread;
    // the copied callback reads names only, never cookie values.
    unsafe {
        page.configuration()
            .websiteDataStore()
            .httpCookieStore()
            .getAllCookies(&cookie_callback);
    }
    let cookie_start = Instant::now();
    while cookies.borrow().is_none() && cookie_start.elapsed() < SAMPLE_DEADLINE {
        pump();
    }
    if let Some((count, clearance, bot_management)) = cookies.borrow_mut().take() {
        eprintln!("liveness_probe site={site:?} stage={stage:?} cookies={count} clearance={clearance} bot_management={bot_management}");
    } else {
        eprintln!("liveness_probe site={site:?} stage={stage:?} cookie_sample_timeout=true");
    }
    let result = Rc::new(RefCell::new(None));
    let completed = result.clone();
    let callback = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        let text = if error.is_null() {
            // SAFETY: WebKit supplies this object for the callback's duration.
            unsafe { value.as_ref() }
                .and_then(AnyObject::downcast_ref::<NSString>)
                .filter(|s| s.length() <= 2048)
                .map(ToString::to_string)
        } else {
            None
        };
        completed.replace(Some(text));
    });
    // SAFETY: a fixed read-only script, isolated world and retained main-thread page;
    // the callback owns its result state and WebKit copies the block.
    unsafe {
        let world =
            WKContentWorld::worldWithName(&NSString::from_str("zephium-liveness-probe"), mtm);
        page.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
            &NSString::from_str(SAMPLE),
            None,
            None,
            &world,
            Some(&callback),
        );
    }
    let sample_start = Instant::now();
    while result.borrow().is_none() && sample_start.elapsed() < SAMPLE_DEADLINE {
        pump();
    }
    window.orderOut(None);
    let raw = result
        .borrow_mut()
        .take()
        .flatten()
        .ok_or("sample_timeout_or_error")?;
    let facts: serde_json::Value = serde_json::from_str(&raw).map_err(|_| "sample_schema")?;
    for field in ["complete", "visible", "challenge", "government_content"] {
        let value = facts
            .get(field)
            .and_then(serde_json::Value::as_bool)
            .ok_or("sample_schema")?;
        eprintln!("liveness_probe site={site:?} stage={stage:?} fact={field} value={value}");
    }
    for field in [
        "light_dom_challenge_frames",
        "light_dom_checkboxes",
        "raf",
        "timers",
        "timer_max_ms",
        "timer_mean_ms",
        "sample_ms",
        "load_ms",
        "body_characters",
    ] {
        let value = facts
            .get(field)
            .and_then(serde_json::Value::as_u64)
            .ok_or("sample_schema")?;
        eprintln!("liveness_probe site={site:?} stage={stage:?} fact={field} count={value}");
    }
    drop(view);
    window.close();
    Ok(())
}

fn bootstrap(
    gate: &crate::platform::work_document_navigation::WorkDocumentNavigation,
) -> Result<(), &'static str> {
    let start = Instant::now();
    while !gate.bootstrap_ready() && !gate.failed() && start.elapsed() < SAMPLE_DEADLINE {
        pump();
    }
    if gate.bootstrap_ready() {
        Ok(())
    } else {
        Err("bootstrap")
    }
}

fn arm(
    gate: &crate::platform::work_document_navigation::WorkDocumentNavigation,
    site: LivenessSite,
) -> Result<(), &'static str> {
    gate.arm_with_policy(
        zephium_agentic::ContextNavigationTarget::parse(site.url()).map_err(|_| "target")?,
        zephium_agentic::WorkBrowserDocumentPolicy::PublicSameDocumentQuery,
    )
    .map_err(|_| "arm")
}
