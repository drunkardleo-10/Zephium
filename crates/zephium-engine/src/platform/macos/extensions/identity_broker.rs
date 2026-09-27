//! One-shot, on-demand noninteractive WebAuth flow for authenticated CRX IDs.
//!
//! The remote document runs in a fresh WebView configuration with the exact
//! originating profile data store and no WebExtension controller. The request
//! is joined to a published runtime and granted `identity` authority before
//! this broker creates that view. A callback URL is canceled at navigation
//! policy time and returned only to the original native reply.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak as RcWeak};
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
#[cfg(feature = "native-web-extension-probes")]
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{
    MainThreadMarker, NSError, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURLRequest,
    NSUTF8StringEncoding, NSURL,
};
use objc2_web_kit::{
    WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate,
    WKWebExtensionContext, WKWebExtensionController, WKWebView, WKWebViewConfiguration,
};
use serde_json::Value;
use zephium_core::extensions::{
    ExtensionCompatibilityBrokerPurpose, ExtensionCompatibilityBrokerWitness,
};
use zephium_core::ids::ProfileId;
use zephium_extension_package::identity_compatibility::{
    IdentityFlow, IdentityFlowDecision, IdentityFlowFailure, MAX_IDENTITY_URL_BYTES,
};
use zephium_extension_package::{ChromiumManifestKeyDigest, ExpectedChromiumIdentity};

use crate::host::NativeResourceLease;

const APPLICATION_ID: &str = "app.zephium.extension-identity.v1";
const ERROR_DOMAIN: &str = "app.zephium.extension-identity";
const QUICK_AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_FLOW_TIMEOUT_MS: u64 = 60_000;
const MAX_WIRE_BYTES: usize = 12 * 1024;

type Reply = RcBlock<dyn Fn(*mut AnyObject, *mut NSError)>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct IdentityRequestId(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IdentityError {
    InvalidContext,
    InvalidRequest,
    Unauthorized,
    UnsupportedInteractive,
    Capacity,
    Unavailable,
    NoRedirect,
    Canceled,
    TimedOut,
}

struct LaunchRequest {
    url: Box<str>,
    interactive: bool,
    abort_on_load: bool,
    timeout: Duration,
}

struct Pending {
    id: IdentityRequestId,
    context: Weak<WKWebExtensionContext>,
    request: LaunchRequest,
    reply: Reply,
    watchdog: crate::platform::imp::ContentPolicyTimeout,
}

struct Active {
    id: IdentityRequestId,
    context: Weak<WKWebExtensionContext>,
    controller: Weak<WKWebExtensionController>,
    flow: IdentityFlow,
    reply: Reply,
    webview: Retained<WKWebView>,
    _delegate: Retained<IdentityNavigationDelegate>,
    watchdog: crate::platform::imp::ContentPolicyTimeout,
    _lease: NativeResourceLease,
}

struct NavigationDelegateIvars {
    broker: RcWeak<IdentityBroker>,
    request: IdentityRequestId,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionIdentityNavigationDelegate"]
    #[ivars = NavigationDelegateIvars]
    struct IdentityNavigationDelegate;

    unsafe impl NSObjectProtocol for IdentityNavigationDelegate {}

    unsafe impl WKNavigationDelegate for IdentityNavigationDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        unsafe fn decide_navigation(
            &self,
            webview: &WKWebView,
            action: &WKNavigationAction,
            decision: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let Some(broker) = self.ivars().broker.upgrade() else {
                decision.call((WKNavigationActionPolicy::Cancel,));
                return;
            };
            let Some(url) = action.request().URL().and_then(|url| url.absoluteString()) else {
                decision.call((WKNavigationActionPolicy::Cancel,));
                broker.reject(self.ivars().request, IdentityError::InvalidRequest);
                return;
            };
            let main_frame =
                unsafe { action.targetFrame() }.is_some_and(|frame| frame.isMainFrame());
            let classification = objc2::rc::autoreleasepool(|pool| {
                broker.navigation(
                    self.ivars().request,
                    webview,
                    unsafe { url.to_str(pool) },
                    main_frame,
                )
            });
            decision.call((if classification.0 {
                WKNavigationActionPolicy::Allow
            } else {
                WKNavigationActionPolicy::Cancel
            },));
            if let Some(result) = classification.1 {
                broker.finish(self.ivars().request, result);
            }
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        unsafe fn did_finish_navigation(
            &self,
            webview: &WKWebView,
            _navigation: Option<&WKNavigation>,
        ) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.page_loaded(self.ivars().request, webview);
            }
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        unsafe fn did_fail_provisional_navigation(
            &self,
            webview: &WKWebView,
            _navigation: Option<&WKNavigation>,
            _error: &NSError,
        ) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.fail_navigation(self.ivars().request, webview);
            }
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn process_terminated(&self, webview: &WKWebView) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.fail_navigation(self.ivars().request, webview);
            }
        }
    }
);

