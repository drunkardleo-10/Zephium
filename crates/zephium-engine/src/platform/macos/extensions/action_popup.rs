//! Exact, resource-budgeted presentation of native macOS extension popups.
//!
//! WebKit owns popup document creation and execution. Zephium owns the user
//! gesture, profile/controller/context/tab join, AppKit anchor, size policy,
//! terminal result and the sole process-wide popup resource lease. A delegate
//! callback without a matching trusted request is rejected, so extension code
//! cannot surface privileged browser chrome on its own.

use std::cell::{Cell, RefCell};
use std::panic::AssertUnwindSafe;
use std::rc::{Rc, Weak as RcWeak};
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSPopover, NSPopoverBehavior, NSPopoverDelegate,
    NSView, NSViewFrameDidChangeNotification, NSWindow, NSWindowDelegate, NSWindowOrderingMode,
    NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSError, NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint,
    NSRect, NSRectEdge, NSSize, NSString, NSURLRequest, NSURL,
};
use objc2_web_kit::{
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKNavigationType,
    WKWebExtensionAction, WKWebExtensionContext, WKWebExtensionController, WKWebExtensionTab,
    WKWebView,
};
use zephium_core::extensions::{
    ExtensionActionRejection, ExtensionActionRequest, ExtensionActionRequestId,
    ExtensionActionSettlement, ExtensionBrowserRequestAction,
    MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES, MAX_EXTENSION_POPUP_HEIGHT, MAX_EXTENSION_POPUP_WIDTH,
    MIN_EXTENSION_POPUP_HEIGHT, MIN_EXTENSION_POPUP_WIDTH,
};
use zephium_core::geometry::{Rect, Size};
use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::EngineEvent;

use crate::host::NativeResourceLease;
use crate::{EngineEventIngress, EngineEventIngressSink};

use super::browser_request_broker::BrowserRequestBroker;
use super::browser_surface::request_url;

const POPUP_LOAD_TIMEOUT: Duration = Duration::from_secs(10);
const POPUP_ERROR_DOMAIN: &str = "app.zephium.extension-action";

struct PendingPopup {
    request: ExtensionActionRequest,
    controller: Retained<WKWebExtensionController>,
    context: Retained<WKWebExtensionContext>,
    tab: Retained<ProtocolObject<dyn WKWebExtensionTab>>,
    parent: Retained<NSView>,
    _lease: NativeResourceLease,
    watchdog: crate::platform::imp::ContentPolicyTimeout,
}

struct ActivePopup {
    request: ExtensionActionRequest,
    context: Retained<WKWebExtensionContext>,
    action: Retained<WKWebExtensionAction>,
    webview: Retained<WKWebView>,
    popover: Retained<NSPopover>,
    parent: Retained<NSView>,
    delegate: Retained<ActionPopoverDelegate>,
    frame_observation: Option<PopupFrameObservation>,
    _lease: NativeResourceLease,
}

struct ActiveOptionsPage {
    context: Retained<WKWebExtensionContext>,
    window: Retained<NSWindow>,
    webview: Retained<WKWebView>,
    delegate: Retained<OptionsWindowDelegate>,
    _lease: NativeResourceLease,
}

struct ClosingPopup {
    token: u64,
    context: Retained<WKWebExtensionContext>,
    parent: Retained<NSView>,
    lease: NativeResourceLease,
}

struct PopupFrameObservation {
    view: Retained<NSView>,
    delegate: Retained<ActionPopoverDelegate>,
    previous_frame_notifications: bool,
}

impl Drop for PopupFrameObservation {
    fn drop(&mut self) {
        // SAFETY: this exactly balances the selector registration installed
        // for this popup; both objects remain retained by this guard.
        let _ = objc2::exception::catch(AssertUnwindSafe(|| {
            unsafe {
                NSNotificationCenter::defaultCenter().removeObserver_name_object(
                    &self.delegate,
                    Some(NSViewFrameDidChangeNotification),
                    Some(&self.view),
                );
            }
            self.view
                .setPostsFrameChangedNotifications(self.previous_frame_notifications);
        }));
    }
}

struct ActionPopoverDelegateIvars {
    broker: RcWeak<ActionPopupBroker>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionActionPopoverDelegate"]
    #[ivars = ActionPopoverDelegateIvars]
    struct ActionPopoverDelegate;

    unsafe impl NSObjectProtocol for ActionPopoverDelegate {}

    unsafe impl NSPopoverDelegate for ActionPopoverDelegate {
        #[unsafe(method(popoverDidShow:))]
        fn popover_did_show(&self, _notification: &NSNotification) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.clamp_active_size();
            }
        }

        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, _notification: &NSNotification) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.finish_native_close();
            }
        }
    }

    // Notification selectors are ordinary Objective-C methods, not protocol
    // overrides. Keeping this outside `NSPopoverDelegate` is load-bearing:
    // objc2 validates protocol membership while registering the class and
    // aborts a debug process if a foreign selector is placed in that impl.
    impl ActionPopoverDelegate {
        #[unsafe(method(popupContentFrameDidChange:))]
        fn popup_content_frame_did_change(&self, _notification: &NSNotification) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.clamp_active_size();
            }
        }
    }
);

impl ActionPopoverDelegate {
    fn new(mtm: MainThreadMarker, broker: RcWeak<ActionPopupBroker>) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(ActionPopoverDelegateIvars { broker });
        // SAFETY: NSObject is the declared superclass and the sole ivar is
        // initialized before its initializer runs.
        unsafe { msg_send![super(object), init] }
    }
}

