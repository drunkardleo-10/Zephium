//! Exact, resource-budgeted presentation of native macOS extension popups.
//!
//! WebKit owns popup document creation and execution. Zephium owns the user
//! gesture, profile/controller/context/tab join, AppKit anchor, size policy,
//! terminal result and the sole process-wide popup resource lease. A delegate
//! callback without a matching trusted request is rejected, so extension code
//! cannot surface privileged browser chrome on its own.

use std::cell::{Cell, RefCell};
#[cfg(feature = "native-extension-lab-diagnostics")]
use std::ffi::OsStr;
use std::panic::AssertUnwindSafe;
use std::rc::{Rc, Weak as RcWeak};
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSPopover, NSPopoverBehavior, NSPopoverDelegate, NSView, NSViewFrameDidChangeNotification,
};
use objc2_foundation::{
    MainThreadMarker, NSError, NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint,
    NSRect, NSRectEdge, NSSize, NSString,
};
use objc2_web_kit::{
    WKWebExtensionAction, WKWebExtensionContext, WKWebExtensionController, WKWebExtensionTab,
    WKWebView,
};
use zephium_core::extensions::{
    ExtensionActionRejection, ExtensionActionRequest, ExtensionActionRequestId,
    ExtensionActionSettlement, MAX_EXTENSION_POPUP_HEIGHT, MAX_EXTENSION_POPUP_WIDTH,
    MIN_EXTENSION_POPUP_HEIGHT, MIN_EXTENSION_POPUP_WIDTH,
};
use zephium_core::geometry::{Rect, Size};
use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::EngineEvent;

use crate::host::NativeResourceLease;
use crate::{EngineEventIngress, EngineEventIngressSink};

use super::browser_request_broker::BrowserRequestBroker;

const POPUP_LOAD_TIMEOUT: Duration = Duration::from_secs(10);
const POPUP_ERROR_DOMAIN: &str = "app.zephium.extension-action";

fn same_native_target(
    actual: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
    expected: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
) -> bool {
    match (actual, expected) {
        (Some(actual), Some(expected)) => std::ptr::eq(actual, expected),
        (None, None) => true,
        _ => false,
    }
}

fn popup_target_current(
    current: Option<(ItemId, bool)>,
    request: ExtensionActionRequest,
    tab_specific: bool,
) -> bool {
    current == Some((request.tab(), tab_specific))
}
#[cfg(feature = "native-extension-lab-diagnostics")]
const LAB_RETAIN_POPUP_ENV: &str = "ZEPHIUM_EXTENSION_LAB_RETAIN_POPUP";

struct PendingPopup {
    request: ExtensionActionRequest,
    controller: Retained<WKWebExtensionController>,
    context: Retained<WKWebExtensionContext>,
    tab: Option<Retained<ProtocolObject<dyn WKWebExtensionTab>>>,
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
    initial_options_match: Option<bool>,
    _lease: NativeResourceLease,
}

struct ClosingPopup {
    token: u64,
    context: Retained<WKWebExtensionContext>,
    _parent: Retained<NSView>,
    _lease: NativeResourceLease,
}

struct PopupTransition {
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
                broker.finish_native_close(self);
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

/// Main-thread owner for one profile controller's pending or visible popup.
pub(super) struct ActionPopupBroker {
    profile: ProfileId,
    sink: Option<EngineEventIngressSink>,
    browser_requests: Rc<BrowserRequestBroker>,
    pending: RefCell<Option<PendingPopup>>,
    active: RefCell<Option<ActivePopup>>,
    closing: RefCell<Option<ClosingPopup>>,
    next_closing_token: Cell<u64>,
    size_clamp_active: Cell<bool>,
    sealed: Cell<bool>,
}

/// Prevents AppKit's synchronous frame-change notification from recursively
/// re-entering `NSPopover::setContentSize`. The notification can fire while
/// Auto Layout is still applying the requested content size; entering the
/// setter again from that stack overflows before Rust regains control.
struct PopupSizeClampGuard<'a> {
    active: &'a Cell<bool>,
}

