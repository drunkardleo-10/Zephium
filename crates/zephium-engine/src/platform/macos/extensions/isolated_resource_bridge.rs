//! Event-driven, package-authenticated resources for controller-free extension
//! documents. WebKit's scheme task waits while the existing extension-service
//! actor reads through its exact active runtime owner; this UI-thread adapter
//! never opens a package path or runs a second worker.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::{Rc, Weak as RcWeak};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadOnly};
use objc2_foundation::{
    MainThreadMarker, NSData, NSError, NSHTTPURLResponse, NSMutableDictionary, NSObjectProtocol,
    NSString, NSUTF8StringEncoding, NSURL,
};
use objc2_web_kit::{
    WKURLSchemeHandler, WKURLSchemeTask, WKWebExtensionContext, WKWebExtensionController,
    WKWebView, WKWebViewConfiguration,
};
use zephium_core::extensions::ExtensionRuntimeInstance;
use zephium_core::ports::engine::EngineEvent;
use zephium_core::ports::extensions::{
    IsolatedExtensionDocumentKind, IsolatedExtensionResourceOutcome,
    IsolatedExtensionResourceCancel, IsolatedExtensionResourceRequest,
    VerifiedIsolatedExtensionResource,
    MAX_ISOLATED_EXTENSION_RESOURCE_PATH_BYTES,
};

use crate::{EngineEventIngress, EngineEventIngressSink};

const SCHEME: &str = "webkit-extension";
const MAX_REQUEST_URL_BYTES: usize = 2048;
const MAX_NATIVE_PENDING_TASKS: usize = 8;
const RESOURCE_TIMEOUT: Duration = Duration::from_secs(10);
const RESOURCE_ERROR_DOMAIN: &str = "app.zephium.isolated-extension-resource";
const OFFSCREEN_CSP: &str = "default-src 'self' data: blob:; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' blob:; img-src 'self' data: blob:; media-src 'self' data: blob:; font-src 'self' data:; frame-src 'none'; worker-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'";
const SANDBOX_CSP: &str = "default-src 'none'; script-src 'none'; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

/// One process-wide native task budget, independent of the service's byte
/// budget. It allocates nothing until a document requests its first resource.
#[derive(Default)]
pub(super) struct IsolatedResourceTaskPool {
    active: Cell<usize>,
    last_id: Cell<u64>,
}

impl IsolatedResourceTaskPool {
    pub(super) fn shared() -> Rc<Self> {
        thread_local! {
            static LIVE_POOL: RefCell<RcWeak<IsolatedResourceTaskPool>> = RefCell::new(RcWeak::new());
        }
        LIVE_POOL.with(|slot| {
            if let Some(pool) = slot.borrow().upgrade() {
                return pool;
            }
            let pool = Rc::new(Self::default());
            *slot.borrow_mut() = Rc::downgrade(&pool);
            pool
        })
    }

    fn allocate_id(&self) -> Option<u64> {
        let id = self.last_id.get().checked_add(1)?;
        self.last_id.set(id);
        Some(id)
    }

    fn reserve(self: &Rc<Self>) -> Option<TaskReservation> {
        let active = self.active.get();
        if active >= MAX_NATIVE_PENDING_TASKS {
            return None;
        }
        self.active.set(active + 1);
        Some(TaskReservation { pool: self.clone() })
    }

    #[cfg(test)]
    fn active(&self) -> usize {
        self.active.get()
    }
}

struct TaskReservation {
    pool: Rc<IsolatedResourceTaskPool>,
}
impl Drop for TaskReservation {
    fn drop(&mut self) {
        self.pool
            .active
            .set(self.pool.active.get().saturating_sub(1));
    }
}

struct PendingTask {
    task: Retained<ProtocolObject<dyn WKURLSchemeTask>>,
    path: Box<str>,
    deadline: Instant,
    gate: Rc<TaskDeliveryGate>,
    _reservation: TaskReservation,
}

struct TaskDeliveryGate {
    cancel: IsolatedExtensionResourceCancel,
    delivering: Cell<bool>,
}

impl TaskDeliveryGate {
    fn new() -> Self {
        Self {
            cancel: IsolatedExtensionResourceCancel::new(),
            delivering: Cell::new(false),
        }
    }

    fn begin(&self) -> bool {
        self.cancel.is_active() && !self.delivering.replace(true)
    }

