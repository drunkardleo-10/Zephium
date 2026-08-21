//! Live admission probe for Apple's public macOS browser-extension API.
//!
//! This module is compiled only for the `native-web-extension-probes` feature.
//! Its fixture, delegates, HTTP harness, and diagnostics have no product entry
//! point. Passing it proves the native backend behavior, but does not provision
//! a product extension catalog or complete the user-facing extension feature.

mod artifact_tree;
mod bitwarden_contract;
mod bitwarden_core_artifact;
mod compatibility_artifact;
mod compatibility_fixture;
mod file_access;
mod major_extension_contract;
mod native_broker_contract;
mod permission_requests;
mod persistent_runtime;
mod profile_isolation;
mod representative_extension;
mod resource_transport;
mod stock_password_manager;
mod vimium_contract;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDate, NSError, NSObjectProtocol, NSPoint, NSProcessInfo, NSRect,
    NSRectEdge, NSRunLoop, NSSet, NSSize, NSString, NSURL,
};
use objc2_web_kit::{
    WKUserScriptInjectionTime, WKWebExtension, WKWebExtensionAction, WKWebExtensionContext,
    WKWebExtensionContextPermissionStatus, WKWebExtensionController,
    WKWebExtensionControllerConfiguration, WKWebExtensionControllerDelegate,
    WKWebExtensionMatchPattern, WKWebExtensionPermission, WKWebExtensionTab, WKWebExtensionWindow,
    WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};
use serde_json::{json, Value};
use wry::WebViewBuilderExtMacos;

const MINIMUM_MACOS_MAJOR: isize = 15;
const MINIMUM_MACOS_MINOR: isize = 4;
const PROBE_TIMEOUT: Duration = Duration::from_secs(12);
const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const PROCESS_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(60);
const IPC_BODY_LIMIT: usize = 64 * 1024;
const HTTP_REQUEST_LIMIT: usize = 8 * 1024;
const HTTP_RESPONSE_LIMIT: usize = 256 * 1024;
const MAX_HTTP_REQUESTS: usize = 128;
const MAX_EXTENSION_SCRIPT_DELTA: usize = 16;
const MAX_WEBVIEW_CALLBACKS: usize = 1_024;
const EXPECTED_NATIVE_CONTROLLERS: usize = 5;
const PROBE_TOKEN: &str = "zephium-wk-web-extension-v1";
const PERSISTENT_PROFILE_A: u128 = 0xf0cc_44f2_4355_4cf9_a74f_27fe_6cb3_7eda;
const PERSISTENT_PROFILE_B: u128 = 0x6a90_af85_503a_4db6_8359_a082_5da9_07ab;
const PRIMARY_CONTEXT_IDENTIFIER: &str = "zephium-probe-primary";
const PROFILE_ROUTING_PRINCIPALS: [&str; 2] =
    ["zephium-profile-route-a", "zephium-profile-route-b"];
const HOST_MATCH_PATTERN: &str = "http://127.0.0.1/*";
const BROAD_HTTP_MATCH_PATTERN: &str = "http://*/*";
const EXCLUDED_MATCH_PATTERN: &str = "http://127.0.0.1/excluded/*";
const EXPECTED_ROLES: [&str; 8] = [
    "main", "same", "cross", "excluded", "about", "srcdoc", "data", "blob",
];
static PROBE_PHASE: Mutex<&'static str> = Mutex::new("runtime-admission");

fn persistent_probe_profiles() -> [zephium_core::ids::ProfileId; 2] {
    [
        zephium_core::ids::ProfileId::from(PERSISTENT_PROFILE_A),
        zephium_core::ids::ProfileId::from(PERSISTENT_PROFILE_B),
    ]
}

struct ProbeHostView {
    view: Retained<NSView>,
}

impl HasWindowHandle for ProbeHostView {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let pointer = NonNull::from(&*self.view).cast::<c_void>();
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(pointer));
        // SAFETY: the retained NSView outlives the returned borrow and the Wry
        // child view constructed from it.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

struct ProbeTabIvars {
    webview: Retained<WKWebView>,
    window: RefCell<Option<Weak<ProbeWindow>>>,
    webview_requests: Arc<AtomicUsize>,
    lifecycle_drops: Arc<AtomicUsize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumWebExtensionProbeTab"]
    #[ivars = ProbeTabIvars]
    struct ProbeTab;

    unsafe impl NSObjectProtocol for ProbeTab {}

    unsafe impl WKWebExtensionTab for ProbeTab {
        #[unsafe(method_id(windowForWebExtensionContext:))]
        fn window_for_context(
            &self,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
            self.ivars()
                .window
                .borrow()
                .as_ref()
                .and_then(Weak::load)
                .map(ProtocolObject::from_retained)
        }

        #[unsafe(method_id(webViewForWebExtensionContext:))]
        fn webview_for_context(
            &self,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<WKWebView>> {
            let request = self
                .ivars()
                .webview_requests
                .fetch_add(1, Ordering::Relaxed)
                + 1;
            if request == 1 {
                eprintln!("native-probe-callback: webViewForWebExtensionContext first-request");
            }
            Some(self.ivars().webview.clone())
        }

        #[unsafe(method(indexInWindowForWebExtensionContext:))]
        fn index_in_window(&self, _context: &WKWebExtensionContext) -> usize {
            0
        }

        #[unsafe(method(isSelectedForWebExtensionContext:))]
        fn is_selected(&self, _context: &WKWebExtensionContext) -> bool {
            true
        }
    }
);

impl ProbeTab {
    fn new(
        mtm: MainThreadMarker,
        webview: Retained<WKWebView>,
        webview_requests: Arc<AtomicUsize>,
        lifecycle_drops: Arc<AtomicUsize>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(ProbeTabIvars {
            webview,
            window: RefCell::new(None),
            webview_requests,
            lifecycle_drops,
        });
        // SAFETY: NSObject is the declared superclass and the ivars are fully
        // initialized before invoking its initializer.
        unsafe { msg_send![super(object), init] }
    }

    fn set_window(&self, window: &Retained<ProbeWindow>) {
        *self.ivars().window.borrow_mut() = Some(Weak::from_retained(window));
    }
}

impl Drop for ProbeTab {
    fn drop(&mut self) {
        self.ivars().lifecycle_drops.fetch_add(1, Ordering::Relaxed);
    }
}

struct ProbeWindowIvars {
    tab: Retained<ProbeTab>,
    is_private: bool,
    lifecycle_drops: Arc<AtomicUsize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumWebExtensionProbeWindow"]
    #[ivars = ProbeWindowIvars]
    struct ProbeWindow;

    unsafe impl NSObjectProtocol for ProbeWindow {}

    unsafe impl WKWebExtensionWindow for ProbeWindow {
        #[unsafe(method_id(tabsForWebExtensionContext:))]
        fn tabs_for_context(
            &self,
            _context: &WKWebExtensionContext,
        ) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionTab>>> {
            let tab = ProtocolObject::from_retained(self.ivars().tab.clone());
            NSArray::arrayWithObject(&*tab)
        }

        #[unsafe(method_id(activeTabForWebExtensionContext:))]
        fn active_tab_for_context(
            &self,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionTab>>> {
            Some(ProtocolObject::from_retained(self.ivars().tab.clone()))
        }

        #[unsafe(method(isPrivateForWebExtensionContext:))]
        fn is_private(&self, _context: &WKWebExtensionContext) -> bool {
            self.ivars().is_private
        }
    }
);

impl ProbeWindow {
    fn new(
        mtm: MainThreadMarker,
        tab: Retained<ProbeTab>,
        is_private: bool,
        lifecycle_drops: Arc<AtomicUsize>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(ProbeWindowIvars {
            tab,
            is_private,
            lifecycle_drops,
        });
        // SAFETY: NSObject is the declared superclass and the ivars are fully
        // initialized before invoking its initializer.
        unsafe { msg_send![super(object), init] }
    }
}

impl Drop for ProbeWindow {
    fn drop(&mut self) {
        self.ivars().lifecycle_drops.fetch_add(1, Ordering::Relaxed);
    }
}

struct ProbeControllerDelegateIvars {
    window: Retained<ProbeWindow>,
    permission_requests: Rc<permission_requests::PermissionRequestProbe>,
    native_message: Option<Rc<ProbeNativeMessageContract>>,
    lifecycle_drops: Arc<AtomicUsize>,
}

/// One exact, read-only native-message exchange used by product-shaped
/// compatibility probes. The contract is bound to native object identity and
/// cardinality before WebKit can execute extension code. It deliberately has
/// no generic dispatch surface and grants no product or external-host
/// authority.
struct ProbeNativeMessageContract {
    controller: NonNull<WKWebExtensionController>,
    context: NonNull<WKWebExtensionContext>,
    application_identifier: String,
    exchanges: Box<[(String, String)]>,
    calls: Cell<usize>,
    exact: Cell<bool>,
    failure: RefCell<Option<String>>,
}

impl ProbeNativeMessageContract {
    fn new_sequence(
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
        application_identifier: impl Into<String>,
        exchanges: Vec<(String, String)>,
    ) -> Rc<Self> {
        Rc::new(Self {
            controller: NonNull::from(controller),
            context: NonNull::from(context),
            application_identifier: application_identifier.into(),
            exchanges: exchanges.into_boxed_slice(),
            calls: Cell::new(0),
            exact: Cell::new(false),
            failure: RefCell::new(None),
        })
    }

    fn handle(
        &self,
        controller: &WKWebExtensionController,
        message: &AnyObject,
        application_identifier: Option<&NSString>,
        context: &WKWebExtensionContext,
        reply: &block2::DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
    ) {
        let index = self.calls.get();
        self.calls.set(index.saturating_add(1));
        let expected = self.exchanges.get(index);
        let exact = expected.is_some()
            && NonNull::from(controller) == self.controller
            && NonNull::from(context) == self.context
            && application_identifier.is_some_and(|actual| {
                actual.isEqualToString(&NSString::from_str(&self.application_identifier))
            })
            && message.downcast_ref::<NSString>().is_some_and(|actual| {
                expected.is_some_and(|(request, _)| {
                    actual.isEqualToString(&NSString::from_str(request))
                })
            });
        self.exact.set(if index == 0 {
            exact
        } else {
            self.exact.get() && exact
        });
        if exact {
            if let Some((_, response)) = expected {
                let response = NSString::from_str(response);
                reply.call((
                    Retained::as_ptr(&response).cast_mut().cast(),
                    std::ptr::null_mut(),
                ));
            } else {
                complete_probe_native_message_error(reply);
            }
        } else {
            let mut failure = self.failure.borrow_mut();
            if failure.is_none() {
                *failure = Some(
                    "native-message controller, principal, identifier, payload, or cardinality drifted"
                        .into(),
                );
            }
            complete_probe_native_message_error(reply);
        }
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(failure) = self.failure.borrow().as_ref() {
            return Err(format!("native-message contract failed closed: {failure}"));
        }
        if self.calls.get() != self.exchanges.len() || !self.exact.get() {
            return Err(format!(
                "native-message contract did not settle exact sequence: calls={}, expected={}, exact={}",
                self.calls.get(),
                self.exchanges.len(),
                self.exact.get(),
            ));
        }
        Ok(())
    }

