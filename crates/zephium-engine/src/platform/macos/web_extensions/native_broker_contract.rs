//! Live proof for WebKit's principal-bound extension-to-application channel.
//!
//! This source-free fixture does not enable product native messaging. It uses
//! one fixed internal application identifier, exact bounded payloads, one
//! read-only history-facade request and one port, and a nonpersistent
//! controller/store. It publishes a real regular window/tab surface before a
//! document-idle content script asks a module worker for an asynchronous
//! response. Passing proves that response and the full facade-to-host-to-facade
//! round trip, while separately recording the asymmetric persistent-port
//! behavior. It grants no generic native-host authority.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use dispatch2::DispatchQueue;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDictionary, NSError, NSObjectProtocol, NSRunLoop, NSString,
};
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionContext, WKWebExtensionContextPermissionStatus,
    WKWebExtensionController, WKWebExtensionControllerDelegate, WKWebExtensionMessagePort,
    WKWebExtensionWindow, WKWebView, WKWebsiteDataStore,
};
use serde_json::{json, Value};

const CONTRACT_PRINCIPAL: &str = "dddddddddddddddddddddddddddddddd";
const APPLICATION_IDENTIFIER: &str = "app.zephium.extension-broker.v1";
const ONE_SHOT_REQUEST: &str = "v1/history.recent/2";
const ONE_SHOT_REPLY: &str = r#"{"v":1,"items":[{"url":"https://first.example/path","title":"First visited page","lastVisit":1000},{"url":"https://second.example/path","title":"Second visited page","lastVisit":2000}]}"#;
const PORT_REQUEST: &str = "zephium-broker-port-request";
const PORT_REPLY: &str = "zephium-broker-port-reply";
const PORT_REPLY_UNOBSERVED: &str = "host-reply-unobserved";

#[derive(Default)]
struct BrokerState {
    expected_controller: Cell<Option<NonNull<WKWebExtensionController>>>,
    expected_context: Cell<Option<NonNull<WKWebExtensionContext>>>,
    one_shot_calls: Cell<usize>,
    one_shot_exact: Cell<bool>,
    port_connect_calls: Cell<usize>,
    port_connect_exact: Cell<bool>,
    port_message_calls: Cell<usize>,
    port_message_exact: Cell<bool>,
    port_send_completions: Cell<usize>,
    port_disconnects: Cell<usize>,
    failure: RefCell<Option<String>>,
}

impl BrokerState {
    fn snapshot(&self) -> (usize, bool, usize, bool, usize, bool, usize, usize) {
        (
            self.one_shot_calls.get(),
            self.one_shot_exact.get(),
            self.port_connect_calls.get(),
            self.port_connect_exact.get(),
            self.port_message_calls.get(),
            self.port_message_exact.get(),
            self.port_send_completions.get(),
            self.port_disconnects.get(),
        )
    }

    fn bind(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
    ) -> Result<(), String> {
        if self
            .expected_controller
            .replace(Some(NonNull::from(controller)))
            .is_some()
            || self
                .expected_context
                .replace(Some(NonNull::from(context)))
                .is_some()
        {
            return Err("native broker probe identity was bound more than once".into());
        }
        Ok(())
    }

    fn record_failure(&self, failure: impl Into<String>) {
        let mut slot = self.failure.borrow_mut();
        if slot.is_none() {
            *slot = Some(failure.into());
        }
    }

    fn exact_identity(
        &self,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
    ) -> bool {
        self.expected_controller.get() == Some(NonNull::from(controller))
            && self.expected_context.get() == Some(NonNull::from(context))
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(failure) = self.failure.borrow().as_ref() {
            return Err(format!("native broker callback failed closed: {failure}"));
        }
        let actual = self.snapshot();
        let expected = (1, true, 1, true, 1, true, 1, 1);
        if actual != expected {
            return Err(format!(
                "native broker callback cardinality/identity drifted: expected {expected:?}, got {actual:?}"
            ));
        }
        Ok(())
    }
}