    fn is_active(&self) -> bool {
        self.cancel.is_active()
    }

    fn cancel(&self) {
        self.cancel.cancel();
    }
}

struct BridgeState {
    kind: IsolatedExtensionDocumentKind,
    runtime: ExtensionRuntimeInstance,
    base_url: Box<str>,
    context: Weak<WKWebExtensionContext>,
    controller: Weak<WKWebExtensionController>,
    view: RefCell<Option<Weak<WKWebView>>>,
    allowed_path: Box<dyn Fn(&str) -> bool>,
    sink: EngineEventIngressSink,
    pool: Rc<IsolatedResourceTaskPool>,
    pending: RefCell<HashMap<u64, PendingTask>>,
    closed: Cell<bool>,
}

impl BridgeState {
    fn accepts_view(&self, view: &WKWebView) -> bool {
        if self.closed.get() {
            return false;
        }
        let expected_view = self.view.borrow().as_ref().and_then(Weak::load);
        let (context, controller) = (self.context.load(), self.controller.load());
        expected_view
            .as_ref()
            .is_some_and(|expected| std::ptr::eq(&**expected, view))
            && context.as_ref().zip(controller.as_ref()).is_some_and(
                |(context, controller)| unsafe {
                    context.isLoaded()
                        && controller.extensionContexts().containsObject(context)
                        && context
                            .webExtensionController()
                            .as_ref()
                            .is_some_and(|actual| {
                                Retained::as_ptr(actual) == Retained::as_ptr(controller)
                            })
                        && context
                            .baseURL()
                            .absoluteString()
                            .as_ref()
                            .is_some_and(|url| url.to_string() == self.base_url.as_ref())
                },
            )
    }