struct OptionsWindowDelegateIvars {
    broker: RcWeak<ActionPopupBroker>,
    browser_requests: Rc<BrowserRequestBroker>,
    context: Retained<WKWebExtensionContext>,
    extension_origin: Box<str>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionOptionsWindowDelegate"]
    #[ivars = OptionsWindowDelegateIvars]
    struct OptionsWindowDelegate;

    unsafe impl NSObjectProtocol for OptionsWindowDelegate {}

    unsafe impl NSWindowDelegate for OptionsWindowDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.finish_options_close();
            }
        }
    }

    unsafe impl WKNavigationDelegate for OptionsWindowDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        unsafe fn decide_navigation(
            &self,
            _webview: &WKWebView,
            action: &WKNavigationAction,
            decision: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let Some(url) = action.request().URL() else {
                decision.call((WKNavigationActionPolicy::Cancel,));
                return;
            };
            let absolute = url.absoluteString();
            let classification = absolute.as_ref().and_then(|absolute| {
                (absolute.lengthOfBytesUsingEncoding(objc2_foundation::NSUTF8StringEncoding)
                    <= MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES)
                    .then(|| {
                        objc2::rc::autoreleasepool(|pool| {
                            classify_options_navigation(
                                self.ivars().extension_origin.as_ref(),
                                unsafe { absolute.to_str(pool) },
                                unsafe { action.navigationType() }
                                    == WKNavigationType::LinkActivated,
                            )
                        })
                    })
            });
            match classification.flatten() {
                Some(OptionsNavigation::Extension) => {
                    decision.call((WKNavigationActionPolicy::Allow,));
                }
                Some(OptionsNavigation::External) => {
                    self.route_external_link(&url);
                    decision.call((WKNavigationActionPolicy::Cancel,));
                }
                None => decision.call((WKNavigationActionPolicy::Cancel,)),
            }
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn web_content_process_did_terminate(&self, webview: &WKWebView) {
            let Some(broker) = self.ivars().broker.upgrade() else {
                return;
            };
            // The callback is borrowed. Retain only long enough to mint a
            // non-owning exact-generation witness for the deferred main-turn
            // teardown; the active options owner remains the sole strong
            // lifetime authority.
            let Some(webview) = (unsafe { Retained::retain(webview as *const _ as *mut _) }) else {
                return;
            };
            broker.schedule_options_process_termination(&self.ivars().context, &webview);
        }
    }
);

impl OptionsWindowDelegate {
    fn new(
        mtm: MainThreadMarker,
        broker: RcWeak<ActionPopupBroker>,
        browser_requests: Rc<BrowserRequestBroker>,
        context: Retained<WKWebExtensionContext>,
        extension_origin: Box<str>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(OptionsWindowDelegateIvars {
            broker,
            browser_requests,
            context,
            extension_origin,
        });
        // SAFETY: NSObject is the declared superclass and the sole ivar is
        // initialized before its initializer runs.
        unsafe { msg_send![super(object), init] }
    }

    fn route_external_link(&self, url: &NSURL) {
        if !self
            .ivars()
            .browser_requests
            .accepts(None, &self.ivars().context)
        {
            return;
        }
        let Ok(url) = request_url(url) else {
            return;
        };
        let completion: RcBlock<dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError)> =
            RcBlock::new(|_, _| {});
        self.ivars().browser_requests.begin_tab(
            ExtensionBrowserRequestAction::CreateTab {
                window: None,
                url: Some(url),
                active: true,
            },
            &completion,
        );
    }
}

/// Main-thread owner for one profile controller's pending or visible popup.
pub(super) struct ActionPopupBroker {
    profile: ProfileId,
    sink: Option<EngineEventIngressSink>,
    browser_requests: Rc<BrowserRequestBroker>,
    pending: RefCell<Option<PendingPopup>>,
    active: RefCell<Option<ActivePopup>>,
    closing: RefCell<Option<ClosingPopup>>,
    next_closing_token: Cell<u64>,
    options: RefCell<Option<ActiveOptionsPage>>,
    sealed: Cell<bool>,
}

impl ActionPopupBroker {
    pub(super) fn new(
        profile: ProfileId,
        sink: Option<EngineEventIngressSink>,
        browser_requests: Rc<BrowserRequestBroker>,
    ) -> Rc<Self> {
        Rc::new(Self {
            profile,
            sink,
            browser_requests,
            pending: RefCell::new(None),
            active: RefCell::new(None),
            closing: RefCell::new(None),
            next_closing_token: Cell::new(1),
            options: RefCell::new(None),
            sealed: Cell::new(false),
        })
    }

