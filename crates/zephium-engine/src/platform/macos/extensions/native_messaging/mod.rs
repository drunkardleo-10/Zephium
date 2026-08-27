//! Main-thread authority broker for publisher native messaging.
//!
//! WebKit callbacks carry only untrusted context, host, and JSON values. The
//! broker first retains a bounded pending callback, then asks the Engine host
//! to join that context to one exact published runtime and sealed publisher
//! requirement. Only that authorization may reserve and start the cold stdio
//! worker in [`process`].

mod process;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ffi::CStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::{Rc, Weak as RcWeak};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use dispatch2::DispatchQueue;
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2_foundation::{
    NSData, NSError, NSHomeDirectory, NSJSONReadingOptions, NSJSONSerialization,
    NSJSONWritingOptions, NSString, NSUTF8StringEncoding,
};
use objc2_web_kit::{WKWebExtensionContext, WKWebExtensionController, WKWebExtensionMessagePort};
use zephium_core::extensions::{
    ExtensionPublisherNativeHostRequirement, ExtensionRuntimeInstance,
    MAX_EXTENSION_NATIVE_HOST_CONNECTIONS_PER_PROFILE, MAX_EXTENSION_NATIVE_HOST_NAME_BYTES,
};
use zephium_core::ids::ProfileId;
use zephium_extension_package::{
    encode_native_messaging_frame, parse_bounded_json, BoundedJsonLimits, NativeMessagingHostName,
    MAX_NATIVE_MESSAGING_MESSAGE_BYTES,
};

pub(super) use self::process::NativeHostProcessPool;
pub(crate) use self::process::NativeHostWorkerEvent;
use self::process::{
    spawn_native_host_worker, NativeHostDiscoveryRoots, NativeHostProcessFailure,
    NativeHostSessionControl, OUTBOUND_FRAME_QUEUE_CAPACITY,
};

const AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(5);
const WORKER_EVENT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_AUTHORIZATION_RETRIES: u8 = 8;
const ERROR_DOMAIN: &str = "app.zephium.extension-native-messaging";

type OneShotReply = RcBlock<dyn Fn(*mut AnyObject, *mut NSError)>;
type PortCompletion = RcBlock<dyn Fn(*mut NSError)>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PublisherNativeMessagingRequestId(u64);

impl PublisherNativeMessagingRequestId {
    fn new(value: u64) -> Option<Self> {
        (value != 0).then_some(Self(value))
    }
}

#[derive(Clone)]
pub(crate) struct PublisherNativeMessagingAuthorization {
    runtime: ExtensionRuntimeInstance,
    requirement: ExtensionPublisherNativeHostRequirement,
}

