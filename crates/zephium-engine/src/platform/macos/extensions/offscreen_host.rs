//! An ordinary, extension-free WKWebView for a single authenticated offscreen
//! document. This module deliberately has no `WKWebExtensionController` input.
//! All packaged bytes are read through a retained, validated package-access
//! capability before WebKit sees them. A caller must bind this host to the
//! matching loaded extension owner and retire it before that owner unloads.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::Read;
use std::ptr::NonNull;
use std::rc::{Rc, Weak as RcWeak};
use std::time::{Duration, Instant};

use block2::{DynBlock, RcBlock};
use dispatch2::DispatchQueue;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadOnly};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSError, NSHTTPCookie, NSHTTPURLResponse, NSMutableDictionary,
    NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURLRequest, NSUTF8StringEncoding, NSURL,
};
use objc2_web_kit::{
    WKContentWorld, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate,
    WKScriptMessage, WKScriptMessageHandlerWithReply, WKURLSchemeHandler, WKURLSchemeTask,
    WKUserContentController, WKUserScript, WKUserScriptInjectionTime, WKWebView,
    WKWebViewConfiguration, WKWebsiteDataStore,
};
use objc2_web_kit::{WKWebExtensionContext, WKWebExtensionController};
use serde_json::{json, Value};
use zephium_core::extensions::ExtensionRuntimeInstance;
use zephium_core::ports::extensions::IsolatedExtensionDocumentKind;
use zephium_extension_runtime_api::ExtensionRuntimeHostDataErasureDisposition;
use zephium_extension_runtime_api::{
    ExtensionPackageAccess, ExtensionRuntimeTarget, ExtensionRuntimeVisitorError,
    MAX_EXTENSION_RUNTIME_RESOURCE_BYTES,
};

use super::isolated_resource_bridge::{IsolatedResourceBridge, IsolatedResourceTaskPool};
use crate::EngineEventIngressSink;

const SCHEME: &str = "webkit-extension";
const HANDLER: &str = "zephiumOffscreenRuntimeV1";
const ERASURE_DOCUMENT: &str = "__zephium_offscreen_origin_erasure__.html";
const ERASURE_HTML: &[u8] = b"<!doctype html><meta charset=utf-8><title></title>";
const MAX_URL_BYTES: usize = 2 * 1024;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_PENDING_MESSAGES: usize = 64;
const MESSAGE_TIMEOUT: Duration = Duration::from_secs(30);
const CSP: &str = "default-src 'self' data: blob:; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' blob:; img-src 'self' data: blob:; media-src 'self' data: blob:; font-src 'self' data:; frame-src 'none'; worker-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'";

/// The background connection is active only while this document is open.
/// Completion must run on the main thread and exactly once. The host enforces
/// its own deadline and ignores late callbacks after close or owner retirement.
pub(crate) type OffscreenBackgroundReply = Box<dyn FnOnce(Result<Box<[u8]>, ()>)>;
pub(crate) type OffscreenBackgroundSend = Rc<dyn Fn(Box<[u8]>, OffscreenBackgroundReply)>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OffscreenHostError {
    MainThread,
    InvalidIdentity,
    InvalidDocument,
    PackageUnavailable,
    Native,
    Closed,
    NotReady,
    MessageCapacity,
}

struct PackageReader {
    access: RefCell<ExtensionPackageAccess>,
}

impl PackageReader {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let mut access = self.access.try_borrow_mut().ok()?;
        let entry = access.resources().entry(path)?;
        let length = usize::try_from(entry.declared_bytes()).ok()?;
        if length > MAX_EXTENSION_RUNTIME_RESOURCE_BYTES as usize {
            return None;
        }
        let resource = entry.resource();
        let mut output = Vec::with_capacity(length);
        let mut visitor = |reader: &mut dyn Read| {
            reader
                .take(length as u64 + 1)
                .read_to_end(&mut output)
                .map_err(|_| ExtensionRuntimeVisitorError::ReadFailed)?;
            if output.len() != length {
                return Err(ExtensionRuntimeVisitorError::InvalidData);
            }
            Ok(())
        };
        match access.visit_resource(resource, &mut visitor) {
            Ok(Ok(())) => Some(output),
            _ => None,
        }
    }
}

struct Core {
    base_url: Box<str>,
    document_url: Box<str>,
    reader: Option<PackageReader>,
    erasure: bool,
    background_send: OffscreenBackgroundSend,
    view: RefCell<Option<Weak<WKWebView>>>,
    ready: Cell<bool>,
    on_ready: RefCell<Option<Box<dyn FnOnce()>>>,
    on_failure: RefCell<Option<Box<dyn FnOnce()>>>,
    closed: Cell<bool>,
    next_message: Cell<u64>,
    pending: RefCell<HashMap<u64, RcBlock<dyn Fn(*mut AnyObject, *mut NSString)>>>,
    requests: Cell<usize>,
    responses: Cell<usize>,
    rejections: Cell<usize>,
    audio_started: Cell<usize>,
    audio_stopped: Cell<usize>,
    audio_active: Cell<bool>,
}

impl Core {
    fn accepts_view(&self, view: &WKWebView) -> bool {
        !self.closed.get()
            && self
                .view
                .borrow()
                .as_ref()
                .and_then(Weak::load)
                .is_some_and(|expected| std::ptr::eq(&*expected, view))
    }

    fn respond(&self, ordinal: u64, response: Result<Box<[u8]>, ()>) {
        let Some(reply) = self.pending.borrow_mut().remove(&ordinal) else {
            return;
        };
        if self.closed.get() {
            reply_error(&reply);
            return;
        }
        match response {
            Ok(bytes) if bytes.len() <= MAX_MESSAGE_BYTES => {
                if let Ok(value) = std::str::from_utf8(&bytes) {
                    let value = NSString::from_str(value);
                    reply.call((
                        Retained::as_ptr(&value).cast_mut().cast(),
                        std::ptr::null_mut(),
                    ));
                } else {
                    reply_error(&reply);
                }
            }
            _ => reply_error(&reply),
        }
    }

    fn seal(&self) {
        self.closed.set(true);
        self.ready.set(false);
        self.view.borrow_mut().take();
        self.on_ready.borrow_mut().take();
        self.on_failure.borrow_mut().take();
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for (_, reply) in pending {
            reply_error(&reply);
        }
    }
}

struct ResourceHandlerIvars {
    core: RcWeak<Core>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumIsolatedOffscreenResourceHandlerV1"]
    #[ivars = ResourceHandlerIvars]
    struct ResourceHandler;
    unsafe impl NSObjectProtocol for ResourceHandler {}
    unsafe impl WKURLSchemeHandler for ResourceHandler {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start_task(&self, view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            let Some(core) = self
                .ivars()
                .core
                .upgrade()
                .filter(|core| core.accepts_view(view))
            else {
                fail_task(task);
                return;
            };
            core.requests.set(core.requests.get() + 1);
            let request = unsafe { task.request() };
            let method = request.HTTPMethod().map(|value| value.to_string());
            if method.as_deref().is_some_and(|method| method != "GET") {
                fail_task(task);
                return;
            }
            let Some(url) = request.URL() else {
                fail_task(task);
                return;
            };
            let Some(absolute) = url.absoluteString().map(|value| value.to_string()) else {
                fail_task(task);
                return;
            };
            let Some(path) = admitted_path(&absolute, &core.base_url) else {
                core.rejections.set(core.rejections.get() + 1);
                fail_task(task);
                return;
            };
            let bytes = if core.erasure && path == ERASURE_DOCUMENT {
                Some(ERASURE_HTML.to_vec())
            } else {
                core.reader.as_ref().and_then(|reader| reader.read(&path))
            };
            let Some(bytes) = bytes else {
                core.rejections.set(core.rejections.get() + 1);
                fail_task(task);
                return;
            };
            let mime = mime_for(&path);
            let headers = NSMutableDictionary::new();
            for (key, value) in [
                ("Content-Type", mime),
                ("Content-Length", &bytes.len().to_string()),
                (
                    "Content-Security-Policy",
                    if core.erasure {
                        "default-src 'none'; base-uri 'none'; form-action 'none'"
                    } else {
                        CSP
                    },
                ),
                ("Cache-Control", "no-store"),
                ("X-Content-Type-Options", "nosniff"),
            ] {
                headers.insert(&*NSString::from_str(key), &*NSString::from_str(value));
            }
            let Some(response) = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
                NSHTTPURLResponse::alloc(),
                &url,
                200,
                Some(&NSString::from_str("HTTP/1.1")),
                Some(&headers),
            ) else {
                fail_task(task);
                return;
            };
            let data = objc2_foundation::NSData::with_bytes(&bytes);
            if objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
                task.didReceiveResponse(&response);
                task.didReceiveData(&data);
                task.didFinish();
            }))
            .is_err()
            {
                core.rejections.set(core.rejections.get() + 1);
                fail_task(task);
            } else {
                core.responses.set(core.responses.get() + 1);
            }
        }
        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop_task(&self, _view: &WKWebView, _task: &ProtocolObject<dyn WKURLSchemeTask>) {}
    }
);