    fn settlement(&self) -> Result<bool, String> {
        if let Some(failure) = self.failure.borrow().as_ref() {
            return Err(format!("native-message contract failed closed: {failure}"));
        }
        match self.calls.get() {
            calls if calls < self.exchanges.len() && self.exact.get() => Ok(false),
            calls if calls == self.exchanges.len() && self.exact.get() => Ok(true),
            calls => Err(format!(
                "native-message contract cardinality drifted: calls={calls}, exact={}",
                self.exact.get(),
            )),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.get()
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumWebExtensionProbeControllerDelegate"]
    #[ivars = ProbeControllerDelegateIvars]
    struct ProbeControllerDelegate;

    unsafe impl NSObjectProtocol for ProbeControllerDelegate {}

    unsafe impl WKWebExtensionControllerDelegate for ProbeControllerDelegate {
        #[unsafe(method_id(webExtensionController:openWindowsForExtensionContext:))]
        fn open_windows(
            &self,
            _controller: &WKWebExtensionController,
            _context: &WKWebExtensionContext,
        ) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionWindow>>> {
            let window = ProtocolObject::from_retained(self.ivars().window.clone());
            NSArray::arrayWithObject(&*window)
        }

        #[unsafe(method_id(webExtensionController:focusedWindowForExtensionContext:))]
        fn focused_window(
            &self,
            _controller: &WKWebExtensionController,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
            Some(ProtocolObject::from_retained(self.ivars().window.clone()))
        }

        #[unsafe(method(webExtensionController:promptForPermissions:inTab:forExtensionContext:completionHandler:))]
        fn prompt_for_permissions(
            &self,
            _controller: &WKWebExtensionController,
            permissions: &NSSet<WKWebExtensionPermission>,
            tab: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<
                dyn Fn(NonNull<NSSet<WKWebExtensionPermission>>, *mut NSDate),
            >,
        ) {
            self.ivars().permission_requests.complete_permissions(
                permissions,
                tab,
                context,
                completion,
            );
        }

        #[unsafe(method(webExtensionController:promptForPermissionToAccessURLs:inTab:forExtensionContext:completionHandler:))]
        fn prompt_for_urls(
            &self,
            _controller: &WKWebExtensionController,
            urls: &NSSet<NSURL>,
            tab: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(NonNull<NSSet<NSURL>>, *mut NSDate)>,
        ) {
            self.ivars()
                .permission_requests
                .reject_urls(urls, tab, context, completion);
        }

        #[unsafe(method(webExtensionController:promptForPermissionMatchPatterns:inTab:forExtensionContext:completionHandler:))]
        fn prompt_for_patterns(
            &self,
            _controller: &WKWebExtensionController,
            patterns: &NSSet<WKWebExtensionMatchPattern>,
            tab: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<
                dyn Fn(NonNull<NSSet<WKWebExtensionMatchPattern>>, *mut NSDate),
            >,
        ) {
            self.ivars()
                .permission_requests
                .complete_patterns(patterns, tab, context, completion);
        }

        #[unsafe(method(webExtensionController:presentPopupForAction:forExtensionContext:completionHandler:))]
        fn present_popup(
            &self,
            _controller: &WKWebExtensionController,
            action: &WKWebExtensionAction,
            _context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            let shown = objc2::exception::catch(AssertUnwindSafe(|| {
                let Some(popover) = (unsafe { action.popupPopover() }) else {
                    return false;
                };
                let anchor = &self.ivars().window.ivars().tab.ivars().webview;
                popover.setContentSize(NSSize::new(400.0, 600.0));
                popover.showRelativeToRect_ofView_preferredEdge(
                    NSRect::new(NSPoint::new(8.0, 8.0), NSSize::new(24.0, 24.0)),
                    anchor,
                    NSRectEdge::MaxY,
                );
                popover.isShown()
            }))
            .unwrap_or(false);
            if shown {
                completion.call((std::ptr::null_mut(),));
            } else {
                complete_probe_popup_error(completion);
            }
        }

        #[unsafe(method(webExtensionController:sendMessage:toApplicationWithIdentifier:forExtensionContext:replyHandler:))]
        unsafe fn send_message(
            &self,
            controller: &WKWebExtensionController,
            message: &AnyObject,
            application_identifier: Option<&NSString>,
            context: &WKWebExtensionContext,
            reply: &block2::DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
        ) {
            if let Some(contract) = self.ivars().native_message.as_ref() {
                contract.handle(controller, message, application_identifier, context, reply);
            } else {
                complete_probe_native_message_error(reply);
            }
        }
    }
);

fn complete_probe_popup_error(completion: &block2::DynBlock<dyn Fn(*mut NSError)>) {
    let domain = NSString::from_str("app.zephium.web-extension-probe");
    let error = unsafe { NSError::errorWithDomain_code_userInfo(&domain, 1, None) };
    completion.call((Retained::as_ptr(&error).cast_mut(),));
}

fn complete_probe_native_message_error(
    completion: &block2::DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
) {
    let domain = NSString::from_str("app.zephium.web-extension-probe.native-message");
    let error = unsafe { NSError::errorWithDomain_code_userInfo(&domain, 1, None) };
    completion.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
}

impl ProbeControllerDelegate {
    fn new(
        mtm: MainThreadMarker,
        window: Retained<ProbeWindow>,
        lifecycle_drops: Arc<AtomicUsize>,
    ) -> Retained<Self> {
        Self::new_with_permission_requests(
            mtm,
            window,
            Rc::new(permission_requests::PermissionRequestProbe::default()),
            lifecycle_drops,
        )
    }

    fn new_with_native_message(
        mtm: MainThreadMarker,
        window: Retained<ProbeWindow>,
        native_message: Rc<ProbeNativeMessageContract>,
        lifecycle_drops: Arc<AtomicUsize>,
    ) -> Retained<Self> {
        Self::new_configured(
            mtm,
            window,
            Rc::new(permission_requests::PermissionRequestProbe::default()),
            Some(native_message),
            lifecycle_drops,
        )
    }

    fn new_with_permission_requests(
        mtm: MainThreadMarker,
        window: Retained<ProbeWindow>,
        permission_requests: Rc<permission_requests::PermissionRequestProbe>,
        lifecycle_drops: Arc<AtomicUsize>,
    ) -> Retained<Self> {
        Self::new_configured(mtm, window, permission_requests, None, lifecycle_drops)
    }

    fn new_configured(
        mtm: MainThreadMarker,
        window: Retained<ProbeWindow>,
        permission_requests: Rc<permission_requests::PermissionRequestProbe>,
        native_message: Option<Rc<ProbeNativeMessageContract>>,
        lifecycle_drops: Arc<AtomicUsize>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(ProbeControllerDelegateIvars {
            window,
            permission_requests,
            native_message,
            lifecycle_drops,
        });
        // SAFETY: NSObject is the declared superclass and the ivars are fully
        // initialized before invoking its initializer.
        unsafe { msg_send![super(object), init] }
    }
}

impl Drop for ProbeControllerDelegate {
    fn drop(&mut self) {
        self.ivars().lifecycle_drops.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct UserScriptFingerprint {
    source: String,
    injection_time: isize,
    main_frame_only: bool,
}

#[derive(Default)]
struct ProbeState {
    batches: HashMap<String, Value>,
    duplicate_batch: bool,
    malformed_batch: Option<String>,
}

#[derive(Clone, Copy)]
enum ExpectedExtensions {
    None,
    Both,
    PeerOnly,
}

struct Fixture {
    _temp: tempfile::TempDir,
    primary_path: std::path::PathBuf,
    peer_path: std::path::PathBuf,
    permission_path: std::path::PathBuf,
    bitwarden_contract_path: std::path::PathBuf,
    major_extension_contract_path: std::path::PathBuf,
    native_broker_contract_path: std::path::PathBuf,
    file_access: file_access::FileAccessFixture,
    runtime_paths: persistent_runtime::RuntimeFixturePaths,
}

impl Fixture {
    fn create() -> Result<Self, String> {
        let temp = tempfile::Builder::new()
            .prefix("zephium-wk-web-extension-probe-")
            .tempdir()
            .map_err(|error| format!("cannot create extension probe directory: {error}"))?;
        let primary_path = temp.path().join("primary");
        let peer_path = temp.path().join("peer");
        let permission_path = temp.path().join("runtime-permission");
        std::fs::create_dir(&primary_path)
            .map_err(|error| format!("cannot create primary extension directory: {error}"))?;
        std::fs::create_dir(&peer_path)
            .map_err(|error| format!("cannot create peer extension directory: {error}"))?;

        write_primary_extension(&primary_path)?;
        write_peer_extension(&peer_path)?;
        permission_requests::write_fixture(&permission_path)?;
        let bitwarden_contract_path = bitwarden_contract::write_fixture(temp.path())?;
        let major_extension_contract_path = major_extension_contract::write_fixture(temp.path())?;
        let native_broker_contract_path = native_broker_contract::write_fixture(temp.path())?;
        let file_access = file_access::write_fixture(temp.path())?;
        let runtime_paths = persistent_runtime::write_runtime_extensions(temp.path())?;
        Ok(Self {
            _temp: temp,
            primary_path,
            peer_path,
            permission_path,
            bitwarden_contract_path,
            major_extension_contract_path,
            native_broker_contract_path,
            file_access,
            runtime_paths,
        })
    }
}

struct FixtureServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl FixtureServer {
    fn start(cross_origin: Option<SocketAddr>) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("cannot bind local fixture server: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot make fixture server nonblocking: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("cannot read fixture server address: {error}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("zephium-wk-extension-probe-http".into())
            .spawn(move || serve_fixture(listener, cross_origin, worker_stop))
            .map_err(|error| format!("cannot start fixture server: {error}"))?;
        Ok(Self {
            address,
            stop,
            worker: Some(worker),
        })
    }

    fn url(&self, path: &str, run: &str) -> String {
        format!("http://{}{}?run={run}", self.address, path)
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct ControllerBundle {
    _configuration: Retained<WKWebExtensionControllerConfiguration>,
    webview_configuration: Retained<WKWebViewConfiguration>,
    _data_store: Retained<WKWebsiteDataStore>,
    controller: Retained<WKWebExtensionController>,
}

struct ProbeTeardown {
    view: Weak<WKWebView>,
    extension_product_views: Vec<Weak<WKWebView>>,
    extension_product_stores: Vec<Weak<WKWebsiteDataStore>>,
    extension_ui_views: Vec<Weak<WKWebView>>,
    capability_views: Vec<Weak<WKWebView>>,
    capability_stores: Vec<Weak<WKWebsiteDataStore>>,
    controllers: Vec<Weak<WKWebExtensionController>>,
    contexts: Vec<Weak<WKWebExtensionContext>>,
    expected_contexts: usize,
    expected_extension_ui_views: usize,
    lifecycle_drops: Arc<AtomicUsize>,
    baseline_script_count: usize,
    peak_extension_script_delta: usize,
    webview_request_count: usize,
    persistent_all_type_removal_callbacks: usize,
    persistent_controllers_released: usize,
    persistent_contexts_released: usize,
    persistent_stores_released: usize,
    profile_views: Vec<Weak<WKWebView>>,
    profile_contexts: Vec<Weak<WKWebExtensionContext>>,
    profile_controllers: Vec<Weak<WKWebExtensionController>>,
    profile_stores: Vec<Weak<WKWebsiteDataStore>>,
    profile_lifecycle_drops: Vec<Arc<AtomicUsize>>,
    bitwarden_web_request_observation: &'static str,
    bitwarden_dynamic_resource_url: &'static str,
    bitwarden_execution_world_namespace: &'static str,
    bitwarden_sandbox_isolation: &'static str,
    bitwarden_runtime_port_early_connect: &'static str,
    bitwarden_tabs_same_document_observation: &'static str,
    major_extension_native_permissions: Box<str>,
    major_extension_namespaces: Box<str>,
    native_broker_port: Weak<objc2_web_kit::WKWebExtensionMessagePort>,
    native_broker_delegate_drops: Arc<AtomicUsize>,
    native_broker_surface_drops: Arc<AtomicUsize>,
    runtime_permission_status: &'static str,
    runtime_permission_readback: String,
    runtime_permission_callbacks_coalesced_before_settlement: bool,
    runtime_permission_replacement_settlement_stranded: bool,
    operating_system: String,
}

pub(crate) fn run_web_extension_probe() -> Result<bool, String> {
    run_web_extension_probe_with_permissions(RuntimePermissionProbeMode::None)
}

pub(crate) fn run_web_extension_permission_probe() -> Result<bool, String> {
    run_web_extension_probe_with_permissions(RuntimePermissionProbeMode::Full)
}

pub(crate) fn run_web_extension_permission_callback_cohort_probe() -> Result<bool, String> {
    run_web_extension_probe_with_permissions(RuntimePermissionProbeMode::CallbackCohort)
}

pub(crate) fn run_web_extension_permission_replacement_settlement_probe() -> Result<bool, String> {
    run_web_extension_probe_with_permissions(RuntimePermissionProbeMode::ReplacementSettlement)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RuntimePermissionProbeMode {
    None,
    Full,
    CallbackCohort,
    ReplacementSettlement,
}

impl RuntimePermissionProbeMode {
    const fn is_interactive(self) -> bool {
        !matches!(self, Self::None)
    }
}

fn run_web_extension_probe_with_permissions(
    permission_mode: RuntimePermissionProbeMode,
) -> Result<bool, String> {
    let Some(operating_system) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog_completed =
        arm_process_watchdog_with_timeout(if permission_mode.is_interactive() {
            Duration::from_secs(600)
        } else {
            PROCESS_WATCHDOG_TIMEOUT
        });
    let result = (|| {
        set_phase("live-probe");
        let teardown =
            objc2::rc::autoreleasepool(|_| run_supported_probe(operating_system, permission_mode))?;
        set_phase("teardown-wait");
        wait_for_teardown(&teardown)?;
        println!(
            "native-probe: macOS WKWebExtension passed; os={}; mv3=temp-directory; controller_before_wry=passed; default_deny=passed; exact_native_grant_replace_readback=passed; exact_native_grant_live_revocation=passed; exact_native_grant_clear_readback=passed; exact_native_owner_lifecycle=passed; restart_controller_absence=passed; exact_host_grant=passed; exact_site_deny_override=passed; dynamic_file_access=unsupported; dynamic_file_grant_readback=accepted; dynamic_file_contexts=3; private_data_default_deny=passed; private_data_explicit_grant=passed; private_data_separation=passed; runtime_permission_request={}; runtime_permission_readback={}; runtime_permission_callbacks_coalesced_before_settlement={}; runtime_permission_replacement_settlement_stranded={}; document_start=passed; isolated_worlds=passed; external_page_and_peer_messaging_default_deny=passed; include_exclude=passed; all_frames=passed; match_about_blank=passed; match_origin_as_fallback=passed; exact_unload_reload=passed; peer_context=passed; nonpersistent_permission_separation=passed; protected_inventory=passed; product_profile_view_store_binding=passed; regular_cookie_isolation=passed; private_cookie_noninheritance=passed; regular_cookie_reconstruction=passed; regular_tab_routing_isolation=passed; browser_mutation_broker=passed; discarded_tab_native_view_refusal=passed; persistent_extension_storage_namespace_isolation=passed; persistent_local_zero_after_reopen=passed; private_extension_storage_noninheritance=passed; mv3_background_execution=passed; major_extension_native_permissions={}; major_extension_namespaces={}; native_broker_history_search=bounded-round-trip; native_broker_port=extension-to-host-only; native_broker_port_host_send=accepted-unobserved; native_broker_principal_binding=passed; native_broker_port_released=1; native_broker_delegate_released={}; bitwarden_web_request_background_registration=passed; bitwarden_web_request_observation={}; bitwarden_scripting_main_world=passed; bitwarden_execution_world_namespace={}; bitwarden_web_navigation_observation=passed; bitwarden_tabs_same_document_observation={}; bitwarden_alarms_lifecycle=passed; bitwarden_commands_readback=passed; bitwarden_commands_native_dispatch=passed; bitwarden_runtime_port_registered=round-trip; bitwarden_runtime_port_early_connect={}; bitwarden_context_menus_lifecycle=passed; bitwarden_context_menus_native_projection=passed; bitwarden_dynamic_resource_execution=passed; bitwarden_dynamic_resource_url={}; bitwarden_sandbox_isolation={}; bitwarden_action_popup_native_lifecycle=passed; bitwarden_http_basic_auth_autofill=degraded; extension_product_views_released={}; extension_product_stores_released={}; extension_ui_views_released={}; capability_views_released={}; capability_stores_released={}; all_type_removal_callbacks_completed={}; baseline_controller_scripts={}; peak_extension_script_delta={}; webview_callbacks={}; protected_scripts=3; lifecycle_objects_released=3; ordinary_native_controllers_released={}; ordinary_native_contexts_released={}; persistent_native_controllers_released={}; persistent_native_contexts_released={}; persistent_native_stores_released={}; profile_views_released={}; profile_contexts_released={}; profile_controllers_released={}; profile_stores_released={}; profile_lifecycle_objects_released={}",
            teardown.operating_system,
            teardown.runtime_permission_status,
            teardown.runtime_permission_readback,
            teardown.runtime_permission_callbacks_coalesced_before_settlement,
            teardown.runtime_permission_replacement_settlement_stranded,
            teardown.major_extension_native_permissions,
            teardown.major_extension_namespaces,
            teardown.native_broker_delegate_drops.load(Ordering::Acquire),
            teardown.bitwarden_web_request_observation,
            teardown.bitwarden_execution_world_namespace,
            teardown.bitwarden_tabs_same_document_observation,
            teardown.bitwarden_runtime_port_early_connect,
            teardown.bitwarden_dynamic_resource_url,
            teardown.bitwarden_sandbox_isolation,
            teardown.extension_product_views.len(),
            teardown.extension_product_stores.len(),
            teardown.extension_ui_views.len(),
            teardown.capability_views.len(),
            teardown.capability_stores.len(),
            teardown.persistent_all_type_removal_callbacks,
            teardown.baseline_script_count,
            teardown.peak_extension_script_delta,
            teardown.webview_request_count,
            teardown.controllers.len(),
            teardown.contexts.len(),
            teardown.persistent_controllers_released,
            teardown.persistent_contexts_released,
            teardown.persistent_stores_released,
            teardown.profile_views.len(),
            teardown.profile_contexts.len(),
            teardown.profile_controllers.len(),
            teardown.profile_stores.len(),
            teardown
                .profile_lifecycle_drops
                .iter()
                .map(|drops| drops.load(Ordering::Acquire))
                .sum::<usize>(),
        );
        Ok(true)
    })();
    watchdog_completed.store(true, Ordering::Release);
    result
}

pub(crate) fn run_bitwarden_core_probe(artifact: &Path) -> Result<bool, String> {
    bitwarden_core_artifact::run(artifact)
}

pub(crate) fn run_stock_password_manager_probe(
    extension: &Path,
    tree_index: &Path,
    mode: crate::MacosStockPasswordManagerProbeMode,
) -> Result<bool, String> {
    stock_password_manager::run(extension, tree_index, mode)
}

pub(crate) fn run_stock_password_manager_compatibility_artifact_probe(
    artifact: &Path,
) -> Result<bool, String> {
    stock_password_manager::run_compatibility_artifact(artifact)
}

pub(crate) fn run_extension_compatibility_fixture_probe(artifact: &Path) -> Result<bool, String> {
    compatibility_fixture::run(artifact)
}

pub(crate) fn run_vimium_compatibility_artifact_probe(artifact: &Path) -> Result<bool, String> {
    vimium_contract::run(artifact)
}

pub(crate) fn run_representative_extension_probe(artifact: &Path) -> Result<bool, String> {
    representative_extension::run_compatibility(artifact)
}

pub(crate) fn run_representative_stock_extension_probe(
    extension: &Path,
    tree_index: &Path,
) -> Result<bool, String> {
    representative_extension::run_stock(extension, tree_index)
}

pub(crate) fn run_resource_transport_probe() -> Result<bool, String> {
    let Some(operating_system) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog_completed = arm_process_watchdog();
    let result = resource_transport::run(operating_system);
    watchdog_completed.store(true, Ordering::Release);
    result.map(|()| true)
}

fn set_phase(phase: &'static str) {
    if let Ok(mut current) = PROBE_PHASE.lock() {
        *current = phase;
    }
    eprintln!("native-probe-phase: {phase}");
}

fn arm_process_watchdog() -> Arc<AtomicBool> {
    arm_process_watchdog_with_timeout(PROCESS_WATCHDOG_TIMEOUT)
}

fn arm_process_watchdog_with_timeout(timeout: Duration) -> Arc<AtomicBool> {
    let completed = Arc::new(AtomicBool::new(false));
    let watchdog_completed = completed.clone();
    thread::spawn(move || {
        thread::sleep(timeout);
        if watchdog_completed.load(Ordering::Acquire) {
            return;
        }
        let phase = PROBE_PHASE
            .lock()
            .map(|phase| *phase)
            .unwrap_or("poisoned-phase-state");
        eprintln!(
            "macOS WKWebExtension probe watchdog expired after {}s in phase {phase}",
            PROCESS_WATCHDOG_TIMEOUT.as_secs()
        );
        std::process::exit(124);
    });
    completed
}

fn supported_runtime() -> Result<Option<String>, String> {
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    if version.majorVersion < 0 || version.minorVersion < 0 || version.patchVersion < 0 {
        return Err("NSProcessInfo returned a negative macOS version component".into());
    }
    let operating_system = format!(
        "{}.{}.{}",
        version.majorVersion, version.minorVersion, version.patchVersion
    );
    if version.majorVersion < MINIMUM_MACOS_MAJOR
        || (version.majorVersion == MINIMUM_MACOS_MAJOR
            && version.minorVersion < MINIMUM_MACOS_MINOR)
    {
        return Ok(None);
    }

    for class_name in [
        "WKWebExtension",
        "WKWebExtensionContext",
        "WKWebExtensionController",
        "WKWebExtensionControllerConfiguration",
    ] {
        if objc2_foundation::NSClassFromString(&NSString::from_str(class_name)).is_none() {
            return Err(format!(
                "macOS {operating_system} is missing public WebKit class {class_name}"
            ));
        }
    }
    Ok(Some(operating_system))
}

fn run_supported_probe(
    operating_system: String,
    permission_mode: RuntimePermissionProbeMode,
) -> Result<ProbeTeardown, String> {
    set_phase("appkit-launch");
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "WKWebExtension probe must run on the process main thread".to_owned())?;
    let app = NSApplication::sharedApplication(mtm);
    let activation_policy = if permission_mode.is_interactive() {
        // Keep the user-gesture gate visible to accessibility automation and
        // ordinary app switching. Unattended probes remain accessory apps and
        // do not perturb the developer's foreground application.
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    let _ = app.setActivationPolicy(activation_policy);
    app.finishLaunching();
    let run_loop = NSRunLoop::mainRunLoop();

    set_phase("fixture-setup");
    let fixture = Fixture::create()?;
    let cross_server = FixtureServer::start(None)?;
    let primary_server = FixtureServer::start(Some(cross_server.address))?;
    set_phase("primary-extension-parse");
    let primary_extension = load_extension(&fixture.primary_path, &run_loop, mtm)?;
    set_phase("peer-extension-parse");
    let peer_extension = load_extension(&fixture.peer_path, &run_loop, mtm)?;
    set_phase("runtime-permission-extension-parse");
    let permission_extension = load_extension(&fixture.permission_path, &run_loop, mtm)?;
    permission_requests::validate_declaration(&permission_extension)?;
    set_phase("bitwarden-contract-parse");
    let bitwarden_contract_extension =
        load_extension(&fixture.bitwarden_contract_path, &run_loop, mtm)?;
    let bitwarden_contract_evidence = bitwarden_contract::inspect(&bitwarden_contract_extension)?;
    eprintln!(
        "native-probe-bitwarden-contract: errors={}; required={:?}; optional={:?}; requested_hosts={:?}; all_requested_matches={:?}",
        bitwarden_contract_evidence.error_count,
        bitwarden_contract_evidence.requested_permissions,
        bitwarden_contract_evidence.optional_permissions,
        bitwarden_contract_evidence.requested_host_patterns,
        bitwarden_contract_evidence.all_requested_match_patterns,
    );
    set_phase("bitwarden-contract-native-grants");
    let bitwarden_contract_teardown = bitwarden_contract::validate_native_grant_round_trip(
        &bitwarden_contract_extension,
        &primary_server.url("/frame/same", "bitwarden-runtime"),
        &run_loop,
        mtm,
    )?;
    drop(bitwarden_contract_extension);
    set_phase("major-extension-contract-parse");
    let major_extension = load_extension(&fixture.major_extension_contract_path, &run_loop, mtm)?;
    set_phase("major-extension-contract-runtime");
    let major_extension_teardown = major_extension_contract::run(&major_extension, &run_loop, mtm)?;
    set_phase("native-broker-contract-parse");
    let native_broker_extension =
        load_extension(&fixture.native_broker_contract_path, &run_loop, mtm)?;
    set_phase("file-access-extension-parse");
    let file_access_extension =
        load_extension(&fixture.file_access.extension_root, &run_loop, mtm)?;
    set_phase("native-broker-contract-runtime");
    let native_broker_teardown =
        native_broker_contract::run(&native_broker_extension, &run_loop, mtm)?;
    set_phase("runtime-extension-parse");
    let runtime_writer = load_extension(&fixture.runtime_paths.writer, &run_loop, mtm)?;
    let runtime_verifier_one = load_extension(&fixture.runtime_paths.verifier_one, &run_loop, mtm)?;
    let runtime_verifier_two = load_extension(&fixture.runtime_paths.verifier_two, &run_loop, mtm)?;
    let runtime_empty = load_extension(&fixture.runtime_paths.empty, &run_loop, mtm)?;
    validate_extension(&primary_extension, "primary")?;
    validate_extension(&peer_extension, "peer")?;
    validate_extension(&file_access_extension, "dynamic file access")?;
    for runtime_extension in [
        &runtime_writer,
        &runtime_verifier_one,
        &runtime_verifier_two,
        &runtime_empty,
    ] {
        persistent_runtime::validate_runtime_extension(runtime_extension)?;
    }

    set_phase("profile-isolation-gates");
    let profile_isolation_evidence = profile_isolation::validate_profile_isolation(
        &run_loop,
        mtm,
        &primary_extension,
        &runtime_writer,
        &runtime_empty,
    )?;
    set_phase("persistent-runtime-gates");
    let persistent_evidence = persistent_runtime::validate_persistent_runtime_gates(
        &runtime_writer,
        &runtime_verifier_one,
        &runtime_verifier_two,
        &runtime_empty,
        &run_loop,
        mtm,
    )?;

    set_phase("controller-construction");
    let primary_bundle = new_nonpersistent_controller(mtm)?;
    let secondary_bundle = new_nonpersistent_controller(mtm)?;
    let controller_weaks = vec![
        bitwarden_contract_teardown.controller,
        major_extension_teardown.controller.clone(),
        native_broker_teardown.controller.clone(),
        Weak::from_retained(&primary_bundle.controller),
        Weak::from_retained(&secondary_bundle.controller),
    ];

    // SAFETY: the controller and configuration are main-thread-only retained
    // objects. The controller is attached before Wry constructs WKWebView.
    unsafe {
        primary_bundle
            .webview_configuration
            .setWebExtensionController(Some(&primary_bundle.controller));
    }

    let primary_context = new_context(&primary_extension, PRIMARY_CONTEXT_IDENTIFIER)?;
    let peer_context = new_context(&peer_extension, "zephium-probe-peer")?;
    let secondary_context = new_context(&primary_extension, "zephium-probe-secondary")?;
    assert_private_data_access(&primary_context, false, "primary default")?;
    assert_private_data_access(&peer_context, false, "peer default")?;
    assert_private_data_access(&secondary_context, false, "secondary default")?;

    // This content-injection surface intentionally uses a nonpersistent store.
    // Grant private-data access only to the two contexts under test and prove
    // the independent controller does not inherit that user decision. Regular
    // profile/store isolation is exercised separately above.
    unsafe {
        primary_context.setHasAccessToPrivateData(true);
        peer_context.setHasAccessToPrivateData(true);
    }
    assert_private_data_access(&primary_context, true, "primary explicit grant")?;
    assert_private_data_access(&peer_context, true, "peer explicit grant")?;
    assert_private_data_access(&secondary_context, false, "secondary separation")?;
    let mut context_weaks = vec![
        bitwarden_contract_teardown.context,
        major_extension_teardown.context.clone(),
        native_broker_teardown.context.clone(),
        Weak::from_retained(&primary_context),
        Weak::from_retained(&peer_context),
        Weak::from_retained(&secondary_context),
    ];

    // A WKWebView receives extension content wiring from the controller in
    // its immutable construction configuration. Load contexts before Wry
    // creates that view; the host publishes its tab/window identity only
    // after the native view exists.
    set_phase("primary-context-load");
    load_context(&primary_bundle.controller, &primary_context, "primary")?;
    set_phase("peer-context-load");
    load_context(&primary_bundle.controller, &peer_context, "peer")?;
    set_phase("secondary-context-load");
    load_context(
        &secondary_bundle.controller,
        &secondary_context,
        "secondary",
    )?;

    set_phase("wry-construction");
    let window = new_window(mtm)?;
    let host = ProbeHostView {
        view: window
            .contentView()
            .ok_or_else(|| "WKWebExtension probe window has no content view".to_owned())?,
    };
    let state = Rc::new(RefCell::new(ProbeState::default()));
    let ipc_state = state.clone();
    let protected_specs = crate::host::protected_script_specs_for_native_probe();
    let mut builder = wry::WebViewBuilder::new()
        .with_webview_configuration(primary_bundle.webview_configuration.clone())
        .with_ipc_handler(move |request| record_page_batch(&ipc_state, request.body()));
    for (source, all_frames) in protected_specs {
        builder = builder.with_initialization_script_for_main_only(source, !all_frames);
    }
    let view = builder
        .build_as_child(&host)
        .map_err(|error| format!("cannot construct WKWebExtension Wry view: {error}"))?;
    set_phase("wry-constructed");
    window.orderFrontRegardless();

    let native_view = super::native::webkit(&view);
    let view_weak = Weak::from_retained(&native_view);
    assert_attached_controller(&native_view, &primary_bundle.controller)?;
    let baseline_inventory = user_script_inventory(&native_view);
    validate_protected_inventory(&baseline_inventory)?;

    let lifecycle_drops = Arc::new(AtomicUsize::new(0));
    let webview_requests = Arc::new(AtomicUsize::new(0));
    let tab = ProbeTab::new(
        mtm,
        native_view.clone(),
        webview_requests.clone(),
        lifecycle_drops.clone(),
    );
    let extension_window = ProbeWindow::new(mtm, tab.clone(), true, lifecycle_drops.clone());
    tab.set_window(&extension_window);
    let permission_request_state = Rc::new(permission_requests::PermissionRequestProbe::default());
    let delegate = ProbeControllerDelegate::new_with_permission_requests(
        mtm,
        extension_window.clone(),
        permission_request_state.clone(),
        lifecycle_drops.clone(),
    );

    let delegate_protocol = ProtocolObject::from_ref(&*delegate);
    let window_protocol = ProtocolObject::from_ref(&*extension_window);
    let tab_protocol = ProtocolObject::from_ref(&*tab);
    // SAFETY: all native values are retained on the main thread. Contexts are
    // intentionally loaded with no windows first, then the exact Wry surface
    // is published through balanced host lifecycle notifications.
    set_phase("controller-set-delegate");
    unsafe {
        primary_bundle
            .controller
            .setDelegate(Some(delegate_protocol));
    }
    set_phase("controller-did-open-window");
    unsafe {
        primary_bundle.controller.didOpenWindow(window_protocol);
    }
    set_phase("controller-did-open-tab");
    unsafe {
        primary_bundle.controller.didOpenTab(tab_protocol);
    }
    set_phase("controller-did-focus-window");
    unsafe {
        primary_bundle
            .controller
            .didFocusWindow(Some(window_protocol));
    }
    set_phase("controller-did-activate-tab");
    unsafe {
        primary_bundle
            .controller
            .didActivateTab_previousActiveTab(tab_protocol, None);
    }
    set_phase("controller-surface-published");
    assert_context_surface(
        &primary_context,
        window_protocol,
        tab_protocol,
        true,
        "primary published surface",
    )?;
    assert_context_surface(
        &peer_context,
        window_protocol,
        tab_protocol,
        true,
        "peer published surface",
    )?;
    assert_context_surface(
        &secondary_context,
        window_protocol,
        tab_protocol,
        false,
        "secondary controller separation",
    )?;
    assert_exact_inventory(&native_view, &baseline_inventory, "context load")?;
    set_phase("dynamic-file-access-gates");
    let file_access_evidence = file_access::run(
        &file_access_extension,
        &primary_bundle.controller,
        &native_view,
        &fixture.file_access,
        &run_loop,
        mtm,
    )?;
    context_weaks.extend(file_access_evidence.contexts.iter().cloned());
    assert_exact_inventory(
        &native_view,
        &baseline_inventory,
        "dynamic file-access cleanup",
    )?;

    set_phase("runtime-permission-request-gates");
    let permission_request_evidence = if permission_mode.is_interactive() {
        permission_requests::run(
            &permission_extension,
            &primary_bundle.controller,
            window_protocol,
            tab_protocol,
            &run_loop,
            &permission_request_state,
            match permission_mode {
                RuntimePermissionProbeMode::Full => {
                    permission_requests::PermissionRequestRunMode::Full
                }
                RuntimePermissionProbeMode::CallbackCohort => {
                    permission_requests::PermissionRequestRunMode::CallbackCohort
                }
                RuntimePermissionProbeMode::ReplacementSettlement => {
                    permission_requests::PermissionRequestRunMode::ReplacementSettlement
                }
                RuntimePermissionProbeMode::None => unreachable!("interactive mode checked above"),
            },
        )?
    } else {
        permission_requests::PermissionRequestEvidence {
            contexts: Vec::new(),
            extension_views: Vec::new(),
            readback: "interactive-not-run".to_owned(),
            callbacks_coalesced_before_settlement: false,
            replacement_settlement_stranded: false,
        }
    };
    let runtime_permission_contexts = permission_request_evidence.contexts.len();
    let runtime_permission_views = permission_request_evidence.extension_views.len();
    context_weaks.extend(permission_request_evidence.contexts.iter().cloned());

    let match_pattern = exact_host_pattern(mtm)?;
    validate_requested_patterns(&primary_extension, "primary")?;
    validate_requested_patterns(&peer_extension, "peer")?;
    let admitted_url = native_url(&primary_server.url("/main", "permission-check"))?;
    assert_not_granted(&primary_context, &match_pattern, "primary default")?;
    assert_not_granted(&peer_context, &match_pattern, "peer default")?;
    assert_not_granted(&secondary_context, &match_pattern, "secondary default")?;
    assert_context_access(&primary_context, &admitted_url, false, "primary default")?;

    set_phase("default-deny-navigation");
    navigate_and_validate(
        &view,
        &state,
        &run_loop,
        &primary_server.url("/main", "default-deny"),
        "default-deny",
        ExpectedExtensions::None,
    )?;

    // Product grants are complete native replacements. WebKit applies these
    // dictionary mutations synchronously on the main thread; exact readback
    // closes the mutation before the next page navigation can observe it.
    let primary_applied_grants = super::extensions::apply_probe_grants(
        &primary_context,
        &[super::extensions::MacosNativeApiPermission::Storage],
        &[HOST_MATCH_PATTERN],
        true,
    )
    .map_err(|error| format!("primary native grant application failed: {error}"))?;
    let peer_applied_grants = super::extensions::apply_probe_grants(
        &peer_context,
        &[super::extensions::MacosNativeApiPermission::Storage],
        &[HOST_MATCH_PATTERN],
        true,
    )
    .map_err(|error| format!("peer native grant application failed: {error}"))?;
    assert_granted(&primary_context, &match_pattern, "primary grant")?;
    assert_granted(&peer_context, &match_pattern, "peer grant")?;
    assert_not_granted(
        &secondary_context,
        &match_pattern,
        "secondary separation after primary grants",
    )?;
    assert_context_access(&primary_context, &admitted_url, true, "primary granted")?;
    assert_context_access(&peer_context, &admitted_url, true, "peer granted")?;
    let wrong_scheme_url = native_url("https://127.0.0.1/outside-pattern")?;
    let wrong_host_url = native_url("http://localhost/outside-pattern")?;
    for (url, description) in [
        (&wrong_scheme_url, "primary wrong-scheme denial"),
        (&wrong_host_url, "primary wrong-host denial"),
    ] {
        assert_context_denied_outside_pattern(&primary_context, url, description)?;
    }
    for (url, description) in [
        (&wrong_scheme_url, "peer wrong-scheme denial"),
        (&wrong_host_url, "peer wrong-host denial"),
    ] {
        assert_context_denied_outside_pattern(&peer_context, url, description)?;
    }
    assert_context_access(
        &secondary_context,
        &admitted_url,
        false,
        "secondary separated",
    )?;
    if unsafe { secondary_context.grantedPermissionMatchPatterns().count() } != 0 {
        return Err("secondary nonpersistent controller inherited a primary grant".into());
    }

    set_phase("both-granted-navigation");
    navigate_and_validate(
        &view,
        &state,
        &run_loop,
        &primary_server.url("/main", "both-granted"),
        "both-granted",
        ExpectedExtensions::Both,
    )?;
    let both_delta =
        extension_script_delta(&native_view, &baseline_inventory, "both contexts granted")?;
    if both_delta.is_empty() {
        return Err("both loaded extension contexts produced no controller-owned scripts".into());
    }

    set_phase("primary-live-exact-site-denial");
    let primary_site_denied_grants = super::extensions::apply_probe_grants_with_denied_patterns(
        &primary_context,
        &[super::extensions::MacosNativeApiPermission::Storage],
        &[BROAD_HTTP_MATCH_PATTERN],
        &[HOST_MATCH_PATTERN],
        true,
    )
    .map_err(|error| format!("primary exact-site native denial failed: {error}"))?;
    drop(primary_applied_grants);
    let broad_pattern = native_match_pattern(mtm, BROAD_HTTP_MATCH_PATTERN)?;
    assert_implicitly_denied(
        &primary_context,
        &broad_pattern,
        "primary broad host status during exact-site denial",
    )?;
    assert_denied(
        &primary_context,
        &match_pattern,
        "primary exact-site denial",
    )?;
    assert_context_access(
        &primary_context,
        &admitted_url,
        false,
        "primary exact-site denial",
    )?;
    assert_url_permission(
        &primary_context,
        &wrong_host_url,
        true,
        "primary non-denied host during exact-site denial",
    )?;
    navigate_and_validate(
        &view,
        &state,
        &run_loop,
        &primary_server.url("/main", "primary-site-denied"),
        "primary-site-denied",
        ExpectedExtensions::PeerOnly,
    )?;

    set_phase("primary-live-exact-site-restoration");
    let primary_applied_grants = super::extensions::apply_probe_grants(
        &primary_context,
        &[super::extensions::MacosNativeApiPermission::Storage],
        &[HOST_MATCH_PATTERN],
        true,
    )
    .map_err(|error| format!("primary exact-site native restoration failed: {error}"))?;
    drop(primary_site_denied_grants);
    assert_granted(
        &primary_context,
        &match_pattern,
        "primary exact-site restoration",
    )?;
    assert_context_access(
        &primary_context,
        &admitted_url,
        true,
        "primary exact-site restoration",
    )?;
    navigate_and_validate(
        &view,
        &state,
        &run_loop,
        &primary_server.url("/main", "primary-site-restored"),
        "primary-site-restored",
        ExpectedExtensions::Both,
    )?;

    set_phase("primary-live-grant-revocation");
    let primary_revoked_grants = super::extensions::apply_probe_grants(
        &primary_context,
        &[super::extensions::MacosNativeApiPermission::Storage],
        &[],
        true,
    )
    .map_err(|error| format!("primary live native grant revocation failed: {error}"))?;
    drop(primary_applied_grants);
    assert_not_granted(
        &primary_context,
        &match_pattern,
        "primary live host revocation",
    )?;
    assert_context_access(
        &primary_context,
        &admitted_url,
        false,
        "primary live host revocation",
    )?;
    navigate_and_validate(
        &view,
        &state,
        &run_loop,
        &primary_server.url("/main", "primary-live-revoked"),
        "primary-live-revoked",
        ExpectedExtensions::PeerOnly,
    )?;

    set_phase("primary-live-grant-restoration");
    let primary_applied_grants = super::extensions::apply_probe_grants(
        &primary_context,
        &[super::extensions::MacosNativeApiPermission::Storage],
        &[HOST_MATCH_PATTERN],
        true,
    )
    .map_err(|error| format!("primary live native grant restoration failed: {error}"))?;
    drop(primary_revoked_grants);
    assert_granted(
        &primary_context,
        &match_pattern,
        "primary live host restoration",
    )?;
    assert_context_access(
        &primary_context,
        &admitted_url,
        true,
        "primary live host restoration",
    )?;
    navigate_and_validate(
        &view,
        &state,
        &run_loop,
        &primary_server.url("/main", "primary-live-restored"),
        "primary-live-restored",
        ExpectedExtensions::Both,
    )?;
    let live_restored_delta = extension_script_delta(
        &native_view,
        &baseline_inventory,
        "primary live host restoration",
    )?;
    if live_restored_delta != both_delta {
        return Err("live host-grant restoration changed controller-owned scripts".into());
    }

    set_phase("primary-context-unload");
    unload_context(&primary_bundle.controller, &primary_context, "primary")?;
    assert_context_surface(
        &primary_context,
        window_protocol,
        tab_protocol,
        false,
        "primary unloaded surface",
    )?;
    assert_context_surface(
        &peer_context,
        window_protocol,
        tab_protocol,
        true,
        "peer surviving surface",
    )?;
    set_phase("peer-only-navigation");
    navigate_and_validate(
        &view,
        &state,
        &run_loop,
        &primary_server.url("/main", "peer-only"),
        "peer-only",
        ExpectedExtensions::PeerOnly,
    )?;
    let peer_delta =
        extension_script_delta(&native_view, &baseline_inventory, "primary context unload")?;
    if peer_delta.is_empty() {
        return Err("peer context remained active without a controller-owned script delta".into());
    }
    if peer_delta == both_delta {
        return Err(
            "primary context unload did not remove its controller-owned script delta".into(),
        );
    }
    if !is_script_multiset_subset(&peer_delta, &both_delta) {
        return Err("primary context unload changed the surviving peer-owned script delta".into());
    }

    set_phase("primary-context-reload");
    load_context(
        &primary_bundle.controller,
        &primary_context,
        "primary reload",
    )?;
    assert_granted(
        &primary_context,
        &match_pattern,
        "primary retained grant after reload",
    )?;
    assert_context_surface(
        &primary_context,
        window_protocol,
        tab_protocol,
        true,
        "primary reloaded surface",
    )?;
    set_phase("primary-reloaded-navigation");
    navigate_and_validate(
        &view,
        &state,
        &run_loop,
        &primary_server.url("/main", "primary-reloaded"),
        "primary-reloaded",
        ExpectedExtensions::Both,
    )?;
    let reloaded_delta =
        extension_script_delta(&native_view, &baseline_inventory, "primary context reload")?;
    if reloaded_delta != both_delta {
        return Err(
            "primary context reload did not restore the exact controller-owned script delta".into(),
        );
    }

    validate_context_errors(&primary_context, "primary")?;
    validate_context_errors(&peer_context, "peer")?;
    validate_context_errors(&secondary_context, "secondary")?;

    // SAFETY: these close notifications balance the exact published tab and
    // window while both primary-controller contexts are still loaded.
    set_phase("controller-close-notifications");
    unsafe {
        primary_bundle.controller.didFocusWindow(None);
        primary_bundle
            .controller
            .didCloseTab_windowIsClosing(tab_protocol, true);
        primary_bundle.controller.didCloseWindow(window_protocol);
    }
    assert_context_surface(
        &primary_context,
        window_protocol,
        tab_protocol,
        false,
        "primary closed surface",
    )?;
    assert_context_surface(
        &peer_context,
        window_protocol,
        tab_protocol,
        false,
        "peer closed surface",
    )?;
    set_phase("context-cleanup");
    unload_context(
        &primary_bundle.controller,
        &primary_context,
        "primary cleanup",
    )?;
    unload_context(&primary_bundle.controller, &peer_context, "peer cleanup")?;
    unload_context(
        &secondary_bundle.controller,
        &secondary_context,
        "secondary cleanup",
    )?;
    primary_applied_grants
        .clear_and_verify(&primary_context)
        .map_err(|error| format!("primary native grant cleanup failed: {error}"))?;
    peer_applied_grants
        .clear_and_verify(&peer_context)
        .map_err(|error| format!("peer native grant cleanup failed: {error}"))?;

    // Exercise the product adapter's exact native-owner construction and
    // retirement seam, independently of the manual capability probes above.
    // The feature-only path supplies the already-authenticated fixture path;
    // ordinary product code can enter the same constructor only through its
    // move-only package-root lease.
    set_phase("native-owner-adapter-activation");
    let adapter_owner_id =
        zephium_extension_runtime_api::ExtensionRuntimeNativeOwnerId::parse_exact(
            "abcdefghijklmnopabcdefghijklmnop",
        )
        .map_err(|error| format!("invalid native-owner probe identity: {error}"))?;
    let adapter_result = Rc::new(RefCell::new(None));
    let callback_result = Rc::clone(&adapter_result);
    super::extensions::begin_probe_native_runtime_activation(
        &fixture.primary_path,
        Box::new([super::extensions::MacosNativeApiPermission::Storage]),
        Box::new([HOST_MATCH_PATTERN]),
        true,
        adapter_owner_id,
        primary_bundle.controller.clone(),
        move |settlement| {
            *callback_result.borrow_mut() = Some(Ok(settlement));
        },
    )
    .map_err(|error| format!("native-owner adapter could not start: {error}"))?;
    let mut adapter_owner = match wait_for_result(
        &adapter_result,
        &run_loop,
        "native-owner adapter activation",
    )? {
        super::extensions::MacosNativeRuntimeActivation::Activated(owner) => owner,
        super::extensions::MacosNativeRuntimeActivation::RejectedWithoutAbsenceProof(failure) => {
            return Err(format!(
                "native-owner adapter rejected activation without absence proof: {failure}"
            ));
        }
        super::extensions::MacosNativeRuntimeActivation::RejectedAfterCleanup {
            failure,
            owner_id: _,
            audit: _,
        } => {
            return Err(format!(
                "native-owner adapter rejected activation after exact cleanup: {failure}"
            ));
        }
        super::extensions::MacosNativeRuntimeActivation::OwnershipUncertain { failure, owner } => {
            // A failed admission probe must not run a possible owner's passive
            // destructor while claiming cleanup. Retain it until process exit.
            std::mem::forget(owner);
            return Err(format!(
                "native-owner adapter left activation ownership uncertain: {failure}"
            ));
        }
    };
    set_phase("native-owner-adapter-reconciliation");
    match adapter_owner.reconcile() {
        super::extensions::MacosNativeRuntimeReconciliation::Owned => {}
        super::extensions::MacosNativeRuntimeReconciliation::Absent(_) => {
            return Err(
                "native-owner adapter reported absence for its loaded retained owner".to_owned(),
            );
        }
        super::extensions::MacosNativeRuntimeReconciliation::StillUncertain(failure) => {
            std::mem::forget(adapter_owner);
            return Err(format!(
                "native-owner adapter could not reconcile its retained owner: {failure}"
            ));
        }
    }
    set_phase("native-owner-adapter-retirement");
    match adapter_owner.retire() {
        super::extensions::MacosNativeRuntimeRetirement::Absent(_audit) => {}
        super::extensions::MacosNativeRuntimeRetirement::Retained { failure, owner } => {
            std::mem::forget(owner);
            return Err(format!(
                "native-owner adapter could not prove retirement: {failure}"
            ));
        }
    }
    assert_exact_inventory(&native_view, &baseline_inventory, "final context unload")?;
    let webview_request_count = webview_requests.load(Ordering::Relaxed);
    if !(1..=MAX_WEBVIEW_CALLBACKS).contains(&webview_request_count) {
        return Err(format!(
            "webViewForWebExtensionContext callback count outside bound: {webview_request_count} (expected 1..={MAX_WEBVIEW_CALLBACKS})"
        ));
    }

    // SAFETY: all contexts are unloaded before severing the weak delegate.
    set_phase("delegate-clear");
    unsafe {
        primary_bundle.controller.setDelegate(None);
    }
    set_phase("native-drop");
    drop(delegate);
    drop(extension_window);
    drop(tab);
    drop(native_view);
    drop(view);
    window.close();
    drop(window);
    drop(primary_context);
    drop(peer_context);
    drop(secondary_context);
    drop(primary_extension);
    drop(peer_extension);
    drop(file_access_extension);
    drop(permission_extension);
    drop(permission_request_state);
    drop(runtime_writer);
    drop(runtime_verifier_one);
    drop(runtime_verifier_two);
    drop(runtime_empty);
    drop(major_extension);
    drop(native_broker_extension);
    drop(primary_bundle);
    drop(secondary_bundle);
    drop(primary_server);
    drop(cross_server);
    drop(fixture);

    set_phase("native-drop-complete");
    Ok(ProbeTeardown {
        view: view_weak,
        extension_product_views: vec![bitwarden_contract_teardown.product_view],
        extension_product_stores: vec![bitwarden_contract_teardown.product_store],
        extension_ui_views: bitwarden_contract_teardown
            .popup_views
            .into_iter()
            .chain(permission_request_evidence.extension_views)
            .collect(),
        capability_views: vec![major_extension_teardown.view, native_broker_teardown.view],
        capability_stores: vec![major_extension_teardown.store, native_broker_teardown.store],
        controllers: controller_weaks,
        contexts: context_weaks,
        expected_contexts: 6 + runtime_permission_contexts + file_access_evidence.dynamic_contexts,
        expected_extension_ui_views: 2 + runtime_permission_views,
        lifecycle_drops,
        baseline_script_count: baseline_inventory.len(),
        peak_extension_script_delta: script_multiset_len(&both_delta),
        webview_request_count,
        persistent_all_type_removal_callbacks: persistent_evidence.all_type_removal_callbacks,
        persistent_controllers_released: persistent_evidence.controllers_released,
        persistent_contexts_released: persistent_evidence.contexts_released,
        persistent_stores_released: persistent_evidence.stores_released,
        profile_views: profile_isolation_evidence.views,
        profile_contexts: profile_isolation_evidence.contexts,
        profile_controllers: profile_isolation_evidence.controllers,
        profile_stores: profile_isolation_evidence.stores,
        profile_lifecycle_drops: profile_isolation_evidence.lifecycle_drops,
        bitwarden_web_request_observation: bitwarden_contract_teardown.web_request_observation,
        bitwarden_dynamic_resource_url: bitwarden_contract_teardown.dynamic_resource_url,
        bitwarden_execution_world_namespace: bitwarden_contract_teardown.execution_world_namespace,
        bitwarden_sandbox_isolation: bitwarden_contract_teardown.sandbox_isolation,
        bitwarden_runtime_port_early_connect: bitwarden_contract_teardown
            .runtime_port_early_connect,
        bitwarden_tabs_same_document_observation: bitwarden_contract_teardown
            .tabs_same_document_observation,
        major_extension_native_permissions: major_extension_teardown
            .native_permissions
            .join(",")
            .into_boxed_str(),
        major_extension_namespaces: major_extension_teardown.namespace_summary,
        native_broker_port: native_broker_teardown.port,
        native_broker_delegate_drops: native_broker_teardown.delegate_drops,
        native_broker_surface_drops: native_broker_teardown.surface_drops,
        runtime_permission_status: match permission_mode {
            RuntimePermissionProbeMode::None => "interactive-not-run",
            RuntimePermissionProbeMode::Full => "passed",
            RuntimePermissionProbeMode::CallbackCohort => "callback-cohort-passed",
            RuntimePermissionProbeMode::ReplacementSettlement => "replacement-settlement-stranded",
        },
        runtime_permission_readback: permission_request_evidence.readback,
        runtime_permission_callbacks_coalesced_before_settlement: permission_request_evidence
            .callbacks_coalesced_before_settlement,
        runtime_permission_replacement_settlement_stranded: permission_request_evidence
            .replacement_settlement_stranded,
        operating_system,
    })
}

fn new_window(mtm: MainThreadMarker) -> Result<Retained<NSWindow>, String> {
    // SAFETY: mtm proves AppKit affinity; the returned retained window owns a
    // content view used by the probe until Wry teardown completes.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(720.0, 540.0)),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the Rust Retained owns the window through explicit close.
    unsafe { window.setReleasedWhenClosed(false) };
    Ok(window)
}

fn new_nonpersistent_controller(mtm: MainThreadMarker) -> Result<ControllerBundle, String> {
    let configuration =
        unsafe { WKWebExtensionControllerConfiguration::nonPersistentConfiguration(mtm) };
    if unsafe { configuration.isPersistent() } {
        return Err("WebKit returned a persistent extension-controller configuration".into());
    }
    let data_store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
    if unsafe { data_store.isPersistent() } {
        return Err("WebKit returned a persistent website data store for the probe".into());
    }
    let webview_configuration = unsafe { WKWebViewConfiguration::new(mtm) };
    // SAFETY: all objects are main-thread-only and retained for at least as
    // long as the copied controller configuration and constructed WebView.
    unsafe {
        webview_configuration.setWebsiteDataStore(&data_store);
        configuration.setWebViewConfiguration(Some(&webview_configuration));
        configuration.setDefaultWebsiteDataStore(Some(&data_store));
    }
    // SAFETY: this is the designated initializer for the fully configured
    // public controller object.
    let controller = unsafe {
        WKWebExtensionController::initWithConfiguration(
            WKWebExtensionController::alloc(mtm),
            &configuration,
        )
    };
    Ok(ControllerBundle {
        _configuration: configuration,
        webview_configuration,
        _data_store: data_store,
        controller,
    })
}

fn load_extension(
    path: &Path,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<Retained<WKWebExtension>, String> {
    let path = path
        .to_str()
        .ok_or_else(|| "extension fixture path is not valid UTF-8".to_owned())?;
    let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path), true);
    let result = Rc::new(RefCell::new(None));
    let callback_result = result.clone();
    let callback =
        block2::RcBlock::new(move |extension: *mut WKWebExtension, error: *mut NSError| {
            let value = if let Some(extension) = unsafe { Retained::retain(extension) } {
                Ok(extension)
            } else if let Some(error) = unsafe { Retained::retain(error) } {
                Err(format_native_error("extension parse", &error))
            } else {
                Err("WebKit returned neither an extension nor an error".into())
            };
            *callback_result.borrow_mut() = Some(value);
        });
    // SAFETY: the file URL and copied block remain retained for WebKit's
    // asynchronous initialization callback.
    unsafe {
        WKWebExtension::extensionWithResourceBaseURL_completionHandler(&url, &callback, mtm);
    }
    wait_for_result(&result, run_loop, "extension initialization")
}

fn wait_for_result<T>(
    result: &RefCell<Option<Result<T, String>>>,
    run_loop: &NSRunLoop,
    operation: &str,
) -> Result<T, String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        if let Some(result) = result.borrow_mut().take() {
            return result;
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out during {operation}"));
        }
        drain_run_loop_once(run_loop);
    }
}

fn validate_extension(extension: &WKWebExtension, name: &str) -> Result<(), String> {
    let errors = unsafe { extension.errors() };
    if errors.count() != 0 {
        return Err(format!(
            "{name} MV3 extension parsed with {} error(s): {}",
            errors.count(),
            describe_native_errors(&errors),
        ));
    }
    if unsafe { extension.manifestVersion() } != 3.0 {
        return Err(format!("{name} extension was not parsed as manifest v3"));
    }
    if !unsafe { extension.hasInjectedContent() } {
        return Err(format!("{name} extension has no injectable content"));
    }
    Ok(())
}

fn new_context(
    extension: &WKWebExtension,
    unique_identifier: &str,
) -> Result<Retained<WKWebExtensionContext>, String> {
    let context = unsafe { WKWebExtensionContext::contextForExtension(extension) };
    if unsafe { context.isLoaded() } {
        return Err("a new extension context was unexpectedly already loaded".into());
    }
    unsafe {
        context.setUniqueIdentifier(&NSString::from_str(unique_identifier));
        context.setInspectable(false);
    }
    Ok(context)
}

fn load_context(
    controller: &WKWebExtensionController,
    context: &WKWebExtensionContext,
    name: &str,
) -> Result<(), String> {
    unsafe { controller.loadExtensionContext_error(context) }
        .map_err(|error| format_native_error(&format!("load {name} context"), &error))?;
    if !unsafe { context.isLoaded() } {
        return Err(format!("{name} context returned success but is not loaded"));
    }
    Ok(())
}

fn assert_private_data_access(
    context: &WKWebExtensionContext,
    expected: bool,
    description: &str,
) -> Result<(), String> {
    let actual = unsafe { context.hasAccessToPrivateData() };
    if actual != expected {
        return Err(format!(
            "{description} private-data access mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

fn assert_context_surface(
    context: &WKWebExtensionContext,
    expected_window: &ProtocolObject<dyn WKWebExtensionWindow>,
    expected_tab: &ProtocolObject<dyn WKWebExtensionTab>,
    expected_visible: bool,
    description: &str,
) -> Result<(), String> {
    let windows = unsafe { context.openWindows() };
    let tabs = unsafe { context.openTabs() };
    let window_visible = windows.containsObject(expected_window);
    let tab_visible = tabs.containsObject(expected_tab);
    let expected_count = usize::from(expected_visible);
    if window_visible != expected_visible
        || tab_visible != expected_visible
        || windows.count() != expected_count
        || tabs.count() != expected_count
    {
        return Err(format!(
            "{description} mismatch: expected_visible={expected_visible}, windows={} (contains={window_visible}), tabs={} (contains={tab_visible})",
            windows.count(),
            tabs.count(),
        ));
    }
    Ok(())
}

fn unload_context(
    controller: &WKWebExtensionController,
    context: &WKWebExtensionContext,
    name: &str,
) -> Result<(), String> {
    unsafe { controller.unloadExtensionContext_error(context) }
        .map_err(|error| format_native_error(&format!("unload {name} context"), &error))?;
    if unsafe { context.isLoaded() } {
        return Err(format!(
            "{name} context returned success but remains loaded"
        ));
    }
    Ok(())
}

fn exact_host_pattern(
    mtm: MainThreadMarker,
) -> Result<Retained<WKWebExtensionMatchPattern>, String> {
    native_match_pattern(mtm, HOST_MATCH_PATTERN)
}

fn native_match_pattern(
    mtm: MainThreadMarker,
    pattern: &str,
) -> Result<Retained<WKWebExtensionMatchPattern>, String> {
    unsafe { WKWebExtensionMatchPattern::matchPatternWithString(&NSString::from_str(pattern), mtm) }
        .ok_or_else(|| format!("WebKit rejected host pattern {pattern}"))
}

fn validate_requested_patterns(extension: &WKWebExtension, name: &str) -> Result<(), String> {
    let patterns = unsafe { extension.allRequestedMatchPatterns() };
    let patterns = patterns.allObjects();
    let declared = (0..patterns.count())
        .map(|index| unsafe { patterns.objectAtIndex(index).string() }.to_string())
        .collect::<HashSet<_>>();
    if !declared.contains(HOST_MATCH_PATTERN) {
        return Err(format!(
            "{name} extension did not declare required host pattern {HOST_MATCH_PATTERN}; got {declared:?}"
        ));
    }
    Ok(())
}

fn native_url(url: &str) -> Result<Retained<NSURL>, String> {
    NSURL::URLWithString(&NSString::from_str(url))
        .ok_or_else(|| format!("WebKit rejected probe URL {url}"))
}

fn assert_context_access(
    context: &WKWebExtensionContext,
    url: &NSURL,
    expected: bool,
    description: &str,
) -> Result<(), String> {
    if !unsafe { context.hasInjectedContentForURL(url) } {
        return Err(format!(
            "{description} context does not recognize declared injectable content for URL {}",
            url.absoluteString()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<missing>".into())
        ));
    }
    let has_access = unsafe { context.hasAccessToURL(url) };
    let status = unsafe { context.permissionStatusForURL(url) };
    let status_granted = status == WKWebExtensionContextPermissionStatus::GrantedExplicitly
        || status == WKWebExtensionContextPermissionStatus::GrantedImplicitly;
    let current_pattern_count = unsafe { context.currentPermissionMatchPatterns() }.count();
    if has_access != expected || status_granted != expected {
        return Err(format!(
            "{description} effective URL access mismatch: expected={expected}, has_access={has_access}, status={status:?}, current_patterns={current_pattern_count}"
        ));
    }
    if expected && current_pattern_count == 0 {
        return Err(format!(
            "{description} reports URL access without a current permission match pattern"
        ));
    }
    Ok(())
}

fn assert_context_denied_outside_pattern(
    context: &WKWebExtensionContext,
    url: &NSURL,
    description: &str,
) -> Result<(), String> {
    let has_injected_content = unsafe { context.hasInjectedContentForURL(url) };
    let has_access = unsafe { context.hasAccessToURL(url) };
    let status = unsafe { context.permissionStatusForURL(url) };
    let status_granted = status == WKWebExtensionContextPermissionStatus::GrantedExplicitly
        || status == WKWebExtensionContextPermissionStatus::GrantedImplicitly;
    if has_injected_content || has_access || status_granted {
        return Err(format!(
            "{description} unexpectedly matched outside the exact host grant: injected_content={has_injected_content}, has_access={has_access}, status={status:?}"
        ));
    }
    Ok(())
}

fn assert_url_permission(
    context: &WKWebExtensionContext,
    url: &NSURL,
    expected: bool,
    description: &str,
) -> Result<(), String> {
    let has_access = unsafe { context.hasAccessToURL(url) };
    let status = unsafe { context.permissionStatusForURL(url) };
    let status_granted = is_granted_permission_status(status);
    if has_access != expected || status_granted != expected {
        return Err(format!(
            "{description} effective URL permission mismatch: expected={expected}, has_access={has_access}, status={status:?}"
        ));
    }
    Ok(())
}

fn is_granted_permission_status(status: WKWebExtensionContextPermissionStatus) -> bool {
    matches!(
        status,
        WKWebExtensionContextPermissionStatus::GrantedExplicitly
            | WKWebExtensionContextPermissionStatus::GrantedImplicitly
    )
}

fn assert_granted(
    context: &WKWebExtensionContext,
    pattern: &WKWebExtensionMatchPattern,
    description: &str,
) -> Result<(), String> {
    let status = unsafe { context.permissionStatusForMatchPattern(pattern) };
    if status != WKWebExtensionContextPermissionStatus::GrantedExplicitly
        && status != WKWebExtensionContextPermissionStatus::GrantedImplicitly
    {
        return Err(format!("{description} has non-granted status {status:?}"));
    }
    Ok(())
}

fn assert_not_granted(
    context: &WKWebExtensionContext,
    pattern: &WKWebExtensionMatchPattern,
    description: &str,
) -> Result<(), String> {
    let status = unsafe { context.permissionStatusForMatchPattern(pattern) };
    if status == WKWebExtensionContextPermissionStatus::GrantedExplicitly
        || status == WKWebExtensionContextPermissionStatus::GrantedImplicitly
    {
        return Err(format!("{description} unexpectedly has status {status:?}"));
    }
    Ok(())
}

fn assert_denied(
    context: &WKWebExtensionContext,
    pattern: &WKWebExtensionMatchPattern,
    description: &str,
) -> Result<(), String> {
    let status = unsafe { context.permissionStatusForMatchPattern(pattern) };
    if status != WKWebExtensionContextPermissionStatus::DeniedExplicitly {
        return Err(format!("{description} has non-denied status {status:?}"));
    }
    Ok(())
}

fn assert_implicitly_denied(
    context: &WKWebExtensionContext,
    pattern: &WKWebExtensionMatchPattern,
    description: &str,
) -> Result<(), String> {
    let status = unsafe { context.permissionStatusForMatchPattern(pattern) };
    if status != WKWebExtensionContextPermissionStatus::DeniedImplicitly {
        return Err(format!(
            "{description} has non-implicit-denial status {status:?}"
        ));
    }
    Ok(())
}

fn validate_context_errors(context: &WKWebExtensionContext, name: &str) -> Result<(), String> {
    let errors = unsafe { context.errors() };
    if errors.count() != 0 {
        return Err(format!(
            "{name} extension context accumulated {} runtime error(s): {}",
            errors.count(),
            describe_native_errors(&errors),
        ));
    }
    Ok(())
}

fn assert_attached_controller(
    view: &WKWebView,
    expected: &WKWebExtensionController,
) -> Result<(), String> {
    let configuration = unsafe { view.configuration() };
    let actual = unsafe { configuration.webExtensionController() }
        .ok_or_else(|| "Wry discarded the preattached WKWebExtensionController".to_owned())?;
    if Retained::as_ptr(&actual) != NonNull::from(expected).as_ptr() {
        return Err("Wry constructed WKWebView with a different extension controller".into());
    }
    Ok(())
}

fn user_script_inventory(view: &WKWebView) -> Vec<UserScriptFingerprint> {
    let controller = unsafe { view.configuration().userContentController() };
    user_content_controller_inventory(&controller)
}

fn user_content_controller_inventory(
    controller: &objc2_web_kit::WKUserContentController,
) -> Vec<UserScriptFingerprint> {
    let scripts = unsafe { controller.userScripts() };
    let count = scripts.count();
    let mut inventory = Vec::with_capacity(count);
    for index in 0..count {
        let script = scripts.objectAtIndex(index);
        inventory.push(UserScriptFingerprint {
            source: unsafe { script.source() }.to_string(),
            injection_time: unsafe { script.injectionTime() }.0,
            main_frame_only: unsafe { script.isForMainFrameOnly() },
        });
    }
    inventory
}

fn protected_inventory_is_installed(inventory: &[UserScriptFingerprint]) -> Result<bool, String> {
    let specs = crate::host::protected_script_specs_for_native_probe();
    let mut installed = 0;
    for (source, all_frames) in specs {
        let matching = inventory
            .iter()
            .filter(|entry| entry.source == source)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            continue;
        }
        if matching.len() != 1
            || matching[0].main_frame_only == all_frames
            || matching[0].injection_time != WKUserScriptInjectionTime::AtDocumentStart.0
        {
            return Err(
                "protected script is missing, duplicated, or has changed native installation metadata"
                    .into(),
            );
        }
        installed += 1;
    }
    if installed != 0 && installed != specs.len() {
        return Err(
            "protected script is missing, duplicated, or has changed native installation metadata"
                .into(),
        );
    }
    Ok(installed == specs.len())
}

fn validate_protected_inventory(inventory: &[UserScriptFingerprint]) -> Result<(), String> {
    if protected_inventory_is_installed(inventory)? {
        Ok(())
    } else {
        Err(
            "protected script is missing, duplicated, or has changed native installation metadata"
                .into(),
        )
    }
}

type ScriptMultiset = HashMap<UserScriptFingerprint, usize>;

fn script_multiset(inventory: &[UserScriptFingerprint]) -> ScriptMultiset {
    let mut counts = HashMap::with_capacity(inventory.len());
    for fingerprint in inventory {
        *counts.entry(fingerprint.clone()).or_default() += 1;
    }
    counts
}

fn script_multiset_len(multiset: &ScriptMultiset) -> usize {
    multiset.values().sum()
}

fn is_script_multiset_subset(candidate: &ScriptMultiset, parent: &ScriptMultiset) -> bool {
    candidate.iter().all(|(fingerprint, count)| {
        parent
            .get(fingerprint)
            .is_some_and(|parent| count <= parent)
    })
}

fn assert_exact_inventory(
    view: &WKWebView,
    expected: &[UserScriptFingerprint],
    operation: &str,
) -> Result<(), String> {
    let actual = user_script_inventory(view);
    validate_protected_inventory(&actual)?;
    if script_multiset(&actual) != script_multiset(expected) {
        return Err(format!(
            "WKUserContentController inventory changed during {operation}: expected {} scripts, got {}",
            expected.len(),
            actual.len()
        ));
    }
    Ok(())
}

fn extension_script_delta(
    view: &WKWebView,
    baseline: &[UserScriptFingerprint],
    operation: &str,
) -> Result<ScriptMultiset, String> {
    let actual = user_script_inventory(view);
    validate_protected_inventory(&actual)?;

    let baseline = script_multiset(baseline);
    let mut actual = script_multiset(&actual);
    for (fingerprint, expected_count) in baseline {
        let actual_count = actual.remove(&fingerprint).unwrap_or_default();
        if actual_count != expected_count {
            return Err(format!(
                "baseline WKUserContentController registration changed during {operation}: expected {expected_count} identical registration(s), got {actual_count}"
            ));
        }
    }

    let delta_len = script_multiset_len(&actual);
    if delta_len > MAX_EXTENSION_SCRIPT_DELTA {
        return Err(format!(
            "controller-owned script delta exceeded its bound during {operation}: {delta_len} > {MAX_EXTENSION_SCRIPT_DELTA}"
        ));
    }
    Ok(actual)
}

fn record_page_batch(state: &Rc<RefCell<ProbeState>>, body: &str) {
    if body.len() > IPC_BODY_LIMIT {
        state.borrow_mut().malformed_batch = Some("IPC body exceeded probe limit".into());
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        state.borrow_mut().malformed_batch = Some("page batch was not JSON".into());
        return;
    };
    if value.get("probe").and_then(Value::as_str) != Some(PROBE_TOKEN) {
        state.borrow_mut().malformed_batch = Some("page batch had wrong probe token".into());
        return;
    }
    let Some(run) = value.get("run").and_then(Value::as_str) else {
        state.borrow_mut().malformed_batch = Some("page batch had no run identifier".into());
        return;
    };
    if run.len() > 64 {
        state.borrow_mut().malformed_batch = Some("page run identifier exceeded limit".into());
        return;
    }
    let mut state = state.borrow_mut();
    if state.batches.insert(run.to_owned(), value).is_some() {
        state.duplicate_batch = true;
    }
}

fn navigate_and_validate(
    view: &wry::WebView,
    state: &Rc<RefCell<ProbeState>>,
    run_loop: &NSRunLoop,
    url: &str,
    run: &str,
    expected: ExpectedExtensions,
) -> Result<(), String> {
    {
        let mut state = state.borrow_mut();
        state.batches.remove(run);
        state.duplicate_batch = false;
        state.malformed_batch = None;
    }
    view.load_url(url)
        .map_err(|error| format!("cannot navigate extension probe to {url}: {error}"))?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let batch = loop {
        {
            let mut state = state.borrow_mut();
            if let Some(error) = state.malformed_batch.take() {
                return Err(format!("{run} received malformed page evidence: {error}"));
            }
            if state.duplicate_batch {
                return Err(format!("{run} received a duplicate page evidence batch"));
            }
            if let Some(batch) = state.batches.remove(run) {
                break batch;
            }
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for page evidence for {run}"));
        }
        drain_run_loop_once(run_loop);
    };
    validate_page_batch(&batch, run, expected)
}

fn validate_page_batch(
    batch: &Value,
    run: &str,
    expected: ExpectedExtensions,
) -> Result<(), String> {
    let early_main = batch
        .get("earlyMain")
        .ok_or_else(|| format!("{run} page evidence has no early main-frame report"))?;
    validate_frame_report(early_main, run, "main", expected, false)?;

    let reports = batch
        .get("reports")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{run} page evidence has no reports array"))?;
    if reports.len() != EXPECTED_ROLES.len() {
        return Err(format!(
            "{run} reported {}/{} frames: {reports:?}",
            reports.len(),
            EXPECTED_ROLES.len()
        ));
    }
    let mut seen = HashSet::with_capacity(reports.len());
    for report in reports {
        let role = report
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{run} frame report has no role: {report}"))?;
        if !EXPECTED_ROLES.contains(&role) || !seen.insert(role) {
            return Err(format!(
                "{run} has unexpected or duplicate frame role {role}"
            ));
        }
        validate_frame_report(report, run, role, expected, true)?;
    }
    Ok(())
}

fn validate_frame_report(
    report: &Value,
    run: &str,
    expected_role: &str,
    expected: ExpectedExtensions,
    final_report: bool,
) -> Result<(), String> {
    if report.get("role").and_then(Value::as_str) != Some(expected_role) {
        return Err(format!(
            "{run} expected a {expected_role} frame report, got {report}"
        ));
    }
    let (primary_direct, primary_fallback, peer) = expected_counts(expected_role, expected);
    for (field, expected_count) in [
        ("primaryDirect", primary_direct),
        ("primaryFallback", primary_fallback),
        ("peer", peer),
    ] {
        let actual = report.get(field).and_then(Value::as_u64);
        if actual != Some(expected_count) {
            return Err(format!(
                "{run}/{expected_role} expected {field}={expected_count}, got {actual:?}: {report}"
            ));
        }
    }
    if report.get("pageWorldLeak").and_then(Value::as_bool) != Some(false)
        || report.get("crossWorldLeak").and_then(Value::as_bool) != Some(false)
    {
        return Err(format!(
            "{run}/{expected_role} observed page/extension world leakage: {report}"
        ));
    }
    let external_attempt = report.get("externalAttempt").and_then(Value::as_str);
    let external_deliveries = report.get("externalDeliveries").and_then(Value::as_str);
    if expected_role == "main" && final_report && run == "default-deny" {
        if external_attempt != Some("message-and-port-issued") {
            return Err(format!(
                "{run}/{expected_role} did not issue both hostile external-messaging attempts: {report}"
            ));
        }
    } else if expected_role == "main" && final_report && run == "both-granted" {
        if external_deliveries != Some("0")
            || report.get("peerExternalAttempt").and_then(Value::as_str)
                != Some("message-and-port-issued")
        {
            return Err(format!(
                "{run}/{expected_role} did not prove zero page/peer external deliveries after exact attempts: {report}"
            ));
        }
    } else if final_report
        && (external_attempt.is_some()
            || external_deliveries.is_some()
            || report
                .get("peerExternalAttempt")
                .is_some_and(|value| !value.is_null()))
    {
        return Err(format!(
            "{run}/{expected_role} carried external-messaging evidence outside its exact phase: {report}"
        ));
    }
    Ok(())
}

fn expected_counts(role: &str, expected: ExpectedExtensions) -> (u64, u64, u64) {
    let primary = !matches!(
        expected,
        ExpectedExtensions::None | ExpectedExtensions::PeerOnly
    );
    let peer = !matches!(expected, ExpectedExtensions::None);
    if role == "excluded" {
        return (0, 0, 0);
    }
    if matches!(role, "srcdoc" | "data" | "blob") {
        return (0, u64::from(primary), 0);
    }
    (u64::from(primary), u64::from(primary), u64::from(peer))
}

fn wait_for_teardown(teardown: &ProbeTeardown) -> Result<(), String> {
    const EXPECTED_EXTENSION_PRODUCT_VIEWS: usize = 1;
    const EXPECTED_EXTENSION_PRODUCT_STORES: usize = 1;
    const EXPECTED_CAPABILITY_VIEWS: usize = 2;
    const EXPECTED_CAPABILITY_STORES: usize = 2;
    const EXPECTED_PROFILE_VIEWS: usize = 7;
    const EXPECTED_PROFILE_OWNERS: usize = 6;
    const EXPECTED_PROFILE_CONTEXTS: usize = 4;
    const EXPECTED_PROFILE_LIFECYCLE_GROUPS: usize =
        profile_isolation::EXPECTED_BROWSER_SURFACE_LIFECYCLE_DROPS.len();
    if teardown.extension_product_views.len() != EXPECTED_EXTENSION_PRODUCT_VIEWS
        || teardown.extension_product_stores.len() != EXPECTED_EXTENSION_PRODUCT_STORES
        || teardown.extension_ui_views.len() != teardown.expected_extension_ui_views
        || teardown.capability_views.len() != EXPECTED_CAPABILITY_VIEWS
        || teardown.capability_stores.len() != EXPECTED_CAPABILITY_STORES
        || teardown.controllers.len() != EXPECTED_NATIVE_CONTROLLERS
        || teardown.contexts.len() != teardown.expected_contexts
        || teardown.profile_views.len() != EXPECTED_PROFILE_VIEWS
        || teardown.profile_contexts.len() != EXPECTED_PROFILE_CONTEXTS
        || teardown.profile_controllers.len() != EXPECTED_PROFILE_OWNERS
        || teardown.profile_stores.len() != EXPECTED_PROFILE_OWNERS
        || teardown.profile_lifecycle_drops.len() != EXPECTED_PROFILE_LIFECYCLE_GROUPS
    {
        return Err(format!(
            "native teardown inventory mismatch: extension_product_views={}/{EXPECTED_EXTENSION_PRODUCT_VIEWS}, extension_product_stores={}/{EXPECTED_EXTENSION_PRODUCT_STORES}, extension_ui_views={}/{}, capability_views={}/{EXPECTED_CAPABILITY_VIEWS}, capability_stores={}/{EXPECTED_CAPABILITY_STORES}, controllers={}/{EXPECTED_NATIVE_CONTROLLERS}, contexts={}/{}, profile_views={}/{EXPECTED_PROFILE_VIEWS}, profile_contexts={}/{EXPECTED_PROFILE_CONTEXTS}, profile_controllers={}/{EXPECTED_PROFILE_OWNERS}, profile_stores={}/{EXPECTED_PROFILE_OWNERS}, profile_lifecycle_groups={}/{EXPECTED_PROFILE_LIFECYCLE_GROUPS}",
            teardown.extension_product_views.len(),
            teardown.extension_product_stores.len(),
            teardown.extension_ui_views.len(),
            teardown.expected_extension_ui_views,
            teardown.capability_views.len(),
            teardown.capability_stores.len(),
            teardown.controllers.len(),
            teardown.contexts.len(),
            teardown.expected_contexts,
            teardown.profile_views.len(),
            teardown.profile_contexts.len(),
            teardown.profile_controllers.len(),
            teardown.profile_stores.len(),
            teardown.profile_lifecycle_drops.len(),
        ));
    }
    let run_loop = NSRunLoop::mainRunLoop();
    let deadline = Instant::now() + TEARDOWN_TIMEOUT;
    loop {
        let controllers_released = teardown
            .controllers
            .iter()
            .all(|controller| controller.load().is_none());
        let extension_product_views_released = teardown
            .extension_product_views
            .iter()
            .all(|view| view.load().is_none());
        let extension_product_stores_released = teardown
            .extension_product_stores
            .iter()
            .all(|store| store.load().is_none());
        let extension_ui_views_released = teardown
            .extension_ui_views
            .iter()
            .all(|view| view.load().is_none());
        let capability_views_released = teardown
            .capability_views
            .iter()
            .all(|view| view.load().is_none());
        let capability_stores_released = teardown
            .capability_stores
            .iter()
            .all(|store| store.load().is_none());
        let contexts_released = teardown
            .contexts
            .iter()
            .all(|context| context.load().is_none());
        let profile_views_released = teardown
            .profile_views
            .iter()
            .all(|view| view.load().is_none());
        let profile_contexts_released = teardown
            .profile_contexts
            .iter()
            .all(|context| context.load().is_none());
        let profile_controllers_released = teardown
            .profile_controllers
            .iter()
            .all(|controller| controller.load().is_none());
        let profile_stores_released = teardown
            .profile_stores
            .iter()
            .all(|store| store.load().is_none());
        let profile_lifecycle_counts =
            profile_isolation::browser_surface_lifecycle_counts(&teardown.profile_lifecycle_drops);
        let profile_lifecycle_released = profile_lifecycle_counts.as_slice()
            == profile_isolation::EXPECTED_BROWSER_SURFACE_LIFECYCLE_DROPS;
        if teardown.view.load().is_none()
            && extension_product_views_released
            && extension_product_stores_released
            && extension_ui_views_released
            && capability_views_released
            && capability_stores_released
            && controllers_released
            && contexts_released
            && profile_views_released
            && profile_contexts_released
            && profile_controllers_released
            && profile_stores_released
            && profile_lifecycle_released
            && teardown.native_broker_port.load().is_none()
            && teardown
                .native_broker_delegate_drops
                .load(Ordering::Acquire)
                == 1
            && teardown.native_broker_surface_drops.load(Ordering::Acquire) == 2
            && teardown.lifecycle_drops.load(Ordering::Acquire) == 3
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "native teardown did not converge: view={}, extension_product_views_released={}/{}, extension_product_stores_released={}/{}, extension_ui_views_released={}/{}, capability_views_released={}/{}, capability_stores_released={}/{}, controllers_released={}/{}, contexts_released={}/{}, profile_views_released={}/{}, profile_contexts_released={}/{}, profile_controllers_released={}/{}, profile_stores_released={}/{}, native_broker_port_released={}, native_broker_delegate_drops={}/1, native_broker_surface_drops={}/2, lifecycle_drops={}/3, profile_lifecycle_drops={profile_lifecycle_counts:?}/{:?}",
                teardown.view.load().is_none(),
                teardown.extension_product_views.iter().filter(|view| view.load().is_none()).count(),
                teardown.extension_product_views.len(),
                teardown.extension_product_stores.iter().filter(|store| store.load().is_none()).count(),
                teardown.extension_product_stores.len(),
                teardown.extension_ui_views.iter().filter(|view| view.load().is_none()).count(),
                teardown.extension_ui_views.len(),
                teardown.capability_views.iter().filter(|view| view.load().is_none()).count(),
                teardown.capability_views.len(),
                teardown.capability_stores.iter().filter(|store| store.load().is_none()).count(),
                teardown.capability_stores.len(),
                teardown.controllers.iter().filter(|controller| controller.load().is_none()).count(),
                teardown.controllers.len(),
                teardown.contexts.iter().filter(|context| context.load().is_none()).count(),
                teardown.contexts.len(),
                teardown.profile_views.iter().filter(|view| view.load().is_none()).count(),
                teardown.profile_views.len(),
                teardown.profile_contexts.iter().filter(|context| context.load().is_none()).count(),
                teardown.profile_contexts.len(),
                teardown.profile_controllers.iter().filter(|controller| controller.load().is_none()).count(),
                teardown.profile_controllers.len(),
                teardown.profile_stores.iter().filter(|store| store.load().is_none()).count(),
                teardown.profile_stores.len(),
                teardown.native_broker_port.load().is_none(),
                teardown.native_broker_delegate_drops.load(Ordering::Acquire),
                teardown.native_broker_surface_drops.load(Ordering::Acquire),
                teardown.lifecycle_drops.load(Ordering::Acquire),
                profile_isolation::EXPECTED_BROWSER_SURFACE_LIFECYCLE_DROPS,
            ));
        }
        drain_run_loop_once(&run_loop);
    }
}

fn drain_run_loop_once(run_loop: &NSRunLoop) {
    objc2::rc::autoreleasepool(|_| {
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    });
}

fn format_native_error(operation: &str, error: &NSError) -> String {
    format!(
        "{operation} failed: domain={}, code={}, description={}",
        error.domain(),
        error.code(),
        error.localizedDescription()
    )
}

fn describe_native_errors(errors: &NSArray<NSError>) -> String {
    (0..errors.count())
        .map(|index| {
            let error = errors.objectAtIndex(index);
            format_native_error("WebKit", &error)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn write_primary_extension(path: &Path) -> Result<(), String> {
    let manifest = json!({
        "manifest_version": 3,
        "name": "Zephium WKWebExtension Primary Probe",
        "description": "Feature-gated native extension admission fixture.",
        "version": "1.0.0",
        "permissions": ["storage"],
        "host_permissions": [BROAD_HTTP_MATCH_PATTERN],
        "background": {
            "service_worker": "background.js"
        },
        "content_scripts": [
            {
                "matches": [HOST_MATCH_PATTERN],
                "exclude_matches": [EXCLUDED_MATCH_PATTERN],
                "js": ["direct.js"],
                "run_at": "document_start",
                "all_frames": true,
                "match_about_blank": true
            },
            {
                "matches": [HOST_MATCH_PATTERN],
                "exclude_matches": [EXCLUDED_MATCH_PATTERN],
                "js": ["fallback.js"],
                "run_at": "document_start",
                "all_frames": true,
                "match_origin_as_fallback": true
            }
        ]
    });
    write_fixture_file(path, "manifest.json", &manifest.to_string())?;
    write_fixture_file(
        path,
        "background.js",
        r#"const api = globalThis.browser ?? globalThis.chrome;
let externalDeliveries = 0;
api.runtime.onMessageExternal.addListener((message, _sender, sendResponse) => {
  if (message?.probe === "zephium-wk-web-extension-v1") {
    externalDeliveries += 1;
    sendResponse({ kind: "unexpected-external-message-accepted" });
  }
  return false;
});
api.runtime.onConnectExternal.addListener((port) => {
  externalDeliveries += 1;
  port.onMessage.addListener((message) => {
    if (message?.probe === "zephium-wk-web-extension-v1") {
      port.postMessage({ kind: "unexpected-external-port-accepted" });
    }
  });
});
api.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.probe === "zephium-wk-web-extension-v1" && message?.kind === "read-external-deliveries") {
    sendResponse({ count: externalDeliveries });
  }
  return false;
});"#,
    )?;
    let mut direct = content_script(
        "primary-direct",
        "data-zepx-primary-direct",
        "__zepxPrimaryDirect",
        &["__zepxPeer"],
    );
    direct.push_str(
        r#"
if (window === top && new URLSearchParams(location.search).get("run") === "both-granted") {
  const api = globalThis.browser ?? globalThis.chrome;
  setTimeout(() => {
    api.runtime.sendMessage(
      { probe: "zephium-wk-web-extension-v1", kind: "read-external-deliveries" },
      (response) => document.documentElement.setAttribute(
        "data-zepx-external-deliveries",
        Number.isSafeInteger(response?.count) ? String(response.count) : "invalid"
      )
    );
  }, 750);
}
"#,
    );
    write_fixture_file(path, "direct.js", &direct)?;
    write_fixture_file(
        path,
        "fallback.js",
        &content_script(
            "primary-fallback",
            "data-zepx-primary-fallback",
            "__zepxPrimaryFallback",
            &["__zepxPeer"],
        ),
    )
}

fn write_peer_extension(path: &Path) -> Result<(), String> {
    let manifest = json!({
        "manifest_version": 3,
        "name": "Zephium WKWebExtension Peer Probe",
        "description": "Feature-gated native peer-isolation fixture.",
        "version": "1.0.0",
        "permissions": ["storage"],
        "content_scripts": [{
            "matches": [HOST_MATCH_PATTERN],
            "exclude_matches": [EXCLUDED_MATCH_PATTERN],
            "js": ["peer.js"],
            "run_at": "document_start",
            "all_frames": true,
            "match_about_blank": true
        }]
    });
    write_fixture_file(path, "manifest.json", &manifest.to_string())?;
    let mut peer = content_script(
        "peer",
        "data-zepx-peer",
        "__zepxPeer",
        &["__zepxPrimaryDirect", "__zepxPrimaryFallback"],
    );
    peer.push_str(&format!(
        r#"
if (window === top && new URLSearchParams(location.search).get("run") === "both-granted") {{
  const api = globalThis.browser ?? globalThis.chrome;
  try {{
    const result = api.runtime.sendMessage(
      {primary_context:?},
      {{ probe: "zephium-wk-web-extension-v1", kind: "hostile-peer-message" }},
      () => undefined
    );
    if (typeof result?.then === "function") result.catch(() => undefined);
  }} catch {{}}
  try {{
    const port = api.runtime.connect(
      {primary_context:?},
      {{ name: "zephium-hostile-peer-port" }}
    );
    port.onMessage.addListener(() => undefined);
    port.onDisconnect.addListener(() => undefined);
    port.postMessage({{ probe: "zephium-wk-web-extension-v1", kind: "hostile-peer-port" }});
  }} catch {{}}
  document.documentElement.setAttribute(
    "data-zepx-peer-external-attempt",
    "message-and-port-issued"
  );
}}
"#,
        primary_context = PRIMARY_CONTEXT_IDENTIFIER,
    ));
    write_fixture_file(path, "peer.js", &peer)
}

fn content_script(
    marker: &str,
    attribute: &str,
    global: &str,
    forbidden_globals: &[&str],
) -> String {
    let forbidden = forbidden_globals
        .iter()
        .map(|name| format!("typeof globalThis.{name} !== 'undefined'"))
        .collect::<Vec<_>>()
        .join(" || ");
    format!(
        r#"(() => {{
            'use strict';
            const root = document.documentElement;
            if (!root) return;
            if ({forbidden}) root.setAttribute('data-zepx-cross-world-leak', '1');
            const current = Number(root.getAttribute('{attribute}') || '0');
            root.setAttribute('{attribute}', String(current + 1));
            Object.defineProperty(globalThis, '{global}', {{
                value: '{marker}', configurable: false, enumerable: false, writable: false
            }});
        }})();"#
    )
}

fn write_fixture_file(directory: &Path, name: &str, contents: &str) -> Result<(), String> {
    std::fs::write(directory.join(name), contents)
        .map_err(|error| format!("cannot write extension fixture {name}: {error}"))
}

fn serve_fixture(listener: TcpListener, cross_origin: Option<SocketAddr>, stop: Arc<AtomicBool>) {
    let mut handled = 0_usize;
    while !stop.load(Ordering::Acquire) && handled < MAX_HTTP_REQUESTS {
        match listener.accept() {
            Ok((mut stream, _)) => {
                handled += 1;
                let _ = respond_to_fixture_request(&mut stream, cross_origin);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
        }
    }
}

fn respond_to_fixture_request(
    stream: &mut TcpStream,
    cross_origin: Option<SocketAddr>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| format!("cannot bound fixture request read: {error}"))?;
    let mut request = [0_u8; HTTP_REQUEST_LIMIT];
    let read = stream
        .read(&mut request)
        .map_err(|error| format!("cannot read fixture request: {error}"))?;
    let request = std::str::from_utf8(&request[..read])
        .map_err(|_| "fixture request was not UTF-8".to_owned())?;
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| "fixture request has no target".to_owned())?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let run = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("run="))
        .filter(|run| !run.is_empty() && run.len() <= 64)
        .unwrap_or("missing-run");
    let body = match path {
        "/main" => {
            let cross_origin =
                cross_origin.ok_or_else(|| "main fixture server has no cross origin".to_owned())?;
            main_page(run, cross_origin)
        }
        "/keyboard" => keyboard_extension_page(run),
        "/activated" => keyboard_activated_page(run),
        "/login" => password_manager_login_page(run),
        "/theme" => representative_theme_page(run),
        "/theme/frame" => representative_theme_frame(run),
        "/frame/same" => child_page("same", run),
        "/frame/cross" => child_page("cross", run),
        "/excluded/frame" => child_page("excluded", run),
        _ => "<!doctype html><title>not found</title>".to_owned(),
    };
    if body.len() > HTTP_RESPONSE_LIMIT {
        return Err("fixture response exceeded bound".into());
    }
    let status = if path == "/main"
        || path == "/keyboard"
        || path == "/activated"
        || path == "/login"
        || path == "/theme"
        || path == "/theme/frame"
        || path == "/frame/same"
        || path == "/frame/cross"
        || path == "/excluded/frame"
    {
        "200 OK"
    } else {
        "404 Not Found"
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| format!("cannot write fixture response: {error}"))
}

fn keyboard_extension_page(run: &str) -> String {
    let run = serde_json::to_string(run).expect("bounded run identifier is serializable");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>keyboard extension fixture</title>
        <style>html,body{{margin:0}}main{{min-height:4000px;padding:24px}}a{{display:inline-block;padding:12px}}</style></head>
        <body><main><a id="target" href="/activated">Open the only target</a></main>
        <script>(() => {{
          'use strict';
          globalThis.__zephiumKeyboardExtensionRun = {run};
          const root = document.documentElement;
          const target = document.querySelector('#target');
          target.href = `/activated?run=${{encodeURIComponent(globalThis.__zephiumKeyboardExtensionRun)}}`;
          const pageApi = globalThis.browser ?? globalThis.chrome;
          root.setAttribute(
            'data-zephium-page-privileged-extension-api',
            pageApi?.runtime?.id != null
                || typeof pageApi?.runtime?.getManifest === 'function'
                || pageApi?.storage?.local != null
                || pageApi?.tabs != null
                || pageApi?.webNavigation != null
              ? 'present'
              : 'absent',
          );
          root.setAttribute(
            'data-zephium-page-adapter',
            globalThis[Symbol.for('zephium.webkit-api-compatibility.v1')] === true
              ? 'present'
              : 'absent',
          );
          root.setAttribute('data-zephium-keyboard-link-click', 'pending');
          root.setAttribute('data-zephium-keyboard-last-key', 'pending');
          addEventListener('keydown', (event) => {{
            root.setAttribute(
              'data-zephium-keyboard-last-key',
              `${{event.key}}:${{event.isTrusted ? 'trusted' : 'untrusted'}}`,
            );
          }}, true);
          target.addEventListener('click', (event) => {{
            sessionStorage.setItem(
              'zephium-keyboard-link-click',
              event.isTrusted ? 'trusted' : 'untrusted',
            );
          }});
        }})()</script></body></html>"#
    )
}

