//! Drives the real runtime against real extensions, outside the browser.
//!
//! ```text
//! webext-lab <scenario.json> [--persist <dir>]
//! ```
//!
//! A scenario is a JSON array of steps, run in order:
//! - `{"load": "<dir or .crx>", "compat": true, "id": "<optional id>"}`
//! - `{"background": true}` starts the last loaded extension's background
//! - `{"tab": "<url>"}` opens a tab and waits for it to load
//! - `{"page": "<path>"}` opens one of the extension's pages in a probe view
//! - `{"eval": "<async function body>", "in": "page" | "tab", "timeout": 5000}`
//! - `{"sleep": <ms>}`
//!
//! Every eval result and every extension log line is printed.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSError, NSPoint, NSRect, NSSize, NSString, NSTimer, NSURLRequest, NSURL};
use objc2_web_kit::{WKContentWorld, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore};
use serde_json::Value;
use zephium_webext_macos::{
    compat, ExtensionSpec, Grants, Host, LogLevel, Runtime, TabRequest, TabRequestDone,
    TabSnapshot, WindowSnapshot,
};

const WINDOW: u64 = 1;

fn main() {
    let mut args = std::env::args().skip(1);
    let scenario = args
        .next()
        .expect("usage: webext-lab <scenario.json> [--persist <dir>]");
    let mut persist = None;
    while let Some(flag) = args.next() {
        if flag == "--persist" {
            persist = args.next().map(PathBuf::from);
        }
    }
    let steps: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&scenario).expect("scenario is readable"))
            .expect("scenario is a JSON array");

    let mtm = MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    let lab = Lab::new(mtm, persist);
    lab.steps.borrow_mut().extend(steps);
    Lab::advance(&lab);
    app.run();
}

struct LabHost {
    lab: RefCell<std::rc::Weak<Lab>>,
}

impl Host for LabHost {
    fn log(&self, extension: &str, level: LogLevel, message: &str) {
        let tag = match level {
            LogLevel::Info => "info",
            LogLevel::Warning => "warn",
            LogLevel::Error => "ERROR",
        };
        eprintln!("[{extension} {tag}] {message}");
    }

    fn tab_request(&self, request: TabRequest, done: TabRequestDone) {
        eprintln!("[lab] extension tab request: {request:?}");
        let Some(lab) = self.lab.borrow().upgrade() else {
            return done(Err("closing".into()));
        };
        match request {
            TabRequest::Create { url, active, .. } => {
                let id = lab.open_tab(url.as_deref().unwrap_or("about:blank"), active);
                done(Ok(Some(id)));
            }
            TabRequest::Activate { tab } => {
                lab.active.set(tab);
                lab.publish();
                done(Ok(Some(tab)));
            }
            TabRequest::Close { tab } => {
                lab.tabs.borrow_mut().retain(|(id, view)| {
                    if *id == tab {
                        view.removeFromSuperview();
                    }
                    *id != tab
                });
                lab.publish();
                done(Ok(None));
            }
            TabRequest::Load { tab, url } => {
                if let Some(view) = lab.tab_view(tab) {
                    load(&view, &url);
                }
                done(Ok(Some(tab)));
            }
            _ => done(Err("not supported by the lab".into())),
        }
    }
}

struct Lab {
    mtm: MainThreadMarker,
    window: Retained<NSWindow>,
    runtime: Runtime,
    steps: RefCell<VecDeque<Value>>,
    tabs: RefCell<Vec<(u64, Retained<WKWebView>)>>,
    active: Cell<u64>,
    next_tab: Cell<u64>,
    page: RefCell<Option<Retained<WKWebView>>>,
    extension: RefCell<Option<String>>,
    scratch: PathBuf,
}