struct BrokerDelegateIvars {
    window: Retained<super::ProbeWindow>,
    state: Rc<BrokerState>,
    port: RefCell<Option<Retained<WKWebExtensionMessagePort>>>,
    lifecycle_drops: Arc<AtomicUsize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumNativeBrokerProbeDelegate"]
    #[ivars = BrokerDelegateIvars]
    struct BrokerDelegate;

    unsafe impl NSObjectProtocol for BrokerDelegate {}

    unsafe impl WKWebExtensionControllerDelegate for BrokerDelegate {
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

        #[unsafe(method(webExtensionController:sendMessage:toApplicationWithIdentifier:forExtensionContext:replyHandler:))]
        unsafe fn send_message(
            &self,
            controller: &WKWebExtensionController,
            message: &AnyObject,
            application_identifier: Option<&NSString>,
            context: &WKWebExtensionContext,
            reply: &block2::DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
        ) {
            let state = &self.ivars().state;
            state.one_shot_calls.set(state.one_shot_calls.get() + 1);
            let exact = state.one_shot_calls.get() == 1
                && state.exact_identity(controller, context)
                && application_identifier.is_some_and(|identifier| {
                    identifier.isEqualToString(&NSString::from_str(APPLICATION_IDENTIFIER))
                })
                && message.downcast_ref::<NSString>().is_some_and(|message| {
                    message.isEqualToString(&NSString::from_str(ONE_SHOT_REQUEST))
                });
            state.one_shot_exact.set(exact);
            if exact {
                let response = NSString::from_str(ONE_SHOT_REPLY);
                reply.call((
                    Retained::as_ptr(&response).cast_mut().cast(),
                    std::ptr::null_mut(),
                ));
            } else {
                state.record_failure(
                    "one-shot identity, identifier, payload, or cardinality mismatch",
                );
                complete_with_error(reply, 1);
            }
        }