fn keyboard_activated_page(run: &str) -> String {
    let run = serde_json::to_string(run).expect("bounded run identifier is serializable");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>keyboard extension target</title></head>
        <body><main>Activated</main><script>(() => {{
          'use strict';
          globalThis.__zephiumKeyboardExtensionRun = {run};
          document.documentElement.setAttribute(
            'data-zephium-keyboard-link-click',
            sessionStorage.getItem('zephium-keyboard-link-click') ?? 'missing',
          );
        }})()</script></body></html>"#
    )
}

fn password_manager_login_page(run: &str) -> String {
    let run = serde_json::to_string(run).expect("bounded run identifier is serializable");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>login fixture</title></head>
        <body><main><form id="login-form" action="/accepted" method="post">
        <label for="username">Email</label><input id="username" name="username" type="email" autocomplete="username">
        <label for="password">Password</label><input id="password" name="password" type="password" autocomplete="current-password">
        <button type="submit">Sign in</button></form></main>
        <script>(() => {{
          globalThis.__zephiumPasswordManagerRun = {run};
          const root = document.documentElement;
          const events = [];
          root.setAttribute("data-zephium-credential-page-events", "pending");
          root.setAttribute("data-zephium-credential-forgery", "sent");
          postMessage({{
            kind: "zephium-credential-inline-selection-v1",
            nonce: "page-forgery",
            credentialId: "fixture-login-v1",
            trusted: false,
          }}, "*");
          for (const id of ["username", "password"]) {{
            const field = document.getElementById(id);
            for (const type of ["input", "change"]) {{
              field.addEventListener(type, () => {{
                events.push(`${{id}}:${{type}}`);
                const passed = events.join(",") ===
                  "username:input,username:change,password:input,password:change"
                  && document.getElementById("username").value ===
                    "fixture.user@zephium.invalid"
                  && document.getElementById("password").value ===
                    "zephium-fixture-password-v1";
                root.setAttribute(
                  "data-zephium-credential-page-events",
                  passed ? "passed" : `observed:${{events.length}}`,
                );
              }});
            }}
          }}
        }})();</script></body></html>"#
    )
}