    /// Reserves the exact native callback expected from `performActionForTab:`.
    /// The host acquires the global resource lease and revalidates the action
    /// before this call. A background-bearing extension is warmed only for
    /// this trusted gesture so its MV3 listeners exist before popup messaging;
    /// ordinary startup remains fully lazy.
    pub(super) fn begin(
        self: &Rc<Self>,
        request: ExtensionActionRequest,
        controller: Retained<WKWebExtensionController>,
        context: Retained<WKWebExtensionContext>,
        tab: Retained<ProtocolObject<dyn WKWebExtensionTab>>,
        parent: Retained<NSView>,
        lease: NativeResourceLease,
    ) -> Result<(), ExtensionActionRejection> {
        if request.runtime().profile() != self.profile {
            return Err(ExtensionActionRejection::InvalidRequest);
        }
        if self.sealed.get() {
            return Err(ExtensionActionRejection::ShuttingDown);
        }
        if self.sink.is_none() {
            return Err(ExtensionActionRejection::NativeAdmissionFailed);
        }
        if self.pending.borrow().is_some() || self.active.borrow().is_some() {
            return Err(ExtensionActionRejection::PopupCapacityExceeded);
        }
        let valid_anchor = objc2::exception::catch(AssertUnwindSafe(|| {
            parent.window().is_some() && popup_anchor_rect(request, &parent).is_some()
        }))
        .unwrap_or(false);
        if !valid_anchor {
            return Err(ExtensionActionRejection::InvalidRequest);
        }

        let profile = self.profile;
        let request_id = request.id();
        let Some(watchdog) =
            crate::platform::imp::schedule_content_policy_timeout(POPUP_LOAD_TIMEOUT, move || {
                #[cfg(feature = "native-extension-lab-diagnostics")]
                eprintln!("extension lab: popup action timed out before native presentation");
                let _ = crate::host::with_extension_action_popup_terminal(move |host| {
                    host.timeout_extension_action_popup(profile, request_id);
                });
            })
        else {
            return Err(ExtensionActionRejection::NativeAdmissionFailed);
        };
        *self.pending.borrow_mut() = Some(PendingPopup {
            request,
            controller,
            context,
            tab,
            parent,
            _lease: lease,
            watchdog,
        });
        self.warm_background_and_perform(request_id);
        Ok(())
    }

    fn warm_background_and_perform(self: &Rc<Self>, request: ExtensionActionRequestId) {
        let context =
            self.pending.borrow().as_ref().and_then(|pending| {
                (pending.request.id() == request).then(|| pending.context.clone())
            });
        let Some(context) = context else {
            self.cancel_pending(request, ExtensionActionRejection::NativeAdmissionFailed);
            return;
        };
        let has_background = match objc2::exception::catch(AssertUnwindSafe(|| unsafe {
            context.webExtension().hasBackgroundContent()
        })) {
            Ok(has_background) => has_background,
            Err(_) => {
                self.cancel_pending(request, ExtensionActionRejection::NativeAdmissionFailed);
                return;
            }
        };
        #[cfg(feature = "native-extension-lab-diagnostics")]
        eprintln!(
            "extension lab: popup background warm-up requested; has-background={has_background}; context-errors={}",
            unsafe { context.errors() }.count()
        );
        if !has_background {
            self.perform_pending_action(request);
            return;
        }

        let broker = Rc::downgrade(self);
        let completion: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(move |error: *mut NSError| {
            let Some(broker) = broker.upgrade() else {
                return;
            };
            if !error.is_null() {
                #[cfg(feature = "native-extension-lab-diagnostics")]
                {
                    // SAFETY: WebKit guarantees a live NSError for the
                    // duration of this completion callback.
                    let error = unsafe { &*error };
                    eprintln!(
                        "extension lab: popup background warm-up failed: domain={}; code={}",
                        error.domain(),
                        error.code()
                    );
                }
                crate::diagnostic!(
                    "extensions: popup background warm-up failed before presentation"
                );
                broker.cancel_pending(request, ExtensionActionRejection::PopupUnavailable);
                return;
            }
            #[cfg(feature = "native-extension-lab-diagnostics")]
            eprintln!("extension lab: popup background warm-up completed");
            broker.perform_pending_action(request);
        });
        if objc2::exception::catch(AssertUnwindSafe(|| unsafe {
            context.loadBackgroundContentWithCompletionHandler(&completion);
        }))
        .is_err()
        {
            self.cancel_pending(request, ExtensionActionRejection::PopupUnavailable);
        }
    }

    fn perform_pending_action(&self, request: ExtensionActionRequestId) {
        let target = self.pending.borrow().as_ref().and_then(|pending| {
            (pending.request.id() == request)
                .then(|| (pending.context.clone(), pending.tab.clone()))
        });
        let Some((context, tab)) = target else {
            return;
        };
        if objc2::exception::catch(AssertUnwindSafe(|| unsafe {
            context.performActionForTab(Some(&tab));
        }))
        .is_err()
        {
            self.cancel_pending(request, ExtensionActionRejection::NativeAdmissionFailed);
        }
    }

    /// Transfers the one already-admitted popup surface into an unprivileged
    /// options window for the context's exact authenticated URL. Popup close
    /// and options presentation share one lease, so no additional native-view
    /// budget can be minted by extension code.
    pub(super) fn open_options_page(
        self: &Rc<Self>,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        let result = objc2::exception::catch(AssertUnwindSafe(|| {
            if self.sealed.get() {
                return Err(ExtensionActionRejection::ShuttingDown);
            }
            if let Some(options) = self.options.borrow().as_ref() {
                if !std::ptr::eq(&*options.context, context) {
                    return Err(ExtensionActionRejection::PopupCapacityExceeded);
                }
                options.window.makeKeyAndOrderFront(None);
                return Ok(());
            }
            if unsafe { context.webExtensionController() }
                .as_ref()
                .is_none_or(|actual| !std::ptr::eq(&**actual, controller))
            {
                return Err(ExtensionActionRejection::InvalidRequest);
            }
            let transition = if let Some(active) = self
                .active
                .try_borrow_mut()
                .map_err(|_| ExtensionActionRejection::NativeAdmissionFailed)?
                .take()
            {
                if !std::ptr::eq(&*active.context, context)
                    || unsafe { active.action.webExtensionContext() }
                        .as_ref()
                        .is_none_or(|actual| !std::ptr::eq(&**actual, context))
                {
                    *self.active.borrow_mut() = Some(active);
                    return Err(ExtensionActionRejection::InvalidRequest);
                }
                transition_popup_to_options(active)
            } else {
                let closing = self
                    .closing
                    .try_borrow_mut()
                    .map_err(|_| ExtensionActionRejection::NativeAdmissionFailed)?
                    .take()
                    .ok_or(ExtensionActionRejection::PopupUnavailable)?;
                if !std::ptr::eq(&*closing.context, context) {
                    *self.closing.borrow_mut() = Some(closing);
                    return Err(ExtensionActionRejection::InvalidRequest);
                }
                PopupTransition {
                    context: closing.context,
                    parent: closing.parent,
                    lease: closing.lease,
                }
            };
            self.present_options_transition(transition)?;
            Ok(())
        }))
        .unwrap_or(Err(ExtensionActionRejection::NativeAdmissionFailed));
        match result {
            Ok(()) => completion.call((std::ptr::null_mut(),)),
            Err(reason) => {
                crate::diagnostic!(
                    "extensions: options-page presentation rejected with typed reason {reason:?}"
                );
                complete_rejected(completion, reason);
            }
        }
    }