        #[unsafe(method(webExtensionController:connectUsingMessagePort:forExtensionContext:completionHandler:))]
        unsafe fn connect_port(
            &self,
            controller: &WKWebExtensionController,
            port: &WKWebExtensionMessagePort,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            let state = self.ivars().state.clone();
            state
                .port_connect_calls
                .set(state.port_connect_calls.get() + 1);
            let identifier = port.applicationIdentifier();
            let exact = state.port_connect_calls.get() == 1
                && state.exact_identity(controller, context)
                && identifier.as_ref().is_some_and(|identifier| {
                    identifier.isEqualToString(&NSString::from_str(APPLICATION_IDENTIFIER))
                })
                && self.ivars().port.borrow().is_none();
            state.port_connect_exact.set(exact);
            if !exact {
                state.record_failure("port identity, identifier, or cardinality mismatch");
                complete_error_only(completion, 2);
                return;
            }

            let Some(retained_port) = Retained::retain(NonNull::from(port).as_ptr()) else {
                state.record_failure("WebKit port could not be retained");
                complete_error_only(completion, 3);
                return;
            };
            let weak_port = Weak::from_retained(&retained_port);
            let message_state = state.clone();
            // Retain the outbound value for the native port lifetime. A send
            // completion proves enqueueing, not that WebKit copied a temporary
            // Objective-C value before returning to this handler.
            let port_reply_key = NSString::from_str("reply");
            let port_reply_value = NSString::from_str(PORT_REPLY);
            let port_reply = NSDictionary::from_slices(&[&*port_reply_key], &[&*port_reply_value]);
            let message_handler =
                block2::RcBlock::new(move |message: *mut AnyObject, error: *mut NSError| {
                    message_state
                        .port_message_calls
                        .set(message_state.port_message_calls.get() + 1);
                    let exact_message = error.is_null()
                        && message_state.port_message_calls.get() == 1
                        && unsafe { message.as_ref() }
                            .and_then(AnyObject::downcast_ref::<NSString>)
                            .is_some_and(|message| {
                                message.isEqualToString(&NSString::from_str(PORT_REQUEST))
                            });
                    message_state.port_message_exact.set(exact_message);
                    if !exact_message {
                        message_state
                            .record_failure("port message payload, error, or cardinality mismatch");
                        return;
                    }
                    let Some(port) = weak_port.load() else {
                        message_state.record_failure("retained native broker port disappeared");
                        return;
                    };
                    let reply = port_reply.clone();
                    let send_state = message_state.clone();
                    let send = block2::RcBlock::new(move || {
                        let completion_state = send_state.clone();
                        let send_completion = block2::RcBlock::new(move |error: *mut NSError| {
                            completion_state
                                .port_send_completions
                                .set(completion_state.port_send_completions.get() + 1);
                            if !error.is_null() || completion_state.port_send_completions.get() != 1
                            {
                                completion_state.record_failure(
                                    "native broker port reply failed or settled more than once",
                                );
                            }
                        });
                        unsafe {
                            port.sendMessage_completionHandler(
                                Some(&reply),
                                Some(&send_completion),
                            );
                        }
                    });
                    // WebKit invokes `messageHandler` while it is delivering
                    // extension-to-host traffic. Send one main-queue turn
                    // later so the reverse message cannot be lost to
                    // re-entrant port dispatch. `dispatch_async` copies the
                    // heap block and the queue is the same UI actor as `port`.
                    unsafe {
                        DispatchQueue::main().exec_async_with_block(block2::RcBlock::as_ptr(&send));
                    }
                });
            let disconnect_state = state.clone();
            let disconnect_handler = block2::RcBlock::new(move |error: *mut NSError| {
                disconnect_state
                    .port_disconnects
                    .set(disconnect_state.port_disconnects.get() + 1);
                if !error.is_null() || disconnect_state.port_disconnects.get() != 1 {
                    disconnect_state.record_failure(
                        "native broker port disconnected with an error or more than once",
                    );
                }
            });
            port.setMessageHandler(Some(&message_handler));
            port.setDisconnectHandler(Some(&disconnect_handler));
            self.ivars().port.replace(Some(retained_port));
            completion.call((std::ptr::null_mut(),));
        }
    }
);

impl BrokerDelegate {
    fn new(
        mtm: MainThreadMarker,
        window: Retained<super::ProbeWindow>,
        state: Rc<BrokerState>,
        lifecycle_drops: Arc<AtomicUsize>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(BrokerDelegateIvars {
            window,
            state,
            port: RefCell::new(None),
            lifecycle_drops,
        });
        // SAFETY: NSObject is the declared superclass and all ivars are fully
        // initialized before its initializer is invoked.
        unsafe { msg_send![super(object), init] }
    }

    fn release_port(&self) {
        if let Some(port) = self.ivars().port.borrow_mut().take() {
            // SAFETY: the retained port and copied handlers are main-thread
            // objects. Clear callbacks first so releasing the last host retain
            // cannot re-enter probe state during teardown.
            unsafe {
                port.setMessageHandler(None);
                port.setDisconnectHandler(None);
                if !port.isDisconnected() {
                    port.disconnect();
                }
            }
            drop(port);
        }
    }

    fn port_weak(&self) -> Option<Weak<WKWebExtensionMessagePort>> {
        self.ivars().port.borrow().as_ref().map(Weak::from_retained)
    }
}

