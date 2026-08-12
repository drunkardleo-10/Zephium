//! Isolated capability gate for app-owned macOS extension-resource transport.
//!
//! The fixture is intentionally tiny and Zephium-owned. It compares an
//! ordinary web view with a context-customized extension page while the same
//! exact native handler is attached to both configurations. The ordinary view
//! proves the handler's MIME response is valid; the extension view proves
//! WebKit's private resource loader bypasses that handler even after a custom
//! context base URL is set. This is a capability classification, not a product
//! transport.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{
    MainThreadMarker, NSError, NSHTTPURLResponse, NSMutableDictionary, NSObjectProtocol, NSRunLoop,
    NSString, NSURL,
};
use objc2_web_kit::{
    WKURLSchemeHandler, WKURLSchemeTask, WKWebExtension, WKWebExtensionContext,
    WKWebExtensionController, WKWebExtensionControllerConfiguration, WKWebExtensionMatchPattern,
    WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};
use wry::http::Method;
use wry::WebViewBuilderExtMacos;

const SCHEME: &str = "zepxresource";
const PRINCIPAL: &str = "cccccccccccccccccccccccccccccccc";
const BASE_URL: &str = "zepxresource://cccccccccccccccccccccccccccccccc/";
const CONTROL_PENDING_TITLE: &str = "zephium-resource-handler-control-pending";
const PENDING_TITLE: &str = "zephium-resource-transport-pending";
const MAX_REQUEST_URL_BYTES: usize = 8 * 1_024;
const MAX_SCHEME_REQUESTS: usize = 16;

const MANIFEST: &[u8] = br#"{
  "manifest_version": 3,
  "name": "Zephium Extension Resource Transport Probe",
  "version": "1.0.0",
  "description": "Zephium-owned custom resource transport fixture.",
  "action": { "default_popup": "transport.html" },
  "content_security_policy": {
    "extension_pages": "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'"
  }
}"#;

const PAGE: &[u8] = br#"<!doctype html>
<meta charset="utf-8">
<title>zephium-resource-transport-pending</title>
<script src="transport.js"></script>
"#;

const CONTROL_PAGE: &[u8] = br#"<!doctype html>
<meta charset="utf-8">
<title>zephium-resource-handler-control-pending</title>
<script src="control.js"></script>
"#;

const CONTROL_SCRIPT: &[u8] = br#"void (async () => {
  const evidence = { mime: "absent", streaming: "pending", settled: false };
  try {
    const response = await fetch(new URL("module.wasm", location.href), { cache: "no-store" });
    evidence.mime = response.headers.get("content-type") ?? "absent";
    const result = await WebAssembly.instantiateStreaming(Promise.resolve(response));
    evidence.streaming = result?.instance instanceof WebAssembly.Instance
      ? "instantiated"
      : "invalid-result";
  } catch (_) {
    evidence.streaming = "rejected";
  }
  evidence.settled = true;
  document.title = JSON.stringify(evidence);
})();
"#;

const SCRIPT: &[u8] = br#"void (async () => {
  const api = globalThis.browser ?? globalThis.chrome;
  const evidence = {
    protocol: location.protocol,
    host: location.hostname,
    runtime: typeof api?.runtime,
    runtimeId: api?.runtime?.id === "cccccccccccccccccccccccccccccccc" ? "matched" : "mismatched",
    status: 0,
    mime: "absent",
    streaming: "pending",
    foreign: "pending",
    settled: false
  };

  try {
    const response = await fetch(new URL("module.wasm", location.href), { cache: "no-store" });
    evidence.status = response.status;
    evidence.mime = response.headers.get("content-type") ?? "absent";
    const result = await WebAssembly.instantiateStreaming(Promise.resolve(response));
    evidence.streaming = result?.instance instanceof WebAssembly.Instance
      ? "instantiated"
      : "invalid-result";
  } catch (_) {
    evidence.streaming = "rejected";
  }

  try {
    const response = await fetch(
      "zepxresource://dddddddddddddddddddddddddddddddd/module.wasm",
      { cache: "no-store" }
    );
    evidence.foreign = response.ok ? "exposed" : "denied";
  } catch (_) {
    evidence.foreign = "denied";
  }

  evidence.settled = true;
  document.title = JSON.stringify(evidence);
})();
"#;