impl PublisherNativeMessagingAuthorization {
    pub(crate) fn new(
        runtime: ExtensionRuntimeInstance,
        requirement: ExtensionPublisherNativeHostRequirement,
    ) -> Self {
        Self {
            runtime,
            requirement,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PublisherNativeMessagingRejection {
    InvalidContext,
    InvalidRequest,
    Unauthorized,
    CapacityExceeded,
    RegistrationUnavailable,
    HostUnavailable,
    MessageInvalid,
    ShuttingDown,
}

enum Endpoint {
    OneShot {
        reply: OneShotReply,
        initial_frame: Option<Box<[u8]>>,
    },
    Port {
        port: Retained<WKWebExtensionMessagePort>,
        completion: Option<PortCompletion>,
    },
}

impl Endpoint {
    fn fail(self, reason: PublisherNativeMessagingRejection) {
        let error = broker_error(reason);
        match self {
            Self::OneShot { reply, .. } => {
                reply.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()))
            }
            Self::Port { port, completion } => {
                unsafe {
                    port.setMessageHandler(None);
                    port.setDisconnectHandler(None);
                }
                if let Some(completion) = completion {
                    completion.call((Retained::as_ptr(&error).cast_mut(),));
                } else if !unsafe { port.isDisconnected() } {
                    unsafe { port.disconnectWithError(Some(&error)) };
                }
            }
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SessionPhase {
    Authorizing,
    Starting,
    Active,
}

struct PendingSession {
    context: Weak<WKWebExtensionContext>,
    host_name: Box<str>,
    endpoint: Endpoint,
    phase: SessionPhase,
    runtime: Option<ExtensionRuntimeInstance>,
    control: Option<NativeHostSessionControl>,
    watchdog: Option<crate::platform::imp::ContentPolicyTimeout>,
    authorization_retries: AuthorizationRetryBudget,
    deferred_port_frames: DeferredPortFrames,
}

struct DeferredPortFrames {
    frames: VecDeque<Box<[u8]>>,
}

impl Default for DeferredPortFrames {
    fn default() -> Self {
        Self {
            frames: VecDeque::with_capacity(OUTBOUND_FRAME_QUEUE_CAPACITY),
        }
    }
}

impl DeferredPortFrames {
    fn try_push(&mut self, frame: Box<[u8]>) -> Result<(), Box<[u8]>> {
        if self.frames.len() >= OUTBOUND_FRAME_QUEUE_CAPACITY {
            return Err(frame);
        }
        self.frames.push_back(frame);
        Ok(())
    }

    fn pop_front(&mut self) -> Option<Box<[u8]>> {
        self.frames.pop_front()
    }
}

#[derive(Default)]
struct AuthorizationRetryBudget {
    used: u8,
}

impl AuthorizationRetryBudget {
    fn reserve(&mut self) -> bool {
        if self.used >= MAX_AUTHORIZATION_RETRIES {
            return false;
        }
        self.used += 1;
        true
    }
}

pub(super) struct PublisherNativeMessagingBroker {
    profile: ProfileId,
    controller: RefCell<Option<Weak<WKWebExtensionController>>>,
    process_pool: Arc<NativeHostProcessPool>,
    self_weak: RcWeak<Self>,
    next_request: Cell<Option<u64>>,
    pending: RefCell<HashMap<PublisherNativeMessagingRequestId, PendingSession>>,
    sealed: Cell<bool>,
}

impl PublisherNativeMessagingBroker {
    pub(super) fn new(profile: ProfileId, process_pool: Arc<NativeHostProcessPool>) -> Rc<Self> {
        Rc::new_cyclic(|self_weak| Self {
            profile,
            controller: RefCell::new(None),
            process_pool,
            self_weak: self_weak.clone(),
            next_request: Cell::new(Some(1)),
            pending: RefCell::new(HashMap::new()),
            sealed: Cell::new(false),
        })
    }

    pub(super) fn new_process_pool() -> Arc<NativeHostProcessPool> {
        Arc::new(NativeHostProcessPool::new(default_discovery_roots()))
    }

    pub(super) fn bind_controller(&self, controller: &Retained<WKWebExtensionController>) {
        *self.controller.borrow_mut() = Some(Weak::from_retained(controller));
    }

    pub(super) fn begin_one_shot(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
        application_identifier: Option<&NSString>,
        message: &AnyObject,
        reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
    ) {
        lab_diagnostic("one-shot-callback");
        let endpoint = match encode_webkit_message(message) {
            Ok(frame) => Endpoint::OneShot {
                reply: reply.copy(),
                initial_frame: Some(frame),
            },
            Err(reason) => {
                complete_one_shot_rejected(reply.copy(), reason);
                return;
            }
        };
        self.begin(controller, context, application_identifier, endpoint);
    }

    pub(super) fn begin_port(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
        port: &WKWebExtensionMessagePort,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        lab_diagnostic("port-callback");
        let Some(port) = (unsafe { Retained::retain(NonNull::from(port).as_ptr()) }) else {
            complete_port_rejected(
                completion.copy(),
                PublisherNativeMessagingRejection::InvalidContext,
            );
            return;
        };
        let application_identifier = unsafe { port.applicationIdentifier() };
        self.begin(
            controller,
            context,
            application_identifier.as_deref(),
            Endpoint::Port {
                port,
                completion: Some(completion.copy()),
            },
        );
    }

    fn begin(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
        application_identifier: Option<&NSString>,
        endpoint: Endpoint,
    ) {
        if self.sealed.get() || !self.accepts(controller, context) {
            endpoint.fail(PublisherNativeMessagingRejection::InvalidContext);
            return;
        }
        let Some(host_name) = parse_host_name(application_identifier) else {
            endpoint.fail(PublisherNativeMessagingRejection::InvalidRequest);
            return;
        };
        if self.pending.borrow().len() >= MAX_EXTENSION_NATIVE_HOST_CONNECTIONS_PER_PROFILE {
            endpoint.fail(PublisherNativeMessagingRejection::CapacityExceeded);
            return;
        }
        let Some(id) = self.allocate_request_id() else {
            endpoint.fail(PublisherNativeMessagingRejection::CapacityExceeded);
            return;
        };
        let Some(context) = retain_weak(context) else {
            endpoint.fail(PublisherNativeMessagingRejection::InvalidContext);
            return;
        };
        let profile = self.profile;
        let Some(watchdog) = crate::platform::imp::schedule_content_policy_timeout(
            AUTHORIZATION_TIMEOUT,
            move || {
                let _ = crate::host::with_extension_browser_request_terminal(move |host| {
                    host.timeout_extension_native_messaging_request(profile, id);
                });
            },
        ) else {
            endpoint.fail(PublisherNativeMessagingRejection::HostUnavailable);
            return;
        };
        self.pending.borrow_mut().insert(
            id,
            PendingSession {
                context,
                host_name,
                endpoint,
                phase: SessionPhase::Authorizing,
                runtime: None,
                control: None,
                watchdog: Some(watchdog),
                authorization_retries: AuthorizationRetryBudget::default(),
                deferred_port_frames: DeferredPortFrames::default(),
            },
        );
        if !self.install_port_handlers(id) {
            self.reject_exact(id, PublisherNativeMessagingRejection::HostUnavailable);
            return;
        }
        if !self.complete_port_connection(id) {
            self.reject_exact(id, PublisherNativeMessagingRejection::HostUnavailable);
            return;
        }
        lab_diagnostic("authorization-staged");
        if !crate::host::with_extension_browser_request_terminal(move |host| {
            host.finalize_extension_native_messaging_request(profile, id);
        }) {
            self.reject_exact(id, PublisherNativeMessagingRejection::HostUnavailable);
        }
    }

    pub(super) fn pending_subject(
        &self,
        id: PublisherNativeMessagingRequestId,
    ) -> Option<(*const WKWebExtensionContext, Box<str>)> {
        let pending = self.pending.borrow();
        let pending = pending.get(&id)?;
        if pending.phase != SessionPhase::Authorizing {
            return None;
        }
        let context = pending.context.load()?;
        Some((Retained::as_ptr(&context), pending.host_name.clone()))
    }

    pub(super) fn reserve_authorization_retry(
        &self,
        id: PublisherNativeMessagingRequestId,
    ) -> bool {
        let mut pending = self.pending.borrow_mut();
        let Some(session) = pending.get_mut(&id) else {
            return false;
        };
        if session.phase != SessionPhase::Authorizing || !session.authorization_retries.reserve() {
            return false;
        }
        lab_diagnostic("authorization-retry-reserved");
        true
    }

    pub(super) fn authorize(
        &self,
        id: PublisherNativeMessagingRequestId,
        authorization: Option<PublisherNativeMessagingAuthorization>,
    ) -> bool {
        lab_diagnostic("authorization-entered");
        let Some(authorization) = authorization else {
            lab_diagnostic("authorization-refused");
            return self.reject_exact(id, PublisherNativeMessagingRejection::Unauthorized);
        };
        let roots = match self.process_pool.roots() {
            Some(roots) => roots,
            None => {
                lab_diagnostic("discovery-roots-unavailable");
                return self.reject_exact(
                    id,
                    PublisherNativeMessagingRejection::RegistrationUnavailable,
                );
            }
        };
        let permit = match self.process_pool.try_reserve() {
            Some(permit) => permit,
            None => {
                lab_diagnostic("process-capacity-refused");
                return self.reject_exact(id, PublisherNativeMessagingRejection::CapacityExceeded);
            }
        };
        {
            let mut pending = self.pending.borrow_mut();
            let Some(session) = pending.get_mut(&id) else {
                return false;
            };
            let current = session.context.load().is_some_and(|context| {
                self.controller
                    .borrow()
                    .as_ref()
                    .and_then(Weak::load)
                    .is_some_and(|controller| unsafe {
                        controller.extensionContexts().containsObject(&context)
                    })
            });
            if session.phase != SessionPhase::Authorizing
                || !current
                || authorization.runtime.profile() != self.profile
                || authorization.requirement.host_name() != &*session.host_name
            {
                drop(pending);
                drop(permit);
                return self.reject_exact(id, PublisherNativeMessagingRejection::Unauthorized);
            }
            session.phase = SessionPhase::Starting;
            session.runtime = Some(authorization.runtime);
        }
        lab_diagnostic("worker-spawn-started");

        let profile = self.profile;
        let control =
            spawn_native_host_worker(authorization.requirement, roots, permit, move |event| {
                dispatch_worker_event(profile, id, event)
            });
        match control {
            Ok(control) => {
                let mut pending = self.pending.borrow_mut();
                let Some(session) = pending.get_mut(&id) else {
                    drop(control);
                    return false;
                };
                if session.phase != SessionPhase::Starting || session.control.is_some() {
                    drop(pending);
                    drop(control);
                    return self
                        .reject_exact(id, PublisherNativeMessagingRejection::HostUnavailable);
                }
                session.control = Some(control);
                lab_diagnostic("worker-spawned");
                true
            }
            Err(reason) => {
                lab_diagnostic("worker-spawn-refused");
                self.reject_exact(id, map_process_failure(reason))
            }
        }
    }

    pub(super) fn handle_worker_event(
        &self,
        id: PublisherNativeMessagingRequestId,
        event: NativeHostWorkerEvent,
    ) -> bool {
        match event {
            NativeHostWorkerEvent::Ready => {
                lab_diagnostic("worker-ready");
                self.handle_ready(id)
            }
            NativeHostWorkerEvent::Message(message) => {
                lab_diagnostic("worker-message");
                self.handle_message(id, &message)
            }
            NativeHostWorkerEvent::Closed(reason) => {
                lab_diagnostic("worker-closed");
                lab_process_failure_diagnostic(reason);
                self.reject_exact(id, map_process_failure(reason))
            }
        }
    }

    fn handle_ready(&self, id: PublisherNativeMessagingRequestId) -> bool {
        enum ReadyEndpoint {
            OneShot,
            Port { completion: Option<PortCompletion> },
        }
        let mut pending = self.pending.borrow_mut();
        let Some(session) = pending.get_mut(&id) else {
            return false;
        };
        if session.phase != SessionPhase::Starting || session.control.is_none() {
            drop(pending);
            return self.reject_exact(id, PublisherNativeMessagingRejection::HostUnavailable);
        }
        session.watchdog.take();
        let endpoint = match &mut session.endpoint {
            Endpoint::OneShot { initial_frame, .. } => {
                let Some(frame) = initial_frame.take() else {
                    drop(pending);
                    return self
                        .reject_exact(id, PublisherNativeMessagingRejection::MessageInvalid);
                };
                if session
                    .control
                    .as_mut()
                    .is_none_or(|control| control.try_send(frame).is_err())
                {
                    drop(pending);
                    return self
                        .reject_exact(id, PublisherNativeMessagingRejection::HostUnavailable);
                }
                ReadyEndpoint::OneShot
            }
            Endpoint::Port { completion, .. } => ReadyEndpoint::Port {
                completion: completion.take(),
            },
        };
        while let Some(frame) = session.deferred_port_frames.pop_front() {
            let rejection = match session.control.as_mut() {
                Some(control) => control.try_send(frame).err().map(map_process_failure),
                None => Some(PublisherNativeMessagingRejection::HostUnavailable),
            };
            if let Some(reason) = rejection {
                drop(pending);
                return self.reject_exact(id, reason);
            }
        }
        session.phase = SessionPhase::Active;
        drop(pending);
        let ReadyEndpoint::Port { completion } = endpoint else {
            return true;
        };
        if let Some(completion) = completion {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                completion.call((std::ptr::null_mut(),));
            }))
            .is_err()
            {
                return self.reject_exact(id, PublisherNativeMessagingRejection::HostUnavailable);
            }
        }
        true
    }

    fn install_port_handlers(&self, id: PublisherNativeMessagingRequestId) -> bool {
        let port = {
            let pending = self.pending.borrow();
            let Some(session) = pending.get(&id) else {
                return false;
            };
            match &session.endpoint {
                Endpoint::OneShot { .. } => return true,
                Endpoint::Port { port, .. } => port.clone(),
            }
        };
        let weak = self.self_weak.clone();
        let message_handler = RcBlock::new(move |message: *mut AnyObject, error: *mut NSError| {
            let Some(broker) = weak.upgrade() else {
                return;
            };
            lab_diagnostic("port-message");
            if !error.is_null() {
                broker.reject_exact(id, PublisherNativeMessagingRejection::MessageInvalid);
                return;
            }
            let Some(message) = (unsafe { message.as_ref() }) else {
                broker.reject_exact(id, PublisherNativeMessagingRejection::MessageInvalid);
                return;
            };
            broker.send_port_message(id, message);
        });
        let weak = self.self_weak.clone();
        let disconnect_handler = RcBlock::new(move |_error: *mut NSError| {
            lab_diagnostic("port-disconnected");
            if let Some(broker) = weak.upgrade() {
                broker.close_port(id);
            }
        });
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            port.setMessageHandler(Some(&message_handler));
            port.setDisconnectHandler(Some(&disconnect_handler));
        }))
        .is_ok()
    }

    fn complete_port_connection(&self, id: PublisherNativeMessagingRequestId) -> bool {
        let completion = {
            let mut pending = self.pending.borrow_mut();
            let Some(session) = pending.get_mut(&id) else {
                return false;
            };
            match &mut session.endpoint {
                Endpoint::OneShot { .. } => return true,
                Endpoint::Port { completion, .. } => completion.take(),
            }
        };
        let Some(completion) = completion else {
            return false;
        };
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            completion.call((std::ptr::null_mut(),));
        }))
        .is_ok()
    }

    fn send_port_message(&self, id: PublisherNativeMessagingRequestId, message: &AnyObject) {
        let frame = match encode_webkit_message(message) {
            Ok(frame) => frame,
            Err(reason) => {
                self.reject_exact(id, reason);
                return;
            }
        };
        let mut pending = self.pending.borrow_mut();
        let Some(session) = pending.get_mut(&id) else {
            return;
        };
        let rejection = match session.phase {
            SessionPhase::Authorizing | SessionPhase::Starting => {
                if !matches!(session.endpoint, Endpoint::Port { .. }) {
                    Some(PublisherNativeMessagingRejection::InvalidRequest)
                } else if session.deferred_port_frames.try_push(frame).is_err() {
                    Some(PublisherNativeMessagingRejection::CapacityExceeded)
                } else {
                    None
                }
            }
            SessionPhase::Active => match session.control.as_mut() {
                Some(control) => control.try_send(frame).err().map(map_process_failure),
                None => Some(PublisherNativeMessagingRejection::HostUnavailable),
            },
        };
        drop(pending);
        if let Some(reason) = rejection {
            self.reject_exact(id, reason);
        }
    }

    fn handle_message(&self, id: PublisherNativeMessagingRequestId, payload: &[u8]) -> bool {
        let object = match decode_webkit_message(payload) {
            Ok(object) => object,
            Err(reason) => return self.reject_exact(id, reason),
        };
        let mut pending = self.pending.borrow_mut();
        let Some(session) = pending.get_mut(&id) else {
            return false;
        };
        if session.phase != SessionPhase::Active {
            drop(pending);
            return self.reject_exact(id, PublisherNativeMessagingRejection::HostUnavailable);
        }
        match &session.endpoint {
            Endpoint::OneShot { .. } => {
                let session = pending.remove(&id).expect("session was present");
                drop(pending);
                let Endpoint::OneShot { reply, .. } = session.endpoint else {
                    unreachable!("matched endpoint changed")
                };
                reply.call((Retained::as_ptr(&object).cast_mut(), std::ptr::null_mut()));
                true
            }
            Endpoint::Port { port, .. } => {
                let port = port.clone();
                drop(pending);
                let weak = self.self_weak.clone();
                let completion = RcBlock::new(move |error: *mut NSError| {
                    if !error.is_null() {
                        if let Some(broker) = weak.upgrade() {
                            broker.reject_exact(
                                id,
                                PublisherNativeMessagingRejection::HostUnavailable,
                            );
                        }
                    }
                });
                let sent = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
                    port.sendMessage_completionHandler(Some(&object), Some(&completion));
                }))
                .is_ok();
                sent || self.reject_exact(id, PublisherNativeMessagingRejection::HostUnavailable)
            }
        }
    }

    fn close_port(&self, id: PublisherNativeMessagingRequestId) {
        let session = self.pending.borrow_mut().remove(&id);
        if let Some(mut session) = session {
            if let Some(control) = session.control.as_mut() {
                control.cancel();
            }
            if let Endpoint::Port { port, .. } = session.endpoint {
                unsafe {
                    port.setMessageHandler(None);
                    port.setDisconnectHandler(None);
                }
            }
        }
    }

    pub(super) fn timeout(&self, id: PublisherNativeMessagingRequestId) -> bool {
        self.reject_exact(id, PublisherNativeMessagingRejection::HostUnavailable)
    }

    pub(super) fn cancel_context(&self, context: *const WKWebExtensionContext) {
        let ids = self
            .pending
            .borrow()
            .iter()
            .filter_map(|(id, pending)| {
                pending
                    .context
                    .load()
                    .is_some_and(|candidate| Retained::as_ptr(&candidate) == context)
                    .then_some(*id)
            })
            .collect::<Vec<_>>();
        for id in ids {
            self.reject_exact(id, PublisherNativeMessagingRejection::InvalidContext);
        }
    }

    fn reject_exact(
        &self,
        id: PublisherNativeMessagingRequestId,
        reason: PublisherNativeMessagingRejection,
    ) -> bool {
        let Some(mut session) = self.pending.borrow_mut().remove(&id) else {
            return false;
        };
        session.watchdog.take();
        if let Some(control) = session.control.as_mut() {
            control.cancel();
        }
        session.endpoint.fail(reason);
        true
    }

    pub(super) fn seal_and_reject(&self) {
        self.sealed.set(true);
        self.controller.borrow_mut().take();
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for (_, mut session) in pending {
            session.watchdog.take();
            if let Some(control) = session.control.as_mut() {
                control.cancel();
            }
            session
                .endpoint
                .fail(PublisherNativeMessagingRejection::ShuttingDown);
        }
    }

    fn accepts(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
    ) -> bool {
        let Some(expected) = self.controller.borrow().as_ref().and_then(Weak::load) else {
            return false;
        };
        std::ptr::eq(&*expected, controller)
            && unsafe { expected.extensionContexts() }.containsObject(context)
    }

    fn allocate_request_id(&self) -> Option<PublisherNativeMessagingRequestId> {
        for _ in 0..=MAX_EXTENSION_NATIVE_HOST_CONNECTIONS_PER_PROFILE {
            let candidate = self.next_request.get()?;
            self.next_request.set(candidate.checked_add(1));
            let candidate = PublisherNativeMessagingRequestId::new(candidate)?;
            if !self.pending.borrow().contains_key(&candidate) {
                return Some(candidate);
            }
        }
        None
    }
}