impl Lab {
    fn new(mtm: MainThreadMarker, persist: Option<PathBuf>) -> Rc<Self> {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(80.0, 80.0), NSSize::new(1400.0, 800.0)),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str("webext-lab"));
        window.makeKeyAndOrderFront(None);

        let host = Rc::new(LabHost {
            lab: RefCell::new(std::rc::Weak::new()),
        });
        // WebKit's in-memory controller never delivers extension pages'
        // messages to the worker, so the lab uses a persistent profile: fresh
        // per run unless LAB_PROFILE names one to reuse.
        let uuid = match std::env::var("LAB_PROFILE") {
            Ok(text) => objc2_foundation::NSUUID::from_string(&NSString::from_str(&text))
                .expect("LAB_PROFILE is a UUID"),
            Err(_) => objc2_foundation::NSUUID::new(),
        };
        eprintln!("[lab] profile {}", uuid.UUIDString());
        let store = unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&uuid, mtm) };
        let runtime = Runtime::new(mtm, &store, Some(&uuid), host.clone());
        let scratch = persist.unwrap_or_else(|| {
            std::env::temp_dir().join(format!("webext-lab-{}", std::process::id()))
        });
        let _ = std::fs::create_dir_all(&scratch);
        let lab = Rc::new(Self {
            mtm,
            window,
            runtime,
            steps: RefCell::new(VecDeque::new()),
            tabs: RefCell::new(Vec::new()),
            active: Cell::new(0),
            next_tab: Cell::new(1),
            page: RefCell::new(None),
            extension: RefCell::new(None),
            scratch,
        });
        *host.lab.borrow_mut() = Rc::downgrade(&lab);
        lab.publish();
        lab
    }

    fn advance(this: &Rc<Self>) {
        let Some(step) = this.steps.borrow_mut().pop_front() else {
            println!("DONE");
            std::process::exit(0);
        };
        println!("STEP {step}");
        let next = {
            let this = this.clone();
            move || Lab::advance(&this)
        };
        if let Some(source) = step.get("load").and_then(Value::as_str) {
            let compat = step.get("compat").and_then(Value::as_bool).unwrap_or(true);
            match this.prepare(
                Path::new(source),
                compat,
                step.get("id").and_then(Value::as_str),
            ) {
                Ok((id, root)) => {
                    *this.extension.borrow_mut() = Some(id.clone());
                    let spec = ExtensionSpec {
                        id,
                        root,
                        grants: Grants::Requested,
                        inspectable: true,
                    };
                    this.runtime.load(spec, move |result| {
                        println!("LOADED {result:?}");
                        next();
                    });
                }
                Err(error) => {
                    println!("LOAD FAILED {error}");
                    std::process::exit(1);
                }
            }
        } else if step.get("background").is_some() {
            let id = this.extension.borrow().clone().unwrap_or_default();
            this.runtime.start_background(&id, move |result| {
                println!("BACKGROUND {result:?}");
                next();
            });
        } else if let Some(url) = step.get("tab").and_then(Value::as_str) {
            let id = this.open_tab(url, true);
            let view = this.tab_view(id).expect("tab exists");
            wait_loaded(view, next);
        } else if let Some(path) = step.get("page").and_then(Value::as_str) {
            let view = this.open_page(path);
            wait_loaded(view, next);
        } else if let Some(body) = step.get("eval").and_then(Value::as_str) {
            let timeout = step.get("timeout").and_then(Value::as_u64).unwrap_or(5000);
            let view = match step.get("in").and_then(Value::as_str) {
                Some("tab") => this.tab_view(this.active.get()),
                _ => this.page.borrow().clone(),
            };
            let Some(view) = view else {
                println!("EVAL SKIPPED: no view");
                return next();
            };
            evaluate(this.mtm, &view, body, timeout, move |result| {
                println!("RESULT {result}");
                next();
            });
        } else if step.get("errors").is_some() {
            let id = this.extension.borrow().clone().unwrap_or_default();
            if let Some(context) = this.runtime.context(&id) {
                let errors = unsafe { context.errors() };
                println!("CONTEXT ERRORS {}", errors.count());
                for error in errors.iter() {
                    println!("  {}", zephium_webext_macos::describe_error(&error));
                }
                println!(
                    "LOADED={} BACKGROUND={}",
                    unsafe { context.isLoaded() },
                    unsafe { context.webExtension().hasBackgroundContent() }
                );
            }
            next();
        } else if let Some(ms) = step.get("sleep").and_then(Value::as_u64) {
            after(ms, next);
        } else {
            println!("UNKNOWN STEP");
            next();
        }
    }

    fn prepare(
        &self,
        source: &Path,
        compat: bool,
        id: Option<&str>,
    ) -> Result<(String, PathBuf), String> {
        let target = self.scratch.join(format!(
            "ext-{}",
            self.next_tab.get() * 1000 + std::process::id() as u64 % 1000
        ));
        let _ = std::fs::remove_dir_all(&target);
        let id = if source
            .extension()
            .is_some_and(|ext| ext == "crx" || ext == "crx3")
        {
            let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
            let verified = zephium_webext::crx::verify(&bytes, None).map_err(|e| e.to_string())?;
            zephium_webext::archive::extract(verified.zip, &target, &Default::default())
                .map_err(|e| e.to_string())?;
            verified.id.to_string()
        } else {
            copy_dir(source, &target).map_err(|e| e.to_string())?;
            id.unwrap_or("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").to_owned()
        };
        if compat {
            let report = zephium_webext::prepare::prepare(
                &target,
                &zephium_webext::prepare::CompatLayer::new(compat::SCRIPT.to_owned()),
            )
            .map_err(|e| e.to_string())?;
            println!(
                "PREPARED worker={:?} html={} events={}",
                report.worker,
                report.html_injected,
                report.events.len()
            );
            ensure_native_messaging(&target)?;
        }
        Ok((id, target))
    }

    fn configuration(&self) -> Retained<WKWebViewConfiguration> {
        let configuration = unsafe { WKWebViewConfiguration::new(self.mtm) };
        let store = unsafe {
            self.runtime
                .controller()
                .configuration()
                .defaultWebsiteDataStore()
        }
        .expect("controller has a data store");
        unsafe { configuration.setWebsiteDataStore(&store) };
        self.runtime.configure(&configuration);
        configuration
    }

    fn open_tab(&self, url: &str, active: bool) -> u64 {
        let id = self.next_tab.get();
        self.next_tab.set(id + 1);
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(900.0, 800.0));
        let view = unsafe {
            WKWebView::initWithFrame_configuration(
                WKWebView::alloc(self.mtm),
                frame,
                &self.configuration(),
            )
        };
        if let Some(content) = self.window.contentView() {
            content.addSubview(&view);
        }
        self.tabs.borrow_mut().push((id, view.clone()));
        if active || self.active.get() == 0 {
            self.active.set(id);
        }
        self.publish();
        self.runtime.bind_view(id, Some(&view));
        load(&view, url);
        id
    }

    fn open_page(&self, path: &str) -> Retained<WKWebView> {
        let id = self.extension.borrow().clone().unwrap_or_default();
        let context = self.runtime.context(&id).expect("extension is loaded");
        let configuration =
            unsafe { context.webViewConfiguration() }.expect("context configuration");
        let frame = NSRect::new(NSPoint::new(900.0, 0.0), NSSize::new(500.0, 800.0));
        let view = unsafe {
            WKWebView::initWithFrame_configuration(
                WKWebView::alloc(self.mtm),
                frame,
                &configuration,
            )
        };
        if let Some(old) = self.page.borrow_mut().replace(view.clone()) {
            old.removeFromSuperview();
        }
        if let Some(content) = self.window.contentView() {
            content.addSubview(&view);
        }
        let base = unsafe { context.baseURL() }
            .absoluteString()
            .map(|s| s.to_string())
            .unwrap_or_default();
        load(&view, &format!("{base}{}", path.trim_start_matches('/')));
        view
    }

    fn tab_view(&self, id: u64) -> Option<Retained<WKWebView>> {
        self.tabs
            .borrow()
            .iter()
            .find(|(tab, _)| *tab == id)
            .map(|(_, view)| view.clone())
    }

    fn publish(&self) {
        let tabs = self
            .tabs
            .borrow()
            .iter()
            .map(|(id, view)| TabSnapshot {
                id: *id,
                title: unsafe { view.title() }
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
                url: unsafe { view.URL() }
                    .and_then(|u| u.absoluteString())
                    .map(|u| u.to_string()),
                loading: unsafe { view.isLoading() },
                pinned: false,
            })
            .collect();
        let active = Some(self.active.get()).filter(|id| *id != 0);
        self.runtime.publish(
            &[WindowSnapshot {
                id: WINDOW,
                tabs,
                active,
                frame: (80.0, 80.0, 1400.0, 800.0),
            }],
            Some(WINDOW),
        );
    }
}