// The canonical eight-byte empty WebAssembly module. Streaming compilation
// still performs MIME validation before accepting these bytes.
const WASM: &[u8] = b"\0asm\x01\0\0\0";

struct Resource {
    path: &'static str,
    mime: &'static str,
    bytes: &'static [u8],
}

const RESOURCES: [Resource; 6] = [
    Resource {
        path: "/manifest.json",
        mime: "application/json",
        bytes: MANIFEST,
    },
    Resource {
        path: "/transport.html",
        mime: "text/html",
        bytes: PAGE,
    },
    Resource {
        path: "/control.html",
        mime: "text/html",
        bytes: CONTROL_PAGE,
    },
    Resource {
        path: "/transport.js",
        mime: "text/javascript",
        bytes: SCRIPT,
    },
    Resource {
        path: "/control.js",
        mime: "text/javascript",
        bytes: CONTROL_SCRIPT,
    },
    Resource {
        path: "/module.wasm",
        mime: "application/wasm",
        bytes: WASM,
    },
];

#[derive(Default)]
struct HandlerDiagnostics {
    requests: AtomicUsize,
    responses: AtomicUsize,
    rejections: AtomicUsize,
    wasm_responses: AtomicUsize,
    exact_native_wasm_mime: AtomicUsize,
}

struct ExactMimeHandlerIvars {
    diagnostics: Arc<HandlerDiagnostics>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionResourceExactMimeHandler"]
    #[ivars = ExactMimeHandlerIvars]
    struct ExactMimeHandler;

    unsafe impl NSObjectProtocol for ExactMimeHandler {}

    unsafe impl WKURLSchemeHandler for ExactMimeHandler {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start_task(&self, _webview: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            let diagnostics = &self.ivars().diagnostics;
            let ordinal = diagnostics.requests.fetch_add(1, Ordering::Relaxed) + 1;
            let admitted = (ordinal <= MAX_SCHEME_REQUESTS)
                .then(|| admitted_native_resource(task))
                .flatten();
            let Some((url, resource)) = admitted else {
                diagnostics.rejections.fetch_add(1, Ordering::Relaxed);
                fail_native_task(task);
                return;
            };
            let headers = NSMutableDictionary::new();
            for (name, value) in [
                ("Content-Type", resource.mime),
                ("Content-Length", &resource.bytes.len().to_string()),
                ("Cache-Control", "no-store"),
                ("X-Content-Type-Options", "nosniff"),
            ] {
                headers.insert(&*NSString::from_str(name), &*NSString::from_str(value));
            }
            let Some(response) = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
                NSHTTPURLResponse::alloc(),
                &url,
                200,
                Some(&NSString::from_str("HTTP/1.1")),
                Some(&headers),
            ) else {
                diagnostics.rejections.fetch_add(1, Ordering::Relaxed);
                fail_native_task(task);
                return;
            };
            if resource.mime == "application/wasm"
                && response
                    .MIMEType()
                    .as_deref()
                    .map(NSString::to_string)
                    .as_deref()
                    == Some(resource.mime)
            {
                diagnostics
                    .exact_native_wasm_mime
                    .fetch_add(1, Ordering::Relaxed);
            }
            let data = objc2_foundation::NSData::with_bytes(resource.bytes);
            if objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
                task.didReceiveResponse(&response);
                task.didReceiveData(&data);
                task.didFinish();
            }))
            .is_err()
            {
                diagnostics.rejections.fetch_add(1, Ordering::Relaxed);
                return;
            }
            diagnostics.responses.fetch_add(1, Ordering::Relaxed);
            if resource.mime == "application/wasm" {
                diagnostics.wasm_responses.fetch_add(1, Ordering::Relaxed);
            }
        }

        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop_task(&self, _webview: &WKWebView, _task: &ProtocolObject<dyn WKURLSchemeTask>) {
            // Every fixture response completes synchronously during start.
        }
    }
);

impl ExactMimeHandler {
    fn new(mtm: MainThreadMarker, diagnostics: Arc<HandlerDiagnostics>) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(ExactMimeHandlerIvars { diagnostics });
        // SAFETY: NSObject is the declared superclass and all ivars are fully
        // initialized before invoking its initializer.
        unsafe { msg_send![super(object), init] }
    }
}