    fn begin(self: &Rc<Self>, view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
        if !self.accepts_view(view) {
            fail_task(task);
            return;
        }
        let request = unsafe { task.request() };
        if request
            .HTTPMethod()
            .is_some_and(|method| method.to_string() != "GET")
        {
            fail_task(task);
            return;
        }
        let Some(url) = request.URL() else {
            fail_task(task);
            return;
        };
        let Some(absolute) = url.absoluteString() else {
            fail_task(task);
            return;
        };
        if absolute.lengthOfBytesUsingEncoding(NSUTF8StringEncoding) > MAX_REQUEST_URL_BYTES {
            fail_task(task);
            return;
        }
        let absolute = absolute.to_string();
        let Some(path) =
            admitted_path(&self.base_url, &absolute).filter(|path| (self.allowed_path)(path))
        else {
            fail_task(task);
            return;
        };
        let Some(reservation) = self.pool.reserve() else {
            fail_task(task);
            return;
        };
        let Some(id) = self.pool.allocate_id() else {
            fail_task(task);
            return;
        };
        let Some(deadline) = Instant::now().checked_add(RESOURCE_TIMEOUT) else {
            fail_task(task);
            return;
        };
        let Some(task) = (unsafe { Retained::retain(NonNull::from(task).as_ptr()) }) else {
            return;
        };
        let gate = Rc::new(TaskDeliveryGate::new());
        self.pending.borrow_mut().insert(
            id,
            PendingTask {
                task,
                path: path.clone().into_boxed_str(),
                deadline,
                gate: gate.clone(),
                _reservation: reservation,
            },
        );
        let Some(request) =
            IsolatedExtensionResourceRequest::new(
                self.runtime,
                self.kind,
                id,
                &path,
                deadline,
                gate.cancel.clone(),
            )
        else {
            self.fail(id);
            return;
        };
        let weak = Rc::downgrade(self);
        let Ok(when) = dispatch2::DispatchTime::try_from(RESOURCE_TIMEOUT) else {
            self.fail(id);
            return;
        };
        let timeout: RcBlock<dyn Fn()> = RcBlock::new(move || {
            if let Some(state) = weak.upgrade() {
                state.fail(id);
            }
        });
        unsafe {
            DispatchQueue::exec_after_with_block(
                when,
                DispatchQueue::main(),
                RcBlock::as_ptr(&timeout),
            );
        }
        let delivered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (self.sink)(EngineEventIngress::global(
                EngineEvent::IsolatedExtensionResourceRequested {
                    request: Box::new(request),
                },
            ));
        }))
        .is_ok();
        if !delivered {
            self.fail(id);
        }
    }

    fn stop(&self, task: &ProtocolObject<dyn WKURLSchemeTask>) {
        let id = self
            .pending
            .borrow()
            .iter()
            .find_map(|(id, pending)| std::ptr::eq(&*pending.task, task).then_some(*id));
        if let Some(id) = id {
            if let Some(pending) = self.pending.borrow_mut().remove(&id) {
                pending.gate.cancel();
            }
        }
    }

    fn fail(&self, id: u64) {
        let pending = self.pending.borrow_mut().remove(&id);
        if let Some(pending) = pending {
            pending.gate.cancel();
            if !pending.gate.delivering.get() {
                fail_task(&pending.task);
            }
        }
    }

    fn settle(
        &self,
        runtime: ExtensionRuntimeInstance,
        kind: IsolatedExtensionDocumentKind,
        id: u64,
        outcome: IsolatedExtensionResourceOutcome,
    ) -> bool {
        let pending_map = self.pending.borrow();
        let Some(pending) = pending_map.get(&id) else {
            return false;
        };
        let task = pending.task.clone();
        let path = pending.path.clone();
        let deadline = pending.deadline;
        let gate = pending.gate.clone();
        if !gate.begin() {
            return false;
        }
        drop(pending_map);
        let Some(view) = self.view.borrow().as_ref().and_then(Weak::load) else {
            self.fail(id);
            return true;
        };
        if runtime != self.runtime
            || kind != self.kind
            || !self.accepts_view(&view)
            || Instant::now() >= deadline
            || !gate.is_active()
        {
            self.fail(id);
            return true;
        }
        let delivery = match outcome {
            IsolatedExtensionResourceOutcome::Verified(receipt)
                if receipt.runtime() == runtime && receipt.path() == path.as_ref() =>
            {
                publish_verified(&task, &self.base_url, self.kind, receipt, || {
                    gate.is_active()
                        && Instant::now() < deadline
                        && self.pending.borrow().contains_key(&id)
                        && self.accepts_view(&view)
                })
            }
            _ => Delivery::Failed,
        };
        match delivery {
            Delivery::Finished | Delivery::Aborted => {
                if let Some(pending) = self.pending.borrow_mut().remove(&id) {
                    pending.gate.cancel();
                }
            }
            Delivery::Failed => {
                let pending = self.pending.borrow_mut().remove(&id);
                if let Some(pending) = pending {
                    pending.gate.cancel();
                    fail_task(&pending.task);
                }
            }
        }
        true
    }

    fn close(&self) {
        self.closed.set(true);
        self.view.borrow_mut().take();
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for (_, pending) in pending {
            pending.gate.cancel();
            if !pending.gate.delivering.get() {
                fail_task(&pending.task);
            }
        }
    }
}

struct HandlerIvars {
    state: RcWeak<BridgeState>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumIsolatedExtensionResourceHandler"]
    #[ivars = HandlerIvars]
    struct Handler;
    unsafe impl NSObjectProtocol for Handler {}
    unsafe impl WKURLSchemeHandler for Handler {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start_task(&self, view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            if let Some(state) = self.ivars().state.upgrade() {
                state.begin(view, task);
            } else {
                fail_task(task);
            }
        }
        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop_task(&self, _view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            if let Some(state) = self.ivars().state.upgrade() {
                state.stop(task);
            }
        }
    }
);

impl Handler {
    fn new(mtm: MainThreadMarker, state: RcWeak<BridgeState>) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(HandlerIvars { state });
        unsafe { msg_send![super(object), init] }
    }
}

/// Live native handler. Construct only after a separate exact published-runtime
/// purpose witness and document grant have been joined by the owner broker.
pub(super) struct IsolatedResourceBridge {
    state: Rc<BridgeState>,
    handler: Retained<Handler>,
}