impl Drop for BrokerDelegate {
    fn drop(&mut self) {
        self.ivars().lifecycle_drops.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) struct RuntimeEvidence {
    pub(super) controller: Weak<WKWebExtensionController>,
    pub(super) context: Weak<WKWebExtensionContext>,
    pub(super) view: Weak<WKWebView>,
    pub(super) store: Weak<WKWebsiteDataStore>,
    pub(super) port: Weak<WKWebExtensionMessagePort>,
    pub(super) delegate_drops: Arc<AtomicUsize>,
    pub(super) surface_drops: Arc<AtomicUsize>,
}

pub(super) fn write_fixture(root: &Path) -> Result<PathBuf, String> {
    let path = root.join("native-broker-contract");
    std::fs::create_dir(&path)
        .map_err(|error| format!("cannot create native-broker contract fixture: {error}"))?;
    let manifest = json!({
        "manifest_version": 3,
        "name": "Zephium Native Broker Contract Probe",
        "version": "1.0.0",
        "description": "Zephium-owned principal-bound native broker fixture.",
        "permissions": ["history", "nativeMessaging"],
        "content_scripts": [{
            "matches": [super::HOST_MATCH_PATTERN],
            "js": ["probe.js"],
            "run_at": "document_idle",
            "all_frames": false
        }],
        "background": {
            "service_worker": "worker.js",
            "type": "module"
        }
    });
    write(&path, "manifest.json", &manifest.to_string())?;
    write(
        &path,
        super::compatibility_artifact::HISTORY_BRIDGE,
        include_str!("../../../../../zephium-extension-package/assets/macos/webkit-history-v1.js"),
    )?;
    write(
        &path,
        "worker.js",
        &format!(
            r#"import "./{}";
const historyEvidence = () => globalThis.chrome.history.search({{ text: "", maxResults: 2, startTime: 0 }})
  .then((items) => Array.isArray(items) && items.length === 2 &&
    items[0]?.id === "https://first.example/path" &&
    items[0]?.title === "First visited page" &&
    items[1]?.id === "https://second.example/path" &&
    items[1]?.lastVisitTime === 2000 &&
    typeof globalThis.chrome.history.addUrl === "undefined" &&
    typeof globalThis.chrome.history.deleteAll === "undefined"
      ? "history-search-passed"
      : "history-search-invalid")
  .catch((error) => `error:${{String(error?.message ?? error)}}`);
globalThis.chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {{
  if (message?.kind !== "zephium-native-broker-worker-evidence-v1") return false;
  const senderValid = Number.isInteger(sender?.frameId) &&
    Number.isInteger(sender?.tab?.id) &&
    /^http:\/\//.test(sender?.url ?? "") &&
    /^http:\/\//.test(sender?.tab?.url ?? "");
  historyEvidence().then((history) => {{
    const evidence = senderValid ? history : "sender-invalid";
    const port = globalThis.chrome.runtime.connectNative({application_identifier:?});
    port.postMessage({port_request:?});
    setTimeout(() => {{
      port.disconnect();
      sendResponse(evidence);
    }}, 100);
  }});
  return true;
}});
"#,
            super::compatibility_artifact::HISTORY_BRIDGE,
            application_identifier = APPLICATION_IDENTIFIER,
            port_request = PORT_REQUEST,
        ),
    )?;
    let script = format!(
        r#"(() => {{
    'use strict';
    const runtime = globalThis.chrome?.runtime;
    const settle = (oneShot, port) => {{
        document.title = JSON.stringify({{ history: "worker-bounded-recent-search", oneShot, port }});
    }};
    if (!runtime?.sendMessage) {{
        settle("absent", "absent");
        return;
    }}
    setTimeout(() => {{
      Promise.resolve(runtime.sendMessage({{
      kind: "zephium-native-broker-worker-evidence-v1",
      }})).then((oneShot) => {{
        settle(oneShot, {port_reply_unobserved:?});
      }}, (error) => settle(`error:${{String(error?.message ?? error)}}`, "not-started"));
    }}, 100);
}})()"#,
        port_reply_unobserved = PORT_REPLY_UNOBSERVED,
    );
    write(&path, "probe.js", &script)?;
    Ok(path)
}