struct MessageHandlerIvars {
    core: RcWeak<Core>,
    controller: Retained<WKUserContentController>,
    world: Retained<WKContentWorld>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumIsolatedOffscreenMessageHandlerV1"]
    #[ivars = MessageHandlerIvars]
    struct MessageHandler;
    unsafe impl NSObjectProtocol for MessageHandler {}
    unsafe impl WKScriptMessageHandlerWithReply for MessageHandler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:replyHandler:))]
        unsafe fn receive(
            &self,
            controller: &WKUserContentController,
            message: &WKScriptMessage,
            reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSString)>,
        ) {
            let Some(core) = self.ivars().core.upgrade() else {
                reply_error(reply);
                return;
            };
            let ivars = self.ivars();
            let world = unsafe { message.world() };
            let frame = unsafe { message.frameInfo() };
            let frame_view = unsafe { frame.webView() };
            let view = unsafe { message.webView() };
            if !std::ptr::eq(controller, &*ivars.controller)
                || !std::ptr::eq(&*world, &*ivars.world)
                || unsafe { message.name() }.to_string() != HANDLER
                || !unsafe { frame.isMainFrame() }
                || !frame_view
                    .as_ref()
                    .is_some_and(|view| core.accepts_view(view))
                || !view.as_ref().is_some_and(|view| core.accepts_view(view))
                || !core.ready.get()
            {
                reply_error(reply);
                return;
            }
            let body = unsafe { message.body() };
            let Some(body) = body.downcast_ref::<NSString>() else {
                reply_error(reply);
                return;
            };
            if body.lengthOfBytesUsingEncoding(NSUTF8StringEncoding) > MAX_MESSAGE_BYTES {
                reply_error(reply);
                return;
            }
            let Ok(decoded) = serde_json::from_str::<Value>(&body.to_string()) else {
                reply_error(reply);
                return;
            };
            if decoded["v"] != 1 {
                reply_error(reply);
                return;
            }
            match decoded["operation"].as_str() {
                Some("audio.activity") => {
                    // An extension can emit hints, but cannot use this signal
                    // alone to keep a document alive. The audio lifetime gate
                    // must independently inspect WK media state at expiry.
                    let Some(active) = decoded["value"]["active"].as_bool() else {
                        reply_error(reply);
                        return;
                    };
                    core.audio_active.set(active);
                    if active {
                        core.audio_started
                            .set(core.audio_started.get().saturating_add(1));
                    } else {
                        core.audio_stopped
                            .set(core.audio_stopped.get().saturating_add(1));
                    }
                    let response = NSString::from_str("{\"v\":1,\"ok\":true}");
                    reply.call((
                        Retained::as_ptr(&response).cast_mut().cast(),
                        std::ptr::null_mut(),
                    ));
                }
                Some("runtime.sendMessage") => {
                    if core.pending.borrow().len() >= MAX_PENDING_MESSAGES {
                        reply_error(reply);
                        return;
                    }
                    let Some(next) = core.next_message.get().checked_add(1) else {
                        reply_error(reply);
                        return;
                    };
                    let ordinal = core.next_message.replace(next);
                    let Ok(value) = serde_json::to_vec(&decoded["value"]) else {
                        reply_error(reply);
                        return;
                    };
                    if value.len() > MAX_MESSAGE_BYTES {
                        reply_error(reply);
                        return;
                    }
                    core.pending.borrow_mut().insert(ordinal, reply.copy());
                    let Ok(deadline) = dispatch2::DispatchTime::try_from(MESSAGE_TIMEOUT) else {
                        core.respond(ordinal, Err(()));
                        return;
                    };
                    let weak = Rc::downgrade(&core);
                    let timeout: RcBlock<dyn Fn()> = RcBlock::new(move || {
                        if let Some(core) = weak.upgrade() {
                            core.respond(ordinal, Err(()));
                        }
                    });
                    // One bounded delayed callback exists only while a
                    // message is pending. A settled request makes it inert.
                    unsafe {
                        DispatchQueue::exec_after_with_block(
                            deadline,
                            DispatchQueue::main(),
                            RcBlock::as_ptr(&timeout),
                        )
                    };
                    let weak = Rc::downgrade(&core);
                    (core.background_send)(
                        value.into_boxed_slice(),
                        Box::new(move |outcome| {
                            if let Some(core) = weak.upgrade() {
                                let reply = outcome.and_then(|value| {
                                    let value: Value =
                                        serde_json::from_slice(&value).map_err(|_| ())?;
                                    serde_json::to_vec(&json!({"v":1,"ok":true,"value":value}))
                                        .map(Vec::into_boxed_slice)
                                        .map_err(|_| ())
                                });
                                core.respond(ordinal, reply);
                            }
                        }),
                    );
                }
                _ => reply_error(reply),
            }
        }
    }
);

struct NavigationIvars {
    core: RcWeak<Core>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumIsolatedOffscreenNavigationV1"]
    #[ivars = NavigationIvars]
    struct Navigation;
    unsafe impl NSObjectProtocol for Navigation {}
    unsafe impl WKNavigationDelegate for Navigation {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide(
            &self,
            view: &WKWebView,
            action: &WKNavigationAction,
            completion: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let allowed = self.ivars().core.upgrade().is_some_and(|core| {
                if !core.accepts_view(view) {
                    return false;
                }
                let Some(frame) = (unsafe { action.targetFrame() }) else {
                    return false;
                };
                if !unsafe { frame.isMainFrame() } {
                    return false;
                }
                unsafe { action.request().URL() }
                    .and_then(|url| url.absoluteString())
                    .is_some_and(|url| url.to_string() == core.document_url.as_ref())
            });
            completion.call((if allowed {
                WKNavigationActionPolicy::Allow
            } else {
                WKNavigationActionPolicy::Cancel
            },));
        }
        #[unsafe(method(webView:didFinishNavigation:))]
        fn finished(&self, view: &WKWebView, _navigation: &objc2_web_kit::WKNavigation) {
            if let Some(core) = self
                .ivars()
                .core
                .upgrade()
                .filter(|core| core.accepts_view(view))
            {
                core.ready.set(true);
                let ready = core.on_ready.borrow_mut().take();
                if let Some(ready) = ready {
                    ready();
                }
            }
        }
        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn failed(
            &self,
            view: &WKWebView,
            _navigation: Option<&objc2_web_kit::WKNavigation>,
            _error: &NSError,
        ) {
            if let Some(core) = self
                .ivars()
                .core
                .upgrade()
                .filter(|core| core.accepts_view(view))
            {
                let failed = core.on_failure.borrow_mut().take();
                if let Some(failed) = failed {
                    failed();
                }
            }
        }
    }
);

/// Owns one live renderer and bridge. No renderer, handler or port exists
/// before `open`, and `close` synchronously severs all native callbacks.
pub(crate) struct OffscreenHost {
    core: Rc<Core>,
    view: Retained<WKWebView>,
    controller: Retained<WKUserContentController>,
    _configuration: Retained<WKWebViewConfiguration>,
    _resource_handler: Option<Retained<ResourceHandler>>,
    resource_bridge: Option<IsolatedResourceBridge>,
    _message_handler: Option<Retained<MessageHandler>>,
    _navigation: Retained<Navigation>,
}