impl IsolatedResourceBridge {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        mtm: MainThreadMarker,
        kind: IsolatedExtensionDocumentKind,
        runtime: ExtensionRuntimeInstance,
        base_url: &str,
        context: &Retained<WKWebExtensionContext>,
        controller: &Retained<WKWebExtensionController>,
        sink: EngineEventIngressSink,
        pool: Rc<IsolatedResourceTaskPool>,
        allowed_path: Box<dyn Fn(&str) -> bool>,
    ) -> Option<Self> {
        let parsed = url::Url::parse(base_url).ok()?;
        if parsed.scheme() != SCHEME
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.port().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.host_str().is_none()
            || parsed.as_str() != base_url
            || unsafe { context.baseURL() }.absoluteString()?.to_string() != base_url
            || !unsafe { controller.extensionContexts() }.containsObject(context)
        {
            return None;
        }
        let state = Rc::new(BridgeState {
            kind,
            runtime,
            base_url: base_url.into(),
            context: Weak::from_retained(context),
            controller: Weak::from_retained(controller),
            view: RefCell::new(None),
            allowed_path,
            sink,
            pool,
            pending: RefCell::new(HashMap::new()),
            closed: Cell::new(false),
        });
        let handler = Handler::new(mtm, Rc::downgrade(&state));
        Some(Self { state, handler })
    }

    pub(super) fn install(&self, configuration: &WKWebViewConfiguration) -> bool {
        if unsafe { configuration.webExtensionController() }.is_some() {
            return false;
        }
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            configuration.setURLSchemeHandler_forURLScheme(
                Some(ProtocolObject::from_ref(&*self.handler)),
                &NSString::from_str(SCHEME),
            );
        }))
        .is_ok()
            && unsafe { configuration.urlSchemeHandlerForURLScheme(&NSString::from_str(SCHEME)) }
                .as_ref()
                .is_some_and(|actual| {
                    std::ptr::eq(&**actual, ProtocolObject::from_ref(&*self.handler))
                })
    }

    pub(super) fn bind_view(&self, view: &Retained<WKWebView>) -> bool {
        if self.state.closed.get()
            || unsafe { view.configuration().webExtensionController() }.is_some()
        {
            return false;
        }
        *self.state.view.borrow_mut() = Some(Weak::from_retained(view));
        true
    }

    pub(super) fn settle(
        &self,
        runtime: ExtensionRuntimeInstance,
        kind: IsolatedExtensionDocumentKind,
        id: u64,
        outcome: IsolatedExtensionResourceOutcome,
    ) -> bool {
        self.state.settle(runtime, kind, id, outcome)
    }

    pub(super) fn close(&self) {
        self.state.close();
    }
}

impl Drop for IsolatedResourceBridge {
    fn drop(&mut self) {
        self.state.close();
    }
}

fn admitted_path(base_url: &str, absolute: &str) -> Option<String> {
    if absolute.len() > MAX_REQUEST_URL_BYTES {
        return None;
    }
    let raw = absolute.strip_prefix(base_url)?;
    if raw.is_empty() || raw.contains(['?', '#']) {
        return None;
    }
    let url = url::Url::parse(absolute).ok()?;
    let base = url::Url::parse(base_url).ok()?;
    if url.scheme() != SCHEME
        || url.host_str() != base.host_str()
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().strip_prefix('/')? != raw
    {
        return None;
    }
    let mut decoded = Vec::with_capacity(raw.len());
    let mut bytes = raw.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            let decoded_byte = ((high << 4) | low) as u8;
            if decoded_byte.is_ascii()
                && matches!(decoded_byte, b'/' | b'\\' | b'.' | b'?' | b'#' | b'%' | 0)
            {
                return None;
            }
            decoded.push(decoded_byte);
        } else {
            decoded.push(byte);
        }
    }
    let path = String::from_utf8(decoded).ok()?;
    if path.is_empty()
        || path.len() > MAX_ISOLATED_EXTENSION_RESOURCE_PATH_BYTES
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

fn mime(path: &str) -> &'static str {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Delivery {
    Finished,
    Aborted,
    Failed,
}