impl IdentityNavigationDelegate {
    fn new(
        mtm: MainThreadMarker,
        broker: RcWeak<IdentityBroker>,
        request: IdentityRequestId,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(NavigationDelegateIvars { broker, request });
        unsafe { msg_send![super(object), init] }
    }
}

/// One profile controller's pending or active identity flow. It owns no view
/// while idle, and admits at most one pending callback and one active view.
pub(super) struct IdentityBroker {
    profile: ProfileId,
    controller: RefCell<Option<Weak<WKWebExtensionController>>>,
    pending: RefCell<Option<Pending>>,
    active: RefCell<Option<Active>>,
    next_id: Cell<u64>,
    sealed: Cell<bool>,
}

impl IdentityBroker {
    pub(super) fn new(profile: ProfileId) -> Rc<Self> {
        Rc::new(Self {
            profile,
            controller: RefCell::new(None),
            pending: RefCell::new(None),
            active: RefCell::new(None),
            next_id: Cell::new(1),
            sealed: Cell::new(false),
        })
    }

    pub(super) fn bind_controller(&self, controller: &Retained<WKWebExtensionController>) {
        *self.controller.borrow_mut() = Some(Weak::from_retained(controller));
    }

    pub(super) fn begin(
        self: &Rc<Self>,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
        message: &AnyObject,
        reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
    ) {
        if self.sealed.get() || !self.accepts(controller, context) {
            complete_error(reply.copy(), IdentityError::InvalidContext);
            return;
        }
        if self.pending.borrow().is_some() || self.active.borrow().is_some() {
            complete_error(reply.copy(), IdentityError::Capacity);
            return;
        }
        let Some(message) = message.downcast_ref::<NSString>() else {
            complete_error(reply.copy(), IdentityError::InvalidRequest);
            return;
        };
        let request = objc2::rc::autoreleasepool(|pool| {
            (message.lengthOfBytesUsingEncoding(NSUTF8StringEncoding) <= MAX_WIRE_BYTES)
                .then(|| parse_launch(unsafe { message.to_str(pool) }))
                .flatten()
        });
        let Some(request) = request else {
            complete_error(reply.copy(), IdentityError::InvalidRequest);
            return;
        };
        if request.interactive {
            complete_error(reply.copy(), IdentityError::UnsupportedInteractive);
            return;
        }
        let Some(id) = self.next_id.get().checked_add(1).map(|next| {
            let id = IdentityRequestId(self.next_id.get());
            self.next_id.set(next);
            id
        }) else {
            complete_error(reply.copy(), IdentityError::Capacity);
            return;
        };
        let Some(context) = retain_weak(context) else {
            complete_error(reply.copy(), IdentityError::InvalidContext);
            return;
        };
        let profile = self.profile;
        let Some(watchdog) =
            crate::platform::imp::schedule_content_policy_timeout(QUICK_AUTH_TIMEOUT, move || {
                let _ = crate::host::with_extension_browser_request_terminal(move |host| {
                    host.timeout_extension_identity_request(profile, id);
                });
            })
        else {
            complete_error(reply.copy(), IdentityError::Unavailable);
            return;
        };
        *self.pending.borrow_mut() = Some(Pending {
            id,
            context,
            request,
            reply: reply.copy(),
            watchdog,
        });
        if !crate::host::with_extension_browser_request_terminal(move |host| {
            host.finalize_extension_identity_request(profile, id);
        }) {
            self.reject(id, IdentityError::Unavailable);
        }
    }

    fn accepts(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
    ) -> bool {
        self.controller
            .borrow()
            .as_ref()
            .and_then(Weak::load)
            .is_some_and(|expected| {
                std::ptr::eq(&*expected, controller)
                    && unsafe { expected.extensionContexts() }.containsObject(context)
            })
    }