pub(super) fn run(
    extension: &WKWebExtension,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<RuntimeEvidence, String> {
    inspect_parse_contract(extension)?;
    let server = super::FixtureServer::start(None)?;
    let bundle = super::new_nonpersistent_controller(mtm)?;
    // SAFETY: the controller and browsing configuration share the exact
    // nonpersistent data store retained by `bundle` for the full probe.
    unsafe {
        bundle
            .webview_configuration
            .setWebExtensionController(Some(&bundle.controller));
    }
    let controller = bundle.controller.clone();
    let store = bundle._data_store.clone();
    let context = super::new_context(extension, CONTRACT_PRINCIPAL)?;
    let state = Rc::new(BrokerState::default());
    state.bind(&controller, &context)?;
    let grants = super::super::extensions::apply_probe_grants(
        &context,
        &[super::super::extensions::MacosNativeApiPermission::NativeMessaging],
        &[super::HOST_MATCH_PATTERN],
        true,
    )
    .map_err(|error| format!("native broker exact grants failed: {error}"))?;
    let permission = NSString::from_str("nativeMessaging");
    if unsafe { context.permissionStatusForPermission(&permission) }
        != WKWebExtensionContextPermissionStatus::GrantedExplicitly
        || unsafe { context.grantedPermissions() }.count() != 1
    {
        grants
            .clear_and_verify(&context)
            .map_err(|error| format!("native broker grant rollback failed: {error}"))?;
        return Err("native broker exact grant readback failed".into());
    }

    let delegate_drops = Arc::new(AtomicUsize::new(0));
    let surface_drops = Arc::new(AtomicUsize::new(0));
    let webview_requests = Arc::new(AtomicUsize::new(0));

    let controller_weak = Weak::from_retained(&controller);
    let context_weak = Weak::from_retained(&context);
    let store_weak = Weak::from_retained(&store);
    let mut window = None;
    let mut view = None;
    let mut view_weak = None;
    let mut delegate = None;
    let mut surface_tab = None;
    let mut surface_window = None;
    let mut surface_published = false;
    let mut loaded = false;
    let gate = (|| {
        let configuration = bundle.webview_configuration.clone();
        let probe_window = super::new_window(mtm)?;
        let host =
            super::profile_isolation::host_for_window(&probe_window, "native-broker contract")?;
        let probe_view = super::profile_isolation::build_profile_view(&host, configuration)?;
        probe_window.orderFrontRegardless();
        let native_view = super::super::native::webkit(&probe_view);
        super::assert_attached_controller(&native_view, &controller)?;
        super::profile_isolation::assert_attached_store(&native_view, &store)?;
        view_weak = Some(Weak::from_retained(&native_view));
        let tab = super::ProbeTab::new(
            mtm,
            native_view.clone(),
            webview_requests.clone(),
            surface_drops.clone(),
        );
        let extension_window =
            super::ProbeWindow::new(mtm, tab.clone(), false, surface_drops.clone());
        tab.set_window(&extension_window);
        let installed_delegate = BrokerDelegate::new(
            mtm,
            extension_window.clone(),
            state.clone(),
            delegate_drops.clone(),
        );
        let delegate_protocol = ProtocolObject::from_ref(&*installed_delegate);
        let window_protocol = ProtocolObject::from_ref(&*extension_window);
        let tab_protocol = ProtocolObject::from_ref(&*tab);
        // SAFETY: the delegate is retained below before extension code can run.
        unsafe { controller.setDelegate(Some(delegate_protocol)) };
        delegate = Some(installed_delegate.clone());
        surface_tab = Some(tab.clone());
        surface_window = Some(extension_window.clone());
        super::load_context(&controller, &context, "native-broker contract")?;
        loaded = true;
        // SAFETY: every published identity remains retained until the exact
        // close notifications and native delegate teardown below.
        unsafe {
            controller.didOpenWindow(window_protocol);
            controller.didOpenTab(tab_protocol);
            controller.didFocusWindow(Some(window_protocol));
            controller.didActivateTab_previousActiveTab(tab_protocol, None);
        }
        surface_published = true;
        super::persistent_runtime::load_background_content(
            &context,
            run_loop,
            "native-broker contract",
        )?;
        drop(native_view);
        let page = server.url("/keyboard", "native-broker-contract");
        let native_page = super::native_url(&page)?;
        super::assert_context_access(
            &context,
            &native_page,
            true,
            "native-broker content-script grant",
        )?;
        probe_view
            .load_url(&page)
            .map_err(|error| format!("cannot navigate native-broker contract: {error}"))?;
        view = Some(probe_view);
        window = Some(probe_window);
        wait_for_evidence(
            view.as_ref().expect("native-broker view was stored"),
            &context,
            run_loop,
            &state,
        )
    })();
    let port_weak = delegate.as_ref().and_then(|delegate| delegate.port_weak());

    let mut cleanup_failures = Vec::new();
    if surface_published {
        let tab_protocol = ProtocolObject::from_ref(
            &**surface_tab
                .as_ref()
                .expect("published native-broker surface retains its tab"),
        );
        let window_protocol = ProtocolObject::from_ref(
            &**surface_window
                .as_ref()
                .expect("published native-broker surface retains its window"),
        );
        // SAFETY: close notifications balance the exact retained identities
        // published above and run before context/delegate teardown.
        unsafe {
            controller.didFocusWindow(None);
            controller.didCloseTab_windowIsClosing(tab_protocol, true);
            controller.didCloseWindow(window_protocol);
        }
    }
    if let Some(delegate) = delegate.as_ref() {
        delegate.release_port();
    }
    // SAFETY: native callbacks are quiesced before context unload.
    unsafe { controller.setDelegate(None) };
    if loaded {
        if let Err(error) = super::unload_context(&controller, &context, "native-broker contract") {
            cleanup_failures.push(error);
        }
    }
    if let Err(error) = grants.clear_and_verify(&context) {
        cleanup_failures.push(format!("native-broker grant cleanup failed: {error}"));
    }
    drop(view.take());
    if let Some(window) = window.take() {
        window.close();
        drop(window);
    }
    drop(surface_tab.take());
    drop(surface_window.take());
    drop(context);
    drop(delegate.take());
    let webview_request_count = webview_requests.load(Ordering::Acquire);
    if !(1..=16).contains(&webview_request_count) {
        cleanup_failures.push(format!(
            "native-broker surface requested its WebView {webview_request_count} times (expected 1..=16)"
        ));
    }
    drop(controller);
    drop(store);
    drop(bundle);

    let cleanup = if cleanup_failures.is_empty() {
        Ok(())
    } else {
        Err(cleanup_failures.join("; "))
    };
    match (gate, cleanup) {
        (Ok(()), Ok(())) => Ok(RuntimeEvidence {
            controller: controller_weak,
            context: context_weak,
            view: view_weak.expect("successful native-broker gate constructed a view"),
            store: store_weak,
            port: port_weak.expect("successful native-broker gate retained its exact port"),
            delegate_drops,
            surface_drops,
        }),
        (Err(gate), Ok(())) => Err(gate),
        (Ok(()), Err(cleanup)) => Err(format!("native-broker cleanup failed: {cleanup}")),
        (Err(gate), Err(cleanup)) => Err(format!(
            "{gate}; native-broker cleanup also failed: {cleanup}"
        )),
    }
}

fn inspect_parse_contract(extension: &WKWebExtension) -> Result<(), String> {
    if unsafe { extension.manifestVersion() } != 3.0 {
        return Err("native-broker contract was not parsed as manifest v3".into());
    }
    let errors = unsafe { extension.errors() };
    let permissions = unsafe { extension.requestedPermissions() };
    let exact_permission = permissions.count() == 1
        && permissions
            .allObjects()
            .objectAtIndex(0)
            .isEqualToString(&NSString::from_str("nativeMessaging"));
    if errors.count() != 0 || !exact_permission {
        return Err(format!(
            "native-broker parse contract drifted: errors={}, permissions={:?}",
            errors.count(),
            (0..permissions.count())
                .map(|index| permissions.allObjects().objectAtIndex(index).to_string())
                .collect::<Vec<_>>()
        ));
    }
    Ok(())
}

fn wait_for_evidence(
    view: &wry::WebView,
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    state: &BrokerState,
) -> Result<(), String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let mut page_settled = false;
    loop {
        if let Some(failure) = state.failure.borrow().as_ref() {
            return Err(format!("native broker callback failed: {failure}"));
        }
        if !page_settled {
            let title = view
                .document_title()
                .map_err(|error| format!("cannot inspect native-broker probe title: {error}"))?;
            if let Some(title) = title.as_deref().filter(|title| title.starts_with('{')) {
                let evidence: Value = serde_json::from_str(title).map_err(|error| {
                    format!("native-broker probe returned invalid evidence {title:?}: {error}")
                })?;
                if evidence
                    != json!({
                        "history": "worker-bounded-recent-search",
                        "oneShot": "history-search-passed",
                        "port": PORT_REPLY_UNOBSERVED
                    })
                {
                    return Err(format!(
                        "native-broker page evidence drifted: {evidence}; callbacks={:?}",
                        state.snapshot()
                    ));
                }
                page_settled = true;
            }
        }
        if page_settled && state.port_disconnects.get() == 1 {
            return state.validate();
        }
        super::validate_context_errors(context, "native-broker contract")?;
        if Instant::now() >= deadline {
            return Err(format!(
                "native-broker probe timed out at {:?}; callbacks={:?}",
                view.url().ok(),
                state.snapshot()
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn complete_with_error(
    completion: &block2::DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
    code: isize,
) {
    let error = probe_error(code);
    completion.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
}

fn complete_error_only(completion: &block2::DynBlock<dyn Fn(*mut NSError)>, code: isize) {
    let error = probe_error(code);
    completion.call((Retained::as_ptr(&error).cast_mut(),));
}

fn probe_error(code: isize) -> Retained<NSError> {
    let domain = NSString::from_str("app.zephium.native-broker-probe");
    unsafe { NSError::errorWithDomain_code_userInfo(&domain, code, None) }
}

fn write(directory: &Path, name: &str, contents: &str) -> Result<(), String> {
    let path = directory.join(name);
    let parent = path
        .parent()
        .ok_or_else(|| format!("native-broker fixture path has no parent: {name}"))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create native-broker fixture directory: {error}"))?;
    std::fs::write(path, contents)
        .map_err(|error| format!("cannot write native-broker contract fixture {name}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_has_one_closed_internal_identifier_and_no_script_network_surface() {
        let temp = tempfile::tempdir().expect("temporary contract root");
        let fixture = write_fixture(temp.path()).expect("native-broker contract fixture");
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(fixture.join("manifest.json")).expect("manifest bytes"),
        )
        .expect("manifest JSON");
        assert_eq!(
            manifest["permissions"],
            json!(["history", "nativeMessaging"])
        );
        assert_eq!(
            manifest["background"],
            json!({"service_worker":"worker.js","type":"module"})
        );
        assert_eq!(
            manifest["content_scripts"][0]["run_at"],
            json!("document_idle")
        );
        let script = std::fs::read_to_string(fixture.join("probe.js")).expect("probe script");
        let worker = std::fs::read_to_string(fixture.join("worker.js")).expect("worker script");
        assert_eq!(script.matches(APPLICATION_IDENTIFIER).count(), 0);
        assert_eq!(worker.matches(APPLICATION_IDENTIFIER).count(), 1);
        assert!(worker.contains("historyEvidence().then((history)"));
        assert!(!fixture.join("background-host.html").exists());
        assert!(worker.contains("history.search"));
        assert!(worker.contains("connectNative"));
        for source in [&script, &worker] {
            assert!(!source.contains("fetch("));
            assert!(!source.contains("XMLHttpRequest"));
        }
        assert_eq!(
            std::fs::read_to_string(
                fixture.join(super::super::compatibility_artifact::HISTORY_BRIDGE)
            )
            .unwrap(),
            include_str!(
                "../../../../../zephium-extension-package/assets/macos/webkit-history-v1.js"
            )
        );
    }
}