impl OffscreenHost {
    /// Opens only after the caller has joined a published runtime, an
    /// OffscreenLocalStorage purpose witness and the exact regular-profile
    /// controller. Resource bytes remain with the service actor until each
    /// asynchronous scheme reply settles.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn open_product(
        mtm: MainThreadMarker,
        store: Retained<WKWebsiteDataStore>,
        runtime: ExtensionRuntimeInstance,
        context: &Retained<WKWebExtensionContext>,
        controller: &Retained<WKWebExtensionController>,
        extension_id: &str,
        base_url: &str,
        document_path: &str,
        allowed_path: Box<dyn Fn(&str) -> bool>,
        sink: EngineEventIngressSink,
        pool: Rc<IsolatedResourceTaskPool>,
        background_send: OffscreenBackgroundSend,
        on_ready: Box<dyn FnOnce()>,
        on_failure: Box<dyn FnOnce()>,
    ) -> Result<Self, OffscreenHostError> {
        if extension_id.len() != 32 || !extension_id.bytes().all(|b| (b'a'..=b'p').contains(&b)) {
            return Err(OffscreenHostError::InvalidIdentity);
        }
        let base = url::Url::parse(base_url).map_err(|_| OffscreenHostError::InvalidIdentity)?;
        if base.scheme() != SCHEME
            || base.host_str() != Some(extension_id)
            || base.path() != "/"
            || base.query().is_some()
            || base.fragment().is_some()
            || base.port().is_some()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.as_str() != base_url
        {
            return Err(OffscreenHostError::InvalidIdentity);
        }
        if !document_path.ends_with(".html")
            || document_path.len() > 512
            || admitted_path(&format!("{base_url}{document_path}"), base_url).as_deref()
                != Some(document_path)
            || !allowed_path(document_path)
        {
            return Err(OffscreenHostError::InvalidDocument);
        }
        let Some(controller_store) =
            (unsafe { controller.configuration().defaultWebsiteDataStore() })
        else {
            return Err(OffscreenHostError::InvalidIdentity);
        };
        if !std::ptr::eq(&*controller_store, &*store) {
            return Err(OffscreenHostError::InvalidIdentity);
        }
        let bridge = IsolatedResourceBridge::new(
            mtm,
            IsolatedExtensionDocumentKind::Offscreen,
            runtime,
            base_url,
            context,
            controller,
            sink,
            pool,
            allowed_path,
        )
        .ok_or(OffscreenHostError::InvalidIdentity)?;
        let document_url = format!("{base_url}{document_path}");
        let core = Rc::new(Core {
            base_url: base_url.into(),
            document_url: document_url.clone().into_boxed_str(),
            reader: None,
            erasure: false,
            background_send,
            view: RefCell::new(None),
            ready: Cell::new(false),
            on_ready: RefCell::new(Some(on_ready)),
            on_failure: RefCell::new(Some(on_failure)),
            closed: Cell::new(false),
            next_message: Cell::new(1),
            pending: RefCell::new(HashMap::new()),
            requests: Cell::new(0),
            responses: Cell::new(0),
            rejections: Cell::new(0),
            audio_started: Cell::new(0),
            audio_stopped: Cell::new(0),
            audio_active: Cell::new(false),
        });
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        super::configure_extension_user_agent(&configuration);
        if !super::install_extension_disposal_symbols(&configuration, mtm) {
            return Err(OffscreenHostError::Native);
        }
        unsafe { configuration.setWebsiteDataStore(&store) };
        if unsafe { configuration.webExtensionController() }.is_some()
            || !bridge.install(&configuration)
        {
            return Err(OffscreenHostError::Native);
        }
        let user_controller = unsafe { configuration.userContentController() };
        let world = unsafe { WKContentWorld::pageWorld(mtm) };
        let source = include_str!("offscreen_runtime_v1.js")
            .replace("__ZEPHIUM_OFFSCREEN_EXTENSION_ID__", extension_id)
            .replace("__ZEPHIUM_OFFSCREEN_BASE_URL__", base_url);
        let script = unsafe {
            WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                WKUserScript::alloc(mtm),
                &NSString::from_str(&source),
                WKUserScriptInjectionTime::AtDocumentStart,
                true,
                &world,
            )
        };
        unsafe { user_controller.addUserScript(&script) };
        let message_handler = MessageHandler::new(
            mtm,
            Rc::downgrade(&core),
            user_controller.clone(),
            world.clone(),
        );
        unsafe {
            user_controller.addScriptMessageHandlerWithReply_contentWorld_name(
                ProtocolObject::from_ref(&*message_handler),
                &world,
                &NSString::from_str(HANDLER),
            )
        };
        let navigation = Navigation::new(mtm, Rc::downgrade(&core));
        let view = unsafe {
            WKWebView::initWithFrame_configuration(
                WKWebView::alloc(mtm),
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(1., 1.)),
                &configuration,
            )
        };
        if !bridge.bind_view(&view) {
            return Err(OffscreenHostError::Native);
        }
        *core.view.borrow_mut() = Some(Weak::from_retained(&view));
        unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*navigation))) };
        let url = NSURL::URLWithString(&NSString::from_str(&document_url))
            .ok_or(OffscreenHostError::Native)?;
        unsafe { view.loadRequest(&NSURLRequest::requestWithURL(&url)) };
        Ok(Self {
            core,
            view,
            controller: user_controller,
            _configuration: configuration,
            _resource_handler: None,
            resource_bridge: Some(bridge),
            _message_handler: Some(message_handler),
            _navigation: navigation,
        })
    }

    pub(crate) fn settle_resource(
        &self,
        runtime: ExtensionRuntimeInstance,
        id: u64,
        outcome: zephium_core::ports::extensions::IsolatedExtensionResourceOutcome,
    ) -> bool {
        self.resource_bridge.as_ref().is_some_and(|bridge| {
            bridge.settle(
                runtime,
                IsolatedExtensionDocumentKind::Offscreen,
                id,
                outcome,
            )
        })
    }

    fn open_with_store(
        mtm: MainThreadMarker,
        store: Retained<WKWebsiteDataStore>,
        extension_id: &str,
        base_url: &str,
        document_path: &str,
        access: ExtensionPackageAccess,
        background_send: OffscreenBackgroundSend,
        audio_playback: bool,
    ) -> Result<Self, OffscreenHostError> {
        if extension_id.len() != 32 || !extension_id.bytes().all(|b| (b'a'..=b'p').contains(&b)) {
            return Err(OffscreenHostError::InvalidIdentity);
        }
        let base = url::Url::parse(base_url).map_err(|_| OffscreenHostError::InvalidIdentity)?;
        if base.scheme() != SCHEME
            || base.path() != "/"
            || base.query().is_some()
            || base.fragment().is_some()
            || base.port().is_some()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.host_str().is_none()
            || base.as_str() != base_url
        {
            return Err(OffscreenHostError::InvalidIdentity);
        }
        if access.target() != ExtensionRuntimeTarget::NativeWebExtension {
            return Err(OffscreenHostError::PackageUnavailable);
        }
        if access.resources().entry(document_path).is_none()
            || !document_path.ends_with(".html")
            || document_path.len() > 512
        {
            return Err(OffscreenHostError::InvalidDocument);
        }
        let document_url = format!("{base_url}{document_path}");
        let core = Rc::new(Core {
            base_url: base_url.into(),
            document_url: document_url.clone().into_boxed_str(),
            reader: Some(PackageReader {
                access: RefCell::new(access),
            }),
            erasure: false,
            background_send,
            view: RefCell::new(None),
            ready: Cell::new(false),
            on_ready: RefCell::new(None),
            on_failure: RefCell::new(None),
            closed: Cell::new(false),
            next_message: Cell::new(1),
            pending: RefCell::new(HashMap::new()),
            requests: Cell::new(0),
            responses: Cell::new(0),
            rejections: Cell::new(0),
            audio_started: Cell::new(0),
            audio_stopped: Cell::new(0),
            audio_active: Cell::new(false),
        });
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        unsafe { configuration.setWebsiteDataStore(&store) };
        if audio_playback {
            unsafe {
                configuration.setMediaTypesRequiringUserActionForPlayback(
                    objc2_web_kit::WKAudiovisualMediaTypes::None,
                );
            }
        }
        if unsafe { configuration.webExtensionController() }.is_some() {
            return Err(OffscreenHostError::Native);
        }
        let controller = unsafe { configuration.userContentController() };
        let world = unsafe { WKContentWorld::pageWorld(mtm) };
        let source = include_str!("offscreen_runtime_v1.js")
            .replace("__ZEPHIUM_OFFSCREEN_EXTENSION_ID__", extension_id)
            .replace("__ZEPHIUM_OFFSCREEN_BASE_URL__", base_url);
        let script = unsafe {
            WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                WKUserScript::alloc(mtm),
                &NSString::from_str(&source),
                WKUserScriptInjectionTime::AtDocumentStart,
                true,
                &world,
            )
        };
        unsafe { controller.addUserScript(&script) };
        let resource_handler = ResourceHandler::new(mtm, Rc::downgrade(&core));
        unsafe {
            configuration.setURLSchemeHandler_forURLScheme(
                Some(ProtocolObject::from_ref(&*resource_handler)),
                &NSString::from_str(SCHEME),
            )
        };
        let message_handler =
            MessageHandler::new(mtm, Rc::downgrade(&core), controller.clone(), world.clone());
        unsafe {
            controller.addScriptMessageHandlerWithReply_contentWorld_name(
                ProtocolObject::from_ref(&*message_handler),
                &world,
                &NSString::from_str(HANDLER),
            )
        };
        let navigation = Navigation::new(mtm, Rc::downgrade(&core));
        let view = unsafe {
            WKWebView::initWithFrame_configuration(
                WKWebView::alloc(mtm),
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(1., 1.)),
                &configuration,
            )
        };
        *core.view.borrow_mut() = Some(Weak::from_retained(&view));
        unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*navigation))) };
        let url = NSURL::URLWithString(&NSString::from_str(&document_url))
            .ok_or(OffscreenHostError::Native)?;
        unsafe { view.loadRequest(&NSURLRequest::requestWithURL(&url)) };
        Ok(Self {
            core,
            view,
            controller,
            _configuration: configuration,
            _resource_handler: Some(resource_handler),
            resource_bridge: None,
            _message_handler: Some(message_handler),
            _navigation: navigation,
        })
    }

    /// Loads only a native-owned inert page at the exact extension origin.
    /// There is no extension controller, publisher resource, injected runtime,
    /// or message handler in this view.
    fn open_erasure(
        mtm: MainThreadMarker,
        store: Retained<WKWebsiteDataStore>,
        extension_id: &str,
        on_ready: Box<dyn FnOnce()>,
        on_failure: Box<dyn FnOnce()>,
    ) -> Result<Self, OffscreenHostError> {
        if extension_id.len() != 32 || !extension_id.bytes().all(|b| (b'a'..=b'p').contains(&b)) {
            return Err(OffscreenHostError::InvalidIdentity);
        }
        let base_url = format!("{SCHEME}://{extension_id}/");
        let document_url = format!("{base_url}{ERASURE_DOCUMENT}");
        let core = Rc::new(Core {
            base_url: base_url.into_boxed_str(),
            document_url: document_url.clone().into_boxed_str(),
            reader: None,
            erasure: true,
            background_send: Rc::new(|_, complete| complete(Err(()))),
            view: RefCell::new(None),
            ready: Cell::new(false),
            on_ready: RefCell::new(Some(on_ready)),
            on_failure: RefCell::new(Some(on_failure)),
            closed: Cell::new(false),
            next_message: Cell::new(0),
            pending: RefCell::new(HashMap::new()),
            requests: Cell::new(0),
            responses: Cell::new(0),
            rejections: Cell::new(0),
            audio_started: Cell::new(0),
            audio_stopped: Cell::new(0),
            audio_active: Cell::new(false),
        });
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        unsafe { configuration.setWebsiteDataStore(&store) };
        if unsafe { configuration.webExtensionController() }.is_some() {
            return Err(OffscreenHostError::Native);
        }
        let controller = unsafe { configuration.userContentController() };
        let resource_handler = ResourceHandler::new(mtm, Rc::downgrade(&core));
        unsafe {
            configuration.setURLSchemeHandler_forURLScheme(
                Some(ProtocolObject::from_ref(&*resource_handler)),
                &NSString::from_str(SCHEME),
            )
        };
        let navigation = Navigation::new(mtm, Rc::downgrade(&core));
        let view = unsafe {
            WKWebView::initWithFrame_configuration(
                WKWebView::alloc(mtm),
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(1., 1.)),
                &configuration,
            )
        };
        *core.view.borrow_mut() = Some(Weak::from_retained(&view));
        unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*navigation))) };
        let url = NSURL::URLWithString(&NSString::from_str(&document_url))
            .ok_or(OffscreenHostError::Native)?;
        unsafe { view.loadRequest(&NSURLRequest::requestWithURL(&url)) };
        Ok(Self {
            core,
            view,
            controller,
            _configuration: configuration,
            _resource_handler: Some(resource_handler),
            resource_bridge: None,
            _message_handler: None,
            _navigation: navigation,
        })
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.core.ready.get() && !self.core.closed.get()
    }
    pub(crate) fn dispatch_to_document(
        &self,
        message: &[u8],
        complete: impl FnOnce(Result<Box<[u8]>, OffscreenHostError>) + 'static,
    ) -> Result<(), OffscreenHostError> {
        if !self.is_ready() {
            return Err(OffscreenHostError::NotReady);
        }
        if message.len() > MAX_MESSAGE_BYTES {
            return Err(OffscreenHostError::MessageCapacity);
        }
        let message =
            std::str::from_utf8(message).map_err(|_| OffscreenHostError::MessageCapacity)?;
        serde_json::from_str::<Value>(message).map_err(|_| OffscreenHostError::MessageCapacity)?;
        let encoded =
            serde_json::to_string(message).map_err(|_| OffscreenHostError::MessageCapacity)?;
        let script = format!(
            "return await globalThis.__zephiumOffscreenRuntimeV1.dispatch(JSON.parse({encoded}));"
        );
        let world = unsafe {
            WKContentWorld::pageWorld(
                MainThreadMarker::new().ok_or(OffscreenHostError::MainThread)?,
            )
        };
        let complete = RefCell::new(Some(complete));
        let weak = Rc::downgrade(&self.core);
        let callback: RcBlock<dyn Fn(*mut AnyObject, *mut NSError)> =
            RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                let Some(complete) = complete.borrow_mut().take() else {
                    return;
                };
                if !weak.upgrade().is_some_and(|core| !core.closed.get()) {
                    complete(Err(OffscreenHostError::Closed));
                    return;
                }
                if !error.is_null() || value.is_null() {
                    complete(Err(OffscreenHostError::Native));
                    return;
                }
                let text = unsafe { &*value }.downcast_ref::<NSString>();
                match text {
                    Some(text)
                        if text.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
                            <= MAX_MESSAGE_BYTES =>
                    {
                        complete(Ok(text.to_string().into_bytes().into_boxed_slice()))
                    }
                    _ => complete(Err(OffscreenHostError::Native)),
                }
            });
        unsafe {
            self.view
                .callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                    &NSString::from_str(&script),
                    None,
                    None,
                    &world,
                    Some(&callback),
                )
        };
        Ok(())
    }

    pub(crate) fn shutdown(&self) {
        self.core.seal();
        if let Some(bridge) = &self.resource_bridge {
            bridge.close();
        }
        unsafe {
            self.view.stopLoading();
            self.view.setNavigationDelegate(None);
            self.controller
                .removeScriptMessageHandlerForName_contentWorld(
                    &NSString::from_str(HANDLER),
                    &WKContentWorld::pageWorld(MainThreadMarker::new().expect("main-thread host")),
                );
            self.controller.removeAllScriptMessageHandlers();
            self.controller.removeAllUserScripts();
        }
    }

    pub(crate) fn close(self) {
        self.shutdown();
    }
}

impl ResourceHandler {
    fn new(mtm: MainThreadMarker, core: RcWeak<Core>) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(ResourceHandlerIvars { core });
        unsafe { msg_send![super(object), init] }
    }
}
impl MessageHandler {
    fn new(
        mtm: MainThreadMarker,
        core: RcWeak<Core>,
        controller: Retained<WKUserContentController>,
        world: Retained<WKContentWorld>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(MessageHandlerIvars {
            core,
            controller,
            world,
        });
        unsafe { msg_send![super(object), init] }
    }
}
impl Navigation {
    fn new(mtm: MainThreadMarker, core: RcWeak<Core>) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(NavigationIvars { core });
        unsafe { msg_send![super(object), init] }
    }
}

struct OriginErasure {
    host: RefCell<Option<OffscreenHost>>,
    store: Retained<WKWebsiteDataStore>,
    extension_id: Box<str>,
    cookies: RefCell<VecDeque<Retained<NSHTTPCookie>>>,
    completion: RefCell<Option<Box<dyn FnOnce(ExtensionRuntimeHostDataErasureDisposition)>>>,
    deadline: Instant,
}

impl OriginErasure {
    fn finish(&self, disposition: ExtensionRuntimeHostDataErasureDisposition) {
        if let Some(host) = self.host.borrow_mut().take() {
            host.close();
        }
        if let Some(completion) = self.completion.borrow_mut().take() {
            completion(disposition);
        }
    }

