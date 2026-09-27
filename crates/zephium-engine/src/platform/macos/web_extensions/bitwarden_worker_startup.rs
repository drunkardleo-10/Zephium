//! Exact prepared Bitwarden worker startup under two native context schemes.
//! This diagnostic owns fresh nonpersistent stores and no product authority.

use super::*;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass};
use objc2_foundation::NSURLRequest;

const BITWARDEN_ID: &str = "nngceckbapebfimnlniiiahkandclblb";

struct DelegateIvars {
    window: Retained<ProbeWindow>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumBitwardenWorkerStartupProbeDelegate"]
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
    fn new(mtm: MainThreadMarker, window: Retained<ProbeWindow>) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(DelegateIvars { window });
        unsafe { msg_send![super(object), init] }
    }
}

struct Observation {
    callback_ok: bool,
    context_errors: usize,
    context_error_code: isize,
    import_error: Option<String>,
    popup_document: Option<String>,
}

pub(super) fn run(path: &Path, scheme: &str, user_agent_mode: &str) -> Result<bool, String> {
    let Some(_) = supported_runtime()? else {
        return Ok(false);
    };
    if !matches!(scheme, "webkit-extension" | "chrome-extension") {
        return Err("unsupported prepared-worker probe scheme".into());
    }
    let use_product_extension_user_agent = match user_agent_mode {
        "native" => false,
        "product-extension" => true,
        _ => return Err("unsupported prepared-worker user-agent mode".into()),
    };
    let watchdog = arm_process_watchdog();
    let result = (|| {
        let mtm = MainThreadMarker::new().ok_or("worker startup probe requires main thread")?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        if scheme == "chrome-extension" {
            // WebKit snapshots custom schemes before the first extension
            // object is parsed; this is the same public registration order
            // used by the existing resource-transport probe.
            unsafe {
                WKWebExtensionMatchPattern::registerCustomURLScheme(
                    &NSString::from_str(scheme),
                    mtm,
                );
            }
        }
        let run_loop = NSRunLoop::mainRunLoop();
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(path.join("manifest.json"))
                .map_err(|_| "prepared manifest unavailable")?,
        )
        .map_err(|_| "prepared manifest invalid")?;
        let wrapper_path = manifest["background"]["service_worker"]
            .as_str()
            .filter(|path| {
                matches!(
                    *path,
                    "__zephium_background_v1.js"
                        | "__zephium_background_v2.js"
                        | "__zephium_background_v3.js"
                )
            })
            .ok_or("prepared Bitwarden background wrapper disagrees")?;
        if manifest["version"] != "2026.9.2" {
            return Err("prepared Bitwarden version or background wrapper disagrees".into());
        }
        let wrapper = std::fs::read_to_string(path.join(wrapper_path))
            .map_err(|_| "prepared worker wrapper unavailable")?;
        let wrapper_class = if wrapper.contains("\"/background.js\"") {
            "full_or_publisher_variant"
        } else {
            "diagnostic_prefix"
        };
        let capture_import_error = wrapper.contains("__zephium_probe_error");
        let api_names = manifest["permissions"]
            .as_array()
            .ok_or("prepared API cohort missing")?
            .iter()
            .map(|value| value.as_str().map(str::to_owned).ok_or("invalid API token"))
            .collect::<Result<Vec<_>, _>>()?;
        let host_patterns = manifest["host_permissions"]
            .as_array()
            .ok_or("prepared host cohort missing")?
            .iter()
            .map(|value| {
                zephium_core::injection::MatchPattern::parse(
                    value.as_str().ok_or("invalid host token")?,
                )
                .map_err(|_| "invalid host pattern")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let observation = run_case(
            path,
            scheme,
            &api_names,
            &host_patterns,
            capture_import_error,
            use_product_extension_user_agent,
            mtm,
            &run_loop,
        )?;
        println!(
            "native-probe: prepared Bitwarden worker scheme={scheme} user_agent_mode={user_agent_mode} generated_wrapper={wrapper_class} callback_ok={} context_errors={} context_error_code={}; publisher_background_bytes_unchanged=true; account_absent=true; product_authority=false",
            observation.callback_ok,
            observation.context_errors,
            observation.context_error_code,
        );
        if capture_import_error {
            println!(
                "native-probe: prepared Bitwarden synchronous-import-exception={}",
                observation.import_error.as_deref().unwrap_or("absent")
            );
        }
        println!(
            "native-probe: prepared Bitwarden popup-document={}",
            observation
                .popup_document
                .as_deref()
                .unwrap_or("unavailable")
        );
        Ok(true)
    })();
    watchdog.store(true, Ordering::Release);
    result
}

fn run_case(
    path: &Path,
    scheme: &str,
    api_names: &[String],
    host_patterns: &[zephium_core::injection::MatchPattern],
    capture_import_error: bool,
    use_product_extension_user_agent: bool,
    mtm: MainThreadMarker,
    run_loop: &NSRunLoop,
) -> Result<Observation, String> {
    objc2::rc::autoreleasepool(|_| {
        // Each case owns a distinct controller and nonpersistent data store.
        let bundle = new_nonpersistent_controller_with_product_extension_user_agent(
            mtm,
            use_product_extension_user_agent,
        )?;
        unsafe {
            bundle
                .webview_configuration
                .setWebExtensionController(Some(&bundle.controller));
        }
        let extension = load_extension(path, run_loop, mtm)?;
        let context = new_context(&extension, BITWARDEN_ID)?;
        let base =
            NSURL::URLWithString(&NSString::from_str(&format!("{scheme}://{BITWARDEN_ID}/")))
                .ok_or("invalid probe base URL")?;
        objc2::exception::catch(AssertUnwindSafe(|| unsafe { context.setBaseURL(&base) }))
            .map_err(|exception| {
                let bounded = exception.as_ref().map_or_else(
                    || "unknown native exception".to_owned(),
                    |exception| exception.to_string().chars().take(240).collect(),
                );
                format!("prepared worker base URL was refused by WebKit: {bounded}")
            })?;
        super::super::extensions::apply_capabilities_v2_probe_grants(
            &context,
            api_names,
            host_patterns,
        )?;
        let window = new_window(mtm)?;
        let host = profile_isolation::host_for_window(&window, "Bitwarden worker startup")?;
        let view =
            profile_isolation::build_profile_view(&host, bundle.webview_configuration.clone())?;
        window.orderFrontRegardless();
        let native_view = super::super::native::webkit(&view);
        assert_attached_controller(&native_view, &bundle.controller)?;
        profile_isolation::assert_attached_store(&native_view, &bundle._data_store)?;
        let views = Arc::new(AtomicUsize::new(0));
        let surfaces = Arc::new(AtomicUsize::new(0));
        let tab = ProbeTab::new(mtm, native_view.clone(), views, surfaces.clone());
        let extension_window = ProbeWindow::new(mtm, tab.clone(), false, surfaces);
        tab.set_window(&extension_window);
        let delegate = Delegate::new(mtm, extension_window.clone());
        unsafe {
            bundle
                .controller
                .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        }
        let mut loaded = false;
        let mut published = false;
        let result = (|| {
            objc2::exception::catch(AssertUnwindSafe(|| {
                load_context(&bundle.controller, &context, "prepared Bitwarden worker")
            }))
            .map_err(|_| "prepared worker context load threw a native exception")??;
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
            let callback_ok = persistent_runtime::load_background_content(
                &context,
                run_loop,
                "prepared Bitwarden worker",
            )
            .is_ok();
            let errors = unsafe { context.errors() };
            let context_error_code = if errors.count() == 0 {
                0
            } else {
                errors.objectAtIndex(errors.count() - 1).code()
            };
            let import_error = if capture_import_error {
                observe_import_error(&context, scheme, mtm, run_loop)?
            } else {
                None
            };
            let popup_document = observe_popup_document(&context, tab_protocol, run_loop);
            Ok(Observation {
                callback_ok,
                context_errors: errors.count(),
                context_error_code,
                import_error,
                popup_document,
            })
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
            unload_context(&bundle.controller, &context, "prepared Bitwarden worker")?;
        }
        super::super::extensions::clear_all_probe_grants(&context)
            .map_err(|error| format!("prepared Bitwarden grant cleanup failed: {error}"))?;
        drop(view);
        window.close();
        result
    })
}

fn observe_popup_document(
    context: &WKWebExtensionContext,
    tab: &ProtocolObject<dyn WKWebExtensionTab>,
    run_loop: &NSRunLoop,
) -> Option<String> {
    let action = unsafe { context.actionForTab(Some(tab)) }?;
    let popup = unsafe { action.popupWebView() }?;
    const PREFIX: &str = "ZEPHIUM_POPUP:";
    let script = NSString::from_str("document.title='ZEPHIUM_POPUP:'+JSON.stringify({popupPage:location.pathname==='/popup/index.html',ready:document.readyState,scripts:document.scripts.length,hashSet:location.hash.length>0,hasInput:!!document.querySelector('input'),hasButton:!!document.querySelector('button')})");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        unsafe { popup.evaluateJavaScript_completionHandler(&script, None) };
        if let Some(value) = unsafe { popup.title() }
            .map(|title| title.to_string())
            .and_then(|title| title.strip_prefix(PREFIX).map(str::to_owned))
        {
            if value.contains("\"popupPage\":true") && value.contains("\"ready\":\"complete\"") {
                return Some(value.chars().take(300).collect());
            }
        }
        drain_run_loop_once(run_loop);
    }
    None
}

fn observe_import_error(
    context: &WKWebExtensionContext,
    scheme: &str,
    mtm: MainThreadMarker,
    run_loop: &NSRunLoop,
) -> Result<Option<String>, String> {
    let configuration = unsafe { context.webViewConfiguration() }
        .ok_or("prepared worker probe extension-page configuration absent")?;
    let view = unsafe {
        WKWebView::initWithFrame_configuration(
            WKWebView::alloc(mtm),
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(320., 200.)),
            &configuration,
        )
    };
    let url = NSURL::URLWithString(&NSString::from_str(&format!(
        "{scheme}://{BITWARDEN_ID}/__zephium_probe.html"
    )))
    .ok_or("prepared worker probe page URL invalid")?;
    unsafe { view.loadRequest(&NSURLRequest::requestWithURL(&url)) };
    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        if let Some(value) = unsafe { view.title() }.map(|title| title.to_string()) {
            if let Some(value) = value.strip_prefix("ZEPHIUM_IMPORT:") {
                unsafe { view.stopLoading() };
                return Ok(Some(value.chars().take(500).collect()));
            }
        }
        drain_run_loop_once(run_loop);
    }
    unsafe { view.stopLoading() };
    Ok(None)
}