fn admitted_native_resource(
    task: &ProtocolObject<dyn WKURLSchemeTask>,
) -> Option<(Retained<NSURL>, &'static Resource)> {
    let request = unsafe { task.request() };
    let method = match request.HTTPMethod() {
        Some(method) => {
            let method = method.to_string();
            Some(Method::from_bytes(method.as_bytes()).ok()?)
        }
        None => None,
    };
    let url = request.URL()?;
    let absolute = url.absoluteString()?.to_string();
    Some((url, admitted_resource(method.as_ref(), &absolute)?))
}

fn fail_native_task(task: &ProtocolObject<dyn WKURLSchemeTask>) {
    let domain = NSString::from_str("app.zephium.extension-resource-probe");
    let error = unsafe { NSError::errorWithDomain_code_userInfo(&domain, 1, None) };
    let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        task.didFailWithError(&error);
    }));
}

struct Fixture {
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
}

impl Fixture {
    fn create() -> Result<Self, String> {
        let temp = tempfile::Builder::new()
            .prefix("zephium-extension-resource-probe-")
            .tempdir()
            .map_err(|error| format!("cannot create extension-resource fixture: {error}"))?;
        let root = temp.path().join("extension");
        std::fs::create_dir(&root)
            .map_err(|error| format!("cannot create extension-resource root: {error}"))?;
        for resource in RESOURCES {
            let name = resource
                .path
                .strip_prefix('/')
                .ok_or_else(|| "extension-resource fixture path is not relative".to_owned())?;
            std::fs::write(root.join(name), resource.bytes).map_err(|error| {
                format!("cannot write extension-resource fixture {name}: {error}")
            })?;
        }
        Ok(Self { _temp: temp, root })
    }
}

struct ControllerBundle {
    _configuration: Retained<WKWebExtensionControllerConfiguration>,
    _webview_configuration: Retained<WKWebViewConfiguration>,
    data_store: Retained<WKWebsiteDataStore>,
    controller: Retained<WKWebExtensionController>,
}

struct Teardown {
    view: Weak<WKWebView>,
    extension: Weak<WKWebExtension>,
    context: Weak<WKWebExtensionContext>,
    controller: Weak<WKWebExtensionController>,
    store: Weak<WKWebsiteDataStore>,
    handler: Weak<ExactMimeHandler>,
    diagnostics: Arc<HandlerDiagnostics>,
    operating_system: String,
}

struct ControlTeardown {
    view: Weak<WKWebView>,
    store: Weak<WKWebsiteDataStore>,
    handler: Weak<ExactMimeHandler>,
    diagnostics: Arc<HandlerDiagnostics>,
}

pub(super) fn run(operating_system: String) -> Result<(), String> {
    super::set_phase("resource-transport-appkit-launch");
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "extension-resource probe must run on the process main thread".to_owned())?;
    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    let run_loop = NSRunLoop::mainRunLoop();

    let control = objc2::rc::autoreleasepool(|_| run_plain_handler_control(&run_loop, mtm))?;
    wait_for_control_teardown(&control, &run_loop)?;
    let teardown = objc2::rc::autoreleasepool(|_| run_supported(operating_system, &run_loop, mtm))?;
    super::set_phase("resource-transport-teardown-wait");
    wait_for_teardown(&teardown, &run_loop)?;
    println!(
        "native-probe: macOS extension-resource transport classified; os={}; plain_handler_wasm_mime=application/wasm; plain_handler_wasm_streaming=passed; custom_base_url=passed; handler_attached=passed; extension_handler_dispatch=absent; extension_identity=passed; extension_wasm_mime=application/octet-stream; extension_wasm_streaming=rejected; base_url_resource_override=unavailable; foreign_principal=denied; extension_handler_requests={}; extension_handler_responses={}; extension_handler_rejections={}; native_objects_released=6; handler_closure_released=passed",
        teardown.operating_system,
        teardown.diagnostics.requests.load(Ordering::Acquire),
        teardown.diagnostics.responses.load(Ordering::Acquire),
        teardown.diagnostics.rejections.load(Ordering::Acquire),
    );
    Ok(())
}