#[cfg(feature = "native-extension-lab-diagnostics")]
fn lab_diagnostic(phase: &'static str) {
    let elapsed = LAB_RUNTIME_STARTED
        .get()
        .map(std::time::Instant::elapsed)
        .unwrap_or_default();
    crate::diagnostic!(
        "extensions: publisher native messaging lab phase={phase}; runtime-elapsed-ms={}",
        elapsed.as_millis()
    );
}

#[cfg(not(feature = "native-extension-lab-diagnostics"))]
fn lab_diagnostic(_phase: &'static str) {}

#[cfg(feature = "native-extension-lab-diagnostics")]
fn lab_process_failure_diagnostic(reason: NativeHostProcessFailure) {
    crate::diagnostic!("extensions: publisher native messaging worker failure={reason:?}");
}

#[cfg(not(feature = "native-extension-lab-diagnostics"))]
fn lab_process_failure_diagnostic(_reason: NativeHostProcessFailure) {}

#[cfg(feature = "native-extension-lab-diagnostics")]
static LAB_RUNTIME_STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

#[cfg(feature = "native-extension-lab-diagnostics")]
pub(super) fn begin_lab_runtime_timing() {
    let _ = LAB_RUNTIME_STARTED.set(std::time::Instant::now());
}

#[cfg(not(feature = "native-extension-lab-diagnostics"))]
pub(super) fn begin_lab_runtime_timing() {}

impl Drop for PublisherNativeMessagingBroker {
    fn drop(&mut self) {
        self.seal_and_reject();
    }
}

fn dispatch_worker_event(
    profile: ProfileId,
    id: PublisherNativeMessagingRequestId,
    event: NativeHostWorkerEvent,
) -> bool {
    let (acknowledge, acknowledged) = mpsc::sync_channel(0);
    DispatchQueue::main().exec_async(move || {
        let fallback = acknowledge.clone();
        if !crate::host::with_extension_browser_request_terminal(move |host| {
            let handled = host.handle_extension_native_messaging_worker_event(profile, id, event);
            let _ = acknowledge.send(handled);
        }) {
            let _ = fallback.send(false);
        }
    });
    acknowledged
        .recv_timeout(WORKER_EVENT_TIMEOUT)
        .unwrap_or(false)
}

pub(crate) fn schedule_authorization_retry(
    profile: ProfileId,
    id: PublisherNativeMessagingRequestId,
) {
    DispatchQueue::main().exec_async(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::host::with_extension_browser_request_terminal(move |host| {
                host.finalize_extension_native_messaging_request(profile, id);
            })
        }));
    });
}