fn load(view: &WKWebView, url: &str) {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        unsafe { view.loadRequest(&NSURLRequest::requestWithURL(&url)) };
    }
}

fn wait_loaded(view: Retained<WKWebView>, next: impl FnOnce() + 'static) {
    let next = RefCell::new(Some(next));
    let ticks = Cell::new(0u32);
    let block = RcBlock::new(move |timer: NonNull<NSTimer>| {
        ticks.set(ticks.get() + 1);
        let loaded = !unsafe { view.isLoading() } && unsafe { view.URL() }.is_some();
        if loaded || ticks.get() > 300 {
            unsafe { timer.as_ref().invalidate() };
            if !loaded {
                println!("LOAD TIMEOUT");
            }
            if let Some(next) = next.take() {
                next();
            }
        }
    });
    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.1, true, &block) };
}

fn after(ms: u64, next: impl FnOnce() + 'static) {
    let next = RefCell::new(Some(next));
    let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
        if let Some(next) = next.take() {
            next();
        }
    });
    unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(ms as f64 / 1000.0, false, &block)
    };
}

fn evaluate(
    mtm: MainThreadMarker,
    view: &WKWebView,
    body: &str,
    timeout: u64,
    done: impl FnOnce(String) + 'static,
) {
    let script = format!(
        "const __work = (async () => {{ {body} }})();\n\
         const __value = await Promise.race([__work, new Promise(r => setTimeout(() => r('__TIMEOUT__'), {timeout}))]);\n\
         try {{ return JSON.stringify(__value); }} catch (e) {{ return String(__value); }}"
    );
    let done = Rc::new(RefCell::new(Some(done)));
    let watchdog = done.clone();
    after(timeout + 3000, move || {
        if let Some(done) = watchdog.take() {
            done("WATCHDOG: page did not answer".into());
        }
    });
    let block = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        let Some(done) = done.take() else { return };
        if let Some(error) = unsafe { error.as_ref() } {
            done(format!(
                "JS ERROR {}",
                zephium_webext_macos::describe_error(error)
            ));
            return;
        }
        let text = unsafe { value.as_ref() }
            .and_then(|value| value.downcast_ref::<NSString>())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "undefined".into());
        done(text);
    });
    unsafe {
        view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
            &NSString::from_str(&script),
            None,
            None,
            &WKContentWorld::pageWorld(mtm),
            Some(&block),
        )
    };
}

fn ensure_native_messaging(root: &Path) -> Result<(), String> {
    let path = root.join("manifest.json");
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut manifest: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let permissions = manifest
        .as_object_mut()
        .ok_or("manifest is not an object")?
        .entry("permissions")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(list) = permissions.as_array_mut() {
        if !list.iter().any(|p| p == "nativeMessaging") {
            list.push(Value::String("nativeMessaging".into()));
            std::fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap())
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