    fn run(self: &Rc<Self>) {
        if Instant::now() >= self.deadline {
            self.finish(ExtensionRuntimeHostDataErasureDisposition::TimedOut);
            return;
        }
        let Some(host) = self.host.borrow().as_ref().map(|host| host.view.clone()) else {
            self.finish(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
            return;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            self.finish(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
            return;
        };
        let world = unsafe { WKContentWorld::pageWorld(mtm) };
        let machine = Rc::clone(self);
        let callback: RcBlock<dyn Fn(*mut AnyObject, *mut NSError)> =
            RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                let disposition = if Instant::now() >= machine.deadline {
                    ExtensionRuntimeHostDataErasureDisposition::TimedOut
                } else if error.is_null()
                    && unsafe { value.as_ref() }
                        .and_then(|value| value.downcast_ref::<NSString>())
                        .is_some_and(|value| value.to_string() == "erased")
                {
                    ExtensionRuntimeHostDataErasureDisposition::Erased
                } else {
                    ExtensionRuntimeHostDataErasureDisposition::FailedClosed
                };
                if disposition == ExtensionRuntimeHostDataErasureDisposition::Erased {
                    machine.inspect_cookies(false);
                } else {
                    machine.finish(disposition);
                }
            });
        unsafe {
            host.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(include_str!("offscreen_origin_erasure_v1.js")),
                None,
                None,
                &world,
                Some(&callback),
            )
        };
    }

    fn inspect_cookies(self: &Rc<Self>, verifying: bool) {
        if self.completion.borrow().is_none() {
            return;
        }
        if Instant::now() >= self.deadline {
            self.finish(ExtensionRuntimeHostDataErasureDisposition::TimedOut);
            return;
        }
        let machine = Rc::clone(self);
        let callback: RcBlock<dyn Fn(NonNull<NSArray<NSHTTPCookie>>)> =
            RcBlock::new(move |cookies: NonNull<NSArray<NSHTTPCookie>>| {
                if machine.completion.borrow().is_none() {
                    return;
                }
                let Some(cookies) = (unsafe { Retained::retain(cookies.as_ptr()) }) else {
                    machine.finish(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
                    return;
                };
                let mut matching = VecDeque::new();
                for index in 0..cookies.count() {
                    let cookie = cookies.objectAtIndex(index);
                    if cookie.domain().to_string().trim_start_matches('.')
                        == machine.extension_id.as_ref()
                    {
                        matching.push_back(cookie);
                        if matching.len() > 512 {
                            machine
                                .finish(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
                            return;
                        }
                    }
                }
                if verifying {
                    machine.finish(if matching.is_empty() {
                        ExtensionRuntimeHostDataErasureDisposition::Erased
                    } else {
                        ExtensionRuntimeHostDataErasureDisposition::FailedClosed
                    });
                } else {
                    *machine.cookies.borrow_mut() = matching;
                    machine.delete_next_cookie();
                }
            });
        unsafe { self.store.httpCookieStore().getAllCookies(&callback) };
    }

    fn delete_next_cookie(self: &Rc<Self>) {
        if self.completion.borrow().is_none() {
            return;
        }
        if Instant::now() >= self.deadline {
            self.finish(ExtensionRuntimeHostDataErasureDisposition::TimedOut);
            return;
        }
        let cookie = self.cookies.borrow_mut().pop_front();
        let Some(cookie) = cookie else {
            self.inspect_cookies(true);
            return;
        };
        let machine = Rc::clone(self);
        let callback: RcBlock<dyn Fn()> = RcBlock::new(move || {
            machine.delete_next_cookie();
        });
        unsafe {
            self.store
                .httpCookieStore()
                .deleteCookie_completionHandler(&cookie, Some(&callback))
        };
    }
}

/// Erases only DOM storage belonging to this extension's exact custom-scheme
/// origin. The native-owned page cannot load publisher resources or scripts.
pub(super) fn begin_origin_erasure(
    store: Retained<WKWebsiteDataStore>,
    extension_id: &str,
    deadline: Instant,
    completion: Box<dyn FnOnce(ExtensionRuntimeHostDataErasureDisposition)>,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        completion(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
        return;
    };
    if Instant::now() >= deadline {
        completion(ExtensionRuntimeHostDataErasureDisposition::TimedOut);
        return;
    }
    let machine = Rc::new(OriginErasure {
        host: RefCell::new(None),
        store: store.clone(),
        extension_id: extension_id.into(),
        cookies: RefCell::new(VecDeque::new()),
        completion: RefCell::new(Some(completion)),
        deadline,
    });
    let weak = Rc::downgrade(&machine);
    let on_ready = Box::new(move || {
        if let Some(machine) = weak.upgrade() {
            machine.run();
        }
    });
    let weak = Rc::downgrade(&machine);
    let on_failure = Box::new(move || {
        if let Some(machine) = weak.upgrade() {
            machine.finish(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
        }
    });
    match OffscreenHost::open_erasure(mtm, store, extension_id, on_ready, on_failure) {
        Ok(host) => *machine.host.borrow_mut() = Some(host),
        Err(_) => {
            machine.finish(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
            return;
        }
    }
    let timeout =
        dispatch2::DispatchTime::try_from(deadline.saturating_duration_since(Instant::now()));
    let Ok(timeout) = timeout else {
        machine.finish(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
        return;
    };
    let delayed: RcBlock<dyn Fn()> = RcBlock::new(move || {
        machine.finish(ExtensionRuntimeHostDataErasureDisposition::TimedOut);
    });
    unsafe {
        DispatchQueue::exec_after_with_block(
            timeout,
            DispatchQueue::main(),
            RcBlock::as_ptr(&delayed),
        )
    };
}

fn admitted_path(absolute: &str, base_url: &str) -> Option<String> {
    if absolute.len() > MAX_URL_BYTES {
        return None;
    }
    let raw = absolute.strip_prefix(base_url)?;
    if raw.is_empty() || raw.contains(['?', '#']) {
        return None;
    }
    let url = url::Url::parse(absolute).ok()?;
    if url.scheme() != SCHEME
        || url.host_str() != url::Url::parse(base_url).ok()?.host_str()
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    // `Url` canonicalizes dot segments, including percent-encoded dots. The
    // raw URL must already be canonical before resource-plan lookup.
    if url.path().strip_prefix('/')? != raw {
        return None;
    }
    let encoded = raw;
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut chars = encoded.bytes();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let high = (chars.next()? as char).to_digit(16)?;
            let low = (chars.next()? as char).to_digit(16)?;
            let value = ((high << 4) | low) as u8;
            if value.is_ascii() && matches!(value, b'/' | b'\\' | b'.' | b'?' | b'#' | b'%' | 0) {
                return None;
            }
            decoded.push(value);
        } else {
            decoded.push(byte);
        }
    }
    let path = String::from_utf8(decoded).ok()?;
    if path.is_empty()
        || path.starts_with('/')
        || path.contains(['\\', '%', '?', '#'])
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    Some(path)
}

fn mime_for(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        _ => "application/octet-stream",
    }
}

fn fail_task(task: &ProtocolObject<dyn WKURLSchemeTask>) {
    let error = unsafe {
        NSError::errorWithDomain_code_userInfo(
            &NSString::from_str("app.zephium.offscreen-resource"),
            1,
            None,
        )
    };
    let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        task.didFailWithError(&error);
    }));
}

fn reply_error(reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSString)>) {
    let error = NSString::from_str("offscreen runtime request rejected");
    reply.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_urls_are_exact_to_one_package_origin() {
        let id = "cccccccccccccccccccccccccccccccc";
        let base = format!("{SCHEME}://{id}/");
        assert_eq!(
            admitted_path(&format!("{base}offscreen.js"), &base).as_deref(),
            Some("offscreen.js")
        );
        for suffix in ["/%2e%2e/x", "/a%2fb", "/a%5cb", "/a?x=1", "/a#x", "//a"] {
            assert!(
                admitted_path(&format!("{SCHEME}://{id}{suffix}"), &base).is_none(),
                "{suffix}"
            );
        }
        assert!(admitted_path(
            &format!("{SCHEME}://dddddddddddddddddddddddddddddddd/a"),
            &base
        )
        .is_none());
    }
}