fn parse_host_name(application_identifier: Option<&NSString>) -> Option<Box<str>> {
    let identifier = application_identifier?;
    if identifier.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
        > MAX_EXTENSION_NATIVE_HOST_NAME_BYTES
    {
        return None;
    }
    objc2::rc::autoreleasepool(|pool| {
        let identifier = unsafe { identifier.to_str(pool) };
        NativeMessagingHostName::parse(identifier)
            .ok()
            .map(|name| Box::<str>::from(name.as_str()))
    })
}

fn encode_webkit_message(
    message: &AnyObject,
) -> Result<Box<[u8]>, PublisherNativeMessagingRejection> {
    let data = unsafe {
        NSJSONSerialization::dataWithJSONObject_options_error(
            message,
            NSJSONWritingOptions::FragmentsAllowed,
        )
    }
    .map_err(|_| PublisherNativeMessagingRejection::MessageInvalid)?;
    if data.is_empty() || data.len() > MAX_NATIVE_MESSAGING_MESSAGE_BYTES {
        return Err(PublisherNativeMessagingRejection::MessageInvalid);
    }
    let parsed = parse_bounded_json(
        unsafe { data.as_bytes_unchecked() },
        BoundedJsonLimits::native_messaging_message(),
    )
    .map_err(|_| PublisherNativeMessagingRejection::MessageInvalid)?;
    encode_native_messaging_frame(parsed.as_value())
        .map(Vec::into_boxed_slice)
        .map_err(|_| PublisherNativeMessagingRejection::MessageInvalid)
}