    pub(super) fn pending_context(
        &self,
        id: IdentityRequestId,
    ) -> Option<*const WKWebExtensionContext> {
        let pending = self.pending.borrow();
        let pending = pending.as_ref().filter(|pending| pending.id == id)?;
        pending
            .context
            .load()
            .map(|context| Retained::as_ptr(&context))
    }

    pub(super) fn finalize(
        self: &Rc<Self>,
        id: IdentityRequestId,
        witness: Option<ExtensionCompatibilityBrokerWitness>,
        lease: Option<NativeResourceLease>,
    ) -> bool {
        let Some(pending) = self
            .pending
            .borrow_mut()
            .take()
            .filter(|pending| pending.id == id)
        else {
            return false;
        };
        drop(pending.watchdog);
        let Some(witness) = witness.filter(|witness| {
            witness.purpose() == ExtensionCompatibilityBrokerPurpose::IdentityWebAuthFlow
                && witness.runtime_instance().profile() == self.profile
        }) else {
            complete_error(pending.reply, IdentityError::Unauthorized);
            return true;
        };
        let Some(lease) = lease else {
            complete_error(pending.reply, IdentityError::Capacity);
            return true;
        };
        let Some(controller) = self.controller.borrow().as_ref().and_then(Weak::load) else {
            complete_error(pending.reply, IdentityError::InvalidContext);
            return true;
        };
        let Some(context) = pending
            .context
            .load()
            .filter(|context| unsafe { controller.extensionContexts() }.containsObject(context))
        else {
            complete_error(pending.reply, IdentityError::InvalidContext);
            return true;
        };
        let key = witness.runtime().package().key().bytes();
        let chromium_id = ExpectedChromiumIdentity::from_manifest_key_digest(
            ChromiumManifestKeyDigest::from_bytes(key),
        )
        .extension_id()
        .clone();
        let flow = match IdentityFlow::new(
            chromium_id,
            &pending.request.url,
            false,
            pending.request.abort_on_load,
        ) {
            Ok(flow) => flow,
            Err(_) => {
                complete_error(pending.reply, IdentityError::InvalidRequest);
                return true;
            }
        };
        let Some(mtm) = MainThreadMarker::new() else {
            complete_error(pending.reply, IdentityError::Unavailable);
            return true;
        };
        let Some(origin_configuration) = (unsafe { context.webViewConfiguration() }) else {
            complete_error(pending.reply, IdentityError::Unavailable);
            return true;
        };
        let profile_store = unsafe { origin_configuration.websiteDataStore() };
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        unsafe { configuration.setWebsiteDataStore(&profile_store) };
        if unsafe { configuration.webExtensionController() }.is_some()
            || !std::ptr::eq(
                &*unsafe { configuration.websiteDataStore() },
                &*profile_store,
            )
        {
            complete_error(pending.reply, IdentityError::Unavailable);
            return true;
        }
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(32.0, 32.0));
        let webview = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &configuration)
        };
        let delegate = IdentityNavigationDelegate::new(mtm, Rc::downgrade(self), id);
        unsafe {
            webview
                .setNavigationDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*delegate)))
        };
        let timeout = pending.request.timeout;
        let profile = self.profile;
        let Some(watchdog) =
            crate::platform::imp::schedule_content_policy_timeout(timeout, move || {
                let _ = crate::host::with_extension_browser_request_terminal(move |host| {
                    host.timeout_extension_identity_request(profile, id);
                });
            })
        else {
            unsafe { webview.setNavigationDelegate(None) };
            complete_error(pending.reply, IdentityError::Unavailable);
            return true;
        };
        let url = NSString::from_str(&pending.request.url);
        let url = NSURL::URLWithString(&url);
        let Some(url) = url else {
            unsafe { webview.setNavigationDelegate(None) };
            complete_error(pending.reply, IdentityError::InvalidRequest);
            return true;
        };
        *self.active.borrow_mut() = Some(Active {
            id,
            context: Weak::from_retained(&context),
            controller: Weak::from_retained(&controller),
            flow,
            reply: pending.reply,
            webview: webview.clone(),
            _delegate: delegate,
            watchdog,
            _lease: lease,
        });
        let request = NSURLRequest::requestWithURL(&url);
        if unsafe { webview.loadRequest(&request) }.is_none() {
            self.reject(id, IdentityError::Unavailable);
        }
        true
    }

    fn navigation(
        &self,
        id: IdentityRequestId,
        webview: &WKWebView,
        url: &str,
        main_frame: bool,
    ) -> (bool, Option<Result<Box<str>, IdentityError>>) {
        let mut active = self.active.borrow_mut();
        let Some(active) = active
            .as_mut()
            .filter(|active| active.id == id && std::ptr::eq(&*active.webview, webview))
        else {
            return (false, None);
        };
        if !self.active_context_current(active) {
            return (false, Some(Err(IdentityError::InvalidContext)));
        }
        match active.flow.navigation(url, main_frame) {
            IdentityFlowDecision::Allow => (true, None),
            IdentityFlowDecision::CancelNavigation | IdentityFlowDecision::Stale => (false, None),
            IdentityFlowDecision::Resolve(url) => (false, Some(Ok(url.into()))),
            IdentityFlowDecision::Reject(reason) => (false, Some(Err(map_failure(reason)))),
            IdentityFlowDecision::Present => {
                (false, Some(Err(IdentityError::UnsupportedInteractive)))
            }
        }
    }

    fn page_loaded(&self, id: IdentityRequestId, webview: &WKWebView) {
        let result = {
            let mut active = self.active.borrow_mut();
            active
                .as_mut()
                .filter(|active| active.id == id && std::ptr::eq(&*active.webview, webview))
                .and_then(|active| {
                    if !self.active_context_current(active) {
                        Some(IdentityError::InvalidContext)
                    } else if matches!(active.flow.page_loaded(), IdentityFlowDecision::Reject(_)) {
                        Some(IdentityError::NoRedirect)
                    } else {
                        None
                    }
                })
        };
        if let Some(error) = result {
            self.reject(id, error);
        }
    }

    fn fail_navigation(&self, id: IdentityRequestId, webview: &WKWebView) {
        let owns = self
            .active
            .borrow()
            .as_ref()
            .is_some_and(|active| active.id == id && std::ptr::eq(&*active.webview, webview));
        if owns {
            self.reject(id, IdentityError::Unavailable);
        }
    }

    fn active_context_current(&self, active: &Active) -> bool {
        active
            .context
            .load()
            .zip(active.controller.load())
            .is_some_and(|(context, controller)| {
                unsafe { controller.extensionContexts() }.containsObject(&context)
                    && self
                        .controller
                        .borrow()
                        .as_ref()
                        .and_then(Weak::load)
                        .is_some_and(|owner| std::ptr::eq(&*owner, &*controller))
            })
    }

    pub(super) fn timeout(&self, id: IdentityRequestId) -> bool {
        self.reject(id, IdentityError::TimedOut)
    }

    pub(super) fn cancel_context(&self, context: *const WKWebExtensionContext) {
        let pending = self.pending.borrow().as_ref().is_some_and(|pending| {
            pending
                .context
                .load()
                .is_some_and(|candidate| Retained::as_ptr(&candidate) == context)
        });
        if pending {
            if let Some(id) = self.pending.borrow().as_ref().map(|pending| pending.id) {
                self.reject(id, IdentityError::Canceled);
            }
        }
        let active = self.active.borrow().as_ref().is_some_and(|active| {
            active
                .context
                .load()
                .is_some_and(|candidate| Retained::as_ptr(&candidate) == context)
        });
        if active {
            if let Some(id) = self.active.borrow().as_ref().map(|active| active.id) {
                self.reject(id, IdentityError::Canceled);
            }
        }
    }

    fn reject(&self, id: IdentityRequestId, error: IdentityError) -> bool {
        self.finish(id, Err(error))
    }

    fn finish(&self, id: IdentityRequestId, result: Result<Box<str>, IdentityError>) -> bool {
        let pending_id = self.pending.borrow().as_ref().map(|pending| pending.id);
        if pending_id == Some(id) {
            if let Some(pending) = self.pending.borrow_mut().take() {
                drop(pending.watchdog);
                complete_result(pending.reply, result);
                return true;
            }
        }
        let active_id = self.active.borrow().as_ref().map(|active| active.id);
        if active_id == Some(id) {
            if let Some(active) = self.active.borrow_mut().take() {
                drop(active.watchdog);
                unsafe {
                    active.webview.setNavigationDelegate(None);
                    active.webview.stopLoading();
                }
                // Keep delegate, view and lease alive through the terminal
                // reply, then release them together on this main-thread turn.
                complete_result(active.reply, result);
                return true;
            }
        }
        false
    }

    pub(super) fn seal_and_reject(&self) {
        self.sealed.set(true);
        self.controller.borrow_mut().take();
        if let Some(id) = self.pending.borrow().as_ref().map(|pending| pending.id) {
            self.reject(id, IdentityError::Canceled);
        }
        if let Some(id) = self.active.borrow().as_ref().map(|active| active.id) {
            self.reject(id, IdentityError::Canceled);
        }
    }
}