    pub(super) fn open_options_page_authorized(
        self: &Rc<Self>,
        context: &WKWebExtensionContext,
    ) -> bool {
        let Some(controller) = (unsafe { context.webExtensionController() }) else {
            return false;
        };
        let opened = Rc::new(Cell::new(false));
        let callback_opened = opened.clone();
        let completion: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(move |error: *mut NSError| {
            callback_opened.set(error.is_null());
        });
        self.open_options_page(&controller, context, &completion);
        opened.get()
    }

    /// Opens settings from trusted browser chrome without requiring a popup
    /// to exist first. The caller supplies the same bounded native-view lease
    /// used by popup-to-options transfer, so this path cannot expand the
    /// extension resource pool.
    pub(super) fn open_options_page_from_browser(
        self: &Rc<Self>,
        context: Retained<WKWebExtensionContext>,
        parent: Retained<NSView>,
        lease: NativeResourceLease,
    ) -> Result<(), ExtensionActionRejection> {
        if self.sealed.get() {
            return Err(ExtensionActionRejection::ShuttingDown);
        }
        if let Some(options) = self.options.borrow().as_ref() {
            if !std::ptr::eq(&*options.context, &*context) {
                return Err(ExtensionActionRejection::PopupCapacityExceeded);
            }
            options.window.makeKeyAndOrderFront(None);
            return Ok(());
        }
        if self.pending.borrow().is_some()
            || self.active.borrow().is_some()
            || self.closing.borrow().is_some()
        {
            return Err(ExtensionActionRejection::PopupCapacityExceeded);
        }
        self.present_options_transition(PopupTransition {
            context,
            parent,
            lease,
        })
    }

    /// Handles WebKit's loaded-and-ready callback. Only the exact context and
    /// tab reserved by a trusted Shell gesture may consume the pending slot.
    pub(super) fn present(
        self: &Rc<Self>,
        controller: &WKWebExtensionController,
        action: &WKWebExtensionAction,
        context: &WKWebExtensionContext,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        let expected = self.pending.try_borrow().ok().and_then(|pending| {
            pending.as_ref().map(|pending| {
                (
                    pending.controller.clone(),
                    pending.context.clone(),
                    pending.tab.clone(),
                )
            })
        });
        let matches = objc2::exception::catch(AssertUnwindSafe(|| {
            expected.is_some_and(|(expected_controller, expected_context, expected_tab)| {
                std::ptr::eq(&*expected_controller, controller)
                    && std::ptr::eq(&*expected_context, context)
                    && unsafe { action.webExtensionContext() }
                        .is_some_and(|actual| std::ptr::eq(&*actual, context))
                    && unsafe { action.associatedTab() }
                        .is_some_and(|actual| std::ptr::eq(&*actual, &*expected_tab))
                    && unsafe { controller.extensionContexts() }.containsObject(context)
            })
        }))
        .unwrap_or(false);
        if !matches || self.sealed.get() {
            complete_rejected(completion, ExtensionActionRejection::PopupUnavailable);
            return;
        }

        let Some(pending) = self
            .pending
            .try_borrow_mut()
            .ok()
            .and_then(|mut pending| pending.take())
        else {
            complete_rejected(completion, ExtensionActionRejection::NativeAdmissionFailed);
            return;
        };
        drop(pending.watchdog);
        let request = pending.request;
        let presentation = objc2::exception::catch(AssertUnwindSafe(|| {
            if !unsafe { action.presentsPopup() } {
                return Err(ExtensionActionRejection::RuntimeSuperseded);
            }
            let popover = unsafe { action.popupPopover() }
                .ok_or(ExtensionActionRejection::PopupUnavailable)?;
            let popup_webview = unsafe { action.popupWebView() }
                .ok_or(ExtensionActionRejection::PopupUnavailable)?;
            let mtm =
                MainThreadMarker::new().ok_or(ExtensionActionRejection::NativeAdmissionFailed)?;
            let anchor = popup_anchor_rect(request, &pending.parent)
                .ok_or(ExtensionActionRejection::InvalidRequest)?;
            let size = clamp_popup_size(popover.contentSize());
            popover.setContentSize(size);
            // Lab diagnostics must survive the application losing
            // focus so Safari can attach to the exact popup WKWebView. The
            // ordinary desktop keeps native transient dismissal semantics.
            #[cfg(feature = "native-extension-lab-diagnostics")]
            popover.setBehavior(NSPopoverBehavior::ApplicationDefined);
            #[cfg(not(feature = "native-extension-lab-diagnostics"))]
            popover.setBehavior(NSPopoverBehavior::Transient);
            popover.setAnimates(true);
            let delegate = ActionPopoverDelegate::new(mtm, Rc::downgrade(self));
            popover.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            let frame_observation = popover.contentViewController().map(|controller| {
                let view = controller.view();
                let previous_frame_notifications = view.postsFrameChangedNotifications();
                view.setPostsFrameChangedNotifications(true);
                // SAFETY: the active popup retains both observer and observed
                // view and removes this exact registration before either can
                // be released.
                unsafe {
                    NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                        &delegate,
                        sel!(popupContentFrameDidChange:),
                        Some(NSViewFrameDidChangeNotification),
                        Some(&view),
                    );
                }
                PopupFrameObservation {
                    view,
                    delegate: delegate.clone(),
                    previous_frame_notifications,
                }
            });

            let retained_action = unsafe { Retained::retain(action as *const _ as *mut _) }
                .ok_or(ExtensionActionRejection::NativeAdmissionFailed)?;
            let active = ActivePopup {
                request,
                context: pending.context,
                action: retained_action,
                webview: popup_webview,
                popover: popover.clone(),
                parent: pending.parent.clone(),
                delegate,
                frame_observation,
                _lease: pending._lease,
            };
            let Ok(mut slot) = self.active.try_borrow_mut() else {
                teardown_active(active, true);
                return Err(ExtensionActionRejection::NativeAdmissionFailed);
            };
            if slot.is_some() {
                drop(slot);
                teardown_active(active, true);
                return Err(ExtensionActionRejection::NativeAdmissionFailed);
            }
            *slot = Some(active);
            drop(slot);
            popover.showRelativeToRect_ofView_preferredEdge(
                anchor,
                &pending.parent,
                if pending.parent.isFlipped() {
                    NSRectEdge::MaxY
                } else {
                    NSRectEdge::MinY
                },
            );
            self.clamp_active_size();
            if !popover.isShown() {
                return Err(ExtensionActionRejection::PopupUnavailable);
            }
            Ok(clamp_popup_size(popover.contentSize()))
        }))
        .unwrap_or(Err(ExtensionActionRejection::NativeAdmissionFailed));