fn decode_webkit_message(
    payload: &[u8],
) -> Result<Retained<AnyObject>, PublisherNativeMessagingRejection> {
    if payload.is_empty() || payload.len() > MAX_NATIVE_MESSAGING_MESSAGE_BYTES {
        return Err(PublisherNativeMessagingRejection::MessageInvalid);
    }
    parse_bounded_json(payload, BoundedJsonLimits::native_messaging_message())
        .map_err(|_| PublisherNativeMessagingRejection::MessageInvalid)?;
    NSJSONSerialization::JSONObjectWithData_options_error(
        &NSData::with_bytes(payload),
        NSJSONReadingOptions::FragmentsAllowed,
    )
    .map_err(|_| PublisherNativeMessagingRejection::MessageInvalid)
}

fn retain_weak(context: &WKWebExtensionContext) -> Option<Weak<WKWebExtensionContext>> {
    let retained = unsafe {
        Retained::retain(context as *const WKWebExtensionContext as *mut WKWebExtensionContext)
    }?;
    Some(Weak::from_retained(&retained))
}

fn default_discovery_roots() -> Option<NativeHostDiscoveryRoots> {
    let home = NSHomeDirectory();
    let bytes = unsafe { CStr::from_ptr(home.fileSystemRepresentation().as_ptr()) }.to_bytes();
    let home = PathBuf::from(std::ffi::OsStr::from_bytes(bytes));
    NativeHostDiscoveryRoots::macos_default(&home)
}