impl Drop for IdentityBroker {
    fn drop(&mut self) {
        self.seal_and_reject();
    }
}

fn retain_weak(context: &WKWebExtensionContext) -> Option<Weak<WKWebExtensionContext>> {
    let retained = unsafe { Retained::retain(context as *const _ as *mut _) }?;
    Some(Weak::from_retained(&retained))
}

fn parse_launch(message: &str) -> Option<LaunchRequest> {
    let value: Value = serde_json::from_str(message).ok()?;
    let object = value.as_object()?;
    if object.len() != 6
        || object.get("v")?.as_u64()? != 1
        || object.get("kind")?.as_str()? != "identity.launch"
    {
        return None;
    }
    let interactive = object.get("interactive")?.as_bool()?;
    let url = object.get("url")?.as_str()?;
    if url.is_empty() || url.len() > MAX_IDENTITY_URL_BYTES {
        return None;
    }
    let abort_on_load = object.get("abortOnLoadForNonInteractive")?.as_bool()?;
    let timeout_ms = object.get("timeoutMsForNonInteractive")?.as_u64()?;
    if timeout_ms == 0 || timeout_ms > MAX_FLOW_TIMEOUT_MS {
        return None;
    }
    Some(LaunchRequest {
        url: url.into(),
        interactive,
        abort_on_load,
        timeout: Duration::from_millis(timeout_ms),
    })
}

