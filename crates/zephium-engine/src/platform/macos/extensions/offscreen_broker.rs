//! Fixed-ID, on-demand native port for one controller-free offscreen document.
//! The port carries only runtime messages; package bytes stay with the active
//! extension-service owner and are delivered through `isolated_resource_bridge`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ptr::NonNull;
use std::rc::{Rc, Weak as RcWeak};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use dispatch2::DispatchQueue;
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2_foundation::{MainThreadMarker, NSError, NSString, NSUTF8StringEncoding};
use objc2_web_kit::{WKWebExtensionContext, WKWebExtensionController, WKWebExtensionMessagePort};
use serde_json::{json, Value};
use zephium_core::extensions::{
    ExtensionCompatibilityBrokerPurpose, ExtensionCompatibilityBrokerWitness,
    ExtensionRuntimeInstance,
};
use zephium_core::ids::ProfileId;
use zephium_core::ports::extensions::IsolatedExtensionResourceOutcome;

use super::isolated_resource_bridge::IsolatedResourceTaskPool;
use super::offscreen_host::{OffscreenBackgroundReply, OffscreenBackgroundSend, OffscreenHost};
use crate::host::NativeResourceLease;
use crate::EngineEventIngressSink;

pub(super) const APPLICATION_ID: &str = "app.zephium.extension-offscreen.v1";
const MAX_PORTS_GLOBAL: usize = 8;
const MAX_SESSIONS_PER_PROFILE: usize = 4;
const MAX_PENDING_FRAMES: usize = 8;
const MAX_DOCUMENT_REPLIES: usize = 64;
const MAX_WIRE_BYTES: usize = 1024 * 1024;
const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const DOCUMENT_MESSAGE_TIMEOUT: Duration = Duration::from_secs(30);
static OPEN_PORTS: AtomicUsize = AtomicUsize::new(0);
static PENDING_AUTHORIZATION: AtomicUsize = AtomicUsize::new(0);

fn qa_trace(phase: &'static str) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var("ZEPHIUM_OFFSCREEN_QA_TRACE").as_deref() == Ok("1")) {
        eprintln!("offscreen-product: {phase}");
    }
}