impl<'a> PopupSizeClampGuard<'a> {
    fn enter(active: &'a Cell<bool>) -> Option<Self> {
        if active.replace(true) {
            // Constructing then dropping a refused guard would clear the
            // outer operation's flag. Only the admitted owner may release it.
            None
        } else {
            Some(Self { active })
        }
    }
}

impl Drop for PopupSizeClampGuard<'_> {
    fn drop(&mut self) {
        self.active.set(false);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ActionPopupPreparation {
    Present,
    Dismissed,
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
            size_clamp_active: Cell::new(false),
            sealed: Cell::new(false),
        })
    }

    /// Applies native toolbar-toggle semantics before a new popup lease is
    /// acquired. The same authenticated target toggles closed; a different
    /// target replaces it after releasing the old popup. A fresh toolbar
    /// gesture also supersedes the post-close options-navigation grace period.
    pub(super) fn prepare_toggle(
        &self,
        request: ExtensionActionRequest,
        context: &WKWebExtensionContext,
        tab: Option<&Retained<ProtocolObject<dyn WKWebExtensionTab>>>,
    ) -> Result<ActionPopupPreparation, ExtensionActionRejection> {
        if request.runtime().profile() != self.profile {
            return Err(ExtensionActionRejection::InvalidRequest);
        }
        if self.sealed.get() {
            return Err(ExtensionActionRejection::ShuttingDown);
        }
        let pending_target = self
            .pending
            .try_borrow()
            .map_err(|_| ExtensionActionRejection::NativeAdmissionFailed)?
            .as_ref()
            .map(|pending| {
                (
                    pending.request,
                    std::ptr::eq(&*pending.context, context)
                        && same_native_target(pending.tab.as_deref(), tab.map(|tab| &**tab)),
                )
            });
        if let Some((pending, native_target_matches)) = pending_target {
            let disposition = popup_switch_disposition(pending, request, native_target_matches)?;
            if !self.settle_pending(pending.id(), ExtensionActionSettlement::PopupDismissed) {
                return Err(ExtensionActionRejection::NativeAdmissionFailed);
            }
            return Ok(disposition);
        }

        let active_target = self
            .active
            .try_borrow()
            .map_err(|_| ExtensionActionRejection::NativeAdmissionFailed)?
            .as_ref()
            .map(|active| {
                let associated_tab = unsafe { active.action.associatedTab() };
                (
                    active.request,
                    std::ptr::eq(&*active.context, context)
                        && same_native_target(associated_tab.as_deref(), tab.map(|tab| &**tab)),
                )
            });
        if let Some((active, native_target_matches)) = active_target {
            let disposition = popup_switch_disposition(active, request, native_target_matches)?;
            self.close_active_for_replacement()?;
            return Ok(disposition);
        }

        self.closing
            .try_borrow_mut()
            .map_err(|_| ExtensionActionRejection::NativeAdmissionFailed)?
            .take();

        Ok(ActionPopupPreparation::Present)
    }

    /// Reserves the exact native callback expected from `performActionForTab:`.
    /// The host acquires the global resource lease and revalidates the action
    /// before this call. Document backgrounds receive a gesture-bound warm-up;
    /// service-worker backgrounds use normal native action dispatch.
    pub(super) fn begin(
        self: &Rc<Self>,
        request: ExtensionActionRequest,
        controller: Retained<WKWebExtensionController>,
        owner: super::native_runtime::MacosNativeActionPopupOwner,
        tab: Option<Retained<ProtocolObject<dyn WKWebExtensionTab>>>,
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
        let warm_document = owner.requires_popup_background_warmup();
        let context = owner.into_context();
        *self.pending.borrow_mut() = Some(PendingPopup {
            request,
            controller,
            context,
            tab,
            parent,
            _lease: lease,
            watchdog,
        });
        if warm_document {
            self.warm_background_and_perform(request_id);
        } else {
            self.perform_pending_action(request_id);
        }
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
                super::trace_action_qa_stage("background-warmup-error");
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
            super::trace_action_qa_stage("background-warmup-exception");
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
            context.performActionForTab(tab.as_deref());
        }))
        .is_err()
        {
            self.cancel_pending(request, ExtensionActionRejection::NativeAdmissionFailed);
        }
    }

    /// Requests the exact options document as a browser-owned extension tab.
    /// The tab completion settles only after Shell and the native guest host
    /// admit the actual tab. The popup is closed before the tab request.
    pub(super) fn open_options_page(
        self: &Rc<Self>,
        controller: &WKWebExtensionController,
        context: &WKWebExtensionContext,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        if self.sealed.get() {
            complete_rejected(completion, ExtensionActionRejection::ShuttingDown);
            return;
        }
        if !self.browser_requests.accepts(Some(controller), context) {
            complete_rejected(completion, ExtensionActionRejection::InvalidRequest);
            return;
        }
        let Some(context) = (unsafe { Retained::retain(context as *const _ as *mut _) }) else {
            complete_rejected(completion, ExtensionActionRejection::InvalidRequest);
            return;
        };
        if let Err(reason) = self.close_popup_for_options(&context) {
            complete_rejected(completion, reason);
            return;
        }
        let unit = completion.copy();
        let callback: RcBlock<dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError)> =
            RcBlock::new(
                move |tab: *mut ProtocolObject<dyn WKWebExtensionTab>, error: *mut NSError| {
                    if tab.is_null() && error.is_null() {
                        let error = popup_error(ExtensionActionRejection::NativeAdmissionFailed);
                        unit.call((Retained::as_ptr(&error).cast_mut(),));
                    } else {
                        unit.call((error,));
                    }
                },
            );
        if let Err(reason) = self.request_options_tab(context, &callback) {
            complete_rejected(completion, reason);
        }
    }

    pub(super) fn open_options_page_authorized(
        self: &Rc<Self>,
        context: &WKWebExtensionContext,
        reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
    ) -> bool {
        let Some(context) = (unsafe { Retained::retain(context as *const _ as *mut _) }) else {
            return false;
        };
        if self.close_popup_for_options(&context).is_err() {
            return false;
        }
        let reply = reply.copy();
        let callback: RcBlock<dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError)> =
            RcBlock::new(
                move |tab: *mut ProtocolObject<dyn WKWebExtensionTab>, error: *mut NSError| {
                    if !error.is_null() {
                        reply.call((std::ptr::null_mut(), error));
                    } else if tab.is_null() {
                        let error = popup_error(ExtensionActionRejection::NativeAdmissionFailed);
                        reply.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
                    } else {
                        let result = NSString::from_str("{\"v\":1,\"opened\":true}");
                        reply.call((
                            Retained::as_ptr(&result).cast_mut().cast(),
                            std::ptr::null_mut(),
                        ));
                    }
                },
            );
        self.request_options_tab(context, &callback).is_ok()
    }

    /// Trusted browser chrome requests the same browser-owned options tab.
    /// Its old popup-view lease is released before the tab request; the tab
    /// has its own exact browser resource admission.
    pub(super) fn open_options_page_from_browser(
        self: &Rc<Self>,
        context: Retained<WKWebExtensionContext>,
        completion: &DynBlock<dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError)>,
    ) -> Result<(), ExtensionActionRejection> {
        self.close_popup_for_options(&context)?;
        self.request_options_tab(context, completion)
    }

    fn close_popup_for_options(
        &self,
        context: &WKWebExtensionContext,
    ) -> Result<(), ExtensionActionRejection> {
        if self
            .active
            .borrow()
            .as_ref()
            .is_some_and(|active| std::ptr::eq(&*active.context, context))
        {
            self.close_active_for_replacement()?;
        }
        if self
            .closing
            .borrow()
            .as_ref()
            .is_some_and(|closing| std::ptr::eq(&*closing.context, context))
        {
            self.closing.borrow_mut().take();
        }
        Ok(())
    }

    fn request_options_tab(
        &self,
        context: Retained<WKWebExtensionContext>,
        completion: &DynBlock<dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError)>,
    ) -> Result<(), ExtensionActionRejection> {
        if self.sealed.get() || !self.browser_requests.accepts(None, &context) {
            return Err(ExtensionActionRejection::InvalidRequest);
        }
        let url = unsafe { context.optionsPageURL() }
            .ok_or(ExtensionActionRejection::PopupUnavailable)?;
        self.browser_requests
            .begin_extension_page(context, url, completion);
        Ok(())
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
                    && same_native_target(
                        unsafe { action.associatedTab() }.as_deref(),
                        expected_tab.as_deref(),
                    )
                    && unsafe { controller.extensionContexts() }.containsObject(context)
            })
        }))
        .unwrap_or(false);
        if !matches || self.sealed.get() {
            super::trace_action_qa_stage("popup-delegate-target-mismatch");
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
                super::trace_action_qa_stage("popup-action-no-longer-presented");
                return Err(ExtensionActionRejection::RuntimeSuperseded);
            }
            let popover = unsafe { action.popupPopover() }.ok_or_else(|| {
                super::trace_action_qa_stage("popup-popover-missing");
                ExtensionActionRejection::PopupUnavailable
            })?;
            let popup_webview = unsafe { action.popupWebView() }.ok_or_else(|| {
                super::trace_action_qa_stage("popup-webview-missing");
                ExtensionActionRejection::PopupUnavailable
            })?;
            let mtm =
                MainThreadMarker::new().ok_or(ExtensionActionRejection::NativeAdmissionFailed)?;
            let anchor = popup_anchor_rect(request, &pending.parent)
                .ok_or(ExtensionActionRejection::InvalidRequest)?;
            let size = clamp_popup_size(popover.contentSize());
            popover.setContentSize(size);
            // Product and ordinary lab runs use native transient dismissal.
            // A diagnostic run may explicitly retain the popup while Safari
            // attaches to this exact WKWebView; merely compiling diagnostics
            // must not change the user-facing close behavior.
            #[cfg(feature = "native-extension-lab-diagnostics")]
            let retain_for_inspection =
                std::env::var_os(LAB_RETAIN_POPUP_ENV).as_deref() == Some(OsStr::new("1"));
            #[cfg(not(feature = "native-extension-lab-diagnostics"))]
            let retain_for_inspection = false;
            popover.setBehavior(popup_behavior(retain_for_inspection));
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
                initial_options_match: popup_matches_options(&popup_webview, &pending.context),
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
                super::trace_action_qa_stage("popup-not-shown");
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
        self.settle_pending(request, ExtensionActionSettlement::Rejected(reason))
    }

    fn settle_pending(
        &self,
        request: ExtensionActionRequestId,
        settlement: ExtensionActionSettlement,
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
        self.emit(request, settlement);
        true
    }

    pub(super) fn timeout(&self, request: ExtensionActionRequestId) -> bool {
        super::trace_action_qa_stage("popup-pending-timeout");
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
    }

    /// Keep a popup only while its logical active tab and native action target
    /// still match. A default popup has no tab-specific page authority.
    pub(super) fn reconcile_tab(&self, current: Option<(ItemId, bool)>) {
        let pending = self
            .pending
            .borrow()
            .as_ref()
            .map(|pending| (pending.request, pending.tab.is_some()));
        if pending
            .is_some_and(|(request, resident)| !popup_target_current(current, request, resident))
        {
            super::trace_action_qa_stage("pending-target-changed");
            self.cancel_pending(
                pending.expect("checked pending request").0.id(),
                ExtensionActionRejection::RuntimeSuperseded,
            );
        }
        let active = self.active.borrow().as_ref().map(|active| {
            (
                active.request,
                unsafe { active.action.associatedTab() }.is_some(),
            )
        });
        if active
            .is_some_and(|(request, resident)| !popup_target_current(current, request, resident))
        {
            super::trace_action_qa_stage("active-target-changed");
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
    }

    fn clamp_active_size(&self) {
        let Some(_guard) = PopupSizeClampGuard::enter(&self.size_clamp_active) else {
            return;
        };
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

    fn finish_native_close(self: &Rc<Self>, delegate: &ActionPopoverDelegate) {
        // A delayed notification from the previous popup must not retire the
        // replacement opened by a newer toolbar gesture.
        if !self
            .active
            .borrow()
            .as_ref()
            .is_some_and(|active| std::ptr::eq(&*active.delegate, delegate))
        {
            return;
        }
        let Some(active) = self.active.borrow_mut().take() else {
            return;
        };
        let options_requested = popup_requested_options(&active);
        crate::diagnostic!(
            "extensions: native popup closed; options-navigation={options_requested}"
        );
        let transition = finish_popup_for_options_transition(active);
        if options_requested {
            let callback: RcBlock<
                dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError),
            > = RcBlock::new(|_, _| {});
            if let Err(reason) = self.request_options_tab(transition.context, &callback) {
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
            _parent: transition.parent,
            _lease: transition.lease,
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

    fn close_active_for_replacement(&self) -> Result<(), ExtensionActionRejection> {
        let Some(mut active) = self
            .active
            .try_borrow_mut()
            .map_err(|_| ExtensionActionRejection::NativeAdmissionFailed)?
            .take()
        else {
            return Ok(());
        };
        let closed = objc2::exception::catch(AssertUnwindSafe(|| {
            active.popover.setDelegate(None);
            active.popover.setAnimates(false);
            unsafe { active.action.closePopup() };
            active.popover.close();
        }));
        if closed.is_err() {
            crate::diagnostic!("extensions: native popup replacement close raised an exception");
            // Keep the existing resource lease if native closure is uncertain.
            // A replacement must never create a second live popup.
            self.active.borrow_mut().replace(active);
            return Err(ExtensionActionRejection::NativeAdmissionFailed);
        }
        drop(active.frame_observation.take());
        drop(active);
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

fn popup_switch_disposition(
    previous: ExtensionActionRequest,
    next: ExtensionActionRequest,
    native_target_matches: bool,
) -> Result<ActionPopupPreparation, ExtensionActionRejection> {
    if previous.runtime().profile() != next.runtime().profile() {
        return Err(ExtensionActionRejection::InvalidRequest);
    }
    if previous.runtime() == next.runtime() && previous.tab() == next.tab() && native_target_matches
    {
        Ok(ActionPopupPreparation::Dismissed)
    } else {
        Ok(ActionPopupPreparation::Present)
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

fn popup_requested_options(active: &ActivePopup) -> bool {
    popup_navigated_to_options(
        active.initial_options_match,
        popup_matches_options(&active.webview, &active.context),
    )
}

fn popup_matches_options(webview: &WKWebView, context: &WKWebExtensionContext) -> Option<bool> {
    let requested = unsafe { webview.URL() }?.absoluteString()?;
    let options = unsafe { context.optionsPageURL() }?.absoluteString()?;
    Some(requested.isEqualToString(&options))
}

fn popup_navigated_to_options(initial: Option<bool>, current: Option<bool>) -> bool {
    // Sharing one document for popup and options is legal. Dismissing that
    // popup is not an options request. Infer a transition only from a known
    // different initial page; explicit native openOptionsPage callbacks keep
    // their existing path, including when the initial URL was unavailable.
    initial == Some(false) && current == Some(true)
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

const fn popup_behavior(retain_for_inspection: bool) -> NSPopoverBehavior {
    if retain_for_inspection {
        NSPopoverBehavior::ApplicationDefined
    } else {
        NSPopoverBehavior::Transient
    }
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
    #[test]
    fn dismissing_a_shared_popup_options_document_does_not_open_settings() {
        assert!(!super::popup_navigated_to_options(Some(true), Some(true)));
        assert!(!super::popup_navigated_to_options(None, Some(true)));
        assert!(!super::popup_navigated_to_options(Some(false), None));
        assert!(!super::popup_navigated_to_options(Some(false), Some(false)));
        assert!(super::popup_navigated_to_options(Some(false), Some(true)));
    }
    use super::*;

    fn switch_request(
        profile: u128,
        extension: u128,
        tab: u128,
        request: u64,
    ) -> ExtensionActionRequest {
        use zephium_core::extensions::{
            ExtensionActionRevision, ExtensionBrowserSurfaceGeneration, ExtensionPopupAnchor,
            ExtensionRuntimeGeneration, ExtensionRuntimeInstance,
        };
        ExtensionActionRequest::new(
            ExtensionActionRequestId::new(request).unwrap(),
            ExtensionRuntimeInstance::new(
                ProfileId::from(profile),
                zephium_core::ids::ExtensionInstallId::from(extension),
                ExtensionRuntimeGeneration::INITIAL,
            ),
            ItemId::from(tab),
            ExtensionBrowserSurfaceGeneration::INITIAL,
            ExtensionActionRevision::INITIAL,
            ExtensionPopupAnchor::new(Rect::new(12.0, 12.0, 24.0, 24.0)).unwrap(),
        )
    }

    #[test]
    fn toolbar_gestures_toggle_the_same_target_and_replace_different_targets() {
        let previous = switch_request(1, 1, 1, 1);
        assert_eq!(
            popup_switch_disposition(previous, switch_request(1, 1, 1, 2), true),
            Ok(ActionPopupPreparation::Dismissed)
        );
        assert_eq!(
            popup_switch_disposition(previous, switch_request(1, 2, 1, 2), false),
            Ok(ActionPopupPreparation::Present)
        );
        assert_eq!(
            popup_switch_disposition(previous, switch_request(1, 1, 2, 2), false),
            Ok(ActionPopupPreparation::Present)
        );
        assert_eq!(
            popup_switch_disposition(previous, switch_request(1, 1, 1, 2), false),
            Ok(ActionPopupPreparation::Present)
        );
        assert_eq!(
            popup_switch_disposition(previous, switch_request(2, 2, 1, 2), false),
            Err(ExtensionActionRejection::InvalidRequest)
        );
    }

    #[test]
    fn default_popup_stays_on_nonresident_tab_and_retires_when_view_returns() {
        let request = switch_request(1, 1, 1, 1);
        let tab = request.tab();
        assert!(popup_target_current(Some((tab, false)), request, false));
        assert!(!popup_target_current(Some((tab, true)), request, false));
        assert!(!popup_target_current(Some((tab, false)), request, true));
        assert!(!popup_target_current(None, request, false));
    }

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
    fn popup_size_clamp_rejects_synchronous_reentry_and_reopens_after_return() {
        let active = Cell::new(false);
        let outer = PopupSizeClampGuard::enter(&active).expect("first clamp enters");
        for _ in 0..3 {
            assert!(PopupSizeClampGuard::enter(&active).is_none());
            assert!(
                active.get(),
                "a refused callback must not release the outer guard"
            );
        }
        drop(outer);
        assert!(!active.get());
        assert!(PopupSizeClampGuard::enter(&active).is_some());
    }

    #[test]
    fn native_popup_dismissal_is_default_and_inspector_retention_is_explicit() {
        assert_eq!(popup_behavior(false), NSPopoverBehavior::Transient);
        assert_eq!(popup_behavior(true), NSPopoverBehavior::ApplicationDefined);
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
}