        match presentation {
            Ok(size) => {
                completion.call((std::ptr::null_mut(),));
                self.emit(
                    request.id(),
                    ExtensionActionSettlement::PopupPresented(Size::new(size.width, size.height)),
                );
            }
            Err(reason) => {
                self.close_active();
                complete_rejected(completion, reason);
                self.emit(request.id(), ExtensionActionSettlement::Rejected(reason));
            }
        }
    }

    pub(super) fn cancel_pending(
        &self,
        request: ExtensionActionRequestId,
        reason: ExtensionActionRejection,
    ) -> bool {
        let matches = self
            .pending
            .try_borrow()
            .ok()
            .and_then(|pending| {
                pending
                    .as_ref()
                    .map(|pending| pending.request.id() == request)
            })
            .unwrap_or(false);
        if !matches {
            return false;
        }
        let Some(pending) = self
            .pending
            .try_borrow_mut()
            .ok()
            .and_then(|mut pending| pending.take())
        else {
            return false;
        };
        drop(pending.watchdog);
        self.emit(request, ExtensionActionSettlement::Rejected(reason));
        true
    }

    pub(super) fn timeout(&self, request: ExtensionActionRequestId) -> bool {
        self.cancel_pending(request, ExtensionActionRejection::PopupUnavailable)
    }

    pub(super) fn cancel_context(
        &self,
        context: *const WKWebExtensionContext,
        reason: ExtensionActionRejection,
    ) {
        let pending_request = self.pending.borrow().as_ref().and_then(|pending| {
            std::ptr::eq(Retained::as_ptr(&pending.context), context)
                .then_some(pending.request.id())
        });
        if let Some(request) = pending_request {
            self.cancel_pending(request, reason);
        }
        let closes_active = self
            .active
            .borrow()
            .as_ref()
            .is_some_and(|active| std::ptr::eq(Retained::as_ptr(&active.context), context));
        if closes_active {
            self.close_active();
        }
        let clears_closing = self
            .closing
            .borrow()
            .as_ref()
            .is_some_and(|closing| std::ptr::eq(Retained::as_ptr(&closing.context), context));
        if clears_closing {
            self.closing.borrow_mut().take();
        }
        let closes_options = self
            .options
            .borrow()
            .as_ref()
            .is_some_and(|options| std::ptr::eq(Retained::as_ptr(&options.context), context));
        if closes_options {
            self.close_options();
        }
    }

    /// Popups are tied to the current resident tab. A surface generation that
    /// removes, discards, or deactivates that tab cancels loading or closes the
    /// visible transient surface without creating a view.
    pub(super) fn reconcile_tab(&self, is_current_resident: impl Fn(ItemId) -> bool) {
        let pending = self
            .pending
            .borrow()
            .as_ref()
            .map(|pending| pending.request);
        if pending.is_some_and(|request| !is_current_resident(request.tab())) {
            self.cancel_pending(
                pending.expect("checked pending request").id(),
                ExtensionActionRejection::RuntimeSuperseded,
            );
        }
        let active = self.active.borrow().as_ref().map(|active| active.request);
        if active.is_some_and(|request| !is_current_resident(request.tab())) {
            self.close_active();
        }
    }

    pub(super) fn seal_and_close(&self) {
        self.sealed.set(true);
        if let Some(request) = self
            .pending
            .borrow()
            .as_ref()
            .map(|pending| pending.request.id())
        {
            self.cancel_pending(request, ExtensionActionRejection::ShuttingDown);
        }
        self.close_active();
        self.closing.borrow_mut().take();
        self.close_options();
    }

    fn clamp_active_size(&self) {
        let Some(popover) = self
            .active
            .borrow()
            .as_ref()
            .map(|active| active.popover.clone())
        else {
            return;
        };
        let _ = objc2::exception::catch(AssertUnwindSafe(|| {
            let current = popover.contentSize();
            let clamped = clamp_popup_size(current);
            if current != clamped {
                popover.setContentSize(clamped);
            }
        }));
    }

    fn finish_native_close(self: &Rc<Self>) {
        let Some(active) = self.active.borrow_mut().take() else {
            return;
        };
        let options_requested = popup_requested_options(&active);
        crate::diagnostic!(
            "extensions: native popup closed; options-navigation={options_requested}"
        );
        let transition = finish_popup_for_options_transition(active);
        if options_requested {
            if let Err(reason) = self.present_options_transition(transition) {
                crate::diagnostic!(
                    "extensions: popup options navigation failed with typed reason {reason:?}"
                );
            }
            return;
        }
        let token = self.next_closing_token.get();
        let Some(next) = token.checked_add(1) else {
            return;
        };
        self.next_closing_token.set(next);
        *self.closing.borrow_mut() = Some(ClosingPopup {
            token,
            context: transition.context,
            parent: transition.parent,
            lease: transition.lease,
        });
        if !schedule_closing_expiry(self, token) {
            self.expire_closing(token);
        }
    }

    fn close_active(&self) {
        let Some(active) = self.active.borrow_mut().take() else {
            return;
        };
        teardown_active(active, true);
    }

    fn finish_options_close(&self) {
        let Some(options) = self.options.borrow_mut().take() else {
            return;
        };
        retire_options_native_surface(&options);
        let _keep_delegate_alive_through_close = options.delegate;
    }

    fn schedule_options_process_termination(
        self: &Rc<Self>,
        context: &Retained<WKWebExtensionContext>,
        webview: &Retained<WKWebView>,
    ) {
        let broker = Rc::downgrade(self);
        let context = Weak::from_retained(context);
        let webview = Weak::from_retained(webview);
        let completion: RcBlock<dyn Fn()> = RcBlock::new(move || {
            let (Some(broker), Some(context), Some(webview)) =
                (broker.upgrade(), context.load(), webview.load())
            else {
                return;
            };
            broker.finish_options_process_termination(&context, &webview);
        });
        // SAFETY: dispatch_async copies this heap block onto the main queue.
        // Every captured native owner and the broker are main-thread-only; the
        // deferred turn also prevents the weak navigation delegate from being
        // released while WebKit is still invoking it.
        unsafe {
            dispatch2::DispatchQueue::main().exec_async_with_block(RcBlock::as_ptr(&completion));
        }
    }

    fn finish_options_process_termination(
        &self,
        context: &WKWebExtensionContext,
        webview: &WKWebView,
    ) {
        let matches = self.options.borrow().as_ref().is_some_and(|options| {
            std::ptr::eq(&*options.context, context) && std::ptr::eq(&*options.webview, webview)
        });
        if !matches {
            return;
        }
        let Some(options) = self.options.borrow_mut().take() else {
            return;
        };
        crate::diagnostic!("extensions: options-page web content process terminated");
        retire_options_native_surface(&options);
        let _keep_delegate_alive_through_close = options.delegate;
        options.window.close();
    }

    fn close_options(&self) {
        let Some(options) = self.options.borrow_mut().take() else {
            return;
        };
        retire_options_native_surface(&options);
        let _keep_delegate_alive_through_close = options.delegate;
        options.window.close();
    }

    fn present_options_transition(
        self: &Rc<Self>,
        transition: PopupTransition,
    ) -> Result<(), ExtensionActionRejection> {
        let url = unsafe { transition.context.optionsPageURL() }
            .ok_or(ExtensionActionRejection::PopupUnavailable)?;
        let extension_origin =
            options_extension_origin(&url).ok_or(ExtensionActionRejection::PopupUnavailable)?;
        let configuration = unsafe { transition.context.webViewConfiguration() }
            .ok_or(ExtensionActionRejection::PopupUnavailable)?;
        let mtm = MainThreadMarker::new().ok_or(ExtensionActionRejection::NativeAdmissionFailed)?;
        let parent_window = transition
            .parent
            .window()
            .ok_or(ExtensionActionRejection::PopupUnavailable)?;
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(MAX_EXTENSION_POPUP_WIDTH, MAX_EXTENSION_POPUP_HEIGHT),
        );
        let webview = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &configuration)
        };
        webview.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let delegate = OptionsWindowDelegate::new(
            mtm,
            Rc::downgrade(self),
            Rc::clone(&self.browser_requests),
            transition.context.clone(),
            extension_origin,
        );
        unsafe {
            webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        }
        let request = NSURLRequest::requestWithURL(&url);
        if unsafe { webview.loadRequest(&request) }.is_none() {
            return Err(ExtensionActionRejection::PopupUnavailable);
        }
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe {
            window.setReleasedWhenClosed(false);
            window.setTitle(&NSString::from_str("Extension Settings"));
        }
        window.setContentView(Some(&webview));
        window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // SAFETY: both retained windows belong to this main-thread
        // application and the options owner removes/closes the child before
        // releasing its resource lease.
        unsafe { parent_window.addChildWindow_ordered(&window, NSWindowOrderingMode::Above) };
        window.center();
        window.makeKeyAndOrderFront(None);
        *self.options.borrow_mut() = Some(ActiveOptionsPage {
            context: transition.context,
            window,
            webview,
            delegate,
            _lease: transition.lease,
        });
        Ok(())
    }

    fn expire_closing(&self, token: u64) {
        if self
            .closing
            .borrow()
            .as_ref()
            .is_some_and(|closing| closing.token == token)
        {
            self.closing.borrow_mut().take();
        }
    }

    fn emit(&self, request: ExtensionActionRequestId, settlement: ExtensionActionSettlement) {
        let Some(sink) = self.sink.as_ref() else {
            return;
        };
        let profile = self.profile;
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            sink(EngineEventIngress::global(
                EngineEvent::ExtensionActionSettled {
                    profile,
                    request,
                    settlement,
                },
            ));
        }));
    }
}