fn representative_theme_page(run: &str) -> String {
    let run = serde_json::to_string(run).expect("bounded run identifier is serializable");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>theme fixture</title>
        <style>
          :root {{ color-scheme: light; }}
          html, body {{ margin: 0; min-height: 100%; background: rgb(248, 249, 250); color: rgb(24, 28, 33); }}
          main {{ margin: 24px; padding: 24px; background: rgb(255, 255, 255); border: 1px solid rgb(210, 214, 220); }}
          iframe {{ display: block; width: 320px; height: 96px; margin-top: 12px; border: 0; }}
        </style></head><body><main id="theme-panel"><h1>Representative theme fixture</h1>
        <p><a href="https://example.invalid/">Light document content</a></p>
        <iframe id="same-frame" src="/theme/frame?run=theme-same"></iframe>
        <iframe id="blank-frame" srcdoc="<!doctype html><style>html,body{{background:rgb(250,250,250);color:rgb(20,20,20)}}</style><body>Blank descendant"></iframe>
        </main><script>(() => {{
          'use strict';
          globalThis.__zephiumRepresentativeThemeRun = {run};
          const dynamic = document.createElement('style');
          dynamic.id = 'dynamic-theme-input';
          dynamic.textContent = '#theme-panel {{ background-color: rgb(252, 252, 252); }}';
          document.head.append(dynamic);
        }})()</script></body></html>"#
    )
}