fn map_process_failure(reason: NativeHostProcessFailure) -> PublisherNativeMessagingRejection {
    match reason {
        NativeHostProcessFailure::RegistrationUnavailable => {
            PublisherNativeMessagingRejection::RegistrationUnavailable
        }
        NativeHostProcessFailure::RegistrationInvalid
        | NativeHostProcessFailure::RegistrationUnauthorized
        | NativeHostProcessFailure::ExecutableInvalid
        | NativeHostProcessFailure::SignatureInvalid => {
            PublisherNativeMessagingRejection::Unauthorized
        }
        NativeHostProcessFailure::MessageInvalid => {
            PublisherNativeMessagingRejection::MessageInvalid
        }
        NativeHostProcessFailure::QueueExceeded => {
            PublisherNativeMessagingRejection::CapacityExceeded
        }
        NativeHostProcessFailure::SpawnFailed
        | NativeHostProcessFailure::TransportFailed
        | NativeHostProcessFailure::Cancelled
        | NativeHostProcessFailure::Exited => PublisherNativeMessagingRejection::HostUnavailable,
    }
}

fn complete_one_shot_rejected(reply: OneShotReply, reason: PublisherNativeMessagingRejection) {
    let error = broker_error(reason);
    reply.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
}

fn complete_port_rejected(completion: PortCompletion, reason: PublisherNativeMessagingRejection) {
    let error = broker_error(reason);
    completion.call((Retained::as_ptr(&error).cast_mut(),));
}

