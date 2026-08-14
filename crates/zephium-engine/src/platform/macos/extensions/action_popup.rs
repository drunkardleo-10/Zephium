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

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::NSViewFrameDidChangeNotification;
use objc2_app_kit::{NSPopover, NSPopoverBehavior, NSPopoverDelegate, NSView};
use objc2_foundation::{
    MainThreadMarker, NSError, NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint,
    NSRect, NSRectEdge, NSSize, NSString,
};
use objc2_web_kit::{
    WKWebExtensionAction, WKWebExtensionContext, WKWebExtensionController, WKWebExtensionTab,
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
    popover: Retained<NSPopover>,
    delegate: Retained<ActionPopoverDelegate>,
    frame_observation: Option<PopupFrameObservation>,
    _lease: NativeResourceLease,
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

/// Main-thread owner for one profile controller's pending or visible popup.
pub(super) struct ActionPopupBroker {
    profile: ProfileId,
    sink: Option<EngineEventIngressSink>,
    pending: RefCell<Option<PendingPopup>>,
    active: RefCell<Option<ActivePopup>>,
    sealed: Cell<bool>,
}

impl ActionPopupBroker {
    pub(super) fn new(profile: ProfileId, sink: Option<EngineEventIngressSink>) -> Rc<Self> {
        Rc::new(Self {
            profile,
            sink,
            pending: RefCell::new(None),
            active: RefCell::new(None),
            sealed: Cell::new(false),
        })
    }

    /// Reserves the exact native callback expected from `performActionForTab:`.
    /// The host acquires the global resource lease before this call.
    pub(super) fn begin(
        &self,
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
            let mtm =
                MainThreadMarker::new().ok_or(ExtensionActionRejection::NativeAdmissionFailed)?;
            let anchor = popup_anchor_rect(request, &pending.parent)
                .ok_or(ExtensionActionRejection::InvalidRequest)?;
            let size = clamp_popup_size(popover.contentSize());
            popover.setContentSize(size);
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
                popover: popover.clone(),
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

    fn finish_native_close(&self) {
        let Some(active) = self.active.borrow_mut().take() else {
            return;
        };
        teardown_active(active, false);
    }

    fn close_active(&self) {
        let Some(active) = self.active.borrow_mut().take() else {
            return;
        };
        teardown_active(active, true);
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
}