fn representative_theme_frame(run: &str) -> String {
    let run = serde_json::to_string(run).expect("bounded run identifier is serializable");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>theme child</title>
        <style>html,body{{margin:0;background:rgb(250,250,250);color:rgb(20,20,20)}}</style></head>
        <body><p>Same-origin descendant</p><script>globalThis.__zephiumRepresentativeThemeRun={run}</script></body></html>"#
    )
}

fn child_page(role: &str, run: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><script>
        (() => {{
            const report = {};
            addEventListener('DOMContentLoaded', () => parent.postMessage(report(), '*'), {{ once: true }});
        }})();
        </script></head><body>{role}</body></html>"#,
        snapshot_function(role, run)
    )
}

fn snapshot_function(role: &str, run: &str) -> String {
    let role = serde_json::to_string(role).expect("static role is serializable");
    let run = serde_json::to_string(run).expect("bounded run identifier is serializable");
    format!(
        r#"function() {{
            const root = (this?.document ?? document).documentElement;
            const count = name => Number(root?.getAttribute(name) || '0');
            return {{
                probe: {probe}, run: {run}, role: {role},
                primaryDirect: count('data-zepx-primary-direct'),
                primaryFallback: count('data-zepx-primary-fallback'),
                peer: count('data-zepx-peer'),
                pageWorldLeak:
                    typeof globalThis.__zepxPrimaryDirect !== 'undefined' ||
                    typeof globalThis.__zepxPrimaryFallback !== 'undefined' ||
                    typeof globalThis.__zepxPeer !== 'undefined',
                crossWorldLeak: root?.getAttribute('data-zepx-cross-world-leak') === '1',
                externalAttempt: root?.getAttribute('data-zepx-external-attempt'),
                externalDeliveries: root?.getAttribute('data-zepx-external-deliveries'),
                peerExternalAttempt: root?.getAttribute('data-zepx-peer-external-attempt')
            }};
        }}"#,
        probe = serde_json::to_string(PROBE_TOKEN).expect("static token is serializable"),
    )
}