fn publish_verified(
    task: &ProtocolObject<dyn WKURLSchemeTask>,
    base_url: &str,
    kind: IsolatedExtensionDocumentKind,
    receipt: VerifiedIsolatedExtensionResource,
    still_live: impl Fn() -> bool,
) -> Delivery {
    // The composed recipe replaces declared sandbox HTML with this exact
    // inert byte sequence. A controller-free offscreen view has a runtime-only
    // facade, so it must never load a withheld sandbox document as its main
    // document even if the publisher asks for that path dynamically.
    if kind == IsolatedExtensionDocumentKind::Offscreen
        && receipt.bytes()
            == zephium_extension_package::sandbox_withholding::SANDBOX_WITHHELD_HTML
    {
        return Delivery::Failed;
    }
    let absolute = format!("{base_url}{}", receipt.path());
    let Some(url) = NSURL::URLWithString(&NSString::from_str(&absolute)) else {
        return Delivery::Failed;
    };
    let bytes_len = receipt.bytes().len();
    let headers = NSMutableDictionary::new();
    let csp = match kind {
        IsolatedExtensionDocumentKind::Offscreen => OFFSCREEN_CSP,
        IsolatedExtensionDocumentKind::Sandbox => SANDBOX_CSP,
    };
    for (key, value) in [
        ("Content-Type", mime(receipt.path())),
        ("Content-Length", &bytes_len.to_string()),
        ("Content-Security-Policy", csp),
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
        return Delivery::Failed;
    };
    let data = if bytes_len == 0 {
        // Empty files consume no byte budget. The request permit remains live
        // through this synchronous native completion frontier.
        NSData::new()
    } else {
        let held = Arc::new(Mutex::new(Some(receipt)));
        let pointer = {
            let guard = held
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            NonNull::new(guard.as_ref().unwrap().bytes().as_ptr() as *mut c_void).unwrap()
        };
        let release = held.clone();
        let deallocator: RcBlock<dyn Fn(NonNull<c_void>, usize)> =
            RcBlock::new(move |_pointer: NonNull<c_void>, _length: usize| {
                let _ = release
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
            });
        let allocated = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            NSData::initWithBytesNoCopy_length_deallocator(
                NSData::alloc(),
                pointer,
                bytes_len,
                Some(&deallocator),
            )
        }));
        let Ok(data) = allocated else {
            return Delivery::Failed;
        };
        data
    };
    if !still_live() {
        return Delivery::Aborted;
    }
    if objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        task.didReceiveResponse(&response);
    }))
    .is_err()
    {
        return Delivery::Failed;
    }
    if !still_live() {
        return Delivery::Aborted;
    }
    if objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        task.didReceiveData(&data);
    }))
    .is_err()
    {
        return Delivery::Failed;
    }
    if !still_live() {
        return Delivery::Aborted;
    }
    if objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        task.didFinish();
    }))
    .is_err()
    {
        return Delivery::Failed;
    }
    Delivery::Finished
}

fn fail_task(task: &ProtocolObject<dyn WKURLSchemeTask>) {
    let error = unsafe {
        NSError::errorWithDomain_code_userInfo(&NSString::from_str(RESOURCE_ERROR_DOMAIN), 1, None)
    };
    let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        task.didFailWithError(&error);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_base_and_canonical_path_keep_foreign_resources_out() {
        let base = "webkit-extension://cccccccccccccccccccccccccccccccc/";
        assert_eq!(
            admitted_path(base, &format!("{base}document.js")).as_deref(),
            Some("document.js")
        );
        for suffix in ["../x", "%2e%2e/x", "a%2fb", "a%5cb", "//x", "a?x=1", "a#x"] {
            assert!(
                admitted_path(base, &format!("{base}{suffix}")).is_none(),
                "{suffix}"
            );
        }
        assert!(admitted_path(
            base,
            "webkit-extension://dddddddddddddddddddddddddddddddd/a"
        )
        .is_none());
    }
    #[test]
    fn native_pending_tasks_have_one_shared_ceiling() {
        let pool = Rc::new(IsolatedResourceTaskPool::default());
        let leases = (0..MAX_NATIVE_PENDING_TASKS)
            .map(|_| pool.reserve().unwrap())
            .collect::<Vec<_>>();
        assert!(pool.reserve().is_none());
        drop(leases);
        assert_eq!(pool.active(), 0);
    }

    #[test]
    fn close_reopen_cannot_reuse_a_late_reply_id() {
        let pool = Rc::new(IsolatedResourceTaskPool::default());
        let first = pool.allocate_id().unwrap();
        let first_lease = pool.reserve().unwrap();
        drop(first_lease);
        let second = pool.allocate_id().unwrap();
        assert_ne!(first, second);
        assert_eq!(second, first + 1);
        pool.last_id.set(u64::MAX);
        assert!(pool.allocate_id().is_none());
    }

    #[test]
    fn native_delivery_gate_rejects_reentry_and_cancellation() {
        let gate = TaskDeliveryGate::new();
        assert!(gate.begin());
        assert!(!gate.begin());
        assert!(gate.is_active());
        gate.cancel();
        assert!(!gate.is_active());
        assert!(!gate.begin());
    }
}
