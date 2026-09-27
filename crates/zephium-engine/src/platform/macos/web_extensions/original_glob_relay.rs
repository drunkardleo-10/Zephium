//! Isolated runtime check for an authenticated, externally prepared glob-relay
//! artifact on an ordinary editable top-level document. No account is used.

use super::*;
use objc2::rc::Weak;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass};
use objc2_foundation::{NSArray, NSSet};
use objc2_web_kit::{WKWebExtensionWindow, WKWebView};

struct DelegateIvars {
    window: Retained<ProbeWindow>,
    drops: Arc<AtomicUsize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumOriginalGlobRelayProbeDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl WKWebExtensionControllerDelegate for Delegate {
        #[unsafe(method_id(webExtensionController:openWindowsForExtensionContext:))]
        fn open_windows(
            &self,
            _: &WKWebExtensionController,
            _: &WKWebExtensionContext,
        ) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionWindow>>> {
            let window = ProtocolObject::from_retained(self.ivars().window.clone());
            NSArray::arrayWithObject(&*window)
        }

        #[unsafe(method_id(webExtensionController:focusedWindowForExtensionContext:))]
        fn focused_window(
            &self,
            _: &WKWebExtensionController,
            _: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
            Some(ProtocolObject::from_retained(self.ivars().window.clone()))
        }
    }
);

impl Delegate {
    fn new(
        mtm: MainThreadMarker,
        window: Retained<ProbeWindow>,
        drops: Arc<AtomicUsize>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(DelegateIvars { window, drops });
        unsafe { msg_send![super(object), init] }
    }
}
impl Drop for Delegate {
    fn drop(&mut self) {
        self.ivars().drops.fetch_add(1, Ordering::Release);
    }
}

pub(super) fn run(path: &Path) -> Result<bool, String> {
    let Some(_) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog = arm_process_watchdog();
    let result = run_inner(path);
    watchdog.store(true, Ordering::Release);
    result.map(|()| true)
}

fn run_inner(path: &Path) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("original glob relay probe requires main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    let run_loop = NSRunLoop::mainRunLoop();
    let server = FixtureServer::start(None)?;
    let (weak, outcome) = objc2::rc::autoreleasepool(|_| {
        let bundle = new_nonpersistent_controller(mtm)?;
        unsafe {
            bundle
                .webview_configuration
                .setWebExtensionController(Some(&bundle.controller));
        }
        let extension = load_extension(path, &run_loop, mtm)?;
        let context = new_context(&extension, "zephium-original-glob-relay-probe-v1")?;
        let unavailable = NSSet::from_retained_slice(&[NSString::from_str("browser.sidePanel")]);
        unsafe {
            context.setUnsupportedAPIs(Some(&unavailable));
        }
        let grants = super::super::extensions::apply_probe_grants(
            &context,
            &[
                super::super::extensions::MacosNativeApiPermission::Cookies,
                super::super::extensions::MacosNativeApiPermission::NativeMessaging,
                super::super::extensions::MacosNativeApiPermission::Notifications,
                super::super::extensions::MacosNativeApiPermission::Scripting,
                super::super::extensions::MacosNativeApiPermission::Storage,
                super::super::extensions::MacosNativeApiPermission::Tabs,
            ],
            &[HOST_MATCH_PATTERN],
            true,
        )
        .map_err(|error| format!("original glob relay grants failed: {error}"))?;
        let window = new_window(mtm)?;
        let host = profile_isolation::host_for_window(&window, "original glob relay")?;
        let view =
            profile_isolation::build_profile_view(&host, bundle.webview_configuration.clone())?;
        window.orderFrontRegardless();
        let native_view = super::super::native::webkit(&view);
        assert_attached_controller(&native_view, &bundle.controller)?;
        profile_isolation::assert_attached_store(&native_view, &bundle._data_store)?;
        let views = Arc::new(AtomicUsize::new(0));
        let surfaces = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let tab = ProbeTab::new(mtm, native_view.clone(), views, surfaces.clone());
        let extension_window = ProbeWindow::new(mtm, tab.clone(), false, surfaces.clone());
        tab.set_window(&extension_window);
        let delegate = Delegate::new(mtm, extension_window.clone(), drops.clone());
        unsafe {
            bundle
                .controller
                .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        }
        let controller_weak = Weak::from_retained(&bundle.controller);
        let context_weak = Weak::from_retained(&context);
        let view_weak = Weak::from_retained(&native_view);
        let store_weak = Weak::from_retained(&bundle._data_store);
        let mut loaded = false;
        let mut published = false;
        let outcome = (|| {
            load_context(&bundle.controller, &context, "original glob relay")?;
            loaded = true;
            let window_protocol = ProtocolObject::from_ref(&*extension_window);
            let tab_protocol = ProtocolObject::from_ref(&*tab);
            unsafe {
                bundle.controller.didOpenWindow(window_protocol);
                bundle.controller.didOpenTab(tab_protocol);
                bundle.controller.didFocusWindow(Some(window_protocol));
                bundle
                    .controller
                    .didActivateTab_previousActiveTab(tab_protocol, None);
            }
            published = true;
            persistent_runtime::load_background_content(
                &context,
                &run_loop,
                "original glob relay",
            )?;
            let url = server.url("/editor", "original-glob-relay");
            let native_url = native_url(&url)?;
            assert_context_access(
                &context,
                &native_url,
                true,
                "original glob relay host grant",
            )?;
            view.load_url(&url)
                .map_err(|error| format!("cannot open original editor: {error}"))?;
            wait_for_editor(&native_view, &context, &run_loop)
        })();
        if published {
            let window_protocol = ProtocolObject::from_ref(&*extension_window);
            let tab_protocol = ProtocolObject::from_ref(&*tab);
            unsafe {
                bundle.controller.didFocusWindow(None);
                bundle
                    .controller
                    .didCloseTab_windowIsClosing(tab_protocol, true);
                bundle.controller.didCloseWindow(window_protocol);
            }
        }
        unsafe {
            bundle.controller.setDelegate(None);
            native_view.stopLoading();
        }
        if loaded {
            unload_context(&bundle.controller, &context, "original glob relay")?;
        }
        grants
            .clear_and_verify(&context)
            .map_err(|error| format!("original glob relay grant cleanup failed: {error}"))?;
        drop(view);
        window.close();
        drop(window);
        drop(native_view);
        drop(tab);
        drop(extension_window);
        drop(delegate);
        Ok::<_, String>((
            (
                controller_weak,
                context_weak,
                view_weak,
                store_weak,
                surfaces,
                drops,
            ),
            outcome,
        ))
    })?;
    let deadline = Instant::now() + TEARDOWN_TIMEOUT;
    loop {
        if weak.0.load().is_none()
            && weak.1.load().is_none()
            && weak.2.load().is_none()
            && weak.3.load().is_none()
            && weak.4.load(Ordering::Acquire) == 2
            && weak.5.load(Ordering::Acquire) == 1
        {
            break;
        }
        if Instant::now() >= deadline {
            return Err("original glob relay native owners did not release".into());
        }
        drain_run_loop_once(&run_loop);
    }
    outcome
}