fn teardown_active(mut active: ActivePopup, close_popover: bool) {
    drop(active.frame_observation.take());
    let _keep_delegate_alive_through_close = active.delegate;
    let _ = objc2::exception::catch(AssertUnwindSafe(|| {
        active.popover.setDelegate(None);
        unsafe { active.action.closePopup() };
        if close_popover {
            active.popover.close()
        }
    }));
}

fn retire_options_native_surface(options: &ActiveOptionsPage) {
    options.window.setDelegate(None);
    unsafe {
        options.webview.setNavigationDelegate(None);
        options.webview.stopLoading();
    }
    if let Some(parent) = options.window.parentWindow() {
        parent.removeChildWindow(&options.window);
    }
    options.window.setContentView(None);
    options.window.orderOut(None);
}

struct PopupTransition {
    context: Retained<WKWebExtensionContext>,
    parent: Retained<NSView>,
    lease: NativeResourceLease,
}

fn popup_requested_options(active: &ActivePopup) -> bool {
    let requested = unsafe { active.webview.URL() }.and_then(|url| url.absoluteString());
    let options = unsafe { active.context.optionsPageURL() }.and_then(|url| url.absoluteString());
    matches!((requested, options), (Some(requested), Some(options)) if requested.isEqualToString(&options))
}