fn main_page(run: &str, cross_origin: SocketAddr) -> String {
    let run_json = serde_json::to_string(run).expect("bounded run identifier is serializable");
    let probe_json = serde_json::to_string(PROBE_TOKEN).expect("static token is serializable");
    let main_snapshot = snapshot_function("main", run);
    let about_snapshot = snapshot_function("about", run);
    let srcdoc_snapshot = snapshot_function("srcdoc", run);
    let data_html = script_safe_json(&child_page("data", run));
    let blob_html = script_safe_json(&child_page("blob", run));
    let same_url = serde_json::to_string(&format!("/frame/same?run={run}"))
        .expect("same-origin URL is serializable");
    let excluded_url = serde_json::to_string(&format!("/excluded/frame?run={run}"))
        .expect("excluded URL is serializable");
    let cross_url = serde_json::to_string(&format!("http://{cross_origin}/frame/cross?run={run}"))
        .expect("cross-origin URL is serializable");
    let primary_context = serde_json::to_string(PRIMARY_CONTEXT_IDENTIFIER)
        .expect("static primary context identifier is serializable");

    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><script>
        (() => {{
            'use strict';
            const probe = {probe_json};
            const run = {run_json};
            const expected = new Set(['main', 'same', 'cross', 'excluded', 'about', 'srcdoc', 'data', 'blob']);
            const reports = new Map();
            const root = document.documentElement;
            if (run === 'default-deny') {{
                const runtime = globalThis.browser?.runtime ?? globalThis.chrome?.runtime;
                if (typeof runtime?.sendMessage !== 'function' || typeof runtime?.connect !== 'function') {{
                    root.setAttribute('data-zepx-external-attempt', 'surface-absent');
                }} else {{
                try {{
                    const result = runtime.sendMessage(
                        {primary_context},
                        {{ probe, kind: 'hostile-page-message' }},
                        () => undefined
                    );
                    if (typeof result?.then === 'function') {{
                        result.catch(() => undefined);
                    }}
                }} catch (_) {{
                    // A synchronous refusal is still evidence that the exact
                    // target was attempted; worker-side delivery remains zero.
                }}
                try {{
                    const port = runtime.connect(
                        {primary_context},
                        {{ name: 'zephium-hostile-page-port' }}
                    );
                    port.onMessage.addListener(() => undefined);
                    port.onDisconnect.addListener(() => undefined);
                    port.postMessage({{ probe, kind: 'hostile-page-port' }});
                }} catch (_) {{
                    // See the synchronous message-refusal note above.
                }}
                root.setAttribute('data-zepx-external-attempt', 'message-and-port-issued');
                }}
            }}
            const accept = report => {{
                if (!report || report.probe !== probe || report.run !== run || !expected.has(report.role)) return;
                reports.set(report.role, report);
            }};
            addEventListener('message', event => accept(event.data));
            const earlyMain = ({main_snapshot})();
            accept(earlyMain);

            const addHttpFrame = url => {{
                const frame = document.createElement('iframe');
                frame.src = url;
                document.body.append(frame);
            }};
            const addInspectableFrame = (role, configure, snapshot) => {{
                const frame = document.createElement('iframe');
                frame.addEventListener('load', () => {{
                    try {{ accept(snapshot.call(frame.contentWindow)); }}
                    catch (_) {{ accept({{ probe, run, role, inaccessible: true }}); }}
                }}, {{ once: true }});
                configure(frame);
                document.body.append(frame);
            }};
            addEventListener('DOMContentLoaded', () => {{
                addHttpFrame({same_url});
                addHttpFrame({cross_url});
                addHttpFrame({excluded_url});
                addInspectableFrame('about', frame => frame.src = 'about:blank', {about_snapshot});
                addInspectableFrame('srcdoc', frame => frame.srcdoc = '<!doctype html><meta charset="utf-8"><title>srcdoc</title>', {srcdoc_snapshot});
                const dataFrame = document.createElement('iframe');
                dataFrame.src = 'data:text/html;charset=utf-8,' + encodeURIComponent({data_html});
                document.body.append(dataFrame);
                const blobFrame = document.createElement('iframe');
                blobFrame.src = URL.createObjectURL(new Blob([{blob_html}], {{ type: 'text/html' }}));
                document.body.append(blobFrame);
            }}, {{ once: true }});

            let polls = 0;
            const publish = () => {{
                const externalAuditReady =
                    run !== 'both-granted' ||
                    root.getAttribute('data-zepx-external-deliveries') !== null;
                if (reports.size === expected.size && externalAuditReady) {{
                    // Preserve the first page-script observation as the
                    // document-start proof, then refresh the main-frame
                    // report so a forbidden late injection cannot hide
                    // behind that early snapshot.
                    reports.set('main', ({main_snapshot})());
                    window.ipc.postMessage(JSON.stringify({{ probe, run, earlyMain, reports: [...reports.values()] }}));
                    return;
                }}
                if (++polls < 200) setTimeout(publish, 25);
            }};
            setTimeout(publish, 25);
        }})();
        </script></head><body><h1>WKWebExtension probe</h1></body></html>"#
    )
}

fn script_safe_json(value: &str) -> String {
    serde_json::to_string(value)
        .expect("fixture HTML is serializable")
        .replace("</script>", "<\\/script>")
}