pub(crate) fn has_pending_authorization() -> bool {
    PENDING_AUTHORIZATION.load(Ordering::Acquire) != 0
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct OffscreenSessionId(u64);

struct PortPermit;
impl PortPermit {
    fn reserve() -> Option<Self> {
        OPEN_PORTS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_PORTS_GLOBAL).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
impl Drop for PortPermit {
    fn drop(&mut self) {
        OPEN_PORTS.fetch_sub(1, Ordering::AcqRel);
    }
}

struct WireRequest {
    id: u64,
    operation: Box<str>,
    value: Value,
}

struct Session {
    context: Weak<WKWebExtensionContext>,
    port: Retained<WKWebExtensionMessagePort>,
    _permit: PortPermit,
    runtime: Option<ExtensionRuntimeInstance>,
    pending: VecDeque<WireRequest>,
    pending_authorization_counted: bool,
    document: Option<Rc<OffscreenHost>>,
    document_path: Option<Box<str>>,
    document_lease: Option<NativeResourceLease>,
    creating: Option<u64>,
    next_document_message: u64,
    document_replies: HashMap<u64, OffscreenBackgroundReply>,
}

impl Session {
    fn take_document(
        &mut self,
    ) -> (
        Option<Rc<OffscreenHost>>,
        Option<NativeResourceLease>,
        Vec<OffscreenBackgroundReply>,
    ) {
        self.creating = None;
        self.document_path = None;
        (
            self.document.take(),
            self.document_lease.take(),
            self.document_replies
                .drain()
                .map(|(_, reply)| reply)
                .collect(),
        )
    }
}

pub(super) struct OffscreenBroker {
    profile: ProfileId,
    sink: Option<EngineEventIngressSink>,
    controller: RefCell<Option<Weak<WKWebExtensionController>>>,
    sessions: RefCell<HashMap<OffscreenSessionId, Rc<RefCell<Session>>>>,
    next_session: Cell<u64>,
    sealed: Cell<bool>,
    self_weak: RcWeak<Self>,
}

impl OffscreenBroker {
    pub(super) fn new(profile: ProfileId, sink: Option<EngineEventIngressSink>) -> Rc<Self> {
        Rc::new_cyclic(|weak| Self {
            profile,
            sink,
            controller: RefCell::new(None),
            sessions: RefCell::new(HashMap::new()),
            next_session: Cell::new(0),
            sealed: Cell::new(false),
            self_weak: weak.clone(),
        })
    }

    pub(super) fn bind_controller(&self, controller: &Retained<WKWebExtensionController>) {
        *self.controller.borrow_mut() = Some(Weak::from_retained(controller));
    }

    pub(super) fn matches_application_identifier(id: &NSString) -> bool {
        id.isEqualToString(&NSString::from_str(APPLICATION_ID))
    }

    fn accepts(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
    ) -> bool {
        !self.sealed.get()
            && self
                .controller
                .borrow()
                .as_ref()
                .and_then(Weak::load)
                .is_some_and(|expected| {
                    std::ptr::eq(&*expected, controller)
                        && unsafe { expected.extensionContexts() }.containsObject(context)
                        && unsafe { context.isLoaded() }
                })
    }

    pub(super) fn begin_port(
        self: &Rc<Self>,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
        port: &WKWebExtensionMessagePort,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        if !unsafe { port.applicationIdentifier() }
            .as_ref()
            .is_some_and(|id| Self::matches_application_identifier(id))
            || !self.accepts(controller, context)
            || self.sink.is_none()
        {
            reject_connection(completion);
            return;
        }
        let previous = self.sessions.borrow().iter().find_map(|(id, session)| {
            let session = session.borrow();
            session
                .context
                .load()
                .is_some_and(|candidate| std::ptr::eq(&*candidate, context))
                .then_some((
                    *id,
                    session.document.is_none()
                        && session.creating.is_none()
                        && session.pending.is_empty()
                        && session.document_replies.is_empty(),
                ))
        });
        if let Some((previous, recyclable)) = previous {
            if !recyclable {
                reject_connection(completion);
                return;
            }
            // A completed `hasDocument() == false` may disconnect just after
            // its caller opens the create port. Retire only an empty session.
            self.close_session(previous);
        }
        if self.sessions.borrow().len() >= MAX_SESSIONS_PER_PROFILE {
            reject_connection(completion);
            return;
        }
        let Some(permit) = PortPermit::reserve() else {
            reject_connection(completion);
            return;
        };
        let (Some(port), Some(context)) = (
            unsafe { Retained::retain(NonNull::from(port).as_ptr()) },
            unsafe { Retained::retain(NonNull::from(context).as_ptr()) },
        ) else {
            reject_connection(completion);
            return;
        };
        let Some(next) = self.next_session.get().checked_add(1) else {
            reject_connection(completion);
            return;
        };
        self.next_session.set(next);
        let id = OffscreenSessionId(next);
        let session = Rc::new(RefCell::new(Session {
            context: Weak::from_retained(&context),
            port: port.clone(),
            _permit: permit,
            runtime: None,
            pending: VecDeque::new(),
            pending_authorization_counted: false,
            document: None,
            document_path: None,
            document_lease: None,
            creating: None,
            next_document_message: 0,
            document_replies: HashMap::new(),
        }));
        let weak = self.self_weak.clone();
        let on_message: RcBlock<dyn Fn(*mut AnyObject, *mut NSError)> =
            RcBlock::new(move |message, error| {
                if let Some(broker) = weak.upgrade() {
                    broker.receive(id, message, error);
                }
            });
        let weak = self.self_weak.clone();
        let on_disconnect: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(move |_| {
            if let Some(broker) = weak.upgrade() {
                broker.close_session(id);
            }
        });
        if objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            port.setMessageHandler(Some(&on_message));
            port.setDisconnectHandler(Some(&on_disconnect));
        }))
        .is_err()
        {
            reject_connection(completion);
            return;
        }
        self.sessions.borrow_mut().insert(id, session);
        qa_trace("port-open");
        completion.call((std::ptr::null_mut(),));
        let weak = self.self_weak.clone();
        let Some(watchdog) = dispatch2::DispatchTime::try_from(AUTH_TIMEOUT).ok() else {
            self.close_session(id);
            return;
        };
        let timeout: RcBlock<dyn Fn()> = RcBlock::new(move || {
            if let Some(broker) = weak.upgrade() {
                broker.timeout_unauthorized(id);
            }
        });
        unsafe {
            DispatchQueue::exec_after_with_block(
                watchdog,
                DispatchQueue::main(),
                RcBlock::as_ptr(&timeout),
            );
        }
    }

    pub(super) fn subject(
        &self,
        id: OffscreenSessionId,
    ) -> Option<(*const WKWebExtensionContext, bool)> {
        let session = self.sessions.borrow().get(&id)?.clone();
        let session = session.borrow();
        let context = session.context.load()?;
        Some((
            Retained::as_ptr(&context),
            session
                .pending
                .iter()
                .any(|frame| frame.operation.as_ref() == "create"),
        ))
    }

    pub(super) fn pending_authorization_ids(&self) -> Vec<OffscreenSessionId> {
        self.sessions
            .borrow()
            .iter()
            .filter_map(|(id, session)| {
                session
                    .borrow()
                    .pending_authorization_counted
                    .then_some(*id)
            })
            .collect()
    }

    pub(super) fn authorize(
        &self,
        id: OffscreenSessionId,
        witness: Option<ExtensionCompatibilityBrokerWitness>,
        lease: Option<NativeResourceLease>,
    ) -> bool {
        let Some(session) = self.sessions.borrow().get(&id).cloned() else {
            return false;
        };
        let Some(witness) = witness else {
            if session.borrow().runtime.is_some() {
                self.close_session(id);
            }
            return false;
        };
        if witness.purpose() != ExtensionCompatibilityBrokerPurpose::OffscreenLocalStorage
            || witness.runtime_instance().profile() != self.profile
            || !self.session_context_current(&session)
        {
            self.close_session(id);
            return false;
        }
        let runtime = witness.runtime_instance();
        let frames = {
            let mut session = session.borrow_mut();
            if session.runtime.is_some_and(|previous| previous != runtime) {
                drop(session);
                self.close_session(id);
                return false;
            }
            session.runtime = Some(runtime);
            if session.pending_authorization_counted {
                session.pending_authorization_counted = false;
                PENDING_AUTHORIZATION.fetch_sub(1, Ordering::AcqRel);
            }
            std::mem::take(&mut session.pending)
        };
        let mut lease = lease;
        qa_trace("authorized");
        for frame in frames {
            let admission = if frame.operation.as_ref() == "create" {
                lease.take()
            } else {
                None
            };
            self.execute(id, runtime, frame, admission);
        }
        true
    }

    fn session_context_current(&self, session: &Rc<RefCell<Session>>) -> bool {
        let context = session.borrow().context.load();
        let controller = self.controller.borrow().as_ref().and_then(Weak::load);
        context
            .as_ref()
            .zip(controller.as_ref())
            .is_some_and(|(context, controller)| self.accepts(controller, context))
    }

    fn receive(
        self: &Rc<Self>,
        id: OffscreenSessionId,
        message: *mut AnyObject,
        error: *mut NSError,
    ) {
        if !error.is_null() {
            self.close_session(id);
            return;
        }
        let Some(message) =
            (unsafe { message.as_ref() }).and_then(|message| message.downcast_ref::<NSString>())
        else {
            self.close_session(id);
            return;
        };
        if message.lengthOfBytesUsingEncoding(NSUTF8StringEncoding) > MAX_WIRE_BYTES {
            self.close_session(id);
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(&message.to_string()) else {
            self.close_session(id);
            return;
        };
        let Some(session) = self.sessions.borrow().get(&id).cloned() else {
            return;
        };
        if value["v"] != 1 {
            self.close_session(id);
            return;
        }
        if value["operation"] == "fromDocumentResult" {
            let Some(reply_id) = value["id"].as_u64() else {
                self.close_session(id);
                return;
            };
            let reply = session.borrow_mut().document_replies.remove(&reply_id);
            if let Some(reply) = reply {
                qa_trace("document-to-background-replied");
                let response = &value["result"];
                let bytes = response["handled"].as_bool().and_then(|_| {
                    serde_json::to_vec(response)
                        .ok()
                        .filter(|bytes| bytes.len() <= MAX_WIRE_BYTES)
                });
                reply(bytes.map(Vec::into_boxed_slice).ok_or(()));
            }
            return;
        }
        let (Some(request_id), Some(operation)) =
            (value["id"].as_u64(), value["operation"].as_str())
        else {
            self.close_session(id);
            return;
        };
        if request_id == 0
            || request_id > NumberMaxSafeInteger::VALUE
            || !matches!(operation, "has" | "create" | "close" | "message")
        {
            self.close_session(id);
            return;
        }
        {
            let mut session = session.borrow_mut();
            if session.pending.len() >= MAX_PENDING_FRAMES {
                drop(session);
                self.close_session(id);
                return;
            }
            session.pending.push_back(WireRequest {
                id: request_id,
                operation: operation.into(),
                value: value["value"].clone(),
            });
            if session.runtime.is_none() && !session.pending_authorization_counted {
                session.pending_authorization_counted = true;
                PENDING_AUTHORIZATION.fetch_add(1, Ordering::AcqRel);
            }
        }
        let profile = self.profile;
        if !crate::host::with_extension_browser_request_terminal(move |host| {
            host.finalize_extension_offscreen_request(profile, id);
        }) {
            self.close_session(id);
        }
    }

    fn execute(
        &self,
        id: OffscreenSessionId,
        runtime: ExtensionRuntimeInstance,
        request: WireRequest,
        lease: Option<NativeResourceLease>,
    ) {
        let Some(session) = self.sessions.borrow().get(&id).cloned() else {
            return;
        };
        if !self.session_context_current(&session) || session.borrow().runtime != Some(runtime) {
            self.close_session(id);
            return;
        }
        match request.operation.as_ref() {
            "has" => {
                qa_trace("has-document");
                self.reply(
                    id,
                    request.id,
                    Ok(Value::Bool(self.document_owner(runtime).is_some())),
                )
            }
            "close" => {
                qa_trace("close-requested");
                if let Some(owner) = self.document_owner(runtime) {
                    self.close_document(owner);
                    self.reply(id, request.id, Ok(Value::Bool(true)));
                } else {
                    self.reply(id, request.id, Err("Offscreen document is absent"));
                }
            }
            "create" => {
                qa_trace("create-requested");
                self.create_document(id, runtime, request, lease)
            }
            "message" => {
                qa_trace("background-to-document-requested");
                let document = session.borrow().document.clone();
                let Some(document) = document else {
                    self.reply(id, request.id, Err("Offscreen document is absent"));
                    return;
                };
                let Ok(bytes) = serde_json::to_vec(&request.value) else {
                    self.reply(id, request.id, Err("Invalid message"));
                    return;
                };
                let weak = self.self_weak.clone();
                let reply_id = request.id;
                if document
                    .dispatch_to_document(&bytes, move |result| {
                        if let Some(broker) = weak.upgrade() {
                            let value = result
                                .ok()
                                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
                            qa_trace(if value.is_some() {
                                "background-to-document-replied"
                            } else {
                                "background-to-document-failed"
                            });
                            broker.reply(id, reply_id, value.ok_or("Offscreen message failed"));
                        }
                    })
                    .is_err()
                {
                    self.reply(id, request.id, Err("Offscreen message failed"));
                }
            }
            _ => self.reply(id, request.id, Err("Invalid operation")),
        }
    }

    fn create_document(
        &self,
        id: OffscreenSessionId,
        runtime: ExtensionRuntimeInstance,
        request: WireRequest,
        lease: Option<NativeResourceLease>,
    ) {
        let Some(session) = self.sessions.borrow().get(&id).cloned() else {
            return;
        };
        if self.document_owner(runtime).is_some() {
            self.reply(
                id,
                request.id,
                Err("Only a single offscreen document may be created."),
            );
            return;
        }
        let Some(path) = request.value.get("url").and_then(Value::as_str) else {
            self.reply(id, request.id, Err("Invalid offscreen document"));
            return;
        };
        if request.value.get("reasons") != Some(&json!(["LOCAL_STORAGE"]))
            || request
                .value
                .get("justification")
                .and_then(Value::as_str)
                .is_none_or(|value| value.len() > 1024)
            || path.len() > 512
        {
            self.reply(id, request.id, Err("Unsupported offscreen reason"));
            return;
        }
        let (Some(lease), Some(sink), Some(controller), Some(context), Some(mtm)) = (
            lease,
            self.sink.clone(),
            self.controller.borrow().as_ref().and_then(Weak::load),
            session.borrow().context.load(),
            MainThreadMarker::new(),
        ) else {
            self.reply(id, request.id, Err("Offscreen host unavailable"));
            return;
        };
        let Some(store) = (unsafe { controller.configuration().defaultWebsiteDataStore() }) else {
            self.reply(id, request.id, Err("Offscreen profile unavailable"));
            return;
        };
        let Some(base_url) = (unsafe { context.baseURL() })
            .absoluteString()
            .map(|url| url.to_string())
        else {
            self.reply(id, request.id, Err("Invalid extension origin"));
            return;
        };
        let Ok(parsed) = url::Url::parse(&base_url) else {
            self.reply(id, request.id, Err("Invalid extension origin"));
            return;
        };
        let Some(extension_id) = parsed.host_str() else {
            self.reply(id, request.id, Err("Invalid extension origin"));
            return;
        };
        let weak = self.self_weak.clone();
        let ready_id = request.id;
        let on_ready: Box<dyn FnOnce()> = Box::new(move || {
            if let Some(broker) = weak.upgrade() {
                broker.document_ready(id, ready_id);
            }
        });
        let weak = self.self_weak.clone();
        let failed_id = request.id;
        let on_failure: Box<dyn FnOnce()> = Box::new(move || {
            if let Some(broker) = weak.upgrade() {
                broker.document_failed(id, failed_id);
            }
        });
        let weak = self.self_weak.clone();
        let send: OffscreenBackgroundSend = Rc::new(move |value, reply| {
            if let Some(broker) = weak.upgrade() {
                broker.send_document_message(id, value, reply);
            } else {
                reply(Err(()));
            }
        });
        let host = OffscreenHost::open_product(
            mtm,
            store,
            runtime,
            &context,
            &controller,
            extension_id,
            &base_url,
            path,
            Box::new(|_| true),
            sink,
            IsolatedResourceTaskPool::shared(),
            send,
            on_ready,
            on_failure,
        );
        match host {
            Ok(host) => {
                let mut session = session.borrow_mut();
                session.creating = Some(request.id);
                session.document_path = Some(path.into());
                session.document_lease = Some(lease);
                session.document = Some(Rc::new(host));
                drop(session);
                qa_trace("document-opening");
            }
            Err(_) => self.reply(id, request.id, Err("Offscreen document could not start")),
        }
    }

    fn document_ready(&self, id: OffscreenSessionId, request: u64) {
        let Some(session) = self.sessions.borrow().get(&id).cloned() else {
            return;
        };
        let ready = session.borrow_mut().creating.take() == Some(request);
        if ready {
            qa_trace("document-ready");
            self.reply(id, request, Ok(Value::Bool(true)));
        }
    }

    fn document_failed(&self, id: OffscreenSessionId, request: u64) {
        let Some(session) = self.sessions.borrow().get(&id).cloned() else {
            return;
        };
        let creating = session.borrow().creating == Some(request);
        if creating {
            self.close_document(id);
            self.reply(id, request, Err("Offscreen document failed to load"));
        }
    }

    fn close_document(&self, id: OffscreenSessionId) {
        let Some(session) = self.sessions.borrow().get(&id).cloned() else {
            return;
        };
        let (document, lease, replies) = session.borrow_mut().take_document();
        if let Some(document) = document {
            document.shutdown();
            qa_trace("document-closed");
        }
        drop(lease);
        for reply in replies {
            reply(Err(()));
        }
    }

    fn document_owner(&self, runtime: ExtensionRuntimeInstance) -> Option<OffscreenSessionId> {
        self.sessions.borrow().iter().find_map(|(id, session)| {
            let session = session.borrow();
            (session.runtime == Some(runtime) && session.document.is_some()).then_some(*id)
        })
    }

    fn send_document_message(
        &self,
        id: OffscreenSessionId,
        value: Box<[u8]>,
        reply: OffscreenBackgroundReply,
    ) {
        let Some(session) = self.sessions.borrow().get(&id).cloned() else {
            reply(Err(()));
            return;
        };
        let at_capacity = session.borrow().document_replies.len() >= MAX_DOCUMENT_REPLIES;
        if at_capacity || value.len() > MAX_WIRE_BYTES {
            reply(Err(()));
            return;
        }
        let Some(value) = serde_json::from_slice::<Value>(&value).ok() else {
            reply(Err(()));
            return;
        };
        let envelope = (|| {
            let mut session = session.borrow_mut();
            let next = session.next_document_message.checked_add(1)?;
            let context = session.context.load()?;
            let base = (unsafe { context.baseURL() }).absoluteString()?.to_string();
            let path = session.document_path.as_ref()?.clone();
            let extension_id = url::Url::parse(&base).ok()?.host_str()?.to_owned();
            session.next_document_message = next;
            Some((
                next,
                json!({"id":extension_id,"url":format!("{base}{path}")}),
            ))
        })();
        let Some((next, sender)) = envelope else {
            reply(Err(()));
            return;
        };
        session.borrow_mut().document_replies.insert(next, reply);
        qa_trace("document-to-background-requested");
        self.send_wire(
            id,
            json!({"v":1,"id":next,"operation":"fromDocument","value":value,"sender":sender}),
        );
        let weak = self.self_weak.clone();
        if let Ok(when) = dispatch2::DispatchTime::try_from(DOCUMENT_MESSAGE_TIMEOUT) {
            let timeout: RcBlock<dyn Fn()> = RcBlock::new(move || {
                if let Some(broker) = weak.upgrade() {
                    if let Some(session) = broker.sessions.borrow().get(&id).cloned() {
                        let reply = session.borrow_mut().document_replies.remove(&next);
                        if let Some(reply) = reply {
                            reply(Err(()));
                        }
                    }
                }
            });
            unsafe {
                DispatchQueue::exec_after_with_block(
                    when,
                    DispatchQueue::main(),
                    RcBlock::as_ptr(&timeout),
                );
            }
        }
    }

    fn reply(&self, id: OffscreenSessionId, request: u64, value: Result<Value, &'static str>) {
        let frame = match value {
            Ok(value) => json!({"v":1,"id":request,"operation":"result","ok":true,"value":value}),
            Err(error) => json!({"v":1,"id":request,"operation":"result","ok":false,"error":error}),
        };
        self.send_wire(id, frame);
    }

    fn send_wire(&self, id: OffscreenSessionId, frame: Value) {
        let Some(session) = self.sessions.borrow().get(&id).cloned() else {
            return;
        };
        let Some(wire) = serde_json::to_string(&frame)
            .ok()
            .filter(|wire| wire.len() <= MAX_WIRE_BYTES)
        else {
            self.close_session(id);
            return;
        };
        let port = session.borrow().port.clone();
        let weak = self.self_weak.clone();
        let send: RcBlock<dyn Fn()> = RcBlock::new(move || {
            let Some(broker) = weak.upgrade() else {
                return;
            };
            let still_current = broker
                .sessions
                .borrow()
                .get(&id)
                .is_some_and(|session| std::ptr::eq(&*session.borrow().port, &*port));
            if !still_current {
                return;
            }
            let wire = NSString::from_str(&wire);
            let weak_completion = weak.clone();
            let completion: RcBlock<dyn Fn(*mut NSError)> =
                RcBlock::new(move |error: *mut NSError| {
                    if !error.is_null() {
                        if let Some(broker) = weak_completion.upgrade() {
                            broker.close_session(id);
                        }
                    }
                });
            if objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
                port.sendMessage_completionHandler(Some(&wire), Some(&completion));
            }))
            .is_err()
            {
                if let Some(broker) = weak.upgrade() {
                    broker.close_session(id);
                }
            }
        });
        unsafe {
            DispatchQueue::main().exec_async_with_block(RcBlock::as_ptr(&send));
        }
    }

    fn timeout_unauthorized(&self, id: OffscreenSessionId) {
        let Some(session) = self.sessions.borrow().get(&id).cloned() else {
            return;
        };
        if session.borrow().runtime.is_none() {
            self.close_session(id);
        }
    }

    pub(super) fn cancel_context(&self, context: *const WKWebExtensionContext) {
        let ids = self
            .sessions
            .borrow()
            .iter()
            .filter_map(|(id, session)| {
                session
                    .borrow()
                    .context
                    .load()
                    .is_some_and(|candidate| Retained::as_ptr(&candidate) == context)
                    .then_some(*id)
            })
            .collect::<Vec<_>>();
        for id in ids {
            self.close_session(id);
        }
    }

    pub(super) fn settle_resource(
        &self,
        runtime: ExtensionRuntimeInstance,
        request: u64,
        outcome: IsolatedExtensionResourceOutcome,
    ) -> bool {
        let verified = matches!(&outcome, IsolatedExtensionResourceOutcome::Verified(_));
        let document = self.sessions.borrow().values().find_map(|session| {
            let session = session.borrow();
            (session.runtime == Some(runtime))
                .then(|| session.document.clone())
                .flatten()
        });
        let settled =
            document.is_some_and(|document| document.settle_resource(runtime, request, outcome));
        if settled {
            qa_trace(if verified {
                "verified-resource-reply-handled"
            } else {
                "rejected-resource-reply-handled"
            });
        }
        settled
    }

    pub(super) fn resource_context(
        &self,
        runtime: ExtensionRuntimeInstance,
    ) -> Option<*const WKWebExtensionContext> {
        let sessions = self.sessions.borrow();
        let mut matched = None;
        for session in sessions.values() {
            let session = session.borrow();
            if session.runtime != Some(runtime) || session.document.is_none() {
                continue;
            }
            let context = session.context.load()?;
            if matched.replace(Retained::as_ptr(&context)).is_some() {
                return None;
            }
        }
        matched
    }

    pub(super) fn close_session(&self, id: OffscreenSessionId) {
        let Some(session) = self.sessions.borrow_mut().remove(&id) else {
            return;
        };
        let (port, document, lease, replies) = {
            let mut session = session.borrow_mut();
            if session.pending_authorization_counted {
                PENDING_AUTHORIZATION.fetch_sub(1, Ordering::AcqRel);
                session.pending_authorization_counted = false;
            }
            let port = session.port.clone();
            let (document, lease, replies) = session.take_document();
            (port, document, lease, replies)
        };
        unsafe {
            port.setMessageHandler(None);
            port.setDisconnectHandler(None);
        }
        if let Some(document) = document {
            document.shutdown();
        }
        drop(lease);
        for reply in replies {
            reply(Err(()));
        }
        unsafe { port.disconnect() };
    }

    pub(super) fn seal(&self) {
        self.sealed.set(true);
        let ids = self.sessions.borrow().keys().copied().collect::<Vec<_>>();
        for id in ids {
            self.close_session(id);
        }
    }
}

struct NumberMaxSafeInteger;
impl NumberMaxSafeInteger {
    const VALUE: u64 = 9_007_199_254_740_991;
}

fn reject_connection(completion: &DynBlock<dyn Fn(*mut NSError)>) {
    let error = unsafe {
        NSError::errorWithDomain_code_userInfo(
            &NSString::from_str("app.zephium.extension-offscreen"),
            1,
            None,
        )
    };
    completion.call((Retained::as_ptr(&error).cast_mut(),));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_port_capacity_is_bounded_and_released() {
        let leases = (0..MAX_PORTS_GLOBAL)
            .map(|_| PortPermit::reserve().unwrap())
            .collect::<Vec<_>>();
        assert!(PortPermit::reserve().is_none());
        drop(leases);
        assert_eq!(OPEN_PORTS.load(Ordering::Acquire), 0);
    }
}