fn map_failure(reason: IdentityFlowFailure) -> IdentityError {
    match reason {
        IdentityFlowFailure::UnsafeNavigation => IdentityError::InvalidRequest,
        IdentityFlowFailure::RedirectNotReached => IdentityError::NoRedirect,
        IdentityFlowFailure::WindowClosed | IdentityFlowFailure::ContextGone => {
            IdentityError::Canceled
        }
        IdentityFlowFailure::TimedOut => IdentityError::TimedOut,
    }
}

fn complete_result(reply: Reply, result: Result<Box<str>, IdentityError>) {
    match result {
        Ok(url) => {
            let response = NSString::from_str(&url);
            reply.call((
                Retained::as_ptr(&response).cast_mut().cast(),
                std::ptr::null_mut(),
            ));
        }
        Err(error) => complete_error(reply, error),
    }
}

fn complete_error(reply: Reply, error: IdentityError) {
    let code = match error {
        IdentityError::InvalidContext => 1,
        IdentityError::InvalidRequest => 2,
        IdentityError::Unauthorized => 3,
        IdentityError::UnsupportedInteractive => 4,
        IdentityError::Capacity => 5,
        IdentityError::Unavailable => 6,
        IdentityError::NoRedirect => 7,
        IdentityError::Canceled => 8,
        IdentityError::TimedOut => 9,
    };
    let error = unsafe {
        NSError::errorWithDomain_code_userInfo(&NSString::from_str(ERROR_DOMAIN), code, None)
    };
    reply.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
}

pub(super) fn matches_application_identifier(identifier: &NSString) -> bool {
    identifier.isEqualToString(&NSString::from_str(APPLICATION_ID))
}

#[cfg(feature = "native-web-extension-probes")]
mod probe {
    use super::*;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::{NSDate, NSRunLoop};
    use objc2_web_kit::WKWebsiteDataStore;

    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    struct ProbeObservation {
        attempted: Cell<usize>,
        captured: RefCell<Option<String>>,
        rejected: Cell<usize>,
    }