#[cfg(feature = "native-web-extension-probes")]
mod probe {
    use super::*;
    use std::io::Cursor;
    use std::path::Path;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow,
        NSWindowStyleMask,
    };
    use objc2_foundation::{NSArray, NSDate, NSRunLoop, NSSet, NSUUID};
    use objc2_web_kit::{
        WKWebExtension, WKWebExtensionContext, WKWebExtensionController,
        WKWebExtensionControllerConfiguration, WKWebsiteDataRecord,
    };
    use sha2::{Digest, Sha256};
    use zephium_extension_acquisition::{AcquiredExtensionArchive, AcquiredExtensionTreeReceipt};
    use zephium_extension_package::{
        ChromiumExtensionId, MAX_CRX3_HEADER_BYTES, MAX_EXTENSION_ARCHIVE_BYTES,
    };
    use zephium_extension_runtime_api::{
        ExtensionPackageAccessError, ExtensionPackageAccessPort,
        ExtensionRuntimeNativeRootLeasePort, ExtensionRuntimeResource,
        ExtensionRuntimeResourceBinding, ExtensionRuntimeResourcePlan,
        ExtensionRuntimeResourceVisitor, ExtensionRuntimeTarget,
    };
    use zephium_extension_runtime_api::{
        ExtensionRuntimeHostDataErasureDisposition, ExtensionRuntimeNativeOwnerId,
    };

    const ID: &str = "cccccccccccccccccccccccccccccccc";
    const GOOGLE_TRANSLATE_ID: &str = "aapbdbdomjkkjkaonfhkkikfgjllcleb";
    const BITWARDEN_ID: &str = "nngceckbapebfimnlniiiahkandclblb";
    const MANIFEST: &[u8] =
        br#"{"manifest_version":3,"name":"Isolated offscreen fixture","version":"1.0"}"#;
    const DOCUMENT: &[u8] = br#"<!doctype html><meta charset="utf-8"><title>pending</title><script src="offscreen.js"></script>"#;
    const SCRIPT: &[u8] = br#"
      chrome.runtime.onMessage.addListener((message) => ({
        dom: new DOMParser().parseFromString('<p>native-dom</p>', 'text/html').body.textContent,
        value: message.value,
        openerNull: window.opener === null,
        tabsAbsent: typeof chrome.tabs === 'undefined',
        storageAbsent: typeof chrome.storage === 'undefined',
        scriptingAbsent: typeof chrome.scripting === 'undefined',
        offscreenAbsent: typeof chrome.offscreen === 'undefined',
        localStorageWorks: (() => { localStorage.setItem('isolated-probe', 'yes'); return localStorage.getItem('isolated-probe') === 'yes'; })()
      }));
    "#;

    struct FixtureProvider {
        plan_digest: [u8; 32],
        files: Vec<(&'static str, &'static [u8], [u8; 32])>,
    }

    impl ExtensionPackageAccessPort for FixtureProvider {
        fn retained_bytes(&self) -> usize {
            std::mem::size_of::<Self>()
                + self.files.capacity() * std::mem::size_of::<(&str, &[u8], [u8; 32])>()
        }

        fn visit_resource(
            &mut self,
            resource: ExtensionRuntimeResource,
            visitor: &mut dyn ExtensionRuntimeResourceVisitor,
        ) -> Result<(), ExtensionPackageAccessError> {
            let index = resource.ordinal() as usize;
            let Some((path, bytes, digest)) = self.files.get(index) else {
                return Err(ExtensionPackageAccessError::ResourceNotDeclared);
            };
            if !resource.authenticates(
                self.plan_digest,
                index as u32,
                path,
                bytes.len() as u64,
                *digest,
            ) || Sha256::digest(bytes).as_slice() != digest
            {
                return Err(ExtensionPackageAccessError::ResourceIdentityMismatch);
            }
            let _ = visitor.visit(&mut Cursor::new(*bytes));
            Ok(())
        }

        fn take_native_root_lease(
            &mut self,
            _target: ExtensionRuntimeTarget,
        ) -> Result<Box<dyn ExtensionRuntimeNativeRootLeasePort>, ExtensionPackageAccessError>
        {
            Err(ExtensionPackageAccessError::NativeRootUnavailable)
        }
    }

    fn access() -> Result<ExtensionPackageAccess, String> {
        let files = vec![
            ("manifest.json", MANIFEST),
            ("offscreen.html", DOCUMENT),
            ("offscreen.js", SCRIPT),
        ];
        let bindings = files
            .iter()
            .map(|(path, bytes)| {
                ExtensionRuntimeResourceBinding::try_new(
                    path,
                    bytes.len() as u64,
                    Sha256::digest(bytes).into(),
                )
                .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let plan =
            ExtensionRuntimeResourcePlan::try_new(bindings).map_err(|error| error.to_string())?;
        let digest = plan.digest();
        let files = files
            .into_iter()
            .map(|(path, bytes)| (path, bytes, Sha256::digest(bytes).into()))
            .collect();
        ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            plan,
            Box::new(FixtureProvider {
                plan_digest: digest,
                files,
            }),
        )
        .map_err(|_| "fixture package access construction failed".to_owned())
    }

    struct OriginalProvider {
        plan_digest: [u8; 32],
        files: Vec<(Box<str>, Box<[u8]>, [u8; 32])>,
        source: AcquiredExtensionTreeReceipt,
    }

    impl ExtensionPackageAccessPort for OriginalProvider {
        fn retained_bytes(&self) -> usize {
            std::mem::size_of::<Self>()
                + self.source.retained_bytes()
                + self
                    .files
                    .iter()
                    .map(|(path, bytes, _)| {
                        path.len()
                            + bytes.len()
                            + std::mem::size_of::<(Box<str>, Box<[u8]>, [u8; 32])>()
                    })
                    .sum::<usize>()
        }

        fn visit_resource(
            &mut self,
            resource: ExtensionRuntimeResource,
            visitor: &mut dyn ExtensionRuntimeResourceVisitor,
        ) -> Result<(), ExtensionPackageAccessError> {
            let index = resource.ordinal() as usize;
            let Some((path, bytes, digest)) = self.files.get(index) else {
                return Err(ExtensionPackageAccessError::ResourceNotDeclared);
            };
            if !resource.authenticates(
                self.plan_digest,
                index as u32,
                path,
                bytes.len() as u64,
                *digest,
            ) || Sha256::digest(bytes).as_slice() != digest
            {
                return Err(ExtensionPackageAccessError::ResourceIdentityMismatch);
            }
            let _ = visitor.visit(&mut Cursor::new(bytes.as_ref()));
            Ok(())
        }

        fn take_native_root_lease(
            &mut self,
            _target: ExtensionRuntimeTarget,
        ) -> Result<Box<dyn ExtensionRuntimeNativeRootLeasePort>, ExtensionPackageAccessError>
        {
            Err(ExtensionPackageAccessError::NativeRootUnavailable)
        }
    }

    fn original_access(
        path: &Path,
        expected_id: &str,
        selected_paths: &[&str],
    ) -> Result<(ExtensionPackageAccess, String, String), String> {
        let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
        let limit = MAX_EXTENSION_ARCHIVE_BYTES as u64 + MAX_CRX3_HEADER_BYTES as u64 + 12;
        let mut crx = Vec::new();
        file.take(limit + 1)
            .read_to_end(&mut crx)
            .map_err(|error| error.to_string())?;
        if crx.len() as u64 > limit {
            return Err("original CRX exceeds package bound".into());
        }
        let original_digest = Sha256::digest(&crx)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let id = ChromiumExtensionId::parse(expected_id).map_err(|error| error.to_string())?;
        let mut archive = AcquiredExtensionArchive::authenticate_upstream_crx3(&crx, &id, None)
            .map_err(|error| error.to_string())?;
        let mut selected = Vec::new();
        let mut receipts = Vec::with_capacity(archive.files().len());
        for index in 0..archive.files().len() {
            let path = archive.files()[index].path().as_str().to_owned();
            let save = selected_paths.contains(&path.as_str());
            let mut body = Vec::new();
            let receipt = if save {
                archive.copy_file(index, &mut body)
            } else {
                archive.copy_file(index, &mut std::io::sink())
            }
            .map_err(|error| error.to_string())?;
            receipts.push(receipt);
            if save {
                selected.push((path.into_boxed_str(), body.into_boxed_slice()));
            }
        }
        let source = archive
            .finish_tree(receipts)
            .map_err(|error| error.to_string())?;
        if selected.len() != selected_paths.len() {
            return Err("original package lacks expected offscreen resources".into());
        }
        selected.sort_by(|a, b| a.0.cmp(&b.0));
        let manifest = selected
            .iter()
            .find(|(path, _)| path.as_ref() == "manifest.json")
            .ok_or("original manifest unavailable")?;
        let parsed: Value =
            serde_json::from_slice(&manifest.1).map_err(|error| error.to_string())?;
        let version = parsed["version"]
            .as_str()
            .ok_or("original manifest version invalid")?
            .to_owned();
        let bindings = selected
            .iter()
            .map(|(path, bytes)| {
                ExtensionRuntimeResourceBinding::try_new(
                    path,
                    bytes.len() as u64,
                    Sha256::digest(bytes).into(),
                )
                .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let plan =
            ExtensionRuntimeResourcePlan::try_new(bindings).map_err(|error| error.to_string())?;
        let plan_digest = plan.digest();
        let files = selected
            .into_iter()
            .map(|(path, bytes)| {
                let digest = Sha256::digest(&bytes).into();
                (path, bytes, digest)
            })
            .collect();
        let access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            plan,
            Box::new(OriginalProvider {
                plan_digest,
                files,
                source,
            }),
        )
        .map_err(|_| "original package access construction failed".to_owned())?;
        Ok((access, version, original_digest))
    }

    fn drain(run_loop: &NSRunLoop) {
        let deadline = NSDate::dateWithTimeIntervalSinceNow(0.01);
        let _ = run_loop
            .runMode_beforeDate(unsafe { objc2_foundation::NSDefaultRunLoopMode }, &deadline);
    }

    fn inspect_original_audio(view: &WKWebView, run_loop: &NSRunLoop) -> String {
        let script = NSString::from_str("JSON.stringify({state:module$contents$google3$googledata$translate$clients$chrome_extensions$translate$offscreen_audioContext?.state ?? null,source:module$contents$google3$googledata$translate$clients$chrome_extensions$translate$offscreen_audioSourceNode !== null})");
        let result = Rc::new(RefCell::new(None));
        let slot = result.clone();
        let completion: RcBlock<dyn Fn(*mut AnyObject, *mut NSError)> =
            RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                *slot.borrow_mut() = Some(if error.is_null() && !value.is_null() {
                    unsafe { &*value }
                        .downcast_ref::<NSString>()
                        .map(NSString::to_string)
                } else {
                    None
                });
            });
        unsafe { view.evaluateJavaScript_completionHandler(&script, Some(&completion)) };
        let deadline = Instant::now() + Duration::from_secs(2);
        while result.borrow().is_none() && Instant::now() < deadline {
            drain(run_loop);
        }
        let script = result
            .borrow_mut()
            .take()
            .flatten()
            .unwrap_or_else(|| "unavailable".into());
        let state = Rc::new(RefCell::new(None));
        let state_slot = state.clone();
        let callback: RcBlock<dyn Fn(objc2_web_kit::WKMediaPlaybackState)> =
            RcBlock::new(move |value: objc2_web_kit::WKMediaPlaybackState| {
                *state_slot.borrow_mut() = Some(value);
            });
        unsafe { view.requestMediaPlaybackStateWithCompletionHandler(&callback) };
        let deadline = Instant::now() + Duration::from_secs(2);
        while state.borrow().is_none() && Instant::now() < deadline {
            drain(run_loop);
        }
        format!("script={script},wk={:?}", state.borrow().as_ref())
    }

    fn dispatch_original_message(
        host: &OffscreenHost,
        run_loop: &NSRunLoop,
        message: Value,
    ) -> Result<Value, String> {
        let message = serde_json::to_vec(&message).map_err(|error| error.to_string())?;
        let result = Rc::new(RefCell::new(None));
        let slot = result.clone();
        host.dispatch_to_document(&message, move |response| {
            *slot.borrow_mut() = Some(response);
        })
        .map_err(|error| format!("original document message was refused: {error:?}"))?;
        let deadline = Instant::now() + Duration::from_secs(8);
        while result.borrow().is_none() && Instant::now() < deadline {
            drain(run_loop);
        }
        let response = result
            .borrow_mut()
            .take()
            .ok_or("original document message did not settle")?
            .map_err(|error| format!("original document message failed: {error:?}"))?;
        serde_json::from_slice(&response).map_err(|error| error.to_string())
    }

    fn inspect_storage_surface(view: &WKWebView, run_loop: &NSRunLoop) -> String {
        let script = NSString::from_str("const result = {indexedDB:typeof indexedDB,caches:typeof caches,opfs:typeof navigator.storage?.getDirectory,cookie:typeof document.cookie,websql:typeof openDatabase,serviceWorker:typeof navigator.serviceWorker,localStorage:typeof localStorage}; for (const [name, probe] of [['indexedDB', async()=>await indexedDB.databases()],['caches',async()=>await caches.keys()],['opfs',async()=>await navigator.storage.getDirectory()],['serviceWorker',async()=>await navigator.serviceWorker.getRegistrations()]]) { try { await probe(); result[name+'Usable']=true } catch(e) { result[name+'Usable']=String(e.name || e) } } try { document.cookie='__zephium_offscreen_cookie_probe=1; path=/'; result.cookieUsable=document.cookie.includes('__zephium_offscreen_cookie_probe=1'); document.cookie='__zephium_offscreen_cookie_probe=; Max-Age=0; path=/' } catch(e) { result.cookieUsable=String(e.name||e) } return JSON.stringify(result)");
        let result = Rc::new(RefCell::new(None));
        let slot = result.clone();
        let callback: RcBlock<dyn Fn(*mut AnyObject, *mut NSError)> =
            RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                *slot.borrow_mut() = Some(if error.is_null() && !value.is_null() {
                    unsafe { &*value }
                        .downcast_ref::<NSString>()
                        .map(NSString::to_string)
                } else {
                    None
                });
            });
        let world = unsafe {
            WKContentWorld::pageWorld(MainThreadMarker::new().expect("main-thread probe"))
        };
        unsafe {
            view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &script,
                None,
                None,
                &world,
                Some(&callback),
            )
        };
        let deadline = Instant::now() + Duration::from_secs(8);
        while result.borrow().is_none() && Instant::now() < deadline {
            drain(run_loop);
        }
        let observed = result
            .borrow_mut()
            .take()
            .flatten()
            .unwrap_or_else(|| "unavailable".into());
        observed
    }

    fn bitwarden_storage_pass(
        mtm: MainThreadMarker,
        store: Retained<WKWebsiteDataStore>,
        access: ExtensionPackageAccess,
        run_loop: &NSRunLoop,
        first: bool,
    ) -> Result<(), String> {
        let (view, handler) = objc2::rc::autoreleasepool(|_| {
            let send: OffscreenBackgroundSend = Rc::new(|_, complete| complete(Err(())));
            let host = OffscreenHost::open_with_store(
                mtm,
                store,
                BITWARDEN_ID,
                &format!("{SCHEME}://{BITWARDEN_ID}/"),
                "offscreen-document/index.html",
                access,
                send,
                false,
            )
            .map_err(|error| format!("original Bitwarden host construction failed: {error:?}"))?;
            let deadline = Instant::now() + Duration::from_secs(8);
            while !host.is_ready() && Instant::now() < deadline {
                drain(run_loop);
            }
            if !host.is_ready() {
                return Err(format!(
                    "original Bitwarden offscreen document did not load: resource_requests={} responses={} rejects={}",
                    host.core.requests.get(), host.core.responses.get(), host.core.rejections.get(),
                ));
            }
            if first {
                println!(
                    "native-probe: Bitwarden offscreen DOM storage surface={}",
                    inspect_storage_surface(&host.view, run_loop)
                );
            }
            if first {
                let _ = dispatch_original_message(
                    &host,
                    run_loop,
                    json!({"command":"localStorageSave","key":"__zephium_bitwarden_probe", "value":"proof-value"}),
                )?;
            }
            let got = dispatch_original_message(
                &host,
                run_loop,
                json!({"command":"localStorageGet","key":"__zephium_bitwarden_probe"}),
            )?;
            if got["handled"] != true || got["value"] != "proof-value" {
                return Err(format!(
                    "original Bitwarden storage response was invalid: {got}"
                ));
            }
            if !first {
                let _ = dispatch_original_message(
                    &host,
                    run_loop,
                    json!({"command":"localStorageRemove","key":"__zephium_bitwarden_probe"}),
                )?;
                let missing = dispatch_original_message(
                    &host,
                    run_loop,
                    json!({"command":"localStorageGet","key":"__zephium_bitwarden_probe"}),
                )?;
                if missing["handled"] != false {
                    return Err(format!(
                        "original Bitwarden storage removal failed: {missing}"
                    ));
                }
            }
            let view = Weak::from_retained(&host.view);
            let handler =
                Weak::from_retained(host._resource_handler.as_ref().expect("probe handler"));
            host.close();
            Ok::<_, String>((view, handler))
        })?;
        let deadline = Instant::now() + Duration::from_secs(8);
        while (view.load().is_some() || handler.load().is_some()) && Instant::now() < deadline {
            drain(run_loop);
        }
        if view.load().is_some() || handler.load().is_some() {
            return Err("original Bitwarden offscreen native teardown was not proven".into());
        }
        Ok(())
    }

    pub(super) fn run_original_bitwarden(path: &Path) -> Result<(), String> {
        let selected = &[
            "manifest.json",
            "offscreen-document/index.html",
            "offscreen-document/offscreen-document.js",
        ];
        let (access, version, digest) = original_access(path, BITWARDEN_ID, selected)?;
        let mtm =
            MainThreadMarker::new().ok_or("Bitwarden offscreen probe requires main thread")?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
        let run_loop = NSRunLoop::mainRunLoop();
        bitwarden_storage_pass(mtm, store.clone(), access, &run_loop, true)?;
        let (access, second_version, second_digest) =
            original_access(path, BITWARDEN_ID, selected)?;
        if second_version != version || second_digest != digest {
            return Err("Bitwarden source changed between offscreen reopen passes".into());
        }
        bitwarden_storage_pass(mtm, store, access, &run_loop, false)?;
        println!("native-probe: original Bitwarden offscreen version={version}; crx_sha256={digest}; complete_original_crx_authentication=passed; exact_offscreen_resources=passed; extension_controller=absent; original_localStorage_save_get_remove=passed; same_store_reopen=passed; native_teardown=passed; product_authority=false");
        Ok(())
    }

    fn bitwarden_storage_absent_pass(
        mtm: MainThreadMarker,
        store: Retained<WKWebsiteDataStore>,
        access: ExtensionPackageAccess,
        run_loop: &NSRunLoop,
    ) -> Result<bool, String> {
        let (empty, view, handler) = objc2::rc::autoreleasepool(|_| {
            let send: OffscreenBackgroundSend = Rc::new(|_, complete| complete(Err(())));
            let host = OffscreenHost::open_with_store(
                mtm,
                store,
                BITWARDEN_ID,
                &format!("{SCHEME}://{BITWARDEN_ID}/"),
                "offscreen-document/index.html",
                access,
                send,
                false,
            )
            .map_err(|error| format!("original Bitwarden erasure host failed: {error:?}"))?;
            let deadline = Instant::now() + Duration::from_secs(8);
            while !host.is_ready() && Instant::now() < deadline {
                drain(run_loop);
            }
            if !host.is_ready() {
                return Err("original Bitwarden erasure reopen did not load".into());
            }
            let result = dispatch_original_message(
                &host,
                run_loop,
                json!({"command":"localStorageGet","key":"__zephium_bitwarden_probe"}),
            )?;
            let view = Weak::from_retained(&host.view);
            let handler =
                Weak::from_retained(host._resource_handler.as_ref().expect("probe handler"));
            host.close();
            Ok::<_, String>((result["handled"] == false, view, handler))
        })?;
        let deadline = Instant::now() + Duration::from_secs(8);
        while (view.load().is_some() || handler.load().is_some()) && Instant::now() < deadline {
            drain(run_loop);
        }
        if view.load().is_some() || handler.load().is_some() {
            return Err("erasure reopen host did not release native objects".into());
        }
        Ok(empty)
    }

    fn peer_origin_pass(
        mtm: MainThreadMarker,
        store: Retained<WKWebsiteDataStore>,
        run_loop: &NSRunLoop,
        write: bool,
    ) -> Result<(), String> {
        const PEER_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let (answer, view) = objc2::rc::autoreleasepool(|_| {
            let host =
                OffscreenHost::open_erasure(mtm, store, PEER_ID, Box::new(|| {}), Box::new(|| {}))
                    .map_err(|error| format!("peer origin host failed: {error:?}"))?;
            let deadline = Instant::now() + Duration::from_secs(8);
            while !host.is_ready() && Instant::now() < deadline {
                drain(run_loop);
            }
            if !host.is_ready() {
                return Err("peer origin page did not load".into());
            }
            let script = if write {
                "localStorage.setItem('__zephium_peer_probe','peer-value'); return localStorage.getItem('__zephium_peer_probe')"
            } else {
                "return localStorage.getItem('__zephium_peer_probe')"
            };
            let answer = Rc::new(RefCell::new(None));
            let slot = answer.clone();
            let callback: RcBlock<dyn Fn(*mut AnyObject, *mut NSError)> =
                RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                    *slot.borrow_mut() = Some(if error.is_null() {
                        unsafe { value.as_ref() }
                            .and_then(|value| value.downcast_ref::<NSString>())
                            .map(NSString::to_string)
                    } else {
                        None
                    });
                });
            let world = unsafe { WKContentWorld::pageWorld(mtm) };
            unsafe {
                host.view
                    .callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                        &NSString::from_str(script),
                        None,
                        None,
                        &world,
                        Some(&callback),
                    )
            };
            let deadline = Instant::now() + Duration::from_secs(8);
            while answer.borrow().is_none() && Instant::now() < deadline {
                drain(run_loop);
            }
            let observed = answer.borrow_mut().take().flatten();
            let view = Weak::from_retained(&host.view);
            host.close();
            Ok::<_, String>((observed, view))
        })?;
        let deadline = Instant::now() + Duration::from_secs(8);
        while view.load().is_some() && Instant::now() < deadline {
            drain(run_loop);
        }
        if view.load().is_some() {
            return Err("peer origin host did not release".into());
        }
        if answer.as_deref() != Some("peer-value") {
            return Err(format!(
                "peer origin value missing after exact erasure: {answer:?}"
            ));
        }
        Ok(())
    }

    fn origin_storage_artifacts_pass(
        mtm: MainThreadMarker,
        store: Retained<WKWebsiteDataStore>,
        run_loop: &NSRunLoop,
        seed: bool,
        expect_present: bool,
    ) -> Result<(), String> {
        const SEED: &str = r#"
            const db = await new Promise((resolve, reject) => {
                const request = indexedDB.open('__zephium_offscreen_erasure_probe', 1);
                request.onsuccess = () => resolve(request.result);
                request.onerror = () => reject(request.error);
            });
            db.close();
            const cache = await caches.open('__zephium_offscreen_erasure_probe');
            await cache.put('https://example.invalid/zephium-offscreen-probe', new Response('proof'));
            const root = await navigator.storage.getDirectory();
            const file = await root.getFileHandle('__zephium_offscreen_erasure_probe', {create: true});
            const writer = await file.createWritable();
            await writer.write('proof');
            await writer.close();
            return 'seeded';
        "#;
        const VERIFY: &str = r#"
            const names = (await indexedDB.databases()).map(value => value.name);
            const cachesRemaining = (await caches.keys()).includes('__zephium_offscreen_erasure_probe');
            const root = await navigator.storage.getDirectory();
            let fileRemaining = false;
            try { await root.getFileHandle('__zephium_offscreen_erasure_probe'); fileRemaining = true; }
            catch (error) { if (error?.name !== 'NotFoundError') throw error; }
            const databaseRemaining = names.includes('__zephium_offscreen_erasure_probe');
            return databaseRemaining && cachesRemaining && fileRemaining ? 'present'
                : databaseRemaining || cachesRemaining || fileRemaining ? 'partial' : 'empty';
        "#;
        let mut observed = None;
        objc2::rc::autoreleasepool(|_| {
            let host = OffscreenHost::open_erasure(
                mtm,
                store,
                BITWARDEN_ID,
                Box::new(|| {}),
                Box::new(|| {}),
            )
            .map_err(|error| format!("storage artifact host failed: {error:?}"))?;
            let deadline = Instant::now() + Duration::from_secs(8);
            while !host.is_ready() && Instant::now() < deadline {
                drain(run_loop);
            }
            if !host.is_ready() {
                return Err("storage artifact page did not load".into());
            }
            let result = Rc::new(RefCell::new(None));
            let slot = result.clone();
            let callback: RcBlock<dyn Fn(*mut AnyObject, *mut NSError)> =
                RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                    *slot.borrow_mut() = Some(if error.is_null() {
                        unsafe { value.as_ref() }
                            .and_then(|value| value.downcast_ref::<NSString>())
                            .map(NSString::to_string)
                    } else {
                        None
                    });
                });
            let world = unsafe { WKContentWorld::pageWorld(mtm) };
            unsafe {
                host.view
                    .callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                        &NSString::from_str(if seed { SEED } else { VERIFY }),
                        None,
                        None,
                        &world,
                        Some(&callback),
                    )
            };
            let deadline = Instant::now() + Duration::from_secs(8);
            while result.borrow().is_none() && Instant::now() < deadline {
                drain(run_loop);
            }
            observed = result.borrow_mut().take().flatten();
            host.close();
            Ok::<_, String>(())
        })?;
        let expected = if seed {
            "seeded"
        } else if expect_present {
            "present"
        } else {
            "empty"
        };
        if observed.as_deref() != Some(expected) {
            return Err(format!(
                "storage artifact check expected {expected}, got {observed:?}"
            ));
        }
        Ok(())
    }

    fn probe_extension_context(
        mtm: MainThreadMarker,
        run_loop: &NSRunLoop,
        controller: &WKWebExtensionController,
    ) -> Result<
        (
            tempfile::TempDir,
            Retained<WKWebExtension>,
            Retained<WKWebExtensionContext>,
        ),
        String,
    > {
        let fixture = tempfile::Builder::new()
            .prefix("zephium-bitwarden-erasure-context-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        std::fs::write(
            fixture.path().join("manifest.json"),
            br#"{"manifest_version":3,"name":"Storage erasure fixture","version":"1.0","action":{"default_popup":"popup.html"}}"#,
        )
        .map_err(|error| error.to_string())?;
        std::fs::write(
            fixture.path().join("popup.html"),
            b"<!doctype html><title>fixture</title>",
        )
        .map_err(|error| error.to_string())?;
        let url = NSURL::fileURLWithPath_isDirectory(
            &NSString::from_str(fixture.path().to_str().ok_or("fixture path is not UTF-8")?),
            true,
        );
        let loaded = Rc::new(RefCell::new(None));
        let slot = loaded.clone();
        let callback: RcBlock<dyn Fn(*mut WKWebExtension, *mut NSError)> =
            RcBlock::new(move |extension: *mut WKWebExtension, error: *mut NSError| {
                *slot.borrow_mut() = Some(
                    if let Some(extension) = unsafe { Retained::retain(extension) } {
                        Ok(extension)
                    } else if !error.is_null() {
                        Err("synthetic extension parse failed".to_owned())
                    } else {
                        Err("synthetic extension parse returned no result".to_owned())
                    },
                );
            });
        unsafe {
            WKWebExtension::extensionWithResourceBaseURL_completionHandler(&url, &callback, mtm)
        };
        let deadline = Instant::now() + Duration::from_secs(8);
        while loaded.borrow().is_none() && Instant::now() < deadline {
            drain(run_loop);
        }
        let extension = loaded
            .borrow_mut()
            .take()
            .ok_or("synthetic extension parse timed out")??;
        let context = unsafe { WKWebExtensionContext::contextForExtension(&extension) };
        let base =
            NSURL::URLWithString(&NSString::from_str(&format!("{SCHEME}://{BITWARDEN_ID}/")))
                .ok_or("invalid original extension origin")?;
        unsafe {
            context.setUniqueIdentifier(&NSString::from_str(BITWARDEN_ID));
            context.setBaseURL(&base);
            controller
                .loadExtensionContext_error(&context)
                .map_err(|_| "synthetic erasure context failed to load")?;
        }
        Ok((fixture, extension, context))
    }

    fn website_local_storage_records(
        store: &WKWebsiteDataStore,
        run_loop: &NSRunLoop,
    ) -> Result<Vec<(String, Retained<WKWebsiteDataRecord>)>, String> {
        let types = NSSet::from_slice(&[unsafe { objc2_web_kit::WKWebsiteDataTypeLocalStorage }]);
        let result = Rc::new(RefCell::new(None));
        let slot = result.clone();
        let callback: RcBlock<dyn Fn(NonNull<NSArray<WKWebsiteDataRecord>>)> =
            RcBlock::new(move |records: NonNull<NSArray<WKWebsiteDataRecord>>| {
                *slot.borrow_mut() = unsafe { Retained::retain(records.as_ptr()) };
            });
        unsafe { store.fetchDataRecordsOfTypes_completionHandler(&types, &callback) };
        let deadline = Instant::now() + Duration::from_secs(8);
        while result.borrow().is_none() && Instant::now() < deadline {
            drain(run_loop);
        }
        let records = result
            .borrow_mut()
            .take()
            .ok_or("website localStorage records timed out")?;
        let mut rows = Vec::new();
        for index in 0..records.count() {
            let record = records.objectAtIndex(index);
            rows.push((unsafe { record.displayName() }.to_string(), record));
        }
        Ok(rows)
    }

    /// Tests the actual product erasure API against authenticated Bitwarden
    /// offscreen JS writing DOM localStorage in an isolated persistent profile.
    /// The random probe data store is deleted after the readback.
    pub(super) fn run_original_bitwarden_erasure(path: &Path) -> Result<(), String> {
        let selected = &[
            "manifest.json",
            "offscreen-document/index.html",
            "offscreen-document/offscreen-document.js",
        ];
        let (access, version, digest) = original_access(path, BITWARDEN_ID, selected)?;
        let mtm = MainThreadMarker::new().ok_or("erasure probe requires main thread")?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let run_loop = NSRunLoop::mainRunLoop();
        let identifier = NSUUID::UUID();
        println!(
            "native-probe: disposable erasure profile={}",
            identifier.UUIDString()
        );
        let proof = objc2::rc::autoreleasepool(|_| {
            let store = unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&identifier, mtm) };
            if !unsafe { store.isPersistent() } {
                return Err("erasure probe received a nonpersistent profile store".into());
            }
            let configuration = unsafe {
                WKWebExtensionControllerConfiguration::configurationWithIdentifier(&identifier, mtm)
            };
            let webview_configuration = unsafe { WKWebViewConfiguration::new(mtm) };
            unsafe {
                webview_configuration.setWebsiteDataStore(&store);
                configuration.setWebViewConfiguration(Some(&webview_configuration));
                configuration.setDefaultWebsiteDataStore(Some(&store));
            }
            let controller = unsafe {
                WKWebExtensionController::initWithConfiguration(
                    WKWebExtensionController::alloc(mtm),
                    &configuration,
                )
            };
            let (_fixture, _extension, context) =
                probe_extension_context(mtm, &run_loop, &controller)?;
            peer_origin_pass(mtm, store.clone(), &run_loop, true)?;
            bitwarden_storage_pass(mtm, store.clone(), access, &run_loop, true)?;
            origin_storage_artifacts_pass(mtm, store.clone(), &run_loop, true, false)?;
            origin_storage_artifacts_pass(mtm, store.clone(), &run_loop, false, true)?;
            let website_records = website_local_storage_records(&store, &run_loop)?;
            println!(
                "native-probe: website localStorage records before extension erasure={:?}",
                website_records
                    .iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>()
            );
            unsafe {
                controller
                    .unloadExtensionContext_error(&context)
                    .map_err(|_| "erasure probe context unload failed")?
            };
            let result = Rc::new(RefCell::new(None));
            let slot = result.clone();
            let owner_id = ExtensionRuntimeNativeOwnerId::parse_exact(BITWARDEN_ID)
                .map_err(|_| "invalid original Bitwarden owner identity")?;
            super::super::record_erasure::begin(
                controller.clone(),
                owner_id,
                Instant::now() + Duration::from_secs(8),
                Box::new(move |disposition| {
                    *slot.borrow_mut() = Some(disposition);
                }),
            );
            let deadline = Instant::now() + Duration::from_secs(8);
            while result.borrow().is_none() && Instant::now() < deadline {
                drain(&run_loop);
            }
            let disposition = result
                .borrow_mut()
                .take()
                .ok_or("extension record erasure timed out")?;
            if !matches!(
                disposition,
                ExtensionRuntimeHostDataErasureDisposition::Erased
                    | ExtensionRuntimeHostDataErasureDisposition::NotPresent
            ) {
                return Err(format!("extension record erasure failed: {disposition:?}"));
            }
            let origin_result = Rc::new(RefCell::new(None));
            let slot = origin_result.clone();
            begin_origin_erasure(
                store.clone(),
                BITWARDEN_ID,
                Instant::now() + Duration::from_secs(8),
                Box::new(move |outcome| {
                    *slot.borrow_mut() = Some(outcome);
                }),
            );
            let deadline = Instant::now() + Duration::from_secs(8);
            while origin_result.borrow().is_none() && Instant::now() < deadline {
                drain(&run_loop);
            }
            let origin_disposition = origin_result
                .borrow_mut()
                .take()
                .ok_or("exact-origin DOM erasure timed out")?;
            if origin_disposition != ExtensionRuntimeHostDataErasureDisposition::Erased {
                return Err(format!(
                    "exact-origin DOM erasure failed: {origin_disposition:?}"
                ));
            }
            let (reopen, reopened_version, reopened_digest) =
                original_access(path, BITWARDEN_ID, selected)?;
            if reopened_version != version || reopened_digest != digest {
                return Err("original Bitwarden source changed during erasure probe".into());
            }
            let empty = bitwarden_storage_absent_pass(mtm, store, reopen, &run_loop)?;
            Ok((disposition, empty))
        });
        let durable: Result<(), String> = (|| {
            if proof.is_ok() {
                let recreated =
                    unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&identifier, mtm) };
                let (access, latest_version, latest_digest) =
                    original_access(path, BITWARDEN_ID, selected)?;
                if latest_version != version || latest_digest != digest {
                    Err("Bitwarden source changed before recreated-store check".to_owned())
                } else if !bitwarden_storage_absent_pass(mtm, recreated.clone(), access, &run_loop)?
                {
                    Err(
                        "Bitwarden DOM storage returned after recreating persistent store"
                            .to_owned(),
                    )
                } else {
                    origin_storage_artifacts_pass(mtm, recreated.clone(), &run_loop, false, false)?;
                    peer_origin_pass(mtm, recreated, &run_loop, false)
                }
            } else {
                Ok(())
            }
        })();
        // WebKit can keep a disposable data store open in its network process
        // until this probe process exits. Its removal is best effort and does
        // not participate in the exact-origin erasure assertion.
        for _ in 0..50 {
            drain(&run_loop);
        }
        let deleted = Rc::new(RefCell::new(None));
        let slot = deleted.clone();
        let callback: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(move |error: *mut NSError| {
            *slot.borrow_mut() = Some(if let Some(error) = unsafe { error.as_ref() } {
                Err(error.localizedDescription().to_string())
            } else {
                Ok(())
            });
        });
        unsafe {
            WKWebsiteDataStore::removeDataStoreForIdentifier_completionHandler(
                &identifier,
                &callback,
                mtm,
            )
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        while deleted.borrow().is_none() && Instant::now() < deadline {
            drain(&run_loop);
        }
        let cleanup = deleted.borrow_mut().take();
        let (disposition, empty) = proof?;
        durable?;
        if !empty {
            return Err(format!("original Bitwarden DOM localStorage survived extension data erasure ({disposition:?})"));
        }
        println!("native-probe: original Bitwarden origin storage erased; version={version}; crx_sha256={digest}; WKWebExtensionDataRecord_disposition={disposition:?}; indexedDB_cache_OPFS_host_reopen_persisted=passed; localStorage_reopen_empty=passed; indexedDB_cache_OPFS_recreated_store_empty=passed; peer_origin_preserved=passed; disposable_profile_cleanup={cleanup:?}");
        Ok(())
    }

    pub(super) fn run() -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("offscreen host probe requires main thread")?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let outbound = Rc::new(AtomicUsize::new(0));
        let run_loop = NSRunLoop::mainRunLoop();
        let (view, handler, diagnostics) = objc2::rc::autoreleasepool(|_| {
            let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
            let observed = outbound.clone();
            let send: OffscreenBackgroundSend = Rc::new(move |message, complete| {
                observed.fetch_add(1, Ordering::Relaxed);
                let request: Value =
                    serde_json::from_slice(&message).expect("bounded fixture message");
                let reply = serde_json::to_vec(&json!({
                    "handled": true,
                    "value": {"from":"background","echo":request},
                }))
                .unwrap();
                complete(Ok(reply.into_boxed_slice()));
            });
            let host = OffscreenHost::open_with_store(
                mtm,
                store,
                ID,
                &format!("{SCHEME}://{ID}/"),
                "offscreen.html",
                access()?,
                send,
                false,
            )
            .map_err(|error| format!("offscreen host construction failed: {error:?}"))?;
            if unsafe { host.view.configuration().webExtensionController() }.is_some() {
                return Err("offscreen host attached an extension controller".into());
            }
            let deadline = Instant::now() + Duration::from_secs(8);
            while !host.is_ready() && Instant::now() < deadline {
                drain(&run_loop);
            }
            if !host.is_ready() {
                return Err("offscreen document did not load".into());
            }
            let response = Rc::new(RefCell::new(None));
            let response_slot = response.clone();
            host.dispatch_to_document(br#"{"value":42}"#, move |reply| {
                *response_slot.borrow_mut() = Some(reply);
            })
            .map_err(|error| format!("offscreen dispatch refused: {error:?}"))?;
            while response.borrow().is_none() && Instant::now() < deadline {
                drain(&run_loop);
            }
            let response = response
                .borrow_mut()
                .take()
                .ok_or("offscreen listener did not answer")?
                .map_err(|error| format!("offscreen listener failed: {error:?}"))?;
            let result: Value =
                serde_json::from_slice(&response).map_err(|error| error.to_string())?;
            let value = &result["value"];
            if result["handled"] != true
                || value["dom"] != "native-dom"
                || value["value"] != 42
                || value["openerNull"] != true
                || value["tabsAbsent"] != true
                || value["storageAbsent"] != true
                || value["scriptingAbsent"] != true
                || value["offscreenAbsent"] != true
                || value["localStorageWorks"] != true
            {
                return Err(format!("offscreen DOM/isolation result invalid: {result}"));
            }
            let from_document = Rc::new(RefCell::new(None));
            let from_document_slot = from_document.clone();
            let completion: RcBlock<dyn Fn(*mut AnyObject, *mut NSError)> =
                RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                    let response = if error.is_null() && !value.is_null() {
                        unsafe { &*value }
                            .downcast_ref::<NSString>()
                            .map(NSString::to_string)
                    } else {
                        None
                    };
                    *from_document_slot.borrow_mut() = Some(response);
                });
            let world = unsafe { WKContentWorld::pageWorld(mtm) };
            unsafe {
                host.view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &NSString::from_str("return JSON.stringify(await chrome.runtime.sendMessage({probe:'from-document'}));"),
                None, None, &world, Some(&completion),
            )
            };
            while from_document.borrow().is_none() && Instant::now() < deadline {
                drain(&run_loop);
            }
            let reply = from_document
                .borrow_mut()
                .take()
                .ok_or("document-to-background did not settle")?
                .ok_or("document-to-background JavaScript failed")?;
            let reply: Value = serde_json::from_str(&reply).map_err(|error| error.to_string())?;
            if reply["from"] != "background" || reply["echo"]["probe"] != "from-document" {
                return Err(format!("document-to-background response mismatch: {reply}"));
            }
            let view = Weak::from_retained(&host.view);
            let handler =
                Weak::from_retained(host._resource_handler.as_ref().expect("probe handler"));
            let diagnostics = host.core.clone();
            host.close();
            Ok::<_, String>((view, handler, diagnostics))
        })?;
        let deadline = Instant::now() + Duration::from_secs(8);
        while (view.load().is_some() || handler.load().is_some()) && Instant::now() < deadline {
            drain(&run_loop);
        }
        if view.load().is_some() || handler.load().is_some() {
            return Err(format!(
                "offscreen native objects survived close: view={} handler={} requests={} responses={} rejections={} pending={}",
                view.load().is_some(), handler.load().is_some(), diagnostics.requests.get(),
                diagnostics.responses.get(), diagnostics.rejections.get(), diagnostics.pending.borrow().len(),
            ));
        }
        println!("native-probe: isolated offscreen document resource=authenticated; extension_controller=absent; runtime_only=passed; dom=passed; local_storage=passed; background_to_document=passed; document_to_background={}; teardown=passed; product_authority=false", outbound.load(Ordering::Relaxed));
        Ok(())
    }

    pub(super) fn run_original(path: &Path) -> Result<(), String> {
        // Authentication and a complete archive stream receipt precede the
        // first native renderer that can execute publisher JavaScript.
        let (access, version, original_digest) = original_access(
            path,
            GOOGLE_TRANSLATE_ID,
            &["manifest.json", "offscreen.html", "offscreen_compiled.js"],
        )?;
        let mtm = MainThreadMarker::new().ok_or("original offscreen probe requires main thread")?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let run_loop = NSRunLoop::mainRunLoop();
        let (view, handler, diagnostics) = objc2::rc::autoreleasepool(|_| {
            let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
            let send: OffscreenBackgroundSend = Rc::new(|_, complete| complete(Err(())));
            let host = OffscreenHost::open_with_store(
                mtm,
                store,
                GOOGLE_TRANSLATE_ID,
                &format!("{SCHEME}://{GOOGLE_TRANSLATE_ID}/"),
                "offscreen.html",
                access,
                send,
                true,
            )
            .map_err(|error| format!("original offscreen host construction failed: {error:?}"))?;
            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    NSRect::new(NSPoint::new(0., 0.), NSSize::new(32., 32.)),
                    NSWindowStyleMask::Borderless,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            unsafe { window.setReleasedWhenClosed(false) };
            window.setAlphaValue(0.0);
            window.setIgnoresMouseEvents(true);
            window
                .contentView()
                .ok_or("offscreen audio host window has no content view")?
                .addSubview(&host.view);
            window.orderFrontRegardless();
            if unsafe { host.view.configuration().webExtensionController() }.is_some() {
                return Err("original offscreen view gained an extension controller".into());
            }
            let deadline = Instant::now() + Duration::from_secs(8);
            while !host.is_ready() && Instant::now() < deadline {
                drain(&run_loop);
            }
            if !host.is_ready() {
                return Err("original offscreen document did not load".into());
            }

            let pause = json!({"target":"offscreen","action":"pauseAudio"});
            let pause = serde_json::to_vec(&pause).map_err(|error| error.to_string())?;
            let result = Rc::new(RefCell::new(None));
            let result_slot = result.clone();
            host.dispatch_to_document(&pause, move |response| {
                *result_slot.borrow_mut() = Some(response);
            })
            .map_err(|error| format!("original pause routing failed: {error:?}"))?;
            while result.borrow().is_none() && Instant::now() < deadline {
                drain(&run_loop);
            }
            let response = result
                .borrow_mut()
                .take()
                .ok_or("original pause listener did not settle")?
                .map_err(|error| format!("original pause listener failed: {error:?}"))?;
            let response: Value =
                serde_json::from_slice(&response).map_err(|error| error.to_string())?;
            if response["handled"] != true {
                return Err(format!(
                    "original pause listener was not invoked: {response}"
                ));
            }

            // A 10ms local PCM WAV exercises the original Google Translate
            // `AudioContext`/buffer-source path without contacting Google or
            // making a user's speaker play meaningful content.
            const SILENT_WAV: &str = "UklGRsQAAABXQVZFZm10IBAAAAABAAEAQB8AAIA+AAACABAAZGF0YaAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
            let play = json!({"target":"offscreen","action":"playAudio","text":"probe","audioSrc":SILENT_WAV});
            let play = serde_json::to_vec(&play).map_err(|error| error.to_string())?;
            let result = Rc::new(RefCell::new(None));
            let result_slot = result.clone();
            host.dispatch_to_document(&play, move |response| {
                *result_slot.borrow_mut() = Some(response);
            })
            .map_err(|error| format!("original audio routing failed: {error:?}"))?;
            while result.borrow().is_none() && Instant::now() < deadline {
                drain(&run_loop);
            }
            let response = result
                .borrow_mut()
                .take()
                .ok_or("original audio listener did not settle")?
                .map_err(|error| format!("original audio listener failed: {error:?}"))?;
            let response: Value =
                serde_json::from_slice(&response).map_err(|error| error.to_string())?;
            if response["handled"] != true {
                return Err(format!(
                    "original audio listener was not invoked: {response}"
                ));
            }
            while host.core.audio_stopped.get() == 0 && Instant::now() < deadline {
                drain(&run_loop);
            }
            if host.core.audio_started.get() == 0
                || host.core.audio_stopped.get() == 0
                || host.core.audio_active.get()
            {
                let playback = inspect_original_audio(&host.view, &run_loop);
                return Err(format!(
                    "original audio playback did not complete: started={} stopped={} active={} {playback}",
                    host.core.audio_started.get(),
                    host.core.audio_stopped.get(),
                    host.core.audio_active.get(),
                ));
            }
            let view = Weak::from_retained(&host.view);
            let handler =
                Weak::from_retained(host._resource_handler.as_ref().expect("probe handler"));
            let diagnostics = host.core.clone();
            host.view.removeFromSuperview();
            host.close();
            window.close();
            Ok::<_, String>((view, handler, diagnostics))
        })?;
        let deadline = Instant::now() + Duration::from_secs(8);
        while (view.load().is_some() || handler.load().is_some()) && Instant::now() < deadline {
            drain(&run_loop);
        }
        if view.load().is_some() || handler.load().is_some() {
            return Err("original offscreen native objects survived close".into());
        }
        println!("native-probe: original Google Translate offscreen version={version}; crx_sha256={original_digest}; complete_original_crx_authentication=passed; exact_offscreen_resources=passed; extension_controller=absent; original_pause_message=passed; original_webaudio_buffer_playback=passed; audio_start_events={}; audio_stop_events={}; teardown=passed; product_authority=false", diagnostics.audio_started.get(), diagnostics.audio_stopped.get());
        Ok(())
    }
}

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn run_offscreen_host_probe() -> Result<(), String> {
    probe::run()
}

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn run_original_google_translate_offscreen_probe(
    path: &std::path::Path,
) -> Result<(), String> {
    probe::run_original(path)
}

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn run_original_bitwarden_offscreen_probe(path: &std::path::Path) -> Result<(), String> {
    probe::run_original_bitwarden(path)
}

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn run_original_bitwarden_offscreen_erasure_probe(
    path: &std::path::Path,
) -> Result<(), String> {
    probe::run_original_bitwarden_erasure(path)
}