fn run_supported(
    operating_system: String,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<Teardown, String> {
    super::set_phase("resource-transport-fixture");
    // WebKit snapshots recognized extension schemes while parsing, so this
    // process-global registration must precede the first extension object.
    unsafe {
        WKWebExtensionMatchPattern::registerCustomURLScheme(&NSString::from_str(SCHEME), mtm);
    }
    let fixture = Fixture::create()?;
    let extension = super::load_extension(&fixture.root, run_loop, mtm)?;
    validate_extension(&extension)?;
    let bundle = new_controller(mtm)?;
    let context = super::new_context(&extension, PRINCIPAL)?;
    let base_url = NSURL::URLWithString(&NSString::from_str(BASE_URL))
        .ok_or_else(|| "WebKit rejected the extension-resource base URL".to_owned())?;
    unsafe {
        context.setBaseURL(&base_url);
        context.setHasAccessToPrivateData(true);
    }
    let readback = unsafe { context.baseURL() }
        .absoluteString()
        .ok_or_else(|| "extension-resource context returned no base URL".to_owned())?
        .to_string();
    if readback != BASE_URL {
        return Err(format!(
            "extension-resource context changed its exact base URL: {readback:?}"
        ));
    }

    super::set_phase("resource-transport-context-load");
    super::load_context(&bundle.controller, &context, "extension-resource")?;
    let configuration = unsafe { context.webViewConfiguration() }.ok_or_else(|| {
        "loaded extension-resource context returned no extension-page configuration".to_owned()
    })?;
    assert_configuration_controller(&configuration, &bundle.controller)?;
    assert_no_handler(&configuration, "pre-Wry extension-page configuration")?;

    let diagnostics = Arc::new(HandlerDiagnostics::default());
    let handler = ExactMimeHandler::new(mtm, diagnostics.clone());
    install_exact_handler(&configuration, &handler)?;
    let window = super::new_window(mtm)?;
    let host = super::profile_isolation::host_for_window(&window, "extension-resource")?;
    let view = super::profile_isolation::build_profile_view(&host, configuration)?;
    window.orderFrontRegardless();
    let native = super::super::native::webkit(&view);
    super::assert_attached_controller(&native, &bundle.controller)?;
    super::profile_isolation::assert_attached_store(&native, &bundle.data_store)?;
    let constructed_configuration = unsafe { native.configuration() };
    assert_handler_attached(&constructed_configuration, "constructed extension view")?;
    let view_weak = Weak::from_retained(&native);
    drop(native);

    super::set_phase("resource-transport-navigation");
    view.load_url(&format!("{BASE_URL}transport.html"))
        .map_err(|error| format!("cannot navigate extension-resource page: {error}"))?;
    let evidence = wait_for_evidence(&view, &context, &diagnostics, run_loop)?;
    validate_evidence(&evidence)?;
    validate_runtime_diagnostics(&diagnostics)?;
    super::validate_context_errors(&context, "extension-resource")?;

    super::set_phase("resource-transport-unload");
    super::unload_context(&bundle.controller, &context, "extension-resource")?;
    unsafe { context.setHasAccessToPrivateData(false) };

    let teardown = Teardown {
        view: view_weak,
        extension: Weak::from_retained(&extension),
        context: Weak::from_retained(&context),
        controller: Weak::from_retained(&bundle.controller),
        store: Weak::from_retained(&bundle.data_store),
        handler: Weak::from_retained(&handler),
        diagnostics,
        operating_system,
    };
    drop(view);
    window.close();
    drop(window);
    drop(context);
    drop(extension);
    drop(bundle);
    drop(handler);
    drop(fixture);
    Ok(teardown)
}

fn run_plain_handler_control(
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<ControlTeardown, String> {
    let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
    let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
    unsafe { configuration.setWebsiteDataStore(&store) };
    let diagnostics = Arc::new(HandlerDiagnostics::default());
    let handler = ExactMimeHandler::new(mtm, diagnostics.clone());
    install_exact_handler(&configuration, &handler)?;
    let window = super::new_window(mtm)?;
    let host = super::profile_isolation::host_for_window(&window, "resource-handler control")?;
    let view = build_resource_view(&host, configuration)?;
    window.orderFrontRegardless();
    let native = super::super::native::webkit(&view);
    let constructed_configuration = unsafe { native.configuration() };
    assert_handler_attached(&constructed_configuration, "plain control view")?;
    let view_weak = Weak::from_retained(&native);
    drop(native);
    view.load_url(&format!("{BASE_URL}control.html"))
        .map_err(|error| format!("cannot navigate resource-handler control: {error}"))?;
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        let title = view
            .document_title()
            .map_err(|error| format!("cannot inspect resource-handler control: {error}"))?;
        if let Some(title) = title
            .as_deref()
            .filter(|title| !title.is_empty() && *title != CONTROL_PENDING_TITLE)
        {
            let evidence: serde_json::Value = serde_json::from_str(title).map_err(|error| {
                format!("resource-handler control evidence is invalid: {error}; title={title:?}")
            })?;
            validate_streaming_evidence(&evidence, "plain resource-handler control")?;
            break;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "plain resource handler did not settle: url={:?}, title={title:?}, requests={}, responses={}, rejections={}",
                view.url().ok(),
                diagnostics.requests.load(Ordering::Acquire),
                diagnostics.responses.load(Ordering::Acquire),
                diagnostics.rejections.load(Ordering::Acquire),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
    if diagnostics.requests.load(Ordering::Acquire) != 3
        || diagnostics.responses.load(Ordering::Acquire) != 3
        || diagnostics.rejections.load(Ordering::Acquire) != 0
        || diagnostics.wasm_responses.load(Ordering::Acquire) != 1
        || diagnostics.exact_native_wasm_mime.load(Ordering::Acquire) != 1
    {
        return Err("plain resource-handler control diagnostics drifted".into());
    }
    let teardown = ControlTeardown {
        view: view_weak,
        store: Weak::from_retained(&store),
        handler: Weak::from_retained(&handler),
        diagnostics,
    };
    drop(view);
    window.close();
    drop(window);
    drop(store);
    drop(handler);
    Ok(teardown)
}

fn build_resource_view(
    host: &super::ProbeHostView,
    configuration: Retained<WKWebViewConfiguration>,
) -> Result<wry::WebView, String> {
    let mut builder = wry::WebViewBuilder::new().with_webview_configuration(configuration);
    for (source, all_frames) in crate::host::protected_script_specs_for_native_probe() {
        builder = builder.with_initialization_script_for_main_only(source, !all_frames);
    }
    builder
        .build_as_child(host)
        .map_err(|error| format!("cannot construct resource-transport Wry view: {error}"))
}

fn admitted_resource(method: Option<&Method>, absolute: &str) -> Option<&'static Resource> {
    if method.is_some_and(|method| method != Method::GET)
        || absolute.len() > MAX_REQUEST_URL_BYTES
        || !absolute.is_ascii()
    {
        return None;
    }
    let parsed = url::Url::parse(absolute).ok()?;
    if parsed.scheme() != SCHEME
        || parsed.host_str() != Some(PRINCIPAL)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path().contains('%')
    {
        return None;
    }
    let resource = RESOURCES
        .iter()
        .find(|resource| resource.path == parsed.path())?;
    (absolute == format!("{SCHEME}://{PRINCIPAL}{}", resource.path)).then_some(resource)
}

fn validate_extension(extension: &WKWebExtension) -> Result<(), String> {
    let errors = unsafe { extension.errors() };
    if errors.count() != 0 {
        return Err(format!(
            "extension-resource fixture parsed with {} error(s): {}",
            errors.count(),
            super::describe_native_errors(&errors),
        ));
    }
    if unsafe { extension.manifestVersion() } != 3.0
        || unsafe { extension.hasPersistentBackgroundContent() }
    {
        return Err("extension-resource fixture did not retain its bounded MV3 shape".into());
    }
    Ok(())
}

fn new_controller(mtm: MainThreadMarker) -> Result<ControllerBundle, String> {
    let configuration =
        unsafe { WKWebExtensionControllerConfiguration::nonPersistentConfiguration(mtm) };
    if unsafe { configuration.isPersistent() } {
        return Err("extension-resource controller configuration is persistent".into());
    }
    let data_store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
    if unsafe { data_store.isPersistent() } {
        return Err("extension-resource website data store is persistent".into());
    }
    let webview_configuration = unsafe { WKWebViewConfiguration::new(mtm) };
    unsafe {
        webview_configuration.setWebsiteDataStore(&data_store);
        configuration.setWebViewConfiguration(Some(&webview_configuration));
        configuration.setDefaultWebsiteDataStore(Some(&data_store));
    }
    let controller = unsafe {
        WKWebExtensionController::initWithConfiguration(
            WKWebExtensionController::alloc(mtm),
            &configuration,
        )
    };
    unsafe { webview_configuration.setWebExtensionController(Some(&controller)) };
    Ok(ControllerBundle {
        _configuration: configuration,
        _webview_configuration: webview_configuration,
        data_store,
        controller,
    })
}

fn assert_no_handler(
    configuration: &WKWebViewConfiguration,
    description: &str,
) -> Result<(), String> {
    if unsafe { configuration.urlSchemeHandlerForURLScheme(&NSString::from_str(SCHEME)) }.is_some()
    {
        return Err(format!(
            "{description} already owns the custom extension-resource scheme"
        ));
    }
    Ok(())
}

fn assert_handler_attached(
    configuration: &WKWebViewConfiguration,
    description: &str,
) -> Result<(), String> {
    if unsafe { configuration.urlSchemeHandlerForURLScheme(&NSString::from_str(SCHEME)) }.is_none()
    {
        return Err(format!(
            "{description} omitted the custom extension-resource handler"
        ));
    }
    Ok(())
}

fn install_exact_handler(
    configuration: &WKWebViewConfiguration,
    handler: &ExactMimeHandler,
) -> Result<(), String> {
    let scheme = NSString::from_str(SCHEME);
    let handler = ProtocolObject::from_ref(handler);
    unsafe {
        configuration.setURLSchemeHandler_forURLScheme(Some(handler), &scheme);
    }
    let actual = unsafe { configuration.urlSchemeHandlerForURLScheme(&scheme) }
        .ok_or_else(|| "extension-page configuration omitted the exact MIME handler".to_owned())?;
    if !std::ptr::eq(&*actual, handler) {
        return Err("extension-page configuration replaced the exact MIME handler".into());
    }
    Ok(())
}

fn assert_configuration_controller(
    configuration: &WKWebViewConfiguration,
    expected: &WKWebExtensionController,
) -> Result<(), String> {
    let actual = unsafe { configuration.webExtensionController() }.ok_or_else(|| {
        "context extension-page configuration omitted its extension controller".to_owned()
    })?;
    if !std::ptr::eq(&*actual, expected) {
        return Err(
            "context extension-page configuration replaced its extension controller".into(),
        );
    }
    Ok(())
}

fn wait_for_evidence(
    view: &wry::WebView,
    context: &WKWebExtensionContext,
    diagnostics: &HandlerDiagnostics,
    run_loop: &NSRunLoop,
) -> Result<serde_json::Value, String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        let title = view
            .document_title()
            .map_err(|error| format!("cannot inspect extension-resource title: {error}"))?;
        if let Some(title) = title
            .as_deref()
            .filter(|title| !title.is_empty() && *title != PENDING_TITLE)
        {
            return serde_json::from_str(title).map_err(|error| {
                format!("extension-resource evidence is invalid: {error}; title={title:?}")
            });
        }
        super::validate_context_errors(context, "extension-resource")?;
        if Instant::now() >= deadline {
            return Err(format!(
                "extension-resource page did not settle; url={:?}, title={title:?}, requests={}, responses={}, rejections={}",
                view.url().ok(),
                diagnostics.requests.load(Ordering::Acquire),
                diagnostics.responses.load(Ordering::Acquire),
                diagnostics.rejections.load(Ordering::Acquire),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn validate_evidence(evidence: &serde_json::Value) -> Result<(), String> {
    let expected_strings = [
        ("protocol", "zepxresource:"),
        ("host", PRINCIPAL),
        ("runtime", "object"),
        ("runtimeId", "matched"),
        ("mime", "application/octet-stream"),
        ("streaming", "rejected"),
        ("foreign", "denied"),
    ];
    if !expected_strings.iter().all(|(key, expected)| {
        evidence.get(key).and_then(serde_json::Value::as_str) == Some(*expected)
    }) || evidence.get("status").and_then(serde_json::Value::as_u64) != Some(200)
        || evidence.get("settled").and_then(serde_json::Value::as_bool) != Some(true)
    {
        return Err(format!(
            "extension-resource transport classification drifted: {evidence}"
        ));
    }
    Ok(())
}

fn validate_streaming_evidence(
    evidence: &serde_json::Value,
    description: &str,
) -> Result<(), String> {
    let exact = evidence.get("mime").and_then(serde_json::Value::as_str)
        == Some("application/wasm")
        && evidence
            .get("streaming")
            .and_then(serde_json::Value::as_str)
            == Some("instantiated")
        && evidence.get("settled").and_then(serde_json::Value::as_bool) == Some(true);
    exact
        .then_some(())
        .ok_or_else(|| format!("{description} did not preserve strict WASM MIME: {evidence}"))
}

fn validate_runtime_diagnostics(diagnostics: &HandlerDiagnostics) -> Result<(), String> {
    let requests = diagnostics.requests.load(Ordering::Acquire);
    let responses = diagnostics.responses.load(Ordering::Acquire);
    let rejections = diagnostics.rejections.load(Ordering::Acquire);
    let wasm_responses = diagnostics.wasm_responses.load(Ordering::Acquire);
    let exact_native_wasm_mime = diagnostics.exact_native_wasm_mime.load(Ordering::Acquire);
    if requests != 0
        || responses != 0
        || rejections != 0
        || wasm_responses != 0
        || exact_native_wasm_mime != 0
    {
        return Err(format!(
            "extension context unexpectedly dispatched its private resources through the attached custom handler: requests={requests}, responses={responses}, rejections={rejections}, wasm_responses={wasm_responses}, exact_native_wasm_mime={exact_native_wasm_mime}"
        ));
    }
    Ok(())
}

fn wait_for_control_teardown(
    teardown: &ControlTeardown,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + super::TEARDOWN_TIMEOUT;
    loop {
        let retained = usize::from(teardown.view.load().is_some())
            + usize::from(teardown.store.load().is_some())
            + usize::from(teardown.handler.load().is_some());
        if retained == 0 && Arc::strong_count(&teardown.diagnostics) == 1 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "plain resource-handler teardown did not settle: retained={retained}, diagnostic_owners={} ",
                Arc::strong_count(&teardown.diagnostics),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_teardown(teardown: &Teardown, run_loop: &NSRunLoop) -> Result<(), String> {
    let deadline = Instant::now() + super::TEARDOWN_TIMEOUT;
    loop {
        let remaining = [
            teardown.view.load().is_some(),
            teardown.extension.load().is_some(),
            teardown.context.load().is_some(),
            teardown.controller.load().is_some(),
            teardown.store.load().is_some(),
            teardown.handler.load().is_some(),
        ]
        .into_iter()
        .filter(|retained| *retained)
        .count();
        if remaining == 0 && Arc::strong_count(&teardown.diagnostics) == 1 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "extension-resource teardown did not settle: retained={remaining}, diagnostic_owners={}",
                Arc::strong_count(&teardown.diagnostics),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_transport_inventory_rejects_aliases_and_foreign_principals() {
        for resource in RESOURCES {
            let url = format!("{SCHEME}://{PRINCIPAL}{}", resource.path);
            assert_eq!(
                admitted_resource(Some(&Method::GET), &url).map(|item| item.mime),
                Some(resource.mime)
            );
        }
        for url in [
            "zepxresource://dddddddddddddddddddddddddddddddd/module.wasm",
            "zepxresource://cccccccccccccccccccccccccccccccc/missing.wasm",
            "zepxresource://cccccccccccccccccccccccccccccccc/module%2ewasm",
            "zepxresource://cccccccccccccccccccccccccccccccc/module.wasm?x=1",
            "zepxresource://cccccccccccccccccccccccccccccccc:9/module.wasm",
        ] {
            assert!(
                admitted_resource(Some(&Method::GET), url).is_none(),
                "{url}"
            );
        }
        assert!(
            admitted_resource(Some(&Method::POST), &format!("{BASE_URL}module.wasm")).is_none()
        );
    }

    #[test]
    fn fixture_requires_native_streaming_without_a_javascript_fallback() {
        let script = std::str::from_utf8(SCRIPT).unwrap();
        assert!(script.contains("WebAssembly.instantiateStreaming"));
        assert!(!script.contains("WebAssembly.instantiate("));
        assert_eq!(WASM, b"\0asm\x01\0\0\0");
    }
}