fn transition_popup_to_options(mut active: ActivePopup) -> PopupTransition {
    drop(active.frame_observation.take());
    let _keep_delegate_alive_through_close = active.delegate;
    let _ = objc2::exception::catch(AssertUnwindSafe(|| {
        active.popover.setDelegate(None);
        unsafe { active.action.closePopup() };
        active.popover.close();
    }));
    PopupTransition {
        context: active.context,
        parent: active.parent,
        lease: active._lease,
    }
}

fn finish_popup_for_options_transition(mut active: ActivePopup) -> PopupTransition {
    drop(active.frame_observation.take());
    active.popover.setDelegate(None);
    PopupTransition {
        context: active.context,
        parent: active.parent,
        lease: active._lease,
    }
}

fn schedule_closing_expiry(broker: &Rc<ActionPopupBroker>, token: u64) -> bool {
    let Ok(when) = dispatch2::DispatchTime::try_from(Duration::from_millis(100)) else {
        return false;
    };
    let broker = Rc::downgrade(broker);
    let callback: RcBlock<dyn Fn()> = RcBlock::new(move || {
        if let Some(broker) = broker.upgrade() {
            broker.expire_closing(token);
        }
    });
    // SAFETY: dispatch_after copies this heap block onto the main queue. All
    // captured native owners are main-thread-only and the callback never runs
    // on another queue.
    unsafe {
        dispatch2::DispatchQueue::exec_after_with_block(
            when,
            dispatch2::DispatchQueue::main(),
            RcBlock::as_ptr(&callback),
        );
    }
    true
}