    struct ProbeDelegateIvars {
        observation: Rc<ProbeObservation>,
        flow: RefCell<IdentityFlow>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "ZephiumIdentityRedirectProbeDelegate"]
        #[ivars = ProbeDelegateIvars]
        struct ProbeDelegate;

        unsafe impl NSObjectProtocol for ProbeDelegate {}

        unsafe impl WKNavigationDelegate for ProbeDelegate {
            #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
            unsafe fn decide_navigation(
                &self,
                _webview: &WKWebView,
                action: &WKNavigationAction,
                decision: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
            ) {
                self.ivars()
                    .observation
                    .attempted
                    .set(self.ivars().observation.attempted.get() + 1);
                let Some(url) = action.request().URL().and_then(|url| url.absoluteString()) else {
                    self.ivars()
                        .observation
                        .rejected
                        .set(self.ivars().observation.rejected.get() + 1);
                    decision.call((WKNavigationActionPolicy::Cancel,));
                    return;
                };
                let main_frame =
                    unsafe { action.targetFrame() }.is_some_and(|frame| frame.isMainFrame());
                let (allow, captured) = objc2::rc::autoreleasepool(|pool| {
                    match self
                        .ivars()
                        .flow
                        .borrow_mut()
                        .navigation(unsafe { url.to_str(pool) }, main_frame)
                    {
                        IdentityFlowDecision::Allow => (true, None),
                        IdentityFlowDecision::Resolve(url) => (false, Some(url.to_owned())),
                        _ => (false, None),
                    }
                });
                match (allow, captured) {
                    (_, Some(url)) => {
                        *self.ivars().observation.captured.borrow_mut() = Some(url);
                        decision.call((WKNavigationActionPolicy::Cancel,));
                    }
                    (true, None) => decision.call((WKNavigationActionPolicy::Allow,)),
                    (false, None) => {
                        self.ivars()
                            .observation
                            .rejected
                            .set(self.ivars().observation.rejected.get() + 1);
                        decision.call((WKNavigationActionPolicy::Cancel,));
                    }
                }
            }
        }
    );

    fn run_case(
        mtm: MainThreadMarker,
        run_loop: &NSRunLoop,
        url: &str,
        expect_capture: bool,
        attach_window: bool,
    ) -> Result<(), String> {
        let (weak_view, weak_delegate) = objc2::rc::autoreleasepool(
            |_pool| -> Result<(Weak<WKWebView>, Weak<ProbeDelegate>), String> {
                let id = zephium_extension_package::ChromiumExtensionId::parse(ID)
                    .map_err(|_| "invalid synthetic Chromium ID")?;
                let flow =
                    IdentityFlow::new(id, "https://accounts.example.test/login", false, false)
                        .map_err(|_| "invalid synthetic launch URL")?;
                let observation = Rc::new(ProbeObservation {
                    attempted: Cell::new(0),
                    captured: RefCell::new(None),
                    rejected: Cell::new(0),
                });
                let delegate = unsafe {
                    msg_send![
                        super(ProbeDelegate::alloc(mtm).set_ivars(ProbeDelegateIvars {
                            observation: observation.clone(),
                            flow: RefCell::new(flow),
                        })),
                        init
                    ]
                };
                let delegate: Retained<ProbeDelegate> = delegate;
                let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
                let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
                unsafe { configuration.setWebsiteDataStore(&store) };
                if unsafe { configuration.webExtensionController() }.is_some() {
                    return Err("synthetic auth view inherited an extension controller".into());
                }
                let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(32.0, 32.0));
                let view = unsafe {
                    WKWebView::initWithFrame_configuration(
                        WKWebView::alloc(mtm),
                        frame,
                        &configuration,
                    )
                };
                let hidden_window = attach_window.then(|| unsafe {
                    NSWindow::initWithContentRect_styleMask_backing_defer(
                        NSWindow::alloc(mtm),
                        frame,
                        NSWindowStyleMask::Borderless,
                        NSBackingStoreType::Buffered,
                        false,
                    )
                });
                if let Some(hidden_window) = hidden_window.as_ref() {
                    unsafe { hidden_window.setReleasedWhenClosed(false) };
                    hidden_window.setAlphaValue(0.0);
                    hidden_window.setIgnoresMouseEvents(true);
                    hidden_window.setContentView(Some(&view));
                    hidden_window.orderFrontRegardless();
                }
                let weak_view = Weak::from_retained(&view);
                let weak_delegate = Weak::from_retained(&delegate);
                unsafe {
                    view.setNavigationDelegate(Some(objc2::runtime::ProtocolObject::from_ref(
                        &*delegate,
                    )))
                };
                let native_url = NSURL::URLWithString(&NSString::from_str(url))
                    .ok_or("synthetic redirect URL could not form NSURL")?;
                let request = NSURLRequest::requestWithURL(&native_url);
                if unsafe { view.loadRequest(&request) }.is_none() {
                    return Err("synthetic redirect navigation did not start".into());
                }
                for _ in 0..200 {
                    if observation.attempted.get() > 0 {
                        break;
                    }
                    run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
                }
                let captured = observation.captured.borrow().clone();
                if observation.attempted.get() == 0
                    || expect_capture != captured.is_some()
                    || (expect_capture && captured.as_deref() != Some(url))
                    || (!expect_capture && observation.rejected.get() == 0)
                {
                    return Err(format!(
                "synthetic redirect navigation did not obey capture policy: attempted={} captured={} rejected={}",
                observation.attempted.get(), captured.is_some(), observation.rejected.get(),
            ));
                }
                unsafe {
                    view.setNavigationDelegate(None);
                    view.stopLoading();
                }
                if let Some(hidden_window) = hidden_window.as_ref() {
                    hidden_window.setContentView(None);
                    hidden_window.orderOut(None);
                    hidden_window.close();
                }
                drop(view);
                drop(delegate);
                drop(hidden_window);
                Ok((weak_view, weak_delegate))
            },
        )?;
        for _ in 0..200 {
            if weak_view.load().is_none() && weak_delegate.load().is_none() {
                break;
            }
            run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        }
        let view_alive = weak_view.load().is_some();
        let delegate_alive = weak_delegate.load().is_some();
        if view_alive || delegate_alive {
            return Err(format!("synthetic auth native objects survived teardown: view={view_alive} delegate={delegate_alive}"));
        }
        Ok(())
    }

    pub(crate) fn run(attach_window: bool) -> Result<bool, String> {
        let mtm = MainThreadMarker::new().ok_or("identity probe needs the main thread")?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let run_loop = NSRunLoop::mainRunLoop();
        run_case(
            mtm,
            &run_loop,
            &format!("https://{ID}.chromiumapp.org/cb?code=a%2Fb#state=opaque"),
            true,
            attach_window,
        )?;
        run_case(
            mtm,
            &run_loop,
            "https://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.chromiumapp.org/cb?code=foreign",
            false,
            attach_window,
        )?;
        println!("native-probe: identity redirect exact_capture=passed; foreign_redirect=blocked; extension_controller=absent; ephemeral_store=passed; teardown=passed; attached_window={attach_window}; product_authority=false");
        Ok(true)
    }
}

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn run_identity_redirect_probe(attach_window: bool) -> Result<bool, String> {
    probe::run(attach_window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_wire_is_closed_bounded_and_keeps_interactive_explicit() {
        let wire = r#"{"v":1,"kind":"identity.launch","url":"https://accounts.example.test/login","interactive":false,"abortOnLoadForNonInteractive":false,"timeoutMsForNonInteractive":30000}"#;
        let parsed = parse_launch(wire).unwrap();
        assert_eq!(&*parsed.url, "https://accounts.example.test/login");
        assert!(!parsed.interactive);
        assert!(!parsed.abort_on_load);
        assert_eq!(parsed.timeout, Duration::from_secs(30));
        assert!(parse_launch(&wire.replace("30000", "0")).is_none());
        assert!(parse_launch(&wire.replace("30000", "60001")).is_none());
        assert!(parse_launch(&wire.replace("\"v\":1", "\"v\":2")).is_none());
        assert!(
            parse_launch(&wire.replace("\"kind\":\"identity.launch\"", "\"kind\":\"other\""))
                .is_none()
        );
        assert!(parse_launch(&wire.replace("\"url\":", "\"extra\":1,\"url\":")).is_none());
        assert!(
            parse_launch(&wire.replace("false,\"abortOnLoad", "true,\"abortOnLoad"))
                .unwrap()
                .interactive
        );
    }
}