fn wait_for_editor(
    view: &WKWebView,
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        let last = evaluate(view, run_loop, deadline)?;
        if last["ready"] == "complete"
            && last["textarea"] == true
            && last["extensionMarker"] == true
        {
            eprintln!("native-original-glob-relay: ordinary editable document shows extension-owned DOM marker; state={last}");
            return Ok(());
        }
        validate_context_errors(context, "original glob relay")?;
        if Instant::now() >= deadline {
            return Err(format!(
                "original Grammarly editor did not show an extension marker: {last}"
            ));
        }
        drain_run_loop_once(run_loop);
    }
}

fn evaluate(
    view: &WKWebView,
    run_loop: &NSRunLoop,
    deadline: Instant,
) -> Result<serde_json::Value, String> {
    let result = Rc::new(RefCell::new(None));
    let captured = result.clone();
    let completion = block2::RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        captured.replace(Some(if let Some(error) = unsafe { error.as_ref() } {
            Err(format_native_error("original glob relay editor", error))
        } else {
            unsafe { value.as_ref() }
                .and_then(AnyObject::downcast_ref::<NSString>)
                .map(ToString::to_string)
                .ok_or_else(|| "editor evaluation returned non-string".to_owned())
        }));
    });
    unsafe {
        view.evaluateJavaScript_completionHandler(&NSString::from_str(r#"
          (() => {
            const nodes = [...document.querySelectorAll('*')];
            const markers = nodes.filter(e => /grammarly|^gr-/i.test(e.localName) ||
              [...e.attributes].some(a => /grammarly|^data-gr-/i.test(a.name)));
            return JSON.stringify({
              ready: document.readyState,
              textarea: !!document.querySelector('textarea'),
              extensionMarker: markers.length > 0,
              markerTags: markers.slice(0, 6).map(e => e.localName),
              markerAttributes: markers.slice(0, 6).map(e => [...e.attributes].map(a => a.name).filter(n => /grammarly|^data-gr-/i.test(n)).slice(0, 6)),
              elements: nodes.length,
              mainWorldApi: typeof globalThis.chrome?.runtime,
            });
          })()
        "#), Some(&completion));
    }
    loop {
        if let Some(result) = result.borrow_mut().take() {
            return serde_json::from_str(&result?)
                .map_err(|error| format!("invalid editor state: {error}"));
        }
        if Instant::now() >= deadline {
            return Err("original editor evaluation timed out".into());
        }
        drain_run_loop_once(run_loop);
    }
}