fn broker_error(reason: PublisherNativeMessagingRejection) -> Retained<NSError> {
    let code = match reason {
        PublisherNativeMessagingRejection::InvalidContext => 1,
        PublisherNativeMessagingRejection::InvalidRequest => 2,
        PublisherNativeMessagingRejection::Unauthorized => 3,
        PublisherNativeMessagingRejection::CapacityExceeded => 4,
        PublisherNativeMessagingRejection::RegistrationUnavailable => 5,
        PublisherNativeMessagingRejection::HostUnavailable => 6,
        PublisherNativeMessagingRejection::MessageInvalid => 7,
        PublisherNativeMessagingRejection::ShuttingDown => 8,
    };
    unsafe { NSError::errorWithDomain_code_userInfo(&NSString::from_str(ERROR_DOMAIN), code, None) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_failures_map_without_exposing_native_details() {
        assert_eq!(
            map_process_failure(NativeHostProcessFailure::SignatureInvalid),
            PublisherNativeMessagingRejection::Unauthorized
        );
        assert_eq!(
            map_process_failure(NativeHostProcessFailure::RegistrationUnavailable),
            PublisherNativeMessagingRejection::RegistrationUnavailable
        );
        assert_eq!(
            map_process_failure(NativeHostProcessFailure::QueueExceeded),
            PublisherNativeMessagingRejection::CapacityExceeded
        );
    }

    #[test]
    fn request_ids_never_accept_or_wrap_zero() {
        assert!(PublisherNativeMessagingRequestId::new(0).is_none());
        assert_eq!(
            PublisherNativeMessagingRequestId::new(1),
            Some(PublisherNativeMessagingRequestId(1))
        );
    }

    #[test]
    fn authorization_retry_budget_is_exact_and_never_wraps() {
        let mut retries = AuthorizationRetryBudget::default();
        for _ in 0..MAX_AUTHORIZATION_RETRIES {
            assert!(retries.reserve());
        }
        assert_eq!(retries.used, MAX_AUTHORIZATION_RETRIES);
        assert!(!retries.reserve());
    }

    #[test]
    fn deferred_port_frames_are_bounded_and_preserve_wire_order() {
        let mut frames = DeferredPortFrames::default();
        for value in 0..OUTBOUND_FRAME_QUEUE_CAPACITY {
            assert!(frames
                .try_push(vec![value as u8].into_boxed_slice())
                .is_ok());
        }
        assert_eq!(
            frames.try_push(vec![0xff].into_boxed_slice()),
            Err(vec![0xff].into_boxed_slice())
        );
        for value in 0..OUTBOUND_FRAME_QUEUE_CAPACITY {
            assert_eq!(frames.pop_front().as_deref(), Some(&[value as u8][..]));
        }
        assert!(frames.pop_front().is_none());
    }
}