impl Drop for ActionPopupBroker {
    fn drop(&mut self) {
        self.seal_and_close();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OptionsNavigation {
    Extension,
    External,
}

fn options_extension_origin(url: &NSURL) -> Option<Box<str>> {
    let absolute = url.absoluteString()?;
    if absolute.lengthOfBytesUsingEncoding(objc2_foundation::NSUTF8StringEncoding)
        > MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES
    {
        return None;
    }
    objc2::rc::autoreleasepool(|pool| {
        let parsed = url::Url::parse(unsafe { absolute.to_str(pool) }).ok()?;
        if parsed.scheme() != "webkit-extension"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port().is_some()
        {
            return None;
        }
        let host = parsed.host_str()?;
        Some(format!("webkit-extension://{host}/").into_boxed_str())
    })
}

fn classify_options_navigation(
    extension_origin: &str,
    requested: &str,
    link_activated: bool,
) -> Option<OptionsNavigation> {
    if requested.len() > MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES {
        return None;
    }
    if requested.starts_with(extension_origin) {
        return Some(OptionsNavigation::Extension);
    }
    (link_activated && zephium_core::navigation::is_allowed_str(requested))
        .then_some(OptionsNavigation::External)
}

fn popup_anchor_rect(request: ExtensionActionRequest, parent: &NSView) -> Option<NSRect> {
    clipped_popup_anchor(request.anchor().rect(), parent.bounds(), parent.isFlipped())
}

fn clipped_popup_anchor(anchor: Rect, bounds: NSRect, flipped: bool) -> Option<NSRect> {
    let left = anchor.x.max(0.0);
    let top = anchor.y.max(0.0);
    let right = (anchor.x + anchor.width).min(bounds.size.width);
    let bottom = (anchor.y + anchor.height).min(bounds.size.height);
    if ![
        left,
        top,
        right,
        bottom,
        bounds.size.width,
        bounds.size.height,
    ]
    .iter()
    .all(|value| value.is_finite())
        || right <= left
        || bottom <= top
    {
        return None;
    }
    let y = if flipped {
        bounds.origin.y + top
    } else {
        bounds.origin.y + bounds.size.height - bottom
    };
    Some(NSRect::new(
        NSPoint::new(bounds.origin.x + left, y),
        NSSize::new(right - left, bottom - top),
    ))
}

fn clamp_popup_size(size: NSSize) -> NSSize {
    let width = if size.width.is_finite() {
        size.width
            .clamp(MIN_EXTENSION_POPUP_WIDTH, MAX_EXTENSION_POPUP_WIDTH)
    } else {
        MIN_EXTENSION_POPUP_WIDTH
    };
    let height = if size.height.is_finite() {
        size.height
            .clamp(MIN_EXTENSION_POPUP_HEIGHT, MAX_EXTENSION_POPUP_HEIGHT)
    } else {
        MIN_EXTENSION_POPUP_HEIGHT
    };
    NSSize::new(width, height)
}

fn complete_rejected(
    completion: &DynBlock<dyn Fn(*mut NSError)>,
    reason: ExtensionActionRejection,
) {
    let error = popup_error(reason);
    completion.call((Retained::as_ptr(&error).cast_mut(),));
}

fn popup_error(reason: ExtensionActionRejection) -> Retained<NSError> {
    let code = match reason {
        ExtensionActionRejection::InvalidRequest => 1,
        ExtensionActionRejection::RuntimeUnavailable => 2,
        ExtensionActionRejection::RuntimeSuperseded => 3,
        ExtensionActionRejection::TabUnavailable => 4,
        ExtensionActionRejection::TabDiscarded => 5,
        ExtensionActionRejection::ActionUnavailable => 6,
        ExtensionActionRejection::ActionDisabled => 7,
        ExtensionActionRejection::CapacityExceeded => 8,
        ExtensionActionRejection::PopupUnavailable => 9,
        ExtensionActionRejection::PopupCapacityExceeded => 10,
        ExtensionActionRejection::NativeAdmissionFailed => 11,
        ExtensionActionRejection::ShuttingDown => 12,
        ExtensionActionRejection::UnsupportedPlatform => 13,
    };
    let domain = NSString::from_str(POPUP_ERROR_DOMAIN);
    // SAFETY: the domain and code are bounded constants. No extension or page
    // content is reflected into the native error object.
    unsafe { NSError::errorWithDomain_code_userInfo(&domain, code, None) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_size_is_finite_and_bounded() {
        assert_eq!(
            clamp_popup_size(NSSize::new(f64::NAN, f64::INFINITY)),
            NSSize::new(MIN_EXTENSION_POPUP_WIDTH, MIN_EXTENSION_POPUP_HEIGHT)
        );
        assert_eq!(
            clamp_popup_size(NSSize::new(10_000.0, 10_000.0)),
            NSSize::new(MAX_EXTENSION_POPUP_WIDTH, MAX_EXTENSION_POPUP_HEIGHT)
        );
    }

    #[test]
    fn popup_anchor_is_clipped_and_converted_without_absolute_coordinates() {
        let bounds = NSRect::new(NSPoint::new(5.0, 7.0), NSSize::new(500.0, 400.0));
        let anchor = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(
            clipped_popup_anchor(anchor, bounds, true),
            Some(NSRect::new(
                NSPoint::new(15.0, 27.0),
                NSSize::new(30.0, 40.0),
            ))
        );
        assert_eq!(
            clipped_popup_anchor(anchor, bounds, false),
            Some(NSRect::new(
                NSPoint::new(15.0, 347.0),
                NSSize::new(30.0, 40.0),
            ))
        );
        assert!(clipped_popup_anchor(Rect::new(600.0, 20.0, 10.0, 10.0), bounds, true).is_none());
    }

    #[test]
    fn native_popup_errors_are_closed_and_content_free() {
        let error = popup_error(ExtensionActionRejection::PopupCapacityExceeded);
        assert_eq!(error.code(), 10);
        assert_eq!(error.domain().to_string(), POPUP_ERROR_DOMAIN);
        assert_eq!(error.userInfo().count(), 0);
    }

    #[test]
    fn options_navigation_is_origin_exact_and_external_links_require_a_user_gesture() {
        let origin = "webkit-extension://00000000-0000-0000-0000-000000000001/";
        assert_eq!(
            classify_options_navigation(
                origin,
                "webkit-extension://00000000-0000-0000-0000-000000000001/pages/options.html#x",
                false,
            ),
            Some(OptionsNavigation::Extension)
        );
        assert_eq!(
            classify_options_navigation(origin, "https://example.com/docs", true),
            Some(OptionsNavigation::External)
        );
        for (url, user_gesture) in [
            ("https://example.com/programmatic", false),
            ("javascript:alert(1)", true),
            ("file:///tmp/secret", true),
            (
                "webkit-extension://00000000-0000-0000-0000-000000000002/pages/options.html",
                true,
            ),
        ] {
            assert_eq!(classify_options_navigation(origin, url, user_gesture), None);
        }
        assert_eq!(
            classify_options_navigation(
                origin,
                &"https://example.com/".repeat(
                    MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES / "https://example.com/".len() + 1
                ),
                true,
            ),
            None
        );
    }

    #[test]
    fn options_renderer_termination_is_deferred_and_identity_exact() {
        let source = include_str!("action_popup.rs");
        let callback = source
            .find("fn web_content_process_did_terminate")
            .expect("options navigation delegate termination callback");
        let deferred = source
            .find("fn schedule_options_process_termination")
            .expect("deferred options teardown");
        let exact = source
            .find("fn finish_options_process_termination")
            .expect("exact options teardown");
        assert!(callback < deferred && deferred < exact);
        let body = &source[exact
            ..source
                .get(exact..)
                .and_then(|tail| tail.find("fn close_options"))
                .map(|end| exact + end)
                .expect("options teardown end")];
        assert!(body.contains("std::ptr::eq(&*options.context, context)"));
        assert!(body.contains("std::ptr::eq(&*options.webview, webview)"));
        assert!(body.contains("retire_options_native_surface(&options)"));
    }
}
